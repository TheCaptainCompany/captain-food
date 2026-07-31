# ADR-20260731-150500 — A reminder is RESCHEDULABLE in place: re-declaring postpones, never duplicates

**Status**: Accepted (product-owner decision, in-session 2026-07-31)
**Context**: [ADR-20260731-120825](ADR-20260731-120825-actor-messages-typed-inside-the-actor.md)
(reminder messages typed inside the actor, build deferred until the first use case),
[PROP-20260728-152752 §3.4](../proposals/PROP-20260728-152752-actor-mailbox-write-path.md) (reminders
= kind `MESSAGE` + `scheduled_at`, identity `UUIDv5(actor_id, purpose)`),
[#242 "Write path becomes an actor mailbox…"](https://github.com/TheCaptainCompany/captain-food/issues/242) Runtime D

## Decision

> "It could be great to reschedule an existing reminder — postpone the reminder by keeping the
> existing one."

When an actor declares a reminder whose deterministic identity (`UUIDv5(actor_id, purpose)`)
already exists as a **SCHEDULED** row, that is a **RESCHEDULE**: the existing row keeps its
identity, its history and its place in the mailbox, and only `scheduled_at` (and the payload, if
the re-declaration carries a newer one) moves. It is NOT a duplicate to reject, and it never
requires cancel-and-recreate.

Pinned semantics for the eventual build:

1. **One pending reminder per (actor, purpose).** The deterministic identity is the guarantee: an
   aggregate that keeps pushing back its own deadline (e.g. a preparation-delay check re-armed on
   every status change) converges on ONE row that always carries the latest time.
2. **Reschedule applies to SCHEDULED rows only.** Once the promotion pass has stamped a position
   (the reminder is RECEIVED or terminal), the pending occurrence is SPENT — a later declaration
   of the same purpose opens the question of occurrence-scoped identity
   (`UUIDv5(actor_id, purpose, occurrence)`), which stays OPEN until the first repeating-reminder
   use case; nothing today needs it.
3. **Postponing is the write, cancelling stays separate.** `SCHEDULED → CANCELLED` remains the
   explicit withdrawal (per journals.yaml); a reschedule never transitions status.
4. **The mailbox insert grows a third outcome for kind MESSAGE**: alongside
   `Inserted`/`Duplicate`, a SCHEDULED-row collision resolves as `Rescheduled` (an
   `ON CONFLICT … DO UPDATE SET scheduled_at, payload WHERE status = 'SCHEDULED'` — atomic, no
   read-modify-write race with the promotion pass; a collision with a non-SCHEDULED row stays
   `Duplicate`).

## Consequences

- The reminders slice of Runtime D implements reschedule from day one — the API an aggregate sees
  is "declare the reminder with the time you NOW want", idempotent and self-postponing.
- The promotion pass needs no change: it promotes whatever `scheduled_at` holds when due.
- The supervision surface gains nothing new: a rescheduled row is the same row with a later
  `scheduled_at`.
