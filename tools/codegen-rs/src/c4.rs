use crate::*;

// ─── c4.generated.dsl + c4.generated.md (port of emit/c4.ts) ─────────────────────────────────────

pub(crate) struct Actor {
    pub(crate) name: String,
    pub(crate) kind: String, // "aggregate" | "process-manager"
    pub(crate) file: &'static str, // "actors.yaml" | "processmanager.yaml" (where the definition lives)
    pub(crate) description: Option<String>,
    pub(crate) receives: Vec<Receive>,
}
pub(crate) struct Receive {
    pub(crate) message_ref: String,
    pub(crate) emits: Vec<String>, // raw $ref strings
    pub(crate) throws: Vec<String>,
    /// Reminders this handler (re)schedules — raw same-actor `$ref`s (ADR-20260731-214500 §2).
    pub(crate) schedules: Vec<String>,
    pub(crate) effect: Option<String>,
}
pub(crate) struct Ctx {
    pub(crate) id: String,
    pub(crate) description: String,
    pub(crate) aggregates: Vec<String>,
    pub(crate) process_managers: Vec<String>,
    /// The `UserType` roles this bounded context serves (`roles:`) — the tie from a role path
    /// (and therefore a `gateway-{role}` bin and the surface that speaks to it) back to the
    /// business boundary that owns it. Absent on a context that serves no role directly.
    pub(crate) roles: Vec<String>,
}
/// Which GENERATED deployment tree a container belongs to (`deploy_tree:` in c4-l2, default
/// `bins`). Two trees exist ON PURPOSE while the cutover is in flight (ADR-20260807-183024 steps
/// (6)-(7)): the per-bin topology we are moving TO, and the monolith we are actually running.
/// `Unknown` is a distinct variant rather than a silent fall-back to `Bins`, so a typo'd value
/// lands in NEITHER tree and is caught by §15 instead of quietly deploying the wrong shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeployTree {
    /// `deploy/generated/manifests/` — one Deployment/CronJob per derived bin (#349/#385).
    Bins,
    /// `deploy/generated/monolith/` — the TRANSITIONAL single `server` process that serves every
    /// host and every role path today; retired when the bin topology takes over.
    Monolith,
    Unknown(String),
}

pub(crate) struct Container {
    pub(crate) id: String,
    pub(crate) technology: String,
    pub(crate) description: String,
    /// The generated deployment tree this container is emitted into (`deploy_tree:`).
    pub(crate) deploy_tree: DeployTree,
    /// Actor/PM names this container realizes (`realizes:` $refs) — the bin ↔ deployable binding
    /// (ADR-20260807-183024; consumed by the #349 emitter chain).
    pub(crate) realizes: Vec<String>,
    /// The dedicated Ingress host this container is served on (`ingress_host:`, #385): the spec
    /// home for the integration host — declared on `adapters`, consumed by the deploy emitter.
    pub(crate) ingress_host: Option<String>,
    /// The domain scopes whose CONFIGURATION KEYS this container's runtime needs
    /// (`integration_scopes:`, #385): the adapters surface hosts every partner ACL, so its pod
    /// env + generated Config carry the integration scopes' keys (webhook secrets live in
    /// payments/delivery/catalog). NOT yet validator-checked: a typo'd scope name silently
    /// drops those keys from the pod's env — the rule is tracked on #385 with the per-key
    /// consumer-metadata design (ADR-20260808-060309 consequences).
    pub(crate) integration_scopes: Vec<String>,
    /// The declared cadence of a periodic `worker-*` container (`schedule:`, 5-field cron in
    /// UTC — ADR-20260808-062933 "shape follows cadence"): present ⇒ the deploy emitter renders
    /// a CronJob; absent ⇒ an always-on Deployment. §15 requires it on every `worker-*`
    /// container and refuses it anywhere else.
    pub(crate) schedule: Option<String>,
    /// `suspended: true` renders the CronJob with `suspend: true` — visibly OFF (used while an
    /// external residence stays authoritative, e.g. sirene-sync.yml until the #358 cutover).
    pub(crate) suspended: bool,
}
pub(crate) struct External {
    pub(crate) id: String,
    pub(crate) description: String,
}
pub(crate) struct Rel {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) description: String,
}
pub(crate) struct Comp {
    pub(crate) id: String,
    pub(crate) description: String,
    pub(crate) instrumented: bool,
    /// The l2 container this component primarily runs in (`container:` — shared framework modules
    /// ship in many bins and declare their money-path instance).
    pub(crate) container: Option<String>,
}
pub(crate) struct C4 {
    pub(crate) system_name: String,
    pub(crate) system_description: String,
    pub(crate) contexts: Vec<Ctx>,
    pub(crate) containers: Vec<Container>,
    pub(crate) externals: Vec<External>,
    pub(crate) relationships: Vec<Rel>,
    pub(crate) components: Vec<Comp>,
}

