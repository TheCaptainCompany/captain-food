# ADR-20260723-172013 — Generated SDUI screen trees + BFF web serving & the wasm asset pipeline

- **Status**: Accepted
- **Date**: 2026-07-23
- **Refines**: ADR-0033 (Spec-Driven SDUI), ADR-0036 (host topology), ADR-20260722-091500 (screens
  taxonomy), ADR-20260721-175411 (CI-built image)
- **Realizes**: #87 "Frontend split 4/4 - per-component markup, customer polish + restaurant/rider
  screen adoption" (the last split of #21)

## Context

Splits 1–3 of #21 delivered the component registry, the resolver/action data layer and the non-SDUI
money path — but the renderer still rendered one static screen, no routing existed, the restaurant
and rider surfaces were unwritten, and no deployment served any of it. ADR-0033's runtime
(server-driven JSON screen delivery via Supabase `screen_specs`) is still a deferred contract; #87
needed the screens to *render* without waiting for it.

## Decisions

### 1. Screen trees are GENERATED Rust data, not runtime JSON (defers ADR-0033's delivery layer)

A new codegen emitter (`emit_web_screens`) compiles every `specs/screens/*.yaml` surface into
`crates/web/src/generated/screens.rs`: per surface, a static `Screen` table (id, route, roles,
`requires_auth`, `sdui`, `data_requirements` bound to the generated `ResolverKey`, and the component
tree as `Node { kind: ComponentKind, props, children }` data). The DSL stays the source of truth;
`make validate`/check-drift gate the derivation. ADR-0033's runtime screen delivery (Supabase-hosted
JSON, runtime-editable) remains open — when it lands, the renderer's input type is already this tree
shape, so the change is a loader, not a rewrite.

Emitter semantics worth recording:
- **Fail-closed on unregistered components** — a `type` outside the shared `component_registry`
  aborts the emitter. This immediately caught two live spec drifts: `captain_frontoffice.yaml` used
  an unregistered `cta_section` (now registered in the `content` group — the one vocabulary addition
  of this split) and `filter_bar.filters[].type: dropdown` (config vocabulary, see next point).
- **Child components live only under `components`/`content`/`fields`/`slots.*`** (a whitelist).
  Other keys reuse `type` as plain config (`filters[].type: dropdown`, `status_config.*`) and
  flatten into dotted-path props (`filters.0.type`) instead of being forced through the registry.
- **Props flatten**: translation `$ref`s → `I18n(key)`, whole-string `{{ … }}` templates →
  `Binding(path)`, other scalars → `Text`; nested config keeps its dotted path
  (`empty_state.title`). Global chrome (`{ component: topbar }`) expands at emit time.
- **`sdui: false` screens emit an empty tree** but still register route/roles — the router serves
  them through their hand-written pages (checkout.rs / tracking.rs).

### 2. The two staff surfaces bind only existing ops and (almost) only existing components

`restaurant_backoffice.yaml` (orders queue / deliveries board / refunds queue / #62 satisfaction;
roles RESTAURANT + RESTAURANT_ACCOUNT) and `rider.yaml` (job list / job detail; role RIDER) add
**zero API surface** — every resolver/action `$ref`s an existing api.yaml op — and reuse the shared
renderer registry (single addition: `cta_section`, which was already used unregistered). The rider
online/offline toggle is an explicit `gap` (`rider_toggle_online`): api.yaml exposes no rider-status
mutation (`ChangeRiderStatus` is domain-only). Per-surface translation sidecars follow
ADR-20260722-101500.

### 3. Host → surface mapping follows ADR-0036's reserved subdomains

`live.`/bare/`www.` → the marketplace; **`restos.captain.food`** → the back office;
**`riders.captain.food`** → the rider app; any other `{slug}.captain.food` → that restaurant's
storefront; localhost/unknown → the marketplace (anonymous-safe default). `web::router` MIRRORS the
server's `hosts::classify_host` (web cannot depend on server — the same mirror rule as
`Role::segment`). Staff surfaces talk to their own role path (`/restaurant`, `/rider` — fail-closed
401 without a matching JWT); customer surfaces start on `/public`.

### 4. Serving model: SSR shell + hydrate-fetch

The server's host fallback (`hosts::host_root`) now SSRs the matched screen via
`web::router::render_path` — with **empty data**: SSR ships the shell (real markup, real i18n — fr
default/en fallback from the embedded generated catalog), and the wasm bundle re-renders with live
data after fetching the screen's `data_requirements` (route `:params` feed resolver args by name;
the one bridge is `:orderId` → `order.byId`'s `id`). Honest residuals: server-side data resolution
(SSR pages with data pre-filled) and screen-level auth redirects are follow-ups; a deployment
without the asset dir serves SSR-only pages (the boot script 404s — degraded, never broken).

### 5. Asset pipeline: wasm-bindgen-cli in the Docker build, compile-check in CI

The image build (GHA, ADR-20260721-175411) gains the wasm toolchain: `wasm32-unknown-unknown` +
`wasm-bindgen-cli` **pinned to the exact `wasm-bindgen` crate version** (`=0.2.126` on both sides —
the CLI refuses a mismatch; bump together). A second `cargo chef cook` caches the wasm dependency
tree; `wasm-bindgen --target web` emits `/app/web-assets` (`web.js` + `web_bg.wasm`), served by the
server under `/assets` (`WEB_ASSETS_DIR`). `ci.yml` adds only the cheap hydrate compile-check
(`make wasm`) inside the existing required `codegen` job — a separate workflow would not be a
required check and a red wasm build could merge. `crates/web` becomes `crate-type = ["cdylib",
"rlib"]`.

## Consequences

- All four audiences render from the spec: 16 screens, one renderer, one registry — the SDUI
  promise ("the API answers the UI, the registry answers the renderer") now holds end to end.
- A spec screen edit is a codegen regeneration away from shipping; an unregistered component or a
  dangling resolver key cannot reach the renderer.
- Markup depth is tiered and recorded: navigation chrome, lists/cards, sections, text, buttons and
  inputs have dedicated shapes; the remaining registered kinds render tagged generic containers
  with their resolved text — visibly present and restyleable without re-architecture.
- Docker build time grows (wasm cook + bundle); accepted — builds run on free GHA with layer
  caching, and Render still only pulls.
- Follow-ups: runtime screen delivery (ADR-0033's deferred contract), server-side data resolution
  for SSR, screen-level auth redirects, action dispatch from generic buttons (the generated
  `action.*` props → `ActionKey` wiring), sheets/overlays, and a rider-status mutation to close the
  `rider_toggle_online` gap.
