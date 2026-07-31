# ADR-20260731-160000 — Order erasure = projection tombstone, then technical stream deletion, owned by a process manager

**Status**: Accepted (product-owner decision, in-session 2026-07-31; AMENDED same day — the
phase-2 mechanism and the receipt's shape, §5/§6 below) — DECIDES decision C
([PROP-20260726-170000 D3](../proposals/PROP-20260726-170000-event-log-integrity-evolution-and-erasure.md),
[#194 "GDPR erasure"](https://github.com/TheCaptainCompany/captain-food/issues/194)) **for the
Order scope**, diverging deliberately from the proposal's crypto-shredding recommendation.
Customer-account-level erasure remains open (see "Left open").
**Context**: [ADR-20260731-153000](ADR-20260731-153000-gdpr-expiry-as-scheduled-actor-message.md)
(the scheduled fact `OrderExpired` is the trigger), ADR-0040 (projections),
[#242](https://github.com/TheCaptainCompany/captain-food/issues/242) Runtime D

## Decision

> "The order expired = an order DELETION from the system. We tombstone all the data related to
> the order thanks to the projections, and later a technical worker deletes the stream. A
> business process manager should ensure this deletion process."

1. **`OrderExpired` means deletion, not redaction.** Recording the fact starts the order's exit
   from the system — not a field-level scrub, not key destruction. The chosen mechanism keeps
   the event store's append-only property intact for the stream's whole LIFE; what ends is the
   stream itself.
2. **Phase 1 — tombstone through the projections.** The projectors fold `OrderExpired` like any
   other fact: every read model holding the order's data (`ordertracking`,
   `order_conversation`, delivery rows, satisfaction, refunds…) DELETES/tombstones its rows for
   that order in its ordinary fold. No side-channel scrubber: the same mechanism that
   materialized the data un-materializes it, ordered by the same checkpoints.
3. **Phase 2 — a TECHNICAL worker deletes the streams.** Later (grace period; window open
   below), an infrastructure-level worker — not a domain handler; deleting log rows is not a
   business decision, the business decision was `OrderExpired` — physically deletes the order's
   streams from `domain_events`. Natural consequence: a projection REBUILD after phase 2 simply
   never sees the order — rebuilds stay consistent with erasure by construction.
4. **A business process manager owns the journey.** An `OrderErasureProcess` PM reacts to
   `OrderExpired` and is accountable for the process reaching its end: verify the tombstone
   fold happened (checkpoints past the fact), wait out the grace period, instruct the technical
   deletion, and record completion. Erasure that nobody supervises is erasure that silently
   half-happens; the PM makes "is this order really gone?" an answerable, monitored question —
   same pattern as every other saga, visible on the supervision surface.

5. **AMENDMENT — the phase-2 instruction is a TECHNICAL TOMBSTONE EVENT** (product-owner
   refinement, 2026-07-31): once the PM has verified every projection checkpoint passed the
   `OrderExpired` fact and the grace period is out, it appends an EventStore-style technical
   tombstone event to the stream (envelope-level, not events.yaml vocabulary — the business fact
   was `OrderExpired`; this is plumbing). A TECHNICAL worker consumes tombstones and physically
   deletes the stream's rows from `domain_events` AND `domain_stream`. The instruction being an
   appended event keeps the whole journey in the log until the very last moment (auditable,
   replayable, supervisable) — the deletion itself is the only non-log operation, performed by
   infrastructure on an explicit, durable instruction.
6. **AMENDMENT — the receipt is the BUSINESS event `OrderDeleted`** (same refinement): the PM
   records completion by emitting `OrderDeleted` on its own erasure-ledger stream — never on the
   (about-to-be / already deleted) order streams — carrying the PSEUDONYMOUS domain references
   the accountability duty needs: `customerId`, `restaurantId`, the rider id(s), the policy
   under which erasure ran, and when. This sharpens the earlier "no personal data" wording:
   the receipt carries identifier REFERENCES (whose order was erased must remain answerable),
   never the erased personal payloads (address, phone, conversation content — those are gone).
   The PM's own trace/stamp of "deleted, thanks to tombstone <id>" lives in the same record, so
   "is this order really gone?" has one durable, queryable answer that outlives the streams.

## What this changes architecturally

- **Projectors gain deletion semantics** — the fold's `Option<Row>` gains "existing row +
  event ⇒ None = DELETE" (today `None` only means "no change"). A spec-visible per-read-model
  concern: every projection fed by Order-related streams must declare its `OrderExpired`
  behavior, and the validator can enforce that none forgets.
- **The related-stream set must be enumerated, not assumed.** An order's data spans
  `Order-{id}`, `Conversation-{id}`, its `DeliveryJob` stream(s), its `Reclamation` stream(s),
  and `Payment-{intentId}` streams referencing it. Which of these die WITH the order (a
  Conversation surely; a Payment stream may serve financial retention independently) is part of
  the PM's definition — explicit list, spec-declared.
- **Erasure must remain provable after the proof's subject is gone** (GDPR accountability): the
  PM's completion record — a minimal erasure receipt (when, what scope, under which policy),
  carrying no personal data — outlives the deleted streams. Where it lives (its own ledger
  stream vs a referential table) lands with the PM design.

## Left open (named, not decided)

- **Per-phase windows**: when `OrderExpired` fires (phase 1) and how long until stream deletion
  (phase 2) — legal/product inputs per data category. The two-phase shape is exactly what the
  French accounting-retention constraint needs: personal data can tombstone EARLY while the
  streams (financial facts) survive to the ~10-year horizon — IF the windows are set that way;
  alternatively the financial skeleton is exported to a bookkeeping store before phase 2. One
  of these must be chosen before phase 2 ships.
- **Customer-account-level erasure** (the account, identity, files registry, Supabase side) —
  decision C's remaining scope; the mechanism chosen here may generalize to it, not assumed.
- **The technical worker's contract** (batching, verification, its own observability) — with
  the PM design, in the realizing proposal.
