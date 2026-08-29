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

/// The classification + landings MOVED to `surface_runtime::hosts` for the #385 API-tier wiring
/// (the surface bins serve the same hosts) — re-exported so every in-crate consumer and test
/// keeps its `hosts::` path. ONE implementation, two hosts, no fork.
pub use surface_runtime::hosts::{classify_host, claim_landing, HostRoute, APEX};

use surface_runtime::hosts::cookie_value;

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
    // Per-request locale (#110): the resolution chain `cookie -> Accept-Language -> default`. The
    // `Customer.locale` rung is carried here by the `captain_locale` cookie (the language switch sets
    // both the cookie and, when authenticated, Customer.locale); a per-request JWT->Customer.locale
    // read is a documented follow-up. Replaces the hardcoded `fr`.
    let cookie_locale = cookie_value(&headers, web::i18n::LOCALE_COOKIE);
    let accept_language =
        headers.get(header::ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok());
    let locale = web::i18n::resolve_locale(None, cookie_locale.as_deref(), accept_language);
    // The retired `/r/{slug}` form 301s to its canonical host BEFORE any host dispatch (#749):
    // one rule for every host, because the address was handed out from more than one.
    if let Some(redirect) = path_addressed_redirect(uri.path()) {
        return redirect;
    }
    match classify_host(raw) {
        HostRoute::Tenant(slug) => tenant_page(&lookup, &ssr, &slug, raw, uri.path(), locale).await,
        other => render(other, &ssr, raw, uri.path(), locale).await,
    }
}

/// SSR one app page with live data (#92): the screen's `data_requirements` resolve through the
/// in-process transport before rendering.
async fn app_page(ssr: &crate::web_ssr::SsrExec, raw_host: &str, path: &str, locale: &str) -> Option<String> {
    // The SSR degrade boundary (#440): about to render the checkout shell with NO mountable
    // publishable key — a degraded render that produces ZERO place-order runs, which the saga
    // contract cannot see by construction. Counted HERE, the framework boundary, because `web`
    // compiles to wasm and stays telemetry-free (specs/observability.yaml, place-order metrics).
    if ssr.stripe_publishable_key.is_none() {
        if let (_, Some(m)) = web::router::resolve(raw_host, path) {
            if m.screen.id == "checkout" {
                telemetry::meters::place_order::degraded_render("stripe_key_absent");
            }
        }
    }
    // ONE transport per page render (its correlation id is the render's id — see
    // `SsrExec::transport`), held here so the degrade boundary below can name it.
    let transport = ssr.transport();
    let page = web::router::render_path_with(
        &transport,
        raw_host,
        path,
        locale,
        ssr.stripe_publishable_key.as_ref(),
    )
    .await?;
    // The #472 degrade boundary: a page whose declared read FAILED for real (never a
    // role-refused skip-by-design) or whose declared condition could not be parsed shipped a
    // degraded/error state. Counted HERE, the framework boundary, because `web` compiles to wasm
    // and stays telemetry-free (specs/observability.yaml, read-authorization metrics).
    let correlation_id = transport.correlation_id();
    for d in &page.degraded {
        telemetry::meters::read_authorization::sdui_degraded_render(
            d.screen,
            d.resolver,
            d.reason.as_str(),
            &correlation_id,
        );
    }
    // Skips-by-design (#745): NEVER counted (a skip is the documented posture — a zero-weight
    // metric reason is a signal wired never to scream), but always TRACEABLE: one event per skip
    // carrying the same correlation id as the read-path spans, so a page that renders an empty
    // slot can answer WHY from its own trace. At `info!` DELIBERATELY (#748 checkpoint,
    // blocking): production pins LOG_LEVEL=info, so a `debug!` here reached neither the JSON
    // logs nor OTLP under deployed defaults — the exact wired-never-to-scream class this comment
    // opens with. Proven visible under the deployed filter by
    // `tests/skip_trace_visibility.rs` (seen RED against the debug form). Volume is bounded by
    // the skip table: at most `skipped_reads.len()` lines per page render, a static per-screen
    // constant (0–1 across today's corpus).
    for s in &page.skipped {
        tracing::info!(
            screen = s.screen,
            resolver = s.resolver,
            reason = s.reason.as_str(),
            correlation_id = %correlation_id,
            "sdui read skipped by design"
        );
    }
    Some(page.html)
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
    locale: &str,
) -> Response {
    let registered = match &lookup.0 {
        Some(repo) => match repo.by_slug(Slug(slug.to_string())).await {
            Ok(Some(_)) => true,
            // Not a current address. Before treating it as unclaimed, check whether it is a SUPERSEDED
            // one (ADR-20260728-011344): a renamed storefront's old label is on printed menus, QR codes,
            // Google listings and inbound links, so it must keep working. 301 to whatever the restaurant's
            // current address is -- resolved through the restaurant row, so one hop lands on the live
            // label however many times it has been renamed.
            Ok(None) => {
                match repo.by_previous_slug(Slug(slug.to_string())).await {
                    Ok(Some(current)) => match &current.slug {
                        Some(current_slug) if current_slug.0 != slug => {
                            let target = format!("https://{}.{APEX}{path}", current_slug.0);
                            return (
                                StatusCode::MOVED_PERMANENTLY,
                                [(axum::http::header::LOCATION, target)],
                            )
                                .into_response();
                        }
                        // No current address, or it somehow equals this one: nothing to redirect to.
                        // Fall through rather than emit a self-redirect loop.
                        _ => false,
                    },
                    Ok(None) => false,
                    Err(_) => true, // fail open to the storefront shell
                }
            }
            Err(_) => true, // fail open to the storefront shell
        },
        None => true, // no database (dev): every slug is a storefront
    };
    if registered {
        return match app_page(ssr, raw_host, path, locale).await {
            Some(html) => Html(html).into_response(),
            None => (StatusCode::NOT_FOUND, "no such page").into_response(),
        };
    }
    // Unclaimed: every path on the host gets the landing (the whole subdomain is the offer).
    Html(claim_landing(slug)).into_response()
}

