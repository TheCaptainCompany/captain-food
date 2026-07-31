//! Partition leases over `mailbox_partitions` (PROP-20260728-152752 §3.1) — the
//! EventProcessorClient pattern: workers declare themselves by claiming rows, renew by heartbeat,
//! and take over expired leases; there is no coordinator. Every ownership CHANGE increments
//! `ownership_version` — the fencing counter [`crate::completion::complete_fenced`] asserts
//! inside the completion transaction. A heartbeat never bumps it (same owner, same authority).

use sqlx::{PgPool, Postgres, Row, Transaction};

/// One owned lane: the claim's proof of authority. `ownership_version` is what the completion
/// transaction asserts — hold it, never re-read it (re-reading would hide a takeover).
#[derive(Debug, Clone, PartialEq)]
pub struct Lane {
    pub actor_type: String,
    pub partition: i16,
    pub ownership_version: i64,
    pub checkpoint: i64,
}

/// Seed the registry rows for `actor_type` at `width` partitions — idempotent (`ON CONFLICT DO
/// NOTHING`), run by every worker at startup. The widths live in the host's actor catalog
/// (actors.yaml `mailbox.partitions`), not in a migration: the spec stays the source of truth.
pub async fn seed_partitions(pool: &PgPool, actor_type: &str, width: i16) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO mailbox_partitions (actor_type, partition, ownership_version, checkpoint) \
         SELECT $1, p, 0, 0 FROM generate_series(0, $2::int - 1) AS p \
         ON CONFLICT (actor_type, partition) DO NOTHING",
    )
    .bind(actor_type)
    .bind(i32::from(width))
    .execute(pool)
    .await?;
    Ok(())
}

/// Claim every CLAIMABLE lane of `actor_type` (unowned, or lease expired) for `worker_id`, up to
/// `max` lanes. Each claim bumps `ownership_version` (a takeover of a crashed owner IS an
/// ownership change). Returns the claimed lanes with their fresh authority.
pub async fn claim_due_lanes(
    pool: &PgPool,
    actor_type: &str,
    worker_id: &str,
    lease_seconds: i64,
    max: i64,
) -> sqlx::Result<Vec<Lane>> {
    // FOR UPDATE SKIP LOCKED keeps two workers starting at the same instant from fighting over
    // the same claimable rows — each claims a disjoint subset in one round.
    let rows = sqlx::query(
        "WITH claimable AS ( \
            SELECT actor_type, partition FROM mailbox_partitions \
            WHERE actor_type = $1 AND (lease_until IS NULL OR lease_until < now()) \
            ORDER BY partition \
            LIMIT $4 \
            FOR UPDATE SKIP LOCKED \
         ) \
         UPDATE mailbox_partitions p \
         SET claimed_by = $2, \
             lease_until = now() + make_interval(secs => $3), \
             ownership_version = p.ownership_version + 1 \
         FROM claimable c \
         WHERE p.actor_type = c.actor_type AND p.partition = c.partition \
         RETURNING p.actor_type, p.partition, p.ownership_version, p.checkpoint",
    )
    .bind(actor_type)
    .bind(worker_id)
    .bind(lease_seconds as f64)
    .bind(max)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(decode_lane).collect::<Result<_, _>>()?)
}

/// STEAL one specific lane regardless of lease liveness — the rebalance move (a worker under its
/// fair share takes from the largest owner) and the test lever for the split-brain window. The
/// previous owner keeps believing it owns the lane until its next heartbeat or completion — both
/// fail on the bumped `ownership_version`, which is the whole point (§3.1: dual BELIEF is
/// tolerated because dual AUTHORITY is impossible).
pub async fn steal_lane(
    pool: &PgPool,
    actor_type: &str,
    partition: i16,
    worker_id: &str,
    lease_seconds: i64,
) -> sqlx::Result<Option<Lane>> {
    let row = sqlx::query(
        "UPDATE mailbox_partitions \
         SET claimed_by = $3, \
             lease_until = now() + make_interval(secs => $4), \
             ownership_version = ownership_version + 1 \
         WHERE actor_type = $1 AND partition = $2 \
         RETURNING actor_type, partition, ownership_version, checkpoint",
    )
    .bind(actor_type)
    .bind(partition)
    .bind(worker_id)
    .bind(lease_seconds as f64)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(decode_lane).transpose()
}

/// Renew the lease on an owned lane. `false` = the renewal matched nothing — the lane was stolen
/// or re-claimed (ownership_version moved on): the caller must DROP the lane immediately and
/// abandon its in-flight work (whose completion would be fenced out anyway).
pub async fn heartbeat(
    pool: &PgPool,
    lane: &Lane,
    worker_id: &str,
    lease_seconds: i64,
) -> sqlx::Result<bool> {
    let res = sqlx::query(
        "UPDATE mailbox_partitions \
         SET lease_until = now() + make_interval(secs => $5) \
         WHERE actor_type = $1 AND partition = $2 AND claimed_by = $3 AND ownership_version = $4",
    )
    .bind(&lane.actor_type)
    .bind(lane.partition)
    .bind(worker_id)
    .bind(lane.ownership_version)
    .bind(lease_seconds as f64)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() == 1)
}

/// Graceful shutdown: surrender an owned lane so a peer can claim it NOW instead of waiting out
/// the lease. Only the current authority may release (same guard as the heartbeat).
pub async fn release_lane(pool: &PgPool, lane: &Lane, worker_id: &str) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE mailbox_partitions SET claimed_by = NULL, lease_until = NULL \
         WHERE actor_type = $1 AND partition = $2 AND claimed_by = $3 AND ownership_version = $4",
    )
    .bind(&lane.actor_type)
    .bind(lane.partition)
    .bind(worker_id)
    .bind(lane.ownership_version)
    .execute(pool)
    .await?;
    Ok(())
}

/// Advance the lane's checkpoint INSIDE a completion transaction, fenced: 0 rows = the caller's
/// authority is stale (stolen/re-claimed lane) and the whole transaction must roll back.
pub(crate) async fn advance_checkpoint_fenced(
    tx: &mut Transaction<'_, Postgres>,
    lane: &Lane,
    worker_id: &str,
    position: i64,
) -> sqlx::Result<bool> {
    // GREATEST keeps a same-authority replay from moving the checkpoint backwards; the fence
    // (claimed_by + ownership_version) is what keeps a STALE authority from moving it at all.
    let res = sqlx::query(
        "UPDATE mailbox_partitions \
         SET checkpoint = GREATEST(checkpoint, $5) \
         WHERE actor_type = $1 AND partition = $2 AND claimed_by = $3 AND ownership_version = $4",
    )
    .bind(&lane.actor_type)
    .bind(lane.partition)
    .bind(worker_id)
    .bind(lane.ownership_version)
    .bind(position)
    .execute(&mut **tx)
    .await?;
    Ok(res.rows_affected() == 1)
}

fn decode_lane(row: &sqlx::postgres::PgRow) -> sqlx::Result<Lane> {
    Ok(Lane {
        actor_type: row.try_get("actor_type")?,
        partition: row.try_get("partition")?,
        ownership_version: row.try_get("ownership_version")?,
        checkpoint: row.try_get("checkpoint")?,
    })
}
