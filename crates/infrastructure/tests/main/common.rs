//! The `TestDb` witness — the ONLY door to a database pool in this binary (#335,
//! ADR-20260808-224500 item 5).
//!
//! Two guarantees, both structural rather than conventional:
//!
//! 1. **Serialization**: [`TestDb::acquire`] holds a binary-wide `tokio::sync::Mutex` for the
//!    life of the witness. Every pool is obtained through it, so two DB tests can never touch
//!    the shared database concurrently — whatever `--test-threads` says. This replaces the
//!    implicit isolation the 27 separate binaries had (cargo ran them one at a time) and the
//!    9 file-local `DB_LOCK` statics that only ever covered their own file.
//! 2. **One real schema**: `acquire` resets the database by dropping `public` and replaying the
//!    REAL migration chain (`migrations/*.sql`, embedded via `include_str!`), replacing the ~20
//!    divergent hand-copied DDL blocks the old files carried. A suite therefore runs against
//!    exactly what production runs against; a table the migrations do not create does not exist
//!    here either — which is the point (the recorded eternal-retry incident was a suite leaning
//!    on a sibling suite's leftover table).
//!
//! The DB gate is now the shared `db-test-gate` crate and the polarity is INVERTED (#474): no
//! `DATABASE_URL` ⇒ the suite FAILS, unless `DB_TESTS_REQUIRED` carries an explicit opt-out, which
//! leaves a receipt. [`embedded_migration_manifest_matches_the_migrations_directory`] keeps the
//! embedded chain from silently drifting when a new migration lands.

use sqlx::PgPool;
use tokio::sync::{Mutex, MutexGuard};

/// The binary-wide gate. `const_new` so no lazy-init dance; tokio's Mutex works across the
/// per-`#[tokio::test]` runtimes.
static DB_GATE: Mutex<()> = Mutex::const_new(());

