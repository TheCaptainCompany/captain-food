# ADR-20260726-171736 — Reclamation REPLACEMENT resolution: the no-charge replacement order

## Status

Accepted

<!-- Realizes the REPLACEMENT automation promoted to V0 by ADR-20260726-124204 (decision #2); extends the
     ReclamationProcess saga introduced by ADR-20260726-163737. Tracking issue #159. -->

## Context

`ReclamationResolved{resolution: REPLACEMENT}` records the decision to remake/redeliver an order, but does
nothing. This ADR makes it real: the `ReclamationProcess` saga (ADR-20260726-163737) gains a `REPLACEMENT`
arm that **places a no-charge replacement order** for the same items as the original, so it enters the
normal fulfilment + dispatch flow.

## Decision

- **Saga arm**: on `ReclamationResolved(REPLACEMENT)`, `ReclamationProcess` reads the original order's
  items (cross-aggregate, by `orderId`) and dispatches a replacement placement.
- **Representation**: reuse the `Order` aggregate — a replacement is a normal order with a `replacementOf`
  link to the original and **no payment** (a $0 buyer total; no Stripe `PaymentIntent`). It flows through
  the existing fulfilment/dispatch as any order does; the restaurant remakes it, the rider redelivers.
- **Idempotency**: one replacement per resolved claim (keyed by `reclamationId`), so an at-least-once
  saga redelivery never places two.
- **Scope/flag**: if placing an order from the saga integrates cleanly with the placement flow
  (`PlaceOrderProcess`/`Order`), build it; if the no-payment placement is too invasive to that flow in one
  slice, land the saga arm + the replacement command/event (recording the intent + the link) and FLAG the
  full fulfilment wiring as a follow-up (mirroring #158's honest flags), never a half-correct order path.

### Sequence

```mermaid
sequenceDiagram
    autonumber
    actor R as Restaurant/admin
    box application core
        participant REC as Reclamation aggregate
        participant PM as ReclamationProcess (saga)
        participant ORD as Order aggregate (replacement)
        participant REPO as Repository
    end
    box infrastructure adapter
        participant PG as PgEventStore
    end
    R->>REC: resolveReclamation(REPLACEMENT)
    REC-->>REPO: save(ReclamationResolved REPLACEMENT)
    REPO->>PG: append
    Note over PM: reacts to ReclamationResolved(REPLACEMENT); reads the original order's items
    PM->>ORD: place a NO-CHARGE replacement order (same items, replacementOf = original, no PaymentIntent)
    ORD-->>REPO: save(OrderPlaced replacementOf, buyer total 0)
    REPO->>PG: append
    Note over ORD: enters the normal fulfilment + dispatch flow (restaurant remakes, rider redelivers)
```

### Mockup — customer sees the replacement (in the claim / order thread)

```
+-------------------------------------------+
|  <  Claim on order A1B2      [ RESOLVED ]  |
+-------------------------------------------+
|  Resolution: Replacement                   |
|  A new order is on its way - no charge.    |
|   * Replacement placed        14:31        |   <- OrderPlaced(replacementOf=A1B2), 0.00 EUR
|   [ Track the replacement -> ]             |   -> the new order's tracking
+-------------------------------------------+
```

## Alternatives considered

**Replacement representation**
| Option | Pros | Cons |
|---|---|---|
| **Reuse `Order` with `replacementOf` + $0, no PaymentIntent → CHOSEN** | Enters the existing fulfilment/dispatch for free; one order concept; the customer tracks it like any order | The placement path must support a no-payment order |
| A dedicated `Replacement` aggregate | Isolated | Reinvents fulfilment/dispatch/tracking; a parallel order concept for no gain |

**Cost bearer**
| Option | Pros | Cons |
|---|---|---|
| **$0 to the customer; the restaurant remakes (their fault) → CHOSEN for V0** | Simple; matches "we got it wrong, here's a fresh one"; no money moves | The rider redelivery cost is unmodelled in V0 (who pays the courier for a redelivery is a follow-up) |
| Charge the restaurant / a settlement | Fair cost attribution | Needs a settlement mechanism — out of V0 scope |

**Idempotency / abuse**
| Option | Pros | Cons |
|---|---|---|
| **One replacement per resolved claim (key = reclamationId) → CHOSEN** | Prevents saga-redelivery duplicates and repeat abuse on one claim | A genuinely-needed second replacement requires a new claim (acceptable) |

## Consequences

### Positive
- A REPLACEMENT claim actually sends a fresh order through the normal flow; the customer tracks it.
- Completes the ReclamationProcess saga (refund arm — follow-up; credit arm — #158; replacement — here).

### Negative / Follow-up
- The rider-redelivery cost attribution is unmodelled (V0: restaurant remakes, no settlement).
- If the no-payment placement is flagged this slice, the saga records the replacement intent + link and
  the fulfilment wiring is the follow-up (double-placement safety is the key property when it lands).
