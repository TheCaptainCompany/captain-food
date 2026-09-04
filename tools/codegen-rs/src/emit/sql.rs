use crate::*;

// ─── views.generated.sql (port of emit/database.ts `emitViewsSql`) ──────────────────────────────
// Byte-identical CREATE TABLE + index DDL for every View_* (aggregate-fed or `source: reference`).

/// One arm of a `status-from-event-type` derivation: for a given event type, the column's value is
/// a literal enum value (`Lit`), extracted from that event's payload (`Payload(prop)`), the explicit
/// YAML `null` (#639 part C step 3-i, ADR-20260904-015903 §3) — RESET to NULL (`Null`): the event
/// CLEARS the column, and its type is in the CASE's event set so it wins over the last setter —
/// or, new with #639 part C step 3-ii (ADR-20260904-015903 §3, "the grammar extension of A.3,
/// mirrored"), `{ from: <field>, map: { <fieldValue>: <columnValue>, … } }` (`Mapped`): the column's
/// value depends on ANOTHER field of the SAME event, keyed through an explicit value→value map (a
/// custody-keyed status arm cannot be a straight `Payload` copy — the field's own scalar and the
/// column's are different enums). Before `Null` existed a YAML null fell out of the map silently, so
/// a "clearing" event left the previous value standing — the latent defect the ADR names, closed
/// together with the validator's `view-derive-value-unknown` (any other unrecognised value is an
/// ERROR).
#[derive(Clone)]
pub(crate) enum DeriveVal {
    Lit(String),
    Payload(String),
    Null,
    Mapped(String, Vec<(String, String)>),
}
pub(crate) struct SqlColumn {
    pub(crate) name: String,
    pub(crate) ty: String,
    pub(crate) pk: bool,
    pub(crate) unique: bool,
    pub(crate) index: bool,
    pub(crate) nullable: bool,
    pub(crate) fk: Option<String>, // "View_Name.column" — used by the GraphQL FK-navigation emitter
    pub(crate) note: Option<String>,
    pub(crate) from: Vec<String>,   // event/property $ref strings that populate the column
    pub(crate) type_derived: bool,  // type was derived from `from` (not declared explicitly)
    /// `status-from-event-type` derivation map (event_type → value), in declared order. Empty = none.
    pub(crate) derive: Vec<(String, DeriveVal)>,
    /// Conditional occurrence-time: `max(occurred_at)` over events matching any (event_type [+ payload
    /// equalities]) clause — e.g. delivered_at = when DeliveryCompleted OR DeliveryStatusUpdated=DELIVERED.
    pub(crate) occurred_when: Vec<(String, Vec<(String, String)>)>,
}
pub(crate) struct SqlView {
    pub(crate) name: String,
    pub(crate) aggregate: String,
    pub(crate) slice: String,
    pub(crate) internal: bool,
    pub(crate) reference: bool,
    pub(crate) filters: Vec<String>,
    pub(crate) rules: Vec<String>,
    pub(crate) note: Option<String>,
    pub(crate) fedby: Vec<String>,
    pub(crate) columns: Vec<SqlColumn>,
    pub(crate) indexes: Vec<Vec<String>>,
    /// true → a materialized read-model TABLE (projection_tables.yaml, fed by a projector); false → a
    /// generated fold VIEW (projection_views.yaml).
    pub(crate) is_table: bool,
    /// (table) how the table is maintained — always "app": an application-layer (Rust) projector,
    /// deferred until crates/ exists. No SQL triggers (ADR-0040).
    pub(crate) projector: Option<String>,
    /// Event type whose presence in the stream drops the row (soft-delete tombstone), if any.
    pub(crate) tombstone: Option<String>,
    /// Hand-written SQL override (escape hatch): when set, used verbatim instead of the generated fold.
    pub(crate) definition: Option<String>,
}

/// A foreign key `"View_Name.column"` — either a literal string or a `{ $ref: '#/View_X/columns/col' }`.
pub(crate) fn parse_fk(raw: Option<&Value>) -> Option<String> {
    match raw {
        Some(Value::String(s)) => Some(s.clone()),
        Some(v) => {
            if let Some(r) = v.get("$ref").and_then(|x| x.as_str()) {
                let segs: Vec<&str> =
                    r.splitn(2, "#/").nth(1).unwrap_or("").split('/').filter(|s| !s.is_empty()).collect();
                if segs.len() >= 2 {
                    return Some(format!("{}.{}", segs[0], segs[segs.len() - 1]));
                }
            }
            None
        }
        None => None,
    }
}

