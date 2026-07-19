# WIP — Process-manager re-architecture (resume notes)

> **Status: DSL LAYER DONE — `make validate` = 0 errors, `cargo test --workspace` = 216 green.** The
> free-text `steps` were replaced by the **typed step DSL** (ADR-20260719-172821): ordered
> `read`/`guard`/`call`/`deliver`/`send`/`state` steps, `$ref`s/enum consts everywhere, state in
> declared typed tables, command-leg guards `throws` / event legs `skip`, PM emits **derived** from
> steps, sequence diagrams **generated** from steps (`specs/generated/c4.generated.md`). The planned
> "exempt PMs from the completeness gate" was **dropped** — validator §2b + the derived inbox make the
> ADR-0032 gate apply to PMs unexempted. RefundProcess is fully specified (admin-approved:
> PENDING_APPROVAL → Approve/Deny → settled; `RefundNotPending`). The **runtime is still the old
> event-sourced implementation** — its reimplementation against the DSL is the remaining program
> (below). Companion: `specs/processmanager.yaml`, `specs/database/tables/process_managers.yaml`.

## Why this change

A long design review of the sagas/diagrams surfaced two layering problems and one modelling shift:
1. **Adapters must never write domain events / touch `domain_events`.** The Stripe webhook adapter was
   appending `StripeEvent-{id}` straight to the event log — an adapter reaching into domain persistence,
   and putting an adapter-idempotency envelope into the domain log. Wrong on both counts.
2. **Deduplication is business logic, not an adapter concern.** The owning **actor** decides "already
   handled?" from its own stream/state (+ the event store's optimistic-concurrency guard). No adapter
   dedup table, nothing synthetic in `domain_events`.
3. **Process managers are not aggregates.** They don't emit domain events; they **orchestrate** — react to
   a message, run ordered steps (call aggregates via commands/events, call externals like Stripe), and keep
   their own **state table**. Aggregates own the facts.

## The new model

### Aggregates (event-sourced actors — `actors.yaml`)
Own their facts; receive commands (and, for births/inbound facts, events) and emit events. Added:
- **`Payment`** — receives `PaymentIntentCreated` / `PaymentCaptured` / `PaymentFailed` / `PaymentRefunded`
  and records them (the Payment stream owns the payment lifecycle). This is where the inbound Stripe facts
  land — recorded by the actor, not the adapter.
- **`Rider`** — `RegisterRider` / `UpdateRiderInfo` / `ChangeRiderStatus`.
- **`DeliveryJob`** — absorbed the delivery operations (accept/pickup/complete/cancel/decline/issue/
  assign-partner/…) that used to live in the PM.
- **`Cart`** — `BindCartToCustomer` (session-id based), **`Order`** — `RequestRefund` (unchanged).
- **Births = the actor receives its birth event from the PM and records it** (decision): `Order` receives
  `OrderPlaced`, `DeliveryJob` receives `DeliveryRequested`, `Payment` receives `PaymentIntentCreated` — no
  birth commands. Idempotency is the actor's (fold its stream; re-delivery = no-op).

### Process managers (`processmanager.yaml` — NOT actors)
State-table orchestrators. Each `receives[]` entry is `{ message, steps, effect }` where **`steps` are
ordered documentation** (lock PM → read/price → call Stripe → save on the Payment actor → lock cart →
unlock PM, …). They hold state in a table (`payment_process_manager`, keyed by `cart_id`;
`refund_process_manager` to add), and dedup Stripe redelivery via that state
(`last_processed_stripe_event_id`). **DECISION: PMs are doc-only — there is nothing to code-generate from
`steps`; the PM logic is hand-written.** The codegen must therefore *exempt* PMs from the aggregate-style
completeness gate (see "remaining").
- `PlaceOrderProcess` — `PlaceOrder` cmd → create Stripe intent, save on Payment actor, lock cart;
  `PaymentCaptured` → materialize Order + close cart; `PaymentFailed` → unlock cart.
- `CartBindingProcess` — `CustomerIdentified` → read the visitor's OPEN carts **by `sessionId`** →
  `BindCartToCustomer`.
- `DeliveryDispatchProcess` — `OrderMarkedReady` → create+offer the delivery job; `DeliveryRejectedByPartner`
  → re-offer.
- `RefundProcess` — **admin-approved** (see below); currently a stub in `processmanager.yaml` to flesh out.

## Decisions taken (this session)

| Topic | Decision |
|---|---|
| Adapter idempotency table | **None.** Dedup is the actor's business decision; adapter is a stateless translator. No `external_stripe_events`, no `..._events`, nothing synthetic in `domain_events`. |
| Where inbound payment facts land | The **`Payment` aggregate** records them; the Stripe adapter *translates + delivers* to it via an application use case (like HubRise → `import_catalog`). |
| Aggregate births | **Actor receives + records** the birth event from the PM (no birth commands). |
| Refunds | **All admin-approved** — a trigger opens a *pending refund* (RefundProcess state); admin `ApproveRefund`(amount)/`DenyRefund`(reason); on approve → Stripe refund → `PaymentRefunded` recorded by Payment actor. Eligibility window = a **config value** the approval enforces, not a domain rule. |
| Payout clawback (Stripe Connect) | **Reverse pre-delivery, keep rider post-delivery** — pre-fulfilment refunds reverse restaurant + rider transfers; a post-delivery complaint keeps the rider paid, restaurant/Captain absorb. (Runtime mechanic; record in the refund ADR.) |
| `CompleteDelivery` | Re-added as a `DeliveryJob` command (→ `DeliveryCompleted`). |
| PMs | **Doc-only** (`steps` are comments); handled manually — nothing to codegen. |

