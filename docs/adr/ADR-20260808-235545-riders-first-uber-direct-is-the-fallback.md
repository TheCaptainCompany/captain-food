# ADR-20260808-235545 — Delivery-channel clarification: riders first, Uber Direct is the fallback rung

**Status**: Accepted · **Date**: 2026-08-08 (night) · **Decider**: the customer (product owner),
in session, refining [ADR-20260808-212741 §1](ADR-20260808-212741-solida-studio-strategic-frame.md).

## The clarification (verbatim)

> "You know that at the beginning we will first try to rely on independent riders then if nobody
> answers to the job delivery we will ask to uber direct to handle the delivery for us."

This sharpens the strategic frame's "Uber Direct is the launch delivery channel — the partner leg
leads at V0" into: **Uber Direct is the launch FALLBACK** — the ranked walk offers independent
riders first; the partner is called when nobody answers.

## What this maps to (already modeled — no design change)

The #60 ranked-walk dispatch (`DeliveryDispatchProcess` over `CityDeliveryRanking`) is exactly
this shape: rank-1 channel offered first, the walk advancing on offer timeout / decline / manual
escalate, failing CLOSED when exhausted. Partner-done events flow inbound through the Uber Direct
ACL (implemented, both halves): courier assigned → `DeliveryAcceptedByPartner`, progress →
`DeliveryStatusUpdated` (the ONE status vocabulary since the slice-1 retirement), DELIVERED →
the saga sends `MarkOrderDelivered` — the same order closure as a rider completing.

## Consequences (the launch-readiness rows this opens)

- **Tours `CityDeliveryRanking` row**: INDEPENDENT ranked above `uber-direct` — runtime
  configuration to seed at cutover, not code.
- **The rider-offer TTL** (how long riders get before Uber Direct is called): reversible gated
  config; the number is a product decision with Friday-19:30 sensitivity — decide with real data
  (register row; composes with the #400 reality-sensing epic's observability contracts).
- **Sequencing dependency named**: the fallback is only safe once the slice-3 double-dispatch
  receiver lands (#415 — a rider accept must resolve the dispatch run, or the timeout advances to
  Uber Direct over a job a rider already took and Captain pays a partner for a delivery its own
  rider is doing). Already a must-not-ship-without item on #415; restated here because this is the
  money consequence of the customer's chosen sequencing.
