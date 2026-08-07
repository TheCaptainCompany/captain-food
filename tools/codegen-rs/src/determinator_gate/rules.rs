//! Repo-specific path rules for the affected-set computation (#363).
//!
//! These are the ONLY hand-written inputs of the determinator gate, kept deliberately small:
//! the file → package mapping is cargo's own resolver (via the `determinator`/`guppy`
//! libraries), and anything these rules do not name falls through to the library's built-in
//! fail-open (no package + no rule ⇒ everything changed). So an omission here OVER-builds,
//! never under-builds — the safe direction. Custom rules run BEFORE the library defaults
//! (which ignore `.gitignore`, `README*`, `LICENSE*` etc. and mark `Cargo.toml`,
//! `rust-toolchain` as global).
//!
//! Every rule is asserted by a test in `main.rs`; change a rule and its test together.

use determinator::rules::DeterminatorRules;
use std::sync::OnceLock;

/// TOML because `DeterminatorRules::parse` takes the same format as the library's own
/// `default-rules.toml` — one notation to read, and the globs stay data, not code.
const CUSTOM_RULES_TOML: &str = r#"
# Spec DSL and the codegen: the generators touch everything, so a change to either rebuilds the
# full matrix (dispatch rule for #363). The materialized generated code usually changes in the
# same commit, but the guarantee must not depend on that.
[[path-rule]]
globs = ["specs/**/*", "tools/**/*"]
mark-changed = "all"

# Global build inputs of the per-bin images (PROP-20260806-223656 D5 addendum): the parametrized
# Dockerfile, the bin -> image mapping, the build workflow itself, the toolchain pin, and
# `.dockerignore` (shapes every docker build context -- this OVERRIDES the library default that
# ignores it; custom rules run first).
[[path-rule]]
globs = [
    "deploy/generated/Dockerfile.bin",
    "deploy/generated/images.json",
    ".github/workflows/build-bins.yml",
    "**/rust-toolchain", "**/rust-toolchain.toml",
    ".dockerignore",
]
mark-changed = "all"

# The deploy LEDGER and its derived manifests are deploy OUTPUT, not build input: a pin-bump
# commit (pins + regenerated manifests, ADR-20260807-220528) must not retrigger any build, or
# deploy -> build -> deploy would cycle. The two build INPUTS under deploy/generated/
# (Dockerfile.bin, images.json) are carved out by the mark-all rule ABOVE -- first match wins.
[[path-rule]]
globs = [
    "deploy/pins/**/*",
    "deploy/generated/**/*",
]
mark-changed = []

# Documentation and process surfaces: never inputs to any built artifact ("a docs commit builds
# and restarts nothing"). specs/** is deliberately NOT here -- it is a mark-all above. NO broad
# markdown glob: `Glob::new` does not treat `/` as a literal separator, so "*.md" would match a
# CRATE-LOCAL .md too -- and those can be `include_str!`-ed into a binary, making an ignore an
# under-build (the dangerous direction; caught by `crate_local_markdown_affects_its_package`).
# Root markdown is therefore ENUMERATED: the library defaults already ignore
# README*/LICENSE*/CONTRIBUTING*/CODE_OF_CONDUCT*/SECURITY* everywhere, leaving only CLAUDE.md;
# a future root-level .md falls through to fail-open-ALL until someone adds it here, reviewed.
[[path-rule]]
globs = [
    "docs/**/*",
    "CLAUDE.md",
    "LICENSES/**/*",
    ".claude/**/*",
    ".mcp.json",
    "Makefile",
    "incoming_news_from_perplexity/**/*",
]
mark-changed = []

# Other CI workflows gate what merges; they do not shape image contents. The bin build workflow
# is carved out as a global input ABOVE (first match wins via the rule order). The monolith's
# Dockerfile and render.yaml belong to the legacy single-image pipeline, which has its own
# in-workflow filter (build-image.yml) -- not inputs to the per-bin matrix.
[[path-rule]]
globs = [".github/**/*", "Dockerfile", "render.yaml"]
mark-changed = []
"#;

pub fn custom_rules() -> &'static DeterminatorRules {
    static RULES: OnceLock<DeterminatorRules> = OnceLock::new();
    RULES.get_or_init(|| {
        DeterminatorRules::parse(CUSTOM_RULES_TOML)
            .expect("embedded determinator rules TOML must parse")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_toml_parses() {
        let rules = custom_rules();
        assert!(!rules.path_rules.is_empty());
    }
}
