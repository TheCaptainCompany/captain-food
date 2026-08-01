//! Captain.Food codegen (ADR-0034) — the single spec gate.
//!
//! It loads every `specs/**` DSL file and runs the full validator (§1–§11: referential integrity, actor
//! wiring, api↔model, views, stories, tests, rules, translations, screens, observability, C4) and every
//! generator (translations, views SQL + the `database.md` §2 injection, C4 Structurizr/Mermaid, GraphQL
//! SDL, and the Markdown + HTML docs). It began as a TypeScript tool (`tools/codegen`) and was ported here
//! at parity — all 8 generated artifacts byte-identical and the same (rule, location) validation issue set
//! (verified by a differential harness) — after which the TypeScript codegen was retired. CI (`codegen`
//! job) builds + tests, validates, regenerates and fails on any drift.

pub(crate) use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
pub(crate) use std::fs;
pub(crate) use std::path::PathBuf;

pub(crate) use serde_yaml::Value;

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
    // Translation keys consumed by hand-written Rust, not referenced from any screen (#110): the
    // `translation-key-unused` gate treats these as USED, and a companion codegen test greps the
    // crates so a stale entry (matching no code) is itself caught.
    "translations.code_refs.yaml",
    "observability.yaml",
    // Runtime configuration (PROP-20260729-004500, issue #246): every env-fulfilled setting the app
    // needs, with its type, per-profile required-ness and — printed in the fail-fast report — what it
    // gates. Emits the typed reader; a drift test pins every env::var call site to a declared key.
    "configuration.yaml",
    "architecture/c4-l2.yaml",
    "architecture/c4-l3.yaml",
];

/// The supported UI locales (#110). The single source of truth for translation-hygiene coverage:
/// every catalog key must carry a message in each (`translation-locale-missing`). Adding a locale =
/// add it here, then the gate forces every key to gain that message before anything ships.
const SUPPORTED_LOCALES: &[&str] = &["en", "fr"];

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
    /// A per-actor typed self-message — `actors.yaml#/<Actor>/reminders/<Name>` (ADR-20260731-214500).
    Reminder,
    /// A declared aggregate state field — `actors.yaml#/<Actor>/state/<field>` (PROP-20260728-135632 §2.1).
    StateField,
    /// A `configuration.yaml#/keys/<KEY>` runtime-configuration key (PROP-20260729-004500).
    ConfigKey,
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
            Kind::Reminder => "actor reminder",
            Kind::StateField => "actor state field",
            Kind::ConfigKey => "configuration key",
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
        // `principals` is the file-header role → identity-scalar map (PROP-20260728-152752 §2.4),
        // not an actor — excluded so it never registers as a phantom aggregate. Below the actor:
        // `<Actor>/reminders/<Name>` is a typed self-message (ADR-20260731-214500) and
        // `<Actor>/state/<field>` a declared state field (what a deletion `match.state` binds to).
        "actors.yaml" => match (top, seg(1), path.len()) {
            (true, _, _) => (path.first().map(String::as_str) != Some("principals")).then_some(Kind::Aggregate),
            (false, Some("reminders"), 3) => Some(Kind::Reminder),
            (false, Some("state"), 3) => Some(Kind::StateField),
            _ => None,
        },
        // Runtime configuration (PROP-20260729-004500): `keys/<KEY>` is what a `deletion.after` /
        // `reminders.*.after` window binds to (ADR-20260731-214500 — a $ref, never a bare string).
        "configuration.yaml" => (path.len() == 2 && seg(0) == Some("keys")).then_some(Kind::ConfigKey),
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

    // Configuration keys are TYPED (PROP-20260729-004500): each binds the scalar whose `pattern` the
    // generated reader enforces at startup, so "present" is checked against "usable".
    ("configuration.yaml", "keys.*.scalar", &[Kind::Scalar, Kind::EnumScalar]),

    // Actors (aggregates): the inbox and the lifecycle state machine. A `message` may also be the
    // actor's own reminder (`#/<Actor>/reminders/<Name>` — ADR-20260731-214500; §2f proves same-actor).
    ("actors.yaml", "*.receives[*].message",            &[Kind::Command, Kind::Event, Kind::Reminder]),
    ("actors.yaml", "*.receives[*].emits[*]",           &[Kind::Event]),
    ("actors.yaml", "*.receives[*].throws[*]",          &[Kind::Error]),
    // Reminders (typed self-messages, ADR-20260731-120825/-150500/-153000/-214500): the payload is an
    // events.yaml FACT (record semantics — never a command), the optional window a configuration key.
    ("actors.yaml", "*.reminders.*.payload",            &[Kind::Event]),
    ("actors.yaml", "*.reminders.*.after",              &[Kind::ConfigKey]),
    // A receive declares the reminders it (re)schedules — the handler's third observable effect.
    ("actors.yaml", "*.receives[*].schedules[*]",       &[Kind::Reminder]),
    // Declarative deletion (ADR-20260731-214500): triggers/undo/receipt are events, the window a
    // configuration key, and a propagation `match` is STRONGLY TYPED (event property ↔ state field).
    ("actors.yaml", "*.deletion.triggers[*].on[*]",           &[Kind::Event]),
    ("actors.yaml", "*.deletion.triggers[*].after",           &[Kind::ConfigKey]),
    ("actors.yaml", "*.deletion.triggers[*].cancelled_on[*]", &[Kind::Event]),
    ("actors.yaml", "*.deletion.triggers[*].match.event",     &[Kind::MessageProperty]),
    ("actors.yaml", "*.deletion.triggers[*].match.state",     &[Kind::StateField]),
    ("actors.yaml", "*.deletion.receipt",                     &[Kind::Event]),
    ("actors.yaml", "*.lifecycle.status",               &[Kind::EnumScalar]),
    // The actor-mailbox addressing layer (ADR-20260730-231500, PROP-20260728-152752 §2/§2.4):
    // `principals` maps each authenticated role to its resolved domain-identity scalar; `identity`
    // is a TYPED same-actor state-field ref (ADR-20260731-214500 consequences — the field is
    // implicitly declared by the ref itself, see `is_implicit_identity_state_ref`; §2d proves it).
    ("actors.yaml", "principals.*.id",                  &[Kind::Scalar]),
    ("actors.yaml", "*.identity",                       &[Kind::StateField]),
    // Write-side per-instance authorization (#235): a non-`any` acting entry binds the role to a
    // DECLARED state field of the same actor (`any` stays a bare keyword, not a ref).
    ("actors.yaml", "*.receives[*].requires.acting.*",  &[Kind::StateField]),
    // Declared aggregate state (PROP-20260728-135632 §2.1): typed fields with event(-property)
    // lineage — `from`/`removedBy` carry properties (latest/set) or whole events (flag/count);
    // `of` is the set element type (single scalar, or a named map for composite elements).
    ("actors.yaml", "*.state.*.type",                   &[Kind::Scalar, Kind::EnumScalar]),
    ("actors.yaml", "*.state.*.from[*]",                &[Kind::MessageProperty, Kind::Event]),
    ("actors.yaml", "*.state.*.removedBy[*]",           &[Kind::MessageProperty, Kind::Event]),
    ("actors.yaml", "*.state.*.of",                     &[Kind::Scalar, Kind::EnumScalar]),
    ("actors.yaml", "*.state.*.of.*",                   &[Kind::Scalar, Kind::EnumScalar]),
    ("actors.yaml", "*.lifecycle.initial[*].event",     &[Kind::Event]),
    ("actors.yaml", "*.lifecycle.transitions[*].event", &[Kind::Event]),

    // Process managers: state-table orchestrators (ADR-20260719-…). The state table is a PM state
    // table — not any table; reads hit read models; deliver/send target aggregates.
    ("processmanager.yaml", "*.state_table",                            &[Kind::PmStateTable]),
    ("processmanager.yaml", "*.ports.*",                                &[Kind::Service]),
    ("processmanager.yaml", "*.receives[*].message",                    &[Kind::Command, Kind::Event]),
    // Wrapper-seam arms a linear step pipeline cannot express (REPLACEMENT/REFUND, #159/#207): a leg may
    // DECLARE the events it emits / errors it throws from its hand-written wrapper, merged with the
    // step-derived set, so the behaviour-test coverage checks (test-then-not-emitted / -thrown) see them.
    ("processmanager.yaml", "*.receives[*].emits[*]",                   &[Kind::Event]),
    ("processmanager.yaml", "*.receives[*].throws[*]",                  &[Kind::Error]),
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
    "actions", "activities", "actor", "after", "args", "by", "call", "cancelled_on", "columns",
    "command", "content", "context", "deletion", "deliver", "emits", "event", "expect", "fixtures",
    "from", "from_hook", "given", "guard", "inputs",
    "acting", "claims", "identity", "lifecycle", "mailbox", "match", "message", "messages", "model",
    "mutations", "of", "on", "operations", "params", "payload", "ports", "principals", "receipt",
    "reminders", "removedBy", "requires",
    "properties", "queries", "read", "reads", "receives", "resolvers", "returns", "rules",
    "schedules", "screens", "send", "set", "state", "state_table", "status", "steps",
    "subscriptions", "tests", "then", "throws", "thrown", "to", "transitions", "triggers", "type",
    "types", "when", "where", "with", "workflows",
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

