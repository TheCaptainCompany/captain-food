# Dispatch — enforcement chunk 1: the authorization matrix, on the table that cannot teach the wrong thing

- **Issue**: tracking [#638](https://github.com/TheCaptainCompany/captain-food/issues/638) · design [PROP-20260818-010343](../proposals/PROP-20260818-010343-database-level-security-the-measured-design.md) · rollback constraint [#637](https://github.com/TheCaptainCompany/captain-food/issues/637)
- **Base**: `main` @ `779bb76` — **verify it yourself before writing anything** — `git fetch && git rev-parse origin/main`, and confirm your checkout equals it. Six cards in a row carried a wrong base. A coordinator-supplied SHA is `UNVERIFIED input`; so is `dba`'s (it read `e973896` without fetching).
- **Reversibility class**: **REVERSIBLE INTERNAL — and `farley` corrected my reason, which matters because the next card inherits reasons.** I wrote that nothing here can reach a real database because the cluster is unprovisioned. **That is false.** `.github/workflows/db-migrate.yml` triggers on `workflow_run: [deploy] completed` and runs `sqlx migrate run` against the real `DATABASE_URL` secret; `deploy` is `workflow_dispatch`-only and production is suspended **by decision**, not by physics. Anything dropped into `migrations/` reaches a real Postgres the moment somebody dispatches a deploy, unattended. **The class is true because of the fence, not the environment** — so the fence becomes mechanical, not prose (see Deliver §0). It becomes HIGH-CONSEQUENCE at the cutover, under a different card. **`beck` accepts the class with one caveat that stands**: this chunk's deliverable *is the evidence*, and a wrong test here is not reversible in the way that matters, because the cutover card will cite it. Reversible, not low-stakes — do not let the class shrink the checkpoint.
- **Roster**: **three** — `beck`, `dba`, `farley`. Sized by the class (ADR-20260816-134352). Six other lenses ruled on this surface within 24 hours and their findings are in the proposal. No lens objected to the sizing.

## The founder's reason, which is not the one the lenses argued

> *"This will help us to avoid AI errors and unauthorised access."* — 2026-08-18

Every lens argued defence in depth **against an attacker**. His first clause is different and, here, stronger: **row security holds when a resolver written by an agent forgets a filter.** That omission is likelier in this repository than a forged token, and it is the failure application-layer review is worst at catching, because the code looks correct. Recorded as §1.1 of the proposal.

## Scope: one table, two policied personas, one deny arm

**`orderconversation`** — and `dba` corrected *why*. `legal` picked it on sensitivity (Article 9 free text). The mechanism argument is stronger: **it has no member-bearing column.** No `customer_id`, no `rider_id`. So the policy *cannot* be short-circuited into `USING (customer_id = current_setting(...))` — the predicate is forced through the `scopemembership` subquery, which is the only path that makes the executor meet the zero-row failures that govern every later table. Pick a table with a `customer_id` and the first cut goes green without ever touching `scopemembership`, and the lesson is never learned.

| Persona | Chunk 1 |
|---|---|
| `customer_role` | policied |
| `restaurant_role` | policied (and the only one granted `internal_notes`) |
| `rider_role` | **default-deny arm** — holds `SELECT`, gets no policy, must read zero |
| `admin_role` | **no policy at all** |
| `projector_order` | `FOR ALL USING (true) WITH CHECK (true)` on **both** tables |

Two of these are `dba` corrections to my draft and both change the deliverable:

- **No ADMIN policy.** `ScopeMembership.rules` declares *"ADMIN holds NO rows — the guard short-circuits on the role."* A membership-based admin policy matches nothing, so the only emittable one is `USING (true)` — and shipping that as the first generated artifact teaches "the most privileged persona gets a permissive policy" as normal output. Admin stays application-enforced; say so in the PR body.
- **No RIDER policy.** `ScopeMembership` is ORDER-scoped, and `DeliveryAcceptedByRider` grants the rider an ORDER membership — so a generated rider policy would correctly, per the index, hand the rider the customer↔restaurant thread. **That is a product decision hiding inside a SQL emission**, and chunk 1 must not settle it by generating.

## What the test asserts, and the one rule that governs all of it

Five probes per persona: sees **its own** row · sees **zero** for a foreign member · sees **zero** with no actor context · sees **zero** claiming another persona's member type · **cannot read** a non-granted column.

> **`beck`'s rule, and it is the whole design: no bare zero.** Four of the five are "sees zero", and the proposal lists four ways to get zero for the wrong reason. **Every zero is asserted jointly with a non-zero it does see — same assertion, same connection, same fixture.** `assert_eq!((own, foreign), (1, 0), …)`, never `assert_eq!(foreign, 0)`. A lone zero passes when the fixture inserted nothing, when the membership rows are absent, when `set_config` landed outside a transaction, when the migration never ran, and when RLS is so broken nobody sees anything. That is a thermometer reading zero because it is unplugged.

**Preflight, or the suite runs inert and green**: before any case, assert `relrowsecurity` and `relforcerowsecurity` on `pg_class`, and a non-zero `pg_policies` count. Without it, "0 rows" is indistinguishable from "the generated SQL was never applied".

**And one line that kills an entire class** (`dba`): inside the scoped transaction, before every arm, assert `current_setting('is_superuser') = 'off'`.

## The two false results this card would have produced — both caught before dispatch

**CI's session is a superuser** (`postgres://postgres:postgres@…`), and `sqlx migrate run` therefore creates the table **superuser-owned**.

- **`FORCE` is observationally inert there**, because it removes only the *owner's* exemption and never the superuser bypass — and for a plain non-owner persona, `ENABLE` alone already enforces. So planting "drop `FORCE`" would have changed nothing, gone **green**, and taught us `FORCE` is unnecessary — the exact inverse of the measured finding that `NO FORCE` lets a rider read every order.
- **A superuser may `SET ROLE` to anything**, so the `permission denied to set role` wall would have gone **red against the correct design** — a false red at 2am is how an assertion gets relaxed.

**Fixes, both required**: the fixture `ALTER TABLE … OWNER TO` a non-superuser test migrator, creates real `LOGIN` roles, and opens **a separate pool per persona**. The `FORCE` mutation is re-aimed **at the owner** — as owner, with `FORCE` → 0 rows; without → every row.

## The fixture — where the discrimination lives

Four conversations across two restaurants and four customers, so the two personas' visible sets **overlap without being equal**:

| Order | customer | restaurant |
|---|---|---|
| O1 | C1 | R1 |
| O2 | C2 | R1 |
| O3 | C1 | R2 |
| O4 | C3 | R2 |

Eight real `ORDER`-scoped membership rows, plus **two decoys, and they are the point**:

- `(scope_type='RESTAURANT', scope_id=O4, member_type='CUSTOMER', member_id=C1)` — right ids, wrong scope type. Catches a policy that drops the `scope_type` clause.
- `(scope_type='ORDER', scope_id=O4, member_type='RESTAURANT', member_id=C1)` — **C1's uuid under the RESTAURANT member type. This is the assertion-4 decoy, and my draft omitted it.** Without it, "sees zero when claiming another persona's member type" passes because there is no row to find, and the arm certifies nothing.

Plus **C4, a member of nothing**. Expected: `customer_role`/C1 → {O1,O3}; `restaurant_role`/R1 → {O1,O2} — overlapping at O1 only, so a "returns the other persona's set" bug is visible; C4 → 0; no context → 0; `rider_role` → 0.

Column arm, named rather than left to the executor: `restaurant_role` reads `internal_notes` on O1 → 1 row; `customer_role` reads the same → SQLSTATE **`42501`**. **And assert `SELECT *` as `customer_role` errors** — `*` expands to ungranted columns, which is the shape that actually breaks application code.

## Mutations — plant, see red, revert, state the count in the PR body

The mutation goes in the **emitter source** and the generator is re-run; the test reads only the generated artifact. **No SQL literal describing a policy may appear in the test file** — otherwise the suite proves that hand-written naive SQL differs from hand-written correct SQL, which is a tautology. *"The objects do not exist"* is a setup failure and is **not** counted among the mutations.

| # | Semantic edit | Expected red |
|---|---|---|
| **M1** | one policy naming four roles, member type read from `current_setting('app.member_type')` | assertion 4 — a rider sets the GUC to `CUSTOMER` and reads customer rows (`dba` measured 2). **M1 is NOT a PR-body claim — it is a permanent `#[test]`** (`farley`): construct the *rejected* policy shape and assert the rider **does** read the customer's rows. It documents the breach in executable form, and an emitter rewrite that "simplifies" back to a GUC goes red automatically. Same fixture, different policy string, near-zero marginal cost — **and it is the structure that makes the shortcut unspellable, which is the founder's stated reason for the chunk.** M2–M7 may stay PR-body claims. |
| **M2** | drop `FORCE`, keep `ENABLE` — **aimed at the owner**, per above | the owner reads every row instead of none |
| **M3a** | replace column grants with a table-level `GRANT SELECT` | assertion 5 |
| **M3b** | replace the join-first access path with `SELECT FROM orderconversation` | plan degrades — see the note below |
| **M4** | a login role granted **both** persona roles **with `INHERIT`**, no `SET ROLE` | reads the **union** of both personas' rows. `dba` predicts this and **has not measured it** — `UNVERIFIED input`. If it comes out green, that is a finding worth more than the rest of the chunk |
| **M5** | delete the `scopemembership` self-row policies | assertion 1, every persona, zero — the zero-row member most likely to bite at cutover |
| **M6** | `set_config(…, true)` outside an explicit transaction | assertion 1 |
| **M7** | **swap the two member ids in the fixture** | assertions 1 **and** 2 both flip |

**M7 is the one `beck` would refuse to ship without** — it is the fixture's own mutation test and the direct answer to "is this grading its own homework".

**Drop the `EXPLAIN`/no-`Seq Scan` assertion from this suite.** At two fixture rows Postgres seq-scans everything; the assertion is either a false red or needs `dba`'s 200k rows. Assert the column denial, record the 0.263 ms / 180.569 ms figures as `UNVERIFIED input` in the PR body, and give the plan-shape gate its own volume test later.

## Permissive first — and `dba`'s concern, which changes the emitter

[ADR-0043](../adr/0043-db-migration-release-strategy.md) makes rollback *"redeploy the previous app"*, valid only because migrations are additive. A policy is subtractive on reads. So policies ship `USING (true)` first and tightening is a separate revertible step (#637).

> **CONCERN (`dba`), and it must be cleared in the diff**: *permissive as I drafted it is not additive, because **column privileges are subtractive too and have no mode**.* A permissive `USING (true)` policy combined with an enforcing `GRANT SELECT (cols)` still breaks every read naming an ungranted column, and breaks **every `SELECT *`** outright. **Permissive mode must therefore also emit the table-level `GRANT SELECT`**; the column list belongs to enforcing only. Corollary: **assertion 5 exists in enforcing mode only**, which contradicts my flat "both modes differ" — the modes differ on *rows* everywhere and on *columns* by design.

**Two databases, one per mode, built in the same setup** — `DROP POLICY` and `ALTER TABLE … FORCE` take `ACCESS EXCLUSIVE`, so do not mutate mode mid-run. "The two modes differ" becomes a comparison of two result sets rather than a time-ordered mutation, and the permissive case stays permanently covered. `beck`'s load-bearing assertion is the *first* one: `permissive == all_ids()`. Everyone remembers to test enforcing; the untested clause is the one #637's mitigation rests on.

## The projector must keep writing — negative arm first

Under `ENABLE` + `FORCE` with only `FOR SELECT` policies, `INSERT` errors loudly while **`UPDATE` and `DELETE` return 0 rows, commit, and report success**. A projector whose fold is mostly updates advances its checkpoint while the read model freezes — green dashboards, order statuses that stop moving at peak.

Two tests. The negative one is a **characterization test of PostgreSQL**, marked as such in its doc comment, because that behaviour is the premise the projector policy exists for: if a future PG makes it error loudly, we want a failing test, not a silent windfall. `INSERT` denial asserted on **SQLSTATE `42501`**, never on message text. **The policy is needed on `scopemembership` as well as `orderconversation`** — my draft named one table, and the membership one causes the first incident, because memberships stop landing and every persona read goes empty, reading as "RLS is broken".

## Where it lives, and how we know it ran

- **Not a new crate.** CI runs `cargo test -p infrastructure -p sirene_ingest -p server -p hubrise-adapter -p actor_runtime --tests` — a suite in a new crate **never runs in CI and nothing reports it** (#335). Land it under `crates/infrastructure/tests/`, or change the `-p` list in the same diff.
- **Harness**: `crates/infrastructure/tests/main/common.rs` plus `crates/db_test_gate/src/lib.rs`, which already inverts the polarity (panics without `DATABASE_URL` unless an explicit opt-out is set; CI sets `DB_TESTS_REQUIRED=1`). **My draft cited `graphql_typed_send.rs` — that file has no database at all** and following it literally produces a test with no Postgres.
- **Positive proof it did not skip**: the suite name absent from the `DB-GATED SUITES SKIPPED` receipt line, the CI log's `test …` count quoted in the PR body and matched against the matrix length, and the `pg_class`/`pg_policies` preflight.
- **Roles are cluster-scoped; `reset_schema` is schema-scoped.** Clean up in **setup**, not teardown (teardown does not run on panic), and give the suite **its own database** per run.


## §0 — Deliver this FIRST, because it is what makes the class statement true

A codegen test in `tools/codegen-rs/src/tests.rs`, roughly ten lines:

> No file under `migrations/` contains `ROW LEVEL SECURITY`, `CREATE POLICY` or `CREATE ROLE`, beyond the file that already does (`20260808070000_claude_ro_select_only.sql`).

Written as a **conditional that converts with no edit** at the cutover: *either* no `migrations/*_security.sql` exists, *or* its body is byte-identical to `specs/generated/security.generated.sql`. Today it asserts the fence; at the flip the same test asserts the two artifacts are the same bytes. That is the honest answer to "CI must prove what we ship" — the equality is not provable today because the migration does not exist, and this is the strongest thing writable now. It also removes the temptation to hand-write the cutover migration.

**As landed** (review correction, 2026-08-18): the implementation accepts EITHER artifact, because the permissive one ships first. An intermediate draft also carried a `!checked_cutover` tripwire asserting the walk had not happened yet — that draft is what made "converts with no edit" false, since the flipper would have met a red gate whose message told them to delete an assertion. The tripwire was **removed**, not re-worded: the flip already adds a file to the deployed migration chain, so its visibility in the diff was never scarce, and a gate must not make its own deletion the documented way past it. The claim in this section is now true of the code.

## The flip condition is the WALK, not the cutover — `farley`'s correction

I wrote "applied at the cutover". ADR-20260817-105844 puts the local acceptance harness ([#556](https://github.com/TheCaptainCompany/captain-food/issues/556)) and the six-clause walk on the enforced stack **before** any production cutover, and production stays suspended as a decided state. So:

- *"Applied at the walk"* is a dated, local, near thing somebody is already building.
- *"Applied at the cutover"* is a cluster that does not exist and a suspension nobody has scheduled lifting.

Same work, honest condition. **Name the walk as the flip event.** And add one line to `docs/STATUS.md` at merge — *generated security SQL exists, applied to no database, since 2026-08-18* — so the unapplied artifact has a **visible age**. If that line is still there in six weeks, the chunk failed and it will be legible.

## Permissive gates one third of the subtractive surface — and one arm is the likeliest way this ships green over a broken config

`mode:` as I attached it covers the read policy predicate only. Three other things in the same emission are subtractive and ungated:

1. **`FORCE` + only `FOR SELECT` policies is subtractive on WRITES, including for the owner.** So *"permissive"* is a misnomer unless the writer policy covers **every role that actually writes the table today** — which today is the app/migrator identity, **not** a `projector_order` role that does not yet exist. `farley`: *"if the emitter only emits `FOR ALL TO projector_{scope}`, then `mode: permissive` already breaks writes. This is the single most likely way this chunk ships a green test over a broken configuration."* **The matrix's projector arm must run as TODAY's writer identity, not as a role the artifact invents.**
2. **Column grants are subtractive the instant they land** and are not under `mode:`. An older binary doing `SELECT *` gets `permission denied` — loud, better than empty, but not rollback-by-redeploy.
3. **`REVOKE CONNECT/TEMPORARY … FROM PUBLIC` is subtractive at database level** and is likewise ungated.

**Decide in the diff**: either `mode:` gates the whole per-table subtractive surface (predicate **and** column-grant narrowing), or the card states that the database-level statements are cutover-only artifacts with their own gating. **Fence the database-level statements out of this chunk's applied set regardless** — see the leakage note below.

**What the tightening step's rollback actually is**, since ADR-0043 gives it nothing and the ledger is append-only: *a forward, generated migration that is the permissive emission of the same table* — which already exists, in bytes, in every CI run. That is why "both modes, asserted to differ" is load-bearing beyond coverage: both come off one emitter, so the revert artifact is **regenerated deterministically** rather than hand-authored under incident pressure. Two further preconditions of the flip, named now and **not** built here: a per-table observability contract alerting on a *drop* in rows returned, with a dead-man's switch (a threshold alert goes silent exactly when a policy withholds everything); and a **measured** commit-to-reverted wall clock via `workflow_dispatch` + `skip_deploy_check`, because unmeasured MTTR on a subtractive change is what turns a five-minute revert into a two-hour outage.

**Flip order, stated as a general rule**: flip the table whose empty set is an immediate unambiguous outage **before** the table whose empty set looks like projection lag. Counter-intuitive and correct.

## CI wiring — concrete

- **`crates/infrastructure/tests/rls_matrix.rs`**, following `crates/infrastructure/tests/authorized_no_birth_metric.rs` exactly: separate binary, `#[path = "main/common.rs"] mod common;`. Picked up by the existing `-p infrastructure --tests` with **no workflow edit**, and inherits `db_test_gate` polarity and `reset_schema` for free.
- **One generated file**, `specs/generated/security.generated.sql`, containing every statement **in apply order** — one file, because an ordered set of files means the order lives somewhere else and drifts.
- The test **reads that file at runtime** (or `include_str!`) after `reset_schema()`. **No SQL literal describing a policy in the test** — except M1's deliberately rejected shape.
- **`reset_schema()` is already the cutover in miniature**: `DROP SCHEMA public CASCADE` then replay all migrations, on every DB suite, on every commit. The security artifact rides it.
- **Role DDL must be idempotent** — `DROP SCHEMA` does not drop cluster-global roles. Copy the `DO $$ … IF NOT EXISTS … EXCEPTION WHEN insufficient_privilege` shape from `migrations/20260808070000_claude_ro_select_only.sql`; it is the house pattern and already handles the managed-provider case we will meet at Supabase and CNPG.
- **Leakage is this chunk's flake risk.** Roles and database-level grants survive `reset_schema` into every later suite in the same CI run. If the applied set ever revokes `CONNECT`/`TEMPORARY` from `PUBLIC`, it poisons the shared database and produces the silent-write failure **somewhere else, attributed to something else**. Fence those statements out, or give the matrix its own database.
- **Assert the ordering behaviourally**, not by filename: for the derived table set, `relrowsecurity AND relforcerowsecurity`.

## The one gate to add

**`rls-matrix-covers-every-generated-policy`** — parse `security.generated.sql` for its (table, role) policy set and assert `matrix_arms == tables x personas x probes`, **both sides derived**. A green run with zero tests fails it, because zero is not the expected number — which is the answer to "how do I know it did not skip" that `db_test_gate` alone cannot give (libtest prints `running 0 tests` and exits 0). It also makes an uncovered table red the day the emitter grows to table two, rather than silent.

Do **not** add a workflow-level `grep 'test result: ok'` step: it lives in YAML, where the next edit deletes it and no compiler notices. Keep the proof inside the artifact being proven. And do not add an artifact-identity gate — the existing codegen drift gate subsumes it.

## Fences

- **One table.** The other three named in the first draft are each broken in their own way; not this chunk.
- **Nothing reaches a real database.** No migration is added to the deployed chain — the test applies the generated SQL itself. That means the emitted script must be **re-runnable** (`DROP POLICY IF EXISTS`, `DO $$` guards on `CREATE ROLE`), free of psql metacommands, and valid as one multi-statement simple query.
- **No `specs/**` change.** `mode:` becomes an **emitter parameter producing two files**, not a DSL key — the DSL key lands with the first table that actually ships enforcing.
- **Do not implement `identity_binding`** (#641, sequenced separately).
- Every other defect found becomes an issue, not this diff.

## Findings

_(Lenses and the executor append here.)_

### Executor, 2026-08-18 — post-review corrections

Four corrections from the independent review (which PASSED, having stood up PG 16.13 and replanted
five mutations itself), all landed on the branch:

1. **The tripwire was removed, not re-worded.** See the "As landed" note in §0 above.
2. **Six → seven.** The mutation count in `rls_matrix.rs` and `STATUS.md` disagreed with its own
   table (M1, M2, M3a, M4, M5, M6, M7). Both now state seven and name the antecedents; M3b is
   explicitly excluded as never-run rather than silently absent. ADR-20260817-105845 class, in the
   evidence artifact itself.
3. **`Probe` is now sealed** in an inner `probe_pair` module with private fields and pair-returning
   accessors only, so "a single probe number is unspellable" is a fact about the compiler rather
   than about the care of whoever writes the next arm. The four direct field reads became
   `pair()`, `flipped()`, `seen_vs_seen()`, `seen_vs_total_of()`.
4. **`Arm.member_type` is now actually asserted against the artifact.** `policy_set` grew into
   `policies()`, which captures the whole USING predicate; the offline coverage gate asserts each
   arm's literal on BOTH layers (guarded table and membership table) and re-asserts G-1 per policy.
   Previously the field's docstring claimed an assertion that did not exist.

**Card wording, for the next dispatch**: M1/M4 are described above (line 76) as a permanent
`#[test]`. They are permanent **arms** — async helpers the mega-test awaits, so they cannot be run
alone and a failure reports under the parent. That structure was accepted by review as a chosen
trade given `TestDb`'s binary-wide lock, not an accident.

### Executor, 2026-08-18 — delivered on `638-rls-authorization-matrix`

**Base**: the card's `779bb76` is stale but sound — `origin/main` was `42bbbe4`, whose only commit
ahead of `779bb76` is the card itself. Branched from `42bbbe4`.

**Two of the card's lens findings needed a correction to build against, and one turned out N/A.**

1. **M2 and `farley`'s fence #1 are in direct tension, and the card does not resolve it.** M2 wants
   *"drop `FORCE` → the owner reads every row instead of none"*, which requires the owner to hold **no**
   policy. Fence #1 requires the write policy to cover today's writer identity — which **is** the
   owner. Both cannot hold in one artifact. Resolved by making the owner's `FOR ALL` policy
   **mode-gated**: present in `permissive`, absent in `enforcing`
   ([ADR-20260818-171500](../adr/ADR-20260818-171500-mode-gates-the-whole-per-table-subtractive-surface-including-the-owners-write-policy.md)).
   Permissive is then genuinely additive for today's writer (measured: owner `INSERT 1`, `UPDATE 5`),
   and enforcing is where M2 goes red (measured: 4 rows / `UPDATE 4` without `FORCE`, 0 / `UPDATE 0`
   with it; a persona reads 2 either way, which is exactly why the mutation had to be aimed at the
   owner). Had I taken either lens alone, this chunk would have shipped a green matrix over a broken
   configuration in one direction or a `FORCE` mutation that cannot go red in the other.
2. **M3b is not applicable and that is a finding, not a gap.** Chunk 1 emits no persona views (A-6 is
   sequenced separately), so there is no join-first access path to degrade. Nothing was skipped.
3. **The card's "one generated file" and "`mode:` produces two files" read as a contradiction.**
   Resolved as *one file per mode* — `security.generated.sql` and `security.permissive.generated.sql`,
   each carrying every statement in apply order. §0's fence accepts **either** as the cutover
   migration, because permissive is what ships first.
4. **The rider's zero needed a non-zero to be paired with, and the fixture as drafted had none.**
   Added a real `(ORDER, O1, RIDER, RD1)` membership — the grant `DeliveryAcceptedByRider` actually
   writes — plus a `scopemembership` self-row policy for the rider. The rider now sees its own
   membership row and zero conversations in one assertion, which is the product decision the card
   fences, stated executably. Also added a third decoy, `(ORDER, O4, CUSTOMER, R1)`, so probe 4 is
   symmetric for `restaurant_role`; the card named only the customer-side decoy.

**Two measurements that were not in anybody's lens and change the next chunk** — both now in
`PROP-20260818-010343` §5 / ADR-20260818-171500:

- **`ALTER ROLE … NOINHERIT` after a `GRANT` is a no-op on PG 16.** Inheritance for policy purposes is
  per-grant (`pg_auth_members.inherit_option`), fixed at `GRANT` time. §5's whole wall therefore
  exists only if A-1 emits `WITH INHERIT FALSE` explicitly. M4 came out red as `dba` predicted
  ({O1,O3,O4} where each persona alone reads 2) **and** the natural fix does not work.
- **`has_table_privilege(role, table, 'SELECT')` is FALSE for a role holding only column grants.**
  Under enforcing that is every persona, so a privilege gate written the obvious way reports "no
  persona can read anything" over a correct artifact. Use `has_any_column_privilege`.

**Not done, reported rather than fixed** (fence: *"every other defect becomes a reported issue"*):
gates G-2, G-4, G-5 and G-7 of §10 are unbuilt; artifact A-6 and A-7 are unbuilt; the emitter's
guarded-table set and the withheld-column map are declared constants in `emit/security.rs` rather than
DSL keys, and both fail loudly at `make generate` if the DSL moves under them.
