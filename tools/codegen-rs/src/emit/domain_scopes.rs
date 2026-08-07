//! Emit the PER-SCOPE GENERATED domain crates under `crates/domains/` — `domain-ordering`,
//! `domain-catalog`, …, plus the KERNEL crate `domain-common` — realization step (2) of
//! ADR-20260807-183024 (#373), on top of the #375 spec reorg.
//!
//! WHAT THE SPLIT BUYS. The spec's coupling becomes the COMPILE coupling, mechanically: each
//! scope's `scalars/entities/events/commands/errors` fragments generate into that scope's own
//! crate, and the crate's `[dependencies]` are DERIVED from the fragments' cross-scope `$ref`
//! edges — never hand-designed. The §14 validator (`scope-cycle`) proves those edges form a DAG
//! before anything is emitted, so the derived manifests can never be cyclic. A bin that links
//! `domain-ordering` links exactly the ordering types and their spec-declared dependencies —
//! which is what makes per-actor images real at step (3) and cancels `DRAIN_ACTOR_TYPES`
//! (link-time scoping over env-var scoping, compiler-first ADR-20260803-234035).
//!
//! WHAT STAYS IN THE FACADE. `crates/domain` keeps its hand-written aggregates and becomes a
//! RE-EXPORTING facade over the scope crates for the generated types (same paths, same type
//! IDENTITY — `domain::generated::scalars::OrderId` IS `domain_ordering::scalars::OrderId`), and
//! keeps the genuinely CROSS-SCOPE generated artifacts: the `DomainEvent` union (the single
//! event log speaks every scope's events — D3), the global error catalog (`ERRORS`/`find`), and
//! the `states`/`lifecycles` folds that match on `DomainEvent`. Splitting those per scope is
//! step (3)'s bin work, not this crate split.
//!
//! HONEST LIMITS (recorded so they never surprise): a KERNEL change ripples every scope crate —
//! correctly, `Money` touches everyone; a cross-scope PROCESS MANAGER links every scope it
//! bridges, so a payments event change rebuilds `pm-place-order` — real coupling made visible is
//! the feature, not a leak. And until step (3) splits the downstream bins, the facade `domain`
//! still couples the monolith consumers (`application`, `server`, …) to every scope; the blast
//! radius win lands when bins link scope crates directly and #363's determinator scopes rebuilds.
//!
//! Also emits `specs/generated/crate-graph.generated.json` — the derived crate topology (scope
//! crate → deps, actor/PM bin → the domain crates its refs reach), the REVIEWABLE face of the
//! `$ref`→dependency derivation and the input contract for step (3)'s bin emitter: a PM's refs
//! are its DECLARED dependency list (the §14 DAG exemption made load-bearing).

use crate::*;

/// The type-bearing catalog kinds emitted into per-scope domain crates, in module order:
/// `(logical file, module name)`.
pub(crate) const TYPE_KINDS: &[(&str, &str)] = &[
    ("scalars.yaml", "scalars"),
    ("entities.yaml", "entities"),
    ("events.yaml", "events"),
    ("commands.yaml", "commands"),
    ("errors.yaml", "errors"),
];

/// `ordering` → package name `domain-ordering` (the kernel is `domain-common`, scope = folder).
pub(crate) fn domain_crate_name(scope: &str) -> String {
    format!("domain-{}", scope)
}

/// `ordering` → the Rust path segment `domain_ordering` (Cargo maps dashes to underscores).
pub(crate) fn domain_crate_ident(scope: &str) -> String {
    format!("domain_{}", scope)
}

/// The items of one `(scope, kind)` module, in merged-catalog (= fragment file) order. Events and
/// commands keep the historical struct-def filter (command/event VALUE OBJECTS are struct defs
/// too; a non-object entry generates nothing, exactly as in the single-crate emitters).
pub(crate) fn scope_items<'a>(model: &'a Model, scope: &str, kind: &str) -> Vec<(&'a str, &'a Value)> {
    let mut out = Vec::new();
    let Some(Value::Mapping(m)) = model.defs.get(kind) else { return out };
    for (k, node) in m {
        let Some(name) = k.as_str() else { continue };
        if model.origins.get(&(kind.to_string(), name.to_string())).map(|s| s.as_str())
            != Some(scope)
        {
            continue;
        }
        if matches!(kind, "events.yaml" | "commands.yaml") && !is_struct_def(node) {
            continue;
        }
        out.push((name, node));
    }
    out
}

