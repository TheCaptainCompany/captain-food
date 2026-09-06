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
//!
//! **#876 round**: `drain_group`'s short-page return (`batch_len < batch_size`, the ordinary "last
//! page" case) never took a confirmatory empty scan, so the LAST recorded value for a backlog that
//! is not an exact multiple of `batch_size` was the PRE-drain pending count, never 0 -- capped at
//! `batch_size` while genuinely behind. Fixed: after a short page, the loop takes ONE MORE scan; a
//! 0 is recorded ONLY by a scan that observed nothing pending (never inferred at the short-page
//! return itself, which would under-report exactly at Friday peak). Separately, the idle gate
//! (`last_head == head`) short-circuited `tick` before any group was scanned, so an idle platform
//! never re-recorded anything after its first drain -- fixed to re-record a literal 0 for every
//! gauge-bearing group on the short-circuit (exact, since pending is 0 by construction; the gate
//! still skips the DB scan, per ADR-20260810-231300's bandwidth carve-out).
//!
//! Four tests now share this ONE process: `telemetry::meters`' instruments bind to whichever
//! provider is GLOBAL at the moment of the FIRST metric emission in the binary, and a LATER
//! `set_meter_provider` call cannot re-point an instrument already created -- so the SECOND and
//! THIRD test functions to install their OWN provider would be silently ignored. [`spy`] installs
//! the pair exactly ONCE (`OnceLock`) and every test acquires [`common::TestDb`] FIRST, before
//! touching metrics at all: `TestDb`'s binary-wide `DB_GATE` mutex is held for the whole test body,
//! so no two tests can straddle each other's `reset()` / `record()` / `force_flush()` window.

#[path = "main/common.rs"]
mod common;

use infrastructure::ProjectionWorker;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, SdkMeterProvider};
use std::sync::OnceLock;

/// The spy provider/exporter pair, installed ONCE per process -- see the module doc for why every
/// test in this binary must share it rather than installing its own.
fn spy() -> &'static (SdkMeterProvider, InMemoryMetricExporter) {
    static SPY: OnceLock<(SdkMeterProvider, InMemoryMetricExporter)> = OnceLock::new();
    SPY.get_or_init(|| {
        let exporter = InMemoryMetricExporter::default();
        let provider = SdkMeterProvider::builder().with_periodic_exporter(exporter.clone()).build();
        opentelemetry::global::set_meter_provider(provider.clone());
        (provider, exporter)
    })
}

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
    let Some(db) = common::TestDb::acquire("rider_standing_lag_metric").await else { return };
    let pool = db.pool();
    let (provider, exporter) = spy();
    exporter.reset();

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

/// #876 pin (a): the DEFAULT batch size is what production actually runs (`DEFAULT_BATCH_SIZE`,
/// far above any realistic single-tick backlog), so this is the case the contract note promises
/// for -- unlike the test above, which forces `.with_batch_size(2)` to make the backlog an exact
/// multiple on purpose. ONE seeded fact under the default batch: `drain_group`'s only page is
/// short (`1 < DEFAULT_BATCH_SIZE`) and non-empty, so before the fix the loop returned right there
/// -- the gauge's last-recorded value stayed at the PRE-drain count (1), never 0. Seen RED verbatim
/// at f676e8c: `assertion `left == right` failed: ... left: Some(1), right: Some(0)`.
#[tokio::test]
async fn the_rider_group_lag_gauge_reads_zero_under_the_default_batch_size() {
    let Some(db) =
        common::TestDb::acquire("rider_standing_lag_metric_default_batch_size").await
    else {
        return;
    };
    let pool = db.pool();
    let (provider, exporter) = spy();
    exporter.reset();

    let rider_id = uuid::Uuid::new_v4();
    append_event(
        &pool,
        &format!("Rider-{rider_id}"),
        1,
        "RiderRegistered",
        serde_json::json!({
            "riderId": rider_id, "authRef": "auth-lag-default", "displayName": "Lag Rider",
            "phone": "+33611119999", "status": "OFFLINE"
        }),
    )
    .await;

    // No `.with_batch_size(...)` override -- the DEFAULT the production worker runs with.
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once");

    provider.force_flush().expect("flush the spy reader");
    let points = lag_points(&exporter);
    assert!(!points.is_empty(), "the Rider group must have emitted at least one lag reading");
    assert_eq!(
        points.last(),
        Some(&0),
        "a single-fact backlog under the default batch size must still end at 0, not capped at \
         the pre-drain pending count: {points:?}"
    );
}

