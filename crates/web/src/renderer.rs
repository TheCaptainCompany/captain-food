//! The SDUI renderer (splits 1+4 of #21).
//!
//! Split 1 proved the registry-dispatch seam on one static screen; split 4 makes it GENERIC: the
//! renderer walks the GENERATED screen trees (`generated/screens.rs` — the DSL compiled to static
//! data) and renders REAL markup per [`ComponentKind`], resolving:
//!   * `PropValue::I18n(key)`   → the embedded translation catalog (`i18n`, fr default);
//!   * `PropValue::Binding(p)`  → the screen's resolved resolver data ([`RenderContext::data`]),
//!     via a dotted-path walk (`| filter` suffixes: `format_currency` on Money objects);
//!   * item-list kinds (`order_list`, `restaurant_card_grid/list`, `cart_lines`, …) → one card per
//!     row of the bound array.
//!
//! Markup depth is deliberately tiered: the load-bearing kinds (navigation chrome, lists/cards,
//! sections, text, buttons, inputs) have dedicated shapes; every other registered kind renders a
//! `data-c`-tagged container with its resolved text slots and children — visibly present,
//! auditable against the spec, restyled without re-architecture. Non-SDUI screens (`sdui: false`)
//! never reach this renderer: checkout.rs / tracking.rs own their markup.

use leptos::prelude::*;
use serde_json::{Map, Value};

use crate::generated::registry::ComponentKind;
use crate::generated::screens::{Node, PropValue, Screen};
use crate::i18n;

/// What a screen renders FROM: the resolver results keyed by BINDING NAME + the locale.
///
/// Binding names: each resolver result is stored under its dotted spec key (`orders.byRestaurant`)
/// AND its natural template aliases — the FIRST segment (`orders`) and, when the second segment is
/// a plain lowercase word, the reversed `second_first` form (`restaurants.featured` →
/// `featured_restaurants`) — matching how the DSL's `{{ … }}` templates name their data.
#[derive(Debug, Clone, Default)]
pub struct RenderContext {
    pub data: Map<String, Value>,
    pub locale: String,
}

impl RenderContext {
    pub fn new(locale: &str) -> Self {
        Self { data: Map::new(), locale: locale.to_string() }
    }

    /// Store one resolver result under its spec key + template aliases (see type docs).
    pub fn insert_resolved(&mut self, resolver_key: &str, value: Value) {
        let mut parts = resolver_key.splitn(2, '.');
        let first = parts.next().unwrap_or(resolver_key);
        if let Some(second) = parts.next() {
            if second.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                self.data.insert(format!("{second}_{first}"), value.clone());
            }
        }
        self.data.insert(first.to_string(), value.clone());
        self.data.insert(resolver_key.to_string(), value);
    }

    /// Resolve a `{{ path | filter }}` binding to display text ("" when absent — bindings are
    /// data slots, not errors).
    fn binding_text(&self, raw: &str) -> String {
        let mut parts = raw.split('|');
        let path = parts.next().unwrap_or("").trim();
        let filter = parts.next().map(str::trim);
        let value = self.lookup(path);
        match (value, filter) {
            (Some(v), Some("format_currency")) => format_currency(v),
            (Some(Value::String(s)), _) => s.clone(),
            (Some(Value::Number(n)), _) => n.to_string(),
            (Some(Value::Bool(b)), _) => b.to_string(),
            (Some(other), _) if !other.is_null() => format_currency(other), // Money-ish objects
            _ => String::new(),
        }
    }

    /// A binding's raw JSON value (filters stripped) — the ACTION-VARIABLE resolution path
    /// (`executor.rs`): `{{ order.id }}` must travel as the value, not display text.
    pub(crate) fn binding_json(&self, raw: &str) -> Option<Value> {
        self.lookup(raw.split('|').next().unwrap_or(raw).trim()).cloned()
    }

    /// Dotted-path walk into the data map (`order.status` → data["order"]["status"]).
    fn lookup(&self, path: &str) -> Option<&Value> {
        let mut segs = path.split('.');
        let mut cur = self.data.get(segs.next()?)?;
        for seg in segs {
            cur = cur.get(seg)?;
        }
        Some(cur)
    }
}

