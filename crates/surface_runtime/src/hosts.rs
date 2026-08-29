//! Host-header (subdomain) classification for the multi-tenant topology (ADR-0036) — MOVED from
//! the monolith's `server::hosts` for the #385 API-tier wiring (`server` re-exports these, so
//! there is ONE implementation serving both the monolith fallback and the surface bins).
//!
//! Reserved subdomains (ADR-0036) are fixed audiences; any other valid label is a restaurant
//! tenant `{slug}`. `www`/`join` and the bare apex marketing 301s are handled off-platform;
//! `hooks` is the declared integration ingress host (#385, c4-l2 `adapters.ingress_host`) —
//! reserved here so it can never be claimed as a storefront.

use axum::http::HeaderMap;

/// The PRODUCTION apex under which every audience/tenant host lives — the address the product
/// hands out (claim landings, superseded-slug 301 targets). Audience-space CLASSIFICATION spans
/// one more root — dev's `.localhost` (#755) — whose one authority is
/// `web::router::AUDIENCE_SPACE_ROOTS` (consumed below), never a second copy here.
pub const APEX: &str = "captain.food";

/// What a request `Host` resolves to. Pure data — see [`classify_host`].
#[derive(Debug, PartialEq, Eq)]
pub enum HostRoute {
    Live,
    Restos,
    Riders,
    System,
    Api,
    Tenant(String),
    /// A host outside audience space: an internal probe host, a bare root (`localhost`, the bare
    /// apex), a direct IP. Neutral landing.
    Default,
    /// An audience-space label (`*.captain.food` / `*.localhost`) that is neither a reserved
    /// audience nor a valid slug (incl. `www`/`join`, served off-platform, and the reserved
    /// integration host `hooks`).
    Unknown(String),
}

/// Classify a raw `Host` header value (may carry a port) into a [`HostRoute`]. Pure — unit-tested
/// in `server::hosts` (the monolith fallback) and consumed by every surface bin.
pub fn classify_host(raw_host: &str) -> HostRoute {
    let host = raw_host.split(':').next().unwrap_or("").trim().to_ascii_lowercase();
    // Only `<label>.{root}` for an audience-space root (`web::router::AUDIENCE_SPACE_ROOTS` — the
    // production apex + dev's `.localhost`, #755; ONE suffix authority shared with the web
    // router's surface mapping) is audience/tenant space; anything else, the bare roots included,
    // is the neutral default. Label semantics are IDENTICAL under every root.
    let Some(sub) = web::router::audience_label(&host) else {
        return HostRoute::Default;
    };
    match sub {
        "live" => HostRoute::Live,
        "restos" => HostRoute::Restos,
        "riders" => HostRoute::Riders,
        "system" => HostRoute::System,
        "api" => HostRoute::Api,
        // Reserved off-platform marketing hosts + the integration ingress host (#385): never a
        // tenant. `hooks` routes to the `adapters` bin via the generated Ingress at cutover.
        "www" | "join" | "hooks" => HostRoute::Unknown(sub.to_string()),
        s if is_valid_slug(s) => HostRoute::Tenant(s.to_string()),
        s => HostRoute::Unknown(s.to_string()),
    }
}

/// The claim-your-subdomain landing (#98, product-owner directive): an unclaimed `{slug}` is an
/// acquisition surface, not a 404 — the CTA sends the restaurateur to the join form. The slug is
/// safe to reflect: [`classify_host`] only builds `Tenant` from [`is_valid_slug`] labels
/// (lowercase alphanumerics + dashes).
pub fn claim_landing(slug: &str) -> String {
    format!(
        "<!DOCTYPE html><html lang=\"fr\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{slug}.{APEX} — disponible</title></head><body>\
<main data-c=\"claim_landing\">\
<h1>{slug}.{APEX} est disponible pour votre restaurant</h1>\
<p>Cette adresse n'est pas encore utilisée. Rejoignez Captain.Food et offrez à vos clients \
une boutique en ligne à votre nom — commissions réduites, commandes en direct.</p>\
<p lang=\"en\">This address is available for your restaurant — join Captain.Food and get your \
own online storefront.</p>\
<a href=\"https://join.captain.food/#rejoindre\" data-c=\"cta_banner\">Rejoindre Captain.Food</a>\
</main></body></html>"
    )
}

/// Read one cookie value from the request `Cookie` header (used for the non-secret
/// `captain_locale`).
pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(axum::http::header::COOKIE).and_then(|v| v.to_str().ok()).and_then(|raw| {
        raw.split(';').map(str::trim).find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            (k.trim() == name).then(|| v.trim().to_string()).filter(|s| !s.is_empty())
        })
    })
}

/// `^[a-z0-9]+(?:-[a-z0-9]+)*$` — lowercase alphanumeric segments joined by single dashes (the
/// slug convention, CLAUDE.md). Input is already lowercased by [`classify_host`].
pub fn is_valid_slug(s: &str) -> bool {
    if s.is_empty() || s.starts_with('-') || s.ends_with('-') || s.contains("--") {
        return false;
    }
    s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