/// Explicit column `type`: a `$ref` into scalars.yaml (→ the scalar name) or an inline SQL primitive string.
pub(crate) fn column_type_explicit(raw: &Value) -> String {
    if let Some(r) = raw.get("$ref").and_then(|x| x.as_str()) {
        return r.splitn(2, "#/").nth(1).unwrap_or("").to_string();
    }
    match raw {
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

/// Map an events.yaml property schema node to the column type it implies (mirrors schemaNodeToColumnType).
pub(crate) fn schema_node_to_column_type(node: &Value) -> String {
    if let Some(r) = node.get("$ref").and_then(|x| x.as_str()) {
        let mut it = r.splitn(2, "#/");
        let file = it.next().unwrap_or("");
        let name = it.next().unwrap_or("");
        return if file == "scalars.yaml" {
            name.to_string()
        } else {
            "jsonb".to_string()
        };
    }
    match node.get("type").and_then(|x| x.as_str()) {
        Some("array") => "jsonb".into(),
        Some("integer") => "integer".into(),
        Some("number") => "numeric".into(),
        Some("boolean") => "boolean".into(),
        Some("string") => {
            if node.get("format").and_then(|x| x.as_str()) == Some("date-time") {
                "timestamptz".into()
            } else {
                "text".into()
            }
        }
        _ => "text".into(),
    }
}

/// Derive a column type from the first `from` entry pointing at a typed event PROPERTY (mirrors deriveType).
pub(crate) fn derive_type(from: &[String], events: &Value) -> String {
    for r in from {
        let ptr = r.splitn(2, "#/").nth(1).unwrap_or("");
        let segs: Vec<&str> = ptr.split('/').filter(|s| !s.is_empty()).collect();
        if segs.len() < 3 || segs[1] != "properties" {
            continue;
        }
        if let Some(node) = events
            .get(segs[0])
            .and_then(|e| e.get("properties"))
            .and_then(|p| p.get(segs[2]))
        {
            return schema_node_to_column_type(node);
        }
    }
    String::new()
}

/// Map a column type (SQL primitive or scalars.yaml type) to a Postgres type (mirrors sqlType).
pub(crate) fn sql_type(ty: &str, model: &Model) -> String {
    let prim = match ty {
        "uuid" => Some("UUID"),
        "text" => Some("TEXT"),
        "integer" => Some("INTEGER"),
        "bigint" => Some("BIGINT"),
        "boolean" => Some("BOOLEAN"),
        "timestamptz" => Some("TIMESTAMPTZ"),
        "jsonb" => Some("JSONB"),
        "numeric" => Some("NUMERIC"),
        _ => None,
    };
    if let Some(p) = prim {
        return p.to_string();
    }
    if let Some(scalar) = model.defs.get("scalars.yaml").and_then(|s| s.get(ty)) {
        if scalar.get("enum").map(|e| e.is_sequence()).unwrap_or(false) {
            // Enums are stored as their TEXT value verbatim (ADR-20260728: supersedes the ADR-0037
            // INTEGER-ordinal + ref_<enum> lookup scheme) — self-describing rows, no join to read.
            return "TEXT".into();
        }
        if scalar.get("format").and_then(|x| x.as_str()) == Some("uuid") {
            return "UUID".into();
        }
        if scalar.get("type").and_then(|x| x.as_str()) == Some("integer") {
            return if ty == "MoneyCents" { "BIGINT".into() } else { "INTEGER".into() };
        }
    }
    "TEXT".into()
}

/// The four forms a `derive:` arm value may take (a literal, `{ from: prop }`, `null`, or
/// `{ from: prop, map: {...} }`), or `None` for anything else — the ONE place the grammar is
/// decided; the validator's `view-derive-value-unknown` rule and the emitter both read it. `map`
/// alongside `from` selects `Mapped` over `Payload` (#639 part C step 3-ii).
pub(crate) fn derive_val(dv: &Value) -> Option<DeriveVal> {
    match dv {
        Value::String(s) => Some(DeriveVal::Lit(s.clone())),
        Value::Null => Some(DeriveVal::Null),
        Value::Mapping(m) => {
            let from = m.get(Value::String("from".into())).and_then(|x| x.as_str())?.to_string();
            match m.get(Value::String("map".into())).and_then(|x| x.as_mapping()) {
                Some(mm) => {
                    let pairs: Vec<(String, String)> = mm
                        .iter()
                        .filter_map(|(k, v)| match (k.as_str(), v.as_str()) {
                            (Some(k), Some(v)) => Some((k.to_string(), v.to_string())),
                            _ => None,
                        })
                        .collect();
                    Some(DeriveVal::Mapped(from, pairs))
                }
                None => Some(DeriveVal::Payload(from)),
            }
        }
        _ => None,
    }
}

pub(crate) fn parse_col(name: String, col: &Value, events: &Value) -> SqlColumn {
    let from: Vec<String> = col
        .get("from")
        .and_then(|f| f.as_sequence())
        .map(|s| s.iter().filter_map(|it| it.get("$ref").and_then(|r| r.as_str()).map(|x| x.to_string())).collect())
        .unwrap_or_default();
    let has_explicit = matches!(col.get("type"), Some(v) if !v.is_null());
    let ty = if has_explicit {
        column_type_explicit(col.get("type").unwrap())
    } else {
        derive_type(&from, events)
    };
    let type_derived = !has_explicit && !ty.is_empty();
    let flag = |k: &str| col.get(k).and_then(|x| x.as_bool()) == Some(true);
    // `derive:` — an event_type → value map for status-from-event-type columns. A string value is a
    // literal enum value; `{ from: prop }` extracts the value from that event's payload; an explicit
    // `null` RESETS the column (DeriveVal::Null). Anything else is refused by the validator
    // (`view-derive-value-unknown`, an ERROR that aborts generation) — the skip below is the parser
    // staying total, never a semantics: no validated corpus reaches it.
    let mut derive = Vec::new();
    if let Some(dm) = col.get("derive").and_then(|d| d.as_mapping()) {
        for (dk, dv) in dm {
            if let Some(evt) = dk.as_str() {
                let val = match derive_val(dv) {
                    Some(v) => v,
                    None => continue,
                };
                derive.push((evt.to_string(), val));
            }
        }
    }
    // `occurredWhen:` — a list of { event, whenPayload?: { key: value } } clauses; the column is the
    // max(occurred_at) over events matching any clause (conditional occurrence time).
    let mut occurred_when = Vec::new();
    if let Some(seq) = col.get("occurredWhen").and_then(|d| d.as_sequence()) {
        for clause in seq {
            if let Some(evt) = clause.get("event").and_then(|x| x.as_str()) {
                let mut conds = Vec::new();
                if let Some(wp) = clause.get("whenPayload").and_then(|x| x.as_mapping()) {
                    for (pk, pv) in wp {
                        if let (Some(k), Some(v)) = (pk.as_str(), pv.as_str()) {
                            conds.push((k.to_string(), v.to_string()));
                        }
                    }
                }
                occurred_when.push((evt.to_string(), conds));
            }
        }
    }
    SqlColumn {
        name,
        ty,
        pk: flag("pk"),
        unique: flag("unique"),
        index: flag("index"),
        nullable: flag("nullable"),
        fk: parse_fk(col.get("fk")),
        note: col.get("note").and_then(|x| x.as_str()).map(|s| s.to_string()),
        from,
        type_derived,
        derive,
        occurred_when,
    }
}

pub(crate) fn parse_views(model: &Model) -> Vec<SqlView> {
    let mut out = Vec::new();
    let events = model.defs.get("events.yaml").cloned().unwrap_or(Value::Null);
    // Read models live in two files: projection_views.yaml (generated fold VIEWs) and
    // tables/projection_tables.yaml (materialized TABLEs fed by a projector). Same metadata shape.
    for (file, is_table) in [
        ("database/projection_views.yaml", false),
        ("database/tables/projection_tables.yaml", true),
    ] {
        let m = match model.defs.get(file) {
            Some(Value::Mapping(m)) => m,
            _ => continue,
        };
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            let is_ref = node.get("source").and_then(|x| x.as_str()) == Some("reference");
            let has_agg = node.get("aggregate").and_then(|x| x.as_str()).is_some();
            if !has_agg && !is_ref {
                continue; // skip file-level meta (version/description) and non-views
            }
            let mut columns = Vec::new();
            if let Some(cm) = node.get("columns").and_then(|c| c.as_mapping()) {
                for (ck, cv) in cm {
                    if let Some(cn) = ck.as_str() {
                        columns.push(parse_col(cn.to_string(), cv, &events));
                    }
                }
            } else if let Some(cs) = node.get("columns").and_then(|c| c.as_sequence()) {
                for cv in cs {
                    let cn = cv.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    columns.push(parse_col(cn, cv, &events));
                }
            }
            let mut indexes = Vec::new();
            if let Some(seq) = node.get("indexes").and_then(|x| x.as_sequence()) {
                for ix in seq {
                    if let Some(cols) = ix.as_sequence() {
                        indexes.push(
                            cols.iter().filter_map(|c| c.as_str().map(|s| s.to_string())).collect(),
                        );
                    }
                }
            }
            // Technical audit timestamps are IMPLICIT on every read model — not declared per table
            // (ADR-0040). created_at = the creation event's occurred_at; updated_at = the latest applied
            // event's occurred_at. Handled by name in generate_fold_sql (views) / the dispatch (tables).
            if node.get("aggregate").and_then(|x| x.as_str()).is_some() {
                for tech in ["created_at", "updated_at"] {
                    columns.push(SqlColumn {
                        name: tech.to_string(),
                        ty: "timestamptz".to_string(),
                        pk: false,
                        unique: false,
                        index: false,
                        nullable: false,
                        fk: None,
                        note: Some("technical — stamped from event.occurred_at (implicit on every read model)".to_string()),
                        from: Vec::new(),
                        type_derived: false,
                        derive: Vec::new(),
                        occurred_when: Vec::new(),
                    });
                }
            }
            let aggregate = node.get("aggregate").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let tombstone = node
                .get("tombstone")
                .and_then(|t| t.get("$ref").and_then(|r| r.as_str()))
                .and_then(ref_name);
            out.push(SqlView {
                name: name.to_string(),
                aggregate,
                slice: node.get("slice").and_then(|x| x.as_str()).unwrap_or("V0").to_string(),
                internal: node.get("internal").and_then(|x| x.as_bool()) == Some(true),
                reference: is_ref,
                filters: string_list(node.get("filters")),
                rules: string_list(node.get("rules")),
                note: node.get("note").and_then(|x| x.as_str()).map(|s| s.to_string()),
                fedby: ref_names(node.get("fedBy")),
                columns,
                indexes,
                is_table,
                projector: node.get("projector").and_then(|x| x.as_str()).map(|s| s.to_string()),
                tombstone,
                definition: node.get("definition").and_then(|x| x.as_str()).map(|s| s.trim_end().to_string()),
            });
        }
    }
    out
}

/// Split an event/property `$ref` into (event_type, Option<property>). A whole-event ref has no property.
pub(crate) fn event_and_prop(r: &str) -> (String, Option<String>) {
    let ptr = r.splitn(2, "#/").nth(1).unwrap_or("");
    let segs: Vec<&str> = ptr.split('/').filter(|s| !s.is_empty()).collect();
    let evt = segs.first().copied().unwrap_or("").to_string();
    let prop = if segs.len() >= 3 && segs[1] == "properties" { Some(segs[2].to_string()) } else { None };
    (evt, prop)
}

/// Postgres cast suffix for a resolved SQL type (JSONB reads via `->` and needs no cast → "").
pub(crate) fn pg_cast(pgty: &str) -> &'static str {
    match pgty {
        "UUID" => "::uuid",
        "INTEGER" => "::int",
        "BIGINT" => "::bigint",
        "NUMERIC" => "::numeric",
        "BOOLEAN" => "::boolean",
        "TIMESTAMPTZ" => "::timestamptz",
        _ => "",
    }
}

