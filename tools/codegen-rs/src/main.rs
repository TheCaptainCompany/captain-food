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
mod c4;
pub(crate) use c4::*;
mod api;
pub(crate) use api::*;
