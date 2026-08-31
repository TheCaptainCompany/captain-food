use crate::*;

// ─── crates/application/src/generated/process_managers.rs (issue #25 — PM orchestrator pipelines) ──

/// One typed value form of the PM step DSL (`where`/`with`/`by`/`expect`/`set`, ADR-20260719-172821).
#[derive(Clone, Debug)]
pub(crate) enum PmVal {
    /// `{ const: <ENUM_MEMBER | integer> }`.
    Const(Value),
    /// `{ from: { $ref: '<file>#/<Msg>/properties/<prop>' } }` — a message/event property.
    From { owner: String, prop: String },
    /// `{ from_state: <column> }` — the PM's own state row.
    FromState(String),
    /// `{ from_read: <alias>.<column> }` — a prior read step's result.
    FromRead { alias: String, col: String },
    /// `{ from_port: <port>.<operation> }` — a prior call step's result (only the hand-written
    /// PlaceOrder command leg consumes one; the generated pipelines never do).
    FromPort,
    /// `{ from_envelope: event_id | correlation_id | occurred_at }` (ADR-0041).
    FromEnvelope(String),
    /// `{ from_hook: <name> }` — an ORCHESTRATOR-computed value (#60). Emits an async call to a
    /// generated per-leg hook `async fn <name>(&self, <leg-scope ctx>) -> Result<T, _>` that resolves
    /// the value at runtime (e.g. from config tables). Unlike `from_state`, it needs NO state row, so
    /// it is usable on a BIRTH leg; the hook receives whatever the leg has in scope (the trigger, any
    /// reads/delivered payloads, and the state row when one is loaded). Only valid inside `state.set`.
    FromHook(String),
}

pub(crate) fn parse_pm_val(v: &Value, loc: &str) -> PmVal {
    if let Some(c) = v.get("const") {
        return PmVal::Const(c.clone());
    }
    if let Some(f) = v.get("from") {
        let r = f
            .get("$ref")
            .and_then(|x| x.as_str())
            .unwrap_or_else(|| panic!("{}: `from` without $ref", loc));
        let pr = parse_ref(r).unwrap_or_else(|| panic!("{}: malformed $ref '{}'", loc, r));
        assert!(
            pr.path.len() == 3 && pr.path[1] == "properties",
            "{}: expected <file>#/<Msg>/properties/<prop>, got '{}'",
            loc,
            r
        );
        return PmVal::From { owner: pr.path[0].clone(), prop: pr.path[2].clone() };
    }
    if let Some(s) = v.get("from_state").and_then(|x| x.as_str()) {
        return PmVal::FromState(s.to_string());
    }
    if let Some(s) = v.get("from_read").and_then(|x| x.as_str()) {
        let (alias, col) = s
            .split_once('.')
            .unwrap_or_else(|| panic!("{}: from_read '{}' is not <alias>.<column>", loc, s));
        return PmVal::FromRead { alias: alias.to_string(), col: col.to_string() };
    }
    if v.get("from_port").is_some() {
        return PmVal::FromPort;
    }
    if let Some(s) = v.get("from_envelope").and_then(|x| x.as_str()) {
        return PmVal::FromEnvelope(s.to_string());
    }
    if let Some(s) = v.get("from_hook").and_then(|x| x.as_str()) {
        return PmVal::FromHook(s.to_string());
    }
    panic!("{}: unsupported value form {:?}", loc, v)
}

/// One ordered typed step of a PM leg (the closed DSL vocabulary, ADR-20260719-172821).
pub(crate) enum PmStepDef {
    Read { table: String, alias: String, where_: Vec<(String, PmVal)>, note: Option<String> },
    /// `that` = (subject, field, const-member) when structurally expressible.
    Guard { that: Option<(String, String, String)>, throws: Option<String>, skip: bool, note: Option<String> },
    Call { port: String, operation: String, note: Option<String> },
    /// `route_gate` = the `configuration.yaml#/keys/<KEY>` name this deliver's LANE ROUTING is
    /// gated on, when the DSL declares one. `Some` IS what makes the step routed: there is no
    /// separate routed-set list to keep in step, and a route whose key does not resolve is a
    /// dangling `$ref` — a `make validate` error (#797).
    Deliver { event: String, to: String, with: Vec<(String, PmVal)>, note: Option<String>, route_gate: Option<String> },
    /// A `send:` is a REQUEST the target may reject, and it addresses a stream the process
    /// manager does not own — so `to` names the target whose lane runs the command and
    /// `route_gate` names the key that route is gated on (#807). Both are `Option` for the same
    /// reason `PmSendDecl`'s are: `pm-route-gate` reports the half-declared forms, and the
    /// emitter treats them as unrouted so a validate error stays a validate error rather than
    /// becoming a codegen panic. `route_decls` reads the both-present PAIR.
    Send {
        command: String,
        to: Option<String>,
        with: Vec<(String, PmVal)>,
        for_each: Option<String>,
        note: Option<String>,
        route_gate: Option<String>,
        /// The command PROPERTY whose value is the routed door's dedup axis (`external_id`), i.e.
        /// the key the TARGET HANDLER is idempotent on. Mandatory on a routed send, with NO
        /// default, because every available default is silently wrong somewhere: keying the door
        /// on the target aggregate's id is right for `MarkOrderDelivered` (an order is delivered
        /// once) and catastrophic for `GrantCustomerCredit`, whose ledger is keyed by CUSTOMER
        /// while the handler dedups per `reclamationId` — one customer legitimately receives many
        /// grants, and a customer-keyed door would swallow every one after the first. Money owed
        /// and never paid, with no error anywhere.
        dedup_by: Option<String>,
    },
    StateStep { by: Vec<(String, PmVal)>, expect: Vec<(String, String)>, set: Vec<(String, PmVal)>, note: Option<String> },
}

pub(crate) struct PmLegDef {
    pub(crate) msg_file: String,
    pub(crate) msg: String,
    pub(crate) description: Option<String>,
    pub(crate) steps: Vec<PmStepDef>,
    /// Commands the leg's hand-written wrapper sends that no step can express (`sends:`, #595).
    /// They are NOT steps and generate no pipeline code; they are carried so the DECLARED
    /// routed-lane table below sees them, because a lane that is watched only because some OTHER
    /// route happens to address it is a lane one deletion away from unwatched.
    pub(crate) sends: Vec<PmSendDecl>,
}

/// One DECLARED wrapper-seam `sends:` entry.
///
/// `to` + `route_gate` are what a ROUTED send needs and a plain declaration does not: the target
/// actor whose lane receives the command, and the `configuration.yaml` key that route is gated on.
/// Both present ⇒ routed; both absent ⇒ the command is declared for wiring/coverage only. One
/// without the other is refused by `pm-route-gate` (`validate::process_managers`) — a gate with no
/// target is a gate on nothing, and a target with no gate is worse than an ungateable route:
/// `route_decls` below reads the both-present PAIR, so the send would silently leave `ROUTED_LANES`
/// (its lane dropping out of #783's dead-man's-switch population) with no `Route` variant emitted
/// for it, at 0 validate errors. That rule was named here before it existed, and the shape it
/// claims to refuse was reachable — round 2 of #797's review.
pub(crate) struct PmSendDecl {
    /// The full `commands.yaml#/Name` ref string — what `pm-sends-kind`/`pm-sends-no-inbox` read.
    pub(crate) command_ref: String,
    /// Bare command name.
    pub(crate) command: String,
    /// `actors.yaml` key of the target whose lane receives it, when routed.
    pub(crate) to: Option<String>,
    /// `configuration.yaml#/keys/<KEY>` name gating the route, when routed.
    pub(crate) route_gate: Option<String>,
}

pub(crate) struct PmOrchDef {
    pub(crate) name: String,
    pub(crate) state_table: Option<String>,
    /// Outbound ports: (port name, services.yaml service name).
    pub(crate) ports: Vec<(String, String)>,
    pub(crate) legs: Vec<PmLegDef>,
}

/// Legs deliberately NOT generated (roadmap item 3 non-goal: genuinely computed business logic —
/// the PlaceOrder command leg is the server-side pricing path; it stays `commands::place_order`).
pub(crate) const PM_HAND_WRITTEN_LEGS: &[(&str, &str)] = &[("PlaceOrderProcess", "PlaceOrder")];

/// One DECLARED lane ROUTE — a `deliver:` or wrapper-seam `sends:` that goes through the target
/// actor's mailbox lane instead of being appended to its stream from the saga
/// (ADR-20260816-040239), together with the configuration key that route is gated on (#797).
///
/// **The routed set is DSL-declared, not a const in this file.** It used to be two hand-kept
/// lists here (`PM_LANE_ROUTED_DELIVERS`, `SENDS_LANE_ROUTED`) with the runtime flag named only in
/// prose, and the routed step then read bare sink PRESENCE — so route selection was `sink.is_some()`
/// and every route on a given runner shared one boolean. A route declares its own gate now, so the
/// two cannot drift: the gate is a `$ref` the refs walker resolves, and the step consults
/// `Route::<this route>` by name.
///
/// Gate-then-stabilize still prices a route move one route at a time: `route_gate:` presence is the
/// RECORD of which pairs have been moved, never a predicate that sweeps every `deliver:` the moment
/// the grammar allows it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RouteDecl {
    /// `actors.yaml` key of the TARGET — its mailbox lane receives the message.
    pub(crate) actor_type: String,
    /// `events.yaml` key for a routed `deliver:` (a FACT), `commands.yaml` key for a routed
    /// `sends:` (a REQUEST the target may refuse).
    pub(crate) message_type: String,
    /// FROZEN door identity, first half: `pm:{ProcessManager}:{Message}`.
    pub(crate) source: String,
    /// `configuration.yaml#/keys/<KEY>` name gating this route.
    pub(crate) config_key: String,
    /// A routed `deliver:` (fact) rather than a routed `sends:` (command).
    pub(crate) is_fact: bool,
}

impl RouteDecl {
    /// The generated `Route` variant name: `{Message}To{Actor}` — the pair IS the route identity,
    /// and both halves are in the name so two routes to the same lane cannot collide.
    pub(crate) fn variant(&self) -> String {
        format!("{}To{}", self.message_type, self.actor_type)
    }

    /// The generated `RouteGates` field name.
    pub(crate) fn field(&self) -> String {
        snake_type(&self.variant())
    }
}

