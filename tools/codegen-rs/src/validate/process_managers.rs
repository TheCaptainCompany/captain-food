use crate::*;

/// `that` conditions: `alias.field == CONST` joined with ` and `.
pub(crate) fn fmt_map_nested(v: Option<&Value>, fmt_value: &dyn Fn(&Value) -> String) -> String {
    v.and_then(|x| x.as_mapping())
        .map(|m| {
            m.iter()
                .flat_map(|(subj, fields)| {
                    let s = subj.as_str().unwrap_or("?").to_string();
                    fields
                        .as_mapping()
                        .map(|fm| {
                            fm.iter()
                                .filter_map(|(f, val)| f.as_str().map(|fx| format!("{}.{} == {}", s, fx, fmt_value(val))))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join(" and ")
        })
        .unwrap_or_default()
}

// ─── §2b — typed-step process-manager validation (processmanager.yaml) ──────────────────────────

/// The leg SELECTs from the projection table its `model:` names — the generated pipeline legs, whose
/// hooks are backed by a `*ReadRepository` over that table. This is the source that makes the leg's
/// host depend on the projector that maintains it.
///
/// IT SAYS NOTHING ABOUT LAG TOLERANCE, and a derivation must not infer any. `PROJECTION` spans both
/// the benign case (`CartBindingProcess` — a cart missed on this pass binds on the next) and the
/// fatal one (`PaymentSettlementProcess` — an unprojected `payment_status` skips the capture and the
/// ~7-day authorization expires, so the restaurant is never paid: the #544 class). Only the step's
/// prose separates them.
pub(crate) const READ_SOURCE_PROJECTION: &str = "PROJECTION";

/// The leg folds the entity from the `captain_write` event stream and NEVER touches the projection
/// its `model:` names — the hand-written command-handler legs and hand-written hooks inside generated
/// legs. Here `model:` is borrowed SHAPE only: the leg is race-free against projector lag, and
/// depends on the event store rather than on a projector.
pub(crate) const READ_SOURCE_EVENT_STREAM: &str = "EVENT_STREAM";

/// The closed set of `read.source` values (ADR-20260811-014129 D2 category 3: a bare token is
/// correct precisely because the set is closed HERE). Deliberately not a `scalars.yaml` enum — these
/// name where the SPEC's own steps read from, a codegen-mechanics distinction with no business
/// meaning, and a domain scalar would emit a `ReadSource` type into the domain crates that no domain
/// value ever inhabits.
pub(crate) const READ_SOURCES: [&str; 2] = [READ_SOURCE_PROJECTION, READ_SOURCE_EVENT_STREAM];

/// The closed key set of a `read:` step body. `model` = the SHAPE consumed, `as` = the alias later
/// steps bind to, `where` = the lookup, `source` = where it is physically read from, `note` = prose.
pub(crate) const READ_KEYS: [&str; 5] = ["model", "as", "where", "note", "source"];

// SOURCE IS PER-STEP, NOT PER-LEG. The methodology lives here rather than on any one step's `note:`,
// because a `note:` is emitted verbatim into two generated doc comments, where a statement about the
// DSL reads as a statement about that leg. A leg's implementation is not uniform:
// `DeliveryDispatchProcess`'s `OrderMarkedReady` leg IS generated, and its restaurant hook folds the
// restaurant's own stream while its order hook SELECTs the projection. So "is this leg hand-written"
// would already be the wrong derivation for a leg committed today — the declaration belongs on the
// step, and that committed pair is the proof.

/// Enum members of a scalars.yaml scalar reached via `r`, when it has an `enum`.
pub(crate) fn scalar_enum(model: &Model, r: &str, ctx: &str) -> Option<Vec<String>> {
    resolve_ref(model, r, ctx)?
        .get("enum")?
        .as_sequence()
        .map(|s| s.iter().filter_map(|v| v.as_str().map(|x| x.to_string())).collect())
}

/// Column name → enum members (when the column's `type.$ref` is an enum scalar) of a table/view/
/// projection definition (`columns` mapping). Columns without a resolvable enum map to None.
pub(crate) fn columns_info(model: &Model, def: &Value, ctx: &str) -> Option<HashMap<String, Option<Vec<String>>>> {
    let cols = def.get("columns")?.as_mapping()?;
    let mut out = HashMap::new();
    for (k, col) in cols {
        let name = match k.as_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let en = col
            .get("type")
            .and_then(|t| t.get("$ref"))
            .and_then(|x| x.as_str())
            .and_then(|r| scalar_enum(model, r, ctx));
        out.insert(name, en);
    }
    Some(out)
}

/// Property name → enum members (when the property `$ref`s an enum scalar) of a command/event def.
pub(crate) fn props_info(model: &Model, def: &Value, ctx: &str) -> HashMap<String, Option<Vec<String>>> {
    let mut out = HashMap::new();
    if let Some(props) = def.get("properties").and_then(|x| x.as_mapping()) {
        for (k, p) in props {
            let name = match k.as_str() {
                Some(s) => s.to_string(),
                None => continue,
            };
            let en = p.get("$ref").and_then(|x| x.as_str()).and_then(|r| scalar_enum(model, r, ctx));
            out.insert(name, en);
        }
    }
    out
}

/// One typed step value (`const` / `from` / `from_state` / `from_read` / `from_port` /
/// `from_envelope`) — exactly one form, and each form checked against what it names.
#[allow(clippy::too_many_arguments)]
pub(crate) fn check_pm_value(
    v: &Value,
    field_enum: Option<&Vec<String>>,
    state_cols: Option<&HashMap<String, Option<Vec<String>>>>,
    aliases: &HashMap<String, HashMap<String, Option<Vec<String>>>>,
    ports: &HashMap<String, BTreeSet<String>>,
    where_: &str,
    issues: &mut Vec<Issue>,
) {
    let forms = ["const", "from", "from_state", "from_read", "from_port", "from_envelope", "from_hook"];
    let present: Vec<&str> = forms.iter().copied().filter(|f| v.get(*f).is_some()).collect();
    if present.len() != 1 {
        issues.push(err(
            "pm-value",
            where_.to_string(),
            "a step value must be exactly one of { const | from | from_state | from_read | from_port | from_envelope | from_hook }.".into(),
        ));
        return;
    }
    match present[0] {
        "const" => {
            let c = v.get("const").and_then(|x| x.as_str()).unwrap_or("");
            if let Some(en) = field_enum {
                if !en.iter().any(|m| m == c) {
                    issues.push(err(
                        "pm-const",
                        where_.to_string(),
                        format!("const '{}' is not a member of the field's enum scalar ({}).", c, en.join("|")),
                    ));
                }
            }
        }
        "from" => {
            if v.get("from").and_then(|f| f.get("$ref")).and_then(|x| x.as_str()).is_none() {
                issues.push(err("pm-value", where_.to_string(), "`from` must be a { $ref: '<file>#/<Msg>/properties/<p>' }.".into()));
            }
        }
        "from_state" => {
            let c = v.get("from_state").and_then(|x| x.as_str()).unwrap_or("");
            match state_cols {
                None => issues.push(err("pm-value", where_.to_string(), "`from_state` used but the process manager declares no state_table.".into())),
                Some(cols) if !cols.contains_key(c) => issues.push(err(
                    "pm-value",
                    where_.to_string(),
                    format!("`from_state` column '{}' does not exist on the state table.", c),
                )),
                _ => {}
            }
        }
        "from_read" => {
            let spec = v.get("from_read").and_then(|x| x.as_str()).unwrap_or("");
            let (alias, col) = match spec.split_once('.') {
                Some(p) => p,
                None => {
                    issues.push(err("pm-value", where_.to_string(), "`from_read` must be '<alias>.<column>'.".into()));
                    return;
                }
            };
            match aliases.get(alias) {
                None => issues.push(err("pm-value", where_.to_string(), format!("`from_read` alias '{}' is not a prior read step.", alias))),
                Some(cols) if !cols.is_empty() && !cols.contains_key(col) => issues.push(err(
                    "pm-value",
                    where_.to_string(),
                    format!("`from_read` column '{}' does not exist on read model '{}'.", col, alias),
                )),
                _ => {}
            }
        }
        "from_port" => {
            let spec = v.get("from_port").and_then(|x| x.as_str()).unwrap_or("");
            let ok = spec
                .split_once('.')
                .map(|(p, op)| ports.get(p).map(|ops| ops.contains(op)).unwrap_or(false))
                .unwrap_or(false);
            if !ok {
                issues.push(err("pm-value", where_.to_string(), format!("`from_port` '{}' is not a declared <port>.<operation>.", spec)));
            }
        }
        "from_envelope" => {
            let f = v.get("from_envelope").and_then(|x| x.as_str()).unwrap_or("");
            if !["event_id", "correlation_id", "occurred_at"].contains(&f) {
                issues.push(err(
                    "pm-value",
                    where_.to_string(),
                    format!("`from_envelope` '{}' is not one of event_id | correlation_id | occurred_at (ADR-0041).", f),
                ));
            }
        }
        "from_hook" => {
            // A runtime orchestrator hook (#60): the value is resolved in code, so the name is the
            // only spec-level constraint — a non-empty snake_case identifier the emitter turns into
            // an async hook method. (The hook is only consumed inside `state.set`.)
            let n = v.get("from_hook").and_then(|x| x.as_str()).unwrap_or("");
            if n.is_empty() || !n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
                issues.push(err(
                    "pm-value",
                    where_.to_string(),
                    format!("`from_hook` '{}' must be a non-empty snake_case hook name.", n),
                ));
            }
        }
        _ => unreachable!(),
    }
}

/// §2b — validate every typed-step process manager: state columns exist (+ enum consts valid), read
/// models resolve, ports are declared, deliver/send targets receive the message, command-leg guards
/// `throws` typed errors while event-leg guards `skip` (facts are never rejected).
pub(crate) fn validate_process_managers(model: &Model, issues: &mut Vec<Issue>) {
    const CTX: &str = "processmanager.yaml";
    // actors.yaml aggregate inboxes: name → received message names.
    let mut agg_inbox: HashMap<String, BTreeSet<String>> = HashMap::new();
    // The DELIVERABLE half of each inbox: the `events.yaml#/…` refs, plus whether the actor has a
    // mailbox at all — what a `deliver:` step needs, since a deliver is now a lane ENQUEUE.
    let mut agg_facts: HashMap<String, (BTreeSet<String>, bool)> = HashMap::new();
    if let Some(Value::Mapping(m)) = model.defs.get("actors.yaml") {
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            let mut set = BTreeSet::new();
            let mut facts = BTreeSet::new();
            if let Some(seq) = node.get("receives").and_then(|x| x.as_sequence()) {
                for e in seq {
                    let r = e.get("message").and_then(|x| x.get("$ref")).and_then(|x| x.as_str());
                    if let Some(n) = r.and_then(ref_name) {
                        set.insert(n.clone());
                        // A `deliver:` is a lane ENQUEUE (ADR-20260816-040239), and only an
                        // EVENT can ride a lane as an inbound fact — the same `events.yaml#/…`
                        // scan the sealed `{Actor}Fact` traits and ACTOR_INBOUND_FACTS come from.
                        if ref_target_file(r.unwrap_or_default(), CTX).as_deref() == Some("events.yaml") {
                            facts.insert(n);
                        }
                    }
                }
            }
            let has_mailbox = node.get("mailbox").is_some();
            agg_inbox.insert(name.to_string(), set);
            agg_facts.insert(name.to_string(), (facts, has_mailbox));
        }
    }

    // WHERE a `source: PROJECTION` step's table physically lives (#564). Resolved ONCE from §18's own
    // placement walk (`validate/databases.rs`) rather than re-read here, so the reader question and
    // the placement requirement can never answer differently — the same discipline #562 applied
    // between §18 and the inventory emitter.
    let placements: HashMap<String, (&'static str, usize)> =
        crate::validate::databases::resolve_placements(model)
            .into_iter()
            .map(|p| (p.table, (p.mode.name(), p.databases.len())))
            .collect();

    let pms = match model.defs.get(CTX) {
        Some(Value::Mapping(m)) => m,
        _ => return,
    };
    for (k, node) in pms {
        let name = match k.as_str() {
            Some(s) => s,
            None => continue,
        };
        if node.get("type").and_then(|x| x.as_str()) != Some("process-manager") {
            issues.push(err("pm-type", format!("{}/{}", CTX, name), "every processmanager.yaml entry must declare `type: process-manager`.".into()));
            continue;
        }
        let at = format!("{}/{}", CTX, name);

        // State table (optional — required as soon as a `state` step exists).
        let st_ref = node.get("state_table").and_then(|x| x.get("$ref")).and_then(|x| x.as_str());
        let state_cols: Option<HashMap<String, Option<Vec<String>>>> = match st_ref {
            None => None,
            Some(r) => {
                if !r.starts_with("database/tables/") {
                    issues.push(err("pm-state-table", format!("{}.state_table", at), format!("state_table must $ref a database/tables/*.yaml table, got '{}'.", r)));
                }
                match resolve_ref(model, r, CTX).and_then(|d| columns_info(model, d, CTX)) {
                    Some(c) => Some(c),
                    None => None, // dangling → §1 reports it
                }
            }
        };

        // Ports: name → operation set, resolved THROUGH the service catalog — each entry is a
        // `$ref` into services.yaml (ADR-20260719-214500); the port's operations are the resolved
        // service's `operations` keys (the catalog itself is validated by §2d).
        let mut ports: HashMap<String, BTreeSet<String>> = HashMap::new();
        if let Some(pmap) = node.get("ports").and_then(|x| x.as_mapping()) {
            for (pk, pv) in pmap {
                let pname = match pk.as_str() {
                    Some(s) => s.to_string(),
                    None => continue,
                };
                let pw = format!("{}.ports.{}", at, pname);
                let sref = match pv.get("$ref").and_then(|x| x.as_str()) {
                    Some(r) => r,
                    None => {
                        issues.push(err("pm-port", pw, "ports must $ref the service catalog (services.yaml), ADR-20260719-214500.".into()));
                        continue;
                    }
                };
                if ref_target_file(sref, CTX).as_deref() != Some("services.yaml") {
                    issues.push(err("pm-port", pw, format!("a port must $ref a services.yaml service, got '{}'.", sref)));
                    continue;
                }
                let ops: BTreeSet<String> = match resolve_ref(model, sref, CTX) {
                    None => continue, // dangling → §1 reports it
                    Some(svc) => svc
                        .get("operations")
                        .and_then(|x| x.as_mapping())
                        .map(|m| m.iter().filter_map(|(ok, _)| ok.as_str().map(|s| s.to_string())).collect())
                        .unwrap_or_default(),
                };
                ports.insert(pname, ops);
            }
        }

        let legs = match node.get("receives").and_then(|x| x.as_sequence()) {
            Some(s) => s,
            None => continue,
        };
        for (i, e) in legs.iter().enumerate() {
            let leg = format!("{}.receives[{}]", at, i);
            let msg_ref = e.get("message").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("");
            let is_command = ref_target_file(msg_ref, CTX).as_deref() == Some("commands.yaml");
            let msg_props = resolve_ref(model, msg_ref, CTX).map(|d| props_info(model, d, CTX)).unwrap_or_default();

            // `pm-route-gate` (#797). A wrapper-seam `sends:` entry is ROUTED — `to:` names the
            // actor whose lane receives the command and `route_gate:` names the configuration key
            // that route is rolled back with — or it is a plain declaration for wiring/coverage
            // and carries NEITHER. One without the other is a half-declared route, and the
            // emitter (`emit::pm_orchestrators::route_decls`) reads exactly the both-present pair,
            // so each half fails a different way and NEITHER of them is loud:
            //   * `to:` with no `route_gate:` — the route drops out of the DECLARED route
            //     population. `ROUTED_LANES` is emitted WITHOUT its row, so the lane leaves the
            //     dead-man's-switch's declared population (#783: a lane nothing watches), and the
            //     `Route` variant it would be enqueued through is never generated. `make validate`
            //     stays at 0 errors and `make generate` writes the smaller table in silence; the
            //     only thing that says anything today is a hand-written seam naming the missing
            //     variant, i.e. a compile error in a file the DSL does not own.
            //   * `route_gate:` with no `to:` — a rollback lever wired to nothing. The key looks
            //     operable (it is a resolvable `$ref`, it appears in the boot report) while no
            //     route consults it.
            // Presence is tested on the KEY, not on a readable `$ref` inside it: a malformed
            // `route_gate:` is reported by §1's `ref-dangling`/`ref-site-undeclared`, and treating
            // it as absent here would answer a shape error with the wrong sentence.
            for (j, s) in e.get("sends").and_then(|x| x.as_sequence()).into_iter().flatten().enumerate() {
                let sw = format!("{}.sends[{}]", leg, j);
                let has_to = s.get("to").is_some();
                let has_gate = s.get("route_gate").is_some();
                let cmd = s
                    .get("command")
                    .and_then(|x| x.get("$ref"))
                    .and_then(|x| x.as_str())
                    .and_then(ref_name)
                    .unwrap_or_else(|| "?".to_string());
                if has_to && !has_gate {
                    issues.push(err(
                        "pm-route-gate",
                        format!("{}.route_gate", sw),
                        format!(
                            "sends '{}' declares `to:` with no `route_gate:` — a target with no gate is an \
                             UNGATEABLE route, and it is not even a route: the emitter reads the both-present \
                             pair, so this send is silently left out of ROUTED_LANES (its lane leaves the \
                             dead-man's-switch population) and no `Route` variant is generated for it. Declare \
                             the configuration key this route is rolled back with, or drop `to:` and leave the \
                             send as a plain declaration.",
                            cmd
                        ),
                    ));
                }
                if has_gate && !has_to {
                    issues.push(err(
                        "pm-route-gate",
                        format!("{}.to", sw),
                        format!(
                            "sends '{}' declares `route_gate:` with no `to:` — a gate on nothing. The key reads \
                             as an operable rollback lever (it resolves, it is in the boot report) while no \
                             route consults it. Name the target actor whose lane receives the command, or drop \
                             the gate.",
                            cmd
                        ),
                    ));
                }
            }

            let steps = match e.get("steps").and_then(|x| x.as_sequence()) {
                Some(s) if !s.is_empty() => s,
                _ => {
                    issues.push(err("pm-no-steps", leg.clone(), "a receives entry must declare an ordered, non-empty `steps` list.".into()));
                    continue;
                }
            };

            let mut aliases: HashMap<String, HashMap<String, Option<Vec<String>>>> = HashMap::new();
            for (j, s) in steps.iter().enumerate() {
                let sw = format!("{}.steps[{}]", leg, j);
                let smap = match s.as_mapping() {
                    Some(m) => m,
                    None => {
                        issues.push(err("pm-step", sw, "each step must be a single-key mapping.".into()));
                        continue;
                    }
                };
                if smap.len() != 1 {
                    issues.push(err("pm-step", sw.clone(), "each step must have exactly one kind (read | guard | call | deliver | send | state).".into()));
                    continue;
                }
                let (kind, body) = {
                    let (k, v) = smap.iter().next().unwrap();
                    (k.as_str().unwrap_or("?"), v)
                };
                match kind {
                    "read" => {
                        // The key set is CLOSED, and that is what makes `source` below enforceable
                        // rather than decorative: this arm used to accept any key silently, so a
                        // misspelled `sourc:` would have been a no-op and the next key added here
                        // would be invisible to every rule (#413's "silently invisible everywhere").
                        if let Some(bm) = body.as_mapping() {
                            for (bk, _) in bm {
                                let key = bk.as_str().unwrap_or("?");
                                if !READ_KEYS.contains(&key) {
                                    issues.push(err(
                                        "pm-read-key",
                                        format!("{}.{}", sw, key),
                                        format!(
                                            "unknown key '{}' on a `read:` step — the key set is closed ({}). An \
                                             unrecognized key is silently ignored by the loader, so it would look \
                                             declared while changing nothing.",
                                            key,
                                            READ_KEYS.join(" | ")
                                        ),
                                    ));
                                }
                            }
                        }
                        // SOURCE vs SHAPE (#564). `model:` says what SHAPE the leg consumes; `source:`
                        // says WHERE the bytes physically come from, and the two genuinely differ: a
                        // generated pipeline leg SELECTs from the named projection, while a
                        // hand-written leg folds the entity from the `captain_write` event stream and
                        // never touches that projection at all. REQUIRED, never optional-with-default:
                        // a default would make the distinction survive only where someone remembered
                        // to write it — transience-by-omission (ADR-20260812-214500 §2), which is the
                        // defect class rather than a convenience.
                        match body.get("source") {
                            None => issues.push(err(
                                "pm-read-source",
                                format!("{}.source", sw),
                                format!(
                                    "a `read:` step must declare `source: <{}>` — where the leg physically reads \
                                     from, which `model:` (the SHAPE it consumes) does not say. {} = the leg SELECTs \
                                     from the named projection; {} = the leg folds the entity from the \
                                     `captain_write` event stream and never touches that projection. Omitting it \
                                     declares neither.",
                                    READ_SOURCES.join(" | "),
                                    READ_SOURCE_PROJECTION,
                                    READ_SOURCE_EVENT_STREAM
                                ),
                            )),
                            Some(v) => {
                                let got = v.as_str();
                                if !got.map(|s| READ_SOURCES.contains(&s)).unwrap_or(false) {
                                    issues.push(err(
                                        "pm-read-source",
                                        format!("{}.source", sw),
                                        format!(
                                            "`source: {}` is not one of the closed set ({}).",
                                            got.map(|s| s.to_string()).unwrap_or_else(|| format!("{:?}", v)),
                                            READ_SOURCES.join(" | ")
                                        ),
                                    ));
                                }
                            }
                        }
                        let model_ref = body.get("model").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("");
                        let tf = ref_target_file(model_ref, CTX).unwrap_or_default();
                        if tf != "database/tables/projection_tables.yaml" && tf != "database/projection_views.yaml" {
                            issues.push(err(
                                "pm-read",
                                format!("{}.model", sw),
                                format!("read.model must $ref a projection table or a View_* (got '{}').", model_ref),
                            ));
                        }
                        // A `PROJECTION` step's whole point downstream is "this leg needs CONNECT on
                        // the database holding that table", so the table must RESOLVE TO ONE. Two
                        // shapes the grammar admits do not, and neither is reachable from anything
                        // committed — which is exactly why the gate is written before a derivation
                        // consumes the key rather than after:
                        //   * a `View_*` (legal above) declares no `database:` at all — it is a view
                        //     over `domain_events`, so `PROJECTION` names an empty database set (the
                        //     empty-CONNECT class, reproduced on the PM side) while `EVENT_STREAM`
                        //     would get the database right and the privilege story wrong;
                        //   * a `replicated: read-databases` table resolves to the SET of every
                        //     `recovery: replay` database, so "which one" has no answer.
                        // Both need a THIRD source token, and inventing one here would be a patch
                        // standing in for a decision. Refusing them keeps the option open instead.
                        if body.get("source").and_then(|x| x.as_str()) == Some(READ_SOURCE_PROJECTION) {
                            let table = ref_name(model_ref).unwrap_or_default();
                            let why = match placements.get(&table) {
                                // Refused whatever the CURRENT member count: the declaration says
                                // "every read database", so a singleton today is an accident of how
                                // many are declared, and the next one added would move the answer
                                // silently.
                                Some(("replicated", n)) => Some(format!(
                                    "'{}' is `replicated: read-databases` — it resolves to the read databases as a SET \
                                     ({} today), not to one",
                                    table, n
                                )),
                                Some((_, 1)) if !table.is_empty() => None,
                                Some((mode, n)) => Some(format!(
                                    "'{}' resolves to {} databases ({} placement) — a reader grant needs exactly one",
                                    table, n, mode
                                )),
                                None => Some(format!(
                                    "'{}' has no resolved database placement (a View_* declares none — it is a view \
                                     over `domain_events`, not a projection with a home)",
                                    if table.is_empty() { model_ref } else { &table }
                                )),
                            };
                            if let Some(why) = why {
                                issues.push(err(
                                    "pm-read-projection-database",
                                    format!("{}.source", sw),
                                    format!(
                                        "`source: {}` needs a single, derivable database and this target has none: {}. \
                                         {} is not the answer either — it would name the write side while the leg reads \
                                         elsewhere. Give the step a target with a declared `database:`, or record the \
                                         decision that adds a source token for this shape.",
                                        READ_SOURCE_PROJECTION, why, READ_SOURCE_EVENT_STREAM
                                    ),
                                ));
                            }
                        }
                        let cols = resolve_ref(model, model_ref, CTX)
                            .and_then(|d| columns_info(model, d, CTX))
                            .unwrap_or_default();
                        let alias = body.get("as").and_then(|x| x.as_str()).unwrap_or("");
                        if alias.is_empty() {
                            issues.push(err("pm-read", format!("{}.as", sw), "read must name its result with `as`.".into()));
                        }
                        if let Some(wmap) = body.get("where").and_then(|x| x.as_mapping()) {
                            for (wk, wv) in wmap {
                                let col = wk.as_str().unwrap_or("?");
                                if !cols.is_empty() && !cols.contains_key(col) {
                                    issues.push(err("pm-read", format!("{}.where.{}", sw, col), format!("column '{}' does not exist on the read model.", col)));
                                }
                                check_pm_value(wv, cols.get(col).and_then(|e| e.as_ref()), state_cols.as_ref(), &aliases, &ports, &format!("{}.where.{}", sw, col), issues);
                            }
                        }
                        if !alias.is_empty() {
                            aliases.insert(alias.to_string(), cols);
                        }
                    }
                    "guard" => {
                        let throws = body.get("throws");
                        let skip = body.get("skip").and_then(|x| x.as_bool()).unwrap_or(false);
                        // Exactly one outcome. `throws` = ERROR (typed, on any leg — an event leg
                        // aborts and surfaces it); `skip` = benign expected alternative (never an
                        // error). A command leg has no benign-skip path: it only rejects.
                        match (throws, skip) {
                            (Some(t), false) => {
                                match t.get("$ref").and_then(|x| x.as_str()) {
                                    None => issues.push(err("pm-guard", format!("{}.throws", sw), "guard.throws must be a { $ref: 'errors.yaml#/<Error>' }.".into())),
                                    Some(r) => {
                                        if ref_target_file(r, CTX).as_deref() != Some("errors.yaml") {
                                            issues.push(err("pm-guard", format!("{}.throws", sw), format!("guard.throws must reference errors.yaml, got '{}'.", r)));
                                        }
                                    }
                                }
                            }
                            (None, true) => {
                                if is_command {
                                    issues.push(err(
                                        "pm-guard",
                                        sw.clone(),
                                        "a COMMAND-leg guard must `throws` a typed error — a command has no benign-skip path; in case of error the guard throws.".into(),
                                    ));
                                }
                            }
                            _ => issues.push(err(
                                "pm-guard",
                                sw.clone(),
                                "a guard must declare exactly one outcome: `throws` (error — typed $ref errors.yaml) or `skip: true` (benign alternative).".into(),
                            )),
                        }
                        if let Some(that) = body.get("that").and_then(|x| x.as_mapping()) {
                            for (subj_k, fields) in that {
                                let subj = subj_k.as_str().unwrap_or("?");
                                let fmap = match fields.as_mapping() {
                                    Some(m) => m,
                                    None => continue,
                                };
                                for (fk, fv) in fmap {
                                    let field = fk.as_str().unwrap_or("?");
                                    let fw = format!("{}.that.{}.{}", sw, subj, field);
                                    let field_enum: Option<&Vec<String>> = match subj {
                                        "message" => {
                                            if !msg_props.is_empty() && !msg_props.contains_key(field) {
                                                issues.push(err("pm-guard", fw.clone(), format!("'{}' is not a property of the trigger message.", field)));
                                            }
                                            msg_props.get(field).and_then(|e| e.as_ref())
                                        }
                                        "state" => match state_cols.as_ref() {
                                            None => {
                                                issues.push(err("pm-guard", fw.clone(), "guard on `state` but the process manager declares no state_table.".into()));
                                                None
                                            }
                                            Some(cols) => {
                                                if !cols.contains_key(field) {
                                                    issues.push(err("pm-guard", fw.clone(), format!("column '{}' does not exist on the state table.", field)));
                                                }
                                                cols.get(field).and_then(|e| e.as_ref())
                                            }
                                        },
                                        alias => match aliases.get(alias) {
                                            None => {
                                                issues.push(err("pm-guard", fw.clone(), format!("guard subject '{}' is neither `message`, `state`, nor a prior read alias.", alias)));
                                                None
                                            }
                                            Some(cols) => {
                                                if !cols.is_empty() && !cols.contains_key(field) {
                                                    issues.push(err("pm-guard", fw.clone(), format!("column '{}' does not exist on read model '{}'.", field, alias)));
                                                }
                                                cols.get(field).and_then(|e| e.as_ref())
                                            }
                                        },
                                    };
                                    if fv.get("const").is_none() {
                                        issues.push(err("pm-guard", fw.clone(), "a guard condition value must be structural: { const: <ENUM> }.".into()));
                                    } else {
                                        check_pm_value(fv, field_enum, state_cols.as_ref(), &aliases, &ports, &fw, issues);
                                    }
                                }
                            }
                        }
                    }
                    "call" => {
                        let port = body.get("port").and_then(|x| x.as_str()).unwrap_or("");
                        let op = body.get("operation").and_then(|x| x.as_str()).unwrap_or("");
                        let ok = ports.get(port).map(|ops| ops.contains(op)).unwrap_or(false);
                        if !ok {
                            issues.push(err("pm-call", sw.clone(), format!("call '{}.{}' is not a declared port operation.", port, op)));
                        }
                    }
                    "deliver" | "send" => {
                        let rule: &'static str = if kind == "deliver" { "pm-deliver" } else { "pm-send" };
                        let (mkey, target_file) = if kind == "deliver" { ("event", "events.yaml") } else { ("command", "commands.yaml") };
                        let mref = body.get(mkey).and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("");
                        if ref_target_file(mref, CTX).as_deref() != Some(target_file) {
                            issues.push(err(rule, format!("{}.{}", sw, mkey), format!("{}.{} must reference {}, got '{}'.", kind, mkey, target_file, mref)));
                        }
                        let mname = ref_name(mref).unwrap_or_default();
                        // `pm-route-gate` over `send:` STEPS (#807). Identical contract to the
                        // wrapper-seam `sends:` block above, and it has to be written twice
                        // because the two shapes are parsed by different code: a step's `to:` was
                        // read here, validated, and then DROPPED by the emitter, because
                        // `PmStepDef::Send` carried no `to`/`route_gate` at all and `route_decls`
                        // had no arm for a step. Four committed sends therefore appended to
                        // streams their process manager does not own with no route, no gate and
                        // no row in `ROUTED_LANES`.
                        //
                        // A `send:` differs from a `deliver:` in one way that matters here: `to:`
                        // is MANDATORY on a send (the check below refuses its absence outright),
                        // so "target present, gate absent" is not an unrouted step the way an
                        // unrouted `deliver:` is -- it is the defect. Every `send:` is therefore
                        // required to declare its route, which is the property #807 installs:
                        // a saga writing a stream it does not own is never the default.
                        //
                        // Presence is tested on the KEY, not on a readable `$ref` inside it: a
                        // malformed `route_gate:` is reported by §1's `ref-dangling` /
                        // `ref-site-undeclared`, and treating it as absent here would answer a
                        // shape error with the wrong sentence.
                        if kind == "send" {
                            let has_to = body.get("to").is_some();
                            let has_gate = body.get("route_gate").is_some();
                            let s_dedup_prop = body
                                .get("dedup_by")
                                .and_then(|x| x.get("$ref"))
                                .and_then(|x| x.as_str())
                                .and_then(|r| r.rsplit('/').next())
                                .map(|s| s.to_string());
                            if has_to && !has_gate {
                                issues.push(err(
                                    "pm-route-gate",
                                    format!("{}.route_gate", sw),
                                    format!(
                                        "send '{}' declares `to:` with no `route_gate:` — a target with no gate is an \
                                         UNGATEABLE route, and it is not even a route: the emitter reads the both-present \
                                         pair, so this send is silently left out of ROUTED_LANES (its lane leaves the \
                                         dead-man's-switch population), no `Route` variant is generated for it, and the \
                                         saga goes on appending to a stream it does not own from its own thread. Declare \
                                         the configuration key this route is rolled back with.",
                                        mname
                                    ),
                                ));
                            }
                            // `pm-send-dedup` (#807). A routed send's door is keyed on
                            // (route, external_id), and `external_id` must be the value the TARGET
                            // HANDLER dedups on -- otherwise the door either swallows legitimate
                            // repeats or lets duplicates through, both silently. There is
                            // deliberately NO default: the safe axis does not follow from the
                            // target. `MarkOrderDelivered` dedups on the ORDER (an order is
                            // delivered once, so the order's own id is right and usefully collapses
                            // a partner report racing a rider completion). `GrantCustomerCredit`
                            // dedups on the RECLAMATION while its ledger is keyed by CUSTOMER -- a
                            // customer legitimately receives many goodwill credits, so a
                            // customer-keyed door would drop every one after the first: money owed,
                            // never paid, no error raised anywhere. Inheriting the target's identity
                            // would have made exactly that the default.
                            if has_to && has_gate {
                                match s_dedup_prop {
                                    None => issues.push(err(
                                        "pm-send-dedup",
                                        format!("{}.dedup_by", sw),
                                        format!(
                                            "routed send '{}' declares no `dedup_by:` — name the command property the \
                                             TARGET HANDLER is idempotent on, as a $ref to that property. It is the \
                                             routed door's `external_id`, and there is no default because the safe axis \
                                             does not follow from the target: an order is delivered once (order id), a \
                                             customer is credited many times on a ledger keyed by customer (reclamation \
                                             id). Guessing it drops money or duplicates it, in silence.",
                                            mname
                                        ),
                                    )),
                                    Some(prop) => {
                                        let has_prop = resolve_ref(model, mref, CTX)
                                            .map(|d| props_info(model, d, CTX))
                                            .unwrap_or_default()
                                            .iter()
                                            .any(|p| *p.0 == *prop);
                                        if !has_prop {
                                            issues.push(err(
                                                "pm-send-dedup",
                                                format!("{}.dedup_by", sw),
                                                format!(
                                                    "routed send '{}' keys its door on '{}', which is not a property of \
                                                     that command — the generated `external_id` would not compile, and \
                                                     a dedup axis that is not IN the payload cannot be one.",
                                                    mname, prop
                                                ),
                                            ));
                                        }
                                    }
                                }
                            }
                            if has_gate && !has_to {
                                issues.push(err(
                                    "pm-route-gate",
                                    format!("{}.to", sw),
                                    format!(
                                        "send '{}' declares `route_gate:` with no `to:` — a gate on nothing. The key reads \
                                         as an operable rollback lever (it resolves, it is in the boot report) while no \
                                         route consults it. Name the target actor whose lane receives the command.",
                                        mname
                                    ),
                                ));
                            }
                        }
                        let to = body.get("to").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("");
                        if ref_target_file(to, CTX).as_deref() != Some("actors.yaml") {
                            issues.push(err(rule, format!("{}.to", sw), format!("{}.to must reference an actors.yaml aggregate, got '{}'.", kind, to)));
                        } else {
                            let tname = ref_name(to).unwrap_or_default();
                            match agg_inbox.get(&tname) {
                                None => issues.push(err(rule, format!("{}.to", sw), format!("aggregate '{}' does not exist in actors.yaml.", tname))),
                                Some(inbox) if !inbox.contains(&mname) => issues.push(err(
                                    rule,
                                    format!("{}.to", sw),
                                    format!("aggregate '{}' does not receive '{}' (actors.yaml inbox) — the {} would be dropped.", tname, mname, mkey),
                                )),
                                _ => {}
                            }
                            // `pm-deliver-lane` (#588, ADR-20260816-040239, evans' compiler-first
                            // condition). A `deliver:` is a TELL that becomes a lane ENQUEUE: the
                            // saga decides the fact, the target's own mailbox worker appends it.
                            // Two things must therefore be true of the target, and the inbox-name
                            // check above proves neither — it matches a bare NAME against the
                            // merged `receives` set, so a COMMAND of the same name satisfies it
                            // and an actor with no mailbox passes it. Enforced rather than
                            // renamed: the DSL word `deliver` and the spec notes already say
                            // Tell; only the code was lying (no keyword split, no rename).
                            if kind == "deliver" {
                                match agg_facts.get(&tname) {
                                    Some((_, false)) => issues.push(err(
                                        "pm-deliver-lane",
                                        format!("{}.to", sw),
                                        format!(
                                            "aggregate '{}' declares no `mailbox:` — a deliver: is a lane ENQUEUE (ADR-20260816-040239), so '{}' would have no lane to arrive on.",
                                            tname, mname
                                        ),
                                    )),
                                    Some((facts, true)) if !facts.contains(&mname) => issues.push(err(
                                        "pm-deliver-lane",
                                        format!("{}.to", sw),
                                        format!(
                                            "aggregate '{}' does not declare '{}' as an inbound EVENT in its actors.yaml `receives` — a deliver: is a lane ENQUEUE (ADR-20260816-040239) and only an events.yaml fact can ride a lane; the birth would be refused at the door.",
                                            tname, mname
                                        ),
                                    )),
                                    _ => {}
                                }
                            }
                        }
                        let target_props = resolve_ref(model, mref, CTX).map(|d| props_info(model, d, CTX)).unwrap_or_default();
                        if let Some(wmap) = body.get("with").and_then(|x| x.as_mapping()) {
                            for (wk, wv) in wmap {
                                let prop = wk.as_str().unwrap_or("?");
                                if !target_props.is_empty() && !target_props.contains_key(prop) {
                                    issues.push(err(rule, format!("{}.with.{}", sw, prop), format!("'{}' is not a property of '{}'.", prop, mname)));
                                }
                                check_pm_value(wv, target_props.get(prop).and_then(|e| e.as_ref()), state_cols.as_ref(), &aliases, &ports, &format!("{}.with.{}", sw, prop), issues);
                            }
                        }
                        if let Some(fe) = body.get("for_each").and_then(|x| x.as_str()) {
                            if !aliases.contains_key(fe) {
                                issues.push(err(rule, format!("{}.for_each", sw), format!("for_each alias '{}' is not a prior read step.", fe)));
                            }
                        }
                    }
                    "state" => {
                        let cols = match state_cols.as_ref() {
                            None => {
                                issues.push(err("pm-state", sw.clone(), "a `state` step requires the process manager to declare `state_table`.".into()));
                                continue;
                            }
                            Some(c) => c,
                        };
                        for section in ["by", "expect", "set"] {
                            if let Some(smap2) = body.get(section).and_then(|x| x.as_mapping()) {
                                for (ck, cv) in smap2 {
                                    let col = ck.as_str().unwrap_or("?");
                                    let cw = format!("{}.{}.{}", sw, section, col);
                                    if !cols.contains_key(col) {
                                        issues.push(err("pm-state", cw.clone(), format!("column '{}' does not exist on the state table.", col)));
                                    }
                                    if section == "expect" && cv.get("const").is_none() {
                                        issues.push(err("pm-state", cw.clone(), "an `expect` value must be structural: { const: <ENUM> }.".into()));
                                        continue;
                                    }
                                    check_pm_value(cv, cols.get(col).and_then(|e| e.as_ref()), state_cols.as_ref(), &aliases, &ports, &cw, issues);
                                }
                            }
                        }
                    }
                    other => {
                        issues.push(err("pm-step", sw, format!("unknown step kind '{}' (read | guard | call | deliver | send | state).", other)));
                    }
                }
            }
        }
    }
}

