# ADR-20260815-032807 — Opening hours and stock are checked SERVER-SIDE on place order, and a big catalog snapshots every 100 events

**Status**: Accepted (the directive; the three mechanisms it requires are OPEN register rows) ·
**Date**: 2026-08-15 ·
**Decider**: the founder / Tech CEO, verbatim below ·
**Register**: [DECISIONS](../proposals/DECISIONS.md) §43 (**RSO-1**, **RSO-2**, **SNAP-1**, **BUS-1**) ·
**Relates to**: [ADR-20260815-030206](ADR-20260815-030206-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side.md)
(same PlaceOrder path; **PMW-2** already names Catalog as the actor residency cannot serve) ·
**Session**: https://claude.ai/code/session_018WtW3eyd4yWFKHTUEQYJkM

## Enforced by

Nothing yet, and that is the point of this record. Parts (a) and (b) are **business guarantees** and will
each need a `rules.yaml` entry plus its behaviour test when they land (ADR-0032) — they do not exist
today, which is exactly why the two gaps below are live. Part (c) is a runtime mechanism with a stated
budget and belongs in `specs/observability.yaml`, not `rules.yaml`. **This ADR lands no code and no
`specs/**` change beyond the `services.yaml` clarification described in the SPEC-LOG row of the same
commit.**

## The directive (verbatim, founder / Tech CEO, 2026-08-15)

> "In the place order process manager we should be careful about the opening hours of the restaurant it
> must be checked
> We can also check the stock of the items on the catalog
> These kind of checks will be also done on the screens but it must be also done on the server side
> If a catalog is too big > 100 events we will use snapshots to avoid reloading of the events from the
> stream we should snapshot every 100 events to avoid too long actor loading delay <5sec"

The directive has **three parts**. All three were verified against `main` before this record was
written, and **all three describe real gaps** — none of them is already done.

## The decision

1. **A restaurant's opening hours are a PLACE-ORDER GUARD**, checked on the server, not only rendered on
   a screen. A restaurant that is closed right now cannot receive an order.
2. **Line orderability — availability and stock — is RE-CHECKED at checkout**, on the server, in
   addition to the existing check at add-to-cart.
3. **A catalog aggregate snapshots every 100 events**, so actor load stays under **5 seconds**.

Part 3 carries the founder's own thresholds (**100 events**, **< 5 s**) and they are adopted verbatim as
the policy. **Where a snapshot lives, and how it interacts with upcasting and GDPR erasure, is a genuine
option space and is NOT decided here** — register row **SNAP-1**.

The framing sentence — *"these kind of checks will be also done on the screens but it must be also done
on the server side"* — is the general principle and it is worth stating on its own: **a client-side check
is a UX affordance, never an enforcement point.** The screen exists to stop a customer wasting their
time; the server exists to stop a wrong order being taken. Both are needed; only one is a guarantee.

## Part (a) — the opening-hours check: the concept does not exist anywhere

**Verified.** This is not "under-tested" or "partially wired": there is **no notion of *open right now*
in the product at all**.

- **`orderable` does not consider hours.** `specs/network/api.yaml:21` derives it as
  `ACTIVE_PARTNER + status ACTIVE + acceptance ≠ PAUSED` — opening hours are **not in that derivation**.
- **The PlaceOrder guard chain has no closed-hours guard.** `specs/ordering/processmanager.yaml:40-49`
  throws `RestaurantPaused`, `CannotOrderTestRestaurant`, `DeliveryAddressRequired`,
  `OutsideDeliveryArea`, `PriceUnresolvable` and `PriceMismatch`. **That is the whole list.**
- **The one closure-shaped event means something else.** `RestaurantMarkedClosed`
  (`specs/database/tables/projection_tables.yaml:216`) folds the restaurant to `INACTIVE` — it is
  **permanent closure** (the business shut down), not *"the kitchen is closed tonight"*. Using it for
  tonight would take the restaurant off the marketplace until someone reactivated it.
- **The raw material is already there.** `opening_hours` and `timezone` are projected columns
  (`projection_tables.yaml:207-228`) and both are exposed on the api type
  (`specs/network/api.yaml:38, 42`), typed `OpeningHoursSlot[]` and `TimeZone`.

