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
//! WHAT THE BINS ARE (and are not) AT THIS STEP — gate-then-stabilize: PROBE-SERVING SHELLS
//! (#349, step 4 — upgraded from step 3's print-and-exit skeletons). Each `main` binds `$PORT`
//! and serves the `/health` + `/ping` probes its generated Deployment (deploy/generated/)
//! declares, draining on SIGTERM — a viable pod, not yet the business runtime. The monolith
//! `server` bin remains the deployed production runtime; the business wiring (actor bins hosting
//! their mailbox worker, per-scope projection filtering, subgraph schema slices, gateway
//! composition tables) is tracked on #349 and blocks the steps (6)–(7) flip. What is REAL now is
//! the topology (buildable, workspace-membered, pruned when the spec drops a deployable), the
//! scope containment (compile-checked via `use … as _;` so the linker cannot silently strip an
//! asserted dependency), and the probe contract the manifests point at.
//!
//! DERIVATION — one source per family, cross-checked against `architecture/c4-l2.yaml` by the
//! §15 validator (`c4-bin-*` rules) so the container list and the emitted crates cannot drift:
//! actor/PM bins from `actors.yaml`/`processmanager.yaml` (deps = `actor_scope_links`, the PM
//! bridge doctrine made load-bearing); projector/subgraph bins from the declared scopes (kernel
//! gets a subgraph but no projector — it owns no `View_*`); gateway bins from the `UserType`
//! role paths (role = path, ADR-0006); surface bins and `bam` from the c4-l2 container list
//! itself (their existence is a deploy-topology decision, not derivable from any other spec).
//! Projector/subgraph bins link ONLY their scope's crate — DELIBERATELY not `domain-common`:
//! today's skeletons name no kernel type, and the assertion stays minimal so that when the
//! runtime wiring (#349+) legitimately reaches kernel value objects (Money in a subgraph's
//! outputs, say), the `domain-common` line lands as a LOUD manifest diff at that step, not as a
//! pre-granted convenience nobody reviews.

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
    /// The outbound service ports the realized actor/PM DECLARES (`ports:` in actors.yaml /
    /// processmanager.yaml, union across both defs) — what decides which integration-adapter
    /// crates the wired main links (#385): `payment` ⇒ the Stripe adapter, `delivery` ⇒ the
    /// delivery-partner composite. Derived, never a hand-curated money-bin list.
    pub(crate) ports: BTreeSet<String>,
    /// The realized actor declares a durable mailbox (`mailbox.partitions`) — its bin hosts the
    /// lane's worker fleet. All 15 aggregates and the two money PMs; the three state-table PMs
    /// host only the restricted saga runner.
    pub(crate) mailboxed: bool,
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

