use crate::*;

pub(crate) fn translation_entries(model: &Model) -> Vec<(String, String, &Value)> {
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
pub(crate) fn emit_translations_json(model: &Model) -> String {
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

