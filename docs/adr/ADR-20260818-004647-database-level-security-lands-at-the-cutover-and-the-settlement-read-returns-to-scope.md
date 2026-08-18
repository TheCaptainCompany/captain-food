# ADR-20260818-004647 — Database-level security lands at the CloudNativePG cutover, on the empty database; and the settlement read comes back into scope

**Status**: Accepted · **Date**: 2026-08-18 ·
**Decider**: the **FOUNDER / Tech CEO**, ruling on the night of 2026-08-17/18 after the whole roster
was consulted (`Consulted:` block below, ADR-20260812-143619) ·
**Register**: [DECISIONS §46](../proposals/DECISIONS.md) **RLS-SEQ**, and
[DECISIONS §32](../proposals/DECISIONS.md) **STO-9**, which returns to scope ·
**Relates**: [ADR-20260807-002705](ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md) (self-hosted
Postgres — CloudNativePG on OVH MKS) ·
[PROP-20260811-093000](../proposals/PROP-20260811-093000-storage-boundaries-and-least-privilege-database-users.md)
§6.1/§6.3 (the role model, and RLS on `domain_events` gated and benchmarked) ·
[PROP-20260725-185140](../proposals/PROP-20260725-185140-read-side-per-instance-authorization.md) §3.4
(the `ScopeMembership` index) ·
[ADR-20260815-030206](ADR-20260815-030206-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side.md)
(a PM never reads the read side) ·
[ADR-20260818-004646](ADR-20260818-004646-no-business-identifier-lives-in-the-identity-provider.md)
(the same night's identity ruling, which sequences before this one) ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

## Status

Accepted.

## The two rulings

**Ruling A — WHEN.** Database-level security (row-level security on the read models) lands **at the
CloudNativePG cutover, on the empty database**. Chosen from a form whose alternatives were
*"`OrderTracking` only, behind a flag, after the walk"* and *"now, as written"*.

**Ruling B — SCOPE.** The `OrderTracking` settlement read comes **back into scope**:

> **"now that we know how to deal with security at the database level we can integrate it now."**

It had been explicitly scoped out of the founder's own earlier prompt. It is back because the two
questions turn out to be the same question — see reason 1 below.

## Why the cutover, and why on an empty database

Enabling a row policy on a populated table is a lock event on live data, and a policy that has never
been exercised against an empty schema is a policy nobody has read. At the cutover the read
databases are **created** — the policy is part of the schema's birth, applied by the `migrator`
credential that owns the schemas (PROP-20260811-093000 §6.1), with no data to lock and no rollback
window to negotiate. Production is suspended as a recorded state (DECISIONS §45 **PROD-1**) and
there is no real phone-verified end user (**Q-L3 = no**, ADR-20260812-214021): the databases the
cutover creates are genuinely empty, and this is the only time that will be true.

**This does not overturn gate-then-stabilize.** PROP-20260811-093000 §6.3 — RLS on `domain_events`
ships behind a flag, benchmarked at ≥ 200 appends/s, default flip a separate one-line ADR — is
**untouched**. That row is about a policy on the hottest table in the system, evaluated on every
append. This ruling is about **read-model tables at the moment they are created**, which is a
different act with a different blast radius.

## The drafted table set does not survive — four measured reasons

The four tables named in the drafted set were `OrderTracking`, `View_DeliveryJob`,
`CustomerCreditBalance` and `OrderConversation`. Three of them fail, each for its own reason,
verified against the tree at `b77c487`.

### 1. `OrderTracking` breaks the settlement read — and it breaks it SILENTLY

`SettlementHooks::load_order` (`crates/application/src/process_managers/payment_settlement.rs:83-84`)
reads the order row on **all four settlement legs**, immediately before every Stripe capture and
every release:

```rust
let Some(o) = self.orders.by_id(order_id, &crate::queries::ReadScope::System).await? else {
    return Ok(HookOutcome::Skip(format!(
        "order {} is not in the OrderTracking read model — nothing to settle", order_id.0)));
};
```

`ReadScope::System` is the **deliberate absence of a principal** — there is no customer, no
restaurant and no membership behind a settlement leg. A row policy keyed on `ScopeMembership` has
nothing for it to satisfy.

And RLS **filters rows, it does not raise**. So the policy does not produce the STO-9 error the
register anticipated; it produces **zero rows**, which this code path already has a meaning for:
`HookOutcome::Skip`, *"nothing to settle"*. The leg completes **successfully**, reports a benign
skip, and the capture never happens. That is the worst-failure class CLAUDE.md names — food
delivered, money never collected — arriving as a **green log line** instead of a retry storm. A
policy on this table is strictly worse than the physical wall STO-9 already describes, because the
wall at least errors.

This is why ruling B exists: any database-level guard on `OrderTracking` must first answer what the
settlement leg reads with. The two questions cannot be sequenced apart.

### 2. `View_DeliveryJob` is a VIEW, and Postgres does not police views

`specs/generated/views.generated.sql:6` — `CREATE OR REPLACE VIEW View_DeliveryJob AS SELECT … FROM
domain_events`. The repository convention is explicit (CLAUDE.md): **`View_*` = a SQL VIEW,
unprefixed = a TABLE**.

- `CREATE POLICY` names a **table**. There is no policy object for a view.
- `ALTER TABLE … FORCE ROW LEVEL SECURITY` cannot target a view either.
- The two view options that *sound* relevant are different mechanisms: `security_barrier` is an
  **optimizer fence** that stops a cheap, leaky user function from being pushed below the view's own
  qualifiers; `security_invoker` (PG 15+) decides **whose privileges** the underlying tables are
  checked against. Neither one is RLS, and `security_barrier` in particular is not
  `security_invoker`.
- The rows come from `domain_events`. Any row filtering for this surface would therefore have to be
  a policy **on the event log** — the exact object PROP-20260811-093000 §6.3 gates behind a flag and
  a benchmark, and the one the read side is forbidden to query directly in the first place.

The consequence has a persona attached (`legal-specialist`): the **rider's job board** is served by
this view (`rider_id, status` index, `projection_views.yaml:93-95`), and rider membership is the
only membership V0 ever REVOKES. So "a rider sees only their own jobs" is precisely the guarantee
the database cannot deliver at this surface, and it must keep being delivered in the application —
recorded, not assumed away.

### 3. `CustomerCreditBalance` has no matching `ScopeType`

`specs/common/scalars.yaml:721-729` declares `ScopeType` with exactly two members: `ORDER` and
`RESTAURANT`. `CustomerCreditBalance` is **one row per customer**
(`specs/database/tables/projection_tables.yaml:972-994`). There is no scope type that names a
customer instance, so a membership-predicate policy on this table cannot be written at all — not
"is awkward", cannot be spelled.

Closing that means widening the scalar, which is a `specs/**` change nobody has approved and whose
blast radius this run did not measure: the generated scalar, the `ScopeMembership` projector's fold
(`projection_tables.yaml:1012-1017`), and the guard's vocabulary all read it. It is a **projection**
enum rather than a stored event shape, so it is rebuildable — but it is still a decision, not a
detail.

### 4. `FORCE` as drafted leaves the projector with no policy slot, and makes rebuilds order-dependent

Two distinct defects in the same clause.

**(a) The writer is locked out.** In the read databases the tables are owned by `migrator` and
written by the per-scope `projector_{scope}` roles (PROP-20260811-093000 §6.1, the role table:
*"`projector_{scope}` (×8) … INSERT/UPDATE/DELETE + its `projection_checkpoint` + its
`projection_watermark` heartbeat"*). A non-owner role is already subject to plain `ENABLE ROW LEVEL
SECURITY`, and RLS is **default-deny**: with no policy naming the projector, every INSERT and UPDATE
it makes is refused. `FORCE` extends the same treatment to the owner. The drafted set writes reader
predicates and gives the writer no slot at all — the projection stops, and it stops on the first
event after cutover.

**(b) A rebuild becomes order-dependent.** The natural predicate resolves against `ScopeMembership`
— a **separate** projection (`projector: app`, `aggregate: Order`,
`replicated: read-databases`, `projection_tables.yaml:1012-1017`) with its own checkpoint. Inherited
as a `WITH CHECK` on the writer, that predicate makes rebuilding a guarded read model depend on
`ScopeMembership` already being caught up in the same database: replay them in the wrong order and
every row fails its own check. A read model whose rebuild depends on another read model's progress
is **no longer a disposable projection** (`young`), which is the property the whole read side is
built on.

## What survives, and where it starts: `OrderConversation`

`OrderConversation` is the right first table, on three independent counts:

- It is a **TABLE**, not a view (`projection_tables.yaml:873`, `projector: app`).
- Its identity **is** an order — a conversation's id is its `orderId`
  (ADR-20260725-015921) — so it maps onto `ScopeType.ORDER` and `ScopeMembership` with no new
  vocabulary, no scalar widening and no invented predicate.
- It is the **highest-value** surface to guard: its own declaration records that the free text
  *"will incidentally carry allergy statements (Art. 9 special category)"*
  (`projection_tables.yaml:879-880`), and `orderConversation` is one of the two CUSTOMER-reachable
  reads that take a caller-supplied id with no ownership check (DECISIONS §45
  **IDOR-DEADLINE-GAP**).

So: **start at `OrderConversation`, not at `OrderTracking`.** The remaining three are not "later
tables"; each has a prerequisite decision named above.

## Ruling B in the register: STO-9 is in scope, not answered

STO-9 (DECISIONS §32) stays **OPEN**, with its options (a)–(e) and the 2026-08-15 lean on **(e)**
unchanged. What changes is that it is **inside** the database-security work instead of scoped out of
it: the settlement read is now a precondition of any policy on `OrderTracking`, per reason 1.

One observation, recorded as an observation and **not** as a decision, because STO-9's answer is the
team's to make: option **(e)** — the process manager folds the aggregate streams in-process through
the `EventStore` port it already holds — is the only option that makes the RLS question **disappear**
for this leg rather than answer it, because `domain_events` already sits inside `captain_write` and
the PM stops reading a read model at all. That is also what
[ADR-20260815-030206](ADR-20260815-030206-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side.md)
already requires of it.

## The three externally-authored ADRs are HELD, not deposited

`ADR-20260817-232744`, `ADR-20260817-232745` and `ADR-20260817-232746` were authored outside the
team on 2026-08-17. The founder directed that they are **held until corrected**; they are **not
deposited** in `docs/adr/`, and this records why rather than leaving three ids dangling.

Their load-bearing content is carried, corrected, by work that is in the repo:
[#635](https://github.com/TheCaptainCompany/captain-food/issues/635) (the refund commands are
decided by a process manager, so no aggregate-owned rule reaches them) and
[#636](https://github.com/TheCaptainCompany/captain-food/issues/636) (the declarative block must key
on the RECEIVING ACTOR and `$ref` `actors.yaml#/principals`), plus this ADR for the `access:` / RLS
half. The register rows **AUTHZ-LOCUS** and **AUTHZ-GRAMMAR** (DECISIONS §46) record what of theirs
was adopted and what was declined.

## Consulted (ADR-20260812-143619)

Thirteen lenses were asked before any answer was composed. One clause each; a lens with nothing to
say on these rulings is recorded as such, and **no lens output is legal advice or clearance**.

- **dba** — RLS is default-deny for non-owner roles, so the projector needs its own policy or the
  projection stops; and a predicate over a second projection turns replay into an ordered operation.
  Both defects are in reason 4.
- **legal-specialist** — the **money hazard** (a policy on `OrderTracking` converts a pre-capture
  read into a silent skip) and the **rider-own-data** problem (the rider's own-jobs guarantee sits
  on a VIEW, where RLS cannot reach). *A grade, not clearance.*
- **farley** — consulted on the rollout surface: the cutover is the only moment the tables are empty
  and the schema is being applied anyway, so the change needs no window of its own; a policy added
  later needs one.
- **young** — a read model whose rebuild depends on another read model's checkpoint is not
  disposable any more; that is the cost of reason 4(b), and it is a doctrine cost, not an ops one.
- **vernon** — the settlement leg reads a PAYMENT fact out of an ORDER read model; the record layers
  on the seam design rather than superseding it, and STO-9 option (e) is the same argument he has
  already made about PM boundaries.
- **beck** — on the test shape: the negative test (the projector cannot write when its policy is
  absent) has to exist before the positive one, or the first green build is a build with a stopped
  projection.
- **architect** — filed [#635](https://github.com/TheCaptainCompany/captain-food/issues/635) and
  [#636](https://github.com/TheCaptainCompany/captain-food/issues/636), and fenced the `access:` /
  RLS half out of #636 as a different project with a different blast radius.
- **holub** — warn-only enforcement has already been rejected on the record for the configuration
  gate (ADR-20260729-010500); he reported a second prior rejection this run could not locate —
  `UNVERIFIED input`, his session finding stands as the record — and named the competing per-actor
  role model that sits behind the six-role draft.
- **evans** — the `requires.acting` grammar already exists and is validated, so the authorization
  vocabulary is not what is missing here; what is missing is a scope type for a customer-keyed table
  (reason 3).
- **graphql-architect** — nothing in this lens on the sequencing; the read surfaces do not change
  shape, and a policy is invisible to the schema.
- **observability-agent** — a silently-skipped settlement leg is the exact shape of a monitoring
  path that can only fire when a signal ARRIVES; whatever lands here needs a dead-man's-switch on
  the capture, not a counter on the failure.
- **ux-designer** — nothing in this lens: no customer-visible surface changes.
- **business-specialist** — nothing in this lens beyond the timing: the empty database is free, and
  every later window is paid for in a maintenance slot at peak-avoiding hours.
