//! Integration test for the `secret-gate` binary (#444, epic #429): run the REAL compiled binary
//! against the REAL generated artifact `deploy/generated/secret-keys.json`. A unit fixture proves
//! the comparison logic; only this proves the binary parses the artifact's actual schema and exits
//! correctly — if the emitter ever changed the `keys.*.from_github_secret` shape, the unit tests
//! would stay green while the deploy gate silently broke. `CARGO_BIN_EXE_secret-gate` is the path
//! Cargo hands integration tests to the built binary.

use std::io::Write;
use std::process::{Command, Stdio};

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The declared repo-secret names of the real artifact, so the fixture can drop exactly one and
/// present the rest — robust to the catalog gaining keys over time.
fn declared_github_secrets() -> Vec<String> {
    let path = repo_root().join("deploy/generated/secret-keys.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let v: serde_json::Value = serde_json::from_str(&text).expect("secret-keys.json is valid JSON");
    let mut names: Vec<String> = v["keys"]
        .as_object()
        .expect("keys object")
        .values()
        .map(|e| e["from_github_secret"].as_str().expect("from_github_secret is a string").to_string())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The declared repo-secret names carrying NO `optional: true` (round 2, R2-1) — the ones an
/// absence must still FAIL the gate on. `missing_declared_secret_fails_the_gate_and_names_it` must
/// drop one of THESE, never an optional one (an optional secret's absence is `missing-optional`,
/// non-fatal by design, and would make that test vacuous or flaky as the catalog changes).
fn declared_required_github_secrets() -> Vec<String> {
    let path = repo_root().join("deploy/generated/secret-keys.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let v: serde_json::Value = serde_json::from_str(&text).expect("secret-keys.json is valid JSON");
    let mut names: Vec<String> = v["keys"]
        .as_object()
        .expect("keys object")
        .values()
        .filter(|e| !e["optional"].as_bool().unwrap_or(false))
        .map(|e| e["from_github_secret"].as_str().expect("from_github_secret is a string").to_string())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// A declared secret ABSENT from the present set is FATAL and the binary NAMES it. Runs the real
/// binary with the real `--declared` artifact and a present set built from it minus one name.
#[test]
fn missing_declared_secret_fails_the_gate_and_names_it() {
    let names = declared_github_secrets();
    let required = declared_required_github_secrets();
    assert!(!required.is_empty(), "the real artifact must declare at least one REQUIRED repo secret");
    let dropped = required[0].clone();
    // Present set = every declared name non-empty EXCEPT the dropped one -> exactly one missing.
    let present: serde_json::Map<String, serde_json::Value> = names
        .iter()
        .filter(|n| **n != dropped)
        .map(|n| (n.clone(), serde_json::Value::String("present".into())))
        .collect();
    let present_json = serde_json::to_string(&serde_json::Value::Object(present)).unwrap();

    let declared = repo_root().join("deploy/generated/secret-keys.json");
    let mut child = Command::new(env!("CARGO_BIN_EXE_secret-gate"))
        .args(["--declared", declared.to_str().unwrap(), "--present", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn secret-gate");
    child.stdin.take().unwrap().write_all(present_json.as_bytes()).unwrap();
    let out = child.wait_with_output().expect("wait secret-gate");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        !out.status.success(),
        "a declared secret absent from the deploy target must fail the gate (exit != 0); \
         stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains(&dropped),
        "the failure must NAME the missing repo secret '{dropped}'; stderr was:\n{stderr}"
    );
}

/// The green path through the real binary: every declared secret present and non-empty -> exit 0.
/// Pins that a fully-satisfied deploy target passes, so the fatal test above is not vacuous.
#[test]
fn all_declared_present_passes_the_gate() {
    let names = declared_github_secrets();
    let present: serde_json::Map<String, serde_json::Value> = names
        .iter()
        .map(|n| (n.clone(), serde_json::Value::String("present".into())))
        .collect();
    let present_json = serde_json::to_string(&serde_json::Value::Object(present)).unwrap();

    let declared = repo_root().join("deploy/generated/secret-keys.json");
    let mut child = Command::new(env!("CARGO_BIN_EXE_secret-gate"))
        .args(["--declared", declared.to_str().unwrap(), "--present", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn secret-gate");
    child.stdin.take().unwrap().write_all(present_json.as_bytes()).unwrap();
    let out = child.wait_with_output().expect("wait secret-gate");
    assert!(
        out.status.success(),
        "all declared secrets present -> gate passes; stderr was:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn run_gate(present: &serde_json::Value) -> std::process::Output {
    let declared = repo_root().join("deploy/generated/secret-keys.json");
    let mut child = Command::new(env!("CARGO_BIN_EXE_secret-gate"))
        .args(["--declared", declared.to_str().unwrap(), "--present", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn secret-gate");
    child.stdin.take().unwrap().write_all(present.to_string().as_bytes()).unwrap();
    child.wait_with_output().expect("wait secret-gate")
}

/// Round 2, R2-1, quote 1: the gate PASSES with `PLATFORM_BOOTSTRAP_ADMIN_SUBJECT` absent from the
/// deploy target — its `secret-keys.json` entry carries `"optional": true` (spec-derived from
/// `required: []`, never required at boot), so its absence is `missing-optional`, non-fatal. Every
/// OTHER declared secret is present, so this isolates the one key's effect.
#[test]
fn platform_bootstrap_admin_subject_absent_passes_the_gate() {
    let names = declared_github_secrets();
    assert!(
        names.iter().any(|n| n == "PLATFORM_BOOTSTRAP_ADMIN_SUBJECT"),
        "the real artifact must still declare PLATFORM_BOOTSTRAP_ADMIN_SUBJECT"
    );
    let present: serde_json::Map<String, serde_json::Value> = names
        .iter()
        .filter(|n| **n != "PLATFORM_BOOTSTRAP_ADMIN_SUBJECT")
        .map(|n| (n.clone(), serde_json::Value::String("present".into())))
        .collect();
    let out = run_gate(&serde_json::Value::Object(present));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "PLATFORM_BOOTSTRAP_ADMIN_SUBJECT absent must NOT fail the gate (a dark feature's secret \
         must never trip the release path); stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("missing-optional") && stderr.contains("PLATFORM_BOOTSTRAP_ADMIN_SUBJECT"),
        "the absence must still be REPORTED as missing-optional; stderr was:\n{stderr}"
    );
}

/// Round 2, R2-1, quote 2: the gate FAILS with `EMAIL_QUOTA_KEY_HMAC_SECRET` absent — it is
/// `required: [staging, production]`, so its `secret-keys.json` entry carries no `optional` flag,
/// and its absence stays fatal exactly as it did before this round.
#[test]
fn email_quota_key_hmac_secret_absent_fails_the_gate() {
    let names = declared_github_secrets();
    assert!(
        names.iter().any(|n| n == "EMAIL_QUOTA_KEY_HMAC_SECRET"),
        "the real artifact must still declare EMAIL_QUOTA_KEY_HMAC_SECRET"
    );
    let present: serde_json::Map<String, serde_json::Value> = names
        .iter()
        .filter(|n| **n != "EMAIL_QUOTA_KEY_HMAC_SECRET")
        .map(|n| (n.clone(), serde_json::Value::String("present".into())))
        .collect();
    let out = run_gate(&serde_json::Value::Object(present));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "EMAIL_QUOTA_KEY_HMAC_SECRET (required in staging/production) absent MUST fail the gate; \
         stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("EMAIL_QUOTA_KEY_HMAC_SECRET"),
        "the failure must NAME the required missing secret; stderr was:\n{stderr}"
    );
}
