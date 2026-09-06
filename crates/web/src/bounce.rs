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

/// [`bounce_after`], plus the `?next=` return-to-screen leg (#904, ADR-20260905-101349 §13 — the
/// member door's flip precondition): composed HERE, at the ONE call site both the hydrate loop
/// (`renderer.rs`) and the mutation dispatcher (`interact.rs:~317`) now go through, so a read
/// refusal and a write refusal can never disagree about whether — or how — a next was attached.
///
/// A `next` is attached ONLY on the unauthenticated (bare 401) leg of a `requires_auth` screen —
/// never on `RIDER_RESTRICTED` (`/restricted`) or `ADMIN_ACCESS_NOT_GRANTED` (`/sign-in/no-access`,
/// a terminal refusal, not a "come back once you sign in" door) — built from `current_path_and_query`
/// (the CALLER's `location.pathname()` + `location.search()`; this module owns composition, never
/// the stripping the caller must already have applied to `token`/`next` themselves, per
/// `router.rs`'s own doc: "query strings are the caller's to strip" — this function additionally
/// belt-and-braces drops both here too, so a call site forgetting to strip cannot leak a stale
/// `next` value into a fresh one, or forward a magic-link token into a query string).
pub fn bounce_target(
    err: &TransportError,
    screen: &Screen,
    current_path_and_query: &str,
) -> Option<String> {
    let route = bounce_after(err, screen)?;
    let is_unauthenticated = matches!(err, TransportError::Status { status: 401 });
    if !is_unauthenticated || !screen.requires_auth {
        return Some(route.to_string());
    }
    let stripped = drop_query_params(current_path_and_query, &["token", "next"]);
    if stripped.is_empty() {
        return Some(route.to_string());
    }
    Some(format!("{route}?next={}", percent_encode_next(&stripped)))
}

/// Drop the named query params from `path_and_query` (`pathname` + `search`) — never a hand-rolled
/// URL library, this crate's whole query-string surface is these few characters (`router.rs`'s doc:
/// "query strings are the caller's to strip").
fn drop_query_params(path_and_query: &str, drop: &[&str]) -> String {
    let mut parts = path_and_query.splitn(2, '?');
    let path = parts.next().unwrap_or("");
    let Some(query) = parts.next() else { return path.to_string() };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|kv| !drop.contains(&kv.split('=').next().unwrap_or("")))
        .collect();
    if kept.is_empty() { path.to_string() } else { format!("{path}?{}", kept.join("&")) }
}

