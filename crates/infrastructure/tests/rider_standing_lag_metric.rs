//! #639 part C step 4-i (ADR-20260904-081527 §9) + #876: FOUR gauges share `drain_group`'s loop
//! (`crates/infrastructure/src/projection/worker.rs`) — `scope_membership_lag_positions`,
//! `rider_standing_lag_positions`, `restaurant_roster_lag_positions`,
//! `restaurant_invitation_list_lag_positions` — proved through the REAL
//! `ProjectionWorker::run_once()` drain, never assumed from the constants in `contract.rs`. This
//! file's focus is the Rider mirror; the other three get the ALL-FOUR check in
//! [`the_rider_group_lag_gauge_is_re_recorded_on_an_idle_pass`] below (the issue's original text
//! named three gauges; the fourth, `read_authorization::lag_positions`, emits the
//! `scope_membership_lag_positions` name and shares the exact same defects — see the hand-back).
//!
//! **What #876 found and fixed, and what it deliberately does not prove.** `run_once()` drains
//! every PENDING event regardless of `with_batch_size` (it keeps taking one page after another for
//! as long as a page comes back full). Before this round, `drain_group`'s loop returned the moment
//! a page came back SHORT of `batch_size` (the ordinary "last page" case) WITHOUT one more, empty
//! scan — so a single seeded fact under the default (large) batch size left the gauge's
//! last-recorded value at the PRE-drain pending count, never 0; the contract note ("0 when caught
//! up") was only true when the backlog happened to be an exact multiple of the batch size, which
//! the `.with_batch_size(2)` test below forces on purpose. FIXED (D2): after a short page, the loop
//! takes ONE MORE scan, and a 0 is recorded ONLY by a scan that observed nothing pending — never
//! inferred at the short-page return itself, which would under-report exactly at Friday peak, where
//! the backlog is rarely an exact multiple of `batch_size`. Separately, `tick`'s idle gate
//! (`last_head == head`) short-circuited before any group was scanned, so an idle platform never
//! re-recorded anything after its first drain and a stale reading could sit forever. FIXED (D3): the
//! gate still skips every group's DB scan (ADR-20260810-231300's bandwidth carve-out stands
//! unchanged), but now re-records a literal 0 for every gauge-bearing group on the way out — exact,
//! since the gate only arms once every group has fully drained.
//!
//! Separately, `rider_standing_lag_positions` is an OTel `Gauge` — last-value-wins per collection
//! interval, not a series this exporter can replay — so there is no way to observe an INTERMEDIATE
//! "> 0, still behind" reading through `run_once()`'s public, to-exhaustion API without racing a
//! concurrent `force_flush()` against a background drain, which would be a flaky gate, not a proof,
//! and is not attempted here. The gauge also still SATURATES at `batch_size` while genuinely behind
//! (honest exactly at 0, not above it) — the linked follow-up
//! ([#936](https://github.com/TheCaptainCompany/captain-food/issues/936)) tracks a `head -
//! checkpoint` reading that would not saturate. What IS deterministic, and is what these tests
//! assert: once the backlog is fully drained, the LAST value recorded is exactly 0, on both a fresh
//! drain and every idle pass after it. D2's own justification (a writer racing the short-page
//! return) is a further, undischarged gap — beck confirmed it unpinnable under a single writer (see
//! the hand-back and [#936](https://github.com/TheCaptainCompany/captain-food/issues/936)).
//!
//! Its OWN test binary on purpose, same reason as `orders_placed_metric.rs`: `telemetry::meters`
//! binds `opentelemetry::global::meter` once per process (`OnceLock`), so the spy provider must be
//! installed before the process's first metric call. The pool comes from the same `TestDb` witness
//! the `main` binary uses (included by `#[path]`, not copied).
//!
//! Five tests now share this ONE process: `telemetry::meters`' instruments bind to whichever
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
use telemetry::contract::metric::{
    RESTAURANT_INVITATION_LIST_LAG_POSITIONS, RESTAURANT_ROSTER_LAG_POSITIONS,
    RIDER_STANDING_LAG_POSITIONS, SCOPE_MEMBERSHIP_LAG_POSITIONS,
};

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

