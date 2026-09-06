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
}
