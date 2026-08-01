# ADR-20260801-010134 — The deletion window rides the REMINDER when the expiry is a business fact (Order pilot shape)

**Status**: Accepted (realizing decision, #272 D2, 2026-08-01) — REFINES how
[ADR-20260731-214500](ADR-20260731-214500-deletion-dsl-declarative-generic-engine.md) §3's two
trigger kinds apply to the Order pilot; COMPOSES
[ADR-20260731-153000](ADR-20260731-153000-gdpr-expiry-as-scheduled-actor-message.md) (the
scheduled fact) and [ADR-20260731-160000](ADR-20260731-160000-order-erasure-tombstone-then-stream-deletion.md)
(the journey).
**Context**: [PROP-20260731-195500](../proposals/PROP-20260731-195500-runtime-d-pm-mailboxes-and-reminders.md)
(rewritten to this shape in the realizing change),
[#272 "Runtime D"](https://github.com/TheCaptainCompany/captain-food/issues/272),
[PR #273 "Runtime D — PM mailboxes (two-phase payment delivery), typed reminders, activations"](https://github.com/TheCaptainCompany/captain-food/pull/273)

## Decision

1. **The Order pilot declares the EXPLICIT chain, not a windowed deletion trigger.** The four
   terminal receives (`MarkOrderDelivered`, `RejectOrder`, `CancelOrderByCustomer`,
   `CancelOrderByRestaurant`) carry `schedules: [#/Order/reminders/OrderExpired]`; the reminder
   declares the window (`after: configuration.yaml#/keys/ORDER_RETENTION_WINDOW_DAYS`,
   `reschedule: in-place`); the reminder receive records the fact with record semantics; and the
   `deletion:` block triggers `on: [OrderExpired]` with a typed SELF-`match`
   (`OrderExpired/properties/orderId` ↔ `#/Order/state/orderId`), receipt `OrderDeleted`.
2. **Why the window is NOT on the deletion trigger here**: ADR-20260731-160000 §2's phase 1
   tombstones through the projections **folding a fact** — so the moment "retention elapsed" must
   exist IN THE LOG as a recorded business fact (`OrderExpired`). A windowed deletion trigger
   (`on:` the terminal facts + `after:`) would have the engine act on a timer with no foldable
   cause: read models would lose rows with no log-visible reason, and the accountability question
   "why is this order gone?" would point at infrastructure instead of a fact. The windowed
   trigger form (`on` + `after`, no intermediate fact) REMAINS the right shape when the trigger
   fact is itself the recorded cause and the window is pure delay — the Restaurant pilot's
   cooling period (`RestaurantDeletionRequested` + `after` + `cancelled_on`).
3. **A `match.state` may bind the identity-implied state field** — the typed `identity` ref
   declares its field implicitly (the stream key exists before any fold), so a self-trigger does
   not force an explicit `state:` entry into the aggregate. Validator extended accordingly.
4. **Window configuration keys carry their unit**: `ORDER_RETENTION_WINDOW` became
   `ORDER_RETENTION_WINDOW_DAYS` (matching the `*_SECONDS` convention); the reminders emitter
   REJECTS a window key without a `_DAYS` suffix until another granularity has a use case.
   Default 3650 days — the conservative accounting horizon, because the per-data-category split
   (personal vs financial retention) is still an open legal/product input (ADR-20260731-153000
   "left open"); shortening below it before that split lands would delete financial facts French
   commercial law retains.
5. **Scheduling is applied INSIDE the completion transaction** (`apply_schedules_in_tx`),
   parameterized by the generated `REMINDER_SCHEDULES` table and the generated
   `Config::reminder_windows()` map — the commit and the retention clock start atomically; a
   crash between commit and a post-commit hand-off can never lose a GDPR deadline. The generated
   behaviour tests assert schedule + reschedule-in-place on every `schedules:`-bearing receive
   (the handler's third observable effect, ADR-20260731-214500 §2).

## Consequences

- The generic deletion engine (next #272 slice) reacts to RECORDED facts uniformly: a
  propagation/self trigger acts immediately on the fact; a windowed trigger schedules its own
  engine delay first. No per-actor expiry-fact naming is ever derived by convention.
- `OrderExpired` is temporarily listed under `nonProjectedEvents` — the per-projection tombstone
  fold (ADR-20260731-160000 §2) ships with the engine's projection slice
  ([#194 "GDPR erasure"](https://github.com/TheCaptainCompany/captain-food/issues/194)); removing
  the entry re-arms the validator to hunt non-folding projections.
