use crate::*;

/// One declared configuration key (`specs/configuration.yaml`).
#[derive(Debug, Clone)]
pub(crate) struct ConfigKey {
    pub(crate) name: String,
    pub(crate) ty: String,
    pub(crate) values: Vec<String>,
    pub(crate) required: Vec<String>,
    pub(crate) default: Option<String>,
    pub(crate) secret: bool,
    pub(crate) gates: String,
    pub(crate) mode_of: Option<String>,
    /// Which binary reads it. `server` (default) lands in the server's typed Config; other consumers
    /// (e.g. the CI `sirene_ingest` job) are DECLARED — so the drift gate and the docs cover them —
    /// without being injected into a process that never reads them.
    pub(crate) consumer: String,
    /// The scalars.yaml type this value must satisfy, and its regex — validated at STARTUP, because
    /// "present" is not "usable" (a live key in the test slot, a 31-byte session key, a pasted newline).
    pub(crate) scalar: Option<String>,
    pub(crate) pattern: Option<String>,
    /// Non-secret per-profile values, BAKED into the binary (PROP-20260729-014500 D5): profile -> value.
    pub(crate) deploy: BTreeMap<String, String>,
    /// Secret sources, SYNCED by CI from GitHub repo secrets: profile -> repo-secret name. Never baked
    /// — the GHCR package is public, so a baked ENV would be world-readable.
    pub(crate) from_secret: BTreeMap<String, String>,
}

/// The profiles a key may be required in — also the `APP_PROFILE` enum.
pub(crate) const CONFIG_PROFILES: &[&str] = &["development", "staging", "production"];

