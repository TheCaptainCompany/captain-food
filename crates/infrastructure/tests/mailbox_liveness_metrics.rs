//! **The mailbox's dead-man's switches, proved by looking at the series** (#589, the harness half
//! of #598).
//!
//! `promotion_watch_tick` is the reminder-promotion DEAD-MAN'S SWITCH (#167,
//! ADR-20260810-231300's monitoring carve-out): a sampler outside the worker it watches, emitting
//! `reminder_promotion_due_lag_ms{actor_type}` and `mailbox_scheduled_depth{actor_type,purpose}`
//! on EVERY tick for every DECLARED lane, **zero included**, so that a silent series means the
//! watcher is dead rather than "nothing was scheduled".
//!
//! Its only test until now ended at `.expect("one watch tick over the real schema")`
//! (`tests/main/mailbox_acceptance_timeout.rs`). That proves the SQL still matches the DDL — a
//! real thing, kept — and nothing at all about the emission: deleting the zero-seeding reds
//! nothing, so the switch that is supposed to scream could be mute and every gate stays green. A
//! monitor is verified when a mutation that SILENCES it goes red; this binary is where that
//! becomes true.
//!
//! **What is asserted, and why in this shape:**
//!
//! - The **full point set of one tick over an EMPTY backlog, by equality** — not `contains`. The
//!   contract is "every declared lane reports, zero included"; `contains` cannot see a lane that
//!   stopped reporting, which is the exact failure the series exists to expose.
//! - A **positive control**: one DUE row must move the lag off zero and the depth to 1. Without
//!   it, a watcher that emits a hard-coded 0.0 forever passes the paragraph above — the assertion
//!   would be vacuous, and a monitor emitting garbage is worse than one emitting nothing, because
//!   it looks alive.
//! - **Nothing here calls `telemetry::meters::*` directly.** The only thing driven is
//!   `promotion_watch_tick`; a test that emits the metric itself and then finds it is the
//!   tautology #588's deleted `enqueue_birth` crutch was, in a new costume. Same reason the ticks
//!   are called directly instead of `spawn_promotion_watch` + a sleep: a spawned watcher makes the
//!   test's own timing, not the watcher's contract, the thing under test.
//! - The backlog is asserted EMPTY before the empty tick, so a zero point means "the watcher
//!   seeded it" and not "there happened to be nothing to count".
//! - **TWO ticks over the UNCHANGED empty backlog, asserting the SAME point set both times**
//!   (#598, beck on phase 1's blind spot). Delta temporality plus a draining read makes a watcher
//!   that seeds ONCE AT STARTUP drain identically to a correct one on the first tick — and *every
//!   tick* is the entire dead-man's-switch claim. The second drain is the only assertion that can
//!   tell them apart: a once-only emitter yields an empty set there and nowhere else.
//!
//! **The Order-lane switch (#598) rides this binary** for the same one-provider-per-process
//! reason, and adds the two shapes its own contract needs: a MONOTONIC counter must show a fresh
//! increment on the second drain, and the lane set the watcher reports on is pinned to the
//! GENERATED `ROUTED_LANES` **by equality in both directions** — a `contains` pin leaves an ADDED
//! lane green, which is the direction that matters when someone declares a new routed `deliver:`
//! and never widens the watch.
//!
//! **The fleet-parity gauge (#598) rides it too**, driven from the `standalone_deps` COMPOSITION
//! ROOT. Driving a composition root is not the tautology the bullet above rules out: the root
//! resolves the flag values from the environment the way a deployed worker fleet does, and nothing
//! here calls `telemetry::meters::*`. Without it, failing to REGISTER the observable gauge in
//! `declare_flag` was a green mutation that silenced the only monitor able to see a split fleet.
//!
//! **Why its own binary, with exactly ONE `#[tokio::test]`**: `telemetry::meters` binds the global
//! meter once per process (`OnceLock`), so the spy provider must be installed before the FIRST
//! metric call in the process — unachievable in the ~30-suite `main` binary, and unstable with a
//! second test fn racing the same binding. Same constraint, same shape, same reason as
//! `tests/orders_placed_metric.rs`. The harness itself lives in `tests/main/spy_meter.rs` and is
//! INCLUDED, not copied, so the retrofit of `orders_placed_metric.rs` onto it is a mechanical
//! follow-up rather than a reconciliation of two drains.

