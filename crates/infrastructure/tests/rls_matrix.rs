//! THE AUTHORIZATION MATRIX (#638 chunk 1, PROP-20260818-010343) — proved against a real
//! PostgreSQL, on the one table that cannot teach the wrong thing.
//!
//! # Why `orderconversation`, and why the reason is mechanical
//!
//! It has **no member-bearing column**. No `customer_id`, no `rider_id`. So the policy cannot be
//! short-circuited into `USING (customer_id = current_setting(...))` and is forced through the
//! `scopemembership` subquery — the only path that makes the executor meet the zero-row failures
//! that govern every later table. Pick a table with a `customer_id` and the first cut goes green
//! without ever touching `scopemembership`, and the lesson is never learned.
//!
//! # The rule that governs every assertion here: NO BARE ZERO
//!
//! Four of the five probes are *"sees zero"*, and a lone zero passes when the fixture inserted
//! nothing, when the membership rows are absent, when `set_config` landed outside a transaction,
//! when the artifact was never applied, and when row security is so broken nobody sees anything.
//! That is a thermometer reading zero because it is unplugged.
//!
//! **Every zero is therefore asserted JOINTLY with a non-zero the same connection does see, in the
//! same assertion**: `assert_eq!((own, foreign), (2, 0), …)`, never `assert_eq!(foreign, 0)`.
//! [`Probe`] exists so that shape is the only one available.
//!
//! # The two false results this suite would otherwise have produced
//!
//! CI connects as `postgres`, a **superuser**, and `sqlx migrate run` therefore creates every table
//! superuser-owned. Two consequences, both of which would have shipped green:
//!
//! - **`FORCE` is observationally inert against a superuser owner**, because it removes only the
//!   *owner's* exemption and never the superuser bypass — and for a plain non-owner persona,
//!   `ENABLE` alone already enforces. Planting "drop `FORCE`" would have changed nothing, gone
//!   green, and taught us `FORCE` is unnecessary — the exact inverse of the measured finding that
//!   `NO FORCE` lets the owner read every row.
//! - **A superuser may `SET ROLE` to anything**, so a `permission denied to set role` assertion
//!   would have gone red against the *correct* design.
//!
//! The fixes are structural rather than careful: the generated artifact itself reassigns table
//! ownership to the declared non-superuser owner (`databases.yaml#/read_order/owner`), the suite
//! opens **one pool per persona** as a real `LOGIN` role, and every scoped transaction asserts
//! `current_setting('is_superuser') = 'off'` before it probes anything.
//!
//! # Two databases, one per mode — not a time-ordered mutation
//!
//! `DROP POLICY` and `ALTER TABLE … FORCE` take `ACCESS EXCLUSIVE`, so flipping mode mid-run would
//! contend with every open persona connection. Instead the suite builds **its own databases**, one
//! per mode, in the same setup. *"The two modes differ"* becomes a comparison of two result sets,
//! and the permissive case stays permanently covered — which matters more than coverage: the
//! tightening step's only rollback is *a forward, generated migration that is the permissive
//! emission of the same table* ([#637]; ADR-0043's "redeploy the previous app" is a no-op against a
//! subtractive change), and the load-bearing assertion is the FIRST one, `permissive == every row`.
//! Everyone remembers to test enforcing.
//!
//! Own databases also close the leakage risk: roles and database-level grants survive
//! `reset_schema` into every later suite sharing CI's database, where they would produce a silent
//! failure somewhere else, attributed to something else. Cleanup happens in **setup**, never
//! teardown — teardown does not run on a panic, and roles are cluster-scoped while `reset_schema`
//! is schema-scoped.
//!
//! # Nothing here writes a policy
//!
//! The suite applies `specs/generated/security*.generated.sql` and asserts behaviour. **No SQL
//! literal describing a policy appears below except M1's deliberately rejected shape** — otherwise
//! the suite would prove only that hand-written naive SQL differs from hand-written correct SQL,
//! which is a tautology. Every mutation went into the EMITTER and the generator was re-run.
//!
//! # SEEN RED — six semantic mutations, each measured on PG 16.13
//!
//! | # | Mutation | Measured |
//! |---|---|---|
//! | **M1** | one policy naming four roles, member type from `current_setting('app.member_type')` | a rider connection reads the customer's **two** orders. Kept as the PERMANENT test [`the_rejected_guc_policy_hands_a_rider_the_customers_thread`] |
//! | **M2** | drop `FORCE`, keep `ENABLE`, aimed at the **owner** | owner reads **4** rows and `UPDATE 4` where the artifact gives 0 and `UPDATE 0`; a persona is unaffected (2 either way), which is exactly why the mutation had to be aimed at the owner |
//! | **M3a** | table-level `GRANT SELECT` replaces the column grants | `customer_role` reads `internal_notes` (2 rows) and `SELECT *` succeeds, where both must be SQLSTATE `42501` |
//! | **M4** | one login role granted **both** persona roles | reads the **union** {O1,O3,O4} where each persona alone reads 2 — and the defence is the PER-GRANT option, not the role attribute. Kept as the PERMANENT test [`an_inherited_persona_grant_hands_one_login_role_the_union`] |
//! | **M5** | delete the `scopemembership` self-row policies | every persona reads **0** — the zero-row member most likely to bite at cutover |
//! | **M6** | `set_config(…, true)` outside an explicit transaction | 2 in-transaction, **0** out of it |
//! | **M7** | swap two member ids in the fixture | C1 sees {O2} instead of {O1,O3} — the fixture's own mutation test, and the direct answer to *"is this grading its own homework"* |
//!
//! **M3b is not applicable to this chunk** and that is a finding rather than a gap: chunk 1 emits no
//! persona views (artifact A-6 is sequenced separately), so there is no join-first access path to
//! degrade. The plan-shape assertion is deliberately absent for a second reason — at four fixture
//! rows PostgreSQL seq-scans everything, so a no-`Seq Scan` assertion here is a false red. The
//! 0.263 ms / 180.569 ms figures behind it are **`UNVERIFIED input`** in this suite; the plan-shape
//! gate needs its own volume test.
//!
//! Needs `DATABASE_URL`: since #474 a missing database FAILS this suite; only an explicit
//! `DB_TESTS_REQUIRED=0` skips it, and that leaves a receipt (`crates/db_test_gate`).

// The TestDb witness and the REAL migration chain, shared with the `main` binary by path rather
// than duplicated (#335, ADR-20260808-224500 item 5).
#[path = "main/common.rs"]
mod common;

use std::collections::BTreeSet;
use std::str::FromStr;

use sqlx::postgres::PgConnectOptions;
use sqlx::{Executor, PgConnection, PgPool};
use uuid::Uuid;

/// The two artifacts, read at COMPILE time so the suite cannot run against a stale generation and
/// the codegen drift gate covers the bytes this suite proves.
const ENFORCING_SQL: &str = include_str!("../../../specs/generated/security.generated.sql");
const PERMISSIVE_SQL: &str = include_str!("../../../specs/generated/security.permissive.generated.sql");