/// Every migration, in apply order — the same chain sqlx-cli applies in CI/production
/// (ADR-0043). `include_str!` so the fixture and production cannot drift.
const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_baseline.sql", include_str!("../../../../migrations/0001_baseline.sql")),
    ("20260717120000_domain_schema.sql", include_str!("../../../../migrations/20260717120000_domain_schema.sql")),
    ("20260717170000_projection_checkpoint.sql", include_str!("../../../../migrations/20260717170000_projection_checkpoint.sql")),
    ("20260717180000_seed_referential_policies.sql", include_str!("../../../../migrations/20260717180000_seed_referential_policies.sql")),
    ("20260718100000_external_sirene_restaurants.sql", include_str!("../../../../migrations/20260718100000_external_sirene_restaurants.sql")),
    ("20260719200000_process_manager_state_tables.sql", include_str!("../../../../migrations/20260719200000_process_manager_state_tables.sql")),
    ("20260720002210_pending_refunds_view.sql", include_str!("../../../../migrations/20260720002210_pending_refunds_view.sql")),
    ("20260720004556_partner_reoffer_policy.sql", include_str!("../../../../migrations/20260720004556_partner_reoffer_policy.sql")),
    ("20260720011500_cart_session_id.sql", include_str!("../../../../migrations/20260720011500_cart_session_id.sql")),
    ("20260720020500_ordertracking_payment_intent_id.sql", include_str!("../../../../migrations/20260720020500_ordertracking_payment_intent_id.sql")),
    ("20260720030000_command_inbound_journals.sql", include_str!("../../../../migrations/20260720030000_command_inbound_journals.sql")),
    ("20260721025159_retention_sweep_function.sql", include_str!("../../../../migrations/20260721025159_retention_sweep_function.sql")),
    ("20260721120000_hubrise_connections.sql", include_str!("../../../../migrations/20260721120000_hubrise_connections.sql")),
    ("20260721130000_external_avelo37_events.sql", include_str!("../../../../migrations/20260721130000_external_avelo37_events.sql")),
    ("20260721140000_delivery_dispatch_strategy.sql", include_str!("../../../../migrations/20260721140000_delivery_dispatch_strategy.sql")),
    ("20260721150000_external_uber_direct_events.sql", include_str!("../../../../migrations/20260721150000_external_uber_direct_events.sql")),
    ("20260721160000_delivery_partner_availability_view.sql", include_str!("../../../../migrations/20260721160000_delivery_partner_availability_view.sql")),
    ("20260722000000_delivery_delay_satisfaction.sql", include_str!("../../../../migrations/20260722000000_delivery_delay_satisfaction.sql")),
    ("20260724150500_auth_sessions.sql", include_str!("../../../../migrations/20260724150500_auth_sessions.sql")),
    ("20260725000000_order_conversation.sql", include_str!("../../../../migrations/20260725000000_order_conversation.sql")),
    ("20260726000000_order_conversation_claim_events.sql", include_str!("../../../../migrations/20260726000000_order_conversation_claim_events.sql")),
    ("20260727000000_customer_credit_balance.sql", include_str!("../../../../migrations/20260727000000_customer_credit_balance.sql")),
    ("20260728020000_restaurant_slug_nullable.sql", include_str!("../../../../migrations/20260728020000_restaurant_slug_nullable.sql")),
    ("20260728030000_slug_reservations_and_alias.sql", include_str!("../../../../migrations/20260728030000_slug_reservations_and_alias.sql")),
    ("20260728040000_sirene_payload_hash.sql", include_str!("../../../../migrations/20260728040000_sirene_payload_hash.sql")),
    ("20260728050000_sirene_payload_transient.sql", include_str!("../../../../migrations/20260728050000_sirene_payload_transient.sql")),
    ("20260728160000_sirene_sync_attempt_tracking.sql", include_str!("../../../../migrations/20260728160000_sirene_sync_attempt_tracking.sql")),
    ("20260730043000_compact_sirene_mirror.sql", include_str!("../../../../migrations/20260730043000_compact_sirene_mirror.sql")),
    ("20260730043100_enum_text_small_tables.sql", include_str!("../../../../migrations/20260730043100_enum_text_small_tables.sql")),
    ("20260730043200_enum_text_restaurant.sql", include_str!("../../../../migrations/20260730043200_enum_text_restaurant.sql")),
    ("20260730043300_enum_text_inbound_events.sql", include_str!("../../../../migrations/20260730043300_enum_text_inbound_events.sql")),
    ("20260730043400_enum_text_command_journal.sql", include_str!("../../../../migrations/20260730043400_enum_text_command_journal.sql")),
    ("20260730043500_enum_text_domain_events.sql", include_str!("../../../../migrations/20260730043500_enum_text_domain_events.sql")),
    ("20260730043600_enum_text_recreate_views.sql", include_str!("../../../../migrations/20260730043600_enum_text_recreate_views.sql")),
    ("20260731063000_actor_mailbox_tables.sql", include_str!("../../../../migrations/20260731063000_actor_mailbox_tables.sql")),
    ("20260731143000_backfill_inbound_events_into_mailbox.sql", include_str!("../../../../migrations/20260731143000_backfill_inbound_events_into_mailbox.sql")),
    ("20260802220000_mailbox_width_100_to_5.sql", include_str!("../../../../migrations/20260802220000_mailbox_width_100_to_5.sql")),
    ("20260802230000_mailbox_attempts_column.sql", include_str!("../../../../migrations/20260802230000_mailbox_attempts_column.sql")),
    ("20260803004500_mailbox_backoff_next_attempt.sql", include_str!("../../../../migrations/20260803004500_mailbox_backoff_next_attempt.sql")),
    ("20260803104819_runtime_posture.sql", include_str!("../../../../migrations/20260803104819_runtime_posture.sql")),
    ("20260808070000_claude_ro_select_only.sql", include_str!("../../../../migrations/20260808070000_claude_ro_select_only.sql")),
    ("20260809000000_catalog_slug_nullable.sql", include_str!("../../../../migrations/20260809000000_catalog_slug_nullable.sql")),
    ("20260809140000_scope_membership.sql", include_str!("../../../../migrations/20260809140000_scope_membership.sql")),
    ("20260809190000_scope_membership_member_rename.sql", include_str!("../../../../migrations/20260809190000_scope_membership_member_rename.sql")),
    ("20260810113000_cart_money_free_fold.sql", include_str!("../../../../migrations/20260810113000_cart_money_free_fold.sql")),
    ("20260812000000_drop_command_journal.sql", include_str!("../../../../migrations/20260812000000_drop_command_journal.sql")),
    ("20260813021500_sms_send_quota.sql", include_str!("../../../../migrations/20260813021500_sms_send_quota.sql")),
    ("20260830210000_rider_identity_projection.sql", include_str!("../../../../migrations/20260830210000_rider_identity_projection.sql")),
];

