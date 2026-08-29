//! The action executor (#93) — from a generated screen node's `action.*` props to something the
//! platform can DO.
//!
//! The screens DSL attaches actions to buttons; the emitter flattens them to dotted props
//! (`action.type`, `action.variables.orderId`, `action.on_success.route`, …). This module is the
//! pure half of the wiring:
//!
//!   * [`ActionSpec::from_node`] — parse a node's action props, resolving `{{ … }}` variable
//!     bindings against the screen's data context AT PARSE TIME (the same data that rendered the
//!     button — so what the user saw is what dispatches).
//!   * [`ActionPlan`] — what executing means: a `mutation` kind carries its [`ActionKey`] + fully
//!     resolved input (dispatched through `pending::dispatch_persisted` by the driver); a `client`
//!     kind is a local [`ClientEffect`]; a `gap` kind is [`ActionPlan::Disabled`] with the spec's
//!     note (rendered as a disabled control — fail closed, visibly).
//!   * [`button_attrs`] — the DOM contract: the renderer stamps a button's plan onto data
//!     attributes (action key, resolved-variables JSON, loading label, on-success route), so the
//!     SSR'd and hydrated DOM carry the SAME information and the hydrate driver (`interact.rs`)
//!     needs ONE delegated listener, no per-button closures.
//!
//! The driver (wasm) executes plans; everything here is native-testable.

use serde_json::{Map, Value};

use crate::generated::data_layer::{ActionKey, ActionKind};
use crate::generated::screens::{Node, PropValue};
use crate::renderer::RenderContext;

/// A local (non-domain) behaviour — the `client` action kinds. `Conditional` collapses to its
/// guest branch until an auth signal exists client-side (late identification, ADR-20260722-174500:
/// anonymous is the default state; the authenticated branch activates with the auth wiring in #94).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientEffect {
    Navigate { route: String },
    OpenSheet { sheet_id: String },
    CloseSheet,
    PhoneCall { number: String },
    CopyToClipboard,
    Share,
}

/// What a parsed action means for the driver.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionPlan {
    /// Dispatch through the two-step layer (`pending::dispatch_persisted`). `unresolved_bindings`
    /// names the input fields whose `{{ … }}` binding had no screen data — the driver fills the
    /// `{{ <field>.value }}` ones from the live form inputs at dispatch time (#94).
    Mutation {
        key: ActionKey,
        input: Map<String, Value>,
        unresolved_bindings: Vec<(String, String)>,
    },
    /// Execute locally.
    Client(ClientEffect),
    /// Render/keep disabled — a declared spec gap (note attached) or an auth-kind action that has
    /// no wiring yet. Never a silent no-op.
    Disabled { reason: String },
}

/// One step of a mutation's ORDERED `on_success` sequence (#529 — the DSL's `action.on_success`,
/// either a single typed object or a list of them; the loader/validator enforces the type is from
/// the CLOSED set, `screen-on-success-unknown-type`). Steps fire in DECLARED order once the
/// mutation's acceptance SUCCEEDS: `ClaimSession` is AWAITED before any step that follows it
/// (`crate::executor::run_on_success`) — the httpOnly session cookie must land before a page reload
/// reads it back, or the reload still sees a guest. `Navigate`'s `route` carries `{{ variables.X }}`
/// templates substituted from the mutation's RESOLVED input (the checkout pattern) at PARSE time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnSuccessStep {
    Navigate { route: String },
    OpenSheet { sheet_id: String },
    CloseSheet,
    /// POST the acceptance `messageId` to `/auth/session` (`crate::auth::claim_session`) — claims
    /// the parked provider session and lands the httpOnly cookie (PROP-20260724-150500,
    /// ADR-20260809-212810). The ONLY step that is genuinely asynchronous.
    ClaimSession,
}

/// The special `navigate` route the client interprets as "reload the current location" instead of
/// a literal href (#529): deliberately NOT a hardcoded destination — after `ClaimSession` lands the
/// cookie, a fresh SSR pass re-reads `me`/the auth cookie and lands wherever THIS page's own
/// `requires_auth`/`conditional` wiring already says an authenticated visitor belongs, so no new
/// per-caller landing route needs inventing here. Same `$`-prefix convention as [`MINT_UUID`] /
/// [`LOCALE_TOKEN`] (a synthesized value, never a screen/form binding) — different subsystem
/// (a [`ClientEffect`]/[`OnSuccessStep`] target, not a mutation variable), same idea.
pub const RELOAD_ROUTE: &str = "$reload";

impl OnSuccessStep {
    fn to_json(&self) -> Value {
        match self {
            OnSuccessStep::Navigate { route } => serde_json::json!({ "type": "navigate", "route": route }),
            OnSuccessStep::OpenSheet { sheet_id } => {
                serde_json::json!({ "type": "open_bottom_sheet", "sheet_id": sheet_id })
            }
            OnSuccessStep::CloseSheet => serde_json::json!({ "type": "close_sheet" }),
            OnSuccessStep::ClaimSession => serde_json::json!({ "type": "claim_session" }),
        }
    }

    fn from_json(v: &Value) -> Option<Self> {
        match v.get("type").and_then(|t| t.as_str())? {
            "navigate" => Some(OnSuccessStep::Navigate { route: v.get("route")?.as_str()?.to_string() }),
            "open_bottom_sheet" => {
                Some(OnSuccessStep::OpenSheet { sheet_id: v.get("sheet_id")?.as_str()?.to_string() })
            }
            "close_sheet" => Some(OnSuccessStep::CloseSheet),
            "claim_session" => Some(OnSuccessStep::ClaimSession),
            // The spec-level closed set (`screen-on-success-unknown-type`) is wider than what this
            // client build executes (e.g. `show_toast`, used only by the bespoke non-SDUI
            // post-delivery screens) — an unimplemented-but-valid type is simply not acted on here,
            // never a crash.
            _ => None,
        }
    }
}

