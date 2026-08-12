//! Emit `specs/generated/apps.generated.md` — THE APP INDEX (slice A1 of
//! PROP-20260811-141654, #491): the 57 deployables, what each one is for, which business
//! boundary it belongs to, and — the column the split is judged on — whether the domain crates
//! it RESOLVES are the domain crates it DECLARES.
//!
//! WHY IT IS GENERATED. The product-owner ask was *"the app list and all dependencies we need to
//! make sure we have a clean split"*. A hand-written list answers that once and is wrong by the
//! next commit; every fact here is already SOURCE somewhere (`architecture/c4-l2.yaml`, the
//! actor/PM catalogs, `configuration.yaml`, the Cargo manifests), so the index is a RENDERED
//! VIEW of what the build actually resolves, held to it by `check-drift`.
//!
//! THE ONE MEASURED INPUT. Every other emitter in this tool derives from the YAML model alone.
//! This one additionally MEASURES the workspace crate graph ([`measure_workspace_crate_graph`],
//! guppy over `cargo metadata` — the same resolver the build uses, the same one the #363
//! determinator gate reads). That is deliberate and is the whole value of the artifact: the
//! declared column is what a manifest ASSERTS, the resolved column is what the linker actually
//! pulls in, and an index that inferred both from the spec could never show the two disagreeing.
//! It measures NORMAL dependency links only (a dev-dependency ships in no image) and reports
//! only WORKSPACE members, so the output cannot vary with a registry version.
//!
//! BOUNDARY. The boundary set is NOT a list in this file: it is read from
//! `specs/architecture/c4-l2.yaml` `boundedContexts` — five business contexts (`customer` ·
//! `order` · `catalog` · `restaurant` · `delivery`) plus `platform` for the apps that serve no
//! business domain. That is the same set DECISIONS.md §31 BND-1 arbitrates (its option (a) is
//! *"it IS the C4's set"*), so if BND-1 lands differently the index follows the spec it reads
//! and needs no edit here. Each family's boundary comes from the
//! evidence that family HAS: an actor/PM bin from the context that DECLARES it (`aggregates:` /
//! `processManagers:`), a projector/subgraph from its scope folder, a gateway from the context
//! declaring its role, a surface from the c4 relationship naming its gateway, an adapter/worker
//! from its declared `integration_scopes`. Nothing here is a per-app `match` arm — a new app
//! inherits its boundary from the same declarations every other artifact reads, and
//! `app_index_every_app_has_a_declared_boundary` fails if that stops being true.
//!
//! WHAT IT DOES NOT DO. It moves no source, creates no `specs/apps/` folder and edits no
//! manifest. A rendered index cannot make a link boundary real — only the crate graph does that
//! (PROP-20260811-090000, [#490]) — so read section 2's fat column as the debt, never as its
//! repayment.

use crate::*;

use guppy::graph::PackageGraph;
use guppy::MetadataCommand;

/// The `platform` bucket: the c4 bounded context for apps that belong to no BUSINESS boundary
/// (the mailbox-supervision actor, the kernel subgraph, the cron workers, the admin/external
/// gateways). It is a real bounded context in `c4-l2.yaml`, not a sentinel invented here.
const PLATFORM_BOUNDARY: &str = "platform";

/// The one register row that is CLOSED and not yet reflected in the spec this index reads
/// (DECISIONS.md §31 D9, 2026-08-11: `CartBindingProcess` is homed in `order`, while
/// `specs/architecture/c4-l2.yaml` still lists it under `customer`). These two names are NOT an
/// app rule and are not a boundary table — they name a decision in flight so the index can state
/// the move it will make instead of silently rendering the pre-decision answer. The bullet they
/// drive is CONDITIONAL on the spec still disagreeing, so both constants become dead the moment
/// the one-line c4 edit lands, and `app_index_states_the_decided_home_until_the_spec_agrees`
/// fails if they are kept past that point.
pub(crate) const CART_BINDING_PROCESS: &str = "CartBindingProcess";
pub(crate) const CART_BINDING_DECIDED_HOME: &str = "order";

// ─── The measured half: the workspace crate graph ───────────────────────────────────────────────

/// The workspace dependency graph, MEASURED — crate → its direct workspace dependencies over
/// NORMAL links. Plain data on purpose: the renderer below is a pure function of (model, graph),
/// so it is testable without cargo, and the one impure step (running `cargo metadata`) is a
/// single call at the top of `main`.
#[derive(Default)]
pub(crate) struct CrateGraph {
    direct: BTreeMap<String, BTreeSet<String>>,
}

impl CrateGraph {
    /// Build from measured edges (tests construct one directly).
    pub(crate) fn from_edges(direct: BTreeMap<String, BTreeSet<String>>) -> Self {
        Self { direct }
    }

    /// The direct workspace dependencies of one crate.
    pub(crate) fn direct(&self, krate: &str) -> &BTreeSet<String> {
        static EMPTY: std::sync::OnceLock<BTreeSet<String>> = std::sync::OnceLock::new();
        self.direct.get(krate).unwrap_or_else(|| EMPTY.get_or_init(BTreeSet::new))
    }

    /// The transitive workspace closure of one crate, EXCLUDING the crate itself.
    pub(crate) fn closure(&self, krate: &str) -> BTreeSet<String> {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut stack: Vec<String> = vec![krate.to_string()];
        while let Some(next) = stack.pop() {
            for dep in self.direct(&next) {
                if seen.insert(dep.clone()) {
                    stack.push(dep.clone());
                }
            }
        }
        seen
    }

    /// Does `krate` reach `target` (or IS it)?
    fn reaches(&self, krate: &str, target: &str) -> bool {
        krate == target || self.closure(krate).contains(target)
    }

    /// Is this crate a member of the measured workspace at all?
    fn known(&self, krate: &str) -> bool {
        self.direct.contains_key(krate)
    }

