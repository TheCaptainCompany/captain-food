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
    // Generic: every `specs/screens/*.yaml` is auto-discovered (ADR-20260722-091500 / -075500), so a
    // new SDUI audience is picked up by dropping in a file — no codegen edit. Two keyings, both sorted
    // for determinism:
    //   • SCREEN SPECS (`<surface>.yaml`, e.g. captain_frontoffice/restaurant_frontoffice) are keyed
    //     WITH the `screens/` prefix (`screens/<name>`), which §11 iterates as the per-app specs.
    //   • i18n SIDECARS (`<surface>.translations.yaml`, ADR-20260722-101500) are keyed BARE (no
    //     `screens/` prefix) so screens `$ref` them as `<surface>.translations.yaml#/<key>` and §11
    //     (which filters `screens/`-prefixed keys) does not mistake them for a screen spec.
    let sdir = specs.join("screens");
    if let Ok(rd) = fs::read_dir(&sdir) {
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
            if name.ends_with(".translations.yaml") {
                load(name, &p)?; // sidecar — keyed bare
            } else {
                load(format!("screens/{}", name), &p)?; // screen spec — keyed with `screens/` prefix
            }
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
    SOURCE_FILES.contains(&f)
        || (f.starts_with("database/tables/") && f.ends_with(".yaml"))
        // Auto-discovered SDUI screen specs (ADR-20260722-091500), keyed `screens/<surface>.yaml`.
        || (f.starts_with("screens/") && f.ends_with(".yaml"))
        // Per-surface i18n sidecars (ADR-20260722-101500), keyed BARE (`<surface>.translations.yaml`).
        || f.ends_with(".translations.yaml")
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
    lifecycles: usize,
    lifecycle_transitions: usize,
}

struct Report {
    issues: Vec<Issue>,
    coverage: Coverage,
    /// Commands actually handled by some actor (the cli's "commands" count; ≤ total command defs, the
    /// difference being command value objects referenced only from `properties`).
    handled_commands: usize,
}

const INLINE_TYPES: [&str; 4] = ["string", "boolean", "integer", "float"];

/// checkRoles: `roles:` is a LITERAL list (ADR-20260720-191500) — omitted means open to every role
/// path (→ @public), present means exactly those paths (→ @auth, PUBLIC = the anonymous path). Each
/// listed role must be a scalars.yaml#/UserType value.
fn check_roles(issues: &mut Vec<Issue>, roles: &[String], where_: &str, uts: &BTreeSet<String>) {
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
        // A $ref onto an ENUM scalar: the sample VALUE must be one of the declared values —
        // an invalid literal would otherwise only surface when the generated suite fails to
        // compile (issue #24 hardening).
        if let Some(target) = resolve_ref(model, rf, "tests.yaml") {
            if let (Some(vals), Some(sample)) = (
                target.get("enum").and_then(|e| e.as_sequence()),
                data.and_then(|d| d.as_str()),
            ) {
                if !vals.iter().any(|v| v.as_str() == Some(sample)) {
                    issues.push(err(
                        "test-invalid-enum-value",
                        where_.into(),
                        format!(
                            "'{}' is not a value of enum {} ({}).",
                            sample,
                            rf,
                            vals.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join("|")
                        ),
                    ));
                }
            }
        }
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

// ─── Ref-KIND contract (§1b) ────────────────────────────────────────────────────────────────────
// Resolving is not enough: a `$ref` must point at the right KIND of thing. `state_table` must be a
// process-manager state table — not merely "some table under database/tables/"; a screen resolver must
// be a query, not a mutation; an actor `emits` must be an event, not a command. §1b makes that a
// declared, exhaustive contract instead of the ad-hoc per-site checks scattered through §2–§11.
//
// It is FAIL-CLOSED: a `$ref` site not covered by REF_CONTRACT is an error, so a new ref-carrying field
// cannot be added to the DSL without declaring what it may point at.

/// What a `$ref` target IS — finer than the file it lives in (a table file holds several kinds).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Command,
    /// A `commands.yaml` definition NO actor receives: a shared payload sub-object (e.g. `CartLine`),
    /// not a business intention. Legal inside `properties`, never as an actor's message.
    PayloadObject,
    Event,
    /// A single property of a command/event/entity — `<file>#/<Def>/properties/<p>`.
    MessageProperty,
    Error,
    Rule,
    Scalar,
    /// A scalar with an `enum` member list (a state/status type).
    EnumScalar,
    Entity,
    /// An `actors.yaml` event-sourced aggregate.
    Aggregate,
    /// A `processmanager.yaml` state-table orchestrator.
    ProcessManager,
    Service,
    ServiceOperation,
    Query,
    Mutation,
    Subscription,
    ApiType,
    ApiInput,
    Test,
    /// A `tests.yaml#/fixtures/<f>` expected-outcome fixture.
    Fixture,
    /// A `translations.yaml` / `<surface>.translations.yaml` i18n key.
    TranslationKey,
    /// A generated fold VIEW over `domain_events` (`database/projection_views.yaml`).
    ProjectionView,
    /// A MATERIALIZED read-model table fed by an app projector (`tables/projection_tables.yaml`).
    ProjectionTable,
    /// A process manager's private state table (`tables/process_managers.yaml`).
    PmStateTable,
    /// A seed/config table configured by the repo seed script (`tables/referential.yaml`).
    ReferentialTable,
    /// A write-path journal — `command_journal` / `inbound_events` (`tables/journals.yaml`).
    JournalTable,
    /// Adapter-owned raw staging (`tables/integration_staging.yaml`).
    StagingTable,
    /// Integration connection storage (`tables/integration_connections.yaml`).
    ConnectionTable,
    /// `domain_events` / `domain_stream` (`tables/eventstore.yaml`).
    EventStoreTable,
    /// A column of any of the table kinds above.
    TableColumn,
    Screen,
    Persona,
    /// An `observability.yaml` workflow contract.
    ObservabilityWorkflow,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Command => "command",
            Kind::PayloadObject => "payload object",
            Kind::Event => "event",
            Kind::MessageProperty => "message property",
            Kind::Error => "error",
            Kind::Rule => "rule",
            Kind::Scalar => "scalar",
            Kind::EnumScalar => "enum scalar",
            Kind::Entity => "entity",
            Kind::Aggregate => "aggregate",
            Kind::ProcessManager => "process manager",
            Kind::Service => "service",
            Kind::ServiceOperation => "service operation",
            Kind::Query => "query",
            Kind::Mutation => "mutation",
            Kind::Subscription => "subscription",
            Kind::ApiType => "api output type",
            Kind::ApiInput => "api input type",
            Kind::Test => "behaviour test",
            Kind::Fixture => "test fixture",
            Kind::TranslationKey => "translation key",
            Kind::ProjectionView => "projection view",
            Kind::ProjectionTable => "projection table",
            Kind::PmStateTable => "process-manager state table",
            Kind::ReferentialTable => "referential table",
            Kind::JournalTable => "journal table",
            Kind::StagingTable => "staging table",
            Kind::ConnectionTable => "connection table",
            Kind::EventStoreTable => "event-store table",
            Kind::TableColumn => "table column",
            Kind::Screen => "screen",
            Kind::Persona => "persona",
            Kind::ObservabilityWorkflow => "observability workflow",
        }
    }
}

fn kind_list(kinds: &[Kind]) -> String {
    kinds.iter().map(|k| k.name()).collect::<Vec<_>>().join(" or ")
}

/// What KIND the target of a resolved `$ref` is: `(file, pointer segments, resolved node)` → `Kind`.
/// `None` = the pointer lands somewhere with no declared kind (e.g. mid-tree) — §1b reports it, which
/// keeps the classifier honest as the DSL grows.
fn classify(file: &str, path: &[String], node: &Value, handled: &BTreeSet<String>) -> Option<Kind> {
    let seg = |i: usize| path.get(i).map(|s| s.as_str());
    let top = path.len() == 1;
    // A table column: `<table>/columns/<col>` in any database/tables/*.yaml file.
    let table_column = path.len() == 3 && seg(1) == Some("columns");
    let table_kind = |k: Kind| -> Option<Kind> {
        if top {
            Some(k)
        } else if table_column {
            Some(Kind::TableColumn)
        } else {
            None
        }
    };
    match file {
        "commands.yaml" | "events.yaml" | "entities.yaml" => {
            let base = match file {
                // A commands.yaml entry is a COMMAND when an actor receives it; otherwise it is a
                // shared payload sub-object (mirrors §3's value-object derivation). A genuinely
                // unhandled command is reported by §3's `command-unhandled`.
                "commands.yaml" => match path.first() {
                    Some(n) if handled.contains(n.as_str()) => Kind::Command,
                    _ => Kind::PayloadObject,
                },
                "events.yaml" => Kind::Event,
                _ => Kind::Entity,
            };
            if top {
                Some(base)
            } else if path.len() == 3 && seg(1) == Some("properties") {
                Some(Kind::MessageProperty)
            } else {
                None
            }
        }
        "errors.yaml" => top.then_some(Kind::Error),
        "rules.yaml" => top.then_some(Kind::Rule),
        "scalars.yaml" => top.then(|| {
            if node.get("enum").is_some() { Kind::EnumScalar } else { Kind::Scalar }
        }),
        "actors.yaml" => top.then_some(Kind::Aggregate),
        "processmanager.yaml" => top.then_some(Kind::ProcessManager),
        "services.yaml" => {
            if top {
                Some(Kind::Service)
            } else if path.len() == 3 && seg(1) == Some("operations") {
                Some(Kind::ServiceOperation)
            } else {
                None
            }
        }
        "api.yaml" => match (seg(0), path.len()) {
            (Some("queries"), 2) => Some(Kind::Query),
            (Some("mutations"), 2) => Some(Kind::Mutation),
            (Some("subscriptions"), 2) => Some(Kind::Subscription),
            (Some("types"), 2) => Some(Kind::ApiType),
            (Some("inputs"), 2) => Some(Kind::ApiInput),
            _ => None,
        },
        "stories.yaml" => top.then_some(Kind::Persona),
        "tests.yaml" => match (seg(0), path.len()) {
            (Some("fixtures"), 2) => Some(Kind::Fixture),
            (Some("tests"), 2) => Some(Kind::Test),
            _ => None,
        },
        "observability.yaml" => top.then_some(Kind::ObservabilityWorkflow),
        "database/projection_views.yaml" => {
            if top {
                Some(Kind::ProjectionView)
            } else if table_column {
                Some(Kind::TableColumn)
            } else {
                None
            }
        }
        "database/tables/projection_tables.yaml" => table_kind(Kind::ProjectionTable),
        "database/tables/process_managers.yaml" => table_kind(Kind::PmStateTable),
        "database/tables/referential.yaml" => table_kind(Kind::ReferentialTable),
        "database/tables/journals.yaml" => table_kind(Kind::JournalTable),
        "database/tables/integration_staging.yaml" => table_kind(Kind::StagingTable),
        "database/tables/integration_connections.yaml" => table_kind(Kind::ConnectionTable),
        "database/tables/eventstore.yaml" => table_kind(Kind::EventStoreTable),
        f if f.ends_with(".translations.yaml") || f == "translations.yaml" => {
            top.then_some(Kind::TranslationKey)
        }
        f if f.starts_with("screens/") => match (seg(0), path.len()) {
            (Some("screens"), 2) => Some(Kind::Screen),
            _ => None,
        },
        _ => None,
    }
}

/// Glob over a `$ref` LOCATION: `*` matches any run of characters except `.` (so it stands for one
/// definition name / list index / map key), `**` matches anything including `.`.
fn glob_match(pat: &[u8], s: &[u8]) -> bool {
    if pat.starts_with(b"**") {
        let rest = &pat[2..];
        if rest.is_empty() {
            return true;
        }
        return (0..=s.len()).any(|i| glob_match(rest, &s[i..]));
    }
    match (pat.first(), s.first()) {
        (None, None) => true,
        (None, _) => false,
        (Some(b'*'), _) => {
            let rest = &pat[1..];
            let mut i = 0usize;
            loop {
                if glob_match(rest, &s[i..]) {
                    return true;
                }
                if i >= s.len() || s[i] == b'.' {
                    return false;
                }
                i += 1;
            }
        }
        (Some(pc), Some(sc)) if pc == sc => glob_match(&pat[1..], &s[1..]),
        _ => false,
    }
}

fn glob(pat: &str, s: &str) -> bool {
    glob_match(pat.as_bytes(), s.as_bytes())
}

/// The contract: `(source-file glob, ref-site location glob, allowed target kinds)`.
/// The location is the `$ref`'s path INSIDE its file (the leading `<file>.` is stripped), with list
/// indices as `[n]`. Order matters only for readability — every entry is tried, and a site with no
/// entry is an error (`ref-site-undeclared`).
#[rustfmt::skip]
const REF_CONTRACT: &[(&str, &str, &[Kind])] = &[
    // Payload shapes: a property/context/arg is a scalar, a value object, or (in api.yaml) a declared type.
    ("commands.yaml",  "*.properties.**",  &[Kind::Scalar, Kind::EnumScalar, Kind::Entity, Kind::PayloadObject]),
    ("events.yaml",    "*.properties.**",  &[Kind::Scalar, Kind::EnumScalar, Kind::Entity, Kind::PayloadObject]),
    ("entities.yaml",  "*.properties.**",  &[Kind::Scalar, Kind::EnumScalar, Kind::Entity]),
    ("errors.yaml",    "*.context.**",     &[Kind::Scalar, Kind::EnumScalar, Kind::Entity]),

    // Actors (aggregates): the inbox and the lifecycle state machine.
    ("actors.yaml", "*.receives[*].message",            &[Kind::Command, Kind::Event]),
    ("actors.yaml", "*.receives[*].emits[*]",           &[Kind::Event]),
    ("actors.yaml", "*.receives[*].throws[*]",          &[Kind::Error]),
    ("actors.yaml", "*.lifecycle.status",               &[Kind::EnumScalar]),
    ("actors.yaml", "*.lifecycle.initial[*].event",     &[Kind::Event]),
    ("actors.yaml", "*.lifecycle.transitions[*].event", &[Kind::Event]),

    // Process managers: state-table orchestrators (ADR-20260719-…). The state table is a PM state
    // table — not any table; reads hit read models; deliver/send target aggregates.
    ("processmanager.yaml", "*.state_table",                            &[Kind::PmStateTable]),
    ("processmanager.yaml", "*.ports.*",                                &[Kind::Service]),
    ("processmanager.yaml", "*.receives[*].message",                    &[Kind::Command, Kind::Event]),
    ("processmanager.yaml", "*.receives[*].steps[*].read.model",        &[Kind::ProjectionTable, Kind::ProjectionView]),
    ("processmanager.yaml", "*.receives[*].steps[*].read.where.*.from", &[Kind::MessageProperty]),
    ("processmanager.yaml", "*.receives[*].steps[*].guard.throws",      &[Kind::Error]),
    ("processmanager.yaml", "*.receives[*].steps[*].deliver.event",     &[Kind::Event]),
    ("processmanager.yaml", "*.receives[*].steps[*].deliver.to",        &[Kind::Aggregate]),
    ("processmanager.yaml", "*.receives[*].steps[*].deliver.with.*.from", &[Kind::MessageProperty]),
    ("processmanager.yaml", "*.receives[*].steps[*].send.command",      &[Kind::Command]),
    ("processmanager.yaml", "*.receives[*].steps[*].send.to",           &[Kind::Aggregate]),
    ("processmanager.yaml", "*.receives[*].steps[*].send.with.*.from",  &[Kind::MessageProperty]),
    ("processmanager.yaml", "*.receives[*].steps[*].state.by.*.from",   &[Kind::MessageProperty]),
    ("processmanager.yaml", "*.receives[*].steps[*].state.expect.*.from", &[Kind::MessageProperty]),
    ("processmanager.yaml", "*.receives[*].steps[*].state.set.*.from",  &[Kind::MessageProperty]),

    // Service catalog (outbound ports). An input may be a domain EVENT: an outbound call sometimes
    // hands the adapter the FACT verbatim (`delivery.offer_job` takes the DeliveryRequested birth
    // fact that carries pickup/dropoff) rather than a parallel entity that would drift from it.
    ("services.yaml", "*.operations.*.input.*",  &[Kind::Scalar, Kind::EnumScalar, Kind::Entity, Kind::Event]),
    ("services.yaml", "*.operations.*.output.*", &[Kind::Scalar, Kind::EnumScalar, Kind::Entity]),
    ("services.yaml", "*.operations.*.errors[*]", &[Kind::Error]),

    // GraphQL surface. A mutation dispatches a COMMAND; a type binds to a READ MODEL (never to
    // domain_events, never to a journal/staging table).
    ("api.yaml", "types.*.properties.**",   &[Kind::Scalar, Kind::EnumScalar, Kind::Entity, Kind::ApiType]),
    ("api.yaml", "types.*.reads[*]",        &[Kind::ProjectionView, Kind::ProjectionTable, Kind::ReferentialTable]),
    ("api.yaml", "inputs.*.properties.**",  &[Kind::Scalar, Kind::EnumScalar, Kind::Entity, Kind::ApiInput]),
    ("api.yaml", "queries.*.args.*",        &[Kind::Scalar, Kind::EnumScalar, Kind::ApiInput]),
    ("api.yaml", "queries.*.returns",       &[Kind::ApiType]),
    ("api.yaml", "mutations.*.command",     &[Kind::Command]),
    ("api.yaml", "mutations.*.args.*",      &[Kind::Scalar, Kind::EnumScalar, Kind::ApiInput]),
    ("api.yaml", "mutations.*.returns",     &[Kind::ApiType]),
    ("api.yaml", "subscriptions.*.args.*",  &[Kind::Scalar, Kind::EnumScalar, Kind::ApiInput]),
    ("api.yaml", "subscriptions.*.returns", &[Kind::ApiType]),

    // Story map: every step is an API operation the persona performs.
    ("stories.yaml", "*.activities.*.steps.*", &[Kind::Query, Kind::Mutation, Kind::Subscription]),

    // Behaviour tests (ADR-0032).
    ("tests.yaml", "fixtures.*.type",   &[Kind::Event]),
    ("tests.yaml", "tests.*.rules[*]",  &[Kind::Rule]),
    ("tests.yaml", "tests.*.actor",     &[Kind::Aggregate, Kind::ProcessManager]),
    ("tests.yaml", "tests.*.when.type", &[Kind::Command, Kind::Event]),
    ("tests.yaml", "tests.*.given[*]",  &[Kind::Fixture]),
    ("tests.yaml", "tests.*.then[*]",   &[Kind::Fixture]),
    ("tests.yaml", "tests.*.thrown[*]", &[Kind::Error]),

    // Observability contracts bind to the domain they diagnose.
    ("observability.yaml", "*.workflow.saga",           &[Kind::ProcessManager]),
    ("observability.yaml", "*.workflow.aggregate",      &[Kind::Aggregate]),
    ("observability.yaml", "*.workflow.command",        &[Kind::Command]),
    ("observability.yaml", "*.workflow.emits[*]",       &[Kind::Event]),
    ("observability.yaml", "*.workflow.inbound[*]",     &[Kind::Event]),
    ("observability.yaml", "*.run_identity[*].businessKey", &[Kind::Scalar, Kind::EnumScalar]),

    // Read models. `from` is event LINEAGE (a whole event for occurrence columns, a property
    // otherwise); `fk` is the read-navigation graph, so it must name a COLUMN.
    ("database/projection_views.yaml", "nonProjectedEvents[*]", &[Kind::Event]),
    ("database/projection_views.yaml", "*.tombstone",           &[Kind::Event]),
    ("database/projection_views.yaml", "*.fedBy[*]",            &[Kind::Event]),
    ("database/projection_views.yaml", "*.columns.*.type",      &[Kind::Scalar, Kind::EnumScalar]),
    ("database/projection_views.yaml", "*.columns.*.from[*]",   &[Kind::Event, Kind::MessageProperty]),
    ("database/projection_views.yaml", "*.columns.*.fk",        &[Kind::TableColumn]),

    // Real tables (globbed): every column types to a domain scalar; FKs name a column.
    ("database/tables/*.yaml", "*.tombstone",         &[Kind::Event]),
    ("database/tables/*.yaml", "*.fedBy[*]",          &[Kind::Event]),
    ("database/tables/*.yaml", "*.columns.*.type",    &[Kind::Scalar, Kind::EnumScalar]),
    ("database/tables/*.yaml", "*.columns.*.from[*]", &[Kind::Event, Kind::MessageProperty]),
    ("database/tables/*.yaml", "*.columns.*.fk",      &[Kind::TableColumn]),

    // SDUI screens (ADR-0033/0037): reads are queries, writes are mutations, live updates are
    // subscriptions — and EVERY other ref in the (free-form, deeply nested) UI tree is an i18n key,
    // which is what `screen-ref-out-of-scope` already asserts. Order matters: first match wins.
    ("screens/*.yaml", "resolvers.**",     &[Kind::Query]),
    ("screens/*.yaml", "actions.**",       &[Kind::Mutation]),
    ("screens/*.yaml", "**.subscription",  &[Kind::Subscription]),
    ("screens/*.yaml", "**",               &[Kind::TranslationKey]),

    // C4 model (source DSL, not generated): containers/components bind to the actors they realize.
    ("architecture/c4-l2.yaml", "boundedContexts.*.aggregates[*]",      &[Kind::Aggregate]),
    ("architecture/c4-l2.yaml", "containers.*.realizes[*]",             &[Kind::Aggregate, Kind::ProcessManager]),
    ("architecture/c4-l2.yaml", "boundedContexts.*.processManagers[*]", &[Kind::ProcessManager]),
    ("architecture/c4-l3.yaml", "components.*.handles[*]", &[Kind::Aggregate, Kind::ProcessManager]),
    ("architecture/c4-l3.yaml", "components.*.updates[*]", &[Kind::ProjectionView, Kind::ProjectionTable]),
];

/// The DSL's own FIELD names — every other segment of a `$ref` location is a definition/instance name
/// (a command, a screen, a column, a persona…). Used only to turn an undeclared site into a suggested
/// contract pattern: field names stay literal, name positions become `*`.
const STRUCTURAL_SEGMENTS: &[&str] = &[
    "actions", "activities", "actor", "args", "by", "call", "columns", "command", "content", "context",
    "deliver", "emits", "expect", "fixtures", "from", "from_hook", "given", "guard", "inputs",
    "lifecycle", "message", "messages", "model", "mutations", "operations", "params", "ports",
    "properties", "queries", "read", "reads", "receives", "resolvers", "returns", "rules", "screens",
    "send", "set", "state", "state_table", "status", "steps", "subscriptions", "tests", "then",
    "throws", "thrown", "to", "transitions", "type", "types", "when", "where", "with", "workflows",
];

