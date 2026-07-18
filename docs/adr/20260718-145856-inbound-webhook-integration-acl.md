# ADR-20260718-145856 — Inbound webhook integrations (Stripe/HubRise) via dedicated REST endpoints + ACL

## Status
Accepted. First adapter (Stripe) implemented in the same change; HubRise follows.

## Context
Third-party systems (Stripe payments, HubRise catalog/inventory) must feed Captain.Food. They are
**machines that PUSH** to us over REST/webhooks, each with **their own authentication** (Stripe request
signatures; HubRise its own token/OAuth) and **their own payload vocabulary**. `specs/events.yaml` already
models the relevant facts as **INBOUND integration events** — `PaymentCaptured`/`PaymentFailed`/
`PaymentRefunded` (Stripe) and `OfferStockUpdated`/`CatalogImported` (HubRise) — explicitly "recorded as
facts, NOT emitted by a command" (CLAUDE.md's request/report split: a reported fact that already happened
is an inbound event, not a rejectable command).

The question was where these land. Two wrong turns to avoid: (a) treating them as calls to
`/external/graphql` (our GraphQL is command/query — inbound *facts* have no mutation, and looping a webhook
back through our own HTTP surface is a pointless hop), and (b) a single generic API-key scheme (each
partner authenticates differently).

## Decision
Each integration gets a **dedicated REST webhook endpoint** with **partner-specific auth**, translated by
an **Anti-Corruption Layer** (the established pattern in `crates/infrastructure/src/integrations/`, cf.
`sirene.rs`, `google.rs`) into domain events, appended **in-process**:

```
Stripe/HubRise ──webhook (REST)──▶ POST /webhooks/{partner}   (crates/server; NOT the GraphQL surface)
                                       │ verify PARTNER-specific auth (signature / token) over the RAW body
                                       ▼ ACL: external payload → domain (keep external vocab out)
                                   inbound event (PaymentCaptured…) → event store        [idempotent by
                                   or, where orchestrated, a command (ImportCatalog) → handler   external ref]
```

- **Endpoint, not GraphQL.** `POST /webhooks/stripe`, `POST /webhooks/hubrise` — mounted alongside the
  other routes (like `/internal/sirene/drain`), never `/external/graphql`.
- **Partner-specific auth, fail-closed.**
  - **Stripe**: verify the `Stripe-Signature` header — `HMAC-SHA256(STRIPE_WEBHOOK_SECRET, "<t>.<rawBody>")`,
    constant-time compare, reject on `|now − t| > 300s` (replay window). Verify over the **raw body bytes**.
    Secret unset ⇒ 503.
  - **HubRise**: `X-HubRise-Hmac-SHA256` = **hex** `HMAC-SHA256(client_secret, raw_body)` (no timestamp),
    constant-time compare, `HUBRISE_WEBHOOK_SECRET` unset ⇒ 503. **Ingress shipped**; the domain ACL
    (→ `OfferStockUpdated`/`ImportCatalog`) is deferred because catalog/inventory callbacks carry no state
    and need an OAuth2 API pull + external-ref→domain-id mapping.
- **ACL translation.** External shapes never leak into the domain: Stripe minor-unit amounts → `Money`, etc.
- **In-process append.** Inbound facts append to `domain_events` via the existing event-store port (no
  command); the one orchestrated case (HubRise catalog) goes through the `ImportCatalog` command handler.
- **Idempotent** by the provider's event id (Stripe `evt_…`) / `ref`, so redelivered webhooks are no-ops.

`/external/graphql` (+ the EXTERNAL role, ADR-0047) is the **opposite direction** — an external system
*pulling* from our API — and is unaffected by this. The `X-External-Api-Key` mechanism serves only that
pull case and stays dormant until such a consumer exists.

## Alternatives considered
- **Route webhooks through `/external/graphql`** — rejected: inbound facts have no mutation; extra hop; wrong layer.
- **One generic `X-External-Api-Key` for all partners** — rejected: each partner authenticates differently
  (Stripe signs, HubRise tokenises); a shared key fits neither.
- **A separate ingestion service** — deferred: in-process is right for V0 (single server); the ACL boundary
  keeps a later extraction cheap.
- **Model payments as commands** — rejected: they are reported facts (Stripe already captured), per
  `events.yaml`/CLAUDE.md; a refund is *requested* by a command but the `PaymentRefunded` fact is inbound.

## Consequences
### Positive
- Correct layering (facts as inbound events), per-partner auth, no external vocab in the domain, idempotent.
- Reuses the existing integrations/ACL pattern and event store; adding a partner = one new endpoint + adapter.
### Negative / caveats
- Signature/token verification is security-critical and per-partner — must verify over the **raw** body and
  fail closed; a bug here is an auth bypass.
- New secrets to manage (`STRIPE_WEBHOOK_SECRET`, HubRise creds) as `sync:false` env.
- In-process coupling to the web instance (fine for V0; ACL boundary allows later extraction).

### Follow-up actions
- Implement the **Stripe** adapter (signature verify + ACL → the three payment events, idempotent). *(this change)*
- Implement the **HubRise** adapter (auth + ACL → `OfferStockUpdated` / `ImportCatalog`).
- Add the webhook secrets to `render.yaml` (`sync:false`).
- Decide replay/idempotency storage (dedupe table vs event-store natural key).

## References
`specs/events.yaml` (PaymentCaptured/Failed/Refunded, OfferStockUpdated, CatalogImported — inbound),
CLAUDE.md (command vs inbound-event rule), ADR-0035 (integrations/ ACL placement), ADR-0047 (the EXTERNAL
*pull* path, distinct from these). Adapters live in `crates/infrastructure/src/integrations/`.