    /// Every measured workspace member — the denominator the "reached by no deployable" line
    /// subtracts from. A crate no image links is either tooling or a crate nothing calls, and
    /// only the full member list can tell the index which crates it never had to mention.
    fn members(&self) -> BTreeSet<&str> {
        self.direct.keys().map(|k| k.as_str()).collect()
    }
}

/// Measure the workspace graph with guppy (`cargo metadata`) — the resolver the build itself
/// uses. NORMAL links only: a dev-dependency is not in any image, and counting one would report
/// a boundary violation that does not ship. Workspace members only: an external crate's presence
/// depends on registry state, and this artifact is drift-gated.
pub(crate) fn measure_workspace_crate_graph(root: &std::path::Path) -> Result<CrateGraph, String> {
    let mut cmd = MetadataCommand::new();
    cmd.current_dir(root);
    let graph: PackageGraph = cmd.build_graph().map_err(|e| format!("cargo metadata: {e}"))?;
    let mut direct: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for pkg in graph.workspace().iter() {
        let deps: BTreeSet<String> = pkg
            .direct_links()
            .filter(|l| l.normal().is_present() && l.to().in_workspace())
            .map(|l| l.to().name().to_string())
            .collect();
        direct.insert(pkg.name().to_string(), deps);
    }
    Ok(direct.into())
}

impl From<BTreeMap<String, BTreeSet<String>>> for CrateGraph {
    fn from(direct: BTreeMap<String, BTreeSet<String>>) -> Self {
        Self::from_edges(direct)
    }
}

// ─── The boundary map ───────────────────────────────────────────────────────────────────────────

/// Which bounded context owns what — read from `c4-l2.yaml boundedContexts` (BND-1, closed
/// 2026-08-11). Nothing in this struct is a literal: the ids, the aggregate membership and the
/// role membership are all declarations the C4 emitter and the §15 validator already read.
pub(crate) struct BoundaryMap {
    /// Bounded-context ids, in spec order — the closed boundary set.
    pub(crate) ids: Vec<String>,
    /// Aggregate/PM name → the context that DECLARES it. Process managers included: c4 lists
    /// them under `processManagers:`, which is the one place a bridge's home is stated.
    of_actor: BTreeMap<String, String>,
    /// `UserType` role → the contexts that CLAIM it. A SET, not one id: two contexts claiming one
    /// role is an ambiguity to show, and a plain `insert` here resolved it by iteration order,
    /// silently. A role no context claims is equally a `None` — see `of_role`.
    of_role: BTreeMap<String, BTreeSet<String>>,
    /// Scope folder → owning context, derived from the contexts of the actors that folder owns.
    /// `None` = the folder's actors span more than one context (an ambiguity the index must
    /// SHOW, never average away) or it owns none.
    of_scope: BTreeMap<String, Option<String>>,
}

impl BoundaryMap {
    /// The context of a scope FOLDER. `None` when the folder's aggregates span more than one
    /// context OR it owns none — both are "not derivable", and the index must show that rather
    /// than average it away. `platform` is returned only for a name that is not a scope folder at
    /// all (an `integration_scopes` entry naming something outside `specs/{scope}/`), because
    /// every real folder is present in the map by construction.
    pub(crate) fn of_scope(&self, scope: &str) -> Option<&str> {
        match self.of_scope.get(scope) {
            Some(Some(bc)) => Some(bc.as_str()),
            Some(None) => None,
            None => Some(PLATFORM_BOUNDARY),
        }
    }

    /// The context of a `UserType` role — `Some` only when EXACTLY ONE context claims it. Zero
    /// claimants and two claimants are both `None`: the first is a role no `boundedContexts`
    /// entry lists (`ADMIN`, `EXTERNAL` today), the second a contradiction. Either way the
    /// gateway falls to `platform` and section 2 names the role out loud, so a default never
    /// passes for a declaration.
    pub(crate) fn of_role(&self, role: &str) -> Option<&str> {
        match self.of_role.get(role) {
            Some(set) if set.len() == 1 => set.iter().next().map(|s| s.as_str()),
            _ => None,
        }
    }

    /// The context c4 DECLARES an aggregate or process manager in.
    pub(crate) fn of_actor(&self, actor: &str) -> Option<&str> {
        self.of_actor.get(actor).map(|s| s.as_str())
    }

    /// Every context claiming `role` — the raw evidence behind `of_role`, so a test can assert the
    /// derivation is unambiguous instead of asserting the answer it produced.
    pub(crate) fn role_claimants(&self, role: &str) -> BTreeSet<&str> {
        self.of_role.get(role).map(|s| s.iter().map(|c| c.as_str()).collect()).unwrap_or_default()
    }

    /// The context owning the role served at `/{path}/graphql` (how a surface inherits its
    /// boundary: the c4 relationship names its gateway, the gateway's path names the role).
    fn of_role_path(&self, path: &str) -> Option<&str> {
        let role = self.of_role.keys().find(|r| role_path(r) == path)?.clone();
        self.of_role(&role)
    }
}

/// The scope FOLDER that owns an actor/PM definition (#375 placement) — the same lookup
/// `emit_actor_scopes` writes into the runtime registries' scope table.
fn actor_owning_scope(model: &Model, actor: &str, is_pm: bool) -> String {
    let file = if is_pm { "processmanager.yaml" } else { "actors.yaml" };
    model
        .origins
        .get(&(file.to_string(), actor.to_string()))
        .or_else(|| model.origins.get(&("actors.yaml".to_string(), actor.to_string())))
        .or_else(|| model.origins.get(&("processmanager.yaml".to_string(), actor.to_string())))
        .cloned()
        .unwrap_or_else(|| KERNEL_SCOPE.to_string())
}

