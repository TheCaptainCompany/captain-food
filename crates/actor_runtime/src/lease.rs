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

/// The live-ownership census of one actor type — what the fair-share rebalance decides from.
/// "Live" = a lease that has not expired; an expired lease's lanes are CLAIMABLE and belong to
/// [`claim_due_lanes`], never to a steal.
#[derive(Debug, Clone, PartialEq)]
pub struct OwnershipCensus {
    /// Total registry rows for the actor type (the partition WIDTH).
    pub total: i64,
    /// Lanes this worker holds with a live lease.
    pub mine: i64,
    /// Distinct workers holding at least one live lease (this worker included when `mine > 0`).
    pub live_owners: i64,
    /// The live owner with the most lanes, excluding this worker: `(claimed_by, count)`.
    pub largest_other: Option<(String, i64)>,
}

impl OwnershipCensus {
    /// The fair share: `total / instances`, floored — `instances` counts this worker even when it
    /// currently owns nothing (it is bidding). A worker at or above the floor never steals, which
    /// is what makes the loop converge instead of thrash when the width does not divide evenly
    /// (PROP-20260728-152752 §3.1: one steal per pass, from the largest owner — the Azure
    /// EventProcessorClient move).
    pub fn fair_share(&self) -> i64 {
        let instances = self.live_owners + i64::from(self.mine == 0);
        if instances == 0 {
            return self.total;
        }
        self.total / instances
    }
}

/// Take the census: total width, this worker's live count, the live-owner count, and the largest
/// OTHER owner (the steal target's identity and size).
pub async fn ownership_census(
    pool: &PgPool,
    actor_type: &str,
    worker_id: &str,
) -> sqlx::Result<OwnershipCensus> {
    let totals = sqlx::query(
        "SELECT count(*) AS total, \
                count(*) FILTER (WHERE claimed_by = $2 AND lease_until > now()) AS mine, \
                count(DISTINCT claimed_by) FILTER (WHERE claimed_by IS NOT NULL AND lease_until > now()) AS live_owners \
         FROM mailbox_partitions WHERE actor_type = $1",
    )
    .bind(actor_type)
    .bind(worker_id)
    .fetch_one(pool)
    .await?;
    let largest_other = sqlx::query(
        "SELECT claimed_by, count(*) AS lanes FROM mailbox_partitions \
         WHERE actor_type = $1 AND claimed_by IS NOT NULL AND claimed_by <> $2 \
           AND lease_until > now() \
         GROUP BY claimed_by ORDER BY lanes DESC, claimed_by LIMIT 1",
    )
    .bind(actor_type)
    .bind(worker_id)
    .fetch_optional(pool)
    .await?;
    Ok(OwnershipCensus {
        total: totals.try_get("total")?,
        mine: totals.try_get("mine")?,
        live_owners: totals.try_get("live_owners")?,
        largest_other: largest_other
            .map(|r| Ok::<_, sqlx::Error>((r.try_get("claimed_by")?, r.try_get("lanes")?)))
            .transpose()?,
    })
}

/// Steal ONE lane from `victim` (the census's largest owner) for `worker_id` — the fair-share
/// rebalance move. Picks the victim's highest-numbered live lane (any of theirs would do; the
/// counts drive convergence, not the choice). Returns `None` when the victim no longer holds a
/// live lane (a concurrent steal or expiry got there first) — the caller just tries again next
/// pass with a fresh census.
pub async fn steal_from(
    pool: &PgPool,
    actor_type: &str,
    victim: &str,
    worker_id: &str,
    lease_seconds: i64,
) -> sqlx::Result<Option<Lane>> {
    let row = sqlx::query(
        "UPDATE mailbox_partitions p \
         SET claimed_by = $3, \
             lease_until = now() + make_interval(secs => $4), \
             ownership_version = p.ownership_version + 1 \
         FROM ( \
            SELECT actor_type, partition FROM mailbox_partitions \
            WHERE actor_type = $1 AND claimed_by = $2 AND lease_until > now() \
            ORDER BY partition DESC LIMIT 1 \
            FOR UPDATE SKIP LOCKED \
         ) v \
         WHERE p.actor_type = v.actor_type AND p.partition = v.partition \
         RETURNING p.actor_type, p.partition, p.ownership_version, p.checkpoint",
    )
    .bind(actor_type)
    .bind(victim)
    .bind(worker_id)
    .bind(lease_seconds as f64)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(decode_lane).transpose()
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

#[cfg(test)]
mod tests {
    use super::OwnershipCensus;

    fn census(total: i64, mine: i64, live_owners: i64) -> OwnershipCensus {
        OwnershipCensus { total, mine, live_owners, largest_other: None }
    }

    /// The floor rule that makes the steal loop converge: a bidder counts itself, a worker at
    /// its floor never steals, and an empty registry yields no bids.
    #[test]
    fn fair_share_floors_and_counts_a_zero_lane_bidder() {
        // A joining worker with nothing yet bids as an instance: 100 lanes, 1 live owner + me.
        assert_eq!(census(100, 0, 1).fair_share(), 50);
        // Three live owners, I am one of them: floor(100/3).
        assert_eq!(census(100, 33, 3).fair_share(), 33);
        // Uneven width: 8 lanes across 3 instances floors to 2 — the 3/3/2 spread is stable.
        assert_eq!(census(8, 2, 3).fair_share(), 2);
        // Nobody live and I hold nothing: claim (not steal) is the path; share = total.
        assert_eq!(census(8, 0, 0).fair_share(), 8);
    }
}

fn decode_lane(row: &sqlx::postgres::PgRow) -> sqlx::Result<Lane> {
    Ok(Lane {
        actor_type: row.try_get("actor_type")?,
        partition: row.try_get("partition")?,
        ownership_version: row.try_get("ownership_version")?,
        checkpoint: row.try_get("checkpoint")?,
    })
}
