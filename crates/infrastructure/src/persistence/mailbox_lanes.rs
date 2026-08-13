//! sqlx read adapter for the ADMIN `mailboxLanes` supervision query (#242 Runtime B,
//! PROP-20260728-152752): every `mailbox_partitions` registry row joined with its live backlog —
//! RECEIVED (pending) and SCHEDULED (future) counts plus the oldest pending `received_at` — from
//! `inbound_messages`. Write-path infrastructure, not a business read model: no `View_*`, and the
//! numbers are a monitoring snapshot, not a serialized truth (the worker's completion transaction
//! is the authority on a lane).

use actor_client::mailbox::MailboxAccess;
use actor_client::supervision::{MailboxLaneRepository, MailboxLaneRow, PoisonedMessageRow};
use application::queries::{MailboxRequeue, MailboxRequeueAccess, RequeueOutcome};
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
        retrying_attempts: row.try_get::<i64, _>("retrying_attempts").map_err(db_err)?,
        poisoned: row.try_get::<i64, _>("poisoned").map_err(db_err)?,
    })
}

#[async_trait]
impl MailboxLaneRepository for PgMailboxLaneRepository {
    async fn list(&self, _access: MailboxAccess) -> Result<Vec<MailboxLaneRow>, DomainError> {
        // LEFT JOIN LATERAL keeps the aggregate scan on the drain/scheduler partial indexes:
        // one indexed probe per lane rather than a full-table GROUP BY, so the page stays cheap
        // even while a backlog is large (which is exactly when someone is staring at it).
        let rows = sqlx::query(
            "SELECT p.actor_type, p.partition, p.ownership_version, p.claimed_by, p.lease_until, \
                    p.checkpoint, b.pending, b.scheduled, b.oldest_pending_at, \
                    COALESCE(b.retrying_attempts, 0)::bigint AS retrying_attempts, \
                    COALESCE(x.poisoned, 0)::bigint AS poisoned \
             FROM mailbox_partitions p \
             LEFT JOIN LATERAL ( \
                 SELECT count(*) FILTER (WHERE m.status = 'RECEIVED') AS pending, \
                        count(*) FILTER (WHERE m.status = 'SCHEDULED') AS scheduled, \
                        min(m.received_at) FILTER (WHERE m.status = 'RECEIVED') AS oldest_pending_at, \
                        max(m.attempts) FILTER (WHERE m.status = 'RECEIVED') AS retrying_attempts \
                 FROM inbound_messages m \
                 WHERE m.actor_type = p.actor_type AND m.partition = p.partition \
                   AND m.status IN ('RECEIVED', 'SCHEDULED') \
             ) b ON true \
             LEFT JOIN LATERAL ( \
                 SELECT count(*) AS poisoned \
                 FROM inbound_messages m \
                 WHERE m.actor_type = p.actor_type AND m.partition = p.partition \
                   AND m.status = 'FAILED' AND (m.error->>'code') = 'DeliveryInfrastructureError' \
             ) x ON true \
             ORDER BY p.actor_type, p.partition",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter().map(decode_lane).collect()
    }

    async fn poisoned(
        &self,
        actor_type: Option<String>,
        limit: i64,
        _access: MailboxAccess,
    ) -> Result<Vec<PoisonedMessageRow>, DomainError> {
        // Same poison predicate as the lane counter above — the detail view behind it (#315).
        // Newest first: the row an operator is hunting is almost always the one that just paged.
        let rows = sqlx::query(
            "SELECT m.message_id, m.actor_type, m.partition, m.message_type, m.attempts, \
                    m.error->>'code' AS error_code, m.correlation_id, m.received_at, m.completed_at \
             FROM inbound_messages m \
             WHERE m.status = 'FAILED' AND (m.error->>'code') = 'DeliveryInfrastructureError' \
               AND ($1::text IS NULL OR m.actor_type = $1) \
             ORDER BY m.received_at DESC \
             LIMIT $2",
        )
        .bind(actor_type)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        rows.iter()
            .map(|row| {
                Ok(PoisonedMessageRow {
                    message_id: row.try_get("message_id").map_err(db_err)?,
                    actor_type: row.try_get("actor_type").map_err(db_err)?,
                    partition: row.try_get::<i16, _>("partition").map_err(db_err)?,
                    message_type: row.try_get("message_type").map_err(db_err)?,
                    attempts: row.try_get::<i16, _>("attempts").map_err(db_err)?,
                    error_code: row.try_get("error_code").map_err(db_err)?,
                    correlation_id: row.try_get("correlation_id").map_err(db_err)?,
                    received_at: row.try_get("received_at").map_err(db_err)?,
                    completed_at: row.try_get("completed_at").map_err(db_err)?,
                })
            })
            .collect()
    }
}

/// Postgres adapter of the poisoned-row recovery write port (#315): predicate + flip in ONE
/// UPDATE (no check-then-act window), the lane nudged in the same transaction-less statement
/// via the SAME `pg_notify` channel the enqueue door uses — so the requeued row is picked up on
/// commit, not at the next heartbeat. The fallback SELECT only runs when nothing flipped, to
/// name WHY for the error context.
pub struct PgMailboxRequeue {
    pool: PgPool,
}

impl PgMailboxRequeue {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MailboxRequeue for PgMailboxRequeue {
    async fn requeue_if_poisoned(
        &self,
        message_id: uuid::Uuid,
        _access: MailboxRequeueAccess,
    ) -> Result<RequeueOutcome, DomainError> {
        // The arbitration IS the write: rows in any other state never match the predicate, so
        // there is nothing to race — a concurrent duplicate requeue simply finds RECEIVED below.
        let flipped = sqlx::query(
            "WITH flip AS ( \
                 UPDATE inbound_messages \
                 SET status = 'RECEIVED', attempts = 0, error = NULL, \
                     next_attempt_at = NULL, last_attempt_at = NULL, completed_at = NULL \
                 WHERE message_id = $1 \
                   AND status = 'FAILED' AND (error->>'code') = 'DeliveryInfrastructureError' \
                 RETURNING actor_type \
             ) \
             SELECT actor_type, pg_notify('inbound_messages', actor_type) FROM flip",
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        if let Some(row) = flipped {
            return Ok(RequeueOutcome::Requeued {
                actor_type: row.try_get("actor_type").map_err(db_err)?,
            });
        }
        let existing = sqlx::query(
            "SELECT actor_type, status FROM inbound_messages WHERE message_id = $1",
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        match existing {
            None => Ok(RequeueOutcome::NotFound),
            Some(row) => {
                let status: String = row.try_get("status").map_err(db_err)?;
                if status == "RECEIVED" {
                    // Our own earlier flip (a retried delivery) or a raced twin — converged.
                    Ok(RequeueOutcome::AlreadyDeliverable {
                        actor_type: row.try_get("actor_type").map_err(db_err)?,
                    })
                } else {
                    Ok(RequeueOutcome::NotRequeueable { status })
                }
            }
        }
    }
}
