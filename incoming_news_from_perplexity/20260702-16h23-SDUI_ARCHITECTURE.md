# Captain.Food — SDUI Architecture

## What This Is

This project uses **Spec-Driven Server-Driven UI (SDUI)**. The UI specification YAML file (`captain_food_ui_spec.yaml`) is the **single source of truth** for the entire frontend. It governs:

- Every screen, layout, and component that can exist
- The component registry (which `type:` keys are valid)
- Data hydration requirements per screen (which GraphQL resolvers are called)
- i18n key contracts (which translation keys must exist)
- TypeScript prop types for every component
- Database migrations (screen specs stored in Supabase)

**Do not invent components, types, or screen structures that are not declared in the spec.** If something is missing from the spec, update the spec first, then regenerate the derived artifacts.

---

## Architecture Overview

```
captain_food_ui_spec.yaml          ← THE LAW. Edit this first, always.
        │
        ├── pnpm generate           ← run locally after every spec change
        │       ├── src/registry.ts                  (component map)
        │       ├── src/types/screens.ts             (TypeScript prop types)
        │       ├── src/i18n/keys.ts                 (canonical i18n key list)
        │       └── supabase/migrations/XXXX_screens.sql  (screen JSON rows)
        │
        ├── CI (on push)
        │       └── re-runs generate → git diff --exit-code
        │           (fails if dev forgot to regenerate after spec change)
        │
        └── Runtime (production)
                ├── GET /api/screens/:screenId
                │       ├── reads spec_json from Supabase screen_specs table
                │       ├── resolves data_requirements in parallel (RSC)
                │       ├── injects hydrated data into spec JSON
                │       └── returns fully hydrated JSON to client
                │
                └── <SDUIRenderer node={hydratedSpec} registry={registry} />
                        └── maps type → React component, renders recursively
```

---

## The Two Layers of SDUI

### Layer 1 — Layout Layer (in Supabase, changeable without deploy)
- Stored in table `screen_specs (screen_id, spec_json, updated_at)`
- Controls component arrangement, order, visibility conditions, props
- Validated against the spec schema on every write (Supabase trigger)
- Change a promo banner, reorder sections, toggle a feature → edit Supabase row → live immediately

### Layer 2 — Data Layer (in code, requires deploy to change)
- `data_requirements` per screen declare which resolvers are called
- Resolvers are an **allowlist** in `src/sdui/resolvers.ts` — nothing outside this list can be called from a spec
- Resolver names use dot notation: `restaurants.featured`, `promotions.active`, `categories.all`
- Arguments support context interpolation: `{ city: "{{user.city}}", userId: "{{user.id}}" }`

---

## Component Registry Rules

Every React component in this project that is rendered by the SDUI engine **must**:

1. Be declared in `captain_food_ui_spec.yaml` under `components_library` or referenced in a `screens[].components` block
2. Have its `type:` key registered in `src/registry.ts`
3. Have its props typed via the generated `src/types/screens.ts`
4. Accept unknown extra props gracefully (forward-compatibility)

**Never add a component to `registry.ts` manually.** The registry is generated from the spec. If you need a new component:
1. Add its `type:` and `props:` definition to the spec YAML
2. Run `pnpm generate`
3. Implement the React component to match the generated prop types

---

## i18n Contract

All user-visible strings in the spec JSON **must be i18n keys**, never hardcoded text.

```json
// ✅ Correct
{ "type": "promo_banner", "props": { "title": "home.promo.title" } }

// ❌ Wrong — never do this
{ "type": "promo_banner", "props": { "title": "Offre du jour" } }
```

The canonical list of required keys lives in `src/i18n/keys.ts` (generated).
The CI checks that `public/locales/en.json` and `public/locales/fr.json` cover every key in this list.

---

## Screen Hydration — How It Works

The Next.js RSC for each screen does exactly this, in order:

```typescript
// app/screens/[screenId]/page.tsx (Server Component — simplified)
const spec = await getScreenSpec(screenId);               // from Supabase
const data = await resolveAll(spec.data_requirements, ctx); // parallel GQL calls
const hydrated = injectData(spec.spec_json, data);         // merge
return <SDUIRenderer node={hydrated} registry={registry} />;
```

