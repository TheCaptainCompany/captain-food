//! The RuntimePosture startup read (#318, ADR-20260803-104819) — the fail-closed-by-cause
//! contract a process-wide posture builds on: a read VALUE is the posture; a missing row or table
//! is [`PostureRead::Unprovable`] (deterministic across processes — every reader takes the same
//! conservative arm together); only a transport failure is `Err` (the caller must not guess).
//!
//! NO POSTURE IS DECLARED TODAY: the mechanism's only tenant, `PM_MAILBOX_DELIVERY`, retired with
//! `command_journal` in #242 Runtime D (ADR-20260812-000000). This suite therefore exercises the
//! CONTRACT over an arbitrary key rather than a live gate — the next posture must be able to trust
//! it on the day it is added, and an untested read is exactly how a money gate resolves wrong.
//!
//! Needs `DATABASE_URL`: since #474 a missing database FAILS this suite; only an explicit
//! `DB_TESTS_REQUIRED=0` skips it, and that leaves a receipt (`crates/db_test_gate`).

use infrastructure::persistence::runtime_posture::{read_posture, PostureRead};

const MIGRATION: &str = include_str!("../../../../migrations/20260803104819_runtime_posture.sql");

/// A key no posture uses — the point is the read's behaviour, not any particular gate.
const PROBE: &str = "TEST_ONLY_PROBE_POSTURE";

#[tokio::test]
async fn posture_read_is_fail_closed_by_cause() {
    let Some(db) = crate::common::TestDb::acquire("runtime_posture").await else { return };
    let pool = db.pool();

    // (1) TABLE MISSING (schema behind this binary): UNPROVABLE, not an error — every reader
    // takes the conservative arm deterministically; no retry loop, no guess.
    sqlx::raw_sql("DROP TABLE IF EXISTS RuntimePosture")
        .execute(&pool)
        .await
        .expect("drop");
    assert!(
        matches!(read_posture(&pool, PROBE).await.expect("read"), PostureRead::Unprovable(_)),
        "a missing table must resolve Unprovable, never Err and never a value"
    );

    // (2) The migration creates the table and seeds NO posture (#242 Runtime D removed the only
    // one), so an unseeded key is still UNPROVABLE — never a fabricated `false`.
    sqlx::raw_sql(MIGRATION).execute(&pool).await.expect("apply the runtime-posture migration");
    assert!(
        matches!(read_posture(&pool, PROBE).await.expect("read"), PostureRead::Unprovable(_)),
        "a missing row must resolve Unprovable, not a defaulted value"
    );

    // (3) A seeded posture is read back verbatim, both ways.
    for value in [false, true] {
        sqlx::query(
            "INSERT INTO RuntimePosture (posture, enabled, updated_at) VALUES ($1, $2, now()) \
             ON CONFLICT (posture) DO UPDATE SET enabled = EXCLUDED.enabled, updated_at = now()",
        )
        .bind(PROBE)
        .bind(value)
        .execute(&pool)
        .await
        .expect("seed the probe posture");
        assert_eq!(read_posture(&pool, PROBE).await.expect("read"), PostureRead::Enabled(value));
    }

    // (4) An operator flip SURVIVES a re-applied migration — the property that made this a table
    // rather than an env key: a redeploy must never silently reset a posture an operator set.
    sqlx::raw_sql(MIGRATION).execute(&pool).await.expect("re-apply idempotently");
    assert_eq!(
        read_posture(&pool, PROBE).await.expect("read"),
        PostureRead::Enabled(true),
        "re-applying the migration overwrote an operator-set posture"
    );
}
