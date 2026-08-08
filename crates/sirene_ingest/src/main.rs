//! SIRENE raw-ingestion runner (ADR-0045) — the THIN scheduled CI job.
//!
//! Fetches ALL currently-active food-service établissements (NAF 56.10A/B/C + 56.30Z) across France
//! from the INSEE Sirene API, partitioned by department (one cursor sweep per department keeps every
//! partition far below INSEE's deep-pagination cap and isolates failures), and UPSERTs each record RAW
//! into the `external_sirene_restaurants` staging table. No ACL, no aggregate, no domain crates: the
//! on-app `sync_sirene_worker` (versioned with the deployed server) translates staged rows into domain
//! commands, so the version-skew hazard of the retired direct-write `sirene_sync` binary is gone.
//!
//! On completion it optionally POSTs the server's internal drain endpoint to wake the worker.
//!
//! Usage: `sirene_ingest --once`    (designed for a scheduled GitHub Actions run)
//!        `sirene_ingest --compact` (one-shot payload compaction, #231 — no INSEE calls, DB only)
//! Env:
//! - `DATABASE_URL`           (required) — Postgres; only the staging table is written (a
//!   limited-privilege role scoped to it is recommended, ADR-0045).
//! - `INSEE_API_TOKEN`        (required) — API key from the INSEE portal (portail-api.insee.fr).
//! - `INSEE_API_BASE_URL`     (optional) — overrides `https://api.insee.fr/api-sirene/3.11`.
//! - `SIRENE_DEPARTMENTS`     (optional) — comma-separated department codes (e.g. `37` or `37,41`)
//!   instead of the full France sweep; useful for the first import and debugging.
//! - `INTERNAL_TRIGGER_URL`   (optional) — the server's drain endpoint to ping when done
//!   (e.g. `https://<app>/internal/sirene/drain`).
//! - `INTERNAL_TRIGGER_TOKEN` (optional, required with the URL) — shared secret sent as the
//!   `x-internal-token` header; the server rejects the ping without it.

use sirene_ingest::sweep::{
    budget_from_env, sweep_from_env, DEFAULT_COMPACTION_BUDGET_MINUTES,
};
use sirene_ingest::compact_payloads;

#[tokio::main]
async fn main() {
    let compact_only = std::env::args().any(|a| a == "--compact");
    if !compact_only && !std::env::args().any(|a| a == "--once") {
        eprintln!(
            "usage: sirene_ingest --once      (one full ingestion pass, then exit)\n\
             \x20      sirene_ingest --compact   (one-shot payload compaction pass, then exit)"
        );
        std::process::exit(2);
    }

    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(std::time::Duration::from_secs(15))
        .connect(&url)
        .await
        .expect("connect to DATABASE_URL");

    // The compaction pass (#231) touches no INSEE API at all — it only reads payloads already in the
    // staging table — so it runs without an INSEE client, and needs no INSEE_API_TOKEN.
    if compact_only {
        let budget = budget_from_env(DEFAULT_COMPACTION_BUDGET_MINUTES);
        println!(
            "sirene_ingest: compaction pass — budget {} min, recomputing hashes and dropping \
             translated payloads",
            budget.as_secs() / 60
        );
        match compact_payloads(&pool, budget).await {
            Ok(counts) => {
                println!("sirene_ingest: compaction done — {counts:?}");
                // Re-runnable: rows left pending or unparsable are picked up next time (or drained by
                // the worker), so an incomplete pass is normal, not a failure.
            }
            Err(e) => {
                eprintln!("sirene_ingest: compaction failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    // The sweep orchestration is SHARED with the generated worker-sirene-sync CronJob bin
    // (sirene_ingest::sweep, ADR-20260808-062933 — one loop, two schedulers, zero forks); this
    // main keeps only what is CI-specific: the exit codes and the deployed-worker drain ping.
    let report = match sweep_from_env(&pool).await {
        Ok(report) => report,
        Err(e) => {
            eprintln!("sirene_ingest: {e}");
            std::process::exit(2);
        }
    };

    // Wake the on-app worker so staged rows are translated without waiting for its poll interval.
    // Best-effort: a ping failure never fails the run (the worker's own loop will catch up).
    ping_internal_trigger().await;

    // Surface partial sweeps in the Actions run: some data landed (and was pinged), but not all.
    if report.is_failure() {
        std::process::exit(1);
    }
}

/// POST `INTERNAL_TRIGGER_URL` with the `x-internal-token: $INTERNAL_TRIGGER_TOKEN` header, if
/// configured. The server's `/internal/sirene/drain` endpoint rejects unauthenticated pings.
async fn ping_internal_trigger() {
    let Ok(url) = std::env::var("INTERNAL_TRIGGER_URL") else {
        println!("sirene_ingest: INTERNAL_TRIGGER_URL not set — skipping the worker ping");
        return;
    };
    if url.trim().is_empty() {
        println!("sirene_ingest: INTERNAL_TRIGGER_URL empty — skipping the worker ping");
        return;
    }
    let token = std::env::var("INTERNAL_TRIGGER_TOKEN").unwrap_or_default();
    if token.trim().is_empty() {
        eprintln!("sirene_ingest: INTERNAL_TRIGGER_URL set but INTERNAL_TRIGGER_TOKEN missing — skipping");
        return;
    }
    let http = reqwest::Client::new();
    match http
        .post(url.trim())
        .header("x-internal-token", token.trim())
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            println!("sirene_ingest: worker ping accepted ({})", resp.status());
        }
        Ok(resp) => eprintln!("sirene_ingest: worker ping rejected ({})", resp.status()),
        Err(e) => eprintln!("sirene_ingest: worker ping failed: {e}"),
    }
}
