//! The command_journal stale-RECEIVED sweep (ADR-20260720-015300), as ONE shared pass with two
//! schedulers and zero forks (ADR-20260808-062933 "one bin per worker"): the monolith composition
//! root loops it on [`SWEEP_INTERVAL_SECONDS`], and the generated `worker-journal-sweep` CronJob
//! bin runs one pass per firing (its `schedule:` in c4-l2 mirrors the same cadence).
//!
//! Why it exists: a spawned command run that CRASHED between acceptance and its terminal journal
//! row would stay RECEIVED forever, and `operationStatus` would report a dead run as pending
//! indefinitely. Flipping stale RECEIVED rows to FAILED after [`STALE_RECEIVED_MINUTES`] is the
//! liveness backstop. It SURVIVES the drain worker's retirement (ADR-20260731-122500): the
//! journal is still the PM legs' door until Runtime D — this sweep retires WITH `command_journal`
//! itself.

use domain::shared::errors::DomainError;
use sqlx::PgPool;

/// A RECEIVED row older than this is a dead run (nothing legitimately runs this long between
/// acceptance and its terminal journal row).
pub const STALE_RECEIVED_MINUTES: i64 = 10;

/// The monolith loop's cadence. The generated `worker-journal-sweep` CronJob's `schedule:`
/// (c4-l2) expresses the same cadence in cron terms — sweeping at half the stale window keeps
/// the worst-case "dead run reported pending" interval at ~1.5× the window.
pub const SWEEP_INTERVAL_SECONDS: u64 = 300;

/// One sweep pass: flip every stale RECEIVED command to FAILED. Returns how many rows flipped.
pub async fn sweep_stale_received_once(pool: &PgPool) -> Result<u64, DomainError> {
    use application::journal::CommandJournal as _;
    crate::persistence::command_journal::PgCommandJournal::new(pool.clone())
        .sweep_stale_received(chrono::Duration::minutes(STALE_RECEIVED_MINUTES))
        .await
}
