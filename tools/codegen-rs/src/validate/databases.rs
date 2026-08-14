//! §18 — database placement (#494 slice 1: the declaration site).
//!
//! Decided by PROP-20260811-093000 (DECISIONS §32 rows STO-1(a)/STO-2(a)) and ADR-20260812-115930
//! "Each adapter owns its own, completely isolated database" (ADP-1, both legs closed). The catalog
//! is `specs/database/databases.yaml`: every database as a declaration — name, owning role,
//! K8s object-name binding, recovery posture — and NOTHING else (grants are #513, migration chains
//! #514, drill legs #509).
//!
//! PLACEMENT IS DERIVED WHERE THE SOURCE EXISTS AND DECLARED ONLY WHERE IT DOES NOT (compiler-first,
//! ADR-20260803-234035):
//!
//! - The four write-side kinds (event store · journals · PM state · reservations) derive
//!   [`WRITE_DATABASE`] mechanically from the §1b table-kind classifier. STO-1(a) is a correctness
//!   fact, not a choice: the fenced completion transaction commits the handler's appends, the
//!   mailbox terminal flip AND the fenced `mailbox_partitions` checkpoint advance in ONE
//!   transaction — separating the log from the mailbox deletes the FENCING TOKEN, so a `database:`
//!   key on these kinds is refused outright rather than checked for agreement (a declaration that
//!   can only restate a derivation is a second place to get it wrong).
//! - Staging and connection tables DECLARE `database:` as a `$ref` into the catalog. Absence is an
//!   ERROR, never `captain_write`-by-omission — a defaulted placement is how a credential store
//!   lands next to `domain_events` silently, which is the exact hole ADP-1 closes.
//! - `replicated: read-databases` is the replicated placement class (STO-2(a), consciously
//!   EXTENDED by the STO-2 closure to seeded referentials: `ScopeMembership` and the pricing
//!   referentials `PricingPolicy`/`UberEstimationPolicy`/`UberSplitPolicy`). The resolved set is
//!   every database declared `recovery: replay` — that equivalence is definitional, not a naming
//!   pun: a read database is precisely one whose whole content is a re-derivable fold of the log
//!   (a seeded referential's replicated copies are re-seeded, which is why the seed script is part
//!   of each read database's recovery path).
//!
//! COMPLETENESS IS PER-KIND, WITH NO DEFAULT — AND SINCE STO-2's ROW CLOSED (2026-08-14, DECISIONS
//! §32 "STO-2 closure"; #562) IT COVERS EVERY BUSINESS KIND: projection and referential tables
//! declare `database:` as a `$ref` (single-home) or `replicated: read-databases`, and ABSENCE IS
//! AN ERROR for them exactly as for staging/connection tables. The requirement CONSUMES the same
//! resolution the inventory emitter walks ([`resolve_placements`]): a covered-kind table ABSENT
//! from the resolved output with no more specific diagnostic is an error, so the validator and
//! `databases.generated.{md,json}` cannot disagree by construction — a naive standalone check
//! could report green while the emitter silently dropped the table.
//!
//! The `tracking` database (`BehaviorEventTrackingDb`) is declared with ZERO tables and there is
//! deliberately NO waiver mechanism recorded for it: its first table owes a placement and the
//! PROP §9.4 retention policy in the same change (a declared retention policy ships WITH the first
//! tracking table, not after).
//!
//! The ADP-1 membership check is the error ADR-20260812-115930 documents nearly shipping: avelo37
//! would have been the one partner mirror still holding CONNECT on the write database while every
//! sibling moved out. Here, a table that names an adapter must live in THAT adapter's database, and
//! an adapter database holds nothing foreign — flip `external_avelo37_events` out of
//! `adapter_avelo37` and the validator is red naming exactly the mismatch.

use crate::*;

/// The catalog file, as the loader keys it.
pub(crate) const DATABASES_FILE: &str = "database/databases.yaml";

/// The write-side transactional unit (STO-1(a), renamed from the directive's `DomainEventLogDb` so
/// the contents are not surprising). A constant, not a lookup: the derivation target is part of the
/// recorded decision, and the catalog is REQUIRED to declare it.
pub(crate) const WRITE_DATABASE: &str = "captain_write";

/// The closed recovery-posture set (loader-schema closure — category 3 of ADR-20260811-014129 D2:
/// a value from a closed set stays a bare token because the set is closed HERE).
pub(crate) const RECOVERY_POSTURES: &[&str] = &["pitr", "replay", "refetch", "backup-required"];

/// The one replicated-placement token (closed set of one; an enumerated database set is grammar for
/// the day a table replicates to a strict subset — not spelled until something needs it).
pub(crate) const REPLICATED_TOKEN: &str = "read-databases";

/// Table kinds whose placement is DERIVED into [`WRITE_DATABASE`] (STO-1(a) — the fenced completion
/// transaction's footprint).
pub(crate) const DERIVED_WRITE_KINDS: &[refs::Kind] = &[
    refs::Kind::EventStoreTable,
    refs::Kind::JournalTable,
    refs::Kind::PmStateTable,
    refs::Kind::ReservationTable,
];

