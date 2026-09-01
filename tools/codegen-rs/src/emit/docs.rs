use crate::*;

// ─── documentation.generated.md (port of emit/documentation.ts) ─────────────────────────────────

pub(crate) fn d_emo(kind: &str) -> &'static str {
    match kind {
        "scalar" => "🔤", "entity" => "📦", "command" => "📩", "event" => "⚡", "view" => "🗄️",
        "actor" => "🎭", "type" => "🧩", "query" => "🔎", "mutation" => "✏️", "error" => "⛔",
        "property" => "🔹", "story" => "🎬", "activity" => "🧭", "test" => "🧪", "obs" => "📡",
        "context" => "🔲", "container" => "🧱", "component" => "⚙️", "subscription" => "🔔",
        "rule" => "📐", "screen" => "📱", "translation" => "🌐", _ => "•",
    }
}
/// Render ONE `status_rules.success.required_spans` term: a plain span name, or the ALTERNATION
/// `{ any_of: [a, b] }` as `(a | b)` (#598).
///
/// It exists because both docs surfaces used to render only the STRING terms, so an alternation
/// would have vanished from the generated documentation — a reader would see a `place-order`
/// success rule that never mentions the money-path append at all, which is worse than the hole the
/// alternation was added to close.
pub(crate) fn required_span_term(term: &Value, wrap: &dyn Fn(&str) -> String) -> String {
    match term {
        Value::String(s) => wrap(s),
        _ => term
            .get("any_of")
            .and_then(|x| x.as_sequence())
            .map(|alts| {
                let joined =
                    alts.iter().filter_map(|a| a.as_str()).map(&wrap).collect::<Vec<_>>().join(" | ");
                format!("({})", joined)
            })
            .unwrap_or_default(),
    }
}

/// Docs label for an operation's `roles:` — an omitted list means open to every role path
/// (literal roles, ADR-20260720-191500).
pub(crate) fn roles_label(roles: &[String]) -> String {
    if roles.is_empty() {
        "EVERYONE (open — roles omitted)".to_string()
    } else {
        roles.join(", ")
    }
}
pub(crate) fn user_emo(role: &str) -> &'static str {
    match role {
        "PUBLIC" => "🌐", "CUSTOMER" => "🙋", "RESTAURANT_ACCOUNT" => "🏪", "RESTAURANT" => "🍽️",
        "RIDER" => "🛵", "ADMIN" => "🛠️", "EXTERNAL" => "🔌", _ => "❔",
    }
}
pub(crate) fn dslug(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in s.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out
}
pub(crate) fn danchor(kind: &str, name: &str) -> String {
    format!("{}-{}", kind, dslug(name))
}
pub(crate) fn dprop_anchor(kind: &str, owner: &str, field: &str) -> String {
    format!("{}--{}", danchor(kind, owner), dslug(field))
}
pub(crate) fn id_tag(id: &str) -> String {
    format!("<a id=\"{}\"></a>", id)
}
pub(crate) fn dlink(kind: &str, name: &str) -> String {
    format!("[{} `{}`](#{})", d_emo(kind), name, danchor(kind, name))
}
pub(crate) fn dprop_link(kind: &str, owner: &str, field: &str) -> String {
    format!("[{} `{}`.`{}`](#{})", d_emo(kind), owner, field, dprop_anchor(kind, owner, field))
}
pub(crate) fn item_head(kind: &str, label: &str, name: &str) -> String {
    format!("{}\n#### {} {}: `{}`", id_tag(&danchor(kind, name)), d_emo(kind), label, name)
}
/// Collapse whitespace runs to a single space (no trim) — JS `.replace(/\s+/g,' ')`.
pub(crate) fn ws1(s: &str) -> String {
    let mut o = String::new();
    let mut p = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !p {
                o.push(' ');
                p = true;
            }
        } else {
            o.push(c);
            p = false;
        }
    }
    o
}
pub(crate) fn push_uniq(m: &mut HashMap<String, Vec<String>>, k: &str, v: &str) {
    let e = m.entry(k.to_string()).or_default();
    if !e.iter().any(|x| x == v) {
        e.push(v.to_string());
    }
}

pub(crate) fn ref_label(rf: &str) -> String {
    let mut it = rf.splitn(2, "#/");
    let file = it.next().unwrap_or("");
    let name = it.next().unwrap_or("");
    if file == "scalars.yaml" {
        dlink("scalar", name)
    } else {
        dlink("entity", name)
    }
}
pub(crate) fn raw_type(p: &Value) -> String {
    if let Some(rf) = p.get("$ref").and_then(|x| x.as_str()) {
        return ref_label(rf);
    }
    if p.get("type").and_then(|x| x.as_str()) == Some("array") {
        if let Some(items) = p.get("items") {
            return format!("[{}]", raw_type(items));
        }
    }
    let mut t = format!("`{}`", p.get("type").and_then(|x| x.as_str()).unwrap_or("?"));
    if let Some(en) = p.get("enum").and_then(|x| x.as_sequence()) {
        t += &format!(" ({})", en.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" \\| "));
    }
    if let Some(fmt) = p.get("format").and_then(|x| x.as_str()) {
        t += &format!(" _{}_", fmt);
    }
    t
}

pub(crate) fn doc_desc(model: &Model, file: &str, name: &str) -> String {
    let d = model.defs.get(file).and_then(|m| m.get(name)).and_then(|n| n.get("description")).and_then(|x| x.as_str()).unwrap_or("");
    ws1(d.trim())
}

pub(crate) struct Doc {
    pub(crate) ctx: String,
    pub(crate) md: String,
}
pub(crate) struct DRow {
    pub(crate) ctx: String,
    pub(crate) cells: Vec<String>,
}

