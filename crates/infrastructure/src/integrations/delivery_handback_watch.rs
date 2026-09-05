//! THE CUSTODY-HANDBACK DEAD-MAN'S SWITCH (#639 part C step 3-ii, ADR-20260904-015903 §8, the
//! `custody-handback` contract in specs/observability.yaml) — mirrors `birth_gap_watch`'s shape
//! for a DIFFERENT worst failure mode: *a rider handed a job back, stating where the food is, and
//! nobody has re-offered it since. How long has that been true?*
//!
//! **The re-offer is `DeliveryDispatchProcess`'s job, and that PM step is FENCED** (issue #860,
//! `deferred:`) — so until it lands, a handed-back job sits PENDING/FAILED with nothing
//! automatically re-offering it. A counter that fires on the handback and goes quiet during the
//! stranding is the ADR-20260810-231300 defect class: a monitor that can only fire when a signal
//! ARRIVES goes quiet exactly when it should scream.
//!
//! **The source is `View_DeliveryJob`, not a raw `domain_events` scan** — the view already folds
//! `food_location`/`handed_back_at`/`status` custody-keyed (#639 part C step 3-ii): `food_location`
//! is set by a handback and RESET to null by the next acceptance (rider or partner), so
//! `food_location IS NOT NULL AND status IN ('PENDING','FAILED')` is exactly "a handback with no
//! later acceptance re-offering it" — the read model already proves the negative the raw log would
//! need a self-join to express.
//!
//! **Zeros are the contract.** The gauge reports on EVERY tick, empty population included, and
//! `delivery_handback_sweep_heartbeat_total` increments only after a COMPLETE pass.
//!
//! **NON-FENCED, beside `delivery_offer_timeout_worker`** — never inside the fenced mailbox handler
//! (ADR-20260904-015903 §10: the fence is opened for exactly one additive arm in `inbox.rs`, and
//! nothing else).
//!
//! **#639 part C step 4-iii-B (ADR-20260904-152807 §8) adds a SECOND gauge to the same tick**: the
//! rider-custody dead-man's switch — *a RESTRICTED rider is still holding food. For how long?* The
//! predicate is LITERALLY `rider_restriction.standing = 'RESTRICTED'` (dba, pre-code read): a
//! `RiderReinstated` sets `standing = ACTIVE` and `reinstated_at` but LEAVES `ground`/`decided_at`/
//! `effective_at` populated (history, not a live flag) — `effective_at IS NOT NULL` would page
//! forever after every reinstatement, a false-negative dead-man's switch. `effective_at` is the
//! ANCHOR only, never the predicate. Emitted AFTER the handback gauge above and BEFORE the
//! heartbeat, so the heartbeat now certifies BOTH gauges' sweep completed; the `?` ordering the
//! module doc already documents means a failed restricted-holding query returns before the
//! heartbeat too. ONE set-based SQL statement, `rider_restriction` (small, unindexed) as the build
//! side, joined to `view_deliveryjob`'s held-status set (never a second statement for the max, and
//! never a per-rider `held_by_rider` call — `View_DeliveryJob` is 14 correlated subqueries over
//! `domain_events` per row with nothing pushed down, #883, so every extra statement folds the whole
//! delivery history again at the sweep cadence).

use std::time::Duration;

use sqlx::{PgPool, Row};

/// Sweep cadence — the offer TTL is minutes-scale (`DELIVERY_OFFER_MAX_TTL_SECONDS`, default 900),
/// so a sub-minute pass keeps the gauge current well inside that budget (mirrors
/// `delivery_offer_timeout_worker::SWEEP_INTERVAL`).
const SWEEP_INTERVAL: Duration = Duration::from_secs(30);

const UNREASSIGNED_SQL: &str = "SELECT coalesce(max(extract(epoch FROM now() - handed_back_at)), 0)::bigint \
                                 FROM view_deliveryjob \
                                 WHERE food_location IS NOT NULL AND status IN ('PENDING', 'FAILED')";