/// `{ amountCents, currency }` → "12,34 EUR" (fr-style decimal comma — V0 market). Non-Money
/// values render empty rather than lying.
fn format_currency(v: &Value) -> String {
    let (Some(cents), Some(cur)) = (
        v.get("amountCents").and_then(Value::as_i64),
        v.get("currency").and_then(Value::as_str),
    ) else {
        return String::new();
    };
    format!("{},{:02} {}", cents / 100, (cents % 100).abs(), cur)
}

/// Resolve any prop value to display text.
fn text_of(prop: PropValue, ctx: &RenderContext) -> String {
    match prop {
        PropValue::Text(s) => s.to_string(),
        PropValue::I18n(key) => i18n::resolve(key, &ctx.locale),
        PropValue::Binding(path) => ctx.binding_text(path),
    }
}

/// A node's prop as text, "" when absent.
fn prop_text(node: &Node, key: &str, ctx: &RenderContext) -> String {
    node.prop(key).map(|p| text_of(p, ctx)).unwrap_or_default()
}

/// The bound item array of a list-rendering node (`items: "{{ orders }}"`), empty when unresolved.
fn items_of(node: &Node, ctx: &RenderContext) -> Vec<Value> {
    match node.prop("items") {
        Some(PropValue::Binding(path)) => ctx
            .lookup(path.split('|').next().unwrap_or(path).trim())
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn children_views(node: &Node, ctx: &RenderContext) -> Vec<AnyView> {
    node.children.iter().map(|c| render_node(c, ctx)).collect()
}

/// One restaurant row/card (discovery lists) — the fields every Restaurant read carries.
fn restaurant_card(item: &Value) -> AnyView {
    let name = item.get("displayName").and_then(Value::as_str).unwrap_or("").to_string();
    let cuisine = item.get("cuisineCategory").and_then(Value::as_str).unwrap_or("").to_string();
    let city = item
        .get("address")
        .and_then(|a| a.get("city"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let rating = item.get("rating").map(|r| r.to_string()).unwrap_or_default();
    let slug = item.get("slug").and_then(Value::as_str).unwrap_or("").to_string();
    view! {
        <article data-c="restaurant_card" data-slug=slug>
            <h3>{name}</h3>
            <p>{cuisine}" - "{city}</p>
            <span data-c="rating_badge">{rating}</span>
        </article>
    }
    .into_any()
}

/// One order row/card (queues, history) — id/status/total, the triage essentials.
fn order_card(item: &Value) -> AnyView {
    let id = item.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    let id_attr = id.clone();
    let status = item.get("status").and_then(Value::as_str).unwrap_or("").to_string();
    let status_attr = status.clone();
    let total = item.get("totalAmount").map(format_currency).unwrap_or_default();
    view! {
        <article data-c="order_card" data-order=id_attr>
            <span data-c="status_chip" data-status=status_attr>{status}</span>
            <span>{id}</span>
            <strong>{total}</strong>
        </article>
    }
    .into_any()
}

/// One cart line row.
fn cart_line_row(item: &Value) -> AnyView {
    let name = item.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let qty = item.get("quantity").and_then(Value::as_i64).unwrap_or(0);
    let total = item.get("lineTotal").map(format_currency).unwrap_or_default();
    view! {
        <div data-c="cart_line_row">
            <span data-c="quantity_stepper">{qty.to_string()}</span>
            <span>{name}</span>
            <strong>{total}</strong>
        </div>
    }
    .into_any()
}

/// Render one generated node — the registry-dispatch heart of the renderer.
pub fn render_node(node: &Node, ctx: &RenderContext) -> AnyView {
    let ty = node.kind.as_str();
    match node.kind {
        // ── chrome ──────────────────────────────────────────────────────────────
        ComponentKind::StickyHeader => {
            view! { <header data-c=ty class="sticky">{children_views(node, ctx)}</header> }.into_any()
        }
        ComponentKind::PageHeader | ComponentKind::BackButtonHeader => {
            let title = prop_text(node, "title", ctx);
            view! { <header data-c=ty><h1>{title}</h1></header> }.into_any()
        }
        ComponentKind::BottomNavigation => {
            // items.N.{label,route,icon} — flattened props; walk indices until one is missing.
            let mut links: Vec<AnyView> = Vec::new();
            for i in 0..16 {
                let label = prop_text(node, &format!("items.{i}.label"), ctx);
                let route = prop_text(node, &format!("items.{i}.route"), ctx);
                if label.is_empty() && route.is_empty() {
                    break;
                }
                links.push(view! { <a href=route>{label}</a> }.into_any());
            }
            view! { <nav data-c=ty>{links}</nav> }.into_any()
        }
        ComponentKind::FloatingActionButton => {
            let label = prop_text(node, "label", ctx);
            view! { <button data-c=ty class="fab">{label}</button> }.into_any()
        }

        // ── layout ──────────────────────────────────────────────────────────────
        ComponentKind::Section | ComponentKind::CheckoutSection | ComponentKind::ConditionalSection => {
            let title = prop_text(node, "title", ctx);
            let has_title = !title.is_empty();
            view! {
                <section data-c=ty>
                    {has_title.then(|| view! { <h2>{title.clone()}</h2> })}
                    {children_views(node, ctx)}
                </section>
            }
            .into_any()
        }
        ComponentKind::StickyBottomBar => {
            view! { <footer data-c=ty>{children_views(node, ctx)}</footer> }.into_any()
        }
        ComponentKind::TabBar => {
            let mut tabs: Vec<AnyView> = Vec::new();
            for i in 0..12 {
                let label = prop_text(node, &format!("tabs.{i}.label"), ctx);
                if label.is_empty() {
                    break;
                }
                tabs.push(view! { <button role="tab">{label}</button> }.into_any());
            }
            view! { <nav data-c=ty role="tablist">{tabs}</nav> }.into_any()
        }
        ComponentKind::HorizontalScroll | ComponentKind::Row | ComponentKind::Column => {
            view! { <div data-c=ty>{children_views(node, ctx)}</div> }.into_any()
        }

        // ── sheets & overlays (#94) ─────────────────────────────────────────────
        ComponentKind::BottomSheet => {
            // Rendered HIDDEN; `open_bottom_sheet` (interact.rs) toggles by `data-sheet-id`.
            let sheet_id = prop_text(node, "id", ctx);
            let title = prop_text(node, "title", ctx);
            let has_title = !title.is_empty();
            view! {
                <section data-c=ty data-sheet-id=sheet_id hidden=true>
                    {has_title.then(|| view! { <h2>{title.clone()}</h2> })}
                    {children_views(node, ctx)}
                </section>
            }
            .into_any()
        }
        ComponentKind::List => {
            // The generic titled list (location picker's address lists): rows from the bound items.
            let title = prop_text(node, "title", ctx);
            let rows: Vec<AnyView> = items_of(node, ctx)
                .iter()
                .map(|item| {
                    let line = item
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| item.get("line1").and_then(Value::as_str).unwrap_or("").to_string());
                    view! { <li>{line}</li> }.into_any()
                })
                .collect();
            view! { <div data-c=ty><h3>{title}</h3><ul>{rows}</ul></div> }.into_any()
        }

        // ── content ─────────────────────────────────────────────────────────────
        ComponentKind::Text => {
            let value = prop_text(node, "value", ctx);
            view! { <p data-c=ty>{value}</p> }.into_any()
        }
        ComponentKind::Image | ComponentKind::HeroImage | ComponentKind::Logo => {
            let src = {
                let asset = prop_text(node, "asset", ctx);
                if asset.is_empty() { prop_text(node, "src", ctx) } else { asset }
            };
            view! { <img data-c=ty src=src alt=""/> }.into_any()
        }
        ComponentKind::CtaBanner | ComponentKind::CtaSection => {
            let title = prop_text(node, "title", ctx);
            let button = {
                let b = prop_text(node, "button_label", ctx);
                if b.is_empty() { prop_text(node, "cta_label", ctx) } else { b }
            };
            let has_button = !button.is_empty();
            view! {
                <aside data-c=ty class="cta">
                    {title}
                    {has_button.then(|| view! { <button>{button.clone()}</button> })}
                    {children_views(node, ctx)}
                </aside>
            }
            .into_any()
        }
        ComponentKind::ValueProps => {
            let mut items: Vec<AnyView> = Vec::new();
            for i in 0..12 {
                let title = prop_text(node, &format!("items.{i}.title"), ctx);
                if title.is_empty() {
                    break;
                }
                let body = prop_text(node, &format!("items.{i}.body"), ctx);
                items.push(view! { <li><strong>{title}</strong><p>{body}</p></li> }.into_any());
            }
            view! { <ul data-c=ty>{items}</ul> }.into_any()
        }
        ComponentKind::InfoRow | ComponentKind::OpeningHoursRow => {
            let label = prop_text(node, "label", ctx);
            let value = prop_text(node, "value", ctx);
            view! { <div data-c=ty><span>{label}</span><span>{value}</span></div> }.into_any()
        }

        // ── discovery lists ─────────────────────────────────────────────────────
        ComponentKind::RestaurantCardGrid | ComponentKind::RestaurantCardList | ComponentKind::SearchResults => {
            let cards: Vec<AnyView> = items_of(node, ctx).iter().map(restaurant_card).collect();
            view! { <div data-c=ty>{cards}</div> }.into_any()
        }

        // ── order lists ─────────────────────────────────────────────────────────
        ComponentKind::OrderList => {
            let items = items_of(node, ctx);
            if items.is_empty() {
                let title = prop_text(node, "empty_state.title", ctx);
                let body = prop_text(node, "empty_state.body", ctx);
                view! { <div data-c=ty data-empty="true"><h3>{title}</h3><p>{body}</p></div> }.into_any()
            } else {
                let cards: Vec<AnyView> = items.iter().map(order_card).collect();
                view! { <div data-c=ty>{cards}</div> }.into_any()
            }
        }

        // ── cart ────────────────────────────────────────────────────────────────
        ComponentKind::CartLines => {
            let rows: Vec<AnyView> = match node.prop("lines") {
                Some(PropValue::Binding(path)) => ctx
                    .lookup(path.trim())
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(cart_line_row).collect())
                    .unwrap_or_default(),
                _ => items_of(node, ctx).iter().map(cart_line_row).collect(),
            };
            view! { <div data-c=ty>{rows}</div> }.into_any()
        }
        ComponentKind::CartSummaryMini | ComponentKind::OrderSummaryBlock => {
            let total = prop_text(node, "total", ctx);
            view! { <div data-c=ty><strong>{total}</strong>{children_views(node, ctx)}</div> }.into_any()
        }

        // ── inputs ──────────────────────────────────────────────────────────────
        ComponentKind::Button | ComponentKind::TextButton | ComponentKind::IconButton | ComponentKind::SignOutButton | ComponentKind::AddButton => {
            let label = prop_text(node, "label", ctx);
            let variant = prop_text(node, "variant", ctx);
            // The action DOM contract (#93): the button's parsed plan travels as data attributes
            // (key + render-time-resolved variables + loading label + on-success route), so the
            // SSR'd and hydrated DOM are identical and ONE delegated listener (`interact.rs`)
            // drives every button. A gap/unwired action renders disabled with its reason.
            let (action_attrs, disabled_reason) = crate::executor::button_attrs(node, ctx);
            let get = |k: &str| {
                action_attrs.iter().find(|(a, _)| *a == k).map(|(_, v)| v.clone())
            };
            use crate::executor::attrs;
            let disabled = disabled_reason.is_some();
            view! {
                <button
                    data-c=ty
                    data-variant=variant
                    data-action=get(attrs::ACTION)
                    data-vars=get(attrs::VARS)
                    data-var-bindings=get(attrs::VAR_BINDINGS)
                    data-loading=get(attrs::LOADING)
                    data-on-success=get(attrs::ON_SUCCESS)
                    data-route=get(attrs::ROUTE)
                    data-sheet=get(attrs::SHEET)
                    data-number=get(attrs::NUMBER)
                    disabled=disabled
                    title=disabled_reason
                >
                    {label}
                </button>
            }
            .into_any()
        }
        ComponentKind::TextInput | ComponentKind::PhoneInput | ComponentKind::EmailInput | ComponentKind::SearchInput | ComponentKind::PhoneField | ComponentKind::OtpInput => {
            let label = prop_text(node, "label", ctx);
            let placeholder = prop_text(node, "placeholder", ctx);
            // The field id is the `{{ <id>.value }}` binding target (#94) — the driver reads the
            // live value by this id at dispatch time, so it must land on the <input> itself.
            let field_id = prop_text(node, "id", ctx);
            view! { <label data-c=ty>{label}<input id=field_id placeholder=placeholder/></label> }.into_any()
        }
        ComponentKind::StatusChip => {
            let status = prop_text(node, "status", ctx);
            let status_attr = status.clone();
            view! { <span data-c=ty data-status=status_attr>{status}</span> }.into_any()
        }

        // ── account ─────────────────────────────────────────────────────────────
        ComponentKind::MenuSection => {
            let title = prop_text(node, "title", ctx);
            let mut items: Vec<AnyView> = Vec::new();
            for i in 0..16 {
                let label = prop_text(node, &format!("items.{i}.label"), ctx);
                if label.is_empty() {
                    break;
                }
                let route = prop_text(node, &format!("items.{i}.route"), ctx);
                items.push(view! { <li><a href=route>{label}</a></li> }.into_any());
            }
            view! { <section data-c=ty><h2>{title}</h2><ul>{items}</ul></section> }.into_any()
        }

        // ── everything else: the tagged generic container (visible + auditable) ─
        _ => {
            let text = {
                let t = prop_text(node, "title", ctx);
                if !t.is_empty() {
                    t
                } else {
                    let l = prop_text(node, "label", ctx);
                    if !l.is_empty() { l } else { prop_text(node, "value", ctx) }
                }
            };
            let group = format!("{:?}", node.kind.group());
            view! { <div data-c=ty data-group=group>{text}{children_views(node, ctx)}</div> }.into_any()
        }
    }
}

/// A whole SDUI screen as a Leptos view: the screen tree + the surface's bottom sheets (#94),
/// mounted HIDDEN after the content (`open_bottom_sheet` toggles them by id at runtime).
#[component]
pub fn SduiScreen(
    screen: &'static Screen,
    sheets: &'static [crate::generated::screens::Sheet],
    ctx: RenderContext,
) -> impl IntoView {
    let nodes: Vec<AnyView> = screen.tree.iter().map(|n| render_node(n, &ctx)).collect();
    let sheet_views: Vec<AnyView> = sheets.iter().map(|s| render_node(&s.node, &ctx)).collect();
    view! {
        <main id="app" data-hydrate=screen.id>
            {nodes}
            {sheet_views}
        </main>
    }
}

/// Wrap a rendered screen body in the shared HTML document shell (the `ssr` build). One shell for
/// every server-rendered page — SDUI screens here, checkout/tracking in their own modules.
/// `hydrate_script` (the wasm bundle loader) is appended when serving with assets.
#[cfg(feature = "ssr")]
pub(crate) fn page_html(title: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{title}</title></head><body>{body}</body></html>"
    )
}

/// Server-side render one SDUI screen (+ its surface's sheets) to a full document.
#[cfg(feature = "ssr")]
pub fn render_screen_html(
    screen: &'static Screen,
    sheets: &'static [crate::generated::screens::Sheet],
    ctx: RenderContext,
) -> String {
    let body = SduiScreen(SduiScreenProps { screen, sheets, ctx }).to_html();
    page_html("Captain.Food", &body)
}

/// Client hydration entry (the `hydrate` build, wasm32): resolve the surface + screen from the
/// browser location, mount, then fetch the screen's `data_requirements` and re-render with live
/// data (SSR ships the shell; the client owns freshness — the split-4 serving model).
#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    use crate::router;
    let window = web_sys::window().expect("browser window");
    let location = window.location();
    let host = location.host().unwrap_or_default();
    let path = location.pathname().unwrap_or_else(|_| "/".into());
    // Shared host+path resolution incl. the tenant-root rule (#98) — same authority as SSR.
    let (surface, matched) = router::resolve(&host, &path);
    let Some(matched) = matched else { return };
    let screen: &'static Screen = matched.screen;
    if !screen.sdui {
        // checkout / order_tracking: their hand-written flows own hydration (split 3 modules).
        return;
    }

    let session = crate::session::SessionId::load_or_mint();
    let origin = location.origin().unwrap_or_default();
    let transport = crate::graphql::HttpTransport::new(&origin, surface.role(), session);

    // The interaction layer (#93): delegated button dispatch + push socket + boot pending-resume.
    crate::interact::install(&origin, surface.role(), session);

    let sheets = surface.sheets();
    wasm_bindgen_futures::spawn_local(async move {
        let mut ctx = RenderContext::new(i18n::DEFAULT_LOCALE);
        for resolver in screen.data_requirements {
            let mut vars = serde_json::Map::new();
            for (k, v) in matched.param_args(*resolver) {
                vars.insert(k, v);
            }
            if let Ok(value) = crate::graphql::execute_resolver(&transport, *resolver, vars).await {
                ctx.insert_resolved(resolver.as_str(), value);
            }
        }
        leptos::mount::mount_to_body(move || SduiScreen(SduiScreenProps { screen, sheets, ctx }));
    });
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;
    use crate::router::Surface;
    use serde_json::json;

    fn ctx() -> RenderContext {
        RenderContext::new("en")
    }

    #[test]
    fn every_sdui_screen_of_every_surface_renders() {
        // The whole generated surface area renders without panicking, empty data included —
        // the "no placeholder left behind a reachable route" gate at the smoke level.
        for surface in [
            Surface::CaptainFrontoffice,
            Surface::RestaurantFrontoffice,
            Surface::RestaurantBackoffice,
            Surface::Rider,
        ] {
            for screen in surface.screens() {
                if !screen.sdui {
                    continue;
                }
                let html = render_screen_html(screen, surface.sheets(), ctx());
                assert!(
                    html.contains(&format!("data-hydrate=\"{}\"", screen.id)),
                    "{}: no hydrate root",
                    screen.id
                );
            }
        }
    }

    #[test]
    fn i18n_props_render_real_strings() {
        // The backoffice queue renders localized text from the merged catalog.
        let screen = Surface::RestaurantBackoffice
            .screens()
            .iter()
            .find(|s| s.id == "orders_queue")
            .unwrap();
        let fr = render_screen_html(screen, Surface::RestaurantBackoffice.sheets(), RenderContext::new("fr"));
        assert!(fr.contains("File des commandes"), "fr title missing");
        assert!(fr.contains("Accepter"), "fr accept button missing");
        let en = render_screen_html(screen, Surface::RestaurantBackoffice.sheets(), RenderContext::new("en"));
        assert!(en.contains("Order queue"), "en title missing");
    }

    #[test]
    fn bindings_render_lists_from_resolved_data() {
        let screen = Surface::RestaurantBackoffice
            .screens()
            .iter()
            .find(|s| s.id == "orders_queue")
            .unwrap();
        let mut c = ctx();
        c.insert_resolved(
            "orders.byRestaurant",
            json!([
                { "id": "o-1", "status": "PLACED", "totalAmount": { "amountCents": 2350, "currency": "EUR" } },
                { "id": "o-2", "status": "ACCEPTED", "totalAmount": { "amountCents": 980, "currency": "EUR" } },
            ]),
        );
        let html = render_screen_html(screen, Surface::RestaurantBackoffice.sheets(), c);
        assert!(html.contains("data-order=\"o-1\""), "{html}");
        assert!(html.contains("23,50 EUR"));
        assert!(html.contains("data-status=\"ACCEPTED\""));

        // Empty data → the spec's empty state, not a blank div.
        let html = render_screen_html(screen, Surface::RestaurantBackoffice.sheets(), ctx());
        assert!(html.contains("data-empty=\"true\""));
    }

    #[test]
    fn resolver_alias_convention_feeds_the_marketplace_rails() {
        // restaurants.featured → alias featured_restaurants (the template name on home).
        let mut c = ctx();
        c.insert_resolved(
            "restaurants.featured",
            json!([{ "displayName": "Chez Test", "slug": "chez-test", "address": { "city": "Tours" } }]),
        );
        assert!(c.data.contains_key("featured_restaurants"));
        assert!(c.data.contains_key("restaurants"));
        let home = Surface::CaptainFrontoffice.screens().iter().find(|s| s.id == "home").unwrap();
        let html = render_screen_html(home, Surface::CaptainFrontoffice.sheets(), c);
        assert!(html.contains("Chez Test"), "{html}");
        assert!(html.contains("data-slug=\"chez-test\""));
    }

    #[test]
    fn buttons_stamp_the_action_dom_contract_in_ssr_html() {
        // The backoffice accept button carries its key + render-time-resolved variables (#93) —
        // the SSR'd DOM is everything the delegated click driver needs.
        let screen = Surface::RestaurantBackoffice
            .screens()
            .iter()
            .find(|s| s.id == "orders_queue")
            .unwrap();
        let mut c = ctx();
        c.insert_resolved("order", json!({ "id": "o-1" }));
        c.insert_resolved("restaurant", json!({ "id": "r-1" }));
        let html = render_screen_html(screen, Surface::RestaurantBackoffice.sheets(), c);
        assert!(html.contains("data-action=\"accept_order\""), "{html}");
        assert!(html.contains("&quot;orderId&quot;:&quot;o-1&quot;"), "resolved vars JSON: {html}");

        // The rider gap toggle renders DISABLED with the spec's note as its tooltip.
        let jobs = Surface::Rider.screens().iter().find(|s| s.id == "jobs").unwrap();
        let html = render_tracking_like(jobs);
        assert!(html.contains("disabled"), "{html}");
        assert!(html.contains("No rider availability mutation"), "{html}");
    }

    fn render_tracking_like(screen: &'static crate::generated::screens::Screen) -> String {
        render_screen_html(screen, &[], ctx())
    }

    #[test]
    fn sheets_render_hidden_into_every_storefront_screen() {
        // #94: the surface's bottom sheets mount HIDDEN after the content; open_bottom_sheet
        // toggles them by data-sheet-id at runtime.
        let cart = Surface::RestaurantFrontoffice.screens().iter().find(|s| s.id == "cart").unwrap();
        let html = render_screen_html(cart, Surface::RestaurantFrontoffice.sheets(), RenderContext::new("fr"));
        for sheet in ["location_picker", "auth_sheet", "otp_sheet", "item_detail_sheet", "rating_sheet"] {
            assert!(html.contains(&format!("data-sheet-id=\"{sheet}\"")), "missing {sheet}");
        }
        assert!(html.contains("hidden"), "sheets must render hidden");
        // Real strings from the merged catalog, and the send_otp button's dispatch attributes.
        assert!(html.contains("Se connecter ou créer un compte"), "auth title fr");
        assert!(html.contains("data-action=\"send_otp\""), "{html}");
        assert!(html.contains("phone_field.value"), "the form-field binding travels: {html}");
        // The field itself carries the id the binding targets.
        assert!(html.contains("id=\"phone_field\""), "{html}");
    }

    #[test]
    fn money_formats_fr_style() {
        assert_eq!(format_currency(&json!({ "amountCents": 980, "currency": "EUR" })), "9,80 EUR");
        assert_eq!(format_currency(&json!({ "amountCents": 2305, "currency": "EUR" })), "23,05 EUR");
        assert_eq!(format_currency(&json!("not money")), "");
    }

    #[test]
    fn registry_allowlist_round_trips() {
        for kind in ComponentKind::ALL {
            assert_eq!(ComponentKind::from_type(kind.as_str()), Some(*kind));
        }
        assert_eq!(ComponentKind::from_type("not_a_component"), None);
    }
}
