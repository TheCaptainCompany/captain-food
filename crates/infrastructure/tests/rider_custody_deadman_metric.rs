//! THE RIDER-CUSTODY DEAD-MAN'S SWITCH, proved by looking at the series (#639 part C step 4-iii-B,
//! ADR-20260904-152807 §8) — sibling to `custody_handback_metric.rs`'s shape for a DIFFERENT worst
//! failure mode: *a RESTRICTED rider is still holding food. For how long?*
//!
//! **The state is manufactured honestly**: `rider_restriction` is a projector-maintained TABLE with
//! its OWN checkpoint (`migrations/20260904110000_rider_standing.sql`) — every scenario appends
//! `RiderRegistered`/`RiderRestricted`/`RiderReinstated` and runs `ProjectionWorker::run_once()`,
//! never a hand `INSERT` (otherwise the replay scenario below would be vacuous). `view_deliveryjob`
//! is a pure SQL fold view over `domain_events` (no worker), so a delivery job's state is a straight
//! event append.
//!
//! **ONE test binary, ONE `#[tokio::test]`, every scenario runs SEQUENTIALLY inside it** — the spy
//! meter provider is a process-global `OnceLock` with shared delta drains
//! (`tests/main/spy_meter.rs` doc §1); six `#[tokio::test]` fns would race and pass vacuously.
//!
//! **Info-event capture**: `rider.restricted.holding_job` is asserted through a captured
//! `tracing` subscriber (the `skip_trace_visibility.rs` `Capture`/`MakeWriter` idiom), installed
//! for the life of the test via `tracing::subscriber::set_default` — sound here because the default
//! `#[tokio::test]` flavor is single-threaded (`current_thread`), so the whole test body, including
//! every `.await`, runs on the one thread the guard was created on.
//!
//! Needs `DATABASE_URL`: since #474 a missing database FAILS this suite; only an explicit
//! `DB_TESTS_REQUIRED=0` skips it, and that leaves a receipt (`crates/db_test_gate`).

#[path = "main/common.rs"]
mod common;
#[path = "main/spy_meter.rs"]
mod spy_meter;

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};
use infrastructure::integrations::delivery_handback_watch::delivery_handback_watch_tick;
use infrastructure::ProjectionWorker;
use sqlx::PgPool;
use telemetry::contract::metric;
use tracing_subscriber::layer::SubscriberExt;

/// The `RIDER_RESTRICTED_CUSTODY_MAX_AGE_SECONDS` default (`specs/delivery/configuration.yaml`) —
/// only a debounce on the info event's `threshold_exceeded` field, never a door; any value works
/// for this suite's assertions, which never look at that field.
const THRESHOLD: i64 = 1800;

// ─── tracing capture (the `skip_trace_visibility.rs` idiom) ──────────────────────────────────────

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn contents(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
    /// Isolate each scenario's captured lines from the next, the same reason `SpyMeter::drain`
    /// takes rather than reads.
    fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = CaptureWriter;
    fn make_writer(&'a self) -> Self::Writer {
        CaptureWriter(self.0.clone())
    }
}

struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// ─── event fixtures ────────────────────────────────────────────────────────────────────────────

