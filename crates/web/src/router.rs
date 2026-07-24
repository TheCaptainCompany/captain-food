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
    /// Excludes every ADR-0036 reserved audience label (`live`/`restos`/`riders`/`system`/`api`)
    /// and the off-server marketing hosts (`www`/`join`).
    pub fn slug_of(host: &str) -> Option<&str> {
        let host = host.split(':').next().unwrap_or(host);
        let label = host.strip_suffix(".captain.food")?;
        (!label.contains('.')
            && !matches!(label, "www" | "join" | "live" | "restos" | "riders" | "system" | "api"))
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

/// Server-side render the page for `host` + `path` — the BFF's serving entry (split 4/4).
/// SDUI screens render their generated tree (empty data: SSR ships the SHELL, the hydrate bundle
/// fetches the screen's `data_requirements` — the split-4 serving model); the non-SDUI screens
/// render their hand-written shells (checkout / order tracking). `None` = no such route (404).
#[cfg(feature = "ssr")]
pub fn render_path(host: &str, path: &str, locale: &str) -> Option<String> {
    use crate::renderer::{render_screen_html, RenderContext};
    let (surface, matched) = resolve(host, path);
    let matched = matched?;
    let html = if matched.screen.sdui {
        render_screen_html(matched.screen, surface.sheets(), RenderContext::new(locale))
    } else {
        match matched.screen.id {
            "checkout" => crate::checkout::render_checkout_html(crate::checkout::CheckoutViewState {
                restaurant_name: String::new(),
                cart_line_count: 0,
                formatted_total: String::new(),
                is_delivery: true,
            }),
            "order_tracking" => {
                let order_id = matched
                    .param("orderId")
                    .and_then(|v| uuid::Uuid::parse_str(v).ok())
                    .unwrap_or_else(uuid::Uuid::nil);
                crate::tracking::render_tracking_html(crate::tracking::TrackingState::new(order_id))
            }
            // A future sdui:false screen without a hand-written shell: an empty SDUI shell.
            _ => render_screen_html(matched.screen, surface.sheets(), RenderContext::new(locale)),
        }
    };
    Some(html.replace("</body>", &format!("{HYDRATE_SCRIPT}</body>")))
}

#[cfg(test)]
mod tests {
    use super::*;

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
