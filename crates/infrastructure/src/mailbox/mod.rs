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
/// can fire"). Keying on the staged set rather than the delivery outcome is what makes it
/// replay-safe — see [`staged_contains_order_placed`]. This is the infra/framework boundary the
/// c4-l3 `instrumented` rule allows the telemetry SDK to live at; the domain and application
/// layers stay SDK-free.
///
/// **WHEN it fires is not this function's business, and no route's either** (#588). It used to be
/// called from ONE delivery route — the PM fact route, because that was the only place an
/// `OrderPlaced` could be staged. Moving the birth onto the Order lane moved the append to a
/// DIFFERENT route, and the counter would have gone silently to zero: a dashboard reading "no
/// stranger has paid us" while orders were being placed normally, which is precisely the failure
/// the counter exists to detect. Nothing caught it, because "which routes count" was a decision
/// each route made separately.
///
/// So the decision now lives in exactly one place — [`flush_staged_in_tx`], THE way staged events
/// reach `domain_events`. A delivery that appends an `OrderPlaced` counts it because it flushed
/// it, and a new route cannot forget. This function stays public because it is the EMIT seam the
/// #456 spy test drives (`tests/orders_placed_metric.rs`); calling it from a delivery route is the
/// mistake, and `no_delivery_route_decides_when_to_count_a_placement` below fails the build if one
/// starts again.
pub fn record_order_placements(staged: &[StagedAppend]) {
    if staged_contains_order_placed(staged) {
        telemetry::meters::place_order::placed("PLACED");
    }
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
    // (durable-first). See [`record_order_placements`] for the regression this shape prevents.
    record_order_placements(staged);
    Ok(())
}

fn actor_user_id(actor: &Actor) -> uuid::Uuid {
    actor.user_id
}

/// Flush staged lane ENQUEUES into the completion transaction — the second half of
/// ADR-20260816-040239's seam, and the reason a routed `deliver:` is not a dual write: the door
/// row and the saga's own effects (the run row, the remaining appends, the verdict) commit or roll
/// back together. A degenerate outbox — same database, same transaction, no relay.
///
/// Three properties, each load-bearing:
///
/// - **`&mut Transaction`, never `self.deps`' pool.** Get this wrong and the atomicity is a
///   fiction: a crash between the insert and the commit would leave a birth message for a saga hop
///   that never happened.
/// - **The identity is the caller's, and it is FROZEN**: `inbound_message_id(source, external_id)`
///   with `external_id` = the TARGET AGGREGATE's id. A redelivered trigger re-derives the SAME id
///   and collides on the primary key, so redelivery dedups at the door rather than at the
///   aggregate — and `ON CONFLICT DO NOTHING` makes that collision a **success**, never an error
///   the process manager must handle.
/// - **`pg_notify` rides the same transaction** (PROP-20260802-223522 D1), so the target lane's
///   worker — in this process or any other — wakes when the delivery commits and never on a
///   rolled-back one. A deduplicated insert notifies nobody, which is correct: the first insert
///   already did.
///
/// The ENVELOPE is stamped from the triggering mailbox row: `cause_id` is that row (the causality
/// chain the saga hop belongs to) and the principal carries over, so the birth records who actually
/// caused it. Post-flip `OrderPlaced` rows therefore carry a different envelope from the
/// saga-appended ones — legal, permanent and never backfilled (ADR-20260816-040239 "What
/// legitimately changes").
pub async fn flush_lane_enqueues_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    message: &actor_runtime::InboundMessage,
    enqueues: &[application::lanes::LaneEnqueue],
) -> Result<(), DomainError> {
    for enqueue in enqueues {
        // The lane keyspace WIDTH comes from the DECLARED contract (`actors.yaml`
        // `mailbox.partitions`, generated into `ACTOR_MAILBOXES`), not from the seeded
        // `mailbox_partitions` row count the PM chain reads. Deliberate, and it is the money-path
        // difference: the seeded count is a runtime artifact, so reading it would make a checkout
        // saga FAIL because a worker had not started yet — a paid order with no birth, the worst
        // failure mode this product has. The declared width is the frozen routing contract, so the
        // row is always addressed correctly and simply WAITS on its lane until a worker claims it.
        let Some((_, width)) = crate::generated::command_router::ACTOR_MAILBOXES
            .iter()
            .find(|(a, _)| *a == enqueue.actor_type)
        else {
            // Unreachable through the emitter (the `pm-deliver-lane` validator rule proves the
            // target declares a mailbox); a wiring bug, never a business outcome.
            return Err(DomainError::Repository(format!(
                "routed deliver of '{}' but '{}' declares no mailbox — wiring bug",
                enqueue.event_type, enqueue.actor_type
            )));
        };
        let message_id = actor_client::inbound_message_id(&enqueue.source, &enqueue.external_id);
        sqlx::query(
            "WITH ins AS ( \
               INSERT INTO inbound_messages \
                 (message_id, kind, actor_type, actor_id, partition, message_type, payload, \
                  payload_hash, channel, user_id, user_type, correlation_id, cause_id, source, \
                  external_id) \
               VALUES ($1, 'EVENT', $2, $3, $4, $5, $6, $7, 'WORKER', $8, $9, $10, $11, $12, $13) \
               ON CONFLICT (message_id) DO NOTHING \
               RETURNING actor_type \
             ) \
             SELECT pg_notify('inbound_messages', actor_type) FROM ins",
        )
        .bind(message_id)
        .bind(enqueue.actor_type)
        .bind(enqueue.actor_id)
        .bind(actor_client::stable_partition(&enqueue.actor_id, *width))
        .bind(enqueue.event_type)
        .bind(&enqueue.payload)
        .bind(application::journal::payload_hash(&enqueue.payload))
        .bind(message.user_id)
        .bind(&message.user_type)
        .bind(message.correlation_id)
        .bind(message.message_id)
        .bind(&enqueue.source)
        .bind(&enqueue.external_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| DomainError::Repository(e.to_string()))?;
    }
    Ok(())
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

    /// **#588 regression guard.** The zeroing this prevents: `record_order_placements` used to be
    /// called by ONE delivery route, because that route was the only place an `OrderPlaced` could
    /// be staged. Routing the birth onto the Order lane moved the append to another route and the
    /// #456 counter would have gone silently to zero — a flat "nobody has paid us" dashboard while
    /// checkout worked fine. NOTHING caught that, because "does this route count?" was a decision
    /// each route took on its own.
    ///
    /// The fix is structural: [`flush_staged_in_tx`] — the single way a staged event reaches
    /// `domain_events` — decides, so a route cannot forget. This test is the other half: it fails
    /// if any delivery route starts deciding again. A source scan rather than a type, because the
    /// mistake is an ABSENT call, and absence is exactly what the type system cannot see (the
    /// `actor_door_is_named_only_by_generated_client_crates` pattern: level 3 where level 4
    /// cannot reach).
    #[test]
    fn no_delivery_route_decides_when_to_count_a_placement() {
        let handler = include_str!("handler.rs");
        assert!(
            !handler.contains("record_order_placements"),
            "handler.rs names `record_order_placements`: the BAM counter's WHEN belongs to \
             flush_staged_in_tx alone (#588). A per-route call is how the counter silently went \
             to zero when the Order birth moved lanes — add the append to the flush, not the \
             count to the route."
        );
    }

    #[test]
    fn absent_when_nothing_staged_or_only_non_order_appends() {
        // The replay shape: the guard is false, nothing is staged.
        assert!(!staged_contains_order_placed(&[]));
        // And a staged set that never placed an order.
        assert!(!staged_contains_order_placed(&staged(vec![cart_started()])));
    }
}