/// The cross-module reach of one `(scope, kind)` module's items: every `(target scope, target
/// kind)` its `$ref`s resolve to among the type kinds, excluding itself. This is the ONLY source
/// of both the module's `use` lines and the crate's `[dependencies]` — one derivation, so imports
/// and manifests cannot disagree.
fn module_reach(
    model: &Model,
    scope: &str,
    kind: &str,
    items: &[(&str, &Value)],
) -> BTreeSet<(String, String)> {
    let mut reach = BTreeSet::new();
    for (_, node) in items {
        let mut refs = Vec::new();
        collect_refs(node, "", &mut refs);
        for (_, r) in refs {
            let Some(pr) = parse_ref(&r) else { continue };
            let file = if pr.file.is_empty() { kind.to_string() } else { pr.file };
            if !TYPE_KINDS.iter().any(|(k, _)| *k == file) {
                continue;
            }
            let Some(name) = pr.path.first() else { continue };
            let Some(tsc) = model.origins.get(&(file.clone(), name.clone())) else { continue };
            if !(tsc == scope && file == kind) {
                reach.insert((tsc.clone(), file));
            }
        }
    }
    reach
}

/// One generated per-scope domain crate: its scope, directory, manifest, and source files
/// (paths relative to the crate dir). ALL of it is generated — the manifest as much as the code,
/// so "which scopes exist" and "which crates exist" cannot drift (`check-drift` diffs the tree).
pub(crate) struct DomainScopeCrate {
    pub(crate) scope: String,
    /// Directory relative to the repo root, e.g. `crates/domains/ordering`.
    pub(crate) dir: String,
    pub(crate) manifest: String,
    pub(crate) files: Vec<(String, String)>,
    /// Scopes this crate's manifest depends on (derived from the `$ref` reach + the kernel
    /// `ErrorDef` edge) — exposed for the crate-graph artifact.
    pub(crate) dep_scopes: BTreeSet<String>,
}

/// The generated file banner for one scope-crate module.
fn scope_banner(scope: &str, kind: &str) -> String {
    format!(
        "// GENERATED by the Captain.Food codegen from specs/{scope}/{kind} — do not edit by hand\n// (#373, ADR-20260807-183024 step 2). This scope's items only; the `domain` facade re-exports\n// them so `domain::generated::*` paths keep resolving with the SAME type identity.\n",
    )
}

