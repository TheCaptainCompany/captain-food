//! Emit the DEPLOYMENT — realization step (4) of ADR-20260807-183024 (#349), on top of the
//! step-3 bin topology (#382): the cluster's desired state becomes a GENERATED artifact, so it
//! cannot drift from the specs (PROP-20260806-223656 D5, reconciled by Argo CD once #366 lands).
//!
//! WHAT IS EMITTED, per derived bin (crate-graph + c4-l2 — the same topology the bin crates are
//! generated from, so image ↔ Deployment ↔ crate cannot fork names):
//!   - `deploy/generated/manifests/bins/{bin}.yaml` — a Deployment (+ a Service for the HTTP
//!     families): `strategy: Recreate` pinned (#193 — a RollingUpdate runs two write paths at
//!     once, which #242's leases/fencing exist to make safe; flipping is a spec change once #242
//!     lands), `replicas: 1` (disjoint actor types are contention-free by construction; two
//!     drains on the SAME type wait on #242), probes on the bins' /health + /ping shell,
//!     resources per PROP-20260806-223656's sizing (small Rust bins), env from the TYPED
//!     configuration keys (secret-sourced keys only — non-secrets are BAKED per
//!     PROP-20260729-014500 D5).
//!   - `deploy/generated/manifests/ingress.yaml` — host routing derived from the screens specs'
//!     `base_url` + per-screen `roles` (role = path, ADR-0006), TLS via the cert-manager
//!     wildcard (#358).
//!   - `deploy/generated/Dockerfile.bin` — ONE parametrized build (`--build-arg BIN=...`): one
//!     shared cargo-chef cook fanning out to N thin runtime images (D5 addendum).
//!   - `deploy/generated/images.json` + `secret-keys.json` — the bin→image mapping (#363's build
//!     matrix input) and the `captain-secrets` contract (which env keys the sealed Secret must
//!     hold, from `deploy.from_secret`).
//!
//! THE PIN LEDGER (`deploy/pins/{bin}.json`, product-owner directive 2026-08-07): each file
//! holds `{digest, source_hash}`; CI writes pins (one structured pin-bump commit per deploy) and
//! `make generate` bakes the digest into the Deployment — so the generated tree stays
//! deterministic from (specs + pins), rollback is `git revert` of the pin change, and
//! `git log -- deploy/pins` IS the deployment history. The emitter SEEDS missing pin files with
//! nulls and NEVER overwrites an existing one (pins are CI-owned state, not spec-derived). A
//! null pin renders as the `:unpinned` tag — visibly undeployable, never a silent default.
//!
//! GATE-THEN-STABILIZE: nothing consumes the BINS tree yet. No Argo CD is installed (#366), no CI
//! step applies manifests; the monolith `server` deployment remains the runtime until steps
//! (6)–(7) of the ADR flip it with the product owner live at the console.
//!
//! WHICH IS WHY THE MONOLITH IS ALSO EMITTED (#358 cutover rehearsal): `deploy/generated/monolith/`
//! is the manifest for the thing we ACTUALLY deploy — Namespace + Deployment + Service + Ingress
//! for the one `server` process, derived from the same specs (the `server` c4-l2 container carrying
//! `deploy_tree: monolith`, the same secret-key contract, the same screens-derived host set). Until
//! this existed, the repo could describe a future cluster in 84 objects and could not describe the
//! ONE workload a cutover has to move — a hole that would have been discovered at the console.
//! The monolith serves EVERY host at `/` because its routes are explicit and host-independent
//! (`crates/server/src/hosts.rs` does host routing in the router FALLBACK), so one Service behind
//! every host rule is the faithful description, not a simplification.

use crate::*;
use std::path::Path;

/// The production namespace (PROP-20260806-223656 §3: `captain-prod`).
pub(crate) const DEPLOY_NAMESPACE: &str = "captain-prod";
/// Per-bin image home: `{IMAGE_PREFIX}/{bin}` on GHCR (the repo's existing registry).
pub(crate) const IMAGE_PREFIX: &str = "ghcr.io/thecaptaincompany/captain-food";
/// The one port every bin shell binds (`PORT` env, declared in configuration.yaml).
const HTTP_PORT: u16 = 8080;
/// The k8s Secret every secret-sourced env key resolves from (sealed at #358 — §2b practice 7).
const SECRETS_NAME: &str = "captain-secrets";

/// One image pin from `deploy/pins/{bin}.json` — the deploy ledger entry. `digest` is the
/// immutable image digest CI published (`sha256:…`); `source_hash` is the bin's source-closure
/// hash (#363's determinator gate) recorded so "unchanged source ⇒ skip rebuild" is repo-vs-repo
/// auditable.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub(crate) struct ImagePin {
    pub(crate) digest: Option<String>,
    #[allow(dead_code)] // read for completeness; consumed by #363's skip gate, not the manifests
    pub(crate) source_hash: Option<String>,
}

/// The seed content of a pin file: explicitly null, never a guessed digest.
pub(crate) fn pin_skeleton_json() -> String {
    "{\n  \"digest\": null,\n  \"source_hash\": null\n}\n".to_string()
}

/// Read every existing pin under `deploy/pins/`. Tolerant of absence (first generation seeds
/// them) but LOUD on malformed JSON — a pin that cannot be parsed must fail generation, because
/// silently rendering `:unpinned` for a bin that HAS a published digest would deploy a stale tag.
pub(crate) fn read_image_pins(repo_root: &Path) -> Result<BTreeMap<String, ImagePin>, String> {
    let mut pins = BTreeMap::new();
    let dir = repo_root.join("deploy/pins");
    let Ok(rd) = fs::read_dir(&dir) else { return Ok(pins) };
    for e in rd.flatten() {
        let p = e.path();
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else { continue };
        let Some(bin) = name.strip_suffix(".json") else { continue };
        let raw = fs::read_to_string(&p).map_err(|e| format!("read {}: {}", p.display(), e))?;
        let pin: ImagePin = serde_json::from_str(&raw)
            .map_err(|e| format!("malformed pin {} ({}): fix or delete it", p.display(), e))?;
        pins.insert(bin.to_string(), pin);
    }
    Ok(pins)
}

/// The image reference a Deployment runs: digest-pinned when the ledger has one
/// (ADR-20260730-051500 — never a moving tag), else the visibly-undeployable `:unpinned`.
fn image_ref(bin: &str, pin: Option<&ImagePin>) -> String {
    match pin.and_then(|p| p.digest.as_deref()) {
        Some(d) => format!("{IMAGE_PREFIX}/{bin}@{d}"),
        None => format!("{IMAGE_PREFIX}/{bin}:unpinned"),
    }
}

