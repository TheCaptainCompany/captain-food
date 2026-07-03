//! Captain.Food codegen — Rust port (ADR-0034), stage 1.
//!
//! Faithful re-implementation of `tools/codegen` (TypeScript), built incrementally and verified by CI
//! (no local Rust toolchain yet). This stage covers the foundation: loading every `specs/**` DSL file and
//! validating referential integrity — every `$ref` anywhere must parse and resolve (mirrors validate.ts
//! §1). Later stages port the remaining gates (actor wiring, api↔model, views, stories, tests, rules,
//! translations, screens, observability, C4) and the emitters. Until parity, the TypeScript codegen stays
//! the blocking gate (see ADR-0034).

use std::collections::{BTreeMap, HashMap, HashSet};
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
    "views.yaml",
    "api.yaml",
    "stories.yaml",
    "rules.yaml",
    "tests.yaml",
    "translations.yaml",
    "customer_screens.yaml",
    "observability.yaml",
    "architecture/c4-l2.yaml",
    "architecture/c4-l3.yaml",
];

/// The loaded model: each source file parsed into its YAML `Value` (the full top-level mapping).
struct Model {
    defs: BTreeMap<String, Value>,
}

fn load_model(specs: &PathBuf) -> Result<Model, String> {
    let mut defs = BTreeMap::new();
    for &f in SOURCE_FILES {
        let p = specs.join(f);
        let s = fs::read_to_string(&p).map_err(|e| format!("read {}: {}", p.display(), e))?;
        let parsed: Value = serde_yaml::from_str(&s).map_err(|e| format!("parse {}: {}", f, e))?;
        // Strip file-level meta (version/description) exactly like load.ts META_KEYS, preserving key order,
        // so scalar/enum/type iteration matches the TypeScript codegen.
        let v = match parsed {
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
        };
        defs.insert(f.to_string(), v);
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
                    collect_refs(val, &format!("{}/{}", loc, key), out);
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

/// §1 — referential integrity: every `$ref` must parse and resolve. Returns (errors, refs_checked).
fn validate(model: &Model) -> (Vec<String>, usize) {
    let mut errors = Vec::new();
    let mut checked = 0usize;
    for &f in SOURCE_FILES {
        if let Some(v) = model.defs.get(f) {
            let mut refs = Vec::new();
            collect_refs(v, f, &mut refs);
            for (loc, r) in refs {
                checked += 1;
                if parse_ref(&r).is_none() {
                    errors.push(format!("[ref-format]   {}: malformed $ref '{}'", loc, r));
                } else if resolve_ref(model, &r, f).is_none() {
                    errors.push(format!("[ref-dangling] {}: $ref '{}' does not resolve", loc, r));
                }
            }
        }
    }
    (errors, checked)
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
    }
}

fn parse_views(model: &Model) -> Vec<SqlView> {
    let mut out = Vec::new();
    let events = model.defs.get("events.yaml").cloned().unwrap_or(Value::Null);
    if let Some(Value::Mapping(m)) = model.defs.get("views.yaml") {
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
            let aggregate = node.get("aggregate").and_then(|x| x.as_str()).unwrap_or("").to_string();
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
            });
        }
    }
    out
}

fn emit_views_sql(model: &Model) -> String {
    let mut blocks = Vec::new();
    for v in parse_views(model) {
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
        blocks.push(if idx.is_empty() { ddl } else { format!("{}\n{}", ddl, idx.join("\n")) });
    }
    format!(
        "-- GENERATED by tools/codegen from specs/views.yaml — do not edit by hand.\n-- Read tables (View_*): denormalized, query-shaped, rebuildable from domain_events.\n\n{}\n",
        blocks.join("\n\n")
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
                description: node.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()),
                receives,
            });
        }
    }
    out
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

