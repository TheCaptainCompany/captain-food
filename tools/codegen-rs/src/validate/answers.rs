//! §2g — the actor `answers:` block (PROP-20260815-142349 §§2-7, #582 actors half).
//!
//! An answer is a VALUE the actor serves on Ask — declared INSIDE the actor beside `receives:`
//! (ADR-20260731-120825's reasoning: one file states the actor's full protocol). Each reply
//! property is a `$ref` into the actor's OWN declared `state:` (or its implicit identity /
//! lifecycle-status field), so the property and its scalar are declared exactly once and the
//! reply only composes (V2 — "one name = one dedicated scalar").
//!
//! PAIRING (PROP §12 V1) IS DELIBERATELY HALF-BUILT THIS SLICE: the consuming side of the pair —
//! the PM `ask:` step — is the PM half of #582 (blocked on PR #566), so a declared answer may be
//! UNCONSUMED here without a diagnostic. The "every `ask:` resolves to a declared answer /
//! every answer is consumed" completeness check lands with the `ask:` grammar.
//!
//! V7 ("breaking reshape = new answer name") is review-carried doctrine per the PROP — it gets
//! NO rule and NO fixture here on purpose: a gate that cannot actually check history would be a
//! fake gate.

use crate::*;

/// The reply-side resolution of one `#/<Actor>/state/<field>` ref: what the field is, and the
/// scalar `$ref` that types it. Shared with the emitters so validator and generation can never
/// disagree on what a reply property means.
pub(crate) enum ReplyField {
    /// An explicitly declared `state:` entry (its `type` ref, and whether it is nullable) —
    /// `mode: flag` fields surface as `Flag`.
    Declared { type_ref: Option<String>, nullable: bool, flag: bool },
    /// The actor's implicit identity field (the stream key — declared by the `identity` ref
    /// itself, see `is_implicit_identity_state_ref`).
    Identity,
    /// The implicit lifecycle `status` field (the `lifecycle:` block owns it —
    /// `st-status-duplicated` forbids redeclaring it in `state:`), typed by `lifecycle.status`.
    LifecycleStatus { status_ref: String },
}

/// Resolve `field` on `actor` (a `#/<Actor>/state/<field>` reply target) against the actor's
/// declared state, implicit identity and implicit lifecycle status. `None` = unknown field.
pub(crate) fn resolve_reply_field(def: &Value, actor: &str, field: &str) -> Option<ReplyField> {
    if let Some(f) = def.get("state").and_then(|s| s.get(field)) {
        let mode = f.get("mode").and_then(|m| m.as_str()).unwrap_or("latest");
        return Some(ReplyField::Declared {
            type_ref: f
                .get("type")
                .and_then(|t| t.get("$ref"))
                .and_then(|r| r.as_str())
                .map(str::to_string),
            nullable: f.get("nullable").and_then(|n| n.as_bool()).unwrap_or(false),
            flag: mode == "flag",
        });
    }
    if actor_identity_field(def, actor).as_deref() == Some(field) {
        return Some(ReplyField::Identity);
    }
    if field == "status" {
        if let Some(status_ref) = def
            .get("lifecycle")
            .and_then(|lc| lc.get("status"))
            .and_then(|s| s.get("$ref"))
            .and_then(|r| r.as_str())
        {
            return Some(ReplyField::LifecycleStatus { status_ref: status_ref.to_string() });
        }
    }
    None
}

