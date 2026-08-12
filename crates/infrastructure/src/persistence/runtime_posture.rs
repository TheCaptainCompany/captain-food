//! Process-wide runtime postures (#318, ADR-20260803-104819): ONE `RuntimePosture` database row
//! per posture, read at STARTUP by every process the posture governs. A posture is deliberately
//! NOT per-process env: env is per-deploy state, and a drifted value across a fleet is how one
//! process behaves differently from its peers on a money path.
//!
//! NO POSTURE IS DECLARED TODAY. The mechanism's first and only tenant, `PM_MAILBOX_DELIVERY`,
//! retired with `command_journal` in #242 Runtime D (ADR-20260812-000000): its OFF arm was the
//! legacy journal+spawn path, so once that table went there was nothing left to pick. The read
//! stays — the next process-wide posture needs exactly this shape, and a posture read is the kind
//! of thing that must be right the first time.
//!
//! The read is deliberately three-valued, because the SAFE reaction differs by cause:
//! - a read VALUE is the posture, process-wide, until the next restart;
//! - a MISSING row/table ([`PostureRead::Unprovable`]) resolves deterministically to the
//!   conservative arm — no process can read `true` from a database state the row does not exist
//!   in, so every process converging on the same answer is consistent by construction;
//! - a TRANSIENT error (`Err`) means a peer may have read `true` — the caller must not guess.

use sqlx::PgPool;

/// Outcome of a posture-row read that REACHED the database (transport failures stay `Err`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostureRead {
    /// The row answered: this is the posture, process-wide, until the next restart.
    Enabled(bool),
    /// The database answered but cannot PROVE a posture: the row is missing, or the table
    /// itself is (schema behind this binary / an unseeded database). Deterministic across every
    /// process reading the same state — the caller takes its conservative arm, never a guess.
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