async fn append_event(
    pool: &PgPool,
    stream_name: &str,
    version: i32,
    event_type: &str,
    payload: serde_json::Value,
) {
    sqlx::query(
        "INSERT INTO domain_events \
         (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, payload, metadata, occurred_at) \
         VALUES ($1, $2, $3, $4, 5, $5, NULL, $6, $7, NULL, now())",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(stream_name)
    .bind(version)
    .bind(uuid::Uuid::nil())
    .bind(uuid::Uuid::new_v4())
    .bind(event_type)
    .bind(payload)
    .execute(pool)
    .await
    .expect("append event");
}

fn rfc3339(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

async fn registered(pool: &PgPool, rider_id: uuid::Uuid, auth_ref: &str) {
    append_event(
        pool,
        &format!("Rider-{rider_id}"),
        1,
        "RiderRegistered",
        serde_json::json!({
            "riderId": rider_id, "authRef": auth_ref, "displayName": "Test Rider",
            "phone": "+33611223344", "status": "OFFLINE"
        }),
    )
    .await;
}

async fn restricted(
    pool: &PgPool,
    rider_id: uuid::Uuid,
    version: i32,
    decided_at: DateTime<Utc>,
    effective_at: DateTime<Utc>,
) {
    append_event(
        pool,
        &format!("Rider-{rider_id}"),
        version,
        "RiderRestricted",
        serde_json::json!({
            "riderId": rider_id, "ground": "ACCOUNT_COMPROMISE",
            "decidedAt": rfc3339(decided_at), "effectiveAt": rfc3339(effective_at),
        }),
    )
    .await;
}

async fn reinstated(pool: &PgPool, rider_id: uuid::Uuid, version: i32) {
    append_event(
        pool,
        &format!("Rider-{rider_id}"),
        version,
        "RiderReinstated",
        serde_json::json!({ "riderId": rider_id }),
    )
    .await;
}

fn address(line1: &str) -> serde_json::Value {
    serde_json::json!({ "line1": line1, "city": "Tours", "postalCode": "37000", "country": "FR" })
}

async fn job_requested(pool: &PgPool, job: uuid::Uuid, order: uuid::Uuid, restaurant: uuid::Uuid) {
    append_event(
        pool,
        &format!("DeliveryJob-{job}"),
        1,
        "DeliveryRequested",
        serde_json::json!({
            "deliveryJobId": job, "orderId": order, "restaurantId": restaurant,
            "pickup": address("1 rue de la Paix"), "dropoff": address("2 avenue Grammont"),
        }),
    )
    .await;
}

/// Requested (v1) then accepted (v2) — status ASSIGNED, rider_id = `rider`. The most common
/// "holding" shape this suite needs.
async fn job_requested_and_assigned(
    pool: &PgPool,
    job: uuid::Uuid,
    order: uuid::Uuid,
    restaurant: uuid::Uuid,
    rider: uuid::Uuid,
) {
    job_requested(pool, job, order, restaurant).await;
    append_event(
        pool,
        &format!("DeliveryJob-{job}"),
        2,
        "DeliveryAcceptedByRider",
        serde_json::json!({ "deliveryJobId": job, "orderId": order, "riderId": rider }),
    )
    .await;
}

/// Requested, accepted, then COMPLETED (status DELIVERED) — a rider's past, resolved job, never a
/// "holding" job.
async fn job_delivered(
    pool: &PgPool,
    job: uuid::Uuid,
    order: uuid::Uuid,
    restaurant: uuid::Uuid,
    rider: uuid::Uuid,
) {
    job_requested_and_assigned(pool, job, order, restaurant, rider).await;
    append_event(
        pool,
        &format!("DeliveryJob-{job}"),
        3,
        "DeliveryCompleted",
        serde_json::json!({ "deliveryJobId": job, "orderId": order }),
    )
    .await;
}

async fn run_once(pool: &PgPool) {
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (rider_restriction fold)");
}

fn gauge_points(spy: &spy_meter::SpyMeter) -> spy_meter::Series {
    spy.drain()
}

#[tokio::test]
async fn the_rider_custody_gauge_is_a_dead_mans_switch() {
    let spy = spy_meter::SpyMeter::install();

    let capture = Capture::default();
    let filter = tracing_subscriber::EnvFilter::new("info");
    let subscriber = tracing_subscriber::registry().with(filter).with(
        tracing_subscriber::fmt::layer().json().flatten_event(true).with_writer(capture.clone()),
    );
    let _guard = tracing::subscriber::set_default(subscriber);

    let Some(db) = common::TestDb::acquire("rider_custody_deadman_metric").await else { return };
    let pool = db.pool();

    let restaurant = uuid::Uuid::new_v4();

    // ── (1) PRESENCE — an empty database still emits ONE point at 0, and the heartbeat fires once.
    delivery_handback_watch_tick(&pool, THRESHOLD).await.expect("a tick over an empty database");
    let s = gauge_points(&spy);
    assert_eq!(
        s.points(metric::RIDER_RESTRICTED_HOLDING_JOB_AGE_SECONDS),
        vec![(BTreeMap::new(), 0.0)],
        "no attributes, one point, 0 -- an ABSENT series must never read as 'nothing restricted'"
    );
    assert_eq!(
        s.points(metric::DELIVERY_HANDBACK_SWEEP_HEARTBEAT_TOTAL),
        vec![(BTreeMap::new(), 1.0)],
        "one completed sweep, one heartbeat"
    );
    capture.clear();

    // ── (2) THE MIS-ORDERED CONTROL (#870 lesson): R1 is RESTRICTED with `decidedAt` OLDER than
    // `effectiveAt` (M3's target -- anchoring on decided_at would read 7200, not 1800), holding an
    // ASSIGNED job, plus an unrelated OLD DELIVERED job (proves the held-status join narrows to
    // the ONE currently-held row, never the resolved one). R2 was restricted, then REINSTATED
    // (standing ACTIVE, `effective_at` STILL populated at 3600s -- M1's target: dropping
    // `standing = 'RESTRICTED'` from the predicate would let this stale row leak in and win the
    // max, 1800 -> 3600), holding an ASSIGNED job too.
    let now = Utc::now();
    let r1 = uuid::Uuid::new_v4();
    registered(&pool, r1, "auth-r1-holding").await;
    let (job_old, order_old) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    job_delivered(&pool, job_old, order_old, restaurant, r1).await;
    let (job_held, order_held) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    job_requested_and_assigned(&pool, job_held, order_held, restaurant, r1).await;
    restricted(&pool, r1, 2, now - Duration::seconds(7200), now - Duration::seconds(1800)).await;

    let r2 = uuid::Uuid::new_v4();
    registered(&pool, r2, "auth-r2-reinstated").await;
    let (job2, order2) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    job_requested_and_assigned(&pool, job2, order2, restaurant, r2).await;
    restricted(&pool, r2, 2, now - Duration::seconds(3650), now - Duration::seconds(3600)).await;
    reinstated(&pool, r2, 3).await;

    run_once(&pool).await;

    delivery_handback_watch_tick(&pool, THRESHOLD).await.expect("a tick with a stranded restriction");
    let s = gauge_points(&spy);
    let points = s.points(metric::RIDER_RESTRICTED_HOLDING_JOB_AGE_SECONDS);
    assert_eq!(points.len(), 1, "exactly one point, no `rider_id` label: {points:?}");
    let (attrs, value) = &points[0];
    assert!(attrs.is_empty(), "the only test of 'no `rider_id` label': {attrs:?}");
    assert!(
        (1700.0..1900.0).contains(value),
        "expected ~1800s (anchored on R1's effectiveAt, not decidedAt=7200 nor R2's stale 3600), got {value}"
    );
    let out = capture.contents();
    let info_lines: Vec<&str> = out.lines().filter(|l| l.contains("rider.restricted.holding_job")).collect();
    assert_eq!(
        info_lines.len(),
        1,
        "exactly one stranded row (R1's held job only -- R1's old DELIVERED job and R2's reinstated \
         row must both be excluded): {info_lines:?}\nfull capture:\n{out}"
    );
    let line = info_lines[0];
    for needle in [r1.to_string(), job_held.to_string()] {
        assert!(line.contains(&needle), "info event must carry both ids ({needle} missing): {line}");
    }

    // dba/farley (item A): #883's cost -- `view_deliveryjob` is 14 correlated subqueries over
    // `domain_events` per row with nothing pushed down -- must be VISIBLE before it bites, not
    // discovered in production. One EXPLAIN, captured in this DB-gated test's own log (run with
    // `--nocapture` to see it), over the SAME query shape the sweep runs with real data present.
    let plan_rows: Vec<(String,)> = sqlx::query_as(
        "EXPLAIN SELECT rr.rider_id, v.delivery_job_id, rr.effective_at \
         FROM rider_restriction rr \
         JOIN view_deliveryjob v ON v.rider_id = rr.rider_id \
         WHERE rr.standing = 'RESTRICTED' \
           AND v.status IN ('ASSIGNED', 'PICKED_UP', 'OUT_FOR_DELIVERY')",
    )
    .fetch_all(&pool)
    .await
    .expect("EXPLAIN the rider-custody sweep query");
    eprintln!("#883 EXPLAIN (rider-custody sweep query, populated):");
    for (row,) in &plan_rows {
        eprintln!("  {row}");
    }

    capture.clear();

    // ── (3) R1 REINSTATED (standing ACTIVE, effective_at STILL populated) -- back to 0.
    reinstated(&pool, r1, 3).await;
    run_once(&pool).await;
    delivery_handback_watch_tick(&pool, THRESHOLD).await.expect("a tick after reinstatement");
    assert_eq!(
        gauge_points(&spy).points(metric::RIDER_RESTRICTED_HOLDING_JOB_AGE_SECONDS),
        vec![(BTreeMap::new(), 0.0)],
        "reinstated (standing ACTIVE) must clear the gauge even though effective_at is still set"
    );

    // ── (3b) R1 RESTRICTED AGAIN, a NEW effectiveAt, still holding the SAME job -- non-zero,
    // anchored on the NEW effective_at (300s), never the OLD one (1800s) or cumulative.
    let now2 = Utc::now();
    restricted(&pool, r1, 4, now2 - Duration::seconds(310), now2 - Duration::seconds(300)).await;
    run_once(&pool).await;
    delivery_handback_watch_tick(&pool, THRESHOLD).await.expect("a tick after re-restriction");
    let s = gauge_points(&spy);
    let points = s.points(metric::RIDER_RESTRICTED_HOLDING_JOB_AGE_SECONDS);
    assert_eq!(points.len(), 1);
    let (_, value_3b) = points[0];
    assert!(
        (250.0..400.0).contains(&value_3b),
        "expected ~300s anchored on the NEW effectiveAt, got {value_3b}"
    );

    // ── (6) REPLAY-IDENTICAL: rebuild `rider_restriction` from scratch and confirm the same point.
    sqlx::query("TRUNCATE rider_restriction").execute(&pool).await.expect("truncate rider_restriction");
    sqlx::query("DELETE FROM projection_checkpoint WHERE projector = 'RiderRestriction'")
        .execute(&pool)
        .await
        .expect("reset the RiderRestriction checkpoint");
    run_once(&pool).await;
    delivery_handback_watch_tick(&pool, THRESHOLD).await.expect("a tick after rebuild");
    let points_after_rebuild =
        gauge_points(&spy).points(metric::RIDER_RESTRICTED_HOLDING_JOB_AGE_SECONDS);
    assert_eq!(points_after_rebuild.len(), 1);
    let (_, value_rebuilt) = points_after_rebuild[0];
    assert!(
        (value_rebuilt - value_3b).abs() < 10.0,
        "a from-zero replay must reach the SAME point as before the rebuild: before={value_3b} after={value_rebuilt}"
    );

    // ── (4) LAST, because it destroys the schema: the tick must return Err, and the `?`-before-
    // heartbeat ordering means the heartbeat must NOT fire.
    sqlx::query("DROP TABLE rider_restriction").execute(&pool).await.expect("drop rider_restriction");
    let result = delivery_handback_watch_tick(&pool, THRESHOLD).await;
    assert!(result.is_err(), "a missing rider_restriction table must fail the tick, not silently read 0");
    let s = gauge_points(&spy);
    assert!(
        s.points(metric::DELIVERY_HANDBACK_SWEEP_HEARTBEAT_TOTAL).is_empty(),
        "the heartbeat must not fire on a failed tick -- the `?`-before-heartbeat ordering"
    );
}