/// The witness: proof that this test holds the database. Owns the pool AND the binary-wide
/// lock — the pool is only reachable through a live witness, so an unlocked DB test cannot be
/// written in this binary.
pub(crate) struct TestDb {
    pool: PgPool,
    _serialized: MutexGuard<'static, ()>,
}

impl TestDb {
    /// The one pool constructor. `None` = skip (no `DATABASE_URL`; loud under
    /// an explicit `DB_TESTS_REQUIRED=0`, #474). Otherwise: take the binary-wide lock, connect, reset the
    /// schema from the real migration chain.
    pub(crate) async fn acquire(suite: &str) -> Option<TestDb> {
        let url = db_test_gate::database_url(suite)?;
        let guard = DB_GATE.lock().await;
        let pool = PgPool::connect(&url).await.expect("connect Postgres");
        reset_schema(&pool).await;
        Some(TestDb { pool, _serialized: guard })
    }

    /// A pool handle (sqlx pools are cheap clones). Only obtainable from a live witness.
    pub(crate) fn pool(&self) -> PgPool {
        self.pool.clone()
    }

    /// The database URL, for the few suites that need a NON-pool connection (`PgListener`).
    /// Only obtainable from a live witness, so the listener is serialized like everything else.
    pub(crate) fn url(&self) -> String {
        std::env::var("DATABASE_URL").expect("TestDb exists, so DATABASE_URL is set")
    }
}

/// Drop everything and replay the real migration chain. `raw_sql` sends each file as one simple
/// query (multi-statement files run in one implicit transaction; the single-statement
/// `VACUUM FULL` file runs in autocommit, same as sqlx-cli's `-- no-transaction`).
async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .execute(pool)
        .await
        .expect("recreate the public schema");
    apply_migration_chain(pool).await;
}

/// Replay the real migration chain onto an ALREADY-EMPTY database, without dropping anything.
///
/// The one caller is `tests/rls_matrix.rs` (#638), which builds its own throwaway databases — one
/// per security mode — because `DROP POLICY` and `ALTER TABLE … FORCE` take `ACCESS EXCLUSIVE`, so
/// the two modes must not be a time-ordered mutation of one database, and because roles and
/// database-level grants survive `reset_schema` into every later suite sharing CI's database.
/// It replays the SAME embedded chain as [`reset_schema`], so a suite that builds its own database
/// still runs against exactly what production runs against.
#[allow(dead_code)] // used by tests/rls_matrix.rs; every other includer of this file uses TestDb only
pub(crate) async fn apply_migration_chain(pool: &PgPool) {
    for (name, sql) in MIGRATIONS {
        sqlx::raw_sql(sql)
            .execute(pool)
            .await
            .unwrap_or_else(|e| panic!("apply migration {name}: {e}"));
    }
}

/// The embedded chain cannot drift from `migrations/`: a new migration that is not added to
/// [`MIGRATIONS`] fails this (offline) test, not some later suite with a missing table.
#[test]
fn embedded_migration_manifest_matches_the_migrations_directory() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../migrations");
    let mut on_disk: Vec<String> = std::fs::read_dir(dir)
        .expect("read migrations/")
        .map(|e| e.expect("dir entry").file_name().into_string().expect("utf-8 name"))
        .filter(|n| n.ends_with(".sql"))
        .collect();
    on_disk.sort();
    let embedded: Vec<&str> = MIGRATIONS.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        embedded, on_disk,
        "tests/main/common.rs MIGRATIONS must list migrations/*.sql exactly, in order"
    );
}
