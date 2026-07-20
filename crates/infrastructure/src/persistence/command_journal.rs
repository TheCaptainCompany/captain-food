//! Postgres adapter for the durable command journal (`command_journal`, ADR-20260720-015300): one row
//! per command submission, persisted BEFORE handling. Insert uses `ON CONFLICT (message_id) DO
//! NOTHING` + fetch-on-conflict so a replayed messageId reports the ORIGINAL row's status + payload
//! hash (the dispatch layer discriminates idempotent replay vs Conflict). Enum columns are INTEGER
//! declaration-order ordinals ([`super::enum_sql`]), matching every other store.

use application::journal::{
    CommandJournal, CommandJournalEntry, CommandJournalRow, JournalInsertOutcome,
};
use async_trait::async_trait;
use chrono::Duration;
use domain::generated::scalars::CommandJournalStatus;
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use super::db_err;
use super::enum_sql::EnumOrd;

/// Column list of `command_journal`, in row field order.
const COLUMNS: &str = "message_id, correlation_id, cause_id, session_id, trace_id, user_id, \
     user_type, channel, command_type, payload, payload_hash, status, error, received_at, \
     completed_at";

fn decode(row: &PgRow) -> Result<CommandJournalRow, DomainError> {
    Ok(CommandJournalRow {
        entry: CommandJournalEntry {
            message_id: row.try_get("message_id").map_err(db_err)?,
            correlation_id: row.try_get("correlation_id").map_err(db_err)?,
            cause_id: row.try_get("cause_id").map_err(db_err)?,
            session_id: row.try_get("session_id").map_err(db_err)?,
            trace_id: row.try_get("trace_id").map_err(db_err)?,
            user_id: row.try_get("user_id").map_err(db_err)?,
            user_type: row.try_get("user_type").map_err(db_err)?,
            channel: EnumOrd::from_ord(row.try_get::<i32, _>("channel").map_err(db_err)?)?,
            command_type: row.try_get("command_type").map_err(db_err)?,
            payload: row.try_get("payload").map_err(db_err)?,
            payload_hash: row.try_get("payload_hash").map_err(db_err)?,
        },
        status: EnumOrd::from_ord(row.try_get::<i32, _>("status").map_err(db_err)?)?,
        error: row.try_get("error").map_err(db_err)?,
        received_at: row.try_get("received_at").map_err(db_err)?,
        completed_at: row.try_get("completed_at").map_err(db_err)?,
    })
}

/// Postgres [`CommandJournal`] over `command_journal`.
pub struct PgCommandJournal {
    pool: PgPool,
}

impl PgCommandJournal {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CommandJournal for PgCommandJournal {
    async fn insert(&self, entry: &CommandJournalEntry) -> Result<JournalInsertOutcome, DomainError> {
        let sql = format!(
            "INSERT INTO command_journal ({COLUMNS}) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,NULL,now(),NULL) \
             ON CONFLICT (message_id) DO NOTHING"
        );
        let inserted = sqlx::query(&sql)
            .bind(entry.message_id)
            .bind(entry.correlation_id)
            .bind(entry.cause_id)
            .bind(entry.session_id)
            .bind(entry.trace_id.clone())
            .bind(entry.user_id)
            .bind(entry.user_type)
            .bind(entry.channel.to_ord())
            .bind(entry.command_type.clone())
            .bind(entry.payload.clone())
            .bind(entry.payload_hash.clone())
            .bind(CommandJournalStatus::RECEIVED.to_ord())
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        if inserted == 1 {
            return Ok(JournalInsertOutcome::Inserted);
        }
        // Conflict: report the original row so the dispatch layer can discriminate replay/conflict.
        let existing = self.by_message(entry.message_id).await?.ok_or_else(|| {
            DomainError::Invariant(format!(
                "command_journal insert conflicted but message {} is unreadable",
                entry.message_id
            ))
        })?;
        Ok(JournalInsertOutcome::Duplicate {
            status: existing.status,
            payload_hash: existing.entry.payload_hash,
        })
    }

    async fn complete(
        &self,
        message_id: uuid::Uuid,
        status: CommandJournalStatus,
        error: Option<serde_json::Value>,
    ) -> Result<(), DomainError> {
        sqlx::query(
            "UPDATE command_journal SET status = $2, error = $3, completed_at = now() \
             WHERE message_id = $1",
        )
        .bind(message_id)
        .bind(status.to_ord())
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn by_message(
        &self,
        message_id: uuid::Uuid,
    ) -> Result<Option<CommandJournalRow>, DomainError> {
        let sql = format!("SELECT {COLUMNS} FROM command_journal WHERE message_id = $1");
        let row =
            sqlx::query(&sql).bind(message_id).fetch_optional(&self.pool).await.map_err(db_err)?;
        row.as_ref().map(decode).transpose()
    }

    async fn sweep_stale_received(&self, older_than: Duration) -> Result<u64, DomainError> {
        // A handler spawned but never completed (crash between insert and complete) → FAILED.
        let swept = sqlx::query(
            "UPDATE command_journal SET status = $1, completed_at = now(), \
             error = jsonb_build_object('code', 'Internal', 'context', \
                     jsonb_build_object('detail', 'stale RECEIVED swept (handler never completed)')) \
             WHERE status = $2 AND received_at < now() - $3::interval",
        )
        .bind(CommandJournalStatus::FAILED.to_ord())
        .bind(CommandJournalStatus::RECEIVED.to_ord())
        .bind(format!("{} seconds", older_than.num_seconds().max(0)))
        .execute(&self.pool)
        .await
        .map_err(db_err)?
        .rows_affected();
        Ok(swept)
    }
}