/// A payload extraction expression for `<alias>.payload`'s `prop`, typed to `pgty`.
pub(crate) fn payload_extract(alias: &str, prop: &str, pgty: &str) -> String {
    if pgty == "JSONB" {
        format!("{}.payload->'{}'", alias, prop)
    } else {
        let c = pg_cast(pgty);
        if c.is_empty() {
            format!("{}.payload->>'{}'", alias, prop)
        } else {
            format!("({}.payload->>'{}'){}", alias, prop, c)
        }
    }
}

/// The Money-value-object subfield a `*_cents`/currency column extracts (the projection convention:
/// `Money = { amountCents, currency }` becomes a `MoneyCents` column + a `CurrencyCode` column).
/// `Some` only when the column's `from` property is `$ref: entities.yaml#/Money` AND the declared
/// column type picks a subfield — `MoneyCents` → `amountCents`, `CurrencyCode` → `currency`.
pub(crate) fn money_subfield(model: &Model, evt: &str, prop: &str, col_ty: &str) -> Option<&'static str> {
    let sub = match col_ty {
        "MoneyCents" => "amountCents",
        "CurrencyCode" => "currency",
        _ => return None,
    };
    let r = model
        .defs
        .get("events.yaml")?
        .get(evt)?
        .get("properties")?
        .get(prop)?
        .get("$ref")?
        .as_str()?;
    if r == "entities.yaml#/Money" {
        Some(sub)
    } else {
        None
    }
}