/// Decode the `data-on-success` DOM attribute (a JSON array [`spec_attrs`] stamped from
/// [`ActionSpec::on_success`]) back into steps — the driver's (`interact.rs`) read side of the
/// contract. Native-testable: no wasm/DOM dependency.
pub fn parse_on_success_attr(raw: &str) -> Vec<OnSuccessStep> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(raw) else { return Vec::new() };
    items.iter().filter_map(OnSuccessStep::from_json).collect()
}

/// Run an ordered `on_success` sequence against an injected sink — pure sequencing, natively
/// testable off-wasm (#529 checkpoint (c)): [`OnSuccessStep::ClaimSession`] is `.await`ed before
/// the FOLLOWING step runs, so a mutant that reorders them (navigate-before-claim) is caught by a
/// recording fake without any real DOM/network. `interact.rs` (wasm-only) is the only real sink.
pub async fn run_on_success<F, Fut>(
    steps: &[OnSuccessStep],
    mut claim_session: F,
    mut navigate: impl FnMut(&str),
    mut open_sheet: impl FnMut(&str),
    mut close_sheet: impl FnMut(),
) where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    for step in steps {
        match step {
            OnSuccessStep::ClaimSession => claim_session().await,
            OnSuccessStep::Navigate { route } => navigate(route),
            OnSuccessStep::OpenSheet { sheet_id } => open_sheet(sheet_id),
            OnSuccessStep::CloseSheet => close_sheet(),
        }
    }
}

/// One button's action, parsed and resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionSpec {
    pub plan: ActionPlan,
    pub loading_label: Option<String>,
    /// The DSL's `action.on_success` — the ordered steps to run once a mutation SUCCEEDS
    /// ([`OnSuccessStep`]). Empty = nothing declared (the pre-#529 default: settle and stop).
    pub on_success: Vec<OnSuccessStep>,
}

impl ActionSpec {
    /// Parse a node's `action.*` props against the screen's resolved data. `None` = the node
    /// declares no action at all (a plain display node).
    pub fn from_node(node: &Node, ctx: &RenderContext) -> Option<ActionSpec> {
        Self::from_node_prefixed(node, ctx, "action")
    }

    /// Parse an action rooted at an arbitrary prop PREFIX (#114): buttons use `action`, an
    /// `otp_input`'s auto-submit uses `on_complete`, a chip's `on_change` uses `on_change` — all
    /// the same `<prefix>.type` + variable-resolution machinery, just a different root key.
    pub fn from_node_prefixed(node: &Node, ctx: &RenderContext, prefix: &str) -> Option<ActionSpec> {
        let action_type = text_prop(node, &format!("{prefix}.type"), ctx)?;
        let Some(key) = ActionKey::from_key(&action_type) else {
            // An action type outside the generated allowlist: fail closed, visibly — the fix is a
            // spec/codegen change, never a silent skip.
            return Some(ActionSpec {
                plan: ActionPlan::Disabled {
                    reason: format!("action `{action_type}` is not in the generated allowlist"),
                },
                loading_label: None,
                on_success: Vec::new(),
            });
        };

        let plan = match key.kind() {
            ActionKind::Mutation => {
                let (input, unresolved_bindings) = resolved_variables(node, ctx, prefix);
                ActionPlan::Mutation { key, input, unresolved_bindings }
            }
            ActionKind::Client => match client_effect(key, node, ctx, prefix) {
                Some(effect) => ActionPlan::Client(effect),
                None => ActionPlan::Disabled {
                    reason: format!("client action `{}` is missing its target prop", key.as_str()),
                },
            },
            // `auth` (sign_out) wires with the auth sheet (#94); `gap` carries the spec's note.
            ActionKind::Auth => ActionPlan::Disabled {
                reason: format!("auth action `{}` awaits the auth wiring (#94)", key.as_str()),
            },
            ActionKind::Gap => ActionPlan::Disabled {
                reason: key.gap().unwrap_or("declared gap").to_string(),
            },
        };

        let input_for_template = match &plan {
            ActionPlan::Mutation { input, .. } => Some(input),
            _ => None,
        };
        let on_success = parse_on_success(node, ctx, prefix, input_for_template);

        Some(ActionSpec { plan, loading_label: text_prop(node, "loading_label", ctx), on_success })
    }
}

/// Parse a node's `<prefix>.on_success` into ordered steps — the DSL uses BOTH spellings today:
/// ONE typed action object (`place_order`'s single-route form) or an ordered LIST of them
/// (`rate_order`/`tip_order`'s `[{…},{…}]`, and #529's `verify_otp` claim-then-navigate pair). A
/// step whose `type` this client build has no local implementation for is simply omitted from the
/// returned list — the CLOSED SET itself is enforced at the spec/validator level
/// (`screen-on-success-unknown-type`), not here.
fn parse_on_success(
    node: &Node,
    ctx: &RenderContext,
    prefix: &str,
    input_for_template: Option<&Map<String, Value>>,
) -> Vec<OnSuccessStep> {
    let root = format!("{prefix}.on_success");
    let mut steps = Vec::new();
    let mut i = 0;
    while let Some(ty) = text_prop(node, &format!("{root}.{i}.type"), ctx) {
        if let Some(step) = build_on_success_step(&ty, node, ctx, &format!("{root}.{i}"), input_for_template) {
            steps.push(step);
        }
        i += 1;
    }
    if steps.is_empty() {
        if let Some(ty) = text_prop(node, &format!("{root}.type"), ctx) {
            if let Some(step) = build_on_success_step(&ty, node, ctx, &root, input_for_template) {
                steps.push(step);
            }
        }
    }
    steps
}

fn build_on_success_step(
    ty: &str,
    node: &Node,
    ctx: &RenderContext,
    prefix: &str,
    input_for_template: Option<&Map<String, Value>>,
) -> Option<OnSuccessStep> {
    match ty {
        "navigate" => {
            let route = text_prop(node, &format!("{prefix}.route"), ctx)?;
            Some(OnSuccessStep::Navigate { route: substitute_variables(&route, input_for_template) })
        }
        "open_bottom_sheet" => {
            Some(OnSuccessStep::OpenSheet { sheet_id: text_prop(node, &format!("{prefix}.sheet_id"), ctx)? })
        }
        "close_sheet" => Some(OnSuccessStep::CloseSheet),
        "claim_session" => Some(OnSuccessStep::ClaimSession),
        _ => None,
    }
}

