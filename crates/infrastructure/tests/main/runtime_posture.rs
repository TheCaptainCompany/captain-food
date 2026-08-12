//! The RuntimePosture startup read (#318, ADR-20260803-104819) — the fail-closed-by-cause
//! contract every mailbox-delivering process builds on: a read VALUE is the posture; a missing
//! row or table is [`PostureRead::Unprovable`] (deterministic across processes — everyone falls
//! to the legacy arm together); only a transport failure is `Err` (the caller must not guess).
//! Also guards the seed's idempotence: re-applying the migration must NEVER overwrite an
//! operator-flipped value — a deploy that silently flips the money gate back off would recreate
//! the exact drift the row exists to remove.
//!
//! Needs `DATABASE_URL`: since #474 a missing database FAILS this suite; only an explicit
//! `DB_TESTS_REQUIRED=0` skips it, and that leaves a receipt (`crates/db_test_gate`).

use infrastructure::persistence::runtime_posture::{
    read_posture, PostureRead, PM_MAILBOX_DELIVERY,
};

const MIGRATION: &str = include_str!("../../../../migrations/20260803104819_runtime_posture.sql");

#[tokio::test]
async fn posture_read_is_fail_closed_by_cause() {
    let Some(db) = crate::common::TestDb::acquire("runtime_posture").await else { return };
    let pool = db.pool();

    // (1) TABLE MISSING (schema behind this binary): UNPROVABLE, not an error — the caller
    // falls to the legacy arm deterministically; no retry loop, no guess.
    sqlx::raw_sql("DROP TABLE IF EXISTS RuntimePosture")
        .execute(&pool)
        .await
        .expect("drop");
    assert!(
        matches!(
            read_posture(&pool, PM_MAILBOX_DELIVERY).await.expect("read"),
            PostureRead::Unprovable(_)
        ),
        "a missing table must resolve Unprovable, never Err and never a value"
    );

    // (2) The migration seeds the gate OFF (gate-then-stabilize).
    sqlx::raw_sql(MIGRATION).execute(&pool).await.expect("apply the runtime-posture migration");
    assert_eq!(
        read_posture(&pool, PM_MAILBOX_DELIVERY).await.expect("read"),
        PostureRead::Enabled(false),
        "the seed row is the Runtime D1 default: OFF"
    );

    // (3) An operator flip is read back verbatim…
    sqlx::query("UPDATE RuntimePosture SET enabled = TRUE, updated_at = now() WHERE posture = $1")
        .bind(PM_MAILBOX_DELIVERY)
        .execute(&pool)
        .await
        .expect("flip");
    assert_eq!(
        read_posture(&pool, PM_MAILBOX_DELIVERY).await.expect("read"),
        PostureRead::Enabled(true),
    );

    // (4) …and SURVIVES a re-applied migration: the seed must never overwrite a flip.
    sqlx::raw_sql(MIGRATION).execute(&pool).await.expect("re-apply idempotently");
    assert_eq!(
        read_posture(&pool, PM_MAILBOX_DELIVERY).await.expect("read"),
        PostureRead::Enabled(true),
        "re-applying the migration silently flipped the money gate back off"
    );

    // (5) ROW MISSING (unseeded database): UNPROVABLE again — same deterministic legacy arm.
    sqlx::query("DELETE FROM RuntimePosture WHERE posture = $1")
        .bind(PM_MAILBOX_DELIVERY)
        .execute(&pool)
        .await
        .expect("delete");
    assert!(
        matches!(
            read_posture(&pool, PM_MAILBOX_DELIVERY).await.expect("read"),
            PostureRead::Unprovable(_)
        ),
        "a missing row must resolve Unprovable"
    );

    // Seeding persistence beyond this test is moot since every TestDb::acquire resets the schema.
    sqlx::raw_sql(MIGRATION).execute(&pool).await.expect("re-seed");
}
