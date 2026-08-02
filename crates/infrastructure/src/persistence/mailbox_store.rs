//! Postgres adapter for the [`actor_client::mailbox::Mailbox`] port (#242 Runtime C3): the
//! `inbound_messages` insert every flipped resolver performs, and the status read behind
//! `operationStatus`. Idempotency rides the `message_id` primary key: `ON CONFLICT DO NOTHING`
//! then read back — the caller discriminates replay (same hash) from conflict (different hash).
//! Reminders (#272 Runtime D) enter through `schedule`: same table, status SCHEDULED, `position`
//! NULL until the promotion pass — with the ADR-20260731-150500 reschedule-in-place conflict arm.

use std::collections::HashMap;
use std::sync::Arc;

use actor_client::mailbox::{
    Mailbox, MailboxEntry, MailboxInsertOutcome, MailboxScheduleOutcome, MailboxStatusRow,
};
use async_trait::async_trait;
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};

use super::db_err;
use super::enum_sql::EnumText;

/// The enqueue→worker wake registry: one [`tokio::sync::Notify`] per mailbox actor type. A
/// Entries per statement in [`Mailbox::insert_many`]. Postgres caps a statement at 65535 bind
/// parameters and each entry binds 17, so the hard ceiling is ~3855; 500 keeps each statement's
/// parameter payload modest while still collapsing a 200-row producer batch into ONE round-trip.
const INSERT_MANY_CHUNK: usize = 500;

/// producer's successful insert nudges the actor type's in-process worker so delivery starts on
/// the next pass instead of waiting out the heartbeat poll (the poll stays the guarantee — a
/// nudge is an accelerator, and a process without the worker simply has nobody listening).
#[derive(Default, Clone)]
pub struct MailboxNudges {
    map: HashMap<String, Arc<tokio::sync::Notify>>,
}

impl MailboxNudges {
    /// Register (or fetch) the actor type's wake signal — the composition root hands the same
    /// `Notify` to the actor type's worker.
    pub fn register(&mut self, actor_type: &str) -> Arc<tokio::sync::Notify> {
        self.map.entry(actor_type.to_owned()).or_default().clone()
    }

    /// The actor type's wake signal, for the worker side of the wiring.
    pub fn get(&self, actor_type: &str) -> Option<Arc<tokio::sync::Notify>> {
        self.map.get(actor_type).cloned()
    }

    /// Wake the actor type's worker, if one is registered in this process.
    pub fn nudge(&self, actor_type: &str) {
        if let Some(n) = self.map.get(actor_type) {
            n.notify_one();
        }
    }
}

pub struct PgMailbox {
    pool: PgPool,
    nudges: Option<Arc<MailboxNudges>>,
}

impl PgMailbox {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, nudges: None }
    }

    /// Wire the enqueue-side wake registry (composition root only — adapters' standalone
    /// binaries have no in-process workers to wake).
    pub fn with_nudges(mut self, nudges: Arc<MailboxNudges>) -> Self {
        self.nudges = Some(nudges);
        self
    }
}

#[async_trait]
impl Mailbox for PgMailbox {
    async fn insert(&self, entry: &MailboxEntry) -> Result<MailboxInsertOutcome, DomainError> {
        let inserted = sqlx::query(
            "INSERT INTO inbound_messages \
               (message_id, kind, actor_type, actor_id, partition, message_type, payload, \
                payload_hash, channel, user_id, user_type, correlation_id, cause_id, session_id, \
                trace_id, source, external_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17) \
             ON CONFLICT (message_id) DO NOTHING",
        )
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
        .execute(&self.pool)
        .await
        .map_err(db_err)?
        .rows_affected()
            == 1;
        if inserted {
            if let Some(nudges) = &self.nudges {
                nudges.nudge(entry.actor_type());
            }
            return Ok(MailboxInsertOutcome::Inserted);
        }
        let row = sqlx::query(
            "SELECT status, payload_hash FROM inbound_messages WHERE message_id = $1",
        )
        .bind(entry.message_id())
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(MailboxInsertOutcome::Duplicate {
            status: EnumText::from_text(&row.try_get::<String, _>("status").map_err(db_err)?)?,
            payload_hash: row.try_get("payload_hash").map_err(db_err)?,
        })
    }

