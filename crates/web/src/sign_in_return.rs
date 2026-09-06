//! The member magic-link RETURN landing (#639 part C step 6-ii round 2, R2-D/R2-R2) —
//! `restaurant_backoffice.yaml#/screens/sign_in_return`, deliberately NOT SDUI (`sdui: false`):
//! extracting `?token=` from the URL the mail client opens and sequencing
//! confirm → claim → route is client logic this DSL declares no query-string-to-variable
//! grammar for (`crates/web/src/router.rs`: "query strings are the caller's to strip"), and
//! `confirmMemberSignIn`'s acceptance-first outcome needs the dispatch+poll sequencing
//! (`crates/web/src/actions.rs`) no SDUI binding expresses either — both reasons `checkout.rs`/
//! `tracking.rs` are hand-written too (`crates/web/src/handwritten.rs`).
//!
//! SSR renders a STATIC "signing you in…" shell (there is nothing to read server-side: the token
//! lives in the query string, which never reaches `RouteMatch`); the real work — reading the
//! token, dispatching, polling, claiming the parked session, and leaving the page — happens in
//! the browser (`handwritten.rs`'s `mount::mount_sign_in_return`, the SAME split
//! `checkout.rs`/`tracking.rs` use between their SSR shell and their `mount` fn).
//!
//! One deliberate simplification against the `checkout`/`auth::verify_otp` precedent: this page
//! does NOT persist its dispatch across a reload (`pending.rs`'s `dispatch_persisted`) — a magic
//! link is opened exactly once in the common case, and a reload re-submitting the same
//! (single-use) token under a FRESH `messageId` is a rare, honestly-failing edge (the identity
//! provider's own token already answers `InvalidVerificationToken` on a second use), not the
//! cart-continuity case `pending.rs` exists for.

use leptos::prelude::*;

/// What this page shows. SSR always renders [`Working`](SignInReturnState::Working); the browser
/// moves it to a terminal state only when there is something to SAY here — a successful
/// confirmation or the `MemberNotLinked` refusal both leave the page immediately
/// (`navigate_away`) rather than rendering a state of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignInReturnState {
    Working,
    /// The URL carried no `?token=` at all — opened by hand, or a mangled link.
    NoToken,
    /// Any other rejection/failure (`InvalidVerificationToken`, `VerificationCodeExpired`,
    /// `MemberSignInDoorClosed`, `MemberSignInRequiresSession`, `AuthSubjectHoldsAnotherRole`, a
    /// technical `Failed`, or the poll giving up) — one honest "this link doesn't work" state,
    /// never a per-code message that would need its own translated copy this round did not scope.
    Failed,
}

/// The rendered shell — the SAME tree for SSR and hydrate (`checkout`/`tracking`'s convention).
#[component]
pub fn SignInReturnScreen(state: SignInReturnState, locale: String) -> impl IntoView {
    let key = match state {
        SignInReturnState::Working => "back.sign_in_return.working",
        SignInReturnState::NoToken => "back.sign_in_return.no_token",
        SignInReturnState::Failed => "back.sign_in_return.failed",
    };
    let message = crate::i18n::resolve(key, &locale);
    let show_back_link = !matches!(state, SignInReturnState::Working);
    let back_label = crate::i18n::resolve("back.sign_in_return.back_to_sign_in", &locale);
    view! {
        <main id="app" data-hydrate="sign_in_return" class="sign-in-return" data-c="sign_in_return">
            <p data-i18n=key>{message}</p>
            {show_back_link.then(|| view! { <a href="/sign-in">{back_label}</a> })}
        </main>
    }
}

/// The SSR shell. `#[cfg(feature = "ssr")]` mirrors `checkout::render_checkout_html`/
/// `tracking::render_tracking_html` exactly (`handwritten.rs`'s `render_html` calls it).
#[cfg(feature = "ssr")]
pub fn render_sign_in_return_html(lang: &str) -> String {
    let lang = crate::i18n::normalize_locale(lang).unwrap_or(crate::i18n::DEFAULT_LOCALE);
    let body = SignInReturnScreen(SignInReturnScreenProps {
        state: SignInReturnState::Working,
        locale: lang.to_string(),
    })
    .to_html();
    crate::renderer::page_html("Signing you in - Captain.Food", lang, &body)
}

/// Read `token` out of `window.location().search()` — this DSL's missing query-string grammar,
/// built here rather than as a general binding (ADR-20260817-105845: no invented grammar to get
/// one page unstuck). `None` for a missing key, an empty value, or no `window` at all (never
/// panics: a stranger can open this URL with anything in it).
#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
pub fn token_from_location() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    parse_token(&search)
}

/// The parse, split out so it is testable off-wasm (the `checkout`/`tracking` convention: sans-IO
/// logic stays plain Rust, only the `window` read needs `wasm32`). `cfg`'d against the one caller
/// above plus the tests below — an `ssr`-only build (the native `server` binary) never calls
/// either, and a function neither target reaches is dead code, not a defensive extra.
#[cfg(any(test, all(target_arch = "wasm32", feature = "hydrate")))]
fn parse_token(search: &str) -> Option<String> {
    let search = search.strip_prefix('?').unwrap_or(search);
    for pair in search.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next()?;
        if key != "token" {
            continue;
        }
        let raw = parts.next().unwrap_or("");
        if raw.is_empty() {
            return None;
        }
        // `application/x-www-form-urlencoded` space encoding, then percent-decoding — `+` is
        // never meaningful inside an opaque token, so this is safe even if over-eager. Decoded
        // in plain Rust (no `js_sys`/browser API) so the SAME behaviour is exercised by the
        // native `cargo test` run as by the real browser — a `#[cfg(wasm32)]` decoder would be
        // untested by every gate that matters here.
        return Some(percent_decode(&raw.replace('+', " ")));
    }
    None
}

