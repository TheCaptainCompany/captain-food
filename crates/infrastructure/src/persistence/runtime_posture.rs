//! Process-wide runtime postures (#318, ADR-20260803-104819): ONE `RuntimePosture` database row
//! per posture, read at STARTUP by every process the posture governs — the monolith composition
//! root and every standalone adapter fleet. Replaces the per-process env read of
//! `PM_MAILBOX_DELIVERY`: per-process env is per-deploy state, and a drifted gate value on one
//! adapter fleet delivering Payment facts without the PM chain hop is the silent paid-order
//! stall ADR-20260803-002712 Q4 exists to remove.
//!
//! The read is deliberately three-valued, because the SAFE reaction differs by cause:
//! - a read VALUE is the posture, process-wide, until the next restart;
//! - a MISSING row/table ([`PostureRead::Unprovable`]) resolves deterministically to the legacy
//!   arm — no process can read `true` from a database state the row does not exist in, so
//!   everyone converging on gate-off/refuse-money-lanes is consistent by construction;
//! - a TRANSIENT error (`Err`) means a peer may have read `true` — the caller must not guess:
//!   the monolith refuses to start, an adapter fleet waits for the row to answer.

use sqlx::PgPool;

/// The posture key of the Runtime D1 money gate (ADR-20260801-023000) — uppercase snake,
/// mirroring the retired env-key spelling so the gate keeps its name across the move.
pub const PM_MAILBOX_DELIVERY: &str = "PM_MAILBOX_DELIVERY";

/// Outcome of a posture-row read that REACHED the database (transport failures stay `Err`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostureRead {
    /// The row answered: this is the posture, process-wide, until the next restart.
    Enabled(bool),
    /// The database answered but cannot PROVE a posture: the row is missing, or the table
    /// itself is (schema behind this binary / an unseeded database). Deterministic across every
    /// process reading the same state — the caller falls to the legacy arm, never a guess.
    Unprovable(&'static str),
}

/// Read one posture row. `Err` is strictly TRANSPORT (pool/connection/timeout) — a database
/// that answers "no such table" (42P01) or "no row" resolves to [`PostureRead::Unprovable`].
pub async fn read_posture(pool: &PgPool, posture: &str) -> Result<PostureRead, sqlx::Error> {
    match sqlx::query_scalar::<_, bool>("SELECT enabled FROM RuntimePosture WHERE posture = $1")
        .bind(posture)
        .fetch_optional(pool)
        .await
    {
        Ok(Some(enabled)) => Ok(PostureRead::Enabled(enabled)),
        Ok(None) => Ok(PostureRead::Unprovable("posture row missing (unseeded database)")),
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("42P01") => Ok(
            PostureRead::Unprovable("RuntimePosture table missing (schema behind this binary)"),
        ),
        Err(e) => Err(e),
    }
}
