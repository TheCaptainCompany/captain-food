//! THE way a staged event reaches `domain_events` — and, because of that, the one place that
//! decides when the `orders_placed_total` BAM counter moves (#456, #588).
//!
//! **Why this is a module and not three functions in `mailbox/mod.rs` (#597).** The WHEN of the
//! counter used to be a decision each delivery route took for itself: `record_order_placements`
//! was called by ONE route, because that route was the only place an `OrderPlaced` could be
//! staged. Routing the Order birth onto its own lane moved the append to a DIFFERENT route, and
//! the counter would have gone silently to zero — a dashboard reading "no stranger has paid us"
//! while checkout worked normally, which is exactly the failure the counter exists to detect.
//!
//! #588 fixed that structurally by moving the decision into [`flush_staged_in_tx`], and guarded it
//! with a source scan over `handler.rs`. The scan could not hold: the function was `pub`, so a
//! call from `pm_delivery.rs`, from a new route module, or from `server` passed it untouched — and
//! its stated justification ("the mistake is an ABSENT call, which types cannot see")
//! mis-described it, because it fired on a PRESENT call, which privacy handles exactly.
//!
//! So the emit decision is now **private to this module**: [`record_order_placements`] is
//! unspellable from any delivery route, in this crate or any other, and the only caller it can
//! ever have is [`flush_staged_in_tx`] below. The scan is deleted — deleting a gate the compiler
//! subsumes is the correct outcome (ADR-20260803-234035, compiler first, a check is the fallback).

use application::ports::{version_conflict, Actor};
use application::staging::StagedAppend;
use domain::generated::events::DomainEvent;
use domain::shared::errors::DomainError;
use sqlx::{Postgres, Transaction};

/// True iff any staged append in this delivery carries an `OrderPlaced`. This is the TRANSITIVE
/// output of the place-order guard (`PlaceOrderHooks::should_deliver_order_placed` = the order
/// fold is `None`, place_order.rs): a re-delivery or partial-reaction replay finds the guard
/// false, stages NO `OrderPlaced`, and this stays false. The BAM counter keys on THIS — never on
/// `Outcome::Completed`, which the handler returns even on a replay that appended nothing and
/// would double-count a monotonic counter into a permanent lie.
fn staged_contains_order_placed(staged: &[StagedAppend]) -> bool {
    staged
        .iter()
        .flat_map(|append| append.events.iter())
        .any(|event| matches!(event, DomainEvent::OrderPlaced(_)))
}

/// Emit `orders_placed_total{status="PLACED"}` once per delivery that actually placed an order —
/// the "a stranger paid us" BAM signal (#456 "Emit orders_placed_total so the un-told-order alarm
/// can fire"). Keying on the staged set rather than the delivery outcome is what makes it
/// replay-safe — see [`staged_contains_order_placed`]. This is the infra/framework boundary the
/// c4-l3 `instrumented` rule allows the telemetry SDK to live at; the domain and application
/// layers stay SDK-free.
///
/// **PRIVATE, and that is the whole point (#597)**: WHEN this fires is not a route's business, and
/// a route can no longer make it its business. The only call site the compiler permits is
/// [`flush_staged_in_tx`] below. See the module docs for the zeroing this shape prevents.
fn record_order_placements(staged: &[StagedAppend]) {
    if staged_contains_order_placed(staged) {
        telemetry::meters::place_order::placed("PLACED");
    }
}

/// The #456 spy seam, and the ONLY reason anything outside this module can reach the emit
/// decision: `tests/orders_placed_metric.rs` proves the counter FIRES, and it must run in its own
/// process (the meter provider binds once per process), so it links this crate as an integration
/// test binary and cannot see `cfg(test)`.
///
/// A cfg-gated delegating seam rather than a `pub` on [`record_order_placements`] itself, because
/// a `pub` in a private module is still spellable by every sibling route module in this crate —
/// which is precisely the hole #597 closed. This exists only under `test-fixtures`, a feature the
/// `test_fixtures_feature_never_reaches_a_release_artifact` guard keeps out of every release
/// graph, so in a shipped build there is no path to the emit decision at all.
#[cfg(any(test, feature = "test-fixtures"))]
pub fn record_order_placements_spy(staged: &[StagedAppend]) {
    record_order_placements(staged);
}

