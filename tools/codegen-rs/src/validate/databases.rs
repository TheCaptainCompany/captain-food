//! §18 — database placement (#494 slice 1: the declaration site).
//!
//! Decided by PROP-20260811-093000 (DECISIONS §32 rows STO-1(a)/STO-2(a)) and ADR-20260812-115930
//! "Each adapter owns its own, completely isolated database" (ADP-1, both legs closed). The catalog
//! is `specs/database/databases.yaml`: the eleven databases as declarations — name, owning role,
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
//! - `replicated: read-databases` is the replicated placement class (STO-2(a): `ScopeMembership`
//!   and the `ref_*` enum family are DECIDED replicated). The resolved set is every database
//!   declared `recovery: replay` — that equivalence is definitional, not a naming pun: a read
//!   database is precisely one whose whole content is a re-derivable fold of the log, which is
//!   what makes N replicated copies cost nothing conceptually.
//!
//! COMPLETENESS IS PER-KIND, WITH NO DEFAULT. Business read models and referential tables outside
//! the replicated class carry NO placement yet: that is register row STO-2's open remainder
//! (DECISIONS §32 — "a working recommendation, not a decision, and needs a yes"), and a `database:`
//! key on one of them is refused here so a spec edit cannot silently close a register row. THIS
//! RULE WIDENS TO ALL TABLE KINDS WHEN STO-2's ROW CLOSES — at that point the refusal arm flips to
//! a requirement arm and every table resolves to a placement.
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

/// Table kinds that MAY declare `replicated:` (the STO-2(a) shapes: the authorization index and the
/// `ref_*` enum family's kind). Anything else declaring it is refused.
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
        } else if DECLARED_KINDS.contains(&kind) {
            if let Some(db) = declared_database(node).filter(|db| declared.contains(db.as_str())) {
                out.push(TablePlacement { table, kind, mode: PlacementMode::Declared, databases: vec![db] });
            }
        } else if node.get("replicated").and_then(|v| v.as_str()) == Some(REPLICATED_TOKEN)
            && REPLICABLE_KINDS.contains(&kind)
        {
            out.push(TablePlacement {
                table,
                kind,
                mode: PlacementMode::Replicated,
                databases: replay.clone(),
            });
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
    for (table, (file, kind)) in validate::table_kinds(model) {
        let node = match model.defs.get(&file).and_then(|v| v.get(table.as_str())) {
            Some(n) => n,
            None => continue,
        };
        let loc = format!("{}/{}", file, table);
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
        } else if DECLARED_KINDS.contains(&kind) {
            if has_repl {
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
            let db = match node.get("database") {
                None => {
                    issues.push(err(
                        "database-placement-missing",
                        loc,
                        format!(
                            "table '{}' ({}) declares no `database:` placement. Every staging/connection table names \
                             its database as a `$ref` into {} — absence is an error, never '{}'-by-omission \
                             (ADP-1, ADR-20260812-115930).",
                            table,
                            kind.name(),
                            DATABASES_FILE,
                            WRITE_DATABASE
                        ),
                    ));
                    continue;
                }
                Some(v) => match declared_database(node) {
                    Some(db) => {
                        if !declared.contains(db.as_str()) {
                            issues.push(err(
                                "database-placement-unknown-database",
                                loc,
                                format!(
                                    "table '{}' is placed in '{}', which {} does not declare — a placement must name \
                                     one of the eleven declared databases.",
                                    table, db, DATABASES_FILE
                                ),
                            ));
                            continue;
                        }
                        db
                    }
                    None => {
                        issues.push(err(
                            "database-placement-not-a-ref",
                            loc,
                            format!(
                                "table '{}' has a `database:` that is not a single-segment `$ref` into {} (got {:?}). \
                                 A bare string is invisible to the refs walker (ADR-20260811-014129 D2, the #413 \
                                 class) — write `database: {{ $ref: '{}#/<name>' }}`.",
                                table, DATABASES_FILE, v, DATABASES_FILE
                            ),
                        ));
                        continue;
                    }
                },
            };
            // The ADP-1 membership wall, both directions.
            match adapter_token_of(&table, &adapter_tokens) {
                Some(tok) => {
                    let expected = format!("adapter_{}", tok);
                    if db != expected {
                        issues.push(err(
                            "database-adapter-mismatch",
                            loc,
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
                            loc,
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
        } else if REPLICABLE_KINDS.contains(&kind) {
            if has_db {
                issues.push(err(
                    "database-placement-not-declarable",
                    loc.clone(),
                    format!(
                        "'{}' is a {} — business-table placement is register row STO-2's OPEN remainder \
                         (DECISIONS §32: \"a working recommendation, not a decision, and needs a yes\"); a \
                         `database:` key here would silently close that row. This refusal flips to a requirement \
                         when STO-2 closes.",
                        table,
                        kind.name()
                    ),
                ));
            }
            if let Some(v) = node.get("replicated") {
                match v.as_str() {
                    Some(REPLICATED_TOKEN) => {
                        if replay_count == 0 {
                            issues.push(err(
                                "database-replicated-empty",
                                loc,
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
                        loc,
                        format!(
                            "table '{}' has `replicated: {:?}` — the only replicated class is the bare token \
                             '{}' (every `recovery: replay` database).",
                            table, v, REPLICATED_TOKEN
                        ),
                    )),
                }
            }
        } else if has_db || has_repl {
            issues.push(err(
                "database-placement-not-declarable",
                loc,
                format!("'{}' is a {} — no placement grammar is open for this kind yet (see STO-2).", table, kind.name()),
            ));
        }
    }
}
