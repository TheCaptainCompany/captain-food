use crate::*;

// ─── crates/{application,infrastructure}/src/generated/pm_state.rs (issue #27 — PM state tables) ─────

/// How a PM state-table column binds to Rust/sqlx — classified from its declared `type` (a scalars.yaml
/// `$ref` or a SQL primitive). PM tables carry no lineage (`from`), so the type is always explicit.
pub(crate) enum PmTy {
    /// scalars.yaml newtype over `uuid::Uuid` (Copy).
    UuidScalar(String),
    /// scalars.yaml newtype over `String`.
    StringScalar(String),
    /// scalars.yaml newtype over `i64` (Copy — e.g. `MoneyCents`, stored BIGINT).
    IntScalar(String),
    /// scalars.yaml enum — stored as its TEXT value verbatim (ADR-20260728).
    EnumScalar(String),
    /// SQL `text`.
    Text,
    /// SQL `integer` (i32 — matches the migration's INTEGER, unlike projection rows' bigint-ish i64).
    Integer,
    /// SQL `timestamptz`.
    Timestamptz,
}

pub(crate) fn pm_ty(model: &Model, table: &str, col: &str, ty: &str) -> PmTy {
    match ty {
        "text" => return PmTy::Text,
        "integer" => return PmTy::Integer,
        "timestamptz" => return PmTy::Timestamptz,
        _ => {}
    }
    let node = model
        .defs
        .get("scalars.yaml")
        .and_then(|s| s.get(ty))
        .unwrap_or_else(|| panic!("process_managers.yaml#/{}/columns/{}: unsupported column type '{}' — expected a scalars.yaml $ref or text/integer/timestamptz", table, col, ty));
    if node.get("enum").map(|e| e.is_sequence()).unwrap_or(false) {
        PmTy::EnumScalar(ty.to_string())
    } else if node.get("format").and_then(|f| f.as_str()) == Some("uuid") {
        PmTy::UuidScalar(ty.to_string())
    } else if node.get("type").and_then(|t| t.as_str()) == Some("integer") {
        PmTy::IntScalar(ty.to_string())
    } else {
        PmTy::StringScalar(ty.to_string())
    }
}

impl PmTy {
    /// The row-struct field type (before the `Option<…>` nullable wrap).
    pub(crate) fn field(&self) -> String {
        match self {
            PmTy::UuidScalar(n) | PmTy::StringScalar(n) | PmTy::IntScalar(n) | PmTy::EnumScalar(n) => n.clone(),
            PmTy::Text => "String".into(),
            PmTy::Integer => "i32".into(),
            PmTy::Timestamptz => "chrono::DateTime<chrono::Utc>".into(),
        }
    }
    /// Whether a lookup passes this type by reference (`&Ty`) — String-backed newtypes only; Copy
    /// scalars go by value (mirrors the hand-written signatures this emitter replaced).
    pub(crate) fn by_ref(&self) -> bool {
        matches!(self, PmTy::StringScalar(_))
    }
}

/// One `by_*` lookup a PM state store exposes: the pk lookup, one per UNIQUE correlation column, plus
/// the emitter-registered extras (reads a saga explicitly declares, e.g. `paymentStatus`).
pub(crate) struct PmLookup {
    pub(crate) method: String,
    pub(crate) column: String,
    pub(crate) doc: String,
}

/// One PM state table of `database/tables/process_managers.yaml`, with its derived Rust names.
pub(crate) struct PmTable {
    pub(crate) table: String,
    /// CamelCase base of the generated names (`PaymentProcess` → `PaymentProcessRow`,
    /// `PaymentProcessStateStore`, `MemPaymentProcessState`, `PgPaymentProcessState`).
    pub(crate) base: String,
    pub(crate) note: Option<String>,
    pub(crate) columns: Vec<SqlColumn>,
    pub(crate) pk: String,
    pub(crate) lookups: Vec<PmLookup>,
}

