//! Integration test for the payload compaction (#231/#238, ADR-20260728-143000).
//!
//! Needs a real Postgres: set `DATABASE_URL` (e.g. a throwaway
//! `docker run -e POSTGRES_PASSWORD=postgres -p 5433:5432 postgres:16-alpine`, then
//! `DATABASE_URL=postgres://postgres:postgres@localhost:5433/postgres?sslmode=disable`).
//! Without it the test SKIPS so `cargo test` stays green offline.
//!
//! ONE test on purpose: it resets the shared staging table, and a second test in this file would race it
//! (test threads within a binary run in parallel).

use sirene_ingest::{compact_payloads, journal_message_id};
use sqlx::PgPool;

/// The sentinel every pre-#231 row carries -- a deliberate matches-nothing value.
const SENTINEL: &str = "unhashed-pre-20260728";

async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS external_sirene_restaurants, command_journal,
                             ref_command_journal_status CASCADE;
        CREATE TABLE external_sirene_restaurants (
          siret TEXT PRIMARY KEY,
          payload JSONB NULL,
          etat TEXT NOT NULL,
          naf TEXT NOT NULL,
          department TEXT NOT NULL,
          first_seen_at TIMESTAMPTZ NOT NULL,
          last_seen_at TIMESTAMPTZ NOT NULL,
          sync_run_id UUID NOT NULL,
          payload_hash TEXT NOT NULL DEFAULT 'unhashed-pre-20260728',
          processed_at TIMESTAMPTZ NULL,
          status TEXT NOT NULL DEFAULT 'PENDING',
          synced_at TIMESTAMPTZ NULL,
          last_attempt_sync_at TIMESTAMPTZ NULL,
          attempt_sync_retry_count INTEGER NOT NULL DEFAULT 0
        );
        -- The journal arm's evidence source (mirrors migrations/20260720030000, columns the arm reads)
        -- + the ref_* lookup it resolves 'SUCCEEDED' through (ADR-0037 ordinals, never hardcoded).
        CREATE TABLE command_journal (
          message_id UUID PRIMARY KEY,
          command_type TEXT NOT NULL,
          status INTEGER NOT NULL,
          completed_at TIMESTAMPTZ NULL
        );
        CREATE TABLE ref_command_journal_status(sort_order INT PRIMARY KEY, value TEXT NOT NULL UNIQUE);
        INSERT INTO ref_command_journal_status (value, sort_order)
             VALUES ('RECEIVED',0),('SUCCEEDED',1),('REJECTED',2),('FAILED',3);
        "#,
    )
    .execute(pool)
    .await
    .expect("reset staging schema");
}

/// Seed one row with an explicit status/synced_at, and a payload present.
async fn seed(pool: &PgPool, siret: &str, status: &str, synced: bool, checkpointed: bool) {
    sqlx::query(
        "INSERT INTO external_sirene_restaurants \
           (siret, payload, etat, naf, department, first_seen_at, last_seen_at, sync_run_id, \
            payload_hash, processed_at, status, synced_at) \
         VALUES ($1, '{\"siret\":\"x\"}'::jsonb, 'A', '56.10A', '37', now(), now(), gen_random_uuid(), \
                 $2, CASE WHEN $4 THEN now() ELSE NULL END, $3, \
                 CASE WHEN $5 THEN now() ELSE NULL END)",
    )
    .bind(siret)
    .bind(SENTINEL)
    .bind(status)
    .bind(checkpointed)
    .bind(synced)
    .execute(pool)
    .await
    .expect("seed staging row");
}

async fn payload_of(pool: &PgPool, siret: &str) -> Option<serde_json::Value> {
    sqlx::query_scalar("SELECT payload FROM external_sirene_restaurants WHERE siret = $1")
        .bind(siret)
        .fetch_one(pool)
        .await
        .expect("row")
}

/// Seed a pre-#227 row: default `PENDING` status, no `synced_at`, an explicit `last_seen_at` (the
/// journal arm derives the expected message_id from it, so it must be a value we control).
async fn seed_pre227(
    pool: &PgPool,
    siret: &str,
    etat: &str,
    last_seen_at: chrono::DateTime<chrono::Utc>,
    checkpointed: bool,
) {
    sqlx::query(
        "INSERT INTO external_sirene_restaurants \
           (siret, payload, etat, naf, department, first_seen_at, last_seen_at, sync_run_id, \
            payload_hash, processed_at) \
         VALUES ($1, '{\"siret\":\"x\"}'::jsonb, $2, '56.10A', '37', $3, $3, gen_random_uuid(), \
                 $4, CASE WHEN $5 THEN $3 ELSE NULL END)",
    )
    .bind(siret)
    .bind(etat)
    .bind(last_seen_at)
    .bind(SENTINEL)
    .bind(checkpointed)
    .execute(pool)
    .await
    .expect("seed pre-#227 row");
}

/// Record the verdict the retired command path (#15) journaled for one staged version.
async fn seed_journal_verdict(
    pool: &PgPool,
    message_id: uuid::Uuid,
    command_type: &str,
    verdict: &str,
    completed_at: chrono::DateTime<chrono::Utc>,
) {
    sqlx::query(
        "INSERT INTO command_journal (message_id, command_type, status, completed_at) \
         VALUES ($1, $2, (SELECT sort_order FROM ref_command_journal_status WHERE value = $3), $4)",
    )
    .bind(message_id)
    .bind(command_type)
    .bind(verdict)
    .bind(completed_at)
    .execute(pool)
    .await
    .expect("seed journal verdict");
}

