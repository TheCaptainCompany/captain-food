use crate::*;

pub(crate) const INLINE_TYPES: [&str; 4] = ["string", "boolean", "integer", "float"];

/// checkRoles: `roles:` is a LITERAL list (ADR-20260720-191500) — omitted means open to every role
/// path (→ @public), present means exactly those paths (→ @auth, PUBLIC = the anonymous path). Each
/// listed role must be a scalars.yaml#/UserType value.
pub(crate) fn check_roles(issues: &mut Vec<Issue>, roles: &[String], where_: &str, uts: &BTreeSet<String>) {
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
pub(crate) fn check_inline(issues: &mut Vec<Issue>, f: &ApiField, where_: &str) {
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
pub(crate) fn check_shape(model: &Model, issues: &mut Vec<Issue>, node: Option<&Value>, data: Option<&Value>, where_: &str) {
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
pub(crate) fn fixture_event(model: &Model, fx_ref: Option<&str>) -> Option<String> {
    let fx = resolve_ref(model, fx_ref?, "tests.yaml")?;
    ref_name(fx.get("type")?.get("$ref")?.as_str()?)
}

/// `{param}` placeholder names in a string (mirrors `/\{(\w+)\}/g`, `\w` = ASCII alnum + `_`).
pub(crate) fn placeholders(v: Option<&Value>) -> BTreeSet<String> {
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

pub(crate) fn map_keys(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_mapping())
        .map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