## Current state — `make validate` 114 → 58

**Done (structural DSL, verified):**
- Added all missing types (the whole "does not resolve" list): `RiderStatus` scalar; Rider 3 commands / 3
  events / 3 errors; the 8 DeliveryJob-op commands + 6 events; `ApproveRefund`/`DenyRefund` +
  `RefundApproved`/`RefundDenied`.
- `actors.yaml`: fixed `messsage`/`thows` typos; wired `CompleteDelivery`; added the `Order`←`OrderPlaced`
  and `DeliveryJob`←`DeliveryRequested` birth-receipt entries.
- Codegen: added `processmanager.yaml` to `SOURCE_FILES` (`tools/codegen-rs/src/main.rs`); repointed every
  `actors.yaml#/<PM>` ref → `processmanager.yaml#/<PM>` in `tests.yaml`, `observability.yaml`, `c4-l2`,
  `c4-l3` (55 danglings cleared). `payment_process_manager` state table picked up generically.

**~~Remaining (the 58)~~ — DONE (typed-step DSL session, ADR-20260719-172821):**
1. ~~Exempt PMs from the gate~~ → **reversed**: PMs joined the gate. `parse_actors` folds
   `processmanager.yaml` in with **derived** emits/throws (delivered events ∪ sent-command emits per
   the target inbox; guard throws); validator **§2b** proves state columns/enums, read models, ports,
   deliver/send targets, command-throws/event-skip; emitters (docs, mermaid, c4) see PMs again and the
   saga diagrams are now **generated from the steps**.
2. ~~sessionId~~ done — and completed through the model: `CartStarted.sessionId` (required) +
   `Cart.session_id` projection column + `VerifyPhone.sessionId` (required, flows onto
   `CustomerIdentified`); runtime handlers/stores/tests updated.
3. ~~Behaviour tests~~ done — Rider (6), DeliveryJob ops (10), Payment records (6), Cart bind/checkout
   (2), Order birth (1), RefundProcess admin decisions (3); new rules `RiderLifecycle`,
   `DeliveryPartnerAssignmentLifecycle`, `DeliveryDeclineKeepsJobPending`, `DeliveryIssueLifecycle`,
   `RefundRequiresAdminApproval`.
4. ~~RefundProcess~~ specified (admin-approved; `refund_process_manager`; `RefundNotPending`).
   Still open: a pending-refund read model + `pendingRefunds`/`approveRefund`/`denyRefund` ADMIN
   surface in `api.yaml` (+ story steps) — deliberately deferred with the runtime.
5. Regenerated at validate = 0. ADR-20260719-172821 written. Still open: observability contracts for
   the re-architected sagas; the Payment-aggregate/clawback ADR rides with the runtime program.

## Runtime reimplementation — DONE (ADR-20260719-193500)

Landed on this branch (266 workspace tests green): state-table orchestrators over the migrated PM
tables (`pm_state` ports + Pg stores), the `Payment`/`Rider` aggregates + DeliveryJob partner/issue
folds, the full missing command surface, the reworked `place_order` (Payment stream + run row), the
stateless Stripe ACL (`record_inbound_payment_event`, `StripeEvent-%` and the snapshot seam
retired), and the runner surfacing thrown guards on `/saga`. Remaining follow-ups are tracked in
docs/sagas.md § Open items.

## The original phase plan (historical)
Separate from the DSL. Replace the event-sourced `ProcessManagerRunner` + saga decider wiring with
**state-table PM orchestrators** (lock/step/unlock over `payment_process_manager` / `refund_process_manager`);
add the `Payment`/`Rider` aggregates in `crates/domain`; make the Stripe adapter a **stateless translator**
that delivers inbound facts to the Payment actor (drop its `EventStore` dependency + the `StripeEvent-{id}`
stream); move `PaymentIntentCreated` + the checkout snapshot onto the `Payment` stream; implement the
outbound Stripe refund + Connect transfer-reversal. Plan phase-by-phase after the DSL is green.

## Already landed this session (green, separate from the above)
The **write-side `Repository` / event-sourced-actor** refactor (ADR-20260719-031136), the **checkout
snapshot on `PaymentIntentCreated`** (ADR-20260719-014434), the Stripe/HubRise **process docs**
(`docs/integrations/`), the **`/adapters/{partner}/webhooks`** route move, the **mermaid convention**
(`docs/claude/mermaid.md`), and the HubRise account-model doc — all built + `cargo test --workspace` green
(216 tests). NOTE: this re-architecture will rework the saga side of the Repository refactor.
