# Uber Direct (Delivery Partner) Integration

Uber **Direct** is Captain.Food's on-demand delivery partner (issue #57) — the Uber **delivery** API
that dispatches a ready DELIVERY order to an Uber courier and reports courier/status facts back. This
is the delivery API, **not** the Uber **Eats marketplace**, and is distinct from the Uber price-
comparison concern (ADRs 0022/0023/0024/0030). All translation goes through the **`uber_direct-acl`**
Anti-Corruption Layer (c4-l3), which keeps the partner API out of the domain — mirroring
`avelo37-acl`, `coopcycle-acl` and `stripe-adapter`. See [ADR-0031](../../docs/adr/0031-delivery-bounded-context.md)
and [ADR-20260721-172500](../../docs/adr/20260721-172500-uber-direct-delivery-partner-adapter.md).

Uber Direct is a `DeliveryProvider = PARTNER` implementation of the pattern established by Avelo37
([specs/integrations/avelo37.md](avelo37.md), issue #28): the two-layer webhook inbox
(`external_uber_direct_events` mirror → `inbound_events` → drain onto the `DeliveryJob` stream), the
outbound `DeliveryService` gateway, and the fail-closed config gate. The two deltas from Avelo37 are
the **OAuth2 client-credentials** auth (§5) and the **signature scheme** (§4).

## 1. Direction of flow

| Direction | Trigger | ACL action |
|---|---|---|
| **Out** (request) | domain event `DeliveryRequested` on the `uber_direct` channel (the dispatch saga offers it, #60) | Fetch/refresh the OAuth2 token, then POST Uber's **Create Delivery** (pickup = restaurant, dropoff = customer), carrying our `deliveryJobId` as `external_id`. |
| **In** (report) | Uber Direct delivery-status webhooks (`X-Uber-Signature`) | Translate to the INBOUND facts below and record them (no command — nothing to reject). |

## 2. Inbound facts (📥 — reported, not requested)

Recorded as-is (the request/report split, CLAUDE.md). **Idempotent on the Uber `event_id`**. These
are the SAME partner-generic events Avelo37/CoopCycle produce — no new events.

- **`DeliveryAcceptedByPartner`** — a courier was assigned (status `pickup`, courier present).
  Carries `partnerRef`, `courier { displayName, phone? }` (no `riderId` — not a Captain rider), and ETAs.
- **`DeliveryRejectedByPartner`** — Uber could not fulfil the delivery (status `canceled`/`returned`
  with **no** courier ever assigned); `DeliveryDispatchProcess` advances the ranked walk (#60).
- **`DeliveryStatusUpdated`** — status progression (`pickup_complete` → PICKED_UP, `dropoff` →
  OUT_FOR_DELIVERY, `delivered` → DELIVERED, post-assignment `canceled`/`returned` → CANCELLED/FAILED).

## 3. Mapping → domain

| Uber Direct concept | Captain domain |
|---|---|
| `data.id` (delivery id) | `partnerRef` (scalar `ExternalReference`; idempotent read-back) |
| `data.external_id` | OUR `DeliveryJobId`, echoed back from the outbound Create Delivery |
| `data.courier` name / phone | `Courier { displayName, phone }` (no `riderId`) |
| `data.status` | `DeliveryStatus` enum (mapped in the ACL) |
| pickup / dropoff | `DeliveryRequested.pickup` / `.dropoff` (`Address`) |

## 4. ACL / boundary rules — the signature DELTA

- Uber's raw status strings and payloads never leak into the domain — the ACL maps them to
  `DeliveryStatus` and the typed inbound events.
- **Signature.** Unlike the Stripe-style timestamped `t=…,v1=…` scheme Avelo37/CoopCycle adopted,
  Uber signs with a plain **`X-Uber-Signature: hex(HMAC-SHA256(webhook_secret, raw_body))`** — no
  timestamp, so no replay window. Verification is a constant-time compare over the raw body; failure
  is fail-closed (`400`).
- Recording is **idempotent** on the Uber `event_id` (globally unique on Uber's side, so — unlike
  CoopCycle's federation — no instance namespacing is needed).

## 5. OAuth2 (the auth DELTA)

Uber Direct authenticates outbound calls with **OAuth2 client-credentials** (token fetch + refresh,
cached in the gateway), unlike Avelo37's static bearer key. Config is a single endpoint + credentials
(not CoopCycle's per-instance registry), all env-gated:

- `UBER_DIRECT_CUSTOMER_ID` — the `{customer_id}` path segment on Create Delivery,
- `UBER_DIRECT_CLIENT_ID` / `UBER_DIRECT_CLIENT_SECRET` — the OAuth2 client,
- `UBER_DIRECT_WEBHOOK_SECRET` — the `X-Uber-Signature` verification secret,
- optional `UBER_DIRECT_BASE_URL` / `UBER_DIRECT_TOKEN_URL` / `UBER_DIRECT_SCOPE` overrides.

All four required vars unset ⇒ the composition root keeps the no-op stand-in (the `uber_direct`
channel is simply unwired, so the dispatch saga's offer times out and advances to the next ranked
channel — V0 Tours without Uber Direct is unchanged). A PARTIALLY-set config is a misconfiguration
surfaced as an error, not a silent no-op.

## 6. Gaps / deferred (runtime)

1. The exact Uber Direct wire contract (Create Delivery request/response fields, webhook `kind`/status
   vocabulary, courier payload) — confirmed against Uber's API docs and a live sandbox; the ACL maps a
   documented best-effort shape until then.
2. A commercial agreement + Uber Direct API access (customer id + OAuth client + webhook secret) is the
   external gate.
3. **Multi-partner ranking** between Avelo37 / CoopCycle / Uber Direct is already the shared dispatch
   foundation (#60, ADR-20260721-161939): Uber Direct plugs in as the `uber_direct` channel in the
   `CityDeliveryRanking` walk (the Tours seed already ranks it), selected among the others with no
   further saga change.
