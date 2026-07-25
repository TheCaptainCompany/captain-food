//! i18n resolution over the GENERATED translation catalog (split 4/4 of #21).
//!
//! `specs/generated/translations.generated.json` (the codegen merge of `translations.yaml` + every
//! `screens/*.translations.yaml`, ADR-0033/ADR-20260722-101500) is embedded at compile time —
//! `check-drift` keeps the embedded copy in step with the DSL, so a renamed key cannot silently
//! survive. Screens carry `PropValue::I18n(key)`; this module turns keys into strings.
//!
//! Locale policy (V0 Tours): default **fr**, fallback **en** (every catalog entry carries both by
//! validator contract, so the fallback chain is total). A missing KEY renders the key itself in
//! brackets — visible in review, never a silent blank (the fail-visible rule).

use std::collections::HashMap;
use std::sync::OnceLock;

/// The embedded catalog (kept in sync by `make generate` + the drift gate).
const CATALOG_JSON: &str = include_str!("../../../specs/generated/translations.generated.json");

/// V0 default locale (Tours) and its fallback.
pub const DEFAULT_LOCALE: &str = "fr";
pub const FALLBACK_LOCALE: &str = "en";

/// The supported UI locales (bare tags), most-preferred first. Mirrors the codegen's SUPPORTED_LOCALES
/// (the validator that guarantees every catalog key carries all of them) — the catalog is keyed by
/// these bare tags, while `Customer.locale`/`Accept-Language` speak full tags (`fr-FR`), so every
/// external tag is reduced through [`normalize_locale`] before it touches the catalog.
pub const SUPPORTED_LOCALES: &[&str] = &["fr", "en"];

/// The cookie that carries a user's explicit language choice (#110): set by the language switch
/// (client-side, non-secret) and read by SSR on the next request — the pre-auth/instant half of the
/// resolution chain; `Customer.locale` is the durable, cross-device half.
pub const LOCALE_COOKIE: &str = "captain_locale";

/// Reduce any BCP-47-ish tag (`fr`, `fr-FR`, `FR`, `en_US`) to a SUPPORTED bare locale, else `None`.
pub fn normalize_locale(raw: &str) -> Option<&'static str> {
    let primary = raw.trim().split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    SUPPORTED_LOCALES.iter().copied().find(|l| *l == primary)
}

/// The runtime locale-resolution chain (#110, PROP-20260724-133700 §1c):
/// `Customer.locale -> cookie -> Accept-Language/device -> DEFAULT_LOCALE`. Each source is tried in
/// order; the first that normalizes to a SUPPORTED locale wins, else the platform default.
pub fn resolve_locale(
    customer_locale: Option<&str>,
    cookie_locale: Option<&str>,
    accept_language: Option<&str>,
) -> &'static str {
    customer_locale
        .and_then(normalize_locale)
        .or_else(|| cookie_locale.and_then(normalize_locale))
        .or_else(|| accept_language.and_then(parse_accept_language))
        .unwrap_or(DEFAULT_LOCALE)
}

/// First SUPPORTED locale in an `Accept-Language` header. q-values are ignored beyond order — browsers
/// already send most-preferred first, which is sufficient for a two-locale catalog.
pub fn parse_accept_language(header: &str) -> Option<&'static str> {
    header
        .split(',')
        .filter_map(|part| part.split(';').next()) // drop `;q=...`
        .find_map(normalize_locale)
}

fn catalog() -> &'static HashMap<String, HashMap<String, String>> {
    static CATALOG: OnceLock<HashMap<String, HashMap<String, String>>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(CATALOG_JSON)
            .expect("translations.generated.json: embedded catalog must parse (drift gate)")
    })
}

