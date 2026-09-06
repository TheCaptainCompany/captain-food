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
            DomainEvent::RiderRestricted(e) => Some(e.decided_at.parse().unwrap_or_else(|_| {
                tracing::error!(
                    position = env.position,
                    raw = %e.decided_at,
                    "RiderRestricted.decidedAt failed to parse -- folding to occurred_at so the \
                     rider stays RESTRICTED with a timestamp rather than reading as ACTIVE"
                );
                env.occurred_at
            })),
            _ => prev.and_then(|r| r.decided_at),
        }
    }

    fn effective_at(
        &self,
        prev: Option<&RiderRestrictionRow>,
        env: &Envelope,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        match &env.event {
            DomainEvent::RiderRestricted(e) => Some(e.effective_at.parse().unwrap_or_else(|_| {
                tracing::error!(
                    position = env.position,
                    raw = %e.effective_at,
                    "RiderRestricted.effectiveAt failed to parse -- folding to occurred_at so the \
                     rider stays RESTRICTED with a timestamp rather than reading as ACTIVE"
                );
                env.occurred_at
            })),
            _ => prev.and_then(|r| r.effective_at),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::generated::events::RiderRestricted;
    use domain::generated::scalars::{RiderId, RiderRestrictionGround};

    fn restricted_envelope(rider_id: uuid::Uuid, decided_at: &str, effective_at: &str) -> Envelope {
        Envelope {
            stream_name: format!("Rider-{rider_id}"),
            position: 7,
            occurred_at: chrono::Utc::now(),
            event: DomainEvent::RiderRestricted(RiderRestricted {
                rider_id: RiderId(rider_id),
                ground: RiderRestrictionGround::IDENTITY_MISMATCH,
                decided_at: decided_at.into(),
                effective_at: effective_at.into(),
            }),
        }
    }

    /// D4 (dba, observability): a malformed `decidedAt`/`effectiveAt` on a RESTRICTED row must
    /// never fold to `None` -- that would render `restriction: null` on a rider the fold just
    /// classified RESTRICTED, worse than a wrong-but-present timestamp. It folds to
    /// `env.occurred_at` instead (deterministic on replay). The production fold ALSO emits a
    /// `tracing::error!` on this path (`position` + the raw string, classified technical) -- NOT
    /// asserted here, since `application` carries no tracing `Capture` dev-dependency to observe
    /// it; the log assertion is the linked follow-up
    /// ([#936](https://github.com/TheCaptainCompany/captain-food/issues/936)). Seen RED against the
    /// mutant `.parse().ok()` (`left: None, right: Some(...)`, i.e. `decided_at` reads `None` on a
    /// RESTRICTED row) -- recorded in the hand-back.
    #[test]
    fn a_malformed_restriction_timestamp_folds_to_occurred_at() {
        let rider_id = uuid::Uuid::new_v4();
        let env = restricted_envelope(rider_id, "not-a-timestamp", "also-not-a-timestamp");

        let decided = RiderRestrictionProjector.decided_at(None, &env);
        let effective = RiderRestrictionProjector.effective_at(None, &env);

        assert_eq!(
            decided,
            Some(env.occurred_at),
            "a malformed decidedAt must fold to occurred_at, never None"
        );
        assert_eq!(
            effective,
            Some(env.occurred_at),
            "a malformed effectiveAt must fold to occurred_at, never None"
        );
    }

    /// The well-formed path is unchanged: a valid RFC3339 string still parses to itself.
    #[test]
    fn a_well_formed_restriction_timestamp_parses_as_is() {
        let rider_id = uuid::Uuid::new_v4();
        let env = restricted_envelope(rider_id, "2026-09-06T12:00:00Z", "2026-09-06T12:00:00Z");
        let expected: chrono::DateTime<chrono::Utc> = "2026-09-06T12:00:00Z".parse().unwrap();

        assert_eq!(RiderRestrictionProjector.decided_at(None, &env), Some(expected));
        assert_eq!(RiderRestrictionProjector.effective_at(None, &env), Some(expected));
    }
}
