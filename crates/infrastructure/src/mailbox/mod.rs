//! Captain.Food's side of the actor mailbox (#242 Runtime C, PROP-20260728-152752): the
//! [`actor_runtime`] crate owns claim/fence/drain generically; THIS module supplies what is ours —
//! the command dispatch (generated router over the application handlers), the staged-event flush
//! into the fenced completion transaction, and the post-commit status-bus fan-out.

mod activation;
// NOTE (#290 phase 1, PROP-20260802-130500 D1): the entry CONSTRUCTORS (`enqueue`), the typed
// actor clients, the outcome enums and the deterministic id derivations all moved to the
// `actor_client` boundary crate — building a `MailboxEntry` is that crate's exclusive,
// compiler-enforced permission. This module keeps ONLY what is genuinely infrastructure: the
// delivery glue over SQL (handler, in-tx flush/schedules), the PM lane chaining, the standalone
// worker spawn, and the activation cache.
mod handler;
mod pm_delivery;
mod promotion_watch;
mod standalone;

pub use activation::{ActivationLaneEvents, ActivationSettings, CachedStream, StreamActivations};
pub use promotion_watch::{promotion_watch_tick, spawn_promotion_watch};
pub use standalone::{
    shutdown_signal, spawn_standalone_workers,
    spawn_standalone_workers_with, standalone_deps, standalone_workers_enabled,
};
pub use handler::{MailboxCommandHandler, StatusBusObserver};
pub use pm_delivery::backfill_stripe_facts_to_pm_lanes;

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
/// can fire"). Call AFTER [`flush_staged_in_tx`] succeeds, so the counter only advances once the
/// `OrderPlaced` append is in the completion transaction (durable-first). Keying on the staged set
/// rather than the delivery outcome is what makes it replay-safe — see
/// [`staged_contains_order_placed`]. This is the infra/framework boundary the c4-l3 `instrumented`
/// rule allows the telemetry SDK to live at; the domain and application layers stay SDK-free.
pub fn record_order_placements(staged: &[StagedAppend]) {
    if staged_contains_order_placed(staged) {
        telemetry::meters::place_order::placed("PLACED");
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
    Ok(())
}

fn actor_user_id(actor: &Actor) -> uuid::Uuid {
    actor.user_id
}

/// Apply the delivered message's declared `schedules:` INSIDE the completion transaction
/// (ADR-20260731-214500 §2): for each generated [`ReminderSchedule`] row matching
/// `(actor_type, message_type)`, upsert the SCHEDULED reminder — the ADR-20260731-150500 §4
/// atomic form, same statement as the pool-backed `PgMailbox::schedule`, so a committed delivery
/// can never lose its declared reminder to a crash between commit and a post-commit hand-off.
/// The window comes from `windows` (config key → typed Duration, the composition root's
/// `Config::reminder_windows()`, which applies the key's declared `unit:` — #167); a missing key
/// is a WIRING bug and aborts the delivery for retry — it must never land a terminal verdict or
/// silently skip a GDPR clock. The conflict arm follows the reminder's DECLARED reschedule
/// policy: `in-place` postpones the pending row, `keep` (#167) leaves the first scheduled_at —
/// a deadline a redelivered birth fact must never push out.
pub async fn apply_schedules_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    message: &actor_runtime::InboundMessage,
    windows: &std::collections::HashMap<&'static str, std::time::Duration>,
) -> Result<(), sqlx::Error> {
    use application::generated::reminders::ReschedulePolicy;
    for spec in application::generated::reminders::reminder_schedules_for(
        &message.actor_type,
        &message.message_type,
    ) {
        let Some(window) = windows.get(spec.after_key) else {
            return Err(sqlx::Error::Protocol(format!(
                "reminder window {} not wired — pass Config::reminder_windows() to the mailbox handler",
                spec.after_key
            )));
        };
        let entry = actor_client::reminders::scheduled_entry(
            spec,
            message.actor_id,
            message.partition,
            message.correlation_id,
            Some(message.message_id),
        );
        let window = chrono::Duration::from_std(*window).map_err(|_| {
            sqlx::Error::Protocol(format!("reminder window {} out of range", spec.after_key))
        })?;
        let scheduled_at = chrono::Utc::now() + window;
        let sql = match spec.reschedule {
            ReschedulePolicy::InPlace => {
                "INSERT INTO inbound_messages \
                   (message_id, position, kind, actor_type, actor_id, partition, message_type, \
                    payload, payload_hash, channel, user_id, user_type, correlation_id, cause_id, \
                    session_id, trace_id, source, external_id, scheduled_at, status) \
                 VALUES ($1, NULL, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
                         $16, $17, $18, 'SCHEDULED') \
                 ON CONFLICT (message_id) DO UPDATE \
                   SET scheduled_at = EXCLUDED.scheduled_at, \
                       payload = EXCLUDED.payload, \
                       payload_hash = EXCLUDED.payload_hash \
                   WHERE inbound_messages.status = 'SCHEDULED'"
            }
            // LOAD-BEARING for the #167 acceptance clock: the birth reminder is applied through
            // THIS module (the worker's post-delivery `schedules:` pass), not through
            // `persistence::mailbox_store`. Guarded by `redelivered_authorization_dedups_the_birth_at_the_door`
            // (crates/infrastructure/tests/main/pm_prepare_delivery.rs, #588) — mutating the
            // DO NOTHING below to DO UPDATE reds it, because a redelivered birth would push the
            // first deadline out.
            ReschedulePolicy::Keep => {
                "INSERT INTO inbound_messages \
                   (message_id, position, kind, actor_type, actor_id, partition, message_type, \
                    payload, payload_hash, channel, user_id, user_type, correlation_id, cause_id, \
                    session_id, trace_id, source, external_id, scheduled_at, status) \
                 VALUES ($1, NULL, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
                         $16, $17, $18, 'SCHEDULED') \
                 ON CONFLICT (message_id) DO NOTHING"
            }
        };
        sqlx::query(sql)
        .bind(entry.message_id())
        .bind(entry.kind())
        .bind(entry.actor_type())
        .bind(entry.actor_id())
        .bind(entry.partition())
        .bind(entry.message_type())
        .bind(entry.payload())
        .bind(entry.payload_hash())
        .bind(entry.channel())
        .bind(entry.user_id())
        .bind(entry.user_type())
        .bind(entry.correlation_id())
        .bind(entry.cause_id())
        .bind(entry.session_id())
        .bind(entry.trace_id())
        .bind(entry.source())
        .bind(entry.external_id())
        .bind(scheduled_at)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Split the adjacently-tagged `DomainEvent` into (event_type, payload) — mirrors the pool-backed
/// store's `split_event`.
fn split_event_tagged(
    event: &domain::generated::events::DomainEvent,
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
