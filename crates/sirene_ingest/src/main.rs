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
//! Usage: `sirene_ingest --once` (designed for a scheduled GitHub Actions run).
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

use sirene_ingest::{
    french_departments, restauration_query, upsert_staging_batch, SireneClient, SireneScope,
    MAX_PAGE_SIZE, STAGING_BATCH_SIZE,
};

/// The new INSEE portal's public quota is ~30 requests/minute — pace page fetches accordingly.
const PAGE_PAUSE: std::time::Duration = std::time::Duration::from_millis(2100);

#[tokio::main]
async fn main() {
    if !std::env::args().any(|a| a == "--once") {
        eprintln!("usage: sirene_ingest --once   (one full ingestion pass, then exit)");
        std::process::exit(2);
    }

    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(std::time::Duration::from_secs(15))
        .connect(&url)
        .await
        .expect("connect to DATABASE_URL");

    let client = SireneClient::from_env().unwrap_or_else(|e| {
        eprintln!("sirene_ingest: {e}");
        std::process::exit(2);
    });

    // One run id correlates every row this ingestion touched (staging column `sync_run_id`).
    let sync_run_id = uuid::Uuid::new_v4();
    let departments: Vec<String> = match std::env::var("SIRENE_DEPARTMENTS") {
        Ok(list) if !list.trim().is_empty() => {
            list.split(',').map(|d| d.trim().to_string()).filter(|d| !d.is_empty()).collect()
        }
        _ => french_departments(),
    };
    println!(
        "sirene_ingest: run {sync_run_id} — {} department partition(s), active food-service scope",
        departments.len()
    );

    let (mut fetched, mut upserted, mut failed_rows) = (0usize, 0usize, 0usize);
    let mut failed_departments: Vec<String> = Vec::new();

    for department in &departments {
        let query = restauration_query(&SireneScope::Department(department.clone()));
        let mut cursor = "*".to_string();
        let mut department_fetched = 0usize;
        loop {
            let page = match client.fetch_page(&query, &cursor, MAX_PAGE_SIZE).await {
                Ok(page) => page,
                Err(e) => {
                    // A page-level failure (after the client's own retries) fails THIS department
                    // only; the sweep continues — re-runs are idempotent UPSERTs anyway.
                    eprintln!("sirene_ingest: department {department} failed at cursor {cursor}: {e}");
                    failed_departments.push(department.clone());
                    break;
                }
            };
            department_fetched += page.records.len();
            // Batch the staging write: each page is UPSERTed in chunks of STAGING_BATCH_SIZE in one
            // round-trip each (with a row-by-row fallback inside `upsert_staging_batch` on batch error),
            // instead of one round-trip per record — the fix for the throughput bottleneck (#215). The
            // `upserted`/`failed_rows` tallies keep their exact meaning.
            for chunk in page.records.chunks(STAGING_BATCH_SIZE) {
                let counts = upsert_staging_batch(&pool, chunk, department, sync_run_id).await;
                upserted += counts.upserted;
                failed_rows += counts.failed_rows;
            }
            match page.next_cursor {
                Some(next) => {
                    cursor = next;
                    tokio::time::sleep(PAGE_PAUSE).await; // stay under the INSEE rate limit
                }
                None => break,
            }
        }
        fetched += department_fetched;
        if department_fetched > 0 {
            println!("sirene_ingest: department {department} — {department_fetched} établissements");
        }
        tokio::time::sleep(PAGE_PAUSE).await; // pace department-to-department requests too
    }

    println!(
        "sirene_ingest: done — fetched {fetched}, upserted {upserted}, failed rows {failed_rows}, \
         failed departments {:?}",
        failed_departments
    );

    // Wake the on-app worker so staged rows are translated without waiting for its poll interval.
    // Best-effort: a ping failure never fails the run (the worker's own loop will catch up).
    ping_internal_trigger().await;

    // Surface partial sweeps in the Actions run: some data landed (and was pinged), but not all.
    if !failed_departments.is_empty() || (failed_rows > 0 && upserted == 0 && fetched > 0) {
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
