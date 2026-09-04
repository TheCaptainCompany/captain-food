use crate::*;

// ─── §2c — aggregate lifecycle state machines (actors.yaml `lifecycle`, ADR-20260720-004419) ────

/// One `{ from: [states], event, to[, via[, when]] }` transition of a declared lifecycle. `via`
/// names the event payload field driving the target state (dynamic target, ADR-20260721-093027).
/// With no `when`, the entry legalizes `from × {to}` when `event.<via> == to` (the field must $ref
/// the SAME scalar as the lifecycle's own status). With `when` (#639 part C step 3-ii,
/// ADR-20260904-015903 §2 — the grammar extension the ADR calls for), `via` may name a field of a
/// DIFFERENT enum scalar and the entry legalizes `from × {to}` when `event.<via> == when`; several
/// rows sharing one `(from, event, via)` and differing `when`/`to` values collectively form an
/// explicit value→target map, without a second event.
pub(crate) struct LifecycleTransition {
    pub(crate) from: Vec<String>,
    pub(crate) event_ref: String,
    pub(crate) to: String,
    pub(crate) via: Option<String>,
    pub(crate) when: Option<String>,
}

/// One `{ event, to[, via[, when]] }` birth entry of a declared lifecycle. With `via` (no `when`),
/// the birth state is event-carried (the fold births from the payload field) and `to` stays the
/// canonical birth state; with `when` the birth is conditioned the same way a mapped transition is
/// (see [`LifecycleTransition`]).
pub(crate) struct LifecycleInitial {
    pub(crate) event_ref: String,
    pub(crate) to: String,
    pub(crate) via: Option<String>,
    pub(crate) when: Option<String>,
}

/// A parsed `lifecycle:` block of an actors.yaml aggregate: the status machine as declared data.
/// Tolerant parsing (missing pieces → empty); `validate_lifecycles` reports the holes.
pub(crate) struct Lifecycle {
    pub(crate) aggregate: String,
    pub(crate) status_ref: String,
    pub(crate) initial: Vec<LifecycleInitial>,
    pub(crate) transitions: Vec<LifecycleTransition>,
    pub(crate) terminal: Vec<String>,
    /// `legacyStates:` (#639 part C step 4-i, ADR-20260904-081527 §6): states named only for a
    /// retired entry, exempt from the reachability check by declaration.
    pub(crate) legacy_states: Vec<String>,
}

