//! §14 — Per-scope spec folders (ADR-20260807-183024 D1/D5/D8, #375).
//!
//! Four rule families make the scope split REAL rather than cosmetic. All of them work from
//! `Model::origins` (which folder each merged item came from) — refs stay kind-logical, so folder
//! placement is the ONLY place scope membership lives, and these gates are what keep it honest:
//!
//! 1. **Placement** (`scope-placement-*`): an item's folder must match the scope DERIVED from the
//!    actor wiring — a command lives with its handling actor, an event with its authoring actor
//!    (echo-records excluded: an aggregate re-emitting the event it received RECORDS it, it does
//!    not author it), an error with its throwing actor, an api mutation with its command, an api
//!    type with its read-model's aggregate, a query/subscription with its return type. Placement
//!    in `common/` is always allowed (kernel PROMOTION is a legitimate design act — a cross-scope
//!    contract); what is forbidden is placing an item in a WRONG scope. Multi-scope derivation
//!    (several authors/handlers/throwers across scopes) REQUIRES common.
//! 2. **Cross-scope `$ref` DAG** (`scope-cycle`): the scope-level reference graph over NON-process-
//!    manager sources must be acyclic. Process managers are declared bridges (PROP-20260807-174246
//!    §2: "cross-scope PM edges are DECLARED edges") — an orchestrator legitimately closes loops
//!    between scopes, and its refs become the pm-crate dependency list at #373 instead.
//! 3. **Kernel purity** (`scope-kernel-purity`): an item in `common/` may reference only `common/`
//!    items — the kernel depends on no scope, ever. This is what makes promotion disciplined.
//! 4. **API nested-intra-scope** (`api-nested-cross-scope`, D8): an api TYPE in scope S may nest
//!    only S or kernel types — cross-scope data appears at TOP LEVEL (gateway routing) or
//!    pre-joined in a projector-owned view, never as a nested subgraph. This is the gate that
//!    keeps codegen-time stitching cheap (no entity resolution, no N+1).

use crate::*;

pub(crate) const KERNEL_SCOPE: &str = "common";

/// The origin folder of a merged item, if it came from a scope fragment. Items loaded from a flat
/// root catalog (test fixtures, transitional layouts) have no origin and are exempt from placement.
fn origin<'a>(model: &'a Model, kind: &str, item: &str) -> Option<&'a str> {
    model.origins.get(&(kind.to_string(), item.to_string())).map(|s| s.as_str())
}

/// Resolve a `$ref` string (appearing in logical file `ctx`) to the `(logical file, origin item
/// key)` its FIRST pointer segment denotes — `"keys/NAME"`-style for section kinds.
fn ref_item(r: &str, ctx: &str) -> Option<(String, String)> {
    let pr = parse_ref(r)?;
    let file = if pr.file.is_empty() { ctx.to_string() } else { pr.file };
    let key = if SCOPED_SECTION_KINDS.iter().any(|(k, _)| *k == file) {
        if pr.path.len() >= 2 { format!("{}/{}", pr.path[0], pr.path[1]) } else { return None }
    } else {
        pr.path.first()?.clone()
    };
    Some((file, key))
}

/// Actor wiring sets, mirroring the migration derivation exactly: which scopes handle each
/// command, author each event (receive-level `emits` minus echo-records, lifecycle events,
/// deletion receipts), handle each event, and throw each error (receive-level `throws`).
#[derive(Default)]
struct Wiring {
    cmd_handlers: BTreeMap<String, BTreeSet<String>>,
    evt_authors: BTreeMap<String, BTreeSet<String>>,
    evt_handlers: BTreeMap<String, BTreeSet<String>>,
    err_throwers: BTreeMap<String, BTreeSet<String>>,
}

