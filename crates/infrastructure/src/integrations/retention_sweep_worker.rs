//! The on-app retention sweep worker (ADR-20260721-025159) — the scheduler half of the journal /
//! webhook-mirror retention policy; the policy itself (windows + predicates) lives entirely in the
//! `sweep_retention()` SQL function (`specs/database/functions/sweep_retention.sql`), so this worker
//! is a dumb periodic caller and the windows can never drift between code sites.
//!
//! Scope (see the ADR): `command_journal` terminal rows (90 d), `inbound_events` DELIVERED rows
//! (30 d), `external_stripe_events` / `external_hubrise_callbacks` processed rows (90 d). NEVER
//! swept: `domain_events`/`domain_stream` (the forever log — the function does not reference them),
//! RECEIVED journal rows, FAILED inbound rows (kept until resolved), unprocessed mirror rows, and
//! `external_sirene_restaurants` (full mirror, detect-by-absence needs every row).
//!
//! Same operational shape as the other in-process workers: the poll loop is env-gated in the
//! composition root (`RUN_RETENTION_SWEEP`, default on); a `pg_cron` job calling the same function
//! is the documented alternative where DB-side scheduling is preferred.

use std::sync::Arc;
use std::time::Duration;

use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};

use crate::persistence::db_err;

/// Sweep cadence. Retention windows are measured in days, so anything sub-daily is purely about
/// smoothing the delete batches; 6 h keeps a free-tier instance's passes small.
const SWEEP_INTERVAL: Duration = Duration::from_secs(6 * 3600);

/// One sweep pass's per-table delete counters, as reported by `sweep_retention()`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RetentionSweepSummary {
    pub command_journal: u64,
    pub inbound_events: u64,
    pub external_stripe_events: u64,
    pub external_hubrise_callbacks: u64,
}

impl RetentionSweepSummary {
    pub fn total(&self) -> u64 {
        self.command_journal
            + self.inbound_events
            + self.external_stripe_events
            + self.external_hubrise_callbacks
    }
}

/// The retention sweep worker: calls `sweep_retention()` on an interval (first pass at boot).
pub struct RetentionSweepWorker {
    pool: PgPool,
}

impl RetentionSweepWorker {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// One sweep pass — a single call to the SQL function that owns the whole policy.
    pub async fn run_once(&self) -> Result<RetentionSweepSummary, DomainError> {
        let rows = sqlx::query("SELECT swept_table, deleted FROM sweep_retention()")
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        let mut summary = RetentionSweepSummary::default();
        for row in rows {
            let table: String = row.try_get("swept_table").map_err(db_err)?;
            let deleted: i64 = row.try_get("deleted").map_err(db_err)?;
            let deleted = deleted.max(0) as u64;
            match table.as_str() {
                "command_journal" => summary.command_journal = deleted,
                "inbound_events" => summary.inbound_events = deleted,
                "external_stripe_events" => summary.external_stripe_events = deleted,
                "external_hubrise_callbacks" => summary.external_hubrise_callbacks = deleted,
                // A future window added to the function must not silently vanish from the logs.
                other => eprintln!("retention sweep: unmapped swept_table '{other}' ({deleted} rows)"),
            }
        }
        Ok(summary)
    }

    /// The periodic loop (first pass at boot, then every [`SWEEP_INTERVAL`]).
    pub async fn run_loop(self: Arc<Self>) {
        loop {
            match self.run_once().await {
                Ok(s) if s.total() > 0 => println!(
                    "retention sweep: command_journal={} inbound_events={} external_stripe_events={} external_hubrise_callbacks={}",
                    s.command_journal, s.inbound_events, s.external_stripe_events, s.external_hubrise_callbacks
                ),
                Ok(_) => {}
                Err(e) => eprintln!("retention sweep failed: {e}"),
            }
            tokio::time::sleep(SWEEP_INTERVAL).await;
        }
    }
}
