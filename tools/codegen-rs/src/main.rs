//! Captain.Food codegen (ADR-0034) — the single spec gate.
//!
//! It loads every `specs/**` DSL file and runs the full validator (§1–§11: referential integrity, actor
//! wiring, api↔model, views, stories, tests, rules, translations, screens, observability, C4) and every
//! generator (translations, views SQL + the `database.md` §2 injection, C4 Structurizr/Mermaid, GraphQL
//! SDL, and the Markdown + HTML docs). It began as a TypeScript tool (`tools/codegen`) and was ported here
//! at parity — all 8 generated artifacts byte-identical and the same (rule, location) validation issue set
//! (verified by a differential harness) — after which the TypeScript codegen was retired. CI (`codegen`
//! job) builds + tests, validates, regenerates and fails on any drift.

pub(crate) use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
pub(crate) use std::fs;
pub(crate) use std::path::PathBuf;

pub(crate) use serde_yaml::Value;

// Since #373 (ADR-20260807-183024 step 2) the Rust domain-type emitters write PER-SCOPE crates
// under crates/domains/ (manifests included, deps derived from the spec's $ref edges) and
// crates/domain re-exporting facades — see emit/domain_scopes.rs for the doctrine.
//
// Module map (#277 split — pure code motion out of the former single-file main.rs). Everything is
// re-exported pub(crate) at the crate root, so modules share one flat namespace via `use crate::*;`
// exactly as they did when this was one file.
mod api; // GraphQL surface parsing (api.yaml) + SDL emitter
mod c4; // C4/actor model structs, parse_actors, Structurizr/Mermaid emitters
mod config; // configuration.yaml parse + §12 validation + typed-reader emitter
mod emit; // generated-artifact emitters, one module per artifact family
mod model; // spec loading, Model, $ref primitives, Issue/Coverage/Report
mod refs; // Kind, REF_CONTRACT, classify — the §1b ref-kind contract
mod validate; // validator sections; run order lives in validate() (validate::core)
#[cfg(test)]
mod tests;

pub(crate) use api::*;
pub(crate) use c4::*;
pub(crate) use config::*;
pub(crate) use emit::*;
pub(crate) use model::*;
pub(crate) use refs::*;
pub(crate) use validate::*;

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
}

