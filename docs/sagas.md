# 🔗 Sagas / Process Managers

> Hand-maintained. Source of truth = `crates/application/src/process_managers/*` (pure decisions) +
> `crates/infrastructure/src/process_manager/*` (runtime). Realizes ADR-0046 (write side) & ADR-0031
> (delivery); actors.yaml declares each process manager's inbox (`receives` → `emits`/`throws`).

## What a process manager is

A **process manager** (saga) is an actor that **reacts to events** (not commands) and **emits events**
(and/or invokes command handlers) to coordinate work across aggregates and external systems. It is the
counterpart of a projection: a projector folds events into a *read model*; a process manager folds events
into *new facts / side-effects*. Business logic stays out of the telemetry SDK (ADR-0012).

Its pure decision **owns emission**; the runtime persists the decided facts **through the write-side
`Repository`** (the actor's journal, ADR-20260719-031136) — the runner is the imperative shell and never
touches the raw `EventStore` port. Rehydration is always the actor's **own** write-side stream (never a
read model), so the version for the optimistic-concurrency append is authoritative.

## Runtime — `ProcessManagerRunner` (`crates/infrastructure/src/process_manager/`)

Mirrors the projection worker:
- A **registry** of PM groups; each has its **own `projection_checkpoint` row** (`pm:<Name>` key — reuses
  the existing table, no migration).
- Each tick drains `domain_events` past the checkpoint **by event type** (`event_type = ANY(triggers)` —
  triggers cross stream categories, e.g. `PaymentCaptured` lands on `StripeEvent-%` streams).
- Per event: load the streams the decision needs (via `PgEventStore`), take the **pure decision**, append
  its emitted events under a **saga system actor** (deterministic UUIDv5 user id, `EXTERNAL` user_type;
  **correlation_id propagated from the trigger, cause_id = trigger event id** — ADR-0041), advance the
  checkpoint.
- **Idempotent by construction**: a re-reacted trigger produces the same deterministic ids / hits the
  `UNIQUE(stream, version)` guard → no duplicate facts. **Poison events** are log-skipped (one bad event
  can't wedge the group). A **version conflict** aborts the group without advancing (re-runs next tick over
  fresh state).
- Runs **in-process** on the server, gated by `RUN_PROCESS_MANAGERS` (default on); status at **`/saga`**
  (mirrors `/projector`). Graduates to a dedicated worker with no logic change.
