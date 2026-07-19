# 🔗 Sagas / Process Managers

> **Behaviour source of truth = [`specs/processmanager.yaml`](../specs/processmanager.yaml)** — the
> TYPED step DSL (ADR-20260719-172821): each process manager declares its state table
> (`specs/database/tables/process_managers.yaml`), its outbound ports, and per-message ordered
> `read`/`guard`/`call`/`deliver`/`send`/`state` steps. The **sequence diagrams are GENERATED from
> those steps** — see [`specs/generated/c4.generated.md`](../specs/generated/c4.generated.md)
> (§ Saga sequences) and each PM's actor section in the product documentation
> (`documentation.generated.md` / `.html`). This file keeps the hand-maintained NARRATIVE only.

## The runtime (ADR-20260719-193500) — state-table orchestrators

The process managers run as **state-table orchestrators executing their DSL legs**
(`crates/application/src/process_managers/*`), replacing the earlier fold-based pure deciders:

- **One row = one saga run** in the PM's private table (`payment_process_manager`,
  `refund_process_manager`, `cart_binding_process_manager`, `delivery_dispatch_process_manager` —
  migrated; application `pm_state` ports, Pg stores in `crates/infrastructure/src/persistence/`).
  `last_update_utc` is stamped by the stores (runtime envelope).
- **Command legs** run in the mutation handlers (`commands::place_order` opens the run;
  `process_managers::refund::{approve_refund, deny_refund}` are the RESTAURANT/ADMIN refund
  decisions). **Event legs** are async fns `(deps, event, TriggerEnvelope) → Result<Outcome, DomainError>`:
  `Ok(Completed)` / `Ok(Skipped)` for benign alternatives, `Err(typed error)` for a thrown guard.
- **Guards**: errors always THROW (`PaymentEventOrphaned` for a Stripe outcome matching no run,
  `DeliveryJobNotFound` for partner reports on an unknown dispatch) — the runner surfaces them on
  `/saga` `last_error` and advances (never wedges, never silently skips). `skip` is only for benign
  alternatives (idempotent re-delivery, COLLECTION no-op, nothing-captured).
- **deliver** appends the fact to the owning aggregate's stream under the saga actor identity
  (correlation propagated, cause = trigger id); **send** invokes the target's command handler — the
  close-order leg sends `MarkOrderDelivered` and the Order's own invariants prevent resurrecting a
  terminal order (rejection on an event leg = logged + skipped).
- The **runner** (`crates/infrastructure/src/process_manager/runner.rs`) keeps the proven skeleton:
  per-group `pm:<Name>` checkpoints in `projection_checkpoint`, draining `domain_events` by trigger
  `event_type`, poison-log-skip, version-conflict abort-without-advance, `/saga` status; in-process
  behind `RUN_PROCESS_MANAGERS` (default on).
- **Payment aggregate** (`domain::payment`, stream `Payment-{paymentIntentId}`): `place_order`
  delivers `PaymentIntentCreated` (with the frozen checkout, ADR-20260719-014434) there; the
  **stateless Stripe ACL** delivers `PaymentCaptured`/`PaymentFailed`/`PaymentRefunded` there via
  `application::payments::record_inbound_payment_event` (dedup = the aggregate's fold; the old
  `StripeEvent-{id}` envelope streams and the fail-closed `CheckoutSnapshotSource` seam are retired —
  the capture leg reads the snapshot straight off the Payment stream).

## The four process managers — status

| Saga | Legs | Status |
|---|---|---|
| PlaceOrderProcess | `PlaceOrder` cmd → intent + run row; `PaymentCaptured` → `OrderPlaced` + `CartCheckedOut` from the frozen snapshot; `PaymentFailed` → run FAILED, cart stays OPEN; orphans throw | ✅ implemented (Stripe create-intent still the fail-closed stand-in gateway) |
| RefundProcess | refundable facts open PENDING_APPROVAL (payment CAPTURED only); `ApproveRefund`/`DenyRefund` (RESTAURANT own orders / ADMIN) → Stripe `request_refund` + decision on the Payment; `PaymentRefunded` settles | ✅ implemented (outbound `request_refund` fail-closed until the Stripe adapter lands; `pendingRefunds`/approve/deny API surface still to add in api.yaml) |
| CartBindingProcess | `CustomerIdentified` → `BindCartToCustomer` per OPEN cart of the session (Cart emits `CartBoundToCustomer`; projection folds same-stream) | ✅ implemented — the old cross-stream projector gap is gone |
| DeliveryDispatchProcess | `OrderMarkedReady` (DELIVERY) → `DeliveryRequested` birth (UUIDv5 job id) + partner `offer_job`; partner accept/reject; DELIVERED/`DeliveryCompleted` → send `MarkOrderDelivered` | ✅ implemented (`offer_job` = no-op stand-in; re-offer policy still TODO) |

## Open items

| Item | Blocked on |
|---|---|
| ~~Real Stripe create-intent + outbound refund~~ ✅ landed: `stripe::outbound::StripePaymentGateway` (env-gated by `STRIPE_SECRET_KEY`; fail-closed stand-in otherwise) | — |
| ~~`approveRefund`/`denyRefund` mutations~~ ✅ landed (roles [RESTAURANT, ADMIN] + story steps); still open: the `pendingRefunds` query + its read model | read-model design |
| Partner re-offer policy on `DeliveryRejectedByPartner` (row flags REOFFER_REQUIRED) | delivery-partner ACL |
| Server-side line pricing (frozen snapshot carries best-available amounts) | pricing program (ADR-0016/0017) |

## References

ADR-20260719-172821 (typed step DSL) · ADR-20260719-193500 (state-table runtime) ·
ADR-20260719-031136 (write-side Repository) · ADR-20260719-014434 (checkout snapshot) ·
ADR-0031 (delivery) · ADR-0041 (event envelope) · `specs/tests.yaml` (behaviour cases) ·
the `/saga` health endpoint.