/// CamelCase base name of a PM state table: the table name minus the `_process_manager` suffix; a
/// single-word stem keeps the `Process` word for readability (`payment` → `PaymentProcess`, but
/// `cart_binding` → `CartBinding`).
pub(crate) fn pm_base_name(table: &str) -> String {
    let stem = table.strip_suffix("_process_manager").unwrap_or(table);
    let camel: String = stem
        .split('_')
        .map(|w| {
            let mut cs = w.chars();
            match cs.next() {
                Some(c) => c.to_ascii_uppercase().to_string() + cs.as_str(),
                None => String::new(),
            }
        })
        .collect();
    if stem.contains('_') { camel } else { format!("{}Process", camel) }
}

/// Lookup method name for a column: `by_<column minus its trailing _id>` — mechanical, so the
/// `state.by` keys of processmanager.yaml map 1:1 onto store methods (roadmap item 3).
pub(crate) fn pm_lookup_method(column: &str) -> String {
    format!("by_{}", column.strip_suffix("_id").unwrap_or(column))
}

/// Parse the PM state tables (file order). Lookups = the pk, every `unique: true` correlation column,
/// plus EXTRA_LOOKUPS — the narrowly-scoped initiator reads a saga explicitly declares. Those reads'
/// resolver bodies are hardcoded in the server query/subscription emitters (`paymentStatus` /
/// `paymentStatusChanged`, ADR-20260720-015500), so the lookup that serves them is registered here,
/// next to that code, rather than inferred from the table DSL.
pub(crate) fn parse_pm_tables(model: &Model) -> Vec<PmTable> {
    const FILE: &str = "database/tables/process_managers.yaml";
    const EXTRA_LOOKUPS: &[(&str, &str, &str)] = &[(
        "payment_process_manager",
        "order_id",
        "The run that will materialize this order — the `paymentStatus(orderId)` read (ADR-20260720-015500; the caller enforces the initiator ownership scope).",
    )];
    let events = model.defs.get("events.yaml").cloned().unwrap_or(Value::Null);
    let mut out = Vec::new();
    let Some(Value::Mapping(m)) = model.defs.get(FILE) else {
        return out;
    };
    for (k, node) in m {
        let (Some(table), Some(cols)) = (k.as_str(), node.get("columns").and_then(|c| c.as_mapping())) else {
            continue;
        };
        let columns: Vec<SqlColumn> = cols
            .iter()
            .filter_map(|(ck, cv)| ck.as_str().map(|n| parse_col(n.to_string(), cv, &events)))
            .collect();
        let pks: Vec<&SqlColumn> = columns.iter().filter(|c| c.pk).collect();
        assert!(pks.len() == 1, "{}#/{}: expected exactly one pk column (the run's correlation identity), found {}", FILE, table, pks.len());
        let pk = pks[0].name.clone();
        assert!(
            columns.iter().any(|c| c.name == "last_update_utc" && c.ty == "timestamptz" && !c.nullable),
            "{}#/{}: missing the non-nullable `last_update_utc` timestamptz envelope column", FILE, table
        );
        let mut lookups = vec![PmLookup {
            method: pm_lookup_method(&pk),
            column: pk.clone(),
            doc: format!("The live run for this {}, if any (pk lookup).", pk.strip_suffix("_id").unwrap_or(&pk).replace('_', " ")),
        }];
        for c in columns.iter().filter(|c| c.unique && !c.pk) {
            assert!(!c.nullable, "{}#/{}: UNIQUE lookup column `{}` must be non-nullable", FILE, table, c.name);
            lookups.push(PmLookup {
                method: pm_lookup_method(&c.name),
                column: c.name.clone(),
                doc: format!("Correlate an inbound fact back to its run (UNIQUE `{}`).", c.name),
            });
        }
        for (_, col, doc) in EXTRA_LOOKUPS.iter().filter(|(t, _, _)| *t == table) {
            let c = columns
                .iter()
                .find(|c| c.name == *col)
                .unwrap_or_else(|| panic!("{}#/{}: EXTRA_LOOKUPS names unknown column `{}`", FILE, table, col));
            assert!(!c.nullable, "{}#/{}: extra lookup column `{}` must be non-nullable", FILE, table, col);
            lookups.push(PmLookup { method: pm_lookup_method(col), column: (*col).to_string(), doc: (*doc).to_string() });
        }
        out.push(PmTable {
            table: table.to_string(),
            base: pm_base_name(table),
            note: node.get("description").and_then(|d| d.as_str()).map(|s| s.to_string()),
            columns,
            pk,
            lookups,
        });
    }
    out
}

