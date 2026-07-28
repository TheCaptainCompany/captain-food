//! Integration test for the one-shot payload compaction (#231, ADR-20260728-143000).
//!
//! Needs a real Postgres: set `DATABASE_URL` (e.g. a throwaway
//! `docker run -e POSTGRES_PASSWORD=postgres -p 5433:5432 postgres:16-alpine`, then
//! `DATABASE_URL=postgres://postgres:postgres@localhost:5433/postgres?sslmode=disable`).
//! Without it the test SKIPS so `cargo test` stays green offline.
//!
//! ONE test on purpose: it resets the shared staging table, and a second test in this file would race
//! it (test threads within a binary run in parallel).

use sirene_ingest::compact_payloads;
use sqlx::PgPool;

/// The sentinel every pre-#231 row carries — a deliberate matches-nothing value.
const SENTINEL: &str = "unhashed-pre-20260728";

/// Mirrors the live table after migration 20260728050000 (payload NULLable).
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
          synced_at TIMESTAMPTZ NULL
        );
        "#,
    )
    .execute(pool)
    .await
    .expect("reset staging schema");
}

/// Insert one pre-compaction row: sentinel hash, payload present, `processed_at` per `done`.
async fn seed(pool: &PgPool, siret: &str, payload: &str, done: bool) {
    sqlx::query(
        "INSERT INTO external_sirene_restaurants \
           (siret, payload, etat, naf, department, first_seen_at, last_seen_at, sync_run_id, \
            payload_hash, processed_at) \
         VALUES ($1, $2::jsonb, 'A', '56.10A', '37', now(), now(), gen_random_uuid(), $3, \
                 CASE WHEN $4 THEN now() ELSE NULL END)",
    )
    .bind(siret)
    .bind(payload)
    .bind(SENTINEL)
    .bind(done)
    .execute(pool)
    .await
    .expect("seed staging row");
}

async fn row(
    pool: &PgPool,
    siret: &str,
) -> (Option<serde_json::Value>, String, String, Option<chrono::DateTime<chrono::Utc>>) {
    sqlx::query_as(
        "SELECT payload, payload_hash, status, synced_at FROM external_sirene_restaurants WHERE siret = $1",
    )
    .bind(siret)
    .fetch_one(pool)
    .await
    .expect("row")
}

const REAL: &str = r#"{"siret":"85242109900021","periodesEtablissement":[{"dateFin":null,"etatAdministratifEtablissement":"A","enseigne1Etablissement":"CHEZ MARCO","activitePrincipaleEtablissement":"56.10A"}]}"#;
const PENDING: &str = r#"{"siret":"85242109900039","periodesEtablissement":[{"dateFin":null,"etatAdministratifEtablissement":"A","enseigne1Etablissement":"LE TOURANGEAU","activitePrincipaleEtablissement":"56.10C"}]}"#;
/// No `siret` — the wire type requires it, so this cannot be deserialized.
const JUNK: &str = r#"{"periodesEtablissement":[],"note":"INSEE sent something unexpected"}"#;

#[tokio::test]
async fn compaction_hashes_before_dropping_and_keeps_what_it_cannot_translate() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP compaction_hashes_before_dropping_and_keeps_what_it_cannot_translate: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

    seed(&pool, "85242109900021", REAL, true).await; // translated already
    seed(&pool, "85242109900039", PENDING, false).await; // worker has not seen it
    seed(&pool, "85242109900047", JUNK, true).await; // unparsable

    let counts = compact_payloads(&pool, std::time::Duration::from_secs(60))
        .await
        .expect("compaction runs");
    assert_eq!(counts.examined, 3);
    assert_eq!(counts.compacted, 1);
    assert_eq!(counts.rehashed_pending, 1);
    assert_eq!(counts.unparsable, 1);

    // 1. The translated row: payload gone, and a REAL hash in place of the sentinel. The order matters
    //    — a payload dropped while the sentinel stood would make the next sweep re-pend the row and
    //    re-write the payload, undoing the whole change.
    let (payload, hash, status, synced_at) = row(&pool, "85242109900021").await;
    assert!(payload.is_none(), "a translated row loses its payload — that is the 655 MB");
    assert_ne!(hash, SENTINEL, "and gains a real hash, so the next sweep sees 'unchanged'");
    assert_eq!(hash.len(), 64, "SHA-256 as hex");
    assert_eq!(status, "SYNCED", "and says so — the column answers 'has this been synced?' directly");
    // Backfilled from `processed_at`, not stamped now(): these rows were synced in the PAST, and
    // pretending otherwise would make the whole mirror look freshly translated the day it compacts.
    assert!(synced_at.is_some(), "a row known to have been synced records WHEN");

    // 2. The pending row keeps its payload — the worker still has to translate it — but is re-hashed,
    //    because the worker seeds its inbound `external_id` with that hash.
    let (payload, hash, status, synced_at) = row(&pool, "85242109900039").await;
    assert!(payload.is_some(), "a row the worker has not drained must keep what it will translate");
    assert_ne!(hash, SENTINEL);
    assert_eq!(status, "PENDING", "not yet synced");
    assert!(synced_at.is_none(), "and therefore has no sync timestamp");

    // 3. The unparsable row is left completely alone: no computable hash, and the payload is the only
    //    evidence of why INSEE's record was unusable (D3).
    let (payload, hash, status, synced_at) = row(&pool, "85242109900047").await;
    assert!(payload.is_some(), "evidence of an unusable record is never discarded");
    assert_eq!(hash, SENTINEL, "and nothing is half-written");
    // THE reason `status` exists: without it this row is indistinguishable from the PENDING one above
    // — both simply "have a payload". The status is what makes the table readable once the payload
    // stopped being universal.
    assert_eq!(status, "UNMAPPABLE", "and the operator can see WHY the payload is still there");
    assert!(synced_at.is_none(), "never reached the domain, so it never synced");

    // Re-running is a no-op on what it already did: the compacted row has dropped out of the
    // `payload IS NOT NULL` predicate, which is what makes the pass resumable across CI runs.
    let again = compact_payloads(&pool, std::time::Duration::from_secs(60))
        .await
        .expect("second compaction runs");
    assert_eq!(again.compacted, 0, "nothing left to compact");
    assert_eq!(again.examined, 2, "only the pending and unparsable rows are still selected");
}