/// One password for every test role. The artifact creates them `NOLOGIN` on purpose (role
/// lifecycle belongs to the platform, and this repository is public); the suite plays the platform
/// for its own throwaway cluster state.
const PW: &str = "rls_matrix_test";

/// The throwaway databases, dropped and recreated in SETUP. Teardown does not run on a panic.
const DB_ENFORCING: &str = "cf_rls_enforcing";
const DB_PERMISSIVE: &str = "cf_rls_permissive";
const DB_REJECTED: &str = "cf_rls_m1_rejected";

/// Every role this suite touches, including the ones its permanent mutation tests mint. Dropped in
/// setup so a previous run's LOGIN attribute or leftover grant cannot decide this run's result.
const ALL_ROLES: [&str; 8] = [
    "admin_role",
    "customer_role",
    "migrator",
    "projector_order",
    "restaurant_role",
    "rider_role",
    // minted by the M4 permanent test
    "inherit_svc",
    "noinherit_svc",
];

// ─── the fixture ────────────────────────────────────────────────────────────────────────────────
//
// Four conversations across two restaurants and four customers, so the two personas' visible sets
// OVERLAP WITHOUT BEING EQUAL — customer C1 sees {O1,O3}, restaurant R1 sees {O1,O2}, meeting only
// at O1. A "returns the other persona's set" bug is therefore visible; with equal sets it is not.
//
//   O1: C1 @ R1     O2: C2 @ R1     O3: C1 @ R2     O4: C3 @ R2
//
// C4 is a member of nothing. RD1 is a rider holding a REAL ORDER membership on O1 (the grant
// `DeliveryAcceptedByRider` writes), which is what makes the rider's zero mean something: the rider
// HAS a membership on that order and still cannot read its thread.

const O1: u128 = 0x01;
const O2: u128 = 0x02;
const O3: u128 = 0x03;
const O4: u128 = 0x04;
const C1: u128 = 0xC1;
const C2: u128 = 0xC2;
const C3: u128 = 0xC3;
const C4: u128 = 0xC4;
const R1: u128 = 0x51;
const R2: u128 = 0x52;
const RD1: u128 = 0xD1;

fn uid(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

/// The five probes every policied persona runs. Named rather than counted, so the coverage gate
/// below fails loudly the day a probe is added to one persona and forgotten on the other.
const PROBES: [&str; 5] = [
    "sees its own rows",
    "sees zero for a foreign member",
    "sees zero with no actor context",
    "sees zero claiming another persona's member type",
    "cannot read a non-granted column",
];

/// One policied persona's arm of the matrix.
struct Arm {
    role: &'static str,
    /// The `UserType` literal its policy embeds. Asserted against the artifact by [`policy_set`].
    member_type: &'static str,
    /// The member the connection binds into `app.member_id`.
    member: u128,
    /// The orders it must see, and only those.
    visible: [u128; 2],
    /// A member of the SAME persona whose rows it must not see (probe 2's foreign member).
    foreign: u128,
    /// How many rows that FOREIGN member sees. Deliberately not the same number for both arms —
    /// C2 is a member of exactly one order and R2 of two — because a fixture where every member
    /// sees the same count is satisfied by a policy that returns a constant-sized set.
    foreign_sees: i64,
}

fn arms() -> Vec<Arm> {
    vec![
        Arm {
            role: "customer_role",
            member_type: "CUSTOMER",
            member: C1,
            visible: [O1, O3],
            foreign: C2,
            foreign_sees: 1,
        },
        Arm {
            role: "restaurant_role",
            member_type: "RESTAURANT",
            member: R1,
            visible: [O1, O2],
            foreign: R2,
            foreign_sees: 2,
        },
    ]
}

/// The guarded table. Chunk 1 has exactly one, and the coverage gate derives the number rather than
/// trusting this constant — it is here only to name what the arms above are arms OF.
const GUARDED: &str = "orderconversation";
const MEMBERSHIP: &str = "scopemembership";

// ─── offline gates (no database; they run even under DB_TESTS_REQUIRED=0) ───────────────────────

/// The artifact with its comment lines removed.
///
/// Not cosmetic: the header explains the fence in prose and therefore contains the literal
/// `CREATE POLICY`, so a naive `matches()` over the whole file counts one policy that does not
/// exist — and [`preflight`]'s "artifact policies == pg_policies" assertion fails against a
/// perfectly applied artifact. Cost: one confusing red run.
fn sql_only(sql: &str) -> String {
    sql.lines().filter(|l| !l.trim_start().starts_with("--")).collect::<Vec<_>>().join("\n")
}

/// Every `CREATE POLICY … ON <table> … FOR <cmd> TO <role>` in an artifact, as
/// `(table, role, command)`. Parsed rather than declared, so both sides of the coverage gate below
/// are derived.
fn policy_set(sql: &str) -> BTreeSet<(String, String, String)> {
    let mut out = BTreeSet::new();
    let lines: Vec<&str> = sql.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let Some(rest) = line.trim().strip_prefix("CREATE POLICY ") else { continue };
        let mut it = rest.split_whitespace();
        let _name = it.next().expect("CREATE POLICY <name>");
        assert_eq!(it.next(), Some("ON"), "unexpected CREATE POLICY shape: {line}");
        let table = it.next().expect("CREATE POLICY <name> ON <table>").to_string();
        let next = lines.get(i + 1).unwrap_or(&"").trim();
        let for_to = next
            .strip_prefix("FOR ")
            .unwrap_or_else(|| panic!("expected `FOR <cmd> TO <role>` after `{line}`, got `{next}`"));
        let (command, role) = for_to
            .split_once(" TO ")
            .unwrap_or_else(|| panic!("expected ` TO <role>` in `{next}`"));
        out.insert((table.to_string(), role.trim().to_string(), command.trim().to_string()));
    }
    assert!(!out.is_empty(), "no CREATE POLICY parsed out of the artifact -- the gate would be vacuous");
    out
}

/// **`rls-matrix-covers-every-generated-policy`** — the one gate this chunk adds, and it lives
/// inside the artifact being proven rather than in a workflow YAML where the next edit deletes it
/// and no compiler notices.
///
/// Both sides are DERIVED: the (table, role) READ-policy set comes out of the generated SQL, the
/// arm set out of [`arms`], and the probe count out of [`PROBES`]. A green run with zero tests fails
/// it, because zero is not the expected number — which is the answer to *"how do I know it did not
/// skip"* that `db_test_gate` alone cannot give (libtest prints `running 0 tests` and exits 0). It
/// also makes an uncovered table red the day the emitter grows to table two, rather than silent.
#[test]
fn rls_matrix_covers_every_generated_policy() {
    for (label, sql) in [("enforcing", ENFORCING_SQL), ("permissive", PERMISSIVE_SQL)] {
        let policies = policy_set(sql);
        let read_arms: BTreeSet<(String, String)> = policies
            .iter()
            .filter(|(t, _, cmd)| cmd == "SELECT" && t == GUARDED)
            .map(|(t, r, _)| (t.clone(), r.clone()))
            .collect();
        let covered: BTreeSet<(String, String)> =
            arms().iter().map(|a| (GUARDED.to_string(), a.role.to_string())).collect();
        assert_eq!(
            read_arms, covered,
            "the {label} artifact's READ policies on `{GUARDED}` and this suite's arms disagree.\n\
             Fix: add the missing arm to `arms()` (and its expected visible set), or remove the \
             policy from the emitter.\n\
             Why: an emitted policy with no arm is an authorization decision nobody measured -- and \
             the day the emitter grows to table two, this is what makes it red instead of silent."
        );
        assert_eq!(
            read_arms.len() * PROBES.len(),
            arms().len() * PROBES.len(),
            "matrix arms != tables x personas x probes for the {label} artifact"
        );
    }
}

