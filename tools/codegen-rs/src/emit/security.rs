//! `specs/generated/security.{,permissive.}generated.sql` — the database-level security emission
//! (#638 chunk 1, PROP-20260818-010343).
//!
//! **Nothing here enters `migrations/`.** The artifacts are applied by
//! `crates/infrastructure/tests/rls_matrix.rs` to throwaway databases it creates and drops, and by
//! nothing else. The fence is mechanical:
//! `tests::security_ddl_fence::migrations_carry_no_security_ddl_unless_they_are_the_generated_artifact`.
//! It is written as the conditional that converts at the local-acceptance walk
//! ([#556](https://github.com/TheCaptainCompany/captain-food/issues/556)) with no edit.
//!
//! # Scope, and why it is one table
//!
//! `OrderConversation` is the only guarded table in chunk 1, and the reason is mechanical rather
//! than about sensitivity: **it has no member-bearing column.** No `customer_id`, no `rider_id`. So
//! the predicate cannot be short-circuited into `USING (customer_id = current_setting(...))` and is
//! forced through the `ScopeMembership` subquery — which is the only path that makes the executor
//! meet the zero-row failures of PROP-20260818-010343 §3 that govern every later table.
//!
//! `ScopeMembership` is emitted alongside it as a **policy-bearing table, not merely a predicate
//! source** (§3 family member 2): a policy expression evaluates with the CALLER's privileges, so a
//! persona that cannot read its own membership rows reads zero of everything, correctly and
//! silently.
//!
//! # `mode:` is an EMITTER PARAMETER here, not a DSL key
//!
//! Two files come off one emitter, and that is load-bearing beyond coverage: the tightening step's
//! rollback is *a forward, generated migration that is the permissive emission of the same table*
//! (#637 has no other rollback — ADR-0043's "redeploy the previous app" is a no-op against a
//! subtractive change). Because both artifacts come off one emitter, that revert is **regenerated
//! deterministically** rather than hand-authored under incident pressure. The DSL key lands with
//! the first table that actually ships enforcing.
//!
//! **`mode:` gates the WHOLE per-table subtractive surface**, which is the decision this chunk was
//! asked to take in the diff rather than defer:
//!
//! | statement | `permissive` | `enforcing` |
//! |---|---|---|
//! | persona read predicate | `USING (true)` | the `ScopeMembership` `EXISTS` |
//! | persona `SELECT` grant | table-level (every column) | per-column `GRANT SELECT (…)` |
//! | the OWNER's `FOR ALL` policy | present | **absent** |
//! | `projector_{scope}`'s `FOR ALL` policy | present | present |
//! | `ENABLE` + `FORCE ROW LEVEL SECURITY` | present | present |
//!
//! The third row is the one two lenses disagreed about, and both are right about their half:
//!
//! - `farley`: `FORCE` + only `FOR SELECT` policies is **subtractive on WRITES, including for the
//!   owner** — and today's writer identity IS the owner, not a `projector_{scope}` role that does
//!   not yet exist. If the emitter only ever emitted `FOR ALL TO projector_order`, `mode: permissive`
//!   would already break writes: *"the single most likely way this chunk ships a green test over a
//!   broken configuration."* So permissive emits the owner policy, and permissive is genuinely
//!   additive — nothing that reads or writes today loses anything.
//! - PROP-20260818-010343 §6, consequence 2: **backfills run as `projector_{scope}`, never as the
//!   owner.** The owner's `UPDATE 0` is a *correct refusal*, not a hole. So enforcing withholds the
//!   owner policy, and the refusal becomes a tested behaviour rather than a note.
//!
//! Gating that clause on the mode is what makes both true at once, and it is also what keeps `FORCE`
//! observable: with an owner policy in force, dropping `FORCE` changes nothing for anybody (the
//! owner is permitted either way, and a non-owner persona is already enforced by `ENABLE` alone).
//! Enforcing mode is where dropping `FORCE` flips the owner from 0 rows to every row.
//!
//! # What is deliberately NOT here
//!
//! - **No database-level statement** (`REVOKE CONNECT, TEMPORARY … FROM PUBLIC`). It is subtractive
//!   at database level and ungated by `mode:`, and it would survive `reset_schema` into every later
//!   suite sharing CI's database — poisoning it, and producing a silent write failure somewhere else
//!   attributed to something else. Cutover-only, with its own gating (gate G-7).
//! - **No ADMIN policy.** `ScopeMembership.rules` declares *"ADMIN holds NO rows — the guard
//!   short-circuits on the role."* A membership-based admin policy therefore matches nothing, so the
//!   only emittable one is `USING (true)` — and shipping that as the first generated artifact would
//!   teach "the most privileged persona gets a permissive policy" as normal output. ADMIN stays
//!   application-enforced: the role is created so its emptiness is assertable, and it holds no grant
//!   and appears in no policy.
//! - **No RIDER policy on the guarded table.** `ScopeMembership` is ORDER-scoped and
//!   `DeliveryAcceptedByRider` grants the rider an ORDER membership, so a generated rider policy
//!   would correctly, per the index, hand the rider the customer↔restaurant thread. That is a
//!   PRODUCT decision hiding inside a SQL emission, and it must not be settled by generating. The
//!   rider holds `SELECT` and reads zero — the default-deny arm.
//! - **No persona views** (artifact A-6) and no `identity_binding` (A-7, [#641]) — sequenced
//!   separately.

