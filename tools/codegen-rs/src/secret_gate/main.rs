//! `secret-gate` — the pre-deploy secret-PRESENCE gate (#444, epic #429).
//!
//! WHAT IT CATCHES, AND ONLY WHAT IT CATCHES. A secret the configuration DSL DECLARES as the
//! source for a deployed key — the `from_github_secret` name in `deploy/generated/secret-keys.json`
//! (itself a deterministic fold of `specs/**/configuration.yaml`) — but which the reachable deploy
//! target does NOT hold, or holds BLANK. That is a mis-NAMING or an ABSENCE. It is the failure that
//! wedges a pod on a missing `secretKeyRef` at container start, or makes render-config-sync report
//! "repo secret is not set" against a key an operator believed configured.
//!
//! WHAT IT CANNOT CATCH — stated loudly because a silent blind spot is worse than a named one:
//!   * VALUES. A name present but GARBAGE (a `pk_test_…` where production needs `sk_live_…`, an
//!     expired token, the staging URL) PASSES this gate. It proves a secret is NAMED and NON-EMPTY,
//!     never that its content is correct. Value correctness is not a CI-provable fact.
//!   * The K8s `captain-secrets` sealed store's CONTENT. That Secret is populated OUT OF BAND from a
//!     sealed store (#358), not from GitHub Actions — a name present in the Actions inventory does
//!     not prove it was sealed into the cluster. This gate proves the Actions -> (Render sync /
//!     declared-source) NAMING boundary; the K8s out-of-band store remains a named residual gap.
//!
//! WHY A RUST BINARY, NOT A SHELL HEREDOC. The comparison is a pure function over two maps
//! (declared names -> the env key each backs; present names -> their values). A shell heredoc has no
//! off-CI seam and can never be seen red; this fn is unit-tested in the module below, and the
//! workflow only feeds it JSON and reads the exit code.
//!
//! The codegen already owns `secret-keys.json`'s INTERNAL consistency (it is a deterministic fold of
//! the specs, drift-checked by `check-drift`). This gate deliberately checks ONLY the genuinely
//! EXTERNAL fact — does the deploy target hold a secret named X — and never re-scans that
//! compiler-owned boundary (the #329 trap).

use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

/// One declared secret the deploy target does not satisfy — a FATAL finding.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MissingSecret {
    /// The GitHub repo-secret NAME the config DSL declares as the source (`from_github_secret`).
    /// The gate keys on THIS, never on the env key — a renamed pair (env `STRIPE_SECRET_KEY`
    /// sourced from `STRIPE_SECRET_KEY_PROD`) is only found by looking up the repo-secret name the
    /// deploy target actually holds.
    pub github_secret: String,
    /// The env key(s) this repo secret backs — carried for the operator-facing message only.
    pub env_key: String,
    pub reason: MissingReason,
}

/// Why a declared secret counts as missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum MissingReason {
    /// The deploy target has no secret of this name at all.
    Absent,
    /// The deploy target has the name, mapped to the empty string. Empty == missing: the
    /// "never written empty" doctrine (render-config-sync.yml) — a key that LOOKS configured but is
    /// blank is worse than one plainly absent, so it is fatal exactly as an absence is.
    ///
    /// FIDELITY NOTE (architect, #444). With the `toJSON(secrets)` present-set, this branch is
    /// mostly defensive: an UNSET Actions secret is OMITTED from the object (→ `Absent`, not
    /// `Empty`), and GitHub's UI forbids empty secret VALUES (the REST API requires an
    /// `encrypted_value`), so a secret-side empty is an API-only edge. The `Empty` branch's
    /// guaranteed reach is the declared-side/defensive case and any FUTURE present-set that can hold
    /// blanks (e.g. a kubectl-read cluster secret) — it does not claim to catch a routine
    /// Actions-side empty.
    Empty,
}