/// The retired path-addressed storefront (`/r/{slug}`, #749 — founder directive 2026-08-29,
/// verbatim: *"I don't want to have /r/<slug> possible we already have it in the
/// <slug>.captain.food"*): the HOST is the tenant selector, so the path form is gone from the
/// screen tables — but the address was HANDED OUT (printed menus, QR codes, old search results),
/// so it 301s to the canonical host root instead of dead-ending (the ADR-20260728-011344
/// precedent for superseded storefront addresses). Only a well-formed slug label redirects — the
/// Location header is built from the path segment, so anything outside `[a-z0-9-]` (or with a
/// deeper path) falls through to the ordinary 404 rather than being reflected.
fn path_addressed_redirect(path: &str) -> Option<Response> {
    let label = path.strip_prefix("/r/")?.trim_end_matches('/');
    let well_formed = !label.is_empty()
        && !label.contains('/')
        && label.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !label.starts_with('-')
        && !label.ends_with('-');
    well_formed.then(|| {
        (
            StatusCode::MOVED_PERMANENTLY,
            [(header::LOCATION, format!("https://{label}.{APEX}/"))],
        )
            .into_response()
    })
}

async fn render(route: HostRoute, ssr: &crate::web_ssr::SsrExec, raw_host: &str, path: &str, locale: &str) -> Response {
    match route {
        // The audience SDUI surfaces: SSR the matched screen WITH live data (web::router mirrors
        // classify_host's audience mapping — see its module docs).
        HostRoute::Live | HostRoute::Restos | HostRoute::Riders => {
            match app_page(ssr, raw_host, path, locale).await {
                Some(html) => Html(html).into_response(),
                None => (StatusCode::NOT_FOUND, "no such page").into_response(),
            }
        }
        // Handled by `tenant_page` before this fn — unreachable defensively kept explicit.
        HostRoute::Tenant(_) => (StatusCode::NOT_FOUND, "no such page").into_response(),
        HostRoute::System => text("System backoffice"),
        HostRoute::Api => text("Captain.Food API — GraphQL served at /{role}/graphql (see /public/graphql)"),
        HostRoute::Default => {
            // localhost / *.onrender.com / IPs: serve the marketplace SHELL — deliberately WITHOUT
            // data resolution (#107): Render's health probe hits this branch on every deploy check,
            // and #92's data-resolving SSR here ran the discovery reads per probe and OOM-killed the
            // 512Mi instance mid-deploy. Probes and dev hosts need a page, not the catalog; the
            // real product hosts (Live/Restos/Riders/Tenant) keep data-full SSR.
            match web::router::render_path(raw_host, path, locale) {
                Some(html) => Html(html).into_response(),
                None => text("Captain.Food server — address a *.captain.food host"),
            }
        }
        HostRoute::Unknown(sub) => {
            (StatusCode::NOT_FOUND, format!("unknown host '{sub}.{APEX}'")).into_response()
        }
    }
}

