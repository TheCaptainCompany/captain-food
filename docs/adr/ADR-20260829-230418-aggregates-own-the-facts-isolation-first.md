# ADR-20260829-230418 — Aggregates own the facts: the isolation subject is resolved FIRST

- **Status**: Accepted
- **Date**: 2026-08-29
- **Decider**: the **FOUNDER / Tech CEO**, after his direct question on process managers appending
  in place of the responsible actor. Directive, verbatim:

  > **"We need to resolve this isolation subject first to avoid any new bad development."**

- **Relates**: [ADR-20260816-040239](ADR-20260816-040239-deliver-is-a-lane-enqueue-not-a-foreign-stream-append.md)
  (the semantic ruling and the 13-step enumeration this plan executes) ·
  dispatch card [`docs/dispatch/598-birth-lane-flip-observability.md`](../dispatch/598-birth-lane-flip-observability.md)
  (§7/§9: the flip-ADR obligations) ·
  [#595 "The reclamation replacement birth writes Order-{id} with no transaction and no lane — a second unlaned birth site, reachable today"](https://github.com/TheCaptainCompany/captain-food/issues/595)
- **Register**: [AGGREGATES-OWN-THE-FACTS](../decisions/AGGREGATES-OWN-THE-FACTS.yaml)

## The property (evans — the DECLARED language, not a new coinage)

> **Aggregates own the facts: only the owning aggregate's lane worker appends to its stream; a
> process manager decides, never appends.**

This sentence is already the spec's, not this record's invention:
`specs/common/processmanager.yaml:7-9` — *"AGGREGATES OWN THE FACTS — a process manager never
appends to domain_events itself; it delivers events for the owning aggregate to record, or sends
commands the aggregate may reject"* — and ADR-20260816-040239's principle — *"being the birth
AUTHORITY licenses the DECISION, never the APPEND."* The code is what still contradicts it on
twelve of thirteen `deliver:` steps; this ADR sequences making the code stop lying, before any new
development builds on the lying shape.

## The plan

Executed in order; each chunk is its own dispatch, PR and record. Ordering is the founder's
"first": no new development that stages appends onto a foreign stream may land while this plan is
open.

- **C1 — the flip-run of the BUILT Order-birth route.** The routed `deliver: OrderPlaced → Order`
  is implemented and gated OFF (`ROUTE_ORDER_BIRTH_THROUGH_LANE`, `specs/ordering/configuration.yaml`).
  C1a produces the pre-flip **evidence** (a routed birth SEEN on the histogram by a test, the walk
  evidence, the paid-customer tracking guard). **The DEFAULT FLIP itself goes to the founder with
  the evidence attached — C1a never flips.** This is a standing recorded obligation, not caution:
  dispatch card 598 §7 already gates flipping the DEFAULT on a founder-visible one-line ADR citing
  the smoke assertions, the liveness series, the budget decision and the fleet-parity posture
  (§9 items 1-5 enumerate the evidence that ADR must carry).
- **C2 — [#595](https://github.com/TheCaptainCompany/captain-food/issues/595).** The reclamation
  replacement birth is the second unlaned birth site, reachable today, and it cannot use the
  staged-intent seam (the polling runner owns no delivery transaction) — it moves onto the mailbox.
- **C3 — the twelve remaining `deliver:` steps**, in TARGET-ACTOR groups from the
  ADR-20260816-040239 enumeration: **Payment ×7** (`PaymentIntentCreated`, `RefundOpened` ×4,
  `RefundApproved`, `RefundDenied`), **DeliveryJob ×4** (`DeliveryRequested`,
  `DeliveryDispatchFailed` ×3), **Cart ×1** (`CartCheckedOut`). One PR per group. Per-route gates
  are **GENERATED from the spec enumeration** — farley: this REFINES ADR-20260816-040239's "the
  remaining twelve behind the same flag" wording; one fused flag would make the twelve routes flip
  together, which is exactly the un-flippable blast radius the gate exists to avoid. Each route:
  gate ON → smoke → **the legacy arm is deleted in a SEPARATE change**, with beck's
  **golden payload-equality** (the laned append's payload byte-equal to the foreign-stream append it
  replaces) as the deletion precondition.
- **C4 — the final vision (compiler-first, ADR-20260803-234035).** The `EventStore` **save**
  capability leaves process-manager signatures: vernon — split the port so a PM can spell `load`
  but never `save`; PM state is table-based and no own-stream append exists, so nothing legitimate
  is lost. A foreign-stream append becomes **unspellable**, and every gate the compiler thereby
  subsumes is deleted (a correct outcome under the compiler-first directive).

## Sequencing rulings

- **The erasure PROPOSAL approval flow proceeds** ([PROP-20260829-150752](../proposals/PROP-20260829-150752-customer-erasure.md)
  awaiting founder approval) — paper produces no development, so the isolation-first directive does
  not pause it. **The erasure BUILD starts only after slice C1 has proved the pattern AND the
  founder has approved the proposal** (holub: the erasure engine legs ride the same
  mailbox/delivery seams this plan is straightening; building them on the pre-isolation shape is
  the "new bad development" the directive forbids).
- **Business timebox note**: the isolation work fills the counsel-wait on the erasure approval. If
  counsel returns fast, isolation must **not silently become the binding constraint** — that
  moment is reported to the founder as a sequencing question, not absorbed.
- **Legal guard-rail**: [PROP-20260808-142532](../proposals/PROP-20260808-142532-disappearance-terminal-states.md)'s
  tombstone / Art. 21 semantics are untouched by every chunk of this plan. No route move may key,
  reshape or re-time the GDPR retention/expiry schedule (the standing fence from
  ADR-20260816-040239 restated plan-wide).

## Vocabulary (evans)

**"Foreign-stream append" is THE term** for the defect this plan removes. **"Legacy" is permitted
only as the gated OFF path's temporal qualifier** ("the legacy arm" = the gated-OFF append that a
route deletion retires). **"Direct append" is retired**: its two living sites — the
`crates/infrastructure/src/mailbox/activation.rs:231` comment and
[PROP-20260811-150242](../proposals/PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md)
§"deliver:" — are rewritten to "foreign-stream append" **when next touched**, not by a sweep.
Historical records keep their vocabulary verbatim (ADR-20260812-143619).

## Consulted (ADR-20260812-143619 — one line per lens; 13 lenses, in-session 2026-08-29)

- **architect** — chunk plan C1→C2→C3→C4 as above; ranked the isolation band Urgent on the
  prioritised backlog under the BACKLOG.md foundations-first clause.
- **vernon** — the two-writers doctrine is the violation class; C4's port split (a PM spells `load`,
  never `save`) is the correct unspellability; PM state is table-based so no own-stream append
  exists to grandfather.
