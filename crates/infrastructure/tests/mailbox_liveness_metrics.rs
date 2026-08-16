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
const LANES: &[(&str, &str)] = &[("Order", "OrderAcceptanceTimedOut"), ("Order", "OrderExpired")];

/// One expected attribute set.
fn attrs<const N: usize>(kvs: [(&str, &str); N]) -> BTreeMap<String, String> {
    kvs.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
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
        vec![(attrs([("actor_type", "Order")]), 0.0)],
        "the dead-man's switch records 0 when nothing is due -- an absent point is what a DEAD \
         watcher looks like, and the two must never be confusable"
    );
    assert_eq!(
        empty.records(metric::REMINDER_PROMOTION_DUE_LAG_MS),
        vec![(attrs([("actor_type", "Order")]), 1)],
        "exactly ONE record per lane per tick: the alert is on the increment, so a tick that \
         records twice (or not at all) breaks the heartbeat's arithmetic"
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
        1,
        "one lag point per actor type, whatever the number of due rows: {lag:?}"
    );
    assert_eq!(lag[0].0, attrs([("actor_type", "Order")]), "the lag is keyed by actor type alone");
    assert!(
        lag[0].1 >= 60_000.0,
        "a reminder 90s overdue must show as ~90000ms of lag, not {}ms -- the value IS the alarm: \
         a watcher that always reports 0 is indistinguishable from a promotion pass that is \
         keeping up, which is exactly the failure it is meant to catch",
        lag[0].1
    );
}
