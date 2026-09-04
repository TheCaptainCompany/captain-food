// ─── `derived:` mutation properties are server-injected, never client input (#865, §6) ──────────
//
// ADR-20260904-015903 §6 (the closed operation-key seam) + [#849 "#639 part C step 2b: the
// auth_ref -> rider_id resolver at the request seam"] / ADR-20260830-191457 parts A+B (the rider's
// domain id lives in `ReadScope::Rider`, minted by `auth.rs::resolve_rider_scope`, never a claim)
// + PROP-171500 D2 (the recorded final vision this discharges): an api.yaml mutation may declare
// `derived: { <commandProperty>: <source> }`. The
// named property of its `$ref`'d command is INJECTED by the resolver from the caller's
// authenticated `ReadScope` at the seam — `emit/server_graphql.rs`'s resolver template, BETWEEN
// `command_payload(&input)?` and the typed `serde_json::from_value` — never accepted as a client
// input. `<Command>Input` omits the property entirely (`api.rs#input_types_block` /
// `emit/server_graphql.rs#emit_server_inputs`): a client that supplies it anyway hits
// async-graphql's OWN "unknown field" validation, never the resolver's guard.
//
// The source→scalar map is CLOSED (today: `rider` -> `scalars.yaml#/RiderId`, the only `ReadScope`
// arm this seam derives an id from) — widening it is a DSL change like any other, made in the SAME
// change as the loader arm that reads it (`emit/server_graphql.rs`'s injection match).
//
//   ERROR `api-derived-field-unknown`  — the derived key names no property of the command.
//   ERROR `api-derived-type-mismatch`  — the property's `$ref` is not EXACTLY the source's scalar
//                                        (one name = one scalar); an unrecognized source counts,
//                                        since it names no scalar the property could ever match.
//   ERROR `api-derived-role-mismatch`  — a derived property that is in the command's `required:`
//                                        list forces `roles:` to EXACTLY the source's role set (the
//                                        identity IS the caller); a nullable derived property keeps
//                                        no such constraint — the resolver simply OMITS the key on
//                                        every path whose scope does not match the source.

use crate::*;

/// The closed `derived:` source -> scalar map. `rider` is the only arm today.
pub(crate) fn derived_source_scalar(source: &str) -> Option<&'static str> {
    match source {
        "rider" => Some("RiderId"),
        _ => None,
    }
}

/// The exact `roles:` a REQUIRED derived property of this source forces — the identity IS the
/// caller, so the operation cannot also be reachable by a role the source cannot resolve.
fn derived_source_roles(source: &str) -> &'static [&'static str] {
    match source {
        "rider" => &["RIDER"],
        _ => &[],
    }
}

pub(crate) fn check_api_derived_fields(model: &Model, issues: &mut Vec<Issue>) {
    let Some(Value::Mapping(mutations)) = model.defs.get("api.yaml").and_then(|v| v.get("mutations")) else {
        return;
    };
    for (name, mu) in mutations {
        let Some(name) = name.as_str() else { continue };
        let Some(derived) = mu.get("derived").and_then(|d| d.as_mapping()) else { continue };
        let Some(cmd_ref) = mu.get("command").and_then(|c| c.get("$ref")).and_then(|x| x.as_str()) else {
            continue;
        };
        let Some(cmd_name) = ref_name(cmd_ref) else { continue };
        let Some(cmd) = model.defs.get("commands.yaml").and_then(|v| v.get(&cmd_name)) else { continue };
        let props = cmd.get("properties").and_then(|p| p.as_mapping());
        let required: HashSet<&str> = cmd
            .get("required")
            .and_then(|r| r.as_sequence())
            .map(|s| s.iter().filter_map(|x| x.as_str()).collect())
            .unwrap_or_default();
        let roles: Vec<String> = mu
            .get("roles")
            .and_then(|r| r.as_sequence())
            .map(|s| s.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default();

        for (prop_key, source_v) in derived {
            let Some(prop_name) = prop_key.as_str() else { continue };
            let source = source_v.as_str().unwrap_or("");
            let at = format!("api.yaml/mutations/{name}/derived/{prop_name}");

            let Some(prop_def) = props.and_then(|p| p.get(prop_name)) else {
                issues.push(err(
                    "api-derived-field-unknown",
                    at,
                    format!(
                        "mutation '{name}' declares `derived: {{ {prop_name}: {source} }}`, but \
                         '{prop_name}' is not a property of commands.yaml#/{cmd_name}."
                    ),
                ));
                continue;
            };

            let expected_scalar = derived_source_scalar(source);
            let actual_scalar = prop_def.get("$ref").and_then(|x| x.as_str()).and_then(ref_name);
            if expected_scalar.is_none() || actual_scalar.as_deref() != expected_scalar {
                let expected_str = expected_scalar.map(|s| format!("`{s}`")).unwrap_or_else(|| {
                    format!("no known scalar -- '{source}' is not a recognized derived source")
                });
                issues.push(err(
                    "api-derived-type-mismatch",
                    at.clone(),
                    format!(
                        "mutation '{name}' derives '{prop_name}' from source '{source}', but \
                         commands.yaml#/{cmd_name}/{prop_name} is {} -- a derived property must \
                         `$ref` EXACTLY the source's scalar ({expected_str}).",
                        actual_scalar
                            .as_deref()
                            .map(|s| format!("`{s}`"))
                            .unwrap_or_else(|| "not a scalar $ref".to_string()),
                    ),
                ));
            }

            if required.contains(prop_name) {
                let expected_roles = derived_source_roles(source);
                let actual: Vec<&str> = roles.iter().map(String::as_str).collect();
                if !expected_roles.is_empty() && actual != expected_roles {
                    issues.push(err(
                        "api-derived-role-mismatch",
                        at,
                        format!(
                            "mutation '{name}' derives the REQUIRED '{prop_name}' from '{source}', so \
                             `roles:` must be EXACTLY [{}] (the identity IS the caller) -- it is [{}].",
                            expected_roles.join(", "),
                            roles.join(", ")
                        ),
                    ));
                }
            }
        }
    }
}
