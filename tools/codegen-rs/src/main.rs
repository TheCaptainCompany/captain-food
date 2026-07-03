//! Captain.Food codegen — Rust port (ADR-0034), stage 1.
//!
//! Faithful re-implementation of `tools/codegen` (TypeScript), built incrementally and verified by CI
//! (no local Rust toolchain yet). This stage covers the foundation: loading every `specs/**` DSL file and
//! validating referential integrity — every `$ref` anywhere must parse and resolve (mirrors validate.ts
//! §1). Later stages port the remaining gates (actor wiring, api↔model, views, stories, tests, rules,
//! translations, screens, observability, C4) and the emitters. Until parity, the TypeScript codegen stays
//! the blocking gate (see ADR-0034).

use std::collections::BTreeMap;
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
        let v: Value = serde_yaml::from_str(&s).map_err(|e| format!("parse {}: {}", f, e))?;
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
    let i18n = out_dir.join("translations.generated.json");
    if let Err(e) = fs::write(&i18n, emit_translations_json(&model)) {
        eprintln!("✗ write {}: {}", i18n.display(), e);
        std::process::exit(1);
    }
    eprintln!("✓ wrote {}", i18n.display());
    eprintln!("(other emitters still produced by the TypeScript codegen until parity — ADR-0034)");
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