/// (view name, fedBy event names) for every View_* (aggregate-fed or reference), in file order.
fn views_fedby(model: &Model) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    if let Some(Value::Mapping(m)) = model.defs.get("views.yaml") {
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            let is_ref = node.get("source").and_then(|x| x.as_str()) == Some("reference");
            let has_agg = node.get("aggregate").and_then(|x| x.as_str()).is_some();
            if !has_agg && !is_ref {
                continue;
            }
            out.push((name.to_string(), ref_names(node.get("fedBy"))));
        }
    }
    out
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

    // 3) Saga sequence diagrams.
    let mut saga_blocks: Vec<String> = Vec::new();
    for a in actors.iter().filter(|a| a.kind == "process-manager") {
        let mut sl: Vec<String> = vec![
            "sequenceDiagram".into(),
            "  autonumber".into(),
            "  participant C as Caller / inbound".into(),
            format!("  participant P as {}", a.name),
            "  participant S as Event store".into(),
        ];
        for e in &a.receives {
            let msg = ref_name(&e.message_ref).unwrap_or_else(|| "?".to_string());
            let kind = if e.message_ref.starts_with("commands.yaml#/") { "command" } else { "event" };
            sl.push(format!("  C->>P: {} ({})", msg, kind));
            let emits: Vec<String> = e.emits.iter().filter_map(|r| ref_name(r)).collect();
            if !emits.is_empty() {
                for ev in &emits {
                    sl.push(format!("  P->>S: {}", ev));
                }
            } else {
                let effect = e.effect.clone().unwrap_or_else(|| "no event emitted".to_string());
                let cleaned: String = effect.replace('\n', " ").replace(':', " ").replace(';', " ");
                let clipped: String = cleaned.chars().take(60).collect();
                sl.push(format!("  Note over P: {}", clipped));
            }
        }
        for line in [format!("### {}", a.name), String::new(), "```mermaid".into(), sl.join("\n"), "```".into(), String::new()] {
            saga_blocks.push(line);
        }
    }

    let mut out: Vec<String> = vec![
        "<!-- GENERATED by tools/codegen — do not edit by hand. Source: specs/architecture/c4-*.yaml. -->".into(),
        "# Captain.Food — C4 diagrams (Mermaid, generated)".into(),
        "".into(),
        "Rendered by any Mermaid-aware viewer (GitHub, VS Code, mermaid.live). The authoritative source is".into(),
        "`specs/architecture/c4-l2.yaml` / `c4-l3.yaml`; regenerate with `npm run generate`.".into(),
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

fn nav_add(
    entity: &str,
    field: &str,
    line: &str,
    entity_names: &HashSet<String>,
    seen: &mut HashMap<String, HashSet<String>>,
    out: &mut HashMap<String, Vec<String>>,
) {
    if !entity_names.contains(entity) {
        return;
    }
    let s = seen.entry(entity.to_string()).or_default();
    if s.contains(field) {
        return;
    }
    s.insert(field.to_string());
    out.entry(entity.to_string()).or_default().push(format!("  {}", line));
}

fn nav_by_entity(views: &[SqlView], entity_names: &HashSet<String>) -> HashMap<String, Vec<String>> {
    let view_agg: HashMap<String, String> = views.iter().map(|v| (v.name.clone(), v.aggregate.clone())).collect();
    let mut seen: HashMap<String, HashSet<String>> = HashMap::new();
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
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
                nav_add(&src, &camel(&tgt), &format!("{}: {}{}", camel(&tgt), tgt, if col.nullable { "" } else { "!" }), entity_names, &mut seen, &mut out);
                nav_add(&tgt, &format!("{}s", camel(&src)), &format!("{}s: [{}!]!", camel(&src), src), entity_names, &mut seen, &mut out);
            }
        }
    }
    out
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

    // actorDocs
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
        Doc { ctx: cx.of_actor(&a.name), md: vec![
            item_head("actor", "Actor", &a.name),
            format!("\n_{}_{}\n", kind, a.description.as_deref().map(|d| format!(" — {}", d)).unwrap_or_default()),
            md_table(&["Receives", "Emits →", "Throws"], &rows),
        ].join("\n") }
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
    let non_projected: HashSet<String> = ref_names(model.defs.get("views.yaml").and_then(|v| v.get("nonProjectedEvents"))).into_iter().collect();
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
        let kind = match file { "commands.yaml" => "command", "events.yaml" => "event", "actors.yaml" => "actor", "views.yaml" => "view", "scalars.yaml" => "scalar", _ => "entity" };
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
    let sf = model.defs.get("customer_screens.yaml");
    let resolvers = sf.and_then(|v| v.get("resolvers")).and_then(|v| v.as_mapping());
    let action_defs = sf.and_then(|v| v.get("actions")).and_then(|v| v.as_mapping());
    let tr_defs = model.defs.get("translations.yaml").and_then(|v| v.as_mapping());
    let cellf = |s: &str| s.replace('|', "\\|");
    let tr_en = |rf: &str| -> String { resolve_ref(model, rf, "customer_screens.yaml").and_then(|t| t.get("messages")).and_then(|m| m.get("en")).and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| rf.rsplit('/').next().unwrap_or(rf).to_string()) };
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

    let (errors, checked) = validate(&model);
    eprintln!(
        "• rust-codegen (stage 1): {} source files loaded, {} $refs checked",
        SOURCE_FILES.len(),
        checked
    );

    if !errors.is_empty() {
        for e in &errors {
            eprintln!("{}", e);
        }
        eprintln!("\n✗ {} referential-integrity error(s).", errors.len());
        std::process::exit(1);
    }
    eprintln!("✓ all {} $refs resolve (Rust codegen — referential integrity).", checked);

    if check {
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
    let artifacts: [(&str, String); 6] = [
        ("translations.generated.json", emit_translations_json(&model)),
        ("views.generated.sql", emit_views_sql(&model)),
        ("c4.generated.dsl", emit_structurizr(&model)),
        ("c4.generated.md", emit_mermaid(&model)),
        ("schema.generated.graphql", emit_schema(&model)),
        ("documentation.generated.md", emit_documentation(&model)),
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
    eprintln!("(remaining emitters still produced by the TypeScript codegen until parity — ADR-0034)");
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
        assert!(!is_source_file("nope.yaml"));
    }
}
