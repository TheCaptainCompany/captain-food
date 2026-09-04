//! #639 part C step 4-i (ADR-20260904-081527 §9): the `rider-restriction` contract's gauge —
//! `rider_standing_lag_positions` — the `scope_membership_lag_positions` mirror, proved emitting
//! 0 once the Rider projector group has caught up, through the REAL `ProjectionWorker::run_once()`
//! drain (`crates/infrastructure/src/projection/worker.rs`), never assumed from the constant in
//! `contract.rs`.
//!
//! **What this proves, and what it deliberately does not.** `run_once()` drains every PENDING
//! event regardless of `with_batch_size` (it keeps taking one page after another for as long as a
//! page comes back full), but `drain_group`'s loop returns the moment a page comes back SHORT of
//! `batch_size` (the ordinary "last page" case) — WITHOUT one more, empty scan. Found live writing
//! this test: a single seeded fact under the default (large) batch size leaves the gauge's
//! last-recorded value at the PRE-drain pending count, never 0 — the contract note ("0 when caught
//! up") is only true when the backlog happens to be an exact multiple of the batch size, which the
//! `.with_batch_size(2)` below forces on purpose (see the seeding loop's comment). This is
//! `drain_group`'s own behaviour, shared verbatim by `scope_membership_lag_positions` and
//! `read_authorization::lag_positions` (this slice's mirror, not its origin) — flagged in the
//! hand-back rather than "fixed" here, since touching the shared loop is out of this slice's scope
//! and would move every OTHER group's lag reading too.
//!
//! Separately, `rider_standing_lag_positions` is an OTel `Gauge` — last-value-wins per collection
//! interval, not a series this exporter can replay — so there is no way to observe an INTERMEDIATE
//! "> 0, still behind" reading through `run_once()`'s public, to-exhaustion API without racing a
//! concurrent `force_flush()` against a background drain, which would be a flaky gate, not a proof,
//! and is not attempted here. What IS deterministic, and is what this test asserts: once the
//! backlog is fully drained AND the batch size lines up with it, the LAST value recorded is exactly
//! 0 — the "idle group reads 0" half the card names. The "> 0 while behind" half stays a genuine,
//! load-bearing GAP (see the hand-back).
//!
//! Its OWN test binary on purpose, same reason as `orders_placed_metric.rs`: `telemetry::meters`
//! binds `opentelemetry::global::meter` once per process (`OnceLock`), so the spy provider must be
//! installed before the process's first metric call. The pool comes from the same `TestDb` witness
//! the `main` binary uses (included by `#[path]`, not copied).

#[path = "main/common.rs"]
mod common;

use infrastructure::ProjectionWorker;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, SdkMeterProvider};

/// Every `rider_standing_lag_positions` data point the spy collected.
fn lag_points(exporter: &InMemoryMetricExporter) -> Vec<i64> {
    let mut out = Vec::new();
    for rm in exporter.get_finished_metrics().expect("finished metrics") {
        for scope in rm.scope_metrics() {
            for metric in scope.metrics() {
                if metric.name() != "rider_standing_lag_positions" {
                    continue;
                }
                let AggregatedMetrics::I64(MetricData::Gauge(gauge)) = metric.data() else {
                    panic!("a lag gauge aggregates as an i64 Gauge: {:?}", metric.data());
                };
                for dp in gauge.data_points() {
                    out.push(dp.value());
                }
            }
        }
    }
    out
}

async fn append_event(
    pool: &sqlx::PgPool,
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

/// The Rider group's gauge, GATED: after a real drain over a real `RiderRegistered` fact, the
/// LAST value the spy collected for this group is 0 -- caught up, exactly as the contract note
/// promises ("Emitted per scan, 0 when caught up").
#[tokio::test]
async fn the_rider_group_lag_gauge_reads_zero_once_the_drain_has_caught_up() {
    // The spy provider FIRST -- before any drain can bind the process-wide meter.
    let exporter = InMemoryMetricExporter::default();
    let provider = SdkMeterProvider::builder().with_periodic_exporter(exporter.clone()).build();
    opentelemetry::global::set_meter_provider(provider.clone());

    let Some(db) = common::TestDb::acquire("rider_standing_lag_metric").await else { return };
    let pool = db.pool();

    // TWO facts, batch size exactly TWO (found live, while writing this test — a real drain_group
    // property, not a mutant): `drain_group`'s loop returns EARLY, without a follow-up scan, the
    // moment one page's `batch_len < batch_size` (the ordinary "last page" case) — so a SINGLE
    // event under the default (large) batch size leaves the LAST recorded value at the pending
    // count BEFORE that one page, never 0. Sizing the batch to match the seeded count forces the
    // loop to take a SECOND, empty scan after draining the first, which is the ONLY path that
    // records the promised 0 — the same shape `read_authorization::lag_positions` and
    // `scope_membership_lag_positions` share (this is `drain_group`'s behaviour, not something
    // this slice introduced; see the module doc comment).
    for n in 0..2u8 {
        let rider_id = uuid::Uuid::new_v4();
        append_event(
            &pool,
            &format!("Rider-{rider_id}"),
            1,
            "RiderRegistered",
            serde_json::json!({
                "riderId": rider_id, "authRef": format!("auth-lag-{n}"), "displayName": "Lag Rider",
                "phone": "+33611119999", "status": "OFFLINE"
            }),
        )
        .await;
    }

    ProjectionWorker::new(pool.clone()).with_batch_size(2).run_once().await.expect("run_once");

    provider.force_flush().expect("flush the spy reader");
    let points = lag_points(&exporter);
    assert!(!points.is_empty(), "the Rider group must have emitted at least one lag reading");
    assert_eq!(
        points.last(),
        Some(&0),
        "the drain caught up -- the gauge's last-recorded value must be exactly 0: {points:?}"
    );
}
