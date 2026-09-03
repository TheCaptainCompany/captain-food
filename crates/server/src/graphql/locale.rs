//! The request's LOCALE for human-readable GraphQL text (#639 part C step 2c-ii).
//!
//! `Operation.message` — the sentence a client shows when a journaled command REJECTED — used to be
//! built with `message_en` on every leg, so a French rider refused with `RiderNotRegistered` read
//! an English toast. The CODE is the contract (`errorCode`, P-10); the message is presentation and
//! is derived at read time from the mailbox row's `{ code, context }`, so localizing it costs no
//! stored shape and no migration: the transport resolves the locale ONCE per request (the SSR
//! chain, `web::i18n::resolve_locale`: cookie → `Accept-Language` → the platform default) and
//! injects it beside the session and tenant; the generated resolvers read it back through
//! `request_locale(ctx)`. A context with none (a direct schema execution) keeps the pre-locale
//! contract — English — so no existing assertion moves.
//!
//! Two locales, because the catalogue has two (`messages.en` / `messages.fr`); a third arrives
//! through `web::i18n::SUPPORTED_LOCALES` and the generated `message_<tag>` accessor together, and
//! `from_tag` is the ONE place that maps a tag onto an accessor.

use axum::http::{header, HeaderMap};

/// The caller's locale, resolved at the transport boundary. `Copy` so the subscription stream can
/// hold it across `yield`s without a clone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequestLocale {
    /// The pre-locale contract and the fallback for an unknown tag.
    #[default]
    En,
    Fr,
}

impl RequestLocale {
    /// A supported BCP-47 primary tag (`fr`, `fr-FR`, `en`) → the catalogue accessor it selects.
    pub fn from_tag(tag: &str) -> Self {
        match web::i18n::normalize_locale(tag) {
            Some("fr") => RequestLocale::Fr,
            _ => RequestLocale::En,
        }
    }

    /// The SSR resolution chain over a GraphQL request's headers: the `captain_locale` cookie,
    /// then `Accept-Language`, then the platform default (`web::i18n::DEFAULT_LOCALE`).
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let cookie_locale = surface_runtime::hosts::cookie_value(headers, web::i18n::LOCALE_COOKIE);
        let accept_language = headers.get(header::ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok());
        Self::from_tag(web::i18n::resolve_locale(None, cookie_locale.as_deref(), accept_language))
    }

    /// The tag this locale answers in.
    pub fn tag(self) -> &'static str {
        match self {
            RequestLocale::En => "en",
            RequestLocale::Fr => "fr",
        }
    }

    /// The catalogued, context-interpolated message for `code` in this locale (`None` for an
    /// uncatalogued code — the caller falls back to the code itself).
    pub fn message(self, code: &str, context: &serde_json::Value) -> Option<String> {
        match self {
            RequestLocale::En => domain::generated::errors::message_en(code, context),
            RequestLocale::Fr => domain::generated::errors::message_fr(code, context),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chain_is_cookie_then_accept_language_then_the_platform_default() {
        let mut h = HeaderMap::new();
        assert_eq!(RequestLocale::from_headers(&h), RequestLocale::from_tag(web::i18n::DEFAULT_LOCALE));
        h.insert(header::ACCEPT_LANGUAGE, "en-GB,en;q=0.9".parse().unwrap());
        assert_eq!(RequestLocale::from_headers(&h), RequestLocale::En);
        h.insert(header::COOKIE, "captain_locale=fr".parse().unwrap());
        assert_eq!(RequestLocale::from_headers(&h), RequestLocale::Fr, "the cookie outranks the header");
        assert_eq!(RequestLocale::from_tag("de"), RequestLocale::En, "unsupported -> the fallback");
    }

    /// The whole reason the module exists: the rider refusal reads in French, with the support
    /// contact interpolated from the row's typed context — never spelled by a screen.
    #[test]
    fn a_rider_refusal_is_the_french_catalogue_sentence_naming_the_support_contact() {
        let context = serde_json::json!({ "supportContact": "support@captain.food" });
        let fr = RequestLocale::Fr.message("RiderNotRegistered", &context).expect("catalogued");
        assert!(fr.contains("support@captain.food"), "{fr}");
        assert!(fr.contains("compte livreur"), "{fr}");
        let en = RequestLocale::En.message("RiderNotRegistered", &context).expect("catalogued");
        assert!(en.contains("rider account"), "{en}");
        assert_eq!(RequestLocale::Fr.message("NoSuchCode", &context), None);
    }
}
