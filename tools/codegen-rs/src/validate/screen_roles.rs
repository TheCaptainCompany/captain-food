// ─── §26 — a screen's transport role, and the operations it binds (#639 part C step 2c-ii) ────────
//
// The defect class this kills: a control that renders and does nothing. A screen's GraphQL client
// IS a role's client (`crates/web/src/graphql.rs`: role = path, ADR-0006), and an operation whose
// `roles:` do not admit that role is `SkipReason::RoleRefused` at runtime — skipped by design,
// silently (PROP-20260831-180622 §5). Until R1 the client role was the SURFACE's, one per surface;
// R1 lets ONE screen declare `graphql_role: <UserType>` and speak to `/{role}/graphql` instead —
// the rider sign-in door, `roles: [PUBLIC]` on the RIDER surface. That capability is only safe if
// the wrong combination is unspellable, so:
//
//   ERRORS (a declared `graphql_role` is a contract, checked in full):
//     • `screen-graphql-role-unknown`            — not a `scalars.yaml#/UserType` value;
//     • `screen-graphql-role-not-admitted`       — not one of the screen's own `roles:` (a screen
//                                                  cannot speak as a role it does not admit);
//     • `screen-graphql-role-requires-anonymous` — `PUBLIC` on a `requires_auth: true` screen (the
//                                                  proposal's own mitigation: the pair is enforced);
//     • `screen-graphql-role-refused-operation`  — an operation the screen BINDS does not admit the
//                                                  declared role (the rule graphql-architect asked
//                                                  for, restricted to the declared transport role).
//     • `screen-unauthenticated-route-unknown`   — `unauthenticated: { type: navigate, route }`
//                                                  names no `requires_auth: false` route of the same
//                                                  file (a bounce into another gated screen loops).
//
//   WARNING (the general form, `screen.roles ⊆ ∩(roles of every bound operation)`):
//     • `screen-role-refused-operation` — one of the screen's admitted roles is refused by a bound
//       operation: part of the screen's audience sees a dead control. RED on two screens of the
//       2026-09-03 corpus (`deliveries_board` × `escalateDelivery` refuses RESTAURANT_ACCOUNT;
//       the storefront `restaurant` × `markRestaurantAsFavorite` refuses PUBLIC), both pre-existing
//       and neither this slice's to re-scope — held by the warning ratchet, filed for the architect.
//
// "Binds" = the screen's own component tree (`{ component: X }` chrome expanded from
// `global_components`) + its `data_requirements` + every bottom sheet reachable through an
// `open_bottom_sheet` edge from that tree (transitively). Sheets are mounted hidden on every
// screen of a surface, but only an edge makes one REACHABLE from a given screen. Every action
// spelling the DSL uses is walked: `action`, `item_action`, `on_complete`, `on_change`, the
// `conditional` branches (`if_guest` / `if_authenticated`) and `on_success` steps.

use crate::*;

/// The closed `unauthenticated:` step set (loader-schema closed set — a bare token is correct per
/// the $ref doctrine, rule 3): today only `navigate`.
const UNAUTHENTICATED_TYPES: &[&str] = &["navigate"];