// The TestDb witness, shared with the `main` binary by path rather than duplicated (#335,
// ADR-20260808-224500 item 5) — one migration chain, one reset, one serialization gate.
#[path = "main/common.rs"]
mod common;
// The metric-assertion harness, likewise shared by path.
#[path = "main/spy_meter.rs"]
mod spy_meter;

use std::collections::BTreeMap;

use telemetry::contract::metric;

/// The lanes the promotion watch must report on, spelled out rather than derived from
/// `REMINDER_SCHEDULES`: an expectation computed from the same table the watcher reads would agree
/// with the watcher by construction. [`declared_lanes_are_still_the_ones_asserted`] keeps the
/// literal honest and tells a future lane-adder exactly what to update.
/// NOTE for the next lane-adder: the list is SORTED, because the assertion compares against a
/// sorted+deduped view of `REMINDER_SCHEDULES`. `Customer/CustomerErasureDue` (#708) is the lane
/// whose silence is least tolerable of the three: the other two fire on orders that someone is
/// actively waiting for and will chase, while an erasure whose due-reminder never lands is a
/// statutory deadline passing with nobody on either side aware of it.
const LANES: &[(&str, &str)] = &[
    ("Customer", "CustomerErasureDue"),
    ("Order", "OrderAcceptanceTimedOut"),
    ("Order", "OrderExpired"),
];

/// The ROUTED-BIRTH lanes the Order-lane watch must report on (#598) — same literal-not-derived
/// discipline as [`LANES`], pinned by [`routed_lanes_are_still_the_ones_asserted`].
const ROUTED: &[&str] = &["Order"];

