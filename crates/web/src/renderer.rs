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

use std::collections::BTreeSet;

use leptos::prelude::*;
use serde_json::{Map, Value};

use crate::condition::Condition;
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
    /// Resolver reads that FAILED for real (#472) — transport/contract failures on a read this
    /// role path is allowed to ask, keyed like `data` (spec key + aliases). NOT the skip-by-design
    /// outcomes (declared gaps, role-refused reads on the anonymous SSR path), which leave no
    /// trace here: a failed binding renders its ERROR state, a skipped one its empty/shell state,
    /// and conflating the two is exactly the "Commande introuvable over a transient failure"
    /// defect this field exists to prevent.
    failed: BTreeSet<String>,
    pub locale: String,
    /// The Stripe publishable TEST key the server was configured with, already PARSED (#440):
    /// `None` = absent/empty/malformed = the checkout shell renders its degraded state. Carried on
    /// the render context (not resolver data, not a window global, not GraphQL) because it is a
    /// server-side deployment fact the page needs at render time — the same seam a future
    /// runtime fact would ride. Only the checkout screen reads it today.
    pub stripe_publishable_key: Option<crate::stripe::PublishableKey>,
}

impl RenderContext {
    pub fn new(locale: &str) -> Self {
        Self {
            data: Map::new(),
            failed: BTreeSet::new(),
            locale: locale.to_string(),
            stripe_publishable_key: None,
        }
    }

