//! Integration test for the journal/mirror retention policy (ADR-20260721-025159; issue #18):
//! `sweep_retention()` called through the `RetentionSweepWorker`.
//!
//! THE FUNCTION UNDER TEST IS THE REAL SPEC SOURCE — `include_str!` of
//! `specs/database/functions/sweep_retention.sql` — never a hand-written mirror. A mirror is how
//! the PR #270 review's C5 escaped: the deployed function still swept the dropped
//! `inbound_events` table while the test's private copy quietly didn't, so the abort was
//! invisible here. Whatever the spec function references, this test must create; a spec change
//! that breaks the function now breaks this test.
//!
//! The point of this test is as much what is DELETED as what is UNTOUCHABLE:
//!   - `domain_events` — the forever log — survives a sweep at ANY age;
//!   - `command_journal` RECEIVED rows survive at any age (only terminal rows age out);
//!   - `inbound_messages` RECEIVED (pending) and SCHEDULED (future) rows survive at any age;
//!   - unprocessed mirror rows (`processed_at IS NULL`) survive at any age;
//!   - `external_sirene_restaurants` (full mirror, detect-by-absence) is never referenced.
//!
//! Needs a real Postgres: set `DATABASE_URL` (see restaurant_projection.rs for a throwaway docker
//! one-liner). Without it the test SKIPS (prints and returns) so `cargo test` stays green offline.
//!
//! One test function on purpose: the tables are shared state, so the scenario must run sequentially.

use infrastructure::RetentionSweepWorker;
use sqlx::PgPool;

/// Fresh copies of every table the policy involves (journals + the mailbox + mirrors + the
/// untouchables), then THE REAL FUNCTION from the spec source.
async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS domain_events, command_journal, inbound_messages, mailbox_partitions,
          external_stripe_events, external_hubrise_callbacks, external_avelo37_events,
          external_uber_direct_events, auth_sessions, external_sirene_restaurants CASCADE;
        DROP SEQUENCE IF EXISTS inbound_messages_position_seq;
        DROP FUNCTION IF EXISTS sweep_retention();

        CREATE TABLE domain_events (
          position BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
          id UUID NOT NULL UNIQUE,
          stream_name TEXT NOT NULL,
          version INTEGER NOT NULL,
          user_id UUID NOT NULL,
          user_type TEXT NOT NULL,
          correlation_id UUID NOT NULL,
          cause_id UUID NULL,
          event_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          metadata JSONB NULL,
          occurred_at TIMESTAMPTZ NOT NULL,
          expired_at TIMESTAMPTZ NULL,
          UNIQUE (stream_name, version)
        );
        CREATE TABLE command_journal (
          message_id UUID PRIMARY KEY,
          correlation_id UUID NOT NULL,
          cause_id UUID NULL,
          session_id UUID NULL,
          trace_id TEXT NULL,
          user_id UUID NULL,
          user_type TEXT NOT NULL,
          channel TEXT NOT NULL,
          command_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          payload_hash TEXT NOT NULL,
          status TEXT NOT NULL,
          error JSONB NULL,
          received_at TIMESTAMPTZ NOT NULL,
          completed_at TIMESTAMPTZ NULL
        );
        CREATE TABLE external_stripe_events (
          stripe_event_id TEXT PRIMARY KEY,
          event_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          received_at TIMESTAMPTZ NOT NULL,
          processed_at TIMESTAMPTZ NULL
        );
        CREATE TABLE external_hubrise_callbacks (
          callback_id TEXT PRIMARY KEY,
          resource_type TEXT NOT NULL,
          event_type TEXT NOT NULL,
          location_id TEXT NULL,
          payload JSONB NOT NULL,
          received_at TIMESTAMPTZ NOT NULL,
          processed_at TIMESTAMPTZ NULL
        );
        CREATE TABLE external_avelo37_events (
          avelo37_event_id TEXT PRIMARY KEY,
          event_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          received_at TIMESTAMPTZ NOT NULL,
          processed_at TIMESTAMPTZ NULL
        );
        CREATE TABLE external_uber_direct_events (
          uber_direct_event_id TEXT PRIMARY KEY,
          event_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          received_at TIMESTAMPTZ NOT NULL,
          processed_at TIMESTAMPTZ NULL
        );
        CREATE TABLE auth_sessions (
          message_id UUID PRIMARY KEY,
          session_id UUID NULL,
          ciphertext BYTEA NOT NULL,
          nonce BYTEA NOT NULL,
          expires_at TIMESTAMPTZ NOT NULL,
          created_at TIMESTAMPTZ NOT NULL
        );
        CREATE TABLE external_sirene_restaurants (
          siret TEXT PRIMARY KEY,
          payload JSONB NOT NULL,
          etat TEXT NOT NULL,
          naf TEXT NOT NULL,
          department TEXT NOT NULL,
          first_seen_at TIMESTAMPTZ NOT NULL,
          last_seen_at TIMESTAMPTZ NOT NULL,
          sync_run_id UUID NOT NULL,
          processed_at TIMESTAMPTZ NULL
        );
        "#,
    )
    .execute(pool)
    .await
    .expect("reset schema");
    // The mailbox table, from the REAL migration DDL.
    sqlx::raw_sql(include_str!("../../../migrations/20260731063000_actor_mailbox_tables.sql"))
        .execute(pool)
        .await
        .expect("apply the actor-mailbox migration");
    sqlx::raw_sql("DROP TABLE IF EXISTS mailbox_partitions").execute(pool).await.expect("trim");
    // THE REAL SPEC FUNCTION — the single source of the retention windows.
    sqlx::raw_sql(include_str!("../../../specs/database/functions/sweep_retention.sql"))
        .execute(pool)
        .await
        .expect("apply the spec sweep_retention function");
}