/// Every DECLARED route, sorted and deduped — read from the MODEL, which is the only place the
/// routed set is written down (#797).
pub(crate) fn route_decls(model: &Model) -> Vec<RouteDecl> {
    let mut out: Vec<RouteDecl> = Vec::new();
    for pm in parse_pm_orchestrators(model) {
        for leg in &pm.legs {
            for step in &leg.steps {
                if let PmStepDef::Deliver { event, to, route_gate: Some(key), .. } = step {
                    out.push(RouteDecl {
                        actor_type: to.clone(),
                        message_type: event.clone(),
                        source: format!("pm:{}:{}", pm.name, event),
                        config_key: key.clone(),
                        is_fact: true,
                    });
                }
                // A routed `send:` STEP (#807). The wrapper-seam arm below reads the same
                // both-present pair off a `sends:` DECLARATION; a step is parsed by different
                // code, so it needs its own arm or its `to:` is read, validated by `pm-send` and
                // then dropped -- which is exactly what happened to all four committed sends.
                // `is_fact: false`: a send is a REQUEST the target may refuse, so it takes the
                // COMMAND door, not the EVENT one.
                if let PmStepDef::Send { command, to: Some(to), route_gate: Some(key), .. } = step {
                    out.push(RouteDecl {
                        actor_type: to.clone(),
                        message_type: command.clone(),
                        source: format!("pm:{}:{}", pm.name, command),
                        config_key: key.clone(),
                        is_fact: false,
                    });
                }
            }
            for send in &leg.sends {
                match (&send.to, &send.route_gate) {
                    (Some(to), Some(key)) => out.push(RouteDecl {
                        actor_type: to.clone(),
                        message_type: send.command.clone(),
                        source: format!("pm:{}:{}", pm.name, send.command),
                        config_key: key.clone(),
                        is_fact: false,
                    }),
                    // `pm-route-gate` (validate::process_managers) reports the half-declared forms;
                    // the emitter treats them as unrouted so a validate error is a validate error,
                    // never a codegen panic. Falling through here is precisely why the rule has to
                    // exist: without it this arm drops the route from ROUTED_LANES and the `Route`
                    // enum in silence, at 0 errors.
                    _ => {}
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Aggregate → payload key property for `deliver` stream addressing. Convention: `<camel(agg)>Id`;
/// the Payment aggregate is keyed by the Stripe intent (`domain::payment::stream`).
pub(crate) fn pm_aggregate_key(aggregate: &str) -> String {
    if aggregate == "Payment" {
        "paymentIntentId".to_string()
    } else {
        format!("{}Id", camel(aggregate))
    }
}

/// The DECLARED identity property of an aggregate — `actors.yaml#/<Actor>/identity`, whose `$ref`
/// last segment names the state field the stream is keyed by.
///
/// This is what [`pm_aggregate_key`]'s `<camel(agg)>Id` convention only APPROXIMATES, and the
/// approximation is wrong wherever an aggregate is keyed by something other than its own name:
/// `CustomerCredit` is keyed by `customerId` (one ledger per customer, stream
/// `CustomerCredit-{customerId}`), which the convention would call `customerCreditId` -- a property
/// no command carries. `Payment` needed a hard-coded special case for the same reason. Routed
/// `send:` steps (#807) read the DECLARATION instead, so a new aggregate never needs a new arm here.
pub(crate) fn pm_actor_identity_prop(model: &Model, actor: &str) -> String {
    model
        .defs
        .get("actors.yaml")
        .and_then(|f| f.get(actor))
        .and_then(|n| n.get("identity"))
        .and_then(|i| i.get("$ref"))
        .and_then(|r| r.as_str())
        .and_then(|r| r.rsplit('/').next())
        .map(|s| s.to_string())
        .unwrap_or_else(|| pm_aggregate_key(actor))
}

/// Read a `{ $ref: 'configuration.yaml#/keys/<KEY>' }` node into the bare KEY name (#797).
///
/// The `$ref` — not a bare string — is the whole point: the refs walker resolves it, so a route
/// gated on a key that does not exist is a `make validate` error rather than a silently-`false`
/// lookup at runtime. That is the compiler-first half of "adding a route without adding its key
/// is a build failure".
pub(crate) fn config_key_ref(node: Option<&Value>, loc: &str) -> Option<String> {
    let node = node?;
    let r = node
        .get("$ref")
        .and_then(|r| r.as_str())
        .unwrap_or_else(|| panic!("{}: route_gate must be a $ref to configuration.yaml#/keys/<KEY>", loc));
    Some(
        r.strip_prefix("configuration.yaml#/keys/")
            .unwrap_or_else(|| {
                panic!("{}: route_gate must $ref configuration.yaml#/keys/<KEY>, got '{}'", loc, r)
            })
            .to_string(),
    )
}

/// Parse a leg's `sends:` list. Each entry is `{ command: {$ref}, [to: {$ref}], [route_gate: {$ref}] }`.
pub(crate) fn parse_pm_sends(node: Option<&Value>, loc: &str) -> Vec<PmSendDecl> {
    let mut out = Vec::new();
    for (i, e) in node.and_then(|x| x.as_sequence()).into_iter().flatten().enumerate() {
        let sloc = format!("{}.sends[{}]", loc, i);
        let command_ref = e
            .get("command")
            .and_then(|c| c.get("$ref"))
            .and_then(|r| r.as_str())
            .unwrap_or_else(|| panic!("{}: a sends: entry needs `command: {{ $ref: 'commands.yaml#/<Name>' }}`", sloc))
            .to_string();
        let command = ref_name(&command_ref).unwrap_or_else(|| panic!("{}: unreadable command $ref", sloc));
        let to = e.get("to").and_then(|x| x.get("$ref")).and_then(|r| r.as_str()).and_then(ref_name);
        let route_gate = config_key_ref(e.get("route_gate"), &sloc);
        out.push(PmSendDecl { command_ref, command, to, route_gate });
    }
    out
}

pub(crate) fn pm_val_entries(node: Option<&Value>, loc: &str) -> Vec<(String, PmVal)> {
    let mut out = Vec::new();
    if let Some(Value::Mapping(m)) = node {
        for (k, v) in m {
            if let Some(key) = k.as_str() {
                out.push((key.to_string(), parse_pm_val(v, &format!("{}/{}", loc, key))));
            }
        }
    }
    out
}

pub(crate) fn parse_pm_orchestrators(model: &Model) -> Vec<PmOrchDef> {
    let mut out = Vec::new();
    let Some(Value::Mapping(m)) = model.defs.get("processmanager.yaml") else {
        return out;
    };
    for (k, node) in m {
        let Some(name) = k.as_str() else { continue };
        if node.get("type").and_then(|t| t.as_str()) != Some("process-manager") {
            continue;
        }
        let state_table = node
            .get("state_table")
            .and_then(|s| s.get("$ref"))
            .and_then(|r| r.as_str())
            .and_then(ref_name);
        let mut ports = Vec::new();
        if let Some(Value::Mapping(pm)) = node.get("ports") {
            for (pk, pv) in pm {
                if let (Some(port), Some(svc)) = (
                    pk.as_str(),
                    pv.get("$ref").and_then(|r| r.as_str()).and_then(ref_name),
                ) {
                    ports.push((port.to_string(), svc));
                }
            }
        }
        let mut legs = Vec::new();
        for (i, leg) in node
            .get("receives")
            .and_then(|r| r.as_sequence())
            .map(|s| s.iter())
            .into_iter()
            .flatten()
            .enumerate()
        {
            let mref = leg
                .get("message")
                .and_then(|x| x.get("$ref"))
                .and_then(|r| r.as_str())
                .unwrap_or_else(|| panic!("processmanager.yaml#/{}/receives[{}]: missing message $ref", name, i));
            let pr = parse_ref(mref).unwrap_or_else(|| panic!("processmanager.yaml#/{}: bad message ref '{}'", name, mref));
            let msg = pr.path[0].clone();
            if PM_HAND_WRITTEN_LEGS.contains(&(name, msg.as_str())) {
                continue;
            }
            let loc = format!("processmanager.yaml#/{}/receives[{}]", name, i);
            let mut steps = Vec::new();
            for (j, step) in leg
                .get("steps")
                .and_then(|s| s.as_sequence())
                .map(|s| s.iter())
                .into_iter()
                .flatten()
                .enumerate()
            {
                let sloc = format!("{}/steps[{}]", loc, j);
                let sm = step.as_mapping().unwrap_or_else(|| panic!("{}: step is not a mapping", sloc));
                let (kind, body) = sm
                    .iter()
                    .next()
                    .map(|(k, v)| (k.as_str().unwrap_or(""), v))
                    .unwrap_or_else(|| panic!("{}: empty step", sloc));
                let note = body.get("note").and_then(|n| n.as_str()).map(|s| s.to_string());
                steps.push(match kind {
                    "read" => PmStepDef::Read {
                        table: body
                            .get("model")
                            .and_then(|x| x.get("$ref"))
                            .and_then(|r| r.as_str())
                            .and_then(ref_name)
                            .unwrap_or_else(|| panic!("{}: read without model $ref", sloc)),
                        alias: body
                            .get("as")
                            .and_then(|a| a.as_str())
                            .unwrap_or_else(|| panic!("{}: read without `as`", sloc))
                            .to_string(),
                        where_: pm_val_entries(body.get("where"), &sloc),
                        note,
                    },
                    "guard" => {
                        let that = body.get("that").and_then(|t| t.as_mapping()).map(|t| {
                            let (subject, fields) = t.iter().next().expect("guard.that subject");
                            let fm = fields.as_mapping().expect("guard.that fields");
                            let (field, cv) = fm.iter().next().expect("guard.that field");
                            let member = cv
                                .get("const")
                                .and_then(|c| c.as_str())
                                .unwrap_or_else(|| panic!("{}: guard.that without const", sloc));
                            (
                                subject.as_str().unwrap().to_string(),
                                field.as_str().unwrap().to_string(),
                                member.to_string(),
                            )
                        });
                        PmStepDef::Guard {
                            that,
                            throws: body
                                .get("throws")
                                .and_then(|x| x.get("$ref"))
                                .and_then(|r| r.as_str())
                                .and_then(ref_name),
                            skip: body.get("skip").and_then(|s| s.as_bool()) == Some(true),
                            note,
                        }
                    }
                    "call" => PmStepDef::Call {
                        port: body.get("port").and_then(|p| p.as_str()).unwrap_or_else(|| panic!("{}: call without port", sloc)).to_string(),
                        operation: body.get("operation").and_then(|p| p.as_str()).unwrap_or_else(|| panic!("{}: call without operation", sloc)).to_string(),
                        note,
                    },
                    "deliver" => {
                        assert!(body.get("for_each").is_none(), "{}: deliver.for_each is not supported by the generator yet", sloc);
                        PmStepDef::Deliver {
                            event: body
                                .get("event")
                                .and_then(|x| x.get("$ref"))
                                .and_then(|r| r.as_str())
                                .and_then(ref_name)
                                .unwrap_or_else(|| panic!("{}: deliver without event $ref", sloc)),
                            to: body
                                .get("to")
                                .and_then(|x| x.get("$ref"))
                                .and_then(|r| r.as_str())
                                .and_then(ref_name)
                                .unwrap_or_else(|| panic!("{}: deliver without target $ref", sloc)),
                            with: pm_val_entries(body.get("with"), &sloc),
                            note,
                            route_gate: config_key_ref(body.get("route_gate"), &sloc),
                        }
                    }
                    "send" => PmStepDef::Send {
                        command: body
                            .get("command")
                            .and_then(|x| x.get("$ref"))
                            .and_then(|r| r.as_str())
                            .and_then(ref_name)
                            .unwrap_or_else(|| panic!("{}: send without command $ref", sloc)),
                        // NOT `unwrap_or_else(panic)` like `deliver.to`: a `send:` with a missing
                        // or malformed target is reported by `pm-send`/`pm-route-gate`, and a
                        // validator that has to survive a mutated model in order to REPORT on it
                        // cannot have the parser abort underneath it (#807).
                        to: body
                            .get("to")
                            .and_then(|x| x.get("$ref"))
                            .and_then(|r| r.as_str())
                            .and_then(ref_name),
                        with: pm_val_entries(body.get("with"), &sloc),
                        for_each: body.get("for_each").and_then(|f| f.as_str()).map(|s| s.to_string()),
                        note,
                        route_gate: config_key_ref(body.get("route_gate"), &sloc),
                        dedup_by: body
                            .get("dedup_by")
                            .and_then(|x| x.get("$ref"))
                            .and_then(|r| r.as_str())
                            .and_then(|r| r.rsplit('/').next())
                            .map(|s| s.to_string()),
                    },
                    "state" => {
                        let mut expect = Vec::new();
                        if let Some(Value::Mapping(em)) = body.get("expect") {
                            for (ck, cv) in em {
                                expect.push((
                                    ck.as_str().unwrap().to_string(),
                                    cv.get("const")
                                        .and_then(|c| c.as_str())
                                        .unwrap_or_else(|| panic!("{}: state.expect without const", sloc))
                                        .to_string(),
                                ));
                            }
                        }
                        PmStepDef::StateStep {
                            by: pm_val_entries(body.get("by"), &sloc),
                            expect,
                            set: pm_val_entries(body.get("set"), &sloc),
                            note,
                        }
                    }
                    other => panic!("{}: unknown step kind '{}'", sloc, other),
                });
            }
            let sends: Vec<PmSendDecl> = parse_pm_sends(leg.get("sends"), &format!("processmanager.yaml#/{}/{}", name, msg));
            legs.push(PmLegDef {
                msg_file: pr.file.clone(),
                msg,
                description: leg.get("description").and_then(|d| d.as_str()).map(|s| s.to_string()),
                steps,
                sends,
            });
        }
        out.push(PmOrchDef { name: name.to_string(), state_table, ports, legs });
    }
    out
}

/// PascalCase of a snake_case name (`open_carts` → `OpenCarts`).
pub(crate) fn pm_pascal(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut cs = w.chars();
            match cs.next() {
                Some(c) => c.to_ascii_uppercase().to_string() + cs.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Fully-qualified Rust path for a bare generated type name (`Money` → `domain::generated::entities::Money`).
pub(crate) fn pm_qualify(model: &Model, ty: &str) -> String {
    if let Some(inner) = ty.strip_prefix("Vec<").and_then(|t| t.strip_suffix('>')) {
        return format!("Vec<{}>", pm_qualify(model, inner));
    }
    if let Some(inner) = ty.strip_prefix("Option<").and_then(|t| t.strip_suffix('>')) {
        return format!("Option<{}>", pm_qualify(model, inner));
    }
    match ty {
        "String" | "i32" | "i64" | "bool" | "f64" | "serde_json::Value" => ty.to_string(),
        "chrono::DateTime<chrono::Utc>" => ty.to_string(),
        name if model.defs.get("scalars.yaml").map(|s| s.get(name).is_some()).unwrap_or(false) => {
            format!("domain::generated::scalars::{}", name)
        }
        name if model.defs.get("entities.yaml").map(|s| s.get(name).is_some()).unwrap_or(false) => {
            format!("domain::generated::entities::{}", name)
        }
        other => panic!("pm orchestrator emitter: cannot qualify type '{}'", other),
    }
}

/// Whether a (bare or qualified) type is `Copy` in the generated Rust (uuid/int/enum scalar newtypes
/// and numeric primitives are; String-backed newtypes, entities and json values are not).
pub(crate) fn pm_is_copy(model: &Model, ty: &str) -> bool {
    let base = ty.rsplit("::").next().unwrap_or(ty);
    if let Some(inner) = base.strip_prefix("Option<").and_then(|t| t.strip_suffix('>')) {
        return pm_is_copy(model, inner);
    }
    match base {
        "i32" | "i64" | "bool" | "f64" => true,
        "String" | "serde_json::Value" => false,
        name => model
            .defs
            .get("scalars.yaml")
            .and_then(|s| s.get(name))
            .map(|node| {
                node.get("enum").map(|e| e.is_sequence()).unwrap_or(false)
                    || node.get("format").and_then(|f| f.as_str()) == Some("uuid")
                    || node.get("type").and_then(|t| t.as_str()) == Some("integer")
            })
            .unwrap_or(false),
    }
}

pub(crate) fn pm_clone_if_needed(model: &Model, ty: &str, expr: String) -> String {
    if pm_is_copy(model, ty) {
        expr
    } else {
        format!("{}.clone()", expr)
    }
}

/// A message/event property's full Rust type (Option-wrapped per required/nullable) + base type.
pub(crate) fn pm_prop_type(model: &Model, file: &str, owner: &str, prop: &str) -> (String, String) {
    let node = model
        .defs
        .get(file)
        .and_then(|f| f.get(owner))
        .unwrap_or_else(|| panic!("pm emitter: {}#/{} not found", file, owner));
    let pnode = node
        .get("properties")
        .and_then(|p| p.get(prop))
        .unwrap_or_else(|| panic!("pm emitter: {}#/{}/properties/{} not found", file, owner, prop));
    let base = struct_field_type(file, owner, prop, pnode);
    let required = node
        .get("required")
        .and_then(|r| r.as_sequence())
        .map(|s| s.iter().any(|v| v.as_str() == Some(prop)))
        .unwrap_or(false);
    let nullable = pnode.get("nullable").and_then(|x| x.as_bool()) == Some(true);
    let full = if (nullable || !required) && !base.starts_with("Vec<") {
        format!("Option<{}>", base)
    } else {
        base.clone()
    };
    (full, base)
}

/// Adapt a source expression of `src` type to the `target` type (both bare, un-qualified type
/// strings). Closed coercion set — anything else is a spec/emitter gap, failed loudly.
pub(crate) fn pm_adapt(model: &Model, expr: String, src: &str, target: &str) -> String {
    if src == target {
        return pm_clone_if_needed(model, target, expr);
    }
    if target == format!("Option<{}>", src) {
        return format!("Some({})", pm_clone_if_needed(model, src, expr));
    }
    if src == "Money" && target == "MoneyCents" {
        return format!("{}.amount_cents", expr);
    }
    // A conditionally-present amount consumed on a branch that already guarantees it is present
    // (ReclamationProcess GOODWILL_CREDIT arm: refundAmount is checked by the leg's branch guard before
    // this send runs, ADR-20260726-163737 / #158) — unwrap on that invariant, never a silent default.
    if src == "Option<Money>" && target == "Money" {
        return format!(
            "{}.clone().expect(\"saga value guaranteed present by the leg's branch guard\")",
            expr
        );
    }
    if src == "Money" && target == "Option<MoneyCents>" {
        return format!("Some({}.amount_cents)", expr);
    }
    if src == "i32" && target == "i64" {
        return format!("i64::from({})", expr);
    }
    panic!("pm emitter: no coercion from {} to {} for `{}`", src, target, expr)
}

/// How one consumed read-column is typed — by its most demanding sink (ADR-20260721 issue #25).
pub(crate) struct PmReadField {
    pub(crate) col: String,
    /// Bare (un-qualified) Rust type, incl. `Option<…>` where the sink allows absence.
    pub(crate) ty: String,
    pub(crate) doc: String,
}

pub(crate) struct PmReadInfo {
    pub(crate) alias: String,
    pub(crate) table: String,
    pub(crate) is_vec: bool,
    /// Hook parameters — the `where` values sourced from the message (const filters stay doc-only).
    pub(crate) params: Vec<(String, String)>,
    pub(crate) fields: Vec<PmReadField>,
}

/// Everything the emitter derives once per PM before writing code.
pub(crate) struct PmEmit<'a> {
    pub(crate) model: &'a Model,
    /// The PM's state table (row/store base name, columns, pk) — `None` for stateless PMs.
    pub(crate) table: Option<&'a PmTable>,
    pub(crate) reads: Vec<PmReadInfo>,
}

impl<'a> PmEmit<'a> {
    pub(crate) fn col(&self, name: &str) -> &SqlColumn {
        self.table
            .and_then(|t| t.columns.iter().find(|c| c.name == name))
            .unwrap_or_else(|| panic!("pm emitter: state column '{}' not found", name))
    }
    /// Bare Rust type of a state column (Option-wrapped per nullability).
    pub(crate) fn col_ty(&self, name: &str) -> String {
        let c = self.col(name);
        let t = self.table.unwrap();
        let base = pm_ty(self.model, &t.table, &c.name, &c.ty).field();
        if c.nullable {
            format!("Option<{}>", base)
        } else {
            base
        }
    }
    pub(crate) fn read(&self, alias: &str) -> &PmReadInfo {
        self.reads
            .iter()
            .find(|r| r.alias == alias)
            .unwrap_or_else(|| panic!("pm emitter: read alias '{}' not found", alias))
    }
    pub(crate) fn read_field_ty(&self, alias: &str, col: &str) -> String {
        self.read(alias)
            .fields
            .iter()
            .find(|f| f.col == col)
            .unwrap_or_else(|| panic!("pm emitter: read field {}.{} not found", alias, col))
            .ty
            .clone()
    }
}

/// Derive each read alias's hook signature: `where` params from the message, and one struct field
/// per CONSUMED column, typed by its most demanding sink (state column > payload literal > guard >
/// builder-only, where builder-only fields are `Option<sink type>` so the hook can signal absence).
pub(crate) fn pm_read_infos<'a>(model: &'a Model, pm: &PmOrchDef, table: Option<&PmTable>) -> Vec<PmReadInfo> {
    let mut reads: Vec<PmReadInfo> = Vec::new();
    // Pass 1: declare aliases (dedup identical re-declarations across legs).
    for leg in &pm.legs {
        for step in &leg.steps {
            if let PmStepDef::Read { table: t, alias, where_, .. } = step {
                let mut params = Vec::new();
                for (col, val) in where_ {
                    if let PmVal::From { owner, prop } = val {
                        let (_, base) = pm_prop_type(model, &leg.msg_file, owner, prop);
                        params.push((rust_ident(col), base));
                    }
                }
                if let Some(existing) = reads.iter().find(|r| r.alias == *alias) {
                    assert!(
                        existing.table == *t && existing.params == params,
                        "processmanager.yaml#/{}: read alias '{}' re-declared with a different shape",
                        pm.name,
                        alias
                    );
                } else {
                    reads.push(PmReadInfo {
                        alias: alias.clone(),
                        table: t.clone(),
                        is_vec: false,
                        params,
                        fields: Vec::new(),
                    });
                }
            }
        }
    }
    // Vecness: an alias a send/deliver iterates with `for_each`.
    for leg in &pm.legs {
        for step in &leg.steps {
            if let PmStepDef::Send { for_each: Some(alias), .. } = step {
                if let Some(r) = reads.iter_mut().find(|r| r.alias == *alias) {
                    r.is_vec = true;
                }
            }
        }
    }
    // Pass 2: consumed columns and their sink types.
    let views = parse_views(model);
    let add_field = |reads: &mut Vec<PmReadInfo>, alias: &str, col: &str, ty: String, doc: String| {
        let r = reads
            .iter_mut()
            .find(|r| r.alias == alias)
            .unwrap_or_else(|| panic!("processmanager.yaml#/{}: from_read references unknown alias '{}'", pm.name, alias));
        if let Some(f) = r.fields.iter().find(|f| f.col == col) {
            assert!(
                f.ty == ty,
                "processmanager.yaml#/{}: read field {}.{} typed both {} and {} by its sinks",
                pm.name, alias, col, f.ty, ty
            );
        } else {
            r.fields.push(PmReadField { col: col.to_string(), ty, doc });
        }
    };
    for leg in &pm.legs {
        for step in &leg.steps {
            match step {
                PmStepDef::Guard { that: Some((subject, field, member)), .. }
                    if reads.iter().any(|r| r.alias == *subject) =>
                {
                    let tname = reads.iter().find(|r| r.alias == *subject).unwrap().table.clone();
                    let view = views
                        .iter()
                        .find(|v| v.name == tname)
                        .unwrap_or_else(|| panic!("pm emitter: projection table '{}' not found", tname));
                    let colty = view
                        .columns
                        .iter()
                        .find(|c| c.name == *field)
                        .map(|c| projection_rust_type(&c.ty))
                        .unwrap_or_else(|| panic!("pm emitter: {}.{} not found", tname, field));
                    add_field(&mut reads, subject, field, colty, format!("Feeds the `= {}` guard.", member));
                }
                PmStepDef::StateStep { set, .. } => {
                    for (col, val) in set {
                        if let PmVal::FromRead { alias, col: rcol } = val {
                            let t = table.unwrap_or_else(|| panic!("pm emitter: {} sets state without a state_table", pm.name));
                            let c = t
                                .columns
                                .iter()
                                .find(|c| c.name == *col)
                                .unwrap_or_else(|| panic!("pm emitter: state column '{}' not found", col));
                            let base = pm_ty(model, &t.table, &c.name, &c.ty).field();
                            let ty = if c.nullable { format!("Option<{}>", base) } else { base };
                            add_field(&mut reads, alias, rcol, ty, format!("Feeds `state.set {}`.", col));
                        }
                    }
                }
                PmStepDef::Deliver { event, with, .. } => {
                    let covered = pm_deliver_covered(model, "events.yaml", event, with);
                    for (prop, val) in with {
                        if let PmVal::FromRead { alias, col } = val {
                            let (full, base) = pm_prop_type(model, "events.yaml", event, prop);
                            let ty = if covered { full } else { format!("Option<{}>", base) };
                            add_field(&mut reads, alias, col, ty, format!("Feeds `{}.{}`.", event, prop));
                        }
                    }
                }
                PmStepDef::Send { command, with, for_each, .. } => {
                    for (prop, val) in with {
                        if let PmVal::FromRead { alias, col } = val {
                            // A `for_each` send reads from the ITERATED alias — same typing rule.
                            let _ = for_each;
                            let (full, _) = pm_prop_type(model, "commands.yaml", command, prop);
                            add_field(&mut reads, alias, col, full, format!("Feeds `{}.{}`.", command, prop));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    reads
}

/// A deliver/send payload is fully expressible ("covered") when every REQUIRED property has a `with`
/// entry — otherwise a builder hook owns the whole payload (frozen snapshots, derived ids…).
pub(crate) fn pm_deliver_covered(model: &Model, file: &str, msg: &str, with: &[(String, PmVal)]) -> bool {
    let node = model
        .defs
        .get(file)
        .and_then(|f| f.get(msg))
        .unwrap_or_else(|| panic!("pm emitter: {}#/{} not found", file, msg));
    node.get("required")
        .and_then(|r| r.as_sequence())
        .map(|s| {
            s.iter()
                .filter_map(|v| v.as_str())
                .all(|req| with.iter().any(|(p, _)| p == req))
        })
        .unwrap_or(true)
}

/// Escape a spec note for embedding inside a generated `format!` string literal.
pub(crate) fn pm_str_lit(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('{', "{{").replace('}', "}}")
}

/// One hook method registered while emitting a leg body (deduped by name — e.g. `finalize`).
pub(crate) struct PmHook {
    pub(crate) name: String,
    /// Full trait-item source (doc comment + signature, default body where one exists).
    pub(crate) code: String,
    pub(crate) is_async: bool,
}

pub(crate) fn pm_push_hook(hooks: &mut Vec<PmHook>, name: &str, is_async: bool, code: String) {
    if !hooks.iter().any(|h| h.name == name) {
        hooks.push(PmHook { name: name.to_string(), code, is_async });
    }
}

/// Leg-local emission scope: what a value expression may reference at the current step.
pub(crate) struct PmScope<'a> {
    pub(crate) msg_file: &'a str,
    pub(crate) msg: &'a str,
    pub(crate) msg_var: &'a str,
    pub(crate) row: bool,
    /// Delivered/built event payload names in scope (var = `snake_type(name)`).
    pub(crate) payloads: Vec<String>,
    pub(crate) loop_alias: Option<String>,
}

pub(crate) fn pm_value_expr(ctx: &PmEmit, scope: &PmScope, val: &PmVal, target_full: &str, loc: &str) -> String {
    let model = ctx.model;
    match val {
        PmVal::Const(v) => {
            let base = target_full
                .strip_prefix("Option<")
                .and_then(|t| t.strip_suffix('>'))
                .unwrap_or(target_full);
            if let Some(member) = v.as_str() {
                let e = format!("domain::generated::scalars::{}::{}", base, member);
                if target_full.starts_with("Option<") {
                    format!("Some({})", e)
                } else {
                    e
                }
            } else if let Some(n) = v.as_i64() {
                assert!(matches!(base, "i32" | "i64"), "{}: integer const for non-integer target {}", loc, target_full);
                n.to_string()
            } else {
                panic!("{}: unsupported const {:?}", loc, v)
            }
        }
        PmVal::From { owner, prop } => {
            let (file, var) = if owner == scope.msg {
                (scope.msg_file.to_string(), scope.msg_var.to_string())
            } else if scope.payloads.iter().any(|p| p == owner) {
                ("events.yaml".to_string(), snake_type(owner))
            } else {
                panic!("{}: `from` references {} which is neither the trigger nor a delivered payload", loc, owner)
            };
            let (full, _) = pm_prop_type(model, &file, owner, prop);
            let expr = format!("{}.{}", var, rust_ident(&snake_field(prop)));
            pm_adapt(model, expr, &full, target_full)
        }
        PmVal::FromState(col) => {
            assert!(scope.row, "{}: from_state with no state row in scope", loc);
            let src = ctx.col_ty(col);
            pm_adapt(model, format!("row.{}", rust_ident(col)), &src, target_full)
        }
        PmVal::FromRead { alias, col } => {
            let expr = if scope.loop_alias.as_deref() == Some(alias.as_str()) {
                format!("item.{}", rust_ident(col))
            } else {
                format!("{}.{}", rust_ident(alias), rust_ident(col))
            };
            let src = ctx.read_field_ty(alias, col);
            pm_adapt(model, expr, &src, target_full)
        }
        PmVal::FromEnvelope(kind) => {
            assert!(kind == "event_id", "{}: from_envelope {} not supported by the generator yet", loc, kind);
            let base = target_full
                .strip_prefix("Option<")
                .and_then(|t| t.strip_suffix('>'))
                .unwrap_or(target_full);
            assert!(base == "ExternalReference", "{}: from_envelope event_id must target ExternalReference", loc);
            let e = "domain::generated::scalars::ExternalReference(env.event_id.to_string())".to_string();
            if target_full.starts_with("Option<") {
                format!("Some({})", e)
            } else {
                e
            }
        }
        PmVal::FromPort => panic!("{}: from_port values are only used by the hand-written PlaceOrder command leg", loc),
        PmVal::FromHook(_) => panic!("{}: from_hook values are only valid inside state.set (handled by emit_state)", loc),
    }
}

/// Emit a full struct literal for a COVERED deliver/send payload: every property in spec order —
/// `with` entries as value expressions, absent optionals as `None`/empty `Vec`.
pub(crate) fn pm_payload_literal(
    ctx: &PmEmit,
    scope: &PmScope,
    file: &str,
    msg_name: &str,
    with: &[(String, PmVal)],
    path: &str,
    var: &str,
    indent: &str,
    loc: &str,
) -> String {
    let node = ctx.model.defs.get(file).and_then(|f| f.get(msg_name)).unwrap();
    let mut out = format!("{}let {} = {} {{\n", indent, var, path);
    if let Some(props) = node.get("properties").and_then(|p| p.as_mapping()) {
        for (pk, _) in props {
            let prop = pk.as_str().unwrap();
            let ident = rust_ident(&snake_field(prop));
            let (full, _) = pm_prop_type(ctx.model, file, msg_name, prop);
            let expr = if let Some((_, val)) = with.iter().find(|(p, _)| p == prop) {
                pm_value_expr(ctx, scope, val, &full, &format!("{}/{}", loc, prop))
            } else if full.starts_with("Option<") {
                "None".to_string()
            } else if full.starts_with("Vec<") {
                "Vec::new()".to_string()
            } else {
                panic!("{}: required property '{}' has no `with` entry", loc, prop)
            };
            out.push_str(&format!("{}    {}: {},\n", indent, ident, expr));
        }
    }
    out.push_str(&format!("{}}};\n", indent));
    out
}

/// Mutable state while emitting one leg's pipeline + hooks trait.
pub(crate) struct PmLegGen<'a> {
    pub(crate) ctx: &'a PmEmit<'a>,
    pub(crate) pm: &'a PmOrchDef,
    pub(crate) leg: &'a PmLegDef,
    pub(crate) is_cmd: bool,
    pub(crate) msg_var: &'static str,
    pub(crate) msg_path: String,
    pub(crate) row_ty: Option<String>,
    pub(crate) body: String,
    pub(crate) hooks: Vec<PmHook>,
    pub(crate) row_in_scope: bool,
    pub(crate) payloads: Vec<String>,
    /// Hook context refs available so far: (param name, param type, call-site expression).
    pub(crate) hook_ctx: Vec<(String, String, String)>,
    /// The correlation identity of the loaded row: (json context key, raw value expression).
    pub(crate) by_ctx: Option<(String, String)>,
    pub(crate) admission_pending: bool,
    pub(crate) single_send: bool,
}

impl<'a> PmLegGen<'a> {
    pub(crate) fn scope(&self, loop_alias: Option<String>) -> PmScope<'a> {
        PmScope {
            msg_file: &self.leg.msg_file,
            msg: &self.leg.msg,
            msg_var: self.msg_var,
            row: self.row_in_scope,
            payloads: self.payloads.clone(),
            loop_alias,
        }
    }
    pub(crate) fn table(&self) -> &PmTable {
        self.ctx.table.unwrap_or_else(|| panic!("processmanager.yaml#/{}: leg needs a state_table", self.pm.name))
    }
    pub(crate) fn row_ty(&self) -> &str {
        self.row_ty.as_deref().expect("state row type")
    }
    pub(crate) fn actor_ref(&self) -> &'static str {
        if self.is_cmd { "actor" } else { "&actor" }
    }
    pub(crate) fn hook_sig(&self) -> String {
        self.hook_ctx.iter().map(|(n, t, _)| format!("{}: {}", n, t)).collect::<Vec<_>>().join(", ")
    }
    pub(crate) fn hook_args(&self) -> String {
        self.hook_ctx.iter().map(|(_, _, e)| e.clone()).collect::<Vec<_>>().join(", ")
    }
    pub(crate) fn push(&mut self, ind: usize, line: &str) {
        if line.is_empty() {
            self.body.push('\n');
        } else {
            self.body.push_str(&" ".repeat(ind));
            self.body.push_str(line);
            self.body.push('\n');
        }
    }

    /// The raw (un-adapted) expression for a `by`/pk value — only message properties are supported.
    pub(crate) fn raw_msg_expr(&self, val: &PmVal, loc: &str) -> String {
        match val {
            PmVal::From { owner, prop } => {
                assert!(owner == &self.leg.msg, "{}: by/pk value must come from the trigger message", loc);
                format!("{}.{}", self.msg_var, rust_ident(&snake_field(prop)))
            }
            _ => panic!("{}: by/pk value must be a `from` message property", loc),
        }
    }

    pub(crate) fn emit_admission(&mut self, ind: usize) {
        let t = self.table();
        let pk = t.pk.clone();
        // The pk value comes from this leg's own `state.set`.
        let pk_val = self
            .leg
            .steps
            .iter()
            .find_map(|s| match s {
                PmStepDef::StateStep { set, .. } => set.iter().find(|(c, _)| *c == pk).map(|(_, v)| v.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("processmanager.yaml#/{}: opening leg sets no pk '{}'", self.pm.name, pk));
        let raw = self.raw_msg_expr(&pk_val, "admission");
        let c = self.ctx.col(&pk);
        let by_ref = pm_ty(self.ctx.model, &self.table().table, &c.name, &c.ty).by_ref();
        let arg = if by_ref { format!("&{}", raw) } else { raw };
        let method = pm_lookup_method(&pk);
        self.push(ind, "// admission: the run row this leg would (re-)open — `admit` may veto with a benign skip.");
        self.push(ind, &format!("if let Some(existing) = state.{}({}).await? {{", method, arg));
        self.push(ind + 4, "if let Some(reason) = hooks.admit(&existing) {");
        self.push(ind + 8, "return Ok(Outcome::Skipped(reason));");
        self.push(ind + 4, "}");
        self.push(ind, "}");
        let row_ty = self.row_ty().to_string();
        pm_push_hook(
            &mut self.hooks,
            "admit",
            false,
            format!(
                "        /// Whether an EXISTING run row may be re-upserted by this opening leg — `Some(reason)` ends\n        /// the leg as a benign skip (e.g. never regress a decided run). Default: admit.\n        fn admit(&self, _existing: &{}) -> Option<String> {{\n            None\n        }}\n",
                row_ty
            ),
        );
        self.admission_pending = false;
    }
}

impl<'a> PmLegGen<'a> {
    pub(crate) fn emit_read(&mut self, table: &str, alias: &str, where_: &[(String, PmVal)], note: &Option<String>, ind: usize) {
        assert!(!self.is_cmd, "read steps on command legs are not generated (hand-written PlaceOrder only)");
        let info = self.ctx.read(alias);
        let struct_name = format!("{}Read", pm_pascal(alias));
        let mut args = Vec::new();
        let mut where_doc = Vec::new();
        for (col, val) in where_ {
            match val {
                PmVal::From { owner, prop } => {
                    let (full, base) = pm_prop_type(self.ctx.model, &self.leg.msg_file, owner, prop);
                    assert!(full == base, "read where {}.{}: optional message properties unsupported", alias, col);
                    let expr = format!("{}.{}", self.msg_var, rust_ident(&snake_field(prop)));
                    args.push(pm_clone_if_needed(self.ctx.model, &base, expr));
                    where_doc.push(format!("{} = message.{}", col, prop));
                }
                PmVal::Const(c) => where_doc.push(format!("{} = {}", col, c.as_str().unwrap_or("?"))),
                other => panic!("read where {}.{}: unsupported value {:?}", alias, col, other),
            }
        }
        let note_s = note.as_deref().map(|n| format!(" {}", ws1(n))).unwrap_or_default();
        self.push(ind, &format!("// read `{}` ← {} where {}.{}", alias, table, where_doc.join(", "), note_s));
        if info.is_vec {
            self.push(ind, &format!("let {} = hooks.read_{}({}).await?;", rust_ident(alias), alias, args.join(", ")));
        } else {
            self.push(ind, &format!("let {} = match hooks.read_{}({}).await? {{", rust_ident(alias), alias, args.join(", ")));
            self.push(ind + 4, "super::HookOutcome::Ready(v) => v,");
            self.push(ind + 4, "super::HookOutcome::Skip(reason) => return Ok(Outcome::Skipped(reason)),");
            self.push(ind, "};");
        }
        let params = info
            .params
            .iter()
            .map(|(n, t)| format!("{}: {}", n, pm_qualify(self.ctx.model, t)))
            .collect::<Vec<_>>()
            .join(", ");
        let ret = if info.is_vec {
            format!("Result<Vec<{}>, domain::shared::errors::DomainError>", struct_name)
        } else {
            format!("Result<super::HookOutcome<{}>, domain::shared::errors::DomainError>", struct_name)
        };
        let absence = if info.is_vec { "Absence = empty (no skip)." } else { "Return `Skip` to end the leg as a benign no-op." };
        pm_push_hook(
            &mut self.hooks,
            &format!("read_{}", alias),
            true,
            format!(
                "        /// Execute `read {}` over `{}` (where {}).{} {}\n        async fn read_{}(&self, {}) -> {};\n",
                alias, table, where_doc.join(", "), note_s, absence, alias, params, ret
            ),
        );
        let ctx_ty = if info.is_vec { format!("&[{}]", struct_name) } else { format!("&{}", struct_name) };
        self.hook_ctx.push((rust_ident(alias), ctx_ty, format!("&{}", rust_ident(alias))));
    }

    pub(crate) fn emit_guard_that(&mut self, subject: &str, field: &str, member: &str, throws: &Option<String>, skip: bool, note: &Option<String>, ind: usize) {
        let model = self.ctx.model;
        let (expr, base) = match subject {
            "message" => {
                let (full, base) = pm_prop_type(model, &self.leg.msg_file, &self.leg.msg, field);
                assert!(full == base, "guard on optional message property {} unsupported", field);
                (format!("{}.{}", self.msg_var, rust_ident(&snake_field(field))), base)
            }
            "state" => {
                assert!(self.row_in_scope, "guard on state with no row in scope");
                let ty = self.ctx.col_ty(field);
                assert!(!ty.starts_with("Option<"), "guard on nullable state column {} unsupported", field);
                (format!("row.{}", rust_ident(field)), ty)
            }
            alias => (format!("{}.{}", rust_ident(alias), rust_ident(field)), self.ctx.read_field_ty(alias, field)),
        };
        let cmp = if base == "String" {
            format!("\"{}\"", member)
        } else {
            format!("domain::generated::scalars::{}::{}", base, member)
        };
        let note_s = note.as_deref().map(ws1).unwrap_or_default();
        if skip {
            self.push(ind, &format!("// guard {}.{} = {} — benign alternative: {}", subject, field, member, note_s));
            self.push(ind, &format!("if {} != {} {{", expr, cmp));
            self.push(
                ind + 4,
                &format!(
                    "return Ok(Outcome::Skipped(format!(\"{}.{} is {{:?}}, not {} — {}\", {})));",
                    subject, field, member, pm_str_lit(&note_s), expr
                ),
            );
            self.push(ind, "}");
        } else {
            let err = throws.as_deref().expect("guard without skip must throw");
            let (key, raw) = self.by_ctx.clone().unwrap_or_else(|| panic!("guard throws {} with no correlation context", err));
            self.push(ind, &format!("// guard {}.{} = {} — throws {}. {}", subject, field, member, err, note_s));
            self.push(ind, &format!("if {} != {} {{", expr, cmp));
            self.push(
                ind + 4,
                &format!(
                    "return Err(domain::shared::errors::DomainError::rejected(\"{}\", serde_json::json!({{ \"{}\": &{} }})));",
                    err, key, raw
                ),
            );
            self.push(ind, "}");
        }
    }

    pub(crate) fn emit_call(&mut self, port: &str, operation: &str, note: &Option<String>, ind: usize) {
        if self.admission_pending {
            self.emit_admission(ind);
        }
        let svc = self
            .pm
            .ports
            .iter()
            .find(|(p, _)| p == port)
            .map(|(_, s)| s.clone())
            .unwrap_or_else(|| panic!("processmanager.yaml#/{}: undeclared port '{}'", self.pm.name, port));
        let base = pm_pascal(&svc);
        let op = pm_pascal(operation);
        let corr = if self.is_cmd { "actor.correlation_id" } else { "env.correlation_id" };
        let note_s = note.as_deref().map(|n| format!(" — {}", ws1(n))).unwrap_or_default();
        let hook = format!("input_{}_{}", port, operation);
        self.push(ind, &format!("// call {}.{}{}", port, operation, note_s));
        self.push(ind, &format!("match hooks.{}({}).await? {{", hook, self.hook_args()));
        self.push(ind + 4, "super::HookOutcome::Ready(input) => {");
        self.push(
            ind + 8,
            &format!(
                "{}.{}(input, &crate::generated::services::ServiceCallMeta::new({})).await?;",
                rust_ident(port), rust_ident(operation), corr
            ),
        );
        self.push(ind + 4, "}");
        self.push(ind + 4, "super::HookOutcome::Skip(reason) => {");
        self.push(ind + 8, &format!("tracing::warn!(saga = \"{}\", port = \"{}\", operation = \"{}\", %reason, \"service call skipped\");", self.pm.name, port, operation));
        self.push(ind + 4, "}");
        self.push(ind, "}");
        let sig = self.hook_sig();
        pm_push_hook(
            &mut self.hooks,
            &hook,
            true,
            format!(
                "        /// Build the `{}.{}` input for this leg.{} `Skip` skips just this call — the leg continues.\n        async fn {}(&self, {}) -> Result<super::HookOutcome<crate::generated::services::{}{}Input>, domain::shared::errors::DomainError>;\n",
                port, operation, note_s, hook, sig, base, op
            ),
        );
    }

    pub(crate) fn emit_deliver(&mut self, event: &str, to: &str, with: &[(String, PmVal)], note: &Option<String>, route_gate: &Option<String>, ind: usize) {
        if self.admission_pending {
            self.emit_admission(ind);
        }
        // A routed deliver reads `env.lane_sink_for(..)`, which only EVENT legs carry (`env_needed`).
        assert!(
            !self.is_cmd || route_gate.is_none(),
            "processmanager.yaml#/{}: a lane-ROUTED deliver ({} -> {}) on a COMMAND leg has no \
             trigger envelope to carry the lane sink",
            self.pm.name, event, to
        );
        let model = self.ctx.model;
        let var = snake_type(event);
        let covered = pm_deliver_covered(model, "events.yaml", event, with);
        let note_s = note.as_deref().map(|n| format!(" — {}", ws1(n))).unwrap_or_default();
        self.push(ind, &format!("// deliver {} → {} (the aggregate records the fact){}", event, to, note_s));
        if covered {
            let lit = pm_payload_literal(
                self.ctx,
                &self.scope(None),
                "events.yaml",
                event,
                with,
                &format!("domain::generated::events::{}", event),
                &var,
                &" ".repeat(ind),
                &format!("processmanager.yaml#/{}/{}", self.pm.name, event),
            );
            self.body.push_str(&lit);
        } else {
            assert!(!self.is_cmd, "builder-hook delivers on command legs are not supported");
            self.push(ind, &format!("let {} = match hooks.build_{}({}).await? {{", var, var, self.hook_args()));
            self.push(ind + 4, "super::HookOutcome::Ready(v) => v,");
            self.push(ind + 4, "super::HookOutcome::Skip(reason) => return Ok(Outcome::Skipped(reason)),");
            self.push(ind, "};");
            let withs = with.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>().join(", ");
            let sig = self.hook_sig();
            pm_push_hook(
                &mut self.hooks,
                &format!("build_{}", var),
                true,
                format!(
                    "        /// Build the FULL `{}` payload — the DSL `with` covers only [{}]; the rest is computed\n        /// (spec note:{}). `Skip` ends the leg as a benign no-op; `Err` aborts and surfaces.\n        async fn build_{}(&self, {}) -> Result<super::HookOutcome<domain::generated::events::{}>, domain::shared::errors::DomainError>;\n",
                    event, withs, note_s, var, sig, event
                ),
            );
        }
        self.payloads.push(event.to_string());
        self.hook_ctx.push((var.clone(), format!("&domain::generated::events::{}", event), format!("&{}", var)));
        // Stream addressing: the target aggregate's key, from the payload, the state row, or a read.
        let key_prop = pm_aggregate_key(to);
        let key_snake = snake_field(&key_prop);
        let in_payload = model
            .defs
            .get("events.yaml")
            .and_then(|f| f.get(event))
            .and_then(|n| n.get("properties"))
            .map(|p| p.get(key_prop.as_str()).is_some())
            .unwrap_or(false);
        let routed = route_gate.is_some();
        let route_variant = format!("{}To{}", event, to);
        let pm_name = self.pm.name.clone();
        let append = |gen: &mut Self, key_expr: &str, ind: usize| {
            let (route_ind, legacy_ind) = if routed { (ind + 4, ind + 4) } else { (ind, ind) };
            if routed {
                // ADR-20260816-040239: this `deliver:` is a lane ENQUEUE. The saga DECIDES the
                // fact; the target actor's own mailbox lane APPENDS it, so the write passes the
                // target's serialization point and the delivery's `Recorded` verdict is what its
                // `schedules:` key on. `env.lane_sink_for(Route::X)` is `Some` only on a route
                // that built the envelope with `TriggerEnvelope::laned` — i.e. one that owns a
                // fenced transaction to stage into (the field is private, #597) — AND whose
                // `RouteGates` has THIS route's own key on. Naming the route is what makes the
                // gate per ROUTE rather than per runner (#797): bare sink presence would flip
                // every route hosted by the same runner together. `None` takes the legacy
                // foreign-stream append below unchanged (gate-then-stabilize — rollback is a
                // config flip, not a redeploy).
                gen.push(
                    ind,
                    &format!(
                        "if let Some(lanes) = env.lane_sink_for(crate::generated::process_managers::Route::{}) {{",
                        route_variant
                    ),
                );
                gen.push(route_ind, "lanes.stage(crate::lanes::LaneEnqueue {");
                // A `deliver:` is a FACT, so it always takes the EVENT door (#595 made the door a
                // typed field because the `sends:` arm takes the COMMAND one).
                gen.push(route_ind + 4, "kind: crate::lanes::LaneMessageKind::Event,");
                gen.push(route_ind + 4, &format!("actor_type: \"{}\",", to));
                gen.push(route_ind + 4, &format!("actor_id: {}.0,", key_expr));
                gen.push(route_ind + 4, &format!("message_type: \"{}\",", event));
                gen.push(
                    route_ind + 4,
                    &format!(
                        "payload: serde_json::to_value(domain::generated::events::DomainEvent::{}({}.clone())).map_err(|e| domain::shared::errors::DomainError::Repository(format!(\"{} lane enqueue payload: {{e}}\")))?,",
                        event, var, event
                    ),
                );
                // The FROZEN door identity: `UUIDv5(source:external_id)` with `source` = the ROUTE
                // (so two routed steps addressing the same aggregate cannot collide) and
                // `external_id` = the TARGET AGGREGATE's id — never the trigger's message id,
                // which changes on every redelivery and would mint a second birth. Changing
                // either half re-mints the identity of every in-flight message, the same
                // treatment `actor_client::surrogate_actor_id` carries.
                gen.push(
                    route_ind + 4,
                    &format!("source: \"pm:{}:{}\".to_string(),", pm_name, event),
                );
                gen.push(route_ind + 4, &format!("external_id: {}.0.to_string(),", key_expr));
                gen.push(route_ind, "});");
                gen.push(ind, "} else {");
            }
            gen.push(legacy_ind, &format!("let stream = format!(\"{}-{{}}\", {}.0);", to, key_expr));
            gen.push(legacy_ind, "let (stream_events, stream_version) = store.load(&stream).await?;");
            gen.push(legacy_ind, &format!("if hooks.should_deliver_{}(&stream_events, &{}) {{", var, var));
            gen.push(legacy_ind + 4, "crate::repository::Repository::new(store)");
            gen.push(
                legacy_ind + 8,
                &format!(
                    ".save(&stream, stream_version, &[domain::generated::events::DomainEvent::{}({}.clone())], {})",
                    event, var, gen.actor_ref()
                ),
            );
            gen.push(legacy_ind + 8, ".await?;");
            gen.push(legacy_ind, "}");
            if routed {
                gen.push(ind, "}");
            }
        };
        if in_payload {
            append(self, &format!("{}.{}", var, rust_ident(&key_snake)), ind);
        } else {
            // Fall back to the state row, then to a read alias — `None` skips this deliver (the run
            // has no target payment/stream to address; the remaining steps still run).
            let (holder, ty) = if self.row_in_scope && self.ctx.table.map(|t| t.columns.iter().any(|c| c.name == key_snake)).unwrap_or(false) {
                (format!("row.{}", rust_ident(&key_snake)), self.ctx.col_ty(&key_snake))
            } else if let Some(r) = self.ctx.reads.iter().find(|r| r.fields.iter().any(|f| f.col == key_snake)) {
                (format!("{}.{}", rust_ident(&r.alias), rust_ident(&key_snake)), self.ctx.read_field_ty(&r.alias, &key_snake))
            } else {
                panic!("processmanager.yaml#/{}: cannot address {} stream for {}", self.pm.name, to, event)
            };
            if ty.starts_with("Option<") {
                self.push(ind, &format!("if let Some(deliver_key) = {}.clone() {{", holder));
                append(self, "deliver_key", ind + 4);
                self.push(ind, "}");
            } else {
                append(self, &holder, ind);
            }
        }
        let event_owned = event.to_string();
        pm_push_hook(
            &mut self.hooks,
            &format!("should_deliver_{}", var),
            false,
            format!(
                "        /// Per-aggregate idempotency predicate: given the target stream as loaded, should this\n        /// `{}` be appended? (A re-delivered trigger must find the fact already recorded.)\n        fn should_deliver_{}(&self, stream: &[domain::generated::events::DomainEvent], event: &domain::generated::events::{}) -> bool;\n",
                event_owned, var, event_owned
            ),
        );
    }

    pub(crate) fn emit_send(&mut self, command: &str, to: &Option<String>, with: &[(String, PmVal)], for_each: &Option<String>, note: &Option<String>, route_gate: &Option<String>, dedup_by: &Option<String>, ind: usize) {
        assert!(!self.is_cmd, "send steps on command legs are not generated");
        if self.admission_pending {
            self.emit_admission(ind);
        }
        let cmd_snake = snake_type(command);
        assert!(
            pm_deliver_covered(self.ctx.model, "commands.yaml", command, with),
            "processmanager.yaml#/{}: send {} payload is not fully covered by `with`",
            self.pm.name, command
        );
        let note_s = note.as_deref().map(|n| format!(" — {}", ws1(n))).unwrap_or_default();
        self.push(
            ind,
            &format!("// send {} (the target validates and may reject; a rejection is logged and skipped){}", command, note_s),
        );
        let (body_ind, loop_alias) = if let Some(alias) = for_each {
            self.push(ind, &format!("for item in &{} {{", rust_ident(alias)));
            (ind + 4, Some(alias.clone()))
        } else {
            (ind, None)
        };
        let lit = pm_payload_literal(
            self.ctx,
            &self.scope(loop_alias.clone()),
            "commands.yaml",
            command,
            with,
            &format!("domain::generated::commands::{}", command),
            "sent",
            &" ".repeat(body_ind),
            &format!("processmanager.yaml#/{}/{}", self.pm.name, command),
        );
        self.body.push_str(&lit);
        // The ROUTED arm (#807). A `send:` addresses a stream this saga does not own; routed, the
        // TARGET's own lane worker runs the command inside its fenced transaction, past the
        // target's serialization point, and a rejection lands a REJECTED verdict on a supervisable
        // row instead of the `tracing::warn!` below that no operator reads. `env.lane_sink_for(..)`
        // is `Some` only on a route built with `TriggerEnvelope::laned` AND with THIS route's own
        // gate ON -- the route is NAMED, never inferred from sink presence, so two routes hosted by
        // the same runner cannot flip together (#797). `None` takes the legacy in-process call
        // below, byte for byte: rollback is a config flip, not a redeploy.
        let routed = to.is_some() && route_gate.is_some();
        let (send_ind, legacy_ind) = if routed { (body_ind + 4, body_ind + 4) } else { (body_ind, body_ind) };
        if routed {
            let to = to.as_deref().expect("routed send has a target");
            let key_prop = pm_actor_identity_prop(self.ctx.model, to);
            let key_field = rust_ident(&snake_field(&key_prop));
            assert!(
                self.ctx
                    .model
                    .defs
                    .get("commands.yaml")
                    .and_then(|f| f.get(command))
                    .and_then(|n| n.get("properties"))
                    .map(|p| p.get(key_prop.as_str()).is_some())
                    .unwrap_or(false),
                "processmanager.yaml#/{}: routed send {} -> {} cannot address the lane: the command \
                 does not carry '{}', the identity actors.yaml declares for {}. A routed send is \
                 partitioned onto the TARGET aggregate's id, so that id must be in the command.",
                self.pm.name, command, to, key_prop, to
            );
            self.push(
                body_ind,
                &format!(
                    "if let Some(lanes) = env.lane_sink_for(crate::generated::process_managers::Route::{}To{}) {{",
                    command, to
                ),
            );
            self.push(send_ind, "lanes.stage(crate::lanes::LaneEnqueue {");
            // A `send:` is a REQUEST the target may refuse, so it takes the COMMAND door.
            self.push(send_ind + 4, "kind: crate::lanes::LaneMessageKind::Command,");
            self.push(send_ind + 4, &format!("actor_type: \"{}\",", to));
            self.push(send_ind + 4, &format!("actor_id: sent.{}.0,", key_field));
            self.push(send_ind + 4, &format!("message_type: \"{}\",", command));
            // The BARE command payload: `dispatch_command` deserializes this column straight into
            // the generated command struct, with no `eventType` wrapper (the EVENT door differs).
            self.push(
                send_ind + 4,
                &format!(
                    "payload: serde_json::to_value(&sent).map_err(|e| domain::shared::errors::DomainError::Repository(format!(\"{} lane enqueue payload: {{e}}\")))?,",
                    command
                ),
            );
            // FROZEN door identity: the ROUTE (so two routed steps addressing the same aggregate
            // cannot collide) plus the DECLARED DEDUP AXIS -- never the trigger's message id, which
            // changes on every redelivery, and never the target aggregate's id by default. The axis
            // is the key the TARGET HANDLER is idempotent on, and it is declared per send because
            // it does not follow from the target: `MarkOrderDelivered` dedups on the ORDER (an
            // order is delivered once) while `GrantCustomerCredit` dedups on the RECLAMATION, on a
            // ledger keyed by CUSTOMER -- keying that door on the ledger's own id would silently
            // swallow every goodwill credit after a customer's first.
            let dedup_prop = dedup_by
                .as_deref()
                .unwrap_or_else(|| panic!(
                    "processmanager.yaml#/{}: routed send {} declares no `dedup_by:` — the door's \
                     dedup axis must name the command property the TARGET HANDLER is idempotent on \
                     (`pm-send-dedup` reports this; there is deliberately no default)",
                    self.pm.name, command
                ));
            let dedup_field = rust_ident(&snake_field(dedup_prop));
            self.push(send_ind + 4, &format!("source: \"pm:{}:{}\".to_string(),", self.pm.name, command));
            self.push(send_ind + 4, &format!("external_id: sent.{}.0.to_string(),", dedup_field));
            self.push(send_ind, "});");
            self.push(body_ind, "} else {");
        }
        self.push(legacy_ind, &format!("match crate::commands::{}(store, sent, &actor).await {{", cmd_snake));
        self.push(legacy_ind + 4, "Ok(()) => {}");
        self.push(legacy_ind + 4, "Err(e) if crate::ports::is_version_conflict(&e) => return Err(e),");
        self.push(legacy_ind + 4, "Err(domain::shared::errors::DomainError::Repository(e)) => {");
        self.push(legacy_ind + 8, "return Err(domain::shared::errors::DomainError::Repository(e))");
        self.push(legacy_ind + 4, "}");
        self.push(legacy_ind + 4, "Err(rejection) => {");
        if for_each.is_some() {
            self.push(
                legacy_ind + 8,
                &format!(
                    "tracing::warn!(saga = \"{}\", command = \"{}\", %rejection, \"command rejected -- leg skipped, the target aggregate's own invariants stand\");",
                    self.pm.name, command
                ),
            );
        } else {
            self.push(
                legacy_ind + 8,
                &format!(
                    "let reason = format!(\"{} rejected: {{rejection}} — the target aggregate's own invariants stand; skipped\");",
                    command
                ),
            );
            self.push(legacy_ind + 8, &format!("tracing::warn!(saga = \"{}\", %reason, \"leg skipped\");", self.pm.name));
            self.push(legacy_ind + 8, "leg_outcome = Outcome::Skipped(reason);");
        }
        self.push(legacy_ind + 4, "}");
        self.push(legacy_ind, "}");
        if routed {
            self.push(body_ind, "}");
        }
        if for_each.is_some() {
            self.push(ind, "}");
        }
    }

    pub(crate) fn emit_state(&mut self, i: usize, by: &[(String, PmVal)], expect: &[(String, String)], set: &[(String, PmVal)], note: &Option<String>, consumed: &BTreeSet<usize>, ind: usize) {
        let model = self.ctx.model;
        let note_s = note.as_deref().map(|n| format!(" {}", ws1(n))).unwrap_or_default();
        if !by.is_empty() {
            assert!(by.len() == 1, "state.by with multiple columns unsupported");
            let (col, val) = &by[0];
            let raw = self.raw_msg_expr(val, "state.by");
            let c = self.ctx.col(col);
            let by_ref = pm_ty(model, &self.table().table, &c.name, &c.ty).by_ref();
            let arg = if by_ref { format!("&{}", raw) } else { pm_clone_if_needed(model, &self.ctx.col_ty(col), raw.clone()) };
            let method = pm_lookup_method(col);
            let table_name = self.table().table.clone();
            self.push(ind, &format!("// state.by {} — load the run this trigger correlates to.{}", col, note_s));
            // Missing-row policy: a following bare `guard throws` (or, on a command leg, the first
            // `that`-guard's error) types the orphan; otherwise absence is a benign event-leg skip.
            let missing_err = match self.leg.steps.get(i + 1) {
                Some(PmStepDef::Guard { that: None, throws: Some(e), .. }) if consumed.contains(&(i + 1)) => Some(e.clone()),
                Some(PmStepDef::Guard { that: Some((s, _, _)), throws: Some(e), .. }) if self.is_cmd && s == "state" => Some(e.clone()),
                _ => None,
            };
            let key = serde_camel(col);
            self.push(ind, &format!("let Some(row) = state.{}({}).await? else {{", method, arg));
            match missing_err {
                Some(e) => self.push(
                    ind + 4,
                    &format!(
                        "return Err(domain::shared::errors::DomainError::rejected(\"{}\", serde_json::json!({{ \"{}\": &{} }})));",
                        e, key, raw
                    ),
                ),
                None => {
                    assert!(!self.is_cmd, "command-leg state.by needs a typed missing-row error");
                    self.push(
                        ind + 4,
                        &format!(
                            "return Ok(Outcome::Skipped(format!(\"no {} run for {} {{:?}} — {}\", {})));",
                            table_name, col, pm_str_lit(note_s.trim()), raw
                        ),
                    );
                }
            }
            self.push(ind, "};");
            self.row_in_scope = true;
            self.by_ctx = Some((key, raw));
            let row_ty = self.row_ty().to_string();
            self.hook_ctx.push(("row".to_string(), format!("&{}", row_ty), "&row".to_string()));
        }
        for (col, member) in expect {
            assert!(!self.is_cmd, "state.expect on a command leg has no benign-skip path");
            assert!(self.row_in_scope, "state.expect with no row in scope");
            let ty = self.ctx.col_ty(col);
            let table_name = self.table().table.clone();
            self.push(ind, &format!("// state.expect {} = {} — a failed expect is a benign skip.{}", col, member, note_s));
            self.push(ind, &format!("if row.{} != domain::generated::scalars::{}::{} {{", rust_ident(col), ty, member));
            self.push(
                ind + 4,
                &format!(
                    "return Ok(Outcome::Skipped(format!(\"{} run is {{:?}}, expected {} — {}\", row.{})));",
                    table_name, member, pm_str_lit(note_s.trim()), rust_ident(col)
                ),
            );
            self.push(ind, "}");
        }
        if !set.is_empty() {
            if self.admission_pending {
                self.emit_admission(ind);
            }
            let row_ty = self.row_ty().to_string();
            self.push(ind, &format!("// state.set — upsert the run row (envelope stamps last_update_utc).{}", note_s));
            // Self-referential `from_state` = orchestrator-computed (arithmetic the DSL cannot carry).
            let mut lines: Vec<String> = Vec::new();
            for (col, val) in set {
                if let PmVal::FromHook(name) = val {
                    // Orchestrator-resolved value (#60) — a runtime hook (e.g. reading config tables)
                    // usable on ANY leg incl. a birth leg with no state row. It receives whatever the
                    // leg has in scope (trigger + reads/payloads + the row when one is loaded).
                    let base = pm_qualify(model, &self.ctx.col_ty(col));
                    let sig = self.hook_sig();
                    let args = self.hook_args();
                    self.push(ind, &format!("let hook_{} = hooks.{}({}).await?;", rust_ident(col), name, args));
                    pm_push_hook(
                        &mut self.hooks,
                        name,
                        true,
                        format!(
                            "        /// Orchestrator-resolved `{}` (#60, the DSL's `from_hook` marker — resolved at runtime,\n        /// e.g. from the delivery-strategy config tables).{}\n        async fn {}(&self, {}) -> Result<{}, domain::shared::errors::DomainError>;\n",
                            col, note_s, name, sig, base
                        ),
                    );
                    lines.push(format!("{}: hook_{},", rust_ident(col), rust_ident(col)));
                    continue;
                }
                if let PmVal::FromState(src) = val {
                    if src == col {
                        let base = {
                            let c = self.ctx.col(col);
                            pm_ty(model, &self.table().table, &c.name, &c.ty).field()
                        };
                        self.push(ind, &format!("let computed_{} = hooks.compute_{}(&row);", rust_ident(col), col));
                        pm_push_hook(
                            &mut self.hooks,
                            &format!("compute_{}", col),
                            false,
                            format!(
                                "        /// Orchestrator-computed `{}` (the DSL's self-referential `from_state` marker — the\n        /// spec note carries the arithmetic).{}\n        fn compute_{}(&self, row: &{}) -> {};\n",
                                col, note_s, col, row_ty, base
                            ),
                        );
                        lines.push(format!("{}: computed_{},", rust_ident(col), rust_ident(col)));
                        continue;
                    }
                }
                let target = self.ctx.col_ty(col);
                let expr = pm_value_expr(
                    self.ctx,
                    &self.scope(None),
                    val,
                    &target,
                    &format!("processmanager.yaml#/{}/state.set/{}", self.pm.name, col),
                );
                lines.push(format!("{}: {},", rust_ident(col), expr));
            }
            self.push(ind, &format!("let mut updated = {} {{", row_ty));
            for l in &lines {
                self.push(ind + 4, l);
            }
            if self.row_in_scope {
                self.push(ind + 4, "..row");
            } else {
                // A full literal: unset nullable columns default to None; the envelope stamps the time.
                let set_cols: BTreeSet<&str> = set.iter().map(|(c, _)| c.as_str()).collect();
                let t = self.ctx.table.expect("state.set without state_table");
                for c in &t.columns {
                    if set_cols.contains(c.name.as_str()) {
                        continue;
                    }
                    if c.name == "last_update_utc" {
                        self.push(ind + 4, "// Ignored on write — the store stamps now() (runtime envelope).");
                        self.push(ind + 4, "last_update_utc: chrono::Utc::now(),");
                    } else if c.nullable {
                        self.push(ind + 4, &format!("{}: None,", rust_ident(&c.name)));
                    } else {
                        panic!(
                            "processmanager.yaml#/{}: opening state.set leaves non-nullable '{}' unset",
                            self.pm.name, c.name
                        );
                    }
                }
            }
            self.push(ind, "};");
            self.push(ind, "hooks.finalize(&mut updated);");
            self.push(ind, "state.upsert(&updated).await?;");
            pm_push_hook(
                &mut self.hooks,
                "finalize",
                false,
                format!(
                    "        /// Envelope-owned fix-ups on the row about to be upserted (e.g. clearing a spent\n        /// credential, ADR-20260720-015500) — default: none.\n        fn finalize(&self, _row: &mut {}) {{}}\n",
                    row_ty
                ),
            );
        }
    }
}

/// Emit `crates/application/src/generated/process_managers.rs` — the process-manager ORCHESTRATOR
/// STEP PIPELINES (issue #25, codegen-roadmap item 3): one module per PM of `specs/processmanager.yaml`,
/// one generated `async fn` per leg executing the DSL's ordered typed steps (state `by`/`expect`/`set`
/// with the pk-admission seam, structural guards, port calls, deliver/send with the event-leg
/// rejection-skip semantics, skip/throw plumbing), delegating the NON-STRUCTURAL seams (reads over
/// projections, computed payloads, idempotency predicates, branch arithmetic, envelope row fix-ups)
/// to per-leg hook traits implemented by `crate::process_managers` (roadmap: "hand-written only the
/// non-structural predicates behind generated hook traits").
/// Emit one leg: `(hooks trait, pipeline fn)` — both nested inside the PM's module (indent 4).
pub(crate) fn emit_pm_leg(ctx: &PmEmit, pm: &PmOrchDef, leg: &PmLegDef) -> (String, String) {
    let is_cmd = leg.msg_file == "commands.yaml";
    let msg_var: &'static str = if is_cmd { "cmd" } else { "event" };
    let msg_snake = snake_type(&leg.msg);
    let msg_path = format!(
        "domain::generated::{}::{}",
        if is_cmd { "commands" } else { "events" },
        leg.msg
    );
    let fn_name = if is_cmd { msg_snake.clone() } else { format!("on_{}", msg_snake) };
    let trait_name = format!("{}Hooks", leg.msg);

    let has_deliver = leg.steps.iter().any(|s| matches!(s, PmStepDef::Deliver { .. }));
    let has_send = leg.steps.iter().any(|s| matches!(s, PmStepDef::Send { .. }));
    let store_needed = has_deliver || has_send;
    let has_by = leg
        .steps
        .iter()
        .any(|s| matches!(s, PmStepDef::StateStep { by, .. } if !by.is_empty()));
    let has_set = leg
        .steps
        .iter()
        .any(|s| matches!(s, PmStepDef::StateStep { set, .. } if !set.is_empty()));
    let uses_envelope = leg.steps.iter().any(|s| match s {
        PmStepDef::StateStep { set, .. } => set.iter().any(|(_, v)| matches!(v, PmVal::FromEnvelope(_))),
        _ => false,
    });
    let has_call = leg.steps.iter().any(|s| matches!(s, PmStepDef::Call { .. }));
    let env_needed = !is_cmd && (store_needed || uses_envelope || has_call);
    let single_send = leg
        .steps
        .iter()
        .any(|s| matches!(s, PmStepDef::Send { for_each: None, .. }));
    let admission = !is_cmd && !has_by && has_set && ctx.table.is_some();
    let mut ports_used: Vec<String> = Vec::new();
    for s in &leg.steps {
        if let PmStepDef::Call { port, .. } = s {
            if !ports_used.contains(port) {
                ports_used.push(port.clone());
            }
        }
    }

    let row_ty = ctx.table.map(|t| format!("crate::pm_state::{}Row", t.base));
    let mut gen = PmLegGen {
        ctx,
        pm,
        leg,
        is_cmd,
        msg_var,
        msg_path: msg_path.clone(),
        row_ty,
        body: String::new(),
        hooks: Vec::new(),
        row_in_scope: false,
        payloads: Vec::new(),
        hook_ctx: vec![(
            msg_var.to_string(),
            format!("&{}", msg_path),
            if is_cmd { format!("&{}", msg_var) } else { msg_var.to_string() },
        )],
        by_ctx: None,
        admission_pending: admission,
        single_send,
    };

    // Consumed presence guards: a bare `guard throws` right after `state.by` IS the missing-row policy.
    let mut consumed: BTreeSet<usize> = BTreeSet::new();
    for (i, s) in leg.steps.iter().enumerate() {
        if let PmStepDef::StateStep { by, .. } = s {
            if !by.is_empty() {
                if let Some(PmStepDef::Guard { that: None, throws: Some(_), .. }) = leg.steps.get(i + 1) {
                    consumed.insert(i + 1);
                }
            }
        }
    }
    // A mid-leg bare `guard skip` is the DSL's linear-branch marker (bounded re-offer): the hook
    // decides which branch runs — the steps before the marker (then end), or the steps after it.
    let branch_at = leg.steps.iter().enumerate().find_map(|(i, s)| match s {
        PmStepDef::Guard { that: None, throws: None, skip: true, .. } if !consumed.contains(&i) => Some(i),
        _ => None,
    });

    let emit_one = |gen: &mut PmLegGen, i: usize, step: &PmStepDef, ind: usize, consumed: &BTreeSet<usize>| match step {
        PmStepDef::Read { table, alias, where_, note } => gen.emit_read(table, alias, where_, note, ind),
        PmStepDef::Guard { .. } if consumed.contains(&i) => {}
        PmStepDef::Guard { that: Some((s, f, m)), throws, skip, note } => {
            gen.emit_guard_that(s, f, m, throws, *skip, note, ind)
        }
        PmStepDef::Guard { that: None, .. } => {
            panic!("processmanager.yaml#/{}/{}: non-structural guard outside the supported positions", pm.name, leg.msg)
        }
        PmStepDef::Call { port, operation, note } => gen.emit_call(port, operation, note, ind),
        PmStepDef::Deliver { event, to, with, note, route_gate } => gen.emit_deliver(event, to, with, note, route_gate, ind),
        PmStepDef::Send { command, to, with, for_each, note, route_gate, dedup_by } => gen.emit_send(command, to, with, for_each, note, route_gate, dedup_by, ind),
        PmStepDef::StateStep { by, expect, set, note } => gen.emit_state(i, by, expect, set, note, consumed, ind),
    };

    if let Some(k) = branch_at {
        let prefix_end = leg.steps[..k]
            .iter()
            .position(|s| match s {
                PmStepDef::Call { .. } | PmStepDef::Deliver { .. } | PmStepDef::Send { .. } => true,
                PmStepDef::StateStep { set, .. } => !set.is_empty(),
                _ => false,
            })
            .unwrap_or_else(|| panic!("processmanager.yaml#/{}/{}: branch marker with an empty first branch", pm.name, leg.msg));
        for (i, s) in leg.steps.iter().enumerate().take(prefix_end) {
            emit_one(&mut gen, i, s, 8, &consumed);
        }
        let note = match &leg.steps[k] {
            PmStepDef::Guard { note, .. } => note.clone(),
            _ => unreachable!(),
        };
        let note_s = note.as_deref().map(ws1).unwrap_or_default();
        // ASYNC branch (#60): the decision may read config (e.g. "does a next ranked channel remain?"),
        // so the hook is async + fallible and receives the leg's scope (trigger + the loaded row).
        let branch_sig = gen.hook_sig();
        let branch_args = gen.hook_args();
        pm_push_hook(
            &mut gen.hooks,
            "branch",
            true,
            format!(
                "        /// The DSL's linear-branch decision (a mid-leg bare `skip` guard): `true` runs the steps\n        /// BEFORE the marker and ends the leg; `false` falls through to the steps after it. Resolved at\n        /// runtime (may read config, #60).\n        /// Spec note: {}\n        async fn branch(&self, {}) -> Result<bool, domain::shared::errors::DomainError>;\n",
                pm_str_lit(&note_s), branch_sig
            ),
        );
        gen.push(8, "// Linear-branch marker (bare `skip` guard): the hook chooses the branch.");
        gen.push(8, &format!("if hooks.branch({}).await? {{", branch_args));
        for (i, s) in leg.steps.iter().enumerate().take(k).skip(prefix_end) {
            emit_one(&mut gen, i, s, 12, &consumed);
        }
        gen.push(12, "return Ok(Outcome::Completed);");
        gen.push(8, "}");
        for (i, s) in leg.steps.iter().enumerate().skip(k + 1) {
            emit_one(&mut gen, i, s, 8, &consumed);
        }
    } else {
        for (i, s) in leg.steps.iter().enumerate() {
            emit_one(&mut gen, i, s, 8, &consumed);
        }
    }

    // ── assemble the trait ──
    let any_async = gen.hooks.iter().any(|h| h.is_async);
    let desc = leg
        .description
        .as_deref()
        .map(|d| format!("\n    /// {}", ws1(d)))
        .unwrap_or_default();
    let mut trait_code = format!(
        "\n    /// Non-structural hooks for the generated `{}` leg — the seams the step DSL cannot express\n    /// (reads, computed payloads, idempotency predicates, envelope fix-ups).{}\n",
        leg.msg, desc
    );
    if any_async {
        trait_code.push_str("    #[async_trait::async_trait]\n");
    }
    trait_code.push_str(&format!("    pub trait {}: Send + Sync {{\n", trait_name));
    for (i, h) in gen.hooks.iter().enumerate() {
        if i > 0 {
            trait_code.push('\n');
        }
        trait_code.push_str(&h.code);
    }
    trait_code.push_str("    }\n");

    // ── assemble the fn ──
    let kind = if is_cmd { "COMMAND" } else { "EVENT" };
    let mut fn_code = format!(
        "\n    /// {} leg `{}#/{}` — generated step pipeline (issue #25).{}\n    pub async fn {}(\n",
        kind, leg.msg_file, leg.msg, desc.replace("\n    ///", "\n    ///"), fn_name
    );
    if store_needed {
        fn_code.push_str("        store: &dyn crate::ports::EventStore,\n");
    }
    if let Some(t) = ctx.table {
        fn_code.push_str(&format!("        state: &dyn crate::pm_state::{}StateStore,\n", t.base));
    }
    for port in &ports_used {
        let svc = pm.ports.iter().find(|(p, _)| p == port).map(|(_, s)| s.clone()).unwrap();
        fn_code.push_str(&format!(
            "        {}: &dyn crate::generated::services::{}Service,\n",
            rust_ident(port),
            pm_pascal(&svc)
        ));
    }
    fn_code.push_str(&format!("        hooks: &dyn {},\n", trait_name));
    if is_cmd {
        fn_code.push_str(&format!("        cmd: {},\n", msg_path));
        fn_code.push_str("        actor: &crate::ports::Actor,\n");
    } else {
        fn_code.push_str(&format!("        event: &{},\n", msg_path));
        if env_needed {
            fn_code.push_str("        env: &crate::process_managers::TriggerEnvelope,\n");
        }
    }
    let ret = if is_cmd {
        "Result<(), domain::shared::errors::DomainError>"
    } else {
        "Result<crate::process_managers::Outcome, domain::shared::errors::DomainError>"
    };
    fn_code.push_str(&format!("    ) -> {} {{\n", ret));
    if !is_cmd {
        fn_code.push_str("        use crate::process_managers::Outcome;\n");
        if store_needed {
            fn_code.push_str("        let actor = crate::process_managers::saga_actor(env);\n");
        }
        if single_send {
            fn_code.push_str("        let mut leg_outcome = Outcome::Completed;\n");
        }
    }
    fn_code.push_str(&gen.body);
    if is_cmd {
        fn_code.push_str("        Ok(())\n");
    } else if single_send {
        fn_code.push_str("        Ok(leg_outcome)\n");
    } else {
        fn_code.push_str("        Ok(Outcome::Completed)\n");
    }
    fn_code.push_str("    }\n");
    (trait_code, fn_code)
}

/// Every ROUTED `deliver:` target as `(target actor, event)` — the FACT half of `ROUTED_LANES`.
///
/// It exists so the #780 route gate has an ORACLE that is not the artifact under test: asking
/// `ROUTED_LANES` which pairs are routed, in order to check `ROUTED_LANES`, measures nothing. A
/// routed `sends:` is deliberately excluded — a command may be refused, so it does not belong to a
/// gate about facts that must be recorded.
pub(crate) fn routed_fact_targets(model: &Model) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = route_decls(model)
        .into_iter()
        .filter(|r| r.is_fact)
        .map(|r| (r.actor_type, r.message_type))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Emit the ROUTE surface: `ROUTED_LANES` (the lanes a routed message lands on, #598), the
/// `Route` enumeration and the per-route `RouteGates` (#797).
///
/// Until `ROUTED_LANES` existed the routed set lived only in the codegen's own hand-kept consts,
/// so nothing at runtime could name "the lanes a routed birth lands on". The Order-lane
/// dead-man's switch needs exactly that: it must report on every DECLARED lane on every tick, zero
/// included, and a watcher whose lane set is a hand-kept literal is one forgotten edit away from a
/// lane that is silently unwatched — the failure the switch exists to make impossible.
///
/// `Route` and `RouteGates` are the #797 half. Before them a routed step read bare
/// `env.lane_sink()` PRESENCE, so the route-selection predicate was `sink.is_some()` and every
/// route hosted by one runner shared that runner's single boolean. Now each routed step names its
/// OWN route, `RouteGates` carries one field per route, and — because `RouteGates` derives no
/// `Default` — declaring a route makes every construction site an E0063 until a human decides
/// which configuration key feeds it. Generate the enumeration, keep the decision human-owned.
///
/// `source` is carried because it is the FROZEN door identity's first half
/// (`pm:{ProcessManager}:{Event}`): changing either half re-mints the identity of every in-flight
/// routed message, so having the produced string in a committed generated file puts a rename in
/// the diff instead of in production.
fn emit_routes(routes: &[RouteDecl]) -> String {
    let mut out = String::from(
        "\n/// One ROUTED target — the lane a process manager hands a message to instead of acting on\n\
         /// that aggregate's stream itself (ADR-20260816-040239; `sends:` routes added by #595).\n\
         #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]\n\
         pub struct RoutedLane {\n\
         \x20   /// `actors.yaml` key of the TARGET — its mailbox lane receives the message.\n\
         \x20   pub actor_type: &'static str,\n\
         \x20   /// The message handed over: an `events.yaml` key for a routed `deliver:` (a FACT), a\n\
         \x20   /// `commands.yaml` key for a routed `sends:` (a REQUEST the target may refuse).\n\
         \x20   pub event_type: &'static str,\n\
         \x20   /// FROZEN door identity, first half: `pm:{ProcessManager}:{Event}`.\n\
         \x20   pub source: &'static str,\n\
         }\n\n\
         /// Every routed `deliver:`/`sends:` in the DSL, sorted. The DECLARED population the\n\
         /// Order-lane liveness watch reports on — every tick, every lane, zero included, whatever\n\
         /// each route's own gate says: a lane that only reports when something was routed is a\n\
         /// lane whose silence is ambiguous.\n\
         pub const ROUTED_LANES: &[RoutedLane] = &[\n",
    );
    for r in routes {
        out.push_str(&format!(
            "    RoutedLane {{ actor_type: \"{}\", event_type: \"{}\", source: \"{}\" }},\n",
            r.actor_type, r.message_type, r.source
        ));
    }
    out.push_str("];\n");

    out.push_str(
        "\n/// Every DECLARED lane ROUTE, one variant per routed `deliver:`/`sends:` in\n\
         /// `specs/*/processmanager.yaml` (#797, ADR-20260829-230418 C3).\n\
         ///\n\
         /// A routed step consults ITS OWN route — `env.lane_sink_for(Route::X)` — never bare sink\n\
         /// presence. That is what makes the gate per ROUTE rather than per runner: two routes\n\
         /// hosted by the same runner can be flipped independently, so turning off a misbehaving\n\
         /// route never turns off an unrelated one. A rollback that changes something you did not\n\
         /// intend to change is not a rollback.\n\
         #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]\n\
         pub enum Route {\n",
    );
    for r in routes {
        out.push_str(&format!(
            "    /// {} `{}` → the `{}` lane, gated on `configuration.yaml#/keys/{}`.\n    {},\n",
            if r.is_fact { "Routed `deliver:` of the FACT" } else { "Routed `sends:` of the COMMAND" },
            r.message_type,
            r.actor_type,
            r.config_key,
            r.variant()
        ));
    }
    out.push_str("}\n\nimpl Route {\n");
    out.push_str("    /// Every declared route, sorted — the population a gate or a report iterates.\n");
    out.push_str("    pub const ALL: &'static [Route] = &[\n");
    for r in routes {
        out.push_str(&format!("        Route::{},\n", r.variant()));
    }
    out.push_str("    ];\n\n");
    out.push_str(
        "    /// The `configuration.yaml` key whose position this route reads. Emitted from the\n\
        \x20   /// DSL `route_gate:` `$ref`, so a key that does not exist is a `make validate`\n\
        \x20   /// error, never a silently-`false` lookup at runtime.\n\
        \x20   pub fn config_key(self) -> &'static str {\n        match self {\n",
    );
    for r in routes {
        out.push_str(&format!("            Route::{} => \"{}\",\n", r.variant(), r.config_key));
    }
    out.push_str("        }\n    }\n\n");
    out.push_str("    /// `actors.yaml` key of the target whose lane receives the message.\n    pub fn actor_type(self) -> &'static str {\n        match self {\n");
    for r in routes {
        out.push_str(&format!("            Route::{} => \"{}\",\n", r.variant(), r.actor_type));
    }
    out.push_str("        }\n    }\n\n");
    out.push_str("    /// The message handed over — an `events.yaml` key for a FACT route, a `commands.yaml` key for a COMMAND route.\n    pub fn message_type(self) -> &'static str {\n        match self {\n");
    for r in routes {
        out.push_str(&format!("            Route::{} => \"{}\",\n", r.variant(), r.message_type));
    }
    out.push_str("        }\n    }\n}\n");

    out.push_str(
        "\n/// Where every route's gate STANDS on this process — one field per [`Route`].\n\
         ///\n\
         /// Deliberately NOT `Default` and deliberately not built from a config struct here:\n\
         /// `application` cannot see `crates/server`'s configuration, so every construction site\n\
         /// writes a struct literal naming every field. Declaring a route therefore breaks every\n\
         /// site with E0063 until a human decides which key feeds it — the compiler-first form of\n\
         /// \"adding a route without adding its key is a build failure\" (farley, #797). The\n\
         /// `route_gates_are_not_fused` guard covers the one thing the compiler cannot: a site\n\
         /// feeding one route's field from ANOTHER route's configuration value.\n\
         #[derive(Debug, Clone, Copy, PartialEq, Eq)]\n\
         pub struct RouteGates {\n",
    );
    for r in routes {
        out.push_str(&format!(
            "    /// [`Route::{}`] — `configuration.yaml#/keys/{}`.\n    pub {}: bool,\n",
            r.variant(), r.config_key, r.field()
        ));
    }
    out.push_str("}\n\nimpl RouteGates {\n");
    out.push_str(
        "    /// Every route OFF — the LEGACY position for all of them, and the value a route that\n\
        \x20   /// cannot stage carries. Never a `Default` impl: `..Default::default()` would let a\n\
        \x20   /// new route slip past a construction site unnoticed, which is the whole failure\n\
        \x20   /// this type exists to make impossible.\n    pub const NONE: Self = Self {\n",
    );
    for r in routes {
        out.push_str(&format!("        {}: false,\n", r.field()));
    }
    out.push_str("    };\n\n");
    out.push_str("    /// Is THIS route's gate on?\n    pub fn enabled(&self, route: Route) -> bool {\n        match route {\n");
    for r in routes {
        out.push_str(&format!("            Route::{} => self.{},\n", r.variant(), r.field()));
    }
    out.push_str("        }\n    }\n}\n");
    out
}

pub(crate) fn emit_pm_orchestrators(model: &Model) -> String {
    let tables = parse_pm_tables(model);
    let pms = parse_pm_orchestrators(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/processmanager.yaml — do not edit by hand.\n\
         // Process-manager ORCHESTRATOR STEP PIPELINES (issue #25, ADR-20260719-172821/-193500): one module\n\
         // per process manager, one generated `async fn` per `receives` leg executing the DSL's ordered typed\n\
         // steps. The pipeline (control flow, state row lifecycle, structural guards, deliver/send plumbing,\n\
         // skip/throw semantics) is GENERATED; the non-structural seams the DSL cannot express (projection\n\
         // reads, computed payloads such as frozen snapshots and derived ids, per-aggregate idempotency\n\
         // predicates, the bounded re-offer branch, envelope-owned row fix-ups) are per-leg HOOK traits,\n\
         // implemented next to the wrappers in `crate::process_managers`. The PlaceOrder COMMAND leg stays\n\
         // hand-written (`crate::commands::place_order`) — server-side pricing is a codegen non-goal.\n\n\
         /// How a hook resolves: the value is ready, or the leg (or just this call) ends as a benign skip.\n\
         pub enum HookOutcome<T> {\n    Ready(T),\n    Skip(String),\n}\n",
    );
    out.push_str(&emit_routes(&route_decls(model)));
    for pm in &pms {
        let table = pm.state_table.as_ref().map(|t| {
            tables
                .iter()
                .find(|pt| pt.table == *t)
                .unwrap_or_else(|| panic!("processmanager.yaml#/{}: unknown state_table '{}'", pm.name, t))
        });
        let ctx = PmEmit { model, table, reads: pm_read_infos(model, pm, table) };
        let module = snake_type(&pm.name);
        out.push_str(&format!(
            "\n/// Generated step pipelines for `processmanager.yaml#/{}`.\npub mod {} {{\n",
            pm.name, module
        ));
        // Read-result structs — one per alias, fields typed by their sinks.
        for r in &ctx.reads {
            out.push_str(&format!(
                "\n    /// Columns of `{}` consumed by this PM's `read {}` step, typed by their consumers.\n    #[derive(Debug, Clone, PartialEq)]\n    pub struct {}Read {{\n",
                r.table, r.alias, pm_pascal(&r.alias)
            ));
            for f in &r.fields {
                out.push_str(&format!("        /// {}\n        pub {}: {},\n", f.doc, rust_ident(&f.col), pm_qualify(model, &f.ty)));
            }
            out.push_str("    }\n");
        }
        for leg in &pm.legs {
            let (trait_code, fn_code) = emit_pm_leg(&ctx, pm, leg);
            out.push_str(&trait_code);
            out.push_str(&fn_code);
        }
        out.push_str("}\n");
    }
    out
}

