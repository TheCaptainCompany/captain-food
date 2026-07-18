//! Prospect aggregate — the PURE write-side state fold (ADR-0035/0020), mirroring `restaurant.rs`.
//! The prospect (id = restaurantId) is BORN by its first `ProspectContacted` fact; the fold only
//! tracks what the anti-spam / lifecycle invariants read. The COMPUTED prospection score lives in the
//! `ProspectionPipeline` read model, never here. No I/O, no serialization logic.

use crate::generated::events::DomainEvent;

/// What the Prospect command handlers need to know to accept or reject a command. `None` (from
/// [`fold`]) means the prospect was never contacted → `ProspectNotFound`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProspectState {
    /// Number of outreach contacts recorded so far — `ProspectContactLimitReached` (anti-spam: ≤ 3).
    pub contacts: usize,
    /// Whether the prospect replied (stops the sequence pending human follow-up).
    pub replied: bool,
    /// Whether the prospect was marked cold (no reply by J+21; sequence stopped).
    pub cold: bool,
}

/// Fold a Prospect stream (events in version order) into its current state. `None` ⇔ the stream has
/// no `ProspectContacted` yet, i.e. the prospect does not exist.
pub fn fold(events: &[DomainEvent]) -> Option<ProspectState> {
    events.iter().fold(None, apply)
}

/// Apply one event — a pure transition, total over the whole event union.
fn apply(state: Option<ProspectState>, event: &DomainEvent) -> Option<ProspectState> {
    if let DomainEvent::ProspectContacted(_) = event {
        let mut s = state.unwrap_or(ProspectState { contacts: 0, replied: false, cold: false });
        s.contacts += 1;
        return Some(s);
    }
    let mut s = state?;
    match event {
        DomainEvent::ProspectReplied(_) => s.replied = true,
        DomainEvent::ProspectMarkedCold(_) => s.cold = true,
        _ => {}
    }
    Some(s)
}