/// Families that get a Service (they are called by other pods / the ingress). Worker families
/// (actor/pm/projector/bam) serve /health + /ping for probes but nothing routes TO them.
fn has_service(family: &str) -> bool {
    matches!(family, "subgraph" | "gateway" | "surface" | "adapter")
}

/// Does this bin's pod get DATABASE_URL? Two derivations, no hand-curated bin list:
/// - FAMILY: actor/pm/projector/worker/subgraph touch the mailbox, the log or the views by
///   construction, and every adapter bin mirrors + enqueues by construction (verify → mirror →
///   ACL → ENQUEUE is the family's definition, ADR-20260808-062432). Gateways and surfaces hold
///   no DB access (D8 — "no surface binary holds broad views access"), so the credential never
///   reaches their pods.
/// - DECLARED c4 EDGE: a bin with an explicit c4-l2 relationship to `event-store` or
///   `read-models` gets it regardless of family. c4 relationships are representative for the
///   uniform families, which is why the family rule stays primary; the edge check only ever
///   WIDENS.
/// Every other secret key routes by scope membership (adapter bins narrowed further to their
/// partner's env prefix — `adapter_key_allowed`).
pub(crate) fn needs_db(b: &BinSpec, model: &Model) -> bool {
    if matches!(b.family, "actor" | "pm" | "projector" | "worker" | "subgraph" | "adapter") {
        return true;
    }
    read_c4(model)
        .relationships
        .iter()
        .any(|r| r.from == b.name && matches!(r.to.as_str(), "event-store" | "read-models"))
}

/// The configuration scopes whose keys a bin's pod receives: its linked domain scopes, its
/// owning scope (projector/subgraph), plus `common` (platform keys — DB, telemetry, identity —
/// per ADR-20260807-183024 D5 every bin reads its own scope's + common's keys), plus the
/// c4-DECLARED `integration_scopes` (#385/ADR-20260808-062432: each adapter bin declares its
/// partner's OWNING scope, whose webhook secrets live in that scope's catalog — spec-declared,
/// never a hand list; `adapter_key_allowed` then narrows within the scope to the partner's own
/// env prefix).
pub(crate) fn bin_config_scopes(b: &BinSpec, model: &Model) -> BTreeSet<String> {
    let mut s: BTreeSet<String> = b.domain_scopes.clone();
    if let Some(scope) = &b.scope {
        s.insert(scope.clone());
    }
    for c in read_c4(model).containers {
        if c.id == b.name {
            s.extend(c.integration_scopes.iter().cloned());
        }
    }
    s.insert(KERNEL_SCOPE.to_string());
    s
}

/// Every production secret-sourced env key: `(env key, origin scope, consumer, github repo
/// secret)`. Non-secret keys are BAKED into the binary per profile (PROP-20260729-014500 D5)
/// and never appear in manifests. The CONSUMER rides along since #393: a non-`server` consumer's
/// key reaches ONLY the pod that declares it hosts that consumer (`BinSpec::consumers` — the
/// worker bins' widening); a key nothing hosts reaches no pod, exactly as before. NOTE the
/// SIRENE ingestion keys (`consumer: sirene_ingest`) deliberately declare NO production
/// `from_secret` today — GitHub Actions injects them directly — so the worker-sirene-sync pod
/// env stays without them until the #358 cutover gives them a deploy source (recorded there).
pub(crate) fn production_secret_keys(model: &Model) -> Vec<(String, String, String, String)> {
    parse_config_keys(model)
        .into_iter()
        .filter_map(|k| {
            let repo_secret = k.from_secret.get("production")?.clone();
            // configuration.yaml is a SECTION kind (`keys:`), so origins key it as
            // `keys/{name}`; a key from a flat root catalog has no origin entry -> common.
            let scope = model
                .origins
                .get(&("configuration.yaml".to_string(), format!("keys/{}", k.name)))
                .or_else(|| model.origins.get(&("configuration.yaml".to_string(), k.name.clone())))
                .cloned()
                .unwrap_or_else(|| KERNEL_SCOPE.to_string());
            Some((k.name, scope, k.consumer, repo_secret))
        })
        .collect()
}

/// THE GRANT of one bin: every secret-sourced env key its pod carries, in `secret_keys` order.
/// The pod manifest below and the app index (`emit_app_index`, #491) both render THIS — one
/// derivation, so "what may this app read?" cannot be answered two ways.
pub(crate) fn bin_secret_env_keys(
    b: &BinSpec,
    model: &Model,
    secret_keys: &[(String, String, String, String)],
) -> Vec<String> {
    let scopes = bin_config_scopes(b, model);
    let mut out = Vec::new();
    for (key, scope, consumer, _) in secret_keys {
        if consumer != "server" && !b.consumers.contains(consumer) {
            continue; // #393: a non-server consumer's key reaches only its hosting worker's pod
        }
        if !scopes.contains(scope) {
            continue;
        }
        if !adapter_key_allowed(b, key, scope) {
            continue; // ADR-20260808-062432: an adapter pod carries ONLY its partner's secrets
        }
        if !worker_key_allowed(b, key, scope, /* secret */ true) {
            continue; // #393: a periodic worker keeps only the DB + telemetry floor from common
        }
        if key == "DATABASE_URL" && !needs_db(b, model) {
            continue; // D8: gateways/surfaces hold no database access — the pod never sees the URL
        }
        out.push(key.clone());
    }
    out
}

/// The env block of one bin: APP_PROFILE + PORT explicit, then each secret-sourced key of the
/// bin's config scopes as a `secretKeyRef` into the sealed `captain-secrets`.
fn env_yaml(
    b: &BinSpec,
    model: &Model,
    secret_keys: &[(String, String, String, String)],
    indent: &str,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{indent}- name: APP_PROFILE\n{indent}  value: production\n{indent}- name: PORT\n{indent}  value: \"{HTTP_PORT}\"\n"
    ));
    for key in bin_secret_env_keys(b, model, secret_keys) {
        out.push_str(&format!(
            "{indent}- name: {key}\n{indent}  valueFrom:\n{indent}    secretKeyRef:\n{indent}      name: {SECRETS_NAME}\n{indent}      key: {key}\n"
        ));
    }
    out
}