**The live consequence, stated plainly**: a restaurant whose kitchen shut at 22:00 still renders
`orderable: true` at 22:40, **and the server accepts the order**. Under the domain lens in CLAUDE.md
that is the worst failure mode there is — *a paid order that nobody is told about* — arriving by the
front door: the customer is charged, the tablet is dark, nobody is in the kitchen, and the first person
to learn is the customer forty minutes later.

**What closing it needs** (the shape, not the decision — see **RSO-1**):

1. a derived **`isOpen`**, computed server-side **in the restaurant's own timezone** (the column is
   already there and nullable — the fallback to the account's timezone is already documented);
2. a new **`errors.yaml#/RestaurantClosed`** with its typed context (the next opening slot is what the UI
   needs to say something useful);
3. a **guard step in the PlaceOrder chain**, beside `RestaurantPaused`;
4. **`orderable` re-derived to include hours** — otherwise the screen and the server disagree, which is
   the same defect one layer up.

## Part (b) — the checkout orderability re-check: a recorded TODO that was never done

**Verified**, and it is written down in the code in as many words.
`crates/application/src/commands.rs:2450-2452`:

```rust
// TODO(invariant): OfferUnavailable / InsufficientStock / InvalidOptionSelection — re-validating
//                  each line's ORDERABILITY at checkout (pricing below already fails closed on a
//                  line that has left the catalog, but availability/stock re-checks are pending).
```

`require_orderable_line` (`crates/application/src/commands.rs:791-812`) — which checks
`OfferNotFound` → `OfferUnavailable` → `InsufficientStock` → `InvalidOptionSelection` — is called from
**`add_cart_line` only** (`:918`, `:950`) and from the quantity-change path for stock (`:1007`). It is
**not called at checkout**. What checkout does have is fail-closed *pricing*: a line that has left the
catalog entirely cannot be priced and rejects with `PriceUnresolvable`. **A line that is still in the
catalog but has been flipped `UNAVAILABLE`, or whose stock has gone to zero since it was added, prices
fine and is accepted.**

That window is exactly the peak window. A cart sits open for twenty minutes on a Friday at 19:30 while
the restaurant 86s a dish; the customer pays for it.

**The founder has now directed that it must be re-checked.** Recorded as **RSO-2**.

### The test that would not have caught it, and must be split when this lands

`specs/tests.yaml#/TestCartAddLineIsRejectedWhenOfferNotOrderable` declares `thrown:` as an **any-of over
three codes** — `OfferNotFound`, `OfferUnavailable`, `InsufficientStock` — while its `when:` uses
`offerId: "off-missing"`. **It therefore passes on `OfferNotFound` alone.** `require_stock_covers`
(`commands.rs:816-834`) could be **deleted entirely** and the suite would stay green.

That is a coverage lie, not a style issue: the stock guard has no test that fails when it is removed.
**When RSO-2 lands, that test splits into three** — one per code, each with a `when:` that can only
produce its own — and the checkout re-check gets its own tests on top. Recorded on RSO-2 so it is not
discovered again by the next reader.

## Part (c) — catalog snapshots: a new mechanism, with the founder's thresholds

**There is no event-sourcing snapshot mechanism in the tree today.** Verified: no snapshot table, no
snapshot event, no snapshot load path. **One false friend to name before someone finds it and thinks the
work is done**: `CatalogSnapshot` (`crates/application/src/pricing.rs:129`) is a **read-side pricing
helper** — it loads the projected catalog row once so `price_cart` walks it in memory instead of
re-reading per line. It has nothing to do with aggregate rehydration and is not a step toward this.

**Catalog is the right aggregate to start with, and the team had independently reached that conclusion
before the directive arrived** (recorded as **PMW-2** option (b) in
[ADR-20260815-030206](ADR-20260815-030206-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side.md)):

- **`CatalogImported` carries the WHOLE menu** — `categories[]`, `products[]`, `optionLists[]`
  (`specs/catalog/events.yaml:217-249`). One sync event is the entire catalog, not a delta.
- **`import_catalog` appends unconditionally** (`crates/application/src/commands.rs:3155`) — **no
  content-hash suppression**. A daily HubRise sync that changes nothing still appends a full menu.
  A year of that is **tens of megabytes in one stream**, and folding it is the actor's load time.
