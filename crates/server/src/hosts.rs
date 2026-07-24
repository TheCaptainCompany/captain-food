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

use std::sync::Arc;

use application::queries::RestaurantReadRepository;
use axum::{
    http::{header, HeaderMap, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    Extension,
};
use domain::generated::scalars::Slug;

/// The fallback's read access to the restaurant read model (#98): decides registered-vs-unclaimed
/// for a tenant host. `None` when no database is configured (dev) — every slug then serves the
/// storefront shell.
#[derive(Clone)]
pub struct TenantLookup(pub Option<Arc<dyn RestaurantReadRepository>>);

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

/// Router fallback: resolve the request `Host` + path and serve the SDUI app (split 4/4 of #21) —
/// the audience surfaces (`live`/`restos`/`riders`) and every restaurant tenant render their
/// GENERATED screen trees server-side (`web::router::render_path`; the wasm bundle hydrates with
/// live data). A tenant host is first checked against the restaurant read model (#98): a
/// REGISTERED slug serves its storefront (`/` included — the tenant-root rule), an UNCLAIMED one
/// gets the claim-your-subdomain landing. Non-app hosts keep their plain-text landings; an app
/// host with an unknown path 404s.
pub async fn host_root(
    Extension(lookup): Extension<TenantLookup>,
    Extension(ssr): Extension<crate::web_ssr::SsrExec>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let raw = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    match classify_host(raw) {
        HostRoute::Tenant(slug) => tenant_page(&lookup, &ssr, &slug, raw, uri.path()).await,
        other => render(other, &ssr, raw, uri.path()).await,
    }
}

/// SSR one app page with live data (#92): the screen's `data_requirements` resolve through the
/// in-process transport before rendering.
async fn app_page(ssr: &crate::web_ssr::SsrExec, raw_host: &str, path: &str) -> Option<String> {
    web::router::render_path_with(&ssr.transport(), raw_host, path, web::i18n::DEFAULT_LOCALE).await
}

/// The tenant branch (#98): registered → storefront; positively-absent → the claim landing;
/// lookup unavailable or erroring → the storefront shell (FAIL OPEN — a DB hiccup must never show
/// "this address is available" for a real restaurant).
async fn tenant_page(
    lookup: &TenantLookup,
    ssr: &crate::web_ssr::SsrExec,
    slug: &str,
    raw_host: &str,
    path: &str,
) -> Response {
    let registered = match &lookup.0 {
        Some(repo) => match repo.by_slug(Slug(slug.to_string())).await {
            Ok(found) => found.is_some(),
            Err(_) => true, // fail open to the storefront shell
        },
        None => true, // no database (dev): every slug is a storefront
    };
    if registered {
        return match app_page(ssr, raw_host, path).await {
            Some(html) => Html(html).into_response(),
            None => (StatusCode::NOT_FOUND, "no such page").into_response(),
        };
    }
    // Unclaimed: every path on the host gets the landing (the whole subdomain is the offer).
    Html(claim_landing(slug)).into_response()
}

async fn render(route: HostRoute, ssr: &crate::web_ssr::SsrExec, raw_host: &str, path: &str) -> Response {
    match route {
        // The audience SDUI surfaces: SSR the matched screen WITH live data (web::router mirrors
        // classify_host's audience mapping — see its module docs).
        HostRoute::Live | HostRoute::Restos | HostRoute::Riders => {
            match app_page(ssr, raw_host, path).await {
                Some(html) => Html(html).into_response(),
                None => (StatusCode::NOT_FOUND, "no such page").into_response(),
            }
        }
        // Handled by `tenant_page` before this fn — unreachable defensively kept explicit.
        HostRoute::Tenant(_) => (StatusCode::NOT_FOUND, "no such page").into_response(),
        HostRoute::System => text("System backoffice"),
        HostRoute::Api => text("Captain.Food API — GraphQL served at /{role}/graphql (see /public/graphql)"),
        HostRoute::Default => {
            // localhost / *.onrender.com / IPs: serve the marketplace app too — a dev box or the
            // Render health-check host hitting `/` should see the product, not a placeholder.
            match app_page(ssr, raw_host, path).await {
                Some(html) => Html(html).into_response(),
                None => text("Captain.Food server — address a *.captain.food host"),
            }
        }
        HostRoute::Unknown(sub) => {
            (StatusCode::NOT_FOUND, format!("unknown host '{sub}.{APEX}'")).into_response()
        }
    }
}

/// The claim-your-subdomain landing (#98, product-owner directive): an unclaimed `{slug}` is an
/// acquisition surface, not a 404 — the CTA sends the restaurateur to the join form. The slug is
/// safe to reflect: `classify_host` only builds `Tenant` from `is_valid_slug` labels (lowercase
/// alphanumerics + dashes).
fn claim_landing(slug: &str) -> String {
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
    use application::queries::{RestaurantFilter, RestaurantRow};
    use async_trait::async_trait;
    use domain::shared::errors::DomainError;
    use domain::generated::scalars::RestaurantId;

    /// A stub read model: one registered slug, everything else absent (or a hard error).
    struct StubRestaurants {
        registered: &'static str,
        erroring: bool,
    }

    fn row(slug: &str) -> RestaurantRow {
        serde_json::from_value(serde_json::json!({
            "restaurant_id": "00000000-0000-7000-8000-000000000001",
            "restaurant_account_id": null, "listing_status": "ACTIVE_PARTNER",
            "external_identifiers": null, "google_place_id": null,
            "slug": slug, "display_name": "Chez Test", "description": null,
            "tags": null, "margin_rate": null, "cuisine_category": null,
            "uber_prices_opt_in": null, "website": null, "rating": null, "reviews_count": null,
            "gbp_order_url": null, "gbp_link_status": null,
            "address": {}, "location": null, "opening_hours": {},
            "status": "ACTIVE", "order_acceptance": "NORMAL", "default_currency": "EUR",
            "timezone": null, "preparation_time_minutes": null,
            "created_at": "2026-07-24T00:00:00Z", "updated_at": "2026-07-24T00:00:00Z",
        }))
        .expect("stub row deserializes")
    }

    #[async_trait]
    impl application::queries::RestaurantReadRepository for StubRestaurants {
        async fn list(&self, _f: RestaurantFilter) -> Result<Vec<RestaurantRow>, DomainError> {
            Ok(vec![])
        }
        async fn by_slug(&self, slug: Slug) -> Result<Option<RestaurantRow>, DomainError> {
            if self.erroring {
                return Err(DomainError::Repository("read model down".into()));
            }
            Ok((slug.0 == self.registered).then(|| row(&slug.0)))
        }
        async fn by_id(&self, _id: RestaurantId) -> Result<Option<RestaurantRow>, DomainError> {
            Ok(None)
        }
    }

    fn ssr() -> crate::web_ssr::SsrExec {
        // A dep-less schema: PUBLIC reads resolve empty, which is exactly the SSR degrade contract.
        crate::web_ssr::SsrExec { schema: crate::graphql::schema::build_schema(None, None, None) }
    }

    async fn body_of(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.expect("body");
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[tokio::test]
    async fn registered_tenant_root_serves_its_storefront() {
        let lookup =
            TenantLookup(Some(Arc::new(StubRestaurants { registered: "chez-test", erroring: false })));
        let response = tenant_page(&lookup, &ssr(), "chez-test", "chez-test.captain.food", "/").await;
        let html = body_of(response).await;
        assert!(html.contains("data-hydrate=\"restaurant\""), "{html}");
        assert!(!html.contains("join.captain.food"), "a registered slug must never see the offer");
    }

    #[tokio::test]
    async fn unclaimed_slug_gets_the_join_landing_on_every_path() {
        let lookup =
            TenantLookup(Some(Arc::new(StubRestaurants { registered: "chez-test", erroring: false })));
        for path in ["/", "/anything"] {
            let response = tenant_page(&lookup, &ssr(), "chezmarco", "chezmarco.captain.food", path).await;
            let html = body_of(response).await;
            assert!(html.contains("https://join.captain.food/#rejoindre"), "{path}: {html}");
            assert!(html.contains("chezmarco.captain.food"), "{path}: the offer names the address");
        }
    }

    #[tokio::test]
    async fn lookup_failure_fails_open_to_the_storefront_never_the_offer() {
        // A DB hiccup must not show "this address is available" for a real restaurant.
        let lookup =
            TenantLookup(Some(Arc::new(StubRestaurants { registered: "chez-test", erroring: true })));
        let response = tenant_page(&lookup, &ssr(), "chez-test", "chez-test.captain.food", "/").await;
        let html = body_of(response).await;
        assert!(html.contains("data-hydrate=\"restaurant\""), "{html}");
        // No database at all (dev): same fail-open behaviour.
        let response = tenant_page(&TenantLookup(None), &ssr(), "any-slug", "any-slug.captain.food", "/").await;
        assert!(body_of(response).await.contains("data-hydrate=\"restaurant\""));
    }

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