/// Emit `crates/application/src/generated/pm_state.rs` — the process-manager STATE persistence ports
/// (issue #27, replacing the hand-written `application/src/pm_state.rs`): one `<Base>Row` struct +
/// `<Base>StateStore` trait per table of `database/tables/process_managers.yaml`, plus the in-memory
/// `mem::Mem<Base>State` doubles the orchestrator tests run against. `last_update_utc` is the RUNTIME
/// ENVELOPE's stamp: every `upsert` writes it server-side (`now()`) and IGNORES the row's carried value.
pub(crate) fn emit_pm_state_application(model: &Model) -> String {
    let tables = parse_pm_tables(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/database/tables/process_managers.yaml — do not edit by hand.\n// Process-manager STATE persistence ports (ADR-20260719-172821): one row = one saga run, keyed by the\n// run's correlation identity. PRIVATE to their process manager — no projection reads them and no query\n// serves them, except the narrowly-scoped initiator reads a saga explicitly declares (paymentStatus,\n// ADR-20260720-015500). `last_update_utc` is maintained by the RUNTIME ENVELOPE, never by a step: every\n// `upsert` stamps it server-side (`now()`) — the value carried on the row is IGNORED on write. The\n// `mem` submodule provides in-memory doubles for the orchestrator tests.\n\nuse async_trait::async_trait;\nuse domain::generated::scalars::*;\nuse domain::shared::errors::DomainError;\n",
    );
    for t in &tables {
        // Row struct.
        out.push('\n');
        let note = ws1(t.note.as_deref().unwrap_or(""));
        if note.trim().is_empty() {
            out.push_str(&format!("/// One `{}` row.\n", t.table));
        } else {
            out.push_str(&format!("/// One `{}` row. {}\n", t.table, note.trim()));
        }
        out.push_str("#[derive(Debug, Clone, PartialEq)]\n");
        out.push_str(&format!("pub struct {}Row {{\n", t.base));
        for c in &t.columns {
            let ty = pm_ty(model, &t.table, &c.name, &c.ty);
            if c.name == "last_update_utc" {
                out.push_str("    /// Maintained by the runtime envelope — ignored on write, stamped `now()` by `upsert`.\n");
            } else if let Some(note) = &c.note {
                out.push_str(&format!("    /// {}\n", ws1(note)));
            }
            let ft = if c.nullable { format!("Option<{}>", ty.field()) } else { ty.field() };
            out.push_str(&format!("    pub {}: {},\n", rust_ident(&c.name), ft));
        }
        out.push_str("}\n");
        // Store trait.
        out.push('\n');
        out.push_str(&format!("/// State store for `{}` runs.\n#[async_trait]\npub trait {}StateStore: Send + Sync {{\n", t.table, t.base));
        for l in &t.lookups {
            let c = t.columns.iter().find(|c| c.name == l.column).unwrap();
            let ty = pm_ty(model, &t.table, &c.name, &c.ty);
            let param = if ty.by_ref() { format!("&{}", ty.field()) } else { ty.field() };
            out.push_str(&format!("    /// {}\n", l.doc));
            out.push_str(&format!(
                "    async fn {}(&self, {}: {}) -> Result<Option<{}Row>, DomainError>;\n\n",
                l.method,
                rust_ident(&l.column),
                param,
                t.base
            ));
        }
        out.push_str("    /// Insert or replace the run's row; `last_update_utc` is stamped server-side (`now()`).\n");
        out.push_str(&format!("    async fn upsert(&self, row: &{}Row) -> Result<(), DomainError>;\n}}\n", t.base));
    }
    // In-memory doubles.
    out.push_str(
        "\n/// In-memory implementations of the state-store ports (plain `Mutex<HashMap>`), for the process-manager\n/// orchestrator tests. They mirror the Postgres semantics: `upsert` replaces the whole row and stamps\n/// `last_update_utc = now()` (the row's own value is ignored), reads return the stored row.\npub mod mem {\n    use super::*;\n    use std::collections::HashMap;\n    use std::sync::Mutex;\n",
    );
    for t in &tables {
        let pk_col = t.columns.iter().find(|c| c.name == t.pk).unwrap();
        let pk_ty = pm_ty(model, &t.table, &pk_col.name, &pk_col.ty);
        let key = match &pk_ty {
            PmTy::UuidScalar(_) => "uuid::Uuid",
            PmTy::StringScalar(_) | PmTy::Text => "String",
            PmTy::IntScalar(_) => "i64",
            PmTy::Integer => "i32",
            other => panic!("process_managers.yaml#/{}: pk type {:?} not supported as a mem key", t.table, other.field()),
        };
        out.push('\n');
        out.push_str(&format!("    /// In-memory [`{}StateStore`], keyed by `{}`.\n    #[derive(Default)]\n    pub struct Mem{}State {{\n        rows: Mutex<HashMap<{}, {}Row>>,\n    }}\n", t.base, t.pk, t.base, key, t.base));
        out.push('\n');
        out.push_str(&format!("    #[async_trait]\n    impl {}StateStore for Mem{}State {{\n", t.base, t.base));
        for l in &t.lookups {
            let c = t.columns.iter().find(|c| c.name == l.column).unwrap();
            let ty = pm_ty(model, &t.table, &c.name, &c.ty);
            let param = if ty.by_ref() { format!("&{}", ty.field()) } else { ty.field() };
            let ident = rust_ident(&l.column);
            let body = if l.column == t.pk {
                format!("Ok(self.rows.lock().unwrap().get(&{}.0).cloned())", ident)
            } else if ty.by_ref() {
                format!("Ok(self.rows.lock().unwrap().values().find(|r| &r.{} == {}).cloned())", ident, ident)
            } else {
                format!("Ok(self.rows.lock().unwrap().values().find(|r| r.{} == {}).cloned())", ident, ident)
            };
            out.push_str(&format!(
                "        async fn {}(&self, {}: {}) -> Result<Option<{}Row>, DomainError> {{\n            {}\n        }}\n\n",
                l.method, ident, param, t.base, body
            ));
        }
        let key_expr = match &pk_ty {
            PmTy::StringScalar(_) => format!("stamped.{}.0.clone()", rust_ident(&t.pk)),
            PmTy::Text => format!("stamped.{}.clone()", rust_ident(&t.pk)),
            _ => format!("stamped.{}.0", rust_ident(&t.pk)),
        };
        out.push_str(&format!(
            "        async fn upsert(&self, row: &{}Row) -> Result<(), DomainError> {{\n            let mut stamped = row.clone();\n            stamped.last_update_utc = chrono::Utc::now();\n            self.rows.lock().unwrap().insert({}, stamped);\n            Ok(())\n        }}\n    }}\n",
            t.base, key_expr
        ));
    }
    out.push_str("}\n");
    out
}