/// Record `order_birth_lag_ms{routed}` — the HANDOVER a routed birth introduces (#588,
/// ADR-20260816-040239): the mailbox row's `received_at` IS the instant the saga's enqueue
/// committed, so `now - received_at` at the Order lane's delivery is enqueue → `Recorded` with no
/// extra clock and nothing for the domain to know about. Call AFTER the flush, and only when the
/// delivery actually appended the birth (the staged set holds `OrderPlaced` on the `Recorded` arm
/// only) — a redelivery that absorbed an already-recorded birth measured nothing and must not
/// report a lag of "however long ago the first delivery was".
///
/// Operational, not BAM (ADR-20260811-014129): this is lane depth and worker liveness, and it has
/// to keep working when Postgres is degraded.
///
/// Lives here, with the flush, because it shares [`staged_contains_order_placed`] — the predicate
/// stays private to the one module that is allowed to draw conclusions from a staged set.
pub fn record_order_birth_lag(
    message: &actor_runtime::InboundMessage,
    staged: &[StagedAppend],
    routed: bool,
) {
    if staged_contains_order_placed(staged) {
        let lag_ms = (chrono::Utc::now() - message.received_at).num_milliseconds().max(0) as f64;
        telemetry::meters::place_order::birth_lag(routed, lag_ms);
    }
}

