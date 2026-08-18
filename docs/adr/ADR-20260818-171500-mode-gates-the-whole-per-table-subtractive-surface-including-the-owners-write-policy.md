# ADR-20260818-171500 — `mode:` gates the WHOLE per-table subtractive surface, including the owner's write policy

**Status**: Accepted · **Date**: 2026-08-18 ·
**Decider**: the executor of dispatch `docs/dispatch/638-rls-authorization-matrix-chunk1.md`, taking
the decision the card explicitly required to be taken **in the diff** (*"either `mode:` gates the
whole per-table subtractive surface (predicate **and** column-grant narrowing), or the card states
that the database-level statements are cutover-only artifacts with their own gating"*) ·
**Realizes**: [#638](https://github.com/TheCaptainCompany/captain-food/issues/638) chunk 1 ·
**Design**: [PROP-20260818-010343](../proposals/PROP-20260818-010343-database-level-security-the-measured-design.md)
§6, §9 (A-4/A-5), §11.2 ·
**Sequenced by**: [ADR-20260818-004647](ADR-20260818-004647-database-level-security-lands-at-the-cutover-and-the-settlement-read-returns-to-scope.md)
(WHEN — untouched here; this ADR answers only WHAT IS EMITTED) ·
**Rollout constraint**: [#637](https://github.com/TheCaptainCompany/captain-food/issues/637) —
ADR-0043's *"redeploy the previous app"* is a no-op against a subtractive change ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

## Status

Accepted.

## Context — two lens findings that were both right and looked contradictory

`mode: permissive` exists so the first landing of database-level security is **additive**: row
security enabled, `FORCE` on, the whole scoped-transaction path exercised under real traffic, and
**no row withheld** — so rollback-by-redeploy still means something. Two findings landed against the
draft in which `mode:` gated only the read predicate:

- **`farley`**: `FORCE` + only `FOR SELECT` policies is **subtractive on WRITES, including for the
  owner**, and today's writer identity **is** the owner (`databases.yaml#/read_order/owner` =
  `migrator`), not a `projector_order` role that does not yet exist. An emitter that only ever wrote
  `FOR ALL TO projector_{scope}` would make `mode: permissive` break every write — *"the single most
  likely way this chunk ships a green test over a broken configuration."*
- **PROP-20260818-010343 §6, consequence 2**: backfills run as `projector_{scope}`, **never as the
  owner**. The owner's `UPDATE 0` is a *correct refusal*, not a hole, and it is what forces a
  migration that must write an RLS-bearing table to name the role allowed to write it (gate G-6).
- **`dba`**: column privileges are subtractive too and have **no mode of their own**. A permissive
  `USING (true)` policy combined with an enforcing `GRANT SELECT (cols)` still breaks every read
  naming an ungranted column, and breaks **every `SELECT *`** outright.

## Decision

**`mode:` gates the whole per-table subtractive surface**, as an emitter parameter producing two
artifacts off one emitter:

| statement | `permissive` | `enforcing` |
|---|---|---|
| persona read predicate | `USING (true)` | the `ScopeMembership` `EXISTS`, member type a literal |
| persona `SELECT` grant | table-level (every column) | per-column `GRANT SELECT (…)` |
| the OWNER's `FOR ALL` policy | **present** | **absent** |
| `projector_{scope}`'s `FOR ALL` policy | present | present |
| `ENABLE` + `FORCE ROW LEVEL SECURITY` | present | present |

Both lens findings then hold at once. Permissive withholds nothing from anybody who reads or writes
today; enforcing makes the owner's refusal a **tested behaviour** rather than a note.

**Database-level statements are excluded from both artifacts.** `REVOKE CONNECT, TEMPORARY … FROM
PUBLIC` is subtractive at *database* level, is not expressible under a per-table `mode:`, and would
survive `reset_schema` into every later suite sharing CI's database — poisoning it and producing a
silent failure somewhere else, attributed to something else. They are cutover-only artifacts with
their own gating (gate G-7). An offline assertion in `crates/infrastructure/tests/rls_matrix.rs`
fails if either artifact ever contains one.

**Neither artifact enters `migrations/`.** A codegen test
(`tools/codegen-rs/src/tests.rs`, `security_ddl_fence`) refuses `ROW LEVEL SECURITY`, `CREATE POLICY`
and `CREATE ROLE` anywhere in the deployed chain, and is written as the conditional that converts at
the local-acceptance walk ([#556](https://github.com/TheCaptainCompany/captain-food/issues/556)) with
no edit: *either* no `migrations/*_security.sql` exists, *or* its body is byte-identical to a
generated artifact.

## Consequences

**Intended.**

1. `FORCE` becomes **observable** exactly where it matters. With an owner policy in force, dropping
   `FORCE` changes nothing for anybody: the owner is permitted either way, and a non-owner persona is
   already enforced by `ENABLE` alone. Under enforcing, dropping `FORCE` flips the owner from 0 rows
   to every row — *measured on PG 16.13: 4 rows and `UPDATE 4`, against 0 and `UPDATE 0`*. Without
   this decision the `FORCE` mutation is green and the chunk teaches that `FORCE` is unnecessary.
2. The tightening step's rollback exists **in bytes, in every CI run**: it is the permissive emission
   of the same table, off the same emitter, regenerated deterministically rather than hand-authored
   under incident pressure. That is what #637 has no other answer for.
3. `permissive == every row, for every reader and writer that exists today` is asserted first, on the
   grounds that everyone remembers to test enforcing.

**Accepted costs.**

4. Two artifacts instead of one. The drift gate covers both; the `security_ddl_fence` conditional
   accepts either as the cutover migration, because permissive is what ships first.
5. Moving a table from `permissive` to `enforcing` **withdraws the owner's write policy** in the same
   step as it narrows rows and columns. That is one revertible step per table, which is the shape
   #637 asks for — but it means a backfill written against the permissive artifact stops working at
   the flip unless it names `projector_{scope}`. Gate G-6 is what makes that loud.

**Not decided here.** The DSL `mode:` key. It lands with the first table that actually ships
enforcing; until then the mode is an emitter parameter and no `specs/**` source file changed.

## Two measurements that change the NEXT chunk, recorded because they were not derivable

- **Inheritance for RLS purposes is a PER-GRANT property, not a role attribute.** *Measured, PG
  16.13*: a login role granted both `customer_role` and `restaurant_role` reads the **union** of both
  personas' rows with no `SET ROLE`. `ALTER ROLE … NOINHERIT` issued **after** the grant is a **no-op**
  — `pg_auth_members.inherit_option` is fixed at `GRANT` time. So PROP-20260818-010343 §5's wall (an
  app connects as its own login role and issues `SET LOCAL ROLE <persona_role>`) only exists if
  artifact **A-1 emits `GRANT <persona_role> TO <app_login_role> WITH INHERIT FALSE` explicitly**.
  Written the obvious way, the emitter hands every app the union of every persona granted to it, with
  nothing on the page to notice. Kept as a permanent test
  (`an_inherited_persona_grant_hands_one_login_role_the_union`).
- **`has_table_privilege(role, table, 'SELECT')` is FALSE for a role holding only column grants.**
  Under enforcing mode that is every persona, so a privilege gate written the obvious way reports
  *"no persona can read anything"* over a perfectly correct artifact. Use
  `has_any_column_privilege`. Cost here: one red run.

## Consulted

This ADR records a decision taken inside a dispatched chunk, from findings the mob had already
banked at briefing; no new consultation was opened.

- **`farley`** — the owner is today's writer identity; `mode: permissive` must not break writes.
  Carried, and it is the reason the owner policy exists in permissive at all.
- **`dba`** — column privileges are subtractive and have no mode; assertion 5 exists in enforcing
  mode only. Carried.
- **`beck`** — the load-bearing assertion is `permissive == every row`, asserted first. Carried.
