//! `determinator` — the #363 build-matrix gate (ADR-20260807-183024 step 5).
//!
//! Two questions, one bias — **fail open to REBUILDING, never to skipping** (settled with the
//! product owner on #363, recorded in PROP-20260806-223656's D5 addendum): a false "changed"
//! costs one useless rebuild; a false "unchanged" ships stale production silently.
//!
//! - `affected` — which bins does a CHANGE touch? The affected-package set comes from the
//!   [`determinator`](https://docs.rs/determinator) library (guppy project): given the file
//!   changes between two commits it computes the affected workspace packages, with the fail-open
//!   rule built in (a changed file belonging to no package and no rule ⇒ everything changed) and
//!   Cargo build simulations including feature sets — cases a hand-rolled path map would miss.
//!   Our layer adds only the repo-specific path rules ([`rules`]) and the bin ↔ image mapping
//!   from `deploy/generated/images.json` (completeness-tested both ways). PRs build/test exactly
//!   this set.
//! - `hash` — is a bin's SOURCE identical to what the pin ledger says was published? Per bin,
//!   hash the git blob shas of its workspace crate closure + the global build inputs
//!   ([`closure`]); `deploy/pins/{bin}.json` records `{digest, source_hash}`, so the compare is
//!   repo-vs-repo, atomic with the pin (ADR-20260807-220528: pins are emitter INPUT). Rust
//!   builds are not bit-reproducible, so the skip must key on source, not digest — otherwise an
//!   unconditional rebuild mints a new digest for identical source, every pin bumps, and under
//!   `Recreate` every Deployment restarts for a docs commit.
//!
//! Exit discipline: ambiguity inside a mode resolves toward "everything affected" IN-BAND (the
//! JSON says `all: true` with a reason); only real I/O or invariant failures exit non-zero, and
//! the calling workflow treats a non-zero exit as "build everything" (the same bias, one level
//! up). Output is a single JSON object on stdout; diagnostics go to stderr.

mod closure;
mod rules;

use std::collections::BTreeSet;
use std::process::ExitCode;

use determinator::rules::PathMatch;
use determinator::Determinator;
use guppy::graph::{DependencyDirection, PackageGraph};
use guppy::{CargoMetadata, MetadataCommand};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str);
    let result = match mode {
        Some("affected") => run_affected(&args[1..]),
        Some("hash") => run_hash(&args[1..]),
        _ => Err("usage: determinator <affected|hash> [options] (see module doc)".to_string()),
    };
    match result {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("determinator: error: {e}");
            eprintln!("determinator: the calling workflow must treat this as ALL BINS AFFECTED (fail open to rebuilding)");
            ExitCode::FAILURE
        }
    }
}