/// The chunk's fences, asserted on the bytes rather than trusted from the dispatch card.
#[test]
fn the_generated_artifacts_carry_no_forbidden_statement() {
    for (label, sql) in [("enforcing", ENFORCING_SQL), ("permissive", PERMISSIVE_SQL)] {
        // G-1: the member type must never be caller-settable. On PG 16.13 a rider connection that
        // ran `set_config('app.member_type', 'CUSTOMER')` read two customer orders -- the whole role
        // tree bypassed by one string. The identifier must not exist in the emission at all.
        let policy_text = sql_only(sql);
        assert!(
            !policy_text.contains("app.member_type"),
            "the {label} artifact references `app.member_type` outside a comment. The member type \
             is a LITERAL bound to the database role; Postgres gates SET ROLE and does not gate a \
             set_config, and that difference is the entire design (G-1)."
        );

        // Database-level statements are subtractive at DATABASE level, ungated by `mode:`, and they
        // survive `reset_schema` into every later suite sharing CI's database -- poisoning it, and
        // producing a silent failure somewhere else attributed to something else. Cutover-only.
        for forbidden in ["FROM PUBLIC", "ON DATABASE", "REVOKE CONNECT", "REVOKE TEMPORARY"] {
            assert!(
                !policy_text.contains(forbidden),
                "the {label} artifact contains `{forbidden}` -- database-level statements are fenced \
                 out of this chunk's applied set (they leak into every later suite sharing CI's \
                 database) and are cutover-only artifacts with their own gating (G-7)."
            );
        }

        // Re-runnable, because `reset_schema()` replays it on every run: it is the cutover in
        // miniature. A CREATE POLICY without its DROP guard breaks the second application.
        let creates = policy_text.matches("CREATE POLICY ").count();
        let drops = policy_text.matches("DROP POLICY IF EXISTS ").count();
        assert_eq!(
            creates, drops,
            "the {label} artifact has {creates} CREATE POLICY and {drops} DROP POLICY IF EXISTS -- \
             every policy needs its guard or the artifact is not re-runnable"
        );
        assert!(
            !policy_text.lines().any(|l| l.trim_start().starts_with('\\')),
            "the {label} artifact contains a psql metacommand -- it must be valid as ONE \
             multi-statement simple query"
        );
    }

    // The two modes must genuinely DIFFER. If an emitter refactor ever collapsed them, every
    // "permissive is additive" assertion below would pass by measuring the enforcing artifact twice.
    assert_ne!(
        ENFORCING_SQL, PERMISSIVE_SQL,
        "the two artifacts are byte-identical -- `mode:` gates nothing"
    );
}

// ─── the harness ────────────────────────────────────────────────────────────────────────────────

/// A probe result: what the connection DID see, paired with what it must NOT.
///
/// The type is the enforcement of `beck`'s rule. There is no constructor that yields a single
/// number, so `assert_eq!(foreign, 0)` is not spellable: every zero arrives already paired with the
/// non-zero the same connection saw, in the same statement, on the same fixture.
#[derive(Debug, PartialEq, Eq)]
struct Probe {
    seen: i64,
    withheld: i64,
}

impl Probe {
    fn is(&self, seen: i64, withheld: i64, why: &str) {
        assert_eq!(
            (self.seen, self.withheld),
            (seen, withheld),
            "{why}\n(left = what this connection actually saw / what it must not see; a zero on the \
             right means nothing unless the left is non-zero on the SAME connection)"
        );
    }
}

/// Open a scoped transaction on a PINNED connection and read one `(seen, withheld)` pair.
///
/// `member = None` is the "no actor context" probe: no `set_config` at all. The transaction is
/// explicit because `set_config(…, true)` is transaction-LOCAL — outside one the statement is its
/// own transaction, the setting is discarded the instant it commits, and the predicate matches zero
/// rows. That is not a hypothetical mis-use; it is the default behaviour of every connection helper
/// that does not open a transaction first (probe M6).
async fn probe(conn: &mut PgConnection, member: Option<Uuid>, sql: &str) -> Probe {
    begin_scope(conn, member).await;
    let (seen, withheld): (i64, i64) =
        sqlx::query_as(sql).fetch_one(&mut *conn).await.unwrap_or_else(|e| panic!("{sql}: {e}"));
    conn.execute("COMMIT").await.expect("commit the scoped transaction");
    Probe { seen, withheld }
}

/// The SQLSTATE a scoped statement fails with, or a panic naming what it returned instead. Never
/// asserted on message text — that is a translation away from a false green.
async fn probe_sqlstate(conn: &mut PgConnection, member: Option<Uuid>, sql: &str) -> String {
    begin_scope(conn, member).await;
    let err = sqlx::query(sql)
        .fetch_all(&mut *conn)
        .await
        .err()
        .unwrap_or_else(|| panic!("`{sql}` SUCCEEDED where a denial was required"));
    let code = err
        .as_database_error()
        .and_then(|d| d.code())
        .unwrap_or_else(|| panic!("`{sql}` failed without a SQLSTATE: {err}"))
        .to_string();
    conn.execute("ROLLBACK").await.expect("roll back the aborted transaction");
    code
}

/// `BEGIN`, the superuser assertion, then the member binding — in that order, always.
///
/// The superuser check is one line that kills an entire class: CI connects as `postgres`, a
/// superuser bypasses RLS AND may `SET ROLE` to anything, so a matrix run on a superuser connection
/// measures nothing while reporting everything.
async fn begin_scope(conn: &mut PgConnection, member: Option<Uuid>) {
    conn.execute("BEGIN").await.expect("open the scoped transaction");
    let su: String = sqlx::query_scalar("SELECT current_setting('is_superuser')")
        .fetch_one(&mut *conn)
        .await
        .expect("read is_superuser");
    assert_eq!(
        su, "off",
        "this scoped transaction is running as a SUPERUSER, which bypasses row security entirely -- \
         every arm below would pass over a configuration that proves nothing. The persona pools must \
         connect as the persona LOGIN roles, not as the DATABASE_URL identity."
    );
    if let Some(m) = member {
        sqlx::query("SELECT set_config('app.member_id', $1, true)")
            .bind(m.to_string())
            .fetch_one(&mut *conn)
            .await
            .expect("bind app.member_id inside the transaction");
    }
}

/// `count(*) FILTER (…) , count(*) FILTER (NOT …)` over the guarded table — the only statement
/// shape the row probes use, so a bare count cannot be written by accident.
fn split_count(table: &str, predicate: &str) -> String {
    format!(
        "SELECT count(*) FILTER (WHERE {predicate})::bigint, \
                count(*) FILTER (WHERE NOT ({predicate}))::bigint FROM {table}"
    )
}

