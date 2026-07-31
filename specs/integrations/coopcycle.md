# CoopCycle (Delivery Partner) Integration

CoopCycle is Captain.Food's **cooperative** delivery partner (issue #58) — the open-source,
cooperatively-owned bike-delivery federation. Captain **dispatches** ready DELIVERY orders to a co-op
and **receives** courier/status facts back. All translation goes through the **`coopcycle-acl`**
Anti-Corruption Layer (c4-l3), which keeps the partner API out of the domain — mirroring `avelo37-acl`
and `stripe-adapter`. See [ADR-0031](../../docs/adr/0031-delivery-bounded-context.md) and
[ADR-20260721-122910](../../docs/adr/20260721-122910-coopcycle-delivery-partner-adapter.md).

CoopCycle is the **third** `DeliveryProvider = PARTNER` implementation of the pattern established by
Avelo37 ([specs/integrations/avelo37.md](avelo37.md), issue #28): the two-layer webhook inbox
(`external_coopcycle_events` mirror → kind-EVENT `inbound_messages` on the `DeliveryJob` lane → mailbox delivery onto the `DeliveryJob` stream, ADR-20260731-122500), the
outbound `DeliveryService` gateway, and the fail-closed config gate. The delta is the **federation
dimension** below.

## 1. Direction of flow

| Direction | Trigger | ACL action |
|---|---|---|
| **Out** (request) | domain event `DeliveryRequested` (emitted by `DeliveryDispatchProcess` when a DELIVERY order is ready) | Resolve the job to a co-op **instance** (§5), then call THAT instance's API to create a delivery/task (pickup = restaurant, dropoff = customer), authenticated with the instance's OAuth2 token. |
| **In** (report) | CoopCycle instance webhooks (per-instance secret) | Translate to the INBOUND facts below and record them (no command — nothing to reject). |

## 2. Inbound facts (📥 — reported, not requested)

Recorded as-is (the request/report split, CLAUDE.md). **Idempotent on `partnerRef`** (the co-op-side
delivery/task id). These are the SAME partner-generic events Avelo37 produces — no new events.

- **`DeliveryAcceptedByPartner`** — the co-op accepted and assigned one of its couriers.
  Carries `partnerRef`, `courier { displayName, phone? }` (no `riderId` — not a Captain rider), and ETAs.
- **`DeliveryRejectedByPartner`** — the co-op declined; `DeliveryDispatchProcess` re-offers (another
  partner / independent riders) or flags for manual handling. The job stays `PENDING`.
- **`DeliveryStatusUpdated`** — status progression (`PICKED_UP`, `OUT_FOR_DELIVERY`, `DELIVERED`, `FAILED`).
  On `DELIVERED`, `DeliveryDispatchProcess` emits `OrderDelivered` to close the order.

## 3. Mapping → domain

| CoopCycle concept | Captain domain |
|---|---|
| task/delivery id | `partnerRef` (scalar `ExternalReference`; idempotent key) |
| courier name / phone | `Courier { displayName, phone }` (no `riderId`) |
| pickup / dropoff | `DeliveryRequested.pickup` / `.dropoff` (`Address`) |
| task status | `DeliveryStatus` enum (mapped in the ACL) |
| co-op instance | `instance_id` on `external_coopcycle_events` (federation origin) — NOT a domain concept |

## 4. ACL / boundary rules

- The co-op's raw status strings and payloads never leak into the domain — the ACL maps them to
  `DeliveryStatus` and the typed inbound events.
- Recording is **idempotent**: the staging pk is `"{instance_id}:{provider event id}"` (federation
  makes provider ids unique only *per instance*, so the ACL namespaces the key). Duplicate webhooks
  are safe.
- The dynamic delivery cost is settled at checkout into `PaymentBreakdown` (ADR-0028); this
  integration does not re-price.

## 5. Federation dimension (the CoopCycle-specific concern)

CoopCycle is **many self-hosted co-op instances** (per city/co-op), NOT one central API. This is the
one thing that distinguishes it from Avelo37's single static endpoint:

- **Per-instance registry.** The adapter config is an *instance registry*, not a single endpoint:
  each instance declares `{ id, base_url, oauth { client_id, client_secret, token_url }, webhook_secret,
  coverage }`. Configured out-of-repo via the `COOPCYCLE_INSTANCES` env var (JSON); unset ⇒ the no-op
  stand-in (fail-closed), exactly like `AVELO37_API_KEY`.
- **Outbound resolution.** `offer_job` resolves a job to an instance by **coverage area** — the
  dropoff postal-code prefix matches an instance's declared `coverage`. No covering instance ⇒
  fail-closed (`DomainError::Repository`), surfaced on `/saga`; the job stays open to independent riders.
- **OAuth2.** Each instance authenticates with **OAuth2 client-credentials** (token fetch + refresh,
  cached per instance) — unlike Avelo37's static bearer key. Mirrors the HubRise OAuth client shape.
- **Inbound routing.** Webhooks arrive per-instance at `POST /adapters/coopcycle/{instance}/webhooks`;
  the `{instance}` path segment selects that instance's `webhook_secret` for signature verification and
  is recorded as `instance_id` on the mirror row.

## 6. Gaps / deferred (runtime)

1. The actual CoopCycle API/webhook contract (task payload, OAuth scopes, status vocabulary, retry) —
   confirmed with a partnering co-op instance; the ACL maps a documented best-effort shape until then.
2. **Multi-partner ranking** — choosing *between* Avelo37 / CoopCycle / Uber on the re-offer leg is the
   shared foundation issue (ADR-20260720-004556 named the extension point); until it lands, CoopCycle
   plugs in as the configured partner (like Avelo37 today), not selected *among* others.
3. A partnering co-op instance + API credentials is the external gate.
4. Independent-rider onboarding/auth (RIDER) is a separate concern from this partner ACL.