/// One expected attribute set.
fn attrs<const N: usize>(kvs: [(&str, &str); N]) -> BTreeMap<String, String> {
    kvs.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// The dead-man's-switch point set: ONE point per declared ACTOR TYPE (the lag is keyed by actor
/// type alone, not by lane), each at `value`. Derived from [`LANES`] rather than spelled out, for
/// the reason the depth helper is: a new reminder on a NEW actor silently widens this series, and
/// a hard-coded literal would then fail three assertions at once with a point-set diff that names
/// no cause. `declared_lanes_are_still_the_ones_asserted` remains the one place a lane change is
/// explained.
fn expected_lag<T: Copy>(value: T) -> Vec<(BTreeMap<String, String>, T)> {
    let mut actors: Vec<&str> = LANES.iter().map(|(actor, _)| *actor).collect();
    actors.sort();
    actors.dedup();
    actors.into_iter().map(|a| (attrs([("actor_type", a)]), value)).collect()
}

/// The depth point set the watcher owes for a backlog where only `due` lanes carry rows: every
/// declared lane present, the ones with no rows at zero.
fn expected_depth(due: &[(&str, &str, f64)]) -> Vec<(BTreeMap<String, String>, f64)> {
    let mut out: Vec<_> = LANES
        .iter()
        .map(|(actor, purpose)| {
            let depth = due
                .iter()
                .find(|(a, p, _)| a == actor && p == purpose)
                .map(|(_, _, d)| *d)
                .unwrap_or(0.0);
            (attrs([("actor_type", actor), ("purpose", purpose)]), depth)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
    out
}

/// The literal [`LANES`] against the generated declaration. Offline, so it runs even without a
/// database: adding a reminder lane in `specs/**` reds HERE, with an instruction, instead of
/// making the equality assertion below fail as an unexplained point-count mismatch.
#[test]
fn declared_lanes_are_still_the_ones_asserted() {
    let mut declared: Vec<(&str, &str)> = application::generated::reminders::REMINDER_SCHEDULES
        .iter()
        .map(|s| (s.actor_type, s.payload_event))
        .collect();
    declared.sort();
    declared.dedup();
    assert_eq!(
        declared,
        LANES.to_vec(),
        "a reminder lane was added or removed in specs/**: add it to LANES here, so the emptiness \
         assertions below keep covering EVERY declared lane (that coverage is the contract)"
    );
}

/// The point set the Order-lane watch owes for a backlog where only `pending` lanes carry rows.
/// Every declared routed lane present, the drained ones at zero — the heartbeat's point set is the
/// same shape with a fixed value of 1 (one increment per lane per tick).
fn expected_lane_points(
    lanes: &[&str],
    pending: &[(&str, f64)],
) -> Vec<(BTreeMap<String, String>, f64)> {
    let mut out: Vec<_> = lanes
        .iter()
        .map(|lane| {
            let age = pending.iter().find(|(l, _)| l == lane).map(|(_, a)| *a).unwrap_or(0.0);
            (attrs([("lane", lane)]), age)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
    out
}

/// The literal [`ROUTED`] against the GENERATED declaration, by EQUALITY IN BOTH DIRECTIONS.
///
/// Both directions is the whole point and `contains` would be useless here: a missing lane is
/// caught by the point-set assertions below anyway, but an ADDED lane — someone declares a second
/// routed `deliver:` and nobody widens the watch — is exactly the case where a one-way pin stays
/// green while a production lane goes unwatched.
#[test]
fn routed_lanes_are_still_the_ones_asserted() {
    let mut declared: Vec<&str> = application::generated::process_managers::ROUTED_LANES
        .iter()
        .map(|l| l.actor_type)
        .collect();
    declared.sort_unstable();
    declared.dedup();
    assert_eq!(
        declared,
        ROUTED.to_vec(),
        "a routed `deliver:` lane was added or removed in specs/** (see \
         tools/codegen-rs PM_LANE_ROUTED_DELIVERS): add it to ROUTED here, so the liveness \
         assertions below keep covering EVERY declared lane — that coverage IS the contract, and \
         a lane nobody widened the watch to is a lane whose silence means nothing"
    );
    assert_eq!(
        declared,
        infrastructure::mailbox::declared_lanes(),
        "the watcher's own lane set must be the declared one, not a second hand-kept list"
    );
}

/// The equality assertion is SENSITIVE TO LANE MEMBERSHIP, not merely to non-emptiness.
///
/// Without this, "assert the full point set" is a claim about the harness that nobody has checked:
/// a comparison satisfied by any vector of the right length would pass every assertion below while
/// a lane silently stopped reporting. Offline — it tests the expectation builder, not the database.
#[test]
fn a_lane_missing_from_the_emission_fails_the_equality() {
    let full = expected_lane_points(ROUTED, &[]);
    let short = expected_lane_points(&ROUTED[..ROUTED.len() - 1], &[]);
    assert_ne!(
        full, short,
        "dropping a lane from the emitted set must NOT compare equal to the full set -- if it \
         does, every point-set assertion in this binary is vacuous"
    );
}

/// The point set `runtime_flag_state` owes for one process: every declared flag present, each
/// labelled with the value THIS process resolved, all observing `1` (the value is a label, not the
/// measurement — see `telemetry::meters::runtime`).
///
/// `bin` comes from [`telemetry::meters::runtime::current_bin`] rather than a literal because the
/// test binary's own file name carries a build hash; the flag VALUES, which are the thing under
/// test, come from the resolved `CommandDeps` and never from a literal `false` — a hard-coded
/// expectation would both red spuriously under a set env var and pass a gauge that reports a
/// constant instead of the decision.
fn expected_flag_points(bin: &str, flags: &[(&str, bool)]) -> Vec<(BTreeMap<String, String>, f64)> {
    let mut out: Vec<_> = flags
        .iter()
        .map(|(flag, value)| {
            (attrs([("flag", *flag), ("value", &value.to_string()), ("bin", bin)]), 1.0_f64)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
    out
}

/// One message sitting UNDELIVERED on the Order lane, received 90 s ago: the gauge's positive
/// control. Without it, a watcher hard-coding `0` forever satisfies every emptiness assertion —
/// a monitor that lies while looking alive, which is worse than one that says nothing.
async fn park_one_pending_order_lane_message(pool: &sqlx::PgPool) {
    let order_id = uuid::Uuid::from_u128(0x0B17);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, \
            payload_hash, channel, user_type, correlation_id, received_at, status) \
         VALUES ($1, 'EVENT', 'Order', $1, 0, 'OrderPlaced', $2, 'h-birth', 'WORKER', \
                 'EXTERNAL', $1, now() - interval '90 seconds', 'RECEIVED')",
    )
    .bind(order_id)
    .bind(serde_json::json!({
        "eventType": "OrderPlaced",
        "payload": { "orderId": order_id },
    }))
    .execute(pool)
    .await
    .expect("park one undelivered Order-lane birth");
}

/// Schedule one DUE acceptance-timeout reminder: the positive control's single row.
async fn schedule_one_due_reminder(pool: &sqlx::PgPool) {
    let order_id = uuid::Uuid::from_u128(0x0DE1);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, \
            payload_hash, channel, user_type, correlation_id, scheduled_at, status) \
         VALUES ($1, 'MESSAGE', 'Order', $1, 0, 'OrderAcceptanceTimedOut', $2, 'h-due', 'WORKER', \
                 'EXTERNAL', $1, now() - interval '90 seconds', 'SCHEDULED')",
    )
    .bind(order_id)
    .bind(serde_json::json!({
        "eventType": "OrderAcceptanceTimedOut",
        "payload": { "orderId": order_id },
    }))
    .execute(pool)
    .await
    .expect("schedule one DUE acceptance timeout");
}

/// The one metric test this binary may hold (see the head comment). Two ticks against real
/// Postgres: an empty backlog, then the same backlog with one DUE row.
#[tokio::test]
async fn promotion_watch_emits_both_liveness_series_for_every_declared_lane_zeros_included() {
    // The spy provider FIRST — before the database, before anything that could bind the
    // process-wide meter to the no-op provider and make this whole binary observe nothing.
    let spy = spy_meter::SpyMeter::install();

    let Some(db) = common::TestDb::acquire("mailbox_liveness_metrics").await else { return };
    let pool = db.pool();

    // Tautology fence: "zero" below must mean the watcher SEEDED a zero, never that the query
    // happened to find nothing to report on an empty table.
    let scheduled: i64 =
        sqlx::query_scalar("SELECT count(*) FROM inbound_messages WHERE status = 'SCHEDULED'")
            .fetch_one(&pool)
            .await
            .expect("count the SCHEDULED backlog");
    assert_eq!(scheduled, 0, "the empty-backlog tick must run against an actually empty backlog");

    infrastructure::mailbox::promotion_watch_tick(&pool)
        .await
        .expect("one watch tick over an empty backlog");
    let empty = spy.drain();

    assert_eq!(
        empty.points(metric::MAILBOX_SCHEDULED_DEPTH),
        expected_depth(&[]),
        "an empty backlog still reports EVERY declared lane at zero -- a lane that reports only \
         when it has rows is a lane whose silence is ambiguous (ADR-20260810-231300)"
    );
    assert_eq!(
        empty.points(metric::REMINDER_PROMOTION_DUE_LAG_MS),
        expected_lag(0.0),
        "the dead-man's switch records 0 when nothing is due -- an absent point is what a DEAD \
         watcher looks like, and the two must never be confusable"
    );
    assert_eq!(
        empty.records(metric::REMINDER_PROMOTION_DUE_LAG_MS),
        expected_lag(1),
        "exactly ONE record per lane per tick: the alert is on the increment, so a tick that \
         records twice (or not at all) breaks the heartbeat's arithmetic"
    );

    // EVERY TICK, not once (beck, on phase 1's blind spot). Delta temporality plus a draining
    // read makes a watcher that seeds at STARTUP indistinguishable from one that seeds per tick
    // on the first drain — and "every tick" IS the dead-man's-switch claim. A second tick over
    // the UNCHANGED empty backlog must produce the SAME point set; a once-only emitter produces
    // an empty one here and nowhere else.
    infrastructure::mailbox::promotion_watch_tick(&pool)
        .await
        .expect("a SECOND watch tick over the same empty backlog");
    let empty_again = spy.drain();
    assert_eq!(
        empty_again.points(metric::MAILBOX_SCHEDULED_DEPTH),
        expected_depth(&[]),
        "the second tick over an unchanged backlog must re-report every lane -- a watcher that \
         seeds once at startup passes the first drain and is dead from then on"
    );
    assert_eq!(
        empty_again.points(metric::REMINDER_PROMOTION_DUE_LAG_MS),
        expected_lag(0.0),
        "the dead-man's switch must RE-ASSERT on every tick: a signal emitted once proves only \
         that the process started"
    );

    // Positive control — without it every assertion above is satisfied by a watcher that emits a
    // hard-coded zero forever, i.e. by a monitor that lies while looking alive.
    schedule_one_due_reminder(&pool).await;
    infrastructure::mailbox::promotion_watch_tick(&pool)
        .await
        .expect("one watch tick over a backlog with one DUE row");
    let due = spy.drain();

    assert_eq!(
        due.points(metric::MAILBOX_SCHEDULED_DEPTH),
        expected_depth(&[("Order", "OrderAcceptanceTimedOut", 1.0)]),
        "the due lane reports depth 1 and every OTHER declared lane still reports zero"
    );
    let lag = due.points(metric::REMINDER_PROMOTION_DUE_LAG_MS);
    assert_eq!(
        lag.len(),
        expected_lag(0.0).len(),
        "one lag point per DECLARED actor type, whatever the number of due rows -- and the actors \
         with nothing due still report, which is the whole dead-man's-switch claim: {lag:?}"
    );
    let order_lag = lag
        .iter()
        .find(|(a, _)| a == &attrs([("actor_type", "Order")]))
        .expect("the lag is keyed by actor type alone, and Order has the overdue row");
    assert!(
        order_lag.1 >= 60_000.0,
        "a reminder 90s overdue must show as ~90000ms of lag, not {}ms -- the value IS the alarm: \
         a watcher that always reports 0 is indistinguishable from a promotion pass that is \
         keeping up, which is exactly the failure it is meant to catch",
        order_lag.1
    );
    // The actors with NOTHING due must still read zero in the same drain. Without this, a watcher
    // that reported only the actor it found work for would satisfy every assertion above -- and
    // that is precisely the defect class ADR-20260810-231300 names, where a threshold alert goes
    // quiet exactly when it should scream.
    for (attr, value) in &lag {
        if attr != &attrs([("actor_type", "Order")]) {
            assert_eq!(*value, 0.0, "an actor with nothing due must report 0, not be absent: {attr:?}");
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────────────────
    // #598 — the ROUTED-BIRTH lane dead-man's switch, in the same binary because the process's
    // meter provider can only be bound once (see the head comment).
    //
    // The contract being proved, in the observability lens's words: exact name, exact attribute
    // set, exact point COUNT and value per declared lane on an empty backlog, plus a
    // missing-lane-fails assertion — return-value assertions do not count. And the flag is OFF
    // throughout: these series must be alive BEFORE the flip, or they are proved by the flip.
    // ─────────────────────────────────────────────────────────────────────────────────────────
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM inbound_messages WHERE status = 'RECEIVED' AND actor_type = 'Order'",
    )
    .fetch_one(&pool)
    .await
    .expect("count the Order lane's undelivered backlog");
    assert_eq!(pending, 0, "the drained-lane tick must run against an actually drained lane");

    infrastructure::mailbox::order_lane_watch_tick(&pool)
        .await
        .expect("one order-lane watch tick over a drained lane");
    let drained = spy.drain();

    assert_eq!(
        drained.points(metric::ORDER_LANE_WATCH_HEARTBEAT_TOTAL),
        expected_lane_points(ROUTED, &[("Order", 1.0)]),
        "every DECLARED routed lane heartbeats on every tick, drained or not -- this counter is \
         the only thing that tells 'nobody ordered' from 'the Order lane worker is dead', and \
         order_birth_lag_ms is silent by design while ROUTE_ORDER_BIRTH_THROUGH_LANE is OFF"
    );
    assert_eq!(
        drained.points(metric::ORDER_LANE_OLDEST_PENDING_AGE_MS),
        expected_lane_points(ROUTED, &[]),
        "a drained lane reports age 0, it does not go absent -- and it must NOT be seeded into \
         order_birth_lag_ms, whose p95 the flip is judged on"
    );
    assert_eq!(
        drained.records(metric::ORDER_LANE_WATCH_HEARTBEAT_TOTAL),
        vec![(attrs([("lane", "Order")]), 1)],
        "exactly ONE heartbeat point per lane per tick: the alarm is the ABSENCE of an increment, \
         so a tick that emits twice inflates the very rate the alarm reads"
    );

    // EVERY TICK, again: the counter must show a fresh increment and the gauge must re-report
    // over an unchanged, still-drained lane. Under delta temporality a once-at-startup emitter
    // yields EMPTY here while satisfying every assertion above.
    infrastructure::mailbox::order_lane_watch_tick(&pool)
        .await
        .expect("a SECOND order-lane watch tick over the same drained lane");
    let drained_again = spy.drain();
    assert_eq!(
        drained_again.points(metric::ORDER_LANE_WATCH_HEARTBEAT_TOTAL),
        expected_lane_points(ROUTED, &[("Order", 1.0)]),
        "the heartbeat must INCREMENT on the second tick -- a monotonic counter that never \
         increments is a dead watcher wearing a live series"
    );
    assert_eq!(
        drained_again.points(metric::ORDER_LANE_OLDEST_PENDING_AGE_MS),
        expected_lane_points(ROUTED, &[]),
        "the gauge must RE-REPORT on the second tick -- a gauge that reports once says only that \
         the process started"
    );

    // Positive control for the gauge: a birth parked 90 s on the Order lane. Without it a
    // watcher hard-coding 0 satisfies everything above and reports a wedged lane as healthy —
    // the head-of-line case this series exists to catch at peak.
    park_one_pending_order_lane_message(&pool).await;
    infrastructure::mailbox::order_lane_watch_tick(&pool)
        .await
        .expect("one order-lane watch tick over a lane with an undelivered birth");
    let backlogged = spy.drain();

    let ages = backlogged.points(metric::ORDER_LANE_OLDEST_PENDING_AGE_MS);
    assert_eq!(ages.len(), ROUTED.len(), "one age point per declared lane, always: {ages:?}");
    assert_eq!(ages[0].0, attrs([("lane", "Order")]), "the age is keyed by lane alone");
    assert!(
        ages[0].1 >= 60_000.0,
        "a birth parked 90s on the lane must show as ~90000ms, not {}ms -- the VALUE is the \
         alarm, and a watcher that always reports 0 reads exactly like a lane that is keeping up",
        ages[0].1
    );
    assert_eq!(
        backlogged.points(metric::ORDER_LANE_WATCH_HEARTBEAT_TOTAL),
        expected_lane_points(ROUTED, &[("Order", 1.0)]),
        "the heartbeat is independent of the backlog: it ticks whether or not anything is waiting"
    );

    // ─────────────────────────────────────────────────────────────────────────────────────────
    // #598 — the FLEET-PARITY gauge, driven from the COMPOSITION ROOT.
    //
    // The first cut of this branch claimed `runtime_flag_state` "has no spy test and cannot
    // honestly have one". That was wrong, and the review disproved it by execution. The tautology
    // objection defeats a test that calls `declare_flag` and then finds it; it says nothing about
    // driving `standalone_deps`, which is a REAL composition root, resolves the values from the
    // environment the way a deployed worker fleet does, and is the same discipline the rest of
    // this binary already keeps (nothing here calls `telemetry::meters::*`).
    //
    // What it buys: forgetting `let _ = flag_state_gauge();` in `declare_flag` — i.e. recording
    // the declaration but never registering the observable gauge — shipped GREEN before this
    // block existed. That mutation silences the ONLY monitor that can see a split fleet, and the
    // only evidence for flip-ADR obligation 1(iv). A monitor whose silencing mutation is green is
    // not a monitor.
    // ─────────────────────────────────────────────────────────────────────────────────────────
    let deps = infrastructure::mailbox::standalone_deps(
        &pool,
        std::sync::Arc::new(infrastructure::FailClosedPaymentGateway),
    );
    let expected_flags = expected_flag_points(
        &telemetry::meters::runtime::current_bin(),
        &[
            ("ENFORCE_ACCEPTANCE_TIMEOUT", deps.enforce_acceptance_timeout),
            ("ENFORCE_SERVICE_HOURS_GUARD", deps.enforce_service_hours_guard),
            ("ROUTE_ORDER_BIRTH_THROUGH_LANE", deps.route_order_birth_through_lane),
        ],
    );

    let declared = spy.drain();
    assert_eq!(
        declared.points(metric::RUNTIME_FLAG_STATE),
        expected_flags,
        "a composition root must publish EVERY flag whose split across a fleet has a consequence, \
         labelled with the value it actually resolved -- this series is the sole input to \
         `count(distinct value) by (flag) > 1`, the query a flip is blocked on, so a flag that \
         goes unpublished is a flag whose fleet-wide disagreement is invisible"
    );

    // EVERY EXPORT CYCLE, the same property E and F pin for the two watchers, and the entire
    // reason this is an OBSERVABLE gauge rather than a value written once at boot: a process that
    // dies must stop contributing its value with no timer of ours. A callback that fires once
    // yields the full set on the first drain and an empty one here.
    let declared_again = spy.drain();
    assert_eq!(
        declared_again.points(metric::RUNTIME_FLAG_STATE),
        expected_flags,
        "the parity gauge must RE-ASSERT on every export cycle -- a value published once says \
         only that this process once started, and the fleet it is meant to describe changes \
         precisely during the rolling deploy that this series exists to watch"
    );
}