/// The scalars.yaml enum a given event PROPERTY `$ref`s — `None` when the property is absent or
/// does not `$ref` a scalars.yaml name. Shared by the lifecycle `via`/`when` grammar
/// (ADR-20260721-093027, extended #639 part C step 3-ii, ADR-20260904-015903 §2) and the
/// `derive: { from, map }` grammar (§3) — both key a target off a DIFFERENT event field.
pub(crate) fn payload_field_scalar(model: &Model, evt: &str, field: &str) -> Option<String> {
    let node = model.defs.get("events.yaml")?.get(evt)?.get("properties")?.get(field)?;
    let r = node.get("$ref")?.as_str()?;
    let mut it = r.splitn(2, "#/");
    let file = it.next()?;
    let name = it.next()?;
    if file == "scalars.yaml" {
        Some(name.to_string())
    } else {
        None
    }
}

/// The values of a scalars.yaml enum, in declared order — `Some` only for an enum scalar.
pub(crate) fn enum_values(model: &Model, ty: &str) -> Option<Vec<String>> {
    model
        .defs
        .get("scalars.yaml")?
        .get(ty)?
        .get("enum")?
        .as_sequence()
        .map(|s| s.iter().filter_map(|v| v.as_str().map(|x| x.to_string())).collect())
}

