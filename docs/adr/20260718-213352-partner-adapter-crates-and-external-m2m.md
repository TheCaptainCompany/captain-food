# ADR-20260718-213352 — Partner adapters as self-contained crates; `/external/graphql` as the M2M standard

## Status
Accepted. Realized for Stripe + HubRise in the same change (their webhook code moved to `crates/adapters/*`).
Amends the crate layout of ADR-0035 and standardizes the `/external` M2M path of ADR-0006/0047.

## Context
External systems interact with Captain.Food in **two opposite directions**, and they were scattered:
partner webhook ACLs lived in `crates/infrastructure/src/integrations/` while their HTTP endpoints lived
in `crates/server` — so "the Stripe adapter" was split across two crates, and every partner shared the
`infrastructure` + `server` blast radius. We also want the option to **deploy a partner's endpoint on its
own web service** (e.g. Stripe separate from HubRise) for isolation and independent scaling.

Two directions to keep distinct:
- **Inbound push (webhooks):** the *partner* POSTs *their* events to us, authenticated by *their* scheme
  (Stripe request signature, HubRise HMAC). This is an Anti-Corruption-Layer concern — adapt external
  shapes to domain events — **not** domain, and **not** our GraphQL surface.
- **Inbound pull/drive (M2M API):** an *external entity* calls *our* API to query/mutate/(subscribe),
  authenticated by *our* credential. This is the `EXTERNAL` role path (ADR-0006/0047).

## Decision
**1. One self-contained crate per partner adapter**, under `crates/adapters/<partner>/`:
- Each crate holds the whole vertical slice — the **framework-free ACL** (`acl.rs`: signature
  verification + external→domain mapping + any ingestor over the `application` ports), the **thin axum
  HTTP shell** (`http.rs`: `pub fn routes(...) -> Router`), and a **standalone binary** (`main.rs`) that
  serves only that partner's endpoint. So a partner can be **mounted into the monolith** (`server` calls
  `<crate>::routes(...)`) **or deployed as its own web service** — clean partner isolation.
- Adapters are **not** part of `domain` or `infrastructure`. They obey the dependency rule (they depend
  inward on `application`/`domain`, and on `infrastructure` only where a standalone binary needs a concrete
  adapter like `PgEventStore`). The ACL stays framework-free (unit-tested without axum).
- Realized now: `crates/adapters/stripe` (ACL + `POST /adapters/stripe/webhooks`, idempotent inbound payment facts)
  and `crates/adapters/hubrise` (ACL + `POST /adapters/hubrise/webhooks`, verified ingress). The SIRENE/Google/
  Supabase-auth seams stay in `infrastructure/integrations/` for now (scheduled pull / outbound seams, not
  partner-push webhooks); they may follow this pattern later if it helps.

**2. `/external/graphql` is the standard M2M inbound API.** External entities query/mutate (and, later,
subscribe) through the one master schema under the `EXTERNAL` role path (ADR-0006), authenticated
machine-to-machine by an **API key** (`X-External-Api-Key`, ADR-0047 — keep it, no longer "dormant").
*What* an external entity may do is spec-driven per-operation by `roles: [EXTERNAL, …]` in `api.yaml`.

## Alternatives considered
- **Keep ACLs in `infrastructure` + endpoints in `server`** (status quo) — rejected: a partner is split
  across crates, can't deploy standalone, shares everyone's blast radius.
- **One shared `adapters` crate for all partners** — rejected: couples all partners together and to axum;
  defeats the "deploy Stripe separately from HubRise" goal.
- **Route webhooks through `/external/graphql`** — rejected (ADR-20260718-145856): inbound *facts* have no
  mutation and use the partner's own auth; that path is the *pull* direction.

## Consequences
### Positive
- A partner is **one folder**, **independently deployable**, isolated blast radius; the ACL stays pure and
  unit-tested; the monolith still mounts everything for the single-service deployment today.
- Clear two-direction model: `crates/adapters/*` (partner push) vs `/external/graphql` (external drive).
### Negative / caveats
- A partner crate that ships a standalone binary pulls heavier deps (Stripe's `main.rs` needs
  `infrastructure` for `PgEventStore`); acceptable for a self-contained service.
- Separate deployment needs new `render.yaml` services (not created yet — single service still mounts all).
- **Subscriptions are not built**: the schema is `EmptySubscription`. "subscribe" needs a real
  `SubscriptionRoot` + WebSocket transport + `api.yaml` subscription defs (a DSL change → plan mode).
- The M2M API key is a **shared list** (`EXTERNAL_API_TOKENS`); per-partner keys + scopes/rate-limits are
  future hardening.

## Follow-up actions
- HubRise **domain enrichment** (OAuth2 API pull → `OfferStockUpdated`/`ImportCatalog` + ref-mapping).
- If/when a partner deploys separately: add its `render.yaml` service (its `main.rs` binary).
- Subscriptions on `/external/graphql` (SubscriptionRoot + WS + `api.yaml`).
- Per-partner API keys + scopes for the M2M path.

## References
Amends ADR-0035 (crate layout). Builds on ADR-20260718-145856 (inbound webhook pattern), ADR-0006
(role-as-path), ADR-0047 (Supabase JWT + `X-External-Api-Key`). Adapters: `crates/adapters/{stripe,hubrise}`.