use crate::*;

/// Which of the two artifacts is being emitted.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SecurityMode {
    /// Additive by construction: every predicate is `USING (true)`, grants are table-level, and the
    /// owner keeps a `FOR ALL` policy. This is what ships first (PROP-20260818-010343 §11.2) and
    /// what the tightening step reverts TO.
    Permissive,
    /// The measured design: role-bound member type as a literal, per-column grants, no owner policy.
    Enforcing,
}

impl SecurityMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            SecurityMode::Permissive => "permissive",
            SecurityMode::Enforcing => "enforcing",
        }
    }
}

/// The guarded tables of chunk 1 — declared, with the reason, because "which table" is a decision
/// and not yet a DSL key. Every other property below is DERIVED from the specs.
const GUARDED_TABLES: [&str; 1] = ["OrderConversation"];

/// The authorization index every predicate resolves against. Also policy-bearing (§3 family 2).
const MEMBERSHIP_TABLE: &str = "ScopeMembership";

/// Columns withheld from every persona but the one named, per guarded table. The DSL declares the
/// column's meaning in prose (*"INTERNAL ConversationMessage[] staff notes"*), which is not
/// derivable, so the narrowing is declared here — and [`emit_security_sql`] PANICS if a named
/// column is not in the table's declared columns, so a rename fails at `make generate` instead of
/// silently widening the grant to everybody.
const WITHHELD: [(&str, &str, &str); 1] =
    [("OrderConversation", "internal_notes", "restaurant_role")];

/// The persona roles, paired with the `UserType` member the policy literal names. Order is the
/// emission order. `ADMIN` is present and deliberately policy-less (see the module docs).
const PERSONAS: [(&str, &str); 4] = [
    ("admin_role", "ADMIN"),
    ("customer_role", "CUSTOMER"),
    ("restaurant_role", "RESTAURANT"),
    ("rider_role", "RIDER"),
];

/// Personas that hold a `SELECT` grant on the guarded table. ADMIN holds none — it is
/// application-enforced, and a grant with no policy would read as an oversight rather than a
/// decision.
const GRANTED_PERSONAS: [&str; 3] = ["customer_role", "restaurant_role", "rider_role"];

/// Personas that get a READ POLICY on the guarded table. The rider's absence is the default-deny
/// arm and the admin's absence is the product decision; both are explained in the module docs.
const POLICIED_PERSONAS: [&str; 2] = ["customer_role", "restaurant_role"];

/// The scope kind the guarded table's rows hang off. Asserted against `ScopeType` at emit time.
const SCOPE_TYPE: &str = "ORDER";

/// The session GUC carrying the member id. `app.member_type` is deliberately absent and must stay
/// absent (gate G-1): it is a setting the CALLER sets, and a rider setting it to `CUSTOMER` read two
/// customer orders on `dba`'s PG 16.13 cluster. The member TYPE is a literal in the policy text,
/// bound to the database role.
const MEMBER_ID_GUC: &str = "app.member_id";

