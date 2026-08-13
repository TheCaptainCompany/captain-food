# ADR-20260812-115930 — Each adapter owns its own, completely isolated database

**Status**: Accepted (founder directive, 2026-08-12) — **corrected 2026-08-12 by a full-roster mob**;
see "What this ADR got wrong" below.
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

What moves, measured against `specs/database/tables/` today — **six adapter databases**: the five
partner adapters that exist as `crates/adapters/*` crates, plus the SIRENE mirror.

| Adapter database | Tables it takes | Today's owner (spec) | The one app that connects |
|---|---|---|---|
| `adapter-stripe` | `external_stripe_events` | `integration_staging.yaml` | `adapter-stripe` |
| `adapter-hubrise` | `external_hubrise_callbacks` · `hubrise_connections` · `hubrise_connection_locations` | `integration_staging.yaml` · `integration_connections.yaml` | `adapter-hubrise` |
| `adapter-uber-direct` | `external_uber_direct_events` | `integration_staging.yaml` | `adapter-uber-direct` |
| `adapter-coopcycle` | `external_coopcycle_events` | `integration_staging.yaml` | `adapter-coopcycle` |
| `adapter-avelo37` | `external_avelo37_events` | `integration_staging.yaml` | `adapter-avelo37` |
| `adapter-sirene` | `external_sirene_restaurants` (the 655 MB mirror, [#231](https://github.com/TheCaptainCompany/captain-food/issues/231)) | `integration_staging.yaml` | `worker-sirene-sync` |

`auth_sessions` is **NOT** in the set — it stays platform state on `captain-write`. That is a
correction of this ADR's first version, argued below.

The set above is every table the spec itself marks ADAPTER-OWNED — the two files' own headers
already state the isolation intent this directive completes (*"deliberately reachable from NOTHING
else — never event-sourced, never referenced by api.yaml, never projected"*).

**One naming asymmetry, stated rather than hidden**: `adapter-sirene` has no `crates/adapters/sirene`
crate; the mirror's sole reader/writer is `sync_sirene_worker`, hosted by the `worker-sirene-sync`
CronJob (`crates/infrastructure/src/integrations/sync_sirene_worker.rs`), plus the offline
`crates/sirene_ingest` loader. It is in the set anyway, and the test it passes is the one that
matters: **exactly one app needs `CONNECT`, and it is the app that owns the mirror.** The database is
named for the partner data, not for a crate that must be invented to justify it.

## What this ADR got wrong, and why the error mattered

The first version of this ADR stated *"`crates/adapters/avelo37` exists and owns no table today; it
gets its database the day it gets state."* **That is false, and it was false when written.**
`specs/database/tables/integration_staging.yaml:178` declares `external_avelo37_events` (raw verified
Avelo37 delivery-partner webhook mirror, `avelo37_event_id` pk), and
`specs/database/functions/sweep_retention.sql:60` already sweeps its processed rows at 90 days from
`processed_at` — it is an established, live, retention-governed adapter table, not future state.

Left uncorrected, **avelo37 would have been the one partner mirror still holding `CONNECT` on the
write database while every sibling moved out** — Stripe, HubRise, Uber Direct and CoopCycle isolated,
and the delivery partner still inside the wall, writing its verbatim partner payloads (courier names
and phone numbers, per the table's own retention note) next to `domain_events`. That is precisely the
hole the directive closes, surviving in the single adapter nobody re-checked. It is also the failure
mode a records error produces in general: the decision was right and the *inventory* was wrong, so
the exception would have been implemented faithfully.

**The count did not move; the membership did.** The first version counted six adapter databases with
`adapter-identity` in and `adapter-avelo37` out. It is still six — with avelo37 in and identity out.
Anyone reading only the total would conclude nothing changed.

## Why `auth_sessions` stays platform (leg 2 redirected to (b))

The first version recommended an `adapter-identity` database, reasoning that *"`auth_sessions`
carries the same credentials-at-rest posture as `hubrise_connections`."* **That rationale is
inverted, and the option is unimplementable as named.** Three measurements:

- **The symmetry runs the other way.** `auth_sessions.ciphertext` is **AES-256-GCM** under the
  `AUTH_SESSION_KEY` secret (`crates/infrastructure/src/persistence/auth_sessions.rs:10,44,72`),
  while `hubrise_connections.access_token` is stored **plaintext `text`**
  (`specs/database/tables/integration_connections.yaml:46`, written verbatim by
  `crates/adapters/hubrise/src/connections.rs`). The encrypted table was to be moved for the
  posture of the plaintext one.
- **There is no such adapter.** `crates/adapters/` is exactly `avelo37 · coopcycle · hubrise ·
  stripe · uber_direct`, and `crates/bins/adapter-*` matches one-for-one. `adapter-identity` names
  nothing that exists.
- **Its two users are not adapters, and they are on the login hot path.** The park happens in the
  actor/mailbox path — `VerifyPhone` on the `Customer` aggregate (`specs/customer/actors.yaml:22`,
  `crates/application/src/commands.rs` ~3300) — and the claim happens in the BFF route
  `POST /auth/session` (`crates/server/src/auth_routes.rs:63-83`); a third writer, the retention
  sweep, deletes unclaimed rows (`specs/database/functions/sweep_retention.sql:74`). So the database
  would carry a `CONNECT` list of two-to-three **non-adapter** apps on the sign-in path — the
  inverse of BND-3's own stop condition, and the opposite of the one-app test every row of the table
  above passes.

**Decision: leg 2 (b)** — the five partner adapters plus sirene; `auth_sessions` stays platform on
`captain-write`.

### The dissent, recorded as the final-vision alternative it is

The GraphQL lens dissents, and the dissent is not a footnote. Under
[ADR-20260808-235113](ADR-20260808-235113-final-vision-first-no-intermediate-steps.md) the clean end
state is not "the table stays platform" but **an identity bin that genuinely owns it**: an
`adapter-identity` (or better-named `identity`) app owning `auth_sessions` *and* the
`/auth/session` · `/auth/refresh` · `/auth/logout` routes, at which point the `CONNECT` list is one
app again and the isolation argument becomes true rather than nominal. It would also give a home to
the [#385](https://github.com/TheCaptainCompany/captain-food/issues/385) precondition — those routes
have **no per-surface bin home today**; they live in the monolith `server`.

That is a **larger slice than a database split**: it moves HTTP routes, cookie handling and a
session-key grant into a new app on the sign-in path. **It is not taken now**, and it is not
foreclosed. What is rejected here is only the intermediate that was recommended — a database named
for a non-existent adapter, connected to by whoever happens to need it.

### What isolation is actually worth here — the finding that reframes it

`AUTH_SESSION_KEY` is granted to **53 of 56 pods**. Measured from
`specs/generated/apps.generated.md` §5: every grant group carries it except the **three** periodic
workers (`worker-erasure` · `worker-retention` · `worker-sirene-sync`), which the
`worker_key_allowed` narrowing (`tools/codegen-rs/src/emit/bins.rs:136`, applied at
`tools/codegen-rs/src/emit/deploy.rs:187-189`) cuts to the database + telemetry floor. **This was
first recorded as "53 of 57, every group but the four periodic workers", and the correction makes
the finding WORSE, not better**: retiring `command_journal`
([#242](https://github.com/TheCaptainCompany/captain-food/issues/242) Runtime D) deleted
`worker-journal-sweep`, which was one of the four EXCLUDED workers — so the denominator fell while
the numerator did not, and the share of pods holding a key only two need rose from 53/57 to 53/56. A
smaller denominator here is not progress. Exactly **two** components decrypt a session. Isolating a
table into its own database while broadcasting its decryption key to 53 pods buys much less than it
looks like it buys — the cheap,
large win is narrowing the grant, already tracked as
[#491](https://github.com/TheCaptainCompany/captain-food/issues/491) slice A4. This does not argue
against isolation generally; it argues that for *this* table the grant surface, not the database
wall, is where the risk lives.

## Why the mailbox front door stands (leg 1 confirmed (a))

The adapter's outward grant is `INSERT` into `inbound_messages` on `captain-write` — the existing
ACL/mailbox contract. The strict reading (an outbox inside the adapter database, drained by a
platform relay) is **rejected**, and for two reasons stronger than the "one fewer component"
argument already on the register row:

- **It inverts the isolation it is meant to buy.** An outbox in the adapter database must be `SELECT`ed
  and marked `UPDATE`d by a **platform relay**, so a non-adapter component ends up holding a
  **bidirectional** grant **inside** every adapter database. That trades a one-way, outward `INSERT`
  for read+write access held by the one class of component the wall exists to keep out. (DBA lens.)
- **`LISTEN`/`NOTIFY` is per-database in Postgres.** A push-driven relay therefore needs a live
  connection **into** each adapter database — an inward hole through the wall, six times over — and
  the only alternative is a permanent poll with no path back, which
  [ADR-20260810-231300](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md)
  forbids: it is neither a declared degraded mode nor one anything detects recovery from.
  (Observability lens.)

## What "completely isolated" means, stated so it is checkable

- **Inward**: no role other than the database's one owning app holds `CONNECT` on `adapter-{name}`.
  No projector, no `graphql_*`, no `admin_ro` business path — the standing exception list
  (`claude_ro` incident tooling) applies only if a register row grants it per BND-9's exception
  mechanism.
- **Outward**: the adapter's role holds `CONNECT` on its own database **plus the one sanctioned
  door** — `INSERT` into `inbound_messages` (`captain-write`), which is how an external fact enters
  the domain (the ACL/mailbox contract, CQRS doctrine: inbound facts are recorded, never commanded).
  Confirmed as leg 1 (a) above.
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
- **Retention forks**: `sweep_retention()` legs that touch adapter tables run per-adapter-database —
  including the `external_avelo37_events` leg, which the first version of this ADR did not know about.
- **`auth_sessions` keeps its current home**, so no migration, no new chain, and no change to the
  sign-in path — the sweep leg that deletes unclaimed rows stays where it is.
- **What it buys**: a partner webhook flood or replay storm bloats and locks its own database, never
  the domain's; credentials at rest are per-partner by construction; offboarding a partner is
  `DROP DATABASE`; and a per-partner GDPR/erasure sweep is database-scoped.

## Not decided here

Sequencing relative to the five-database split itself stays in
[DECISIONS §32 row **ADP-1**](../proposals/DECISIONS.md) (it lands inside the #494 program). Both
confirm-or-redirect legs are now closed: leg 1 as (a), leg 2 as (b). The identity-bin dissent above
is an open design option, not an open leg of this decision.

## Consulted

Per [ADR-20260812-143619](ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md),
the correction pass names its lenses: **architect** (leg 2 unimplementable — no such crate),
**dba** (the encryption asymmetry; the outbox grant inversion), **observability**
(`LISTEN`/`NOTIFY` is per-database; the 53-of-57 key grant), **graphql-architect** (the identity-bin
dissent and the #385 route home), **holub**, **farley**, **beck**, **business-specialist**,
**legal-specialist**, **ux-designer** (avelo37 inventory error, sequencing, retention leg, PII in
partner payloads).
