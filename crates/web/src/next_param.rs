//! `?next=` capture across the sign-in email hop (#904 D3, ADR-20260905-101349 §13).
//!
//! `next` stays CLIENT-SIDE, never inside the magic-link URL itself: ux's Q2 answer is that the
//! mailed link is an attacker-influenced path through the identity provider's own logs, and every
//! distinct redirect URL would need its own Supabase allowlist entry (founder-gated) — a per-screen
//! `next` would mean wildcards there, which is an open redirect delegated to a third party. So the
//! value rides `sessionStorage` (same-device only — the copy already promises that,
//! `back.sign_in.open_on_device`/`sys.sign_in.open_on_device`) across the request→confirm hop.
//!
//! This module only captures and hands back the RAW (still percent-encoded) value; it never
//! decides whether a `next` target is SAFE — that is `router::safe_next`'s job alone, run once at
//! CONSUMPTION (`sign_in_return.rs`/`admin_sign_in_return.rs`), never here and never twice.

/// The sessionStorage key the captured value lives under.
pub const NEXT_STORAGE_KEY: &str = "captain.next";

/// Extract the RAW `next` query value out of a `location.search()` string (the
/// `sign_in_return::parse_token` convention — sans-IO, testable off-wasm). `None` for a missing
/// key or an empty value; the value is returned VERBATIM (still percent-encoded) — decoding
/// happens exactly once, at consumption, inside `router::safe_next`.
pub fn extract_next(search: &str) -> Option<String> {
    let search = search.strip_prefix('?').unwrap_or(search);
    for pair in search.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next()?;
        if key != "next" {
            continue;
        }
        let raw = parts.next().unwrap_or("");
        return if raw.is_empty() { None } else { Some(raw.to_string()) };
    }
    None
}

/// Store the current location's `next=` value once, if it carries one. Called on EVERY screen load
/// (`renderer::hydrate`, before any other branch): `next` only ever appears on a sign-in door's URL
/// in practice (the only thing that ever composes one is `bounce::bounce_target`), so gating this
/// on "is the matched screen a sign-in door" would need an id allowlist this DSL declares no
/// grammar for (ADR-20260817-105845 — no invented grammar to get one page unstuck); capturing
/// unconditionally on any load that happens to carry the param reaches the identical outcome
/// without one. A load with none is a silent no-op — never overwrites a pending value with nothing.
#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
pub fn store_next_once(location: &web_sys::Location) {
    let Ok(search) = location.search() else { return };
    let Some(next) = extract_next(&search) else { return };
    let Some(window) = web_sys::window() else { return };
    let Ok(Some(storage)) = window.session_storage() else { return };
    let _ = storage.set_item(NEXT_STORAGE_KEY, &next);
}

/// Consume the stored value ONCE — removed on read, so a later load without a fresh `?next=`
/// never replays a stale target. `None` when nothing was captured this browser session (storage
/// disabled, private mode quota, or genuinely nothing stored).
#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
pub fn take_next() -> Option<String> {
    let window = web_sys::window()?;
    let storage = window.session_storage().ok().flatten()?;
    let value = storage.get_item(NEXT_STORAGE_KEY).ok().flatten()?;
    let _ = storage.remove_item(NEXT_STORAGE_KEY);
    Some(value)
}

/// The pure decision behind `interact.rs`'s "rider door" leg (#904 R2-2, ADR-20260905-101349 §13):
/// given the DECLARED `route` an `on_success` chain is about to navigate to, the CURRENT `host`
/// and `search`, decide whether a `?next=` still sitting in the URL should override it. Split out
/// of `interact.rs` (which is `#![cfg(all(target_arch = "wasm32", feature = "hydrate"))]` for the
/// WHOLE file and therefore never compiled by a native `cargo test -p web` run) so this decision
/// is testable off-wasm — the SAME reasoning that already put `resolve_return_target` in
/// `sign_in_return.rs` and `safe_next` in `router.rs`.
///
/// Only the generic "go home" route (`"/"`) is ever eligible for an override — any OTHER declared
/// route is an explicit destination the screen author wrote, and is never second-guessed
/// (`None`, meaning: the caller keeps `route` as-is). `None` is also the answer when the query
/// carries no `next=`, or one that `router::safe_next` rejects (foreign host, `//`, a scheme, an
/// unmatched path, the sign-in door itself) — the caller then falls back to `route` itself.
pub fn same_tab_next_override(host: &str, route: &str, search: &str) -> Option<String> {
    if route != "/" {
        return None;
    }
    let raw = extract_next(search)?;
    crate::router::safe_next(host, &raw).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_next_query_param() {
        assert_eq!(extract_next("?next=%2Forders"), Some("%2Forders".to_string()));
        assert_eq!(extract_next("next=%2Forders"), Some("%2Forders".to_string()));
    }

    #[test]
    fn finds_next_among_other_params() {
        assert_eq!(
            extract_next("?utm_source=x&next=%2Forders&foo=bar"),
            Some("%2Forders".to_string())
        );
    }

    #[test]
    fn missing_or_empty_next_is_none() {
        assert_eq!(extract_next(""), None);
        assert_eq!(extract_next("?"), None);
        assert_eq!(extract_next("?foo=bar"), None);
        assert_eq!(extract_next("?next="), None);
    }

    /// Never confused with the OTHER query param this same URL family carries (#904 vs the return
    /// leg's `token`) — a mutant reading `token` instead of `next` fails this immediately.
    #[test]
    fn never_reads_the_token_param_as_next() {
        assert_eq!(extract_next("?token=abc123"), None);
        assert_eq!(extract_next("?token=abc123&next=%2Forders"), Some("%2Forders".to_string()));
    }

    // ---- #904 R2-2 (ADR-20260905-101349 §13): the rider-door same-tab `next` decision ----

    /// Red-first (ADR-20260905-101349:171): a mutant that skips `router::safe_next` and accepts
    /// ANY captured `next` verbatim would let `//evil.com` through as an override, and a mutant
    /// that always answers `Some("/")` regardless of the query would leave the valid-`next` case
    /// unhonoured (left `"/"` right `"/deliveries"`).
    #[test]
    fn same_tab_next_decision_honours_a_valid_next_and_rejects_a_foreign_one() {
        let host = "restos.captain.food";
        assert_eq!(
            same_tab_next_override(host, "/", "?next=/%64eliveries"),
            Some("/deliveries".to_string()),
            "a valid next on the generic home route is honoured"
        );
        assert_eq!(
            same_tab_next_override(host, "/", "?next=//evil.com"),
            None,
            "an open redirect must never override the generic home route"
        );
    }

    /// Red-first (ADR-20260905-101349:171): a mutant that drops the `route != "/"` guard and
    /// considers a pending `next` for EVERY declared route would repoint an explicit destination
    /// the screen author wrote (`"/orders"` becoming `"/deliveries"`) — this must never happen.
    #[test]
    fn a_declared_route_other_than_root_is_never_repointed() {
        let host = "restos.captain.food";
        assert_eq!(
            same_tab_next_override(host, "/orders", "?next=/%64eliveries"),
            None,
            "an explicit declared destination is never second-guessed by a pending next"
        );
    }
}
