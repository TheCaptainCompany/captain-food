//! Hand-written `RestaurantInvitationListCompute` (ADR-0040; #639 part C step 6-iv round 2,
//! ADR-20260905-101349 §2 amendment): `expires_at` is COMPLEX by declaration (empty `from:` on the
//! table spec — the generator's occurred_at-vs-parsed-string classifier has no third case for
//! "occurred_at plus a configuration offset").
//!
//! Reads `RESTAURANT_INVITATION_TTL_SECONDS`'s SPEC DEFAULT off the generated `REMINDER_SCHEDULES`
//! table (the SAME row `InviteRestaurantMember`'s own `reminders:` schedule reads) rather than a
//! live env override — a scoped simplification (named here, not hidden): if a deployment ever
//! overrides the window at runtime, this projected `expiresAt` would disagree with the REAL
//! scheduled deadline. Acceptable for now because the door
//! (`RUN_RESTAURANT_INVITATION`) ships OFF and the TTL default itself is `UNVERIFIED input`
//! (register check) — revisit the day the value is confirmed and any override is wired.

use crate::generated::reminders::REMINDER_SCHEDULES;
use crate::generated::rows::RestaurantInvitationListRow;
use crate::projections::{Envelope, RestaurantInvitationListCompute};
use domain::generated::events::DomainEvent;

/// The spec-default TTL window for `RestaurantInvitationExpired` — see the module doc for why this
/// is the spec default, not a live env read.
fn restaurant_invitation_ttl() -> chrono::Duration {
    REMINDER_SCHEDULES
        .iter()
        .find(|s| s.actor_type == "RestaurantInvitation" && s.reminder == "RestaurantInvitationExpired")
        .map(|s| chrono::Duration::from_std(s.after_default).expect("TTL window fits chrono"))
        .expect("RestaurantInvitationExpired schedule is declared (reminder_schedule_pin.rs pins it)")
}

pub struct RestaurantInvitationListProjector;

impl RestaurantInvitationListCompute for RestaurantInvitationListProjector {
    fn expires_at(
        &self,
        prev: Option<&RestaurantInvitationListRow>,
        env: &Envelope,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        match &env.event {
            DomainEvent::RestaurantInvitationSent(_) => Some(env.occurred_at + restaurant_invitation_ttl()),
            _ => prev.and_then(|r| r.expires_at),
        }
    }
}