/// What the walk reports for one screen: bound operation names with their `roles:` (empty = open
/// to every role path), and the sheets it reached.
#[derive(Default)]
struct Bound {
    /// `(kind, operation name, roles)`.
    ops: Vec<(&'static str, String, Vec<String>)>,
}

fn op_name(rf: &str) -> String {
    rf.rsplit('/').next().unwrap_or("").to_string()
}

fn roles_of(model: &Model, section: &str, name: &str) -> Vec<String> {
    model
        .defs
        .get("api.yaml")
        .and_then(|v| v.get(section))
        .and_then(|v| v.get(name))
        .and_then(|v| v.get("roles"))
        .and_then(|v| v.as_sequence())
        .map(|s| s.iter().filter_map(|r| r.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// Walk one subtree: every action type it dispatches (all spellings) and every sheet it opens.
fn walk(node: &Value, globals: Option<&serde_yaml::Mapping>, actions: &mut Vec<String>, sheets: &mut Vec<String>) {
    match node {
        Value::Sequence(seq) => {
            for n in seq {
                walk(n, globals, actions, sheets);
            }
        }
        Value::Mapping(map) => {
            // `{ component: X }` — the surface's chrome, expanded exactly as the emitter does.
            if let Some(name) = map.get(Value::String("component".into())).and_then(|v| v.as_str()) {
                if let Some(def) = globals.and_then(|g| g.get(Value::String(name.to_string()))) {
                    walk(def, globals, actions, sheets);
                }
            }
            for key in ["action", "item_action", "on_complete", "on_change", "if_guest", "if_authenticated"] {
                if let Some(a) = map.get(Value::String(key.into())) {
                    if let Some(t) = a.get("type").and_then(|x| x.as_str()) {
                        actions.push(t.to_string());
                        if t == "open_bottom_sheet" {
                            if let Some(id) = a.get("sheet_id").and_then(|x| x.as_str()) {
                                sheets.push(id.to_string());
                            }
                        }
                    }
                }
            }
            // `on_success` — one step or an ordered list; an `open_bottom_sheet` step is an edge.
            if let Some(os) = map.get(Value::String("on_success".into())) {
                let steps: Vec<&Value> = match os {
                    Value::Sequence(s) => s.iter().collect(),
                    other => vec![other],
                };
                for step in steps {
                    if step.get("type").and_then(|x| x.as_str()) == Some("open_bottom_sheet") {
                        if let Some(id) = step.get("sheet_id").and_then(|x| x.as_str()) {
                            sheets.push(id.to_string());
                        }
                    }
                }
            }
            for (_, v) in map {
                walk(v, globals, actions, sheets);
            }
        }
        _ => {}
    }
}

/// Everything one screen binds (see the module docs for the definition).
fn bound_operations(model: &Model, doc: &Value, screen: &Value) -> Bound {
    let globals = doc.get("global_components").and_then(|v| v.as_mapping());
    let resolvers = doc.get("resolvers").and_then(|v| v.as_mapping());
    let actions_map = doc.get("actions").and_then(|v| v.as_mapping());
    let sheets_map = doc.get("bottom_sheets").and_then(|v| v.as_mapping());

    let mut action_types: Vec<String> = Vec::new();
    let mut to_visit: Vec<String> = Vec::new();
    if let Some(cs) = screen.get("components") {
        walk(cs, globals, &mut action_types, &mut to_visit);
    }
    let mut visited: BTreeSet<String> = BTreeSet::new();
    while let Some(sheet_id) = to_visit.pop() {
        if !visited.insert(sheet_id.clone()) {
            continue;
        }
        if let Some(def) = sheets_map.and_then(|m| m.get(Value::String(sheet_id))) {
            walk(def, globals, &mut action_types, &mut to_visit);
        }
    }

    let mut bound = Bound::default();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for t in action_types {
        let Some(rf) = actions_map
            .and_then(|m| m.get(Value::String(t.clone())))
            .and_then(|a| a.get("mutation"))
            .and_then(|m| m.get("$ref"))
            .and_then(|x| x.as_str())
        else {
            continue;
        };
        let name = op_name(rf);
        if seen.insert(format!("mutation:{name}")) {
            let roles = roles_of(model, "mutations", &name);
            bound.ops.push(("mutation", name, roles));
        }
    }
    if let Some(drs) = screen.get("data_requirements").and_then(|x| x.as_sequence()) {
        for dr in drs.iter().filter_map(|d| d.as_str()) {
            let Some(rf) = resolvers
                .and_then(|m| m.get(Value::String(dr.to_string())))
                .and_then(|r| r.get("query"))
                .and_then(|q| q.get("$ref"))
                .and_then(|x| x.as_str())
            else {
                continue;
            };
            let name = op_name(rf);
            if seen.insert(format!("query:{name}")) {
                let roles = roles_of(model, "queries", &name);
                bound.ops.push(("query", name, roles));
            }
        }
    }
    bound
}

/// §26 for one screen. `user_types` is the `scalars.yaml#/UserType` value set (the same one §11's
/// `screen-unknown-role` consults).
pub(crate) fn check_screen_roles(
    model: &Model,
    issues: &mut Vec<Issue>,
    sfkey: &str,
    sid: &str,
    screen: &Value,
    doc: &Value,
    user_types: &BTreeSet<String>,
) {
    let at = format!("{}/screens/{}", sfkey, sid);
    let screen_roles: Vec<String> = screen
        .get("roles")
        .and_then(|x| x.as_sequence())
        .map(|s| s.iter().filter_map(|r| r.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let requires_auth = screen.get("requires_auth").and_then(|v| v.as_bool()).unwrap_or(false);
    let bound = bound_operations(model, doc, screen);

    // The declared transport role (R1) — every clause is an ERROR.
    if let Some(gr) = screen.get("graphql_role") {
        match gr.as_str() {
            Some(role) if user_types.contains(role) => {
                if !screen_roles.iter().any(|r| r == role) {
                    issues.push(err(
                        "screen-graphql-role-not-admitted",
                        at.clone(),
                        format!(
                            "graphql_role '{}' is not one of this screen's roles [{}] — a screen cannot speak to /{}/graphql as a role it does not admit.",
                            role,
                            screen_roles.join(", "),
                            role.to_ascii_lowercase().replace('_', "-")
                        ),
                    ));
                }
                if role == "PUBLIC" && requires_auth {
                    issues.push(err(
                        "screen-graphql-role-requires-anonymous",
                        at.clone(),
                        "graphql_role: PUBLIC on a requires_auth: true screen — an anonymous transport cannot serve an authenticated screen; declare requires_auth: false (PROP-20260831-180622 §5, R1).".to_string(),
                    ));
                }
                for (kind, name, roles) in &bound.ops {
                    if !roles.is_empty() && !roles.iter().any(|r| r == role) {
                        issues.push(err(
                            "screen-graphql-role-refused-operation",
                            at.clone(),
                            format!(
                                "graphql_role '{}' would be REFUSED by {} '{}' (roles [{}]) which this screen binds — the control would render and do nothing (SkipReason::RoleRefused). Bind only operations that admit the transport role, or drop graphql_role.",
                                role,
                                kind,
                                name,
                                roles.join(", ")
                            ),
                        ));
                    }
                }
            }
            Some(role) => issues.push(err(
                "screen-graphql-role-unknown",
                at.clone(),
                format!("graphql_role '{}' is not a scalars.yaml#/UserType value.", role),
            )),
            None => issues.push(err(
                "screen-graphql-role-unknown",
                at.clone(),
                "graphql_role must be one scalars.yaml#/UserType token.".to_string(),
            )),
        }
    }

    // The general form — `screen.roles ⊆ ∩(roles of every bound operation)` — as a warning held
    // by the ratchet (two pre-existing hits, see the module docs).
    for (kind, name, roles) in &bound.ops {
        if roles.is_empty() {
            continue;
        }
        let refused: Vec<&str> = screen_roles
            .iter()
            .filter(|r| !roles.iter().any(|x| x == *r))
            .map(String::as_str)
            .collect();
        if !refused.is_empty() {
            issues.push(warn(
                "screen-role-refused-operation",
                at.clone(),
                format!(
                    "{} '{}' (roles [{}]) refuses screen role{} {} — that part of the screen's audience sees a control that renders and does nothing.",
                    kind,
                    name,
                    roles.join(", "),
                    if refused.len() == 1 { "" } else { "s" },
                    refused.join(", ")
                ),
            ));
        }
    }

    // `unauthenticated:` — the bounce a requires_auth screen takes when the surface has no session.
    if let Some(un) = screen.get("unauthenticated") {
        let ty = un.get("type").and_then(|x| x.as_str()).unwrap_or("");
        let route = un.get("route").and_then(|x| x.as_str());
        let target_ok = route.is_some_and(|route| {
            doc.get("screens")
                .and_then(|x| x.as_sequence())
                .map(|screens| {
                    screens.iter().any(|s| {
                        s.get("route").and_then(|r| r.as_str()) == Some(route)
                            && !s.get("requires_auth").and_then(|v| v.as_bool()).unwrap_or(false)
                    })
                })
                .unwrap_or(false)
        });
        if !UNAUTHENTICATED_TYPES.contains(&ty) || !target_ok {
            issues.push(err(
                "screen-unauthenticated-route-unknown",
                at.clone(),
                format!(
                    "unauthenticated must be {{ type: navigate, route: <a requires_auth: false route of this file> }}; got type '{}', route {}.",
                    ty,
                    route.map(|r| format!("'{r}'")).unwrap_or_else(|| "<none>".to_string())
                ),
            ));
        }
        if !requires_auth {
            issues.push(err(
                "screen-unauthenticated-route-unknown",
                at,
                "unauthenticated: declared on a screen that does not require auth — nothing would ever take the bounce.".to_string(),
            ));
        }
    }
}
