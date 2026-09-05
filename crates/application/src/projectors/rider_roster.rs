//! Hand-written `RiderRosterCompute` (ADR-0040) for the admin rider roster read model (#639 part C
//! step 4-iii-A, ADR-20260904-152807 §1). `standing` is fully mechanical (`derive:` literals) and
//! `display_name`/`phone`/`status`/`ground`/`reinstated_at` are flat/occurrence columns — the
//! generator maps all of those inline. Only `decided_at`/`effective_at` need a hand-written hook:
//! timestamptz VALUE columns parsed from `RiderRestricted`'s RFC3339 payload strings (the
//! `RiderRestrictionCompute` precedent, `crates/application/src/projectors/rider_restriction.rs`,
//! generalised one table further) and preserved otherwise.

use crate::generated::rows::RiderRosterRow;
use crate::projections::{Envelope, RiderRosterCompute};
use domain::generated::events::DomainEvent;

pub struct RiderRosterProjector;

impl RiderRosterCompute for RiderRosterProjector {
    fn decided_at(
        &self,
        prev: Option<&RiderRosterRow>,
        env: &Envelope,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        match &env.event {
            DomainEvent::RiderRestricted(e) => e.decided_at.parse().ok(),
            _ => prev.and_then(|r| r.decided_at),
        }
    }

    fn effective_at(
        &self,
        prev: Option<&RiderRosterRow>,
        env: &Envelope,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        match &env.event {
            DomainEvent::RiderRestricted(e) => e.effective_at.parse().ok(),
            _ => prev.and_then(|r| r.effective_at),
        }
    }
}