pub(crate) const PIPELINE: &[(&str, &str, &str)] = &[
    ("graphql-gateway", "command-bus", "dispatches command"),
    ("command-bus", "command-handlers", "invokes handler"),
    ("command-handlers", "event-store-adapter", "appends events"),
    ("event-store-adapter", "event-publisher", "publishes appended"),
    ("event-publisher", "message-consumers", "delivers events"),
    ("message-consumers", "projection-updaters", "feeds projections"),
    ("process-managers", "command-bus", "issues commands"),
];

/// `${prefix}${s.replace(/[^a-zA-Z0-9]+/g, '_')}` — runs of non-alphanumerics collapse to a single `_`.
pub(crate) fn c4id(prefix: &str, s: &str) -> String {
    let mut out = String::from(prefix);
    let mut prev_us = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_us = false;
        } else if !prev_us {
            out.push('_');
            prev_us = true;
        }
    }
    out
}

/// `"${s.replace(/"/g,'\"').replace(/\s+/g,' ').trim()}"` — escape quotes, collapse whitespace, trim, wrap.
pub(crate) fn q(s: &str) -> String {
    let escaped = s.replace('"', "\\\"");
    let mut collapsed = String::new();
    let mut prev_ws = false;
    for ch in escaped.chars() {
        if ch.is_whitespace() {
            if !prev_ws {
                collapsed.push(' ');
                prev_ws = true;
            }
        } else {
            collapsed.push(ch);
            prev_ws = false;
        }
    }
    format!("\"{}\"", collapsed.trim())
}

