use crate::*;

/// The DSL source files, in load order (mirrors model.ts `SOURCE_FILES`).
pub(crate) const SOURCE_FILES: &[&str] = &[
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
pub(crate) const SUPPORTED_LOCALES: &[&str] = &["en", "fr"];

// ─── Per-scope spec folders (ADR-20260807-183024 D1) ────────────────────────────────────────────
//
// `$ref`s are KIND-logical: `commands.yaml#/X` names a KIND (the ref path encodes kind — see
// REF_CONTRACT in refs.rs), not a file location. The loader therefore merges every
// `specs/{scope}/{kind}.yaml` fragment into the SAME logical catalog key (`commands.yaml`, …),
// recording each item's ORIGIN scope, so no ref ever needs rewriting when an item moves folders.

/// Catalog kinds whose fragments are flat `name → definition` mappings, mergeable per item.
pub(crate) const SCOPED_FLAT_KINDS: &[&str] = &[
    "scalars.yaml",
    "entities.yaml",
    "events.yaml",
    "commands.yaml",
    "errors.yaml",
    "actors.yaml",
    "processmanager.yaml",
    "rules.yaml",
];

/// Catalog kinds structured as top-level SECTIONS (each section a `name → definition` mapping);
/// fragments merge per section entry. Origin keys are `"{section}/{name}"`.
pub(crate) const SCOPED_SECTION_KINDS: &[(&str, &[&str])] = &[
    ("api.yaml", &["types", "inputs", "queries", "mutations", "subscriptions"]),
    ("configuration.yaml", &["keys"]),
];

/// `specs/` subdirectories that are NOT scope folders (structural homes with their own loaders).
pub(crate) const NON_SCOPE_DIRS: &[&str] =
    &["architecture", "database", "screens", "generated", "integrations"];

/// The loaded model: each source file parsed into its YAML `Value` (the full top-level mapping).
/// Per-scope fragments (`specs/{scope}/{kind}.yaml`) are merged into the LOGICAL catalog keys, with
/// `origins` recording each merged item's scope (items from a flat root catalog have no entry).
#[derive(Default)]
pub(crate) struct Model {
    pub(crate) defs: BTreeMap<String, Value>,
    /// `(logical file, item key)` → origin scope folder. For section kinds the item key is
    /// `"{section}/{name}"` (e.g. `("api.yaml", "queries/restaurant")`).
    pub(crate) origins: BTreeMap<(String, String), String>,
    /// Scope folders discovered under `specs/` (sorted). A directory is a scope iff it is not one
    /// of `NON_SCOPE_DIRS` and holds at least one scoped kind file.
    pub(crate) scopes: Vec<String>,
    /// Issues raised while loading (duplicate item names across fragment files). Surfaced by
    /// `validate()` so they gate like any other issue.
    pub(crate) load_issues: Vec<Issue>,
    /// The adapter-crate directory names under `crates/adapters/` (sorted), scanned at load time.
    /// THE derivation source of the `adapter-*` bin family (ADR-20260808-062432 — one bin per
    /// partner ACL, never a hand list): a sixth adapter crate produces a sixth bin the day it
    /// exists, and §15 then requires its c4-l2 container. Empty on fixture models (`Default`),
    /// which keeps the degenerate-model guard intact.
    pub(crate) adapter_crates: Vec<String>,
}

/// Strip file-level meta (version/description) like load.ts META_KEYS, preserving key order.
pub(crate) fn strip_meta(parsed: Value) -> Value {
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

/// Is this logical file a splittable catalog kind (flat or section-structured)?
pub(crate) fn scoped_kind(f: &str) -> bool {
    SCOPED_FLAT_KINDS.contains(&f) || SCOPED_SECTION_KINDS.iter().any(|(k, _)| *k == f)
}

pub(crate) fn load_model(specs: &PathBuf) -> Result<Model, String> {
    let mut defs = BTreeMap::new();
    let mut origins: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut load_issues: Vec<Issue> = Vec::new();
    fn read_spec(key: &str, p: &std::path::Path) -> Result<Value, String> {
        let s = fs::read_to_string(p).map_err(|e| format!("read {}: {}", p.display(), e))?;
        let parsed: Value = serde_yaml::from_str(&s).map_err(|e| format!("parse {}: {}", key, e))?;
        Ok(strip_meta(parsed))
    }
    let load = |defs: &mut BTreeMap<String, Value>, key: String, p: &std::path::Path| -> Result<(), String> {
        let v = read_spec(&key, p)?;
        defs.insert(key, v);
        Ok(())
    };
    let mut root_scoped_files: Vec<&str> = Vec::new();
    for &f in SOURCE_FILES {
        let p = specs.join(f);
        // Splittable catalog kinds may live ENTIRELY in per-scope folders (ADR-20260807-183024 D1):
        // a missing root file starts the logical catalog empty and the fragments fill it.
        if scoped_kind(f) {
            if !p.exists() {
                defs.insert(f.to_string(), Value::Mapping(serde_yaml::Mapping::new()));
                continue;
            }
            root_scoped_files.push(f);
        }
        load(&mut defs, f.to_string(), &p)?;
    }
    // ── Per-scope fragments: specs/{scope}/{kind}.yaml merged into the logical catalogs ──────────
    let mut scopes: Vec<String> = Vec::new();
    if let Ok(rd) = fs::read_dir(specs) {
        let mut dirs: Vec<String> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
            .filter(|n| !NON_SCOPE_DIRS.contains(&n.as_str()))
            .collect();
        dirs.sort();
        for scope in dirs {
            let sdir = specs.join(&scope);
            let kind_files: Vec<&str> = SCOPED_FLAT_KINDS
                .iter()
                .copied()
                .chain(SCOPED_SECTION_KINDS.iter().map(|(k, _)| *k))
                .collect();
            let mut has_any = false;
            for kind in kind_files {
                let fp = sdir.join(kind);
                if !fp.exists() {
                    continue;
                }
                has_any = true;
                let s = fs::read_to_string(&fp).map_err(|e| format!("read {}: {}", fp.display(), e))?;
                let parsed: Value = serde_yaml::from_str(&s)
                    .map_err(|e| format!("parse {}/{}: {}", scope, kind, e))?;
                let fragment = strip_meta(parsed);
                let Value::Mapping(frag) = fragment else {
                    return Err(format!("{}/{}: fragment must be a YAML mapping", scope, kind));
                };
                let target = defs
                    .entry(kind.to_string())
                    .or_insert_with(|| Value::Mapping(serde_yaml::Mapping::new()));
                let Value::Mapping(tm) = target else {
                    return Err(format!("{}: logical catalog is not a mapping", kind));
                };
                let sections = SCOPED_SECTION_KINDS
                    .iter()
                    .find(|(k, _)| *k == kind)
                    .map(|(_, secs)| *secs);
                match sections {
                    None => {
                        // Flat kind: every top-level key is one item.
                        for (k, v) in frag {
                            let name = k.as_str().unwrap_or("?").to_string();
                            if tm.contains_key(&k) {
                                load_issues.push(err(
                                    "scope-duplicate-item",
                                    format!("{}/{}", scope, kind),
                                    format!(
                                        "'{}' is already defined in another spec file mapping to {} — one name, one definition (first definition kept).",
                                        name, kind
                                    ),
                                ));
                                continue;
                            }
                            origins.insert((kind.to_string(), name), scope.clone());
                            tm.insert(k, v);
                        }
                    }
                    Some(secs) => {
                        // Section kind: merge each declared section's entries.
                        for (sk, sv) in frag {
                            let sec = sk.as_str().unwrap_or("?").to_string();
                            if !secs.contains(&sec.as_str()) {
                                load_issues.push(err(
                                    "scope-unknown-section",
                                    format!("{}/{}", scope, kind),
                                    format!("unknown section '{}' — expected one of {:?}.", sec, secs),
                                ));
                                continue;
                            }
                            let Value::Mapping(sm) = sv else {
                                return Err(format!("{}/{}: section '{}' must be a mapping", scope, kind, sec));
                            };
                            let tsec = tm
                                .entry(Value::String(sec.clone()))
                                .or_insert_with(|| Value::Mapping(serde_yaml::Mapping::new()));
                            let Value::Mapping(tsm) = tsec else {
                                return Err(format!("{}: section '{}' is not a mapping", kind, sec));
                            };
                            for (k, v) in sm {
                                let name = k.as_str().unwrap_or("?").to_string();
                                if tsm.contains_key(&k) {
                                    load_issues.push(err(
                                        "scope-duplicate-item",
                                        format!("{}/{}", scope, kind),
                                        format!(
                                            "'{}/{}' is already defined in another spec file mapping to {} — one name, one definition (first definition kept).",
                                            sec, name, kind
                                        ),
                                    ));
                                    continue;
                                }
                                origins.insert((kind.to_string(), format!("{}/{}", sec, name)), scope.clone());
                                tsm.insert(k, v);
                            }
                        }
                    }
                }
            }
            if has_any {
                scopes.push(scope);
            }
        }
    }
    // Once scope folders exist, a ROOT splittable catalog is forbidden: its items would carry no
    // origin and silently bypass every scope gate (placement, DAG, purity) — a hole an agent adding
    // "just one command" to a recreated flat file would walk straight through.
    if !scopes.is_empty() {
        for f in root_scoped_files {
            load_issues.push(err(
                "scope-root-catalog-forbidden",
                f.to_string(),
                format!(
                    "flat root catalog specs/{} coexists with per-scope folders — its items have no origin scope and bypass the scope gates; move them into specs/{{scope}}/{}.",
                    f, f
                ),
            ));
        }
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
            load(&mut defs, format!("database/tables/{}", name), &p)?;
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
                load(&mut defs, name, &p)?; // sidecar — keyed bare
            } else {
                load(&mut defs, format!("screens/{}", name), &p)?; // screen spec — keyed with `screens/` prefix
            }
        }
    }
    // The adapter-crate list (ADR-20260808-062432): every `crates/adapters/<dir>` holding a
    // Cargo.toml, relative to the repo root the specs live in. This is a WORKSPACE fact, not a
    // spec fact — deliberately scanned here so `bin_topology` stays a pure function of the Model
    // and the §15 validator can prove the c4-l2 container list against it in both directions.
    let mut adapter_crates: Vec<String> = Vec::new();
    if let Some(root) = specs.parent() {
        if let Ok(rd) = fs::read_dir(root.join("crates/adapters")) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() && p.join("Cargo.toml").is_file() {
                    if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                        adapter_crates.push(name.to_string());
                    }
                }
            }
        }
    }
    adapter_crates.sort();
    Ok(Model { defs, origins, scopes, load_issues, adapter_crates })
}