/// Table kinds that MUST declare a single-home `database:` placement (ADP-1: adapter-owned state
/// plus the two platform connection tables).
pub(crate) const DECLARED_KINDS: &[refs::Kind] = &[refs::Kind::StagingTable, refs::Kind::ConnectionTable];

/// Table kinds that MUST declare a placement as EITHER a single-home `database:` `$ref` OR
/// `replicated: read-databases` (the business kinds — STO-2 closed 2026-08-14, DECISIONS §32
/// "STO-2 closure"). Only these kinds may take the replicated class; anything else declaring it
/// is refused.
pub(crate) const REPLICABLE_KINDS: &[refs::Kind] = &[refs::Kind::ProjectionTable, refs::Kind::ReferentialTable];

/// One declared database, verbatim from the catalog.
pub(crate) struct DatabaseDecl {
    pub(crate) name: String,
    pub(crate) owner: String,
    pub(crate) k8s_name: String,
    pub(crate) recovery: String,
    pub(crate) description: String,
}

/// Parse the catalog leniently (missing fields become empty strings); §18's decl checks report the
/// gaps. The emitter runs only after validation is green, so it may consume this as-is.
pub(crate) fn parse_databases(model: &Model) -> Vec<DatabaseDecl> {
    let mut out = Vec::new();
    if let Some(Value::Mapping(m)) = model.defs.get(DATABASES_FILE) {
        for (k, v) in m {
            let get = |f: &str| v.get(f).and_then(|x| x.as_str()).unwrap_or("").to_string();
            out.push(DatabaseDecl {
                name: k.as_str().unwrap_or("?").to_string(),
                owner: get("owner"),
                k8s_name: get("k8sName"),
                recovery: get("recovery"),
                description: get("description"),
            });
        }
    }
    out
}

/// How a table came to be placed — exposed in the generated inventory so #509/#513/#514 read the
/// provenance, not just the answer.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlacementMode {
    /// Derived into [`WRITE_DATABASE`] from the table kind (STO-1(a)).
    Derived,
    /// Declared per table with a `database:` `$ref`.
    Declared,
    /// The replicated class: resolved to every `recovery: replay` database (STO-2(a)).
    Replicated,
}

impl PlacementMode {
    pub(crate) fn name(self) -> &'static str {
        match self {
            PlacementMode::Derived => "derived",
            PlacementMode::Declared => "declared",
            PlacementMode::Replicated => "replicated",
        }
    }
}

/// A table's RESOLVED placement: always a SET of databases, even while every single-home value is a
/// singleton — the erasure sweep (#528) and the grant emitter (#513) need the full set shape.
pub(crate) struct TablePlacement {
    pub(crate) table: String,
    pub(crate) kind: refs::Kind,
    pub(crate) mode: PlacementMode,
    pub(crate) databases: Vec<String>,
}

/// The declared-database name a table's `database:` key points at, if it is a well-formed `$ref`
/// into the catalog with a single pointer segment.
fn declared_database(node: &Value) -> Option<String> {
    let r = node.get("database")?.get("$ref")?.as_str()?;
    let pr = parse_ref(r)?;
    if pr.file != DATABASES_FILE || pr.path.len() != 1 {
        return None;
    }
    Some(pr.path[0].clone())
}

/// Resolve every table of a covered kind to its database set. Pure resolution — pushes no issues,
/// skips what does not resolve; [`validate_databases`] reports the gaps. Sorted by table name.
pub(crate) fn resolve_placements(model: &Model) -> Vec<TablePlacement> {
    let decls = parse_databases(model);
    let declared: BTreeSet<&str> = decls.iter().map(|d| d.name.as_str()).collect();
    let replay: Vec<String> = decls
        .iter()
        .filter(|d| d.recovery == "replay")
        .map(|d| d.name.clone())
        .collect();
    let mut out = Vec::new();
    for (table, (file, kind)) in validate::table_kinds(model) {
        let node = match model.defs.get(&file).and_then(|v| v.get(table.as_str())) {
            Some(n) => n,
            None => continue,
        };
        if DERIVED_WRITE_KINDS.contains(&kind) {
            out.push(TablePlacement {
                table,
                kind,
                mode: PlacementMode::Derived,
                databases: vec![WRITE_DATABASE.to_string()],
            });
        } else if DECLARED_KINDS.contains(&kind) || REPLICABLE_KINDS.contains(&kind) {
            if let Some(db) = declared_database(node).filter(|db| declared.contains(db.as_str())) {
                out.push(TablePlacement { table, kind, mode: PlacementMode::Declared, databases: vec![db] });
            } else if REPLICABLE_KINDS.contains(&kind)
                && node.get("replicated").and_then(|v| v.as_str()) == Some(REPLICATED_TOKEN)
            {
                out.push(TablePlacement {
                    table,
                    kind,
                    mode: PlacementMode::Replicated,
                    databases: replay.clone(),
                });
            }
        }
    }
    out
}

