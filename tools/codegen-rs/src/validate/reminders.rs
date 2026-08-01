use crate::*;

// ─── §2f — actor reminders + declarative deletion (ADR-20260731-214500) ─────────────────────────

/// A declared per-actor typed self-message (`reminders:`, ADR-20260731-120825 as renamed by
/// ADR-20260731-214500). Tolerant parsing (missing pieces → empty/None); §2f reports the holes.
pub(crate) struct ReminderDef {
    pub(crate) actor: String,
    pub(crate) name: String,
    /// `payload.$ref` — MUST target events.yaml (fact vocabulary, ADR-20260731-153000 §1a).
    pub(crate) payload_ref: String,
    /// A declared `identity:` field — always an ERROR (`reminder-identity-declared`): a reminder's
    /// identity is DERIVED, `UUIDv5(actor_id, reminder name)` (the runtime's `reminder_message_id`
    /// computes it), never declared (ADR-20260731-214500 consequences).
    pub(crate) identity_declared: bool,
    /// Optional default window: `after.$ref` into configuration.yaml keys.
    pub(crate) after_ref: Option<String>,
    /// Reschedule semantics — only `in-place` exists (ADR-20260731-150500).
    pub(crate) reschedule: Option<String>,
}

/// One `deletion.triggers[*]` entry: `on` + optional window (`after`), undo (`cancelled_on`) and
/// the strongly-typed propagation `match` (event property ↔ child state field).
pub(crate) struct DeletionTriggerDef {
    pub(crate) on: Vec<String>,
    pub(crate) after_ref: Option<String>,
    pub(crate) cancelled_on: Vec<String>,
    pub(crate) match_event_ref: Option<String>,
    pub(crate) match_state_ref: Option<String>,
    pub(crate) has_match: bool,
}

/// A parsed `deletion:` block of an actors.yaml actor (ADR-20260731-214500 §3).
pub(crate) struct DeletionDef {
    pub(crate) actor: String,
    pub(crate) triggers: Vec<DeletionTriggerDef>,
    /// `receipt.$ref` — the business fact recorded on the deletion ledger when the journey completes.
    pub(crate) receipt_ref: Option<String>,
}

/// `(actor, reminder)` when `r` points into an actors.yaml `reminders:` section — the self-message
/// form `#/<Actor>/reminders/<Name>` (or spelled `actors.yaml#/<Actor>/reminders/<Name>`).
pub(crate) fn reminder_ref_parts(r: &str) -> Option<(String, String)> {
    let pr = parse_ref(r)?;
    if !pr.file.is_empty() && pr.file != "actors.yaml" {
        return None;
    }
    if pr.path.len() == 3 && pr.path[1] == "reminders" {
        Some((pr.path[0].clone(), pr.path[2].clone()))
    } else {
        None
    }
}

/// The events.yaml event name a reminder message ref delivers (its `payload.$ref`), or None.
pub(crate) fn reminder_payload_event(model: &Model, message_ref: &str) -> Option<String> {
    let (actor, name) = reminder_ref_parts(message_ref)?;
    let payload = model
        .defs
        .get("actors.yaml")?
        .get(actor.as_str())?
        .get("reminders")?
        .get(name.as_str())?
        .get("payload")?
        .get("$ref")?
        .as_str()?;
    if ref_target_file(payload, "actors.yaml").as_deref() != Some("events.yaml") {
        return None;
    }
    ref_name(payload)
}

/// The configuration key name a window `$ref` denotes (`configuration.yaml#/keys/<KEY>`), or None
/// when the ref has any other shape.
pub(crate) fn config_key_ref_name(r: &str) -> Option<String> {
    let pr = parse_ref(r)?;
    (pr.file == "configuration.yaml" && pr.path.len() == 2 && pr.path[0] == "keys")
        .then(|| pr.path[1].clone())
}