/// A parsed `<file>#/<a>/<b>` reference. `file` is empty for a local `#/…` ref (resolved against context).
pub(crate) struct ParsedRef {
    pub(crate) file: String,
    pub(crate) path: Vec<String>,
}

/// Mirrors refs.ts `parseRef`: split on the first `#/`; the pointer is split on `/` (dotted keys such as
/// translation keys `home.title` stay a single segment — they contain no `/`).
pub(crate) fn parse_ref(r: &str) -> Option<ParsedRef> {
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

pub(crate) fn is_source_file(f: &str) -> bool {
    SOURCE_FILES.contains(&f)
        || (f.starts_with("database/tables/") && f.ends_with(".yaml"))
        // Auto-discovered SDUI screen specs (ADR-20260722-091500), keyed `screens/<surface>.yaml`.
        || (f.starts_with("screens/") && f.ends_with(".yaml"))
        // Per-surface i18n sidecars (ADR-20260722-101500), keyed BARE (`<surface>.translations.yaml`).
        || f.ends_with(".translations.yaml")
}

/// The bare definition name a `$ref` denotes: the FIRST pointer segment (mirrors refs.ts `refName`).
pub(crate) fn ref_name(r: &str) -> Option<String> {
    parse_ref(r).and_then(|p| p.path.into_iter().next())
}

/// Mirrors refs.ts `resolveRef`: resolve `ref` (appearing in `ctx`) into the target file's Value tree.
pub(crate) fn resolve_ref<'a>(model: &'a Model, r: &str, ctx: &str) -> Option<&'a Value> {
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
pub(crate) fn collect_refs(v: &Value, loc: &str, out: &mut Vec<(String, String)>) {
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
pub(crate) enum Level {
    Error,
    Warning,
}

#[derive(Clone)]
pub(crate) struct Issue {
    pub(crate) level: Level,
    pub(crate) rule: &'static str,
    pub(crate) location: String,
    pub(crate) message: String,
}

pub(crate) fn err(rule: &'static str, location: String, message: String) -> Issue {
    Issue { level: Level::Error, rule, location, message }
}
pub(crate) fn warn(rule: &'static str, location: String, message: String) -> Issue {
    Issue { level: Level::Warning, rule, location, message }
}

/// Count of what was actually checked — so a clean run shows coverage, not just silence (Coverage in TS).
#[derive(Default)]
pub(crate) struct Coverage {
    pub(crate) refs: usize,
    pub(crate) views: usize,
    pub(crate) view_columns: usize,
    pub(crate) view_fed_by: usize,
    pub(crate) mutation_links: usize,
    pub(crate) reads_links: usize,
    pub(crate) component_reads_links: usize,
    pub(crate) story_links: usize,
    pub(crate) test_cases: usize,
    pub(crate) rules: usize,
    pub(crate) obs_contracts: usize,
    pub(crate) translations: usize,
    pub(crate) screens: usize,
    pub(crate) screen_bindings: usize,
    pub(crate) screen_gaps: usize,
    pub(crate) lifecycles: usize,
    pub(crate) lifecycle_transitions: usize,
}

pub(crate) struct Report {
    pub(crate) issues: Vec<Issue>,
    pub(crate) coverage: Coverage,
    /// Commands actually handled by some actor (the cli's "commands" count; ≤ total command defs, the
    /// difference being command value objects referenced only from `properties`).
    pub(crate) handled_commands: usize,
}