/// The adapter token a table name carries, if any: the token (an `adapter_{token}` database's
/// suffix) whose `_`-segments appear as a CONTIGUOUS run in the table name's segments —
/// `external_uber_direct_events` carries `uber_direct`; `auth_sessions` carries none. Longest token
/// wins so `uber_direct` can never be shadowed by a hypothetical `uber`.
fn adapter_token_of<'a>(table: &str, tokens: &'a [String]) -> Option<&'a String> {
    let segs: Vec<&str> = table.split('_').collect();
    let mut hit: Option<&String> = None;
    for tok in tokens {
        let ts: Vec<&str> = tok.split('_').collect();
        let found = (0..segs.len().saturating_sub(ts.len() - 1)).any(|i| segs[i..i + ts.len()] == ts[..]);
        if found && hit.map(|h| h.len() < tok.len()).unwrap_or(true) {
            hit = Some(tok);
        }
    }
    hit
}

/// §18 — the catalog's own hygiene, then per-kind placement completeness and the ADP-1 membership
/// wall. Every check is an ERROR: this family gates, it does not advise.
pub(crate) fn validate_databases(model: &Model, issues: &mut Vec<Issue>) {
    let decls = parse_databases(model);

    // ── 18a. The catalog's declarations are well-formed ──────────────────────────────────────────
    let snake = |s: &str| {
        !s.is_empty()
            && s.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false)
            && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    let rfc1123 = |s: &str| {
        !s.is_empty()
            && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !s.starts_with('-')
            && !s.ends_with('-')
    };
    let mut k8s_seen: BTreeMap<&str, &str> = BTreeMap::new();
    for d in &decls {
        let loc = format!("{}/{}", DATABASES_FILE, d.name);
        if !snake(&d.name) {
            issues.push(err(
                "database-name-not-underscored",
                loc.clone(),
                format!(
                    "database name '{}' must be lowercase snake_case (`adapter_stripe`, never `adapter-stripe`): \
                     a hyphen in a Postgres database name is a quoted identifier in every DSN and GRANT forever. \
                     The Kubernetes spelling is the `k8sName` binding, not the name.",
                    d.name
                ),
            ));
        }
        if !snake(&d.owner) {
            issues.push(err(
                "database-decl-invalid",
                loc.clone(),
                format!("database '{}' needs an `owner:` role (lowercase snake_case), got '{}'.", d.name, d.owner),
            ));
        }
        if !rfc1123(&d.k8s_name) {
            issues.push(err(
                "database-decl-invalid",
                loc.clone(),
                format!(
                    "database '{}' needs a `k8sName:` (RFC 1123: lowercase alphanumerics and '-', no underscore \
                     — the CNPG Database CR object name), got '{}'.",
                    d.name, d.k8s_name
                ),
            ));
        } else if let Some(prev) = k8s_seen.insert(d.k8s_name.as_str(), d.name.as_str()) {
            issues.push(err(
                "database-decl-invalid",
                loc.clone(),
                format!("k8sName '{}' is already bound by database '{}' — one object name, one database.", d.k8s_name, prev),
            ));
        }
        if !RECOVERY_POSTURES.contains(&d.recovery.as_str()) {
            issues.push(err(
                "database-decl-invalid",
                loc.clone(),
                format!(
                    "database '{}' needs a `recovery:` posture from {:?} (the drill slice #509 consumes it), got '{}'.",
                    d.name, RECOVERY_POSTURES, d.recovery
                ),
            ));
        }
        if d.description.is_empty() {
            issues.push(err(
                "database-decl-invalid",
                loc,
                format!("database '{}' needs a `description:` — a declaration with no rationale is a name squat.", d.name),
            ));
        }
    }
    let declared: BTreeSet<&str> = decls.iter().map(|d| d.name.as_str()).collect();
    // Required the moment any covered-kind table exists (so a fixture/degenerate model with no
    // store needs no catalog, while the real tree cannot lose it silently).
    let any_covered = validate::table_kinds(model)
        .values()
        .any(|(_, k)| DERIVED_WRITE_KINDS.contains(k) || DECLARED_KINDS.contains(k));
    if any_covered && !declared.contains(WRITE_DATABASE) {
        issues.push(err(
            "database-write-unit-undeclared",
            DATABASES_FILE.to_string(),
            format!(
                "the catalog does not declare '{}' — the write-side transactional unit every derived placement \
                 (event store, journals, PM state, reservations) resolves into (STO-1(a)).",
                WRITE_DATABASE
            ),
        ));
    }
    let replay_count = decls.iter().filter(|d| d.recovery == "replay").count();
    let adapter_tokens: Vec<String> = decls
        .iter()
        .filter_map(|d| d.name.strip_prefix("adapter_").map(|t| t.to_string()))
        .collect();

    // ── 18b. Per-kind placement completeness + the ADP-1 membership wall ─────────────────────────
    // THE REQUIREMENT CONSUMES THE RESOLUTION (#562): `resolved` is the inventory emitter's own
    // walk. A covered-kind table absent from it must leave an error behind — the specific
    // diagnostics below say WHY it is absent (bad `$ref`, unknown database, bad replicated token),
    // and the `database-placement-missing` fallback fires when no diagnostic did — so the
    // validator and `databases.generated.{md,json}` cannot disagree by construction.
    let resolved: BTreeSet<String> = resolve_placements(model).into_iter().map(|p| p.table).collect();
    for (table, (file, kind)) in validate::table_kinds(model) {
        let node = match model.defs.get(&file).and_then(|v| v.get(table.as_str())) {
            Some(n) => n,
            None => continue,
        };
        let loc = format!("{}/{}", file, table);
        let before = issues.len();
        let has_db = node.get("database").is_some();
        let has_repl = node.get("replicated").is_some();
        if has_db && has_repl {
            issues.push(err(
                "database-placement-conflict",
                loc.clone(),
                format!("table '{}' declares BOTH `database:` and `replicated:` — a table is single-home or replicated, never both.", table),
            ));
        }
        if DERIVED_WRITE_KINDS.contains(&kind) {
            if has_db || has_repl {
                issues.push(err(
                    "database-placement-not-declarable",
                    loc,
                    format!(
                        "'{}' is a {} — its placement is DERIVED into '{}' from the table kind (STO-1(a)): the fenced \
                         completion transaction commits appends, the mailbox terminal flip and the fenced checkpoint \
                         advance atomically, and moving one of these tables deletes the fencing token. Remove the key; \
                         the derivation is not overridable.",
                        table,
                        kind.name(),
                        WRITE_DATABASE
                    ),
                ));
            }
        } else if DECLARED_KINDS.contains(&kind) || REPLICABLE_KINDS.contains(&kind) {
            let replicable = REPLICABLE_KINDS.contains(&kind);
            if !replicable && has_repl {
                issues.push(err(
                    "database-placement-not-declarable",
                    loc.clone(),
                    format!(
                        "'{}' is a {} — adapter/platform-owned single-home state; `replicated:` is the read-model \
                         class (STO-2(a)) and replicating a staging mirror or credential store has no meaning.",
                        table,
                        kind.name()
                    ),
                ));
            }
            if replicable {
                if let Some(v) = node.get("replicated") {
                    match v.as_str() {
                        Some(REPLICATED_TOKEN) => {
                            if replay_count == 0 {
                                issues.push(err(
                                    "database-replicated-empty",
                                    loc.clone(),
                                    format!(
                                        "table '{}' is `replicated: {}` but no database declares `recovery: replay` — \
                                         the replicated class resolves to the read databases, and an empty set is a \
                                         placement to nowhere.",
                                        table, REPLICATED_TOKEN
                                    ),
                                ));
                            }
                        }
                        _ => issues.push(err(
                            "database-placement-invalid",
                            loc.clone(),
                            format!(
                                "table '{}' has `replicated: {:?}` — the only replicated class is the bare token \
                                 '{}' (every `recovery: replay` database).",
                                table, v, REPLICATED_TOKEN
                            ),
                        )),
                    }
                }
            }
            // The single-home value validation — ONE path for staging/connection AND the business
            // kinds (beck's trap (b): a bare string or an unknown database must not satisfy the
            // requirement on any kind), ending in the ADP-1 membership wall, HOISTED here from the
            // staging/connection-only branch so it runs on every resolved single-home placement.
            if let Some(v) = node.get("database") {
                match declared_database(node) {
                    Some(db) if !declared.contains(db.as_str()) => {
                        issues.push(err(
                            "database-placement-unknown-database",
                            loc.clone(),
                            format!(
                                "table '{}' is placed in '{}', which {} does not declare — a placement must name \
                                 one of the {} databases that file declares.",
                                table,
                                db,
                                DATABASES_FILE,
                                declared.len()
                            ),
                        ));
                    }
                    Some(db) => {
                        // The ADP-1 membership wall, both directions.
                        match adapter_token_of(&table, &adapter_tokens) {
                            Some(tok) => {
                                let expected = format!("adapter_{}", tok);
                                if db != expected {
                                    issues.push(err(
                                        "database-adapter-mismatch",
                                        loc.clone(),
                                        format!(
                                            "table '{}' carries adapter '{}' in its name but is placed in '{}' — it belongs in \
                                             '{}': each adapter owns its own, completely isolated database \
                                             (ADR-20260812-115930; this is the error that nearly shipped, with avelo37 as the \
                                             one mirror left inside the write database).",
                                            table, tok, db, expected
                                        ),
                                    ));
                                }
                            }
                            None => {
                                if db.starts_with("adapter_") {
                                    issues.push(err(
                                        "database-adapter-mismatch",
                                        loc.clone(),
                                        format!(
                                            "table '{}' names no adapter but is placed in '{}' — an adapter database holds \
                                             nothing foreign (ADR-20260812-115930's inward clause: no role other than the \
                                             database's one owning app holds CONNECT).",
                                            table, db
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                    None => {
                        issues.push(err(
                            "database-placement-not-a-ref",
                            loc.clone(),
                            format!(
                                "table '{}' has a `database:` that is not a single-segment `$ref` into {} (got {:?}). \
                                 A bare string is invisible to the refs walker (ADR-20260811-014129 D2, the #413 \
                                 class) — write `database: {{ $ref: '{}#/<name>' }}`.",
                                table, DATABASES_FILE, v, DATABASES_FILE
                            ),
                        ));
                    }
                }
            }
            // The completeness fallback: absent from the emitter's resolution and no diagnostic
            // above explains why ⇒ the placement is MISSING.
            if !resolved.contains(&table) && issues.len() == before {
                let requirement = if replicable {
                    format!(
                        "Every projection/referential table declares `database:` as a `$ref` into {} or \
                         `replicated: {}` — STO-2 CLOSED 2026-08-14 (DECISIONS §32 \"STO-2 closure\"): absence \
                         is an error, never a default, and a table missing here would otherwise be silently \
                         absent from the generated inventory.",
                        DATABASES_FILE, REPLICATED_TOKEN
                    )
                } else {
                    format!(
                        "Every staging/connection table names its database as a `$ref` into {} — absence is an \
                         error, never '{}'-by-omission (ADP-1, ADR-20260812-115930).",
                        DATABASES_FILE, WRITE_DATABASE
                    )
                };
                issues.push(err(
                    "database-placement-missing",
                    loc,
                    format!("table '{}' ({}) declares no placement. {}", table, kind.name(), requirement),
                ));
            }
        } else if has_db || has_repl {
            issues.push(err(
                "database-placement-not-declarable",
                loc,
                format!(
                    "'{}' is a {} — no placement grammar is open for this kind (see DECISIONS §32).",
                    table,
                    kind.name()
                ),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A well-formed catalog fixture — the write unit, one read database (so `replicated:` resolves
    /// to a non-empty set) and the two adapter databases the table fixtures place into. Tests mutate
    /// a clone of this or drop a database from it.
    fn catalog() -> String {
        [
            "captain_write:",
            "  owner: migrator",
            "  k8sName: captain-write",
            "  recovery: pitr",
            "  description: the write side",
            "read_order:",
            "  owner: migrator",
            "  k8sName: read-order",
            "  recovery: replay",
            "  description: the order read database",
            "adapter_stripe:",
            "  owner: adapter_stripe",
            "  k8sName: adapter-stripe",
            "  recovery: refetch",
            "  description: stripe isolated database",
            "adapter_avelo37:",
            "  owner: adapter_avelo37",
            "  k8sName: adapter-avelo37",
            "  recovery: refetch",
            "  description: avelo37 isolated database",
        ]
        .join("\n")
    }

    /// Build a model from a `(logical-file, yaml)` list — the tests.rs fixture pattern.
    fn model(files: &[(&str, &str)]) -> Model {
        let mut defs = BTreeMap::new();
        for (f, y) in files {
            defs.insert((*f).to_string(), serde_yaml::from_str::<Value>(y).expect("valid yaml"));
        }
        Model { defs, ..Default::default() }
    }

    /// `Issue` is not `Debug`; render the database-* subset for assertion messages, and return only
    /// those so an unrelated rule never masks or fakes a pass.
    fn db_issues(model: &Model) -> Vec<Issue> {
        let mut issues = Vec::new();
        validate_databases(model, &mut issues);
        issues.into_iter().filter(|i| i.rule.starts_with("database-")).collect()
    }
    fn render(issues: &[Issue]) -> String {
        issues.iter().map(|i| format!("  {} {}: {}", i.rule, i.location, i.message)).collect::<Vec<_>>().join("\n")
    }
    fn has(issues: &[Issue], rule: &str) -> bool {
        issues.iter().any(|i| i.rule == rule)
    }

    /// The baseline is CLEAN — every arm below is proven to fire only against its mutant, not
    /// against a well-formed catalog. Two adapter mirrors, each in its own database; one derived
    /// write table; one replicated read model.
    #[test]
    fn a_well_formed_catalog_raises_nothing() {
        let m = model(&[
            ("database/databases.yaml", &catalog()),
            ("database/tables/eventstore.yaml", "domain_events:\n  columns:\n    id: { type: uuid }\n"),
            (
                "database/tables/integration_staging.yaml",
                "external_stripe_events:\n  staging: true\n  database: { $ref: 'database/databases.yaml#/adapter_stripe' }\n\
                 external_avelo37_events:\n  staging: true\n  database: { $ref: 'database/databases.yaml#/adapter_avelo37' }\n",
            ),
            (
                "database/tables/projection_tables.yaml",
                "ScopeMembership:\n  replicated: read-databases\n  columns:\n    id: { type: uuid }\n",
            ),
        ]);
        let issues = db_issues(&m);
        assert!(issues.is_empty(), "expected a clean baseline, got:\n{}", render(&issues));
    }

    /// A covered-kind table with no placement key — the credential-store-next-to-the-log hole.
    #[test]
    fn a_staging_table_with_no_placement_is_missing() {
        let m = model(&[
            ("database/databases.yaml", &catalog()),
            ("database/tables/integration_staging.yaml", "external_stripe_events:\n  staging: true\n"),
        ]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-placement-missing"), "{}", render(&issues));
    }

    /// A placement `$ref` into a database the catalog does not declare.
    #[test]
    fn a_placement_into_an_undeclared_database_is_flagged() {
        let m = model(&[
            ("database/databases.yaml", &catalog()),
            (
                "database/tables/integration_staging.yaml",
                "external_stripe_events:\n  staging: true\n  database: { $ref: 'database/databases.yaml#/adapter_identity' }\n",
            ),
        ]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-placement-unknown-database"), "{}", render(&issues));
    }

    /// ADP-1 wall, direction 1: an adapter's table placed OUTSIDE its own database. This is the
    /// error ADR-20260812-115930 documents nearly shipping (avelo37 left inside the write DB).
    #[test]
    fn an_adapter_table_outside_its_own_database_is_a_mismatch() {
        let m = model(&[
            ("database/databases.yaml", &catalog()),
            (
                "database/tables/integration_staging.yaml",
                "external_avelo37_events:\n  staging: true\n  database: { $ref: 'database/databases.yaml#/captain_write' }\n",
            ),
        ]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-adapter-mismatch"), "{}", render(&issues));
        assert!(issues.iter().any(|i| i.message.contains("adapter_avelo37")), "{}", render(&issues));
    }

    /// ADP-1 wall, direction 2: a table naming no adapter placed INSIDE an adapter database — the
    /// inward clause (an adapter database holds nothing foreign).
    #[test]
    fn a_foreign_table_inside_an_adapter_database_is_a_mismatch() {
        let m = model(&[
            ("database/databases.yaml", &catalog()),
            (
                "database/tables/integration_connections.yaml",
                "auth_sessions:\n  database: { $ref: 'database/databases.yaml#/adapter_stripe' }\n  columns:\n    id: { type: uuid }\n",
            ),
        ]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-adapter-mismatch"), "{}", render(&issues));
    }

    /// A `database:` key on a DERIVED write-side kind: the transactional unit is not overridable
    /// one table at a time (STO-1(a) — the fencing token).
    #[test]
    fn a_placement_on_a_derived_write_kind_is_not_declarable() {
        let m = model(&[
            ("database/databases.yaml", &catalog()),
            (
                "database/tables/eventstore.yaml",
                "domain_events:\n  database: { $ref: 'database/databases.yaml#/captain_write' }\n  columns:\n    id: { type: uuid }\n",
            ),
        ]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-placement-not-declarable"), "{}", render(&issues));
    }

    /// #562's direction-2 witness — the INVERSION (same fixture, flipped assertion) of the
    /// pre-closure test `a_placement_on_an_open_business_kind_is_refused`: STO-2 CLOSED 2026-08-14
    /// (DECISIONS §32 "STO-2 closure"), so a `database:` on a business kind is the requirement
    /// being SATISFIED, never a refusal. Seen RED before the flip (2026-08-14) with:
    /// `database-placement-not-declarable ... business-table placement is register row STO-2's
    /// OPEN remainder`.
    #[test]
    fn a_placement_on_a_business_kind_is_accepted_since_sto2_closed() {
        let m = model(&[
            ("database/databases.yaml", &catalog()),
            (
                "database/tables/referential.yaml",
                "PricingPolicy:\n  reference: true\n  database: { $ref: 'database/databases.yaml#/read_order' }\n  columns:\n    id: { type: uuid }\n",
            ),
        ]);
        let issues = db_issues(&m);
        assert!(issues.is_empty(), "a declared business placement satisfies the closed-row requirement, got:\n{}", render(&issues));
    }

    // ── #562: STO-2 closed — placement is a REQUIREMENT on the replicable kinds, proven against
    // the same resolution the inventory emitter walks (a table of a covered kind absent from
    // `resolve_placements` output is an error, so validate and emit cannot disagree by
    // construction). One red mutant per RULE direction, no per-table matrix.

    /// A projection table with NO placement key — RED before the flip (nothing fired), the
    /// requirement's first witness.
    #[test]
    fn a_projection_table_with_no_placement_is_missing() {
        let m = model(&[
            ("database/databases.yaml", &catalog()),
            (
                "database/tables/projection_tables.yaml",
                "OrderTracking:\n  columns:\n    id: { type: uuid }\n",
            ),
        ]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-placement-missing"), "{}", render(&issues));
    }

    /// A referential table with NO placement key — the second REPLICABLE_KIND's witness.
    #[test]
    fn a_referential_table_with_no_placement_is_missing() {
        let m = model(&[
            ("database/databases.yaml", &catalog()),
            (
                "database/tables/referential.yaml",
                "PricingPolicy:\n  reference: true\n  columns:\n    id: { type: uuid }\n",
            ),
        ]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-placement-missing"), "{}", render(&issues));
    }

    /// A bare-string `database:` on a replicable kind must NOT satisfy the requirement — the #413
    /// invisible-to-the-refs-walker class, enforced on the widened kinds exactly as on
    /// staging/connection tables (beck's trap (b)).
    #[test]
    fn a_bare_string_placement_on_a_replicable_kind_is_not_a_ref() {
        let m = model(&[
            ("database/databases.yaml", &catalog()),
            (
                "database/tables/referential.yaml",
                "PricingPolicy:\n  reference: true\n  database: read_order\n  columns:\n    id: { type: uuid }\n",
            ),
        ]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-placement-not-a-ref"), "{}", render(&issues));
        assert!(!has(&issues, "database-placement-missing"), "one gap, one diagnostic:\n{}", render(&issues));
    }

    /// A replicable kind's `$ref` into a database the catalog does not declare — same value
    /// validation as the DECLARED_KINDS path.
    #[test]
    fn an_unknown_database_on_a_replicable_kind_is_flagged() {
        let m = model(&[
            ("database/databases.yaml", &catalog()),
            (
                "database/tables/projection_tables.yaml",
                "OrderTracking:\n  database: { $ref: 'database/databases.yaml#/read_nowhere' }\n  columns:\n    id: { type: uuid }\n",
            ),
        ]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-placement-unknown-database"), "{}", render(&issues));
    }

    /// The ADP-1 wall is HOISTED, not duplicated: a projection table placed inside an adapter
    /// database trips the inward clause exactly as a connection table does (farley's trap (c) —
    /// before the hoist the wall ran only on the DECLARED_KINDS branch).
    #[test]
    fn a_projection_table_inside_an_adapter_database_is_a_mismatch() {
        let m = model(&[
            ("database/databases.yaml", &catalog()),
            (
                "database/tables/projection_tables.yaml",
                "OrderTracking:\n  database: { $ref: 'database/databases.yaml#/adapter_stripe' }\n  columns:\n    id: { type: uuid }\n",
            ),
        ]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-adapter-mismatch"), "{}", render(&issues));
    }

    /// The resolver's replicable-declared branch (farley's trap (a)): a business table with a valid
    /// `database:` `$ref` is PRESENT in `resolve_placements` output — the emitter walks the same
    /// resolution the requirement consumes, so validator-green while inventory-absent is
    /// unrepresentable.
    #[test]
    fn a_declared_replicable_placement_resolves_into_the_inventory() {
        let m = model(&[
            ("database/databases.yaml", &catalog()),
            (
                "database/tables/referential.yaml",
                "PricingPolicy:\n  reference: true\n  database: { $ref: 'database/databases.yaml#/read_order' }\n  columns:\n    id: { type: uuid }\n",
            ),
            (
                "database/tables/projection_tables.yaml",
                "ScopeMembership:\n  replicated: read-databases\n  columns:\n    id: { type: uuid }\n",
            ),
        ]);
        let placements = resolve_placements(&m);
        let pricing = placements.iter().find(|p| p.table == "PricingPolicy").expect("declared business placement resolves");
        assert!(pricing.mode == PlacementMode::Declared && pricing.databases == vec!["read_order".to_string()]);
        let membership = placements.iter().find(|p| p.table == "ScopeMembership").expect("replicated placement resolves");
        assert!(membership.mode == PlacementMode::Replicated && membership.databases == vec!["read_order".to_string()]);
    }

    /// `replicated:` with no `recovery: replay` database in the catalog — a placement to nowhere.
    #[test]
    fn a_replicated_table_with_no_read_database_is_empty() {
        // Catalog with the read database dropped: only captain_write + the two adapters remain.
        let no_read: String = catalog()
            .lines()
            .collect::<Vec<_>>()
            .chunks(5)
            .filter(|c| !c.first().map(|l| l.starts_with("read_order")).unwrap_or(false))
            .flatten()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        let m = model(&[
            ("database/databases.yaml", &no_read),
            (
                "database/tables/projection_tables.yaml",
                "ScopeMembership:\n  replicated: read-databases\n  columns:\n    id: { type: uuid }\n",
            ),
        ]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-replicated-empty"), "{}", render(&issues));
    }

    /// Declaring BOTH `database:` and `replicated:` — single-home or replicated, never both.
    #[test]
    fn declaring_both_placement_keys_conflicts() {
        let m = model(&[
            ("database/databases.yaml", &catalog()),
            (
                "database/tables/projection_tables.yaml",
                "ScopeMembership:\n  replicated: read-databases\n  database: { $ref: 'database/databases.yaml#/read_order' }\n  columns:\n    id: { type: uuid }\n",
            ),
        ]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-placement-conflict"), "{}", render(&issues));
    }

    // ── §18 red-test coverage completion (#546): the arms two review passes named as having no
    // fixture forcing them red. Each mutant trips exactly its check; the `database-decl-invalid`
    // family shares one slug, so those assert additionally on the distinguishing message substring so
    // each of the five sub-checks is individually provable (a single fixture tripping all five would
    // not prove each fires). The anchor `a_well_formed_catalog_raises_nothing` already exercises every
    // new arm's happy path (staging with a valid `$ref` = the not-a-ref happy path; valid decls = all
    // §18a happy paths; declared `captain_write` = the write-unit happy path; the valid `read-databases`
    // token = the placement-invalid happy path), so no false positive slips through.

    /// A single well-formed database declaration, field by field, so a §18a hygiene test mutates
    /// exactly ONE line and proves exactly ONE check fires. A catalog-only model exercises 18a in
    /// isolation: with no tables, `any_covered` is false and the 18b placement loop never runs.
    fn one_database(name: &str, owner: &str, k8s: &str, recovery: &str, description: &str) -> String {
        [
            format!("{}:", name),
            format!("  owner: {}", owner),
            format!("  k8sName: {}", k8s),
            format!("  recovery: {}", recovery),
            format!("  description: {}", description),
        ]
        .join("\n")
    }

    /// THE priority arm: a bare-string `database:` value instead of a `$ref`. This is the literal
    /// enforcement of the #413 "invisible to the refs walker" class (ADR-20260811-014129 D2) the
    /// whole §18 placement design exists to protect — a plain-string placement resolves nowhere and
    /// is silently unseen by every ref-driven rule.
    #[test]
    fn a_bare_string_placement_is_not_a_ref() {
        let m = model(&[
            ("database/databases.yaml", &catalog()),
            (
                "database/tables/integration_staging.yaml",
                "external_stripe_events:\n  staging: true\n  database: adapter_stripe\n",
            ),
        ]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-placement-not-a-ref"), "{}", render(&issues));
    }

    /// §18a `database-name-not-underscored`: a hyphenated database name — a hyphen in a Postgres
    /// database name is a quoted identifier in every DSN and GRANT forever.
    #[test]
    fn a_hyphenated_database_name_is_not_underscored() {
        let m = model(&[(
            "database/databases.yaml",
            &one_database("adapter-stripe", "adapter_stripe", "adapter-stripe", "refetch", "stripe db"),
        )]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-name-not-underscored"), "{}", render(&issues));
    }

    /// §18a `database-decl-invalid`, sub-check `owner`: a non-snake_case owning role.
    #[test]
    fn a_bad_owner_is_decl_invalid() {
        let m = model(&[(
            "database/databases.yaml",
            &one_database("adapter_stripe", "Bad-Owner", "adapter-stripe", "refetch", "stripe db"),
        )]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-decl-invalid"), "{}", render(&issues));
        assert!(issues.iter().any(|i| i.message.contains("owner")), "{}", render(&issues));
    }

    /// §18a `database-decl-invalid`, sub-check `k8sName`: an underscore is not RFC 1123 (the CNPG
    /// Database CR object name).
    #[test]
    fn a_non_rfc1123_k8s_name_is_decl_invalid() {
        let m = model(&[(
            "database/databases.yaml",
            &one_database("adapter_stripe", "adapter_stripe", "adapter_stripe", "refetch", "stripe db"),
        )]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-decl-invalid"), "{}", render(&issues));
        assert!(issues.iter().any(|i| i.message.contains("k8sName")), "{}", render(&issues));
    }

    /// §18a `database-decl-invalid`, sub-check `k8sName` COLLISION: two databases binding the same
    /// (individually valid) K8s object name — one object name, one database.
    #[test]
    fn a_k8s_name_collision_is_decl_invalid() {
        let m = model(&[(
            "database/databases.yaml",
            &[
                "adapter_stripe:",
                "  owner: adapter_stripe",
                "  k8sName: shared-name",
                "  recovery: refetch",
                "  description: stripe db",
                "adapter_avelo37:",
                "  owner: adapter_avelo37",
                "  k8sName: shared-name",
                "  recovery: refetch",
                "  description: avelo db",
            ]
            .join("\n"),
        )]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-decl-invalid"), "{}", render(&issues));
        assert!(issues.iter().any(|i| i.message.contains("already bound")), "{}", render(&issues));
    }

    /// §18a `database-decl-invalid`, sub-check `recovery`: a posture outside the closed set (the
    /// drill slice #509 consumes it).
    #[test]
    fn an_out_of_set_recovery_is_decl_invalid() {
        let m = model(&[(
            "database/databases.yaml",
            &one_database("adapter_stripe", "adapter_stripe", "adapter-stripe", "bogus", "stripe db"),
        )]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-decl-invalid"), "{}", render(&issues));
        assert!(issues.iter().any(|i| i.message.contains("recovery")), "{}", render(&issues));
    }

    /// §18a `database-decl-invalid`, sub-check `description`: a declaration with no rationale is a
    /// name squat.
    #[test]
    fn an_empty_description_is_decl_invalid() {
        let m = model(&[(
            "database/databases.yaml",
            &one_database("adapter_stripe", "adapter_stripe", "adapter-stripe", "refetch", "''"),
        )]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-decl-invalid"), "{}", render(&issues));
        assert!(issues.iter().any(|i| i.message.contains("description")), "{}", render(&issues));
    }

    /// §18a `database-write-unit-undeclared`: a covered-kind table exists (so `any_covered` is true)
    /// but the catalog does not declare `captain_write`, the unit every derived placement resolves
    /// into (STO-1(a)).
    #[test]
    fn a_catalog_missing_the_write_unit_is_flagged() {
        let no_write = one_database("read_order", "migrator", "read-order", "replay", "the order read database");
        let m = model(&[
            ("database/databases.yaml", &no_write),
            ("database/tables/eventstore.yaml", "domain_events:\n  columns:\n    id: { type: uuid }\n"),
        ]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-write-unit-undeclared"), "{}", render(&issues));
    }

    /// The bad-`replicated`-value branch of `database-placement-invalid`: a replicable kind whose
    /// `replicated:` value is outside the accepted set — the only replicated class is the bare token
    /// `read-databases`.
    #[test]
    fn a_bad_replicated_value_is_placement_invalid() {
        let m = model(&[
            ("database/databases.yaml", &catalog()),
            (
                "database/tables/projection_tables.yaml",
                "ScopeMembership:\n  replicated: sometimes\n  columns:\n    id: { type: uuid }\n",
            ),
        ]);
        let issues = db_issues(&m);
        assert!(has(&issues, "database-placement-invalid"), "{}", render(&issues));
    }
}