/// `200 text/plain` body. `text/plain` (not HTML) makes reflecting the tenant slug injection-safe.
fn text(body: &str) -> Response {
    (StatusCode::OK, body.to_string()).into_response()
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
        /// A label this restaurant has renamed away from, if any (ADR-20260728-011344).
        renamed_from: Option<&'static str>,
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
        /// `renamed_from` stands in for a `slugalias` hit: it resolves to the restaurant that moved
        /// away from that label, whose CURRENT slug is `registered`.
        async fn by_previous_slug(&self, slug: Slug) -> Result<Option<RestaurantRow>, DomainError> {
            if self.erroring {
                return Err(DomainError::Repository("read model down".into()));
            }
            Ok((Some(slug.0.as_str()) == self.renamed_from).then(|| row(self.registered)))
        }
    }

    fn ssr() -> crate::web_ssr::SsrExec {
        // A dep-less schema: PUBLIC reads resolve empty, which is exactly the SSR degrade contract.
        // No publishable key — the checkout-degrade default; `ssr_with_key` is the configured twin.
        crate::web_ssr::SsrExec {
            schema: crate::graphql::schema::build_schema(None, None, None),
            stripe_publishable_key: None,
        }
    }

    /// The #440 configured state: what production is with STRIPE_PUBLISHABLE_KEY set.
    fn ssr_with_key(raw: &str) -> crate::web_ssr::SsrExec {
        crate::web_ssr::SsrExec {
            schema: crate::graphql::schema::build_schema(None, None, None),
            stripe_publishable_key: web::stripe::PublishableKey::parse(Some(raw)),
        }
    }

    async fn body_of(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.expect("body");
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// The behaviour that protects printed menus, QR codes and search results: a renamed storefront's
    /// OLD host keeps working, with a 301 to the current one (ADR-20260728-011344).
    #[tokio::test]
    async fn a_superseded_host_redirects_to_the_current_address() {
        let lookup = TenantLookup(Some(Arc::new(StubRestaurants {
            registered: "chez-test",
            erroring: false,
            renamed_from: Some("chez-test-old"),
        })));
        let response = tenant_page(
            &lookup,
            &ssr(),
            "chez-test-old",
            "chez-test-old.captain.food",
            "/menu",
            "fr",
        )
        .await;
        assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
        let location = response.headers().get(axum::http::header::LOCATION).expect("Location");
        // The PATH is preserved -- a deep link into the old storefront lands on the same page, not the
        // homepage. Losing it would turn every shared menu link into a bounce.
        assert_eq!(location, "https://chez-test.captain.food/menu");
    }

    /// An unknown label with no alias is still the claim offer, not a redirect -- the fallback must not
    /// swallow the acquisition path.
    #[tokio::test]
    async fn an_unknown_host_with_no_alias_still_sees_the_offer() {
        let lookup = TenantLookup(Some(Arc::new(StubRestaurants {
            registered: "chez-test",
            erroring: false,
            renamed_from: Some("chez-test-old"),
        })));
        let response =
            tenant_page(&lookup, &ssr(), "nobody", "nobody.captain.food", "/", "fr").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(body_of(response).await.contains("join.captain.food"));
    }

    #[tokio::test]
    async fn registered_tenant_root_serves_its_storefront() {
        let lookup =
            TenantLookup(Some(Arc::new(StubRestaurants { registered: "chez-test", erroring: false, renamed_from: None })));
        let response = tenant_page(&lookup, &ssr(), "chez-test", "chez-test.captain.food", "/", "fr").await;
        let html = body_of(response).await;
        assert!(html.contains("data-hydrate=\"restaurant\""), "{html}");
        assert!(!html.contains("join.captain.food"), "a registered slug must never see the offer");
    }

    #[tokio::test]
    async fn unclaimed_slug_gets_the_join_landing_on_every_path() {
        let lookup =
            TenantLookup(Some(Arc::new(StubRestaurants { registered: "chez-test", erroring: false, renamed_from: None })));
        for path in ["/", "/anything"] {
            let response = tenant_page(&lookup, &ssr(), "chezmarco", "chezmarco.captain.food", path, "fr").await;
            let html = body_of(response).await;
            assert!(html.contains("https://join.captain.food/#rejoindre"), "{path}: {html}");
            assert!(html.contains("chezmarco.captain.food"), "{path}: the offer names the address");
        }
    }

    #[tokio::test]
    async fn lookup_failure_fails_open_to_the_storefront_never_the_offer() {
        // A DB hiccup must not show "this address is available" for a real restaurant.
        let lookup =
            TenantLookup(Some(Arc::new(StubRestaurants { registered: "chez-test", erroring: true, renamed_from: None })));
        let response = tenant_page(&lookup, &ssr(), "chez-test", "chez-test.captain.food", "/", "fr").await;
        let html = body_of(response).await;
        assert!(html.contains("data-hydrate=\"restaurant\""), "{html}");
        // No database at all (dev): same fail-open behaviour.
        let response = tenant_page(&TenantLookup(None), &ssr(), "any-slug", "any-slug.captain.food", "/", "fr").await;
        assert!(body_of(response).await.contains("data-hydrate=\"restaurant\""));
    }

    /// #440 end to end through production's own call path (`tenant_page` → `app_page` →
    /// `render_path_with`): the configured key reaches the served /checkout BODY as the mount
    /// div's data attribute; unconfigured (and — the architect pin — EMPTY, which the generated
    /// config validation lets through as `Some("")`) serves the degraded shell instead.
    #[tokio::test]
    async fn the_served_checkout_body_carries_the_key_iff_the_service_is_configured() {
        let lookup = TenantLookup(Some(Arc::new(StubRestaurants {
            registered: "chez-test",
            erroring: false,
            renamed_from: None,
        })));
        let page = |exec: crate::web_ssr::SsrExec| {
            let lookup = &lookup;
            async move {
                let response = tenant_page(
                    lookup,
                    &exec,
                    "chez-test",
                    "chez-test.captain.food",
                    "/checkout",
                    "fr",
                )
                .await;
                body_of(response).await
            }
        };

        let html = page(ssr_with_key("pk_test_abc123")).await;
        assert!(html.contains("data-pk=\"pk_test_abc123\""), "{html}");
        assert!(html.contains("js.stripe.com"), "{html}");
        assert!(!html.contains("payment_unavailable_state"), "{html}");

        // Unset, empty, and malformed are ONE state — parse in the composition root collapses
        // them — and all three serve the honest degrade, never a dead element.
        for exec in [ssr(), ssr_with_key(""), ssr_with_key("pk_live_abc123")] {
            let html = page(exec).await;
            assert!(html.contains("id=\"payment_unavailable_state\""), "{html}");
            assert!(!html.contains("data-pk="), "{html}");
            assert!(!html.contains("js.stripe.com"), "{html}");
        }
    }

    /// #749 (founder, verbatim: "I don't want to have /r/<slug> possible we already have it in
    /// the <slug>.captain.food"): the path-addressed storefront is GONE — but a printed QR code,
    /// bookmark or old search result must not dead-end, so any `/r/{slug}` path 301s to the
    /// canonical host root (the ADR-20260728-011344 precedent: redirect, never 404, for an
    /// address that was once handed out). Seen RED against the route still serving the screen.
    #[tokio::test]
    async fn a_path_addressed_storefront_redirects_to_its_canonical_host() {
        let lookup = TenantLookup(Some(Arc::new(StubRestaurants {
            registered: "chez-test",
            erroring: false,
            renamed_from: None,
        })));
        // Through PRODUCTION'S OWN entry (`host_root`), on the tenant's own host AND on any
        // other app host: same 301, same target.
        for host in ["chez-test.captain.food", "live.captain.food"] {
            let response = through_host_root(&lookup, host, "/r/chez-test").await;
            assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY, "{host}");
            assert_eq!(
                response.headers().get(axum::http::header::LOCATION).expect("Location"),
                "https://chez-test.captain.food/",
                "{host}"
            );
        }
        // A malformed label is NOT reflected into a Location header — plain 404 (the storefront
        // route no longer matches any path, and the redirect refuses non-slug labels).
        let response =
            through_host_root(&lookup, "chez-test.captain.food", "/r/Bad%20Label").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    /// Drive the REAL router fallback (`host_root`) — the production dispatch, no test fork.
    async fn through_host_root(lookup: &TenantLookup, host: &str, path: &str) -> Response {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, host.parse().unwrap());
        host_root(
            Extension(lookup.clone()),
            Extension(ssr()),
            headers,
            path.parse::<Uri>().unwrap(),
        )
        .await
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