fn id_list(ids: &[u128]) -> String {
    ids.iter().map(|i| format!("'{}'::uuid", uid(*i))).collect::<Vec<_>>().join(", ")
}

// ─── database construction ──────────────────────────────────────────────────────────────────────

fn options(url: &str) -> PgConnectOptions {
    PgConnectOptions::from_str(url).expect("DATABASE_URL parses as a Postgres connection string")
}

async fn persona_pool(url: &str, db: &str, role: &str) -> PgPool {
    PgPool::connect_with(options(url).database(db).username(role).password(PW))
        .await
        .unwrap_or_else(|e| panic!("connect to {db} as {role}: {e}"))
}

/// Build one throwaway database: the REAL migration chain, then the artifact, then the fixture.
///
/// The fixture is seeded by the SUPERUSER on purpose — it is the projector's job in production, and
/// a superuser bypasses RLS, so seeding cannot be silently filtered by the very policies under test.
async fn build(url: &str, db: &str, artifact: &str) {
    let pool = PgPool::connect_with(options(url).database(db))
        .await
        .unwrap_or_else(|e| panic!("connect to the fresh {db}: {e}"));
    common::apply_migration_chain(&pool).await;
    sqlx::raw_sql(artifact)
        .execute(&pool)
        .await
        .unwrap_or_else(|e| panic!("apply the generated security artifact to {db}: {e}"));
    seed(&pool).await;
    pool.close().await;
}

async fn seed(pool: &PgPool) {
    for (order, restaurant) in [(O1, R1), (O2, R1), (O3, R2), (O4, R2)] {
        sqlx::query(
            "INSERT INTO orderconversation (order_id, restaurant_id, customer_chat_enabled, status, \
               messages, internal_notes, opened_at, admin_invited, muted, created_at, updated_at, \
               claim_events) \
             VALUES ($1, $2, true, 'PLACED', '[]'::jsonb, $3::jsonb, now(), false, '[]'::jsonb, \
                     now(), now(), '[]'::jsonb)",
        )
        .bind(uid(order))
        .bind(uid(restaurant))
        .bind(format!(r#"[{{"note":"internal {order:#x}"}}]"#))
        .execute(pool)
        .await
        .expect("seed a conversation");
    }

    // The eight REAL memberships the projector folds from OrderPlaced, plus the rider grant
    // DeliveryAcceptedByRider writes.
    let real: [(&str, u128, &str, u128); 9] = [
        ("ORDER", O1, "CUSTOMER", C1),
        ("ORDER", O2, "CUSTOMER", C2),
        ("ORDER", O3, "CUSTOMER", C1),
        ("ORDER", O4, "CUSTOMER", C3),
        ("ORDER", O1, "RESTAURANT", R1),
        ("ORDER", O2, "RESTAURANT", R1),
        ("ORDER", O3, "RESTAURANT", R2),
        ("ORDER", O4, "RESTAURANT", R2),
        ("ORDER", O1, "RIDER", RD1),
    ];
    // The three DECOYS, and they are the point. All three aim at O4, so "the decoy target" is one
    // order and every wrong-clause bug lands on the same visible row.
    let decoys: [(&str, u128, &str, u128); 3] = [
        // right ids, WRONG scope type -- catches a policy that drops the `scope_type` clause.
        ("RESTAURANT", O4, "CUSTOMER", C1),
        // C1's uuid under the RESTAURANT member type. Without it, "sees zero claiming another
        // persona's member type" passes because there is no row to find, and the arm certifies
        // nothing.
        ("ORDER", O4, "RESTAURANT", C1),
        // the symmetric one for restaurant_role: R1's uuid under the CUSTOMER member type.
        ("ORDER", O4, "CUSTOMER", R1),
    ];
    for (scope_type, scope_id, member_type, member_id) in real.iter().chain(decoys.iter()) {
        sqlx::query(
            "INSERT INTO scopemembership (membership_id, scope_type, scope_id, member_type, \
               member_id, granted_at, created_at, updated_at) \
             VALUES (gen_random_uuid(), $1, $2, $3, $4, now(), now(), now())",
        )
        .bind(scope_type)
        .bind(uid(*scope_id))
        .bind(member_type)
        .bind(uid(*member_id))
        .execute(pool)
        .await
        .expect("seed a membership");
    }
}

/// The tables the artifact enables row security on, parsed out of the artifact itself. Asserting a
/// derived set is what makes the preflight behavioural rather than a filename convention.
fn enabled_tables(sql: &str) -> BTreeSet<String> {
    sql.lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix("ALTER TABLE ")?
                .strip_suffix(" ENABLE ROW LEVEL SECURITY;")
                .map(|t| t.to_string())
        })
        .collect()
}

/// Without this, "0 rows" is indistinguishable from "the generated SQL was never applied".
async fn preflight(url: &str, db: &str, artifact: &str) {
    let pool = PgPool::connect_with(options(url).database(db))
        .await
        .unwrap_or_else(|e| panic!("preflight connect to {db}: {e}"));

    let tables = enabled_tables(artifact);
    assert!(!tables.is_empty(), "{db}: the artifact enables row security on no table at all");
    for t in &tables {
        let (enabled, forced, owner_is_super): (bool, bool, bool) = sqlx::query_as(
            "SELECT c.relrowsecurity, c.relforcerowsecurity, r.rolsuper \
               FROM pg_class c JOIN pg_roles r ON r.oid = c.relowner \
              WHERE c.relname = $1",
        )
        .bind(t)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| panic!("{db}: pg_class for {t}: {e}"));
        assert_eq!(
            (enabled, forced, owner_is_super),
            (true, true, false),
            "{db}.{t}: expected (relrowsecurity, relforcerowsecurity, owner-is-superuser) = \
             (true, true, false). FORCE removes only the OWNER's exemption and never the superuser \
             bypass, so a superuser-owned table makes every assertion below inert."
        );
    }

    let policies: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_policies WHERE schemaname = 'public'")
        .fetch_one(&pool)
        .await
        .expect("count pg_policies");
    let expected = sql_only(artifact).matches("CREATE POLICY ").count() as i64;
    assert_eq!(
        policies, expected, "{db}: {expected} policies in the artifact, {policies} in pg_policies"
    );

    // The fixture is real. A zero anywhere below must never be reachable by "nothing was inserted".
    let (orders, memberships): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM orderconversation), (SELECT count(*) FROM scopemembership)",
    )
    .fetch_one(&pool)
    .await
    .expect("count the fixture");
    assert_eq!(
        (orders, memberships),
        (4, 12),
        "{db}: the fixture is not what the arms below expect (4 conversations, 9 real memberships \
         + 3 decoys). A decoy that is not there makes its arm certify nothing."
    );
    pool.close().await;
}