fn actor_wiring(model: &Model) -> Wiring {
    let mut w = Wiring::default();
    for file in ["actors.yaml", "processmanager.yaml"] {
        let Some(Value::Mapping(m)) = model.defs.get(file) else { continue };
        for (k, def) in m {
            let Some(name) = k.as_str() else { continue };
            let Some(scope) = origin(model, file, name).map(|s| s.to_string()) else { continue };
            if let Some(seq) = def.get("receives").and_then(|x| x.as_sequence()) {
                for e in seq {
                    let msg = e
                        .get("message")
                        .and_then(|mm| mm.get("$ref"))
                        .and_then(|r| r.as_str())
                        .unwrap_or("");
                    let recv_evt = if msg.starts_with("events.yaml#/") { ref_name(msg) } else { None };
                    if msg.starts_with("commands.yaml#/") {
                        if let Some(c) = ref_name(msg) {
                            w.cmd_handlers.entry(c).or_default().insert(scope.clone());
                        }
                    }
                    if let Some(ev) = &recv_evt {
                        w.evt_handlers.entry(ev.clone()).or_default().insert(scope.clone());
                    }
                    if let Some(emits) = e.get("emits").and_then(|x| x.as_sequence()) {
                        for em in emits {
                            let Some(n) = em.get("$ref").and_then(|r| r.as_str()).and_then(ref_name)
                            else {
                                continue;
                            };
                            // Echo-record exclusion: re-emitting the received event records it.
                            if recv_evt.as_deref() == Some(n.as_str()) {
                                continue;
                            }
                            w.evt_authors.entry(n).or_default().insert(scope.clone());
                        }
                    }
                    if let Some(throws) = e.get("throws") {
                        let mut refs = Vec::new();
                        collect_refs(throws, "", &mut refs);
                        for (_, r) in refs {
                            if r.starts_with("errors.yaml#/") {
                                if let Some(n) = ref_name(&r) {
                                    w.err_throwers.entry(n).or_default().insert(scope.clone());
                                }
                            }
                        }
                    }
                }
            }
            if let Some(lc) = def.get("lifecycle") {
                for sec in ["initial", "transitions"] {
                    if let Some(seq) = lc.get(sec).and_then(|x| x.as_sequence()) {
                        for t in seq {
                            if let Some(n) = t
                                .get("event")
                                .and_then(|x| x.get("$ref"))
                                .and_then(|r| r.as_str())
                                .and_then(ref_name)
                            {
                                w.evt_authors.entry(n).or_default().insert(scope.clone());
                            }
                        }
                    }
                }
            }
            if let Some(n) = def
                .get("deletion")
                .and_then(|d| d.get("receipt"))
                .and_then(|x| x.get("$ref"))
                .and_then(|r| r.as_str())
                .and_then(ref_name)
            {
                w.evt_authors.entry(n).or_default().insert(scope.clone());
            }
        }
    }
    w
}

/// Derived placement: `None` = underivable (no wiring — placement free). `Some(scope)` = the one
/// scope the folder must match (or `common`); `Some(KERNEL_SCOPE)` = several scopes derive it, so
/// only the kernel is a legal home.
fn derived(sets: Option<&BTreeSet<String>>) -> Option<&str> {
    let s = sets?;
    match s.len() {
        0 => None,
        1 => s.iter().next().map(|x| x.as_str()),
        _ => Some(KERNEL_SCOPE),
    }
}

fn check_placement(
    issues: &mut Vec<Issue>,
    rule: &'static str,
    kind: &str,
    item: &str,
    folder: &str,
    want: Option<&str>,
    what: &str,
) {
    let Some(want) = want else { return };
    if folder == want || folder == KERNEL_SCOPE {
        return;
    }
    issues.push(err(
        rule,
        format!("{}/{}/{}", folder, kind, item),
        format!(
            "'{}' derives scope '{}' ({}) but lives in specs/{}/ — move it to specs/{}/ (or promote it to specs/common/ if it is a cross-scope contract).",
            item, want, what, folder, want
        ),
    ));
}

/// Is this actor/PM item a process manager (a declared cross-scope bridge)?
fn is_pm(model: &Model, file: &str, name: &str) -> bool {
    file == "processmanager.yaml"
        || model
            .defs
            .get(file)
            .and_then(|m| m.get(name))
            .and_then(|d| d.get("type"))
            .and_then(|t| t.as_str())
            == Some("process-manager")
}

