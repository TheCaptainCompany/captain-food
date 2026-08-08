//! The `fo-*`/`bo-*` surface runtime (#385 API-tier wiring, PROP-20260807-174246 D8).
//!
//! A surface bin serves ONE audience's hosts: its wasm `/assets`, and the SSR of its generated
//! screen trees with data resolved THROUGH THE ROLE GATEWAY over the same
//! [`web::graphql::HttpTransport`] the hydrating browser uses — a surface holds no database
//! credential and no views access (D8: "no surface binary holds broad views access any more").
//! SSR stays PUBLIC + anonymous + sessionless, the monolith's `web_ssr` contract: a server render
//! can never see more than an anonymous first request would.
//!
//! Host classification and the tenant landings MOVED here from the monolith's `server::hosts`
//! (which re-exports them — one implementation, two hosts, no fork).
//!
//! RECORDED GAPS (#385, cutover preconditions for `fo-storefront`):
//! - The tenant probe rides the public `restaurant(slug:)` query, which resolves CURRENT slugs
//!   only. Without the monolith's `by_previous_slug` read there is no alias data, so an unknown
//!   slug FAILS OPEN to the storefront shell — the claim-your-subdomain landing and the
//!   superseded-slug 301 (ADR-20260728-011344) need an alias-aware public read before this bin
//!   takes tenant traffic. The monolith keeps both until then.

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    Router,
};

pub mod hosts;

pub use hosts::{classify_host, claim_landing, HostRoute};

/// Which audience a surface bin serves — one bin, one audience (the deploy topology's Ingress
/// routes each host to its surface Service).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    /// `fo-marketplace`: `live.captain.food` + the bare apex (spec-settled owner, #385) + the
    /// neutral default hosts (probes, localhost).
    Marketplace,
    /// `fo-storefront`: every `{slug}.captain.food` tenant host.
    Storefront,
    /// `bo-restaurant`: `restos.captain.food`.
    RestaurantBackoffice,
    /// `bo-rider`: `riders.captain.food`.
    Rider,
    /// `bo-admin`: `system.captain.food` — the plain landing the monolith serves (the system
    /// screens have no `web` surface yet; parity, not a gap introduced here).
    AdminLanding,
}

/// What a surface bin's generated Config supplies.
#[derive(Clone)]
pub struct SurfaceSettings {
    pub audience: Audience,
    /// The role gateway origin SSR reads go through. Empty ⇒ the in-cluster default
    /// (`http://gateway-public` — SSR is always the anonymous PUBLIC role, per the monolith's
    /// `web_ssr` contract).
    pub gateway_url: String,
    /// The wasm hydrate bundle dir, served under `/assets` (empty dir ⇒ pages stay SSR-only —
    /// degraded, never broken; same rule as the monolith).
    pub web_assets_dir: String,
}

#[derive(Clone)]
struct SurfaceState {
    audience: Audience,
    gateway_url: String,
}

/// The resolved gateway origin (health detail + transport base).
pub fn gateway_origin(settings: &SurfaceSettings) -> String {
    if settings.gateway_url.trim().is_empty() {
        "http://gateway-public".to_string()
    } else {
        settings.gateway_url.trim().trim_end_matches('/').to_string()
    }
}

/// The axum app of one surface bin: `/assets` (wasm bundle) + the audience host fallback.
pub fn surface_app(settings: &SurfaceSettings) -> Router {
    let state = SurfaceState {
        audience: settings.audience,
        gateway_url: gateway_origin(settings),
    };
    Router::new()
        .nest_service(
            "/assets",
            tower_http::services::ServeDir::new(settings.web_assets_dir.clone()),
        )
        .fallback(host_root)
        .with_state(state)
}

/// One page render's transport: the PUBLIC role over the gateway, a fresh anonymous session —
/// SSR can never see more than an anonymous first request would (the `web_ssr` contract).
fn ssr_transport(gateway_url: &str) -> web::graphql::HttpTransport {
    web::graphql::HttpTransport::new(
        gateway_url,
        web::graphql::Role::Public,
        web::session::SessionId::mint(),
    )
}

async fn ssr_page(gateway_url: &str, raw_host: &str, path: &str, locale: &str) -> Option<String> {
    web::router::render_path_with(&ssr_transport(gateway_url), raw_host, path, locale).await
}

