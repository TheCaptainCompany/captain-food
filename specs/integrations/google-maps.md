# Google Maps integration (restaurant base data sync)

A second restaurant **pre-registration source** alongside Sirene ([sirene.md](sirene.md)): a scheduled
sync reads **Google Maps Places** to build a base of food establishments — including the brands a single
SIRET hosts (dark kitchens) that Sirene cannot separate. It goes through the **`sirene-google-acl`**
adapter (c4-l3) which **calls the `Restaurant` aggregate's normal commands at `/external/graphql` as the
owner** (no special command). Sirene and Google run **independently** and both feed the *same* generic
commands, each keyed by its own id in `externalIdentifiers`.

## Scope: what we fetch & store

`place_id`, `location` (lat/long), `displayName`, `address`, `openingHours`. **Not** photos, **not**
menus, **not** reviews text (see Legal).

## 1. Data used from Google Maps Places

Per place: `place_id`, `geometry.location` (lat/long), `name`, `formatted_address` / components,
`opening_hours`, and (for the GBP-specific channel) `rating` + `user_ratings_total`. Requires a Google
Maps Platform API key; attributions shown wherever Google content is displayed.

## 2. Google Maps → Captain.Food domain mapping

General restaurant info flows through `Register`/`UpdateRestaurant`; only Google's own metrics use the
GBP channel — keeping the two sources decoupled.

| Google Maps | Domain | Note |
|---|---|---|
| `place_id` | `externalIdentifiers[{key:"google_place_id"}]` + `googlePlaceId` | the sync's idempotency key (one listing per place) |
| `geometry.location` | `RegisterRestaurant.location` / `UpdateRestaurant.location` (`GeoPoint`) | general restaurant info |
| `name` | `displayName` | |
| `formatted_address` | `address` (`Address`) | |
| `opening_hours` | `openingHours` | |
| derived | `slug` (ACL generates; suffix on `SlugAlreadyTaken`) · `listingStatus = NON_PARTNER` | seeded listing |
| `rating` / `user_ratings_total` | `UpdateRestaurantGoogleBusinessProfile` → `RestaurantGoogleBusinessProfileUpdated` | GBP-specific metrics only |

A Google-seeded listing has **no** `accountId` (no owner yet). Tags/website/contact stay empty until the
owner completes them (UI-time Google assist or claim).

## 3. Request / report split (command, not inbound event)

Like Sirene and HubRise, the sync is **orchestrated and rejectable** (we filter, generate slugs, can be
told "no") → it issues **commands**, not inbound events. Google is **polled** on a schedule (no webhook).

## 4. The cron

- Periodic sync over the Tours/Touraine area (food place types). New place → `RegisterRestaurant`
  (`NON_PARTNER`); changed name/address/hours/coords → `UpdateRestaurant`; refreshed rating/reviews →
  `UpdateRestaurantGoogleBusinessProfile`. **Idempotent**, keyed on `place_id`.
- **Dark kitchens:** Google distinguishes brands that share one SIRET/address, so the Google sync can seed
  listings Sirene can't — the two sources reconcile per their own ids (`google_place_id` vs `siret`); a
  listing may end up carrying both.

## 5. Legal / ToS — risk explicitly accepted (CTPO)

Decision: store the **factual** fields above; they are public facts (not copyrightable). Specifics:
- **`place_id`** — storing is **explicitly permitted** (Google's documented caching exception).
- **lat/long** — Google permits caching **≤ 30 days**; the periodic re-sync stays within that window.
- **name / address / opening hours** — facts, so the only constraint is Google Maps Platform's
  **no-storage ToS clause** — a **contractual** risk **knowingly accepted** by the CTPO (these same facts
  are also available from Sirene / the restaurant, which mitigates lock-in).
- **Photos & menus are never fetched/stored** — that was the real copyright hard-stop (user-owned photos;
  no license-clean menu feed). Photos remain a **UI-time, attributed, non-stored** display only.
- Attributions shown wherever Google content is displayed; every non-partner card keeps the ADR-0019 opt-out.

## 6. Runtime & monitoring (deferred until app code exists)

- GitHub Actions cron + worker calling `/external/graphql` (role `EXTERNAL`); the domain API must exist
  first, so this doc is the **contract**. Backoff/quotas per the Maps key; graceful degradation if Google
  is unavailable. Env: `GOOGLE_MAPS_API_KEY`, `CAPTAIN_API_URL`, `EXTERNAL_API_TOKEN`.

## 7. Gaps / deferred

- **Prospection scoring + outreach** is a separate later step (score = read-model projection, not an event).
- A combined Sirene+Google reconciliation/observability contract lands with that step.
