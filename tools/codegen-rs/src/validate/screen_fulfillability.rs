// ─── §25b — screen read fulfillability: a required arg with no paint-time source (#745) ─────────
//
// The defect class this kills: a screen binds a resolver whose query REQUIRES an argument the
// screen structurally cannot supply at paint time (no route `:param`, no DSL pin, no tenant-host
// slug), so the read is sent arg-less, fails GraphQL validation on EVERY paint, and — since the
// query admits the asking role — classifies as a REAL failure. Checkout's `paymentStatus.byOrder`
// bumped `sdui_degraded_render_total` on every anonymous paint that way, drowning the alert
// channel the #472 contract promises ("alert on ANY sustained non-zero rate").
//
// The verdict is CODEGEN, not a runtime heuristic: this module computes, per screen × resolver,
// the required args (api.yaml `required: true`) that no source fulfills. Sources, in order:
//   • a route `:param` the arg accepts — its own name, or (the emitted rename bridge, one source
//     of truth with `ResolverKey::arg_for_param`) the lowerCamel of the arg's scalar type
//     (`:orderId` feeds `order.byId`'s `id: OrderId`);
//   • a DSL pin (`resolvers.<key>.args`);
//   • the tenant HOST's slug on the tenant-hosted surface (the router injects it as the `slug`
//     param for every matched storefront route — `crates/web/src/router.rs::resolve`).
//
// BOTH directions are errors (the dead-man's proof): an unfulfillable read the screen does not
// declare under `skipped_reads:` is `screen-read-unfulfillable-undeclared`; a declaration the
// computation no longer proves (the spec later grew a source) is `screen-skipped-read-fulfillable`
// — the skip can never silently outlive its justification. The emitters consume the SAME verdict
// (`Screen::skipped_reads` — the generated skip table the runtime consults before any network),
// so the declaration, the validator and the runtime cannot disagree.

use crate::*;

/// The one tenant-hosted surface (`{slug}.captain.food`): its HOST carries a `Slug`, which the
/// router injects as the `slug` route param for every matched storefront route (#745). MIRRORED
/// with `crates/web/src/router.rs` (`Surface::slug_of` + `resolve`) and `server::hosts::
/// classify_host` — same mirror-honesty rule as `Role::segment`: the host model is spelled in a
/// closed set of places, and an unknown host fails safe (marketplace / 404), which keeps the
/// mirrors honest.
pub(crate) const TENANT_HOSTED_SURFACE: &str = "screens/restaurant_frontoffice.yaml";

/// The closed `supplied_by:` set of a `skipped_reads` declaration (loader-schema closed set —
/// a bare token is correct here per the $ref doctrine, rule 3):
///   `client_dispatch` — a client flow supplies the arg AFTER paint (checkout's `orderId` is the
///                       client-minted PlaceOrder id, given to the read at dispatch time);
///   `none`            — no supplier exists anywhere; the note MUST say what tracks the gap.
pub(crate) const SKIP_SUPPLIED_BY: &[&str] = &["client_dispatch", "none"];

/// Does a route `:param` feed the arg `arg_name: arg_scalar`? Its own name, or the lowerCamel of
/// the scalar type (the rename bridge — the ONE derivation `ResolverKey::arg_for_param` emits).
pub(crate) fn param_feeds_arg(param: &str, arg_name: &str, arg_scalar: &str) -> bool {
    param == arg_name || param == camel(arg_scalar)
}