/// The held set `held_by_rider(s)` uses (`crates/infrastructure/src/persistence/delivery.rs`),
/// joined through `rider_restriction` (small, unindexed — the build side) to every RESTRICTED
/// rider. Returns `(rider_id, delivery_job_id, effective_at)` per stranded row; the max age is
/// computed in Rust from these rows, never in a second statement.
const RESTRICTED_HOLDING_SQL: &str = "SELECT rr.rider_id, v.delivery_job_id, rr.effective_at \
                                       FROM rider_restriction rr \
                                       JOIN view_deliveryjob v ON v.rider_id = rr.rider_id \
                                       WHERE rr.standing = 'RESTRICTED' \
                                         AND v.status IN ('ASSIGNED', 'PICKED_UP', 'OUT_FOR_DELIVERY')";

/// One sweep: emit the handback gauge, then the rider-custody gauge (0 included on both), then —
/// only on a complete pass — the heartbeat. `restricted_custody_max_age_seconds` is the resolved
/// `RIDER_RESTRICTED_CUSTODY_MAX_AGE_SECONDS` (the composition root's job to resolve, mirroring
/// `DeliveryOfferTimeoutWorker::new` — this worker does not read the environment itself); it gates
/// nothing but the info event's `threshold_exceeded` field, a debounce on the PAGE, never a door.
pub async fn delivery_handback_watch_tick(
    pool: &PgPool,
    restricted_custody_max_age_seconds: i64,
) -> Result<(), sqlx::Error> {
    let row = sqlx::query(UNREASSIGNED_SQL).fetch_one(pool).await?;
    let age: i64 = row.try_get(0)?;
    telemetry::meters::custody_handback::unreassigned_age(age.max(0));

    // #639 part C step 4-iii-B: the rider-custody dead-man's switch, second gauge on this tick.
    let started = std::time::Instant::now();
    let rows = sqlx::query(RESTRICTED_HOLDING_SQL).fetch_all(pool).await?;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    tracing::info!(
        query = "rider_restricted_holding_job",
        rows = rows.len(),
        elapsed_ms,
        "custody sweep query -- #883's cost is visible before it bites"
    );

    let now = chrono::Utc::now();
    let mut max_age_seconds = 0i64;
    for r in &rows {
        let rider_id: uuid::Uuid = r.try_get("rider_id")?;
        let delivery_job_id: uuid::Uuid = r.try_get("delivery_job_id")?;
        let effective_at: chrono::DateTime<chrono::Utc> = r.try_get("effective_at")?;
        let age_seconds = (now - effective_at).num_seconds().max(0);
        max_age_seconds = max_age_seconds.max(age_seconds);
        let threshold_exceeded = age_seconds > restricted_custody_max_age_seconds;
        // No `correlation_id` on a timer tick -- a recorded divergence from the #748 skip-trace
        // shape, which pairs a denial's identity with the request that triggered it; this event
        // has no request, only a sweep. Joined to the gauge by aggregate ids instead.
        tracing::info!(
            rider_id = %rider_id,
            delivery_job_id = %delivery_job_id,
            age_seconds,
            threshold_exceeded,
            "rider.restricted.holding_job"
        );
    }
    telemetry::meters::rider_restriction::holding_job_age(max_age_seconds);

    // LAST. The heartbeat certifies a COMPLETE sweep -- both gauges above, since #639 part C step
    // 4-iii-B; the `?`s above return before it, so a failed tick leaves the counter flat -- the
    // alertable condition.
    telemetry::meters::custody_handback::sweep_completed();
    Ok(())
}

/// Spawn the sweep on its own clock. A failed tick logs and keeps ticking -- a watcher that dies
/// with its patient defeats its purpose; its OWN death is observable as the heartbeat stopping.
pub fn spawn_delivery_handback_watch(
    pool: PgPool,
    every: Duration,
    restricted_custody_max_age_seconds: i64,
) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(e) =
                delivery_handback_watch_tick(&pool, restricted_custody_max_age_seconds).await
            {
                tracing::warn!(error = %e, "delivery handback watch: tick failed -- retrying next tick");
            }
        }
    });
}

/// See [`SWEEP_INTERVAL`].
pub fn default_sweep_interval() -> Duration {
    SWEEP_INTERVAL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_interval_is_well_inside_the_offer_ttl_default() {
        // DELIVERY_OFFER_MAX_TTL_SECONDS defaults to 900s (delivery_offer_timeout_worker.rs) -- the
        // gauge must stay current well inside that budget.
        assert!(SWEEP_INTERVAL.as_secs() < 900);
    }
}