// ─── the suite ──────────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn the_authorization_matrix_holds_in_both_modes() {
    let Some(db) = common::TestDb::acquire("rls_matrix").await else { return };
    let url = db.url();
    let admin = db.pool();

    // ── SETUP-side cleanup: teardown does not run on a panic, and roles are CLUSTER-scoped while
    //    reset_schema is schema-scoped. Databases first, then roles -- a role cannot be dropped
    //    while it holds a privilege in a database that still exists.
    for name in [DB_ENFORCING, DB_PERMISSIVE, DB_REJECTED] {
        admin
            .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
            .await
            .unwrap_or_else(|e| panic!("drop {name}: {e}"));
    }
    for role in ALL_ROLES {
        admin
            .execute(format!("DROP ROLE IF EXISTS {role}").as_str())
            .await
            .unwrap_or_else(|e| panic!("drop role {role} (it still holds something somewhere): {e}"));
    }
    for name in [DB_ENFORCING, DB_PERMISSIVE, DB_REJECTED] {
        admin
            .execute(format!("CREATE DATABASE {name}").as_str())
            .await
            .unwrap_or_else(|e| panic!("create {name}: {e}"));
    }

    build(&url, DB_ENFORCING, ENFORCING_SQL).await;
    build(&url, DB_PERMISSIVE, PERMISSIVE_SQL).await;
    build(&url, DB_REJECTED, ENFORCING_SQL).await;

    // The artifact creates every role NOLOGIN, because role lifecycle belongs to the platform and
    // this repository is public. The suite plays the platform for its throwaway cluster state.
    for role in ["admin_role", "customer_role", "migrator", "projector_order", "restaurant_role", "rider_role"]
    {
        admin
            .execute(format!("ALTER ROLE {role} LOGIN PASSWORD '{PW}'").as_str())
            .await
            .unwrap_or_else(|e| panic!("grant LOGIN to {role}: {e}"));
    }

    preflight(&url, DB_ENFORCING, ENFORCING_SQL).await;
    preflight(&url, DB_PERMISSIVE, PERMISSIVE_SQL).await;

    let mut probes_run = 0usize;

    // ── ENFORCING: the five probes, per policied persona ────────────────────────────────────────
    for arm in arms() {
        let pool = persona_pool(&url, DB_ENFORCING, arm.role).await;
        let mut conn = pool.acquire().await.expect("pin one connection for the whole arm");
        let mine = id_list(&arm.visible);
        let own_vs_rest = split_count(GUARDED, &format!("order_id IN ({mine})"));

        // 1 + 2. Its own rows, and ZERO of anybody else's -- one statement, one connection.
        // "Foreign member" is measured twice over: the rows of the other members of this persona are
        // in `withheld` here, and the same connection re-bound to a foreign member sees them and not
        // these. Either alone is satisfiable by a policy that returns a constant set.
        probe(&mut conn, Some(uid(arm.member)), &own_vs_rest).await.is(
            2,
            0,
            &format!(
                "{}/{}: {} -- it must see EXACTLY its own two orders and none of the other four rows",
                arm.role, arm.member_type, PROBES[0]
            ),
        );
        probes_run += 1;

        probe(&mut conn, Some(uid(arm.foreign)), &own_vs_rest).await.is(
            0,
            arm.foreign_sees,
            &format!(
                "{}/{}: {} -- re-bound to a FOREIGN member of the same persona, the same connection \
                 must see NONE of the first member's rows (left) while still seeing that foreign \
                 member's own {} (right). A zero on the left without a non-zero on the right is \
                 satisfied by a policy that returns nothing to anybody",
                arm.role, arm.member_type, PROBES[1], arm.foreign_sees
            ),
        );
        probes_run += 1;

        // 3. No actor context at all. The non-zero pair is the SAME connection's previous
        // transaction, so this zero cannot be "the connection is broken" or "RLS returns nothing".
        let with_context = probe(&mut conn, Some(uid(arm.member)), &own_vs_rest).await;
        let no_context = probe(&mut conn, None, &own_vs_rest).await;
        assert_eq!(
            (with_context.seen, no_context.seen + no_context.withheld),
            (2, 0),
            "{}/{}: {} -- the SAME connection sees two rows one transaction earlier and must see \
             NOTHING once app.member_id is unbound. `current_setting(…, true)` returns NULL, the \
             predicate is false, and the answer is zero rows rather than an error the caller might \
             mistake for a schema problem",
            arm.role,
            arm.member_type,
            PROBES[2]
        );
        probes_run += 1;

        // 4. Claiming another persona's member type. O4 is reachable from this member id ONLY
        // through the decoys -- one with the wrong `scope_type`, one with the wrong `member_type` --
        // so a policy that drops either clause hands over exactly one visible row.
        let decoy = split_count(GUARDED, &format!("order_id = '{}'::uuid", uid(O4)));
        let p = probe(&mut conn, Some(uid(arm.member)), &decoy).await;
        assert_eq!(
            (p.withheld, p.seen),
            (2, 0),
            "{}/{}: {} -- this member id appears in the fixture under the OTHER persona's \
             member_type, and under the wrong scope_type, both pointing at O4. The connection must \
             see its own two rows (left) and never O4 (right)",
            arm.role,
            arm.member_type,
            PROBES[3]
        );
        probes_run += 1;

        // 5. The column. `restaurant_role` is the only persona granted `internal_notes`; every other
        // read of it, and every `SELECT *` (which expands to it), must be SQLSTATE 42501 -- asserted
        // on the code, never on message text.
        if arm.role == "restaurant_role" {
            let notes = format!(
                "SELECT count(internal_notes)::bigint, count(*) FILTER (WHERE internal_notes IS NULL)::bigint \
                 FROM {GUARDED} WHERE order_id = '{}'::uuid",
                uid(O1)
            );
            probe(&mut conn, Some(uid(arm.member)), &notes).await.is(
                1,
                0,
                &format!(
                    "{}: {} -- the ONE persona granted internal_notes must actually read it, or the \
                     denial below proves only that nobody can read anything",
                    arm.role, PROBES[4]
                ),
            );
        } else {
            for sql in [
                format!("SELECT internal_notes FROM {GUARDED}"),
                format!("SELECT * FROM {GUARDED}"),
            ] {
                let code = probe_sqlstate(&mut conn, Some(uid(arm.member)), &sql).await;
                assert_eq!(
                    code, "42501",
                    "{}: {} -- `{sql}` must be denied with SQLSTATE 42501. `SELECT *` expands to \
                     ungranted columns, and that is the shape that actually breaks application code",
                    arm.role, PROBES[4]
                );
            }
            // And the same connection still reads what it IS granted -- otherwise the denial above
            // is satisfied by a connection with no privileges at all.
            probe(&mut conn, Some(uid(arm.member)), &own_vs_rest).await.is(
                2,
                0,
                &format!("{}: denied on internal_notes, still reading its granted columns", arm.role),
            );
        }
        probes_run += 1;

        drop(conn);
        pool.close().await;
    }
    assert_eq!(
        probes_run,
        arms().len() * PROBES.len(),
        "the matrix ran {probes_run} probes where tables x personas x probes = {}",
        arms().len() * PROBES.len()
    );

    // ── The RIDER default-deny arm ──────────────────────────────────────────────────────────────
    // RD1 holds a REAL ORDER membership on O1, exactly as `DeliveryAcceptedByRider` writes it. The
    // rider therefore SEES that membership row -- which is the non-zero that makes the zero mean
    // something -- and still reads no conversation, because no rider policy exists on the guarded
    // table. Emitting one would correctly, per the index, hand the rider the customer<->restaurant
    // thread: a PRODUCT decision hiding inside a SQL emission, and chunk 1 must not settle it by
    // generating.
    {
        let pool = persona_pool(&url, DB_ENFORCING, "rider_role").await;
        let mut conn = pool.acquire().await.expect("pin the rider connection");
        let sql = format!(
            "SELECT (SELECT count(*) FROM {MEMBERSHIP})::bigint, (SELECT count(*) FROM {GUARDED})::bigint"
        );
        probe(&mut conn, Some(uid(RD1)), &sql).await.is(
            1,
            0,
            "rider_role/RD1: it must see its OWN membership row on O1 and ZERO conversations. The \
             non-zero is load-bearing -- without it this zero is equally produced by a broken \
             connection, an unapplied artifact, a missing fixture or a discarded set_config",
        );
        drop(conn);
        pool.close().await;
    }

    // ── C4, a member of nothing ─────────────────────────────────────────────────────────────────
    // Paired with C1 on the SAME connection, so "the customer policy returns nothing for anybody"
    // cannot satisfy it.
    {
        let pool = persona_pool(&url, DB_ENFORCING, "customer_role").await;
        let mut conn = pool.acquire().await.expect("pin the customer connection");
        let all = split_count(GUARDED, "true");
        let c1 = probe(&mut conn, Some(uid(C1)), &all).await;
        let c4 = probe(&mut conn, Some(uid(C4)), &all).await;
        assert_eq!(
            (c1.seen, c4.seen),
            (2, 0),
            "customer_role: C1 is a member of two orders and C4 of none -- same connection, same \
             fixture, one transaction apart"
        );
        drop(conn);
        pool.close().await;
    }

    // ── ADMIN: no policy, no grant, and both are the decision ───────────────────────────────────
    // A membership-based admin policy matches nothing (`ScopeMembership.rules`: "ADMIN holds NO rows
    // -- the guard short-circuits on the role"), so the only emittable one is `USING (true)`, and
    // shipping that as the first generated artifact teaches "the most privileged persona gets a
    // permissive policy" as normal output. Admin stays application-enforced. Asserted jointly with
    // the rider's grant so it is not a bare false.
    {
        // `has_any_column_privilege`, NOT `has_table_privilege`: under enforcing mode every persona
        // holds COLUMN grants and no table-level grant, and `has_table_privilege(…, 'SELECT')` is
        // FALSE for all of them. A gate written the obvious way would therefore report "no persona
        // can read anything" over a perfectly correct artifact -- and the version of this assertion
        // that got written first did exactly that. Cost: one red run; recorded here so the next
        // privilege gate is not written the same way.
        let (admin_reads, rider_reads): (bool, bool) = sqlx::query_as(
            "SELECT has_any_column_privilege('admin_role', $1, 'SELECT'), \
                    has_any_column_privilege('rider_role', $1, 'SELECT')",
        )
        .bind(GUARDED)
        .fetch_one(
            &PgPool::connect_with(options(&url).database(DB_ENFORCING))
                .await
                .expect("connect for the admin privilege check"),
        )
        .await
        .expect("has_any_column_privilege");
        assert_eq!(
            (admin_reads, rider_reads),
            (false, true),
            "admin_role must hold NO privilege on {GUARDED} while rider_role does -- the pair is what \
             makes the false mean 'deliberately ungranted' rather than 'nothing is granted to anyone'"
        );
        let admin_policies: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_policies WHERE schemaname='public' AND 'admin_role' = ANY(roles)",
        )
        .fetch_one(
            &PgPool::connect_with(options(&url).database(DB_ENFORCING))
                .await
                .expect("connect for the admin policy check"),
        )
        .await
        .expect("count admin policies");
        assert_eq!(admin_policies, 0, "no policy may name admin_role (see above)");
    }

    // ── PERMISSIVE: the load-bearing assertion, and it is the FIRST one ─────────────────────────
    // Everyone remembers to test enforcing; the untested clause is the one #637's mitigation rests
    // on. `permissive == every row` is what makes "ship permissive, tighten separately" true.
    for arm in arms() {
        let pool = persona_pool(&url, DB_PERMISSIVE, arm.role).await;
        let mut conn = pool.acquire().await.expect("pin the permissive connection");
        let all = split_count(GUARDED, "true");
        probe(&mut conn, Some(uid(arm.member)), &all).await.is(
            4,
            0,
            &format!(
                "{}: PERMISSIVE must withhold NO row -- it is the additive step, and it is what the \
                 tightening step reverts TO (#637). Enforcing gives this same member 2",
                arm.role
            ),
        );
        // Column privileges are subtractive too and have NO mode of their own, so permissive must
        // also carry the table-level grant: `SELECT *` and the withheld column both work here and
        // are both 42501 under enforcing.
        let star = format!(
            "SELECT count(*)::bigint, count(*) FILTER (WHERE internal_notes IS NULL)::bigint \
             FROM (SELECT * FROM {GUARDED}) x"
        );
        probe(&mut conn, Some(uid(arm.member)), &star).await.is(
            4,
            0,
            &format!(
                "{}: PERMISSIVE must not break `SELECT *`. A permissive USING(true) policy combined \
                 with an enforcing per-column grant breaks every read naming an ungranted column -- \
                 which is why the column list belongs to enforcing only",
                arm.role
            ),
        );
        drop(conn);
        pool.close().await;
    }

    // The rider's default-deny holds in BOTH modes, and that is deliberate: `mode:` gates the
    // PREDICATE, and a persona with no policy at all has no predicate to relax.
    {
        let pool = persona_pool(&url, DB_PERMISSIVE, "rider_role").await;
        let mut conn = pool.acquire().await.expect("pin the permissive rider connection");
        let sql = format!(
            "SELECT (SELECT count(*) FROM {MEMBERSHIP})::bigint, (SELECT count(*) FROM {GUARDED})::bigint"
        );
        probe(&mut conn, Some(uid(RD1)), &sql).await.is(
            12,
            0,
            "rider_role under PERMISSIVE: the membership policy relaxes to USING(true) and it sees \
             all twelve rows, while the guarded table stays at zero because the rider has no policy \
             there in EITHER mode. The absence of a policy is not a predicate `mode:` can loosen",
        );
        drop(conn);
        pool.close().await;
    }

    // ── The writer, negative arm FIRST ──────────────────────────────────────────────────────────
    write_arms(&url).await;

    // ── M1, as a permanent test ─────────────────────────────────────────────────────────────────
    the_rejected_guc_policy_hands_a_rider_the_customers_thread(&url).await;

    // ── M4, as a permanent test ─────────────────────────────────────────────────────────────────
    an_inherited_persona_grant_hands_one_login_role_the_union(&url, &admin).await;
}