/// The action's variables, resolved from TWO spellings the DSL uses: the explicit
/// `action.variables.<name>` map AND bare `action.<name>` value props (the sheets' style —
/// `send_otp` carries `action.phone`), excluding the reserved structural keys (`type`, effect
/// targets, and any nested `x.y` path — `on_success.route`, `if_guest.*`, `data.*` are structure,
/// not variables). Bindings pull the JSON VALUE from the screen data (`{{ order.id }}` → the id
/// itself, not display text); an unresolved binding travels as `null` — the server's
/// required-field validation is the authority — and is REPORTED in the bindings map so the driver
/// can fill `{{ <field>.value }}` form references from the live DOM at dispatch time (#94).
fn resolved_variables(
    node: &Node,
    ctx: &RenderContext,
    prefix: &str,
) -> (Map<String, Value>, Vec<(String, String)>) {
    let explicit = format!("{prefix}.variables.");
    let bare = format!("{prefix}.");
    const RESERVED: [&str; 4] = ["type", "route", "sheet_id", "number"];
    let mut input = Map::new();
    let mut unresolved_bindings = Vec::new();
    for (path, prop) in node.props {
        let name = match path.strip_prefix(explicit.as_str()) {
            Some(name) if !name.contains('.') => name,
            Some(_) => continue,
            None => match path.strip_prefix(bare.as_str()) {
                // Bare style: a single-segment key that is not structural.
                Some(name) if !name.contains('.') && !RESERVED.contains(&name) => name,
                _ => continue,
            },
        };
        let value = match prop {
            PropValue::Binding(b) => {
                let resolved = ctx.binding_json(b);
                if resolved.is_none() {
                    unresolved_bindings.push((name.to_string(), (*b).to_string()));
                }
                resolved.unwrap_or(Value::Null)
            }
            PropValue::Text(s) => Value::String((*s).to_string()),
            PropValue::I18n(key) => Value::String(crate::i18n::resolve(key, &ctx.locale)),
        };
        input.insert(name.to_string(), value);
    }
    (input, unresolved_bindings)
}

/// The client-mint sentinel (#147): an action variable bound to `{{ $uuid }}` is NOT a screen
/// binding — no resolver datum can satisfy it (the `$` prefix is reserved for a *synthesized*
/// value; screen data paths never start with `$`), so `resolved_variables` reports it as an
/// unresolved binding, exactly like a `{{ <field>.value }}` form reference. The dispatch layer then
/// mints a fresh UUIDv7 for it via [`fill_mint_tokens`]. This is how `order_conversation`'s
/// `post_message.messageId` (a client-minted idempotency key, required by the command spec) is
/// produced without a bespoke flow like checkout's orderId.
pub const MINT_UUID: &str = "$uuid";

/// Fill any client-mint tokens ([`MINT_UUID`]) in a mutation's resolved variables, minting one
/// fresh value per token from `mint`. Called at DISPATCH time (once per user action), NOT at render
/// time — so the id is minted the moment the write is dispatched and then PERSISTED with the pending
/// write. A same-operation retry re-sends the stored input (`pending::retry`), keeping the original
/// id (idempotency); a brand-new user action mints a new one.
///
/// It reads the same `unresolved_bindings` list the driver uses to fill `{{ <field>.value }}` form
/// references and acts ONLY on `$uuid` entries — the two seams never overlap (a form binding ends in
/// `.value`, never equals `$uuid`). Pure and seam-injected (`mint`) so the mint is unit-testable
/// off-wasm; the wasm driver passes `uuid::Uuid::now_v7`.
pub fn fill_mint_tokens(
    input: &mut Map<String, Value>,
    unresolved_bindings: &[(String, String)],
    mut mint: impl FnMut() -> uuid::Uuid,
) {
    for (name, binding) in unresolved_bindings {
        if binding == MINT_UUID {
            input.insert(name.clone(), Value::String(mint().to_string()));
        }
    }
}

/// The client-locale sentinel (#147): an action variable bound to `{{ $locale }}` is filled at
/// DISPATCH time with the client's current UI locale — the same resolved locale #110 stamps on
/// `<html lang>` (the hydrate driver reads it back from the DOM). Like [`MINT_UUID`], the `$` prefix
/// marks a synthesized value, so `resolved_variables` reports it as an unresolved binding and the
/// dispatch layer supplies it via [`fill_locale_tokens`]. This is how `post_message.originalLocale`
/// (required by the command spec) travels without the screen having to bind a data path for it.
pub const LOCALE_TOKEN: &str = "$locale";

/// Fill any client-locale tokens ([`LOCALE_TOKEN`]) in a mutation's resolved variables with
/// `locale`. Sibling of [`fill_mint_tokens`]: same seam (the reported `unresolved_bindings`), same
/// dispatch-time timing, acts ONLY on `$locale` entries so it never collides with `$uuid` tokens or
/// `{{ <field>.value }}` form references. Pure and value-injected so it is unit-testable off-wasm;
/// the wasm driver passes the DOM `<html lang>` (defaulting to the supported default locale).
pub fn fill_locale_tokens(
    input: &mut Map<String, Value>,
    unresolved_bindings: &[(String, String)],
    locale: &str,
) {
    for (name, binding) in unresolved_bindings {
        if binding == LOCALE_TOKEN {
            input.insert(name.clone(), Value::String(locale.to_string()));
        }
    }
}

/// `{{ variables.<name> }}` templates inside a route, substituted from the resolved input — the
/// checkout `on_success` route carries the client-minted orderId this way.
fn substitute_variables(route: &str, input: Option<&Map<String, Value>>) -> String {
    let Some(input) = input else { return route.to_string() };
    let mut out = route.to_string();
    for (name, value) in input {
        let token = format!("{{{{ variables.{name} }}}}");
        if let Some(s) = value.as_str() {
            out = out.replace(&token, s);
        }
    }
    out
}

