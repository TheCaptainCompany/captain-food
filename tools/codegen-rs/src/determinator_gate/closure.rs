//! Per-bin source-closure hash — the recorded-state half of the #363 gate.
//!
//! The pin ledger (`deploy/pins/{bin}.json`, ADR-20260807-220528) stores `{digest, source_hash}`
//! per published image. `source_hash` is what this module computes: a hash over the **git blob
//! shas** of every tracked file in the bin's workspace-crate closure, plus the global build
//! inputs, plus the bin's image name. Blob shas (via `git ls-tree -r HEAD`) rather than file
//! contents: git has already content-addressed every tracked file, so the hash is fast, and it
//! keys on COMMITTED state — exactly what CI builds — never on dirty working-tree bytes.
//!
//! The closure comes from cargo's own resolver (`guppy` over `cargo metadata`): the bin package
//! plus its transitive workspace dependencies' directories. Files under a crate directory that
//! are not build inputs (a crate README) widen the hash slightly — over-building, the safe
//! direction; narrowing would risk the silent-stale failure mode.
//!
//! Format versioned as `v1:` (the D5 addendum's "Cargo.lock wholesale v1"): any change to the
//! recipe below must bump the prefix so every pin compares as changed and everything rebuilds
//! once — fail open across format migrations too.

use guppy::graph::{DependencyDirection, PackageGraph};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Global build inputs: files OUTSIDE every crate closure that still shape every per-bin image
/// (PROP-20260806-223656 D5 addendum). `Cargo.lock` wholesale — a lock bump legitimately
/// rebuilds everything. The build workflow itself is one: it chooses the Dockerfile, the build
/// args and the publish target. Keep in step with the mark-all path rules in `rules.rs`.
const GLOBAL_INPUTS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    ".dockerignore",
    "deploy/generated/Dockerfile.bin",
    ".github/workflows/build-bins.yml",
];

/// `{bin: source_hash}` for every bin named by `deploy/generated/images.json`.
pub fn bin_source_hashes(
    graph: &PackageGraph,
    repo: &std::path::Path,
    images_rel: &str,
) -> Result<BTreeMap<String, String>, String> {
    // The hash keys on the git tree, so the graph and the git repo must be the same tree.
    let ws_root = std::path::Path::new(graph.workspace().root().as_str())
        .canonicalize()
        .map_err(|e| format!("workspace root: {e}"))?;
    let repo_canon = repo.canonicalize().map_err(|e| format!("repo {}: {e}", repo.display()))?;
    if ws_root != repo_canon {
        return Err(format!(
            "workspace root {} != repo {} — refusing to hash a different tree than git sees",
            ws_root.display(),
            repo_canon.display()
        ));
    }

    let tree = git_tree_blobs(repo)?;
    let images = load_image_map(repo, images_rel)?;

    let mut out = BTreeMap::new();
    for (bin, image) in &images {
        let dirs = closure_dirs(graph, bin)?;
        let lines = closure_lines(&tree, &dirs, image)?;
        out.insert(bin.clone(), hash_lines(&lines));
    }
    Ok(out)
}

/// `bin → image URL` from images.json (the URL participates in the hash: renaming an image must
/// republish it even with identical source).
fn load_image_map(
    repo: &std::path::Path,
    images_rel: &str,
) -> Result<BTreeMap<String, String>, String> {
    let path = repo.join(images_rel);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: invalid JSON: {e}", path.display()))?;
    let map = v
        .get("images")
        .and_then(|i| i.as_object())
        .ok_or_else(|| format!("{}: missing `images` object", path.display()))?;
    map.iter()
        .map(|(k, val)| {
            val.as_str()
                .map(|s| (k.clone(), s.to_string()))
                .ok_or_else(|| format!("{}: image '{k}' is not a string", path.display()))
        })
        .collect()
}