/// Common labels of one bin's objects.
fn labels_yaml(b: &BinSpec, indent: &str) -> String {
    let mut out = format!(
        "{indent}app.kubernetes.io/name: {}\n{indent}app.kubernetes.io/part-of: captain-food\n{indent}captain.food/family: {}\n",
        b.name, b.family
    );
    if let Some(scope) = &b.scope {
        out.push_str(&format!("{indent}captain.food/scope: {scope}\n"));
    }
    out
}

/// A periodic worker's per-Job wall-clock ceiling, mirroring what its pass declares about
/// itself: the SIRENE sweep self-budgets at 75 min (`sirene_ingest::sweep`), so its Job ceiling
/// is 90 min — the same budget-vs-hang-catcher spacing sirene-sync.yml uses; every other pass is
/// a few SQL statements, bounded at 10 min. Wiring detail (like the per-worker main arms), not
/// topology — the DECLARED cadence lives in c4-l2.
fn cron_active_deadline_seconds(b: &BinSpec) -> u32 {
    match b.name.as_str() {
        "worker-sirene-sync" => 5400,
        _ => 600,
    }
}

/// The CronJob manifest of a PERIODIC worker bin (ADR-20260808-062933 "shape follows cadence"):
/// the platform's scheduler is the scheduler — no probes, no Service, no internal cron loop.
/// The main runs one pass and exits; `concurrencyPolicy: Forbid` keeps passes from overlapping.
fn cron_manifest_yaml(
    b: &BinSpec,
    model: &Model,
    pin: Option<&ImagePin>,
    secret_keys: &[(String, String, String, String)],
) -> String {
    let image = image_ref(&b.name, pin);
    let schedule = b.schedule.as_deref().expect("cron manifest requires a declared schedule");
    let unpinned_note = if pin.and_then(|p| p.digest.as_ref()).is_none() {
        "\n# UNPINNED: deploy/pins/{bin}.json has no digest yet -- CI's pin-bump (#363/#371) writes it\n# and regenerates this file; the :unpinned tag is deliberately undeployable, never a default."
            .replace("{bin}", &b.name)
    } else {
        String::new()
    };
    let suspend_note = if b.suspended {
        "\n# SUSPENDED (c4-l2 `suspended: true`): visibly OFF -- an external residence stays\n# authoritative for this pass until the #358 cutover records the handover."
    } else {
        ""
    };
    format!(
        r#"# GENERATED by the Captain.Food codegen -- do not edit by hand (ADR-20260808-062933, #393).
# `{name}` (worker, PERIODIC -- declared cadence `{schedule}` UTC in c4-l2). Image resolved from
# the deploy ledger deploy/pins/{name}.json ({{digest, source_hash}}; rollback = git revert of
# the pin change). Applied by NOTHING yet -- gate-then-stabilize: Argo CD (#366) reconciles this
# tree only after steps (6)-(7) flip deployment.{unpinned}{suspend_note}
apiVersion: batch/v1
kind: CronJob
metadata:
  name: {name}
  namespace: {ns}
  labels:
{labels}spec:
  # The DECLARED cadence (c4-l2 `schedule:`, UTC) -- the platform's scheduler is the scheduler
  # (ADR-20260808-062933); the bin main runs ONE pass and exits.
  schedule: "{schedule}"
  timeZone: Etc/UTC
  suspend: {suspend}
  # Never two passes at once: each pass is idempotent/checkpointed, but overlap would contend on
  # the same rows for no benefit -- the NEXT scheduled run is the retry.
  concurrencyPolicy: Forbid
  startingDeadlineSeconds: 300
  successfulJobsHistoryLimit: 3
  failedJobsHistoryLimit: 3
  jobTemplate:
    spec:
      # No in-Job retry (the pass is idempotent; the next scheduled run retries with fresh
      # state) and a wall-clock ceiling so a hung pass can never straddle its own next firing.
      backoffLimit: 0
      activeDeadlineSeconds: {deadline}
      template:
        metadata:
          labels:
{tpl_labels}        spec:
          restartPolicy: Never
          containers:
            - name: {name}
              image: {image}
              imagePullPolicy: IfNotPresent
              env:
{env}              resources:
                requests:
                  cpu: 25m
                  memory: 32Mi
                limits:
                  memory: 96Mi
"#,
        name = b.name,
        ns = DEPLOY_NAMESPACE,
        labels = labels_yaml(b, "    "),
        tpl_labels = labels_yaml(b, "            "),
        image = image,
        schedule = schedule,
        suspend = b.suspended,
        deadline = cron_active_deadline_seconds(b),
        env = env_yaml(b, model, secret_keys, "                "),
        unpinned = unpinned_note,
        suspend_note = suspend_note,
    )
}

/// One bin's manifest file: Deployment (+ Service for the HTTP families), or a CronJob for a
/// periodic worker (declared `schedule:` — ADR-20260808-062933).
fn bin_manifest_yaml(b: &BinSpec, model: &Model, pin: Option<&ImagePin>, secret_keys: &[(String, String, String, String)]) -> String {
    if b.schedule.is_some() {
        return cron_manifest_yaml(b, model, pin, secret_keys);
    }
    let image = image_ref(&b.name, pin);
    let unpinned_note = if pin.and_then(|p| p.digest.as_ref()).is_none() {
        "\n# UNPINNED: deploy/pins/{bin}.json has no digest yet -- CI's pin-bump (#363/#371) writes it\n# and regenerates this file; the :unpinned tag is deliberately undeployable, never a default."
            .replace("{bin}", &b.name)
    } else {
        String::new()
    };
    let mut out = format!(
        r#"# GENERATED by the Captain.Food codegen -- do not edit by hand (ADR-20260807-183024 step 4, #349).
# `{name}` ({family}). Image resolved from the deploy ledger deploy/pins/{name}.json
# ({{digest, source_hash}}; rollback = git revert of the pin change). Applied by NOTHING yet --
# gate-then-stabilize: Argo CD (#366) reconciles this tree only after steps (6)-(7) flip deployment.{unpinned}
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {name}
  namespace: {ns}
  labels:
{labels}spec:
  # ONE replica, strategy Recreate (#193 "One instance only until #242's leases and fencing"):
  # a RollingUpdate keeps old AND new pods alive, i.e. two write paths on the same mailbox lanes.
  # Flipping either value is a SPEC change once #242 lands -- never a hand edit here.
  replicas: 1
  strategy:
    type: Recreate
  selector:
    matchLabels:
      app.kubernetes.io/name: {name}
  template:
    metadata:
      labels:
{tpl_labels}    spec:
      containers:
        - name: {name}
          image: {image}
          imagePullPolicy: IfNotPresent
          ports:
            - name: http
              containerPort: {port}
          env:
{env}          readinessProbe:
            httpGet:
              path: /health
              port: http
            periodSeconds: 10
          livenessProbe:
            httpGet:
              path: /ping
              port: http
            periodSeconds: 15
          resources:
            requests:
              cpu: 25m
              memory: 32Mi
            limits:
              memory: 96Mi
"#,
        name = b.name,
        family = b.family,
        ns = DEPLOY_NAMESPACE,
        labels = labels_yaml(b, "    "),
        tpl_labels = labels_yaml(b, "        "),
        image = image,
        port = HTTP_PORT,
        env = env_yaml(b, model, secret_keys, "            "),
        unpinned = unpinned_note,
    );
    if has_service(b.family) {
        out.push_str(&format!(
            r#"---
apiVersion: v1
kind: Service
metadata:
  name: {name}
  namespace: {ns}
  labels:
{labels}spec:
  selector:
    app.kubernetes.io/name: {name}
  ports:
    - name: http
      port: 80
      targetPort: http
"#,
            name = b.name,
            ns = DEPLOY_NAMESPACE,
            labels = labels_yaml(b, "    "),
        ));
    }
    out
}

