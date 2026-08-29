//! Surface + route resolution (split 4/4 of #21) — which app a HOST serves, and which screen a
//! PATH names inside it.
//!
//! Host → surface (the multi-tenant model, ADR-0036's reserved subdomains + ADR-20260722-160000;
//! MIRRORED with the server's `hosts::classify_host` — `web` cannot depend on `server`, same
//! mirror-honesty rule as `Role::segment`):
//!   * `captain.food` / `www.` / `live.` → the **marketplace** (`captain_frontoffice`);
//!   * `restos.captain.food`  → the **restaurant back office** (ADR-0036 reserved audience);
//!   * `riders.captain.food`  → the **rider app** (ADR-0036 reserved audience);
//!   * any other `{slug}.captain.food` → that restaurant's **storefront** (`restaurant_frontoffice`),
//!     the slug being the first label;
//!   * localhost / IPs / unknown hosts → the marketplace (the safe anonymous default).
//!
//! Path → screen: routes come from the GENERATED screen tables (`generated/screens.rs`), matched
//! segment-wise with `:param` capture (`/orders/:orderId/confirmation`). Captured params feed
//! resolver arguments on the hydrate path (`param_args`).

use crate::generated::data_layer::ResolverKey;
use crate::generated::screens::{self, Screen};
use crate::graphql::Role;

/// The four SDUI surfaces — one per `specs/screens/*.yaml` audience file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    CaptainFrontoffice,
    RestaurantFrontoffice,
    RestaurantBackoffice,
    Rider,
}

impl Surface {
    /// The generated screen table of this surface.
    pub fn screens(&self) -> &'static [Screen] {
        match self {
            Surface::CaptainFrontoffice => screens::captain_frontoffice::SCREENS,
            Surface::RestaurantFrontoffice => screens::restaurant_frontoffice::SCREENS,
            Surface::RestaurantBackoffice => screens::restaurant_backoffice::SCREENS,
            Surface::Rider => screens::rider::SCREENS,
        }
    }

    /// The generated bottom sheets of this surface (#94) — mounted hidden into every screen.
    pub fn sheets(&self) -> &'static [crate::generated::screens::Sheet] {
        match self {
            Surface::CaptainFrontoffice => screens::captain_frontoffice::SHEETS,
            Surface::RestaurantFrontoffice => screens::restaurant_frontoffice::SHEETS,
            Surface::RestaurantBackoffice => screens::restaurant_backoffice::SHEETS,
            Surface::Rider => screens::rider::SHEETS,
        }
    }

    /// The GraphQL role path this surface's ANONYMOUS/default client talks to. The customer
    /// surfaces start anonymous (`/public`) and upgrade after auth; staff surfaces are their role
    /// by construction (the path 401s without a matching JWT — fail closed).
    pub fn role(&self) -> Role {
        match self {
            Surface::CaptainFrontoffice | Surface::RestaurantFrontoffice => Role::Public,
            Surface::RestaurantBackoffice => Role::Restaurant,
            Surface::Rider => Role::Rider,
        }
    }

    /// The storefront tenant slug when this host is a `{slug}.captain.food` storefront.
    /// Excludes every ADR-0036 reserved audience label (`live`/`restos`/`riders`/`system`/`api`),
    /// the off-server marketing hosts (`www`/`join`), and the integration ingress host
    /// (`hooks`, #385 — mirrors `server::hosts::classify_host`, same mirror-honesty rule).
    pub fn slug_of(host: &str) -> Option<&str> {
        let host = host.split(':').next().unwrap_or(host);
        let label = host.strip_suffix(".captain.food")?;
        (!label.contains('.')
            && !matches!(
                label,
                "www" | "join" | "hooks" | "live" | "restos" | "riders" | "system" | "api"
            ))
        .then_some(label)
    }
}

/// Resolve the serving surface from the request `Host`.
pub fn surface_for_host(host: &str) -> Surface {
    let host = host.split(':').next().unwrap_or(host); // strip port
    match host {
        "captain.food" | "www.captain.food" | "live.captain.food" => Surface::CaptainFrontoffice,
        "restos.captain.food" => Surface::RestaurantBackoffice,
        "riders.captain.food" => Surface::Rider,
        other => {
            if Surface::slug_of(other).is_some() {
                Surface::RestaurantFrontoffice
            } else {
                // localhost / IPs / preview hosts: the marketplace is the anonymous-safe default.
                Surface::CaptainFrontoffice
            }
        }
    }
}

/// A matched route: the screen + the captured `:param` values.
#[derive(Debug, Clone)]
pub struct RouteMatch {
    pub screen: &'static Screen,
    pub params: Vec<(String, String)>,
}

