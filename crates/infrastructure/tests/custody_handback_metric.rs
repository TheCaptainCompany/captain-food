//! THE CUSTODY-HANDBACK DEAD-MAN'S SWITCH, proved by looking at the series (#639 part C step
//! 3-ii, ADR-20260904-015903 §8) — mirrors `tests/authorized_no_birth_metric.rs`'s shape (#608)
//! for a different worst failure mode: *a rider handed a job back, stating where the food is, and
//! nobody has re-offered it since.*
//!
//! **The state is manufactured honestly**: `view_deliveryjob` is a pure SQL fold VIEW over
//! `domain_events` (no worker, no projector), so every scenario here is a straight event append —
//! no row is inserted directly. The one thing moved by hand is the clock (`occurred_at`), for the
//! same truncation reason `authorized_no_birth_metric.rs` documents.
//!
//! **What is asserted (beck's zero-healthy suite, the same shape as #608):**
//! - **presence** — an empty population still emits ONE point (this gauge carries no attributes,
//!   unlike the birth-gap's `reason` set) at 0.
//! - **a VALUE-DERIVED positive control, DELIBERATELY MIS-ORDERED (review round 2 on #870,
//!   young)** — a stranded job (no later acceptance) at 1800s AND a re-assigned job whose handback
//!   is aged OLDER, at 3600s. If the "no later acceptance" predicate degraded to "does a handback
//!   exist on this job" (the named mutant below), the gauge would report the re-assigned job's
//!   3600s — the OLDER of the two — not the stranded job's 1800s, and the assertion would read the
//!   wrong number. Ages that happened to be close (the ORIGINAL shape of this test, both jobs'
//!   handbacks landing within seconds of each other in wall-clock terms) let the mutant survive:
//!   a max-of-two that differs by under a second can round to the same reported value either way.
//!   The ordering is the plant now, not just the SQL's own predicate.
//! - **the NAMED mutant: delete the "no later acceptance" predicate** — a job handed back and then
//!   RE-ASSIGNED (a later `DeliveryAcceptedByRider`) must read as NOT stranded, because
//!   `food_location`/`status` reset on the next acceptance (the same `derive: null` fold this
//!   gauge's own SQL leans on). Caught DIRECTLY by the positive control above (its age is the
//!   discriminant); recovery below is a second, independent proof on the SAME job rather than the
//!   mutant's primary witness.
//! - **repetition** — a second tick over the unchanged state must re-emit.
//! - **recovery** — reassigning the stranded job must bring the gauge back to 0 on the next tick.
//!
//! **Its own binary, ONE `#[tokio::test]`**: same `OnceLock` constraint as every other spy suite
//! in this crate (`authorized_no_birth_metric.rs`, `orders_placed_metric.rs`).
//!
//! Needs `DATABASE_URL`: since #474 a missing database FAILS this suite; only an explicit
//! `DB_TESTS_REQUIRED=0` skips it, and that leaves a receipt (`crates/db_test_gate`).

#[path = "main/common.rs"]
mod common;
#[path = "main/spy_meter.rs"]
mod spy_meter;

use chrono::{Duration, Utc};
use infrastructure::integrations::delivery_handback_watch::delivery_handback_watch_tick;
use sqlx::PgPool;
use telemetry::contract::metric;

