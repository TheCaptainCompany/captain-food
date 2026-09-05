//! The invitation acceptance landing (#639 part C step 6-iv round 2, ADR-20260905-101349 §2
//! amendment) — `restaurant_backoffice.yaml#/screens/invitation_accept`, deliberately NOT SDUI
//! (`sdui: false`): the SAME two reasons `sign_in_return.rs` is hand-written (query-string
//! extraction this DSL declares no grammar for; acceptance-first dispatch+poll sequencing no SDUI
//! binding expresses) apply here TWICE over, because this page sequences TWO commands, client-side
//! (ADR-20260905-101349 §2: never a process manager) — `acceptRestaurantInvitation` first, then
//! `grantRestaurantAccessByInvitation`.
//!
//! SSR renders a STATIC "confirming your invitation…" shell (the token/invitationId live in the
//! query string, which never reaches `RouteMatch`); the real work happens in the browser
//! (`handwritten.rs`'s `mount::mount_invitation_accept`, the `mount_sign_in_return` shape).
//!
//! Leg 2 RETRIES on transient failure a bounded number of times (business: never show "link no
//! longer valid" to someone who already accepted) — the invitation's own `invitationId` is
//! folded into the grant leg's DERIVED `membershipId`, so a retried leg 2 for an
//! already-succeeded grant is the ordinary idempotent-replay path, never a duplicate.

use leptos::prelude::*;

/// What this page shows. SSR always renders [`Working`](InvitationAcceptState::Working); the
/// browser moves it to a terminal state only when there is something to SAY (a successful grant
/// leaves the page via `navigate_away` rather than rendering a state of its own).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvitationAcceptState {
    Working,
    /// The URL carried no `?token=`/`?invitationId=` at all — opened by hand, or a mangled link.
    NoToken,
    /// Leg 1 (`acceptRestaurantInvitation`) itself refused, or leg 2 refused with a business
    /// rejection that is not "transient" (see the module doc) — the SAME typed
    /// `RestaurantInvitationNotAcceptable` covers unknown/wrong-email/already-accepted-by-someone-
    /// else/revoked/expired, by the server's own no-enumeration design (`errors.yaml`), so this ONE
    /// state is correct for every one of those causes — never a per-cause message.
    Failed,
    /// Leg 1 SUCCEEDED (the invitation WAS validly accepted) but leg 2 has not yet succeeded after
    /// its retries — NEVER worded as "invalid": the person did accept, and access is still being
    /// set up. Distinct from [`Failed`] on purpose (business: never show "link no longer valid" to
    /// someone who already accepted).
    AccessPending,
}

/// The rendered shell — the SAME tree for SSR and hydrate (`checkout`/`tracking`/`sign_in_return`'s
/// convention).
#[component]
pub fn InvitationAcceptScreen(state: InvitationAcceptState, locale: String) -> impl IntoView {
    let key = match state {
        InvitationAcceptState::Working => "back.invitation.working",
        InvitationAcceptState::NoToken => "back.invitation.no_token",
        InvitationAcceptState::Failed => "back.invitation.failed",
        InvitationAcceptState::AccessPending => "back.invitation.access_pending",
    };
    let message = crate::i18n::resolve(key, &locale);
    let show_back_link = !matches!(state, InvitationAcceptState::Working);
    let back_label = crate::i18n::resolve("back.invitation.back_to_sign_in", &locale);
    view! {
        <main id="app" data-hydrate="invitation_accept" class="invitation-accept" data-c="invitation_accept">
            <p data-i18n=key>{message}</p>
            {show_back_link.then(|| view! { <a href="/sign-in">{back_label}</a> })}
        </main>
    }
}

/// The SSR shell. `#[cfg(feature = "ssr")]` mirrors `sign_in_return::render_sign_in_return_html`.
#[cfg(feature = "ssr")]
pub fn render_invitation_accept_html(lang: &str) -> String {
    let lang = crate::i18n::normalize_locale(lang).unwrap_or(crate::i18n::DEFAULT_LOCALE);
    let body = InvitationAcceptScreen(InvitationAcceptScreenProps {
        state: InvitationAcceptState::Working,
        locale: lang.to_string(),
    })
    .to_html();
    crate::renderer::page_html("Joining your team - Captain.Food", lang, &body)
}

/// Read `key` out of a query string — the `sign_in_return::parse_token` shape, generalised to a
/// named key because this page needs TWO (`token`, `invitationId`). Re-implemented rather than
/// shared: `sign_in_return`'s parser is `pub(crate)`-scoped to its own module's test/hydrate cfg
/// gates, the same reason it was not shared with `checkout`/`tracking` either.
#[cfg(any(test, all(target_arch = "wasm32", feature = "hydrate")))]
fn parse_param(search: &str, key: &str) -> Option<String> {
    let search = search.strip_prefix('?').unwrap_or(search);
    for pair in search.split('&') {
        let mut parts = pair.splitn(2, '=');
        let k = parts.next()?;
        if k != key {
            continue;
        }
        let raw = parts.next().unwrap_or("");
        if raw.is_empty() {
            return None;
        }
        return Some(percent_decode(&raw.replace('+', " ")));
    }
    None
}

/// A minimal percent-decoder (`%XX` → byte), lossy on malformed input — the
/// `sign_in_return::percent_decode` shape, duplicated for the same module-scoping reason above.
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

/// Read `(token, invitationId)` out of `window.location().search()`. `None` for either missing
/// key, an empty value, or no `window` at all (never panics: a stranger can open this URL with
/// anything in it).
#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
pub fn params_from_location() -> Option<(String, String)> {
    let search = web_sys::window()?.location().search().ok()?;
    let token = parse_param(&search, "token")?;
    let invitation_id = parse_param(&search, "invitationId")?;
    Some((token, invitation_id))
}

/// Leave the page with a full browser navigation — the `sign_in_return::navigate_away` shape.
#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
pub fn navigate_away(origin: &str, path: &str) {
    if let Some(window) = web_sys::window() {
        let _ = window
            .location()
            .set_href(&format!("{}{}", origin.trim_end_matches('/'), path));
    }
}

/// How many times leg 2 (`grantRestaurantAccessByInvitation`) retries a non-terminal failure
/// before this page gives up and shows [`InvitationAcceptState::AccessPending`] — `UNVERIFIED
/// input` (register check: no controlling record names a retry count for this leg), chosen small
/// because a genuinely transient failure (a worker hiccup) resolves in well under this window and
/// a real block (e.g. `MemberAuthSubjectAlreadyBound`) will not heal by retrying regardless.
pub const GRANT_LEG_MAX_ATTEMPTS: u32 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_both_params() {
        assert_eq!(
            (parse_param("?token=abc&invitationId=def", "token"), parse_param("?token=abc&invitationId=def", "invitationId")),
            (Some("abc".to_string()), Some("def".to_string()))
        );
    }

    #[test]
    fn missing_either_param_is_none() {
        assert_eq!(parse_param("?token=abc", "invitationId"), None);
        assert_eq!(parse_param("?invitationId=def", "token"), None);
        assert_eq!(parse_param("", "token"), None);
    }

    #[test]
    fn percent_and_plus_decoding() {
        assert_eq!(parse_param("?token=a+b", "token"), Some("a b".to_string()));
        assert_eq!(parse_param("?token=a%40b", "token"), Some("a@b".to_string()));
    }
}
