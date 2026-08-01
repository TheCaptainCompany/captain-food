use crate::*;

/// Whether a message definition ("commands.yaml#/X" / "events.yaml#/X" / "messages.yaml#/X")
/// declares `properties.<prop>`.
pub(crate) fn message_property_exists(model: &Model, message_ref: &str, prop: &str) -> bool {
    let Some((file, key)) = message_ref.split_once("#/") else { return false };
    model
        .defs
        .get(file)
        .and_then(|d| d.get(key))
        .and_then(|n| n.get("properties"))
        .and_then(|p| p.get(prop))
        .is_some()
}

/// The payload-property NAME an actor's TYPED `identity` declares —
/// `identity: { $ref: '#/<Actor>/state/<field>' }` (ADR-20260731-214500 consequences: typed $refs
/// everywhere; the bare-string form is a hard §2d error, `identity-untyped`). Returns `None` for a
/// missing identity, a bare string, or a ref of any other shape (wrong file/actor/path — §2d's
/// `identity-state-field-missing`). Generation (command addressing) reads through this helper so
/// validator and emitters can never disagree on the field.
pub(crate) fn actor_identity_field(def: &Value, actor: &str) -> Option<String> {
    let r = def.get("identity")?.get("$ref")?.as_str()?;
    let pr = parse_ref(r)?;
    if !pr.file.is_empty() && pr.file != "actors.yaml" {
        return None;
    }
    (pr.path.len() == 3 && pr.path[0] == actor && pr.path[1] == "state")
        .then(|| pr.path[2].clone())
}

/// True when `r` points at an actor's IMPLICIT identity state field: `#/<Actor>/state/<field>`
/// where `<field>` is exactly what that actor's typed `identity` declares. The identity is the
/// STREAM KEY — it exists before any fold — so it is declared by the `identity` ref itself, not by
/// an explicit `state:` entry (forcing one into every aggregate would add fold fields that change
/// the generated states.rs for no behaviour). §1 exempts these refs from `ref-dangling`; §2d's
/// `identity-state-field-missing` owns the shape proof.
pub(crate) fn is_implicit_identity_state_ref(model: &Model, r: &str, ctx: &str) -> bool {
    let Some(pr) = parse_ref(r) else { return false };
    let file = if pr.file.is_empty() { ctx } else { pr.file.as_str() };
    if file != "actors.yaml" || pr.path.len() != 3 || pr.path[1] != "state" {
        return false;
    }
    model
        .defs
        .get("actors.yaml")
        .and_then(|m| m.get(pr.path[0].as_str()))
        .and_then(|def| actor_identity_field(def, &pr.path[0]))
        .is_some_and(|f| f == pr.path[2])
}