pub(crate) fn ref_names(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_sequence())
        .map(|s| {
            s.iter()
                .filter_map(|it| it.get("$ref").and_then(|r| r.as_str()).and_then(ref_name))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn parse_actors(model: &Model) -> Vec<Actor> {
    let mut out = Vec::new();
    if let Some(Value::Mapping(m)) = model.defs.get("actors.yaml") {
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            let kind = match node.get("type").and_then(|x| x.as_str()) {
                Some(t @ ("aggregate" | "process-manager")) => t,
                _ => continue,
            };
            let mut receives = Vec::new();
            if let Some(seq) = node.get("receives").and_then(|x| x.as_sequence()) {
                for e in seq {
                    let message_ref = e
                        .get("message")
                        .and_then(|mm| mm.get("$ref"))
                        .and_then(|r| r.as_str())
                        .unwrap_or("")
                        .to_string();
                    let emits = ref_strings(e.get("emits"));
                    let throws = ref_strings(e.get("throws"));
                    let schedules = ref_strings(e.get("schedules"));
                    let effect = e.get("effect").and_then(|x| x.as_str()).map(|s| s.to_string());
                    receives.push(Receive { message_ref, emits, throws, schedules, effect });
                }
            }
            out.push(Actor {
                name: name.to_string(),
                kind: kind.to_string(),
                file: "actors.yaml",
                description: node.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()),
                receives,
            });
        }
    }
    // Process managers (processmanager.yaml, typed-step DSL) project into the same Actor shape with
    // DERIVED emits/throws per leg: emits = delivered events ∪ the emits of each sent command per the
    // target aggregate's inbox (actors.yaml stays the single wiring truth); throws = guard `throws`.
    let agg_emits: HashMap<(String, String), Vec<String>> = out
        .iter()
        .flat_map(|a| {
            a.receives.iter().filter_map(move |r| {
                ref_name(&r.message_ref).map(|m| ((a.name.clone(), m), r.emits.clone()))
            })
        })
        .collect();
    if let Some(Value::Mapping(m)) = model.defs.get("processmanager.yaml") {
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            if node.get("type").and_then(|x| x.as_str()) != Some("process-manager") {
                continue;
            }
            let mut receives = Vec::new();
            if let Some(seq) = node.get("receives").and_then(|x| x.as_sequence()) {
                for e in seq {
                    let message_ref = e
                        .get("message")
                        .and_then(|mm| mm.get("$ref"))
                        .and_then(|r| r.as_str())
                        .unwrap_or("")
                        .to_string();
                    let mut emits: Vec<String> = Vec::new();
                    let mut throws: Vec<String> = Vec::new();
                    if let Some(steps) = e.get("steps").and_then(|x| x.as_sequence()) {
                        for s in steps {
                            if let Some(d) = s.get("deliver") {
                                if let Some(ev) = d.get("event").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()) {
                                    if !emits.contains(&ev.to_string()) {
                                        emits.push(ev.to_string());
                                    }
                                }
                            }
                            if let Some(sd) = s.get("send") {
                                let cmd = sd.get("command").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name);
                                let to = sd.get("to").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name);
                                if let (Some(cmd), Some(to)) = (cmd, to) {
                                    if let Some(evs) = agg_emits.get(&(to, cmd)) {
                                        for ev in evs {
                                            if !emits.contains(ev) {
                                                emits.push(ev.clone());
                                            }
                                        }
                                    }
                                }
                            }
                            if let Some(g) = s.get("guard") {
                                if let Some(er) = g.get("throws").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()) {
                                    if !throws.contains(&er.to_string()) {
                                        throws.push(er.to_string());
                                    }
                                }
                            }
                        }
                    }
                    // Wrapper-seam arms (#159/#207): a leg may DECLARE additional emits/throws its
                    // hand-written wrapper produces (events/errors a linear step pipeline cannot express),
                    // merged with the step-derived set so behaviour-test coverage sees the full inbox.
                    for ev in ref_strings(e.get("emits")) {
                        if !emits.contains(&ev) {
                            emits.push(ev);
                        }
                    }
                    for er in ref_strings(e.get("throws")) {
                        if !throws.contains(&er) {
                            throws.push(er);
                        }
                    }
                    let effect = e.get("description").and_then(|x| x.as_str()).map(|s| s.to_string());
                    receives.push(Receive { message_ref, emits, throws, schedules: Vec::new(), effect });
                }
            }
            out.push(Actor {
                name: name.to_string(),
                kind: "process-manager".to_string(),
                file: "processmanager.yaml",
                description: node.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()),
                receives,
            });
        }
    }
    out
}

