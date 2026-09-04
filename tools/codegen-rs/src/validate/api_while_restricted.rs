// ─── §29 — `whileRestricted:` is the standing carve-out grammar (#639 part C step 4-i, ADR-20260904-081527 §4) ──
//
// A SUBSET of `roles:` a role-guarded operation admits even while the caller's standing is
// RESTRICTED. Closed key (joined the loader + api-operation-keys.rs's sets in the same change, see
// api_operation_keys.rs); this file validates the VALUES.
//
//   ERROR `api-while-restricted-not-subset` — a `whileRestricted:` value is not in the operation's
//   own `roles:`, or `roles:` is omitted entirely (nothing to carve out of an open operation).
//   ERROR `api-while-restricted-no-standing-source` — a `whileRestricted:` value names a role with
//   no standing to test (the closed set of standing-bearing roles is `{RIDER}` today).
//   ERROR `api-while-restricted-mutation-derives-actor` — a carved MUTATION must declare
//   `derived: { riderId: rider }` (or another standing-bearing role's derived source), or a
//   restricted caller could act as ANY OTHER identity under the carve-out.

use crate::*;

/// The closed set of roles that carry a `standing` (today: `{RIDER}`) — the only roles
/// `whileRestricted:` may legitimately name.
const STANDING_BEARING_ROLES: &[&str] = &["RIDER"];

pub(crate) fn check_api_while_restricted(model: &Model, issues: &mut Vec<Issue>) {
    let Some(api) = model.defs.get("api.yaml") else { return };
    for section in ["queries", "mutations", "subscriptions"] {
        let Some(ops) = api.get(section).and_then(|v| v.as_mapping()) else { continue };
        for (name, op) in ops {
            let Some(name) = name.as_str() else { continue };
            let Some(op) = op.as_mapping() else { continue };
            let Some(wr) = op.get(&Value::from("whileRestricted")).and_then(|w| w.as_sequence()) else {
                continue;
            };
            let wr_values: Vec<String> = wr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
            if wr_values.is_empty() {
                continue;
            }
            let at = format!("api.yaml/{}/{}/whileRestricted", section, name);
            let roles: Vec<String> = op
                .get("roles")
                .and_then(|r| r.as_sequence())
                .map(|s| s.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            if roles.is_empty() {
                issues.push(err(
                    "api-while-restricted-not-subset",
                    at.clone(),
                    format!(
                        "'{}' declares `whileRestricted:` but `roles:` is omitted — nothing to carve out of an \
                         open operation (operations with `roles:` omitted are unaffected by restriction, \
                         ADR-20260904-081527 §4).",
                        name
                    ),
                ));
                continue;
            }
            for v in &wr_values {
                if !roles.contains(v) {
                    issues.push(err(
                        "api-while-restricted-not-subset",
                        at.clone(),
                        format!(
                            "'{}' carves out role '{}' in `whileRestricted:`, which is not in its own \
                             `roles:` {:?} — the carve-out must be a SUBSET.",
                            name, v, roles
                        ),
                    ));
                }
                if !STANDING_BEARING_ROLES.contains(&v.as_str()) {
                    issues.push(err(
                        "api-while-restricted-no-standing-source",
                        at.clone(),
                        format!(
                            "'{}' carves out role '{}' in `whileRestricted:`, which has no standing to test — \
                             the closed set of standing-bearing roles is {:?}.",
                            name, v, STANDING_BEARING_ROLES
                        ),
                    ));
                }
            }
            if section == "mutations" && wr_values.contains(&"RIDER".to_string()) {
                let derives_rider = op
                    .get("derived")
                    .and_then(|d| d.as_mapping())
                    .map(|m| m.values().any(|v| v.as_str() == Some("rider")))
                    .unwrap_or(false);
                if !derives_rider {
                    issues.push(err(
                        "api-while-restricted-mutation-derives-actor",
                        at,
                        format!(
                            "mutation '{}' carves out RIDER in `whileRestricted:` but declares no \
                             `derived: {{ <field>: rider }}` — a restricted rider acting under the carve-out \
                             must have its own identity derived from `ReadScope::Rider`, never a client-supplied \
                             id (#865), or it could act as any other identity while restricted.",
                            name
                        ),
                    ));
                }
            }
        }
    }
}
