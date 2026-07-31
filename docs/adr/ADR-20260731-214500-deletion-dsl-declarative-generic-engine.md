# ADR-20260731-214500 — The `deletion:` DSL: declarative per-actor deletion, child-declared propagation, one generic engine

**Status**: Accepted (product-owner decisions, in-session 2026-07-31, iterative design with the
assistant) — REFINES [ADR-20260731-160000](ADR-20260731-160000-order-erasure-tombstone-then-stream-deletion.md)
(the journey stands; its OWNER changes), [ADR-20260731-120825](ADR-20260731-120825-actor-messages-typed-inside-the-actor.md)
(section renamed `reminders:`), and [ADR-20260731-203000](ADR-20260731-203000-runtime-d-choices-a2-b2-c2.md)
(C2's trigger list becomes `schedules:` on receives + the `deletion:` block)
**Context**: [PROP-20260731-195500](../proposals/PROP-20260731-195500-runtime-d-pm-mailboxes-and-reminders.md)
(approved; these refinements are the realizing change's divergences, recorded here per the
proposal-immutability rule), [#272](https://github.com/TheCaptainCompany/captain-food/issues/272)

## Decisions

1. **Naming**: the per-actor typed self-message section is **`reminders:`** (not `messages:` —
   the spec speaks domain language, the mailbox's `kind = MESSAGE` stays plumbing). The
   data-removal concern is **`deletion:`** (not `erasure:` — product-owner preference).
2. **Reminder USAGE is declared on the receive that fires it**: a `receives` entry gains
   `schedules: [{ $ref: '#/<Actor>/reminders/<Name>' }]` alongside `emits`/`throws` — the
   handler's third observable effect, so generated behaviour tests assert scheduling (and
   rescheduling) exactly as they assert emissions and rejections. This supersedes the separate
   `schedule.on` event list of ADR-20260731-203000 C2: same information, declared at the handler,
   testable per receive.
3. **`deletion:` block per actor** — triggers + receipt, both trigger kinds uniform:
   - `on:` (event `$ref`s) + `after:` (a **`$ref` into configuration.yaml**, never a bare
     string) ⇒ the codegen expands to a generated reminder (deterministic identity, reschedule
     in place, promotion pass — the ADR-150500/153000 machinery unchanged);
   - `on:` without `after:` ⇒ the generic engine reacts to the recorded fact immediately —
     this is PROPAGATION: **the child declares how it dies** by listing the parent's receipt
     fact as its own trigger. The dependency tree EMERGES from the declarations; the validator
     builds it, proves acyclicity, proves every ref resolves; the docs generator renders it.
     No parent-side cascade list — read models need no declaration at all (each projection
     folds the deletion fact and removes its own rows, ADR-160000 §2 unchanged).
   - `match:` on a propagation trigger is STRONGLY TYPED — `$ref` to the triggering event's
     property AND `$ref` to the child actor's state property (e.g.
     `events.yaml#/RestaurantDeleted/properties/restaurantId` ↔ `#/Catalog/state/restaurantId`);
     the engine enumerates child instances through the child's projection by that key. Bare
     string paths are barred here and scheduled for removal everywhere (see consequences).
   - `cancelled_on:` (event `$ref`s) — the UNDO: recording a listed fact cancels the pending
     scheduled deletion (`SCHEDULED → CANCELLED`, the explicit transition ADR-150500 kept
     separate from reschedule). Pilot: `CancelRestaurantDeletion` during the cooling window.
   - `receipt:` (event `$ref`) — the business fact recorded on the deletion ledger when the
     journey completes (ADR-160000 §6's `OrderDeleted` shape: pseudonymous references, never
     erased payloads).
4. **One GENERIC deletion engine replaces per-aggregate business PMs** (refines ADR-160000 §4):
   the decided journey — verify projection checkpoints past the fact, honor the window, append
   the technical tombstone event, technical worker deletes the stream from `domain_events` +
   `domain_stream`, record the receipt — is implemented ONCE in infrastructure, parameterized
   entirely by the `deletion:` declarations. Accountability survives: every step is a mailbox
   row / ledger fact on the supervision surface. The escape hatch stands: an aggregate needing
   bespoke steps (e.g. the bookkeeping export before stream deletion) falls back to a
   hand-written PM — `deletion:` is sugar over the same machinery.
5. **The leaving restaurant is the second pilot**: `RequestRestaurantDeletion` is a refusable
   COMMAND (open orders, unsettled payouts — and, when the invoicing concept exists, an
   `UnsettledInvoices` rejection slots into its `throws` with no design change) emitting the
   FACT `RestaurantDeletionRequested`; the `deletion:` trigger runs it through the cooling
   window with `cancelled_on: RestaurantDeletionCancelled`. NO `delete:` flag ever appears on a
   receive — deletion semantics live only in the `deletion:` block.

## Consequences

- Runtime D (D2) implements: the `reminders:`/`schedules:`/`deletion:` DSL + validator rules
  (`reminder-without-receive`, `receive-without-reminder`, deletion-tree acyclicity, ref
  resolution incl. configuration.yaml windows, `match` property typing), the codegen expansion
  (reminder structs, scheduling calls at declared receives, the generic engine's parameter
  tables), the promotion pass, the `Rescheduled` and `Cancelled` outcomes, and the `Order`
  pilot; the `Restaurant` deletion flow lands with its commands/events/errors + tests when
  prioritized.
- **String-path normalization**: `requires.acting: state.customerId` and `identity: orderId`
  predate the strong-`$ref` standard `match:` sets; D2's DSL pass migrates them to typed refs
  so the spec has one dialect, validator-checked end to end.
- ADR-20260731-160000 §4's "an `OrderErasureProcess` PM owns the journey" is realized BY the
  generic engine — the supervision property it argued for is kept; the per-aggregate class is
  not written.