/// Every data point the spy collected for the named gauge (any of the four #876 metric names).
fn lag_points(exporter: &InMemoryMetricExporter, metric_name: &str) -> Vec<i64> {
    let mut out = Vec::new();
    for rm in exporter.get_finished_metrics().expect("finished metrics") {
        for scope in rm.scope_metrics() {
            for metric in scope.metrics() {
                if metric.name() != metric_name {
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

    // TWO facts, batch size exactly TWO: the backlog is an exact multiple of the batch, so the
    // FIRST scan already lands on a full page and the loop's SECOND scan (now empty) is the one
    // that records 0 -- the simplest case D2's confirmatory-scan fix handles. #876 pin (a)/(c)
    // below cover the harder cases this test does not: a backlog that is NOT an exact multiple
    // (the default-batch, single-fact case) and a multi-page drain ending in a short final page.
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
    let points = lag_points(&exporter, RIDER_STANDING_LAG_POSITIONS);
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
    let points = lag_points(&exporter, RIDER_STANDING_LAG_POSITIONS);
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
/// drain and would keep showing whatever it last held. Checks ALL FOUR gauge-bearing groups, not
/// only Rider (the issue text named three; the fourth, `read_authorization::lag_positions`, emits
/// `scope_membership_lag_positions` and shares this loop identically).
///
/// **The trade this test makes: it arranges through the PRODUCTION instrument to defeat cumulative
/// temporality, rather than reading around it.** The SDK's default Gauge aggregation is CUMULATIVE
/// (`Temporality::Cumulative`, `opentelemetry_sdk`'s own default): a collection always re-reports
/// the LAST value an instrument was ever given, whether or not anything recorded again since the
/// previous collection -- there is no "went stale, stopped exporting" signal to observe here (that
/// pinned claim -- UNVERIFIED input, dba -- does not hold for this SDK's default temporality;
/// corrected in the hand-back). Because idle always means truly-caught-up (pending is 0 by
/// construction), the CORRECT re-recorded value and a STALE, never-touched-again value would
/// coincide at 0 and the defect would be invisible to a plain before/after read. So this test calls
/// the same `telemetry::meters::*` functions `drain_group` itself calls, directly, with a value
/// `drain_group` would never itself produce (`999`, simulating some other stale reading surviving
/// from before this idle window), THEN runs the idle pass. If D3 fires, the literal-0 re-record
/// overwrites the poke on every gauge; if it does not (or only partially), a poke survives
/// unchanged into the next flush. Seen RED at f676e8c (Rider only, pre-generalization):
/// `points.last()` was `Some(999)`, the poke, never touched by the idle pass. Seen RED again against
/// the D3-partial mutant (drop the re-record for the other three groups, keep Rider's): the other
/// three each read back `Some(999)` while Rider alone read `Some(0)` -- quoted in the hand-back.
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

    // Poke a value `drain_group` would never itself produce, on EVERY gauge-bearing instrument --
    // surviving proof, on the next flush, that nothing touched it again unless the idle path does.
    telemetry::meters::read_authorization::lag_positions(999);
    telemetry::meters::rider_restriction::lag(999);
    telemetry::meters::restaurant_invitation::roster_lag(999);
    telemetry::meters::restaurant_invitation::invitation_list_lag(999);
    provider.force_flush().expect("flush the spy reader");
    exporter.reset(); // isolate the IDLE PASS's own recording from the pokes' own points

    // Second pass: the log has not moved -- `tick` short-circuits on `last_head == head` before
    // scanning any group. D3: the gate must still re-record a literal 0 for every gauge-bearing
    // group (exact -- pending is 0 by construction, no query needed) -- overwriting every poke.
    worker.run_once().await.expect("run_once (idle pass)");
    provider.force_flush().expect("flush the spy reader");
    for name in [
        SCOPE_MEMBERSHIP_LAG_POSITIONS,
        RIDER_STANDING_LAG_POSITIONS,
        RESTAURANT_ROSTER_LAG_POSITIONS,
        RESTAURANT_INVITATION_LIST_LAG_POSITIONS,
    ] {
        let points = lag_points(&exporter, name);
        assert_eq!(
            points.last(),
            Some(&0),
            "gauge {name} must be re-recorded to a literal 0 on every idle pass, overwriting a \
             stale reading, not leave it in place: {points:?}"
        );
    }
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
///
/// Named for what it pins, not for the property it cannot cash: the reviewer's short-page mutant
/// (inferring 0 at the short page itself, never re-scanning) still passes all 5 assertions here,
/// which is exactly the gap above. The undischarged concurrent-writer seam this test's ORIGINAL
/// name claimed ("a zero is only recorded by a scan that observed nothing pending") is carried
/// forward under that name in #936 item 3.
#[tokio::test]
async fn a_multi_page_drain_ending_in_a_short_page_still_ends_at_zero_with_every_fact_folded() {
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
    let points = lag_points(&exporter, RIDER_STANDING_LAG_POSITIONS);
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
