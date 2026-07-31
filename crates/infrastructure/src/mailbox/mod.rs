//! Captain.Food's side of the actor mailbox (#242 Runtime C, PROP-20260728-152752): the
//! [`actor_runtime`] crate owns claim/fence/drain generically; THIS module supplies what is ours —
//! the command dispatch (generated router over the application handlers), the staged-event flush
//! into the fenced completion transaction, and the post-commit status-bus fan-out.

mod handler;

pub use handler::{MailboxCommandHandler, StatusBusObserver};

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