/// THE rule (#238): a payload is removed only from a row that positively records having reached the
/// domain — `status = 'SYNCED'` AND `synced_at IS NOT NULL`. Everything else keeps its payload.
///
/// The case that matters most is the third one. A pre-#231 row is CHECKPOINTED (`processed_at` set, not
/// behind `last_seen_at`) but carries no evidence of the sync's OUTCOME, because nothing recorded one
/// before the status column existed. An earlier version of this pass read that checkpoint as success and
/// deleted the payload — deriving certainty from a column that never carried any, and destroying the
/// only copy of the record. Recovering it costs a ~4h re-fetch from INSEE, so the test is the guard.
#[tokio::test]
async fn only_a_confirmed_sync_loses_its_payload() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP only_a_confirmed_sync_loses_its_payload: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

    //                                    status        synced_at  processed_at
    seed(&pool, "0001", "SYNCED", true, true).await; //  confirmed both ways
    seed(&pool, "0002", "STAGED", false, true).await; //  handed over, aggregate has not decided
    seed(&pool, "0003", "PENDING", false, true).await; // pre-#231: checkpointed, outcome unrecorded
    seed(&pool, "0004", "UNMAPPABLE", false, true).await; // evidence
    seed(&pool, "0005", "FAILED", false, false).await; //  will be retried
    seed(&pool, "0006", "POISON", false, false).await; //  quarantined, needs diagnosing
    // Status says SYNCED but nothing timestamped it — a half-written row must not be trusted either.
    seed(&pool, "0007", "SYNCED", false, true).await;

    // Pre-#227 rows whose outcome IS recorded — one table over, in the command journal (#238). The
    // timestamp is microsecond-precise on purpose: it is what the retired command path read from the
    // row and seeded the deterministic message_id with, so the arm must reproduce it exactly.
    let seen = chrono::DateTime::parse_from_rfc3339("2026-07-20T12:00:00.123456+00:00")
        .expect("fixed timestamp")
        .with_timezone(&chrono::Utc);
    let done = seen + chrono::Duration::minutes(5);
    // 0008: checkpointed + a SUCCEEDED RegisterRestaurant verdict for THIS version → confirmed.
    seed_pre227(&pool, "0008", "A", seen, true).await;
    seed_journal_verdict(
        &pool,
        journal_message_id("RegisterRestaurant", "0008", seen),
        "RegisterRestaurant",
        "SUCCEEDED",
        done,
    )
    .await;
    // 0009: the verdict was a REJECTION (the 605-row SlugAlreadyTaken class) → never synced, kept.
    seed_pre227(&pool, "0009", "A", seen, true).await;
    seed_journal_verdict(
        &pool,
        journal_message_id("RegisterRestaurant", "0009", seen),
        "RegisterRestaurant",
        "REJECTED",
        done,
    )
    .await;
    // 0010: an explicit closure — the etat=F path journaled MarkRestaurantClosed → confirmed.
    seed_pre227(&pool, "0010", "F", seen, true).await;
    seed_journal_verdict(
        &pool,
        journal_message_id("MarkRestaurantClosed", "0010", seen),
        "MarkRestaurantClosed",
        "SUCCEEDED",
        done,
    )
    .await;
    // 0011: the SUCCEEDED verdict is for an OLDER version (the row was refreshed since) → no match,
    // kept — the arm errs conservative rather than trusting a stale verdict.
    seed_pre227(&pool, "0011", "A", seen, true).await;
    seed_journal_verdict(
        &pool,
        journal_message_id("RegisterRestaurant", "0011", seen - chrono::Duration::days(7)),
        "RegisterRestaurant",
        "SUCCEEDED",
        done,
    )
    .await;
    // 0012: not checkpointed (still pending translation) — a journal verdict alone must not spend it.
    seed_pre227(&pool, "0012", "A", seen, false).await;
    seed_journal_verdict(
        &pool,
        journal_message_id("RegisterRestaurant", "0012", seen),
        "RegisterRestaurant",
        "SUCCEEDED",
        done,
    )
    .await;

    let counts = compact_payloads(&pool, std::time::Duration::from_secs(60))
        .await
        .expect("compaction runs");

    assert_eq!(counts.compacted, 1, "exactly the one row with confirmed evidence");
    assert_eq!(counts.journal_confirmed, 2, "and exactly the two the journal's verdicts confirm");
    assert_eq!(counts.left_unconfirmed, 9, "and the rest are reported, not silently skipped");

    assert!(payload_of(&pool, "0001").await.is_none(), "confirmed SYNCED — payload spent");
    for siret in ["0008", "0010"] {
        assert!(
            payload_of(&pool, siret).await.is_none(),
            "siret {siret} carries a recorded SUCCEEDED verdict — payload spent"
        );
    }
    for siret in ["0002", "0003", "0004", "0005", "0006", "0007", "0009", "0011", "0012"] {
        assert!(
            payload_of(&pool, siret).await.is_some(),
            "siret {siret} is not confirmed synced — its payload must survive"
        );
    }

    // The confirmation is a TRANSCRIPTION of the journal's verdict, not a fresh claim: the row now
    // says SYNCED and carries the moment the dispatch recorded, not this run's clock.
    let (status, synced_at): (String, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT status, synced_at FROM external_sirene_restaurants WHERE siret = '0008'",
    )
    .fetch_one(&pool)
    .await
    .expect("confirmed row");
    assert_eq!(status, "SYNCED");
    assert_eq!(synced_at, Some(done), "synced_at is the journal's completed_at, not now()");

    // Re-running reclaims nothing more and still reports the backlog, so an operator can tell "nothing
    // left to do" from "nothing is confirmed yet" — which need opposite responses.
    let again = compact_payloads(&pool, std::time::Duration::from_secs(60))
        .await
        .expect("second run");
    assert_eq!(again.compacted, 0);
    assert_eq!(again.journal_confirmed, 0);
    assert_eq!(again.left_unconfirmed, 9);
}