/// One Mermaid sequence diagram per process manager, generated from the typed steps
/// (processmanager.yaml). Participants map 1:1 to layers: the PM's pure decision, its private state
/// table, the read models (infrastructure read side), the outbound ports (adapters), and the target
/// aggregates (owners of the facts). A guard renders as a rejection arrow (command legs) or a skip
/// note (event legs) — so the diagram proves who may say "no" and who only records.
/// Returns (name → diagram body, in processmanager.yaml order); callers add their own framing
/// (Markdown fence, HTML <pre>), so one diagram source feeds every artifact.
pub(crate) fn pm_sequence_map(model: &Model) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let pms = match model.defs.get("processmanager.yaml") {
        Some(Value::Mapping(m)) => m,
        _ => return out,
    };
    let fmt_value = |v: &Value| -> String {
        if let Some(c) = v.get("const").and_then(|x| x.as_str()) {
            return c.to_string();
        }
        if let Some(f) = v.get("from").and_then(|f| f.get("$ref")).and_then(|x| x.as_str()) {
            let prop = f.rsplit('/').next().unwrap_or("?");
            return format!("{}.{}", ref_name(f).unwrap_or_default(), prop);
        }
        for (k, pfx) in [("from_state", "state."), ("from_read", ""), ("from_port", ""), ("from_envelope", "envelope.")] {
            if let Some(s) = v.get(k).and_then(|x| x.as_str()) {
                return format!("{}{}", pfx, s);
            }
        }
        "?".to_string()
    };
    let fmt_map = |v: Option<&Value>| -> String {
        v.and_then(|x| x.as_mapping())
            .map(|m| {
                m.iter()
                    .filter_map(|(k, val)| k.as_str().map(|c| format!("{}={}", c, fmt_value(val))))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    };
    for (k, node) in pms {
        let name = match k.as_str() {
            Some(s) => s,
            None => continue,
        };
        if node.get("type").and_then(|x| x.as_str()) != Some("process-manager") {
            continue;
        }
        let mut sl: Vec<String> = vec!["sequenceDiagram".into(), "  autonumber".into()];
        sl.push("  participant IN as Inbox (trigger)".into());
        sl.push(format!("  participant PM as {} (decides)", name));
        let state_table = node
            .get("state_table")
            .and_then(|x| x.get("$ref"))
            .and_then(|x| x.as_str())
            .and_then(ref_name);
        if let Some(st) = &state_table {
            sl.push(format!("  participant ST as {} (state)", st));
        }
        // Deterministic first-use participant order for read models, ports, aggregates.
        let mut extra: Vec<(String, String)> = Vec::new(); // (id, declaration)
        let declare = |extra: &mut Vec<(String, String)>, id: String, label: String| {
            if !extra.iter().any(|(i, _)| *i == id) {
                extra.push((id.clone(), label));
            }
        };
        let legs = node.get("receives").and_then(|x| x.as_sequence()).cloned().unwrap_or_default();
        for e in &legs {
            if let Some(steps) = e.get("steps").and_then(|x| x.as_sequence()) {
                for s in steps {
                    if let Some(r) = s.get("read") {
                        if let Some(m) = r.get("model").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name) {
                            declare(&mut extra, c4id("RM_", &m), format!("  participant {} as {} (read model)", c4id("RM_", &m), m));
                        }
                    }
                    if let Some(c) = s.get("call") {
                        if let Some(p) = c.get("port").and_then(|x| x.as_str()) {
                            declare(&mut extra, c4id("PT_", p), format!("  participant {} as port {} (adapter)", c4id("PT_", p), p));
                        }
                    }
                    for kind in ["deliver", "send"] {
                        if let Some(d) = s.get(kind) {
                            if let Some(t) = d.get("to").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name) {
                                declare(&mut extra, c4id("AG_", &t), format!("  participant {} as {} (aggregate)", c4id("AG_", &t), t));
                            }
                        }
                    }
                }
            }
        }
        for (_, decl) in &extra {
            sl.push(decl.clone());
        }
        for e in &legs {
            let msg_ref = e.get("message").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("");
            let msg = ref_name(msg_ref).unwrap_or_else(|| "?".to_string());
            let is_command = msg_ref.starts_with("commands.yaml#/");
            sl.push(format!("  rect rgb(245,245,245)").to_string());
            sl.push(format!("  IN->>PM: {} ({})", msg, if is_command { "command" } else { "event" }));
            let steps = e.get("steps").and_then(|x| x.as_sequence()).cloned().unwrap_or_default();
            for s in &steps {
                if let Some(r) = s.get("read") {
                    let m = r.get("model").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name).unwrap_or_default();
                    let alias = r.get("as").and_then(|x| x.as_str()).unwrap_or("?");
                    let w = fmt_map(r.get("where"));
                    sl.push(format!("  PM->>{}: read as {}{}", c4id("RM_", &m), alias, if w.is_empty() { String::new() } else { format!(" [{}]", w) }));
                } else if let Some(g) = s.get("guard") {
                    let cond = fmt_map_nested(g.get("that"), &fmt_value);
                    if let Some(er) = g.get("throws").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name) {
                        sl.push(format!("  PM--xIN: throws {}{}", er, if cond.is_empty() { String::new() } else { format!(" unless {}", cond) }));
                    } else {
                        sl.push(format!("  Note over PM: skip unless {}", if cond.is_empty() { "precondition holds".to_string() } else { cond }));
                    }
                } else if let Some(c) = s.get("call") {
                    let p = c.get("port").and_then(|x| x.as_str()).unwrap_or("?");
                    let op = c.get("operation").and_then(|x| x.as_str()).unwrap_or("?");
                    sl.push(format!("  PM->>{}: {}", c4id("PT_", p), op));
                } else if let Some(d) = s.get("deliver") {
                    let ev = d.get("event").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name).unwrap_or_default();
                    let to = d.get("to").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name).unwrap_or_default();
                    let fe = d.get("for_each").and_then(|x| x.as_str()).map(|a| format!(" (for each {})", a)).unwrap_or_default();
                    sl.push(format!("  PM->>{}: deliver {}{} — the aggregate records it", c4id("AG_", &to), ev, fe));
                } else if let Some(d) = s.get("send") {
                    let cm = d.get("command").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name).unwrap_or_default();
                    let to = d.get("to").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name).unwrap_or_default();
                    let fe = d.get("for_each").and_then(|x| x.as_str()).map(|a| format!(" (for each {})", a)).unwrap_or_default();
                    sl.push(format!("  PM->>{}: send {}{} — the aggregate validates", c4id("AG_", &to), cm, fe));
                } else if let Some(st) = s.get("state") {
                    let by = fmt_map(st.get("by"));
                    let exp = fmt_map(st.get("expect"));
                    let set = fmt_map(st.get("set"));
                    let mut parts: Vec<String> = Vec::new();
                    if !by.is_empty() {
                        parts.push(format!("by {}", by));
                    }
                    if !exp.is_empty() {
                        parts.push(format!("expect {}", exp));
                    }
                    if !set.is_empty() {
                        parts.push(format!("set {}", set));
                    }
                    sl.push(format!("  PM->>ST: {}", parts.join("; ")));
                }
            }
            sl.push("  end".into());
        }
        out.push((name.to_string(), sl.join("\n")));
    }
    out
}

