//! The SIRENE sweep ORCHESTRATION (#218/#215), extracted from the `sirene_ingest` bin's main so
//! it has exactly two callers and zero forks (ADR-20260808-062933 "one bin per worker"): the
//! scheduled GitHub Actions job (`sirene_ingest --once`, authoritative until the #358 cutover)
//! and the generated `worker-sirene-sync` CronJob bin. Both run THIS loop; the mains keep only
//! argument parsing, exit codes and (for the CI job) the drain ping.
//!
//! The shape is unchanged from the main it came out of: department partitions swept
//! stalest-first (or an explicit `SIRENE_DEPARTMENTS` list, honoured verbatim and in order)
//! within a self-imposed wall-clock budget, stopping cleanly BETWEEN departments (a half-swept
//! department would look fully swept to `departments_by_staleness` and its unfetched tail would
//! silently rot), one idempotent UPSERT batch per page chunk.

use crate::{
    departments_by_staleness, french_departments, restauration_query, upsert_staging_batch,
    SireneClient, SireneScope, MAX_PAGE_SIZE, STAGING_BATCH_SIZE,
};

/// The new INSEE portal's public quota is ~30 requests/minute — pace page fetches accordingly.
pub const PAGE_PAUSE: std::time::Duration = std::time::Duration::from_millis(2100);

/// How long one run may spend fetching before it stops and leaves the rest to the next run (#218).
///
/// The bottleneck is INSEE's ~30 req/min quota, not our DB (that was #215/#216), so a full France
/// sweep needs ~4 hours and CANNOT fit a single run — the previous behaviour was to run until the
/// 90-minute CI `timeout-minutes` ceiling KILLED it, which loses the summary, the worker ping and
/// the exit status, and reports a red job for working as designed.
///
/// So the run budgets itself and stops cleanly instead. Default 75 minutes, comfortably inside
/// the CI workflow's 90 (and the generated CronJob's activeDeadlineSeconds), leaving room for the
/// final drain and shutdown. Coverage completes across successive runs because
/// [`departments_by_staleness`] resumes at the stalest partition.
pub const DEFAULT_BUDGET_MINUTES: u64 = 75;

/// Wall-clock budget for `--compact` (#231). Same self-limiting shape as the sweep's, and for the
/// same reason: the pass is resumable (`payload IS NOT NULL` is its own progress marker), so
/// stopping early costs nothing and being KILLED at the CI ceiling loses the summary.
pub const DEFAULT_COMPACTION_BUDGET_MINUTES: u64 = 60;

/// `SIRENE_BUDGET_MINUTES` or the given default, as a Duration. An unparsable/empty value counts
/// as "not set" so an untouched dispatch form behaves exactly like the schedule does.
pub fn budget_from_env(default_minutes: u64) -> std::time::Duration {
    std::time::Duration::from_secs(
        std::env::var("SIRENE_BUDGET_MINUTES")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(default_minutes)
            * 60,
    )
}

/// One sweep run's tallies — what the callers print/log and turn into an exit code.
#[derive(Debug, Default)]
pub struct SweepReport {
    pub sync_run_id: uuid::Uuid,
    /// Departments fully swept this run (in sweep order).
    pub covered: usize,
    /// Departments the run set out to sweep (explicit list or all of France).
    pub planned: usize,
    pub fetched: usize,
    pub upserted: usize,
    pub failed_rows: usize,
    pub failed_departments: Vec<String>,
    pub budget_exhausted: bool,
}

impl SweepReport {
    /// The CI job's red/green rule, unchanged: a failed department, or rows that all failed to
    /// stage while something WAS fetched, is a partial sweep worth a red run.
    pub fn is_failure(&self) -> bool {
        !self.failed_departments.is_empty()
            || (self.failed_rows > 0 && self.upserted == 0 && self.fetched > 0)
    }
}

