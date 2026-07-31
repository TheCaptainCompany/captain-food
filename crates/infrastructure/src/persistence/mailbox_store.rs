//! Postgres adapter for the [`application::mailbox::Mailbox`] port (#242 Runtime C3): the
//! `inbound_messages` insert every flipped resolver performs, and the status read behind
//! `operationStatus`. Idempotency rides the `message_id` primary key: `ON CONFLICT DO NOTHING`
//! then read back — the caller discriminates replay (same hash) from conflict (different hash).

use application::mailbox::{Mailbox, MailboxEntry, MailboxInsertOutcome, MailboxStatusRow};
use async_trait::async_trait;
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};

use super::db_err;
use super::enum_sql::EnumText;

pub struct PgMailbox {
    pool: PgPool,
}

impl PgMailbox {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
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
        .execute(&self.pool)
        .await
        .map_err(db_err)?
        .rows_affected()
            == 1;
        if inserted {
            return Ok(MailboxInsertOutcome::Inserted);
        }
        let row = sqlx::query(
            "SELECT status, payload_hash FROM inbound_messages WHERE message_id = $1",
        )
        .bind(entry.message_id)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(MailboxInsertOutcome::Duplicate {
            status: EnumText::from_text(&row.try_get::<String, _>("status").map_err(db_err)?)?,
            payload_hash: row.try_get("payload_hash").map_err(db_err)?,
        })
    }

    async fn by_message(
        &self,
        message_id: uuid::Uuid,
    ) -> Result<Option<MailboxStatusRow>, DomainError> {
        let row = sqlx::query(
            "SELECT message_id, correlation_id, status, error, user_id, session_id, received_at, \
                    completed_at \
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
                user_id: r.try_get("user_id").map_err(db_err)?,
                session_id: r.try_get("session_id").map_err(db_err)?,
                received_at: r.try_get("received_at").map_err(db_err)?,
                completed_at: r.try_get("completed_at").map_err(db_err)?,
            })
        })
        .transpose()
    }
}