pub(crate) fn emit_documentation(model: &Model) -> String {
    let api = parse_api(model);
    let actors = parse_actors(model);
    let views = parse_views(model);
    let personas = parse_stories(model);
    let cx = build_context_map(model, &api, &actors, &views);

    let scalar_set = scalar_names(model);
    let entity_set: HashSet<String> = model.defs.get("entities.yaml").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let type_set: HashSet<String> = api.types.iter().map(|t| t.name.clone()).collect();

    let api_type_md = |f: &ApiField| -> String {
        let base = if f.is_ref {
            if scalar_set.contains(&f.ty) {
                dlink("scalar", &f.ty)
            } else if type_set.contains(&f.ty) {
                dlink("type", &f.ty)
            } else if entity_set.contains(&f.ty) {
                dlink("entity", &f.ty)
            } else {
                format!("`{}`", f.ty)
            }
        } else {
            format!("`{}`{}", f.ty, f.format.as_deref().map(|fmt| format!(" _{}_", fmt)).unwrap_or_default())
        };
        if f.array {
            format!("[{}]", base)
        } else {
            base
        }
    };
    let prop_rows = |def: &Value, kind: &str, owner: &str| -> Vec<Vec<String>> {
        let props = match def.get("properties").and_then(|x| x.as_mapping()) {
            Some(m) => m,
            None => return vec![],
        };
        let required: HashSet<&str> = def.get("required").and_then(|x| x.as_sequence()).map(|s| s.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
        let mut rows = Vec::new();
        for (k, p) in props {
            let n = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            let req = if required.contains(n) { "✅" } else { "⬜" };
            let d = p.get("description").and_then(|x| x.as_str()).unwrap_or("");
            rows.push(vec![format!("{}`{}`", id_tag(&dprop_anchor(kind, owner, n)), n), raw_type(p), req.to_string(), ws1(d)]);
        }
        rows
    };

    // relationship indexes
    let mut cmd_handler: HashMap<String, (String, Vec<String>, Vec<String>)> = HashMap::new();
    let mut evt_emitted_by: HashMap<String, Vec<String>> = HashMap::new();
    let mut evt_consumed_by: HashMap<String, Vec<String>> = HashMap::new();
    let mut err_thrown_by: HashMap<String, Vec<String>> = HashMap::new();
    for a in &actors {
        for e in &a.receives {
            let msg = ref_name(&e.message_ref);
            let emits: Vec<String> = e.emits.iter().filter_map(|r| ref_name(r)).collect();
            let throws: Vec<String> = e.throws.iter().filter_map(|r| ref_name(r)).collect();
            if e.message_ref.starts_with("commands.yaml#/") {
                if let Some(m) = &msg {
                    cmd_handler.insert(m.clone(), (a.name.clone(), emits.clone(), throws.clone()));
                    for er in &throws {
                        push_uniq(&mut err_thrown_by, er, m);
                    }
                }
            } else if e.message_ref.starts_with("events.yaml#/") {
                if let Some(m) = &msg {
                    push_uniq(&mut evt_consumed_by, m, &a.name);
                }
            }
            for ev in &emits {
                push_uniq(&mut evt_emitted_by, ev, &a.name);
            }
        }
    }
    let mut evt_views: HashMap<String, Vec<String>> = HashMap::new();
    for v in &views {
        for e in &v.fedby {
            push_uniq(&mut evt_views, e, &v.name);
        }
    }
    let mut mut_by_command: HashMap<String, String> = HashMap::new();
    for m in &api.mutations {
        mut_by_command.insert(m.command.clone(), m.name.clone());
    }

    // 1. STORIES
    let stories_section = personas.iter().map(|p| {
        let badge = format!("{} `{}`{}", user_emo(&p.role), p.role, p.locale.as_deref().map(|l| format!(" · 🗣️ `{}`", l)).unwrap_or_default());
        let mut rows: Vec<Vec<String>> = Vec::new();
        for act in &p.activities {
            for (i, step) in act.steps.iter().enumerate() {
                let op = if let (Some(op), Some(kind)) = (&step.op, &step.op_kind) {
                    dlink(kind, op)
                } else if let Some(note) = &step.note {
                    format!("📝 {}", note)
                } else {
                    "—".to_string()
                };
                rows.push(vec![if i == 0 { format!("{} **{}**", d_emo("activity"), act.name) } else { String::new() }, step.name.clone(), op]);
            }
        }
        format!(
            "{}\n### {} `{}` · {}\n{}\n{}",
            id_tag(&danchor("story", &p.name)),
            d_emo("story"),
            p.name,
            badge,
            p.description.as_deref().map(|d| format!("\n{}\n", d)).unwrap_or_default(),
            md_table(&["Activity", "Step", "Operation"], &rows)
        )
    }).collect::<Vec<_>>().join("\n\n");

    // 2. API operations
    let mut api_docs: Vec<Doc> = Vec::new();
    for q in &api.queries {
        let field_list = q.args.iter().map(|a| format!("`{}{}`: {}", a.name, if a.required { "" } else { "?" }, api_type_md(a))).collect::<Vec<_>>().join(", ");
        let input = if q.args.is_empty() {
            "- **Input**: _(none)_".to_string()
        } else {
            format!("- **Input**: 🧩 `{}QueryInput{}` — {}", pascal(&q.name), if q.args.iter().any(|a| a.required) { "!" } else { "" }, field_list)
        };
        let ret = format!(
            "{}{}",
            if type_set.contains(&q.returns_type) || entity_set.contains(&q.returns_type) {
                dlink(if type_set.contains(&q.returns_type) { "type" } else { "entity" }, &q.returns_type)
            } else {
                format!("`{}`", q.returns_type)
            },
            if q.returns_list { " (list)" } else { "" }
        );
        let reads = if q.reads.is_empty() { "—".to_string() } else { q.reads.iter().map(|v| dlink("view", v)).collect::<Vec<_>>().join(", ") };
        let ctx = cx.of_operation(&q.roles, &(if !q.reads.is_empty() { cx.of_reads(&q.reads) } else { cx.of_type(&q.returns_type) }));
        api_docs.push(Doc { ctx, md: vec![
            item_head("query", "Query", &q.name),
            q.description.as_deref().map(|d| format!("\n{}\n", d)).unwrap_or_default(),
            input,
            format!("- **Returns**: {} · **reads** {}", ret, reads),
            format!("- **Roles**: {} · **slice** {}", roles_label(&q.roles), q.slice),
        ].join("\n") });
    }
    for m in &api.mutations {
        let handler = cmd_handler.get(&m.command);
        api_docs.push(Doc { ctx: cx.of_command(&m.command), md: vec![
            item_head("mutation", "Mutation", &m.name),
            format!("\n- **Command**: {}{}", dlink("command", &m.command), handler.map(|h| format!(" → handled by {}", dlink("actor", &h.0))).unwrap_or_default()),
            format!("- **Roles**: {} · **slice** {}", roles_label(&m.roles), m.slice),
            format!("- **Returns**: {} (acceptance-first — outcome via {})", dlink("type", "MutationAcceptance"), dlink("query", "operationStatus")),
        ].join("\n") });
    }
    for s in &api.subscriptions {
        let field_list = s.args.iter().map(|a| format!("`{}{}`: {}", a.name, if a.required { "" } else { "?" }, api_type_md(a))).collect::<Vec<_>>().join(", ");
        let input = if s.args.is_empty() {
            "- **Input**: _(none)_".to_string()
        } else {
            format!("- **Input**: 🧩 `{}SubscriptionInput{}` — {}", pascal(&s.name), if s.args.iter().any(|a| a.required) { "!" } else { "" }, field_list)
        };
        let ret = format!(
            "{}{}",
            if type_set.contains(&s.returns_type) || entity_set.contains(&s.returns_type) {
                dlink(if type_set.contains(&s.returns_type) { "type" } else { "entity" }, &s.returns_type)
            } else {
                format!("`{}`", s.returns_type)
            },
            if s.returns_list { " (list)" } else { "" }
        );
        api_docs.push(Doc { ctx: cx.of_operation(&s.roles, &cx.of_type(&s.returns_type)), md: vec![
            format!("{}\n#### {} Subscription: [`{}`](#{})", id_tag(&danchor("subscription", &s.name)), d_emo("subscription"), s.name, danchor("subscription", &s.name)),
            s.description.as_deref().map(|d| format!("\n{}\n", d)).unwrap_or_default(),
            input,
            format!("- **Streams**: {}", ret),
            format!("- **Roles**: {} · **slice** {}", roles_label(&s.roles), s.slice),
        ].join("\n") });
    }

    // typeDocs
    let type_docs: Vec<Doc> = api.types.iter().map(|t| {
        let reads = t.reads.iter().map(|v| dlink("view", v)).collect::<Vec<_>>().join(", ");
        let rows: Vec<Vec<String>> = t.properties.iter().map(|f| vec![format!("{}`{}`", id_tag(&dprop_anchor("type", &t.name, &f.name)), f.name), api_type_md(f), if f.nullable { "⬜".into() } else { "✅".into() }]).collect();
        Doc { ctx: cx.of_type(&t.name), md: vec![
            item_head("type", "Type", &t.name),
            t.description.as_deref().map(|d| format!("\n{}\n", d)).unwrap_or_default(),
            if reads.is_empty() { "- **Read model**: _(resolved within a parent projection)_".to_string() } else { format!("- **Read model**: {}", reads) },
            if rows.is_empty() { String::new() } else { format!("\n{}", md_table(&["Field", "Type", "Required"], &rows)) },
        ].join("\n") }
    }).collect();

    // actorDocs — process managers also embed their saga sequence diagram (typed steps); aggregates
    // with a declared `lifecycle` embed their state diagram (ADR-20260720-004419).
    let pm_seq: HashMap<String, String> = pm_sequence_map(model).into_iter().collect();
    let lc_state: HashMap<String, String> = lifecycle_state_map(model).into_iter().collect();
    let all_reminders = parse_reminders(model);
    let all_deletions = parse_deletions(model);
    let actor_docs: Vec<Doc> = actors.iter().map(|a| {
        let rows: Vec<Vec<String>> = a.receives.iter().map(|e| {
            // A reminder self-message (ADR-20260731-214500) has no cross-file anchor — render it
            // inline with its ⏰ marker; commands/events keep their links.
            let msg = match reminder_ref_parts(&e.message_ref) {
                Some((_, rname)) => format!("⏰ `{}` _(reminder)_", rname),
                None => {
                    let msg_name = ref_name(&e.message_ref).unwrap_or_else(|| "?".to_string());
                    let is_cmd = e.message_ref.starts_with("commands.yaml#/");
                    dlink(if is_cmd { "command" } else { "event" }, &msg_name)
                }
            };
            let emits = {
                let mut cells: Vec<String> = e.emits.iter().map(|r| dlink("event", &ref_name(r).unwrap_or_default())).collect();
                cells.extend(e.schedules.iter().filter_map(|r| reminder_ref_parts(r)).map(|(_, n)| format!("⏰ schedules `{}`", n)));
                let s = cells.join(", ");
                if s.is_empty() { e.effect.as_deref().map(|x| format!("_{}_", x)).unwrap_or_else(|| "—".to_string()) } else { s }
            };
            let throws = {
                let s = e.throws.iter().map(|r| dlink("error", &ref_name(r).unwrap_or_default())).collect::<Vec<_>>().join(", ");
                if s.is_empty() { "—".to_string() } else { s }
            };
            vec![msg, emits, throws]
        }).collect();
        let kind = if a.kind == "aggregate" { "🧩 aggregate" } else { "⚙️ process manager" };
        let mut parts = vec![
            item_head("actor", "Actor", &a.name),
            format!("\n_{}_{}\n", kind, a.description.as_deref().map(|d| format!(" — {}", d)).unwrap_or_default()),
            md_table(&["Receives", "Emits →", "Throws"], &rows),
        ];
        let rems: Vec<&ReminderDef> = all_reminders.iter().filter(|r| r.actor == a.name).collect();
        if !rems.is_empty() {
            let rrows: Vec<Vec<String>> = rems.iter().map(|r| vec![
                format!("⏰ `{}`", r.name),
                dlink("event", &ref_name(&r.payload_ref).unwrap_or_else(|| "?".to_string())),
                r.after_ref.as_deref().and_then(config_key_ref_name).map(|k| format!("⚙️ `{}`", k)).unwrap_or_else(|| "—".to_string()),
                r.reschedule.clone().unwrap_or_else(|| "in-place".to_string()),
            ]).collect();
            parts.push(format!("\nReminders (self-scheduled facts — ADR-20260731-214500):\n\n{}", md_table(&["Reminder", "Payload", "After", "Reschedule"], &rrows)));
        }
        if let Some(d) = all_deletions.iter().find(|d| d.actor == a.name) {
            let trows: Vec<Vec<String>> = d.triggers.iter().map(|t| {
                let on = { let s = t.on.iter().map(|r| dlink("event", &ref_name(r).unwrap_or_default())).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
                let window = t.after_ref.as_deref().and_then(config_key_ref_name).map(|k| format!("⚙️ `{}`", k)).unwrap_or_else(|| "_immediate (propagation)_".to_string());
                let cancelled = { let s = t.cancelled_on.iter().map(|r| dlink("event", &ref_name(r).unwrap_or_default())).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
                let m = match (t.match_event_ref.as_deref().and_then(lineage_parts), t.match_state_ref.as_deref().and_then(parse_ref)) {
                    (Some((ev, Some(prop))), Some(st)) => format!("{} ↔ `state.{}`", dprop_link("event", ev, prop), st.path.last().cloned().unwrap_or_default()),
                    _ => "—".to_string(),
                };
                vec![on, window, cancelled, m]
            }).collect();
            let receipt = d.receipt_ref.as_deref().and_then(ref_name).map(|e| dlink("event", &e)).unwrap_or_else(|| "—".to_string());
            parts.push(format!("\nDeletion (declarative, generic engine — ADR-20260731-214500):\n\n{}\n- **Receipt**: {}", md_table(&["On", "Window", "Cancelled on", "Match"], &trows), receipt));
        }
        if a.kind == "aggregate" {
            if let Some(d) = lc_state.get(&a.name) {
                parts.push(format!("\nLifecycle (generated from the declared state machine):\n\n```mermaid\n{}\n```", d));
            }
        } else if let Some(d) = pm_seq.get(&a.name) {
            parts.push(format!("\nSequence (generated from the typed steps):\n\n```mermaid\n{}\n```", d));
        }
        Doc { ctx: cx.of_actor(&a.name), md: parts.join("\n") }
    }).collect();

    // 4. VIEWS
    let view_docs: Vec<Doc> = views.iter().map(|v| {
        let slice = if v.slice == "V1" { "🔭 V1" } else { "🛶 V0" };
        let fed_by = { let s = v.fedby.iter().map(|n| dlink("event", n)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
        let cols: Vec<Vec<String>> = v.columns.iter().map(|c| {
            let flags = { let f: Vec<&str> = [(c.pk, "PK"), (c.unique, "unique"), (c.index, "index"), (c.nullable, "nullable")].iter().filter(|(b, _)| *b).map(|(_, s)| *s).collect(); if f.is_empty() { "—".to_string() } else { f.join(", ") } };
            let fk = c.fk.as_ref().map(|f| format!(" → {}", dlink("view", f.split('.').next().unwrap_or(f)))).unwrap_or_default();
            let type_cell = format!("{}{}", if scalar_set.contains(&c.ty) { dlink("scalar", &c.ty) } else { format!("`{}`", if c.ty.is_empty() { "?" } else { &c.ty }) }, if c.type_derived { " _(derived)_" } else { "" });
            let source = { let s = c.from.iter().map(|rf| { let segs: Vec<&str> = rf.splitn(2, "#/").nth(1).unwrap_or("").split('/').filter(|x| !x.is_empty()).collect(); let ev = segs.first().copied().unwrap_or(""); let prop = if segs.get(1) == Some(&"properties") { segs.get(2).copied() } else { None }; match prop { Some(p) => dprop_link("event", ev, p), None => dlink("event", ev) } }).collect::<Vec<_>>().join(", "); if s.is_empty() { "⚠️ _(none)_".to_string() } else { s } };
            vec![format!("`{}`", c.name), format!("{}{}", type_cell, fk), source, flags, ws1(c.note.as_deref().unwrap_or(""))]
        }).collect();
        Doc { ctx: cx.of_view(&v.name), md: [
            item_head("view", "View", &v.name),
            format!("\n- **Source**: {} · {}{}", if v.reference { "📦 reference (static seed)".to_string() } else { dlink("actor", &v.aggregate) }, slice, if v.internal { " · 🔒 internal" } else { "" }),
            v.note.as_deref().map(|n| format!("- **Note**: {}", ws1(n))).unwrap_or_default(),
            if v.filters.is_empty() { String::new() } else { format!("- **Filters**: {}", v.filters.join(" ")) },
            if v.rules.is_empty() { String::new() } else { format!("- **Rules**: {}", v.rules.join(" ")) },
            format!("- **Fed by**: {}", fed_by),
            format!("\n{}", md_table(&["Column", "Type", "Sourced from", "Constraints", "Notes"], &cols)),
        ].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n") }
    }).collect();

    let cmd_map = model.defs.get("commands.yaml").and_then(|v| v.as_mapping());
    let cmd_keys: Vec<String> = cmd_map.map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    // 5. COMMANDS (only those handled by an actor)
    let command_docs: Vec<Doc> = cmd_keys.iter().filter(|c| cmd_handler.contains_key(*c)).map(|c| {
        let h = cmd_handler.get(c).unwrap();
        let mutn = mut_by_command.get(c);
        let def = cmd_map.and_then(|m| m.get(c.as_str())).cloned().unwrap_or(Value::Null);
        let rows = prop_rows(&def, "command", c);
        Doc { ctx: cx.of_command(c), md: vec![
            item_head("command", "Command", c),
            { let d = doc_desc(model, "commands.yaml", c); if d.is_empty() { String::new() } else { format!("\n{}\n", d) } },
            format!("- **Dispatched by**: {} · **handled by** {}", mutn.map(|m| dlink("mutation", m)).unwrap_or_else(|| "—".to_string()), dlink("actor", &h.0)),
            format!("- **Emits**: {}", { let s = h.1.iter().map(|e| dlink("event", e)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } }),
            format!("- **Throws**: {}", { let s = h.2.iter().map(|e| dlink("error", e)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } }),
            if rows.is_empty() { String::new() } else { format!("\n{}", md_table(&["Field", "Type", "Required", "Description"], &rows)) },
        ].join("\n") }
    }).collect();

    // 6. EVENTS
    let non_projected: HashSet<String> = ref_names(model.defs.get("database/projection_views.yaml").and_then(|v| v.get("nonProjectedEvents"))).into_iter().collect();
    let evt_map = model.defs.get("events.yaml").and_then(|v| v.as_mapping());
    let event_docs: Vec<Doc> = evt_map.map(|m| m.iter().filter_map(|(k, _)| k.as_str()).map(|ev| {
        let def = evt_map.and_then(|m| m.get(ev)).cloned().unwrap_or(Value::Null);
        let rows = prop_rows(&def, "event", ev);
        let projected = { let s = evt_views.get(ev).map(|vs| vs.iter().map(|v| dlink("view", v)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if !s.is_empty() { s } else if non_projected.contains(ev) { "_non-projected (saga/transient)_".to_string() } else { "—".to_string() } };
        Doc { ctx: cx.of_event(ev), md: vec![
            item_head("event", "Event", ev),
            { let d = doc_desc(model, "events.yaml", ev); if d.is_empty() { String::new() } else { format!("\n{}\n", d) } },
            format!("- **Emitted by**: {}", { let s = evt_emitted_by.get(ev).map(|a| a.iter().map(|x| dlink("actor", x)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "_inbound / external_".to_string() } else { s } }),
            format!("- **Consumed by**: {}", { let s = evt_consumed_by.get(ev).map(|a| a.iter().map(|x| dlink("actor", x)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "—".to_string() } else { s } }),
            format!("- **Projected into**: {}", projected),
            if rows.is_empty() { String::new() } else { format!("\n{}", md_table(&["Field", "Type", "Required", "Description"], &rows)) },
        ].join("\n") }
    }).collect()).unwrap_or_default();

    // 7. ENTITIES
    let ent_map = model.defs.get("entities.yaml").and_then(|v| v.as_mapping());
    let entity_docs: Vec<Doc> = ent_map.map(|m| m.iter().filter_map(|(k, _)| k.as_str()).map(|e| {
        let def = ent_map.and_then(|m| m.get(e)).cloned().unwrap_or(Value::Null);
        let rows = prop_rows(&def, "entity", e);
        Doc { ctx: cx.of_entity(e), md: vec![
            item_head("entity", "Entity", e),
            { let d = doc_desc(model, "entities.yaml", e); if d.is_empty() { String::new() } else { format!("\n{}\n", d) } },
            if rows.is_empty() { "_(no fields)_".to_string() } else { md_table(&["Field", "Type", "Required", "Description"], &rows) },
        ].join("\n") }
    }).collect()).unwrap_or_default();

    // 8. SCALARS
    let scalar_rows: Vec<DRow> = model.defs.get("scalars.yaml").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, d)| k.as_str().map(|name| {
        let mut t = d.get("type").and_then(|x| x.as_str()).unwrap_or("?").to_string();
        if let Some(en) = d.get("enum").and_then(|x| x.as_sequence()) {
            t = format!("enum ({})", en.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" \\| "));
        } else if let Some(fmt) = d.get("format").and_then(|x| x.as_str()) {
            t += &format!(" _{}_", fmt);
        } else if let Some(pat) = d.get("pattern").and_then(|x| x.as_str()) {
            t += &format!(" `{}`", pat);
        }
        DRow { ctx: cx.of_scalar(name), cells: vec![format!("{}{} `{}`", id_tag(&danchor("scalar", name)), d_emo("scalar"), name), t, ws1(d.get("description").and_then(|x| x.as_str()).unwrap_or(""))] }
    })).collect()).unwrap_or_default();

    // 9. ERRORS
    let error_rows: Vec<DRow> = model.defs.get("errors.yaml").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, d)| k.as_str().map(|name| {
        let msgs = d.get("messages");
        let en = msgs.and_then(|x| x.get("en")).and_then(|x| x.as_str()).unwrap_or("");
        let fr = msgs.and_then(|x| x.get("fr")).and_then(|x| x.as_str()).unwrap_or("");
        let by = { let s = err_thrown_by.get(name).map(|c| c.iter().map(|x| dlink("command", x)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "—".to_string() } else { s } };
        DRow { ctx: cx.of_error(name), cells: vec![format!("{}{} `{}`", id_tag(&danchor("error", name)), d_emo("error"), name), ws1(d.get("description").and_then(|x| x.as_str()).unwrap_or("")), format!("🇬🇧 {}", en), format!("🇫🇷 {}", fr), by] }
    })).collect()).unwrap_or_default();

    // 10a/b. RULES ↔ TESTS
    let rule_defs = model.defs.get("rules.yaml").and_then(|v| v.as_mapping());
    let tests_map = model.defs.get("tests.yaml").and_then(|v| v.get("tests")).and_then(|v| v.as_mapping());
    let fixtures_map = model.defs.get("tests.yaml").and_then(|v| v.get("fixtures")).and_then(|v| v.as_mapping());
    let rules_of_test = |t: &Value| -> Vec<String> { t.get("rules").and_then(|x| x.as_sequence()).map(|s| s.iter().filter_map(|r| r.get("$ref").and_then(|x| x.as_str()).and_then(ref_name)).collect()).unwrap_or_default() };
    let mut rule_tests: HashMap<String, Vec<String>> = HashMap::new();
    let mut test_actor_name: HashMap<String, String> = HashMap::new();
    if let Some(tm) = tests_map {
        for (k, t) in tm {
            if let Some(tn) = k.as_str() {
                test_actor_name.insert(tn.to_string(), ref_name(t.get("actor").and_then(|a| a.get("$ref")).and_then(|x| x.as_str()).unwrap_or("")).unwrap_or_default());
                for rn in rules_of_test(t) {
                    let e = rule_tests.entry(rn).or_default();
                    if !e.contains(&tn.to_string()) { e.push(tn.to_string()); }
                }
            }
        }
    }
    let fx_event = |fx_ref: &str| -> Option<String> {
        let key = fx_ref.rsplit('/').next().unwrap_or("");
        fixtures_map.and_then(|m| m.get(key)).and_then(|fx| fx.get("type")).and_then(|t| t.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name)
    };
    let ev_links = |arr: Option<&Value>| -> String {
        arr.and_then(|x| x.as_sequence()).map(|s| s.iter().map(|it| it.get("$ref").and_then(|x| x.as_str()).and_then(|r| fx_event(r)).map(|e| dlink("event", &e)).unwrap_or_else(|| "—".to_string())).collect::<Vec<_>>().join(", ")).unwrap_or_default()
    };
    // testDocs — per actor
    let test_docs: Vec<Doc> = actors.iter().filter_map(|a| {
        let entries: Vec<(String, Value)> = tests_map.map(|m| m.iter().filter(|(_, t)| ref_name(t.get("actor").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("")).as_deref() == Some(a.name.as_str())).filter_map(|(k, t)| k.as_str().map(|s| (s.to_string(), t.clone()))).collect()).unwrap_or_default();
        if entries.is_empty() { return None; }
        let cases = entries.iter().map(|(name, t)| {
            // THE `when:` MESSAGE IS NOT ALWAYS A COMMAND. `$ref` names a KIND
            // (`commands.yaml#/X` vs `events.yaml#/X`), and 59 of this repo's tests are driven by
            // an INBOUND (integration) event -- an external fact that already happened, recorded
            // through the ACL with no command at all (CLAUDE.md, ADR-0004). This line rendered
            // every `when:` as a command, so those 59 got the command emoji AND a
            // `#command-<name>` anchor that no `<a id>` in this document ever defines: 95 dead
            // in-page links in the generated documentation, found by `tools/link-check.py` (#837).
            // Derived from the ref prefix, the same way the map data at the bottom of this file
            // already derives `isCommand`.
            let when_ref = t.get("when").and_then(|w| w.get("type")).and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("");
            let when_kind = if when_ref.starts_with("events.yaml#/") { "event" } else { "command" };
            let cmd = ref_name(when_ref).unwrap_or_else(|| "?".to_string());
            let given = { let g = t.get("given"); if g.and_then(|x| x.as_sequence()).map(|s| !s.is_empty()).unwrap_or(false) { ev_links(g) } else { "_(none)_".to_string() } };
            let has_thrown = t.get("thrown").is_some();
            let then_arr = t.get("then");
            let then_line = if has_thrown { String::new() } else { format!("- **Then**: {}", { let s = ev_links(then_arr); if then_arr.and_then(|x| x.as_sequence()).map(|s| !s.is_empty()).unwrap_or(false) { s } else { "∅ _no event (idempotent no-op)_".to_string() } }) };
            let thrown_line = if has_thrown { format!("- **Thrown**: {}", { let s = t.get("thrown").and_then(|x| x.as_sequence()).map(|arr| arr.iter().filter_map(|r| r.get("$ref").and_then(|x| x.as_str()).and_then(ref_name)).map(|e| dlink("error", &e)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "—".to_string() } else { s } }) } else { String::new() };
            let rules = rules_of_test(t).iter().map(|rn| dlink("rule", rn)).collect::<Vec<_>>().join(", ");
            vec![
                format!("{}\n#### {} Test: `{}`", id_tag(&danchor("test", name)), d_emo("test"), name),
                t.get("name").and_then(|x| x.as_str()).map(|n| format!("\n_{}_\n", n)).unwrap_or_default(),
                format!("- **Given**: {}", given),
                format!("- **When**: {}", dlink(when_kind, &cmd)),
                then_line,
                thrown_line,
                if rules.is_empty() { String::new() } else { format!("- **Verifies**: {}", rules) },
            ].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n")
        }).collect::<Vec<_>>().join("\n\n");
        Some(Doc { ctx: cx.of_actor(&a.name), md: format!("**{}**\n\n{}", dlink("actor", &a.name), cases) })
    }).collect();

    let rule_docs: Vec<Doc> = rule_defs.map(|m| m.iter().filter_map(|(k, r)| k.as_str().map(|name| {
        let tns = rule_tests.get(name).cloned().unwrap_or_default();
        let ctx = tns.first().map(|tn| cx.of_actor(test_actor_name.get(tn).map(|s| s.as_str()).unwrap_or(""))).unwrap_or_else(|| CROSS.to_string());
        let verified_by = { let s = tns.iter().map(|tn| dlink("test", tn)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
        Doc { ctx, md: vec![
            format!("{}\n#### {} Rule: `{}`", id_tag(&danchor("rule", name)), d_emo("rule"), name),
            r.get("description").and_then(|x| x.as_str()).map(|d| format!("\n_{}_\n", ws1(d.trim()))).unwrap_or_default(),
            format!("- **Verified by**: {}", verified_by),
        ].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n") }
    })).collect()).unwrap_or_default();

    // 10. OBSERVABILITY
    /// `$ref` -> a documentation link, with the KIND taken from the ref's FILE.
    ///
    /// `processmanager.yaml` maps to `actor`, not to the `_ => "entity"` default, because that
    /// is the section the emitter actually writes for a process manager
    /// (`<a id="actor-placeorderprocess">`). Under the default it produced
    /// `#entity-placeorderprocess`, an anchor no `<a id>` in the document ever defines -- dead
    /// in-page links for every saga in `observability.yaml`, rendered by GitHub with no error.
    /// Found by `tools/link-check.py` (#837).
    fn any_link(rf: &str) -> String {
        let mut it = rf.splitn(2, "#/");
        let file = it.next().unwrap_or("");
        let name = it.next().unwrap_or("");
        let kind = match file { "commands.yaml" => "command", "events.yaml" => "event", "actors.yaml" => "actor", "processmanager.yaml" => "actor", "database/projection_views.yaml" => "view", "database/tables/projection_tables.yaml" => "view", "database/tables/referential.yaml" => "view", "scalars.yaml" => "scalar", _ => "entity" };
        dlink(kind, name)
    }
    fn ref_list_links(v: Option<&Value>) -> String {
        let s = v.and_then(|x| x.as_sequence()).map(|arr| arr.iter().filter_map(|it| it.get("$ref").and_then(|r| r.as_str())).map(any_link).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        if s.is_empty() { "—".to_string() } else { s }
    }
    let obs_docs: Vec<Doc> = model.defs.get("observability.yaml").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, c)| k.as_str().map(|feature| {
        let wf = c.get("workflow");
        let id_rows: Vec<Vec<String>> = c.get("run_identity").and_then(|x| x.as_sequence()).map(|s| s.iter().map(|i| vec![format!("`{}`", i.get("name").and_then(|x| x.as_str()).unwrap_or("")), format!("`{}`", i.get("source").and_then(|x| x.as_str()).unwrap_or("")), if i.get("required").and_then(|x| x.as_bool()) == Some(true) { "✅".into() } else { "⬜".into() }, i.get("businessKey").and_then(|b| b.get("$ref")).and_then(|x| x.as_str()).map(any_link).unwrap_or_else(|| "—".to_string())]).collect()).unwrap_or_default();
        let span_rows: Vec<Vec<String>> = c.get("spans").and_then(|x| x.as_sequence()).map(|s| s.iter().map(|sp| { let a = sp.get("attributes").and_then(|x| x.as_sequence()).map(|at| at.iter().map(|x| format!("`{}`{}", x.get("key").and_then(|k| k.as_str()).unwrap_or(""), if x.get("required").and_then(|r| r.as_bool()) == Some(true) { "*" } else { "" })).collect::<Vec<_>>().join(", ")).unwrap_or_default(); let a = if a.is_empty() { "—".to_string() } else { a }; vec![format!("`{}`", sp.get("name").and_then(|x| x.as_str()).unwrap_or("")), format!("`{}`", sp.get("kind").and_then(|x| x.as_str()).unwrap_or("")), if sp.get("required").and_then(|x| x.as_bool()) == Some(true) { "✅".into() } else { "⬜".into() }, sp.get("multiplicity").and_then(|x| x.as_str()).map(|mu| format!("`{}`", mu)).unwrap_or_else(|| "—".to_string()), a] }).collect()).unwrap_or_default();
        let metric_list = |key: &str| -> String { let s = c.get(key).and_then(|x| x.as_sequence()).map(|arr| arr.iter().map(|m| format!("`{}` _({})_", m.get("name").and_then(|x| x.as_str()).unwrap_or(""), m.get("type").and_then(|x| x.as_str()).unwrap_or(""))).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "—".to_string() } else { s } };
        let sr_success = c.get("status_rules").and_then(|x| x.get("success"));
        let success = sr_success.map(|s| format!("success ⇐ spans [{}]", s.get("required_spans").and_then(|x| x.as_sequence()).map(|a| a.iter().map(|t| required_span_term(t, &|s| format!("`{}`", s))).collect::<Vec<_>>().join(", ")).unwrap_or_default())).unwrap_or_default();
        let lat = c.get("latency_budget");
        let err = c.get("error_budget");
        let cmd = ref_name(wf.and_then(|w| w.get("command")).and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or(""));
        let saga = ref_name(wf.and_then(|w| w.get("saga")).and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or(""));
        let ctx = if let Some(c) = &cmd { cx.of_command(c) } else if let Some(s) = &saga { cx.of_actor(s) } else { CROSS.to_string() };
        let s3 = |v: Option<&Value>, k: &str| v.and_then(|x| x.get(k)).map(|x| if let Some(n) = x.as_i64() { n.to_string() } else if let Some(f) = x.as_f64() { f.to_string() } else { x.as_str().unwrap_or("—").to_string() }).unwrap_or_else(|| "—".to_string());
        Doc { ctx, md: vec![
            format!("{}\n#### {} Contract: `{}`", id_tag(&danchor("obs", feature)), d_emo("obs"), feature),
            format!("\n_criticality: **{}**_\n", c.get("criticality").and_then(|x| x.as_str()).unwrap_or("—")),
            format!("- **Workflow**: {}{}{}", wf.and_then(|w| w.get("surface")).and_then(|s| s.as_str()).map(|s| format!("surface `{}` (dispatch pipeline)", s)).unwrap_or_default(), wf.and_then(|w| w.get("saga")).map(|s| format!("saga {}", any_link(s.get("$ref").and_then(|x| x.as_str()).unwrap_or_default()))).unwrap_or_default(), wf.and_then(|w| w.get("command")).map(|c| format!(" · command {}", any_link(c.get("$ref").and_then(|x| x.as_str()).unwrap_or_default()))).unwrap_or_default()),
            format!("- **Emits**: {} · **Inbound**: {}", ref_list_links(wf.and_then(|w| w.get("emits"))), ref_list_links(wf.and_then(|w| w.get("inbound")))),
            if id_rows.is_empty() { String::new() } else { format!("\n**Run identity**\n\n{}", md_table(&["Id", "Source", "Req.", "Business key"], &id_rows)) },
            if span_rows.is_empty() { String::new() } else { format!("\n**Spans** (`*` = required attribute)\n\n{}", md_table(&["Span", "Kind", "Req.", "Multiplicity", "Attributes"], &span_rows)) },
            format!("\n- **Metrics**: {} · **Business metrics**: {}", metric_list("metrics"), metric_list("business_metrics")),
            if success.is_empty() { String::new() } else { format!("- **Status rules**: {}", success) },
            format!("- **SLOs**: p95 ≤ {}ms · p99 ≤ {}ms · error rate ≤ {}%", s3(lat, "max_p95_ms"), s3(lat, "max_p99_ms"), s3(err, "max_error_rate_pct")),
        ].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n") }
    })).collect()).unwrap_or_default();

    // C4 doc
    let c4_doc = {
        let l2 = model.defs.get("architecture/c4-l2.yaml");
        let l3 = model.defs.get("architecture/c4-l3.yaml");
        let sysn = l2.and_then(|v| v.get("system")).and_then(|s| s.get("name")).and_then(|x| x.as_str()).unwrap_or("Captain.Food");
        let sysd = l2.and_then(|v| v.get("system")).and_then(|s| s.get("description")).and_then(|x| x.as_str()).unwrap_or("");
        let map_rows = |sect: &str, f: &dyn Fn(&str, &Value) -> Vec<String>| -> Vec<Vec<String>> { l2.and_then(|v| v.get(sect)).and_then(|x| x.as_mapping()).map(|m| m.iter().filter_map(|(k, v)| k.as_str().map(|n| f(n, v))).collect()).unwrap_or_default() };
        let bc_rows = map_rows("boundedContexts", &|n, bc| vec![format!("{} `{}`", d_emo("context"), n), bc.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string(), format!("{}{}", ref_list_links(bc.get("aggregates")), if bc.get("processManagers").is_some() { format!(" · {}", ref_list_links(bc.get("processManagers"))) } else { String::new() })]);
        let c_rows = map_rows("containers", &|n, c| vec![format!("{} `{}`", d_emo("container"), n), c.get("technology").and_then(|x| x.as_str()).unwrap_or("").to_string(), format!("{}{}", c.get("description").and_then(|x| x.as_str()).unwrap_or(""), if c.get("realizes").is_some() { format!("<br>realizes: {}", ref_list_links(c.get("realizes"))) } else { String::new() })]);
        let x_rows = map_rows("externalSystems", &|n, x| vec![format!("🔌 `{}`", n), x.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string()]);
        let rel_rows: Vec<Vec<String>> = l2.and_then(|v| v.get("relationships")).and_then(|x| x.as_sequence()).map(|s| s.iter().map(|r| vec![format!("`{}` → `{}`", r.get("from").and_then(|x| x.as_str()).unwrap_or(""), r.get("to").and_then(|x| x.as_str()).unwrap_or("")), r.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string()]).collect()).unwrap_or_default();
        let comp_rows: Vec<Vec<String>> = l3.and_then(|v| v.get("components")).and_then(|x| x.as_mapping()).map(|m| m.iter().filter_map(|(k, c)| k.as_str().map(|n| { let mut binds: Vec<String> = Vec::new(); if c.get("handles").is_some() { binds.push(format!("handles {}", ref_list_links(c.get("handles")))); } if c.get("updates").is_some() { binds.push(format!("updates {}", ref_list_links(c.get("updates")))); } if c.get("reads").is_some() { binds.push(format!("reads {}", ref_list_links(c.get("reads")))); } let bind = if binds.is_empty() { "—".to_string() } else { binds.join("<br>") }; vec![format!("{} `{}`", d_emo("component"), n), if c.get("instrumented").and_then(|x| x.as_bool()) == Some(true) { "📡 yes".into() } else { "— no".into() }, c.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string(), bind] })).collect()).unwrap_or_default();
        [
            format!("**System**: `{}` — {}", sysn, sysd),
            format!("\n### 🔲 L2 — Bounded contexts\n\n{}", md_table(&["Context", "Description", "Aggregates / process managers"], &bc_rows)),
            format!("\n### 🧱 L2 — Containers\n\n{}", md_table(&["Container", "Technology", "Description"], &c_rows)),
            format!("\n### 🔌 L2 — External systems\n\n{}", md_table(&["System", "Description"], &x_rows)),
            format!("\n### ➡️ L2 — Relationships\n\n{}", md_table(&["Edge", "Description"], &rel_rows)),
            format!("\n### ⚙️ L3 — Components of the `api` container\n\n{}", md_table(&["Component", "Instrumented", "Description", "Binds"], &comp_rows)),
        ].join("\n")
    };

    // SDUI screens + translations (reuse the C4/HTML approach). Generic over every screens/*.yaml
    // surface (ADR-20260722-091500): each surface renders its own screens block under a header, so a new
    // audience appears in the docs automatically. tr_en/op_cell/boxf/collect_action_types are
    // surface-independent; resolvers/actions/screens are read per surface inside the loop.
    let screens_files: Vec<String> = model.defs.keys().filter(|k| k.starts_with("screens/")).cloned().collect();
    // translations merged from translations.yaml + screens/*.translations.yaml (translation_entries)
    let cellf = |s: &str| s.replace('|', "\\|");
    let tr_en = |rf: &str| -> String { resolve_ref(model, rf, "translations.yaml").and_then(|t| t.get("messages")).and_then(|m| m.get("en")).and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| rf.rsplit('/').next().unwrap_or(rf).to_string()) };
    let t_text = |v: &Value| -> String { if let Some(rf) = v.get("$ref").and_then(|x| x.as_str()) { tr_en(rf) } else if let Some(s) = v.as_str() { s.to_string() } else { String::new() } };
    let tr_rows: Vec<Vec<String>> = translation_entries(model).into_iter().map(|(_f, key, t)| { let params = t.get("params").and_then(|x| x.as_mapping()).map(|pm| pm.iter().filter_map(|(pk, _)| pk.as_str().map(|p| format!("`{}`", p))).collect::<Vec<_>>().join(", ")).unwrap_or_default(); let params = if params.is_empty() { "—".to_string() } else { params }; vec![format!("{}`{}`", id_tag(&danchor("translation", &key)), key), params, cellf(t.get("messages").and_then(|mm| mm.get("en")).and_then(|x| x.as_str()).unwrap_or("")), cellf(t.get("messages").and_then(|mm| mm.get("fr")).and_then(|x| x.as_str()).unwrap_or(""))] }).collect();
    let translations_section = md_table(&["Key", "Params", "🇬🇧 en", "🇫🇷 fr"], &tr_rows);
    let op_cell = |rf: Option<&str>, gap: Option<&str>| -> String { if let Some(g) = gap { return format!("⚠️ _gap: {}_", cellf(g)); } match rf { None => "—".to_string(), Some(rf) => { let name = rf.rsplit('/').next().unwrap_or(""); let kind = if rf.contains("/mutations/") { "mutation" } else if rf.contains("/subscriptions/") { "subscription" } else { "query" }; dlink(kind, name) } } };
    fn collect_action_types(node: &Value, keys: &HashSet<String>, acc: &mut Vec<String>) {
        match node {
            Value::Sequence(s) => s.iter().for_each(|n| collect_action_types(n, keys, acc)),
            Value::Mapping(m) => {
                if let Some(t) = m.get(Value::String("type".into())).and_then(|x| x.as_str()) { if keys.contains(t) && !acc.contains(&t.to_string()) { acc.push(t.to_string()); } }
                for (_, v) in m { collect_action_types(v, keys, acc); }
            }
            _ => {}
        }
    }
    let boxf = |w: usize, s: &str| -> String { let n = s.chars().count(); let inner = if n > w { let t: String = s.chars().take(w - 1).collect(); format!("{}…", t) } else { format!("{}{}", s, " ".repeat(w - n)) }; format!("│ {} │", inner) };
    let mut surface_blocks: Vec<String> = Vec::new();
    for sfkey in &screens_files {
        let sf = model.defs.get(sfkey);
        let resolvers = sf.and_then(|v| v.get("resolvers")).and_then(|v| v.as_mapping());
        let action_defs = sf.and_then(|v| v.get("actions")).and_then(|v| v.as_mapping());
        let action_keys: HashSet<String> = action_defs.map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
        let screens_arr = sf.and_then(|v| v.get("screens")).and_then(|x| x.as_sequence()).cloned().unwrap_or_default();
        let screen_docs: Vec<String> = screens_arr.iter().map(|s| {
            let id = s.get("id").and_then(|x| x.as_str()).unwrap_or("?");
            let route = s.get("route").and_then(|x| x.as_str()).unwrap_or("");
            let title = { let t = s.get("title").map(|v| t_text(v)).unwrap_or_default(); if t.is_empty() { id.to_string() } else { t } };
            let sdui_badge = if s.get("sdui").and_then(|x| x.as_bool()) == Some(false) { format!("🚫 not SDUI{}", s.get("sdui_reason").and_then(|x| x.as_str()).map(|r| format!(" — {}", r)).unwrap_or_default()) } else { "📱 SDUI".to_string() };
            let auth = if s.get("requires_auth").and_then(|x| x.as_bool()) == Some(true) { " · 🔒 auth" } else { "" };
            let mut rows: Vec<Vec<String>> = Vec::new();
            for rn in s.get("data_requirements").and_then(|x| x.as_sequence()).map(|s| s.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect::<Vec<_>>()).unwrap_or_default() {
                let r = resolvers.and_then(|m| m.get(rn.as_str()));
                rows.push(vec!["read".to_string(), format!("`{}`", rn), op_cell(r.and_then(|x| x.get("query")).and_then(|q| q.get("$ref")).and_then(|x| x.as_str()), r.and_then(|x| x.get("gap")).and_then(|x| x.as_str()))]);
            }
            let mut acts: Vec<String> = Vec::new();
            if let Some(comps) = s.get("components") { collect_action_types(comps, &action_keys, &mut acts); }
            for a in s.get("actions_used").and_then(|x| x.as_sequence()).map(|s| s.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect::<Vec<_>>()).unwrap_or_default() { if !acts.contains(&a) { acts.push(a); } }
            for a in &acts {
                let ad = action_defs.and_then(|m| m.get(a.as_str()));
                if ad.map(|x| x.get("mutation").is_some() || x.get("gap").is_some()).unwrap_or(false) {
                    rows.push(vec!["write".to_string(), format!("`{}`", a), op_cell(ad.and_then(|x| x.get("mutation")).and_then(|q| q.get("$ref")).and_then(|x| x.as_str()), ad.and_then(|x| x.get("gap")).and_then(|x| x.as_str()))]);
                }
            }
            let ops_table = md_table(&["Kind", "UI need", "GraphQL operation"], &rows);
            let mut mock_lines: Vec<String> = Vec::new();
            if let Some(comps) = s.get("components").and_then(|x| x.as_sequence()) {
                for c in comps {
                    let t = if let Some(cp) = c.get("component").and_then(|x| x.as_str()) { format!("«{}»", cp) } else { c.get("type").and_then(|x| x.as_str()).unwrap_or("?").to_string() };
                    let lbl = { let l = c.get("title").map(|v| t_text(v)).filter(|s| !s.is_empty()).or_else(|| c.get("label").map(|v| t_text(v)).filter(|s| !s.is_empty())).or_else(|| c.get("placeholder").map(|v| t_text(v)).filter(|s| !s.is_empty())).unwrap_or_default(); l };
                    mock_lines.push(boxf(40, &format!("{}{}", t, if lbl.is_empty() { String::new() } else { format!(" — {}", lbl) })));
                }
            }
            let mut mock = vec![format!("┌{}┐", "─".repeat(42)), boxf(40, &title), format!("├{}┤", "─".repeat(42))];
            mock.extend(mock_lines);
            mock.push(format!("└{}┘", "─".repeat(42)));
            let gaps = s.get("gaps").and_then(|x| x.as_sequence()).map(|g| g.iter().filter_map(|x| x.as_str()).map(|g| format!("- ⚠️ {}", g)).collect::<Vec<_>>().join("\n")).unwrap_or_default();
            format!("{}\n### {} `{}` · `{}` · {}{}\n\n```\n{}\n```\n\n{}{}", id_tag(&danchor("screen", id)), d_emo("screen"), id, route, sdui_badge, auth, mock.join("\n"), ops_table, if gaps.is_empty() { String::new() } else { format!("\n\n**Gaps**\n{}", gaps) })
        }).collect();
        let surface = sfkey.strip_prefix("screens/").unwrap_or(sfkey);
        surface_blocks.push(format!("_Surface_ **`{}`**\n\n{}", surface, screen_docs.join("\n\n")));
    }
    let screens_section = surface_blocks.join("\n\n");

    // Assembly
    let sec = |id: &str, emoji: &str, title: &str| format!("{}\n## {} {}", id_tag(&format!("sec-{}", id)), emoji, title);
    let in_ctx = |docs: &[Doc], ctx: &str| -> Vec<String> { docs.iter().filter(|d| d.ctx == ctx).map(|d| d.md.clone()).collect() };
    let kind_sub = |emoji: &str, title: &str, bodies: Vec<String>| -> String { if bodies.is_empty() { String::new() } else { format!("### {} {} _({})_\n\n{}", emoji, title, bodies.len(), bodies.join("\n\n")) } };
    let doc_sub = |emoji: &str, title: &str, docs: &[Doc], ctx: &str| kind_sub(emoji, title, in_ctx(docs, ctx));
    let row_sub = |emoji: &str, title: &str, head: &[&str], rows: &[DRow], ctx: &str| -> String { let r: Vec<&DRow> = rows.iter().filter(|x| x.ctx == ctx).collect(); if r.is_empty() { String::new() } else { format!("### {} {} _({})_\n\n{}", emoji, title, r.len(), md_table(head, &r.iter().map(|x| x.cells.clone()).collect::<Vec<_>>())) } };
    let mut ctx_blocks: Vec<(String, Vec<String>)> = Vec::new();
    for ctx in &cx.order {
        let parts: Vec<String> = [
            doc_sub("🧰", "API operations", &api_docs, ctx),
            doc_sub(d_emo("type"), "Output types", &type_docs, ctx),
            doc_sub(d_emo("actor"), "Actors", &actor_docs, ctx),
            doc_sub(d_emo("view"), "Views (read models)", &view_docs, ctx),
            doc_sub(d_emo("command"), "Commands", &command_docs, ctx),
            doc_sub(d_emo("event"), "Events", &event_docs, ctx),
            doc_sub(d_emo("entity"), "Entities", &entity_docs, ctx),
            row_sub(d_emo("scalar"), "Scalars", &["Scalar", "Type", "Description"], &scalar_rows, ctx),
            row_sub(d_emo("error"), "Errors", &["Error", "Description", "Message (en)", "Message (fr)", "Thrown by"], &error_rows, ctx),
            doc_sub(d_emo("rule"), "Business rules", &rule_docs, ctx),
            doc_sub(d_emo("test"), "Tests", &test_docs, ctx),
            doc_sub(d_emo("obs"), "Observability", &obs_docs, ctx),
        ].into_iter().filter(|s| !s.is_empty()).collect();
        if !parts.is_empty() {
            ctx_blocks.push((ctx.clone(), parts));
        }
    }
    let ctx_sections = ctx_blocks.iter().enumerate().map(|(i, (ctx, parts))| {
        let d = cx.describe(ctx);
        format!("{}\n## {} {}. {}\n\n{}{}", id_tag(&format!("sec-ctx-{}", dslug(ctx))), d_emo("context"), i + 1, ctx, if d.is_empty() { String::new() } else { format!("_{}_\n\n", d) }, parts.join("\n\n"))
    }).collect::<Vec<_>>().join("\n\n");
    let ctx_toc = ctx_blocks.iter().map(|(ctx, _)| format!("[{} {}](#sec-ctx-{})", d_emo("context"), ctx, dslug(ctx))).collect::<Vec<_>>().join(" · ");

    let md = format!(
        "<!-- GENERATED by tools/codegen — do not edit by hand. Source: specs/*.yaml. -->\n# 📖 Captain.Food — Product Documentation (generated)\n\nA single, navigable view of the whole product, built from the specs and organized **top-level by\nbounded context** (🔲). Within each context: its API operations, output types, actors, views, commands,\nevents, entities, scalars, errors, business rules (📐 — what we guarantee), tests (🧪 — how it's verified,\ncross-linked to the rules) and observability contracts. Every item — and every\n**property** 🔹 — is anchored and **cross-linked**; `cross-cutting` holds the shared vocabulary and ops\nthat belong to no single context. Stories and Architecture span all contexts.\n\n**Kinds**: {q} query · {mu} mutation · {su} subscription · {ty} type · {ac} actor · {vi} view · {cm} command · {ev} event · {en} entity · {sc} scalar · {er} error · {pr} property\n**Roles**: 🌐 PUBLIC · 🙋 CUSTOMER · 🏪 RESTAURANT_ACCOUNT · 🍽️ RESTAURANT · 🛵 RIDER · 🛠️ ADMIN · 🔌 EXTERNAL\n**Markers**: ✅ required · ⬜ optional · 🛶 V0 · 🔭 V1 · 🔒 internal · ⚠️ design hole\n\n**Contents** — [🎬 Stories](#sec-stories) · {toc} · [📱 Screens](#sec-screens) · [🌐 Translations](#sec-translations) · [🏛️ Architecture](#sec-architecture)\n\n{s_stories}\n\nHow each persona uses the API. `personaRole` is the persona's GraphQL path-role (UserType).\n\n{stories}\n\n{ctxs}\n\n{s_screens}\n\nServer-Driven UI screens (`specs/screens/*.yaml`, one file per audience, ADR-0033/ADR-20260722-091500).\nEach screen's **reads** (resolvers →\nqueries) and **writes** (actions → mutations) are `$ref`-bound to the GraphQL API and validated, so the\nmockups below are the **proof the API answers the UI**. ⚠️ gaps mark UI needs the API does not serve yet.\nScreens marked 🚫 are intentionally not SDUI-rendered (Stripe/subscription/auth integrity).\n\n{screens}\n\n{s_trans}\n\nThe i18n catalog (`specs/translations.yaml`) — every user-visible screen string, referenced by `$ref` and\ngenerated to a single `translations.generated.json`. `{{param}}` tokens are validated against `params`.\n\n{trans}\n\n{s_arch}\n\nC4 views as source-managed DSL (`specs/architecture/c4-l{{2,3}}.yaml`). Bounded contexts bind their\naggregates; components bind the aggregates they handle, the read models they update, and (`reads`) the\nread models they CONSUME outside GraphQL -- every read model must have a declared reader (#305).\n\n{c4}\n",
        q = d_emo("query"), mu = d_emo("mutation"), su = d_emo("subscription"), ty = d_emo("type"), ac = d_emo("actor"), vi = d_emo("view"), cm = d_emo("command"), ev = d_emo("event"), en = d_emo("entity"), sc = d_emo("scalar"), er = d_emo("error"), pr = d_emo("property"),
        toc = ctx_toc,
        s_stories = sec("stories", "🎬", "Stories"),
        stories = stories_section,
        ctxs = ctx_sections,
        s_screens = sec("screens", "📱", "Front-office screens (SDUI)"),
        screens = screens_section,
        s_trans = sec("translations", "🌐", "Translations"),
        trans = translations_section,
        s_arch = sec("architecture", "🏛️", "Architecture (C4)"),
        c4 = c4_doc,
    );
    delink_dangling_anchors(md)
}

/// Render `[label](#anchor)` as plain `label` wherever this document defines no `#anchor`.
///
/// THE CLASS, NOT THE FOUR INSTANCES. Every in-page link here is built as
/// `dlink(kind, name)` -> `#{kind}-{slug}`, while the SECTION that would define the anchor is
/// written by a different arm of this emitter. The two agree only by convention, and #837 found
/// 102 places where they had drifted apart: a `when:` that is an inbound event linked as a
/// command, a saga linked as an entity, and -- the residue this function exists for -- links to
/// things the document does not document AT ALL (referential tables such as `PricingPolicy`, and
/// `CartLine`). The first two were kind bugs and are fixed at their source above. The third is
/// not a bug in the LINK, it is a gap in the CONTENT, and closing it means deciding what
/// sections the generated documentation should grow -- a separate question from this one.
///
/// So the invariant is established here instead, once, for every arm at once: this document
/// cannot contain a dead in-page link. A missing section degrades to plain text -- which is what
/// GitHub shows the reader anyway, except that a real link would have promised a destination it
/// does not have. The degradation is NOT silent: it lands in `specs/generated/**`, which
/// `check-drift` forces into the same commit, so a link turning into text is visible in review.
///
/// Only `](#...)` is touched: a fragment on a PATH (`[x](../adr/y.md#z)`) points into another
/// file, whose anchors are `tools/link-check.py`'s business, not this function's.
fn delink_dangling_anchors(md: String) -> String {
    let defined: std::collections::HashSet<&str> = md
        .match_indices("<a id=\"")
        .filter_map(|(i, _)| {
            let rest = &md[i + 7..];
            rest.find('"').map(|e| &rest[..e])
        })
        .collect();

    let mut out = String::with_capacity(md.len());
    let mut rest = md.as_str();
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        rest = &rest[open..];
        // `[label](#anchor)` with no `[`/`]` inside the label -- every `dlink` output is this shape.
        let parsed = rest[1..].find(']').and_then(|lc| {
            let label = &rest[1..1 + lc];
            let after = &rest[1 + lc + 1..];
            if label.contains('[') || !after.starts_with("(#") {
                return None;
            }
            after[2..].find(')').map(|pc| (label, &after[2..2 + pc], 1 + lc + 1 + 2 + pc + 1))
        });
        match parsed {
            Some((label, anchor, consumed)) if !defined.contains(anchor) => {
                out.push_str(label);
                rest = &rest[consumed..];
            }
            _ => {
                out.push('[');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

// ─── documentation.generated.html (port of emit/documentation-html.ts) ──────────────────────────

pub(crate) const THEME: &str = r##"<style>
  :root {
    --bg:#2b2b2b; --bg2:#313335; --bg3:#3c3f41; --fg:#a9b7c6; --muted:#808080; --line:#4b4b4b;
    --type:#4ec9b0; --scalar:#4fc1ff; --op:#dcdcaa; --event:#c586c0; --error:#f44747;
    --prop:#9cdcfe; --param:#d7ba7d; --const:#b5cea8; --kw:#cc7832; --accent:#ffc66d;
  }
  * { box-sizing:border-box; }
  body { margin:0; background:#2b2b2b; }
  .doc { background:var(--bg); color:var(--fg); font:14px/1.55 "JetBrains Mono","SFMono-Regular",Consolas,"Liberation Mono",monospace; padding:0 0 40vh; }
  .doc .wrap { max-width:1100px; margin:0 auto; padding:24px 20px; }
  .doc h1 { color:#fff; font-size:24px; border-bottom:2px solid var(--line); padding-bottom:10px; }
  .doc h3 { color:var(--accent); margin:18px 0 6px; }
  .doc a { color:var(--prop); text-decoration:none; }
  .doc a:hover { text-decoration:underline; }
  .doc code, .doc .id { font-family:inherit; }
  .k-type { color:var(--type); } .k-scalar { color:var(--scalar); } .k-op { color:var(--op); }
  .k-event { color:var(--event); } .k-error { color:var(--error); } .k-prop { color:var(--prop); }
  .k-param { color:var(--param); } .k-const { color:var(--const); } .k-id { color:var(--fg); }
  .kw { color:var(--kw); } .muted { color:var(--muted); } .req { color:var(--const); } .opt { color:var(--muted); }
  /* collapsible sections + items */
  details.sec { border:1px solid var(--line); border-radius:6px; margin:14px 0; background:var(--bg2); }
  details.sec > summary { cursor:pointer; padding:12px 16px; font-size:18px; color:#fff; list-style:none; background:var(--bg2); border-radius:6px; }
  details.sec[open] > summary { border-bottom:1px solid var(--line); border-radius:6px 6px 0 0; }
  details.sec > .body { padding:8px 16px 16px; }
  details.subsec { border:1px solid var(--line); border-radius:6px; margin:10px 0; background:var(--bg); }
  details.subsec > summary { cursor:pointer; padding:8px 12px; font-size:15px; color:var(--accent); list-style:none; }
  details.subsec[open] > summary { border-bottom:1px solid var(--line); }
  details.subsec > .body { padding:8px 12px; }
  details.item { border-left:2px solid var(--line); margin:10px 0; padding-left:12px; }
  details.item > summary { cursor:pointer; list-style:none; padding:3px 0; }
  summary::-webkit-details-marker { display:none; }
  summary .tw { color:var(--muted); display:inline-block; width:1em; }
  .perma { color:var(--muted); opacity:0; margin-left:8px; font-size:.85em; }
  summary:hover .perma, h2:hover .perma { opacity:1; }
  .desc { color:var(--fg); margin:4px 0 8px; opacity:.92; }
  .rel { margin:2px 0; } .rel .lbl { color:var(--muted); }
  table { border-collapse:collapse; margin:6px 0 4px; width:100%; }
  th,td { border:1px solid var(--line); padding:4px 8px; text-align:left; vertical-align:top; }
  th { background:var(--bg3); color:#fff; font-weight:600; }
  .badge { background:var(--bg3); border:1px solid var(--line); border-radius:4px; padding:0 6px; font-size:.85em; }
  .toolbar { background:var(--bg); padding:10px 0; border-bottom:1px solid var(--line); }
  /* sticky breadcrumb: shows context › section › item wherever you are, each segment clickable */
  .crumb { position:sticky; top:0; z-index:6; background:var(--bg3); border-bottom:1px solid var(--line); margin:0 -20px 8px; padding:7px 20px; font-size:13px; white-space:nowrap; overflow-x:auto; color:var(--muted); }
  .crumb .seg { color:var(--fg); cursor:pointer; }
  .crumb .seg:hover { color:var(--accent); text-decoration:underline; }
  .crumb .sep { color:var(--muted); margin:0 7px; }
  /* hover tooltip: an object's description, looked up (centralized) from CF_DESC by anchor id */
  .cf-tip { position:fixed; z-index:50; max-width:440px; background:#1e1e1e; color:var(--fg); border:1px solid var(--line); border-radius:6px; padding:8px 10px; font-size:12.5px; line-height:1.5; box-shadow:0 4px 16px rgba(0,0,0,.45); pointer-events:none; display:none; }
  .cf-tip.empty { color:var(--muted); font-style:italic; }
  .toolbar button { background:var(--bg3); color:var(--fg); border:1px solid var(--line); border-radius:4px; padding:4px 10px; cursor:pointer; font:inherit; }
  .toolbar button:hover { border-color:var(--accent); color:#fff; }
  .toc a { margin-right:14px; white-space:nowrap; }
  .hole { color:var(--error); }
  /* interactive C4 / flow map */
  .cfmap { border:1px solid var(--line); border-radius:6px; background:#262626; padding:8px; }
  .cfmap-bar { display:flex; align-items:center; gap:10px; padding:4px 6px; flex-wrap:wrap; }
  .cfmap-bar button { background:var(--bg3); color:var(--fg); border:1px solid var(--line); border-radius:4px; padding:3px 10px; cursor:pointer; font:inherit; }
  .cfmap-bar button:hover { border-color:var(--accent); color:#fff; }
  #cf-svg { width:100%; height:auto; display:block; background:#262626; border-radius:4px; }
  .cf-node { cursor:pointer; }
  .cf-node:hover rect { filter:brightness(1.3); }
  .cf-node text { pointer-events:none; }
  .cfmap-info { padding:6px; font-size:.88em; }
  /* saga sequence diagrams: MERMAID_JS renders pre.mermaid in place; offline the same styling
     keeps the diagram SOURCE readable (monospace, scrollable, dark-palette border) */
  .pm-seq { margin:8px 0; }
  .pm-seq pre.mermaid { background:#262626; border:1px solid var(--line); border-radius:6px; padding:10px 12px; overflow-x:auto; font-size:12.5px; line-height:1.5; color:var(--fg); }
  .pm-seq pre.mermaid svg { max-width:100%; }
</style>
<script>
  function setAll(open){ document.querySelectorAll('details').forEach(d=>d.open=open); }
</script>"##;

pub(crate) const MAP_JS: &str = r##"(function(){var M=__CF_DATA__;var svg=document.getElementById('cf-svg'),crumb=document.getElementById('cf-crumb'),info=document.getElementById('cf-info'),back=document.getElementById('cf-back');if(!svg)return;var NS='http://www.w3.org/2000/svg';var stack=[{key:'system',title:'System'}];function slug(s){return String(s).toLowerCase().replace(/[^a-z0-9_]+/g,'-');}function el(t,a,x){var e=document.createElementNS(NS,t);for(var k in a)e.setAttribute(k,a[k]);if(x!=null)e.textContent=x;return e;}var K={container:'#4ec9b0',external:'#cc7832',context:'#ffc66d',actor:'#4ec9b0','process':'#56a0c0',command:'#dcdcaa',event:'#c586c0',view:'#9cdcfe'};function find(a,id){for(var i=0;i<a.length;i++)if(a[i].id===id)return a[i];return null;}function frame(key){if(key==='system'){var nodes=[];M.containers.forEach(function(c){nodes.push({id:c.id,label:c.id,kind:'container',sub:'container:'+c.id,desc:c.technology+' — '+c.description});});M.externals.forEach(function(x){nodes.push({id:x.id,label:x.id,kind:'external',desc:x.description});});var ids={};nodes.forEach(function(n){ids[n.id]=1;});var edges=M.relationships.filter(function(r){return ids[r.from]&&ids[r.to];}).map(function(r){return {from:r.from,to:r.to,label:r.description};});return {title:'System',nodes:nodes,edges:edges,note:'Containers (teal) and external systems (orange). Click a container to see its bounded contexts.'};}if(key.indexOf('container:')===0){var id=key.slice(10);var c=find(M.containers,id)||{realizes:[]};var nodes=[];M.contexts.forEach(function(ctx){var inIt=(ctx.aggregates||[]).some(function(a){return (c.realizes||[]).indexOf(a)>=0;});if(inIt)nodes.push({id:ctx.id,label:ctx.id,kind:'context',sub:'context:'+ctx.id,desc:ctx.description});});return {title:id,nodes:nodes,edges:[],note:nodes.length?'Bounded contexts running in this container. Click one to see its aggregates.':'No bounded context runs in this container (infrastructure/runtime unit).'};}if(key.indexOf('context:')===0){var id=key.slice(8);var ctx=find(M.contexts,id)||{aggregates:[],processManagers:[]};var nodes=(ctx.aggregates||[]).map(function(a){return {id:a,label:a,kind:'actor',sub:'actor:'+a,anchor:'actor-'+slug(a)};});(ctx.processManagers||[]).forEach(function(a){nodes.push({id:a,label:a,kind:'process',sub:'actor:'+a,anchor:'actor-'+slug(a)});});return {title:id,nodes:nodes,edges:[],note:'Aggregates and process managers (sagas). Click one to see its command → event → view flow.'};}if(key.indexOf('actor:')===0){var name=key.slice(6);var a=M.actors[name]||{receives:[]};var nodes=[],edges=[],seen={};function add(id,label,kind,anchor){if(!seen[id]){seen[id]=1;nodes.push({id:id,label:label,kind:kind,anchor:anchor});}}add('A',name,a.type==='process-manager'?'process':'actor','actor-'+slug(name));a.receives.forEach(function(r){var mid=(r.isCommand?'c:':'e:')+r.message;add(mid,r.message,r.isCommand?'command':'event',(r.isCommand?'command-':'event-')+slug(r.message));edges.push({from:'A',to:mid,label:'receives'});(r.emits||[]).forEach(function(ev){add('e:'+ev,ev,'event','event-'+slug(ev));edges.push({from:mid,to:'e:'+ev,label:'emits'});M.views.forEach(function(v){if((v.fedBy||[]).indexOf(ev)>=0){add('v:'+v.name,v.name,'view','view-'+slug(v.name));edges.push({from:'e:'+ev,to:'v:'+v.name,label:'projects'});}});});});return {title:name,nodes:nodes,edges:edges,note:'Flow: message (yellow=command, purple=event) → emitted events → read models (blue). Click a box to jump to its section.'};}return {title:'?',nodes:[],edges:[]};}function render(){var f=frame(stack[stack.length-1].key);crumb.textContent=stack.map(function(s){return s.title;}).join('  ›  ');back.style.visibility=stack.length>1?'visible':'hidden';while(svg.firstChild)svg.removeChild(svg.firstChild);var defs=el('defs');var mk=el('marker',{id:'cf-arrow',viewBox:'0 0 10 10',refX:'9',refY:'5',markerWidth:'7',markerHeight:'7',orient:'auto'});mk.appendChild(el('path',{d:'M0,0 L10,5 L0,10 z',fill:'#888'}));defs.appendChild(mk);svg.appendChild(defs);var W=960,H=560,n=f.nodes.length||1;var cols=Math.max(1,Math.ceil(Math.sqrt(n)));var rows=Math.ceil(n/cols);var nw=180,nh=48;var gx=(W-cols*nw)/(cols+1),gy=(H-rows*nh)/(rows+1);var pos={};f.nodes.forEach(function(nd,i){var r=Math.floor(i/cols),c=i%cols;pos[nd.id]={x:gx+c*(nw+gx),y:gy+r*(nh+gy)};});f.edges.forEach(function(e){var a=pos[e.from],b=pos[e.to];if(!a||!b)return;var x1=a.x+nw/2,y1=a.y+nh/2,x2=b.x+nw/2,y2=b.y+nh/2;var ln=el('line',{x1:x1,y1:y1,x2:x2,y2:y2,stroke:'#6a6a6a','stroke-width':'1.3','marker-end':'url(#cf-arrow)'});if(e.label)ln.appendChild(el('title',null,e.label));svg.appendChild(ln);});f.nodes.forEach(function(nd){var p=pos[nd.id];var g=el('g',{'class':'cf-node',transform:'translate('+p.x+','+p.y+')'});g.appendChild(el('rect',{width:nw,height:nh,rx:'7',fill:'#313335',stroke:(K[nd.kind]||'#888'),'stroke-width':'1.6'}));var label=nd.label.length>24?nd.label.slice(0,23)+'…':nd.label;g.appendChild(el('text',{x:nw/2,y:nh/2+4,'text-anchor':'middle',fill:'#e6e6e6','font-size':'12'},label));if(nd.desc)g.appendChild(el('title',null,nd.desc));g.addEventListener('click',function(){if(nd.sub){stack.push({key:nd.sub,title:nd.label});render();}else if(nd.anchor){location.hash=nd.anchor;}});svg.appendChild(g);});info.textContent=f.note||'';}back.addEventListener('click',function(){if(stack.length>1){stack.pop();render();}});render();})();"##;

pub(crate) const NAV_JS: &str = r##"<script>(function(){var bar=document.getElementById('cf-crumb'),tip=document.getElementById('cf-tip'),doc=document.querySelector('.doc');if(!bar||!doc)return;var TH=54,cur={};function esc(s){return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;');}function lab(el){return el?(el.getAttribute('data-crumb')||''):'';}function lastAbove(sel){var e=document.querySelectorAll(sel),f=null;for(var i=0;i<e.length;i++){var s=e[i];if(s.offsetParent===null)continue;if(s.getBoundingClientRect().top<=TH)f=s;}return f;}function upd(){var a=lastAbove('details.sec>summary'),b=lastAbove('details.subsec>summary'),c=lastAbove('details.item>summary');cur.ctx=a?a.parentElement:null;cur.sec=b?b.parentElement:null;cur.item=c?c.parentElement:null;if(cur.sec&&cur.ctx&&!cur.ctx.contains(cur.sec))cur.sec=null;if(cur.item&&cur.sec&&!cur.sec.contains(cur.item))cur.item=null;if(cur.item&&!cur.sec)cur.item=null;var p=[];if(cur.ctx)p.push('<span class="seg" data-role="ctx">'+esc(lab(cur.ctx))+'</span>');if(cur.sec)p.push('<span class="seg" data-role="sec">'+esc(lab(cur.sec))+'</span>');if(cur.item)p.push('<span class="seg" data-role="item">'+esc(lab(cur.item))+'</span>');bar.innerHTML=p.length?p.join('<span class="sep">\u203a</span>'):'<span class="muted">\ud83d\udcd6 Captain.Food \u2014 Product Documentation</span>';}bar.addEventListener('click',function(e){var s=e.target.closest('.seg');if(!s)return;var el=cur[s.getAttribute('data-role')];if(!el)return;var sm=el.querySelector(':scope>summary')||el;var y=sm.getBoundingClientRect().top+window.pageYOffset-TH-8;window.scrollTo({top:y,behavior:'smooth'});});var raf=0;function onScroll(){if(raf)return;raf=requestAnimationFrame(function(){raf=0;upd();});}window.addEventListener('scroll',onScroll,{passive:true});window.addEventListener('resize',onScroll);document.addEventListener('toggle',onScroll,true);upd();var D=window.CF_DESC||{};doc.addEventListener('mouseover',function(e){var a=e.target.closest('a[href^="#"]');if(!a)return;var id=decodeURIComponent(a.getAttribute('href').slice(1));if(!(id in D)){tip.style.display='none';return;}var d=D[id];tip.textContent=d||'no description yet';tip.className='cf-tip'+(d?'':' empty');tip.style.display='block';});doc.addEventListener('mousemove',function(e){if(tip.style.display!=='block')return;var x=e.clientX+14,y=e.clientY+16,w=tip.offsetWidth,h=tip.offsetHeight;if(x+w>window.innerWidth-8)x=window.innerWidth-w-8;if(y+h>window.innerHeight-8)y=e.clientY-h-14;tip.style.left=x+'px';tip.style.top=y+'px';});doc.addEventListener('mouseout',function(e){if(e.target.closest('a[href^="#"]'))tip.style.display='none';});})();</script>"##;

// Renders every <pre class="mermaid"> (the saga sequence diagrams). Constraints: the CDN import may
// be unreachable (offline docs) — then the styled source text must stay as-is; diagrams sit inside
// <details> that the reader may collapse/re-open — mermaid mis-sizes hidden elements, so only
// visible ones are rendered and re-opened <details> render lazily on their toggle event, with a
// data-mermaid-rendered guard against double rendering.
pub(crate) const MERMAID_JS: &str = r##"<script type="module">
try {
  const { default: mermaid } = await import('https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs');
  mermaid.initialize({ startOnLoad: false, theme: 'dark', securityLevel: 'loose' });
  const render = (root) => {
    const nodes = [...root.querySelectorAll('pre.mermaid:not([data-mermaid-rendered])')].filter((n) => n.offsetParent !== null);
    if (!nodes.length) return;
    nodes.forEach((n) => n.setAttribute('data-mermaid-rendered', ''));
    mermaid.run({ nodes }).catch(() => {});
  };
  document.addEventListener('toggle', (e) => { if (e.target.open) render(e.target); }, true);
  render(document);
} catch (e) { /* offline: the <pre> keeps showing the diagram source */ }
</script>"##;

pub(crate) fn h_esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
pub(crate) fn h_cls(k: &str) -> &'static str {
    match k {
        "type" | "entity" | "view" | "actor" | "context" | "container" | "screen" => "k-type",
        "scalar" | "rule" | "translation" => "k-scalar",
        "query" | "mutation" | "command" | "test" | "component" | "subscription" => "k-op",
        "event" | "obs" => "k-event",
        "error" => "k-error",
        "property" => "k-prop",
        _ => "k-id",
    }
}
pub(crate) fn h_link(kind: &str, name: &str) -> String {
    format!("<a class=\"{}\" href=\"#{}\">{}&nbsp;{}</a>", h_cls(kind), danchor(kind, name), d_emo(kind), h_esc(name))
}
pub(crate) fn h_plink(kind: &str, owner: &str, field: &str) -> String {
    format!("<a class=\"{}\" href=\"#{}\">{}&nbsp;{}.<span class=\"k-prop\">{}</span></a>", h_cls(kind), dprop_anchor(kind, owner, field), d_emo(kind), h_esc(owner), h_esc(field))
}
pub(crate) fn h_ref_label(rf: &str) -> String {
    let mut it = rf.splitn(2, "#/");
    let file = it.next().unwrap_or("");
    let name = it.next().unwrap_or("");
    if file == "scalars.yaml" { h_link("scalar", name) } else { h_link("entity", name) }
}
pub(crate) fn h_raw_type(p: &Value) -> String {
    if let Some(rf) = p.get("$ref").and_then(|x| x.as_str()) {
        return h_ref_label(rf);
    }
    if p.get("type").and_then(|x| x.as_str()) == Some("array") {
        if let Some(items) = p.get("items") {
            return format!("[{}]", h_raw_type(items));
        }
    }
    let mut t = format!("<span class=\"k-const\">{}</span>", h_esc(p.get("type").and_then(|x| x.as_str()).unwrap_or("?")));
    if let Some(en) = p.get("enum").and_then(|x| x.as_sequence()) {
        t += &format!(" <span class=\"muted\">({})</span>", en.iter().filter_map(|v| v.as_str()).map(h_esc).collect::<Vec<_>>().join(" | "));
    }
    if let Some(fmt) = p.get("format").and_then(|x| x.as_str()) {
        t += &format!(" <span class=\"muted\">{}</span>", h_esc(fmt));
    }
    t
}
pub(crate) fn h_req_cell(required: bool, nullable: bool) -> String {
    if required {
        "<span class=\"req\">✅ required</span>".to_string()
    } else {
        format!("<span class=\"opt\">⬜ {}</span>", if nullable { "nullable" } else { "optional" })
    }
}
pub(crate) fn h_table(head: &[&str], rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let thead = head.iter().map(|h| format!("<th>{}</th>", h)).collect::<Vec<_>>().join("");
    let tbody = rows.iter().map(|r| format!("<tr>{}</tr>", r.iter().map(|c| format!("<td>{}</td>", c)).collect::<Vec<_>>().join(""))).collect::<Vec<_>>().join("");
    format!("<table><thead><tr>{}</tr></thead><tbody>{}</tbody></table>", thead, tbody)
}
pub(crate) fn h_item(kind: &str, label: &str, name: &str, body: &str, desc_txt: Option<&str>) -> String {
    let id = danchor(kind, name);
    let perma = format!("<a class=\"perma\" href=\"#{}\" title=\"Lien vers cette section\">🔗 #{}</a>", id, id);
    let desc = match desc_txt {
        Some(d) if !d.is_empty() => format!("<div class=\"desc\">{}</div>", h_esc(d)),
        _ => String::new(),
    };
    format!("<details class=\"item\" id=\"{}\" data-crumb=\"{} {}\" open><summary><span class=\"tw\">▸</span><span class=\"muted\">{}:</span> <span class=\"{}\">{} {}</span>{}</summary>{}{}</details>", id, d_emo(kind), h_esc(name), label, h_cls(kind), d_emo(kind), h_esc(name), perma, desc, body)
}
pub(crate) fn h_prop_rows(def: &Value, kind: &str, owner: &str) -> Vec<Vec<String>> {
    let props = match def.get("properties").and_then(|x| x.as_mapping()) {
        Some(m) => m,
        None => return vec![],
    };
    let required: HashSet<&str> = def.get("required").and_then(|x| x.as_sequence()).map(|s| s.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
    let mut rows = Vec::new();
    for (k, p) in props {
        let n = match k.as_str() { Some(s) => s, None => continue };
        rows.push(vec![
            format!("<span id=\"{}\" class=\"k-prop\">{}</span>", dprop_anchor(kind, owner, n), h_esc(n)),
            h_raw_type(p),
            h_req_cell(required.contains(n), p.get("nullable").and_then(|x| x.as_bool()) == Some(true)),
            h_esc(&ws1(p.get("description").and_then(|x| x.as_str()).unwrap_or(""))),
        ]);
    }
    rows
}
pub(crate) fn h_sec(id: &str, emoji: &str, title: &str, body: &str) -> String {
    format!("<details class=\"sec\" id=\"sec-{}\" data-crumb=\"{} {}\" open><summary>{} {} <a class=\"perma\" href=\"#sec-{}\">🔗</a></summary><div class=\"body\">{}</div></details>", id, emoji, h_esc(title), emoji, h_esc(title), id, body)
}
pub(crate) fn h_subsec(emoji: &str, title: &str, count: usize, body: &str) -> String {
    format!("<details class=\"subsec\" data-crumb=\"{} {}\" open><summary>{} {} <span class=\"muted\">({})</span></summary><div class=\"body\">{}</div></details>", emoji, h_esc(title), emoji, h_esc(title), count, body)
}
pub(crate) fn h_any_link(rf: &str) -> String {
    let mut it = rf.splitn(2, "#/");
    let file = it.next().unwrap_or("");
    let name = it.next().unwrap_or("");
    let kind = match file { "commands.yaml" => "command", "events.yaml" => "event", "actors.yaml" => "actor", "database/projection_views.yaml" => "view", "database/tables/projection_tables.yaml" => "view", "database/tables/referential.yaml" => "view", "scalars.yaml" => "scalar", _ => "entity" };
    h_link(kind, name)
}
pub(crate) fn h_ref_links(v: Option<&Value>) -> String {
    let s = v.and_then(|x| x.as_sequence()).map(|arr| arr.iter().filter_map(|it| it.get("$ref").and_then(|r| r.as_str())).map(h_any_link).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    if s.is_empty() { "—".to_string() } else { s }
}

pub(crate) struct HDoc {
    pub(crate) ctx: String,
    pub(crate) html: String,
}
pub(crate) struct HRow {
    pub(crate) ctx: String,
    pub(crate) cells: Vec<String>,
}

pub(crate) fn emit_documentation_html(model: &Model) -> String {
    let api = parse_api(model);
    let actors = parse_actors(model);
    let views = parse_views(model);
    let personas = parse_stories(model);
    let cx = build_context_map(model, &api, &actors, &views);
    let scalar_set = scalar_names(model);
    let entity_set: HashSet<String> = model.defs.get("entities.yaml").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let type_set: HashSet<String> = api.types.iter().map(|t| t.name.clone()).collect();
    let raw_desc = |file: &str, name: &str| -> String { model.defs.get(file).and_then(|m| m.get(name)).and_then(|n| n.get("description")).and_then(|x| x.as_str()).unwrap_or("").to_string() };

    let h_api_type = |f: &ApiField| -> String {
        let base = if f.is_ref {
            if scalar_set.contains(&f.ty) { h_link("scalar", &f.ty) } else if type_set.contains(&f.ty) { h_link("type", &f.ty) } else if entity_set.contains(&f.ty) { h_link("entity", &f.ty) } else { format!("<span class=\"k-id\">{}</span>", h_esc(&f.ty)) }
        } else {
            format!("<span class=\"k-const\">{}</span>{}", h_esc(&f.ty), f.format.as_deref().map(|fmt| format!(" <span class=\"muted\">{}</span>", h_esc(fmt))).unwrap_or_default())
        };
        if f.array { format!("[{}]", base) } else { base }
    };

    // relationship indexes
    let mut cmd_handler: HashMap<String, (String, Vec<String>, Vec<String>)> = HashMap::new();
    let mut evt_emitted_by: HashMap<String, Vec<String>> = HashMap::new();
    let mut evt_consumed_by: HashMap<String, Vec<String>> = HashMap::new();
    let mut err_thrown_by: HashMap<String, Vec<String>> = HashMap::new();
    for a in &actors {
        for e in &a.receives {
            let msg = ref_name(&e.message_ref);
            let emits: Vec<String> = e.emits.iter().filter_map(|r| ref_name(r)).collect();
            let throws: Vec<String> = e.throws.iter().filter_map(|r| ref_name(r)).collect();
            if e.message_ref.starts_with("commands.yaml#/") {
                if let Some(m) = &msg {
                    cmd_handler.insert(m.clone(), (a.name.clone(), emits.clone(), throws.clone()));
                    for er in &throws { push_uniq(&mut err_thrown_by, er, m); }
                }
            } else if e.message_ref.starts_with("events.yaml#/") {
                if let Some(m) = &msg { push_uniq(&mut evt_consumed_by, m, &a.name); }
            }
            for ev in &emits { push_uniq(&mut evt_emitted_by, ev, &a.name); }
        }
    }
    let mut evt_views: HashMap<String, Vec<String>> = HashMap::new();
    for v in &views { for e in &v.fedby { push_uniq(&mut evt_views, e, &v.name); } }
    let mut mut_by_command: HashMap<String, String> = HashMap::new();
    for m in &api.mutations { mut_by_command.insert(m.command.clone(), m.name.clone()); }

    // 1. Stories
    let stories_html = personas.iter().map(|p| {
        let badge = format!("<span class=\"badge\">{} {}</span>{}", user_emo(&p.role), h_esc(&p.role), p.locale.as_deref().map(|l| format!(" <span class=\"badge\">🗣️ {}</span>", h_esc(l))).unwrap_or_default());
        let mut rows: Vec<Vec<String>> = Vec::new();
        for act in &p.activities {
            for (i, s) in act.steps.iter().enumerate() {
                let op = if let (Some(op), Some(kind)) = (&s.op, &s.op_kind) { h_link(kind, op) } else if let Some(note) = &s.note { format!("📝 <span class=\"muted\">{}</span>", h_esc(note)) } else { "—".to_string() };
                rows.push(vec![if i == 0 { format!("<span class=\"kw\">{}</span>", h_esc(&act.name)) } else { String::new() }, h_esc(&s.name), op]);
            }
        }
        h_item("story", "Persona", &p.name, &h_table(&["Activity", "Step", "Operation"], &rows), p.description.as_deref())
            .replacen("</summary>", &format!(" {}</summary>", badge), 1)
    }).collect::<Vec<_>>().join("");

    // 2. API operations
    let mut api_docs: Vec<HDoc> = Vec::new();
    for q in &api.queries {
        let field_list = q.args.iter().map(|a| format!("<span class=\"k-param\">{}{}</span>: {}", h_esc(&a.name), if a.required { "" } else { "?" }, h_api_type(a))).collect::<Vec<_>>().join(", ");
        let input_rel = if q.args.is_empty() {
            "<div class=\"rel\"><span class=\"lbl\">input:</span> <span class=\"muted\">(none)</span></div>".to_string()
        } else {
            format!("<div class=\"rel\"><span class=\"lbl\">input:</span> <span class=\"k-type\">🧩 {}QueryInput{}</span> <span class=\"muted\">{{ {} }}</span></div>", h_esc(&pascal(&q.name)), if q.args.iter().any(|a| a.required) { "!" } else { "" }, field_list)
        };
        let ret = format!("{}{}", if type_set.contains(&q.returns_type) { h_link("type", &q.returns_type) } else if entity_set.contains(&q.returns_type) { h_link("entity", &q.returns_type) } else { format!("<span class=\"k-id\">{}</span>", h_esc(&q.returns_type)) }, if q.returns_list { " []" } else { "" });
        let reads = { let s = q.reads.iter().map(|v| h_link("view", v)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
        let body = format!("{}<div class=\"rel\"><span class=\"lbl\">returns:</span> {} · <span class=\"lbl\">reads</span> {}</div><div class=\"rel\"><span class=\"lbl\">roles:</span> {} · <span class=\"badge\">{}</span></div>", input_rel, ret, reads, h_esc(&roles_label(&q.roles)), q.slice);
        let ctx = cx.of_operation(&q.roles, &(if !q.reads.is_empty() { cx.of_reads(&q.reads) } else { cx.of_type(&q.returns_type) }));
        api_docs.push(HDoc { ctx, html: h_item("query", "Query", &q.name, &body, q.description.as_deref()) });
    }
    for m in &api.mutations {
        let h = cmd_handler.get(&m.command);
        let body = format!("<div class=\"rel\"><span class=\"lbl\">command:</span> {}{}</div><div class=\"rel\"><span class=\"lbl\">roles:</span> {} · <span class=\"badge\">{}</span></div><div class=\"rel\"><span class=\"lbl\">returns:</span> {} <span class=\"muted\">(acceptance-first — outcome via {})</span></div>", h_link("command", &m.command), h.map(|h| format!(" → {}", h_link("actor", &h.0))).unwrap_or_default(), h_esc(&roles_label(&m.roles)), m.slice, h_link("type", "MutationAcceptance"), h_link("query", "operationStatus"));
        api_docs.push(HDoc { ctx: cx.of_command(&m.command), html: h_item("mutation", "Mutation", &m.name, &body, None) });
    }
    for s in &api.subscriptions {
        let field_list = s.args.iter().map(|a| format!("<span class=\"k-param\">{}{}</span>: {}", h_esc(&a.name), if a.required { "" } else { "?" }, h_api_type(a))).collect::<Vec<_>>().join(", ");
        let input_rel = if s.args.is_empty() {
            "<div class=\"rel\"><span class=\"lbl\">input:</span> <span class=\"muted\">(none)</span></div>".to_string()
        } else {
            format!("<div class=\"rel\"><span class=\"lbl\">input:</span> <span class=\"k-type\">🧩 {}SubscriptionInput{}</span> <span class=\"muted\">{{ {} }}</span></div>", h_esc(&pascal(&s.name)), if s.args.iter().any(|a| a.required) { "!" } else { "" }, field_list)
        };
        let ret = format!("{}{}", if type_set.contains(&s.returns_type) { h_link("type", &s.returns_type) } else if entity_set.contains(&s.returns_type) { h_link("entity", &s.returns_type) } else { format!("<span class=\"k-id\">{}</span>", h_esc(&s.returns_type)) }, if s.returns_list { " []" } else { "" });
        let body = format!("{}<div class=\"rel\"><span class=\"lbl\">streams:</span> {}</div><div class=\"rel\"><span class=\"lbl\">roles:</span> {} · <span class=\"badge\">{}</span></div>", input_rel, ret, h_esc(&roles_label(&s.roles)), s.slice);
        api_docs.push(HDoc { ctx: cx.of_operation(&s.roles, &cx.of_type(&s.returns_type)), html: h_item("subscription", "Subscription", &s.name, &body, s.description.as_deref()) });
    }
    let type_docs: Vec<HDoc> = api.types.iter().map(|t| {
        let reads = t.reads.iter().map(|v| h_link("view", v)).collect::<Vec<_>>().join(", ");
        let rows: Vec<Vec<String>> = t.properties.iter().map(|f| vec![format!("<span id=\"{}\" class=\"k-prop\">{}</span>", dprop_anchor("type", &t.name, &f.name), h_esc(&f.name)), h_api_type(f), h_req_cell(!f.nullable, f.nullable)]).collect();
        let body = format!("<div class=\"rel\"><span class=\"lbl\">read model:</span> {}</div>{}", if reads.is_empty() { "<span class=\"muted\">(within a parent projection)</span>".to_string() } else { reads }, h_table(&["Field", "Type", "Req."], &rows));
        HDoc { ctx: cx.of_type(&t.name), html: h_item("type", "Type", &t.name, &body, t.description.as_deref()) }
    }).collect();

    // 3. Actors — process managers also embed their saga sequence diagram, aggregates with a declared
    // `lifecycle` their state diagram (ADR-20260720-004419); the <pre class="mermaid">
    // source is rendered client-side by MERMAID_JS and stays readable as text when offline.
    let pm_seq: HashMap<String, String> = pm_sequence_map(model).into_iter().collect();
    let lc_state: HashMap<String, String> = lifecycle_state_map(model).into_iter().collect();
    let all_reminders = parse_reminders(model);
    let all_deletions = parse_deletions(model);
    let actor_docs: Vec<HDoc> = actors.iter().map(|a| {
        let kind = if a.kind == "aggregate" { "🧩 aggregate" } else { "⚙️ process manager" };
        let rows: Vec<Vec<String>> = a.receives.iter().map(|e| {
            let msg = match reminder_ref_parts(&e.message_ref) {
                Some((_, rname)) => format!("⏰ <span class=\"k-id\">{}</span> <span class=\"muted\">(reminder)</span>", h_esc(&rname)),
                None => {
                    let is_cmd = e.message_ref.starts_with("commands.yaml#/");
                    h_link(if is_cmd { "command" } else { "event" }, &ref_name(&e.message_ref).unwrap_or_else(|| "?".to_string()))
                }
            };
            let emits = {
                let mut cells: Vec<String> = e.emits.iter().map(|r| h_link("event", &ref_name(r).unwrap_or_default())).collect();
                cells.extend(e.schedules.iter().filter_map(|r| reminder_ref_parts(r)).map(|(_, n)| format!("⏰ schedules <span class=\"k-id\">{}</span>", h_esc(&n))));
                let s = cells.join(", ");
                if s.is_empty() { e.effect.as_deref().map(|x| format!("<span class=\"muted\">{}</span>", h_esc(x))).unwrap_or_else(|| "—".to_string()) } else { s }
            };
            let throws = { let s = e.throws.iter().map(|r| h_link("error", &ref_name(r).unwrap_or_default())).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
            vec![msg, emits, throws]
        }).collect();
        let mut extras = String::new();
        let rems: Vec<&ReminderDef> = all_reminders.iter().filter(|r| r.actor == a.name).collect();
        if !rems.is_empty() {
            let rrows: Vec<Vec<String>> = rems.iter().map(|r| vec![
                format!("⏰ <span class=\"k-id\">{}</span>", h_esc(&r.name)),
                h_link("event", &ref_name(&r.payload_ref).unwrap_or_else(|| "?".to_string())),
                r.after_ref.as_deref().and_then(config_key_ref_name).map(|k| format!("⚙️ <span class=\"k-const\">{}</span>", h_esc(&k))).unwrap_or_else(|| "—".to_string()),
                h_esc(r.reschedule.as_deref().unwrap_or("in-place")),
            ]).collect();
            extras.push_str(&format!("<div class=\"rel\"><span class=\"lbl\">reminders (self-scheduled facts — ADR-20260731-214500):</span></div>{}", h_table(&["Reminder", "Payload", "After", "Reschedule"], &rrows)));
        }
        if let Some(d) = all_deletions.iter().find(|d| d.actor == a.name) {
            let trows: Vec<Vec<String>> = d.triggers.iter().map(|t| {
                let on = { let s = t.on.iter().map(|r| h_link("event", &ref_name(r).unwrap_or_default())).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
                let window = t.after_ref.as_deref().and_then(config_key_ref_name).map(|k| format!("⚙️ <span class=\"k-const\">{}</span>", h_esc(&k))).unwrap_or_else(|| "<span class=\"muted\">immediate (propagation)</span>".to_string());
                let cancelled = { let s = t.cancelled_on.iter().map(|r| h_link("event", &ref_name(r).unwrap_or_default())).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
                let m = match (t.match_event_ref.as_deref().and_then(lineage_parts), t.match_state_ref.as_deref().and_then(parse_ref)) {
                    (Some((ev, Some(prop))), Some(st)) => format!("{} ↔ <span class=\"k-prop\">state.{}</span>", h_plink("event", ev, prop), h_esc(&st.path.last().cloned().unwrap_or_default())),
                    _ => "—".to_string(),
                };
                vec![on, window, cancelled, m]
            }).collect();
            let receipt = d.receipt_ref.as_deref().and_then(ref_name).map(|e| h_link("event", &e)).unwrap_or_else(|| "—".to_string());
            extras.push_str(&format!("<div class=\"rel\"><span class=\"lbl\">deletion (declarative, generic engine — ADR-20260731-214500):</span></div>{}<div class=\"rel\"><span class=\"lbl\">receipt:</span> {}</div>", h_table(&["On", "Window", "Cancelled on", "Match"], &trows), receipt));
        }
        let seq = if a.kind == "aggregate" {
            lc_state.get(&a.name).map(|d| format!("<div class=\"pm-seq\"><pre class=\"mermaid\">{}</pre></div>", h_esc(d))).unwrap_or_default()
        } else {
            pm_seq.get(&a.name).map(|d| format!("<div class=\"pm-seq\"><pre class=\"mermaid\">{}</pre></div>", h_esc(d))).unwrap_or_default()
        };
        HDoc { ctx: cx.of_actor(&a.name), html: h_item("actor", "Actor", &a.name, &format!("<div class=\"rel muted\">{}</div>{}{}{}", kind, h_table(&["Receives", "Emits →", "Throws"], &rows), extras, seq), a.description.as_deref()) }
    }).collect();

    // 4. Views
    let view_docs: Vec<HDoc> = views.iter().map(|v| {
        let slice = if v.slice == "V1" { "🔭 V1" } else { "🛶 V0" };
        let fed_by = { let s = v.fedby.iter().map(|n| h_link("event", n)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
        let rows: Vec<Vec<String>> = v.columns.iter().map(|c| {
            let type_cell = format!("{}{}{}", if scalar_set.contains(&c.ty) { h_link("scalar", &c.ty) } else { format!("<span class=\"k-const\">{}</span>", h_esc(if c.ty.is_empty() { "?" } else { &c.ty })) }, if c.type_derived { " <span class=\"muted\">(derived)</span>" } else { "" }, c.fk.as_ref().map(|f| format!(" → {}", h_link("view", f.split('.').next().unwrap_or(f)))).unwrap_or_default());
            let src = { let s = c.from.iter().map(|rf| { let segs: Vec<&str> = rf.splitn(2, "#/").nth(1).unwrap_or("").split('/').filter(|x| !x.is_empty()).collect(); let prop = if segs.get(1) == Some(&"properties") { segs.get(2).copied() } else { None }; match prop { Some(p) => h_plink("event", segs.first().copied().unwrap_or(""), p), None => h_link("event", segs.first().copied().unwrap_or("")) } }).collect::<Vec<_>>().join(", "); if s.is_empty() { "<span class=\"hole\">⚠️ none</span>".to_string() } else { s } };
            let flags = { let f: Vec<&str> = [(c.pk, "PK"), (c.unique, "unique"), (c.index, "index"), (c.nullable, "nullable")].iter().filter(|(b, _)| *b).map(|(_, s)| *s).collect(); if f.is_empty() { "—".to_string() } else { f.join(", ") } };
            vec![format!("<span id=\"{}\" class=\"k-prop\">{}</span>", dprop_anchor("view", &v.name, &c.name), h_esc(&c.name)), type_cell, src, flags, h_esc(&ws1(c.note.as_deref().unwrap_or("")))]
        }).collect();
        let body = format!("<div class=\"rel\"><span class=\"lbl\">source:</span> {} · {}{}</div>{}<div class=\"rel\"><span class=\"lbl\">fed by:</span> {}</div>{}", if v.reference { "📦 reference (static seed)".to_string() } else { h_link("actor", &v.aggregate) }, slice, if v.internal { " · 🔒 internal" } else { "" }, v.note.as_deref().map(|n| format!("<div class=\"desc\">{}</div>", h_esc(&ws1(n)))).unwrap_or_default(), fed_by, h_table(&["Column", "Type", "Sourced from", "Constraints", "Notes"], &rows));
        HDoc { ctx: cx.of_view(&v.name), html: h_item("view", "View", &v.name, &body, None) }
    }).collect();

    // 5. Commands
    let cmd_map = model.defs.get("commands.yaml").and_then(|v| v.as_mapping());
    let command_docs: Vec<HDoc> = cmd_map.map(|m| m.iter().filter_map(|(k, _)| k.as_str()).filter(|c| cmd_handler.contains_key(*c)).map(|c| {
        let h = cmd_handler.get(c).unwrap();
        let mutn = mut_by_command.get(c);
        let def = cmd_map.and_then(|m| m.get(c)).cloned().unwrap_or(Value::Null);
        let body = format!("<div class=\"rel\"><span class=\"lbl\">dispatched by:</span> {} · <span class=\"lbl\">handled by</span> {}</div><div class=\"rel\"><span class=\"lbl\">emits:</span> {}</div><div class=\"rel\"><span class=\"lbl\">throws:</span> {}</div>{}",
            mutn.map(|m| h_link("mutation", m)).unwrap_or_else(|| "—".to_string()), h_link("actor", &h.0),
            { let s = h.1.iter().map(|e| h_link("event", e)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } },
            { let s = h.2.iter().map(|e| h_link("error", e)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } },
            h_table(&["Field", "Type", "Req.", "Description"], &h_prop_rows(&def, "command", c)));
        HDoc { ctx: cx.of_command(c), html: h_item("command", "Command", c, &body, Some(&doc_desc(model, "commands.yaml", c))) }
    }).collect()).unwrap_or_default();

    // 6. Events
    let non_projected: HashSet<String> = ref_names(model.defs.get("database/projection_views.yaml").and_then(|v| v.get("nonProjectedEvents"))).into_iter().collect();
    let evt_map = model.defs.get("events.yaml").and_then(|v| v.as_mapping());
    let event_docs: Vec<HDoc> = evt_map.map(|m| m.iter().filter_map(|(k, _)| k.as_str()).map(|ev| {
        let def = evt_map.and_then(|m| m.get(ev)).cloned().unwrap_or(Value::Null);
        let projected = { let s = evt_views.get(ev).map(|vs| vs.iter().map(|v| h_link("view", v)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if !s.is_empty() { s } else if non_projected.contains(ev) { "<span class=\"muted\">non-projected</span>".to_string() } else { "—".to_string() } };
        let body = format!("<div class=\"rel\"><span class=\"lbl\">emitted by:</span> {}</div><div class=\"rel\"><span class=\"lbl\">consumed by:</span> {}</div><div class=\"rel\"><span class=\"lbl\">projected into:</span> {}</div>{}",
            { let s = evt_emitted_by.get(ev).map(|a| a.iter().map(|x| h_link("actor", x)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "<span class=\"muted\">inbound / external</span>".to_string() } else { s } },
            { let s = evt_consumed_by.get(ev).map(|a| a.iter().map(|x| h_link("actor", x)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "—".to_string() } else { s } },
            projected, h_table(&["Field", "Type", "Req.", "Description"], &h_prop_rows(&def, "event", ev)));
        HDoc { ctx: cx.of_event(ev), html: h_item("event", "Event", ev, &body, Some(&doc_desc(model, "events.yaml", ev))) }
    }).collect()).unwrap_or_default();

    // 7. Entities
    let ent_map = model.defs.get("entities.yaml").and_then(|v| v.as_mapping());
    let entity_docs: Vec<HDoc> = ent_map.map(|m| m.iter().filter_map(|(k, _)| k.as_str()).map(|e| {
        let def = ent_map.and_then(|m| m.get(e)).cloned().unwrap_or(Value::Null);
        HDoc { ctx: cx.of_entity(e), html: h_item("entity", "Entity", e, &h_table(&["Field", "Type", "Req.", "Description"], &h_prop_rows(&def, "entity", e)), Some(&doc_desc(model, "entities.yaml", e))) }
    }).collect()).unwrap_or_default();

    // 8. Scalars
    let scalar_rows: Vec<HRow> = model.defs.get("scalars.yaml").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, d)| k.as_str().map(|name| {
        let mut t = format!("<span class=\"k-const\">{}</span>", h_esc(d.get("type").and_then(|x| x.as_str()).unwrap_or("?")));
        if let Some(en) = d.get("enum").and_then(|x| x.as_sequence()) {
            t = format!("<span class=\"kw\">enum</span> <span class=\"muted\">({})</span>", en.iter().filter_map(|v| v.as_str()).map(h_esc).collect::<Vec<_>>().join(" | "));
        } else if let Some(fmt) = d.get("format").and_then(|x| x.as_str()) {
            t += &format!(" <span class=\"muted\">{}</span>", h_esc(fmt));
        } else if let Some(pat) = d.get("pattern").and_then(|x| x.as_str()) {
            t += &format!(" <span class=\"muted\">{}</span>", h_esc(pat));
        }
        HRow { ctx: cx.of_scalar(name), cells: vec![format!("<span id=\"{}\" class=\"k-scalar\">{} {}</span>", danchor("scalar", name), d_emo("scalar"), h_esc(name)), t, h_esc(&ws1(d.get("description").and_then(|x| x.as_str()).unwrap_or("")))] }
    })).collect()).unwrap_or_default();

    // 9. Errors
    let error_rows: Vec<HRow> = model.defs.get("errors.yaml").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, d)| k.as_str().map(|name| {
        let msgs = d.get("messages");
        let en = msgs.and_then(|x| x.get("en")).and_then(|x| x.as_str()).unwrap_or("");
        let fr = msgs.and_then(|x| x.get("fr")).and_then(|x| x.as_str()).unwrap_or("");
        let by = { let s = err_thrown_by.get(name).map(|c| c.iter().map(|x| h_link("command", x)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "—".to_string() } else { s } };
        HRow { ctx: cx.of_error(name), cells: vec![format!("<span id=\"{}\" class=\"k-error\">{} {}</span>", danchor("error", name), d_emo("error"), h_esc(name)), h_esc(&ws1(d.get("description").and_then(|x| x.as_str()).unwrap_or(""))), format!("🇬🇧 {}", h_esc(en)), format!("🇫🇷 {}", h_esc(fr)), by] }
    })).collect()).unwrap_or_default();

    // rules ↔ tests
    let rule_defs = model.defs.get("rules.yaml").and_then(|v| v.as_mapping());
    let tests_map = model.defs.get("tests.yaml").and_then(|v| v.get("tests")).and_then(|v| v.as_mapping());
    let fixtures_map = model.defs.get("tests.yaml").and_then(|v| v.get("fixtures")).and_then(|v| v.as_mapping());
    let rules_of_test = |t: &Value| -> Vec<String> { t.get("rules").and_then(|x| x.as_sequence()).map(|s| s.iter().filter_map(|r| r.get("$ref").and_then(|x| x.as_str()).and_then(ref_name)).collect()).unwrap_or_default() };
    let mut rule_tests: HashMap<String, Vec<String>> = HashMap::new();
    let mut test_actor_name: HashMap<String, String> = HashMap::new();
    if let Some(tm) = tests_map {
        for (k, t) in tm {
            if let Some(tn) = k.as_str() {
                test_actor_name.insert(tn.to_string(), ref_name(t.get("actor").and_then(|a| a.get("$ref")).and_then(|x| x.as_str()).unwrap_or("")).unwrap_or_default());
                for rn in rules_of_test(t) { let e = rule_tests.entry(rn).or_default(); if !e.contains(&tn.to_string()) { e.push(tn.to_string()); } }
            }
        }
    }
    let fx_event = |fx_ref: &str| -> Option<String> { let key = fx_ref.rsplit('/').next().unwrap_or(""); fixtures_map.and_then(|m| m.get(key)).and_then(|fx| fx.get("type")).and_then(|t| t.get("$ref")).and_then(|x| x.as_str()).and_then(ref_name) };
    let ev_links = |arr: Option<&Value>| -> String { arr.and_then(|x| x.as_sequence()).map(|s| s.iter().map(|it| it.get("$ref").and_then(|x| x.as_str()).and_then(|r| fx_event(r)).map(|e| h_link("event", &e)).unwrap_or_else(|| "—".to_string())).collect::<Vec<_>>().join(", ")).unwrap_or_default() };
    let test_docs: Vec<HDoc> = actors.iter().filter_map(|a| {
        let entries: Vec<(String, Value)> = tests_map.map(|m| m.iter().filter(|(_, t)| ref_name(t.get("actor").and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("")).as_deref() == Some(a.name.as_str())).filter_map(|(k, t)| k.as_str().map(|s| (s.to_string(), t.clone()))).collect()).unwrap_or_default();
        if entries.is_empty() { return None; }
        let cases = entries.iter().map(|(name, t)| {
            // THE `when:` MESSAGE IS NOT ALWAYS A COMMAND. `$ref` names a KIND
            // (`commands.yaml#/X` vs `events.yaml#/X`), and 59 of this repo's tests are driven by
            // an INBOUND (integration) event -- an external fact that already happened, recorded
            // through the ACL with no command at all (CLAUDE.md, ADR-0004). This line rendered
            // every `when:` as a command, so those 59 got the command emoji and a command anchor.
            // This is the HTML sibling of the markdown fix above: same defect, same derivation.
            // `tools/link-check.py` scans markdown only, so nothing would have caught this half.
            let when_ref = t.get("when").and_then(|w| w.get("type")).and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or("");
            let when_kind = if when_ref.starts_with("events.yaml#/") { "event" } else { "command" };
            let cmd = ref_name(when_ref).unwrap_or_else(|| "?".to_string());
            let given = { let g = t.get("given"); if g.and_then(|x| x.as_sequence()).map(|s| !s.is_empty()).unwrap_or(false) { ev_links(g) } else { "<span class=\"muted\">(none)</span>".to_string() } };
            let has_thrown = t.get("thrown").is_some();
            let outcome = if has_thrown {
                format!("<div class=\"rel\"><span class=\"lbl\">thrown:</span> {}</div>", { let s = t.get("thrown").and_then(|x| x.as_sequence()).map(|arr| arr.iter().filter_map(|r| r.get("$ref").and_then(|x| x.as_str()).and_then(ref_name)).map(|e| h_link("error", &e)).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "—".to_string() } else { s } })
            } else {
                let then_arr = t.get("then");
                format!("<div class=\"rel\"><span class=\"lbl\">then:</span> {}</div>", if then_arr.and_then(|x| x.as_sequence()).map(|s| !s.is_empty()).unwrap_or(false) { ev_links(then_arr) } else { "<span class=\"k-const\">∅ no event (idempotent no-op)</span>".to_string() })
            };
            let rules = rules_of_test(t).iter().map(|rn| h_link("rule", rn)).collect::<Vec<_>>().join(", ");
            let body = format!("<div class=\"rel\"><span class=\"lbl\">given:</span> {}</div><div class=\"rel\"><span class=\"lbl\">when:</span> {}</div>{}{}", given, h_link(when_kind, &cmd), outcome, if rules.is_empty() { String::new() } else { format!("<div class=\"rel\"><span class=\"lbl\">verifies:</span> {}</div>", rules) });
            h_item("test", "Test", name, &body, t.get("name").and_then(|x| x.as_str()))
        }).collect::<Vec<_>>().join("");
        Some(HDoc { ctx: cx.of_actor(&a.name), html: format!("<h3>{}</h3>{}", h_link("actor", &a.name), cases) })
    }).collect();
    let rule_docs: Vec<HDoc> = rule_defs.map(|m| m.iter().filter_map(|(k, r)| k.as_str().map(|name| {
        let tns = rule_tests.get(name).cloned().unwrap_or_default();
        let ctx = tns.first().map(|tn| cx.of_actor(test_actor_name.get(tn).map(|s| s.as_str()).unwrap_or(""))).unwrap_or_else(|| CROSS.to_string());
        let verified_by = { let s = tns.iter().map(|tn| h_link("test", tn)).collect::<Vec<_>>().join(", "); if s.is_empty() { "—".to_string() } else { s } };
        HDoc { ctx, html: h_item("rule", "Rule", name, &format!("<div class=\"rel\"><span class=\"lbl\">verified by:</span> {}</div>", verified_by), Some(&ws1(r.get("description").and_then(|x| x.as_str()).unwrap_or("").trim()))) }
    })).collect()).unwrap_or_default();

    // 11. Observability
    let obs_docs: Vec<HDoc> = model.defs.get("observability.yaml").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, c)| k.as_str().map(|feature| {
        let wf = c.get("workflow");
        let id_rows: Vec<Vec<String>> = c.get("run_identity").and_then(|x| x.as_sequence()).map(|s| s.iter().map(|i| vec![format!("<span class=\"k-prop\">{}</span>", h_esc(i.get("name").and_then(|x| x.as_str()).unwrap_or(""))), format!("<span class=\"muted\">{}</span>", h_esc(i.get("source").and_then(|x| x.as_str()).unwrap_or(""))), if i.get("required").and_then(|x| x.as_bool()) == Some(true) { "<span class=\"req\">✅</span>".into() } else { "<span class=\"opt\">⬜</span>".into() }, i.get("businessKey").and_then(|b| b.get("$ref")).and_then(|x| x.as_str()).map(h_any_link).unwrap_or_else(|| "—".to_string())]).collect()).unwrap_or_default();
        let span_rows: Vec<Vec<String>> = c.get("spans").and_then(|x| x.as_sequence()).map(|s| s.iter().map(|sp| { let a = sp.get("attributes").and_then(|x| x.as_sequence()).map(|at| at.iter().map(|x| format!("<span class=\"k-prop\">{}</span>{}", h_esc(x.get("key").and_then(|k| k.as_str()).unwrap_or("")), if x.get("required").and_then(|r| r.as_bool()) == Some(true) { "<span class=\"req\">*</span>" } else { "" })).collect::<Vec<_>>().join(", ")).unwrap_or_default(); let a = if a.is_empty() { "—".to_string() } else { a }; vec![format!("<span class=\"k-op\">{}</span>", h_esc(sp.get("name").and_then(|x| x.as_str()).unwrap_or(""))), format!("<span class=\"kw\">{}</span>", h_esc(sp.get("kind").and_then(|x| x.as_str()).unwrap_or(""))), if sp.get("required").and_then(|x| x.as_bool()) == Some(true) { "<span class=\"req\">✅</span>".into() } else { "<span class=\"opt\">⬜</span>".into() }, sp.get("multiplicity").and_then(|x| x.as_str()).map(|mu| format!("<span class=\"muted\">{}</span>", h_esc(mu))).unwrap_or_else(|| "—".to_string()), a] }).collect()).unwrap_or_default();
        let metric_list = |key: &str| -> String { let s = c.get(key).and_then(|x| x.as_sequence()).map(|arr| arr.iter().map(|mm| format!("<span class=\"k-const\">{}</span> <span class=\"muted\">({})</span>", h_esc(mm.get("name").and_then(|x| x.as_str()).unwrap_or("")), h_esc(mm.get("type").and_then(|x| x.as_str()).unwrap_or("")))).collect::<Vec<_>>().join(", ")).unwrap_or_default(); if s.is_empty() { "—".to_string() } else { s } };
        let req_spans = c.get("status_rules").and_then(|x| x.get("success")).and_then(|x| x.get("required_spans")).and_then(|x| x.as_sequence()).map(|a| a.iter().map(|t| required_span_term(t, &|s| format!("<span class=\"k-op\">{}</span>", h_esc(s)))).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        let s3 = |v: Option<&Value>, k: &str| c.get(v.map(|_| "").unwrap_or("")).map(|_| "").unwrap_or("").to_string() + &{ let node = c.get(k); let _ = node; String::new() };
        let _ = s3;
        let slo = |group: &str, key: &str| -> String { c.get(group).and_then(|g| g.get(key)).map(|x| if let Some(n) = x.as_i64() { n.to_string() } else if let Some(f) = x.as_f64() { f.to_string() } else { x.as_str().unwrap_or("—").to_string() }).unwrap_or_else(|| "—".to_string()) };
        let cmd = ref_name(wf.and_then(|w| w.get("command")).and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or(""));
        let saga = ref_name(wf.and_then(|w| w.get("saga")).and_then(|x| x.get("$ref")).and_then(|x| x.as_str()).unwrap_or(""));
        let ctx = if let Some(c) = &cmd { cx.of_command(c) } else if let Some(s) = &saga { cx.of_actor(s) } else { CROSS.to_string() };
        let body = format!(
            "<div class=\"rel\"><span class=\"lbl\">workflow:</span> {}{}{}</div><div class=\"rel\"><span class=\"lbl\">emits:</span> {} · <span class=\"lbl\">inbound:</span> {}</div>{}{}<div class=\"rel\"><span class=\"lbl\">metrics:</span> {} · <span class=\"lbl\">business:</span> {}</div>{}<div class=\"rel\"><span class=\"lbl\">SLOs:</span> p95 ≤ {}ms · p99 ≤ {}ms · error ≤ {}%</div>",
            wf.and_then(|w| w.get("surface")).and_then(|s| s.as_str()).map(|s| format!("surface <span class=\"kw\">{}</span> <span class=\"muted\">(dispatch pipeline)</span>", h_esc(s))).unwrap_or_default(),
            wf.and_then(|w| w.get("saga")).map(|s| format!("saga {}", h_any_link(s.get("$ref").and_then(|x| x.as_str()).unwrap_or_default()))).unwrap_or_default(),
            wf.and_then(|w| w.get("command")).map(|c| format!(" · command {}", h_any_link(c.get("$ref").and_then(|x| x.as_str()).unwrap_or_default()))).unwrap_or_default(),
            h_ref_links(wf.and_then(|w| w.get("emits"))), h_ref_links(wf.and_then(|w| w.get("inbound"))),
            if id_rows.is_empty() { String::new() } else { format!("<div class=\"rel\"><span class=\"lbl\">run identity</span></div>{}", h_table(&["Id", "Source", "Req.", "Business key"], &id_rows)) },
            if span_rows.is_empty() { String::new() } else { format!("<div class=\"rel\"><span class=\"lbl\">spans</span> <span class=\"muted\">(* = required attribute)</span></div>{}", h_table(&["Span", "Kind", "Req.", "Multiplicity", "Attributes"], &span_rows)) },
            metric_list("metrics"), metric_list("business_metrics"),
            if req_spans.is_empty() { String::new() } else { format!("<div class=\"rel\"><span class=\"lbl\">success ⇐ spans:</span> {}</div>", req_spans) },
            slo("latency_budget", "max_p95_ms"), slo("latency_budget", "max_p99_ms"), slo("error_budget", "max_error_rate_pct")
        );
        HDoc { ctx, html: h_item("obs", "Contract", feature, &body, Some(&format!("criticality: {}", c.get("criticality").and_then(|x| x.as_str()).unwrap_or("—")))) }
    })).collect()).unwrap_or_default();

    // 12. C4
    let l2 = model.defs.get("architecture/c4-l2.yaml");
    let l3 = model.defs.get("architecture/c4-l3.yaml");
    let sysn = l2.and_then(|v| v.get("system")).and_then(|s| s.get("name")).and_then(|x| x.as_str()).unwrap_or("Captain.Food");
    let sysd = l2.and_then(|v| v.get("system")).and_then(|s| s.get("description")).and_then(|x| x.as_str()).unwrap_or("");
    let mrows = |sect: &str, f: &dyn Fn(&str, &Value) -> Vec<String>| -> Vec<Vec<String>> { l2.and_then(|v| v.get(sect)).and_then(|x| x.as_mapping()).map(|m| m.iter().filter_map(|(k, v)| k.as_str().map(|n| f(n, v))).collect()).unwrap_or_default() };
    let bc_rows = mrows("boundedContexts", &|n, bc| vec![format!("{} <span class=\"k-type\">{}</span>", d_emo("context"), h_esc(n)), h_esc(bc.get("description").and_then(|x| x.as_str()).unwrap_or("")), format!("{}{}", h_ref_links(bc.get("aggregates")), if bc.get("processManagers").is_some() { format!(" · {}", h_ref_links(bc.get("processManagers"))) } else { String::new() })]);
    let c_rows = mrows("containers", &|n, c| vec![format!("{} <span class=\"k-type\">{}</span>", d_emo("container"), h_esc(n)), format!("<span class=\"muted\">{}</span>", h_esc(c.get("technology").and_then(|x| x.as_str()).unwrap_or(""))), format!("{}{}", h_esc(c.get("description").and_then(|x| x.as_str()).unwrap_or("")), if c.get("realizes").is_some() { format!("<br>realizes: {}", h_ref_links(c.get("realizes"))) } else { String::new() })]);
    let x_rows = mrows("externalSystems", &|n, x| vec![format!("🔌 <span class=\"k-id\">{}</span>", h_esc(n)), h_esc(x.get("description").and_then(|d| d.as_str()).unwrap_or(""))]);
    let rel_rows: Vec<Vec<String>> = l2.and_then(|v| v.get("relationships")).and_then(|x| x.as_sequence()).map(|s| s.iter().map(|r| vec![format!("<span class=\"k-id\">{}</span> → <span class=\"k-id\">{}</span>", h_esc(r.get("from").and_then(|x| x.as_str()).unwrap_or("")), h_esc(r.get("to").and_then(|x| x.as_str()).unwrap_or(""))), h_esc(r.get("description").and_then(|x| x.as_str()).unwrap_or(""))]).collect()).unwrap_or_default();
    let comp_rows: Vec<Vec<String>> = l3.and_then(|v| v.get("components")).and_then(|x| x.as_mapping()).map(|m| m.iter().filter_map(|(k, c)| k.as_str().map(|n| { let mut binds: Vec<String> = Vec::new(); if c.get("handles").is_some() { binds.push(format!("handles {}", h_ref_links(c.get("handles")))); } if c.get("updates").is_some() { binds.push(format!("updates {}", h_ref_links(c.get("updates")))); } if c.get("reads").is_some() { binds.push(format!("reads {}", h_ref_links(c.get("reads")))); } let bind = if binds.is_empty() { "—".to_string() } else { binds.join("<br>") }; vec![format!("{} <span class=\"k-op\">{}</span>", d_emo("component"), h_esc(n)), if c.get("instrumented").and_then(|x| x.as_bool()) == Some(true) { "📡 yes".to_string() } else { "<span class=\"muted\">— no</span>".to_string() }, h_esc(c.get("description").and_then(|x| x.as_str()).unwrap_or("")), bind] })).collect()).unwrap_or_default();
    let c4_html = format!("<div class=\"rel\"><span class=\"lbl\">system:</span> <span class=\"k-type\">{}</span> — {}</div><h3>🔲 L2 — Bounded contexts</h3>{}<h3>🧱 L2 — Containers</h3>{}<h3>🔌 L2 — External systems</h3>{}<h3>➡️ L2 — Relationships</h3>{}<h3>⚙️ L3 — Components of the api container</h3>{}",
        h_esc(sysn), h_esc(sysd),
        h_table(&["Context", "Description", "Aggregates / process managers"], &bc_rows),
        h_table(&["Container", "Technology", "Description"], &c_rows),
        h_table(&["System", "Description"], &x_rows),
        h_table(&["Edge", "Description"], &rel_rows),
        h_table(&["Component", "Instrumented", "Description", "Binds"], &comp_rows));

    // 13. Interactive map data
    let screens_files: Vec<String> = model.defs.keys().filter(|k| k.starts_with("screens/")).cloned().collect();
    let l2m = |k: &str| l2.and_then(|v| v.get(k));
    let contexts_j: Vec<serde_json::Value> = l2m("boundedContexts").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, bc)| k.as_str().map(|id| serde_json::json!({"id": id, "description": bc.get("description").and_then(|x| x.as_str()).unwrap_or(""), "aggregates": ref_names(bc.get("aggregates")), "processManagers": ref_names(bc.get("processManagers"))}))).collect()).unwrap_or_default();
    let containers_j: Vec<serde_json::Value> = l2m("containers").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, c)| k.as_str().map(|id| serde_json::json!({"id": id, "technology": c.get("technology").and_then(|x| x.as_str()).unwrap_or(""), "description": c.get("description").and_then(|x| x.as_str()).unwrap_or(""), "realizes": ref_names(c.get("realizes"))}))).collect()).unwrap_or_default();
    let externals_j: Vec<serde_json::Value> = l2m("externalSystems").and_then(|v| v.as_mapping()).map(|m| m.iter().filter_map(|(k, x)| k.as_str().map(|id| serde_json::json!({"id": id, "description": x.get("description").and_then(|d| d.as_str()).unwrap_or("")}))).collect()).unwrap_or_default();
    let rels_j: Vec<serde_json::Value> = l2m("relationships").and_then(|x| x.as_sequence()).map(|s| s.iter().map(|r| serde_json::json!({"from": r.get("from").and_then(|x| x.as_str()).unwrap_or(""), "to": r.get("to").and_then(|x| x.as_str()).unwrap_or(""), "description": r.get("description").and_then(|x| x.as_str()).unwrap_or("")})).collect()).unwrap_or_default();
    let mut actors_obj = serde_json::Map::new();
    for a in &actors {
        let receives: Vec<serde_json::Value> = a.receives.iter().map(|e| serde_json::json!({"message": ref_name(&e.message_ref), "isCommand": e.message_ref.starts_with("commands.yaml#/"), "emits": e.emits.iter().filter_map(|r| ref_name(r)).collect::<Vec<_>>(), "throws": e.throws.iter().filter_map(|r| ref_name(r)).collect::<Vec<_>>()})).collect();
        actors_obj.insert(a.name.clone(), serde_json::json!({"type": a.kind, "receives": receives}));
    }
    let views_j: Vec<serde_json::Value> = views.iter().map(|v| serde_json::json!({"name": v.name, "fedBy": v.fedby.clone()})).collect();
    let map_data = serde_json::json!({"system": {"name": sysn, "description": sysd}, "contexts": contexts_j, "containers": containers_j, "externals": externals_j, "relationships": rels_j, "actors": serde_json::Value::Object(actors_obj), "views": views_j});
    let map_html = format!("<div class=\"cfmap\"><div class=\"cfmap-bar\"><button id=\"cf-back\">◀ back</button> <span id=\"cf-crumb\" class=\"muted\"></span></div><svg id=\"cf-svg\" viewBox=\"0 0 960 560\" preserveAspectRatio=\"xMidYMid meet\" role=\"img\" aria-label=\"Captain.Food system map\"></svg><div id=\"cf-info\" class=\"cfmap-info muted\"></div></div><script>{}</script>", MAP_JS.replace("__CF_DATA__", &serde_json::to_string(&map_data).unwrap()));

    // legend + toc
    let legend = [
        format!("{} <span class=\"k-op\">query</span>", d_emo("query")), format!("{} <span class=\"k-op\">mutation</span>", d_emo("mutation")), format!("{} <span class=\"k-op\">subscription</span>", d_emo("subscription")),
        format!("{} <span class=\"k-type\">type</span>", d_emo("type")), format!("{} <span class=\"k-type\">actor</span>", d_emo("actor")),
        format!("{} <span class=\"k-type\">view</span>", d_emo("view")), format!("{} <span class=\"k-op\">command</span>", d_emo("command")),
        format!("{} <span class=\"k-event\">event</span>", d_emo("event")), format!("{} <span class=\"k-type\">entity</span>", d_emo("entity")),
        format!("{} <span class=\"k-scalar\">scalar</span>", d_emo("scalar")), format!("{} <span class=\"k-error\">error</span>", d_emo("error")),
        "🔹 <span class=\"k-prop\">property</span>".to_string(), "<span class=\"k-param\">parameter</span>".to_string(), format!("{} <span class=\"k-scalar\">rule</span>", d_emo("rule")), format!("{} <span class=\"k-op\">test</span>", d_emo("test")), format!("{} <span class=\"k-type\">screen</span>", d_emo("screen")), format!("{} <span class=\"k-scalar\">translation</span>", d_emo("translation")), format!("{} <span class=\"k-event\">observability</span>", d_emo("obs")),
    ].join(" · ");

    // SDUI screens + translations — generic over every screens/*.yaml surface (ADR-20260722-091500):
    // one screens block per surface under a header. tr_en/t_text/op_link/collect_action_types are
    // surface-independent; resolvers/actions/screens are read per surface inside the loop.
    // translations merged from translations.yaml + screens/*.translations.yaml (translation_entries)
    let tr_en = |rf: &str| -> String { resolve_ref(model, rf, "translations.yaml").and_then(|t| t.get("messages")).and_then(|m| m.get("en")).and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or_else(|| rf.rsplit('/').next().unwrap_or(rf).to_string()) };
    let t_text = |v: &Value| -> String { if let Some(rf) = v.get("$ref").and_then(|x| x.as_str()) { tr_en(rf) } else if let Some(s) = v.as_str() { s.to_string() } else { String::new() } };
    let tr_rows: Vec<Vec<String>> = translation_entries(model).into_iter().map(|(_f, key, t)| { let params = t.get("params").and_then(|x| x.as_mapping()).map(|pm| pm.iter().filter_map(|(pk, _)| pk.as_str().map(|p| format!("<span class=\"k-param\">{}</span>", h_esc(p)))).collect::<Vec<_>>().join(", ")).unwrap_or_default(); let params = if params.is_empty() { "<span class=\"muted\">—</span>".to_string() } else { params }; vec![format!("<span id=\"{}\" class=\"k-scalar\">{} {}</span>", danchor("translation", &key), d_emo("translation"), h_esc(&key)), params, format!("🇬🇧 {}", h_esc(t.get("messages").and_then(|mm| mm.get("en")).and_then(|x| x.as_str()).unwrap_or(""))), format!("🇫🇷 {}", h_esc(t.get("messages").and_then(|mm| mm.get("fr")).and_then(|x| x.as_str()).unwrap_or("")))] }).collect();
    let translations_html = h_table(&["Key", "Params", "en", "fr"], &tr_rows);
    let op_link = |rf: Option<&str>, gap: Option<&str>| -> String { if let Some(g) = gap { return format!("<span class=\"opt\">⚠️ {}</span>", h_esc(g)); } match rf { None => "—".to_string(), Some(rf) => { let name = rf.rsplit('/').next().unwrap_or(""); let kind = if rf.contains("/mutations/") { "mutation" } else if rf.contains("/subscriptions/") { "subscription" } else { "query" }; h_link(kind, name) } } };
    fn collect_action_types(node: &Value, keys: &HashSet<String>, acc: &mut Vec<String>) {
        match node {
            Value::Sequence(s) => s.iter().for_each(|n| collect_action_types(n, keys, acc)),
            Value::Mapping(m) => { if let Some(t) = m.get(Value::String("type".into())).and_then(|x| x.as_str()) { if keys.contains(t) && !acc.contains(&t.to_string()) { acc.push(t.to_string()); } } for (_, v) in m { collect_action_types(v, keys, acc); } }
            _ => {}
        }
    }
    let mut all_screens: Vec<Value> = Vec::new();
    let mut screens_html = String::new();
    for sfkey in &screens_files {
        let sf = model.defs.get(sfkey);
        let resolvers = sf.and_then(|v| v.get("resolvers")).and_then(|v| v.as_mapping());
        let action_defs = sf.and_then(|v| v.get("actions")).and_then(|v| v.as_mapping());
        let action_keys: HashSet<String> = action_defs.map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
        let screens_arr = sf.and_then(|v| v.get("screens")).and_then(|x| x.as_sequence()).cloned().unwrap_or_default();
        let surface = sfkey.strip_prefix("screens/").unwrap_or(sfkey);
        screens_html.push_str(&format!("<p class=\"muted\">Surface <strong>{}</strong></p>", h_esc(surface)));
        let block: String = screens_arr.iter().map(|s| {
            let id = s.get("id").and_then(|x| x.as_str()).unwrap_or("?");
            let route = s.get("route").and_then(|x| x.as_str()).unwrap_or("");
            let title = { let t = s.get("title").map(|v| t_text(v)).unwrap_or_default(); if t.is_empty() { id.to_string() } else { t } };
            let not_sdui = s.get("sdui").and_then(|x| x.as_bool()) == Some(false);
            let badge = if not_sdui { "<span class=\"badge\">🚫 not SDUI</span>".to_string() } else { "<span class=\"badge\">📱 SDUI</span>".to_string() };
            let auth = if s.get("requires_auth").and_then(|x| x.as_bool()) == Some(true) { "<span class=\"badge\">🔒 auth</span>" } else { "" };
            let reason = if not_sdui { s.get("sdui_reason").and_then(|x| x.as_str()).map(|r| format!("<div class=\"desc\">{}</div>", h_esc(r))).unwrap_or_default() } else { String::new() };
            let mock_rows = s.get("components").and_then(|x| x.as_sequence()).map(|comps| comps.iter().map(|c| { let t = if let Some(cp) = c.get("component").and_then(|x| x.as_str()) { format!("«{}»", cp) } else { c.get("type").and_then(|x| x.as_str()).unwrap_or("?").to_string() }; let lbl = c.get("title").map(|v| t_text(v)).filter(|s| !s.is_empty()).or_else(|| c.get("label").map(|v| t_text(v)).filter(|s| !s.is_empty())).or_else(|| c.get("placeholder").map(|v| t_text(v)).filter(|s| !s.is_empty())).unwrap_or_default(); format!("<div style=\"padding:5px 10px;border-top:1px solid var(--line)\"><span class=\"muted\">{}</span>{}</div>", h_esc(&t), if lbl.is_empty() { String::new() } else { format!(" {}", h_esc(&lbl)) }) }).collect::<Vec<_>>().join("")).unwrap_or_default();
            let mock = format!("<div style=\"border:1px solid var(--line);border-radius:12px;max-width:340px;overflow:hidden;margin:8px 0\"><div style=\"background:var(--bg3);padding:7px 10px;font-weight:600\">📱 {}<span class=\"muted\"> · {}</span></div>{}</div>", h_esc(&title), h_esc(route), mock_rows);
            let mut rows: Vec<Vec<String>> = Vec::new();
            for rn in s.get("data_requirements").and_then(|x| x.as_sequence()).map(|s| s.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect::<Vec<_>>()).unwrap_or_default() {
                let r = resolvers.and_then(|m| m.get(rn.as_str()));
                rows.push(vec!["<span class=\"muted\">read</span>".to_string(), format!("<span class=\"k-op\">{}</span>", h_esc(&rn)), op_link(r.and_then(|x| x.get("query")).and_then(|q| q.get("$ref")).and_then(|x| x.as_str()), r.and_then(|x| x.get("gap")).and_then(|x| x.as_str()))]);
            }
            let mut acts: Vec<String> = Vec::new();
            if let Some(comps) = s.get("components") { collect_action_types(comps, &action_keys, &mut acts); }
            for a in s.get("actions_used").and_then(|x| x.as_sequence()).map(|s| s.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect::<Vec<_>>()).unwrap_or_default() { if !acts.contains(&a) { acts.push(a); } }
            for a in &acts {
                let ad = action_defs.and_then(|m| m.get(a.as_str()));
                if ad.map(|x| x.get("mutation").is_some() || x.get("gap").is_some()).unwrap_or(false) {
                    rows.push(vec!["<span class=\"muted\">write</span>".to_string(), format!("<span class=\"k-op\">{}</span>", h_esc(a)), op_link(ad.and_then(|x| x.get("mutation")).and_then(|q| q.get("$ref")).and_then(|x| x.as_str()), ad.and_then(|x| x.get("gap")).and_then(|x| x.as_str()))]);
                }
            }
            let ops_table = h_table(&["", "UI need", "GraphQL operation"], &rows);
            let gaps = s.get("gaps").and_then(|x| x.as_sequence()).map(|g| g.iter().filter_map(|x| x.as_str()).map(|g| format!("<li>⚠️ {}</li>", h_esc(g))).collect::<Vec<_>>().join("")).unwrap_or_default();
            let body = format!("{}<div style=\"display:flex;gap:20px;flex-wrap:wrap;align-items:flex-start\">{}<div style=\"flex:1;min-width:280px\">{}{}</div></div>", reason, mock, ops_table, if gaps.is_empty() { String::new() } else { format!("<p class=\"muted\">Gaps</p><ul>{}</ul>", gaps) });
            format!("<details class=\"item\" id=\"{}\" data-crumb=\"{} {}\" open><summary><span class=\"tw\">▸</span><span class=\"muted\">Screen:</span> <span class=\"k-type\">{} {}</span> <span class=\"muted\">{}</span> {}{}<a class=\"perma\" href=\"#{}\">🔗</a></summary>{}</details>", danchor("screen", id), d_emo("screen"), h_esc(id), d_emo("screen"), h_esc(id), h_esc(route), badge, auth, danchor("screen", id), body)
        }).collect();
        screens_html.push_str(&block);
        all_screens.extend(screens_arr);
    }

    // descIndex (insertion order preserved via serde_json preserve_order Map)
    let mut desc_map = serde_json::Map::new();
    let mut put = |k: &str, name: &str, val: &str| { desc_map.insert(danchor(k, name), serde_json::Value::String(ws1(val.trim()))); };
    if let Some(m) = model.defs.get("scalars.yaml").and_then(|v| v.as_mapping()) { for (k, d) in m { if let Some(n) = k.as_str() { put("scalar", n, d.get("description").and_then(|x| x.as_str()).unwrap_or("")); } } }
    if let Some(m) = ent_map { for (k, _) in m { if let Some(n) = k.as_str() { let d = doc_desc(model, "entities.yaml", n); put("entity", n, &d); } } }
    if let Some(m) = evt_map { for (k, _) in m { if let Some(n) = k.as_str() { let d = doc_desc(model, "events.yaml", n); put("event", n, &d); } } }
    if let Some(m) = cmd_map { for (k, _) in m { if let Some(n) = k.as_str() { let d = doc_desc(model, "commands.yaml", n); put("command", n, &d); } } }
    if let Some(m) = model.defs.get("errors.yaml").and_then(|v| v.as_mapping()) { for (k, d) in m { if let Some(n) = k.as_str() { put("error", n, d.get("description").and_then(|x| x.as_str()).unwrap_or("")); } } }
    for a in &actors { put("actor", &a.name, a.description.as_deref().unwrap_or("")); }
    for v in &views { put("view", &v.name, v.note.as_deref().unwrap_or("")); }
    for t in &api.types { put("type", &t.name, t.description.as_deref().unwrap_or("")); }
    for q in &api.queries { put("query", &q.name, q.description.as_deref().unwrap_or("")); }
    for m in &api.mutations { let d = doc_desc(model, "commands.yaml", &m.command); put("mutation", &m.name, &d); }
    for s in &api.subscriptions { put("subscription", &s.name, s.description.as_deref().unwrap_or("")); }
    if let Some(m) = model.defs.get("observability.yaml").and_then(|v| v.as_mapping()) { for (k, c) in m { if let Some(f) = k.as_str() { let s = format!("Observability contract — criticality: {}.", c.get("criticality").and_then(|x| x.as_str()).unwrap_or("—")); put("obs", f, &s); } } }
    if let Some(m) = rule_defs { for (k, d) in m { if let Some(n) = k.as_str() { put("rule", n, d.get("description").and_then(|x| x.as_str()).unwrap_or("")); } } }
    for (_f, key, t) in translation_entries(model) { let s = format!("{} / {}", t.get("messages").and_then(|mm| mm.get("en")).and_then(|x| x.as_str()).unwrap_or(""), t.get("messages").and_then(|mm| mm.get("fr")).and_then(|x| x.as_str()).unwrap_or("")); put("translation", &key, &s); }
    for s in &all_screens { if let Some(id) = s.get("id").and_then(|x| x.as_str()) { let msg = format!("{}screen {}", if s.get("sdui").and_then(|x| x.as_bool()) == Some(false) { "Non-SDUI " } else { "SDUI " }, s.get("route").and_then(|x| x.as_str()).unwrap_or("")); put("screen", id, &msg); } }
    drop(put);
    let desc_script = format!("<script>window.CF_DESC={};</script>", serde_json::to_string(&serde_json::Value::Object(desc_map)).unwrap().replace('<', "\\u003c"));

    // assembly
    let in_ctx = |docs: &[HDoc], ctx: &str| -> String { docs.iter().filter(|d| d.ctx == ctx).map(|d| d.html.clone()).collect::<Vec<_>>().join("") };
    let doc_sub = |emoji: &str, title: &str, docs: &[HDoc], ctx: &str| -> String { let n = docs.iter().filter(|d| d.ctx == ctx).count(); if n == 0 { String::new() } else { h_subsec(emoji, title, n, &in_ctx(docs, ctx)) } };
    let table_sub = |emoji: &str, title: &str, head: &[&str], rows: &[HRow], ctx: &str| -> String { let r: Vec<&HRow> = rows.iter().filter(|x| x.ctx == ctx).collect(); if r.is_empty() { String::new() } else { h_subsec(emoji, title, r.len(), &h_table(head, &r.iter().map(|x| x.cells.clone()).collect::<Vec<_>>())) } };
    let mut ctx_sections = String::new();
    let mut ctx_toc = String::new();
    let mut i = 0usize;
    for ctx in &cx.order {
        let inner = format!("{}{}{}{}{}{}{}{}{}{}{}{}",
            doc_sub("🧰", "API operations", &api_docs, ctx),
            doc_sub(d_emo("type"), "Output types", &type_docs, ctx),
            doc_sub(d_emo("actor"), "Actors", &actor_docs, ctx),
            doc_sub(d_emo("view"), "Views", &view_docs, ctx),
            doc_sub(d_emo("command"), "Commands", &command_docs, ctx),
            doc_sub(d_emo("event"), "Events", &event_docs, ctx),
            doc_sub(d_emo("entity"), "Entities", &entity_docs, ctx),
            table_sub(d_emo("scalar"), "Scalars", &["Scalar", "Type", "Description"], &scalar_rows, ctx),
            table_sub(d_emo("error"), "Errors", &["Error", "Description", "Message (en)", "Message (fr)", "Thrown by"], &error_rows, ctx),
            doc_sub(d_emo("rule"), "Business rules", &rule_docs, ctx),
            doc_sub(d_emo("test"), "Tests", &test_docs, ctx),
            doc_sub(d_emo("obs"), "Observability", &obs_docs, ctx));
        if inner.is_empty() { continue; }
        i += 1;
        ctx_sections.push_str(&h_sec(&format!("ctx-{}", dslug(ctx)), d_emo("context"), &format!("{}. {}", i, ctx), &format!("<div class=\"desc\">{}</div>{}", h_esc(&cx.describe(ctx)), inner)));
        ctx_toc.push_str(&format!("<a href=\"#sec-ctx-{}\">{} {}</a>", dslug(ctx), d_emo("context"), h_esc(ctx)));
    }
    let toc = format!("<a href=\"#sec-stories\">🎬 Stories</a>{}<a href=\"#sec-screens\">📱 Screens</a><a href=\"#sec-translations\">🌐 Translations</a><a href=\"#sec-architecture\">🏛️ Architecture</a><a href=\"#sec-map\">🗺️ Map</a>", ctx_toc);
    let roles_line = "🌐 PUBLIC · 🙋 CUSTOMER · 🏪 RESTAURANT_ACCOUNT · 🍽️ RESTAURANT · 🛵 RIDER · 🛠️ ADMIN · 🔌 EXTERNAL";

    let mut out = String::new();
    out.push_str(THEME);
    out.push_str("\n<div class=\"doc\"><div class=\"wrap\">\n  <div id=\"cf-crumb\" class=\"crumb\"></div>\n  <h1>📖 Captain.Food — Product Documentation</h1>\n  <p class=\"muted\">Generated from the specs, organized <strong>top-level by bounded context</strong> (🔲). The bar above shows where you are (context › section › item — click to jump); hover any link for its description. Every item is anchored — click 🔗 to copy a deep link. Sections are collapsible.</p>\n  <p><strong>Kinds:</strong> ");
    out.push_str(&legend);
    out.push_str("</p>\n  <p><strong>Roles:</strong> ");
    out.push_str(roles_line);
    out.push_str("</p>\n  <div class=\"toolbar\"><button onclick=\"setAll(true)\">⊞ Expand all</button> <button onclick=\"setAll(false)\">⊟ Collapse all</button> &nbsp; <span class=\"toc\">");
    out.push_str(&toc);
    out.push_str("</span></div>\n  ");
    out.push_str(&h_sec("stories", "🎬", "Stories", &stories_html));
    out.push_str("\n  ");
    out.push_str(&ctx_sections);
    out.push_str("\n  ");
    out.push_str(&h_sec("screens", "📱", "Front-office screens (SDUI)", &(String::from("<p class=\"muted\">Server-Driven UI screens (specs/screens/*.yaml, one file per audience, ADR-0033/ADR-20260722-091500). Per screen, the reads (resolvers→queries) and writes (actions→mutations) are $ref-bound to the GraphQL API and validated — the mockups are the <strong>proof the API answers the UI</strong>. ⚠️ marks gaps the API does not serve yet; 🚫 screens are intentionally not SDUI-rendered.</p>") + &screens_html)));
    out.push_str("\n  ");
    out.push_str(&h_sec("translations", "🌐", "Translations", &(String::from("<p class=\"muted\">The i18n catalog (translations.yaml) — every screen string, referenced by $ref, generated to one translations.generated.json. {param} tokens are validated against declared params.</p>") + &translations_html)));
    out.push_str("\n  ");
    out.push_str(&h_sec("architecture", "🏛️", "Architecture (C4)", &c4_html));
    out.push_str("\n  ");
    out.push_str(&h_sec("map", "🗺️", "System map (interactive)", &(String::from("<p class=\"muted\">Drill in: <strong>System → container → bounded context → aggregate flow</strong>. Boxes are colored by kind (containers/aggregates teal, externals orange, contexts gold, commands yellow, events purple, views blue). Click to go deeper; leaf boxes jump to their section; use ◀ back to climb out.</p>") + &map_html)));
    out.push_str("\n</div></div>\n<div id=\"cf-tip\" class=\"cf-tip\"></div>\n");
    out.push_str(&desc_script);
    out.push('\n');
    out.push_str(NAV_JS);
    out.push('\n');
    out.push_str(MERMAID_JS);
    out
}

// ─── Bounded-context resolution (port of emit/contexts.ts) ──────────────────────────────────────

pub(crate) const CROSS: &str = "cross-cutting";

pub(crate) fn single(s: &HashSet<String>) -> String {
    if s.len() == 1 {
        s.iter().next().unwrap().clone()
    } else {
        CROSS.to_string()
    }
}

pub(crate) struct Cx {
    pub(crate) order: Vec<String>,
    pub(crate) descriptions: HashMap<String, String>,
    pub(crate) actor_ctx: HashMap<String, String>,
    pub(crate) role_ctx: HashMap<String, String>,
    pub(crate) cmd_actor: HashMap<String, String>,
    pub(crate) evt_emitter: HashMap<String, String>,
    pub(crate) evt_consumer: HashMap<String, String>,
    pub(crate) err_cmds: HashMap<String, HashSet<String>>,
    pub(crate) entity_ctx: HashMap<String, String>,
    pub(crate) scalar_ctx: HashMap<String, String>,
    pub(crate) view_agg: HashMap<String, (bool, String)>, // view name -> (is_reference, aggregate)
    pub(crate) type_reads: HashMap<String, Vec<String>>,
}

impl Cx {
    pub(crate) fn of_actor(&self, n: &str) -> String {
        self.actor_ctx.get(n).cloned().unwrap_or_else(|| CROSS.to_string())
    }
    pub(crate) fn of_view(&self, n: &str) -> String {
        match self.view_agg.get(n) {
            Some((false, agg)) => self.of_actor(agg),
            _ => CROSS.to_string(),
        }
    }
    pub(crate) fn of_reads(&self, reads: &[String]) -> String {
        reads.first().map(|r| self.of_view(r)).unwrap_or_else(|| CROSS.to_string())
    }
    pub(crate) fn of_command(&self, n: &str) -> String {
        match self.cmd_actor.get(n) {
            Some(a) => self.of_actor(a),
            None => CROSS.to_string(),
        }
    }
    pub(crate) fn of_event(&self, n: &str) -> String {
        match self.evt_emitter.get(n).or_else(|| self.evt_consumer.get(n)) {
            Some(a) => self.of_actor(a),
            None => CROSS.to_string(),
        }
    }
    pub(crate) fn of_type(&self, n: &str) -> String {
        match self.type_reads.get(n) {
            Some(r) => self.of_reads(r),
            None => CROSS.to_string(),
        }
    }
    pub(crate) fn of_error(&self, n: &str) -> String {
        match self.err_cmds.get(n) {
            None => CROSS.to_string(),
            Some(cmds) => single(&cmds.iter().map(|c| self.of_command(c)).filter(|c| c != CROSS).collect()),
        }
    }
    pub(crate) fn of_entity(&self, n: &str) -> String {
        self.entity_ctx.get(n).cloned().unwrap_or_else(|| CROSS.to_string())
    }
    pub(crate) fn of_scalar(&self, n: &str) -> String {
        self.scalar_ctx.get(n).cloned().unwrap_or_else(|| CROSS.to_string())
    }
    pub(crate) fn describe(&self, ctx: &str) -> String {
        self.descriptions.get(ctx).cloned().unwrap_or_default()
    }
    pub(crate) fn of_operation(&self, roles: &[String], fallback: &str) -> String {
        let performer: HashSet<String> = roles.iter().filter_map(|r| self.role_ctx.get(r).cloned()).collect();
        if performer.len() == 1 {
            performer.into_iter().next().unwrap()
        } else {
            fallback.to_string()
        }
    }
}

pub(crate) fn vote(m: &mut HashMap<String, HashSet<String>>, name: &str, ctx: &str) {
    if name.is_empty() || ctx == CROSS {
        return;
    }
    m.entry(name.to_string()).or_default().insert(ctx.to_string());
}

pub(crate) fn build_context_map(model: &Model, api: &Api, actors: &[Actor], views: &[SqlView]) -> Cx {
    let l2 = model.defs.get("architecture/c4-l2.yaml");
    let l2bc = l2.and_then(|v| v.get("boundedContexts")).and_then(|v| v.as_mapping());
    let mut order = Vec::new();
    let mut descriptions = HashMap::new();
    let mut actor_ctx = HashMap::new();
    let mut role_ctx = HashMap::new();
    if let Some(bcs) = l2bc {
        for (k, bc) in bcs {
            let cid = match k.as_str() {
                Some(s) => s.to_string(),
                None => continue,
            };
            order.push(cid.clone());
            descriptions.insert(cid.clone(), bc.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string());
            for key in ["aggregates", "processManagers"] {
                for n in ref_names(bc.get(key)) {
                    actor_ctx.insert(n, cid.clone());
                }
            }
            for role in bc.get("roles").and_then(|x| x.as_sequence()).map(|s| s.to_vec()).unwrap_or_default() {
                if let Some(r) = role.as_str() {
                    role_ctx.insert(r.to_string(), cid.clone());
                }
            }
        }
    }
    order.push(CROSS.to_string());
    descriptions.insert(CROSS.to_string(), "Shared vocabulary and operations that span several bounded contexts (or belong to none).".to_string());

    let mut cmd_actor = HashMap::new();
    let mut evt_emitter = HashMap::new();
    let mut evt_consumer = HashMap::new();
    let mut err_cmds: HashMap<String, HashSet<String>> = HashMap::new();
    for a in actors {
        for e in &a.receives {
            let msg = ref_name(&e.message_ref);
            if e.message_ref.starts_with("commands.yaml#/") {
                if let Some(m) = &msg {
                    cmd_actor.insert(m.clone(), a.name.clone());
                    for t in &e.throws {
                        if let Some(er) = ref_name(t) {
                            err_cmds.entry(er).or_default().insert(m.clone());
                        }
                    }
                }
            } else if e.message_ref.starts_with("events.yaml#/") {
                if let Some(m) = &msg {
                    evt_consumer.entry(m.clone()).or_insert_with(|| a.name.clone());
                }
            }
            for em in &e.emits {
                if let Some(ev) = ref_name(em) {
                    evt_emitter.entry(ev).or_insert_with(|| a.name.clone());
                }
            }
        }
    }

    let view_agg: HashMap<String, (bool, String)> =
        views.iter().map(|v| (v.name.clone(), (v.reference, v.aggregate.clone()))).collect();
    let type_reads: HashMap<String, Vec<String>> =
        api.types.iter().map(|t| (t.name.clone(), t.reads.clone())).collect();

    let mut cx = Cx {
        order,
        descriptions,
        actor_ctx,
        role_ctx,
        cmd_actor,
        evt_emitter,
        evt_consumer,
        err_cmds,
        entity_ctx: HashMap::new(),
        scalar_ctx: HashMap::new(),
        view_agg,
        type_reads,
    };

    // entities & scalars: attribute by usage across the strongly-anchored artifacts (voting).
    let scalar_names = scalar_names(model);
    let entity_names: Vec<String> = model
        .defs
        .get("entities.yaml")
        .and_then(|v| v.as_mapping())
        .map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let mut entity_votes: HashMap<String, HashSet<String>> = HashMap::new();
    let mut scalar_votes: HashMap<String, HashSet<String>> = HashMap::new();
    let vote_refs = |def: &Value, ctx: &str, sv: &mut HashMap<String, HashSet<String>>, ev: &mut HashMap<String, HashSet<String>>| {
        if ctx == CROSS {
            return;
        }
        let mut refs = Vec::new();
        collect_refs(def, "", &mut refs);
        for (_loc, r) in refs {
            if let Some(p) = parse_ref(&r) {
                if let Some(name) = p.path.first() {
                    if p.file == "scalars.yaml" {
                        vote(sv, name, ctx);
                    } else if p.file == "entities.yaml" || p.file.is_empty() {
                        vote(ev, name, ctx);
                    }
                }
            }
        }
    };

    let cmd_defs = model.defs.get("commands.yaml").and_then(|v| v.as_mapping());
    if let Some(m) = cmd_defs {
        for (k, def) in m {
            if let Some(c) = k.as_str() {
                vote_refs(def, &cx.of_command(c), &mut scalar_votes, &mut entity_votes);
            }
        }
    }
    if let Some(m) = model.defs.get("events.yaml").and_then(|v| v.as_mapping()) {
        for (k, def) in m {
            if let Some(ev) = k.as_str() {
                vote_refs(def, &cx.of_event(ev), &mut scalar_votes, &mut entity_votes);
            }
        }
    }
    if let Some(m) = model.defs.get("errors.yaml").and_then(|v| v.as_mapping()) {
        for (k, def) in m {
            if let Some(er) = k.as_str() {
                vote_refs(def, &cx.of_error(er), &mut scalar_votes, &mut entity_votes);
            }
        }
    }
    for t in &api.types {
        let ctx = cx.of_type(&t.name);
        for f in &t.properties {
            if f.is_ref {
                vote(if scalar_names.contains(&f.ty) { &mut scalar_votes } else { &mut entity_votes }, &f.ty, &ctx);
            }
        }
    }
    for q in api.queries.iter().chain(api.subscriptions.iter()) {
        let ctx = if !q.reads.is_empty() { cx.of_reads(&q.reads) } else { cx.of_type(&q.returns_type) };
        for a in &q.args {
            if a.is_ref {
                vote(if scalar_names.contains(&a.ty) { &mut scalar_votes } else { &mut entity_votes }, &a.ty, &ctx);
            }
        }
    }
    for m in &api.mutations {
        let ctx = cx.of_command(&m.command);
        for f in &m.payload {
            if f.is_ref {
                vote(if scalar_names.contains(&f.ty) { &mut scalar_votes } else { &mut entity_votes }, &f.ty, &ctx);
            }
        }
    }
    for v in views {
        let ctx = cx.of_view(&v.name);
        for col in &v.columns {
            if scalar_names.contains(&col.ty) {
                vote(&mut scalar_votes, &col.ty, &ctx);
            }
        }
    }

    // resolve entity context: aggregate-name match wins, else a single usage context
    let ent_defs = model.defs.get("entities.yaml").and_then(|v| v.as_mapping());
    let mut entity_ctx: HashMap<String, String> = HashMap::new();
    for e in &entity_names {
        let c = if cx.actor_ctx.contains_key(e) {
            cx.actor_ctx.get(e).unwrap().clone()
        } else {
            single(entity_votes.get(e).unwrap_or(&HashSet::new()))
        };
        entity_ctx.insert(e.clone(), c);
    }
    // anchored entities propagate their context to the entities & scalars they reference (one pass)
    for e in &entity_names {
        let ctx = entity_ctx.get(e).cloned().unwrap_or_else(|| CROSS.to_string());
        if ctx != CROSS {
            if let Some(def) = ent_defs.and_then(|m| m.get(e.as_str())) {
                vote_refs(def, &ctx, &mut scalar_votes, &mut entity_votes);
            }
        }
    }
    for e in &entity_names {
        if entity_ctx.get(e).map(|c| c == CROSS).unwrap_or(true) {
            entity_ctx.insert(e.clone(), single(entity_votes.get(e).unwrap_or(&HashSet::new())));
        }
    }
    let mut scalar_ctx: HashMap<String, String> = HashMap::new();
    for s in &scalar_names {
        scalar_ctx.insert(s.clone(), single(scalar_votes.get(s).unwrap_or(&HashSet::new())));
    }
    cx.entity_ctx = entity_ctx;
    cx.scalar_ctx = scalar_ctx;
    cx
}

// ─── stories (personas) — port of load.ts parseStories ──────────────────────────────────────────
pub(crate) struct StoryStep {
    pub(crate) name: String,
    pub(crate) op_kind: Option<String>,
    pub(crate) op: Option<String>,
    pub(crate) note: Option<String>,
}
pub(crate) struct StoryActivity {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) steps: Vec<StoryStep>,
}
pub(crate) struct Persona {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) role: String,
    pub(crate) locale: Option<String>,
    pub(crate) activities: Vec<StoryActivity>,
}

pub(crate) fn parse_stories(model: &Model) -> Vec<Persona> {
    let mut out = Vec::new();
    if let Some(m) = model.defs.get("stories.yaml").and_then(|v| v.as_mapping()) {
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            let has_role = node.get("personaRole").and_then(|x| x.as_str()).is_some();
            let has_acts = node.get("activities").map(|x| !x.is_null()).unwrap_or(false);
            if !has_role && !has_acts {
                continue;
            }
            let mut activities = Vec::new();
            if let Some(am) = node.get("activities").and_then(|x| x.as_mapping()) {
                for (ak, a) in am {
                    let aname = match ak.as_str() {
                        Some(s) => s,
                        None => continue,
                    };
                    let mut steps = Vec::new();
                    if let Some(sm) = a.get("steps").and_then(|x| x.as_mapping()) {
                        for (sk, s) in sm {
                            let sname = match sk.as_str() {
                                Some(x) => x.to_string(),
                                None => continue,
                            };
                            if let Some(rf) = s.get("$ref").and_then(|x| x.as_str()) {
                                let ptr = rf.splitn(2, "#/").nth(1).unwrap_or("");
                                let mut segs = ptr.split('/');
                                let seg0 = segs.next().unwrap_or("");
                                let op = segs.next().map(|s| s.to_string());
                                let op_kind = match seg0 {
                                    "queries" => Some("query".to_string()),
                                    "mutations" => Some("mutation".to_string()),
                                    _ => None,
                                };
                                steps.push(StoryStep { name: sname, op_kind, op, note: None });
                            } else {
                                steps.push(StoryStep {
                                    name: sname,
                                    op_kind: None,
                                    op: None,
                                    note: s.get("note").and_then(|x| x.as_str()).map(|x| x.to_string()),
                                });
                            }
                        }
                    }
                    activities.push(StoryActivity {
                        name: aname.to_string(),
                        description: a.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()),
                        steps,
                    });
                }
            }
            out.push(Persona {
                name: name.to_string(),
                description: node.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()),
                role: node.get("personaRole").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                locale: node.get("locale").and_then(|x| x.as_str()).map(|s| s.to_string()),
                activities,
            });
        }
    }
    out
}