/// Parse every actor's `reminders:` map, in actors.yaml order.
pub(crate) fn parse_reminders(model: &Model) -> Vec<ReminderDef> {
    let mut out = Vec::new();
    let Some(Value::Mapping(actors)) = model.defs.get("actors.yaml") else { return out };
    for (k, node) in actors {
        let Some(actor) = k.as_str().filter(|s| *s != "principals") else { continue };
        let Some(rem) = node.get("reminders").and_then(|r| r.as_mapping()) else { continue };
        for (rk, rv) in rem {
            let Some(name) = rk.as_str() else { continue };
            out.push(ReminderDef {
                actor: actor.to_string(),
                name: name.to_string(),
                payload_ref: rv
                    .get("payload")
                    .and_then(|p| p.get("$ref"))
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string(),
                identity_declared: rv.get("identity").is_some(),
                after_ref: rv
                    .get("after")
                    .and_then(|p| p.get("$ref"))
                    .and_then(|r| r.as_str())
                    .map(str::to_string),
                reschedule: rv.get("reschedule").and_then(|x| x.as_str()).map(str::to_string),
            });
        }
    }
    out
}

/// Parse every actor's `deletion:` block, in actors.yaml order.
pub(crate) fn parse_deletions(model: &Model) -> Vec<DeletionDef> {
    let mut out = Vec::new();
    let Some(Value::Mapping(actors)) = model.defs.get("actors.yaml") else { return out };
    for (k, node) in actors {
        let Some(actor) = k.as_str().filter(|s| *s != "principals") else { continue };
        let Some(del) = node.get("deletion") else { continue };
        let triggers = del
            .get("triggers")
            .and_then(|t| t.as_sequence())
            .map(|seq| {
                seq.iter()
                    .map(|t| {
                        let m = t.get("match");
                        DeletionTriggerDef {
                            on: ref_strings(t.get("on")),
                            after_ref: t
                                .get("after")
                                .and_then(|a| a.get("$ref"))
                                .and_then(|r| r.as_str())
                                .map(str::to_string),
                            cancelled_on: ref_strings(t.get("cancelled_on")),
                            match_event_ref: m
                                .and_then(|x| x.get("event"))
                                .and_then(|e| e.get("$ref"))
                                .and_then(|r| r.as_str())
                                .map(str::to_string),
                            match_state_ref: m
                                .and_then(|x| x.get("state"))
                                .and_then(|s| s.get("$ref"))
                                .and_then(|r| r.as_str())
                                .map(str::to_string),
                            has_match: m.is_some(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(DeletionDef {
            actor: actor.to_string(),
            triggers,
            receipt_ref: del
                .get("receipt")
                .and_then(|r| r.get("$ref"))
                .and_then(|r| r.as_str())
                .map(str::to_string),
        });
    }
    out
}

/// §2f — the `reminders:` / `schedules:` / `deletion:` DSL (ADR-20260731-214500, refining
/// ADR-20260731-120825/-150500/-153000). Each rule maps to a way the declarations could silently
/// stop being trustworthy:
///   - `reminder-payload-not-event` (error): a reminder payload that is not an events.yaml FACT —
///     record semantics only, never a command (ADR-20260731-153000 §1a);
///   - `reminder-after-unresolved` (error): a reminder window that is not a resolving
///     `configuration.yaml#/keys/<KEY>` ref (never a bare string);
///   - `reminder-reschedule-unknown` (error): a `reschedule` value other than `in-place` — the only
///     semantics that exist (ADR-20260731-150500);
///   - `reminder-identity-declared` (error): a reminder declaring `identity:` — it is DERIVED
///     (`UUIDv5(actor_id, reminder name)`, the runtime's `reminder_message_id`), never declared
///     (ADR-20260731-214500 consequences);
///   - `reminder-without-receive` (error): a declared reminder the SAME actor does not receive —
///     strong typing means handler-proof, per actor (ADR-20260731-120825 §2);
///   - `receive-without-reminder` (error): a receives message ref into a `reminders:` section that
///     does not resolve to a reminder declared on that same actor;
///   - `schedules-unresolved` (error): a `schedules:` entry that is not a resolving SAME-actor
///     reminder ref (scheduling is the handler's third observable effect);
///   - `deletion-ref-unresolved` (error): a deletion `on`/`cancelled_on`/`receipt` that is not a
///     resolving events.yaml event, an `after` that is not a resolving configuration key, an empty
///     `on`/`triggers` list, or a missing `receipt`;
///   - `deletion-match-untyped` (error): a propagation trigger (no `after`) without a `match`, or a
///     `match` whose `event` is not a property of a triggering `on` event / whose `state` is not a
///     declared state field of THIS actor — bare string paths are barred (ADR-20260731-214500 §3);
///   - `deletion-tree-cycle` (error): the propagation graph (edge A → B when B's `on` lists A's
///     `receipt` fact) must be acyclic — the dependency tree EMERGES from child declarations, and a
///     cycle would make the generic engine chase its own tail.
pub(crate) fn validate_reminders_and_deletion(model: &Model, issues: &mut Vec<Issue>) {
    let reminders = parse_reminders(model);
    let deletions = parse_deletions(model);
    let declared: BTreeSet<(String, String)> =
        reminders.iter().map(|r| (r.actor.clone(), r.name.clone())).collect();

    // ---- reminders: payload / window / reschedule hygiene ----
    for r in &reminders {
        let at = format!("actors.yaml/{}/reminders/{}", r.actor, r.name);
        let payload_ok = ref_target_file(&r.payload_ref, "actors.yaml").as_deref()
            == Some("events.yaml")
            && resolve_ref(model, &r.payload_ref, "actors.yaml").is_some();
        if !payload_ok {
            issues.push(err(
                "reminder-payload-not-event",
                at.clone(),
                format!(
                    "reminder payload must be a resolving events.yaml FACT (record semantics, ADR-20260731-153000), got '{}'.",
                    r.payload_ref
                ),
            ));
        }
        if let Some(a) = &r.after_ref {
            let resolves = config_key_ref_name(a)
                .map(|_| resolve_ref(model, a, "actors.yaml").is_some())
                .unwrap_or(false);
            if !resolves {
                issues.push(err(
                    "reminder-after-unresolved",
                    at.clone(),
                    format!("`after` must be a resolving configuration.yaml#/keys/<KEY> ref, got '{}'.", a),
                ));
            }
        }
        if let Some(rs) = &r.reschedule {
            if rs != "in-place" {
                issues.push(err(
                    "reminder-reschedule-unknown",
                    at.clone(),
                    format!("reschedule '{}' — only `in-place` exists (ADR-20260731-150500).", rs),
                ));
            }
        }
        if r.identity_declared {
            issues.push(err(
                "reminder-identity-declared",
                at.clone(),
                "reminder `identity` is DERIVED — UUIDv5(actor_id, reminder name), computed by the runtime's reminder_message_id — never declared; remove the field (ADR-20260731-214500 consequences).".into(),
            ));
        }
    }

    // ---- receives ↔ reminders (both ways) + schedules, per actors.yaml actor ----
    let actors = match model.defs.get("actors.yaml") {
        Some(Value::Mapping(m)) => m,
        _ => return,
    };
    // (actor, reminder) pairs some receives entry handles — for `reminder-without-receive`.
    let mut received: BTreeSet<(String, String)> = BTreeSet::new();
    for (k, node) in actors {
        let Some(actor) = k.as_str().filter(|s| *s != "principals") else { continue };
        for (i, entry) in
            node.get("receives").and_then(|r| r.as_sequence()).into_iter().flatten().enumerate()
        {
            let at = format!("actors.yaml/{}.receives[{}]", actor, i);
            if let Some(mref) =
                entry.get("message").and_then(|m| m.get("$ref")).and_then(|r| r.as_str())
            {
                if let Some((ra, rn)) = reminder_ref_parts(mref) {
                    if ra != actor || !declared.contains(&(ra.clone(), rn.clone())) {
                        issues.push(err(
                            "receive-without-reminder",
                            format!("{}.message", at),
                            format!(
                                "message '{}' does not resolve to a reminder declared on '{}' — a reminder is one actor talking to ITSELF (ADR-20260731-120825).",
                                mref, actor
                            ),
                        ));
                    } else {
                        received.insert((ra, rn));
                    }
                }
            }
            for (j, sref) in ref_strings(entry.get("schedules")).into_iter().enumerate() {
                let ok = reminder_ref_parts(&sref)
                    .map(|(ra, rn)| ra == actor && declared.contains(&(ra.clone(), rn)))
                    .unwrap_or(false);
                if !ok {
                    issues.push(err(
                        "schedules-unresolved",
                        format!("{}.schedules[{}]", at, j),
                        format!(
                            "'{}' does not resolve to a reminder declared on '{}' (expected #/{}/reminders/<Name>).",
                            sref, actor, actor
                        ),
                    ));
                }
            }
        }
    }
    for r in &reminders {
        if !received.contains(&(r.actor.clone(), r.name.clone())) {
            issues.push(err(
                "reminder-without-receive",
                format!("actors.yaml/{}/reminders/{}", r.actor, r.name),
                format!(
                    "no receives entry on '{}' handles this reminder — add {{ message: {{ $ref: '#/{}/reminders/{}' }} }} (strong typing means handler-proof, ADR-20260731-120825).",
                    r.actor, r.actor, r.name
                ),
            ));
        }
    }

    // ---- deletion blocks ----
    for d in &deletions {
        let at = format!("actors.yaml/{}/deletion", d.actor);
        // The actor's declared state fields — what a `match.state` may bind to. The typed
        // `identity` ref IMPLICITLY declares its field (the stream key exists before any fold,
        // same doctrine as `is_implicit_identity_state_ref`), so a self-trigger may match on it
        // without forcing an explicit `state:` entry into the aggregate.
        let mut state_fields: BTreeSet<String> = actors
            .get(d.actor.as_str())
            .and_then(|n| n.get("state"))
            .and_then(|s| s.as_mapping())
            .map(|m| m.keys().filter_map(|k| k.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        if let Some(f) =
            actors.get(d.actor.as_str()).and_then(|n| actor_identity_field(n, &d.actor))
        {
            state_fields.insert(f);
        }
        match &d.receipt_ref {
            None => issues.push(err(
                "deletion-ref-unresolved",
                at.clone(),
                "deletion declares no `receipt` — the completing fact recorded on the deletion ledger (ADR-20260731-160000 §6).".into(),
            )),
            Some(r) => {
                let ok = ref_target_file(r, "actors.yaml").as_deref() == Some("events.yaml")
                    && resolve_ref(model, r, "actors.yaml").is_some();
                if !ok {
                    issues.push(err(
                        "deletion-ref-unresolved",
                        format!("{}.receipt", at),
                        format!("receipt must be a resolving events.yaml event, got '{}'.", r),
                    ));
                }
            }
        }
        if d.triggers.is_empty() {
            issues.push(err(
                "deletion-ref-unresolved",
                at.clone(),
                "deletion declares no `triggers` — nothing would ever start the journey.".into(),
            ));
        }
        for (i, t) in d.triggers.iter().enumerate() {
            let tat = format!("{}.triggers[{}]", at, i);
            if t.on.is_empty() {
                issues.push(err(
                    "deletion-ref-unresolved",
                    tat.clone(),
                    "trigger declares no `on` events — it can never fire.".into(),
                ));
            }
            let mut on_events: BTreeSet<String> = BTreeSet::new();
            for (key, refs) in [("on", &t.on), ("cancelled_on", &t.cancelled_on)] {
                for (j, r) in refs.iter().enumerate() {
                    let ok = ref_target_file(r, "actors.yaml").as_deref() == Some("events.yaml")
                        && resolve_ref(model, r, "actors.yaml").is_some();
                    if !ok {
                        issues.push(err(
                            "deletion-ref-unresolved",
                            format!("{}.{}[{}]", tat, key, j),
                            format!("'{}' is not a resolving events.yaml event.", r),
                        ));
                    } else if key == "on" {
                        if let Some(n) = ref_name(r) {
                            on_events.insert(n);
                        }
                    }
                }
            }
            if let Some(a) = &t.after_ref {
                let resolves = config_key_ref_name(a)
                    .map(|_| resolve_ref(model, a, "actors.yaml").is_some())
                    .unwrap_or(false);
                if !resolves {
                    issues.push(err(
                        "deletion-ref-unresolved",
                        format!("{}.after", tat),
                        format!("`after` must be a resolving configuration.yaml#/keys/<KEY> ref, got '{}'.", a),
                    ));
                }
            }
            // `match` — required for propagation (no `after`), allowed with one; always typed.
            if !t.has_match {
                if t.after_ref.is_none() {
                    issues.push(err(
                        "deletion-match-untyped",
                        tat.clone(),
                        "propagation trigger (no `after`) needs a typed `match` — the engine enumerates child instances by it (ADR-20260731-214500 §3).".into(),
                    ));
                }
                continue;
            }
            match &t.match_event_ref {
                None => issues.push(err(
                    "deletion-match-untyped",
                    format!("{}.match", tat),
                    "match declares no `event` — a $ref to the triggering event's property is required.".into(),
                )),
                Some(r) => {
                    let parts = lineage_parts(r);
                    match parts {
                        Some((ev, Some(_))) if resolve_ref(model, r, "actors.yaml").is_some() => {
                            if !on_events.contains(ev) {
                                issues.push(err(
                                    "deletion-match-untyped",
                                    format!("{}.match.event", tat),
                                    format!("match.event names '{}', which is not one of this trigger's `on` events.", ev),
                                ));
                            }
                        }
                        _ => issues.push(err(
                            "deletion-match-untyped",
                            format!("{}.match.event", tat),
                            format!("'{}' is not a resolving events.yaml#/<Event>/properties/<prop> ref.", r),
                        )),
                    }
                }
            }
            match &t.match_state_ref {
                None => issues.push(err(
                    "deletion-match-untyped",
                    format!("{}.match", tat),
                    "match declares no `state` — a $ref to this actor's state field is required.".into(),
                )),
                Some(r) => {
                    let ok = parse_ref(r)
                        .filter(|pr| pr.file.is_empty() || pr.file == "actors.yaml")
                        .filter(|pr| {
                            pr.path.len() == 3
                                && pr.path[0] == d.actor
                                && pr.path[1] == "state"
                                && state_fields.contains(&pr.path[2])
                        })
                        .is_some();
                    if !ok {
                        issues.push(err(
                            "deletion-match-untyped",
                            format!("{}.match.state", tat),
                            format!(
                                "'{}' is not a declared state field of '{}' (expected #/{}/state/<field>).",
                                r, d.actor, d.actor
                            ),
                        ));
                    }
                }
            }
        }
    }

    // ---- deletion propagation tree: acyclic ----
    // Edge A → B when B's `on` lists A's `receipt` fact (the child declares how it dies).
    let receipt_of: BTreeMap<&str, String> = deletions
        .iter()
        .filter_map(|d| d.receipt_ref.as_deref().and_then(ref_name).map(|e| (d.actor.as_str(), e)))
        .collect();
    let mut edges: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for d in &deletions {
        let on: BTreeSet<String> =
            d.triggers.iter().flat_map(|t| t.on.iter()).filter_map(|r| ref_name(r)).collect();
        for (parent, receipt) in &receipt_of {
            if on.contains(receipt) {
                edges.entry(parent).or_default().push(d.actor.as_str());
            }
        }
    }
    // DFS with an explicit path; each cycle is reported once, keyed by its sorted member set.
    let mut reported: BTreeSet<Vec<String>> = BTreeSet::new();
    fn dfs<'a>(
        node: &'a str,
        edges: &BTreeMap<&'a str, Vec<&'a str>>,
        path: &mut Vec<&'a str>,
        done: &mut BTreeSet<&'a str>,
        reported: &mut BTreeSet<Vec<String>>,
        issues: &mut Vec<Issue>,
    ) {
        if let Some(pos) = path.iter().position(|n| *n == node) {
            let cycle: Vec<&str> = path[pos..].to_vec();
            let mut key: Vec<String> = cycle.iter().map(|s| s.to_string()).collect();
            key.sort();
            if reported.insert(key) {
                issues.push(err(
                    "deletion-tree-cycle",
                    format!("actors.yaml/{}/deletion", cycle[0]),
                    format!(
                        "deletion propagation cycle: {} -> {} — the emergent dependency tree must be acyclic (ADR-20260731-214500).",
                        cycle.join(" -> "),
                        node
                    ),
                ));
            }
            return;
        }
        if done.contains(node) {
            return;
        }
        path.push(node);
        for next in edges.get(node).cloned().unwrap_or_default() {
            dfs(next, edges, path, done, reported, issues);
        }
        path.pop();
        done.insert(node);
    }
    let mut done: BTreeSet<&str> = BTreeSet::new();
    let roots: Vec<&str> = edges.keys().copied().collect();
    for root in roots {
        let mut path = Vec::new();
        dfs(root, &edges, &mut path, &mut done, &mut reported, issues);
    }
}

