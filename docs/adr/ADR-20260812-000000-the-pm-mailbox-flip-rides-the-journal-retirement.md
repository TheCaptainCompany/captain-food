# ADR-20260812-000000 — The PM_MAILBOX_DELIVERY flip rides the journal retirement, and the gate goes with it

- **Status**: Accepted
- **Date**: 2026-08-12
- **Origin**: product-owner direction, 2026-08-11 — *"Remove inbound events and command journal from
  the dsl, the only tables that must remain is inbound messages"*
- **Realizes**: [#242 "Write path becomes an actor mailbox"](https://github.com/TheCaptainCompany/captain-food/issues/242) Runtime D
- **Amends**: [ADR-20260801-023000](ADR-20260801-023000-a2-realizes-as-prepare-phase-single-delivery.md)
  (which shipped PM mailbox delivery GATED) and
  [ADR-20260803-104819](20260803-104819-db-persisted-pm-mailbox-delivery-posture.md)
  (which moved that gate into a `RuntimePosture` row)

## Decision

**`PM_MAILBOX_DELIVERY` flips ON and the gate is DELETED in the same change that drops
`command_journal`.** PM mailbox delivery is now unconditional: the three PM mutations
(`placeOrder` / `approveRefund` / `denyRefund`) enqueue on their actor's mailbox lane like every
other command, the Payment lane B2-chains recorded Stripe facts in the completion transaction, and
the saga runner carries no Stripe-fact triggers. There is no posture to read and no arm to pick.

The `RuntimePosture` table and its fail-closed read STAY. The mechanism (#318) is right and the
next process-wide posture will need exactly it; it simply has no tenant today, which the table's
description and the module's doc now say out loud.

## Why the flip is not a separate decision this time

Gate-then-stabilize says flipping a default is its own recorded decision **after the gated form has
been smoked**. This ADR *is* that separate decision — it is only landing in the same commit because
the two facts that normally make sequencing valuable are both absent right now:

- **Production is down.** There is no traffic to break and no in-flight client poll to strand.
- **The event log is empty by decision** (start-clean,
  [ADR-20260807-002705](ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md) D6). There is
  nothing to migrate, and nothing a wrong flip could corrupt.

A staging smoke of the gated form has nothing to smoke against: an empty log produces no PM
deliveries to observe. Waiting would buy a rehearsal that cannot be performed, at the cost of
keeping a second write-path journal alive through the cutover — which is the thing the product
owner asked to remove. **This window is the cheapest this flip will ever be, and it closes the
moment the first real order lands.**

## Why the gate is deleted rather than defaulted ON

Because its OFF position would be a lie. The gate's OFF arm *was* `command_journal` — the
journal+spawn path in the generated PM resolvers. With that table dropped, OFF could no longer mean
what it says: the mutations would still enqueue on the mailbox (their only remaining arm), while the
Payment lane stopped chaining and the saga runner's Stripe-fact triggers came back — the exact
"silent paid-order stall" the posture row's own description warns about. A lever whose one position
is a money stall is worse than no lever (CLAUDE.md: *a control that renders but does nothing is
worse than no control*). Final vision first
([ADR-20260808-235113](ADR-20260808-235113-final-vision-first-no-intermediate-steps.md)): the
finished shape is one door, so build one door.

## Consequences

- The composition roots (monolith, subgraph bins, PM bins, standalone adapter fleets) no longer read
  a posture at startup. The monolith's *"refuse to boot on a transient posture read"* exception to
  ADR-0043 is withdrawn with it — there is no money posture left to guess.
- `filter_lanes_by_posture` and the UNPROVABLE money-lane refusal are gone: a fleet spawns the lane
  set it was handed, because no peer can hold a different value.
- The startup Stripe-fact backfill (`backfill_stripe_facts_to_pm_lanes`) now runs
  **unconditionally**. It was flip-transition machinery; it stays because *this* deploy is that
  transition — a monolith restarting out of gate-off must not strand a fact its saga runner accepted
  and never reacted to. On an empty log it enqueues nothing and costs one query.
- Rollback is a `git revert` + the down-migration you would have to write; there is no runtime lever.
  Acceptable only because of the window named above.
- **A rolled-back binary reports itself HEALTHY while checkout is dead.** The readiness probe
  (`crates/server/src/lib.rs`, `applied >= REQUIRED_SCHEMA_VERSION`) is ONE-DIRECTIONAL: schema
  *behind* the binary ⇒ `SCHEMA_BEHIND` ⇒ 503, but schema *ahead* ⇒ `HEALTHY` ⇒ **200 OK**. Reverting
  the image without also reverting the migration therefore takes traffic immediately: the old code
  reads `Unprovable` for the deleted posture row, takes the conservative arm — the legacy one — and
  fails on `relation "command_journal" does not exist` **inside the mutation**, not at the probe.
  Net: a `/health` that is green over a checkout that cannot complete. Tolerable only in this window
  (production down, log empty), and it makes the ORDER of any rollback non-negotiable: revert the
  migration first, or not at all. It is also the sharper reason the gate had to be deleted rather
  than defaulted ON — a lever whose OFF arm needs a dropped table is unusable precisely when someone
  would reach for it.

## The residual hazard: prose that outlives the mechanism, and why no gate catches it

Deleting a guard deletes its code, never its *description*. Three independent lens reviews and the
product owner read this branch's full diff and none of them saw that the bin emitter still promised
the guard: `pm-place-order`/`pm-refund` shipped *"the fleet reads the money posture itself and
refuses the lane when it is unprovable"*, and all fifteen `actor-*` bins shipped *"posture-gated
money lanes"* — on lines no hunk touched, in the artifact an operator opens FIRST when a money PM
pod is stuck. A false claim of enforcement on the money path is the same defect class this ADR
retires, one layer up. It is corrected here (the emitted text now borrows the wording
`standalone.rs` already carries: the fleet drains exactly the lane set it is handed, and there is no
value a peer could hold differently), together with the sibling that hid the same way — the
five-line doc comment of the deleted `pm_mailboxes` field, which Rust silently re-attached to the
`only` field beside it, so `ProcessManagerRunner` documented a gate flip on a field that selects a
PM.

**No gate is reachable for this, and building one would be the mistake.** The only mechanism that
would catch it is a source-text scanner over comment prose — precisely the class
[ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md) and
[#329](https://github.com/TheCaptainCompany/captain-food/issues/329) rule out (seven review rounds
hardening a scanner whose every gap was found by a reviewer, not by the scanner); the type system
cannot reach a comment, and a keyword denylist would go stale exactly as the comments did. The
defence is procedural and belongs with the deletion: **when a mechanism is deleted, grep its
vocabulary — not just its identifiers — across the emitter AND the generated output, because a
stale sentence lives on an unchanged line and is invisible to every diff-based review.**