/// Workspace-relative directories of the bin's crate closure (the bin package + its transitive
/// workspace dependencies), sorted. Errors if the bin is not a workspace package — the caller
/// treats that as mapping drift, never as "unaffected".
fn closure_dirs(graph: &PackageGraph, bin: &str) -> Result<BTreeSet<String>, String> {
    let member = graph
        .workspace()
        .member_by_name(bin)
        .map_err(|_| format!("'{bin}' is not a workspace package — bin ↔ image mapping drifted"))?;
    let set = graph
        .query_forward(std::iter::once(member.id()))
        .map_err(|e| format!("closure query for '{bin}': {e}"))?
        .resolve();
    let root = graph.workspace().root();
    set.packages(DependencyDirection::Forward)
        .filter(|p| p.in_workspace())
        .map(|p| {
            let dir = p
                .manifest_path()
                .parent()
                .ok_or_else(|| format!("{bin}: manifest without parent dir"))?;
            let rel = dir
                .strip_prefix(root)
                .map_err(|_| format!("{bin}: crate dir {dir} outside workspace root {root}"))?;
            Ok(rel.as_str().replace('\\', "/"))
        })
        .collect()
}

/// The canonical line set a bin's hash covers: every tracked blob under its closure dirs, the
/// global inputs (`absent=` keeps a missing one deterministic AND different from present), and
/// the image name. Pure — tests feed synthetic trees.
fn closure_lines(
    tree: &BTreeMap<String, String>,
    dirs: &BTreeSet<String>,
    image: &str,
) -> Result<Vec<String>, String> {
    let mut lines: BTreeSet<String> = BTreeSet::new();
    for dir in dirs {
        let prefix = format!("{dir}/");
        let mut any = false;
        for (path, blob) in tree.range(prefix.clone()..) {
            if !path.starts_with(&prefix) {
                break;
            }
            lines.insert(format!("{path}={blob}"));
            any = true;
        }
        if !any {
            // A closure dir with no tracked files would silently drop a crate from the hash —
            // stale-production territory. Refuse instead (fail safe).
            return Err(format!("closure dir '{dir}' has no tracked files in HEAD"));
        }
    }
    for global in GLOBAL_INPUTS {
        match tree.get(*global) {
            Some(blob) => lines.insert(format!("{global}={blob}")),
            None => lines.insert(format!("absent={global}")),
        };
    }
    lines.insert(format!("image={image}"));
    Ok(lines.into_iter().collect())
}

