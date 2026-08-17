# Dispatch — #623 "A failed PlaceOrder records `Internal` with an empty context and nothing in the log"

- **Issue**: [#623](https://github.com/TheCaptainCompany/captain-food/issues/623)
- **Base**: `main` @ `4077188`
- **Reversibility class**: **HIGH-CONSEQUENCE** — the money path and an error surface a client renders. Not `HOLD: human`: nothing here changes what money does, only what is recorded when it fails. If the work turns out to change a failure's *outcome* rather than its *attribution*, it has left this class and stops for the coordinator.
- **Briefing roster** (3): `observability-agent` (owns the contract this path fails to populate), `beck` (a diagnosis nobody can produce is an unverified claim), `young` (typed errors are DSL surface, and `context` is a declared shape).
- **Checkpoint**: to lenses that declare a concern.

> **Antecedent rule** ([ADR-20260817-105845](../adr/ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)). Every figure below is quoted from #623's own measured evidence or marked `UNVERIFIED input`. Two dispatch cards in a row have been materially wrong; verify before relying.

## The defect, verbatim from the walk

```
 message_type | status | attempts |                 err
--------------+--------+----------+-------------------------------------
 PlaceOrder   | FAILED |        0 | {"code": "Internal", "context": {}}
```

And **nothing at ERROR or WARN in the server log** for that failure — the only WARN lines in the whole boot-to-failure window are an unrelated paused worker and an unmapped swept table.

So the single most consequential command in the product failed, and an operator has nothing to diagnose with from either the log or the journal row.

## Why this is not a logging nit

`PlaceOrder` performs **two sequential unfenced durable writes** — the Stripe intent-create, then the append. So `Internal` here spans at least: gateway rejection, gateway timeout, the append itself, and the cart read. Those have completely different operational responses and the record distinguishes none of them.

At Friday peak this is the difference between *"Stripe is refusing us"* and *"our database is wedged"*, and today they are the same string.

The domain lens names *a paid order nobody is told about* as the worst failure mode. This is its neighbour: **an order that failed to be placed, where nobody can find out why.**

**The context in which it was observed is the sharpest part of the argument.** The walk ran with a deliberately-named placeholder Stripe key, so a gateway `401` is the most probable proximate cause — and *even knowing the likely cause, the recorded evidence cannot confirm it.* The empty `context` is the defect, not the rejection underneath it.

## Scope

Make a failed `PlaceOrder` attributable, without changing what the customer sees and without changing what the system does.

- **The customer-facing message does not change.** `operationStatus` continues to serve the generic apology. That is correct behaviour and it is not what is broken.
- **`context` records which seam failed and its kind or status.** The observability contract already distinguishes `technical_error` from `business_rejected` ([ADR-20260810-112836](../adr/ADR-20260810-112836-technical-error-vs-business-rejected.md)); this path appears not to populate it. **Establish whether that is a missing call site or a missing classification**, and say which — they are different fixes.
- **Never the key, never the card, never a raw provider payload.** A gateway status and a seam name; nothing that turns a journal row into a secrets leak. The typed `context` shape is the fence — use it rather than a free-text blob.
- **A log line at the boundary, not in the domain.** Business code stays independent of the telemetry SDK (CLAUDE.md); instrumentation lives at framework/middleware boundaries.

**Open question for the briefing, not pre-decided:** whether `Internal` should stay one code with a populated `context`, or whether the seams deserve distinct typed errors. `young` owns that call — a code is a promise to a client, and adding members is a client-visible change even when it is additive.

## Evidence required

**The bar is the mutation, not the fix.** A classification nobody has seen fire is the same unverified claim this issue is about.

- **Drive each seam to fail and paste the resulting journal row.** At minimum the gateway rejection — the placeholder-key path reproduces it for free — and the append. Each must produce a row that names *which* seam, distinguishably.
- **Mutation, named as the semantic edit**: blank the populated `context` back to `{}` and show the new assertion goes red with a message naming the missing attribution. A test that only proves the happy path leaves exactly today's defect reachable.
- **Show the log line exists** for a failure that previously produced none, and show it carries no secret.
- If an observability contract entry is added or amended, it must be red-on-mutant, not merely declared. **A declared-but-unemitted signal is the failure this repo has now shipped twice** and caught in review both times.

## Fences

- **Do not change any failure's outcome.** A command that fails today must still fail, with the same status, the same customer message and the same retry behaviour. This chunk changes the *record*, nothing else.
- **Do not fix the underlying leg-6 failure.** Diagnosing it is the *next* chunk and it depends on this one; if the fix incidentally reveals the cause, file it and say so.
- **Do not touch the Stripe adapter's behaviour**, the mailbox runtime, or `place_order`'s write ordering. The two unfenced writes are a known and separately-recorded concern — naming them here is context, not scope.
- `specs/**` changes are in scope **only** for `errors.yaml` typed context and `observability.yaml`, and each carries its `SPEC-LOG.md` sentence in the same commit.

## Findings

_(Lenses and the executor append here. "Nothing in my lens" is a complete answer.)_
