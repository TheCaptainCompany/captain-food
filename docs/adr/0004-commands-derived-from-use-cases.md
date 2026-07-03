# ADR-0004 — Commands derived from use cases (not one-per-event)

## Status
Accepted

## Context
A naive CQRS model mirrors one command per event. That produces anemic, mechanical commands and misses
real business intentions (a single intent may emit several events; some events have no command).

## Decision
Commands are derived from **use cases** in the story map (`specs/stories.yaml`).
A command may emit several events (e.g. `PlaceOrder` → `PaymentIntentCreated` → … → `OrderPlaced` +
`CartCheckedOut`). Facts reported by external systems are recorded as **inbound integration events**
(no command, through the ACL) — e.g. Stripe `PaymentCaptured`/`PaymentFailed`/`PaymentRefunded`, HubRise
inventory sync. Rule of thumb: if the originator can be told "no" → command; if it states something that
already happened → inbound event.

## Alternatives considered
- One command per event — mechanical, hides intent, can't model sagas or inbound facts.

## Consequences
### Positive
- Commands express business intentions; sagas and inbound facts are modeled honestly.
### Negative
- Requires the story map to be maintained as the derivation source.
### Follow-up actions
- Keep `actors.yaml` (`receives → emits/throws`) and `tests.yaml` coverage aligned with the use cases.
