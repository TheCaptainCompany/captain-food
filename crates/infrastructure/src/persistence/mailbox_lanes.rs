//! sqlx read adapter for the ADMIN `mailboxLanes` supervision query (#242 Runtime B,
//! PROP-20260728-152752): every `mailbox_partitions` registry row joined with its live backlog —
//! RECEIVED (pending) and SCHEDULED (future) counts plus the oldest pending `received_at` — from
//! `inbound_messages`. Write-path infrastructure, not a business read model: no `View_*`, and the
//! numbers are a monitoring snapshot, not a serialized truth (the worker's completion transaction
//! is the authority on a lane).

use application::queries::{MailboxLaneRepository, MailboxLaneRow};
use async_trait::async_trait;
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use super::db_err;

/// Postgres adapter over `mailbox_partitions` + `inbound_messages`.
pub struct PgMailboxLaneRepository {
    pool: PgPool,
}

impl PgMailboxLaneRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn decode_lane(row: &PgRow) -> Result<MailboxLaneRow, DomainError> {
    Ok(MailboxLaneRow {
        actor_type: row.try_get("actor_type").map_err(db_err)?,
        partition: row.try_get::<i16, _>("partition").map_err(db_err)?,
        ownership_version: row.try_get::<i64, _>("ownership_version").map_err(db_err)?,
        claimed_by: row.try_get("claimed_by").map_err(db_err)?,
        lease_until: row.try_get("lease_until").map_err(db_err)?,
        checkpoint: row.try_get::<i64, _>("checkpoint").map_err(db_err)?,
        pending: row.try_get::<i64, _>("pending").map_err(db_err)?,
        scheduled: row.try_get::<i64, _>("scheduled").map_err(db_err)?,
        oldest_pending_at: row.try_get("oldest_pending_at").map_err(db_err)?,
    })
}

#[async_trait]
impl MailboxLaneRepository for PgMailboxLaneRepository {
    async fn list(&self) -> Result<Vec<MailboxLaneRow>, DomainError> {
        // LEFT JOIN LATERAL keeps the aggregate scan on the drain/scheduler partial indexes:
        // one indexed probe per lane rather than a full-table GROUP BY, so the page stays cheap
        // even while a backlog is large (which is exactly when someone is staring at it).
        let rows = sqlx::query(
            "SELECT p.actor_type, p.partition, p.ownership_version, p.claimed_by, p.lease_until, \
                    p.checkpoint, b.pending, b.scheduled, b.oldest_pending_at \
             FROM mailbox_partitions p \
             LEFT JOIN LATERAL ( \
                 SELECT count(*) FILTER (WHERE m.status = 'RECEIVED') AS pending, \
                        count(*) FILTER (WHERE m.status = 'SCHEDULED') AS scheduled, \
                        min(m.received_at) FILTER (WHERE m.status = 'RECEIVED') AS oldest_pending_at \
                 FROM inbound_messages m \
                 WHERE m.actor_type = p.actor_type AND m.partition = p.partition \
                   AND m.status IN ('RECEIVED', 'SCHEDULED') \
             ) b ON true \
             ORDER BY p.actor_type, p.partition",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter().map(decode_lane).collect()
    }
}
