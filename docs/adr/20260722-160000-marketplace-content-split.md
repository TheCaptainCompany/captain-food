# ADR-20260722-160000 — Marketplace content-split: extract `captain_frontoffice.yaml`

## Status

Accepted (product-owner directive, 2026-07-22). Realizes the **content-split follow-up** of
**ADR-20260722-091500** (SDUI screens by audience) and **ADR-20260722-101500** (per-surface translation
sidecars); builds on **ADR-0033** (Spec-Driven SDUI). Implements issue #75.

## Context

ADR-20260722-091500 defined two customer-facing front offices split by host, but the marketplace
(cross-restaurant discovery) screens still lived in `restaurant_frontoffice.yaml` (the per-tenant
`{slug}.captain.food` storefront). This ADR carries out the extraction and records the decisions that the
split forced — the ones ADR-091500 explicitly deferred ("Customer-account screens reachable from both
hosts are placed during that split"; "Generalizing those emitters … is a follow-up").

## Decision

1. **New surface `captain_frontoffice.yaml` (+ sidecar).** The marketplace at `live.captain.food` → bare
   `captain.food` gets its own SDUI spec holding **`home`, `search`** (discovery) and **`partner_landing`**
   (marketplace marketing). Its strings (`home.*`, `search.*`, `partner.*`) move to the co-located
   `captain_frontoffice.translations.yaml`. Roles `[PUBLIC, CUSTOMER]` (partner_landing PUBLIC).

2. **Shared customer-account/order screens stay in `restaurant_frontoffice.yaml`.** The account, order
   history/tracking, cart and checkout screens remain in the storefront surface. The split's purpose is
   extracting cross-restaurant *discovery*; the account/order journey is already modelled there, and
   whether the marketplace host later grows its **own** account surface (vs. redirecting to a central
   host) is a product-routing question deferred to when that host ships. Cross-host navigation to
   `/account`, `/orders` is a runtime routing concern (tenant middleware), **not** spec completeness — so
   these screens are **not duplicated** onto the marketplace.

3. **The SDUI component registry stays a single shared allowlist in `restaurant_frontoffice.yaml`.** The
   `component_registry` is the **renderer-level** dispatch surface — one Leptos renderer serves all
   surfaces, one `ComponentKind` enum — so it is declared once and carries the marketplace
   `discovery`/`content` components too. `captain_frontoffice.yaml` declares **no** `component_registry`
   and uses components from that shared allowlist. `crates/web/src/generated/registry.rs` is therefore
   **byte-identical** (zero drift). A future per-surface registry + merging emitter (ADR-091500 §4's
   alternative) is deferred — no ownership benefit yet, and it would reorder the generated enum.

4. **Shared chrome is duplicated per surface; its strings are cross-referenced, not moved.** The top bar /
   bottom nav / cart FAB / toast and the location-picker + auth/OTP sheets are duplicated into
   `captain_frontoffice.yaml` (each surface file is self-contained per ADR-091500). Their strings
   (`location.*`, `auth.*`) stay in `restaurant_frontoffice.translations.yaml`; the marketplace file
   `$ref`s them there. The validator allows cross-file translation refs (any `*.translations.yaml`) and
   keys stay globally unique (ADR-101500). Promoting genuinely-shared chrome strings to `translations.yaml`
   `common.*` is a possible tidy-up, deferred (it would churn many refs for no runtime change).

5. **Codegen generalized to iterate all `screens/*.yaml`.** The loader now **auto-discovers** every
   `specs/screens/*.yaml` screen spec (keyed `screens/<surface>`, sidecars keyed bare) — dropping in a
   surface file is enough. The **doc emitters** (Markdown + HTML) iterate every screens surface and render
   one block per surface under a header, so new audiences appear in `documentation.generated.*`
   automatically. The validator (§10/§11) was already generic. The `registry.rs` emitter stays pinned per
   decision 3.

## Consequences

- `translations.generated.json` is **byte-identical** (the same keys, just re-homed — 32 marketplace keys
  moved to the new sidecar; the merged flat catalog is unchanged) — `leptos_i18n` unaffected. `registry.rs`
  **unchanged**. Only the two generated docs change (they gain the marketplace surface).
- Adding a future audience (`restaurant_backoffice.yaml`, `rider.yaml`, `system.yaml`) = drop in the file
  + its sidecar; the loader, validator and doc emitters pick it up with no codegen edit.
- The `screen-ref-out-of-scope` / `screen-translation-ref-unresolved` / `translation-duplicate-key` guards
  keep the move honest (a dangling or mis-scoped ref fails `make validate`).
- **Deferred:** the marketplace's own account surface (decision 2), a per-surface component registry
  (decision 3), and promoting shared chrome strings to `common.*` (decision 4).