/// `nullif(current_setting(…, true), '')::uuid` and not `current_setting(…)::uuid`, for two distinct
/// reasons that both end in a wrong-looking result:
///
/// - a never-set custom GUC makes the one-argument form **raise**, and "sees zero with no actor
///   context" must be zero rows, not an error the caller may catch and mistake for a schema problem;
/// - a GUC set to the empty string makes `''::uuid` raise, which is the shape a connection helper
///   produces when it binds a missing value.
fn member_id_expr() -> String {
    format!("nullif(current_setting('{MEMBER_ID_GUC}', true), '')::uuid")
}

// ─── derivations from the specs ─────────────────────────────────────────────────────────────────

fn declared_table<'a>(views: &'a [SqlView], name: &str) -> &'a SqlView {
    views
        .iter()
        .find(|v| v.name == name && v.is_table)
        .unwrap_or_else(|| {
            panic!(
                "security emitter: `{name}` is not a materialized table in \
                 specs/database/tables/projection_tables.yaml. If it was renamed or moved, update \
                 the security emitter in the SAME change -- an emitter that silently drops a \
                 guarded table emits a green artifact that guards nothing."
            )
        })
}

/// Every column of a declared read-model table, in declared order (the implicit `created_at` /
/// `updated_at` included — they are columns a persona reads like any other).
fn columns_of(views: &[SqlView], name: &str) -> Vec<String> {
    declared_table(views, name).columns.iter().map(|c| c.name.clone()).collect()
}

/// The database a guarded table is placed in — `database: { $ref: 'database/databases.yaml#/X' }`.
fn database_of(model: &Model, table: &str) -> String {
    let node = model
        .defs
        .get("database/tables/projection_tables.yaml")
        .and_then(|m| m.get(table))
        .unwrap_or_else(|| panic!("security emitter: {table} is not declared"));
    let r = node
        .get("database")
        .and_then(|d| d.get("$ref"))
        .and_then(|x| x.as_str())
        .unwrap_or_else(|| {
            panic!(
                "security emitter: {table} declares no `database:` $ref, so the owner and the \
                 projector role cannot be derived -- placement is a DSL decision, never a default"
            )
        });
    r.rsplit('/').next().unwrap_or_default().to_string()
}

/// The role that OWNS the business schema of a database — `databases.yaml#/<db>/owner`. Today's
/// writer identity, and the reason the permissive artifact carries an owner policy at all.
fn owner_of(model: &Model, db: &str) -> String {
    model
        .defs
        .get("database/databases.yaml")
        .and_then(|m| m.get(db))
        .and_then(|d| d.get("owner"))
        .and_then(|x| x.as_str())
        .unwrap_or_else(|| {
            panic!("security emitter: database `{db}` declares no `owner:` in databases.yaml")
        })
        .to_string()
}

/// `read_order` → `projector_order`. The writer role of PROP-20260818-010343 §6.
fn projector_role(db: &str) -> String {
    let scope = db.strip_prefix("read_").unwrap_or_else(|| {
        panic!(
            "security emitter: `{db}` is not a read database (`read_*`), so `projector_{{scope}}` \
             has no derivation. A guarded table placed outside a `recovery: replay` database is a \
             placement question, not an emitter default."
        )
    });
    format!("projector_{scope}")
}

/// A declared enum value of a scalar, or a loud panic. Keeps every literal the policies embed
/// traceable to the DSL rather than to this file.
fn assert_enum_member(model: &Model, scalar: &str, value: &str) {
    let ok = model
        .defs
        .get("scalars.yaml")
        .and_then(|m| m.get(scalar))
        .and_then(|s| s.get("enum"))
        .and_then(|e| e.as_sequence())
        .map(|seq| seq.iter().any(|v| v.as_str() == Some(value)))
        .unwrap_or(false);
    assert!(
        ok,
        "security emitter: '{value}' is not a declared member of scalars.yaml#/{scalar}. Every \
         literal a policy embeds must be traceable to the DSL -- a typo mints a member type no \
         policy will ever match, which is a zero-row family member with no diagnosis."
    );
}