/// #876 pin (b), D3: `run_once`'s idle gate (`last_head == head`) skips every group's DB scan --
/// a deliberate bandwidth decision (ADR-20260810-231300's carve-out) -- but before the fix it also
/// skipped RE-RECORDING the gauge, so an idle platform's dashboard never refreshed after the first
/// drain and would keep showing whatever it last held.
///
/// **Why this pokes the instrument directly, bypassing `drain_group`.** The SDK's default Gauge
/// aggregation is CUMULATIVE (`Temporality::Cumulative`, `opentelemetry_sdk`'s own default): a
/// collection always re-reports the LAST value an instrument was ever given, whether or not
/// anything recorded again since the previous collection -- there is no "went stale, stopped
/// exporting" signal to observe here (that pinned claim -- UNVERIFIED input, dba -- does not hold
/// for this SDK's default temporality; corrected in the hand-back). Because idle always means
/// truly-caught-up (pending is 0 by construction), the CORRECT re-recorded value and a STALE,
/// never-touched-again value would coincide at 0 and the defect would be invisible to a plain
/// before/after read. So this test manufactures a value the fix must OVERWRITE: it pokes the same
/// instrument the drain writes to with a value the drain would never itself produce (`999`,
/// simulating some other stale reading surviving from before this idle window), THEN runs the idle
/// pass. If D3 fires, the literal-0 re-record overwrites the poke; if it does not, the poke survives
/// unchanged into the next flush. Seen RED at f676e8c: `points.last()` was `Some(999)`, the poke,
/// never touched by the idle pass.
#[tokio::test]
async fn the_rider_group_lag_gauge_is_re_recorded_on_an_idle_pass() {
    let Some(db) = common::TestDb::acquire("rider_standing_lag_metric_idle_pass").await else {
        return;
    };
    let pool = db.pool();
    let (provider, exporter) = spy();
    exporter.reset();

    let worker = ProjectionWorker::new(pool.clone());

    // First pass: nothing pending anywhere -- drains every group to an immediate empty scan and
    // arms the idle gate (`last_head` stores the observed head).
    worker.run_once().await.expect("run_once (arm the idle gate)");
    provider.force_flush().expect("flush the spy reader");
    exporter.reset();

    // Poke a value `drain_group` would never itself produce -- surviving proof, on the next
    // flush, that nothing touched the instrument again unless the idle path does.
    telemetry::meters::rider_restriction::lag(999);
    provider.force_flush().expect("flush the spy reader");
    exporter.reset(); // isolate the IDLE PASS's own recording from the poke's own point

    // Second pass: the log has not moved -- `tick` short-circuits on `last_head == head` before
    // scanning any group. D3: the gate must still re-record a literal 0 for every gauge-bearing
    // group (exact -- pending is 0 by construction, no query needed) -- overwriting the poke.
    worker.run_once().await.expect("run_once (idle pass)");
    provider.force_flush().expect("flush the spy reader");
    let points = lag_points(&exporter);
    assert_eq!(
        points.last(),
        Some(&0),
        "the idle gate must re-record a literal 0 on every pass it short-circuits, overwriting a \
         stale reading, not leave it in place: {points:?}"
    );
}

/// #876 pin (c), D2: a zero is only recorded by a scan that observed nothing pending, generalized
/// beyond the single-page case above -- a backlog that spans SEVERAL full pages before a final,
/// non-empty short one (batch size 2, three facts: pages of 2 then 1) must still end at 0, and the
/// checkpoint must have actually advanced past every seeded fact when it does (never a premature
/// "0" that masks an unfolded event). `RiderRow` existing for all three riders is the load-bearing
/// half of that: it proves the LAST batch was folded and committed BEFORE the confirmatory scan
/// that recorded 0, not skipped in favor of inferring 0 at the short page itself.
///
/// **What this does not, and cannot, prove**: distinguishing "0 recorded because an empty scan
/// observed the database live" from "0 inferred at the short-page return because that page HAPPENS
/// to be the true end" is invisible to a value-based assertion in a single-writer test -- a short
/// page IS tautologically the true end absent a concurrent writer, so both implementations reach
/// the identical final value here. D2's actual justification (a writer racing the short-page
/// return) is a genuine, undischarged gap, exactly the class the module doc already names for the
/// ">0 while behind" reading -- not attempted here for the same reason (a flaky gate, not a proof).
#[tokio::test]
async fn a_zero_is_only_recorded_by_a_scan_that_observed_nothing_pending() {
    let Some(db) =
        common::TestDb::acquire("rider_standing_lag_metric_multi_page_short_tail").await
    else {
        return;
    };
    let pool = db.pool();
    let (provider, exporter) = spy();
    exporter.reset();

    let mut rider_ids = Vec::new();
    for n in 0..3u8 {
        let rider_id = uuid::Uuid::new_v4();
        rider_ids.push(rider_id);
        append_event(
            &pool,
            &format!("Rider-{rider_id}"),
            1,
            "RiderRegistered",
            serde_json::json!({
                "riderId": rider_id, "authRef": format!("auth-lag-tail-{n}"), "displayName": "Lag Rider",
                "phone": "+33611119999", "status": "OFFLINE"
            }),
        )
        .await;
    }

    // Batch size 2, three facts: a full page (2) then a short, non-empty final page (1).
    ProjectionWorker::new(pool.clone()).with_batch_size(2).run_once().await.expect("run_once");

    provider.force_flush().expect("flush the spy reader");
    let points = lag_points(&exporter);
    assert_eq!(
        points.last(),
        Some(&0),
        "a multi-page drain ending in a short, non-empty final page must still end at 0: {points:?}"
    );

    let folded: i64 = sqlx::query_scalar("SELECT count(*) FROM rider WHERE rider_id = ANY($1)")
        .bind(&rider_ids[..])
        .fetch_one(&pool)
        .await
        .expect("count folded riders");
    assert_eq!(folded, 3, "the 0 must follow every fact actually being folded, never precede it");
}
