# ADR-20260721-104233 — Avelo37 delivery-partner adapter: outbound `DeliveryService` + verified webhook two-layer inbox

## Status

Accepted — implements issue #28. Realizes the "delivery-partner adopts the two-layer inbox on
arrival" follow-up of ADR-20260720-015400, the outbound half of the `delivery` service catalog entry
(ADR-20260719-214500 / issue #26), and conforms to the bounded re-offer policy (ADR-20260720-004556).
Applies the partner-adapter-crate pattern (ADR-20260718-213352).

## Context

The `DeliveryPartner` capability was a no-op: `DeliveryDispatchProcess` offered jobs to
`NoopDeliveryService` (offers went nowhere; jobs stayed open to independent riders only), and the
three inbound partner facts (`DeliveryAcceptedByPartner` / `DeliveryRejectedByPartner` /
`DeliveryStatusUpdated`) had **no production write path** — they existed in the model, folded by the
`DeliveryJob` aggregate and reacted to by the saga, but nothing ever recorded them (only tests
constructed them). Automated delivery dispatch — the last unautomated leg of the order lifecycle —
was blocked on that gap.

The codegen wave the issue named as its precondition has landed: the `DeliveryDispatchProcess` legs
are generated from `specs/processmanager.yaml` (#25), the `DeliveryService` port from
`specs/services.yaml` (#26), and the PM state stores from the table DSL (#27). So this adapter is now
"mostly spec + one adapter crate" rather than a hand-built saga stack, exactly as #28 predicted.

Constraints:
- **Inbound partner facts are NOT commands** (CLAUDE.md): the partner reports what already happened —
  nothing to validate, nothing to reject. They must be recorded through the inbound-event path, not a
  command handler.
- The `inbound_events` drain (ADR-20260720-015400) routed **only** Payment events; delivery facts
  hit its `other =>` arm and were marked FAILED.
- The `DeliveryJob` status is a **declared lifecycle machine** (ADR-20260721-093027) — a recorded
  fact's target is applied unconditionally at fold time, so an illegal transition would corrupt the
  machine. Payment facts have no such guard.

## Decision

**1. A self-contained `crates/adapters/avelo37` crate** (ADR-20260718-213352 pattern, mirroring
`crates/adapters/stripe`): `acl.rs` (framework-free signature verification + partner→domain mapping +
the two-layer-inbox `Avelo37WebhookIngestor`), `raw.rs` (`PgRawAvelo37Events` over the staging
table), `outbound.rs` (`Avelo37DeliveryGateway`), `http.rs` (`POST /adapters/avelo37/webhooks`),
`main.rs` (standalone binary). Mountable into the monolith or deployable as its own web service.

**2. Inbound: the two-layer inbox, adopted verbatim.** A new adapter-owned staging table
`external_avelo37_events` (`staging: true`, `integration_staging.yaml`) mirrors the verbatim verified
webhook body; the ACL translates it into one of the three delivery facts and stages it in
`inbound_events` (`source = 'avelo37'`); the drain delivers it through the normal write path. Webhook
signatures use the **Stripe timestamped-HMAC scheme adopted as our partner contract**
(`Avelo37-Signature: t=…,v1=…`, ±300s replay window, fail-closed on `AVELO37_WEBHOOK_SECRET`) — a
bounded-replay scheme beats a bare body HMAC, and reusing the proven verifier keeps the security
surface uniform.

**3. Drain routing extended beyond Payment events.** `InboundEventsDrainWorker::deliver` gains a
delivery arm routing the three facts to a new recorder,
`application::deliveries::record_inbound_delivery_event` — the delivery sibling of
`record_inbound_payment_event`. It records onto the `DeliveryJob-<id>` stream with the EXTERNAL actor
whose `cause_id = inbound_event_id` (the causality chain), and the DeliveryJob's fold stays the
authoritative dedupe.

**4. Idempotency + lifecycle guard at the recorder.** Where the fold gives an answer, it is
authoritative: an acceptance already reflected when the job carries that `partnerRef`, a status
report already reflected when the job already sits in the reported status. The **lifecycle-bearing**
facts (acceptance, status report) are additionally **guarded**: a report that is not a declared
transition from the current status is NOT appended (it would corrupt the machine) — the drain keeps
the inbound row FAILED/inspectable (the partner is out of sync). **Rejections** are outside the
machine and two successive declines are legitimately identical payloads, so every staged rejection is
recorded — the journal's `(source, external_id)` unique is their delivery-level dedupe, and each
decline advances the bounded re-offer counter (the residual crash-window redelivery can at worst
count one extra decline, erring toward `DeliveryDispatchFailed` + manual handling, never an unbounded
loop). Orphan (birthless) streams record the fact anyway — the saga's `DeliveryJobNotFound` guard
surfaces the anomaly, mirroring the Payment orphan philosophy.

**5. Outbound: fail-closed, env-gated, no behaviour change when unconfigured.**
`Avelo37DeliveryGateway::from_env()` returns the real gateway when `AVELO37_API_KEY` is set (base URL
overridable via `AVELO37_API_BASE_URL`), else `None` — the composition root keeps the logged
`NoopDeliveryService` stand-in, so an unconfigured deployment behaves exactly as before (jobs open to
independent riders; the bounded re-offer run still terminates). `offer_job` POSTs the job carrying our
`deliveryJobId` as `job_reference` — the **read-back key** the partner echoes on every webhook so the
inbound ACL maps facts back onto the stream (the exact Stripe-`metadata` pattern). The offer's return
is only "received"; acceptance/decline arrive asynchronously as inbound facts.

**6. Observability contract.** `avelo37-webhook-ingestion` in `specs/observability.yaml` mirrors
`stripe-webhook-ingestion` (verify → external.persist → acl.translate → inbound.persist →
inbound.drain.deliver → event.store.append), binding the DeliveryJob aggregate + the three inbound
events, so the workflow is asserted DIAGNOSABLE, not merely implemented.

## Alternatives considered

- **A bare body-HMAC webhook (HubRise-style)** — rejected: no replay bound. The timestamped scheme is
  already implemented and unit-tested; reusing it is cheaper and safer.
- **Record inbound facts via a command handler** — rejected: violates the request/report split
  (CLAUDE.md). The partner reports facts; there is nothing to reject. A command would also duplicate
  the lifecycle guard the aggregate fold already owns.
- **Append every inbound status report unconditionally (like payment facts)** — rejected: the
  DeliveryJob lifecycle applies a recorded fact's target unconditionally at fold time, so an illegal
  transition (e.g. a `DELIVERED` report on a CANCELLED job) would silently corrupt the machine.
  Guarding at the recorder keeps the row inspectable instead.
- **Dedupe rejections by fold** — rejected: a genuine second decline is an identical payload; folding
  it as a no-op would stall the bounded re-offer counter. The journal unique is the right dedupe layer
  for distinct-but-identical provider events.
- **Wire the real gateway unconditionally** — rejected: fail-closed operating principle. Unconfigured
  deployments must not change behaviour; `from_env` keeps the no-op stand-in.

## Consequences

### Positive
- Automated delivery dispatch is now end-to-end: offer out, partner facts recorded, saga advances,
  order closes on `DELIVERED`. The last unautomated leg is closed.
- The two-layer inbox is proven as a *general* adapter shape (Stripe payments + Avelo37 delivery),
  not a Stripe special case — the drain is now multi-domain by construction.
- A partner status report that cannot legally apply is surfaced (FAILED inbound row) instead of
  corrupting the job — an operational signal, not silent drift.
- Unconfigured deployments (V0 Tours) are unchanged: no key, no behaviour change.

### Negative
- The real Avelo37 wire contract (event type names `delivery.accepted/declined/status_updated`, the
  `data.delivery` shape, the signature header) is **assumed** from `specs/integrations/avelo37.md` —
  it will need reconciliation against the actual partner API when the commercial agreement lands
  (mapping-only changes, isolated to `acl.rs`).
- Offer timeouts remain OUT (ADR-20260720-004556 Decision 5): a partner that never answers parks the
  run `OFFERED` until a sweep/expiry mechanism lands.

### Follow-up actions
- Reconcile the assumed wire shapes with the real Avelo37 API on commercial go-live.
- Multi-partner routing/ranking in the re-offer step once a second partner integrates (issues #57
  Uber Direct, #58 CoopCycle) — the named extension point of ADR-20260720-004556.
- A `deliveryStatusChanged` subscription for live customer tracking (specs/integrations/avelo37.md §5).