- **Residency makes it WORSE, not neutral.** `estimate_bytes`
  (`crates/infrastructure/src/mailbox/activation.rs:200`) sums serialized payload over the whole held
  stream, and `put_locked` (`crates/actor_runtime/src/activation.rs:142-181`) **inserts first, then
  evicts LRU** to get back under the 64 MB bound. The just-inserted entry has the highest `last_used`,
  so a large Catalog fill **evicts every resident Order, Cart and Payment first** — and a Catalog bigger
  than the bound evicts itself too. A HubRise import burst at peak makes every subsequent order delivery
  pay a cold refold.

**So the directive names the right target.** Snapshots are the recorded answer to exactly this shape, and
the founder has now supplied the policy: **snapshot every 100 events; actor load budget < 5 s.**

### What the design owes before it can be built — the open questions (SNAP-1)

- **Where does a snapshot live?** Its own table keyed by `(stream_name, version)`, or a `Snapshot` event
  written onto the stream itself? These are materially different: a table keeps the log pure and lets a
  snapshot be dropped at will; an on-stream event makes the snapshot part of the immutable history, which
  is precisely what a snapshot must **not** be.
- **How does it interact with `upcasting`?** A snapshot is a **materialized fold of events whose schema
  may evolve**. Young's rule is the one that decides it: **snapshots are disposable and rebuildable,
  never authoritative.** A snapshot taken before an upcaster existed must be *discardable*, not
  *upcastable* — which means every snapshot carries the code version that produced it, and a mismatch
  means "throw it away and refold", not "migrate it".
- **How does it interact with GDPR stream deletion?** The erasure path
  ([ADR-20260731-160000](ADR-20260731-160000-order-erasure-tombstone-then-stream-deletion.md)) deletes
  events. **A snapshot is a second copy of the data those events carried** — erasure that deletes the
  stream and leaves the snapshot has not erased anything. Whatever shape wins, the erasure path deletes
  the snapshot in the same transaction, and that must be *enforced*, not remembered.
- **Is the 100-event threshold per aggregate TYPE or global?** The founder's sentence is scoped to *"a
  catalog"*. An Order stream terminates in tens of events and would never benefit; a global rule would
  add a write to every aggregate for nothing. **Lean: per aggregate type, declared in the DSL**, so it is
  a spec decision like `binding:` is — but that is a recommendation, not a decision.
- **What is measured against the < 5 s budget, and where is it observable?** There is **no activation
  hit-ratio, bytes or eviction counter in `specs/observability.yaml` today** (already recorded as owed on
  PMW-2), so *"is actor load under 5 s?"* is currently **unanswerable from telemetry**. A budget nobody
  can measure is not a budget. This is owed with the mechanism, not after it.

## The bonus finding, filed loudly — BUS-1

While verifying part (b)'s peak-time window, the restaurant-facing push path was checked and it is
**already broken in shipped code**, in the exact shape the founder warned about in a different directive.

`operationStatusChanged` is a **declared product subscription** (`specs/common/api.yaml:234-243`, open to
every role path, ownership-scoped). It is served by the monolith
(`crates/server/src/graphql/schema.rs:219`) over a **process-local `tokio::broadcast`** whose payload
type `OperationUpdate` (`crates/actor_client/src/status_bus.rs:20-38`) **carries no serde**. Consequently:

- **post-split, the subgraph bins build fresh EMPTY buses** — `bin_support.rs:11-13, 39-41` constructs an
  `OperationStatusBus::default()` per bin *"so the schema always carries them"*, and its own header says
  the honest limit out loud: completions delivered by an actor bin *"reach this process's POLL reads but
  not its push subscribers"*;
- **the gateway refuses the WS handshake outright** — `crates/gateway_runtime/src/lib.rs:311-319` returns
  `501 NOT_IMPLEMENTED` with *"use POST; poll reads are authoritative"*;
- **so the client polls**: `crates/web/src/actions.rs:15, 26` — `POLL_MAX_ATTEMPTS = 30`,
  `POLL_INTERVAL = 1s`.

**Under [ADR-20260810-231300](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md)
this fails all three conditions**: the poll is the **primary** transport, not a declared degraded mode;
it is **not observably degraded** (no `*_push_down_total{reason}` contract exists for it); and there is
**no path back that anything detects**. It is *"just a poll with an excuse"*, by that ADR's own words.

