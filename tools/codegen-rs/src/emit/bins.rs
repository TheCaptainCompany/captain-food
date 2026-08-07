//! Emit the PER-DEPLOYABLE BIN CRATES under `crates/bins/` — realization step (3) of
//! ADR-20260807-183024 (#382), on top of the #375 spec reorg and the #373 per-scope domain
//! crates. One binary crate per c4-l2 deployable: `actor-{type}` / `pm-{name}` (mailbox
//! workers), `projector-{scope}` (D4), `graphql-{scope}` subgraphs + `gateway-{role}` role
//! gateways (D8), the `fo-*`/`bo-*`/`adapters` surface bins and the `bam` worker
//! (PROP-20260806-223656 §2b D5 addendum container list).
//!
//! WHAT THE SPLIT BUYS. Each bin's `Cargo.toml` links ONLY the domain-scope crates its entry in
//! the derived crate graph declares — the manifest IS the lane/scope assertion (compiler-first,
//! ADR-20260803-234035): `actor-order` cannot spell a catalog type, a gateway cannot spell any
//! domain type at all, and a new cross-scope edge lands as a reviewable manifest diff, never an
//! import nobody notices. This removes step (2)'s recorded limit for the bins: the monolith
//! consumers still couple to every scope through the `domain` facade, but the deployables no
//! longer do.
//!
//! WHAT THE BINS ARE (and are not) AT THIS STEP — gate-then-stabilize: SKELETONS. Each `main`
//! prints its identity and exits; the monolith `server` bin remains the deployed production
//! runtime until #349 (manifests/images emitter) and #358 (MKS cutover) flip deployment as their
//! own recorded steps. What is REAL now is the topology (buildable, workspace-membered, pruned
//! when the spec drops a deployable) and the scope containment (compile-checked via
//! `use … as _;` so the linker cannot silently strip an asserted dependency).
//!
//! DERIVATION — one source per family, cross-checked against `architecture/c4-l2.yaml` by the
//! §15 validator (`c4-bin-*` rules) so the container list and the emitted crates cannot drift:
//! actor/PM bins from `actors.yaml`/`processmanager.yaml` (deps = `actor_scope_links`, the PM
//! bridge doctrine made load-bearing); projector/subgraph bins from the declared scopes (kernel
//! gets a subgraph but no projector — it owns no `View_*`); gateway bins from the `UserType`
//! role paths (role = path, ADR-0006); surface bins and `bam` from the c4-l2 container list
//! itself (their existence is a deploy-topology decision, not derivable from any other spec).

use crate::*;

/// One deployable in the derived bin topology.
pub(crate) struct BinSpec {
    /// Bin/crate/container name, e.g. `actor-order`, `projector-catalog`, `gateway-public`.
    pub(crate) name: String,
    /// Family: `actor` | `pm` | `projector` | `subgraph` | `gateway` | `surface` | `worker`.
    pub(crate) family: &'static str,
    /// The realized actor/PM (actor + pm families).
    pub(crate) actor: Option<String>,
    /// The owning scope (projector + subgraph families).
    pub(crate) scope: Option<String>,
    /// The served UserType role (gateway family).
    pub(crate) role: Option<String>,
    /// Domain-scope crates the bin's manifest links, by scope name (⊆ emitted scope crates).
    pub(crate) domain_scopes: BTreeSet<String>,
}

/// `RESTAURANT_ACCOUNT` → `restaurant-account`: the role's path segment (`/{path}/graphql`,
/// ADR-0006 role-as-path) and its gateway bin suffix.
pub(crate) fn role_path(role: &str) -> String {
    role.to_ascii_lowercase().replace('_', "-")
}

/// The UserType role values, in enum order (each is a served `/{path}/graphql` role path).
pub(crate) fn user_type_roles(model: &Model) -> Vec<String> {
    model
        .defs
        .get("scalars.yaml")
        .and_then(|s| s.get("UserType"))
        .and_then(|u| u.get("enum"))
        .and_then(|e| e.as_sequence())
        .map(|s| s.iter().filter_map(|v| v.as_str().map(|x| x.to_string())).collect())
        .unwrap_or_default()
}

/// The c4-l2 container ids that are SURFACE bins (`fo-*`, `bo-*`, `adapters`) or the `bam`
/// worker. These two families exist only in the deploy topology — the container list is their
/// source of truth, so they are read from it rather than re-derived.
fn c4_surface_and_worker_bins(model: &Model) -> (Vec<String>, bool) {
    let mut surfaces = Vec::new();
    let mut bam = false;
    for c in read_c4(model).containers {
        if c.id.starts_with("fo-") || c.id.starts_with("bo-") || c.id == "adapters" {
            surfaces.push(c.id);
        } else if c.id == "bam" {
            bam = true;
        }
    }
    surfaces.sort();
    (surfaces, bam)
}