/// Build the boundary map from the model.
pub(crate) fn boundary_map(model: &Model) -> BoundaryMap {
    let c4 = read_c4(model);
    let mut of_actor = BTreeMap::new();
    let mut of_aggregate = BTreeMap::new();
    let mut of_role = BTreeMap::new();
    let mut ids = Vec::new();
    for ctx in &c4.contexts {
        ids.push(ctx.id.clone());
        for a in &ctx.aggregates {
            of_aggregate.insert(a.clone(), ctx.id.clone());
        }
        for a in ctx.aggregates.iter().chain(ctx.process_managers.iter()) {
            of_actor.insert(a.clone(), ctx.id.clone());
        }
        for r in &ctx.roles {
            of_role.entry(r.clone()).or_insert_with(BTreeSet::new).insert(ctx.id.clone());
        }
    }
    // scope → context, via the AGGREGATES each scope folder owns (the folder is the
    // scope-membership declaration everything else derives from, ADR-20260807-183024).
    //
    // AGGREGATES ONLY, ON PRINCIPLE: a boundary owns the state it can SERIALIZE, and an aggregate
    // is the only thing that owns state — so an aggregate's folder is evidence of where a
    // boundary lives, while a process manager is a DECLARED BRIDGE whose whole job is to span
    // two. A bridge's c4 home may legitimately differ from the folder its definition sits in, and
    // letting it vote would make that folder's boundary ambiguous and silently un-derivable for
    // every app that reads the folder. So a PM takes its home from `of_actor`, where c4 states it
    // directly, and never contributes to `of_scope`. This is the rule, not a workaround: it holds
    // when no folder disagrees. The live example that shows the difference is `specs/ordering/`,
    // which owns `CartBindingProcess` while c4 homes it in `customer` — without the rule,
    // `projector-ordering` and `graphql-ordering` would lose their boundary.
    let mut per_scope: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (actor, _, _) in actor_scope_links(model) {
        let Some(bc) = of_aggregate.get(&actor) else { continue };
        let scope = actor_owning_scope(model, &actor, false);
        per_scope.entry(scope).or_default().insert(bc.clone());
    }
    let mut of_scope = BTreeMap::new();
    for scope in &model.scopes {
        let set = per_scope.get(scope).cloned().unwrap_or_default();
        let one = if set.len() == 1 { set.into_iter().next() } else { None };
        of_scope.insert(scope.clone(), one);
    }
    BoundaryMap { ids, of_actor, of_role, of_scope }
}

// ─── One row of the index ───────────────────────────────────────────────────────────────────────

/// One app, fully resolved: the row the index renders and the tests assert against.
pub(crate) struct AppRow {
    pub(crate) name: String,
    /// The index's family vocabulary (the request's own words): the topology's `subgraph` is
    /// `graphql` here because that is the bin's name and the deploy artifact's label, and the
    /// topology's `surface` splits into `fo`/`bo`, which are different audiences with different
    /// gateways.
    pub(crate) family: &'static str,
    /// The bounded context this app belongs to — one of `BoundaryMap::ids`.
    pub(crate) boundary: String,
    /// The `UserType` a gateway serves, `None` for every other family — kept on the row so the
    /// index can say which roles no context claims without re-deriving the topology.
    pub(crate) role: Option<String>,
    /// The aggregate (`actor`) or process manager (`pm`) this app realizes, `None` elsewhere.
    pub(crate) actor: Option<String>,
    /// What the manifest says (scope names of the `domain-*` crates it links).
    pub(crate) declared: BTreeSet<String>,
    /// What the workspace graph resolves (scope names of every `domain-*` crate in the closure).
    pub(crate) resolved: BTreeSet<String>,
    /// The direct dependencies through which the `domain` facade enters the image. Empty = the
    /// facade is not in the closure at all (the honest families).
    pub(crate) facade_carriers: Vec<String>,
    /// Is this app a member of the MEASURED workspace? False would mean the index is describing a
    /// deployable cargo does not know about — reported as such, never averaged into `honest`.
    pub(crate) measured: bool,
    /// One line of what the app HOSTS — family-shaped (lanes, projection groups, api surface,
    /// partner, cadence).
    pub(crate) hosts: String,
    /// Secret-sourced env keys the generated pod carries (the SAME derivation the deploy emitter
    /// renders — `bin_secret_env_keys`, one source).
    pub(crate) secrets: Vec<String>,
    /// Keys the bin's own hosted consumer DECLARES it reads and the pod does NOT carry, each
    /// with why. A live trap at the #358 cutover, invisible in every other artifact.
    pub(crate) missing_secrets: Vec<(String, String)>,
}

impl AppRow {
    /// THE THREE STATES, exhaustive and mutually exclusive: `honest` (measured, and the image
    /// links exactly what the manifest declares), `fat` (measured, and it links more), and
    /// unmeasured (`!measured` — cargo does not know this deployable). `honest + fat + unmeasured
    /// == 1` for every row, which is why the tables render all three: folding the third into
    /// `fat` would report an app we cannot see as an app we caught lying.
    ///
    /// An unmeasured app is never honest — absence of evidence is not evidence.
    pub(crate) fn honest(&self) -> bool {
        self.measured && self.declared == self.resolved
    }

    /// Measured, and the image links domain crates the manifest never names. NOT the complement
    /// of `honest`: an unmeasured app is neither.
    pub(crate) fn fat(&self) -> bool {
        self.measured && self.declared != self.resolved
    }

    /// The BUSINESS boundaries this app's DECLARED crates reach (`platform` excluded — the
    /// kernel is shared by construction and spanning it is not a boundary crossing).
    pub(crate) fn declared_span(&self, b: &BoundaryMap) -> BTreeSet<String> {
        scope_boundaries(&self.declared, b)
    }

    /// The BUSINESS boundaries this app's RESOLVED crates reach.
    pub(crate) fn resolved_span(&self, b: &BoundaryMap) -> BTreeSet<String> {
        scope_boundaries(&self.resolved, b)
    }
}