/// Parse `configuration.yaml` into the declared key list, in DECLARATION ORDER (the emitted report
/// follows it, so the spec's grouping — persistence, identity, payments, toggles — survives into the
/// operator-facing output).
pub(crate) fn parse_config_keys(model: &Model) -> Vec<ConfigKey> {
    let Some(doc) = model.defs.get("configuration.yaml") else { return Vec::new() };
    let Some(keys) = doc.get("keys").and_then(|k| k.as_mapping()) else { return Vec::new() };
    let mut out = Vec::new();
    for (name, node) in keys {
        let Some(name) = name.as_str() else { continue };
        let str_at = |k: &str| node.get(k).and_then(|v| v.as_str()).map(|s| s.trim().to_string());
        // `default` is typed in YAML (bool/int/string) — render it back to its literal text.
        let scalar_ref = node
            .get("scalar")
            .and_then(|sc| sc.get("$ref"))
            .and_then(|r| r.as_str())
            .and_then(ref_name);
        let default = node.get("default").map(|v| match v {
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            other => format!("{other:?}"),
        });
        out.push(ConfigKey {
            name: name.to_string(),
            ty: str_at("type").unwrap_or_default(),
            values: node
                .get("values")
                .and_then(|v| v.as_sequence())
                .map(|s| s.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
            required: node
                .get("required")
                .and_then(|v| v.as_sequence())
                .map(|s| s.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
            default,
            secret: node.get("secret").and_then(|v| v.as_bool()).unwrap_or(false),
            gates: str_at("gates").unwrap_or_default(),
            mode_of: str_at("mode_of"),
            consumer: str_at("consumer").unwrap_or_else(|| "server".to_string()),
            deploy: node
                .get("deploy")
                .and_then(|d| d.as_mapping())
                .map(|m| {
                    m.iter()
                        .filter(|(k, _)| k.as_str() != Some("from_secret"))
                        .filter_map(|(k, v)| match (k.as_str(), v) {
                            (Some(k), Value::String(v)) => Some((k.to_string(), v.clone())),
                            (Some(k), Value::Bool(v)) => Some((k.to_string(), v.to_string())),
                            (Some(k), Value::Number(v)) => Some((k.to_string(), v.to_string())),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            from_secret: node
                .get("deploy")
                .and_then(|d| d.get("from_secret"))
                .and_then(|f| f.as_mapping())
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| Some((k.as_str()?.to_string(), v.as_str()?.to_string())))
                        .collect()
                })
                .unwrap_or_default(),
            scalar: scalar_ref.clone(),
            pattern: scalar_ref.as_deref().and_then(|name| {
                model
                    .defs
                    .get("scalars.yaml")
                    .and_then(|sc| sc.get(name))
                    .and_then(|sc| sc.get("pattern"))
                    .and_then(|p| p.as_str())
                    .map(str::to_string)
            }),
        });
    }
    out
}

/// §12 — configuration hygiene (PROP-20260729-004500). The rules exist because each one maps to a way
/// the declaration could silently stop being trustworthy, which is the failure this spec file replaces.
pub(crate) fn validate_configuration(model: &Model, issues: &mut Vec<Issue>) {
    let keys = parse_config_keys(model);
    if keys.is_empty() {
        issues.push(err(
            "config-empty",
            "configuration.yaml".to_string(),
            "no configuration keys declared — the app reads env vars, so this cannot be empty".into(),
        ));
        return;
    }
    for k in &keys {
        let at = format!("configuration.yaml/{}", k.name);
        if k.name != k.name.to_uppercase() || k.name.contains('-') {
            issues.push(err(
                "config-key-name",
                at.clone(),
                format!("'{}' must be SCREAMING_SNAKE_CASE (it is an env var name)", k.name),
            ));
        }
        // `gates` is not documentation: it is PRINTED next to the key when startup fails, so an empty
        // one produces a report that names a variable without saying why anyone should care.
        if k.gates.is_empty() {
            issues.push(err(
                "config-gates-missing",
                at.clone(),
                "no `gates`: the fail-fast report prints it, so a key without one is unactionable"
                    .into(),
            ));
        }
        match k.ty.as_str() {
            "bool" | "string" | "int" => {}
            "enum" => {
                if k.values.is_empty() {
                    issues.push(err(
                        "config-enum-values",
                        at.clone(),
                        "type: enum declares no `values`".into(),
                    ));
                }
                if let Some(d) = &k.default {
                    if !k.values.contains(d) {
                        issues.push(err(
                            "config-default-invalid",
                            at.clone(),
                            format!("default '{d}' is not one of {:?}", k.values),
                        ));
                    }
                }
            }
            other => issues.push(err(
                "config-type-unknown",
                at.clone(),
                format!("unknown type '{other}' (bool | string | int | enum)"),
            )),
        }
        if k.ty == "int" {
            if let Some(d) = &k.default {
                if d.parse::<i64>().is_err() {
                    issues.push(err(
                        "config-default-invalid",
                        at.clone(),
                        format!("type: int but default '{d}' is not an integer"),
                    ));
                }
            }
        }
        // A `scalar` that does not resolve, or resolves to something with no `pattern`, is a validation
        // that silently does nothing — the exact shape of bug this whole file exists to prevent.
        if let Some(name) = &k.scalar {
            match model.defs.get("scalars.yaml").and_then(|sc| sc.get(name.as_str())) {
                None => issues.push(err(
                    "config-scalar-unresolved",
                    at.clone(),
                    format!("scalar '{name}' is not declared in scalars.yaml"),
                )),
                Some(_) if k.pattern.is_none() => issues.push(err(
                    "config-scalar-no-pattern",
                    at.clone(),
                    format!(
                        "scalar '{name}' declares no `pattern`, so binding it validates nothing"
                    ),
                )),
                Some(_) => {
                    if let Some(pat) = &k.pattern {
                        if let Err(e) = regex::Regex::new(pat) {
                            issues.push(err(
                                "config-pattern-invalid",
                                at.clone(),
                                format!("scalar '{name}' pattern does not compile: {e}"),
                            ));
                        }
                    }
                }
            }
        }
        // A declared default must itself satisfy the scalar — otherwise the fallback is the bad value.
        if let (Some(pat), Some(d)) = (&k.pattern, &k.default) {
            if let Ok(re) = regex::Regex::new(pat) {
                if !re.is_match(d) {
                    issues.push(err(
                        "config-default-invalid",
                        at.clone(),
                        format!("default '{d}' does not match scalar {:?} ({pat})", k.scalar),
                    ));
                }
            }
        }
        // --- deploy block (PROP-20260729-014500) -------------------------------------------------
        for profile in k.deploy.keys().chain(k.from_secret.keys()) {
            if !CONFIG_PROFILES.contains(&profile.as_str()) {
                issues.push(err(
                    "config-deploy-profile-unknown",
                    at.clone(),
                    format!("deploy declares unknown profile '{profile}' (expected {CONFIG_PROFILES:?})"),
                ));
            }
        }
        // A secret must never be BAKED: the GHCR package is public, so a baked value is world-readable
        // via `docker pull` + `docker history`. This is the rule that keeps D5's split honest.
        if k.secret && !k.deploy.is_empty() {
            issues.push(err(
                "config-secret-baked",
                at.clone(),
                "a secret declares literal `deploy` values — the image is PUBLIC, so those would be                  world-readable. Use `deploy.from_secret` instead".into(),
            ));
        }
        // A non-secret may NOT be sourced from a repo secret (product-owner directive, 2026-07-29:
        // "the non secret keys should not be put in the repo actions secrets"). The point of declaring
        // configuration is that the configuration is VISIBLE: a non-secret hidden in Actions secrets is
        // as opaque as one typed into the dashboard — you still cannot read the repo and know what
        // production runs. Non-secrets carry literal per-profile `deploy` values and are baked.
        if !k.secret && !k.from_secret.is_empty() {
            issues.push(err(
                "config-nonsecret-from-secret",
                at.clone(),
                "non-secret declares `deploy.from_secret`: give it literal per-profile `deploy` values \
                 so the configuration is readable in the repo and baked into the image, or mark it \
                 `secret: true` if it genuinely must not be committed"
                    .into(),
            ));
        }
        // The profile selector cannot be baked: one image is promoted across environments by digest, so
        // the thing that DISTINGUISHES them cannot live inside it (and selecting the per-profile table
        // by a baked profile would be circular).
        if k.name == "APP_PROFILE" && !k.deploy.is_empty() {
            issues.push(err(
                "config-profile-baked",
                at.clone(),
                "APP_PROFILE must come from the service environment — baking it is circular (it is what                  selects the baked table) and one image serves every profile".into(),
            ));
        }
        // A baked value must satisfy its own scalar, or the artifact ships a value the reader rejects.
        if let Some(pat) = &k.pattern {
            if let Ok(re) = regex::Regex::new(pat) {
                for (profile, value) in &k.deploy {
                    if !re.is_match(value) {
                        issues.push(err(
                            "config-deploy-value-invalid",
                            at.clone(),
                            format!(
                                "baked value '{value}' for profile '{profile}' does not match scalar {:?} ({pat})",
                                k.scalar
                            ),
                        ));
                    }
                }
            }
        }
        for p in &k.required {
            if !CONFIG_PROFILES.contains(&p.as_str()) {
                issues.push(err(
                    "config-profile-unknown",
                    at.clone(),
                    format!("required in unknown profile '{p}' (expected one of {CONFIG_PROFILES:?})"),
                ));
            }
        }
        // A required key with a default can never be reported missing — the default satisfies it — so
        // the requirement is silently inert. That is exactly the class of bug this file exists to end.
        if !k.required.is_empty() && k.default.is_some() {
            issues.push(err(
                "config-required-with-default",
                at.clone(),
                "declares both `required` and `default` — the default masks the requirement, so it can never fail".into(),
            ));
        }
        // An optional non-secret with no default resolves to "absent" with nothing to fall back on.
        if k.required.is_empty() && k.default.is_none() && !k.secret && k.ty != "string" {
            issues.push(warn(
                "config-optional-no-default",
                at.clone(),
                "optional and non-secret but declares no default".into(),
            ));
        }
    }
}

/// Emit `crates/server/src/generated/config.rs` — the typed reader for `specs/configuration.yaml`.
///
/// The generated `Config::from_env` collects EVERY missing required key before failing, because an
/// operator who learns about one missing variable per deploy cycle fixes a three-key outage in three
/// deploys. `Display` renders the report the app prints before exiting.
pub(crate) fn emit_config(model: &Model) -> String {
    // Only the server's own keys become its Config; other consumers are declared for the drift gate
    // and the docs, not injected into a process that never reads them.
    let keys: Vec<ConfigKey> =
        parse_config_keys(model).into_iter().filter(|k| k.consumer == "server").collect();
    emit_config_module(model, &keys, /* is_server */ true)
}

/// The server-consumed configuration keys whose ORIGIN SCOPE is in `scopes` — the per-bin Config
/// key set (#374 Q4, ADR-20260807-183024 D5): a bin reads its own scopes' keys plus `common`,
/// exactly the set the deploy emitter routes to its pod. Keys merged from the flat root (no
/// origin) count as `common` — the kernel is where placement-less platform keys live.
pub(crate) fn scoped_config_keys(model: &Model, scopes: &BTreeSet<String>) -> Vec<ConfigKey> {
    scoped_config_keys_with_consumers(model, scopes, &BTreeSet::new())
}

/// [`scoped_config_keys`] widened by a bin's HOSTED CONSUMERS (#393, ADR-20260808-062933): a
/// worker bin that hosts another consumer's pass (worker-sirene-sync hosts `sirene_ingest`)
/// reads that consumer's declared keys too — still scope-filtered, so the widening never
/// reaches past the bin's own scopes. Every other bin passes the empty set and behaves exactly
/// as before.
pub(crate) fn scoped_config_keys_with_consumers(
    model: &Model,
    scopes: &BTreeSet<String>,
    extra_consumers: &BTreeSet<String>,
) -> Vec<ConfigKey> {
    parse_config_keys(model)
        .into_iter()
        .filter(|k| k.consumer == "server" || extra_consumers.contains(&k.consumer))
        .filter(|k| scopes.contains(config_key_origin(model, &k.name)))
        .collect()
}

/// The ORIGIN SCOPE of one configuration key. configuration.yaml is a SECTION kind (`keys:`),
/// so origins key it as `keys/{name}`; a key from a flat root catalog has no origin entry →
/// common (the kernel is where placement-less platform keys live).
pub(crate) fn config_key_origin<'a>(model: &'a Model, key: &str) -> &'a str {
    model
        .origins
        .get(&("configuration.yaml".to_string(), format!("keys/{key}")))
        .or_else(|| model.origins.get(&("configuration.yaml".to_string(), key.to_string())))
        .map(|s| s.as_str())
        .unwrap_or(KERNEL_SCOPE)
}

/// The generated typed reader over an explicit key subset. `is_server` picks the consumer
/// posture: the SERVER asserts every actors.yaml reminder-window key is declared (it hosts every
/// lane, so a missing key is a codegen bug) and every helper is exercised; a BIN filters the
/// windows to its own keys (it hosts a lane subset — the standalone fleet's spec-default
/// fallback covers the honest gap loudly at runtime) and carries a module-level
/// `allow(dead_code)`: which helpers a bin's subset exercises varies per bin (no stripe key ⇒
/// no `stripe_mode` call), and 27 crates of per-bin cfg surgery would buy nothing.
pub(crate) fn emit_config_module(model: &Model, keys: &[ConfigKey], is_server: bool) -> String {
    let strict_windows = is_server;
    let field = |name: &str| name.to_lowercase();
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/configuration.yaml — do not edit by hand.\n\
         //\n\
         // The typed runtime configuration (PROP-20260729-004500, issue #246). Every setting the app\n\
         // needs is DECLARED in the spec and read here, once, at startup — never through a scattered\n\
         // `env::var` call. A missing REQUIRED key (per the resolved `APP_PROFILE`) is collected with\n\
         // every other missing key and reported together; the composition root then stops the app.\n\
         //\n\
         // Secrets are never rendered: the boot report shows `set` / `unset`, and for a key declaring\n\
         // `mode_of: stripe` the derived test/live mode — never the value.\n\n",
    );
    if !is_server {
        out.push_str(
            "// Scope-filtered BIN reader (#374 Q4): which helpers a bin's key subset exercises varies\n\
             // per bin, so the shared reader shape carries a module-level allow instead of per-bin\n\
             // cfg surgery. The reader's behaviour is covered by the server-side config tests.\n\
             #![allow(dead_code)]\n\n",
        );
    }
    out.push_str("use std::fmt;\n\n");

    // ---- Profile -----------------------------------------------------------------------------
    out.push_str(
        "/// Which keys are required. Declared via `APP_PROFILE`, never inferred from the host or from\n\
         /// a key's prefix — an inferred profile is wrong exactly when it matters most (a mode switch).\n\
         #[derive(Debug, Clone, Copy, PartialEq, Eq)]\n\
         pub enum Profile {\n    Development,\n    Staging,\n    Production,\n}\n\n\
         impl Profile {\n    pub fn as_str(self) -> &'static str {\n        match self {\n            Profile::Development => \"development\",\n            Profile::Staging => \"staging\",\n            Profile::Production => \"production\",\n        }\n    }\n}\n\n\
         impl fmt::Display for Profile {\n    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {\n        f.write_str(self.as_str())\n    }\n}\n\n",
    );

    // ---- Value parsing -----------------------------------------------------------------------
    out.push_str(
        "/// Lenient boolean parsing (ADR-20260728-224500): `true/1/yes/on` and `false/0/no/off`,\n\
         /// case-insensitive, trimmed, wrapping quotes stripped. An unrecognised value falls back to the\n\
         /// declared default AND says so — a typo must never be silently interpreted as either state.\n\
         fn parse_bool(name: &str, raw: &str, default: bool) -> bool {\n\
         \x20   match raw.trim().trim_matches(['\"', '\\'']).trim().to_ascii_lowercase().as_str() {\n\
         \x20       \"true\" | \"1\" | \"yes\" | \"on\" => true,\n\
         \x20       \"false\" | \"0\" | \"no\" | \"off\" => false,\n\
         \x20       \"\" => default,\n\
         \x20       other => {\n\
         \x20           println!(\n\
         \x20               \"{name}: unrecognised value {other:?} -- using the default ({default}). \\\n\
         Accepted: true/1/yes/on, false/0/no/off.\"\n\
         \x20           );\n\
         \x20           default\n\
         \x20       }\n\
         \x20   }\n}\n\n\
         /// Read a key, treating an empty/whitespace-only value as ABSENT — a dashboard field someone\n\
         /// cleared must count as missing, not as an empty string that satisfies a requirement.\n\
         fn raw(name: &str) -> Option<String> {\n\
         \x20   std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())\n}\n\n\
         /// Validate a value against its scalar's regex. Compiled once per pattern and cached: startup\n\
         /// cost is a handful of small regexes, paid to make \"present but unusable\" impossible to miss.\n\
         fn matches_pattern(pattern: &str, value: &str) -> bool {\n\
         \x20   use std::collections::HashMap;\n\
         \x20   use std::sync::{Mutex, OnceLock};\n\
         \x20   static CACHE: OnceLock<Mutex<HashMap<String, regex::Regex>>> = OnceLock::new();\n\
         \x20   let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));\n\
         \x20   let mut guard = cache.lock().unwrap_or_else(|p| p.into_inner());\n\
         \x20   let re = guard.entry(pattern.to_string()).or_insert_with(|| {\n\
         \x20       regex::Regex::new(pattern).expect(\"configuration pattern compiles (checked by codegen)\")\n\
         \x20   });\n\
         \x20   re.is_match(value)\n}\n\n\
         /// The non-secret values DECLARED per profile (`deploy:` in configuration.yaml), baked into the\n\
         /// binary (PROP-20260729-014500 D5). Render has no per-deploy env override, so this is the only\n\
         /// way to make configuration part of the ARTIFACT: the digest then determines behaviour, and a\n\
         /// rollback restores the configuration that shipped with that build. Secrets are never here —\n\
         /// the GHCR package is public, so a baked value is world-readable.\n\
         fn baked(name: &str, profile: Profile) -> Option<&'static str> {\n\
         \x20   BAKED\n\
         \x20       .iter()\n\
         \x20       .find(|(k, p, _)| *k == name && *p == profile.as_str())\n\
         \x20       .map(|(_, _, v)| *v)\n}\n\n\
         /// Stripe mode from the key prefix — reportable where the key itself never is.\n\
         fn stripe_mode(value: &str) -> &'static str {\n\
         \x20   if value.starts_with(\"sk_live_\") {\n        \"live\"\n    } else if value.starts_with(\"sk_test_\") {\n        \"test\"\n    } else {\n        \"unknown\"\n    }\n}\n\n",
    );

    // ---- Missing-key report ------------------------------------------------------------------
    out.push_str(
        "/// A required key that was not supplied, with the `gates` text from the spec.\n\
         #[derive(Debug, Clone)]\npub struct MissingKey {\n    pub name: &'static str,\n    pub gates: &'static str,\n}\n\n\
         /// A key that WAS supplied but does not satisfy its scalar. Carries the expected shape, never\n\
         /// the offending value: a malformed secret is still a secret.\n\
         #[derive(Debug, Clone)]\npub struct InvalidKey {\n    pub name: &'static str,\n    pub scalar: &'static str,\n    pub pattern: &'static str,\n    pub gates: &'static str,\n}\n\n\
         /// Everything wrong with the configuration, in one report — never just the first problem. An\n\
         /// operator who learns of one bad key per deploy cycle fixes a three-key outage in three deploys.\n\
         #[derive(Debug, Clone, Default)]\npub struct ConfigProblems {\n    pub missing: Vec<MissingKey>,\n    pub invalid: Vec<InvalidKey>,\n}\n\n\
         impl ConfigProblems {\n    pub fn is_empty(&self) -> bool {\n        self.missing.is_empty() && self.invalid.is_empty()\n    }\n\n    pub fn len(&self) -> usize {\n        self.missing.len() + self.invalid.len()\n    }\n}\n\n\
         /// The report the app prints before it stops (production/staging) or continues (development).\n\
         #[derive(Debug, Clone)]\npub struct MissingConfig {\n    pub profile: Profile,\n    pub problems: ConfigProblems,\n}\n\n\
         impl fmt::Display for MissingConfig {\n\
         \x20   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {\n\
         \x20       writeln!(\n            f,\n            \"FATAL: {} configuration problem(s) (profile: {})\\n\",\n            self.problems.len(),\n            self.profile\n        )?;\n\
         \x20       let width = self\n            .problems\n            .missing\n            .iter()\n            .map(|m| m.name.len())\n            .chain(self.problems.invalid.iter().map(|i| i.name.len()))\n            .max()\n            .unwrap_or(0);\n\
         \x20       if !self.problems.missing.is_empty() {\n            writeln!(f, \"MISSING — required in this profile:\")?;\n            for m in &self.problems.missing {\n                writeln!(f, \"  {:width$}  {}\", m.name, m.gates, width = width)?;\n            }\n            writeln!(f)?;\n        }\n\
         \x20       if !self.problems.invalid.is_empty() {\n            writeln!(f, \"INVALID — supplied but malformed (the value is never printed):\")?;\n            for i in &self.problems.invalid {\n                writeln!(f, \"  {:width$}  expected {} matching {}\", i.name, i.scalar, i.pattern, width = width)?;\n                writeln!(f, \"  {:width$}  {}\", \"\", i.gates, width = width)?;\n            }\n            writeln!(f)?;\n        }\n\
         \x20       write!(f, \"Fix them in the service environment and redeploy. Nothing was started.\")\n\
         \x20   }\n}\n\n",
    );

    // ---- The struct --------------------------------------------------------------------------
    out.push_str("/// The resolved configuration. Built once, injected — never re-read from env.\n#[derive(Debug, Clone)]\npub struct Config {\n    pub profile: Profile,\n");
    for k in keys.iter().filter(|k| k.name != "APP_PROFILE") {
        // "No default declared" means ABSENT, for every type — never a typed zero. An `int` with no
        // default used to resolve to `0`, so `DELIVERY_OFFER_MAX_TTL_SECONDS` would have been reported
        // in the boot log as a TTL of zero seconds while the worker was in fact applying its own 900s
        // fallback. A boot report that states a number the process does not use is worse than one that
        // admits it does not know: the whole point of the report is that an operator can trust it.
        let ty = match k.ty.as_str() {
            "bool" if k.default.is_some() => "bool".to_string(),
            "bool" => "Option<bool>".to_string(),
            "int" if k.default.is_some() => "i64".to_string(),
            "int" => "Option<i64>".to_string(),
            // Optional strings stay Option: "absent" is a state the composition root branches on.
            _ if k.required.is_empty() && k.default.is_none() => "Option<String>".to_string(),
            _ => "String".to_string(),
        };
        out.push_str(&format!("    /// {}\n    pub {}: {},\n", k.gates.replace('\n', " ").trim(), field(&k.name), ty));
    }
    out.push_str("}\n\n");

    // ---- from_env ----------------------------------------------------------------------------
    out.push_str(
        "impl Config {\n\
         \x20   /// Resolve from the process environment, returning the config AND every missing required\n\
         \x20   /// key. Both, always: the warn-only rollout phase (`CONFIG_ENFORCE=false`, D5) has to boot\n\
         \x20   /// with the config it managed to build while still reporting what was absent. A missing\n\
         \x20   /// required string resolves to empty — the caller decides whether that is fatal.\n\
         \x20   pub fn resolve() -> (Self, ConfigProblems) {\n\
         \x20       Self::resolve_inner()\n\
         \x20   }\n\n\
         \x20   /// Strict form: `Err` when anything required is missing. Used by tests and any caller that\n\
         \x20   /// wants no warn-only escape hatch.\n\
         \x20   pub fn from_env() -> Result<Self, MissingConfig> {\n\
         \x20       let (cfg, problems) = Self::resolve_inner();\n\
         \x20       if problems.is_empty() {\n            Ok(cfg)\n        } else {\n            Err(MissingConfig { profile: cfg.profile, problems })\n        }\n\
         \x20   }\n\n\
         \x20   /// Whether a configuration problem must STOP the app. Decided by the PROFILE, not by a\n\
         \x20   /// separate toggle (product-owner directive 2026-07-29): production and staging stop,\n\
         \x20   /// development reports and continues. Stopping is safe on Render — an exiting container\n\
         \x20   /// fails the deploy and the PREVIOUS version keeps serving, so a misconfigured build can\n\
         \x20   /// never replace a working one.\n\
         \x20   pub fn must_stop_on_problems(&self) -> bool {\n        !matches!(self.profile, Profile::Development)\n    }\n\n\
         \x20   fn resolve_inner() -> (Self, ConfigProblems) {\n\
         \x20       let profile = match raw(\"APP_PROFILE\").as_deref() {\n\
         \x20           Some(\"production\") => Profile::Production,\n\
         \x20           Some(\"staging\") => Profile::Staging,\n\
         \x20           Some(\"development\") | None => Profile::Development,\n\
         \x20           Some(other) => {\n\
         \x20               println!(\"APP_PROFILE: unrecognised value {other:?} -- using development.\");\n\
         \x20               Profile::Development\n\
         \x20           }\n\
         \x20       };\n\
         \x20       let mut problems = ConfigProblems::default();\n",
    );
    for k in keys.iter().filter(|k| k.name != "APP_PROFILE") {
        let f = field(&k.name);
        let gates_lit = k.gates.replace('\n', " ").trim().replace('"', "\\\"");
        let required_expr = if k.required.is_empty() {
            "false".to_string()
        } else {
            let arms: Vec<String> = k
                .required
                .iter()
                .map(|p| format!("Profile::{}", {
                    let mut c = p.chars();
                    c.next().map(|x| x.to_uppercase().to_string() + c.as_str()).unwrap_or_default()
                }))
                .collect();
            format!("matches!(profile, {})", arms.join(" | "))
        };
        match k.ty.as_str() {
            "bool" => match &k.default {
                Some(d) => out.push_str(&format!(
                    "        let {f} = raw(\"{n}\")\n            .or_else(|| baked(\"{n}\", profile).map(str::to_string))\n            .map(|v| parse_bool(\"{n}\", &v, {d}))\n            .unwrap_or({d});\n",
                    n = k.name
                )),
                None => out.push_str(&format!(
                    "        let {f} = raw(\"{n}\")\n            .or_else(|| baked(\"{n}\", profile).map(str::to_string))\n            .map(|v| parse_bool(\"{n}\", &v, false));\n",
                    n = k.name
                )),
            },
            "int" => match &k.default {
                Some(d) => out.push_str(&format!(
                    "        let {f} = raw(\"{n}\").and_then(|v| v.parse::<i64>().ok()).unwrap_or({d});\n",
                    n = k.name
                )),
                None => out.push_str(&format!(
                    "        let {f} = raw(\"{n}\").and_then(|v| v.parse::<i64>().ok());\n",
                    n = k.name
                )),
            },
            _ => {
                // PRECEDENCE: env var > baked profile value > default. The env var wins so an operator
                // keeps a seconds-fast override in an incident; the baked value is what runs otherwise.
                if k.deploy.is_empty() {
                    out.push_str(&format!("        let {f} = raw(\"{n}\");\n", n = k.name));
                } else {
                    out.push_str(&format!(
                        "        let {f} = raw(\"{n}\").or_else(|| baked(\"{n}\", profile).map(str::to_string));\n",
                        n = k.name
                    ));
                }
                if !k.required.is_empty() {
                    out.push_str(&format!(
                        "        if {f}.is_none() && {required_expr} {{\n            problems.missing.push(MissingKey {{ name: \"{n}\", gates: \"{g}\" }});\n        }}\n",
                        n = k.name,
                        g = gates_lit
                    ));
                    out.push_str(&format!("        let {f} = {f}.unwrap_or_default();\n"));
                } else if let Some(d) = &k.default {
                    out.push_str(&format!(
                        "        let {f} = {f}.unwrap_or_else(|| \"{d}\".to_string());\n"
                    ));
                }
            }
        }
    }
    // Pattern validation, emitted AFTER every key is read so one pass reports every problem. A bool
    // is validated by its own parser (an unrecognised spelling is reported there, not guessed).
    for k in keys.iter().filter(|k| k.name != "APP_PROFILE") {
        let (Some(pat), Some(scalar)) = (&k.pattern, &k.scalar) else { continue };
        if k.ty == "bool" {
            continue;
        }
        let f = field(&k.name);
        let gates_lit = k.gates.replace('\n', " ").trim().replace('"', "\\\"");
        // Escaped for a NORMAL Rust string literal (see the emission below), NOT a raw one. This was
        // previously emitted into `r"..."`, where escapes are not processed -- so every `\.` in a spec
        // pattern reached the regex engine as `\\.` (a LITERAL backslash followed by any char) and the
        // key could never match its own valid value. `OTEL_TRACES_SAMPLE_RATIO=1.0` was reported INVALID
        // against `^(0(\.[0-9]+)?|1(\.0+)?)$`, which on a production/staging profile REFUSES THE BOOT.
        // Only the `development` profile's "starting anyway" fallback kept it off production's floor.
        let pat_lit = pat.replace('\\', "\\\\").replace('"', "\\\"");
        // Optional keys are Option<String> here; required ones were unwrapped to String above.
        let value_expr = if k.required.is_empty() && k.default.is_none() {
            format!("{f}.as_deref()")
        } else {
            format!("Some({f}.as_str())")
        };
        out.push_str(&format!(
            "        if let Some(v) = {value_expr} {{\n            if !v.is_empty() && !matches_pattern(\"{pat_lit}\", v) {{\n                problems.invalid.push(InvalidKey {{ name: \"{n}\", scalar: \"{scalar}\", pattern: \"{pat_lit}\", gates: \"{g}\" }});\n            }}\n        }}\n",
            n = k.name,
            g = gates_lit
        ));
    }
    out.push_str("        (\n            Self {\n                profile,\n");
    for k in keys.iter().filter(|k| k.name != "APP_PROFILE") {
        out.push_str(&format!("                {},\n", field(&k.name)));
    }
    out.push_str("            },\n            problems,\n        )\n    }\n\n");

    // ---- Reminder / deletion windows (ADR-20260731-214500) ------------------------------------
    // Every configuration key an actors.yaml `reminders.*.after` or `deletion.triggers[*].after`
    // names, resolved to its runtime value — what the composition root hands the mailbox delivery
    // glue and the deletion engine. Generated so a new window key can never silently miss the
    // hand-off (the map grows with the declaration, not with someone remembering).
    {
        let mut window_keys: BTreeSet<String> = BTreeSet::new();
        for r in parse_reminders(model) {
            if let Some(k) = r.after_ref.as_deref().and_then(config_key_ref_name) {
                window_keys.insert(k);
            }
        }
        for d in parse_deletions(model) {
            for t in &d.triggers {
                if let Some(k) = t.after_ref.as_deref().and_then(config_key_ref_name) {
                    window_keys.insert(k);
                }
            }
        }
        out.push_str(
            "    /// The declared reminder/deletion window keys (actors.yaml `after:` refs), each with its\n\
             \x20   /// resolved runtime value in DAYS — handed to the mailbox delivery glue so scheduling\n\
             \x20   /// reads configuration, never a constant (ADR-20260731-214500).\n\
             \x20   pub fn reminder_windows(&self) -> std::collections::HashMap<&'static str, i64> {\n\
             \x20       [\n",
        );
        for k in &window_keys {
            let declared = keys.iter().any(|c| &c.name == k);
            if strict_windows {
                assert!(declared, "config: window key {} named by actors.yaml is not a server-consumed configuration key", k);
            } else if !declared {
                continue;
            }
            out.push_str(&format!("            (\"{}\", self.{}),\n", k, field(k)));
        }
        out.push_str("        ]\n        .into_iter()\n        .collect()\n    }\n\n");
    }

    // ---- Boot report -------------------------------------------------------------------------
    out.push_str(
        "    /// What actually resolved, for the boot log. Secrets render as `set` / `unset`, never as a\n\
         \x20   /// value; a key declaring `mode_of: stripe` additionally reports its derived mode, so\n\
         \x20   /// \"is production actually live?\" is answerable without reading a secret.\n\
         \x20   pub fn boot_report(&self) -> String {\n\
         \x20       let mut out = format!(\"config: profile={}, {} keys resolved\\n\", self.profile, KEY_COUNT);\n",
    );
    for k in keys.iter().filter(|k| k.name != "APP_PROFILE") {
        let f = field(&k.name);
        // Anything with no value to fall back on is an `Option` in the struct — "absent" is a state the
        // composition root branches on — so its report arm must not assume a bare value.
        let numeric = k.ty == "bool" || k.ty == "int";
        let is_option = if numeric {
            k.default.is_none()
        } else {
            k.required.is_empty() && k.default.is_none()
        };
        let line = if k.secret && k.mode_of.as_deref() == Some("stripe") {
            format!(
                "        out.push_str(&format!(\"  {:<26} = {{}}\\n\", if self.{f}.is_empty() {{ \"unset\".to_string() }} else {{ format!(\"set [{{}} mode]\", stripe_mode(&self.{f})) }}));\n",
                k.name
            )
        } else if k.secret && is_option {
            format!(
                "        out.push_str(&format!(\"  {:<26} = {{}}\\n\", if self.{f}.is_some() {{ \"set\" }} else {{ \"unset\" }}));\n",
                k.name
            )
        } else if k.secret {
            format!(
                "        out.push_str(&format!(\"  {:<26} = {{}}\\n\", if self.{f}.is_empty() {{ \"unset\" }} else {{ \"set\" }}));\n",
                k.name
            )
        } else if numeric && is_option {
            format!(
                "        out.push_str(&format!(\"  {:<26} = {{}}\\n\", self.{f}.map_or_else(|| \"unset\".to_string(), |v| v.to_string())));\n",
                k.name
            )
        } else if numeric {
            format!("        out.push_str(&format!(\"  {:<26} = {{}}\\n\", self.{f}));\n", k.name)
        } else if k.required.is_empty() && k.default.is_none() {
            format!(
                "        out.push_str(&format!(\"  {:<26} = {{}}\\n\", self.{f}.as_deref().unwrap_or(\"unset\")));\n",
                k.name
            )
        } else {
            format!("        out.push_str(&format!(\"  {:<26} = {{}}\\n\", self.{f}));\n", k.name)
        };
        out.push_str(&line);
    }
    out.push_str("        out\n    }\n}\n\n");
    out.push_str(&format!(
        "/// How many keys the spec declares (excluding the profile selector).\npub const KEY_COUNT: usize = {};\n\n\
         /// Every declared key name — the drift test asserts each `env::var` call site is one of these.\n\
         pub const DECLARED_KEYS: &[&str] = &[\n",
        keys.iter().filter(|k| k.name != "APP_PROFILE").count()
    ));
    for k in keys {
        out.push_str(&format!("    \"{}\",\n", k.name));
    }
    out.push_str("];\n\n");
    out.push_str(
        "/// `(key, profile, value)` — the declared non-secret configuration, baked in. Reviewed in a PR\n\
         /// and carried by the image digest, so redeploying a digest reproduces its behaviour exactly.\n\
         const BAKED: &[(&str, &str, &str)] = &[\n",
    );
    for k in keys {
        for (profile, value) in &k.deploy {
            out.push_str(&format!(
                "    (\"{}\", \"{}\", \"{}\"),\n",
                k.name,
                profile,
                value.replace('\\', "\\\\").replace('"', "\\\"")
            ));
        }
    }
    out.push_str("];\n");
    out
}

