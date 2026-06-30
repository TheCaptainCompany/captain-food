# INSEE Sirene integration (restaurant pre-registration sync)

Captain.Food **pre-registers** Touraine food establishments from public data so the marketplace launches
with coverage and a B2B pipeline (ADR-0019). The source is the open **Recherche d'entreprises API**
(`recherche-entreprises.api.gouv.fr`, Etalab) over INSEE **Sirene**. A scheduled **`sync-worker`**
(c4-l2; ADR-0020) reads Sirene and goes through the **`sirene-google-acl`** adapter (c4-l3), which
**calls the `Restaurant` aggregate's normal commands at `/external/graphql` as if it were the owner** —
there is no special pre-registration command. The Sirene SDK never leaks into the aggregate.

> Google Maps is a **separate, independent sync source** — see [google-maps.md](google-maps.md). Sirene
> and Google run side-by-side and both feed the *same* generic `Restaurant` commands (the decoupling
> principle), each keyed by its own id in `externalIdentifiers` (`siret` vs `google_place_id`).

## 1. Data exposed by Recherche d'entreprises / Sirene

Per **établissement** (SIRET level): `siret` (SIREN+NIC), `siren`, legal/trade name (`nom_complet`,
`enseigne`), address (`adresse`, `code_postal`, `code_commune`, `commune`), `activite_principale` (NAF),
`date_creation`, and administrative state (`etat_administratif` = `A` active / `F` closed). Open data,
**no API key**; **7 req/s per IP** (HTTP 429 + `Retry-After` on overflow); paginate.

## 2. Sirene → Captain.Food domain mapping

| Recherche d'entreprises | Domain | Note |
|---|---|---|
| `siret` | `externalIdentifiers[{key:"siret"}]` | the sync's idempotency key (one seed listing per SIRET établissement) |
| `activite_principale` (NAF) | `externalIdentifiers[{key:"naf"}]` | also the prospection filter (below) |
| `nom_complet` / `enseigne` | `RegisterRestaurant.displayName` | enseigne preferred when present |
| `adresse` / `code_postal` / `commune` | `RegisterRestaurant.address` | `Address{line1,postalCode,city,country:"FR"}` |
| derived from displayName + commune | `RegisterRestaurant.slug` | ACL generates a candidate; on `SlugAlreadyTaken` it suffixes and retries |
| (none) | `RegisterRestaurant.listingStatus = NON_PARTNER` | seeded listing — not orderable, no agreement |
| `etat_administratif = F` | `MarkRestaurantClosed` | closure detected on a known listing |
| name/address change | `UpdateRestaurant` | full-replace of the changed fields |

The cron sets **no** `accountId` (no owner yet) and **no** Google fields (UI-time). General restaurant
info beyond name/address (website, tags, hours, phone) is left empty until the owner completes it.

## 3. Request / report split (command, not inbound event)

The sync is **orchestrated by us and rejectable** — we choose which établissements to ingest (NAF/postal
filters), generate slugs, and can be told "no" (`SlugAlreadyTaken`, validation). So it issues
**commands** (CLAUDE.md rule), exactly like `ImportCatalog` orchestrating HubRise — **not** inbound
events. Sirene is **polled**, not pushed (no webhook); freshness is bounded by the schedule.

## 4. The cron (ADR-0020)

- **Weekly, Mon 03:00 — Sirene sync.** For the target scope (below): new active établissements →
  `RegisterRestaurant` (`NON_PARTNER`); `etat_administratif=F` on a known listing → `MarkRestaurantClosed`;
  name/address drift → `UpdateRestaurant`. Each run is **idempotent** (keyed on `siret`): re-running never
  duplicates. Write a run summary to `sync_logs`; **alert ops (Slack)** if an *active partner* turns
  `F` in Sirene.
- **Target scope (Tours / Touraine):** NAF `56.10A`, `56.10B`, `56.10C`, `56.21Z`, `56.29A`, `56.29B`;
  `code_postal` ∈ `37000, 37100, 37200, 37300, 37270, 37250, 37400`. Active établissements only for new seeds.
- **Identity & dark kitchens:** one seed listing **per SIRET établissement**. SIRET is **not** a unique
  key for a Captain *restaurant* — a single SIRET/address can host several dark-kitchen brands that Sirene
  cannot separate; those brands are split later via the **owner's claim** + the UI-time Google assist, not
  by this cron.

## 5. Legal / ToS

- Recherche d'entreprises / Sirene is **Etalab open data (Licence Ouverte)** — **commercial use OK**;
  we use only public legal-entity/establishment facts.
- Every non-partner card carries the ADR-0019 **opt-out** ("This is my restaurant — edit / remove").
- **No scraping** of aggregators (Uber Eats/Deliveroo) and **no** importing third-party menus/photos
  without the restaurant's consent (ADR-0019). Google's factual fields are synced separately
  ([google-maps.md](google-maps.md)); **photos/menus are never grabbed** (copyright).

## 6. Runtime & monitoring (deferred until app code exists)

- Built as a **GitHub Actions cron + worker** (ADR-0020) calling `/external/graphql` with role `EXTERNAL`;
  the domain API must exist first, so this doc is the **contract**, not yet an implementation.
- Backoff on 429 (`Retry-After`); graceful degradation if Sirene is unavailable (skip run, log, retry next schedule).
- Env: `RECHERCHE_ENTREPRISES_BASE_URL` (no key), `CAPTAIN_API_URL`, `EXTERNAL_API_TOKEN`, `SLACK_WEBHOOK_OPS`, `DATABASE_URL`.

## 7. Gaps / deferred

- **Prospection scoring + B2B outreach** (HubSpot/Resend/Slack sequences) is a **separate later step**;
  when built, the score is a **read-model projection** over the listing events, never stored in an event.
- **Observability contract** for the sync workflow lands with that step.
- INSEE **Sirene API** (keyed) is an alternative source if the open API's freshness/limits become a
  constraint; the mapping above is unchanged.