impl RouteMatch {
    /// A captured param by name.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// The GraphQL input args a route's params feed into one of its resolvers: by convention a
    /// `:param` maps onto the arg OF THE SAME NAME; the one naming mismatch in the spec today is
    /// `order.byId` (query arg `id`) fed by `:orderId`, mapped explicitly.
    pub fn param_args(&self, resolver: ResolverKey) -> Vec<(String, serde_json::Value)> {
        self.params
            .iter()
            .map(|(k, v)| {
                let arg = match (resolver, k.as_str()) {
                    (ResolverKey::OrderById, "orderId") => "id".to_string(),
                    _ => k.clone(),
                };
                (arg, serde_json::Value::String(v.clone()))
            })
            .collect()
    }
}

/// Match `path` against a surface's generated routes: literal segments must equal, `:name`
/// segments capture. Trailing-slash tolerant; query strings are the caller's to strip.
pub fn match_route(surface: Surface, path: &str) -> Option<RouteMatch> {
    let want: Vec<&str> = path.trim_end_matches('/').split('/').filter(|s| !s.is_empty()).collect();
    'screens: for screen in surface.screens() {
        let have: Vec<&str> =
            screen.route.trim_end_matches('/').split('/').filter(|s| !s.is_empty()).collect();
        if have.len() != want.len() {
            continue;
        }
        let mut params = Vec::new();
        for (h, w) in have.iter().zip(&want) {
            if let Some(name) = h.strip_prefix(':') {
                params.push((name.to_string(), (*w).to_string()));
            } else if h != w {
                continue 'screens;
            }
        }
        return Some(RouteMatch { screen, params });
    }
    None
}

/// Resolve `host` + `path` to a screen — the table match PLUS the tenant-root rule (#98): on a
/// `{slug}.captain.food` storefront, `/` IS the restaurant screen, its `slug` param taken from the
/// HOST (the ADR-0036 tenant model — the host is the tenant selector; the `/r/:slug` path route
/// stays for path-addressed access). Both the SSR entry (`render_path`) and the hydrate entry go
/// through here so the two paths cannot disagree.
pub fn resolve(host: &str, path: &str) -> (Surface, Option<RouteMatch>) {
    let surface = surface_for_host(host);
    let matched = match_route(surface, path).or_else(|| {
        let is_root = path.trim_end_matches('/').is_empty();
        if surface == Surface::RestaurantFrontoffice && is_root {
            let slug = Surface::slug_of(host)?;
            let screen = surface.screens().iter().find(|s| s.id == "restaurant")?;
            return Some(RouteMatch { screen, params: vec![("slug".into(), slug.to_string())] });
        }
        None
    });
    (surface, matched)
}

/// The module script that boots the wasm bundle over an SSR page. The bundle URL is fixed
/// (`/assets/web.js`, served by the BFF's asset route out of the Docker image); on a deployment
/// without assets the script 404s and the page simply stays server-rendered — degraded, never broken.
#[cfg(feature = "ssr")]
const HYDRATE_SCRIPT: &str = "<script type=\"module\">import init, { hydrate } from '/assets/web.js'; await init(); hydrate();</script>";

/// One SSR'd page + what degraded while building it (#472). The renderer stays telemetry-free
/// (it compiles to wasm), so the DEGRADATIONS travel out to the server boundary, which counts
/// them (`sdui_degraded_render_total{screen, resolver, reason}` — specs/observability.yaml,
/// read-authorization).
#[cfg(feature = "ssr")]
pub struct RenderedPage {
    pub html: String,
    pub degraded: Vec<Degradation>,
}

/// One degraded fact of one page render (#472).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Degradation {
    /// The generated screen id (a closed set).
    pub screen: &'static str,
    /// The failing resolver's spec key — `"none"` for condition defects, which belong to the
    /// screen tree, not a read.
    pub resolver: &'static str,
    pub reason: DegradedReason,
}

/// The contract's bounded server-leg reason set (`client_*` legs are reserved in the contract,
/// unemitted — no OTel in WASM).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradedReason {
    /// A REAL transport/contract failure on a read this role path is allowed to ask
    /// (`graphql::classify_resolve` — a role-refused read is a skip by design, never counted).
    ResolverFailed,
    /// A declared condition expression outside the corpus grammar reached a render (fails closed
    /// + loud DOM marker; the codegen corpus gate keeps checked-in specs out of here).
    ConditionUnparseable,
}

impl DegradedReason {
    /// The contract's label value.
    pub fn as_str(&self) -> &'static str {
        match self {
            DegradedReason::ResolverFailed => "resolver_failed",
            DegradedReason::ConditionUnparseable => "condition_unparseable",
        }
    }
}

