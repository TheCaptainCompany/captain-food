use crate::*;

/// A resolver's pinned static `args:` (#82) — the ONLY place a screens surface names an api.yaml
/// query ARGUMENT by hand. §1 proves the resolver's `query.$ref` RESOLVES and §1b proves it resolves
/// to a QUERY, but neither looks inside `args:`, so a typo in a pinned key (`listKey` where the query
/// declares `list`) stayed invisible until a client actually issued the query — the server then
/// rejects the whole operation on an unknown input field. Fail closed here instead:
///
/// - `resolver-unknown-arg` — the pinned key is not an argument of the bound query;
/// - `resolver-invalid-arg-value` — the argument IS declared and enum-typed, but the pinned literal
///   is not one of its members (mirrors the `test-invalid-enum-value` check of `check_shape`, done
///   inline here because `check_shape` resolves refs against `tests.yaml`, not `api.yaml`).
///
/// NOT checked: that every REQUIRED arg is pinned. A pin is a static DEFAULT — the remaining args are
/// supplied by the caller at runtime (`crates/web/src/graphql.rs#execute_resolver` merges caller
/// variables OVER the pins), so an unpinned required arg is normal, not an error.

pub(crate) fn validate_resolver_args(model: &Model, issues: &mut Vec<Issue>, at: &str, query: &str, args: &Value) {
    let Some(pinned) = args.as_mapping() else { return };
    let empty = serde_yaml::Mapping::new();
    let declared = model
        .defs
        .get("api.yaml")
        .and_then(|v| v.get("queries"))
        .and_then(|v| v.get(query))
        .and_then(|v| v.get("args"))
        .and_then(|v| v.as_mapping())
        .unwrap_or(&empty);
    let names: Vec<&str> = declared.keys().filter_map(|k| k.as_str()).collect();

    for (ak, av) in pinned {
        let Some(name) = ak.as_str() else { continue };
        let Some(node) = declared.get(Value::String(name.to_string())) else {
            issues.push(err(
                "resolver-unknown-arg",
                at.into(),
                format!(
                    "pinned arg '{}' is not an argument of api.yaml query '{}' ({}).",
                    name,
                    query,
                    if names.is_empty() {
                        "it declares none".to_string()
                    } else {
                        format!("declared: {}", names.join("|"))
                    }
                ),
            ));
            continue;
        };
        // Enum-typed arg: the pinned literal (or every item of an `array: true` pin) must be a member.
        let Some(rf) = node.get("$ref").and_then(|x| x.as_str()) else { continue };
        let Some(target) = resolve_ref(model, rf, "api.yaml") else { continue };
        let Some(vals) = target.get("enum").and_then(|e| e.as_sequence()) else { continue };
        let pins: Vec<&Value> = match av.as_sequence() {
            Some(seq) => seq.iter().collect(),
            None => vec![av],
        };
        for p in pins {
            let Some(lit) = p.as_str() else { continue };
            if !vals.iter().any(|v| v.as_str() == Some(lit)) {
                issues.push(err(
                    "resolver-invalid-arg-value",
                    at.into(),
                    format!(
                        "pinned arg '{}' = '{}' is not a value of enum {} ({}).",
                        name,
                        lit,
                        rf,
                        vals.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join("|")
                    ),
                ));
            }
        }
    }
}

/// Every `action: { type, variables }` block in a screen's component tree, flattened. Components nest
/// under `content` / `components` / `children` and an action can sit on any node, so this walks the
/// whole subtree rather than a fixed shape.
fn collect_screen_actions(node: &Value, out: &mut Vec<(String, BTreeSet<String>)>) {
    match node {
        Value::Sequence(seq) => {
            for n in seq {
                collect_screen_actions(n, out);
            }
        }
        Value::Mapping(map) => {
            if let Some(a) = map.get(Value::String("action".to_string())) {
                if let Some(t) = a.get("type").and_then(|x| x.as_str()) {
                    let vars = a
                        .get("variables")
                        .and_then(|v| v.as_mapping())
                        .map(|m| m.keys().filter_map(|k| k.as_str()).map(str::to_string).collect())
                        .unwrap_or_default();
                    out.push((t.to_string(), vars));
                }
            }
            for (_, v) in map {
                collect_screen_actions(v, out);
            }
        }
        _ => {}
    }
}

