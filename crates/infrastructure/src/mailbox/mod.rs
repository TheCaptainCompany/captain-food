//! Captain.Food's side of the actor mailbox (#242 Runtime C, PROP-20260728-152752): the
//! [`actor_runtime`] crate owns claim/fence/drain generically; THIS module supplies what is ours —
//! the command dispatch (generated router over the application handlers), the staged-event flush
//! into the fenced completion transaction, and the post-commit status-bus fan-out.

mod enqueue;
mod handler;
mod pm_delivery;

pub use enqueue::{
    cancel_reminder, enqueue_inbound_fact, enqueue_worker_command, inbound_fact_for,
    inbound_message_id, inbound_namespace, reminder_message_id, schedule_reminder,
    surrogate_actor_id, EnqueueOutcome, InboundFact, ScheduleOutcome,
};
pub use handler::{MailboxCommandHandler, StatusBusObserver};
pub use pm_delivery::backfill_stripe_facts_to_pm_lanes;

use application::ports::{version_conflict, Actor};
use application::staging::StagedAppend;
use domain::shared::errors::DomainError;
use sqlx::{Postgres, Transaction};

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
/// The window comes from `windows` (config key → DAYS, the composition root's
/// `Config::reminder_windows()`); a missing key is a WIRING bug and aborts the delivery for
/// retry — it must never land a terminal verdict or silently skip a GDPR clock.
pub async fn apply_schedules_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    message: &actor_runtime::InboundMessage,
    windows: &std::collections::HashMap<&'static str, i64>,
) -> Result<(), sqlx::Error> {
    for spec in application::generated::reminders::reminder_schedules_for(
        &message.actor_type,
        &message.message_type,
    ) {
        let Some(days) = windows.get(spec.after_days_key) else {
            return Err(sqlx::Error::Protocol(format!(
                "reminder window {} not wired — pass Config::reminder_windows() to the mailbox handler",
                spec.after_days_key
            )));
        };
        let entry = application::reminders::scheduled_entry(
            spec,
            message.actor_id,
            message.partition,
            message.correlation_id,
            Some(message.message_id),
        );
        let scheduled_at = chrono::Utc::now() + chrono::Duration::days(*days);
        sqlx::query(
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
               WHERE inbound_messages.status = 'SCHEDULED'",
        )
        .bind(entry.message_id)
        .bind(&entry.kind)
        .bind(&entry.actor_type)
        .bind(entry.actor_id)
        .bind(entry.partition)
        .bind(&entry.message_type)
        .bind(&entry.payload)
        .bind(&entry.payload_hash)
        .bind(&entry.channel)
        .bind(entry.user_id)
        .bind(&entry.user_type)
        .bind(entry.correlation_id)
        .bind(entry.cause_id)
        .bind(entry.session_id)
        .bind(&entry.trace_id)
        .bind(&entry.source)
        .bind(&entry.external_id)
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
