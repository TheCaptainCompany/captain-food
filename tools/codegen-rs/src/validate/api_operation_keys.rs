// ─── §27 — api operation keys are a CLOSED set (#639 part C step 3-i, ADR-20260904-015903 §6) ────
//
// The defect class this kills: a key the loader never reads. `api.rs#parse_api` builds each
// operation from a fixed list of keys and IGNORES every other one — so a `whileRestricted:` (step
// 4's per-operation carve-out), a misspelt `role:` or a `slices:` is dropped on the floor with the
// gate green, and the spec reads as if the contract were declared. This rule is step 3's half of
// that seam: the set of keys is closed HERE, per section, to exactly what the loader consumes;
// step 4 adds `whileRestricted` to the mutation set AND to the loader in the same change, so a key
// can never again exist in one and not the other.
//
//   ERROR `api-operation-key` — an operation carries a key outside its section's closed set. The
//   message names the key and the operation; the location is `api.yaml/<section>/<name>`.
//
// The sets are the loader's, not a corpus census: `payload` is read by `parse_api` for mutations
// and is therefore legal even though no operation declares it today (a legal-but-unused key is
// not a silently dropped one). `description` is legal everywhere.

use crate::*;

/// Keys `api.rs#parse_api`'s `parse_query` reads (queries AND subscriptions share the parser).
const QUERY_KEYS: &[&str] = &["description", "args", "argsExactlyOneOf", "returns", "roles", "slice"];
/// Keys `api.rs#parse_api` reads on a mutation. `derived` (#865, ADR-20260904-015903 §6): a
/// command property the resolver INJECTS from the caller's `ReadScope` at the seam, never a
/// client-suppliable input — see `validate/api_derived.rs`.
const MUTATION_KEYS: &[&str] = &["description", "command", "roles", "slice", "payload", "derived"];

/// The closed key set of one `api.yaml` section.
fn allowed(section: &str) -> &'static [&'static str] {
    match section {
        "mutations" => MUTATION_KEYS,
        _ => QUERY_KEYS,
    }
}

pub(crate) fn check_api_operation_keys(model: &Model, issues: &mut Vec<Issue>) {
    let Some(api) = model.defs.get("api.yaml") else { return };
    for section in ["queries", "mutations", "subscriptions"] {
        let Some(ops) = api.get(section).and_then(|v| v.as_mapping()) else { continue };
        let legal = allowed(section);
        for (name, op) in ops {
            let Some(name) = name.as_str() else { continue };
            let Some(op) = op.as_mapping() else { continue };
            for key in op.keys() {
                let Some(key) = key.as_str() else { continue };
                if !legal.contains(&key) {
                    issues.push(err(
                        "api-operation-key",
                        format!("api.yaml/{section}/{name}"),
                        format!(
                            "{} '{}' carries the key '{}', which the api loader does not read — it would be \
                             silently dropped. The closed key set for `{}` is [{}]; a new key is added to the \
                             loader AND to this set in the same change (ADR-20260904-015903 §6).",
                            &section[..section.len() - 1],
                            name,
                            key,
                            section,
                            legal.join(", ")
                        ),
                    ));
                }
            }
        }
    }
}