    async fn schedule(
        &self,
        entry: &MailboxEntry,
        scheduled_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<MailboxScheduleOutcome, DomainError> {
        // The ADR-20260731-150500 §4 atomic form. Two constraints shape the statement:
        // - `position` is named and bound NULL — leaving it out would fire the column DEFAULT and
        //   burn a sequence value the promotion pass is supposed to allocate (§3.4: the position
        //   is stamped when due, so drain order reflects due order, not declare order);
        // - the conflict arm updates ONLY WHERE the row is still SCHEDULED, so a reschedule can
        //   never race the promotion pass into rewriting a promoted row. `(xmax = 0)` is the
        //   standard upsert discriminator: 0 on a fresh insert, the updater's xid on DO UPDATE.
        let row = sqlx::query(
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
               WHERE inbound_messages.status = 'SCHEDULED' \
             RETURNING (xmax = 0) AS inserted",
        )
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
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        // No nudge on any path: a SCHEDULED row is not deliverable until promotion.
        match row {
            Some(r) if r.try_get::<bool, _>("inserted").map_err(db_err)? => {
                Ok(MailboxScheduleOutcome::Scheduled)
            }
            Some(_) => Ok(MailboxScheduleOutcome::Rescheduled),
            // The conflict arm's WHERE excluded the row: a non-SCHEDULED collision, untouched.
            None => {
                let row = sqlx::query(
                    "SELECT status, payload_hash FROM inbound_messages WHERE message_id = $1",
                )
                .bind(entry.message_id())
                .fetch_one(&self.pool)
                .await
                .map_err(db_err)?;
                Ok(MailboxScheduleOutcome::Duplicate {
                    status: EnumText::from_text(
                        &row.try_get::<String, _>("status").map_err(db_err)?,
                    )?,
                    payload_hash: row.try_get("payload_hash").map_err(db_err)?,
                })
            }
        }
    }

    async fn cancel_scheduled(&self, message_id: uuid::Uuid) -> Result<bool, DomainError> {
        // The status predicate makes the flip race-safe against promotion: whichever statement
        // locks the row first wins, the loser matches zero rows.
        Ok(sqlx::query(
            "UPDATE inbound_messages SET status = 'CANCELLED', completed_at = now() \
             WHERE message_id = $1 AND status = 'SCHEDULED'",
        )
        .bind(message_id)
        .execute(&self.pool)
        .await
        .map_err(db_err)?
        .rows_affected()
            == 1)
    }

    /// One multi-row INSERT for the whole slice — the batched form of [`Self::insert`], same columns,
    /// same `ON CONFLICT (message_id) DO NOTHING`, same semantics per row.
    ///
    /// `RETURNING message_id` is what makes the result honest: `ON CONFLICT DO NOTHING` returns only
    /// the rows it actually wrote, so the returned set IS the set of new messages and everything
    /// absent from it was already on the mailbox.
    ///
    /// Chunked at [`INSERT_MANY_CHUNK`] because Postgres caps a statement at 65535 bind parameters
    /// and each entry binds 17 — a caller handing over a large slice must not hit a wall that has
    /// nothing to do with its own batching.
    ///
    /// Nudges fire ONCE per distinct actor type that gained a row, not once per row: the nudge is a
    /// wake-up for that type's worker, so N of them is N-1 wasted.
    async fn insert_many(
        &self,
        entries: &[MailboxEntry],
    ) -> Result<Vec<uuid::Uuid>, DomainError> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }
        let mut inserted: Vec<uuid::Uuid> = Vec::with_capacity(entries.len());
        for chunk in entries.chunks(INSERT_MANY_CHUNK) {
            let mut qb = sqlx::QueryBuilder::new(
                "INSERT INTO inbound_messages \
                   (message_id, kind, actor_type, actor_id, partition, message_type, payload, \
                    payload_hash, channel, user_id, user_type, correlation_id, cause_id, session_id, \
                    trace_id, source, external_id) ",
            );
            qb.push_values(chunk, |mut b, e| {
                b.push_bind(e.message_id())
                    .push_bind(e.kind().to_owned())
                    .push_bind(e.actor_type().to_owned())
                    .push_bind(e.actor_id())
                    .push_bind(e.partition())
                    .push_bind(e.message_type().to_owned())
                    .push_bind(e.payload().to_owned())
                    .push_bind(e.payload_hash().to_owned())
                    .push_bind(e.channel().to_owned())
                    .push_bind(e.user_id())
                    .push_bind(e.user_type().to_owned())
                    .push_bind(e.correlation_id())
                    .push_bind(e.cause_id())
                    .push_bind(e.session_id())
                    .push_bind(e.trace_id().map(str::to_owned))
                    .push_bind(e.source().map(str::to_owned))
                    .push_bind(e.external_id().map(str::to_owned));
            });
            qb.push(" ON CONFLICT (message_id) DO NOTHING RETURNING message_id");
            let ids: Vec<(uuid::Uuid,)> =
                qb.build_query_as().fetch_all(&self.pool).await.map_err(db_err)?;
            inserted.extend(ids.into_iter().map(|(id,)| id));
        }
        if !inserted.is_empty() {
            if let Some(nudges) = &self.nudges {
                let new: std::collections::HashSet<uuid::Uuid> = inserted.iter().copied().collect();
                let mut woken: std::collections::HashSet<&str> = std::collections::HashSet::new();
                for e in entries.iter().filter(|e| new.contains(&e.message_id())) {
                    if woken.insert(e.actor_type()) {
                        nudges.nudge(e.actor_type());
                    }
                }
            }
        }
        Ok(inserted)
    }

    async fn by_message(
        &self,
        message_id: uuid::Uuid,
    ) -> Result<Option<MailboxStatusRow>, DomainError> {
        let row = sqlx::query(
            "SELECT message_id, correlation_id, status, error, payload_hash, user_id, session_id, \
                    received_at, completed_at \
             FROM inbound_messages WHERE message_id = $1",
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        row.map(|r| {
            Ok(MailboxStatusRow {
                message_id: r.try_get("message_id").map_err(db_err)?,
                correlation_id: r.try_get("correlation_id").map_err(db_err)?,
                status: EnumText::from_text(&r.try_get::<String, _>("status").map_err(db_err)?)?,
                error: r.try_get("error").map_err(db_err)?,
                payload_hash: r.try_get("payload_hash").map_err(db_err)?,
                user_id: r.try_get("user_id").map_err(db_err)?,
                session_id: r.try_get("session_id").map_err(db_err)?,
                received_at: r.try_get("received_at").map_err(db_err)?,
                completed_at: r.try_get("completed_at").map_err(db_err)?,
            })
        })
        .transpose()
    }
}