/// Generate a `SELECT … FROM domain_events` state-fold body for a foldable view (ADR-0035 #2), sourcing
/// each column from its declared `from` lineage + derivation mode. Correct-by-construction: set-once
/// fields fall out of the per-column "latest carrying event" rule, so there is no latest-wins hazard.
pub(crate) fn generate_fold_sql(v: &SqlView, model: &Model) -> Result<String, String> {
    // The creation event = the event carrying the PK column; it defines row existence (one row per stream).
    let pk = v.columns.iter().find(|c| c.pk).ok_or_else(|| "no PK column".to_string())?;
    let creation = pk
        .from
        .iter()
        .filter_map(|r| { let (e, p) = event_and_prop(r); p.map(|_| e) })
        .next()
        .ok_or_else(|| format!("PK column '{}' has no property `from` to anchor the creation event", pk.name))?;

    let mut selects: Vec<String> = Vec::new();
    for c in &v.columns {
        let pgty = sql_type(&c.ty, model);
        // Enum columns store the enum's TEXT value verbatim — exactly what the payload holds, so no
        // mapping is needed; the values are still validated against scalars.yaml for literals.
        let enum_vals = enum_values(model, &c.ty);
        let expr = if c.name == "created_at" {
            // implicit technical column: the creation event's occurrence time.
            "c.occurred_at".to_string()
        } else if c.name == "updated_at" {
            // implicit technical column: the latest applied event's occurrence time.
            let types: Vec<String> = v.fedby.iter().map(|e| format!("'{}'", e)).collect();
            format!(
                "(SELECT max(e.occurred_at) FROM domain_events e\n     WHERE e.stream_name = c.stream_name AND e.event_type IN ({}))",
                types.join(", ")
            )
        } else if !c.occurred_when.is_empty() {
            // conditional occurrence: max(occurred_at) over events matching any (type [+ payload =]) clause.
            let clauses: Vec<String> = c
                .occurred_when
                .iter()
                .map(|(evt, conds)| {
                    let mut parts = vec![format!("e.event_type = '{}'", evt)];
                    for (k, val) in conds {
                        parts.push(format!("e.payload->>'{}' = '{}'", k, val));
                    }
                    if parts.len() == 1 { parts.remove(0) } else { format!("({})", parts.join(" AND ")) }
                })
                .collect();
            format!(
                "(SELECT max(e.occurred_at) FROM domain_events e\n     WHERE e.stream_name = c.stream_name AND ({}))",
                clauses.join(" OR ")
            )
        } else if !c.derive.is_empty() {
            // status-from-event-type: CASE over the latest matching lifecycle event.
            let arms: Vec<String> = c
                .derive
                .iter()
                .map(|(evt, val)| {
                    let then = match val {
                        DeriveVal::Lit(s) => {
                            if let Some(vals) = &enum_vals {
                                assert!(
                                    vals.iter().any(|v| v == s),
                                    "derive value '{}' not in enum {}",
                                    s,
                                    c.ty
                                );
                            }
                            format!("'{}'", s)
                        }
                        DeriveVal::Payload(p) => format!("e.payload->>'{}'", p),
                        DeriveVal::Null => "NULL".to_string(),
                        DeriveVal::Mapped(field, pairs) => {
                            let inner: Vec<String> =
                                pairs.iter().map(|(k, v)| format!("WHEN '{}' THEN '{}'", k, v)).collect();
                            format!("(CASE e.payload->>'{}' {} END)", field, inner.join(" "))
                        }
                    };
                    format!("WHEN '{}' THEN {}", evt, then)
                })
                .collect();
            let types: Vec<String> = c.derive.iter().map(|(e, _)| format!("'{}'", e)).collect();
            format!(
                "(SELECT CASE e.event_type {} END FROM domain_events e\n     WHERE e.stream_name = c.stream_name AND e.event_type IN ({})\n     ORDER BY e.position DESC LIMIT 1)",
                arms.join(" "),
                types.join(", ")
            )
        } else {
            let carrying: Vec<(String, String)> = c
                .from
                .iter()
                .filter_map(|r| { let (e, p) = event_and_prop(r); p.map(|p| (e, p)) })
                .collect();
            let whole: Vec<String> =
                c.from.iter().filter_map(|r| { let (e, p) = event_and_prop(r); if p.is_none() { Some(e) } else { None } }).collect();
            if c.ty == "timestamptz" && carrying.is_empty() && !whole.is_empty() {
                // occurrence time: max(occurred_at) over the contributing event types.
                if whole.len() == 1 && whole[0] == creation {
                    "c.occurred_at".to_string()
                } else {
                    let types: Vec<String> = whole.iter().map(|e| format!("'{}'", e)).collect();
                    format!(
                        "(SELECT max(e.occurred_at) FROM domain_events e\n     WHERE e.stream_name = c.stream_name AND e.event_type IN ({}))",
                        types.join(", ")
                    )
                }
            } else if let Some((first_evt, prop)) = carrying.first() {
                // scalar "latest carrying event": the newest event whose payload holds this property.
                // An enum column stores the payload's TEXT value verbatim; a Money property splits into
                // its `amountCents`/`currency` subfield by declared column type; others extract+cast.
                let money_sub = money_subfield(model, first_evt, prop, &c.ty);
                let val_expr = |alias: &str| {
                    if let Some(sub) = money_sub {
                        let cast = pg_cast(&pgty);
                        if cast.is_empty() {
                            format!("{}.payload->'{}'->>'{}'", alias, prop, sub)
                        } else {
                            format!("({}.payload->'{}'->>'{}'){}", alias, prop, sub, cast)
                        }
                    } else {
                        payload_extract(alias, prop, &pgty)
                    }
                };
                let only_creation = carrying.iter().all(|(e, _)| e == &creation);
                if only_creation {
                    val_expr("c")
                } else {
                    // Scope by the declared carrying event types AND the property key — so a JSON key shared
                    // by an unrelated event type can never win over the intended source.
                    let mut types: Vec<String> = Vec::new();
                    for (e, _) in &carrying {
                        let q = format!("'{}'", e);
                        if !types.contains(&q) {
                            types.push(q);
                        }
                    }
                    format!(
                        "(SELECT {} FROM domain_events e\n     WHERE e.stream_name = c.stream_name AND e.event_type IN ({}) AND e.payload ? '{}'\n     ORDER BY e.position DESC LIMIT 1)",
                        val_expr("e"),
                        types.join(", "),
                        prop
                    )
                }
            } else {
                return Err(format!(
                    "column '{}' is not foldable (no property `from`, not a timestamp occurrence, no `derive`) — move the view to projection_tables.yaml (materialized) or add a mode",
                    c.name
                ));
            }
        };
        selects.push(format!("  {} AS {}", expr, c.name));
    }

    let mut sql = format!("SELECT\n{}\nFROM domain_events c\nWHERE c.event_type = '{}'", selects.join(",\n"), creation);
    if let Some(tomb) = &v.tombstone {
        sql.push_str(&format!(
            "\n  AND NOT EXISTS (SELECT 1 FROM domain_events d\n                  WHERE d.stream_name = c.stream_name AND d.event_type = '{}')",
            tomb
        ));
    }
    Ok(sql)
}