/// The full validator — a faithful port of validate.ts §1–§11. Returns issues + coverage.
pub(crate) fn validate(model: &Model) -> Report {
    let mut issues: Vec<Issue> = Vec::new();
    let mut cov = Coverage::default();

    // --- 0. Load-time issues (per-scope fragment merge, ADR-20260807-183024 D1): duplicate item
    // names across files mapping to one logical catalog gate here like any other error.
    issues.extend(model.load_issues.iter().cloned());

    // --- 1. Referential integrity: every `$ref` anywhere must resolve ---------------------------
    // Iterate every loaded file (incl. globbed database/tables/*.yaml), not just the fixed SOURCE_FILES.
    for (f, v) in &model.defs {
        {
            let f = f.as_str();
            let mut refs = Vec::new();
            collect_refs(v, f, &mut refs);
            for (loc, r) in refs {
                cov.refs += 1;
                if parse_ref(&r).is_none() {
                    issues.push(err("ref-format", loc, format!("Malformed $ref '{}'.", r)));
                } else if resolve_ref(model, &r, f).is_none()
                    && !is_implicit_identity_state_ref(model, &r, f)
                {
                    // The identity state field is implicitly declared by the actor's own typed
                    // `identity` ref (the stream key needs no fold entry) — see
                    // `is_implicit_identity_state_ref`; §2d proves the declaration's shape.
                    issues.push(err("ref-dangling", loc, format!("$ref '{}' does not resolve.", r)));
                }
            }
        }
    }

    // --- 1b. Ref-KIND contract: a resolving $ref must also point at the right KIND of thing -------
    validate_ref_kinds(model, &mut issues);

    // --- 1c. Configuration hygiene (PROP-20260729-004500) ----------------------------------------
    validate_configuration(model, &mut issues);

    let actors = parse_actors(model);
    let api = parse_api(model);

    // --- 2. Actor wiring: messages, emits and throws must target the right kind of file ---------
    let mut handled_commands: BTreeSet<String> = BTreeSet::new();
    let mut emitted_events: BTreeSet<String> = BTreeSet::new();
    let mut consumed_events: BTreeSet<String> = BTreeSet::new();
    for actor in &actors {
        for (i, entry) in actor.receives.iter().enumerate() {
            let where_ = format!("{}/{}.receives[{}]", actor.file, actor.name, i);
            if entry.message_ref.is_empty() {
                issues.push(err("actor-message", where_.clone(), "receives entry has no message $ref.".into()));
            } else if ref_target_file(&entry.message_ref, "actors.yaml").as_deref() == Some("commands.yaml") {
                if let Some(n) = ref_name(&entry.message_ref) {
                    handled_commands.insert(n);
                }
            } else if ref_target_file(&entry.message_ref, "actors.yaml").as_deref() == Some("events.yaml") {
                if let Some(n) = ref_name(&entry.message_ref) {
                    consumed_events.insert(n);
                }
            } else if reminder_ref_parts(&entry.message_ref).is_some() {
                // A reminder self-message (ADR-20260731-214500): §2f proves it resolves on the SAME
                // actor. Its payload FACT counts as consumed — the delivery records it (record
                // semantics, ADR-20260731-153000) — so the event is not an orphan.
                if let Some(ev) = reminder_payload_event(model, &entry.message_ref) {
                    consumed_events.insert(ev);
                }
            } else {
                issues.push(err(
                    "actor-message",
                    format!("{}.message", where_),
                    format!("message must reference commands.yaml or events.yaml, got '{}'.", entry.message_ref),
                ));
            }
            for (j, e) in entry.emits.iter().enumerate() {
                if ref_target_file(e, "actors.yaml").as_deref() != Some("events.yaml") {
                    issues.push(err(
                        "actor-emits",
                        format!("{}.emits[{}]", where_, j),
                        format!("emits must reference events.yaml, got '{}'.", e),
                    ));
                } else if let Some(n) = ref_name(e) {
                    emitted_events.insert(n);
                }
            }
            for (j, t) in entry.throws.iter().enumerate() {
                if ref_target_file(t, "actors.yaml").as_deref() != Some("errors.yaml") {
                    issues.push(err(
                        "actor-throws",
                        format!("{}.throws[{}]", where_, j),
                        format!("throws must reference errors.yaml, got '{}'.", t),
                    ));
                }
            }
        }
    }

    // --- 2b. Process managers (processmanager.yaml): typed-step validation -----------------------
    validate_process_managers(model, &mut issues);

    // --- 2c. Aggregate lifecycle state machines (actors.yaml `lifecycle`, ADR-20260720-004419) ---
    validate_lifecycles(model, &mut issues);
    validate_mailbox_addressing(model, &mut issues);
    validate_actor_state(model, &mut issues);
    // --- 2f. Reminders + declarative deletion (ADR-20260731-214500) ------------------------------
    validate_reminders_and_deletion(model, &mut issues);
    {
        let lcs = parse_lifecycles(model);
        cov.lifecycles = lcs.len();
        cov.lifecycle_transitions =
            lcs.iter().map(|l| l.transitions.iter().map(|t| t.from.len()).sum::<usize>()).sum();
    }

    // --- 2d. Service catalog (services.yaml, ADR-20260719-214500) --------------------------------
    validate_services(model, &mut issues);

    // --- §18. Database catalog + per-kind placement (#494 slice 1, STO-1(a)/STO-2(a)/ADP-1) ------
    validate_databases(model, &mut issues);

    // --- 3. Coverage: derive value-objects vs commands, and orphan events ------------------------
    let mut refd_from_properties: BTreeSet<String> = BTreeSet::new();
    for (f, v) in &model.defs {
        {
            let f = f.as_str();
            let mut refs = Vec::new();
            collect_refs(v, f, &mut refs);
            for (loc, r) in refs {
                if ref_target_file(&r, f).as_deref() == Some("commands.yaml") && loc.contains(".properties.") {
                    if let Some(n) = ref_name(&r) {
                        refd_from_properties.insert(n);
                    }
                }
            }
        }
    }
    for c in map_keys(model.defs.get("commands.yaml")) {
        if handled_commands.contains(&c) {
            continue;
        }
        if !refd_from_properties.contains(&c) {
            issues.push(warn(
                "command-unhandled",
                format!("commands.yaml/{}", c),
                format!("Command '{}' is defined but no actor handles it.", c),
            ));
        }
    }
    let mut produced_events: BTreeSet<String> = emitted_events.clone();
    produced_events.extend(consumed_events.iter().cloned());
    // Declarative deletion (ADR-20260731-214500): the generic engine RECORDS each declared
    // `receipt` fact (on the deletion ledger) and CONSUMES each trigger/undo fact — a fact that
    // exists only in a `deletion:` block is engine vocabulary, not an orphan.
    for d in parse_deletions(model) {
        if let Some(e) = d.receipt_ref.as_deref().and_then(ref_name) {
            produced_events.insert(e);
        }
        for t in &d.triggers {
            for r in t.on.iter().chain(t.cancelled_on.iter()) {
                if let Some(e) = ref_name(r) {
                    produced_events.insert(e);
                }
            }
        }
    }
    for e in map_keys(model.defs.get("events.yaml")) {
        if !produced_events.contains(&e) {
            issues.push(warn(
                "event-orphan",
                format!("events.yaml/{}", e),
                format!("Event '{}' is never emitted nor consumed by any actor.", e),
            ));
        }
    }

    // --- 4. API surface (api.yaml ↔ model) ------------------------------------------------------
    let user_type_set: BTreeSet<String> = model
        .defs
        .get("scalars.yaml")
        .and_then(|s| s.get("UserType"))
        .and_then(|u| u.get("enum"))
        .and_then(|e| e.as_sequence())
        .map(|s| s.iter().filter_map(|v| v.as_str().map(|x| x.to_string())).collect())
        .unwrap_or_default();
    let all_commands: BTreeSet<String> = map_keys(model.defs.get("commands.yaml")).into_iter().collect();

    // 4a. mutations
    let mut declared_by_command: BTreeMap<String, String> = BTreeMap::new();
    for m in &api.mutations {
        let where_ = format!("api.yaml/mutations.{}", m.name);
        check_roles(&mut issues, &m.roles, &where_, &user_type_set);
        if m.command.is_empty() {
            issues.push(err("op-missing-command", where_.clone(), "mutation declares no command.".into()));
        } else if !all_commands.contains(&m.command) {
            issues.push(err(
                "mutation-unknown-command",
                where_.clone(),
                format!("command '{}' is not defined in commands.yaml.", m.command),
            ));
        } else if !handled_commands.contains(&m.command) {
            issues.push(warn(
                "mutation-command-unhandled",
                where_.clone(),
                format!("command '{}' has no actor handler.", m.command),
            ));
        }
        if !m.command.is_empty() {
            if let Some(prev) = declared_by_command.get(&m.command) {
                issues.push(err(
                    "command-duplicate-mutation",
                    where_.clone(),
                    format!("command '{}' is already dispatched by mutation '{}'.", m.command, prev),
                ));
            } else {
                declared_by_command.insert(m.command.clone(), m.name.clone());
            }
        }
        // Acceptance-first (ADR-20260720-015500): a mutation declares NO per-operation payload —
        // business outcomes are reads. The uniform MutationAcceptance is the only mutation payload.
        if !m.payload.is_empty() {
            issues.push(err(
                "mutation-payload-forbidden",
                where_.clone(),
                format!(
                    "mutation '{}' declares a payload — acceptance-first mutations return only \
                     MutationAcceptance; expose business results as a query/subscription (ADR-20260720-015500).",
                    m.name
                ),
            ));
        }
    }
    cov.mutation_links = declared_by_command.len();
    // 4a'. the acceptance-first surface both emitters depend on must exist in the spec.
    if !api.types.iter().any(|t| t.name == "MutationAcceptance") {
        issues.push(err(
            "acceptance-type-missing",
            "api.yaml/types".into(),
            "acceptance-first mutations require the shared #/types/MutationAcceptance (ADR-20260720-015500).".into(),
        ));
    }
    if !api.inputs.iter().any(|(n, _)| n == "MetadataInput") {
        issues.push(err(
            "metadata-input-missing",
            "api.yaml/inputs".into(),
            "acceptance-first mutations require #/inputs/MetadataInput (ADR-20260720-015500).".into(),
        ));
    } else if let Some((_, fields)) = api.inputs.iter().find(|(n, _)| n == "MetadataInput") {
        for f in fields {
            check_inline(&mut issues, f, &format!("api.yaml/inputs.MetadataInput.{}", f.name));
        }
    }
    // 4b. every handled command must be dispatched by exactly one mutation.
    for cmd in &handled_commands {
        if !declared_by_command.contains_key(cmd) {
            issues.push(warn(
                "command-no-mutation",
                format!("commands.yaml/{}", cmd),
                format!("Handled command '{}' is not dispatched by any mutation.", cmd),
            ));
        }
    }

    // 4c. queries
    let mut output_types: BTreeSet<String> = map_keys(model.defs.get("entities.yaml")).into_iter().collect();
    for t in &api.types {
        output_types.insert(t.name.clone());
    }
    // A TRANSIENT type — one a query returns that has no read model behind it — must DECLARE the
    // write-path table it is served from (`readsInfrastructure:`), not certify itself by leaving
    // `reads:` off (ADR-20260812-214500). Transience-by-omission was the real leak: the
    // `command_journal` resolvers declared no `reads:` at all, so no reads-side rule ever looked at
    // them, and retiring the table cost 110 files. `readsInfrastructure` is a $ref, so the table is
    // now a REFERENCE the validator resolves — the next retirement is a grep the loader does for you.
    let transient_types: BTreeSet<String> = api
        .types
        .iter()
        .filter(|t| t.reads.is_empty() && !t.reads_infrastructure.is_empty())
        .map(|t| t.name.clone())
        .collect();
    for q in &api.queries {
        let where_ = format!("api.yaml/queries.{}", q.name);
        check_roles(&mut issues, &q.roles, &where_, &user_type_set);
        if q.reads.is_empty() && !transient_types.contains(&q.returns_type) {
            issues.push(err(
                "op-missing-reads",
                where_.clone(),
                format!(
                    "return type '{}' declares no `reads` binding (→ @reads); bind it to a View_* in api.yaml types.",
                    if q.returns_type.is_empty() { "?" } else { &q.returns_type }
                ),
            ));
        }
        if q.returns_type.is_empty() {
            issues.push(err("query-no-returns", where_.clone(), "query has no return type.".into()));
        } else if !output_types.contains(&q.returns_type) {
            issues.push(err(
                "query-unknown-type",
                where_.clone(),
                format!("return type '{}' is neither an entities.yaml type nor an api projection.", q.returns_type),
            ));
        }
        for a in &q.args {
            check_inline(&mut issues, a, &format!("{}.args.{}", where_, a.name));
        }
    }

    // 4d. subscriptions
    for s in &api.subscriptions {
        let where_ = format!("api.yaml/subscriptions.{}", s.name);
        check_roles(&mut issues, &s.roles, &where_, &user_type_set);
        if s.returns_type.is_empty() {
            issues.push(err("subscription-no-returns", where_.clone(), "subscription has no return type.".into()));
        } else if !output_types.contains(&s.returns_type) {
            issues.push(err(
                "subscription-unknown-type",
                where_.clone(),
                format!("return type '{}' is neither an entities.yaml type nor an api projection.", s.returns_type),
            ));
        }
        for a in &s.args {
            check_inline(&mut issues, a, &format!("{}.args.{}", where_, a.name));
        }
    }

    // --- 5. Read models (views.yaml) ------------------------------------------------------------
    let sql_primitives: BTreeSet<&str> =
        ["uuid", "text", "integer", "bigint", "boolean", "timestamptz", "jsonb", "numeric"].into_iter().collect();
    let scalar_names: BTreeSet<String> = map_keys(model.defs.get("scalars.yaml")).into_iter().collect();
    let aggregate_names: BTreeSet<String> =
        actors.iter().filter(|a| a.kind == "aggregate").map(|a| a.name.clone()).collect();
    let views = parse_views(model);

    cov.views = views.len();
    for view in &views {
        let at = format!("views.yaml/{}", view.name);
        cov.view_columns += view.columns.len();
        cov.view_fed_by += view.fedby.len();
        // Naming convention (ADR-0039): a generated VIEW is `View_*`; a materialized TABLE has no prefix.
        if !view.is_table && !view.name.starts_with("View_") {
            issues.push(warn("view-naming", at.clone(), format!("Fold view '{}' should be prefixed 'View_'.", view.name)));
        }
        if view.is_table && view.name.starts_with("View_") {
            issues.push(warn("view-naming", at.clone(), format!("Materialized table '{}' should NOT be prefixed 'View_'.", view.name)));
        }
        if !view.reference && !aggregate_names.contains(&view.aggregate) {
            issues.push(err(
                "view-unknown-aggregate",
                at.clone(),
                format!("aggregate '{}' is not an aggregate in actors.yaml.", view.aggregate),
            ));
        }
        if view.columns.is_empty() {
            issues.push(err("view-no-columns", at.clone(), "view has no columns.".into()));
        }

        let col_names: BTreeSet<&str> = view.columns.iter().map(|c| c.name.as_str()).collect();
        let fed_by_names: BTreeSet<&str> = view.fedby.iter().map(|s| s.as_str()).collect();
        let mut used_events: BTreeSet<String> = BTreeSet::new();
        let mut pk_count = 0;
        for col in &view.columns {
            if col.pk {
                pk_count += 1;
            }
            if col.ty.is_empty() {
                issues.push(err(
                    "view-column-no-type",
                    format!("{}.{}", at, col.name),
                    "column has no `type` and none could be derived from `from` (declare a type or map it to a typed event property).".into(),
                ));
            } else if !sql_primitives.contains(col.ty.as_str()) && !scalar_names.contains(&col.ty) {
                issues.push(err(
                    "view-column-type",
                    format!("{}.{}", at, col.name),
                    format!("type '{}' is neither a SQL primitive nor a scalars.yaml type.", col.ty),
                ));
            }
            // created_at/updated_at are IMPLICIT technical columns (stamped from event.occurred_at,
            // ADR-0040) — no `from`, and not a design hole.
            let is_technical_ts = col.name == "created_at" || col.name == "updated_at";
            if col.from.is_empty() {
                if !view.reference && !is_technical_ts {
                    issues.push(warn(
                        "view-column-no-source",
                        format!("{}.{}", at, col.name),
                        "column has no `from` — not traced to any event (possible design hole).".into(),
                    ));
                }
            } else {
                for r in &col.from {
                    if let Some(ev) = ref_name(r) {
                        if !fed_by_names.contains(ev.as_str()) {
                            issues.push(err(
                                "view-column-source-not-fedby",
                                format!("{}.{}", at, col.name),
                                format!("from '{}' refers to event '{}', which is not in this view's fedBy.", r, ev),
                            ));
                        }
                        used_events.insert(ev);
                    }
                }
            }
            if let Some(fk) = &col.fk {
                let mut parts = fk.splitn(2, '.');
                let fk_view = parts.next().unwrap_or("");
                let fk_col = parts.next().unwrap_or("");
                match views.iter().find(|v| v.name == fk_view) {
                    None => issues.push(err(
                        "view-fk-unknown-view",
                        format!("{}.{}", at, col.name),
                        format!("fk '{}' references unknown view '{}'.", fk, fk_view),
                    )),
                    Some(target) => {
                        if !target.columns.iter().any(|c| c.name == fk_col) {
                            issues.push(err(
                                "view-fk-unknown-column",
                                format!("{}.{}", at, col.name),
                                format!("fk '{}' references unknown column '{}' on '{}'.", fk, fk_col, fk_view),
                            ));
                        }
                    }
                }
            }
        }
        if pk_count == 0 {
            issues.push(warn("view-no-pk", at.clone(), "view declares no primary-key column.".into()));
        }

        for (i, n) in view.fedby.iter().enumerate() {
            if !produced_events.contains(n) {
                issues.push(warn(
                    "view-fedby-unproduced",
                    format!("{}.fedBy[{}]", at, i),
                    format!("fed by '{}', which no actor emits or consumes.", n),
                ));
            }
        }
        for (i, ix) in view.indexes.iter().enumerate() {
            for c in ix {
                if !col_names.contains(c.as_str()) {
                    issues.push(err(
                        "view-index-column",
                        format!("{}.indexes[{}]", at, i),
                        format!("index references unknown column '{}'.", c),
                    ));
                }
            }
        }
        // The `tombstone:` event is USED by construction — the projector routes it to row deletion
        // (emit/projectors.rs), so it can never map to a column `from` and must not read as unused.
        // But that routing walks ONLY `fedBy` (emit/projectors.rs dispatch), so a tombstone absent
        // from fedBy would silently never dispatch: the erasure fold just doesn't happen. Error,
        // not warn — there is no legitimate transitional state where an unroutable tombstone is
        // intended.
        if let Some(tomb) = &view.tombstone {
            if !fed_by_names.contains(tomb.as_str()) {
                issues.push(err(
                    "view-tombstone-not-fedby",
                    at.clone(),
                    format!(
                        "tombstone '{}' is not in this view's fedBy — the projector dispatch routes only fedBy events, so the row deletion would never happen.",
                        tomb
                    ),
                ));
            }
            used_events.insert(tomb.clone());
        }
        if !used_events.is_empty() {
            let mut seen: BTreeSet<&str> = BTreeSet::new();
            for ev in &view.fedby {
                if !seen.insert(ev.as_str()) {
                    continue;
                }
                if !used_events.contains(ev) {
                    issues.push(warn(
                        "view-fedby-unused",
                        at.clone(),
                        format!("fed by '{}' but no column maps `from` it (possible design hole).", ev),
                    ));
                }
            }
        }
    }

    // 5b. every emitted event should be projected into a view, unless declared non-projected.
    let non_projected: BTreeSet<String> = model
        .defs
        .get("database/projection_views.yaml")
        .and_then(|v| v.get("nonProjectedEvents"))
        .and_then(|x| x.as_sequence())
        .map(|s| s.iter().filter_map(|r| r.get("$ref").and_then(|x| x.as_str()).and_then(ref_name)).collect())
        .unwrap_or_default();
    for e in &emitted_events {
        if non_projected.contains(e) {
            continue;
        }
        if !views.iter().any(|v| v.fedby.iter().any(|n| n == e)) {
            issues.push(warn(
                "event-not-projected",
                format!("events.yaml/{}", e),
                format!("Emitted event '{}' feeds no View_* (mark it under views.yaml nonProjectedEvents if intentional).", e),
            ));
        }
    }

    // 5b-bis. Read-model form (ADR-0039): a fold VIEW (projection_views.yaml) must be generatable from its
    // column lineage; a materialized TABLE (projection_tables.yaml) must declare its projector mechanism.
    for view in &views {
        if view.reference {
            continue;
        }
        if view.is_table {
            if view.projector.as_deref() != Some("app") {
                issues.push(err(
                    "projection-table-no-projector",
                    format!("projection_tables.yaml/{}", view.name),
                    "a materialized read-model table must declare `projector: app` (application-layer Rust projector; no SQL triggers — ADR-0040).".into(),
                ));
            }
        } else if view.definition.is_none() {
            if let Err(e) = generate_fold_sql(view, model) {
                issues.push(err(
                    "view-fold-ungeneratable",
                    format!("projection_views.yaml/{}", view.name),
                    format!("fold view cannot be generated: {} (move it to projection_tables.yaml if computed).", e),
                ));
            }
        }
    }

    // 5c. type `reads` (api.yaml) bind output types to views.
    {
        // The ownership wall lives in validate::read_targets (extracted so its rules can be driven on a
        // small fixture, like validate_ref_kinds): who may be a `reads:` target at all, the
        // `reference: true` opt-in's own guard, and the transient types' `readsInfrastructure:`.
        let bound_views = validate_read_targets(model, &api, &views, &mut cov, &mut issues);
        let internal_views: BTreeSet<&str> = views.iter().filter(|v| v.internal).map(|v| v.name.as_str()).collect();
        // navRoles (#22, ADR-20260720-230000): each key must be a DERIVED navigation edge on that
        // type, each list a LITERAL roles list (ADR-20260720-191500 semantics).
        {
            let registered: HashSet<String> = api.types.iter().map(|t| t.name.clone()).collect();
            let nav = nav_fields(&views, &registered);
            for t in &api.types {
                for (field, roles) in &t.nav_roles {
                    let known = nav
                        .get(&t.name)
                        .map_or(false, |nfs| nfs.iter().any(|n| &n.field == field));
                    if !known {
                        issues.push(err(
                            "nav-roles-unknown-field",
                            format!("api.yaml/types.{}", t.name),
                            format!("navRoles key '{}' is not a derived navigation field on '{}'.", field, t.name),
                        ));
                    }
                    check_roles(
                        &mut issues,
                        roles,
                        &format!("api.yaml/types.{}.navRoles.{}", t.name, field),
                        &user_type_set,
                    );
                }
            }
        }
        // Non-GraphQL readers (#305): `components.*.reads[*]` in architecture/c4-l3.yaml is the mirror
        // of `updates[*]`, and declares the consumers no api.yaml type can speak for — the tenant host
        // router, command handlers using a read as a write-side invariant, process managers, the ACLs.
        let mut component_reads: BTreeSet<String> = BTreeSet::new();
        if let Some(cm) = model
            .defs
            .get("architecture/c4-l3.yaml")
            .and_then(|v| v.get("components"))
            .and_then(|v| v.as_mapping())
        {
            for (name, comp) in cm {
                let component = name.as_str().unwrap_or("");
                for r in comp.get("reads").and_then(|v| v.as_sequence()).into_iter().flatten() {
                    if let Some(target) = r.get("$ref").and_then(|x| x.as_str()).and_then(|s| s.rsplit('/').next()) {
                        // The two declarations must not overlap. A GraphQL-reached read model is
                        // declared by its api.yaml type `reads:` binding; re-listing it on the gateway
                        // would let ONE blanket component declaration satisfy the reader gate for every
                        // model at once, and would drift from api.yaml the moment a binding moves.
                        if component == "graphql-gateway" {
                            issues.push(err(
                                "gateway-declares-reads",
                                "architecture/c4-l3.yaml/components.graphql-gateway".to_string(),
                                format!(
                                    "graphql-gateway must not declare `reads` ('{}') — a read model reached through \
                                     GraphQL is declared by its api.yaml output type `reads:` binding. `components.*.reads` \
                                     is for consumers no api.yaml type can speak for.",
                                    target
                                ),
                            ));
                            continue;
                        }
                        cov.component_reads_links += 1;
                        component_reads.insert(target.to_string());
                    }
                }
            }
        }
        // Every read model must have a DECLARED reader — the read-side mirror of the write side's
        // spec-gated surface (ADR-20260802-170059, issue #305). Three ways to satisfy it, all
        // declarations rather than exemptions: an api.yaml output type binds it (`reads:`), a c4-l3
        // component declares it, or it is explicitly `internal: true`. Replaces the old `view-no-query`
        // WARNING, which only ever asked the first question and so could not see a non-GraphQL reader.
        for v in &views {
            if !bound_views.contains(&v.name)
                && !component_reads.contains(&v.name)
                && !internal_views.contains(v.name.as_str())
            {
                issues.push(err(
                    "read-model-no-reader",
                    format!("views.yaml/{}", v.name),
                    format!(
                        "read model '{}' has no declared reader — bind it to an api.yaml output type (`reads:`), \
                         declare the consuming component in architecture/c4-l3.yaml (`components.*.reads`), \
                         or mark it `internal: true`.",
                        v.name
                    ),
                ));
            }
        }
    }

    // --- 6. Story map (stories.yaml): personas → activities → steps -----------------------------
    let personas = parse_stories(model);
    {
        let query_roles: HashMap<&str, &Vec<String>> = api.queries.iter().map(|q| (q.name.as_str(), &q.roles)).collect();
        let mutation_roles: HashMap<&str, &Vec<String>> =
            api.mutations.iter().map(|m| (m.name.as_str(), &m.roles)).collect();
        for p in &personas {
            let at = format!("stories.yaml/{}", p.name);
            if p.role.is_empty() {
                issues.push(err("persona-no-role", at.clone(), "persona declares no personaRole.".into()));
            } else if !user_type_set.contains(&p.role) {
                issues.push(err(
                    "persona-unknown-role",
                    at.clone(),
                    format!("personaRole '{}' is not a scalars.yaml#/UserType.", p.role),
                ));
            }
            for act in &p.activities {
                for step in &act.steps {
                    let (op, op_kind) = match (&step.op, &step.op_kind) {
                        (Some(o), Some(k)) => (o, k),
                        _ => continue,
                    };
                    cov.story_links += 1;
                    let where_ = format!("{}.{}.{}", at, act.name, step.name);
                    let roles = if op_kind == "query" { query_roles.get(op.as_str()) } else { mutation_roles.get(op.as_str()) };
                    let roles = match roles {
                        Some(r) => *r,
                        None => {
                            issues.push(err(
                                "story-unknown-op",
                                where_.clone(),
                                format!("step references unknown {} '{}'.", op_kind, op),
                            ));
                            continue;
                        }
                    };
                    // Literal roles (ADR-20260720-191500): omitted = every persona may call it;
                    // present = the persona's path-role must be listed (PUBLIC = the anonymous path).
                    let allowed = roles.is_empty() || (!p.role.is_empty() && roles.iter().any(|r| r == &p.role));
                    if !allowed {
                        issues.push(err(
                            "story-role-not-authorized",
                            where_,
                            format!(
                                "persona role '{}' may not call {} '{}' (op roles: [{}]).",
                                p.role,
                                op_kind,
                                op,
                                roles.join(", ")
                            ),
                        ));
                    }
                }
            }
        }
        // COMPLETENESS: every mutation & query must be reached by ≥1 story step.
        let mut story_ops: BTreeSet<&str> = BTreeSet::new();
        for p in &personas {
            for act in &p.activities {
                for step in &act.steps {
                    if let Some(o) = &step.op {
                        story_ops.insert(o.as_str());
                    }
                }
            }
        }
        for m in &api.mutations {
            if !story_ops.contains(m.name.as_str()) {
                issues.push(err(
                    "op-uncovered-by-story",
                    format!("api.yaml/mutations/{}", m.name),
                    format!("mutation '{}' is referenced by no story step (stories.yaml) — every write must anchor to a persona use case.", m.name),
                ));
            }
        }
        for q in &api.queries {
            if !story_ops.contains(q.name.as_str()) {
                issues.push(err(
                    "op-uncovered-by-story",
                    format!("api.yaml/queries/{}", q.name),
                    format!("query '{}' is referenced by no story step (stories.yaml) — every read must anchor to a persona use case.", q.name),
                ));
            }
        }
    }

    // --- 7. Behaviour tests (tests.yaml): fixtures + Given/When/Then consistency -----------------
    {
        let empty = Value::Mapping(Default::default());
        let tests_file = model.defs.get("tests.yaml").unwrap_or(&empty);
        let fixtures = tests_file.get("fixtures").and_then(|x| x.as_mapping());
        let tests = tests_file.get("tests").and_then(|x| x.as_mapping());

        // Per-actor inbox.
        struct InboxEntry {
            actor: String,
            file: &'static str,
            message: String,
            is_command: bool,
            emits: BTreeSet<String>,
            throws: BTreeSet<String>,
        }
        let mut inbox: HashMap<String, HashMap<String, usize>> = HashMap::new();
        let mut inbox_entries: Vec<InboxEntry> = Vec::new();
        let mut t_emitted_events: BTreeSet<String> = BTreeSet::new();
        let mut t_throwable_errors: BTreeSet<String> = BTreeSet::new();
        for a in &actors {
            let mut by_msg: HashMap<String, usize> = HashMap::new();
            for e in &a.receives {
                // A reminder receive is keyed by its PAYLOAD event (record semantics,
                // ADR-20260731-153000): the delivery records that fact, so that is the vocabulary a
                // behaviour test's `when.type` names (tests.*.when.type must be a command or event).
                let msg = match reminder_ref_parts(&e.message_ref) {
                    Some((_, rname)) => reminder_payload_event(model, &e.message_ref).unwrap_or(rname),
                    None => match ref_name(&e.message_ref) {
                        Some(m) => m,
                        None => continue,
                    },
                };
                let emits: BTreeSet<String> = e.emits.iter().filter_map(|r| ref_name(r)).collect();
                let throws: BTreeSet<String> = e.throws.iter().filter_map(|r| ref_name(r)).collect();
                for ev in &emits {
                    t_emitted_events.insert(ev.clone());
                }
                for er in &throws {
                    t_throwable_errors.insert(er.clone());
                }
                let idx = inbox_entries.len();
                inbox_entries.push(InboxEntry {
                    actor: a.name.clone(),
                    file: a.file,
                    message: msg.clone(),
                    is_command: e.message_ref.starts_with("commands.yaml#/"),
                    emits,
                    throws,
                });
                by_msg.insert(msg, idx);
            }
            inbox.insert(a.name.clone(), by_msg);
        }

        let mut used_messages: BTreeSet<String> = BTreeSet::new();
        let mut used_events: BTreeSet<String> = BTreeSet::new();
        let mut used_errors: BTreeSet<String> = BTreeSet::new();
        let mut used_rules: BTreeSet<String> = BTreeSet::new();
        let all_rules = map_keys(model.defs.get("rules.yaml"));
        cov.rules = all_rules.len();

        // 7a. fixtures: data shape.
        if let Some(fx_map) = fixtures {
            for (k, fx) in fx_map {
                let name = match k.as_str() {
                    Some(s) => s,
                    None => continue,
                };
                let where_ = format!("tests.yaml/fixtures.{}", name);
                match fx.get("type").and_then(|t| t.get("$ref")).and_then(|x| x.as_str()) {
                    None => issues.push(err("fixture-no-type", where_, "fixture has no `type.$ref`.".into())),
                    Some(rf) => check_data_shape(model, &mut issues, rf, fx.get("data"), &where_),
                }
            }
        }

        // 7b. tests.
        cov.test_cases = tests.map(|t| t.len()).unwrap_or(0);
        if let Some(t_map) = tests {
            for (k, t) in t_map {
                let name = match k.as_str() {
                    Some(s) => s,
                    None => continue,
                };
                let where_ = format!("tests.yaml/tests.{}", name);
                let actor_name = t
                    .get("actor")
                    .and_then(|a| a.get("$ref"))
                    .and_then(|x| x.as_str())
                    .and_then(ref_name)
                    .unwrap_or_default();
                let when = t.get("when");
                let when_ref = when.and_then(|w| w.get("type")).and_then(|ty| ty.get("$ref")).and_then(|x| x.as_str());
                let when_ref = match when_ref {
                    Some(r) => r,
                    None => {
                        issues.push(err("test-no-when", where_, "test has no `when.type.$ref` (command or event).".into()));
                        continue;
                    }
                };
                check_data_shape(model, &mut issues, when_ref, when.and_then(|w| w.get("data")), &format!("{}.when", where_));

                let msg = ref_name(when_ref).unwrap_or_default();
                let entry_idx = if !actor_name.is_empty() && !msg.is_empty() {
                    inbox.get(&actor_name).and_then(|m| m.get(&msg)).copied()
                } else {
                    None
                };
                match entry_idx {
                    None => issues.push(err(
                        "test-message-not-handled",
                        format!("{}.when", where_),
                        format!("actor '{}' does not receive '{}' (actors.yaml/processmanager.yaml inbox).", actor_name, msg),
                    )),
                    Some(idx) => {
                        used_messages.insert(format!("{}::{}", actor_name, msg));
                        if !inbox_entries[idx].is_command {
                            used_events.insert(msg.clone());
                        }
                    }
                }

                // `given` preconditions exercise their events too.
                if let Some(given) = t.get("given").and_then(|x| x.as_sequence()) {
                    for g in given {
                        if let Some(ev) = fixture_event(model, g.get("$ref").and_then(|x| x.as_str())) {
                            used_events.insert(ev);
                        }
                    }
                }

                // Every test must assert ≥1 business rule (ADR-0032).
                let test_rules = t.get("rules").and_then(|x| x.as_sequence()).cloned().unwrap_or_default();
                if test_rules.is_empty() {
                    issues.push(err(
                        "test-no-rule",
                        where_.clone(),
                        "test asserts no business rule — add `rules: [{ $ref: 'rules.yaml#/<Rule>' }]` (ADR-0032).".into(),
                    ));
                }
                for (i, r) in test_rules.iter().enumerate() {
                    let rf = r.get("$ref").and_then(|x| x.as_str()).unwrap_or("");
                    if ref_target_file(rf, "tests.yaml").as_deref() != Some("rules.yaml") {
                        issues.push(err(
                            "test-rule-wrong-file",
                            format!("{}.rules[{}]", where_, i),
                            format!("rule ref '{}' must target rules.yaml.", rf),
                        ));
                        continue;
                    }
                    if let Some(rn) = ref_name(rf) {
                        used_rules.insert(rn);
                    }
                }

                // A test must assert SOMETHING.
                let obj = t.as_mapping();
                let has_then = obj.map(|o| o.contains_key(Value::String("then".into()))).unwrap_or(false);
                let has_thrown = obj.map(|o| o.contains_key(Value::String("thrown".into()))).unwrap_or(false);
                if !has_then && !has_thrown {
                    issues.push(err(
                        "test-no-assertion",
                        where_.clone(),
                        "test asserts nothing — declare `then` (events, [] for a no-op) and/or `thrown` (errors).".into(),
                    ));
                }

                if let Some(thens) = t.get("then").and_then(|x| x.as_sequence()) {
                    for (i, th) in thens.iter().enumerate() {
                        let ev = match fixture_event(model, th.get("$ref").and_then(|x| x.as_str())) {
                            Some(e) => e,
                            None => continue,
                        };
                        used_events.insert(ev.clone());
                        if let Some(idx) = entry_idx {
                            if !inbox_entries[idx].emits.contains(&ev) {
                                issues.push(err(
                                    "test-then-not-emitted",
                                    format!("{}.then[{}]", where_, i),
                                    format!("expected event '{}' is not emitted by '{}' for '{}'.", ev, inbox_entries[idx].actor, msg),
                                ));
                            }
                        }
                    }
                }

                if let Some(throwns) = t.get("thrown").and_then(|x| x.as_sequence()) {
                    for (i, th) in throwns.iter().enumerate() {
                        let er = match th.get("$ref").and_then(|x| x.as_str()).and_then(ref_name) {
                            Some(e) => e,
                            None => continue,
                        };
                        used_errors.insert(er.clone());
                        if let Some(idx) = entry_idx {
                            if !inbox_entries[idx].throws.contains(&er) {
                                issues.push(err(
                                    "test-thrown-not-declared",
                                    format!("{}.thrown[{}]", where_, i),
                                    format!("error '{}' is not declared in '{}' throws for '{}' (actors.yaml).", er, inbox_entries[idx].actor, msg),
                                ));
                            }
                        }
                    }
                }
            }
        }

        // 7c. COVERAGE (blocking).
        for e in &inbox_entries {
            if !used_messages.contains(&format!("{}::{}", e.actor, e.message)) {
                issues.push(err(
                    "test-uncovered-message",
                    format!("{}/{}", e.file, e.actor),
                    format!("no test exercises {} '{}' on '{}'.", if e.is_command { "command" } else { "event" }, e.message, e.actor),
                ));
            }
        }
        for ev in &t_emitted_events {
            if !used_events.contains(ev) {
                issues.push(err(
                    "test-uncovered-event",
                    format!("events.yaml/{}", ev),
                    format!("emitted event '{}' is asserted by no test (in a `then`/`given`).", ev),
                ));
            }
        }
        for er in &t_throwable_errors {
            if !used_errors.contains(er) {
                issues.push(err(
                    "test-uncovered-error",
                    format!("errors.yaml/{}", er),
                    format!("throwable error '{}' is asserted by no test (in a `thrown`).", er),
                ));
            }
        }
        for rn in &all_rules {
            if !used_rules.contains(rn) {
                issues.push(err(
                    "rule-uncovered",
                    format!("rules.yaml/{}", rn),
                    format!("business rule '{}' is asserted by no test — add a test with `rules: [{{ $ref: 'rules.yaml#/{}' }}]` or remove the rule (ADR-0032).", rn, rn),
                ));
            }
        }
    }

    // --- 8. Observability contracts (observability.yaml) ----------------------------------------
    {
        let span_kinds: BTreeSet<&str> = ["SERVER", "CLIENT", "INTERNAL", "PRODUCER", "CONSUMER"].into_iter().collect();
        // Dispatch surfaces a contract may bind INSTEAD of a single command/saga/aggregate
        // (ADR-20260721-031127: pipeline contracts, e.g. command-acceptance over the GraphQL dispatch).
        const SURFACE_KINDS: [&str; 1] = ["graphql"];
        if let Some(obs) = model.defs.get("observability.yaml").and_then(|x| x.as_mapping()) {
            for (fk, c) in obs {
                let feature = match fk.as_str() {
                    Some(s) => s,
                    None => continue,
                };
                let at = format!("observability.yaml/{}", feature);
                cov.obs_contracts += 1;

                let wf = c.get("workflow");
                let has = |k: &str| wf.and_then(|w| w.get(k)).map(|v| !v.is_null()).unwrap_or(false);
                let surface = wf.and_then(|w| w.get("surface")).and_then(|v| v.as_str());
                if surface.is_none() && !has("command") && !has("saga") && !has("aggregate") {
                    issues.push(err(
                        "obs-no-workflow-binding",
                        at.clone(),
                        "workflow must bind a `command` and/or `saga`/`aggregate` ($ref into the model), or a dispatch `surface`.".into(),
                    ));
                }
                if let Some(s) = surface {
                    if !SURFACE_KINDS.contains(&s) {
                        issues.push(err(
                            "obs-surface-unknown",
                            format!("{}.workflow.surface", at),
                            format!("surface '{}' is not a known dispatch surface ({}).", s, SURFACE_KINDS.join("|")),
                        ));
                    }
                    if has("command") || has("saga") || has("aggregate") {
                        issues.push(err(
                            "obs-surface-exclusive",
                            format!("{}.workflow", at),
                            "a `surface` contract binds the whole dispatch surface — it must not also bind a `command`/`saga`/`aggregate`.".into(),
                        ));
                    }
                }

                let id_names: BTreeSet<&str> = c
                    .get("run_identity")
                    .and_then(|x| x.as_sequence())
                    .map(|s| s.iter().filter_map(|i| i.get("name").and_then(|n| n.as_str())).collect())
                    .unwrap_or_default();
                for must in ["correlation_id", "trace_id"] {
                    if !id_names.contains(must) {
                        issues.push(err(
                            "obs-missing-id",
                            format!("{}.run_identity", at),
                            format!("run_identity must declare the mandatory id '{}'.", must),
                        ));
                    }
                }

                let spans = c.get("spans").and_then(|x| x.as_sequence()).cloned().unwrap_or_default();
                if spans.is_empty() {
                    issues.push(err("obs-no-spans", at.clone(), "contract declares no spans.".into()));
                }
                let mut span_names: BTreeSet<String> = BTreeSet::new();
                for (i, s) in spans.iter().enumerate() {
                    match s.get("name").and_then(|x| x.as_str()) {
                        None => issues.push(err("obs-span-no-name", format!("{}.spans[{}]", at, i), "span has no `name`.".into())),
                        Some(n) => {
                            span_names.insert(n.to_string());
                        }
                    }
                    if let Some(kind) = s.get("kind").and_then(|x| x.as_str()) {
                        if !span_kinds.contains(kind) {
                            issues.push(err(
                                "obs-span-kind",
                                format!("{}.spans[{}]", at, i),
                                format!("span kind '{}' is not one of SERVER|CLIENT|INTERNAL|PRODUCER|CONSUMER.", kind),
                            ));
                        }
                    }
                }

                let req_spans = c
                    .get("status_rules")
                    .and_then(|sr| sr.get("success"))
                    .and_then(|s| s.get("required_spans"))
                    .and_then(|x| x.as_sequence())
                    .map(|s| s.iter().filter_map(|v| v.as_str().map(|x| x.to_string())).collect::<Vec<_>>())
                    .unwrap_or_default();
                for rs in &req_spans {
                    if !span_names.contains(rs) {
                        issues.push(err(
                            "obs-required-span-undeclared",
                            format!("{}.status_rules.success", at),
                            format!("required_span '{}' is not a declared span.", rs),
                        ));
                    }
                }
            }
        }
    }

    // --- 9. C4 consistency (architecture/c4-l2.yaml) --------------------------------------------
    {
        let l2 = model.defs.get("architecture/c4-l2.yaml");
        let bcs = l2.and_then(|v| v.get("boundedContexts")).and_then(|x| x.as_mapping());
        let mut mapped: BTreeSet<String> = BTreeSet::new();
        if let Some(bcs) = bcs {
            for (_, bc) in bcs {
                for key in ["aggregates", "processManagers"] {
                    if let Some(seq) = bc.get(key).and_then(|x| x.as_sequence()) {
                        for r in seq {
                            if let Some(n) = r.get("$ref").and_then(|x| x.as_str()).and_then(ref_name) {
                                mapped.insert(n);
                            }
                        }
                    }
                }
            }
            for a in &actors {
                if !mapped.contains(&a.name) {
                    issues.push(warn(
                        "c4-actor-unmapped",
                        "architecture/c4-l2.yaml".into(),
                        format!("actor '{}' belongs to no bounded context (C4 L2 drift).", a.name),
                    ));
                }
            }
            // Container split (ADR-20260807-183024): every actor/PM is REALIZED by exactly one
            // container (the bin ↔ deployable binding the #349 emitter chain consumes), and every
            // l3 component's `container:` names a real l2 container.
            let c4m = read_c4(model);
            let container_ids: BTreeSet<&str> = c4m.containers.iter().map(|c| c.id.as_str()).collect();
            let mut realized_by: HashMap<&str, &str> = HashMap::new();
            for c in &c4m.containers {
                for n in &c.realizes {
                    match realized_by.get(n.as_str()) {
                        Some(prev) => issues.push(err(
                            "c4-actor-realized-twice",
                            format!("architecture/c4-l2.yaml/{}", c.id),
                            format!(
                                "'{}' is realized by both '{}' and '{}' — one actor, one bin (the realizes binding becomes the image/Deployment mapping).",
                                n, prev, c.id
                            ),
                        )),
                        None => {
                            realized_by.insert(n.as_str(), c.id.as_str());
                        }
                    }
                }
            }
            for a in &actors {
                if !realized_by.contains_key(a.name.as_str()) {
                    issues.push(warn(
                        "c4-actor-unrealized",
                        "architecture/c4-l2.yaml".into(),
                        format!(
                            "actor '{}' is realized by no container — it would build into no bin and deploy nowhere (add a realizes: entry).",
                            a.name
                        ),
                    ));
                }
            }
            for comp in &c4m.components {
                if let Some(home) = &comp.container {
                    if !container_ids.contains(home.as_str()) {
                        issues.push(err(
                            "c4-component-container-unknown",
                            format!("architecture/c4-l3.yaml/components.{}", comp.id),
                            format!("component '{}' declares container '{}' which is not an l2 container.", comp.id, home),
                        ));
                    }
                }
            }
            let mut role_owner: HashMap<String, String> = HashMap::new();
            for (ck, bc) in bcs {
                let cid = ck.as_str().unwrap_or("");
                if let Some(roles) = bc.get("roles").and_then(|x| x.as_sequence()) {
                    for role in roles {
                        let r = role.as_str().map(|s| s.to_string()).unwrap_or_else(|| format!("{:?}", role));
                        if !user_type_set.is_empty() && !user_type_set.contains(&r) {
                            issues.push(err(
                                "c4-context-role-unknown",
                                format!("architecture/c4-l2.yaml/{}", cid),
                                format!("bounded-context role '{}' is not a scalars.yaml#/UserType value.", r),
                            ));
                        }
                        match role_owner.get(&r) {
                            Some(prev) if prev != cid => issues.push(err(
                                "c4-context-role-overlap",
                                format!("architecture/c4-l2.yaml/{}", cid),
                                format!("UserType '{}' is claimed by both '{}' and '{}' — each role maps to at most one context.", r, prev, cid),
                            )),
                            _ => {
                                role_owner.insert(r, cid.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // --- 10 (+10b). Translation hygiene (#110): uniqueness, full locale coverage, param match, and the
    // unused-key + code_refs gates. Extracted to a standalone fn (like `validate_ref_kinds`) so tests can
    // exercise it on minimal fixtures without running the whole validator.
    cov.translations += translation_entries(model).len();
    validate_translations(model, &mut issues);

    // --- 14. Per-scope spec folders (ADR-20260807-183024, #375): placement, cross-scope DAG,
    // kernel purity, api nested-intra-scope. No-op on a flat layout (fixtures have no scope dirs).
    validate_scopes(model, &mut issues);

    // --- 15. Bin topology ↔ c4-l2 containers (ADR-20260807-183024 step 3, #382): derived bins
    // and the container list may not drift, either direction. No-op without containers/scopes.
    validate_bin_topology(model, &mut issues);

    // --- 11. SDUI screens (screens/*.yaml, one file per app/audience): each app's spec is bound to the
    // API (ADR-0033/0037). Generic over all screens files — no hard-coded screens filename. Each screen
    // declares `roles` (⊆ UserType) and the file declares `app_types` (⊆ web|ios|android|windows).
    {
        let query_names: BTreeSet<&str> = api.queries.iter().map(|q| q.name.as_str()).collect();
        let mutation_names: BTreeSet<&str> = api.mutations.iter().map(|m| m.name.as_str()).collect();
        let op_name = |r: &str| r.rsplit('/').next().unwrap_or("").to_string();
        const APP_TYPES: [&str; 4] = ["web", "ios", "android", "windows"];
        let screens_files: Vec<String> =
            model.defs.keys().filter(|k| k.starts_with("screens/")).cloned().collect();

        for sfkey in &screens_files {
            let cs = model.defs.get(sfkey);
            let resolvers = cs.and_then(|v| v.get("resolvers")).and_then(|x| x.as_mapping());
            let actions = cs.and_then(|v| v.get("actions")).and_then(|x| x.as_mapping());
            let mut resolver_names: BTreeSet<String> = BTreeSet::new();

            // File-level app_types (target platforms) must be known.
            if let Some(ats) = cs.and_then(|v| v.get("app_types")).and_then(|x| x.as_sequence()) {
                for at in ats {
                    if let Some(a) = at.as_str() {
                        if !APP_TYPES.contains(&a) {
                            issues.push(err(
                                "screen-unknown-apptype",
                                format!("{}/app_types", sfkey),
                                format!("app_type '{}' is not one of web|ios|android|windows.", a),
                            ));
                        }
                    }
                }
            }

            if let Some(rmap) = resolvers {
                for (nk, r) in rmap {
                    let name = match nk.as_str() {
                        Some(s) => s,
                        None => continue,
                    };
                    resolver_names.insert(name.to_string());
                    if r.get("gap").map(|v| !v.is_null()).unwrap_or(false) {
                        cov.screen_gaps += 1;
                        continue;
                    }
                    match r.get("query").and_then(|q| q.get("$ref")).and_then(|x| x.as_str()) {
                        None => issues.push(err(
                            "resolver-no-binding",
                            format!("{}/resolvers/{}", sfkey, name),
                            format!("resolver '{}' must declare a `query` ($ref into api.yaml) or a `gap`.", name),
                        )),
                        Some(rf) => {
                            if ref_target_file(rf, sfkey).as_deref() != Some("api.yaml")
                                || !rf.contains("/queries/")
                                || !query_names.contains(op_name(rf).as_str())
                            {
                                issues.push(err(
                                    "resolver-not-a-query",
                                    format!("{}/resolvers/{}", sfkey, name),
                                    format!("resolver '{}' query must $ref an api.yaml query; '{}' is not one.", name, rf),
                                ));
                            } else {
                                cov.screen_bindings += 1;
                                // The binding is a real query — now prove its pinned static args
                                // name real arguments of THAT query (#82).
                                if let Some(args) = r.get("args") {
                                    validate_resolver_args(
                                        model,
                                        &mut issues,
                                        &format!("{}/resolvers/{}/args", sfkey, name),
                                        &op_name(rf),
                                        args,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            // action name → the command its mutation carries, so a component's `action.variables`
            // can be checked against the command's REQUIRED input properties below.
            let mut action_command: HashMap<String, String> = HashMap::new();
            if let Some(amap) = actions {
                for (nk, a) in amap {
                    let name = match nk.as_str() {
                        Some(s) => s,
                        None => continue,
                    };
                    let rf = match a.get("mutation").and_then(|m| m.get("$ref")).and_then(|x| x.as_str()) {
                        Some(r) => r,
                        None => continue,
                    };
                    if let Some(cmd) = model
                        .defs
                        .get("api.yaml")
                        .and_then(|v| v.get("mutations"))
                        .and_then(|v| v.get(op_name(rf).as_str()))
                        .and_then(|v| v.get("command"))
                        .and_then(|v| v.get("$ref"))
                        .and_then(|x| x.as_str())
                    {
                        action_command.insert(name.to_string(), op_name(cmd));
                    }
                    if ref_target_file(rf, sfkey).as_deref() != Some("api.yaml")
                        || !rf.contains("/mutations/")
                        || !mutation_names.contains(op_name(rf).as_str())
                    {
                        issues.push(err(
                            "action-not-a-mutation",
                            format!("{}/actions/{}", sfkey, name),
                            format!("action '{}' mutation must $ref an api.yaml mutation; '{}' is not one.", name, rf),
                        ));
                    } else {
                        cov.screen_bindings += 1;
                    }
                }
            }
            if let Some(screens) = cs.and_then(|v| v.get("screens")).and_then(|x| x.as_sequence()) {
                for s in screens {
                    cov.screens += 1;
                    let sid = s.get("id").and_then(|x| x.as_str()).unwrap_or("?").to_string();
                    cov.screen_gaps += s.get("gaps").and_then(|x| x.as_sequence()).map(|g| g.len()).unwrap_or(0);
                    // A screen ACTION is the caller of its mutation, so unlike a resolver's pinned
                    // `args:` (a static default the runtime merges caller variables over) its
                    // `variables:` are the whole input. A required command property with no variable
                    // is a form that cannot submit — invisible until a human presses the button,
                    // because `action-not-a-mutation` only proves the $ref names a mutation and
                    // `op-uncovered-by-story` is satisfied by a story STEP, which is not a screen.
                    let mut found: Vec<(String, BTreeSet<String>)> = Vec::new();
                    if let Some(cs) = s.get("components") {
                        collect_screen_actions(cs, &mut found);
                    }
                    for (action_name, provided) in found {
                        let Some(cmd) = action_command.get(&action_name) else { continue };
                        let Some(required) = model
                            .defs
                            .get("commands.yaml")
                            .and_then(|v| v.get(cmd.as_str()))
                            .and_then(|v| v.get("required"))
                            .and_then(|v| v.as_sequence())
                        else {
                            continue;
                        };
                        // The mirror of `resolver-unknown-arg` on the write side: a variable that
                        // names no property of the command is dropped on the floor, and reads in the
                        // spec like the input IS wired.
                        if let Some(props) = model
                            .defs
                            .get("commands.yaml")
                            .and_then(|v| v.get(cmd.as_str()))
                            .and_then(|v| v.get("properties"))
                            .and_then(|v| v.as_mapping())
                        {
                            let unknown: Vec<&str> = provided
                                .iter()
                                .map(|p| p.as_str())
                                .filter(|p| props.get(Value::String(p.to_string())).is_none())
                                .collect();
                            if !unknown.is_empty() {
                                issues.push(warn(
                                    "action-unknown-input",
                                    format!("{}/screens/{}/{}", sfkey, sid, action_name),
                                    format!(
                                        "action '{}' passes variable {}, which commands.yaml#/{} does not declare.",
                                        action_name,
                                        unknown.join(", "),
                                        cmd
                                    ),
                                ));
                            }
                        }
                        let missing: Vec<&str> = required
                            .iter()
                            .filter_map(|r| r.as_str())
                            .filter(|r| !provided.contains(*r))
                            .collect();
                        if !missing.is_empty() {
                            issues.push(warn(
                                "action-missing-required-input",
                                format!("{}/screens/{}/{}", sfkey, sid, action_name),
                                format!(
                                    "action '{}' supplies no variable for required {} property {} of \
                                     commands.yaml#/{} — the mutation is unsubmittable from this screen.",
                                    action_name,
                                    if missing.len() == 1 { "input" } else { "inputs" },
                                    missing.join(", "),
                                    cmd
                                ),
                            ));
                        }
                    }
                    // Per-screen roles must be scalars.yaml#/UserType values.
                    if let Some(rs) = s.get("roles").and_then(|x| x.as_sequence()) {
                        for r in rs {
                            if let Some(role) = r.as_str() {
                                if !user_type_set.contains(role) {
                                    issues.push(err(
                                        "screen-unknown-role",
                                        format!("{}/screens/{}", sfkey, sid),
                                        format!("role '{}' is not a scalars.yaml#/UserType value.", role),
                                    ));
                                }
                            }
                        }
                    }
                    if let Some(drs) = s.get("data_requirements").and_then(|x| x.as_sequence()) {
                        for dr in drs {
                            let name = dr.as_str().map(|s| s.to_string()).unwrap_or_else(|| format!("{:?}", dr));
                            if !resolver_names.contains(&name) {
                                issues.push(err(
                                    "screen-unknown-resolver",
                                    format!("{}/screens/{}", sfkey, sid),
                                    format!("data_requirement '{}' is not a declared resolver.", name),
                                ));
                            }
                        }
                    }
                }
            }
            // --- Translation-ref scope (ADR-20260722-101500): the API refs live in `resolvers`/`actions`
            // (validated above); EVERY OTHER `$ref` in a screen is a content/text slot and MUST be a
            // translation ref that resolves to a real entry (a key carrying `messages`). This catches
            // dangling/renamed keys AND text slots pointing at the wrong file/scope (e.g. an api.yaml or
            // scalar ref where a string is expected).
            if let Some(map) = cs.and_then(|v| v.as_mapping()) {
                let mut refs: Vec<(String, String)> = Vec::new();
                for (k, v) in map {
                    match k.as_str() {
                        Some("resolvers") | Some("actions") => {} // API bindings — validated above.
                        Some(key) => collect_refs(v, &format!("{}.{}", sfkey, key), &mut refs),
                        None => {}
                    }
                }
                for (loc, rf) in &refs {
                    // A screen-level realtime binding (`subscription: { $ref: api.yaml#/subscriptions/… }`)
                    // is an API ref, not content — skip it (validated as an operation elsewhere).
                    if loc.ends_with(".subscription") {
                        continue;
                    }
                    match ref_target_file(rf, sfkey).as_deref() {
                        Some(f) if f == "translations.yaml" || f.ends_with(".translations.yaml") => {
                            if resolve_ref(model, rf, sfkey).and_then(|n| n.get("messages")).is_none() {
                                issues.push(err(
                                    "screen-translation-ref-unresolved",
                                    loc.clone(),
                                    format!("translation $ref '{}' does not resolve to a translation entry (a key with `messages`).", rf),
                                ));
                            }
                        }
                        other => issues.push(err(
                            "screen-ref-out-of-scope",
                            loc.clone(),
                            format!("content $ref '{}' in a screen must be a translations key; it targets '{}'.", rf, other.unwrap_or("<local/unknown>")),
                        )),
                    }
                }
            }
        }
    }

    // --- 12. Rust codegen naming: a generated type name must not collide with a Rust reserved/prelude
    // type (the codegen emits it verbatim as a Rust `struct`/`enum`). Resolve at the root — rename it in
    // the spec — rather than working around it in the generator (ADR-0035 naming policy).
    {
        let reserved: BTreeSet<&str> = [
            "Option", "Result", "Box", "Vec", "String", "Some", "None", "Ok", "Err", "Copy", "Clone",
            "Debug", "Default", "Drop", "Eq", "Ord", "PartialEq", "PartialOrd", "Hash", "Iterator", "Send",
            "Sync", "Sized", "From", "Into", "TryFrom", "TryInto", "ToString", "AsRef", "AsMut", "Fn",
            "FnMut", "FnOnce", "Self", "Cow", "Rc", "Arc", "Cell", "RefCell", "Duration", "Ordering",
        ]
        .into_iter()
        .collect();
        for file in ["scalars.yaml", "entities.yaml"] {
            for name in map_keys(model.defs.get(file)) {
                if reserved.contains(name.as_str()) {
                    issues.push(err(
                        "rust-reserved-typename",
                        format!("{}/{}", file, name),
                        format!("type name '{}' collides with a Rust prelude/reserved type — rename it in the spec (generated Rust cannot use it as a struct/enum).", name),
                    ));
                }
            }
        }
    }

    Report { issues, coverage: cov, handled_commands: handled_commands.len() }
}

/// checkData: resolve a `type.$ref` then check the data against its schema (validate.ts §7 checkData).
pub(crate) fn check_data_shape(model: &Model, issues: &mut Vec<Issue>, type_ref: &str, data: Option<&Value>, where_: &str) {
    check_shape(model, issues, resolve_ref(model, type_ref, "tests.yaml"), data, where_);
}

pub(crate) fn map_of_keys(m: &serde_yaml::Mapping) -> BTreeSet<String> {
    m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect()
}