/// The declared `ports:` and `mailbox:` of one actor/PM, unioned across its actors.yaml and
/// processmanager.yaml defs (a PM may carry orchestration in one and the mailbox in the other).
fn actor_meta(model: &Model, actor: &str) -> (BTreeSet<String>, bool) {
    let mut ports = BTreeSet::new();
    let mut mailboxed = false;
    for file in ["actors.yaml", "processmanager.yaml"] {
        let Some(def) = model.defs.get(file).and_then(|d| d.get(actor)) else { continue };
        if let Some(p) = def.get("ports").and_then(|p| p.as_mapping()) {
            for (k, _) in p {
                if let Some(k) = k.as_str() {
                    ports.insert(k.to_string());
                }
            }
        }
        if def.get("mailbox").and_then(|m| m.get("partitions")).is_some() {
            mailboxed = true;
        }
    }
    (ports, mailboxed)
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
        let (ports, mailboxed) = actor_meta(model, &actor);
        out.push(BinSpec {
            name: actor_bin_name(&actor, is_pm),
            family: if is_pm { "pm" } else { "actor" },
            actor: Some(actor),
            scope: None,
            role: None,
            domain_scopes: scopes.intersection(&emitted).cloned().collect(),
            ports,
            mailboxed,
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
                ports: BTreeSet::new(),
                mailboxed: false,
            });
        }
        out.push(BinSpec {
            name: format!("graphql-{}", scope),
            family: "subgraph",
            actor: None,
            scope: Some(scope.clone()),
            role: None,
            domain_scopes: BTreeSet::from([scope.clone()]),
            ports: BTreeSet::new(),
            mailboxed: false,
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
            ports: BTreeSet::new(),
            mailboxed: false,
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
            ports: BTreeSet::new(),
            mailboxed: false,
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
            ports: BTreeSet::new(),
            mailboxed: false,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// One generated bin crate: directory, manifest, `src/main.rs` (+ `src/config.rs` when wired).
pub(crate) struct BinCrate {
    pub(crate) name: String,
    /// Directory relative to the repo root, e.g. `crates/bins/actor-order`.
    pub(crate) dir: String,
    pub(crate) manifest: String,
    pub(crate) main: String,
    /// The scope-filtered generated Config reader (#374 Q4) — `Some` for wired families only.
    pub(crate) config: Option<String>,
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
        "worker" => "business-activity projector -- a cross-scope consumer BY DESIGN (it folds every scope's events)".to_string(),
        f => unreachable!("unknown bin family '{f}' — every family added to bin_topology needs its purpose line here"),
    }
}

/// Is this family WIRED as a business runtime? ALL of them since the #385 API-tier wiring:
/// the CQRS spine (actor/pm/projector, PR #387), the subgraph slices + role gateways +
/// surfaces + bam (this change). The shell template below survives only as the fallback for a
/// family the topology grows before its wiring lands.
pub(crate) fn wired(b: &BinSpec) -> bool {
    matches!(
        b.family,
        "actor" | "pm" | "projector" | "subgraph" | "gateway" | "surface" | "worker"
    )
}

/// The manifest of one bin crate. The domain `[dependencies]` are the bin's SCOPE ASSERTION —
/// exactly the crate-graph entry. WIRED families (#385) additionally link the composition kit
/// (`bin_runtime`) and — ONLY where the spec declares the port — the matching integration
/// adapters; shell families keep the step-4 probe runtime (axum + tokio) and nothing else.
fn bin_manifest(b: &BinSpec) -> String {
    let mut deps = String::new();
    for s in &b.domain_scopes {
        deps.push_str(&format!("{} = {{ path = \"../../domains/{}\" }}\n", domain_crate_name(s), s));
    }
    if deps.is_empty() {
        deps.push_str(
            "# (no domain crates -- this family holds NO domain vocabulary; an added domain crate\n#  here is a boundary violation to review, not a convenience)\n",
        );
    }
    if wired(b) {
        let common = "tokio = { workspace = true }\nserde_json = { workspace = true }\ntracing = \"0.1\"\n# The generated scope-filtered Config reader (src/config.rs, #374 Q4) validates values\n# against their scalars' regexes at startup.\nregex = \"1\"\n";
        match b.family {
            "actor" | "pm" | "projector" | "worker" => {
                deps.push_str(
                    "# The business runtime (#385): the shared composition kit -- probe server, telemetry,\n# declared-size pool, and the family spawn helpers over application/infrastructure.\nbin_runtime = { path = \"../../bin_runtime\" }\n",
                );
                deps.push_str(common);
            }
            "subgraph" => {
                deps.push_str(
                    "# SUBGRAPH runtime (#385 API-tier wiring, D8): the monolith's GraphQL surface as a\n# library -- same generated type layer, same resolvers over the same adapters\n# (server::build_graphql_di), restricted to THIS scope by the scope-slice extension over the\n# generated composition table. The GRANT-scoped views role (#360) adds the storage wall later.\nserver = { path = \"../../server\" }\nbin_runtime = { path = \"../../bin_runtime\" }\n",
                );
                deps.push_str(common);
            }
            "gateway" => {
                deps.push_str(
                    "# GATEWAY runtime (#385 API-tier wiring, D8): top-level-field routing to the owning\n# subgraphs from the embedded composition table. DELIBERATELY no server, no infrastructure,\n# no database -- auth lives at the subgraph (the schema boundary); a domain crate appearing\n# here is a boundary violation to review.\ngateway_runtime = { path = \"../../gateway_runtime\" }\nbin_probes = { path = \"../../bin_probes\" }\n",
                );
                deps.push_str(common);
            }
            "surface" if b.name == "adapters" => {
                deps.push_str(
                    "# The ADAPTERS surface (#385): re-hosts the partner webhook ingestors the monolith\n# mounts (stripe/avelo37/coopcycle/uber_direct/hubrise) -- verify, mirror, ACL, ENQUEUE on\n# the shared mailbox; the owning actor bins deliver (woken by the INSERT's pg_notify).\nbin_runtime = { path = \"../../bin_runtime\" }\ninfrastructure = { path = \"../../infrastructure\" }\nactor_client = { path = \"../../actor_client\" }\nstripe-adapter = { path = \"../../adapters/stripe\" }\navelo37-adapter = { path = \"../../adapters/avelo37\" }\ncoopcycle-adapter = { path = \"../../adapters/coopcycle\" }\nuber-direct-adapter = { path = \"../../adapters/uber_direct\" }\nhubrise-adapter = { path = \"../../adapters/hubrise\" }\n",
                );
                deps.push_str(common);
            }
            "surface" => {
                deps.push_str(
                    "# SURFACE runtime (#385 API-tier wiring, D8): audience host serving + SSR of the\n# generated screens THROUGH THE ROLE GATEWAY + the wasm /assets dir. DELIBERATELY no\n# database, no server, no infrastructure -- a surface reads only over GraphQL.\nsurface_runtime = { path = \"../../surface_runtime\" }\nbin_probes = { path = \"../../bin_probes\" }\n",
                );
                deps.push_str(common);
            }
            f => unreachable!("wired manifest for unknown family '{f}'"),
        }
        if b.ports.contains("payment") {
            deps.push_str(
                "# DECLARED `payment` port (actors/processmanager.yaml `ports:`): this host drives Stripe.\nstripe-adapter = { path = \"../../adapters/stripe\" }\n",
            );
        }
        if b.ports.contains("delivery") {
            deps.push_str(
                "# DECLARED `delivery` port: the composite delivery-partner gateway and its channel adapters.\napplication = { path = \"../../application\" }\ninfrastructure = { path = \"../../infrastructure\" }\navelo37-adapter = { path = \"../../adapters/avelo37\" }\ncoopcycle-adapter = { path = \"../../adapters/coopcycle\" }\nuber-direct-adapter = { path = \"../../adapters/uber_direct\" }\n",
            );
        }
    } else {
        deps.push_str(
            "# The probe shell (step 4, #349): serves the /health + /ping the generated Deployment probes.\naxum = { workspace = true }\ntokio = { workspace = true }\n",
        );
    }
    let posture = if wired(b) {
        "# BUSINESS RUNTIME (#385, gate-then-stabilize): runnable end-to-end, but the monolith\n# `server` bin remains the DEPLOYED runtime until ADR-20260807-183024 steps (6)-(7) (#358\n# cutover) point traffic and the mailbox at these pods."
    } else {
        "# PROBE-SERVING SHELL (gate-then-stabilize): the monolith `server` bin remains the deployed\n# runtime; this family's business wiring is the recorded remainder on #385."
    };
    format!(
        r#"# GENERATED by the Captain.Food codegen -- do not edit by hand
# (ADR-20260807-183024 steps 3+4 and #385; container list per PROP-20260806-223656 s2b D5 addendum).
#
# PER-DEPLOYABLE BIN CRATE: `{name}` ({family}). The domain [dependencies] below are this bin's
# SCOPE ASSERTION, copied from its entry in specs/generated/crate-graph.generated.json -- linking
# a domain crate is the ONLY way that scope's vocabulary exists in this deployable, so the wrong
# coupling is unspellable rather than merely unrouted (compiler-first, ADR-20260803-234035).
#
{posture}
[package]
name = "{name}"
version = "0.1.0"
edition.workspace = true
license-file.workspace = true
publish = false
description = "Captain.Food `{name}` deployable: {desc} (ADR-20260807-183024)."

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
        posture = posture,
    )
}

/// The `src/main.rs` of a WIRED bin (#385): a thin parameter list over `bin_runtime` — config
/// gate, telemetry, pool, the family spawn, the probe server. No business logic: every helper
/// assembles existing application/infrastructure code (dispatch non-negotiable (a)).
fn wired_main(b: &BinSpec) -> String {
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
    let hosting = match b.family {
        "actor" => format!(
            "hosts the `{}` mailbox lane's supervised worker fleet (the SAME `infrastructure::mailbox::standalone` runtime the adapter binaries use: leases, fencing, head-of-line, posture-gated money lanes, graceful drain)",
            b.actor.as_deref().unwrap_or("?")
        ),
        "pm" if b.mailboxed => format!(
            "hosts the `{}` saga runner (restricted to this PM, flip-time backfill sequenced first) AND its mailbox lane's worker fleet",
            b.actor.as_deref().unwrap_or("?")
        ),
        "pm" => format!(
            "hosts the `{}` saga runner, restricted to this PM over its own `pm:` checkpoint",
            b.actor.as_deref().unwrap_or("?")
        ),
        "projector" => format!(
            "hosts the `{}`-scope projection worker: the shared registry filtered to this scope's groups, on their shared per-group checkpoints (monolith handover needs no re-projection)",
            b.scope.as_deref().unwrap_or("?")
        ),
        f => unreachable!("wired_main called for unwired family '{f}'"),
    };
    let payments_expr = if b.ports.contains("payment") {
        "bin_runtime::payment_binding(|key| {\n            std::sync::Arc::new(stripe_adapter::StripePaymentGateway::new(key))\n        })"
    } else {
        "bin_runtime::fail_closed_payments()"
    };
    let body = match b.family {
        "actor" | "pm" => {
            let mut body = String::new();
            body.push_str(
                "    // Without a database this runtime cannot host its slice: stay a viable pod that says\n    // so (readiness 503) rather than crash-loop -- the config gate already stopped prod/staging.\n    let db_ready = !config.database_url.is_empty();\n",
            );
            if b.family == "pm" {
                body.push_str("    let mut saga_status = None;\n");
            }
            body.push_str("    if db_ready {\n        let pool = bin_runtime::pg_pool(&config.database_url, config.database_pool_max_connections)\n            .unwrap_or_else(|e| panic!(\"{BIN}: pg pool init: {e}\"));\n");
            if b.family == "pm" {
                let pm = b.actor.as_deref().unwrap_or("?");
                let lane = if b.mailboxed { format!("Some(\"{pm}\")") } else { "None".to_string() };
                let partner = if b.ports.contains("delivery") {
                    "Some(delivery_partner_binding())"
                } else {
                    "None"
                };
                let payments_opt = if b.ports.contains("payment") {
                    format!("let payments = {payments_expr};\n        ")
                } else {
                    String::new()
                };
                let payments_field = if b.ports.contains("payment") {
                    "Some(payments.clone())"
                } else {
                    "None"
                };
                body.push_str(&format!(
                    "        {payments_opt}let waiter = bin_runtime::event_waiter(&config.database_url, config.run_event_push);\n        let (status, posture) = bin_runtime::spawn_pm_runtime(bin_runtime::PmRuntime {{\n            pool: pool.clone(),\n            bin: BIN,\n            pm: PM,\n            mailbox_lane: {lane},\n            partner: {partner},\n            payments: {payments_field},\n            waiter,\n        }})\n        .await;\n        tracing::info!(pm = PM, posture = ?posture, \"saga runner spawned (restricted to this PM)\");\n        saga_status = Some(status);\n",
                ));
                if b.mailboxed {
                    body.push_str(
                        "        // The PM's OWN mailbox lane (Runtime D1): the gate-on delivery path. The fleet\n        // reads the money posture itself and refuses the lane when it is unprovable.\n        bin_runtime::spawn_actor_fleet(\n            pool.clone(),\n            BIN,\n            LANES,\n            payments,\n            config.reminder_windows(),\n            bin_runtime::MailboxSettings {\n                lease_seconds: config.mailbox_lease_seconds,\n                heartbeat_seconds: config.mailbox_heartbeat_seconds,\n                max_delivery_attempts: config.mailbox_max_delivery_attempts,\n            },\n        );\n",
                    );
                }
                if b.ports.contains("delivery") {
                    body.push_str(
                        "        // Delivery offer-timeout worker (#60): escalates a stale OFFERED run to the next\n        // ranked channel -- it belongs to the dispatch PM, so its bin hosts it.\n        if config.run_delivery_offer_timeout {\n            let timeout_worker = std::sync::Arc::new(infrastructure::DeliveryOfferTimeoutWorker::new(\n                pool.clone(),\n                config.delivery_offer_max_ttl_seconds,\n            ));\n            tokio::spawn(timeout_worker.run_loop());\n            tracing::info!(worker = \"delivery_offer_timeout\", running = true, toggle = \"RUN_DELIVERY_OFFER_TIMEOUT\", \"worker running\");\n        } else {\n            tracing::warn!(worker = \"delivery_offer_timeout\", running = false, toggle = \"RUN_DELIVERY_OFFER_TIMEOUT\", \"worker NOT started -- an unanswered offer is never expired\");\n        }\n",
                    );
                }
            } else {
                body.push_str(&format!(
                    "        bin_runtime::spawn_actor_fleet(\n            pool,\n            BIN,\n            LANES,\n            {payments_expr},\n            config.reminder_windows(),\n            bin_runtime::MailboxSettings {{\n                lease_seconds: config.mailbox_lease_seconds,\n                heartbeat_seconds: config.mailbox_heartbeat_seconds,\n                max_delivery_attempts: config.mailbox_max_delivery_attempts,\n            }},\n        );\n",
                ));
            }
            body.push_str("    } else {\n        tracing::warn!(bin = BIN, \"DATABASE_URL unset -- no workers hosted, readiness stays 503\");\n    }\n");
            match b.family {
                "pm" => {
                    let lanes_line = if b.mailboxed {
                        "                obj.insert(\"lanes\".into(), serde_json::json!(LANES));\n"
                    } else {
                        ""
                    };
                    body.push_str(&format!(
                        "    // Readiness = the restricted saga runner is running; the body carries its\n    // checkpoint/head/lag snapshot (the monolith's /saga shape) plus the hosted PM.\n    let health: bin_runtime::HealthFn = std::sync::Arc::new(move || match &saga_status {{\n        Some(handle) => {{\n            let st = handle.lock().unwrap_or_else(|p| p.into_inner()).clone();\n            let running = st.running;\n            let mut body = serde_json::to_value(&st).unwrap_or_else(|_| serde_json::json!({{ \"running\": false }}));\n            if let Some(obj) = body.as_object_mut() {{\n                obj.insert(\"pm\".into(), serde_json::json!(PM));\n{lanes_line}            }}\n            (running, body)\n        }}\n        None => (false, serde_json::json!({{ \"running\": false, \"reason\": \"database_not_configured\" }})),\n    }});\n",
                    ));
                }
                _ => body.push_str(
                    "    // Readiness = the fleet's host has its database; the fleet supervises itself (respawn\n    // with backoff) and reports per-lane state through the mailbox rows, not this probe.\n    let health: bin_runtime::HealthFn = std::sync::Arc::new(move || {\n        (\n            db_ready,\n            serde_json::json!({\n                \"lanes\": LANES,\n                \"db\": if db_ready { \"configured\" } else { \"not_configured\" },\n            }),\n        )\n    });\n",
                ),
            }
            body
        }
        "projector" => {
            let mut body = String::new();
            body.push_str(
                "    // Without a database this runtime cannot project: stay a viable pod that says so\n    // (readiness 503) rather than crash-loop -- the config gate already stopped prod/staging.\n    let db_ready = !config.database_url.is_empty();\n    let mut proj_status = None;\n    if db_ready {\n        let pool = bin_runtime::pg_pool(&config.database_url, config.database_pool_max_connections)\n            .unwrap_or_else(|e| panic!(\"{BIN}: pg pool init: {e}\"));\n        let waiter = bin_runtime::event_waiter(&config.database_url, config.run_event_push);\n        proj_status = Some(bin_runtime::spawn_scope_projector(pool, SCOPE, waiter));\n    } else {\n        tracing::warn!(bin = BIN, \"DATABASE_URL unset -- no projection worker hosted, readiness stays 503\");\n    }\n",
            );
            body.push_str(
                "    // Readiness = the scoped worker is running; the body carries its checkpoint/head/lag\n    // snapshot (the monolith's /projector shape) plus the owned scope.\n    let health: bin_runtime::HealthFn = std::sync::Arc::new(move || match &proj_status {\n        Some(handle) => {\n            let st = handle.lock().unwrap_or_else(|p| p.into_inner()).clone();\n            let running = st.running;\n            let mut body = serde_json::to_value(&st).unwrap_or_else(|_| serde_json::json!({ \"running\": false }));\n            if let Some(obj) = body.as_object_mut() {\n                obj.insert(\"scope\".into(), serde_json::json!(SCOPE));\n            }\n            (running, body)\n        }\n        None => (false, serde_json::json!({ \"running\": false, \"reason\": \"database_not_configured\" })),\n    });\n",
            );
            body
        }
        f => unreachable!("wired_main body for unwired family '{f}'"),
    };
    let mut consts = format!(
        "/// The c4-l2 container this binary realizes.\nconst BIN: &str = \"{}\";\n/// The bin's family in the deploy topology.\nconst FAMILY: &str = \"{}\";\n",
        b.name, b.family
    );
    match b.family {
        "actor" => consts.push_str(&format!(
            "/// The mailbox lane(s) this deployable drains -- the scoping is the linker AND this list.\nconst LANES: &[&str] = &[\"{}\"];\n",
            b.actor.as_deref().unwrap_or("?")
        )),
        "pm" => {
            consts.push_str(&format!(
                "/// The process manager this deployable realizes (its `pm:` checkpoint slice).\nconst PM: &str = \"{}\";\n",
                b.actor.as_deref().unwrap_or("?")
            ));
            if b.mailboxed {
                consts.push_str(&format!(
                    "/// The PM's own mailbox lane (Runtime D1 gate-on delivery path).\nconst LANES: &[&str] = &[\"{}\"];\n",
                    b.actor.as_deref().unwrap_or("?")
                ));
            }
        }
        "projector" => consts.push_str(&format!(
            "/// The scope whose projection groups this deployable drains (D4).\nconst SCOPE: &str = \"{}\";\n",
            b.scope.as_deref().unwrap_or("?")
        )),
        _ => {}
    }
    let partner_fn = if b.ports.contains("delivery") {
        "\n/// The composite delivery gateway (#60), resolved through the GENERATED `delivery` service\n/// binding -- the SAME construction the monolith composition root performs: `independent` is the\n/// rider pool (deliberate no-op), partner channels join when their credentials are configured,\n/// and an unwired channel falls through via offer timeout.\nfn delivery_partner_binding() -> std::sync::Arc<dyn application::generated::services::DeliveryService> {\n    infrastructure::generated::service_bindings::delivery_service(|| {\n        let mut gateway = infrastructure::CompositeDeliveryGateway::new().with_channel(\n            \"independent\",\n            std::sync::Arc::new(application::ports::NoopDeliveryService),\n        );\n        if let Some(avelo) = avelo37_adapter::Avelo37DeliveryGateway::from_env() {\n            gateway = gateway.with_channel(\"avelo37\", std::sync::Arc::new(avelo));\n        }\n        let coopcycle_registry = coopcycle_adapter::CoopCycleRegistry::from_env()\n            .unwrap_or_else(|e| {\n                tracing::warn!(error = %e, key = \"COOPCYCLE_INSTANCES\", \"misconfigured, treating as unset\");\n                None\n            })\n            .unwrap_or_default();\n        if !coopcycle_registry.is_empty() {\n            gateway = gateway.with_channel(\n                \"coopcycle\",\n                std::sync::Arc::new(coopcycle_adapter::CoopCycleDeliveryGateway::new(coopcycle_registry)),\n            );\n        }\n        if let Some(config) = uber_direct_adapter::UberDirectConfig::from_env().unwrap_or_else(|e| {\n            tracing::warn!(error = %e, key = \"UBER_DIRECT_*\", \"misconfigured, treating as unset\");\n            None\n        }) {\n            gateway = gateway.with_channel(\n                \"uber_direct\",\n                std::sync::Arc::new(uber_direct_adapter::UberDirectDeliveryGateway::new(config)),\n            );\n        }\n        tracing::info!(\n            binding = \"delivery\",\n            impl_ = \"composite\",\n            wired_channels = ?gateway.wired_channels(),\n            \"delivery gateway wired (unwired channels fall through via offer timeout)\"\n        );\n        std::sync::Arc::new(gateway)\n    })\n    .expect(\"delivery service binding (services.yaml)\")\n}\n"
    } else {
        ""
    };
    format!(
        r#"// GENERATED by the Captain.Food codegen from the derived bin topology — do not edit by hand
// (ADR-20260807-183024 steps 3+4 and #385).

//! `{name}` — {desc}.
//!
//! BUSINESS RUNTIME (#385): {hosting}. Serves the `/health` + `/ping` probes its generated
//! Deployment (deploy/generated/manifests/bins/{name}.yaml) declares (`wired:true`, readiness
//! 503 until the hosted runtime is up), draining on SIGTERM.
//!
//! GATE-THEN-STABILIZE: runnable end-to-end, but the monolith `server` bin remains the DEPLOYED
//! production runtime until ADR-20260807-183024 steps (6)–(7) (#358 cutover) point traffic and
//! the mailbox at these pods.

{uses}/// The bin's scope-filtered typed configuration (#374 Q4): the declared keys of its linked
/// scopes + `common`, read once at startup — the same generated reader the monolith uses,
/// emitted over this bin's key subset.
mod config;

{consts}
#[tokio::main]
async fn main() {{
    // Identity FIRST, before any fallible startup (ADR-20260721-175411): a boot that never binds
    // still names its version in the logs.
    println!("{{BIN}} ({{FAMILY}}) starting -- version {{}}, business runtime (#385)", bin_runtime::build_version());

    // Configuration gate (PROP-20260729-004500): every problem reported at once; production and
    // staging refuse to start on a broken configuration, development reports and continues.
    let (config, problems) = config::Config::resolve();

    // Telemetry SECOND -- after config (it needs LOG_LEVEL and the ingest key), before every
    // worker, so their startup lines are structured and correlated (issue #191).
    let mut telemetry_guard = bin_runtime::init_telemetry(bin_runtime::TelemetrySettings {{
        api_key: config.honeycomb_api_key.clone(),
        endpoint: config.honeycomb_api_endpoint.clone(),
        dataset: config.honeycomb_dataset.clone(),
        sample_ratio: config.otel_traces_sample_ratio.clone(),
        log_level: config.log_level.clone(),
        profile: config.profile.to_string(),
    }});
    if !problems.is_empty() {{
        let report = config::MissingConfig {{ profile: config.profile, problems }};
        tracing::error!(profile = %config.profile, "{{report}}");
        if config.must_stop_on_problems() {{
            // Flush before exiting, or the error explaining the failed deploy never leaves the box.
            telemetry_guard.shutdown();
            // 78 = EX_CONFIG (sysexits.h): a configuration error, not a crash.
            std::process::exit(78);
        }}
        tracing::warn!(profile = %config.profile, "starting anyway -- production and staging STOP here");
    }}
    tracing::info!("{{}}", config.boot_report());

{body}
    bin_runtime::serve_probes(BIN, FAMILY, &config.port.to_string(), health).await;
    telemetry_guard.shutdown();
}}
{partner_fn}"#,
        name = b.name,
        desc = family_purpose(b),
        hosting = hosting,
        uses = uses,
        consts = consts,
        body = body,
        partner_fn = partner_fn,
    )
}

/// The `src/main.rs` of one bin crate: the identity constants and the skeleton entrypoint. The
/// `use … as _;` lines exist so every manifest-declared domain link is a COMPILE-CHECKED fact —
/// a dependency the source never names is an assertion nobody verifies (and cargo-machete's D6
/// gate would rightly flag it).
fn bin_main(b: &BinSpec, model: &Model) -> String {
    match b.family {
        // The CQRS spine (PR #387) — template untouched, byte-parity with the merged wiring.
        "actor" | "pm" | "projector" => return wired_main(b),
        // The API tier (#385 remainder): subgraph slices, role gateways, surfaces, bam.
        "subgraph" => return subgraph_main(b),
        "gateway" => return gateway_main(b, model),
        "surface" if b.name == "adapters" => return adapters_main(b),
        "surface" => return surface_main(b),
        "worker" => return worker_main(b),
        _ => {}
    }
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
// (ADR-20260807-183024 steps 3+4, #382/#349).

//! `{name}` — {desc}.
//!
//! PROBE-SERVING SHELL (gate-then-stabilize): binds `$PORT` and serves the `/health` + `/ping`
//! probes its generated Deployment (deploy/generated/manifests/bins/{name}.yaml) declares,
//! draining on SIGTERM — a viable pod, not yet the business runtime. The monolith `server` bin
//! remains the deployed production runtime; this bin's business wiring is tracked on #349 and
//! flips with steps (6)–(7) (#358 cutover).

{uses}/// The c4-l2 container this binary realizes.
const BIN: &str = "{name}";
/// The bin's family in the deploy topology.
const FAMILY: &str = "{family}";

#[tokio::main]
async fn main() {{
    // Identity FIRST, before any fallible startup (ADR-20260721-175411): a boot that never binds
    // still names its version in the logs. `dev` outside an image; CI bakes `sha-<commit>`.
    let version = std::env::var("CAPTAIN_BUILD_VERSION").unwrap_or_else(|_| "dev".into());
    println!("{{BIN}} ({{FAMILY}}) starting -- version {{version}}, probe-serving shell (#349)");

    // `wired:false` is the shell telling the truth: the pod is viable (probes pass) but the
    // business runtime is not hosted here yet. Real wiring flips this with its own /health.
    let health_body = format!(
        "{{{{\"bin\":\"{{BIN}}\",\"family\":\"{{FAMILY}}\",\"version\":\"{{}}\",\"wired\":false}}}}",
        version.replace('"', "'")
    );
    let app = axum::Router::new()
        .route(
            "/health",
            axum::routing::get(move || {{
                let body = health_body.clone();
                async move {{ ([(axum::http::header::CONTENT_TYPE, "application/json")], body) }}
            }}),
        )
        .route("/ping", axum::routing::get(|| async {{ "ok" }}));

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let addr = format!("0.0.0.0:{{port}}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("{{BIN}}: failed to bind {{addr}}: {{e}}"));
    println!("{{BIN}} listening on {{addr}}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
    println!("{{BIN}}: drained -- bye");
}}

/// Resolve on Ctrl-C or SIGTERM (Kubernetes sends SIGTERM on every Recreate rollout) so
/// in-flight probe requests drain instead of RSTing mid-deploy.
async fn shutdown_signal() {{
    let ctrl_c = async {{
        tokio::signal::ctrl_c().await.expect("install Ctrl-C handler");
    }};
    #[cfg(unix)]
    let terminate = async {{
        use tokio::signal::unix::{{signal, SignalKind}};
        signal(SignalKind::terminate()).expect("install SIGTERM handler").recv().await;
    }};
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {{
        _ = ctrl_c => {{}},
        _ = terminate => {{}},
    }}
    println!("{{BIN}}: shutdown signal received -- draining");
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
            main: bin_main(b, model),
            config: wired(b).then(|| {
                // The bin's key subset = the SAME scope routing the deploy emitter uses for its
                // pod's env (#374 Q4 closed by #385): its linked scopes + owning scope + common.
                // DATABASE_URL follows the SAME needs_db exclusion as deploy.rs::env_yaml — a
                // db-less bin (gateway/surface) whose Config still required the key would
                // exit(78) on boot while the manifest correctly never provides it.
                let mut keys = scoped_config_keys(model, &bin_config_scopes(b, model));
                keys.retain(|k| k.name != "DATABASE_URL" || super::deploy::needs_db(b, model));
                emit_config_module(model, &keys, /* is_server */ false)
            }),
        })
        .collect()
}

/// Emit `crates/infrastructure/src/generated/scopes.rs` — every actor/PM with its OWNING scope
/// (the `specs/{scope}/` folder its definition lives in, #375). The hand-written projection and
/// saga registries carry per-group scope labels for the per-scope bins (D4/#385); their unit
/// tests assert each label against THIS table, so a registry label cannot drift from spec
/// placement without a red test.
pub(crate) fn emit_actor_scopes(model: &Model) -> String {
    let mut rows = String::new();
    for (actor, is_pm, _) in actor_scope_links(model) {
        let file = if is_pm { "processmanager.yaml" } else { "actors.yaml" };
        let scope = model
            .origins
            .get(&(file.to_string(), actor.clone()))
            .or_else(|| model.origins.get(&("actors.yaml".to_string(), actor.clone())))
            .map(|s| s.as_str())
            .unwrap_or(KERNEL_SCOPE);
        rows.push_str(&format!("    (\"{actor}\", \"{scope}\"),\n"));
    }
    format!(
        "// GENERATED by the Captain.Food codegen from specs/{{scope}}/ placement (#375, #385) — do not\n// edit by hand. Every actor/PM with the scope folder that OWNS its definition: the executable\n// tie between the hand-written runtime registries' scope labels and the spec's screaming\n// architecture, consumed by tests (a wrong label is a red test, not a mis-routed projector).\n\n/// `(actor_or_pm, owning scope folder)`, sorted by actor name.\npub const ACTOR_SCOPES: &[(&str, &str)] = &[\n{rows}];\n\n/// The owning scope of one actor/PM, if declared.\npub fn actor_scope(actor: &str) -> Option<&'static str> {{\n    ACTOR_SCOPES.iter().find(|(a, _)| *a == actor).map(|(_, s)| *s)\n}}\n"
    )
}

// ── The API tier (#385 remainder): subgraph / gateway / surface / adapters / bam mains ────────
//
// Same shape as the spine: a GENERATED main = config gate → telemetry → family composition →
// probe server; all machinery lives in hand-written crates (server::bin_support for subgraphs,
// gateway_runtime, surface_runtime, bin_runtime/bin_probes). No business logic in a template.

/// The compile-checked scope-assertion imports of one bin (`use domain_x as _;`).
fn scope_assert_uses(b: &BinSpec) -> String {
    if b.domain_scopes.is_empty() {
        return String::new();
    }
    let mut uses = String::from(
        "// The manifest is the scope assertion; these imports make each declared domain-crate\n// link a compile-checked fact (the linker cannot silently strip it).\n",
    );
    for s in &b.domain_scopes {
        uses.push_str(&format!("use {} as _;\n", domain_crate_ident(s)));
    }
    uses.push('\n');
    uses
}

/// The shared main() prologue: identity print → config gate → telemetry → boot report.
/// `probes` is the crate path serving the probe floor (`bin_runtime` re-exports `bin_probes`).
fn main_prologue(probes: &str) -> String {
    format!(
        r#"#[tokio::main]
async fn main() {{
    // Identity FIRST, before any fallible startup (ADR-20260721-175411): a boot that never binds
    // still names its version in the logs.
    println!("{{BIN}} ({{FAMILY}}) starting -- version {{}}, business runtime (#385)", {probes}::build_version());

    // Configuration gate (PROP-20260729-004500): every problem reported at once; production and
    // staging refuse to start on a broken configuration, development reports and continues.
    let (config, problems) = config::Config::resolve();

    // Telemetry SECOND -- after config (it needs LOG_LEVEL and the ingest key), before every
    // worker, so their startup lines are structured and correlated (issue #191).
    let mut telemetry_guard = {probes}::init_telemetry({probes}::TelemetrySettings {{
        api_key: config.honeycomb_api_key.clone(),
        endpoint: config.honeycomb_api_endpoint.clone(),
        dataset: config.honeycomb_dataset.clone(),
        sample_ratio: config.otel_traces_sample_ratio.clone(),
        log_level: config.log_level.clone(),
        profile: config.profile.to_string(),
    }});
    if !problems.is_empty() {{
        let report = config::MissingConfig {{ profile: config.profile, problems }};
        tracing::error!(profile = %config.profile, "{{report}}");
        if config.must_stop_on_problems() {{
            // Flush before exiting, or the error explaining the failed deploy never leaves the box.
            telemetry_guard.shutdown();
            // 78 = EX_CONFIG (sysexits.h): a configuration error, not a crash.
            std::process::exit(78);
        }}
        tracing::warn!(profile = %config.profile, "starting anyway -- production and staging STOP here");
    }}
    tracing::info!("{{}}", config.boot_report());

"#
    )
}

/// The shared main() epilogue: serve the app + probes on `$PORT`, drain, flush telemetry.
fn main_epilogue(probes: &str) -> String {
    format!(
        "    {probes}::serve_with_app(BIN, FAMILY, &config.port.to_string(), health, app).await;\n    telemetry_guard.shutdown();\n}}\n"
    )
}

/// The generated file header + module docs of one API-tier main.
fn main_header(b: &BinSpec, hosting: &str) -> String {
    format!(
        r#"// GENERATED by the Captain.Food codegen from the derived bin topology — do not edit by hand
// (ADR-20260807-183024 steps 3+4 and #385).

//! `{name}` — {desc}.
//!
//! BUSINESS RUNTIME (#385 API-tier wiring): {hosting}. Serves the `/health` + `/ping` probes its
//! generated Deployment (deploy/generated/manifests/bins/{name}.yaml) declares (`wired:true`),
//! draining on SIGTERM.
//!
//! GATE-THEN-STABILIZE: runnable end-to-end, but the monolith `server` bin remains the DEPLOYED
//! production runtime until ADR-20260807-183024 steps (6)–(7) (#358 cutover) point traffic and
//! the mailbox at these pods.

{uses}/// The bin's scope-filtered typed configuration (#374 Q4): the declared keys of its linked
/// scopes + `common`, read once at startup — the same generated reader the monolith uses,
/// emitted over this bin's key subset.
mod config;

/// The c4-l2 container this binary realizes.
const BIN: &str = "{name}";
/// The bin's family in the deploy topology.
const FAMILY: &str = "{family}";
"#,
        name = b.name,
        desc = family_purpose(b),
        hosting = hosting,
        uses = scope_assert_uses(b),
        family = b.family,
    )
}

/// `graphql-{scope}`: the monolith's GraphQL surface (same DI, same resolvers, same role-path
/// auth) restricted to this scope by the scope-slice extension (D8).
fn subgraph_main(b: &BinSpec) -> String {
    let scope = b.scope.as_deref().unwrap_or("?");
    let mut out = main_header(
        b,
        &format!(
            "serves `/{{role}}/graphql` for every role, restricted to the `{scope}` scope's slice of the master schema (server::bin_support — same composition as the monolith, scope wall from the generated composition table)"
        ),
    );
    out.push_str(&format!(
        "/// The scope whose api.yaml fragment this subgraph serves (D8: one domain, one graph).\nconst SCOPE: &str = \"{scope}\";\n\n"
    ));
    out.push_str(&main_prologue("bin_runtime"));
    out.push_str(r#"    // Without a database this subgraph can neither resolve reads nor enqueue writes: stay a
    // viable pod that says so (readiness 503) -- the config gate already stopped prod/staging.
    let db_ready = !config.database_url.is_empty();
    let app = if db_ready {
        let pool = bin_runtime::pg_pool(&config.database_url, config.database_pool_max_connections)
            .unwrap_or_else(|e| panic!("{BIN}: pg pool init: {e}"));
        server::bin_support::subgraph_app(
            pool,
            server::bin_support::SubgraphSettings {
                scope: SCOPE,
                bin: BIN,
                supabase_jwks_url: config.supabase_jwks_url.clone(),
                supabase_url: config.supabase_url.clone(),
            },
        )
        .await
    } else {
        tracing::warn!(bin = BIN, "DATABASE_URL unset -- subgraph not mounted, readiness stays 503");
        Default::default()
    };
    // Readiness = the subgraph is mounted over its database; the slice itself is static.
    let health: bin_runtime::HealthFn = std::sync::Arc::new(move || {
        (
            db_ready,
            serde_json::json!({
                "scope": SCOPE,
                "db": if db_ready { "configured" } else { "not_configured" },
            }),
        )
    });
"#);
    out.push_str(&main_epilogue("bin_runtime"));
    out
}

/// The `(kind, field, scope)` composition rows — the same derivation as the generated
/// `operation_scopes.rs` (one source: the per-scope api.yaml fragments' origins).
fn operation_scope_rows(model: &Model) -> Vec<(String, String, String)> {
    let api = parse_api(model);
    let origin = |section: &str, name: &str| -> String {
        model
            .origins
            .get(&("api.yaml".to_string(), format!("{section}/{name}")))
            .cloned()
            .unwrap_or_else(|| KERNEL_SCOPE.to_string())
    };
    let mut rows = Vec::new();
    for q in &api.queries {
        rows.push(("query".into(), q.name.clone(), origin("queries", &q.name)));
    }
    for m in &api.mutations {
        rows.push(("mutation".into(), m.name.clone(), origin("mutations", &m.name)));
    }
    for s in &api.subscriptions {
        rows.push(("subscription".into(), s.name.clone(), origin("subscriptions", &s.name)));
    }
    rows
}

/// `gateway-{role}`: static stitching — the embedded composition table routes each top-level
/// field to the owning subgraph; no DB, no auth (the subgraph is the schema boundary), no state.
fn gateway_main(b: &BinSpec, model: &Model) -> String {
    let segment = b.role.as_deref().map(role_path).unwrap_or_default();
    let mut out = main_header(
        b,
        &format!(
            "routes `/{segment}/graphql` top-level fields to the owning subgraphs from the embedded composition table (static stitching, D8: no database, no business logic, no state; auth lives at the subgraph)"
        ),
    );
    out.push_str(&format!(
        "/// The ONE role path segment this gateway serves (role = path, ADR-0006).\nconst ROLE_SEGMENT: &str = \"{segment}\";\n"
    ));
    out.push_str(
        "/// The generated composition table — the SAME rows the subgraphs' scope slice enforces\n/// (specs/{scope}/api.yaml fragments, D8), so routing and acceptance cannot disagree.\nconst COMPOSITION: &[(&str, &str, &str)] = &[\n",
    );
    for (kind, field, scope) in operation_scope_rows(model) {
        out.push_str(&format!("    (\"{kind}\", \"{field}\", \"{scope}\"),\n"));
    }
    out.push_str("];\n/// Every subgraph scope (a `graphql-{scope}` Service exists for each).\nconst SCOPES: &[&str] = &[\n");
    for scope in &model.scopes {
        out.push_str(&format!("    \"{scope}\",\n"));
    }
    out.push_str("];\n\n");
    out.push_str(&main_prologue("bin_probes"));
    out.push_str(r#"    let gateway = gateway_runtime::Gateway::new(
        gateway_runtime::GatewayTable {
            role_segment: ROLE_SEGMENT,
            fields: COMPOSITION,
            scopes: SCOPES,
            kernel_scope: "common",
        },
        &config.gateway_subgraph_urls,
    );
    let subgraphs = serde_json::json!(gateway.urls());
    let app = gateway.app();
    // A gateway is stateless (D8): ready as soon as it listens; the detail names its routing.
    let health: bin_probes::HealthFn = std::sync::Arc::new(move || {
        (true, serde_json::json!({ "role": ROLE_SEGMENT, "subgraphs": subgraphs }))
    });
"#);
    out.push_str(&main_epilogue("bin_probes"));
    out
}

/// `fo-*` / `bo-*`: the audience surface — wasm `/assets` + gateway-backed SSR of the generated
/// screens (D8: no database credential, no views access; reads only over GraphQL).
fn surface_main(b: &BinSpec) -> String {
    let (variant, label) = match b.name.as_str() {
        "fo-marketplace" => ("Marketplace", "marketplace (live.captain.food + apex + neutral hosts)"),
        "fo-storefront" => ("Storefront", "storefront ({slug}.captain.food tenant hosts)"),
        "bo-restaurant" => ("RestaurantBackoffice", "restaurant back office (restos.captain.food)"),
        "bo-rider" => ("Rider", "rider back office (riders.captain.food)"),
        "bo-admin" => ("AdminLanding", "platform back office landing (system.captain.food)"),
        other => unreachable!("surface bin '{other}' has no audience mapping — add it here and in surface_runtime::Audience"),
    };
    let mut out = main_header(
        b,
        &format!("serves the {label}: the wasm hydrate bundle under `/assets` and the audience's generated screens SSR'd through the PUBLIC role gateway (surface_runtime)"),
    );
    out.push_str(&format!(
        "/// The audience this surface serves (health detail; the dispatch lives in surface_runtime).\nconst AUDIENCE: &str = \"{variant}\";\n\n"
    ));
    out.push_str(&main_prologue("bin_probes"));
    out.push_str(&format!(
        r#"    let settings = surface_runtime::SurfaceSettings {{
        audience: surface_runtime::Audience::{variant},
        gateway_url: config.surface_gateway_url.clone(),
        web_assets_dir: config.web_assets_dir.clone(),
    }};
    let gateway = surface_runtime::gateway_origin(&settings);
    let app = surface_runtime::surface_app(&settings);
    // A surface is stateless: ready as soon as it listens; SSR degrades per page (a failed read
    // renders the shell, never a 503 pod).
    let health: bin_probes::HealthFn = std::sync::Arc::new(move || {{
        (true, serde_json::json!({{ "audience": AUDIENCE, "gateway": gateway }}))
    }});
"#
    ));
    out.push_str(&main_epilogue("bin_probes"));
    out
}

/// `adapters`: the partner webhook ingestion surface — the SAME verify → mirror → ACL → ENQUEUE
/// composition the monolith mounts, one route set per partner. Delivery is the owning actor
/// bins' job (the mailbox INSERT's in-transaction pg_notify wakes them); this bin spawns NO
/// worker fleet — one drainer per lane is the #193/#242 constraint, and the actor bins hold it.
fn adapters_main(b: &BinSpec) -> String {
    let mut out = main_header(
        b,
        &"hosts the partner webhook ingestors (stripe/avelo37/coopcycle/uber_direct/hubrise): verify, mirror into the external_* journals, translate through the ACL, ENQUEUE on the shared mailbox — the owning actor bins deliver".to_string(),
    );
    out.push('\n');
    out.push_str(&main_prologue("bin_runtime"));
    out.push_str(r#"    // Without a database nothing can be mirrored or enqueued: stay a viable pod that says so
    // (readiness 503) -- the config gate already stopped prod/staging.
    let db_ready = !config.database_url.is_empty();
    let app = if db_ready {
        let pool = bin_runtime::pg_pool(&config.database_url, config.database_pool_max_connections)
            .unwrap_or_else(|e| panic!("{BIN}: pg pool init: {e}"));
        // Enqueue→worker nudges are in-process wakes; the owning actor bins are woken by the
        // mailbox INSERT's pg_notify (ADR-20260802-200416) — registered for monolith parity.
        let nudges = {
            let mut n = infrastructure::persistence::mailbox_store::MailboxNudges::default();
            for (actor_type, _) in infrastructure::generated::command_router::ACTOR_MAILBOXES {
                n.register(actor_type);
            }
            std::sync::Arc::new(n)
        };
        let mailbox = || {
            std::sync::Arc::new(
                infrastructure::persistence::mailbox_store::PgMailbox::new(pool.clone())
                    .with_nudges(nudges.clone()),
            )
        };
        // Stripe (ADR-20260731-122500 -- the mailbox is the only door): POST /adapters/stripe/webhooks.
        let stripe_ingestor = std::sync::Arc::new(stripe_adapter::StripeWebhookIngestor::new(
            std::sync::Arc::new(stripe_adapter::PgRawStripeEvents::new(pool.clone())),
            mailbox(),
        ));
        // Avelo37 (issue #28): POST /adapters/avelo37/webhooks.
        let avelo37_ingestor = std::sync::Arc::new(avelo37_adapter::Avelo37WebhookIngestor::new(
            std::sync::Arc::new(avelo37_adapter::PgRawAvelo37Events::new(pool.clone())),
            mailbox(),
        ));
        // CoopCycle (issue #58, federated): POST /adapters/coopcycle/{instance}/webhooks.
        let coopcycle_ingestor = std::sync::Arc::new(coopcycle_adapter::CoopCycleWebhookIngestor::new(
            std::sync::Arc::new(coopcycle_adapter::PgRawCoopCycleEvents::new(pool.clone())),
            mailbox(),
        ));
        let coopcycle_registry = coopcycle_adapter::CoopCycleRegistry::from_env()
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, key = "COOPCYCLE_INSTANCES", "misconfigured, treating as unset");
                None
            })
            .unwrap_or_default();
        // Uber Direct (issue #57): POST /adapters/uber-direct/webhooks (X-Uber-Signature HMAC).
        let uber_direct_ingestor = std::sync::Arc::new(uber_direct_adapter::UberDirectWebhookIngestor::new(
            std::sync::Arc::new(uber_direct_adapter::PgRawUberDirectEvents::new(pool.clone())),
            mailbox(),
        ));
        let uber_direct_config = uber_direct_adapter::UberDirectConfig::from_env().unwrap_or_else(|e| {
            tracing::warn!(error = %e, key = "UBER_DIRECT_*", "misconfigured, treating as unset");
            None
        });
        // HubRise (issue #20): raw mirror + enrichment + connect flow, per-account pull tokens.
        // The connect status bus is process-local (the recorded #385 push gap; polls unaffected).
        let mut hubrise_state = hubrise_adapter::HubRiseWebhookState::default();
        hubrise_state.raw =
            Some(std::sync::Arc::new(hubrise_adapter::PgRawHubRiseCallbacks::new(pool.clone())));
        let hubrise_connections =
            std::sync::Arc::new(hubrise_adapter::PgHubRiseConnections::new(pool.clone()));
        hubrise_state.enricher = Some(std::sync::Arc::new(hubrise_adapter::HubRiseEnricher::new(
            mailbox(),
            hubrise_connections.clone(),
            hubrise_adapter::api::HubRiseApi::from_env(),
        )));
        hubrise_state.connect = Some(std::sync::Arc::new(hubrise_adapter::HubRiseConnectFlow::new(
            mailbox(),
            Some(actor_client::OperationStatusBus::default()),
            std::sync::Arc::new(infrastructure::PgRestaurantRepository::new(pool.clone())),
            hubrise_connections,
            hubrise_adapter::connect::HttpHubRiseConnectGateway {
                api: hubrise_adapter::api::HubRiseApi::from_env(),
                client_id: config.hubrise_client_id.clone().unwrap_or_default(),
                client_secret: std::env::var("HUBRISE_WEBHOOK_SECRET").unwrap_or_default(),
            },
        )));
        stripe_adapter::routes(Some(stripe_ingestor))
            .merge(avelo37_adapter::routes(Some(avelo37_ingestor)))
            .merge(coopcycle_adapter::routes(coopcycle_adapter::CoopCycleWebhookState {
                ingestor: Some(coopcycle_ingestor),
                registry: std::sync::Arc::new(coopcycle_registry),
            }))
            .merge(uber_direct_adapter::routes(uber_direct_adapter::UberDirectWebhookState {
                ingestor: Some(uber_direct_ingestor),
                webhook_secret: uber_direct_config.map(|c| std::sync::Arc::new(c.webhook_secret)),
            }))
            .merge(hubrise_adapter::routes(hubrise_state))
    } else {
        tracing::warn!(bin = BIN, "DATABASE_URL unset -- webhook ingestion not mounted, readiness stays 503");
        Default::default()
    };
    // Readiness = the ingestion routes are mounted over their database.
    let health: bin_runtime::HealthFn = std::sync::Arc::new(move || {
        (
            db_ready,
            serde_json::json!({
                "adapters": ["stripe", "avelo37", "coopcycle", "uber_direct", "hubrise"],
                "db": if db_ready { "configured" } else { "not_configured" },
            }),
        )
    });
"#);
    out.push_str(&main_epilogue("bin_runtime"));
    out
}

/// `bam`: the business-activity worker — the scope-filtered projection runtime pointed at the
/// `bam` label. HONEST TODAY: no projection group carries the `bam` scope yet (no bam View_* is
/// specified), so the worker idles caught-up and says so — the moment a bam view lands in the
/// specs, the registry gains its group and this bin drains it with no further wiring.
fn worker_main(b: &BinSpec) -> String {
    let mut out = main_header(
        b,
        &"hosts the business-activity projection worker: the shared registry filtered to the `bam` scope label (no group carries it yet — the worker idles caught-up, honestly, until a bam view is specified)".to_string(),
    );
    out.push_str(
        "/// The scope label whose projection groups this deployable drains (D4 slice rule).\nconst SCOPE: &str = \"bam\";\n\n",
    );
    out.push_str(&main_prologue("bin_runtime"));
    out.push_str(r#"    // Without a database this runtime cannot project: stay a viable pod that says so
    // (readiness 503) rather than crash-loop -- the config gate already stopped prod/staging.
    let db_ready = !config.database_url.is_empty();
    let mut proj_status = None;
    if db_ready {
        let pool = bin_runtime::pg_pool(&config.database_url, config.database_pool_max_connections)
            .unwrap_or_else(|e| panic!("{BIN}: pg pool init: {e}"));
        let waiter = bin_runtime::event_waiter(&config.database_url, config.run_event_push);
        proj_status = Some(bin_runtime::spawn_scope_projector(pool, SCOPE, waiter));
    } else {
        tracing::warn!(bin = BIN, "DATABASE_URL unset -- no projection worker hosted, readiness stays 503");
    }
    let app = Default::default();
    // Readiness = the scoped worker is running; the body carries its checkpoint/head/lag
    // snapshot (the monolith's /projector shape) plus the owned scope label.
    let health: bin_runtime::HealthFn = std::sync::Arc::new(move || match &proj_status {
        Some(handle) => {
            let st = handle.lock().unwrap_or_else(|p| p.into_inner()).clone();
            let running = st.running;
            let mut body = serde_json::to_value(&st).unwrap_or_else(|_| serde_json::json!({ "running": false }));
            if let Some(obj) = body.as_object_mut() {
                obj.insert("scope".into(), serde_json::json!(SCOPE));
            }
            (running, body)
        }
        None => (false, serde_json::json!({ "running": false, "reason": "database_not_configured" })),
    });
"#);
    out.push_str(&main_epilogue("bin_runtime"));
    out
}