- **young** — confirmed the PM routes are decide-then-bypass today; the C1 flip is replay-safe and
  NOT a migration (payload, type, stream unchanged; the envelope change is recorded and never
  backfilled).
- **beck** — the laned Order route has been seen red-to-green, but the birth-lag EMISSION has never
  been observed by any test (the C1a closure); golden payload-equality is the deletion precondition
  discipline for every C3 legacy arm.
- **farley** — split the fused flips: per-route gates generated from the spec enumeration, refining
  ADR-20260816-040239's "same flag" wording; flip and delete are SEPARATE changes per route.
- **observability** — the birth-lag emitter is live but silent-while-OFF by design; the per-route
  histogram for C3 routes is a gap to declare with each route; the alert-route wiring gap stands
  founder-gated (`specs/observability.yaml:360-363`).
- **dba** — the routed birth is a net peak improvement (one aggregate per transaction on the
  heaviest saga leg); wants an `n_dead_tup` gauge on `inbound_messages` (churn from laned routes).
- **graphql-architect** — the paid-then-null handoff window is real once the flip lands: `order.byId`
  can honestly answer null between payment and the laned birth; the tracking surface must never
  read that as not-found (C1a's guard).
- **ux-designer** — the tracking guard is Absent-with-DispatchHandle, and the reassurance copy is
  the GAP(copy) riding #420; a paid customer never sees "introuvable" in the handoff window.
- **evans** — the DSL grammar is honest (`deliver … to:` was always a Tell); the #595 seam is
  grammar-invisible (a runner call, not a `deliver:` step); supplied the one property sentence and
  the vocabulary ruling above.
- **holub** — the isolation band runs as a WIP=1 lane; the erasure build needs only slice C1 proved
  (plus founder approval), not the whole plan.
- **business-specialist** — the timebox note above; isolation filling the counsel-wait is good
  sequencing only while it stays non-binding.
- **legal-specialist** — no legal exposure in the route moves themselves; the tombstone guard-rail
  above is the one standing condition. (No lens output is legal advice or clearance.)

## Consequences

- Every new dispatch touching PM/delivery seams is measured against the property sentence; a
  dispatch that would stage a foreign-stream append is a card defect.
- The plan's chunks carry `HOLD: human` where they touch the mailbox runtime, stored event
  envelopes or money paths (ADR-20260815-115220 as amended by ADR-20260815-134655) — C1a included.
- Rollback stays a config flip per route until that route's separate deletion change lands; after
  C4, the gate surface shrinks because the compiler owns the property.