/// Insert a `command_journal` row with the given status name, ages expressed in days.
async fn journal_row(pool: &PgPool, status: &str, received_days: i32, completed_days: Option<i32>) {
    sqlx::query(
        "INSERT INTO command_journal
           (message_id, correlation_id, user_type, channel, command_type, payload, payload_hash,
            status, received_at, completed_at)
         VALUES ($1, $1, 'PUBLIC', 'GRAPHQL', 'PlaceOrder', '{}', 'h',
                 $2,
                 now() - make_interval(days => $3),
                 CASE WHEN $4::int IS NULL THEN NULL ELSE now() - make_interval(days => $4) END)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(status)
    .bind(received_days)
    .bind(completed_days)
    .execute(pool)
    .await
    .expect("insert command_journal row");
}

/// Insert an `inbound_messages` row with the given status name, ages expressed in days.
async fn mailbox_row(pool: &PgPool, status: &str, received_days: i32, completed_days: Option<i32>) {
    sqlx::query(
        "INSERT INTO inbound_messages
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash,
            channel, user_type, correlation_id, source, external_id,
            status, received_at, completed_at)
         VALUES ($1, 'EVENT', 'Payment', $1, 0, 'PaymentCaptured', '{}', 'h',
                 'EXTERNAL', 'EXTERNAL', $1, 'stripe', $2,
                 $3,
                 now() - make_interval(days => $4),
                 CASE WHEN $5::int IS NULL THEN NULL ELSE now() - make_interval(days => $5) END)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(status)
    .bind(received_days)
    .bind(completed_days)
    .execute(pool)
    .await
    .expect("insert inbound_messages row");
}

/// Insert a mirror row (`table` is one of the webhook mirrors), ages expressed in days.
async fn mirror_row(pool: &PgPool, table: &str, id: &str, received_days: i32, processed_days: Option<i32>) {
    let sql = match table {
        "external_stripe_events" => {
            "INSERT INTO external_stripe_events (stripe_event_id, event_type, payload, received_at, processed_at)
             VALUES ($1, 'payment_intent.succeeded', '{}', now() - make_interval(days => $2),
                     CASE WHEN $3::int IS NULL THEN NULL ELSE now() - make_interval(days => $3) END)"
        }
        "external_hubrise_callbacks" => {
            "INSERT INTO external_hubrise_callbacks (callback_id, resource_type, event_type, payload, received_at, processed_at)
             VALUES ($1, 'catalog', 'update', '{}', now() - make_interval(days => $2),
                     CASE WHEN $3::int IS NULL THEN NULL ELSE now() - make_interval(days => $3) END)"
        }
        "external_avelo37_events" => {
            "INSERT INTO external_avelo37_events (avelo37_event_id, event_type, payload, received_at, processed_at)
             VALUES ($1, 'delivery.updated', '{}', now() - make_interval(days => $2),
                     CASE WHEN $3::int IS NULL THEN NULL ELSE now() - make_interval(days => $3) END)"
        }
        "external_uber_direct_events" => {
            "INSERT INTO external_uber_direct_events (uber_direct_event_id, event_type, payload, received_at, processed_at)
             VALUES ($1, 'event.delivery_status', '{}', now() - make_interval(days => $2),
                     CASE WHEN $3::int IS NULL THEN NULL ELSE now() - make_interval(days => $3) END)"
        }
        other => panic!("unknown mirror table {other}"),
    };
    sqlx::query(sql)
        .bind(id)
        .bind(received_days)
        .bind(processed_days)
        .execute(pool)
        .await
        .expect("insert mirror row");
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
        .fetch_one(pool)
        .await
        .expect("count")
}

#[tokio::test]
async fn sweep_deletes_aged_rows_and_never_touches_the_untouchables() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP retention_sweep: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

    // domain_events: an ANCIENT fact — the forever log must survive any sweep untouched.
    sqlx::query(
        "INSERT INTO domain_events (id, stream_name, version, user_id, user_type, correlation_id,
                                    event_type, payload, occurred_at)
         VALUES ($1, 'Order-1', 0, $1, 0, $1, 'OrderPlaced', '{}', now() - INTERVAL '400 days')",
    )
    .bind(uuid::Uuid::new_v4())
    .execute(&pool)
    .await
    .expect("insert domain event");

    // command_journal: the three terminal statuses past 90d age out; a terminal row inside the
    // window and a RECEIVED row of ANY age are kept.
    journal_row(&pool, "SUCCEEDED", 92, Some(91)).await;
    journal_row(&pool, "REJECTED", 92, Some(91)).await;
    journal_row(&pool, "FAILED", 92, Some(91)).await;
    journal_row(&pool, "SUCCEEDED", 90, Some(89)).await;
    journal_row(&pool, "RECEIVED", 400, None).await;

    // inbound_messages: terminal rows past 90d age out (FAILED included — the mailbox keeps
    // errors on the row, not the workflow); terminal inside the window and RECEIVED (pending
    // work) of any age are kept.
    mailbox_row(&pool, "SUCCEEDED", 92, Some(91)).await;
    mailbox_row(&pool, "FAILED", 92, Some(91)).await;
    mailbox_row(&pool, "SUCCEEDED", 90, Some(89)).await;
    mailbox_row(&pool, "RECEIVED", 400, None).await;

    // Mirrors: processed past 90d ages out; processed inside the window and UNPROCESSED rows of
    // any age are kept.
    mirror_row(&pool, "external_stripe_events", "evt_old", 92, Some(91)).await;
    mirror_row(&pool, "external_stripe_events", "evt_fresh", 90, Some(89)).await;
    mirror_row(&pool, "external_stripe_events", "evt_pending", 400, None).await;
    mirror_row(&pool, "external_hubrise_callbacks", "cb_old", 92, Some(91)).await;
    mirror_row(&pool, "external_hubrise_callbacks", "cb_fresh", 90, Some(89)).await;
    mirror_row(&pool, "external_hubrise_callbacks", "cb_pending", 400, None).await;
    mirror_row(&pool, "external_avelo37_events", "av_old", 92, Some(91)).await;
    mirror_row(&pool, "external_avelo37_events", "av_pending", 400, None).await;
    mirror_row(&pool, "external_uber_direct_events", "ud_old", 92, Some(91)).await;
    mirror_row(&pool, "external_uber_direct_events", "ud_pending", 400, None).await;

    // auth_sessions: one expired pickup row (swept), one live (kept).
    sqlx::query(
        "INSERT INTO auth_sessions (message_id, ciphertext, nonce, expires_at, created_at)
         VALUES ($1, ''::bytea, ''::bytea, now() - INTERVAL '1 hour', now()),
                ($2, ''::bytea, ''::bytea, now() + INTERVAL '1 hour', now())",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(uuid::Uuid::new_v4())
    .execute(&pool)
    .await
    .expect("insert auth sessions");

    // SIRENE mirror: an ancient, long-processed row — exempt from retention entirely.
    sqlx::query(
        "INSERT INTO external_sirene_restaurants
           (siret, payload, etat, naf, department, first_seen_at, last_seen_at, sync_run_id, processed_at)
         VALUES ('12345678900012', '{}', 'A', '56.10A', '37',
                 now() - INTERVAL '400 days', now() - INTERVAL '400 days', $1, now() - INTERVAL '400 days')",
    )
    .bind(uuid::Uuid::new_v4())
    .execute(&pool)
    .await
    .expect("insert sirene row");

    // One sweep pass through the worker.
    let worker = RetentionSweepWorker::new(pool.clone());
    let summary = worker.run_once().await.expect("sweep pass");
    assert_eq!(summary.command_journal, 3, "the three aged terminal journal rows");
    assert_eq!(summary.inbound_messages, 2, "the aged terminal mailbox rows");
    assert_eq!(summary.external_stripe_events, 1, "only the aged processed mirror row");
    assert_eq!(summary.external_hubrise_callbacks, 1, "only the aged processed callback row");
    assert_eq!(summary.external_avelo37_events, 1, "only the aged processed avelo37 row");
    assert_eq!(summary.external_uber_direct_events, 1, "only the aged processed uber row");
    assert_eq!(summary.auth_sessions, 1, "only the expired pickup row");

    // What remains is exactly the keep-set…
    assert_eq!(count(&pool, "command_journal").await, 2, "in-window terminal + ancient RECEIVED");
    assert_eq!(count(&pool, "inbound_messages").await, 2, "in-window terminal + pending RECEIVED");
    assert_eq!(count(&pool, "external_stripe_events").await, 2, "in-window processed + unprocessed");
    assert_eq!(count(&pool, "external_hubrise_callbacks").await, 2, "in-window processed + unprocessed");
    assert_eq!(count(&pool, "external_avelo37_events").await, 1, "unprocessed survives");
    assert_eq!(count(&pool, "external_uber_direct_events").await, 1, "unprocessed survives");
    assert_eq!(count(&pool, "auth_sessions").await, 1, "the live session survives");
    // …and the untouchables are untouched, at any age.
    assert_eq!(count(&pool, "domain_events").await, 1, "the forever log is NEVER swept");
    assert_eq!(count(&pool, "external_sirene_restaurants").await, 1, "the SIRENE mirror is exempt");
    let received: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM command_journal
          WHERE status = 'RECEIVED'",
    )
    .fetch_one(&pool)
    .await
    .expect("count RECEIVED");
    assert_eq!(received, 1, "RECEIVED journal rows are never age-swept");
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM inbound_messages
          WHERE status = 'RECEIVED'",
    )
    .fetch_one(&pool)
    .await
    .expect("count pending mailbox rows");
    assert_eq!(pending, 1, "RECEIVED mailbox rows are never age-swept");

    // A second pass is a no-op — the sweep is idempotent.
    let again = worker.run_once().await.expect("second sweep pass");
    assert_eq!(again.total(), 0, "nothing left in any window");
}
