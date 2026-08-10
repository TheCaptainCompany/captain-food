# ADR-20260810-225036 — A fold the database rejects is classified, and halting on it ships gated

- **Status**: Accepted (the classification and the gate are realized; the default flip is NOT decided here)
- **Date**: 2026-08-10
- **Issue**: [#474 "`make rust` runs no workspace tests at all, and DB-gated tests skip silently — \"local gates green\" is a false signal"](https://github.com/TheCaptainCompany/captain-food/issues/474)
- **Follows**: [#451](https://github.com/TheCaptainCompany/captain-food/issues/451), [#230](https://github.com/TheCaptainCompany/captain-food/issues/230), [ADR-20260809-160000](ADR-20260809-160000-read-authorization-lands-ported-from-152.md)

## Context

The projection drain loop treats a failed fold as a poison record: log it, skip it, advance the
checkpoint, keep the group live. That was a deliberate liveness decision — before it, one bad event
re-failed every tick and halted **all** projection, and a legacy payload hitting a panicking
accessor froze the read models on every boot.

[#451](https://github.com/TheCaptainCompany/captain-food/issues/451) showed the cost of applying it
to *every* failure. `Cart.total_amount_cents` and `Cart.currency` were `NOT NULL` with no `DEFAULT`
while `cart_store::upsert` never listed them, so **every** Cart fold failed `23502`. The loop
skipped each one and advanced, and the projector reported a clean, caught-up drain over a read model
that was permanently empty. `placeOrder` was dead. The lie was not the skip — it was the
**checkpoint**, which is the system's own claim about what it has processed.

The loop could not have behaved otherwise: `apply_record` returned `DomainError`, and every path
built `DomainError::Repository`. A malformed payload, a panicking accessor and a constraint
violation were literally the same value.

## Decision

**1. Classify the fault where it is constructed.** `FoldFault::{PayloadShape, Database}`
(`crates/infrastructure/src/projection/worker.rs`). An enum, not a predicate over the error string:
the class is a fact known at the failure site, so the compiler makes every new site declare it and
no reword can silently reclassify. A PANIC classifies as `PayloadShape` — the observed case is a
legacy payload, and halting on it would restore exactly the boot-refold wedge `catch_unwind` exists
to prevent.

**2. `PayloadShape` still skips and advances.** It is genuinely per-record: the next event is
unaffected. [#230](https://github.com/TheCaptainCompany/captain-food/issues/230)'s behaviour is
unchanged, deliberately.

**3. `Database` gets a policy, and the policy ships gated.** `DbFaultPolicy::Skip` is the **default**
and reproduces today's behaviour exactly, so this change lands **inert on every deployed path**.
`DbFaultPolicy::Halt` rolls the batch back and returns the error, leaving the checkpoint exactly
where it was; the group retries the same slice next tick and the failure stays in front of us.
Nothing is lost by halting: the events are still in `domain_events`, behind an unmoved checkpoint.

**4. Flipping the default is a separate decision, not made here.** Halting has a real cost of its
own — a genuinely poisoned row wedges its group until an operator intervenes, taking a read model
down for every tenant. That trade-off deserves its own arbitration with the gated form smoked first
(CLAUDE.md gate-then-stabilize). `the_default_policy_still_advances_past_a_rejected_write_todays_behaviour`
pins the current arm so the flip is a visible deletion in a diff rather than a drift.

## Alternatives considered

- **Just make `Halt` the behaviour.** Rejected: it silently reverses a recorded liveness decision on
  a critical path in a change whose subject is test honesty, and it re-introduces the wedge #230
  removed.
- **Leave the loop alone; rely on the new `make validate` writer/schema rule.** Rejected as
  sufficient: that rule catches the `NOT NULL`-without-`DEFAULT` shape, which is one way to make the
  database reject a write. Cast errors, missing columns from a partial migration and check
  constraints all produce the same silent-advance lie and no static rule sees them.
- **Sniff the error string for a SQLSTATE.** Rejected: level 5 of the enforcement hierarchy where
  level 2 is available, and it would break the moment a message is reworded.
- **A quarantine table for rejected events.** A better end state than either policy, and out of
  scope here; it needs its own design (replay semantics, operator surface). Noted as follow-up.

## Consequences

**Positive.** The loop can no longer claim progress it did not make, once the policy is on. The
distinction is compiler-enforced, so a new failure site cannot inherit the wrong class by accident.
Three tests pin the classification rather than only the halt, and they manufacture the #451
condition themselves (`ALTER TABLE … DROP DEFAULT`) instead of depending on a planted defect — a
test that only fails against a plant dies with the plant.

**Negative.** Two behaviours now exist where there was one, and until the default flips, production
still advances past rejected writes — the honest state, written down rather than assumed away.

**Follow-up.** (a) Decide the default flip, with the gated form smoked. (b) Consider the quarantine
table, which would let `Halt` be safe unconditionally. (c) `revoke_role`'s named-skip special case
(ADR-20260809-160000 addendum) is a `Database` fault under this classification and would be covered
by the flip — worth revisiting there when it happens.