**Rules:**
- All data fetching happens server-side. The client receives a fully hydrated JSON — no secondary data calls from the browser.
- `resolveAll` calls all resolvers in parallel (`Promise.all`). Never sequential.
- Resolver errors are isolated: one failed resolver returns `null` for that slot, the screen still renders.
- The client-side renderer is intentionally dumb — it maps types to components and recurses. No data fetching, no business logic.

---

## Actions

User interactions are declared in the spec as `action` objects. The client-side `ActionDispatcher` handles them.

```json
{ "type": "button", "props": { "label": "home.cta.order_now" },
  "action": { "type": "navigate", "route": "/r/{{item.slug}}" } }
```

**Registered action types** (all others are silently ignored):
- `navigate` — client-side route push
- `open_bottom_sheet` — opens a sheet by `sheet_id`
- `close_sheet` — closes active sheet
- `add_to_cart` — dispatches cart mutation
- `change_cart_line_quantity` — dispatches cart mutation
- `apply_promo_code` — dispatches cart mutation
- `graphql_mutation` — named mutation from the allowlist
- `set_delivery_address` — updates delivery context
- `use_geolocation` — requests browser geolocation
- `sign_out` — Supabase auth sign out
- `send_otp` / `verify_otp` / `resend_otp` — auth flow
- `authenticate_passkey` — WebAuthn flow
- `toggle_favorite` — requires auth
- `reorder` — repopulates cart from past order lines
- `copy_to_clipboard` — copies value to clipboard
- `share` — Web Share API
- `phone_call` — `tel:` link

Adding a new action type requires:
1. Adding it to the spec under the relevant component
2. Implementing its handler in `src/sdui/action-dispatcher.ts`
3. No other files need to change

---

## Screens That Are NOT SDUI-Rendered

The following screens are implemented as **standard Next.js pages with static React components**. They have transactional or security constraints that must not be driven by runtime JSON:

| Screen | Route | Reason |
|---|---|---|
| Checkout | `/checkout` | Stripe Elements, payment security |
| Order confirmation | `/orders/:id/confirmation` | Real-time subscription, GraphQL state machine |
| Auth OTP entry | (bottom sheet) | Supabase Auth flow integrity |

Do not attempt to move these screens into the SDUI renderer.

---

## Key Files

```
captain_food_ui_spec.yaml       ← spec source of truth (edit this)
src/
  sdui/
    renderer.tsx                ← recursive SDUI renderer (~30 lines)
    registry.ts                 ← GENERATED — do not edit manually
    action-dispatcher.ts        ← handles all action types
    resolvers.ts                ← allowlist of data resolvers
    inject-data.ts              ← merges resolved data into spec JSON
    validate-spec.ts            ← runtime JSON validation against spec schema
  types/
    screens.ts                  ← GENERATED — do not edit manually
  i18n/
    keys.ts                     ← GENERATED — do not edit manually
  components/                   ← one file per registry type
    RestaurantCard.tsx
    PromoBanner.tsx
    CategoryPill.tsx
    ... (one per type in spec)
supabase/
  migrations/
    XXXX_screen_specs.sql       ← GENERATED — do not edit manually
scripts/
  generate.ts                   ← the codegen script (run via pnpm generate)
```

---

## Non-Negotiable Rules for Claude Code

1. **Never modify generated files** (`registry.ts`, `types/screens.ts`, `i18n/keys.ts`, migration SQL). These are outputs, not inputs.
2. **Never add a hardcoded string** to a component rendered by the SDUI engine. Use i18n keys.
3. **Never fetch data inside a component** rendered by the SDUI engine. All data comes from the hydrated spec JSON via props.
4. **Never add a component to the registry manually.** Update the spec, run generate, implement the component.
5. **If a screen layout change is needed**, update the spec JSON in Supabase (or the fixture file in `fixtures/`) — do not change React component structure just to rearrange layout.
6. **If a new data requirement is needed for a screen**, add it to `data_requirements` in the spec first, then add the resolver to `src/sdui/resolvers.ts`.
7. **The spec is always right.** If there is a conflict between the spec and the code, the spec wins. Fix the code.
