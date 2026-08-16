//! The ROUTED-BIRTH lane DEAD-MAN'S SWITCH (#598, the `place-order` contract in
//! specs/observability.yaml).
//!
//! `order_birth_lag_ms{routed}` records **only when a delivery actually appended a routed birth**
//! (`flush.rs`), which is correct for a latency histogram and useless as a liveness signal: while
//! `ROUTE_ORDER_BIRTH_THROUGH_LANE` is OFF it records nothing at all, and after the flip a silent
//! series means either "flag off", "nobody ordered", or "the Order lane worker is dead". That is
//! precisely ADR-20260810-231300's defect class — *a monitoring path that can only fire when a
//! signal ARRIVES goes quiet exactly when it should scream.*
//!
//! **Why two new series rather than seeding that histogram with zeros**: injected zeros would
//! poison the p95 the flip is judged on. A liveness signal and a latency measurement are different
//! facts and must not share a series. So, on EVERY tick, for EVERY lane in
//! [`ROUTED_LANES`](application::generated::process_managers::ROUTED_LANES), **whatever the flag
//! says**:
//!
//! - `order_lane_watch_heartbeat_total{lane}` — monotonic, +1 per tick per lane. **Alert on the
//!   ABSENCE of an increment**, never on a threshold: this is the only series that distinguishes
//!   "the lane is quiet" from "the watcher, or its process, is gone".
//! - `order_lane_oldest_pending_age_ms{lane}` — how long the oldest still-undelivered message on
//!   that lane has been waiting; **0 when the lane is drained**. Readable only BECAUSE the
//!   heartbeat proves the reporter alive.
//!
//! Emitting while the flag is OFF is deliberate and is the point: a switch first proved on the day
//! of the flip is a switch proved BY the flip. The lag histogram stays silent; these two do not.
//!
//! Like `promotion_watch`, this is a MONITOR on its own clock, deliberately OUTSIDE the worker it
//! watches (the ADR's monitoring carve-out — for a monitor, silence is ambiguous, so it keeps a
//! poll permanently and with no exit): a watcher living inside the lane's own worker would be
//! silenced by the very death it exists to report.

use application::generated::process_managers::ROUTED_LANES;
use sqlx::{PgPool, Row};

/// One watch tick: for every DECLARED routed lane, emit the heartbeat and the oldest-pending age.
///
/// The query is grouped and asked ONCE for all lanes rather than per lane — the declared
/// population is small, but a per-lane round trip would make the monitor's own cost grow with the
/// thing it monitors. Lanes with no pending rows are absent from the result and report `0`; a lane
/// that reports only when it has a backlog is a lane whose silence is ambiguous.
pub async fn order_lane_watch_tick(pool: &PgPool) -> Result<(), sqlx::Error> {
    let lanes: Vec<&'static str> = declared_lanes();
    let rows = sqlx::query(
        "SELECT actor_type, min(received_at) AS oldest \
         FROM inbound_messages \
         WHERE status = 'RECEIVED' AND actor_type = ANY($1) \
         GROUP BY actor_type",
    )
    .bind(&lanes)
    .fetch_all(pool)
    .await?;

    let now = chrono::Utc::now();
    for lane in &lanes {
        let age_ms = rows
            .iter()
            .find(|r| r.try_get::<String, _>("actor_type").ok().as_deref() == Some(*lane))
            .and_then(|r| r.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("oldest").ok())
            .flatten()
            .map(|oldest| (now - oldest).num_milliseconds().max(0))
            .unwrap_or(0);
        telemetry::meters::place_order::order_lane_watch(lane, age_ms);
    }
    Ok(())
}

/// The DECLARED routed-birth lanes, deduplicated — two routed `deliver:` steps may address the
/// same aggregate, and the lane is watched once either way.
pub fn declared_lanes() -> Vec<&'static str> {
    let mut lanes: Vec<&'static str> = ROUTED_LANES.iter().map(|l| l.actor_type).collect();
    lanes.sort_unstable();
    lanes.dedup();
    lanes
}

/// Spawn the watch on its own clock. A failed tick logs and keeps ticking — a watcher that dies
/// with its patient defeats its purpose; its OWN death is observable as the heartbeat stopping,
/// which is the whole contract.
///
/// Spawned UNCONDITIONALLY, never behind `ROUTE_ORDER_BIRTH_THROUGH_LANE`: a liveness signal that
/// starts with the flip cannot be trusted at the flip, because nothing has ever seen it work.
pub fn spawn_order_lane_watch(pool: PgPool, every: std::time::Duration) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(e) = order_lane_watch_tick(&pool).await {
                tracing::warn!(error = %e, "order lane watch: tick failed -- retrying next tick");
            }
        }
    });
}