/// Repo root, derived from the `--specs` path's parent (so generated crate files land correctly whether
/// `--specs` is relative like `specs` or an absolute path).
fn repo_root(specs: &std::path::Path) -> PathBuf {
    match specs.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
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

    let Report { mut issues, coverage, handled_commands } = validate(&model);
    // ─── §13 — proposal hygiene (docs/proposals/PROP-*.md, #272) — runs alongside the spec
    // validator: same issue list, same gate, but reads the proposals corpus from the repo root
    // (derived from `--specs`) because proposals are markdown, not part of the YAML model.
    let proposals = load_proposal_files(&repo_root(&specs));
    issues.extend(validate_proposal_hygiene(&proposals));
    // ─── §16 — writer/schema agreement (#474): a NOT NULL column with no DEFAULT that its
    // writer's insert list omits fails EVERY insert (the #451 cart defect, which passed `cargo
    // check`, six hand-run suites and three `make rust` rounds). Same posture as §13: reads
    // repo text rather than the YAML model, joins the same issue list and the same gate.
    let root = repo_root(&specs);
    issues.extend(validate_writer_schema_agreement(
        &load_migration_files(&root),
        &load_writer_files(&root),
    ));
    let errors: Vec<&Issue> = issues.iter().filter(|i| i.level == Level::Error).collect();
    let warnings: Vec<&Issue> = issues.iter().filter(|i| i.level == Level::Warning).collect();

    // Summary counts (mirrors cli.ts), derived from the model.
    let n_actors = parse_actors(&model).len();
    let n_commands = handled_commands; // cli.ts prints derived.handledCommands.size, not total defs
    let n_events = map_keys(model.defs.get("events.yaml")).len();
    let n_errdefs = map_keys(model.defs.get("errors.yaml")).len();
    let n_personas = parse_stories(&model).len();
    let n_activities: usize = parse_stories(&model).iter().map(|p| p.activities.len()).sum();
    let n_fixtures = model
        .defs
        .get("tests.yaml")
        .and_then(|t| t.get("fixtures"))
        .and_then(|f| f.as_mapping())
        .map(|m| m.len())
        .unwrap_or(0);
    let n_bcs = model
        .defs
        .get("architecture/c4-l2.yaml")
        .and_then(|v| v.get("boundedContexts"))
        .and_then(|x| x.as_mapping())
        .map(|m| m.len())
        .unwrap_or(0);

    eprintln!("• specs:  {}", specs.display());
    eprintln!("• model:  {} actors, {} commands, {} events, {} errors", n_actors, n_commands, n_events, n_errdefs);
    let api_s = parse_api(&model);
    eprintln!("• api:    {} mutations, {} queries, {} projections", api_s.mutations.len(), api_s.queries.len(), api_s.types.len());
    eprintln!("• stories:{} personas, {} activities", n_personas, n_activities);
    eprintln!("• views:  {} views, {} columns, {} fedBy links", coverage.views, coverage.view_columns, coverage.view_fed_by);
    eprintln!("• tests:  {} behaviour tests, {} fixtures, {} business rules", coverage.test_cases, n_fixtures, coverage.rules);
    eprintln!("• obs:    {} observability contracts · C4: {} bounded contexts", coverage.obs_contracts, n_bcs);
    eprintln!(
        "• ui:     {} SDUI screens, {} API bindings, {} gaps · {} translation keys (en/fr)",
        coverage.screens, coverage.screen_bindings, coverage.screen_gaps, coverage.translations
    );
    eprintln!("• validated against specs:");
    eprintln!(
        "    - {} $refs resolve AND point at the kind their site declares (REF_CONTRACT, §1b)",
        coverage.refs
    );
    eprintln!("    - actor wiring: messages→commands/events, emits→events, throws→errors");
    eprintln!("    - lifecycles: {} aggregate state machines, {} transitions (lc-*: states∈enum, events emitted, deterministic, terminal closed, reachable)", coverage.lifecycles, coverage.lifecycle_transitions);
    eprintln!("    - api↔model: {} command links→commands, {} reads→views, roles→UserType", coverage.mutation_links, coverage.reads_links);
    eprintln!("    - read models: {} c4-l3 component reads→views; every read model has a declared reader (api type, component, or internal)", coverage.component_reads_links);
    eprintln!("    - views: aggregate→actors, fedBy→events, column types→scalars, indexes→columns, fk→views");
    eprintln!("    - stories: {} step→op links resolve, persona role authorized, every mutation/query reached by a story step", coverage.story_links);
    eprintln!("    - tests: {} Given/When/Then cases — data fields, actor handles `when`, `then`⊆emits, `thrown`⊆throws; every message/event/error exercised", coverage.test_cases);
    eprintln!("    - rules: {} business rules — every test asserts ≥1 rule, every rule asserted by ≥1 test (ADR-0032)", coverage.rules);
    eprintln!("    - ui: {} SDUI screens — resolver/action bindings $ref real api ops (API-meets-UI), data_requirements resolve; {} translations (en+fr, params match)", coverage.screens, coverage.translations);
    eprintln!("    - observability: {} workflow contracts — $ref/surface bindings resolve, mandatory ids (correlation_id/trace_id), span kinds, success.required_spans ⊆ declared spans", coverage.obs_contracts);
    eprintln!("    - c4: bounded-context↔actor mapping (no unmapped aggregate / phantom container ref)");
    eprintln!(
        "    - proposals: {} docs/proposals/PROP-*.md — Status header, tracking-issue link, Concerns resolved before Approved, Approved names an ADR",
        proposals.len()
    );

    if !issues.is_empty() {
        eprintln!("• checks: {} error(s), {} warning(s)", errors.len(), warnings.len());
        for i in &issues {
            let tag = if i.level == Level::Error { "error" } else { "warn " };
            eprintln!("  [{}] {}  {}\n           {}", tag, i.rule, i.location, i.message);
        }
    } else {
        eprintln!("• checks: all cross-references resolve, no warnings");
    }

    if !errors.is_empty() {
        eprintln!("\n✗ validation failed — fix the errors above before generating.");
        std::process::exit(1);
    }

    if check {
        eprintln!("\n✓ validation passed (--check: no files written).");
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
    let artifacts: [(&str, String); 10] = [
        // The CI env-sync manifest (PROP-20260729-014500): which repo secret supplies which service
        // env key, per profile. Baked values are NOT here — they ride the image (D5).
        ("render-config-sync.json", emit_render_sync_manifest(&model)),
        // The derived crate topology (#373, ADR-20260807-183024 step 2): scope crate → deps,
        // actor/PM bin → domain crates — the reviewable face of the $ref→dependency derivation
        // and the input contract for step (3)'s bin emitter.
        ("crate-graph.generated.json", emit_crate_graph(&model)),
        ("translations.generated.json", emit_translations_json(&model)),
        ("views.generated.sql", emit_views_sql(&model)),
        ("schema.generated.sql", emit_schema_sql(&model, &specs)),
        ("c4.generated.dsl", emit_structurizr(&model)),
        ("c4.generated.md", emit_mermaid(&model)),
        ("schema.generated.graphql", emit_schema(&model)),
        ("documentation.generated.md", emit_documentation(&model)),
        (
            "documentation.generated.html",
            format!(
                "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>Captain.Food — Product Documentation</title>\n</head>\n<body>\n{}\n</body>\n</html>\n",
                emit_documentation_html(&model)
            ),
        ),
    ];
    for (name, content) in &artifacts {
        let path = out_dir.join(name);
        if let Err(e) = fs::write(&path, content) {
            eprintln!("✗ write {}: {}", path.display(), e);
            std::process::exit(1);
        }
        eprintln!("✓ wrote {}", path.display());
    }
    // database.md: inject the §2 read-model tables between the GENERATED:views markers (in-place).
    let db_md = specs.join("database.md");
    match inject_generated(&db_md, "views", &emit_views_markdown(&model)) {
        Ok(true) => eprintln!("✓ injected views into {}", db_md.display()),
        Ok(false) => eprintln!("! {}: no GENERATED:views markers — skipped", db_md.display()),
        Err(e) => {
            eprintln!("✗ {}", e);
            std::process::exit(1);
        }
    }
    // crates/domains/{scope}/: PER-SCOPE GENERATED domain crates + the kernel (#373,
    // ADR-20260807-183024 step 2). Manifest AND code are generated, so "which scopes exist" and
    // "which crates exist" cannot drift. STALE crates are REMOVED (a crate for a scope the spec no
    // longer declares is a door to nothing that still compiles — same rule as crates/clients).
    // Guarded on a non-empty emission so a degenerate flat-layout model can never mass-delete.
    let scope_crates = emit_domain_scope_crates(&model);
    if !scope_crates.is_empty() {
        let domains_root = repo_root(&specs).join("crates/domains");
        let keep: std::collections::BTreeSet<String> = scope_crates
            .iter()
            .filter_map(|c| c.dir.rsplit('/').next().map(|s| s.to_string()))
            .collect();
        if let Ok(rd) = fs::read_dir(&domains_root) {
            for e in rd.flatten() {
                let p = e.path();
                let stale = p.is_dir()
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| !keep.contains(n));
                if stale {
                    if let Err(e) = fs::remove_dir_all(&p) {
                        eprintln!("✗ remove stale domain scope crate {}: {}", p.display(), e);
                        std::process::exit(1);
                    }
                    eprintln!("✓ removed stale domain scope crate {}", p.display());
                }
            }
        }
        for c in &scope_crates {
            let dir = repo_root(&specs).join(&c.dir);
            if let Err(e) = fs::create_dir_all(dir.join("src")) {
                eprintln!("✗ create {}: {}", dir.display(), e);
                std::process::exit(1);
            }
            let manifest_path = dir.join("Cargo.toml");
            if let Err(e) = fs::write(&manifest_path, &c.manifest) {
                eprintln!("✗ write {}: {}", manifest_path.display(), e);
                std::process::exit(1);
            }
            for (name, content) in &c.files {
                let path = dir.join(name);
                if let Err(e) = fs::write(&path, content) {
                    eprintln!("✗ write {}: {}", path.display(), e);
                    std::process::exit(1);
                }
            }
            eprintln!("✓ wrote {}", c.dir);
        }
    }
    // crates/bins/{name}/: PER-DEPLOYABLE BIN CRATES (#382, ADR-20260807-183024 step 3). One
    // crate per c4-l2 deployable, manifest = the bin's scope assertion (deps from the derived
    // crate graph). STALE bins are REMOVED (a bin for a deployable the topology no longer
    // declares is an image that deploys nowhere but still builds). Guarded on a non-empty
    // topology so a degenerate flat-layout model can never mass-delete.
    let bin_crates = emit_bin_crates(&model);
    if !bin_crates.is_empty() {
        let bins_root = repo_root(&specs).join("crates/bins");
        let keep: std::collections::BTreeSet<String> =
            bin_crates.iter().map(|c| c.name.clone()).collect();
        if let Ok(rd) = fs::read_dir(&bins_root) {
            for e in rd.flatten() {
                let p = e.path();
                let stale = p.is_dir()
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| !keep.contains(n));
                if stale {
                    if let Err(e) = fs::remove_dir_all(&p) {
                        eprintln!("✗ remove stale bin crate {}: {}", p.display(), e);
                        std::process::exit(1);
                    }
                    eprintln!("✓ removed stale bin crate {}", p.display());
                }
            }
        }
        for c in &bin_crates {
            let dir = repo_root(&specs).join(&c.dir);
            if let Err(e) = fs::create_dir_all(dir.join("src")) {
                eprintln!("✗ create {}: {}", dir.display(), e);
                std::process::exit(1);
            }
            for (name, content) in [("Cargo.toml", &c.manifest), ("src/main.rs", &c.main)] {
                let path = dir.join(name);
                if let Err(e) = fs::write(&path, content) {
                    eprintln!("✗ write {}: {}", path.display(), e);
                    std::process::exit(1);
                }
            }
            // The scope-filtered Config reader (#374 Q4): present exactly for WIRED bins; a
            // family falling back to shell must not leave a stale module behind.
            let config_path = dir.join("src/config.rs");
            match &c.config {
                Some(content) => {
                    if let Err(e) = fs::write(&config_path, content) {
                        eprintln!("✗ write {}: {}", config_path.display(), e);
                        std::process::exit(1);
                    }
                }
                None => {
                    if config_path.exists() {
                        if let Err(e) = fs::remove_file(&config_path) {
                            eprintln!("✗ remove stale {}: {}", config_path.display(), e);
                            std::process::exit(1);
                        }
                    }
                }
            }
        }
        eprintln!("✓ wrote {} bin crates under crates/bins/", bin_crates.len());
    }
    // deploy/: THE GENERATED DEPLOYMENT (#349, ADR-20260807-183024 step 4). `deploy/generated/`
    // is emitter-owned (stale files pruned); `deploy/pins/` is the CI-owned deploy ledger — the
    // emitter READS pins to bake image digests into the Deployments, SEEDS missing pin files
    // with nulls, and never overwrites or prunes an existing pin (a stale pin is a codegen-test
    // failure, not a silent deletion of deploy history). Guarded on a non-empty topology so a
    // degenerate flat-layout model can never mass-delete the deploy tree.
    if !bin_crates.is_empty() {
        let root = repo_root(&specs);
        let pins = match read_image_pins(&root) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("✗ deploy pins: {e}");
                std::process::exit(1);
            }
        };
        let tree = emit_deploy_tree(&model, &pins);
        let gen_root = root.join("deploy/generated");
        let keep: std::collections::BTreeSet<PathBuf> =
            tree.iter().map(|(p, _)| gen_root.join(p)).collect();
        // Prune: any file under deploy/generated/ the emitter no longer produces is stale (a
        // manifest for a deployable the topology dropped would still be applied by GitOps).
        let mut stack = vec![gen_root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if !keep.contains(&p) {
                    if let Err(err) = fs::remove_file(&p) {
                        eprintln!("✗ remove stale deploy file {}: {}", p.display(), err);
                        std::process::exit(1);
                    }
                    eprintln!("✓ removed stale deploy file {}", p.display());
                }
            }
        }
        for (rel, content) in &tree {
            let path = gen_root.join(rel);
            if let Some(parent) = path.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    eprintln!("✗ create {}: {}", parent.display(), e);
                    std::process::exit(1);
                }
            }
            if let Err(e) = fs::write(&path, content) {
                eprintln!("✗ write {}: {}", path.display(), e);
                std::process::exit(1);
            }
        }
        // Seed missing pins (never overwrite: pins are CI state, not spec-derived).
        let pins_dir = root.join("deploy/pins");
        if let Err(e) = fs::create_dir_all(&pins_dir) {
            eprintln!("✗ create {}: {}", pins_dir.display(), e);
            std::process::exit(1);
        }
        let mut seeded = 0usize;
        for c in &bin_crates {
            let p = pins_dir.join(format!("{}.json", c.name));
            if !p.exists() {
                if let Err(e) = fs::write(&p, pin_skeleton_json()) {
                    eprintln!("✗ write {}: {}", p.display(), e);
                    std::process::exit(1);
                }
                seeded += 1;
            }
        }
        eprintln!(
            "✓ wrote deploy/generated/ ({} files){}",
            tree.len(),
            if seeded > 0 { format!(", seeded {seeded} pin file(s)") } else { String::new() }
        );
    }
    // crates/domain/src/generated/{scalars,entities,events,commands}.rs: since #373 RE-EXPORT
    // facades over the per-scope crates (+ the cross-scope DomainEvent union, global error
    // catalog, states and lifecycles). mod.rs lists them.
    let gen_dir = repo_root(&specs).join("crates/domain/src/generated");
    if let Err(e) = fs::create_dir_all(&gen_dir) {
        eprintln!("✗ create {}: {}", gen_dir.display(), e);
        std::process::exit(1);
    }
    for (name, content) in [
        ("scalars.rs", emit_domain_scalars(&model)),
        ("entities.rs", emit_domain_entities(&model)),
        ("events.rs", emit_domain_events(&model)),
        ("commands.rs", emit_domain_commands(&model)),
        ("errors.rs", emit_domain_errors(&model)),
        ("lifecycles.rs", emit_domain_lifecycles(&model)),
        ("states.rs", emit_domain_states(&model)),
        ("mod.rs", "// GENERATED module index — do not edit by hand.\npub mod scalars;\npub mod entities;\npub mod events;\npub mod commands;\npub mod errors;\npub mod lifecycles;\npub mod states;\n".to_string()),
    ] {
        let path = gen_dir.join(name);
        if let Err(e) = fs::write(&path, content) {
            eprintln!("✗ write {}: {}", path.display(), e);
            std::process::exit(1);
        }
        eprintln!("✓ wrote {}", path.display());
    }
    // crates/application/src/generated/: read-model row types from projection_tables.yaml (ADR-0040).
    let app_gen = repo_root(&specs).join("crates/application/src/generated");
    if let Err(e) = fs::create_dir_all(&app_gen) {
        eprintln!("✗ create {}: {}", app_gen.display(), e);
        std::process::exit(1);
    }
    for (name, content) in [
        ("rows.rs", emit_projection_rows(&model)),
        ("projectors.rs", emit_projectors(&model)),
        ("pm_state.rs", emit_pm_state_application(&model)),
        ("services.rs", emit_services_application(&model)),
        ("process_managers.rs", emit_pm_orchestrators(&model)),
        ("handlers.rs", emit_application_handlers(&model)),
        ("reminders.rs", emit_app_reminders(&model)),
        ("behaviour_tests.rs", emit_behaviour_tests(&model)),
        ("mod.rs", "// GENERATED module index — do not edit by hand.\npub mod rows;\npub mod projectors;\npub mod pm_state;\npub mod process_managers;\npub mod services;\npub mod handlers;\npub mod reminders;\n#[cfg(test)]\npub mod behaviour_tests;\n".to_string()),
    ] {
        let path = app_gen.join(name);
        if let Err(e) = fs::write(&path, content) {
            eprintln!("✗ write {}: {}", path.display(), e);
            std::process::exit(1);
        }
        eprintln!("✓ wrote {}", path.display());
    }
    // crates/actor_client/src/generated/: the typed per-actor clients + the frozen addressing
    // tables (#290 phase 1, PROP-20260802-130500 D1) — emitted INTO the boundary crate that owns
    // the private MailboxEntry, so the write door is compiler-enforced.
    let client_gen = repo_root(&specs).join("crates/actor_client/src/generated");
    if let Err(e) = fs::create_dir_all(&client_gen) {
        eprintln!("✗ create {}: {}", client_gen.display(), e);
        std::process::exit(1);
    }
    for (name, content) in [
        ("addresses.rs", emit_actor_addresses(&model)),
        ("mod.rs", "// GENERATED module index — do not edit by hand.\npub mod addresses;\n".to_string()),
    ] {
        let path = client_gen.join(name);
        if let Err(e) = fs::write(&path, content) {
            eprintln!("✗ write {}: {}", path.display(), e);
            std::process::exit(1);
        }
        eprintln!("✓ wrote {}", path.display());
    }
    // crates/clients/{actor}/: ONE CRATE PER MAILBOX ACTOR (PROP-20260802-130500 phase 2, #306).
    // Manifest AND code are generated, so "which actors exist" and "which crates exist" cannot
    // drift. STALE crates are REMOVED here rather than left behind: a client crate for an actor
    // the spec no longer declares is a door to nothing that still compiles, and `check-drift`
    // diffs content — it would never notice a directory that simply stopped being regenerated.
    let clients_root = repo_root(&specs).join("crates/clients");
    let emitted = emit_client_crates(&model);
    let keep: std::collections::BTreeSet<String> = emitted
        .iter()
        .filter_map(|c| c.dir.rsplit('/').next().map(|s| s.to_string()))
        .collect();
    if let Ok(rd) = fs::read_dir(&clients_root) {
        for e in rd.flatten() {
            let p = e.path();
            let stale = p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| !keep.contains(n));
            if stale {
                if let Err(e) = fs::remove_dir_all(&p) {
                    eprintln!("✗ remove stale client crate {}: {}", p.display(), e);
                    std::process::exit(1);
                }
                eprintln!("✓ removed stale client crate {}", p.display());
            }
        }
    }
    for c in &emitted {
        let dir = repo_root(&specs).join(&c.dir);
        if let Err(e) = fs::create_dir_all(dir.join("src")) {
            eprintln!("✗ create {}: {}", dir.display(), e);
            std::process::exit(1);
        }
        for (name, content) in [("Cargo.toml", &c.manifest), ("src/lib.rs", &c.lib)] {
            let path = dir.join(name);
            if let Err(e) = fs::write(&path, content) {
                eprintln!("✗ write {}: {}", path.display(), e);
                std::process::exit(1);
            }
        }
        eprintln!("✓ wrote {}", c.dir);
    }
    // crates/infrastructure/src/generated/: the Postgres PM state stores from process_managers.yaml
    // (issue #27) — the adapter side of the application pm_state ports.
    let infra_gen = repo_root(&specs).join("crates/infrastructure/src/generated");
    if let Err(e) = fs::create_dir_all(&infra_gen) {
        eprintln!("✗ create {}: {}", infra_gen.display(), e);
        std::process::exit(1);
    }
    // The deletion-engine parameter table exists only once an actor declares `deletion:`
    // (ADR-20260731-214500) — absent, neither the file nor its mod.rs line is emitted (zero drift).
    let deletion_policy = emit_infra_deletion_policy(&model);
    let infra_mod = format!(
        "// GENERATED module index — do not edit by hand.\npub mod pm_state;\npub mod service_clients;\npub mod service_bindings;\npub mod command_router;\npub mod scopes;\n{}",
        if deletion_policy.is_some() { "pub mod deletion_policy;\n" } else { "" }
    );
    let mut infra_files: Vec<(&str, String)> = vec![
        ("pm_state.rs", emit_pm_state_infrastructure(&model)),
        ("service_clients.rs", emit_services_http_clients(&model)),
        ("service_bindings.rs", emit_service_bindings(&model)),
        ("command_router.rs", emit_infra_command_router(&model)),
        ("scopes.rs", emit_actor_scopes(&model)),
        ("mod.rs", infra_mod),
    ];
    if let Some(dp) = deletion_policy {
        infra_files.push(("deletion_policy.rs", dp));
    }
    for (name, content) in infra_files {
        let path = infra_gen.join(name);
        if let Err(e) = fs::write(&path, content) {
            eprintln!("✗ write {}: {}", path.display(), e);
            std::process::exit(1);
        }
        eprintln!("✓ wrote {}", path.display());
    }
    // crates/server/src/generated/: the expose-gated /services/* routes from services.yaml (issue #26).
    let srv_svc_gen = repo_root(&specs).join("crates/server/src/generated");
    if let Err(e) = fs::create_dir_all(&srv_svc_gen) {
        eprintln!("✗ create {}: {}", srv_svc_gen.display(), e);
        std::process::exit(1);
    }
    for (name, content) in [
        ("services_routes.rs", emit_services_routes(&model)),
        ("config.rs", emit_config(&model)),
        ("mod.rs", "// GENERATED module index — do not edit by hand.\npub mod config;\npub mod services_routes;\n".to_string()),
    ] {
        let path = srv_svc_gen.join(name);
        if let Err(e) = fs::write(&path, content) {
            eprintln!("✗ write {}: {}", path.display(), e);
            std::process::exit(1);
        }
        eprintln!("✓ wrote {}", path.display());
    }
    // crates/server/src/graphql/generated/: the async-graphql type layer from api.yaml (Stage 1a) —
    // wrapper scalars/mirror enums, SimpleObject output types, InputObject inputs, and the QueryRoot.
    let srv_gen = repo_root(&specs).join("crates/server/src/graphql/generated");
    if let Err(e) = fs::create_dir_all(&srv_gen) {
        eprintln!("✗ create {}: {}", srv_gen.display(), e);
        std::process::exit(1);
    }
    for (name, content) in [
        ("scalars.rs", emit_server_scalars(&model)),
        ("types.rs", emit_server_types(&model)),
        ("inputs.rs", emit_server_inputs(&model)),
        ("acl.rs", emit_server_acl(&model)),
        ("query.rs", emit_server_query(&model)),
        ("mutation.rs", emit_server_mutation(&model)),
        ("subscription.rs", emit_server_subscription(&model)),
        ("operation_scopes.rs", emit_server_operation_scopes(&model)),
        ("mod.rs", "// GENERATED module index — do not edit by hand.\npub mod scalars;\npub mod types;\npub mod inputs;\npub mod acl;\npub mod query;\npub mod mutation;\npub mod subscription;\npub mod operation_scopes;\n".to_string()),
    ] {
        let path = srv_gen.join(name);
        if let Err(e) = fs::write(&path, content) {
            eprintln!("✗ write {}: {}", path.display(), e);
            std::process::exit(1);
        }
        eprintln!("✓ wrote {}", path.display());
    }
    // crates/web/src/generated/: the SDUI component registry (allowlist) from restaurant_frontoffice.yaml
    // (#/component_registry) — the Leptos renderer's GENERATED dispatch surface (codegen roadmap
    // item 6, ADR-0033). Keeps the screens DSL the source of truth for what components may render.
    let web_gen = repo_root(&specs).join("crates/web/src/generated");
    if let Err(e) = fs::create_dir_all(&web_gen) {
        eprintln!("✗ create {}: {}", web_gen.display(), e);
        std::process::exit(1);
    }
    for (name, content) in [
        ("registry.rs", emit_web_registry(&model)),
        ("data_layer.rs", emit_web_data_layer(&model)),
        ("screens.rs", emit_web_screens(&model)),
        (
            "mod.rs",
            "// GENERATED module index — do not edit by hand.\npub mod data_layer;\npub mod registry;\npub mod screens;\n".to_string(),
        ),
    ] {
        let path = web_gen.join(name);
        if let Err(e) = fs::write(&path, content) {
            eprintln!("✗ write {}: {}", path.display(), e);
            std::process::exit(1);
        }
        eprintln!("✓ wrote {}", path.display());
    }

    // The design tokens → CSS custom properties (#115): the DSL palette becomes `:root { --… }` so
    // the hand-written base stylesheet (crates/web/assets/app.css) can consume `var(--color-primary)`
    // etc. Written under crates/web/assets so the renderer `include_str!`s it; drift-gated.
    let assets = repo_root(&specs).join("crates/web/assets");
    if let Err(e) = fs::create_dir_all(&assets) {
        eprintln!("✗ create {}: {}", assets.display(), e);
        std::process::exit(1);
    }
    let css_path = assets.join("tokens.generated.css");
    if let Err(e) = fs::write(&css_path, emit_web_tokens_css(&model)) {
        eprintln!("✗ write {}: {}", css_path.display(), e);
        std::process::exit(1);
    }
    eprintln!("✓ wrote {}", css_path.display());

    // specs/generated/apps.generated.md — THE APP INDEX (#491, PROP-20260811-141654 slice A1).
    // LAST on purpose, and the only artifact that MEASURES rather than derives: its resolved
    // column comes from cargo's own resolver over the workspace, so it must run AFTER the domain
    // and bin crate manifests this same pass writes — otherwise a run that adds a deployable
    // would render the graph as it stood before that deployable existed, and only the NEXT run
    // would agree with itself (a two-pass instability check-drift catches one commit too late).
    let root = repo_root(&specs);
    match measure_workspace_crate_graph(&root) {
        Ok(graph) => {
            let path = out_dir.join("apps.generated.md");
            if let Err(e) = fs::write(&path, emit_app_index(&model, &graph)) {
                eprintln!("✗ write {}: {}", path.display(), e);
                std::process::exit(1);
            }
            eprintln!("✓ wrote {}", path.display());
        }
        Err(e) => {
            // Refuse rather than emit an index whose central column is a guess: a resolved set
            // that silently fell back to the declared one would report a clean split that is not.
            eprintln!("✗ app index: cannot measure the workspace crate graph: {e}");
            std::process::exit(1);
        }
    }
}
