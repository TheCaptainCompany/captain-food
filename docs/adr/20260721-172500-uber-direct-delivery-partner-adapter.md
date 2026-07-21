# ADR-20260721-172500 — Uber Direct delivery-partner adapter: an OAuth2 PARTNER path

## Status

Accepted — implements issue #57. A `DeliveryProvider = PARTNER` adapter via the Uber **Direct**
delivery API (not the Uber Eats marketplace; distinct from the price-comparison ADRs
0022/0023/0024/0030), applying the Avelo37 pattern (ADR-20260721-104233): the outbound
`DeliveryService` half (ADR-20260719-214500 / issue #26), the two-layer webhook inbox
(ADR-20260720-015400), the partner-adapter-crate shape (ADR-20260718-213352). It plugs into the
multi-partner dispatch foundation (#60, ADR-20260721-161939) as the `uber_direct` channel — the Tours
ranking seed already ranks it, so no saga change is needed. Sibling of #58 (CoopCycle).

## Context

Avelo37 (#28) established a reusable delivery-partner shape and CoopCycle (#58) proved it generalises:
a self-contained adapter crate, a verified-webhook two-layer inbox (`external_<partner>_events` →
`inbound_events` → drain onto the `DeliveryJob` stream, routed by event type — already partner-generic),
and a fail-closed, env-gated outbound gateway behind the generated `DeliveryService` port. The three
inbound facts (`DeliveryAcceptedByPartner` / `DeliveryRejectedByPartner` / `DeliveryStatusUpdated`) and
`application::deliveries::record_inbound_delivery_event` are **partner-agnostic**. So a new partner is
"mostly one adapter crate + one `services.yaml` entry + one staging table" — no new events, commands,
errors, or drain routing.

The delivery dispatch foundation (#60, ADR-20260721-161939) then made partner **selection** data-driven:
a channel CATALOG + a per-city `CityDeliveryRanking` walk, offered on a strategy-resolved `channel`
routed by a composite `DeliveryService`. The foundation seed already registers `uber_direct` in the
catalog and ranks it in Tours (rank 2, after independent riders) — this adapter is what makes that
channel *wired*.

Uber Direct differs from Avelo37 in exactly **two** structural ways:

- **Auth: OAuth2 client-credentials.** Uber Direct calls need a fetched/refreshed bearer token
  (a token manager), unlike Avelo37's single static key. (CoopCycle already introduced an OAuth2 token
  manager; Uber Direct is the single-token, single-endpoint form of it.)
- **Signature scheme.** Uber signs webhooks with a plain **`X-Uber-Signature = hex(HMAC-SHA256(secret,
  raw_body))`** — no timestamp, so no replay window — unlike the Stripe-style `t=…,v1=…` scheme
  Avelo37/CoopCycle adopted.

## Decision

**1. A self-contained `crates/adapters/uber_direct` crate**, mirroring `crates/adapters/avelo37`:
`config.rs` (single-endpoint config + OAuth2 credentials, env-gated), `acl.rs` (framework-free
`X-Uber-Signature` verification + Uber→domain mapping + the two-layer `UberDirectWebhookIngestor`),
`raw.rs` (`PgRawUberDirectEvents` over the staging table), `outbound.rs` (`UberDirectDeliveryGateway` +
OAuth2 token manager + Create Delivery), `http.rs` (`POST /adapters/uber-direct/webhooks`), `main.rs`
(standalone binary). Mountable into the monolith or deployable as its own web service.

**2. Inbound: the two-layer inbox, reused.** A new adapter-owned staging table
`external_uber_direct_events` (`staging: true`) mirrors the verbatim verified webhook body; the ACL
translates it into one of the three (existing) delivery facts and stages it in `inbound_events` with
`source = 'uber_direct'`; the existing drain delivers it through the normal write path — **no
drain/journal DSL change**. No new events/commands/errors ⇒ no new ADR-0032 behaviour-test obligations
on that axis.

**3. Config = a single endpoint + OAuth2 credentials, env-gated, fail-closed.** `UBER_DIRECT_CUSTOMER_ID`
/ `UBER_DIRECT_CLIENT_ID` / `UBER_DIRECT_CLIENT_SECRET` / `UBER_DIRECT_WEBHOOK_SECRET` (+ optional
`_BASE_URL` / `_TOKEN_URL` / `_SCOPE`) configure the adapter out-of-repo; all four required vars unset
⇒ the composition root leaves the `uber_direct` channel unwired (its offers time out and the saga
advances to the next ranked channel — V0 Tours behaviour unchanged). A PARTIALLY-set config is a
misconfiguration surfaced as an error, not a silent no-op. Secrets stay out of the repo.

**4. Outbound.** `offer_job` fetches/refreshes the OAuth2 client-credentials token (single cached
token), then POSTs Uber's Create Delivery (`POST {base}/v1/customers/{customer_id}/deliveries`),
carrying our `deliveryJobId` as `external_id` — the read-back key Uber echoes on every webhook. The
offer's return is only "created"; courier assignment/progress arrive asynchronously as inbound facts.

**5. Inbound.** Webhooks arrive at `POST /adapters/uber-direct/webhooks`, verified against
`UBER_DIRECT_WEBHOOK_SECRET` with the `X-Uber-Signature` raw-body HMAC (constant-time compare,
fail-closed). Idempotency key is Uber's `event_id` (globally unique — no instance namespacing, unlike
CoopCycle). The status mapping treats courier assignment (`pickup`) as `DeliveryAcceptedByPartner`, a
pre-assignment `canceled`/`returned` (no courier) as `DeliveryRejectedByPartner` (so the saga
re-offers), and every other transition as `DeliveryStatusUpdated`.

**6. Wiring & observability.** The composition root registers `uber_direct` on the composite delivery
gateway when configured, and mounts the webhook route with the signing secret. `REQUIRED_SCHEMA_VERSION`
is bumped to the migration that adds the mirror. `uber_direct-webhook-ingestion` mirrors
`avelo37-webhook-ingestion` (verify → external.persist → acl.translate → inbound.persist →
inbound.drain.deliver → event.store.append), binding the DeliveryJob aggregate + the three inbound
events.

## Alternatives considered

- **A federation registry like CoopCycle** — rejected: Uber Direct is one central API. A single
  endpoint + credentials is the right shape; federation config would be dead complexity.
- **The Uber Eats *marketplace* API** — out of scope: that is order *channelling* (and the separate
  price-comparison concern), not on-demand courier dispatch. This adapter is Uber **Direct** only.
- **Reuse the Stripe timestamped-HMAC verifier** — rejected: Uber's contract is a plain raw-body HMAC
  with no timestamp; forcing the timestamped scheme would reject valid Uber signatures.
- **A synchronous accept/decline on the create call** — rejected: Uber Direct is guaranteed-fulfilment
  with asynchronous status webhooks; acceptance is modelled from the first courier-assignment webhook,
  keeping the partner-generic inbound model intact.

## Consequences

### Positive
- Real courier coverage where Avelo37/CoopCycle don't operate; first true exercise of the multi-partner
  `CityDeliveryRanking` walk (Tours already ranks `uber_direct`).
- Proves the partner shape generalises to an **OAuth2, single-endpoint** provider with zero domain
  change — no new events/commands/errors/drain routing.
- Unconfigured deployments (V0 Tours) are unchanged: no `UBER_DIRECT_*`, the channel is unwired, offers
  time out and advance — today's behaviour.

### Negative
- The real Uber Direct wire contract (Create Delivery fields, webhook `kind`/status vocabulary, courier
  payload) is **assumed** from a best-effort reading — reconciliation against Uber's actual API is
  mapping-only, isolated to `acl.rs` / `outbound.rs`.
- Uber Direct's guaranteed-fulfilment model has no native "decline"; the pre-assignment-cancel →
  `DeliveryRejectedByPartner` mapping is a modelling choice that reconciliation may refine.

### Follow-up actions
- Reconcile the assumed wire shapes + OAuth scopes against Uber's API docs and a live sandbox on access.
- A `deliveryStatusChanged` subscription for live customer tracking (specs/integrations/uber-direct.md §6).
- Retire CoopCycle's pre-existing gap: `external_coopcycle_events` is not yet swept by
  `sweep_retention()` nor created by a migration (this ADR adds both for `external_uber_direct_events`).