pub(crate) fn emit_views_sql(model: &Model) -> String {
    let mut blocks = Vec::new();
    for v in parse_views(model) {
        // Only fold VIEWs (projection_views.yaml) → CREATE OR REPLACE VIEW over domain_events, from a
        // hand-written `definition` override if present, else generated from the column `from` lineage.
        // Materialized read-model TABLEs (projection_tables.yaml) are emitted into schema.generated.sql.
        if v.is_table {
            continue;
        }
        let body = match &v.definition {
            Some(def) => def.clone(),
            None => generate_fold_sql(&v, model)
                .unwrap_or_else(|e| panic!("projection_views.yaml#/{}: cannot generate fold: {}", v.name, e)),
        };
        blocks.push(format!("CREATE OR REPLACE VIEW {} AS\n{};", v.name, body));
    }
    format!(
        "-- GENERATED by tools/codegen from specs/database/projection_views.yaml — do not edit by hand.\n-- Read models realized as SQL VIEWS: a `CREATE OR REPLACE VIEW` state-fold over domain_events, generated\n-- from each column's `from` lineage (ADR-0039). Read models whose columns are COMPUTED are materialized\n-- tables in tables/projection_tables.yaml (emitted into schema.generated.sql) instead.\n\n{}\n",
        blocks.join("\n\n")
    )
}

/// CREATE TABLE DDL (+ indexes) for a materialized read-model table, column types resolved from the
/// per-column `from` lineage (unlike referential tables, whose columns carry an explicit `type`).
pub(crate) fn view_table_ddl(v: &SqlView, model: &Model) -> String {
    let mut cols = Vec::new();
    for c in &v.columns {
        let mut bits = vec![format!("  {}", c.name), sql_type(&c.ty, model)];
        if c.pk {
            bits.push("PRIMARY KEY".into());
        } else if c.unique {
            bits.push(if c.nullable { "UNIQUE".into() } else { "NOT NULL UNIQUE".into() });
        } else if !c.nullable {
            bits.push("NOT NULL".into());
        }
        cols.push(bits.join(" "));
    }
    let ddl = format!("CREATE TABLE {} (\n{}\n);", v.name, cols.join(",\n"));
    let mut idx: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for c in &v.columns {
        if c.index && !c.pk && seen.insert(c.name.clone()) {
            idx.push(format!("CREATE INDEX ON {} ({});", v.name, c.name));
        }
    }
    for ix in &v.indexes {
        if seen.insert(ix.join(",")) {
            idx.push(format!("CREATE INDEX ON {} ({});", v.name, ix.join(", ")));
        }
    }
    if idx.is_empty() { ddl } else { format!("{}\n{}", ddl, idx.join("\n")) }
}

