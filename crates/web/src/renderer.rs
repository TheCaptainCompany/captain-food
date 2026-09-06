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

/// What a screen renders FROM: the resolver results keyed by FULL RESOLVER KEY + the locale.
///
/// Keying (#729): each resolver result is stored under its dotted spec key
/// (`orders.byRestaurant`) and NOTHING else — the old first-segment alias storage let same-root
/// siblings (`mailbox.lanes`/`mailbox.poisoned`) overwrite each other's entry. Template names
/// reach the data through [`feeding_key`]'s ONE matching rule instead: the longest stored-key
/// prefix of the binding path, else the stored key whose derived template alias
/// ([`resolver_aliases`]) equals the binding's ROOT — so `{{ orders }}`,
/// `{{ featured_restaurants }}` and `{{ cart.lines }}` all reach the entry their resolver stored.
#[derive(Debug, Clone, Default)]
pub struct RenderContext {
    pub data: Map<String, Value>,
    /// Resolver reads that FAILED for real (#472) — transport/contract failures on a read this
    /// role path is allowed to ask, keyed by FULL resolver key, exactly like `data` (#729 — the
    /// parity `resolver_key_parity_between_data_and_failure_marks` pins). NOT the skip-by-design
    /// outcomes (declared gaps, role-refused reads on the anonymous SSR path), which leave no
    /// trace here: a failed binding renders its ERROR state, a skipped one its empty/shell state,
    /// and conflating the two is exactly the "Commande introuvable over a transient failure"
    /// defect this field exists to prevent.
    failed: BTreeSet<String>,
    /// The per-render error-anchor assignment (#730, [`RenderContext::assign_error_anchors`]):
    /// for each failed resolver, the ONE node (by address — screen trees are `'static`) that
    /// renders its error affordance, plus the resolver keys that HAVE such an anchor. Inactive
    /// (`anchors_assigned == false`) until a screen-level render assigns it, so direct
    /// `render_node` calls keep their per-node semantics.
    error_anchors: BTreeSet<usize>,
    error_claimed: BTreeSet<String>,
    anchors_assigned: bool,
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
            error_anchors: BTreeSet::new(),
            error_claimed: BTreeSet::new(),
            anchors_assigned: false,
            locale: locale.to_string(),
            stripe_publishable_key: None,
        }
    }

    /// Record one resolver read as FAILED (#472) — under the FULL resolver key only, exactly as
    /// `insert_resolved` keys `data` (#729): the old per-alias marks made one failed resolver
    /// shadow its same-root sibling's resolved data.
    pub fn insert_failed(&mut self, resolver_key: &str) {
        self.failed.insert(resolver_key.to_string());
    }

    /// Whether the resolver feeding this `{{ path }}` binding failed for real (#472) — matched by
    /// [`feeding_key`]'s longest-prefix/alias rule over the FULL resolver keys, never the bare
    /// root (#729). Precedence: `answer_beats_failure_mark` (the tracking.rs sentence, now the
    /// context's own rule) — a binding some stored ANSWER feeds is never failed, whatever marks
    /// exist. Cross-resolver precedence only, never intra-resolver salvage: the rule is per
    /// resolver KEY, and a null field inside Ok data stays legitimate absence, never a failure.
    pub fn binding_failed(&self, raw: &str) -> bool {
        self.failed_resolver_for(raw).is_some()
    }

    /// The FAILED resolver key feeding this binding, `None` when an answer feeds it instead (or
    /// nothing matches) — `binding_failed` plus the key the #730 anchor assignment claims once.
    ///
    /// Answers and marks compete under the SAME [`feeding_key`] rule: the longer match decides
    /// which resolver the binding actually names (`{{ mailbox.lanes }}` names the failed
    /// `mailbox.lanes`, never its answered sibling via the shared root), and an answer wins any
    /// tie — the same resolver both answered and marked (a failed re-read) is answered.
    fn failed_resolver_for(&self, raw: &str) -> Option<&str> {
        let path = raw.split('|').next().unwrap_or(raw).trim();
        let answered = feeding_key(self.data.keys().map(String::as_str), path);
        let marked = feeding_key(self.failed.iter().map(String::as_str), path)?;
        match answered {
            Some((_, answered_len)) if answered_len >= marked.1 => None,
            _ => Some(marked.0),
        }
    }

    /// Store one resolver result under its FULL spec key — and nothing else (#729): sibling
    /// resolvers sharing a root must never overwrite each other. Template aliases are resolved at
    /// read time by [`feeding_key`].
    pub fn insert_resolved(&mut self, resolver_key: &str, value: Value) {
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
            (Some(v), Some("format_datetime")) => format_datetime(v),
            (Some(v), Some("format_address")) => format_address(v),
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

    /// Dotted-path walk into the data map: [`feeding_key`] picks the stored entry, the remaining
    /// segments walk into its value (`order.status` → data["order.byId"]["status"] when
    /// `order.byId` is the stored key `order` aliases to).
    fn lookup(&self, path: &str) -> Option<&Value> {
        let (key, consumed) = feeding_key(self.data.keys().map(String::as_str), path)?;
        let mut cur = self.data.get(key)?;
        for seg in path[consumed..].split('.').filter(|s| !s.is_empty()) {
            cur = cur.get(seg)?;
        }
        Some(cur)
    }
}

/// The stored key that FEEDS a binding path — the ONE matching rule shared by value lookup and
/// failure-mark matching (#729), so the two can never disagree on which resolver a binding names.
/// Returns the key + how many bytes of `path` it consumed (the rest is walked into the value).
///
///   1. **Longest dotted prefix** of the path among the stored keys (`mailbox.poisoned` beats the
///      shared root `mailbox`; an exact key match beats both) — this is what keeps same-root
///      siblings apart.
///   2. Otherwise, a key whose derived template alias ([`resolver_aliases`]) equals the path's
///      ROOT segment (`{{ orders }}` → `orders.byRestaurant`, `{{ featured_restaurants }}` →
///      `restaurants.featured`).
///
/// Ties (two keys aliasing the same bare root, e.g. `{{ restaurants }}` over
/// `restaurants.featured`/`restaurants.all`) resolve to the lexicographically smallest key —
/// deterministic; no checked-in binding uses an ambiguous root (the corpus binds the reversed
/// aliases).
fn feeding_key<'k>(keys: impl Iterator<Item = &'k str>, path: &str) -> Option<(&'k str, usize)> {
    let root_len = path.find('.').unwrap_or(path.len());
    let root = &path[..root_len];
    // (tier, consumed, Reverse-ordered key) — bigger wins; tier 2 = key-prefix, tier 1 = alias.
    let mut best: Option<(u8, usize, &'k str)> = None;
    for k in keys {
        let candidate = if path == k
            || (path.len() > k.len() && path.as_bytes()[k.len()] == b'.' && path.starts_with(k))
        {
            (2u8, k.len(), k)
        } else {
            let (first, reversed) = resolver_aliases(k);
            if root == first || reversed.as_deref() == Some(root) {
                (1u8, root_len, k)
            } else {
                continue;
            }
        };
        let better = match best {
            None => true,
            Some((t, c, bk)) => {
                (candidate.0, candidate.1) > (t, c)
                    || ((candidate.0, candidate.1) == (t, c) && candidate.2 < bk)
            }
        };
        if better {
            best = Some(candidate);
        }
    }
    best.map(|(_, consumed, k)| (k, consumed))
}

/// The template aliases a resolver key's result answers to (see [`RenderContext`] type docs):
/// `(first_segment, Option<reversed second_first form>)` — the ONE authority for the aliasing
/// rule, shared by [`feeding_key`] (lookup + failure marks, #729), the gap classification (#725)
/// and mirrored by the §25 validator (`tools/codegen-rs/src/validate/screen_bindings.rs`).
fn resolver_aliases(resolver_key: &str) -> (String, Option<String>) {
    let mut parts = resolver_key.splitn(2, '.');
    let first = parts.next().unwrap_or(resolver_key).to_string();
    let reversed = parts.next().and_then(|second| {
        second
            .chars()
            .all(|c| c.is_ascii_lowercase() || c == '_')
            .then(|| format!("{second}_{first}"))
    });
    (first, reversed)
}

/// The declared `gap:` note of the resolver whose stored aliases would feed this binding ROOT
/// (#725) — how the renderer tells "this section sits on a spec-declared gap" (render GAPPED,
/// auditably) from "this section answered empty" (render the empty state). `None` = no gap
/// resolver aliases to this root.
fn resolver_gap_for_root(root: &str) -> Option<&'static str> {
    use crate::generated::data_layer::ResolverKey;
    ResolverKey::ALL.iter().find_map(|k| {
        let note = k.gap()?;
        let key = k.as_str();
        let (first, reversed) = resolver_aliases(key);
        (root == key || root == first || reversed.as_deref() == Some(root)).then_some(note)
    })
}

// ─── #730: screen-level error-anchor assignment ────────────────────────────────────────────────

impl RenderContext {
    /// Assign, for each FAILED resolver, the ONE node that renders its error affordance — the
    /// granularity is the RESOLVER, rendered ONCE, never per-scalar inline and never screen-level
    /// (#730, the #472 pattern generalized past list kinds):
    ///
    ///   * the anchor is the first node (pre-order, visibility mirrored) whose OWN display props
    ///     bind the failed resolver, PROMOTED to its nearest enclosing section — an error card
    ///     where one label sat is noise, an error state where the section sat is legible;
    ///   * a kind with a bespoke error state (the #472 list kinds, with per-surface copy like
    ///     `cart.error.load`) anchors ITSELF and keeps rendering that state;
    ///   * chrome (headers, nav, sheets) never anchors and never degrades — navigation survives a
    ///     failed read;
    ///   * every OTHER node fed by an anchored failed resolver renders ABSENT
    ///     ([`render_node`]) — blank money and empty scalars over failed data are a lie, and the
    ///     resolver's one error state already stands at its anchor.
    ///
    /// Action variables are NOT display bindings: a control whose `variables` bind a failed
    /// resolver DISABLES instead (`executor::resolved_variables`) — a live button over failed
    /// data is a false signifier.
    pub fn assign_error_anchors(
        &mut self,
        screen: &Screen,
        sheets: &[crate::generated::screens::Sheet],
    ) {
        let mut anchors = BTreeSet::new();
        let mut claimed = BTreeSet::new();
        let mut sections = Vec::new();
        walk_error_anchors(self, screen.tree, &mut sections, &mut anchors, &mut claimed);
        for sheet in sheets {
            walk_error_anchors(
                self,
                std::slice::from_ref(&sheet.node),
                &mut sections,
                &mut anchors,
                &mut claimed,
            );
        }
        self.error_anchors = anchors;
        self.error_claimed = claimed;
        self.anchors_assigned = true;
    }
}

/// A node's identity for the anchor set: its address (screen trees are `'static`; test-local
/// nodes outlive their render pass).
fn node_id(node: &Node) -> usize {
    node as *const Node as usize
}

/// Kinds with a bespoke, per-surface-copy error state of their own (#472) — they anchor
/// themselves and `render_node_kind` renders their state, never the generic affordance.
fn bespoke_error_kind(kind: ComponentKind) -> bool {
    matches!(
        kind,
        ComponentKind::List
            | ComponentKind::RestaurantCardGrid
            | ComponentKind::RestaurantCardList
            | ComponentKind::SearchResults
            | ComponentKind::OrderList
            | ComponentKind::MessageBubble
            | ComponentKind::CartLines
    )
}

/// The FAILED resolvers feeding this node's OWN display bindings: `Binding` props outside the
/// action/trigger/per-item namespaces (those disable their control or resolve per row instead).
fn failed_display_resolvers(node: &Node, ctx: &RenderContext) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (key, prop) in node.props {
        let PropValue::Binding(path) = prop else { continue };
        let ns = key.split('.').next().unwrap_or(key);
        if matches!(
            ns,
            "action" | "on_change" | "on_complete" | "on_success" | "item_components"
                | "item_badge" | "item_action"
        ) {
            continue;
        }
        if let Some(r) = ctx.failed_resolver_for(path) {
            if !out.iter().any(|x| x == r) {
                out.push(r.to_string());
            }
        }
    }
    out
}