/// The per-PM diagrams as `### name` + fenced Markdown blocks (c4.generated.md framing).
pub(crate) fn pm_sequence_blocks(model: &Model) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (name, body) in pm_sequence_map(model) {
        for line in [format!("### {}", name), String::new(), "```mermaid".into(), body, "```".into(), String::new()] {
            out.push(line);
        }
    }
    out
}

/// Raw `$ref` strings of a ref-list (toRefList).
pub(crate) fn ref_strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_sequence())
        .map(|s| {
            s.iter()
                .filter_map(|it| it.get("$ref").and_then(|r| r.as_str()).map(|x| x.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// (view name, fedBy event names) for every read model — fold views + materialized tables.
pub(crate) fn views_fedby(model: &Model) -> Vec<(String, Vec<String>)> {
    parse_views(model).iter().map(|v| (v.name.clone(), v.fedby.clone())).collect()
}

pub(crate) fn read_c4(model: &Model) -> C4 {
    let l2 = model.defs.get("architecture/c4-l2.yaml");
    let l3 = model.defs.get("architecture/c4-l3.yaml");
    let l2get = |k: &str| l2.and_then(|v| v.get(k));
    let system = l2get("system");
    let sstr = |k: &str| system.and_then(|s| s.get(k)).and_then(|x| x.as_str());
    let mut contexts = Vec::new();
    if let Some(cm) = l2get("boundedContexts").and_then(|v| v.as_mapping()) {
        for (k, bc) in cm {
            if let Some(id) = k.as_str() {
                contexts.push(Ctx {
                    id: id.to_string(),
                    description: bc.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    aggregates: ref_names(bc.get("aggregates")),
                    process_managers: ref_names(bc.get("processManagers")),
                    roles: bc
                        .get("roles")
                        .and_then(|v| v.as_sequence())
                        .map(|s| {
                            s.iter().filter_map(|v| v.as_str().map(|x| x.to_string())).collect()
                        })
                        .unwrap_or_default(),
                });
            }
        }
    }
    let mut containers = Vec::new();
    if let Some(cm) = l2get("containers").and_then(|v| v.as_mapping()) {
        for (k, c) in cm {
            if let Some(id) = k.as_str() {
                containers.push(Container {
                    id: id.to_string(),
                    technology: c.get("technology").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    description: c.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    deploy_tree: match c.get("deploy_tree").and_then(|x| x.as_str()) {
                        None | Some("bins") => DeployTree::Bins,
                        Some("monolith") => DeployTree::Monolith,
                        Some(other) => DeployTree::Unknown(other.to_string()),
                    },
                    realizes: ref_names(c.get("realizes")),
                    ingress_host: c
                        .get("ingress_host")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string()),
                    integration_scopes: c
                        .get("integration_scopes")
                        .and_then(|v| v.as_sequence())
                        .map(|s| {
                            s.iter().filter_map(|v| v.as_str().map(|x| x.to_string())).collect()
                        })
                        .unwrap_or_default(),
                    schedule: c.get("schedule").and_then(|x| x.as_str()).map(|s| s.to_string()),
                    suspended: c.get("suspended").and_then(|x| x.as_bool()) == Some(true),
                });
            }
        }
    }
    let mut externals = Vec::new();
    if let Some(cm) = l2get("externalSystems").and_then(|v| v.as_mapping()) {
        for (k, x) in cm {
            if let Some(id) = k.as_str() {
                externals.push(External {
                    id: id.to_string(),
                    description: x.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string(),
                });
            }
        }
    }
    let mut relationships = Vec::new();
    if let Some(seq) = l2get("relationships").and_then(|v| v.as_sequence()) {
        for r in seq {
            relationships.push(Rel {
                from: r.get("from").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                to: r.get("to").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                description: r.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            });
        }
    }
    let mut components = Vec::new();
    if let Some(cm) = l3.and_then(|v| v.get("components")).and_then(|v| v.as_mapping()) {
        for (k, c) in cm {
            if let Some(id) = k.as_str() {
                components.push(Comp {
                    id: id.to_string(),
                    description: c.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    instrumented: c.get("instrumented").and_then(|x| x.as_bool()) == Some(true),
                    container: c.get("container").and_then(|x| x.as_str()).map(|s| s.to_string()),
                });
            }
        }
    }
    C4 {
        system_name: sstr("name").unwrap_or("Captain.Food").to_string(),
        system_description: sstr("description").unwrap_or("").to_string(),
        contexts,
        containers,
        externals,
        relationships,
        components,
    }
}

pub(crate) fn push_view(l: &mut Vec<String>, decl: &str) {
    l.push(format!("    {} {{", decl));
    l.push("      include *".into());
    l.push("      autolayout lr".into());
    l.push("    }".into());
}
pub(crate) fn push_style(l: &mut Vec<String>, tag: &str, props: &[&str]) {
    l.push(format!("      element \"{}\" {{", tag));
    for p in props {
        l.push(format!("        {}", p));
    }
    l.push("      }".into());
}

pub(crate) fn emit_structurizr(model: &Model) -> String {
    let c4 = read_c4(model);
    let comp_ids: std::collections::HashSet<&str> = c4.components.iter().map(|c| c.id.as_str()).collect();
    let node_id = |key: &str| -> String {
        if comp_ids.contains(key) {
            c4id("c_", key)
        } else if c4.containers.iter().any(|c| c.id == key) {
            c4id("ct_", key)
        } else if c4.externals.iter().any(|x| x.id == key) {
            c4id("x_", key)
        } else {
            c4id("n_", key)
        }
    };
    // Which bounded context (if any) owns each actor/PM — used to tag realized members.
    let mut member_kind: HashMap<&str, &str> = HashMap::new();
    for ctx in &c4.contexts {
        for a in &ctx.aggregates {
            member_kind.insert(a.as_str(), "Aggregate");
        }
        for p in &ctx.process_managers {
            member_kind.insert(p.as_str(), "ProcessManager");
        }
    }
    let mut l: Vec<String> = Vec::new();
    l.push(format!("workspace {} {} {{", q(&c4.system_name), q(&c4.system_description)));
    l.push("  model {".into());
    l.push(format!("    ss = softwareSystem {} {} {{", q(&c4.system_name), q(&c4.system_description)));
    // Containers with members (realized actors/PMs from l2 `realizes:`, plus l3 components homed
    // here via `container:`) open a block; the rest are leaves. This replaced the pre-split shape
    // where every component nested inside the single `api` container (ADR-20260807-183024).
    for c in &c4.containers {
        let comps_here: Vec<&Comp> =
            c4.components.iter().filter(|k| k.container.as_deref() == Some(c.id.as_str())).collect();
        let open = format!(
            "      {} = container {} {} {}",
            c4id("ct_", &c.id), q(&c.id), q(&c.description), q(&c.technology)
        );
        if c.realizes.is_empty() && comps_here.is_empty() {
            l.push(open);
            continue;
        }
        l.push(format!("{} {{", open));
        for n in &c.realizes {
            let tag = member_kind.get(n.as_str()).copied().unwrap_or("Aggregate");
            l.push(format!("        {} = component {} {} {}", c4id("a_", n), q(n), q(""), q(tag)));
        }
        for comp in comps_here {
            l.push(format!(
                "        {} = component {} {} {}",
                c4id("c_", &comp.id), q(&comp.id), q(&comp.description),
                q(if comp.instrumented { "Instrumented" } else { "Domain" })
            ));
        }
        l.push("      }".into());
    }
    l.push("    }".into());
    for x in &c4.externals {
        l.push(format!("    {} = softwareSystem {} {} \"External\"", c4id("x_", &x.id), q(&x.id), q(&x.description)));
    }
    l.push("".into());
    for r in &c4.relationships {
        l.push(format!("    {} -> {} {}", node_id(&r.from), node_id(&r.to), q(&r.description)));
    }
    for (from, to, desc) in PIPELINE {
        if comp_ids.contains(from) && comp_ids.contains(to) {
            l.push(format!("    {} -> {} {}", c4id("c_", from), c4id("c_", to), q(desc)));
        }
    }
    if comp_ids.contains("projection-updaters") {
        l.push(format!("    {} -> {} \"writes read models\"", c4id("c_", "projection-updaters"), c4id("ct_", "read-models")));
    }
    if comp_ids.contains("event-store-adapter") {
        l.push(format!("    {} -> {} \"appends to domain_events\"", c4id("c_", "event-store-adapter"), c4id("ct_", "event-store")));
    }
    l.push("  }".into());
    l.push("  views {".into());
    push_view(&mut l, "systemContext ss \"SystemContext\"");
    push_view(&mut l, "container ss \"Containers\"");
    // One component view per container that has members (post-split there is no single `api`
    // container to show — each bin's internals get their own view).
    for c in &c4.containers {
        let has_members = !c.realizes.is_empty()
            || c4.components.iter().any(|k| k.container.as_deref() == Some(c.id.as_str()));
        if has_members {
            let key: String = c
                .id
                .split(|ch: char| !ch.is_ascii_alphanumeric())
                .filter(|s| !s.is_empty())
                .map(|s| {
                    let mut cs = s.chars();
                    cs.next().map(|f| f.to_ascii_uppercase().to_string() + cs.as_str()).unwrap_or_default()
                })
                .collect();
            push_view(&mut l, &format!("component {} \"{}Components\"", c4id("ct_", &c.id), key));
        }
    }
    l.push("    styles {".into());
    push_style(&mut l, "Element", &["color #ffffff"]);
    push_style(&mut l, "Software System", &["background #2d4f4a"]);
    push_style(&mut l, "Container", &["background #313335"]);
    push_style(&mut l, "External", &["background #cc7832"]);
    push_style(&mut l, "Aggregate", &["background #4ec9b0", "color #11201d"]);
    push_style(&mut l, "ProcessManager", &["background #56a0c0"]);
    push_style(&mut l, "Instrumented", &["background #c586c0"]);
    push_style(&mut l, "Domain", &["background #313335"]);
    l.push("    }".into());
    l.push("  }".into());
    l.push("}".into());
    l.push("".into());
    l.join("\n")
}

pub(crate) fn emit_mermaid(model: &Model) -> String {
    let c4 = read_c4(model);
    let actors = parse_actors(model);
    let views = views_fedby(model);

    // 1) Container diagram.
    let mut container: Vec<String> = vec!["flowchart LR".into()];
    container.push("  subgraph CaptainFood[\"Captain.Food\"]".into());
    for c in &c4.containers {
        container.push(format!("    {}[\"{}<br/><small>{}</small>\"]", c4id("n_", &c.id), c.id, c.technology));
    }
    container.push("  end".into());
    for x in &c4.externals {
        container.push(format!("  {}[/\"{}\"/]", c4id("n_", &x.id), x.id));
    }
    for r in &c4.relationships {
        container.push(format!("  {} -->|\"{}\"| {}", c4id("n_", &r.from), r.description.replace('"', "'"), c4id("n_", &r.to)));
    }

    // 2) Domain diagram: contexts → aggregates → the read models they feed.
    let mut evt_views: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for (vname, fedby) in &views {
        for e in fedby {
            evt_views.entry(e.clone()).or_default().push(vname.clone());
        }
    }
    let emits_of = |a: &Actor| -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for r in &a.receives {
            for ev in &r.emits {
                if let Some(n) = ref_name(ev) {
                    if !v.contains(&n) {
                        v.push(n);
                    }
                }
            }
        }
        v
    };
    let mut domain: Vec<String> = vec!["flowchart LR".into()];
    for ctx in &c4.contexts {
        domain.push(format!("  subgraph {}[\"{}\"]", c4id("g_", &ctx.id), ctx.id));
        for a in ctx.aggregates.iter().chain(ctx.process_managers.iter()) {
            domain.push(format!("    {}[\"{}\"]", c4id("a_", a), a));
        }
        domain.push("  end".into());
    }
    let mut view_ids: Vec<String> = Vec::new();
    let mut edges: Vec<String> = Vec::new();
    for a in &actors {
        let mut seen_v: Vec<String> = Vec::new();
        for ev in emits_of(a) {
            if let Some(vs) = evt_views.get(&ev) {
                for v in vs {
                    if !seen_v.contains(v) {
                        seen_v.push(v.clone());
                    }
                }
            }
        }
        for v in &seen_v {
            if !view_ids.contains(v) {
                view_ids.push(v.clone());
            }
            let edge = format!("  {} --> {}", c4id("a_", &a.name), c4id("v_", v));
            if !edges.contains(&edge) {
                edges.push(edge);
            }
        }
    }
    for v in &view_ids {
        domain.push(format!("  {}[(\"{}\")]", c4id("v_", v), v));
    }
    domain.extend(edges);

    // 3) Saga sequence diagrams — generated from the TYPED STEPS (processmanager.yaml): each step
    //    kind maps to exactly one participant/layer, so the diagram IS the layer contract.
    let saga_blocks: Vec<String> = pm_sequence_blocks(model);

    let mut out: Vec<String> = vec![
        "<!-- GENERATED by tools/codegen — do not edit by hand. Source: specs/architecture/c4-*.yaml. -->".into(),
        "# Captain.Food — C4 diagrams (Mermaid, generated)".into(),
        "".into(),
        "Rendered by any Mermaid-aware viewer (GitHub, VS Code, mermaid.live). The authoritative source is".into(),
        "`specs/architecture/c4-l2.yaml` / `c4-l3.yaml`; regenerate with `make generate`.".into(),
        "".into(),
        "## L2 — Containers & external systems".into(),
        "".into(),
        "```mermaid".into(),
        container.join("\n"),
        "```".into(),
        "".into(),
        "## Domain — bounded contexts → aggregates → read models".into(),
        "".into(),
        "Each aggregate links to the `View_*` read models its emitted events project into.".into(),
        "".into(),
        "```mermaid".into(),
        domain.join("\n"),
        "```".into(),
        "".into(),
        "## Saga sequences — message → emitted events, in order".into(),
        "".into(),
        "Each process manager (saga) as a time-ordered sequence: the command/event it receives and the".into(),
        "events it emits in response (derived from `actors.yaml`).".into(),
        "".into(),
    ];
    out.extend(saga_blocks);
    out.join("\n")
}