/// The materialized read-model TABLEs (projection_tables.yaml) as DDL, for inclusion in schema.generated.sql.
pub(crate) fn emit_projection_tables_sql(model: &Model) -> String {
    parse_views(model)
        .iter()
        .filter(|v| v.is_table)
        .map(|v| view_table_ddl(v, model))
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ─── schema.generated.sql (ADR-0037 — store DDL from database/tables.yaml + functions/*.sql + scalars enums) ──

/// snake_case of a PascalCase type name, without a leading underscore (`OrderStatus` → `order_status`).
pub(crate) fn snake_type(s: &str) -> String {
    snake_field(s).trim_start_matches('_').to_string()
}

/// Map a tables.yaml SQL-primitive column type to its Postgres spelling. Infrastructure tables are
/// deliberately decoupled from the domain scalars, so this is a closed map — an unknown type is a spec
/// error, failed loudly rather than defaulted.
pub(crate) fn table_sql_type(ty: &str) -> &'static str {
    match ty {
        "uuid" => "UUID",
        "text" => "TEXT",
        "integer" => "INTEGER",
        "bigint" => "BIGINT",
        "smallint" => "SMALLINT",   // mailbox partition ordinal (inbound_messages, #242)
        "boolean" => "BOOLEAN",
        "timestamptz" => "TIMESTAMPTZ",
        "jsonb" => "JSONB",
        "numeric" => "NUMERIC",
        "interval" => "INTERVAL",
        "bytea" => "BYTEA",   // encrypted-at-rest blobs (auth_sessions ciphertext, #112)
        other => panic!("database/tables.yaml: unknown column type '{}' — extend table_sql_type", other),
    }
}

/// Emit `schema.generated.sql` (ADR-0037, enum storage revised by ADR-20260728): the full store DDL —
/// the real tables from database/tables.yaml, the raw SQL functions from database/functions/*.sql
/// (sorted by filename), then the triggers declared on the tables (after the functions they execute).
/// Enum columns are TEXT holding the scalars.yaml value verbatim — no ref_<enum> lookup tables.
pub(crate) fn emit_schema_sql(model: &Model, specs: &std::path::Path) -> String {
    let mut sections: Vec<String> = Vec::new();

    // 1. Tables from database/tables/*.yaml, in file order. Triggers are collected and emitted after
    // the functions they execute (step 3). projection_tables.yaml is handled separately (step 1b) — its
    // columns derive their type from event lineage, not an explicit `type`.
    let mut triggers: Vec<String> = Vec::new();
    for (_fkey, fval) in model
        .defs
        .iter()
        .filter(|(k, _)| k.starts_with("database/tables/") && k.as_str() != "database/tables/projection_tables.yaml")
    {
        let m = match fval {
            Value::Mapping(m) => m,
            _ => continue,
        };
        for (k, node) in m {
            let name = match k.as_str() {
                Some(s) => s,
                None => continue,
            };
            let cols = match node.get("columns").and_then(|c| c.as_mapping()) {
                Some(c) => c,
                None => continue,
            };
            let mut lines: Vec<String> = Vec::new();
            // More than one `pk: true` column is a COMPOSITE key (`auth_subject_reservations`'s
            // `(principal_kind, auth_subject)`, #794): each such column is `NOT NULL` and the key is
            // emitted as one table-level `PRIMARY KEY (a, b)` after the columns. Inline
            // `PRIMARY KEY` on two columns is not SQL, so a single-column key keeps today's inline
            // form and only the composite case takes the table-level one.
            let pk_cols: Vec<&str> = cols
                .iter()
                .filter(|(_, cv)| cv.get("pk").and_then(|x| x.as_bool()) == Some(true))
                .filter_map(|(ck, _)| ck.as_str())
                .collect();
            let composite_pk = pk_cols.len() > 1;
            for (ck, cv) in cols {
                let cname = match ck.as_str() {
                    Some(s) => s,
                    None => continue,
                };
                // `type` is either a SQL-primitive string (→ table_sql_type) or a `$ref` into a
                // scalars.yaml scalar (→ its Postgres type via the shared sql_type mapping).
                let ty_node = cv.get("type").unwrap_or_else(|| {
                    panic!("database/tables.yaml#/{}/columns/{}: missing type", name, cname)
                });
                let sqlty: String = if let Some(s) = ty_node.as_str() {
                    table_sql_type(s).to_string()
                } else if let Some(rf) = ty_node.get("$ref").and_then(|x| x.as_str()) {
                    let scalar = ref_name(rf).unwrap_or_else(|| {
                        panic!("database/tables.yaml#/{}/columns/{}: malformed $ref '{}'", name, cname, rf)
                    });
                    sql_type(&scalar, model) // an enum scalar → TEXT value, verbatim
                } else {
                    panic!("database/tables.yaml#/{}/columns/{}: type must be a SQL primitive or a $ref", name, cname)
                };
                let flag = |f: &str| cv.get(f).and_then(|x| x.as_bool()) == Some(true);
                let mut line = format!("  {} {}", cname, sqlty);
                if flag("identity") {
                    line.push_str(" GENERATED ALWAYS AS IDENTITY");
                }
                if flag("pk") && !composite_pk {
                    line.push_str(" PRIMARY KEY");
                } else if flag("pk") {
                    line.push_str(" NOT NULL");
                } else {
                    line.push_str(if flag("nullable") { " NULL" } else { " NOT NULL" });
                    if flag("unique") {
                        line.push_str(" UNIQUE");
                    }
                }
                lines.push(line);
            }
            if composite_pk {
                lines.push(format!("  PRIMARY KEY ({})", pk_cols.join(", ")));
            }
            if let Some(cs) = node.get("constraints").and_then(|c| c.as_sequence()) {
                for c in cs {
                    if let Some(u) = c.get("unique").and_then(|x| x.as_sequence()) {
                        let cols: Vec<&str> = u.iter().filter_map(|v| v.as_str()).collect();
                        lines.push(format!("  UNIQUE ({})", cols.join(", ")));
                    }
                }
            }
            let mut block = format!("CREATE TABLE {} (\n{}\n);", name, lines.join(",\n"));
            // per-column `index: true` (non-pk) → a single-column index (e.g. referential dialing_code).
            for (ck, cv) in cols {
                if let Some(cn) = ck.as_str() {
                    let f = |x: &str| cv.get(x).and_then(|b| b.as_bool()) == Some(true);
                    if f("index") && !f("pk") {
                        block.push_str(&format!("\nCREATE INDEX ON {} ({});", name, cn));
                    }
                }
            }
            if let Some(seq) = node.get("indexes").and_then(|x| x.as_sequence()) {
                for ix in seq {
                    if let Some(cols) = ix.as_sequence() {
                        let cols: Vec<&str> = cols.iter().filter_map(|v| v.as_str()).collect();
                        block.push_str(&format!("\nCREATE INDEX ON {} ({});", name, cols.join(", ")));
                    }
                }
            }
            sections.push(block);
            if let Some(ts) = node.get("triggers").and_then(|t| t.as_sequence()) {
                for t in ts {
                    let get = |f: &str| {
                        t.get(f).and_then(|x| x.as_str()).unwrap_or_else(|| {
                            panic!("database/tables.yaml#/{}/triggers: missing {}", name, f)
                        })
                    };
                    triggers.push(format!(
                        "CREATE TRIGGER {} {} ON {} FOR EACH {} EXECUTE FUNCTION {}();",
                        get("name"),
                        get("timing"),
                        name,
                        get("for_each").to_uppercase(),
                        get("function")
                    ));
                }
            }
        }
    }

    // 1b. Materialized read-model tables (database/tables/projection_tables.yaml) — column types resolved
    // from event lineage. Filled by an application-layer (Rust) projector, not SQL (ADR-0040). Emitted
    // here so the read-model tables sit alongside the store tables.
    let ptables = emit_projection_tables_sql(model);
    if !ptables.trim().is_empty() {
        sections.push(ptables);
    }

    // 2. Functions — raw SQL bodies from database/functions/*.sql, sorted by filename. They reference
    // domain_events/domain_stream, which now exist above.
    let fn_dir = specs.join("database/functions");
    let mut fn_files: Vec<PathBuf> = fs::read_dir(&fn_dir)
        .unwrap_or_else(|e| panic!("read {}: {}", fn_dir.display(), e))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("sql"))
        .collect();
    fn_files.sort_by_key(|p| p.file_name().map(|n| n.to_os_string()));
    for p in &fn_files {
        let body = fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {}", p.display(), e));
        sections.push(body.replace("\r\n", "\n").trim().to_string());
    }

    // 3. Triggers — after the functions they execute.
    sections.extend(triggers);

    format!(
        "-- GENERATED by the Captain.Food codegen from specs/database/ + scalars.yaml — do not edit by hand.\n\n{}\n",
        sections.join("\n\n")
    )
}

// ─── database.md §2 read-model tables (port of emit/database.ts `emitViewsMarkdown`) ────────────

pub(crate) fn md_table(header: &[&str], rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new(); // matches documentation.ts mdTable (empty → no table); database.ts always passes rows
    }
    let mut out = vec![
        format!("| {} |", header.join(" | ")),
        format!("| {} |", header.iter().map(|_| "---").collect::<Vec<_>>().join(" | ")),
    ];
    for r in rows {
        out.push(format!("| {} |", r.join(" | ")));
    }
    out.join("\n")
}

pub(crate) fn constraints(c: &SqlColumn) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if c.pk {
        parts.push("PK");
    }
    if c.unique {
        parts.push("unique");
    }
    if c.index {
        parts.push("index");
    }
    if c.nullable {
        parts.push("nullable");
    }
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join(", ")
    }
}

pub(crate) fn view_block(v: &SqlView, model: &Model) -> String {
    let slice = if v.slice == "V1" { "🔭 V1" } else { "🛶 V0" };
    let internal = if v.internal { " · 🔒 internal" } else { "" };
    let origin = if v.reference {
        "📦 reference (static seed)".to_string()
    } else {
        format!("source aggregate `{}`", v.aggregate)
    };
    let mut lines = vec![format!("### `{}` · {}{} · {}", v.name, slice, internal, origin), String::new()];
    if v.internal {
        lines.push("- **Consumed by**: command handlers / auth resolution (no GraphQL query).".into());
    }
    if v.reference {
        lines.push("- **Reference data**: seeded at deploy time (not event-fed).".into());
    } else {
        lines.push(format!("- **Fed by**: {}", v.fedby.iter().map(|n| format!("`{}`", n)).collect::<Vec<_>>().join(", ")));
    }
    if !v.filters.is_empty() {
        lines.push(format!("- **Filters**: {}", v.filters.join(" ")));
    }
    if !v.rules.is_empty() {
        lines.push(format!("- **Rules**: {}", v.rules.join(" ")));
    }
    if let Some(note) = &v.note {
        lines.push(format!("- **Note**: {}", note));
    }
    if !v.indexes.is_empty() {
        lines.push(format!("- **Indexes**: {}", v.indexes.iter().map(|ix| format!("`({})`", ix.join(", "))).collect::<Vec<_>>().join(", ")));
    }
    lines.push(String::new());
    let rows: Vec<Vec<String>> = v
        .columns
        .iter()
        .map(|c| {
            vec![
                format!("`{}`", c.name),
                format!("`{}`", c.ty),
                format!("`{}`", sql_type(&c.ty, model)),
                constraints(c),
                c.note.clone().unwrap_or_default(),
            ]
        })
        .collect();
    lines.push(md_table(&["Column", "Type", "SQL", "Constraints", "Notes"], &rows));
    lines.join("\n")
}

pub(crate) fn emit_views_markdown(model: &Model) -> String {
    parse_views(model).iter().map(|v| view_block(v, model)).collect::<Vec<_>>().join("\n\n")
}

/// Replace the body between `<!-- GENERATED:<id> START … -->` and `<!-- GENERATED:<id> END -->`
/// (port of cli.ts `injectGenerated`). Returns false if the markers are absent.
pub(crate) fn inject_generated(path: &PathBuf, id: &str, body: &str) -> Result<bool, String> {
    let src = fs::read_to_string(path).map_err(|e| format!("read {}: {}", path.display(), e))?;
    let start_pat = format!("<!-- GENERATED:{} START", id);
    let end_pat = format!("<!-- GENERATED:{} END -->", id);
    let start_idx = match src.find(&start_pat) {
        Some(i) => i,
        None => return Ok(false),
    };
    let rel = match src[start_idx..].find("-->") {
        Some(i) => i,
        None => return Ok(false),
    };
    let start_marker_end = start_idx + rel + 3;
    let end_idx = match src.find(&end_pat) {
        Some(i) => i,
        None => return Ok(false),
    };
    let new = format!("{}\n\n{}\n\n{}", &src[..start_marker_end], body, &src[end_idx..]);
    fs::write(path, new).map_err(|e| format!("write {}: {}", path.display(), e))?;
    Ok(true)
}