/// Emit every per-scope domain crate, sorted by scope (stable output). A scope whose fragments
/// define no type items gets no crate (nothing to link); the kernel (`common`) additionally
/// carries `ErrorDef` + `interpolate`, the one shared error mechanic every scope's catalog
/// constants are built from.
pub(crate) fn emit_domain_scope_crates(model: &Model) -> Vec<DomainScopeCrate> {
    let mut out = Vec::new();
    for scope in &model.scopes {
        let mut files: Vec<(String, String)> = Vec::new();
        let mut modules: Vec<&str> = Vec::new();
        let mut dep_scopes: BTreeSet<String> = BTreeSet::new();
        for (kind, module) in TYPE_KINDS {
            let items = scope_items(model, scope, kind);
            if items.is_empty() {
                continue;
            }
            let mut body = scope_banner(scope, kind);
            // `use` lines derived from the module's reach (errors excepted: its consts embed no
            // typed context — the `$ref`s under `context:` are documentation for the JSON shape,
            // so importing their targets would only produce unused-import warnings).
            let mut uses: Vec<String> = Vec::new();
            if *kind != "errors.yaml" {
                uses.push("use serde::{Deserialize, Serialize};".to_string());
                let reach = module_reach(model, scope, kind, &items);
                // Same-scope siblings first (module order), then cross-scope by (scope, kind).
                for (tk, tm) in TYPE_KINDS {
                    if reach.contains(&(scope.to_string(), tk.to_string())) {
                        uses.push(format!("use crate::{}::*;", tm));
                    }
                }
                for (tsc, tk) in &reach {
                    if tsc == scope {
                        continue;
                    }
                    dep_scopes.insert(tsc.clone());
                    let tm = TYPE_KINDS.iter().find(|(k, _)| k == tk).map(|(_, m)| *m).unwrap();
                    uses.push(format!("use {}::{}::*;", domain_crate_ident(tsc), tm));
                }
            } else if scope != KERNEL_SCOPE {
                // Every non-kernel error catalog builds on the kernel's `ErrorDef`.
                dep_scopes.insert(KERNEL_SCOPE.to_string());
                uses.push(format!("use {}::errors::ErrorDef;", domain_crate_ident(KERNEL_SCOPE)));
            }
            if !uses.is_empty() {
                body.push('\n');
                body.push_str(&uses.join("\n"));
                body.push('\n');
            }
            match *kind {
                "scalars.yaml" => {
                    for (name, node) in &items {
                        push_scalar(&mut body, name, node);
                    }
                }
                "errors.yaml" => {
                    if scope == KERNEL_SCOPE {
                        body.push_str(ERROR_DEF_AND_INTERPOLATE);
                    }
                    for (name, node) in &items {
                        push_error_const(&mut body, name, node);
                    }
                }
                _ => {
                    for (name, node) in &items {
                        push_struct(&mut body, kind, name, node);
                    }
                }
            }
            files.push((format!("src/{}.rs", module), body));
            modules.push(module);
        }
        if modules.is_empty() {
            continue;
        }
        let mut lib = format!(
            "// GENERATED by the Captain.Food codegen from specs/{scope}/ — do not edit by hand\n// (#373, ADR-20260807-183024 step 2).\n\n//! Captain.Food `{crate_name}`: the `{scope}` scope's generated domain types{kernel_note}.\n//! Types only — aggregates/handlers/folds stay in their layer crates; the `domain` facade\n//! re-exports these modules so existing `domain::generated::*` paths keep the same type identity.\n\n",
            scope = scope,
            crate_name = domain_crate_name(scope),
            kernel_note = if scope == KERNEL_SCOPE {
                " — the KERNEL: cross-scope contracts every scope may build on (and nothing else may leak into)"
            } else {
                ""
            },
        );
        for m in &modules {
            lib.push_str(&format!("pub mod {};\n", m));
        }
        // uuid/serde_json only when the emitted code actually names them (keeps every manifest
        // dependency earned — an unused dep is a drift signal, not a convenience).
        let all_src = files.iter().map(|(_, c)| c.as_str()).collect::<Vec<_>>().join("\n");
        let manifest = scope_manifest(
            scope,
            &dep_scopes,
            all_src.contains("uuid::Uuid"),
            all_src.contains("serde_json"),
        );
        files.insert(0, ("src/lib.rs".to_string(), lib));
        out.push(DomainScopeCrate {
            scope: scope.clone(),
            dir: format!("crates/domains/{}", scope),
            manifest,
            files,
            dep_scopes,
        });
    }
    out
}

