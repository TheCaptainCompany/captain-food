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

/// The one key `20260803104819` seeds. It is NOT a live posture any more — `20260812000000` deletes
/// the row — but the seed statement is history and is correctly left unedited, so it is the only key
/// through which the migration's own idempotence can be observed. Step (4) depends on that: a
/// re-apply test aimed at [`PROBE`] would assert over a key the migration never writes, and would
/// pass no matter what the seed's `ON CONFLICT` clause said.
const SEEDED_BY_MIGRATION: &str = "PM_MAILBOX_DELIVERY";

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

    // (2) The migration creates the table and seeds exactly ONE key (`PM_MAILBOX_DELIVERY` —
    // history, correctly unedited; `20260812000000` DELETEs that row afterwards, which is why no
    // posture is live today). An unseeded key is therefore still UNPROVABLE — never a fabricated
    // `false`.
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
    //
    // This MUST be asserted over `SEEDED_BY_MIGRATION`, the key the seed statement actually names.
    // The seed is `ON CONFLICT (posture) DO NOTHING`; the mutant this step exists to catch is
    // `DO UPDATE SET enabled = ...`, and only a key the INSERT touches can observe the difference.
    // Its seeded value is FALSE, so flipping to TRUE first makes the overwrite visible as a value
    // change rather than a no-op.
    sqlx::query("UPDATE RuntimePosture SET enabled = TRUE, updated_at = now() WHERE posture = $1")
        .bind(SEEDED_BY_MIGRATION)
        .execute(&pool)
        .await
        .expect("flip the seeded posture as an operator would");
    assert_eq!(
        read_posture(&pool, SEEDED_BY_MIGRATION).await.expect("read"),
        PostureRead::Enabled(true),
        "the operator flip did not land — the rest of this step would prove nothing"
    );
    sqlx::raw_sql(MIGRATION).execute(&pool).await.expect("re-apply idempotently");
    assert_eq!(
        read_posture(&pool, SEEDED_BY_MIGRATION).await.expect("read"),
        PostureRead::Enabled(true),
        "re-applying the migration overwrote an operator-set posture"
    );

    // (5) …and a re-apply does not resurrect a key it never seeded either: `PROBE` keeps the value
    // step (3) left, so the seed writes exactly one row and no more.
    assert_eq!(read_posture(&pool, PROBE).await.expect("read"), PostureRead::Enabled(true));
}