/// §2g rules (each red-first tested, `tools/codegen-rs/src/tests.rs`):
///   - `ans-shape` (error): the block's closed key set — `answers:` is a map of operation →
///     `{ description (required), reply (required, non-empty), params (optional) }`; anything
///     else (missing/empty description, missing reply, unknown keys, a non-aggregate actor) is
///     refused. Answering takes a FOLD, so only aggregates may declare the block.
///   - `ans-inline-type` (error): a params/reply value that is not exactly `{ $ref: ... }` — an
///     inline `type:` here would fork the kernel ("one name = one dedicated scalar", PROP §3).
///   - `ans-reply-ref` (error): a reply property ref that is not `#/<SameActor>/state/<field>`,
///     or that names a field which is neither declared state, the implicit identity, nor the
///     implicit lifecycle status (V2 — refs resolve, nothing redeclared).
///   - `ans-params-ref` (error): a params value ref outside scalars.yaml/entities.yaml.
///   - `ans-op-unique` (error): the derived `<Actor><Op>` PascalCase pair must be unique across
///     the whole catalog — it names the generated `Request`/`Reply` types (V6).
///   - `ans-ref-isolation` (error): NO `$ref` anywhere may target `actors.yaml#/…/answers/…`
///     except a PM `ask:` step (the one legal consumer, PM half of #582) — a reply is
///     constitutionally unpersistable and never an event/projection/api/screen source (kind
///     doctrine rules 2-3, PROP §4).
pub(crate) fn validate_actor_answers(model: &Model, issues: &mut Vec<Issue>) {
    let actors = match model.defs.get("actors.yaml") {
        Some(Value::Mapping(m)) => m,
        _ => return,
    };
    // Derived-type-name uniqueness across the catalog: `<Actor><Op>` (PascalCase) is the
    // generated Request/Reply prefix, so `Order` + `paymentReference` and `OrderPayment` +
    // `reference` would collide in code even though the YAML keys differ.
    let mut derived: BTreeMap<String, String> = BTreeMap::new(); // PascalCase pair -> first site
    for (k, def) in actors {
        let name = match k.as_str() {
            Some(s) if s != "principals" => s,
            _ => continue,
        };
        let Some(answers) = def.get("answers") else { continue };
        let site = |suffix: String| format!("actors.yaml/{}{}", name, suffix);
        if def.get("type").and_then(|t| t.as_str()) != Some("aggregate") {
            issues.push(err(
                "ans-shape",
                site("/answers".into()),
                format!("`answers:` on '{}' — only an aggregate can answer (the reply projects its own fold; a process manager has a state table, not a fold).", name),
            ));
            continue;
        }
        let Some(answers) = answers.as_mapping() else {
            issues.push(err(
                "ans-shape",
                site("/answers".into()),
                "`answers:` must be a map of operation → { description, reply[, params] }.".into(),
            ));
            continue;
        };
        for (ok, op_def) in answers {
            let Some(op) = ok.as_str() else { continue };
            let osite = || site(format!("/answers/{}", op));
            let Some(op_map) = op_def.as_mapping() else {
                issues.push(err(
                    "ans-shape",
                    osite(),
                    format!("operation '{}' must be a map with `description` and `reply`.", op),
                ));
                continue;
            };
            // Closed key set (the loader schema): description + reply + optional params.
            for (kk, _) in op_map {
                if let Some(key) = kk.as_str() {
                    if !matches!(key, "description" | "reply" | "params") {
                        issues.push(err(
                            "ans-shape",
                            osite(),
                            format!("unknown key '{}' on answer operation '{}' — the closed set is description | reply | params (no binding/transport/deadline keys: absence means local, the deadline is the caller's — PROP-20260815-142349 D5/D6).", key, op),
                        ));
                    }
                }
            }
            if op_def.get("description").and_then(|d| d.as_str()).map(str::trim).filter(|s| !s.is_empty()).is_none() {
                issues.push(err(
                    "ans-shape",
                    osite(),
                    format!("answer operation '{}' needs a non-empty `description` (catalog convention — prose stays prose).", op),
                ));
            }
            // Derived-name uniqueness (V6).
            let pair = format!("{}{}", name, pascal(op));
            if let Some(first) = derived.get(&pair) {
                issues.push(err(
                    "ans-op-unique",
                    osite(),
                    format!("derived type prefix '{}' collides with {} — the generated `{}Request`/`{}Reply` names must be catalog-unique.", pair, first, pair, pair),
                ));
            } else {
                derived.insert(pair, osite());
            }
            // params: optional map of name -> { $ref: scalars|entities }.
            if let Some(params) = op_def.get("params") {
                let Some(params) = params.as_mapping() else {
                    issues.push(err("ans-shape", osite(), format!("`params` on '{}' must be a map of name → {{ $ref }}.", op)));
                    continue;
                };
                for (pk, pv) in params {
                    let pname = pk.as_str().unwrap_or("?");
                    let psite = || site(format!("/answers/{}/params/{}", op, pname));
                    let Some(r) = sole_ref(pv) else {
                        issues.push(err(
                            "ans-inline-type",
                            psite(),
                            format!("param '{}' must be exactly {{ $ref: 'scalars.yaml#/…' }} — no inline `type:` (a reply/param may declare NO new scalar, PROP §3).", pname),
                        ));
                        continue;
                    };
                    match ref_target_file(&r, "actors.yaml").as_deref() {
                        Some("scalars.yaml") | Some("entities.yaml") => {}
                        _ => issues.push(err(
                            "ans-params-ref",
                            psite(),
                            format!("param '{}' $ref '{}' must name a scalar or entity (scalars.yaml / entities.yaml).", pname, r),
                        )),
                    }
                }
            }
            // reply: required, non-empty, every value a same-actor state ref.
            let Some(reply) = op_def.get("reply").and_then(|r| r.as_mapping()).filter(|m| !m.is_empty()) else {
                issues.push(err(
                    "ans-shape",
                    osite(),
                    format!("answer operation '{}' needs a non-empty `reply` map (the value served).", op),
                ));
                continue;
            };
            for (rk, rv) in reply {
                let rname = rk.as_str().unwrap_or("?");
                let rsite = || site(format!("/answers/{}/reply/{}", op, rname));
                let Some(r) = sole_ref(rv) else {
                    issues.push(err(
                        "ans-inline-type",
                        rsite(),
                        format!("reply property '{}' must be exactly {{ $ref: '#/{}/state/<field>' }} — no inline `type:` (composition, not declaration; PROP §3).", rname, name),
                    ));
                    continue;
                };
                let field = match parse_ref(&r) {
                    Some(pr)
                        if (pr.file.is_empty() || pr.file == "actors.yaml")
                            && pr.path.len() == 3
                            && pr.path[0] == name
                            && pr.path[1] == "state" =>
                    {
                        pr.path[2].clone()
                    }
                    _ => {
                        issues.push(err(
                            "ans-reply-ref",
                            rsite(),
                            format!("reply property '{}' $ref '{}' must point at a state field of the SAME actor (`#/{}/state/<field>`) — the asker crossing the boundary via $ref IS the translation.", rname, r, name),
                        ));
                        continue;
                    }
                };
                if resolve_reply_field(def, name, &field).is_none() {
                    issues.push(err(
                        "ans-reply-ref",
                        rsite(),
                        format!("reply property '{}' references '{}', which is neither a declared `state:` field, the identity, nor the lifecycle status of '{}' — a reply needing a value the state does not declare is a missing STATE declaration.", rname, field, name),
                    ));
                }
            }
        }
    }
    // Kind isolation (PROP §4 rules 2-3): a reply exists on the wire for one Ask and nowhere
    // else. The only site that may ever $ref into `answers/` is a PM `ask:` step (its `operation`
    // / `from_ask.property` keys — PM half); everything else (events, projections, api, screens,
    // emits, folds, tombstones) is refused.
    for (f, v) in &model.defs {
        let file = f.as_str();
        let mut refs = Vec::new();
        collect_refs(v, file, &mut refs);
        for (loc, r) in refs {
            let Some(pr) = parse_ref(&r) else { continue };
            let target_file = if pr.file.is_empty() { file } else { pr.file.as_str() };
            if target_file != "actors.yaml" || pr.path.get(1).map(String::as_str) != Some("answers") {
                continue;
            }
            let is_pm_ask_site = file == "processmanager.yaml"
                && (loc.contains(".ask.") || loc.ends_with(".ask") || loc.contains(".from_ask."));
            if !is_pm_ask_site {
                issues.push(err(
                    "ans-ref-isolation",
                    loc,
                    format!("$ref '{}' targets an actor answer — a reply is a snapshot whose authority expires at send: only a PM `ask:` step may reference it, never an event, projection, api, screen or fold (PROP-20260815-142349 §4 rules 2-3).", r),
                ));
            }
        }
    }
}

/// The single `$ref` string of a `{ $ref: ... }` node with NO other keys, else `None`.
fn sole_ref(v: &Value) -> Option<String> {
    let m = v.as_mapping()?;
    if m.len() != 1 {
        return None;
    }
    v.get("$ref").and_then(|r| r.as_str()).map(str::to_string)
}
