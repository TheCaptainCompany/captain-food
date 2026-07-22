# ADR-20260722-091500 — SDUI screens organized by audience; drop the `_screens` suffix (refines ADR-0037)

## Status

Accepted (product-owner directive, 2026-07-22). Refines **ADR-0037** (§2 screens naming, §4 admin/system);
builds on **ADR-0033** (Spec-Driven SDUI).

## Context

ADR-0033 introduced Spec-Driven Server-Driven UI with a single `customer_screens.yaml`. ADR-0037 §2 then
proposed **one file per role**, named `{role}_screens.yaml` (`customer_screens.yaml`,
`restaurant_screens.yaml`, `rider_screens.yaml`) inside `specs/screens/`, and §4 decided screens are
role-tagged with **no ADMIN screen set** — admin being a back office plus impersonation ("view as").

Two things surfaced as the frontend work began (#21):

- The `_screens` **suffix is redundant** once the files already live in a `screens/` folder.
- **"Customer" is the wrong axis.** The surface at `{slug}.captain.food` is the restaurant's **front
  office** (its customer-facing storefront); the staff dashboard is its **back office**. Naming by
  **audience** (frontoffice / backoffice / rider / system) is clearer, scales, and pairs the two
  restaurant-facing surfaces under one prefix.

## Decision

1. **One SDUI screens file per audience, in `specs/screens/`, with NO `_screens` suffix** (the folder
   conveys it). Two of these are **customer-facing front offices** distinguished by host binding:
   - **`captain_frontoffice.yaml`** — the **Captain marketplace**: cross-restaurant discovery and search,
     served at **`live.captain.food`** today and the **bare domain `captain.food`** later (roles
     PUBLIC + CUSTOMER). *To be created* — the marketplace discovery screens (`home`, `search`,
     categories) currently living in `restaurant_frontoffice.yaml` are earmarked to move here (see the
     content-split follow-up below).
   - **`restaurant_frontoffice.yaml`** — a **single restaurant's storefront** (catalog → cart → checkout
     → order tracking), served per-tenant at **`{slug}.captain.food`** (roles PUBLIC + CUSTOMER).
     **Renamed from `customer_screens.yaml`**; content unchanged *for now* (it still holds the marketplace
     discovery screens pending the split above).
   - `restaurant_backoffice.yaml` — restaurant-staff dashboard (to follow).
   - `rider.yaml` — delivery-rider app (to follow).
   - `system.yaml` — platform/admin surface (to follow).
2. **Audience sets MAY include a `system` set**, refining ADR-0037 §4's blanket "no ADMIN screen set".
   Impersonation ("view as") stays the mechanism for acting *as* a restaurant/customer; a `system.yaml`
   may still exist for platform-native ops screens. The concrete choice is deferred to when that surface
   is built — this ADR only removes the blanket "no system screens" stance.
3. **Host bindings** are documented in each file header and here; **no new DSL schema field** — host
   resolution lives in the server tenant middleware (C4 `middleware/tenant`):
   - `captain_frontoffice.yaml` → `live.captain.food` (then the bare `captain.food`) — the platform
     marketplace host.
   - `restaurant_frontoffice.yaml` → `{slug}.captain.food` — the per-tenant restaurant host (wildcard
     `*.captain.food`).
4. The codegen **validator is already generic** over `screens/*.yaml`; only the doc / translation /
   component-registry **emitters** pinned the single filename and are updated to the new name.
   Generalizing those emitters to iterate over **all** `screens/*.yaml` (so new audiences appear in the
   generated docs automatically) is a follow-up, not part of this change.

## Consequences

- `git mv specs/screens/customer_screens.yaml specs/screens/restaurant_frontoffice.yaml`; the codegen
  `SPEC_FILES` entry, the doc/translation/registry emitters, and the generated docs + `crates/web` registry
  header are updated; `make rust` stays green with **no drift**.
- ADR-0037's `{role}_screens.yaml` naming (§2) and "no system screens" (§4) are superseded by the above;
  the ADR-0037 file carries a one-line "Refined by" note.
- **No behaviour or API change** — the front-office spec's screens, roles (`[PUBLIC, CUSTOMER]`) and
  resolver/action bindings are identical. Future audiences add one file each, picked up automatically by
  the generic validator; the human-facing "Screens" documentation section stays single-file until the
  emitter generalization follow-up lands.
- **Follow-up — marketplace content split**: extract the cross-restaurant discovery screens (`home`,
  `search`, category discovery) from `restaurant_frontoffice.yaml` into a new `captain_frontoffice.yaml`
  bound to `live.captain.food`/`captain.food`, leaving the single-restaurant journey (catalog, cart,
  checkout, tracking) in `restaurant_frontoffice.yaml`. Customer-account screens (`orders`, `account`)
  reachable from both hosts are placed during that split. Tracked separately; not part of this rename.
  **→ Done by ADR-20260722-160000 (#75)**: `captain_frontoffice.yaml` created with `home`/`search`/
  `partner_landing`; account/order screens kept in the storefront; doc emitters generalized over all
  `screens/*.yaml`.
