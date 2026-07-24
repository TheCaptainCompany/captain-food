# PROP-20260724-164102 — First-class pagination on `queries/restaurants`
- **Status**: Approved (product-owner standing directive, 2026-07-24: "continue to the next issues without my approval")
- **Date**: 2026-07-24
- **Tracking issue**: [#113 "First-class pagination on queries/restaurants (turn the #107 LIMIT 200 adapter guard into a contract)"](https://github.com/TheCaptainCompany/captain-food/issues/113)
- **Realized by**: (pending)

## Why

[#107](https://github.com/TheCaptainCompany/captain-food/issues/107) OOM-killed the 512Mi instance
because `PgRestaurantRepository::list()` had no bound; PR
[#108](https://github.com/TheCaptainCompany/captain-food/pull/108) hotfixed a defensive `LIMIT 200`
in the adapter. But the cap is INVISIBLE in the contract: with SIRENE-scale listings the marketplace
silently shows at most 200 and cannot page past them, and a client has no way to ask for the next
set. Pagination must be in the API surface, not a hidden adapter guard.

## Decision — offset/limit with a server-enforced maximum

Add two optional args to `queries/restaurants`:

```yaml
    args:
      # … existing filters …
      limit:  { $ref: 'scalars.yaml#/PageLimit' }    # page size; server clamps to PAGE_LIMIT_MAX (200)
      offset: { $ref: 'scalars.yaml#/PageOffset' }   # rows to skip; default 0
```

- New scalars `PageLimit` / `PageOffset` (integers) — first-class, reusable by other list queries
  later (`orders`, `pendingRefunds`) but this proposal touches only `restaurants`.
- **The `LIMIT 200` becomes the MAX, not the default**: the repo applies
  `LIMIT least(limit ?? DEFAULT, MAX) OFFSET offset`. `DEFAULT` = 24 (a discovery screen's first
  page), `MAX` = 200 (the #108 constant, now a named ceiling). A caller asking for more than the max
  gets the max — a clamp, never an error, never an unbounded scan.
- The discovery screens' `restaurants.*` resolvers gain the pins/args they need (the featured rail
  stays `list: RECOMMENDED` + a small limit); "load more" passes `offset`.

## Why offset/limit over cursor

- **Offset/limit** fits V0's read model exactly: `list()` is a single `ORDER BY created_at DESC`,
  so `LIMIT/OFFSET` is a one-line SQL change and the client's "page 2" is `offset += limit`. The
  marketplace is a browse-and-filter surface, not an infinite real-time feed.
- **Cursor (keyset)** is more robust against inserts mid-scroll and avoids deep-offset cost — the
  right choice IF discovery becomes a high-churn infinite feed. It needs an opaque cursor scalar, a
  stable total-order tiebreak, and encode/decode on both sides. Deferred as the considered
  alternative: the wire shape can move to cursor later behind the same "page the restaurants query"
  intent without a domain change. At V0 Tours scale (hundreds of listings, not millions) deep-offset
  cost is a non-issue.

## Scope of change

| Layer | Change |
|---|---|
| `specs/scalars.yaml` | + `PageLimit`, `PageOffset` (integer) |
| `specs/api.yaml` | `restaurants` args gain `limit`/`offset`; regenerated SDL + input type carry them (the [#97](https://github.com/TheCaptainCompany/captain-food/issues/97) generated-name machinery) |
| `specs/screens/*` | discovery resolvers pin a sensible default limit; "load more" wiring is a client follow-up (not this PR) |
| `crates/application` | `RestaurantFilter` gains `limit`/`offset`; the port doc states the clamp |
| `crates/infrastructure` | `PgRestaurantRepository::list()` applies `LIMIT least(…, MAX) OFFSET` — the #108 guard becomes the clamp ceiling |
| completeness (ADR-0032) | no new op/command/event/error → no new story/test/rule; the existing `restaurants` story step is unchanged (args are optional) |

## Verification

- `make validate` 0 errors (optional args don't change story coverage); generated SDL/data layer
  show `limit`/`offset` on `RestaurantsQueryInput`.
- A repo test (Pg-gated) proves: default page size when unset; the clamp when `limit > MAX`; `offset`
  skips; ordering stable.
- `make rust` green, no unintended drift.
