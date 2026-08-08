# PROP-20260808-142532 — Disappearance is a designed state: the terminal-state contract and the opt-out fold

- **Status**: Proposed (awaiting product-owner approval)
- **Date**: 2026-08-08
- **Tracking issues**:
  [#398 "Decide the API contract for tombstoned rows before the #194 projection sweep"](https://github.com/TheCaptainCompany/captain-food/issues/398)
  and
  [#347 "Decide the last annotated read-model hole: Restaurant fed by RestaurantListingOptedOut"](https://github.com/TheCaptainCompany/captain-food/issues/347)
  — one proposal answers both because they are two faces of one principle (§C).
- **Realized by**: _(filled at completion — the §E steps land independently)_
- **Author lens**: `ux-designer` agent, session
  https://claude.ai/code/session_01AKgDqRbCcCxtUePWPRfxtp — every claim below carries a
  `file:line` cite verified on current `main`.
- **Concerns**:
  - [ ] **D2 touches `OrderPlaced`'s payload**: adding `restaurantName`/`restaurantPhone` is event
    evolution — additive, no consumer breaks — but event vocabulary is a product-owner call and
    must be signed off before the field lands.
  - [ ] **The resolver policy change (banning silent-drop and join hard-errors) modifies GENERATED
    resolver behaviour** (`crates/server/src/graphql/generated/query.rs`): it must land through
    the emitter, coordinated with the resolver-touching slices of the
    [#348 "Epic: the rider/delivery write surface does not exist"](https://github.com/TheCaptainCompany/captain-food/issues/348)
    proposal ([PROP-20260808-141817](PROP-20260808-141817-rider-delivery-write-surface.md)) —
    never as hand-edits to generated output.
  - [ ] **Section B's OPTED_OUT guard adds a new error to the network scope**
    (`ListingOptOutNotApplicable` or equivalent): ADR-0032 completeness applies — the realizing
    change needs a behaviour test with its `rules:` link, not just the error definition.
- **Related**:
  [#194 "GDPR Article 17 has no technical answer: PII lives in an immutable event log with no erasure path, and no DPIA/privacy policy/terms exist"](https://github.com/TheCaptainCompany/captain-food/issues/194)
  (the sweep this proposal must precede) ·
  [#346 "view-fedby-unused ignores `tombstone:` — latent since the reorg deleted the only firing instance"](https://github.com/TheCaptainCompany/captain-food/issues/346)
  and
  [#399 "Validator gap: a tombstone event absent from the view's fedBy silently never dispatches"](https://github.com/TheCaptainCompany/captain-food/issues/399)
  (validator siblings) · ADR-20260731-160000 (erasure design: stream deletion, the `OrderDeleted`
  ledger, rebuild survival) · ADR-0041 (acting user is envelope metadata) · ADR-0004 (commands vs
  inbound facts — the SIRENE sync is an ACL, not a command path).

---

## TL;DR

Two coupled decisions, one principle: **disappearance is always a designed state; physical row
removal is reserved for legal erasure.**

The load-bearing reframe for
[#398](https://github.com/TheCaptainCompany/captain-food/issues/398): `OrderExpired` is only ever
scheduled from a **TERMINAL** lifecycle fact, so **a live order can never tombstone** — the
scary scenarios (tracking screen goes blank mid-delivery, board loses an undelivered paid order)
are *never-reachable states* that a pinned test must KEEP unreachable. What remains to design is
deep links to long-expired orders, restaurant disappearance under a retained order, and the
acceptance-first not-yet-projected window. Today the resolvers answer those three situations with
**three different accidental behaviours** (hard error, silent drop, ambiguous null); the proposal
replaces them with three **pinned faces of absence** and a scoped mix of projector-/event-carried
composition plus a thin dangling policy (D1/D2).

For [#347](https://github.com/TheCaptainCompany/captain-food/issues/347): tombstoning the
`Restaurant` row on `RestaurantListingOptedOut` is **self-defeating** — the SIRENE sync would
re-create the listing the owner asked to remove on its next sweep. The recommendation is a
**`listing_status` fold to a new `OPTED_OUT` value** (D3) with a write-side guard for
ACTIVE_PARTNER (D4) — and it closes a **live compliance exposure**: `ProspectionPipeline` does not
fold the opt-out today, so an opted-out owner can still be cold-emailed.

---

## A. The terminal-state contract (#398)

### A.1 The load-bearing reframe

`OrderExpired` is only ever scheduled from a **TERMINAL** lifecycle fact — the reminder is
declared on terminal receives with `ORDER_RETENTION_WINDOW_DAYS` (default 3650)
(`specs/ordering/actors.yaml:85-103`, `specs/ordering/configuration.yaml:10-22`). By
construction, a live (undelivered, un-cancelled) order can never tombstone. The
anxiety-curve-critical scenarios ("customer watching the tracking screen when the row vanishes",
"board loses an undelivered paid order to expiry") are **never-reachable states the design must
keep unreachable**, and the API contract must not quietly depend on a config value to guarantee
that — a pinned test asserts terminal-only scheduling.

The reachable cases, which the contract must design for:

1. deep links / history to long-terminal orders after expiry;
2. restaurant-side disappearance under a retained order;
3. the acceptance-first not-yet-projected window masquerading as case 1.

### A.2 What exists / what is pinned today

- `order(id)` is `nullable: true` and its description says nothing about what null means
  (`specs/ordering/api.yaml:122-128`). **GAP: no pinned null semantics.** Nothing in
  `specs/tests.yaml` asserts read-side behaviour for a tombstoned row — only write-side
  (`TestOrderExpiredRecorded` / `TestOrderExpiredRedeliveryIsNoOp`, `specs/tests.yaml:1918-1942`).
- Query-time joins have **THREE different accidental behaviours** for the same situation:
  - **Hard error**: `order(id)` → "order references an unknown restaurant"
    (`crates/server/src/graphql/generated/query.rs:360-364`); same for cart (`:322`), catalog
    (`:31`), delivery (`:133-138`), and the WHOLE `restaurantDeliveries` board if the restaurant
    row is gone (`:191`).
  - **Silent drop**: customer order history (`orders`) `filter_map`s away any order whose
    restaurant is missing (`query.rs:347-350`); the delivery board drops any job whose order row
    is missing (`query.rs:194`). (The same pattern exists at `query.rs:162-169` for rider
    `myDeliveries` — the rider-epic instance should inherit the same policy.)
  - **Legitimate null**: row itself missing → `Ok(None)` (`query.rs:357-359`) — indistinguishable
    from "never existed" and "not yet projected".
- `OrderExpired` is parked in `nonProjectedEvents` awaiting the
  [#194](https://github.com/TheCaptainCompany/captain-food/issues/194) sweep
  (`specs/database/projection_views.yaml:53-57`).
- `OrderTracking` does NOT carry restaurant name/phone — only a `restaurant_id` fk
  (`specs/database/tables/projection_tables.yaml:517-519`), yet the tracking screen renders
  `order.restaurant.name` / `.phone` (`specs/screens/restaurant_frontoffice.yaml:456`) and
  `OrderPlaced` does not carry the name either (`specs/ordering/events.yaml:113-143`). **GAP —
  this single fact decides the engineering option (D2).**

### A.3 Per-surface terminal-state table

| Surface | (i) Order expires/tombstones | (ii) Restaurant vanishes under a live/retained order | Anxiety class |
|---|---|---|---|
| Customer order tracking, mid-session (`order_tracking`, `restaurant_frontoffice.yaml:429-464`) | Never reachable for a live order (expiry only from terminal facts) — must stay guaranteed by write-side design, asserted by a test. Acceptance-first window: null within N seconds of `placeOrder` returning is a designed "accepted ✓ — confirming…" state, never "not found" (client holds the orderId; `operationStatus`/subscription is the truth channel, not `order(id)` polling). GAP: window state has no pinned copy/spec. | Must keep rendering. Contact row + name from the order's OWN read model, not a live join. Restaurant deactivated (status fold, `projection_tables.yaml:152-160`) → order continues; genuinely erased → tracking shows order with "restaurant no longer on Captain.Food" on the contact row — never today's hard error. | MUST REASSURE — peak of the curve; a 500 or null-flip here is churn per second. |
| Customer order history (`order_history`, `restaurant_frontoffice.yaml:466-487`) | Absence IS the designed state: GDPR erasure means "not readable through any query"; a per-row "expired" marker would itself retain data. Inform generically: static footer "Orders older than {years} years are removed from your account" (translations GAP). Deep link to an expired order → designed expired screen, copy: "This order has expired and its details are no longer available." | Row MUST render: "Pizza Luigi (no longer on Captain.Food) — 24,90 € — Delivered 12 Jan". Today SILENTLY DROPPED (`query.rs:349`) — paid money history vanishing without explanation is the worst support-call generator on this list. Never-reachable requirement: a retained order absent from history because of someone ELSE's erasure. | May inform (i); must NEVER silently lose money history (ii). |
| Cart (`query.rs:297-323`) | n/a | Open cart whose restaurant vanishes → designed dead-end, not a 500: "This restaurant is no longer available on Captain.Food — this cart can't be ordered." + CTA to browse. Checkout mutation rejects actionably, never accept-then-strand. | May inform — pre-payment, money hasn't moved. |
| Storefront catalog (`{slug}.captain.food`, `query.rs:19-33`) | n/a | Erased restaurant's host currently falls through to the claim-your-subdomain landing (`crates/server/src/hosts.rs:40-44, 256-263`) — inviting the public to claim a dead business's address is worse than a 404. Designed state: parked "closed" page (D5). Slug-reservation rule ("released label never reused", `projection_tables.yaml:192`) is the residual marker making this renderable without retaining erased data. | Must inform; must never invite resurrection. |
| `trackDelivery` (`query.rs:122-140`) | Never reachable live. Deep link post-expiry: null with pinned meaning → expired-order screen. Today: hard error "delivery references an unknown order" — must go. | Degrade restaurant-sourced fields, keep the job renderable. | MUST REASSURE while live; may inform historic. |
| Restaurant delivery board (`deliveries_board`, `restaurant_backoffice.yaml:147-167`) | NEVER REACHABLE — two independent guarantees: (1) design: live orders never expire; (2) engineering: a board row must not be droppable by a missing join (`query.rs:194` silent drop = the "paid order nobody is told about" mode wearing a resolver). If violated anyway: render degraded and LOUD — "⚠ Order data incomplete — contact support", never absent (`View_DeliveryJob` already carries pickup/dropoff addresses, `delivery/api.yaml:19-25`). Also: whole-board hard error when the restaurant row is missing (`query.rs:191`) turns one bad row into a dead board at Friday 19:30 — same ban. | Restaurant reading its own board cannot be erased mid-session; treat as infra error with retry state, not a blank board. | Never-reachable / fail-loud. |
| Admin | Admin must see MORE after erasure: `order(id)` null + a queryable erasure receipt — "Order erased under policy {policy} on {date}, receipt {id}" from the `OrderDeleted` ledger (ADR-20260731-160000 §6). GAP(read-model)+GAP(api)+GAP(screen): the deletion ledger has no projection/query/screen — "is this order really gone?" designed answerable (`docs/adr/ADR-20260731-160000…md:36-38,49-57`) but no surface answers it. | Terminal state / receipt, never a join error. | Informs; audit surface. |

### A.4 Mockups

Expired order (deep link / history), customer:

```
┌────────────────────────────────┐
│  ←  Ma commande                │
│        ⏳                      │
│   Cette commande a expiré      │
│   Ses détails ne sont plus     │
│   disponibles (conservation    │
│   des données arrivée à terme).│
│   [ Voir mes commandes ]       │
│   [ Aide et factures ]         │
└────────────────────────────────┘
```

Parked storefront (erased/closed restaurant host):

```
┌────────────────────────────────┐
│        captain.food            │
│   Ce restaurant n'est plus     │
│   sur Captain.Food.            │
│   [ Découvrir les restaurants  │
│     autour de vous → ]         │
└────────────────────────────────┘
```

### A.5 Option verdict

**Projector-carried composition for money-history surfaces, plus a thin pinned dangling policy as
the safety net (the scoped mix — D1).** The journeys decide it: order history MUST render
"Pizza Luigi (no longer on Captain.Food)" and tracking MUST keep its name/contact row without a
live Restaurant join — only possible if the Order read model owns those fields; a dangling policy
alone can decorate absence but never recover the name. Conversely composition alone leaves
unpinned semantics on the remaining edges. So:

1. **Projector-carried**: any field a journey renders after the FK target may legitimately vanish
   is copied into the referencing read model at fold time (precedent:
   `Restaurant.default_currency` cross-stream fold, `projection_tables.yaml:38,163-164`;
   `View_DeliveryJob` embeds both addresses).
2. **Dangling policy pinned in api.yaml + tests**: silent drop and hard error BOTH BANNED; a
   missing composition target degrades the affected fields; a missing row returns null with
   pinned meaning.

### A.6 Read-model / spec requirements (each one-to-one proposable)

| Requirement | Artifact | Status |
|---|---|---|
| `OrderTracking.restaurant_name` (+`restaurant_phone`) — `OrderPlaced` gains `restaurantName`/`restaurantPhone` (event-carried; survives rebuild after restaurant stream deletion per ADR-20260731-160000 §3) vs projector snapshot (D2) | `projection_tables.yaml#/OrderTracking` + `ordering/events.yaml#/OrderPlaced` | GAP |
| Pin `order(id)` null semantics: "null = no order readable to you: never existed, not yours, or erased. Just-placed window served by `operationStatus`/subscription, never by polling this to null." | `ordering/api.yaml:122-128` description | GAP |
| Board rows never droppable: `View_DeliveryJob` self-sufficiency + resolver policy drop→degrade-loud | `projection_views.yaml#/View_DeliveryJob` + generator emitter | GAP (resolver policy) |
| History silent-drop removed (`orders` resolver): degraded row instead | generator emitter for nav joins | GAP |
| Erasure-receipt admin surface: `OrderDeleted` ledger projection + admin query + screen | `specs/database/` + `ordering/api.yaml` + screens | GAP ×3 |
| Expired-order terminal screen + parked-storefront page + history footer copy | `restaurant_frontoffice.yaml` + `hosts.rs` behaviour + translations sidecars | GAP |
| Test pinning "a live order never tombstones" (terminal-only scheduling) + read-side null/degrade contract tests | `specs/tests.yaml` | GAP |

### A.7 Sequence diagram — the three faces of absence on the read path

```mermaid
sequenceDiagram
  actor C as Customer
  participant S as order_tracking / order_history
  participant G as GraphQL gateway (/customer)
  participant V as View_Order / OrderTracking
  alt just placed (acceptance-first window)
    C->>S: lands on tracking right after placeOrder
    S->>G: operationStatus(operationId)
    Note over S: "accepted ✓ — confirming…" — NEVER order(id)-polling to null
  else deep link to an expired order
    C->>S: opens old link / history row
    S->>G: order(id)
    G->>V: select by id
    V-->>G: no row (OrderExpired tombstone fold)
    G-->>S: null — pinned meaning: not readable to you
    S->>C: designed expired screen (mockup A.4), never "not found"
  else retained order, restaurant erased
    S->>G: orders / order(id)
    G->>V: rows present; restaurant composition target GONE
    G-->>S: degraded-but-present object (name from OWN read model, D2)
    S->>C: "Pizza Luigi (no longer on Captain.Food) — 24,90 € — Delivered 12 Jan"
    Note over G: silent drop (query.rs:349) and hard error (query.rs:364) BANNED
  end
```

## B. What "opted out" means (#347)

### B.1 Grounding

Event `specs/network/events.yaml:344-355` ("an owner asked to edit/remove their public listing,
proven via GBP ownership"); command+actor `specs/network/commands.yaml:342-348`,
`specs/network/actors.yaml:138-142`; mutation roles PUBLIC/RESTAURANT_ACCOUNT
`specs/network/api.yaml:190-193`; story step `specs/stories.yaml:144`; enum
NON_PARTNER/PASSIVE_PARTNER/ACTIVE_PARTNER `specs/network/scalars.yaml:118-126`; `listing_status`
fold `projection_tables.yaml:53-58`; browse shows non-partner cards by default
`specs/network/api.yaml:82`.

**Decisive structural fact**: opt-out is an unclaimed/open-data-listing journey. A NON_PARTNER or
PASSIVE_PARTNER listing was never orderable (`scalars.yaml:126`), so no cart/order/money history
references it through commerce — the opt-out population is precisely the ~200k SIRENE rows. An
ACTIVE_PARTNER leaving is offboarding (`RestaurantDeactivated` / `RestaurantRemoved` — already a
status fold to INACTIVE, `projection_tables.yaml:152-160`, not row deletion). **Opt-out,
offboarding and erasure are three different journeys.**

### B.2 Per-persona table

| Persona | Meaning | Rendered as |
|---|---|---|
| Marketplace browser | Listing does not exist for them. Hidden entirely, never greyed — a browser has no relationship with this business; a greyed corpse is decision fatigue plus a broken promise to the owner. Hidden-by-filter ≠ tombstone. | Absent from restaurants/search/shelves. |
| Customer with history there | Practically empty set (never orderable). Legacy edge: row still exists → joins hold; A's carried name makes it moot anyway. | Unchanged history. |
| Storefront host | Unclaimed listing has NO slug (NULL until owner configures, `projection_tables.yaml:69-78`) → no storefront to take down. For a claimed restaurant the storefront is the owner's OWN channel (GBP order button points at it, ADR-0021) — opting out of the marketplace directory must not touch it. `hosts.rs` doesn't consult `listing_status`; keep it that way. | No change. |
| Restaurant owner (back-office) | Opted out of LISTING, not existence. Still sees themselves, state named and reversible. | "Retiré de l'annuaire — sur votre demande" + path back (claim / re-list). |
| Admin / ops | An objection on record: audit (who/when/reason) AND a do-not-contact flag — an opt-out is a GDPR Art. 21-shaped objection; prospection must stop. `ProspectionPipeline` does NOT fold `RestaurantListingOptedOut` today (`projection_tables.yaml:214-220`) — **an opted-out listing can still be cold-emailed. GAP, the sharpest consequence of the current hole.** | Visible in listings admin + excluded/flagged in pipeline. |

### B.3 Recommendation

**Fold into `listing_status`, adding an `OPTED_OUT` enum value (D3, option 2).** Three reasons:

1. **Tombstone is self-defeating**: the SIRENE sync
   (`crates/infrastructure/src/integrations/sirene.rs`, `sync_sirene_worker.rs`) continuously
   imports the open-data universe keyed by external identifiers — delete the row and the next
   sweep RE-CREATES the listing the owner asked to remove; the row must persist precisely to
   remember the refusal (ACL skips/holds OPTED_OUT rows). Tombstone also amputates other
   consumers (audit, do-not-contact, legacy joins) for a reversible business withdrawal — and the
   population is legal-entity open data, not personal data under an erasure duty;
   [#194](https://github.com/TheCaptainCompany/captain-food/issues/194)'s frame does not apply.
2. **Vestigial is a broken promise**: an owner who proved GBP ownership still shows on the
   marketplace forever — the "I asked and nothing happened" support call with a regulator-shaped
   tail.
3. **The fold is one column, zero new machinery**: `listing_status` already folds
   `RestaurantListingStatusChanged` (`projection_tables.yaml:57`); add
   `RestaurantListingOptedOut → OPTED_OUT`, filter browse/search/shelves, add the event to
   `ProspectionPipeline.fedBy` (terminal do-not-contact state); reversal = existing
   `changeRestaurantListingStatus` / re-claim path.

**Named caveat** (→ D4): `OPTED_OUT` in the funnel enum conflates marketplace visibility with
partnership stage — wrong for ACTIVE_PARTNER (would flip orderable and kill their storefront
ordering). Close with a write-side guard: `OptOutRestaurantListing` rejected for ACTIVE_PARTNER
(new error, e.g. `ListingOptOutNotApplicable`: "partner offboarding is a different journey").
Fallback: a separate orthogonal `delisted` boolean. Journeys satisfied by either; enum+guard is
the smaller change.

**Consequences under the recommendation**: storefront host unchanged; order history unchanged;
marketplace hidden; prospection stopped permanently with reason on record. Also decide
explicitly: fold the event's `reason` into an admin-visible column, or leave log-only
(→ Unresolved questions).

### B.4 Sequence diagram — opt-out fold, guard, and re-import proof

```mermaid
sequenceDiagram
  actor O as Owner (GBP-proven)
  participant G as GraphQL gateway
  participant M as Restaurant mailbox
  participant P as Projector
  participant V as Restaurant.listing_status / ProspectionPipeline
  participant W as SyncSireneWorker (ACL)
  O->>G: optOutRestaurantListing(restaurantId, reason)
  G->>M: enqueue OptOutRestaurantListing (PENDING, acceptance-first)
  alt listing is ACTIVE_PARTNER
    M-->>G: rejected — ListingOptOutNotApplicable [GAP, D4 guard]
    Note over O: "partner offboarding is a different journey"
  else NON_PARTNER / PASSIVE_PARTNER
    M->>M: aggregate decides (pure), Repository saves via PgEventStore
    M->>P: RestaurantListingOptedOut appended
    P->>V: listing_status = OPTED_OUT [GAP]; ProspectionPipeline -> do-not-contact, terminal [GAP]
    Note over V: browse/search/shelves filter OPTED_OUT out; storefront host untouched
  end
  W->>W: next SIRENE sweep imports the same legal entity (external ref key)
  W->>V: row EXISTS with OPTED_OUT -> ACL skips/holds [GAP]
  Note over W: a tombstone here would have RE-CREATED the listing — the row IS the memory of the refusal
```

## C. The unified principle

Disappearance is always a designed state; physical row removal is reserved for legal erasure. One
rule, three clauses:

1. **Business withdrawal is a state fold, never a tombstone** — opt-out, deactivation, closure all
   resolve to a column readers filter on (reversible, auditable, re-import-proof). Section B is
   the test case; it passes only under option 2 (D3).
2. **Legal erasure is a tombstone, and rows referenced by money history are never physically
   dangled**: before any Order-fed or Restaurant-referencing tombstone lands, every surface that
   must still render afterwards carries its own copy of what it renders (projector-/event-
   carried). The FK is a navigation convenience, never a rendering dependency across an erasure
   boundary.
3. **At the API layer, absence has exactly three pinned faces** — null meaning "not readable
   (never existed / not yours / erased)"; a degraded-but-present object when a composition target
   is gone; the acceptance-first "confirming…" window owned by `operationStatus`. Silent drop and
   join hard-errors are banned states, at their worst on the delivery board where they
   impersonate the "paid order nobody is told about" failure mode.

## D. Decisions surfaced

Each decision carries per-option trade-offs; the recommended option is marked.

### D1 — API contract for dangling/tombstoned references: policy only vs composition only vs the scoped mix

| Option | Pros | Cons |
|---|---|---|
| Dangling-reference policy only (pin null/degrade semantics, no read-model changes) | Smallest change; no event or projection churn; uniform rule for the emitter | **A policy alone can decorate absence but never recover a name**: order history cannot render "Pizza Luigi (no longer on Captain.Food)" and the tracking contact row goes blank — the journeys in §A.3 fail on the exact surfaces that must reassure |
| Projector-carried composition only (copy every rendered field into referencing read models) | Money-history surfaces fully self-sufficient; rebuild-safe | **Still leaves unpinned nulls**: `order(id)`'s null stays ambiguous (never existed / not yours / erased / not yet projected), and the silent-drop/hard-error resolver behaviours survive untouched on the edges no journey copied |
| **The scoped mix — composition for money-history surfaces + thin pinned dangling policy as the safety net** — **RECOMMENDED** | The journeys decide it (§A.5): copied fields where a surface must render after the target vanishes; pinned three-face policy everywhere else; each half small; precedents exist (`default_currency` fold, `View_DeliveryJob` addresses) | Two mechanisms to document and test instead of one; deciding WHICH fields are journey-rendered requires the per-surface table to stay maintained |

### D2 — `OrderTracking` restaurant name/phone: event-carried on `OrderPlaced` vs projector cross-stream snapshot

| Option | Pros | Cons |
|---|---|---|
| **Event-carried: `OrderPlaced` gains `restaurantName`/`restaurantPhone`** — **RECOMMENDED** | **Survives projection rebuild after restaurant stream deletion** (ADR-20260731-160000 §3) — a snapshot does not: replay after erasure finds no Restaurant stream to read, and the name is gone exactly when it is needed; the fact "this order was placed with Pizza Luigi" is genuinely part of the placement fact; additive, no consumer breaks | Event evolution on the money-path event (PO sign-off — header Concern); marginally larger payload; the name at placement time can go stale vs later renames (acceptable: money history should show the name as it was) |
| Projector cross-stream snapshot at fold time | No event change; follows the existing `default_currency` precedent | **Does not survive rebuild after restaurant stream deletion** — the precise erasure scenario this exists for; the copied value silently depends on projection ordering across streams |

### D3 — #347: tombstone vs `listing_status` fold `OPTED_OUT` vs vestigial removal

| Option | Pros | Cons |
|---|---|---|
| Tombstone the Restaurant row | Strongest "gone" semantics; matches a naive reading of "remove my listing" | **Self-defeating under SIRENE re-import**: the sync re-creates the deleted row on the next sweep — the row must persist to remember the refusal; amputates audit, do-not-contact and legacy joins for a reversible business withdrawal; wrong legal frame (legal-entity open data, no erasure duty) |
| **Fold into `listing_status` → new `OPTED_OUT` value; filter browse/search/shelves; add the event to `ProspectionPipeline.fedBy`; ACL skips OPTED_OUT rows** — **RECOMMENDED** | One column, zero new machinery (`listing_status` already folds `RestaurantListingStatusChanged`); reversible via the existing re-list/claim path; auditable; re-import-proof; stops prospection permanently with reason on record | Adds a visibility state to a partnership-funnel enum (the D4 caveat); browse/search/shelves queries each need the filter and a test |
| Vestigial removal (delete the event/command as unused) | Deletes a warning and some spec surface | **A broken promise to an owner who proved GBP ownership**: they still show on the marketplace forever — the "I asked and nothing happened" support call with a regulator-shaped tail; the mutation and story step already exist and describe a real journey |

### D4 — `OPTED_OUT` shape: enum value + write-side guard vs a separate orthogonal `delisted` boolean

| Option | Pros | Cons |
|---|---|---|
| **`OPTED_OUT` enum value + guard: `OptOutRestaurantListing` rejected for ACTIVE_PARTNER with a new error (e.g. `ListingOptOutNotApplicable`)** — **RECOMMENDED** | The smaller change: one enum value, one fold arm, one guard; a single column keeps every reader's filter trivial; the guard names the domain truth ("partner offboarding is a different journey") as a designed rejection | Conflates marketplace visibility with partnership stage inside one enum — safe ONLY because the guard makes OPTED_OUT unreachable for ACTIVE_PARTNER; new error carries ADR-0032 completeness cost (test + rules link — header Concern) |
| Separate orthogonal `delisted` boolean | Clean separation of concerns: visibility and funnel stage never share a column; no guard needed | Second column every browse/search/shelf/prospection reader must remember to consult (a forgotten filter re-exposes the owner); more spec surface (column, fold, filters ×N) for the same journeys — the fallback if the PO rejects the enum conflation |

### D5 — Erased-restaurant storefront host: claim-landing fall-through (current) vs parked "closed" page vs plain 404

| Option | Pros | Cons |
|---|---|---|
| Current claim-landing fall-through (`hosts.rs:40-44, 256-263`) | Zero work | **Invites the public to claim a dead business's address** — worse than a 404: resurrection of an erased restaurant's identity, on their old URL, by a stranger |
| **Parked "closed" page** ("Ce restaurant n'est plus sur Captain.Food" + browse CTA — mockup A.4) — **RECOMMENDED** | **Never invites resurrection of a dead business's address**; informs honestly; recovers the visit with a browse CTA; renderable without retaining erased data — the slug-reservation rule ("released label never reused", `projection_tables.yaml:192`) is the residual marker | One more host-routing state + page + copy to maintain in `hosts.rs` behaviour and translations |
| Plain 404 | Simple; no data retained | Looks like breakage, not a designed state — old links from GBP/social read as "the platform lost my restaurant"; recovers nothing (no CTA); still better than the fall-through |

## E. Sequencing with the #194 projection sweep

Each step is independently proposable (per the
[#194](https://github.com/TheCaptainCompany/captain-food/issues/194) frame; the validator
siblings [#346](https://github.com/TheCaptainCompany/captain-food/issues/346) and
[#399](https://github.com/TheCaptainCompany/captain-food/issues/399) make step 3 safe to verify):

1. **BEFORE the sweep**: pin `order(id)`/`delivery` null semantics in api.yaml; replace resolver
   silent-drop (`query.rs:194,349`) and hard-error (`query.rs:191,364` et al.) with the
   degrade-loud policy **in the emitter** (header Concern — coordinated with the
   [#348](https://github.com/TheCaptainCompany/captain-food/issues/348) slices); add the "live
   orders never tombstone" test.
2. **BEFORE the sweep**: `OrderTracking.restaurant_name`/`phone` (event-carried preferred, D2) +
   expired-order screen, history footer copy, parked-storefront host behaviour (D5).
3. **The sweep itself** ([#194](https://github.com/TheCaptainCompany/captain-food/issues/194)):
   per-projection `OrderExpired` tombstone folds; remove the `nonProjectedEvents` parking entry
   (`projection_views.yaml:53-57`) so the validator hunts stragglers, exactly as annotated.
4. **With the deletion engine**: `OrderDeleted` receipt projection + admin query + screen ("is it
   really gone?" surface).
5. **Independently, any time**: Section B's `OPTED_OUT` fold + browse filter +
   `ProspectionPipeline.fedBy` + opt-out guard + SIRENE-ACL skip — small, **self-contained, and
   closes a LIVE compliance exposure**: opted-out owners can still be cold-emailed today, because
   `ProspectionPipeline` does not fold the opt-out. This step closes both
   [#347](https://github.com/TheCaptainCompany/captain-food/issues/347) and that exposure, and
   does not wait on steps 1–4.

## F. Drawbacks

Why we might regret the whole thing, distinct from per-option cons:

- The three-face absence contract and the copied-field rule are **doctrine the emitter and every
  future resolver must honor** — a standing maintenance obligation, and a constraint on how cheap
  a future "just join it" read path can be.
- Event-carried denormalization (D2) opens the door to payload growth pressure ("while we're at
  it, carry the address too"); each addition needs the same rebuild-survival justification or it
  is bloat.
- `OPTED_OUT` inside the partnership funnel enum is a conflation held safe by a guard; if the
  guard is ever weakened or bypassed, the enum lies. The orthogonal-boolean fallback exists
  precisely because of this.
- The parked-page + never-reuse-slug posture permanently retires storefront labels — accepted
  consciously, but it is namespace the platform gives up forever.

## G. Unresolved questions

Copied to the tracking issues' checklists on approval (README convention):

- The three header Concerns (D2 event sign-off; emitter-landing of the resolver policy; ADR-0032
  completeness for the new guard error).
- Fold the opt-out event's `reason` into an admin-visible column, or leave it log-only?
- The acceptance-first window copy and its timeout (when does "confirming…" become an error
  state?) — no pinned copy/spec exists (§A.3, row 1).
- The history footer's `{years}` value: bind to `ORDER_RETENTION_WINDOW_DAYS` or fix the copy?
- Does the rider `myDeliveries` silent drop (`query.rs:162-169`) land here (step 1) or ride the
  [#348](https://github.com/TheCaptainCompany/captain-food/issues/348) rider slices? Same policy
  either way; one owner must be named.

## H. Verification plan

- Every realizing step lands through the standard flow: `make validate` at 0 errors, warning
  count/kinds diffed against a re-measured `main` baseline, `make rust` green.
- Step 1 is verified by the pinned tests it adds: the "live order never tombstones"
  (terminal-only scheduling) test, plus read-side contract tests for each of the three faces
  (null meaning, degraded row rendering the carried name, no silent drop / no join hard-error).
- Step 2's D2 field is verified by a projection-rebuild test: delete the restaurant stream,
  replay, and the OrderTracking row still renders name/phone.
- Step 5 is verified by: browse/search/shelves exclude OPTED_OUT; `ProspectionPipeline` marks the
  row do-not-contact (terminal); the SIRENE ACL skip test (re-import does not resurrect); the
  ACTIVE_PARTNER guard rejection test with its `rules:` link (ADR-0032).