/// One ingress host rule: the surface at `/`, plus each audience role's `/{role}/graphql` path
/// routed to its gateway (role = path, ADR-0006).
struct HostRule {
    host: String,
    /// The Service serving `/` — `None` for the integration host, which has ONLY the partner
    /// paths (no surface at `/`; anything else is refused by the ingress itself).
    surface: Option<String>,
    roles: Vec<String>,
    /// Extra `(path, service)` routes (partner adapter paths, /external/graphql).
    extra: Vec<(String, String)>,
}

/// Derive the ingress host rules from the screens specs: each surface's host is its screens
/// file's `base_url`, its role paths the union of its screens' `roles`. The file→surface
/// binding mirrors c4-l2's container descriptions; `screens_surface_bindings` is asserted
/// complete against the topology by a codegen test. `fo-storefront` serves the TENANT hosts
/// (`{slug}.captain.food`, CLAUDE.md multi-tenancy), so its rule is the wildcard under its
/// base domain. The BARE domain is SETTLED (#385): captain_frontoffice.yaml declares it in
/// `additional_hosts` (spec home; ADR-20260808-060309) — the marketplace owns the apex, matching
/// `web::router`'s host → surface mapping. The integration host is SETTLED the same way:
/// each `adapter-*` container declares `ingress_host` (`hooks.captain.food`) in c4-l2; bins
/// sharing that host are GROUPED into one rule with a per-partner path (`/adapters/{slug}` →
/// its Service — ADR-20260808-062432, one bin per adapter) and no surface at `/`. The
/// marketplace-host `/adapters/{slug}` paths stay as the per-partner transition alias (partner
/// dashboards point at them until re-registered); the old bare `/webhooks` and `/services`
/// aliases are gone — no adapter ever mounted routes there, so they only routed to 404s.
pub(crate) fn screens_surface_bindings() -> &'static [(&'static str, &'static str)] {
    &[
        ("captain_frontoffice", "fo-marketplace"),
        ("restaurant_frontoffice", "fo-storefront"),
        ("restaurant_backoffice", "bo-restaurant"),
        ("rider", "bo-rider"),
        ("system", "bo-admin"),
    ]
}

fn host_rules(model: &Model, topology: &[BinSpec]) -> Vec<HostRule> {
    let bins: BTreeSet<&str> = topology.iter().map(|b| b.name.as_str()).collect();
    let gateway_roles: BTreeSet<String> = topology
        .iter()
        .filter(|b| b.family == "gateway")
        .filter_map(|b| b.role.clone())
        .collect();
    let mut rules = Vec::new();
    for (file, surface) in screens_surface_bindings() {
        if !bins.contains(surface) {
            continue;
        }
        let Some(def) = model.defs.get(&format!("screens/{file}.yaml")) else { continue };
        let Some(base_url) = def.get("base_url").and_then(|v| v.as_str()) else { continue };
        let bare = base_url.trim_start_matches("https://").trim_end_matches('/');
        // The storefront's base_url is the BARE domain its tenants live under -- serve the
        // wildcard, not the bare host (see the doc comment above).
        let host = if *surface == "fo-storefront" { format!("*.{bare}") } else { bare.to_string() };
        // Audience roles: the union of the file's per-screen `roles` (ADR-0037), kept to roles
        // that actually have a gateway bin.
        let mut roles: BTreeSet<String> = BTreeSet::new();
        if let Some(screens) = def.get("screens").and_then(|v| v.as_sequence()) {
            for screen in screens {
                if let Some(rs) = screen.get("roles").and_then(|v| v.as_sequence()) {
                    for r in rs.iter().filter_map(|v| v.as_str()) {
                        if gateway_roles.contains(r) {
                            roles.insert(r.to_string());
                        }
                    }
                }
            }
        }
        // TRANSITION ALIAS (partner dashboards still point at the marketplace host until
        // re-registered on hooks.captain.food): each partner's /adapters/{slug} prefix routes
        // to ITS bin — same per-partner mapping as the integration host, derived from the
        // adapter family, never a hand list. The external ACL's /external/graphql also rides
        // this host.
        let extra = if *surface == "fo-marketplace" {
            let mut e = Vec::new();
            for a in topology.iter().filter(|b| b.family == "adapter") {
                if let Some(dir) = &a.partner {
                    e.push((format!("/adapters/{}", partner_slug(dir)), a.name.clone()));
                }
            }
            if bins.contains("gateway-external") {
                e.push(("/external/graphql".to_string(), "gateway-external".to_string()));
            }
            e
        } else {
            Vec::new()
        };
        let roles: Vec<String> = roles.into_iter().collect();
        // The declared extra hosts of this surface (#385 spec home): `additional_hosts` on the
        // screens file — the bare apex on captain_frontoffice — served identically to the
        // base host (same surface, same role paths).
        if let Some(hosts) = def.get("additional_hosts").and_then(|v| v.as_sequence()) {
            for h in hosts.iter().filter_map(|v| v.as_str()) {
                rules.push(HostRule {
                    host: h.to_string(),
                    surface: Some(surface.to_string()),
                    roles: roles.clone(),
                    extra: Vec::new(),
                });
            }
        }
        rules.push(HostRule {
            host,
            surface: Some(surface.to_string()),
            roles,
            extra,
        });
    }
    // The dedicated integration host (#385 spec home, ADR-20260808-062432): c4-l2 `ingress_host`
    // on the adapter containers — bins sharing a host are grouped into ONE rule carrying a
    // per-partner path (`/adapters/{slug}` → its Service) and NO surface at `/` (webhooks are
    // path-verified by the adapters themselves; anything outside the declared partner paths is
    // refused by the ingress). A non-adapter container with an `ingress_host` keeps the
    // whole-host rule.
    let by_bin: BTreeMap<&str, &BinSpec> = topology.iter().map(|b| (b.name.as_str(), b)).collect();
    let mut integration_hosts: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for c in read_c4(model).containers {
        // A container of the OTHER deploy tree (the transitional monolith) declares its own host
        // — `api.captain.food` — which belongs to the monolith overlay, never here: this tree has
        // no `server` Service to route it to, so emitting the rule would produce an Ingress that
        // dangles the moment it is applied.
        if c.deploy_tree != DeployTree::Bins {
            continue;
        }
        if let Some(host) = c.ingress_host {
            let Some(b) = by_bin.get(c.id.as_str()) else { continue };
            match (b.family, &b.partner) {
                ("adapter", Some(dir)) => integration_hosts
                    .entry(host)
                    .or_default()
                    .push((format!("/adapters/{}", partner_slug(dir)), b.name.clone())),
                _ => rules.push(HostRule {
                    host,
                    surface: Some(c.id.clone()),
                    roles: Vec::new(),
                    extra: Vec::new(),
                }),
            }
        }
    }
    for (host, extra) in integration_hosts {
        rules.push(HostRule { host, surface: None, roles: Vec::new(), extra });
    }
    rules
}