/// Server-side render with LIVE data (#92): resolve the matched screen's `data_requirements`
/// through the given transport (the BFF passes its in-process `SchemaTransport` — no loopback
/// HTTP) before rendering, exactly like the hydrate path (route `:params` feed resolver args), so
/// the initial HTML carries the real content the screens spec contracts
/// (`rendering_strategy: SSR_first`). Since #472 a resolver outcome is CLASSIFIED
/// (`graphql::classify_resolve`): a skip-by-design (declared gap, role-refused read on this
/// anonymous transport) leaves the binding silently unresolved (the shell slot renders empty;
/// hydrate retries), while a REAL failure marks the binding FAILED — the renderer shows its error
/// state, never the empty state — and is reported on [`RenderedPage::degraded`]. Either way SSR
/// must degrade, never 500.
///
/// #420 removed BOTH conditions this fetch used to carry, and each removal is a separate decision:
///
///   * `screen.sdui` — never had a reason. `checkout` and `order_tracking` declare
///     `data_requirements` like every other screen; skipping them is what shipped a checkout shell
///     with an empty cart summary and a confirmation page stuck on the not-found hero for every
///     order (PROP-20260809-021351 §2, G5/G6).
///   * `!screen.requires_auth` — the stated reason was "a document GET carries no credentials, so
///     their session-scoped reads could only answer empty". That is a fact about the TRANSPORT, not
///     about the screen, and this function cannot know it: the caller supplies the transport. Let
///     the transport answer. Today's in-process SSR transport is anonymous/PUBLIC, so a
///     CUSTOMER-scoped read (`order`, `cart`) fails its role guard and the binding is skipped —
///     byte-identical output to the old skip, one wasted role-guard rejection per requirement. The
///     day the BFF's SSR transport carries the caller's identity, the confirmation page is right on
///     first paint with no further change here. (The #107 OOM is unrelated: that was the DEFAULT
///     host branch, which still serves through the data-less [`render_path`].)
///
/// `stripe_publishable_key` (#440) is the server's configured checkout delivery fact, already
/// parsed to "mountable or absent" — it rides the [`RenderContext`] into the checkout shell
/// (a data attribute on the Stripe mount div), never a window global and never GraphQL.
#[cfg(feature = "ssr")]
pub async fn render_path_with<T: crate::graphql::Transport + Sync>(
    transport: &T,
    host: &str,
    path: &str,
    locale: &str,
    stripe_publishable_key: Option<&crate::stripe::PublishableKey>,
) -> Option<RenderedPage> {
    use crate::graphql::ResolveOutcome;
    use crate::renderer::RenderContext;
    let (surface, matched) = resolve(host, path);
    let matched = matched?;
    let mut ctx = RenderContext::new(locale);
    ctx.stripe_publishable_key = stripe_publishable_key.cloned();
    let mut degraded: Vec<Degradation> = Vec::new();
    for resolver in matched.screen.data_requirements {
        let mut vars = serde_json::Map::new();
        for (k, v) in matched.param_args(*resolver) {
            vars.insert(k, v);
        }
        let result = crate::graphql::execute_resolver(transport, *resolver, vars).await;
        match crate::graphql::classify_resolve(surface.role(), *resolver, result) {
            ResolveOutcome::Resolved(value) => ctx.insert_resolved(resolver.as_str(), value),
            ResolveOutcome::SkippedByDesign => {}
            ResolveOutcome::Failed(_) => {
                ctx.insert_failed(resolver.as_str());
                degraded.push(Degradation {
                    screen: matched.screen.id,
                    resolver: resolver.as_str(),
                    reason: DegradedReason::ResolverFailed,
                });
            }
        }
    }
    // Condition-defect pre-scan (#472): parseability is a STATIC property of the screen tree
    // (never data-dependent), so one pure walk reports every expression the renderer will fail
    // closed on — the `condition_unparseable` leg of the degradation counter.
    for _defect in crate::condition::condition_defects(matched.screen.tree) {
        degraded.push(Degradation {
            screen: matched.screen.id,
            resolver: "none",
            reason: DegradedReason::ConditionUnparseable,
        });
    }
    Some(RenderedPage { html: render_matched(&matched, surface, ctx, host, locale), degraded })
}

/// Server-side render the page for `host` + `path` — the data-less entry (SSR SHELL only; the
/// hydrate bundle fetches). Kept for data-less callers and tests; the BFF serves through
/// [`render_path_with`]. `None` = no such route (404).
#[cfg(feature = "ssr")]
pub fn render_path(host: &str, path: &str, locale: &str) -> Option<String> {
    use crate::renderer::RenderContext;
    let (surface, matched) = resolve(host, path);
    let matched = matched?;
    Some(render_matched(&matched, surface, RenderContext::new(locale), host, locale))
}