/// The full derived bin topology, sorted by name (stable output). Empty on a model with no
/// emitted scope crates (flat-layout fixtures) — the guard that keeps degenerate models from
/// mass-emitting or mass-pruning, same posture as the domain-scope emitter.
pub(crate) fn bin_topology(model: &Model) -> Vec<BinSpec> {
    // `crate_scopes`, not `emit_domain_scope_crates`: this runs on VALIDATION paths (§15), where
    // generating code from a minimal fixture model would panic — deciding which crates exist
    // must stay side-effect-free and total.
    let emitted: BTreeSet<String> = crate_scopes(model);
    if emitted.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<BinSpec> = Vec::new();
    // actor-* / pm-*: deps are the spec-declared reach (the PM's refs ARE its dependency list).
    for (actor, is_pm, scopes) in actor_scope_links(model) {
        out.push(BinSpec {
            name: actor_bin_name(&actor, is_pm),
            family: if is_pm { "pm" } else { "actor" },
            actor: Some(actor),
            scope: None,
            role: None,
            domain_scopes: scopes.intersection(&emitted).cloned().collect(),
        });
    }
    // projector-{scope}: every non-kernel scope projects its own views schema (D4). The kernel
    // owns no View_* (its subgraph serves the write-path journals), so it gets none.
    // graphql-{scope}: every scope, kernel included (D8).
    for scope in &model.scopes {
        if !emitted.contains(scope) {
            continue;
        }
        if scope != KERNEL_SCOPE {
            out.push(BinSpec {
                name: format!("projector-{}", scope),
                family: "projector",
                actor: None,
                scope: Some(scope.clone()),
                role: None,
                domain_scopes: BTreeSet::from([scope.clone()]),
            });
        }
        out.push(BinSpec {
            name: format!("graphql-{}", scope),
            family: "subgraph",
            actor: None,
            scope: Some(scope.clone()),
            role: None,
            domain_scopes: BTreeSet::from([scope.clone()]),
        });
    }
    // gateway-{role}: thin generated routing per role path — NO domain crates, no DB, no state
    // (D8: composition happens in the projector, the gateway only routes top-level fields).
    for role in user_type_roles(model) {
        out.push(BinSpec {
            name: format!("gateway-{}", role_path(&role)),
            family: "gateway",
            actor: None,
            scope: None,
            role: Some(role),
            domain_scopes: BTreeSet::new(),
        });
    }
    // Surface bins (assets/SSR/webhooks — speak to their role gateway, hold no domain
    // vocabulary) and the bam worker (cross-scope consumer BY DESIGN: it folds every scope's
    // events into business-activity views, so it links every scope crate).
    let (surfaces, bam) = c4_surface_and_worker_bins(model);
    for s in surfaces {
        out.push(BinSpec {
            name: s,
            family: "surface",
            actor: None,
            scope: None,
            role: None,
            domain_scopes: BTreeSet::new(),
        });
    }
    if bam {
        out.push(BinSpec {
            name: "bam".to_string(),
            family: "worker",
            actor: None,
            scope: None,
            role: None,
            domain_scopes: emitted.clone(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// One generated bin crate: directory, manifest, `src/main.rs`.
pub(crate) struct BinCrate {
    pub(crate) name: String,
    /// Directory relative to the repo root, e.g. `crates/bins/actor-order`.
    pub(crate) dir: String,
    pub(crate) manifest: String,
    pub(crate) main: String,
}

/// One line of purpose per family, for the manifest description and the main.rs doc.
fn family_purpose(b: &BinSpec) -> String {
    match b.family {
        "actor" => format!(
            "mailbox worker realizing the `{}` aggregate -- drains ONLY its own lanes (the scoping is the linker)",
            b.actor.as_deref().unwrap_or("?")
        ),
        "pm" => format!(
            "mailbox worker realizing the `{}` process manager -- a DECLARED cross-scope bridge; its domain-crate links are its spec refs",
            b.actor.as_deref().unwrap_or("?")
        ),
        "projector" => format!(
            "projection worker for the `{}` scope -- consumes the single log filtered to its scope's events, maintains only its schema's View_*, own checkpoint (D4)",
            b.scope.as_deref().unwrap_or("?")
        ),
        "subgraph" => format!(
            "GraphQL subgraph for the `{}` scope -- one domain, one graph, one GRANT (D8)",
            b.scope.as_deref().unwrap_or("?")
        ),
        "gateway" => format!(
            "role gateway for /{}/graphql -- thin generated top-level routing, no DB access, no business logic, no state (D8)",
            b.role.as_deref().map(role_path).unwrap_or_default()
        ),
        "surface" => "surface bin -- assets/SSR/webhooks; speaks to its role gateway, holds no domain vocabulary and no broad views access".to_string(),
        _ => "business-activity projector -- a cross-scope consumer BY DESIGN (it folds every scope's events)".to_string(),
    }
}

/// The manifest of one bin crate. `[dependencies]` is the bin's SCOPE ASSERTION — exactly the
/// crate-graph entry, nothing else (no sqlx, no reqwest, no framework deps until the runtime
/// wiring lands with #349/#358).
fn bin_manifest(b: &BinSpec) -> String {
    let mut deps = String::new();
    for s in &b.domain_scopes {
        deps.push_str(&format!("{} = {{ path = \"../../domains/{}\" }}\n", domain_crate_name(s), s));
    }
    if deps.is_empty() {
        deps.push_str(
            "# (none -- this family holds NO domain vocabulary; an added domain crate here is a\n#  boundary violation to review, not a convenience)\n",
        );
    }
    format!(
        r#"# GENERATED by the Captain.Food codegen -- do not edit by hand
# (ADR-20260807-183024 step 3, #382; container list per PROP-20260806-223656 s2b D5 addendum).
#
# PER-DEPLOYABLE BIN CRATE: `{name}` ({family}). The [dependencies] below are this bin's SCOPE
# ASSERTION, copied from its entry in specs/generated/crate-graph.generated.json -- linking a
# domain crate is the ONLY way that scope's vocabulary exists in this deployable, so the wrong
# coupling is unspellable rather than merely unrouted (compiler-first, ADR-20260803-234035).
#
# SKELETON (gate-then-stabilize): the monolith `server` bin remains the deployed runtime until
# #349 (manifests/images emitter) and #358 (MKS cutover) flip deployment.
[package]
name = "{name}"
version = "0.1.0"
edition.workspace = true
license-file.workspace = true
publish = false
description = "Captain.Food `{name}` deployable: {desc} (ADR-20260807-183024 step 3)."

[dependencies]
{deps}
# D6 lint floor (PROP-20260802-130500, #302): inherit the workspace [lints] baseline.
[lints]
workspace = true
"#,
        name = b.name,
        family = b.family,
        desc = family_purpose(b),
        deps = deps,
    )
}

/// The `src/main.rs` of one bin crate: the identity constants and the skeleton entrypoint. The
/// `use … as _;` lines exist so every manifest-declared domain link is a COMPILE-CHECKED fact —
/// a dependency the source never names is an assertion nobody verifies (and cargo-machete's D6
/// gate would rightly flag it).
fn bin_main(b: &BinSpec) -> String {
    let mut uses = String::new();
    if !b.domain_scopes.is_empty() {
        uses.push_str(
            "// The manifest is the scope assertion; these imports make each declared domain-crate\n// link a compile-checked fact (the linker cannot silently strip it).\n",
        );
        for s in &b.domain_scopes {
            uses.push_str(&format!("use {} as _;\n", domain_crate_ident(s)));
        }
        uses.push('\n');
    }
    format!(
        r#"// GENERATED by the Captain.Food codegen from the derived bin topology — do not edit by hand
// (ADR-20260807-183024 step 3, #382).

//! `{name}` — {desc}.
//!
//! SKELETON (gate-then-stabilize): this bin exists so the deploy topology is buildable and its
//! scope containment is compiler-checked; the monolith `server` bin remains the deployed
//! production runtime until #349 (manifests/images emitter) and #358 (MKS cutover) flip
//! deployment as their own recorded steps.

{uses}/// The c4-l2 container this binary realizes.
const BIN: &str = "{name}";
/// The bin's family in the deploy topology.
const FAMILY: &str = "{family}";

fn main() {{
    println!(
        "{{BIN}} ({{FAMILY}}): bin skeleton (ADR-20260807-183024 step 3) — runtime wiring lands \
         with #349/#358; the monolith server remains the deployed runtime."
    );
}}
"#,
        name = b.name,
        desc = family_purpose(b),
        uses = uses,
        family = b.family,
    )
}

/// Emit every bin crate from the derived topology.
pub(crate) fn emit_bin_crates(model: &Model) -> Vec<BinCrate> {
    bin_topology(model)
        .iter()
        .map(|b| BinCrate {
            name: b.name.clone(),
            dir: format!("crates/bins/{}", b.name),
            manifest: bin_manifest(b),
            main: bin_main(b),
        })
        .collect()
}