fn client_effect(key: ActionKey, node: &Node, ctx: &RenderContext, prefix: &str) -> Option<ClientEffect> {
    let p = |suffix: &str| text_prop(node, &format!("{prefix}.{suffix}"), ctx);
    match key.as_str() {
        "navigate" => Some(ClientEffect::Navigate { route: p("route")? }),
        "open_bottom_sheet" => Some(ClientEffect::OpenSheet { sheet_id: p("sheet_id")? }),
        "close_sheet" => Some(ClientEffect::CloseSheet),
        "phone_call" => Some(ClientEffect::PhoneCall { number: p("number")? }),
        "copy_to_clipboard" => Some(ClientEffect::CopyToClipboard),
        "share" => Some(ClientEffect::Share),
        // `conditional`: guest branch until auth state exists (see ClientEffect docs). The nested
        // branch is itself a mini-action: navigate or open a sheet.
        "conditional" => {
            if let Some(route) = p("if_guest.route") {
                return Some(ClientEffect::Navigate { route });
            }
            if let Some(sheet) = p("if_guest.sheet_id") {
                return Some(ClientEffect::OpenSheet { sheet_id: sheet });
            }
            None
        }
        // A client kind without a local implementation yet (`set_delivery_address`,
        // `use_geolocation`): disabled via the None path, never silently swallowed.
        _ => None,
    }
}

fn text_prop(node: &Node, key: &str, ctx: &RenderContext) -> Option<String> {
    node.prop(key).map(|p| match p {
        PropValue::Text(s) => s.to_string(),
        PropValue::I18n(k) => crate::i18n::resolve(k, &ctx.locale),
        PropValue::Binding(b) => ctx
            .binding_json(b)
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default(),
    })
}

// ─── The DOM contract (renderer → driver) ──────────────────────────────────────────────────────

/// Attribute names buttons carry — ONE list so renderer and driver cannot drift.
pub mod attrs {
    /// The [`ActionKey`] spec key (`data-action`).
    pub const ACTION: &str = "data-action";
    /// Resolved mutation variables as JSON (`data-vars`) — mutation kinds only.
    pub const VARS: &str = "data-vars";
    /// The label to show while the write is unsettled (`data-loading`).
    pub const LOADING: &str = "data-loading";
    /// The ordered `on_success` steps, JSON-encoded (`data-on-success`) — e.g.
    /// `[{"type":"claim_session"},{"type":"navigate","route":"$reload"}]`. Mutation kinds only;
    /// see [`super::OnSuccessStep`]/[`super::parse_on_success_attr`] for the wire shape.
    pub const ON_SUCCESS: &str = "data-on-success";
    /// Unresolved `{{ field.value }}` bindings as JSON `{input_name: binding}` — the driver fills
    /// them from the live form inputs at dispatch time (`data-var-bindings`).
    pub const VAR_BINDINGS: &str = "data-var-bindings";
    /// Client-effect targets.
    pub const ROUTE: &str = "data-route";
    pub const SHEET: &str = "data-sheet";
    pub const NUMBER: &str = "data-number";
    /// The DOM EVENT that fires a non-click action (#114): `complete` (an `otp_input` reaching its
    /// length) or `change` (a chip/select). Absent = a click (buttons).
    pub const TRIGGER: &str = "data-trigger";
    /// The input length an `on_complete` fires at (`otp_input.length`).
    pub const COMPLETE_LEN: &str = "data-complete-len";
}