/// The shared tail of both entries: render the matched screen (SDUI tree + sheets, or the
/// hand-written non-SDUI shells) and inject the hydrate boot script.
///
/// The hand-written branch dispatches on [`HandWrittenScreen`], whose variants are proved at
/// COMPILE TIME to be exactly the `sdui: false` set (`handwritten.rs`) — so the old
/// `_ => empty SDUI shell` fallback, which is what a new hand-written screen used to land in
/// silently, no longer exists and cannot be reintroduced without failing the build.
#[cfg(feature = "ssr")]
fn render_matched(
    matched: &RouteMatch,
    surface: Surface,
    ctx: crate::renderer::RenderContext,
    host: &str,
    locale: &str,
) -> String {
    use crate::handwritten::HandWrittenScreen;
    use crate::renderer::render_screen_html;
    let html = match HandWrittenScreen::of(matched.screen) {
        Some(hand_written) => {
            hand_written.render_html(matched, &ctx, Surface::slug_of(host), locale)
        }
        None => render_screen_html(matched.screen, surface.sheets(), ctx),
    };
    html.replace("</body>", &format!("{HYDRATE_SCRIPT}</body>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "ssr")]
    #[test]
    fn ssr_html_lang_reflects_the_resolved_locale() {
        // #110: the SSR shell's `<html lang>` carries the resolved locale (hydrate reads it back).
        let en = render_path("live.captain.food", "/", "en").expect("home renders");
        assert!(en.contains("<html lang=\"en\">"), "en shell should tag lang=en");
        let fr = render_path("live.captain.food", "/", "fr").expect("home renders");
        assert!(fr.contains("<html lang=\"fr\">"), "fr shell should tag lang=fr");
        // An unsupported/full tag normalizes to the platform default (fr) — never a bad lang attr.
        let de = render_path("live.captain.food", "/", "de-DE").expect("home renders");
        assert!(de.contains("<html lang=\"fr\">"), "unsupported locale falls back to fr");
    }

    #[test]
    fn hosts_route_to_their_surfaces() {
        assert_eq!(surface_for_host("captain.food"), Surface::CaptainFrontoffice);
        assert_eq!(surface_for_host("live.captain.food"), Surface::CaptainFrontoffice);
        assert_eq!(surface_for_host("www.captain.food:443"), Surface::CaptainFrontoffice);
        assert_eq!(surface_for_host("restos.captain.food"), Surface::RestaurantBackoffice);
        assert_eq!(surface_for_host("riders.captain.food"), Surface::Rider);
        assert_eq!(surface_for_host("chez-test.captain.food"), Surface::RestaurantFrontoffice);
        assert_eq!(Surface::slug_of("chez-test.captain.food"), Some("chez-test"));
        // Unknown hosts / localhost: anonymous-safe marketplace default.
        assert_eq!(surface_for_host("localhost:8080"), Surface::CaptainFrontoffice);
        assert_eq!(surface_for_host("127.0.0.1"), Surface::CaptainFrontoffice);
        // `slug_of` takes a HOST, never an ORIGIN: it splits on `:` to strip a port, so an origin
        // reduces to "https" and the storefront label silently vanishes. The hydrate mount got this
        // wrong once (#420, caught in self-review, never shipped) — pinned so it cannot come back.
        assert_eq!(Surface::slug_of("https://chez-test.captain.food"), None);
        assert_eq!(Surface::slug_of("chez-test.captain.food:8080"), Some("chez-test"));
    }

    #[test]
    fn staff_surfaces_talk_to_their_role_paths() {
        assert_eq!(Surface::RestaurantBackoffice.role().segment(), "restaurant");
        assert_eq!(Surface::Rider.role().segment(), "rider");
        assert_eq!(Surface::RestaurantFrontoffice.role().segment(), "public");
    }

    #[test]
    fn routes_match_with_params() {
        let m = match_route(Surface::RestaurantFrontoffice, "/orders/abc-123/confirmation")
            .expect("tracking route");
        assert_eq!(m.screen.id, "order_tracking");
        assert_eq!(m.param("orderId"), Some("abc-123"));
        // The explicit naming bridge: :orderId feeds order.byId's `id` arg.
        let args = m.param_args(ResolverKey::OrderById);
        assert_eq!(args[0].0, "id");

        let m = match_route(Surface::Rider, "/jobs/xyz").expect("rider job detail");
        assert_eq!(m.screen.id, "job_detail");
        // Same-name convention: :orderId feeds delivery.byOrder's `orderId`.
        let args = m.param_args(ResolverKey::DeliveryByOrder);
        assert_eq!(args[0].0, "orderId");
    }

    #[test]
    fn every_generated_route_is_reachable_and_unknown_paths_are_none() {
        for surface in [
            Surface::CaptainFrontoffice,
            Surface::RestaurantFrontoffice,
            Surface::RestaurantBackoffice,
            Surface::Rider,
        ] {
            for screen in surface.screens() {
                // Substitute a dummy value for each :param, then the route must match itself.
                let concrete: String = screen
                    .route
                    .split('/')
                    .map(|seg| if seg.starts_with(':') { "x" } else { seg })
                    .collect::<Vec<_>>()
                    .join("/");
                let m = match_route(surface, &concrete)
                    .unwrap_or_else(|| panic!("route {} unreachable", screen.route));
                assert_eq!(m.screen.id, screen.id);
            }
            assert!(match_route(surface, "/definitely/not/a/route").is_none());
        }
    }

    #[test]
    fn tenant_root_is_the_restaurant_screen_with_the_slug_from_the_host() {
        // #98: on a {slug} storefront, `/` IS the storefront — slug from the HOST.
        let (surface, m) = resolve("chez-marco.captain.food", "/");
        assert_eq!(surface, Surface::RestaurantFrontoffice);
        let m = m.expect("tenant root must resolve");
        assert_eq!(m.screen.id, "restaurant");
        assert_eq!(m.param("slug"), Some("chez-marco"));
        // The path route keeps working, and a non-root unknown path still 404s.
        assert_eq!(resolve("chez-marco.captain.food", "/r/other").1.unwrap().screen.id, "restaurant");
        assert!(resolve("chez-marco.captain.food", "/nope").1.is_none());
        // The marketplace root is untouched by the rule.
        assert_eq!(resolve("captain.food", "/").1.unwrap().screen.id, "home");
    }

    #[cfg(feature = "ssr")]
    #[tokio::test]
    async fn render_path_with_ships_live_data_in_the_initial_html() {
        use crate::graphql::test_support::FakeTransport;
        use serde_json::json;
        // The marketplace home: data_requirements = [promotions.active (GAP — refused before any
        // network), categories.all, restaurants.featured, restaurants.all] → 3 transport calls.
        let fake = FakeTransport::scripted(vec![
            Ok(json!({ "categories": [] })),
            Ok(json!({ "restaurants": [{ "displayName": "Chez Test", "slug": "chez-test",
                        "address": { "city": "Tours" } }] })),
            Ok(json!({ "restaurants": [] })),
        ]);
        let html = render_path_with(&fake, "captain.food", "/", "fr", None).await.expect("home renders").html;
        // The SSR HTML carries the restaurant — no client fetch needed for first paint (#92).
        assert!(html.contains("Chez Test"), "{html}");
        assert!(html.contains("data-slug=\"chez-test\""));
        assert_eq!(fake.call_count(), 3, "one read per non-gap data requirement");
        // The featured rail's pinned arg travelled (the #82 contract, now exercised server-side).
        assert!(fake.call(1).0.contains("$input: RestaurantsQueryInput!"));
        assert_eq!(fake.call(1).1["input"]["list"], json!("RECOMMENDED"));

        // A requires_auth screen ASKS its transport (since #420 — see `render_path_with`'s docs:
        // whether a session-scoped read can be answered is the transport's fact, not the screen's)
        // and DEGRADES to a shell when the answer is a refusal. Today's SSR transport is
        // anonymous/PUBLIC, so this is what production sees: one role-guard rejection, and the
        // client owns the data — byte-identical output to the old unconditional skip.
        let fake = FakeTransport::scripted(vec![Err(crate::graphql::TransportError::Errors(
            "Unauthorized: orders requires CUSTOMER".into(),
        ))]);
        let html = render_path_with(&fake, "chez-marco.captain.food", "/orders", "fr", None)
            .await
            .expect("order history renders").html;
        assert!(html.contains("data-hydrate=\"order_history\""));
        assert_eq!(fake.call_count(), 1, "the screen's declared read is attempted");
        assert!(html.contains("data-empty=\"true\""), "a refused read degrades, never 500s: {html}");
    }

    /// #420 / PROP-20260809-021351 §2 (G6): the confirmation page is the ONE screen a customer who
    /// has just paid looks at, and until now production SSR built it as `TrackingState::new(id)` —
    /// the UNKNOWN / not-found hero, for every order, forever. Rendered through **production's own
    /// call site** (`render_path_with`, what the BFF serves), not through `render_tracking_html`
    /// with a state the test built itself.
    #[cfg(feature = "ssr")]
    #[tokio::test]
    async fn the_confirmation_page_tells_a_stranger_what_state_their_order_is_in() {
        use crate::graphql::test_support::FakeTransport;
        use serde_json::json;

        let order = || {
            json!({ "order": {
                "id": "00000000-0000-7000-8000-000000000000",
                "status": "ACCEPTED",
                "statusChangedAt": "2026-08-09T19:30:00Z",
                "estimatedReadyAt": "2026-08-09T19:55:00Z",
                "items": [{ "offerId": "o1" }, { "offerId": "o2" }],
            }})
        };
        let path = "/orders/00000000-0000-7000-8000-000000000000/confirmation";

        // The human sentence, in BOTH shipped languages — a `data-i18n` attribute with an empty
        // element is not a page that tells anybody anything.
        for (locale, sentence) in [("fr", "Commande acceptée"), ("en", "Order accepted")] {
            let fake = FakeTransport::scripted(vec![Ok(order())]);
            let html = render_path_with(&fake, "chez-test.captain.food", path, locale, None)
                .await
                .expect("the confirmation route renders").html;
            assert_eq!(fake.call_count(), 1, "the page must READ the order it is about: {locale}");
            assert!(html.contains(sentence), "{locale}: no human status sentence in {html}");
            assert!(html.contains("data-status=\"ACCEPTED\""), "{locale}: {html}");
            assert!(
                !html.contains("data-status=\"UNKNOWN\""),
                "{locale}: a real order must not render the not-found hero: {html}"
            );
            assert!(
                !html.contains("[order.status."),
                "{locale}: no `[key]` fallback marker — every key resolved: {html}"
            );
        }

        // A read the transport cannot answer degrades to the not-found hero — never a 500, never a
        // blank page (the SSR contract), and never a claim about an order we could not see.
        let fake = FakeTransport::scripted(vec![Ok(json!({ "order": null }))]);
        let html = render_path_with(&fake, "chez-test.captain.food", path, "fr", None)
            .await
            .expect("the confirmation route still renders").html;
        assert!(html.contains("data-status=\"UNKNOWN\""), "{html}");
        assert!(html.contains("Commande introuvable"), "the not-found copy is resolved too: {html}");
    }

    /// #420 / PROP-20260809-021351 §2 (G5): the checkout shell was hardcoded to an empty restaurant,
    /// zero lines, an empty total and `payment_failed: false` at its ONLY production call site — so
    /// `checkout::tests::a_failed_payment_renders_the_failure_state_…` passed over a state
    /// production never built. This is the counterpart test, through the real call site.
    #[cfg(feature = "ssr")]
    #[tokio::test]
    async fn the_checkout_shell_carries_the_cart_it_is_about_to_charge_for() {
        use crate::graphql::test_support::FakeTransport;
        use serde_json::json;

        // The response key is `current`, not `cart` (#451): the storefront checkout binds
        // `cart.current`, whose GraphQL field is `current` — `data_layer::response_key` unwraps
        // THAT. A fixture keyed `cart` binds nothing and renders the empty shell, which is how
        // this test caught the Phase-1 rebinding.
        let cart = || {
            json!({ "current": {
                "id": "cart-1", "restaurantId": "r-1", "status": "OPEN",
                "lines": [
                    { "offerId": "o1", "name": "Burger maison", "quantity": 2 },
                    { "offerId": "o2", "name": "Frites", "quantity": 1 },
                ],
                "totalAmount": { "amountCents": 2350, "currency": "EUR" },
            }})
        };
        let profile = || json!({ "me": { "customerId": "c-1", "displayName": "Camille Durand" } });
        // RSO-1: the checkout screen now ALSO declares `restaurant.bySlug` (Phase 1 spec — the
        // service-window evidence the shell will bind when the refusal surfacing lands), so the
        // shell performs a 4th read. Scripted with the prod-smoke L4 shape (timezone, no hours →
        // HOURS_UNDECLARED, which accepts in both gate positions).
        let restaurant = || {
            json!({ "restaurant": {
                "displayName": "Chez Test", "slug": "chez-test", "timezone": "Europe/Paris",
                "serviceWindow": { "verdict": "HOURS_UNDECLARED", "opensAt": null, "lastOrderAt": null,
                                   "evaluatedAt": "2026-01-06T12:00:00Z", "validUntil": "2026-01-06T12:15:00Z" },
            }})
        };

        let fake = FakeTransport::scripted(vec![
            Ok(cart()),
            Ok(profile()),
            Ok(json!({ "paymentStatus": null })),
            Ok(restaurant()),
        ]);
        let html = render_path_with(&fake, "chez-test.captain.food", "/checkout", "fr", None)
            .await
            .expect("the checkout route renders").html;
        assert_eq!(fake.call_count(), 4, "one read per declared resolver");
        assert!(html.contains("2 items"), "the real line count: {html}");
        assert!(html.contains("23,50 EUR"), "the real total, formatted: {html}");
        assert!(html.contains("chez-test"), "the restaurant being ordered from: {html}");
        assert!(
            !html.contains("payment_failed_state"),
            "no failure state before a failure: {html}"
        );

        // A FAILED payment status now REACHES the shell — the state the unit test asserts is built
        // by production, not only by a test.
        let fake = FakeTransport::scripted(vec![
            Ok(cart()),
            Ok(profile()),
            Ok(json!({ "paymentStatus": {
                "paymentIntentId": "pi_1", "clientSecret": null, "status": "FAILED",
            }})),
            Ok(restaurant()),
        ]);
        let html = render_path_with(&fake, "chez-test.captain.food", "/checkout", "fr", None)
            .await
            .expect("the checkout route renders").html;
        assert!(html.contains("id=\"payment_failed_state\""), "{html}");
        assert!(html.contains("Paiement refusé"), "{html}");
        assert!(html.contains("Votre carte n'a pas été débitée. Votre panier est intact."), "{html}");
        assert!(
            !html.contains("[checkout.payment_failed"),
            "no `[key]` fallback marker: {html}"
        );
    }

    /// #440: the delivery seam end to end at the router layer — the parsed key given to
    /// `render_path_with` reaches the checkout shell as the mount div's `data-pk` (plus the
    /// checkout-only stripe.js tag); no key ⇒ the degraded state, no attribute, no Stripe request.
    /// The type is the gate: this entry takes `Option<&PublishableKey>`, so an empty or malformed
    /// value is UNSPELLABLE here — it died at `PublishableKey::parse` in the composition root.
    #[cfg(feature = "ssr")]
    #[tokio::test]
    async fn the_checkout_shell_delivers_the_publishable_key_only_when_configured() {
        use crate::graphql::test_support::FakeTransport;
        use serde_json::json;
        let scripted = || {
            FakeTransport::scripted(vec![
                // Keyed `current` like the checkout's real read (#451) — this fixture asserts
                // nothing about the cart, so a stale key would have gone on passing silently.
                Ok(json!({ "current": {
                    "lines": [{ "offerId": "o1" }],
                    "totalAmount": { "amountCents": 1000, "currency": "EUR" },
                }})),
                Ok(json!({ "me": null })),
                Ok(json!({ "paymentStatus": null })),
                // RSO-1: the checkout screen's 4th declared read (`restaurant.bySlug`).
                Ok(json!({ "restaurant": {
                    "displayName": "Chez Test", "slug": "chez-test",
                    "serviceWindow": { "verdict": "HOURS_UNDECLARED", "opensAt": null, "lastOrderAt": null,
                                       "evaluatedAt": "2026-01-06T12:00:00Z", "validUntil": "2026-01-06T12:15:00Z" },
                }})),
            ])
        };

        let key = crate::stripe::PublishableKey::parse(Some("pk_test_abc123"));
        let fake = scripted();
        let html =
            render_path_with(&fake, "chez-test.captain.food", "/checkout", "fr", key.as_ref())
                .await
                .expect("checkout renders").html;
        assert!(html.contains("data-pk=\"pk_test_abc123\""), "{html}");
        assert!(html.contains("js.stripe.com"), "the checkout shell carries stripe.js: {html}");
        assert!(!html.contains("payment_unavailable_state"), "{html}");

        let fake = scripted();
        let html = render_path_with(&fake, "chez-test.captain.food", "/checkout", "fr", None)
            .await
            .expect("checkout renders").html;
        assert!(html.contains("id=\"payment_unavailable_state\""), "{html}");
        assert!(!html.contains("data-pk="), "{html}");
        assert!(!html.contains("js.stripe.com"), "no key, no Stripe request: {html}");
    }

    /// #472 (graphql-architect, blocking severity): a transport `Err` on a resolver this role IS
    /// allowed to ask must render an ERROR state — never the null-data empty state. A transient
    /// failure must never tell a paid customer their cart/order does not exist. Seen RED against
    /// the `if let Ok(value) = execute_resolver(...)` swallow (red evidence in the introducing
    /// commit message).
    #[cfg(feature = "ssr")]
    #[tokio::test]
    async fn a_transport_failure_renders_the_error_state_not_the_empty_state() {
        use crate::graphql::test_support::FakeTransport;
        // /cart declares exactly one read, `cart.current`, whose bound query admits PUBLIC — so a
        // transport failure here is a REAL failure, not the documented anonymous-SSR skip.
        let fake = FakeTransport::scripted(vec![Err(crate::graphql::TransportError::Network(
            "connection reset by peer".into(),
        ))]);
        let html = render_path_with(&fake, "chez-test.captain.food", "/cart", "fr", None)
            .await
            .expect("the cart route still renders — degraded, never 500").html;
        assert!(html.contains("data-error=\"true\""), "an error state must render: {html}");
        assert!(
            html.contains("Impossible de charger votre panier"),
            "the per-surface error copy (translation-keyed, fr): {html}"
        );
        assert!(html.contains("Réessayer"), "a user-initiated retry control: {html}");
        // The transport string is server internals — it must NEVER reach the customer's HTML.
        assert!(!html.contains("connection reset"), "no transport leak: {html}");
        // Fail-closed composition: with the cart unresolvable, the checkout button's
        // `disabled_when: cart.lines.length == 0` is unevaluatable → disabled.
        assert!(html.contains("disabled"), "checkout must not be clickable over no data: {html}");
        // #730: ONE state, not two — the summary's price rows (blank money over the same failed
        // read) render ABSENT; the cart_lines error above is the failure's single affordance.
        assert!(
            !html.contains("<div data-c=\"order_summary_block\""),
            "no blank-money summary beside the error state: {html}"
        );
        assert_eq!(html.matches("data-error=\"true\"").count(), 1, "ONE error state: {html}");
    }

    /// #472: error state and empty state are DISTINCT rendered states, per binding. A read that
    /// ANSWERS (null or empty) is the empty state — no error marker, no retry.
    #[cfg(feature = "ssr")]
    #[tokio::test]
    async fn an_answered_empty_read_is_the_empty_state_not_the_error_state() {
        use crate::graphql::test_support::FakeTransport;
        use serde_json::json;
        for answered in [json!({ "current": null }), json!({ "current": { "lines": [] } })] {
            let fake = FakeTransport::scripted(vec![Ok(answered.clone())]);
            let html = render_path_with(&fake, "chez-test.captain.food", "/cart", "fr", None)
                .await
                .expect("the cart route renders").html;
            assert!(
                !html.contains("data-error=\"true\""),
                "an ANSWERED read is never an error state ({answered}): {html}"
            );
            assert!(
                !html.contains("Impossible de charger votre panier"),
                "no error copy on an answered read ({answered}): {html}"
            );
        }
    }

    /// #730: the scalar mirror of the list rule above — an ANSWERED null restaurant (unknown
    /// slug) is the empty/shell state, never the error affordance. Only a read that FAILED may
    /// render `data-error`.
    #[cfg(feature = "ssr")]
    #[tokio::test]
    async fn an_answered_null_restaurant_is_the_empty_state_not_the_error_state() {
        use crate::graphql::test_support::FakeTransport;
        use serde_json::json;
        let fake = FakeTransport::scripted(vec![
            Ok(json!({ "restaurant": null })),
            Ok(json!({ "catalog": { "categories": [] } })),
        ]);
        let html = render_path_with(&fake, "chez-test.captain.food", "/r/chez-test", "fr", None)
            .await
            .expect("the storefront route renders")
            .html;
        assert!(
            !html.contains("data-error=\"true\""),
            "an answered null is never an error state: {html}"
        );
        assert!(
            !html.contains("Impossible de charger le contenu."),
            "no error copy on an answered read: {html}"
        );
    }

    #[cfg(feature = "ssr")]
    #[test]
    fn render_path_serves_every_surface_and_injects_the_hydrate_boot() {
        // The marketplace home, a storefront catalog page, the backoffice queue and a rider job:
        // all four surfaces serve HTML with the wasm boot script.
        for (host, path, marker) in [
            ("captain.food", "/", "data-hydrate=\"home\""),
            ("chez-test.captain.food", "/cart", "data-hydrate=\"cart\""),
            ("restos.captain.food", "/", "data-hydrate=\"orders_queue\""),
            ("riders.captain.food", "/jobs/x", "data-hydrate=\"job_detail\""),
        ] {
            let html = render_path(host, path, "fr").unwrap_or_else(|| panic!("{host}{path}"));
            assert!(html.contains(marker), "{host}{path}: {marker} missing");
            assert!(html.contains("/assets/web.js"), "{host}{path}: hydrate boot missing");
        }
        // The non-SDUI screens serve their hand-written shells.
        let checkout = render_path("chez-test.captain.food", "/checkout", "fr").unwrap();
        assert!(checkout.contains("data-hydrate=\"checkout\""));
        let tracking = render_path(
            "chez-test.captain.food",
            "/orders/00000000-0000-7000-8000-000000000000/confirmation",
            "fr",
        )
        .unwrap();
        assert!(tracking.contains("data-hydrate=\"order_tracking\""));
        // Unknown path → None (the server 404s).
        assert!(render_path("captain.food", "/nope", "fr").is_none());
    }
}
