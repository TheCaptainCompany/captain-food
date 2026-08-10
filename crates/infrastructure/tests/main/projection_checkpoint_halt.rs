//! The checkpoint must not advance past a fold the DATABASE rejected (#474 test 1).
//!
//! The failure being guarded is [#451]: `Cart.total_amount_cents` and `Cart.currency` were
//! `NOT NULL` with no `DEFAULT` while `cart_store::upsert` never listed them, so every folded Cart
//! event failed `23502`. The drain loop log-and-skipped each one and advanced the checkpoint, so
//! the projector reported a clean, caught-up drain over a read model that was permanently empty --
//! and `placeOrder` was dead. `cargo check` passed, six crate suites run by hand passed, and three
//! `make rust` rounds passed; only CI's Postgres job went red.
//!
//! These tests do NOT depend on that migration being broken. Each manufactures the exact #451
//! condition on the live test schema (`ALTER TABLE Cart ... DROP DEFAULT`), so the guard keeps
//! working after the defect is fixed -- which is the only version of this test worth having, since
//! a test that only fails against a planted bug dies with the plant.
//!
//! The three cases together pin the CLASSIFICATION, not just the halt: a database fault halts, a
//! malformed payload still skips, and the current default still behaves exactly as it does today.

use infrastructure::projection::DbFaultPolicy;
use infrastructure::ProjectionWorker;
use sqlx::{PgPool, Row};

/// Reproduce the #451 shape: `NOT NULL`, no `DEFAULT`, not in the writer's insert list.
async fn plant_the_451_defect(pool: &PgPool) {
    sqlx::raw_sql(
        "ALTER TABLE Cart ALTER COLUMN total_amount_cents DROP DEFAULT, \
         ALTER COLUMN currency DROP DEFAULT",
    )
    .execute(pool)
    .await
    .expect("drop the defaults that make the Cart fold survivable");
}

async fn append(pool: &PgPool, stream: &str, version: i32, event_type: &str, payload: serde_json::Value) -> i64 {
    sqlx::query(
        "INSERT INTO domain_events \
         (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, payload, metadata, occurred_at) \
         VALUES ($1, $2, $3, $4, 5, $5, NULL, $6, $7, NULL, now()) RETURNING position",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(stream)
    .bind(version)
    .bind(uuid::Uuid::nil())
    .bind(uuid::Uuid::new_v4())
    .bind(event_type)
    .bind(payload)
    .fetch_one(pool)
    .await
    .expect("append event")
    .get::<i64, _>("position")
}

/// `None` = the group has never committed a checkpoint, which is distinct from "committed 0".
async fn checkpoint(pool: &PgPool, projector: &str) -> Option<i64> {
    sqlx::query_scalar("SELECT position FROM projection_checkpoint WHERE projector = $1")
        .bind(projector)
        .fetch_optional(pool)
        .await
        .expect("read checkpoint")
}

async fn cart_rows(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM Cart").fetch_one(pool).await.expect("count carts")
}

fn cart_started(cart_id: uuid::Uuid) -> serde_json::Value {
    serde_json::json!({
        "cartId": cart_id,
        "restaurantId": uuid::Uuid::new_v4(),
        "sessionId": uuid::Uuid::new_v4(),
        "customerId": uuid::Uuid::new_v4(),
    })
}

/// THE guard. Under [`DbFaultPolicy::Halt`] a rejected write leaves the checkpoint untouched and
/// surfaces the error, so the events are still in front of the projector on the next tick.
#[tokio::test]
async fn a_database_rejected_fold_leaves_the_checkpoint_exactly_where_it_was() {
    let Some(db) = crate::common::TestDb::acquire("projection_checkpoint_halt").await else { return };
    let pool = db.pool();
    plant_the_451_defect(&pool).await;

    let cart_id = uuid::Uuid::new_v4();
    let position = append(&pool, &format!("Cart-{cart_id}"), 1, "CartStarted", cart_started(cart_id)).await;
    let before = checkpoint(&pool, "Cart").await;

    let outcome = ProjectionWorker::new(pool.clone())
        .with_db_fault_policy(DbFaultPolicy::Halt)
        .run_once()
        .await;

    let error = outcome.expect_err("a NOT NULL violation must fail the drain, not be reported as progress");
    assert!(
        format!("{error}").contains("null value") || format!("{error}").contains("23502"),
        "the surfaced error should be the database's own rejection, got: {error}"
    );
    assert_eq!(
        checkpoint(&pool, "Cart").await,
        before,
        "the Cart checkpoint moved past position {position}, whose fold never landed -- \
         this is the #451 lie: reported progress over an empty read model"
    );
    assert_eq!(cart_rows(&pool).await, 0, "the row genuinely did not land, so nothing was lost by halting");
}

/// The classification's other half: a record whose PAYLOAD does not fit its event type is genuinely
/// per-record, so it still skips and still advances -- even under `Halt`. Without this, `Halt`
/// would silently re-introduce the boot-time refold wedge #230 removed, where one legacy payload
/// froze every projection forever.
#[tokio::test]
async fn a_malformed_payload_still_skips_and_advances_even_under_halt() {
    let Some(db) = crate::common::TestDb::acquire("projection_checkpoint_halt_payload").await else { return };
    let pool = db.pool();

    let cart_id = uuid::Uuid::new_v4();
    // Right event type, payload that cannot deserialize into it (cartId is not a UUID).
    let position = append(
        &pool,
        &format!("Cart-{cart_id}"),
        1,
        "CartStarted",
        serde_json::json!({ "cartId": "not-a-uuid" }),
    )
    .await;

    ProjectionWorker::new(pool.clone())
        .with_db_fault_policy(DbFaultPolicy::Halt)
        .run_once()
        .await
        .expect("a malformed payload is one bad record, not a reason to stop the group");

    assert_eq!(
        checkpoint(&pool, "Cart").await,
        Some(position),
        "a payload-shape skip must still advance, or one bad record wedges the group forever"
    );
}

/// The gate's OTHER arm, pinned so the flip is a visible diff rather than a drift: the default
/// policy still does exactly what it does today -- advance past the rejected write and report a
/// clean drain. This assertion documents the lie #451 shipped behind; the day the default flips it
/// must be DELETED in the same change, with an ADR, and that is the point of writing it down.
#[tokio::test]
async fn the_default_policy_still_advances_past_a_rejected_write_todays_behaviour() {
    let Some(db) = crate::common::TestDb::acquire("projection_checkpoint_skip_default").await else { return };
    let pool = db.pool();
    plant_the_451_defect(&pool).await;

    let cart_id = uuid::Uuid::new_v4();
    let position = append(&pool, &format!("Cart-{cart_id}"), 1, "CartStarted", cart_started(cart_id)).await;

    ProjectionWorker::new(pool.clone()).run_once().await.expect("today's default reports a clean drain");

    assert_eq!(
        checkpoint(&pool, "Cart").await,
        Some(position),
        "default policy is Skip: it advances past the failure (this is the behaviour #474 gates, not endorses)"
    );
    assert_eq!(cart_rows(&pool).await, 0, "...over a read model that stayed empty");
}