// ─── the emission ───────────────────────────────────────────────────────────────────────────────

/// Emit one of the two artifacts. Re-runnable by construction: `DO $$` guards on every `CREATE
/// ROLE`, `DROP POLICY IF EXISTS` before every `CREATE POLICY`, no psql metacommand, and valid as a
/// single multi-statement simple query (`reset_schema()` replays it on every run).
pub(crate) fn emit_security_sql(model: &Model, mode: SecurityMode) -> String {
    let views = parse_views(model);
    assert_enum_member(model, "ScopeType", SCOPE_TYPE);
    for (_, member_type) in PERSONAS {
        assert_enum_member(model, "UserType", member_type);
    }

    let membership = MEMBERSHIP_TABLE.to_lowercase();
    let membership_cols = columns_of(&views, MEMBERSHIP_TABLE);
    // Exactly the four columns the predicate reads. A persona granted more of the authorization
    // index than the predicate needs is a widening nobody asked for; granted fewer, every read in
    // the system returns zero (§3 family 2).
    let predicate_cols = ["scope_type", "scope_id", "member_type", "member_id"];
    for c in predicate_cols {
        assert!(
            membership_cols.iter().any(|m| m == c),
            "security emitter: {MEMBERSHIP_TABLE} has no `{c}` column -- the policy predicate would \
             not compile. It was renamed (#435 renamed principal_* -> member_*); follow it here."
        );
    }

    // A withheld column that no longer exists is a grant SILENTLY WIDENED to every persona — the
    // column arm would still pass (nobody is denied anything that is not there) while the masking
    // this artifact exists for has evaporated. Fail at `make generate`, not in a later review.
    for (table, column, only) in WITHHELD {
        let cols = columns_of(&views, table);
        assert!(
            cols.iter().any(|c| c == column),
            "security emitter: `{table}.{column}` is withheld from every persona but `{only}`, but \
             it is not a declared column of {table} any more (declared: {cols:?}). It was renamed \
             or removed -- follow it here in the SAME change, or the narrowing silently becomes a \
             grant to everybody."
        );
        assert!(
            GRANTED_PERSONAS.contains(&only),
            "security emitter: `{table}.{column}` is reserved to `{only}`, which holds no SELECT \
             grant on the table -- the reservation would grant the column to nobody"
        );
    }

    let db = database_of(model, GUARDED_TABLES[0]);
    let owner = owner_of(model, &db);
    let projector = projector_role(&db);

    let mut roles: Vec<String> =
        PERSONAS.iter().map(|(r, _)| (*r).to_string()).chain([owner.clone(), projector.clone()]).collect();
    roles.sort();
    roles.dedup();

    let mut out = String::new();
    header(&mut out, mode, &db, &owner, &projector);

    // ── 1. Roles ────────────────────────────────────────────────────────────────────────────────
    out.push_str(&section(
        "1. ROLES",
        "NOLOGIN, exactly like migrations/20260808070000_claude_ro_select_only.sql: role LIFECYCLE\n\
         -- (login, password) belongs to the platform (CNPG `managed.roles`), and this repository is\n\
         -- public. NOBYPASSRLS on every one of them is the invariant gate G-6 asserts -- a role\n\
         -- attribute is not scoped to a table, so BYPASSRLS bypasses on tables it was never meant\n\
         -- to. NOINHERIT so a granted persona is reached only by an explicit SET ROLE.\n\
         -- Idempotent, and it degrades the way the house pattern degrades: a migration user without\n\
         -- CREATEROLE gets a NOTICE, not a failure.",
    ));
    out.push_str(&format!(
        "DO $$\nDECLARE r text;\nBEGIN\n  \
         FOREACH r IN ARRAY ARRAY[{}] LOOP\n    \
         IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = r) THEN\n      \
         BEGIN\n        \
         EXECUTE format('CREATE ROLE %I NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOINHERIT', r);\n      \
         EXCEPTION WHEN insufficient_privilege THEN\n        \
         RAISE NOTICE 'role % not created (this user lacks CREATEROLE) -- create it out of band and re-run', r;\n      \
         END;\n    \
         END IF;\n  \
         END LOOP;\nEND $$;\n\n",
        roles.iter().map(|r| format!("'{r}'")).collect::<Vec<_>>().join(", ")
    ));

    // ── 2. Ownership ────────────────────────────────────────────────────────────────────────────
    out.push_str(&section(
        "2. OWNERSHIP",
        "The business schema is owned by the declared owner of its database\n\
         -- (databases.yaml#/<database>/owner), NOT by whichever superuser ran the chain.\n\
         -- This is not tidiness. `FORCE ROW LEVEL SECURITY` removes only the OWNER's exemption and\n\
         -- never the superuser bypass, so a superuser-owned table makes FORCE observationally inert\n\
         -- and a suite asserting it passes over a configuration that proves nothing.",
    ));
    for t in tables(&membership) {
        out.push_str(&format!("ALTER TABLE {t} OWNER TO {owner};\n"));
    }
    out.push('\n');

    // ── 3. Grants ───────────────────────────────────────────────────────────────────────────────
    out.push_str(&section(
        "3. GRANTS",
        match mode {
            SecurityMode::Permissive =>
                "PERMISSIVE: table-level SELECT, every column. Column privileges are subtractive too\n\
                 -- and have NO mode of their own -- a permissive USING(true) policy combined with an\n\
                 -- enforcing per-column grant still breaks every read naming an ungranted column, and\n\
                 -- breaks every `SELECT *` outright. So the column list belongs to enforcing only.",
            SecurityMode::Enforcing =>
                "ENFORCING: per-column SELECT. Native column grants rather than per-role masking views\n\
                 -- (PROP-20260818-010343 §11.4): same outcome, roughly half the surface, no\n\
                 -- view-owner-versus-FORCE interaction, and visible in information_schema.column_privileges.",
        },
    ));
    out.push_str(&format!("GRANT USAGE ON SCHEMA public TO {};\n", roles.join(", ")));
    // The authorization index: readable by every persona that has a policy path through it. A
    // policy expression evaluates with the CALLER's privileges (§3 family 2).
    for role in GRANTED_PERSONAS {
        let cols = match mode {
            SecurityMode::Permissive => String::new(),
            SecurityMode::Enforcing => format!(" ({})", predicate_cols.join(", ")),
        };
        out.push_str(&format!("GRANT SELECT{cols} ON {membership} TO {role};\n"));
    }
    for table in GUARDED_TABLES {
        let t = table.to_lowercase();
        let all_cols = columns_of(&views, table);
        for role in GRANTED_PERSONAS {
            let cols = match mode {
                SecurityMode::Permissive => String::new(),
                SecurityMode::Enforcing => {
                    let visible: Vec<&String> = all_cols
                        .iter()
                        .filter(|c| {
                            !WITHHELD.iter().any(|(wt, wc, only)| {
                                *wt == table && *wc == c.as_str() && *only != role
                            })
                        })
                        .collect();
                    format!(
                        " ({})",
                        visible.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", ")
                    )
                }
            };
            out.push_str(&format!("GRANT SELECT{cols} ON {t} TO {role};\n"));
        }
    }
    // The writer. Table-level and every verb: column narrowing is a READER concern, and a projector
    // that cannot write a column it folds is the frozen-read-model failure of §6.
    for t in tables(&membership) {
        out.push_str(&format!("GRANT SELECT, INSERT, UPDATE, DELETE ON {t} TO {projector};\n"));
    }
    out.push('\n');

    // ── 4. Write policies ───────────────────────────────────────────────────────────────────────
    out.push_str(&section(
        "4. WRITE POLICIES",
        "Under ENABLE + FORCE with only FOR SELECT policies, a writer's INSERT errors loudly while\n\
         -- its UPDATE and DELETE return 0 rows, COMMIT, and report success -- and a projector's fold\n\
         -- is mostly updates. It advances its checkpoint over a stream whose effects never land: the\n\
         -- dashboards stay green, checkpoint lag reads zero, and the read model is frozen. At Friday\n\
         -- peak that is order statuses that stop moving while everything reports healthy.\n\
         -- FOR ALL ... USING(true) WITH CHECK(true) rather than ALTER ROLE ... BYPASSRLS: visible in\n\
         -- pg_policies, therefore assertable, and it keeps the NOBYPASSRLS invariant whole.\n\
         -- WITH CHECK (true) deliberately, NOT the reader predicate: inheriting it would make a\n\
         -- rebuild depend on ScopeMembership already being caught up in the same database, and a read\n\
         -- model whose rebuild depends on another read model's checkpoint is no longer a disposable\n\
         -- projection.",
    ));
    for t in tables(&membership) {
        out.push_str(&policy(
            &format!("{t}_write_{projector}"),
            &t,
            "ALL",
            &projector,
            "true",
            Some("true"),
        ));
    }
    if mode == SecurityMode::Permissive {
        out.push_str(&format!(
            "-- The OWNER keeps a FOR ALL policy in PERMISSIVE mode ONLY. Today's writer identity is\n\
             -- the owner, not a `{projector}` role that does not yet exist, so without this clause\n\
             -- `permissive` would already break every write -- the single most likely way this\n\
             -- artifact ships green over a broken configuration. Enforcing withholds it on purpose\n\
             -- (backfills run as `{projector}`, never as the owner), which is what makes the owner's\n\
             -- `UPDATE 0` a correct refusal rather than a hole -- and what keeps FORCE observable.\n"
        ));
        for t in tables(&membership) {
            out.push_str(&policy(&format!("{t}_write_{owner}"), &t, "ALL", &owner, "true", Some("true")));
        }
    } else {
        out.push_str(&format!(
            "-- NO owner policy in ENFORCING mode: `{owner}` is subject to FORCE and holds no write\n\
             -- policy, so a backfill run as the owner returns `UPDATE 0` -- a correct refusal. A\n\
             -- migration that needs to write an RLS-bearing table must SET ROLE {projector} (G-6).\n\n"
        ));
    }

    // ── 5. Read policies ────────────────────────────────────────────────────────────────────────
    out.push_str(&section(
        "5. READ POLICIES",
        match mode {
            SecurityMode::Permissive =>
                "PERMISSIVE: USING (true). Row security is ENABLED, FORCE is on, and the whole\n\
                 -- SET ROLE / set_config path is exercised end to end under real traffic -- while no\n\
                 -- row is withheld, so rollback-by-redeploy (ADR-0043) still means something. This is\n\
                 -- the artifact the tightening step reverts TO (#637); it exists in bytes in every CI\n\
                 -- run rather than being hand-authored under incident pressure.",
            SecurityMode::Enforcing =>
                "ENFORCING: the member TYPE is a LITERAL bound to the database role -- one policy per\n\
                 -- (table x role), never one policy naming four roles. The moment a policy has to work\n\
                 -- out WHICH of its roles is calling, the member type comes back as caller-settable\n\
                 -- data: on PG 16.13 a rider connection that ran `set_config('app.member_type',\n\
                 -- 'CUSTOMER')` read two customer orders. Postgres gates SET ROLE; it does not gate a\n\
                 -- set_config. That difference is the whole design.",
        },
    ));
    let member_id = member_id_expr();
    for (role, member_type) in PERSONAS {
        if !GRANTED_PERSONAS.contains(&role) {
            continue;
        }
        let using = match mode {
            SecurityMode::Permissive => "true".to_string(),
            SecurityMode::Enforcing => format!(
                "member_type = '{member_type}' AND member_id = {member_id}"
            ),
        };
        out.push_str(&policy(
            &format!("{membership}_read_{role}"),
            &membership,
            "SELECT",
            role,
            &using,
            None,
        ));
    }
    for table in GUARDED_TABLES {
        let t = table.to_lowercase();
        let pk = columns_of(&views, table)
            .first()
            .cloned()
            .unwrap_or_else(|| panic!("security emitter: {table} declares no columns"));
        for (role, member_type) in PERSONAS {
            if !POLICIED_PERSONAS.contains(&role) {
                continue;
            }
            let using = match mode {
                SecurityMode::Permissive => "true".to_string(),
                SecurityMode::Enforcing => format!(
                    "EXISTS (SELECT 1 FROM {membership} m\n           \
                     WHERE m.scope_type  = '{SCOPE_TYPE}'\n             \
                     AND m.scope_id    = {t}.{pk}\n             \
                     AND m.member_type = '{member_type}'\n             \
                     AND m.member_id   = {member_id})"
                ),
            };
            out.push_str(&policy(&format!("{t}_read_{role}"), &t, "SELECT", role, &using, None));
        }
    }

    // ── 6. ENABLE + FORCE, last ─────────────────────────────────────────────────────────────────
    out.push_str(&section(
        "6. ENABLE + FORCE ROW LEVEL SECURITY",
        "LAST, so that at COMMIT no table is ever enabled without its policies.\n\
         -- FORCE is invisible in the table DDL -- an ALTER beside the CREATE, not inside it -- so a\n\
         -- newly generated projection table is born unFORCEd and wide open with nothing on the page\n\
         -- to notice. With NO FORCE the measured result was a full cross-order read through a view\n\
         -- whose text reads correctly. It is a generated invariant plus a CI assertion (G-3), never a\n\
         -- hand-written line: the assertion is behavioural (relrowsecurity AND relforcerowsecurity\n\
         -- over the derived table set), never by filename.",
    ));
    for t in tables(&membership) {
        out.push_str(&format!("ALTER TABLE {t} ENABLE ROW LEVEL SECURITY;\n"));
        out.push_str(&format!("ALTER TABLE {t} FORCE ROW LEVEL SECURITY;\n"));
    }
    out
}

