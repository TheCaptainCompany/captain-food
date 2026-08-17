//! Captain.Food's side of the actor mailbox (#242 Runtime C, PROP-20260728-152752): the
//! [`actor_runtime`] crate owns claim/fence/drain generically; THIS module supplies what is ours —
//! the command dispatch (generated router over the application handlers), the staged-event flush
//! into the fenced completion transaction, and the post-commit status-bus fan-out.

mod activation;
// THE BIRTH-GAP dead-man's switch (#608) — "money held, no order" made a signal, on its own clock.
mod birth_gap_watch;
// The staged-event flush and the BAM counter's WHEN (#597). PRIVATE, and it exports only
// `flush_staged_in_tx` + `record_order_birth_lag`: the emit decision itself
// (`record_order_placements`) is unreachable from any delivery route — see `flush.rs`'s module
// docs for the counter-zeroing that hole caused and the source scan this privacy replaced.
mod flush;
// NOTE (#290 phase 1, PROP-20260802-130500 D1): the entry CONSTRUCTORS (`enqueue`), the typed
// actor clients, the outcome enums and the deterministic id derivations all moved to the
// `actor_client` boundary crate — building a `MailboxEntry` is that crate's exclusive,
// compiler-enforced permission. This module keeps ONLY what is genuinely infrastructure: the
// delivery glue over SQL (handler, in-tx flush/schedules), the PM lane chaining, the standalone
// worker spawn, and the activation cache.
mod handler;
// The ROUTED-BIRTH lane dead-man's switch (#598) — a monitor on its own clock, outside the lane.
mod order_lane_watch;
mod pm_delivery;
mod promotion_watch;
mod standalone;

pub use activation::{ActivationLaneEvents, ActivationSettings, CachedStream, StreamActivations};
pub use birth_gap_watch::{birth_gap_watch_tick, spawn_birth_gap_watch};
pub use flush::{flush_staged_in_tx, record_order_birth_lag};
pub use order_lane_watch::{
    declared_lanes, order_lane_watch_tick, spawn_order_lane_watch,
};
pub use promotion_watch::{promotion_watch_tick, spawn_promotion_watch};
pub use standalone::{
    shutdown_signal, spawn_standalone_workers,
    spawn_standalone_workers_with, standalone_deps, standalone_workers_enabled,
};
// `verdict_of_error` is PUBLIC for one reason (#623): it is the ONLY function that decides what
// a failed command leaves behind in `inbound_messages.error` — the jsonb column that persists 90
// days and is served as `Operation.errorCode`. The leak canary in
// `crates/adapters/stripe/tests/journal_leak_canary.rs` drives a RECORDED Stripe error body
// through the REAL adapter decoder and then through this function, which is the only way to
// assert on the journal row's error JSON without a live call, a hand-written error string, or a
// database. It cannot live in this crate: the decoder is in `stripe-adapter`, which depends on
// `infrastructure`, so the test has to sit on the other side of that edge.
pub use handler::{verdict_of_error, MailboxCommandHandler, StatusBusObserver};
pub use pm_delivery::backfill_stripe_facts_to_pm_lanes;

use domain::shared::errors::DomainError;
use sqlx::{Postgres, Transaction};
use tracing::Instrument as _;

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
        // The lane comes from the DECLARED contract (`actors.yaml` `mailbox.partitions`, generated
        // into `ACTOR_MAILBOXES`). This site was always right and was the reference the #596 fix
        // copied; it now shares that fix's single accessor, `actor_client::declared_lane`, so the
        // two can no longer drift apart the way they had. The reason, kept verbatim because it is
        // the money-path difference: the seeded `mailbox_partitions` count is a RUNTIME artifact,
        // so reading it would make a checkout saga FAIL because a worker had not started yet — a
        // paid order with no birth, the worst failure mode this product has. The declared width is
        // the frozen routing contract, so the row is always addressed correctly and simply WAITS
        // on its lane until a worker claims it.
        let Some(partition) = actor_client::declared_lane(enqueue.actor_type, &enqueue.actor_id)
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
        .bind(partition)
        .bind(enqueue.event_type)
        .bind(&enqueue.payload)
        .bind(application::journal::payload_hash(&enqueue.payload))
        .bind(message.user_id)
        .bind(&message.user_type)
        .bind(message.correlation_id)
        .bind(message.message_id)
        .bind(&enqueue.source)
        .bind(&enqueue.external_id)
        // `order.lane.enqueue` (#598): the HANDOVER's act, instrumented HERE at the
        // infrastructure seam — `c4-l3.yaml` keeps the saga and the aggregates SDK-free, and this
        // glue is the framework boundary that owns the write. It is the second branch of
        // `place-order`'s success ALTERNATION: with the flag ON no `event.store.append` occurs in
        // the saga's trace, so without this span a checkout that lost the birth entirely and one
        // that handed it over correctly are the same trace shape.
        .execute(&mut **tx)
        .instrument(telemetry::spans::order_lane_enqueue(enqueue.event_type, &enqueue.external_id))
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