    /// Record one resolver read as FAILED (#472), under the same aliases `insert_resolved` uses.
    pub fn insert_failed(&mut self, resolver_key: &str) {
        let mut parts = resolver_key.splitn(2, '.');
        let first = parts.next().unwrap_or(resolver_key);
        if let Some(second) = parts.next() {
            if second.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                self.failed.insert(format!("{second}_{first}"));
            }
        }
        self.failed.insert(first.to_string());
        self.failed.insert(resolver_key.to_string());
    }

    /// Whether the resolver feeding this `{{ path }}` binding failed for real (#472) — matched on
    /// the binding's ROOT segment, the name resolver results are stored under.
    pub fn binding_failed(&self, raw: &str) -> bool {
        let path = raw.split('|').next().unwrap_or(raw).trim();
        let root = path.split('.').next().unwrap_or(path);
        self.failed.contains(root) || self.failed.contains(path)
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
/// values render empty rather than lying. `pub(crate)` since #420: the hand-written checkout shell
/// formats its cart total through the SAME function the SDUI money bindings use, so the price a
/// customer confirms cannot be formatted one way on one screen and another way on the next.
pub(crate) fn format_currency(v: &Value) -> String {
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

/// Whether the item-list binding this node renders from FAILED for real (#472) — checked on the
/// `items` prop (and `cart_lines`' alternative `lines` prop).
fn items_binding_failed(node: &Node, ctx: &RenderContext) -> bool {
    ["items", "lines"].iter().any(|key| match node.prop(key) {
        Some(PropValue::Binding(path)) => ctx.binding_failed(path),
        _ => false,
    })
}

/// The per-binding ERROR state (#472, graphql-architect blocking finding): a read that FAILED
/// renders this — DISTINCT from the empty state a read that ANSWERED empty renders. The copy is
/// translation-keyed (never the transport string — that is server internals), and the state is
/// STATIC: retry is the user's act (a same-URL anchor reload), never an auto-refetch loop
/// (ADR-20260810-231300). Ugly-but-visible ships (holub).
fn binding_error_state(ty: &str, copy_key: &str, ctx: &RenderContext) -> AnyView {
    let message = i18n::resolve(copy_key, &ctx.locale);
    let retry = i18n::resolve("common.error.retry", &ctx.locale);
    let ty = ty.to_string();
    view! {
        <div data-c=ty data-error="true">
            <p>{message}</p>
            <a href="" data-c="retry_button" role="button">{retry}</a>
        </div>
    }
    .into_any()
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

/// One conversation message bubble (#145) — body + author role + timestamp, with a small
/// "translated" hint when the message carries cached translations. `translated_label` is the
/// screen-resolved (localized) hint text, shown only on bubbles that have translations.
fn message_bubble_row(item: &Value, translated_label: &str) -> AnyView {
    let body = item.get("body").and_then(Value::as_str).unwrap_or("").to_string();
    let role = item.get("authorRole").and_then(Value::as_str).unwrap_or("").to_string();
    let role_attr = role.clone();
    let posted = item.get("postedAt").and_then(Value::as_str).unwrap_or("").to_string();
    let has_translations =
        item.get("translations").and_then(Value::as_array).map(|a| !a.is_empty()).unwrap_or(false);
    let hint = translated_label.to_string();
    let show_hint = has_translations && !hint.is_empty();
    view! {
        <article data-c="message_bubble_row" data-role=role_attr>
            <span data-c="author">{role}</span>
            <p>{body}</p>
            <time>{posted}</time>
            {show_hint.then(|| view! { <span data-c="translated">{hint.clone()}</span> })}
        </article>
    }
    .into_any()
}

/// Evaluate a node's condition prop over the resolved data. `None` = the node declares none.
/// `Some(Ok(bool))` = evaluated (missing data already failed CLOSED per `polarity_on_unknown`);
/// `Some(Err(expr))` = the expression is outside the corpus grammar — the caller must be LOUD.
fn eval_condition_prop(
    node: &Node,
    key: &str,
    ctx: &RenderContext,
    on_unevaluatable: bool,
) -> Option<Result<bool, &'static str>> {
    let prop = node.prop(key)?;
    let PropValue::Text(expr) = prop else {
        // A binding/i18n value in a condition slot is invisible to the grammar — loud, like any
        // unknown construct (the corpus gate keeps checked-in specs out of this branch).
        return Some(Err("<non-literal condition>"));
    };
    Some(match Condition::parse(expr) {
        Err(_) => Err(expr),
        Ok(c) => Ok(c
            .eval(&|path| ctx.lookup(path).cloned())
            // Fail CLOSED over missing data (#472 briefing): hidden for visible_when
            // (on_unevaluatable = false), disabled for disabled_when (on_unevaluatable = true).
            .unwrap_or(on_unevaluatable)),
    })
}

/// The LOUD fail-closed render of an unparseable condition (#472): the content never renders
/// (never silently-true), and an auditable — but invisible — marker records the expression for
/// review tooling and tests. The SSR boundary counts it (`sdui_degraded_render_total{reason=
/// condition_unparseable}`, emitted server-side from the static pre-scan, not from here — the
/// renderer compiles to wasm and stays telemetry-free).
fn condition_error_marker(expr: &str) -> AnyView {
    let expr = expr.to_string();
    view! { <span data-condition-error=expr hidden=true></span> }.into_any()
}

/// Render one generated node: the `visible_when` choke point, then registry dispatch. EVERY
/// render path goes through here (screen roots, children, sheets), so a declared condition cannot
/// be skipped by construction (#472).
///
/// LEGAL INVARIANT (#472 checkpoint, legal lens): a legally required element — an allergen
/// declaration (EU FIC 1169/2011 distance selling), the total price, pre-contractual information —
/// must NEVER be bound behind a failable condition. Fail-closed hiding is correct for UX
/// affordances (a resend button, a cart FAB) and WRONG for mandatory information: a missing
/// binding would silently remove a disclosure the law requires at the moment of ordering. Put
/// mandatory information OUTSIDE conditions; if it ever needs one, that is a legal-surface change
/// (`HOLD: human` class), not a screen tweak.
pub fn render_node(node: &Node, ctx: &RenderContext) -> AnyView {
    match eval_condition_prop(node, "visible_when", ctx, false) {
        Some(Err(expr)) => return condition_error_marker(expr),
        Some(Ok(false)) => return ().into_any(), // hidden: absent, no DOM node at all
        Some(Ok(true)) | None => {}
    }
    render_node_kind(node, ctx)
}

/// Registry dispatch for a node whose visibility gate already passed.
fn render_node_kind(node: &Node, ctx: &RenderContext) -> AnyView {
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
            // `conditional_section` spells its predicate TWO ways in the spec today (#472):
            // `visible_when` (handled by the render_node choke point like every node) and
            // `condition:` with if_true/if_false branches. Both route through the ONE evaluator.
            // The branches arrive FLATTENED from the generator (`if_true.0.type`, …) — not
            // renderable Nodes — so the honest render is the evaluated verdict stamped on the
            // section (`data-cond`), with branch rendering left to the emitter follow-up noted on
            // the PR. An unparseable `condition:` is loud, like any unknown construct.
            match eval_condition_prop(node, "condition", ctx, false) {
                Some(Err(expr)) => return condition_error_marker(expr),
                verdict => {
                    let cond = verdict.map(|v| {
                        if v.unwrap_or(false) { "true".to_string() } else { "false".to_string() }
                    });
                    let title = prop_text(node, "title", ctx);
                    let has_title = !title.is_empty();
                    return view! {
                        <section data-c=ty data-cond=cond>
                            {has_title.then(|| view! { <h2>{title.clone()}</h2> })}
                            {children_views(node, ctx)}
                        </section>
                    }
                    .into_any();
                }
            }
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
            if items_binding_failed(node, ctx) {
                return binding_error_state(ty, "common.error.data_unavailable", ctx);
            }
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
            if items_binding_failed(node, ctx) {
                return binding_error_state(ty, "common.error.data_unavailable", ctx);
            }
            let cards: Vec<AnyView> = items_of(node, ctx).iter().map(restaurant_card).collect();
            view! { <div data-c=ty>{cards}</div> }.into_any()
        }

        // ── order lists ─────────────────────────────────────────────────────────
        ComponentKind::OrderList => {
            if items_binding_failed(node, ctx) {
                // A failed read is NEVER the empty state: "no orders" over a transient failure is
                // a lie to both sides of the marketplace.
                return binding_error_state(ty, "common.error.data_unavailable", ctx);
            }
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

        // ── status-scoped card treatment (#167) ────────────────────────────────
        ComponentKind::OrderCardStatus => {
            // A PER-CARD treatment, never an unconditional banner (the PR #586 ux STOP: the
            // generic fallback rendered "Expired — no response" on every board load with zero
            // timed-out orders). One treatment card per bound order actually holding the
            // declared `status`; NOTHING when no bound order holds it.
            let status = prop_text(node, "status", ctx);
            let icon = prop_text(node, "icon", ctx);
            let title = prop_text(node, "title", ctx);
            let body = prop_text(node, "body", ctx);
            let cards: Vec<AnyView> = items_of(node, ctx)
                .iter()
                .filter(|item| {
                    item.get("status").and_then(Value::as_str) == Some(status.as_str())
                })
                .map(|item| {
                    let id = item.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                    view! {
                        <article data-c=ty data-order=id data-status=status.clone() data-icon=icon.clone()>
                            <h3>{title.clone()}</h3>
                            <p>{body.clone()}</p>
                        </article>
                    }
                    .into_any()
                })
                .collect();
            if cards.is_empty() {
                // Absent, not hidden: no DOM node at all when no order holds the status.
                ().into_any()
            } else {
                cards.into_any()
            }
        }

        // ── conversation (#145) ───────────────────────────────────────────────────
        ComponentKind::MessageBubble => {
            // The order chat timeline: one bubble per message (mirrors OrderList → order_card),
            // its empty state when the thread has no messages yet.
            if items_binding_failed(node, ctx) {
                return binding_error_state(ty, "common.error.data_unavailable", ctx);
            }
            let items = items_of(node, ctx);
            if items.is_empty() {
                let title = prop_text(node, "empty_state.title", ctx);
                let body = prop_text(node, "empty_state.body", ctx);
                view! { <div data-c=ty data-empty="true"><h3>{title}</h3><p>{body}</p></div> }.into_any()
            } else {
                let translated_label = prop_text(node, "translated_label", ctx);
                let bubbles: Vec<AnyView> =
                    items.iter().map(|item| message_bubble_row(item, &translated_label)).collect();
                view! { <div data-c=ty>{bubbles}</div> }.into_any()
            }
        }
        ComponentKind::QuickReplyChips => {
            // Static quick-reply chips (chips.N.label — flattened props, walked like bottom_nav).
            let mut chips: Vec<AnyView> = Vec::new();
            for i in 0..12 {
                let label = prop_text(node, &format!("chips.{i}.label"), ctx);
                if label.is_empty() {
                    break;
                }
                chips.push(view! { <button type="button" data-c="quick_reply_chip">{label}</button> }.into_any());
            }
            view! { <div data-c=ty>{chips}</div> }.into_any()
        }

        // ── cart ────────────────────────────────────────────────────────────────
        ComponentKind::CartLines => {
            if items_binding_failed(node, ctx) {
                // The cart's own copy (ux, #472): never "your cart is empty" over a failed read.
                return binding_error_state(ty, "cart.error.load", ctx);
            }
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
            // #472: `disabled_when` through the one evaluator. Fail CLOSED: unevaluatable (missing
            // data) disables; an unparseable expression disables AND stamps the loud marker — in
            // SSR HTML the `disabled` attribute IS the behaviour (the delegated driver never
            // fires on a disabled control).
            let (condition_disabled, condition_error) =
                match eval_condition_prop(node, "disabled_when", ctx, true) {
                    None => (false, None),
                    Some(Ok(b)) => (b, None),
                    Some(Err(expr)) => (true, Some(expr.to_string())),
                };
            let disabled = disabled_reason.is_some() || condition_disabled;
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
                    data-condition-error=condition_error
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
            // otp_input auto-submits on `on_complete` when it reaches `length` digits (#114): the
            // trigger action rides `input` (a keystroke), so it lands on the <input> element.
            let (trig, _) = crate::executor::trigger_attrs(node, ctx, "on_complete", "complete");
            let g = |k: &str| trig.iter().find(|(a, _)| *a == k).map(|(_, v)| v.clone());
            use crate::executor::attrs;
            let len = prop_text(node, "length", ctx);
            view! {
                <label data-c=ty>
                    {label}
                    <input
                        id=field_id
                        placeholder=placeholder
                        data-action=g(attrs::ACTION)
                        data-vars=g(attrs::VARS)
                        data-var-bindings=g(attrs::VAR_BINDINGS)
                        data-trigger=g(attrs::TRIGGER)
                        data-complete-len={if len.is_empty() { None } else { Some(len) }}
                        data-on-success=g(attrs::ON_SUCCESS)
                    />
                </label>
            }
            .into_any()
        }
        ComponentKind::StatusChip => {
            let status = prop_text(node, "status", ctx);
            let status_attr = status.clone();
            view! { <span data-c=ty data-status=status_attr>{status}</span> }.into_any()
        }

        // ── account ─────────────────────────────────────────────────────────────
        // A single/multi-select chip group (#114): each option is a chip carrying its value; the
        // group's `on_change` fires when a chip is picked. The selected value lands in a hidden
        // input (id = the group's field id), so the driver's existing form-field binding fill
        // (`{{ <field>.value }}`) reads it with zero new resolution. This is what finally fires the
        // #62 delivery-satisfaction survey from the UI (the timeliness chips carry
        // `record_delivery_satisfaction`).
        ComponentKind::ChipMultiSelect => {
            let field_id = prop_text(node, "id", ctx);
            let label = prop_text(node, "label", ctx);
            let has_label = !label.is_empty();
            let (trig, _) = crate::executor::trigger_attrs(node, ctx, "on_change", "change");
            let g = |k: &str| trig.iter().find(|(a, _)| *a == k).map(|(_, v)| v.clone());
            use crate::executor::attrs;
            // Options flatten to options.N.value / options.N.label (or a bare translation ref).
            let mut chips: Vec<AnyView> = Vec::new();
            for i in 0..16 {
                let value = prop_text(node, &format!("options.{i}.value"), ctx);
                let opt_label = {
                    let l = prop_text(node, &format!("options.{i}.label"), ctx);
                    if l.is_empty() { prop_text(node, &format!("options.{i}"), ctx) } else { l }
                };
                if value.is_empty() && opt_label.is_empty() {
                    break;
                }
                let chip_value = if value.is_empty() { opt_label.clone() } else { value };
                chips.push(view! {
                    <button type="button" data-c="chip" data-chip-value=chip_value data-chip-group=field_id.clone()>
                        {opt_label}
                    </button>
                }.into_any());
            }
            view! {
                <fieldset
                    data-c=ty
                    data-action=g(attrs::ACTION)
                    data-vars=g(attrs::VARS)
                    data-var-bindings=g(attrs::VAR_BINDINGS)
                    data-trigger=g(attrs::TRIGGER)
                >
                    {has_label.then(|| view! { <legend>{label.clone()}</legend> })}
                    <input type="hidden" id=field_id/>
                    {chips}
                </fieldset>
            }
            .into_any()
        }
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

/// The design system (#115), INLINED into every SSR page's `<head>`: the generated token variables
/// (`tokens.generated.css`, DSL-derived) followed by the hand-written base component styles
/// (`app.css`, keyed by the renderer's `data-c` attributes). `include_str!` bakes them at compile
/// time (drift-gated for the generated half), so pages are styled on first paint AND after
/// hydration — no external request, no dependency on the assets dir being present.
#[cfg(feature = "ssr")]
const STYLE: &str = concat!(
    include_str!("../assets/tokens.generated.css"),
    "\n",
    include_str!("../assets/app.css"),
);

/// Wrap a rendered screen body in the shared HTML document shell (the `ssr` build). One shell for
/// every server-rendered page — SDUI screens here, checkout/tracking in their own modules.
/// `hydrate_script` (the wasm bundle loader) is appended when serving with assets.
#[cfg(feature = "ssr")]
pub(crate) fn page_html(title: &str, lang: &str, body: &str) -> String {
    // `<html lang>` reflects the resolved locale (#110): SSR's source of truth for the page's
    // language, which hydrate reads back from the DOM so the re-render can't disagree with the shell.
    format!(
        "<!DOCTYPE html><html lang=\"{lang}\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{title}</title><style>{STYLE}</style></head><body>{body}</body></html>"
    )
}

/// Server-side render one SDUI screen (+ its surface's sheets) to a full document.
#[cfg(feature = "ssr")]
pub fn render_screen_html(
    screen: &'static Screen,
    sheets: &'static [crate::generated::screens::Sheet],
    ctx: RenderContext,
) -> String {
    let lang = crate::i18n::normalize_locale(&ctx.locale).unwrap_or(crate::i18n::DEFAULT_LOCALE);
    let body = SduiScreen(SduiScreenProps { screen, sheets, ctx }).to_html();
    page_html("Captain.Food", lang, &body)
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
    // Locale parity (#110): the shell's `<html lang>` is what SSR resolved through the chain; read it
    // back so the hydrate re-render can't disagree with the server's language (no flash, no re-resolve).
    let locale = window
        .document()
        .and_then(|d| d.document_element())
        .and_then(|e| e.get_attribute("lang"))
        .unwrap_or_else(|| i18n::DEFAULT_LOCALE.to_string());
    // Shared host+path resolution incl. the tenant-root rule (#98) — same authority as SSR.
    let (surface, matched) = router::resolve(&host, &path);
    let Some(matched) = matched else { return };
    let screen: &'static Screen = matched.screen;

    let session = crate::session::SessionId::load_or_mint();
    let origin = location.origin().unwrap_or_default();

    // The hand-written screens (`sdui: false`) mount their own flows — #420. This used to be
    // `if !screen.sdui { return; }`, sitting ABOVE the crate's only `mount_to_body`, so checkout
    // mounted no Stripe element and no submit handler and tracking never moved. `HandWrittenScreen`
    // is proved at compile time to cover exactly that set (`handwritten.rs`), so this branch cannot
    // be a silent default again.
    if let Some(hand_written) = crate::handwritten::HandWrittenScreen::of(screen) {
        crate::handwritten::mount::mount(
            hand_written,
            matched,
            host,
            origin,
            surface.role(),
            session,
            locale,
        );
        return;
    }

    let transport = crate::graphql::HttpTransport::new(&origin, surface.role(), session);

    // The interaction layer (#93): delegated button dispatch + push socket + boot pending-resume.
    crate::interact::install(&origin, surface.role(), session);

    let sheets = surface.sheets();
    wasm_bindgen_futures::spawn_local(async move {
        let mut ctx = RenderContext::new(&locale);
        for resolver in screen.data_requirements {
            let mut vars = serde_json::Map::new();
            for (k, v) in matched.param_args(*resolver) {
                vars.insert(k, v);
            }
            // #472: classify — a role-refused read on this anonymous path stays a silent skip;
            // a REAL failure marks the binding failed so the client render shows the error
            // state, not the empty state. No new fetch volume: the loop shape is unchanged.
            // (The client degradation legs are RESERVED in the observability contract — no
            // OTel in WASM, so nothing is emitted here.)
            let result = crate::graphql::execute_resolver(&transport, *resolver, vars).await;
            match crate::graphql::classify_resolve(surface.role(), *resolver, result) {
                crate::graphql::ResolveOutcome::Resolved(value) => {
                    ctx.insert_resolved(resolver.as_str(), value)
                }
                crate::graphql::ResolveOutcome::SkippedByDesign => {}
                crate::graphql::ResolveOutcome::Failed(_) => ctx.insert_failed(resolver.as_str()),
            }
        }
        leptos::mount::mount_to_body(move || SduiScreen(SduiScreenProps { screen, sheets, ctx }));

        // The requires_auth guard (#92, client-side): auth state lives ONLY in the browser (no
        // auth cookie exists yet — the server-side 302 is the recorded follow-up), and today no
        // token store exists at all, so every visitor is anonymous. Customer surfaces open the
        // auth sheet OVER the screen (the DSL's own if_guest pattern — late identification,
        // ADR-20260722-174500); a staff surface without one bounces to its root, where the
        // role-pathed GraphQL enforces the real gate.
        if screen.requires_auth {
            let opened = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.query_selector("[data-sheet-id=\"auth_sheet\"]").ok().flatten())
                .map(|sheet| sheet.remove_attribute("hidden").is_ok())
                .unwrap_or(false);
            if !opened && path != "/" {
                if let Some(w) = web_sys::window() {
                    let _ = w.location().set_href("/");
                }
            }
        }
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

    /// The whole generated surface area renders without panicking, empty data included — the "no
    /// placeholder left behind a reachable route" gate at the smoke level.
    ///
    /// #420 renamed this from `every_sdui_screen_of_every_surface_renders` and removed its
    /// `if !screen.sdui { continue }`. The skip excluded EXACTLY the two screens that were broken
    /// (checkout and order_tracking) while the name promised "every screen" — beck's verdict on
    /// `main` was *"not one test in this repo would go red if a stranger could not order"*, and this
    /// skip is a large part of why. Hand-written screens are now rendered through the same
    /// [`HandWrittenScreen`](crate::handwritten::HandWrittenScreen) dispatch production uses.
    #[test]
    fn every_screen_of_every_surface_renders() {
        use crate::handwritten::HandWrittenScreen;
        use crate::router::match_route;
        for surface in [
            Surface::CaptainFrontoffice,
            Surface::RestaurantFrontoffice,
            Surface::RestaurantBackoffice,
            Surface::Rider,
        ] {
            for screen in surface.screens() {
                let html = match HandWrittenScreen::of(screen) {
                    None => render_screen_html(screen, surface.sheets(), ctx()),
                    Some(hand_written) => {
                        // Through the real route match, so `:param` capture participates.
                        let concrete: String = screen
                            .route
                            .split('/')
                            .map(|seg| if seg.starts_with(':') { "x" } else { seg })
                            .collect::<Vec<_>>()
                            .join("/");
                        let matched = match_route(surface, &concrete)
                            .unwrap_or_else(|| panic!("{}: route unreachable", screen.id));
                        hand_written.render_html(&matched, &ctx(), Some("chez-test"), "fr")
                    }
                };
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

    /// #167 (PR #586 ux STOP): the timed-out treatment is a PER-CARD render, never an
    /// unconditional banner. Absent when no bound order holds CANCELLED_BY_TIMEOUT; present
    /// exactly on the timed-out card when one does.
    #[test]
    fn order_card_status_is_absent_without_a_timed_out_order_and_present_exactly_on_it() {
        let screen = Surface::RestaurantBackoffice
            .screens()
            .iter()
            .find(|s| s.id == "orders_queue")
            .unwrap();

        // A busy board with ZERO timed-out orders: the expired copy must not render at all —
        // the exact false signifier the mob stopped (the generic fallback rendered
        // "Expired — no response" on every board load).
        let mut c = ctx();
        c.insert_resolved(
            "orders.byRestaurant",
            json!([
                { "id": "o-1", "status": "PLACED", "totalAmount": { "amountCents": 2350, "currency": "EUR" } },
                { "id": "o-2", "status": "ACCEPTED", "totalAmount": { "amountCents": 980, "currency": "EUR" } },
            ]),
        );
        let html = render_screen_html(screen, Surface::RestaurantBackoffice.sheets(), c);
        assert!(
            !html.contains("data-c=\"order_card_status\""),
            "no timed-out order → no treatment node at all: {html}"
        );
        assert!(!html.contains("Expired"), "no expired copy without a timed-out order: {html}");

        // And the empty board renders nothing either (the zero-orders load the STOP named).
        let html = render_screen_html(screen, Surface::RestaurantBackoffice.sheets(), ctx());
        assert!(!html.contains("data-c=\"order_card_status\""), "{html}");
        assert!(!html.contains("Expired"), "{html}");

        // One order actually holding CANCELLED_BY_TIMEOUT: the treatment renders ONCE, bound to
        // THAT card (data-order), with the declared icon and the legal-register copy
        // ("is being released" — in progress, never done).
        let mut c = ctx();
        c.insert_resolved(
            "orders.byRestaurant",
            json!([
                { "id": "o-1", "status": "PLACED", "totalAmount": { "amountCents": 2350, "currency": "EUR" } },
                { "id": "o-3", "status": "CANCELLED_BY_TIMEOUT", "totalAmount": { "amountCents": 980, "currency": "EUR" } },
            ]),
        );
        let html = render_screen_html(screen, Surface::RestaurantBackoffice.sheets(), c);
        assert_eq!(
            html.matches("data-c=\"order_card_status\"").count(),
            1,
            "exactly one treatment, on the one timed-out card: {html}"
        );
        assert!(
            html.contains("data-c=\"order_card_status\" data-order=\"o-3\""),
            "bound to the timed-out order, not the PLACED one: {html}"
        );
        assert!(html.contains("data-icon=\"clock_x\""), "{html}");
        assert!(html.contains("Expired — no response"), "{html}");
        assert!(html.contains("is being released"), "release stated in progress, never done: {html}");
        assert!(!html.contains("was released"), "the legal STOP wording must be gone: {html}");
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

        // The rider toggle is DISPATCHABLE since #95 exposed changeRiderStatus — no disabled gap
        // control left on the jobs screen.
        let jobs = Surface::Rider.screens().iter().find(|s| s.id == "jobs").unwrap();
        let html = render_tracking_like(jobs);
        assert!(html.contains("data-action=\"rider_toggle_online\""), "{html}");
        assert!(!html.contains("No rider availability mutation"), "{html}");

        // A still-declared gap DOES render disabled with its note (the fail-closed proof moved to
        // the passkey button, executor tests) — here: the auth sheet's passkey control. Since
        // #472 the button also declares `visible_when: passkey_available`, which fails CLOSED
        // over missing data — so the gap-note proof now supplies the condition's data, and the
        // unresolved render is asserted hidden (the new, correct behaviour).
        let cart = Surface::RestaurantFrontoffice.screens().iter().find(|s| s.id == "cart").unwrap();
        let html = render_screen_html(cart, Surface::RestaurantFrontoffice.sheets(), ctx());
        assert!(
            !html.contains("WebAuthn"),
            "passkey_available unresolved → the passkey control is hidden (fail closed): {html}"
        );
        let mut c = ctx();
        c.insert_resolved("passkey_available", json!(true));
        let html = render_screen_html(cart, Surface::RestaurantFrontoffice.sheets(), c);
        assert!(html.contains("WebAuthn"), "the passkey gap note must surface: {html}");
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
    fn ssr_pages_ship_the_inlined_design_system() {
        // #115: the token variables (DSL-derived) + the base component styles keyed by data-c are
        // inlined into every page <head>, so live pages are styled without an external request.
        let home = Surface::CaptainFrontoffice.screens().iter().find(|s| s.id == "home").unwrap();
        let html = render_screen_html(home, Surface::CaptainFrontoffice.sheets(), ctx());
        assert!(html.contains("<style>"), "a style block is inlined");
        assert!(html.contains("--color-primary: #F97316"), "the generated token var is present");
        assert!(html.contains("[data-c=\"restaurant_card\"]"), "base component styles are present");
        assert!(html.contains("var(--color-primary)"), "app.css consumes the token vars");
    }

    // ── #472: condition evaluation (beck's red-first suite) ────────────────────
    //
    // A dead control stays live: the renderer consumed no `visible_when`/`disabled_when` at all,
    // so every declared condition rendered as if true. These tests were seen RED against that
    // behaviour before the evaluator existed (red evidence in the introducing commit message).

    fn node_html(node: &Node, c: &RenderContext) -> String {
        render_node(node, c).to_html()
    }

    #[test]
    fn visible_when_false_hides() {
        let node = Node {
            kind: ComponentKind::Text,
            props: &[
                ("value", PropValue::Text("SECRET-CONTENT")),
                ("visible_when", PropValue::Text("flag")),
            ],
            children: &[],
        };
        let mut c = ctx();
        c.insert_resolved("flag", json!(false));
        let html = node_html(&node, &c);
        assert!(!html.contains("SECRET-CONTENT"), "visible_when=false must hide: {html}");
    }

    #[test]
    fn visible_when_true_shows() {
        let node = Node {
            kind: ComponentKind::Text,
            props: &[
                ("value", PropValue::Text("SECRET-CONTENT")),
                ("visible_when", PropValue::Text("flag")),
            ],
            children: &[],
        };
        let mut c = ctx();
        c.insert_resolved("flag", json!(true));
        let html = node_html(&node, &c);
        assert!(html.contains("SECRET-CONTENT"), "visible_when=true must render: {html}");
    }

    /// The attribute assertion is DELIBERATE (mob briefing): in SSR HTML the `disabled` attribute
    /// IS the behaviour — the delegated click driver never fires on a disabled control.
    #[test]
    fn disabled_when_disables() {
        let node = Node {
            kind: ComponentKind::Button,
            props: &[
                ("label", PropValue::Text("Pay")),
                ("disabled_when", PropValue::Text("cart.lines.length == 0")),
            ],
            children: &[],
        };
        let mut c = ctx();
        c.insert_resolved("cart", json!({ "lines": [] }));
        let html = node_html(&node, &c);
        assert!(html.contains("disabled"), "empty cart must disable the pay button: {html}");

        let mut c = ctx();
        c.insert_resolved("cart", json!({ "lines": [{ "offerId": "o1" }] }));
        let html = node_html(&node, &c);
        assert!(!html.contains("disabled"), "a non-empty cart must not disable: {html}");
    }

    /// An unknown construct fails LOUDLY — never silently-true. `a >= b` is outside the
    /// corpus-exact grammar on purpose.
    #[test]
    fn unknown_construct_is_loud() {
        let node = Node {
            kind: ComponentKind::Text,
            props: &[
                ("value", PropValue::Text("SECRET-CONTENT")),
                ("visible_when", PropValue::Text("a >= b")),
            ],
            children: &[],
        };
        let mut c = ctx();
        c.insert_resolved("a", json!(1));
        c.insert_resolved("b", json!(2));
        let html = node_html(&node, &c);
        assert!(
            !html.contains("SECRET-CONTENT"),
            "an unparseable condition must never render its content: {html}"
        );
        assert!(
            html.contains("data-condition-error"),
            "an unparseable condition must leave a loud, auditable marker: {html}"
        );
    }

    /// Fail-safe semantics (beck + graphql, decided at the #472 briefing): a condition over
    /// MISSING/unresolved data is not an unknown construct — it fails CLOSED. `visible_when`
    /// unevaluatable → hidden; never default-visible.
    #[test]
    fn visible_when_over_missing_data_fails_closed_hidden() {
        for expr in ["passkey_available", "order.serviceType == 'DELIVERY'"] {
            let props: &'static [(&'static str, PropValue)] = match expr {
                "passkey_available" => &[
                    ("value", PropValue::Text("SECRET-CONTENT")),
                    ("visible_when", PropValue::Text("passkey_available")),
                ],
                _ => &[
                    ("value", PropValue::Text("SECRET-CONTENT")),
                    ("visible_when", PropValue::Text("order.serviceType == 'DELIVERY'")),
                ],
            };
            let node = Node { kind: ComponentKind::Text, props, children: &[] };
            let html = node_html(&node, &ctx());
            assert!(!html.contains("SECRET-CONTENT"), "{expr}: missing data must hide: {html}");
            assert!(
                !html.contains("data-condition-error"),
                "{expr}: missing data is NOT an unknown construct — no loud marker: {html}"
            );
        }
    }

    /// `disabled_when` unevaluatable → disabled; never default-enabled.
    #[test]
    fn disabled_when_over_missing_data_fails_closed_disabled() {
        let node = Node {
            kind: ComponentKind::Button,
            props: &[
                ("label", PropValue::Text("Pay")),
                ("disabled_when", PropValue::Text("cart.lines.length == 0")),
            ],
            children: &[],
        };
        let html = node_html(&node, &ctx());
        assert!(
            html.contains("disabled"),
            "an unevaluatable disabled_when must render disabled: {html}"
        );
    }

    /// The Tours-facing pin: the marketplace home's cart FAB declares
    /// `visible_when: cart_item_count > 0` — with no cart data (every anonymous first paint) it
    /// must not render, and with items it must.
    #[test]
    fn the_home_cart_fab_obeys_its_declared_condition() {
        let home = Surface::CaptainFrontoffice.screens().iter().find(|s| s.id == "home").unwrap();
        // NOTE: the inlined design system CSS mentions the [data-c="floating_action_button"]
        // SELECTOR on every page, so the assertion targets the rendered ELEMENT markup.
        let html = render_screen_html(home, Surface::CaptainFrontoffice.sheets(), ctx());
        assert!(
            !html.contains("<button data-c=\"floating_action_button\""),
            "no cart data → the cart FAB must be hidden (fail closed): {html}"
        );
        let mut c = ctx();
        c.insert_resolved("cart_item_count", json!(2));
        let html = render_screen_html(home, Surface::CaptainFrontoffice.sheets(), c);
        assert!(
            html.contains("<button data-c=\"floating_action_button\""),
            "a filled cart must show the FAB: {html}"
        );
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