fn scope_boundaries(scopes: &BTreeSet<String>, b: &BoundaryMap) -> BTreeSet<String> {
    scopes
        .iter()
        .filter_map(|s| b.of_scope(s))
        .filter(|bc| *bc != PLATFORM_BOUNDARY)
        .map(|bc| bc.to_string())
        .collect()
}

/// The index's family label for a topology family (see `AppRow::family`).
fn index_family(b: &BinSpec) -> &'static str {
    match b.family {
        "subgraph" => "graphql",
        "surface" => {
            if b.name.starts_with("bo-") {
                "bo"
            } else {
                "fo"
            }
        }
        "worker" if b.name == "bam" => "bam",
        f => f,
    }
}

/// Family display order — the request's order, spine first, surfaces last.
pub(crate) const FAMILY_ORDER: &[&str] =
    &["actor", "pm", "projector", "worker", "adapter", "graphql", "gateway", "fo", "bo", "bam"];

/// `domain-ordering` → `ordering`; anything else → `None`.
fn domain_crate_scope(krate: &str) -> Option<String> {
    krate.strip_prefix("domain-").map(|s| s.replace('-', "_"))
}

/// Every app, resolved into its row — the whole derivation, in one place.
pub(crate) fn app_rows(model: &Model, graph: &CrateGraph) -> Vec<AppRow> {
    let bounds = boundary_map(model);
    let c4 = read_c4(model);
    let secret_keys = production_secret_keys(model);
    let config_keys = parse_config_keys(model);
    let api = parse_api(model);
    let views = parse_views(model);
    let ops = operation_scope_rows(model);
    // surface → the gateway it speaks to (the c4 relationship IS the declaration).
    let surface_gateway: BTreeMap<&str, &str> = c4
        .relationships
        .iter()
        .filter(|r| r.from.starts_with("fo-") || r.from.starts_with("bo-"))
        .map(|r| (r.from.as_str(), r.to.as_str()))
        .collect();
    let container = |id: &str| c4.containers.iter().find(|c| c.id == id);

    let mut rows = Vec::new();
    for b in bin_topology(model) {
        let declared: BTreeSet<String> = b.domain_scopes.clone();
        let resolved: BTreeSet<String> = graph
            .closure(&b.name)
            .iter()
            .filter_map(|c| domain_crate_scope(c))
            .collect();
        let facade_carriers: Vec<String> = if graph.known(&b.name) {
            graph
                .direct(&b.name)
                .iter()
                .filter(|d| domain_crate_scope(d).is_none() && graph.reaches(d, "domain"))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let measured = graph.known(&b.name);
        let boundary = app_boundary(&b, model, &bounds, &surface_gateway, container(&b.name));
        let hosts = app_hosts(&b, model, &api, &views, &ops, container(&b.name), &surface_gateway);
        let secrets = bin_secret_env_keys(&b, model, &secret_keys);
        let missing = missing_secret_keys(&b, &config_keys, &secrets);
        rows.push(AppRow {
            name: b.name.clone(),
            family: index_family(&b),
            boundary,
            role: b.role.clone(),
            actor: b.actor.clone(),
            declared,
            resolved,
            facade_carriers,
            measured,
            hosts,
            secrets,
            missing_secrets: missing,
        });
    }
    rows.sort_by(|a, b| {
        let rank = |f: &str| FAMILY_ORDER.iter().position(|x| *x == f).unwrap_or(usize::MAX);
        (rank(a.family), &a.name).cmp(&(rank(b.family), &b.name))
    });
    rows
}

/// One app's boundary, from the evidence its family has (see the module doc).
fn app_boundary(
    b: &BinSpec,
    model: &Model,
    bounds: &BoundaryMap,
    surface_gateway: &BTreeMap<&str, &str>,
    container: Option<&Container>,
) -> String {
    let from_scopes = |scopes: &[String]| -> Option<String> {
        let set: BTreeSet<&str> = scopes.iter().filter_map(|s| bounds.of_scope(s)).collect();
        (set.len() == 1).then(|| set.into_iter().next().unwrap_or(PLATFORM_BOUNDARY).to_string())
    };
    let unresolved = || PLATFORM_BOUNDARY.to_string();
    match b.family {
        // The context that DECLARES the aggregate; a PM (which no context lists as an aggregate)
        // falls back to the scope folder that owns its definition — its home, not its reach.
        "actor" | "pm" => {
            let actor = b.actor.clone().unwrap_or_default();
            if let Some(bc) = bounds.of_actor.get(&actor) {
                return bc.clone();
            }
            // No context declares it (a new actor c4 has not been told about — §15 catches that
            // separately). Its HOME is its scope folder, never the widest scope it can reach.
            bounds
                .of_scope(&actor_owning_scope(model, &actor, b.family == "pm"))
                .map(|s| s.to_string())
                .unwrap_or_else(unresolved)
        }
        "projector" | "subgraph" => b
            .scope
            .as_deref()
            .and_then(|s| bounds.of_scope(s))
            .map(|s| s.to_string())
            .unwrap_or_else(unresolved),
        "gateway" => b
            .role
            .as_deref()
            .and_then(|r| bounds.of_role(r))
            .map(|s| s.to_string())
            .unwrap_or_else(unresolved),
        "surface" => surface_gateway
            .get(b.name.as_str())
            .and_then(|gw| gw.strip_prefix("gateway-"))
            .and_then(|path| bounds.of_role_path(path))
            .map(|s| s.to_string())
            .unwrap_or_else(unresolved),
        // An adapter/worker declares the scope whose keys and journals it works on; `bam` and the
        // generic sweeps declare none, and belong to no business boundary by construction.
        _ => container
            .map(|c| c.integration_scopes.clone())
            .and_then(|s| from_scopes(&s))
            .unwrap_or_else(unresolved),
    }
}

/// One line of what the app HOSTS, shaped by what its family makes meaningful.
fn app_hosts(
    b: &BinSpec,
    model: &Model,
    api: &Api,
    views: &[SqlView],
    ops: &[(String, String, String)],
    container: Option<&Container>,
    surface_gateway: &BTreeMap<&str, &str>,
) -> String {
    match b.family {
        "actor" => {
            let mut s = format!("lane `{}`", b.actor.as_deref().unwrap_or("?"));
            if !b.ports.is_empty() {
                s.push_str(&format!(
                    "; ports {}",
                    b.ports.iter().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            s
        }
        "pm" => {
            let mut s = format!("saga `{}`", b.actor.as_deref().unwrap_or("?"));
            if b.mailboxed {
                s.push_str("; own mailbox lane");
            }
            if !b.ports.is_empty() {
                s.push_str(&format!(
                    "; ports {}",
                    b.ports.iter().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            s
        }
        "projector" => {
            let scope = b.scope.as_deref().unwrap_or("?");
            let (v, t) = scope_read_models(model, views, scope);
            format!("scope `{scope}`: {v} View_* + {t} projection table(s)")
        }
        "subgraph" => {
            let scope = b.scope.as_deref().unwrap_or("?");
            let count = |kind: &str| {
                ops.iter().filter(|(k, _, s)| k == kind && s == scope).count()
            };
            format!(
                "scope `{scope}`: {} quer(ies), {} mutation(s), {} subscription(s)",
                count("query"),
                count("mutation"),
                count("subscription")
            )
        }
        "gateway" => {
            let role = b.role.as_deref().unwrap_or("?");
            let authorized = api
                .queries
                .iter()
                .chain(api.subscriptions.iter())
                .filter(|q| q.roles.iter().any(|r| r == role))
                .count()
                + api.mutations.iter().filter(|m| m.roles.iter().any(|r| r == role)).count();
            format!(
                "`/{}/graphql`: routes {} field(s) to {} subgraph(s); {authorized} authorized for {role}",
                role_path(role),
                ops.len(),
                model.scopes.len()
            )
        }
        "surface" => {
            let gw = surface_gateway.get(b.name.as_str()).copied().unwrap_or("-");
            match container.and_then(|c| c.ingress_host.as_deref()) {
                Some(host) => format!("SSR + assets; speaks to `{gw}`; host {host}"),
                None => format!("SSR + assets; speaks to `{gw}`"),
            }
        }
        "adapter" => {
            let partner = b.partner.as_deref().map(partner_slug).unwrap_or_default();
            let host = container.and_then(|c| c.ingress_host.as_deref()).unwrap_or("-");
            let scopes = container
                .map(|c| c.integration_scopes.join(", "))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "-".to_string());
            format!("partner `{partner}`: {host}/adapters/{partner}; integration scope(s) {scopes}")
        }
        "worker" => match &b.schedule {
            Some(cron) => {
                let mut s = format!("CronJob `{cron}`");
                if b.suspended {
                    s.push_str(" (SUSPENDED)");
                }
                if !b.consumers.is_empty() {
                    s.push_str(&format!(
                        "; hosts consumer(s) {}",
                        b.consumers.iter().cloned().collect::<Vec<_>>().join(", ")
                    ));
                }
                s
            }
            None => "always-on projector; folds every scope's events".to_string(),
        },
        _ => String::new(),
    }
}

/// `(fold views, projection tables)` a scope's projector maintains — a read model belongs to the
/// scope folder that owns its source aggregate (the same tie `emit_actor_scopes` writes for the
/// runtime registries).
fn scope_read_models(model: &Model, views: &[SqlView], scope: &str) -> (usize, usize) {
    let owner = |aggregate: &str| -> Option<String> {
        model
            .origins
            .get(&("actors.yaml".to_string(), aggregate.to_string()))
            .or_else(|| {
                model.origins.get(&("processmanager.yaml".to_string(), aggregate.to_string()))
            })
            .cloned()
    };
    let mine = views.iter().filter(|v| owner(&v.aggregate).as_deref() == Some(scope));
    (mine.clone().filter(|v| !v.is_table).count(), mine.filter(|v| v.is_table).count())
}

/// Secret keys the bin's OWN hosted consumer declares it reads, that its pod does not carry —
/// with the reason. `worker-sirene-sync` is the live case: `SireneClient::from_env` fails without
/// `INSEE_API_TOKEN`, the key declares no production `from_secret` (GitHub Actions still injects
/// it), and NOTHING else in the repo states that this pod needs it. Correct today, a trap at the
/// #358 cutover — so the index says it out loud.
pub(crate) fn missing_secret_keys(
    b: &BinSpec,
    config_keys: &[ConfigKey],
    granted: &[String],
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for k in config_keys {
        if !k.secret || k.consumer == "server" || !b.consumers.contains(&k.consumer) {
            continue;
        }
        if granted.iter().any(|g| *g == k.name) {
            continue;
        }
        let why = if k.from_secret.contains_key("production") {
            "granted to no scope of this pod".to_string()
        } else {
            "no production `from_secret` (injected outside the cluster)".to_string()
        };
        out.push((k.name.clone(), why));
    }
    out.sort();
    out
}

// ─── The rendered index ─────────────────────────────────────────────────────────────────────────

fn scope_list(scopes: &BTreeSet<String>, all: &[String]) -> String {
    if scopes.is_empty() {
        return "--".to_string();
    }
    if scopes.len() == all.len() {
        return format!("**all {}**", all.len());
    }
    scopes.iter().cloned().collect::<Vec<_>>().join(" + ")
}

/// `specs/generated/apps.generated.md`.
pub(crate) fn emit_app_index(model: &Model, graph: &CrateGraph) -> String {
    let rows = app_rows(model, graph);
    let bounds = boundary_map(model);
    let all_scopes: Vec<String> = crate_scopes(model).into_iter().collect();
    let mut out = String::new();

    out.push_str("<!-- GENERATED by the Captain.Food codegen (tools/codegen-rs, emit/app_index.rs) -- do not edit by hand. -->\n\n");
    out.push_str("# Captain.Food -- the app index\n\n");
    out.push_str(&format!(
        "**{} deployable apps.** Every column below is derived: the app list from the c4-l2 container topology, the boundary from `specs/architecture/c4-l2.yaml` `boundedContexts` (five business contexts + `platform`), the DECLARED domain crates from each bin's generated manifest, and the RESOLVED ones by MEASURING the workspace crate graph with cargo's own resolver (normal links, workspace members). `check-drift` holds this file to all of it.\n\n",
        rows.len()
    ));
    out.push_str("Read it for one question -- **is the split clean?** It is clean when no app spans two business boundaries AND every app's resolved domain set equals its declared one. Sections 2 and 3 answer both without arithmetic.\n\n");

    // ── 1. Families ────────────────────────────────────────────────────────────────────────────
    out.push_str("## 1. The families\n\n");
    out.push_str("Three states, exhaustive and exclusive. **`honest`** = measured, and the image links exactly the domain crates its manifest declares. **`fat`** = measured, and it links MORE than it declares -- the crates are in the image, the manifest never names them. **`unmeasured`** = cargo does not know this deployable at all, so nothing is claimed about it: absence of evidence is not evidence, and it is never counted as fat.\n\n");
    out.push_str("| family | apps | honest | fat | unmeasured | what one app of this family IS |\n|---|---:|---:|---:|---:|---|\n");
    for fam in FAMILY_ORDER {
        let mine: Vec<&AppRow> = rows.iter().filter(|r| r.family == *fam).collect();
        if mine.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "| `{fam}` | {} | {} | {} | {} | {} |\n",
            mine.len(),
            mine.iter().filter(|r| r.honest()).count(),
            mine.iter().filter(|r| r.fat()).count(),
            mine.iter().filter(|r| !r.measured).count(),
            family_blurb(fam)
        ));
    }
    let honest_total = rows.iter().filter(|r| r.honest()).count();
    let fat_total = rows.iter().filter(|r| r.fat()).count();
    let unmeasured_total = rows.iter().filter(|r| !r.measured).count();
    out.push_str(&format!(
        "| **total** | **{}** | **{}** | **{}** | **{}** | |\n\n",
        rows.len(),
        honest_total,
        fat_total,
        unmeasured_total
    ));

    // ── 2. Boundaries ──────────────────────────────────────────────────────────────────────────
    out.push_str("## 2. Per boundary -- is the split clean?\n\n");
    out.push_str("`honest`/`fat`/`unmeasured` as defined in section 1. `cross-boundary (declared)` = apps whose DECLARED crates reach more than one business boundary -- a bridge, legitimate for a process manager, a smell anywhere else. `cross-boundary (resolved)` = the same question asked of what actually links.\n\n");
    out.push_str("| boundary | apps | honest | fat | unmeasured | cross-boundary (declared) | cross-boundary (resolved) |\n|---|---:|---:|---:|---:|---:|---:|\n");
    let mut ids = bounds.ids.clone();
    ids.sort();
    for bc in &ids {
        let mine: Vec<&AppRow> = rows.iter().filter(|r| r.boundary == *bc).collect();
        if mine.is_empty() {
            continue;
        }
        let dspan = mine.iter().filter(|r| r.declared_span(&bounds).len() > 1).count();
        let rspan = mine.iter().filter(|r| r.resolved_span(&bounds).len() > 1).count();
        out.push_str(&format!(
            "| **{bc}** | {} | {} | {} | {} | {} | {} |\n",
            mine.len(),
            mine.iter().filter(|r| r.honest()).count(),
            mine.iter().filter(|r| r.fat()).count(),
            mine.iter().filter(|r| !r.measured).count(),
            dspan,
            rspan
        ));
    }
    let dspan_all: Vec<&AppRow> =
        rows.iter().filter(|r| r.declared_span(&bounds).len() > 1).collect();
    let rspan_all = rows.iter().filter(|r| r.resolved_span(&bounds).len() > 1).count();
    out.push_str(&format!(
        "| **total** | **{}** | **{}** | **{}** | **{}** | **{}** | **{}** |\n\n",
        rows.len(),
        honest_total,
        fat_total,
        unmeasured_total,
        dspan_all.len(),
        rspan_all
    ));
    out.push_str("**The verdicts:**\n\n");
    out.push_str(&format!(
        "- **No app spans two boundaries** -- {}. {} of {} apps DECLARE crates from more than one business boundary{}. On the graph the build actually resolves, {} do.\n",
        if dspan_all.is_empty() { "true as declared" } else { "not yet" },
        dspan_all.len(),
        rows.len(),
        if dspan_all.is_empty() {
            String::new()
        } else {
            format!(
                " ({})",
                dspan_all
                    .iter()
                    .map(|r| format!(
                        "`{}` = {}",
                        r.name,
                        r.declared_span(&bounds).into_iter().collect::<Vec<_>>().join("+")
                    ))
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        },
        rspan_all
    ));
    out.push_str(&format!(
        "- **resolved == declared** -- {} of {} apps. {} link domain crates their manifest never names{}.\n",
        honest_total,
        rows.len(),
        fat_total,
        if unmeasured_total == 0 {
            String::new()
        } else {
            format!(
                "; {unmeasured_total} are UNMEASURED (not workspace members cargo knows), so this index claims nothing about them"
            )
        }
    ));
    // WHERE the repair goes: how many apps each DIRECT dependency carries the facade into. One
    // crate carrying most of them is not a detail, it is the shape of the work.
    let mut carriers: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &rows {
        for c in &r.facade_carriers {
            *carriers.entry(c.as_str()).or_default() += 1;
        }
    }
    let mut carriers: Vec<(&str, usize)> = carriers.into_iter().collect();
    carriers.sort_by_key(|(c, n)| (std::cmp::Reverse(*n), *c));
    out.push_str(&format!(
        "- **Which dependency carries the facade in** -- {}. Splitting the crate at the top of that list is the single largest move available; section 4's `via` column says which apps it would free.\n",
        carriers.iter().map(|(c, n)| format!("`{c}` into {n}")).collect::<Vec<_>>().join(", ")
    ));
    // A gateway's boundary is its ROLE's context. A role no context claims is not an error -- it
    // is an undeclared one, and the row lands on `platform` by default. Say which, so nobody reads
    // the default as a decision.
    let unclaimed: BTreeSet<&str> = rows
        .iter()
        .filter(|r| r.family == "gateway")
        .filter_map(|r| r.role.as_deref())
        .filter(|role| bounds.role_claimants(role).is_empty())
        .collect();
    if !unclaimed.is_empty() {
        out.push_str(&format!(
            "- **Roles no bounded context claims** -- {}. Their gateways are indexed under `{PLATFORM_BOUNDARY}` because nothing else is derivable, NOT because a context decided so; a `roles:` entry in `c4-l2.yaml` moves them with no edit here.\n",
            unclaimed.iter().map(|r| format!("`{r}`")).collect::<Vec<_>>().join(", ")
        ));
    }
    // A decision that is CLOSED in the register and not yet in the spec the index reads. The index
    // renders the spec, so it renders the pre-decision answer -- and says so, with the move it
    // will make. Derived from the spec: the line disappears when c4 catches up.
    if let (Some(home), Some(row)) = (
        bounds.of_actor.get(CART_BINDING_PROCESS),
        rows.iter().find(|r| r.family == "pm" && r.actor.as_deref() == Some(CART_BINDING_PROCESS)),
    ) {
        if home != CART_BINDING_DECIDED_HOME {
            let count = |bc: &str| rows.iter().filter(|r| r.boundary == bc).count();
            out.push_str(&format!(
                "- **One row is decided and not yet spec'd** -- DECISIONS.md D9 closes `{CART_BINDING_PROCESS}` into `{CART_BINDING_DECIDED_HOME}`, and `specs/architecture/c4-l2.yaml` still homes it in `{home}`. This index renders the SPEC, so `{}` reads `{home}` above; when that one-liner lands the table reads {home} {} -> {}, {CART_BINDING_DECIDED_HOME} {} -> {}, and this bullet disappears.\n",
                row.name,
                count(home),
                count(home) - 1,
                count(CART_BINDING_DECIDED_HOME),
                count(CART_BINDING_DECIDED_HOME) + 1,
            ));
        }
    }
    out.push('\n');

    // ── 3. Shared crates ───────────────────────────────────────────────────────────────────────
    out.push_str("## 3. What the boundaries share\n\n");
    out.push_str("Every workspace crate any app resolves, by the set of boundaries whose apps reach it AND by HOW MANY apps do.\n\n");
    // Two measurements per crate: WHICH boundaries reach it, and HOW MANY apps do. The first
    // saturates and the second does not -- see the paragraph below, which is why both are grouped
    // on and why the count is a column and not a footnote.
    let mut reach: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut app_reach: BTreeMap<String, usize> = BTreeMap::new();
    for r in &rows {
        for c in graph.closure(&r.name) {
            reach.entry(c.clone()).or_default().insert(r.boundary.clone());
            *app_reach.entry(c).or_default() += 1;
        }
    }
    let mut by_sig: BTreeMap<(Vec<String>, usize), Vec<String>> = BTreeMap::new();
    for (krate, bcs) in &reach {
        let key = (bcs.iter().cloned().collect::<Vec<_>>(), app_reach[krate]);
        by_sig.entry(key).or_default().push(krate.clone());
    }
    let mut sigs: Vec<(&(Vec<String>, usize), &Vec<String>)> = by_sig.iter().collect();
    sigs.sort_by_key(|((sig, apps), crates)| {
        (std::cmp::Reverse(sig.len()), std::cmp::Reverse(*apps), crates.len())
    });
    let exclusive: usize = sigs.iter().filter(|((sig, _), _)| sig.len() == 1).map(|(_, c)| c.len()).sum();
    // The boundary signature's CEILING and what clears it alone. `graphql-*` is one app per scope
    // and those apps' boundaries already cover the whole set, so ANY crate a single subgraph links
    // reads "all boundaries". Stating the ceiling is what stops the column being over-read.
    let subgraph_boundaries: BTreeSet<&str> =
        rows.iter().filter(|r| r.family == "graphql").map(|r| r.boundary.as_str()).collect();
    let min_apps = app_reach.values().copied().min().unwrap_or(0);
    let max_apps = app_reach.values().copied().max().unwrap_or(0);
    out.push_str(&format!(
        "**No crate the apps reach is boundary-EXCLUSIVE: {} of {} are linked from at least one app of every boundary; {} belong to exactly one.** Read that as what it says and no more. The boundary column's ceiling is {} and one family clears it on its own -- the {} `graphql-*` subgraphs are one app per scope and between them cover {} of the {} boundaries, so any crate a single subgraph links already scores the maximum. **The `apps` column is the one with resolution**: it counts how many of the {} deployables actually link the crate, and it ranges from {} to {}.\n\n",
        reach.len() - exclusive,
        reach.len(),
        exclusive,
        bounds.ids.len(),
        rows.iter().filter(|r| r.family == "graphql").count(),
        subgraph_boundaries.len(),
        bounds.ids.len(),
        rows.len(),
        min_apps,
        max_apps,
    ));
    out.push_str(&format!("| reached by | apps (of {}) | crates | which |\n|---|---:|---:|---|\n", rows.len()));
    for ((sig, apps), crates) in sigs {
        out.push_str(&format!(
            "| {} boundar{} ({}) | {} | {} | {} |\n",
            sig.len(),
            if sig.len() == 1 { "y" } else { "ies" },
            sig.join(", "),
            apps,
            crates.len(),
            crates.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(", ")
        ));
    }
    out.push('\n');
    // The complement, which no other artifact states: workspace members NO deployable links. An
    // index asked for "all dependencies we need for a clean split" has to name what is not one.
    let app_names: BTreeSet<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    let orphans: Vec<&str> = graph
        .members()
        .into_iter()
        .filter(|m| !app_names.contains(m) && !reach.contains_key(*m))
        .collect();
    if !orphans.is_empty() {
        let clients: Vec<&&str> = orphans.iter().filter(|c| c.starts_with("client-")).collect();
        out.push_str(&format!(
            "**Reached by no deployable** -- {} workspace crate(s): {}. {}\n\n",
            orphans.len(),
            orphans.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(", "),
            if clients.is_empty() {
                "Codegen, gates and the desktop shell are expected here; anything else is a crate the images do not ship.".to_string()
            } else {
                format!(
                    "Codegen, gates and the desktop shell are expected here. {} is not: a generated actor client no app links is either an actor with no caller or a wiring gap, and this is the only artifact that would show it.",
                    clients.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(", ")
                )
            }
        ));
    }

    // ── 4. The apps ────────────────────────────────────────────────────────────────────────────
    out.push_str("## 4. The apps\n\n");
    out.push_str("`declared` = the bin manifest's domain-crate list (by scope). `resolved` = every `domain-*` crate in the measured closure. `via` = the DIRECT dependencies through which the `domain` facade enters the image -- an empty cell is an app whose closure has no domain vocabulary at all. `secrets` = secret-sourced env keys in its generated pod (`deploy/generated/manifests/bins/{app}.yaml`), `missing` = a key the app's own hosted consumer declares it reads and the pod does not carry.\n\n");
    for fam in FAMILY_ORDER {
        let mine: Vec<&AppRow> = rows.iter().filter(|r| r.family == *fam).collect();
        if mine.is_empty() {
            continue;
        }
        out.push_str(&format!("### `{fam}` -- {} app(s)\n\n", mine.len()));
        out.push_str("| app | boundary | hosts | declared | resolved | honest | via | secrets | missing |\n|---|---|---|---|---|---|---|---:|---|\n");
        for r in mine {
            let honest = if r.honest() {
                "yes".to_string()
            } else if !r.measured {
                "**unmeasured**".to_string()
            } else {
                format!("**fat +{}**", r.resolved.difference(&r.declared).count())
            };
            let via = if r.facade_carriers.is_empty() {
                "--".to_string()
            } else {
                r.facade_carriers.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(", ")
            };
            let missing = if r.missing_secrets.is_empty() {
                "--".to_string()
            } else {
                r.missing_secrets
                    .iter()
                    .map(|(k, why)| format!("**{k}** ({why})"))
                    .collect::<Vec<_>>()
                    .join("; ")
            };
            out.push_str(&format!(
                "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                r.name,
                r.boundary,
                r.hosts,
                scope_list(&r.declared, &all_scopes),
                scope_list(&r.resolved, &all_scopes),
                honest,
                via,
                r.secrets.len(),
                missing
            ));
        }
        out.push('\n');
    }

    // ── 5. Grants ──────────────────────────────────────────────────────────────────────────────
    out.push_str("## 5. Secret grants\n\n");
    out.push_str("The distinct pod grants, largest first: what a compromise of any app in that group could read. A grant is derived from the app's config scopes (its own + the kernel + its declared integration scopes), narrowed for the adapter family to its partner's key prefix and for the periodic-worker family to the database + telemetry floor.\n\n");
    let mut grants: BTreeMap<Vec<String>, Vec<&AppRow>> = BTreeMap::new();
    for r in &rows {
        grants.entry(r.secrets.clone()).or_default().push(r);
    }
    let mut grants: Vec<(&Vec<String>, &Vec<&AppRow>)> = grants.iter().collect();
    grants.sort_by_key(|(keys, apps)| (std::cmp::Reverse(keys.len()), apps.len()));
    out.push_str("| keys | apps | held by | the keys |\n|---:|---:|---|---|\n");
    for (keys, apps) in grants {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            keys.len(),
            apps.len(),
            apps.iter().map(|a| format!("`{}`", a.name)).collect::<Vec<_>>().join(", "),
            if keys.is_empty() {
                "--".to_string()
            } else {
                keys.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ")
            }
        ));
    }
    out.push('\n');
    let missing: Vec<&AppRow> = rows.iter().filter(|r| !r.missing_secrets.is_empty()).collect();
    if missing.is_empty() {
        out.push_str("No app declares a hosted consumer whose secret keys its pod lacks.\n");
    } else {
        out.push_str("**Needed and not granted** -- each of these apps hosts a consumer that reads a key its pod does not carry:\n\n");
        for r in &missing {
            for (k, why) in &r.missing_secrets {
                out.push_str(&format!("- `{}` needs `{k}` -- {why}.\n", r.name));
            }
        }
    }
    out.push('\n');
    out
}

/// One line per family — what one app of it IS, not what this repo happens to have.
fn family_blurb(fam: &str) -> &'static str {
    match fam {
        "actor" => "a mailbox worker draining ONE aggregate's lanes -- the one-writer-per-aggregate promise, deployed",
        "pm" => "a mailbox worker running ONE saga; a declared cross-scope bridge",
        "projector" => "a projection worker folding the single log, filtered to its scope, on its own checkpoint (D4)",
        "worker" => "a periodic pass; the declared cadence renders a CronJob, no cadence an always-on Deployment",
        "adapter" => "one partner ACL: verify, mirror into that partner's journal, translate, enqueue",
        "graphql" => "one scope's GraphQL subgraph -- queries over its views, mutations onto its lanes (D8)",
        "gateway" => "one role path `/{role}/graphql`: routing only -- no DB, no state, no domain vocabulary",
        "fo" => "a front-office surface: assets + SSR for one public audience, speaking only to its gateway",
        "bo" => "a back-office surface: assets + SSR for one operator audience",
        "bam" => "the business-activity projector -- a cross-scope consumer BY DESIGN",
        _ => "",
    }
}