/// A resolver's pinned static `args:` (#82) — the ONLY place a screens surface names an api.yaml
/// query ARGUMENT by hand. §1 proves the resolver's `query.$ref` RESOLVES and §1b proves it resolves
/// to a QUERY, but neither looks inside `args:`, so a typo in a pinned key (`listKey` where the query
/// declares `list`) stayed invisible until a client actually issued the query — the server then
/// rejects the whole operation on an unknown input field. Fail closed here instead:
///
/// - `resolver-unknown-arg` — the pinned key is not an argument of the bound query;
/// - `resolver-invalid-arg-value` — the argument IS declared and enum-typed, but the pinned literal
///   is not one of its members (mirrors the `test-invalid-enum-value` check of `check_shape`, done
///   inline here because `check_shape` resolves refs against `tests.yaml`, not `api.yaml`).
///
/// NOT checked: that every REQUIRED arg is pinned. A pin is a static DEFAULT — the remaining args are
/// supplied by the caller at runtime (`crates/web/src/graphql.rs#execute_resolver` merges caller
/// variables OVER the pins), so an unpinned required arg is normal, not an error.
fn validate_resolver_args(model: &Model, issues: &mut Vec<Issue>, at: &str, query: &str, args: &Value) {
    let Some(pinned) = args.as_mapping() else { return };
    let empty = serde_yaml::Mapping::new();
    let declared = model
        .defs
        .get("api.yaml")
        .and_then(|v| v.get("queries"))
        .and_then(|v| v.get(query))
        .and_then(|v| v.get("args"))
        .and_then(|v| v.as_mapping())
        .unwrap_or(&empty);
    let names: Vec<&str> = declared.keys().filter_map(|k| k.as_str()).collect();

    for (ak, av) in pinned {
        let Some(name) = ak.as_str() else { continue };
        let Some(node) = declared.get(Value::String(name.to_string())) else {
            issues.push(err(
                "resolver-unknown-arg",
                at.into(),
                format!(
                    "pinned arg '{}' is not an argument of api.yaml query '{}' ({}).",
                    name,
                    query,
                    if names.is_empty() {
                        "it declares none".to_string()
                    } else {
                        format!("declared: {}", names.join("|"))
                    }
                ),
            ));
            continue;
        };
        // Enum-typed arg: the pinned literal (or every item of an `array: true` pin) must be a member.
        let Some(rf) = node.get("$ref").and_then(|x| x.as_str()) else { continue };
        let Some(target) = resolve_ref(model, rf, "api.yaml") else { continue };
        let Some(vals) = target.get("enum").and_then(|e| e.as_sequence()) else { continue };
        let pins: Vec<&Value> = match av.as_sequence() {
            Some(seq) => seq.iter().collect(),
            None => vec![av],
        };
        for p in pins {
            let Some(lit) = p.as_str() else { continue };
            if !vals.iter().any(|v| v.as_str() == Some(lit)) {
                issues.push(err(
                    "resolver-invalid-arg-value",
                    at.into(),
                    format!(
                        "pinned arg '{}' = '{}' is not a value of enum {} ({}).",
                        name,
                        lit,
                        rf,
                        vals.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join("|")
                    ),
                ));
            }
        }
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
                } else if resolve_ref(model, &r, f).is_none()
                    && !is_implicit_identity_state_ref(model, &r, f)
                {
                    // The identity state field is implicitly declared by the actor's own typed
                    // `identity` ref (the stream key needs no fold entry) — see
                    // `is_implicit_identity_state_ref`; §2d proves the declaration's shape.
                    issues.push(err("ref-dangling", loc, format!("$ref '{}' does not resolve.", r)));
                }
            }
        }
    }

    // --- 1b. Ref-KIND contract: a resolving $ref must also point at the right KIND of thing -------
    validate_ref_kinds(model, &mut issues);

    // --- 1c. Configuration hygiene (PROP-20260729-004500) ----------------------------------------
    validate_configuration(model, &mut issues);

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
            } else if reminder_ref_parts(&entry.message_ref).is_some() {
                // A reminder self-message (ADR-20260731-214500): §2f proves it resolves on the SAME
                // actor. Its payload FACT counts as consumed — the delivery records it (record
                // semantics, ADR-20260731-153000) — so the event is not an orphan.
                if let Some(ev) = reminder_payload_event(model, &entry.message_ref) {
                    consumed_events.insert(ev);
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
    validate_mailbox_addressing(model, &mut issues);
    validate_actor_state(model, &mut issues);
    // --- 2f. Reminders + declarative deletion (ADR-20260731-214500) ------------------------------
    validate_reminders_and_deletion(model, &mut issues);
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
    // Declarative deletion (ADR-20260731-214500): the generic engine RECORDS each declared
    // `receipt` fact (on the deletion ledger) and CONSUMES each trigger/undo fact — a fact that
    // exists only in a `deletion:` block is engine vocabulary, not an orphan.
    for d in parse_deletions(model) {
        if let Some(e) = d.receipt_ref.as_deref().and_then(ref_name) {
            produced_events.insert(e);
        }
        for t in &d.triggers {
            for r in t.on.iter().chain(t.cancelled_on.iter()) {
                if let Some(e) = ref_name(r) {
                    produced_events.insert(e);
                }
            }
        }
    }
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
                // A reminder receive is keyed by its PAYLOAD event (record semantics,
                // ADR-20260731-153000): the delivery records that fact, so that is the vocabulary a
                // behaviour test's `when.type` names (tests.*.when.type must be a command or event).
                let msg = match reminder_ref_parts(&e.message_ref) {
                    Some((_, rname)) => reminder_payload_event(model, &e.message_ref).unwrap_or(rname),
                    None => match ref_name(&e.message_ref) {
                        Some(m) => m,
                        None => continue,
                    },
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

    // --- 10 (+10b). Translation hygiene (#110): uniqueness, full locale coverage, param match, and the
    // unused-key + code_refs gates. Extracted to a standalone fn (like `validate_ref_kinds`) so tests can
    // exercise it on minimal fixtures without running the whole validator.
    cov.translations += translation_entries(model).len();
    validate_translations(model, &mut issues);

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
                                // The binding is a real query — now prove its pinned static args
                                // name real arguments of THAT query (#82).
                                if let Some(args) = r.get("args") {
                                    validate_resolver_args(
                                        model,
                                        &mut issues,
                                        &format!("{}/resolvers/{}/args", sfkey, name),
                                        &op_name(rf),
                                        args,
                                    );
                                }
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
/// Translation hygiene (#110, PROP-20260724-133700 §1c) — the §10/§10b rules, standalone so tests can
/// run them on a minimal fixture. Emits: `translation-duplicate-key`, `translation-locale-missing`
/// (full SUPPORTED_LOCALES coverage), `translation-param-mismatch`, `translation-key-unused` (a key no
/// screen and no `code_refs` entry references), and `translation-code-ref-unknown` (a stale manifest
/// entry). Does not touch coverage — the caller counts entries.
fn validate_translations(model: &Model, issues: &mut Vec<Issue>) {
    // §10 — per-entry: uniqueness across files, locale coverage, param declaration/usage.
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
        let messages = t.get("messages");
        // Full locale coverage is a HARD error: every key carries every SUPPORTED_LOCALES message —
        // a new `en` string without its `fr` cannot ship.
        for loc in SUPPORTED_LOCALES {
            let ok = messages
                .and_then(|m| m.get(loc))
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false);
            if !ok {
                issues.push(err(
                    "translation-locale-missing",
                    at.clone(),
                    format!("translation '{}' has no '{}' message (every supported locale [{}] is required).", key, loc, SUPPORTED_LOCALES.join(", ")),
                ));
            }
        }
        let params: BTreeSet<String> = t.get("params").and_then(|p| p.as_mapping()).map(map_of_keys).unwrap_or_default();
        for loc in SUPPORTED_LOCALES {
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
        let mut used_params: BTreeSet<String> = BTreeSet::new();
        for loc in SUPPORTED_LOCALES {
            used_params.extend(placeholders(messages.and_then(|m| m.get(loc))));
        }
        for p in &params {
            if !used_params.contains(p) {
                issues.push(err(
                    "translation-param-mismatch",
                    at.clone(),
                    format!("declared param '{}' is used by no message.", p),
                ));
            }
        }
    }

    // §10b — every key is USED, or it must be DELETED. Used = referenced by a screen `$ref` OR matched
    // by a `code_refs` manifest entry (keys consumed by hand-written Rust, e.g. `order.status.*`).
    let defined: Vec<(String, String)> =
        translation_entries(model).into_iter().map(|(f, k, _)| (f, k)).collect();

    // Used-by-screen: every content `$ref` across every screens/*.yaml that targets a translation file
    // (resolvers/actions target api.yaml, so they fall out). The used key is the ref's pointer.
    let mut used: BTreeSet<String> = BTreeSet::new();
    for (fkey, doc) in model.defs.iter().filter(|(k, _)| k.starts_with("screens/")) {
        let mut refs: Vec<(String, String)> = Vec::new();
        collect_refs(doc, fkey, &mut refs);
        for (_loc, rf) in &refs {
            let target = ref_target_file(rf, fkey);
            if matches!(target.as_deref(), Some(f) if f == "translations.yaml" || f.ends_with(".translations.yaml")) {
                if let Some(k) = ref_name(rf) {
                    used.insert(k);
                }
            }
        }
    }

    // Used-by-code: the code_refs manifest. An entry is an exact key or a `prefix.*` wildcard; each entry
    // must match >=1 defined key (else it is stale — `translation-code-ref-unknown`).
    let code_refs = model
        .defs
        .get("translations.code_refs.yaml")
        .and_then(|v| v.get("code_refs"))
        .and_then(|v| v.as_sequence())
        .cloned()
        .unwrap_or_default();
    let defined_keys: BTreeSet<&str> = defined.iter().map(|(_, k)| k.as_str()).collect();
    for entry in &code_refs {
        let Some(pat) = entry.get("key").and_then(|v| v.as_str()) else { continue };
        let matched: Vec<&str> = if let Some(prefix) = pat.strip_suffix(".*") {
            let p = format!("{prefix}.");
            defined_keys.iter().copied().filter(|k| k.starts_with(&p)).collect()
        } else {
            defined_keys.iter().copied().filter(|k| *k == pat).collect()
        };
        if matched.is_empty() {
            issues.push(err(
                "translation-code-ref-unknown",
                format!("translations.code_refs.yaml/{}", pat),
                format!("code_refs entry '{}' matches no translation key — remove it or fix the key.", pat),
            ));
        }
        for k in matched {
            used.insert(k.to_string());
        }
    }

    for (file, key) in &defined {
        if !used.contains(key) {
            issues.push(err(
                "translation-key-unused",
                format!("{}/{}", file, key),
                format!("translation key '{}' is referenced by no screen and no code_refs entry — delete it, or if hand-written Rust consumes it, declare it in translations.code_refs.yaml.", key),
            ));
        }
    }
}

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
            // Enums are stored as their TEXT value verbatim (ADR-20260728: supersedes the ADR-0037
            // INTEGER-ordinal + ref_<enum> lookup scheme) — self-describing rows, no join to read.
            return "TEXT".into();
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

/// The values of a scalars.yaml enum, in declared order — `Some` only for an enum scalar.
fn enum_values(model: &Model, ty: &str) -> Option<Vec<String>> {
    model
        .defs
        .get("scalars.yaml")?
        .get(ty)?
        .get("enum")?
        .as_sequence()
        .map(|s| s.iter().filter_map(|v| v.as_str().map(|x| x.to_string())).collect())
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
        // Enum columns store the enum's TEXT value verbatim — exactly what the payload holds, so no
        // mapping is needed; the values are still validated against scalars.yaml for literals.
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
                        DeriveVal::Lit(s) => {
                            if let Some(vals) = &enum_vals {
                                assert!(
                                    vals.iter().any(|v| v == s),
                                    "derive value '{}' not in enum {}",
                                    s,
                                    c.ty
                                );
                            }
                            format!("'{}'", s)
                        }
                        DeriveVal::Payload(p) => format!("e.payload->>'{}'", p),
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
                // An enum column stores the payload's TEXT value verbatim; a Money property splits into
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
                        payload_extract(alias, prop, &pgty)
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
        "smallint" => "SMALLINT",   // mailbox partition ordinal (inbound_messages, #242)
        "boolean" => "BOOLEAN",
        "timestamptz" => "TIMESTAMPTZ",
        "jsonb" => "JSONB",
        "numeric" => "NUMERIC",
        "interval" => "INTERVAL",
        "bytea" => "BYTEA",   // encrypted-at-rest blobs (auth_sessions ciphertext, #112)
        other => panic!("database/tables.yaml: unknown column type '{}' — extend table_sql_type", other),
    }
}

/// Emit `schema.generated.sql` (ADR-0037, enum storage revised by ADR-20260728): the full store DDL —
/// the real tables from database/tables.yaml, the raw SQL functions from database/functions/*.sql
/// (sorted by filename), then the triggers declared on the tables (after the functions they execute).
/// Enum columns are TEXT holding the scalars.yaml value verbatim — no ref_<enum> lookup tables.
fn emit_schema_sql(model: &Model, specs: &std::path::Path) -> String {
    let mut sections: Vec<String> = Vec::new();

    // 1. Tables from database/tables/*.yaml, in file order. Triggers are collected and emitted after
    // the functions they execute (step 3). projection_tables.yaml is handled separately (step 1b) — its
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
                    sql_type(&scalar, model) // an enum scalar → TEXT value, verbatim
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

    // 1b. Materialized read-model tables (database/tables/projection_tables.yaml) — column types resolved
    // from event lineage. Filled by an application-layer (Rust) projector, not SQL (ADR-0040). Emitted
    // here so the read-model tables sit alongside the store tables.
    let ptables = emit_projection_tables_sql(model);
    if !ptables.trim().is_empty() {
        sections.push(ptables);
    }

    // 2. Functions — raw SQL bodies from database/functions/*.sql, sorted by filename. They reference
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

    // 3. Triggers — after the functions they execute.
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
    /// Reminders this handler (re)schedules — raw same-actor `$ref`s (ADR-20260731-214500 §2).
    schedules: Vec<String>,
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
                    let schedules = ref_strings(e.get("schedules"));
                    let effect = e.get("effect").and_then(|x| x.as_str()).map(|s| s.to_string());
                    receives.push(Receive { message_ref, emits, throws, schedules, effect });
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
                    // Wrapper-seam arms (#159/#207): a leg may DECLARE additional emits/throws its
                    // hand-written wrapper produces (events/errors a linear step pipeline cannot express),
                    // merged with the step-derived set so behaviour-test coverage sees the full inbox.
                    for ev in ref_strings(e.get("emits")) {
                        if !emits.contains(&ev) {
                            emits.push(ev);
                        }
                    }
                    for er in ref_strings(e.get("throws")) {
                        if !throws.contains(&er) {
                            throws.push(er);
                        }
                    }
                    let effect = e.get("description").and_then(|x| x.as_str()).map(|s| s.to_string());
                    receives.push(Receive { message_ref, emits, throws, schedules: Vec::new(), effect });
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

/// Whether a message definition ("commands.yaml#/X" / "events.yaml#/X" / "messages.yaml#/X")
/// declares `properties.<prop>`.
fn message_property_exists(model: &Model, message_ref: &str, prop: &str) -> bool {
    let Some((file, key)) = message_ref.split_once("#/") else { return false };
    model
        .defs
        .get(file)
        .and_then(|d| d.get(key))
        .and_then(|n| n.get("properties"))
        .and_then(|p| p.get(prop))
        .is_some()
}

/// The payload-property NAME an actor's TYPED `identity` declares —
/// `identity: { $ref: '#/<Actor>/state/<field>' }` (ADR-20260731-214500 consequences: typed $refs
/// everywhere; the bare-string form is a hard §2d error, `identity-untyped`). Returns `None` for a
/// missing identity, a bare string, or a ref of any other shape (wrong file/actor/path — §2d's
/// `identity-state-field-missing`). Generation (command addressing) reads through this helper so
/// validator and emitters can never disagree on the field.
fn actor_identity_field(def: &Value, actor: &str) -> Option<String> {
    let r = def.get("identity")?.get("$ref")?.as_str()?;
    let pr = parse_ref(r)?;
    if !pr.file.is_empty() && pr.file != "actors.yaml" {
        return None;
    }
    (pr.path.len() == 3 && pr.path[0] == actor && pr.path[1] == "state")
        .then(|| pr.path[2].clone())
}

/// True when `r` points at an actor's IMPLICIT identity state field: `#/<Actor>/state/<field>`
/// where `<field>` is exactly what that actor's typed `identity` declares. The identity is the
/// STREAM KEY — it exists before any fold — so it is declared by the `identity` ref itself, not by
/// an explicit `state:` entry (forcing one into every aggregate would add fold fields that change
/// the generated states.rs for no behaviour). §1 exempts these refs from `ref-dangling`; §2d's
/// `identity-state-field-missing` owns the shape proof.
fn is_implicit_identity_state_ref(model: &Model, r: &str, ctx: &str) -> bool {
    let Some(pr) = parse_ref(r) else { return false };
    let file = if pr.file.is_empty() { ctx } else { pr.file.as_str() };
    if file != "actors.yaml" || pr.path.len() != 3 || pr.path[1] != "state" {
        return false;
    }
    model
        .defs
        .get("actors.yaml")
        .and_then(|m| m.get(pr.path[0].as_str()))
        .and_then(|def| actor_identity_field(def, &pr.path[0]))
        .is_some_and(|f| f == pr.path[2])
}

/// §2d — the actor-mailbox ADDRESSING layer (ADR-20260730-231500, PROP-20260728-152752 §2):
/// the file-header `principals` map (role → domain-identity scalar), each aggregate's `identity`
/// (the payload property that addresses its instances = the stream id) and `mailbox.partitions`
/// (the fixed keyspace width). Enforced here:
///   - `pr-role-unknown` (error): a principals key that is not a scalars.yaml#/UserType value;
///   - `mb-partitions-range` (error): partitions outside 1..=32767 (smallint keyspace);
///   - `id-missing` (warn, adoption — like lc-missing): an aggregate with no `identity`;
///   - `identity-untyped` (error, ADR-20260731-214500 consequences): a bare-string `identity` —
///     the catalog is fully migrated to `identity: { $ref: '#/<Actor>/state/<field>' }`;
///   - `identity-state-field-missing` (error): an identity $ref that does not land on a state
///     field of the SAME actor (`#/<Actor>/state/<field>`) — the identity field is the stream key,
///     implicitly declared by this very ref (an explicit `state:` entry of the name also counts);
///   - `identity-property-not-on-command` (warn — CALIBRATED, see below): a received COMMAND whose
///     payload lacks the identity property. Warn, not error, because the current generation
///     legitimately tolerates it: `command_addressing` maps such a command to `identity_prop:
///     None` and the edge mints an ADDRESSING-ONLY actor_id (correct for a birth/side-effect
///     command whose id the server mints — today exactly `RequestPhoneVerification`). Making this
///     an error needs a DSL marker for server-minted ids first (a plan-mode decision);
///   - `id-not-in-payload` (warn, PROP-20260728-152752 §8): the same gap on a received EVENT
///     (fan-out facts may key differently — the Payment refund legs); reminder self-messages are
///     exempt (the reminder row itself carries the actor_id, so no payload key is needed).
fn validate_mailbox_addressing(model: &Model, issues: &mut Vec<Issue>) {
    let actors = match model.defs.get("actors.yaml") {
        Some(Value::Mapping(m)) => m,
        _ => return,
    };
    let user_types = scalar_enum_values(model, "UserType").unwrap_or_default();
    if let Some(pr) = actors.get("principals").and_then(|v| v.as_mapping()) {
        for (k, _) in pr {
            let Some(role) = k.as_str() else { continue };
            if !user_types.iter().any(|u| u == role) {
                issues.push(err(
                    "pr-role-unknown",
                    format!("actors.yaml/principals/{}", role),
                    format!("principals key '{}' is not a scalars.yaml#/UserType value.", role),
                ));
            }
        }
    }
    for (k, node) in actors {
        let name = match k.as_str() {
            Some(s) if s != "principals" => s,
            _ => continue,
        };
        // The activations sub-block is legal on ANY mailbox actor (aggregate or PM) — validate
        // it before the aggregate-only addressing checks below.
        validate_mailbox_activations(node, name, issues);
        if node.get("type").and_then(|x| x.as_str()) != Some("aggregate") {
            continue;
        }
        if let Some(p) = node.get("mailbox").and_then(|m| m.get("partitions")) {
            if !p.as_i64().map(|n| (1..=32767).contains(&n)).unwrap_or(false) {
                issues.push(err(
                    "mb-partitions-range",
                    format!("actors.yaml/{}", name),
                    format!("mailbox.partitions must be an integer in 1..=32767 (smallint keyspace width), got {:?}.", p),
                ));
            }
        }
        let Some(identity_node) = node.get("identity") else {
            issues.push(warn(
                "id-missing",
                format!("actors.yaml/{}", name),
                format!(
                    "aggregate '{}' declares no `identity` — the mailbox cannot address its instances (PROP-20260728-152752 §2).",
                    name
                ),
            ));
            continue;
        };
        if let Some(bare) = identity_node.as_str() {
            issues.push(err(
                "identity-untyped",
                format!("actors.yaml/{}", name),
                format!(
                    "identity is the bare string '{bare}' — migrate to the typed form `identity: {{ $ref: '#/{name}/state/{bare}' }}` (ADR-20260731-214500 consequences: typed $refs everywhere).",
                ),
            ));
            continue;
        }
        let Some(identity) = actor_identity_field(node, name) else {
            let raw = identity_node
                .get("$ref")
                .and_then(|r| r.as_str())
                .unwrap_or("<no $ref>");
            issues.push(err(
                "identity-state-field-missing",
                format!("actors.yaml/{}", name),
                format!(
                    "identity $ref '{raw}' does not land on a state field of '{name}' — expected `#/{name}/state/<field>` (the identity IS the actor's stream-key state field, declared by this ref; an explicit `state:` entry of that name also satisfies it).",
                ),
            ));
            continue;
        };
        for entry in node.get("receives").and_then(|r| r.as_sequence()).into_iter().flatten() {
            let Some(mref) = entry.get("message").and_then(|m| m.get("$ref")).and_then(|r| r.as_str()) else {
                continue;
            };
            // A reminder self-message needs no identity property: the reminder ROW carries the
            // actor_id (message_id = UUIDv5(actor_id, name)), so delivery is self-addressed.
            if reminder_ref_parts(mref).is_some() {
                continue;
            }
            if !message_property_exists(model, mref, &identity) {
                let is_command =
                    ref_target_file(mref, "actors.yaml").as_deref() == Some("commands.yaml");
                if is_command {
                    issues.push(warn(
                        "identity-property-not-on-command",
                        format!("actors.yaml/{}", name),
                        format!(
                            "command '{}' has no payload property '{}' — the mailbox mints an ADDRESSING-ONLY actor_id for it (legitimate only when the server mints the id; a non-birth command missing it would mis-address every delivery).",
                            mref, identity
                        ),
                    ));
                } else {
                    issues.push(warn(
                        "id-not-in-payload",
                        format!("actors.yaml/{}", name),
                        format!(
                            "'{}' does not carry identity property '{}' — a birth message minting its id, or a gap the slice-3 dispatch must resolve.",
                            mref, identity
                        ),
                    ));
                }
            }
        }
    }
}

/// The optional `mailbox.activations` sub-block (PROP-20260728-152752 §3.5, #272 D3): `false`
/// opts the actor out of the ACTOR_ACTIVATIONS-gated held-state cache; a mapping tunes it
/// (`enabled`, `idle_seconds` — the per-actor passivation override). Anything else is a shape
/// error: a knob that parses to nothing would silently run the global defaults.
fn validate_mailbox_activations(node: &Value, name: &str, issues: &mut Vec<Issue>) {
    let Some(act) = node.get("mailbox").and_then(|m| m.get("activations")) else {
        return;
    };
    match act {
        Value::Bool(_) => {}
        Value::Mapping(m) => {
            for (k, v) in m {
                match k.as_str() {
                    Some("enabled") => {
                        if v.as_bool().is_none() {
                            issues.push(err(
                                "mb-activations-shape",
                                format!("actors.yaml/{}", name),
                                format!("mailbox.activations.enabled must be a bool, got {:?}.", v),
                            ));
                        }
                    }
                    Some("idle_seconds") => {
                        if !v.as_i64().map(|n| n >= 1).unwrap_or(false) {
                            issues.push(err(
                                "mb-activations-shape",
                                format!("actors.yaml/{}", name),
                                format!("mailbox.activations.idle_seconds must be an integer >= 1, got {:?} (to disable the cache for this actor, use `activations: false`).", v),
                            ));
                        }
                    }
                    other => {
                        issues.push(err(
                            "mb-activations-shape",
                            format!("actors.yaml/{}", name),
                            format!("mailbox.activations knows only `enabled` and `idle_seconds`, got {:?}.", other),
                        ));
                    }
                }
            }
        }
        other => {
            issues.push(err(
                "mb-activations-shape",
                format!("actors.yaml/{}", name),
                format!("mailbox.activations must be a bool or a {{enabled, idle_seconds}} mapping, got {:?}.", other),
            ));
        }
    }
}

/// Split a lineage ref into (event name, Some(property)) — "events.yaml#/X/properties/y" — or
/// (event name, None) for a whole-event ref "events.yaml#/X".
fn lineage_parts(r: &str) -> Option<(&str, Option<&str>)> {
    let rest = r.strip_prefix("events.yaml#/")?;
    match rest.split_once("/properties/") {
        Some((ev, prop)) => Some((ev, Some(prop))),
        None => (!rest.contains('/')).then_some((rest, None)),
    }
}

/// The `$ref` string of an event property's type, or None (inline primitive like `type: boolean`).
fn event_property_type_ref(model: &Model, event: &str, prop: &str) -> Option<String> {
    let node = model.defs.get("events.yaml")?.get(event)?.get("properties")?.get(prop)?;
    node.get("$ref").and_then(|r| r.as_str()).map(str::to_string)
}

/// §2e — declared aggregate STATE + write-side `requires` (ADR-20260730-231500,
/// PROP-20260728-135632 §2): the state block is a typed, event-lineaged fold declaration; the
/// requires block is the per-instance authorization contract over it. Validation only in slice 1
/// (#242 — generation and enforcement are slice 2):
///   - `st-status-duplicated` (error): a state field named `status` next to a `lifecycle` block;
///   - `st-event-foreign` (error): a lineage event this aggregate neither emits nor receives;
///   - `st-type-mismatch` (error): a state field whose `type` $ref differs from its lineage
///     property's $ref;
///   - `st-shape` (error): mode/shape contradictions (set without `of`; flag/count lineage that
///     names properties; latest lineage that names whole events);
///   - `requires-acting-untyped` (error, ADR-20260731-214500 consequences): an acting value that
///     is a bare `state.<field>` path string (or any non-`any` bare string) — migrated to the
///     typed form `{ $ref: '#/<Actor>/state/<field>' }`; `any` stays a bare keyword;
///   - `req-state-unknown` (error): an acting $ref that is not a same-actor `#/<Actor>/state/…`
///     ref, or that names an undeclared state field;
///   - `req-principal-type` (error): the acting role's principals id scalar differs from the
///     state field's type;
///   - `req-principal-missing` (error): a non-`any` acting entry for a role absent from
///     `principals` (roles without a domain identity can only ever be `any`);
///   - `req-claim-unknown` (error): a `claims` key that is not a property of the command payload,
///     or a value other than `actor.role` / `actor.id`.
fn validate_actor_state(model: &Model, issues: &mut Vec<Issue>) {
    let actors = match model.defs.get("actors.yaml") {
        Some(Value::Mapping(m)) => m,
        _ => return,
    };
    // principals: role -> id scalar ref
    let mut principal_ids: BTreeMap<String, String> = BTreeMap::new();
    if let Some(pr) = actors.get("principals").and_then(|v| v.as_mapping()) {
        for (k, v) in pr {
            if let (Some(role), Some(id)) =
                (k.as_str(), v.get("id").and_then(|i| i.get("$ref")).and_then(|r| r.as_str()))
            {
                principal_ids.insert(role.to_string(), id.to_string());
            }
        }
    }
    for (k, node) in actors {
        let name = match k.as_str() {
            Some(s) if s != "principals" => s,
            _ => continue,
        };
        if node.get("type").and_then(|x| x.as_str()) != Some("aggregate") {
            continue;
        }
        let site = |suffix: String| format!("actors.yaml/{}{}", name, suffix);
        // The aggregate's event universe: everything it emits or receives.
        let mut own_events: BTreeSet<String> = BTreeSet::new();
        for entry in node.get("receives").and_then(|r| r.as_sequence()).into_iter().flatten() {
            for r in [entry.get("message")].into_iter().flatten() {
                if let Some(s) = r.get("$ref").and_then(|x| x.as_str()) {
                    if let Some(ev) = s.strip_prefix("events.yaml#/") {
                        own_events.insert(ev.to_string());
                    }
                }
            }
            for e in entry.get("emits").and_then(|x| x.as_sequence()).into_iter().flatten() {
                if let Some(s) = e.get("$ref").and_then(|x| x.as_str()) {
                    if let Some(ev) = s.strip_prefix("events.yaml#/") {
                        own_events.insert(ev.to_string());
                    }
                }
            }
        }
        // ---- state ----
        let state = node.get("state").and_then(|s| s.as_mapping());
        let mut state_types: BTreeMap<String, Option<String>> = BTreeMap::new(); // field -> type $ref
        if let Some(state) = state {
            for (fk, field) in state {
                let Some(fname) = fk.as_str() else { continue };
                let fsite = || site(format!("/state/{}", fname));
                if fname == "status" && node.get("lifecycle").is_some() {
                    issues.push(err(
                        "st-status-duplicated",
                        fsite(),
                        format!("state field 'status' on '{}' — the lifecycle block already owns it (one field, one owner).", name),
                    ));
                }
                let mode = field.get("mode").and_then(|m| m.as_str()).unwrap_or("latest");
                let type_ref = field.get("type").and_then(|t| t.get("$ref")).and_then(|r| r.as_str()).map(str::to_string);
                state_types.insert(fname.to_string(), type_ref.clone());
                if mode == "set" && field.get("of").is_none() {
                    issues.push(err("st-shape", fsite(), format!("mode `set` on '{}.{}' needs `of` (the element type).", name, fname)));
                }
                for key in ["from", "removedBy"] {
                    for r in field.get(key).and_then(|f| f.as_sequence()).into_iter().flatten() {
                        let Some(rs) = r.get("$ref").and_then(|x| x.as_str()) else { continue };
                        let Some((ev, prop)) = lineage_parts(rs) else {
                            issues.push(err("st-shape", fsite(), format!("lineage ref '{}' is not an events.yaml event or event property.", rs)));
                            continue;
                        };
                        if !own_events.contains(ev) {
                            issues.push(err(
                                "st-event-foreign",
                                fsite(),
                                format!("lineage event '{}' is neither emitted nor received by '{}'.", ev, name),
                            ));
                        }
                        match (mode, prop) {
                            ("flag", Some(_)) | ("count", Some(_)) => issues.push(err(
                                "st-shape",
                                fsite(),
                                format!("mode `{}` folds whole-event occurrences — lineage must not name a property ('{}').", mode, rs),
                            )),
                            ("latest", None) => issues.push(err(
                                "st-shape",
                                fsite(),
                                format!("mode `latest` folds a carried property — whole-event ref '{}' needs mode flag/count or a /properties/ path.", rs),
                            )),
                            ("latest", Some(p)) => {
                                if let (Some(want), Some(got)) = (type_ref.as_deref(), event_property_type_ref(model, ev, p).as_deref()) {
                                    if want != got {
                                        issues.push(err(
                                            "st-type-mismatch",
                                            fsite(),
                                            format!("'{}.{}' is typed {} but lineage property '{}/{}' is {}.", name, fname, want, ev, p, got),
                                        ));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        // ---- requires ----
        for entry in node.get("receives").and_then(|r| r.as_sequence()).into_iter().flatten() {
            let Some(req) = entry.get("requires") else { continue };
            let mref = entry.get("message").and_then(|m| m.get("$ref")).and_then(|r| r.as_str()).unwrap_or("");
            let rsite = || site(format!("/requires[{}]", mref));
            for (rk, rv) in req.get("acting").and_then(|a| a.as_mapping()).into_iter().flatten() {
                let Some(role) = rk.as_str() else { continue };
                // `any` is the only bare keyword; every binding value is a typed same-actor
                // state-field $ref (ADR-20260731-214500 consequences: typed $refs everywhere).
                let field: String = if let Some(val) = rv.as_str() {
                    if val == "any" {
                        continue;
                    }
                    let suggested = val.strip_prefix("state.").unwrap_or("<field>");
                    issues.push(err(
                        "requires-acting-untyped",
                        rsite(),
                        format!(
                            "acting.{role} is the bare string '{val}' — migrate to `{{ $ref: '#/{name}/state/{suggested}' }}` (`any` is the only bare keyword)."
                        ),
                    ));
                    continue;
                } else if let Some(r) = rv.get("$ref").and_then(|x| x.as_str()) {
                    match parse_ref(r) {
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
                                "req-state-unknown",
                                rsite(),
                                format!(
                                    "acting.{role} $ref '{r}' must point at a state field of the SAME actor (`#/{name}/state/<field>`)."
                                ),
                            ));
                            continue;
                        }
                    }
                } else {
                    issues.push(err(
                        "requires-acting-untyped",
                        rsite(),
                        format!(
                            "acting.{role} must be `any` or a `$ref` to `#/{name}/state/<field>`."
                        ),
                    ));
                    continue;
                };
                let field = field.as_str();
                let Some(ftype) = state_types.get(field) else {
                    issues.push(err("req-state-unknown", rsite(), format!("acting.{} references undeclared state field '{}'.", role, field)));
                    continue;
                };
                match principal_ids.get(role) {
                    None => issues.push(err(
                        "req-principal-missing",
                        rsite(),
                        format!("acting.{} compares a domain identity, but '{}' has no principals entry — roles without one can only be `any`.", role, role),
                    )),
                    Some(pid) => {
                        if ftype.as_deref() != Some(pid.as_str()) {
                            issues.push(err(
                                "req-principal-type",
                                rsite(),
                                format!("acting.{}: state.{} is typed {:?} but principals.{}.id is '{}'.", role, field, ftype, role, pid),
                            ));
                        }
                    }
                }
            }
            for (ck, cv) in req.get("claims").and_then(|c| c.as_mapping()).into_iter().flatten() {
                let (Some(prop), Some(val)) = (ck.as_str(), cv.as_str()) else { continue };
                if !matches!(val, "actor.role" | "actor.id") {
                    issues.push(err("req-claim-unknown", rsite(), format!("claims.{} must pin to `actor.role` or `actor.id`, got '{}'.", prop, val)));
                }
                if !mref.is_empty() && !message_property_exists(model, mref, prop) {
                    issues.push(err("req-claim-unknown", rsite(), format!("claims key '{}' is not a property of '{}'.", prop, mref)));
                }
            }
        }
    }
}

// ─── §2f — actor reminders + declarative deletion (ADR-20260731-214500) ─────────────────────────

/// A declared per-actor typed self-message (`reminders:`, ADR-20260731-120825 as renamed by
/// ADR-20260731-214500). Tolerant parsing (missing pieces → empty/None); §2f reports the holes.
struct ReminderDef {
    actor: String,
    name: String,
    /// `payload.$ref` — MUST target events.yaml (fact vocabulary, ADR-20260731-153000 §1a).
    payload_ref: String,
    /// A declared `identity:` field — always an ERROR (`reminder-identity-declared`): a reminder's
    /// identity is DERIVED, `UUIDv5(actor_id, reminder name)` (the runtime's `reminder_message_id`
    /// computes it), never declared (ADR-20260731-214500 consequences).
    identity_declared: bool,
    /// Optional default window: `after.$ref` into configuration.yaml keys.
    after_ref: Option<String>,
    /// Reschedule semantics — only `in-place` exists (ADR-20260731-150500).
    reschedule: Option<String>,
}

/// One `deletion.triggers[*]` entry: `on` + optional window (`after`), undo (`cancelled_on`) and
/// the strongly-typed propagation `match` (event property ↔ child state field).
struct DeletionTriggerDef {
    on: Vec<String>,
    after_ref: Option<String>,
    cancelled_on: Vec<String>,
    match_event_ref: Option<String>,
    match_state_ref: Option<String>,
    has_match: bool,
}

/// A parsed `deletion:` block of an actors.yaml actor (ADR-20260731-214500 §3).
struct DeletionDef {
    actor: String,
    triggers: Vec<DeletionTriggerDef>,
    /// `receipt.$ref` — the business fact recorded on the deletion ledger when the journey completes.
    receipt_ref: Option<String>,
}

/// `(actor, reminder)` when `r` points into an actors.yaml `reminders:` section — the self-message
/// form `#/<Actor>/reminders/<Name>` (or spelled `actors.yaml#/<Actor>/reminders/<Name>`).
fn reminder_ref_parts(r: &str) -> Option<(String, String)> {
    let pr = parse_ref(r)?;
    if !pr.file.is_empty() && pr.file != "actors.yaml" {
        return None;
    }
    if pr.path.len() == 3 && pr.path[1] == "reminders" {
        Some((pr.path[0].clone(), pr.path[2].clone()))
    } else {
        None
    }
}

/// The events.yaml event name a reminder message ref delivers (its `payload.$ref`), or None.
fn reminder_payload_event(model: &Model, message_ref: &str) -> Option<String> {
    let (actor, name) = reminder_ref_parts(message_ref)?;
    let payload = model
        .defs
        .get("actors.yaml")?
        .get(actor.as_str())?
        .get("reminders")?
        .get(name.as_str())?
        .get("payload")?
        .get("$ref")?
        .as_str()?;
    if ref_target_file(payload, "actors.yaml").as_deref() != Some("events.yaml") {
        return None;
    }
    ref_name(payload)
}

/// The configuration key name a window `$ref` denotes (`configuration.yaml#/keys/<KEY>`), or None
/// when the ref has any other shape.
fn config_key_ref_name(r: &str) -> Option<String> {
    let pr = parse_ref(r)?;
    (pr.file == "configuration.yaml" && pr.path.len() == 2 && pr.path[0] == "keys")
        .then(|| pr.path[1].clone())
}

/// Parse every actor's `reminders:` map, in actors.yaml order.
fn parse_reminders(model: &Model) -> Vec<ReminderDef> {
    let mut out = Vec::new();
    let Some(Value::Mapping(actors)) = model.defs.get("actors.yaml") else { return out };
    for (k, node) in actors {
        let Some(actor) = k.as_str().filter(|s| *s != "principals") else { continue };
        let Some(rem) = node.get("reminders").and_then(|r| r.as_mapping()) else { continue };
        for (rk, rv) in rem {
            let Some(name) = rk.as_str() else { continue };
            out.push(ReminderDef {
                actor: actor.to_string(),
                name: name.to_string(),
                payload_ref: rv
                    .get("payload")
                    .and_then(|p| p.get("$ref"))
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string(),
                identity_declared: rv.get("identity").is_some(),
                after_ref: rv
                    .get("after")
                    .and_then(|p| p.get("$ref"))
                    .and_then(|r| r.as_str())
                    .map(str::to_string),
                reschedule: rv.get("reschedule").and_then(|x| x.as_str()).map(str::to_string),
            });
        }
    }
    out
}

/// Parse every actor's `deletion:` block, in actors.yaml order.
fn parse_deletions(model: &Model) -> Vec<DeletionDef> {
    let mut out = Vec::new();
    let Some(Value::Mapping(actors)) = model.defs.get("actors.yaml") else { return out };
    for (k, node) in actors {
        let Some(actor) = k.as_str().filter(|s| *s != "principals") else { continue };
        let Some(del) = node.get("deletion") else { continue };
        let triggers = del
            .get("triggers")
            .and_then(|t| t.as_sequence())
            .map(|seq| {
                seq.iter()
                    .map(|t| {
                        let m = t.get("match");
                        DeletionTriggerDef {
                            on: ref_strings(t.get("on")),
                            after_ref: t
                                .get("after")
                                .and_then(|a| a.get("$ref"))
                                .and_then(|r| r.as_str())
                                .map(str::to_string),
                            cancelled_on: ref_strings(t.get("cancelled_on")),
                            match_event_ref: m
                                .and_then(|x| x.get("event"))
                                .and_then(|e| e.get("$ref"))
                                .and_then(|r| r.as_str())
                                .map(str::to_string),
                            match_state_ref: m
                                .and_then(|x| x.get("state"))
                                .and_then(|s| s.get("$ref"))
                                .and_then(|r| r.as_str())
                                .map(str::to_string),
                            has_match: m.is_some(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(DeletionDef {
            actor: actor.to_string(),
            triggers,
            receipt_ref: del
                .get("receipt")
                .and_then(|r| r.get("$ref"))
                .and_then(|r| r.as_str())
                .map(str::to_string),
        });
    }
    out
}

/// §2f — the `reminders:` / `schedules:` / `deletion:` DSL (ADR-20260731-214500, refining
/// ADR-20260731-120825/-150500/-153000). Each rule maps to a way the declarations could silently
/// stop being trustworthy:
///   - `reminder-payload-not-event` (error): a reminder payload that is not an events.yaml FACT —
///     record semantics only, never a command (ADR-20260731-153000 §1a);
///   - `reminder-after-unresolved` (error): a reminder window that is not a resolving
///     `configuration.yaml#/keys/<KEY>` ref (never a bare string);
///   - `reminder-reschedule-unknown` (error): a `reschedule` value other than `in-place` — the only
///     semantics that exist (ADR-20260731-150500);
///   - `reminder-identity-declared` (error): a reminder declaring `identity:` — it is DERIVED
///     (`UUIDv5(actor_id, reminder name)`, the runtime's `reminder_message_id`), never declared
///     (ADR-20260731-214500 consequences);
///   - `reminder-without-receive` (error): a declared reminder the SAME actor does not receive —
///     strong typing means handler-proof, per actor (ADR-20260731-120825 §2);
///   - `receive-without-reminder` (error): a receives message ref into a `reminders:` section that
///     does not resolve to a reminder declared on that same actor;
///   - `schedules-unresolved` (error): a `schedules:` entry that is not a resolving SAME-actor
///     reminder ref (scheduling is the handler's third observable effect);
///   - `deletion-ref-unresolved` (error): a deletion `on`/`cancelled_on`/`receipt` that is not a
///     resolving events.yaml event, an `after` that is not a resolving configuration key, an empty
///     `on`/`triggers` list, or a missing `receipt`;
///   - `deletion-match-untyped` (error): a propagation trigger (no `after`) without a `match`, or a
///     `match` whose `event` is not a property of a triggering `on` event / whose `state` is not a
///     declared state field of THIS actor — bare string paths are barred (ADR-20260731-214500 §3);
///   - `deletion-tree-cycle` (error): the propagation graph (edge A → B when B's `on` lists A's
///     `receipt` fact) must be acyclic — the dependency tree EMERGES from child declarations, and a
///     cycle would make the generic engine chase its own tail.
fn validate_reminders_and_deletion(model: &Model, issues: &mut Vec<Issue>) {
    let reminders = parse_reminders(model);
    let deletions = parse_deletions(model);
    let declared: BTreeSet<(String, String)> =
        reminders.iter().map(|r| (r.actor.clone(), r.name.clone())).collect();

    // ---- reminders: payload / window / reschedule hygiene ----
    for r in &reminders {
        let at = format!("actors.yaml/{}/reminders/{}", r.actor, r.name);
        let payload_ok = ref_target_file(&r.payload_ref, "actors.yaml").as_deref()
            == Some("events.yaml")
            && resolve_ref(model, &r.payload_ref, "actors.yaml").is_some();
        if !payload_ok {
            issues.push(err(
                "reminder-payload-not-event",
                at.clone(),
                format!(
                    "reminder payload must be a resolving events.yaml FACT (record semantics, ADR-20260731-153000), got '{}'.",
                    r.payload_ref
                ),
            ));
        }
        if let Some(a) = &r.after_ref {
            let resolves = config_key_ref_name(a)
                .map(|_| resolve_ref(model, a, "actors.yaml").is_some())
                .unwrap_or(false);
            if !resolves {
                issues.push(err(
                    "reminder-after-unresolved",
                    at.clone(),
                    format!("`after` must be a resolving configuration.yaml#/keys/<KEY> ref, got '{}'.", a),
                ));
            }
        }
        if let Some(rs) = &r.reschedule {
            if rs != "in-place" {
                issues.push(err(
                    "reminder-reschedule-unknown",
                    at.clone(),
                    format!("reschedule '{}' — only `in-place` exists (ADR-20260731-150500).", rs),
                ));
            }
        }
        if r.identity_declared {
            issues.push(err(
                "reminder-identity-declared",
                at.clone(),
                "reminder `identity` is DERIVED — UUIDv5(actor_id, reminder name), computed by the runtime's reminder_message_id — never declared; remove the field (ADR-20260731-214500 consequences).".into(),
            ));
        }
    }

    // ---- receives ↔ reminders (both ways) + schedules, per actors.yaml actor ----
    let actors = match model.defs.get("actors.yaml") {
        Some(Value::Mapping(m)) => m,
        _ => return,
    };
    // (actor, reminder) pairs some receives entry handles — for `reminder-without-receive`.
    let mut received: BTreeSet<(String, String)> = BTreeSet::new();
    for (k, node) in actors {
        let Some(actor) = k.as_str().filter(|s| *s != "principals") else { continue };
        for (i, entry) in
            node.get("receives").and_then(|r| r.as_sequence()).into_iter().flatten().enumerate()
        {
            let at = format!("actors.yaml/{}.receives[{}]", actor, i);
            if let Some(mref) =
                entry.get("message").and_then(|m| m.get("$ref")).and_then(|r| r.as_str())
            {
                if let Some((ra, rn)) = reminder_ref_parts(mref) {
                    if ra != actor || !declared.contains(&(ra.clone(), rn.clone())) {
                        issues.push(err(
                            "receive-without-reminder",
                            format!("{}.message", at),
                            format!(
                                "message '{}' does not resolve to a reminder declared on '{}' — a reminder is one actor talking to ITSELF (ADR-20260731-120825).",
                                mref, actor
                            ),
                        ));
                    } else {
                        received.insert((ra, rn));
                    }
                }
            }
            for (j, sref) in ref_strings(entry.get("schedules")).into_iter().enumerate() {
                let ok = reminder_ref_parts(&sref)
                    .map(|(ra, rn)| ra == actor && declared.contains(&(ra.clone(), rn)))
                    .unwrap_or(false);
                if !ok {
                    issues.push(err(
                        "schedules-unresolved",
                        format!("{}.schedules[{}]", at, j),
                        format!(
                            "'{}' does not resolve to a reminder declared on '{}' (expected #/{}/reminders/<Name>).",
                            sref, actor, actor
                        ),
                    ));
                }
            }
        }
    }
    for r in &reminders {
        if !received.contains(&(r.actor.clone(), r.name.clone())) {
            issues.push(err(
                "reminder-without-receive",
                format!("actors.yaml/{}/reminders/{}", r.actor, r.name),
                format!(
                    "no receives entry on '{}' handles this reminder — add {{ message: {{ $ref: '#/{}/reminders/{}' }} }} (strong typing means handler-proof, ADR-20260731-120825).",
                    r.actor, r.actor, r.name
                ),
            ));
        }
    }

    // ---- deletion blocks ----
    for d in &deletions {
        let at = format!("actors.yaml/{}/deletion", d.actor);
        // The actor's declared state fields — what a `match.state` may bind to. The typed
        // `identity` ref IMPLICITLY declares its field (the stream key exists before any fold,
        // same doctrine as `is_implicit_identity_state_ref`), so a self-trigger may match on it
        // without forcing an explicit `state:` entry into the aggregate.
        let mut state_fields: BTreeSet<String> = actors
            .get(d.actor.as_str())
            .and_then(|n| n.get("state"))
            .and_then(|s| s.as_mapping())
            .map(|m| m.keys().filter_map(|k| k.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        if let Some(f) =
            actors.get(d.actor.as_str()).and_then(|n| actor_identity_field(n, &d.actor))
        {
            state_fields.insert(f);
        }
        match &d.receipt_ref {
            None => issues.push(err(
                "deletion-ref-unresolved",
                at.clone(),
                "deletion declares no `receipt` — the completing fact recorded on the deletion ledger (ADR-20260731-160000 §6).".into(),
            )),
            Some(r) => {
                let ok = ref_target_file(r, "actors.yaml").as_deref() == Some("events.yaml")
                    && resolve_ref(model, r, "actors.yaml").is_some();
                if !ok {
                    issues.push(err(
                        "deletion-ref-unresolved",
                        format!("{}.receipt", at),
                        format!("receipt must be a resolving events.yaml event, got '{}'.", r),
                    ));
                }
            }
        }
        if d.triggers.is_empty() {
            issues.push(err(
                "deletion-ref-unresolved",
                at.clone(),
                "deletion declares no `triggers` — nothing would ever start the journey.".into(),
            ));
        }
        for (i, t) in d.triggers.iter().enumerate() {
            let tat = format!("{}.triggers[{}]", at, i);
            if t.on.is_empty() {
                issues.push(err(
                    "deletion-ref-unresolved",
                    tat.clone(),
                    "trigger declares no `on` events — it can never fire.".into(),
                ));
            }
            let mut on_events: BTreeSet<String> = BTreeSet::new();
            for (key, refs) in [("on", &t.on), ("cancelled_on", &t.cancelled_on)] {
                for (j, r) in refs.iter().enumerate() {
                    let ok = ref_target_file(r, "actors.yaml").as_deref() == Some("events.yaml")
                        && resolve_ref(model, r, "actors.yaml").is_some();
                    if !ok {
                        issues.push(err(
                            "deletion-ref-unresolved",
                            format!("{}.{}[{}]", tat, key, j),
                            format!("'{}' is not a resolving events.yaml event.", r),
                        ));
                    } else if key == "on" {
                        if let Some(n) = ref_name(r) {
                            on_events.insert(n);
                        }
                    }
                }
            }
            if let Some(a) = &t.after_ref {
                let resolves = config_key_ref_name(a)
                    .map(|_| resolve_ref(model, a, "actors.yaml").is_some())
                    .unwrap_or(false);
                if !resolves {
                    issues.push(err(
                        "deletion-ref-unresolved",
                        format!("{}.after", tat),
                        format!("`after` must be a resolving configuration.yaml#/keys/<KEY> ref, got '{}'.", a),
                    ));
                }
            }
            // `match` — required for propagation (no `after`), allowed with one; always typed.
            if !t.has_match {
                if t.after_ref.is_none() {
                    issues.push(err(
                        "deletion-match-untyped",
                        tat.clone(),
                        "propagation trigger (no `after`) needs a typed `match` — the engine enumerates child instances by it (ADR-20260731-214500 §3).".into(),
                    ));
                }
                continue;
            }
            match &t.match_event_ref {
                None => issues.push(err(
                    "deletion-match-untyped",
                    format!("{}.match", tat),
                    "match declares no `event` — a $ref to the triggering event's property is required.".into(),
                )),
                Some(r) => {
                    let parts = lineage_parts(r);
                    match parts {
                        Some((ev, Some(_))) if resolve_ref(model, r, "actors.yaml").is_some() => {
                            if !on_events.contains(ev) {
                                issues.push(err(
                                    "deletion-match-untyped",
                                    format!("{}.match.event", tat),
                                    format!("match.event names '{}', which is not one of this trigger's `on` events.", ev),
                                ));
                            }
                        }
                        _ => issues.push(err(
                            "deletion-match-untyped",
                            format!("{}.match.event", tat),
                            format!("'{}' is not a resolving events.yaml#/<Event>/properties/<prop> ref.", r),
                        )),
                    }
                }
            }
            match &t.match_state_ref {
                None => issues.push(err(
                    "deletion-match-untyped",
                    format!("{}.match", tat),
                    "match declares no `state` — a $ref to this actor's state field is required.".into(),
                )),
                Some(r) => {
                    let ok = parse_ref(r)
                        .filter(|pr| pr.file.is_empty() || pr.file == "actors.yaml")
                        .filter(|pr| {
                            pr.path.len() == 3
                                && pr.path[0] == d.actor
                                && pr.path[1] == "state"
                                && state_fields.contains(&pr.path[2])
                        })
                        .is_some();
                    if !ok {
                        issues.push(err(
                            "deletion-match-untyped",
                            format!("{}.match.state", tat),
                            format!(
                                "'{}' is not a declared state field of '{}' (expected #/{}/state/<field>).",
                                r, d.actor, d.actor
                            ),
                        ));
                    }
                }
            }
        }
    }

    // ---- deletion propagation tree: acyclic ----
    // Edge A → B when B's `on` lists A's `receipt` fact (the child declares how it dies).
    let receipt_of: BTreeMap<&str, String> = deletions
        .iter()
        .filter_map(|d| d.receipt_ref.as_deref().and_then(ref_name).map(|e| (d.actor.as_str(), e)))
        .collect();
    let mut edges: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for d in &deletions {
        let on: BTreeSet<String> =
            d.triggers.iter().flat_map(|t| t.on.iter()).filter_map(|r| ref_name(r)).collect();
        for (parent, receipt) in &receipt_of {
            if on.contains(receipt) {
                edges.entry(parent).or_default().push(d.actor.as_str());
            }
        }
    }
    // DFS with an explicit path; each cycle is reported once, keyed by its sorted member set.
    let mut reported: BTreeSet<Vec<String>> = BTreeSet::new();
    fn dfs<'a>(
        node: &'a str,
        edges: &BTreeMap<&'a str, Vec<&'a str>>,
        path: &mut Vec<&'a str>,
        done: &mut BTreeSet<&'a str>,
        reported: &mut BTreeSet<Vec<String>>,
        issues: &mut Vec<Issue>,
    ) {
        if let Some(pos) = path.iter().position(|n| *n == node) {
            let cycle: Vec<&str> = path[pos..].to_vec();
            let mut key: Vec<String> = cycle.iter().map(|s| s.to_string()).collect();
            key.sort();
            if reported.insert(key) {
                issues.push(err(
                    "deletion-tree-cycle",
                    format!("actors.yaml/{}/deletion", cycle[0]),
                    format!(
                        "deletion propagation cycle: {} -> {} — the emergent dependency tree must be acyclic (ADR-20260731-214500).",
                        cycle.join(" -> "),
                        node
                    ),
                ));
            }
            return;
        }
        if done.contains(node) {
            return;
        }
        path.push(node);
        for next in edges.get(node).cloned().unwrap_or_default() {
            dfs(next, edges, path, done, reported, issues);
        }
        path.pop();
        done.insert(node);
    }
    let mut done: BTreeSet<&str> = BTreeSet::new();
    let roots: Vec<&str> = edges.keys().copied().collect();
    for root in roots {
        let mut path = Vec::new();
        dfs(root, &edges, &mut path, &mut done, &mut reported, issues);
    }
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
    let all_reminders = parse_reminders(model);
    let all_deletions = parse_deletions(model);
    let actor_docs: Vec<Doc> = actors.iter().map(|a| {
        let rows: Vec<Vec<String>> = a.receives.iter().map(|e| {
            // A reminder self-message (ADR-20260731-214500) has no cross-file anchor — render it
            // inline with its ⏰ marker; commands/events keep their links.
            let msg = match reminder_ref_parts(&e.message_ref) {
                Some((_, rname)) => format!("⏰ `{}` _(reminder)_", rname),
                None => {
                    let msg_name = ref_name(&e.message_ref).unwrap_or_else(|| "?".to_string());
                    let is_cmd = e.message_ref.starts_with("commands.yaml#/");
                    dlink(if is_cmd { "command" } else { "event" }, &msg_name)
                }
            };
            let emits = {
                let mut cells: Vec<String> = e.emits.iter().map(|r| dlink("event", &ref_name(r).unwrap_or_default())).collect();
                cells.extend(e.schedules.iter().filter_map(|r| reminder_ref_parts(r)).map(|(_, n)| format!("⏰ schedules `{}`", n)));
                let s = cells.join(", ");
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
        let rems: Vec<&ReminderDef> = all_reminders.iter().filter(|r| r.actor == a.name).collect();
        if !rems.is_empty() {
            let rrows: Vec<Vec<String>> = rems.iter().map(|r| vec![
                format!("⏰ `{}`", r.name),
                dlink("event", &ref_name(&r.payload_ref).unwrap_or_else(|| "?".to_string())),
                r.after_ref.as_deref().and_then(config_key_ref_name).map(|k| format!("⚙️ `{}`", k)).unwrap_or_else(|| "—".to_string()),
                r.reschedule.clone().unwrap_or_else(|| "in-place".to_string()),
            ]).collect();
            parts.push(format!("\nReminders (self-scheduled facts — ADR-20260731-214500):\n\n{}", md_table(&["Reminder", "Payload", "After", "Reschedule"], &rrows)));
        }
        if let Some(d) = all_deletions.iter().find(|d| d.actor == a.name) {
            let trows: Vec<Vec<String>> = d.triggers.iter().map(|t| {
                let on = { let s = t.on.iter().map(|r| dlink("event", &ref_name(r).unwrap_or_default())).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
                let window = t.after_ref.as_deref().and_then(config_key_ref_name).map(|k| format!("⚙️ `{}`", k)).unwrap_or_else(|| "_immediate (propagation)_".to_string());
                let cancelled = { let s = t.cancelled_on.iter().map(|r| dlink("event", &ref_name(r).unwrap_or_default())).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
                let m = match (t.match_event_ref.as_deref().and_then(lineage_parts), t.match_state_ref.as_deref().and_then(parse_ref)) {
                    (Some((ev, Some(prop))), Some(st)) => format!("{} ↔ `state.{}`", dprop_link("event", ev, prop), st.path.last().cloned().unwrap_or_default()),
                    _ => "—".to_string(),
                };
                vec![on, window, cancelled, m]
            }).collect();
            let receipt = d.receipt_ref.as_deref().and_then(ref_name).map(|e| dlink("event", &e)).unwrap_or_else(|| "—".to_string());
            parts.push(format!("\nDeletion (declarative, generic engine — ADR-20260731-214500):\n\n{}\n- **Receipt**: {}", md_table(&["On", "Window", "Cancelled on", "Match"], &trows), receipt));
        }
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
    let all_reminders = parse_reminders(model);
    let all_deletions = parse_deletions(model);
    let actor_docs: Vec<HDoc> = actors.iter().map(|a| {
        let kind = if a.kind == "aggregate" { "🧩 aggregate" } else { "⚙️ process manager" };
        let rows: Vec<Vec<String>> = a.receives.iter().map(|e| {
            let msg = match reminder_ref_parts(&e.message_ref) {
                Some((_, rname)) => format!("⏰ <span class=\"k-id\">{}</span> <span class=\"muted\">(reminder)</span>", h_esc(&rname)),
                None => {
                    let is_cmd = e.message_ref.starts_with("commands.yaml#/");
                    h_link(if is_cmd { "command" } else { "event" }, &ref_name(&e.message_ref).unwrap_or_else(|| "?".to_string()))
                }
            };
            let emits = {
                let mut cells: Vec<String> = e.emits.iter().map(|r| h_link("event", &ref_name(r).unwrap_or_default())).collect();
                cells.extend(e.schedules.iter().filter_map(|r| reminder_ref_parts(r)).map(|(_, n)| format!("⏰ schedules <span class=\"k-id\">{}</span>", h_esc(&n))));
                let s = cells.join(", ");
                if s.is_empty() { e.effect.as_deref().map(|x| format!("<span class=\"muted\">{}</span>", h_esc(x))).unwrap_or_else(|| "—".to_string()) } else { s }
            };
            let throws = { let s = e.throws.iter().map(|r| h_link("error", &ref_name(r).unwrap_or_default())).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
            vec![msg, emits, throws]
        }).collect();
        let mut extras = String::new();
        let rems: Vec<&ReminderDef> = all_reminders.iter().filter(|r| r.actor == a.name).collect();
        if !rems.is_empty() {
            let rrows: Vec<Vec<String>> = rems.iter().map(|r| vec![
                format!("⏰ <span class=\"k-id\">{}</span>", h_esc(&r.name)),
                h_link("event", &ref_name(&r.payload_ref).unwrap_or_else(|| "?".to_string())),
                r.after_ref.as_deref().and_then(config_key_ref_name).map(|k| format!("⚙️ <span class=\"k-const\">{}</span>", h_esc(&k))).unwrap_or_else(|| "—".to_string()),
                h_esc(r.reschedule.as_deref().unwrap_or("in-place")),
            ]).collect();
            extras.push_str(&format!("<div class=\"rel\"><span class=\"lbl\">reminders (self-scheduled facts — ADR-20260731-214500):</span></div>{}", h_table(&["Reminder", "Payload", "After", "Reschedule"], &rrows)));
        }
        if let Some(d) = all_deletions.iter().find(|d| d.actor == a.name) {
            let trows: Vec<Vec<String>> = d.triggers.iter().map(|t| {
                let on = { let s = t.on.iter().map(|r| h_link("event", &ref_name(r).unwrap_or_default())).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
                let window = t.after_ref.as_deref().and_then(config_key_ref_name).map(|k| format!("⚙️ <span class=\"k-const\">{}</span>", h_esc(&k))).unwrap_or_else(|| "<span class=\"muted\">immediate (propagation)</span>".to_string());
                let cancelled = { let s = t.cancelled_on.iter().map(|r| h_link("event", &ref_name(r).unwrap_or_default())).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
                let m = match (t.match_event_ref.as_deref().and_then(lineage_parts), t.match_state_ref.as_deref().and_then(parse_ref)) {
                    (Some((ev, Some(prop))), Some(st)) => format!("{} ↔ <span class=\"k-prop\">state.{}</span>", h_plink("event", ev, prop), h_esc(&st.path.last().cloned().unwrap_or_default())),
                    _ => "—".to_string(),
                };
                vec![on, window, cancelled, m]
            }).collect();
            let receipt = d.receipt_ref.as_deref().and_then(ref_name).map(|e| h_link("event", &e)).unwrap_or_else(|| "—".to_string());
            extras.push_str(&format!("<div class=\"rel\"><span class=\"lbl\">deletion (declarative, generic engine — ADR-20260731-214500):</span></div>{}<div class=\"rel\"><span class=\"lbl\">receipt:</span> {}</div>", h_table(&["On", "Window", "Cancelled on", "Match"], &trows), receipt));
        }
        let seq = if a.kind == "aggregate" {
            lc_state.get(&a.name).map(|d| format!("<div class=\"pm-seq\"><pre class=\"mermaid\">{}</pre></div>", h_esc(d))).unwrap_or_default()
        } else {
            pm_seq.get(&a.name).map(|d| format!("<div class=\"pm-seq\"><pre class=\"mermaid\">{}</pre></div>", h_esc(d))).unwrap_or_default()
        };
        HDoc { ctx: cx.of_actor(&a.name), html: h_item("actor", "Actor", &a.name, &format!("<div class=\"rel muted\">{}</div>{}{}{}", kind, h_table(&["Receives", "Emits →", "Throws"], &rows), extras, seq), a.description.as_deref()) }
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

    let Report { mut issues, coverage, handled_commands } = validate(&model);
    // ─── §13 — proposal hygiene (docs/proposals/PROP-*.md, #272) — runs alongside the spec
    // validator: same issue list, same gate, but reads the proposals corpus from the repo root
    // (derived from `--specs`) because proposals are markdown, not part of the YAML model.
    let proposals = load_proposal_files(&repo_root(&specs));
    issues.extend(validate_proposal_hygiene(&proposals));
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
    eprintln!(
        "    - proposals: {} docs/proposals/PROP-*.md — Status header, tracking-issue link, Concerns resolved before Approved, Approved names an ADR",
        proposals.len()
    );

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
    let artifacts: [(&str, String); 9] = [
        // The CI env-sync manifest (PROP-20260729-014500): which repo secret supplies which service
        // env key, per profile. Baked values are NOT here — they ride the image (D5).
        ("render-config-sync.json", emit_render_sync_manifest(&model)),
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
        ("states.rs", emit_domain_states(&model)),
        ("mod.rs", "// GENERATED module index — do not edit by hand.\npub mod scalars;\npub mod entities;\npub mod events;\npub mod commands;\npub mod errors;\npub mod lifecycles;\npub mod states;\n".to_string()),
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
        ("reminders.rs", emit_app_reminders(&model)),
        ("behaviour_tests.rs", emit_behaviour_tests(&model)),
        ("mod.rs", "// GENERATED module index — do not edit by hand.\npub mod rows;\npub mod projectors;\npub mod pm_state;\npub mod process_managers;\npub mod services;\npub mod handlers;\npub mod reminders;\n#[cfg(test)]\npub mod behaviour_tests;\n".to_string()),
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
    // The deletion-engine parameter table exists only once an actor declares `deletion:`
    // (ADR-20260731-214500) — absent, neither the file nor its mod.rs line is emitted (zero drift).
    let deletion_policy = emit_infra_deletion_policy(&model);
    let infra_mod = format!(
        "// GENERATED module index — do not edit by hand.\npub mod pm_state;\npub mod service_clients;\npub mod service_bindings;\npub mod command_router;\n{}",
        if deletion_policy.is_some() { "pub mod deletion_policy;\n" } else { "" }
    );
    let mut infra_files: Vec<(&str, String)> = vec![
        ("pm_state.rs", emit_pm_state_infrastructure(&model)),
        ("service_clients.rs", emit_services_http_clients(&model)),
        ("service_bindings.rs", emit_service_bindings(&model)),
        ("command_router.rs", emit_infra_command_router(&model)),
        ("mod.rs", infra_mod),
    ];
    if let Some(dp) = deletion_policy {
        infra_files.push(("deletion_policy.rs", dp));
    }
    for (name, content) in infra_files {
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
        ("config.rs", emit_config(&model)),
        ("mod.rs", "// GENERATED module index — do not edit by hand.\npub mod config;\npub mod services_routes;\n".to_string()),
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
        ("data_layer.rs", emit_web_data_layer(&model)),
        ("screens.rs", emit_web_screens(&model)),
        (
            "mod.rs",
            "// GENERATED module index — do not edit by hand.\npub mod data_layer;\npub mod registry;\npub mod screens;\n".to_string(),
        ),
    ] {
        let path = web_gen.join(name);
        if let Err(e) = fs::write(&path, content) {
            eprintln!("✗ write {}: {}", path.display(), e);
            std::process::exit(1);
        }
        eprintln!("✓ wrote {}", path.display());
    }

    // The design tokens → CSS custom properties (#115): the DSL palette becomes `:root { --… }` so
    // the hand-written base stylesheet (crates/web/assets/app.css) can consume `var(--color-primary)`
    // etc. Written under crates/web/assets so the renderer `include_str!`s it; drift-gated.
    let assets = repo_root(&specs).join("crates/web/assets");
    if let Err(e) = fs::create_dir_all(&assets) {
        eprintln!("✗ create {}: {}", assets.display(), e);
        std::process::exit(1);
    }
    let css_path = assets.join("tokens.generated.css");
    if let Err(e) = fs::write(&css_path, emit_web_tokens_css(&model)) {
        eprintln!("✗ write {}: {}", css_path.display(), e);
        std::process::exit(1);
    }
    eprintln!("✓ wrote {}", css_path.display());
}

#[cfg(test)]
mod tests;

mod emit;
pub(crate) use emit::*;
mod validate;
pub(crate) use validate::*;
mod config;
pub(crate) use config::*;