fn ingress_yaml(model: &Model, topology: &[BinSpec]) -> String {
    let rules = host_rules(model, topology);
    let mut hosts_tls = String::new();
    let mut rules_yaml = String::new();
    for r in &rules {
        hosts_tls.push_str(&format!("        - \"{}\"\n", r.host));
        let mut paths = String::new();
        for role in &r.roles {
            paths.push_str(&format!(
                "          - path: /{p}/graphql\n            pathType: Prefix\n            backend:\n              service:\n                name: gateway-{p}\n                port:\n                  name: http\n",
                p = role_path(role)
            ));
        }
        for (path, svc) in &r.extra {
            paths.push_str(&format!(
                "          - path: {path}\n            pathType: Prefix\n            backend:\n              service:\n                name: {svc}\n                port:\n                  name: http\n"
            ));
        }
        if let Some(surface) = &r.surface {
            paths.push_str(&format!(
                "          - path: /\n            pathType: Prefix\n            backend:\n              service:\n                name: {}\n                port:\n                  name: http\n",
                surface
            ));
        }
        rules_yaml.push_str(&format!("    - host: \"{}\"\n      http:\n        paths:\n{}", r.host, paths));
    }
    format!(
        r#"# GENERATED by the Captain.Food codegen -- do not edit by hand (ADR-20260807-183024 step 4, #349).
# Host routing derived from the screens specs' base_url + per-screen roles (role = path,
# ADR-0006): each host serves its surface bin at / and its audience roles' /{{role}}/graphql at
# that role's gateway. TLS: the cert-manager DNS-01 wildcard certificate (#358 bootstrap issues
# it; ingress-nginx terminates). The BARE captain.food host is owned by fo-marketplace
# (captain_frontoffice.yaml additional_hosts, #385); hooks.captain.food is the declared
# integration host (c4-l2 adapter-* ingress_host, ADR-20260808-062432 one bin per adapter) --
# one /adapters/{{partner}} path per adapter Service, no surface at /, with the marketplace-host
# per-partner paths kept as the transition alias.
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: captain-food
  namespace: {ns}
  labels:
    app.kubernetes.io/part-of: captain-food
spec:
  ingressClassName: nginx
  tls:
    - hosts:
{hosts}      secretName: wildcard-captain-food-tls
  rules:
{rules}"#,
        ns = DEPLOY_NAMESPACE,
        hosts = hosts_tls,
        rules = rules_yaml,
    )
}

// ─── The TRANSITIONAL monolith overlay (`deploy/generated/monolith/`) ────────────────────────────
//
// The bins tree above describes where we are going. This one describes what is running: ONE
// `server` process, one image, every host, every role path. It is emitted from the same specs so
// the two cannot fork, and it is a COMPLETE, applyable overlay (its own Namespace + kustomization)
// so a cutover rehearsal — or the cutover itself — is `kubectl apply -k deploy/generated/monolith`
// and nothing else.

/// The monolith image repo: `IMAGE_PREFIX` ITSELF, not `{IMAGE_PREFIX}/{bin}` — the monolith
/// predates the per-bin split and CI has been publishing it there since ADR-20260721-175411
/// (`.github/workflows/build-image.yml`). Changing it would orphan every published digest.
fn monolith_image_ref(pin: Option<&ImagePin>) -> String {
    match pin.and_then(|p| p.digest.as_deref()) {
        Some(d) => format!("{IMAGE_PREFIX}@{d}"),
        None => format!("{IMAGE_PREFIX}:unpinned"),
    }
}

/// The `server` container of c4-l2 — the monolith's spec home (`deploy_tree: monolith`). `None`
/// on a model that does not declare one: the overlay is then simply not emitted, never guessed.
pub(crate) fn monolith_container(model: &Model) -> Option<Container> {
    read_c4(model).containers.into_iter().find(|c| c.deploy_tree == DeployTree::Monolith)
}

/// The monolith pod's env: APP_PROFILE + PORT, then EVERY production secret key whose declared
/// `consumer` is `server`. No scope narrowing and no `needs_db` narrowing, unlike a bin — this ONE
/// process is every scope, holds the write path, the read models and every integration ACL at
/// once. That breadth is exactly what the bin split exists to end; describing it accurately here
/// is what makes the difference visible.
fn monolith_env_yaml(secret_keys: &[(String, String, String, String)], indent: &str) -> String {
    let mut out = format!(
        "{indent}- name: APP_PROFILE\n{indent}  value: production\n{indent}- name: PORT\n{indent}  value: \"{HTTP_PORT}\"\n"
    );
    for (key, _scope, consumer, _) in secret_keys {
        if consumer != "server" {
            continue;
        }
        out.push_str(&format!(
            "{indent}- name: {key}\n{indent}  valueFrom:\n{indent}    secretKeyRef:\n{indent}      name: {SECRETS_NAME}\n{indent}      key: {key}\n"
        ));
    }
    out
}

