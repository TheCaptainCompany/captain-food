# ADR-20260728-011344 — The slug is an owner-chosen lifecycle, and SIRENE is an inbound event

## Status

Accepted

<!-- Realizes PROP-20260728-004616 (tracking issue #220). Accepted: the product owner answered D1 in
     session and D2-D6 on 2026-07-28. Partially supersedes ADR-0045 (SIRENE -> RegisterRestaurant via
     the command path). Realized incrementally by the proposal's §10 sub-issues. -->

## Context

A Supabase "depleting Disk IO Budget" alert on the `captain-food` project led to a trace of the SIRENE
write path. The IO was a symptom. Three defects sat underneath it, all traceable to one decision — that
the restaurant **slug is derived at seeding time** from the INSEE name plus the NIC
(`crates/infrastructure/src/integrations/sirene.rs:215-216` → `chez-marco-00021`).

1. **A failed INSERT was the idempotency mechanism.** `register_restaurant` never rehydrated the
   aggregate: it hard-coded `expected_version = 0` (`crates/application/src/commands.rs:365`) and let a
   `UNIQUE (stream_name, version)` violation answer *"does this restaurant already exist?"*, which
   `idempotent_on_existing` (`:160-166`) then laundered into `Ok(())`. Postgres writes the heap tuple
   *before* the constraint fires, so each weekly sweep left ~200k dead tuples in `domain_events` and its
   three indexes. Six creation handlers shared the pattern; `verify_phone` (`:3074`) reported
   `created: true` after a swallowed conflict.
2. **INSEE updates were silently discarded.** No `UpdateRestaurant` existed in the SIRENE worker at all,
   so a renamed or relocated établissement refreshed the staged payload, built a `RegisterRestaurant`,
   conflicted, was swallowed as success, and was marked processed. The mirror updated; the domain never
   did. Because a swallowed conflict is indistinguishable from a successful write, nothing surfaced it.
3. **The write path consulted the read side, unindexed.** `by_external_identifier` ran
   `external_identifiers @> $1` (`crates/infrastructure/src/persistence/restaurant.rs:39-43`) against the
   eventually-consistent `Restaurant` projection. That column is `JSONB` with **no GIN index** anywhere in
   the generated schema, so it was a sequential scan of the whole table — every JSONB column included —
   once per staged SIRET.

The derived slug is the common root. It is the **tenant host** (`{slug}.captain.food`, wildcard DNS),
i.e. identity: deriving it at seeding reserved ~200k hostnames no merchant would choose, for businesses
that never opted in; the NIC only disambiguates *within* a company, so generic names on the common
`00019`/`00021` establishment numbers collided across different SIREN (the 605-row `SlugAlreadyTaken`
storm was the derivation, not bad luck); and it made identity a function of INSEE's mutable
`denominationUsuelle`.

Full option space, trade-offs and diagrams:
[PROP-20260728-004616](../proposals/PROP-20260728-004616-slug-lifecycle-and-sirene-inbound-events.md).

## Decision

Two coupled changes. **The slug change lands first** — fixing the update path before it would turn an
INSEE rename into a request to rename a *live storefront*, breaking printed menus, QR codes, SEO and the
GBP "order online" link.

### 1. The slug becomes an owner-chosen lifecycle

- `RestaurantRegistered` **carries no slug.** The slug arrives via **`RestaurantSlugConfigured`** (first
  configuration) and **`RestaurantSlugReconfigured`** (a rename, carrying **`previousSlug`** as business
  data). *(D1, decided in session.)* Per ADR-0041 the acting user and timestamp stay envelope metadata on
  `domain_events.user_id` / `occurred_at`, so "who chose this address and when" needs no payload field.
- A separate event rather than an optional field: `Option<Slug>` on a creation event conflates "not chosen
  yet" with "has none", forces every consumer to handle null, and leaves the moment of choosing with no
  record and nothing for a policy to react to.
- `previousSlug` feeds a **slug-alias read model** so `crates/server/src/hosts.rs` can serve a **301** from
  the superseded host. Folding history would also yield it, but host resolution runs on every request and
  must not fold.
- **The owner chooses the slug between claim and activation** *(D2)* — a dedicated onboarding step after
  ownership is verified, gated by a new invariant: **no activation without a configured slug**. This keeps
  `ActivateRestaurant` a pure lifecycle transition rather than a form submission, and lets an owner secure
  their address before going live. The gate is **aggregate-local**: `activate_restaurant` folds its own
  stream and sees whether `RestaurantSlugConfigured` happened — no read model consulted.
- **Uniqueness is enforced by a write-side reservation table with a real `UNIQUE` constraint** *(D3)*. The
  database is the arbiter, race-free, with no projection involved. It also holds **released** slugs, so a
  rename's previous address stays reserved and the 301 cannot be hijacked by a competitor claiming the old
  host.
- `Restaurant.slug` on the projection becomes **nullable, still `UNIQUE`**. Postgres permits multiple
  `NULL`s in a unique index, so ~200k unclaimed listings coexist while uniqueness is enforced by the
  database over exactly the configured set.
- **Migration**: slugs are **nulled on `NON_PARTNER` rows** and kept for claimed ones *(D5)*, releasing the
  reserved hostnames. Nothing was ever published at those addresses.
- `ConfigureRestaurantSlug` is a **command** — the owner can be told no, so `SlugAlreadyTaken` becomes a
  real rejection delivered to a human who can pick again, plus a new `SlugNotConfigured` for the activation
  gate.

### 2. SIRENE becomes an inbound event

- Per CLAUDE.md's own test — *if the originator can be told "no" → command; if they state a fact that has
  already occurred → inbound event* — **INSEE cannot be told no**, which is exactly why today's rejections
  are dropped to `eprintln!`. SIRENE therefore routes through **`inbound_events`**, not `command_journal`.
- **The ACL stages `RestaurantRegistered` only, unconditionally** *(D4)*. It does **not** branch between a
  register and an update, and no new registry-fact event is introduced. The **aggregate** folds its own
  stream and decides the resulting fact: absent → record `RestaurantRegistered`; present and materially
  changed → emit `RestaurantUpdated`; present and unchanged → **decide nothing, write nothing**. This keeps
  the last domain decision out of the adapter. Note the consequence for `actors.yaml`: the Restaurant
  aggregate's inbox gains `RestaurantRegistered` as an inbound *message* whose `emits` includes
  `RestaurantUpdated` — an inbound event that is not merely recorded verbatim.
- Delivery-level dedupe uses the **stable** `UNIQUE (source, external_id)` on `inbound_events` with
  `external_id = {siret}:{payload_hash}`. `command_journal`'s `message_id` is
  `UUIDv5(command type, SIRET, last_seen_at)` — deliberately versioned, so it can never dedupe across
  sweeps and produced a fresh journal row per SIRET per week.
- **`InboundEventStatus` gains `IGNORED` and `DUPLICATE`** *(D6)*. The drain worker already distinguishes
  `RecordOutcome::Recorded` from `AlreadyRecorded`
  (`crates/infrastructure/src/integrations/inbound_drain_worker.rs:177-179`) and then throws the
  distinction away by calling `mark_delivered` for both (`:139-144`) — the spec even codifies the loss
  (*"an already-recorded no-op still DELIVERs"*, `specs/database/tables/journals.yaml:48`). The two statuses
  have different causes and different fixes: `IGNORED` = the aggregate decided nothing changed,
  `DUPLICATE` = the same `(source, external_id)` was re-staged.
- **Two comparisons, two homes.** *"Has this external record changed since we mirrored it?"* is a fact
  about INSEE with no domain content — it belongs in the ACL, as a `payload_hash` compared inside
  `external_sirene_restaurants`, and it stops the sweep re-pending ~200k identical rows. *"Is this a
  meaningful change to the Restaurant?"* is a domain question and belongs only in the fold. The hash covers
  the **ACL-relevant fields**, not the raw payload, so an INSEE-internal timestamp does not defeat it.
- **The closure path stays a command.** Detect-by-absence is *our inference* from a missing row — absence is
  not a statement by INSEE — and it can be refused (`NON_PARTNER` prospects auto-close, partners are
  flagged for review). So `MarkRestaurantClosed` remains a command.

### 3. `idempotent_on_existing` is deleted

Handlers fold before deciding, like every other handler in the file. A version conflict then means what it
always should have: a genuine optimistic-concurrency clash. It is **retried once** (reload, re-decide) and
**counted**, never mapped to success. This covers all six sites, including the `verify_phone`
`created: true` fiction.

## Alternatives considered

- **Add the GIN index and move on** — makes the wrong query fast. The write path still consults an
  eventually-consistent projection for a write decision, and the aborted INSERTs and dropped updates remain.
- **Keep the command, add a worker-side pre-check on the read model** — hard-codes a domain decision into an
  adapter and keeps the write path dependent on projector freshness. Rejected by the product owner.
- **Keep the command, make `RegisterRestaurant` declarative (upsert semantics)** — workable, and was the
  leading option before the command/event reframing, but it leaves an unrejectable "command" and
  `*Registered` stops meaning "first time" for every consumer.
- **`ON CONFLICT DO NOTHING` in the event store** — avoids the transaction abort and yields
  `rows_affected == 0` to branch on, but speculative insertion still leaves a dead tuple, and it keeps the
  decision in the adapter. Retained only as the race arbiter *behind* a fold-first decision.
- **Stage a registry fact (`RestaurantObservedInRegistry`) plus a policy** — the more purist split, and the
  recommendation put to the product owner; rejected in favour of `RestaurantRegistered` only, which reaches
  the same place with one fewer event and the same adapter-free decision.
- **`RestaurantSlugChanged` instead of `Reconfigured`** — matches the house convention for a scalar changing
  (`CustomerPhoneChanged`, `RestaurantAcceptanceModeChanged`); rejected in favour of the visible
  `Configured`/`Reconfigured` pairing.
- **Defer until launch** — every element gets more expensive with scale, and a storefront address becomes
  immovable the moment it is printed on a menu or configured in Google.

## Consequences

### Positive

- A re-synced unchanged SIRET writes **nothing**: no `domain_events` row, no aborted INSERT, no journal row,
  no read-model scan. The dominant IO consumers of a sweep all disappear rather than being optimised.
- INSEE changes finally reach the domain, via `RestaurantUpdated` decided by the aggregate.
- The cross-aggregate slug-uniqueness invariant stops existing in that form: one half becomes a database
  constraint over the claimed set, the other becomes an aggregate-local activation gate.
- Merchants choose their own storefront address, and a rename keeps old links working.
- `SELECT status, count(*) FROM inbound_events WHERE source = 'sirene'` becomes the per-sweep report
  (created / updated / ignored / failed) — durable and queryable, with no telemetry stack required. This
  matters because there is currently **no** `opentelemetry`, `tracing-subscriber` or `/metrics` anywhere in
  the workspace, and the first notification of this whole class of defect was an email from the database
  vendor.

### Negative

- Wide spec surface: `events.yaml`, `commands.yaml`, `errors.yaml`, `scalars.yaml`, `entities.yaml`,
  `actors.yaml`, `rules.yaml`, `tests.yaml`, `api.yaml`, the screen and translation files, and three
  database table files — plus everything generated from them.
- `RestaurantState` must be widened to carry address, contact, location, opening hours and cuisine category
  before the aggregate can answer *"nothing changed"*. Pure domain work, but it is a precondition for
  everything else.
- An inbound event whose handling can emit a *different* event (`RestaurantRegistered` in →
  `RestaurantUpdated` out) is a new shape in `actors.yaml`; the validator wiring must accommodate it.
- Migration must null ~200k projection slugs and, until the marketplace listing-path model exists, unclaimed
  listings have no pretty address of their own.
- Two new persistence concerns (slug reservation, slug alias) and a released-slug quarantine policy.

### Follow-up actions

- Land in the proposal's §10 order: widen `RestaurantState` → slug lifecycle spec → slug lifecycle code →
  SIRENE inbound conversion → delete `idempotent_on_existing` across the remaining five sites →
  observability.
- **Resume SIRENE only when the chain is complete**, and resume **both halves together** — the weekly cron
  in `.github/workflows/sirene-sync.yml` and `RUN_SIRENE_WORKER` (paused 2026-07-28, PR #221). CI-only piles
  up unprocessed staging rows; worker-only re-drains whatever is pending.
- Add the `sirene-sync` observability contract to `specs/observability.yaml` and a counter for absorbed
  version conflicts.
- Decide the released-slug quarantine window when the reservation table lands.
- Define the marketplace listing-path form for unclaimed listings (out of scope here — the decision recorded
  is only that it must not be a host).