/// The projector must keep writing — and the NEGATIVE arm comes first, because otherwise the first
/// green build is a build with a stopped projection.
///
/// **The negative half is a CHARACTERIZATION TEST OF POSTGRESQL**, marked as such deliberately.
/// Under `ENABLE` + `FORCE` with only `FOR SELECT` policies, a writer's `INSERT` errors loudly while
/// its `UPDATE` and `DELETE` return **0 rows, commit, and report success**. That behaviour is the
/// premise the projector policy exists for, so if a future PostgreSQL makes it error loudly we want
/// a FAILING test here, not a silent windfall.
///
/// The arm runs as **`migrator`, today's writer identity** — not as a role the artifact invents.
/// Under `enforcing` the owner holds no write policy (backfills run as `projector_order`, never as
/// the owner), so its refusal is correct; under `permissive` it must write freely, or `permissive`
/// is not additive and "ship permissive first" is a story with no mechanism.
async fn write_arms(url: &str) {
    let insert = format!(
        "INSERT INTO {GUARDED} (order_id, restaurant_id, customer_chat_enabled, status, messages, \
           internal_notes, opened_at, admin_invited, muted, created_at, updated_at, claim_events) \
         VALUES (gen_random_uuid(), '{}'::uuid, true, 'PLACED', '[]'::jsonb, '[]'::jsonb, now(), \
                 false, '[]'::jsonb, now(), now(), '[]'::jsonb)",
        uid(R1)
    );

    // ── negative: the OWNER, under enforcing ────────────────────────────────────────────────────
    let owner = persona_pool(url, DB_ENFORCING, "migrator").await;
    let code = owner
        .execute(insert.as_str())
        .await
        .err()
        .unwrap_or_else(|| panic!("the owner INSERTED under enforcing -- it holds no write policy"))
        .as_database_error()
        .and_then(|d| d.code())
        .expect("a SQLSTATE")
        .to_string();
    assert_eq!(
        code, "42501",
        "the owner's denied INSERT must be SQLSTATE 42501 (`new row violates row-level security \
         policy`), asserted on the code and never on message text"
    );
    let updated = owner
        .execute(format!("UPDATE {GUARDED} SET admin_invited = true").as_str())
        .await
        .expect("the UPDATE must SUCCEED -- that is the whole point of this arm")
        .rows_affected();
    let deleted = owner
        .execute(format!("DELETE FROM {GUARDED}").as_str())
        .await
        .expect("the DELETE must SUCCEED too")
        .rows_affected();
    let visible: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {GUARDED}"))
        .fetch_one(&owner)
        .await
        .expect("count as the owner");
    assert_eq!(
        (updated, deleted, visible),
        (0, 0, 0),
        "CHARACTERIZATION OF POSTGRESQL, not of us: under ENABLE + FORCE with only FOR SELECT \
         policies, the owner's UPDATE and DELETE affect ZERO rows, COMMIT, and report SUCCESS. A \
         projector's fold is mostly updates, so it would advance its checkpoint over a stream whose \
         effects never land -- green dashboards, zero checkpoint lag, and a frozen read model. If a \
         future PostgreSQL makes this error loudly, this assertion SHOULD fail: we want to hear it, \
         not inherit a silent windfall."
    );
    owner.close().await;

    // ── positive: the declared writer, under enforcing ──────────────────────────────────────────
    let projector = persona_pool(url, DB_ENFORCING, "projector_order").await;
    projector.execute(insert.as_str()).await.expect("projector_order INSERT");
    let updated = projector
        .execute(format!("UPDATE {GUARDED} SET admin_invited = true").as_str())
        .await
        .expect("projector_order UPDATE")
        .rows_affected();
    let membership_writes = projector
        .execute(
            format!(
                "INSERT INTO {MEMBERSHIP} (membership_id, scope_type, scope_id, member_type, \
                   member_id, granted_at, created_at, updated_at) \
                 VALUES (gen_random_uuid(), 'ORDER', gen_random_uuid(), 'CUSTOMER', \
                         gen_random_uuid(), now(), now(), now())"
            )
            .as_str(),
        )
        .await
        .expect(
            "projector_order must be able to write scopemembership TOO. My draft named one table; \
             the membership one causes the FIRST incident, because memberships stop landing and \
             every persona read goes empty -- which reads as 'RLS is broken'",
        )
        .rows_affected();
    assert_eq!(
        (updated, membership_writes),
        (5, 1),
        "projector_order must write both tables freely -- four seeded conversations plus the one it \
         just inserted"
    );
    projector.close().await;

    // ── the owner under PERMISSIVE: additive, or the rollback story has no mechanism ────────────
    let owner = persona_pool(url, DB_PERMISSIVE, "migrator").await;
    let inserted = owner.execute(insert.as_str()).await.expect(
        "the owner MUST be able to write under PERMISSIVE. `FORCE` + only FOR SELECT policies is \
         subtractive on writes INCLUDING for the owner, and today's writer identity IS the owner, \
         not a projector_order role that does not yet exist. An emitter that omitted the owner's \
         permissive write policy would ship a green matrix over a configuration whose every write \
         fails -- the single most likely way this chunk ships a false green",
    );
    let updated = owner
        .execute(format!("UPDATE {GUARDED} SET admin_invited = true").as_str())
        .await
        .expect("owner UPDATE under permissive")
        .rows_affected();
    assert_eq!(
        (inserted.rows_affected(), updated),
        (1, 5),
        "under PERMISSIVE the owner writes everything it writes today"
    );
    owner.close().await;
}

