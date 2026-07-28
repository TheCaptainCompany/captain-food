//! Integration test for the payload compaction (#231/#238, ADR-20260728-143000).
//!
//! Needs a real Postgres: set `DATABASE_URL` (e.g. a throwaway
//! `docker run -e POSTGRES_PASSWORD=postgres -p 5433:5432 postgres:16-alpine`, then
//! `DATABASE_URL=postgres://postgres:postgres@localhost:5433/postgres?sslmode=disable`).
//! Without it the test SKIPS so `cargo test` stays green offline.
//!
//! ONE test on purpose: it resets the shared staging table, and a second test in this file would race it
//! (test threads within a binary run in parallel).

use sirene_ingest::compact_payloads;
use sqlx::PgPool;

/// The sentinel every pre-#231 row carries -- a deliberate matches-nothing value.
const SENTINEL: &str = "unhashed-pre-20260728";

async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS external_sirene_restaurants CASCADE;
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

    let counts = compact_payloads(&pool, std::time::Duration::from_secs(60))
        .await
        .expect("compaction runs");

    assert_eq!(counts.compacted, 1, "exactly the one row with confirmed evidence");
    assert_eq!(counts.left_unconfirmed, 6, "and the rest are reported, not silently skipped");

    assert!(payload_of(&pool, "0001").await.is_none(), "confirmed SYNCED — payload spent");
    for siret in ["0002", "0003", "0004", "0005", "0006", "0007"] {
        assert!(
            payload_of(&pool, siret).await.is_some(),
            "siret {siret} is not confirmed synced — its payload must survive"
        );
    }

    // Re-running reclaims nothing more and still reports the backlog, so an operator can tell "nothing
    // left to do" from "nothing is confirmed yet" — which need opposite responses.
    let again = compact_payloads(&pool, std::time::Duration::from_secs(60))
        .await
        .expect("second run");
    assert_eq!(again.compacted, 0);
    assert_eq!(again.left_unconfirmed, 6);
}