- Decisions are **pure sync functions** returning `Act(StreamAppend)` / `Skip(reason)` / `Nothing` over
  pre-loaded stream slices — all I/O lives in the runner (same split as the projectors' `Compute`).

```mermaid
sequenceDiagram
    box application core
        participant PM as Process manager (decides — pure)
        participant ES as Repository (actor journal)
    end
    box infrastructure adapters
        participant R as ProcessManagerRunner (per tick)
        participant PG as PgEventStore adapter → domain_events
    end
    R->>PG: scan events by trigger type after pm:{Name} checkpoint
    PG-->>R: pending events (ordered)
    loop each event
        R->>ES: load the streams the decision needs
        ES-->>R: stream slices (via PgEventStore)
        R->>PM: decide(trigger, state)
        PM-->>R: Act(emits) / Skip(reason) / Nothing
        R->>ES: save the decided emits (correlation propagated, cause = trigger id)
        ES-->>PG: append (behind the port)
        R->>PG: advance pm:{Name} checkpoint
    end
    Note over PM,PG: the facts are the PM's decision — EventStore is the port, PgEventStore + domain_events the adapter/detail
```

## The four process managers

### 1. PlaceOrderProcess (checkout) — `place_order.rs`
- **Entry (command):** the `placeOrder` mutation → `place_order` handler creates a Stripe PaymentIntent via
  the `PaymentGateway` port and emits **`PaymentIntentCreated`** on `Order-<id>`.
- **Reactions:** `PaymentCaptured` → emit **`OrderPlaced`** (`Order-<id>`) + **`CartCheckedOut`** (`Cart-<id>`);
  `PaymentFailed` → `Nothing` (cart stays OPEN).
- **Status:** wired + idempotent. ⚠️ **Fail-closed in production** — see the checkout-snapshot gap below.
- Real Stripe **create-intent** (outbound) is the Stripe adapter's job (`crates/adapters/stripe`); a
  **fail-closed `PaymentGateway` stand-in** declines meanwhile so nothing is silently charged.

```mermaid
sequenceDiagram
    actor C as Customer
    box application core (crates/application)
        participant H as place_order handler (decides — pure)
        participant PGW as PaymentGateway (port)
        participant PM as PlaceOrderProcess (decides — pure)
        participant REPO as Repository (actor journal)
    end
    box infrastructure adapters
        participant R as server / runner (shell)
        participant SA as Stripe adapter (outbound + webhook ACL)
        participant PG as PgEventStore (→ domain_events)
    end
    C->>R: placeOrder(cart, address, paymentMethod)
    R->>H: PlaceOrder command
    H->>PGW: create_payment_intent
    PGW->>SA: (adapter) Stripe create-intent
    SA-->>H: clientSecret
    H->>REPO: save PaymentIntentCreated (+ frozen checkout) on Order-{id}
    REPO->>PG: append (behind the port)
    R-->>C: { paymentIntentId, clientSecret }
    C->>SA: confirm payment (Stripe.js)
    SA->>PG: record PaymentCaptured (non-aggregate envelope — journal, not Repository)
    R->>PM: drains PaymentCaptured → decide(state)
    PM-->>R: Act[ OrderPlaced on Order-{id}, CartCheckedOut on Cart-{id} ]
    R->>REPO: save the decided facts
    REPO->>PG: append (behind the port)
    Note over PM,PG: on PaymentFailed → Nothing (cart stays OPEN). Checkout frozen on PaymentIntentCreated (ADR-20260719-014434) — full materialization rides pricing
```

### 2. RefundProcess — `refund.rs`
- **Reactions:** `OrderRejectedByRestaurant` / `OrderCancelledByCustomer` / `OrderCancelledByRestaurant` /
  `RefundRequested` → request a refund; `PaymentRefunded` → `Nothing` (the fact is already recorded by the
  Stripe webhook ACL).
- **Status:** done per actors.yaml (all legs `emits: []`). The **outbound Stripe refund call** is a
  `TODO(saga)` awaiting the Stripe adapter (an inbound `PaymentRefunded` still closes the loop).

```mermaid
sequenceDiagram
    box application core (crates/application)
        participant PM as RefundProcess (decides — pure)
    end
    box infrastructure adapters
        participant R as ProcessManagerRunner (shell)
        participant SA as Stripe adapter (outbound + webhook ACL)
        participant PG as PgEventStore (→ domain_events)
    end
    Note over R,PG: trigger already in the log — OrderRejectedByRestaurant / OrderCancelledBy* / RefundRequested
    R->>PM: drain trigger → decide
    PM-->>R: Skip — request the refund (emits [])
    R->>SA: request Stripe refund (TODO saga, outbound)
    SA->>PG: record PaymentRefunded (non-aggregate envelope — journal, not Repository)
    R->>PM: drain PaymentRefunded → decide
    PM-->>R: Nothing (settled fact already recorded)
```

### 3. CartBindingProcess — `cart_binding.rs`
- **Reaction:** `CustomerIdentified` → bind the guest cart to the now-known customer.
- **Status:** done per spec (`emits: []` — "no new event in V0"). The actual bind awaits the Cart
  projection's cross-stream routing (`CartProjector::customer_id` `TODO(runtime)`).

```mermaid
sequenceDiagram
    box application core (crates/application)
        participant PM as CartBindingProcess (decides — pure)
    end
    box infrastructure adapters
        participant R as ProcessManagerRunner (shell)
        participant PROJ as CartProjector (read-model, TODO runtime)
        participant PG as PgEventStore (→ domain_events)
    end
    Note over R,PG: trigger already in the log — CustomerIdentified (login / phone verify)
    R->>PM: drain CustomerIdentified → decide
    PM-->>R: Skip — no new domain event in V0 (emits [])
    Note over PROJ: the actual bind is a read-model cross-stream job (CartProjector::customer_id) — no write through the Repository
```

### 4. DeliveryDispatchProcess — `delivery_dispatch.rs` (ADR-0031)
- **Reactions:** `OrderMarkedReady` → **`DeliveryRequested`** for DELIVERY orders (pickup = restaurant
  address, dropoff = order address, **deterministic UUIDv5 job id from the order id** = idempotency key;
  no-op for COLLECTION); partner `DeliveryAcceptedByPartner` → records courier; `DeliveryRejectedByPartner`
  → `TODO(saga)` re-offer (needs the delivery-partner ACL); `DeliveryStatusUpdated(DELIVERED)` /
  `DeliveryCompleted` → **`OrderDelivered`** (idempotent; a terminal cancelled/rejected order is never
  resurrected).
- **Status:** dispatch + close-order legs functional from the log; re-offer awaits the partner ACL.

```mermaid
sequenceDiagram
    box application core (crates/application)
        participant PM as DeliveryDispatchProcess (decides — pure)
        participant REPO as Repository (actor journal)
    end
    box infrastructure adapters
        participant R as ProcessManagerRunner (shell)
        participant DP as Delivery partner (ACL)
        participant PG as PgEventStore (→ domain_events)
    end
    Note over R,PG: trigger already in the log — OrderMarkedReady (emitted by the Order aggregate)
    R->>PM: drain OrderMarkedReady → decide(order, restaurant, job state)
    PM-->>R: Act[ DeliveryRequested on DeliveryJob-{uuidv5(orderId)} ]  [DELIVERY only]
    R->>REPO: save the decided fact
    REPO->>PG: append (behind the port)
    DP->>PG: record DeliveryAcceptedByPartner / DeliveryStatusUpdated(DELIVERED) (partner ACL — journal)
    R->>PM: drain DELIVERED → decide
    PM-->>R: Act[ OrderDelivered ]
    R->>REPO: save → append (behind the port)
    Note over PM,DP: DeliveryRejectedByPartner → re-offer (TODO saga: partner ACL)
```

## ✅ Resolved — checkout snapshot on `PaymentIntentCreated` (ADR-20260719-014434)

`events.yaml#/PaymentIntentCreated` now carries a required `checkout` (`entities.yaml#/CheckoutSnapshot` —
cartId, contact, serviceType, delivery address, priced items, breakdown), frozen by `commands::place_order`
when it creates the PaymentIntent (`rules.yaml#/CheckoutSnapshotFrozenAtIntent`). So **`OrderPlaced` +
`CartCheckedOut` are reconstructable from the `Order-{id}` stream alone** — no out-of-log store (Option 1,
over the rejected durable pending-checkout store). `PaymentIntentCreated` is non-projected → zero read-model
impact.

**Still riding on pricing (not done yet):** `place_order` freezes `items`/`breakdown` best-available until
server-side line pricing lands (the Cart projector does not price lines yet; the ADR-0016/0017 fee/split is
unwired). Until then the saga still resolves the snapshot through the fail-closed `CheckoutSnapshotSource`
seam and **`Skip`s** — nothing consumes the approximate breakdown. Retiring that seam (read
`PaymentIntentCreated.checkout` directly from the log) is a trivial follow-up once pricing makes the frozen
data correct.

## Open `TODO(saga)` summary
| Saga | Open item | Blocked on |
|---|---|---|
| PlaceOrder | materialize `OrderPlaced` in prod (retire `CheckoutSnapshotSource`) | server-side pricing (priced items + breakdown) |
| Refund | outbound Stripe refund request | Stripe adapter (outbound) |
| CartBinding | actually bind the cart | `CartProjector` cross-stream routing |
| DeliveryDispatch | re-offer on partner rejection | delivery-partner ACL |

## References
ADR-0046 (write side / command handlers), ADR-0031 (delivery bounded context), ADR-0041 (event envelope),
`specs/actors.yaml` (process-manager inboxes), `specs/tests.yaml` (behaviour cases), the `/saga` health
endpoint.
