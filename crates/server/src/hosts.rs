//! Host-header (subdomain) routing for the multi-tenant topology (ADR-0036).
//!
//! One deployed server answers every `*.captain.food` host (the Dynadot DNS wildcard → Render). This module
//! maps the request `Host` to a placeholder landing per audience; real web apps replace these later.
//! Reserved subdomains (ADR-0036) are fixed audiences; any other valid label is a restaurant tenant
//! `{slug}`. `api.captain.food` is served by the GraphQL routes (`/{role}/graphql`, ADR-0006); its bare `/`
//! shows a pointer. `www`/`join` and the bare apex are handled off-Render (301 → GitHub Pages marketing),
//! so they should never arrive here; if one does it is treated as unknown.
//!
//! This is wired as the router **fallback**, so the explicit routes (`/health`, `/ping`, `/projector`,
//! `/{role}/graphql`) always win — in particular Render's health check (which hits the internal
//! `*.onrender.com` host) is unaffected. Bodies are `text/plain`, so reflecting the `{slug}` is
//! injection-safe.

use axum::{
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

/// The apex under which every audience/tenant host lives.
const APEX: &str = "captain.food";

/// What a request `Host` resolves to. Pure data — see [`classify_host`].
#[derive(Debug, PartialEq, Eq)]
pub enum HostRoute {
    Live,
    Restos,
    Riders,
    System,
    Api,
    Tenant(String),
    /// Non-`captain.food` host: Render's internal `*.onrender.com`, `localhost`, a direct IP. Neutral landing.
    Default,
    /// A `*.captain.food` label that is neither a reserved audience nor a valid slug (incl. `www`/`join`,
    /// which are served off-Render and should not reach us).
    Unknown(String),
}

/// Classify a raw `Host` header value (may carry a port) into a [`HostRoute`]. Pure — unit-tested below.
pub fn classify_host(raw_host: &str) -> HostRoute {
    let host = raw_host.split(':').next().unwrap_or("").trim().to_ascii_lowercase();
    // Only `<label>.captain.food` is audience/tenant space; anything else is the neutral default.
    let sub = match host.strip_suffix(APEX).and_then(|p| p.strip_suffix('.')) {
        Some(sub) if !sub.is_empty() => sub,
        _ => return HostRoute::Default,
    };
    match sub {
        "live" => HostRoute::Live,
        "restos" => HostRoute::Restos,
        "riders" => HostRoute::Riders,
        "system" => HostRoute::System,
        "api" => HostRoute::Api,
        // Reserved off-Render marketing hosts; never expected here, never a tenant.
        "www" | "join" => HostRoute::Unknown(sub.to_string()),
        s if is_valid_slug(s) => HostRoute::Tenant(s.to_string()),
        s => HostRoute::Unknown(s.to_string()),
    }
}

/// Router fallback: resolve the request `Host` and return its placeholder landing.
pub async fn host_root(headers: HeaderMap) -> Response {
    let raw = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    render(classify_host(raw))
}

fn render(route: HostRoute) -> Response {
    match route {
        HostRoute::Live => text("This is the captain.food front-office"),
        HostRoute::Restos => text("Restaurants' backoffice"),
        HostRoute::Riders => text("Riders' backoffice"),
        HostRoute::System => text("System backoffice"),
        HostRoute::Api => text("Captain.Food API — GraphQL served at /{role}/graphql (see /public/graphql)"),
        HostRoute::Tenant(slug) => text(&format!("This is the front-office for the restaurant '{slug}'")),
        HostRoute::Default => text("Captain.Food server — address a *.captain.food host"),
        HostRoute::Unknown(sub) => {
            (StatusCode::NOT_FOUND, format!("unknown host '{sub}.{APEX}'")).into_response()
        }
    }
}

/// `200 text/plain` body. `text/plain` (not HTML) makes reflecting the tenant slug injection-safe.
fn text(body: &str) -> Response {
    (StatusCode::OK, body.to_string()).into_response()
}

/// `^[a-z0-9]+(?:-[a-z0-9]+)*$` — lowercase alphanumeric segments joined by single dashes (the slug
/// convention, CLAUDE.md). Input is already lowercased by [`classify_host`].
fn is_valid_slug(s: &str) -> bool {
    if s.is_empty() || s.starts_with('-') || s.ends_with('-') || s.contains("--") {
        return false;
    }
    s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_audiences_map_to_their_route() {
        assert_eq!(classify_host("live.captain.food"), HostRoute::Live);
        assert_eq!(classify_host("restos.captain.food"), HostRoute::Restos);
        assert_eq!(classify_host("riders.captain.food"), HostRoute::Riders);
        assert_eq!(classify_host("system.captain.food"), HostRoute::System);
        assert_eq!(classify_host("api.captain.food"), HostRoute::Api);
    }

    #[test]
    fn port_and_case_are_normalized() {
        assert_eq!(classify_host("LIVE.Captain.Food:443"), HostRoute::Live);
    }

    #[test]
    fn arbitrary_label_is_a_tenant_slug() {
        assert_eq!(classify_host("tonton-pizza.captain.food"), HostRoute::Tenant("tonton-pizza".into()));
        assert_eq!(classify_host("le-bureau.captain.food"), HostRoute::Tenant("le-bureau".into()));
    }

    #[test]
    fn off_render_and_non_apex_hosts_are_default() {
        assert_eq!(classify_host("captain-food.onrender.com"), HostRoute::Default);
        assert_eq!(classify_host("localhost:8080"), HostRoute::Default);
        assert_eq!(classify_host("captain.food"), HostRoute::Default); // bare apex never reaches Render
        assert_eq!(classify_host(""), HostRoute::Default);
    }

    #[test]
    fn marketing_and_malformed_labels_are_unknown() {
        assert_eq!(classify_host("www.captain.food"), HostRoute::Unknown("www".into()));
        assert_eq!(classify_host("join.captain.food"), HostRoute::Unknown("join".into()));
        assert_eq!(classify_host("-bad.captain.food"), HostRoute::Unknown("-bad".into()));
        assert_eq!(classify_host("a.b.captain.food"), HostRoute::Unknown("a.b".into()));
    }
}