/// Router fallback: resolve the request `Host` + path for THIS audience and serve the SDUI app —
/// the same dispatch the monolith's `hosts::host_root` performs, restricted to one audience and
/// reading through the gateway instead of in-process.
async fn host_root(
    State(state): State<SurfaceState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let raw = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    // Per-request locale (#110): cookie → Accept-Language → default, same chain as the monolith.
    let cookie_locale = hosts::cookie_value(&headers, web::i18n::LOCALE_COOKIE);
    let accept_language = headers.get(header::ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok());
    let locale = web::i18n::resolve_locale(None, cookie_locale.as_deref(), accept_language);
    let path = uri.path();
    match (state.audience, classify_host(raw)) {
        // The marketplace's product hosts get data-full SSR; the neutral default hosts
        // (probes, localhost, direct IPs) DELIBERATELY get the shell without data resolution —
        // the #107 rule (a health probe must never run the discovery reads).
        (Audience::Marketplace, HostRoute::Live) => {
            page(ssr_page(&state.gateway_url, raw, path, &locale).await)
        }
        (Audience::Marketplace, HostRoute::Default) => {
            match web::router::render_path(raw, path, &locale) {
                Some(html) => Html(html).into_response(),
                None => (
                    StatusCode::OK,
                    "Captain.Food fo-marketplace — address a *.captain.food host",
                )
                    .into_response(),
            }
        }
        (Audience::Storefront, HostRoute::Tenant(slug)) => {
            tenant_page(&state, &slug, raw, path, &locale).await
        }
        (Audience::RestaurantBackoffice, HostRoute::Restos) => {
            page(ssr_page(&state.gateway_url, raw, path, &locale).await)
        }
        (Audience::Rider, HostRoute::Riders) => {
            page(ssr_page(&state.gateway_url, raw, path, &locale).await)
        }
        // Monolith parity: the system audience serves a plain landing until its screens gain a
        // `web` surface.
        (Audience::AdminLanding, HostRoute::System) => {
            (StatusCode::OK, "System backoffice").into_response()
        }
        (audience, other) => (
            StatusCode::NOT_FOUND,
            format!("host {other:?} is not served by this surface ({audience:?})"),
        )
            .into_response(),
    }
}

fn page(html: Option<String>) -> Response {
    match html {
        Some(html) => Html(html).into_response(),
        None => (StatusCode::NOT_FOUND, "no such page").into_response(),
    }
}

/// The tenant branch, gateway-backed: a REGISTERED slug serves its storefront; an unknown or
/// unverifiable one FAILS OPEN to the storefront shell (see the module docs — the claim landing
/// and the alias 301 wait on an alias-aware public read; a DB hiccup or a renamed slug must
/// never be told "this address is available").
async fn tenant_page(
    state: &SurfaceState,
    slug: &str,
    raw_host: &str,
    path: &str,
    locale: &str,
) -> Response {
    use web::graphql::Transport as _;
    // `slug` came out of `classify_host`, so it matches the slug charset — safe to inline.
    let probe = ssr_transport(&state.gateway_url)
        .execute(
            &format!("query {{ restaurant(input: {{ slug: \"{slug}\" }}) {{ id }} }}"),
            serde_json::json!({}),
        )
        .await;
    let registered = match probe {
        Ok(data) => !data.get("restaurant").is_none_or(serde_json::Value::is_null),
        // Gateway/subgraph unreachable or erroring: fail OPEN to the shell, like the monolith
        // does on a repository error.
        Err(_) => true,
    };
    if registered {
        return page(ssr_page(&state.gateway_url, raw_host, path, locale).await);
    }
    // Positively absent AND alias-unknowable (recorded gap): still the storefront shell, never
    // the claim landing — a renamed restaurant's printed QR codes outrank the acquisition page.
    page(ssr_page(&state.gateway_url, raw_host, path, locale).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_audience_owns_exactly_its_hosts() {
        // The dispatch table above is the product's host contract — pin the classification the
        // audiences key on (the full classify_host matrix is tested in hosts.rs).
        assert_eq!(classify_host("live.captain.food"), HostRoute::Live);
        assert_eq!(classify_host("restos.captain.food"), HostRoute::Restos);
        assert_eq!(classify_host("riders.captain.food"), HostRoute::Riders);
        assert_eq!(classify_host("system.captain.food"), HostRoute::System);
        assert_eq!(
            classify_host("tonton-pizza.captain.food"),
            HostRoute::Tenant("tonton-pizza".into())
        );
    }

    #[test]
    fn gateway_origin_defaults_to_the_public_gateway_service() {
        let s = SurfaceSettings {
            audience: Audience::Marketplace,
            gateway_url: String::new(),
            web_assets_dir: "/tmp/none".into(),
        };
        assert_eq!(gateway_origin(&s), "http://gateway-public");
        let s = SurfaceSettings { gateway_url: "http://127.0.0.1:7200/".into(), ..s };
        assert_eq!(gateway_origin(&s), "http://127.0.0.1:7200");
    }
}
