//! The ADMIN magic-link RETURN landing (#639 part C step 6-iii, ADR-20260906-023825) --
//! `system.yaml#/screens/admin_sign_in_return`, deliberately NOT SDUI (`sdui: false`): the
//! `crates/web/src/sign_in_return.rs` System TWIN (never the SAME hand-written page reused --
//! the confirmed action, the error/refusal routes and the translation keys all differ). Extracting
//! `?token=` from the URL the mail client opens and sequencing confirm -> claim -> route is client
//! logic this DSL declares no query-string-to-variable grammar for, and `confirmAdminSignIn`'s
//! acceptance-first outcome needs the dispatch+poll sequencing no SDUI binding expresses either --
//! both reasons `sign_in_return.rs`/`checkout.rs`/`tracking.rs` are hand-written too
//! (`crates/web/src/handwritten.rs`).
//!
//! SSR renders a STATIC "signing you in…" shell (there is nothing to read server-side: the token
//! lives in the query string, which never reaches `RouteMatch`); the real work — reading the
//! token, dispatching, polling, claiming the parked session, and leaving the page — happens in
//! the browser (`handwritten.rs`'s `mount::mount_admin_sign_in_return`).

use leptos::prelude::*;

/// What this page shows. SSR always renders [`Working`](AdminSignInReturnState::Working); the
/// browser moves it to a terminal state only when there is something to SAY here — a successful
/// confirmation or the `AdminAccessNotGranted` refusal both leave the page immediately
/// (`navigate_away`) rather than rendering a state of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminSignInReturnState {
    Working,
    /// The URL carried no `?token=` at all — opened by hand, or a mangled link.
    NoToken,
    /// Any other rejection/failure (`InvalidVerificationToken`, `VerificationCodeExpired`,
    /// `AdminSignInDoorClosed`, `AdminSignInRequiresSession`, `AuthSubjectHoldsAnotherRole`, a
    /// technical `Failed`, or the poll giving up) — one honest "this link doesn't work" state,
    /// never a per-code message that would need its own translated copy this round did not scope.
    Failed,
}

/// The rendered shell — the SAME tree for SSR and hydrate (`sign_in_return`'s convention).
#[component]
pub fn AdminSignInReturnScreen(state: AdminSignInReturnState, locale: String) -> impl IntoView {
    let key = match state {
        AdminSignInReturnState::Working => "sys.sign_in_return.working",
        AdminSignInReturnState::NoToken => "sys.sign_in_return.no_token",
        AdminSignInReturnState::Failed => "sys.sign_in_return.failed",
    };
    let message = crate::i18n::resolve(key, &locale);
    let show_back_link = !matches!(state, AdminSignInReturnState::Working);
    let back_label = crate::i18n::resolve("sys.sign_in_return.back_to_sign_in", &locale);
    view! {
        <main id="app" data-hydrate="admin_sign_in_return" class="sign-in-return" data-c="sign_in_return">
            <p data-i18n=key>{message}</p>
            {show_back_link.then(|| view! { <a href="/sign-in">{back_label}</a> })}
        </main>
    }
}

/// The SSR shell. `#[cfg(feature = "ssr")]` mirrors `sign_in_return::render_sign_in_return_html`
/// exactly (`handwritten.rs`'s `render_html` calls it).
#[cfg(feature = "ssr")]
pub fn render_admin_sign_in_return_html(lang: &str) -> String {
    let lang = crate::i18n::normalize_locale(lang).unwrap_or(crate::i18n::DEFAULT_LOCALE);
    let body = AdminSignInReturnScreen(AdminSignInReturnScreenProps {
        state: AdminSignInReturnState::Working,
        locale: lang.to_string(),
    })
    .to_html();
    crate::renderer::page_html("Signing you in - Captain.Food", lang, &body)
}

/// Read `token` out of `window.location().search()` — the SAME missing query-string grammar
/// `sign_in_return.rs` reads, built there rather than as a general binding.
#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
pub fn token_from_location() -> Option<String> {
    crate::sign_in_return::token_from_location()
}

/// Leave the page with a full browser navigation — the `sign_in_return.rs` shape.
#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
pub fn navigate_away(origin: &str, path: &str) {
    if let Some(window) = web_sys::window() {
        let _ = window
            .location()
            .set_href(&format!("{}{}", origin.trim_end_matches('/'), path));
    }
}
