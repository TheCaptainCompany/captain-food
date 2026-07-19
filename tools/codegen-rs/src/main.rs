//! Captain.Food codegen (ADR-0034) — the single spec gate.
//!
//! It loads every `specs/**` DSL file and runs the full validator (§1–§11: referential integrity, actor
//! wiring, api↔model, views, stories, tests, rules, translations, screens, observability, C4) and every
//! generator (translations, views SQL + the `database.md` §2 injection, C4 Structurizr/Mermaid, GraphQL
//! SDL, and the Markdown + HTML docs). It began as a TypeScript tool (`tools/codegen`) and was ported here
//! at parity — all 8 generated artifacts byte-identical and the same (rule, location) validation issue set
//! (verified by a differential harness) — after which the TypeScript codegen was retired. CI (`codegen`
//! job) builds + tests, validates, regenerates and fails on any drift.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use serde_yaml::Value;

/// The DSL source files, in load order (mirrors model.ts `SOURCE_FILES`).
const SOURCE_FILES: &[&str] = &[
    "scalars.yaml",
    "entities.yaml",
    "events.yaml",
    "commands.yaml",
    "errors.yaml",
    "actors.yaml",
    "processmanager.yaml",
    "services.yaml",
    "database/projection_views.yaml",
    "api.yaml",
    "stories.yaml",
    "rules.yaml",
    "tests.yaml",
    "translations.yaml",
    "screens/customer_screens.yaml",
    "observability.yaml",
    "architecture/c4-l2.yaml",
    "architecture/c4-l3.yaml",
];

/// The loaded model: each source file parsed into its YAML `Value` (the full top-level mapping).
struct Model {
    defs: BTreeMap<String, Value>,
}

/// Strip file-level meta (version/description) like load.ts META_KEYS, preserving key order.
fn strip_meta(parsed: Value) -> Value {
    match parsed {
        Value::Mapping(m) => {
            let mut nm = serde_yaml::Mapping::new();
            for (k, val) in m {
                if matches!(k.as_str(), Some("version") | Some("description")) {
                    continue;
                }
                nm.insert(k, val);
            }
            Value::Mapping(nm)
        }
        other => other,
    }
}

fn load_model(specs: &PathBuf) -> Result<Model, String> {
    let mut defs = BTreeMap::new();
    let mut load = |key: String, p: &std::path::Path| -> Result<(), String> {
        let s = fs::read_to_string(p).map_err(|e| format!("read {}: {}", p.display(), e))?;
        let parsed: Value = serde_yaml::from_str(&s).map_err(|e| format!("parse {}: {}", key, e))?;
        defs.insert(key, strip_meta(parsed));
        Ok(())
    };
    for &f in SOURCE_FILES {
        load(f.to_string(), &specs.join(f))?;
    }
    // Generic: every `specs/database/tables/*.yaml` is a real-table spec (ADR-0037), keyed by its path —
    // drop a file in and it's picked up (eventstore.yaml, referential.yaml, …). Sorted for determinism.
    let tdir = specs.join("database/tables");
    if let Ok(rd) = fs::read_dir(&tdir) {
        let mut paths: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("yaml"))
            .collect();
        paths.sort();
        for p in paths {
            let name = match p.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            load(format!("database/tables/{}", name), &p)?;
        }
    }
    Ok(Model { defs })
}

/// A parsed `<file>#/<a>/<b>` reference. `file` is empty for a local `#/…` ref (resolved against context).
struct ParsedRef {
    file: String,
    path: Vec<String>,
}

/// Mirrors refs.ts `parseRef`: split on the first `#/`; the pointer is split on `/` (dotted keys such as
/// translation keys `home.title` stay a single segment — they contain no `/`).
fn parse_ref(r: &str) -> Option<ParsedRef> {
    let idx = r.find("#/")?;
    let file = r[..idx].to_string();
    let pointer = &r[idx + 2..];
    if pointer.is_empty() {
        return None;
    }
    let path = pointer
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    Some(ParsedRef { file, path })
}

fn is_source_file(f: &str) -> bool {
    SOURCE_FILES.contains(&f) || (f.starts_with("database/tables/") && f.ends_with(".yaml"))
}

/// The bare definition name a `$ref` denotes: the FIRST pointer segment (mirrors refs.ts `refName`).
fn ref_name(r: &str) -> Option<String> {
    parse_ref(r).and_then(|p| p.path.into_iter().next())
}

/// Mirrors refs.ts `resolveRef`: resolve `ref` (appearing in `ctx`) into the target file's Value tree.
fn resolve_ref<'a>(model: &'a Model, r: &str, ctx: &str) -> Option<&'a Value> {
    let pr = parse_ref(r)?;
    let file = if pr.file.is_empty() {
        ctx.to_string()
    } else {
        pr.file
    };
    if !is_source_file(&file) {
        return None;
    }
    let mut node = model.defs.get(&file)?;
    for seg in &pr.path {
        node = node.get(seg.as_str())?;
    }
    Some(node)
}

/// Recursively collect every `$ref` string with a human-readable location (mirrors refs.ts `collectRefs`).
fn collect_refs(v: &Value, loc: &str, out: &mut Vec<(String, String)>) {
    match v {
        Value::Mapping(m) => {
            for (k, val) in m {
                let key = k.as_str().unwrap_or("?");
                if key == "$ref" {
                    if let Some(r) = val.as_str() {
                        out.push((loc.to_string(), r.to_string()));
                    }
                } else {
                    collect_refs(val, &format!("{}.{}", loc, key), out);
                }
            }
        }
        Value::Sequence(s) => {
            for (i, val) in s.iter().enumerate() {
                collect_refs(val, &format!("{}[{}]", loc, i), out);
            }
        }
        _ => {}
    }
}

// ─── Validation report (faithful port of validate.ts) ───────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Error,
    Warning,
}

#[derive(Clone)]
struct Issue {
    level: Level,
    rule: &'static str,
    location: String,
    message: String,
}

fn err(rule: &'static str, location: String, message: String) -> Issue {
    Issue { level: Level::Error, rule, location, message }
}
fn warn(rule: &'static str, location: String, message: String) -> Issue {
    Issue { level: Level::Warning, rule, location, message }
}

/// Count of what was actually checked — so a clean run shows coverage, not just silence (Coverage in TS).
#[derive(Default)]
struct Coverage {
    refs: usize,
    views: usize,
    view_columns: usize,
    view_fed_by: usize,
    mutation_links: usize,
    reads_links: usize,
    story_links: usize,
    test_cases: usize,
    rules: usize,
    obs_contracts: usize,
    translations: usize,
    screens: usize,
    screen_bindings: usize,
    screen_gaps: usize,
}

struct Report {
    issues: Vec<Issue>,
    coverage: Coverage,
    /// Commands actually handled by some actor (the cli's "commands" count; ≤ total command defs, the
    /// difference being command value objects referenced only from `properties`).
    handled_commands: usize,
}

const INLINE_TYPES: [&str; 4] = ["string", "boolean", "integer", "float"];

/// checkRoles: an operation must declare ≥1 role, and each must be a scalars.yaml#/UserType value.
fn check_roles(issues: &mut Vec<Issue>, roles: &[String], where_: &str, uts: &BTreeSet<String>) {
    if roles.is_empty() {
        issues.push(err("op-no-authz", where_.into(), "operation declares no roles (→ @auth/@public).".into()));
    }
    for r in roles {
        if !uts.contains(r) {
            issues.push(err(
                "op-unknown-usertype",
                where_.into(),
                format!("unknown user type '{}' (not in scalars.yaml#/UserType).", r),
            ));
        }
    }
}

/// checkInline: a non-`$ref` field must use one of the inline primitive types.
fn check_inline(issues: &mut Vec<Issue>, f: &ApiField, where_: &str) {
    if !f.is_ref && !INLINE_TYPES.contains(&f.ty.as_str()) {
        issues.push(err(
            "api-inline-type",
            where_.into(),
            format!("inline type '{}' must be one of {} (or a $ref).", f.ty, INLINE_TYPES.join("|")),
        ));
    }
}

/// checkShape: every REQUIRED property is set and no UNKNOWN field appears; recurses through `$ref`s,
/// inline `properties` and `array` items (mirrors validate.ts §7 checkShape).
fn check_shape(model: &Model, issues: &mut Vec<Issue>, node: Option<&Value>, data: Option<&Value>, where_: &str) {
    let node = match node {
        Some(n) => n,
        None => return,
    };
    if let Some(rf) = node.get("$ref").and_then(|x| x.as_str()) {
        check_shape(model, issues, resolve_ref(model, rf, "tests.yaml"), data, where_);
        return;
    }
    if let Some(props) = node.get("properties").and_then(|p| p.as_mapping()) {
        let required: Vec<&str> = node
            .get("required")
            .and_then(|r| r.as_sequence())
            .map(|s| s.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let obj = data.and_then(|d| d.as_mapping());
        for r in &required {
            let present = obj.map(|o| o.contains_key(Value::String((*r).to_string()))).unwrap_or(false);
            if !present {
                issues.push(err(
                    "test-missing-required",
                    format!("{}.{}", where_, r),
                    format!("required property '{}' is not set by the data.", r),
                ));
            }
        }
        if let Some(o) = obj {
            for (k, v) in o {
                let key = match k.as_str() {
                    Some(s) => s,
                    None => continue,
                };
                match props.get(Value::String(key.to_string())) {
                    None => issues.push(err(
                        "test-unknown-field",
                        format!("{}.{}", where_, key),
                        format!("data field '{}' is not a property of this schema.", key),
                    )),
                    Some(child) => check_shape(model, issues, Some(child), Some(v), &format!("{}.{}", where_, key)),
                }
            }
        }
        return;
    }
    if node.get("type").and_then(|x| x.as_str()) == Some("array") {
        if let (Some(items), Some(arr)) = (node.get("items"), data.and_then(|d| d.as_sequence())) {
            for (i, item) in arr.iter().enumerate() {
                check_shape(model, issues, Some(items), Some(item), &format!("{}[{}]", where_, i));
            }
        }
    }
    // otherwise a leaf (scalar / primitive) — nothing to check.
}

/// The event name a `#/fixtures/<name>` ref ultimately denotes (via its `type.$ref`).
fn fixture_event(model: &Model, fx_ref: Option<&str>) -> Option<String> {
    let fx = resolve_ref(model, fx_ref?, "tests.yaml")?;
    ref_name(fx.get("type")?.get("$ref")?.as_str()?)
}

/// `{param}` placeholder names in a string (mirrors `/\{(\w+)\}/g`, `\w` = ASCII alnum + `_`).
fn placeholders(v: Option<&Value>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let s = match v.and_then(|x| x.as_str()) {
        Some(s) => s,
        None => return out,
    };
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            let mut j = i + 1;
            let mut name = String::new();
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                name.push(chars[j]);
                j += 1;
            }
            if !name.is_empty() && j < chars.len() && chars[j] == '}' {
                out.insert(name);
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn map_keys(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_mapping())
        .map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

/// The full validator — a faithful port of validate.ts §1–§11. Returns issues + coverage.
fn validate(model: &Model) -> Report {
    let mut issues: Vec<Issue> = Vec::new();
    let mut cov = Coverage::default();

    // --- 1. Referential integrity: every `$ref` anywhere must resolve ---------------------------
    // Iterate every loaded file (incl. globbed database/tables/*.yaml), not just the fixed SOURCE_FILES.
    for (f, v) in &model.defs {
        {
            let f = f.as_str();
            let mut refs = Vec::new();
            collect_refs(v, f, &mut refs);
            for (loc, r) in refs {
                cov.refs += 1;
                if parse_ref(&r).is_none() {
                    issues.push(err("ref-format", loc, format!("Malformed $ref '{}'.", r)));
                } else if resolve_ref(model, &r, f).is_none() {
                    issues.push(err("ref-dangling", loc, format!("$ref '{}' does not resolve.", r)));
                }
            }
        }
    }

    let actors = parse_actors(model);
    let api = parse_api(model);

    // --- 2. Actor wiring: messages, emits and throws must target the right kind of file ---------
    let mut handled_commands: BTreeSet<String> = BTreeSet::new();
    let mut emitted_events: BTreeSet<String> = BTreeSet::new();
    let mut consumed_events: BTreeSet<String> = BTreeSet::new();
    for actor in &actors {
        for (i, entry) in actor.receives.iter().enumerate() {
            let where_ = format!("{}/{}.receives[{}]", actor.file, actor.name, i);
            if entry.message_ref.is_empty() {
                issues.push(err("actor-message", where_.clone(), "receives entry has no message $ref.".into()));
            } else if ref_target_file(&entry.message_ref, "actors.yaml").as_deref() == Some("commands.yaml") {
                if let Some(n) = ref_name(&entry.message_ref) {
                    handled_commands.insert(n);
                }
            } else if ref_target_file(&entry.message_ref, "actors.yaml").as_deref() == Some("events.yaml") {
                if let Some(n) = ref_name(&entry.message_ref) {
                    consumed_events.insert(n);
                }
            } else {
                issues.push(err(
                    "actor-message",
                    format!("{}.message", where_),
                    format!("message must reference commands.yaml or events.yaml, got '{}'.", entry.message_ref),
                ));
            }
            for (j, e) in entry.emits.iter().enumerate() {
                if ref_target_file(e, "actors.yaml").as_deref() != Some("events.yaml") {
                    issues.push(err(
                        "actor-emits",
                        format!("{}.emits[{}]", where_, j),
                        format!("emits must reference events.yaml, got '{}'.", e),
                    ));
                } else if let Some(n) = ref_name(e) {
                    emitted_events.insert(n);
                }
            }
            for (j, t) in entry.throws.iter().enumerate() {
                if ref_target_file(t, "actors.yaml").as_deref() != Some("errors.yaml") {
                    issues.push(err(
                        "actor-throws",
                        format!("{}.throws[{}]", where_, j),
                        format!("throws must reference errors.yaml, got '{}'.", t),
                    ));
                }
            }
        }
    }

    // --- 2b. Process managers (processmanager.yaml): typed-step validation -----------------------
    validate_process_managers(model, &mut issues);

    // --- 2d. Service catalog (services.yaml, ADR-20260719-214500) --------------------------------
    validate_services(model, &mut issues);

    // --- 3. Coverage: derive value-objects vs commands, and orphan events ------------------------
    let mut refd_from_properties: BTreeSet<String> = BTreeSet::new();
    for (f, v) in &model.defs {
        {
            let f = f.as_str();
            let mut refs = Vec::new();
            collect_refs(v, f, &mut refs);
            for (loc, r) in refs {
                if ref_target_file(&r, f).as_deref() == Some("commands.yaml") && loc.contains(".properties.") {
                    if let Some(n) = ref_name(&r) {
                        refd_from_properties.insert(n);
                    }
                }
            }
        }
    }
    for c in map_keys(model.defs.get("commands.yaml")) {
        if handled_commands.contains(&c) {
            continue;
        }
        if !refd_from_properties.contains(&c) {
            issues.push(warn(
                "command-unhandled",
                format!("commands.yaml/{}", c),
                format!("Command '{}' is defined but no actor handles it.", c),
            ));
        }
    }
    let mut produced_events: BTreeSet<String> = emitted_events.clone();
    produced_events.extend(consumed_events.iter().cloned());
    for e in map_keys(model.defs.get("events.yaml")) {
        if !produced_events.contains(&e) {
            issues.push(warn(
                "event-orphan",
                format!("events.yaml/{}", e),
                format!("Event '{}' is never emitted nor consumed by any actor.", e),
            ));
        }
    }

    // --- 4. API surface (api.yaml ↔ model) ------------------------------------------------------
    let user_type_set: BTreeSet<String> = model
        .defs
        .get("scalars.yaml")
        .and_then(|s| s.get("UserType"))
        .and_then(|u| u.get("enum"))
        .and_then(|e| e.as_sequence())
        .map(|s| s.iter().filter_map(|v| v.as_str().map(|x| x.to_string())).collect())
        .unwrap_or_default();
    let all_commands: BTreeSet<String> = map_keys(model.defs.get("commands.yaml")).into_iter().collect();

    // 4a. mutations
    let mut declared_by_command: BTreeMap<String, String> = BTreeMap::new();
    for m in &api.mutations {
        let where_ = format!("api.yaml/mutations.{}", m.name);
        check_roles(&mut issues, &m.roles, &where_, &user_type_set);
        if m.command.is_empty() {
            issues.push(err("op-missing-command", where_.clone(), "mutation declares no command.".into()));
        } else if !all_commands.contains(&m.command) {
            issues.push(err(
                "mutation-unknown-command",
                where_.clone(),
                format!("command '{}' is not defined in commands.yaml.", m.command),
            ));
        } else if !handled_commands.contains(&m.command) {
            issues.push(warn(
                "mutation-command-unhandled",
                where_.clone(),
                format!("command '{}' has no actor handler.", m.command),
            ));
        }
        if !m.command.is_empty() {
            if let Some(prev) = declared_by_command.get(&m.command) {
                issues.push(err(
                    "command-duplicate-mutation",
                    where_.clone(),
                    format!("command '{}' is already dispatched by mutation '{}'.", m.command, prev),
                ));
            } else {
                declared_by_command.insert(m.command.clone(), m.name.clone());
            }
        }
        for f in &m.payload {
            check_inline(&mut issues, f, &format!("{}.payload.{}", where_, f.name));
        }
    }
    cov.mutation_links = declared_by_command.len();
    // 4b. every handled command must be dispatched by exactly one mutation.
    for cmd in &handled_commands {
        if !declared_by_command.contains_key(cmd) {
            issues.push(warn(
                "command-no-mutation",
                format!("commands.yaml/{}", cmd),
                format!("Handled command '{}' is not dispatched by any mutation.", cmd),
            ));
        }
    }

    // 4c. queries
    let mut output_types: BTreeSet<String> = map_keys(model.defs.get("entities.yaml")).into_iter().collect();
    for t in &api.types {
        output_types.insert(t.name.clone());
    }
    let transient_types: BTreeSet<String> =
        api.types.iter().filter(|t| t.reads.is_empty()).map(|t| t.name.clone()).collect();
    for q in &api.queries {
        let where_ = format!("api.yaml/queries.{}", q.name);
        check_roles(&mut issues, &q.roles, &where_, &user_type_set);
        if q.reads.is_empty() && !transient_types.contains(&q.returns_type) {
            issues.push(err(
                "op-missing-reads",
                where_.clone(),
                format!(
                    "return type '{}' declares no `reads` binding (→ @reads); bind it to a View_* in api.yaml types.",
                    if q.returns_type.is_empty() { "?" } else { &q.returns_type }
                ),
            ));
        }
        if q.returns_type.is_empty() {
            issues.push(err("query-no-returns", where_.clone(), "query has no return type.".into()));
        } else if !output_types.contains(&q.returns_type) {
            issues.push(err(
                "query-unknown-type",
                where_.clone(),
                format!("return type '{}' is neither an entities.yaml type nor an api projection.", q.returns_type),
            ));
        }
        for a in &q.args {
            check_inline(&mut issues, a, &format!("{}.args.{}", where_, a.name));
        }
    }

    // 4d. subscriptions
    for s in &api.subscriptions {
        let where_ = format!("api.yaml/subscriptions.{}", s.name);
        check_roles(&mut issues, &s.roles, &where_, &user_type_set);
        if s.returns_type.is_empty() {
            issues.push(err("subscription-no-returns", where_.clone(), "subscription has no return type.".into()));
        } else if !output_types.contains(&s.returns_type) {
            issues.push(err(
                "subscription-unknown-type",
                where_.clone(),
                format!("return type '{}' is neither an entities.yaml type nor an api projection.", s.returns_type),
            ));
        }
        for a in &s.args {
            check_inline(&mut issues, a, &format!("{}.args.{}", where_, a.name));
        }
    }

    // --- 5. Read models (views.yaml) ------------------------------------------------------------
    let sql_primitives: BTreeSet<&str> =
        ["uuid", "text", "integer", "bigint", "boolean", "timestamptz", "jsonb", "numeric"].into_iter().collect();
    let scalar_names: BTreeSet<String> = map_keys(model.defs.get("scalars.yaml")).into_iter().collect();
    let aggregate_names: BTreeSet<String> =
        actors.iter().filter(|a| a.kind == "aggregate").map(|a| a.name.clone()).collect();
    let views = parse_views(model);

    cov.views = views.len();
    for view in &views {
        let at = format!("views.yaml/{}", view.name);
        cov.view_columns += view.columns.len();
        cov.view_fed_by += view.fedby.len();
        // Naming convention (ADR-0039): a generated VIEW is `View_*`; a materialized TABLE has no prefix.
        if !view.is_table && !view.name.starts_with("View_") {
            issues.push(warn("view-naming", at.clone(), format!("Fold view '{}' should be prefixed 'View_'.", view.name)));
        }
        if view.is_table && view.name.starts_with("View_") {
            issues.push(warn("view-naming", at.clone(), format!("Materialized table '{}' should NOT be prefixed 'View_'.", view.name)));
        }
        if !view.reference && !aggregate_names.contains(&view.aggregate) {
            issues.push(err(
                "view-unknown-aggregate",
                at.clone(),
                format!("aggregate '{}' is not an aggregate in actors.yaml.", view.aggregate),
            ));
        }
        if view.columns.is_empty() {
            issues.push(err("view-no-columns", at.clone(), "view has no columns.".into()));
        }

        let col_names: BTreeSet<&str> = view.columns.iter().map(|c| c.name.as_str()).collect();
        let fed_by_names: BTreeSet<&str> = view.fedby.iter().map(|s| s.as_str()).collect();
        let mut used_events: BTreeSet<String> = BTreeSet::new();
        let mut pk_count = 0;
        for col in &view.columns {
            if col.pk {
                pk_count += 1;
            }
            if col.ty.is_empty() {
                issues.push(err(
                    "view-column-no-type",
                    format!("{}.{}", at, col.name),
                    "column has no `type` and none could be derived from `from` (declare a type or map it to a typed event property).".into(),
                ));
            } else if !sql_primitives.contains(col.ty.as_str()) && !scalar_names.contains(&col.ty) {
                issues.push(err(
                    "view-column-type",
                    format!("{}.{}", at, col.name),
                    format!("type '{}' is neither a SQL primitive nor a scalars.yaml type.", col.ty),
                ));
            }
            // created_at/updated_at are IMPLICIT technical columns (stamped from event.occurred_at,
            // ADR-0040) — no `from`, and not a design hole.
            let is_technical_ts = col.name == "created_at" || col.name == "updated_at";
            if col.from.is_empty() {
                if !view.reference && !is_technical_ts {
                    issues.push(warn(
                        "view-column-no-source",
                        format!("{}.{}", at, col.name),
                        "column has no `from` — not traced to any event (possible design hole).".into(),
                    ));
                }
            } else {
                for r in &col.from {
                    if let Some(ev) = ref_name(r) {
                        if !fed_by_names.contains(ev.as_str()) {
                            issues.push(err(
                                "view-column-source-not-fedby",
                                format!("{}.{}", at, col.name),
                                format!("from '{}' refers to event '{}', which is not in this view's fedBy.", r, ev),
                            ));
                        }
                        used_events.insert(ev);
                    }
                }
            }
            if let Some(fk) = &col.fk {
                let mut parts = fk.splitn(2, '.');
                let fk_view = parts.next().unwrap_or("");
                let fk_col = parts.next().unwrap_or("");
                match views.iter().find(|v| v.name == fk_view) {
                    None => issues.push(err(
                        "view-fk-unknown-view",
                        format!("{}.{}", at, col.name),
                        format!("fk '{}' references unknown view '{}'.", fk, fk_view),
                    )),
                    Some(target) => {
                        if !target.columns.iter().any(|c| c.name == fk_col) {
                            issues.push(err(
                                "view-fk-unknown-column",
                                format!("{}.{}", at, col.name),
                                format!("fk '{}' references unknown column '{}' on '{}'.", fk, fk_col, fk_view),
                            ));
                        }
                    }
                }
            }
        }
        if pk_count == 0 {
            issues.push(warn("view-no-pk", at.clone(), "view declares no primary-key column.".into()));
        }

        for (i, n) in view.fedby.iter().enumerate() {
            if !produced_events.contains(n) {
                issues.push(warn(
                    "view-fedby-unproduced",
                    format!("{}.fedBy[{}]", at, i),
                    format!("fed by '{}', which no actor emits or consumes.", n),
                ));
            }
        }
        for (i, ix) in view.indexes.iter().enumerate() {
            for c in ix {
                if !col_names.contains(c.as_str()) {
                    issues.push(err(
                        "view-index-column",
                        format!("{}.indexes[{}]", at, i),
                        format!("index references unknown column '{}'.", c),
                    ));
                }
            }
        }
        if !used_events.is_empty() {
            let mut seen: BTreeSet<&str> = BTreeSet::new();
            for ev in &view.fedby {
                if !seen.insert(ev.as_str()) {
                    continue;
                }
                if !used_events.contains(ev) {
                    issues.push(warn(
                        "view-fedby-unused",
                        at.clone(),
                        format!("fed by '{}' but no column maps `from` it (possible design hole).", ev),
                    ));
                }
            }
        }
    }

    // 5b. every emitted event should be projected into a view, unless declared non-projected.
    let non_projected: BTreeSet<String> = model
        .defs
        .get("database/projection_views.yaml")
        .and_then(|v| v.get("nonProjectedEvents"))
        .and_then(|x| x.as_sequence())
        .map(|s| s.iter().filter_map(|r| r.get("$ref").and_then(|x| x.as_str()).and_then(ref_name)).collect())
        .unwrap_or_default();
    for e in &emitted_events {
        if non_projected.contains(e) {
            continue;
        }
        if !views.iter().any(|v| v.fedby.iter().any(|n| n == e)) {
            issues.push(warn(
                "event-not-projected",
                format!("events.yaml/{}", e),
                format!("Emitted event '{}' feeds no View_* (mark it under views.yaml nonProjectedEvents if intentional).", e),
            ));
        }
    }

    // 5b-bis. Read-model form (ADR-0039): a fold VIEW (projection_views.yaml) must be generatable from its
    // column lineage; a materialized TABLE (projection_tables.yaml) must declare its projector mechanism.
    for view in &views {
        if view.reference {
            continue;
        }
        if view.is_table {
            if view.projector.as_deref() != Some("app") {
                issues.push(err(
                    "projection-table-no-projector",
                    format!("projection_tables.yaml/{}", view.name),
                    "a materialized read-model table must declare `projector: app` (application-layer Rust projector; no SQL triggers — ADR-0040).".into(),
                ));
            }
        } else if view.definition.is_none() {
            if let Err(e) = generate_fold_sql(view, model) {
                issues.push(err(
                    "view-fold-ungeneratable",
                    format!("projection_views.yaml/{}", view.name),
                    format!("fold view cannot be generated: {} (move it to projection_tables.yaml if computed).", e),
                ));
            }
        }
    }

    // 5c. type `reads` (api.yaml) bind output types to views.
    {
        // Valid read targets = projection views (projection_views.yaml) PLUS reference/config tables
        // under database/tables/*.yaml that opt in with `reference: true` (referential.yaml) — both back
        // queries via `reads`. The event-store tables (domain_events/domain_stream) are NOT read targets.
        let mut view_names: BTreeSet<String> = views.iter().map(|v| v.name.clone()).collect();
        for (_k, val) in model.defs.iter().filter(|(k, _)| k.starts_with("database/tables/")) {
            if let Value::Mapping(m) = val {
                for (tk, tv) in m {
                    if let Some(n) = tk.as_str() {
                        if tv.get("reference").and_then(|b| b.as_bool()) == Some(true) {
                            view_names.insert(n.to_string());
                        }
                    }
                }
            }
        }
        let internal_views: BTreeSet<&str> = views.iter().filter(|v| v.internal).map(|v| v.name.as_str()).collect();
        let mut bound_views: BTreeSet<String> = BTreeSet::new();
        for t in &api.types {
            for v in &t.reads {
                cov.reads_links += 1;
                bound_views.insert(v.clone());
                if !view_names.contains(v.as_str()) {
                    issues.push(err(
                        "reads-unknown-view",
                        format!("api.yaml/types.{}", t.name),
                        format!("reads references unknown view '{}'.", v),
                    ));
                }
            }
        }
        for v in &views {
            if !bound_views.contains(&v.name) && !internal_views.contains(v.name.as_str()) {
                issues.push(warn(
                    "view-no-query",
                    format!("views.yaml/{}", v.name),
                    format!("View '{}' is bound by no output type (api.yaml types reads).", v.name),
                ));
            }
        }
    }

    // --- 6. Story map (stories.yaml): personas → activities → steps -----------------------------
    let personas = parse_stories(model);
    {
        let query_roles: HashMap<&str, &Vec<String>> = api.queries.iter().map(|q| (q.name.as_str(), &q.roles)).collect();
        let mutation_roles: HashMap<&str, &Vec<String>> =
            api.mutations.iter().map(|m| (m.name.as_str(), &m.roles)).collect();
        for p in &personas {
            let at = format!("stories.yaml/{}", p.name);
            if p.role.is_empty() {
                issues.push(err("persona-no-role", at.clone(), "persona declares no personaRole.".into()));
            } else if !user_type_set.contains(&p.role) {
                issues.push(err(
                    "persona-unknown-role",
                    at.clone(),
                    format!("personaRole '{}' is not a scalars.yaml#/UserType.", p.role),
                ));
            }
            for act in &p.activities {
                for step in &act.steps {
                    let (op, op_kind) = match (&step.op, &step.op_kind) {
                        (Some(o), Some(k)) => (o, k),
                        _ => continue,
                    };
                    cov.story_links += 1;
                    let where_ = format!("{}.{}.{}", at, act.name, step.name);
                    let roles = if op_kind == "query" { query_roles.get(op.as_str()) } else { mutation_roles.get(op.as_str()) };
                    let roles = match roles {
                        Some(r) => *r,
                        None => {
                            issues.push(err(
                                "story-unknown-op",
                                where_.clone(),
                                format!("step references unknown {} '{}'.", op_kind, op),
                            ));
                            continue;
                        }
                    };
                    let allowed = roles.iter().any(|r| r == "PUBLIC") || (!p.role.is_empty() && roles.iter().any(|r| r == &p.role));
                    if !allowed {
                        issues.push(err(
                            "story-role-not-authorized",
                            where_,
                            format!(
                                "persona role '{}' may not call {} '{}' (op roles: [{}]).",
                                p.role,
                                op_kind,
                                op,
                                roles.join(", ")
                            ),
                        ));
                    }
                }
            }
        }
        // COMPLETENESS: every mutation & query must be reached by ≥1 story step.
        let mut story_ops: BTreeSet<&str> = BTreeSet::new();
        for p in &personas {
            for act in &p.activities {
                for step in &act.steps {
                    if let Some(o) = &step.op {
                        story_ops.insert(o.as_str());
                    }
                }
            }
        }
        for m in &api.mutations {
            if !story_ops.contains(m.name.as_str()) {
                issues.push(err(
                    "op-uncovered-by-story",
                    format!("api.yaml/mutations/{}", m.name),
                    format!("mutation '{}' is referenced by no story step (stories.yaml) — every write must anchor to a persona use case.", m.name),
                ));
            }
        }
        for q in &api.queries {
            if !story_ops.contains(q.name.as_str()) {
                issues.push(err(
                    "op-uncovered-by-story",
                    format!("api.yaml/queries/{}", q.name),
                    format!("query '{}' is referenced by no story step (stories.yaml) — every read must anchor to a persona use case.", q.name),
                ));
            }
        }
    }

    // --- 7. Behaviour tests (tests.yaml): fixtures + Given/When/Then consistency -----------------
    {
        let empty = Value::Mapping(Default::default());
        let tests_file = model.defs.get("tests.yaml").unwrap_or(&empty);
        let fixtures = tests_file.get("fixtures").and_then(|x| x.as_mapping());
        let tests = tests_file.get("tests").and_then(|x| x.as_mapping());

        // Per-actor inbox.
        struct InboxEntry {
            actor: String,
            file: &'static str,
            message: String,
            is_command: bool,
            emits: BTreeSet<String>,
            throws: BTreeSet<String>,
        }
        let mut inbox: HashMap<String, HashMap<String, usize>> = HashMap::new();
        let mut inbox_entries: Vec<InboxEntry> = Vec::new();
        let mut t_emitted_events: BTreeSet<String> = BTreeSet::new();
        let mut t_throwable_errors: BTreeSet<String> = BTreeSet::new();
        for a in &actors {
            let mut by_msg: HashMap<String, usize> = HashMap::new();
            for e in &a.receives {
                let msg = match ref_name(&e.message_ref) {
                    Some(m) => m,
                    None => continue,
                };
                let emits: BTreeSet<String> = e.emits.iter().filter_map(|r| ref_name(r)).collect();
                let throws: BTreeSet<String> = e.throws.iter().filter_map(|r| ref_name(r)).collect();
                for ev in &emits {
                    t_emitted_events.insert(ev.clone());
                }
                for er in &throws {
                    t_throwable_errors.insert(er.clone());
                }
                let idx = inbox_entries.len();
                inbox_entries.push(InboxEntry {
                    actor: a.name.clone(),
                    file: a.file,
                    message: msg.clone(),
                    is_command: e.message_ref.starts_with("commands.yaml#/"),
                    emits,
                    throws,
                });
                by_msg.insert(msg, idx);
            }
            inbox.insert(a.name.clone(), by_msg);
        }

        let mut used_messages: BTreeSet<String> = BTreeSet::new();
        let mut used_events: BTreeSet<String> = BTreeSet::new();
        let mut used_errors: BTreeSet<String> = BTreeSet::new();
        let mut used_rules: BTreeSet<String> = BTreeSet::new();
        let all_rules = map_keys(model.defs.get("rules.yaml"));
        cov.rules = all_rules.len();

        // 7a. fixtures: data shape.
        if let Some(fx_map) = fixtures {
            for (k, fx) in fx_map {
                let name = match k.as_str() {
                    Some(s) => s,
                    None => continue,
                };
                let where_ = format!("tests.yaml/fixtures.{}", name);
                match fx.get("type").and_then(|t| t.get("$ref")).and_then(|x| x.as_str()) {
                    None => issues.push(err("fixture-no-type", where_, "fixture has no `type.$ref`.".into())),
                    Some(rf) => check_data_shape(model, &mut issues, rf, fx.get("data"), &where_),
                }
            }
        }

        // 7b. tests.
        cov.test_cases = tests.map(|t| t.len()).unwrap_or(0);
        if let Some(t_map) = tests {
            for (k, t) in t_map {
                let name = match k.as_str() {
                    Some(s) => s,
                    None => continue,
                };
                let where_ = format!("tests.yaml/tests.{}", name);
                let actor_name = t
                    .get("actor")
                    .and_then(|a| a.get("$ref"))
                    .and_then(|x| x.as_str())
                    .and_then(ref_name)
                    .unwrap_or_default();
                let when = t.get("when");
                let when_ref = when.and_then(|w| w.get("type")).and_then(|ty| ty.get("$ref")).and_then(|x| x.as_str());
                let when_ref = match when_ref {
                    Some(r) => r,
                    None => {
                        issues.push(err("test-no-when", where_, "test has no `when.type.$ref` (command or event).".into()));
                        continue;
                    }
                };
                check_data_shape(model, &mut issues, when_ref, when.and_then(|w| w.get("data")), &format!("{}.when", where_));

                let msg = ref_name(when_ref).unwrap_or_default();
                let entry_idx = if !actor_name.is_empty() && !msg.is_empty() {
                    inbox.get(&actor_name).and_then(|m| m.get(&msg)).copied()
                } else {
                    None
                };
                match entry_idx {
                    None => issues.push(err(
                        "test-message-not-handled",
                        format!("{}.when", where_),
                        format!("actor '{}' does not receive '{}' (actors.yaml/processmanager.yaml inbox).", actor_name, msg),
                    )),
                    Some(idx) => {
                        used_messages.insert(format!("{}::{}", actor_name, msg));
                        if !inbox_entries[idx].is_command {
                            used_events.insert(msg.clone());
                        }
                    }
                }

                // `given` preconditions exercise their events too.
                if let Some(given) = t.get("given").and_then(|x| x.as_sequence()) {
                    for g in given {
                        if let Some(ev) = fixture_event(model, g.get("$ref").and_then(|x| x.as_str())) {
                            used_events.insert(ev);
                        }
                    }
                }

                // Every test must assert ≥1 business rule (ADR-0032).
                let test_rules = t.get("rules").and_then(|x| x.as_sequence()).cloned().unwrap_or_default();
                if test_rules.is_empty() {
                    issues.push(err(
                        "test-no-rule",
                        where_.clone(),
                        "test asserts no business rule — add `rules: [{ $ref: 'rules.yaml#/<Rule>' }]` (ADR-0032).".into(),
                    ));
                }
                for (i, r) in test_rules.iter().enumerate() {
                    let rf = r.get("$ref").and_then(|x| x.as_str()).unwrap_or("");
                    if ref_target_file(rf, "tests.yaml").as_deref() != Some("rules.yaml") {
                        issues.push(err(
                            "test-rule-wrong-file",
                            format!("{}.rules[{}]", where_, i),
                            format!("rule ref '{}' must target rules.yaml.", rf),
                        ));
                        continue;
                    }
                    if let Some(rn) = ref_name(rf) {
                        used_rules.insert(rn);
                    }
                }

                // A test must assert SOMETHING.
                let obj = t.as_mapping();
                let has_then = obj.map(|o| o.contains_key(Value::String("then".into()))).unwrap_or(false);
                let has_thrown = obj.map(|o| o.contains_key(Value::String("thrown".into()))).unwrap_or(false);
                if !has_then && !has_thrown {
                    issues.push(err(
                        "test-no-assertion",
                        where_.clone(),
                        "test asserts nothing — declare `then` (events, [] for a no-op) and/or `thrown` (errors).".into(),
                    ));
                }

                if let Some(thens) = t.get("then").and_then(|x| x.as_sequence()) {
                    for (i, th) in thens.iter().enumerate() {
                        let ev = match fixture_event(model, th.get("$ref").and_then(|x| x.as_str())) {
                            Some(e) => e,
                            None => continue,
                        };
                        used_events.insert(ev.clone());
                        if let Some(idx) = entry_idx {
                            if !inbox_entries[idx].emits.contains(&ev) {
                                issues.push(err(
                                    "test-then-not-emitted",
                                    format!("{}.then[{}]", where_, i),
                                    format!("expected event '{}' is not emitted by '{}' for '{}'.", ev, inbox_entries[idx].actor, msg),
                                ));
                            }
                        }
                    }
                }

                if let Some(throwns) = t.get("thrown").and_then(|x| x.as_sequence()) {
                    for (i, th) in throwns.iter().enumerate() {
                        let er = match th.get("$ref").and_then(|x| x.as_str()).and_then(ref_name) {
                            Some(e) => e,
                            None => continue,
                        };
                        used_errors.insert(er.clone());
                        if let Some(idx) = entry_idx {
                            if !inbox_entries[idx].throws.contains(&er) {
                                issues.push(err(
                                    "test-thrown-not-declared",
                                    format!("{}.thrown[{}]", where_, i),
                                    format!("error '{}' is not declared in '{}' throws for '{}' (actors.yaml).", er, inbox_entries[idx].actor, msg),
                                ));
                            }
                        }
                    }
                }
            }
        }

        // 7c. COVERAGE (blocking).
        for e in &inbox_entries {
            if !used_messages.contains(&format!("{}::{}", e.actor, e.message)) {
                issues.push(err(
                    "test-uncovered-message",
                    format!("{}/{}", e.file, e.actor),
                    format!("no test exercises {} '{}' on '{}'.", if e.is_command { "command" } else { "event" }, e.message, e.actor),
                ));
            }
        }
        for ev in &t_emitted_events {
            if !used_events.contains(ev) {
                issues.push(err(
                    "test-uncovered-event",
                    format!("events.yaml/{}", ev),
                    format!("emitted event '{}' is asserted by no test (in a `then`/`given`).", ev),
                ));
            }
        }
        for er in &t_throwable_errors {
            if !used_errors.contains(er) {
                issues.push(err(
                    "test-uncovered-error",
                    format!("errors.yaml/{}", er),
                    format!("throwable error '{}' is asserted by no test (in a `thrown`).", er),
                ));
            }
        }
        for rn in &all_rules {
            if !used_rules.contains(rn) {
                issues.push(err(
                    "rule-uncovered",
                    format!("rules.yaml/{}", rn),
                    format!("business rule '{}' is asserted by no test — add a test with `rules: [{{ $ref: 'rules.yaml#/{}' }}]` or remove the rule (ADR-0032).", rn, rn),
                ));
            }
        }
    }

    // --- 8. Observability contracts (observability.yaml) ----------------------------------------
    {
        let span_kinds: BTreeSet<&str> = ["SERVER", "CLIENT", "INTERNAL", "PRODUCER", "CONSUMER"].into_iter().collect();
        if let Some(obs) = model.defs.get("observability.yaml").and_then(|x| x.as_mapping()) {
            for (fk, c) in obs {
                let feature = match fk.as_str() {
                    Some(s) => s,
                    None => continue,
                };
                let at = format!("observability.yaml/{}", feature);
                cov.obs_contracts += 1;

                let wf = c.get("workflow");
                let has = |k: &str| wf.and_then(|w| w.get(k)).map(|v| !v.is_null()).unwrap_or(false);
                if !has("command") && !has("saga") && !has("aggregate") {
                    issues.push(err(
                        "obs-no-workflow-binding",
                        at.clone(),
                        "workflow must bind a `command` and/or `saga`/`aggregate` ($ref into the model).".into(),
                    ));
                }

                let id_names: BTreeSet<&str> = c
                    .get("run_identity")
                    .and_then(|x| x.as_sequence())
                    .map(|s| s.iter().filter_map(|i| i.get("name").and_then(|n| n.as_str())).collect())
                    .unwrap_or_default();
                for must in ["correlation_id", "trace_id"] {
                    if !id_names.contains(must) {
                        issues.push(err(
                            "obs-missing-id",
                            format!("{}.run_identity", at),
                            format!("run_identity must declare the mandatory id '{}'.", must),
                        ));
                    }
                }

                let spans = c.get("spans").and_then(|x| x.as_sequence()).cloned().unwrap_or_default();
                if spans.is_empty() {
                    issues.push(err("obs-no-spans", at.clone(), "contract declares no spans.".into()));
                }
                let mut span_names: BTreeSet<String> = BTreeSet::new();
                for (i, s) in spans.iter().enumerate() {
                    match s.get("name").and_then(|x| x.as_str()) {
                        None => issues.push(err("obs-span-no-name", format!("{}.spans[{}]", at, i), "span has no `name`.".into())),
                        Some(n) => {
                            span_names.insert(n.to_string());
                        }
                    }
                    if let Some(kind) = s.get("kind").and_then(|x| x.as_str()) {
                        if !span_kinds.contains(kind) {
                            issues.push(err(
                                "obs-span-kind",
                                format!("{}.spans[{}]", at, i),
                                format!("span kind '{}' is not one of SERVER|CLIENT|INTERNAL|PRODUCER|CONSUMER.", kind),
                            ));
                        }
                    }
                }

                let req_spans = c
                    .get("status_rules")
                    .and_then(|sr| sr.get("success"))
                    .and_then(|s| s.get("required_spans"))
                    .and_then(|x| x.as_sequence())
                    .map(|s| s.iter().filter_map(|v| v.as_str().map(|x| x.to_string())).collect::<Vec<_>>())
                    .unwrap_or_default();
                for rs in &req_spans {
                    if !span_names.contains(rs) {
                        issues.push(err(
                            "obs-required-span-undeclared",
                            format!("{}.status_rules.success", at),
                            format!("required_span '{}' is not a declared span.", rs),
                        ));
                    }
                }
            }
        }
    }

    // --- 9. C4 consistency (architecture/c4-l2.yaml) --------------------------------------------
    {
        let l2 = model.defs.get("architecture/c4-l2.yaml");
        let bcs = l2.and_then(|v| v.get("boundedContexts")).and_then(|x| x.as_mapping());
        let mut mapped: BTreeSet<String> = BTreeSet::new();
        if let Some(bcs) = bcs {
            for (_, bc) in bcs {
                for key in ["aggregates", "processManagers"] {
                    if let Some(seq) = bc.get(key).and_then(|x| x.as_sequence()) {
                        for r in seq {
                            if let Some(n) = r.get("$ref").and_then(|x| x.as_str()).and_then(ref_name) {
                                mapped.insert(n);
                            }
                        }
                    }
                }
            }
            for a in &actors {
                if !mapped.contains(&a.name) {
                    issues.push(warn(
                        "c4-actor-unmapped",
                        "architecture/c4-l2.yaml".into(),
                        format!("actor '{}' belongs to no bounded context (C4 L2 drift).", a.name),
                    ));
                }
            }
            let mut role_owner: HashMap<String, String> = HashMap::new();
            for (ck, bc) in bcs {
                let cid = ck.as_str().unwrap_or("");
                if let Some(roles) = bc.get("roles").and_then(|x| x.as_sequence()) {
                    for role in roles {
                        let r = role.as_str().map(|s| s.to_string()).unwrap_or_else(|| format!("{:?}", role));
                        if !user_type_set.is_empty() && !user_type_set.contains(&r) {
                            issues.push(err(
                                "c4-context-role-unknown",
                                format!("architecture/c4-l2.yaml/{}", cid),
                                format!("bounded-context role '{}' is not a scalars.yaml#/UserType value.", r),
                            ));
                        }
                        match role_owner.get(&r) {
                            Some(prev) if prev != cid => issues.push(err(
                                "c4-context-role-overlap",
                                format!("architecture/c4-l2.yaml/{}", cid),
                                format!("UserType '{}' is claimed by both '{}' and '{}' — each role maps to at most one context.", r, prev, cid),
                            )),
                            _ => {
                                role_owner.insert(r, cid.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // --- 10. Translations (translations.yaml) ---------------------------------------------------
    if let Some(tr) = model.defs.get("translations.yaml").and_then(|x| x.as_mapping()) {
        for (kk, t) in tr {
            let key = match kk.as_str() {
                Some(s) => s,
                None => continue,
            };
            let at = format!("translations.yaml/{}", key);
            cov.translations += 1;
            let messages = t.get("messages");
            for loc in ["en", "fr"] {
                let ok = messages
                    .and_then(|m| m.get(loc))
                    .and_then(|v| v.as_str())
                    .map(|s| !s.is_empty())
                    .unwrap_or(false);
                if !ok {
                    issues.push(err(
                        "translation-missing-locale",
                        at.clone(),
                        format!("translation '{}' has no '{}' message (both en and fr are required).", key, loc),
                    ));
                }
            }
            let params: BTreeSet<String> = t.get("params").and_then(|p| p.as_mapping()).map(map_of_keys).unwrap_or_default();
            for loc in ["en", "fr"] {
                for ph in placeholders(messages.and_then(|m| m.get(loc))) {
                    if !params.contains(&ph) {
                        issues.push(err(
                            "translation-param-mismatch",
                            at.clone(),
                            format!("'{}' message uses {{{}}} but it is not declared in `params`.", loc, ph),
                        ));
                    }
                }
            }
            let mut used: BTreeSet<String> = placeholders(messages.and_then(|m| m.get("en")));
            used.extend(placeholders(messages.and_then(|m| m.get("fr"))));
            for p in &params {
                if !used.contains(p) {
                    issues.push(err(
                        "translation-param-mismatch",
                        at.clone(),
                        format!("declared param '{}' is used by no message.", p),
                    ));
                }
            }
        }
    }

    // --- 11. SDUI screens (screens/*.yaml, one file per app/audience): each app's spec is bound to the
    // API (ADR-0033/0037). Generic over all screens files — no hard-coded `customer_screens`. Each screen
    // declares `roles` (⊆ UserType) and the file declares `app_types` (⊆ web|ios|android|windows).
    {
        let query_names: BTreeSet<&str> = api.queries.iter().map(|q| q.name.as_str()).collect();
        let mutation_names: BTreeSet<&str> = api.mutations.iter().map(|m| m.name.as_str()).collect();
        let op_name = |r: &str| r.rsplit('/').next().unwrap_or("").to_string();
        const APP_TYPES: [&str; 4] = ["web", "ios", "android", "windows"];
        let screens_files: Vec<String> =
            model.defs.keys().filter(|k| k.starts_with("screens/")).cloned().collect();

        for sfkey in &screens_files {
            let cs = model.defs.get(sfkey);
            let resolvers = cs.and_then(|v| v.get("resolvers")).and_then(|x| x.as_mapping());
            let actions = cs.and_then(|v| v.get("actions")).and_then(|x| x.as_mapping());
            let mut resolver_names: BTreeSet<String> = BTreeSet::new();

            // File-level app_types (target platforms) must be known.
            if let Some(ats) = cs.and_then(|v| v.get("app_types")).and_then(|x| x.as_sequence()) {
                for at in ats {
                    if let Some(a) = at.as_str() {
                        if !APP_TYPES.contains(&a) {
                            issues.push(err(
                                "screen-unknown-apptype",
                                format!("{}/app_types", sfkey),
                                format!("app_type '{}' is not one of web|ios|android|windows.", a),
                            ));
                        }
                    }
                }
            }

            if let Some(rmap) = resolvers {
                for (nk, r) in rmap {
                    let name = match nk.as_str() {
                        Some(s) => s,
                        None => continue,
                    };
                    resolver_names.insert(name.to_string());
                    if r.get("gap").map(|v| !v.is_null()).unwrap_or(false) {
                        cov.screen_gaps += 1;
                        continue;
                    }
                    match r.get("query").and_then(|q| q.get("$ref")).and_then(|x| x.as_str()) {
                        None => issues.push(err(
                            "resolver-no-binding",
                            format!("{}/resolvers/{}", sfkey, name),
                            format!("resolver '{}' must declare a `query` ($ref into api.yaml) or a `gap`.", name),
                        )),
                        Some(rf) => {
                            if ref_target_file(rf, sfkey).as_deref() != Some("api.yaml")
                                || !rf.contains("/queries/")
                                || !query_names.contains(op_name(rf).as_str())
                            {
                                issues.push(err(
                                    "resolver-not-a-query",
                                    format!("{}/resolvers/{}", sfkey, name),
                                    format!("resolver '{}' query must $ref an api.yaml query; '{}' is not one.", name, rf),
                                ));
                            } else {
                                cov.screen_bindings += 1;
                            }
                        }
                    }
                }
            }
            if let Some(amap) = actions {
                for (nk, a) in amap {
                    let name = match nk.as_str() {
                        Some(s) => s,
                        None => continue,
                    };
                    let rf = match a.get("mutation").and_then(|m| m.get("$ref")).and_then(|x| x.as_str()) {
                        Some(r) => r,
                        None => continue,
                    };
                    if ref_target_file(rf, sfkey).as_deref() != Some("api.yaml")
                        || !rf.contains("/mutations/")
                        || !mutation_names.contains(op_name(rf).as_str())
                    {
                        issues.push(err(
                            "action-not-a-mutation",
                            format!("{}/actions/{}", sfkey, name),
                            format!("action '{}' mutation must $ref an api.yaml mutation; '{}' is not one.", name, rf),
                        ));
                    } else {
                        cov.screen_bindings += 1;
                    }
                }
            }
            if let Some(screens) = cs.and_then(|v| v.get("screens")).and_then(|x| x.as_sequence()) {
                for s in screens {
                    cov.screens += 1;
                    let sid = s.get("id").and_then(|x| x.as_str()).unwrap_or("?").to_string();
                    cov.screen_gaps += s.get("gaps").and_then(|x| x.as_sequence()).map(|g| g.len()).unwrap_or(0);
                    // Per-screen roles must be scalars.yaml#/UserType values.
                    if let Some(rs) = s.get("roles").and_then(|x| x.as_sequence()) {
                        for r in rs {
                            if let Some(role) = r.as_str() {
                                if !user_type_set.contains(role) {
                                    issues.push(err(
                                        "screen-unknown-role",
                                        format!("{}/screens/{}", sfkey, sid),
                                        format!("role '{}' is not a scalars.yaml#/UserType value.", role),
                                    ));
                                }
                            }
                        }
                    }
                    if let Some(drs) = s.get("data_requirements").and_then(|x| x.as_sequence()) {
                        for dr in drs {
                            let name = dr.as_str().map(|s| s.to_string()).unwrap_or_else(|| format!("{:?}", dr));
                            if !resolver_names.contains(&name) {
                                issues.push(err(
                                    "screen-unknown-resolver",
                                    format!("{}/screens/{}", sfkey, sid),
                                    format!("data_requirement '{}' is not a declared resolver.", name),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    // --- 12. Rust codegen naming: a generated type name must not collide with a Rust reserved/prelude
    // type (the codegen emits it verbatim as a Rust `struct`/`enum`). Resolve at the root — rename it in
    // the spec — rather than working around it in the generator (ADR-0035 naming policy).
    {
        let reserved: BTreeSet<&str> = [
            "Option", "Result", "Box", "Vec", "String", "Some", "None", "Ok", "Err", "Copy", "Clone",
            "Debug", "Default", "Drop", "Eq", "Ord", "PartialEq", "PartialOrd", "Hash", "Iterator", "Send",
            "Sync", "Sized", "From", "Into", "TryFrom", "TryInto", "ToString", "AsRef", "AsMut", "Fn",
            "FnMut", "FnOnce", "Self", "Cow", "Rc", "Arc", "Cell", "RefCell", "Duration", "Ordering",
        ]
        .into_iter()
        .collect();
        for file in ["scalars.yaml", "entities.yaml"] {
            for name in map_keys(model.defs.get(file)) {
                if reserved.contains(name.as_str()) {
                    issues.push(err(
                        "rust-reserved-typename",
                        format!("{}/{}", file, name),
                        format!("type name '{}' collides with a Rust prelude/reserved type — rename it in the spec (generated Rust cannot use it as a struct/enum).", name),
                    ));
                }
            }
        }
    }

    Report { issues, coverage: cov, handled_commands: handled_commands.len() }
}

/// checkData: resolve a `type.$ref` then check the data against its schema (validate.ts §7 checkData).
fn check_data_shape(model: &Model, issues: &mut Vec<Issue>, type_ref: &str, data: Option<&Value>, where_: &str) {
    check_shape(model, issues, resolve_ref(model, type_ref, "tests.yaml"), data, where_);
}

fn map_of_keys(m: &serde_yaml::Mapping) -> BTreeSet<String> {
    m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect()
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
}

/// Emit the single i18n bundle from translations.yaml (ADR-0033) — the first ported emitter. Must be
/// BYTE-IDENTICAL to the TypeScript `emitTranslationsJson` output (keys sorted; `{ "<key>": { en, fr } }`;
/// 2-space pretty JSON + trailing newline) so the CI generate+diff gate stays clean during the migration.
fn emit_translations_json(model: &Model) -> String {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    if let Some(Value::Mapping(m)) = model.defs.get("translations.yaml") {
        for (k, v) in m {
            let key = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            // skip file-level meta (version/description) — only real translation entries have `messages`.
            let messages = match v.get("messages").and_then(|x| x.as_mapping()) {
                Some(mm) => mm,
                None => continue,
            };
            let mut locales = BTreeMap::new();
            for (lk, lv) in messages {
                if let (Some(l), Some(t)) = (lk.as_str(), lv.as_str()) {
                    locales.insert(l.to_string(), t.to_string());
                }
            }
            out.insert(key.to_string(), locales);
        }
    }
    let mut s = serde_json::to_string_pretty(&out).expect("serialize translations");
    s.push('\n');
    s
}

// ─── views.generated.sql (port of emit/database.ts `emitViewsSql`) ──────────────────────────────
// Byte-identical CREATE TABLE + index DDL for every View_* (aggregate-fed or `source: reference`).

/// One arm of a `status-from-event-type` derivation: for a given event type, the column's value is
/// either a literal enum value (`Lit`) or extracted from that event's payload (`Payload(prop)`).
#[derive(Clone)]
enum DeriveVal {
    Lit(String),
    Payload(String),
}
struct SqlColumn {
    name: String,
    ty: String,
    pk: bool,
    unique: bool,
    index: bool,
    nullable: bool,
    fk: Option<String>, // "View_Name.column" — used by the GraphQL FK-navigation emitter
    note: Option<String>,
    from: Vec<String>,   // event/property $ref strings that populate the column
    type_derived: bool,  // type was derived from `from` (not declared explicitly)
    /// `status-from-event-type` derivation map (event_type → value), in declared order. Empty = none.
    derive: Vec<(String, DeriveVal)>,
    /// Conditional occurrence-time: `max(occurred_at)` over events matching any (event_type [+ payload
    /// equalities]) clause — e.g. delivered_at = when DeliveryCompleted OR DeliveryStatusUpdated=DELIVERED.
    occurred_when: Vec<(String, Vec<(String, String)>)>,
}
struct SqlView {
    name: String,
    aggregate: String,
    slice: String,
    internal: bool,
    reference: bool,
    filters: Vec<String>,
    rules: Vec<String>,
    note: Option<String>,
    fedby: Vec<String>,
    columns: Vec<SqlColumn>,
    indexes: Vec<Vec<String>>,
    /// true → a materialized read-model TABLE (projection_tables.yaml, fed by a projector); false → a
    /// generated fold VIEW (projection_views.yaml).
    is_table: bool,
    /// (table) how the table is maintained — always "app": an application-layer (Rust) projector,
    /// deferred until crates/ exists. No SQL triggers (ADR-0040).
    projector: Option<String>,
    /// Event type whose presence in the stream drops the row (soft-delete tombstone), if any.
    tombstone: Option<String>,
    /// Hand-written SQL override (escape hatch): when set, used verbatim instead of the generated fold.
    definition: Option<String>,
}

/// A foreign key `"View_Name.column"` — either a literal string or a `{ $ref: '#/View_X/columns/col' }`.
fn parse_fk(raw: Option<&Value>) -> Option<String> {
    match raw {
        Some(Value::String(s)) => Some(s.clone()),
        Some(v) => {
            if let Some(r) = v.get("$ref").and_then(|x| x.as_str()) {
                let segs: Vec<&str> =
                    r.splitn(2, "#/").nth(1).unwrap_or("").split('/').filter(|s| !s.is_empty()).collect();
                if segs.len() >= 2 {
                    return Some(format!("{}.{}", segs[0], segs[segs.len() - 1]));
                }
            }
            None
        }
        None => None,
    }
}

/// Explicit column `type`: a `$ref` into scalars.yaml (→ the scalar name) or an inline SQL primitive string.
fn column_type_explicit(raw: &Value) -> String {
    if let Some(r) = raw.get("$ref").and_then(|x| x.as_str()) {
        return r.splitn(2, "#/").nth(1).unwrap_or("").to_string();
    }
    match raw {
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

/// Map an events.yaml property schema node to the column type it implies (mirrors schemaNodeToColumnType).
fn schema_node_to_column_type(node: &Value) -> String {
    if let Some(r) = node.get("$ref").and_then(|x| x.as_str()) {
        let mut it = r.splitn(2, "#/");
        let file = it.next().unwrap_or("");
        let name = it.next().unwrap_or("");
        return if file == "scalars.yaml" {
            name.to_string()
        } else {
            "jsonb".to_string()
        };
    }
    match node.get("type").and_then(|x| x.as_str()) {
        Some("array") => "jsonb".into(),
        Some("integer") => "integer".into(),
        Some("number") => "numeric".into(),
        Some("boolean") => "boolean".into(),
        Some("string") => {
            if node.get("format").and_then(|x| x.as_str()) == Some("date-time") {
                "timestamptz".into()
            } else {
                "text".into()
            }
        }
        _ => "text".into(),
    }
}

/// Derive a column type from the first `from` entry pointing at a typed event PROPERTY (mirrors deriveType).
fn derive_type(from: &[String], events: &Value) -> String {
    for r in from {
        let ptr = r.splitn(2, "#/").nth(1).unwrap_or("");
        let segs: Vec<&str> = ptr.split('/').filter(|s| !s.is_empty()).collect();
        if segs.len() < 3 || segs[1] != "properties" {
            continue;
        }
        if let Some(node) = events
            .get(segs[0])
            .and_then(|e| e.get("properties"))
            .and_then(|p| p.get(segs[2]))
        {
            return schema_node_to_column_type(node);
        }
    }
    String::new()
}

/// Map a column type (SQL primitive or scalars.yaml type) to a Postgres type (mirrors sqlType).
fn sql_type(ty: &str, model: &Model) -> String {
    let prim = match ty {
        "uuid" => Some("UUID"),
        "text" => Some("TEXT"),
        "integer" => Some("INTEGER"),
        "bigint" => Some("BIGINT"),
        "boolean" => Some("BOOLEAN"),
        "timestamptz" => Some("TIMESTAMPTZ"),
        "jsonb" => Some("JSONB"),
        "numeric" => Some("NUMERIC"),
        _ => None,
    };
    if let Some(p) = prim {
        return p.to_string();
    }
    if let Some(scalar) = model.defs.get("scalars.yaml").and_then(|s| s.get(ty)) {
        if scalar.get("enum").map(|e| e.is_sequence()).unwrap_or(false) {
            // Enums are stored as their compact INTEGER ordinal (the sort_order in the generated
            // ref_<enum> lookup) — by principle, since a ref table always exists for the enum (ADR-0037).
            return "INTEGER".into();
        }
        if scalar.get("format").and_then(|x| x.as_str()) == Some("uuid") {
            return "UUID".into();
        }
        if scalar.get("type").and_then(|x| x.as_str()) == Some("integer") {
            return if ty == "MoneyCents" { "BIGINT".into() } else { "INTEGER".into() };
        }
    }
    "TEXT".into()
}

fn parse_col(name: String, col: &Value, events: &Value) -> SqlColumn {
    let from: Vec<String> = col
        .get("from")
        .and_then(|f| f.as_sequence())
        .map(|s| s.iter().filter_map(|it| it.get("$ref").and_then(|r| r.as_str()).map(|x| x.to_string())).collect())
        .unwrap_or_default();
    let has_explicit = matches!(col.get("type"), Some(v) if !v.is_null());
    let ty = if has_explicit {
        column_type_explicit(col.get("type").unwrap())
    } else {
        derive_type(&from, events)
    };
    let type_derived = !has_explicit && !ty.is_empty();
    let flag = |k: &str| col.get(k).and_then(|x| x.as_bool()) == Some(true);
    // `derive:` — an event_type → value map for status-from-event-type columns. A string value is a
    // literal enum value; `{ from: prop }` extracts the value from that event's payload.
    let mut derive = Vec::new();
    if let Some(dm) = col.get("derive").and_then(|d| d.as_mapping()) {
        for (dk, dv) in dm {
            if let Some(evt) = dk.as_str() {
                let val = match dv {
                    Value::String(s) => DeriveVal::Lit(s.clone()),
                    v => match v.get("from").and_then(|x| x.as_str()) {
                        Some(p) => DeriveVal::Payload(p.to_string()),
                        None => continue,
                    },
                };
                derive.push((evt.to_string(), val));
            }
        }
    }
    // `occurredWhen:` — a list of { event, whenPayload?: { key: value } } clauses; the column is the
    // max(occurred_at) over events matching any clause (conditional occurrence time).
    let mut occurred_when = Vec::new();
    if let Some(seq) = col.get("occurredWhen").and_then(|d| d.as_sequence()) {
        for clause in seq {
            if let Some(evt) = clause.get("event").and_then(|x| x.as_str()) {
                let mut conds = Vec::new();
                if let Some(wp) = clause.get("whenPayload").and_then(|x| x.as_mapping()) {
                    for (pk, pv) in wp {
                        if let (Some(k), Some(v)) = (pk.as_str(), pv.as_str()) {
                            conds.push((k.to_string(), v.to_string()));
                        }
                    }
                }
                occurred_when.push((evt.to_string(), conds));
            }
        }
    }
    SqlColumn {
        name,
        ty,
        pk: flag("pk"),
        unique: flag("unique"),
        index: flag("index"),
        nullable: flag("nullable"),
        fk: parse_fk(col.get("fk")),
        note: col.get("note").and_then(|x| x.as_str()).map(|s| s.to_string()),
        from,
        type_derived,
        derive,
        occurred_when,
    }
}

fn parse_views(model: &Model) -> Vec<SqlView> {
    let mut out = Vec::new();
    let events = model.defs.get("events.yaml").cloned().unwrap_or(Value::Null);
    // Read models live in two files: projection_views.yaml (generated fold VIEWs) and
    // tables/projection_tables.yaml (materialized TABLEs fed by a projector). Same metadata shape.
    for (file, is_table) in [
        ("database/projection_views.yaml", false),
        ("database/tables/projection_tables.yaml", true),
    ] {
        let m = match model.defs.get(file) {
            Some(Value::Mapping(m)) => m,
            _ => continue,
        };
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            let is_ref = node.get("source").and_then(|x| x.as_str()) == Some("reference");
            let has_agg = node.get("aggregate").and_then(|x| x.as_str()).is_some();
            if !has_agg && !is_ref {
                continue; // skip file-level meta (version/description) and non-views
            }
            let mut columns = Vec::new();
            if let Some(cm) = node.get("columns").and_then(|c| c.as_mapping()) {
                for (ck, cv) in cm {
                    if let Some(cn) = ck.as_str() {
                        columns.push(parse_col(cn.to_string(), cv, &events));
                    }
                }
            } else if let Some(cs) = node.get("columns").and_then(|c| c.as_sequence()) {
                for cv in cs {
                    let cn = cv.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    columns.push(parse_col(cn, cv, &events));
                }
            }
            let mut indexes = Vec::new();
            if let Some(seq) = node.get("indexes").and_then(|x| x.as_sequence()) {
                for ix in seq {
                    if let Some(cols) = ix.as_sequence() {
                        indexes.push(
                            cols.iter().filter_map(|c| c.as_str().map(|s| s.to_string())).collect(),
                        );
                    }
                }
            }
            // Technical audit timestamps are IMPLICIT on every read model — not declared per table
            // (ADR-0040). created_at = the creation event's occurred_at; updated_at = the latest applied
            // event's occurred_at. Handled by name in generate_fold_sql (views) / the dispatch (tables).
            if node.get("aggregate").and_then(|x| x.as_str()).is_some() {
                for tech in ["created_at", "updated_at"] {
                    columns.push(SqlColumn {
                        name: tech.to_string(),
                        ty: "timestamptz".to_string(),
                        pk: false,
                        unique: false,
                        index: false,
                        nullable: false,
                        fk: None,
                        note: Some("technical — stamped from event.occurred_at (implicit on every read model)".to_string()),
                        from: Vec::new(),
                        type_derived: false,
                        derive: Vec::new(),
                        occurred_when: Vec::new(),
                    });
                }
            }
            let aggregate = node.get("aggregate").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let tombstone = node
                .get("tombstone")
                .and_then(|t| t.get("$ref").and_then(|r| r.as_str()))
                .and_then(ref_name);
            out.push(SqlView {
                name: name.to_string(),
                aggregate,
                slice: node.get("slice").and_then(|x| x.as_str()).unwrap_or("V0").to_string(),
                internal: node.get("internal").and_then(|x| x.as_bool()) == Some(true),
                reference: is_ref,
                filters: string_list(node.get("filters")),
                rules: string_list(node.get("rules")),
                note: node.get("note").and_then(|x| x.as_str()).map(|s| s.to_string()),
                fedby: ref_names(node.get("fedBy")),
                columns,
                indexes,
                is_table,
                projector: node.get("projector").and_then(|x| x.as_str()).map(|s| s.to_string()),
                tombstone,
                definition: node.get("definition").and_then(|x| x.as_str()).map(|s| s.trim_end().to_string()),
            });
        }
    }
    out
}

/// Split an event/property `$ref` into (event_type, Option<property>). A whole-event ref has no property.
fn event_and_prop(r: &str) -> (String, Option<String>) {
    let ptr = r.splitn(2, "#/").nth(1).unwrap_or("");
    let segs: Vec<&str> = ptr.split('/').filter(|s| !s.is_empty()).collect();
    let evt = segs.first().copied().unwrap_or("").to_string();
    let prop = if segs.len() >= 3 && segs[1] == "properties" { Some(segs[2].to_string()) } else { None };
    (evt, prop)
}

/// Postgres cast suffix for a resolved SQL type (JSONB reads via `->` and needs no cast → "").
fn pg_cast(pgty: &str) -> &'static str {
    match pgty {
        "UUID" => "::uuid",
        "INTEGER" => "::int",
        "BIGINT" => "::bigint",
        "NUMERIC" => "::numeric",
        "BOOLEAN" => "::boolean",
        "TIMESTAMPTZ" => "::timestamptz",
        _ => "",
    }
}

/// A payload extraction expression for `<alias>.payload`'s `prop`, typed to `pgty`.
fn payload_extract(alias: &str, prop: &str, pgty: &str) -> String {
    if pgty == "JSONB" {
        format!("{}.payload->'{}'", alias, prop)
    } else {
        let c = pg_cast(pgty);
        if c.is_empty() {
            format!("{}.payload->>'{}'", alias, prop)
        } else {
            format!("({}.payload->>'{}'){}", alias, prop, c)
        }
    }
}

/// The values of a scalars.yaml enum, in declared (ordinal) order — `Some` only for an enum scalar.
fn enum_values(model: &Model, ty: &str) -> Option<Vec<String>> {
    model
        .defs
        .get("scalars.yaml")?
        .get(ty)?
        .get("enum")?
        .as_sequence()
        .map(|s| s.iter().filter_map(|v| v.as_str().map(|x| x.to_string())).collect())
}

/// SQL mapping a text-enum expression to its INTEGER ordinal (`ref_<enum>.sort_order`) via a CASE over the
/// enum's values — the event payload stores the enum's TEXT value, but read models store the ordinal.
fn enum_ordinal_case(expr: &str, values: &[String]) -> String {
    let arms: Vec<String> =
        values.iter().enumerate().map(|(i, v)| format!("WHEN '{}' THEN {}", v, i)).collect();
    format!("(CASE {} {} END)", expr, arms.join(" "))
}

/// Generate a `SELECT … FROM domain_events` state-fold body for a foldable view (ADR-0035 #2), sourcing
/// each column from its declared `from` lineage + derivation mode. Correct-by-construction: set-once
/// fields fall out of the per-column "latest carrying event" rule, so there is no latest-wins hazard.
fn generate_fold_sql(v: &SqlView, model: &Model) -> Result<String, String> {
    // The creation event = the event carrying the PK column; it defines row existence (one row per stream).
    let pk = v.columns.iter().find(|c| c.pk).ok_or_else(|| "no PK column".to_string())?;
    let creation = pk
        .from
        .iter()
        .filter_map(|r| { let (e, p) = event_and_prop(r); p.map(|_| e) })
        .next()
        .ok_or_else(|| format!("PK column '{}' has no property `from` to anchor the creation event", pk.name))?;

    let mut selects: Vec<String> = Vec::new();
    for c in &v.columns {
        let pgty = sql_type(&c.ty, model);
        // Enum columns are stored as their INTEGER ordinal (by principle) — the payload holds the TEXT
        // value, so enum expressions are wrapped in a value→ordinal CASE.
        let enum_vals = enum_values(model, &c.ty);
        let expr = if c.name == "created_at" {
            // implicit technical column: the creation event's occurrence time.
            "c.occurred_at".to_string()
        } else if c.name == "updated_at" {
            // implicit technical column: the latest applied event's occurrence time.
            let types: Vec<String> = v.fedby.iter().map(|e| format!("'{}'", e)).collect();
            format!(
                "(SELECT max(e.occurred_at) FROM domain_events e\n     WHERE e.stream_name = c.stream_name AND e.event_type IN ({}))",
                types.join(", ")
            )
        } else if !c.occurred_when.is_empty() {
            // conditional occurrence: max(occurred_at) over events matching any (type [+ payload =]) clause.
            let clauses: Vec<String> = c
                .occurred_when
                .iter()
                .map(|(evt, conds)| {
                    let mut parts = vec![format!("e.event_type = '{}'", evt)];
                    for (k, val) in conds {
                        parts.push(format!("e.payload->>'{}' = '{}'", k, val));
                    }
                    if parts.len() == 1 { parts.remove(0) } else { format!("({})", parts.join(" AND ")) }
                })
                .collect();
            format!(
                "(SELECT max(e.occurred_at) FROM domain_events e\n     WHERE e.stream_name = c.stream_name AND ({}))",
                clauses.join(" OR ")
            )
        } else if !c.derive.is_empty() {
            // status-from-event-type: CASE over the latest matching lifecycle event.
            let arms: Vec<String> = c
                .derive
                .iter()
                .map(|(evt, val)| {
                    let then = match val {
                        DeriveVal::Lit(s) => match &enum_vals {
                            Some(vals) => vals
                                .iter()
                                .position(|v| v == s)
                                .unwrap_or_else(|| panic!("derive value '{}' not in enum {}", s, c.ty))
                                .to_string(),
                            None => format!("'{}'", s),
                        },
                        DeriveVal::Payload(p) => match &enum_vals {
                            Some(vals) => enum_ordinal_case(&format!("e.payload->>'{}'", p), vals),
                            None => format!("e.payload->>'{}'", p),
                        },
                    };
                    format!("WHEN '{}' THEN {}", evt, then)
                })
                .collect();
            let types: Vec<String> = c.derive.iter().map(|(e, _)| format!("'{}'", e)).collect();
            format!(
                "(SELECT CASE e.event_type {} END FROM domain_events e\n     WHERE e.stream_name = c.stream_name AND e.event_type IN ({})\n     ORDER BY e.position DESC LIMIT 1)",
                arms.join(" "),
                types.join(", ")
            )
        } else {
            let carrying: Vec<(String, String)> = c
                .from
                .iter()
                .filter_map(|r| { let (e, p) = event_and_prop(r); p.map(|p| (e, p)) })
                .collect();
            let whole: Vec<String> =
                c.from.iter().filter_map(|r| { let (e, p) = event_and_prop(r); if p.is_none() { Some(e) } else { None } }).collect();
            if c.ty == "timestamptz" && carrying.is_empty() && !whole.is_empty() {
                // occurrence time: max(occurred_at) over the contributing event types.
                if whole.len() == 1 && whole[0] == creation {
                    "c.occurred_at".to_string()
                } else {
                    let types: Vec<String> = whole.iter().map(|e| format!("'{}'", e)).collect();
                    format!(
                        "(SELECT max(e.occurred_at) FROM domain_events e\n     WHERE e.stream_name = c.stream_name AND e.event_type IN ({}))",
                        types.join(", ")
                    )
                }
            } else if let Some((_, prop)) = carrying.first() {
                // scalar "latest carrying event": the newest event whose payload holds this property.
                // An enum column stores the ordinal (value→ordinal CASE); others extract+cast by type.
                let val_expr = |alias: &str| match &enum_vals {
                    Some(vals) => enum_ordinal_case(&format!("{}.payload->>'{}'", alias, prop), vals),
                    None => payload_extract(alias, prop, &pgty),
                };
                let only_creation = carrying.iter().all(|(e, _)| e == &creation);
                if only_creation {
                    val_expr("c")
                } else {
                    // Scope by the declared carrying event types AND the property key — so a JSON key shared
                    // by an unrelated event type can never win over the intended source.
                    let mut types: Vec<String> = Vec::new();
                    for (e, _) in &carrying {
                        let q = format!("'{}'", e);
                        if !types.contains(&q) {
                            types.push(q);
                        }
                    }
                    format!(
                        "(SELECT {} FROM domain_events e\n     WHERE e.stream_name = c.stream_name AND e.event_type IN ({}) AND e.payload ? '{}'\n     ORDER BY e.position DESC LIMIT 1)",
                        val_expr("e"),
                        types.join(", "),
                        prop
                    )
                }
            } else {
                return Err(format!(
                    "column '{}' is not foldable (no property `from`, not a timestamp occurrence, no `derive`) — move the view to projection_tables.yaml (materialized) or add a mode",
                    c.name
                ));
            }
        };
        selects.push(format!("  {} AS {}", expr, c.name));
    }

    let mut sql = format!("SELECT\n{}\nFROM domain_events c\nWHERE c.event_type = '{}'", selects.join(",\n"), creation);
    if let Some(tomb) = &v.tombstone {
        sql.push_str(&format!(
            "\n  AND NOT EXISTS (SELECT 1 FROM domain_events d\n                  WHERE d.stream_name = c.stream_name AND d.event_type = '{}')",
            tomb
        ));
    }
    Ok(sql)
}

fn emit_views_sql(model: &Model) -> String {
    let mut blocks = Vec::new();
    for v in parse_views(model) {
        // Only fold VIEWs (projection_views.yaml) → CREATE OR REPLACE VIEW over domain_events, from a
        // hand-written `definition` override if present, else generated from the column `from` lineage.
        // Materialized read-model TABLEs (projection_tables.yaml) are emitted into schema.generated.sql.
        if v.is_table {
            continue;
        }
        let body = match &v.definition {
            Some(def) => def.clone(),
            None => generate_fold_sql(&v, model)
                .unwrap_or_else(|e| panic!("projection_views.yaml#/{}: cannot generate fold: {}", v.name, e)),
        };
        blocks.push(format!("CREATE OR REPLACE VIEW {} AS\n{};", v.name, body));
    }
    format!(
        "-- GENERATED by tools/codegen from specs/database/projection_views.yaml — do not edit by hand.\n-- Read models realized as SQL VIEWS: a `CREATE OR REPLACE VIEW` state-fold over domain_events, generated\n-- from each column's `from` lineage (ADR-0039). Read models whose columns are COMPUTED are materialized\n-- tables in tables/projection_tables.yaml (emitted into schema.generated.sql) instead.\n\n{}\n",
        blocks.join("\n\n")
    )
}

/// CREATE TABLE DDL (+ indexes) for a materialized read-model table, column types resolved from the
/// per-column `from` lineage (unlike referential tables, whose columns carry an explicit `type`).
fn view_table_ddl(v: &SqlView, model: &Model) -> String {
    let mut cols = Vec::new();
    for c in &v.columns {
        let mut bits = vec![format!("  {}", c.name), sql_type(&c.ty, model)];
        if c.pk {
            bits.push("PRIMARY KEY".into());
        } else if c.unique {
            bits.push(if c.nullable { "UNIQUE".into() } else { "NOT NULL UNIQUE".into() });
        } else if !c.nullable {
            bits.push("NOT NULL".into());
        }
        cols.push(bits.join(" "));
    }
    let ddl = format!("CREATE TABLE {} (\n{}\n);", v.name, cols.join(",\n"));
    let mut idx: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for c in &v.columns {
        if c.index && !c.pk && seen.insert(c.name.clone()) {
            idx.push(format!("CREATE INDEX ON {} ({});", v.name, c.name));
        }
    }
    for ix in &v.indexes {
        if seen.insert(ix.join(",")) {
            idx.push(format!("CREATE INDEX ON {} ({});", v.name, ix.join(", ")));
        }
    }
    if idx.is_empty() { ddl } else { format!("{}\n{}", ddl, idx.join("\n")) }
}

/// The materialized read-model TABLEs (projection_tables.yaml) as DDL, for inclusion in schema.generated.sql.
fn emit_projection_tables_sql(model: &Model) -> String {
    parse_views(model)
        .iter()
        .filter(|v| v.is_table)
        .map(|v| view_table_ddl(v, model))
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ─── schema.generated.sql (ADR-0037 — store DDL from database/tables.yaml + functions/*.sql + scalars enums) ──

/// snake_case of a PascalCase type name, without a leading underscore (`OrderStatus` → `order_status`).
fn snake_type(s: &str) -> String {
    snake_field(s).trim_start_matches('_').to_string()
}

/// Map a tables.yaml SQL-primitive column type to its Postgres spelling. Infrastructure tables are
/// deliberately decoupled from the domain scalars, so this is a closed map — an unknown type is a spec
/// error, failed loudly rather than defaulted.
fn table_sql_type(ty: &str) -> &'static str {
    match ty {
        "uuid" => "UUID",
        "text" => "TEXT",
        "integer" => "INTEGER",
        "bigint" => "BIGINT",
        "boolean" => "BOOLEAN",
        "timestamptz" => "TIMESTAMPTZ",
        "jsonb" => "JSONB",
        "numeric" => "NUMERIC",
        "interval" => "INTERVAL",
        other => panic!("database/tables.yaml: unknown column type '{}' — extend table_sql_type", other),
    }
}

/// Emit `schema.generated.sql` (ADR-0037): the full store DDL — enum reference tables from
/// scalars.yaml, the real tables from database/tables.yaml, the raw SQL functions from
/// database/functions/*.sql (sorted by filename), then the triggers declared on the tables
/// (after the functions they execute).
fn emit_schema_sql(model: &Model, specs: &std::path::Path) -> String {
    let mut sections: Vec<String> = Vec::new();

    // 1. Enum reference tables — one ref_<snake> lookup table per scalars.yaml enum, in file order.
    if let Some(Value::Mapping(m)) = model.defs.get("scalars.yaml") {
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            let vals = match node.get("enum").and_then(|e| e.as_sequence()) {
                Some(v) => v,
                None => continue,
            };
            let table = format!("ref_{}", snake_type(name));
            let values: Vec<String> = vals
                .iter()
                .enumerate()
                .filter_map(|(i, v)| v.as_str().map(|s| format!("('{}',{})", s, i)))
                .collect();
            sections.push(format!(
                "-- {}\nCREATE TABLE {}(sort_order INT PRIMARY KEY, value TEXT NOT NULL UNIQUE);\nINSERT INTO {} (value, sort_order) VALUES {};",
                name,
                table,
                table,
                values.join(",")
            ));
        }
    }

    // 2. Tables from database/tables/*.yaml, in file order. Triggers are collected and emitted after
    // the functions they execute (step 4). projection_tables.yaml is handled separately (step 2b) — its
    // columns derive their type from event lineage, not an explicit `type`.
    let mut triggers: Vec<String> = Vec::new();
    for (_fkey, fval) in model
        .defs
        .iter()
        .filter(|(k, _)| k.starts_with("database/tables/") && k.as_str() != "database/tables/projection_tables.yaml")
    {
        let m = match fval {
            Value::Mapping(m) => m,
            _ => continue,
        };
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            let cols = match node.get("columns").and_then(|c| c.as_mapping()) {
                Some(c) => c,
                None => continue,
            };
            let mut lines: Vec<String> = Vec::new();
            for (ck, cv) in cols {
                let cname = match ck.as_str() {
                    Some(s) => s,
                    None => continue,
                };
                // `type` is either a SQL-primitive string (→ table_sql_type) or a `$ref` into a
                // scalars.yaml scalar (→ its Postgres type via the shared sql_type mapping).
                let ty_node = cv.get("type").unwrap_or_else(|| {
                    panic!("database/tables.yaml#/{}/columns/{}: missing type", name, cname)
                });
                let sqlty: String = if let Some(s) = ty_node.as_str() {
                    table_sql_type(s).to_string()
                } else if let Some(rf) = ty_node.get("$ref").and_then(|x| x.as_str()) {
                    let scalar = ref_name(rf).unwrap_or_else(|| {
                        panic!("database/tables.yaml#/{}/columns/{}: malformed $ref '{}'", name, cname, rf)
                    });
                    sql_type(&scalar, model) // an enum scalar → INTEGER ordinal (ref_<enum>), by principle
                } else {
                    panic!("database/tables.yaml#/{}/columns/{}: type must be a SQL primitive or a $ref", name, cname)
                };
                let flag = |f: &str| cv.get(f).and_then(|x| x.as_bool()) == Some(true);
                let mut line = format!("  {} {}", cname, sqlty);
                if flag("identity") {
                    line.push_str(" GENERATED ALWAYS AS IDENTITY");
                }
                if flag("pk") {
                    line.push_str(" PRIMARY KEY");
                } else {
                    line.push_str(if flag("nullable") { " NULL" } else { " NOT NULL" });
                    if flag("unique") {
                        line.push_str(" UNIQUE");
                    }
                }
                lines.push(line);
            }
            if let Some(cs) = node.get("constraints").and_then(|c| c.as_sequence()) {
                for c in cs {
                    if let Some(u) = c.get("unique").and_then(|x| x.as_sequence()) {
                        let cols: Vec<&str> = u.iter().filter_map(|v| v.as_str()).collect();
                        lines.push(format!("  UNIQUE ({})", cols.join(", ")));
                    }
                }
            }
            let mut block = format!("CREATE TABLE {} (\n{}\n);", name, lines.join(",\n"));
            // per-column `index: true` (non-pk) → a single-column index (e.g. referential dialing_code).
            for (ck, cv) in cols {
                if let Some(cn) = ck.as_str() {
                    let f = |x: &str| cv.get(x).and_then(|b| b.as_bool()) == Some(true);
                    if f("index") && !f("pk") {
                        block.push_str(&format!("\nCREATE INDEX ON {} ({});", name, cn));
                    }
                }
            }
            if let Some(seq) = node.get("indexes").and_then(|x| x.as_sequence()) {
                for ix in seq {
                    if let Some(cols) = ix.as_sequence() {
                        let cols: Vec<&str> = cols.iter().filter_map(|v| v.as_str()).collect();
                        block.push_str(&format!("\nCREATE INDEX ON {} ({});", name, cols.join(", ")));
                    }
                }
            }
            sections.push(block);
            if let Some(ts) = node.get("triggers").and_then(|t| t.as_sequence()) {
                for t in ts {
                    let get = |f: &str| {
                        t.get(f).and_then(|x| x.as_str()).unwrap_or_else(|| {
                            panic!("database/tables.yaml#/{}/triggers: missing {}", name, f)
                        })
                    };
                    triggers.push(format!(
                        "CREATE TRIGGER {} {} ON {} FOR EACH {} EXECUTE FUNCTION {}();",
                        get("name"),
                        get("timing"),
                        name,
                        get("for_each").to_uppercase(),
                        get("function")
                    ));
                }
            }
        }
    }

    // 2b. Materialized read-model tables (database/tables/projection_tables.yaml) — column types resolved
    // from event lineage. Filled by an application-layer (Rust) projector, not SQL (ADR-0040). Emitted
    // here so the read-model tables sit alongside the store tables.
    let ptables = emit_projection_tables_sql(model);
    if !ptables.trim().is_empty() {
        sections.push(ptables);
    }

    // 3. Functions — raw SQL bodies from database/functions/*.sql, sorted by filename. They reference
    // domain_events/domain_stream, which now exist above.
    let fn_dir = specs.join("database/functions");
    let mut fn_files: Vec<PathBuf> = fs::read_dir(&fn_dir)
        .unwrap_or_else(|e| panic!("read {}: {}", fn_dir.display(), e))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("sql"))
        .collect();
    fn_files.sort_by_key(|p| p.file_name().map(|n| n.to_os_string()));
    for p in &fn_files {
        let body = fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {}", p.display(), e));
        sections.push(body.replace("\r\n", "\n").trim().to_string());
    }

    // 4. Triggers — after the functions they execute.
    sections.extend(triggers);

    format!(
        "-- GENERATED by the Captain.Food codegen from specs/database/ + scalars.yaml — do not edit by hand.\n\n{}\n",
        sections.join("\n\n")
    )
}

// ─── database.md §2 read-model tables (port of emit/database.ts `emitViewsMarkdown`) ────────────

fn md_table(header: &[&str], rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new(); // matches documentation.ts mdTable (empty → no table); database.ts always passes rows
    }
    let mut out = vec![
        format!("| {} |", header.join(" | ")),
        format!("| {} |", header.iter().map(|_| "---").collect::<Vec<_>>().join(" | ")),
    ];
    for r in rows {
        out.push(format!("| {} |", r.join(" | ")));
    }
    out.join("\n")
}

fn constraints(c: &SqlColumn) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if c.pk {
        parts.push("PK");
    }
    if c.unique {
        parts.push("unique");
    }
    if c.index {
        parts.push("index");
    }
    if c.nullable {
        parts.push("nullable");
    }
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join(", ")
    }
}

fn view_block(v: &SqlView, model: &Model) -> String {
    let slice = if v.slice == "V1" { "🔭 V1" } else { "🛶 V0" };
    let internal = if v.internal { " · 🔒 internal" } else { "" };
    let origin = if v.reference {
        "📦 reference (static seed)".to_string()
    } else {
        format!("source aggregate `{}`", v.aggregate)
    };
    let mut lines = vec![format!("### `{}` · {}{} · {}", v.name, slice, internal, origin), String::new()];
    if v.internal {
        lines.push("- **Consumed by**: command handlers / auth resolution (no GraphQL query).".into());
    }
    if v.reference {
        lines.push("- **Reference data**: seeded at deploy time (not event-fed).".into());
    } else {
        lines.push(format!("- **Fed by**: {}", v.fedby.iter().map(|n| format!("`{}`", n)).collect::<Vec<_>>().join(", ")));
    }
    if !v.filters.is_empty() {
        lines.push(format!("- **Filters**: {}", v.filters.join(" ")));
    }
    if !v.rules.is_empty() {
        lines.push(format!("- **Rules**: {}", v.rules.join(" ")));
    }
    if let Some(note) = &v.note {
        lines.push(format!("- **Note**: {}", note));
    }
    if !v.indexes.is_empty() {
        lines.push(format!("- **Indexes**: {}", v.indexes.iter().map(|ix| format!("`({})`", ix.join(", "))).collect::<Vec<_>>().join(", ")));
    }
    lines.push(String::new());
    let rows: Vec<Vec<String>> = v
        .columns
        .iter()
        .map(|c| {
            vec![
                format!("`{}`", c.name),
                format!("`{}`", c.ty),
                format!("`{}`", sql_type(&c.ty, model)),
                constraints(c),
                c.note.clone().unwrap_or_default(),
            ]
        })
        .collect();
    lines.push(md_table(&["Column", "Type", "SQL", "Constraints", "Notes"], &rows));
    lines.join("\n")
}

fn emit_views_markdown(model: &Model) -> String {
    parse_views(model).iter().map(|v| view_block(v, model)).collect::<Vec<_>>().join("\n\n")
}

/// Replace the body between `<!-- GENERATED:<id> START … -->` and `<!-- GENERATED:<id> END -->`
/// (port of cli.ts `injectGenerated`). Returns false if the markers are absent.
fn inject_generated(path: &PathBuf, id: &str, body: &str) -> Result<bool, String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {}", path.display(), e))?;
    let start_pat = format!("<!-- GENERATED:{} START", id);
    let end_pat = format!("<!-- GENERATED:{} END -->", id);
    let start_idx = match src.find(&start_pat) {
        Some(i) => i,
        None => return Ok(false),
    };
    let rel = match src[start_idx..].find("-->") {
        Some(i) => i,
        None => return Ok(false),
    };
    let start_marker_end = start_idx + rel + 3;
    let end_idx = match src.find(&end_pat) {
        Some(i) => i,
        None => return Ok(false),
    };
    let new = format!("{}\n\n{}\n\n{}", &src[..start_marker_end], body, &src[end_idx..]);
    fs::write(path, new).map_err(|e| format!("write {}: {}", path.display(), e))?;
    Ok(true)
}

// ─── c4.generated.dsl + c4.generated.md (port of emit/c4.ts) ─────────────────────────────────────

struct Actor {
    name: String,
    kind: String, // "aggregate" | "process-manager"
    file: &'static str, // "actors.yaml" | "processmanager.yaml" (where the definition lives)
    description: Option<String>,
    receives: Vec<Receive>,
}
struct Receive {
    message_ref: String,
    emits: Vec<String>, // raw $ref strings
    throws: Vec<String>,
    effect: Option<String>,
}
struct Ctx {
    id: String,
    description: String,
    aggregates: Vec<String>,
    process_managers: Vec<String>,
}
struct Container {
    id: String,
    technology: String,
    description: String,
}
struct External {
    id: String,
    description: String,
}
struct Rel {
    from: String,
    to: String,
    description: String,
}
struct Comp {
    id: String,
    description: String,
    instrumented: bool,
}
struct C4 {
    system_name: String,
    system_description: String,
    contexts: Vec<Ctx>,
    containers: Vec<Container>,
    externals: Vec<External>,
    relationships: Vec<Rel>,
    components: Vec<Comp>,
}

const PIPELINE: &[(&str, &str, &str)] = &[
    ("graphql-gateway", "command-bus", "dispatches command"),
    ("command-bus", "command-handlers", "invokes handler"),
    ("command-handlers", "event-store-adapter", "appends events"),
    ("event-store-adapter", "event-publisher", "publishes appended"),
    ("event-publisher", "message-consumers", "delivers events"),
    ("message-consumers", "projection-updaters", "feeds projections"),
    ("process-managers", "command-bus", "issues commands"),
];

/// `${prefix}${s.replace(/[^a-zA-Z0-9]+/g, '_')}` — runs of non-alphanumerics collapse to a single `_`.
fn c4id(prefix: &str, s: &str) -> String {
    let mut out = String::from(prefix);
    let mut prev_us = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_us = false;
        } else if !prev_us {
            out.push('_');
            prev_us = true;
        }
    }
    out
}

/// `"${s.replace(/"/g,'\"').replace(/\s+/g,' ').trim()}"` — escape quotes, collapse whitespace, trim, wrap.
fn q(s: &str) -> String {
    let escaped = s.replace('"', "\\\"");
    let mut collapsed = String::new();
    let mut prev_ws = false;
    for ch in escaped.chars() {
        if ch.is_whitespace() {
            if !prev_ws {
                collapsed.push(' ');
                prev_ws = true;
            }
        } else {
            collapsed.push(ch);
            prev_ws = false;
        }
    }
    format!("\"{}\"", collapsed.trim())
}

fn ref_names(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_sequence())
        .map(|s| {
            s.iter()
                .filter_map(|it| it.get("$ref").and_then(|r| r.as_str()).and_then(ref_name))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_actors(model: &Model) -> Vec<Actor> {
    let mut out = Vec::new();
    if let Some(Value::Mapping(m)) = model.defs.get("actors.yaml") {
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            let kind = match node.get("type").and_then(|x| x.as_str()) {
                Some(t @ ("aggregate" | "process-manager")) => t,
                _ => continue,
            };
            let mut receives = Vec::new();
            if let Some(seq) = node.get("receives").and_then(|x| x.as_sequence()) {
                for e in seq {
                    let message_ref = e
                        .get("message")
                        .and_then(|mm| mm.get("$ref"))
                        .and_then(|r| r.as_str())
                        .unwrap_or("")
                        .to_string();
                    let emits = ref_strings(e.get("emits"));
                    let throws = ref_strings(e.get("throws"));
                    let effect = e.get("effect").and_then(|x| x.as_str()).map(|s| s.to_string());
                    receives.push(Receive { message_ref, emits, throws, effect });
                }
            }
            out.push(Actor {
                name: name.to_string(),
                kind: kind.to_string(),
                file: "actors.yaml",
                description: node.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()),
                receives,
            });
        }
    }
    // Process managers (processmanager.yaml, typed-step DSL) project into the same Actor shape with
    // DERIVED emits/throws per leg: emits = delivered events ∪ the emits of each sent command per the
    // target aggregate's inbox (actors.yaml stays the single wiring truth); throws = guard `throws`.
    let agg_emits: HashMap<(String, String), Vec<String>> = out
        .iter()
        .flat_map(|a| {
            a.receives.iter().filter_map(move |r| {
                ref_name(&r.message_ref).map(|m| ((a.name.clone(), m), r.emits.clone()))
            })
        })
        .collect();
    if let Some(Value::Mapping(m)) = model.defs.get("processmanager.yaml") {
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            if node.get("type").and_then(|x| x.as_str()) != Some("process-manager") {
                continue;
            }
            let mut receives = Vec::new();
            if let Some(seq) = node.get("receives").and_then(|x| x.as_sequence()) {
                for e in seq {
                    let message_ref = e
                        .get("message")
                        .and_then(|mm| mm.get("$ref"))
                        .and_then(|r| r.as_str())
                        .unwrap_or("")
                        .to_string();
                    let mut emits: Vec<String> = Vec::new();
                    let mut throws: Vec<String> = Vec::new();
                    if let Some(steps) = e.get("steps").and_then(|x| x.as_sequence()) {
                        for s in steps {
                            if let Some(d) = s.get("deliver") {
                                if let Some(ev) = d.get("event").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()) {
                                    if !emits.contains(&ev.to_string()) {
                                        emits.push(ev.to_string());
                                    }
                                }
                            }
                            if let Some(sd) = s.get("send") {
                                let cmd = sd.get("command").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name);
                                let to = sd.get("to").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name);
                                if let (Some(cmd), Some(to)) = (cmd, to) {
                                    if let Some(evs) = agg_emits.get(&(to, cmd)) {
                                        for ev in evs {
                                            if !emits.contains(ev) {
                                                emits.push(ev.clone());
                                            }
                                        }
                                    }
                                }
                            }
                            if let Some(g) = s.get("guard") {
                                if let Some(er) = g.get("throws").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()) {
                                    if !throws.contains(&er.to_string()) {
                                        throws.push(er.to_string());
                                    }
                                }
                            }
                        }
                    }
                    let effect = e.get("description").and_then(|x| x.as_str()).map(|s| s.to_string());
                    receives.push(Receive { message_ref, emits, throws, effect });
                }
            }
            out.push(Actor {
                name: name.to_string(),
                kind: "process-manager".to_string(),
                file: "processmanager.yaml",
                description: node.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()),
                receives,
            });
        }
    }
    out
}

/// One Mermaid sequence diagram per process manager, generated from the typed steps
/// (processmanager.yaml). Participants map 1:1 to layers: the PM's pure decision, its private state
/// table, the read models (infrastructure read side), the outbound ports (adapters), and the target
/// aggregates (owners of the facts). A guard renders as a rejection arrow (command legs) or a skip
/// note (event legs) — so the diagram proves who may say "no" and who only records.
/// Returns (name → diagram body, in processmanager.yaml order); callers add their own framing
/// (Markdown fence, HTML <pre>), so one diagram source feeds every artifact.
fn pm_sequence_map(model: &Model) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let pms = match model.defs.get("processmanager.yaml") {
        Some(Value::Mapping(m)) => m,
        _ => return out,
    };
    let fmt_value = |v: &Value| -> String {
        if let Some(c) = v.get("const").and_then(|x| x.as_str()) {
            return c.to_string();
        }
        if let Some(f) = v.get("from").and_then(|f| f.get("$ref")).and_then(|x| x.as_str()) {
            let prop = f.rsplit('/').next().unwrap_or("?");
            return format!("{}.{}", ref_name(f).unwrap_or_default(), prop);
        }
        for (k, pfx) in [("from_state", "state."), ("from_read", ""), ("from_port", ""), ("from_envelope", "envelope.")] {
            if let Some(s) = v.get(k).and_then(|x| x.as_str()) {
                return format!("{}{}", pfx, s);
            }
        }
        "?".to_string()
    };
    let fmt_map = |v: Option<&Value>| -> String {
        v.and_then(|x| x.as_mapping())
            .map(|m| {
                m.iter()
                    .filter_map(|(k, val)| k.as_str().map(|c| format!("{}={}", c, fmt_value(val))))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    };
    for (k, node) in pms {
        let name = match k.as_str() {
            Some(s) => s,
            None => continue,
        };
        if node.get("type").and_then(|x| x.as_str()) != Some("process-manager") {
            continue;
        }
        let mut sl: Vec<String> = vec!["sequenceDiagram".into(), "  autonumber".into()];
        sl.push("  participant IN as Inbox (trigger)".into());
        sl.push(format!("  participant PM as {} (decides)", name));
        let state_table = node
            .get("state_table")
            .and_then(|x| x.get("$ref"))
            .and_then(|x| x.as_str())
            .and_then(ref_name);
        if let Some(st) = &state_table {
            sl.push(format!("  participant ST as {} (state)", st));
        }
        // Deterministic first-use participant order for read models, ports, aggregates.
        let mut extra: Vec<(String, String)> = Vec::new(); // (id, declaration)
        let declare = |extra: &mut Vec<(String, String)>, id: String, label: String| {
            if !extra.iter().any(|(i, _)| *i == id) {
                extra.push((id.clone(), label));
            }
        };
        let legs = node.get("receives").and_then(|x| x.as_sequence()).cloned().unwrap_or_default();
        for e in &legs {
            if let Some(steps) = e.get("steps").and_then(|x| x.as_sequence()) {
                for s in steps {
                    if let Some(r) = s.get("read") {
                        if let Some(m) = r.get("model").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name) {
                            declare(&mut extra, c4id("RM_", &m), format!("  participant {} as {} (read model)", c4id("RM_", &m), m));
                        }
                    }
                    if let Some(c) = s.get("call") {
                        if let Some(p) = c.get("port").and_then(|x| x.as_str()) {
                            declare(&mut extra, c4id("PT_", p), format!("  participant {} as port {} (adapter)", c4id("PT_", p), p));
                        }
                    }
                    for kind in ["deliver", "send"] {
                        if let Some(d) = s.get(kind) {
                            if let Some(t) = d.get("to").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name) {
                                declare(&mut extra, c4id("AG_", &t), format!("  participant {} as {} (aggregate)", c4id("AG_", &t), t));
                            }
                        }
                    }
                }
            }
        }
        for (_, decl) in &extra {
            sl.push(decl.clone());
        }
        for e in &legs {
            let msg_ref = e.get("message").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("");
            let msg = ref_name(msg_ref).unwrap_or_else(|| "?".to_string());
            let is_command = msg_ref.starts_with("commands.yaml#/");
            sl.push(format!("  rect rgb(245,245,245)").to_string());
            sl.push(format!("  IN->>PM: {} ({})", msg, if is_command { "command" } else { "event" }));
            let steps = e.get("steps").and_then(|x| x.as_sequence()).cloned().unwrap_or_default();
            for s in &steps {
                if let Some(r) = s.get("read") {
                    let m = r.get("model").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name).unwrap_or_default();
                    let alias = r.get("as").and_then(|x| x.as_str()).unwrap_or("?");
                    let w = fmt_map(r.get("where"));
                    sl.push(format!("  PM->>{}: read as {}{}", c4id("RM_", &m), alias, if w.is_empty() { String::new() } else { format!(" [{}]", w) }));
                } else if let Some(g) = s.get("guard") {
                    let cond = fmt_map_nested(g.get("that"), &fmt_value);
                    if let Some(er) = g.get("throws").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name) {
                        sl.push(format!("  PM--xIN: throws {}{}", er, if cond.is_empty() { String::new() } else { format!(" unless {}", cond) }));
                    } else {
                        sl.push(format!("  Note over PM: skip unless {}", if cond.is_empty() { "precondition holds".to_string() } else { cond }));
                    }
                } else if let Some(c) = s.get("call") {
                    let p = c.get("port").and_then(|x| x.as_str()).unwrap_or("?");
                    let op = c.get("operation").and_then(|x| x.as_str()).unwrap_or("?");
                    sl.push(format!("  PM->>{}: {}", c4id("PT_", p), op));
                } else if let Some(d) = s.get("deliver") {
                    let ev = d.get("event").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name).unwrap_or_default();
                    let to = d.get("to").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name).unwrap_or_default();
                    let fe = d.get("for_each").and_then(|x| x.as_str()).map(|a| format!(" (for each {})", a)).unwrap_or_default();
                    sl.push(format!("  PM->>{}: deliver {}{} — the aggregate records it", c4id("AG_", &to), ev, fe));
                } else if let Some(d) = s.get("send") {
                    let cm = d.get("command").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name).unwrap_or_default();
                    let to = d.get("to").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name).unwrap_or_default();
                    let fe = d.get("for_each").and_then(|x| x.as_str()).map(|a| format!(" (for each {})", a)).unwrap_or_default();
                    sl.push(format!("  PM->>{}: send {}{} — the aggregate validates", c4id("AG_", &to), cm, fe));
                } else if let Some(st) = s.get("state") {
                    let by = fmt_map(st.get("by"));
                    let exp = fmt_map(st.get("expect"));
                    let set = fmt_map(st.get("set"));
                    let mut parts: Vec<String> = Vec::new();
                    if !by.is_empty() {
                        parts.push(format!("by {}", by));
                    }
                    if !exp.is_empty() {
                        parts.push(format!("expect {}", exp));
                    }
                    if !set.is_empty() {
                        parts.push(format!("set {}", set));
                    }
                    sl.push(format!("  PM->>ST: {}", parts.join("; ")));
                }
            }
            sl.push("  end".into());
        }
        out.push((name.to_string(), sl.join("\n")));
    }
    out
}

/// The per-PM diagrams as `### name` + fenced Markdown blocks (c4.generated.md framing).
fn pm_sequence_blocks(model: &Model) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (name, body) in pm_sequence_map(model) {
        for line in [format!("### {}", name), String::new(), "```mermaid".into(), body, "```".into(), String::new()] {
            out.push(line);
        }
    }
    out
}

/// `that` conditions: `alias.field == CONST` joined with ` and `.
fn fmt_map_nested(v: Option<&Value>, fmt_value: &dyn Fn(&Value) -> String) -> String {
    v.and_then(|x| x.as_mapping())
        .map(|m| {
            m.iter()
                .flat_map(|(subj, fields)| {
                    let s = subj.as_str().unwrap_or("?").to_string();
                    fields
                        .as_mapping()
                        .map(|fm| {
                            fm.iter()
                                .filter_map(|(f, val)| f.as_str().map(|fx| format!("{}.{} == {}", s, fx, fmt_value(val))))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join(" and ")
        })
        .unwrap_or_default()
}

// ─── §2b — typed-step process-manager validation (processmanager.yaml) ──────────────────────────

/// Enum members of a scalars.yaml scalar reached via `r`, when it has an `enum`.
fn scalar_enum(model: &Model, r: &str, ctx: &str) -> Option<Vec<String>> {
    resolve_ref(model, r, ctx)?
        .get("enum")?
        .as_sequence()
        .map(|s| s.iter().filter_map(|v| v.as_str().map(|x| x.to_string())).collect())
}

/// Column name → enum members (when the column's `type.$ref` is an enum scalar) of a table/view/
/// projection definition (`columns` mapping). Columns without a resolvable enum map to None.
fn columns_info(model: &Model, def: &Value, ctx: &str) -> Option<HashMap<String, Option<Vec<String>>>> {
    let cols = def.get("columns")?.as_mapping()?;
    let mut out = HashMap::new();
    for (k, col) in cols {
        let name = match k.as_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let en = col
            .get("type")
            .and_then(|t| t.get("$ref"))
            .and_then(|x| x.as_str())
            .and_then(|r| scalar_enum(model, r, ctx));
        out.insert(name, en);
    }
    Some(out)
}

/// Property name → enum members (when the property `$ref`s an enum scalar) of a command/event def.
fn props_info(model: &Model, def: &Value, ctx: &str) -> HashMap<String, Option<Vec<String>>> {
    let mut out = HashMap::new();
    if let Some(props) = def.get("properties").and_then(|x| x.as_mapping()) {
        for (k, p) in props {
            let name = match k.as_str() {
                Some(s) => s.to_string(),
                None => continue,
            };
            let en = p.get("$ref").and_then(|x| x.as_str()).and_then(|r| scalar_enum(model, r, ctx));
            out.insert(name, en);
        }
    }
    out
}

/// One typed step value (`const` / `from` / `from_state` / `from_read` / `from_port` /
/// `from_envelope`) — exactly one form, and each form checked against what it names.
#[allow(clippy::too_many_arguments)]
fn check_pm_value(
    v: &Value,
    field_enum: Option<&Vec<String>>,
    state_cols: Option<&HashMap<String, Option<Vec<String>>>>,
    aliases: &HashMap<String, HashMap<String, Option<Vec<String>>>>,
    ports: &HashMap<String, BTreeSet<String>>,
    where_: &str,
    issues: &mut Vec<Issue>,
) {
    let forms = ["const", "from", "from_state", "from_read", "from_port", "from_envelope"];
    let present: Vec<&str> = forms.iter().copied().filter(|f| v.get(*f).is_some()).collect();
    if present.len() != 1 {
        issues.push(err(
            "pm-value",
            where_.to_string(),
            "a step value must be exactly one of { const | from | from_state | from_read | from_port | from_envelope }.".into(),
        ));
        return;
    }
    match present[0] {
        "const" => {
            let c = v.get("const").and_then(|x| x.as_str()).unwrap_or("");
            if let Some(en) = field_enum {
                if !en.iter().any(|m| m == c) {
                    issues.push(err(
                        "pm-const",
                        where_.to_string(),
                        format!("const '{}' is not a member of the field's enum scalar ({}).", c, en.join("|")),
                    ));
                }
            }
        }
        "from" => {
            if v.get("from").and_then(|f| f.get("$ref")).and_then(|x| x.as_str()).is_none() {
                issues.push(err("pm-value", where_.to_string(), "`from` must be a { $ref: '<file>#/<Msg>/properties/<p>' }.".into()));
            }
        }
        "from_state" => {
            let c = v.get("from_state").and_then(|x| x.as_str()).unwrap_or("");
            match state_cols {
                None => issues.push(err("pm-value", where_.to_string(), "`from_state` used but the process manager declares no state_table.".into())),
                Some(cols) if !cols.contains_key(c) => issues.push(err(
                    "pm-value",
                    where_.to_string(),
                    format!("`from_state` column '{}' does not exist on the state table.", c),
                )),
                _ => {}
            }
        }
        "from_read" => {
            let spec = v.get("from_read").and_then(|x| x.as_str()).unwrap_or("");
            let (alias, col) = match spec.split_once('.') {
                Some(p) => p,
                None => {
                    issues.push(err("pm-value", where_.to_string(), "`from_read` must be '<alias>.<column>'.".into()));
                    return;
                }
            };
            match aliases.get(alias) {
                None => issues.push(err("pm-value", where_.to_string(), format!("`from_read` alias '{}' is not a prior read step.", alias))),
                Some(cols) if !cols.is_empty() && !cols.contains_key(col) => issues.push(err(
                    "pm-value",
                    where_.to_string(),
                    format!("`from_read` column '{}' does not exist on read model '{}'.", col, alias),
                )),
                _ => {}
            }
        }
        "from_port" => {
            let spec = v.get("from_port").and_then(|x| x.as_str()).unwrap_or("");
            let ok = spec
                .split_once('.')
                .map(|(p, op)| ports.get(p).map(|ops| ops.contains(op)).unwrap_or(false))
                .unwrap_or(false);
            if !ok {
                issues.push(err("pm-value", where_.to_string(), format!("`from_port` '{}' is not a declared <port>.<operation>.", spec)));
            }
        }
        "from_envelope" => {
            let f = v.get("from_envelope").and_then(|x| x.as_str()).unwrap_or("");
            if !["event_id", "correlation_id", "occurred_at"].contains(&f) {
                issues.push(err(
                    "pm-value",
                    where_.to_string(),
                    format!("`from_envelope` '{}' is not one of event_id | correlation_id | occurred_at (ADR-0041).", f),
                ));
            }
        }
        _ => unreachable!(),
    }
}

/// §2b — validate every typed-step process manager: state columns exist (+ enum consts valid), read
/// models resolve, ports are declared, deliver/send targets receive the message, command-leg guards
/// `throws` typed errors while event-leg guards `skip` (facts are never rejected).
fn validate_process_managers(model: &Model, issues: &mut Vec<Issue>) {
    const CTX: &str = "processmanager.yaml";
    // actors.yaml aggregate inboxes: name → received message names.
    let mut agg_inbox: HashMap<String, BTreeSet<String>> = HashMap::new();
    if let Some(Value::Mapping(m)) = model.defs.get("actors.yaml") {
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            let mut set = BTreeSet::new();
            if let Some(seq) = node.get("receives").and_then(|x| x.as_sequence()) {
                for e in seq {
                    if let Some(n) = e.get("message").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name) {
                        set.insert(n);
                    }
                }
            }
            agg_inbox.insert(name.to_string(), set);
        }
    }

    let pms = match model.defs.get(CTX) {
        Some(Value::Mapping(m)) => m,
        _ => return,
    };
    for (k, node) in pms {
        let name = match k.as_str() {
            Some(s) => s,
            None => continue,
        };
        if node.get("type").and_then(|x| x.as_str()) != Some("process-manager") {
            issues.push(err("pm-type", format!("{}/{}", CTX, name), "every processmanager.yaml entry must declare `type: process-manager`.".into()));
            continue;
        }
        let at = format!("{}/{}", CTX, name);

        // State table (optional — required as soon as a `state` step exists).
        let st_ref = node.get("state_table").and_then(|x| x.get("$ref")).and_then(|x| x.as_str());
        let state_cols: Option<HashMap<String, Option<Vec<String>>>> = match st_ref {
            None => None,
            Some(r) => {
                if !r.starts_with("database/tables/") {
                    issues.push(err("pm-state-table", format!("{}.state_table", at), format!("state_table must $ref a database/tables/*.yaml table, got '{}'.", r)));
                }
                match resolve_ref(model, r, CTX).and_then(|d| columns_info(model, d, CTX)) {
                    Some(c) => Some(c),
                    None => None, // dangling → §1 reports it
                }
            }
        };

        // Ports: name → operation set, resolved THROUGH the service catalog — each entry is a
        // `$ref` into services.yaml (ADR-20260719-214500); the port's operations are the resolved
        // service's `operations` keys (the catalog itself is validated by §2d).
        let mut ports: HashMap<String, BTreeSet<String>> = HashMap::new();
        if let Some(pmap) = node.get("ports").and_then(|x| x.as_mapping()) {
            for (pk, pv) in pmap {
                let pname = match pk.as_str() {
                    Some(s) => s.to_string(),
                    None => continue,
                };
                let pw = format!("{}.ports.{}", at, pname);
                let sref = match pv.get("$ref").and_then(|x| x.as_str()) {
                    Some(r) => r,
                    None => {
                        issues.push(err("pm-port", pw, "ports must $ref the service catalog (services.yaml), ADR-20260719-214500.".into()));
                        continue;
                    }
                };
                if ref_target_file(sref, CTX).as_deref() != Some("services.yaml") {
                    issues.push(err("pm-port", pw, format!("a port must $ref a services.yaml service, got '{}'.", sref)));
                    continue;
                }
                let ops: BTreeSet<String> = match resolve_ref(model, sref, CTX) {
                    None => continue, // dangling → §1 reports it
                    Some(svc) => svc
                        .get("operations")
                        .and_then(|x| x.as_mapping())
                        .map(|m| m.iter().filter_map(|(ok, _)| ok.as_str().map(|s| s.to_string())).collect())
                        .unwrap_or_default(),
                };
                ports.insert(pname, ops);
            }
        }

        let legs = match node.get("receives").and_then(|x| x.as_sequence()) {
            Some(s) => s,
            None => continue,
        };
        for (i, e) in legs.iter().enumerate() {
            let leg = format!("{}.receives[{}]", at, i);
            let msg_ref = e.get("message").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("");
            let is_command = ref_target_file(msg_ref, CTX).as_deref() == Some("commands.yaml");
            let msg_props = resolve_ref(model, msg_ref, CTX).map(|d| props_info(model, d, CTX)).unwrap_or_default();

            let steps = match e.get("steps").and_then(|x| x.as_sequence()) {
                Some(s) if !s.is_empty() => s,
                _ => {
                    issues.push(err("pm-no-steps", leg.clone(), "a receives entry must declare an ordered, non-empty `steps` list.".into()));
                    continue;
                }
            };

            let mut aliases: HashMap<String, HashMap<String, Option<Vec<String>>>> = HashMap::new();
            for (j, s) in steps.iter().enumerate() {
                let sw = format!("{}.steps[{}]", leg, j);
                let smap = match s.as_mapping() {
                    Some(m) => m,
                    None => {
                        issues.push(err("pm-step", sw, "each step must be a single-key mapping.".into()));
                        continue;
                    }
                };
                if smap.len() != 1 {
                    issues.push(err("pm-step", sw.clone(), "each step must have exactly one kind (read | guard | call | deliver | send | state).".into()));
                    continue;
                }
                let (kind, body) = {
                    let (k, v) = smap.iter().next().unwrap();
                    (k.as_str().unwrap_or("?"), v)
                };
                match kind {
                    "read" => {
                        let model_ref = body.get("model").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("");
                        let tf = ref_target_file(model_ref, CTX).unwrap_or_default();
                        if tf != "database/tables/projection_tables.yaml" && tf != "database/projection_views.yaml" {
                            issues.push(err(
                                "pm-read",
                                format!("{}.model", sw),
                                format!("read.model must $ref a projection table or a View_* (got '{}').", model_ref),
                            ));
                        }
                        let cols = resolve_ref(model, model_ref, CTX)
                            .and_then(|d| columns_info(model, d, CTX))
                            .unwrap_or_default();
                        let alias = body.get("as").and_then(|x| x.as_str()).unwrap_or("");
                        if alias.is_empty() {
                            issues.push(err("pm-read", format!("{}.as", sw), "read must name its result with `as`.".into()));
                        }
                        if let Some(wmap) = body.get("where").and_then(|x| x.as_mapping()) {
                            for (wk, wv) in wmap {
                                let col = wk.as_str().unwrap_or("?");
                                if !cols.is_empty() && !cols.contains_key(col) {
                                    issues.push(err("pm-read", format!("{}.where.{}", sw, col), format!("column '{}' does not exist on the read model.", col)));
                                }
                                check_pm_value(wv, cols.get(col).and_then(|e| e.as_ref()), state_cols.as_ref(), &aliases, &ports, &format!("{}.where.{}", sw, col), issues);
                            }
                        }
                        if !alias.is_empty() {
                            aliases.insert(alias.to_string(), cols);
                        }
                    }
                    "guard" => {
                        let throws = body.get("throws");
                        let skip = body.get("skip").and_then(|x| x.as_bool()).unwrap_or(false);
                        // Exactly one outcome. `throws` = ERROR (typed, on any leg — an event leg
                        // aborts and surfaces it); `skip` = benign expected alternative (never an
                        // error). A command leg has no benign-skip path: it only rejects.
                        match (throws, skip) {
                            (Some(t), false) => {
                                match t.get("$ref").and_then(|x| x.as_str()) {
                                    None => issues.push(err("pm-guard", format!("{}.throws", sw), "guard.throws must be a { $ref: 'errors.yaml#/<Error>' }.".into())),
                                    Some(r) => {
                                        if ref_target_file(r, CTX).as_deref() != Some("errors.yaml") {
                                            issues.push(err("pm-guard", format!("{}.throws", sw), format!("guard.throws must reference errors.yaml, got '{}'.", r)));
                                        }
                                    }
                                }
                            }
                            (None, true) => {
                                if is_command {
                                    issues.push(err(
                                        "pm-guard",
                                        sw.clone(),
                                        "a COMMAND-leg guard must `throws` a typed error — a command has no benign-skip path; in case of error the guard throws.".into(),
                                    ));
                                }
                            }
                            _ => issues.push(err(
                                "pm-guard",
                                sw.clone(),
                                "a guard must declare exactly one outcome: `throws` (error — typed $ref errors.yaml) or `skip: true` (benign alternative).".into(),
                            )),
                        }
                        if let Some(that) = body.get("that").and_then(|x| x.as_mapping()) {
                            for (subj_k, fields) in that {
                                let subj = subj_k.as_str().unwrap_or("?");
                                let fmap = match fields.as_mapping() {
                                    Some(m) => m,
                                    None => continue,
                                };
                                for (fk, fv) in fmap {
                                    let field = fk.as_str().unwrap_or("?");
                                    let fw = format!("{}.that.{}.{}", sw, subj, field);
                                    let field_enum: Option<&Vec<String>> = match subj {
                                        "message" => {
                                            if !msg_props.is_empty() && !msg_props.contains_key(field) {
                                                issues.push(err("pm-guard", fw.clone(), format!("'{}' is not a property of the trigger message.", field)));
                                            }
                                            msg_props.get(field).and_then(|e| e.as_ref())
                                        }
                                        "state" => match state_cols.as_ref() {
                                            None => {
                                                issues.push(err("pm-guard", fw.clone(), "guard on `state` but the process manager declares no state_table.".into()));
                                                None
                                            }
                                            Some(cols) => {
                                                if !cols.contains_key(field) {
                                                    issues.push(err("pm-guard", fw.clone(), format!("column '{}' does not exist on the state table.", field)));
                                                }
                                                cols.get(field).and_then(|e| e.as_ref())
                                            }
                                        },
                                        alias => match aliases.get(alias) {
                                            None => {
                                                issues.push(err("pm-guard", fw.clone(), format!("guard subject '{}' is neither `message`, `state`, nor a prior read alias.", alias)));
                                                None
                                            }
                                            Some(cols) => {
                                                if !cols.is_empty() && !cols.contains_key(field) {
                                                    issues.push(err("pm-guard", fw.clone(), format!("column '{}' does not exist on read model '{}'.", field, alias)));
                                                }
                                                cols.get(field).and_then(|e| e.as_ref())
                                            }
                                        },
                                    };
                                    if fv.get("const").is_none() {
                                        issues.push(err("pm-guard", fw.clone(), "a guard condition value must be structural: { const: <ENUM> }.".into()));
                                    } else {
                                        check_pm_value(fv, field_enum, state_cols.as_ref(), &aliases, &ports, &fw, issues);
                                    }
                                }
                            }
                        }
                    }
                    "call" => {
                        let port = body.get("port").and_then(|x| x.as_str()).unwrap_or("");
                        let op = body.get("operation").and_then(|x| x.as_str()).unwrap_or("");
                        let ok = ports.get(port).map(|ops| ops.contains(op)).unwrap_or(false);
                        if !ok {
                            issues.push(err("pm-call", sw.clone(), format!("call '{}.{}' is not a declared port operation.", port, op)));
                        }
                    }
                    "deliver" | "send" => {
                        let rule: &'static str = if kind == "deliver" { "pm-deliver" } else { "pm-send" };
                        let (mkey, target_file) = if kind == "deliver" { ("event", "events.yaml") } else { ("command", "commands.yaml") };
                        let mref = body.get(mkey).and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("");
                        if ref_target_file(mref, CTX).as_deref() != Some(target_file) {
                            issues.push(err(rule, format!("{}.{}", sw, mkey), format!("{}.{} must reference {}, got '{}'.", kind, mkey, target_file, mref)));
                        }
                        let mname = ref_name(mref).unwrap_or_default();
                        let to = body.get("to").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("");
                        if ref_target_file(to, CTX).as_deref() != Some("actors.yaml") {
                            issues.push(err(rule, format!("{}.to", sw), format!("{}.to must reference an actors.yaml aggregate, got '{}'.", kind, to)));
                        } else {
                            let tname = ref_name(to).unwrap_or_default();
                            match agg_inbox.get(&tname) {
                                None => issues.push(err(rule, format!("{}.to", sw), format!("aggregate '{}' does not exist in actors.yaml.", tname))),
                                Some(inbox) if !inbox.contains(&mname) => issues.push(err(
                                    rule,
                                    format!("{}.to", sw),
                                    format!("aggregate '{}' does not receive '{}' (actors.yaml inbox) — the {} would be dropped.", tname, mname, mkey),
                                )),
                                _ => {}
                            }
                        }
                        let target_props = resolve_ref(model, mref, CTX).map(|d| props_info(model, d, CTX)).unwrap_or_default();
                        if let Some(wmap) = body.get("with").and_then(|x| x.as_mapping()) {
                            for (wk, wv) in wmap {
                                let prop = wk.as_str().unwrap_or("?");
                                if !target_props.is_empty() && !target_props.contains_key(prop) {
                                    issues.push(err(rule, format!("{}.with.{}", sw, prop), format!("'{}' is not a property of '{}'.", prop, mname)));
                                }
                                check_pm_value(wv, target_props.get(prop).and_then(|e| e.as_ref()), state_cols.as_ref(), &aliases, &ports, &format!("{}.with.{}", sw, prop), issues);
                            }
                        }
                        if let Some(fe) = body.get("for_each").and_then(|x| x.as_str()) {
                            if !aliases.contains_key(fe) {
                                issues.push(err(rule, format!("{}.for_each", sw), format!("for_each alias '{}' is not a prior read step.", fe)));
                            }
                        }
                    }
                    "state" => {
                        let cols = match state_cols.as_ref() {
                            None => {
                                issues.push(err("pm-state", sw.clone(), "a `state` step requires the process manager to declare `state_table`.".into()));
                                continue;
                            }
                            Some(c) => c,
                        };
                        for section in ["by", "expect", "set"] {
                            if let Some(smap2) = body.get(section).and_then(|x| x.as_mapping()) {
                                for (ck, cv) in smap2 {
                                    let col = ck.as_str().unwrap_or("?");
                                    let cw = format!("{}.{}.{}", sw, section, col);
                                    if !cols.contains_key(col) {
                                        issues.push(err("pm-state", cw.clone(), format!("column '{}' does not exist on the state table.", col)));
                                    }
                                    if section == "expect" && cv.get("const").is_none() {
                                        issues.push(err("pm-state", cw.clone(), "an `expect` value must be structural: { const: <ENUM> }.".into()));
                                        continue;
                                    }
                                    check_pm_value(cv, cols.get(col).and_then(|e| e.as_ref()), state_cols.as_ref(), &aliases, &ports, &cw, issues);
                                }
                            }
                        }
                    }
                    other => {
                        issues.push(err("pm-step", sw, format!("unknown step kind '{}' (read | guard | call | deliver | send | state).", other)));
                    }
                }
            }
        }
    }
}

// ─── §2d — service-catalog validation (services.yaml, ADR-20260719-214500) ──────────────────────

/// `^[a-z][a-z0-9_]*$` — a service operation name is a snake_case domain verb (never the
/// provider's vocabulary; the qualified form is `<service>.<operation>`).
fn svc_op_name_ok(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `^POST /adapters/[a-z0-9-]+(/[a-z0-9-]+)+$` — adapter routes are POST and live under
/// `/adapters/` in the PROVIDER's vocabulary (the derived `/services/*` surface is never declared).
fn svc_adapter_route_ok(s: &str) -> bool {
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
fn validate_services(model: &Model, issues: &mut Vec<Issue>) {
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

/// Raw `$ref` strings of a ref-list (toRefList).
fn ref_strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_sequence())
        .map(|s| {
            s.iter()
                .filter_map(|it| it.get("$ref").and_then(|r| r.as_str()).map(|x| x.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// (view name, fedBy event names) for every read model — fold views + materialized tables.
fn views_fedby(model: &Model) -> Vec<(String, Vec<String>)> {
    parse_views(model).iter().map(|v| (v.name.clone(), v.fedby.clone())).collect()
}

fn read_c4(model: &Model) -> C4 {
    let l2 = model.defs.get("architecture/c4-l2.yaml");
    let l3 = model.defs.get("architecture/c4-l3.yaml");
    let l2get = |k: &str| l2.and_then(|v| v.get(k));
    let system = l2get("system");
    let sstr = |k: &str| system.and_then(|s| s.get(k)).and_then(|x| x.as_str());
    let mut contexts = Vec::new();
    if let Some(cm) = l2get("boundedContexts").and_then(|v| v.as_mapping()) {
        for (k, bc) in cm {
            if let Some(id) = k.as_str() {
                contexts.push(Ctx {
                    id: id.to_string(),
                    description: bc.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    aggregates: ref_names(bc.get("aggregates")),
                    process_managers: ref_names(bc.get("processManagers")),
                });
            }
        }
    }
    let mut containers = Vec::new();
    if let Some(cm) = l2get("containers").and_then(|v| v.as_mapping()) {
        for (k, c) in cm {
            if let Some(id) = k.as_str() {
                containers.push(Container {
                    id: id.to_string(),
                    technology: c.get("technology").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    description: c.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                });
            }
        }
    }
    let mut externals = Vec::new();
    if let Some(cm) = l2get("externalSystems").and_then(|v| v.as_mapping()) {
        for (k, x) in cm {
            if let Some(id) = k.as_str() {
                externals.push(External {
                    id: id.to_string(),
                    description: x.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string(),
                });
            }
        }
    }
    let mut relationships = Vec::new();
    if let Some(seq) = l2get("relationships").and_then(|v| v.as_sequence()) {
        for r in seq {
            relationships.push(Rel {
                from: r.get("from").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                to: r.get("to").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                description: r.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            });
        }
    }
    let mut components = Vec::new();
    if let Some(cm) = l3.and_then(|v| v.get("components")).and_then(|v| v.as_mapping()) {
        for (k, c) in cm {
            if let Some(id) = k.as_str() {
                components.push(Comp {
                    id: id.to_string(),
                    description: c.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    instrumented: c.get("instrumented").and_then(|x| x.as_bool()) == Some(true),
                });
            }
        }
    }
    C4 {
        system_name: sstr("name").unwrap_or("Captain.Food").to_string(),
        system_description: sstr("description").unwrap_or("").to_string(),
        contexts,
        containers,
        externals,
        relationships,
        components,
    }
}

fn push_view(l: &mut Vec<String>, decl: &str) {
    l.push(format!("    {} {{", decl));
    l.push("      include *".into());
    l.push("      autolayout lr".into());
    l.push("    }".into());
}
fn push_style(l: &mut Vec<String>, tag: &str, props: &[&str]) {
    l.push(format!("      element \"{}\" {{", tag));
    for p in props {
        l.push(format!("        {}", p));
    }
    l.push("      }".into());
}

fn emit_structurizr(model: &Model) -> String {
    let c4 = read_c4(model);
    let comp_ids: std::collections::HashSet<&str> = c4.components.iter().map(|c| c.id.as_str()).collect();
    let node_id = |key: &str| -> String {
        if comp_ids.contains(key) {
            c4id("c_", key)
        } else if c4.containers.iter().any(|c| c.id == key) {
            c4id("ct_", key)
        } else if c4.externals.iter().any(|x| x.id == key) {
            c4id("x_", key)
        } else {
            c4id("n_", key)
        }
    };
    let mut l: Vec<String> = Vec::new();
    l.push(format!("workspace {} {} {{", q(&c4.system_name), q(&c4.system_description)));
    l.push("  model {".into());
    l.push(format!("    ss = softwareSystem {} {} {{", q(&c4.system_name), q(&c4.system_description)));
    for c in &c4.containers {
        let open = format!(
            "      {} = container {} {} {}",
            c4id("ct_", &c.id), q(&c.id), q(&c.description), q(&c.technology)
        );
        if c.id != "api" {
            l.push(open);
            continue;
        }
        l.push(format!("{} {{", open));
        for ctx in &c4.contexts {
            let mut members: Vec<(&str, &str)> = Vec::new();
            for a in &ctx.aggregates {
                members.push((a.as_str(), "Aggregate"));
            }
            for p in &ctx.process_managers {
                members.push((p.as_str(), "ProcessManager"));
            }
            if members.is_empty() {
                continue;
            }
            l.push(format!("        group {} {{", q(&ctx.id)));
            for (n, tag) in &members {
                l.push(format!("          {} = component {} {} {}", c4id("a_", n), q(n), q(&ctx.description), q(tag)));
            }
            l.push("        }".into());
        }
        l.push("        group \"Infrastructure\" {".into());
        for comp in &c4.components {
            l.push(format!(
                "          {} = component {} {} {}",
                c4id("c_", &comp.id), q(&comp.id), q(&comp.description),
                q(if comp.instrumented { "Instrumented" } else { "Domain" })
            ));
        }
        l.push("        }".into());
        l.push("      }".into());
    }
    l.push("    }".into());
    for x in &c4.externals {
        l.push(format!("    {} = softwareSystem {} {} \"External\"", c4id("x_", &x.id), q(&x.id), q(&x.description)));
    }
    l.push("".into());
    for r in &c4.relationships {
        l.push(format!("    {} -> {} {}", node_id(&r.from), node_id(&r.to), q(&r.description)));
    }
    for (from, to, desc) in PIPELINE {
        if comp_ids.contains(from) && comp_ids.contains(to) {
            l.push(format!("    {} -> {} {}", c4id("c_", from), c4id("c_", to), q(desc)));
        }
    }
    if comp_ids.contains("projection-updaters") {
        l.push(format!("    {} -> {} \"writes read models\"", c4id("c_", "projection-updaters"), c4id("ct_", "read-models")));
    }
    if comp_ids.contains("event-store-adapter") {
        l.push(format!("    {} -> {} \"appends to domain_events\"", c4id("c_", "event-store-adapter"), c4id("ct_", "event-store")));
    }
    l.push("  }".into());
    l.push("  views {".into());
    push_view(&mut l, "systemContext ss \"SystemContext\"");
    push_view(&mut l, "container ss \"Containers\"");
    push_view(&mut l, &format!("component {} \"ApiComponents\"", c4id("ct_", "api")));
    l.push("    styles {".into());
    push_style(&mut l, "Element", &["color #ffffff"]);
    push_style(&mut l, "Software System", &["background #2d4f4a"]);
    push_style(&mut l, "Container", &["background #313335"]);
    push_style(&mut l, "External", &["background #cc7832"]);
    push_style(&mut l, "Aggregate", &["background #4ec9b0", "color #11201d"]);
    push_style(&mut l, "ProcessManager", &["background #56a0c0"]);
    push_style(&mut l, "Instrumented", &["background #c586c0"]);
    push_style(&mut l, "Domain", &["background #313335"]);
    l.push("    }".into());
    l.push("  }".into());
    l.push("}".into());
    l.push("".into());
    l.join("\n")
}

fn emit_mermaid(model: &Model) -> String {
    let c4 = read_c4(model);
    let actors = parse_actors(model);
    let views = views_fedby(model);

    // 1) Container diagram.
    let mut container: Vec<String> = vec!["flowchart LR".into()];
    container.push("  subgraph CaptainFood[\"Captain.Food\"]".into());
    for c in &c4.containers {
        container.push(format!("    {}[\"{}<br/><small>{}</small>\"]", c4id("n_", &c.id), c.id, c.technology));
    }
    container.push("  end".into());
    for x in &c4.externals {
        container.push(format!("  {}[/\"{}\"/]", c4id("n_", &x.id), x.id));
    }
    for r in &c4.relationships {
        container.push(format!("  {} -->|\"{}\"| {}", c4id("n_", &r.from), r.description.replace('"', "'"), c4id("n_", &r.to)));
    }

    // 2) Domain diagram: contexts → aggregates → the read models they feed.
    let mut evt_views: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for (vname, fedby) in &views {
        for e in fedby {
            evt_views.entry(e.clone()).or_default().push(vname.clone());
        }
    }
    let emits_of = |a: &Actor| -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for r in &a.receives {
            for ev in &r.emits {
                if let Some(n) = ref_name(ev) {
                    if !v.contains(&n) {
                        v.push(n);
                    }
                }
            }
        }
        v
    };
    let mut domain: Vec<String> = vec!["flowchart LR".into()];
    for ctx in &c4.contexts {
        domain.push(format!("  subgraph {}[\"{}\"]", c4id("g_", &ctx.id), ctx.id));
        for a in ctx.aggregates.iter().chain(ctx.process_managers.iter()) {
            domain.push(format!("    {}[\"{}\"]", c4id("a_", a), a));
        }
        domain.push("  end".into());
    }
    let mut view_ids: Vec<String> = Vec::new();
    let mut edges: Vec<String> = Vec::new();
    for a in &actors {
        let mut seen_v: Vec<String> = Vec::new();
        for ev in emits_of(a) {
            if let Some(vs) = evt_views.get(&ev) {
                for v in vs {
                    if !seen_v.contains(v) {
                        seen_v.push(v.clone());
                    }
                }
            }
        }
        for v in &seen_v {
            if !view_ids.contains(v) {
                view_ids.push(v.clone());
            }
            let edge = format!("  {} --> {}", c4id("a_", &a.name), c4id("v_", v));
            if !edges.contains(&edge) {
                edges.push(edge);
            }
        }
    }
    for v in &view_ids {
        domain.push(format!("  {}[(\"{}\")]", c4id("v_", v), v));
    }
    domain.extend(edges);

    // 3) Saga sequence diagrams — generated from the TYPED STEPS (processmanager.yaml): each step
    //    kind maps to exactly one participant/layer, so the diagram IS the layer contract.
    let saga_blocks: Vec<String> = pm_sequence_blocks(model);

    let mut out: Vec<String> = vec![
        "<!-- GENERATED by tools/codegen — do not edit by hand. Source: specs/architecture/c4-*.yaml. -->".into(),
        "# Captain.Food — C4 diagrams (Mermaid, generated)".into(),
        "".into(),
        "Rendered by any Mermaid-aware viewer (GitHub, VS Code, mermaid.live). The authoritative source is".into(),
        "`specs/architecture/c4-l2.yaml` / `c4-l3.yaml`; regenerate with `make generate`.".into(),
        "".into(),
        "## L2 — Containers & external systems".into(),
        "".into(),
        "```mermaid".into(),
        container.join("\n"),
        "```".into(),
        "".into(),
        "## Domain — bounded contexts → aggregates → read models".into(),
        "".into(),
        "Each aggregate links to the `View_*` read models its emitted events project into.".into(),
        "".into(),
        "```mermaid".into(),
        domain.join("\n"),
        "```".into(),
        "".into(),
        "## Saga sequences — message → emitted events, in order".into(),
        "".into(),
        "Each process manager (saga) as a time-ordered sequence: the command/event it receives and the".into(),
        "events it emits in response (derived from `actors.yaml`).".into(),
        "".into(),
    ];
    out.extend(saga_blocks);
    out.join("\n")
}

// ─── schema.generated.graphql (port of emit/schema.ts) ──────────────────────────────────────────

struct ApiField {
    name: String,
    ty: String,
    is_ref: bool,
    required: bool,
    nullable: bool,
    array: bool,
    format: Option<String>,
    description: Option<String>,
}
struct ApiType {
    name: String,
    description: Option<String>,
    reads: Vec<String>,
    properties: Vec<ApiField>,
}
struct ApiQuery {
    name: String,
    description: Option<String>,
    args: Vec<ApiField>,
    returns_type: String,
    returns_list: bool,
    returns_nullable: bool,
    reads: Vec<String>,
    roles: Vec<String>,
    slice: String,
}
struct ApiMutation {
    name: String,
    description: Option<String>,
    command: String,
    roles: Vec<String>,
    slice: String,
    payload: Vec<ApiField>,
}
struct Api {
    types: Vec<ApiType>,
    queries: Vec<ApiQuery>,
    mutations: Vec<ApiMutation>,
    subscriptions: Vec<ApiQuery>,
}

const DIRECTIVES: &str = "directive @auth(requires: [UserType!]!) on FIELD_DEFINITION\ndirective @public on FIELD_DEFINITION\ndirective @command(name: String!) on FIELD_DEFINITION\ndirective @reads(views: [String!]!) on FIELD_DEFINITION";

fn pascal(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
fn camel(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// refOrName: the LAST `/`-segment of a `$ref` (object or string) or a bare type string.
fn ref_or_name(v: &Value) -> String {
    if let Some(r) = v.get("$ref").and_then(|x| x.as_str()) {
        return r.rsplit('/').next().unwrap_or("").to_string();
    }
    if let Some(s) = v.as_str() {
        return s.rsplit('/').next().unwrap_or("").to_string();
    }
    String::new()
}
fn name_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_sequence())
        .map(|s| s.iter().map(ref_or_name).filter(|r| !r.is_empty()).collect())
        .unwrap_or_default()
}
fn string_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_sequence())
        .map(|s| s.iter().filter_map(|i| i.as_str().map(|x| x.to_string())).collect())
        .unwrap_or_default()
}

fn parse_field(name: &str, n: &Value) -> ApiField {
    let is_ref = n.get("$ref").and_then(|x| x.as_str()).is_some();
    let ty = if is_ref {
        ref_or_name(n)
    } else {
        n.get("type").and_then(|x| x.as_str()).unwrap_or("").to_string()
    };
    let flag = |k: &str| n.get(k).and_then(|x| x.as_bool()) == Some(true);
    ApiField {
        name: name.to_string(),
        ty,
        is_ref,
        required: flag("required"),
        nullable: flag("nullable"),
        array: flag("array"),
        format: n.get("format").and_then(|x| x.as_str()).map(|s| s.to_string()),
        description: n.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()),
    }
}
fn field_map(v: Option<&Value>) -> Vec<ApiField> {
    match v.and_then(|x| x.as_mapping()) {
        Some(m) => m.iter().filter_map(|(k, node)| k.as_str().map(|name| parse_field(name, node))).collect(),
        None => vec![],
    }
}

fn parse_api(model: &Model) -> Api {
    let sect = |k: &str| model.defs.get("api.yaml").and_then(|v| v.get(k)).and_then(|v| v.as_mapping());
    let mut types = Vec::new();
    if let Some(m) = sect("types") {
        for (k, t) in m {
            if let Some(name) = k.as_str() {
                types.push(ApiType { name: name.into(), description: t.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()), reads: name_list(t.get("reads")), properties: field_map(t.get("properties")) });
            }
        }
    }
    let reads_by_type: HashMap<String, Vec<String>> = types.iter().map(|t| (t.name.clone(), t.reads.clone())).collect();
    let parse_query = |name: &str, q: &Value, with_reads: bool| -> ApiQuery {
        let returns = q.get("returns");
        let rt = returns.and_then(|r| r.get("$ref")).or_else(|| returns.and_then(|r| r.get("type")));
        let returns_type = rt.map(ref_or_name).unwrap_or_default();
        let reads = if with_reads {
            reads_by_type.get(&returns_type).cloned().unwrap_or_default()
        } else {
            vec![]
        };
        ApiQuery {
            name: name.into(),
            description: q.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()),
            args: field_map(q.get("args")),
            returns_type,
            returns_list: returns.and_then(|r| r.get("array")).and_then(|x| x.as_bool()) == Some(true),
            returns_nullable: returns.and_then(|r| r.get("nullable")).and_then(|x| x.as_bool()) == Some(true),
            reads,
            roles: string_list(q.get("roles")),
            slice: q.get("slice").and_then(|x| x.as_str()).unwrap_or("V0").to_string(),
        }
    };
    let mut queries = Vec::new();
    if let Some(m) = sect("queries") {
        for (k, q) in m {
            if let Some(n) = k.as_str() {
                queries.push(parse_query(n, q, true));
            }
        }
    }
    let mut subscriptions = Vec::new();
    if let Some(m) = sect("subscriptions") {
        for (k, q) in m {
            if let Some(n) = k.as_str() {
                subscriptions.push(parse_query(n, q, false));
            }
        }
    }
    let mut mutations = Vec::new();
    if let Some(m) = sect("mutations") {
        for (k, mu) in m {
            if let Some(n) = k.as_str() {
                mutations.push(ApiMutation {
                    name: n.into(),
                    description: mu.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()),
                    command: mu.get("command").map(ref_or_name).unwrap_or_default(),
                    roles: string_list(mu.get("roles")),
                    slice: mu.get("slice").and_then(|x| x.as_str()).unwrap_or("V0").to_string(),
                    payload: field_map(mu.get("payload")),
                });
            }
        }
    }
    Api { types, queries, mutations, subscriptions }
}

fn inline_primitive(t: &str, format: Option<&str>) -> String {
    match t {
        "integer" => "Int".into(),
        "boolean" => "Boolean".into(),
        "string" => if format == Some("date-time") { "DateTime".into() } else { "String".into() },
        _ => "String".into(),
    }
}

fn ref_target_file(r: &str, ctx: &str) -> Option<String> {
    let pr = parse_ref(r)?;
    let file = if pr.file.is_empty() { ctx.to_string() } else { pr.file };
    if is_source_file(&file) { Some(file) } else { None }
}

fn base_type(model: &Model, node: &Value, ctx: &str, input: bool) -> String {
    if let Some(rf) = node.get("$ref").and_then(|x| x.as_str()) {
        let file = ref_target_file(rf, ctx);
        let name = parse_ref(rf).and_then(|p| p.path.into_iter().next()).unwrap_or_else(|| "String".into());
        if file.as_deref() == Some("scalars.yaml") {
            return name;
        }
        return if input { format!("{}Input", name) } else { name };
    }
    if node.get("type").and_then(|x| x.as_str()) == Some("array") {
        if let Some(items) = node.get("items") {
            return format!("[{}!]", base_type(model, items, ctx, input));
        }
    }
    inline_primitive(
        node.get("type").and_then(|x| x.as_str()).unwrap_or("string"),
        node.get("format").and_then(|x| x.as_str()),
    )
}

fn object_fields(model: &Model, def: &Value, ctx: &str, input: bool) -> Vec<String> {
    let props = match def.get("properties").and_then(|p| p.as_mapping()) {
        Some(m) => m,
        None => return vec![],
    };
    let required: HashSet<&str> = def
        .get("required")
        .and_then(|r| r.as_sequence())
        .map(|s| s.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    let mut out = Vec::new();
    for (k, p) in props {
        let name = match k.as_str() {
            Some(s) => s,
            None => continue,
        };
        if input && p.get("readOnly").and_then(|x| x.as_bool()) == Some(true) {
            continue;
        }
        let base = base_type(model, p, ctx, input);
        let non_null = if input {
            required.contains(name)
        } else {
            p.get("nullable").and_then(|x| x.as_bool()) != Some(true)
        };
        out.push(format!("  {}: {}{}", name, base, if non_null { "!" } else { "" }));
    }
    out
}

fn scalar_names(model: &Model) -> HashSet<String> {
    model
        .defs
        .get("scalars.yaml")
        .and_then(|v| v.as_mapping())
        .map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

fn api_field_type(model: &Model, f: &ApiField, input: bool) -> String {
    let mut base = if f.is_ref {
        if input && !scalar_names(model).contains(&f.ty) {
            format!("{}Input", f.ty)
        } else {
            f.ty.clone()
        }
    } else {
        inline_primitive(&f.ty, f.format.as_deref())
    };
    if f.array {
        base = format!("[{}!]", base);
    }
    let non_null = if input { f.required } else { !f.nullable };
    format!("{}{}", base, if non_null { "!" } else { "" })
}

fn scalars_block(model: &Model) -> String {
    let mut lines = vec!["scalar DateTime".to_string()];
    if let Some(m) = model.defs.get("scalars.yaml").and_then(|v| v.as_mapping()) {
        for (k, def) in m {
            if let Some(name) = k.as_str() {
                if !def.get("enum").map(|e| e.is_sequence()).unwrap_or(false) {
                    lines.push(format!("scalar {}", name));
                }
            }
        }
    }
    lines.join("\n")
}

fn enums_block(model: &Model) -> String {
    let mut blocks = Vec::new();
    if let Some(m) = model.defs.get("scalars.yaml").and_then(|v| v.as_mapping()) {
        for (k, def) in m {
            if let (Some(name), Some(vals)) = (k.as_str(), def.get("enum").and_then(|e| e.as_sequence())) {
                let body: Vec<String> = vals.iter().map(|v| format!("  {}", v.as_str().unwrap_or(""))).collect();
                blocks.push(format!("enum {} {{\n{}\n}}", name, body.join("\n")));
            }
        }
    }
    blocks.join("\n\n")
}

/// One FK-derived navigation field on an output type (shared by the SDL emitter and the server
/// async-graphql emitter, so the two can never drift).
struct NavField {
    field: String,
    target: String,
    list: bool,
    nullable: bool,
}

fn nav_add(
    entity: &str,
    nf: NavField,
    entity_names: &HashSet<String>,
    seen: &mut HashMap<String, HashSet<String>>,
    out: &mut HashMap<String, Vec<NavField>>,
) {
    if !entity_names.contains(entity) {
        return;
    }
    let s = seen.entry(entity.to_string()).or_default();
    if s.contains(&nf.field) {
        return;
    }
    s.insert(nf.field.clone());
    out.entry(entity.to_string()).or_default().push(nf);
}

/// FK-derived navigation fields per entity, structured (views.yaml foreign keys → `src.tgt` single
/// navigation + `tgt.srcs` reverse collection).
fn nav_fields(views: &[SqlView], entity_names: &HashSet<String>) -> HashMap<String, Vec<NavField>> {
    let view_agg: HashMap<String, String> = views.iter().map(|v| (v.name.clone(), v.aggregate.clone())).collect();
    let mut seen: HashMap<String, HashSet<String>> = HashMap::new();
    let mut out: HashMap<String, Vec<NavField>> = HashMap::new();
    for v in views {
        for col in &v.columns {
            let fk = match &col.fk {
                Some(f) => f,
                None => continue,
            };
            let target_view = fk.split('.').next().unwrap_or("");
            let tgt = match view_agg.get(target_view) {
                Some(t) if !t.is_empty() => t.clone(),
                _ => continue,
            };
            let src = v.aggregate.clone();
            if entity_names.contains(&tgt) {
                nav_add(&src, NavField { field: camel(&tgt), target: tgt.clone(), list: false, nullable: col.nullable }, entity_names, &mut seen, &mut out);
                nav_add(&tgt, NavField { field: format!("{}s", camel(&src)), target: src.clone(), list: true, nullable: false }, entity_names, &mut seen, &mut out);
            }
        }
    }
    out
}

fn nav_by_entity(views: &[SqlView], entity_names: &HashSet<String>) -> HashMap<String, Vec<String>> {
    nav_fields(views, entity_names)
        .into_iter()
        .map(|(entity, nfs)| {
            let lines = nfs
                .into_iter()
                .map(|n| {
                    if n.list {
                        format!("  {}: [{}!]!", n.field, n.target)
                    } else {
                        format!("  {}: {}{}", n.field, n.target, if n.nullable { "" } else { "!" })
                    }
                })
                .collect();
            (entity, lines)
        })
        .collect()
}

fn output_types_block(model: &Model, views: &[SqlView], api: &Api) -> String {
    let registered: HashSet<String> = api.types.iter().map(|t| t.name.clone()).collect();
    let nav = nav_by_entity(views, &registered);
    let mut blocks = Vec::new();
    if let Some(m) = model.defs.get("entities.yaml").and_then(|v| v.as_mapping()) {
        for (k, def) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            if registered.contains(name) {
                continue;
            }
            let mut fields = object_fields(model, def, "entities.yaml", false);
            if let Some(nf) = nav.get(name) {
                fields.extend(nf.clone());
            }
            blocks.push(format!("type {} {{\n{}\n}}", name, fields.join("\n")));
        }
    }
    for t in &api.types {
        let mut fields: Vec<String> = t.properties.iter().map(|f| format!("  {}: {}", f.name, api_field_type(model, f, false))).collect();
        if let Some(nf) = nav.get(&t.name) {
            fields.extend(nf.clone());
        }
        blocks.push(format!("type {} {{\n{}\n}}", t.name, fields.join("\n")));
    }
    blocks.join("\n\n")
}

fn visit_inputs(model: &Model, name: &str, file: &str, needed: &mut Vec<(String, String)>, visited: &mut HashSet<String>) {
    let key = format!("{}#{}", file, name);
    if visited.contains(&key) {
        return;
    }
    visited.insert(key);
    let def = match model.defs.get(file).and_then(|d| d.get(name)) {
        Some(d) => d,
        None => return,
    };
    let mut refs = Vec::new();
    collect_refs(def, file, &mut refs);
    for (_loc, r) in refs {
        if let Some(tf) = ref_target_file(&r, file) {
            let rn = parse_ref(&r).and_then(|p| p.path.into_iter().next());
            if tf != "scalars.yaml" {
                if let Some(rn) = rn {
                    needed.push((rn.clone(), tf.clone()));
                    visit_inputs(model, &rn, &tf, needed, visited);
                }
            }
        }
    }
}

fn input_types_block(model: &Model, api: &Api) -> String {
    let mut needed: Vec<(String, String)> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();

    let mut command_inputs = Vec::new();
    for m in &api.mutations {
        if let Some(def) = model.defs.get("commands.yaml").and_then(|d| d.get(&m.command)) {
            command_inputs.push(format!("input {}Input {{\n{}\n}}", m.command, object_fields(model, def, "commands.yaml", true).join("\n")));
            visit_inputs(model, &m.command, "commands.yaml", &mut needed, &mut visited);
        }
    }

    let scalars = scalar_names(model);
    let mut query_inputs = Vec::new();
    for q in &api.queries {
        if q.args.is_empty() {
            continue;
        }
        let fields: Vec<String> = q.args.iter().map(|a| format!("  {}: {}", a.name, api_field_type(model, a, true))).collect();
        query_inputs.push(format!("input {}QueryInput {{\n{}\n}}", pascal(&q.name), fields.join("\n")));
        for a in &q.args {
            if a.is_ref && !scalars.contains(&a.ty) {
                visit_inputs(model, &a.ty, "entities.yaml", &mut needed, &mut visited);
            }
        }
    }

    let mut subscription_inputs = Vec::new();
    for s in &api.subscriptions {
        if s.args.is_empty() {
            continue;
        }
        let fields: Vec<String> = s.args.iter().map(|a| format!("  {}: {}", a.name, api_field_type(model, a, true))).collect();
        subscription_inputs.push(format!("input {}SubscriptionInput {{\n{}\n}}", pascal(&s.name), fields.join("\n")));
        for a in &s.args {
            if a.is_ref && !scalars.contains(&a.ty) {
                visit_inputs(model, &a.ty, "entities.yaml", &mut needed, &mut visited);
            }
        }
    }

    let mut emitted: HashSet<String> = HashSet::new();
    let mut object_inputs = Vec::new();
    for (name, file) in &needed {
        if emitted.contains(name) {
            continue;
        }
        emitted.insert(name.clone());
        if let Some(def) = model.defs.get(file).and_then(|d| d.get(name)) {
            object_inputs.push(format!("input {}Input {{\n{}\n}}", name, object_fields(model, def, file, true).join("\n")));
        }
    }

    let mut all = command_inputs;
    all.extend(query_inputs);
    all.extend(subscription_inputs);
    all.extend(object_inputs);
    all.join("\n\n")
}

fn payloads_block(model: &Model, api: &Api) -> String {
    api.mutations
        .iter()
        .map(|m| {
            let mut fields = vec!["  correlationId: CorrelationId!".to_string()];
            for f in &m.payload {
                fields.push(format!("  {}: {}", f.name, api_field_type(model, f, false)));
            }
            format!("type {}Payload {{\n{}\n}}", pascal(&m.name), fields.join("\n"))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn auth_directive(roles: &[String]) -> String {
    if roles.iter().any(|r| r == "PUBLIC") {
        "@public".to_string()
    } else {
        format!("@auth(requires: [{}])", roles.join(", "))
    }
}

fn query_block(api: &Api) -> String {
    let fields: Vec<String> = api
        .queries
        .iter()
        .map(|q| {
            let arg_str = if q.args.is_empty() {
                String::new()
            } else {
                format!("(input: {}QueryInput{})", pascal(&q.name), if q.args.iter().any(|a| a.required) { "!" } else { "" })
            };
            let inner = if q.returns_list { format!("[{}!]", q.returns_type) } else { q.returns_type.clone() };
            let ret = format!("{}{}", inner, if q.returns_nullable { "" } else { "!" });
            let reads = if q.reads.is_empty() {
                String::new()
            } else {
                format!(" @reads(views: [{}])", q.reads.iter().map(|v| format!("\"{}\"", v)).collect::<Vec<_>>().join(", "))
            };
            format!("  {}{}: {} {}{}", q.name, arg_str, ret, auth_directive(&q.roles), reads)
        })
        .collect();
    format!("type Query {{\n{}\n}}", fields.join("\n"))
}

fn mutation_block(api: &Api) -> String {
    let fields: Vec<String> = api
        .mutations
        .iter()
        .map(|m| {
            format!(
                "  {}(input: {}Input!): {}Payload! {} @command(name: \"{}\")",
                m.name, m.command, pascal(&m.name), auth_directive(&m.roles), m.command
            )
        })
        .collect();
    format!("type Mutation {{\n{}\n}}", fields.join("\n"))
}

fn subscription_block(api: &Api) -> String {
    let fields: Vec<String> = api
        .subscriptions
        .iter()
        .map(|s| {
            let arg_str = if s.args.is_empty() {
                String::new()
            } else {
                format!("(input: {}SubscriptionInput{})", pascal(&s.name), if s.args.iter().any(|a| a.required) { "!" } else { "" })
            };
            let inner = if s.returns_list { format!("[{}!]", s.returns_type) } else { s.returns_type.clone() };
            let ret = format!("{}{}", inner, if s.returns_nullable { "" } else { "!" });
            format!("  {}{}: {} {}", s.name, arg_str, ret, auth_directive(&s.roles))
        })
        .collect();
    format!("type Subscription {{\n{}\n}}", fields.join("\n"))
}

fn header(title: &str) -> String {
    let bar = "=".repeat(78);
    format!("# {}\n# {}\n# {}", bar, title, bar)
}

fn emit_schema(model: &Model) -> String {
    let api = parse_api(model);
    let views = parse_views(model);
    let mut s = String::new();
    s.push_str("# GENERATED by tools/codegen from specs/api.yaml (+ scalars/entities/commands/views) — do not edit by hand.\n");
    s.push_str("# Strong typing: one scalars.yaml type = one GraphQL scalar/enum. Navigation fields on output types\n");
    s.push_str("# are derived from views.yaml foreign keys. Mutations return <Name>Payload (always carrying correlationId).\n\n");
    s.push_str(&header("Custom scalars"));
    s.push('\n');
    s.push_str(&scalars_block(model));
    s.push_str("\n\n");
    s.push_str(&header("Enums"));
    s.push('\n');
    s.push_str(&enums_block(model));
    s.push_str("\n\n");
    s.push_str(&header("Directives — ACL (@auth/@public) + declared links (@command/@reads)"));
    s.push('\n');
    s.push_str(DIRECTIVES);
    s.push_str("\n\n");
    s.push_str(&header("Output types (entities.yaml + FK-derived navigation + projections)"));
    s.push('\n');
    s.push_str(&output_types_block(model, &views, &api));
    s.push_str("\n\n");
    s.push_str(&header("Input types (mutation command payloads + query args)"));
    s.push('\n');
    s.push_str(&input_types_block(model, &api));
    s.push_str("\n\n");
    s.push_str(&header("Mutation payloads"));
    s.push('\n');
    s.push_str(&payloads_block(model, &api));
    s.push_str("\n\n");
    s.push_str(&header("Queries — read side"));
    s.push('\n');
    s.push_str(&query_block(&api));
    s.push_str("\n\n");
    s.push_str(&header("Mutations — write side"));
    s.push('\n');
    s.push_str(&mutation_block(&api));
    if !api.subscriptions.is_empty() {
        s.push_str("\n\n"); // template line break + the conditional's leading newline
        s.push_str(&header("Subscriptions — streams"));
        s.push('\n');
        s.push_str(&subscription_block(&api));
        s.push('\n');
    }
    s
}

// ─── documentation.generated.md (port of emit/documentation.ts) ─────────────────────────────────

fn d_emo(kind: &str) -> &'static str {
    match kind {
        "scalar" => "🔤", "entity" => "📦", "command" => "📩", "event" => "⚡", "view" => "🗄️",
        "actor" => "🎭", "type" => "🧩", "query" => "🔎", "mutation" => "✏️", "error" => "⛔",
        "property" => "🔹", "story" => "🎬", "activity" => "🧭", "test" => "🧪", "obs" => "📡",
        "context" => "🔲", "container" => "🧱", "component" => "⚙️", "subscription" => "🔔",
        "rule" => "📐", "screen" => "📱", "translation" => "🌐", _ => "•",
    }
}
fn user_emo(role: &str) -> &'static str {
    match role {
        "PUBLIC" => "🌐", "CUSTOMER" => "🙋", "RESTAURANT_ACCOUNT" => "🏪", "RESTAURANT" => "🍽️",
        "RIDER" => "🛵", "ADMIN" => "🛠️", "EXTERNAL" => "🔌", _ => "❔",
    }
}
fn dslug(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in s.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out
}
fn danchor(kind: &str, name: &str) -> String {
    format!("{}-{}", kind, dslug(name))
}
fn dprop_anchor(kind: &str, owner: &str, field: &str) -> String {
    format!("{}--{}", danchor(kind, owner), dslug(field))
}
fn id_tag(id: &str) -> String {
    format!("<a id=\"{}\"></a>", id)
}
fn dlink(kind: &str, name: &str) -> String {
    format!("[{} `{}`](#{})", d_emo(kind), name, danchor(kind, name))
}
fn dprop_link(kind: &str, owner: &str, field: &str) -> String {
    format!("[{} `{}`.`{}`](#{})", d_emo(kind), owner, field, dprop_anchor(kind, owner, field))
}
fn item_head(kind: &str, label: &str, name: &str) -> String {
    format!("{}\n#### {} {}: `{}`", id_tag(&danchor(kind, name)), d_emo(kind), label, name)
}
/// Collapse whitespace runs to a single space (no trim) — JS `.replace(/\s+/g,' ')`.
fn ws1(s: &str) -> String {
    let mut o = String::new();
    let mut p = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !p {
                o.push(' ');
                p = true;
            }
        } else {
            o.push(c);
            p = false;
        }
    }
    o
}
fn push_uniq(m: &mut HashMap<String, Vec<String>>, k: &str, v: &str) {
    let e = m.entry(k.to_string()).or_default();
    if !e.iter().any(|x| x == v) {
        e.push(v.to_string());
    }
}

fn ref_label(rf: &str) -> String {
    let mut it = rf.splitn(2, "#/");
    let file = it.next().unwrap_or("");
    let name = it.next().unwrap_or("");
    if file == "scalars.yaml" {
        dlink("scalar", name)
    } else {
        dlink("entity", name)
    }
}
fn raw_type(p: &Value) -> String {
    if let Some(rf) = p.get("$ref").and_then(|x| x.as_str()) {
        return ref_label(rf);
    }
    if p.get("type").and_then(|x| x.as_str()) == Some("array") {
        if let Some(items) = p.get("items") {
            return format!("[{}]", raw_type(items));
        }
    }
    let mut t = format!("`{}`", p.get("type").and_then(|x| x.as_str()).unwrap_or("?"));
    if let Some(en) = p.get("enum").and_then(|x| x.as_sequence()) {
        t += &format!(" ({})", en.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" \\| "));
    }
    if let Some(fmt) = p.get("format").and_then(|x| x.as_str()) {
        t += &format!(" _{}_", fmt);
    }
    t
}

fn doc_desc(model: &Model, file: &str, name: &str) -> String {
    let d = model.defs.get(file).and_then(|m| m.get(name)).and_then(|n| n.get("description")).and_then(|x| x.as_str()).unwrap_or("");
    ws1(d.trim())
}

struct Doc {
    ctx: String,
    md: String,
}
struct DRow {
    ctx: String,
    cells: Vec<String>,
}

fn emit_documentation(model: &Model) -> String {
    let api = parse_api(model);
    let actors = parse_actors(model);
    let views = parse_views(model);
    let personas = parse_stories(model);
    let cx = build_context_map(model, &api, &actors, &views);

    let scalar_set = scalar_names(model);
    let entity_set: HashSet<String> = model.defs.get("entities.yaml").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let type_set: HashSet<String> = api.types.iter().map(|t| t.name.clone()).collect();

    let api_type_md = |f: &ApiField| -> String {
        let base = if f.is_ref {
            if scalar_set.contains(&f.ty) {
                dlink("scalar", &f.ty)
            } else if type_set.contains(&f.ty) {
                dlink("type", &f.ty)
            } else if entity_set.contains(&f.ty) {
                dlink("entity", &f.ty)
            } else {
                format!("`{}`", f.ty)
            }
        } else {
            format!("`{}`{}", f.ty, f.format.as_deref().map(|fmt| format!(" _{}_", fmt)).unwrap_or_default())
        };
        if f.array {
            format!("[{}]", base)
        } else {
            base
        }
    };
    let prop_rows = |def: &Value, kind: &str, owner: &str| -> Vec<Vec<String>> {
        let props = match def.get("properties").and_then(|x| x.as_mapping()) {
            Some(m) => m,
            None => return vec![],
        };
        let required: HashSet<&str> = def.get("required").and_then(|x| x.as_sequence()).map(|s| s.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
        let mut rows = Vec::new();
        for (k, p) in props {
            let n = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            let req = if required.contains(n) { "✅" } else { "⬜" };
            let d = p.get("description").and_then(|x| x.as_str()).unwrap_or("");
            rows.push(vec![format!("{}`{}`", id_tag(&dprop_anchor(kind, owner, n)), n), raw_type(p), req.to_string(), ws1(d)]);
        }
        rows
    };

    // relationship indexes
    let mut cmd_handler: HashMap<String, (String, Vec<String>, Vec<String>)> = HashMap::new();
    let mut evt_emitted_by: HashMap<String, Vec<String>> = HashMap::new();
    let mut evt_consumed_by: HashMap<String, Vec<String>> = HashMap::new();
    let mut err_thrown_by: HashMap<String, Vec<String>> = HashMap::new();
    for a in &actors {
        for e in &a.receives {
            let msg = ref_name(&e.message_ref);
            let emits: Vec<String> = e.emits.iter().filter_map(|r| ref_name(r)).collect();
            let throws: Vec<String> = e.throws.iter().filter_map(|r| ref_name(r)).collect();
            if e.message_ref.starts_with("commands.yaml#/") {
                if let Some(m) = &msg {
                    cmd_handler.insert(m.clone(), (a.name.clone(), emits.clone(), throws.clone()));
                    for er in &throws {
                        push_uniq(&mut err_thrown_by, er, m);
                    }
                }
            } else if e.message_ref.starts_with("events.yaml#/") {
                if let Some(m) = &msg {
                    push_uniq(&mut evt_consumed_by, m, &a.name);
                }
            }
            for ev in &emits {
                push_uniq(&mut evt_emitted_by, ev, &a.name);
            }
        }
    }
    let mut evt_views: HashMap<String, Vec<String>> = HashMap::new();
    for v in &views {
        for e in &v.fedby {
            push_uniq(&mut evt_views, e, &v.name);
        }
    }
    let mut mut_by_command: HashMap<String, String> = HashMap::new();
    for m in &api.mutations {
        mut_by_command.insert(m.command.clone(), m.name.clone());
    }

    // 1. STORIES
    let stories_section = personas.iter().map(|p| {
        let badge = format!("{} `{}`{}", user_emo(&p.role), p.role, p.locale.as_deref().map(|l| format!(" · 🗣️ `{}`", l)).unwrap_or_default());
        let mut rows: Vec<Vec<String>> = Vec::new();
        for act in &p.activities {
            for (i, step) in act.steps.iter().enumerate() {
                let op = if let (Some(op), Some(kind)) = (&step.op, &step.op_kind) {
                    dlink(kind, op)
                } else if let Some(note) = &step.note {
                    format!("📝 {}", note)
                } else {
                    "—".to_string()
                };
                rows.push(vec![if i == 0 { format!("{} **{}**", d_emo("activity"), act.name) } else { String::new() }, step.name.clone(), op]);
            }
        }
        format!(
            "{}\n### {} `{}` · {}\n{}\n{}",
            id_tag(&danchor("story", &p.name)),
            d_emo("story"),
            p.name,
            badge,
            p.description.as_deref().map(|d| format!("\n{}\n", d)).unwrap_or_default(),
            md_table(&["Activity", "Step", "Operation"], &rows)
        )
    }).collect::<Vec<_>>().join("\n\n");

    // 2. API operations
    let mut api_docs: Vec<Doc> = Vec::new();
    for q in &api.queries {
        let field_list = q.args.iter().map(|a| format!("`{}{}`: {}", a.name, if a.required { "" } else { "?" }, api_type_md(a))).collect::<Vec<_>>().join(", ");
        let input = if q.args.is_empty() {
            "- **Input**: _(none)_".to_string()
        } else {
            format!("- **Input**: 🧩 `{}QueryInput{}` — {}", pascal(&q.name), if q.args.iter().any(|a| a.required) { "!" } else { "" }, field_list)
        };
        let ret = format!(
            "{}{}",
            if type_set.contains(&q.returns_type) || entity_set.contains(&q.returns_type) {
                dlink(if type_set.contains(&q.returns_type) { "type" } else { "entity" }, &q.returns_type)
            } else {
                format!("`{}`", q.returns_type)
            },
            if q.returns_list { " (list)" } else { "" }
        );
        let reads = if q.reads.is_empty() { "—".to_string() } else { q.reads.iter().map(|v| dlink("view", v)).collect::<Vec<_>>().join(", ") };
        let ctx = cx.of_operation(&q.roles, &(if !q.reads.is_empty() { cx.of_reads(&q.reads) } else { cx.of_type(&q.returns_type) }));
        api_docs.push(Doc { ctx, md: vec![
            item_head("query", "Query", &q.name),
            q.description.as_deref().map(|d| format!("\n{}\n", d)).unwrap_or_default(),
            input,
            format!("- **Returns**: {} · **reads** {}", ret, reads),
            format!("- **Roles**: {} · **slice** {}", q.roles.join(", "), q.slice),
        ].join("\n") });
    }
    for m in &api.mutations {
        let payload = m.payload.iter().map(|f| format!("`{}`: {}", f.name, api_type_md(f))).collect::<Vec<_>>().join(", ");
        let handler = cmd_handler.get(&m.command);
        api_docs.push(Doc { ctx: cx.of_command(&m.command), md: vec![
            item_head("mutation", "Mutation", &m.name),
            format!("\n- **Command**: {}{}", dlink("command", &m.command), handler.map(|h| format!(" → handled by {}", dlink("actor", &h.0))).unwrap_or_default()),
            format!("- **Roles**: {} · **slice** {}", m.roles.join(", "), m.slice),
            format!("- **Payload**: correlationId{}", if payload.is_empty() { String::new() } else { format!(", {}", payload) }),
        ].join("\n") });
    }
    for s in &api.subscriptions {
        let field_list = s.args.iter().map(|a| format!("`{}{}`: {}", a.name, if a.required { "" } else { "?" }, api_type_md(a))).collect::<Vec<_>>().join(", ");
        let input = if s.args.is_empty() {
            "- **Input**: _(none)_".to_string()
        } else {
            format!("- **Input**: 🧩 `{}SubscriptionInput{}` — {}", pascal(&s.name), if s.args.iter().any(|a| a.required) { "!" } else { "" }, field_list)
        };
        let ret = format!(
            "{}{}",
            if type_set.contains(&s.returns_type) || entity_set.contains(&s.returns_type) {
                dlink(if type_set.contains(&s.returns_type) { "type" } else { "entity" }, &s.returns_type)
            } else {
                format!("`{}`", s.returns_type)
            },
            if s.returns_list { " (list)" } else { "" }
        );
        api_docs.push(Doc { ctx: cx.of_operation(&s.roles, &cx.of_type(&s.returns_type)), md: vec![
            format!("{}\n#### {} Subscription: [`{}`](#{})", id_tag(&danchor("subscription", &s.name)), d_emo("subscription"), s.name, danchor("subscription", &s.name)),
            s.description.as_deref().map(|d| format!("\n{}\n", d)).unwrap_or_default(),
            input,
            format!("- **Streams**: {}", ret),
            format!("- **Roles**: {} · **slice** {}", s.roles.join(", "), s.slice),
        ].join("\n") });
    }

    // typeDocs
    let type_docs: Vec<Doc> = api.types.iter().map(|t| {
        let reads = t.reads.iter().map(|v| dlink("view", v)).collect::<Vec<_>>().join(", ");
        let rows: Vec<Vec<String>> = t.properties.iter().map(|f| vec![format!("{}`{}`", id_tag(&dprop_anchor("type", &t.name, &f.name)), f.name), api_type_md(f), if f.nullable { "⬜".into() } else { "✅".into() }]).collect();
        Doc { ctx: cx.of_type(&t.name), md: vec![
            item_head("type", "Type", &t.name),
            t.description.as_deref().map(|d| format!("\n{}\n", d)).unwrap_or_default(),
            if reads.is_empty() { "- **Read model**: _(resolved within a parent projection)_".to_string() } else { format!("- **Read model**: {}", reads) },
            if rows.is_empty() { String::new() } else { format!("\n{}", md_table(&["Field", "Type", "Required"], &rows)) },
        ].join("\n") }
    }).collect();

    // actorDocs — process managers also embed their saga sequence diagram (typed steps).
    let pm_seq: HashMap<String, String> = pm_sequence_map(model).into_iter().collect();
    let actor_docs: Vec<Doc> = actors.iter().map(|a| {
        let rows: Vec<Vec<String>> = a.receives.iter().map(|e| {
            let msg_name = ref_name(&e.message_ref).unwrap_or_else(|| "?".to_string());
            let is_cmd = e.message_ref.starts_with("commands.yaml#/");
            let msg = dlink(if is_cmd { "command" } else { "event" }, &msg_name);
            let emits = {
                let s = e.emits.iter().map(|r| dlink("event", &ref_name(r).unwrap_or_default())).collect::<Vec<_>>().join(", ");
                if s.is_empty() { e.effect.as_deref().map(|x| format!("_{}_", x)).unwrap_or_else(|| "—".to_string()) } else { s }
            };
            let throws = {
                let s = e.throws.iter().map(|r| dlink("error", &ref_name(r).unwrap_or_default())).collect::<Vec<_>>().join(", ");
                if s.is_empty() { "—".to_string() } else { s }
            };
            vec![msg, emits, throws]
        }).collect();
        let kind = if a.kind == "aggregate" { "🧩 aggregate" } else { "⚙️ process manager" };
        let mut parts = vec![
            item_head("actor", "Actor", &a.name),
            format!("\n_{}_{}\n", kind, a.description.as_deref().map(|d| format!(" — {}", d)).unwrap_or_default()),
            md_table(&["Receives", "Emits →", "Throws"], &rows),
        ];
        if a.kind != "aggregate" {
            if let Some(d) = pm_seq.get(&a.name) {
                parts.push(format!("\nSequence (generated from the typed steps):\n\n```mermaid\n{}\n```", d));
            }
        }
        Doc { ctx: cx.of_actor(&a.name), md: parts.join("\n") }
    }).collect();

    // 4. VIEWS
    let view_docs: Vec<Doc> = views.iter().map(|v| {
        let slice = if v.slice == "V1" { "🔭 V1" } else { "🛶 V0" };
        let fed_by = { let s = v.fedby.iter().map(|n| dlink("event", n)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
        let cols: Vec<Vec<String>> = v.columns.iter().map(|c| {
            let flags = { let f: Vec<&str> = [(c.pk, "PK"), (c.unique, "unique"), (c.index, "index"), (c.nullable, "nullable")].iter().filter(|(b, _)| *b).map(|(_, s)| *s).collect(); if f.is_empty() { "—".to_string() } else { f.join(", ") } };
            let fk = c.fk.as_ref().map(|f| format!(" → {}", dlink("view", f.split('.').next().unwrap_or(f)))).unwrap_or_default();
            let type_cell = format!("{}{}", if scalar_set.contains(&c.ty) { dlink("scalar", &c.ty) } else { format!("`{}`", if c.ty.is_empty() { "?" } else { &c.ty }) }, if c.type_derived { " _(derived)_" } else { "" });
            let source = { let s = c.from.iter().map(|rf| { let segs: Vec<&str> = rf.splitn(2, "#/").nth(1).unwrap_or("").split('/').filter(|x| !x.is_empty()).collect(); let ev = segs.first().copied().unwrap_or(""); let prop = if segs.get(1) == Some(&"properties") { segs.get(2).copied() } else { None }; match prop { Some(p) => dprop_link("event", ev, p), None => dlink("event", ev) } }).collect::<Vec<_>>().join(", "); if s.is_empty() { "⚠️ _(none)_".to_string() } else { s } };
            vec![format!("`{}`", c.name), format!("{}{}", type_cell, fk), source, flags, ws1(c.note.as_deref().unwrap_or(""))]
        }).collect();
        Doc { ctx: cx.of_view(&v.name), md: [
            item_head("view", "View", &v.name),
            format!("\n- **Source**: {} · {}{}", if v.reference { "📦 reference (static seed)".to_string() } else { dlink("actor", &v.aggregate) }, slice, if v.internal { " · 🔒 internal" } else { "" }),
            v.note.as_deref().map(|n| format!("- **Note**: {}", ws1(n))).unwrap_or_default(),
            if v.filters.is_empty() { String::new() } else { format!("- **Filters**: {}", v.filters.join(" ")) },
            if v.rules.is_empty() { String::new() } else { format!("- **Rules**: {}", v.rules.join(" ")) },
            format!("- **Fed by**: {}", fed_by),
            format!("\n{}", md_table(&["Column", "Type", "Sourced from", "Constraints", "Notes"], &cols)),
        ].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n") }
    }).collect();

    let cmd_map = model.defs.get("commands.yaml").and_then(|v| v.as_mapping());
    let cmd_keys: Vec<String> = cmd_map.map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    // 5. COMMANDS (only those handled by an actor)
    let command_docs: Vec<Doc> = cmd_keys.iter().filter(|c| cmd_handler.contains_key(*c)).map(|c| {
        let h = cmd_handler.get(c).unwrap();
        let mutn = mut_by_command.get(c);
        let def = cmd_map.and_then(|m| m.get(c.as_str())).cloned().unwrap_or(Value::Null);
        let rows = prop_rows(&def, "command", c);
        Doc { ctx: cx.of_command(c), md: vec![
            item_head("command", "Command", c),
            { let d = doc_desc(model, "commands.yaml", c); if d.is_empty() { String::new() } else { format!("\n{}\n", d) } },
            format!("- **Dispatched by**: {} · **handled by** {}", mutn.map(|m| dlink("mutation", m)).unwrap_or_else(|| "—".to_string()), dlink("actor", &h.0)),
            format!("- **Emits**: {}", { let s = h.1.iter().map(|e| dlink("event", e)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } }),
            format!("- **Throws**: {}", { let s = h.2.iter().map(|e| dlink("error", e)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } }),
            if rows.is_empty() { String::new() } else { format!("\n{}", md_table(&["Field", "Type", "Required", "Description"], &rows)) },
        ].join("\n") }
    }).collect();

    // 6. EVENTS
    let non_projected: HashSet<String> = ref_names(model.defs.get("database/projection_views.yaml").and_then(|v| v.get("nonProjectedEvents"))).into_iter().collect();
    let evt_map = model.defs.get("events.yaml").and_then(|v| v.as_mapping());
    let event_docs: Vec<Doc> = evt_map.map(|m| m.iter().filter_map(|(k, _)| k.as_str()).map(|ev| {
        let def = evt_map.and_then(|m| m.get(ev)).cloned().unwrap_or(Value::Null);
        let rows = prop_rows(&def, "event", ev);
        let projected = { let s = evt_views.get(ev).map(|vs| vs.iter().map(|v| dlink("view", v)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if !s.is_empty() { s } else if non_projected.contains(ev) { "_non-projected (saga/transient)_".to_string() } else { "—".to_string() } };
        Doc { ctx: cx.of_event(ev), md: vec![
            item_head("event", "Event", ev),
            { let d = doc_desc(model, "events.yaml", ev); if d.is_empty() { String::new() } else { format!("\n{}\n", d) } },
            format!("- **Emitted by**: {}", { let s = evt_emitted_by.get(ev).map(|a| a.iter().map(|x| dlink("actor", x)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "_inbound / external_".to_string() } else { s } }),
            format!("- **Consumed by**: {}", { let s = evt_consumed_by.get(ev).map(|a| a.iter().map(|x| dlink("actor", x)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "—".to_string() } else { s } }),
            format!("- **Projected into**: {}", projected),
            if rows.is_empty() { String::new() } else { format!("\n{}", md_table(&["Field", "Type", "Required", "Description"], &rows)) },
        ].join("\n") }
    }).collect()).unwrap_or_default();

    // 7. ENTITIES
    let ent_map = model.defs.get("entities.yaml").and_then(|v| v.as_mapping());
    let entity_docs: Vec<Doc> = ent_map.map(|m| m.iter().filter_map(|(k, _)| k.as_str()).map(|e| {
        let def = ent_map.and_then(|m| m.get(e)).cloned().unwrap_or(Value::Null);
        let rows = prop_rows(&def, "entity", e);
        Doc { ctx: cx.of_entity(e), md: vec![
            item_head("entity", "Entity", e),
            { let d = doc_desc(model, "entities.yaml", e); if d.is_empty() { String::new() } else { format!("\n{}\n", d) } },
            if rows.is_empty() { "_(no fields)_".to_string() } else { md_table(&["Field", "Type", "Required", "Description"], &rows) },
        ].join("\n") }
    }).collect()).unwrap_or_default();

    // 8. SCALARS
    let scalar_rows: Vec<DRow> = model.defs.get("scalars.yaml").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, d)| k.as_str().map(|name| {
        let mut t = d.get("type").and_then(|x| x.as_str()).unwrap_or("?").to_string();
        if let Some(en) = d.get("enum").and_then(|x| x.as_sequence()) {
            t = format!("enum ({})", en.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" \\| "));
        } else if let Some(fmt) = d.get("format").and_then(|x| x.as_str()) {
            t += &format!(" _{}_", fmt);
        } else if let Some(pat) = d.get("pattern").and_then(|x| x.as_str()) {
            t += &format!(" `{}`", pat);
        }
        DRow { ctx: cx.of_scalar(name), cells: vec![format!("{}{} `{}`", id_tag(&danchor("scalar", name)), d_emo("scalar"), name), t, ws1(d.get("description").and_then(|x| x.as_str()).unwrap_or(""))] }
    })).collect()).unwrap_or_default();

    // 9. ERRORS
    let error_rows: Vec<DRow> = model.defs.get("errors.yaml").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, d)| k.as_str().map(|name| {
        let msgs = d.get("messages");
        let en = msgs.and_then(|x| x.get("en")).and_then(|x| x.as_str()).unwrap_or("");
        let fr = msgs.and_then(|x| x.get("fr")).and_then(|x| x.as_str()).unwrap_or("");
        let by = { let s = err_thrown_by.get(name).map(|c| c.iter().map(|x| dlink("command", x)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "—".to_string() } else { s } };
        DRow { ctx: cx.of_error(name), cells: vec![format!("{}{} `{}`", id_tag(&danchor("error", name)), d_emo("error"), name), ws1(d.get("description").and_then(|x| x.as_str()).unwrap_or("")), format!("🇬🇧 {}", en), format!("🇫🇷 {}", fr), by] }
    })).collect()).unwrap_or_default();

    // 10a/b. RULES ↔ TESTS
    let rule_defs = model.defs.get("rules.yaml").and_then(|v| v.as_mapping());
    let tests_map = model.defs.get("tests.yaml").and_then(|v| v.get("tests")).and_then(|v| v.as_mapping());
    let fixtures_map = model.defs.get("tests.yaml").and_then(|v| v.get("fixtures")).and_then(|v| v.as_mapping());
    let rules_of_test = |t: &Value| -> Vec<String> { t.get("rules").and_then(|x| x.as_sequence()).map(|s| s.iter().filter_map(|r| r.get("$ref").and_then(|x| x.as_str()).and_then(ref_name)).collect()).unwrap_or_default() };
    let mut rule_tests: HashMap<String, Vec<String>> = HashMap::new();
    let mut test_actor_name: HashMap<String, String> = HashMap::new();
    if let Some(tm) = tests_map {
        for (k, t) in tm {
            if let Some(tn) = k.as_str() {
                test_actor_name.insert(tn.to_string(), ref_name(t.get("actor").and_then(|a| a.get("$ref")).and_then(|x| x.as_str()).unwrap_or("")).unwrap_or_default());
                for rn in rules_of_test(t) {
                    let e = rule_tests.entry(rn).or_default();
                    if !e.contains(&tn.to_string()) { e.push(tn.to_string()); }
                }
            }
        }
    }
    let fx_event = |fx_ref: &str| -> Option<String> {
        let key = fx_ref.rsplit('/').next().unwrap_or("");
        fixtures_map.and_then(|m| m.get(key)).and_then(|fx| fx.get("type")).and_then(|t| t.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name)
    };
    let ev_links = |arr: Option<&Value>| -> String {
        arr.and_then(|x| x.as_sequence()).map(|s| s.iter().map(|it| it.get("$ref").and_then(|x| x.as_str()).and_then(|r| fx_event(r)).map(|e| dlink("event", &e)).unwrap_or_else(|| "—".to_string())).collect::<Vec<_>>().join(", ")).unwrap_or_default()
    };
    // testDocs — per actor
    let test_docs: Vec<Doc> = actors.iter().filter_map(|a| {
        let entries: Vec<(String, Value)> = tests_map.map(|m| m.iter().filter(|(_, t)| ref_name(t.get("actor").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("")).as_deref() == Some(a.name.as_str())).filter_map(|(k, t)| k.as_str().map(|s| (s.to_string(), t.clone()))).collect()).unwrap_or_default();
        if entries.is_empty() { return None; }
        let cases = entries.iter().map(|(name, t)| {
            let cmd = ref_name(t.get("when").and_then(|w| w.get("type")).and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("")).unwrap_or_else(|| "?".to_string());
            let given = { let g = t.get("given"); if g.and_then(|x| x.as_sequence()).map(|s| !s.is_empty()).unwrap_or(false) { ev_links(g) } else { "_(none)_".to_string() } };
            let has_thrown = t.get("thrown").is_some();
            let then_arr = t.get("then");
            let then_line = if has_thrown { String::new() } else { format!("- **Then**: {}", { let s = ev_links(then_arr); if then_arr.and_then(|x| x.as_sequence()).map(|s| !s.is_empty()).unwrap_or(false) { s } else { "∅ _no event (idempotent no-op)_".to_string() } }) };
            let thrown_line = if has_thrown { format!("- **Thrown**: {}", { let s = t.get("thrown").and_then(|x| x.as_sequence()).map(|arr| arr.iter().filter_map(|r| r.get("$ref").and_then(|x| x.as_str()).and_then(ref_name)).map(|e| dlink("error", &e)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "—".to_string() } else { s } }) } else { String::new() };
            let rules = rules_of_test(t).iter().map(|rn| dlink("rule", rn)).collect::<Vec<_>>().join(", ");
            vec![
                format!("{}\n#### {} Test: `{}`", id_tag(&danchor("test", name)), d_emo("test"), name),
                t.get("name").and_then(|x| x.as_str()).map(|n| format!("\n_{}_\n", n)).unwrap_or_default(),
                format!("- **Given**: {}", given),
                format!("- **When**: {}", dlink("command", &cmd)),
                then_line,
                thrown_line,
                if rules.is_empty() { String::new() } else { format!("- **Verifies**: {}", rules) },
            ].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n")
        }).collect::<Vec<_>>().join("\n\n");
        Some(Doc { ctx: cx.of_actor(&a.name), md: format!("**{}**\n\n{}", dlink("actor", &a.name), cases) })
    }).collect();

    let rule_docs: Vec<Doc> = rule_defs.map(|m| m.iter().filter_map(|(k, r)| k.as_str().map(|name| {
        let tns = rule_tests.get(name).cloned().unwrap_or_default();
        let ctx = tns.first().map(|tn| cx.of_actor(test_actor_name.get(tn).map(|s| s.as_str()).unwrap_or(""))).unwrap_or_else(|| CROSS.to_string());
        let verified_by = { let s = tns.iter().map(|tn| dlink("test", tn)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
        Doc { ctx, md: vec![
            format!("{}\n#### {} Rule: `{}`", id_tag(&danchor("rule", name)), d_emo("rule"), name),
            r.get("description").and_then(|x| x.as_str()).map(|d| format!("\n_{}_\n", ws1(d.trim()))).unwrap_or_default(),
            format!("- **Verified by**: {}", verified_by),
        ].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n") }
    })).collect()).unwrap_or_default();

    // 10. OBSERVABILITY
    fn any_link(rf: &str) -> String {
        let mut it = rf.splitn(2, "#/");
        let file = it.next().unwrap_or("");
        let name = it.next().unwrap_or("");
        let kind = match file { "commands.yaml" => "command", "events.yaml" => "event", "actors.yaml" => "actor", "database/projection_views.yaml" => "view", "database/tables/projection_tables.yaml" => "view", "database/tables/referential.yaml" => "view", "scalars.yaml" => "scalar", _ => "entity" };
        dlink(kind, name)
    }
    fn ref_list_links(v: Option<&Value>) -> String {
        let s = v.and_then(|x| x.as_sequence()).map(|arr| arr.iter().filter_map(|it| it.get("$ref").and_then(|r| r.as_str())).map(any_link).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        if s.is_empty() { "—".to_string() } else { s }
    }
    let obs_docs: Vec<Doc> = model.defs.get("observability.yaml").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, c)| k.as_str().map(|feature| {
        let wf = c.get("workflow");
        let id_rows: Vec<Vec<String>> = c.get("run_identity").and_then(|x| x.as_sequence()).map(|s| s.iter().map(|i| vec![format!("`{}`", i.get("name").and_then(|x| x.as_str()).unwrap_or("")), format!("`{}`", i.get("source").and_then(|x| x.as_str()).unwrap_or("")), if i.get("required").and_then(|x| x.as_bool()) == Some(true) { "✅".into() } else { "⬜".into() }, i.get("businessKey").and_then(|b| b.get("$ref")).and_then(|x| x.as_str()).map(any_link).unwrap_or_else(|| "—".to_string())]).collect()).unwrap_or_default();
        let span_rows: Vec<Vec<String>> = c.get("spans").and_then(|x| x.as_sequence()).map(|s| s.iter().map(|sp| { let a = sp.get("attributes").and_then(|x| x.as_sequence()).map(|at| at.iter().map(|x| format!("`{}`{}", x.get("key").and_then(|k| k.as_str()).unwrap_or(""), if x.get("required").and_then(|r| r.as_bool()) == Some(true) { "*" } else { "" })).collect::<Vec<_>>().join(", ")).unwrap_or_default(); let a = if a.is_empty() { "—".to_string() } else { a }; vec![format!("`{}`", sp.get("name").and_then(|x| x.as_str()).unwrap_or("")), format!("`{}`", sp.get("kind").and_then(|x| x.as_str()).unwrap_or("")), if sp.get("required").and_then(|x| x.as_bool()) == Some(true) { "✅".into() } else { "⬜".into() }, sp.get("multiplicity").and_then(|x| x.as_str()).map(|mu| format!("`{}`", mu)).unwrap_or_else(|| "—".to_string()), a] }).collect()).unwrap_or_default();
        let metric_list = |key: &str| -> String { let s = c.get(key).and_then(|x| x.as_sequence()).map(|arr| arr.iter().map(|m| format!("`{}` _({})_", m.get("name").and_then(|x| x.as_str()).unwrap_or(""), m.get("type").and_then(|x| x.as_str()).unwrap_or(""))).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "—".to_string() } else { s } };
        let sr_success = c.get("status_rules").and_then(|x| x.get("success"));
        let success = sr_success.map(|s| format!("success ⇐ spans [{}]", s.get("required_spans").and_then(|x| x.as_sequence()).map(|a| a.iter().filter_map(|x| x.as_str()).map(|s| format!("`{}`", s)).collect::<Vec<_>>().join(", ")).unwrap_or_default())).unwrap_or_default();
        let lat = c.get("latency_budget");
        let err = c.get("error_budget");
        let cmd = ref_name(wf.and_then(|w| w.get("command")).and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or(""));
        let saga = ref_name(wf.and_then(|w| w.get("saga")).and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or(""));
        let ctx = if let Some(c) = &cmd { cx.of_command(c) } else if let Some(s) = &saga { cx.of_actor(s) } else { CROSS.to_string() };
        let s3 = |v: Option<&Value>, k: &str| v.and_then(|x| x.get(k)).map(|x| if let Some(n) = x.as_i64() { n.to_string() } else if let Some(f) = x.as_f64() { f.to_string() } else { x.as_str().unwrap_or("—").to_string() }).unwrap_or_else(|| "—".to_string());
        Doc { ctx, md: vec![
            format!("{}\n#### {} Contract: `{}`", id_tag(&danchor("obs", feature)), d_emo("obs"), feature),
            format!("\n_criticality: **{}**_\n", c.get("criticality").and_then(|x| x.as_str()).unwrap_or("—")),
            format!("- **Workflow**: {}{}", wf.and_then(|w| w.get("saga")).map(|s| format!("saga {}", any_link(s.get("$ref").and_then(|x| x.as_str()).unwrap_or_default()))).unwrap_or_default(), wf.and_then(|w| w.get("command")).map(|c| format!(" · command {}", any_link(c.get("$ref").and_then(|x| x.as_str()).unwrap_or_default()))).unwrap_or_default()),
            format!("- **Emits**: {} · **Inbound**: {}", ref_list_links(wf.and_then(|w| w.get("emits"))), ref_list_links(wf.and_then(|w| w.get("inbound")))),
            if id_rows.is_empty() { String::new() } else { format!("\n**Run identity**\n\n{}", md_table(&["Id", "Source", "Req.", "Business key"], &id_rows)) },
            if span_rows.is_empty() { String::new() } else { format!("\n**Spans** (`*` = required attribute)\n\n{}", md_table(&["Span", "Kind", "Req.", "Multiplicity", "Attributes"], &span_rows)) },
            format!("\n- **Metrics**: {} · **Business metrics**: {}", metric_list("metrics"), metric_list("business_metrics")),
            if success.is_empty() { String::new() } else { format!("- **Status rules**: {}", success) },
            format!("- **SLOs**: p95 ≤ {}ms · p99 ≤ {}ms · error rate ≤ {}%", s3(lat, "max_p95_ms"), s3(lat, "max_p99_ms"), s3(err, "max_error_rate_pct")),
        ].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n") }
    })).collect()).unwrap_or_default();

    // C4 doc
    let c4_doc = {
        let l2 = model.defs.get("architecture/c4-l2.yaml");
        let l3 = model.defs.get("architecture/c4-l3.yaml");
        let sysn = l2.and_then(|v| v.get("system")).and_then(|s| s.get("name")).and_then(|x| x.as_str()).unwrap_or("Captain.Food");
        let sysd = l2.and_then(|v| v.get("system")).and_then(|s| s.get("description")).and_then(|x| x.as_str()).unwrap_or("");
        let map_rows = |sect: &str, f: &dyn Fn(&str, &Value) -> Vec<String>| -> Vec<Vec<String>> { l2.and_then(|v| v.get(sect)).and_then(|x| x.as_mapping()).map(|m| m.iter().filter_map(|(k, v)| k.as_str().map(|n| f(n, v))).collect()).unwrap_or_default() };
        let bc_rows = map_rows("boundedContexts", &|n, bc| vec![format!("{} `{}`", d_emo("context"), n), bc.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string(), format!("{}{}", ref_list_links(bc.get("aggregates")), if bc.get("processManagers").is_some() { format!(" · {}", ref_list_links(bc.get("processManagers"))) } else { String::new() })]);
        let c_rows = map_rows("containers", &|n, c| vec![format!("{} `{}`", d_emo("container"), n), c.get("technology").and_then(|x| x.as_str()).unwrap_or("").to_string(), format!("{}{}", c.get("description").and_then(|x| x.as_str()).unwrap_or(""), if c.get("realizes").is_some() { format!("<br>realizes: {}", ref_list_links(c.get("realizes"))) } else { String::new() })]);
        let x_rows = map_rows("externalSystems", &|n, x| vec![format!("🔌 `{}`", n), x.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string()]);
        let rel_rows: Vec<Vec<String>> = l2.and_then(|v| v.get("relationships")).and_then(|x| x.as_sequence()).map(|s| s.iter().map(|r| vec![format!("`{}` → `{}`", r.get("from").and_then(|x| x.as_str()).unwrap_or(""), r.get("to").and_then(|x| x.as_str()).unwrap_or("")), r.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string()]).collect()).unwrap_or_default();
        let comp_rows: Vec<Vec<String>> = l3.and_then(|v| v.get("components")).and_then(|x| x.as_mapping()).map(|m| m.iter().filter_map(|(k, c)| k.as_str().map(|n| { let bind = if c.get("handles").is_some() { format!("handles {}", ref_list_links(c.get("handles"))) } else if c.get("updates").is_some() { format!("updates {}", ref_list_links(c.get("updates"))) } else { "—".to_string() }; vec![format!("{} `{}`", d_emo("component"), n), if c.get("instrumented").and_then(|x| x.as_bool()) == Some(true) { "📡 yes".into() } else { "— no".into() }, c.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string(), bind] })).collect()).unwrap_or_default();
        [
            format!("**System**: `{}` — {}", sysn, sysd),
            format!("\n### 🔲 L2 — Bounded contexts\n\n{}", md_table(&["Context", "Description", "Aggregates / process managers"], &bc_rows)),
            format!("\n### 🧱 L2 — Containers\n\n{}", md_table(&["Container", "Technology", "Description"], &c_rows)),
            format!("\n### 🔌 L2 — External systems\n\n{}", md_table(&["System", "Description"], &x_rows)),
            format!("\n### ➡️ L2 — Relationships\n\n{}", md_table(&["Edge", "Description"], &rel_rows)),
            format!("\n### ⚙️ L3 — Components of the `api` container\n\n{}", md_table(&["Component", "Instrumented", "Description", "Binds"], &comp_rows)),
        ].join("\n")
    };

    // SDUI screens + translations (reuse the C4/HTML approach)
    let sf = model.defs.get("screens/customer_screens.yaml");
    let resolvers = sf.and_then(|v| v.get("resolvers")).and_then(|v| v.as_mapping());
    let action_defs = sf.and_then(|v| v.get("actions")).and_then(|v| v.as_mapping());
    let tr_defs = model.defs.get("translations.yaml").and_then(|v| v.as_mapping());
    let cellf = |s: &str| s.replace('|', "\\|");
    let tr_en = |rf: &str| -> String { resolve_ref(model, rf, "screens/customer_screens.yaml").and_then(|t| t.get("messages")).and_then(|m| m.get("en")).and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| rf.rsplit('/').next().unwrap_or(rf).to_string()) };
    let t_text = |v: &Value| -> String { if let Some(rf) = v.get("$ref").and_then(|x| x.as_str()) { tr_en(rf) } else if let Some(s) = v.as_str() { s.to_string() } else { String::new() } };
    let tr_rows: Vec<Vec<String>> = tr_defs.map(|m| m.iter().filter_map(|(k, t)| k.as_str().map(|key| { let params = t.get("params").and_then(|x| x.as_mapping()).map(|pm| pm.iter().filter_map(|(pk, _)| pk.as_str().map(|p| format!("`{}`", p))).collect::<Vec<_>>().join(", ")).unwrap_or_default(); let params = if params.is_empty() { "—".to_string() } else { params }; vec![format!("{}`{}`", id_tag(&danchor("translation", key)), key), params, cellf(t.get("messages").and_then(|mm| mm.get("en")).and_then(|x| x.as_str()).unwrap_or("")), cellf(t.get("messages").and_then(|mm| mm.get("fr")).and_then(|x| x.as_str()).unwrap_or(""))] })).collect()).unwrap_or_default();
    let translations_section = md_table(&["Key", "Params", "🇬🇧 en", "🇫🇷 fr"], &tr_rows);
    let op_cell = |rf: Option<&str>, gap: Option<&str>| -> String { if let Some(g) = gap { return format!("⚠️ _gap: {}_", cellf(g)); } match rf { None => "—".to_string(), Some(rf) => { let name = rf.rsplit('/').next().unwrap_or(""); let kind = if rf.contains("/mutations/") { "mutation" } else if rf.contains("/subscriptions/") { "subscription" } else { "query" }; dlink(kind, name) } } };
    let action_keys: HashSet<String> = action_defs.map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    fn collect_action_types(node: &Value, keys: &HashSet<String>, acc: &mut Vec<String>) {
        match node {
            Value::Sequence(s) => s.iter().for_each(|n| collect_action_types(n, keys, acc)),
            Value::Mapping(m) => {
                if let Some(t) = m.get(Value::String("type".into())).and_then(|x| x.as_str()) { if keys.contains(t) && !acc.contains(&t.to_string()) { acc.push(t.to_string()); } }
                for (_, v) in m { collect_action_types(v, keys, acc); }
            }
            _ => {}
        }
    }
    let boxf = |w: usize, s: &str| -> String { let n = s.chars().count(); let inner = if n > w { let t: String = s.chars().take(w - 1).collect(); format!("{}…", t) } else { format!("{}{}", s, " ".repeat(w - n)) }; format!("│ {} │", inner) };
    let screens_arr = sf.and_then(|v| v.get("screens")).and_then(|x| x.as_sequence()).cloned().unwrap_or_default();
    let screen_docs: Vec<String> = screens_arr.iter().map(|s| {
        let id = s.get("id").and_then(|x| x.as_str()).unwrap_or("?");
        let route = s.get("route").and_then(|x| x.as_str()).unwrap_or("");
        let title = { let t = s.get("title").map(|v| t_text(v)).unwrap_or_default(); if t.is_empty() { id.to_string() } else { t } };
        let sdui_badge = if s.get("sdui").and_then(|x| x.as_bool()) == Some(false) { format!("🚫 not SDUI{}", s.get("sdui_reason").and_then(|x| x.as_str()).map(|r| format!(" — {}", r)).unwrap_or_default()) } else { "📱 SDUI".to_string() };
        let auth = if s.get("requires_auth").and_then(|x| x.as_bool()) == Some(true) { " · 🔒 auth" } else { "" };
        let mut rows: Vec<Vec<String>> = Vec::new();
        for rn in s.get("data_requirements").and_then(|x| x.as_sequence()).map(|s| s.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect::<Vec<_>>()).unwrap_or_default() {
            let r = resolvers.and_then(|m| m.get(rn.as_str()));
            rows.push(vec!["read".to_string(), format!("`{}`", rn), op_cell(r.and_then(|x| x.get("query")).and_then(|q| q.get("$ref")).and_then(|x| x.as_str()), r.and_then(|x| x.get("gap")).and_then(|x| x.as_str()))]);
        }
        let mut acts: Vec<String> = Vec::new();
        if let Some(comps) = s.get("components") { collect_action_types(comps, &action_keys, &mut acts); }
        for a in s.get("actions_used").and_then(|x| x.as_sequence()).map(|s| s.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect::<Vec<_>>()).unwrap_or_default() { if !acts.contains(&a) { acts.push(a); } }
        for a in &acts {
            let ad = action_defs.and_then(|m| m.get(a.as_str()));
            if ad.map(|x| x.get("mutation").is_some() || x.get("gap").is_some()).unwrap_or(false) {
                rows.push(vec!["write".to_string(), format!("`{}`", a), op_cell(ad.and_then(|x| x.get("mutation")).and_then(|q| q.get("$ref")).and_then(|x| x.as_str()), ad.and_then(|x| x.get("gap")).and_then(|x| x.as_str()))]);
            }
        }
        let ops_table = md_table(&["Kind", "UI need", "GraphQL operation"], &rows);
        let mut mock_lines: Vec<String> = Vec::new();
        if let Some(comps) = s.get("components").and_then(|x| x.as_sequence()) {
            for c in comps {
                let t = if let Some(cp) = c.get("component").and_then(|x| x.as_str()) { format!("«{}»", cp) } else { c.get("type").and_then(|x| x.as_str()).unwrap_or("?").to_string() };
                let lbl = { let l = c.get("title").map(|v| t_text(v)).filter(|s| !s.is_empty()).or_else(|| c.get("label").map(|v| t_text(v)).filter(|s| !s.is_empty())).or_else(|| c.get("placeholder").map(|v| t_text(v)).filter(|s| !s.is_empty())).unwrap_or_default(); l };
                mock_lines.push(boxf(40, &format!("{}{}", t, if lbl.is_empty() { String::new() } else { format!(" — {}", lbl) })));
            }
        }
        let mut mock = vec![format!("┌{}┐", "─".repeat(42)), boxf(40, &title), format!("├{}┤", "─".repeat(42))];
        mock.extend(mock_lines);
        mock.push(format!("└{}┘", "─".repeat(42)));
        let gaps = s.get("gaps").and_then(|x| x.as_sequence()).map(|g| g.iter().filter_map(|x| x.as_str()).map(|g| format!("- ⚠️ {}", g)).collect::<Vec<_>>().join("\n")).unwrap_or_default();
        format!("{}\n### {} `{}` · `{}` · {}{}\n\n```\n{}\n```\n\n{}{}", id_tag(&danchor("screen", id)), d_emo("screen"), id, route, sdui_badge, auth, mock.join("\n"), ops_table, if gaps.is_empty() { String::new() } else { format!("\n\n**Gaps**\n{}", gaps) })
    }).collect();
    let screens_section = screen_docs.join("\n\n");

    // Assembly
    let sec = |id: &str, emoji: &str, title: &str| format!("{}\n## {} {}", id_tag(&format!("sec-{}", id)), emoji, title);
    let in_ctx = |docs: &[Doc], ctx: &str| -> Vec<String> { docs.iter().filter(|d| d.ctx == ctx).map(|d| d.md.clone()).collect() };
    let kind_sub = |emoji: &str, title: &str, bodies: Vec<String>| -> String { if bodies.is_empty() { String::new() } else { format!("### {} {} _({})_\n\n{}", emoji, title, bodies.len(), bodies.join("\n\n")) } };
    let doc_sub = |emoji: &str, title: &str, docs: &[Doc], ctx: &str| kind_sub(emoji, title, in_ctx(docs, ctx));
    let row_sub = |emoji: &str, title: &str, head: &[&str], rows: &[DRow], ctx: &str| -> String { let r: Vec<&DRow> = rows.iter().filter(|x| x.ctx == ctx).collect(); if r.is_empty() { String::new() } else { format!("### {} {} _({})_\n\n{}", emoji, title, r.len(), md_table(head, &r.iter().map(|x| x.cells.clone()).collect::<Vec<_>>())) } };
    let mut ctx_blocks: Vec<(String, Vec<String>)> = Vec::new();
    for ctx in &cx.order {
        let parts: Vec<String> = [
            doc_sub("🧰", "API operations", &api_docs, ctx),
            doc_sub(d_emo("type"), "Output types", &type_docs, ctx),
            doc_sub(d_emo("actor"), "Actors", &actor_docs, ctx),
            doc_sub(d_emo("view"), "Views (read models)", &view_docs, ctx),
            doc_sub(d_emo("command"), "Commands", &command_docs, ctx),
            doc_sub(d_emo("event"), "Events", &event_docs, ctx),
            doc_sub(d_emo("entity"), "Entities", &entity_docs, ctx),
            row_sub(d_emo("scalar"), "Scalars", &["Scalar", "Type", "Description"], &scalar_rows, ctx),
            row_sub(d_emo("error"), "Errors", &["Error", "Description", "Message (en)", "Message (fr)", "Thrown by"], &error_rows, ctx),
            doc_sub(d_emo("rule"), "Business rules", &rule_docs, ctx),
            doc_sub(d_emo("test"), "Tests", &test_docs, ctx),
            doc_sub(d_emo("obs"), "Observability", &obs_docs, ctx),
        ].into_iter().filter(|s| !s.is_empty()).collect();
        if !parts.is_empty() {
            ctx_blocks.push((ctx.clone(), parts));
        }
    }
    let ctx_sections = ctx_blocks.iter().enumerate().map(|(i, (ctx, parts))| {
        let d = cx.describe(ctx);
        format!("{}\n## {} {}. {}\n\n{}{}", id_tag(&format!("sec-ctx-{}", dslug(ctx))), d_emo("context"), i + 1, ctx, if d.is_empty() { String::new() } else { format!("_{}_\n\n", d) }, parts.join("\n\n"))
    }).collect::<Vec<_>>().join("\n\n");
    let ctx_toc = ctx_blocks.iter().map(|(ctx, _)| format!("[{} {}](#sec-ctx-{})", d_emo("context"), ctx, dslug(ctx))).collect::<Vec<_>>().join(" · ");

    format!(
        "<!-- GENERATED by tools/codegen — do not edit by hand. Source: specs/*.yaml. -->\n# 📖 Captain.Food — Product Documentation (generated)\n\nA single, navigable view of the whole product, built from the specs and organized **top-level by\nbounded context** (🔲). Within each context: its API operations, output types, actors, views, commands,\nevents, entities, scalars, errors, business rules (📐 — what we guarantee), tests (🧪 — how it's verified,\ncross-linked to the rules) and observability contracts. Every item — and every\n**property** 🔹 — is anchored and **cross-linked**; `cross-cutting` holds the shared vocabulary and ops\nthat belong to no single context. Stories and Architecture span all contexts.\n\n**Kinds**: {q} query · {mu} mutation · {su} subscription · {ty} type · {ac} actor · {vi} view · {cm} command · {ev} event · {en} entity · {sc} scalar · {er} error · {pr} property\n**Roles**: 🌐 PUBLIC · 🙋 CUSTOMER · 🏪 RESTAURANT_ACCOUNT · 🍽️ RESTAURANT · 🛵 RIDER · 🛠️ ADMIN · 🔌 EXTERNAL\n**Markers**: ✅ required · ⬜ optional · 🛶 V0 · 🔭 V1 · 🔒 internal · ⚠️ design hole\n\n**Contents** — [🎬 Stories](#sec-stories) · {toc} · [📱 Customer screens](#sec-screens) · [🌐 Translations](#sec-translations) · [🏛️ Architecture](#sec-architecture)\n\n{s_stories}\n\nHow each persona uses the API. `personaRole` is the persona's GraphQL path-role (UserType).\n\n{stories}\n\n{ctxs}\n\n{s_screens}\n\nServer-Driven UI screens (`specs/customer_screens.yaml`, ADR-0033). Each screen's **reads** (resolvers →\nqueries) and **writes** (actions → mutations) are `$ref`-bound to the GraphQL API and validated, so the\nmockups below are the **proof the API answers the UI**. ⚠️ gaps mark UI needs the API does not serve yet.\nScreens marked 🚫 are intentionally not SDUI-rendered (Stripe/subscription/auth integrity).\n\n{screens}\n\n{s_trans}\n\nThe i18n catalog (`specs/translations.yaml`) — every user-visible screen string, referenced by `$ref` and\ngenerated to a single `translations.generated.json`. `{{param}}` tokens are validated against `params`.\n\n{trans}\n\n{s_arch}\n\nC4 views as source-managed DSL (`specs/architecture/c4-l{{2,3}}.yaml`). Bounded contexts bind their\naggregates; components bind the aggregates they handle and the read models they update.\n\n{c4}\n",
        q = d_emo("query"), mu = d_emo("mutation"), su = d_emo("subscription"), ty = d_emo("type"), ac = d_emo("actor"), vi = d_emo("view"), cm = d_emo("command"), ev = d_emo("event"), en = d_emo("entity"), sc = d_emo("scalar"), er = d_emo("error"), pr = d_emo("property"),
        toc = ctx_toc,
        s_stories = sec("stories", "🎬", "Stories"),
        stories = stories_section,
        ctxs = ctx_sections,
        s_screens = sec("screens", "📱", "Customer screens (SDUI)"),
        screens = screens_section,
        s_trans = sec("translations", "🌐", "Translations"),
        trans = translations_section,
        s_arch = sec("architecture", "🏛️", "Architecture (C4)"),
        c4 = c4_doc,
    )
}

// ─── documentation.generated.html (port of emit/documentation-html.ts) ──────────────────────────

const THEME: &str = r##"<style>
  :root {
    --bg:#2b2b2b; --bg2:#313335; --bg3:#3c3f41; --fg:#a9b7c6; --muted:#808080; --line:#4b4b4b;
    --type:#4ec9b0; --scalar:#4fc1ff; --op:#dcdcaa; --event:#c586c0; --error:#f44747;
    --prop:#9cdcfe; --param:#d7ba7d; --const:#b5cea8; --kw:#cc7832; --accent:#ffc66d;
  }
  * { box-sizing:border-box; }
  body { margin:0; background:#2b2b2b; }
  .doc { background:var(--bg); color:var(--fg); font:14px/1.55 "JetBrains Mono","SFMono-Regular",Consolas,"Liberation Mono",monospace; padding:0 0 40vh; }
  .doc .wrap { max-width:1100px; margin:0 auto; padding:24px 20px; }
  .doc h1 { color:#fff; font-size:24px; border-bottom:2px solid var(--line); padding-bottom:10px; }
  .doc h3 { color:var(--accent); margin:18px 0 6px; }
  .doc a { color:var(--prop); text-decoration:none; }
  .doc a:hover { text-decoration:underline; }
  .doc code, .doc .id { font-family:inherit; }
  .k-type { color:var(--type); } .k-scalar { color:var(--scalar); } .k-op { color:var(--op); }
  .k-event { color:var(--event); } .k-error { color:var(--error); } .k-prop { color:var(--prop); }
  .k-param { color:var(--param); } .k-const { color:var(--const); } .k-id { color:var(--fg); }
  .kw { color:var(--kw); } .muted { color:var(--muted); } .req { color:var(--const); } .opt { color:var(--muted); }
  /* collapsible sections + items */
  details.sec { border:1px solid var(--line); border-radius:6px; margin:14px 0; background:var(--bg2); }
  details.sec > summary { cursor:pointer; padding:12px 16px; font-size:18px; color:#fff; list-style:none; background:var(--bg2); border-radius:6px; }
  details.sec[open] > summary { border-bottom:1px solid var(--line); border-radius:6px 6px 0 0; }
  details.sec > .body { padding:8px 16px 16px; }
  details.subsec { border:1px solid var(--line); border-radius:6px; margin:10px 0; background:var(--bg); }
  details.subsec > summary { cursor:pointer; padding:8px 12px; font-size:15px; color:var(--accent); list-style:none; }
  details.subsec[open] > summary { border-bottom:1px solid var(--line); }
  details.subsec > .body { padding:8px 12px; }
  details.item { border-left:2px solid var(--line); margin:10px 0; padding-left:12px; }
  details.item > summary { cursor:pointer; list-style:none; padding:3px 0; }
  summary::-webkit-details-marker { display:none; }
  summary .tw { color:var(--muted); display:inline-block; width:1em; }
  .perma { color:var(--muted); opacity:0; margin-left:8px; font-size:.85em; }
  summary:hover .perma, h2:hover .perma { opacity:1; }
  .desc { color:var(--fg); margin:4px 0 8px; opacity:.92; }
  .rel { margin:2px 0; } .rel .lbl { color:var(--muted); }
  table { border-collapse:collapse; margin:6px 0 4px; width:100%; }
  th,td { border:1px solid var(--line); padding:4px 8px; text-align:left; vertical-align:top; }
  th { background:var(--bg3); color:#fff; font-weight:600; }
  .badge { background:var(--bg3); border:1px solid var(--line); border-radius:4px; padding:0 6px; font-size:.85em; }
  .toolbar { background:var(--bg); padding:10px 0; border-bottom:1px solid var(--line); }
  /* sticky breadcrumb: shows context › section › item wherever you are, each segment clickable */
  .crumb { position:sticky; top:0; z-index:6; background:var(--bg3); border-bottom:1px solid var(--line); margin:0 -20px 8px; padding:7px 20px; font-size:13px; white-space:nowrap; overflow-x:auto; color:var(--muted); }
  .crumb .seg { color:var(--fg); cursor:pointer; }
  .crumb .seg:hover { color:var(--accent); text-decoration:underline; }
  .crumb .sep { color:var(--muted); margin:0 7px; }
  /* hover tooltip: an object's description, looked up (centralized) from CF_DESC by anchor id */
  .cf-tip { position:fixed; z-index:50; max-width:440px; background:#1e1e1e; color:var(--fg); border:1px solid var(--line); border-radius:6px; padding:8px 10px; font-size:12.5px; line-height:1.5; box-shadow:0 4px 16px rgba(0,0,0,.45); pointer-events:none; display:none; }
  .cf-tip.empty { color:var(--muted); font-style:italic; }
  .toolbar button { background:var(--bg3); color:var(--fg); border:1px solid var(--line); border-radius:4px; padding:4px 10px; cursor:pointer; font:inherit; }
  .toolbar button:hover { border-color:var(--accent); color:#fff; }
  .toc a { margin-right:14px; white-space:nowrap; }
  .hole { color:var(--error); }
  /* interactive C4 / flow map */
  .cfmap { border:1px solid var(--line); border-radius:6px; background:#262626; padding:8px; }
  .cfmap-bar { display:flex; align-items:center; gap:10px; padding:4px 6px; flex-wrap:wrap; }
  .cfmap-bar button { background:var(--bg3); color:var(--fg); border:1px solid var(--line); border-radius:4px; padding:3px 10px; cursor:pointer; font:inherit; }
  .cfmap-bar button:hover { border-color:var(--accent); color:#fff; }
  #cf-svg { width:100%; height:auto; display:block; background:#262626; border-radius:4px; }
  .cf-node { cursor:pointer; }
  .cf-node:hover rect { filter:brightness(1.3); }
  .cf-node text { pointer-events:none; }
  .cfmap-info { padding:6px; font-size:.88em; }
  /* saga sequence diagrams: MERMAID_JS renders pre.mermaid in place; offline the same styling
     keeps the diagram SOURCE readable (monospace, scrollable, dark-palette border) */
  .pm-seq { margin:8px 0; }
  .pm-seq pre.mermaid { background:#262626; border:1px solid var(--line); border-radius:6px; padding:10px 12px; overflow-x:auto; font-size:12.5px; line-height:1.5; color:var(--fg); }
  .pm-seq pre.mermaid svg { max-width:100%; }
</style>
<script>
  function setAll(open){ document.querySelectorAll('details').forEach(d=>d.open=open); }
</script>"##;

const MAP_JS: &str = r##"(function(){var M=__CF_DATA__;var svg=document.getElementById('cf-svg'),crumb=document.getElementById('cf-crumb'),info=document.getElementById('cf-info'),back=document.getElementById('cf-back');if(!svg)return;var NS='http://www.w3.org/2000/svg';var stack=[{key:'system',title:'System'}];function slug(s){return String(s).toLowerCase().replace(/[^a-z0-9_]+/g,'-');}function el(t,a,x){var e=document.createElementNS(NS,t);for(var k in a)e.setAttribute(k,a[k]);if(x!=null)e.textContent=x;return e;}var K={container:'#4ec9b0',external:'#cc7832',context:'#ffc66d',actor:'#4ec9b0','process':'#56a0c0',command:'#dcdcaa',event:'#c586c0',view:'#9cdcfe'};function find(a,id){for(var i=0;i<a.length;i++)if(a[i].id===id)return a[i];return null;}function frame(key){if(key==='system'){var nodes=[];M.containers.forEach(function(c){nodes.push({id:c.id,label:c.id,kind:'container',sub:'container:'+c.id,desc:c.technology+' — '+c.description});});M.externals.forEach(function(x){nodes.push({id:x.id,label:x.id,kind:'external',desc:x.description});});var ids={};nodes.forEach(function(n){ids[n.id]=1;});var edges=M.relationships.filter(function(r){return ids[r.from]&&ids[r.to];}).map(function(r){return {from:r.from,to:r.to,label:r.description};});return {title:'System',nodes:nodes,edges:edges,note:'Containers (teal) and external systems (orange). Click a container to see its bounded contexts.'};}if(key.indexOf('container:')===0){var id=key.slice(10);var c=find(M.containers,id)||{realizes:[]};var nodes=[];M.contexts.forEach(function(ctx){var inIt=(ctx.aggregates||[]).some(function(a){return (c.realizes||[]).indexOf(a)>=0;});if(inIt)nodes.push({id:ctx.id,label:ctx.id,kind:'context',sub:'context:'+ctx.id,desc:ctx.description});});return {title:id,nodes:nodes,edges:[],note:nodes.length?'Bounded contexts running in this container. Click one to see its aggregates.':'No bounded context runs in this container (infrastructure/runtime unit).'};}if(key.indexOf('context:')===0){var id=key.slice(8);var ctx=find(M.contexts,id)||{aggregates:[],processManagers:[]};var nodes=(ctx.aggregates||[]).map(function(a){return {id:a,label:a,kind:'actor',sub:'actor:'+a,anchor:'actor-'+slug(a)};});(ctx.processManagers||[]).forEach(function(a){nodes.push({id:a,label:a,kind:'process',sub:'actor:'+a,anchor:'actor-'+slug(a)});});return {title:id,nodes:nodes,edges:[],note:'Aggregates and process managers (sagas). Click one to see its command → event → view flow.'};}if(key.indexOf('actor:')===0){var name=key.slice(6);var a=M.actors[name]||{receives:[]};var nodes=[],edges=[],seen={};function add(id,label,kind,anchor){if(!seen[id]){seen[id]=1;nodes.push({id:id,label:label,kind:kind,anchor:anchor});}}add('A',name,a.type==='process-manager'?'process':'actor','actor-'+slug(name));a.receives.forEach(function(r){var mid=(r.isCommand?'c:':'e:')+r.message;add(mid,r.message,r.isCommand?'command':'event',(r.isCommand?'command-':'event-')+slug(r.message));edges.push({from:'A',to:mid,label:'receives'});(r.emits||[]).forEach(function(ev){add('e:'+ev,ev,'event','event-'+slug(ev));edges.push({from:mid,to:'e:'+ev,label:'emits'});M.views.forEach(function(v){if((v.fedBy||[]).indexOf(ev)>=0){add('v:'+v.name,v.name,'view','view-'+slug(v.name));edges.push({from:'e:'+ev,to:'v:'+v.name,label:'projects'});}});});});return {title:name,nodes:nodes,edges:edges,note:'Flow: message (yellow=command, purple=event) → emitted events → read models (blue). Click a box to jump to its section.'};}return {title:'?',nodes:[],edges:[]};}function render(){var f=frame(stack[stack.length-1].key);crumb.textContent=stack.map(function(s){return s.title;}).join('  ›  ');back.style.visibility=stack.length>1?'visible':'hidden';while(svg.firstChild)svg.removeChild(svg.firstChild);var defs=el('defs');var mk=el('marker',{id:'cf-arrow',viewBox:'0 0 10 10',refX:'9',refY:'5',markerWidth:'7',markerHeight:'7',orient:'auto'});mk.appendChild(el('path',{d:'M0,0 L10,5 L0,10 z',fill:'#888'}));defs.appendChild(mk);svg.appendChild(defs);var W=960,H=560,n=f.nodes.length||1;var cols=Math.max(1,Math.ceil(Math.sqrt(n)));var rows=Math.ceil(n/cols);var nw=180,nh=48;var gx=(W-cols*nw)/(cols+1),gy=(H-rows*nh)/(rows+1);var pos={};f.nodes.forEach(function(nd,i){var r=Math.floor(i/cols),c=i%cols;pos[nd.id]={x:gx+c*(nw+gx),y:gy+r*(nh+gy)};});f.edges.forEach(function(e){var a=pos[e.from],b=pos[e.to];if(!a||!b)return;var x1=a.x+nw/2,y1=a.y+nh/2,x2=b.x+nw/2,y2=b.y+nh/2;var ln=el('line',{x1:x1,y1:y1,x2:x2,y2:y2,stroke:'#6a6a6a','stroke-width':'1.3','marker-end':'url(#cf-arrow)'});if(e.label)ln.appendChild(el('title',null,e.label));svg.appendChild(ln);});f.nodes.forEach(function(nd){var p=pos[nd.id];var g=el('g',{'class':'cf-node',transform:'translate('+p.x+','+p.y+')'});g.appendChild(el('rect',{width:nw,height:nh,rx:'7',fill:'#313335',stroke:(K[nd.kind]||'#888'),'stroke-width':'1.6'}));var label=nd.label.length>24?nd.label.slice(0,23)+'…':nd.label;g.appendChild(el('text',{x:nw/2,y:nh/2+4,'text-anchor':'middle',fill:'#e6e6e6','font-size':'12'},label));if(nd.desc)g.appendChild(el('title',null,nd.desc));g.addEventListener('click',function(){if(nd.sub){stack.push({key:nd.sub,title:nd.label});render();}else if(nd.anchor){location.hash=nd.anchor;}});svg.appendChild(g);});info.textContent=f.note||'';}back.addEventListener('click',function(){if(stack.length>1){stack.pop();render();}});render();})();"##;

const NAV_JS: &str = r##"<script>(function(){var bar=document.getElementById('cf-crumb'),tip=document.getElementById('cf-tip'),doc=document.querySelector('.doc');if(!bar||!doc)return;var TH=54,cur={};function esc(s){return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;');}function lab(el){return el?(el.getAttribute('data-crumb')||''):'';}function lastAbove(sel){var e=document.querySelectorAll(sel),f=null;for(var i=0;i<e.length;i++){var s=e[i];if(s.offsetParent===null)continue;if(s.getBoundingClientRect().top<=TH)f=s;}return f;}function upd(){var a=lastAbove('details.sec>summary'),b=lastAbove('details.subsec>summary'),c=lastAbove('details.item>summary');cur.ctx=a?a.parentElement:null;cur.sec=b?b.parentElement:null;cur.item=c?c.parentElement:null;if(cur.sec&&cur.ctx&&!cur.ctx.contains(cur.sec))cur.sec=null;if(cur.item&&cur.sec&&!cur.sec.contains(cur.item))cur.item=null;if(cur.item&&!cur.sec)cur.item=null;var p=[];if(cur.ctx)p.push('<span class="seg" data-role="ctx">'+esc(lab(cur.ctx))+'</span>');if(cur.sec)p.push('<span class="seg" data-role="sec">'+esc(lab(cur.sec))+'</span>');if(cur.item)p.push('<span class="seg" data-role="item">'+esc(lab(cur.item))+'</span>');bar.innerHTML=p.length?p.join('<span class="sep">\u203a</span>'):'<span class="muted">\ud83d\udcd6 Captain.Food \u2014 Product Documentation</span>';}bar.addEventListener('click',function(e){var s=e.target.closest('.seg');if(!s)return;var el=cur[s.getAttribute('data-role')];if(!el)return;var sm=el.querySelector(':scope>summary')||el;var y=sm.getBoundingClientRect().top+window.pageYOffset-TH-8;window.scrollTo({top:y,behavior:'smooth'});});var raf=0;function onScroll(){if(raf)return;raf=requestAnimationFrame(function(){raf=0;upd();});}window.addEventListener('scroll',onScroll,{passive:true});window.addEventListener('resize',onScroll);document.addEventListener('toggle',onScroll,true);upd();var D=window.CF_DESC||{};doc.addEventListener('mouseover',function(e){var a=e.target.closest('a[href^="#"]');if(!a)return;var id=decodeURIComponent(a.getAttribute('href').slice(1));if(!(id in D)){tip.style.display='none';return;}var d=D[id];tip.textContent=d||'no description yet';tip.className='cf-tip'+(d?'':' empty');tip.style.display='block';});doc.addEventListener('mousemove',function(e){if(tip.style.display!=='block')return;var x=e.clientX+14,y=e.clientY+16,w=tip.offsetWidth,h=tip.offsetHeight;if(x+w>window.innerWidth-8)x=window.innerWidth-w-8;if(y+h>window.innerHeight-8)y=e.clientY-h-14;tip.style.left=x+'px';tip.style.top=y+'px';});doc.addEventListener('mouseout',function(e){if(e.target.closest('a[href^="#"]'))tip.style.display='none';});})();</script>"##;

// Renders every <pre class="mermaid"> (the saga sequence diagrams). Constraints: the CDN import may
// be unreachable (offline docs) — then the styled source text must stay as-is; diagrams sit inside
// <details> that the reader may collapse/re-open — mermaid mis-sizes hidden elements, so only
// visible ones are rendered and re-opened <details> render lazily on their toggle event, with a
// data-mermaid-rendered guard against double rendering.
const MERMAID_JS: &str = r##"<script type="module">
try {
  const { default: mermaid } = await import('https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs');
  mermaid.initialize({ startOnLoad: false, theme: 'dark', securityLevel: 'loose' });
  const render = (root) => {
    const nodes = [...root.querySelectorAll('pre.mermaid:not([data-mermaid-rendered])')].filter((n) => n.offsetParent !== null);
    if (!nodes.length) return;
    nodes.forEach((n) => n.setAttribute('data-mermaid-rendered', ''));
    mermaid.run({ nodes }).catch(() => {});
  };
  document.addEventListener('toggle', (e) => { if (e.target.open) render(e.target); }, true);
  render(document);
} catch (e) { /* offline: the <pre> keeps showing the diagram source */ }
</script>"##;

fn h_esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
fn h_cls(k: &str) -> &'static str {
    match k {
        "type" | "entity" | "view" | "actor" | "context" | "container" | "screen" => "k-type",
        "scalar" | "rule" | "translation" => "k-scalar",
        "query" | "mutation" | "command" | "test" | "component" | "subscription" => "k-op",
        "event" | "obs" => "k-event",
        "error" => "k-error",
        "property" => "k-prop",
        _ => "k-id",
    }
}
fn h_link(kind: &str, name: &str) -> String {
    format!("<a class=\"{}\" href=\"#{}\">{}&nbsp;{}</a>", h_cls(kind), danchor(kind, name), d_emo(kind), h_esc(name))
}
fn h_plink(kind: &str, owner: &str, field: &str) -> String {
    format!("<a class=\"{}\" href=\"#{}\">{}&nbsp;{}.<span class=\"k-prop\">{}</span></a>", h_cls(kind), dprop_anchor(kind, owner, field), d_emo(kind), h_esc(owner), h_esc(field))
}
fn h_ref_label(rf: &str) -> String {
    let mut it = rf.splitn(2, "#/");
    let file = it.next().unwrap_or("");
    let name = it.next().unwrap_or("");
    if file == "scalars.yaml" { h_link("scalar", name) } else { h_link("entity", name) }
}
fn h_raw_type(p: &Value) -> String {
    if let Some(rf) = p.get("$ref").and_then(|x| x.as_str()) {
        return h_ref_label(rf);
    }
    if p.get("type").and_then(|x| x.as_str()) == Some("array") {
        if let Some(items) = p.get("items") {
            return format!("[{}]", h_raw_type(items));
        }
    }
    let mut t = format!("<span class=\"k-const\">{}</span>", h_esc(p.get("type").and_then(|x| x.as_str()).unwrap_or("?")));
    if let Some(en) = p.get("enum").and_then(|x| x.as_sequence()) {
        t += &format!(" <span class=\"muted\">({})</span>", en.iter().filter_map(|v| v.as_str()).map(h_esc).collect::<Vec<_>>().join(" | "));
    }
    if let Some(fmt) = p.get("format").and_then(|x| x.as_str()) {
        t += &format!(" <span class=\"muted\">{}</span>", h_esc(fmt));
    }
    t
}
fn h_req_cell(required: bool, nullable: bool) -> String {
    if required {
        "<span class=\"req\">✅ required</span>".to_string()
    } else {
        format!("<span class=\"opt\">⬜ {}</span>", if nullable { "nullable" } else { "optional" })
    }
}
fn h_table(head: &[&str], rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let thead = head.iter().map(|h| format!("<th>{}</th>", h)).collect::<Vec<_>>().join("");
    let tbody = rows.iter().map(|r| format!("<tr>{}</tr>", r.iter().map(|c| format!("<td>{}</td>", c)).collect::<Vec<_>>().join(""))).collect::<Vec<_>>().join("");
    format!("<table><thead><tr>{}</tr></thead><tbody>{}</tbody></table>", thead, tbody)
}
fn h_item(kind: &str, label: &str, name: &str, body: &str, desc_txt: Option<&str>) -> String {
    let id = danchor(kind, name);
    let perma = format!("<a class=\"perma\" href=\"#{}\" title=\"Lien vers cette section\">🔗 #{}</a>", id, id);
    let desc = match desc_txt {
        Some(d) if !d.is_empty() => format!("<div class=\"desc\">{}</div>", h_esc(d)),
        _ => String::new(),
    };
    format!("<details class=\"item\" id=\"{}\" data-crumb=\"{} {}\" open><summary><span class=\"tw\">▸</span><span class=\"muted\">{}:</span> <span class=\"{}\">{} {}</span>{}</summary>{}{}</details>", id, d_emo(kind), h_esc(name), label, h_cls(kind), d_emo(kind), h_esc(name), perma, desc, body)
}
fn h_prop_rows(def: &Value, kind: &str, owner: &str) -> Vec<Vec<String>> {
    let props = match def.get("properties").and_then(|x| x.as_mapping()) {
        Some(m) => m,
        None => return vec![],
    };
    let required: HashSet<&str> = def.get("required").and_then(|x| x.as_sequence()).map(|s| s.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
    let mut rows = Vec::new();
    for (k, p) in props {
        let n = match k.as_str() { Some(s) => s, None => continue };
        rows.push(vec![
            format!("<span id=\"{}\" class=\"k-prop\">{}</span>", dprop_anchor(kind, owner, n), h_esc(n)),
            h_raw_type(p),
            h_req_cell(required.contains(n), p.get("nullable").and_then(|x| x.as_bool()) == Some(true)),
            h_esc(&ws1(p.get("description").and_then(|x| x.as_str()).unwrap_or(""))),
        ]);
    }
    rows
}
fn h_sec(id: &str, emoji: &str, title: &str, body: &str) -> String {
    format!("<details class=\"sec\" id=\"sec-{}\" data-crumb=\"{} {}\" open><summary>{} {} <a class=\"perma\" href=\"#sec-{}\">🔗</a></summary><div class=\"body\">{}</div></details>", id, emoji, h_esc(title), emoji, h_esc(title), id, body)
}
fn h_subsec(emoji: &str, title: &str, count: usize, body: &str) -> String {
    format!("<details class=\"subsec\" data-crumb=\"{} {}\" open><summary>{} {} <span class=\"muted\">({})</span></summary><div class=\"body\">{}</div></details>", emoji, h_esc(title), emoji, h_esc(title), count, body)
}
fn h_any_link(rf: &str) -> String {
    let mut it = rf.splitn(2, "#/");
    let file = it.next().unwrap_or("");
    let name = it.next().unwrap_or("");
    let kind = match file { "commands.yaml" => "command", "events.yaml" => "event", "actors.yaml" => "actor", "database/projection_views.yaml" => "view", "database/tables/projection_tables.yaml" => "view", "database/tables/referential.yaml" => "view", "scalars.yaml" => "scalar", _ => "entity" };
    h_link(kind, name)
}
fn h_ref_links(v: Option<&Value>) -> String {
    let s = v.and_then(|x| x.as_sequence()).map(|arr| arr.iter().filter_map(|it| it.get("$ref").and_then(|r| r.as_str())).map(h_any_link).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    if s.is_empty() { "—".to_string() } else { s }
}

struct HDoc {
    ctx: String,
    html: String,
}
struct HRow {
    ctx: String,
    cells: Vec<String>,
}

fn emit_documentation_html(model: &Model) -> String {
    let api = parse_api(model);
    let actors = parse_actors(model);
    let views = parse_views(model);
    let personas = parse_stories(model);
    let cx = build_context_map(model, &api, &actors, &views);
    let scalar_set = scalar_names(model);
    let entity_set: HashSet<String> = model.defs.get("entities.yaml").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let type_set: HashSet<String> = api.types.iter().map(|t| t.name.clone()).collect();
    let raw_desc = |file: &str, name: &str| -> String { model.defs.get(file).and_then(|m| m.get(name)).and_then(|n| n.get("description")).and_then(|x| x.as_str()).unwrap_or("").to_string() };

    let h_api_type = |f: &ApiField| -> String {
        let base = if f.is_ref {
            if scalar_set.contains(&f.ty) { h_link("scalar", &f.ty) } else if type_set.contains(&f.ty) { h_link("type", &f.ty) } else if entity_set.contains(&f.ty) { h_link("entity", &f.ty) } else { format!("<span class=\"k-id\">{}</span>", h_esc(&f.ty)) }
        } else {
            format!("<span class=\"k-const\">{}</span>{}", h_esc(&f.ty), f.format.as_deref().map(|fmt| format!(" <span class=\"muted\">{}</span>", h_esc(fmt))).unwrap_or_default())
        };
        if f.array { format!("[{}]", base) } else { base }
    };

    // relationship indexes
    let mut cmd_handler: HashMap<String, (String, Vec<String>, Vec<String>)> = HashMap::new();
    let mut evt_emitted_by: HashMap<String, Vec<String>> = HashMap::new();
    let mut evt_consumed_by: HashMap<String, Vec<String>> = HashMap::new();
    let mut err_thrown_by: HashMap<String, Vec<String>> = HashMap::new();
    for a in &actors {
        for e in &a.receives {
            let msg = ref_name(&e.message_ref);
            let emits: Vec<String> = e.emits.iter().filter_map(|r| ref_name(r)).collect();
            let throws: Vec<String> = e.throws.iter().filter_map(|r| ref_name(r)).collect();
            if e.message_ref.starts_with("commands.yaml#/") {
                if let Some(m) = &msg {
                    cmd_handler.insert(m.clone(), (a.name.clone(), emits.clone(), throws.clone()));
                    for er in &throws { push_uniq(&mut err_thrown_by, er, m); }
                }
            } else if e.message_ref.starts_with("events.yaml#/") {
                if let Some(m) = &msg { push_uniq(&mut evt_consumed_by, m, &a.name); }
            }
            for ev in &emits { push_uniq(&mut evt_emitted_by, ev, &a.name); }
        }
    }
    let mut evt_views: HashMap<String, Vec<String>> = HashMap::new();
    for v in &views { for e in &v.fedby { push_uniq(&mut evt_views, e, &v.name); } }
    let mut mut_by_command: HashMap<String, String> = HashMap::new();
    for m in &api.mutations { mut_by_command.insert(m.command.clone(), m.name.clone()); }

    // 1. Stories
    let stories_html = personas.iter().map(|p| {
        let badge = format!("<span class=\"badge\">{} {}</span>{}", user_emo(&p.role), h_esc(&p.role), p.locale.as_deref().map(|l| format!(" <span class=\"badge\">🗣️ {}</span>", h_esc(l))).unwrap_or_default());
        let mut rows: Vec<Vec<String>> = Vec::new();
        for act in &p.activities {
            for (i, s) in act.steps.iter().enumerate() {
                let op = if let (Some(op), Some(kind)) = (&s.op, &s.op_kind) { h_link(kind, op) } else if let Some(note) = &s.note { format!("📝 <span class=\"muted\">{}</span>", h_esc(note)) } else { "—".to_string() };
                rows.push(vec![if i == 0 { format!("<span class=\"kw\">{}</span>", h_esc(&act.name)) } else { String::new() }, h_esc(&s.name), op]);
            }
        }
        h_item("story", "Persona", &p.name, &h_table(&["Activity", "Step", "Operation"], &rows), p.description.as_deref())
            .replacen("</summary>", &format!(" {}</summary>", badge), 1)
    }).collect::<Vec<_>>().join("");

    // 2. API operations
    let mut api_docs: Vec<HDoc> = Vec::new();
    for q in &api.queries {
        let field_list = q.args.iter().map(|a| format!("<span class=\"k-param\">{}{}</span>: {}", h_esc(&a.name), if a.required { "" } else { "?" }, h_api_type(a))).collect::<Vec<_>>().join(", ");
        let input_rel = if q.args.is_empty() {
            "<div class=\"rel\"><span class=\"lbl\">input:</span> <span class=\"muted\">(none)</span></div>".to_string()
        } else {
            format!("<div class=\"rel\"><span class=\"lbl\">input:</span> <span class=\"k-type\">🧩 {}QueryInput{}</span> <span class=\"muted\">{{ {} }}</span></div>", h_esc(&pascal(&q.name)), if q.args.iter().any(|a| a.required) { "!" } else { "" }, field_list)
        };
        let ret = format!("{}{}", if type_set.contains(&q.returns_type) { h_link("type", &q.returns_type) } else if entity_set.contains(&q.returns_type) { h_link("entity", &q.returns_type) } else { format!("<span class=\"k-id\">{}</span>", h_esc(&q.returns_type)) }, if q.returns_list { " []" } else { "" });
        let reads = { let s = q.reads.iter().map(|v| h_link("view", v)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
        let body = format!("{}<div class=\"rel\"><span class=\"lbl\">returns:</span> {} · <span class=\"lbl\">reads</span> {}</div><div class=\"rel\"><span class=\"lbl\">roles:</span> {} · <span class=\"badge\">{}</span></div>", input_rel, ret, reads, h_esc(&q.roles.join(", ")), q.slice);
        let ctx = cx.of_operation(&q.roles, &(if !q.reads.is_empty() { cx.of_reads(&q.reads) } else { cx.of_type(&q.returns_type) }));
        api_docs.push(HDoc { ctx, html: h_item("query", "Query", &q.name, &body, q.description.as_deref()) });
    }
    for m in &api.mutations {
        let h = cmd_handler.get(&m.command);
        let payload = m.payload.iter().map(|f| format!("<span class=\"k-prop\">{}</span>: {}", h_esc(&f.name), h_api_type(f))).collect::<Vec<_>>().join(", ");
        let body = format!("<div class=\"rel\"><span class=\"lbl\">command:</span> {}{}</div><div class=\"rel\"><span class=\"lbl\">roles:</span> {} · <span class=\"badge\">{}</span></div><div class=\"rel\"><span class=\"lbl\">payload:</span> <span class=\"muted\">correlationId</span>{}</div>", h_link("command", &m.command), h.map(|h| format!(" → {}", h_link("actor", &h.0))).unwrap_or_default(), h_esc(&m.roles.join(", ")), m.slice, if payload.is_empty() { String::new() } else { format!(", {}", payload) });
        api_docs.push(HDoc { ctx: cx.of_command(&m.command), html: h_item("mutation", "Mutation", &m.name, &body, None) });
    }
    for s in &api.subscriptions {
        let field_list = s.args.iter().map(|a| format!("<span class=\"k-param\">{}{}</span>: {}", h_esc(&a.name), if a.required { "" } else { "?" }, h_api_type(a))).collect::<Vec<_>>().join(", ");
        let input_rel = if s.args.is_empty() {
            "<div class=\"rel\"><span class=\"lbl\">input:</span> <span class=\"muted\">(none)</span></div>".to_string()
        } else {
            format!("<div class=\"rel\"><span class=\"lbl\">input:</span> <span class=\"k-type\">🧩 {}SubscriptionInput{}</span> <span class=\"muted\">{{ {} }}</span></div>", h_esc(&pascal(&s.name)), if s.args.iter().any(|a| a.required) { "!" } else { "" }, field_list)
        };
        let ret = format!("{}{}", if type_set.contains(&s.returns_type) { h_link("type", &s.returns_type) } else if entity_set.contains(&s.returns_type) { h_link("entity", &s.returns_type) } else { format!("<span class=\"k-id\">{}</span>", h_esc(&s.returns_type)) }, if s.returns_list { " []" } else { "" });
        let body = format!("{}<div class=\"rel\"><span class=\"lbl\">streams:</span> {}</div><div class=\"rel\"><span class=\"lbl\">roles:</span> {} · <span class=\"badge\">{}</span></div>", input_rel, ret, h_esc(&s.roles.join(", ")), s.slice);
        api_docs.push(HDoc { ctx: cx.of_operation(&s.roles, &cx.of_type(&s.returns_type)), html: h_item("subscription", "Subscription", &s.name, &body, s.description.as_deref()) });
    }
    let type_docs: Vec<HDoc> = api.types.iter().map(|t| {
        let reads = t.reads.iter().map(|v| h_link("view", v)).collect::<Vec<_>>().join(", ");
        let rows: Vec<Vec<String>> = t.properties.iter().map(|f| vec![format!("<span id=\"{}\" class=\"k-prop\">{}</span>", dprop_anchor("type", &t.name, &f.name), h_esc(&f.name)), h_api_type(f), h_req_cell(!f.nullable, f.nullable)]).collect();
        let body = format!("<div class=\"rel\"><span class=\"lbl\">read model:</span> {}</div>{}", if reads.is_empty() { "<span class=\"muted\">(within a parent projection)</span>".to_string() } else { reads }, h_table(&["Field", "Type", "Req."], &rows));
        HDoc { ctx: cx.of_type(&t.name), html: h_item("type", "Type", &t.name, &body, t.description.as_deref()) }
    }).collect();

    // 3. Actors — process managers also embed their saga sequence diagram; the <pre class="mermaid">
    // source is rendered client-side by MERMAID_JS and stays readable as text when offline.
    let pm_seq: HashMap<String, String> = pm_sequence_map(model).into_iter().collect();
    let actor_docs: Vec<HDoc> = actors.iter().map(|a| {
        let kind = if a.kind == "aggregate" { "🧩 aggregate" } else { "⚙️ process manager" };
        let rows: Vec<Vec<String>> = a.receives.iter().map(|e| {
            let is_cmd = e.message_ref.starts_with("commands.yaml#/");
            let emits = { let s = e.emits.iter().map(|r| h_link("event", &ref_name(r).unwrap_or_default())).collect::<Vec<_>>().join(", "); if s.is_empty() { e.effect.as_deref().map(|x| format!("<span class=\"muted\">{}</span>", h_esc(x))).unwrap_or_else(|| "—".to_string()) } else { s } };
            let throws = { let s = e.throws.iter().map(|r| h_link("error", &ref_name(r).unwrap_or_default())).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
            vec![h_link(if is_cmd { "command" } else { "event" }, &ref_name(&e.message_ref).unwrap_or_else(|| "?".to_string())), emits, throws]
        }).collect();
        let seq = if a.kind == "aggregate" { String::new() } else {
            pm_seq.get(&a.name).map(|d| format!("<div class=\"pm-seq\"><pre class=\"mermaid\">{}</pre></div>", h_esc(d))).unwrap_or_default()
        };
        HDoc { ctx: cx.of_actor(&a.name), html: h_item("actor", "Actor", &a.name, &format!("<div class=\"rel muted\">{}</div>{}{}", kind, h_table(&["Receives", "Emits →", "Throws"], &rows), seq), a.description.as_deref()) }
    }).collect();

    // 4. Views
    let view_docs: Vec<HDoc> = views.iter().map(|v| {
        let slice = if v.slice == "V1" { "🔭 V1" } else { "🛶 V0" };
        let fed_by = { let s = v.fedby.iter().map(|n| h_link("event", n)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
        let rows: Vec<Vec<String>> = v.columns.iter().map(|c| {
            let type_cell = format!("{}{}{}", if scalar_set.contains(&c.ty) { h_link("scalar", &c.ty) } else { format!("<span class=\"k-const\">{}</span>", h_esc(if c.ty.is_empty() { "?" } else { &c.ty })) }, if c.type_derived { " <span class=\"muted\">(derived)</span>" } else { "" }, c.fk.as_ref().map(|f| format!(" → {}", h_link("view", f.split('.').next().unwrap_or(f)))).unwrap_or_default());
            let src = { let s = c.from.iter().map(|rf| { let segs: Vec<&str> = rf.splitn(2, "#/").nth(1).unwrap_or("").split('/').filter(|x| !x.is_empty()).collect(); let prop = if segs.get(1) == Some(&"properties") { segs.get(2).copied() } else { None }; match prop { Some(p) => h_plink("event", segs.first().copied().unwrap_or(""), p), None => h_link("event", segs.first().copied().unwrap_or("")) } }).collect::<Vec<_>>().join(", "); if s.is_empty() { "<span class=\"hole\">⚠️ none</span>".to_string() } else { s } };
            let flags = { let f: Vec<&str> = [(c.pk, "PK"), (c.unique, "unique"), (c.index, "index"), (c.nullable, "nullable")].iter().filter(|(b, _)| *b).map(|(_, s)| *s).collect(); if f.is_empty() { "—".to_string() } else { f.join(", ") } };
            vec![format!("<span id=\"{}\" class=\"k-prop\">{}</span>", dprop_anchor("view", &v.name, &c.name), h_esc(&c.name)), type_cell, src, flags, h_esc(&ws1(c.note.as_deref().unwrap_or("")))]
        }).collect();
        let body = format!("<div class=\"rel\"><span class=\"lbl\">source:</span> {} · {}{}</div>{}<div class=\"rel\"><span class=\"lbl\">fed by:</span> {}</div>{}", if v.reference { "📦 reference (static seed)".to_string() } else { h_link("actor", &v.aggregate) }, slice, if v.internal { " · 🔒 internal" } else { "" }, v.note.as_deref().map(|n| format!("<div class=\"desc\">{}</div>", h_esc(&ws1(n)))).unwrap_or_default(), fed_by, h_table(&["Column", "Type", "Sourced from", "Constraints", "Notes"], &rows));
        HDoc { ctx: cx.of_view(&v.name), html: h_item("view", "View", &v.name, &body, None) }
    }).collect();

    // 5. Commands
    let cmd_map = model.defs.get("commands.yaml").and_then(|v| v.as_mapping());
    let command_docs: Vec<HDoc> = cmd_map.map(|m| m.iter().filter_map(|(k, _)| k.as_str()).filter(|c| cmd_handler.contains_key(*c)).map(|c| {
        let h = cmd_handler.get(c).unwrap();
        let mutn = mut_by_command.get(c);
        let def = cmd_map.and_then(|m| m.get(c)).cloned().unwrap_or(Value::Null);
        let body = format!("<div class=\"rel\"><span class=\"lbl\">dispatched by:</span> {} · <span class=\"lbl\">handled by</span> {}</div><div class=\"rel\"><span class=\"lbl\">emits:</span> {}</div><div class=\"rel\"><span class=\"lbl\">throws:</span> {}</div>{}",
            mutn.map(|m| h_link("mutation", m)).unwrap_or_else(|| "—".to_string()), h_link("actor", &h.0),
            { let s = h.1.iter().map(|e| h_link("event", e)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } },
            { let s = h.2.iter().map(|e| h_link("error", e)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } },
            h_table(&["Field", "Type", "Req.", "Description"], &h_prop_rows(&def, "command", c)));
        HDoc { ctx: cx.of_command(c), html: h_item("command", "Command", c, &body, Some(&doc_desc(model, "commands.yaml", c))) }
    }).collect()).unwrap_or_default();

    // 6. Events
    let non_projected: HashSet<String> = ref_names(model.defs.get("database/projection_views.yaml").and_then(|v| v.get("nonProjectedEvents"))).into_iter().collect();
    let evt_map = model.defs.get("events.yaml").and_then(|v| v.as_mapping());
    let event_docs: Vec<HDoc> = evt_map.map(|m| m.iter().filter_map(|(k, _)| k.as_str()).map(|ev| {
        let def = evt_map.and_then(|m| m.get(ev)).cloned().unwrap_or(Value::Null);
        let projected = { let s = evt_views.get(ev).map(|vs| vs.iter().map(|v| h_link("view", v)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if !s.is_empty() { s } else if non_projected.contains(ev) { "<span class=\"muted\">non-projected</span>".to_string() } else { "—".to_string() } };
        let body = format!("<div class=\"rel\"><span class=\"lbl\">emitted by:</span> {}</div><div class=\"rel\"><span class=\"lbl\">consumed by:</span> {}</div><div class=\"rel\"><span class=\"lbl\">projected into:</span> {}</div>{}",
            { let s = evt_emitted_by.get(ev).map(|a| a.iter().map(|x| h_link("actor", x)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "<span class=\"muted\">inbound / external</span>".to_string() } else { s } },
            { let s = evt_consumed_by.get(ev).map(|a| a.iter().map(|x| h_link("actor", x)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "—".to_string() } else { s } },
            projected, h_table(&["Field", "Type", "Req.", "Description"], &h_prop_rows(&def, "event", ev)));
        HDoc { ctx: cx.of_event(ev), html: h_item("event", "Event", ev, &body, Some(&doc_desc(model, "events.yaml", ev))) }
    }).collect()).unwrap_or_default();

    // 7. Entities
    let ent_map = model.defs.get("entities.yaml").and_then(|v| v.as_mapping());
    let entity_docs: Vec<HDoc> = ent_map.map(|m| m.iter().filter_map(|(k, _)| k.as_str()).map(|e| {
        let def = ent_map.and_then(|m| m.get(e)).cloned().unwrap_or(Value::Null);
        HDoc { ctx: cx.of_entity(e), html: h_item("entity", "Entity", e, &h_table(&["Field", "Type", "Req.", "Description"], &h_prop_rows(&def, "entity", e)), Some(&doc_desc(model, "entities.yaml", e))) }
    }).collect()).unwrap_or_default();

    // 8. Scalars
    let scalar_rows: Vec<HRow> = model.defs.get("scalars.yaml").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, d)| k.as_str().map(|name| {
        let mut t = format!("<span class=\"k-const\">{}</span>", h_esc(d.get("type").and_then(|x| x.as_str()).unwrap_or("?")));
        if let Some(en) = d.get("enum").and_then(|x| x.as_sequence()) {
            t = format!("<span class=\"kw\">enum</span> <span class=\"muted\">({})</span>", en.iter().filter_map(|v| v.as_str()).map(h_esc).collect::<Vec<_>>().join(" | "));
        } else if let Some(fmt) = d.get("format").and_then(|x| x.as_str()) {
            t += &format!(" <span class=\"muted\">{}</span>", h_esc(fmt));
        } else if let Some(pat) = d.get("pattern").and_then(|x| x.as_str()) {
            t += &format!(" <span class=\"muted\">{}</span>", h_esc(pat));
        }
        HRow { ctx: cx.of_scalar(name), cells: vec![format!("<span id=\"{}\" class=\"k-scalar\">{} {}</span>", danchor("scalar", name), d_emo("scalar"), h_esc(name)), t, h_esc(&ws1(d.get("description").and_then(|x| x.as_str()).unwrap_or("")))] }
    })).collect()).unwrap_or_default();

    // 9. Errors
    let error_rows: Vec<HRow> = model.defs.get("errors.yaml").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, d)| k.as_str().map(|name| {
        let msgs = d.get("messages");
        let en = msgs.and_then(|x| x.get("en")).and_then(|x| x.as_str()).unwrap_or("");
        let fr = msgs.and_then(|x| x.get("fr")).and_then(|x| x.as_str()).unwrap_or("");
        let by = { let s = err_thrown_by.get(name).map(|c| c.iter().map(|x| h_link("command", x)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "—".to_string() } else { s } };
        HRow { ctx: cx.of_error(name), cells: vec![format!("<span id=\"{}\" class=\"k-error\">{} {}</span>", danchor("error", name), d_emo("error"), h_esc(name)), h_esc(&ws1(d.get("description").and_then(|x| x.as_str()).unwrap_or(""))), format!("🇬🇧 {}", h_esc(en)), format!("🇫🇷 {}", h_esc(fr)), by] }
    })).collect()).unwrap_or_default();

    // rules ↔ tests
    let rule_defs = model.defs.get("rules.yaml").and_then(|v| v.as_mapping());
    let tests_map = model.defs.get("tests.yaml").and_then(|v| v.get("tests")).and_then(|v| v.as_mapping());
    let fixtures_map = model.defs.get("tests.yaml").and_then(|v| v.get("fixtures")).and_then(|v| v.as_mapping());
    let rules_of_test = |t: &Value| -> Vec<String> { t.get("rules").and_then(|x| x.as_sequence()).map(|s| s.iter().filter_map(|r| r.get("$ref").and_then(|x| x.as_str()).and_then(ref_name)).collect()).unwrap_or_default() };
    let mut rule_tests: HashMap<String, Vec<String>> = HashMap::new();
    let mut test_actor_name: HashMap<String, String> = HashMap::new();
    if let Some(tm) = tests_map {
        for (k, t) in tm {
            if let Some(tn) = k.as_str() {
                test_actor_name.insert(tn.to_string(), ref_name(t.get("actor").and_then(|a| a.get("$ref")).and_then(|x| x.as_str()).unwrap_or("")).unwrap_or_default());
                for rn in rules_of_test(t) { let e = rule_tests.entry(rn).or_default(); if !e.contains(&tn.to_string()) { e.push(tn.to_string()); } }
            }
        }
    }
    let fx_event = |fx_ref: &str| -> Option<String> { let key = fx_ref.rsplit('/').next().unwrap_or(""); fixtures_map.and_then(|m| m.get(key)).and_then(|fx| fx.get("type")).and_then(|t| t.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name) };
    let ev_links = |arr: Option<&Value>| -> String { arr.and_then(|x| x.as_sequence()).map(|s| s.iter().map(|it| it.get("$ref").and_then(|x| x.as_str()).and_then(|r| fx_event(r)).map(|e| h_link("event", &e)).unwrap_or_else(|| "—".to_string())).collect::<Vec<_>>().join(", ")).unwrap_or_default() };
    let test_docs: Vec<HDoc> = actors.iter().filter_map(|a| {
        let entries: Vec<(String, Value)> = tests_map.map(|m| m.iter().filter(|(_, t)| ref_name(t.get("actor").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("")).as_deref() == Some(a.name.as_str())).filter_map(|(k, t)| k.as_str().map(|s| (s.to_string(), t.clone()))).collect()).unwrap_or_default();
        if entries.is_empty() { return None; }
        let cases = entries.iter().map(|(name, t)| {
            let cmd = ref_name(t.get("when").and_then(|w| w.get("type")).and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("")).unwrap_or_else(|| "?".to_string());
            let given = { let g = t.get("given"); if g.and_then(|x| x.as_sequence()).map(|s| !s.is_empty()).unwrap_or(false) { ev_links(g) } else { "<span class=\"muted\">(none)</span>".to_string() } };
            let has_thrown = t.get("thrown").is_some();
            let outcome = if has_thrown {
                format!("<div class=\"rel\"><span class=\"lbl\">thrown:</span> {}</div>", { let s = t.get("thrown").and_then(|x| x.as_sequence()).map(|arr| arr.iter().filter_map(|r| r.get("$ref").and_then(|x| x.as_str()).and_then(ref_name)).map(|e| h_link("error", &e)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "—".to_string() } else { s } })
            } else {
                let then_arr = t.get("then");
                format!("<div class=\"rel\"><span class=\"lbl\">then:</span> {}</div>", if then_arr.and_then(|x| x.as_sequence()).map(|s| !s.is_empty()).unwrap_or(false) { ev_links(then_arr) } else { "<span class=\"k-const\">∅ no event (idempotent no-op)</span>".to_string() })
            };
            let rules = rules_of_test(t).iter().map(|rn| h_link("rule", rn)).collect::<Vec<_>>().join(", ");
            let body = format!("<div class=\"rel\"><span class=\"lbl\">given:</span> {}</div><div class=\"rel\"><span class=\"lbl\">when:</span> {}</div>{}{}", given, h_link("command", &cmd), outcome, if rules.is_empty() { String::new() } else { format!("<div class=\"rel\"><span class=\"lbl\">verifies:</span> {}</div>", rules) });
            h_item("test", "Test", name, &body, t.get("name").and_then(|x| x.as_str()))
        }).collect::<Vec<_>>().join("");
        Some(HDoc { ctx: cx.of_actor(&a.name), html: format!("<h3>{}</h3>{}", h_link("actor", &a.name), cases) })
    }).collect();
    let rule_docs: Vec<HDoc> = rule_defs.map(|m| m.iter().filter_map(|(k, r)| k.as_str().map(|name| {
        let tns = rule_tests.get(name).cloned().unwrap_or_default();
        let ctx = tns.first().map(|tn| cx.of_actor(test_actor_name.get(tn).map(|s| s.as_str()).unwrap_or(""))).unwrap_or_else(|| CROSS.to_string());
        let verified_by = { let s = tns.iter().map(|tn| h_link("test", tn)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
        HDoc { ctx, html: h_item("rule", "Rule", name, &format!("<div class=\"rel\"><span class=\"lbl\">verified by:</span> {}</div>", verified_by), Some(&ws1(r.get("description").and_then(|x| x.as_str()).unwrap_or("").trim()))) }
    })).collect()).unwrap_or_default();

    // 11. Observability
    let obs_docs: Vec<HDoc> = model.defs.get("observability.yaml").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, c)| k.as_str().map(|feature| {
        let wf = c.get("workflow");
        let id_rows: Vec<Vec<String>> = c.get("run_identity").and_then(|x| x.as_sequence()).map(|s| s.iter().map(|i| vec![format!("<span class=\"k-prop\">{}</span>", h_esc(i.get("name").and_then(|x| x.as_str()).unwrap_or(""))), format!("<span class=\"muted\">{}</span>", h_esc(i.get("source").and_then(|x| x.as_str()).unwrap_or(""))), if i.get("required").and_then(|x| x.as_bool()) == Some(true) { "<span class=\"req\">✅</span>".into() } else { "<span class=\"opt\">⬜</span>".into() }, i.get("businessKey").and_then(|b| b.get("$ref")).and_then(|x| x.as_str()).map(h_any_link).unwrap_or_else(|| "—".to_string())]).collect()).unwrap_or_default();
        let span_rows: Vec<Vec<String>> = c.get("spans").and_then(|x| x.as_sequence()).map(|s| s.iter().map(|sp| { let a = sp.get("attributes").and_then(|x| x.as_sequence()).map(|at| at.iter().map(|x| format!("<span class=\"k-prop\">{}</span>{}", h_esc(x.get("key").and_then(|k| k.as_str()).unwrap_or("")), if x.get("required").and_then(|r| r.as_bool()) == Some(true) { "<span class=\"req\">*</span>" } else { "" })).collect::<Vec<_>>().join(", ")).unwrap_or_default(); let a = if a.is_empty() { "—".to_string() } else { a }; vec![format!("<span class=\"k-op\">{}</span>", h_esc(sp.get("name").and_then(|x| x.as_str()).unwrap_or(""))), format!("<span class=\"kw\">{}</span>", h_esc(sp.get("kind").and_then(|x| x.as_str()).unwrap_or(""))), if sp.get("required").and_then(|x| x.as_bool()) == Some(true) { "<span class=\"req\">✅</span>".into() } else { "<span class=\"opt\">⬜</span>".into() }, sp.get("multiplicity").and_then(|x| x.as_str()).map(|mu| format!("<span class=\"muted\">{}</span>", h_esc(mu))).unwrap_or_else(|| "—".to_string()), a] }).collect()).unwrap_or_default();
        let metric_list = |key: &str| -> String { let s = c.get(key).and_then(|x| x.as_sequence()).map(|arr| arr.iter().map(|mm| format!("<span class=\"k-const\">{}</span> <span class=\"muted\">({})</span>", h_esc(mm.get("name").and_then(|x| x.as_str()).unwrap_or("")), h_esc(mm.get("type").and_then(|x| x.as_str()).unwrap_or("")))).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "—".to_string() } else { s } };
        let req_spans = c.get("status_rules").and_then(|x| x.get("success")).and_then(|x| x.get("required_spans")).and_then(|x| x.as_sequence()).map(|a| a.iter().filter_map(|x| x.as_str()).map(|s| format!("<span class=\"k-op\">{}</span>", h_esc(s))).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        let s3 = |v: Option<&Value>, k: &str| c.get(v.map(|_| "").unwrap_or("")).map(|_| "").unwrap_or("").to_string() + &{ let node = c.get(k); let _ = node; String::new() };
        let _ = s3;
        let slo = |group: &str, key: &str| -> String { c.get(group).and_then(|g| g.get(key)).map(|x| if let Some(n) = x.as_i64() { n.to_string() } else if let Some(f) = x.as_f64() { f.to_string() } else { x.as_str().unwrap_or("—").to_string() }).unwrap_or_else(|| "—".to_string()) };
        let cmd = ref_name(wf.and_then(|w| w.get("command")).and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or(""));
        let saga = ref_name(wf.and_then(|w| w.get("saga")).and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or(""));
        let ctx = if let Some(c) = &cmd { cx.of_command(c) } else if let Some(s) = &saga { cx.of_actor(s) } else { CROSS.to_string() };
        let body = format!(
            "<div class=\"rel\"><span class=\"lbl\">workflow:</span> {}{}</div><div class=\"rel\"><span class=\"lbl\">emits:</span> {} · <span class=\"lbl\">inbound:</span> {}</div>{}{}<div class=\"rel\"><span class=\"lbl\">metrics:</span> {} · <span class=\"lbl\">business:</span> {}</div>{}<div class=\"rel\"><span class=\"lbl\">SLOs:</span> p95 ≤ {}ms · p99 ≤ {}ms · error ≤ {}%</div>",
            wf.and_then(|w| w.get("saga")).map(|s| format!("saga {}", h_any_link(s.get("$ref").and_then(|x| x.as_str()).unwrap_or_default()))).unwrap_or_default(),
            wf.and_then(|w| w.get("command")).map(|c| format!(" · command {}", h_any_link(c.get("$ref").and_then(|x| x.as_str()).unwrap_or_default()))).unwrap_or_default(),
            h_ref_links(wf.and_then(|w| w.get("emits"))), h_ref_links(wf.and_then(|w| w.get("inbound"))),
            if id_rows.is_empty() { String::new() } else { format!("<div class=\"rel\"><span class=\"lbl\">run identity</span></div>{}", h_table(&["Id", "Source", "Req.", "Business key"], &id_rows)) },
            if span_rows.is_empty() { String::new() } else { format!("<div class=\"rel\"><span class=\"lbl\">spans</span> <span class=\"muted\">(* = required attribute)</span></div>{}", h_table(&["Span", "Kind", "Req.", "Multiplicity", "Attributes"], &span_rows)) },
            metric_list("metrics"), metric_list("business_metrics"),
            if req_spans.is_empty() { String::new() } else { format!("<div class=\"rel\"><span class=\"lbl\">success ⇐ spans:</span> {}</div>", req_spans) },
            slo("latency_budget", "max_p95_ms"), slo("latency_budget", "max_p99_ms"), slo("error_budget", "max_error_rate_pct")
        );
        HDoc { ctx, html: h_item("obs", "Contract", feature, &body, Some(&format!("criticality: {}", c.get("criticality").and_then(|x| x.as_str()).unwrap_or("—")))) }
    })).collect()).unwrap_or_default();

    // 12. C4
    let l2 = model.defs.get("architecture/c4-l2.yaml");
    let l3 = model.defs.get("architecture/c4-l3.yaml");
    let sysn = l2.and_then(|v| v.get("system")).and_then(|s| s.get("name")).and_then(|x| x.as_str()).unwrap_or("Captain.Food");
    let sysd = l2.and_then(|v| v.get("system")).and_then(|s| s.get("description")).and_then(|x| x.as_str()).unwrap_or("");
    let mrows = |sect: &str, f: &dyn Fn(&str, &Value) -> Vec<String>| -> Vec<Vec<String>> { l2.and_then(|v| v.get(sect)).and_then(|x| x.as_mapping()).map(|m| m.iter().filter_map(|(k, v)| k.as_str().map(|n| f(n, v))).collect()).unwrap_or_default() };
    let bc_rows = mrows("boundedContexts", &|n, bc| vec![format!("{} <span class=\"k-type\">{}</span>", d_emo("context"), h_esc(n)), h_esc(bc.get("description").and_then(|x| x.as_str()).unwrap_or("")), format!("{}{}", h_ref_links(bc.get("aggregates")), if bc.get("processManagers").is_some() { format!(" · {}", h_ref_links(bc.get("processManagers"))) } else { String::new() })]);
    let c_rows = mrows("containers", &|n, c| vec![format!("{} <span class=\"k-type\">{}</span>", d_emo("container"), h_esc(n)), format!("<span class=\"muted\">{}</span>", h_esc(c.get("technology").and_then(|x| x.as_str()).unwrap_or(""))), format!("{}{}", h_esc(c.get("description").and_then(|x| x.as_str()).unwrap_or("")), if c.get("realizes").is_some() { format!("<br>realizes: {}", h_ref_links(c.get("realizes"))) } else { String::new() })]);
    let x_rows = mrows("externalSystems", &|n, x| vec![format!("🔌 <span class=\"k-id\">{}</span>", h_esc(n)), h_esc(x.get("description").and_then(|d| d.as_str()).unwrap_or(""))]);
    let rel_rows: Vec<Vec<String>> = l2.and_then(|v| v.get("relationships")).and_then(|x| x.as_sequence()).map(|s| s.iter().map(|r| vec![format!("<span class=\"k-id\">{}</span> → <span class=\"k-id\">{}</span>", h_esc(r.get("from").and_then(|x| x.as_str()).unwrap_or("")), h_esc(r.get("to").and_then(|x| x.as_str()).unwrap_or(""))), h_esc(r.get("description").and_then(|x| x.as_str()).unwrap_or(""))]).collect()).unwrap_or_default();
    let comp_rows: Vec<Vec<String>> = l3.and_then(|v| v.get("components")).and_then(|x| x.as_mapping()).map(|m| m.iter().filter_map(|(k, c)| k.as_str().map(|n| { let bind = if c.get("handles").is_some() { format!("handles {}", h_ref_links(c.get("handles"))) } else if c.get("updates").is_some() { format!("updates {}", h_ref_links(c.get("updates"))) } else { "—".to_string() }; vec![format!("{} <span class=\"k-op\">{}</span>", d_emo("component"), h_esc(n)), if c.get("instrumented").and_then(|x| x.as_bool()) == Some(true) { "📡 yes".to_string() } else { "<span class=\"muted\">— no</span>".to_string() }, h_esc(c.get("description").and_then(|x| x.as_str()).unwrap_or("")), bind] })).collect()).unwrap_or_default();
    let c4_html = format!("<div class=\"rel\"><span class=\"lbl\">system:</span> <span class=\"k-type\">{}</span> — {}</div><h3>🔲 L2 — Bounded contexts</h3>{}<h3>🧱 L2 — Containers</h3>{}<h3>🔌 L2 — External systems</h3>{}<h3>➡️ L2 — Relationships</h3>{}<h3>⚙️ L3 — Components of the api container</h3>{}",
        h_esc(sysn), h_esc(sysd),
        h_table(&["Context", "Description", "Aggregates / process managers"], &bc_rows),
        h_table(&["Container", "Technology", "Description"], &c_rows),
        h_table(&["System", "Description"], &x_rows),
        h_table(&["Edge", "Description"], &rel_rows),
        h_table(&["Component", "Instrumented", "Description", "Binds"], &comp_rows));

    // 13. Interactive map data
    let sf = model.defs.get("screens/customer_screens.yaml");
    let l2m = |k: &str| l2.and_then(|v| v.get(k));
    let contexts_j: Vec<serde_json::Value> = l2m("boundedContexts").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, bc)| k.as_str().map(|id| serde_json::json!({"id": id, "description": bc.get("description").and_then(|x| x.as_str()).unwrap_or(""), "aggregates": ref_names(bc.get("aggregates")), "processManagers": ref_names(bc.get("processManagers"))}))).collect()).unwrap_or_default();
    let containers_j: Vec<serde_json::Value> = l2m("containers").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, c)| k.as_str().map(|id| serde_json::json!({"id": id, "technology": c.get("technology").and_then(|x| x.as_str()).unwrap_or(""), "description": c.get("description").and_then(|x| x.as_str()).unwrap_or(""), "realizes": ref_names(c.get("realizes"))}))).collect()).unwrap_or_default();
    let externals_j: Vec<serde_json::Value> = l2m("externalSystems").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, x)| k.as_str().map(|id| serde_json::json!({"id": id, "description": x.get("description").and_then(|d| d.as_str()).unwrap_or("")}))).collect()).unwrap_or_default();
    let rels_j: Vec<serde_json::Value> = l2m("relationships").and_then(|x| x.as_sequence()).map(|s| s.iter().map(|r| serde_json::json!({"from": r.get("from").and_then(|x| x.as_str()).unwrap_or(""), "to": r.get("to").and_then(|x| x.as_str()).unwrap_or(""), "description": r.get("description").and_then(|x| x.as_str()).unwrap_or("")})).collect()).unwrap_or_default();
    let mut actors_obj = serde_json::Map::new();
    for a in &actors {
        let receives: Vec<serde_json::Value> = a.receives.iter().map(|e| serde_json::json!({"message": ref_name(&e.message_ref), "isCommand": e.message_ref.starts_with("commands.yaml#/"), "emits": e.emits.iter().filter_map(|r| ref_name(r)).collect::<Vec<_>>(), "throws": e.throws.iter().filter_map(|r| ref_name(r)).collect::<Vec<_>>()})).collect();
        actors_obj.insert(a.name.clone(), serde_json::json!({"type": a.kind, "receives": receives}));
    }
    let views_j: Vec<serde_json::Value> = views.iter().map(|v| serde_json::json!({"name": v.name, "fedBy": v.fedby.clone()})).collect();
    let map_data = serde_json::json!({"system": {"name": sysn, "description": sysd}, "contexts": contexts_j, "containers": containers_j, "externals": externals_j, "relationships": rels_j, "actors": serde_json::Value::Object(actors_obj), "views": views_j});
    let map_html = format!("<div class=\"cfmap\"><div class=\"cfmap-bar\"><button id=\"cf-back\">◀ back</button> <span id=\"cf-crumb\" class=\"muted\"></span></div><svg id=\"cf-svg\" viewBox=\"0 0 960 560\" preserveAspectRatio=\"xMidYMid meet\" role=\"img\" aria-label=\"Captain.Food system map\"></svg><div id=\"cf-info\" class=\"cfmap-info muted\"></div></div><script>{}</script>", MAP_JS.replace("__CF_DATA__", &serde_json::to_string(&map_data).unwrap()));

    // legend + toc
    let legend = [
        format!("{} <span class=\"k-op\">query</span>", d_emo("query")), format!("{} <span class=\"k-op\">mutation</span>", d_emo("mutation")), format!("{} <span class=\"k-op\">subscription</span>", d_emo("subscription")),
        format!("{} <span class=\"k-type\">type</span>", d_emo("type")), format!("{} <span class=\"k-type\">actor</span>", d_emo("actor")),
        format!("{} <span class=\"k-type\">view</span>", d_emo("view")), format!("{} <span class=\"k-op\">command</span>", d_emo("command")),
        format!("{} <span class=\"k-event\">event</span>", d_emo("event")), format!("{} <span class=\"k-type\">entity</span>", d_emo("entity")),
        format!("{} <span class=\"k-scalar\">scalar</span>", d_emo("scalar")), format!("{} <span class=\"k-error\">error</span>", d_emo("error")),
        "🔹 <span class=\"k-prop\">property</span>".to_string(), "<span class=\"k-param\">parameter</span>".to_string(), format!("{} <span class=\"k-scalar\">rule</span>", d_emo("rule")), format!("{} <span class=\"k-op\">test</span>", d_emo("test")), format!("{} <span class=\"k-type\">screen</span>", d_emo("screen")), format!("{} <span class=\"k-scalar\">translation</span>", d_emo("translation")), format!("{} <span class=\"k-event\">observability</span>", d_emo("obs")),
    ].join(" · ");

    // SDUI screens + translations
    let resolvers = sf.and_then(|v| v.get("resolvers")).and_then(|v| v.as_mapping());
    let action_defs = sf.and_then(|v| v.get("actions")).and_then(|v| v.as_mapping());
    let tr_defs = model.defs.get("translations.yaml").and_then(|v| v.as_mapping());
    let tr_en = |rf: &str| -> String { resolve_ref(model, rf, "screens/customer_screens.yaml").and_then(|t| t.get("messages")).and_then(|m| m.get("en")).and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| rf.rsplit('/').next().unwrap_or(rf).to_string()) };
    let t_text = |v: &Value| -> String { if let Some(rf) = v.get("$ref").and_then(|x| x.as_str()) { tr_en(rf) } else if let Some(s) = v.as_str() { s.to_string() } else { String::new() } };
    let tr_rows: Vec<Vec<String>> = tr_defs.map(|m| m.iter().filter_map(|(k, t)| k.as_str().map(|key| { let params = t.get("params").and_then(|x| x.as_mapping()).map(|pm| pm.iter().filter_map(|(pk, _)| pk.as_str().map(|p| format!("<span class=\"k-param\">{}</span>", h_esc(p)))).collect::<Vec<_>>().join(", ")).unwrap_or_default(); let params = if params.is_empty() { "<span class=\"muted\">—</span>".to_string() } else { params }; vec![format!("<span id=\"{}\" class=\"k-scalar\">{} {}</span>", danchor("translation", key), d_emo("translation"), h_esc(key)), params, format!("🇬🇧 {}", h_esc(t.get("messages").and_then(|mm| mm.get("en")).and_then(|x| x.as_str()).unwrap_or(""))), format!("🇫🇷 {}", h_esc(t.get("messages").and_then(|mm| mm.get("fr")).and_then(|x| x.as_str()).unwrap_or("")))] })).collect()).unwrap_or_default();
    let translations_html = h_table(&["Key", "Params", "en", "fr"], &tr_rows);
    let op_link = |rf: Option<&str>, gap: Option<&str>| -> String { if let Some(g) = gap { return format!("<span class=\"opt\">⚠️ {}</span>", h_esc(g)); } match rf { None => "—".to_string(), Some(rf) => { let name = rf.rsplit('/').next().unwrap_or(""); let kind = if rf.contains("/mutations/") { "mutation" } else if rf.contains("/subscriptions/") { "subscription" } else { "query" }; h_link(kind, name) } } };
    let action_keys: HashSet<String> = action_defs.map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    fn collect_action_types(node: &Value, keys: &HashSet<String>, acc: &mut Vec<String>) {
        match node {
            Value::Sequence(s) => s.iter().for_each(|n| collect_action_types(n, keys, acc)),
            Value::Mapping(m) => { if let Some(t) = m.get(Value::String("type".into())).and_then(|x| x.as_str()) { if keys.contains(t) && !acc.contains(&t.to_string()) { acc.push(t.to_string()); } } for (_, v) in m { collect_action_types(v, keys, acc); } }
            _ => {}
        }
    }
    let screens_arr = sf.and_then(|v| v.get("screens")).and_then(|x| x.as_sequence()).cloned().unwrap_or_default();
    let screens_html: String = screens_arr.iter().map(|s| {
        let id = s.get("id").and_then(|x| x.as_str()).unwrap_or("?");
        let route = s.get("route").and_then(|x| x.as_str()).unwrap_or("");
        let title = { let t = s.get("title").map(|v| t_text(v)).unwrap_or_default(); if t.is_empty() { id.to_string() } else { t } };
        let not_sdui = s.get("sdui").and_then(|x| x.as_bool()) == Some(false);
        let badge = if not_sdui { "<span class=\"badge\">🚫 not SDUI</span>".to_string() } else { "<span class=\"badge\">📱 SDUI</span>".to_string() };
        let auth = if s.get("requires_auth").and_then(|x| x.as_bool()) == Some(true) { "<span class=\"badge\">🔒 auth</span>" } else { "" };
        let reason = if not_sdui { s.get("sdui_reason").and_then(|x| x.as_str()).map(|r| format!("<div class=\"desc\">{}</div>", h_esc(r))).unwrap_or_default() } else { String::new() };
        let mock_rows = s.get("components").and_then(|x| x.as_sequence()).map(|comps| comps.iter().map(|c| { let t = if let Some(cp) = c.get("component").and_then(|x| x.as_str()) { format!("«{}»", cp) } else { c.get("type").and_then(|x| x.as_str()).unwrap_or("?").to_string() }; let lbl = c.get("title").map(|v| t_text(v)).filter(|s| !s.is_empty()).or_else(|| c.get("label").map(|v| t_text(v)).filter(|s| !s.is_empty())).or_else(|| c.get("placeholder").map(|v| t_text(v)).filter(|s| !s.is_empty())).unwrap_or_default(); format!("<div style=\"padding:5px 10px;border-top:1px solid var(--line)\"><span class=\"muted\">{}</span>{}</div>", h_esc(&t), if lbl.is_empty() { String::new() } else { format!(" {}", h_esc(&lbl)) }) }).collect::<Vec<_>>().join("")).unwrap_or_default();
        let mock = format!("<div style=\"border:1px solid var(--line);border-radius:12px;max-width:340px;overflow:hidden;margin:8px 0\"><div style=\"background:var(--bg3);padding:7px 10px;font-weight:600\">📱 {}<span class=\"muted\"> · {}</span></div>{}</div>", h_esc(&title), h_esc(route), mock_rows);
        let mut rows: Vec<Vec<String>> = Vec::new();
        for rn in s.get("data_requirements").and_then(|x| x.as_sequence()).map(|s| s.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect::<Vec<_>>()).unwrap_or_default() {
            let r = resolvers.and_then(|m| m.get(rn.as_str()));
            rows.push(vec!["<span class=\"muted\">read</span>".to_string(), format!("<span class=\"k-op\">{}</span>", h_esc(&rn)), op_link(r.and_then(|x| x.get("query")).and_then(|q| q.get("$ref")).and_then(|x| x.as_str()), r.and_then(|x| x.get("gap")).and_then(|x| x.as_str()))]);
        }
        let mut acts: Vec<String> = Vec::new();
        if let Some(comps) = s.get("components") { collect_action_types(comps, &action_keys, &mut acts); }
        for a in s.get("actions_used").and_then(|x| x.as_sequence()).map(|s| s.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect::<Vec<_>>()).unwrap_or_default() { if !acts.contains(&a) { acts.push(a); } }
        for a in &acts {
            let ad = action_defs.and_then(|m| m.get(a.as_str()));
            if ad.map(|x| x.get("mutation").is_some() || x.get("gap").is_some()).unwrap_or(false) {
                rows.push(vec!["<span class=\"muted\">write</span>".to_string(), format!("<span class=\"k-op\">{}</span>", h_esc(a)), op_link(ad.and_then(|x| x.get("mutation")).and_then(|q| q.get("$ref")).and_then(|x| x.as_str()), ad.and_then(|x| x.get("gap")).and_then(|x| x.as_str()))]);
            }
        }
        let ops_table = h_table(&["", "UI need", "GraphQL operation"], &rows);
        let gaps = s.get("gaps").and_then(|x| x.as_sequence()).map(|g| g.iter().filter_map(|x| x.as_str()).map(|g| format!("<li>⚠️ {}</li>", h_esc(g))).collect::<Vec<_>>().join("")).unwrap_or_default();
        let body = format!("{}<div style=\"display:flex;gap:20px;flex-wrap:wrap;align-items:flex-start\">{}<div style=\"flex:1;min-width:280px\">{}{}</div></div>", reason, mock, ops_table, if gaps.is_empty() { String::new() } else { format!("<p class=\"muted\">Gaps</p><ul>{}</ul>", gaps) });
        format!("<details class=\"item\" id=\"{}\" data-crumb=\"{} {}\" open><summary><span class=\"tw\">▸</span><span class=\"muted\">Screen:</span> <span class=\"k-type\">{} {}</span> <span class=\"muted\">{}</span> {}{}<a class=\"perma\" href=\"#{}\">🔗</a></summary>{}</details>", danchor("screen", id), d_emo("screen"), h_esc(id), d_emo("screen"), h_esc(id), h_esc(route), badge, auth, danchor("screen", id), body)
    }).collect();

    // descIndex (insertion order preserved via serde_json preserve_order Map)
    let mut desc_map = serde_json::Map::new();
    let mut put = |k: &str, name: &str, val: &str| { desc_map.insert(danchor(k, name), serde_json::Value::String(ws1(val.trim()))); };
    if let Some(m) = model.defs.get("scalars.yaml").and_then(|v| v.as_mapping()) { for (k, d) in m { if let Some(n) = k.as_str() { put("scalar", n, d.get("description").and_then(|x| x.as_str()).unwrap_or("")); } } }
    if let Some(m) = ent_map { for (k, _) in m { if let Some(n) = k.as_str() { let d = doc_desc(model, "entities.yaml", n); put("entity", n, &d); } } }
    if let Some(m) = evt_map { for (k, _) in m { if let Some(n) = k.as_str() { let d = doc_desc(model, "events.yaml", n); put("event", n, &d); } } }
    if let Some(m) = cmd_map { for (k, _) in m { if let Some(n) = k.as_str() { let d = doc_desc(model, "commands.yaml", n); put("command", n, &d); } } }
    if let Some(m) = model.defs.get("errors.yaml").and_then(|v| v.as_mapping()) { for (k, d) in m { if let Some(n) = k.as_str() { put("error", n, d.get("description").and_then(|x| x.as_str()).unwrap_or("")); } } }
    for a in &actors { put("actor", &a.name, a.description.as_deref().unwrap_or("")); }
    for v in &views { put("view", &v.name, v.note.as_deref().unwrap_or("")); }
    for t in &api.types { put("type", &t.name, t.description.as_deref().unwrap_or("")); }
    for q in &api.queries { put("query", &q.name, q.description.as_deref().unwrap_or("")); }
    for m in &api.mutations { let d = doc_desc(model, "commands.yaml", &m.command); put("mutation", &m.name, &d); }
    for s in &api.subscriptions { put("subscription", &s.name, s.description.as_deref().unwrap_or("")); }
    if let Some(m) = model.defs.get("observability.yaml").and_then(|v| v.as_mapping()) { for (k, c) in m { if let Some(f) = k.as_str() { let s = format!("Observability contract — criticality: {}.", c.get("criticality").and_then(|x| x.as_str()).unwrap_or("—")); put("obs", f, &s); } } }
    if let Some(m) = rule_defs { for (k, d) in m { if let Some(n) = k.as_str() { put("rule", n, d.get("description").and_then(|x| x.as_str()).unwrap_or("")); } } }
    if let Some(m) = tr_defs { for (k, t) in m { if let Some(key) = k.as_str() { let s = format!("{} / {}", t.get("messages").and_then(|mm| mm.get("en")).and_then(|x| x.as_str()).unwrap_or(""), t.get("messages").and_then(|mm| mm.get("fr")).and_then(|x| x.as_str()).unwrap_or("")); put("translation", key, &s); } } }
    for s in &screens_arr { if let Some(id) = s.get("id").and_then(|x| x.as_str()) { let msg = format!("{}screen {}", if s.get("sdui").and_then(|x| x.as_bool()) == Some(false) { "Non-SDUI " } else { "SDUI " }, s.get("route").and_then(|x| x.as_str()).unwrap_or("")); put("screen", id, &msg); } }
    drop(put);
    let desc_script = format!("<script>window.CF_DESC={};</script>", serde_json::to_string(&serde_json::Value::Object(desc_map)).unwrap().replace('<', "\\u003c"));

    // assembly
    let in_ctx = |docs: &[HDoc], ctx: &str| -> String { docs.iter().filter(|d| d.ctx == ctx).map(|d| d.html.clone()).collect::<Vec<_>>().join("") };
    let doc_sub = |emoji: &str, title: &str, docs: &[HDoc], ctx: &str| -> String { let n = docs.iter().filter(|d| d.ctx == ctx).count(); if n == 0 { String::new() } else { h_subsec(emoji, title, n, &in_ctx(docs, ctx)) } };
    let table_sub = |emoji: &str, title: &str, head: &[&str], rows: &[HRow], ctx: &str| -> String { let r: Vec<&HRow> = rows.iter().filter(|x| x.ctx == ctx).collect(); if r.is_empty() { String::new() } else { h_subsec(emoji, title, r.len(), &h_table(head, &r.iter().map(|x| x.cells.clone()).collect::<Vec<_>>())) } };
    let mut ctx_sections = String::new();
    let mut ctx_toc = String::new();
    let mut i = 0usize;
    for ctx in &cx.order {
        let inner = format!("{}{}{}{}{}{}{}{}{}{}{}{}",
            doc_sub("🧰", "API operations", &api_docs, ctx),
            doc_sub(d_emo("type"), "Output types", &type_docs, ctx),
            doc_sub(d_emo("actor"), "Actors", &actor_docs, ctx),
            doc_sub(d_emo("view"), "Views", &view_docs, ctx),
            doc_sub(d_emo("command"), "Commands", &command_docs, ctx),
            doc_sub(d_emo("event"), "Events", &event_docs, ctx),
            doc_sub(d_emo("entity"), "Entities", &entity_docs, ctx),
            table_sub(d_emo("scalar"), "Scalars", &["Scalar", "Type", "Description"], &scalar_rows, ctx),
            table_sub(d_emo("error"), "Errors", &["Error", "Description", "Message (en)", "Message (fr)", "Thrown by"], &error_rows, ctx),
            doc_sub(d_emo("rule"), "Business rules", &rule_docs, ctx),
            doc_sub(d_emo("test"), "Tests", &test_docs, ctx),
            doc_sub(d_emo("obs"), "Observability", &obs_docs, ctx));
        if inner.is_empty() { continue; }
        i += 1;
        ctx_sections.push_str(&h_sec(&format!("ctx-{}", dslug(ctx)), d_emo("context"), &format!("{}. {}", i, ctx), &format!("<div class=\"desc\">{}</div>{}", h_esc(&cx.describe(ctx)), inner)));
        ctx_toc.push_str(&format!("<a href=\"#sec-ctx-{}\">{} {}</a>", dslug(ctx), d_emo("context"), h_esc(ctx)));
    }
    let toc = format!("<a href=\"#sec-stories\">🎬 Stories</a>{}<a href=\"#sec-screens\">📱 Screens</a><a href=\"#sec-translations\">🌐 Translations</a><a href=\"#sec-architecture\">🏛️ Architecture</a><a href=\"#sec-map\">🗺️ Map</a>", ctx_toc);
    let roles_line = "🌐 PUBLIC · 🙋 CUSTOMER · 🏪 RESTAURANT_ACCOUNT · 🍽️ RESTAURANT · 🛵 RIDER · 🛠️ ADMIN · 🔌 EXTERNAL";

    let mut out = String::new();
    out.push_str(THEME);
    out.push_str("\n<div class=\"doc\"><div class=\"wrap\">\n  <div id=\"cf-crumb\" class=\"crumb\"></div>\n  <h1>📖 Captain.Food — Product Documentation</h1>\n  <p class=\"muted\">Generated from the specs, organized <strong>top-level by bounded context</strong> (🔲). The bar above shows where you are (context › section › item — click to jump); hover any link for its description. Every item is anchored — click 🔗 to copy a deep link. Sections are collapsible.</p>\n  <p><strong>Kinds:</strong> ");
    out.push_str(&legend);
    out.push_str("</p>\n  <p><strong>Roles:</strong> ");
    out.push_str(roles_line);
    out.push_str("</p>\n  <div class=\"toolbar\"><button onclick=\"setAll(true)\">⊞ Expand all</button> <button onclick=\"setAll(false)\">⊟ Collapse all</button> &nbsp; <span class=\"toc\">");
    out.push_str(&toc);
    out.push_str("</span></div>\n  ");
    out.push_str(&h_sec("stories", "🎬", "Stories", &stories_html));
    out.push_str("\n  ");
    out.push_str(&ctx_sections);
    out.push_str("\n  ");
    out.push_str(&h_sec("screens", "📱", "Customer screens (SDUI)", &(String::from("<p class=\"muted\">Server-Driven UI screens (customer_screens.yaml, ADR-0033). Per screen, the reads (resolvers→queries) and writes (actions→mutations) are $ref-bound to the GraphQL API and validated — the mockups are the <strong>proof the API answers the UI</strong>. ⚠️ marks gaps the API does not serve yet; 🚫 screens are intentionally not SDUI-rendered.</p>") + &screens_html)));
    out.push_str("\n  ");
    out.push_str(&h_sec("translations", "🌐", "Translations", &(String::from("<p class=\"muted\">The i18n catalog (translations.yaml) — every screen string, referenced by $ref, generated to one translations.generated.json. {param} tokens are validated against declared params.</p>") + &translations_html)));
    out.push_str("\n  ");
    out.push_str(&h_sec("architecture", "🏛️", "Architecture (C4)", &c4_html));
    out.push_str("\n  ");
    out.push_str(&h_sec("map", "🗺️", "System map (interactive)", &(String::from("<p class=\"muted\">Drill in: <strong>System → container → bounded context → aggregate flow</strong>. Boxes are colored by kind (containers/aggregates teal, externals orange, contexts gold, commands yellow, events purple, views blue). Click to go deeper; leaf boxes jump to their section; use ◀ back to climb out.</p>") + &map_html)));
    out.push_str("\n</div></div>\n<div id=\"cf-tip\" class=\"cf-tip\"></div>\n");
    out.push_str(&desc_script);
    out.push('\n');
    out.push_str(NAV_JS);
    out.push('\n');
    out.push_str(MERMAID_JS);
    out
}

// ─── Bounded-context resolution (port of emit/contexts.ts) ──────────────────────────────────────

const CROSS: &str = "cross-cutting";

fn single(s: &HashSet<String>) -> String {
    if s.len() == 1 {
        s.iter().next().unwrap().clone()
    } else {
        CROSS.to_string()
    }
}

struct Cx {
    order: Vec<String>,
    descriptions: HashMap<String, String>,
    actor_ctx: HashMap<String, String>,
    role_ctx: HashMap<String, String>,
    cmd_actor: HashMap<String, String>,
    evt_emitter: HashMap<String, String>,
    evt_consumer: HashMap<String, String>,
    err_cmds: HashMap<String, HashSet<String>>,
    entity_ctx: HashMap<String, String>,
    scalar_ctx: HashMap<String, String>,
    view_agg: HashMap<String, (bool, String)>, // view name -> (is_reference, aggregate)
    type_reads: HashMap<String, Vec<String>>,
}

impl Cx {
    fn of_actor(&self, n: &str) -> String {
        self.actor_ctx.get(n).cloned().unwrap_or_else(|| CROSS.to_string())
    }
    fn of_view(&self, n: &str) -> String {
        match self.view_agg.get(n) {
            Some((false, agg)) => self.of_actor(agg),
            _ => CROSS.to_string(),
        }
    }
    fn of_reads(&self, reads: &[String]) -> String {
        reads.first().map(|r| self.of_view(r)).unwrap_or_else(|| CROSS.to_string())
    }
    fn of_command(&self, n: &str) -> String {
        match self.cmd_actor.get(n) {
            Some(a) => self.of_actor(a),
            None => CROSS.to_string(),
        }
    }
    fn of_event(&self, n: &str) -> String {
        match self.evt_emitter.get(n).or_else(|| self.evt_consumer.get(n)) {
            Some(a) => self.of_actor(a),
            None => CROSS.to_string(),
        }
    }
    fn of_type(&self, n: &str) -> String {
        match self.type_reads.get(n) {
            Some(r) => self.of_reads(r),
            None => CROSS.to_string(),
        }
    }
    fn of_error(&self, n: &str) -> String {
        match self.err_cmds.get(n) {
            None => CROSS.to_string(),
            Some(cmds) => single(&cmds.iter().map(|c| self.of_command(c)).filter(|c| c != CROSS).collect()),
        }
    }
    fn of_entity(&self, n: &str) -> String {
        self.entity_ctx.get(n).cloned().unwrap_or_else(|| CROSS.to_string())
    }
    fn of_scalar(&self, n: &str) -> String {
        self.scalar_ctx.get(n).cloned().unwrap_or_else(|| CROSS.to_string())
    }
    fn describe(&self, ctx: &str) -> String {
        self.descriptions.get(ctx).cloned().unwrap_or_default()
    }
    fn of_operation(&self, roles: &[String], fallback: &str) -> String {
        let performer: HashSet<String> = roles.iter().filter_map(|r| self.role_ctx.get(r).cloned()).collect();
        if performer.len() == 1 {
            performer.into_iter().next().unwrap()
        } else {
            fallback.to_string()
        }
    }
}

fn vote(m: &mut HashMap<String, HashSet<String>>, name: &str, ctx: &str) {
    if name.is_empty() || ctx == CROSS {
        return;
    }
    m.entry(name.to_string()).or_default().insert(ctx.to_string());
}

fn build_context_map(model: &Model, api: &Api, actors: &[Actor], views: &[SqlView]) -> Cx {
    let l2 = model.defs.get("architecture/c4-l2.yaml");
    let l2bc = l2.and_then(|v| v.get("boundedContexts")).and_then(|v| v.as_mapping());
    let mut order = Vec::new();
    let mut descriptions = HashMap::new();
    let mut actor_ctx = HashMap::new();
    let mut role_ctx = HashMap::new();
    if let Some(bcs) = l2bc {
        for (k, bc) in bcs {
            let cid = match k.as_str() {
                Some(s) => s.to_string(),
                None => continue,
            };
            order.push(cid.clone());
            descriptions.insert(cid.clone(), bc.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string());
            for key in ["aggregates", "processManagers"] {
                for n in ref_names(bc.get(key)) {
                    actor_ctx.insert(n, cid.clone());
                }
            }
            for role in bc.get("roles").and_then(|x| x.as_sequence()).map(|s| s.to_vec()).unwrap_or_default() {
                if let Some(r) = role.as_str() {
                    role_ctx.insert(r.to_string(), cid.clone());
                }
            }
        }
    }
    order.push(CROSS.to_string());
    descriptions.insert(CROSS.to_string(), "Shared vocabulary and operations that span several bounded contexts (or belong to none).".to_string());

    let mut cmd_actor = HashMap::new();
    let mut evt_emitter = HashMap::new();
    let mut evt_consumer = HashMap::new();
    let mut err_cmds: HashMap<String, HashSet<String>> = HashMap::new();
    for a in actors {
        for e in &a.receives {
            let msg = ref_name(&e.message_ref);
            if e.message_ref.starts_with("commands.yaml#/") {
                if let Some(m) = &msg {
                    cmd_actor.insert(m.clone(), a.name.clone());
                    for t in &e.throws {
                        if let Some(er) = ref_name(t) {
                            err_cmds.entry(er).or_default().insert(m.clone());
                        }
                    }
                }
            } else if e.message_ref.starts_with("events.yaml#/") {
                if let Some(m) = &msg {
                    evt_consumer.entry(m.clone()).or_insert_with(|| a.name.clone());
                }
            }
            for em in &e.emits {
                if let Some(ev) = ref_name(em) {
                    evt_emitter.entry(ev).or_insert_with(|| a.name.clone());
                }
            }
        }
    }

    let view_agg: HashMap<String, (bool, String)> =
        views.iter().map(|v| (v.name.clone(), (v.reference, v.aggregate.clone()))).collect();
    let type_reads: HashMap<String, Vec<String>> =
        api.types.iter().map(|t| (t.name.clone(), t.reads.clone())).collect();

    let mut cx = Cx {
        order,
        descriptions,
        actor_ctx,
        role_ctx,
        cmd_actor,
        evt_emitter,
        evt_consumer,
        err_cmds,
        entity_ctx: HashMap::new(),
        scalar_ctx: HashMap::new(),
        view_agg,
        type_reads,
    };

    // entities & scalars: attribute by usage across the strongly-anchored artifacts (voting).
    let scalar_names = scalar_names(model);
    let entity_names: Vec<String> = model
        .defs
        .get("entities.yaml")
        .and_then(|v| v.as_mapping())
        .map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let mut entity_votes: HashMap<String, HashSet<String>> = HashMap::new();
    let mut scalar_votes: HashMap<String, HashSet<String>> = HashMap::new();
    let vote_refs = |def: &Value, ctx: &str, sv: &mut HashMap<String, HashSet<String>>, ev: &mut HashMap<String, HashSet<String>>| {
        if ctx == CROSS {
            return;
        }
        let mut refs = Vec::new();
        collect_refs(def, "", &mut refs);
        for (_loc, r) in refs {
            if let Some(p) = parse_ref(&r) {
                if let Some(name) = p.path.first() {
                    if p.file == "scalars.yaml" {
                        vote(sv, name, ctx);
                    } else if p.file == "entities.yaml" || p.file.is_empty() {
                        vote(ev, name, ctx);
                    }
                }
            }
        }
    };

    let cmd_defs = model.defs.get("commands.yaml").and_then(|v| v.as_mapping());
    if let Some(m) = cmd_defs {
        for (k, def) in m {
            if let Some(c) = k.as_str() {
                vote_refs(def, &cx.of_command(c), &mut scalar_votes, &mut entity_votes);
            }
        }
    }
    if let Some(m) = model.defs.get("events.yaml").and_then(|v| v.as_mapping()) {
        for (k, def) in m {
            if let Some(ev) = k.as_str() {
                vote_refs(def, &cx.of_event(ev), &mut scalar_votes, &mut entity_votes);
            }
        }
    }
    if let Some(m) = model.defs.get("errors.yaml").and_then(|v| v.as_mapping()) {
        for (k, def) in m {
            if let Some(er) = k.as_str() {
                vote_refs(def, &cx.of_error(er), &mut scalar_votes, &mut entity_votes);
            }
        }
    }
    for t in &api.types {
        let ctx = cx.of_type(&t.name);
        for f in &t.properties {
            if f.is_ref {
                vote(if scalar_names.contains(&f.ty) { &mut scalar_votes } else { &mut entity_votes }, &f.ty, &ctx);
            }
        }
    }
    for q in api.queries.iter().chain(api.subscriptions.iter()) {
        let ctx = if !q.reads.is_empty() { cx.of_reads(&q.reads) } else { cx.of_type(&q.returns_type) };
        for a in &q.args {
            if a.is_ref {
                vote(if scalar_names.contains(&a.ty) { &mut scalar_votes } else { &mut entity_votes }, &a.ty, &ctx);
            }
        }
    }
    for m in &api.mutations {
        let ctx = cx.of_command(&m.command);
        for f in &m.payload {
            if f.is_ref {
                vote(if scalar_names.contains(&f.ty) { &mut scalar_votes } else { &mut entity_votes }, &f.ty, &ctx);
            }
        }
    }
    for v in views {
        let ctx = cx.of_view(&v.name);
        for col in &v.columns {
            if scalar_names.contains(&col.ty) {
                vote(&mut scalar_votes, &col.ty, &ctx);
            }
        }
    }

    // resolve entity context: aggregate-name match wins, else a single usage context
    let ent_defs = model.defs.get("entities.yaml").and_then(|v| v.as_mapping());
    let mut entity_ctx: HashMap<String, String> = HashMap::new();
    for e in &entity_names {
        let c = if cx.actor_ctx.contains_key(e) {
            cx.actor_ctx.get(e).unwrap().clone()
        } else {
            single(entity_votes.get(e).unwrap_or(&HashSet::new()))
        };
        entity_ctx.insert(e.clone(), c);
    }
    // anchored entities propagate their context to the entities & scalars they reference (one pass)
    for e in &entity_names {
        let ctx = entity_ctx.get(e).cloned().unwrap_or_else(|| CROSS.to_string());
        if ctx != CROSS {
            if let Some(def) = ent_defs.and_then(|m| m.get(e.as_str())) {
                vote_refs(def, &ctx, &mut scalar_votes, &mut entity_votes);
            }
        }
    }
    for e in &entity_names {
        if entity_ctx.get(e).map(|c| c == CROSS).unwrap_or(true) {
            entity_ctx.insert(e.clone(), single(entity_votes.get(e).unwrap_or(&HashSet::new())));
        }
    }
    let mut scalar_ctx: HashMap<String, String> = HashMap::new();
    for s in &scalar_names {
        scalar_ctx.insert(s.clone(), single(scalar_votes.get(s).unwrap_or(&HashSet::new())));
    }
    cx.entity_ctx = entity_ctx;
    cx.scalar_ctx = scalar_ctx;
    cx
}

// ─── stories (personas) — port of load.ts parseStories ──────────────────────────────────────────
struct StoryStep {
    name: String,
    op_kind: Option<String>,
    op: Option<String>,
    note: Option<String>,
}
struct StoryActivity {
    name: String,
    description: Option<String>,
    steps: Vec<StoryStep>,
}
struct Persona {
    name: String,
    description: Option<String>,
    role: String,
    locale: Option<String>,
    activities: Vec<StoryActivity>,
}

fn parse_stories(model: &Model) -> Vec<Persona> {
    let mut out = Vec::new();
    if let Some(m) = model.defs.get("stories.yaml").and_then(|v| v.as_mapping()) {
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            let has_role = node.get("personaRole").and_then(|x| x.as_str()).is_some();
            let has_acts = node.get("activities").map(|x| !x.is_null()).unwrap_or(false);
            if !has_role && !has_acts {
                continue;
            }
            let mut activities = Vec::new();
            if let Some(am) = node.get("activities").and_then(|x| x.as_mapping()) {
                for (ak, a) in am {
                    let aname = match ak.as_str() {
                        Some(s) => s,
                        None => continue,
                    };
                    let mut steps = Vec::new();
                    if let Some(sm) = a.get("steps").and_then(|x| x.as_mapping()) {
                        for (sk, s) in sm {
                            let sname = match sk.as_str() {
                                Some(x) => x.to_string(),
                                None => continue,
                            };
                            if let Some(rf) = s.get("$ref").and_then(|x| x.as_str()) {
                                let ptr = rf.splitn(2, "#/").nth(1).unwrap_or("");
                                let mut segs = ptr.split('/');
                                let seg0 = segs.next().unwrap_or("");
                                let op = segs.next().map(|s| s.to_string());
                                let op_kind = match seg0 {
                                    "queries" => Some("query".to_string()),
                                    "mutations" => Some("mutation".to_string()),
                                    _ => None,
                                };
                                steps.push(StoryStep { name: sname, op_kind, op, note: None });
                            } else {
                                steps.push(StoryStep {
                                    name: sname,
                                    op_kind: None,
                                    op: None,
                                    note: s.get("note").and_then(|x| x.as_str()).map(|x| x.to_string()),
                                });
                            }
                        }
                    }
                    activities.push(StoryActivity {
                        name: aname.to_string(),
                        description: a.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()),
                        steps,
                    });
                }
            }
            out.push(Persona {
                name: name.to_string(),
                description: node.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()),
                role: node.get("personaRole").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                locale: node.get("locale").and_then(|x| x.as_str()).map(|s| s.to_string()),
                activities,
            });
        }
    }
    out
}

// ─── crates/domain/src/generated/scalars.rs (ADR-0034 #3 — Rust domain types from scalars.yaml) ──

/// A scalar's `description` as `///` doc lines (one per non-empty source line, trimmed).
fn scalar_doc(node: &Value) -> String {
    let mut out = String::new();
    if let Some(d) = node.get("description").and_then(|d| d.as_str()) {
        for line in d.trim().lines() {
            let line = line.trim();
            if !line.is_empty() {
                out.push_str("/// ");
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// Emit `crates/domain/src/generated/scalars.rs` from scalars.yaml: enums (VERBATIM SCREAMING_SNAKE
/// variants — no serde rename, so spec == Rust == wire 1:1) and newtypes over `uuid::Uuid` / `i64` /
/// `f64` / `String`, in file order.
fn emit_domain_scalars(model: &Model) -> String {
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/scalars.yaml — do not edit by hand.\n\nuse serde::{Deserialize, Serialize};\n",
    );
    if let Some(Value::Mapping(m)) = model.defs.get("scalars.yaml") {
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            out.push('\n');
            out.push_str(&scalar_doc(node));
            if let Some(vals) = node.get("enum").and_then(|e| e.as_sequence()) {
                out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]\n");
                out.push_str("#[allow(non_camel_case_types)]\n");
                out.push_str(&format!("pub enum {} {{\n", name));
                for v in vals {
                    if let Some(vs) = v.as_str() {
                        // Verbatim: the Rust variant IS the spec value — 1:1 spec↔code↔wire, no serde
                        // transform. The value must be a valid Rust identifier; a spec smell (hyphen,
                        // space, leading digit) fails here so it is fixed at the root, not masked.
                        assert!(
                            vs.chars().next().map_or(false, |c| c.is_ascii_alphabetic() || c == '_')
                                && vs.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                            "scalars.yaml#/{}: enum value '{}' is not a valid Rust identifier — rename it in the spec",
                            name,
                            vs
                        );
                        out.push_str(&format!("    {},\n", vs));
                    }
                }
                out.push_str("}\n");
                continue;
            }
            let ty = node.get("type").and_then(|t| t.as_str()).unwrap_or("string");
            let is_uuid = node.get("format").and_then(|f| f.as_str()) == Some("uuid");
            let (derives, inner) = if is_uuid {
                ("Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize", "uuid::Uuid")
            } else if ty == "integer" {
                ("Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize", "i64")
            } else if ty == "number" {
                ("Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize", "f64")
            } else {
                ("Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize", "String")
            };
            out.push_str(&format!("#[derive({})]\n", derives));
            out.push_str(&format!("pub struct {}(pub {});\n", name, inner));
        }
    }
    out
}

// ─── crates/domain/src/generated/entities.rs (ADR-0034 #3 — Rust entity structs from entities.yaml) ──

/// snake_case of a camelCase property name (e.g. `amountCents` → `amount_cents`).
fn snake_field(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// What serde's `rename_all = "camelCase"` produces for a snake_case field (uppercase the char after
/// each `_`, dropping the `_`). Used to PROVE at generation time that every struct field round-trips
/// back to its exact `entities.yaml` property name on the wire.
fn serde_camel(field: &str) -> String {
    let mut out = String::new();
    let mut upper_next = false;
    for c in field.chars() {
        if c == '_' {
            upper_next = true;
        } else if upper_next {
            out.push(c.to_ascii_uppercase());
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Rust keywords that may appear as entity property names (`ref`, `default` do) — emitted as raw
/// identifiers (`r#ref`). serde renames from the identifier WITHOUT the `r#`, so the wire value stays
/// the spec property name.
const RUST_FIELD_KEYWORDS: &[&str] = &[
    "as", "async", "await", "box", "break", "const", "continue", "default", "do", "dyn", "else", "enum",
    "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut",
    "priv", "pub", "ref", "return", "static", "struct", "trait", "true", "try", "type", "union",
    "unsafe", "use", "where", "while", "yield",
];

/// The Rust type of one struct property node, BEFORE optionality wrapping: `$ref` → the referenced
/// type name (scalars/entities are in scope via `use super::…::*`, same-module refs resolve directly),
/// arrays → `Vec<Item>`, inline primitives → `String`/`i64`/`bool`/`f64` (`date-time` stays `String`).
/// `file` is the spec file the owning struct came from (for panic messages only).
fn struct_field_type(file: &str, owner: &str, prop: &str, node: &Value) -> String {
    if let Some(rf) = node.get("$ref").and_then(|x| x.as_str()) {
        return ref_name(rf)
            .unwrap_or_else(|| panic!("{}#/{}/{}: malformed $ref '{}'", file, owner, prop, rf));
    }
    match node.get("type").and_then(|t| t.as_str()) {
        Some("array") => {
            let items = node
                .get("items")
                .unwrap_or_else(|| panic!("{}#/{}/{}: array without items", file, owner, prop));
            format!("Vec<{}>", struct_field_type(file, owner, prop, items))
        }
        Some("string") => "String".to_string(),
        Some("integer") => "i64".to_string(),
        Some("boolean") => "bool".to_string(),
        Some("number") => "f64".to_string(),
        other => panic!("{}#/{}/{}: unsupported inline type {:?}", file, owner, prop, other),
    }
}

/// Emit one serde `camelCase` payload struct for a spec node with `properties`/`required` (shared by the
/// entities, events and commands emitters — same shape). A field is optional (`Option<T>`) when
/// `nullable: true` or absent from `required`; optional ARRAYS stay `Vec<T>` with `#[serde(default)]`
/// (a missing array deserializes to empty, never `Option<Vec>`).
fn push_struct(out: &mut String, file: &str, name: &str, node: &Value) {
    out.push('\n');
    out.push_str(&scalar_doc(node));
    out.push_str("#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]\n");
    out.push_str("#[serde(rename_all = \"camelCase\")]\n");
    out.push_str(&format!("pub struct {} {{\n", name));
    let required: BTreeSet<&str> = node
        .get("required")
        .and_then(|r| r.as_sequence())
        .map(|s| s.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    if let Some(props) = node.get("properties").and_then(|p| p.as_mapping()) {
        for (pk, pnode) in props {
            let prop = match pk.as_str() {
                Some(s) => s,
                None => continue,
            };
            let field = snake_field(prop);
            // PROVE serde's camelCase rename restores the exact spec property name on the wire
            // (raw `r#` prefixes are stripped before renaming, so `ref`/`default` stay as-is);
            // fail loudly at generation rather than corrupt the wire.
            assert_eq!(
                serde_camel(&field),
                prop,
                "{}#/{}/{}: field '{}' does not round-trip through serde rename_all",
                file,
                name,
                prop,
                field
            );
            let ident = if RUST_FIELD_KEYWORDS.contains(&field.as_str()) {
                format!("r#{}", field)
            } else {
                field
            };
            let ty = struct_field_type(file, name, prop, pnode);
            let nullable = pnode.get("nullable").and_then(|x| x.as_bool()) == Some(true);
            let optional = nullable || !required.contains(prop);
            if ty.starts_with("Vec<") {
                if optional {
                    out.push_str("    #[serde(default)]\n");
                }
                out.push_str(&format!("    pub {}: {},\n", ident, ty));
            } else if optional {
                out.push_str(&format!("    pub {}: Option<{}>,\n", ident, ty));
            } else {
                out.push_str(&format!("    pub {}: {},\n", ident, ty));
            }
        }
    }
    out.push_str("}\n");
}

/// True for a spec node that defines a payload struct (an object with `properties`, or `type: object`) —
/// distinguishes real definitions from a file's top-level `version`/`description` meta.
fn is_struct_def(node: &Value) -> bool {
    node.is_mapping()
        && (node.get("properties").is_some() || node.get("type").and_then(|t| t.as_str()) == Some("object"))
}

/// Emit `crates/domain/src/generated/entities.rs` from entities.yaml: one serde `camelCase` struct per
/// top-level entity, in file order. Type names may safely use the prelude `Option` — the
/// `rust-reserved-typename` validator gate forbids a spec type from colliding with it (resolve at root).
fn emit_domain_entities(model: &Model) -> String {
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/entities.yaml — do not edit by hand.\n\nuse serde::{Deserialize, Serialize};\nuse super::scalars::*;\n",
    );
    if let Some(Value::Mapping(m)) = model.defs.get("entities.yaml") {
        for (k, node) in m {
            if let Some(name) = k.as_str() {
                push_struct(&mut out, "entities.yaml", name, node);
            }
        }
    }
    out
}

/// Emit `crates/domain/src/generated/events.rs` from events.yaml — one serde `camelCase` payload struct
/// per business event (ADR-0034 #3). BUSINESS payloads only; the technical envelope (eventId, occurredAt,
/// metadata…) is added by infrastructure, never here (CLAUDE.md). Events reference scalars + entities.
fn emit_domain_events(model: &Model) -> String {
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/events.yaml — do not edit by hand.\n// BUSINESS event payloads only — the technical envelope is added by infrastructure.\n\nuse serde::{Deserialize, Serialize};\nuse super::scalars::*;\nuse super::entities::*;\n",
    );
    let mut names: Vec<String> = Vec::new();
    if let Some(Value::Mapping(m)) = model.defs.get("events.yaml") {
        for (k, node) in m {
            if let Some(name) = k.as_str() {
                if is_struct_def(node) {
                    push_struct(&mut out, "events.yaml", name, node);
                    names.push(name.to_string());
                }
            }
        }
    }
    // The DomainEvent enum — a typed union of every business event, adjacently tagged so it (de)serializes
    // straight from the stored envelope shape `{ eventType, payload }`. Projectors and the event store
    // match on this instead of stringly-typed event_type + a serde_json::Value payload.
    out.push_str(
        "\n/// Every business event as a typed, adjacently-tagged union: `{ \"eventType\": <name>, \"payload\": { … } }`.\n#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]\n#[serde(tag = \"eventType\", content = \"payload\")]\npub enum DomainEvent {\n",
    );
    for n in &names {
        out.push_str(&format!("    {}({}),\n", n, n));
    }
    out.push_str("}\n");
    out
}

/// The Rust type of a materialized read-model column: a SQL primitive maps to its Rust counterpart,
/// otherwise the name is a scalars.yaml newtype (in scope via `use domain::generated::scalars::*`).
/// An undetermined type (a computed column with no derivable lineage) falls back to `serde_json::Value`.
fn projection_rust_type(ty: &str) -> String {
    match ty {
        "" => "serde_json::Value".to_string(),
        "text" => "String".to_string(),
        "jsonb" => "serde_json::Value".to_string(),
        "integer" => "i64".to_string(),
        "bigint" => "i64".to_string(),
        "boolean" => "bool".to_string(),
        "numeric" => "f64".to_string(),
        "timestamptz" => "chrono::DateTime<chrono::Utc>".to_string(),
        scalar => scalar.to_string(),
    }
}

/// Emit `crates/application/src/generated/rows.rs` — one `<Table>Row` struct per materialized read-model
/// table (projection_tables.yaml). These are the rows a projector writes and the query side returns
/// (ADR-0040). Column names are already snake_case, so serde's defaults match; jsonb / entity-shaped
/// columns are `serde_json::Value`, timestamps `chrono::DateTime<Utc>`, domain scalars their newtype.
fn emit_projection_rows(model: &Model) -> String {
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/database/tables/projection_tables.yaml — do not edit by hand.\n// Read-model row types: what a projector writes and the query side returns (ADR-0040).\n\nuse serde::{Deserialize, Serialize};\nuse domain::generated::scalars::*;\n",
    );
    for v in parse_views(model).iter().filter(|v| v.is_table) {
        out.push('\n');
        if let Some(note) = &v.note {
            out.push_str(&format!("/// {}\n", ws1(note)));
        }
        out.push_str("#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]\n");
        out.push_str(&format!("pub struct {}Row {{\n", v.name));
        for c in &v.columns {
            let rt = projection_rust_type(&c.ty);
            let ty = if c.nullable && !c.pk { format!("Option<{}>", rt) } else { rt };
            let ident = if RUST_FIELD_KEYWORDS.contains(&c.name.as_str()) {
                format!("r#{}", c.name)
            } else {
                c.name.clone()
            };
            out.push_str(&format!("    pub {}: {},\n", ident, ty));
        }
        out.push_str("}\n");
    }
    out
}

/// An event property's node in events.yaml (`events.yaml#/<event>/properties/<prop>`), if present.
fn event_prop_node<'a>(model: &'a Model, event: &str, prop: &str) -> Option<&'a Value> {
    model.defs.get("events.yaml")?.get(event)?.get("properties")?.get(prop)
}

/// Whether an event carries a property at all (used as the same-stream test: an event is same-stream for a
/// table iff it carries the table's PK property).
fn event_has_prop(model: &Model, event: &str, prop: &str) -> bool {
    event_prop_node(model, event, prop).is_some()
}

/// Whether an event property is optional on the wire (nullable, or absent from the event's `required`).
fn event_prop_optional(model: &Model, event: &str, prop: &str) -> bool {
    let ev = model.defs.get("events.yaml").and_then(|e| e.get(event));
    let required = ev
        .and_then(|e| e.get("required"))
        .and_then(|r| r.as_sequence())
        .map(|s| s.iter().any(|v| v.as_str() == Some(prop)))
        .unwrap_or(false);
    let nullable = event_prop_node(model, event, prop)
        .and_then(|p| p.get("nullable"))
        .and_then(|b| b.as_bool())
        == Some(true);
    nullable || !required
}

/// Snake_case Rust field ident for a column/property, raw-escaped if it collides with a keyword.
fn rust_ident(name: &str) -> String {
    if RUST_FIELD_KEYWORDS.contains(&name) {
        format!("r#{}", name)
    } else {
        name.to_string()
    }
}

/// How a projection-table column is populated — classified from its lineage (mirrors the fold modes).
enum ColMode {
    /// Implicit technical timestamp — stamped by the dispatch wrapper, not per event.
    Timestamp,
    /// Flat same-stream scalar copy: each carrying event holds `prop` and its type equals the column's.
    ScalarLatest { prop: String, events: Vec<String> },
    /// Status-from-event-type: `derive` map (literal enum value or payload-extracted).
    Derive,
    /// `max(occurred_at)` over whole-event `from` → set to the event time on those events.
    Occurrence { events: Vec<String> },
    /// Computed / cross-stream / accumulate / composite — a hand-written `Compute` hook (typed via the event).
    Complex,
}

/// Classify a projection-table column. `pk_prop` = the creation-anchoring property (same-stream test);
/// `creation` = the creation event. A non-nullable flat column the creation event does NOT carry can't be
/// initialized mechanically → it becomes a `Complex` hook (which supplies the initial value too).
fn classify_column(model: &Model, c: &SqlColumn, pk_prop: &str, creation: &str) -> ColMode {
    if c.name == "created_at" || c.name == "updated_at" {
        return ColMode::Timestamp;
    }
    if !c.derive.is_empty() {
        return ColMode::Derive;
    }
    let carrying: Vec<(String, String)> =
        c.from.iter().filter_map(|r| { let (e, p) = event_and_prop(r); p.map(|p| (e, p)) }).collect();
    let whole: Vec<String> =
        c.from.iter().filter_map(|r| { let (e, p) = event_and_prop(r); if p.is_none() { Some(e) } else { None } }).collect();
    if c.ty == "timestamptz" && carrying.is_empty() && !whole.is_empty() {
        return ColMode::Occurrence { events: whole };
    }
    // Flat scalar-latest iff every carrying event is same-stream (carries the PK prop) AND its property's
    // type equals the column type (a composite like `breakdown`/`totalAmount` fails this → Complex). A
    // timestamptz VALUE column (date-time string in the payload → DateTime) needs parsing → Complex.
    if !carrying.is_empty() && c.occurred_when.is_empty() && c.ty != "timestamptz" {
        let flat = carrying.iter().all(|(ev, prop)| {
            event_has_prop(model, ev, pk_prop)
                && event_prop_node(model, ev, prop).map(schema_node_to_column_type).as_deref() == Some(c.ty.as_str())
        });
        let opt = c.nullable && !c.pk;
        let on_creation = carrying.iter().any(|(e, _)| e == creation);
        if flat && (opt || on_creation) {
            let mut events: Vec<String> = Vec::new();
            for (e, _) in &carrying {
                if !events.contains(e) {
                    events.push(e.clone());
                }
            }
            let prop = carrying[0].1.clone();
            return ColMode::ScalarLatest { prop, events };
        }
    }
    ColMode::Complex
}

/// Emit `crates/application/src/generated/projectors.rs` — the HYBRID projector per read-model table
/// (ADR-0040): the generator maps the MECHANICAL columns inline from the `from` lineage (flat same-stream
/// scalar-latest, `derive` status, occurrence timestamps, the implicit created_at/updated_at), and for each
/// COMPLEX column (computed / cross-stream / accumulate / composite) generates a typed hook on a
/// `<Table>Compute` trait — `fn <col>(&self, prev, env) -> <ColType>` — implemented by hand (business logic
/// stays tested + out of generated code). The dispatch routes each `fedBy` event; a `tombstone` → `None`.
fn emit_projectors(model: &Model) -> String {
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/database/tables/projection_tables.yaml — do not edit by hand.\n// HYBRID projector (ADR-0040): mechanical columns (flat same-stream scalar-latest, derive, occurrence,\n// created_at/updated_at) are mapped inline from the `from` lineage; COMPLEX columns call a typed hook on\n// the per-table `…Compute` trait (implemented by hand). The dispatch routes each fedBy event.\n#![allow(unused_variables)]\n\nuse super::rows::*;\nuse crate::projections::Envelope;\nuse domain::generated::events::*;\nuse domain::generated::scalars::*;\n",
    );
    // Rust type of a column (Option<…> when nullable & not pk) — mirrors emit_projection_rows.
    let col_ty = |c: &SqlColumn| -> String {
        let rt = projection_rust_type(&c.ty);
        if c.nullable && !c.pk { format!("Option<{}>", rt) } else { rt }
    };
    for v in parse_views(model).iter().filter(|v| v.is_table) {
        let row = format!("{}Row", v.name);
        let fnname = snake_type(&v.name);
        let pk = match v.columns.iter().find(|c| c.pk) {
            Some(p) => p,
            None => continue,
        };
        let (creation, pk_prop) = match pk.from.iter().filter_map(|r| { let (e, p) = event_and_prop(r); p.map(|p| (e, p)) }).next() {
            Some(x) => x,
            None => continue,
        };
        let modes: Vec<(&SqlColumn, ColMode)> =
            v.columns.iter().map(|c| (c, classify_column(model, c, &pk_prop, &creation))).collect();
        let complex: Vec<&SqlColumn> =
            modes.iter().filter(|(_, m)| matches!(m, ColMode::Complex)).map(|(c, _)| *c).collect();

        // Compute trait — a typed hook per complex column (business logic, hand-written).
        out.push_str(&format!(
            "\n/// Hand-written business logic for `{}`'s computed / cross-stream / accumulate columns\n/// (`env.event` is the typed, declared event). Mechanical columns are mapped by the generator.\npub trait {}Compute {{\n",
            v.name, v.name
        ));
        for c in &complex {
            out.push_str(&format!(
                "    fn {}(&self, prev: Option<&{}>, env: &Envelope) -> {};\n",
                rust_ident(&c.name), row, col_ty(c)
            ));
        }
        out.push_str("}\n");

        // The value expression for a column on the CREATION event (building a fresh row).
        let creation_value = |c: &SqlColumn, m: &ColMode| -> String {
            let opt = c.nullable && !c.pk;
            match m {
                ColMode::Timestamp => "env.occurred_at".to_string(),
                ColMode::Complex => format!("c.{}(None, env)", rust_ident(&c.name)),
                ColMode::Occurrence { events } => {
                    if events.iter().any(|e| e == &creation) || !opt {
                        if opt { "Some(env.occurred_at)".to_string() } else { "env.occurred_at".to_string() }
                    } else {
                        "None".to_string()
                    }
                }
                ColMode::Derive => {
                    let arm = c.derive.iter().find(|(e, _)| e == &creation);
                    match arm {
                        Some((_, DeriveVal::Lit(s))) => {
                            let inner = format!("{}::{}", c.ty, s);
                            if opt { format!("Some({})", inner) } else { inner }
                        }
                        Some((_, DeriveVal::Payload(p))) => {
                            let inner = format!("e.{}.clone()", rust_ident(&snake_field(p)));
                            if opt { format!("Some({})", inner) } else { inner }
                        }
                        None => {
                            if opt { "None".to_string() }
                            else { panic!("projection {}: non-nullable derive column '{}' but creation event '{}' is not in its derive map", v.name, c.name, creation) }
                        }
                    }
                }
                ColMode::ScalarLatest { prop, events } => {
                    if events.iter().any(|e| e == &creation) {
                        let field = rust_ident(&snake_field(prop));
                        if c.ty == "jsonb" {
                            // typed event field → jsonb column: serialize (works for structs/arrays/optionals).
                            let base = format!("serde_json::to_value(&e.{}).unwrap_or(serde_json::Value::Null)", field);
                            if opt { format!("Some({})", base) } else { base }
                        } else {
                            let ev_opt = event_prop_optional(model, &creation, prop);
                            match (ev_opt, opt) {
                                (true, true) => format!("e.{}.clone()", field),
                                (true, false) => format!("e.{}.clone().expect(\"{} required on {}\")", field, c.name, creation),
                                (false, true) => format!("Some(e.{}.clone())", field),
                                (false, false) => format!("e.{}.clone()", field),
                            }
                        }
                    } else {
                        "None".to_string() // classify downgraded non-nullable-uncarried to Complex
                    }
                }
            }
        };

        // Dispatch — build on creation, mutate on updates, delete on tombstone; timestamps stamped after.
        out.push_str(&format!(
            "\npub fn project_{}<C: {}Compute>(c: &C, state: Option<{}>, env: &Envelope) -> Option<{}> {{\n",
            fnname, v.name, row, row
        ));
        if complex.is_empty() {
            out.push_str("    let _ = c;\n");
        }
        out.push_str("    let created = state.as_ref().map(|r| r.created_at);\n    let next = match &env.event {\n");
        for ev in &v.fedby {
            if v.tombstone.as_deref() == Some(ev.as_str()) {
                out.push_str(&format!("        DomainEvent::{}(_) => None,\n", ev));
                continue;
            }
            if ev == &creation {
                out.push_str(&format!("        DomainEvent::{}(e) => Some({} {{\n", ev, row));
                for (c, m) in &modes {
                    out.push_str(&format!("            {}: {},\n", rust_ident(&c.name), creation_value(c, m)));
                }
                out.push_str("        }),\n");
                continue;
            }
            // Update arm: set the columns this event feeds. Mechanical first, then complex (reads the row).
            let mut mech: Vec<String> = Vec::new();
            let mut cplx: Vec<String> = Vec::new();
            let mut uses_e = false;
            for (c, m) in &modes {
                let opt = c.nullable && !c.pk;
                let cid = rust_ident(&c.name);
                match m {
                    ColMode::ScalarLatest { prop, events } if events.iter().any(|e| e == ev) => {
                        uses_e = true;
                        let field = rust_ident(&snake_field(prop));
                        if c.ty == "jsonb" {
                            let base = format!("serde_json::to_value(&e.{}).unwrap_or(serde_json::Value::Null)", field);
                            mech.push(if opt {
                                format!("row.{} = Some({});", cid, base)
                            } else {
                                format!("row.{} = {};", cid, base)
                            });
                        } else {
                            let ev_opt = event_prop_optional(model, ev, prop);
                            mech.push(match (ev_opt, opt) {
                                (true, true) => format!("row.{} = e.{}.clone();", cid, field),
                                (true, false) => format!("if let Some(v) = &e.{} {{ row.{} = v.clone(); }}", field, cid),
                                (false, true) => format!("row.{} = Some(e.{}.clone());", cid, field),
                                (false, false) => format!("row.{} = e.{}.clone();", cid, field),
                            });
                        }
                    }
                    ColMode::Derive => {
                        if let Some((_, dv)) = c.derive.iter().find(|(e, _)| e == ev) {
                            let inner = match dv {
                                DeriveVal::Lit(s) => format!("{}::{}", c.ty, s),
                                DeriveVal::Payload(p) => { uses_e = true; format!("e.{}.clone()", rust_ident(&snake_field(p))) }
                            };
                            let val = if opt { format!("Some({})", inner) } else { inner };
                            mech.push(format!("row.{} = {};", cid, val));
                        }
                    }
                    ColMode::Occurrence { events } if events.iter().any(|e| e == ev) => {
                        let val = if opt { "Some(env.occurred_at)".to_string() } else { "env.occurred_at".to_string() };
                        mech.push(format!("row.{} = {};", cid, val));
                    }
                    ColMode::Complex if c.from.iter().any(|r| &event_and_prop(r).0 == ev) => {
                        cplx.push(format!("let v = c.{}(Some(&row), env); row.{} = v;", cid, cid));
                    }
                    _ => {}
                }
            }
            let mut stmts = mech;
            stmts.extend(cplx);
            let bind = if uses_e { "e" } else { "_" };
            if stmts.is_empty() {
                out.push_str(&format!("        DomainEvent::{}(_) => state,\n", ev));
            } else {
                out.push_str(&format!(
                    "        DomainEvent::{}({}) => {{ let mut row = state?; {} Some(row) }},\n",
                    ev, bind, stmts.join(" ")
                ));
            }
        }
        out.push_str("        _ => return state,\n    };\n");
        out.push_str("    next.map(|mut row| {\n        row.created_at = created.unwrap_or(env.occurred_at);\n        row.updated_at = env.occurred_at;\n        row\n    })\n");
        out.push_str("}\n");
    }
    out
}

/// Emit `crates/domain/src/generated/commands.rs` from commands.yaml — one serde `camelCase` payload
/// struct per command (and command value object), the CQRS write-side input types (ADR-0034 #3). A
/// command is a request the system may reject; its payload references scalars + entities.
fn emit_domain_commands(model: &Model) -> String {
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/commands.yaml — do not edit by hand.\n// CQRS command (write-side) input payloads.\n\nuse serde::{Deserialize, Serialize};\nuse super::scalars::*;\nuse super::entities::*;\n",
    );
    if let Some(Value::Mapping(m)) = model.defs.get("commands.yaml") {
        for (k, node) in m {
            if let Some(name) = k.as_str() {
                if is_struct_def(node) {
                    push_struct(&mut out, "commands.yaml", name, node);
                }
            }
        }
    }
    out
}

// ─── crates/domain/src/generated/errors.rs (anticipated-error catalog from errors.yaml) ───

/// Escape a spec string for embedding in a Rust string literal.
fn rust_string_lit(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

/// SCREAMING_SNAKE const name for a PascalCase errors.yaml key (`SlugAlreadyTaken` →
/// `SLUG_ALREADY_TAKEN`).
fn screaming_snake(name: &str) -> String {
    snake_field(name).trim_start_matches('_').to_ascii_uppercase()
}

/// Emit `crates/domain/src/generated/errors.rs` from errors.yaml — the anticipated-error catalog
/// behind the structured `DomainError::Rejected { code, context }` rejections (ADR-0046 follow-up):
/// per error one `ErrorDef` const carrying the stable PascalCase wire CODE (= the errors.yaml key =
/// the GraphQL `extensions.code`, error contract P-10) plus its `en`/`fr` message templates, and the
/// `{placeholder}` interpolation over the rejection's JSON context (field names = the errors.yaml
/// `context` keys). This module is the single source for wire code + localized message.
fn emit_domain_errors(model: &Model) -> String {
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/errors.yaml — do not edit by hand.\n// The anticipated-error catalog: one entry per errors.yaml error — the stable PascalCase wire CODE\n// (= the GraphQL `extensions.code`, error contract P-10) plus its localized `{placeholder}` message\n// templates, interpolated from the rejection's JSON context (`DomainError::Rejected`).\n\n/// One anticipated error (errors.yaml): the stable wire code + its localized message templates.\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub struct ErrorDef {\n    /// The stable PascalCase error code (= the errors.yaml key = the wire `extensions.code`).\n    pub code: &'static str,\n    /// English message template; `{placeholder}` tokens name fields of the error's typed context.\n    pub message_en: &'static str,\n    /// French message template (same tokens).\n    pub message_fr: &'static str,\n}\n",
    );
    let mut names: Vec<String> = Vec::new();
    if let Some(Value::Mapping(m)) = model.defs.get("errors.yaml") {
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            let konst = screaming_snake(name);
            let en = node
                .get("messages")
                .and_then(|ms| ms.get("en"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let fr = node
                .get("messages")
                .and_then(|ms| ms.get("fr"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            out.push('\n');
            push_doc(&mut out, "", node.get("description").and_then(|x| x.as_str()));
            // Document the typed context fields (camelCase keys of the rejection's JSON context).
            if let Some(ctx) = node.get("context").and_then(|c| c.as_mapping()) {
                if !ctx.is_empty() {
                    let fields: Vec<&str> = ctx.iter().filter_map(|(ck, _)| ck.as_str()).collect();
                    out.push_str(&format!("/// Context: `{}`.\n", fields.join("`, `")));
                }
            }
            out.push_str(&format!(
                "pub const {}: ErrorDef = ErrorDef {{\n    code: \"{}\",\n    message_en: \"{}\",\n    message_fr: \"{}\",\n}};\n",
                konst,
                rust_string_lit(name),
                rust_string_lit(en),
                rust_string_lit(fr)
            ));
            names.push(konst);
        }
    }
    out.push_str("\n/// Every anticipated error, in errors.yaml order.\npub const ERRORS: &[ErrorDef] = &[\n");
    for n in &names {
        out.push_str(&format!("    {},\n", n));
    }
    out.push_str("];\n");
    out.push_str(
        "\n/// Look up an anticipated error by its stable PascalCase code.\npub fn find(code: &str) -> Option<&'static ErrorDef> {\n    ERRORS.iter().find(|e| e.code == code)\n}\n\n/// Interpolate the `{placeholder}` tokens of `template` from the rejection's JSON `context` object:\n/// a token naming a context field renders its value (strings verbatim, other JSON values via their\n/// canonical JSON form); a token with no matching field is left as-is (a visible spec/context gap,\n/// never a panic).\npub fn interpolate(template: &str, context: &serde_json::Value) -> String {\n    let mut out = String::with_capacity(template.len());\n    let mut rest = template;\n    while let Some(start) = rest.find('{') {\n        out.push_str(&rest[..start]);\n        let after = &rest[start + 1..];\n        match after.find('}') {\n            Some(end) => {\n                let key = &after[..end];\n                match context.get(key) {\n                    Some(serde_json::Value::String(s)) => out.push_str(s),\n                    Some(v) => out.push_str(&v.to_string()),\n                    None => {\n                        out.push('{');\n                        out.push_str(key);\n                        out.push('}');\n                    }\n                }\n                rest = &after[end + 1..];\n            }\n            None => {\n                out.push('{');\n                rest = after;\n            }\n        }\n    }\n    out.push_str(rest);\n    out\n}\n\n/// The interpolated English message for `code`, if it is a catalogued error.\npub fn message_en(code: &str, context: &serde_json::Value) -> Option<String> {\n    find(code).map(|e| interpolate(e.message_en, context))\n}\n\n/// The interpolated French message for `code`, if it is a catalogued error.\npub fn message_fr(code: &str, context: &serde_json::Value) -> Option<String> {\n    find(code).map(|e| interpolate(e.message_fr, context))\n}\n",
    );
    out
}

// ─── crates/server/src/graphql/generated/ (Stage 1a — async-graphql type layer from api.yaml) ───
//
// The server hosts the GraphQL surface with async-graphql, but `domain` must stay GraphQL-free
// (ADR-0035) and the orphan rule forbids implementing async-graphql's foreign traits on the foreign
// domain newtypes from `server`. So the generator emits a SERVER-SIDE wrapper layer: one wrapper
// newtype (GraphQL scalar) / mirror enum per scalars.yaml type with `From` conversions both ways,
// SimpleObject output types, InputObject inputs, and a QueryRoot exposing every api.yaml query
// (read resolvers stubbed until the read-model repositories land).

/// Rust-safe struct name for a GraphQL type emitted into the server layer: a spec type may collide with
/// a Rust prelude name (the API type `Option` does) — emitted as `<Name>_` plus an explicit
/// `#[graphql(name = "<Name>")]`, so the GraphQL name stays the spec name.
fn gql_rust_name(name: &str) -> String {
    match name {
        "Option" | "Box" | "String" | "Vec" | "Result" => format!("{}_", name),
        _ => name.to_string(),
    }
}

/// Rust type of an inline (non-`$ref`) schema primitive in the GraphQL layer. `integer` → `i64`
/// (async-graphql serializes any Rust integer as the GraphQL `Int`, and the domain uses `i64`);
/// `date-time` strings → `chrono::DateTime<Utc>` (the `DateTime` scalar via async-graphql's `chrono`
/// feature).
fn rust_inline_primitive(t: &str, format: Option<&str>) -> String {
    match t {
        "integer" => "i64".into(),
        "boolean" => "bool".into(),
        "number" => "f64".into(),
        "string" if format == Some("date-time") => "chrono::DateTime<chrono::Utc>".into(),
        _ => "String".into(),
    }
}

/// Rust base type of a spec node in the server GraphQL layer — mirrors `base_type` (the SDL emitter):
/// scalars.yaml refs → the wrapper scalar / mirror enum, other refs → the generated struct
/// (`…Input`-suffixed when `input`), arrays → `Vec<…>`, inline primitives via `rust_inline_primitive`.
fn rust_base_type(node: &Value, ctx: &str, input: bool) -> String {
    if let Some(rf) = node.get("$ref").and_then(|x| x.as_str()) {
        let file = ref_target_file(rf, ctx);
        let name = parse_ref(rf).and_then(|p| p.path.into_iter().next()).unwrap_or_else(|| "String".into());
        if file.as_deref() == Some("scalars.yaml") {
            return gql_rust_name(&name);
        }
        return if input { format!("{}Input", name) } else { gql_rust_name(&name) };
    }
    if node.get("type").and_then(|x| x.as_str()) == Some("array") {
        if let Some(items) = node.get("items") {
            return format!("Vec<{}>", rust_base_type(items, ctx, input));
        }
    }
    rust_inline_primitive(
        node.get("type").and_then(|x| x.as_str()).unwrap_or("string"),
        node.get("format").and_then(|x| x.as_str()),
    )
}

/// Rust base type of an api.yaml field — mirrors `api_field_type` (without the nullability suffix).
fn rust_api_field_base(model: &Model, f: &ApiField, input: bool) -> String {
    let mut base = if f.is_ref {
        if input && !scalar_names(model).contains(&f.ty) {
            format!("{}Input", f.ty)
        } else {
            gql_rust_name(&f.ty)
        }
    } else {
        rust_inline_primitive(&f.ty, f.format.as_deref())
    };
    if f.array {
        base = format!("Vec<{}>", base);
    }
    base
}

/// Spec `description` → Rust `///` doc lines at `indent` (one per non-empty trimmed line). async-graphql
/// turns doc comments into GraphQL descriptions (SimpleObject/InputObject structs + fields, Enum,
/// `#[Object]` resolvers), so the spec documentation reaches introspection/GraphiQL. No description →
/// no lines.
fn push_doc(out: &mut String, indent: &str, desc: Option<&str>) {
    if let Some(d) = desc {
        for line in d.trim().lines() {
            let line = line.trim();
            if !line.is_empty() {
                out.push_str(&format!("{}/// {}\n", indent, line));
            }
        }
    }
}

/// Push one generated GraphQL struct field: the spec description as a `///` doc (→ introspection), an
/// explicit `#[graphql(name = …)]` (the exact SDL name — independent of derive rename rules and raw
/// `r#` idents), `#[serde(default)]` on arrays (lenient jsonb → typed mapping), raw-escaped snake_case
/// ident.
fn push_gql_field(out: &mut String, name: &str, base: &str, non_null: bool, desc: Option<&str>) {
    let ident = rust_ident(&snake_field(name));
    let ty = if non_null { base.to_string() } else { format!("Option<{}>", base) };
    push_doc(out, "    ", desc);
    out.push_str(&format!("    #[graphql(name = \"{}\")]\n", name));
    if ty.starts_with("Vec<") {
        out.push_str("    #[serde(default)]\n");
    }
    out.push_str(&format!("    pub {}: {},\n", ident, ty));
}

/// Open one generated server-side GraphQL struct (`derive` = `SimpleObject` or `InputObject`), with the
/// spec description as a `///` doc (→ the type's introspection description). serde derives use
/// `rename_all = "camelCase"` so the struct (de)serializes to/from the spec wire shape — this is what
/// lets jsonb read-model columns deserialize straight into the typed output structs.
fn push_gql_struct_open(out: &mut String, gql_name: &str, derive: &str, desc: Option<&str>) {
    let rust = gql_rust_name(gql_name);
    out.push('\n');
    push_doc(out, "", desc);
    out.push_str(&format!(
        "#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, async_graphql::{})]\n#[serde(rename_all = \"camelCase\")]\n",
        derive
    ));
    if rust != gql_name {
        out.push_str(&format!("#[graphql(name = \"{}\")]\n", gql_name));
    }
    out.push_str(&format!("pub struct {} {{\n", rust));
}

/// Push the fields of a spec object def (entities.yaml / commands.yaml shape) — mirrors `object_fields`.
fn push_gql_object_fields(out: &mut String, def: &Value, ctx: &str, input: bool) {
    let props = match def.get("properties").and_then(|p| p.as_mapping()) {
        Some(m) => m,
        None => return,
    };
    let required: HashSet<&str> = def
        .get("required")
        .and_then(|r| r.as_sequence())
        .map(|s| s.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    for (k, p) in props {
        let name = match k.as_str() {
            Some(s) => s,
            None => continue,
        };
        if input && p.get("readOnly").and_then(|x| x.as_bool()) == Some(true) {
            continue;
        }
        let base = rust_base_type(p, ctx, input);
        let non_null = if input {
            required.contains(name)
        } else {
            p.get("nullable").and_then(|x| x.as_bool()) != Some(true)
        };
        push_gql_field(out, name, &base, non_null, p.get("description").and_then(|x| x.as_str()));
    }
}

/// Emit `crates/server/src/graphql/generated/scalars.rs` — the async-graphql wrapper layer over the
/// domain scalars (orphan rule): non-enum scalars.yaml types become wrapper newtypes registered via
/// `async_graphql::scalar!`, enums become mirror `async_graphql::Enum`s (verbatim variants), each with
/// `From` conversions both ways to `domain::generated::scalars`.
fn emit_server_scalars(model: &Model) -> String {
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/scalars.yaml — do not edit by hand.\n// Server-side async-graphql scalar layer: `domain` stays GraphQL-free (ADR-0035) and the orphan rule\n// forbids implementing async-graphql traits on domain newtypes here, so each scalars.yaml type gets a\n// wrapper newtype (GraphQL scalar) / mirror enum with `From` conversions both ways.\n#![allow(dead_code)]\n#![allow(non_camel_case_types)]\n\nuse domain::generated::scalars as ds;\n",
    );
    if let Some(Value::Mapping(m)) = model.defs.get("scalars.yaml") {
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            out.push('\n');
            out.push_str(&scalar_doc(node));
            if let Some(vals) = node.get("enum").and_then(|e| e.as_sequence()) {
                let variants: Vec<&str> = vals.iter().filter_map(|v| v.as_str()).collect();
                out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, async_graphql::Enum)]\n");
                out.push_str(&format!("pub enum {} {{\n", name));
                for v in &variants {
                    out.push_str(&format!("    #[graphql(name = \"{}\")]\n    {},\n", v, v));
                }
                out.push_str("}\n");
                out.push_str(&format!("impl From<ds::{}> for {} {{\n    fn from(v: ds::{}) -> Self {{\n        match v {{\n", name, name, name));
                for v in &variants {
                    out.push_str(&format!("            ds::{}::{} => Self::{},\n", name, v, v));
                }
                out.push_str("        }\n    }\n}\n");
                out.push_str(&format!("impl From<{}> for ds::{} {{\n    fn from(v: {}) -> Self {{\n        match v {{\n", name, name, name));
                for v in &variants {
                    out.push_str(&format!("            {}::{} => Self::{},\n", name, v, v));
                }
                out.push_str("        }\n    }\n}\n");
                continue;
            }
            let ty = node.get("type").and_then(|t| t.as_str()).unwrap_or("string");
            let is_uuid = node.get("format").and_then(|f| f.as_str()) == Some("uuid");
            let (derives, inner) = if is_uuid {
                ("Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize", "uuid::Uuid")
            } else if ty == "integer" {
                ("Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize", "i64")
            } else if ty == "number" {
                ("Debug, Clone, Copy, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize", "f64")
            } else {
                ("Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize", "String")
            };
            out.push_str(&format!("#[derive({})]\n", derives));
            out.push_str(&format!("pub struct {}(pub {});\n", name, inner));
            // The scalar! macro takes the introspection description explicitly (doc comments don't
            // reach it) — whitespace-collapsed to one line, escaped as a Rust string literal.
            match node.get("description").and_then(|d| d.as_str()) {
                Some(d) => out.push_str(&format!("async_graphql::scalar!({}, {:?}, {:?});\n", name, name, ws1(d.trim()))),
                None => out.push_str(&format!("async_graphql::scalar!({});\n", name)),
            }
            out.push_str(&format!(
                "impl From<ds::{}> for {} {{\n    fn from(v: ds::{}) -> Self {{\n        Self(v.0)\n    }}\n}}\n",
                name, name, name
            ));
            out.push_str(&format!(
                "impl From<{}> for ds::{} {{\n    fn from(v: {}) -> Self {{\n        Self(v.0)\n    }}\n}}\n",
                name, name, name
            ));
        }
    }
    out
}

/// Emit `crates/server/src/graphql/generated/types.rs` — the GraphQL output types (SimpleObject),
/// mirroring `output_types_block`: entities.yaml types not registered in api.yaml `types`, then the
/// api.yaml types, each with its FK-derived navigation fields (data fields, resolved empty until the
/// read resolvers land). Includes the worked `From<RestaurantRow> for Restaurant` mapping (Stage 1a).
fn emit_server_types(model: &Model) -> String {
    let api = parse_api(model);
    let views = parse_views(model);
    let registered: HashSet<String> = api.types.iter().map(|t| t.name.clone()).collect();
    let nav = nav_fields(&views, &registered);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/api.yaml + specs/entities.yaml — do not edit by hand.\n// GraphQL output types (async-graphql SimpleObject), mirroring the generated SDL: entities.yaml types\n// not registered as api.yaml projections, then the api.yaml types, each with its FK-derived navigation\n// fields (plain data fields for now — resolved empty until the read resolvers land).\n#![allow(dead_code)]\n#![allow(non_camel_case_types)]\n\nuse application::projections::{CartRow, CatalogRow, CustomerRow, OrderTrackingRow, ProspectionPipelineRow, RestaurantRow};\nuse application::queries::{DeliveryJobRow, PricingPolicyRow, UberEstimationPolicyRow, UberSplitPolicyRow};\nuse domain::generated::scalars as ds;\n\nuse super::scalars::*;\n",
    );
    let push_nav = |out: &mut String, name: &str| {
        if let Some(nfs) = nav.get(name) {
            for n in nfs {
                let base = if n.list { format!("Vec<{}>", gql_rust_name(&n.target)) } else { gql_rust_name(&n.target) };
                push_gql_field(out, &n.field, &base, n.list || !n.nullable, None);
            }
        }
    };
    if let Some(Value::Mapping(m)) = model.defs.get("entities.yaml") {
        for (k, def) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            if registered.contains(name) {
                continue;
            }
            push_gql_struct_open(&mut out, name, "SimpleObject", def.get("description").and_then(|d| d.as_str()));
            push_gql_object_fields(&mut out, def, "entities.yaml", false);
            push_nav(&mut out, name);
            out.push_str("}\n");
        }
    }
    for t in &api.types {
        push_gql_struct_open(&mut out, &t.name, "SimpleObject", t.description.as_deref());
        for f in &t.properties {
            let base = rust_api_field_base(model, f, false);
            push_gql_field(&mut out, &f.name, &base, !f.nullable, f.description.as_deref());
        }
        push_nav(&mut out, &t.name);
        out.push_str("}\n");
    }
    // Worked example (Stage 1a): read-model row → API type. Mechanical columns map by conversion;
    // jsonb columns deserialize into the typed structs (serde camelCase); `orderable` is the derived
    // flag documented in api.yaml; nav fields resolve empty until the read resolvers land.
    out.push_str(
        "\n/// Read-model row → API type (Stage 1a worked example). jsonb columns deserialize into the typed\n/// structs; `orderable` = ACTIVE_PARTNER + status ACTIVE + acceptance != PAUSED (api.yaml); navigation\n/// fields resolve empty until the read resolvers land.\nimpl From<RestaurantRow> for Restaurant {\n    fn from(row: RestaurantRow) -> Self {\n        Self {\n            id: row.restaurant_id.into(),\n            account_id: row.restaurant_account_id.map(Into::into),\n            listing_status: row.listing_status.into(),\n            orderable: row.listing_status == ds::RestaurantListingStatus::ACTIVE_PARTNER\n                && row.status == ds::RestaurantStatus::ACTIVE\n                && row.order_acceptance != ds::OrderAcceptanceMode::PAUSED,\n            external_identifiers: row\n                .external_identifiers\n                .and_then(|v| serde_json::from_value(v).ok())\n                .unwrap_or_default(),\n            slug: row.slug.into(),\n            display_name: row.display_name.into(),\n            description: row.description,\n            tags: row.tags.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),\n            cuisine_category: row.cuisine_category.map(Into::into),\n            rating: row.rating.map(Into::into),\n            reviews_count: row.reviews_count,\n            website: row.website.map(Into::into),\n            gbp_order_url: row.gbp_order_url.map(Into::into),\n            gbp_link_status: row.gbp_link_status.map(Into::into),\n            address: serde_json::from_value(row.address).expect(\"Restaurant.address: invalid jsonb\"),\n            location: row.location.and_then(|v| serde_json::from_value(v).ok()),\n            opening_hours: serde_json::from_value(row.opening_hours).unwrap_or_default(),\n            status: row.status.into(),\n            order_acceptance: row.order_acceptance.into(),\n            default_currency: row.default_currency.into(),\n            timezone: row.timezone.map(Into::into),\n            preparation_time_minutes: row.preparation_time_minutes,\n            updated_at: row.updated_at,\n            delivery_jobs: Vec::new(),\n            prospects: Vec::new(),\n            catalogs: Vec::new(),\n            carts: Vec::new(),\n            orders: Vec::new(),\n        }\n    }\n}\n",
    );
    // Prospect: the FK-derived `restaurant` navigation field is NON-NULL, so the mapping takes the
    // joined Restaurant row alongside the pipeline row (the resolver performs the join).
    out.push_str(
        "\n/// Read-model rows → API type: the ProspectionPipeline row plus the joined Restaurant row (the\n/// FK-derived `restaurant` navigation field is non-null, so the resolver hydrates it from the\n/// Restaurant read model).\nimpl From<(ProspectionPipelineRow, RestaurantRow)> for Prospect {\n    fn from((row, restaurant): (ProspectionPipelineRow, RestaurantRow)) -> Self {\n        Self {\n            restaurant_id: row.restaurant_id.into(),\n            score: row.score.into(),\n            pipeline_status: row.pipeline_status.into(),\n            contacts_count: row.contacts_count,\n            last_contacted_at: row.last_contacted_at,\n            replied_at: row.replied_at,\n            restaurant: restaurant.into(),\n        }\n    }\n}\n",
    );
    // Catalog: categories/products/optionLists are carried inside the projected `tree` jsonb; the
    // FK-derived `restaurant` navigation field is NON-NULL, so the mapping takes the joined Restaurant
    // row (the resolver performs the join).
    out.push_str(
        "\n/// One section of the projected `Catalog.tree` jsonb (camelCase keys, as folded by the\n/// `CatalogProjector` with the derived per-offer `stockStatus`), leniently parsed: an absent key or\n/// an empty tree (a catalog created before any content event) yields an empty list.\npub(crate) fn catalog_tree_section<T: serde::de::DeserializeOwned>(tree: &serde_json::Value, key: &str) -> Vec<T> {\n    tree.get(key).cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()\n}\n",
    );
    out.push_str(
        "\n/// Read-model rows → API type: the Catalog row plus the joined Restaurant row (the FK-derived\n/// `restaurant` navigation field is non-null, so the resolver hydrates it from the Restaurant read\n/// model). categories/products/optionLists deserialize out of the projected `tree` jsonb.\nimpl From<(CatalogRow, RestaurantRow)> for Catalog {\n    fn from((row, restaurant): (CatalogRow, RestaurantRow)) -> Self {\n        Self {\n            id: row.catalog_id.into(),\n            restaurant_id: row.restaurant_id.into(),\n            slug: row.slug.into(),\n            name: row.name.into(),\n            categories: catalog_tree_section(&row.tree, \"categories\"),\n            products: catalog_tree_section(&row.tree, \"products\"),\n            option_lists: catalog_tree_section(&row.tree, \"optionLists\"),\n            updated_at: row.updated_at,\n            restaurant: restaurant.into(),\n        }\n    }\n}\n",
    );
    // Cart: jsonb columns deserialize into the typed structs (serde camelCase); the non-null
    // `restaurant` navigation field is hydrated by the resolver, as for Prospect.
    out.push_str(
        "\n/// Read-model rows → API type: the Cart row plus the joined Restaurant row (non-null `restaurant`\n/// navigation field). jsonb columns deserialize into the typed structs (serde camelCase); the priced\n/// columns are whatever the projector computed (documented TODO(runtime) until the pricing ports land).\nimpl From<(CartRow, RestaurantRow)> for Cart {\n    fn from((row, restaurant): (CartRow, RestaurantRow)) -> Self {\n        Self {\n            id: row.cart_id.into(),\n            restaurant_id: row.restaurant_id.into(),\n            customer_id: row.customer_id.map(Into::into),\n            status: row.status.into(),\n            lines: serde_json::from_value(row.lines).unwrap_or_default(),\n            total_amount: Money {\n                amount_cents: row.total_amount_cents.into(),\n                currency: row.currency.into(),\n            },\n            breakdown: row.estimated_breakdown.and_then(|v| serde_json::from_value(v).ok()),\n            uber_comparison: row.uber_comparison.and_then(|v| serde_json::from_value(v).ok()),\n            updated_at: row.updated_at,\n            restaurant: restaurant.into(),\n        }\n    }\n}\n",
    );
    // Order: minor-units columns + the row currency rebuild the Money values; the breakdown's
    // `restaurantContribution` is re-derived from the stored leaves; the Uber comparison needs every
    // uber_* column; paymentStatus is the projector's TEXT fold parsed leniently.
    out.push_str(
        "\n/// Minor-units column + the row's currency → the Money value object.\nfn order_money(cents: ds::MoneyCents, currency: &ds::CurrencyCode) -> Money {\n    Money { amount_cents: cents.into(), currency: currency.clone().into() }\n}\n",
    );
    out.push_str(
        "\n/// Read-model rows → API type: the OrderTracking row plus the joined Restaurant row (non-null\n/// `restaurant` navigation field). The breakdown's `restaurantContribution` is re-derived as\n/// articles − restaurantPayout (the projection stores the split's leaves); the Uber comparison is\n/// rebuilt only when every `uber_*` column is present; `paymentStatus` is folded as TEXT by the\n/// projector and parsed leniently (unknown → PENDING); nav `deliveryJobs` resolve empty until that\n/// read model lands.\nimpl From<(OrderTrackingRow, RestaurantRow)> for Order {\n    fn from((row, restaurant): (OrderTrackingRow, RestaurantRow)) -> Self {\n        let currency = row.currency.clone();\n        let breakdown = PaymentBreakdown {\n            articles: order_money(row.articles_cents.clone(), &currency),\n            delivery: order_money(row.delivery_cents.clone(), &currency),\n            service_fee: order_money(row.service_fee_cents.clone(), &currency),\n            total: order_money(row.total_amount_cents.clone(), &currency),\n            restaurant_contribution: order_money(\n                ds::MoneyCents(row.articles_cents.0 - row.restaurant_payout_cents.0),\n                &currency,\n            ),\n            restaurant_payout: order_money(row.restaurant_payout_cents.clone(), &currency),\n            rider_payout: order_money(row.rider_payout_cents.clone(), &currency),\n            captain_net: order_money(row.captain_net_cents.clone(), &currency),\n        };\n        let uber_comparison = match (\n            row.uber_total_cents,\n            row.uber_restaurant_cents,\n            row.uber_rider_cents,\n            row.uber_platform_cents,\n            row.uber_basis,\n        ) {\n            (Some(total), Some(restaurant_share), Some(rider_share), Some(platform_share), Some(basis)) => {\n                Some(UberComparison {\n                    total: order_money(total, &currency),\n                    restaurant_share: order_money(restaurant_share, &currency),\n                    rider_share: order_money(rider_share, &currency),\n                    platform_share: order_money(platform_share, &currency),\n                    basis: basis.into(),\n                })\n            }\n            _ => None,\n        };\n        Self {\n            id: row.order_id.into(),\n            r#ref: row.r#ref.into(),\n            restaurant_id: row.restaurant_id.into(),\n            customer_id: row.customer_id.map(Into::into),\n            status: row.status.into(),\n            service_type: row.service_type.into(),\n            items: serde_json::from_value(row.items).unwrap_or_default(),\n            total_amount: order_money(row.total_amount_cents, &currency),\n            breakdown,\n            delivery_address: row.delivery_address.and_then(|v| serde_json::from_value(v).ok()),\n            estimated_ready_at: row.estimated_ready_at,\n            placed_at: row.placed_at,\n            status_changed_at: row.status_changed_at,\n            payment_status: match row.payment_status.as_str() {\n                \"CAPTURED\" => PaymentStatus::CAPTURED,\n                \"FAILED\" => PaymentStatus::FAILED,\n                \"REFUNDED\" => PaymentStatus::REFUNDED,\n                _ => PaymentStatus::PENDING,\n            },\n            restaurant_stars: row.restaurant_stars.map(Into::into),\n            rating_comment: row.rating_comment.map(Into::into),\n            rider_thumb: row.rider_thumb.map(Into::into),\n            rider_tip: row.rider_tip_cents.map(|c| order_money(c, &currency)),\n            restaurant_tip: row.restaurant_tip_cents.map(|c| order_money(c, &currency)),\n            captain_tip: row.captain_tip_cents.map(|c| order_money(c, &currency)),\n            uber_comparison,\n            delivery_status: row.delivery_status.map(Into::into),\n            courier: row.courier.and_then(|v| serde_json::from_value(v).ok()),\n            estimated_dropoff_at: row.estimated_dropoff_at,\n            rated_at: row.rated_at,\n            delivery_jobs: Vec::new(),\n            restaurant: restaurant.into(),\n        }\n    }\n}\n",
    );
    // DeliveryJob: the View_DeliveryJob fold-view row (hand-written DTO — view-backed read models get
    // no generated row); both nav fields are NON-NULL, so the mapping takes the joined OrderTracking +
    // Restaurant rows (the resolver performs the joins).
    out.push_str(
        "\n/// Read-model rows → API type: the `View_DeliveryJob` row (ADR-0031/0039) plus the joined\n/// OrderTracking + Restaurant rows (the FK-derived `order`/`restaurant` navigation fields are\n/// non-null, so the resolver hydrates them — all three are projections of the same domain log).\n/// Addresses and the courier deserialize out of the view's jsonb columns.\nimpl From<(DeliveryJobRow, OrderTrackingRow, RestaurantRow)> for DeliveryJob {\n    fn from((row, order, restaurant): (DeliveryJobRow, OrderTrackingRow, RestaurantRow)) -> Self {\n        Self {\n            id: row.delivery_job_id.into(),\n            order_id: row.order_id.into(),\n            restaurant_id: row.restaurant_id.into(),\n            status: row.status.into(),\n            provider: row.provider.map(Into::into),\n            courier: row.courier.and_then(|v| serde_json::from_value(v).ok()),\n            pickup_address: serde_json::from_value(row.pickup_address)\n                .expect(\"DeliveryJob.pickupAddress: invalid jsonb\"),\n            dropoff_address: serde_json::from_value(row.dropoff_address)\n                .expect(\"DeliveryJob.dropoffAddress: invalid jsonb\"),\n            estimated_pickup_at: row.estimated_pickup_at,\n            estimated_dropoff_at: row.estimated_dropoff_at,\n            requested_at: row.requested_at,\n            picked_up_at: row.picked_up_at,\n            delivered_at: row.delivered_at,\n            order: (order, restaurant.clone()).into(),\n            restaurant: restaurant.into(),\n        }\n    }\n}\n",
    );
    // CustomerProfile: the `me` query's projection of the Customer identity row — only the profile
    // surface; the jsonb accumulation columns (ratings/favorites/preferences/addresses) stay internal.
    out.push_str(
        "\n/// Read-model row → API type: the Customer identity row behind the `me` query. Only the profile\n/// surface is exposed — the jsonb accumulation columns (ratings/favorites/preferences/addresses)\n/// stay internal to the read model.\nimpl From<CustomerRow> for CustomerProfile {\n    fn from(row: CustomerRow) -> Self {\n        Self {\n            customer_id: row.customer_id.into(),\n            display_name: row.display_name.map(Into::into),\n            email: row.email.map(Into::into),\n            email_verified: row.email_verified,\n            phone: row.phone.into(),\n            locale: row.locale.map(Into::into),\n            timezone: row.timezone.map(Into::into),\n        }\n    }\n}\n",
    );
    // Referential rows → API types (ADR-0037): the policy tables are seeded configuration, not
    // projections, so their hand-written rows live in `application::queries`.
    out.push_str(
        "\n/// Referential row → API type: the seeded `pricingpolicy` table (ADR-0016/0017).\nimpl From<PricingPolicyRow> for PricingPolicy {\n    fn from(row: PricingPolicyRow) -> Self {\n        Self {\n            currency: row.currency.into(),\n            fee_rate: row.fee_rate,\n            buyer_share: row.buyer_share,\n            margin_low: row.margin_low,\n            margin_high: row.margin_high,\n            effective_from: row.effective_from,\n        }\n    }\n}\n",
    );
    out.push_str(
        "\n/// Referential row → API type: the seeded `uberestimationpolicy` table (ADR-0024/0030).\nimpl From<UberEstimationPolicyRow> for UberEstimationPolicy {\n    fn from(row: UberEstimationPolicyRow) -> Self {\n        Self {\n            cuisine_category: row.cuisine_category.into(),\n            price_coefficient: row.price_coefficient,\n            effective_from: row.effective_from,\n        }\n    }\n}\n",
    );
    out.push_str(
        "\n/// Referential row → API type: the seeded `ubersplitpolicy` table (ADR-0024/0025/0030).\nimpl From<UberSplitPolicyRow> for UberSplitPolicy {\n    fn from(row: UberSplitPolicyRow) -> Self {\n        Self {\n            currency: row.currency.into(),\n            uber_commission_pct: row.uber_commission_pct,\n            rider_base_cents: row.rider_base_cents,\n            rider_per_km_cents: row.rider_per_km_cents,\n            avg_delivery_fee_cents: row.avg_delivery_fee_cents,\n            platform_fee_pct: row.platform_fee_pct,\n            effective_from: row.effective_from,\n        }\n    }\n}\n",
    );
    out
}

/// Emit `crates/server/src/graphql/generated/inputs.rs` — the GraphQL input types (InputObject),
/// mirroring `input_types_block`: one `<Command>Input` per mutation command, one `<Query>QueryInput`
/// per query with args, one `<Name>SubscriptionInput` per subscription with args, plus every entity
/// reachable from those payloads as `<Name>Input` (recursive, deduped).
fn emit_server_inputs(model: &Model) -> String {
    let api = parse_api(model);
    let mut needed: Vec<(String, String)> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/api.yaml + specs/commands.yaml — do not edit by hand.\n// GraphQL input types (async-graphql InputObject), mirroring the generated SDL: command payloads,\n// query/subscription args, and every entity reachable from them as `<Name>Input`.\n#![allow(dead_code)]\n\nuse super::scalars::*;\n",
    );

    for m in &api.mutations {
        if let Some(def) = model.defs.get("commands.yaml").and_then(|d| d.get(&m.command)) {
            push_gql_struct_open(&mut out, &format!("{}Input", m.command), "InputObject", def.get("description").and_then(|d| d.as_str()));
            push_gql_object_fields(&mut out, def, "commands.yaml", true);
            out.push_str("}\n");
            visit_inputs(model, &m.command, "commands.yaml", &mut needed, &mut visited);
        }
    }

    let scalars = scalar_names(model);
    for q in &api.queries {
        if q.args.is_empty() {
            continue;
        }
        push_gql_struct_open(&mut out, &format!("{}QueryInput", pascal(&q.name)), "InputObject", None);
        for a in &q.args {
            let base = rust_api_field_base(model, a, true);
            push_gql_field(&mut out, &a.name, &base, a.required, a.description.as_deref());
        }
        out.push_str("}\n");
        for a in &q.args {
            if a.is_ref && !scalars.contains(&a.ty) {
                visit_inputs(model, &a.ty, "entities.yaml", &mut needed, &mut visited);
            }
        }
    }

    for s in &api.subscriptions {
        if s.args.is_empty() {
            continue;
        }
        push_gql_struct_open(&mut out, &format!("{}SubscriptionInput", pascal(&s.name)), "InputObject", None);
        for a in &s.args {
            let base = rust_api_field_base(model, a, true);
            push_gql_field(&mut out, &a.name, &base, a.required, a.description.as_deref());
        }
        out.push_str("}\n");
        for a in &s.args {
            if a.is_ref && !scalars.contains(&a.ty) {
                visit_inputs(model, &a.ty, "entities.yaml", &mut needed, &mut visited);
            }
        }
    }

    let mut emitted: HashSet<String> = HashSet::new();
    for (name, file) in &needed {
        if emitted.contains(name) {
            continue;
        }
        emitted.insert(name.clone());
        if let Some(def) = model.defs.get(file).and_then(|d| d.get(name)) {
            push_gql_struct_open(&mut out, &format!("{}Input", name), "InputObject", def.get("description").and_then(|d| d.as_str()));
            push_gql_object_fields(&mut out, def, file, true);
            out.push_str("}\n");
        }
    }
    out
}

/// The api.yaml role name → the server's `RequestRole` variant (`RESTAURANT_ACCOUNT` →
/// `RestaurantAccount`).
fn acl_role_variant(role: &str) -> String {
    role.split('_').map(|seg| pascal(&seg.to_lowercase())).collect()
}

/// An operation's allowed-role set in canonical `scalars.yaml#/UserType` declaration order, or `None`
/// when the operation is public (`roles` include PUBLIC → open to every role, no guard/visible needed).
fn acl_role_set(model: &Model, roles: &[String]) -> Option<Vec<String>> {
    if roles.iter().any(|r| r == "PUBLIC") {
        return None;
    }
    let order = model
        .defs
        .get("scalars.yaml")
        .and_then(|v| v.get("UserType"))
        .and_then(|v| v.get("enum"))
        .and_then(|v| v.as_sequence())
        .map(|s| s.iter().filter_map(|x| x.as_str().map(|r| r.to_string())).collect::<Vec<_>>())
        .unwrap_or_default();
    Some(order.into_iter().filter(|r| roles.contains(r)).collect())
}

/// The identifier stem shared by a role set's generated const/fn (`[RESTAURANT_ACCOUNT, ADMIN]` →
/// `restaurant_account_admin` → `ALLOW_RESTAURANT_ACCOUNT_ADMIN` / `visible_restaurant_account_admin`).
fn acl_set_ident(set: &[String]) -> String {
    set.join("_").to_lowercase()
}

/// The `guard`/`visible` additions to a generated QueryRoot/MutationRoot field's `#[graphql(...)]`
/// attribute, from the operation's api.yaml `roles`. Empty for public operations.
fn acl_field_attr(model: &Model, roles: &[String]) -> String {
    match acl_role_set(model, roles) {
        Some(set) => {
            let ident = acl_set_ident(&set);
            format!(
                ", guard = \"RoleGuard::new(ALLOW_{})\", visible = \"visible_{}\"",
                ident.to_uppercase(),
                ident
            )
        }
        None => String::new(),
    }
}

/// Emit `crates/server/src/graphql/generated/acl.rs` — the spec-derived ACL data (ADR-0006): one
/// allowed-role const + one `visible` fn per distinct non-public role set found on api.yaml
/// queries/mutations. The generated QueryRoot/MutationRoot fields reference them as
/// `guard = "RoleGuard::new(ALLOW_…)"` (execution) and `visible = "visible_…"` (introspection); the
/// guard/lookup logic itself is the hand-written `graphql::acl` seam.
fn emit_server_acl(model: &Model) -> String {
    let api = parse_api(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/api.yaml — do not edit by hand.\n// Per-operation ACL role sets (ADR-0006 role-as-path): each distinct non-public `roles:` set on an\n// api.yaml query/mutation/subscription becomes an allowed-role const + a `visible` fn. The generated\n// QueryRoot/MutationRoot/SubscriptionRoot fields wire them as `guard = \"RoleGuard::new(ALLOW_…)\"`\n// (execution — unauthorized roles get a FORBIDDEN error) and `visible = \"visible_…\"` (introspection —\n// the field is hidden from unauthorized roles, and async-graphql's `find_visible_types` then hides\n// every type reachable only through hidden fields, so per-role introspection/Voyager expose only that\n// role's surface). Public operations (roles include PUBLIC) carry no guard/visible: open to every role.\n#![allow(dead_code)]\n\npub(crate) use super::super::acl::RoleGuard;\nuse super::super::acl::{role_allows, RequestRole};\n",
    );
    // Distinct non-public role sets across queries + mutations + subscriptions (the generated
    // SubscriptionRoot carries the same guard/visible pairs), keyed by identifier for a
    // deterministic, deduped emission order.
    let mut sets: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for roles in api
        .queries
        .iter()
        .map(|q| &q.roles)
        .chain(api.mutations.iter().map(|m| &m.roles))
        .chain(api.subscriptions.iter().map(|s| &s.roles))
    {
        if let Some(set) = acl_role_set(model, roles) {
            sets.insert(acl_set_ident(&set), set);
        }
    }
    for (ident, set) in &sets {
        let variants: Vec<String> =
            set.iter().map(|r| format!("RequestRole::{}", acl_role_variant(r))).collect();
        out.push_str(&format!(
            "\n/// roles: [{}]\npub(crate) const ALLOW_{}: &[RequestRole] = &[{}];\npub(crate) fn visible_{}(ctx: &async_graphql::Context<'_>) -> bool {{\n    role_allows(ctx, ALLOW_{})\n}}\n",
            set.join(", "),
            ident.to_uppercase(),
            variants.join(", "),
            ident,
            ident.to_uppercase()
        ));
    }
    out
}

/// Emit `crates/server/src/graphql/generated/query.rs` — the `QueryRoot`, mirroring `query_block`:
/// one async resolver per api.yaml query with the SDL argument/return shape. Every resolver returns
/// `Err("not implemented")` until the read-model repositories are injected (a later stage).
fn emit_server_query(model: &Model) -> String {
    let api = parse_api(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/api.yaml — do not edit by hand.\n// The GraphQL QueryRoot: one resolver per api.yaml query, matching the generated SDL shape. Resolvers\n// whose read-model repository is wired delegate to it (via ctx.data); the rest stub `not implemented`\n// until their repos land. Each non-public field carries its api.yaml `roles` as a `guard` (execution)\n// + `visible` (introspection) pair from the generated acl module (ADR-0006 role-as-path).\n#![allow(unused_variables)]\n#![allow(dead_code)]\n\nuse super::acl::*;\nuse super::inputs::*;\nuse super::types::*;\n\npub struct QueryRoot;\n\n#[async_graphql::Object(name = \"Query\")]\nimpl QueryRoot {\n",
    );
    for q in &api.queries {
        let fnname = rust_ident(&snake_field(&q.name));
        let acl = acl_field_attr(model, &q.roles);
        let arg = if q.args.is_empty() {
            String::new()
        } else {
            let ty = format!("{}QueryInput", pascal(&q.name));
            let ty = if q.args.iter().any(|a| a.required) { ty } else { format!("Option<{}>", ty) };
            format!(", input: {}", ty)
        };
        let inner = gql_rust_name(&q.returns_type);
        let mut ret = if q.returns_list { format!("Vec<{}>", inner) } else { inner };
        if q.returns_nullable {
            ret = format!("Option<{}>", ret);
        }
        push_doc(&mut out, "    ", q.description.as_deref());
        match wired_query_body(&q.name) {
            // Wired: delegate to the injected read-model repo (ctx.data); takes &Context.
            Some(body) => out.push_str(&format!(
                "    #[graphql(name = \"{}\"{})]\n    async fn {}(&self, ctx: &async_graphql::Context<'_>{}) -> async_graphql::Result<{}> {{\n{}\n    }}\n",
                q.name, acl, fnname, arg, ret, body
            )),
            None => out.push_str(&format!(
                "    #[graphql(name = \"{}\"{})]\n    async fn {}(&self{}) -> async_graphql::Result<{}> {{\n        Err(async_graphql::Error::new(\"not implemented\"))\n    }}\n",
                q.name, acl, fnname, arg, ret
            )),
        }
    }
    out.push_str("}\n");
    out
}

/// Resolver bodies for queries whose read-model repository is wired (injected via `ctx.data`). Returned as
/// the fn body (8-space indent); `None` → the `not implemented` stub. Extend as read repos land.
fn wired_query_body(name: &str) -> Option<&'static str> {
    match name {
        // The two Customer-vertical queries resolve through the Customer identity read model: `me`
        // maps the verified session Principal's authRef (ADR-0047/0015) to its Customer row;
        // `favoriteRestaurants` joins the row's projected favorite ids to the Restaurant read model.
        "me" => Some(
            "        // The verified session identity (ADR-0047), injected per-request by the HTTP layer. No\n        // principal (schema executed outside a request) or an anonymous one → no profile, not an error.\n        let Some(auth_ref) = ctx.data_opt::<crate::auth::Principal>().and_then(|p| p.user_id.clone()) else {\n            return Ok(None);\n        };\n        let customers = ctx.data::<std::sync::Arc<dyn application::queries::CustomerReadRepository>>()?;\n        let row = customers\n            .by_auth_ref(domain::generated::scalars::ExternalReference(auth_ref))\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(row.map(CustomerProfile::from))",
        ),
        "favoriteRestaurants" => Some(
            "        let customers = ctx.data::<std::sync::Arc<dyn application::queries::CustomerReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let Some(row) = customers.by_id(input.customer_id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else {\n            return Ok(Vec::new());\n        };\n        // The projected favorite set is a jsonb array of restaurant-id strings (CustomerProjector);\n        // resolve each against the Restaurant read model (an unknown id simply drops out).\n        let ids: Vec<uuid::Uuid> = row\n            .favorite_restaurant_ids\n            .as_array()\n            .map(|a| a.iter().filter_map(|v| v.as_str().and_then(|s| uuid::Uuid::parse_str(s).ok())).collect())\n            .unwrap_or_default();\n        let mut out = Vec::new();\n        for id in ids {\n            let found = restaurants\n                .by_id(domain::generated::scalars::RestaurantId(id))\n                .await\n                .map_err(|e| async_graphql::Error::new(e.to_string()))?;\n            if let Some(r) = found {\n                out.push(Restaurant::from(r));\n            }\n        }\n        Ok(out)",
        ),
        "restaurants" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let filter = input\n            .map(|i| application::queries::RestaurantFilter { search: i.search, orderable_only: i.orderable_only })\n            .unwrap_or_default();\n        let rows = repo.list(filter).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(Restaurant::from).collect())",
        ),
        "restaurant" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let row = repo.by_slug(input.slug.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(row.map(Restaurant::from))",
        ),
        "catalog" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::CatalogReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let Some(row) = repo.by_restaurant(input.restaurant_id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else {\n            return Ok(None);\n        };\n        // The non-null `restaurant` navigation field: hydrate from the Restaurant read model (both rows\n        // are projections of the same domain log, so the FK target always exists).\n        let restaurant = restaurants\n            .by_id(row.restaurant_id)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .ok_or_else(|| async_graphql::Error::new(\"catalog references an unknown restaurant\"))?;\n        Ok(Some(Catalog::from((row, restaurant))))",
        ),
        "categories" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::CatalogReadRepository>>()?;\n        let row = repo.by_restaurant(input.restaurant_id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        // Categories live inside the projected Catalog.tree jsonb; an absent catalog or an empty\n        // tree (a catalog created before any content event) yields an empty list.\n        Ok(row.map(|r| catalog_tree_section::<CatalogCategory>(&r.tree, \"categories\")).unwrap_or_default())",
        ),
        "carts" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::CartReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let rows = repo.by_customer(input.customer_id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        // The non-null `restaurant` navigation field: join against the Restaurant read model in memory\n        // (a cart is only ever started against a projected restaurant, so a match always exists).\n        let by_id: std::collections::HashMap<_, _> = restaurants\n            .list(application::queries::RestaurantFilter::default())\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .into_iter()\n            .map(|r| (r.restaurant_id.0, r))\n            .collect();\n        Ok(rows\n            .into_iter()\n            .filter_map(|c| by_id.get(&c.restaurant_id.0).cloned().map(|r| Cart::from((c, r))))\n            .collect())",
        ),
        "cart" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::CartReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let Some(row) = repo.by_id(input.id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else {\n            return Ok(None);\n        };\n        let restaurant = restaurants\n            .by_id(row.restaurant_id)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .ok_or_else(|| async_graphql::Error::new(\"cart references an unknown restaurant\"))?;\n        Ok(Some(Cart::from((row, restaurant))))",
        ),
        "orders" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::OrderReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let filter = input\n            .map(|i| application::queries::OrderFilter {\n                customer_id: i.customer_id.map(Into::into),\n                restaurant_id: i.restaurant_id.map(Into::into),\n                status: i.status.map(Into::into),\n            })\n            .unwrap_or_default();\n        let rows = repo.list(filter).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        // The non-null `restaurant` navigation field: join against the Restaurant read model in memory\n        // (an order is only ever placed against a projected restaurant, so a match always exists).\n        let by_id: std::collections::HashMap<_, _> = restaurants\n            .list(application::queries::RestaurantFilter::default())\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .into_iter()\n            .map(|r| (r.restaurant_id.0, r))\n            .collect();\n        Ok(rows\n            .into_iter()\n            .filter_map(|o| by_id.get(&o.restaurant_id.0).cloned().map(|r| Order::from((o, r))))\n            .collect())",
        ),
        "order" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::OrderReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let Some(row) = repo.by_id(input.id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else {\n            return Ok(None);\n        };\n        let restaurant = restaurants\n            .by_id(row.restaurant_id)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .ok_or_else(|| async_graphql::Error::new(\"order references an unknown restaurant\"))?;\n        Ok(Some(Order::from((row, restaurant))))",
        ),
        "restaurantLocationsByAccount" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let rows = repo.by_account(input.account_id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(Restaurant::from).collect())",
        ),
        // The three DeliveryJob queries read the View_DeliveryJob fold view (ADR-0031/0039). The
        // non-null `order`/`restaurant` navigation fields hydrate from their read models — all three
        // rows are projections of the same domain log.
        "delivery" => Some(
            "        let deliveries = ctx.data::<std::sync::Arc<dyn application::queries::DeliveryReadRepository>>()?;\n        let orders = ctx.data::<std::sync::Arc<dyn application::queries::OrderReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let Some(job) = deliveries.by_order(input.order_id.into()).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else {\n            return Ok(None);\n        };\n        let order = orders\n            .by_id(job.order_id)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .ok_or_else(|| async_graphql::Error::new(\"delivery references an unknown order\"))?;\n        let restaurant = restaurants\n            .by_id(job.restaurant_id)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .ok_or_else(|| async_graphql::Error::new(\"delivery references an unknown restaurant\"))?;\n        Ok(Some(DeliveryJob::from((job, order, restaurant))))",
        ),
        "myDeliveries" => Some(
            "        // The rider's identity is the verified session principal (ADR-0047): the rider app acts\n        // under its Supabase subject, which serves as the RiderId until a dedicated rider identity\n        // read model lands. No principal (schema executed outside a request) or an anonymous one →\n        // no jobs, not an error.\n        let Some(rider_id) = ctx\n            .data_opt::<crate::auth::Principal>()\n            .and_then(|p| p.user_id.as_deref())\n            .and_then(|s| uuid::Uuid::parse_str(s).ok())\n        else {\n            return Ok(Vec::new());\n        };\n        let deliveries = ctx.data::<std::sync::Arc<dyn application::queries::DeliveryReadRepository>>()?;\n        let orders = ctx.data::<std::sync::Arc<dyn application::queries::OrderReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let rows = deliveries\n            .for_rider(domain::generated::scalars::RiderId(rider_id), input.and_then(|i| i.status).map(Into::into))\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        // Non-null `order`/`restaurant` navigation fields: join by id (a job is only dispatched for a\n        // projected order+restaurant, so a missing target simply drops the job).\n        let mut out = Vec::new();\n        for job in rows {\n            let Some(order) = orders.by_id(job.order_id).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else { continue };\n            let Some(restaurant) = restaurants.by_id(job.restaurant_id).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else { continue };\n            out.push(DeliveryJob::from((job, order, restaurant)));\n        }\n        Ok(out)",
        ),
        "restaurantDeliveries" => Some(
            "        let deliveries = ctx.data::<std::sync::Arc<dyn application::queries::DeliveryReadRepository>>()?;\n        let orders = ctx.data::<std::sync::Arc<dyn application::queries::OrderReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let restaurant_id: domain::generated::scalars::RestaurantId = input.restaurant_id.into();\n        let rows = deliveries\n            .by_restaurant(restaurant_id, input.status.map(Into::into))\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        if rows.is_empty() {\n            return Ok(Vec::new());\n        }\n        // One board = one restaurant: hydrate the non-null `restaurant` navigation target once.\n        let restaurant = restaurants\n            .by_id(restaurant_id)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .ok_or_else(|| async_graphql::Error::new(\"delivery references an unknown restaurant\"))?;\n        let mut out = Vec::new();\n        for job in rows {\n            let Some(order) = orders.by_id(job.order_id).await.map_err(|e| async_graphql::Error::new(e.to_string()))? else { continue };\n            out.push(DeliveryJob::from((job, order, restaurant.clone())));\n        }\n        Ok(out)",
        ),
        "prospectionPipeline" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::ProspectionReadRepository>>()?;\n        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n        let filter = input\n            .map(|i| application::queries::ProspectFilter {\n                min_score: i.min_score.map(|s| s.0 as i32),\n                status: i.status.map(Into::into),\n            })\n            .unwrap_or_default();\n        let rows = repo.list(filter).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        // The non-null `restaurant` navigation field: join against the Restaurant read model in memory\n        // (both rows are folded from the same Restaurant-stream events, so a match always exists).\n        let by_id: std::collections::HashMap<_, _> = restaurants\n            .list(application::queries::RestaurantFilter::default())\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n            .into_iter()\n            .map(|r| (r.restaurant_id.0, r))\n            .collect();\n        Ok(rows\n            .into_iter()\n            .filter_map(|p| by_id.get(&p.restaurant_id.0).cloned().map(|r| Prospect::from((p, r))))\n            .collect())",
        ),
        // The three admin policy queries read seeded REFERENTIAL tables (ADR-0037) — no args, no input.
        "pricingPolicy" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::PricingPolicyReadRepository>>()?;\n        let rows = repo.list().await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(PricingPolicy::from).collect())",
        ),
        "uberEstimationPolicy" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::UberEstimationPolicyReadRepository>>()?;\n        let rows = repo.list().await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(UberEstimationPolicy::from).collect())",
        ),
        "uberSplitPolicy" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::UberSplitPolicyReadRepository>>()?;\n        let rows = repo.list().await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(UberSplitPolicy::from).collect())",
        ),
        _ => None,
    }
}

/// Emit `crates/server/src/graphql/generated/mutation.rs` — the `MutationRoot`, mirroring
/// `mutation_block` + `payloads_block`: one `<Name>Payload` SimpleObject per api.yaml mutation (every
/// payload carries `correlationId: CorrelationId!`) and one async resolver per mutation, taking the
/// generated `<Command>Input`. Resolvers whose command handler is wired delegate to it (write side:
/// EventStore + read/verification ports from ctx.data); the rest stub `not implemented` until their
/// aggregates land.
fn emit_server_mutation(model: &Model) -> String {
    let api = parse_api(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/api.yaml — do not edit by hand.\n// The GraphQL MutationRoot: one resolver per api.yaml mutation, matching the generated SDL shape\n// (input: <Command>Input! → <Name>Payload!, always carrying correlationId). Mutations whose command\n// handler is wired delegate to it (via ctx.data — EventStore + the ports the handler needs); the rest\n// stub `not implemented` until their aggregates land. Each non-public field carries its api.yaml\n// `roles` as a `guard` (execution) + `visible` (introspection) pair from the generated acl module\n// (ADR-0006 role-as-path).\n#![allow(unused_variables)]\n#![allow(dead_code)]\n\nuse super::acl::*;\nuse super::inputs::*;\nuse super::scalars::*;\n",
    );
    // Mutation payload output types (payloads_block's runtime mirror).
    for m in &api.mutations {
        push_gql_struct_open(&mut out, &format!("{}Payload", pascal(&m.name)), "SimpleObject", None);
        push_gql_field(
            &mut out,
            "correlationId",
            "CorrelationId",
            true,
            Some("Correlates this command with the events/state it produces (matches domain_events.correlation_id)."),
        );
        for f in &m.payload {
            let base = rust_api_field_base(model, f, false);
            push_gql_field(&mut out, &f.name, &base, !f.nullable, f.description.as_deref());
        }
        out.push_str("}\n");
    }
    out.push_str("\npub struct MutationRoot;\n\n#[async_graphql::Object(name = \"Mutation\")]\nimpl MutationRoot {\n");
    for m in &api.mutations {
        let fnname = rust_ident(&snake_field(&m.name));
        let payload = format!("{}Payload", pascal(&m.name));
        let acl = acl_field_attr(model, &m.roles);
        push_doc(&mut out, "    ", m.description.as_deref());
        match wired_mutation_body(&m.name, &payload) {
            // Wired: run the command handler over the injected write-side ports (ctx.data).
            Some(body) => out.push_str(&format!(
                "    #[graphql(name = \"{}\"{})]\n    async fn {}(&self, ctx: &async_graphql::Context<'_>, input: {}Input) -> async_graphql::Result<{}> {{\n{}\n    }}\n",
                m.name, acl, fnname, m.command, payload, body
            )),
            None => out.push_str(&format!(
                "    #[graphql(name = \"{}\"{})]\n    async fn {}(&self, input: {}Input) -> async_graphql::Result<{}> {{\n        Err(async_graphql::Error::new(\"not implemented\"))\n    }}\n",
                m.name, acl, fnname, m.command, payload
            )),
        }
    }
    out.push_str("}\n");
    // Shared write-side plumbing for the wired resolvers.
    out.push_str(
        "\n/// GraphQL input → domain command over the shared serde wire shape: both sides are generated from\n/// the same commands.yaml (camelCase), so the mapping is mechanical. `null`s are stripped first — an\n/// unset GraphQL optional serializes as an explicit null, while the domain payloads model absence as a\n/// MISSING key (`Option` fields / `#[serde(default)]` arrays).\nfn to_command<C: serde::de::DeserializeOwned>(input: &impl serde::Serialize) -> async_graphql::Result<C> {\n    let mut value = serde_json::to_value(input).map_err(|e| async_graphql::Error::new(e.to_string()))?;\n    strip_nulls(&mut value);\n    serde_json::from_value(value).map_err(|e| async_graphql::Error::new(e.to_string()))\n}\n\nfn strip_nulls(value: &mut serde_json::Value) {\n    match value {\n        serde_json::Value::Object(map) => {\n            map.retain(|_, v| !v.is_null());\n            for v in map.values_mut() {\n                strip_nulls(v);\n            }\n        }\n        serde_json::Value::Array(items) => {\n            for v in items.iter_mut() {\n                strip_nulls(v);\n            }\n        }\n        _ => {}\n    }\n}\n\n/// The acting user stamped on the event envelope (ADR-0041). Authn/ACL is a separate workstream: until\n/// it lands, every mutation runs as the anonymous PUBLIC principal with a fresh correlation id (also\n/// returned in the payload so the client can track the outcome on the read side).\nfn request_actor(_ctx: &async_graphql::Context<'_>) -> application::ports::Actor {\n    application::ports::Actor {\n        user_id: uuid::Uuid::nil(),\n        user_type: 0, // UserType::PUBLIC ordinal (enums are declaration-order integers, ADR-0037)\n        correlation_id: uuid::Uuid::new_v4(),\n        cause_id: None,\n    }\n}\n\n/// Map a command rejection onto the GraphQL error contract (P-10): an anticipated errors.yaml\n/// rejection surfaces `extensions.code` = the stable PascalCase code, the interpolated English\n/// message as the error message, and its typed context fields under the extensions; anything\n/// unexpected (repository/adapter failures) surfaces as the generic catalogued `Internal` — never\n/// leaking adapter details to the client.\nfn domain_error(e: domain::shared::errors::DomainError) -> async_graphql::Error {\n    use async_graphql::ErrorExtensions;\n    use domain::shared::errors::DomainError;\n    match e {\n        DomainError::Rejected { code, context } => {\n            let message = domain::generated::errors::message_en(&code, &context)\n                .unwrap_or_else(|| code.clone());\n            async_graphql::Error::new(message).extend_with(|_, ext| {\n                ext.set(\"code\", code.as_str());\n                if let Some(fields) = context.as_object() {\n                    for (key, value) in fields {\n                        if key == \"code\" {\n                            continue; // never let a context field shadow the wire code\n                        }\n                        ext.set(\n                            key.as_str(),\n                            async_graphql::Value::from_json(value.clone())\n                                .unwrap_or(async_graphql::Value::Null),\n                        );\n                    }\n                }\n            })\n        }\n        // Legacy \"<Code>: <detail>\" string invariants (interim adapters, e.g. the fail-closed\n        // payment stand-in): surface the prefix when it is a catalogued code, else it is unexpected.\n        DomainError::Invariant(msg) => {\n            let code = msg.split(':').next().map(str::trim).unwrap_or(\"\").to_string();\n            if domain::generated::errors::find(&code).is_some() {\n                async_graphql::Error::new(msg).extend_with(|_, ext| ext.set(\"code\", code.as_str()))\n            } else {\n                internal_error()\n            }\n        }\n        DomainError::Repository(_) => internal_error(),\n    }\n}\n\n/// The generic catalogued `Internal` fallback (errors.yaml): unexpected/infrastructure failures\n/// never leak their detail to the client.\nfn internal_error() -> async_graphql::Error {\n    use async_graphql::ErrorExtensions;\n    let def = domain::generated::errors::INTERNAL;\n    async_graphql::Error::new(def.message_en).extend_with(|_, ext| ext.set(\"code\", def.code))\n}\n",
    );
    out
}

/// Resolver bodies for mutations whose command handler is wired (the Restaurant aggregate — the proven
/// write vertical). Returned as the fn body (8-space indent); `None` → the `not implemented` stub.
/// Extend as more aggregates land. `payload` is the mutation's `<Name>Payload` Rust type.
fn wired_mutation_body(name: &str, payload: &str) -> Option<String> {
    // verifyPhone is the one payload carrying more than the correlation id (customerId + created):
    // the handler returns a VerifyPhoneOutcome the resolver maps into the payload, so it gets a
    // bespoke body instead of the generic fire-and-ack template below.
    if name == "verifyPhone" {
        return Some(format!(
            "        let store = ctx.data::<std::sync::Arc<dyn application::ports::EventStore>>()?;\n        let auth = ctx.data::<std::sync::Arc<dyn application::ports::AuthProviderGateway>>()?;\n        let customers = ctx.data::<std::sync::Arc<dyn application::queries::CustomerReadRepository>>()?;\n        let cmd: domain::generated::commands::VerifyPhone = to_command(&input)?;\n        let actor = request_actor(ctx);\n        let outcome = application::commands::verify_phone(store.as_ref(), auth.as_ref(), customers.as_ref(), cmd, &actor)\n            .await\n            .map_err(domain_error)?;\n        Ok({payload} {{\n            correlation_id: CorrelationId(actor.correlation_id),\n            customer_id: outcome.customer_id.into(),\n            created: outcome.created,\n        }})"
        ));
    }
    // placeOrder's payload carries the Stripe-assigned values (paymentIntentId + clientSecret): the
    // handler returns the CreatedPaymentIntent the resolver maps into the payload, and it needs the
    // CartReadRepository (server-side pricing) + PaymentGateway (create-intent seam; the composition
    // root injects the fail-closed Stripe stand-in until the real adapter lands) + the
    // PaymentProcessStateStore (the payment_process_manager row the handler opens
    // AWAITING_PAYMENT_RESULT and single-flights concurrent checkouts of the same cart on,
    // ADR-20260719-193500) — so it gets a bespoke body like verifyPhone. The saga's event legs
    // (PaymentCaptured/PaymentFailed) run in the infrastructure ProcessManagerRunner, not here.
    if name == "placeOrder" {
        return Some(format!(
            "        let store = ctx.data::<std::sync::Arc<dyn application::ports::EventStore>>()?;\n        let carts = ctx.data::<std::sync::Arc<dyn application::queries::CartReadRepository>>()?;\n        let payments = ctx.data::<std::sync::Arc<dyn application::ports::PaymentGateway>>()?;\n        let pm_state = ctx.data::<std::sync::Arc<dyn application::pm_state::PaymentProcessStateStore>>()?;\n        let cmd: domain::generated::commands::PlaceOrder = to_command(&input)?;\n        let actor = request_actor(ctx);\n        let intent = application::commands::place_order(store.as_ref(), carts.as_ref(), payments.as_ref(), pm_state.as_ref(), cmd, &actor)\n            .await\n            .map_err(domain_error)?;\n        Ok({payload} {{\n            correlation_id: CorrelationId(actor.correlation_id),\n            payment_intent_id: intent.payment_intent_id.into(),\n            client_secret: intent.client_secret,\n        }})"
        ));
    }
    // (domain command, application::commands handler, extra port beyond the EventStore).
    enum Extra {
        None,
        /// `RestaurantReadRepository` — backs the SlugAlreadyTaken/RestaurantNotFound and
        /// CurrencyMismatch (default currency) checks.
        Restaurants,
        /// `GoogleOwnershipVerifier` — GBP ownership proof (ADR-0019).
        Ownership,
        /// `GbpOrderLinkProbe` — GBP 'Order online' link ping (ADR-0021).
        Probe,
        /// `ProspectionReadRepository` — backs the ProspectContactedTooRecently check (ADR-0020).
        Prospection,
        /// `CatalogReadRepository` — the offer-level live-catalog lookups behind the Cart line
        /// invariants (OfferNotFound / OfferUnavailable / InsufficientStock / InvalidOptionSelection).
        Catalogs,
        /// `AuthProviderGateway` — the wrapped Supabase Auth ACL boundary (ADR-0015).
        Auth,
        /// `AuthProviderGateway` + `CustomerReadRepository` — identity flows that also need the
        /// phone/email uniqueness-and-resolution lookups.
        AuthCustomers,
    }
    let (command, handler, extra) = match name {
        "registerRestaurant" => ("RegisterRestaurant", "register_restaurant", Extra::Restaurants),
        "activateRestaurant" => ("ActivateRestaurant", "activate_restaurant", Extra::None),
        "updateRestaurant" => ("UpdateRestaurant", "update_restaurant", Extra::None),
        "deactivateRestaurant" => ("DeactivateRestaurant", "deactivate_restaurant", Extra::None),
        "removeRestaurant" => ("RemoveRestaurant", "remove_restaurant", Extra::None),
        "changeOrderAcceptanceMode" => {
            ("ChangeOrderAcceptanceMode", "change_order_acceptance_mode", Extra::None)
        }
        "updateRestaurantGoogleBusinessProfile" => (
            "UpdateRestaurantGoogleBusinessProfile",
            "update_restaurant_google_business_profile",
            Extra::None,
        ),
        "markRestaurantClosed" => ("MarkRestaurantClosed", "mark_restaurant_closed", Extra::None),
        "claimRestaurantListing" => {
            ("ClaimRestaurantListing", "claim_restaurant_listing", Extra::Ownership)
        }
        "optOutRestaurantListing" => {
            ("OptOutRestaurantListing", "opt_out_restaurant_listing", Extra::Ownership)
        }
        "changeRestaurantListingStatus" => {
            ("ChangeRestaurantListingStatus", "change_restaurant_listing_status", Extra::None)
        }
        "configureGbpOrderLink" => {
            ("ConfigureGoogleBusinessProfileOrderLink", "configure_gbp_order_link", Extra::None)
        }
        "verifyGbpOrderLink" => {
            ("VerifyGoogleBusinessProfileOrderLink", "verify_gbp_order_link", Extra::Probe)
        }
        // Cart aggregate (ADR-0046 round 2: the checkout→order→delivery flow). Line-level commands
        // validate against the LIVE catalog (offer-level read port over the projected tree).
        "addCartLine" => ("AddCartLine", "add_cart_line", Extra::Catalogs),
        "removeCartLine" => ("RemoveCartLine", "remove_cart_line", Extra::None),
        "changeCartLineQuantity" => {
            ("ChangeCartLineQuantity", "change_cart_line_quantity", Extra::Catalogs)
        }
        // Order aggregate.
        "acceptOrder" => ("AcceptOrder", "accept_order", Extra::None),
        "rejectOrder" => ("RejectOrder", "reject_order", Extra::None),
        "startPreparation" => ("StartPreparation", "start_preparation", Extra::None),
        "markOrderReady" => ("MarkOrderReady", "mark_order_ready", Extra::None),
        "markOrderDelivered" => ("MarkOrderDelivered", "mark_order_delivered", Extra::None),
        "cancelOrderByCustomer" => ("CancelOrderByCustomer", "cancel_order_by_customer", Extra::None),
        "cancelOrderByRestaurant" => {
            ("CancelOrderByRestaurant", "cancel_order_by_restaurant", Extra::None)
        }
        "rateOrder" => ("RateOrder", "rate_order", Extra::None),
        "rateRestaurant" => ("RateRestaurant", "rate_restaurant", Extra::None),
        "tipOrder" => ("TipOrder", "tip_order", Extra::None),
        "requestRefund" => ("RequestRefund", "request_refund", Extra::None),
        // DeliveryJob aggregate (independent-rider fulfilment, ADR-0031).
        "acceptDelivery" => ("AcceptDelivery", "accept_delivery", Extra::None),
        "confirmPickup" => ("ConfirmPickup", "confirm_pickup", Extra::None),
        "completeDelivery" => ("CompleteDelivery", "complete_delivery", Extra::None),
        "cancelDelivery" => ("CancelDelivery", "cancel_delivery", Extra::None),
        // placeOrder is handled by the bespoke body above (Stripe-assigned payload fields).
        // RestaurantAccount aggregate.
        "registerRestaurantAccount" => {
            ("RegisterRestaurantAccount", "register_restaurant_account", Extra::None)
        }
        "updateRestaurantAccount" => {
            ("UpdateRestaurantAccount", "update_restaurant_account", Extra::None)
        }
        "deleteRestaurantAccount" => {
            ("DeleteRestaurantAccount", "delete_restaurant_account", Extra::None)
        }
        // Prospect aggregate (ADR-0020).
        "recordProspectContact" => {
            ("RecordProspectContact", "record_prospect_contact", Extra::Prospection)
        }
        "markProspectCold" => ("MarkProspectCold", "mark_prospect_cold", Extra::None),
        "recordProspectReply" => ("RecordProspectReply", "record_prospect_reply", Extra::None),
        // Catalog aggregate.
        "createCatalog" => ("CreateCatalog", "create_catalog", Extra::Restaurants),
        "addProduct" => ("AddProduct", "add_product", Extra::Restaurants),
        "updateProduct" => ("UpdateProduct", "update_product", Extra::Restaurants),
        "removeProduct" => ("RemoveProduct", "remove_product", Extra::None),
        "addCatalogCategory" => ("AddCatalogCategory", "add_catalog_category", Extra::None),
        "updateCatalogCategory" => ("UpdateCatalogCategory", "update_catalog_category", Extra::None),
        "removeCatalogCategory" => ("RemoveCatalogCategory", "remove_catalog_category", Extra::None),
        "addOptionList" => ("AddOptionList", "add_option_list", Extra::None),
        "updateOptionList" => ("UpdateOptionList", "update_option_list", Extra::None),
        "removeOptionList" => ("RemoveOptionList", "remove_option_list", Extra::None),
        "updateOfferStock" => ("UpdateOfferStock", "update_offer_stock", Extra::None),
        "importCatalog" => ("ImportCatalog", "import_catalog", Extra::None),
        // Customer aggregate (wrapped Supabase Auth, ADR-0015).
        "requestPhoneVerification" => {
            ("RequestPhoneVerification", "request_phone_verification", Extra::Auth)
        }
        "requestEmailVerification" => {
            ("RequestEmailVerification", "request_email_verification", Extra::AuthCustomers)
        }
        "confirmEmailVerification" => {
            ("ConfirmEmailVerification", "confirm_email_verification", Extra::Auth)
        }
        "requestPhoneChange" => ("RequestPhoneChange", "request_phone_change", Extra::AuthCustomers),
        "confirmPhoneChange" => ("ConfirmPhoneChange", "confirm_phone_change", Extra::AuthCustomers),
        "changeLanguage" => ("ChangeLanguage", "change_language", Extra::None),
        "markRestaurantAsFavorite" => {
            ("MarkRestaurantAsFavorite", "mark_restaurant_as_favorite", Extra::Restaurants)
        }
        "unmarkRestaurantAsFavorite" => {
            ("UnmarkRestaurantAsFavorite", "unmark_restaurant_as_favorite", Extra::None)
        }
        "updateCustomerInfo" => ("UpdateCustomerInfo", "update_customer_info", Extra::None),
        "setCustomerPreferences" => ("SetCustomerPreferences", "set_customer_preferences", Extra::None),
        "setCustomerAddress" => ("SetCustomerAddress", "set_customer_address", Extra::None),
        "removeCustomerAddress" => ("RemoveCustomerAddress", "remove_customer_address", Extra::None),
        "setCustomerPaymentMethod" => {
            ("SetCustomerPaymentMethod", "set_customer_payment_method", Extra::None)
        }
        _ => return None,
    };
    let (resolve_extra, extra_arg) = match extra {
        Extra::None => (String::new(), ""),
        Extra::Restaurants => (
            "        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?;\n".to_string(),
            ", restaurants.as_ref()",
        ),
        Extra::Ownership => (
            "        let ownership = ctx.data::<std::sync::Arc<dyn application::ports::GoogleOwnershipVerifier>>()?;\n".to_string(),
            ", ownership.as_ref()",
        ),
        Extra::Probe => (
            "        let probe = ctx.data::<std::sync::Arc<dyn application::ports::GbpOrderLinkProbe>>()?;\n".to_string(),
            ", probe.as_ref()",
        ),
        Extra::Prospection => (
            "        let prospection = ctx.data::<std::sync::Arc<dyn application::queries::ProspectionReadRepository>>()?;\n".to_string(),
            ", prospection.as_ref()",
        ),
        Extra::Catalogs => (
            "        let catalogs = ctx.data::<std::sync::Arc<dyn application::queries::CatalogReadRepository>>()?;\n".to_string(),
            ", catalogs.as_ref()",
        ),
        Extra::Auth => (
            "        let auth = ctx.data::<std::sync::Arc<dyn application::ports::AuthProviderGateway>>()?;\n".to_string(),
            ", auth.as_ref()",
        ),
        Extra::AuthCustomers => (
            "        let auth = ctx.data::<std::sync::Arc<dyn application::ports::AuthProviderGateway>>()?;\n        let customers = ctx.data::<std::sync::Arc<dyn application::queries::CustomerReadRepository>>()?;\n".to_string(),
            ", auth.as_ref(), customers.as_ref()",
        ),
    };
    Some(format!(
        "        let store = ctx.data::<std::sync::Arc<dyn application::ports::EventStore>>()?;\n{resolve_extra}        let cmd: domain::generated::commands::{command} = to_command(&input)?;\n        let actor = request_actor(ctx);\n        application::commands::{handler}(store.as_ref(){extra_arg}, cmd, &actor)\n            .await\n            .map_err(domain_error)?;\n        Ok({payload} {{ correlation_id: CorrelationId(actor.correlation_id) }})"
    ))
}

/// Emit `crates/server/src/graphql/generated/subscription.rs` — the `SubscriptionRoot`, mirroring
/// `subscription_block`: one stream resolver per api.yaml subscription with the SDL argument/return
/// shape. Wired resolvers subscribe to the in-process `infrastructure::EventBus` (each envelope is
/// published by `PgEventStore::append` AFTER a successful commit) and map matching envelopes onto the
/// declared return type — re-resolving the read models rather than exposing raw `domain_events`. Each
/// non-public field carries the same generated `guard`/`visible` ACL pair as queries/mutations.
fn emit_server_subscription(model: &Model) -> String {
    let api = parse_api(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/api.yaml — do not edit by hand.\n// The GraphQL SubscriptionRoot: one stream resolver per api.yaml subscription, matching the generated\n// SDL shape. Wired resolvers subscribe to the in-process EventBus (each envelope is published by\n// PgEventStore::append AFTER a successful commit) and map matching envelopes onto the declared return\n// type — re-resolving the read models rather than exposing raw domain_events (ADR-0005/0035). Each\n// non-public field carries its api.yaml `roles` as a `guard` (execution) + `visible` (introspection)\n// pair from the generated acl module (ADR-0006 role-as-path).\n//\n// Free-tier caveat: the bus is IN-PROCESS and a GraphQL-over-WebSocket connection lives only while\n// the app instance is warm (the uptimerobot ping keeps it so); after a restart/redeploy clients must\n// resubscribe and re-sync via the pull queries (`order`, `operation`).\n#![allow(unused_variables)]\n#![allow(dead_code)]\n\nuse async_graphql::futures_util::Stream;\n\nuse super::acl::*;\nuse super::inputs::*;\nuse super::scalars::*;\nuse super::types::*;\n\npub struct SubscriptionRoot;\n\n#[async_graphql::Subscription(name = \"Subscription\")]\nimpl SubscriptionRoot {\n",
    );
    for s in &api.subscriptions {
        let fnname = rust_ident(&snake_field(&s.name));
        let acl = acl_field_attr(model, &s.roles);
        let arg = if s.args.is_empty() {
            String::new()
        } else {
            let ty = format!("{}SubscriptionInput", pascal(&s.name));
            let ty = if s.args.iter().any(|a| a.required) { ty } else { format!("Option<{}>", ty) };
            format!(", input: {}", ty)
        };
        let inner = gql_rust_name(&s.returns_type);
        let mut ret = if s.returns_list { format!("Vec<{}>", inner) } else { inner };
        if s.returns_nullable {
            ret = format!("Option<{}>", ret);
        }
        push_doc(&mut out, "    ", s.description.as_deref());
        match wired_subscription_body(&s.name) {
            // Wired: stream over the injected EventBus (+ read repos) from ctx.data.
            Some(body) => out.push_str(&format!(
                "    #[graphql(name = \"{}\"{})]\n    async fn {}(&self, ctx: &async_graphql::Context<'_>{}) -> async_graphql::Result<impl Stream<Item = async_graphql::Result<{}>>> {{\n{}\n    }}\n",
                s.name, acl, fnname, arg, ret, body
            )),
            None => out.push_str(&format!(
                "    #[graphql(name = \"{}\"{})]\n    async fn {}(&self{}) -> async_graphql::Result<impl Stream<Item = async_graphql::Result<{}>>> {{\n        Err::<async_graphql::futures_util::stream::Empty<async_graphql::Result<{}>>, _>(async_graphql::Error::new(\"not implemented\"))\n    }}\n",
                s.name, acl, fnname, arg, ret, ret
            )),
        }
    }
    out.push_str("}\n");
    out
}

/// Resolver bodies for subscriptions wired over the EventBus + read models. Returned as the fn body
/// (8-space indent); `None` → the `not implemented` stub. `orderStatusChanged` re-resolves the Order
/// read row per matching envelope (dedupe + terminal completion); `operationStatusChanged` maps each
/// matching envelope onto a SUCCEEDED `Operation` tick (the transient, non-projected type).
fn wired_subscription_body(name: &str) -> Option<&'static str> {
    match name {
        // A bus envelope EXISTS only after its append committed, so every matching envelope is a
        // durable SUCCEEDED confirmation (one per emitted event). Rejections/failures return inline
        // on the mutation itself and never reach the log — the `operation` query is the pull
        // counterpart for the full PENDING/REJECTED/FAILED picture.
        "operationStatusChanged" => Some(
            r#"        let bus = ctx.data::<infrastructure::EventBus>()?.clone();
        let wanted = input.correlation_id.0;
        let mut rx = bus.subscribe();
        Ok(async_stream::stream! {
            loop {
                match rx.recv().await {
                    Ok(evt) if evt.correlation_id == wanted => {
                        yield Ok(Operation {
                            correlation_id: CorrelationId(evt.correlation_id),
                            status: OperationStatus::SUCCEEDED,
                            message: Some(format!("{} ({} v{})", evt.event_type, evt.stream_name, evt.position)),
                            occurred_at: chrono::Utc::now(),
                        });
                    }
                    Ok(_) => {}
                    // Lagged: skipped envelopes are harmless for a liveness tick.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        })"#,
        ),
        // Push-based order tracking: each matching envelope re-resolves the CURRENT Order from the
        // read model (queries never read raw domain_events), dedupes identical consecutive states
        // and completes on a terminal status.
        "orderStatusChanged" => Some(
            r#"        let bus = ctx.data::<infrastructure::EventBus>()?.clone();
        let orders = ctx.data::<std::sync::Arc<dyn application::queries::OrderReadRepository>>()?.clone();
        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?.clone();
        let wanted = input.correlation_id.0;
        let mut rx = bus.subscribe();
        Ok(async_stream::stream! {
            use domain::generated::scalars as ds;
            // The last state pushed to this subscriber: (status, row updated_at). The timestamp
            // advances on EVERY projected fold, so "identical to last" distinguishes a not-yet-folded
            // event (re-poll briefly) from a fold that truly left the status unchanged (dedupe).
            let mut last: Option<(ds::OrderStatus, chrono::DateTime<chrono::Utc>)> = None;
            'events: loop {
                let evt = match rx.recv().await {
                    Ok(evt) => evt,
                    // Lagged: skipped envelopes are harmless — the next matching one re-resolves the
                    // CURRENT state anyway.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                if evt.correlation_id != wanted {
                    continue;
                }
                // The Order stream name carries the order id (`Order-<uuid>`); other streams under
                // the same correlation (Cart, DeliveryJob, ...) don't move the Order read model.
                let Some(order_id) = evt
                    .stream_name
                    .strip_prefix("Order-")
                    .and_then(|s| uuid::Uuid::parse_str(s).ok())
                else {
                    continue;
                };
                // The row is folded ASYNCHRONOUSLY by the projection worker (ADR-0040): give it a
                // bounded window to absorb this event before treating it as a no-op.
                for attempt in 0..12u32 {
                    if attempt > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    }
                    let row = match orders.by_id(ds::OrderId(order_id)).await {
                        Ok(Some(row)) => row,
                        Ok(None) => continue, // not projected yet — re-poll
                        Err(e) => {
                            yield Err(async_graphql::Error::new(e.to_string()));
                            continue 'events;
                        }
                    };
                    if last == Some((row.status, row.updated_at)) {
                        continue; // fold not visible yet — re-poll
                    }
                    if last.map(|(status, _)| status) == Some(row.status) {
                        // A fold landed but the status is unchanged — dedupe, don't re-push.
                        last = Some((row.status, row.updated_at));
                        continue 'events;
                    }
                    last = Some((row.status, row.updated_at));
                    let terminal = matches!(
                        row.status,
                        ds::OrderStatus::REJECTED
                            | ds::OrderStatus::DELIVERED
                            | ds::OrderStatus::CANCELLED_BY_CUSTOMER
                            | ds::OrderStatus::CANCELLED_BY_RESTAURANT
                    );
                    // The non-null `restaurant` navigation field: hydrate like the `order` query does.
                    match restaurants.by_id(row.restaurant_id).await {
                        Ok(Some(restaurant)) => yield Ok(Order::from((row, restaurant))),
                        Ok(None) => {}
                        Err(e) => yield Err(async_graphql::Error::new(e.to_string())),
                    }
                    if terminal {
                        break 'events; // terminal status — complete the subscription
                    }
                    continue 'events;
                }
            }
        })"#,
        ),
        _ => None,
    }
}

/// Repo root, derived from the `--specs` path's parent (so generated crate files land correctly whether
/// `--specs` is relative like `specs` or an absolute path).
fn repo_root(specs: &std::path::Path) -> PathBuf {
    match specs.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let check = args.iter().any(|a| a == "--check");
    let specs = arg_value(&args, "--specs")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("specs"));

    let model = match load_model(&specs) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("✗ load error: {}", e);
            std::process::exit(1);
        }
    };

    let Report { issues, coverage, handled_commands } = validate(&model);
    let errors: Vec<&Issue> = issues.iter().filter(|i| i.level == Level::Error).collect();
    let warnings: Vec<&Issue> = issues.iter().filter(|i| i.level == Level::Warning).collect();

    // Summary counts (mirrors cli.ts), derived from the model.
    let n_actors = parse_actors(&model).len();
    let n_commands = handled_commands; // cli.ts prints derived.handledCommands.size, not total defs
    let n_events = map_keys(model.defs.get("events.yaml")).len();
    let n_errdefs = map_keys(model.defs.get("errors.yaml")).len();
    let n_personas = parse_stories(&model).len();
    let n_activities: usize = parse_stories(&model).iter().map(|p| p.activities.len()).sum();
    let n_fixtures = model
        .defs
        .get("tests.yaml")
        .and_then(|t| t.get("fixtures"))
        .and_then(|f| f.as_mapping())
        .map(|m| m.len())
        .unwrap_or(0);
    let n_bcs = model
        .defs
        .get("architecture/c4-l2.yaml")
        .and_then(|v| v.get("boundedContexts"))
        .and_then(|x| x.as_mapping())
        .map(|m| m.len())
        .unwrap_or(0);

    eprintln!("• specs:  {}", specs.display());
    eprintln!("• model:  {} actors, {} commands, {} events, {} errors", n_actors, n_commands, n_events, n_errdefs);
    let api_s = parse_api(&model);
    eprintln!("• api:    {} mutations, {} queries, {} projections", api_s.mutations.len(), api_s.queries.len(), api_s.types.len());
    eprintln!("• stories:{} personas, {} activities", n_personas, n_activities);
    eprintln!("• views:  {} views, {} columns, {} fedBy links", coverage.views, coverage.view_columns, coverage.view_fed_by);
    eprintln!("• tests:  {} behaviour tests, {} fixtures, {} business rules", coverage.test_cases, n_fixtures, coverage.rules);
    eprintln!("• obs:    {} observability contracts · C4: {} bounded contexts", coverage.obs_contracts, n_bcs);
    eprintln!(
        "• ui:     {} SDUI screens, {} API bindings, {} gaps · {} translation keys (en/fr)",
        coverage.screens, coverage.screen_bindings, coverage.screen_gaps, coverage.translations
    );
    eprintln!("• validated against specs:");
    eprintln!("    - {} $refs resolve (scalars/entities/events/commands/errors/views/api)", coverage.refs);
    eprintln!("    - actor wiring: messages→commands/events, emits→events, throws→errors");
    eprintln!("    - api↔model: {} command links→commands, {} reads→views, roles→UserType", coverage.mutation_links, coverage.reads_links);
    eprintln!("    - views: aggregate→actors, fedBy→events, column types→scalars, indexes→columns, fk→views");
    eprintln!("    - stories: {} step→op links resolve, persona role authorized, every mutation/query reached by a story step", coverage.story_links);
    eprintln!("    - tests: {} Given/When/Then cases — data fields, actor handles `when`, `then`⊆emits, `thrown`⊆throws; every message/event/error exercised", coverage.test_cases);
    eprintln!("    - rules: {} business rules — every test asserts ≥1 rule, every rule asserted by ≥1 test (ADR-0032)", coverage.rules);
    eprintln!("    - ui: {} SDUI screens — resolver/action bindings $ref real api ops (API-meets-UI), data_requirements resolve; {} translations (en+fr, params match)", coverage.screens, coverage.translations);
    eprintln!("    - observability: {} workflow contracts — $ref bindings resolve, mandatory ids (correlation_id/trace_id), span kinds, success.required_spans ⊆ declared spans", coverage.obs_contracts);
    eprintln!("    - c4: bounded-context↔actor mapping (no unmapped aggregate / phantom container ref)");

    if !issues.is_empty() {
        eprintln!("• checks: {} error(s), {} warning(s)", errors.len(), warnings.len());
        for i in &issues {
            let tag = if i.level == Level::Error { "error" } else { "warn " };
            eprintln!("  [{}] {}  {}\n           {}", tag, i.rule, i.location, i.message);
        }
    } else {
        eprintln!("• checks: all cross-references resolve, no warnings");
    }

    if !errors.is_empty() {
        eprintln!("\n✗ validation failed — fix the errors above before generating.");
        std::process::exit(1);
    }

    if check {
        eprintln!("\n✓ validation passed (--check: no files written).");
        return;
    }

    // Generation (ported incrementally). Emitters not yet ported are still produced by the TypeScript
    // codegen; the Rust tool must only (re)write artifacts it emits byte-identically, so the CI
    // generate+diff gate stays clean. Ported so far: translations.generated.json.
    let out_dir = arg_value(&args, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| specs.join("generated"));
    if let Err(e) = fs::create_dir_all(&out_dir) {
        eprintln!("✗ create {}: {}", out_dir.display(), e);
        std::process::exit(1);
    }
    let artifacts: [(&str, String); 8] = [
        ("translations.generated.json", emit_translations_json(&model)),
        ("views.generated.sql", emit_views_sql(&model)),
        ("schema.generated.sql", emit_schema_sql(&model, &specs)),
        ("c4.generated.dsl", emit_structurizr(&model)),
        ("c4.generated.md", emit_mermaid(&model)),
        ("schema.generated.graphql", emit_schema(&model)),
        ("documentation.generated.md", emit_documentation(&model)),
        (
            "documentation.generated.html",
            format!(
                "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>Captain.Food — Product Documentation</title>\n</head>\n<body>\n{}\n</body>\n</html>\n",
                emit_documentation_html(&model)
            ),
        ),
    ];
    for (name, content) in &artifacts {
        let path = out_dir.join(name);
        if let Err(e) = fs::write(&path, content) {
            eprintln!("✗ write {}: {}", path.display(), e);
            std::process::exit(1);
        }
        eprintln!("✓ wrote {}", path.display());
    }
    // database.md: inject the §2 read-model tables between the GENERATED:views markers (in-place).
    let db_md = specs.join("database.md");
    match inject_generated(&db_md, "views", &emit_views_markdown(&model)) {
        Ok(true) => eprintln!("✓ injected views into {}", db_md.display()),
        Ok(false) => eprintln!("! {}: no GENERATED:views markers — skipped", db_md.display()),
        Err(e) => {
            eprintln!("✗ {}", e);
            std::process::exit(1);
        }
    }
    // crates/domain/src/generated/{scalars,entities,events,commands}.rs: Rust domain types from
    // scalars.yaml + entities.yaml + events.yaml + commands.yaml (ADR-0034 #3 / 0035). mod.rs lists them.
    let gen_dir = repo_root(&specs).join("crates/domain/src/generated");
    if let Err(e) = fs::create_dir_all(&gen_dir) {
        eprintln!("✗ create {}: {}", gen_dir.display(), e);
        std::process::exit(1);
    }
    for (name, content) in [
        ("scalars.rs", emit_domain_scalars(&model)),
        ("entities.rs", emit_domain_entities(&model)),
        ("events.rs", emit_domain_events(&model)),
        ("commands.rs", emit_domain_commands(&model)),
        ("errors.rs", emit_domain_errors(&model)),
        ("mod.rs", "// GENERATED module index — do not edit by hand.\npub mod scalars;\npub mod entities;\npub mod events;\npub mod commands;\npub mod errors;\n".to_string()),
    ] {
        let path = gen_dir.join(name);
        if let Err(e) = fs::write(&path, content) {
            eprintln!("✗ write {}: {}", path.display(), e);
            std::process::exit(1);
        }
        eprintln!("✓ wrote {}", path.display());
    }
    // crates/application/src/generated/: read-model row types from projection_tables.yaml (ADR-0040).
    let app_gen = repo_root(&specs).join("crates/application/src/generated");
    if let Err(e) = fs::create_dir_all(&app_gen) {
        eprintln!("✗ create {}: {}", app_gen.display(), e);
        std::process::exit(1);
    }
    for (name, content) in [
        ("rows.rs", emit_projection_rows(&model)),
        ("projectors.rs", emit_projectors(&model)),
        ("mod.rs", "// GENERATED module index — do not edit by hand.\npub mod rows;\npub mod projectors;\n".to_string()),
    ] {
        let path = app_gen.join(name);
        if let Err(e) = fs::write(&path, content) {
            eprintln!("✗ write {}: {}", path.display(), e);
            std::process::exit(1);
        }
        eprintln!("✓ wrote {}", path.display());
    }
    // crates/server/src/graphql/generated/: the async-graphql type layer from api.yaml (Stage 1a) —
    // wrapper scalars/mirror enums, SimpleObject output types, InputObject inputs, and the QueryRoot.
    let srv_gen = repo_root(&specs).join("crates/server/src/graphql/generated");
    if let Err(e) = fs::create_dir_all(&srv_gen) {
        eprintln!("✗ create {}: {}", srv_gen.display(), e);
        std::process::exit(1);
    }
    for (name, content) in [
        ("scalars.rs", emit_server_scalars(&model)),
        ("types.rs", emit_server_types(&model)),
        ("inputs.rs", emit_server_inputs(&model)),
        ("acl.rs", emit_server_acl(&model)),
        ("query.rs", emit_server_query(&model)),
        ("mutation.rs", emit_server_mutation(&model)),
        ("subscription.rs", emit_server_subscription(&model)),
        ("mod.rs", "// GENERATED module index — do not edit by hand.\npub mod scalars;\npub mod types;\npub mod inputs;\npub mod acl;\npub mod query;\npub mod mutation;\npub mod subscription;\n".to_string()),
    ] {
        let path = srv_gen.join(name);
        if let Err(e) = fs::write(&path, content) {
            eprintln!("✗ write {}: {}", path.display(), e);
            std::process::exit(1);
        }
        eprintln!("✓ wrote {}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ref_splits_file_and_pointer() {
        let p = parse_ref("api.yaml#/queries/restaurants").expect("parses");
        assert_eq!(p.file, "api.yaml");
        assert_eq!(p.path, vec!["queries".to_string(), "restaurants".to_string()]);
    }

    #[test]
    fn parse_ref_keeps_dotted_translation_key_as_one_segment() {
        let p = parse_ref("translations.yaml#/home.craving").expect("parses");
        assert_eq!(p.file, "translations.yaml");
        assert_eq!(p.path, vec!["home.craving".to_string()]);
    }

    #[test]
    fn parse_ref_local_has_empty_file() {
        let p = parse_ref("#/fixtures/orderPlaced").expect("parses");
        assert_eq!(p.file, "");
        assert_eq!(p.path, vec!["fixtures".to_string(), "orderPlaced".to_string()]);
    }

    #[test]
    fn parse_ref_rejects_non_pointer() {
        assert!(parse_ref("api.yaml").is_none());
    }

    #[test]
    fn source_file_membership() {
        assert!(is_source_file("api.yaml"));
        assert!(is_source_file("architecture/c4-l2.yaml"));
        assert!(is_source_file("services.yaml"));
        assert!(!is_source_file("nope.yaml"));
    }

    #[test]
    fn svc_op_name_is_snake_case_domain_verb() {
        assert!(svc_op_name_ok("request"));
        assert!(svc_op_name_ok("offer_job"));
        assert!(svc_op_name_ok("verify_phone_otp"));
        assert!(!svc_op_name_ok("Request"));
        assert!(!svc_op_name_ok("offer-job"));
        assert!(!svc_op_name_ok("_request"));
        assert!(!svc_op_name_ok("1request"));
        assert!(!svc_op_name_ok(""));
    }

    #[test]
    fn svc_adapter_route_is_post_under_adapters() {
        assert!(svc_adapter_route_ok("POST /adapters/stripe/payment-intents"));
        assert!(svc_adapter_route_ok("POST /adapters/avelo37/deliveries"));
        assert!(!svc_adapter_route_ok("GET /adapters/stripe/refunds"));
        assert!(!svc_adapter_route_ok("POST /adapters/stripe")); // provider alone — needs ≥1 path segment
        assert!(!svc_adapter_route_ok("POST /services/payment/request")); // the DERIVED surface is never declared
        assert!(!svc_adapter_route_ok("POST /adapters/Stripe/refunds"));
        assert!(!svc_adapter_route_ok("POST /adapters/stripe/refunds/"));
    }
}
