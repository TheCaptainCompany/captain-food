# ADR-20260731-122500 — The mailbox is the only door: worker channels flip to fire-and-forget enqueue

**Status**: Accepted (product-owner decision, in-session 2026-07-31)
**Context**: [PROP-20260728-152752 "The write path becomes an actor mailbox"](../proposals/PROP-20260728-152752-actor-mailbox-write-path.md) §8.6,
[ADR-20260730-231500](ADR-20260730-231500-write-path-becomes-actor-mailbox.md),
[#242 "Write path becomes an actor mailbox…"](https://github.com/TheCaptainCompany/captain-food/issues/242) Runtime C3b

## The question

The GraphQL channel flipped to the mailbox (Runtime C3a). Two on-app workers still dispatched
commands SYNCHRONOUSLY through `dispatch_journaled` — the SIRENE sync worker and the HubRise
enricher/connect flow — running the handler in line and consuming the outcome for their own
retry/poison bookkeeping. Flipping them to the mailbox makes them asynchronous: they enqueue and
no longer see the outcome. Keep them synchronous on `command_journal`, or flip?

## Decision

> "Flip them to fire-and-forget enqueue, the mailbox is the only door."

1. **Every command submission, whatever the channel, is a mailbox enqueue** — GRAPHQL, WORKER and
   EXTERNAL alike. No producer runs a command handler in line; the partitioned mailbox worker is
   the single delivery engine, and the fenced completion transaction is the single place a
   command becomes true.
2. **The producers' bookkeeping records the HAND-OFF, not the outcome.** A SIRENE row is "synced"
   when its command is durably enqueued (deterministic `message_id`s keep redelivery idempotent —
   a re-enqueue dedupes on the mailbox pk); what happens next is the mailbox's ledger
   (`inbound_messages.status`), observable per lane on the supervision surface. The per-producer
   retry/poison counters keep counting ENQUEUE failures (DB unreachable), which is the only
   failure mode left on their side.
3. **The adapter inbox converges too**: adapter ACLs stage kind `EVENT` rows on the mailbox
   (source + external_id dedupe, `message_id = UUIDv5(source, external_id)`) instead of
   `inbound_events`; the mailbox worker routes them through the same `record_inbound_*` delivery
   handlers the drain worker used. The `InboundEventsDrainWorker` retires with the table.
4. **Retirement is staged by what still uses each table**: `inbound_events` is backfilled into
   `inbound_messages` and dropped with this flip. `command_journal` remains ONLY as the
   process-manager legs' door (placeOrder / approveRefund / denyRefund) and the pre-flip
   operationStatus history; it is backfilled and dropped when the PM mailboxes land (Runtime D,
   the last non-mailbox door). Migrations APPLY at the manual deploy only (ADR-20260730-051500).

## Consequences

- `application::dispatch::dispatch_journaled` (the worker-channel sync dispatch) is retired with
  the flip; the SIRENE/HubRise duplicate/conflict semantics carry over to the mailbox insert
  outcome (same id + same hash = deduplicated, same id + different hash = conflict, logged and
  skipped).
- A producer can no longer distinguish "rejected" from "succeeded" at the call site. That
  distinction moves where it belongs: the mailbox row's terminal status, the supervision lanes,
  and the operation status surface.
- The SIRENE worker's `synced_at` semantics shift from "the fact was appended" to "the command
  was durably handed off" — the mailbox is then the authority on delivery.
