# ADR-20260831-093000 — The enumeration is `deliver:` ∪ `send:` ∪ wrapper-seam `sends:`, not `deliver:` alone

- **Status**: Accepted
- **Date**: 2026-08-31
- **Decider**: the team, executing
  [ADR-20260829-230418](ADR-20260829-230418-aggregates-own-the-facts-the-isolation-subject-is-resolved-first.md)
  — this record corrects that ADR's ENUMERATION, not its property.
- **Relates**:
  [ADR-20260816-040239](ADR-20260816-040239-deliver-is-a-lane-enqueue-not-a-foreign-stream-append.md) ·
  [#807 "The `send:` route grammar drops its routing target: four PM sends append to streams they do not own"](https://github.com/TheCaptainCompany/captain-food/issues/807)
- **Register**: [AGGREGATES-OWN-THE-FACTS](../decisions/AGGREGATES-OWN-THE-FACTS.yaml)

## Consulted

- **vernon** — the `send:` class. A `send:` is an Ask, not a Tell: the target may refuse, so routing
  it changes WHERE the refusal is recorded (a supervisable REJECTED row instead of a log line), not
  whether the aggregate may refuse. Also: C4's load-only port must NOT be narrowed to `deliver:`, or
  these four sends type-check forever against a port that cannot express them.
- **evans** — the property sentence in `specs/common/processmanager.yaml` already says *"or sends
  commands the aggregate may reject"*. This is enumeration correction inside the declared language,
  not a new coinage and not a scope amendment.

## Context

ADR-20260829-230418 states the property — *aggregates own the facts; a process manager decides,
never appends* — and then enumerates the work as **thirteen `deliver:` steps**. The property's own
sentence is broader than that enumeration: it covers **sends** explicitly. Four `send:` steps were
therefore outside the count while being squarely inside the property, and each one has a process
manager appending to a stream it does not own, synchronously, on the saga's thread:

| Step | Command | Stream the saga writes |
|---|---|---|
| `CartBindingProcess` (`for_each: open_carts`) | `BindCartToCustomer` | `Cart-{id}` |
| `ReclamationProcess` | `GrantCustomerCredit` | `CustomerCredit-{customerId}` — the money path |
| `DeliveryDispatchProcess` (partner DELIVERED report) | `MarkOrderDelivered` | `Order-{id}` |
| `DeliveryDispatchProcess` (independent rider completion) | `MarkOrderDelivered` | `Order-{id}` |

The grammar compounded it. `PmStepDef::Send` carried `{ command, with, for_each, note }` — no `to`,
no `route_gate` — while all four steps **already wrote `to:` in the DSL**. `pm-send` validated that
target and the emitter then discarded it, because `route_decls` had arms only for `deliver:` steps
and wrapper-seam `sends:` declarations. A `$ref` that reads as a routing target and routes nothing
is the false-signal class this programme keeps paying for.

## Decision

**The enumeration is `deliver:` ∪ `send:` ∪ wrapper-seam `sends:`.** All three are ways a process
manager reaches another aggregate's stream, all three are covered by the property as written, and
all three are routed the same way — through the target's mailbox lane, behind a per-route gate.

Consequences carried in the same change (#807):

1. `PmStepDef::Send` consumes `to` and `route_gate`; `route_decls` gains its fourth arm, so a routed
   `send:` reaches `ROUTED_LANES` and the `Route` enum — and therefore #783's dead-man's-switch
   population.
2. `pm-route-gate` extends to `send:` steps. Because `to:` is **mandatory** on a send, "target
   present, gate absent" is not an unrouted step the way an unrouted `deliver:` is — it is the
   defect. **Every `send:` must declare its route**: a saga writing a stream it does not own is
   never the default.
3. A new rule, **`pm-send-dedup`**, requires a routed send to NAME the axis its door dedups on. This
   was found by generating the money path and reading it: the door is keyed
   `(route, external_id)`, and inheriting the TARGET's identity would have keyed the credit door on
   `customerId` while the handler is idempotent per `reclamationId`. A customer legitimately
   receives many goodwill credits, so that door would have swallowed every grant after the first —
   **money owed, never paid, no error raised anywhere.** There is deliberately no default, because
   the safe axis does not follow from the target: an order is delivered once (order id), a customer
   is credited many times on a ledger keyed by customer (reclamation id).
4. Three per-route configuration keys, all `default: false`, in **`specs/common/`**. Route gates are
   KERNEL configuration, not scope configuration: the generated `RouteGates` struct carries one
   field per declared route and every PM bin's composition root constructs the whole struct, so a
   gate must be readable from every bin's scoped `Config` or that bin does not compile. It is also
   the semantics — every composition root must resolve the same value or a split fleet routes some
   messages and appends others.

Four sends, **three** routes: the two `MarkOrderDelivered` legs are two TRIGGERS for one route (same
command, same target, same PM), so they share one `Route` variant, one `ROUTED_LANES` row and one
key. That is not a fused flag; the per-route independence ADR-20260829-230418 C3 protects is
independence between UNRELATED routes.

## What this does NOT decide

Every **flip** is its own recorded decision, after smoke — all three keys ship OFF and the legacy
in-process arms are preserved byte-for-byte behind them. Every **legacy-arm deletion** is a separate
change again, with golden payload equality as its precondition. The C4 capability witness is its own
chunk. No `deliver:` route moves here.

## Consequences

- ADR-20260829-230418's "thirteen `deliver:` steps" is a count of one FORM, not of the work the
  property implies. Anyone reading it for scope reads this record with it.
- #764's credit settlement leg would have made the erasure journey a SECOND unlaned writer to
  `CustomerCredit-{customerId}`, separated from the first only by an optimistic version conflict.
  That precondition is removed whichever way `CREDIT-LEG-SEQUENCING` is answered.
- The general lesson, one level up: **a property and an enumeration of it are different artifacts,
  and the enumeration is the one that rots.** Where a record states both, the property governs.