/// §2d — the actor-mailbox ADDRESSING layer (ADR-20260730-231500, PROP-20260728-152752 §2):
/// the file-header `principals` map (role → domain-identity scalar), each aggregate's `identity`
/// (the payload property that addresses its instances = the stream id) and `mailbox.partitions`
/// (the fixed keyspace width). Enforced here:
///   - `pr-role-unknown` (error): a principals key that is not a scalars.yaml#/UserType value;
///   - `mb-partitions-range` (error): partitions outside 1..=32767 (smallint keyspace);
///   - `id-missing` (warn, adoption — like lc-missing): an aggregate with no `identity`;
///   - `identity-untyped` (error, ADR-20260731-214500 consequences): a bare-string `identity` —
///     the catalog is fully migrated to `identity: { $ref: '#/<Actor>/state/<field>' }`;
///   - `identity-state-field-missing` (error): an identity $ref that does not land on a state
///     field of the SAME actor (`#/<Actor>/state/<field>`) — the identity field is the stream key,
///     implicitly declared by this very ref (an explicit `state:` entry of the name also counts);
///   - `identity-property-not-on-command` (warn — CALIBRATED, see below): a received COMMAND whose
///     payload lacks the identity property. Warn, not error, because the current generation
///     legitimately tolerates it: `command_addressing` maps such a command to `identity_prop:
///     None` and the edge mints an ADDRESSING-ONLY actor_id (correct for a birth/side-effect
///     command whose id the server mints — today exactly `RequestPhoneVerification`). Making this
///     an error needs a DSL marker for server-minted ids first (a plan-mode decision);
///   - `id-not-in-payload` (warn, PROP-20260728-152752 §8): the same gap on a received EVENT
///     (fan-out facts may key differently — the Payment refund legs); reminder self-messages are
///     exempt (the reminder row itself carries the actor_id, so no payload key is needed).
pub(crate) fn validate_mailbox_addressing(model: &Model, issues: &mut Vec<Issue>) {
    let actors = match model.defs.get("actors.yaml") {
        Some(Value::Mapping(m)) => m,
        _ => return,
    };
    let user_types = scalar_enum_values(model, "UserType").unwrap_or_default();
    if let Some(pr) = actors.get("principals").and_then(|v| v.as_mapping()) {
        for (k, _) in pr {
            let Some(role) = k.as_str() else { continue };
            if !user_types.iter().any(|u| u == role) {
                issues.push(err(
                    "pr-role-unknown",
                    format!("actors.yaml/principals/{}", role),
                    format!("principals key '{}' is not a scalars.yaml#/UserType value.", role),
                ));
            }
        }
    }
    for (k, node) in actors {
        let name = match k.as_str() {
            Some(s) if s != "principals" => s,
            _ => continue,
        };
        // The activations sub-block is legal on ANY mailbox actor (aggregate or PM) — validate
        // it before the aggregate-only addressing checks below.
        validate_mailbox_activations(node, name, issues);
        if node.get("type").and_then(|x| x.as_str()) != Some("aggregate") {
            continue;
        }
        if let Some(p) = node.get("mailbox").and_then(|m| m.get("partitions")) {
            if !p.as_i64().map(|n| (1..=32767).contains(&n)).unwrap_or(false) {
                issues.push(err(
                    "mb-partitions-range",
                    format!("actors.yaml/{}", name),
                    format!("mailbox.partitions must be an integer in 1..=32767 (smallint keyspace width), got {:?}.", p),
                ));
            }
        }
        let Some(identity_node) = node.get("identity") else {
            issues.push(warn(
                "id-missing",
                format!("actors.yaml/{}", name),
                format!(
                    "aggregate '{}' declares no `identity` — the mailbox cannot address its instances (PROP-20260728-152752 §2).",
                    name
                ),
            ));
            continue;
        };
        if let Some(bare) = identity_node.as_str() {
            issues.push(err(
                "identity-untyped",
                format!("actors.yaml/{}", name),
                format!(
                    "identity is the bare string '{bare}' — migrate to the typed form `identity: {{ $ref: '#/{name}/state/{bare}' }}` (ADR-20260731-214500 consequences: typed $refs everywhere).",
                ),
            ));
            continue;
        }
        let Some(identity) = actor_identity_field(node, name) else {
            let raw = identity_node
                .get("$ref")
                .and_then(|r| r.as_str())
                .unwrap_or("<no $ref>");
            issues.push(err(
                "identity-state-field-missing",
                format!("actors.yaml/{}", name),
                format!(
                    "identity $ref '{raw}' does not land on a state field of '{name}' — expected `#/{name}/state/<field>` (the identity IS the actor's stream-key state field, declared by this ref; an explicit `state:` entry of that name also satisfies it).",
                ),
            ));
            continue;
        };
        for entry in node.get("receives").and_then(|r| r.as_sequence()).into_iter().flatten() {
            let Some(mref) = entry.get("message").and_then(|m| m.get("$ref")).and_then(|r| r.as_str()) else {
                continue;
            };
            // A reminder self-message needs no identity property: the reminder ROW carries the
            // actor_id (message_id = UUIDv5(actor_id, name)), so delivery is self-addressed.
            if reminder_ref_parts(mref).is_some() {
                continue;
            }
            if !message_property_exists(model, mref, &identity) {
                let is_command =
                    ref_target_file(mref, "actors.yaml").as_deref() == Some("commands.yaml");
                if is_command {
                    issues.push(warn(
                        "identity-property-not-on-command",
                        format!("actors.yaml/{}", name),
                        format!(
                            "command '{}' has no payload property '{}' — the mailbox mints an ADDRESSING-ONLY actor_id for it (legitimate only when the server mints the id; a non-birth command missing it would mis-address every delivery).",
                            mref, identity
                        ),
                    ));
                } else {
                    issues.push(warn(
                        "id-not-in-payload",
                        format!("actors.yaml/{}", name),
                        format!(
                            "'{}' does not carry identity property '{}' — a birth message minting its id, or a gap the slice-3 dispatch must resolve.",
                            mref, identity
                        ),
                    ));
                }
            }
        }
    }
}