async fn append_event(
    pool: &PgPool,
    stream_name: &str,
    version: i32,
    event_type: &str,
    payload: serde_json::Value,
    occurred_at: chrono::DateTime<Utc>,
) {
    sqlx::query(
        "INSERT INTO domain_events \
         (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, payload, metadata, occurred_at) \
         VALUES ($1, $2, $3, $4, 5, $5, NULL, $6, $7, NULL, $8)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(stream_name)
    .bind(version)
    .bind(uuid::Uuid::nil())
    .bind(uuid::Uuid::new_v4())
    .bind(event_type)
    .bind(payload)
    .bind(occurred_at)
    .execute(pool)
    .await
    .expect("append event");
}

fn address(line1: &str) -> serde_json::Value {
    serde_json::json!({ "line1": line1, "city": "Tours", "postalCode": "37000", "country": "FR" })
}

async fn requested(pool: &PgPool, job: uuid::Uuid, order: uuid::Uuid, restaurant: uuid::Uuid, at: chrono::DateTime<Utc>) {
    append_event(
        pool,
        &format!("DeliveryJob-{job}"),
        1,
        "DeliveryRequested",
        serde_json::json!({
            "deliveryJobId": job, "orderId": order, "restaurantId": restaurant,
            "pickup": address("1 rue de la Paix"), "dropoff": address("2 avenue Grammont"),
        }),
        at,
    )
    .await;
}

#[tokio::test]
async fn a_stranded_handback_ages_a_reassignment_clears_it() {
    let spy = spy_meter::SpyMeter::install();
    let Some(db) = common::TestDb::acquire("custody_handback_metric").await else { return };
    let pool = db.pool();

    let t0 = Utc::now() - Duration::minutes(30);
    let restaurant = uuid::Uuid::new_v4();

    // ── (a) PRESENCE — an empty database still emits ONE point at 0.
    delivery_handback_watch_tick(&pool, 1800).await.expect("a tick over an empty database");
    let s = spy.drain();
    assert_eq!(
        s.points(metric::DELIVERY_HANDED_BACK_UNREASSIGNED_AGE_SECONDS),
        vec![(std::collections::BTreeMap::new(), 0.0)],
        "no attributes, one point, 0 -- an ABSENT series must never read as 'nothing stranded'"
    );
    assert_eq!(
        s.points(metric::DELIVERY_HANDBACK_SWEEP_HEARTBEAT_TOTAL),
        vec![(std::collections::BTreeMap::new(), 1.0)],
        "one completed sweep, one heartbeat"
    );

    // ── (b) THE VALUE-DERIVED POSITIVE CONTROL: one stranded job, one re-assigned job.
    let (job_stranded, order_stranded) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let rider1 = uuid::Uuid::new_v4();
    requested(&pool, job_stranded, order_stranded, restaurant, t0).await;
    append_event(
        &pool,
        &format!("DeliveryJob-{job_stranded}"),
        2,
        "DeliveryAcceptedByRider",
        serde_json::json!({ "deliveryJobId": job_stranded, "orderId": order_stranded, "riderId": rider1 }),
        t0 + Duration::minutes(1),
    )
    .await;
    append_event(
        &pool,
        &format!("DeliveryJob-{job_stranded}"),
        3,
        "DeliveryHandedBackByRider",
        serde_json::json!({ "deliveryJobId": job_stranded, "orderId": order_stranded, "riderId": rider1, "foodLocation": "RETURNED_TO_RESTAURANT" }),
        t0 + Duration::minutes(2),
    )
    .await;
    // Age the handback so its truncated age is unambiguous (the same reason #608's suite ages
    // hand-by-hand rather than relying on millisecond deltas).
    sqlx::query(
        "UPDATE domain_events SET occurred_at = now() - make_interval(secs => $1) \
         WHERE stream_name = $2 AND event_type = 'DeliveryHandedBackByRider'",
    )
    .bind(1800.0_f64)
    .bind(format!("DeliveryJob-{job_stranded}"))
    .execute(&pool)
    .await
    .expect("age the stranded handback");

    // THE NAMED MUTANT'S CONTROL: a SECOND job, handed back too, but then RE-ASSIGNED (a later
    // acceptance). Its `food_location` resets to NULL and `status` moves to ASSIGNED, so the "no
    // later acceptance" predicate must exclude it -- if the SQL degraded to "does any handback
    // exist for this job" (the mutant), this job would count and the assertion below would fail.
    let (job_reassigned, order_reassigned) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let rider2 = uuid::Uuid::new_v4();
    requested(&pool, job_reassigned, order_reassigned, restaurant, t0).await;
    append_event(
        &pool,
        &format!("DeliveryJob-{job_reassigned}"),
        2,
        "DeliveryAcceptedByRider",
        serde_json::json!({ "deliveryJobId": job_reassigned, "orderId": order_reassigned, "riderId": rider1 }),
        t0 + Duration::minutes(1),
    )
    .await;
    append_event(
        &pool,
        &format!("DeliveryJob-{job_reassigned}"),
        3,
        "DeliveryHandedBackByRider",
        serde_json::json!({ "deliveryJobId": job_reassigned, "orderId": order_reassigned, "riderId": rider1, "foodLocation": "RETURNED_TO_RESTAURANT" }),
        t0 + Duration::minutes(2),
    )
    .await;
    // Aged OLDER than the stranded job's 1800s (see the module doc): a mutant that drops the "no
    // later acceptance" predicate would report THIS job's age instead, and 3600 != 1800 fails loudly
    // where two nearly-equal ages would not have.
    sqlx::query(
        "UPDATE domain_events SET occurred_at = now() - make_interval(secs => $1) \
         WHERE stream_name = $2 AND event_type = 'DeliveryHandedBackByRider'",
    )
    .bind(3600.0_f64)
    .bind(format!("DeliveryJob-{job_reassigned}"))
    .execute(&pool)
    .await
    .expect("age the reassigned job's handback OLDER than the stranded one");
    append_event(
        &pool,
        &format!("DeliveryJob-{job_reassigned}"),
        4,
        "DeliveryAcceptedByRider",
        serde_json::json!({ "deliveryJobId": job_reassigned, "orderId": order_reassigned, "riderId": rider2 }),
        t0 + Duration::minutes(5),
    )
    .await;

    delivery_handback_watch_tick(&pool, 1800).await.expect("a tick with one stranded and one reassigned job");
    let s = spy.drain();
    assert_eq!(
        s.points(metric::DELIVERY_HANDED_BACK_UNREASSIGNED_AGE_SECONDS),
        vec![(std::collections::BTreeMap::new(), 1800.0)],
        "only the STRANDED job's age -- the reassigned job's later acceptance excludes it entirely \
         (the mutant this control kills: dropping the 'no later acceptance' predicate would count it)"
    );

    // ── (c) SECOND TICK over unchanged state -- delta temporality re-emits every tick.
    delivery_handback_watch_tick(&pool, 1800).await.expect("a second tick");
    assert_eq!(
        spy.drain().points(metric::DELIVERY_HANDED_BACK_UNREASSIGNED_AGE_SECONDS),
        vec![(std::collections::BTreeMap::new(), 1800.0)],
        "every tick re-emits -- a series that reports once is a dead-man's switch that fires once"
    );

    // ── (d) RECOVERY -- reassign the stranded job for real; the next sweep must return to 0.
    append_event(
        &pool,
        &format!("DeliveryJob-{job_stranded}"),
        4,
        "DeliveryAcceptedByRider",
        serde_json::json!({ "deliveryJobId": job_stranded, "orderId": order_stranded, "riderId": rider2 }),
        t0 + Duration::minutes(10),
    )
    .await;
    delivery_handback_watch_tick(&pool, 1800).await.expect("a tick after recovery");
    assert_eq!(
        spy.drain().points(metric::DELIVERY_HANDED_BACK_UNREASSIGNED_AGE_SECONDS),
        vec![(std::collections::BTreeMap::new(), 0.0)],
        "a gauge nobody can close an incident on is not a gauge"
    );
}