/// Tiny flag parser — `--name value` pairs only; no external CLI dep for two subcommands.
fn opt(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn load_bins(repo: &std::path::Path, images: &str) -> Result<BTreeSet<String>, String> {
    let path = repo.join(images);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: invalid JSON: {e}", path.display()))?;
    let map = v
        .get("images")
        .and_then(|i| i.as_object())
        .ok_or_else(|| format!("{}: missing `images` object", path.display()))?;
    Ok(map.keys().cloned().collect())
}

fn workspace_graph(repo: &std::path::Path) -> Result<PackageGraph, String> {
    let mut cmd = MetadataCommand::new();
    cmd.current_dir(repo);
    cmd.build_graph().map_err(|e| format!("cargo metadata failed in {}: {e}", repo.display()))
}

/// `affected --changed-files <file> [--repo .] [--base-metadata <json>] [--images deploy/generated/images.json]`
///
/// `--base-metadata` is the `cargo metadata --format-version 1` output of the BASE revision's
/// tree (the CI job materializes it from a `git worktree` of the merge-base). Without it the
/// current graph stands in for both sides: path-based detection still works (a manifest edit is
/// itself a changed file inside its package), but build-summary diffs between the two
/// dependency graphs are not computed — CI always passes it.
fn run_affected(args: &[String]) -> Result<String, String> {
    let repo = std::path::PathBuf::from(opt(args, "--repo").unwrap_or_else(|| ".".into()));
    let images = opt(args, "--images").unwrap_or_else(|| "deploy/generated/images.json".into());
    let changed_file =
        opt(args, "--changed-files").ok_or("affected: --changed-files <file> is required")?;

    let changed_raw = std::fs::read_to_string(&changed_file)
        .map_err(|e| format!("cannot read --changed-files {changed_file}: {e}"))?;
    let changed: Vec<camino::Utf8PathBuf> = changed_raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(camino::Utf8PathBuf::from)
        .collect();

    let new_graph = workspace_graph(&repo)?;
    let bins = load_bins(&repo, &images)?;
    // Completeness (the D5-addendum "codegen TEST" requirement, enforced at runtime too): every
    // image is a workspace package, or the mapping has drifted and no answer is trustworthy.
    for b in &bins {
        new_graph
            .workspace()
            .member_by_name(b)
            .map_err(|_| format!("images.json names '{b}' but the workspace has no such package — bin ↔ image mapping drifted, refusing to answer"))?;
    }

    let old_graph_owned: Option<PackageGraph> = match opt(args, "--base-metadata") {
        Some(f) => {
            let json = std::fs::read_to_string(&f)
                .map_err(|e| format!("cannot read --base-metadata {f}: {e}"))?;
            let meta = CargoMetadata::parse_json(&json)
                .map_err(|e| format!("--base-metadata {f}: not cargo metadata JSON: {e}"))?;
            Some(meta.build_graph().map_err(|e| format!("--base-metadata {f}: {e}"))?)
        }
        None => None,
    };
    let old_graph: &PackageGraph = old_graph_owned.as_ref().unwrap_or(&new_graph);

    let decision = compute_affected(old_graph, &new_graph, &changed, &bins)?;
    serde_json::to_string_pretty(&decision).map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct AffectedDecision {
    /// True when every bin is affected — the full matrix. The REASON says why (a mark-all rule,
    /// a file outside every package/rule, or genuine full blast radius).
    all: bool,
    reason: Option<String>,
    affected_bins: Vec<String>,
    affected_packages: Vec<String>,
    bins_total: usize,
    changed_files: usize,
}

fn compute_affected(
    old_graph: &PackageGraph,
    new_graph: &PackageGraph,
    changed: &[camino::Utf8PathBuf],
    bins: &BTreeSet<String>,
) -> Result<AffectedDecision, String> {
    let mut det = Determinator::new(old_graph, new_graph);
    det.set_rules(rules::custom_rules()).map_err(|e| format!("path rules invalid: {e}"))?;
    det.add_changed_paths(changed.iter());

    // Record WHY the answer is "everything" when it is — `match_path` replays the per-path rule
    // walk without mutating the set. NoMatches is the library's built-in fail-open (a path
    // belonging to no package and no rule marks the whole workspace changed).
    let mut all_reason: Option<String> = None;
    for p in changed {
        match det.match_path(p, |_| {}) {
            PathMatch::NoMatches => {
                all_reason = Some(format!(
                    "'{p}' belongs to no workspace package and no rule — fail-open to the full matrix"
                ));
                break;
            }
            PathMatch::RuleMatchedAll => {
                all_reason = Some(format!("'{p}' matches a mark-all rule (global build input)"));
                break;
            }
            PathMatch::RuleMatched(_) | PathMatch::AncestorMatched => {}
        }
    }

    let det_set = det.compute();
    let affected_packages: BTreeSet<String> = det_set
        .affected_set
        .packages(DependencyDirection::Forward)
        .filter(|p| p.in_workspace())
        .map(|p| p.name().to_string())
        .collect();
    let affected_bins: Vec<String> =
        bins.iter().filter(|b| affected_packages.contains(*b)).cloned().collect();
    let all = affected_bins.len() == bins.len() && !bins.is_empty();

    Ok(AffectedDecision {
        all,
        reason: if all {
            Some(all_reason.unwrap_or_else(|| {
                "every bin is transitively affected by the changed packages".to_string()
            }))
        } else {
            all_reason // non-empty only in the impossible "rule said all but set isn't" case
        },
        affected_bins,
        affected_packages: affected_packages.into_iter().collect(),
        bins_total: bins.len(),
        changed_files: changed.len(),
    })
}

/// `hash [--repo .] [--images deploy/generated/images.json] [--pins deploy/pins]`
///
/// Emits `{hashes: {bin: source_hash}}`; with `--pins`, also `changed`/`unchanged` vs the
/// ledger. A bin is CHANGED unless its pin carries BOTH the same `source_hash` AND a digest —
/// a seeded `{null, null}` pin means "never published", which must build (fail open).
fn run_hash(args: &[String]) -> Result<String, String> {
    let repo = std::path::PathBuf::from(opt(args, "--repo").unwrap_or_else(|| ".".into()));
    let images = opt(args, "--images").unwrap_or_else(|| "deploy/generated/images.json".into());
    let graph = workspace_graph(&repo)?;
    let bins = load_bins(&repo, &images)?;
    let hashes = closure::bin_source_hashes(&graph, &repo, &images)?;
    for b in &bins {
        if !hashes.contains_key(b) {
            return Err(format!("no source hash computed for bin '{b}' — closure computation is not total"));
        }
    }

    let mut out = serde_json::Map::new();
    out.insert("hashes".into(), serde_json::to_value(&hashes).map_err(|e| e.to_string())?);

    if let Some(pins_dir) = opt(args, "--pins") {
        let mut changed: Vec<String> = Vec::new();
        let mut unchanged: Vec<String> = Vec::new();
        for (bin, hash) in &hashes {
            let pin_path = repo.join(&pins_dir).join(format!("{bin}.json"));
            let pinned = std::fs::read_to_string(&pin_path)
                .ok()
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
            let same_source = pinned
                .as_ref()
                .and_then(|p| p.get("source_hash"))
                .and_then(|h| h.as_str())
                .is_some_and(|h| h == hash);
            let has_digest = pinned
                .as_ref()
                .and_then(|p| p.get("digest"))
                .and_then(|d| d.as_str())
                .is_some();
            // Missing or malformed pin file ⇒ changed (fail open): never skip a build because
            // the ledger could not be read.
            if same_source && has_digest {
                unchanged.push(bin.clone());
            } else {
                changed.push(bin.clone());
            }
        }
        out.insert("changed".into(), serde_json::to_value(changed).map_err(|e| e.to_string())?);
        out.insert("unchanged".into(), serde_json::to_value(unchanged).map_err(|e| e.to_string())?);
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(out)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    fn repo_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// One `cargo metadata` for the whole test module — the graph build is the slow part.
    fn graph() -> &'static PackageGraph {
        static GRAPH: OnceLock<PackageGraph> = OnceLock::new();
        GRAPH.get_or_init(|| workspace_graph(&repo_root()).expect("workspace graph builds"))
    }

    fn bins() -> BTreeSet<String> {
        load_bins(&repo_root(), "deploy/generated/images.json").expect("images.json loads")
    }

    fn decide(changed: &[&str]) -> AffectedDecision {
        let changed: Vec<camino::Utf8PathBuf> =
            changed.iter().map(camino::Utf8PathBuf::from).collect();
        compute_affected(graph(), graph(), &changed, &bins()).expect("compute_affected")
    }

    /// THE dangerous failure mode (#363 dispatch): a wrong "nothing affected" answer. A file
    /// belonging to no package and no rule must fail OPEN to the full matrix — this is the
    /// asserted property, not a comment.
    #[test]
    fn unknown_file_fails_open_to_all_bins() {
        let d = decide(&["mystery-artifact.bin"]);
        assert!(d.all, "a file outside every package and rule must affect ALL bins");
        assert_eq!(d.affected_bins.len(), d.bins_total);
        assert!(d.reason.as_deref().unwrap_or("").contains("fail-open"));
    }

    /// Same property one directory deep — new top-level trees someone adds later must not be
    /// silently ignored.
    #[test]
    fn unknown_directory_fails_open_to_all_bins() {
        assert!(decide(&["new-toplevel-tree/some/file.rs"]).all);
        assert!(decide(&["migrations/20990101000000_future.sql"]).all, "migrations ride fail-open");
    }

    /// Spec and codegen changes rebuild everything (the dispatch's stated rule: codegen touches
    /// everything; the materialized generated code usually changes too, but the rule must not
    /// depend on that).
    #[test]
    fn spec_and_codegen_changes_mark_all() {
        assert!(decide(&["specs/ordering/events.yaml"]).all);
        assert!(decide(&["specs/api.yaml"]).all);
        assert!(decide(&["tools/codegen-rs/src/emit/deploy.rs"]).all);
    }

    /// Global build inputs of the per-bin images. `.dockerignore` deliberately OVERRIDES the
    /// determinator library's default (which ignores it): it shapes every docker build context.
    #[test]
    fn global_build_inputs_mark_all() {
        assert!(decide(&["rust-toolchain.toml"]).all);
        assert!(decide(&[".dockerignore"]).all);
        assert!(decide(&["deploy/generated/Dockerfile.bin"]).all);
        assert!(decide(&["deploy/generated/images.json"]).all);
        assert!(decide(&[".github/workflows/build-bins.yml"]).all);
        assert!(decide(&["Cargo.toml"]).all, "root manifest carries profiles/lints — library default");
    }

    /// Docs and process files build nothing — the "a docs commit restarts nothing" half of the
    /// D5 addendum's net effect.
    #[test]
    fn docs_and_process_files_affect_nothing() {
        let d = decide(&[
            "docs/STATUS.md",
            "docs/adr/ADR-20260807-183024-one-decomposition-axis.md",
            "README.md",
            "CLAUDE.md",
            ".claude/agents/architect.md",
            ".github/workflows/ci.yml",
            "Makefile",
        ]);
        assert!(!d.all);
        assert!(d.affected_bins.is_empty(), "affected: {:?}", d.affected_bins);
    }

    /// The no-loop property: a pin-bump commit (pins + regenerated manifests) must not
    /// retrigger any build, or deploy → build → deploy would cycle forever.
    #[test]
    fn pin_bump_commit_affects_nothing() {
        let d = decide(&[
            "deploy/pins/actor-order.json",
            "deploy/generated/manifests/bins/actor-order.yaml",
            "deploy/generated/manifests/kustomization.yaml",
            "deploy/generated/README.md",
            "deploy/generated/secret-keys.json",
        ]);
        assert!(!d.all);
        assert!(d.affected_bins.is_empty(), "affected: {:?}", d.affected_bins);
    }

    /// Markdown is ignored at the ROOT only: inside a package a .md file can be
    /// `include_str!`-ed into the binary, so it must affect its package (under-building is the
    /// dangerous direction; review finding on #386).
    #[test]
    fn crate_local_markdown_affects_its_package() {
        let d = decide(&["crates/domains/ordering/NOTES.md"]);
        assert!(!d.all);
        assert!(
            d.affected_bins.iter().any(|b| b == "actor-order"),
            "a crate-local .md is package content: {:?}",
            d.affected_bins
        );
    }

    /// Blast radius is the spec-derived crate graph, not a hand list (ADR-20260807-183024 —
    /// "the spec's coupling becomes the compile-and-deploy coupling, mechanically"). Since #385
    /// wired every family over the existing crates, a domain-scope change honestly reaches every
    /// bin that carries domain vocabulary: the spine through `infrastructure` → `domain` facade,
    /// the subgraphs through `server` (same facade), the surfaces through `web` → `core` →
    /// `domain` (the SSR renderer folds domain rows). The recorded exits: the per-scope
    /// `infrastructure` split re-sharpens the spine and subgraphs; a domain-free SSR data layer
    /// would re-sharpen the surfaces. The ONE family that keeps the sharp radius today is the
    /// gateways — no domain, no server, no web (D8), and this test is the wall that keeps it so.
    #[test]
    fn domain_scope_change_scopes_the_blast_radius() {
        let d = decide(&["crates/domains/ordering/src/lib.rs"]);
        assert!(!d.all, "one scope must not rebuild the world");
        let hit: BTreeSet<&str> = d.affected_bins.iter().map(String::as_str).collect();
        for expected in ["actor-order", "projector-ordering", "graphql-ordering", "pm-place-order", "bam"] {
            assert!(hit.contains(expected), "{expected} links domain-ordering and must be affected");
        }
        // Bins of OTHER scopes are hit through the shared runtime crates: honest, not a
        // hand-list — re-sharpened by the recorded #385 follow-ups, not by editing this test.
        assert!(hit.contains("actor-rider"), "wired bins couple through the runtime spine (recorded #385 limit)");
        assert!(hit.contains("graphql-delivery"), "subgraphs couple through server's facade (recorded #385 limit)");
        assert!(hit.contains("fo-storefront"), "surfaces couple through web -> core -> domain (recorded #385 limit)");
        assert!(
            !hit.contains("gateway-public"),
            "gateways hold no domain vocabulary (D8) — the one family with a sharp radius; a domain \
             crate reaching a gateway closure is a boundary violation, not a test to relax"
        );
    }

    /// The kernel honestly ripples every domain-linking bin (recorded limit, not a bug)…
    #[test]
    fn kernel_change_ripples_every_domain_bin() {
        let d = decide(&["crates/domains/common/src/lib.rs"]);
        let hit: BTreeSet<&str> = d.affected_bins.iter().map(String::as_str).collect();
        for expected in ["actor-order", "actor-rider", "bam", "projector-payments"] {
            assert!(hit.contains(expected), "{expected} links domain-common");
        }
        assert!(!hit.contains("gateway-public"), "…but never the domain-free gateways (D8)");
    }

    /// …and a bin-local change affects that bin alone.
    #[test]
    fn bin_local_change_affects_only_that_bin() {
        let d = decide(&["crates/bins/actor-order/src/main.rs"]);
        assert_eq!(d.affected_bins, vec!["actor-order".to_string()]);
    }

    /// Every image is a workspace package — the runtime completeness refusal (a drifted mapping
    /// must be an error, never a silent partial answer).
    #[test]
    fn every_image_is_a_workspace_package() {
        for b in bins() {
            assert!(
                graph().workspace().member_by_name(&b).is_ok(),
                "images.json names '{b}' but the workspace has no such package"
            );
        }
    }
}