pub(crate) fn validate_scopes(model: &Model, issues: &mut Vec<Issue>) {
    if model.scopes.is_empty() {
        return; // flat layout (unit fixtures) — nothing scope-shaped to check
    }
    let w = actor_wiring(model);

    // ── 1. Placement ────────────────────────────────────────────────────────────────────────────
    // An item defined in BOTH actors.yaml and processmanager.yaml (a PM with a mailbox entry)
    // must sit in ONE scope folder — a split-brain PM is two different deploy targets.
    if let (Some(Value::Mapping(am)), Some(_)) =
        (model.defs.get("actors.yaml"), model.defs.get("processmanager.yaml"))
    {
        for (k, _) in am {
            let Some(name) = k.as_str() else { continue };
            if let (Some(a), Some(p)) =
                (origin(model, "actors.yaml", name), origin(model, "processmanager.yaml", name))
            {
                if a != p {
                    issues.push(err(
                        "scope-placement-actor-pm",
                        format!("{}/actors.yaml/{}", a, name),
                        format!(
                            "'{}' sits in specs/{}/actors.yaml but specs/{}/processmanager.yaml — one actor, one scope folder.",
                            name, a, p
                        ),
                    ));
                }
            }
        }
    }
    for (kind, sets, rule, what) in [
        ("commands.yaml", &w.cmd_handlers, "scope-placement-command", "its handling actor's scope"),
        ("errors.yaml", &w.err_throwers, "scope-placement-error", "its throwing actor's scope"),
    ] {
        if let Some(Value::Mapping(m)) = model.defs.get(kind) {
            for (k, _) in m {
                let Some(name) = k.as_str() else { continue };
                let Some(folder) = origin(model, kind, name) else { continue };
                check_placement(issues, rule, kind, name, folder, derived(sets.get(name)), what);
            }
        }
    }
    if let Some(Value::Mapping(m)) = model.defs.get("events.yaml") {
        for (k, _) in m {
            let Some(name) = k.as_str() else { continue };
            let Some(folder) = origin(model, "events.yaml", name) else { continue };
            // Authors first (who writes the fact); pure inbound facts fall back to handlers.
            let want = derived(w.evt_authors.get(name)).or_else(|| derived(w.evt_handlers.get(name)));
            check_placement(
                issues,
                "scope-placement-event",
                "events.yaml",
                name,
                folder,
                want,
                "its authoring actor's scope (echo-records excluded; inbound facts: the handling scope)",
            );
        }
    }
    // API placement: mutation ↔ command, type-with-reads ↔ its view's aggregate, operation ↔ its
    // return type. (Section keys are `"{section}/{name}"` in origins.)
    let api_origin = |sec: &str, name: &str| origin(model, "api.yaml", &format!("{}/{}", sec, name));
    let actor_scope = |name: &str| {
        origin(model, "actors.yaml", name).or_else(|| origin(model, "processmanager.yaml", name))
    };
    if let Some(api) = model.defs.get("api.yaml") {
        if let Some(Value::Mapping(muts)) = api.get("mutations") {
            for (k, def) in muts {
                let Some(name) = k.as_str() else { continue };
                let Some(folder) = api_origin("mutations", name) else { continue };
                let cmd = def
                    .get("command")
                    .and_then(|c| c.get("$ref"))
                    .and_then(|r| r.as_str())
                    .and_then(ref_name);
                let want = cmd.as_deref().and_then(|c| origin(model, "commands.yaml", c));
                check_placement(
                    issues,
                    "scope-placement-api",
                    "api.yaml/mutations",
                    name,
                    folder,
                    want,
                    "its command's scope",
                );
            }
        }
        if let Some(Value::Mapping(types)) = api.get("types") {
            for (k, def) in types {
                let Some(name) = k.as_str() else { continue };
                let Some(folder) = api_origin("types", name) else { continue };
                let Some(reads) = def.get("reads") else { continue };
                let mut refs = Vec::new();
                collect_refs(reads, "", &mut refs);
                let mut agg_scopes: BTreeSet<String> = BTreeSet::new();
                for (_, r) in refs {
                    // reads → a database view/table; its `aggregate:` names the owning actor.
                    if let Some((file, view)) = ref_item(&r, "api.yaml") {
                        if let Some(agg) = model
                            .defs
                            .get(&file)
                            .and_then(|d| d.get(view.as_str()))
                            .and_then(|v| v.get("aggregate"))
                            .and_then(|a| a.as_str())
                        {
                            if let Some(s) = actor_scope(agg) {
                                agg_scopes.insert(s.to_string());
                            }
                        }
                    }
                }
                check_placement(
                    issues,
                    "scope-placement-api",
                    "api.yaml/types",
                    name,
                    folder,
                    derived(Some(&agg_scopes)),
                    "the scope of its read-model's aggregate",
                );
            }
        }
        for sec in ["queries", "subscriptions"] {
            if let Some(Value::Mapping(ops)) = api.get(sec) {
                for (k, def) in ops {
                    let Some(name) = k.as_str() else { continue };
                    let Some(folder) = api_origin(sec, name) else { continue };
                    let ret = def
                        .get("returns")
                        .and_then(|r| r.get("$ref"))
                        .and_then(|r| r.as_str())
                        .and_then(|r| r.strip_prefix("#/types/"))
                        .map(|s| s.to_string());
                    let want = ret.as_deref().and_then(|t| api_origin("types", t));
                    // A kernel return type constrains nothing — any scope may serve it.
                    if want == Some(KERNEL_SCOPE) {
                        continue;
                    }
                    check_placement(
                        issues,
                        "scope-placement-api",
                        &format!("api.yaml/{}", sec),
                        name,
                        folder,
                        want,
                        "its return type's scope",
                    );
                }
            }
        }
    }

    // ── 2 + 3. Cross-scope DAG (non-PM sources) and kernel purity ──────────────────────────────
    // One walk over every PLACED item's definition: resolve each ref to its target's origin.
    let mut edge_witness: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut scan = |model: &Model,
                    issues: &mut Vec<Issue>,
                    file: &str,
                    item_key: &str,
                    def: &Value,
                    src_scope: &str,
                    pm: bool| {
        let mut refs = Vec::new();
        collect_refs(def, "", &mut refs);
        for (_, r) in refs {
            let Some((tf, tk)) = ref_item(&r, file) else { continue };
            let Some(tsc) = origin(model, &tf, &tk) else { continue };
            if tsc == src_scope {
                continue;
            }
            if src_scope == KERNEL_SCOPE {
                issues.push(err(
                    "scope-kernel-purity",
                    format!("common/{}/{}", file, item_key),
                    format!(
                        "kernel item '{}' references {}#/{} which lives in specs/{}/ — common/ depends on no scope; move the target to common/ or the item out of it.",
                        item_key, tf, tk, tsc
                    ),
                ));
                continue;
            }
            if tsc == KERNEL_SCOPE || pm {
                continue; // kernel edges are free; PM bridges are declared multi-scope (#373)
            }
            edge_witness
                .entry((src_scope.to_string(), tsc.to_string()))
                .or_insert_with(|| format!("{}#{} → {}#{}", file, item_key, tf, tk));
        }
    };
    for ((file, item_key), scope) in &model.origins {
        let def = match file.as_str() {
            "api.yaml" | "configuration.yaml" => {
                let (sec, name) = item_key.split_once('/').unwrap_or((item_key.as_str(), ""));
                model.defs.get(file).and_then(|d| d.get(sec)).and_then(|s| s.get(name))
            }
            _ => model.defs.get(file).and_then(|d| d.get(item_key.as_str())),
        };
        let Some(def) = def else { continue };
        let pm = matches!(file.as_str(), "actors.yaml" | "processmanager.yaml")
            && is_pm(model, file, item_key);
        scan(model, issues, file, item_key, def, scope, pm);
    }
    // Cycle detection over the scope graph (DFS, deterministic order).
    let mut g: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (a, b) in edge_witness.keys() {
        g.entry(a.as_str()).or_default().insert(b.as_str());
    }
    let mut color: BTreeMap<&str, u8> = BTreeMap::new();
    fn visit<'a>(
        u: &'a str,
        g: &BTreeMap<&'a str, BTreeSet<&'a str>>,
        color: &mut BTreeMap<&'a str, u8>,
        path: &mut Vec<&'a str>,
        cycles: &mut Vec<Vec<String>>,
    ) {
        color.insert(u, 1);
        path.push(u);
        for &v in g.get(u).into_iter().flatten() {
            match color.get(v) {
                Some(1) => {
                    let start = path.iter().position(|&x| x == v).unwrap_or(0);
                    let mut cyc: Vec<String> = path[start..].iter().map(|s| s.to_string()).collect();
                    cyc.push(v.to_string());
                    cycles.push(cyc);
                }
                None => visit(v, g, color, path, cycles),
                _ => {}
            }
        }
        path.pop();
        color.insert(u, 2);
    }
    let mut cycles = Vec::new();
    for &u in g.keys() {
        if !color.contains_key(u) {
            let mut path = Vec::new();
            visit(u, &g, &mut color, &mut path, &mut cycles);
        }
    }
    for cyc in cycles {
        let witnesses: Vec<String> = cyc
            .windows(2)
            .filter_map(|p| edge_witness.get(&(p[0].clone(), p[1].clone())).cloned())
            .collect();
        issues.push(err(
            "scope-cycle",
            "specs/".to_string(),
            format!(
                "cross-scope $refs form a cycle: {} (via {}). Scopes must form a DAG — promote the shared contract to specs/common/ or move an item (process managers are exempt bridges).",
                cyc.join(" → "),
                witnesses.join("; ")
            ),
        ));
    }

    // ── 4. API nested-intra-scope (D8) ──────────────────────────────────────────────────────────
    if let Some(Value::Mapping(types)) = model.defs.get("api.yaml").and_then(|a| a.get("types")) {
        for (k, def) in types {
            let Some(name) = k.as_str() else { continue };
            let Some(folder) = api_origin("types", name) else { continue };
            if folder == KERNEL_SCOPE {
                continue; // kernel types are covered by kernel purity above
            }
            let mut refs = Vec::new();
            collect_refs(def, "", &mut refs);
            for (_, r) in refs {
                let Some(nested) = r.strip_prefix("#/types/") else { continue };
                let nested = nested.split('/').next().unwrap_or(nested);
                let Some(tsc) = api_origin("types", nested) else { continue };
                if tsc != folder && tsc != KERNEL_SCOPE {
                    issues.push(err(
                        "api-nested-cross-scope",
                        format!("{}/api.yaml/types/{}", folder, name),
                        format!(
                            "type '{}' ({}) nests type '{}' ({}): cross-scope data appears only at top level or pre-joined in a projector-owned view (D8) — denormalize into a view or move the type.",
                            name, folder, nested, tsc
                        ),
                    ));
                }
            }
        }
    }
}