/// The result of comparing the declared secrets against the present ones.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct SecretDrift {
    /// Declared-but-unsatisfied (absent OR empty). FATAL — deploy must not proceed.
    pub missing: Vec<MissingSecret>,
    /// Present-but-undeclared. NON-FATAL report: `RENDER_API_KEY`, `GITHUB_TOKEN`,
    /// `RENDER_DEPLOY_HOOK_URL`, `MKS_KUBECONFIG` are legitimately present and declared by no
    /// deployed key — treating these as fatal would false-positive on every real repo.
    pub extra: Vec<String>,
}

impl SecretDrift {
    /// The gate's verdict: only `missing` fails the deploy. `extra` is informational.
    pub fn is_fatal(&self) -> bool {
        !self.missing.is_empty()
    }
}

/// THE pure comparison — the whole reason this is a binary and not a heredoc.
///
/// `declared`: `from_github_secret` NAME -> the env key(s) it backs (value is message-only).
/// `present`:  secret NAME -> its value (empty string = present-but-blank, which is `Empty`).
///
/// Deterministic ordering: both inputs are `BTreeMap`s, so `missing` and `extra` come out sorted by
/// name with no explicit sort — stable output, stable tests.
pub fn compare_secrets(
    declared: &BTreeMap<String, String>,
    present: &BTreeMap<String, String>,
) -> SecretDrift {
    let mut missing = Vec::new();
    for (github_secret, env_key) in declared {
        let reason = match present.get(github_secret) {
            None => Some(MissingReason::Absent),
            Some(v) if v.is_empty() => Some(MissingReason::Empty),
            Some(_) => None,
        };
        if let Some(reason) = reason {
            missing.push(MissingSecret {
                github_secret: github_secret.clone(),
                env_key: env_key.clone(),
                reason,
            });
        }
    }
    let declared_names: BTreeSet<&String> = declared.keys().collect();
    let extra: Vec<String> =
        present.keys().filter(|n| !declared_names.contains(n)).cloned().collect();
    SecretDrift { missing, extra }
}

/// Parse the declared repo-secret map from `deploy/generated/secret-keys.json`, keyed on the
/// `from_github_secret` NAME (never the env key). Value = the env key(s) that name backs, joined
/// when more than one deployed key sources the same repo secret (so no env key is silently
/// dropped on a collision).
pub fn declared_from_secret_keys_json(json: &str) -> Result<BTreeMap<String, String>, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("secret-keys.json: invalid JSON: {e}"))?;
    let keys = v
        .get("keys")
        .and_then(|k| k.as_object())
        .ok_or("secret-keys.json: missing `keys` object")?;
    let mut by_secret: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (env_key, entry) in keys {
        let gh = entry
            .get("from_github_secret")
            .and_then(|s| s.as_str())
            .ok_or_else(|| format!("secret-keys.json: key '{env_key}' has no string from_github_secret"))?;
        by_secret.entry(gh.to_string()).or_default().insert(env_key.clone());
    }
    Ok(by_secret
        .into_iter()
        .map(|(gh, envs)| (gh, envs.into_iter().collect::<Vec<_>>().join(", ")))
        .collect())
}

/// Parse the present-secrets map from a `toJSON(secrets)` object (`{ "NAME": "value", … }`).
/// Non-string values are rejected loudly rather than coerced — the runner only ever emits strings,
/// so anything else means we are not reading what we think we are.
pub fn present_from_secrets_json(json: &str) -> Result<BTreeMap<String, String>, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("present secrets: invalid JSON: {e}"))?;
    let obj = v
        .as_object()
        .ok_or("present secrets: expected a JSON object of name -> value")?;
    let mut out = BTreeMap::new();
    for (name, value) in obj {
        let s = value
            .as_str()
            .ok_or_else(|| format!("present secrets: '{name}' is not a string value"))?;
        out.insert(name.clone(), s.to_string());
    }
    Ok(out)
}