/// **M1, as a permanent `#[test]` rather than a PR-body claim.**
///
/// It constructs the REJECTED policy shape — one policy naming four roles, with the member type read
/// from `current_setting('app.member_type')` — and asserts that a rider connection **does** read the
/// customer's thread. It documents the breach in executable form, and an emitter rewrite that
/// "simplifies" back to a GUC goes red automatically. Same fixture, a different policy string,
/// near-zero marginal cost — and it is the structure that makes the shortcut unspellable, which is
/// the founder's stated reason for this chunk (*"this will help us to avoid AI errors and
/// unauthorised access"*).
///
/// This is the ONE place in this file where a policy literal is written by hand, and that is exactly
/// why: it is the shape the emitter must never produce.
async fn the_rejected_guc_policy_hands_a_rider_the_customers_thread(url: &str) {
    let setup = PgPool::connect_with(options(url).database(DB_REJECTED))
        .await
        .expect("connect to the rejected-shape database");

    // Sweep the GENERATED read policies without naming one: a pattern over pg_policies, not a
    // literal, so this stays a mutation of the artifact rather than a second hand-written design.
    setup
        .execute(
            r"DO $$ DECLARE p record; BEGIN
                FOR p IN SELECT policyname, tablename FROM pg_policies
                          WHERE schemaname = 'public' AND policyname LIKE '%\_read\_%' LOOP
                  EXECUTE format('DROP POLICY %I ON %I', p.policyname, p.tablename);
                END LOOP; END $$;",
        )
        .await
        .expect("sweep the generated read policies");

    // THE REJECTED SHAPE. Do not copy this into an emitter.
    setup
        .execute(
            "CREATE POLICY orderconversation_read_all ON orderconversation \
               FOR SELECT TO customer_role, restaurant_role, rider_role, admin_role \
               USING (EXISTS (SELECT 1 FROM scopemembership m \
                               WHERE m.scope_type = 'ORDER' \
                                 AND m.scope_id = orderconversation.order_id \
                                 AND m.member_type = current_setting('app.member_type', true) \
                                 AND m.member_id = nullif(current_setting('app.member_id', true), '')::uuid)); \
             CREATE POLICY scopemembership_read_all ON scopemembership \
               FOR SELECT TO customer_role, restaurant_role, rider_role, admin_role \
               USING (member_type = current_setting('app.member_type', true) \
                  AND member_id = nullif(current_setting('app.member_id', true), '')::uuid);",
        )
        .await
        .expect("install the rejected GUC-shaped policies");
    setup.close().await;

    let pool = persona_pool(url, DB_REJECTED, "rider_role").await;
    let mut conn = pool.acquire().await.expect("pin the rider connection");

    let honest = guc_scoped_count(&mut conn, "RIDER", uid(RD1)).await;
    let forged = guc_scoped_count(&mut conn, "CUSTOMER", uid(C1)).await;
    assert_eq!(
        (honest, forged),
        (1, 2),
        "THE BREACH, in executable form. Under a policy whose member type comes from a GUC, the same \
         rider connection reads its own one delivery thread and then TWO of the customer's, by \
         setting a string. Nothing cross-checks that string against the database role it is \
         connected as, so the whole role tree is bypassed by one `set_config` -- a role hierarchy any \
         caller can step outside of is documentation with a CREATE ROLE in front of it.\n\
         If this assertion ever fails at (1, 0), read it as GOOD NEWS about PostgreSQL and BAD NEWS \
         about this test: the rejected shape stopped being reachable, and the emitter's literal-bound \
         member type is no longer the thing standing between the rider and the thread."
    );

    // And the wall the correct design rests on, on the same connection: Postgres gates the ROLE
    // change even though it does not gate the setting. That asymmetry IS the design.
    let code = probe_sqlstate(&mut conn, None, "SET ROLE customer_role").await;
    assert_eq!(
        code, "42501",
        "the rider connection must be REFUSED `SET ROLE customer_role`. This assertion is only \
         meaningful because the connection is a real non-superuser LOGIN role -- a superuser may SET \
         ROLE to anything, and on CI's `postgres` session this would have gone red against the \
         CORRECT design"
    );

    drop(conn);
    pool.close().await;
}

