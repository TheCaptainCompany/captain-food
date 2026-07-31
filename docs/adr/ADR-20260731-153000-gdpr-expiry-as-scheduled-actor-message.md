# ADR-20260731-153000 — GDPR data expiry is a SCHEDULED actor message; Order expiration is the first reminder use case

**Status**: Accepted (product-owner decision, in-session 2026-07-31) — decides the TRIGGER
mechanism only; the erasure ACTION remains the open decision C
([PROP-20260726-170000 D3](../proposals/PROP-20260726-170000-event-log-integrity-evolution-and-erasure.md),
crypto-shredding recommended, gates
[#194 "GDPR erasure"](https://github.com/TheCaptainCompany/captain-food/issues/194))
**Context**: [ADR-20260731-120825](ADR-20260731-120825-actor-messages-typed-inside-the-actor.md)
(reminders deferred "until the first use case" — this IS the first use case),
[ADR-20260731-150500](ADR-20260731-150500-reminders-reschedule-in-place.md) (reschedule in place),
[#242](https://github.com/TheCaptainCompany/captain-food/issues/242) Runtime D

## Decision

> "It could be useful for the GDPR compliance to schedule the expiration of the order."

1. **Data-retention expiry is triggered by a SCHEDULED message on the owning actor** — not by a
   table-scan sweep. When an Order reaches a terminal state, the Order actor declares its own
   `ExpireOrder` reminder: kind `MESSAGE`, identity `UUIDv5(orderId, "expire")`,
   `scheduled_at = terminal time + the retention window`. The promotion pass delivers it when
   due; the delivery performs the expiry through the ordinary write path, leaving the ordinary
   audit trail (an `OrderExpired` fact — the moment expiry became true is itself a fact).
2. **Why per-actor scheduling beats a sweep**: the schedule is DECLARED at the moment the
   retention clock starts, visible per order on the supervision surface ("when does this
   expire?" is a row, not a query over a policy), idempotent (one pending expiry per order, the
   deterministic identity guarantees it), and self-correcting — a retention-policy change
   re-declares the reminder and the ADR-150500 reschedule postpones/advances the SAME row in
   place. A sweep answers none of those without new machinery.
3. **This opens the reminders build** (Runtime D): the promotion pass, the per-actor typed
   message declaration (`Order.messages: ExpireOrder`), the `receives`-coverage validation and
   the `Rescheduled` insert outcome now have a concrete pilot. The `CheckPreparationDelay`
   illustration from the proposal stays an illustration.

## Deliberately left open (not decided here)

- **The erasure action** — what `ExpireOrder`'s handler DOES: crypto-shredding vs payload
  rewrite vs deletion is decision C, and the handler is a stub until it lands.
- **The retention window(s)** — a legal/product input, per DATA CATEGORY, not a single number:
  French commercial law retains accounting records ~10 years, so an order's FINANCIAL facts
  outlive its PERSONAL data (delivery address, phone, conversation) — "expire the order" will in
  practice mean "anonymize the personal categories at window A; the financial skeleton follows
  at window B". The windows land in the referential/config layer with the decision, never
  hard-coded in the handler.
- **Which other aggregates get expiry schedules** — Customer (inactive-account erasure),
  Conversation, DeliveryJob follow the same mechanism once Order proves it.
- **`domain_events.expired_at`** — the dormant column (PROP-20260726-170000 D4 "implement or
  delete") is the natural landing zone for this mechanism's effect on the log; confirmed with
  decision C, not before.

## Consequences

- Runtime D's reminders slice targets `ExpireOrder` as its pilot: schedule-at-terminal,
  promotion, reschedule-on-policy-change, and a STUB handler that emits nothing until decision C
  — the machinery is provable end to end (scheduled → promoted → delivered → verdict) without
  pre-empting the erasure strategy.
- The DECISIONS.md queue gains the cross-reference: deciding C now also unblocks the pilot's
  handler, raising its leverage.
