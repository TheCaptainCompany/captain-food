use crate::*;

// ─── §2d — service-catalog validation (services.yaml, ADR-20260719-214500) ──────────────────────

/// `^[a-z][a-z0-9_]*$` — a service operation name is a snake_case domain verb (never the
/// provider's vocabulary; the qualified form is `<service>.<operation>`).
pub(crate) fn svc_op_name_ok(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `^POST /adapters/[a-z0-9-]+(/[a-z0-9-]+)+$` — adapter routes are POST and live under
/// `/adapters/` in the PROVIDER's vocabulary (the derived `/services/*` surface is never declared).
pub(crate) fn svc_adapter_route_ok(s: &str) -> bool {
    let rest = match s.strip_prefix("POST /adapters/") {
        Some(r) => r,
        None => return false,
    };
    let segs: Vec<&str> = rest.split('/').collect();
    segs.len() >= 2
        && segs
            .iter()
            .all(|seg| !seg.is_empty() && seg.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
}

/// §2d — validate the service catalog: snake_case operation names, typed error lists ($ref
/// errors.yaml), a spec-owned `binding: local | http` (+ boolean `expose`), implementation routes
/// keyed by real operations in the adapter-route format, and — for `binding: http` — at least one
/// implementation whose routes cover EVERY operation. `input`/`output` field `$ref` resolution is
/// §1's job (not duplicated here).
pub(crate) fn validate_services(model: &Model, issues: &mut Vec<Issue>) {
    const CTX: &str = "services.yaml";
    let services = match model.defs.get(CTX) {
        Some(Value::Mapping(m)) => m,
        _ => return,
    };
    for (k, node) in services {
        let name = match k.as_str() {
            Some(s) => s,
            None => continue,
        };
        let at = format!("{}/{}", CTX, name);

        // Operations: a non-empty mapping of snake_case domain verbs; `errors` (when present) is a
        // list of typed $refs into errors.yaml.
        let op_names: BTreeSet<String> = match node.get("operations").and_then(|x| x.as_mapping()) {
            Some(m) if !m.is_empty() => {
                let mut set = BTreeSet::new();
                for (ok, op) in m {
                    let oname = match ok.as_str() {
                        Some(s) => s,
                        None => continue,
                    };
                    if !svc_op_name_ok(oname) {
                        issues.push(err(
                            "svc-op-name",
                            format!("{}.operations.{}", at, oname),
                            format!("operation name '{}' must be a snake_case domain verb (^[a-z][a-z0-9_]*$).", oname),
                        ));
                    }
                    if let Some(errs) = op.get("errors") {
                        match errs.as_sequence() {
                            None => issues.push(err(
                                "svc-op-errors",
                                format!("{}.operations.{}.errors", at, oname),
                                "operation errors must be a list of { $ref: 'errors.yaml#/<Error>' }.".into(),
                            )),
                            Some(seq) => {
                                for (i, e) in seq.iter().enumerate() {
                                    let ew = format!("{}.operations.{}.errors[{}]", at, oname, i);
                                    match e.get("$ref").and_then(|x| x.as_str()) {
                                        None => issues.push(err("svc-op-errors", ew, "each operation error must be a { $ref: 'errors.yaml#/<Error>' }.".into())),
                                        Some(r) => {
                                            if ref_target_file(r, CTX).as_deref() != Some("errors.yaml") {
                                                issues.push(err("svc-op-errors", ew, format!("operation errors must reference errors.yaml, got '{}'.", r)));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    set.insert(oname.to_string());
                }
                set
            }
            _ => {
                issues.push(err("svc-op-name", format!("{}.operations", at), "a service must declare a non-empty `operations` mapping.".into()));
                BTreeSet::new()
            }
        };

        // Binding & exposure: the topology decision is SPEC-OWNED — never an environment knob.
        let binding = node.get("binding").and_then(|x| x.as_str());
        if !matches!(binding, Some("local") | Some("http")) {
            issues.push(err(
                "svc-binding",
                format!("{}.binding", at),
                format!("binding must be exactly `local` or `http` (spec-owned topology decision), got '{}'.", binding.unwrap_or("")),
            ));
        }
        if let Some(x) = node.get("expose") {
            if x.as_bool().is_none() {
                issues.push(err("svc-expose", format!("{}.expose", at), "expose must be a boolean.".into()));
            }
        }

        // Implementations: `routes` keys ⊆ the service's operations, values in the adapter-route
        // format; an http binding needs at least one implementation covering every operation.
        let mut full_cover = false;
        if let Some(impls) = node.get("implementations").and_then(|x| x.as_mapping()) {
            for (ik, iv) in impls {
                let iname = ik.as_str().unwrap_or("?");
                let routes = match iv.get("routes").and_then(|x| x.as_mapping()) {
                    Some(r) => r,
                    None => continue, // routes are optional (external-SaaS ACLs with no HTTP surface of ours)
                };
                let mut covered: BTreeSet<String> = BTreeSet::new();
                for (rk, rv) in routes {
                    let op = rk.as_str().unwrap_or("?");
                    let rw = format!("{}.implementations.{}.routes.{}", at, iname, op);
                    if !op_names.contains(op) {
                        issues.push(err("svc-impl-route-op", rw.clone(), format!("route key '{}' is not an operation of service '{}'.", op, name)));
                    } else {
                        covered.insert(op.to_string());
                    }
                    let route = rv.as_str().unwrap_or("");
                    if !svc_adapter_route_ok(route) {
                        issues.push(err(
                            "svc-impl-route",
                            rw,
                            format!("route '{}' must be `POST /adapters/<provider>/<path…>` (^POST /adapters/[a-z0-9-]+(/[a-z0-9-]+)+$).", route),
                        ));
                    }
                }
                if !op_names.is_empty() && covered.len() == op_names.len() {
                    full_cover = true;
                }
            }
        }
        if binding == Some("http") && !full_cover {
            issues.push(err(
                "svc-http-unroutable",
                at.clone(),
                format!("binding: http requires at least one implementation whose routes cover every operation of '{}'.", name),
            ));
        }
    }
}