/// Resolve `key` in `locale` (falling back to [`FALLBACK_LOCALE`], then to the visible
/// `[key]` marker). `{param}` tokens are left verbatim — parameter interpolation belongs to the
/// call sites that own the values (`format_message`).
pub fn resolve(key: &str, locale: &str) -> String {
    // Tolerate full tags (`fr-FR`) and unknown locales: the catalog is keyed by bare SUPPORTED tags,
    // so reduce first, then fall back to en, then to the visible `[key]` marker.
    let loc = normalize_locale(locale).unwrap_or(FALLBACK_LOCALE);
    match catalog().get(key) {
        Some(messages) => messages
            .get(loc)
            .or_else(|| messages.get(FALLBACK_LOCALE))
            .cloned()
            .unwrap_or_else(|| format!("[{key}]")),
        None => format!("[{key}]"),
    }
}

/// Resolve + interpolate `{param}` tokens from the given pairs.
pub fn format_message(key: &str, locale: &str, params: &[(&str, &str)]) -> String {
    let mut msg = resolve(key, locale);
    for (name, value) in params {
        msg = msg.replace(&format!("{{{name}}}"), value);
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_fr_by_default_and_falls_back_to_en() {
        // A real catalog key (shared nav) — fr and en both exist.
        assert_eq!(resolve("common.nav.home", "fr"), "Accueil");
        assert_eq!(resolve("common.nav.home", "en"), "Home");
        // Unknown locale falls back to en, never blank.
        assert_eq!(resolve("common.nav.home", "de"), "Home");
    }

    #[test]
    fn missing_key_is_visibly_marked_not_blank() {
        assert_eq!(resolve("no.such.key", "fr"), "[no.such.key]");
    }

    #[test]
    fn params_interpolate() {
        // account.coins_badge = "{points} pts"
        assert_eq!(format_message("account.coins_badge", "fr", &[("points", "120")]), "120 pts");
    }

    #[test]
    fn the_new_surface_catalogs_are_merged_in() {
        // Keys from the split-4 sidecars prove the codegen merge covers the new surfaces.
        assert_eq!(resolve("back.orders.accept", "fr"), "Accepter");
        assert_eq!(resolve("rider.jobs.title", "en"), "My deliveries");
    }

    #[test]
    fn resolve_tolerates_full_tags_and_underscores() {
        // #110: Customer.locale / Accept-Language speak `fr-FR`; the catalog is keyed bare.
        assert_eq!(resolve("common.nav.home", "fr-FR"), "Accueil");
        assert_eq!(resolve("common.nav.home", "en_US"), "Home");
        assert_eq!(resolve("common.nav.home", "FR"), "Accueil");
    }

    #[test]
    fn normalize_reduces_tags_to_supported_or_none() {
        assert_eq!(normalize_locale("fr-FR"), Some("fr"));
        assert_eq!(normalize_locale("EN"), Some("en"));
        assert_eq!(normalize_locale("de-DE"), None);
        assert_eq!(normalize_locale(""), None);
    }

    #[test]
    fn resolve_locale_walks_the_chain_customer_then_cookie_then_accept_then_default() {
        // Customer.locale wins when present and supported.
        assert_eq!(resolve_locale(Some("en-US"), Some("fr"), Some("fr-FR")), "en");
        // Falls to the cookie when there is no customer choice.
        assert_eq!(resolve_locale(None, Some("en"), Some("fr")), "en");
        // Falls to Accept-Language when no customer and no cookie.
        assert_eq!(resolve_locale(None, None, Some("de-DE,en;q=0.8,fr;q=0.6")), "en");
        // Unsupported everywhere -> platform default (fr).
        assert_eq!(resolve_locale(None, None, Some("de-DE,es;q=0.8")), "fr");
        assert_eq!(resolve_locale(None, None, None), "fr");
        // An unsupported earlier source is skipped, not fatal.
        assert_eq!(resolve_locale(Some("de"), Some("en"), None), "en");
    }

    #[test]
    fn accept_language_picks_the_first_supported_in_order() {
        assert_eq!(parse_accept_language("fr-FR,fr;q=0.9,en;q=0.8"), Some("fr"));
        assert_eq!(parse_accept_language("de,en-GB;q=0.9"), Some("en"));
        assert_eq!(parse_accept_language("es,it"), None);
    }
}
