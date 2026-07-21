//! Integration test for the journal/mirror retention policy (ADR-20260721-025159; issue #18):
//! `sweep_retention()` (the SQL function that OWNS the windows — mirrored here from
//! migrations/20260721025159, whose body is the generated `specs/generated/schema.generated.sql`)
//! called through the `RetentionSweepWorker`.
//!
//! The point of this test is as much what is DELETED as what is UNTOUCHABLE:
//!   - `domain_events` — the forever log — survives a sweep at ANY age;
//!   - `command_journal` RECEIVED rows survive at any age (only terminal rows age out);
//!   - `inbound_events` FAILED rows survive until resolved, RECEIVED rows are pending work;
//!   - unprocessed mirror rows (`processed_at IS NULL`) survive at any age;
//!   - `external_sirene_restaurants` (full mirror, detect-by-absence) is never referenced.
//!
//! Needs a real Postgres: set `DATABASE_URL` (see restaurant_projection.rs for a throwaway docker
//! one-liner). Without it the test SKIPS (prints and returns) so `cargo test` stays green offline.
//!
//! One test function on purpose: the tables are shared state, so the scenario must run sequentially.

use infrastructure::RetentionSweepWorker;
use sqlx::PgPool;

/// Fresh copies of every table the policy involves (journals + mirrors + the untouchables), the
/// ref_* enum lookups the function resolves statuses through, and the function itself.
async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS domain_events, command_journal, inbound_events,
          external_stripe_events, external_hubrise_callbacks, external_sirene_restaurants,
          ref_command_journal_status, ref_inbound_event_status CASCADE;
        DROP FUNCTION IF EXISTS sweep_retention();

        CREATE TABLE ref_command_journal_status(sort_order INT PRIMARY KEY, value TEXT NOT NULL UNIQUE);
        INSERT INTO ref_command_journal_status (value, sort_order) VALUES ('RECEIVED',0),('SUCCEEDED',1),('REJECTED',2),('FAILED',3);
        CREATE TABLE ref_inbound_event_status(sort_order INT PRIMARY KEY, value TEXT NOT NULL UNIQUE);
        INSERT INTO ref_inbound_event_status (value, sort_order) VALUES ('RECEIVED',0),('DELIVERED',1),('FAILED',2);

        CREATE TABLE domain_events (
          position BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
          id UUID NOT NULL UNIQUE,
          stream_name TEXT NOT NULL,
          version INTEGER NOT NULL,
          user_id UUID NOT NULL,
          user_type INTEGER NOT NULL,
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
          user_type INTEGER NOT NULL,
          channel INTEGER NOT NULL,
          command_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          payload_hash TEXT NOT NULL,
          status INTEGER NOT NULL,
          error JSONB NULL,
          received_at TIMESTAMPTZ NOT NULL,
          completed_at TIMESTAMPTZ NULL
        );
        CREATE TABLE inbound_events (
          inbound_event_id UUID PRIMARY KEY,
          source TEXT NOT NULL,
          external_id TEXT NOT NULL,
          correlation_id UUID NOT NULL,
          event_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          status INTEGER NOT NULL,
          error JSONB NULL,
          received_at TIMESTAMPTZ NOT NULL,
          delivered_at TIMESTAMPTZ NULL,
          UNIQUE (source, external_id)
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

        CREATE FUNCTION sweep_retention()
        RETURNS TABLE (swept_table TEXT, deleted BIGINT)
        LANGUAGE plpgsql
        AS $fn$
        DECLARE
          n BIGINT;
        BEGIN
          DELETE FROM command_journal
           WHERE status IN (SELECT sort_order FROM ref_command_journal_status
                             WHERE value IN ('SUCCEEDED', 'REJECTED', 'FAILED'))
             AND completed_at IS NOT NULL
             AND completed_at < now() - INTERVAL '90 days';
          GET DIAGNOSTICS n = ROW_COUNT;
          swept_table := 'command_journal'; deleted := n; RETURN NEXT;

          DELETE FROM inbound_events
           WHERE status = (SELECT sort_order FROM ref_inbound_event_status WHERE value = 'DELIVERED')
             AND delivered_at IS NOT NULL
             AND delivered_at < now() - INTERVAL '30 days';
          GET DIAGNOSTICS n = ROW_COUNT;
          swept_table := 'inbound_events'; deleted := n; RETURN NEXT;

          DELETE FROM external_stripe_events
           WHERE processed_at IS NOT NULL
             AND processed_at < now() - INTERVAL '90 days';
          GET DIAGNOSTICS n = ROW_COUNT;
          swept_table := 'external_stripe_events'; deleted := n; RETURN NEXT;

          DELETE FROM external_hubrise_callbacks
           WHERE processed_at IS NOT NULL
             AND processed_at < now() - INTERVAL '90 days';
          GET DIAGNOSTICS n = ROW_COUNT;
          swept_table := 'external_hubrise_callbacks'; deleted := n; RETURN NEXT;
        END;
        $fn$;
        "#,
    )
    .execute(pool)
    .await
    .expect("reset schema");
}

