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
    /// Dispatch through the two-step layer (`pending::dispatch_persisted`).
    Mutation { key: ActionKey, input: Map<String, Value> },
    /// Execute locally.
    Client(ClientEffect),
    /// Render/keep disabled — a declared spec gap (note attached) or an auth-kind action that has
    /// no wiring yet. Never a silent no-op.
    Disabled { reason: String },
}

/// One button's action, parsed and resolved. `on_success_route` is the DSL's
/// `action.on_success.route` (the checkout pattern: navigate to the confirmation on acceptance) —
/// `{{ variables.orderId }}` templates in it are substituted from the RESOLVED input.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionSpec {
    pub plan: ActionPlan,
    pub loading_label: Option<String>,
    pub on_success_route: Option<String>,
}

impl ActionSpec {
    /// Parse a node's `action.*` props against the screen's resolved data. `None` = the node
    /// declares no action at all (a plain display node).
    pub fn from_node(node: &Node, ctx: &RenderContext) -> Option<ActionSpec> {
        let action_type = text_prop(node, "action.type", ctx)?;
        let Some(key) = ActionKey::from_key(&action_type) else {
            // An action type outside the generated allowlist: fail closed, visibly — the fix is a
            // spec/codegen change, never a silent skip.
            return Some(ActionSpec {
                plan: ActionPlan::Disabled {
                    reason: format!("action `{action_type}` is not in the generated allowlist"),
                },
                loading_label: None,
                on_success_route: None,
            });
        };

        let plan = match key.kind() {
            ActionKind::Mutation => {
                ActionPlan::Mutation { key, input: resolved_variables(node, ctx) }
            }
            ActionKind::Client => match client_effect(key, node, ctx) {
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
        let on_success_route = text_prop(node, "action.on_success.route", ctx)
            .map(|route| substitute_variables(&route, input_for_template));

        Some(ActionSpec {
            plan,
            loading_label: text_prop(node, "loading_label", ctx),
            on_success_route,
        })
    }
}

/// The `action.variables.*` props, resolved: bindings pull the JSON VALUE from the screen data
/// (`{{ order.id }}` → the id itself, not display text); literals pass through as strings. A
/// binding with no data resolves to `null` — the server's required-field validation is the
/// authority on whether that is acceptable, not the client.
fn resolved_variables(node: &Node, ctx: &RenderContext) -> Map<String, Value> {
    const PREFIX: &str = "action.variables.";
    let mut input = Map::new();
    for (path, prop) in node.props {
        if let Some(name) = path.strip_prefix(PREFIX) {
            let value = match prop {
                PropValue::Binding(b) => ctx.binding_json(b).unwrap_or(Value::Null),
                PropValue::Text(s) => Value::String((*s).to_string()),
                PropValue::I18n(key) => Value::String(crate::i18n::resolve(key, &ctx.locale)),
            };
            input.insert(name.to_string(), value);
        }
    }
    input
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

fn client_effect(key: ActionKey, node: &Node, ctx: &RenderContext) -> Option<ClientEffect> {
    match key.as_str() {
        "navigate" => Some(ClientEffect::Navigate { route: text_prop(node, "action.route", ctx)? }),
        "open_bottom_sheet" => {
            Some(ClientEffect::OpenSheet { sheet_id: text_prop(node, "action.sheet_id", ctx)? })
        }
        "close_sheet" => Some(ClientEffect::CloseSheet),
        "phone_call" => {
            Some(ClientEffect::PhoneCall { number: text_prop(node, "action.number", ctx)? })
        }
        "copy_to_clipboard" => Some(ClientEffect::CopyToClipboard),
        "share" => Some(ClientEffect::Share),
        // `conditional`: guest branch until auth state exists (see ClientEffect docs). The nested
        // branch is itself a mini-action: navigate or open a sheet.
        "conditional" => {
            if let Some(route) = text_prop(node, "action.if_guest.route", ctx) {
                return Some(ClientEffect::Navigate { route });
            }
            if let Some(sheet) = text_prop(node, "action.if_guest.sheet_id", ctx) {
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
    /// Route to navigate to on acceptance (`data-on-success`).
    pub const ON_SUCCESS: &str = "data-on-success";
    /// Client-effect targets.
    pub const ROUTE: &str = "data-route";
    pub const SHEET: &str = "data-sheet";
    pub const NUMBER: &str = "data-number";
}

/// The renderer-side half of the contract: a button's plan as `(attribute, value)` pairs, plus
/// whether it renders disabled (with the reason as its tooltip). SSR'd HTML and hydrated DOM carry
/// identical attributes, so the delegated listener works on both.
pub fn button_attrs(node: &Node, ctx: &RenderContext) -> (Vec<(&'static str, String)>, Option<String>) {
    let Some(spec) = ActionSpec::from_node(node, ctx) else { return (Vec::new(), None) };
    let mut out = Vec::new();
    let mut disabled = None;
    match &spec.plan {
        ActionPlan::Mutation { key, input } => {
            out.push((attrs::ACTION, key.as_str().to_string()));
            out.push((attrs::VARS, Value::Object(input.clone()).to_string()));
            if let Some(label) = &spec.loading_label {
                out.push((attrs::LOADING, label.clone()));
            }
            if let Some(route) = &spec.on_success_route {
                out.push((attrs::ON_SUCCESS, route.clone()));
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
            ActionPlan::Mutation { key, input } => {
                assert_eq!(*key, ActionKey::AcceptOrder);
                assert_eq!(input["orderId"], json!("o-1"));
                assert_eq!(input["restaurantId"], json!("r-1"));
            }
            other => panic!("expected a mutation plan, got {other:?}"),
        }
    }

    #[test]
    fn unresolved_bindings_travel_as_null_for_the_server_to_judge() {
        let spec = ActionSpec::from_node(accept_button(), &ctx_with(&[])).unwrap();
        let ActionPlan::Mutation { input, .. } = &spec.plan else { panic!() };
        assert_eq!(input["orderId"], Value::Null);
    }

    #[test]
    fn gap_and_unknown_actions_disable_with_a_reason() {
        // The rider online toggle is a declared gap — its note must surface.
        let rider_top = Surface::Rider.screens().iter().find(|s| s.id == "jobs").unwrap();
        fn find_gap<'a>(nodes: &'a [Node]) -> Option<&'a Node> {
            for n in nodes {
                if matches!(n.prop("action.type"), Some(PropValue::Text("rider_toggle_online"))) {
                    return Some(n);
                }
                if let Some(hit) = find_gap(n.children) {
                    return Some(hit);
                }
            }
            None
        }
        let toggle = find_gap(rider_top.tree).expect("online toggle in the generated tree");
        let spec = ActionSpec::from_node(toggle, &ctx_with(&[])).unwrap();
        match &spec.plan {
            ActionPlan::Disabled { reason } => {
                assert!(reason.contains("No rider availability mutation"), "{reason}")
            }
            other => panic!("a gap must disable, got {other:?}"),
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
}
