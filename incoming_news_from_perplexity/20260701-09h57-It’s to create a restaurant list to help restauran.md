<img src="https://r2cdn.perplexity.ai/pplx-full-logo-primary-dark%402x.png" style="height:64px;margin-right:32px"/>

# It’s to create a restaurant list to help restaurant onboarding so the sirene api will be the source of truth

That changes the design significantly. If SIRENE is the source of truth, the flow is **pull-based pre-population**, not enrichment-on-demand.

## What this means architecturally

SIRENE becomes an **upstream system** in your context map — you're a **conformist** or **customer** of it. Your Restaurant BC doesn't own the restaurant's legal existence, SIRENE does.

```
SIRENE (upstream, source of truth)
    ↓ ACL (SireneTranslator)
RestaurantDirectory BC  ←── new, separate from Restaurant Onboarding
    ↓ (restaurateur claims a pre-registered restaurant)
Restaurant BC
    ↓
RestaurantActor (fully onboarded)
```


## A new bounded context: RestaurantDirectory

This is distinct from Restaurant Onboarding. Its job is:

- maintain a **searchable catalogue** of French restaurants from SIRENE
- enrich with Google Places / Perplexity
- allow a restaurateur to **claim** a restaurant ("c'est mon établissement")

```yaml
bounded_context: RestaurantDirectory
responsibility: Pre-populate and maintain a directory of French restaurants
source_of_truth: SIRENE (APE codes 56.x — restauration)
entities:
  - RestaurantDirectoryEntry  # NOT a Restaurant aggregate — just a record
    fields:
      - siret (unique, from SIRENE)
      - name
      - address
      - status: DISCOVERED | ENRICHED | CLAIMED | ONBOARDED
      - claimedBy?: restaurateurId
aggregates: none  # RestaurantDirectoryEntry is a simple entity, not an aggregate
                  # no complex invariants, no state machine needed
```


## Why RestaurantDirectoryEntry is NOT a RestaurantActor aggregate

A `RestaurantActor` owns the **operational state** of a live restaurant (menu, orders, payments). A `RestaurantDirectoryEntry` is just a **reference record** — it has no commands, no invariants beyond "siret is unique", no events that other actors react to.

Promoting it to a full aggregate would be overengineering for what is essentially a lookup table with a status column.

## The claim flow

```
Restaurateur signs up
  → searches RestaurantDirectoryEntry by SIRET or name
  → clicks "C'est mon restaurant"
  → ClaimRestaurantCommand { siret, restaurateurId }
    → RestaurantDirectoryEntry.status = CLAIMED
    → emits: RestaurantClaimed { siret, restaurateurId }
      → Restaurant Onboarding BC reacts
        → creates RestaurantActor with pre-filled data from RestaurantDirectoryEntry
        → Onboarding starts from a warm state, not blank
```


## SIRENE sync strategy

```
Cronjob (weekly or on-demand)
  → SIRENE API: fetch etablissements WHERE ape IN ['5610A','5610B','5610C','5630Z'...]
                AND etatAdministratif = 'A'  -- actif seulement
  → SireneACL.translate() → RestaurantDirectoryEntry upsert (ON CONFLICT siret DO UPDATE)
  → if new entry: dispatch EnrichWithGoogleCommand (async, rate-limited)
  → if SIRENE marks as closed: RestaurantDirectoryEntry.status = CLOSED (never delete)
```

The key point: **SIRENE data never flows directly into RestaurantActor**. It flows into `RestaurantDirectoryEntry`, and only when a restaurateur claims it does the data get translated into an `OnboardRestaurantCommand` that initialises the `RestaurantActor`.