fn monolith_labels_yaml(indent: &str) -> String {
    format!(
        "{indent}app.kubernetes.io/name: server\n{indent}app.kubernetes.io/part-of: captain-food\n{indent}captain.food/family: monolith\n"
    )
}

fn monolith_workload_yaml(model: &Model, pin: Option<&ImagePin>, secret_keys: &[(String, String, String, String)]) -> String {
    let unpinned_note = if pin.and_then(|p| p.digest.as_ref()).is_none() {
        "\n# UNPINNED: deploy/pins/server.json has no digest yet -- CI's pin-bump writes it and\n# regenerates this file; the :unpinned tag is deliberately undeployable, never a default."
    } else {
        ""
    };
    let desc = monolith_container(model).map(|c| c.description).unwrap_or_default();
    format!(
        r#"# GENERATED by the Captain.Food codegen -- do not edit by hand (#358 cutover, ADR-20260807-183024).
# THE WORKLOAD WE ACTUALLY DEPLOY: the monolith `server` (c4-l2 `server`, deploy_tree: monolith).
# {desc}
# Image resolved from the deploy ledger deploy/pins/server.json ({{digest, source_hash}}; rollback =
# git revert of the pin change).{unpinned}
apiVersion: apps/v1
kind: Deployment
metadata:
  name: server
  namespace: {ns}
  labels:
{labels}spec:
  # ONE replica, strategy Recreate -- and here it is not a placeholder for #242's leases: this pod
  # ALSO hosts the in-process projector and the mailbox drain (ADR-0040/0043), so a second replica
  # would double-drain the same lanes and a RollingUpdate would overlap two of them mid-deploy.
  replicas: 1
  strategy:
    type: Recreate
  selector:
    matchLabels:
      app.kubernetes.io/name: server
  template:
    metadata:
      labels:
{tpl_labels}    spec:
      containers:
        - name: server
          image: {image}
          imagePullPolicy: IfNotPresent
          ports:
            - name: http
              containerPort: {port}
          env:
{env}          # READINESS is /health ON PURPOSE (ADR-0043): it returns 503 while the applied schema
          # is behind the binary's REQUIRED_SCHEMA_VERSION, so a deploy that races ahead of the
          # out-of-band migration is held OUT of rotation instead of serving a half-migrated read
          # model. `failureThreshold` is generous for exactly that case -- the pod is not sick, it
          # is waiting for CI's `sqlx migrate run`.
          readinessProbe:
            httpGet:
              path: /health
              port: http
            initialDelaySeconds: 5
            periodSeconds: 10
            failureThreshold: 30
          # LIVENESS is /ping, which never touches the database -- a database outage must not get
          # the process killed and restarted into the same outage.
          livenessProbe:
            httpGet:
              path: /ping
              port: http
            initialDelaySeconds: 15
            periodSeconds: 15
            failureThreshold: 6
          resources:
            # Sized from what this binary has actually been living inside: the Render free instance
            # it runs on today is 512Mi/0.1 CPU. Requests match that floor; the limit keeps the
            # headroom the SSR + projector path uses at peak without letting a leak take the node.
            requests:
              cpu: 100m
              memory: 256Mi
            limits:
              memory: 512Mi
---
apiVersion: v1
kind: Service
metadata:
  name: server
  namespace: {ns}
  labels:
{labels}spec:
  selector:
    app.kubernetes.io/name: server
  ports:
    - name: http
      port: 80
      targetPort: http
"#,
        ns = DEPLOY_NAMESPACE,
        labels = monolith_labels_yaml("    "),
        tpl_labels = monolith_labels_yaml("        "),
        image = monolith_image_ref(pin),
        port = HTTP_PORT,
        env = monolith_env_yaml(secret_keys, "            "),
        unpinned = unpinned_note,
        desc = desc,
    )
}

/// Every host the monolith answers: the SAME screens-derived host set as the bins Ingress (one
/// derivation, `host_rules`), plus the monolith's own declared `ingress_host`. Each host routes
/// `/` to the single Service, because the monolith's role paths, webhook paths and SSR surfaces
/// are all one router — the host only selects which surface the FALLBACK renders.
fn monolith_ingress_yaml(model: &Model, topology: &[BinSpec], container: &Container) -> String {
    let mut hosts: Vec<String> = host_rules(model, topology).into_iter().map(|r| r.host).collect();
    if let Some(h) = &container.ingress_host {
        if !hosts.contains(h) {
            hosts.push(h.clone());
        }
    }
    let mut tls = String::new();
    let mut rules = String::new();
    for h in &hosts {
        tls.push_str(&format!("        - \"{h}\"\n"));
        rules.push_str(&format!(
            "    - host: \"{h}\"\n      http:\n        paths:\n          - path: /\n            pathType: Prefix\n            backend:\n              service:\n                name: server\n                port:\n                  name: http\n"
        ));
    }
    format!(
        r#"# GENERATED by the Captain.Food codegen -- do not edit by hand (#358 cutover).
# The monolith's Ingress: EVERY host at `/` to the one `server` Service. The host set is the same
# screens-derived set as the bins Ingress (one derivation) plus the monolith's own declared
# `ingress_host`, `api.captain.food` -- which the bins tree deliberately does NOT serve, because
# role = path per audience host there (ADR-0006). `api.` therefore has a LIFETIME: it must keep
# answering until every registered partner webhook endpoint (Stripe's is
# `https://api.captain.food/adapters/stripe/webhooks`) has been re-registered on
# `hooks.captain.food`. Retiring it first silently drops payment-capture callbacks -- paid orders
# nobody is told about.
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: captain-food-monolith
  namespace: {ns}
  labels:
    app.kubernetes.io/part-of: captain-food
spec:
  ingressClassName: nginx
  tls:
    - hosts:
{tls}      secretName: wildcard-captain-food-tls
  rules:
{rules}"#,
        ns = DEPLOY_NAMESPACE,
        tls = tls,
        rules = rules,
    )
}

fn monolith_kustomization_yaml() -> String {
    format!(
        r#"# GENERATED by the Captain.Food codegen -- do not edit by hand (#358 cutover).
# `kubectl apply -k deploy/generated/monolith` is the WHOLE of deploying Captain.Food as it runs
# today, minus the two things that are deliberately not in it: the `{SECRETS_NAME}` Secret (sealed
# at provisioning, never in the repo -- deploy/generated/secret-keys.json is its checklist) and the
# schema, applied out-of-band by sqlx-cli (ADR-0043; the /health readiness gate above holds the pod
# out of rotation until it lands). Rehearsal procedure: docs/runbooks/cutover-local-rehearsal.md.
apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization
resources:
  - namespace.yaml
  - server.yaml
  - ingress.yaml
"#
    )
}