/// The loud value/K8s caveat printed on every run — a blind spot named on screen cannot be
/// mistaken for coverage.
const CAVEAT: &str = "\
NOTE: this gate proves a secret is NAMED and NON-EMPTY in the deploy target — it CANNOT prove the \n\
VALUE is correct (a pk_test where prod needs sk_live PASSES), nor that the K8s captain-secrets \n\
sealed store (populated out of band, #358) holds it. Mis-NAMING and ABSENCE only.";

fn opt(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn read_source(spec: &str) -> Result<String, String> {
    if spec == "-" {
        use std::io::Read;
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| format!("reading stdin: {e}"))?;
        Ok(s)
    } else {
        std::fs::read_to_string(spec).map_err(|e| format!("cannot read {spec}: {e}"))
    }
}

fn run(args: &[String]) -> Result<SecretDrift, String> {
    let declared_path =
        opt(args, "--declared").unwrap_or_else(|| "deploy/generated/secret-keys.json".into());
    let present_spec = opt(args, "--present")
        .ok_or("--present <file|-> is required (a toJSON(secrets) object)")?;
    let declared = declared_from_secret_keys_json(&read_source(&declared_path)?)?;
    let present = present_from_secrets_json(&read_source(&present_spec)?)?;
    Ok(compare_secrets(&declared, &present))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let drift = match run(&args) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("secret-gate: error: {e}");
            return ExitCode::FAILURE;
        }
    };

    if !drift.extra.is_empty() {
        // NON-FATAL (GitHub `::warning::` annotation): RENDER_API_KEY, GITHUB_TOKEN,
        // RENDER_DEPLOY_HOOK_URL, MKS_KUBECONFIG are legitimately present and declared by no
        // deployed key — a report, never a failure.
        eprintln!("::warning::secret-gate: {} present-but-undeclared secret(s) (non-fatal report)", drift.extra.len());
        for name in &drift.extra {
            eprintln!("  UNDECLARED  {name}  [in the deploy target, declared by no deployed key]");
        }
    }

    if drift.is_fatal() {
        eprintln!("::error::secret-gate: {} declared secret(s) missing from the deploy target:", drift.missing.len());
        for m in &drift.missing {
            let why = match m.reason {
                MissingReason::Absent => "ABSENT",
                MissingReason::Empty => "EMPTY (present but blank)",
            };
            eprintln!("  {why}  repo secret '{}'  (backs env key {})", m.github_secret, m.env_key);
        }
        eprintln!("{CAVEAT}");
        return ExitCode::FAILURE;
    }

    println!("secret-gate: OK — every declared repo secret is present and non-empty.");
    eprintln!("{CAVEAT}");
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    /// THE keying property: the gate keys on `from_github_secret`, NOT the env key. A renamed pair
    /// (env `STRIPE_SECRET_KEY` sourced from repo secret `STRIPE_SECRET_KEY_PROD`) whose repo secret
    /// is absent must be fatal AND name `STRIPE_SECRET_KEY_PROD` — the name the deploy target holds,
    /// not the env key. If the gate keyed on the env key it would look up the wrong name and pass.
    #[test]
    fn renamed_pair_absent_is_fatal_and_names_the_github_secret() {
        let declared = map(&[("STRIPE_SECRET_KEY_PROD", "STRIPE_SECRET_KEY")]);
        // The DECOY (mutation-kill, beck): the env key `STRIPE_SECRET_KEY` IS present. A mutant
        // that looked up `present.get(env_key)` would find "decoy" and wrongly pass. The gate keys
        // on the `from_github_secret` name (STRIPE_SECRET_KEY_PROD), which is absent -> fatal.
        let present = map(&[("STRIPE_SECRET_KEY", "decoy"), ("SOMETHING_ELSE", "x")]);
        let drift = compare_secrets(&declared, &present);
        assert!(drift.is_fatal(), "an absent declared repo secret must be fatal");
        assert_eq!(drift.missing.len(), 1);
        let m = &drift.missing[0];
        assert_eq!(m.github_secret, "STRIPE_SECRET_KEY_PROD", "must name the repo-secret name, not the env key");
        assert_eq!(m.env_key, "STRIPE_SECRET_KEY");
        assert_eq!(m.reason, MissingReason::Absent);
    }

    /// Empty == missing (the never-written-empty doctrine): a name mapped to "" is fatal, reason
    /// Empty. Forces a map-valued present set — a SET could not distinguish this from present.
    #[test]
    fn empty_value_lands_in_missing() {
        let declared = map(&[("STRIPE_SECRET_KEY_PROD", "STRIPE_SECRET_KEY")]);
        let present = map(&[("STRIPE_SECRET_KEY_PROD", "")]);
        let drift = compare_secrets(&declared, &present);
        assert!(drift.is_fatal());
        assert_eq!(drift.missing.len(), 1);
        assert_eq!(drift.missing[0].reason, MissingReason::Empty);
    }

    /// Present-but-undeclared is a NON-FATAL report, not a failure — the legitimately-present
    /// operational secrets (RENDER_API_KEY, GITHUB_TOKEN, RENDER_DEPLOY_HOOK_URL) live here.
    #[test]
    fn present_undeclared_is_extra_not_fatal() {
        let declared = map(&[("STRIPE_SECRET_KEY_PROD", "STRIPE_SECRET_KEY")]);
        let present = map(&[
            ("STRIPE_SECRET_KEY_PROD", "sk_live_x"),
            ("RENDER_API_KEY", "rnd_x"),
            ("GITHUB_TOKEN", "ghs_x"),
        ]);
        let drift = compare_secrets(&declared, &present);
        assert!(!drift.is_fatal(), "undeclared-but-present secrets must never fail the gate");
        assert!(drift.missing.is_empty());
        // Sorted, deterministic (BTreeMap iteration order).
        assert_eq!(drift.extra, vec!["GITHUB_TOKEN".to_string(), "RENDER_API_KEY".to_string()]);
    }

    /// The green case: every declared repo secret present and non-empty, nothing undeclared.
    #[test]
    fn all_present_is_green() {
        let declared = map(&[("A_PROD", "A"), ("B", "B")]);
        let present = map(&[("A_PROD", "va"), ("B", "vb")]);
        let drift = compare_secrets(&declared, &present);
        assert!(!drift.is_fatal());
        assert!(drift.missing.is_empty());
        assert!(drift.extra.is_empty());
    }

    /// The parse keys on `from_github_secret`, not the env key — the fixture uses a RENAMED pair
    /// (env `STRIPE_SECRET_KEY` <- `STRIPE_SECRET_KEY_PROD`) and the resulting map must be keyed on
    /// the repo-secret name. This is what makes the keying property above reach the real artifact.
    #[test]
    fn declared_parse_keys_on_from_github_secret() {
        let json = r#"{
          "keys": {
            "STRIPE_SECRET_KEY": { "scope": "payments", "from_github_secret": "STRIPE_SECRET_KEY_PROD" },
            "DATABASE_URL": { "scope": "common", "from_github_secret": "DATABASE_URL" }
          }
        }"#;
        let declared = declared_from_secret_keys_json(json).expect("parses");
        assert!(declared.contains_key("STRIPE_SECRET_KEY_PROD"), "keyed on from_github_secret");
        assert!(!declared.contains_key("STRIPE_SECRET_KEY"), "must NOT be keyed on the env key");
        assert_eq!(declared["STRIPE_SECRET_KEY_PROD"], "STRIPE_SECRET_KEY");
        assert_eq!(declared["DATABASE_URL"], "DATABASE_URL");
    }

    /// A blank/non-string present secret is rejected at parse, and an empty string value survives
    /// as `""` (so the Empty branch can see it) — the parse must not coerce blanks away.
    #[test]
    fn present_parse_preserves_empty_string() {
        let present = present_from_secrets_json(r#"{ "A": "", "B": "v" }"#).expect("parses");
        assert_eq!(present["A"], "");
        assert_eq!(present["B"], "v");
    }
}