/// The manifest of one per-scope domain crate. Dependencies are DERIVED (the `$ref` reach and the
/// kernel `ErrorDef` edge) — the manifest is where the spec's coupling becomes Cargo's, so a new
/// cross-scope edge shows up here as a reviewable diff.
fn scope_manifest(scope: &str, dep_scopes: &BTreeSet<String>, uuid: bool, serde_json: bool) -> String {
    let mut deps = String::new();
    for d in dep_scopes {
        deps.push_str(&format!("{} = {{ path = \"../{}\" }}\n", domain_crate_name(d), d));
    }
    let mut extra = String::new();
    if serde_json {
        extra.push_str("serde_json = { workspace = true }\n");
    }
    if uuid {
        extra.push_str("uuid = { workspace = true }\n");
    }
    format!(
        r#"# GENERATED by the Captain.Food codegen from specs/{scope}/ -- do not edit by hand
# (#373, ADR-20260807-183024 step 2, PROP-20260807-174246).
#
# PER-SCOPE DOMAIN CRATE: this scope's generated types, and ONLY the dependencies its spec
# fragments' $refs earn -- the [dependencies] below are DERIVED from the cross-scope $ref edges
# the section-14 validator proves acyclic (process managers are declared bridges and appear in
# the bin graph instead, never here). Linking this crate is linking exactly this scope's
# vocabulary; a new cross-scope edge lands here as a reviewable manifest diff.
[package]
name = "{name}"
version = "0.1.0"
edition.workspace = true
license-file.workspace = true
publish = false
description = "{desc}"

[dependencies]
{deps}serde = {{ workspace = true }}
{extra}
# D6 lint floor (PROP-20260802-130500, #302): inherit the workspace [lints] baseline.
[lints]
workspace = true
"#,
        scope = scope,
        name = domain_crate_name(scope),
        desc = if scope == KERNEL_SCOPE {
            "Captain.Food domain KERNEL: the cross-scope contracts (shared scalars, Money, Address, ErrorDef) every scope crate builds on -- changes here ripple every scope, correctly (#373).".to_string()
        } else {
            format!(
                "Captain.Food `{}` scope: generated domain types (scalars, entities, events, commands, errors) -- the compile-time face of specs/{}/ (#373).",
                scope, scope
            )
        },
        deps = deps,
        extra = extra,
    )
}

/// The kernel's shared error mechanics: `ErrorDef` and the `{placeholder}` interpolation. Emitted
/// ONCE into `domain-common`; every scope's constants are typed by it, and the facade's global
/// catalog (`ERRORS`/`find`/`message_*`) re-exports it.
const ERROR_DEF_AND_INTERPOLATE: &str = r#"
/// One anticipated error (errors.yaml): the stable wire code + its localized message templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorDef {
    /// The stable PascalCase error code (= the errors.yaml key = the wire `extensions.code`).
    pub code: &'static str,
    /// English message template; `{placeholder}` tokens name fields of the error's typed context.
    pub message_en: &'static str,
    /// French message template (same tokens).
    pub message_fr: &'static str,
}

