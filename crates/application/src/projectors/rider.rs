//! Hand-written `RiderCompute` (ADR-0040) for the rider identity read model (#639 part A, extended
//! #639 part C step 4-i, ADR-20260904-081527 §2).
//!
//! Five of six columns are mechanical, and the generated `project_rider` dispatch maps them
//! straight off the rider facts. `RiderRegistered` carries `riderId`, `authRef`, `displayName`,
//! `phone` and `status` and all are required, so the creation arm needs no computation for them;
//! `RiderInfoUpdated` and `RiderStatusChanged` only overwrite. `standing` is the one COMPLEX
//! column, and deliberately so: the creating arm (`RiderRegistered`) must never WRITE it — the
//! ADR's replay-neutral fold. On creation this hook returns the PRIOR row's standing when one
//! already exists (a checkpoint-reset replay-in-place over an existing, possibly RESTRICTED row —
//! never TRUNCATE, so the row survives the rewind) and only defaults a genuinely fresh row to
//! ACTIVE (mirroring the column's own SQL `DEFAULT 'ACTIVE'`); RiderRestricted/RiderReinstated are
//! the ONLY facts that ever move it on a live row.

use crate::generated::rows::RiderRow;
use crate::projections::{Envelope, RiderCompute};
use domain::generated::events::DomainEvent;
use domain::generated::scalars::RiderStanding;

pub struct RiderProjector;

impl RiderCompute for RiderProjector {
    fn standing(&self, prev: Option<&RiderRow>, env: &Envelope) -> RiderStanding {
        match &env.event {
            DomainEvent::RiderRestricted(_) => RiderStanding::RESTRICTED,
            DomainEvent::RiderReinstated(_) => RiderStanding::ACTIVE,
            // RiderRegistered (creation) and any other event this table folds: never touch it —
            // preserve whatever is already there, defaulting to ACTIVE only for a genuinely fresh
            // row (`prev` is `None`).
            _ => prev.map(|r| r.standing).unwrap_or(RiderStanding::ACTIVE),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projections::Envelope;
    use domain::generated::events::RiderRegistered;
    use domain::generated::scalars::{AuthSubject, PhoneNumber, RiderId, RiderStatus};

    fn registered_envelope(rider_id: uuid::Uuid) -> Envelope {
        Envelope {
            stream_name: format!("Rider-{rider_id}"),
            position: 1,
            occurred_at: chrono::Utc::now(),
            event: DomainEvent::RiderRegistered(RiderRegistered {
                rider_id: RiderId(rider_id),
                auth_ref: AuthSubject("auth-1".into()),
                display_name: "Léa".into(),
                phone: PhoneNumber("+33600000000".into()),
                status: RiderStatus::OFFLINE,
            }),
        }
    }

    fn restricted_row(rider_id: uuid::Uuid) -> RiderRow {
        RiderRow {
            rider_id: RiderId(rider_id),
            auth_ref: AuthSubject("auth-1".into()),
            display_name: "Léa".into(),
            phone: PhoneNumber("+33600000000".into()),
            status: RiderStatus::OFFLINE,
            standing: RiderStanding::RESTRICTED,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    /// M5 (#639 part C step 4-i card): the creating arm (`RiderRegistered`) must NEVER write
    /// `standing` — a from-zero replay-in-place over an EXISTING restricted row (`prev` carries
    /// it) must preserve RESTRICTED, never reset to ACTIVE. Direct unit test on the Compute hook
    /// itself: the DB-gated `replaying_rider_registered_over_a_restricted_row_keeps_it_restricted`
    /// integration test cannot isolate this arm alone — `run_once()` drains a whole stream to
    /// exhaustion in one call, so a LATER `RiderRestricted` on the same stream always re-heals the
    /// row before the drain returns, masking a creating arm that misbehaves. This hook-level test
    /// has no such mask: it calls the mutated function directly, with no self-healing event after it.
    /// Seen RED verbatim (`left: ACTIVE, right: RESTRICTED`) against the mutant `_ =>
    /// RiderStanding::ACTIVE` — recorded in the hand-back, not repeated here.
    #[test]
    fn the_creating_arm_never_writes_standing_over_an_existing_restricted_row() {
        let rider_id = uuid::Uuid::new_v4();
        let got = RiderProjector.standing(Some(&restricted_row(rider_id)), &registered_envelope(rider_id));
        assert_eq!(got, RiderStanding::RESTRICTED, "a replayed creation must preserve the prior standing");
    }

    /// The other half: a GENUINELY fresh row (`prev = None`) defaults to ACTIVE — the column's own
    /// SQL `DEFAULT 'ACTIVE'`, mirrored here.
    #[test]
    fn a_genuinely_fresh_row_defaults_to_active() {
        let rider_id = uuid::Uuid::new_v4();
        let got = RiderProjector.standing(None, &registered_envelope(rider_id));
        assert_eq!(got, RiderStanding::ACTIVE);
    }
}