**Two precision notes**, because the register row must be right:

- The poll's justifying comment (`actions.rs:30-34`) reads *"command handling is a single in-process
  journal write (typically sub-second)"*. It does **not** literally name the `command_journal` table. But
  its premise is stale for a stronger reason: **command handling stopped being an in-process write** when
  `PM_MAILBOX_DELIVERY` flipped and the journal was dropped
  ([ADR-20260812-000000](ADR-20260812-000000-the-pm-mailbox-flip-rides-the-journal-retirement.md)) —
  a mutation now enqueues on a mailbox lane drained by a **different process**. The 30-second ceiling was
  sized against a latency profile that no longer exists. (`telemetry::spans::command_journal` survives as
  a **span name** in generated mutation code; that is a naming leftover, not a table.)
- `operationStatusChanged` appears in `specs/screens/` **once**, at
  `specs/screens/restaurant_frontoffice.yaml:464` — and it is a **comment on the CUSTOMER checkout
  action**, describing how the confirmation screen resolves the outcome, not a bound subscription on the
  restaurant's order board. So the user who eats this today is the **customer staring at a spinner after
  paying**, which is if anything worse than the framing that prompted the check. The restaurant board's
  own liveness is a separate question and is not answered by this row.

Filed as **BUS-1**. It is the founder's own no-polling warning **already realised in shipped code**, on
the money path.

## Consequences

- **Positive.** Two real customer-facing defects are now recorded with their evidence and their closing
  shape, instead of living as a TODO comment and an absence. The catalog load hazard gets the founder's
  own policy attached to it. And a fourth defect (BUS-1) surfaced from the same sweep.
- **Negative / accepted.** All three parts are **new work that does not exist**; nothing ships with this
  record. Part (a) adds a guard to the hottest path in the product and part (b) adds catalog reads to
  checkout — **both cost latency at peak**, which is when they matter and when they hurt.
- **A constraint that outlives V0.** Parts (a) and (b) are the same principle applied twice: **the screen
  is an affordance, the server is the guarantee.** Any future check added to a screen inherits it.
- **Not decided here.** Where `isOpen` is derived (**RSO-1**), the fail-closed/fail-open posture and the
  test split (**RSO-2**), where snapshots live and how they meet upcasting and erasure (**SNAP-1**), and
  what replaces the poll (**BUS-1**).

## Consulted (ADR-20260812-143619 — one line per lens)

This record was written from a coordinator dispatch carrying findings already produced by the lenses
named below. **The remaining lenses have not yet answered on this directive** — that is stated rather
than elided, and it is an obligation carried on RSO-1 / RSO-2 / SNAP-1 / BUS-1: **no implementation
dispatch on any of them goes out without the full mob briefing first**, because each one touches the
checkout path.

- **ux**: Part (a) is a confirmed product gap, not a hardening item — `orderable` at
  `specs/network/api.yaml:21` does not consider hours and the PlaceOrder chain has no closed-hours
  guard, so the closed restaurant renders as open **and** accepts. On part (b), the posture question at
  the pay button is already answered: **fail-CLOSED on the money, fail-open on add-to-cart** — refusing
  a payment for a dish that just sold out is a recoverable disappointment; taking the money for it is
  not.
- **beck**: The existing stock test is an any-of over three codes and passes on `OfferNotFound` alone —
  `require_stock_covers` could be deleted with a green suite. Splitting it into three is part of RSO-2's
  definition of done, not a follow-up.
- **architect / dba** (carried forward from
  [ADR-20260815-030206](ADR-20260815-030206-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side.md)
  round 2, where these findings were produced): Catalog is the aggregate residency cannot serve —
  unconditional full-menu appends, LRU insert-then-evict, and a fill that evicts the money actors first.
  Snapshots are the recorded answer and PMW-2 option (b) already named them.
- **observability**: Owed with the mechanism, not after it — there are **no** activation hit-ratio,
  bytes or eviction counters in `specs/observability.yaml`, so the founder's **< 5 s** budget is
  currently unmeasurable, and BUS-1's poll has no `*_push_down_total{reason}` contract to make its
  degraded mode visible.
- **farley · graphql · holub**: not yet consulted on this directive (see the note above).