/// Interpolate the `{placeholder}` tokens of `template` from the rejection's JSON `context` object:
/// a token naming a context field renders its value (strings verbatim, other JSON values via their
/// canonical JSON form); a token with no matching field is left as-is (a visible spec/context gap,
/// never a panic).
pub fn interpolate(template: &str, context: &serde_json::Value) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match context.get(key) {
                    Some(serde_json::Value::String(s)) => out.push_str(s),
                    Some(v) => out.push_str(&v.to_string()),
                    None => {
                        out.push('{');
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}
"#;

// ─── The derived crate graph: specs/generated/crate-graph.generated.json ────────────────────────

/// The bin an actor maps to at step (3): `actor-{kebab}` for aggregates, `pm-{kebab}` for process
/// managers with a trailing `Process` stripped (D5 addendum naming directive, 2026-08-07:
/// `PlaceOrderProcess` → `pm-place-order`).
pub(crate) fn actor_bin_name(actor: &str, is_pm: bool) -> String {
    if is_pm {
        let base = actor.strip_suffix("Process").filter(|s| !s.is_empty()).unwrap_or(actor);
        format!("pm-{}", kebab(base))
    } else {
        format!("actor-{}", kebab(actor))
    }
}

/// Every actor/PM and the SCOPES its declarations reach: its own folder scope plus the origin
/// scope of every `$ref` in its `actors.yaml`/`processmanager.yaml` definitions (both merged when
/// an actor appears in both — one actor, one bin). This is the PM-bridge doctrine made
/// load-bearing: a PM's refs ARE its dependency list, and become the pm bin's domain-crate
/// dependencies at step (3).
pub(crate) fn actor_scope_links(model: &Model) -> Vec<(String, bool, BTreeSet<String>)> {
    let mut links: BTreeMap<String, (bool, BTreeSet<String>)> = BTreeMap::new();
    for file in ["actors.yaml", "processmanager.yaml"] {
        let Some(Value::Mapping(m)) = model.defs.get(file) else { continue };
        for (k, def) in m {
            let Some(name) = k.as_str().filter(|s| *s != "principals") else { continue };
            let is_pm = file == "processmanager.yaml"
                || def.get("type").and_then(|t| t.as_str()) == Some("process-manager");
            if file == "actors.yaml"
                && !matches!(
                    def.get("type").and_then(|t| t.as_str()),
                    Some("aggregate" | "process-manager")
                )
            {
                continue;
            }
            let entry = links.entry(name.to_string()).or_insert_with(|| (false, BTreeSet::new()));
            entry.0 |= is_pm;
            if let Some(own) = model.origins.get(&(file.to_string(), name.to_string())) {
                entry.1.insert(own.clone());
            }
            let mut refs = Vec::new();
            collect_refs(def, "", &mut refs);
            for (_, r) in refs {
                let Some(pr) = parse_ref(&r) else { continue };
                let file = if pr.file.is_empty() { file.to_string() } else { pr.file };
                let key = if SCOPED_SECTION_KINDS.iter().any(|(k, _)| *k == file) {
                    if pr.path.len() >= 2 {
                        format!("{}/{}", pr.path[0], pr.path[1])
                    } else {
                        continue;
                    }
                } else {
                    match pr.path.first() {
                        Some(k) => k.clone(),
                        None => continue,
                    }
                };
                if let Some(tsc) = model.origins.get(&(file, key)) {
                    entry.1.insert(tsc.clone());
                }
            }
        }
    }
    links.into_iter().map(|(n, (pm, s))| (n, pm, s)).collect()
}

/// Emit `specs/generated/crate-graph.generated.json` — the derived crate topology, committed so
/// the `$ref`→dependency derivation has a REVIEWABLE face: `domain_crates` (scope crate → its
/// derived deps) and `bins` (each actor/PM bin → the domain crates its spec declarations reach —
/// step (3)'s emitter consumes exactly this to write the bin manifests).
pub(crate) fn emit_crate_graph(model: &Model) -> String {
    let crates = emit_domain_scope_crates(model);
    let mut domain_crates = serde_json::Map::new();
    for c in &crates {
        let deps: Vec<serde_json::Value> = c
            .dep_scopes
            .iter()
            .map(|d| serde_json::Value::String(domain_crate_name(d)))
            .collect();
        domain_crates.insert(
            domain_crate_name(&c.scope),
            serde_json::json!({ "scope": c.scope, "path": c.dir, "deps": deps }),
        );
    }
    let emitted: BTreeSet<String> = crates.iter().map(|c| c.scope.clone()).collect();
    let mut bins = serde_json::Map::new();
    for (actor, is_pm, scopes) in actor_scope_links(model) {
        let linked: Vec<serde_json::Value> = scopes
            .iter()
            .filter(|s| emitted.contains(*s))
            .map(|s| serde_json::Value::String(domain_crate_name(s)))
            .collect();
        bins.insert(
            actor_bin_name(&actor, is_pm),
            serde_json::json!({
                "actor": actor,
                "kind": if is_pm { "process-manager" } else { "aggregate" },
                "domain_crates": linked,
            }),
        );
    }
    let doc = serde_json::json!({
        "_generated": "GENERATED by the Captain.Food codegen from specs/{scope}/ $refs (#373, ADR-20260807-183024 step 2) -- do not edit by hand. The derived crate topology: the spec's coupling as Cargo's. `bins` is the input contract for step (3)'s bin emitter -- a PM's refs are its declared dependency list.",
        "domain_crates": serde_json::Value::Object(domain_crates),
        "bins": serde_json::Value::Object(bins),
    });
    let mut s = serde_json::to_string_pretty(&doc).expect("crate graph serializes");
    s.push('\n');
    s
}