/// The guarded tables plus the membership index, lower-cased, in apply order.
fn tables(membership: &str) -> Vec<String> {
    let mut v: Vec<String> = GUARDED_TABLES.iter().map(|t| t.to_lowercase()).collect();
    v.push(membership.to_string());
    v.sort();
    v
}

fn policy(
    name: &str,
    table: &str,
    command: &str,
    role: &str,
    using: &str,
    with_check: Option<&str>,
) -> String {
    let check = with_check.map(|c| format!("\n  WITH CHECK ({c})")).unwrap_or_default();
    format!(
        "DROP POLICY IF EXISTS {name} ON {table};\n\
         CREATE POLICY {name} ON {table}\n  \
         FOR {command} TO {role}\n  \
         USING ({using}){check};\n\n"
    )
}

fn section(title: &str, why: &str) -> String {
    format!(
        "-- ─── {title} {}\n-- {why}\n\n",
        "─".repeat(78usize.saturating_sub(title.len() + 8))
    )
}

fn header(out: &mut String, mode: SecurityMode, db: &str, owner: &str, projector: &str) {
    out.push_str(&format!(
        "-- GENERATED by tools/codegen-rs (emit/security.rs) from specs/database/** -- do not edit by hand.\n\
         --\n\
         -- MODE: {mode}\n\
         -- Database: {db}   Owner: {owner}   Writer: {projector}\n\
         --\n\
         -- Database-level security for the #638 chunk-1 table set (PROP-20260818-010343).\n\
         -- Applied by crates/infrastructure/tests/rls_matrix.rs to throwaway databases, and by\n\
         -- NOTHING else: no file under migrations/ may carry ROW LEVEL SECURITY, CREATE POLICY or\n\
         -- CREATE ROLE (tools/codegen-rs/src/tests.rs, security_ddl_fence). db-migrate.yml runs\n\
         -- `sqlx migrate run` against the real DATABASE_URL on `workflow_run: [deploy] completed`,\n\
         -- and production is suspended by a DECISION, not by physics.\n\
         --\n\
         -- Two artifacts come off this one emitter: `security.generated.sql` (enforcing) and\n\
         -- `security.permissive.generated.sql`. The permissive one is what ships first and what the\n\
         -- tightening step reverts TO -- #637: a policy is subtractive on reads, so ADR-0043's\n\
         -- `redeploy the previous app` is a no-op against it, and the only rollback is a forward,\n\
         -- GENERATED migration that is the permissive emission of the same table.\n\
         --\n\
         -- Re-runnable: DO $$ guards on CREATE ROLE, DROP POLICY IF EXISTS before every CREATE\n\
         -- POLICY, no psql metacommand, valid as ONE multi-statement simple query.\n\n",
        mode = mode.label().to_uppercase()
    ));
}