/// Emit `crates/infrastructure/src/generated/pm_state.rs` — the Postgres adapters for the PM state
/// stores (issue #27, replacing the hand-written `infrastructure/persistence/pm_state.rs`): one
/// `Pg<Base>State` per `application::pm_state` port. Conventions match the projection stores: enum
/// columns are TEXT values held verbatim (`persistence::enum_sql`), scalar newtypes bind via
/// `.0`, upserts are `INSERT … ON CONFLICT (pk) DO UPDATE` over all columns, and `last_update_utc` is
/// stamped `now()` server-side on every upsert (the row's carried value is IGNORED).
pub(crate) fn emit_pm_state_infrastructure(model: &Model) -> String {
    let tables = parse_pm_tables(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/database/tables/process_managers.yaml — do not edit by hand.\n// Postgres adapters for the process-manager STATE stores (ADR-20260719-172821): one `Pg…State` per\n// `application::pm_state` port, over the saga state tables (migration\n// `20260719200000_process_manager_state_tables.sql`). Conventions match the projection stores: enum\n// columns are TEXT holding the scalars.yaml value verbatim (`crate::persistence::enum_sql`); scalar newtypes\n// bind via `.0`; upserts are `INSERT … ON CONFLICT (pk) DO UPDATE` over all columns. `last_update_utc`\n// is the runtime envelope's stamp: every upsert writes `now()` server-side (the row's carried value is\n// IGNORED), reads return the stored value.\n\nuse application::pm_state::*;\nuse async_trait::async_trait;\nuse domain::generated::scalars::*;\nuse domain::shared::errors::DomainError;\nuse sqlx::postgres::PgRow;\nuse sqlx::{PgPool, Row};\n\nuse crate::persistence::db_err;\nuse crate::persistence::enum_sql::EnumText;\n",
    );
    for t in &tables {
        let upper = snake_type(&t.base).to_uppercase();
        let snake = snake_type(&t.base);
        let cols: Vec<&str> = t.columns.iter().map(|c| c.name.as_str()).collect();
        out.push_str(&format!(
            "\n// ---------------------------------------------------------------------------------------------------\n// {}\n// ---------------------------------------------------------------------------------------------------\n",
            t.table
        ));
        // Column list const.
        out.push_str(&format!(
            "\n/// Column list of `{}`, in [`{}Row`] field order.\nconst {}_COLUMNS: &str = \"{}\";\n",
            t.table,
            t.base,
            upper,
            cols.join(", ")
        ));
        // Row decoder.
        out.push_str(&format!("\nfn decode_{}(row: &PgRow) -> Result<{}Row, DomainError> {{\n    Ok({}Row {{\n", snake, t.base, t.base));
        for c in &t.columns {
            let ty = pm_ty(model, &t.table, &c.name, &c.ty);
            let ident = rust_ident(&c.name);
            let expr = match (&ty, c.nullable) {
                (PmTy::UuidScalar(n), false) => format!("{}(row.try_get(\"{}\").map_err(db_err)?)", n, c.name),
                (PmTy::UuidScalar(n), true) => format!("row.try_get::<Option<uuid::Uuid>, _>(\"{}\").map_err(db_err)?.map({})", c.name, n),
                (PmTy::StringScalar(n), false) => format!("{}(row.try_get(\"{}\").map_err(db_err)?)", n, c.name),
                (PmTy::StringScalar(n), true) => format!("row.try_get::<Option<String>, _>(\"{}\").map_err(db_err)?.map({})", c.name, n),
                (PmTy::IntScalar(n), false) => format!("{}(row.try_get(\"{}\").map_err(db_err)?)", n, c.name),
                (PmTy::IntScalar(n), true) => format!("row.try_get::<Option<i64>, _>(\"{}\").map_err(db_err)?.map({})", c.name, n),
                (PmTy::EnumScalar(_), false) => format!("EnumText::from_text(&row.try_get::<String, _>(\"{}\").map_err(db_err)?)?", c.name),
                (PmTy::EnumScalar(_), true) => format!("crate::persistence::enum_sql::opt_from_text(row.try_get::<Option<String>, _>(\"{}\").map_err(db_err)?)?", c.name),
                (PmTy::Text, true) => format!("row.try_get::<Option<String>, _>(\"{}\").map_err(db_err)?", c.name),
                _ => format!("row.try_get(\"{}\").map_err(db_err)?", c.name),
            };
            out.push_str(&format!("        {}: {},\n", ident, expr));
        }
        out.push_str("    })\n}\n");
        // Store struct.
        out.push_str(&format!(
            "\n/// Postgres [`{}StateStore`] over `{}`.\npub struct Pg{}State {{\n    pool: PgPool,\n}}\n\nimpl Pg{}State {{\n    pub fn new(pool: PgPool) -> Self {{\n        Self {{ pool }}\n    }}\n}}\n",
            t.base, t.table, t.base, t.base
        ));
        // Trait impl.
        out.push_str(&format!("\n#[async_trait]\nimpl {}StateStore for Pg{}State {{\n", t.base, t.base));
        for l in &t.lookups {
            let c = t.columns.iter().find(|c| c.name == l.column).unwrap();
            let ty = pm_ty(model, &t.table, &c.name, &c.ty);
            let param = if ty.by_ref() { format!("&{}", ty.field()) } else { ty.field() };
            let ident = rust_ident(&l.column);
            let bind = match &ty {
                PmTy::StringScalar(_) => format!("{}.0.clone()", ident),
                PmTy::Text => format!("{}.clone()", ident),
                PmTy::EnumScalar(_) => format!("{}.to_text()", ident),
                PmTy::Integer => ident.clone(),
                _ => format!("{}.0", ident),
            };
            out.push_str(&format!(
                "    async fn {}(&self, {}: {}) -> Result<Option<{}Row>, DomainError> {{\n        let sql = format!(\"SELECT {{{}_COLUMNS}} FROM {} WHERE {} = $1\");\n        let row = sqlx::query(&sql).bind({}).fetch_optional(&self.pool).await.map_err(db_err)?;\n        row.as_ref().map(decode_{}).transpose()\n    }}\n\n",
                l.method, ident, param, t.base, upper, t.table, l.column, bind, snake
            ));
        }
        // Upsert: VALUES ($1..$n) with now() at last_update_utc's position; DO UPDATE over non-pk columns.
        let mut placeholders = Vec::new();
        let mut binds = Vec::new();
        let mut i = 0;
        for c in &t.columns {
            if c.name == "last_update_utc" {
                placeholders.push("now()".to_string());
                continue;
            }
            i += 1;
            placeholders.push(format!("${}", i));
            let ty = pm_ty(model, &t.table, &c.name, &c.ty);
            let ident = rust_ident(&c.name);
            let bind = match (&ty, c.nullable) {
                (PmTy::UuidScalar(_), false) => format!("row.{}.0", ident),
                (PmTy::UuidScalar(_), true) => format!("row.{}.as_ref().map(|v| v.0)", ident),
                (PmTy::StringScalar(_), false) => format!("row.{}.0.clone()", ident),
                (PmTy::StringScalar(_), true) => format!("row.{}.as_ref().map(|v| v.0.clone())", ident),
                (PmTy::IntScalar(_), false) => format!("row.{}.0", ident),
                (PmTy::IntScalar(_), true) => format!("row.{}.map(|v| v.0)", ident),
                (PmTy::EnumScalar(_), false) => format!("row.{}.to_text()", ident),
                (PmTy::EnumScalar(_), true) => format!("crate::persistence::enum_sql::opt_to_text(&row.{})", ident),
                (PmTy::Text, _) => format!("row.{}.clone()", ident),
                (PmTy::Integer, _) => format!("row.{}", ident),
                (PmTy::Timestamptz, _) => format!("row.{}", ident),
            };
            binds.push(bind);
        }
        let updates: Vec<String> = t
            .columns
            .iter()
            .filter(|c| !c.pk)
            .map(|c| {
                if c.name == "last_update_utc" {
                    "last_update_utc = now()".to_string()
                } else {
                    format!("{} = EXCLUDED.{}", c.name, c.name)
                }
            })
            .collect();
        out.push_str(&format!(
            "    async fn upsert(&self, row: &{}Row) -> Result<(), DomainError> {{\n        upsert_{}_with(&self.pool, row).await\n    }}\n}}\n",
            t.base, snake
        ));
        // Executor-generic upsert: ONE SQL body serving both the pool-backed trait impl above and
        // the fenced completion transaction (#272 Runtime D1, ADR-20260801-023000 — a PM row must
        // commit atomically with the staged events and the mailbox verdict, and a hand-mirrored
        // copy of this statement would drift silently).
        out.push_str(&format!(
            "\n/// Upsert one `{}` row through ANY PgExecutor — the pool (the trait impl delegates here)\n/// or the fenced completion transaction (#272 Runtime D1, ADR-20260801-023000).\npub async fn upsert_{}_with<'e, E: sqlx::PgExecutor<'e>>(executor: E, row: &{}Row) -> Result<(), DomainError> {{\n    let sql = format!(\n        \"INSERT INTO {} ({{{}_COLUMNS}}) \\\n         VALUES ({}) \\\n         ON CONFLICT ({}) DO UPDATE SET \\\n         {}\"\n    );\n    sqlx::query(&sql)\n",
            t.table,
            snake,
            t.base,
            t.table,
            upper,
            placeholders.join(","),
            t.pk,
            updates.join(", \\\n         ")
        ));
        for b in &binds {
            out.push_str(&format!("        .bind({})\n", b));
        }
        out.push_str("        .execute(executor)\n        .await\n        .map_err(db_err)?;\n    Ok(())\n}\n");
    }
    out
}

