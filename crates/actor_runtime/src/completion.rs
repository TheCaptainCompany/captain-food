//! The FENCED COMPLETION TRANSACTION (PROP-20260728-152752 §3.1/§3.4) — the one place a delivery
//! becomes true. One `BEGIN … COMMIT` carries all of: the handler's own effects (event appends,
//! process-manager state, scheduled reminders — whatever the host wrote through the transaction),
//! the mailbox row's terminal flip, and the checkpoint advance — the last two guarded so a stale
//! owner CANNOT commit:
//!
//! - the row flip matches only `status = 'RECEIVED'` (a row the new owner already completed is
//!   gone from under the stale owner);
//! - the checkpoint advance matches only `claimed_by = me AND ownership_version = mine` (a stolen
//!   lane's bumped counter matches nothing).
//!
//! Either guard matching 0 rows rolls the WHOLE transaction back — handler effects included.
//! That is the §3.1 guarantee: dual BELIEF (two workers thinking they own a lane, inside the
//! steal window) is tolerated because dual AUTHORITY (two workers both able to commit) is
//! impossible.

use sqlx::{PgPool, Postgres, Transaction};

use crate::lease::{advance_checkpoint_fenced, Lane};
use crate::message::{HandlerVerdict, InboundMessage, MessageHandler};

/// Why a delivery did not commit.
#[derive(Debug)]
pub enum CompletionError {
    /// The fence rejected the commit — this worker's authority over the lane is stale (stolen or
    /// re-claimed). Drop the lane; the new owner redelivers the message. Nothing was written.
    FencedOut,
    /// The mailbox row was no longer RECEIVED — the new owner already completed it (the other
    /// side of the same race). Nothing was written.
    AlreadyCompleted,
    /// Infrastructure failure (connection, SQL). The row stays RECEIVED; redelivery retries.
    Db(sqlx::Error),
}

impl std::fmt::Display for CompletionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompletionError::FencedOut => write!(f, "fenced out: stale lane authority"),
            CompletionError::AlreadyCompleted => write!(f, "row already completed by the new owner"),
            CompletionError::Db(e) => write!(f, "db: {e}"),
        }
    }
}

impl std::error::Error for CompletionError {}

impl From<sqlx::Error> for CompletionError {
    fn from(e: sqlx::Error) -> Self {
        CompletionError::Db(e)
    }
}

/// Deliver ONE message under the lane's authority: run the handler inside a fresh transaction,
/// then commit its effects + the row flip + the checkpoint advance atomically. Returns the
/// committed verdict.
pub async fn complete_fenced(
    pool: &PgPool,
    lane: &Lane,
    worker_id: &str,
    message: &InboundMessage,
    handler: &dyn MessageHandler,
) -> Result<HandlerVerdict, CompletionError> {
    let mut tx: Transaction<'_, Postgres> = pool.begin().await?;

    let delivery = handler.handle(&mut tx, message).await?;
    let verdict = delivery.verdict;

    // Terminal flip — only if still RECEIVED (guard 1).
    let flipped = sqlx::query(
        "UPDATE inbound_messages \
         SET status = $2, error = $3, completed_at = now() \
         WHERE message_id = $1 AND status = 'RECEIVED'",
    )
    .bind(message.message_id)
    .bind(verdict.status())
    .bind(verdict.error())
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    if !flipped {
        tx.rollback().await?;
        return Err(CompletionError::AlreadyCompleted);
    }

    // Checkpoint advance — only under live authority (guard 2, THE fence).
    if !advance_checkpoint_fenced(&mut tx, lane, worker_id, message.position).await? {
        tx.rollback().await?;
        return Err(CompletionError::FencedOut);
    }

    tx.commit().await?;
    // Post-commit action: fan-out of what just became durable. Never on a rolled-back path.
    if let Some(action) = delivery.post_commit {
        action();
    }
    Ok(verdict)
}
