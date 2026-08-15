//! The Ask ENVELOPE and the LOCAL adapter (PROP-20260815-142349 §7, #582 actors half).
//!
//! An ask is a typed request/reply against ONE actor instance, served — today, and until a
//! PMW-3 transport decision ever says otherwise (DECISIONS §42: NOT adopted) — by the local
//! in-process fold: load the instance's stream through the application `EventStore` port, fold
//! with the actor's own hand-written fold, project the DECLARED reply properties, and envelope
//! the answer with the fold's stream version. No transport, no mailbox involvement: an ask is
//! read-only and positionless, so it never rides `inbound_messages` and never holds a lease.
//!
//! THE WALL (PROP §6): a reply is a snapshot whose authority expires at the moment it is sent.
//! It is never stored, never folded, never projected — `served_version` rides HERE, on the
//! envelope, never as a payload field (the same envelope/payload split as `occurredAt`).

use std::time::Duration;

use application::ports::EventStore;
use domain::generated::events::DomainEvent;
use domain::shared::errors::DomainError;

/// The three modeled outcomes of an ask — the YAML never spells this type: a PM `ask:` step's
/// `as:` / `absent:` / `deadline:` keys ARE its three arms, declared once each (PROP §8.4 rule
/// 6). `Err` stays reserved for infrastructure failure; `Absent` and `Deadline` are Ok-channel
/// arms the caller must match — exhaustively, by the compiler (V5: the enum stays fully
/// matchable on purpose, and a codegen gate refuses any escape-hatch attribute on this file).
#[derive(Debug, Clone, PartialEq)]
pub enum AskOutcome<R> {
    /// The stream folded to a live state; `served_version` is the fold's stream version — the
    /// dba's PMW-3 minimum: `(stream, served_version)` travels with every answer, so a caller
    /// CAN re-assert where a fenced transaction exists.
    Answered { reply: R, served_version: i64 },
    /// The stream has no birth event — modeled, never an error string (the exact
    /// `HookOutcome::Skip(String)` defect class ADR-20260815-030206 §5 tables).
    Absent,
    /// The CALLER's declared deadline elapsed (the deadline lives on the call site, never on
    /// the answering actor — founder-decided, PROP D5). Modeled, never a hang.
    Deadline,
}

/// The local adapter: one EventStore load, one fold, one projection, one envelope.
///
/// `fold` and `project` are the actor's OWN hand fold and the generated reply projection — the
/// generated per-actor clients pass them, so the compiled projection site is the YAML-state ↔
/// hand-fold parity proof. A fold gives freshness, not atomicity (CHK-1): the answer's
/// authority expires at send, which is exactly why the version rides the envelope.
pub async fn ask_local<S, R>(
    store: &dyn EventStore,
    stream: &str,
    deadline: Duration,
    fold: impl FnOnce(&[DomainEvent]) -> Option<S>,
    project: impl FnOnce(&S) -> R,
) -> Result<AskOutcome<R>, DomainError> {
    // The CALLER's deadline bounds the whole load: a stalled pool resolves to `Deadline`, never
    // a hang (seen red first — the unwrapped await never completed, PR #583).
    let (events, version) = match tokio::time::timeout(deadline, store.load(stream)).await {
        Err(_elapsed) => return Ok(AskOutcome::Deadline),
        Ok(loaded) => loaded?,
    };
    Ok(match fold(&events) {
        None => AskOutcome::Absent,
        Some(state) => AskOutcome::Answered { reply: project(&state), served_version: version },
    })
}