/// The `:param` names of a screen's route.
pub(crate) fn route_params(screen: &Value) -> Vec<String> {
    screen
        .get("route")
        .and_then(|r| r.as_str())
        .map(|r| {
            r.split('/')
                .filter_map(|seg| seg.strip_prefix(':'))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The `(name, scalar)` pairs of a bound query's args, split into (required, all).
fn query_args(model: &Model, query_name: &str) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let mut required = Vec::new();
    let mut all = Vec::new();
    let Some(args) = model
        .defs
        .get("api.yaml")
        .and_then(|v| v.get("queries"))
        .and_then(|v| v.get(query_name))
        .and_then(|q| q.get("args"))
        .and_then(|a| a.as_mapping())
    else {
        return (required, all);
    };
    for (k, v) in args {
        let Some(name) = k.as_str() else { continue };
        let scalar = ref_or_name(v);
        if v.get("required").and_then(|r| r.as_bool()) == Some(true) {
            required.push((name.to_string(), scalar.clone()));
        }
        all.push((name.to_string(), scalar));
    }
    (required, all)
}

/// Every `data_requirements` resolver of `screen` that is STRUCTURALLY unfulfillable at paint
/// time, as `(resolver key, missing required arg names)`. `gap` resolvers contribute nothing —
/// they never reach a transport (the dispatcher fails them closed as a declared gap).
pub(crate) fn screen_unfulfillable_reads(
    model: &Model,
    surface_file: &str,
    screen: &Value,
    resolvers: Option<&serde_yaml::Mapping>,
) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    let Some(resolvers) = resolvers else { return out };
    let params = route_params(screen);
    let host_slug = surface_file == TENANT_HOSTED_SURFACE;
    let reqs: Vec<String> = screen
        .get("data_requirements")
        .and_then(|x| x.as_sequence())
        .map(|s| s.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    for dr in reqs {
        let Some(r) = resolvers.get(Value::String(dr.clone())) else { continue };
        if r.get("gap").map(|v| !v.is_null()).unwrap_or(false) {
            continue;
        }
        let Some(query_name) = r.get("query").and_then(ref_op_name) else { continue };
        let pins: BTreeSet<String> = r
            .get("args")
            .and_then(|a| a.as_mapping())
            .map(|a| a.keys().filter_map(|k| k.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let (required, _all) = query_args(model, &query_name);
        let missing: Vec<String> = required
            .iter()
            .filter(|(name, scalar)| {
                let pinned = pins.contains(name);
                let routed = params.iter().any(|p| param_feeds_arg(p, name, scalar));
                let hosted = host_slug && name == "slug" && scalar == "Slug";
                !(pinned || routed || hosted)
            })
            .map(|(name, _)| name.clone())
            .collect();
        if !missing.is_empty() {
            out.push((dr, missing));
        }
    }
    out
}

/// One parsed `skipped_reads` declaration: `(resolver key, arg name, query name of the arg ref)`.
struct DeclaredSkip {
    resolver: String,
    arg: String,
    arg_query: String,
    index: usize,
}

/// The wired rule (§25b): declared-skip ⇔ provably-unfulfillable, both directions ERROR, plus the
/// declaration's own shape (resolver ∈ data_requirements, arg ∈ the bound query's args,
/// `supplied_by` in the closed set, a non-empty note).
pub(crate) fn check_screen_fulfillability(
    model: &Model,
    issues: &mut Vec<Issue>,
    sfkey: &str,
    sid: &str,
    screen: &Value,
    resolvers: Option<&serde_yaml::Mapping>,
) {
    let computed = screen_unfulfillable_reads(model, sfkey, screen, resolvers);
    let data_requirements: BTreeSet<String> = screen
        .get("data_requirements")
        .and_then(|x| x.as_sequence())
        .map(|s| s.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();

    let mut declared: Vec<DeclaredSkip> = Vec::new();
    if let Some(entries) = screen.get("skipped_reads").and_then(|v| v.as_sequence()) {
        for (i, e) in entries.iter().enumerate() {
            let at = format!("{}/screens/{}/skipped_reads[{}]", sfkey, sid, i);
            let shape_err = |issues: &mut Vec<Issue>, msg: String| {
                issues.push(err("screen-skipped-read-shape", at.clone(), msg));
            };
            // resolver: a $ref to this surface's own resolver declaration (`#/resolvers/<key>`).
            let resolver = e
                .get("resolver")
                .and_then(|r| r.get("$ref"))
                .and_then(|x| x.as_str())
                .and_then(|rf| rf.strip_prefix("#/resolvers/"))
                .map(str::to_string);
            let Some(resolver) = resolver else {
                shape_err(issues, "`resolver` must be a same-file `$ref` of the form '#/resolvers/<key>'.".into());
                continue;
            };
            if resolvers.is_none_or(|m| !m.contains_key(Value::String(resolver.clone()))) {
                shape_err(issues, format!("resolver '{}' is not declared in this surface's `resolvers:`.", resolver));
                continue;
            }
            if !data_requirements.contains(&resolver) {
                shape_err(
                    issues,
                    format!("resolver '{}' is not in this screen's data_requirements — a skip of a read the screen never performs declares nothing.", resolver),
                );
                continue;
            }
            // missing_arg: a $ref to the bound query's own arg (`api.yaml#/queries/<q>/args/<a>`).
            let arg_ref = e
                .get("missing_arg")
                .and_then(|r| r.get("$ref"))
                .and_then(|x| x.as_str())
                .map(str::to_string);
            let parsed = arg_ref.as_deref().and_then(|rf| {
                let rest = rf.strip_prefix("api.yaml#/queries/")?;
                let mut it = rest.split('/');
                let q = it.next()?;
                (it.next()? == "args").then_some(())?;
                let a = it.next()?;
                Some((q.to_string(), a.to_string()))
            });
            let Some((arg_query, arg)) = parsed else {
                shape_err(issues, "`missing_arg` must be a `$ref` of the form 'api.yaml#/queries/<query>/args/<arg>'.".into());
                continue;
            };
            let bound_query = resolvers
                .and_then(|m| m.get(Value::String(resolver.clone())))
                .and_then(|r| r.get("query"))
                .and_then(ref_op_name);
            if bound_query.as_deref() != Some(arg_query.as_str()) {
                shape_err(
                    issues,
                    format!(
                        "missing_arg names query '{}' but resolver '{}' binds '{}' — the declaration must point at the read it excuses.",
                        arg_query, resolver, bound_query.as_deref().unwrap_or("<none>")
                    ),
                );
                continue;
            }
            let (required, _) = query_args(model, &arg_query);
            if !required.iter().any(|(n, _)| n == &arg) {
                shape_err(
                    issues,
                    format!("'{}' is not a REQUIRED arg of query '{}' — only a required arg can make a read unfulfillable.", arg, arg_query),
                );
                continue;
            }
            match e.get("supplied_by").and_then(|s| s.as_str()) {
                Some(s) if SKIP_SUPPLIED_BY.contains(&s) => {}
                other => shape_err(
                    issues,
                    format!(
                        "`supplied_by` must be one of {:?} (got {:?}) — who, if anyone, supplies the arg outside the paint loop.",
                        SKIP_SUPPLIED_BY, other
                    ),
                ),
            }
            if e.get("note").and_then(|n| n.as_str()).map(str::trim).unwrap_or("").is_empty() {
                shape_err(issues, "`note` is required: say WHY the read has no paint-time source and what supplies (or tracks) it.".into());
            }
            declared.push(DeclaredSkip { resolver, arg, arg_query, index: i });
        }
    }

    // Direction 1 — every computed (resolver, missing arg) must be declared.
    for (resolver, missing) in &computed {
        for arg in missing {
            if !declared.iter().any(|d| &d.resolver == resolver && &d.arg == arg) {
                issues.push(err(
                    "screen-read-unfulfillable-undeclared",
                    format!("{}/screens/{}/{}", sfkey, sid, resolver),
                    format!(
                        "resolver '{}' requires arg '{}' and this screen has NO paint-time source for it \
                         (no route :param — same-name or scalar-bridge —, no pin, no tenant-host slug): \
                         the read fails on EVERY paint. Declare it under `skipped_reads:` (with \
                         `supplied_by` + note) so the runtime skips it before network, or give it a source.",
                        resolver, arg
                    ),
                ));
            }
        }
    }
    // Direction 2 — the dead-man's proof: a declaration the computation no longer backs is an
    // ERROR, so a skip can never silently outlive its justification once the spec grows a source.
    for d in &declared {
        let still_missing = computed
            .iter()
            .any(|(r, missing)| r == &d.resolver && missing.contains(&d.arg));
        if !still_missing {
            issues.push(err(
                "screen-skipped-read-fulfillable",
                format!("{}/screens/{}/skipped_reads[{}]", sfkey, sid, d.index),
                format!(
                    "declared skip of '{}' (arg '{}' of query '{}') is no longer proven: the screen now \
                     HAS a paint-time source for that arg. Remove the declaration — the read must run.",
                    d.resolver, d.arg, d.arg_query
                ),
            ));
        }
    }
}