/// The optional `mailbox.activations` sub-block (PROP-20260728-152752 §3.5, #272 D3): `false`
/// opts the actor out of the ACTOR_ACTIVATIONS-gated held-state cache; a mapping tunes it
/// (`enabled`, `idle_seconds` — the per-actor passivation override). Anything else is a shape
/// error: a knob that parses to nothing would silently run the global defaults.
pub(crate) fn validate_mailbox_activations(node: &Value, name: &str, issues: &mut Vec<Issue>) {
    let Some(act) = node.get("mailbox").and_then(|m| m.get("activations")) else {
        return;
    };
    match act {
        Value::Bool(_) => {}
        Value::Mapping(m) => {
            for (k, v) in m {
                match k.as_str() {
                    Some("enabled") => {
                        if v.as_bool().is_none() {
                            issues.push(err(
                                "mb-activations-shape",
                                format!("actors.yaml/{}", name),
                                format!("mailbox.activations.enabled must be a bool, got {:?}.", v),
                            ));
                        }
                    }
                    Some("idle_seconds") => {
                        if !v.as_i64().map(|n| n >= 1).unwrap_or(false) {
                            issues.push(err(
                                "mb-activations-shape",
                                format!("actors.yaml/{}", name),
                                format!("mailbox.activations.idle_seconds must be an integer >= 1, got {:?} (to disable the cache for this actor, use `activations: false`).", v),
                            ));
                        }
                    }
                    other => {
                        issues.push(err(
                            "mb-activations-shape",
                            format!("actors.yaml/{}", name),
                            format!("mailbox.activations knows only `enabled` and `idle_seconds`, got {:?}.", other),
                        ));
                    }
                }
            }
        }
        other => {
            issues.push(err(
                "mb-activations-shape",
                format!("actors.yaml/{}", name),
                format!("mailbox.activations must be a bool or a {{enabled, idle_seconds}} mapping, got {:?}.", other),
            ));
        }
    }
}

/// Split a lineage ref into (event name, Some(property)) — "events.yaml#/X/properties/y" — or
/// (event name, None) for a whole-event ref "events.yaml#/X".
pub(crate) fn lineage_parts(r: &str) -> Option<(&str, Option<&str>)> {
    let rest = r.strip_prefix("events.yaml#/")?;
    match rest.split_once("/properties/") {
        Some((ev, prop)) => Some((ev, Some(prop))),
        None => (!rest.contains('/')).then_some((rest, None)),
    }
}

/// The `$ref` string of an event property's type, or None (inline primitive like `type: boolean`).
pub(crate) fn event_property_type_ref(model: &Model, event: &str, prop: &str) -> Option<String> {
    let node = model.defs.get("events.yaml")?.get(event)?.get("properties")?.get(prop)?;
    node.get("$ref").and_then(|r| r.as_str()).map(str::to_string)
}