/// The renderer-side half of the contract: a button's plan as `(attribute, value)` pairs, plus
/// whether it renders disabled (with the reason as its tooltip). SSR'd HTML and hydrated DOM carry
/// identical attributes, so the delegated listener works on both.
pub fn button_attrs(node: &Node, ctx: &RenderContext) -> (Vec<(&'static str, String)>, Option<String>) {
    match ActionSpec::from_node(node, ctx) {
        Some(spec) => spec_attrs(&spec),
        None => (Vec::new(), None),
    }
}

/// [`button_attrs`] for a click action rooted at an arbitrary prop PREFIX (#725): a List's
/// per-item component templates flatten as `item_components.N.*`, so their buttons parse from
/// `item_components.N.action.*` — same machinery, same DOM contract, resolved against the
/// per-row context (the row travels as `item`).
pub fn button_attrs_prefixed(
    node: &Node,
    ctx: &RenderContext,
    prefix: &str,
) -> (Vec<(&'static str, String)>, Option<String>) {
    match ActionSpec::from_node_prefixed(node, ctx, prefix) {
        Some(spec) => spec_attrs(&spec),
        None => (Vec::new(), None),
    }
}

/// A non-click TRIGGER action (#114): an action rooted at `prefix` (`on_complete`/`on_change`),
/// stamped with `data-trigger` = the DOM event that fires it. `interact.rs` delegates `input`/
/// `change` events to these exactly as it delegates clicks to `button_attrs`.
pub fn trigger_attrs(
    node: &Node,
    ctx: &RenderContext,
    prefix: &str,
    trigger: &str,
) -> (Vec<(&'static str, String)>, Option<String>) {
    let Some(spec) = ActionSpec::from_node_prefixed(node, ctx, prefix) else {
        return (Vec::new(), None);
    };
    let (mut out, disabled) = spec_attrs(&spec);
    if !out.is_empty() {
        out.push((attrs::TRIGGER, trigger.to_string()));
    }
    (out, disabled)
}

/// Flatten an [`ActionSpec`] to the DOM attribute contract — shared by click buttons and triggers.
fn spec_attrs(spec: &ActionSpec) -> (Vec<(&'static str, String)>, Option<String>) {
    let mut out = Vec::new();
    let mut disabled = None;
    match &spec.plan {
        ActionPlan::Mutation { key, input, unresolved_bindings } => {
            out.push((attrs::ACTION, key.as_str().to_string()));
            out.push((attrs::VARS, Value::Object(input.clone()).to_string()));
            if !unresolved_bindings.is_empty() {
                let map: Map<String, Value> = unresolved_bindings
                    .iter()
                    .map(|(name, binding)| (name.clone(), Value::String(binding.clone())))
                    .collect();
                out.push((attrs::VAR_BINDINGS, Value::Object(map).to_string()));
            }
            if let Some(label) = &spec.loading_label {
                out.push((attrs::LOADING, label.clone()));
            }
            if !spec.on_success.is_empty() {
                let arr: Vec<Value> = spec.on_success.iter().map(OnSuccessStep::to_json).collect();
                out.push((attrs::ON_SUCCESS, Value::Array(arr).to_string()));
            }
        }
        ActionPlan::Client(effect) => {
            let key = match effect {
                ClientEffect::Navigate { route } => {
                    out.push((attrs::ROUTE, route.clone()));
                    "navigate"
                }
                ClientEffect::OpenSheet { sheet_id } => {
                    out.push((attrs::SHEET, sheet_id.clone()));
                    "open_bottom_sheet"
                }
                ClientEffect::CloseSheet => "close_sheet",
                ClientEffect::PhoneCall { number } => {
                    out.push((attrs::NUMBER, number.clone()));
                    "phone_call"
                }
                ClientEffect::CopyToClipboard => "copy_to_clipboard",
                ClientEffect::Share => "share",
            };
            out.push((attrs::ACTION, key.to_string()));
        }
        ActionPlan::Disabled { reason } => disabled = Some(reason.clone()),
    }
    (out, disabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::Surface;
    use serde_json::json;

    /// The backoffice accept button, straight from the GENERATED tree — the real thing, not a
    /// fixture.
    fn accept_button() -> &'static Node {
        fn find<'a>(nodes: &'a [Node]) -> Option<&'a Node> {
            for n in nodes {
                if let Some(PropValue::I18n("back.orders.accept")) = n.prop("label") {
                    return Some(n);
                }
                if let Some(hit) = find(n.children) {
                    return Some(hit);
                }
            }
            None
        }
        let queue = Surface::RestaurantBackoffice
            .screens()
            .iter()
            .find(|s| s.id == "orders_queue")
            .expect("queue screen");
        find(queue.tree).expect("accept button in the generated tree")
    }

    fn ctx_with(data: &[(&str, Value)]) -> RenderContext {
        let mut ctx = RenderContext::new("en");
        for (k, v) in data {
            ctx.data.insert((*k).to_string(), v.clone());
        }
        ctx
    }

    #[test]
    fn a_generated_mutation_button_resolves_its_variables_from_screen_data() {
        let ctx = ctx_with(&[
            ("order", json!({ "id": "o-1" })),
            ("restaurant", json!({ "id": "r-1" })),
        ]);
        let spec = ActionSpec::from_node(accept_button(), &ctx).expect("an action");
        match &spec.plan {
            ActionPlan::Mutation { key, input, unresolved_bindings } => {
                assert_eq!(*key, ActionKey::AcceptOrder);
                assert_eq!(input["orderId"], json!("o-1"));
                assert_eq!(input["restaurantId"], json!("r-1"));
                assert!(unresolved_bindings.is_empty(), "everything resolved from screen data");
            }
            other => panic!("expected a mutation plan, got {other:?}"),
        }
    }

    #[test]
    fn unresolved_bindings_travel_as_null_and_are_reported() {
        let spec = ActionSpec::from_node(accept_button(), &ctx_with(&[])).unwrap();
        let ActionPlan::Mutation { input, unresolved_bindings, .. } = &spec.plan else { panic!() };
        assert_eq!(input["orderId"], Value::Null);
        assert!(
            unresolved_bindings.iter().any(|(name, b)| name == "orderId" && b == "order.id"),
            "{unresolved_bindings:?}"
        );
    }

    #[test]
    fn sheet_buttons_use_bare_action_props_and_report_form_field_bindings() {
        // The auth sheet's send_otp button, straight from the GENERATED sheet trees (#94):
        // `action.phone: "{{ phone_field.value }}"` — a bare-style variable bound to a FORM FIELD,
        // unresolvable from screen data; the driver fills it from the live input at dispatch time.
        fn find_send_otp<'a>(nodes: &'a [Node]) -> Option<&'a Node> {
            for n in nodes {
                if matches!(n.prop("action.type"), Some(PropValue::Text("send_otp"))) {
                    return Some(n);
                }
                if let Some(hit) = find_send_otp(n.children) {
                    return Some(hit);
                }
            }
            None
        }
        let auth = Surface::RestaurantFrontoffice
            .sheets()
            .iter()
            .find(|s| s.id == "auth_sheet")
            .expect("auth sheet in the generated tables");
        let btn = find_send_otp(std::slice::from_ref(&auth.node)).expect("send_otp button");
        let spec = ActionSpec::from_node(btn, &ctx_with(&[])).unwrap();
        let ActionPlan::Mutation { key, unresolved_bindings, .. } = &spec.plan else {
            panic!("send_otp is a mutation kind")
        };
        assert_eq!(*key, ActionKey::SendOtp);
        assert!(
            unresolved_bindings.iter().any(|(n, b)| n == "phone" && b == "phone_field.value"),
            "{unresolved_bindings:?}"
        );
        // And the DOM contract carries the bindings map for the driver.
        let (attrs_list, _) = button_attrs(btn, &ctx_with(&[]));
        let bindings = attrs_list
            .iter()
            .find(|(a, _)| *a == attrs::VAR_BINDINGS)
            .map(|(_, v)| v.clone())
            .expect("data-var-bindings stamped");
        assert!(bindings.contains("phone_field.value"), "{bindings}");
    }

    #[test]
    fn otp_on_complete_and_chip_on_change_parse_as_trigger_actions() {
        // #114: the OTP field's on_complete → verify_otp, and the rating sheet's timeliness chips'
        // on_change → record_delivery_satisfaction (the #62 survey), both from the GENERATED trees.
        fn find_by_prefix_type<'a>(nodes: &'a [Node], prefix: &str, ty: &str) -> Option<&'a Node> {
            for n in nodes {
                if matches!(n.prop(&format!("{prefix}.type")), Some(PropValue::Text(t)) if t == ty) {
                    return Some(n);
                }
                if let Some(hit) = find_by_prefix_type(n.children, prefix, ty) {
                    return Some(hit);
                }
            }
            None
        }
        let sheets = Surface::RestaurantFrontoffice.sheets();
        let otp = sheets.iter().find(|s| s.id == "otp_sheet").expect("otp sheet");
        let field = find_by_prefix_type(std::slice::from_ref(&otp.node), "on_complete", "verify_otp")
            .expect("otp_input on_complete");
        let (attrs_list, _) = trigger_attrs(field, &ctx_with(&[]), "on_complete", "complete");
        let g = |k: &str| attrs_list.iter().find(|(a, _)| *a == k).map(|(_, v)| v.clone());
        assert_eq!(g(attrs::ACTION).as_deref(), Some("verify_otp"));
        assert_eq!(g(attrs::TRIGGER).as_deref(), Some("complete"));
        assert!(g(attrs::VAR_BINDINGS).unwrap().contains("otp_field.value"), "code binding");

        let rating = sheets.iter().find(|s| s.id == "rating_sheet").expect("rating sheet");
        let chips = find_by_prefix_type(
            std::slice::from_ref(&rating.node),
            "on_change",
            "record_delivery_satisfaction",
        )
        .expect("timeliness chips on_change");
        let (attrs_list, _) = trigger_attrs(chips, &ctx_with(&[]), "on_change", "change");
        let g = |k: &str| attrs_list.iter().find(|(a, _)| *a == k).map(|(_, v)| v.clone());
        assert_eq!(g(attrs::ACTION).as_deref(), Some("record_delivery_satisfaction"));
        assert_eq!(g(attrs::TRIGGER).as_deref(), Some("change"));
        // The selected chip feeds `timeliness` via the group's field-value binding.
        assert!(g(attrs::VAR_BINDINGS).unwrap().contains("delivery_timeliness.value"));
    }

    fn find_action<'a>(nodes: &'a [Node], ty: &str) -> Option<&'a Node> {
        for n in nodes {
            if let Some(PropValue::Text(t)) = n.prop("action.type") {
                if t == ty {
                    return Some(n);
                }
            }
            if let Some(hit) = find_action(n.children, ty) {
                return Some(hit);
            }
        }
        None
    }

    #[test]
    fn gap_actions_disable_with_their_spec_note() {
        // `authenticate_passkey` (auth sheet) is a declared gap — its note must surface.
        let auth = Surface::RestaurantFrontoffice
            .sheets()
            .iter()
            .find(|s| s.id == "auth_sheet")
            .unwrap();
        let node = find_action(std::slice::from_ref(&auth.node), "authenticate_passkey")
            .expect("passkey button in the generated sheet");
        let spec = ActionSpec::from_node(node, &ctx_with(&[])).unwrap();
        match &spec.plan {
            ActionPlan::Disabled { reason } => assert!(reason.contains("WebAuthn"), "{reason}"),
            other => panic!("a gap must disable, got {other:?}"),
        }
    }

    #[test]
    fn the_rider_toggle_is_dispatchable_since_95_closed_its_gap() {
        let rider_top = Surface::Rider.screens().iter().find(|s| s.id == "jobs").unwrap();
        let toggle = find_action(rider_top.tree, "rider_toggle_online")
            .expect("online toggle in the generated tree");
        let spec = ActionSpec::from_node(toggle, &ctx_with(&[])).unwrap();
        match &spec.plan {
            ActionPlan::Mutation { key, .. } => {
                assert_eq!(key.mutation(), Some("changeRiderStatus"));
                assert_eq!(key.input_type(), Some("ChangeRiderStatusInput"));
            }
            other => panic!("#95 exposed the mutation — got {other:?}"),
        }
    }

    #[test]
    fn button_attrs_stamp_the_dom_contract() {
        let ctx = ctx_with(&[
            ("order", json!({ "id": "o-1" })),
            ("restaurant", json!({ "id": "r-1" })),
        ]);
        let (attrs, disabled) = button_attrs(accept_button(), &ctx);
        assert!(disabled.is_none());
        let get = |k: &str| attrs.iter().find(|(a, _)| *a == k).map(|(_, v)| v.clone());
        assert_eq!(get(attrs::ACTION).as_deref(), Some("accept_order"));
        let vars: Value = serde_json::from_str(&get(attrs::VARS).unwrap()).unwrap();
        assert_eq!(vars["orderId"], json!("o-1"));
    }

    #[test]
    fn the_uuid_mint_token_produces_a_fresh_v7_id_at_dispatch_time() {
        // #147: the order_conversation compose send button, straight from the GENERATED tree — its
        // `messageId` variable is `{{ $uuid }}`, a client-mint token (not a screen binding).
        let convo = Surface::RestaurantFrontoffice
            .screens()
            .iter()
            .find(|s| s.id == "order_conversation")
            .expect("order_conversation screen");
        let send = find_action(convo.tree, "post_message").expect("post_message send button");

        let ctx = ctx_with(&[("conversation", json!({ "orderId": "o-9" }))]);
        let spec = ActionSpec::from_node(send, &ctx).expect("an action");
        let ActionPlan::Mutation { input, unresolved_bindings, .. } = &spec.plan else {
            panic!("post_message is a mutation kind")
        };
        // The static/screen-bound variables resolve exactly as before (no regression) …
        assert_eq!(input["orderId"], json!("o-9"), "screen binding still resolves");
        assert_eq!(input["visibility"], json!("PUBLIC"), "literal still passes through");
        assert_eq!(input["authorRole"], json!("CUSTOMER"));
        // … and the mint token is reported unresolved (value null), like a form-field binding.
        assert_eq!(input["messageId"], Value::Null, "unminted at parse time");
        assert!(
            unresolved_bindings.iter().any(|(n, b)| n == "messageId" && b == MINT_UUID),
            "the $uuid token must be reported for the dispatch-time mint: {unresolved_bindings:?}"
        );
        // `body` is a live form-field binding, reported too — filled from the DOM, not by the mint.
        assert!(unresolved_bindings.iter().any(|(n, b)| n == "body" && b == "body.value"));

        // (a) The token resolves to a valid UUIDv7 string at dispatch time.
        let mut first = input.clone();
        fill_mint_tokens(&mut first, unresolved_bindings, uuid::Uuid::now_v7);
        let id = first["messageId"].as_str().expect("messageId is a string after minting");
        assert_eq!(
            uuid::Uuid::parse_str(id).expect("a valid UUID").get_version_num(),
            7,
            "a client-minted UUIDv7 idempotency key"
        );
        // The mint pass touches ONLY the token — the form-field binding and screen data are intact.
        assert_eq!(first["body"], Value::Null, "the $uuid pass never fills form fields");
        assert_eq!(first["orderId"], json!("o-9"));

        // (b) Two dispatches mint DIFFERENT ids (a fresh id per user action).
        let mut second = input.clone();
        fill_mint_tokens(&mut second, unresolved_bindings, uuid::Uuid::now_v7);
        assert_ne!(first["messageId"], second["messageId"], "a fresh id per dispatch");
    }

    #[test]
    fn the_locale_token_fills_the_client_locale_and_leaves_other_entries_untouched() {
        // #147: `{{ $locale }}` resolves to the provided UI locale at dispatch time; a literal, a
        // form-field binding, and the `$uuid` token slot are all left for their own passes.
        let mut input = Map::new();
        input.insert("visibility".into(), json!("PUBLIC")); // literal
        input.insert("body".into(), Value::Null); // form-field binding
        input.insert("messageId".into(), Value::Null); // $uuid token slot
        input.insert("originalLocale".into(), Value::Null); // $locale token slot
        let bindings = vec![
            ("body".to_string(), "body.value".to_string()),
            ("messageId".to_string(), MINT_UUID.to_string()),
            ("originalLocale".to_string(), LOCALE_TOKEN.to_string()),
        ];
        fill_locale_tokens(&mut input, &bindings, "fr");
        assert_eq!(input["originalLocale"], json!("fr"), "the token takes the provided locale");
        assert_eq!(input["visibility"], json!("PUBLIC"), "literal unchanged");
        assert_eq!(input["body"], Value::Null, "form-field binding not touched");
        assert_eq!(input["messageId"], Value::Null, "the $uuid slot is another pass");
    }

    #[test]
    fn a_dispatched_post_message_carries_every_required_field() {
        // The whole point of #147: after the dispatch-time fills, the compose send input satisfies
        // EVERY required PostMessage field — a real send is server-acceptable (no missing field).
        let convo = Surface::RestaurantFrontoffice
            .screens()
            .iter()
            .find(|s| s.id == "order_conversation")
            .expect("order_conversation screen");
        let send = find_action(convo.tree, "post_message").expect("post_message send button");
        let ctx = ctx_with(&[("conversation", json!({ "orderId": "o-9" }))]);
        let ActionPlan::Mutation { input, unresolved_bindings, .. } =
            ActionSpec::from_node(send, &ctx).expect("an action").plan
        else {
            panic!("post_message is a mutation kind")
        };

        // Reproduce the driver's dispatch-time fills, in order:
        let mut input = input;
        fill_mint_tokens(&mut input, &unresolved_bindings, uuid::Uuid::now_v7); // {{ $uuid }}
        fill_locale_tokens(&mut input, &unresolved_bindings, "fr"); // {{ $locale }}
        input.insert("body".into(), json!("merci !")); // {{ body.value }} — live input (DOM in prod)

        // Every required field of commands.yaml#/PostMessage, non-null.
        for field in ["orderId", "messageId", "authorRole", "visibility", "body", "originalLocale"] {
            assert!(
                input.get(field).is_some_and(|v| !v.is_null()),
                "required PostMessage field `{field}` must be non-null after dispatch: {input:?}"
            );
        }
        assert_eq!(input["originalLocale"], json!("fr"));
        assert_eq!(input["authorRole"], json!("CUSTOMER"));
        assert_eq!(input["visibility"], json!("PUBLIC"));
        assert_eq!(input["orderId"], json!("o-9"));
    }

    #[test]
    fn fill_mint_tokens_leaves_literals_and_form_bindings_untouched() {
        // (c) No regression: only `$uuid` entries are minted; a `.value` form binding and an
        // already-resolved literal in the input are left exactly as they were.
        let mut input = Map::new();
        input.insert("visibility".into(), json!("PUBLIC")); // a resolved literal
        input.insert("body".into(), Value::Null); // an unresolved form-field binding
        input.insert("messageId".into(), Value::Null); // the mint token slot
        let bindings = vec![
            ("body".to_string(), "body.value".to_string()),
            ("messageId".to_string(), MINT_UUID.to_string()),
        ];
        fill_mint_tokens(&mut input, &bindings, uuid::Uuid::now_v7);
        assert_eq!(input["visibility"], json!("PUBLIC"), "literal unchanged");
        assert_eq!(input["body"], Value::Null, "form-field binding not minted");
        assert!(input["messageId"].as_str().is_some(), "only the token is minted");
    }

    #[test]
    fn on_success_route_substitutes_resolved_variables() {
        // The checkout pattern: `/orders/{{ variables.orderId }}/confirmation` from the DSL.
        let route = substitute_variables(
            "/orders/{{ variables.orderId }}/confirmation",
            Some(&{
                let mut m = Map::new();
                m.insert("orderId".into(), json!("abc-123"));
                m
            }),
        );
        assert_eq!(route, "/orders/abc-123/confirmation");
    }

    // ─── #529 — the OTP sheet never opens, and the session cookie is never picked up ────────────

    #[test]
    fn send_otp_success_opens_the_otp_sheet() {
        // #529 verified break 1: `send_otp_btn`'s action carried NO `on_success` at all, and the
        // old parser only ever read `.route` — the OTP sheet never opened. Straight from the
        // GENERATED sheet tree (#94's send_otp button).
        fn find_send_otp<'a>(nodes: &'a [Node]) -> Option<&'a Node> {
            for n in nodes {
                if matches!(n.prop("action.type"), Some(PropValue::Text("send_otp"))) {
                    return Some(n);
                }
                if let Some(hit) = find_send_otp(n.children) {
                    return Some(hit);
                }
            }
            None
        }
        let auth = Surface::RestaurantFrontoffice
            .sheets()
            .iter()
            .find(|s| s.id == "auth_sheet")
            .expect("auth sheet in the generated tables");
        let btn = find_send_otp(std::slice::from_ref(&auth.node)).expect("send_otp button");
        let spec = ActionSpec::from_node(btn, &ctx_with(&[])).unwrap();
        assert_eq!(
            spec.on_success,
            vec![OnSuccessStep::OpenSheet { sheet_id: "otp_sheet".to_string() }],
            "send_otp's on_success must open the otp_sheet on ACCEPTANCE, not before"
        );

        // The marketplace surface must carry the exact same wiring (reviewer checkpoint: the two
        // screens files are kept consistent).
        let auth_marketplace = Surface::CaptainFrontoffice
            .sheets()
            .iter()
            .find(|s| s.id == "auth_sheet")
            .expect("auth sheet in the marketplace surface");
        let btn2 =
            find_send_otp(std::slice::from_ref(&auth_marketplace.node)).expect("send_otp button");
        let spec2 = ActionSpec::from_node(btn2, &ctx_with(&[])).unwrap();
        assert_eq!(spec.on_success, spec2.on_success, "both surfaces must wire send_otp identically");
    }

    #[test]
    fn verify_otp_success_claims_the_session_then_navigates_in_order() {
        // #529 verified break 2: `pickup_session` (renamed `claim_session`) had ZERO call sites —
        // the parked session was never claimed, the cookie never landed. The DSL now declares an
        // ORDERED pair on `verify_otp`'s `on_complete.on_success`.
        fn find_by_prefix_type<'a>(nodes: &'a [Node], prefix: &str, ty: &str) -> Option<&'a Node> {
            for n in nodes {
                if matches!(n.prop(&format!("{prefix}.type")), Some(PropValue::Text(t)) if t == ty) {
                    return Some(n);
                }
                if let Some(hit) = find_by_prefix_type(n.children, prefix, ty) {
                    return Some(hit);
                }
            }
            None
        }
        let otp = Surface::RestaurantFrontoffice
            .sheets()
            .iter()
            .find(|s| s.id == "otp_sheet")
            .expect("otp sheet");
        let field = find_by_prefix_type(std::slice::from_ref(&otp.node), "on_complete", "verify_otp")
            .expect("otp_input on_complete");
        let spec = ActionSpec::from_node_prefixed(field, &ctx_with(&[]), "on_complete").unwrap();
        assert_eq!(
            spec.on_success,
            vec![
                OnSuccessStep::ClaimSession,
                OnSuccessStep::Navigate { route: RELOAD_ROUTE.to_string() },
            ],
            "the cookie must land (claim_session) BEFORE the reload picks it up"
        );

        // The DOM contract (`trigger_attrs`, what `interact.rs` actually reads) carries the SAME
        // ordered steps, round-tripped through the JSON wire format.
        let (attrs_list, _) = trigger_attrs(field, &ctx_with(&[]), "on_complete", "complete");
        let raw = attrs_list
            .iter()
            .find(|(a, _)| *a == attrs::ON_SUCCESS)
            .map(|(_, v)| v.clone())
            .expect("data-on-success stamped for interact.rs to read");
        assert_eq!(parse_on_success_attr(&raw), spec.on_success);
    }

    #[tokio::test]
    async fn claim_session_is_awaited_before_the_following_step_runs() {
        // #529 checkpoint (c) — hydrate-gated ordering, made natively testable: `run_on_success` is
        // the pure sequencing `interact.rs` (wasm-only, untestable off-browser) delegates to. A
        // mutant that fires `navigate` before `claim_session`'s future is polled to completion
        // would record `navigate` FIRST — this test catches exactly that reorder.
        use std::cell::RefCell;
        use std::rc::Rc;
        let calls: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let steps =
            vec![OnSuccessStep::ClaimSession, OnSuccessStep::Navigate { route: RELOAD_ROUTE.to_string() }];
        let claim_calls = Rc::clone(&calls);
        run_on_success(
            &steps,
            || {
                let calls = Rc::clone(&claim_calls);
                async move {
                    // A real await point: a same-tick/synchronous mutant would still (accidentally)
                    // pass without this — the yield forces genuine interleaving to matter.
                    tokio::task::yield_now().await;
                    calls.borrow_mut().push("claim_session".to_string());
                }
            },
            |route| calls.borrow_mut().push(format!("navigate:{route}")),
            |sheet_id| calls.borrow_mut().push(format!("open_sheet:{sheet_id}")),
            || calls.borrow_mut().push("close_sheet".to_string()),
        )
        .await;
        assert_eq!(
            *calls.borrow(),
            vec!["claim_session".to_string(), format!("navigate:{RELOAD_ROUTE}")],
            "claim_session must complete before navigate runs"
        );
    }

    #[test]
    fn on_success_attr_round_trips_every_step_kind() {
        let steps = vec![
            OnSuccessStep::ClaimSession,
            OnSuccessStep::Navigate { route: "/checkout".to_string() },
            OnSuccessStep::OpenSheet { sheet_id: "otp_sheet".to_string() },
            OnSuccessStep::CloseSheet,
        ];
        let arr: Vec<Value> = steps.iter().map(OnSuccessStep::to_json).collect();
        let raw = Value::Array(arr).to_string();
        assert_eq!(parse_on_success_attr(&raw), steps);
    }
}
