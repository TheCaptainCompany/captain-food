# ADR-0033 — Spec-Driven Server-Driven UI (SDUI) + translations catalog

## Status

Accepted

## Context

The customer web app needs a UI spec that (like the domain DSL) is a validated single source of truth, and
a way to **prove the GraphQL API actually serves what the screens need**. A Perplexity note
(`SDUI_ARCHITECTURE`) framed the approach: **Spec-Driven Server-Driven UI** — screens declared as component
trees, a component registry, per-screen data needs bound to resolvers, an action allowlist, and i18n keys;
the layout layer is runtime-editable (Supabase) while the data layer is code. `specs/customer_screens.yaml`
existed but was an unvalidated island with hardcoded English strings.

## Decision

1. **`customer_screens.yaml` is the SDUI source of truth**, registered in the codegen `SOURCE_FILES` and
   validated. Two layers: a **layout** layer (screens/components/props) and a **data** layer:
   - **`resolvers`** (reads) — a dot-notation allowlist, each `$ref`-bound to an `api.yaml` **query** (or a
     declared `gap`). Screens declare `data_requirements: [<resolver>]`.
   - **`actions`** (writes + client behaviours) — the registered action-type allowlist; mutation-bearing
     actions `$ref`-bind to an `api.yaml` **mutation** (or a `gap`).
   A screen referencing a non-existent op fails validation (`ref-dangling` / `resolver-not-a-query` /
   `action-not-a-mutation`) — **this is the API-meets-UI gate**. Resolver/action bindings + `data_requirements`
   resolution are enforced.
2. **New `specs/translations.yaml`** (modelled on `errors.yaml`): every user-visible string is a dotted key
   with optional typed **`params`** (for contextualization, `{token}` like error messages) and
   `messages.en`/`fr`. Screens reference `translations.yaml#/<key>` by `$ref` — **no hardcoded UI text**. The
   validator checks both locales exist and that `{param}` tokens match the declared `params`. Generated to a
   single **`translations.generated.json`** (`{ "<key>": { en, fr } }`).
3. **Non-SDUI screens are flagged** (`sdui: false` + `sdui_reason`): Checkout (Stripe Elements/payment),
   Order confirmation (realtime subscription + state machine), Auth OTP — static React pages, not runtime JSON.
4. **Missing UI needs are surfaced as `gaps`, never silently invented ops.** Found: promo codes/deals
   (`apply_promo_code`, cart discount), dish/product search, promotions feed, Captain Coins/loyalty + referral,
   passkeys/notifications management, cart-level delivery-mode toggle, reorder, and restaurant presentation
   fields (coverUrl/logoUrl/deliveryTime/deliveryFee/minimumOrder/badges/isOpen).
5. **Documented**: the generated docs gain a **📱 Customer screens** section (per screen: a mockup, an
   API-operations table with each read/write linked to its api anchor, and its ⚠️ gaps) and a **🌐 Translations**
   table. `component_registry` + `actions` declare the allowlists now.

### Deferred to runtime (contracts, when apps/ exists)
The SDUI **runtime**: the recursive renderer, the generated `registry.ts` / `types/screens.ts` / `i18n/keys.ts`,
the Supabase `screen_specs` table + validation trigger, `resolvers.ts` / `action-dispatcher.ts`, the
`/api/screens/:id` hydration route, and enforcing the component registry & i18n-key coverage in generated code.
Recorded like the observability P-items; promote when `apps/web-client` lands.

## Alternatives considered
- **Keep the screens file unvalidated / hardcoded strings** — the exact drift risk this closes. Rejected.
- **Bind screens directly to api ops (no resolver layer)** — loses the SDUI data-layer allowlist and the
  dot-notation resolver contract from the architecture. Rejected in favour of the resolver indirection.
- **Invent the missing ops (promo/loyalty/dish search)** — those are deferred product domains; fabricating
  them hides the real backlog. Surfaced as `gaps` instead.
- **Build the runtime now** — needs `apps/` (doesn't exist); would fork a second codegen prematurely. Deferred.

## Consequences
### Positive
- The API is provably sufficient for every screen's declared reads/writes (or the gap is explicit); UI text is
  centralized, translated, and generated; the docs show each screen as a mockup wired to real operations.
### Negative
- Adds a large spec surface (screens + 149 translation keys) to maintain; the two-layer runtime is still to build.

## Influences
This project's method stands on: **Eric Evans** (DDD, Bounded Context, Anti-Corruption Layer), **Greg Young**
(CQRS, Event Sourcing), **Vaughn Vernon** (implementing DDD, the actor model), **Kent Beck** (TDD — our
behaviour tests), and **Jeff Patton** (User Story Mapping — `stories.yaml`). SDUI itself follows Airbnb's
server-driven UI ("Ghost Platform").

## References
Perplexity `SDUI_ARCHITECTURE` note; `specs/customer_screens.yaml`, `specs/translations.yaml`,
`specs/generated/translations.generated.json`; `tools/codegen/src/{model,validate,cli}.ts`,
`src/emit/translations.ts` + `documentation{,-html}.ts`. Complements ADR-0032 (completeness gates), ADR-0006
(role=path API), ADR-0015 (Supabase Auth), ADR-0028 (pricing) & ADR-0031 (delivery).