/// A minimal percent-decoder (`%XX` → byte), lossy on malformed input (an invalid escape or a
/// non-UTF-8 result is passed through/replaced rather than failing the whole page over a
/// mangled link — this is a stranger-controlled URL).
#[cfg(any(test, all(target_arch = "wasm32", feature = "hydrate")))]
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The pure decision behind [`return_target`], split out so it is testable off-wasm (the
/// `parse_token`/`percent_decode` convention this file already uses): given the RAW captured
/// value (if any) and the CURRENT host, decide the destination through `router::safe_next` — the
/// ONE validation point. `None`/an invalid/unresolvable candidate all fall back to `/` — **NEVER**
/// to `/sign-in`: this landing runs AFTER the sign-in confirm already succeeded, so falling back to
/// sign-in would be a loop, not a safe default.
#[cfg(any(test, all(target_arch = "wasm32", feature = "hydrate")))]
fn resolve_return_target(host: &str, raw_next: Option<&str>) -> &'static str {
    raw_next.and_then(|raw| crate::router::safe_next(host, raw)).unwrap_or("/")
}

/// The validated return-to-screen target after a SUCCESSFUL sign-in (#904 D3, ADR-20260905-101349
/// §13): consumes the ONE captured `?next=` value (`next_param::take_next` — removed on read, so a
/// reload of THIS page never replays a stale target) and validates it through [`resolve_return_target`]
/// at THIS moment, the ONE consumption point — never earlier (storage never validates), never twice.
#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
pub fn return_target() -> &'static str {
    let Some(host) = web_sys::window().and_then(|w| w.location().host().ok()) else { return "/" };
    resolve_return_target(&host, crate::next_param::take_next().as_deref())
}

/// Leave the page with a full browser navigation (never an SPA route, since this page mounts no
/// router): the confirm/claim work is finished, and the destination is a fresh document either
/// way (the orders queue needs a signed-in read; the not-linked screen is `graphql_role: PUBLIC`
/// but still a distinct route).
#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
pub fn navigate_away(origin: &str, path: &str) {
    if let Some(window) = web_sys::window() {
        let _ = window
            .location()
            .set_href(&format!("{}{}", origin.trim_end_matches('/'), path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_token_query_param() {
        assert_eq!(parse_token("?token=abc123"), Some("abc123".to_string()));
        assert_eq!(parse_token("token=abc123"), Some("abc123".to_string()));
    }

    #[test]
    fn finds_token_among_other_params() {
        assert_eq!(parse_token("?utm_source=x&token=abc123&foo=bar"), Some("abc123".to_string()));
    }

    #[test]
    fn missing_or_empty_token_is_none() {
        assert_eq!(parse_token(""), None);
        assert_eq!(parse_token("?"), None);
        assert_eq!(parse_token("?foo=bar"), None);
        assert_eq!(parse_token("?token="), None);
    }

    #[test]
    fn percent_and_plus_decoding() {
        // A `+` decodes to a space (form encoding); `%40` decodes to `@` — neither should ever
        // appear in a real token, but a stranger can put anything in a URL.
        assert_eq!(parse_token("?token=a+b"), Some("a b".to_string()));
        assert_eq!(parse_token("?token=a%40b"), Some("a@b".to_string()));
    }

    // ---- D3 (#904, ADR-20260905-101349 §13): the return-to-screen consumption ----

    /// Red-first (ADR-20260905-101349:171): a mutant that "accepts it" — returns the raw captured
    /// value verbatim instead of routing it through `router::safe_next` — would send a visitor
    /// straight back to `/sign-in` for the `Some("/sign-in")` case (a loop: this landing runs
    /// AFTER sign-in already succeeded) instead of falling back to `/`.
    #[test]
    fn next_absent_or_invalid_navigates_to_root() {
        let host = "riders.captain.food";
        assert_eq!(resolve_return_target(host, None), "/", "nothing captured -> root");
        assert_eq!(
            resolve_return_target(host, Some("/sign-in")),
            "/",
            "the sign-in door itself must never be a return target (would loop)"
        );
        assert_eq!(resolve_return_target(host, Some("//evil.com")), "/", "an open redirect must never pass");
        assert_eq!(resolve_return_target(host, Some("/route/does/not/exist")), "/");
    }

    /// A valid captured `next` (a `requires_auth` screen of the SAME surface) is honored — asserted
    /// against a NON-fallback destination (`/deliveries`, a real `requires_auth` screen of the
    /// restaurant-backoffice surface, decoded once from `/%64eliveries`) so a mutant that makes
    /// `resolve_return_target` always return `"/"` cannot pass this test byte-identically to the
    /// fallback case above (ADR-20260906-024838 rule 1 / R2-1).
    #[test]
    fn a_valid_captured_next_is_honored() {
        assert_eq!(resolve_return_target("restos.captain.food", Some("/%64eliveries")), "/deliveries");
    }
}