fn namespace_yaml() -> String {
    format!(
        r#"# GENERATED by the Captain.Food codegen -- do not edit by hand (ADR-20260807-183024 step 4, #349).
apiVersion: v1
kind: Namespace
metadata:
  name: {ns}
  labels:
    app.kubernetes.io/part-of: captain-food
"#,
        ns = DEPLOY_NAMESPACE
    )
}

fn kustomization_yaml(topology: &[BinSpec]) -> String {
    let mut resources = String::from("  - namespace.yaml\n  - ingress.yaml\n");
    for b in topology {
        resources.push_str(&format!("  - bins/{}.yaml\n", b.name));
    }
    format!(
        r#"# GENERATED by the Captain.Food codegen -- do not edit by hand (ADR-20260807-183024 step 4, #349).
# `kustomize build deploy/generated/manifests` renders the whole desired state; Argo CD (#366)
# points here once steps (6)-(7) flip deployment. Until then nothing applies this tree.
apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization
resources:
{resources}"#
    )
}

/// The parametrized per-bin image build: one shared cargo-chef cook, N thin runtime stages
/// (PROP-20260806-223656 D5 addendum -- "one cargo-chef cook -> N thin runtime images"). #363's
/// build matrix invokes it per changed bin: `docker build -f deploy/generated/Dockerfile.bin
/// --build-arg BIN=actor-order .`
fn dockerfile_bin() -> String {
    r#"# GENERATED by the Captain.Food codegen -- do not edit by hand (ADR-20260807-183024 step 4, #349).
# Parametrized per-bin image: `--build-arg BIN=<bin>` selects which crates/bins/ binary the
# runtime stage carries. The chef/planner/cook stages are IDENTICAL for every bin, so one cook is
# shared across the whole matrix build (layer-cached); only the final `cargo build -p` and the
# runtime COPY differ. Two final targets (#385): `runtime` (default) and `runtime-web` -- the
# SDUI surface bins build with `--target runtime-web` (images.json `targets`) so their image
# additionally carries the wasm hydrate bundle they serve under /assets; buildkit only builds
# the wasm stage when that target is requested, so the other 45 bins never pay for it.
FROM rust:1-bookworm AS chef
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/* \
    && cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
# ARG BIN is declared strictly AFTER the cook: a RUN that follows an ARG keys its layer cache on
# the ARG's value, so declaring BIN earlier would give every bin its own cook -- 49 dependency
# compiles instead of the one shared cook the matrix build relies on (#363).
ARG BIN
RUN cargo build --release -p "${BIN}"

# The wasm hydrate bundle (#385, mirrors the monolith Dockerfile's wasm stage): built only when
# the requested target is runtime-web. wasm-bindgen-cli is pinned to the exact wasm-bindgen
# version of crates/web (the CLI refuses a mismatch) -- bump both together, and with the
# monolith Dockerfile, in the same change.
FROM chef AS wasm-builder
RUN cargo install wasm-bindgen-cli --locked --version 0.2.126
COPY --from=planner /app/recipe.json recipe.json
RUN rustup target add wasm32-unknown-unknown \
    && cargo chef cook --release --recipe-path recipe.json \
       --target wasm32-unknown-unknown --no-default-features --features hydrate --package web
COPY . .
RUN cargo build --release -p web --target wasm32-unknown-unknown --no-default-features --features hydrate \
    && wasm-bindgen --target web --no-typescript --out-dir /app/dist --out-name web \
       target/wasm32-unknown-unknown/release/web.wasm

FROM debian:bookworm-slim AS runtime
ARG BIN
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/${BIN} /usr/local/bin/app
# Build identity (ADR-20260721-175411): declared in the FINAL stage only, so a new SHA never
# invalidates the cached cook layers. The bin shell reports it at /health. SOURCE_HASH is the
# #363 source-closure hash baked in as a forensic label -- the pin ledger
# (deploy/pins/{bin}.json) stays the authoritative compare (repo-vs-repo, ADR-20260807-220528);
# the label lets an image on the registry answer "what source produced you?" on its own.
ARG CAPTAIN_BUILD_VERSION=dev
ARG SOURCE_HASH=unknown
ENV CAPTAIN_BUILD_VERSION=$CAPTAIN_BUILD_VERSION
LABEL org.opencontainers.image.revision=$CAPTAIN_BUILD_VERSION \
      org.opencontainers.image.source=https://github.com/TheCaptainCompany/captain-food \
      food.captain.source-hash=$SOURCE_HASH
CMD ["app"]

# The wasm-carrying surface image (#385): `runtime` + the hydrate bundle under the
# WEB_ASSETS_DIR default the surface bins serve at /assets.
FROM runtime AS runtime-web
COPY --from=wasm-builder /app/dist /app/web-assets
"#
    .to_string()
}

/// Does this bin's image carry the wasm hydrate bundle (#385: the SDUI surfaces serve `/assets`
/// themselves — the monolith Dockerfile's wasm stage moves into their images)? The four
/// audiences with a `web::router::Surface`; `bo-admin` serves a plain landing (no web surface
/// yet) and carries none.
pub(crate) fn wasm_target(b: &BinSpec) -> bool {
    b.family == "surface" && b.name != "bo-admin"
}

/// The bin -> image mapping (#363's matrix input; the "every bin maps to exactly one image and
/// back" completeness contract) + each bin's Dockerfile TARGET (`runtime` | `runtime-web`,
/// #385: the SDUI surface images additionally carry the wasm hydrate bundle).
fn images_json(topology: &[BinSpec]) -> String {
    let mut map = serde_json::Map::new();
    map.insert(
        "_generated".into(),
        serde_json::Value::String(
            "GENERATED by the Captain.Food codegen (ADR-20260807-183024 step 4, #349) -- do not edit by hand. Bin -> image mapping; the build matrix (#363) and the pin ledger (deploy/pins/) key off it. targets: the Dockerfile.bin stage per bin (runtime-web = wasm-carrying SDUI surface, #385)."
                .into(),
        ),
    );
    let mut images = serde_json::Map::new();
    let mut targets = serde_json::Map::new();
    for b in topology {
        images.insert(b.name.clone(), serde_json::Value::String(format!("{IMAGE_PREFIX}/{}", b.name)));
        targets.insert(
            b.name.clone(),
            serde_json::Value::String(
                if wasm_target(b) { "runtime-web" } else { "runtime" }.to_string(),
            ),
        );
    }
    map.insert("images".into(), serde_json::Value::Object(images));
    map.insert("targets".into(), serde_json::Value::Object(targets));
    serde_json::to_string_pretty(&serde_json::Value::Object(map)).expect("images.json serializes") + "\n"
}

/// The `captain-secrets` contract: which env keys the sealed Secret must hold in production and
/// which GitHub repo secret supplies each (the k8s face of render-config-sync.json; #358 seals
/// the actual values -- they never touch the repo).
fn secret_keys_json(model: &Model, topology: &[BinSpec]) -> String {
    let mut map = serde_json::Map::new();
    map.insert(
        "_generated".into(),
        serde_json::Value::String(format!(
            "GENERATED by the Captain.Food codegen (ADR-20260807-183024 step 4, #349) -- do not edit by hand. The `{SECRETS_NAME}` Secret contract: env key -> {{scope, github repo secret}} for production. Values are sealed at #358 (PROP-20260806-223656 s2b practice 7); this file is the checklist, never the values."
        )),
    );
    let mut keys = serde_json::Map::new();
    let hosted: BTreeSet<&String> = topology.iter().flat_map(|b| b.consumers.iter()).collect();
    for (key, scope, consumer, repo_secret) in production_secret_keys(model) {
        // The contract lists exactly what some pod's env references (#393): `server`-consumed
        // keys plus any consumer a worker bin declares it hosts. A non-server consumer's key
        // with NO hosting pod stays out — today that is every SIRENE key anyway, since they
        // declare no production `from_secret` (GitHub-Actions-injected until the #358 cutover).
        if consumer != "server" && !hosted.contains(&consumer) {
            continue;
        }
        let mut entry = serde_json::Map::new();
        entry.insert("scope".into(), serde_json::Value::String(scope));
        entry.insert("from_github_secret".into(), serde_json::Value::String(repo_secret));
        keys.insert(key, serde_json::Value::Object(entry));
    }
    map.insert("keys".into(), serde_json::Value::Object(keys));
    serde_json::to_string_pretty(&serde_json::Value::Object(map)).expect("secret-keys.json serializes") + "\n"
}

fn deploy_readme(topology: &[BinSpec]) -> String {
    format!(
        r#"# deploy/ -- the GENERATED deployment (ADR-20260807-183024 step 4, #349)

Everything under `deploy/generated/` is EMITTED by `make generate` from the specs (bin topology =
crate-graph + c4-l2; env = the typed configuration keys; ingress = the screens specs' hosts and
roles) and drift-checked by `make validate` / CI -- never hand-edited. `deploy/pins/` is the ONE
exception: the deploy LEDGER, written by CI (one `{{digest, source_hash}}` pin-bump commit per
deploy, #363/#371), read by the emitter to bake image digests into the Deployments. Rollback of
one image = `git revert` of its pin change; `git log -- deploy/pins` is the deployment history.

TWO TREES, ON PURPOSE, while the cutover is in flight:

- `manifests/` -- the per-bin topology we are moving TO. GATE-THEN-STABILIZE: nothing applies it
  today. Argo CD (#366) reconciles it only when steps (6)-(7) of the ADR flip deployment, with the
  product owner live at the console. The bins behind these manifests are probe-serving shells
  (/health + /ping) -- their business runtime wiring (mailbox hosting, per-scope projection,
  subgraph slices, gateway composition tables) is tracked on #349 and blocks the flip.
- `monolith/` -- the workload we ACTUALLY deploy (#358): the single `server` process, serving every
  host and every role path. `kubectl apply -k deploy/generated/monolith` is the whole of deploying
  Captain.Food as it runs today, minus the sealed `captain-secrets` (checklist: secret-keys.json)
  and the schema, applied out-of-band by sqlx-cli (ADR-0043). It exists because the repo used to
  describe a future cluster in per-bin manifests and could not describe the one workload a cutover
  has to move. It is emitted from the c4-l2 `server` container's `deploy_tree: monolith`, so
  deleting that container is what retires the monolith -- and prunes this overlay with it.
  `api.captain.food` is served HERE and deliberately not in `manifests/`: it is the registered
  partner webhook address, and it outlives the cutover until those are re-registered on
  `hooks.captain.food` (ADR-20260811-004500).

Rehearse the whole sequence locally before spending anything:
docs/runbooks/cutover-local-rehearsal.md.

Bins: {n}. Build one image: `docker build -f deploy/generated/Dockerfile.bin --build-arg
BIN=<bin> .` Render the manifests: `kustomize build deploy/generated/manifests`.
"#,
        n = topology.len(),
    )
}

/// The full generated deploy tree: `(path relative to deploy/generated/, content)`.
pub(crate) fn emit_deploy_tree(model: &Model, pins: &BTreeMap<String, ImagePin>) -> Vec<(String, String)> {
    let topology = bin_topology(model);
    if topology.is_empty() {
        return Vec::new(); // degenerate/fixture model: never mass-emit or mass-prune
    }
    let secret_keys = production_secret_keys(model);
    let mut out: Vec<(String, String)> = vec![
        ("README.md".into(), deploy_readme(&topology)),
        ("Dockerfile.bin".into(), dockerfile_bin()),
        ("images.json".into(), images_json(&topology)),
        ("secret-keys.json".into(), secret_keys_json(model, &topology)),
        ("manifests/namespace.yaml".into(), namespace_yaml()),
        ("manifests/kustomization.yaml".into(), kustomization_yaml(&topology)),
        ("manifests/ingress.yaml".into(), ingress_yaml(model, &topology)),
    ];
    for b in &topology {
        out.push((
            format!("manifests/bins/{}.yaml", b.name),
            bin_manifest_yaml(b, model, pins.get(&b.name), &secret_keys),
        ));
    }
    // The transitional monolith overlay — emitted only when c4-l2 declares a `deploy_tree:
    // monolith` container, so retiring the monolith is a SPEC deletion that prunes the whole
    // overlay, not a code change plus a hand `rm`.
    if let Some(c) = monolith_container(model) {
        out.push(("monolith/namespace.yaml".into(), namespace_yaml()));
        out.push((
            "monolith/server.yaml".into(),
            monolith_workload_yaml(model, pins.get("server"), &secret_keys),
        ));
        out.push(("monolith/ingress.yaml".into(), monolith_ingress_yaml(model, &topology, &c)));
        out.push(("monolith/kustomization.yaml".into(), monolith_kustomization_yaml()));
    }
    out
}
