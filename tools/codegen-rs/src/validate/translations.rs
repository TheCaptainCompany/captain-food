use crate::*;

/// All translation ENTRIES merged from every source (ADR-20260722-101500): the shared `translations.yaml`
/// plus every per-surface `screens/*.translations.yaml` sidecar. Returns `(fileKey, entryKey, node)` per
/// real entry (skips file-level meta — only nodes carrying `messages`), file-sorted then file-order, so
/// output is deterministic. Keys must be unique across files (the §10 validator enforces it).
/// Translation hygiene (#110, PROP-20260724-133700 §1c) — the §10/§10b rules, standalone so tests can
/// run them on a minimal fixture. Emits: `translation-duplicate-key`, `translation-locale-missing`
/// (full SUPPORTED_LOCALES coverage), `translation-param-mismatch`, `translation-key-unused` (a key no
/// screen and no `code_refs` entry references), and `translation-code-ref-unknown` (a stale manifest
/// entry). Does not touch coverage — the caller counts entries.
pub(crate) fn validate_translations(model: &Model, issues: &mut Vec<Issue>) {
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

