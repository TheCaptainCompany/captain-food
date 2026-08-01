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