/// The pre-order walk behind [`RenderContext::assign_error_anchors`] — mirrors the render's own
/// visibility gates (a hidden subtree can anchor nothing) and the conditional section's
/// one-branch rule.
fn walk_error_anchors(
    ctx: &RenderContext,
    nodes: &[Node],
    sections: &mut Vec<usize>,
    anchors: &mut BTreeSet<usize>,
    claimed: &mut BTreeSet<String>,
) {
    use crate::generated::registry::ComponentGroup;
    for node in nodes {
        match eval_condition_prop(node, "visible_when", ctx, false) {
            Some(Err(_)) | Some(Ok(false)) => continue,
            _ => {}
        }
        if node.kind.group() == ComponentGroup::Chrome {
            continue; // headers/nav/sheets stay rendered — never an anchor
        }
        let is_section = matches!(
            node.kind,
            ComponentKind::Section | ComponentKind::CheckoutSection | ComponentKind::ConditionalSection
        );
        if is_section {
            sections.push(node_id(node));
        }
        for resolver in failed_display_resolvers(node, ctx) {
            if claimed.insert(resolver) {
                let anchor = if bespoke_error_kind(node.kind) {
                    node_id(node)
                } else {
                    sections.last().copied().unwrap_or_else(|| node_id(node))
                };
                anchors.insert(anchor);
            }
        }
        walk_error_anchors(ctx, node.children, sections, anchors, claimed);
        if let Some(Ok(Some(verdict))) = eval_condition_verdict(node, "condition", ctx) {
            let branch = if verdict { "if_true" } else { "if_false" };
            if let Some(group) = node.branch(branch) {
                walk_error_anchors(ctx, group, sections, anchors, claimed);
            }
        }
        if is_section {
            sections.pop();
        }
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

/// The French month abbreviations Europe/Paris display uses (`format_datetime`) — three/four-letter
/// forms with the trailing period France actually prints, `mars`/`mai`/`juin`/`août` excepted (no
/// abbreviation shorter than the full word).
const FR_MONTHS: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc.",
];

/// `| format_datetime` (#639 part C step 4-ii, ADR-20260904-124600 §4): a UTC instant string
/// (`decidedAt`/`effectiveAt`) rendered Europe/Paris, `fr` — e.g. "4 sept. 2026, 14:02". The event
/// and the read model keep the UTC instant; this is presentation-only, beside `format_currency` —
/// the SECOND filter this catalog has ever needed. Unparseable/absent input renders "" (a binding
/// is a data slot, not an error) — never a raw ISO string leaking through.
pub(crate) fn format_datetime(v: &Value) -> String {
    use chrono::{DateTime, Datelike, Timelike};
    let Some(s) = v.as_str() else { return String::new() };
    let Ok(dt) = DateTime::parse_from_rfc3339(s) else { return String::new() };
    let paris = dt.with_timezone(&chrono_tz::Europe::Paris);
    let month = FR_MONTHS[paris.month0() as usize];
    format!("{} {} {}, {:02}:{:02}", paris.day(), month, paris.year(), paris.hour(), paris.minute())
}

/// `| format_address` (#639 part C step 4-ii round 2, ADR-20260904-124600 §5): an `Address`
/// object (`line1`[, `line2`], `postalCode`, `city`) rendered as ONE display line — "12 rue de la
/// Paix, 37000 Tours". `line2` is appended only when present (most France addresses carry none);
/// `country` is never shown (V0 is Tours-only, ADR-0004). Missing `line1` renders "" — a binding
/// is a data slot, not an error — never a bare object falling through to `format_currency`'s
/// Money-shaped read (the exact defect this filter closes, round-2 item 1b: an `Address` object
/// bound with no filter used to route there and silently render "").
pub(crate) fn format_address(v: &Value) -> String {
    let Some(line1) = v.get("line1").and_then(Value::as_str).filter(|s| !s.is_empty()) else {
        return String::new();
    };
    let mut out = line1.to_string();
    if let Some(line2) = v.get("line2").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        out.push_str(", ");
        out.push_str(line2);
    }
    let postal = v.get("postalCode").and_then(Value::as_str).unwrap_or_default();
    let city = v.get("city").and_then(Value::as_str).unwrap_or_default();
    if !postal.is_empty() || !city.is_empty() {
        out.push_str(", ");
        out.push_str(postal);
        if !postal.is_empty() && !city.is_empty() {
            out.push(' ');
        }
        out.push_str(city);
    }
    out
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

/// The per-row render context (#725): the row travels as `item`, everything else inherited — so
/// per-item templates (`item_components`, `item_badge`) resolve `{{ item.* }}` bindings and
/// `item.*` conditions through the SAME machinery every other binding uses.
fn item_ctx(ctx: &RenderContext, row: &Value) -> RenderContext {
    let mut c = ctx.clone();
    c.data.insert("item".to_string(), row.clone());
    c
}

/// Resolve every inline `{{ path }}` template in a literal string against the context (#725):
/// the shape multi-template values use ("{{ item.actorType }} / {{ item.partition }}"), which is
/// deliberately NOT one Binding (the emitter keeps it Text). Unresolvable paths render empty —
/// bindings are data slots, not errors.
fn interpolate_templates(s: &str, ctx: &RenderContext) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                out.push_str(&ctx.binding_text(after[..end].trim()));
                rest = &after[end + 2..];
            }
            None => {
                out.push_str(&rest[start..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A per-item template prop as display text (#725): a Binding resolves against the row context; a
/// literal Text may carry inline templates and interpolates.
fn item_prop_text(node: &Node, key: &str, row_ctx: &RenderContext) -> String {
    match node.prop(key) {
        Some(PropValue::Text(s)) if s.contains("{{") => interpolate_templates(s, row_ctx),
        _ => prop_text(node, key, row_ctx),
    }
}

/// The `item_badge` per-item config of a List (#725/#160): the badge on rows whose declared
/// condition holds — evaluated against THAT row (fail closed per #472: unevaluatable hides;
/// unparseable is loud). `None` = no badge on this row (or none declared).
fn item_badge_view(node: &Node, row_ctx: &RenderContext) -> Option<AnyView> {
    node.prop("item_badge.text")?;
    match eval_condition_prop(node, "item_badge.visible_when", row_ctx, false) {
        Some(Err(expr)) => Some(condition_error_marker(expr)),
        Some(Ok(false)) => None,
        Some(Ok(true)) | None => {
            let text = prop_text(node, "item_badge.text", row_ctx);
            let variant = prop_text(node, "item_badge.variant", row_ctx);
            Some(
                view! { <span data-c="item_badge" data-variant=variant>{text}</span> }.into_any(),
            )
        }
    }
}

/// The `item_components.N.*` per-item templates of a List (#725): each row renders the declared
/// component sequence against its own context. `None` = the node declares none (the plain-line
/// List shape).
fn item_component_views(node: &Node, row_ctx: &RenderContext) -> Option<Vec<AnyView>> {
    node.prop("item_components.0.type")?;
    Some(render_item_entries(node, "item_components", row_ctx))
}

/// Does an item-components-style entry exist at `prefix` at all (a `type` prop present there)?
/// The one presence check shared by the top-level scan and any nested `conditional_section`
/// branch scan below — a `type` value the corpus does not carry is "no more entries", never a
/// render decision.
fn item_entry_type(node: &Node, prefix: &str) -> Option<&'static str> {
    match node.prop(&format!("{prefix}.type")) {
        Some(PropValue::Text(ty)) => Some(ty),
        _ => None,
    }
}

/// Every item-components-style entry declared under `prefix` (`item_components` at the top level,
/// `item_components.{i}.if_true`/`if_false` one level into a nested `conditional_section`),
/// rendered against `row_ctx`, in declared order, skipping any entry a `visible_when` hides.
fn render_item_entries(node: &Node, prefix: &str, row_ctx: &RenderContext) -> Vec<AnyView> {
    let mut out = Vec::new();
    for i in 0..32 {
        let entry_prefix = format!("{prefix}.{i}");
        if item_entry_type(node, &entry_prefix).is_none() {
            break;
        }
        if let Some(view) = render_item_entry(node, &entry_prefix, row_ctx) {
            out.push(view);
        }
    }
    out
}

/// One `item_components`-style entry at `prefix`, against `row_ctx`. Supported types mirror the
/// tiered markup rule: `info_row`/`badge` (label+value rows), `text`, `button` (the full action
/// DOM contract via the prefixed parser — the mailbox requeue intervention, #315), and — round 3
/// (#639 part C step 6-iv, ux) — `conditional_section`: the corpus's ONLY compound-condition
/// grammar is NESTING (`rider.yaml:~449,463`'s conditional_section-in-conditional_section, the
/// "no `&&`" grammar generalised into a per-row entry), so a per-row control gated on more than
/// the one comparison a bare `visible_when` can express (e.g. "PENDING invitations only") nests a
/// `conditional_section` around it instead of inventing a second condition slot. Anything else
/// renders the tagged generic label+value container. `variant_when` is out of scope, as at the
/// #472 briefing (evans). `None` = hidden on this row (a `visible_when: false`, or an
/// unevaluatable/false nested `condition:`) — the caller skips it, never a "no such entry" signal
/// (that is [`item_entry_type`]'s job).
fn render_item_entry(node: &Node, prefix: &str, row_ctx: &RenderContext) -> Option<AnyView> {
    let key = |suffix: &str| format!("{prefix}.{suffix}");
    let ty = item_entry_type(node, prefix)?;
    match eval_condition_prop(node, &key("visible_when"), row_ctx, false) {
        Some(Err(expr)) => return Some(condition_error_marker(expr)),
        Some(Ok(false)) => return None, // hidden on THIS row — fail closed like every condition
        Some(Ok(true)) | None => {}
    }
    let label = prop_text(node, &key("label"), row_ctx);
    match ty {
        "conditional_section" => match eval_condition_verdict(node, &key("condition"), row_ctx) {
            Some(Err(expr)) => Some(condition_error_marker(expr)),
            verdict => {
                // Mutual exclusion, same as the top-level `ComponentKind::ConditionalSection`
                // (#725, beck's trap): EXACTLY ONE branch renders; unevaluatable fails CLOSED to
                // NEITHER, never a silent default-true.
                let chosen = match verdict {
                    Some(Ok(Some(true))) => Some("if_true"),
                    Some(Ok(Some(false))) => Some("if_false"),
                    _ => None,
                };
                let views = chosen
                    .map(|branch| render_item_entries(node, &key(branch), row_ctx))
                    .unwrap_or_default();
                Some(view! { <span data-c=ty>{views}</span> }.into_any())
            }
        },
        "text" => {
            let value = item_prop_text(node, &key("value"), row_ctx);
            Some(view! { <p data-c=ty>{value}</p> }.into_any())
        }
        "button" => {
            let (action_attrs, disabled_reason) =
                crate::executor::button_attrs_prefixed(node, row_ctx, &key("action"));
            let get =
                |k: &str| action_attrs.iter().find(|(a, _)| *a == k).map(|(_, v)| v.clone());
            use crate::executor::attrs;
            let variant = prop_text(node, &key("variant"), row_ctx);
            let disabled = disabled_reason.is_some();
            Some(
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
                        disabled=disabled
                        title=disabled_reason
                    >
                        {label}
                    </button>
                }
                .into_any(),
            )
        }
        // A per-item `{ type: badge, text:, variant:, visible_when: }` entry (#639 part C
        // step 4-iii-A's roster list) — the SAME shape and reasoning as the top-level
        // `ComponentKind::Badge` arm: `text`, never `label`/`value`, resolved per-row. Guarded
        // on the declared field so the mailbox lanes screen's older `label:`/`value:` per-item
        // badge (`item_components.N.type: badge`) keeps its existing rendering unchanged.
        "badge" if node.prop(&key("text")).is_some() => {
            let text = item_prop_text(node, &key("text"), row_ctx);
            let variant = item_prop_text(node, &key("variant"), row_ctx);
            Some(view! { <span data-c=ty data-variant=variant>{text}</span> }.into_any())
        }
        // info_row, and any other labelled value template.
        _ => {
            let value = item_prop_text(node, &key("value"), row_ctx);
            Some(
                view! { <div data-c=ty><span>{label}</span><span>{value}</span></div> }
                    .into_any(),
            )
        }
    }
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

/// Evaluate a node's condition prop over the resolved data, three-way. `None` = the node declares
/// none. `Some(Err(expr))` = outside the corpus grammar — the caller must be LOUD.
/// `Some(Ok(Some(bool)))` = evaluated; `Some(Ok(None))` = UNEVALUATABLE over the data at hand
/// (missing data) — the caller fails CLOSED in whatever way its surface demands (#472: hidden for
/// `visible_when`, disabled for `disabled_when`; #725: NEITHER branch for `condition:`).
fn eval_condition_verdict(
    node: &Node,
    key: &str,
    ctx: &RenderContext,
) -> Option<Result<Option<bool>, &'static str>> {
    let prop = node.prop(key)?;
    let PropValue::Text(expr) = prop else {
        // A binding/i18n value in a condition slot is invisible to the grammar — loud, like any
        // unknown construct (the corpus gate keeps checked-in specs out of this branch).
        return Some(Err("<non-literal condition>"));
    };
    Some(match Condition::parse(expr) {
        Err(_) => Err(expr),
        Ok(c) => Ok(c.eval(&|path| ctx.lookup(path).cloned())),
    })
}

/// [`eval_condition_verdict`] with the unevaluatable case already collapsed to the caller's
/// fail-closed polarity (#472): hidden for `visible_when` (`on_unevaluatable = false`), disabled
/// for `disabled_when` (`on_unevaluatable = true`).
fn eval_condition_prop(
    node: &Node,
    key: &str,
    ctx: &RenderContext,
    on_unevaluatable: bool,
) -> Option<Result<bool, &'static str>> {
    Some(eval_condition_verdict(node, key, ctx)?.map(|v| v.unwrap_or(on_unevaluatable)))
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
    // #730: screen-level error granularity — active only when a screen-level render assigned
    // anchors ([`RenderContext::assign_error_anchors`]); direct per-node renders keep their
    // per-node semantics.
    if ctx.anchors_assigned {
        if ctx.error_anchors.contains(&node_id(node)) {
            if !bespoke_error_kind(node.kind) {
                return binding_error_state(
                    node.kind.as_str(),
                    "common.error.data_unavailable",
                    ctx,
                );
            }
            // A bespoke kind IS its own anchor: fall through to its per-surface error state.
        } else if node.kind.group() != crate::generated::registry::ComponentGroup::Chrome
            && failed_display_resolvers(node, ctx)
                .iter()
                .any(|r| ctx.error_claimed.contains(r))
        {
            // Fed by an anchored failed resolver, not the anchor: ABSENT — blank money/scalars
            // over failed data lie, and the resolver's ONE error state stands at its anchor.
            return ().into_any();
        }
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
            // `conditional_section` spells its predicate TWO ways in the spec (#472):
            // `visible_when` (handled by the render_node choke point like every node) and
            // `condition:` with if_true/if_false branches — real named child groups since #725.
            // Both route through the ONE evaluator. An unparseable `condition:` is loud, like any
            // unknown construct.
            match eval_condition_verdict(node, "condition", ctx) {
                Some(Err(expr)) => return condition_error_marker(expr),
                verdict => {
                    let cond = verdict.as_ref().map(|v| match v {
                        Ok(Some(true)) => "true".to_string(),
                        Ok(Some(false)) => "false".to_string(),
                        Ok(None) | Err(_) => "unevaluatable".to_string(),
                    });
                    // Mutual exclusion (#725, beck's trap): EXACTLY ONE branch renders — the
                    // evaluated verdict picks it. Unevaluatable (missing data, e.g. client form
                    // state on the SSR pass) fails CLOSED: NEITHER branch, `data-cond`
                    // stamps "unevaluatable" (the loud marker stays reserved for unparseable
                    // expressions, per the #472 missing-data semantics).
                    let chosen = match &verdict {
                        Some(Ok(Some(true))) => Some("if_true"),
                        Some(Ok(Some(false))) => Some("if_false"),
                        _ => None,
                    };
                    let branch_views: Vec<AnyView> = chosen
                        .and_then(|name| node.branch(name))
                        .map(|group| group.iter().map(|c| render_node(c, ctx)).collect())
                        .unwrap_or_default();
                    let title = prop_text(node, "title", ctx);
                    let has_title = !title.is_empty();
                    return view! {
                        <section data-c=ty data-cond=cond>
                            {has_title.then(|| view! { <h2>{title.clone()}</h2> })}
                            {children_views(node, ctx)}
                            {branch_views}
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
            // The generic titled list (location picker's address lists, the backoffice claims
            // list, the system mailbox lanes): rows from the bound items. Per-item templates
            // (`item_components.N.*`) and per-item config (`item_badge.*`) render against each
            // ROW's own context (#725) — before that they were declared and silently ignored.
            if items_binding_failed(node, ctx) {
                return binding_error_state(ty, "common.error.data_unavailable", ctx);
            }
            let title = prop_text(node, "title", ctx);
            let items = items_of(node, ctx);
            if items.is_empty() {
                let empty_title = prop_text(node, "empty_state.title", ctx);
                if !empty_title.is_empty() {
                    // The DECLARED empty state (e.g. the mailbox's #596 copy) — previously
                    // spec'd and never rendered.
                    let body = prop_text(node, "empty_state.body", ctx);
                    return view! {
                        <div data-c=ty data-empty="true"><h3>{empty_title}</h3><p>{body}</p></div>
                    }
                    .into_any();
                }
            }
            let rows: Vec<AnyView> = items
                .iter()
                .map(|item| {
                    let row_ctx = item_ctx(ctx, item);
                    let badge = item_badge_view(node, &row_ctx);
                    if let Some(components) = item_component_views(node, &row_ctx) {
                        return view! { <li data-c="list_item">{components}{badge}</li> }
                            .into_any();
                    }
                    let line = item
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| item.get("line1").and_then(Value::as_str).unwrap_or("").to_string());
                    view! { <li>{line}{badge}</li> }.into_any()
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
        // A standalone `{ type: badge, text:, variant:, visible_when: }` (#639 part C step 3-i's
        // `delivery_issue_card`/`delivery_handback_card`, #639 part C step 4-iii-A's roster/detail
        // — badge-per-enum-value with a per-row `visible_when`, never `variant_when`, which the
        // renderer does not consume). Before this arm every SCREEN-level badge (never inside a
        // `list`'s `item_components`, which `item_badge_view`/`item_component_views` already
        // handle) fell into the tagged generic container: it checked `title`/`label`/`value`,
        // never `text`, so the badge rendered an EMPTY node with no `data-variant` at all — a
        // control that renders but shows nothing, found while wiring the roster's own badges.
        // Dispatched on the DECLARED field (`text:` vs the mailbox lanes screen's own older
        // `label:`/`value:`/`variant_when:` shape, ADR-20260904-152807 §5 refuses `variant_when`
        // for new screens but does not retire the mailbox screen's existing one) so this arm is
        // strictly additive, never a behaviour change for an already-shipped badge.
        ComponentKind::Badge if node.prop("text").is_some() => {
            let text = prop_text(node, "text", ctx);
            let variant = prop_text(node, "variant", ctx);
            view! { <span data-c=ty data-variant=variant>{text}</span> }.into_any()
        }

        // ── discovery lists ─────────────────────────────────────────────────────
        ComponentKind::RestaurantCardGrid | ComponentKind::RestaurantCardList => {
            if items_binding_failed(node, ctx) {
                return binding_error_state(ty, "common.error.data_unavailable", ctx);
            }
            let cards: Vec<AnyView> = items_of(node, ctx).iter().map(restaurant_card).collect();
            view! { <div data-c=ty>{cards}</div> }.into_any()
        }
        ComponentKind::SearchResults => {
            // `sections.N.{id,title,items,item_type}` — flattened per-section config (#725).
            // Each section renders HONESTLY: a binding on a spec-declared gap resolver renders
            // GAPPED (visible and auditable via `data-gap`, never a permanently-empty
            // live-looking list); a failed read renders the error state; a backed binding
            // renders its rows. The node-level empty_state renders once when every backed
            // section answered empty.
            let mut sections: Vec<AnyView> = Vec::new();
            let mut declared = false;
            let mut backed = 0usize;
            let mut backed_rows = 0usize;
            for i in 0..12 {
                let sk = |s: &str| format!("sections.{i}.{s}");
                if node.prop(&sk("id")).is_none() && node.prop(&sk("title")).is_none() {
                    break;
                }
                declared = true;
                let sid = prop_text(node, &sk("id"), ctx);
                let title = prop_text(node, &sk("title"), ctx);
                let Some(PropValue::Binding(raw)) = node.prop(&sk("items")) else { continue };
                let path = raw.split('|').next().unwrap_or(raw).trim();
                let root = path.split('.').next().unwrap_or(path);
                let rows_val = ctx.lookup(path).and_then(Value::as_array).cloned();
                if rows_val.is_none() {
                    if let Some(note) = resolver_gap_for_root(root) {
                        let note = note.to_string();
                        sections.push(
                            view! {
                                <section data-c="search_result_section" data-id=sid data-gap=note>
                                    <h3>{title}</h3>
                                </section>
                            }
                            .into_any(),
                        );
                        continue;
                    }
                    if ctx.binding_failed(path) {
                        sections.push(binding_error_state(
                            "search_result_section",
                            "common.error.data_unavailable",
                            ctx,
                        ));
                        continue;
                    }
                }
                let rows = rows_val.unwrap_or_default();
                backed += 1;
                backed_rows += rows.len();
                let item_type = prop_text(node, &sk("item_type"), ctx);
                let cards: Vec<AnyView> = rows
                    .iter()
                    .map(|item| {
                        if item_type == "restaurant_card" {
                            restaurant_card(item)
                        } else {
                            let name = item
                                .get("displayName")
                                .or_else(|| item.get("name"))
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let it = item_type.clone();
                            view! { <div data-c=it>{name}</div> }.into_any()
                        }
                    })
                    .collect();
                sections.push(
                    view! {
                        <section data-c="search_result_section" data-id=sid>
                            <h3>{title}</h3>
                            {cards}
                        </section>
                    }
                    .into_any(),
                );
            }
            if !declared {
                // The undeclared-sections shape: a bare `items` binding of restaurant rows.
                if items_binding_failed(node, ctx) {
                    return binding_error_state(ty, "common.error.data_unavailable", ctx);
                }
                let cards: Vec<AnyView> = items_of(node, ctx).iter().map(restaurant_card).collect();
                return view! { <div data-c=ty>{cards}</div> }.into_any();
            }
            let empty = backed > 0 && backed_rows == 0;
            let empty_view = empty.then(|| {
                let t = prop_text(node, "empty_state.title", ctx);
                let b = prop_text(node, "empty_state.body", ctx);
                view! { <div data-empty="true"><h3>{t}</h3><p>{b}</p></div> }
            });
            view! { <div data-c=ty>{sections}{empty_view}</div> }.into_any()
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
            // A declared default (`value: "+33"` — the rider door's prefilled dialing code, #639
            // 2c-ii): the ONE thing the driver's `{{ <id>.value }}` fill reads back, so a field the
            // user never touches still submits its declared value.
            let default_value = prop_text(node, "value", ctx);
            view! {
                <label data-c=ty>
                    {label}
                    <input
                        id=field_id
                        placeholder=placeholder
                        value={if default_value.is_empty() { None } else { Some(default_value) }}
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
        // A refusal's own place on the screen (#639 2c-ii, ADR-20260830-213135): `for_action`
        // names the action whose REJECTED/FAILED verdict lands here — the driver fills the text
        // (the server's localized catalogue sentence, context interpolated) and un-hides it
        // instead of a passing toast. Hidden until then; a static `message` renders as before.
        ComponentKind::InlineError => {
            let message = prop_text(node, "message", ctx);
            let for_action = prop_text(node, "for_action", ctx);
            let field_id = prop_text(node, "id", ctx);
            let hidden = !for_action.is_empty() && message.is_empty();
            view! {
                <p
                    data-c=ty
                    id={if field_id.is_empty() { None } else { Some(field_id) }}
                    role="alert"
                    data-for-action={if for_action.is_empty() { None } else { Some(for_action) }}
                    hidden=hidden
                >
                    {message}
                </p>
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

        ComponentKind::CatalogSections => {
            // The MENU (#749) — the content the storefront exists to show. Until this arm the
            // kind fell into the generic tagged container, so even a RESOLVED catalog rendered an
            // empty div: the schema fix alone would have unbroken the data and left the customer
            // with no menu. Categories render as section headers with the products whose
            // `categoryRef` names them; unmatched (or category-less) products render in one
            // trailing run so no item is silently lost. Each item row carries the name,
            // description and first-offer price — the add-to-cart affordance is hydrate-side
            // (`item_add_action`), SSR renders what the customer decides on.
            let bound_array = |key: &str| -> Vec<Value> {
                match node.prop(key) {
                    Some(PropValue::Binding(path)) => ctx
                        .lookup(path.split('|').next().unwrap_or(path).trim())
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default(),
                    _ => Vec::new(),
                }
            };
            let categories = bound_array("categories");
            let products = bound_array("products");
            let item_row = |p: &Value| -> AnyView {
                let name = p.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                let description =
                    p.get("description").and_then(Value::as_str).unwrap_or("").to_string();
                let price = p
                    .get("offers")
                    .and_then(Value::as_array)
                    .and_then(|offers| offers.first())
                    .and_then(|offer| offer.get("price"))
                    .map(format_currency)
                    .unwrap_or_default();
                let has_description = !description.is_empty();
                view! {
                    <article data-c="catalog_item_row">
                        <span data-c="item_name">{name}</span>
                        {has_description
                            .then(|| view! { <p data-c="item_description">{description.clone()}</p> })}
                        <span data-c="item_price">{price}</span>
                    </article>
                }
                .into_any()
            };
            let mut used: BTreeSet<usize> = BTreeSet::new();
            let mut sections: Vec<AnyView> = Vec::new();
            for c in &categories {
                let cid = c.get("id").and_then(Value::as_str).unwrap_or("");
                let cname = c.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                let mut rows: Vec<AnyView> = Vec::new();
                for (i, p) in products.iter().enumerate() {
                    if !cid.is_empty()
                        && p.get("categoryRef").and_then(Value::as_str) == Some(cid)
                    {
                        used.insert(i);
                        rows.push(item_row(p));
                    }
                }
                sections.push(
                    view! { <section data-c="catalog_category"><h2>{cname}</h2>{rows}</section> }
                        .into_any(),
                );
            }
            let rest: Vec<AnyView> = products
                .iter()
                .enumerate()
                .filter(|(i, _)| !used.contains(i))
                .map(|(_, p)| item_row(p))
                .collect();
            view! { <div data-c=ty>{sections}{rest}</div> }.into_any()
        }

        // ── declared, no renderer arm yet (GAP(renderer)) ────────────────────────────────
        //
        // `render_node_kind` is now EXHAUSTIVE over `ComponentKind` (no `_` wildcard): the
        // compiler enumerated exactly the 46 kinds below the moment the old wildcard was
        // removed, replacing the issue's own "eleven" — a number stated with no antecedent
        // (ADR-20260817-105845) and off by 35 (see the branch's hand-back for the E0004 quote).
        // Every kind here renders BYTE-IDENTICAL to the old wildcard's tagged container
        // (title/label/value text, children, `data-group`) PLUS one BARE marker attribute
        // (`data-no-arm`, no value — evans/observability/reviewer: a ticket id baked into
        // customer-facing HTML names a concept after the ticket that found it, and the bare
        // attribute audits identically) so an audit can grep the DOM for what still has no
        // dedicated arm. `data-no-arm` is the DOM spelling of the specs' `GAP(renderer)`; the
        // DOM could not take `data-gap`, which already means no-backing-query (renderer.rs:1066).
        // NO `data-action` / `data-vars` / `data-trigger` here — an inert control must never
        // look wired (CLAUDE.md).
        //
        // `Badge` is listed a SECOND time, unguarded: its dedicated arm above is GUARDED
        // (`if node.prop("text").is_some()`), and a match-arm guard never counts towards
        // exhaustivity (rustc 1.98.1: `match arms with guards don't count towards exhaustivity`)
        // — so a `badge` node declared without `text` still needs somewhere to land. This guard
        // gap is invisible while any OTHER kind is fully unmatched (rustc reports only the fully-
        // uncovered patterns then) and only surfaces once those 45 are closed — found by doing
        // exactly that and re-running the compiler, not read off the first error message.
        //
        // `text_area` and `tip_amount_selector` stay here ON PURPOSE (thirteen-lens consent,
        // ADR-20260904-013834 — a split resolved by the safer option on a legal-adjacent
        // surface). `text_area` now has exactly ONE surviving reason (holub: D2 already killed
        // the read leg — `interact.rs input_value` reads a `<textarea>` too): three of its six
        // DSL sites bind to NO mutation (`delivery_reason`, `delivery_instructions`;
        // `item_instructions` rides `add_to_cart.instructions` but `CartLineInput` has no such
        // field). Binding `item_instructions` needs a new field threaded through codegen, the
        // GraphQL resolver, persistence and the kitchen ticket (`crates/server`) — an
        // unquantified cost, not a promised one-commit follow-up (#934 item 1 tracks it).
        // `tip_amount_selector` stays deferred to #887.
        ComponentKind::Screen
        | ComponentKind::Spacer
        | ComponentKind::Divider
        | ComponentKind::ToastNotification
        | ComponentKind::Overlay
        | ComponentKind::Badge
        | ComponentKind::BadgeRow
        | ComponentKind::RatingBadge
        | ComponentKind::DotSeparator
        | ComponentKind::PromoCard
        | ComponentKind::HeroSection
        | ComponentKind::HeroSearchBar
        | ComponentKind::SearchBarActive
        | ComponentKind::FilterBar
        | ComponentKind::CategoryPill
        | ComponentKind::CategoryTile
        | ComponentKind::CategoryGrid
        | ComponentKind::RestaurantCard
        | ComponentKind::DishRow
        | ComponentKind::StickyCategoryNav
        | ComponentKind::CatalogItemRow
        | ComponentKind::ItemThumbnail
        | ComponentKind::ItemHeader
        | ComponentKind::OptionGroups
        | ComponentKind::QuantitySelector
        | ComponentKind::CartLineRow
        | ComponentKind::QuantityStepper
        | ComponentKind::PromoCodeInput
        | ComponentKind::DeliveryModeToggle
        | ComponentKind::AddressSelector
        | ComponentKind::Form
        | ComponentKind::TextArea
        | ComponentKind::StripeExpressCheckoutElement
        | ComponentKind::OrderStatusHero
        | ComponentKind::EtaBar
        | ComponentKind::OrderTimeline
        | ComponentKind::RestaurantContactRow
        | ComponentKind::OrderItemsSummary
        | ComponentKind::OrderIdRow
        | ComponentKind::OrderCard
        | ComponentKind::AccountHeader
        | ComponentKind::AvatarButton
        | ComponentKind::LocationPill
        | ComponentKind::Countdown
        | ComponentKind::StarRating
        | ComponentKind::TipAmountSelector => {
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
            view! { <div data-c=ty data-group=group data-no-arm=true>{text}{children_views(node, ctx)}</div> }
                .into_any()
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
    let mut ctx = ctx;
    // #730: the screen-level pass owns the error-granularity assignment (SSR and hydrate both
    // enter here, so the two renders cannot disagree on where a failure surfaces).
    ctx.assign_error_anchors(screen, sheets);
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
    // #904 D3 (ADR-20260905-101349 §13): capture a `?next=` this load's URL is carrying, BEFORE
    // anything else runs — every screen load is a candidate (see `next_param`'s doc for why this
    // never needs a "is this the sign-in screen" allowlist).
    crate::next_param::store_next_once(&location);
    let current_path_and_query =
        format!("{path}{}", location.search().unwrap_or_default());
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

    // R1 (#639 2c-ii): the role path is the SCREEN's — its declared `graphql_role` when it has
    // one (the rider sign-in door speaks to `/public/graphql`), else the surface's own role. Both
    // transports of this page (reads here, writes + push socket in `interact`) are built from the
    // same answer, so a screen can never read as one role and write as another.
    let role = surface.role_for(screen);
    // #904 (ADR-20260905-101349 §13, the member door's flip precondition): ONE one-shot-refresh
    // budget for the whole page load, shared between this load's reads below and
    // `interact::install`'s later mutation dispatches -- a refresh failure is remembered for the
    // PAGE (`graphql::RefreshingTransport`'s doc comment), not just for the read loop.
    let refresh_used = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let transport = crate::graphql::RefreshingTransport::new(
        Box::new(crate::graphql::HttpTransport::new(&origin, role, session)),
        Box::new(crate::graphql::HttpRefresher::new(&origin, session)),
        std::sync::Arc::clone(&refresh_used),
    );

    // The interaction layer (#93): delegated button dispatch + push socket + boot pending-resume.
    // `screen` (#639 4-ii): the bounce decision on a refused Tell needs the SAME screen's declared
    // routes the hydrate loop above reads.
    crate::interact::install(&origin, role, session, screen, std::sync::Arc::clone(&refresh_used));

    let sheets = surface.sheets();
    wasm_bindgen_futures::spawn_local(async move {
        let mut ctx = RenderContext::new(&locale);
        // #639 part C step 4-ii (ADR-20260904-124600 §2), extended by #904 D2: the bounce decision
        // is now `crate::bounce::bounce_target` — a 401 (no session, the 2c-ii leg) or a refused
        // read carrying `extensions.reason == RIDER_RESTRICTED` both resolve through it; the first
        // read that answers either signal decides where this screen sends its visitor, and a bare
        // 401 on a `requires_auth` screen carries `?next=` back to itself (#904).
        let mut bounce_route: Option<String> = None;
        for resolver in screen.data_requirements {
            // #745: the generated §25b skip table — a structurally unfulfillable read (required
            // arg, no paint-time source, declared on the binding) is skipped before any network
            // on the hydrate leg exactly as on SSR: the two paths share the verdict table, so
            // they cannot disagree about which reads run.
            if screen.skipped_reads.contains(resolver) {
                continue;
            }
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
            if bounce_route.is_none() {
                if let Err(crate::graphql::ResolverError::Transport(t)) = &result {
                    bounce_route = crate::bounce::bounce_target(t, screen, &current_path_and_query);
                }
            }
            match crate::graphql::classify_resolve(role, *resolver, result) {
                crate::graphql::ResolveOutcome::Resolved(value) => {
                    ctx.insert_resolved(resolver.as_str(), value)
                }
                crate::graphql::ResolveOutcome::SkippedByDesign(_) => {}
                crate::graphql::ResolveOutcome::Failed(_) => ctx.insert_failed(resolver.as_str()),
            }
        }
        leptos::mount::mount_to_body(move || SduiScreen(SduiScreenProps { screen, sheets, ctx }));

        // The declared bounce first (#639 2c-ii, extended 4-ii): a refused read whose signal names
        // a door sends the visitor there instead of painting a shell over it — the server already
        // 302s a cookie-less GET, this is the leg for a cookie that no longer verifies OR a
        // standing that flipped since the last paint. A visitor whose reads answered has neither
        // signal and stays.
        if let Some(route) = bounce_route {
            if let Some(w) = web_sys::window() {
                let _ = w.location().set_href(&route);
            }
            return;
        }

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
            Surface::System,
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

    /// Round 3 R3-1 (ux, BLOCKING, `#870` class -- see the comment at line ~1751): the
    /// `member_sign_in_confirmation_sheet` used to echo `{{ member_email.value }}` in a `text`
    /// node right after `confirmation_body`. A `text` node resolves at PAINT from RESOLVER data
    /// (`text_of` -> `RenderContext::lookup`); a form field's `.value` exists only at DISPATCH,
    /// for ACTION variables (`interact.rs:~251-272`) -- there is no resolver named `member_email`
    /// this SSR/render context ever populates, so the binding always resolved to nothing and the
    /// panel painted the sentence followed by an EMPTY `<p data-c="text"></p>`. The surface's
    /// sheets render unconditionally (HIDDEN, `#94`) alongside every screen of that surface, so
    /// rendering `sign_in` is enough to reach the confirmation panel without any dispatch.
    #[test]
    fn member_sign_in_confirmation_panel_has_no_empty_echoed_address() {
        let screen = Surface::RestaurantBackoffice.screens().iter().find(|s| s.id == "sign_in").unwrap();

        let fr = render_screen_html(screen, Surface::RestaurantBackoffice.sheets(), RenderContext::new("fr"));
        assert!(
            fr.contains("Si cette adresse est enregistrée, un lien vient d'être envoyé à cette adresse."),
            "fr confirmation panel must carry the self-contained sentence: {fr}"
        );
        assert!(
            !fr.contains("<p data-c=\"text\"> </p>"),
            "confirmation panel must never paint an empty text node: {fr}"
        );

        let en = render_screen_html(screen, Surface::RestaurantBackoffice.sheets(), RenderContext::new("en"));
        assert!(
            en.contains("If this address is registered, a link has just been sent to it."),
            "en confirmation panel must carry the self-contained sentence: {en}"
        );
        assert!(
            !en.contains("<p data-c=\"text\"> </p>"),
            "confirmation panel must never paint an empty text node: {en}"
        );
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

    /// Review round 2 on #870: `rider_issue_sheet` used to nest its real content behind
    /// `issue_exit.value` — a FORM FIELD `visible_when`. `RenderContext::lookup` reads resolver
    /// data only, so the condition always evaluated to nothing, `visible_when` fails CLOSED, and
    /// `interact.rs` never re-evaluates conditions after a chip pick: neither exit's content ever
    /// rendered. Fixed by splitting `rider_issue_sheet` into a ROUTER (two `open_bottom_sheet`
    /// buttons — a real SDUI edge, not a form-field condition) and two content sheets
    /// (`rider_report_sheet`, `rider_handback_sheet`) whose own gates are RESOLVER data
    /// (`delivery.status`), which `lookup` DOES serve at paint. This test renders `job_detail` (the
    /// only screen reaching these sheets) and asserts every confirm control is actually present in
    /// the DOM, plus the ASSIGNED-status food-cards-absent case the ADR's "derive, don't ask" rule
    /// (§2) requires.
    #[test]
    fn the_issue_router_and_its_two_child_sheets_render_their_confirm_controls() {
        let screen = Surface::Rider.screens().iter().find(|s| s.id == "job_detail").unwrap();
        let delivery = |status: &str| {
            json!({
                "id": "d-1",
                "status": status,
                // The REAL shape (#882 R2 item 1b) — `Address`, not a bare string; an unfiltered
                // binding used to fall through to `format_currency`'s Money-shaped read and
                // silently render "", and a plain-string fixture masked exactly that.
                "pickupAddress": { "line1": "12 rue de la Paix", "postalCode": "37000", "city": "Tours" },
                "dropoffAddress": { "line1": "4 avenue Foch", "postalCode": "37000", "city": "Tours" },
                "foodLocation": null,
                "openIssue": null,
                "restaurant": { "displayName": "Chez Test" },
            })
        };

        // ASSIGNED — the rider never picked up: `foodLocation` is DERIVED (NOT_COLLECTED), so the
        // handback sheet must ask NOTHING — the food cards (`handback_location`) must be ABSENT.
        let mut c = ctx();
        c.insert_resolved("delivery.byOrder", delivery("ASSIGNED"));
        let html = render_screen_html(screen, Surface::Rider.sheets(), c);
        assert!(html.contains("data-sheet=\"rider_report_sheet\""), "router: continue exit missing -- {html}");
        assert!(html.contains("data-sheet=\"rider_handback_sheet\""), "router: hand-back exit missing -- {html}");
        assert!(html.contains("data-chip-group=\"issue_kind\""), "report sheet: kind chips missing -- {html}");
        assert!(html.contains("data-action=\"report_delivery_issue\""), "report sheet: confirm missing -- {html}");
        assert!(html.contains("data-action=\"hand_back_delivery\""), "handback sheet: confirm missing on ASSIGNED -- {html}");
        assert!(
            !html.contains("data-chip-group=\"handback_location\""),
            "ASSIGNED: food cards must be absent (derived, never asked) -- {html}"
        );
        // #882 R2 item 1b: `pickupAddress`/`dropoffAddress` are `Address` objects, formatted via
        // `| format_address` — never a raw `[object Object]`-shaped fall-through.
        assert!(html.contains("12 rue de la Paix, 37000 Tours"), "pickup address must format: {html}");
        assert!(html.contains("4 avenue Foch, 37000 Tours"), "dropoff address must format: {html}");

        // PICKED_UP — collected: the food cards ask WITH_RIDER vs RETURNED_TO_RESTAURANT.
        let mut c2 = ctx();
        c2.insert_resolved("delivery.byOrder", delivery("PICKED_UP"));
        let html2 = render_screen_html(screen, Surface::Rider.sheets(), c2);
        assert!(html2.contains("data-chip-group=\"handback_location\""), "PICKED_UP: food cards missing -- {html2}");
        assert!(html2.contains("data-action=\"hand_back_delivery\""), "handback sheet: confirm missing on PICKED_UP -- {html2}");
    }

    /// Round 3 (#639 part C step 4-iii-A, `screen-condition-on-form-field`): `rider_report_sheet`'s
    /// `issue_note` (`text_area`, `visible_when: "issue_kind.value == 'OTHER'"`) was DELETED rather
    /// than un-conditioned — `text_area` has no renderer arm (falls to the generic catch-all,
    /// `data-c=ty`) and would have painted an inert empty div regardless of which kind chip was
    /// picked. This proves the sheet still fully works with no kind chosen: all SIX
    /// `DeliveryIssueKind` chips (the spec's real count — `ADDRESS_NOT_FOUND`,
    /// `CUSTOMER_UNREACHABLE`, `RESTAURANT_NOT_READY`, `FOOD_DAMAGED`, `VEHICLE_OR_INJURY`,
    /// `OTHER`), the confirm button, and NO leftover `text_area` node (positive + negative, per
    /// CLAUDE.md's "never a bare `!contains` alone").
    #[test]
    fn the_rider_issue_sheet_has_no_free_text_note_and_renders_its_kind_chips_and_confirm() {
        let screen = Surface::Rider.screens().iter().find(|s| s.id == "job_detail").unwrap();
        let mut c = ctx();
        c.insert_resolved(
            "delivery.byOrder",
            json!({
                "id": "d-1",
                "status": "ASSIGNED",
                "pickupAddress": { "line1": "12 rue de la Paix", "postalCode": "37000", "city": "Tours" },
                "dropoffAddress": { "line1": "4 avenue Foch", "postalCode": "37000", "city": "Tours" },
                "foodLocation": null,
                "openIssue": null,
                "restaurant": { "displayName": "Chez Test" },
            }),
        );
        let html = render_screen_html(screen, Surface::Rider.sheets(), c);
        assert_eq!(html.matches("data-chip-group=\"issue_kind\"").count(), 6, "all six kind chips must render: {html}");
        assert!(html.contains("data-action=\"report_delivery_issue\""), "confirm button missing -- {html}");
        assert!(!html.contains("data-c=\"text_area\""), "the deleted free-text note must never render (no renderer arm) -- {html}");
    }

    /// R3-3 (#639 part C step 4-iii-A round 3, ux + reviewer): `claim_resolve`'s `refund_amount`
    /// (`tip_amount_selector`) was rendered unconditionally in round 2 — but `tip_amount_selector`
    /// ALSO has no renderer arm (falls to the generic catch-all: a bare "Montant" label, no presets,
    /// no input, no `data-action`), so this put an INERT MONEY-PATH control on a LIVE screen. DELETED
    /// rather than left inert. Asserts the resolve button still dispatches `resolve_reclamation`
    /// (`resolve_btn`'s own action, unaffected by the deletion) AND no `tip_amount_selector` node
    /// remains (positive + negative — quoted RED before the deletion in the round-3 hand-back).
    #[test]
    fn the_claim_resolve_screen_has_no_inert_amount_picker_and_still_resolves() {
        let screen = Surface::RestaurantBackoffice.screens().iter().find(|s| s.id == "claim_resolve").unwrap();
        let mut c = ctx();
        c.insert_resolved(
            "reclamation.byId",
            json!({
                "reclamationId": "rec-1",
                "orderId": "o-1",
                "category": "FOOD_QUALITY",
                "description": "Cold food",
                "status": "OPEN",
                "resolution": null,
                "refundAmount": null,
                "rejectReason": null,
                "overdue": false,
            }),
        );
        let html = render_screen_html(screen, Surface::RestaurantBackoffice.sheets(), c);
        assert!(html.contains("data-action=\"resolve_reclamation\""), "resolve button missing -- {html}");
        assert!(!html.contains("data-c=\"tip_amount_selector\""), "the deleted inert amount picker must never render (no renderer arm) -- {html}");
    }

    /// #639 part C step 4-ii (ADR-20260904-124600 §4): a `standing.mine` fixture per ground, `fr`
    /// locale — every ground's own sentence, BOTH formatted dates (never a raw ISO instant), the
    /// contact address, "contester" in the footer, no `rider_toggle_online` control (no
    /// `rider_topbar`) anywhere on this screen.
    #[test]
    fn the_restricted_notice_shows_both_dates_and_the_contact_for_every_ground_and_the_catch_all() {
        let screen = Surface::Rider.screens().iter().find(|s| s.id == "restricted").unwrap();
        let standing = |ground: Option<&str>| {
            json!({
                "standing": "RESTRICTED",
                "restriction": { "ground": ground, "decidedAt": "2026-09-04T14:02:00Z", "effectiveAt": "2026-09-04T14:02:00Z" },
                "heldDelivery": null,
                "contestContact": "support@captain.food",
            })
        };
        for (ground, fr_fragment) in [
            (Some("RIDER_REQUESTED"), "À votre demande"),
            (Some("ELIGIBILITY_DOCUMENT_LAPSED"), "Justificatif expiré"),
            (Some("IDENTITY_MISMATCH"), "Identité non concordante"),
            (Some("ACCOUNT_COMPROMISE"), "Sécurité du compte"),
            (None, "Motif non reconnu"),
        ] {
            let mut c = RenderContext::new("fr");
            c.insert_resolved("standing.mine", standing(ground));
            let html = render_screen_html(screen, Surface::Rider.sheets(), c);
            assert!(html.contains(fr_fragment), "ground {ground:?}: missing '{fr_fragment}' -- {html}");
            // Both dates, formatted (never a raw ISO instant leaking through) — "4 sept. 2026" for
            // both decidedAt and effectiveAt (equal in V0, both shown per ADR-081527 §5). 14:02Z
            // is 16:02 Europe/Paris in September (CEST, UTC+2) — the conversion IS the assertion.
            assert_eq!(html.matches("4 sept. 2026, 16:02").count(), 2, "both dates must render, converted to Europe/Paris: {html}");
            assert!(!html.contains("2026-09-04T14:02:00Z"), "no raw ISO instant may leak through: {html}");
            // #882 R2 item 4: both LABELS, not merely both VALUES — a label dropped from either
            // `info_row` would still pass a bare value-only assertion.
            assert!(html.contains("Décidé le"), "the decided-at label must render: {html}");
            assert!(html.contains("Effectif depuis"), "the effective-at label must render: {html}");
            // The contact renders TWICE — once inside the ground sentence, once in the footer —
            // so a contact dropped from EITHER branch is caught (a bare `contains` would still
            // pass with only one of the two, M6's exact trap).
            assert_eq!(html.matches("support@captain.food").count(), 2, "the contact must render in both the ground sentence and the footer: {html}");
            assert!(html.contains("contester"), "the footer's contest sentence must render: {html}");
            assert!(!html.contains("data-action=\"rider_toggle_online\""), "no rider_topbar on the restricted screen: {html}");
            // #882 R2 item 7: the split legal sentence (lead + bound address + trail, three
            // `<p data-c="text">` children of ONE `<div data-c="row">`) must read as ONE
            // continuous line, never one fragment per visual row — the inlined `app.css` (SSR'd
            // in the SAME document, `renderer.rs:1469-1471`) must carry the flex rule that makes
            // it flow.
            assert!(
                html.contains("[data-c=\"row\"] { display: flex; flex-wrap: wrap;"),
                "the row must lay its text fragments out horizontally, not one per line: {html}"
            );
        }
    }

    /// The transient row (`standing.restriction == null`, the documented one-tick lag between the
    /// `Rider.standing` and `RiderRestriction` checkpoints, ADR-20260904-081527 §4/§9) never
    /// renders blank: the sentence AND the (unconditional) contact both show.
    #[test]
    fn the_details_pending_transient_never_renders_blank() {
        let screen = Surface::Rider.screens().iter().find(|s| s.id == "restricted").unwrap();
        let mut c = RenderContext::new("fr");
        c.insert_resolved(
            "standing.mine",
            json!({ "standing": "RESTRICTED", "restriction": null, "heldDelivery": null, "contestContact": "support@captain.food" }),
        );
        let html = render_screen_html(screen, Surface::Rider.sheets(), c);
        assert!(html.contains("Détails de la restriction pas encore disponibles"), "{html}");
        assert!(html.contains("support@captain.food"), "the contact stays unconditional: {html}");
        // Never the ground/date rows (the OTHER branch of the same conditional_section).
        assert!(!html.contains("Décidé le"), "the transient must not show the resolved attribution: {html}");
    }

    /// The held-job card + its second sheet: the ONE control opens `rider_restricted_handback_sheet`
    /// and dispatches `hand_back_delivery` carrying the held job's id — bound to
    /// `standing.heldDelivery.*`, never `myStanding.*` / `delivery.*` (the card-defect ADR banked:
    /// no screen-level alias grammar exists). Absent held job: neither the card nor its control.
    /// `foodLocation` set: the after-state text, no control.
    #[test]
    fn the_held_job_card_and_its_sheet_dispatch_hand_back_delivery() {
        let screen = Surface::Rider.screens().iter().find(|s| s.id == "restricted").unwrap();
        let standing_with = |held: Value| {
            json!({
                "standing": "RESTRICTED",
                "restriction": { "ground": "RIDER_REQUESTED", "decidedAt": "2026-09-04T14:02:00Z", "effectiveAt": "2026-09-04T14:02:00Z" },
                "heldDelivery": held,
                "contestContact": "support@captain.food",
            })
        };

        // Held, still with the rider (foodLocation null): the control + the sheet's dispatch.
        let mut c = RenderContext::new("fr");
        c.insert_resolved(
            "standing.mine",
            standing_with(json!({
                "id": "d-1", "status": "PICKED_UP", "foodLocation": null,
                // The REAL shape (#882 R2 item 1b) — `Address`, not a bare string.
                "pickupAddress": { "line1": "12 rue de la Paix", "postalCode": "37000", "city": "Tours" },
                "restaurant": { "displayName": "Chez Test" },
            })),
        );
        let html = render_screen_html(screen, Surface::Rider.sheets(), c);
        assert!(html.contains("Vous avez encore une commande"), "{html}");
        assert!(html.contains("data-sheet=\"rider_restricted_handback_sheet\""), "{html}");
        assert!(html.contains("data-action=\"hand_back_delivery\""), "{html}");
        // #882 R2 item 6: sharpened from a bare `contains("d-1")` (which would also match a
        // mis-bound `delivery.id` fixture, M3's exact trap) to the `data-vars` PAYLOAD carrying
        // the held job's id under its own key.
        assert!(
            html.contains("&quot;deliveryJobId&quot;:&quot;d-1&quot;"),
            "the sheet's data-vars must carry the held job's id: {html}"
        );
        assert!(html.contains("Chez Test"), "the restaurant name (FK nav edge): {html}");
        assert!(html.contains("12 rue de la Paix, 37000 Tours"), "the formatted pickup address: {html}");

        // #882 R2 item 5: the SECOND sheet's ASSIGNED arm asks the rider nothing about the food —
        // `foodLocation` is the literal `NOT_COLLECTED` (ADR-20260904-015903 §2), never a chip
        // read, and it must actually reach `data-vars`, not merely the button's `visible_when`.
        let mut c_assigned = RenderContext::new("fr");
        c_assigned.insert_resolved(
            "standing.mine",
            standing_with(json!({
                "id": "d-1", "status": "ASSIGNED", "foodLocation": null,
                "pickupAddress": { "line1": "12 rue de la Paix", "postalCode": "37000", "city": "Tours" },
                "restaurant": { "displayName": "Chez Test" },
            })),
        );
        let html_assigned = render_screen_html(screen, Surface::Rider.sheets(), c_assigned);
        assert!(html_assigned.contains("data-action=\"hand_back_delivery\""), "{html_assigned}");
        assert!(
            html_assigned.contains("&quot;foodLocation&quot;:&quot;NOT_COLLECTED&quot;"),
            "the ASSIGNED arm's data-vars must carry the literal NOT_COLLECTED: {html_assigned}"
        );
        assert!(
            html_assigned.contains("&quot;deliveryJobId&quot;:&quot;d-1&quot;"),
            "the ASSIGNED arm's data-vars must also carry the held job's id: {html_assigned}"
        );

        // No held job: neither the card nor the control.
        let mut c2 = RenderContext::new("fr");
        c2.insert_resolved("standing.mine", standing_with(Value::Null));
        let html2 = render_screen_html(screen, Surface::Rider.sheets(), c2);
        assert!(!html2.contains("Vous avez encore une commande"), "{html2}");

        // Handed back (foodLocation set): the after-state, no control ever live on a job this
        // rider no longer holds.
        let mut c3 = RenderContext::new("fr");
        c3.insert_resolved(
            "standing.mine",
            standing_with(json!({
                "id": "d-1", "status": "PICKED_UP", "foodLocation": "RETURNED_TO_RESTAURANT",
                "pickupAddress": { "line1": "12 rue de la Paix", "postalCode": "37000", "city": "Tours" },
                "restaurant": { "displayName": "Chez Test" },
            })),
        );
        let html3 = render_screen_html(screen, Surface::Rider.sheets(), c3);
        assert!(html3.contains("Course rendue. Le restaurant est prévenu."), "{html3}");
        assert!(
            !html3.contains("Rapportez la commande au restaurant."),
            "the instruction must not show once handed back: {html3}"
        );
        // The control button itself must be gone (`data-sheet="…"` on the BUTTON — distinct from
        // the sheet's own always-mounted `data-sheet-id="…"`): no control is ever live on a job
        // this rider no longer holds.
        assert!(
            !html3.contains("data-sheet=\"rider_restricted_handback_sheet\""),
            "the control must not render once handed back: {html3}"
        );
    }

    /// #882 round-2 item 2 (ADR-20260904-081527 §7, verbatim): a reinstated (ACTIVE) rider who
    /// lands on `/restricted` (back-navigation, a stale `$reload` after `ReinstateRider`) must
    /// NEVER read the restricted notice — the whole notice body is gated on the rider's OWN
    /// current standing, and the else branch is the reinstated sentence + the one control back to
    /// the job list. M2's exact trap: a wrong-root `visible_when` fails CLOSED (neither branch
    /// renders), which this test's negative assertions alone would not catch — the POSITIVE
    /// assertion on the reinstated sentence is what proves the if_false branch actually fired.
    #[test]
    fn a_reinstated_rider_on_restricted_reads_the_restored_sentence_not_the_notice() {
        let screen = Surface::Rider.screens().iter().find(|s| s.id == "restricted").unwrap();
        let mut c = RenderContext::new("fr");
        c.insert_resolved(
            "standing.mine",
            json!({
                "standing": "ACTIVE",
                "restriction": null,
                "heldDelivery": null,
                "contestContact": "support@captain.food",
            }),
        );
        let html = render_screen_html(screen, Surface::Rider.sheets(), c);
        assert!(!html.contains("restreint"), "an ACTIVE rider must never read the restricted copy: {html}");
        assert!(!html.contains("Vous ne recevrez plus de courses."), "{html}");
        assert!(html.contains("Votre accès est rétabli."), "the reinstated sentence must render, verbatim: {html}");
        assert!(html.contains("data-route=\"/\""), "the control must navigate to \"/\": {html}");
        assert!(html.contains("Retour aux courses"), "the control reuses the back-to-jobs label: {html}");

        // The RESTRICTED tests above are unchanged: re-confirm a RESTRICTED standing never shows
        // the reinstated sentence.
        let mut c2 = RenderContext::new("fr");
        c2.insert_resolved(
            "standing.mine",
            json!({
                "standing": "RESTRICTED",
                "restriction": { "ground": "RIDER_REQUESTED", "decidedAt": "2026-09-04T14:02:00Z", "effectiveAt": "2026-09-04T14:02:00Z" },
                "heldDelivery": null,
                "contestContact": "support@captain.food",
            }),
        );
        let html2 = render_screen_html(screen, Surface::Rider.sheets(), c2);
        assert!(!html2.contains("Votre accès est rétabli."), "{html2}");
        assert!(html2.contains("Vous ne recevrez plus de courses."), "{html2}");
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
        // restaurants.featured → alias featured_restaurants (the template name on home). Since
        // #729 the data map holds the FULL key only; the alias is resolved at read time.
        let mut c = ctx();
        c.insert_resolved(
            "restaurants.featured",
            json!([{ "displayName": "Chez Test", "slug": "chez-test", "address": { "city": "Tours" } }]),
        );
        let home = Surface::CaptainFrontoffice.screens().iter().find(|s| s.id == "home").unwrap();
        let html = render_screen_html(home, Surface::CaptainFrontoffice.sheets(), c);
        assert!(html.contains("Chez Test"), "{html}");
        assert!(html.contains("data-slug=\"chez-test\""));
    }

    /// #729 parity rule (graphql's second alias-derivation defect made a rule): for EVERY
    /// resolver key, the names a resolved answer feeds and the names a failure mark matches are
    /// IDENTICAL — the full key, its first-segment alias, and (when derived) its reversed alias.
    /// A binding a resolver can answer under but not fail under (or vice versa) is exactly how
    /// the mailbox bindings sat dormant.
    #[test]
    fn resolver_key_parity_between_data_and_failure_marks() {
        use crate::generated::data_layer::ResolverKey;
        for key in ResolverKey::ALL {
            let key = key.as_str();
            let mut answered = ctx();
            answered.insert_resolved(key, json!({ "probe": "x" }));
            answered.insert_failed(key); // the re-read-failed shape: the answer must win
            let mut failed = ctx();
            failed.insert_failed(key);

            let mut names: Vec<String> = vec![key.to_string()];
            let mut parts = key.splitn(2, '.');
            let first = parts.next().unwrap_or(key).to_string();
            if let Some(second) = parts.next() {
                if !second.is_empty() && second.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                {
                    names.push(format!("{second}_{first}"));
                }
            }
            names.push(first);
            for name in names {
                assert!(
                    answered.binding_json(&name).is_some(),
                    "{key}: an answer must feed `{{{{ {name} }}}}`"
                );
                assert!(
                    failed.binding_failed(&name),
                    "{key}: a failure mark must match `{{{{ {name} }}}}`"
                );
                assert!(
                    !answered.binding_failed(&name),
                    "{key}: an answer beats a failure mark for `{{{{ {name} }}}}`"
                );
            }
        }
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
            branches: &[],
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
            branches: &[],
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
            branches: &[],
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
            branches: &[],
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

    /// #725 (beck): the backoffice claims list's `item_badge` is per-item CONFIG (correctly
    /// flattened, no `type`) the List arm used to ignore — the overdue badge (#160) never
    /// rendered. It renders per item, honoring its condition against THAT row: two rows,
    /// overdue true/false → the badge on exactly one.
    #[test]
    fn list_item_badge_renders_per_item_honoring_its_condition() {
        let node = Node {
            kind: ComponentKind::List,
            props: &[
                ("items", PropValue::Binding("reclamations")),
                ("item_badge.text", PropValue::Text("OVERDUE-BADGE")),
                ("item_badge.visible_when", PropValue::Text("item.overdue")),
                ("item_badge.variant", PropValue::Text("warning")),
            ],
            children: &[],
            branches: &[],
        };
        let mut c = ctx();
        c.insert_resolved(
            "reclamations",
            json!([
                { "reclamationId": "c-1", "overdue": true },
                { "reclamationId": "c-2", "overdue": false },
            ]),
        );
        let html = node_html(&node, &c);
        assert_eq!(
            html.matches("OVERDUE-BADGE").count(),
            1,
            "the badge renders on exactly the overdue row: {html}"
        );
        assert!(html.contains("data-variant=\"warning\""), "{html}");
    }

    /// #725: the system mailbox lists declare per-item `item_components` TEMPLATES — the same
    /// flattening class as `item_badge` — which the List arm ignored, leaving the poisoned-row
    /// detail (and its ONE intervention, the allowlisted `requeue_mailbox_message` button,
    /// system.yaml:48/#315) structurally unrenderable. They render per row against THAT row's
    /// data, the button carrying the resolved action DOM contract.
    #[test]
    fn mailbox_item_components_render_per_row_and_the_requeue_button_dispatches() {
        let screen = crate::generated::screens::system::SCREENS
            .iter()
            .find(|s| s.id == "mailbox_lanes")
            .expect("system mailbox screen");
        let mut c = ctx();
        // Through the REAL resolver keys (#729): the spec now binds the derived reversed aliases
        // (`{{ lanes_mailbox }}`/`{{ poisoned_mailbox }}` — the former `{{ mailbox_lanes }}`
        // spelling matched no derived alias and lay dormant, journal W35), so this hydrates
        // exactly as `insert_resolved` would in production.
        c.insert_resolved(
            "mailbox.lanes",
            json!([{ "actorType": "ORDER", "partition": "p-7", "registration": "SEEDED",
                     "claimedBy": "w-1", "leaseUntil": "2026-08-29T02:00:00Z",
                     "ownershipVersion": 4, "checkpoint": "m-100", "pending": 3, "scheduled": 0,
                     "oldestPendingAt": "2026-08-29T01:00:00Z" }]),
        );
        c.insert_resolved(
            "mailbox.poisoned",
            json!([{ "messageType": "PlaceOrder", "actorType": "ORDER", "partition": "p-7",
                     "messageId": "m-poison-1", "attempts": 5, "errorCode": "CAP_EXCEEDED",
                     "receivedAt": "2026-08-29T00:00:00Z" }]),
        );
        let html = render_screen_html(screen, &[], c);
        // The lane row renders its per-item values — including the MULTI-template lane label
        // ("{{ item.actorType }} / {{ item.partition }}"), which pre-#725 mis-lexed into a
        // garbage single binding and rendered empty.
        assert!(html.contains("ORDER / p-7"), "lane label interpolates per row: {html}");
        assert!(html.contains("m-poison-1"), "the poisoned messageId renders: {html}");
        // The requeue button carries the action DOM contract with the ROW's id resolved.
        assert!(html.contains("data-action=\"requeue_mailbox_message\""), "{html}");
        assert!(
            html.contains("&quot;targetMessageId&quot;:&quot;m-poison-1&quot;"),
            "resolved per-row vars JSON: {html}"
        );
        // Fail-closed per-item condition: no errorCode == null row here, so the error info_row
        // renders (visible_when "item.errorCode != null" is true for the poisoned row).
        assert!(html.contains("CAP_EXCEEDED"), "{html}");
    }

    /// Round 3 (#639 part C step 6-iv, ux BLOCKING): before this round the `/team` invitation
    /// list's "Retirer" button rendered on EVERY row — ACCEPTED/ACCEPTED_PENDING_ACCESS/REVOKED/
    /// EXPIRED included — because `visible_when` alone can only express ONE comparison
    /// (`roster.viewerAuthority == 'MANAGER'`), never a compound "PENDING and MANAGER". Nesting a
    /// `conditional_section` (`condition: "item.status == 'PENDING'"`) around it — the corpus's
    /// only conjunction grammar (`rider.yaml`'s conditional_section-in-conditional_section) — is
    /// what [`render_item_entry`]'s new `"conditional_section"` arm exists to make real: two rows,
    /// one PENDING and one REVOKED, and the button renders on exactly the PENDING one.
    #[test]
    fn team_invitation_row_revoke_button_renders_only_on_a_pending_row() {
        let screen = crate::generated::screens::restaurant_backoffice::SCREENS
            .iter()
            .find(|s| s.id == "team")
            .expect("the /team screen");
        let mut c = ctx();
        c.insert_resolved(
            "roster.mine",
            json!({ "items": [], "viewerAuthority": "MANAGER" }),
        );
        c.insert_resolved(
            "invitations.mine",
            json!([
                { "invitationId": "inv-pending", "invitedEmail": "a@example.com",
                  "authority": "OPERATOR", "status": "PENDING",
                  "expiresAt": "2026-09-12T00:00:00Z", "createdAt": "2026-09-05T00:00:00Z" },
                { "invitationId": "inv-revoked", "invitedEmail": "b@example.com",
                  "authority": "OPERATOR", "status": "REVOKED",
                  "expiresAt": "2026-09-12T00:00:00Z", "createdAt": "2026-09-05T00:00:00Z" },
            ]),
        );
        let html = render_screen_html(screen, &[], c);
        assert_eq!(
            html.matches("data-action=\"revoke_restaurant_invitation\"").count(),
            1,
            "exactly one Retirer control -- the PENDING row's, never the REVOKED row's: {html}"
        );
        assert!(
            html.contains("&quot;invitationId&quot;:&quot;inv-pending&quot;"),
            "the ONE rendered control carries the PENDING row's own id: {html}"
        );
        assert!(
            !html.contains("inv-revoked&quot;"),
            "the REVOKED row's id never reaches a revoke control's variables: {html}"
        );
    }

    // ── #729/#730: error-state granularity is the RESOLVER, never the shared root ──────────────
    //
    // beck's red-first list, written before the fix and seen RED against the root-alias matching
    // (red evidence recorded in the PR/checkpoint report).

    /// #729 red 1: a failed resolver must not mark its same-root SIBLING's bindings failed.
    /// `mailbox.lanes` and `mailbox.poisoned` share the root `mailbox` — the only live same-root
    /// pair in the corpus (plus `restaurants.featured`/`restaurants.all` on the marketplace home).
    #[test]
    fn a_failed_resolver_does_not_mark_its_same_root_sibling_failed() {
        let mut c = ctx();
        c.insert_resolved("mailbox.poisoned", json!([{ "messageId": "m-1" }]));
        c.insert_failed("mailbox.lanes");
        assert!(
            !c.binding_failed("mailbox.poisoned"),
            "poisoned ANSWERED — the lanes failure must not shadow its sibling's data"
        );
        assert!(c.binding_failed("mailbox.lanes"), "the failed resolver itself stays marked");
    }

    /// #729 red 2: `answer_beats_failure_mark` — an answer always beats a failure mark for the
    /// same resolver (the tracking.rs precedence sentence, now a RenderContext-level rule).
    /// Cross-resolver precedence only, never intra-resolver salvage: the rule is per resolver
    /// KEY, and a null field inside Ok data stays legitimate absence, never a failure.
    #[test]
    fn an_answer_beats_a_failure_mark_for_the_same_resolver() {
        let mut c = ctx();
        c.insert_resolved("order.byId", json!({ "id": "o-1" }));
        c.insert_failed("order.byId");
        assert!(!c.binding_failed("order.byId"), "the answer wins on the full key");
        assert!(!c.binding_failed("order.status"), "…and on the alias-rooted binding");
    }

    /// #729 red 3 (screen-level): one failed mailbox read renders ITS error state while the
    /// same-root sibling's resolved rows still render. Forces the spec's dormant
    /// `{{ mailbox_lanes }}`/`{{ mailbox_poisoned }}` bindings onto aliases the runtime derives.
    #[test]
    fn a_failed_lanes_read_renders_one_error_and_the_poisoned_rows_still_render() {
        let screen = crate::generated::screens::system::SCREENS
            .iter()
            .find(|s| s.id == "mailbox_lanes")
            .expect("system mailbox screen");
        let mut c = ctx();
        c.insert_failed("mailbox.lanes");
        c.insert_resolved(
            "mailbox.poisoned",
            json!([{ "messageType": "PlaceOrder", "actorType": "ORDER", "partition": "p-7",
                     "messageId": "m-poison-1", "attempts": 5, "errorCode": "CAP_EXCEEDED",
                     "receivedAt": "2026-08-29T00:00:00Z" }]),
        );
        let html = render_screen_html(screen, &[], c);
        // Element markup, never CSS substrings (app.css carries selectors on every page).
        assert!(
            html.contains("<div data-c=\"list\" data-error=\"true\""),
            "the failed lanes list renders the error state: {html}"
        );
        assert!(html.contains("m-poison-1"), "the resolved poisoned row still renders: {html}");
        assert_eq!(
            html.matches("data-error=\"true\"").count(),
            1,
            "ONE error state — the failed resolver's, never its sibling's: {html}"
        );
    }

    /// #749: the MENU renders — a resolved catalog's products reach the storefront HTML as item
    /// rows (name + price), grouped under their category when `categoryRef` matches, with
    /// unmatched products in the trailing run (no item silently lost). Before the
    /// `CatalogSections` arm the kind fell into the generic container and a RESOLVED catalog
    /// still rendered an empty div — the schema fix alone would not have shown a menu.
    #[test]
    fn the_catalog_sections_render_the_menu_items() {
        let screen = Surface::RestaurantFrontoffice
            .screens()
            .iter()
            .find(|s| s.id == "restaurant")
            .unwrap();
        let mut c = ctx();
        c.insert_resolved(
            "catalog.byRestaurant",
            json!({
                "categories": [{ "id": "starters", "name": "Starters" }],
                "products": [
                    { "name": "Burger maison", "categoryRef": "starters", "description": "Le classique",
                      "offers": [{ "price": { "amountCents": 1500, "currency": "EUR" } }] },
                    { "name": "Frites", "categoryRef": null,
                      "offers": [{ "price": { "amountCents": 400, "currency": "EUR" } }] },
                ],
            }),
        );
        let html = render_screen_html(screen, Surface::RestaurantFrontoffice.sheets(), c);
        assert!(html.contains("Burger maison"), "the categorized item renders: {html}");
        assert!(html.contains("15,00 EUR"), "with its price: {html}");
        assert!(html.contains("Starters"), "under its category header: {html}");
        assert!(html.contains("Frites"), "the category-less item still renders: {html}");
        assert!(html.contains("data-c=\"catalog_item_row\""), "as item rows: {html}");
    }

    /// #730 red 4 (scalar affordance, Tours-facing): a failed `restaurant.bySlug` renders the
    /// section-level error affordance ONCE — replacing `restaurant_info` — while the catalog
    /// (a DIFFERENT resolver, resolved) still renders. Every error assertion pairs with a
    /// positive sibling-renders assertion, else the test passes when the whole screen errors
    /// (#729 in reverse).
    #[test]
    fn a_failed_restaurant_read_errors_the_info_section_and_the_catalog_still_renders() {
        let screen = Surface::RestaurantFrontoffice
            .screens()
            .iter()
            .find(|s| s.id == "restaurant")
            .unwrap();
        let mut c = ctx();
        c.insert_failed("restaurant.bySlug");
        c.insert_resolved(
            "catalog.byRestaurant",
            json!({ "categories": [{ "id": "starters", "name": "Starters" }] }),
        );
        let html = render_screen_html(screen, Surface::RestaurantFrontoffice.sheets(), c);
        assert!(
            html.contains("<div data-c=\"section\" data-error=\"true\""),
            "the restaurant_info section renders the error affordance: {html}"
        );
        assert_eq!(
            html.matches("data-error=\"true\"").count(),
            1,
            "the error renders ONCE, at the first section owning a failed binding: {html}"
        );
        assert!(
            html.contains("data-c=\"catalog_sections\""),
            "the catalog (its own resolver, resolved) must keep rendering: {html}"
        );
        // The action-disable half of the rule (a control whose variables bind the failed
        // resolver) is pinned at the executor seam:
        // `executor::tests::a_variable_bound_to_a_failed_resolver_disables_the_control` — this
        // screen's header slots (where the favorite toggle lives) are not rendered by the
        // back_button_header arm, so no such control exists in THIS DOM to assert on.
    }

    /// #725 (ux corpus repairs): the marketplace search screen's branch content renders HONESTLY.
    /// Typed query → the if_false branch: backed restaurant results as cards, the "Dishes"
    /// section GAPPED on the declared `dishes.search` gap (never a live-looking empty list).
    /// Empty query → the if_true branch: popular categories (rebound to `categories.all`), the
    /// recent-searches section hidden (its `searches.recent` producer is a declared gap, #723 —
    /// fail-closed, no dead control).
    #[test]
    fn the_marketplace_search_screen_renders_branch_content_honestly() {
        let search =
            Surface::CaptainFrontoffice.screens().iter().find(|s| s.id == "search").unwrap();

        // A typed query.
        let mut c = ctx();
        c.data.insert("search_input".into(), json!({ "value": "pizza" }));
        c.insert_resolved(
            "restaurants.search",
            json!([{ "displayName": "Pizza Chez Test", "slug": "pizza-chez-test",
                     "address": { "city": "Tours" } }]),
        );
        c.insert_resolved("categories.all", json!([{ "id": "pizza", "name": "Pizza" }]));
        let html = render_screen_html(search, Surface::CaptainFrontoffice.sheets(), c);
        assert!(html.contains("Pizza Chez Test"), "backed restaurant results render: {html}");
        assert!(
            html.contains("data-id=\"dish_results\" data-gap=\"No dish/product search query"),
            "the dish section renders as its DECLARED gap: {html}"
        );
        assert!(!html.contains("Popular categories"), "if_true must not render: {html}");

        // An empty query.
        let mut c = ctx();
        c.data.insert("search_input".into(), json!({ "value": "" }));
        c.insert_resolved("categories.all", json!([{ "id": "pizza", "name": "Pizza" }]));
        let html = render_screen_html(search, Surface::CaptainFrontoffice.sheets(), c);
        assert!(html.contains("Popular categories"), "if_true renders popular categories: {html}");
        assert!(
            !html.contains("Recent searches"),
            "the recent-searches section stays hidden on its declared gap (no dead control): {html}"
        );
        // NOTE: the inlined design-system CSS may mention the SELECTOR — target element markup.
        assert!(!html.contains("<div data-c=\"search_results\""), "if_false must not render: {html}");
    }

    /// #725 (beck's trap): a `conditional_section` renders EXACTLY ONE branch — the moment the
    /// emitter emits branch content, an unconditional children render would show BOTH. Asserted on
    /// branch CONTENT, not on `data-cond`.
    #[test]
    fn conditional_section_renders_exactly_one_branch() {
        let node = Node {
            kind: ComponentKind::ConditionalSection,
            props: &[("condition", PropValue::Text("search_input.value == ''"))],
            children: &[],
            branches: &[
                (
                    "if_true",
                    &[Node {
                        kind: ComponentKind::Text,
                        props: &[("value", PropValue::Text("TRUE-BRANCH-SENTINEL"))],
                        children: &[],
                        branches: &[],
                    }],
                ),
                (
                    "if_false",
                    &[Node {
                        kind: ComponentKind::Text,
                        props: &[("value", PropValue::Text("FALSE-BRANCH-SENTINEL"))],
                        children: &[],
                        branches: &[],
                    }],
                ),
            ],
        };
        let mut c = ctx();
        c.insert_resolved("search_input", json!({ "value": "" }));
        let html = node_html(&node, &c);
        assert!(html.contains("TRUE-BRANCH-SENTINEL"), "empty query → if_true renders: {html}");
        assert!(!html.contains("FALSE-BRANCH-SENTINEL"), "empty query → if_false must NOT: {html}");

        let mut c = ctx();
        c.insert_resolved("search_input", json!({ "value": "pizza" }));
        let html = node_html(&node, &c);
        assert!(html.contains("FALSE-BRANCH-SENTINEL"), "a query → if_false renders: {html}");
        assert!(!html.contains("TRUE-BRANCH-SENTINEL"), "a query → if_true must NOT: {html}");
    }

    /// #725 fail-closed leg: an UNEVALUATABLE `condition:` (missing data — e.g. client form state
    /// on the SSR pass) renders NEITHER branch, per the #472 missing-data semantics (silent
    /// fail-closed; the LOUD marker stays reserved for unparseable expressions).
    #[test]
    fn conditional_section_over_missing_data_renders_neither_branch() {
        let node = Node {
            kind: ComponentKind::ConditionalSection,
            props: &[("condition", PropValue::Text("search_input.value == ''"))],
            children: &[],
            branches: &[
                (
                    "if_true",
                    &[Node {
                        kind: ComponentKind::Text,
                        props: &[("value", PropValue::Text("TRUE-BRANCH-SENTINEL"))],
                        children: &[],
                        branches: &[],
                    }],
                ),
                (
                    "if_false",
                    &[Node {
                        kind: ComponentKind::Text,
                        props: &[("value", PropValue::Text("FALSE-BRANCH-SENTINEL"))],
                        children: &[],
                        branches: &[],
                    }],
                ),
            ],
        };
        let html = node_html(&node, &ctx());
        assert!(!html.contains("TRUE-BRANCH-SENTINEL"), "unevaluatable → neither: {html}");
        assert!(!html.contains("FALSE-BRANCH-SENTINEL"), "unevaluatable → neither: {html}");
        assert!(
            !html.contains("data-condition-error"),
            "missing data is NOT an unknown construct — no loud marker: {html}"
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
            let node = Node { kind: ComponentKind::Text, props, children: &[], branches: &[] };
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
            branches: &[],
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

    // ─── #639 part C step 4-iii-A (ADR-20260904-152807 §5-6) — the admin roster screens ─────────
    // `Surface` has no `System` variant (`crates/web/src/handwritten.rs`), so these screens are
    // NOT reached by `every_screen_of_every_surface_renders` — these dedicated tests are the only
    // cover, rendered with `crate::generated::screens::system::SHEETS`, never `&[]`.

    fn system_screen(id: &str) -> &'static crate::generated::screens::Screen {
        crate::generated::screens::system::SCREENS.iter().find(|s| s.id == id).unwrap_or_else(|| {
            panic!("no system screen '{id}' — SCREENS: {:?}", crate::generated::screens::system::SCREENS.iter().map(|s| s.id).collect::<Vec<_>>())
        })
    }

    /// `the_riders_list_badges_exactly_the_restricted_rows`: three rows — ACTIVE (no held job),
    /// RESTRICTED (held, OUT_FOR_DELIVERY), and a LEGACY SUSPENDED-status rider whose `standing`
    /// stays ACTIVE (a legacy availability value is never a grant, ADR-20260904-014136 §4/§6). One
    /// `data-variant="warning"` badge total (the restricted row's own — Actif is `outline`, the
    /// held stage badge is `info`); the raw enum token `OUT_FOR_DELIVERY` never appears — only its
    /// French translation.
    #[test]
    fn the_riders_list_badges_exactly_the_restricted_rows() {
        let screen = system_screen("riders");
        let mut c = RenderContext::new("fr");
        c.insert_resolved(
            "riders.all",
            json!([
                { "riderId": "r-active", "displayName": "Alice", "phone": "+33600000001", "status": "AVAILABLE", "standing": "ACTIVE", "ground": null, "heldDelivery": null, "restrictionDoorOpen": true },
                { "riderId": "r-restricted", "displayName": "Bob", "phone": "+33600000002", "status": "OFFLINE", "standing": "RESTRICTED", "ground": "IDENTITY_MISMATCH",
                  "heldDelivery": { "status": "OUT_FOR_DELIVERY", "foodLocation": null, "pickupAddress": { "line1": "1 Rue Nationale", "postalCode": "37000", "city": "Tours" } },
                  "restrictionDoorOpen": true },
                { "riderId": "r-legacy", "displayName": "Carla", "phone": "+33600000003", "status": "SUSPENDED", "standing": "ACTIVE", "ground": null, "heldDelivery": null, "restrictionDoorOpen": true },
            ]),
        );
        let html = render_screen_html(screen, crate::generated::screens::system::SHEETS, c);
        assert_eq!(html.matches("data-variant=\"warning\"").count(), 1, "exactly ONE restricted row: {html}");
        assert!(html.contains("Restreint"), "{html}");
        assert!(html.contains("Actif"), "{html}");
        assert!(html.contains("En livraison"), "the FRENCH stage label must render: {html}");
        assert!(!html.contains("OUT_FOR_DELIVERY"), "no raw enum token on a French screen: {html}");
        assert!(html.contains("1 Rue Nationale"), "the held job's pickup address: {html}");
        assert!(html.contains("Alice") && html.contains("Bob") && html.contains("Carla"), "{html}");
    }

    /// `the_legacy_suspended_row_reads_suspendu_ancien_never_restreint` — the DETAIL screen's own
    /// availability section: SUSPENDED renders its OWN dedicated legacy badge, never the standing
    /// (access) vocabulary.
    #[test]
    fn the_legacy_suspended_row_reads_suspendu_ancien_never_restreint() {
        let screen = system_screen("rider_detail");
        let mut c = RenderContext::new("fr");
        c.insert_resolved(
            "rider.byId",
            json!({
                "riderId": "r-legacy", "displayName": "Carla", "phone": "+33600000003",
                "status": "SUSPENDED", "standing": "ACTIVE", "ground": null,
                "decidedAt": null, "effectiveAt": null, "reinstatedAt": null,
                "heldDelivery": null, "restrictionDoorOpen": true,
            }),
        );
        let html = render_screen_html(screen, crate::generated::screens::system::SHEETS, c);
        assert!(html.contains("Suspendu (ancien statut)"), "{html}");
        assert!(!html.contains("Restreint"), "a legacy SUSPENDED status must never read as Restreint: {html}");
        assert!(html.contains("Actif"), "the access badge stays Actif: {html}");
    }

    /// `the_reinstate_control_renders_only_when_restricted` — 1/0 and the inverse for the restrict
    /// control; the restrict control is ALSO hidden when `restrictionDoorOpen: false` even for an
    /// ACTIVE rider (a live control bound to a closed door is the control that does nothing).
    #[test]
    fn the_reinstate_control_renders_only_when_restricted() {
        let screen = system_screen("rider_detail");
        let base = json!({
            "riderId": "r-1", "displayName": "Alice", "phone": "+33600000001",
            "status": "AVAILABLE", "ground": null, "decidedAt": null, "effectiveAt": null,
            "reinstatedAt": null, "heldDelivery": null,
        });

        let mut active_open = base.clone();
        active_open["standing"] = json!("ACTIVE");
        active_open["restrictionDoorOpen"] = json!(true);
        let mut c = RenderContext::new("fr");
        c.insert_resolved("rider.byId", active_open);
        let html = render_screen_html(screen, crate::generated::screens::system::SHEETS, c);
        assert_eq!(html.matches("data-sheet=\"restrict_rider_sheet\">Restreindre l'accès<").count(), 1, "ACTIVE + door open: the restrict control renders: {html}");
        assert_eq!(html.matches("Lever la restriction").count(), 0, "ACTIVE: no reinstate control: {html}");

        let mut active_closed = base.clone();
        active_closed["standing"] = json!("ACTIVE");
        active_closed["restrictionDoorOpen"] = json!(false);
        let mut c2 = RenderContext::new("fr");
        c2.insert_resolved("rider.byId", active_closed);
        let html2 = render_screen_html(screen, crate::generated::screens::system::SHEETS, c2);
        assert_eq!(
            html2.matches("data-sheet=\"restrict_rider_sheet\">Restreindre l'accès<").count(),
            0,
            "door CLOSED: the restrict control must not render even for an ACTIVE rider: {html2}"
        );

        let mut restricted = base.clone();
        restricted["standing"] = json!("RESTRICTED");
        restricted["restrictionDoorOpen"] = json!(true);
        let mut c3 = RenderContext::new("fr");
        c3.insert_resolved("rider.byId", restricted);
        let html3 = render_screen_html(screen, crate::generated::screens::system::SHEETS, c3);
        assert_eq!(html3.matches("Lever la restriction").count(), 1, "RESTRICTED: the reinstate control renders: {html3}");
        assert_eq!(
            html3.matches("data-sheet=\"restrict_rider_sheet\">Restreindre l'accès<").count(),
            0,
            "RESTRICTED: no restrict control (already restricted): {html3}"
        );
    }

    /// Round 2 item 4 (legal): a REINSTATED (ACTIVE) rider whose `ground`/`effectiveAt` are still
    /// set (M1: the tuple STAYS set after reinstate) must not read as still-restricted — no ground
    /// text, no "Effectif depuis" row, and the NEW "Rétabli le" row from `reinstatedAt` instead.
    #[test]
    fn a_reinstated_rider_shows_no_ground_and_shows_reinstated_on() {
        let screen = system_screen("rider_detail");
        let mut c = RenderContext::new("fr");
        c.insert_resolved(
            "rider.byId",
            json!({
                "riderId": "r-1", "displayName": "Alice", "phone": "+33600000001",
                "status": "AVAILABLE", "standing": "ACTIVE",
                "ground": "IDENTITY_MISMATCH",
                "decidedAt": "2026-01-06T12:00:00Z", "effectiveAt": "2026-01-06T12:00:00Z",
                "reinstatedAt": "2026-01-10T09:30:00Z",
                "heldDelivery": null, "restrictionDoorOpen": true,
            }),
        );
        let html = render_screen_html(screen, crate::generated::screens::system::SHEETS, c);
        // The ground string ALSO exists as the sheet's own (always-mounted, hidden) chip label —
        // so a bare `contains` would pass vacuously even if the detail's own ground row rendered
        // too. `a_restricted_riders_detail_shows_its_ground_twice_and_effective_since` below proves
        // the RESTRICTED case shows it TWICE (chip label + detail row); here, reinstated (ACTIVE),
        // it must show ONCE (the chip label only). (Round 3 R3-6, legal: this comment used to name
        // `the_detail_shows_no_count` as that proof — FALSE, that test only scans for digit-only
        // text nodes and asserts nothing about the ground string's count; struck.)
        assert_eq!(
            html.matches("Identité non concordante").count(), 1,
            "a reinstated rider must not show the DETAIL ground row (only the sheet's own chip label survives): {html}"
        );
        assert!(!html.contains("Effectif depuis"), "a reinstated rider must not show the past effective-since row: {html}");
        assert!(html.contains("Rétabli le"), "a reinstated rider must show the reinstated-on row: {html}");
    }

    /// `the_four_ground_chips_carry_the_admin_labels` — the sheet's `ground` chip group carries
    /// exactly the FOUR closed values with the admin's OWN labels, no catch-all.
    #[test]
    fn the_four_ground_chips_carry_the_admin_labels() {
        let screen = system_screen("rider_detail");
        let mut c = RenderContext::new("fr");
        c.insert_resolved(
            "rider.byId",
            json!({
                "riderId": "r-1", "displayName": "Alice", "phone": "+33600000001",
                "status": "AVAILABLE", "standing": "ACTIVE", "ground": null,
                "decidedAt": null, "effectiveAt": null, "reinstatedAt": null,
                "heldDelivery": null, "restrictionDoorOpen": true,
            }),
        );
        let html = render_screen_html(screen, crate::generated::screens::system::SHEETS, c);
        assert_eq!(html.matches("data-chip-group=\"ground\"").count(), 4, "exactly four chips: {html}");
        for label in ["À la demande du rider", "Justificatif expiré", "Identité non concordante", "Compte compromis"] {
            assert!(html.contains(label), "missing ground label '{label}': {html}");
        }
        assert!(!html.contains("UNRECOGNISED"), "no catch-all chip: {html}");
    }

    /// ADDENDUM item 12 (reviewer): the `RIDER_REQUESTED` procedure sentence used to be gated on
    /// `ground.value == 'RIDER_REQUESTED'` — a FORM FIELD, never resolver data, so it could NEVER
    /// render (the #870 round-2 defect class, recurring). It now renders UNCONDITIONALLY under
    /// the chips, with no chip picked at all — proving it does not depend on any selection.
    #[test]
    fn the_rider_requested_procedure_sentence_always_renders() {
        let screen = system_screen("rider_detail");
        let mut c = RenderContext::new("fr");
        c.insert_resolved(
            "rider.byId",
            json!({
                "riderId": "r-1", "displayName": "Alice", "phone": "+33600000001",
                "status": "AVAILABLE", "standing": "ACTIVE", "ground": null,
                "decidedAt": null, "effectiveAt": null, "reinstatedAt": null,
                "heldDelivery": null, "restrictionDoorOpen": true,
            }),
        );
        let html = render_screen_html(screen, crate::generated::screens::system::SHEETS, c);
        assert!(
            html.contains("Conservez le message du rider"),
            "the RIDER_REQUESTED procedure sentence must render unconditionally, no chip picked: {html}"
        );
    }

    /// `the_rider_detail_sheet_dispatches_restrict_rider_with_the_chip_value_and_no_free_text` —
    /// the confirm button's `data-vars` names EXACTLY `riderId`/`ground`, "Effectif : maintenant"
    /// and the notice line render, and NO `text_area`/`text_input` exists anywhere on the page
    /// (no free text rides beside the closed grounds, ADR §6).
    #[test]
    fn the_rider_detail_sheet_dispatches_restrict_rider_with_the_chip_value_and_no_free_text() {
        let screen = system_screen("rider_detail");
        let mut c = RenderContext::new("fr");
        c.insert_resolved(
            "rider.byId",
            json!({
                "riderId": "r-1", "displayName": "Alice", "phone": "+33600000001",
                "status": "AVAILABLE", "standing": "ACTIVE", "ground": null,
                "decidedAt": null, "effectiveAt": null, "reinstatedAt": null,
                "heldDelivery": null, "restrictionDoorOpen": true,
            }),
        );
        let html = render_screen_html(screen, crate::generated::screens::system::SHEETS, c);
        assert!(html.contains("data-sheet-id=\"restrict_rider_sheet\""), "{html}");
        assert!(html.contains("data-action=\"restrict_rider\""), "{html}");
        // Round 2 item 2 (beck, ux): sharpened from a vacuous `contains(A) || contains(B)` (where
        // the right disjunct is always true, since "riderId" is a substring of the left literal
        // too) to the RESOLVED `data-vars` payload directly, no `||` — `riderId` must carry the
        // fixture's own value ("r-1"), and `ground` (no chip picked yet) must ride as `null` in
        // `data-vars` AND as an unresolved `data-var-bindings` entry naming `ground.value`, the
        // same idiom `the_held_job_card_and_its_sheet_dispatch_hand_back_delivery` established for
        // `deliveryJobId`.
        assert!(
            html.contains("&quot;riderId&quot;:&quot;r-1&quot;"),
            "riderId must be RESOLVED to the fixture's own id in data-vars: {html}"
        );
        assert!(
            html.contains("&quot;ground&quot;:null"),
            "ground has no chip picked yet, so it must travel as null in data-vars: {html}"
        );
        assert!(
            html.contains("data-var-bindings") && html.contains("ground.value"),
            "ground must be reported as an unresolved binding until a chip is picked: {html}"
        );
        assert!(html.contains("Effectif : maintenant"), "{html}");
        assert!(html.contains("Le rider est informé dans l&#x27;application") || html.contains("informé dans l'application"), "the notice line: {html}");
        assert!(!html.contains("data-c=\"text_area\""), "no free text on the sheet: {html}");
        assert!(!html.contains("data-c=\"text_input\""), "no free text on the sheet: {html}");
    }

    /// `the_detail_shows_no_count` — no digit-only text node anywhere on the detail (ADR-014136
    /// §3: no per-rider count of any kind on the admin surface). Asserted by scanning every
    /// `>text<` run in the rendered HTML and refusing a run that, trimmed, is PURELY digits.
    #[test]
    fn the_detail_shows_no_count() {
        let screen = system_screen("rider_detail");
        let mut c = RenderContext::new("fr");
        c.insert_resolved(
            "rider.byId",
            json!({
                "riderId": "r-1", "displayName": "Alice", "phone": "+33600000001",
                "status": "AVAILABLE", "standing": "RESTRICTED", "ground": "ACCOUNT_COMPROMISE",
                "decidedAt": "2026-01-06T12:00:00Z", "effectiveAt": "2026-01-06T12:00:00Z", "reinstatedAt": null,
                "heldDelivery": { "status": "ASSIGNED", "foodLocation": "WITH_RIDER", "pickupAddress": { "line1": "1 Rue Nationale", "postalCode": "37000", "city": "Tours" } },
                "restrictionDoorOpen": true,
            }),
        );
        let html = render_screen_html(screen, crate::generated::screens::system::SHEETS, c);
        // Manual `>text<` scan (no `regex` dependency in this crate): every run of characters
        // strictly between a `>` and the NEXT `<` is one rendered text node.
        let mut i = 0;
        while let Some(gt) = html[i..].find('>') {
            let start = i + gt + 1;
            let Some(lt) = html[start..].find('<') else { break };
            let text = html[start..start + lt].trim();
            assert!(
                !(!text.is_empty() && text.chars().all(|c| c.is_ascii_digit())),
                "a digit-only text node is a bare count — not allowed on the admin detail: {text:?} in {html}"
            );
            i = start + lt;
        }
    }

    /// R3-6 (#639 part C step 4-iii-A round 3, legal): `detail_access_restricted_facts`'s RESTRICTED
    /// side had NO positive test — deleting the whole `conditional_section` still passed every
    /// existing test, since `the_detail_shows_no_count` (its own doc comment falsely claimed this
    /// coverage) only scans for digit-only text nodes. The ground string ALSO exists as the sheet's
    /// own always-mounted chip label (same trap as `a_reinstated_rider_shows_no_ground_and_shows_reinstated_on`
    /// above), so a RESTRICTED rider must show it TWICE (chip label + detail row) — a bare `contains`
    /// would pass vacuously even with the detail row deleted.
    #[test]
    fn a_restricted_riders_detail_shows_its_ground_twice_and_effective_since() {
        let screen = system_screen("rider_detail");
        let mut c = RenderContext::new("fr");
        c.insert_resolved(
            "rider.byId",
            json!({
                "riderId": "r-1", "displayName": "Alice", "phone": "+33600000001",
                "status": "AVAILABLE", "standing": "RESTRICTED", "ground": "ACCOUNT_COMPROMISE",
                "decidedAt": "2026-01-06T12:00:00Z", "effectiveAt": "2026-01-06T12:00:00Z", "reinstatedAt": null,
                "heldDelivery": null, "restrictionDoorOpen": true,
            }),
        );
        let html = render_screen_html(screen, crate::generated::screens::system::SHEETS, c);
        assert_eq!(
            html.matches("Compte compromis").count(), 2,
            "a RESTRICTED rider must show the ground TWICE (the sheet's own chip label + the detail's own row): {html}"
        );
        assert!(html.contains("Effectif depuis"), "the detail must show the effective-since row: {html}");
    }

    /// R3-7 (#639 part C step 4-iii-A round 3, beck + ux): round 2's `reinstate_rider` `inline_error`
    /// (`rider_detail`'s direct-Tell button, no sheet) was asserted by NOTHING — deleting the node
    /// still passes every existing test. Asserts it mounts, named for the right action.
    #[test]
    fn the_reinstate_rider_inline_error_is_mounted() {
        let screen = system_screen("rider_detail");
        let mut c = RenderContext::new("fr");
        c.insert_resolved(
            "rider.byId",
            json!({
                "riderId": "r-1", "displayName": "Alice", "phone": "+33600000001",
                "status": "AVAILABLE", "standing": "RESTRICTED", "ground": "ACCOUNT_COMPROMISE",
                "decidedAt": "2026-01-06T12:00:00Z", "effectiveAt": "2026-01-06T12:00:00Z", "reinstatedAt": null,
                "heldDelivery": null, "restrictionDoorOpen": true,
            }),
        );
        let html = render_screen_html(screen, crate::generated::screens::system::SHEETS, c);
        assert!(
            html.contains("data-for-action=\"reinstate_rider\""),
            "the reinstate_rider inline_error must be mounted: {html}"
        );
    }

    /// The `phone_call` control's target prop is `number:` (`crates/web/src/executor.rs`
    /// `client_effect`), never `phone:` — a wrong field name renders the button PERMANENTLY
    /// disabled with a loud reason, which this asserts is NOT the case here.
    #[test]
    fn the_phone_call_control_is_not_disabled() {
        let screen = system_screen("rider_detail");
        let mut c = RenderContext::new("fr");
        c.insert_resolved(
            "rider.byId",
            json!({
                "riderId": "r-1", "displayName": "Alice", "phone": "+33600000001",
                "status": "AVAILABLE", "standing": "ACTIVE", "ground": null,
                "decidedAt": null, "effectiveAt": null, "reinstatedAt": null,
                "heldDelivery": null, "restrictionDoorOpen": true,
            }),
        );
        let html = render_screen_html(screen, crate::generated::screens::system::SHEETS, c);
        // Round 2 item 10 (beck): sharpened from the ABSENCE of an error string (which a totally
        // different rendering bug could also satisfy) to the POSITIVE assertion — the control
        // actually dispatches `phone_call` with the resolved number.
        assert!(html.contains("data-action=\"phone_call\""), "the phone_call control must render its action: {html}");
        assert!(html.contains("data-number=\"+33600000001\""), "the phone_call control must carry the resolved number: {html}");
        assert!(
            !html.contains("missing its target prop"),
            "the phone_call control must not render disabled: {html}"
        );
    }

    #[test]
    fn registry_allowlist_round_trips() {
        for kind in ComponentKind::ALL {
            assert_eq!(ComponentKind::from_type(kind.as_str()), Some(*kind));
        }
        assert_eq!(ComponentKind::from_type("not_a_component"), None);
    }

    /// #888: the FROZEN set of kinds with no dedicated `render_node_kind` arm — the concrete
    /// answer to the issue's own uncited "eleven" (ADR-20260817-105845: a card may not state a
    /// derived number with no antecedent). Derived from the compiler's own E0004 the moment the
    /// old `_` wildcard was removed (45 kinds), PLUS `Badge` (its own arm is guarded, and a
    /// guard never counts towards exhaustivity — confirmed against rustc 1.98.1; the gap only
    /// surfaces once the other 45 are closed, since rustc doesn't report a guard-only gap while
    /// any OTHER pattern is fully unmatched). Kept as LITERAL strings, deliberately NOT derived
    /// from the match arm itself, so moving a kind in or out of the no-arm arm REDS the
    /// `no_arm_set_is_exactly_the_declared_list` test below instead of silently changing
    /// behaviour (beck).
    const NO_ARM_KINDS: &[&str] = &[
        "Screen", "Spacer", "Divider", "ToastNotification", "Overlay", "Badge", "BadgeRow",
        "RatingBadge", "DotSeparator", "PromoCard", "HeroSection", "HeroSearchBar",
        "SearchBarActive", "FilterBar", "CategoryPill", "CategoryTile", "CategoryGrid",
        "RestaurantCard", "DishRow", "StickyCategoryNav", "CatalogItemRow", "ItemThumbnail",
        "ItemHeader", "OptionGroups", "QuantitySelector", "CartLineRow", "QuantityStepper",
        "PromoCodeInput", "DeliveryModeToggle", "AddressSelector", "Form", "TextArea",
        "StripeExpressCheckoutElement", "OrderStatusHero", "EtaBar", "OrderTimeline",
        "RestaurantContactRow", "OrderItemsSummary", "OrderIdRow", "OrderCard", "AccountHeader",
        "AvatarButton", "LocationPill", "Countdown", "StarRating", "TipAmountSelector",
    ];

    fn bare_node(kind: ComponentKind) -> Node {
        Node { kind, props: &[], children: &[], branches: &[] }
    }

    /// The set of kinds that render the bare `data-no-arm` marker today must equal the frozen
    /// #888 list EXACTLY — moving a kind out of the no-arm arm into a real rendering arm (or
    /// into it) without updating this list reds here.
    #[test]
    fn no_arm_set_is_exactly_the_declared_list() {
        let c = ctx();
        let actual: std::collections::BTreeSet<String> = ComponentKind::ALL
            .iter()
            .filter(|k| render_node_kind(&bare_node(**k), &c).to_html().contains("data-no-arm"))
            .map(|k| format!("{k:?}"))
            .collect();
        let frozen: std::collections::BTreeSet<String> = NO_ARM_KINDS.iter().map(|s| s.to_string()).collect();
        assert_eq!(actual, frozen, "the rendered no-arm set must equal the frozen #888 list exactly");
    }

    /// beck: the guard-only-covered class (`Badge`) must fail by NAME, not only inside a
    /// 46-element set diff — a bare `badge` (no `text`) renders the marker; a `badge` WITH
    /// `text` renders its dedicated span and carries NO marker. Mutant quoted in the hand-back:
    /// drop the `if node.prop("text").is_some()` guard from the dedicated `Badge` arm — it then
    /// covers `Badge` UNCONDITIONALLY (the no-arm OR-list's `Badge` becomes an unreachable-pattern
    /// warning, not a compile error), so the bare badge above renders the dedicated span instead
    /// of the marker and this test reds on the first assertion.
    #[test]
    fn a_guarded_arm_kind_without_its_guard_condition_still_lands_in_no_arm() {
        let c = ctx();
        let bare_badge = bare_node(ComponentKind::Badge);
        let bare_html = render_node_kind(&bare_badge, &c).to_html();
        assert!(bare_html.contains("data-no-arm"), "a text-less badge must land in no-arm: {bare_html}");

        let texted_badge =
            Node { kind: ComponentKind::Badge, props: &[("text", PropValue::Text("New"))], children: &[], branches: &[] };
        let texted_html = render_node_kind(&texted_badge, &c).to_html();
        assert!(texted_html.contains("<span data-c=\"badge\""), "a badge with text must render its dedicated span: {texted_html}");
        assert!(!texted_html.contains("data-no-arm"), "a badge with text must never carry the no-arm marker: {texted_html}");
    }

    /// A no-arm node must carry the marker and NEVER look wired — an inert control that looks
    /// live is worse than one that visibly is not (CLAUDE.md). Fixture is `DotSeparator` (beck:
    /// keep `TextArea` reserved for the frozen-list assertion only, not doubled up here as a
    /// concrete fixture too).
    #[test]
    fn no_arm_nodes_carry_the_marker_and_no_action() {
        let c = ctx();
        let node = Node {
            kind: ComponentKind::DotSeparator,
            props: &[("title", PropValue::Text("Untitled"))],
            children: &[],
            branches: &[],
        };
        let html = render_node_kind(&node, &c).to_html();
        assert!(html.contains("data-no-arm"), "no-arm node must carry the marker: {html}");
        // `!html.contains(..)` scans the WHOLE subtree, including children -- this asserts the
        // OUTER no-arm element itself never carries these, not merely that nothing nested does.
        assert!(!html.contains("data-action"), "no-arm outer element must never carry data-action: {html}");
        assert!(!html.contains("data-trigger"), "no-arm outer element must never carry data-trigger: {html}");
        assert!(!html.contains("data-vars"), "no-arm outer element must never carry data-vars: {html}");
    }

    /// The no-arm arm renders TODAY'S tagged container unchanged (reviewer/beck: byte-identical
    /// is untestable by construction, so this pins the two things that ARE testable — the
    /// `data-c` tag and the title/label/value text survive the marker's addition).
    #[test]
    fn a_no_arm_node_renders_todays_container_unchanged() {
        let c = ctx();
        let node = Node {
            kind: ComponentKind::TipAmountSelector,
            props: &[("label", PropValue::Text("Montant"))],
            children: &[],
            branches: &[],
        };
        let html = render_node_kind(&node, &c).to_html();
        assert!(html.contains("data-c=\"tip_amount_selector\""), "the tagged container must keep its data-c: {html}");
        assert!(html.contains("Montant"), "the label text must still render: {html}");
    }
}