/// Parse every aggregate's `lifecycle:` block, in actors.yaml order.
pub(crate) fn parse_lifecycles(model: &Model) -> Vec<Lifecycle> {
    let mut out = Vec::new();
    let actors = match model.defs.get("actors.yaml") {
        Some(Value::Mapping(m)) => m,
        _ => return out,
    };
    for (k, node) in actors {
        let name = match k.as_str() {
            Some(s) => s,
            None => continue,
        };
        if node.get("type").and_then(|x| x.as_str()) != Some("aggregate") {
            continue;
        }
        let lc = match node.get("lifecycle") {
            Some(v) => v,
            None => continue,
        };
        let str_seq = |v: Option<&Value>| -> Vec<String> {
            v.and_then(|x| x.as_sequence())
                .map(|s| s.iter().filter_map(|it| it.as_str().map(|x| x.to_string())).collect())
                .unwrap_or_default()
        };
        let event_ref =
            |e: &Value| e.get("event").and_then(|x| x.get("$ref")).and_then(|r| r.as_str()).unwrap_or("").to_string();
        let initial = lc
            .get("initial")
            .and_then(|x| x.as_sequence())
            .map(|s| {
                s.iter()
                    .map(|e| LifecycleInitial {
                        event_ref: event_ref(e),
                        to: e.get("to").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                        via: e.get("via").and_then(|x| x.as_str()).map(str::to_string),
                        when: e.get("when").and_then(|x| x.as_str()).map(str::to_string),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let transitions = lc
            .get("transitions")
            .and_then(|x| x.as_sequence())
            .map(|s| {
                s.iter()
                    .map(|t| LifecycleTransition {
                        from: str_seq(t.get("from")),
                        event_ref: event_ref(t),
                        to: t.get("to").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                        via: t.get("via").and_then(|x| x.as_str()).map(str::to_string),
                        when: t.get("when").and_then(|x| x.as_str()).map(str::to_string),
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(Lifecycle {
            aggregate: name.to_string(),
            status_ref: lc
                .get("status")
                .and_then(|x| x.get("$ref"))
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .to_string(),
            initial,
            transitions,
            terminal: str_seq(lc.get("terminal")),
            legacy_states: str_seq(lc.get("legacyStates")),
        });
    }
    out
}

/// The enum values of a scalars.yaml enum scalar, or `None` when the name is not an enum scalar.
pub(crate) fn scalar_enum_values(model: &Model, scalar: &str) -> Option<Vec<String>> {
    model
        .defs
        .get("scalars.yaml")
        .and_then(|s| s.get(scalar))
        .and_then(|n| n.get("enum"))
        .and_then(|e| e.as_sequence())
        .map(|s| s.iter().filter_map(|v| v.as_str().map(|x| x.to_string())).collect())
}

/// §2c — validate the declared aggregate lifecycles (ADR-20260720-004419): the status is an enum
/// scalar; every named state is a member of it; every claimed event is emitted by THIS aggregate;
/// the machine is deterministic (no two transitions from one state on one event); terminal states
/// have no outgoing transition; every named state is reachable from an initial state. An aggregate
/// whose `<Name>Status` scalar exists (trailing `Job` stripped, so DeliveryJob ↔ DeliveryStatus)
/// but that declares no lifecycle WARNS (`lc-missing`) — adoption is incremental.
pub(crate) fn validate_lifecycles(model: &Model, issues: &mut Vec<Issue>) {
    let actors = match model.defs.get("actors.yaml") {
        Some(Value::Mapping(m)) => m,
        _ => return,
    };
    let lifecycles: BTreeSet<String> = parse_lifecycles(model).into_iter().map(|l| l.aggregate).collect();
    // Coverage: an aggregate with a status scalar but no declared lifecycle.
    for (k, node) in actors {
        let name = match k.as_str() {
            Some(s) => s,
            None => continue,
        };
        if node.get("type").and_then(|x| x.as_str()) != Some("aggregate") || lifecycles.contains(name) {
            continue;
        }
        let base = name.strip_suffix("Job").unwrap_or(name);
        for candidate in [format!("{}Status", name), format!("{}Status", base)] {
            if scalar_enum_values(model, &candidate).is_some() {
                issues.push(warn(
                    "lc-missing",
                    format!("actors.yaml/{}", name),
                    format!(
                        "aggregate '{}' has a status scalar (scalars.yaml#/{}) but declares no `lifecycle` — its status machine stays implicit code (ADR-20260720-004419).",
                        name, candidate
                    ),
                ));
                break;
            }
        }
    }
    for lc in parse_lifecycles(model) {
        let at = format!("actors.yaml/{}.lifecycle", lc.aggregate);
        // status → a scalars.yaml ENUM scalar.
        let enum_values: Vec<String> = match ref_name(&lc.status_ref) {
            Some(scalar)
                if ref_target_file(&lc.status_ref, "actors.yaml").as_deref() == Some("scalars.yaml") =>
            {
                match scalar_enum_values(model, &scalar) {
                    Some(vals) => vals,
                    None => {
                        issues.push(err(
                            "lc-status",
                            format!("{}.status", at),
                            format!("'{}' is not an enum scalar — the lifecycle status must enumerate its states.", scalar),
                        ));
                        continue;
                    }
                }
            }
            _ => {
                issues.push(err(
                    "lc-status",
                    format!("{}.status", at),
                    "status must be a { $ref: 'scalars.yaml#/<EnumScalar>' }.".into(),
                ));
                continue;
            }
        };
        let state_set: BTreeSet<&str> = enum_values.iter().map(|s| s.as_str()).collect();
        let check_state = |issues: &mut Vec<Issue>, state: &str, where_: String| {
            if !state_set.contains(state) {
                issues.push(err(
                    "lc-state",
                    where_,
                    format!("'{}' is not a member of {} ({}).", state, ref_name(&lc.status_ref).unwrap_or_default(), enum_values.join(", ")),
                ));
            }
        };
        // The events THIS aggregate emits, per its receives[].emits (actors.yaml stays the wiring truth).
        let emitted: BTreeSet<String> = actors
            .get(lc.aggregate.as_str())
            .and_then(|n| n.get("receives"))
            .and_then(|r| r.as_sequence())
            .map(|seq| {
                seq.iter()
                    .flat_map(|e| ref_strings(e.get("emits")))
                    .filter_map(|r| ref_name(&r))
                    .collect()
            })
            .unwrap_or_default();
        let check_event = |issues: &mut Vec<Issue>, event_ref: &str, where_: String| -> Option<String> {
            if ref_target_file(event_ref, "actors.yaml").as_deref() != Some("events.yaml") {
                issues.push(err(
                    "lc-event",
                    where_,
                    format!("event must be a {{ $ref: 'events.yaml#/<Event>' }}, got '{}'.", event_ref),
                ));
                return None;
            }
            let name = ref_name(event_ref)?; // resolution itself is §1's job (ref-dangling)
            if !emitted.contains(&name) {
                issues.push(err(
                    "lc-event-not-emitted",
                    where_,
                    format!("event '{}' is not emitted by aggregate '{}' (per its receives[].emits) — the machine may only claim its own facts.", name, lc.aggregate),
                ));
            }
            Some(name)
        };
        // via — a dynamic target (ADR-20260721-093027): the named field must exist on the event's
        // events.yaml payload, be REQUIRED (an optional target cannot drive a machine). With no
        // `when` it must $ref the same scalar as `lifecycle.status` (the legacy `event.<via> == to`
        // form). With `when` (#639 part C step 3-ii, ADR-20260904-015903 §2) the field may $ref ANY
        // enum scalar and `when` must be one of ITS members — the row legalizes `from × {to}` when
        // `event.<via> == when`, so several rows sharing one `(from, event, via)` and differing
        // `when`/`to` values form an explicit value→target map without a second event.
        let status_scalar = ref_name(&lc.status_ref).unwrap_or_default();
        let check_via = |issues: &mut Vec<Issue>, event: &str, via: &str, when: Option<&str>, where_: String| {
            let node = model.defs.get("events.yaml").and_then(|e| e.get(event));
            let prop = node.and_then(|n| n.get("properties")).and_then(|p| p.get(via));
            match prop {
                None => issues.push(err(
                    "lc-via",
                    where_,
                    format!("via field '{}' does not exist on events.yaml#/{}'s payload.", via, event),
                )),
                Some(p) => {
                    let target = p.get("$ref").and_then(|r| r.as_str()).unwrap_or("");
                    let target_name = ref_name(target);
                    let target_is_scalar =
                        ref_target_file(target, "events.yaml").as_deref() == Some("scalars.yaml");
                    match when {
                        None => {
                            let same_scalar =
                                target_name.as_deref() == Some(status_scalar.as_str()) && target_is_scalar;
                            if !same_scalar {
                                issues.push(err(
                                    "lc-via",
                                    where_.clone(),
                                    format!("via field '{}' on events.yaml#/{} must $ref scalars.yaml#/{} (the lifecycle status scalar), or the row needs an explicit `when:` value.", via, event, status_scalar),
                                ));
                            }
                        }
                        Some(w) => {
                            if !target_is_scalar {
                                issues.push(err(
                                    "lc-via",
                                    where_.clone(),
                                    format!("via field '{}' on events.yaml#/{} must $ref a scalars.yaml enum scalar.", via, event),
                                ));
                            } else {
                                match target_name.as_deref().and_then(|s| scalar_enum_values(model, s).map(|v| (s, v))) {
                                    Some((_, vals)) if !vals.iter().any(|v| v == w) => {
                                        issues.push(err(
                                            "lc-via",
                                            where_.clone(),
                                            format!("when: '{}' is not a member of scalars.yaml#/{} ({}).", w, target_name.unwrap_or_default(), vals.join(", ")),
                                        ));
                                    }
                                    None => {
                                        issues.push(err(
                                            "lc-via",
                                            where_.clone(),
                                            format!("via field '{}' on events.yaml#/{} does not $ref an enum scalar.", via, event),
                                        ));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    let required = node
                        .and_then(|n| n.get("required"))
                        .and_then(|r| r.as_sequence())
                        .map(|s| s.iter().any(|v| v.as_str() == Some(via)))
                        .unwrap_or(false);
                    if !required {
                        issues.push(err(
                            "lc-via",
                            where_,
                            format!("via field '{}' on events.yaml#/{} must be required — an optional target cannot drive the machine.", via, event),
                        ));
                    }
                }
            }
        };
        // An event must use ONE consistent via FIELD (and static-vs-dynamic mode) across all its
        // lifecycle entries — mixing static and dynamic arms, two different fields, or a direct
        // (no-`when`) arm with a mapped (`when`) arm, is ambiguous. Distinct `when` values on the
        // SAME field are exactly the value→target map and do not count as mixing.
        let mut via_by_event: BTreeMap<String, BTreeSet<(Option<String>, bool)>> = BTreeMap::new();
        for ini in &lc.initial {
            if let Some(name) = ref_name(&ini.event_ref) {
                via_by_event.entry(name).or_default().insert((ini.via.clone(), ini.when.is_some()));
            }
        }
        for t in &lc.transitions {
            if let Some(name) = ref_name(&t.event_ref) {
                via_by_event.entry(name).or_default().insert((t.via.clone(), t.when.is_some()));
            }
        }
        for (event, vias) in &via_by_event {
            if vias.len() > 1 {
                issues.push(err(
                    "lc-ambiguous",
                    at.clone(),
                    format!("event '{}' mixes static and dynamic (`via`) entries (or two different via fields/modes) — one event, one consistent target mode.", event),
                ));
            }
        }
        // initial — at least one birth entry; unique events; states in the enum.
        if lc.initial.is_empty() {
            issues.push(err("lc-shape", format!("{}.initial", at), "lifecycle must declare at least one `initial` { event, to } entry.".into()));
        }
        let mut initial_events: BTreeSet<String> = BTreeSet::new();
        for (i, ini) in lc.initial.iter().enumerate() {
            let w = format!("{}.initial[{}]", at, i);
            check_state(issues, &ini.to, w.clone());
            if let Some(name) = check_event(issues, &ini.event_ref, w.clone()) {
                if let Some(via) = &ini.via {
                    check_via(issues, &name, via, ini.when.as_deref(), w.clone());
                }
                if !initial_events.insert(name.clone()) {
                    issues.push(err("lc-ambiguous", w, format!("duplicate initial event '{}' — the machine must be deterministic.", name)));
                }
            }
        }
        // transitions — states/events valid, deterministic: one arm per (from, event) for a static
        // target; per (from, event, to) for a dynamic one (the event INSTANCE picks the arm).
        let mut seen: BTreeSet<(String, String, String)> = BTreeSet::new();
        for (i, t) in lc.transitions.iter().enumerate() {
            let w = format!("{}.transitions[{}]", at, i);
            if t.from.is_empty() {
                issues.push(err("lc-shape", w.clone(), "a transition must declare a non-empty `from: [states]`.".into()));
            }
            check_state(issues, &t.to, w.clone());
            let ev = check_event(issues, &t.event_ref, w.clone());
            if let (Some(name), Some(via)) = (&ev, &t.via) {
                check_via(issues, name, via, t.when.as_deref(), w.clone());
            }
            for f in &t.from {
                check_state(issues, f, w.clone());
                if let Some(name) = &ev {
                    // Dynamic entries are keyed by (to, when) so several rows on the same field can
                    // legally share a `from` state (they are disambiguated by the event's carried
                    // value at fold time); a static entry keys on the empty string (unchanged).
                    let key_to = if t.via.is_some() {
                        format!("{}|{}", t.to, t.when.clone().unwrap_or_default())
                    } else {
                        String::new()
                    };
                    if !seen.insert((f.clone(), name.clone(), key_to)) {
                        issues.push(err(
                            "lc-ambiguous",
                            w.clone(),
                            format!("two transitions from '{}' on '{}' — the machine must be deterministic.", f, name),
                        ));
                    }
                }
            }
        }
        // terminal — in the enum, and with NO outgoing transition.
        for (i, s) in lc.terminal.iter().enumerate() {
            let w = format!("{}.terminal[{}]", at, i);
            check_state(issues, s, w.clone());
            if lc.transitions.iter().any(|t| t.from.iter().any(|f| f == s)) {
                issues.push(err("lc-terminal-outgoing", w, format!("terminal state '{}' has an outgoing transition.", s)));
            }
        }
        // reachability — every state the lifecycle names is reachable from an initial state.
        let mut reachable: BTreeSet<String> = lc.initial.iter().map(|i| i.to.clone()).collect();
        loop {
            let before = reachable.len();
            for t in &lc.transitions {
                if t.from.iter().any(|f| reachable.contains(f)) {
                    reachable.insert(t.to.clone());
                }
            }
            if reachable.len() == before {
                break;
            }
        }
        let mut named: BTreeSet<String> = lc.terminal.iter().cloned().collect();
        for t in &lc.transitions {
            named.extend(t.from.iter().cloned());
            named.insert(t.to.clone());
        }
        // `legacyStates:` (#639 part C step 4-i, ADR-20260904-081527 §6): a state named ONLY for a
        // retired entry — no live transition ever produces it again, but a pre-existing STORED row
        // may still carry it, so its EXIT edge(s) stay declared for legacy reads (fold-side reading
        // uses the payload's target, so no stored row breaks). Exempt from reachability by name,
        // never silently: the state must still appear in the scalar's enum (`check_state` above)
        // and the exemption is a declared list, not an absence.
        let legacy: BTreeSet<String> = lc
            .legacy_states
            .iter()
            .cloned()
            .collect();
        for s in named {
            if legacy.contains(&s) {
                continue;
            }
            if state_set.contains(s.as_str()) && !reachable.contains(&s) {
                issues.push(err(
                    "lc-unreachable",
                    at.clone(),
                    format!("state '{}' is named by the lifecycle but not reachable from an initial state.", s),
                ));
            }
        }
    }
}

