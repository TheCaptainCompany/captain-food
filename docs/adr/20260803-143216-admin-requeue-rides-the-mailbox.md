# ADR-20260803-143216 — The admin requeue rides the mailbox it supervises

## Status

Accepted — realizes [#315 "Admin requeue mutation for poisoned mailbox rows (ADR-20260803-002712 Q1)"](https://github.com/TheCaptainCompany/captain-food/issues/315),
decided by [ADR-20260803-002712](20260803-002712-mailbox-poison-follow-ups-decided.md) Q1
("first-class ADMIN mutation, full ADR-0032 completeness train — operators never touch SQL on
the money path").

## Context

The #313 poison cap converts repeated delivery-infrastructure failures into a terminal `FAILED`
row (`error->>'code' = 'DeliveryInfrastructureError'`), unblocking the lane. Recovery after the
operator fixes the cause needs a sanctioned write. The mutation targets mailbox
INFRASTRUCTURE (an `inbound_messages` row), not a domain aggregate — and the ADR-0032
completeness gates (behaviour test ⇆ rule, both directions blocking) key off actors.yaml
inboxes, so a port-only "direct resolver" mutation structurally cannot carry its own tested
rule.

## Decision

1. **A `MailboxSupervision` aggregate owns operator actions over the mailbox** (actors.yaml,
   `type: aggregate`, partitions 1), keyed by the SUPERVISED row's `messageId`. Its
   `RequeueMailboxMessage` command rides the mailbox like any other command — acceptance-first,
   pollable `operationStatus`, retried by the runtime — and every intervention lands as a
   `MailboxMessageRequeued` fact on `MailboxSupervision-{targetMessageId}`: the audit trail a
   SQL runbook never leaves (who = envelope `user_id`, ADR-0041), plus a requeue COUNT per row
   (a row that keeps needing requeues means the cause was never actually fixed).

2. **The flip is a single-statement arbitration behind an application port**
   (`MailboxRequeue::requeue_if_poisoned`; Pg adapter `PgMailboxRequeue`): one `UPDATE … WHERE
   status = 'FAILED' AND error->>'code' = 'DeliveryInfrastructureError' RETURNING actor_type`
   decides AND applies — no check-then-act window — resetting `attempts`, clearing `error` /
   `next_attempt_at` / `last_attempt_at` / `completed_at`, and `pg_notify`-nudging the target
   lane in the same statement (redelivery on commit, not at the next heartbeat). Anything else
   refuses: no row → `MailboxMessageNotFound`; any non-poisoned state →
   `MailboxMessageNotRequeueable` — a handler REJECTED/FAILED is a recorded business decision
   and SUCCEEDED/IGNORED/DUPLICATE already ran (rules.yaml#/OnlyCapPoisonedMailboxRowsAreRequeueable).

3. **Already-deliverable converges as success — with two honestly-accepted edges** (independent
   review, 2026-08-03). Like the slug-reservation port, the row write lands alongside (not
   inside) the fenced event append, so a retried delivery of the requeue command meets its own
   earlier effect. The full retry outcome matrix:
   - target still `RECEIVED` → `AlreadyDeliverable`, the fact records, the operation SUCCEEDS
     (the common case — idempotence by outcome, not by transaction);
   - target already REDELIVERED and terminal by retry time (the flip's own nudge makes this
     fast) → the retry reads `NotRequeueable{SUCCEEDED|…}` and the operation lands REJECTED
     **even though the requeue worked** and no audit fact was recorded. Narrow (requires the
     completion transaction to abort exactly between the flip and the append) and self-evident
     to the operator (the row has left the poisoned list, visibly recovered) — accepted rather
     than plumbing flip-provenance through the mailbox.
   - Corollary of convergence: requeueing a row that is `RECEIVED` because it was NEVER
     poisoned also records a `MailboxMessageRequeued` fact (the intent, not a flip). ADMIN-only
     surface, audit-visible, harmless to the row — accepted; keying convergence to the
     supervision stream's own history would refuse it at the cost of the retry case above.

4. **Discovery is a separate ADMIN query, `poisonedMailboxMessages`** (transient type
   `PoisonedMailboxMessage`, page clamped to 200, optional lane filter): `MailboxLane.poisoned`
   was a bare count, and a requeue control without the `messageId` would be a control bound to
   nothing. Kept off the `mailboxLanes` row (no unbounded array on the lanes page; its LATERAL
   counters stay cheap). The `system.captain.food` screen wires the list + per-row Requeue
   button next to the lanes.

### Options considered

- **Direct resolver + port, no actor** (journal+spawn arm): fewer moving parts, but no
  behaviour test or rule can exist for it (coverage keys off inboxes), two permanent validator
  warns, and no durable audit trail — exactly what Q1's "full completeness train" rejects.
- **Flip inside the delivery's fenced transaction** (handler.rs special-case, like PM
  chaining): true atomicity with the audit fact, but grows the infrastructure special-case
  surface on the most safety-critical path for a rare, human-paced operation whose port is
  already idempotent by outcome. Revisit only if the non-atomic window (flip durable, fact
  lost, next delivery records `AlreadyDeliverable`) ever proves confusing in practice.
- **Requeue-by-UPDATE runbook**: rejected by the product owner in ADR-20260803-002712 Q1.

## Consequences

- Poison recovery is: fix the cause → `poisonedMailboxMessages` → `requeueMailboxMessage` →
  the lane redelivers on commit. No SQL, full audit, ADMIN-only at every step.
- **Ordering honesty** (independent review, 2026-08-03): the requeued row keeps its ORIGINAL
  position, so it re-enters HEAD-OF-LINE — ahead of every newer pending row on its lane — and
  it executes AFTER whatever was delivered while it sat FAILED: a genuine per-aggregate ordering
  inversion (an old command re-applied after newer ones; `*Updated` replace semantics make that
  a last-write-wins reversal). The operator judges whether replaying the OLD intent is still
  right — that judgement is exactly why requeue is a human action, and the screen guide says so.
  Also: a row poisoned by an UNDECODABLE payload carries the same error code and offers the same
  button, but a requeue can only re-burn the backoff ladder head-of-line — the guide warns that
  a row whose error names a decode failure is not recoverable by requeue.
- New spec surface rides the ADR-0032 train end to end: command + event + 2 errors + actor +
  mutation + query + rule + 3 behaviour tests + story steps + screen wiring + `platform`
  bounded context (C4 L2).
- E2E `mailbox_requeue` proves the loop on Postgres: listing → worker-delivered flip → audit
  fact → arbitration matrix (converge / refuse-by-status / not-found).
