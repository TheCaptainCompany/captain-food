# ADR-20260812-115930 — Each adapter owns its own, completely isolated database

**Status**: Accepted (product-owner directive, 2026-08-12)
**Amends**: [PROP-20260811-093000 "Storage boundaries and least-privilege database users"](../proposals/PROP-20260811-093000-storage-boundaries-and-least-privilege-database-users.md) §11/§13 · [DECISIONS §32 STO-2](../proposals/DECISIONS.md)
**Tracking**: [#494 "Storage boundaries and least-privilege database users: the write-side transactional unit, the five-database split, and the last five View_*"](https://github.com/TheCaptainCompany/captain-food/issues/494)

## Directive

Verbatim, 2026-08-12: *"Each adapter must have there own database completely isolated."*

## Decision

Every integration adapter's owned state leaves the shared business databases and lands in a
**database of its own**, one per adapter, reachable by **that adapter's role and nothing else**.
This supersedes the part of PROP-20260811-093000 §11 that folded *"integration staging"* into
`DomainCommonDb`, and answers the staging/connections leg of register row **STO-2** with a stronger
shape than the one recommended there.

What moves, measured against `specs/database/tables/` today:

| Adapter database | Tables it takes | Today's owner (spec) |
|---|---|---|
| `adapter-stripe` | `external_stripe_events` | `integration_staging.yaml` |
| `adapter-hubrise` | `external_hubrise_callbacks` · `hubrise_connections` · `hubrise_connection_locations` | `integration_staging.yaml` · `integration_connections.yaml` |
| `adapter-uber-direct` | `external_uber_direct_events` | `integration_staging.yaml` |
| `adapter-coopcycle` | `external_coopcycle_events` | `integration_staging.yaml` |
| `adapter-sirene` | `external_sirene_restaurants` (the 655 MB mirror, [#231](https://github.com/TheCaptainCompany/captain-food/issues/231)) | `integration_staging.yaml` |
| `adapter-identity` | `auth_sessions` (encrypted session parking, #112) | `integration_connections.yaml` |

`crates/adapters/avelo37` exists and owns **no table today**; it gets its database the day it gets
state, not before. The set above is every table the spec itself marks ADAPTER-OWNED — the two files'
own headers already state the isolation intent this directive completes (*"deliberately reachable
from NOTHING else — never event-sourced, never referenced by api.yaml, never projected"*).

## What "completely isolated" means, stated so it is checkable

- **Inward**: no role other than `adapter_{name}` holds `CONNECT` on `adapter-{name}`. No projector,
  no `graphql_*`, no `admin_ro` business path — the standing exception list (`claude_ro` incident
  tooling) applies only if a register row grants it per BND-9's exception mechanism.
- **Outward**: the adapter's role holds `CONNECT` on its own database **plus the one sanctioned
  door** — `INSERT` into `inbound_messages` (`captain-write`), which is how an external fact enters
  the domain (the ACL/mailbox contract, CQRS doctrine: inbound facts are recorded, never commanded).
  The door is a **team-confirmable leg**, not settled here: the strictest reading (adapter touches
  ONLY its own database, an in-adapter outbox drained by a platform relay) is named in the register
  row so choosing the door is visible, not defaulted.
- **Databases, not clusters**: the adapter databases live in the shared business CNPG cluster.
  STO-3 already measured per-database clusters as unaffordable on the node (5 × 512 Mi that does not
  exist) and rejected the multiplied WAL timelines; the isolation mechanism is the role + `CONNECT`
  grant (BND-3's answer), which is exactly the mechanism this directive strengthens.

## Consequences

- The §11 placement map is amended in the same change; the grant emitter (#491/REP-5 lineage) gains
  six databases whose grant block is one line each — which is the point.
- **STO-4's pool arithmetic grows again**: each adapter bin holds its own pool plus the mailbox
  pool. The pooler-before-split sequencing STO-4 recommends becomes harder to defer, not easier.
- **Six more migration chains** and six `REQUIRED_SCHEMA_VERSION` entries; the drill gains legs.
  Mitigation is the same as the proposal's: staging state is re-derivable from the partner, so
  adapter databases are candidates for the replay/refetch posture, **except the credential tables**
  (`hubrise_connections` is a NON-expiring token that only a human re-connect can replace) — the one
  non-rederivable adapter state, and therefore the one adapter table that must be in a backup story.
- **Retention forks**: `sweep_retention()` legs that touch adapter tables run per-adapter-database.
- **What it buys**: a partner webhook flood or replay storm bloats and locks its own database, never
  the domain's; credentials at rest are per-partner by construction; offboarding a partner is
  `DROP DATABASE`; and a per-partner GDPR/erasure sweep is database-scoped.

## Not decided here

The confirm-or-redirect legs live in [DECISIONS §32 row **ADP-1**](../proposals/DECISIONS.md):
the adapter set (is `adapter-identity` in scope of "adapter"?), the mailbox door vs the outbox+relay
strict reading, and sequencing relative to the five-database split itself.