/// `v1:` + sha256 over the sorted line set. Bump the prefix whenever the recipe changes.
fn hash_lines(lines: &[String]) -> String {
    let mut hasher = Sha256::new();
    for line in lines {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("v1:{hex}")
}

/// Every tracked blob in HEAD: `path → blob sha` via one `git ls-tree -r -z HEAD`.
fn git_tree_blobs(repo: &std::path::Path) -> Result<BTreeMap<String, String>, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["ls-tree", "-r", "-z", "HEAD"])
        .output()
        .map_err(|e| format!("git ls-tree: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git ls-tree failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = String::from_utf8(output.stdout).map_err(|e| format!("git ls-tree output: {e}"))?;
    let mut tree = BTreeMap::new();
    for entry in text.split('\0').filter(|e| !e.is_empty()) {
        // "<mode> <type> <sha>\t<path>"
        let (meta, path) = entry
            .split_once('\t')
            .ok_or_else(|| format!("git ls-tree entry without tab: {entry:?}"))?;
        let mut fields = meta.split_whitespace();
        let (_mode, kind, sha) = (
            fields.next().ok_or("ls-tree: missing mode")?,
            fields.next().ok_or("ls-tree: missing type")?,
            fields.next().ok_or("ls-tree: missing sha")?,
        );
        if kind == "blob" {
            tree.insert(path.to_string(), sha.to_string());
        }
    }
    if tree.is_empty() {
        return Err("git ls-tree returned no blobs — not a git checkout?".to_string());
    }
    Ok(tree)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guppy::MetadataCommand;
    use std::sync::OnceLock;

    fn repo_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn graph() -> &'static PackageGraph {
        static GRAPH: OnceLock<PackageGraph> = OnceLock::new();
        GRAPH.get_or_init(|| {
            let mut cmd = MetadataCommand::new();
            cmd.current_dir(repo_root());
            cmd.build_graph().expect("workspace graph builds")
        })
    }

    /// Totality — every bin in images.json gets a hash, deterministically. This is the
    /// "closure → image mapping stays complete when a bin crate is added" assertion from the
    /// issue's settled protocol: a new bin reaches images.json via the emitter (its own
    /// completeness test), and this test proves the hasher covers whatever images.json names.
    #[test]
    fn hashes_are_total_and_deterministic() {
        let a = bin_source_hashes(graph(), &repo_root(), "deploy/generated/images.json")
            .expect("hashes compute");
        let b = bin_source_hashes(graph(), &repo_root(), "deploy/generated/images.json")
            .expect("hashes compute twice");
        assert_eq!(a, b, "same tree must hash identically");
        assert!(a.len() >= 49, "expected the full bin matrix, got {}", a.len());
        for (bin, hash) in &a {
            assert!(hash.starts_with("v1:"), "{bin}: hash must carry the format version");
            assert_eq!(hash.len(), 3 + 64, "{bin}: v1 is sha256 hex");
        }
    }

    /// The hash keys on closure + globals + image name — each moves it; unrelated files don't.
    #[test]
    fn hash_moves_with_inputs_and_only_inputs() {
        let mut tree = BTreeMap::new();
        tree.insert("crates/bins/actor-order/Cargo.toml".to_string(), "aaa".to_string());
        tree.insert("crates/bins/actor-order/src/main.rs".to_string(), "bbb".to_string());
        tree.insert("crates/domains/ordering/src/lib.rs".to_string(), "ccc".to_string());
        tree.insert("Cargo.lock".to_string(), "lock1".to_string());
        tree.insert("docs/STATUS.md".to_string(), "ddd".to_string());
        let dirs: BTreeSet<String> =
            ["crates/bins/actor-order".to_string(), "crates/domains/ordering".to_string()].into();
        let base = hash_lines(&closure_lines(&tree, &dirs, "ghcr.io/x/actor-order").unwrap());

        // Unrelated file: no movement.
        let mut t2 = tree.clone();
        t2.insert("docs/STATUS.md".to_string(), "MOVED".to_string());
        assert_eq!(base, hash_lines(&closure_lines(&t2, &dirs, "ghcr.io/x/actor-order").unwrap()));

        // Closure blob: moves.
        let mut t3 = tree.clone();
        t3.insert("crates/domains/ordering/src/lib.rs".to_string(), "MOVED".to_string());
        assert_ne!(base, hash_lines(&closure_lines(&t3, &dirs, "ghcr.io/x/actor-order").unwrap()));

        // Global input (Cargo.lock wholesale): moves.
        let mut t4 = tree.clone();
        t4.insert("Cargo.lock".to_string(), "lock2".to_string());
        assert_ne!(base, hash_lines(&closure_lines(&t4, &dirs, "ghcr.io/x/actor-order").unwrap()));

        // A global going from absent to present: moves (absent= lines are load-bearing).
        let mut t5 = tree.clone();
        t5.insert(".dockerignore".to_string(), "eee".to_string());
        assert_ne!(base, hash_lines(&closure_lines(&t5, &dirs, "ghcr.io/x/actor-order").unwrap()));

        // Image rename: moves (a renamed target must republish).
        assert_ne!(base, hash_lines(&closure_lines(&tree, &dirs, "ghcr.io/x/renamed").unwrap()));
    }

    /// A closure dir with no tracked files is an error, not an empty contribution — the
    /// fail-safe against silently hashing less than the build will compile.
    #[test]
    fn empty_closure_dir_is_an_error() {
        let tree = BTreeMap::from([("Cargo.lock".to_string(), "l".to_string())]);
        let dirs: BTreeSet<String> = ["crates/bins/ghost".to_string()].into();
        assert!(closure_lines(&tree, &dirs, "img").is_err());
    }

    /// Prefix discipline: `crates/bins/actor-order-x` must not leak into `actor-order`'s
    /// closure (the `/`-terminated prefix does the separation).
    #[test]
    fn sibling_dir_with_common_prefix_stays_out() {
        let tree = BTreeMap::from([
            ("crates/bins/actor-order/src/main.rs".to_string(), "a".to_string()),
            ("crates/bins/actor-order-x/src/main.rs".to_string(), "b".to_string()),
        ]);
        let dirs: BTreeSet<String> = ["crates/bins/actor-order".to_string()].into();
        let lines = closure_lines(&tree, &dirs, "img").unwrap();
        assert!(lines.iter().any(|l| l.starts_with("crates/bins/actor-order/")));
        assert!(!lines.iter().any(|l| l.starts_with("crates/bins/actor-order-x/")));
    }

    /// Real-tree spot check. Since #385 wired the CQRS-spine families over the (monolithic)
    /// `infrastructure` crate, a WIRED bin's closure honestly includes the runtime spine — and
    /// through the `domain` facade, every scope crate: its sources really do shape the binary,
    /// so the hash must move with them (silent-stale is the failure mode this gate exists to
    /// prevent). The per-scope SHARPNESS therefore now lives in two places: the manifest-level
    /// vocabulary assertion (actor-order still cannot SPELL a catalog type — `domain` is a
    /// transitive dep, not nameable), and the still-shell families below, whose closures stay
    /// exactly their own domain slice. Re-sharpening the wired families' build closure is the
    /// recorded follow-up of splitting `infrastructure` per scope.
    #[test]
    fn real_closure_covers_bin_domains_and_globals() {
        let dirs = closure_dirs(graph(), "actor-order").expect("closure resolves");
        assert!(dirs.contains("crates/bins/actor-order"));
        assert!(dirs.contains("crates/domains/ordering"));
        assert!(dirs.contains("crates/domains/common"));
        assert!(dirs.contains("crates/bin_runtime"), "wired bins ride the composition kit");
        assert!(dirs.contains("crates/infrastructure"), "the runtime spine is a real build input");

        // A WIRED subgraph (#385) serves the master schema through `server`, so its closure
        // honestly carries the whole facade — the recorded blast-radius cost; the per-scope
        // infrastructure split is the exit. The direct scope link stays its manifest assertion.
        let subgraph = closure_dirs(graph(), "graphql-ordering").expect("closure resolves");
        assert!(subgraph.contains("crates/domains/ordering"));
        assert!(
            subgraph.contains("crates/server"),
            "a wired subgraph re-hosts the monolith's GraphQL surface (#385)"
        );

        // A WIRED surface reads only over GraphQL but renders through `web` (SSR), whose core
        // folds domain rows — so its closure carries web/core, NEVER server or infrastructure.
        let surface = closure_dirs(graph(), "fo-storefront").expect("closure resolves");
        assert!(surface.contains("crates/surface_runtime"));
        assert!(surface.contains("crates/web"));
        assert!(
            !surface.contains("crates/server") && !surface.contains("crates/infrastructure"),
            "a surface holds no server/infrastructure link (D8: no views access, no DB)"
        );

        // One bin per adapter (ADR-20260808-062432): each adapter bin's closure carries ITS OWN
        // partner crate and NO OTHER partner's — the whole point of the split is that a Stripe
        // rebuild no longer implies an Avelo37 rebuild (and vice versa), and a shared partner
        // crate sneaking into another bin's manifest would silently rebuild the family in
        // lockstep again. Derived from the workspace, never a hand list of partners.
        let adapter_bins: Vec<String> = graph()
            .packages()
            .filter(|p| p.in_workspace() && p.name().starts_with("adapter-"))
            .map(|p| p.name().to_string())
            .collect();
        assert!(adapter_bins.len() >= 5, "expected the full adapter family, found {adapter_bins:?}");
        for bin in &adapter_bins {
            let own_dir = format!("crates/adapters/{}", bin.trim_start_matches("adapter-").replace('-', "_"));
            let closure = closure_dirs(graph(), bin).expect("closure resolves");
            assert!(closure.contains(own_dir.as_str()), "{bin} misses its own partner crate {own_dir}");
            for dir in closure.iter().filter(|d| d.starts_with("crates/adapters/")) {
                assert_eq!(
                    dir, &own_dir,
                    "{bin}'s closure carries ANOTHER partner's crate — the per-partner split broke"
                );
            }
            // HONEST, like the wired spine above: the bin rides `infrastructure` (the mailbox is
            // the only door), which carries the whole domain facade transitively — the recorded
            // blast-radius cost whose exit is the per-scope infrastructure split. The MANIFEST
            // keeps domain vocabulary unspellable (no domain crate is nameable), so the sharp
            // assertion here is the per-partner one above, not a domain wall.
            assert!(
                closure.contains("crates/infrastructure"),
                "{bin} must reach the mailbox through infrastructure (verify -> mirror -> ACL -> ENQUEUE)"
            );
        }

        // One bin per worker (ADR-20260808-062933): each periodic worker's closure rides
        // `infrastructure` (the shared passes live there — no logic forks), and the SIRENE
        // worker DIRECTLY links `sirene_ingest` (the shared sweep orchestration). HONEST, like
        // the wired spine above: `infrastructure` itself depends on `sirene_ingest` for the
        // wire types (ADR-0045), so every sweep worker carries it TRANSITIVELY too — the
        // recorded blast-radius cost whose exit is the per-scope infrastructure split; the
        // sharp assertion here is the manifest-level one (only worker-sirene-sync can SPELL
        // sirene_ingest APIs), not a build-closure wall.
        for bin in ["worker-retention", "worker-erasure", "worker-sirene-sync"] {
            let closure = closure_dirs(graph(), bin).expect("closure resolves");
            assert!(
                closure.contains("crates/infrastructure"),
                "{bin} runs its pass out of infrastructure (no logic forks)"
            );
        }
        let sirene_manifest =
            std::fs::read_to_string(repo_root().join("crates/bins/worker-sirene-sync/Cargo.toml"))
                .expect("worker-sirene-sync manifest");
        assert!(
            sirene_manifest.contains("sirene_ingest = { path"),
            "worker-sirene-sync must link the shared sweep orchestration directly"
        );
        for bin in ["worker-retention", "worker-erasure"] {
            let manifest =
                std::fs::read_to_string(repo_root().join(format!("crates/bins/{bin}/Cargo.toml")))
                    .expect("worker bin manifest");
            assert!(
                !manifest.contains("sirene_ingest"),
                "{bin}: only the SIRENE worker may NAME sirene_ingest (the manifest is the scope assertion)"
            );
        }

        // A gateway's closure is the ONE that stays sharp: no domain, no server, no web (D8).
        // ALL gateway bins are asserted, derived from the workspace — a gate that samples one of
        // seven instances is not a backstop for the other six (ADR-20260803-234035): a one-off
        // patch or a role-conditional emitter branch would compile clean and slip past a sample.
        let gateway_bins: Vec<String> = graph()
            .packages()
            .filter(|p| p.name().starts_with("gateway-"))
            .map(|p| p.name().to_string())
            .collect();
        assert!(gateway_bins.len() >= 7, "expected the full gateway family, found {gateway_bins:?}");
        for bin in &gateway_bins {
            let gateway = closure_dirs(graph(), bin).expect("closure resolves");
            assert!(gateway.contains("crates/gateway_runtime"), "{bin} misses gateway_runtime");
            assert!(
                !gateway.iter().any(|d| d.starts_with("crates/domains/"))
                    && !gateway.contains("crates/server")
                    && !gateway.contains("crates/infrastructure")
                    && !gateway.contains("crates/web"),
                "{bin}'s closure carrying domain/server/web is a D8 boundary violation: {gateway:?}"
            );
        }
    }
}
