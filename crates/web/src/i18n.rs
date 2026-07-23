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
    match catalog().get(key) {
        Some(messages) => messages
            .get(locale)
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
}