/// §2e — declared aggregate STATE + write-side `requires` (ADR-20260730-231500,
/// PROP-20260728-135632 §2): the state block is a typed, event-lineaged fold declaration; the
/// requires block is the per-instance authorization contract over it. Validation only in slice 1
/// (#242 — generation and enforcement are slice 2):
///   - `st-status-duplicated` (error): a state field named `status` next to a `lifecycle` block;
///   - `st-event-foreign` (error): a lineage event this aggregate neither emits nor receives;
///   - `st-type-mismatch` (error): a state field whose `type` $ref differs from its lineage
///     property's $ref;
///   - `st-shape` (error): mode/shape contradictions (set without `of`; flag/count lineage that
///     names properties; latest lineage that names whole events);
///   - `requires-acting-untyped` (error, ADR-20260731-214500 consequences): an acting value that
///     is a bare `state.<field>` path string (or any non-`any` bare string) — migrated to the
///     typed form `{ $ref: '#/<Actor>/state/<field>' }`; `any` stays a bare keyword;
///   - `req-state-unknown` (error): an acting $ref that is not a same-actor `#/<Actor>/state/…`
///     ref, or that names an undeclared state field;
///   - `req-principal-type` (error): the acting role's principals id scalar differs from the
///     state field's type;
///   - `req-principal-missing` (error): a non-`any` acting entry for a role absent from
///     `principals` (roles without a domain identity can only ever be `any`);
///   - `req-claim-unknown` (error): a `claims` key that is not a property of the command payload,
///     or a value other than `actor.role` / `actor.id`.
pub(crate) fn validate_actor_state(model: &Model, issues: &mut Vec<Issue>) {
    let actors = match model.defs.get("actors.yaml") {
        Some(Value::Mapping(m)) => m,
        _ => return,
    };
    // principals: role -> id scalar ref
    let mut principal_ids: BTreeMap<String, String> = BTreeMap::new();
    if let Some(pr) = actors.get("principals").and_then(|v| v.as_mapping()) {
        for (k, v) in pr {
            if let (Some(role), Some(id)) =
                (k.as_str(), v.get("id").and_then(|i| i.get("$ref")).and_then(|r| r.as_str()))
            {
                principal_ids.insert(role.to_string(), id.to_string());
            }
        }
    }
    for (k, node) in actors {
        let name = match k.as_str() {
            Some(s) if s != "principals" => s,
            _ => continue,
        };
        if node.get("type").and_then(|x| x.as_str()) != Some("aggregate") {
            continue;
        }
        let site = |suffix: String| format!("actors.yaml/{}{}", name, suffix);
        // The aggregate's event universe: everything it emits or receives.
        let mut own_events: BTreeSet<String> = BTreeSet::new();
        for entry in node.get("receives").and_then(|r| r.as_sequence()).into_iter().flatten() {
            for r in [entry.get("message")].into_iter().flatten() {
                if let Some(s) = r.get("$ref").and_then(|x| x.as_str()) {
                    if let Some(ev) = s.strip_prefix("events.yaml#/") {
                        own_events.insert(ev.to_string());
                    }
                }
            }
            for e in entry.get("emits").and_then(|x| x.as_sequence()).into_iter().flatten() {
                if let Some(s) = e.get("$ref").and_then(|x| x.as_str()) {
                    if let Some(ev) = s.strip_prefix("events.yaml#/") {
                        own_events.insert(ev.to_string());
                    }
                }
            }
        }
        // ---- state ----
        let state = node.get("state").and_then(|s| s.as_mapping());
        let mut state_types: BTreeMap<String, Option<String>> = BTreeMap::new(); // field -> type $ref
        if let Some(state) = state {
            for (fk, field) in state {
                let Some(fname) = fk.as_str() else { continue };
                let fsite = || site(format!("/state/{}", fname));
                if fname == "status" && node.get("lifecycle").is_some() {
                    issues.push(err(
                        "st-status-duplicated",
                        fsite(),
                        format!("state field 'status' on '{}' — the lifecycle block already owns it (one field, one owner).", name),
                    ));
                }
                let mode = field.get("mode").and_then(|m| m.as_str()).unwrap_or("latest");
                let type_ref = field.get("type").and_then(|t| t.get("$ref")).and_then(|r| r.as_str()).map(str::to_string);
                state_types.insert(fname.to_string(), type_ref.clone());
                if mode == "set" && field.get("of").is_none() {
                    issues.push(err("st-shape", fsite(), format!("mode `set` on '{}.{}' needs `of` (the element type).", name, fname)));
                }
                for key in ["from", "removedBy"] {
                    for r in field.get(key).and_then(|f| f.as_sequence()).into_iter().flatten() {
                        let Some(rs) = r.get("$ref").and_then(|x| x.as_str()) else { continue };
                        let Some((ev, prop)) = lineage_parts(rs) else {
                            issues.push(err("st-shape", fsite(), format!("lineage ref '{}' is not an events.yaml event or event property.", rs)));
                            continue;
                        };
                        if !own_events.contains(ev) {
                            issues.push(err(
                                "st-event-foreign",
                                fsite(),
                                format!("lineage event '{}' is neither emitted nor received by '{}'.", ev, name),
                            ));
                        }
                        match (mode, prop) {
                            ("flag", Some(_)) | ("count", Some(_)) => issues.push(err(
                                "st-shape",
                                fsite(),
                                format!("mode `{}` folds whole-event occurrences — lineage must not name a property ('{}').", mode, rs),
                            )),
                            ("latest", None) => issues.push(err(
                                "st-shape",
                                fsite(),
                                format!("mode `latest` folds a carried property — whole-event ref '{}' needs mode flag/count or a /properties/ path.", rs),
                            )),
                            ("latest", Some(p)) => {
                                if let (Some(want), Some(got)) = (type_ref.as_deref(), event_property_type_ref(model, ev, p).as_deref()) {
                                    if want != got {
                                        issues.push(err(
                                            "st-type-mismatch",
                                            fsite(),
                                            format!("'{}.{}' is typed {} but lineage property '{}/{}' is {}.", name, fname, want, ev, p, got),
                                        ));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        // ---- requires ----
        for entry in node.get("receives").and_then(|r| r.as_sequence()).into_iter().flatten() {
            let Some(req) = entry.get("requires") else { continue };
            let mref = entry.get("message").and_then(|m| m.get("$ref")).and_then(|r| r.as_str()).unwrap_or("");
            let rsite = || site(format!("/requires[{}]", mref));
            for (rk, rv) in req.get("acting").and_then(|a| a.as_mapping()).into_iter().flatten() {
                let Some(role) = rk.as_str() else { continue };
                // `any` is the only bare keyword; every binding value is a typed same-actor
                // state-field $ref (ADR-20260731-214500 consequences: typed $refs everywhere).
                let field: String = if let Some(val) = rv.as_str() {
                    if val == "any" {
                        continue;
                    }
                    let suggested = val.strip_prefix("state.").unwrap_or("<field>");
                    issues.push(err(
                        "requires-acting-untyped",
                        rsite(),
                        format!(
                            "acting.{role} is the bare string '{val}' — migrate to `{{ $ref: '#/{name}/state/{suggested}' }}` (`any` is the only bare keyword)."
                        ),
                    ));
                    continue;
                } else if let Some(r) = rv.get("$ref").and_then(|x| x.as_str()) {
                    match parse_ref(r) {
                        Some(pr)
                            if (pr.file.is_empty() || pr.file == "actors.yaml")
                                && pr.path.len() == 3
                                && pr.path[0] == name
                                && pr.path[1] == "state" =>
                        {
                            pr.path[2].clone()
                        }
                        _ => {
                            issues.push(err(
                                "req-state-unknown",
                                rsite(),
                                format!(
                                    "acting.{role} $ref '{r}' must point at a state field of the SAME actor (`#/{name}/state/<field>`)."
                                ),
                            ));
                            continue;
                        }
                    }
                } else {
                    issues.push(err(
                        "requires-acting-untyped",
                        rsite(),
                        format!(
                            "acting.{role} must be `any` or a `$ref` to `#/{name}/state/<field>`."
                        ),
                    ));
                    continue;
                };
                let field = field.as_str();
                let Some(ftype) = state_types.get(field) else {
                    issues.push(err("req-state-unknown", rsite(), format!("acting.{} references undeclared state field '{}'.", role, field)));
                    continue;
                };
                match principal_ids.get(role) {
                    None => issues.push(err(
                        "req-principal-missing",
                        rsite(),
                        format!("acting.{} compares a domain identity, but '{}' has no principals entry — roles without one can only be `any`.", role, role),
                    )),
                    Some(pid) => {
                        if ftype.as_deref() != Some(pid.as_str()) {
                            issues.push(err(
                                "req-principal-type",
                                rsite(),
                                format!("acting.{}: state.{} is typed {:?} but principals.{}.id is '{}'.", role, field, ftype, role, pid),
                            ));
                        }
                    }
                }
            }
            for (ck, cv) in req.get("claims").and_then(|c| c.as_mapping()).into_iter().flatten() {
                let (Some(prop), Some(val)) = (ck.as_str(), cv.as_str()) else { continue };
                if !matches!(val, "actor.role" | "actor.id") {
                    issues.push(err("req-claim-unknown", rsite(), format!("claims.{} must pin to `actor.role` or `actor.id`, got '{}'.", prop, val)));
                }
                if !mref.is_empty() && !message_property_exists(model, mref, prop) {
                    issues.push(err("req-claim-unknown", rsite(), format!("claims key '{}' is not a property of '{}'.", prop, mref)));
                }
            }
        }
    }
}

