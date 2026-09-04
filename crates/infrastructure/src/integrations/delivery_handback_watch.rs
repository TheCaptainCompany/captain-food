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

use std::time::Duration;

use sqlx::{PgPool, Row};

/// Sweep cadence — the offer TTL is minutes-scale (`DELIVERY_OFFER_MAX_TTL_SECONDS`, default 900),
/// so a sub-minute pass keeps the gauge current well inside that budget (mirrors
/// `delivery_offer_timeout_worker::SWEEP_INTERVAL`).
const SWEEP_INTERVAL: Duration = Duration::from_secs(30);

const UNREASSIGNED_SQL: &str = "SELECT coalesce(max(extract(epoch FROM now() - handed_back_at)), 0)::bigint \
                                 FROM view_deliveryjob \
                                 WHERE food_location IS NOT NULL AND status IN ('PENDING', 'FAILED')";

/// One sweep: emit the gauge (0 included), then — only on a complete pass — the heartbeat.
pub async fn delivery_handback_watch_tick(pool: &PgPool) -> Result<(), sqlx::Error> {
    let row = sqlx::query(UNREASSIGNED_SQL).fetch_one(pool).await?;
    let age: i64 = row.try_get(0)?;
    telemetry::meters::custody_handback::unreassigned_age(age.max(0));

    // LAST. The heartbeat certifies a COMPLETE sweep; the `?` above returns before it, so a failed
    // tick leaves the counter flat -- the alertable condition.
    telemetry::meters::custody_handback::sweep_completed();
    Ok(())
}

/// Spawn the sweep on its own clock. A failed tick logs and keeps ticking -- a watcher that dies
/// with its patient defeats its purpose; its OWN death is observable as the heartbeat stopping.
pub fn spawn_delivery_handback_watch(pool: PgPool, every: Duration) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(e) = delivery_handback_watch_tick(&pool).await {
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