/// Flush staged appends INTO the completion transaction — the same insert the pool-backed
/// [`crate::persistence::PgEventStore`] performs, minus the commit (the runtime commits, fenced).
/// Optimistic concurrency is asserted HERE, at commit time: a `UNIQUE (stream_name, version)`
/// clash maps to the canonical version conflict and rolls the whole delivery back.
pub async fn flush_staged_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    staged: &[StagedAppend],
) -> Result<(), DomainError> {
    for append in staged {
        let split: Vec<(String, serde_json::Value)> = append
            .events
            .iter()
            .map(split_event_tagged)
            .collect::<Result<_, _>>()?;
        for (index, (event_type, payload)) in split.into_iter().enumerate() {
            let version = append.expected_version + index as i64 + 1;
            let insert = sqlx::query(
                "INSERT INTO domain_events \
                 (id, stream_name, version, user_id, user_type, correlation_id, cause_id, \
                  event_type, payload, metadata, occurred_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULL, now())",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(&append.stream_name)
            .bind(i32::try_from(version).map_err(|e| DomainError::Repository(e.to_string()))?)
            .bind(actor_user_id(&append.actor))
            .bind(append.actor.user_type.clone())
            .bind(append.actor.correlation_id)
            .bind(append.actor.cause_id)
            .bind(&event_type)
            .bind(payload)
            .execute(&mut **tx)
            .await;
            if let Err(e) = insert {
                if matches!(&e, sqlx::Error::Database(db) if db.is_unique_violation()) {
                    return Err(version_conflict(&append.stream_name, append.expected_version));
                }
                return Err(DomainError::Repository(e.to_string()));
            }
        }
    }
    // The #456 BAM counter, decided HERE and nowhere else (#588): this function is the only way a
    // staged event reaches `domain_events`, so whichever delivery route appends the birth counts
    // it — and exactly one route ever can, so the routing flag cannot double-count. Emitted after
    // the inserts, so the count only moves once the append is in the completion transaction
    // (durable-first). Since #597 no other caller is even spellable.
    record_order_placements(staged);
    Ok(())
}

fn actor_user_id(actor: &Actor) -> uuid::Uuid {
    actor.user_id
}

/// Split the adjacently-tagged `DomainEvent` into (event_type, payload) — mirrors the pool-backed
/// store's `split_event`.
fn split_event_tagged(
    event: &DomainEvent,
) -> Result<(String, serde_json::Value), DomainError> {
    let tagged =
        serde_json::to_value(event).map_err(|e| DomainError::Repository(e.to_string()))?;
    let event_type = tagged
        .get("eventType")
        .and_then(|t| t.as_str())
        .ok_or_else(|| DomainError::Repository("DomainEvent without eventType tag".into()))?
        .to_owned();
    let payload = tagged.get("payload").cloned().unwrap_or_else(|| serde_json::json!({}));
    Ok((event_type, payload))
}

#[cfg(test)]
mod order_placed_predicate_tests {
    use super::*;
    use domain::generated::entities as ent;
    use domain::generated::events as evs;
    use domain::generated::scalars as sc;

    fn actor() -> Actor {
        Actor {
            user_id: uuid::Uuid::from_u128(0xC057),
            user_type: "CUSTOMER".into(),
            domain_id: None,
            correlation_id: uuid::Uuid::from_u128(0xC0),
            cause_id: None,
        }
    }

    fn eur(cents: i64) -> ent::Money {
        ent::Money { amount_cents: sc::MoneyCents(cents), currency: sc::CurrencyCode("EUR".into()) }
    }

    /// A real `OrderPlaced` fact — the append the place-order guard makes exactly once per order.
    fn order_placed() -> DomainEvent {
        DomainEvent::OrderPlaced(evs::OrderPlaced {
            mode: None,
            order_id: sc::OrderId(uuid::Uuid::from_u128(0x0AD1)),
            r#ref: None,
            restaurant_id: sc::RestaurantId(uuid::Uuid::from_u128(0x0E57)),
            customer_id: sc::CustomerId(uuid::Uuid::from_u128(0xC057)),
            customer_contact: ent::CustomerContact {
                display_name: sc::CustomerDisplayName("Johnny".into()),
                email: None,
                phone: sc::PhoneNumber("+33612345678".into()),
            },
            service_type: sc::ServiceType::COLLECTION,
            delivery_address: None,
            items: Vec::new(),
            total_amount: eur(1960),
            breakdown: ent::PaymentBreakdown {
                articles: eur(1960),
                delivery: eur(0),
                service_fee: eur(0),
                total: eur(1960),
                restaurant_contribution: eur(0),
                restaurant_payout: eur(1960),
                rider_payout: eur(0),
                captain_net: eur(0),
            },
            note: None,
            replacement_of: None,
            payment_intent_id: Some(sc::PaymentIntentId("pi_test".into())),
        })
    }

    /// A non-order append (a cart fact) — what a partial-reaction delivery might stage without ever
    /// placing an order.
    fn cart_started() -> DomainEvent {
        DomainEvent::CartStarted(evs::CartStarted {
            cart_id: sc::CartId(uuid::Uuid::from_u128(0xCA47)),
            restaurant_id: sc::RestaurantId(uuid::Uuid::from_u128(0x0E57)),
            session_id: sc::SessionId(uuid::Uuid::from_u128(0x5E55)),
            customer_id: None,
        })
    }

    fn staged(events: Vec<DomainEvent>) -> Vec<StagedAppend> {
        vec![StagedAppend {
            stream_name: "Order-0000".into(),
            expected_version: 0,
            events,
            actor: actor(),
        }]
    }

    #[test]
    fn present_when_a_staged_append_carries_order_placed() {
        assert!(staged_contains_order_placed(&staged(vec![order_placed()])));
        // Also present when OrderPlaced sits alongside other appends (real placement stages more).
        let mut appends = staged(vec![cart_started()]);
        appends.extend(staged(vec![order_placed()]));
        assert!(staged_contains_order_placed(&appends));
    }

    #[test]
    fn absent_when_nothing_staged_or_only_non_order_appends() {
        // The replay shape: the guard is false, nothing is staged.
        assert!(!staged_contains_order_placed(&[]));
        // And a staged set that never placed an order.
        assert!(!staged_contains_order_placed(&staged(vec![cart_started()])));
    }
}