/// Turn a concrete `$ref` site into the contract pattern that would cover it: list indices → `[*]`,
/// definition/instance names → `*`, DSL field names kept literal.
fn normalize_site(site: &str) -> String {
    site.split('.')
        .map(|part| {
            // Split a segment into its name and any trailing `[index]` suffixes.
            let (name, idx) = match part.find('[') {
                Some(i) => (&part[..i], &part[i..]),
                None => (part, ""),
            };
            let name = if STRUCTURAL_SEGMENTS.contains(&name) { name } else { "*" };
            let mut idx_out = String::new();
            let mut depth = 0;
            for ch in idx.chars() {
                match ch {
                    '[' => {
                        depth += 1;
                        idx_out.push_str("[*");
                    }
                    ']' => {
                        depth -= 1;
                        idx_out.push(']');
                    }
                    _ if depth > 0 => {}
                    c => idx_out.push(c),
                }
            }
            format!("{}{}", name, idx_out)
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// §1b — every `$ref` site must be declared, and its target must be of an allowed kind.
fn validate_ref_kinds(model: &Model, issues: &mut Vec<Issue>) {
    // Which commands.yaml entries are real COMMANDS (received by an actor or a process manager) —
    // the rest are shared payload sub-objects. See `Kind::PayloadObject`.
    let mut handled: BTreeSet<String> = BTreeSet::new();
    for f in ["actors.yaml", "processmanager.yaml"] {
        let mut refs = Vec::new();
        if let Some(v) = model.defs.get(f) {
            collect_refs(v, f, &mut refs);
        }
        for (loc, r) in refs {
            let site = loc.strip_prefix(f).and_then(|s| s.strip_prefix('.')).unwrap_or(&loc);
            if glob("*.receives[*].message", site) && ref_target_file(&r, f).as_deref() == Some("commands.yaml") {
                if let Some(n) = ref_name(&r) {
                    handled.insert(n);
                }
            }
        }
    }
    // Undeclared sites are reported once per NORMALIZED pattern (definition name and list indices
    // wildcarded), so the message doubles as the contract line to add.
    let mut undeclared: BTreeMap<String, (String, String, usize)> = BTreeMap::new();
    for (f, v) in &model.defs {
        let file = f.as_str();
        let mut refs = Vec::new();
        collect_refs(v, file, &mut refs);
        for (loc, r) in refs {
            let site = loc.strip_prefix(file).and_then(|s| s.strip_prefix('.')).unwrap_or(&loc);
            let allowed: Option<&[Kind]> = REF_CONTRACT
                .iter()
                .find(|(fg, lg, _)| glob(fg, file) && glob(lg, site))
                .map(|(_, _, k)| *k);
            let allowed = match allowed {
                Some(k) => k,
                None => {
                    let e = undeclared
                        .entry(format!("{}|{}", file, normalize_site(site)))
                        .or_insert((loc.clone(), site.to_string(), 0));
                    e.2 += 1;
                    continue;
                }
            };
            // Kind check (dangling/malformed refs are §1's job — skip what does not resolve).
            let pr = match parse_ref(&r) {
                Some(p) => p,
                None => continue,
            };
            let target_file = if pr.file.is_empty() { file.to_string() } else { pr.file.clone() };
            let node = match resolve_ref(model, &r, file) {
                Some(n) => n,
                None => continue,
            };
            match classify(&target_file, &pr.path, node, &handled) {
                Some(k) if allowed.contains(&k) => {}
                Some(k) => {
                    // A commands.yaml entry only counts as a COMMAND once an actor receives it —
                    // spell that out rather than leaving "is a payload object" to be decoded.
                    let hint = if k == Kind::PayloadObject && allowed.contains(&Kind::Command) {
                        " (no actor or process manager receives it — wire it into an inbox, or move it to entities.yaml if it is a payload shape)"
                    } else {
                        ""
                    };
                    issues.push(err(
                        "ref-kind",
                        loc.clone(),
                        format!("$ref '{}' is a {}; this site requires a {}{}.", r, k.name(), kind_list(allowed), hint),
                    ))
                }
                None => issues.push(err(
                    "ref-kind-unknown",
                    loc.clone(),
                    format!("$ref '{}' does not name a classifiable definition (expected a {}).", r, kind_list(allowed)),
                )),
            }
        }
    }
    for (key, (example, example_site, count)) in undeclared {
        let (file, norm) = key.split_once('|').unwrap_or(("?", "?"));
        issues.push(err(
            "ref-site-undeclared",
            example,
            format!(
                "no ref-kind contract for the $ref site '{}' ({} occurrence(s)) — declare what it may point at, e.g. (\"{}\", \"{}\", &[…]) in REF_CONTRACT.",
                example_site, count, file, norm
            ),
        ));
    }
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

    // --- 1b. Ref-KIND contract: a resolving $ref must also point at the right KIND of thing -------
    validate_ref_kinds(model, &mut issues);

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

    // --- 2c. Aggregate lifecycle state machines (actors.yaml `lifecycle`, ADR-20260720-004419) ---
    validate_lifecycles(model, &mut issues);
    {
        let lcs = parse_lifecycles(model);
        cov.lifecycles = lcs.len();
        cov.lifecycle_transitions =
            lcs.iter().map(|l| l.transitions.iter().map(|t| t.from.len()).sum::<usize>()).sum();
    }

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
        // Acceptance-first (ADR-20260720-015500): a mutation declares NO per-operation payload —
        // business outcomes are reads. The uniform MutationAcceptance is the only mutation payload.
        if !m.payload.is_empty() {
            issues.push(err(
                "mutation-payload-forbidden",
                where_.clone(),
                format!(
                    "mutation '{}' declares a payload — acceptance-first mutations return only \
                     MutationAcceptance; expose business results as a query/subscription (ADR-20260720-015500).",
                    m.name
                ),
            ));
        }
    }
    cov.mutation_links = declared_by_command.len();
    // 4a'. the acceptance-first surface both emitters depend on must exist in the spec.
    if !api.types.iter().any(|t| t.name == "MutationAcceptance") {
        issues.push(err(
            "acceptance-type-missing",
            "api.yaml/types".into(),
            "acceptance-first mutations require the shared #/types/MutationAcceptance (ADR-20260720-015500).".into(),
        ));
    }
    if !api.inputs.iter().any(|(n, _)| n == "MetadataInput") {
        issues.push(err(
            "metadata-input-missing",
            "api.yaml/inputs".into(),
            "acceptance-first mutations require #/inputs/MetadataInput (ADR-20260720-015500).".into(),
        ));
    } else if let Some((_, fields)) = api.inputs.iter().find(|(n, _)| n == "MetadataInput") {
        for f in fields {
            check_inline(&mut issues, f, &format!("api.yaml/inputs.MetadataInput.{}", f.name));
        }
    }
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
        // navRoles (#22, ADR-20260720-230000): each key must be a DERIVED navigation edge on that
        // type, each list a LITERAL roles list (ADR-20260720-191500 semantics).
        {
            let registered: HashSet<String> = api.types.iter().map(|t| t.name.clone()).collect();
            let nav = nav_fields(&views, &registered);
            for t in &api.types {
                for (field, roles) in &t.nav_roles {
                    let known = nav
                        .get(&t.name)
                        .map_or(false, |nfs| nfs.iter().any(|n| &n.field == field));
                    if !known {
                        issues.push(err(
                            "nav-roles-unknown-field",
                            format!("api.yaml/types.{}", t.name),
                            format!("navRoles key '{}' is not a derived navigation field on '{}'.", field, t.name),
                        ));
                    }
                    check_roles(
                        &mut issues,
                        roles,
                        &format!("api.yaml/types.{}.navRoles.{}", t.name, field),
                        &user_type_set,
                    );
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
                    // Literal roles (ADR-20260720-191500): omitted = every persona may call it;
                    // present = the persona's path-role must be listed (PUBLIC = the anonymous path).
                    let allowed = roles.is_empty() || (!p.role.is_empty() && roles.iter().any(|r| r == &p.role));
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
        // Dispatch surfaces a contract may bind INSTEAD of a single command/saga/aggregate
        // (ADR-20260721-031127: pipeline contracts, e.g. command-acceptance over the GraphQL dispatch).
        const SURFACE_KINDS: [&str; 1] = ["graphql"];
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
                let surface = wf.and_then(|w| w.get("surface")).and_then(|v| v.as_str());
                if surface.is_none() && !has("command") && !has("saga") && !has("aggregate") {
                    issues.push(err(
                        "obs-no-workflow-binding",
                        at.clone(),
                        "workflow must bind a `command` and/or `saga`/`aggregate` ($ref into the model), or a dispatch `surface`.".into(),
                    ));
                }
                if let Some(s) = surface {
                    if !SURFACE_KINDS.contains(&s) {
                        issues.push(err(
                            "obs-surface-unknown",
                            format!("{}.workflow.surface", at),
                            format!("surface '{}' is not a known dispatch surface ({}).", s, SURFACE_KINDS.join("|")),
                        ));
                    }
                    if has("command") || has("saga") || has("aggregate") {
                        issues.push(err(
                            "obs-surface-exclusive",
                            format!("{}.workflow", at),
                            "a `surface` contract binds the whole dispatch surface — it must not also bind a `command`/`saga`/`aggregate`.".into(),
                        ));
                    }
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

    // --- 10. Translations (translations.yaml + screens/*.translations.yaml sidecars) ------------
    // Merged across all sources (ADR-20260722-101500); keys must be globally unique across files.
    {
        let mut seen: BTreeMap<String, String> = BTreeMap::new(); // key -> first file it was defined in
        for (file, key, t) in translation_entries(model) {
            let at = format!("{}/{}", file, key);
            if let Some(prev) = seen.insert(key.clone(), file.clone()) {
                issues.push(err(
                    "translation-duplicate-key",
                    at.clone(),
                    format!("translation key '{}' is defined in both '{}' and '{}' — keys must be unique across all translation files.", key, prev, file),
                ));
            }
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
    // API (ADR-0033/0037). Generic over all screens files — no hard-coded screens filename. Each screen
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
            // --- Translation-ref scope (ADR-20260722-101500): the API refs live in `resolvers`/`actions`
            // (validated above); EVERY OTHER `$ref` in a screen is a content/text slot and MUST be a
            // translation ref that resolves to a real entry (a key carrying `messages`). This catches
            // dangling/renamed keys AND text slots pointing at the wrong file/scope (e.g. an api.yaml or
            // scalar ref where a string is expected).
            if let Some(map) = cs.and_then(|v| v.as_mapping()) {
                let mut refs: Vec<(String, String)> = Vec::new();
                for (k, v) in map {
                    match k.as_str() {
                        Some("resolvers") | Some("actions") => {} // API bindings — validated above.
                        Some(key) => collect_refs(v, &format!("{}.{}", sfkey, key), &mut refs),
                        None => {}
                    }
                }
                for (loc, rf) in &refs {
                    // A screen-level realtime binding (`subscription: { $ref: api.yaml#/subscriptions/… }`)
                    // is an API ref, not content — skip it (validated as an operation elsewhere).
                    if loc.ends_with(".subscription") {
                        continue;
                    }
                    match ref_target_file(rf, sfkey).as_deref() {
                        Some(f) if f == "translations.yaml" || f.ends_with(".translations.yaml") => {
                            if resolve_ref(model, rf, sfkey).and_then(|n| n.get("messages")).is_none() {
                                issues.push(err(
                                    "screen-translation-ref-unresolved",
                                    loc.clone(),
                                    format!("translation $ref '{}' does not resolve to a translation entry (a key with `messages`).", rf),
                                ));
                            }
                        }
                        other => issues.push(err(
                            "screen-ref-out-of-scope",
                            loc.clone(),
                            format!("content $ref '{}' in a screen must be a translations key; it targets '{}'.", rf, other.unwrap_or("<local/unknown>")),
                        )),
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

/// All translation ENTRIES merged from every source (ADR-20260722-101500): the shared `translations.yaml`
/// plus every per-surface `screens/*.translations.yaml` sidecar. Returns `(fileKey, entryKey, node)` per
/// real entry (skips file-level meta — only nodes carrying `messages`), file-sorted then file-order, so
/// output is deterministic. Keys must be unique across files (the §10 validator enforces it).
fn translation_entries(model: &Model) -> Vec<(String, String, &Value)> {
    let mut files: Vec<&String> = model
        .defs
        .keys()
        .filter(|k| k.as_str() == "translations.yaml" || k.ends_with(".translations.yaml"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        if let Some(Value::Mapping(m)) = model.defs.get(f) {
            for (k, v) in m {
                if let Some(key) = k.as_str() {
                    if v.get("messages").is_some() {
                        out.push((f.clone(), key.to_string(), v));
                    }
                }
            }
        }
    }
    out
}

/// Emit the single i18n bundle from translations.yaml (ADR-0033) — the first ported emitter. Must be
/// BYTE-IDENTICAL to the TypeScript `emitTranslationsJson` output (keys sorted; `{ "<key>": { en, fr } }`;
/// 2-space pretty JSON + trailing newline) so the CI generate+diff gate stays clean during the migration.
fn emit_translations_json(model: &Model) -> String {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    // Merge translations.yaml + every screens/*.translations.yaml sidecar (keys are globally unique and
    // BTreeMap-sorted, so the flat catalog stays byte-identical regardless of which file a key lives in).
    for (_file, key, v) in translation_entries(model) {
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
        out.insert(key, locales);
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

/// The Money-value-object subfield a `*_cents`/currency column extracts (the projection convention:
/// `Money = { amountCents, currency }` becomes a `MoneyCents` column + a `CurrencyCode` column).
/// `Some` only when the column's `from` property is `$ref: entities.yaml#/Money` AND the declared
/// column type picks a subfield — `MoneyCents` → `amountCents`, `CurrencyCode` → `currency`.
fn money_subfield(model: &Model, evt: &str, prop: &str, col_ty: &str) -> Option<&'static str> {
    let sub = match col_ty {
        "MoneyCents" => "amountCents",
        "CurrencyCode" => "currency",
        _ => return None,
    };
    let r = model
        .defs
        .get("events.yaml")?
        .get(evt)?
        .get("properties")?
        .get(prop)?
        .get("$ref")?
        .as_str()?;
    if r == "entities.yaml#/Money" {
        Some(sub)
    } else {
        None
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
            } else if let Some((first_evt, prop)) = carrying.first() {
                // scalar "latest carrying event": the newest event whose payload holds this property.
                // An enum column stores the ordinal (value→ordinal CASE); a Money property splits into
                // its `amountCents`/`currency` subfield by declared column type; others extract+cast.
                let money_sub = money_subfield(model, first_evt, prop, &c.ty);
                let val_expr = |alias: &str| {
                    if let Some(sub) = money_sub {
                        let cast = pg_cast(&pgty);
                        if cast.is_empty() {
                            format!("{}.payload->'{}'->>'{}'", alias, prop, sub)
                        } else {
                            format!("({}.payload->'{}'->>'{}'){}", alias, prop, sub, cast)
                        }
                    } else {
                        match &enum_vals {
                            Some(vals) => enum_ordinal_case(&format!("{}.payload->>'{}'", alias, prop), vals),
                            None => payload_extract(alias, prop, &pgty),
                        }
                    }
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
    let forms = ["const", "from", "from_state", "from_read", "from_port", "from_envelope", "from_hook"];
    let present: Vec<&str> = forms.iter().copied().filter(|f| v.get(*f).is_some()).collect();
    if present.len() != 1 {
        issues.push(err(
            "pm-value",
            where_.to_string(),
            "a step value must be exactly one of { const | from | from_state | from_read | from_port | from_envelope | from_hook }.".into(),
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
        "from_hook" => {
            // A runtime orchestrator hook (#60): the value is resolved in code, so the name is the
            // only spec-level constraint — a non-empty snake_case identifier the emitter turns into
            // an async hook method. (The hook is only consumed inside `state.set`.)
            let n = v.get("from_hook").and_then(|x| x.as_str()).unwrap_or("");
            if n.is_empty() || !n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
                issues.push(err(
                    "pm-value",
                    where_.to_string(),
                    format!("`from_hook` '{}' must be a non-empty snake_case hook name.", n),
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

// ─── §2c — aggregate lifecycle state machines (actors.yaml `lifecycle`, ADR-20260720-004419) ────

/// One `{ from: [states], event, to[, via] }` transition of a declared lifecycle. `via` names the
/// event payload field carrying the target state (dynamic target, ADR-20260721-093027): the entry
/// legalizes `from × {to}` when `event.<via> == to`.
struct LifecycleTransition {
    from: Vec<String>,
    event_ref: String,
    to: String,
    via: Option<String>,
}

/// One `{ event, to[, via] }` birth entry of a declared lifecycle. With `via`, the birth state is
/// event-carried (the fold births from the payload field); `to` stays the canonical birth state.
struct LifecycleInitial {
    event_ref: String,
    to: String,
    via: Option<String>,
}

/// A parsed `lifecycle:` block of an actors.yaml aggregate: the status machine as declared data.
/// Tolerant parsing (missing pieces → empty); `validate_lifecycles` reports the holes.
struct Lifecycle {
    aggregate: String,
    status_ref: String,
    initial: Vec<LifecycleInitial>,
    transitions: Vec<LifecycleTransition>,
    terminal: Vec<String>,
}

/// Parse every aggregate's `lifecycle:` block, in actors.yaml order.
fn parse_lifecycles(model: &Model) -> Vec<Lifecycle> {
    let mut out = Vec::new();
    let actors = match model.defs.get("actors.yaml") {
        Some(Value::Mapping(m)) => m,
        _ => return out,
    };
    for (k, node) in actors {
        let name = match k.as_str() {
            Some(s) => s,
            None => continue,
        };
        if node.get("type").and_then(|x| x.as_str()) != Some("aggregate") {
            continue;
        }
        let lc = match node.get("lifecycle") {
            Some(v) => v,
            None => continue,
        };
        let str_seq = |v: Option<&Value>| -> Vec<String> {
            v.and_then(|x| x.as_sequence())
                .map(|s| s.iter().filter_map(|it| it.as_str().map(|x| x.to_string())).collect())
                .unwrap_or_default()
        };
        let event_ref =
            |e: &Value| e.get("event").and_then(|x| x.get("$ref")).and_then(|r| r.as_str()).unwrap_or("").to_string();
        let initial = lc
            .get("initial")
            .and_then(|x| x.as_sequence())
            .map(|s| {
                s.iter()
                    .map(|e| LifecycleInitial {
                        event_ref: event_ref(e),
                        to: e.get("to").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                        via: e.get("via").and_then(|x| x.as_str()).map(str::to_string),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let transitions = lc
            .get("transitions")
            .and_then(|x| x.as_sequence())
            .map(|s| {
                s.iter()
                    .map(|t| LifecycleTransition {
                        from: str_seq(t.get("from")),
                        event_ref: event_ref(t),
                        to: t.get("to").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                        via: t.get("via").and_then(|x| x.as_str()).map(str::to_string),
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(Lifecycle {
            aggregate: name.to_string(),
            status_ref: lc
                .get("status")
                .and_then(|x| x.get("$ref"))
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .to_string(),
            initial,
            transitions,
            terminal: str_seq(lc.get("terminal")),
        });
    }
    out
}

/// The enum values of a scalars.yaml enum scalar, or `None` when the name is not an enum scalar.
fn scalar_enum_values(model: &Model, scalar: &str) -> Option<Vec<String>> {
    model
        .defs
        .get("scalars.yaml")
        .and_then(|s| s.get(scalar))
        .and_then(|n| n.get("enum"))
        .and_then(|e| e.as_sequence())
        .map(|s| s.iter().filter_map(|v| v.as_str().map(|x| x.to_string())).collect())
}

/// §2c — validate the declared aggregate lifecycles (ADR-20260720-004419): the status is an enum
/// scalar; every named state is a member of it; every claimed event is emitted by THIS aggregate;
/// the machine is deterministic (no two transitions from one state on one event); terminal states
/// have no outgoing transition; every named state is reachable from an initial state. An aggregate
/// whose `<Name>Status` scalar exists (trailing `Job` stripped, so DeliveryJob ↔ DeliveryStatus)
/// but that declares no lifecycle WARNS (`lc-missing`) — adoption is incremental.
fn validate_lifecycles(model: &Model, issues: &mut Vec<Issue>) {
    let actors = match model.defs.get("actors.yaml") {
        Some(Value::Mapping(m)) => m,
        _ => return,
    };
    let lifecycles: BTreeSet<String> = parse_lifecycles(model).into_iter().map(|l| l.aggregate).collect();
    // Coverage: an aggregate with a status scalar but no declared lifecycle.
    for (k, node) in actors {
        let name = match k.as_str() {
            Some(s) => s,
            None => continue,
        };
        if node.get("type").and_then(|x| x.as_str()) != Some("aggregate") || lifecycles.contains(name) {
            continue;
        }
        let base = name.strip_suffix("Job").unwrap_or(name);
        for candidate in [format!("{}Status", name), format!("{}Status", base)] {
            if scalar_enum_values(model, &candidate).is_some() {
                issues.push(warn(
                    "lc-missing",
                    format!("actors.yaml/{}", name),
                    format!(
                        "aggregate '{}' has a status scalar (scalars.yaml#/{}) but declares no `lifecycle` — its status machine stays implicit code (ADR-20260720-004419).",
                        name, candidate
                    ),
                ));
                break;
            }
        }
    }
    for lc in parse_lifecycles(model) {
        let at = format!("actors.yaml/{}.lifecycle", lc.aggregate);
        // status → a scalars.yaml ENUM scalar.
        let enum_values: Vec<String> = match ref_name(&lc.status_ref) {
            Some(scalar)
                if ref_target_file(&lc.status_ref, "actors.yaml").as_deref() == Some("scalars.yaml") =>
            {
                match scalar_enum_values(model, &scalar) {
                    Some(vals) => vals,
                    None => {
                        issues.push(err(
                            "lc-status",
                            format!("{}.status", at),
                            format!("'{}' is not an enum scalar — the lifecycle status must enumerate its states.", scalar),
                        ));
                        continue;
                    }
                }
            }
            _ => {
                issues.push(err(
                    "lc-status",
                    format!("{}.status", at),
                    "status must be a { $ref: 'scalars.yaml#/<EnumScalar>' }.".into(),
                ));
                continue;
            }
        };
        let state_set: BTreeSet<&str> = enum_values.iter().map(|s| s.as_str()).collect();
        let check_state = |issues: &mut Vec<Issue>, state: &str, where_: String| {
            if !state_set.contains(state) {
                issues.push(err(
                    "lc-state",
                    where_,
                    format!("'{}' is not a member of {} ({}).", state, ref_name(&lc.status_ref).unwrap_or_default(), enum_values.join(", ")),
                ));
            }
        };
        // The events THIS aggregate emits, per its receives[].emits (actors.yaml stays the wiring truth).
        let emitted: BTreeSet<String> = actors
            .get(lc.aggregate.as_str())
            .and_then(|n| n.get("receives"))
            .and_then(|r| r.as_sequence())
            .map(|seq| {
                seq.iter()
                    .flat_map(|e| ref_strings(e.get("emits")))
                    .filter_map(|r| ref_name(&r))
                    .collect()
            })
            .unwrap_or_default();
        let check_event = |issues: &mut Vec<Issue>, event_ref: &str, where_: String| -> Option<String> {
            if ref_target_file(event_ref, "actors.yaml").as_deref() != Some("events.yaml") {
                issues.push(err(
                    "lc-event",
                    where_,
                    format!("event must be a {{ $ref: 'events.yaml#/<Event>' }}, got '{}'.", event_ref),
                ));
                return None;
            }
            let name = ref_name(event_ref)?; // resolution itself is §1's job (ref-dangling)
            if !emitted.contains(&name) {
                issues.push(err(
                    "lc-event-not-emitted",
                    where_,
                    format!("event '{}' is not emitted by aggregate '{}' (per its receives[].emits) — the machine may only claim its own facts.", name, lc.aggregate),
                ));
            }
            Some(name)
        };
        // via — a dynamic target (ADR-20260721-093027): the named field must exist on the event's
        // events.yaml payload, be REQUIRED (an optional target cannot drive a machine), and $ref the
        // same scalar as `lifecycle.status`.
        let status_scalar = ref_name(&lc.status_ref).unwrap_or_default();
        let check_via = |issues: &mut Vec<Issue>, event: &str, via: &str, where_: String| {
            let node = model.defs.get("events.yaml").and_then(|e| e.get(event));
            let prop = node.and_then(|n| n.get("properties")).and_then(|p| p.get(via));
            match prop {
                None => issues.push(err(
                    "lc-via",
                    where_,
                    format!("via field '{}' does not exist on events.yaml#/{}'s payload.", via, event),
                )),
                Some(p) => {
                    let target = p.get("$ref").and_then(|r| r.as_str()).unwrap_or("");
                    let same_scalar = ref_name(target).as_deref() == Some(status_scalar.as_str())
                        && ref_target_file(target, "events.yaml").as_deref() == Some("scalars.yaml");
                    if !same_scalar {
                        issues.push(err(
                            "lc-via",
                            where_.clone(),
                            format!("via field '{}' on events.yaml#/{} must $ref scalars.yaml#/{} (the lifecycle status scalar).", via, event, status_scalar),
                        ));
                    }
                    let required = node
                        .and_then(|n| n.get("required"))
                        .and_then(|r| r.as_sequence())
                        .map(|s| s.iter().any(|v| v.as_str() == Some(via)))
                        .unwrap_or(false);
                    if !required {
                        issues.push(err(
                            "lc-via",
                            where_,
                            format!("via field '{}' on events.yaml#/{} must be required — an optional target cannot drive the machine.", via, event),
                        ));
                    }
                }
            }
        };
        // An event must use ONE consistent `via` across all its lifecycle entries — mixing static
        // and dynamic arms (or two different fields) for the same event is ambiguous.
        let mut via_by_event: BTreeMap<String, BTreeSet<Option<String>>> = BTreeMap::new();
        for ini in &lc.initial {
            if let Some(name) = ref_name(&ini.event_ref) {
                via_by_event.entry(name).or_default().insert(ini.via.clone());
            }
        }
        for t in &lc.transitions {
            if let Some(name) = ref_name(&t.event_ref) {
                via_by_event.entry(name).or_default().insert(t.via.clone());
            }
        }
        for (event, vias) in &via_by_event {
            if vias.len() > 1 {
                issues.push(err(
                    "lc-ambiguous",
                    at.clone(),
                    format!("event '{}' mixes static and dynamic (`via`) entries (or two different via fields) — one event, one consistent target mode.", event),
                ));
            }
        }
        // initial — at least one birth entry; unique events; states in the enum.
        if lc.initial.is_empty() {
            issues.push(err("lc-shape", format!("{}.initial", at), "lifecycle must declare at least one `initial` { event, to } entry.".into()));
        }
        let mut initial_events: BTreeSet<String> = BTreeSet::new();
        for (i, ini) in lc.initial.iter().enumerate() {
            let w = format!("{}.initial[{}]", at, i);
            check_state(issues, &ini.to, w.clone());
            if let Some(name) = check_event(issues, &ini.event_ref, w.clone()) {
                if let Some(via) = &ini.via {
                    check_via(issues, &name, via, w.clone());
                }
                if !initial_events.insert(name.clone()) {
                    issues.push(err("lc-ambiguous", w, format!("duplicate initial event '{}' — the machine must be deterministic.", name)));
                }
            }
        }
        // transitions — states/events valid, deterministic: one arm per (from, event) for a static
        // target; per (from, event, to) for a dynamic one (the event INSTANCE picks the arm).
        let mut seen: BTreeSet<(String, String, String)> = BTreeSet::new();
        for (i, t) in lc.transitions.iter().enumerate() {
            let w = format!("{}.transitions[{}]", at, i);
            if t.from.is_empty() {
                issues.push(err("lc-shape", w.clone(), "a transition must declare a non-empty `from: [states]`.".into()));
            }
            check_state(issues, &t.to, w.clone());
            let ev = check_event(issues, &t.event_ref, w.clone());
            if let (Some(name), Some(via)) = (&ev, &t.via) {
                check_via(issues, name, via, w.clone());
            }
            for f in &t.from {
                check_state(issues, f, w.clone());
                if let Some(name) = &ev {
                    let key_to = if t.via.is_some() { t.to.clone() } else { String::new() };
                    if !seen.insert((f.clone(), name.clone(), key_to)) {
                        issues.push(err(
                            "lc-ambiguous",
                            w.clone(),
                            format!("two transitions from '{}' on '{}' — the machine must be deterministic.", f, name),
                        ));
                    }
                }
            }
        }
        // terminal — in the enum, and with NO outgoing transition.
        for (i, s) in lc.terminal.iter().enumerate() {
            let w = format!("{}.terminal[{}]", at, i);
            check_state(issues, s, w.clone());
            if lc.transitions.iter().any(|t| t.from.iter().any(|f| f == s)) {
                issues.push(err("lc-terminal-outgoing", w, format!("terminal state '{}' has an outgoing transition.", s)));
            }
        }
        // reachability — every state the lifecycle names is reachable from an initial state.
        let mut reachable: BTreeSet<String> = lc.initial.iter().map(|i| i.to.clone()).collect();
        loop {
            let before = reachable.len();
            for t in &lc.transitions {
                if t.from.iter().any(|f| reachable.contains(f)) {
                    reachable.insert(t.to.clone());
                }
            }
            if reachable.len() == before {
                break;
            }
        }
        let mut named: BTreeSet<String> = lc.terminal.iter().cloned().collect();
        for t in &lc.transitions {
            named.extend(t.from.iter().cloned());
            named.insert(t.to.clone());
        }
        for s in named {
            if state_set.contains(s.as_str()) && !reachable.contains(&s) {
                issues.push(err(
                    "lc-unreachable",
                    at.clone(),
                    format!("state '{}' is named by the lifecycle but not reachable from an initial state.", s),
                ));
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
    /// OPTIONAL per-type `navRoles:` — FK-derived navigation edge → LITERAL roles list (#22,
    /// ADR-20260720-230000). Omitted edge = open (inherits the parent type's reachability).
    nav_roles: Vec<(String, Vec<String>)>,
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
    /// api.yaml `inputs:` — generator-injected input types that are not command payloads
    /// (MetadataInput, ADR-20260720-015500). (name, fields) pairs, emission order = declaration.
    inputs: Vec<(String, Vec<ApiField>)>,
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

/// api.yaml `types.<T>.navRoles` — field name → literal roles list for FK-derived nav edges (#22).
fn nav_roles_map(v: Option<&Value>) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    if let Some(Value::Mapping(m)) = v {
        for (k, r) in m {
            if let (Some(field), Some(seq)) = (k.as_str(), r.as_sequence()) {
                out.push((
                    field.to_string(),
                    seq.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
                ));
            }
        }
    }
    out
}

fn parse_api(model: &Model) -> Api {
    let sect = |k: &str| model.defs.get("api.yaml").and_then(|v| v.get(k)).and_then(|v| v.as_mapping());
    let mut types = Vec::new();
    if let Some(m) = sect("types") {
        for (k, t) in m {
            if let Some(name) = k.as_str() {
                types.push(ApiType { name: name.into(), description: t.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()), reads: name_list(t.get("reads")), properties: field_map(t.get("properties")), nav_roles: nav_roles_map(t.get("navRoles")) });
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
    let mut inputs = Vec::new();
    if let Some(m) = sect("inputs") {
        for (k, def) in m {
            if let Some(n) = k.as_str() {
                inputs.push((n.to_string(), field_map(def.get("properties"))));
            }
        }
    }
    Api { types, queries, mutations, subscriptions, inputs }
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
    // Both ends must be registered API types: a navigation field TO an unregistered aggregate (e.g.
    // Payment, whose View_PendingRefunds fk only documents read lineage) would emit an SDL/Rust
    // reference to a type that does not exist.
    if !entity_names.contains(entity) || !entity_names.contains(&nf.target) {
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

fn nav_by_entity(
    views: &[SqlView],
    entity_names: &HashSet<String>,
    nav_roles: &HashMap<String, HashMap<String, Vec<String>>>,
) -> HashMap<String, Vec<String>> {
    nav_fields(views, entity_names)
        .into_iter()
        .map(|(entity, nfs)| {
            let lines = nfs
                .into_iter()
                .map(|n| {
                    // Guarded edge (#22): same @auth directive as operations; omitted = bare/open.
                    let auth = nav_roles
                        .get(&entity)
                        .and_then(|m| m.get(&n.field))
                        .map(|roles| format!(" {}", auth_directive(roles)))
                        .unwrap_or_default();
                    if n.list {
                        format!("  {}: [{}!]!{}", n.field, n.target, auth)
                    } else {
                        format!("  {}: {}{}{}", n.field, n.target, if n.nullable { "" } else { "!" }, auth)
                    }
                })
                .collect();
            (entity, lines)
        })
        .collect()
}

fn output_types_block(model: &Model, views: &[SqlView], api: &Api) -> String {
    let registered: HashSet<String> = api.types.iter().map(|t| t.name.clone()).collect();
    let nav_roles: HashMap<String, HashMap<String, Vec<String>>> = api
        .types
        .iter()
        .map(|t| (t.name.clone(), t.nav_roles.iter().cloned().collect()))
        .collect();
    let nav = nav_by_entity(views, &registered, &nav_roles);
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

    // Generator-injected inputs (api.yaml `inputs:` — MetadataInput): declared fields, all optional
    // unless marked required (the technical envelope is always client-optional).
    let mut declared_inputs = Vec::new();
    for (name, fields) in &api.inputs {
        let lines: Vec<String> = fields
            .iter()
            .map(|f| format!("  {}: {}", f.name, api_field_type(model, f, true)))
            .collect();
        declared_inputs.push(format!("input {} {{\n{}\n}}", name, lines.join("\n")));
    }

    let mut all = command_inputs;
    all.extend(query_inputs);
    all.extend(subscription_inputs);
    all.extend(object_inputs);
    all.extend(declared_inputs);
    all.join("\n\n")
}

fn auth_directive(roles: &[String]) -> String {
    // Literal roles (ADR-20260720-191500): omitted = open to every role path (@public); present =
    // exactly the listed paths (@auth) — PUBLIC inside `requires` is the anonymous path.
    if roles.is_empty() {
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
    // Acceptance-first (ADR-20260720-015500): every mutation takes the optional technical envelope
    // and returns the ONE shared MutationAcceptance — business outcomes are reads.
    let fields: Vec<String> = api
        .mutations
        .iter()
        .map(|m| {
            format!(
                "  {}(input: {}Input!, metadata: MetadataInput): MutationAcceptance! {} @command(name: \"{}\")",
                m.name, m.command, auth_directive(&m.roles), m.command
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
    s.push_str("# are derived from views.yaml foreign keys. Mutations are ACCEPTANCE-FIRST (ADR-20260720-015500):\n");
    s.push_str("# every mutation takes an optional `metadata: MetadataInput` and returns the shared MutationAcceptance\n");
    s.push_str("# (effective envelope + operationStatus); business outcomes are reads (operationStatus/paymentStatus).\n\n");
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
/// Docs label for an operation's `roles:` — an omitted list means open to every role path
/// (literal roles, ADR-20260720-191500).
fn roles_label(roles: &[String]) -> String {
    if roles.is_empty() {
        "EVERYONE (open — roles omitted)".to_string()
    } else {
        roles.join(", ")
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
            format!("- **Roles**: {} · **slice** {}", roles_label(&q.roles), q.slice),
        ].join("\n") });
    }
    for m in &api.mutations {
        let handler = cmd_handler.get(&m.command);
        api_docs.push(Doc { ctx: cx.of_command(&m.command), md: vec![
            item_head("mutation", "Mutation", &m.name),
            format!("\n- **Command**: {}{}", dlink("command", &m.command), handler.map(|h| format!(" → handled by {}", dlink("actor", &h.0))).unwrap_or_default()),
            format!("- **Roles**: {} · **slice** {}", roles_label(&m.roles), m.slice),
            format!("- **Returns**: {} (acceptance-first — outcome via {})", dlink("type", "MutationAcceptance"), dlink("query", "operationStatus")),
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
            format!("- **Roles**: {} · **slice** {}", roles_label(&s.roles), s.slice),
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

    // actorDocs — process managers also embed their saga sequence diagram (typed steps); aggregates
    // with a declared `lifecycle` embed their state diagram (ADR-20260720-004419).
    let pm_seq: HashMap<String, String> = pm_sequence_map(model).into_iter().collect();
    let lc_state: HashMap<String, String> = lifecycle_state_map(model).into_iter().collect();
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
        if a.kind == "aggregate" {
            if let Some(d) = lc_state.get(&a.name) {
                parts.push(format!("\nLifecycle (generated from the declared state machine):\n\n```mermaid\n{}\n```", d));
            }
        } else if let Some(d) = pm_seq.get(&a.name) {
            parts.push(format!("\nSequence (generated from the typed steps):\n\n```mermaid\n{}\n```", d));
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
            format!("- **Workflow**: {}{}{}", wf.and_then(|w| w.get("surface")).and_then(|s| s.as_str()).map(|s| format!("surface `{}` (dispatch pipeline)", s)).unwrap_or_default(), wf.and_then(|w| w.get("saga")).map(|s| format!("saga {}", any_link(s.get("$ref").and_then(|x| x.as_str()).unwrap_or_default()))).unwrap_or_default(), wf.and_then(|w| w.get("command")).map(|c| format!(" · command {}", any_link(c.get("$ref").and_then(|x| x.as_str()).unwrap_or_default()))).unwrap_or_default()),
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

    // SDUI screens + translations (reuse the C4/HTML approach). Generic over every screens/*.yaml
    // surface (ADR-20260722-091500): each surface renders its own screens block under a header, so a new
    // audience appears in the docs automatically. tr_en/op_cell/boxf/collect_action_types are
    // surface-independent; resolvers/actions/screens are read per surface inside the loop.
    let screens_files: Vec<String> = model.defs.keys().filter(|k| k.starts_with("screens/")).cloned().collect();
    // translations merged from translations.yaml + screens/*.translations.yaml (translation_entries)
    let cellf = |s: &str| s.replace('|', "\\|");
    let tr_en = |rf: &str| -> String { resolve_ref(model, rf, "translations.yaml").and_then(|t| t.get("messages")).and_then(|m| m.get("en")).and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| rf.rsplit('/').next().unwrap_or(rf).to_string()) };
    let t_text = |v: &Value| -> String { if let Some(rf) = v.get("$ref").and_then(|x| x.as_str()) { tr_en(rf) } else if let Some(s) = v.as_str() { s.to_string() } else { String::new() } };
    let tr_rows: Vec<Vec<String>> = translation_entries(model).into_iter().map(|(_f, key, t)| { let params = t.get("params").and_then(|x| x.as_mapping()).map(|pm| pm.iter().filter_map(|(pk, _)| pk.as_str().map(|p| format!("`{}`", p))).collect::<Vec<_>>().join(", ")).unwrap_or_default(); let params = if params.is_empty() { "—".to_string() } else { params }; vec![format!("{}`{}`", id_tag(&danchor("translation", &key)), key), params, cellf(t.get("messages").and_then(|mm| mm.get("en")).and_then(|x| x.as_str()).unwrap_or("")), cellf(t.get("messages").and_then(|mm| mm.get("fr")).and_then(|x| x.as_str()).unwrap_or(""))] }).collect();
    let translations_section = md_table(&["Key", "Params", "🇬🇧 en", "🇫🇷 fr"], &tr_rows);
    let op_cell = |rf: Option<&str>, gap: Option<&str>| -> String { if let Some(g) = gap { return format!("⚠️ _gap: {}_", cellf(g)); } match rf { None => "—".to_string(), Some(rf) => { let name = rf.rsplit('/').next().unwrap_or(""); let kind = if rf.contains("/mutations/") { "mutation" } else if rf.contains("/subscriptions/") { "subscription" } else { "query" }; dlink(kind, name) } } };
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
    let mut surface_blocks: Vec<String> = Vec::new();
    for sfkey in &screens_files {
        let sf = model.defs.get(sfkey);
        let resolvers = sf.and_then(|v| v.get("resolvers")).and_then(|v| v.as_mapping());
        let action_defs = sf.and_then(|v| v.get("actions")).and_then(|v| v.as_mapping());
        let action_keys: HashSet<String> = action_defs.map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
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
        let surface = sfkey.strip_prefix("screens/").unwrap_or(sfkey);
        surface_blocks.push(format!("_Surface_ **`{}`**\n\n{}", surface, screen_docs.join("\n\n")));
    }
    let screens_section = surface_blocks.join("\n\n");

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
        "<!-- GENERATED by tools/codegen — do not edit by hand. Source: specs/*.yaml. -->\n# 📖 Captain.Food — Product Documentation (generated)\n\nA single, navigable view of the whole product, built from the specs and organized **top-level by\nbounded context** (🔲). Within each context: its API operations, output types, actors, views, commands,\nevents, entities, scalars, errors, business rules (📐 — what we guarantee), tests (🧪 — how it's verified,\ncross-linked to the rules) and observability contracts. Every item — and every\n**property** 🔹 — is anchored and **cross-linked**; `cross-cutting` holds the shared vocabulary and ops\nthat belong to no single context. Stories and Architecture span all contexts.\n\n**Kinds**: {q} query · {mu} mutation · {su} subscription · {ty} type · {ac} actor · {vi} view · {cm} command · {ev} event · {en} entity · {sc} scalar · {er} error · {pr} property\n**Roles**: 🌐 PUBLIC · 🙋 CUSTOMER · 🏪 RESTAURANT_ACCOUNT · 🍽️ RESTAURANT · 🛵 RIDER · 🛠️ ADMIN · 🔌 EXTERNAL\n**Markers**: ✅ required · ⬜ optional · 🛶 V0 · 🔭 V1 · 🔒 internal · ⚠️ design hole\n\n**Contents** — [🎬 Stories](#sec-stories) · {toc} · [📱 Screens](#sec-screens) · [🌐 Translations](#sec-translations) · [🏛️ Architecture](#sec-architecture)\n\n{s_stories}\n\nHow each persona uses the API. `personaRole` is the persona's GraphQL path-role (UserType).\n\n{stories}\n\n{ctxs}\n\n{s_screens}\n\nServer-Driven UI screens (`specs/screens/*.yaml`, one file per audience, ADR-0033/ADR-20260722-091500).\nEach screen's **reads** (resolvers →\nqueries) and **writes** (actions → mutations) are `$ref`-bound to the GraphQL API and validated, so the\nmockups below are the **proof the API answers the UI**. ⚠️ gaps mark UI needs the API does not serve yet.\nScreens marked 🚫 are intentionally not SDUI-rendered (Stripe/subscription/auth integrity).\n\n{screens}\n\n{s_trans}\n\nThe i18n catalog (`specs/translations.yaml`) — every user-visible screen string, referenced by `$ref` and\ngenerated to a single `translations.generated.json`. `{{param}}` tokens are validated against `params`.\n\n{trans}\n\n{s_arch}\n\nC4 views as source-managed DSL (`specs/architecture/c4-l{{2,3}}.yaml`). Bounded contexts bind their\naggregates; components bind the aggregates they handle and the read models they update.\n\n{c4}\n",
        q = d_emo("query"), mu = d_emo("mutation"), su = d_emo("subscription"), ty = d_emo("type"), ac = d_emo("actor"), vi = d_emo("view"), cm = d_emo("command"), ev = d_emo("event"), en = d_emo("entity"), sc = d_emo("scalar"), er = d_emo("error"), pr = d_emo("property"),
        toc = ctx_toc,
        s_stories = sec("stories", "🎬", "Stories"),
        stories = stories_section,
        ctxs = ctx_sections,
        s_screens = sec("screens", "📱", "Front-office screens (SDUI)"),
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
        let body = format!("{}<div class=\"rel\"><span class=\"lbl\">returns:</span> {} · <span class=\"lbl\">reads</span> {}</div><div class=\"rel\"><span class=\"lbl\">roles:</span> {} · <span class=\"badge\">{}</span></div>", input_rel, ret, reads, h_esc(&roles_label(&q.roles)), q.slice);
        let ctx = cx.of_operation(&q.roles, &(if !q.reads.is_empty() { cx.of_reads(&q.reads) } else { cx.of_type(&q.returns_type) }));
        api_docs.push(HDoc { ctx, html: h_item("query", "Query", &q.name, &body, q.description.as_deref()) });
    }
    for m in &api.mutations {
        let h = cmd_handler.get(&m.command);
        let body = format!("<div class=\"rel\"><span class=\"lbl\">command:</span> {}{}</div><div class=\"rel\"><span class=\"lbl\">roles:</span> {} · <span class=\"badge\">{}</span></div><div class=\"rel\"><span class=\"lbl\">returns:</span> {} <span class=\"muted\">(acceptance-first — outcome via {})</span></div>", h_link("command", &m.command), h.map(|h| format!(" → {}", h_link("actor", &h.0))).unwrap_or_default(), h_esc(&roles_label(&m.roles)), m.slice, h_link("type", "MutationAcceptance"), h_link("query", "operationStatus"));
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
        let body = format!("{}<div class=\"rel\"><span class=\"lbl\">streams:</span> {}</div><div class=\"rel\"><span class=\"lbl\">roles:</span> {} · <span class=\"badge\">{}</span></div>", input_rel, ret, h_esc(&roles_label(&s.roles)), s.slice);
        api_docs.push(HDoc { ctx: cx.of_operation(&s.roles, &cx.of_type(&s.returns_type)), html: h_item("subscription", "Subscription", &s.name, &body, s.description.as_deref()) });
    }
    let type_docs: Vec<HDoc> = api.types.iter().map(|t| {
        let reads = t.reads.iter().map(|v| h_link("view", v)).collect::<Vec<_>>().join(", ");
        let rows: Vec<Vec<String>> = t.properties.iter().map(|f| vec![format!("<span id=\"{}\" class=\"k-prop\">{}</span>", dprop_anchor("type", &t.name, &f.name), h_esc(&f.name)), h_api_type(f), h_req_cell(!f.nullable, f.nullable)]).collect();
        let body = format!("<div class=\"rel\"><span class=\"lbl\">read model:</span> {}</div>{}", if reads.is_empty() { "<span class=\"muted\">(within a parent projection)</span>".to_string() } else { reads }, h_table(&["Field", "Type", "Req."], &rows));
        HDoc { ctx: cx.of_type(&t.name), html: h_item("type", "Type", &t.name, &body, t.description.as_deref()) }
    }).collect();

    // 3. Actors — process managers also embed their saga sequence diagram, aggregates with a declared
    // `lifecycle` their state diagram (ADR-20260720-004419); the <pre class="mermaid">
    // source is rendered client-side by MERMAID_JS and stays readable as text when offline.
    let pm_seq: HashMap<String, String> = pm_sequence_map(model).into_iter().collect();
    let lc_state: HashMap<String, String> = lifecycle_state_map(model).into_iter().collect();
    let actor_docs: Vec<HDoc> = actors.iter().map(|a| {
        let kind = if a.kind == "aggregate" { "🧩 aggregate" } else { "⚙️ process manager" };
        let rows: Vec<Vec<String>> = a.receives.iter().map(|e| {
            let is_cmd = e.message_ref.starts_with("commands.yaml#/");
            let emits = { let s = e.emits.iter().map(|r| h_link("event", &ref_name(r).unwrap_or_default())).collect::<Vec<_>>().join(", "); if s.is_empty() { e.effect.as_deref().map(|x| format!("<span class=\"muted\">{}</span>", h_esc(x))).unwrap_or_else(|| "—".to_string()) } else { s } };
            let throws = { let s = e.throws.iter().map(|r| h_link("error", &ref_name(r).unwrap_or_default())).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
            vec![h_link(if is_cmd { "command" } else { "event" }, &ref_name(&e.message_ref).unwrap_or_else(|| "?".to_string())), emits, throws]
        }).collect();
        let seq = if a.kind == "aggregate" {
            lc_state.get(&a.name).map(|d| format!("<div class=\"pm-seq\"><pre class=\"mermaid\">{}</pre></div>", h_esc(d))).unwrap_or_default()
        } else {
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
            "<div class=\"rel\"><span class=\"lbl\">workflow:</span> {}{}{}</div><div class=\"rel\"><span class=\"lbl\">emits:</span> {} · <span class=\"lbl\">inbound:</span> {}</div>{}{}<div class=\"rel\"><span class=\"lbl\">metrics:</span> {} · <span class=\"lbl\">business:</span> {}</div>{}<div class=\"rel\"><span class=\"lbl\">SLOs:</span> p95 ≤ {}ms · p99 ≤ {}ms · error ≤ {}%</div>",
            wf.and_then(|w| w.get("surface")).and_then(|s| s.as_str()).map(|s| format!("surface <span class=\"kw\">{}</span> <span class=\"muted\">(dispatch pipeline)</span>", h_esc(s))).unwrap_or_default(),
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
    let screens_files: Vec<String> = model.defs.keys().filter(|k| k.starts_with("screens/")).cloned().collect();
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

    // SDUI screens + translations — generic over every screens/*.yaml surface (ADR-20260722-091500):
    // one screens block per surface under a header. tr_en/t_text/op_link/collect_action_types are
    // surface-independent; resolvers/actions/screens are read per surface inside the loop.
    // translations merged from translations.yaml + screens/*.translations.yaml (translation_entries)
    let tr_en = |rf: &str| -> String { resolve_ref(model, rf, "translations.yaml").and_then(|t| t.get("messages")).and_then(|m| m.get("en")).and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| rf.rsplit('/').next().unwrap_or(rf).to_string()) };
    let t_text = |v: &Value| -> String { if let Some(rf) = v.get("$ref").and_then(|x| x.as_str()) { tr_en(rf) } else if let Some(s) = v.as_str() { s.to_string() } else { String::new() } };
    let tr_rows: Vec<Vec<String>> = translation_entries(model).into_iter().map(|(_f, key, t)| { let params = t.get("params").and_then(|x| x.as_mapping()).map(|pm| pm.iter().filter_map(|(pk, _)| pk.as_str().map(|p| format!("<span class=\"k-param\">{}</span>", h_esc(p)))).collect::<Vec<_>>().join(", ")).unwrap_or_default(); let params = if params.is_empty() { "<span class=\"muted\">—</span>".to_string() } else { params }; vec![format!("<span id=\"{}\" class=\"k-scalar\">{} {}</span>", danchor("translation", &key), d_emo("translation"), h_esc(&key)), params, format!("🇬🇧 {}", h_esc(t.get("messages").and_then(|mm| mm.get("en")).and_then(|x| x.as_str()).unwrap_or(""))), format!("🇫🇷 {}", h_esc(t.get("messages").and_then(|mm| mm.get("fr")).and_then(|x| x.as_str()).unwrap_or("")))] }).collect();
    let translations_html = h_table(&["Key", "Params", "en", "fr"], &tr_rows);
    let op_link = |rf: Option<&str>, gap: Option<&str>| -> String { if let Some(g) = gap { return format!("<span class=\"opt\">⚠️ {}</span>", h_esc(g)); } match rf { None => "—".to_string(), Some(rf) => { let name = rf.rsplit('/').next().unwrap_or(""); let kind = if rf.contains("/mutations/") { "mutation" } else if rf.contains("/subscriptions/") { "subscription" } else { "query" }; h_link(kind, name) } } };
    fn collect_action_types(node: &Value, keys: &HashSet<String>, acc: &mut Vec<String>) {
        match node {
            Value::Sequence(s) => s.iter().for_each(|n| collect_action_types(n, keys, acc)),
            Value::Mapping(m) => { if let Some(t) = m.get(Value::String("type".into())).and_then(|x| x.as_str()) { if keys.contains(t) && !acc.contains(&t.to_string()) { acc.push(t.to_string()); } } for (_, v) in m { collect_action_types(v, keys, acc); } }
            _ => {}
        }
    }
    let mut all_screens: Vec<Value> = Vec::new();
    let mut screens_html = String::new();
    for sfkey in &screens_files {
        let sf = model.defs.get(sfkey);
        let resolvers = sf.and_then(|v| v.get("resolvers")).and_then(|v| v.as_mapping());
        let action_defs = sf.and_then(|v| v.get("actions")).and_then(|v| v.as_mapping());
        let action_keys: HashSet<String> = action_defs.map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
        let screens_arr = sf.and_then(|v| v.get("screens")).and_then(|x| x.as_sequence()).cloned().unwrap_or_default();
        let surface = sfkey.strip_prefix("screens/").unwrap_or(sfkey);
        screens_html.push_str(&format!("<p class=\"muted\">Surface <strong>{}</strong></p>", h_esc(surface)));
        let block: String = screens_arr.iter().map(|s| {
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
        screens_html.push_str(&block);
        all_screens.extend(screens_arr);
    }

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
    for (_f, key, t) in translation_entries(model) { let s = format!("{} / {}", t.get("messages").and_then(|mm| mm.get("en")).and_then(|x| x.as_str()).unwrap_or(""), t.get("messages").and_then(|mm| mm.get("fr")).and_then(|x| x.as_str()).unwrap_or("")); put("translation", &key, &s); }
    for s in &all_screens { if let Some(id) = s.get("id").and_then(|x| x.as_str()) { let msg = format!("{}screen {}", if s.get("sdui").and_then(|x| x.as_bool()) == Some(false) { "Non-SDUI " } else { "SDUI " }, s.get("route").and_then(|x| x.as_str()).unwrap_or("")); put("screen", id, &msg); } }
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
    out.push_str(&h_sec("screens", "📱", "Front-office screens (SDUI)", &(String::from("<p class=\"muted\">Server-Driven UI screens (specs/screens/*.yaml, one file per audience, ADR-0033/ADR-20260722-091500). Per screen, the reads (resolvers→queries) and writes (actions→mutations) are $ref-bound to the GraphQL API and validated — the mockups are the <strong>proof the API answers the UI</strong>. ⚠️ marks gaps the API does not serve yet; 🚫 screens are intentionally not SDUI-rendered.</p>") + &screens_html)));
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
                // Default: a projection folding a legacy event that predates a required property
                // falls back to the empty value instead of panicking the worker (see the
                // ScalarLatest required-on-creation arm of the projector emitter).
                ("Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize", "String")
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
                                // Optional on the event but NOT NULL in the row: legacy production
                                // events can predate the property — defaulting keeps the projection
                                // total (a panicking accessor wedges the whole worker at that
                                // position forever).
                                (true, false) => format!("e.{}.clone().unwrap_or_default()", field),
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

// ─── crates/{application,infrastructure}/src/generated/pm_state.rs (issue #27 — PM state tables) ─────

/// How a PM state-table column binds to Rust/sqlx — classified from its declared `type` (a scalars.yaml
/// `$ref` or a SQL primitive). PM tables carry no lineage (`from`), so the type is always explicit.
enum PmTy {
    /// scalars.yaml newtype over `uuid::Uuid` (Copy).
    UuidScalar(String),
    /// scalars.yaml newtype over `String`.
    StringScalar(String),
    /// scalars.yaml newtype over `i64` (Copy — e.g. `MoneyCents`, stored BIGINT).
    IntScalar(String),
    /// scalars.yaml enum — stored as its INTEGER declaration-order ordinal (ADR-0037).
    EnumScalar(String),
    /// SQL `text`.
    Text,
    /// SQL `integer` (i32 — matches the migration's INTEGER, unlike projection rows' bigint-ish i64).
    Integer,
    /// SQL `timestamptz`.
    Timestamptz,
}

fn pm_ty(model: &Model, table: &str, col: &str, ty: &str) -> PmTy {
    match ty {
        "text" => return PmTy::Text,
        "integer" => return PmTy::Integer,
        "timestamptz" => return PmTy::Timestamptz,
        _ => {}
    }
    let node = model
        .defs
        .get("scalars.yaml")
        .and_then(|s| s.get(ty))
        .unwrap_or_else(|| panic!("process_managers.yaml#/{}/columns/{}: unsupported column type '{}' — expected a scalars.yaml $ref or text/integer/timestamptz", table, col, ty));
    if node.get("enum").map(|e| e.is_sequence()).unwrap_or(false) {
        PmTy::EnumScalar(ty.to_string())
    } else if node.get("format").and_then(|f| f.as_str()) == Some("uuid") {
        PmTy::UuidScalar(ty.to_string())
    } else if node.get("type").and_then(|t| t.as_str()) == Some("integer") {
        PmTy::IntScalar(ty.to_string())
    } else {
        PmTy::StringScalar(ty.to_string())
    }
}

impl PmTy {
    /// The row-struct field type (before the `Option<…>` nullable wrap).
    fn field(&self) -> String {
        match self {
            PmTy::UuidScalar(n) | PmTy::StringScalar(n) | PmTy::IntScalar(n) | PmTy::EnumScalar(n) => n.clone(),
            PmTy::Text => "String".into(),
            PmTy::Integer => "i32".into(),
            PmTy::Timestamptz => "chrono::DateTime<chrono::Utc>".into(),
        }
    }
    /// Whether a lookup passes this type by reference (`&Ty`) — String-backed newtypes only; Copy
    /// scalars go by value (mirrors the hand-written signatures this emitter replaced).
    fn by_ref(&self) -> bool {
        matches!(self, PmTy::StringScalar(_))
    }
}

/// One `by_*` lookup a PM state store exposes: the pk lookup, one per UNIQUE correlation column, plus
/// the emitter-registered extras (reads a saga explicitly declares, e.g. `paymentStatus`).
struct PmLookup {
    method: String,
    column: String,
    doc: String,
}

/// One PM state table of `database/tables/process_managers.yaml`, with its derived Rust names.
struct PmTable {
    table: String,
    /// CamelCase base of the generated names (`PaymentProcess` → `PaymentProcessRow`,
    /// `PaymentProcessStateStore`, `MemPaymentProcessState`, `PgPaymentProcessState`).
    base: String,
    note: Option<String>,
    columns: Vec<SqlColumn>,
    pk: String,
    lookups: Vec<PmLookup>,
}

/// CamelCase base name of a PM state table: the table name minus the `_process_manager` suffix; a
/// single-word stem keeps the `Process` word for readability (`payment` → `PaymentProcess`, but
/// `cart_binding` → `CartBinding`).
fn pm_base_name(table: &str) -> String {
    let stem = table.strip_suffix("_process_manager").unwrap_or(table);
    let camel: String = stem
        .split('_')
        .map(|w| {
            let mut cs = w.chars();
            match cs.next() {
                Some(c) => c.to_ascii_uppercase().to_string() + cs.as_str(),
                None => String::new(),
            }
        })
        .collect();
    if stem.contains('_') { camel } else { format!("{}Process", camel) }
}

/// Lookup method name for a column: `by_<column minus its trailing _id>` — mechanical, so the
/// `state.by` keys of processmanager.yaml map 1:1 onto store methods (roadmap item 3).
fn pm_lookup_method(column: &str) -> String {
    format!("by_{}", column.strip_suffix("_id").unwrap_or(column))
}

/// Parse the PM state tables (file order). Lookups = the pk, every `unique: true` correlation column,
/// plus EXTRA_LOOKUPS — the narrowly-scoped initiator reads a saga explicitly declares. Those reads'
/// resolver bodies are hardcoded in the server query/subscription emitters (`paymentStatus` /
/// `paymentStatusChanged`, ADR-20260720-015500), so the lookup that serves them is registered here,
/// next to that code, rather than inferred from the table DSL.
fn parse_pm_tables(model: &Model) -> Vec<PmTable> {
    const FILE: &str = "database/tables/process_managers.yaml";
    const EXTRA_LOOKUPS: &[(&str, &str, &str)] = &[(
        "payment_process_manager",
        "order_id",
        "The run that will materialize this order — the `paymentStatus(orderId)` read (ADR-20260720-015500; the caller enforces the initiator ownership scope).",
    )];
    let events = model.defs.get("events.yaml").cloned().unwrap_or(Value::Null);
    let mut out = Vec::new();
    let Some(Value::Mapping(m)) = model.defs.get(FILE) else {
        return out;
    };
    for (k, node) in m {
        let (Some(table), Some(cols)) = (k.as_str(), node.get("columns").and_then(|c| c.as_mapping())) else {
            continue;
        };
        let columns: Vec<SqlColumn> = cols
            .iter()
            .filter_map(|(ck, cv)| ck.as_str().map(|n| parse_col(n.to_string(), cv, &events)))
            .collect();
        let pks: Vec<&SqlColumn> = columns.iter().filter(|c| c.pk).collect();
        assert!(pks.len() == 1, "{}#/{}: expected exactly one pk column (the run's correlation identity), found {}", FILE, table, pks.len());
        let pk = pks[0].name.clone();
        assert!(
            columns.iter().any(|c| c.name == "last_update_utc" && c.ty == "timestamptz" && !c.nullable),
            "{}#/{}: missing the non-nullable `last_update_utc` timestamptz envelope column", FILE, table
        );
        let mut lookups = vec![PmLookup {
            method: pm_lookup_method(&pk),
            column: pk.clone(),
            doc: format!("The live run for this {}, if any (pk lookup).", pk.strip_suffix("_id").unwrap_or(&pk).replace('_', " ")),
        }];
        for c in columns.iter().filter(|c| c.unique && !c.pk) {
            assert!(!c.nullable, "{}#/{}: UNIQUE lookup column `{}` must be non-nullable", FILE, table, c.name);
            lookups.push(PmLookup {
                method: pm_lookup_method(&c.name),
                column: c.name.clone(),
                doc: format!("Correlate an inbound fact back to its run (UNIQUE `{}`).", c.name),
            });
        }
        for (_, col, doc) in EXTRA_LOOKUPS.iter().filter(|(t, _, _)| *t == table) {
            let c = columns
                .iter()
                .find(|c| c.name == *col)
                .unwrap_or_else(|| panic!("{}#/{}: EXTRA_LOOKUPS names unknown column `{}`", FILE, table, col));
            assert!(!c.nullable, "{}#/{}: extra lookup column `{}` must be non-nullable", FILE, table, col);
            lookups.push(PmLookup { method: pm_lookup_method(col), column: (*col).to_string(), doc: (*doc).to_string() });
        }
        out.push(PmTable {
            table: table.to_string(),
            base: pm_base_name(table),
            note: node.get("description").and_then(|d| d.as_str()).map(|s| s.to_string()),
            columns,
            pk,
            lookups,
        });
    }
    out
}

/// Emit `crates/application/src/generated/pm_state.rs` — the process-manager STATE persistence ports
/// (issue #27, replacing the hand-written `application/src/pm_state.rs`): one `<Base>Row` struct +
/// `<Base>StateStore` trait per table of `database/tables/process_managers.yaml`, plus the in-memory
/// `mem::Mem<Base>State` doubles the orchestrator tests run against. `last_update_utc` is the RUNTIME
/// ENVELOPE's stamp: every `upsert` writes it server-side (`now()`) and IGNORES the row's carried value.
fn emit_pm_state_application(model: &Model) -> String {
    let tables = parse_pm_tables(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/database/tables/process_managers.yaml — do not edit by hand.\n// Process-manager STATE persistence ports (ADR-20260719-172821): one row = one saga run, keyed by the\n// run's correlation identity. PRIVATE to their process manager — no projection reads them and no query\n// serves them, except the narrowly-scoped initiator reads a saga explicitly declares (paymentStatus,\n// ADR-20260720-015500). `last_update_utc` is maintained by the RUNTIME ENVELOPE, never by a step: every\n// `upsert` stamps it server-side (`now()`) — the value carried on the row is IGNORED on write. The\n// `mem` submodule provides in-memory doubles for the orchestrator tests.\n\nuse async_trait::async_trait;\nuse domain::generated::scalars::*;\nuse domain::shared::errors::DomainError;\n",
    );
    for t in &tables {
        // Row struct.
        out.push('\n');
        let note = ws1(t.note.as_deref().unwrap_or(""));
        if note.trim().is_empty() {
            out.push_str(&format!("/// One `{}` row.\n", t.table));
        } else {
            out.push_str(&format!("/// One `{}` row. {}\n", t.table, note.trim()));
        }
        out.push_str("#[derive(Debug, Clone, PartialEq)]\n");
        out.push_str(&format!("pub struct {}Row {{\n", t.base));
        for c in &t.columns {
            let ty = pm_ty(model, &t.table, &c.name, &c.ty);
            if c.name == "last_update_utc" {
                out.push_str("    /// Maintained by the runtime envelope — ignored on write, stamped `now()` by `upsert`.\n");
            } else if let Some(note) = &c.note {
                out.push_str(&format!("    /// {}\n", ws1(note)));
            }
            let ft = if c.nullable { format!("Option<{}>", ty.field()) } else { ty.field() };
            out.push_str(&format!("    pub {}: {},\n", rust_ident(&c.name), ft));
        }
        out.push_str("}\n");
        // Store trait.
        out.push('\n');
        out.push_str(&format!("/// State store for `{}` runs.\n#[async_trait]\npub trait {}StateStore: Send + Sync {{\n", t.table, t.base));
        for l in &t.lookups {
            let c = t.columns.iter().find(|c| c.name == l.column).unwrap();
            let ty = pm_ty(model, &t.table, &c.name, &c.ty);
            let param = if ty.by_ref() { format!("&{}", ty.field()) } else { ty.field() };
            out.push_str(&format!("    /// {}\n", l.doc));
            out.push_str(&format!(
                "    async fn {}(&self, {}: {}) -> Result<Option<{}Row>, DomainError>;\n\n",
                l.method,
                rust_ident(&l.column),
                param,
                t.base
            ));
        }
        out.push_str("    /// Insert or replace the run's row; `last_update_utc` is stamped server-side (`now()`).\n");
        out.push_str(&format!("    async fn upsert(&self, row: &{}Row) -> Result<(), DomainError>;\n}}\n", t.base));
    }
    // In-memory doubles.
    out.push_str(
        "\n/// In-memory implementations of the state-store ports (plain `Mutex<HashMap>`), for the process-manager\n/// orchestrator tests. They mirror the Postgres semantics: `upsert` replaces the whole row and stamps\n/// `last_update_utc = now()` (the row's own value is ignored), reads return the stored row.\npub mod mem {\n    use super::*;\n    use std::collections::HashMap;\n    use std::sync::Mutex;\n",
    );
    for t in &tables {
        let pk_col = t.columns.iter().find(|c| c.name == t.pk).unwrap();
        let pk_ty = pm_ty(model, &t.table, &pk_col.name, &pk_col.ty);
        let key = match &pk_ty {
            PmTy::UuidScalar(_) => "uuid::Uuid",
            PmTy::StringScalar(_) | PmTy::Text => "String",
            PmTy::IntScalar(_) => "i64",
            PmTy::Integer => "i32",
            other => panic!("process_managers.yaml#/{}: pk type {:?} not supported as a mem key", t.table, other.field()),
        };
        out.push('\n');
        out.push_str(&format!("    /// In-memory [`{}StateStore`], keyed by `{}`.\n    #[derive(Default)]\n    pub struct Mem{}State {{\n        rows: Mutex<HashMap<{}, {}Row>>,\n    }}\n", t.base, t.pk, t.base, key, t.base));
        out.push('\n');
        out.push_str(&format!("    #[async_trait]\n    impl {}StateStore for Mem{}State {{\n", t.base, t.base));
        for l in &t.lookups {
            let c = t.columns.iter().find(|c| c.name == l.column).unwrap();
            let ty = pm_ty(model, &t.table, &c.name, &c.ty);
            let param = if ty.by_ref() { format!("&{}", ty.field()) } else { ty.field() };
            let ident = rust_ident(&l.column);
            let body = if l.column == t.pk {
                format!("Ok(self.rows.lock().unwrap().get(&{}.0).cloned())", ident)
            } else if ty.by_ref() {
                format!("Ok(self.rows.lock().unwrap().values().find(|r| &r.{} == {}).cloned())", ident, ident)
            } else {
                format!("Ok(self.rows.lock().unwrap().values().find(|r| r.{} == {}).cloned())", ident, ident)
            };
            out.push_str(&format!(
                "        async fn {}(&self, {}: {}) -> Result<Option<{}Row>, DomainError> {{\n            {}\n        }}\n\n",
                l.method, ident, param, t.base, body
            ));
        }
        let key_expr = match &pk_ty {
            PmTy::StringScalar(_) => format!("stamped.{}.0.clone()", rust_ident(&t.pk)),
            PmTy::Text => format!("stamped.{}.clone()", rust_ident(&t.pk)),
            _ => format!("stamped.{}.0", rust_ident(&t.pk)),
        };
        out.push_str(&format!(
            "        async fn upsert(&self, row: &{}Row) -> Result<(), DomainError> {{\n            let mut stamped = row.clone();\n            stamped.last_update_utc = chrono::Utc::now();\n            self.rows.lock().unwrap().insert({}, stamped);\n            Ok(())\n        }}\n    }}\n",
            t.base, key_expr
        ));
    }
    out.push_str("}\n");
    out
}

/// Emit `crates/infrastructure/src/generated/pm_state.rs` — the Postgres adapters for the PM state
/// stores (issue #27, replacing the hand-written `infrastructure/persistence/pm_state.rs`): one
/// `Pg<Base>State` per `application::pm_state` port. Conventions match the projection stores: enum
/// columns are INTEGER declaration-order ordinals (`persistence::enum_sql`), scalar newtypes bind via
/// `.0`, upserts are `INSERT … ON CONFLICT (pk) DO UPDATE` over all columns, and `last_update_utc` is
/// stamped `now()` server-side on every upsert (the row's carried value is IGNORED).
fn emit_pm_state_infrastructure(model: &Model) -> String {
    let tables = parse_pm_tables(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/database/tables/process_managers.yaml — do not edit by hand.\n// Postgres adapters for the process-manager STATE stores (ADR-20260719-172821): one `Pg…State` per\n// `application::pm_state` port, over the saga state tables (migration\n// `20260719200000_process_manager_state_tables.sql`). Conventions match the projection stores: enum\n// columns are INTEGER declaration-order ordinals (`crate::persistence::enum_sql`); scalar newtypes\n// bind via `.0`; upserts are `INSERT … ON CONFLICT (pk) DO UPDATE` over all columns. `last_update_utc`\n// is the runtime envelope's stamp: every upsert writes `now()` server-side (the row's carried value is\n// IGNORED), reads return the stored value.\n\nuse application::pm_state::*;\nuse async_trait::async_trait;\nuse domain::generated::scalars::*;\nuse domain::shared::errors::DomainError;\nuse sqlx::postgres::PgRow;\nuse sqlx::{PgPool, Row};\n\nuse crate::persistence::db_err;\nuse crate::persistence::enum_sql::EnumOrd;\n",
    );
    for t in &tables {
        let upper = snake_type(&t.base).to_uppercase();
        let snake = snake_type(&t.base);
        let cols: Vec<&str> = t.columns.iter().map(|c| c.name.as_str()).collect();
        out.push_str(&format!(
            "\n// ---------------------------------------------------------------------------------------------------\n// {}\n// ---------------------------------------------------------------------------------------------------\n",
            t.table
        ));
        // Column list const.
        out.push_str(&format!(
            "\n/// Column list of `{}`, in [`{}Row`] field order.\nconst {}_COLUMNS: &str = \"{}\";\n",
            t.table,
            t.base,
            upper,
            cols.join(", ")
        ));
        // Row decoder.
        out.push_str(&format!("\nfn decode_{}(row: &PgRow) -> Result<{}Row, DomainError> {{\n    Ok({}Row {{\n", snake, t.base, t.base));
        for c in &t.columns {
            let ty = pm_ty(model, &t.table, &c.name, &c.ty);
            let ident = rust_ident(&c.name);
            let expr = match (&ty, c.nullable) {
                (PmTy::UuidScalar(n), false) => format!("{}(row.try_get(\"{}\").map_err(db_err)?)", n, c.name),
                (PmTy::UuidScalar(n), true) => format!("row.try_get::<Option<uuid::Uuid>, _>(\"{}\").map_err(db_err)?.map({})", c.name, n),
                (PmTy::StringScalar(n), false) => format!("{}(row.try_get(\"{}\").map_err(db_err)?)", n, c.name),
                (PmTy::StringScalar(n), true) => format!("row.try_get::<Option<String>, _>(\"{}\").map_err(db_err)?.map({})", c.name, n),
                (PmTy::IntScalar(n), false) => format!("{}(row.try_get(\"{}\").map_err(db_err)?)", n, c.name),
                (PmTy::IntScalar(n), true) => format!("row.try_get::<Option<i64>, _>(\"{}\").map_err(db_err)?.map({})", c.name, n),
                (PmTy::EnumScalar(_), false) => format!("EnumOrd::from_ord(row.try_get::<i32, _>(\"{}\").map_err(db_err)?)?", c.name),
                (PmTy::EnumScalar(_), true) => format!("crate::persistence::enum_sql::opt_from_ord(row.try_get::<Option<i32>, _>(\"{}\").map_err(db_err)?)?", c.name),
                (PmTy::Text, true) => format!("row.try_get::<Option<String>, _>(\"{}\").map_err(db_err)?", c.name),
                _ => format!("row.try_get(\"{}\").map_err(db_err)?", c.name),
            };
            out.push_str(&format!("        {}: {},\n", ident, expr));
        }
        out.push_str("    })\n}\n");
        // Store struct.
        out.push_str(&format!(
            "\n/// Postgres [`{}StateStore`] over `{}`.\npub struct Pg{}State {{\n    pool: PgPool,\n}}\n\nimpl Pg{}State {{\n    pub fn new(pool: PgPool) -> Self {{\n        Self {{ pool }}\n    }}\n}}\n",
            t.base, t.table, t.base, t.base
        ));
        // Trait impl.
        out.push_str(&format!("\n#[async_trait]\nimpl {}StateStore for Pg{}State {{\n", t.base, t.base));
        for l in &t.lookups {
            let c = t.columns.iter().find(|c| c.name == l.column).unwrap();
            let ty = pm_ty(model, &t.table, &c.name, &c.ty);
            let param = if ty.by_ref() { format!("&{}", ty.field()) } else { ty.field() };
            let ident = rust_ident(&l.column);
            let bind = match &ty {
                PmTy::StringScalar(_) => format!("{}.0.clone()", ident),
                PmTy::Text => format!("{}.clone()", ident),
                PmTy::EnumScalar(_) => format!("{}.to_ord()", ident),
                PmTy::Integer => ident.clone(),
                _ => format!("{}.0", ident),
            };
            out.push_str(&format!(
                "    async fn {}(&self, {}: {}) -> Result<Option<{}Row>, DomainError> {{\n        let sql = format!(\"SELECT {{{}_COLUMNS}} FROM {} WHERE {} = $1\");\n        let row = sqlx::query(&sql).bind({}).fetch_optional(&self.pool).await.map_err(db_err)?;\n        row.as_ref().map(decode_{}).transpose()\n    }}\n\n",
                l.method, ident, param, t.base, upper, t.table, l.column, bind, snake
            ));
        }
        // Upsert: VALUES ($1..$n) with now() at last_update_utc's position; DO UPDATE over non-pk columns.
        let mut placeholders = Vec::new();
        let mut binds = Vec::new();
        let mut i = 0;
        for c in &t.columns {
            if c.name == "last_update_utc" {
                placeholders.push("now()".to_string());
                continue;
            }
            i += 1;
            placeholders.push(format!("${}", i));
            let ty = pm_ty(model, &t.table, &c.name, &c.ty);
            let ident = rust_ident(&c.name);
            let bind = match (&ty, c.nullable) {
                (PmTy::UuidScalar(_), false) => format!("row.{}.0", ident),
                (PmTy::UuidScalar(_), true) => format!("row.{}.as_ref().map(|v| v.0)", ident),
                (PmTy::StringScalar(_), false) => format!("row.{}.0.clone()", ident),
                (PmTy::StringScalar(_), true) => format!("row.{}.as_ref().map(|v| v.0.clone())", ident),
                (PmTy::IntScalar(_), false) => format!("row.{}.0", ident),
                (PmTy::IntScalar(_), true) => format!("row.{}.map(|v| v.0)", ident),
                (PmTy::EnumScalar(_), false) => format!("row.{}.to_ord()", ident),
                (PmTy::EnumScalar(_), true) => format!("crate::persistence::enum_sql::opt_to_ord(&row.{})", ident),
                (PmTy::Text, _) => format!("row.{}.clone()", ident),
                (PmTy::Integer, _) => format!("row.{}", ident),
                (PmTy::Timestamptz, _) => format!("row.{}", ident),
            };
            binds.push(bind);
        }
        let updates: Vec<String> = t
            .columns
            .iter()
            .filter(|c| !c.pk)
            .map(|c| {
                if c.name == "last_update_utc" {
                    "last_update_utc = now()".to_string()
                } else {
                    format!("{} = EXCLUDED.{}", c.name, c.name)
                }
            })
            .collect();
        out.push_str(&format!(
            "    async fn upsert(&self, row: &{}Row) -> Result<(), DomainError> {{\n        let sql = format!(\n            \"INSERT INTO {} ({{{}_COLUMNS}}) \\\n             VALUES ({}) \\\n             ON CONFLICT ({}) DO UPDATE SET \\\n             {}\"\n        );\n        sqlx::query(&sql)\n",
            t.base,
            t.table,
            upper,
            placeholders.join(","),
            t.pk,
            updates.join(", \\\n             ")
        ));
        for b in &binds {
            out.push_str(&format!("            .bind({})\n", b));
        }
        out.push_str("            .execute(&self.pool)\n            .await\n            .map_err(db_err)?;\n        Ok(())\n    }\n}\n");
    }
    out
}

// ================================================================================================
// Service-catalog emitters (issue #26, ADR-20260719-214500 / codegen-roadmap item 4): trait +
// http client + binding wiring + expose-gated /services routes from specs/services.yaml.
// ================================================================================================

/// One operation of a service: the typed `input`/`output` property maps (services.yaml field style)
/// and the anticipated `errors.yaml` rejection names.
struct SvcOp {
    name: String,
    desc: String,
    input: Option<Value>,
    output: Option<Value>,
    errors: Vec<String>,
}

/// One service of services.yaml with its derived Rust names and SPEC-OWNED topology.
struct Svc {
    name: String,
    /// PascalCase base of the generated names (`payment` → trait `PaymentService`,
    /// structs `Payment<Op>Input`/`…Output`, client `HttpPaymentService`).
    base: String,
    desc: String,
    binding: String,
    expose: bool,
    ops: Vec<SvcOp>,
}

/// PascalCase of a snake_case name (`offer_job` → `OfferJob`).
fn pascal_snake(s: &str) -> String {
    s.split('_').map(pascal).collect()
}

/// The DERIVED HTTP path of a service operation: `/services/<service>/<op>`, snake_case →
/// kebab-case (ADR-20260719-214500 — derived, never hand-picked).
fn svc_http_path(service: &str, op: &str) -> String {
    format!("/services/{}/{}", service.replace('_', "-"), op.replace('_', "-"))
}

/// The address-book variable of an http-bound service: `SERVICE_<NAME>_URL`.
fn svc_url_var(service: &str) -> String {
    format!("SERVICE_{}_URL", service.to_uppercase())
}

/// Parse services.yaml (file order; the §2d `svc-*` rules already validated the shape).
fn parse_services(model: &Model) -> Vec<Svc> {
    let mut out = Vec::new();
    let Some(Value::Mapping(m)) = model.defs.get("services.yaml") else {
        return out;
    };
    for (k, node) in m {
        let Some(name) = k.as_str() else { continue };
        let ops = node
            .get("operations")
            .and_then(|x| x.as_mapping())
            .map(|ops| {
                ops.iter()
                    .filter_map(|(ok, op)| {
                        let oname = ok.as_str()?;
                        Some(SvcOp {
                            name: oname.to_string(),
                            desc: op.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string(),
                            input: op.get("input").cloned(),
                            output: op.get("output").cloned(),
                            errors: ref_strings(op.get("errors")).iter().filter_map(|r| ref_name(r)).collect(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(Svc {
            name: name.to_string(),
            base: pascal_snake(name),
            desc: node.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string(),
            binding: node.get("binding").and_then(|b| b.as_str()).unwrap_or("local").to_string(),
            expose: node.get("expose").and_then(|b| b.as_bool()).unwrap_or(false),
            ops,
        });
    }
    out
}

/// Emit one serde `camelCase` struct for a service operation's `input`/`output` property map.
/// Unlike events/commands (which carry `required:` lists), service payload fields are REQUIRED
/// unless marked `nullable: true` — the catalog declares the exact call surface.
fn push_service_struct(out: &mut String, name: &str, doc: &str, props: &Value) {
    out.push('\n');
    if !doc.trim().is_empty() {
        out.push_str(&format!("/// {}\n", ws1(doc).trim()));
    }
    out.push_str("#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]\n");
    out.push_str("#[serde(rename_all = \"camelCase\")]\n");
    out.push_str(&format!("pub struct {} {{\n", name));
    if let Some(map) = props.as_mapping() {
        for (pk, pnode) in map {
            let Some(prop) = pk.as_str() else { continue };
            let field = snake_field(prop);
            // PROVE serde's camelCase rename restores the exact spec property name on the wire.
            assert_eq!(
                serde_camel(&field),
                prop,
                "services.yaml#/{}/{}: field '{}' does not round-trip through serde rename_all",
                name,
                prop,
                field
            );
            let ident = if RUST_FIELD_KEYWORDS.contains(&field.as_str()) { format!("r#{}", field) } else { field };
            let ty = struct_field_type("services.yaml", name, prop, pnode);
            if pnode.get("nullable").and_then(|x| x.as_bool()) == Some(true) {
                out.push_str(&format!("    pub {}: Option<{}>,\n", ident, ty));
            } else {
                out.push_str(&format!("    pub {}: {},\n", ident, ty));
            }
        }
    }
    out.push_str("}\n");
}

/// The generated trait-method signature of a service operation: `(&self, input: <In>, meta:
/// &ServiceCallMeta) -> Result<<Out> | (), DomainError>`; an input-less operation drops the
/// `input` parameter.
fn svc_op_signature(svc: &Svc, op: &SvcOp) -> String {
    let input = if op.input.is_some() {
        format!("input: {}{}Input, ", svc.base, pascal_snake(&op.name))
    } else {
        String::new()
    };
    let output = if op.output.is_some() {
        format!("{}{}Output", svc.base, pascal_snake(&op.name))
    } else {
        "()".to_string()
    };
    format!("async fn {}(&self, {}meta: &ServiceCallMeta) -> Result<{}, DomainError>", op.name, input, output)
}

/// Emit `crates/application/src/generated/services.rs` — the SERVICE PORT traits (issue #26,
/// ADR-20260719-214500): per service one `<Base>Service` trait with one method per operation, plus
/// the typed `<Base><Op>Input`/`…Output` structs (serde `camelCase`, so the http binding's wire
/// shape IS the spec shape). Every call also carries the [`ServiceCallMeta`] ENVELOPE — the
/// correlation metadata (ADR-0041 spirit) that is never part of the spec-declared business input.
fn emit_services_application(model: &Model) -> String {
    let services = parse_services(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/services.yaml — do not edit by hand.\n\
         // SERVICE PORT traits (ADR-20260719-214500, issue #26): the abstract capabilities the domain\n\
         // calls, one trait per service, consumed by command handlers and process managers. Provider\n\
         // vocabulary never appears here — the implementation ACL (adapter crates) translates names and\n\
         // payloads. The spec-declared `input`/`output` are the BUSINESS payload; correlation metadata\n\
         // travels on the [`ServiceCallMeta`] envelope (like the event envelope, ADR-0041), so business\n\
         // ids an INBOUND adapter needs to map provider facts back onto our aggregates (e.g. the Stripe\n\
         // PaymentIntent `metadata` the webhook ACL reads back) are declared at the call site and copied\n\
         // verbatim by the provider ACL — never smuggled into the operation input.\n\n\
         use async_trait::async_trait;\n\
         use domain::generated::entities::*;\n\
         use domain::generated::events::*;\n\
         use domain::generated::scalars::*;\n\
         use domain::shared::errors::DomainError;\n\
         use serde::{Deserialize, Serialize};\n",
    );
    out.push_str(
        r#"
/// The service-call ENVELOPE: infrastructure/correlation metadata that travels WITH every
/// operation call but is never part of the spec-declared business input (ADR-0041 spirit).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceCallMeta {
    /// The triggering command/event's correlation id (ADR-0041), propagated across every binding.
    pub correlation_id: uuid::Uuid,
    /// Business correlation references the INBOUND adapter needs to map provider facts back onto
    /// our aggregates (e.g. Stripe intent metadata `orderId`/`restaurantId`/`cartId`) — copied
    /// verbatim onto the provider call, never business input.
    #[serde(default)]
    pub refs: std::collections::BTreeMap<String, String>,
}

impl ServiceCallMeta {
    /// Envelope for a call correlated to `correlation_id`, with no business refs.
    pub fn new(correlation_id: uuid::Uuid) -> Self {
        Self { correlation_id, refs: std::collections::BTreeMap::new() }
    }
    /// Attach one business correlation reference (builder style).
    pub fn with_ref(mut self, key: &str, value: impl Into<String>) -> Self {
        self.refs.insert(key.to_string(), value.into());
        self
    }
}
"#,
    );
    for svc in &services {
        out.push_str(&format!(
            "\n// ---------------------------------------------------------------------------------------------------\n// service `{}` — binding: {}, expose: {}\n// ---------------------------------------------------------------------------------------------------\n",
            svc.name, svc.binding, svc.expose
        ));
        for op in &svc.ops {
            if let Some(input) = &op.input {
                push_service_struct(
                    &mut out,
                    &format!("{}{}Input", svc.base, pascal_snake(&op.name)),
                    &format!("Input of `{}.{}`.", svc.name, op.name),
                    input,
                );
            }
            if let Some(output) = &op.output {
                push_service_struct(
                    &mut out,
                    &format!("{}{}Output", svc.base, pascal_snake(&op.name)),
                    &format!("Output of `{}.{}`.", svc.name, op.name),
                    output,
                );
            }
        }
        out.push('\n');
        if !svc.desc.trim().is_empty() {
            out.push_str(&format!("/// {}\n", ws1(&svc.desc).trim()));
        }
        out.push_str(&format!("#[async_trait]\npub trait {}Service: Send + Sync {{\n", svc.base));
        for op in &svc.ops {
            if !op.desc.trim().is_empty() {
                out.push_str(&format!("    /// {}\n", ws1(&op.desc).trim()));
            }
            if op.errors.is_empty() {
                out.push_str("    /// Anticipated rejections: none declared.\n");
            } else {
                out.push_str(&format!(
                    "    /// Anticipated rejections: {}.\n",
                    op.errors.iter().map(|e| format!("`errors.yaml#/{}`", e)).collect::<Vec<_>>().join(", ")
                ));
            }
            out.push_str(&format!("    {};\n", svc_op_signature(svc, op)));
        }
        out.push_str("}\n");
    }
    out
}

/// Emit `crates/infrastructure/src/generated/service_clients.rs` — the HTTP clients for the DERIVED
/// `/services/<service>/<op>` surface plus the shared wire envelopes. One `Http<Base>Service` per
/// service (compiled for every service so the wire path stays covered; the composition root only
/// CONSTRUCTS a client for a service whose spec binding is `http`, see `service_bindings.rs`).
fn emit_services_http_clients(model: &Model) -> String {
    let services = parse_services(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/services.yaml — do not edit by hand.\n\
         // HTTP clients for the DERIVED `/services/<service>/<op>` surface (ADR-20260719-214500,\n\
         // issue #26): every operation is a POST with the JSON call envelope `{ input, meta }`;\n\
         // a 2xx answers `{ output }` (null for output-less operations) and an error answers the\n\
         // kind-tagged `{ error }` envelope, so a remote failure rehydrates into the SAME\n\
         // `DomainError` a local call would return (rejections keep their errors.yaml code +\n\
         // context). Addresses come from `SERVICE_<NAME>_URL` — an address book, never a decision.\n\n\
         use application::generated::services::*;\n\
         use async_trait::async_trait;\n\
         use domain::shared::errors::DomainError;\n",
    );
    out.push_str(
        r#"
/// Wire envelope of one service-operation POST: the spec-declared input + the call envelope.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct WireCall<I> {
    pub input: I,
    pub meta: ServiceCallMeta,
}

/// Wire envelope of a 2xx response (`output` is `null` for output-less operations).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct WireOutput<O> {
    pub output: O,
}

/// Wire envelope of a non-2xx response.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct WireErrorEnvelope {
    pub error: WireError,
}

/// A `DomainError` on the wire, kind-tagged so the caller-side error is indistinguishable
/// from a local call's.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireError {
    Rejected { code: String, context: serde_json::Value },
    Invariant { message: String },
    Repository { message: String },
}

impl From<&DomainError> for WireError {
    fn from(e: &DomainError) -> Self {
        match e {
            DomainError::Rejected { code, context } => {
                WireError::Rejected { code: code.clone(), context: context.clone() }
            }
            DomainError::Invariant(m) => WireError::Invariant { message: m.clone() },
            DomainError::Repository(m) => WireError::Repository { message: m.clone() },
        }
    }
}

impl From<WireError> for DomainError {
    fn from(e: WireError) -> Self {
        match e {
            WireError::Rejected { code, context } => DomainError::Rejected { code, context },
            WireError::Invariant { message } => DomainError::Invariant(message),
            WireError::Repository { message } => DomainError::Repository(message),
        }
    }
}

/// HTTP status of a service error: anticipated rejections are 422, invariants 409,
/// dependency failures 502.
pub fn wire_error_status(e: &WireError) -> u16 {
    match e {
        WireError::Rejected { .. } => 422,
        WireError::Invariant { .. } => 409,
        WireError::Repository { .. } => 502,
    }
}

/// POST one service-operation call and decode the response envelopes back into the port's
/// `Result`. Transport failures and undecodable bodies are `DomainError::Repository`.
async fn post_call<I: serde::Serialize, O: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    base_url: &str,
    path: &str,
    input: I,
    meta: &ServiceCallMeta,
) -> Result<O, DomainError> {
    let response = http
        .post(format!("{base_url}{path}"))
        .json(&WireCall { input, meta: meta.clone() })
        .send()
        .await
        .map_err(|e| DomainError::Repository(format!("service call: transport error on {path}: {e}")))?;
    let status = response.status().as_u16();
    let body = response
        .text()
        .await
        .map_err(|e| DomainError::Repository(format!("service call: body read error on {path}: {e}")))?;
    if (200..300).contains(&status) {
        let decoded: WireOutput<O> = serde_json::from_str(&body)
            .map_err(|e| DomainError::Repository(format!("service call: undecodable output on {path}: {e}")))?;
        return Ok(decoded.output);
    }
    match serde_json::from_str::<WireErrorEnvelope>(&body) {
        Ok(envelope) => Err(envelope.error.into()),
        Err(_) => Err(DomainError::Repository(format!(
            "service call: HTTP {status} on {path}: {}",
            &body[..body.len().min(200)]
        ))),
    }
}
"#,
    );
    for svc in &services {
        out.push_str(&format!(
            "\n/// HTTP [`{base}Service`] over `POST {{base}}{example}` (address: `{var}`).\npub struct Http{base}Service {{\n    http: reqwest::Client,\n    base_url: String,\n}}\n\nimpl Http{base}Service {{\n    pub fn new(base_url: impl Into<String>) -> Self {{\n        Self {{ http: reqwest::Client::new(), base_url: base_url.into().trim_end_matches('/').to_string() }}\n    }}\n}}\n\n#[async_trait]\nimpl {base}Service for Http{base}Service {{\n",
            base = svc.base,
            example = svc_http_path(&svc.name, "<op>"),
            var = svc_url_var(&svc.name),
        ));
        for op in &svc.ops {
            let arg = if op.input.is_some() { "input" } else { "()" };
            out.push_str(&format!(
                "    {} {{\n        post_call(&self.http, &self.base_url, \"{}\", {}, meta).await\n    }}\n",
                svc_op_signature(svc, op),
                svc_http_path(&svc.name, &op.name),
                arg
            ));
        }
        out.push_str("}\n");
    }
    out
}

/// Emit `crates/infrastructure/src/generated/service_bindings.rs` — the composition-root topology
/// resolvers (SPEC-OWNED binding, ADR-20260719-214500): one function per service. `binding: local`
/// invokes the supplied in-process constructor; `binding: http` ignores it and constructs the
/// generated client from `SERVICE_<NAME>_URL` (a missing address is a startup error). Flipping a
/// service's binding is a reviewed spec change that regenerates ONLY this wiring.
fn emit_service_bindings(model: &Model) -> String {
    let services = parse_services(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/services.yaml — do not edit by hand.\n\
         // Composition-root topology bindings (ADR-20260719-214500, issue #26): the deployment\n\
         // topology is DECIDED IN THE SPEC (`binding: local | http` per service) — environment\n\
         // configuration carries only addresses (`SERVICE_<NAME>_URL`), never decisions. The\n\
         // composition root supplies the in-process constructor for every service; whether it is\n\
         // used (local) or replaced by the generated HTTP client (http) is this module's call.\n\n\
         use application::generated::services::*;\n\
         use std::sync::Arc;\n",
    );
    for svc in &services {
        if svc.binding == "http" {
            out.push_str(&format!(
                "\n/// `{name}` — binding: http (services.yaml): the generated client over `{var}`;\n/// the in-process constructor is NOT wired. A missing address is a startup error.\npub fn {name}_service(\n    local: impl FnOnce() -> Arc<dyn {base}Service>,\n) -> Result<Arc<dyn {base}Service>, String> {{\n    let _ = local; // http binding — the in-process implementation is not used\n    let url = std::env::var(\"{var}\")\n        .map_err(|_| \"{var} is required: service '{name}' is bound http (services.yaml)\".to_string())?;\n    Ok(Arc::new(crate::generated::service_clients::Http{base}Service::new(url)))\n}}\n",
                name = svc.name,
                base = svc.base,
                var = svc_url_var(&svc.name),
            ));
        } else {
            out.push_str(&format!(
                "\n/// `{name}` — binding: local (services.yaml): the in-process adapter the composition\n/// root supplies; zero HTTP inside the deployable.\npub fn {name}_service(\n    local: impl FnOnce() -> Arc<dyn {base}Service>,\n) -> Result<Arc<dyn {base}Service>, String> {{\n    Ok(local())\n}}\n",
                name = svc.name,
                base = svc.base,
            ));
        }
    }
    out
}

/// Emit `crates/server/src/generated/services_routes.rs` — the DERIVED `/services/<service>/<op>`
/// axum routes, emitted ONLY for services declaring `expose: true` (ADR-20260719-214500). With no
/// exposed service (the V0 default) the router is empty and state-generic; exposing one is a
/// reviewed spec change that regenerates the typed POST handlers + `ServicesRouterState`.
fn emit_services_routes(model: &Model) -> String {
    let services = parse_services(model);
    let exposed: Vec<&Svc> = services.iter().filter(|s| s.expose).collect();
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/services.yaml — do not edit by hand.\n\
         // The DERIVED `/services/<service>/<op>` surface (ADR-20260719-214500, issue #26): emitted\n\
         // ONLY for services declaring `expose: true`. All service operations are POSTs carrying the\n\
         // JSON call envelope `{ input, meta }`; responses are `{ output }` or the kind-tagged\n\
         // `{ error }` envelope (see infrastructure::generated::service_clients).\n",
    );
    if exposed.is_empty() {
        out.push_str(
            "\n/// No service declares `expose: true` (V0: one deployable, zero internal HTTP) — an empty\n/// router, mergeable into any state. Exposing a service is a reviewed spec change.\npub fn services_router<S: Clone + Send + Sync + 'static>() -> axum::Router<S> {\n    axum::Router::new()\n}\n",
        );
        return out;
    }
    out.push_str(
        "\nuse application::generated::services::*;\nuse axum::extract::State;\nuse axum::response::IntoResponse;\nuse axum::routing::post;\nuse axum::Json;\nuse infrastructure::generated::service_clients::{wire_error_status, WireCall, WireError, WireErrorEnvelope, WireOutput};\nuse std::sync::Arc;\n\n/// The local ports behind the exposed `/services/*` routes.\n#[derive(Clone)]\npub struct ServicesRouterState {\n",
    );
    for svc in &exposed {
        out.push_str(&format!("    pub {}: Arc<dyn {}Service>,\n", svc.name, svc.base));
    }
    out.push_str("}\n\n/// The exposed `/services/*` routes.\npub fn services_router(state: ServicesRouterState) -> axum::Router {\n    axum::Router::new()\n");
    for svc in &exposed {
        for op in &svc.ops {
            out.push_str(&format!(
                "        .route(\"{}\", post({}_{}))\n",
                svc_http_path(&svc.name, &op.name),
                svc.name,
                op.name
            ));
        }
    }
    out.push_str("        .with_state(state)\n}\n");
    for svc in &exposed {
        for op in &svc.ops {
            let input_ty = if op.input.is_some() {
                format!("{}{}Input", svc.base, pascal_snake(&op.name))
            } else {
                "serde_json::Value".to_string()
            };
            let call_args = if op.input.is_some() { "call.input, &call.meta" } else { "&call.meta" };
            out.push_str(&format!(
                "\nasync fn {name}_{op}(\n    State(state): State<ServicesRouterState>,\n    Json(call): Json<WireCall<{input_ty}>>,\n) -> axum::response::Response {{\n    match state.{name}.{op}({call_args}).await {{\n        Ok(output) => Json(WireOutput {{ output }}).into_response(),\n        Err(e) => {{\n            let error = WireError::from(&e);\n            let status = axum::http::StatusCode::from_u16(wire_error_status(&error))\n                .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);\n            (status, Json(WireErrorEnvelope {{ error }})).into_response()\n        }}\n    }}\n}}\n",
                name = svc.name,
                op = op.name,
                input_ty = input_ty,
                call_args = call_args,
            ));
        }
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

// ─── crates/domain/src/generated/lifecycles.rs (aggregate lifecycle tables, ADR-20260720-004419) ──

/// Emit `crates/domain/src/generated/lifecycles.rs` — one module per aggregate declaring a
/// `lifecycle:` block in actors.yaml (ADR-20260720-004419): the status machine as plain data/match
/// (no SDK, no I/O — the domain stays dependency-free). `initial` maps a birth event to its entry
/// state; `transition` is the declared table (`Some(next)` iff legal — `None` = illegal move OR an
/// event outside the machine, a status no-op for the fold); `TERMINAL`/`is_terminal` close it.
fn emit_domain_lifecycles(model: &Model) -> String {
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/actors.yaml `lifecycle` blocks (ADR-20260720-004419)\n// — do not edit by hand. Aggregate lifecycle state machines as plain data/match: the fold and the\n// command handlers consult `transition` so the write side can never disagree with the declared spec.\n",
    );
    for lc in parse_lifecycles(model) {
        let status = ref_name(&lc.status_ref).unwrap_or_default();
        let module = snake_type(&lc.aggregate);
        out.push_str(&format!(
            "\n/// {} lifecycle over [`{}`] (specs/actors.yaml#/{}/lifecycle).\npub mod {} {{\n    use crate::generated::events::DomainEvent;\n    use crate::generated::scalars::{};\n\n",
            lc.aggregate, status, lc.aggregate, module, status
        ));
        let terminal: Vec<String> = lc.terminal.iter().map(|s| format!("{}::{}", status, s)).collect();
        out.push_str(&format!(
            "    /// Terminal states — no outgoing transitions.\n    pub const TERMINAL: &[{}] = &[{}];\n\n",
            status,
            terminal.join(", ")
        ));
        out.push_str(&format!(
            "    /// The state a birth event enters, or `None` when `event` does not birth this lifecycle.\n    pub fn initial(event: &DomainEvent) -> Option<{}> {{\n        match event {{\n",
            status
        ));
        for ini in &lc.initial {
            let ev = ref_name(&ini.event_ref).unwrap_or_default();
            match &ini.via {
                // Event-carried birth state (dynamic, ADR-20260721-093027): the recorded fact wins.
                Some(via) => out.push_str(&format!(
                    "            DomainEvent::{}(e) => Some(e.{}),\n",
                    ev,
                    snake_field(via)
                )),
                None => out.push_str(&format!("            DomainEvent::{}(_) => Some({}::{}),\n", ev, status, ini.to)),
            }
        }
        out.push_str("            _ => None,\n        }\n    }\n\n");
        out.push_str(&format!(
            "    /// The declared transition table: `Some(next)` iff `event` legally moves the machine from\n    /// `from`; `None` = illegal transition, or an event outside the machine (status no-op). A\n    /// dynamic-target arm matches only when the event's carried state equals the declared target.\n    pub fn transition(from: {}, event: &DomainEvent) -> Option<{}> {{\n        match (from, event) {{\n",
            status, status
        ));
        for t in &lc.transitions {
            let ev = ref_name(&t.event_ref).unwrap_or_default();
            for f in &t.from {
                match &t.via {
                    Some(via) => out.push_str(&format!(
                        "            ({}::{}, DomainEvent::{}(e)) if e.{} == {}::{} => Some({}::{}),\n",
                        status,
                        f,
                        ev,
                        snake_field(via),
                        status,
                        t.to,
                        status,
                        t.to
                    )),
                    None => out.push_str(&format!(
                        "            ({}::{}, DomainEvent::{}(_)) => Some({}::{}),\n",
                        status, f, ev, status, t.to
                    )),
                }
            }
        }
        out.push_str("            _ => None,\n        }\n    }\n\n");
        // target: the state an event drives the machine to IRRESPECTIVE of the current state — at
        // fold time the recorded fact wins (legality was enforced at append time by `transition`).
        // A dynamic (`via`) event's target is its carried payload field; a static event is only
        // emitted when it has a single target across all its transitions.
        let mut targets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut dynamic_via: BTreeMap<String, String> = BTreeMap::new();
        for t in &lc.transitions {
            if let Some(ev) = ref_name(&t.event_ref) {
                match &t.via {
                    Some(via) => {
                        dynamic_via.insert(ev, via.clone());
                    }
                    None => {
                        targets.entry(ev).or_default().insert(t.to.clone());
                    }
                }
            }
        }
        out.push_str(&format!(
            "    /// The state `event` drives the machine to, irrespective of the current state — at fold\n    /// time the recorded fact wins (legality was enforced at append time by [`transition`]). `None`\n    /// for an event outside the machine (or whose target depends on the current state). A dynamic\n    /// (event-carried) target is the event's payload field.\n    pub fn target(event: &DomainEvent) -> Option<{}> {{\n        match event {{\n",
            status
        ));
        for t in &lc.transitions {
            let ev = match ref_name(&t.event_ref) {
                Some(e) => e,
                None => continue,
            };
            if let Some(via) = dynamic_via.remove(&ev) {
                // emit each dynamic event once, in declaration order
                out.push_str(&format!("            DomainEvent::{}(e) => Some(e.{}),\n", ev, snake_field(&via)));
                continue;
            }
            if targets.get(&ev).map(|s| s.len()) != Some(1) {
                continue;
            }
            targets.remove(&ev); // emit each single-target event once, in declaration order
            out.push_str(&format!("            DomainEvent::{}(_) => Some({}::{}),\n", ev, status, t.to));
        }
        out.push_str("            _ => None,\n        }\n    }\n\n");
        out.push_str(&format!(
            "    /// Whether `state` is terminal (no outgoing transitions).\n    pub fn is_terminal(state: {}) -> bool {{\n        TERMINAL.contains(&state)\n    }}\n}}\n",
            status
        ));
    }
    out
}

/// Per-aggregate lifecycle state diagrams (mermaid `stateDiagram-v2`) from the declared `lifecycle`
/// blocks — (aggregate → diagram body), in actors.yaml order. Mirrors [`pm_sequence_map`]: callers
/// add their own framing (Markdown fence, HTML `<pre>`), so one source feeds every artifact.
fn lifecycle_state_map(model: &Model) -> Vec<(String, String)> {
    parse_lifecycles(model)
        .into_iter()
        .map(|lc| {
            // A dynamic (event-carried) edge is labelled `Event(field)` so the docs show which
            // payload field names the target state (ADR-20260721-093027).
            let label = |ev: &str, via: &Option<String>| match via {
                Some(v) => format!("{}({})", ev, v),
                None => ev.to_string(),
            };
            let mut lines: Vec<String> = vec!["stateDiagram-v2".into()];
            for ini in &lc.initial {
                lines.push(format!(
                    "  [*] --> {} : {}",
                    ini.to,
                    label(&ref_name(&ini.event_ref).unwrap_or_default(), &ini.via)
                ));
            }
            for t in &lc.transitions {
                let ev = label(&ref_name(&t.event_ref).unwrap_or_default(), &t.via);
                for f in &t.from {
                    lines.push(format!("  {} --> {} : {}", f, t.to, ev));
                }
            }
            for s in &lc.terminal {
                lines.push(format!("  {} --> [*]", s));
            }
            (lc.aggregate, lines.join("\n"))
        })
        .collect()
}

// ─── crates/application/src/generated/handlers.rs (issue #23 — require+guard+append handlers) ────

/// The per-aggregate SEAMS of the generated require+guard+append handlers (ADR-20260721-093027):
/// rehydration (existence + tenant scoping), stream naming and the illegal-move rejection stay
/// hand-written policy in `crates/application/src/commands.rs` (`pub(crate)`), referenced here by
/// expression — error-context construction is per-aggregate policy, not mechanics. Folded into the
/// DSL if a second consumer appears (PM-pipeline precedent, ADR-20260721-053456).
struct LifecycleHandlerSeam {
    aggregate: &'static str,
    /// The domain module whose `lifecycle` re-export holds this aggregate's generated tables.
    lifecycle: &'static str,
    /// Rehydration expression yielding `(state, version)` (awaited with `?`).
    require: &'static str,
    /// Rejection expression for a move the declared machine does not contain.
    reject: &'static str,
    /// Stream-name expression for the append.
    stream: &'static str,
}

const LIFECYCLE_HANDLER_SEAMS: &[LifecycleHandlerSeam] = &[
    LifecycleHandlerSeam {
        aggregate: "Order",
        lifecycle: "domain::order::lifecycle",
        require: "require_order(store, &cmd.order_id, &cmd.restaurant_id)",
        reject: "invalid_order_status(&cmd.order_id, state.status)",
        stream: "order_stream(&cmd.order_id)",
    },
    LifecycleHandlerSeam {
        aggregate: "Rider",
        lifecycle: "domain::rider::lifecycle",
        require: "require_rider(store, &cmd.rider_id)",
        reject: "reject(\"InvalidRiderStatusTransition\", json!({ \"riderId\": cmd.rider_id, \"currentStatus\": state.status, \"targetStatus\": cmd.status }))",
        stream: "rider_stream(&cmd.rider_id)",
    },
    LifecycleHandlerSeam {
        aggregate: "DeliveryJob",
        lifecycle: "domain::delivery_job::lifecycle",
        require: "require_delivery_job(store, &cmd.delivery_job_id)",
        reject: "invalid_delivery_status(&cmd.delivery_job_id, state.status, canonical_predecessor(cmd.status))",
        stream: "delivery_job_stream(&cmd.delivery_job_id)",
    },
];

/// The commands whose WHOLE handler is mechanical require+guard+append — the single emitted event is
/// built from the command by name and legalized by the declared lifecycle table. A command with
/// business checks beyond the machine (`DeliveryAlreadyAssigned` arbitration, rider-identity checks,
/// ensure-command idempotency, cross-aggregate invariants) stays hand-written in commands.rs.
const LIFECYCLE_GENERATED_HANDLERS: &[(&str, &str)] = &[
    ("Order", "AcceptOrder"),
    ("Order", "StartPreparation"),
    ("Order", "MarkOrderReady"),
    ("Order", "MarkOrderDelivered"),
    ("Order", "RejectOrder"),
    ("Order", "CancelOrderByCustomer"),
    ("Order", "CancelOrderByRestaurant"),
    ("Rider", "ChangeRiderStatus"),
    ("DeliveryJob", "UpdateDeliveryStatus"),
    ("DeliveryJob", "UpdateDeliveryPartnerStatus"),
];

/// Emit `crates/application/src/generated/handlers.rs` — one require+guard+append command handler per
/// [`LIFECYCLE_GENERATED_HANDLERS`] row (issue #23, ADR-20260721-093027): rehydrate through the
/// aggregate's seam, build the single emitted event from the command's same-named fields (`None` for
/// an optional field the command does not carry), consult the GENERATED lifecycle transition table,
/// append. Generation FAILS if the actors.yaml wiring stops matching (≠1 emitted event, an event
/// outside the declared machine, or a required event field the command cannot supply).
fn emit_application_handlers(model: &Model) -> String {
    let lifecycles: BTreeSet<String> = parse_lifecycles(model).into_iter().map(|l| l.aggregate).collect();
    let mut commands: BTreeSet<String> = BTreeSet::new();
    let mut events: BTreeSet<String> = BTreeSet::new();
    let mut fns = String::new();
    for (aggregate, command) in LIFECYCLE_GENERATED_HANDLERS {
        let seam = LIFECYCLE_HANDLER_SEAMS
            .iter()
            .find(|s| s.aggregate == *aggregate)
            .unwrap_or_else(|| panic!("handlers: no seam for aggregate '{}'", aggregate));
        if !lifecycles.contains(*aggregate) {
            panic!("handlers: aggregate '{}' declares no lifecycle", aggregate);
        }
        // The command's single emitted event, per the aggregate's receives wiring.
        let receives = model
            .defs
            .get("actors.yaml")
            .and_then(|a| a.get(*aggregate))
            .and_then(|n| n.get("receives"))
            .and_then(|r| r.as_sequence())
            .unwrap_or_else(|| panic!("handlers: actors.yaml#/{} has no receives", aggregate));
        let entry = receives
            .iter()
            .find(|e| {
                e.get("message")
                    .and_then(|m| m.get("$ref"))
                    .and_then(|r| r.as_str())
                    .map(|r| {
                        ref_target_file(r, "actors.yaml").as_deref() == Some("commands.yaml")
                            && ref_name(r).as_deref() == Some(command)
                    })
                    .unwrap_or(false)
            })
            .unwrap_or_else(|| panic!("handlers: actors.yaml#/{} does not receive '{}'", aggregate, command));
        let emits: Vec<String> = ref_strings(entry.get("emits")).iter().filter_map(|r| ref_name(r)).collect();
        let event = match emits.as_slice() {
            [one] => one.clone(),
            other => panic!("handlers: '{}' must emit exactly one event, got {:?}", command, other),
        };
        // Build the event payload from the command's same-named fields.
        let event_node = model
            .defs
            .get("events.yaml")
            .and_then(|e| e.get(event.as_str()))
            .unwrap_or_else(|| panic!("handlers: events.yaml#/{} missing", event));
        let required: BTreeSet<String> = event_node
            .get("required")
            .and_then(|r| r.as_sequence())
            .map(|s| s.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let cmd_props: BTreeSet<String> = model
            .defs
            .get("commands.yaml")
            .and_then(|c| c.get(*command))
            .and_then(|n| n.get("properties"))
            .and_then(|p| p.as_mapping())
            .map(|m| m.keys().filter_map(|k| k.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let mut fields = String::new();
        if let Some(props) = event_node.get("properties").and_then(|p| p.as_mapping()) {
            for (k, _) in props {
                let prop = k.as_str().unwrap_or_default();
                let field = snake_field(prop);
                if cmd_props.contains(prop) {
                    fields.push_str(&format!("        {}: cmd.{},\n", field, field));
                } else if required.contains(prop) {
                    panic!(
                        "handlers: required event field {}.{} has no same-named field on command {}",
                        event, prop, command
                    );
                } else {
                    fields.push_str(&format!("        {}: None,\n", field));
                }
            }
        }
        commands.insert((*command).to_string());
        events.insert(event.clone());
        fns.push_str(&format!(
            "\n/// Handle `commands.yaml#/{cmd}` → emit `events.yaml#/{evt}` — require + guard + append over\n/// the declared machine (`{lc}::transition`); an illegal move rejects through the\n/// aggregate's seam (specs/actors.yaml#/{agg}/lifecycle).\npub async fn {f}(\n    store: &dyn EventStore,\n    cmd: {cmd},\n    actor: &Actor,\n) -> Result<(), DomainError> {{\n    let (state, version) = {require}.await?;\n    let event = DomainEvent::{evt}({evt} {{\n{fields}    }});\n    if {lc}::transition(state.status, &event).is_none() {{\n        return Err({reject});\n    }}\n    Repository::new(store).save(&{stream}, version, &[event], actor).await.map(|_| ())\n}}\n",
            cmd = command,
            evt = event,
            agg = aggregate,
            lc = seam.lifecycle,
            f = snake_type(command),
            require = seam.require,
            reject = seam.reject,
            stream = seam.stream,
            fields = fields,
        ));
    }
    let cmd_list = commands.into_iter().collect::<Vec<_>>().join(", ");
    let evt_list = events.into_iter().collect::<Vec<_>>().join(", ");
    format!(
        "// GENERATED by the Captain.Food codegen from specs/actors.yaml (receives wiring + `lifecycle`\n// blocks), specs/commands.yaml and specs/events.yaml (issue #23, ADR-20260721-093027) — do not edit\n// by hand. The mechanical \"require + guard + append\" command handlers: rehydrate the aggregate\n// through its hand-written seam, build the single emitted event from the command's same-named\n// fields, legalize the move against the GENERATED lifecycle transition table, append.\n// `crates/application/src/commands.rs` re-exports these, so call sites and the behaviour suite (the\n// parity gate until #24's generated harness lands) are unchanged.\n\nuse serde_json::json;\n\nuse domain::generated::commands::{{{cmds}}};\nuse domain::generated::events::{{DomainEvent, {evts}}};\nuse domain::shared::errors::DomainError;\n\nuse crate::commands::{{\n    canonical_predecessor, delivery_job_stream, invalid_delivery_status, invalid_order_status,\n    order_stream, reject, require_delivery_job, require_order, require_rider, rider_stream,\n}};\nuse crate::ports::{{Actor, EventStore}};\nuse crate::repository::Repository;\n{fns}",
        cmds = cmd_list,
        evts = evt_list,
        fns = fns,
    )
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
        "// GENERATED by the Captain.Food codegen from specs/api.yaml + specs/entities.yaml — do not edit by hand.\n// GraphQL output types (async-graphql SimpleObject), mirroring the generated SDL: entities.yaml types\n// not registered as api.yaml projections, then the api.yaml types, each with its FK-derived navigation\n// fields (plain data fields for now — resolved empty until the read resolvers land).\n#![allow(dead_code)]\n#![allow(non_camel_case_types)]\n\nuse application::projections::{CartRow, CatalogRow, CustomerRow, OrderTrackingRow, ProspectionPipelineRow, RestaurantRow};\nuse application::queries::{DeliveryJobRow, DeliveryPartnerAvailabilityRow, DeliverySatisfactionRow, PricingPolicyRow, RefundRow, UberEstimationPolicyRow, UberSplitPolicyRow};\nuse domain::generated::scalars as ds;\n\nuse super::scalars::*;\n",
    );
    let nav_roles: HashMap<String, HashMap<String, Vec<String>>> = api
        .types
        .iter()
        .map(|t| (t.name.clone(), t.nav_roles.iter().cloned().collect()))
        .collect();
    let push_nav = |out: &mut String, name: &str| {
        if let Some(nfs) = nav.get(name) {
            for n in nfs {
                let base = if n.list { format!("Vec<{}>", gql_rust_name(&n.target)) } else { gql_rust_name(&n.target) };
                let roles = nav_roles.get(name).and_then(|m| m.get(&n.field));
                match roles.and_then(|r| acl_role_set(model, r)) {
                    // Guarded nav edge (#22): the operations' guard/visible pair, fully qualified so
                    // types.rs needs no acl import.
                    Some(set) => {
                        let ident = acl_set_ident(&set);
                        let ty = if n.list || !n.nullable { base.clone() } else { format!("Option<{}>", base) };
                        out.push_str(&format!(
                            "    #[graphql(name = \"{}\", guard = \"super::acl::RoleGuard::new(super::acl::ALLOW_{})\", visible = \"super::acl::visible_{}\")]\n",
                            n.field,
                            ident.to_uppercase(),
                            ident
                        ));
                        if ty.starts_with("Vec<") {
                            out.push_str("    #[serde(default)]\n");
                        }
                        out.push_str(&format!("    pub {}: {},\n", rust_ident(&snake_field(&n.field)), ty));
                    }
                    None => push_gql_field(out, &n.field, &base, n.list || !n.nullable, None),
                }
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
        "\n/// Read-model rows → API type: the OrderTracking row plus the joined Restaurant row (non-null\n/// `restaurant` navigation field). The breakdown's `restaurantContribution` is re-derived as\n/// articles − restaurantPayout (the projection stores the split's leaves); the Uber comparison is\n/// rebuilt only when every `uber_*` column is present; `paymentStatus` is folded as TEXT by the\n/// projector and parsed leniently (unknown → PENDING); nav `deliveryJobs` resolve empty until that\n/// read model lands.\nimpl From<(OrderTrackingRow, RestaurantRow)> for Order {\n    fn from((row, restaurant): (OrderTrackingRow, RestaurantRow)) -> Self {\n        let currency = row.currency.clone();\n        let breakdown = PaymentBreakdown {\n            articles: order_money(row.articles_cents.clone(), &currency),\n            delivery: order_money(row.delivery_cents.clone(), &currency),\n            service_fee: order_money(row.service_fee_cents.clone(), &currency),\n            total: order_money(row.total_amount_cents.clone(), &currency),\n            restaurant_contribution: order_money(\n                ds::MoneyCents(row.articles_cents.0 - row.restaurant_payout_cents.0),\n                &currency,\n            ),\n            restaurant_payout: order_money(row.restaurant_payout_cents.clone(), &currency),\n            rider_payout: order_money(row.rider_payout_cents.clone(), &currency),\n            captain_net: order_money(row.captain_net_cents.clone(), &currency),\n        };\n        let uber_comparison = match (\n            row.uber_total_cents,\n            row.uber_restaurant_cents,\n            row.uber_rider_cents,\n            row.uber_platform_cents,\n            row.uber_basis,\n        ) {\n            (Some(total), Some(restaurant_share), Some(rider_share), Some(platform_share), Some(basis)) => {\n                Some(UberComparison {\n                    total: order_money(total, &currency),\n                    restaurant_share: order_money(restaurant_share, &currency),\n                    rider_share: order_money(rider_share, &currency),\n                    platform_share: order_money(platform_share, &currency),\n                    basis: basis.into(),\n                })\n            }\n            _ => None,\n        };\n        Self {\n            id: row.order_id.into(),\n            r#ref: row.r#ref.into(),\n            restaurant_id: row.restaurant_id.into(),\n            customer_id: row.customer_id.map(Into::into),\n            status: row.status.into(),\n            service_type: row.service_type.into(),\n            items: serde_json::from_value(row.items).unwrap_or_default(),\n            total_amount: order_money(row.total_amount_cents, &currency),\n            breakdown,\n            delivery_address: row.delivery_address.and_then(|v| serde_json::from_value(v).ok()),\n            estimated_ready_at: row.estimated_ready_at,\n            placed_at: row.placed_at,\n            status_changed_at: row.status_changed_at,\n            payment_status: match row.payment_status.as_str() {\n                \"CAPTURED\" => PaymentStatus::CAPTURED,\n                \"FAILED\" => PaymentStatus::FAILED,\n                \"REFUNDED\" => PaymentStatus::REFUNDED,\n                _ => PaymentStatus::PENDING,\n            },\n            restaurant_stars: row.restaurant_stars.map(Into::into),\n            rating_comment: row.rating_comment.map(Into::into),\n            rider_thumb: row.rider_thumb.map(Into::into),\n            delivery_timeliness: row.delivery_timeliness.map(Into::into),\n            rider_tip: row.rider_tip_cents.map(|c| order_money(c, &currency)),\n            restaurant_tip: row.restaurant_tip_cents.map(|c| order_money(c, &currency)),\n            captain_tip: row.captain_tip_cents.map(|c| order_money(c, &currency)),\n            uber_comparison,\n            delivery_status: row.delivery_status.map(Into::into),\n            courier: row.courier.and_then(|v| serde_json::from_value(v).ok()),\n            estimated_dropoff_at: row.estimated_dropoff_at,\n            rated_at: row.rated_at,\n            delivery_jobs: Vec::new(),\n            restaurant: restaurant.into(),\n        }\n    }\n}\n",
    );
    // DeliveryJob: the View_DeliveryJob fold-view row (hand-written DTO — view-backed read models get
    // no generated row); both nav fields are NON-NULL, so the mapping takes the joined OrderTracking +
    // Restaurant rows (the resolver performs the joins).
    out.push_str(
        "\n/// Read-model rows → API type: the `View_DeliveryJob` row (ADR-0031/0039) plus the joined\n/// OrderTracking + Restaurant rows (the FK-derived `order`/`restaurant` navigation fields are\n/// non-null, so the resolver hydrates them — all three are projections of the same domain log).\n/// Addresses and the courier deserialize out of the view's jsonb columns.\nimpl From<(DeliveryJobRow, OrderTrackingRow, RestaurantRow)> for DeliveryJob {\n    fn from((row, order, restaurant): (DeliveryJobRow, OrderTrackingRow, RestaurantRow)) -> Self {\n        Self {\n            id: row.delivery_job_id.into(),\n            order_id: row.order_id.into(),\n            restaurant_id: row.restaurant_id.into(),\n            status: row.status.into(),\n            provider: row.provider.map(Into::into),\n            courier: row.courier.and_then(|v| serde_json::from_value(v).ok()),\n            pickup_address: serde_json::from_value(row.pickup_address)\n                .expect(\"DeliveryJob.pickupAddress: invalid jsonb\"),\n            dropoff_address: serde_json::from_value(row.dropoff_address)\n                .expect(\"DeliveryJob.dropoffAddress: invalid jsonb\"),\n            estimated_pickup_at: row.estimated_pickup_at,\n            estimated_dropoff_at: row.estimated_dropoff_at,\n            requested_at: row.requested_at,\n            picked_up_at: row.picked_up_at,\n            delivered_at: row.delivered_at,\n            order: (order, restaurant.clone()).into(),\n            restaurant: restaurant.into(),\n        }\n    }\n}\n",
    );
    // Refund: the View_PendingRefunds fold-view row (hand-written DTO — view-backed read models get
    // no generated row). Minor-units columns + the row currency rebuild the Money values, like Order.
    out.push_str(
        "\n/// Read-model row → API type: the `View_PendingRefunds` fold-view row (the refund queue —\n/// RefundOpened/RefundApproved/RefundDenied/PaymentRefunded folded on the Payment stream). The\n/// minor-units columns + the row currency rebuild the Money values (`approvedAmount` only once a\n/// possibly-partial approval is recorded).\nimpl From<RefundRow> for Refund {\n    fn from(row: RefundRow) -> Self {\n        let currency = row.currency.clone();\n        Self {\n            order_id: row.order_id.into(),\n            restaurant_id: row.restaurant_id.into(),\n            status: row.status.into(),\n            amount: order_money(row.amount_cents, &currency),\n            approved_amount: row.approved_amount_cents.map(|c| order_money(c, &currency)),\n            reason: row.reason,\n            refund_id: row.refund_id.map(Into::into),\n            requested_at: row.requested_at,\n            decided_at: row.decided_at,\n        }\n    }\n}\n",
    );
    // DeliverySatisfaction: the View_DeliverySatisfaction fold-view row (#62; hand-written DTO). Rows
    // map 1:1 — no navigation fields (the survey view carries no FK edges), so no joins.
    out.push_str(
        "\n/// Read-model row → API type: the `View_DeliverySatisfaction` fold-view row (#62) — one\n/// customer delivery-delay answer (`DeliverySatisfactionRecorded` folded on the Order stream). Rows\n/// map 1:1 (no navigation fields), so no joins.\nimpl From<DeliverySatisfactionRow> for DeliverySatisfaction {\n    fn from(row: DeliverySatisfactionRow) -> Self {\n        Self {\n            order_id: row.order_id.into(),\n            restaurant_id: row.restaurant_id.into(),\n            timeliness: row.timeliness.into(),\n            reason: row.reason.map(Into::into),\n            recorded_at: row.recorded_at,\n        }\n    }\n}\n",
    );
    // DeliveryPartnerAvailability (#61): the `View_DeliveryPartnerAvailability` fold-view row. Rows map
    // 1:1 — set-once identity from the Requested birth fact, status derived, decided_at null while PENDING.
    out.push_str(
        "\n/// Read-model row → API type: the `View_DeliveryPartnerAvailability` fold-view row (delivery partner\n/// self-registration, #61 — Requested/Approved/Revoked folded on the DeliveryPartnerRegistration stream).\nimpl From<DeliveryPartnerAvailabilityRow> for DeliveryPartnerAvailability {\n    fn from(row: DeliveryPartnerAvailabilityRow) -> Self {\n        Self {\n            registration_id: row.registration_id.into(),\n            channel: row.channel.into(),\n            city_id: row.city_id.into(),\n            partner_name: row.partner_name.into(),\n            contact_email: row.contact_email.into(),\n            status: row.status.into(),\n            requested_at: row.requested_at,\n            decided_at: row.decided_at,\n        }\n    }\n}\n",
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

    // Generator-injected inputs (api.yaml `inputs:` — MetadataInput, ADR-20260720-015500).
    for (name, fields) in &api.inputs {
        push_gql_struct_open(&mut out, name, "InputObject", None);
        for f in fields {
            let base = rust_api_field_base(model, f, true);
            push_gql_field(&mut out, &f.name, &base, f.required, f.description.as_deref());
        }
        out.push_str("}\n");
    }
    out
}

/// The api.yaml role name → the server's `RequestRole` variant (`RESTAURANT_ACCOUNT` →
/// `RestaurantAccount`).
fn acl_role_variant(role: &str) -> String {
    role.split('_').map(|seg| pascal(&seg.to_lowercase())).collect()
}

/// An operation's allowed-role set in canonical `scalars.yaml#/UserType` declaration order, or `None`
/// when the operation is open to everyone (`roles` OMITTED — ADR-20260720-191500 literal lists; a
/// present list is guarded verbatim, PUBLIC in it being just the anonymous path).
fn acl_role_set(model: &Model, roles: &[String]) -> Option<Vec<String>> {
    if roles.is_empty() {
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
        "// GENERATED by the Captain.Food codegen from specs/api.yaml — do not edit by hand.\n// Per-operation ACL role sets (ADR-0006 role-as-path): each distinct non-public `roles:` set on an\n// api.yaml query/mutation/subscription becomes an allowed-role const + a `visible` fn. The generated\n// QueryRoot/MutationRoot/SubscriptionRoot fields wire them as `guard = \"RoleGuard::new(ALLOW_…)\"`\n// (execution — unauthorized roles get a FORBIDDEN error) and `visible = \"visible_…\"` (introspection —\n// the field is hidden from unauthorized roles, and async-graphql's `find_visible_types` then hides\n// every type reachable only through hidden fields, so per-role introspection/Voyager expose only that\n// role's surface). Operations with `roles:` OMITTED carry no guard/visible: open to every role path\n// (LITERAL roles, ADR-20260720-191500 — PUBLIC in a list is just the anonymous path).\n#![allow(dead_code)]\n\npub(crate) use super::super::acl::RoleGuard;\nuse super::super::acl::{role_allows, RequestRole};\n",
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
    // Guarded FK-derived nav edges (#22) share the same const/visible pairs.
    for t in &api.types {
        for (_field, roles) in &t.nav_roles {
            if let Some(set) = acl_role_set(model, roles) {
                sets.insert(acl_set_ident(&set), set);
            }
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
        // The journaled-command status poll (ADR-20260720-015500): PUBLIC, ownership-scoped —
        // a non-owned/unknown messageId resolves null (no existence oracle).
        "operationStatus" => Some(
            "        let journal = ctx.data::<std::sync::Arc<dyn application::journal::CommandJournal>>()?;\n        let Some(row) = journal\n            .by_message(input.message_id.0)\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n        else {\n            return Ok(None);\n        };\n        if !super::mutation::operation_owned(ctx, &row) {\n            return Ok(None);\n        }\n        Ok(Some(super::mutation::operation_from_journal(&row)))",
        ),
        // The checkout payment state (ADR-20260720-015500): served from the PlaceOrderProcess run
        // row (the declared PM-privacy exception); initiator-scoped — ADMIN, the checkout's
        // customer (JWT subject → Customer row), or the checkout's session.
        "paymentStatus" => Some(
            "        let pm = ctx.data::<std::sync::Arc<dyn application::pm_state::PaymentProcessStateStore>>()?;\n        let Some(row) = pm\n            .by_order(input.order_id.into())\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?\n        else {\n            return Ok(None);\n        };\n        let admin = matches!(\n            ctx.data_opt::<crate::graphql::acl::RequestRole>(),\n            Some(crate::graphql::acl::RequestRole::Admin)\n        );\n        let session = ctx.data_opt::<crate::graphql::session::SessionHeader>().and_then(|s| s.0);\n        let session_owned = session.is_some() && session == row.session_id.as_ref().map(|s| s.0);\n        let mut customer_owned = false;\n        if let (Some(auth_ref), Some(row_customer)) = (\n            ctx.data_opt::<crate::auth::Principal>().and_then(|p| p.user_id.clone()),\n            row.customer_id.as_ref(),\n        ) {\n            let customers = ctx.data::<std::sync::Arc<dyn application::queries::CustomerReadRepository>>()?;\n            customer_owned = customers\n                .by_auth_ref(domain::generated::scalars::ExternalReference(auth_ref))\n                .await\n                .map_err(|e| async_graphql::Error::new(e.to_string()))?\n                .is_some_and(|c| c.customer_id == *row_customer);\n        }\n        if !(admin || customer_owned || session_owned) {\n            return Ok(None);\n        }\n        Ok(Some(PaymentIntent {\n            payment_intent_id: row.payment_intent_id.into(),\n            client_secret: row.client_secret,\n            status: row.payment_status.into(),\n        }))",
        ),
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
        // The refund queue reads the View_PendingRefunds fold view (RefundProcess). Rows map 1:1 —
        // no navigation fields (the Payment aggregate is not a registered API type), so no joins.
        "pendingRefunds" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::RefundReadRepository>>()?;\n        let filter = input\n            .map(|i| application::queries::RefundFilter {\n                restaurant_id: i.restaurant_id.map(Into::into),\n                status: i.status.map(Into::into),\n            })\n            .unwrap_or_default();\n        let rows = repo.list(filter).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(Refund::from).collect())",
        ),
        // The restaurant timeliness insight reads the View_DeliverySatisfaction fold view (#62). Rows
        // map 1:1 — no navigation fields, so no joins.
        "restaurantDeliverySatisfaction" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::DeliverySatisfactionReadRepository>>()?;\n        let rows = repo\n            .by_restaurant(input.restaurant_id.into(), input.timeliness.map(Into::into))\n            .await\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(DeliverySatisfaction::from).collect())",
        ),
        // Delivery-partner self-registration (#61): the EXTERNAL/admin review queue reads the
        // View_DeliveryPartnerAvailability fold view. Rows map 1:1 — no navigation joins.
        "deliveryPartnerAvailabilities" => Some(
            "        let repo = ctx.data::<std::sync::Arc<dyn application::queries::DeliveryPartnerAvailabilityReadRepository>>()?;\n        let filter = input\n            .map(|i| application::queries::DeliveryPartnerAvailabilityFilter {\n                city_id: i.city_id.map(Into::into),\n                channel: i.channel.map(Into::into),\n                status: i.status.map(Into::into),\n            })\n            .unwrap_or_default();\n        let rows = repo.list(filter).await.map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        Ok(rows.into_iter().map(DeliveryPartnerAvailability::from).collect())",
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
/// `mutation_block`: one async resolver per api.yaml mutation, ACCEPTANCE-FIRST
/// (ADR-20260720-015500): the resolver journals the command (durable RECEIVED row, idempotent by
/// messageId + payload hash), spawns the handler over Arc-cloned ports, and returns the uniform
/// `MutationAcceptance` immediately; the spawned task completes the journal row and publishes the
/// transition on the `OperationStatusBus` for `operationStatus`/`operationStatusChanged`.
fn emit_server_mutation(model: &Model) -> String {
    let api = parse_api(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/api.yaml — do not edit by hand.\n// The GraphQL MutationRoot, ACCEPTANCE-FIRST (ADR-20260720-015500): one resolver per api.yaml\n// mutation, `(input: <Command>Input!, metadata: MetadataInput) -> MutationAcceptance!`. The resolver\n// journals the command into `command_journal` (durable RECEIVED, idempotent by messageId — same\n// payload hash replays the original acceptance, a different one is a Conflict), spawns the command\n// handler on Arc-cloned ports, and answers with the effective envelope + PENDING. The spawned task\n// completes the journal row (SUCCEEDED | REJECTED | FAILED) and publishes the transition on the\n// OperationStatusBus; post-acceptance rejections surface as Operation.errorCode, never as GraphQL\n// errors (the sync path — input/metadata validation, duplicate-payload Conflict — still uses them).\n// Each non-public field carries its api.yaml `roles` as a `guard` + `visible` pair (ADR-0006).\n#![allow(unused_variables)]\n#![allow(dead_code)]\n\nuse super::acl::*;\nuse super::inputs::*;\nuse super::scalars::*;\nuse super::types::*;\n",
    );
    out.push_str("\npub struct MutationRoot;\n\n#[async_graphql::Object(name = \"Mutation\")]\nimpl MutationRoot {\n");
    for m in &api.mutations {
        let fnname = rust_ident(&snake_field(&m.name));
        let acl = acl_field_attr(model, &m.roles);
        push_doc(&mut out, "    ", m.description.as_deref());
        match wired_mutation_dispatch(&m.name) {
            // Wired: journal → spawn the command handler over Arc-cloned ports → acceptance.
            Some((resolve_ports, handler_call)) => out.push_str(&format!(
                "    #[graphql(name = \"{name}\"{acl})]\n    async fn {fnname}(&self, ctx: &async_graphql::Context<'_>, input: {command}Input, metadata: Option<MetadataInput>) -> async_graphql::Result<MutationAcceptance> {{\n        let journal = ctx.data::<std::sync::Arc<dyn application::journal::CommandJournal>>()?.clone();\n        let status_bus = ctx.data::<infrastructure::OperationStatusBus>()?.clone();\n{resolve_ports}        let payload_json = command_payload(&input)?;\n        let cmd: domain::generated::commands::{command} = serde_json::from_value(payload_json.clone())\n            .map_err(|e| async_graphql::Error::new(e.to_string()))?;\n        let env = request_envelope(ctx, &metadata);\n        let entry = application::journal::CommandJournalEntry {{\n            message_id: env.message_id,\n            correlation_id: env.correlation_id,\n            cause_id: env.cause_id,\n            session_id: env.session_id,\n            trace_id: env.trace_id.clone(),\n            user_id: env.user_id,\n            user_type: env.user_type,\n            channel: domain::generated::scalars::CommandChannel::GRAPHQL,\n            command_type: \"{command}\".into(),\n            payload_hash: application::journal::payload_hash(&payload_json),\n            payload: payload_json,\n        }};\n        match journal.insert(&entry).await.map_err(domain_error)? {{\n            application::journal::JournalInsertOutcome::Duplicate {{ status, payload_hash }} => {{\n                if payload_hash != entry.payload_hash {{\n                    return Err(conflict_error(env.message_id));\n                }}\n                return Ok(acceptance(&env, journal_status_api(status), true));\n            }}\n            application::journal::JournalInsertOutcome::Inserted => {{}}\n        }}\n        // Envelope → Actor (ADR-0041): events appended by this command carry cause_id = messageId.\n        let actor = application::ports::Actor {{\n            user_id: env.user_id.unwrap_or_else(uuid::Uuid::nil),\n            user_type: env.user_type,\n            correlation_id: env.correlation_id,\n            cause_id: Some(env.message_id),\n        }};\n        let (message_id, correlation_id) = (env.message_id, env.correlation_id);\n        tokio::spawn(async move {{\n            let outcome = {handler_call};\n            complete_operation(journal, status_bus, message_id, correlation_id, outcome).await;\n        }});\n        Ok(acceptance(&env, OperationStatus::PENDING, false))\n    }}\n",
                name = m.name, acl = acl, fnname = fnname, command = m.command,
                resolve_ports = resolve_ports, handler_call = handler_call
            )),
            None => out.push_str(&format!(
                "    #[graphql(name = \"{}\"{})]\n    async fn {}(&self, input: {}Input, metadata: Option<MetadataInput>) -> async_graphql::Result<MutationAcceptance> {{\n        Err(async_graphql::Error::new(\"not implemented\"))\n    }}\n",
                m.name, acl, fnname, m.command
            )),
        }
    }
    out.push_str("}\n");
    // Shared write-side plumbing for the wired resolvers.
    out.push_str(
        "\n/// The stripped serde wire shape of the GraphQL input — both the journal `payload` column and the\n/// domain command deserialize from it (generated from the same commands.yaml, camelCase). `null`s\n/// are stripped first — an unset GraphQL optional serializes as an explicit null, while the domain\n/// payloads model absence as a MISSING key (`Option` fields / `#[serde(default)]` arrays).\nfn command_payload(input: &impl serde::Serialize) -> async_graphql::Result<serde_json::Value> {\n    let mut value = serde_json::to_value(input).map_err(|e| async_graphql::Error::new(e.to_string()))?;\n    strip_nulls(&mut value);\n    Ok(value)\n}\n\nfn strip_nulls(value: &mut serde_json::Value) {\n    match value {\n        serde_json::Value::Object(map) => {\n            map.retain(|_, v| !v.is_null());\n            for v in map.values_mut() {\n                strip_nulls(v);\n            }\n        }\n        serde_json::Value::Array(items) => {\n            for v in items.iter_mut() {\n                strip_nulls(v);\n            }\n        }\n        _ => {}\n    }\n}\n\n/// `RequestRole` → the scalars.yaml UserType declaration-order ordinal (ADR-0037).\nfn role_ordinal(role: &crate::graphql::acl::RequestRole) -> i32 {\n    use crate::graphql::acl::RequestRole as R;\n    match role {\n        R::Public => 0,\n        R::Customer => 1,\n        R::RestaurantAccount => 2,\n        R::Restaurant => 3,\n        R::Rider => 4,\n        R::Admin => 5,\n        R::External => 6,\n    }\n}\n\n/// The EFFECTIVE technical envelope of one mutation request (ADR-20260720-015500): what the client\n/// supplied via MetadataInput/headers, completed server-side (UUIDv7) and echoed back verbatim in\n/// the MutationAcceptance.\npub(crate) struct RequestEnvelope {\n    pub message_id: uuid::Uuid,\n    pub correlation_id: uuid::Uuid,\n    pub cause_id: Option<uuid::Uuid>,\n    pub session_id: Option<uuid::Uuid>,\n    pub trace_id: Option<String>,\n    pub user_id: Option<uuid::Uuid>,\n    pub user_type: i32,\n}\n\nfn request_envelope(ctx: &async_graphql::Context<'_>, metadata: &Option<MetadataInput>) -> RequestEnvelope {\n    let principal = ctx.data_opt::<crate::auth::Principal>();\n    let user_id = principal\n        .and_then(|p| p.user_id.as_deref())\n        .and_then(|s| uuid::Uuid::parse_str(s).ok());\n    let user_type = principal.map(|p| role_ordinal(&p.role)).unwrap_or(0);\n    let session_id = ctx.data_opt::<crate::graphql::session::SessionHeader>().and_then(|s| s.0);\n    let trace_id = ctx.data_opt::<crate::graphql::session::TraceContext>().and_then(|t| t.0.clone());\n    // Client-suppliable ids validate structurally at scalar parse time; anything missing is\n    // server-generated (time-ordered UUIDv7) and the correlation defaults to the messageId.\n    let message_id = metadata\n        .as_ref()\n        .and_then(|m| m.message_id.as_ref())\n        .map(|v| v.0)\n        .unwrap_or_else(uuid::Uuid::now_v7);\n    let correlation_id = metadata\n        .as_ref()\n        .and_then(|m| m.correlation_id.as_ref())\n        .map(|v| v.0)\n        .unwrap_or(message_id);\n    let cause_id = metadata.as_ref().and_then(|m| m.cause_id.as_ref()).map(|v| v.0);\n    RequestEnvelope { message_id, correlation_id, cause_id, session_id, trace_id, user_id, user_type }\n}\n\n/// The uniform acceptance payload from the effective envelope.\nfn acceptance(env: &RequestEnvelope, status: OperationStatus, duplicate: bool) -> MutationAcceptance {\n    MutationAcceptance {\n        message_id: MessageId(env.message_id),\n        correlation_id: CorrelationId(env.correlation_id),\n        cause_id: env.cause_id.map(CauseId),\n        session_id: env.session_id.map(SessionId),\n        trace_id: env.trace_id.clone().map(TraceId),\n        operation_status: status,\n        duplicate,\n    }\n}\n\n/// `command_journal` lifecycle → the caller-facing OperationStatus (RECEIVED reads as PENDING).\npub(crate) fn journal_status_api(s: domain::generated::scalars::CommandJournalStatus) -> OperationStatus {\n    use domain::generated::scalars::CommandJournalStatus as J;\n    match s {\n        J::RECEIVED => OperationStatus::PENDING,\n        J::SUCCEEDED => OperationStatus::SUCCEEDED,\n        J::REJECTED => OperationStatus::REJECTED,\n        J::FAILED => OperationStatus::FAILED,\n    }\n}\n\n/// A `command_journal` row → the API Operation shape (`operationStatus` / `operationStatusChanged`).\npub(crate) fn operation_from_journal(row: &application::journal::CommandJournalRow) -> Operation {\n    let error_code = row\n        .error\n        .as_ref()\n        .and_then(|e| e.get(\"code\"))\n        .and_then(|c| c.as_str())\n        .map(str::to_owned);\n    let message = match (&error_code, row.error.as_ref().and_then(|e| e.get(\"context\"))) {\n        (Some(code), Some(context)) => domain::generated::errors::message_en(code, context),\n        _ => None,\n    };\n    Operation {\n        message_id: MessageId(row.entry.message_id),\n        correlation_id: CorrelationId(row.entry.correlation_id),\n        status: journal_status_api(row.status),\n        error_code,\n        message,\n        occurred_at: row.completed_at.unwrap_or(row.received_at),\n    }\n}\n\n/// The operation ownership scope (ADR-20260720-015500): ADMIN, the journaling actor (JWT subject),\n/// or the journaling session (X-SESSION-ID). Callers resolve null / an empty stream on false — the\n/// PUBLIC surface must not become an existence oracle.\npub(crate) fn operation_owned(\n    ctx: &async_graphql::Context<'_>,\n    row: &application::journal::CommandJournalRow,\n) -> bool {\n    let admin = matches!(\n        ctx.data_opt::<crate::graphql::acl::RequestRole>(),\n        Some(crate::graphql::acl::RequestRole::Admin)\n    );\n    let principal_uuid = ctx\n        .data_opt::<crate::auth::Principal>()\n        .and_then(|p| p.user_id.as_deref())\n        .and_then(|s| uuid::Uuid::parse_str(s).ok());\n    let session = ctx.data_opt::<crate::graphql::session::SessionHeader>().and_then(|s| s.0);\n    admin\n        || (principal_uuid.is_some() && principal_uuid == row.entry.user_id)\n        || (session.is_some() && session == row.entry.session_id)\n}\n\n/// The spawned handler's terminal transition: complete the journal row and publish the update.\n/// REJECTED = an anticipated errors.yaml rejection (surfaced as Operation.errorCode); FAILED = the\n/// catalogued generic Internal (adapter detail never leaks).\nasync fn complete_operation(\n    journal: std::sync::Arc<dyn application::journal::CommandJournal>,\n    bus: infrastructure::OperationStatusBus,\n    message_id: uuid::Uuid,\n    correlation_id: uuid::Uuid,\n    outcome: Result<(), domain::shared::errors::DomainError>,\n) {\n    use domain::generated::scalars::CommandJournalStatus as J;\n    use domain::shared::errors::DomainError;\n    let (status, error, error_code, message) = match outcome {\n        Ok(()) => (J::SUCCEEDED, None, None, None),\n        Err(DomainError::Rejected { code, context }) => {\n            let msg = domain::generated::errors::message_en(&code, &context).unwrap_or_else(|| code.clone());\n            let error = serde_json::json!({ \"code\": code, \"context\": context });\n            (J::REJECTED, Some(error), Some(code), Some(msg))\n        }\n        // Legacy \"<Code>: <detail>\" string invariants (interim adapters): a catalogued prefix is a\n        // rejection; anything else — and every Repository failure — is a technical failure.\n        Err(DomainError::Invariant(msg)) => {\n            let code = msg.split(':').next().map(str::trim).unwrap_or(\"\").to_string();\n            if domain::generated::errors::find(&code).is_some() {\n                let error = serde_json::json!({ \"code\": code, \"context\": { \"detail\": msg } });\n                (J::REJECTED, Some(error), Some(code), Some(msg))\n            } else {\n                internal_completion()\n            }\n        }\n        Err(DomainError::Repository(_)) => internal_completion(),\n    };\n    if let Err(e) = journal.complete(message_id, status, error).await {\n        eprintln!(\"command journal: complete({message_id}) failed: {e}\");\n    }\n    bus.publish(infrastructure::OperationUpdate { message_id, correlation_id, status, error_code, message });\n}\n\nfn internal_completion() -> (\n    domain::generated::scalars::CommandJournalStatus,\n    Option<serde_json::Value>,\n    Option<String>,\n    Option<String>,\n) {\n    let def = domain::generated::errors::INTERNAL;\n    (\n        domain::generated::scalars::CommandJournalStatus::FAILED,\n        Some(serde_json::json!({ \"code\": def.code, \"context\": {} })),\n        Some(def.code.to_string()),\n        Some(def.message_en.to_string()),\n    )\n}\n\n/// The synchronous Conflict for a replayed messageId whose payload differs — a client bug, not a\n/// retry (ADR-20260720-015300); errors.yaml cross-cutting `Conflict`, P-10 extensions shape.\nfn conflict_error(message_id: uuid::Uuid) -> async_graphql::Error {\n    use async_graphql::ErrorExtensions;\n    let def = domain::generated::errors::CONFLICT;\n    async_graphql::Error::new(format!(\n        \"messageId {message_id} was already used with a different payload\"\n    ))\n    .extend_with(|_, ext| ext.set(\"code\", def.code))\n}\n\n/// Map a SYNCHRONOUS failure (journal insert, input deserialization) onto the GraphQL error\n/// contract (P-10): an anticipated errors.yaml rejection surfaces `extensions.code` = the stable\n/// PascalCase code, the interpolated English message as the error message, and its typed context\n/// fields under the extensions; anything unexpected (repository/adapter failures) surfaces as the\n/// generic catalogued `Internal` — never leaking adapter details to the client.\nfn domain_error(e: domain::shared::errors::DomainError) -> async_graphql::Error {\n",
    );
    out.push_str(
        "    use async_graphql::ErrorExtensions;\n    use domain::shared::errors::DomainError;\n    match e {\n        DomainError::Rejected { code, context } => {\n            let message = domain::generated::errors::message_en(&code, &context)\n                .unwrap_or_else(|| code.clone());\n            async_graphql::Error::new(message).extend_with(|_, ext| {\n                ext.set(\"code\", code.as_str());\n                if let Some(fields) = context.as_object() {\n                    for (key, value) in fields {\n                        if key == \"code\" {\n                            continue; // never let a context field shadow the wire code\n                        }\n                        ext.set(\n                            key.as_str(),\n                            async_graphql::Value::from_json(value.clone())\n                                .unwrap_or(async_graphql::Value::Null),\n                        );\n                    }\n                }\n            })\n        }\n        // Legacy \"<Code>: <detail>\" string invariants (interim adapters, e.g. the fail-closed\n        // payment stand-in): surface the prefix when it is a catalogued code, else it is unexpected.\n        DomainError::Invariant(msg) => {\n            let code = msg.split(':').next().map(str::trim).unwrap_or(\"\").to_string();\n            if domain::generated::errors::find(&code).is_some() {\n                async_graphql::Error::new(msg).extend_with(|_, ext| ext.set(\"code\", code.as_str()))\n            } else {\n                internal_error()\n            }\n        }\n        DomainError::Repository(_) => internal_error(),\n    }\n}\n\n/// The generic catalogued `Internal` fallback (errors.yaml): unexpected/infrastructure failures\n/// never leak their detail to the client.\nfn internal_error() -> async_graphql::Error {\n    use async_graphql::ErrorExtensions;\n    let def = domain::generated::errors::INTERNAL;\n    async_graphql::Error::new(def.message_en).extend_with(|_, ext| ext.set(\"code\", def.code))\n}\n",
    );
    out
}

/// Dispatch fragments for mutations whose command handler is wired: `(resolve_ports, handler_call)`.
/// `resolve_ports` Arc-CLONES every port the handler needs out of ctx.data (the spawned task owns
/// them); `handler_call` is the awaited expression run INSIDE the task, normalized to
/// `Result<(), DomainError>` (business return values are discarded — acceptance-first results are
/// reads, ADR-20260720-015500). `None` → the `not implemented` stub.
fn wired_mutation_dispatch(name: &str) -> Option<(String, String)> {
    // verifyPhone resolves through the wrapped auth ACL (ADR-0015): its VerifyPhoneOutcome
    // (customerId/created) is no longer returned — the client reads `me` once the operation
    // SUCCEEDs.
    if name == "verifyPhone" {
        return Some((
            "        let store = ctx.data::<std::sync::Arc<dyn application::ports::EventStore>>()?.clone();\n        let auth = ctx.data::<std::sync::Arc<dyn application::generated::services::IdentityService>>()?.clone();\n        let customers = ctx.data::<std::sync::Arc<dyn application::queries::CustomerReadRepository>>()?.clone();\n".into(),
            "application::commands::verify_phone(store.as_ref(), auth.as_ref(), customers.as_ref(), cmd, &actor).await.map(|_| ())".into(),
        ));
    }
    // placeOrder needs the CatalogReadRepository (server-side line pricing from the live catalog —
    // the only price authority, rules.yaml#/ServerPriceAuthority) + the generated PaymentService
    // port (services.yaml `payment.request`, issue #26) + PaymentProcessStateStore (the
    // payment_process_manager row it opens and single-flights
    // on, ADR-20260719-193500). Its created intent is no longer returned — the checkout reads
    // queries/paymentStatus (+ paymentStatusChanged) off the run row. The saga's event legs
    // (PaymentCaptured/PaymentFailed) run in the infrastructure ProcessManagerRunner, not here.
    if name == "placeOrder" {
        return Some((
            "        let store = ctx.data::<std::sync::Arc<dyn application::ports::EventStore>>()?.clone();\n        let catalogs = ctx.data::<std::sync::Arc<dyn application::queries::CatalogReadRepository>>()?.clone();\n        let payments = ctx.data::<std::sync::Arc<dyn application::generated::services::PaymentService>>()?.clone();\n        let pm_state = ctx.data::<std::sync::Arc<dyn application::pm_state::PaymentProcessStateStore>>()?.clone();\n".into(),
            // env.session_id rides into the spawn (disjoint capture of a Copy field): the anonymous
            // session scope for the paymentStatus read survives an app restart (#12).
            "application::commands::place_order(store.as_ref(), catalogs.as_ref(), payments.as_ref(), pm_state.as_ref(), cmd, env.session_id.map(domain::generated::scalars::SessionId), &actor).await.map(|_| ())".into(),
        ));
    }
    // The refund DECISION legs run on the RefundProcess orchestrator (application::process_managers::
    // refund), not an aggregate command handler: they need the RefundProcessStateStore (the pending
    // refund_process_manager row they decide on) and — for the approval — the generated
    // PaymentService port (services.yaml `payment.refund`) that requests the Stripe refund
    // (fail closed).
    if name == "approveRefund" {
        return Some((
            "        let store = ctx.data::<std::sync::Arc<dyn application::ports::EventStore>>()?.clone();\n        let refund_state = ctx.data::<std::sync::Arc<dyn application::pm_state::RefundProcessStateStore>>()?.clone();\n        let payments = ctx.data::<std::sync::Arc<dyn application::generated::services::PaymentService>>()?.clone();\n".into(),
            "application::process_managers::refund::approve_refund(store.as_ref(), refund_state.as_ref(), payments.as_ref(), cmd, &actor).await.map(|_| ())".into(),
        ));
    }
    if name == "denyRefund" {
        return Some((
            "        let store = ctx.data::<std::sync::Arc<dyn application::ports::EventStore>>()?.clone();\n        let refund_state = ctx.data::<std::sync::Arc<dyn application::pm_state::RefundProcessStateStore>>()?.clone();\n".into(),
            "application::process_managers::refund::deny_refund(store.as_ref(), refund_state.as_ref(), cmd, &actor).await.map(|_| ())".into(),
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
        // DeliveryPartnerRegistration aggregate (self-registration, #61).
        "registerDeliveryPartnerAvailability" => {
            ("RegisterDeliveryPartnerAvailability", "register_delivery_partner_availability", Extra::None)
        }
        "approveDeliveryPartnerAvailability" => {
            ("ApproveDeliveryPartnerAvailability", "approve_delivery_partner_availability", Extra::None)
        }
        "revokeDeliveryPartnerAvailability" => {
            ("RevokeDeliveryPartnerAvailability", "revoke_delivery_partner_availability", Extra::None)
        }
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
            "        let restaurants = ctx.data::<std::sync::Arc<dyn application::queries::RestaurantReadRepository>>()?.clone();\n".to_string(),
            ", restaurants.as_ref()",
        ),
        Extra::Ownership => (
            "        let ownership = ctx.data::<std::sync::Arc<dyn application::ports::GoogleOwnershipVerifier>>()?.clone();\n".to_string(),
            ", ownership.as_ref()",
        ),
        Extra::Probe => (
            "        let probe = ctx.data::<std::sync::Arc<dyn application::ports::GbpOrderLinkProbe>>()?.clone();\n".to_string(),
            ", probe.as_ref()",
        ),
        Extra::Prospection => (
            "        let prospection = ctx.data::<std::sync::Arc<dyn application::queries::ProspectionReadRepository>>()?.clone();\n".to_string(),
            ", prospection.as_ref()",
        ),
        Extra::Catalogs => (
            "        let catalogs = ctx.data::<std::sync::Arc<dyn application::queries::CatalogReadRepository>>()?.clone();\n".to_string(),
            ", catalogs.as_ref()",
        ),
        Extra::Auth => (
            "        let auth = ctx.data::<std::sync::Arc<dyn application::generated::services::IdentityService>>()?.clone();\n".to_string(),
            ", auth.as_ref()",
        ),
        Extra::AuthCustomers => (
            "        let auth = ctx.data::<std::sync::Arc<dyn application::generated::services::IdentityService>>()?.clone();\n        let customers = ctx.data::<std::sync::Arc<dyn application::queries::CustomerReadRepository>>()?.clone();\n".to_string(),
            ", auth.as_ref(), customers.as_ref()",
        ),
    };
    Some((
        format!(
            "        let store = ctx.data::<std::sync::Arc<dyn application::ports::EventStore>>()?.clone();\n{resolve_extra}"
        ),
        format!(
            "application::commands::{handler}(store.as_ref(){extra_arg}, cmd, &actor).await.map(|_| ())"
        ),
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
        "// GENERATED by the Captain.Food codegen from specs/api.yaml — do not edit by hand.\n// The GraphQL SubscriptionRoot: one stream resolver per api.yaml subscription, matching the generated\n// SDL shape. `operationStatusChanged` streams the command_journal lifecycle over the in-process\n// OperationStatusBus (snapshot-first, ownership-scoped — ADR-20260720-015500); the domain-fact\n// subscriptions (`orderStatusChanged`, `paymentStatusChanged`) subscribe to the in-process EventBus\n// (each envelope is published by PgEventStore::append AFTER a successful commit) and re-resolve the\n// read models / saga row rather than exposing raw domain_events (ADR-0005/0035). Each non-public\n// field carries its api.yaml `roles` as a `guard` (execution) + `visible` (introspection) pair from\n// the generated acl module (ADR-0006 role-as-path).\n//\n// Free-tier caveat: the buses are IN-PROCESS and a GraphQL-over-WebSocket connection lives only while\n// the app instance is warm (the uptimerobot ping keeps it so); after a restart/redeploy clients must\n// resubscribe and re-sync via the pull queries (`order`, `operationStatus`, `paymentStatus`).\n#![allow(unused_variables)]\n#![allow(dead_code)]\n\nuse async_graphql::futures_util::Stream;\n\nuse super::acl::*;\nuse super::inputs::*;\nuse super::scalars::*;\nuse super::types::*;\n\npub struct SubscriptionRoot;\n\n#[async_graphql::Subscription(name = \"Subscription\")]\nimpl SubscriptionRoot {\n",
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
        // The journaled-command status stream (ADR-20260720-015500): snapshot-first from the
        // command_journal (closes the subscribe/complete race), then every OperationStatusBus
        // transition for this messageId; completes on a terminal status. Ownership is checked at
        // setup — a non-owned/unknown messageId yields an EMPTY stream (no existence oracle).
        "operationStatusChanged" => Some(
            r#"        let journal = ctx.data::<std::sync::Arc<dyn application::journal::CommandJournal>>()?.clone();
        let bus = ctx.data::<infrastructure::OperationStatusBus>()?.clone();
        let wanted = input.message_id.0;
        let admin = matches!(
            ctx.data_opt::<crate::graphql::acl::RequestRole>(),
            Some(crate::graphql::acl::RequestRole::Admin)
        );
        let principal_uuid = ctx
            .data_opt::<crate::auth::Principal>()
            .and_then(|p| p.user_id.as_deref())
            .and_then(|s| uuid::Uuid::parse_str(s).ok());
        let session = ctx.data_opt::<crate::graphql::session::SessionHeader>().and_then(|s| s.0);
        let mut rx = bus.subscribe();
        Ok(async_stream::stream! {
            use domain::generated::scalars::CommandJournalStatus as J;
            // Snapshot-first: the current journal row (the acceptance already inserted it).
            let Ok(Some(row)) = journal.by_message(wanted).await else { return };
            let owned = admin
                || (principal_uuid.is_some() && principal_uuid == row.entry.user_id)
                || (session.is_some() && session == row.entry.session_id);
            if !owned {
                return;
            }
            let terminal = row.status != J::RECEIVED;
            yield Ok(super::mutation::operation_from_journal(&row));
            if terminal {
                return;
            }
            loop {
                match rx.recv().await {
                    Ok(update) if update.message_id == wanted => {
                        let terminal = update.status != J::RECEIVED;
                        yield Ok(Operation {
                            message_id: MessageId(update.message_id),
                            correlation_id: CorrelationId(update.correlation_id),
                            status: super::mutation::journal_status_api(update.status),
                            error_code: update.error_code.clone(),
                            message: update.message.clone(),
                            occurred_at: chrono::Utc::now(),
                        });
                        if terminal {
                            break;
                        }
                    }
                    Ok(_) => {}
                    // Lagged: the journal row is the pull truth — re-read and finish if terminal.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if let Ok(Some(row)) = journal.by_message(wanted).await {
                            let terminal = row.status != J::RECEIVED;
                            yield Ok(super::mutation::operation_from_journal(&row));
                            if terminal {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        })"#,
        ),
        // Push-based checkout payment tracking (ADR-20260720-015500): initial resolve + re-resolve
        // of the PlaceOrderProcess run row on every Payment-stream envelope; dedupes identical
        // states and completes when the run resolves. Initiator-scoped like queries/paymentStatus.
        "paymentStatusChanged" => Some(
            r#"        let bus = ctx.data::<infrastructure::EventBus>()?.clone();
        let pm = ctx.data::<std::sync::Arc<dyn application::pm_state::PaymentProcessStateStore>>()?.clone();
        let order_id: domain::generated::scalars::OrderId = input.order_id.into();
        let admin = matches!(
            ctx.data_opt::<crate::graphql::acl::RequestRole>(),
            Some(crate::graphql::acl::RequestRole::Admin)
        );
        let session = ctx.data_opt::<crate::graphql::session::SessionHeader>().and_then(|s| s.0);
        // Resolve the caller's Customer identity ONCE at setup (same path as queries/paymentStatus).
        let caller_customer: Option<domain::generated::scalars::CustomerId> = match ctx
            .data_opt::<crate::auth::Principal>()
            .and_then(|p| p.user_id.clone())
        {
            Some(auth_ref) => {
                let customers = ctx.data::<std::sync::Arc<dyn application::queries::CustomerReadRepository>>()?.clone();
                customers
                    .by_auth_ref(domain::generated::scalars::ExternalReference(auth_ref))
                    .await
                    .ok()
                    .flatten()
                    .map(|c| c.customer_id)
            }
            None => None,
        };
        let mut rx = bus.subscribe();
        Ok(async_stream::stream! {
            use domain::generated::scalars as ds;
            let owned = |row: &application::pm_state::PaymentProcessRow| {
                admin
                    || (caller_customer.is_some() && caller_customer == row.customer_id)
                    || (session.is_some() && session == row.session_id.as_ref().map(|s| s.0))
            };
            // (payment_status, clientSecret presence): dedupe key + what the checkout cares about.
            let mut last: Option<(ds::PaymentStatus, bool)> = None;
            if let Ok(Some(row)) = pm.by_order(order_id).await {
                if !owned(&row) {
                    return;
                }
                let terminal = row.process_status != ds::PaymentProcessStatus::AWAITING_PAYMENT_RESULT;
                last = Some((row.payment_status, row.client_secret.is_some()));
                yield Ok(PaymentIntent {
                    payment_intent_id: row.payment_intent_id.into(),
                    client_secret: row.client_secret,
                    status: row.payment_status.into(),
                });
                if terminal {
                    return;
                }
            }
            loop {
                let evt = match rx.recv().await {
                    Ok(evt) => evt,
                    // Lagged: the next Payment envelope re-resolves the CURRENT row anyway.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                if !evt.stream_name.starts_with("Payment-") {
                    continue;
                }
                let Ok(Some(row)) = pm.by_order(order_id).await else { continue };
                if !owned(&row) {
                    continue;
                }
                let key = (row.payment_status, row.client_secret.is_some());
                if last.as_ref() == Some(&key) {
                    continue;
                }
                last = Some(key);
                let terminal = row.process_status != ds::PaymentProcessStatus::AWAITING_PAYMENT_RESULT;
                yield Ok(PaymentIntent {
                    payment_intent_id: row.payment_intent_id.into(),
                    client_secret: row.client_secret,
                    status: row.payment_status.into(),
                });
                if terminal {
                    break;
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
        // Tracked by orderId (#14, ADR-20260720-220000) — the key the confirmation screen has —
        // replacing the pre-acceptance-first correlationId convention.
        let order_id: domain::generated::scalars::OrderId = input.order_id.into();
        let wanted_stream = format!("Order-{}", order_id.0);
        // Ownership scope, resolved ONCE at setup and applied per resolved row (the row may not be
        // projected yet when the confirmation screen subscribes): ADMIN sees any order; a CUSTOMER
        // caller must BE the order's customer (auth_ref → Customer); RESTAURANT/RESTAURANT_ACCOUNT
        // paths are trusted like the `orders` query until a caller↔restaurant binding exists
        // (recorded gap, ADR-20260720-220000). Guests: no session scope on Order reads
        // (ADR-20260720-213000 §3).
        let role = ctx.data_opt::<crate::graphql::acl::RequestRole>().copied();
        let caller_customer: Option<domain::generated::scalars::CustomerId> = match ctx
            .data_opt::<crate::auth::Principal>()
            .and_then(|p| p.user_id.clone())
        {
            Some(auth_ref) => {
                let customers = ctx.data::<std::sync::Arc<dyn application::queries::CustomerReadRepository>>()?.clone();
                customers
                    .by_auth_ref(domain::generated::scalars::ExternalReference(auth_ref))
                    .await
                    .ok()
                    .flatten()
                    .map(|c| c.customer_id)
            }
            None => None,
        };
        let mut rx = bus.subscribe();
        Ok(async_stream::stream! {
            use domain::generated::scalars as ds;
            let owned = |row: &application::queries::OrderTrackingRow| match role {
                Some(crate::graphql::acl::RequestRole::Admin) => true,
                Some(crate::graphql::acl::RequestRole::Customer) => {
                    caller_customer.is_some() && caller_customer == row.customer_id
                }
                // RESTAURANT / RESTAURANT_ACCOUNT — the only other paths the roles list admits.
                _ => true,
            };
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
                // Only THIS order's stream moves this subscription (`Order-<uuid>`).
                if evt.stream_name != wanted_stream {
                    continue;
                }
                // The row is folded ASYNCHRONOUSLY by the projection worker (ADR-0040): give it a
                // bounded window to absorb this event before treating it as a no-op.
                for attempt in 0..12u32 {
                    if attempt > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    }
                    let row = match orders.by_id(order_id).await {
                        Ok(Some(row)) => row,
                        Ok(None) => continue, // not projected yet — re-poll
                        Err(e) => {
                            yield Err(async_graphql::Error::new(e.to_string()));
                            continue 'events;
                        }
                    };
                    if !owned(&row) {
                        continue 'events; // not this caller's order — stay silent, no oracle
                    }
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

// ─── crates/application/src/generated/process_managers.rs (issue #25 — PM orchestrator pipelines) ──

/// One typed value form of the PM step DSL (`where`/`with`/`by`/`expect`/`set`, ADR-20260719-172821).
#[derive(Clone, Debug)]
enum PmVal {
    /// `{ const: <ENUM_MEMBER | integer> }`.
    Const(Value),
    /// `{ from: { $ref: '<file>#/<Msg>/properties/<prop>' } }` — a message/event property.
    From { owner: String, prop: String },
    /// `{ from_state: <column> }` — the PM's own state row.
    FromState(String),
    /// `{ from_read: <alias>.<column> }` — a prior read step's result.
    FromRead { alias: String, col: String },
    /// `{ from_port: <port>.<operation> }` — a prior call step's result (only the hand-written
    /// PlaceOrder command leg consumes one; the generated pipelines never do).
    FromPort,
    /// `{ from_envelope: event_id | correlation_id | occurred_at }` (ADR-0041).
    FromEnvelope(String),
    /// `{ from_hook: <name> }` — an ORCHESTRATOR-computed value (#60). Emits an async call to a
    /// generated per-leg hook `async fn <name>(&self, <leg-scope ctx>) -> Result<T, _>` that resolves
    /// the value at runtime (e.g. from config tables). Unlike `from_state`, it needs NO state row, so
    /// it is usable on a BIRTH leg; the hook receives whatever the leg has in scope (the trigger, any
    /// reads/delivered payloads, and the state row when one is loaded). Only valid inside `state.set`.
    FromHook(String),
}

fn parse_pm_val(v: &Value, loc: &str) -> PmVal {
    if let Some(c) = v.get("const") {
        return PmVal::Const(c.clone());
    }
    if let Some(f) = v.get("from") {
        let r = f
            .get("$ref")
            .and_then(|x| x.as_str())
            .unwrap_or_else(|| panic!("{}: `from` without $ref", loc));
        let pr = parse_ref(r).unwrap_or_else(|| panic!("{}: malformed $ref '{}'", loc, r));
        assert!(
            pr.path.len() == 3 && pr.path[1] == "properties",
            "{}: expected <file>#/<Msg>/properties/<prop>, got '{}'",
            loc,
            r
        );
        return PmVal::From { owner: pr.path[0].clone(), prop: pr.path[2].clone() };
    }
    if let Some(s) = v.get("from_state").and_then(|x| x.as_str()) {
        return PmVal::FromState(s.to_string());
    }
    if let Some(s) = v.get("from_read").and_then(|x| x.as_str()) {
        let (alias, col) = s
            .split_once('.')
            .unwrap_or_else(|| panic!("{}: from_read '{}' is not <alias>.<column>", loc, s));
        return PmVal::FromRead { alias: alias.to_string(), col: col.to_string() };
    }
    if v.get("from_port").is_some() {
        return PmVal::FromPort;
    }
    if let Some(s) = v.get("from_envelope").and_then(|x| x.as_str()) {
        return PmVal::FromEnvelope(s.to_string());
    }
    if let Some(s) = v.get("from_hook").and_then(|x| x.as_str()) {
        return PmVal::FromHook(s.to_string());
    }
    panic!("{}: unsupported value form {:?}", loc, v)
}

/// One ordered typed step of a PM leg (the closed DSL vocabulary, ADR-20260719-172821).
enum PmStepDef {
    Read { table: String, alias: String, where_: Vec<(String, PmVal)>, note: Option<String> },
    /// `that` = (subject, field, const-member) when structurally expressible.
    Guard { that: Option<(String, String, String)>, throws: Option<String>, skip: bool, note: Option<String> },
    Call { port: String, operation: String, note: Option<String> },
    Deliver { event: String, to: String, with: Vec<(String, PmVal)>, note: Option<String> },
    Send { command: String, with: Vec<(String, PmVal)>, for_each: Option<String>, note: Option<String> },
    StateStep { by: Vec<(String, PmVal)>, expect: Vec<(String, String)>, set: Vec<(String, PmVal)>, note: Option<String> },
}

struct PmLegDef {
    msg_file: String,
    msg: String,
    description: Option<String>,
    steps: Vec<PmStepDef>,
}

struct PmOrchDef {
    name: String,
    state_table: Option<String>,
    /// Outbound ports: (port name, services.yaml service name).
    ports: Vec<(String, String)>,
    legs: Vec<PmLegDef>,
}

/// Legs deliberately NOT generated (roadmap item 3 non-goal: genuinely computed business logic —
/// the PlaceOrder command leg is the server-side pricing path; it stays `commands::place_order`).
const PM_HAND_WRITTEN_LEGS: &[(&str, &str)] = &[("PlaceOrderProcess", "PlaceOrder")];

/// Aggregate → payload key property for `deliver` stream addressing. Convention: `<camel(agg)>Id`;
/// the Payment aggregate is keyed by the Stripe intent (`domain::payment::stream`).
fn pm_aggregate_key(aggregate: &str) -> String {
    if aggregate == "Payment" {
        "paymentIntentId".to_string()
    } else {
        format!("{}Id", camel(aggregate))
    }
}

fn pm_val_entries(node: Option<&Value>, loc: &str) -> Vec<(String, PmVal)> {
    let mut out = Vec::new();
    if let Some(Value::Mapping(m)) = node {
        for (k, v) in m {
            if let Some(key) = k.as_str() {
                out.push((key.to_string(), parse_pm_val(v, &format!("{}/{}", loc, key))));
            }
        }
    }
    out
}

fn parse_pm_orchestrators(model: &Model) -> Vec<PmOrchDef> {
    let mut out = Vec::new();
    let Some(Value::Mapping(m)) = model.defs.get("processmanager.yaml") else {
        return out;
    };
    for (k, node) in m {
        let Some(name) = k.as_str() else { continue };
        if node.get("type").and_then(|t| t.as_str()) != Some("process-manager") {
            continue;
        }
        let state_table = node
            .get("state_table")
            .and_then(|s| s.get("$ref"))
            .and_then(|r| r.as_str())
            .and_then(ref_name);
        let mut ports = Vec::new();
        if let Some(Value::Mapping(pm)) = node.get("ports") {
            for (pk, pv) in pm {
                if let (Some(port), Some(svc)) = (
                    pk.as_str(),
                    pv.get("$ref").and_then(|r| r.as_str()).and_then(ref_name),
                ) {
                    ports.push((port.to_string(), svc));
                }
            }
        }
        let mut legs = Vec::new();
        for (i, leg) in node
            .get("receives")
            .and_then(|r| r.as_sequence())
            .map(|s| s.iter())
            .into_iter()
            .flatten()
            .enumerate()
        {
            let mref = leg
                .get("message")
                .and_then(|x| x.get("$ref"))
                .and_then(|r| r.as_str())
                .unwrap_or_else(|| panic!("processmanager.yaml#/{}/receives[{}]: missing message $ref", name, i));
            let pr = parse_ref(mref).unwrap_or_else(|| panic!("processmanager.yaml#/{}: bad message ref '{}'", name, mref));
            let msg = pr.path[0].clone();
            if PM_HAND_WRITTEN_LEGS.contains(&(name, msg.as_str())) {
                continue;
            }
            let loc = format!("processmanager.yaml#/{}/receives[{}]", name, i);
            let mut steps = Vec::new();
            for (j, step) in leg
                .get("steps")
                .and_then(|s| s.as_sequence())
                .map(|s| s.iter())
                .into_iter()
                .flatten()
                .enumerate()
            {
                let sloc = format!("{}/steps[{}]", loc, j);
                let sm = step.as_mapping().unwrap_or_else(|| panic!("{}: step is not a mapping", sloc));
                let (kind, body) = sm
                    .iter()
                    .next()
                    .map(|(k, v)| (k.as_str().unwrap_or(""), v))
                    .unwrap_or_else(|| panic!("{}: empty step", sloc));
                let note = body.get("note").and_then(|n| n.as_str()).map(|s| s.to_string());
                steps.push(match kind {
                    "read" => PmStepDef::Read {
                        table: body
                            .get("model")
                            .and_then(|x| x.get("$ref"))
                            .and_then(|r| r.as_str())
                            .and_then(ref_name)
                            .unwrap_or_else(|| panic!("{}: read without model $ref", sloc)),
                        alias: body
                            .get("as")
                            .and_then(|a| a.as_str())
                            .unwrap_or_else(|| panic!("{}: read without `as`", sloc))
                            .to_string(),
                        where_: pm_val_entries(body.get("where"), &sloc),
                        note,
                    },
                    "guard" => {
                        let that = body.get("that").and_then(|t| t.as_mapping()).map(|t| {
                            let (subject, fields) = t.iter().next().expect("guard.that subject");
                            let fm = fields.as_mapping().expect("guard.that fields");
                            let (field, cv) = fm.iter().next().expect("guard.that field");
                            let member = cv
                                .get("const")
                                .and_then(|c| c.as_str())
                                .unwrap_or_else(|| panic!("{}: guard.that without const", sloc));
                            (
                                subject.as_str().unwrap().to_string(),
                                field.as_str().unwrap().to_string(),
                                member.to_string(),
                            )
                        });
                        PmStepDef::Guard {
                            that,
                            throws: body
                                .get("throws")
                                .and_then(|x| x.get("$ref"))
                                .and_then(|r| r.as_str())
                                .and_then(ref_name),
                            skip: body.get("skip").and_then(|s| s.as_bool()) == Some(true),
                            note,
                        }
                    }
                    "call" => PmStepDef::Call {
                        port: body.get("port").and_then(|p| p.as_str()).unwrap_or_else(|| panic!("{}: call without port", sloc)).to_string(),
                        operation: body.get("operation").and_then(|p| p.as_str()).unwrap_or_else(|| panic!("{}: call without operation", sloc)).to_string(),
                        note,
                    },
                    "deliver" => {
                        assert!(body.get("for_each").is_none(), "{}: deliver.for_each is not supported by the generator yet", sloc);
                        PmStepDef::Deliver {
                            event: body
                                .get("event")
                                .and_then(|x| x.get("$ref"))
                                .and_then(|r| r.as_str())
                                .and_then(ref_name)
                                .unwrap_or_else(|| panic!("{}: deliver without event $ref", sloc)),
                            to: body
                                .get("to")
                                .and_then(|x| x.get("$ref"))
                                .and_then(|r| r.as_str())
                                .and_then(ref_name)
                                .unwrap_or_else(|| panic!("{}: deliver without target $ref", sloc)),
                            with: pm_val_entries(body.get("with"), &sloc),
                            note,
                        }
                    }
                    "send" => PmStepDef::Send {
                        command: body
                            .get("command")
                            .and_then(|x| x.get("$ref"))
                            .and_then(|r| r.as_str())
                            .and_then(ref_name)
                            .unwrap_or_else(|| panic!("{}: send without command $ref", sloc)),
                        with: pm_val_entries(body.get("with"), &sloc),
                        for_each: body.get("for_each").and_then(|f| f.as_str()).map(|s| s.to_string()),
                        note,
                    },
                    "state" => {
                        let mut expect = Vec::new();
                        if let Some(Value::Mapping(em)) = body.get("expect") {
                            for (ck, cv) in em {
                                expect.push((
                                    ck.as_str().unwrap().to_string(),
                                    cv.get("const")
                                        .and_then(|c| c.as_str())
                                        .unwrap_or_else(|| panic!("{}: state.expect without const", sloc))
                                        .to_string(),
                                ));
                            }
                        }
                        PmStepDef::StateStep {
                            by: pm_val_entries(body.get("by"), &sloc),
                            expect,
                            set: pm_val_entries(body.get("set"), &sloc),
                            note,
                        }
                    }
                    other => panic!("{}: unknown step kind '{}'", sloc, other),
                });
            }
            legs.push(PmLegDef {
                msg_file: pr.file.clone(),
                msg,
                description: leg.get("description").and_then(|d| d.as_str()).map(|s| s.to_string()),
                steps,
            });
        }
        out.push(PmOrchDef { name: name.to_string(), state_table, ports, legs });
    }
    out
}

/// PascalCase of a snake_case name (`open_carts` → `OpenCarts`).
fn pm_pascal(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut cs = w.chars();
            match cs.next() {
                Some(c) => c.to_ascii_uppercase().to_string() + cs.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Fully-qualified Rust path for a bare generated type name (`Money` → `domain::generated::entities::Money`).
fn pm_qualify(model: &Model, ty: &str) -> String {
    if let Some(inner) = ty.strip_prefix("Vec<").and_then(|t| t.strip_suffix('>')) {
        return format!("Vec<{}>", pm_qualify(model, inner));
    }
    if let Some(inner) = ty.strip_prefix("Option<").and_then(|t| t.strip_suffix('>')) {
        return format!("Option<{}>", pm_qualify(model, inner));
    }
    match ty {
        "String" | "i32" | "i64" | "bool" | "f64" | "serde_json::Value" => ty.to_string(),
        "chrono::DateTime<chrono::Utc>" => ty.to_string(),
        name if model.defs.get("scalars.yaml").map(|s| s.get(name).is_some()).unwrap_or(false) => {
            format!("domain::generated::scalars::{}", name)
        }
        name if model.defs.get("entities.yaml").map(|s| s.get(name).is_some()).unwrap_or(false) => {
            format!("domain::generated::entities::{}", name)
        }
        other => panic!("pm orchestrator emitter: cannot qualify type '{}'", other),
    }
}

/// Whether a (bare or qualified) type is `Copy` in the generated Rust (uuid/int/enum scalar newtypes
/// and numeric primitives are; String-backed newtypes, entities and json values are not).
fn pm_is_copy(model: &Model, ty: &str) -> bool {
    let base = ty.rsplit("::").next().unwrap_or(ty);
    if let Some(inner) = base.strip_prefix("Option<").and_then(|t| t.strip_suffix('>')) {
        return pm_is_copy(model, inner);
    }
    match base {
        "i32" | "i64" | "bool" | "f64" => true,
        "String" | "serde_json::Value" => false,
        name => model
            .defs
            .get("scalars.yaml")
            .and_then(|s| s.get(name))
            .map(|node| {
                node.get("enum").map(|e| e.is_sequence()).unwrap_or(false)
                    || node.get("format").and_then(|f| f.as_str()) == Some("uuid")
                    || node.get("type").and_then(|t| t.as_str()) == Some("integer")
            })
            .unwrap_or(false),
    }
}

fn pm_clone_if_needed(model: &Model, ty: &str, expr: String) -> String {
    if pm_is_copy(model, ty) {
        expr
    } else {
        format!("{}.clone()", expr)
    }
}

/// A message/event property's full Rust type (Option-wrapped per required/nullable) + base type.
fn pm_prop_type(model: &Model, file: &str, owner: &str, prop: &str) -> (String, String) {
    let node = model
        .defs
        .get(file)
        .and_then(|f| f.get(owner))
        .unwrap_or_else(|| panic!("pm emitter: {}#/{} not found", file, owner));
    let pnode = node
        .get("properties")
        .and_then(|p| p.get(prop))
        .unwrap_or_else(|| panic!("pm emitter: {}#/{}/properties/{} not found", file, owner, prop));
    let base = struct_field_type(file, owner, prop, pnode);
    let required = node
        .get("required")
        .and_then(|r| r.as_sequence())
        .map(|s| s.iter().any(|v| v.as_str() == Some(prop)))
        .unwrap_or(false);
    let nullable = pnode.get("nullable").and_then(|x| x.as_bool()) == Some(true);
    let full = if (nullable || !required) && !base.starts_with("Vec<") {
        format!("Option<{}>", base)
    } else {
        base.clone()
    };
    (full, base)
}

/// Adapt a source expression of `src` type to the `target` type (both bare, un-qualified type
/// strings). Closed coercion set — anything else is a spec/emitter gap, failed loudly.
fn pm_adapt(model: &Model, expr: String, src: &str, target: &str) -> String {
    if src == target {
        return pm_clone_if_needed(model, target, expr);
    }
    if target == format!("Option<{}>", src) {
        return format!("Some({})", pm_clone_if_needed(model, src, expr));
    }
    if src == "Money" && target == "MoneyCents" {
        return format!("{}.amount_cents", expr);
    }
    if src == "Money" && target == "Option<MoneyCents>" {
        return format!("Some({}.amount_cents)", expr);
    }
    if src == "i32" && target == "i64" {
        return format!("i64::from({})", expr);
    }
    panic!("pm emitter: no coercion from {} to {} for `{}`", src, target, expr)
}

/// How one consumed read-column is typed — by its most demanding sink (ADR-20260721 issue #25).
struct PmReadField {
    col: String,
    /// Bare (un-qualified) Rust type, incl. `Option<…>` where the sink allows absence.
    ty: String,
    doc: String,
}

struct PmReadInfo {
    alias: String,
    table: String,
    is_vec: bool,
    /// Hook parameters — the `where` values sourced from the message (const filters stay doc-only).
    params: Vec<(String, String)>,
    fields: Vec<PmReadField>,
}

/// Everything the emitter derives once per PM before writing code.
struct PmEmit<'a> {
    model: &'a Model,
    /// The PM's state table (row/store base name, columns, pk) — `None` for stateless PMs.
    table: Option<&'a PmTable>,
    reads: Vec<PmReadInfo>,
}

impl<'a> PmEmit<'a> {
    fn col(&self, name: &str) -> &SqlColumn {
        self.table
            .and_then(|t| t.columns.iter().find(|c| c.name == name))
            .unwrap_or_else(|| panic!("pm emitter: state column '{}' not found", name))
    }
    /// Bare Rust type of a state column (Option-wrapped per nullability).
    fn col_ty(&self, name: &str) -> String {
        let c = self.col(name);
        let t = self.table.unwrap();
        let base = pm_ty(self.model, &t.table, &c.name, &c.ty).field();
        if c.nullable {
            format!("Option<{}>", base)
        } else {
            base
        }
    }
    fn read(&self, alias: &str) -> &PmReadInfo {
        self.reads
            .iter()
            .find(|r| r.alias == alias)
            .unwrap_or_else(|| panic!("pm emitter: read alias '{}' not found", alias))
    }
    fn read_field_ty(&self, alias: &str, col: &str) -> String {
        self.read(alias)
            .fields
            .iter()
            .find(|f| f.col == col)
            .unwrap_or_else(|| panic!("pm emitter: read field {}.{} not found", alias, col))
            .ty
            .clone()
    }
}

/// Derive each read alias's hook signature: `where` params from the message, and one struct field
/// per CONSUMED column, typed by its most demanding sink (state column > payload literal > guard >
/// builder-only, where builder-only fields are `Option<sink type>` so the hook can signal absence).
fn pm_read_infos<'a>(model: &'a Model, pm: &PmOrchDef, table: Option<&PmTable>) -> Vec<PmReadInfo> {
    let mut reads: Vec<PmReadInfo> = Vec::new();
    // Pass 1: declare aliases (dedup identical re-declarations across legs).
    for leg in &pm.legs {
        for step in &leg.steps {
            if let PmStepDef::Read { table: t, alias, where_, .. } = step {
                let mut params = Vec::new();
                for (col, val) in where_ {
                    if let PmVal::From { owner, prop } = val {
                        let (_, base) = pm_prop_type(model, &leg.msg_file, owner, prop);
                        params.push((rust_ident(col), base));
                    }
                }
                if let Some(existing) = reads.iter().find(|r| r.alias == *alias) {
                    assert!(
                        existing.table == *t && existing.params == params,
                        "processmanager.yaml#/{}: read alias '{}' re-declared with a different shape",
                        pm.name,
                        alias
                    );
                } else {
                    reads.push(PmReadInfo {
                        alias: alias.clone(),
                        table: t.clone(),
                        is_vec: false,
                        params,
                        fields: Vec::new(),
                    });
                }
            }
        }
    }
    // Vecness: an alias a send/deliver iterates with `for_each`.
    for leg in &pm.legs {
        for step in &leg.steps {
            if let PmStepDef::Send { for_each: Some(alias), .. } = step {
                if let Some(r) = reads.iter_mut().find(|r| r.alias == *alias) {
                    r.is_vec = true;
                }
            }
        }
    }
    // Pass 2: consumed columns and their sink types.
    let views = parse_views(model);
    let add_field = |reads: &mut Vec<PmReadInfo>, alias: &str, col: &str, ty: String, doc: String| {
        let r = reads
            .iter_mut()
            .find(|r| r.alias == alias)
            .unwrap_or_else(|| panic!("processmanager.yaml#/{}: from_read references unknown alias '{}'", pm.name, alias));
        if let Some(f) = r.fields.iter().find(|f| f.col == col) {
            assert!(
                f.ty == ty,
                "processmanager.yaml#/{}: read field {}.{} typed both {} and {} by its sinks",
                pm.name, alias, col, f.ty, ty
            );
        } else {
            r.fields.push(PmReadField { col: col.to_string(), ty, doc });
        }
    };
    for leg in &pm.legs {
        for step in &leg.steps {
            match step {
                PmStepDef::Guard { that: Some((subject, field, member)), .. }
                    if reads.iter().any(|r| r.alias == *subject) =>
                {
                    let tname = reads.iter().find(|r| r.alias == *subject).unwrap().table.clone();
                    let view = views
                        .iter()
                        .find(|v| v.name == tname)
                        .unwrap_or_else(|| panic!("pm emitter: projection table '{}' not found", tname));
                    let colty = view
                        .columns
                        .iter()
                        .find(|c| c.name == *field)
                        .map(|c| projection_rust_type(&c.ty))
                        .unwrap_or_else(|| panic!("pm emitter: {}.{} not found", tname, field));
                    add_field(&mut reads, subject, field, colty, format!("Feeds the `= {}` guard.", member));
                }
                PmStepDef::StateStep { set, .. } => {
                    for (col, val) in set {
                        if let PmVal::FromRead { alias, col: rcol } = val {
                            let t = table.unwrap_or_else(|| panic!("pm emitter: {} sets state without a state_table", pm.name));
                            let c = t
                                .columns
                                .iter()
                                .find(|c| c.name == *col)
                                .unwrap_or_else(|| panic!("pm emitter: state column '{}' not found", col));
                            let base = pm_ty(model, &t.table, &c.name, &c.ty).field();
                            let ty = if c.nullable { format!("Option<{}>", base) } else { base };
                            add_field(&mut reads, alias, rcol, ty, format!("Feeds `state.set {}`.", col));
                        }
                    }
                }
                PmStepDef::Deliver { event, with, .. } => {
                    let covered = pm_deliver_covered(model, "events.yaml", event, with);
                    for (prop, val) in with {
                        if let PmVal::FromRead { alias, col } = val {
                            let (full, base) = pm_prop_type(model, "events.yaml", event, prop);
                            let ty = if covered { full } else { format!("Option<{}>", base) };
                            add_field(&mut reads, alias, col, ty, format!("Feeds `{}.{}`.", event, prop));
                        }
                    }
                }
                PmStepDef::Send { command, with, for_each, .. } => {
                    for (prop, val) in with {
                        if let PmVal::FromRead { alias, col } = val {
                            // A `for_each` send reads from the ITERATED alias — same typing rule.
                            let _ = for_each;
                            let (full, _) = pm_prop_type(model, "commands.yaml", command, prop);
                            add_field(&mut reads, alias, col, full, format!("Feeds `{}.{}`.", command, prop));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    reads
}

/// A deliver/send payload is fully expressible ("covered") when every REQUIRED property has a `with`
/// entry — otherwise a builder hook owns the whole payload (frozen snapshots, derived ids…).
fn pm_deliver_covered(model: &Model, file: &str, msg: &str, with: &[(String, PmVal)]) -> bool {
    let node = model
        .defs
        .get(file)
        .and_then(|f| f.get(msg))
        .unwrap_or_else(|| panic!("pm emitter: {}#/{} not found", file, msg));
    node.get("required")
        .and_then(|r| r.as_sequence())
        .map(|s| {
            s.iter()
                .filter_map(|v| v.as_str())
                .all(|req| with.iter().any(|(p, _)| p == req))
        })
        .unwrap_or(true)
}

/// Escape a spec note for embedding inside a generated `format!` string literal.
fn pm_str_lit(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('{', "{{").replace('}', "}}")
}

/// One hook method registered while emitting a leg body (deduped by name — e.g. `finalize`).
struct PmHook {
    name: String,
    /// Full trait-item source (doc comment + signature, default body where one exists).
    code: String,
    is_async: bool,
}

fn pm_push_hook(hooks: &mut Vec<PmHook>, name: &str, is_async: bool, code: String) {
    if !hooks.iter().any(|h| h.name == name) {
        hooks.push(PmHook { name: name.to_string(), code, is_async });
    }
}

/// Leg-local emission scope: what a value expression may reference at the current step.
struct PmScope<'a> {
    msg_file: &'a str,
    msg: &'a str,
    msg_var: &'a str,
    row: bool,
    /// Delivered/built event payload names in scope (var = `snake_type(name)`).
    payloads: Vec<String>,
    loop_alias: Option<String>,
}

fn pm_value_expr(ctx: &PmEmit, scope: &PmScope, val: &PmVal, target_full: &str, loc: &str) -> String {
    let model = ctx.model;
    match val {
        PmVal::Const(v) => {
            let base = target_full
                .strip_prefix("Option<")
                .and_then(|t| t.strip_suffix('>'))
                .unwrap_or(target_full);
            if let Some(member) = v.as_str() {
                let e = format!("domain::generated::scalars::{}::{}", base, member);
                if target_full.starts_with("Option<") {
                    format!("Some({})", e)
                } else {
                    e
                }
            } else if let Some(n) = v.as_i64() {
                assert!(matches!(base, "i32" | "i64"), "{}: integer const for non-integer target {}", loc, target_full);
                n.to_string()
            } else {
                panic!("{}: unsupported const {:?}", loc, v)
            }
        }
        PmVal::From { owner, prop } => {
            let (file, var) = if owner == scope.msg {
                (scope.msg_file.to_string(), scope.msg_var.to_string())
            } else if scope.payloads.iter().any(|p| p == owner) {
                ("events.yaml".to_string(), snake_type(owner))
            } else {
                panic!("{}: `from` references {} which is neither the trigger nor a delivered payload", loc, owner)
            };
            let (full, _) = pm_prop_type(model, &file, owner, prop);
            let expr = format!("{}.{}", var, rust_ident(&snake_field(prop)));
            pm_adapt(model, expr, &full, target_full)
        }
        PmVal::FromState(col) => {
            assert!(scope.row, "{}: from_state with no state row in scope", loc);
            let src = ctx.col_ty(col);
            pm_adapt(model, format!("row.{}", rust_ident(col)), &src, target_full)
        }
        PmVal::FromRead { alias, col } => {
            let expr = if scope.loop_alias.as_deref() == Some(alias.as_str()) {
                format!("item.{}", rust_ident(col))
            } else {
                format!("{}.{}", rust_ident(alias), rust_ident(col))
            };
            let src = ctx.read_field_ty(alias, col);
            pm_adapt(model, expr, &src, target_full)
        }
        PmVal::FromEnvelope(kind) => {
            assert!(kind == "event_id", "{}: from_envelope {} not supported by the generator yet", loc, kind);
            let base = target_full
                .strip_prefix("Option<")
                .and_then(|t| t.strip_suffix('>'))
                .unwrap_or(target_full);
            assert!(base == "ExternalReference", "{}: from_envelope event_id must target ExternalReference", loc);
            let e = "domain::generated::scalars::ExternalReference(env.event_id.to_string())".to_string();
            if target_full.starts_with("Option<") {
                format!("Some({})", e)
            } else {
                e
            }
        }
        PmVal::FromPort => panic!("{}: from_port values are only used by the hand-written PlaceOrder command leg", loc),
        PmVal::FromHook(_) => panic!("{}: from_hook values are only valid inside state.set (handled by emit_state)", loc),
    }
}

/// Emit a full struct literal for a COVERED deliver/send payload: every property in spec order —
/// `with` entries as value expressions, absent optionals as `None`/empty `Vec`.
fn pm_payload_literal(
    ctx: &PmEmit,
    scope: &PmScope,
    file: &str,
    msg_name: &str,
    with: &[(String, PmVal)],
    path: &str,
    var: &str,
    indent: &str,
    loc: &str,
) -> String {
    let node = ctx.model.defs.get(file).and_then(|f| f.get(msg_name)).unwrap();
    let mut out = format!("{}let {} = {} {{\n", indent, var, path);
    if let Some(props) = node.get("properties").and_then(|p| p.as_mapping()) {
        for (pk, _) in props {
            let prop = pk.as_str().unwrap();
            let ident = rust_ident(&snake_field(prop));
            let (full, _) = pm_prop_type(ctx.model, file, msg_name, prop);
            let expr = if let Some((_, val)) = with.iter().find(|(p, _)| p == prop) {
                pm_value_expr(ctx, scope, val, &full, &format!("{}/{}", loc, prop))
            } else if full.starts_with("Option<") {
                "None".to_string()
            } else if full.starts_with("Vec<") {
                "Vec::new()".to_string()
            } else {
                panic!("{}: required property '{}' has no `with` entry", loc, prop)
            };
            out.push_str(&format!("{}    {}: {},\n", indent, ident, expr));
        }
    }
    out.push_str(&format!("{}}};\n", indent));
    out
}

/// Mutable state while emitting one leg's pipeline + hooks trait.
struct PmLegGen<'a> {
    ctx: &'a PmEmit<'a>,
    pm: &'a PmOrchDef,
    leg: &'a PmLegDef,
    is_cmd: bool,
    msg_var: &'static str,
    msg_path: String,
    row_ty: Option<String>,
    body: String,
    hooks: Vec<PmHook>,
    row_in_scope: bool,
    payloads: Vec<String>,
    /// Hook context refs available so far: (param name, param type, call-site expression).
    hook_ctx: Vec<(String, String, String)>,
    /// The correlation identity of the loaded row: (json context key, raw value expression).
    by_ctx: Option<(String, String)>,
    admission_pending: bool,
    single_send: bool,
}

impl<'a> PmLegGen<'a> {
    fn scope(&self, loop_alias: Option<String>) -> PmScope<'a> {
        PmScope {
            msg_file: &self.leg.msg_file,
            msg: &self.leg.msg,
            msg_var: self.msg_var,
            row: self.row_in_scope,
            payloads: self.payloads.clone(),
            loop_alias,
        }
    }
    fn table(&self) -> &PmTable {
        self.ctx.table.unwrap_or_else(|| panic!("processmanager.yaml#/{}: leg needs a state_table", self.pm.name))
    }
    fn row_ty(&self) -> &str {
        self.row_ty.as_deref().expect("state row type")
    }
    fn actor_ref(&self) -> &'static str {
        if self.is_cmd { "actor" } else { "&actor" }
    }
    fn hook_sig(&self) -> String {
        self.hook_ctx.iter().map(|(n, t, _)| format!("{}: {}", n, t)).collect::<Vec<_>>().join(", ")
    }
    fn hook_args(&self) -> String {
        self.hook_ctx.iter().map(|(_, _, e)| e.clone()).collect::<Vec<_>>().join(", ")
    }
    fn push(&mut self, ind: usize, line: &str) {
        if line.is_empty() {
            self.body.push('\n');
        } else {
            self.body.push_str(&" ".repeat(ind));
            self.body.push_str(line);
            self.body.push('\n');
        }
    }

    /// The raw (un-adapted) expression for a `by`/pk value — only message properties are supported.
    fn raw_msg_expr(&self, val: &PmVal, loc: &str) -> String {
        match val {
            PmVal::From { owner, prop } => {
                assert!(owner == &self.leg.msg, "{}: by/pk value must come from the trigger message", loc);
                format!("{}.{}", self.msg_var, rust_ident(&snake_field(prop)))
            }
            _ => panic!("{}: by/pk value must be a `from` message property", loc),
        }
    }

    fn emit_admission(&mut self, ind: usize) {
        let t = self.table();
        let pk = t.pk.clone();
        // The pk value comes from this leg's own `state.set`.
        let pk_val = self
            .leg
            .steps
            .iter()
            .find_map(|s| match s {
                PmStepDef::StateStep { set, .. } => set.iter().find(|(c, _)| *c == pk).map(|(_, v)| v.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("processmanager.yaml#/{}: opening leg sets no pk '{}'", self.pm.name, pk));
        let raw = self.raw_msg_expr(&pk_val, "admission");
        let c = self.ctx.col(&pk);
        let by_ref = pm_ty(self.ctx.model, &self.table().table, &c.name, &c.ty).by_ref();
        let arg = if by_ref { format!("&{}", raw) } else { raw };
        let method = pm_lookup_method(&pk);
        self.push(ind, "// admission: the run row this leg would (re-)open — `admit` may veto with a benign skip.");
        self.push(ind, &format!("if let Some(existing) = state.{}({}).await? {{", method, arg));
        self.push(ind + 4, "if let Some(reason) = hooks.admit(&existing) {");
        self.push(ind + 8, "return Ok(Outcome::Skipped(reason));");
        self.push(ind + 4, "}");
        self.push(ind, "}");
        let row_ty = self.row_ty().to_string();
        pm_push_hook(
            &mut self.hooks,
            "admit",
            false,
            format!(
                "        /// Whether an EXISTING run row may be re-upserted by this opening leg — `Some(reason)` ends\n        /// the leg as a benign skip (e.g. never regress a decided run). Default: admit.\n        fn admit(&self, _existing: &{}) -> Option<String> {{\n            None\n        }}\n",
                row_ty
            ),
        );
        self.admission_pending = false;
    }
}

impl<'a> PmLegGen<'a> {
    fn emit_read(&mut self, table: &str, alias: &str, where_: &[(String, PmVal)], note: &Option<String>, ind: usize) {
        assert!(!self.is_cmd, "read steps on command legs are not generated (hand-written PlaceOrder only)");
        let info = self.ctx.read(alias);
        let struct_name = format!("{}Read", pm_pascal(alias));
        let mut args = Vec::new();
        let mut where_doc = Vec::new();
        for (col, val) in where_ {
            match val {
                PmVal::From { owner, prop } => {
                    let (full, base) = pm_prop_type(self.ctx.model, &self.leg.msg_file, owner, prop);
                    assert!(full == base, "read where {}.{}: optional message properties unsupported", alias, col);
                    let expr = format!("{}.{}", self.msg_var, rust_ident(&snake_field(prop)));
                    args.push(pm_clone_if_needed(self.ctx.model, &base, expr));
                    where_doc.push(format!("{} = message.{}", col, prop));
                }
                PmVal::Const(c) => where_doc.push(format!("{} = {}", col, c.as_str().unwrap_or("?"))),
                other => panic!("read where {}.{}: unsupported value {:?}", alias, col, other),
            }
        }
        let note_s = note.as_deref().map(|n| format!(" {}", ws1(n))).unwrap_or_default();
        self.push(ind, &format!("// read `{}` ← {} where {}.{}", alias, table, where_doc.join(", "), note_s));
        if info.is_vec {
            self.push(ind, &format!("let {} = hooks.read_{}({}).await?;", rust_ident(alias), alias, args.join(", ")));
        } else {
            self.push(ind, &format!("let {} = match hooks.read_{}({}).await? {{", rust_ident(alias), alias, args.join(", ")));
            self.push(ind + 4, "super::HookOutcome::Ready(v) => v,");
            self.push(ind + 4, "super::HookOutcome::Skip(reason) => return Ok(Outcome::Skipped(reason)),");
            self.push(ind, "};");
        }
        let params = info
            .params
            .iter()
            .map(|(n, t)| format!("{}: {}", n, pm_qualify(self.ctx.model, t)))
            .collect::<Vec<_>>()
            .join(", ");
        let ret = if info.is_vec {
            format!("Result<Vec<{}>, domain::shared::errors::DomainError>", struct_name)
        } else {
            format!("Result<super::HookOutcome<{}>, domain::shared::errors::DomainError>", struct_name)
        };
        let absence = if info.is_vec { "Absence = empty (no skip)." } else { "Return `Skip` to end the leg as a benign no-op." };
        pm_push_hook(
            &mut self.hooks,
            &format!("read_{}", alias),
            true,
            format!(
                "        /// Execute `read {}` over `{}` (where {}).{} {}\n        async fn read_{}(&self, {}) -> {};\n",
                alias, table, where_doc.join(", "), note_s, absence, alias, params, ret
            ),
        );
        let ctx_ty = if info.is_vec { format!("&[{}]", struct_name) } else { format!("&{}", struct_name) };
        self.hook_ctx.push((rust_ident(alias), ctx_ty, format!("&{}", rust_ident(alias))));
    }

    fn emit_guard_that(&mut self, subject: &str, field: &str, member: &str, throws: &Option<String>, skip: bool, note: &Option<String>, ind: usize) {
        let model = self.ctx.model;
        let (expr, base) = match subject {
            "message" => {
                let (full, base) = pm_prop_type(model, &self.leg.msg_file, &self.leg.msg, field);
                assert!(full == base, "guard on optional message property {} unsupported", field);
                (format!("{}.{}", self.msg_var, rust_ident(&snake_field(field))), base)
            }
            "state" => {
                assert!(self.row_in_scope, "guard on state with no row in scope");
                let ty = self.ctx.col_ty(field);
                assert!(!ty.starts_with("Option<"), "guard on nullable state column {} unsupported", field);
                (format!("row.{}", rust_ident(field)), ty)
            }
            alias => (format!("{}.{}", rust_ident(alias), rust_ident(field)), self.ctx.read_field_ty(alias, field)),
        };
        let cmp = if base == "String" {
            format!("\"{}\"", member)
        } else {
            format!("domain::generated::scalars::{}::{}", base, member)
        };
        let note_s = note.as_deref().map(ws1).unwrap_or_default();
        if skip {
            self.push(ind, &format!("// guard {}.{} = {} — benign alternative: {}", subject, field, member, note_s));
            self.push(ind, &format!("if {} != {} {{", expr, cmp));
            self.push(
                ind + 4,
                &format!(
                    "return Ok(Outcome::Skipped(format!(\"{}.{} is {{:?}}, not {} — {}\", {})));",
                    subject, field, member, pm_str_lit(&note_s), expr
                ),
            );
            self.push(ind, "}");
        } else {
            let err = throws.as_deref().expect("guard without skip must throw");
            let (key, raw) = self.by_ctx.clone().unwrap_or_else(|| panic!("guard throws {} with no correlation context", err));
            self.push(ind, &format!("// guard {}.{} = {} — throws {}. {}", subject, field, member, err, note_s));
            self.push(ind, &format!("if {} != {} {{", expr, cmp));
            self.push(
                ind + 4,
                &format!(
                    "return Err(domain::shared::errors::DomainError::rejected(\"{}\", serde_json::json!({{ \"{}\": &{} }})));",
                    err, key, raw
                ),
            );
            self.push(ind, "}");
        }
    }

    fn emit_call(&mut self, port: &str, operation: &str, note: &Option<String>, ind: usize) {
        if self.admission_pending {
            self.emit_admission(ind);
        }
        let svc = self
            .pm
            .ports
            .iter()
            .find(|(p, _)| p == port)
            .map(|(_, s)| s.clone())
            .unwrap_or_else(|| panic!("processmanager.yaml#/{}: undeclared port '{}'", self.pm.name, port));
        let base = pm_pascal(&svc);
        let op = pm_pascal(operation);
        let corr = if self.is_cmd { "actor.correlation_id" } else { "env.correlation_id" };
        let note_s = note.as_deref().map(|n| format!(" — {}", ws1(n))).unwrap_or_default();
        let hook = format!("input_{}_{}", port, operation);
        self.push(ind, &format!("// call {}.{}{}", port, operation, note_s));
        self.push(ind, &format!("match hooks.{}({}).await? {{", hook, self.hook_args()));
        self.push(ind + 4, "super::HookOutcome::Ready(input) => {");
        self.push(
            ind + 8,
            &format!(
                "{}.{}(input, &crate::generated::services::ServiceCallMeta::new({})).await?;",
                rust_ident(port), rust_ident(operation), corr
            ),
        );
        self.push(ind + 4, "}");
        self.push(ind + 4, "super::HookOutcome::Skip(reason) => {");
        self.push(ind + 8, &format!("eprintln!(\"saga[{}]: call {}.{} skipped — {{reason}}\");", self.pm.name, port, operation));
        self.push(ind + 4, "}");
        self.push(ind, "}");
        let sig = self.hook_sig();
        pm_push_hook(
            &mut self.hooks,
            &hook,
            true,
            format!(
                "        /// Build the `{}.{}` input for this leg.{} `Skip` skips just this call — the leg continues.\n        async fn {}(&self, {}) -> Result<super::HookOutcome<crate::generated::services::{}{}Input>, domain::shared::errors::DomainError>;\n",
                port, operation, note_s, hook, sig, base, op
            ),
        );
    }

    fn emit_deliver(&mut self, event: &str, to: &str, with: &[(String, PmVal)], note: &Option<String>, ind: usize) {
        if self.admission_pending {
            self.emit_admission(ind);
        }
        let model = self.ctx.model;
        let var = snake_type(event);
        let covered = pm_deliver_covered(model, "events.yaml", event, with);
        let note_s = note.as_deref().map(|n| format!(" — {}", ws1(n))).unwrap_or_default();
        self.push(ind, &format!("// deliver {} → {} (the aggregate records the fact){}", event, to, note_s));
        if covered {
            let lit = pm_payload_literal(
                self.ctx,
                &self.scope(None),
                "events.yaml",
                event,
                with,
                &format!("domain::generated::events::{}", event),
                &var,
                &" ".repeat(ind),
                &format!("processmanager.yaml#/{}/{}", self.pm.name, event),
            );
            self.body.push_str(&lit);
        } else {
            assert!(!self.is_cmd, "builder-hook delivers on command legs are not supported");
            self.push(ind, &format!("let {} = match hooks.build_{}({}).await? {{", var, var, self.hook_args()));
            self.push(ind + 4, "super::HookOutcome::Ready(v) => v,");
            self.push(ind + 4, "super::HookOutcome::Skip(reason) => return Ok(Outcome::Skipped(reason)),");
            self.push(ind, "};");
            let withs = with.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>().join(", ");
            let sig = self.hook_sig();
            pm_push_hook(
                &mut self.hooks,
                &format!("build_{}", var),
                true,
                format!(
                    "        /// Build the FULL `{}` payload — the DSL `with` covers only [{}]; the rest is computed\n        /// (spec note:{}). `Skip` ends the leg as a benign no-op; `Err` aborts and surfaces.\n        async fn build_{}(&self, {}) -> Result<super::HookOutcome<domain::generated::events::{}>, domain::shared::errors::DomainError>;\n",
                    event, withs, note_s, var, sig, event
                ),
            );
        }
        self.payloads.push(event.to_string());
        self.hook_ctx.push((var.clone(), format!("&domain::generated::events::{}", event), format!("&{}", var)));
        // Stream addressing: the target aggregate's key, from the payload, the state row, or a read.
        let key_prop = pm_aggregate_key(to);
        let key_snake = snake_field(&key_prop);
        let in_payload = model
            .defs
            .get("events.yaml")
            .and_then(|f| f.get(event))
            .and_then(|n| n.get("properties"))
            .map(|p| p.get(key_prop.as_str()).is_some())
            .unwrap_or(false);
        let append = |gen: &mut Self, key_expr: &str, ind: usize| {
            gen.push(ind, &format!("let stream = format!(\"{}-{{}}\", {}.0);", to, key_expr));
            gen.push(ind, "let (stream_events, stream_version) = store.load(&stream).await?;");
            gen.push(ind, &format!("if hooks.should_deliver_{}(&stream_events, &{}) {{", var, var));
            gen.push(ind + 4, "crate::repository::Repository::new(store)");
            gen.push(
                ind + 8,
                &format!(
                    ".save(&stream, stream_version, &[domain::generated::events::DomainEvent::{}({}.clone())], {})",
                    event, var, gen.actor_ref()
                ),
            );
            gen.push(ind + 8, ".await?;");
            gen.push(ind, "}");
        };
        if in_payload {
            append(self, &format!("{}.{}", var, rust_ident(&key_snake)), ind);
        } else {
            // Fall back to the state row, then to a read alias — `None` skips this deliver (the run
            // has no target payment/stream to address; the remaining steps still run).
            let (holder, ty) = if self.row_in_scope && self.ctx.table.map(|t| t.columns.iter().any(|c| c.name == key_snake)).unwrap_or(false) {
                (format!("row.{}", rust_ident(&key_snake)), self.ctx.col_ty(&key_snake))
            } else if let Some(r) = self.ctx.reads.iter().find(|r| r.fields.iter().any(|f| f.col == key_snake)) {
                (format!("{}.{}", rust_ident(&r.alias), rust_ident(&key_snake)), self.ctx.read_field_ty(&r.alias, &key_snake))
            } else {
                panic!("processmanager.yaml#/{}: cannot address {} stream for {}", self.pm.name, to, event)
            };
            if ty.starts_with("Option<") {
                self.push(ind, &format!("if let Some(deliver_key) = {}.clone() {{", holder));
                append(self, "deliver_key", ind + 4);
                self.push(ind, "}");
            } else {
                append(self, &holder, ind);
            }
        }
        let event_owned = event.to_string();
        pm_push_hook(
            &mut self.hooks,
            &format!("should_deliver_{}", var),
            false,
            format!(
                "        /// Per-aggregate idempotency predicate: given the target stream as loaded, should this\n        /// `{}` be appended? (A re-delivered trigger must find the fact already recorded.)\n        fn should_deliver_{}(&self, stream: &[domain::generated::events::DomainEvent], event: &domain::generated::events::{}) -> bool;\n",
                event_owned, var, event_owned
            ),
        );
    }

    fn emit_send(&mut self, command: &str, with: &[(String, PmVal)], for_each: &Option<String>, note: &Option<String>, ind: usize) {
        assert!(!self.is_cmd, "send steps on command legs are not generated");
        if self.admission_pending {
            self.emit_admission(ind);
        }
        let cmd_snake = snake_type(command);
        assert!(
            pm_deliver_covered(self.ctx.model, "commands.yaml", command, with),
            "processmanager.yaml#/{}: send {} payload is not fully covered by `with`",
            self.pm.name, command
        );
        let note_s = note.as_deref().map(|n| format!(" — {}", ws1(n))).unwrap_or_default();
        self.push(
            ind,
            &format!("// send {} (the target validates and may reject; a rejection is logged and skipped){}", command, note_s),
        );
        let (body_ind, loop_alias) = if let Some(alias) = for_each {
            self.push(ind, &format!("for item in &{} {{", rust_ident(alias)));
            (ind + 4, Some(alias.clone()))
        } else {
            (ind, None)
        };
        let lit = pm_payload_literal(
            self.ctx,
            &self.scope(loop_alias.clone()),
            "commands.yaml",
            command,
            with,
            &format!("domain::generated::commands::{}", command),
            "sent",
            &" ".repeat(body_ind),
            &format!("processmanager.yaml#/{}/{}", self.pm.name, command),
        );
        self.body.push_str(&lit);
        self.push(body_ind, &format!("match crate::commands::{}(store, sent, &actor).await {{", cmd_snake));
        self.push(body_ind + 4, "Ok(()) => {}");
        self.push(body_ind + 4, "Err(e) if crate::ports::is_version_conflict(&e) => return Err(e),");
        self.push(body_ind + 4, "Err(domain::shared::errors::DomainError::Repository(e)) => {");
        self.push(body_ind + 8, "return Err(domain::shared::errors::DomainError::Repository(e))");
        self.push(body_ind + 4, "}");
        self.push(body_ind + 4, "Err(rejection) => {");
        if for_each.is_some() {
            self.push(
                body_ind + 8,
                &format!(
                    "eprintln!(\"saga[{}]: {} rejected ({{rejection}}) — skipped, the target aggregate's own invariants stand\");",
                    self.pm.name, command
                ),
            );
        } else {
            self.push(
                body_ind + 8,
                &format!(
                    "let reason = format!(\"{} rejected: {{rejection}} — the target aggregate's own invariants stand; skipped\");",
                    command
                ),
            );
            self.push(body_ind + 8, &format!("eprintln!(\"saga[{}]: {{reason}}\");", self.pm.name));
            self.push(body_ind + 8, "leg_outcome = Outcome::Skipped(reason);");
        }
        self.push(body_ind + 4, "}");
        self.push(body_ind, "}");
        if for_each.is_some() {
            self.push(ind, "}");
        }
    }

    fn emit_state(&mut self, i: usize, by: &[(String, PmVal)], expect: &[(String, String)], set: &[(String, PmVal)], note: &Option<String>, consumed: &BTreeSet<usize>, ind: usize) {
        let model = self.ctx.model;
        let note_s = note.as_deref().map(|n| format!(" {}", ws1(n))).unwrap_or_default();
        if !by.is_empty() {
            assert!(by.len() == 1, "state.by with multiple columns unsupported");
            let (col, val) = &by[0];
            let raw = self.raw_msg_expr(val, "state.by");
            let c = self.ctx.col(col);
            let by_ref = pm_ty(model, &self.table().table, &c.name, &c.ty).by_ref();
            let arg = if by_ref { format!("&{}", raw) } else { pm_clone_if_needed(model, &self.ctx.col_ty(col), raw.clone()) };
            let method = pm_lookup_method(col);
            let table_name = self.table().table.clone();
            self.push(ind, &format!("// state.by {} — load the run this trigger correlates to.{}", col, note_s));
            // Missing-row policy: a following bare `guard throws` (or, on a command leg, the first
            // `that`-guard's error) types the orphan; otherwise absence is a benign event-leg skip.
            let missing_err = match self.leg.steps.get(i + 1) {
                Some(PmStepDef::Guard { that: None, throws: Some(e), .. }) if consumed.contains(&(i + 1)) => Some(e.clone()),
                Some(PmStepDef::Guard { that: Some((s, _, _)), throws: Some(e), .. }) if self.is_cmd && s == "state" => Some(e.clone()),
                _ => None,
            };
            let key = serde_camel(col);
            self.push(ind, &format!("let Some(row) = state.{}({}).await? else {{", method, arg));
            match missing_err {
                Some(e) => self.push(
                    ind + 4,
                    &format!(
                        "return Err(domain::shared::errors::DomainError::rejected(\"{}\", serde_json::json!({{ \"{}\": &{} }})));",
                        e, key, raw
                    ),
                ),
                None => {
                    assert!(!self.is_cmd, "command-leg state.by needs a typed missing-row error");
                    self.push(
                        ind + 4,
                        &format!(
                            "return Ok(Outcome::Skipped(format!(\"no {} run for {} {{:?}} — {}\", {})));",
                            table_name, col, pm_str_lit(note_s.trim()), raw
                        ),
                    );
                }
            }
            self.push(ind, "};");
            self.row_in_scope = true;
            self.by_ctx = Some((key, raw));
            let row_ty = self.row_ty().to_string();
            self.hook_ctx.push(("row".to_string(), format!("&{}", row_ty), "&row".to_string()));
        }
        for (col, member) in expect {
            assert!(!self.is_cmd, "state.expect on a command leg has no benign-skip path");
            assert!(self.row_in_scope, "state.expect with no row in scope");
            let ty = self.ctx.col_ty(col);
            let table_name = self.table().table.clone();
            self.push(ind, &format!("// state.expect {} = {} — a failed expect is a benign skip.{}", col, member, note_s));
            self.push(ind, &format!("if row.{} != domain::generated::scalars::{}::{} {{", rust_ident(col), ty, member));
            self.push(
                ind + 4,
                &format!(
                    "return Ok(Outcome::Skipped(format!(\"{} run is {{:?}}, expected {} — {}\", row.{})));",
                    table_name, member, pm_str_lit(note_s.trim()), rust_ident(col)
                ),
            );
            self.push(ind, "}");
        }
        if !set.is_empty() {
            if self.admission_pending {
                self.emit_admission(ind);
            }
            let row_ty = self.row_ty().to_string();
            self.push(ind, &format!("// state.set — upsert the run row (envelope stamps last_update_utc).{}", note_s));
            // Self-referential `from_state` = orchestrator-computed (arithmetic the DSL cannot carry).
            let mut lines: Vec<String> = Vec::new();
            for (col, val) in set {
                if let PmVal::FromHook(name) = val {
                    // Orchestrator-resolved value (#60) — a runtime hook (e.g. reading config tables)
                    // usable on ANY leg incl. a birth leg with no state row. It receives whatever the
                    // leg has in scope (trigger + reads/payloads + the row when one is loaded).
                    let base = pm_qualify(model, &self.ctx.col_ty(col));
                    let sig = self.hook_sig();
                    let args = self.hook_args();
                    self.push(ind, &format!("let hook_{} = hooks.{}({}).await?;", rust_ident(col), name, args));
                    pm_push_hook(
                        &mut self.hooks,
                        name,
                        true,
                        format!(
                            "        /// Orchestrator-resolved `{}` (#60, the DSL's `from_hook` marker — resolved at runtime,\n        /// e.g. from the delivery-strategy config tables).{}\n        async fn {}(&self, {}) -> Result<{}, domain::shared::errors::DomainError>;\n",
                            col, note_s, name, sig, base
                        ),
                    );
                    lines.push(format!("{}: hook_{},", rust_ident(col), rust_ident(col)));
                    continue;
                }
                if let PmVal::FromState(src) = val {
                    if src == col {
                        let base = {
                            let c = self.ctx.col(col);
                            pm_ty(model, &self.table().table, &c.name, &c.ty).field()
                        };
                        self.push(ind, &format!("let computed_{} = hooks.compute_{}(&row);", rust_ident(col), col));
                        pm_push_hook(
                            &mut self.hooks,
                            &format!("compute_{}", col),
                            false,
                            format!(
                                "        /// Orchestrator-computed `{}` (the DSL's self-referential `from_state` marker — the\n        /// spec note carries the arithmetic).{}\n        fn compute_{}(&self, row: &{}) -> {};\n",
                                col, note_s, col, row_ty, base
                            ),
                        );
                        lines.push(format!("{}: computed_{},", rust_ident(col), rust_ident(col)));
                        continue;
                    }
                }
                let target = self.ctx.col_ty(col);
                let expr = pm_value_expr(
                    self.ctx,
                    &self.scope(None),
                    val,
                    &target,
                    &format!("processmanager.yaml#/{}/state.set/{}", self.pm.name, col),
                );
                lines.push(format!("{}: {},", rust_ident(col), expr));
            }
            self.push(ind, &format!("let mut updated = {} {{", row_ty));
            for l in &lines {
                self.push(ind + 4, l);
            }
            if self.row_in_scope {
                self.push(ind + 4, "..row");
            } else {
                // A full literal: unset nullable columns default to None; the envelope stamps the time.
                let set_cols: BTreeSet<&str> = set.iter().map(|(c, _)| c.as_str()).collect();
                let t = self.ctx.table.expect("state.set without state_table");
                for c in &t.columns {
                    if set_cols.contains(c.name.as_str()) {
                        continue;
                    }
                    if c.name == "last_update_utc" {
                        self.push(ind + 4, "// Ignored on write — the store stamps now() (runtime envelope).");
                        self.push(ind + 4, "last_update_utc: chrono::Utc::now(),");
                    } else if c.nullable {
                        self.push(ind + 4, &format!("{}: None,", rust_ident(&c.name)));
                    } else {
                        panic!(
                            "processmanager.yaml#/{}: opening state.set leaves non-nullable '{}' unset",
                            self.pm.name, c.name
                        );
                    }
                }
            }
            self.push(ind, "};");
            self.push(ind, "hooks.finalize(&mut updated);");
            self.push(ind, "state.upsert(&updated).await?;");
            pm_push_hook(
                &mut self.hooks,
                "finalize",
                false,
                format!(
                    "        /// Envelope-owned fix-ups on the row about to be upserted (e.g. clearing a spent\n        /// credential, ADR-20260720-015500) — default: none.\n        fn finalize(&self, _row: &mut {}) {{}}\n",
                    row_ty
                ),
            );
        }
    }
}

/// Emit `crates/application/src/generated/process_managers.rs` — the process-manager ORCHESTRATOR
/// STEP PIPELINES (issue #25, codegen-roadmap item 3): one module per PM of `specs/processmanager.yaml`,
/// one generated `async fn` per leg executing the DSL's ordered typed steps (state `by`/`expect`/`set`
/// with the pk-admission seam, structural guards, port calls, deliver/send with the event-leg
/// rejection-skip semantics, skip/throw plumbing), delegating the NON-STRUCTURAL seams (reads over
/// projections, computed payloads, idempotency predicates, branch arithmetic, envelope row fix-ups)
/// to per-leg hook traits implemented by `crate::process_managers` (roadmap: "hand-written only the
/// non-structural predicates behind generated hook traits").
/// Emit one leg: `(hooks trait, pipeline fn)` — both nested inside the PM's module (indent 4).
fn emit_pm_leg(ctx: &PmEmit, pm: &PmOrchDef, leg: &PmLegDef) -> (String, String) {
    let is_cmd = leg.msg_file == "commands.yaml";
    let msg_var: &'static str = if is_cmd { "cmd" } else { "event" };
    let msg_snake = snake_type(&leg.msg);
    let msg_path = format!(
        "domain::generated::{}::{}",
        if is_cmd { "commands" } else { "events" },
        leg.msg
    );
    let fn_name = if is_cmd { msg_snake.clone() } else { format!("on_{}", msg_snake) };
    let trait_name = format!("{}Hooks", leg.msg);

    let has_deliver = leg.steps.iter().any(|s| matches!(s, PmStepDef::Deliver { .. }));
    let has_send = leg.steps.iter().any(|s| matches!(s, PmStepDef::Send { .. }));
    let store_needed = has_deliver || has_send;
    let has_by = leg
        .steps
        .iter()
        .any(|s| matches!(s, PmStepDef::StateStep { by, .. } if !by.is_empty()));
    let has_set = leg
        .steps
        .iter()
        .any(|s| matches!(s, PmStepDef::StateStep { set, .. } if !set.is_empty()));
    let uses_envelope = leg.steps.iter().any(|s| match s {
        PmStepDef::StateStep { set, .. } => set.iter().any(|(_, v)| matches!(v, PmVal::FromEnvelope(_))),
        _ => false,
    });
    let has_call = leg.steps.iter().any(|s| matches!(s, PmStepDef::Call { .. }));
    let env_needed = !is_cmd && (store_needed || uses_envelope || has_call);
    let single_send = leg
        .steps
        .iter()
        .any(|s| matches!(s, PmStepDef::Send { for_each: None, .. }));
    let admission = !is_cmd && !has_by && has_set && ctx.table.is_some();
    let mut ports_used: Vec<String> = Vec::new();
    for s in &leg.steps {
        if let PmStepDef::Call { port, .. } = s {
            if !ports_used.contains(port) {
                ports_used.push(port.clone());
            }
        }
    }

    let row_ty = ctx.table.map(|t| format!("crate::pm_state::{}Row", t.base));
    let mut gen = PmLegGen {
        ctx,
        pm,
        leg,
        is_cmd,
        msg_var,
        msg_path: msg_path.clone(),
        row_ty,
        body: String::new(),
        hooks: Vec::new(),
        row_in_scope: false,
        payloads: Vec::new(),
        hook_ctx: vec![(
            msg_var.to_string(),
            format!("&{}", msg_path),
            if is_cmd { format!("&{}", msg_var) } else { msg_var.to_string() },
        )],
        by_ctx: None,
        admission_pending: admission,
        single_send,
    };

    // Consumed presence guards: a bare `guard throws` right after `state.by` IS the missing-row policy.
    let mut consumed: BTreeSet<usize> = BTreeSet::new();
    for (i, s) in leg.steps.iter().enumerate() {
        if let PmStepDef::StateStep { by, .. } = s {
            if !by.is_empty() {
                if let Some(PmStepDef::Guard { that: None, throws: Some(_), .. }) = leg.steps.get(i + 1) {
                    consumed.insert(i + 1);
                }
            }
        }
    }
    // A mid-leg bare `guard skip` is the DSL's linear-branch marker (bounded re-offer): the hook
    // decides which branch runs — the steps before the marker (then end), or the steps after it.
    let branch_at = leg.steps.iter().enumerate().find_map(|(i, s)| match s {
        PmStepDef::Guard { that: None, throws: None, skip: true, .. } if !consumed.contains(&i) => Some(i),
        _ => None,
    });

    let emit_one = |gen: &mut PmLegGen, i: usize, step: &PmStepDef, ind: usize, consumed: &BTreeSet<usize>| match step {
        PmStepDef::Read { table, alias, where_, note } => gen.emit_read(table, alias, where_, note, ind),
        PmStepDef::Guard { .. } if consumed.contains(&i) => {}
        PmStepDef::Guard { that: Some((s, f, m)), throws, skip, note } => {
            gen.emit_guard_that(s, f, m, throws, *skip, note, ind)
        }
        PmStepDef::Guard { that: None, .. } => {
            panic!("processmanager.yaml#/{}/{}: non-structural guard outside the supported positions", pm.name, leg.msg)
        }
        PmStepDef::Call { port, operation, note } => gen.emit_call(port, operation, note, ind),
        PmStepDef::Deliver { event, to, with, note } => gen.emit_deliver(event, to, with, note, ind),
        PmStepDef::Send { command, with, for_each, note } => gen.emit_send(command, with, for_each, note, ind),
        PmStepDef::StateStep { by, expect, set, note } => gen.emit_state(i, by, expect, set, note, consumed, ind),
    };

    if let Some(k) = branch_at {
        let prefix_end = leg.steps[..k]
            .iter()
            .position(|s| match s {
                PmStepDef::Call { .. } | PmStepDef::Deliver { .. } | PmStepDef::Send { .. } => true,
                PmStepDef::StateStep { set, .. } => !set.is_empty(),
                _ => false,
            })
            .unwrap_or_else(|| panic!("processmanager.yaml#/{}/{}: branch marker with an empty first branch", pm.name, leg.msg));
        for (i, s) in leg.steps.iter().enumerate().take(prefix_end) {
            emit_one(&mut gen, i, s, 8, &consumed);
        }
        let note = match &leg.steps[k] {
            PmStepDef::Guard { note, .. } => note.clone(),
            _ => unreachable!(),
        };
        let note_s = note.as_deref().map(ws1).unwrap_or_default();
        // ASYNC branch (#60): the decision may read config (e.g. "does a next ranked channel remain?"),
        // so the hook is async + fallible and receives the leg's scope (trigger + the loaded row).
        let branch_sig = gen.hook_sig();
        let branch_args = gen.hook_args();
        pm_push_hook(
            &mut gen.hooks,
            "branch",
            true,
            format!(
                "        /// The DSL's linear-branch decision (a mid-leg bare `skip` guard): `true` runs the steps\n        /// BEFORE the marker and ends the leg; `false` falls through to the steps after it. Resolved at\n        /// runtime (may read config, #60).\n        /// Spec note: {}\n        async fn branch(&self, {}) -> Result<bool, domain::shared::errors::DomainError>;\n",
                pm_str_lit(&note_s), branch_sig
            ),
        );
        gen.push(8, "// Linear-branch marker (bare `skip` guard): the hook chooses the branch.");
        gen.push(8, &format!("if hooks.branch({}).await? {{", branch_args));
        for (i, s) in leg.steps.iter().enumerate().take(k).skip(prefix_end) {
            emit_one(&mut gen, i, s, 12, &consumed);
        }
        gen.push(12, "return Ok(Outcome::Completed);");
        gen.push(8, "}");
        for (i, s) in leg.steps.iter().enumerate().skip(k + 1) {
            emit_one(&mut gen, i, s, 8, &consumed);
        }
    } else {
        for (i, s) in leg.steps.iter().enumerate() {
            emit_one(&mut gen, i, s, 8, &consumed);
        }
    }

    // ── assemble the trait ──
    let any_async = gen.hooks.iter().any(|h| h.is_async);
    let desc = leg
        .description
        .as_deref()
        .map(|d| format!("\n    /// {}", ws1(d)))
        .unwrap_or_default();
    let mut trait_code = format!(
        "\n    /// Non-structural hooks for the generated `{}` leg — the seams the step DSL cannot express\n    /// (reads, computed payloads, idempotency predicates, envelope fix-ups).{}\n",
        leg.msg, desc
    );
    if any_async {
        trait_code.push_str("    #[async_trait::async_trait]\n");
    }
    trait_code.push_str(&format!("    pub trait {}: Send + Sync {{\n", trait_name));
    for (i, h) in gen.hooks.iter().enumerate() {
        if i > 0 {
            trait_code.push('\n');
        }
        trait_code.push_str(&h.code);
    }
    trait_code.push_str("    }\n");

    // ── assemble the fn ──
    let kind = if is_cmd { "COMMAND" } else { "EVENT" };
    let mut fn_code = format!(
        "\n    /// {} leg `{}#/{}` — generated step pipeline (issue #25).{}\n    pub async fn {}(\n",
        kind, leg.msg_file, leg.msg, desc.replace("\n    ///", "\n    ///"), fn_name
    );
    if store_needed {
        fn_code.push_str("        store: &dyn crate::ports::EventStore,\n");
    }
    if let Some(t) = ctx.table {
        fn_code.push_str(&format!("        state: &dyn crate::pm_state::{}StateStore,\n", t.base));
    }
    for port in &ports_used {
        let svc = pm.ports.iter().find(|(p, _)| p == port).map(|(_, s)| s.clone()).unwrap();
        fn_code.push_str(&format!(
            "        {}: &dyn crate::generated::services::{}Service,\n",
            rust_ident(port),
            pm_pascal(&svc)
        ));
    }
    fn_code.push_str(&format!("        hooks: &dyn {},\n", trait_name));
    if is_cmd {
        fn_code.push_str(&format!("        cmd: {},\n", msg_path));
        fn_code.push_str("        actor: &crate::ports::Actor,\n");
    } else {
        fn_code.push_str(&format!("        event: &{},\n", msg_path));
        if env_needed {
            fn_code.push_str("        env: &crate::process_managers::TriggerEnvelope,\n");
        }
    }
    let ret = if is_cmd {
        "Result<(), domain::shared::errors::DomainError>"
    } else {
        "Result<crate::process_managers::Outcome, domain::shared::errors::DomainError>"
    };
    fn_code.push_str(&format!("    ) -> {} {{\n", ret));
    if !is_cmd {
        fn_code.push_str("        use crate::process_managers::Outcome;\n");
        if store_needed {
            fn_code.push_str("        let actor = crate::process_managers::saga_actor(env);\n");
        }
        if single_send {
            fn_code.push_str("        let mut leg_outcome = Outcome::Completed;\n");
        }
    }
    fn_code.push_str(&gen.body);
    if is_cmd {
        fn_code.push_str("        Ok(())\n");
    } else if single_send {
        fn_code.push_str("        Ok(leg_outcome)\n");
    } else {
        fn_code.push_str("        Ok(Outcome::Completed)\n");
    }
    fn_code.push_str("    }\n");
    (trait_code, fn_code)
}

fn emit_pm_orchestrators(model: &Model) -> String {
    let tables = parse_pm_tables(model);
    let pms = parse_pm_orchestrators(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/processmanager.yaml — do not edit by hand.\n\
         // Process-manager ORCHESTRATOR STEP PIPELINES (issue #25, ADR-20260719-172821/-193500): one module\n\
         // per process manager, one generated `async fn` per `receives` leg executing the DSL's ordered typed\n\
         // steps. The pipeline (control flow, state row lifecycle, structural guards, deliver/send plumbing,\n\
         // skip/throw semantics) is GENERATED; the non-structural seams the DSL cannot express (projection\n\
         // reads, computed payloads such as frozen snapshots and derived ids, per-aggregate idempotency\n\
         // predicates, the bounded re-offer branch, envelope-owned row fix-ups) are per-leg HOOK traits,\n\
         // implemented next to the wrappers in `crate::process_managers`. The PlaceOrder COMMAND leg stays\n\
         // hand-written (`crate::commands::place_order`) — server-side pricing is a codegen non-goal.\n\n\
         /// How a hook resolves: the value is ready, or the leg (or just this call) ends as a benign skip.\n\
         pub enum HookOutcome<T> {\n    Ready(T),\n    Skip(String),\n}\n",
    );
    for pm in &pms {
        let table = pm.state_table.as_ref().map(|t| {
            tables
                .iter()
                .find(|pt| pt.table == *t)
                .unwrap_or_else(|| panic!("processmanager.yaml#/{}: unknown state_table '{}'", pm.name, t))
        });
        let ctx = PmEmit { model, table, reads: pm_read_infos(model, pm, table) };
        let module = snake_type(&pm.name);
        out.push_str(&format!(
            "\n/// Generated step pipelines for `processmanager.yaml#/{}`.\npub mod {} {{\n",
            pm.name, module
        ));
        // Read-result structs — one per alias, fields typed by their sinks.
        for r in &ctx.reads {
            out.push_str(&format!(
                "\n    /// Columns of `{}` consumed by this PM's `read {}` step, typed by their consumers.\n    #[derive(Debug, Clone, PartialEq)]\n    pub struct {}Read {{\n",
                r.table, r.alias, pm_pascal(&r.alias)
            ));
            for f in &r.fields {
                out.push_str(&format!("        /// {}\n        pub {}: {},\n", f.doc, rust_ident(&f.col), pm_qualify(model, &f.ty)));
            }
            out.push_str("    }\n");
        }
        for leg in &pm.legs {
            let (trait_code, fn_code) = emit_pm_leg(&ctx, pm, leg);
            out.push_str(&trait_code);
            out.push_str(&fn_code);
        }
        out.push_str("}\n");
    }
    out
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// crates/application/src/generated/behaviour_tests.rs — the GENERATED behaviour-test suite
// (issue #24, codegen-roadmap item 2): one #[tokio::test] per tests.yaml Given/When/Then case, so
// the spec IS the executable suite. GIVEN seeds each fact onto its aggregate's stream (the
// TestBed mirrors read-model/PM-run effects), WHEN dispatches the command/event through the real
// write path (the same handlers/legs production uses), THEN asserts the appended facts equal the
// spec payloads (strict per-stream diff; `then: []` asserts a strict no-op) and `thrown` asserts
// the typed rejection code. The runtime the suite runs on is the hand-written
// `application::behaviour_support` (playbook: a failing behaviour test means fixing that runtime
// or this emitter — never the spec).
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// Aggregate-actor metadata the specs do not carry as data: the stream category (= the actor
/// name), the payload property that keys the aggregate's stream, and whether that id scalar is a
/// UUID (mapped through `support::uid`) or a plain string (used verbatim in the stream key).
/// Mirrors the domain `Aggregate` impls; a NEW aggregate actor must be added here — generation
/// panics otherwise, so the gap cannot pass silently.
const BT_AGGREGATES: &[(&str, &str, bool)] = &[
    ("RestaurantAccount", "restaurantAccountId", true),
    ("Restaurant", "restaurantId", true),
    ("Prospect", "restaurantId", true),
    ("Catalog", "catalogId", true),
    ("Customer", "customerId", true),
    ("Cart", "cartId", true),
    ("Order", "orderId", true),
    ("Payment", "paymentIntentId", false),
    ("DeliveryJob", "deliveryJobId", true),
    ("Rider", "riderId", true),
    ("DeliveryPartnerRegistration", "registrationId", true),
];

fn bt_agg(actor: &str) -> Option<(&'static str, &'static str, bool)> {
    BT_AGGREGATES.iter().copied().find(|(a, _, _)| *a == actor)
}

/// event name → owning AGGREGATE actor (the stream its recorded fact lives on), built from
/// actors.yaml: an aggregate owns every event it emits (and every event it receives as an inbound
/// fact). Ambiguity (two aggregates claiming one event) is a generation error.
fn bt_event_owners(model: &Model) -> BTreeMap<String, &'static str> {
    let mut owners: BTreeMap<String, &'static str> = BTreeMap::new();
    for (agg, _, _) in BT_AGGREGATES {
        let def = model
            .defs
            .get("actors.yaml")
            .and_then(|m| m.get(*agg))
            .unwrap_or_else(|| panic!("behaviour-tests: actors.yaml#/{} missing", agg));
        let receives = def.get("receives").and_then(|r| r.as_sequence()).cloned().unwrap_or_default();
        let mut claim = |event: String| {
            if let Some(prev) = owners.get(&event) {
                assert_eq!(
                    prev, agg,
                    "behaviour-tests: event {} claimed by two aggregates ({} and {})",
                    event, prev, agg
                );
            }
            owners.insert(event, agg);
        };
        for entry in &receives {
            if let Some(msg) = entry.get("message").and_then(|m| m.get("$ref")).and_then(|x| x.as_str()) {
                if msg.starts_with("events.yaml#/") {
                    if let Some(name) = ref_name(msg) {
                        claim(name);
                    }
                }
            }
            for e in entry.get("emits").and_then(|e| e.as_sequence()).cloned().unwrap_or_default() {
                if let Some(name) = e.get("$ref").and_then(|x| x.as_str()).and_then(ref_name) {
                    claim(name);
                }
            }
        }
    }
    owners
}

/// Is `name` a scalars.yaml def (vs an entities.yaml value object)?
fn bt_is_scalar(model: &Model, name: &str) -> bool {
    model.defs.get("scalars.yaml").map(|m| m.get(name).is_some()).unwrap_or(false)
}

/// Render one yaml float/int as a Rust f64 literal (always with a decimal point).
fn bt_f64_lit(v: &Value) -> String {
    if let Some(i) = v.as_i64() {
        return format!("{}.0", i);
    }
    let f = v.as_f64().expect("behaviour-tests: numeric literal expected");
    let s = format!("{}", f);
    if s.contains('.') || s.contains('e') {
        s
    } else {
        format!("{}.0", s)
    }
}

/// Render a scalars.yaml-typed sample value as its Rust expression (`sc::` qualified).
fn bt_scalar_expr(model: &Model, name: &str, val: &Value, path: &str) -> String {
    let def = model
        .defs
        .get("scalars.yaml")
        .and_then(|m| m.get(name))
        .unwrap_or_else(|| panic!("behaviour-tests: scalars.yaml#/{} missing ({})", name, path));
    if def.get("enum").is_some() {
        let v = val.as_str().unwrap_or_else(|| panic!("behaviour-tests: {}: enum value must be a string", path));
        return format!("sc::{}::{}", name, v);
    }
    let ty = def.get("type").and_then(|t| t.as_str()).unwrap_or("string");
    if def.get("format").and_then(|f| f.as_str()) == Some("uuid") {
        let s = val.as_str().unwrap_or_else(|| panic!("behaviour-tests: {}: uuid sample must be a string", path));
        return format!("sc::{}(support::uid(\"{}\"))", name, rust_string_lit(s));
    }
    match ty {
        "integer" => format!("sc::{}({})", name, val.as_i64().unwrap_or_else(|| panic!("behaviour-tests: {}: integer expected", path))),
        "number" => format!("sc::{}({})", name, bt_f64_lit(val)),
        _ => {
            let s = val.as_str().unwrap_or_else(|| panic!("behaviour-tests: {}: string expected", path));
            format!("sc::{}(\"{}\".into())", name, rust_string_lit(s))
        }
    }
}

/// Render one property VALUE (no optionality wrapping) as a Rust expression. `ctx` is the spec
/// file the surrounding schema came from (file-relative `$ref`s resolve against it).
fn bt_value_expr(model: &Model, ctx: &str, node: &Value, val: &Value, path: &str) -> String {
    if let Some(rf) = node.get("$ref").and_then(|x| x.as_str()) {
        let name = ref_name(rf).unwrap_or_else(|| panic!("behaviour-tests: {}: malformed $ref", path));
        if bt_is_scalar(model, &name) {
            return bt_scalar_expr(model, &name, val, path);
        }
        let def = resolve_ref(model, rf, ctx)
            .unwrap_or_else(|| panic!("behaviour-tests: {}: unresolvable $ref {}", path, rf));
        let next_ctx = match rf.split_once("#/") {
            Some((f, _)) if !f.is_empty() => f.to_string(),
            _ => ctx.to_string(),
        };
        let module = match next_ctx.as_str() {
            "entities.yaml" => "ent",
            "commands.yaml" => "cmds",
            "events.yaml" => "evs",
            other => panic!("behaviour-tests: {}: struct $ref into unsupported file {}", path, other),
        };
        return bt_struct_expr(model, &next_ctx, &format!("{}::{}", module, name), def, val, path);
    }
    match node.get("type").and_then(|t| t.as_str()) {
        Some("array") => {
            let items = node.get("items").unwrap_or_else(|| panic!("behaviour-tests: {}: array without items", path));
            let seq = val.as_sequence().unwrap_or_else(|| panic!("behaviour-tests: {}: sequence expected", path));
            let parts: Vec<String> = seq
                .iter()
                .enumerate()
                .map(|(i, item)| bt_value_expr(model, ctx, items, item, &format!("{}[{}]", path, i)))
                .collect();
            format!("vec![{}]", parts.join(", "))
        }
        Some("string") => format!("\"{}\".to_string()", rust_string_lit(val.as_str().unwrap_or_else(|| panic!("behaviour-tests: {}: string expected", path)))),
        Some("integer") => format!("{}", val.as_i64().unwrap_or_else(|| panic!("behaviour-tests: {}: integer expected", path))),
        Some("boolean") => format!("{}", val.as_bool().unwrap_or_else(|| panic!("behaviour-tests: {}: boolean expected", path))),
        Some("number") => bt_f64_lit(val),
        other => panic!("behaviour-tests: {}: unsupported inline type {:?}", path, other),
    }
}

/// Render a sample `data` object as a Rust struct literal for a spec node with
/// `properties`/`required` — properties in spec order, absent optionals `None`, absent arrays
/// `Vec::new()` (the same optionality rules the struct emitters use).
fn bt_struct_expr(model: &Model, ctx: &str, qualified: &str, def: &Value, val: &Value, path: &str) -> String {
    let props = def
        .get("properties")
        .and_then(|p| p.as_mapping())
        .unwrap_or_else(|| panic!("behaviour-tests: {}: schema has no properties", path));
    let required: HashSet<&str> = def
        .get("required")
        .and_then(|r| r.as_sequence())
        .map(|s| s.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let obj = val.as_mapping();
    let mut fields = Vec::new();
    for (k, pnode) in props {
        let prop = k.as_str().expect("property key");
        let field = rust_ident(&snake_field(prop));
        let is_array = pnode.get("type").and_then(|t| t.as_str()) == Some("array");
        let optional = !required.contains(prop) || pnode.get("nullable").and_then(|n| n.as_bool()).unwrap_or(false);
        let sample = obj.and_then(|o| o.get(Value::String(prop.to_string())));
        let expr = match sample {
            Some(v) if !v.is_null() => {
                let inner = bt_value_expr(model, ctx, pnode, v, &format!("{}.{}", path, prop));
                if optional && !is_array {
                    format!("Some({})", inner)
                } else {
                    inner
                }
            }
            _ => {
                if is_array {
                    "Vec::new()".to_string()
                } else if optional {
                    "None".to_string()
                } else {
                    panic!("behaviour-tests: {}.{}: required property missing from sample data", path, prop)
                }
            }
        };
        fields.push(format!("{}: {}", field, expr));
    }
    format!("{} {{ {} }}", qualified, fields.join(", "))
}

/// snake_case test/fixture identifier from a PascalCase key.
fn bt_fn_name(key: &str) -> String {
    snake_field(key).trim_start_matches('_').to_string()
}

/// The stream EXPRESSION (Rust) for an aggregate + spec string id.
fn bt_stream_expr(agg: &str, uuid_keyed: bool, id: &str) -> String {
    if uuid_keyed {
        format!("format!(\"{}-{{}}\", support::uid(\"{}\"))", agg, rust_string_lit(id))
    } else {
        format!("\"{}-{}\".to_string()", agg, rust_string_lit(id))
    }
}

/// Resolve the stream of one event instance: owner aggregate + id (from the payload's id property,
/// else the test's running context for that aggregate, else the FIXTURE POOL's unique id). Updates
/// the context.
fn bt_event_stream(
    owners: &BTreeMap<String, &'static str>,
    pool: &BTreeMap<&'static str, BTreeSet<String>>,
    ctx: &mut BTreeMap<&'static str, String>,
    event: &str,
    data: Option<&Value>,
    where_: &str,
) -> (&'static str, String) {
    let agg = owners
        .get(event)
        .copied()
        .unwrap_or_else(|| panic!("behaviour-tests: {}: no aggregate owns event {}", where_, event));
    let (_, id_prop, _) = bt_agg(agg).expect("aggregate meta");
    let id = data
        .and_then(|d| d.get(id_prop))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| ctx.get(agg).cloned())
        .or_else(|| {
            let ids = pool.get(agg)?;
            if ids.len() == 1 {
                ids.iter().next().cloned()
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("behaviour-tests: {}: cannot key the {} stream for {}", where_, agg, event));
    ctx.insert(agg, id.clone());
    (agg, id)
}

/// The dispatch expression for a WHEN command (a `cmd` binding is in scope).
fn bt_command_call(cmd: &str) -> String {
    let snake = match cmd {
        "ConfigureGoogleBusinessProfileOrderLink" => "configure_gbp_order_link".to_string(),
        "VerifyGoogleBusinessProfileOrderLink" => "verify_gbp_order_link".to_string(),
        _ => bt_fn_name(cmd),
    };
    match cmd {
        "PlaceOrder" => "crate::commands::place_order(&bed.store, &bed.catalogs, &bed.payments, &bed.payment_pm, cmd, None, &support::actor()).await".to_string(),
        "ApproveRefund" => "crate::process_managers::refund::approve_refund(&bed.store, &bed.refund_pm, &bed.payments, cmd, &support::actor()).await".to_string(),
        "DenyRefund" => "crate::process_managers::refund::deny_refund(&bed.store, &bed.refund_pm, cmd, &support::actor()).await".to_string(),
        "RegisterRestaurant" | "CreateCatalog" | "AddProduct" | "UpdateProduct" | "MarkRestaurantAsFavorite" => {
            format!("crate::commands::{}(&bed.store, &bed.restaurants, cmd, &support::actor()).await", snake)
        }
        "ClaimRestaurantListing" | "OptOutRestaurantListing" => {
            format!("crate::commands::{}(&bed.store, &bed.ownership, cmd, &support::actor()).await", snake)
        }
        "VerifyGoogleBusinessProfileOrderLink" => {
            format!("crate::commands::{}(&bed.store, &bed.probe, cmd, &support::actor()).await", snake)
        }
        "AddCartLine" | "ChangeCartLineQuantity" => {
            format!("crate::commands::{}(&bed.store, &bed.catalogs, cmd, &support::actor()).await", snake)
        }
        "RecordProspectContact" => {
            format!("crate::commands::{}(&bed.store, &bed.prospection, cmd, &support::actor()).await", snake)
        }
        "RequestPhoneVerification" | "ConfirmEmailVerification" => {
            format!("crate::commands::{}(&bed.store, &bed.identity, cmd, &support::actor()).await", snake)
        }
        "VerifyPhone" | "RequestEmailVerification" | "RequestPhoneChange" | "ConfirmPhoneChange" => {
            format!("crate::commands::{}(&bed.store, &bed.identity, &bed.customers, cmd, &support::actor()).await", snake)
        }
        _ => format!("crate::commands::{}(&bed.store, cmd, &support::actor()).await", snake),
    }
}

/// The dispatch expression for a WHEN event on a PROCESS MANAGER (an `ev` binding is in scope).
fn bt_pm_event_call(pm: &str, event: &str) -> String {
    match (pm, event) {
        ("PlaceOrderProcess", "PaymentCaptured") => "crate::process_managers::place_order::on_payment_captured(&bed.store, &bed.payment_pm, &ev, &support::envelope()).await".into(),
        ("PlaceOrderProcess", "PaymentFailed") => "crate::process_managers::place_order::on_payment_failed(&bed.payment_pm, &ev, &support::envelope()).await".into(),
        ("RefundProcess", "OrderRejectedByRestaurant") => "crate::process_managers::refund::on_order_rejected(&bed.store, &bed.refund_pm, &bed.orders, &ev, &support::envelope()).await".into(),
        ("RefundProcess", "OrderCancelledByCustomer") => "crate::process_managers::refund::on_order_cancelled_by_customer(&bed.store, &bed.refund_pm, &bed.orders, &ev, &support::envelope()).await".into(),
        ("RefundProcess", "OrderCancelledByRestaurant") => "crate::process_managers::refund::on_order_cancelled_by_restaurant(&bed.store, &bed.refund_pm, &bed.orders, &ev, &support::envelope()).await".into(),
        ("RefundProcess", "RefundRequested") => "crate::process_managers::refund::on_refund_requested(&bed.store, &bed.refund_pm, &bed.orders, &ev, &support::envelope()).await".into(),
        ("RefundProcess", "PaymentRefunded") => "crate::process_managers::refund::on_payment_refunded(&bed.refund_pm, &ev).await".into(),
        ("CartBindingProcess", "CustomerIdentified") => "crate::process_managers::cart_binding::on_customer_identified(&bed.store, &bed.cart_pm, &bed.carts, &ev, &support::envelope()).await".into(),
        ("DeliveryDispatchProcess", "OrderMarkedReady") => "crate::process_managers::delivery_dispatch::on_order_marked_ready(&bed.store, &bed.dispatch_pm, &bed.orders, &bed.delivery, &bed.dispatch_config, &ev, &support::envelope()).await".into(),
        ("DeliveryDispatchProcess", "DeliveryAcceptedByPartner") => "crate::process_managers::delivery_dispatch::on_delivery_accepted_by_partner(&bed.dispatch_pm, &ev).await".into(),
        ("DeliveryDispatchProcess", "DeliveryRejectedByPartner") => "crate::process_managers::delivery_dispatch::on_delivery_rejected_by_partner(&bed.store, &bed.dispatch_pm, &bed.delivery, &bed.dispatch_config, &ev, &support::envelope()).await".into(),
        ("DeliveryDispatchProcess", "DeliveryEscalationRequested") => "crate::process_managers::delivery_dispatch::on_delivery_escalation_requested(&bed.store, &bed.dispatch_pm, &bed.delivery, &bed.dispatch_config, &ev, &support::envelope()).await".into(),
        ("DeliveryDispatchProcess", "DeliveryOfferTimedOut") => "crate::process_managers::delivery_dispatch::on_delivery_offer_timed_out(&bed.store, &bed.dispatch_pm, &bed.delivery, &bed.dispatch_config, &ev, &support::envelope()).await".into(),
        ("DeliveryDispatchProcess", "DeliveryStatusUpdated") => "crate::process_managers::delivery_dispatch::on_delivery_status_updated(&bed.store, &bed.dispatch_pm, &ev, &support::envelope()).await".into(),
        ("DeliveryDispatchProcess", "DeliveryCompleted") => "crate::process_managers::delivery_dispatch::on_delivery_completed(&bed.store, &bed.dispatch_pm, &ev, &support::envelope()).await".into(),
        _ => panic!("behaviour-tests: no dispatch entry for process-manager {} ← event {} — extend bt_pm_event_call", pm, event),
    }
}

/// Emit `crates/application/src/generated/behaviour_tests.rs`.
fn emit_behaviour_tests(model: &Model) -> String {
    let owners = bt_event_owners(model);
    let tests_doc = model.defs.get("tests.yaml").expect("tests.yaml");
    let fixtures = tests_doc.get("fixtures").and_then(|f| f.as_mapping()).cloned().unwrap_or_default();
    let tests = tests_doc.get("tests").and_then(|t| t.as_mapping()).cloned().unwrap_or_default();

    // The fixture pool's ids per aggregate — the fallback when an event payload does not carry its
    // aggregate's id (e.g. RefundApproved on the Payment stream).
    let mut pool: BTreeMap<&'static str, BTreeSet<String>> = BTreeMap::new();
    for (_, fx) in &fixtures {
        let event = match fx.get("type").and_then(|t| t.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name) {
            Some(e) => e,
            None => continue,
        };
        if let Some(agg) = owners.get(&event) {
            let (_, id_prop, _) = bt_agg(agg).expect("aggregate meta");
            if let Some(id) = fx.get("data").and_then(|d| d.get(id_prop)).and_then(|v| v.as_str()) {
                pool.entry(agg).or_default().insert(id.to_string());
            }
        }
    }

    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/tests.yaml — do not edit by hand.\n\
         // The behaviour suite (issue #24): one #[tokio::test] per Given/When/Then case — the spec IS\n\
         // the test suite. Runs on the hand-written `application::behaviour_support` runtime; when a\n\
         // test fails, fix that runtime or the emitter (tools/codegen-rs), never this file or the spec.\n\
         #![allow(dead_code)]\n\n\
         use domain::generated::commands as cmds;\n\
         use domain::generated::entities as ent;\n\
         use domain::generated::events as evs;\n\
         use domain::generated::events::DomainEvent;\n\
         use domain::generated::scalars as sc;\n\n\
         use crate::behaviour_support::{self as support, TestBed};\n\n",
    );

    // ── fixture constructors ──────────────────────────────────────────────────────────────────
    for (k, fx) in &fixtures {
        let name = k.as_str().expect("fixture key");
        let event = fx
            .get("type")
            .and_then(|t| t.get("$ref"))
            .and_then(|x| x.as_str())
            .and_then(ref_name)
            .unwrap_or_else(|| panic!("behaviour-tests: fixtures/{}: malformed type", name));
        let def = resolve_ref(model, &format!("events.yaml#/{}", event), "tests.yaml")
            .unwrap_or_else(|| panic!("behaviour-tests: events.yaml#/{} missing", event));
        let data = fx.get("data").cloned().unwrap_or(Value::Null);
        let literal = bt_struct_expr(model, "events.yaml", &format!("evs::{}", event), def, &data, &format!("fixtures/{}", name));
        out.push_str(&format!(
            "/// tests.yaml#/fixtures/{} — events.yaml#/{}\nfn fx_{}() -> DomainEvent {{\n    DomainEvent::{}({})\n}}\n\n",
            name, event, bt_fn_name(name), event, literal
        ));
    }

    // ── the spec read-model baseline (fixture pool → canned rows the sagas/pricing read) ──────
    out.push_str(
        "/// Read-model baseline canned from the fixture pool: the catalog offers pricing reads and\n\
         /// the canonical OrderTracking rows the saga legs read (`read_order`) — state the spec's\n\
         /// GIVEN (an event list) cannot express but its cases assume.\n\
         async fn spec_baseline(bed: &TestBed) {\n",
    );
    for (k, fx) in &fixtures {
        let name = k.as_str().expect("fixture key");
        let event = fx.get("type").and_then(|t| t.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name).unwrap_or_default();
        if event == "OrderPlaced" {
            out.push_str(&format!(
                "    if let DomainEvent::OrderPlaced(op) = fx_{}() {{\n        bed.orders.upsert(support::tracking_row_from_order_placed(&op));\n    }}\n",
                bt_fn_name(name)
            ));
        }
        if event == "ProductAdded" || event == "CatalogImported" {
            out.push_str(&format!(
                "    support::install_catalog_offers(bed, &fx_{}());\n",
                bt_fn_name(name)
            ));
        }
    }
    out.push_str("}\n\n");

    // ── one test per case ─────────────────────────────────────────────────────────────────────
    for (k, t) in &tests {
        let key = k.as_str().expect("test key");
        let title = t.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let rules: Vec<String> = t
            .get("rules")
            .and_then(|r| r.as_sequence())
            .map(|s| s.iter().filter_map(|v| v.get("$ref").and_then(|x| x.as_str()).and_then(ref_name)).collect())
            .unwrap_or_default();
        let actor_ref = t
            .get("actor")
            .and_then(|a| a.get("$ref"))
            .and_then(|x| x.as_str())
            .unwrap_or_else(|| panic!("behaviour-tests: {}: missing actor", key));
        let actor = ref_name(actor_ref).unwrap_or_else(|| panic!("behaviour-tests: {}: malformed actor ref", key));
        let is_pm = actor_ref.starts_with("processmanager.yaml#/");
        let mut ctx: BTreeMap<&'static str, String> = BTreeMap::new();

        out.push_str(&format!("/// tests.yaml#/tests/{} — \"{}\"\n", key, rust_string_lit(title)));
        if !rules.is_empty() {
            out.push_str(&format!("/// rules: {}\n", rules.join(", ")));
        }
        out.push_str("#[tokio::test]\n");
        out.push_str(&format!("async fn {}() {{\n", bt_fn_name(key)));
        out.push_str("    let bed = TestBed::new();\n    spec_baseline(&bed).await;\n");

        // GIVEN — group consecutive fixtures of the same stream into one seed call.
        let given: Vec<String> = t
            .get("given")
            .and_then(|g| g.as_sequence())
            .map(|s| {
                s.iter()
                    .filter_map(|v| v.get("$ref").and_then(|x| x.as_str()))
                    .map(|r| r.trim_start_matches("#/fixtures/").to_string())
                    .collect()
            })
            .unwrap_or_default();
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for fx_name in &given {
            let fx = fixtures
                .get(Value::String(fx_name.clone()))
                .unwrap_or_else(|| panic!("behaviour-tests: {}: unknown fixture {}", key, fx_name));
            let event = fx.get("type").and_then(|t| t.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name).unwrap();
            let (agg, id) = bt_event_stream(&owners, &pool, &mut ctx, &event, fx.get("data"), &format!("{}/given", key));
            let (_, _, uuid_keyed) = bt_agg(agg).expect("aggregate meta");
            let stream = bt_stream_expr(agg, uuid_keyed, &id);
            let call = format!("fx_{}()", bt_fn_name(fx_name));
            match groups.last_mut() {
                Some((s, evs_)) if *s == stream => evs_.push(call),
                _ => groups.push((stream, vec![call])),
            }
        }
        for (stream, evs_) in &groups {
            out.push_str(&format!("    bed.seed(&{}, vec![{}]).await;\n", stream, evs_.join(", ")));
        }
        out.push_str("    let before = bed.snapshot();\n");

        // WHEN
        let when = t.get("when").unwrap_or_else(|| panic!("behaviour-tests: {}: missing when", key));
        let wref = when
            .get("type")
            .and_then(|ty| ty.get("$ref"))
            .and_then(|x| x.as_str())
            .unwrap_or_else(|| panic!("behaviour-tests: {}: malformed when", key));
        let msg = ref_name(wref).unwrap();
        let wdata = when.get("data").cloned().unwrap_or(Value::Null);
        if wref.starts_with("commands.yaml#/") {
            let def = resolve_ref(model, wref, "tests.yaml").unwrap();
            let literal = bt_struct_expr(model, "commands.yaml", &format!("cmds::{}", msg), def, &wdata, &format!("{}/when", key));
            out.push_str(&format!("    let cmd = {};\n", literal));
            let mut call = bt_command_call(&msg);
            // TipOrder derives `tippedBy` from the acting persona (ADR-0041): dispatch as the
            // RESTAURANT ordinal when the asserted fact says the restaurant tipped.
            if msg == "TipOrder" {
                let restaurant_tips = t
                    .get("then")
                    .and_then(|x| x.as_sequence())
                    .map(|seq| {
                        seq.iter()
                            .filter_map(|v| v.get("$ref").and_then(|x| x.as_str()))
                            .filter_map(|r| fixtures.get(Value::String(r.trim_start_matches("#/fixtures/").to_string())))
                            .any(|fx| {
                                fx.get("data").and_then(|d| d.get("tippedBy")).and_then(|v| v.as_str())
                                    == Some("RESTAURANT")
                            })
                    })
                    .unwrap_or(false);
                if restaurant_tips {
                    call = call.replace("support::actor()", "support::actor_as(3)");
                }
            }
            out.push_str(&format!("    let result = {};\n", call));
        } else {
            let def = resolve_ref(model, wref, "tests.yaml").unwrap();
            let literal = bt_struct_expr(model, "events.yaml", &format!("evs::{}", msg), def, &wdata, &format!("{}/when", key));
            out.push_str(&format!("    let ev = {};\n", literal));
            if is_pm {
                out.push_str(&format!("    let result = {};\n", bt_pm_event_call(&actor, &msg)));
            } else {
                // Aggregate ← delivered/inbound fact: record it on its stream through the write
                // path (Stripe payment facts go through the real inbound recording function).
                if matches!(msg.as_str(), "PaymentCaptured" | "PaymentFailed" | "PaymentRefunded") {
                    out.push_str(&format!(
                        "    let result = crate::payments::record_inbound_payment_event(&bed.store, DomainEvent::{}(ev), &support::actor()).await;\n",
                        msg
                    ));
                } else {
                    let (agg, id) = bt_event_stream(&owners, &pool, &mut ctx, &msg, Some(&wdata), &format!("{}/when", key));
                    let (_, _, uuid_keyed) = bt_agg(agg).expect("aggregate meta");
                    out.push_str(&format!(
                        "    let result = bed.record_fact(&{}, DomainEvent::{}(ev)).await;\n",
                        bt_stream_expr(agg, uuid_keyed, &id),
                        msg
                    ));
                }
            }
        }

        // THEN / THROWN
        if let Some(thrown) = t.get("thrown").and_then(|x| x.as_sequence()) {
            let codes: Vec<String> = thrown
                .iter()
                .filter_map(|e| e.get("$ref").and_then(|x| x.as_str()).and_then(ref_name))
                .map(|c| format!("\"{}\"", c))
                .collect();
            out.push_str(&format!(
                "    let err = result.expect_err(\"{}: the spec expects a typed rejection\");\n",
                key
            ));
            out.push_str(&format!("    support::assert_thrown(\"{}\", &err, &[{}]);\n", key, codes.join(", ")));
            out.push_str(&format!("    bed.assert_appended(\"{}\", &before, &[]);\n", key));
        } else {
            out.push_str(&format!("    let _ = result.expect(\"{}: the spec expects acceptance\");\n", key));
            let then: Vec<String> = t
                .get("then")
                .and_then(|x| x.as_sequence())
                .map(|s| {
                    s.iter()
                        .filter_map(|v| v.get("$ref").and_then(|x| x.as_str()))
                        .map(|r| r.trim_start_matches("#/fixtures/").to_string())
                        .collect()
                })
                .unwrap_or_default();
            let mut expected = Vec::new();
            for fx_name in &then {
                let fx = fixtures
                    .get(Value::String(fx_name.clone()))
                    .unwrap_or_else(|| panic!("behaviour-tests: {}: unknown fixture {}", key, fx_name));
                let event = fx.get("type").and_then(|t| t.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name).unwrap();
                let (agg, id) = bt_event_stream(&owners, &pool, &mut ctx, &event, fx.get("data"), &format!("{}/then", key));
                let (_, _, uuid_keyed) = bt_agg(agg).expect("aggregate meta");
                expected.push(format!("({}, fx_{}())", bt_stream_expr(agg, uuid_keyed, &id), bt_fn_name(fx_name)));
            }
            if expected.is_empty() {
                out.push_str(&format!("    bed.assert_appended(\"{}\", &before, &[]);\n", key));
            } else {
                out.push_str(&format!(
                    "    bed.assert_appended(\"{}\", &before, &[\n        {},\n    ]);\n",
                    key,
                    expected.join(",\n        ")
                ));
            }
        }
        out.push_str("}\n\n");
    }
    out
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
    eprintln!(
        "    - {} $refs resolve AND point at the kind their site declares (REF_CONTRACT, §1b)",
        coverage.refs
    );
    eprintln!("    - actor wiring: messages→commands/events, emits→events, throws→errors");
    eprintln!("    - lifecycles: {} aggregate state machines, {} transitions (lc-*: states∈enum, events emitted, deterministic, terminal closed, reachable)", coverage.lifecycles, coverage.lifecycle_transitions);
    eprintln!("    - api↔model: {} command links→commands, {} reads→views, roles→UserType", coverage.mutation_links, coverage.reads_links);
    eprintln!("    - views: aggregate→actors, fedBy→events, column types→scalars, indexes→columns, fk→views");
    eprintln!("    - stories: {} step→op links resolve, persona role authorized, every mutation/query reached by a story step", coverage.story_links);
    eprintln!("    - tests: {} Given/When/Then cases — data fields, actor handles `when`, `then`⊆emits, `thrown`⊆throws; every message/event/error exercised", coverage.test_cases);
    eprintln!("    - rules: {} business rules — every test asserts ≥1 rule, every rule asserted by ≥1 test (ADR-0032)", coverage.rules);
    eprintln!("    - ui: {} SDUI screens — resolver/action bindings $ref real api ops (API-meets-UI), data_requirements resolve; {} translations (en+fr, params match)", coverage.screens, coverage.translations);
    eprintln!("    - observability: {} workflow contracts — $ref/surface bindings resolve, mandatory ids (correlation_id/trace_id), span kinds, success.required_spans ⊆ declared spans", coverage.obs_contracts);
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
        ("lifecycles.rs", emit_domain_lifecycles(&model)),
        ("mod.rs", "// GENERATED module index — do not edit by hand.\npub mod scalars;\npub mod entities;\npub mod events;\npub mod commands;\npub mod errors;\npub mod lifecycles;\n".to_string()),
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
        ("pm_state.rs", emit_pm_state_application(&model)),
        ("services.rs", emit_services_application(&model)),
        ("process_managers.rs", emit_pm_orchestrators(&model)),
        ("handlers.rs", emit_application_handlers(&model)),
        ("behaviour_tests.rs", emit_behaviour_tests(&model)),
        ("mod.rs", "// GENERATED module index — do not edit by hand.\npub mod rows;\npub mod projectors;\npub mod pm_state;\npub mod process_managers;\npub mod services;\npub mod handlers;\n#[cfg(test)]\npub mod behaviour_tests;\n".to_string()),
    ] {
        let path = app_gen.join(name);
        if let Err(e) = fs::write(&path, content) {
            eprintln!("✗ write {}: {}", path.display(), e);
            std::process::exit(1);
        }
        eprintln!("✓ wrote {}", path.display());
    }
    // crates/infrastructure/src/generated/: the Postgres PM state stores from process_managers.yaml
    // (issue #27) — the adapter side of the application pm_state ports.
    let infra_gen = repo_root(&specs).join("crates/infrastructure/src/generated");
    if let Err(e) = fs::create_dir_all(&infra_gen) {
        eprintln!("✗ create {}: {}", infra_gen.display(), e);
        std::process::exit(1);
    }
    for (name, content) in [
        ("pm_state.rs", emit_pm_state_infrastructure(&model)),
        ("service_clients.rs", emit_services_http_clients(&model)),
        ("service_bindings.rs", emit_service_bindings(&model)),
        ("mod.rs", "// GENERATED module index — do not edit by hand.\npub mod pm_state;\npub mod service_clients;\npub mod service_bindings;\n".to_string()),
    ] {
        let path = infra_gen.join(name);
        if let Err(e) = fs::write(&path, content) {
            eprintln!("✗ write {}: {}", path.display(), e);
            std::process::exit(1);
        }
        eprintln!("✓ wrote {}", path.display());
    }
    // crates/server/src/generated/: the expose-gated /services/* routes from services.yaml (issue #26).
    let srv_svc_gen = repo_root(&specs).join("crates/server/src/generated");
    if let Err(e) = fs::create_dir_all(&srv_svc_gen) {
        eprintln!("✗ create {}: {}", srv_svc_gen.display(), e);
        std::process::exit(1);
    }
    for (name, content) in [
        ("services_routes.rs", emit_services_routes(&model)),
        ("mod.rs", "// GENERATED module index — do not edit by hand.\npub mod services_routes;\n".to_string()),
    ] {
        let path = srv_svc_gen.join(name);
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
    // crates/web/src/generated/: the SDUI component registry (allowlist) from restaurant_frontoffice.yaml
    // (#/component_registry) — the Leptos renderer's GENERATED dispatch surface (codegen roadmap
    // item 6, ADR-0033). Keeps the screens DSL the source of truth for what components may render.
    let web_gen = repo_root(&specs).join("crates/web/src/generated");
    if let Err(e) = fs::create_dir_all(&web_gen) {
        eprintln!("✗ create {}: {}", web_gen.display(), e);
        std::process::exit(1);
    }
    for (name, content) in [
        ("registry.rs", emit_web_registry(&model)),
        ("mod.rs", "// GENERATED module index — do not edit by hand.\npub mod registry;\n".to_string()),
    ] {
        let path = web_gen.join(name);
        if let Err(e) = fs::write(&path, content) {
            eprintln!("✗ write {}: {}", path.display(), e);
            std::process::exit(1);
        }
        eprintln!("✓ wrote {}", path.display());
    }
}

// ─── crates/web/src/generated/registry.rs (codegen roadmap item 6 — SDUI component allowlist) ──

/// Emit `crates/web/src/generated/registry.rs` — the GENERATED Leptos component registry: the
/// allowlist of SDUI component `type` keys from `restaurant_frontoffice.yaml#/component_registry`, as a
/// `ComponentKind` enum with `as_str` / `from_type` / `group` and the spec-ordered `ALL` slice. The
/// renderer dispatches on this enum, so a screen can never name a component outside the spec
/// allowlist (ADR-0033). The DSL stays the source of truth; this file is derived.
fn emit_web_registry(model: &Model) -> String {
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/screens/restaurant_frontoffice.yaml\n// (#/component_registry) — do not edit by hand. The SDUI component allowlist (ADR-0033, codegen\n// roadmap item 6): the renderer dispatches on `ComponentKind`, so a screen may only name a\n// component declared in the spec registry.\n\nuse serde::{Deserialize, Serialize};\n",
    );
    let reg = model
        .defs
        .get("screens/restaurant_frontoffice.yaml")
        .and_then(|v| v.get("component_registry"))
        .and_then(|v| v.as_mapping());

    // Flatten the groups in spec order; de-dup defensively — a key declared in two groups would
    // collide as a Rust variant, so we assert uniqueness (a spec smell fixed at the root, not masked).
    let mut groups: Vec<String> = Vec::new();
    let mut items: Vec<(String, String)> = Vec::new(); // (type_key, group)
    let mut seen = std::collections::BTreeSet::new();
    if let Some(reg) = reg {
        for (gk, gv) in reg {
            let group = match gk.as_str() {
                Some(s) => s.to_string(),
                None => continue,
            };
            groups.push(group.clone());
            if let Some(seq) = gv.as_sequence() {
                for t in seq {
                    if let Some(tk) = t.as_str() {
                        assert!(
                            seen.insert(tk.to_string()),
                            "component_registry: component type '{}' is declared in more than one group — it must be unique",
                            tk
                        );
                        items.push((tk.to_string(), group.clone()));
                    }
                }
            }
        }
    }

    out.push_str("\n/// The UI-intent groups the registry is organized into (spec `component_registry` keys).\n");
    out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]\n");
    out.push_str("pub enum ComponentGroup {\n");
    for g in &groups {
        out.push_str(&format!("    {},\n", pascal_snake(g)));
    }
    out.push_str("}\n");

    out.push_str("\n/// The allowlisted SDUI component kinds — one variant per `component_registry` entry.\n");
    out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]\n");
    out.push_str("pub enum ComponentKind {\n");
    for (t, _g) in &items {
        out.push_str(&format!("    {},\n", pascal_snake(t)));
    }
    out.push_str("}\n");

    out.push_str("\nimpl ComponentKind {\n");
    out.push_str("    /// Every registered component kind, in spec (registry) order.\n");
    out.push_str("    pub const ALL: &'static [ComponentKind] = &[\n");
    for (t, _g) in &items {
        out.push_str(&format!("        ComponentKind::{},\n", pascal_snake(t)));
    }
    out.push_str("    ];\n\n");
    out.push_str("    /// The spec `type` key (snake_case) this kind renders — 1:1 with the DSL.\n");
    out.push_str("    pub fn as_str(&self) -> &'static str {\n        match self {\n");
    for (t, _g) in &items {
        out.push_str(&format!("            ComponentKind::{} => \"{}\",\n", pascal_snake(t), t));
    }
    out.push_str("        }\n    }\n\n");
    out.push_str("    /// The registry group this kind belongs to.\n");
    out.push_str("    pub fn group(&self) -> ComponentGroup {\n        match self {\n");
    for (t, g) in &items {
        out.push_str(&format!(
            "            ComponentKind::{} => ComponentGroup::{},\n",
            pascal_snake(t),
            pascal_snake(g)
        ));
    }
    out.push_str("        }\n    }\n\n");
    out.push_str("    /// Resolve a spec `type` key to its kind — `None` for anything outside the allowlist.\n");
    out.push_str("    pub fn from_type(type_key: &str) -> Option<ComponentKind> {\n        match type_key {\n");
    for (t, _g) in &items {
        out.push_str(&format!("            \"{}\" => Some(ComponentKind::{}),\n", t, pascal_snake(t)));
    }
    out.push_str("            _ => None,\n        }\n    }\n}\n");

    out
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

    // ─── §1b ref-kind contract ──────────────────────────────────────────────────────────────────

    #[test]
    fn glob_star_stops_at_a_dot_but_doublestar_does_not() {
        assert!(glob("*.receives[*].message", "Cart.receives[3].message"));
        assert!(!glob("*.message", "Cart.receives[3].message"));
        assert!(glob("*.properties.**", "AddCartLine.properties.line.items"));
        assert!(glob("screens/*.yaml", "screens/captain_frontoffice.yaml"));
        assert!(glob("**.subscription", "screens[3].subscription"));
        assert!(!glob("resolvers.**", "actions.checkout.mutation"));
    }

    #[test]
    fn normalize_site_wildcards_names_and_indices_but_keeps_field_names() {
        assert_eq!(normalize_site("Cart.receives[12].message"), "*.receives[*].message");
        assert_eq!(
            normalize_site("PlaceOrderProcess.receives[0].steps[3].read.where.cart_id.from"),
            "*.receives[*].steps[*].read.where.*.from"
        );
        assert_eq!(normalize_site("types.Cart.properties.status"), "types.*.properties.status");
    }

    /// The model behind the kind checks below: two tables of DIFFERENT kinds that a naive
    /// "starts_with database/tables/" test cannot tell apart, plus a command and a payload object.
    fn kind_fixture() -> Model {
        let mut defs = BTreeMap::new();
        let y = |s: &str| serde_yaml::from_str::<Value>(s).expect("valid yaml");
        defs.insert(
            "database/tables/process_managers.yaml".into(),
            y("payment_process_manager:\n  columns:\n    cart_id: { type: text }\n"),
        );
        defs.insert("database/tables/referential.yaml".into(), y("ref_currency:\n  columns:\n    code: { type: text }\n"));
        defs.insert("commands.yaml".into(), y("PlaceOrder:\n  type: object\nCartLine:\n  type: object\n"));
        defs.insert("scalars.yaml".into(), y("OrderId:\n  type: string\nOrderStatus:\n  enum: [NEW, PAID]\n"));
        Model { defs }
    }

    #[test]
    fn classify_separates_kinds_that_share_a_file_or_a_directory() {
        let m = kind_fixture();
        let handled: BTreeSet<String> = ["PlaceOrder".to_string()].into_iter().collect();
        let k = |r: &str| {
            let p = parse_ref(r).expect("parses");
            classify(&p.file, &p.path, resolve_ref(&m, r, "x.yaml").expect("resolves"), &handled)
        };
        // Same directory, different kinds.
        assert_eq!(k("database/tables/process_managers.yaml#/payment_process_manager"), Some(Kind::PmStateTable));
        assert_eq!(k("database/tables/referential.yaml#/ref_currency"), Some(Kind::ReferentialTable));
        assert_eq!(k("database/tables/process_managers.yaml#/payment_process_manager/columns/cart_id"), Some(Kind::TableColumn));
        // Same file, different kinds: a handled command vs a shared payload sub-object.
        assert_eq!(k("commands.yaml#/PlaceOrder"), Some(Kind::Command));
        assert_eq!(k("commands.yaml#/CartLine"), Some(Kind::PayloadObject));
        // A scalar with an `enum` is an enum scalar (what a lifecycle `status` requires).
        assert_eq!(k("scalars.yaml#/OrderId"), Some(Kind::Scalar));
        assert_eq!(k("scalars.yaml#/OrderStatus"), Some(Kind::EnumScalar));
    }

    #[test]
    fn ref_kind_rejects_a_state_table_that_is_not_a_state_table() {
        let mut m = kind_fixture();
        m.defs.insert(
            "processmanager.yaml".into(),
            serde_yaml::from_str(
                "RefundProcess:\n  state_table: { $ref: 'database/tables/referential.yaml#/ref_currency' }\n",
            )
            .expect("valid yaml"),
        );
        let mut issues = Vec::new();
        validate_ref_kinds(&m, &mut issues);
        let hit = issues.iter().find(|i| i.rule == "ref-kind").expect("kind violation reported");
        assert!(hit.message.contains("referential table"), "{}", hit.message);
        assert!(hit.message.contains("process-manager state table"), "{}", hit.message);
    }

    #[test]
    fn ref_kind_accepts_the_right_state_table() {
        let mut m = kind_fixture();
        m.defs.insert(
            "processmanager.yaml".into(),
            serde_yaml::from_str(
                "RefundProcess:\n  state_table: { $ref: 'database/tables/process_managers.yaml#/payment_process_manager' }\n",
            )
            .expect("valid yaml"),
        );
        let mut issues = Vec::new();
        validate_ref_kinds(&m, &mut issues);
        assert!(issues.is_empty(), "expected no issues, got {:?}", issues.iter().map(|i| &i.message).collect::<Vec<_>>());
    }

    #[test]
    fn ref_site_undeclared_is_fail_closed() {
        let mut m = kind_fixture();
        // A brand-new ref-carrying field nobody declared a contract for.
        m.defs.insert(
            "processmanager.yaml".into(),
            serde_yaml::from_str("RefundProcess:\n  brand_new_field: { $ref: 'commands.yaml#/PlaceOrder' }\n")
                .expect("valid yaml"),
        );
        let mut issues = Vec::new();
        validate_ref_kinds(&m, &mut issues);
        let hit = issues.iter().find(|i| i.rule == "ref-site-undeclared").expect("undeclared site reported");
        assert!(hit.message.contains("'RefundProcess.brand_new_field'"), "{}", hit.message);
    }

    #[test]
    fn source_file_membership() {
        assert!(is_source_file("api.yaml"));
        assert!(is_source_file("architecture/c4-l2.yaml"));
        assert!(is_source_file("services.yaml"));
        assert!(is_source_file("screens/captain_frontoffice.yaml"));
        assert!(is_source_file("restaurant_frontoffice.translations.yaml"));
        assert!(!is_source_file("nope.yaml"));
    }

    #[test]
    fn snake_type_is_module_case() {
        assert_eq!(snake_type("Order"), "order");
        assert_eq!(snake_type("DeliveryJob"), "delivery_job");
        assert_eq!(snake_type("RestaurantAccount"), "restaurant_account");
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
    fn pm_base_name_strips_suffix_and_keeps_process_for_single_words() {
        assert_eq!(pm_base_name("payment_process_manager"), "PaymentProcess");
        assert_eq!(pm_base_name("refund_process_manager"), "RefundProcess");
        assert_eq!(pm_base_name("cart_binding_process_manager"), "CartBinding");
        assert_eq!(pm_base_name("delivery_dispatch_process_manager"), "DeliveryDispatch");
    }

    #[test]
    fn pm_lookup_method_is_by_column_minus_id() {
        assert_eq!(pm_lookup_method("cart_id"), "by_cart");
        assert_eq!(pm_lookup_method("payment_intent_id"), "by_payment_intent");
        assert_eq!(pm_lookup_method("delivery_job_id"), "by_delivery_job");
        assert_eq!(pm_lookup_method("session_id"), "by_session");
    }

    /// A Model from inline YAML sources (path → content), for emitter tests that need spec shapes
    /// the committed catalog does not exercise (http binding, expose: true).
    fn inline_model(files: &[(&str, &str)]) -> Model {
        let mut defs = BTreeMap::new();
        for (path, content) in files {
            let parsed: Value = serde_yaml::from_str(content).expect("test yaml parses");
            defs.insert(path.to_string(), strip_meta(parsed));
        }
        Model { defs }
    }

    const SVC_HTTP_EXPOSED: &str = r#"
geocoding:
  description: "Test service."
  operations:
    resolve_address:
      description: "Resolve one address."
      input:
        query: { type: string }
      output:
        latitude: { type: number }
      errors: []
    warm_cache:
      description: "No input, no output."
      errors: []
  binding: http
  expose: true
  implementations:
    nominatim:
      routes:
        resolve_address: 'POST /adapters/nominatim/search'
        warm_cache: 'POST /adapters/nominatim/warm'
"#;

    #[test]
    fn services_trait_signatures_follow_the_catalog() {
        let model = inline_model(&[("services.yaml", SVC_HTTP_EXPOSED)]);
        let out = emit_services_application(&model);
        assert!(out.contains("pub trait GeocodingService: Send + Sync {"), "{out}");
        assert!(
            out.contains("async fn resolve_address(&self, input: GeocodingResolveAddressInput, meta: &ServiceCallMeta) -> Result<GeocodingResolveAddressOutput, DomainError>;"),
            "{out}"
        );
        // Input-less + output-less operation: no input parameter, unit result.
        assert!(
            out.contains("async fn warm_cache(&self, meta: &ServiceCallMeta) -> Result<(), DomainError>;"),
            "{out}"
        );
        assert!(out.contains("pub struct GeocodingResolveAddressInput {\n    pub query: String,\n}"), "{out}");
    }

    #[test]
    fn services_http_client_derives_paths_and_kebab_case() {
        let model = inline_model(&[("services.yaml", SVC_HTTP_EXPOSED)]);
        let out = emit_services_http_clients(&model);
        assert!(out.contains("pub struct HttpGeocodingService"), "{out}");
        assert!(out.contains("\"/services/geocoding/resolve-address\""), "{out}");
        assert!(out.contains("post_call(&self.http, &self.base_url, \"/services/geocoding/warm-cache\", (), meta).await"), "{out}");
    }

    #[test]
    fn service_bindings_honor_the_spec_topology() {
        let http = inline_model(&[("services.yaml", SVC_HTTP_EXPOSED)]);
        let out = emit_service_bindings(&http);
        assert!(out.contains("SERVICE_GEOCODING_URL"), "{out}");
        assert!(out.contains("HttpGeocodingService::new(url)"), "{out}");
        let local = inline_model(&[(
            "services.yaml",
            "payment:\n  operations:\n    request:\n      errors: []\n  binding: local\n  expose: false\n",
        )]);
        let out = emit_service_bindings(&local);
        assert!(out.contains("pub fn payment_service("), "{out}");
        assert!(out.contains("Ok(local())"), "{out}");
        assert!(!out.contains("SERVICE_PAYMENT_URL"), "{out}");
    }

    #[test]
    fn services_routes_are_expose_gated() {
        let none = inline_model(&[(
            "services.yaml",
            "payment:\n  operations:\n    request:\n      errors: []\n  binding: local\n  expose: false\n",
        )]);
        let out = emit_services_routes(&none);
        assert!(out.contains("pub fn services_router<S: Clone + Send + Sync + 'static>() -> axum::Router<S> {"), "{out}");
        assert!(!out.contains("ServicesRouterState"), "{out}");
        let exposed = inline_model(&[("services.yaml", SVC_HTTP_EXPOSED)]);
        let out = emit_services_routes(&exposed);
        assert!(out.contains("pub struct ServicesRouterState {\n    pub geocoding: Arc<dyn GeocodingService>,\n}"), "{out}");
        assert!(out.contains(".route(\"/services/geocoding/resolve-address\", post(geocoding_resolve_address))"), "{out}");
        assert!(out.contains("Json(call): Json<WireCall<GeocodingResolveAddressInput>>"), "{out}");
    }

    #[test]
    fn svc_names_derive_mechanically() {
        assert_eq!(pascal_snake("payment"), "Payment");
        assert_eq!(pascal_snake("offer_job"), "OfferJob");
        assert_eq!(pascal_snake("verify_phone_otp"), "VerifyPhoneOtp");
        assert_eq!(svc_http_path("payment", "request"), "/services/payment/request");
        assert_eq!(svc_http_path("delivery", "offer_job"), "/services/delivery/offer-job");
        assert_eq!(svc_url_var("payment"), "SERVICE_PAYMENT_URL");
        assert_eq!(svc_url_var("catalog_sync"), "SERVICE_CATALOG_SYNC_URL");
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