/// A minimal percent-encoder for embedding an arbitrary path+query as ONE query VALUE (`?next=`):
/// everything outside the unreserved set (plus `/`, kept bare for readability) becomes `%XX`.
/// `router::safe_next`'s `percent_decode_once` is this encoder's exact inverse; decoding happens
/// exactly once, there, never here.
fn percent_encode_next(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The per-screen restricted-route rule (= `screen.restricted_route`, never invented on a screen
/// that declares none): the ONE seam that reads it, written here so it is written exactly ONCE.
/// [`bounce_after`] (below) calls it for the HTTP leg; it exists as its OWN name — not inlined into
/// `bounce_after` — because a raw WebSocket close (#894 D2, `subscriptions::handle_close`'s
/// `restricted` closure) needs the SAME rule and never carries a [`TransportError`], so it cannot
/// run through `bounce_after`/[`bounce_target`] at all, and must NEVER synthesise one just to get
/// there (that would forge a server signal the socket never actually carried). Grows nowhere else:
/// these two callers are the whole surface.
pub fn restricted_target(screen: &Screen) -> Option<&'static str> {
    screen.restricted_route
}

/// Where one refused GraphQL call bounces `screen`'s visitor, or `None` to stay put. Private
/// (#904 R2-4, compiler-first): [`bounce_target`] is the ONE public surface both call sites (the
/// hydrate loop's per-read outcome, `interact.rs`'s pre-acceptance mutation failure) use — routing
/// through this function directly, bypassing `bounce_target`'s `?next=` composition, is
/// unspellable outside this module now, not merely discouraged. See the module docs for the two
/// legs.
fn bounce_after(err: &TransportError, screen: &Screen) -> Option<&'static str> {
    match err {
        TransportError::Errors { extensions, .. } => {
            if extensions.iter().any(|e| e.reason.as_deref() == Some(shared_types::RIDER_RESTRICTED)) {
                return restricted_target(screen);
            }
            // #639 part C step 6-iii (ADR-20260906-023825): an ADMIN-claimed token with no live
            // platform grant -- the server's own re-derivation refused it (`RoleGuard`, never a
            // client-visible claim). ONE fixed target across the whole System surface (unlike
            // `RIDER_RESTRICTED`'s per-screen `restricted_route`): every requires_auth screen on
            // this surface bounces here identically, there being exactly one such refusal reason
            // for the whole surface.
            if extensions.iter().any(|e| e.reason.as_deref() == Some(shared_types::ADMIN_ACCESS_NOT_GRANTED)) {
                return Some("/sign-in/no-access");
            }
            None
        }
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

    /// #639 part C step 6-iii: an ADMIN-claimed token the seam re-derived to `Identity::Unbound`
    /// (no live platform grant).
    fn admin_not_granted() -> TransportError {
        TransportError::Errors {
            message: "forbidden: role PUBLIC is not authorized for this operation (allowed: ADMIN)".into(),
            extensions: vec![ErrorExtensions {
                code: Some("FORBIDDEN".into()),
                reason: Some(shared_types::ADMIN_ACCESS_NOT_GRANTED.into()),
            }],
        }
    }

    fn mailbox_lanes() -> &'static Screen {
        crate::generated::screens::system::SCREENS.iter().find(|s| s.id == "mailbox_lanes").expect("mailbox_lanes screen")
    }

    /// The reason bounces to a FIXED target, not a per-screen declared route -- unlike
    /// `RIDER_RESTRICTED`, there is exactly one such refusal for the whole System surface.
    #[test]
    fn admin_access_not_granted_bounces_to_no_access() {
        assert_eq!(bounce_after(&admin_not_granted(), mailbox_lanes()), Some("/sign-in/no-access"));
    }

    /// A bare role-mismatch FORBIDDEN (no reason at all -- the beck instruction) never bounces to
    /// `/sign-in/no-access` either: `forbidden_no_reason()` -> `None`, exactly as for every other
    /// screen's ordinary role refusal.
    #[test]
    fn forbidden_no_reason_never_bounces_to_no_access() {
        assert_eq!(bounce_after(&forbidden_no_reason(), mailbox_lanes()), None);
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

    // ---- #894 D2: restricted_target, the socket-close leg's per-screen rule ----

    /// Equals the screen's own declared route (the jobs pair carries `Some("/restricted")`).
    #[test]
    fn restricted_target_equals_the_screens_declared_route() {
        assert_eq!(restricted_target(jobs()), Some("/restricted"));
        assert_eq!(restricted_target(job_detail()), Some("/restricted"));
    }

    /// `None` on a screen that declares none — the sign-in door, same fixture the HTTP leg uses.
    #[test]
    fn restricted_target_is_none_on_the_sign_in_door() {
        assert_eq!(restricted_target(sign_in_door()), None);
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

    // ---- D2 (#904, ADR-20260905-101349 §13): `bounce_target`'s `?next=` composition ----

    /// Red-first (ADR-20260905-101349:171): a mutant that drops the `?next=` composition entirely
    /// (bounces without one) fails this — the route would lack `?next=` altogether.
    #[test]
    fn a_401_bounces_with_next_equal_to_the_current_path_stripped_of_token() {
        let target = bounce_target(
            &TransportError::Status { status: 401 },
            jobs(),
            "/jobs?token=stale-abc&foo=bar",
        );
        // `token` (a stale magic-link leftover) and the query order otherwise are both stripped/
        // kept as expected; the whole thing is percent-encoded as ONE query value.
        assert_eq!(target, Some("/sign-in?next=/jobs%3Ffoo%3Dbar".to_string()));
    }

    /// The bare-path case (no query at all) still gets a `next` — never conditioned on a query
    /// existing.
    #[test]
    fn a_401_on_a_bare_path_still_carries_next() {
        let target = bounce_target(&TransportError::Status { status: 401 }, jobs(), "/jobs");
        assert_eq!(target, Some("/sign-in?next=/jobs".to_string()));
    }

    /// A `next` param already on the URL (a stale bounce chain) is ALSO stripped before
    /// recomposing — never nested/doubled.
    #[test]
    fn a_stale_next_param_is_stripped_before_recomposing() {
        let target =
            bounce_target(&TransportError::Status { status: 401 }, jobs(), "/jobs?next=%2Fold");
        assert_eq!(target, Some("/sign-in?next=/jobs".to_string()));
    }

    /// `RIDER_RESTRICTED` and `ADMIN_ACCESS_NOT_GRANTED` never carry a `next` — both are terminal
    /// refusals, not a "come back once you sign in" door.
    #[test]
    fn restricted_and_admin_not_granted_bounces_never_carry_next() {
        assert_eq!(
            bounce_target(&restricted_reason(), jobs(), "/jobs"),
            Some("/restricted".to_string())
        );
        assert_eq!(
            bounce_target(&admin_not_granted(), mailbox_lanes(), "/system/mailbox"),
            Some("/sign-in/no-access".to_string())
        );
    }

    /// A screen without `requires_auth` never gets a `next` even on a bare 401 (defensive — no
    /// such screen should ever 401 in practice, but the rule is structural, not incidental).
    #[test]
    fn an_open_screen_never_carries_next() {
        assert!(!sign_in_door().requires_auth, "fixture assumption");
        // sign_in_door() declares no unauthenticated_route, so bounce_after is already None here;
        // the assertion is that `bounce_target` never invents a `next` even if it did.
        assert_eq!(bounce_target(&TransportError::Status { status: 401 }, sign_in_door(), "/sign-in"), None);
    }

    /// Network/malformed/non-401 statuses never carry a `next` either (`bounce_after` already
    /// returns `None` for them; `bounce_target` must not diverge).
    #[test]
    fn non_bouncing_errors_never_carry_next() {
        assert_eq!(bounce_target(&TransportError::Network("reset".into()), jobs(), "/jobs"), None);
        assert_eq!(bounce_target(&TransportError::Status { status: 500 }, jobs(), "/jobs"), None);
    }
}
