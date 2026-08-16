//! sqlx read adapter for the ADMIN `mailboxLanes` supervision query (#242 Runtime B,
//! PROP-20260728-152752): every DECLARED lane joined with its registry row and its live backlog —
//! RECEIVED (pending) and SCHEDULED (future) counts plus the oldest pending `received_at` — from
//! `inbound_messages`. Write-path infrastructure, not a business read model: no `View_*`, and the
//! numbers are a monitoring snapshot, not a serialized truth (the worker's completion transaction
//! is the authority on a lane).
//!
//! Since #596 the lane population comes from the DECLARATION (`ACTOR_MAILBOXES`) rather than from
//! `mailbox_partitions`, so a lane that has work but no registry row is visible instead of absent.
//! See [`MailboxLaneRepository::list`]'s body for why that inversion is load-bearing.

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
        // The lane population is DRIVEN BY THE DECLARATION (#596, `dba`), not by the registry.
        //
        // This used to read `FROM mailbox_partitions p`, and the #596 fix would have made that a
        // blind spot: before it, a hop addressed to an unseeded lane ERRORED, so the operator
        // heard about it immediately (too immediately — it aborted a money-path transaction).
        // After it, the hop is written and WAITS, which is correct — but a lane with no registry
        // row was invisible to this page, so the one screen an operator opens to ask "is anything
        // stuck?" would answer "no" over a backlog of orders waiting on a worker that never
        // started. Trading a loud wrong failure for a silent right one is not an improvement.
        //
        // So the driving set is the declared grid (`ACTOR_MAILBOXES`, `unnest`ed below) UNIONed
        // with any lane actually CARRYING work, and the registry is the thing LEFT JOINed in.
        // Three states are now all visible and distinguishable:
        //   - declared + seeded  -> the normal row, lease and checkpoint populated;
        //   - declared, NOT seeded -> `ownership_version = 0`, `claimed_by = NULL`, and a real
        //     pending count: exactly the "waiting on a worker that never started" case;
        //   - NOT declared but carrying rows -> the orphan a width DECREASE strands. Nothing else
        //     in the system would ever mention it; `seed_partitions`' drift check refuses the
        //     start that creates it, and this row is where an operator sees what was stranded.
        //
        // Cost: one extra distinct-scan of the pending/scheduled rows on the drain and scheduler
        // partial indexes. The LEFT JOIN LATERAL probes are unchanged and still keep the per-lane
        // aggregates off a full-table GROUP BY, so the page stays cheap while a backlog is large —
        // which is exactly when someone is staring at it.
        let declared = crate::generated::command_router::ACTOR_MAILBOXES;
        let mut actor_types: Vec<&str> = Vec::new();
        let mut partitions: Vec<i16> = Vec::new();
        for (actor_type, width) in declared {
            for partition in 0..*width as i16 {
                actor_types.push(actor_type);
                partitions.push(partition);
            }
        }
        let rows = sqlx::query(
            "WITH declared AS ( \
                 SELECT * FROM unnest($1::text[], $2::smallint[]) AS d(actor_type, partition) \
             ), \
             lanes AS ( \
                 SELECT actor_type, partition FROM declared \
                 UNION \
                 SELECT actor_type, partition FROM mailbox_partitions \
                 UNION \
                 SELECT DISTINCT actor_type, partition FROM inbound_messages \
                   WHERE status IN ('RECEIVED', 'SCHEDULED') \
             ) \
             SELECT l.actor_type, l.partition, \
                    COALESCE(p.ownership_version, 0)::bigint AS ownership_version, \
                    p.claimed_by, p.lease_until, \
                    COALESCE(p.checkpoint, 0)::bigint AS checkpoint, \
                    b.pending, b.scheduled, b.oldest_pending_at, \
                    COALESCE(b.retrying_attempts, 0)::bigint AS retrying_attempts, \
                    COALESCE(x.poisoned, 0)::bigint AS poisoned \
             FROM lanes l \
             LEFT JOIN mailbox_partitions p \
                    ON p.actor_type = l.actor_type AND p.partition = l.partition \
             LEFT JOIN LATERAL ( \
                 SELECT count(*) FILTER (WHERE m.status = 'RECEIVED') AS pending, \
                        count(*) FILTER (WHERE m.status = 'SCHEDULED') AS scheduled, \
                        min(m.received_at) FILTER (WHERE m.status = 'RECEIVED') AS oldest_pending_at, \
                        max(m.attempts) FILTER (WHERE m.status = 'RECEIVED') AS retrying_attempts \
                 FROM inbound_messages m \
                 WHERE m.actor_type = l.actor_type AND m.partition = l.partition \
                   AND m.status IN ('RECEIVED', 'SCHEDULED') \
             ) b ON true \
             LEFT JOIN LATERAL ( \
                 SELECT count(*) AS poisoned \
                 FROM inbound_messages m \
                 WHERE m.actor_type = l.actor_type AND m.partition = l.partition \
                   AND m.status = 'FAILED' AND (m.error->>'code') = 'DeliveryInfrastructureError' \
             ) x ON true \
             ORDER BY l.actor_type, l.partition",
        )
        .bind(&actor_types)
        .bind(&partitions)
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