async fn guc_scoped_count(conn: &mut PgConnection, member_type: &str, member: Uuid) -> i64 {
    begin_scope(conn, Some(member)).await;
    sqlx::query("SELECT set_config('app.member_type', $1, true)")
        .bind(member_type)
        .fetch_one(&mut *conn)
        .await
        .expect("forge app.member_type");
    let n: i64 = sqlx::query_scalar(&format!("SELECT count(*)::bigint FROM {GUARDED}"))
        .fetch_one(&mut *conn)
        .await
        .expect("count under the rejected policy");
    conn.execute("COMMIT").await.expect("commit");
    n
}

/// **M4, as a permanent test — and it came out sharper than the prediction it was written from.**
///
/// The prediction (`dba`, unmeasured): a login role granted BOTH persona roles with `INHERIT`, and
/// no `SET ROLE`, reads the union of both personas' rows. **Measured on PG 16.13: it does** —
/// {O1,O3} ∪ {O4} = three rows for a member id that legitimately reaches two, the third arriving
/// through the decoy that carries C1's uuid under the `RESTAURANT` member type.
///
/// The sharper half is what the measurement added. Inheritance for RLS purposes is a **per-grant**
/// property (`pg_auth_members.inherit_option`), fixed at `GRANT` time from the member's `INHERIT`
/// attribute. **`ALTER ROLE … NOINHERIT` after the grant does not change it** — the role keeps
/// inheriting, silently. So the composition in PROP-20260818-010343 §5 (an app connects as its own
/// login role and issues `SET LOCAL ROLE <persona_role>`) is only a wall if the emitter writes
/// `GRANT <persona_role> TO <app_login_role> WITH INHERIT FALSE` **explicitly**. Relying on the login
/// role's attribute — the obvious way to write it — hands every app the union of every persona it is
/// granted, with no `SET ROLE` and nothing on the page to notice.
///
/// Chunk 1 emits no login roles, so this is a finding for the next chunk. It is kept here because it
/// costs one role and one assertion, and because it goes red the day that emitter is written the
/// obvious way.
async fn an_inherited_persona_grant_hands_one_login_role_the_union(url: &str, admin: &PgPool) {
    for (role, attr) in [("inherit_svc", "INHERIT"), ("noinherit_svc", "NOINHERIT")] {
        admin
            .execute(format!("CREATE ROLE {role} LOGIN {attr} PASSWORD '{PW}'").as_str())
            .await
            .unwrap_or_else(|e| panic!("create {role}: {e}"));
        admin
            .execute(format!("GRANT customer_role, restaurant_role TO {role}").as_str())
            .await
            .unwrap_or_else(|e| panic!("grant the personas to {role}: {e}"));
    }
    // The "obvious hardening" that does NOTHING, and is the finding: the grant above already
    // recorded inherit_option = true, and the role attribute is only the DEFAULT for FUTURE grants.
    admin.execute("ALTER ROLE inherit_svc NOINHERIT").await.expect("the no-op hardening");

    let pool = persona_pool(url, DB_ENFORCING, "inherit_svc").await;
    let mut conn = pool.acquire().await.expect("pin the inheriting connection");
    let union = probe(
        &mut conn,
        Some(uid(C1)),
        &split_count(GUARDED, &format!("order_id IN ({})", id_list(&[O1, O3]))),
    )
    .await;
    assert_eq!(
        (union.seen, union.withheld),
        (2, 1),
        "one login role granted BOTH persona roles reads the UNION -- C1's own two orders PLUS O4, \
         which it reaches only through the decoy carrying C1's uuid under the RESTAURANT member \
         type. Each persona alone reads two.\n\
         Note what did NOT save it: `ALTER ROLE … NOINHERIT` was issued after the GRANT and is a \
         no-op, because inheritance is a PER-GRANT property fixed at GRANT time \
         (pg_auth_members.inherit_option). The emitter that writes the `GRANT <persona> TO <app>` \
         edges must say `WITH INHERIT FALSE` explicitly."
    );
    drop(conn);
    pool.close().await;

    // The same shape with the option actually set: no rows, and not because of a policy -- the login
    // role has no privilege at all until it SET ROLEs. Denial, not silence.
    let pool = persona_pool(url, DB_ENFORCING, "noinherit_svc").await;
    let mut conn = pool.acquire().await.expect("pin the non-inheriting connection");
    let code = probe_sqlstate(&mut conn, Some(uid(C1)), &format!("SELECT order_id FROM {GUARDED}")).await;
    assert_eq!(
        code, "42501",
        "a login role whose persona grants are NON-inheriting holds nothing until it SET ROLEs, and \
         the refusal is LOUD (42501) rather than an empty result set -- which is the right failure \
         shape and the reason the per-grant option matters"
    );
    drop(conn);
    pool.close().await;
}