/// Insert a `command_journal` row with the given status name, ages expressed in days.
async fn journal_row(pool: &PgPool, status: &str, received_days: i32, completed_days: Option<i32>) {
    sqlx::query(
        "INSERT INTO command_journal
           (message_id, correlation_id, user_type, channel, command_type, payload, payload_hash,
            status, received_at, completed_at)
         VALUES ($1, $1, 0, 0, 'PlaceOrder', '{}', 'h',
                 (SELECT sort_order FROM ref_command_journal_status WHERE value = $2),
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

/// Insert an `inbound_events` row with the given status name, ages expressed in days.
async fn inbound_row(pool: &PgPool, status: &str, received_days: i32, delivered_days: Option<i32>) {
    sqlx::query(
        "INSERT INTO inbound_events
           (inbound_event_id, source, external_id, correlation_id, event_type, payload,
            status, received_at, delivered_at)
         VALUES ($1, 'stripe', $2, $1, 'PaymentCaptured', '{}',
                 (SELECT sort_order FROM ref_inbound_event_status WHERE value = $3),
                 now() - make_interval(days => $4),
                 CASE WHEN $5::int IS NULL THEN NULL ELSE now() - make_interval(days => $5) END)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(status)
    .bind(received_days)
    .bind(delivered_days)
    .execute(pool)
    .await
    .expect("insert inbound_events row");
}

/// Insert a mirror row (`table` is one of the two webhook mirrors), ages expressed in days.
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

    // inbound_events: DELIVERED past 30d ages out; DELIVERED inside the window, FAILED (kept until
    // resolved) and RECEIVED (pending work) of any age are kept.
    inbound_row(&pool, "DELIVERED", 32, Some(31)).await;
    inbound_row(&pool, "DELIVERED", 30, Some(29)).await;
    inbound_row(&pool, "FAILED", 400, None).await;
    inbound_row(&pool, "RECEIVED", 400, None).await;

    // Mirrors: processed past 90d ages out; processed inside the window and UNPROCESSED rows of
    // any age are kept.
    mirror_row(&pool, "external_stripe_events", "evt_old", 92, Some(91)).await;
    mirror_row(&pool, "external_stripe_events", "evt_fresh", 90, Some(89)).await;
    mirror_row(&pool, "external_stripe_events", "evt_pending", 400, None).await;
    mirror_row(&pool, "external_hubrise_callbacks", "cb_old", 92, Some(91)).await;
    mirror_row(&pool, "external_hubrise_callbacks", "cb_fresh", 90, Some(89)).await;
    mirror_row(&pool, "external_hubrise_callbacks", "cb_pending", 400, None).await;

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
    assert_eq!(summary.inbound_events, 1, "only the aged DELIVERED row");
    assert_eq!(summary.external_stripe_events, 1, "only the aged processed mirror row");
    assert_eq!(summary.external_hubrise_callbacks, 1, "only the aged processed callback row");

    // What remains is exactly the keep-set…
    assert_eq!(count(&pool, "command_journal").await, 2, "in-window terminal + ancient RECEIVED");
    assert_eq!(count(&pool, "inbound_events").await, 3, "in-window DELIVERED + FAILED + RECEIVED");
    assert_eq!(count(&pool, "external_stripe_events").await, 2, "in-window processed + unprocessed");
    assert_eq!(count(&pool, "external_hubrise_callbacks").await, 2, "in-window processed + unprocessed");
    // …and the untouchables are untouched, at any age.
    assert_eq!(count(&pool, "domain_events").await, 1, "the forever log is NEVER swept");
    assert_eq!(count(&pool, "external_sirene_restaurants").await, 1, "the SIRENE mirror is exempt");
    let received: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM command_journal
          WHERE status = (SELECT sort_order FROM ref_command_journal_status WHERE value = 'RECEIVED')",
    )
    .fetch_one(&pool)
    .await
    .expect("count RECEIVED");
    assert_eq!(received, 1, "RECEIVED journal rows are never age-swept");
    let failed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM inbound_events
          WHERE status = (SELECT sort_order FROM ref_inbound_event_status WHERE value = 'FAILED')",
    )
    .fetch_one(&pool)
    .await
    .expect("count FAILED");
    assert_eq!(failed, 1, "FAILED inbound rows are kept until resolved");

    // A second pass is a no-op — the sweep is idempotent.
    let again = worker.run_once().await.expect("second sweep pass");
    assert_eq!(again.total(), 0, "nothing left in any window");
}