/// One full ingestion pass, resolved from the environment exactly as the CI job does it:
/// `SireneClient::from_env` (FAILS without `INSEE_API_TOKEN` — a sweep with no token refuses
/// rather than silently ingesting zero établissements), `SIRENE_DEPARTMENTS` verbatim or
/// stalest-first, `SIRENE_BUDGET_MINUTES` or the default.
pub async fn sweep_from_env(pool: &sqlx::PgPool) -> Result<SweepReport, String> {
    let client = SireneClient::from_env().map_err(|e| e.to_string())?;
    let departments: Vec<String> = match std::env::var("SIRENE_DEPARTMENTS") {
        // An explicit list is honoured verbatim and in the given order — debugging and the
        // single-market V0 scope must stay predictable, not get reordered under the operator.
        Ok(list) if !list.trim().is_empty() => {
            list.split(',').map(|d| d.trim().to_string()).filter(|d| !d.is_empty()).collect()
        }
        // Otherwise sweep stalest-first so successive runs converge on full coverage instead of
        // restarting at department 01 every time and never passing 37 (#218).
        _ => departments_by_staleness(pool, &french_departments()).await,
    };
    let budget = budget_from_env(DEFAULT_BUDGET_MINUTES);
    Ok(run_sweep(pool, &client, &departments, budget).await)
}

/// The sweep loop itself (extracted verbatim from the bin main). Progress goes to stdout with the
/// historical `sirene_ingest:` prefix — both callers are run-to-completion processes whose stdout
/// IS the run log (the Actions transcript / the Job pod log).
pub async fn run_sweep(
    pool: &sqlx::PgPool,
    client: &SireneClient,
    departments: &[String],
    budget: std::time::Duration,
) -> SweepReport {
    // One run id correlates every row this ingestion touched (staging column `sync_run_id`).
    let sync_run_id = uuid::Uuid::new_v4();
    let started = std::time::Instant::now();
    println!(
        "sirene_ingest: run {sync_run_id} — {} department partition(s), stalest first, \
         budget {} min, active food-service scope",
        departments.len(),
        budget.as_secs() / 60
    );

    let mut report = SweepReport {
        sync_run_id,
        planned: departments.len(),
        ..SweepReport::default()
    };

    for department in departments {
        // Stop BETWEEN departments, never mid-partition: a half-swept department would look fully
        // swept to `departments_by_staleness` (its max(last_seen_at) is now), so the next run
        // would deprioritise it and its unfetched tail would silently rot.
        if started.elapsed() >= budget {
            report.budget_exhausted = true;
            println!(
                "sirene_ingest: budget of {} min reached after {} department(s) — stopping cleanly, \
                 the next run resumes at the stalest partition",
                budget.as_secs() / 60,
                report.covered
            );
            break;
        }
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
                    report.failed_departments.push(department.clone());
                    break;
                }
            };
            department_fetched += page.records.len();
            // Batch the staging write: each page is UPSERTed in chunks of STAGING_BATCH_SIZE in
            // one round-trip each (with a row-by-row fallback inside `upsert_staging_batch` on
            // batch error), instead of one round-trip per record — the fix for the throughput
            // bottleneck (#215). The `upserted`/`failed_rows` tallies keep their exact meaning.
            for chunk in page.records.chunks(STAGING_BATCH_SIZE) {
                let counts = upsert_staging_batch(pool, chunk, department, sync_run_id).await;
                report.upserted += counts.upserted;
                report.failed_rows += counts.failed_rows;
            }
            match page.next_cursor {
                Some(next) => {
                    cursor = next;
                    tokio::time::sleep(PAGE_PAUSE).await; // stay under the INSEE rate limit
                }
                None => break,
            }
        }
        report.fetched += department_fetched;
        report.covered += 1;
        if department_fetched > 0 {
            println!("sirene_ingest: department {department} — {department_fetched} établissements");
        }
        tokio::time::sleep(PAGE_PAUSE).await; // pace department-to-department requests too
    }

    println!(
        "sirene_ingest: done — {} of {} department(s) this run, fetched {}, upserted {}, \
         failed rows {}, failed departments {:?}{}",
        report.covered,
        report.planned,
        report.fetched,
        report.upserted,
        report.failed_rows,
        report.failed_departments,
        if report.budget_exhausted { " (budget-limited — remaining partitions next run)" } else { "" }
    );
    report
}
