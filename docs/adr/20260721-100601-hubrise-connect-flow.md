# ADR-20260721-100601 — HubRise connect flow: adapter-orchestrated provisioning + account-scoped token store

## Status

Accepted (issue #20; closes the "Open contract" gap of ADR-20260718-145856 §0 / STATUS 2a).

## Context

The HubRise enricher (ADR-20260718-145856/-213352) stood on two temporary assumptions: the
`RestaurantAccount`/`Restaurant`/`Catalog` aggregates already exist under the ACL's derived UUIDv5
ids (nobody provisioned them — `ImportCatalog` skipped `CatalogNotFound` forever), and one global
`HUBRISE_ACCESS_TOKEN` served all pulls — a single-tenant hack where a leaked token exposes every
connected account. A HubRise OAuth connection is authorized against an **Account** and returns a
**non-expiring, account-scoped** token (confirmed against the HubRise auth docs: the token response
itself carries `account_id`/`location_id`/`catalog_id` + names; scope format `account[...]` vs
`location[...]`; no refresh tokens). HubRise Account ⇔ our `RestaurantAccount`, Location ⇔
`Restaurant`, 1:1 by design.

## Decision

1. **Adapter-orchestrated connect flow, no process manager, no new domain messages.** Two routes on
   the HubRise adapter: `GET /adapters/hubrise/connect` (302 to the HubRise authorize page with a
   stateless HMAC-signed anti-CSRF `state`, 15-min validity) and `GET /adapters/hubrise/oauth/callback`
   (exchange the code → pull `/account`, `/locations`, `/catalogs` → provision → store the token →
   initial import). Provisioning reuses the EXISTING rejectable commands — `RegisterRestaurantAccount`,
   `RegisterRestaurant` per location (`listingStatus: PASSIVE_PARTNER`, slug =
   `slugify(name)-slugify(location id)`, `ref` = the HubRise location id), `CreateCatalog` per catalog
   — with the enricher's derived UUIDv5 ids supplied in the payloads (commands.yaml: aggregate ids are
   client/ACL-generated). Creation handlers are idempotent on an existing id, so a **re-connect** is a
   token refresh + location catch-up + catalog re-import (replace semantics), never a duplicate.
   Deterministic rejections are collected as warnings, never retried (the SIRENE lesson).
2. **Account-scoped token store, adapter-owned** (`specs/database/tables/integration_connections.yaml`,
   a new `tables/*.yaml` category file): `hubrise_connections` (pk `restaurant_account_id` =
   UUIDv5(account id), `hubrise_account_id` UNIQUE, `access_token`, connected/last-connected stamps)
   + `hubrise_connection_locations` (callback→token resolution: HubRise callbacks carry a
   `location_id`, not the account id). The token is a **credential, not a business fact** — never in
   `domain_events` (the `payment_process_manager.client_secret` stance), never referenced by api.yaml,
   so no GraphQL edge can reach it (nothing for #22's `navRoles` to even guard).
3. **The global env token is retired.** `HubRiseApiClient` (token-holding) became `HubRiseApi`
   (token-per-call); the enricher resolves `callback.location_id → hubrise_connections.access_token`
   per callback and skips definitively when the location is unconnected. Enrichment + connect now
   need only `DATABASE_URL`; the connect routes additionally need `HUBRISE_CLIENT_ID` +
   `HUBRISE_WEBHOOK_SECRET` (the app client secret, already the webhook HMAC key) +
   `HUBRISE_CONNECT_REDIRECT_URL` (+ optional `HUBRISE_OAUTH_SCOPE`, default
   `account[catalog.read,inventory.read]`), checked per request fail-closed.
4. **Every provisioning send is journaled** (WORKER channel, ADR-20260720-015300): `message_id` =
   UUIDv5(attempt, command type, entity id), `correlation_id` = the per-callback attempt id — one
   connect's whole fan-out shares a correlation and leaves a durable trace, rejections included.
5. **Initial import at connect**: after `CreateCatalog`, the flow pulls each catalog and dispatches
   `ImportCatalog` through the same ACL mapping the callback enrichment uses — onboarding completes
   without waiting for the first HubRise callback. Because `create_catalog` guards on the Restaurant
   READ MODEL (async projection), the flow polls `by_id` briefly (≤10 s) before creating; a miss is a
   warning, healed by re-connect or the next catalog callback.

## Alternatives considered

- **Process manager + `services.yaml` port** — a `catalog_sync` service and a typed-step PM reacting
  to an inbound "connect authorized" event. Rejected for V0: the OAuth callback is an HTTP redirect
  (not a business message), the domain never calls HubRise here (only the adapter does), and the PM
  would force a new service port, state table, inbound event, tests and story steps for what is an
  integration-boundary orchestration. Revisit if connect grows business behaviour (e.g. approval).
- **New `ConnectHubRise` command + `HubRiseConnected` event** — would put connection lifecycle in the
  event log without an owning aggregate, and the token still could not ride the event. Deferred until
  a restaurant-facing UI needs a "connected" read model.
- **Keeping `HUBRISE_ACCESS_TOKEN` as fallback** — rejected: the issue's point is retiring the
  single-tenant hack; a fallback would silently mask unconnected locations.

## Consequences

### Positive
- Self-serve onboarding for HubRise-equipped restaurants: one authorize click provisions the account,
  its locations, its catalogs (imported), and stores the token — the enricher's assumptions become
  guarantees. Multi-account inventory sync works; a leaked token exposes one account, not all.

### Negative
- Token at rest in plaintext in Postgres (like other secrets in env today); encryption-at-rest is a
  follow-up. A location added on HubRise AFTER connect resolves no token until a re-connect refreshes
  the snapshot (documented skip).

### Follow-up actions
- Restaurant-facing connect UI + GraphQL surface (needs restaurant screens; likely a "connected"
  read model / `HubRiseConnected` fact at that point).
- Disconnect/revoke flow (`POST /oauth2/v1/revoke`) + token encryption at rest (#22-adjacent).
- Confirm `GET /catalogs` and the `opening_hours` wire shape against the live API (opening hours are
  currently left empty on provisioning); map `order`/`location` callbacks later.
