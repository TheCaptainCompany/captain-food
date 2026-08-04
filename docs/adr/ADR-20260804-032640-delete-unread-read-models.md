# ADR-20260804-032640 — Delete unread read models (View_RestaurantAccount, PhoneCountry)

## Status

Accepted

## Context

[ADR-20260804-014546](ADR-20260804-014546-read-models-declare-their-readers.md) made "every read model
declares a reader" an executable gate (`read-model-no-reader`). It shipped with a deliberately **bounded
claim**: it proves a read model has *a declared* reader, not that any code actually reads it. Two things
fell out of that bound, and the product owner directed both be deleted rather than carried:

- **`View_RestaurantAccount`** passed the gate only through the `internal: true` exemption — the sole
  such exemption in the whole database spec. It had no api.yaml binding and no `components.*.reads`
  entry, and a literal search for the view name across `crates/**` and `tools/**` returned **zero**
  hits. The 55 `restaurant_account` matches are the *aggregate* and the `restaurant_account_id`
  *column*, not the view. Its own note claimed the RegisterRestaurant handler consulted it; the handler
  in fact folds the event store.
- **`PhoneCountry`** is a `reference: true` table, which the gate does not check at all (the open hole
  tracked by [#337](https://github.com/TheCaptainCompany/captain-food/issues/337)). Its only query,
  `phoneCountries`, was deleted as unreached in
  [#333](https://github.com/TheCaptainCompany/captain-food/pull/333); the table was kept then. It has
  zero references anywhere in `crates/**` or `tools/**`.

"No declared reader" turned out **not** to be the same as "unused". A trial deletion of
`View_RestaurantAccount` produced **3 validator errors**, because the view was load-bearing in ways the
reader gate does not measure: `Restaurant.restaurant_account_id` carried an `fk:` into it (a node in the
read-navigation graph), and `projection-updaters` listed it in `updates[*]` (the write side maintained
it). It was also the only projection of `RestaurantAccountUpdated` and `RestaurantAccountDeleted`.

## Decision

Delete both read models, and record the resulting hole honestly rather than disguise it.

- Remove `View_RestaurantAccount` from `projection_views.yaml`, drop the `fk:` on
  `Restaurant.restaurant_account_id`, and remove the `projection-updaters` `updates[*]` entry.
- Remove `PhoneCountry` from `tables/referential.yaml`.
- List `RestaurantAccountUpdated` and `RestaurantAccountDeleted` under `nonProjectedEvents`, and **widen
  that list's documented meaning** to cover two distinct reasons: (a) transient/saga-internal facts, the
  original meaning, and (b) **recorded but unread** — durable facts appended to the log with no read
  model because nothing queries them. Filing these two under (a) would have been false: an account
  update and an account deletion are neither transient nor returned in a mutation payload.

The `restaurant_account_id` column itself stays, still indexed — `restaurantLocationsByAccount` queries
by it. It simply no longer points at a read model, so there is no read-navigation edge to declare.

## Alternatives considered

- **Keep `View_RestaurantAccount`, replace `internal: true` with its real consumer** — the safe option,
  and the one this session recommended. Rejected by the product owner: nothing reads it, and a read
  model kept because it might be wanted is the speculative infrastructure the reader gate exists to find.
- **Delete the view but first fold `RestaurantAccountUpdated` into the `Restaurant` projection**, so
  account changes still reach the location rows and no hole opens. Rejected as scope: that is new
  projection work, not a deletion, and it should be driven by the surface that needs it.
- **Keep `PhoneCountry` as reference data for upcoming phone/address work** — the standing decision from
  #333. Reversed here: it is cheaper to re-add four seeded columns than to carry a table nothing reads.

## Consequences

### Positive
- The single `internal: true` exemption in the database spec is gone; every surviving read model is now
  held up by a *positive* declaration (an api.yaml binding or a `components.*.reads` entry) rather than
  by an exemption. The exemption branch of `read-model-no-reader` is now unused in practice.
- One fewer generated SQL view and one fewer seeded table. No `crates/**` file changed as a result of
  either deletion — direct confirmation that nothing read them.
- `view-fedby-unused` drops from 2 to 1.

### Negative
- **A known hole, deliberately accepted**: an account legal-name or timezone change
  (`RestaurantAccountUpdated`) and an account deletion (`RestaurantAccountDeleted`) are recorded in the
  event log and propagate to **no read model**. The account's creation fact still reaches one — the
  `Restaurant` projection folds `RestaurantAccountRegistered` for `default_currency` — so account data is
  correct at creation and silently stale afterwards. A back-office account surface must add a
  projection, not merely a query.
- `RestaurantAccountDeleted` is a **tombstone/erasure-adjacent** fact now sitting unprojected. This is
  not a GDPR erasure path (that is the deletion engine's own work), but anything building on account
  deletion must not assume a read model reflects it.
- `nonProjectedEvents` now mixes two meanings. Mitigated by the per-entry comments and the widened
  header, but the list is doing more work than its name suggests.

### Follow-up actions
- Filed as a successor issue: fold `RestaurantAccountUpdated` into a read model when a back-office
  account surface is specified, and remove both entries from `nonProjectedEvents` then.
- [#337](https://github.com/TheCaptainCompany/captain-food/issues/337) (referential tables are never
  checked for a reader) is unchanged by this and still open — `PhoneCountry` was found by hand, not by
  the gate. That is the argument for closing #337 rather than relying on manual sweeps.
