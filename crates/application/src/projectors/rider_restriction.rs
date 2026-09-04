//! Hand-written `RiderRestrictionCompute` (ADR-0040) for the restriction attribution read model
//! (#639 part C step 4-i, ADR-20260904-081527 §2). `standing` mirrors `RiderCompute::standing`
//! (`crates/application/src/projectors/rider.rs`) exactly — the creating arm (`RiderRegistered`)
//! never moves it once a prior row exists (replay-in-place, never TRUNCATE). `decided_at` /
//! `effective_at` are timestamptz VALUE columns parsed from `RiderRestricted`'s RFC3339 payload
//! strings (the `OrderTracking::estimated_ready_at` precedent) and preserved otherwise.

use crate::generated::rows::RiderRestrictionRow;
use crate::projections::{Envelope, RiderRestrictionCompute};
use domain::generated::events::DomainEvent;
use domain::generated::scalars::RiderStanding;

pub struct RiderRestrictionProjector;

impl RiderRestrictionCompute for RiderRestrictionProjector {
    fn standing(&self, prev: Option<&RiderRestrictionRow>, env: &Envelope) -> RiderStanding {
        match &env.event {
            DomainEvent::RiderRestricted(_) => RiderStanding::RESTRICTED,
            DomainEvent::RiderReinstated(_) => RiderStanding::ACTIVE,
            _ => prev.map(|r| r.standing).unwrap_or(RiderStanding::ACTIVE),
        }
    }

    fn decided_at(
        &self,
        prev: Option<&RiderRestrictionRow>,
        env: &Envelope,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        match &env.event {
            DomainEvent::RiderRestricted(e) => e.decided_at.parse().ok(),
            _ => prev.and_then(|r| r.decided_at),
        }
    }

    fn effective_at(
        &self,
        prev: Option<&RiderRestrictionRow>,
        env: &Envelope,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        match &env.event {
            DomainEvent::RiderRestricted(e) => e.effective_at.parse().ok(),
            _ => prev.and_then(|r| r.effective_at),
        }
    }
}
