//! The bounce decision (#639 part C step 4-ii, ADR-20260904-124600 §2) — ONE pure function
//! deciding where a refused GraphQL call navigates a rider: a refused READ (the hydrate loop,
//! `renderer::hydrate`) or a refused TELL (`interact.rs`'s mutation dispatch), both reduced to the
//! same typed [`crate::graphql::TransportError`] shape, so a data-read refusal and a button-click
//! refusal can never disagree about which screen a rider ends up on.
//!
//! Two legs, keyed on the SERVER's own signal — never on a bare `FORBIDDEN` (ADR-081527 §4 keeps
//! `code: FORBIDDEN` unchanged for every guard; only `StandingGuard` adds the `reason`):
//!   * `extensions.reason == RIDER_RESTRICTED` (any error in the array) → the screen's own
//!     `restricted_route` (`None` on a screen that declares none — never invented).
//!   * a bare HTTP 401 (no session at all) → the screen's `unauthenticated_route` — the 2c-ii leg,
//!     moved here from its old home inline in `renderer::hydrate`'s `spawn_local` block, now
//!     covered by [`bounce_after`]'s own tests for the first time.
//!   * anything else (a `RoleGuard` refusal with no reason, a network failure, a malformed
//!     envelope, a business rejection surfacing through `operationStatus` — never through here) →
//!     `None`: the caller keeps its own degraded-render / toast posture.

use crate::generated::screens::Screen;
use crate::graphql::TransportError;

/// Where one refused GraphQL call bounces `screen`'s visitor, or `None` to stay put. The ONLY
/// entry point either call site (the hydrate loop's per-read outcome, `interact.rs`'s pre-
/// acceptance mutation failure) may use to decide a bounce — see the module docs for the two legs.
pub fn bounce_after(err: &TransportError, screen: &Screen) -> Option<&'static str> {
    match err {
        TransportError::Errors { extensions, .. } => extensions
            .iter()
            .any(|e| e.reason.as_deref() == Some(shared_types::RIDER_RESTRICTED))
            .then_some(())
            .and_then(|()| screen.restricted_route),
        TransportError::Status { status: 401 } => screen.unauthenticated_route,
        TransportError::Status { .. } | TransportError::Network(_) | TransportError::Malformed(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::screens::rider;
    use crate::graphql::ErrorExtensions;

    fn restricted_reason() -> TransportError {
        TransportError::Errors {
            message: "forbidden: your access is restricted".into(),
            extensions: vec![ErrorExtensions {
                code: Some("FORBIDDEN".into()),
                reason: Some(shared_types::RIDER_RESTRICTED.into()),
            }],
        }
    }

    fn forbidden_no_reason() -> TransportError {
        TransportError::Errors {
            message: "forbidden: role RIDER is not authorized".into(),
            extensions: vec![ErrorExtensions { code: Some("FORBIDDEN".into()), reason: None }],
        }
    }

    fn jobs() -> &'static Screen {
        rider::SCREENS.iter().find(|s| s.id == "jobs").expect("jobs screen")
    }

    fn job_detail() -> &'static Screen {
        rider::SCREENS.iter().find(|s| s.id == "job_detail").expect("job_detail screen")
    }

    fn sign_in_door() -> &'static Screen {
        rider::SCREENS.iter().find(|s| s.id == "sign_in").expect("sign_in screen")
    }

    /// A restricted reason on a screen that declares `restricted:` bounces there.
    #[test]
    fn restricted_reason_on_jobs_bounces_to_restricted() {
        assert_eq!(bounce_after(&restricted_reason(), jobs()), Some("/restricted"));
    }

    /// A `FORBIDDEN` with NO reason (a role refusal) never bounces — never on `code` alone (M1).
    #[test]
    fn forbidden_with_no_reason_on_job_detail_does_not_bounce() {
        assert_eq!(bounce_after(&forbidden_no_reason(), job_detail()), None);
    }

    /// A screen that declares no `restricted:` route never bounces there even on the reason —
    /// the sign-in door is `requires_auth: false` and carries none.
    #[test]
    fn a_screen_without_a_restricted_route_never_bounces() {
        assert_eq!(sign_in_door().restricted_route, None, "fixture assumption");
        assert_eq!(bounce_after(&restricted_reason(), sign_in_door()), None);
    }

    /// A bare 401 (no session) bounces to the screen's `unauthenticated_route` — the 2c-ii leg,
    /// now unified here and seen red for the first time through this function's own test.
    #[test]
    fn a_401_bounces_to_the_unauthenticated_route() {
        assert_eq!(
            bounce_after(&TransportError::Status { status: 401 }, jobs()),
            Some("/sign-in")
        );
    }

    /// A network failure or a malformed envelope never bounces — the caller's own retry/error
    /// posture applies.
    #[test]
    fn network_and_malformed_never_bounce() {
        assert_eq!(bounce_after(&TransportError::Network("reset".into()), jobs()), None);
        assert_eq!(bounce_after(&TransportError::Malformed("no data".into()), jobs()), None);
    }

    /// A non-401 status (e.g. 500) never bounces either — only the specific "no session" signal
    /// does.
    #[test]
    fn a_non_401_status_never_bounces() {
        assert_eq!(bounce_after(&TransportError::Status { status: 500 }, jobs()), None);
    }
}
