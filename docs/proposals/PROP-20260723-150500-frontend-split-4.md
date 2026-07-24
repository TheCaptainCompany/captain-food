# PROP-20260723-150500 — Frontend split 4/4: generated screen trees, real markup, routing, new surfaces, full asset pipeline
- **Status**: Approved (2026-07-23, incl. two AskUserQuestion scope decisions recorded in the body)
- **Date**: 2026-07-23
- **Tracking issue**: [#87 "Frontend split 4/4 - per-component markup, customer polish + restaurant/rider screen adoption"](https://github.com/TheCaptainCompany/captain-food/issues/87)
- **Realized by**: [PR #89](https://github.com/TheCaptainCompany/captain-food/pull/89) + ADR-20260723-172013 (post-approval divergences: host convention corrected to ADR-0036 reserved subdomains restos./riders.; cta_section registry addition surfaced by the fail-closed emitter)

> BACKFILLED as the inaugural docs/proposals entry (ADR-20260724-135945): the one plan-mode proposal
> whose session artifact survived. Earlier proposals are unrecoverable; this file marks the cutoff.


## Context

#87 "Frontend split 4/4 - per-component markup, customer polish + restaurant/rider screen adoption"
is the last piece of #21. Splits 1–3 delivered the registry, the data layer and the non-SDUI money
path; the renderer still renders one static screen (`crates/web/src/renderer.rs` `HOME_NODES`), no
routing exists, the restaurant/rider surfaces don't exist, and nothing serves the app.

**User-approved scope (AskUserQuestion):** (a) author `restaurant_backoffice.yaml` + `rider.yaml`
(+ sidecars) binding ONLY existing api.yaml ops, reusing the existing component vocabulary (new
kinds only where nothing fits); (b) **full pipeline now** — wasm bundle built in CI/Docker, SSR pages
served by the BFF.

## Approach

### 1. Codegen emitter `emit_web_screens` (tools/codegen-rs/src/main.rs)

`screens/*.yaml` → `crates/web/src/generated/screens.rs` (registered next to `emit_web_registry` /
`emit_web_data_layer`, byte-stable, covered by codegen unit tests):

- Per surface (module per file: `restaurant_frontoffice`, `captain_frontoffice`,
  `restaurant_backoffice`, `rider`): `pub const SCREENS: &[Screen]`.
- `Screen { id, route, roles: &[&str], requires_auth: bool, sdui: bool, data_requirements:
  &[ResolverKey], tree: &[Node] }` — `data_requirements` resolved through
  `ResolverKey::from_key` at emit time (unknown key = emitter abort; the validator already
  guarantees resolution).
- `Node { kind: ComponentKind, props: &[(&str, PropValue)], children: &[Node] }` where
  `PropValue::Text(&str) | I18n(&str) | Binding(&str) | List(&[Node])`-ish — flatten YAML props:
  translation `$ref`s → `I18n(key)`, `{{ path }}` strings → `Binding(path)`, literals → `Text`;
  named child-carrying fields (`content`, `components`, `slots.*`, `fields`, `items`, `tabs`) →
  children. `{ component: topbar }` refs expand from `global_components` at emit time.
- Non-SDUI screens (`sdui: false`) emit `tree: &[]` — checkout/tracking render from their
  hand-written modules; the generated entry still carries route/roles for the router.
- Unknown component `type` (not in the registry) = emitter abort — same fail-closed stance as the
  data layer.

### 2. Generic renderer (crates/web/src/renderer.rs — rewrite)

- `render_node(node, data, i18n) -> AnyView`: dispatch on `ComponentKind` GROUP (`registry.rs`
  `group()`), with real element shapes per group + per-kind refinements (cards, rails, lists,
  sections, buttons, chrome), keeping the `data-c` tagging. Item-list kinds (`restaurant_card_grid`,
  `order_list`, `catalog_sections`, …) iterate the bound data array and render one child per row.
- Binding resolution: `Binding("cart.total")` → JSON pointer walk into the screen's resolved
  resolver data (`serde_json::Value` map keyed by resolver key), formatted scalars.
- i18n: embed `specs/generated/translations.generated.json` via `include_str!` + a small
  `i18n::resolve(key, locale)` (default `fr`, fallback `en`) so rendered pages carry REAL strings,
  not just `data-i18n` markers (check-drift keeps the JSON in sync).
- `render_screen_html(surface, screen, data, locale)` replaces `render_home_html` (which becomes a
  thin wrapper or is deleted with its tests updated); reuse `page_html`.

### 3. Routing + hydrate entry (crates/web/src/router.rs — new)

- `Surface` enum (4 variants) with `screens()` from the generated tables; `match_route(surface,
  path) -> Option<&Screen>` — exact + `:param` segments (`/orders/:orderId/confirmation`), params
  exposed for resolver args (`order.byId` ← `:orderId`).
- `surface_for_host(host) -> Surface`: `live.captain.food`/`captain.food`/`www` → marketplace;
  `back.`/`rider.` prefixes → backoffice/rider (subdomain convention recorded in the ADR); anything
  else with a subdomain → storefront (slug = first label). Unit-tested.
- `hydrate()` (renderer.rs wasm entry) becomes route-aware: read `location.host`+`pathname`, match
  the screen, fetch its `data_requirements` via `execute_resolver` (HttpTransport, role from
  surface), re-render + attach; checkout/tracking hydrate their hand-written flows.

### 4. New spec surfaces (specs/screens/ — the approved DSL work)

- **`restaurant_backoffice.yaml`** (roles RESTAURANT + RESTAURANT_ACCOUNT, app_types [web]):
  - `orders_queue` `/` — `orders(restaurantId, status)` tab-bar queue (PLACED/ACCEPTED/PREPARING/READY);
    actions accept/reject/start-prep/mark-ready/cancel (`acceptOrder`, `rejectOrder`,
    `startPreparation`, `markOrderReady`, `cancelOrderByRestaurant`) + `changeOrderAcceptanceMode`
    pause toggle.
  - `deliveries_board` `/deliveries` — `restaurantDeliveries(restaurantId, status)` +
    `escalateDelivery`, `markOrderDelivered`.
  - `refunds_queue` `/refunds` — `pendingRefunds(restaurantId, status)` + `approveRefund`/`denyRefund`.
  - `satisfaction` `/satisfaction` — `restaurantDeliverySatisfaction(restaurantId)` (#62 read).
- **`rider.yaml`** (roles RIDER, app_types [web, ios, android]):
  - `jobs` `/` — `myDeliveries(status)` job cards; `acceptDelivery`.
  - `job_detail` `/jobs/:orderId` — `delivery(orderId)`; `confirmPickup`, `completeDelivery`,
    `markOrderDelivered`; restaurant/customer contact rows.
  - Online/offline toggle = explicit `gap` (no rider-status mutation in api.yaml).
- Resolver/action keys: NEW dotted keys (e.g. `orders.byRestaurant`, `deliveries.byRestaurant`,
  `refunds.pending`, `deliveries.mine`, `delivery.byOrder`) — extend the shared union allowlist;
  none collide with existing keys (a collision with different binding aborts the emitter).
- Sidecars `restaurant_backoffice.translations.yaml` + `rider.translations.yaml` (en+fr, ADR-20260722-101500).
- Component vocabulary: reuse (`tab_bar`, `order_list`, `order_card`, `status_chip`, `section`,
  `button`, `info_row`, `page_header`, `sticky_header`, `toast_notification`, …). Only add a
  registry kind if something truly has no fit (expect ≤2, e.g. `swipe_action_row` NOT needed for V0
  — plain buttons); every addition regenerates `registry.rs`.

### 5. Full asset pipeline (approved: "Full pipeline now")

- `crates/web/Cargo.toml`: `[lib] crate-type = ["cdylib", "rlib"]`; pin `wasm-bindgen = "=0.2.x"`
  (exact version matching the lockfile so wasm-bindgen-cli can be pinned identically).
- **Dockerfile**: builder stage gains `rustup target add wasm32-unknown-unknown` + a pinned
  `cargo install wasm-bindgen-cli --version <same>`; `cargo chef cook` twice (native + an added
  `--target wasm32-unknown-unknown --no-default-features --features hydrate` cook for `web`);
  build `web` for wasm32 --release, run `wasm-bindgen --target web --out-dir /app/dist`; runtime
  stage `COPY --from=builder /app/dist /app/web-assets`.
- **Server** (`crates/server`): new `web` dep (default `ssr` feature) + `tower-http` (`fs`).
  New routes module `crates/server/src/web_app.rs`: `/assets/*` → `ServeDir` on `WEB_ASSETS_DIR`
  (default `/app/web-assets`; absent dir ⇒ 404s, dev-friendly), and a **fallback** GET handler:
  `surface_for_host(Host header)` + `match_route` → `web::render_screen_html(...)` (empty data —
  hydrate fetches; recorded honest residual: server-side data resolution) + the module `<script>`
  loading `/assets/web.js` → `init(); hydrate();`. Unknown route → 404. Existing routes (GraphQL,
  adapters, health, internal) take precedence — fallback only.
- **CI** (`.github/workflows/ci.yml`): add `rustup target add wasm32-unknown-unknown` + the
  `cargo build -p web --target wasm32-unknown-unknown --no-default-features --features hydrate`
  check (bundle emission itself stays in the Docker build). Add a `make wasm` alias in the Makefile
  (ASCII-only recipe lines!).

### 6. Completeness & docs

- ADR `docs/adr/ADR-<datetime>-sdui-screen-trees-and-web-serving.md`: generated screen trees (DSL →
  Rust, no runtime JSON contract yet — defers ADR-0033's server-driven JSON delivery explicitly),
  host→surface convention (`back.`/`rider.` subdomains), the SSR-shell-first serving model +
  hydrate-fetch, the asset pipeline (wasm-bindgen-cli, pinned), and the rider-status gap.
- `docs/STATUS.md` top entry; `docs/frontend/renderer-architecture.md` §6c.
- No new api.yaml ops ⇒ no stories/tests/rules changes (ADR-0032 holds). The screens validator +
  §1b REF_CONTRACT already cover the new files generically (`screens/*.yaml` globs).

## Files

| Area | Files |
|---|---|
| Codegen | `tools/codegen-rs/src/main.rs` (emit_web_screens + tests; SPEC_FILES/loader already glob) |
| Specs (approved) | `specs/screens/restaurant_backoffice.yaml`, `rider.yaml`, 2 sidecars; possible ≤2 registry kinds in `restaurant_frontoffice.yaml` |
| Generated | `crates/web/src/generated/screens.rs` (new), `registry.rs`/`data_layer.rs` (regen), `specs/generated/*` docs (regen) |
| Web | `renderer.rs` (rewrite), `router.rs` (new), `lib.rs`, `Cargo.toml` (cdylib + pin) |
| Server | `Cargo.toml` (+web, +tower-http), `src/web_app.rs` (new), `src/lib.rs` (mount) |
| Pipeline | `Dockerfile`, `.github/workflows/ci.yml`, `Makefile` (`make wasm`) |
| Docs | ADR, `docs/STATUS.md`, `docs/frontend/renderer-architecture.md` |

## Verification

1. Codegen unit tests: emitter fixture (mini screens yaml → tree snapshot; unknown component type
   aborts; `{ component: … }` expansion; `sdui:false` empty tree).
2. `crates/web` tests: route matching (params, host→surface), binding resolution, i18n fallback,
   every generated screen renders SSR without panic (`for surface, for screen: render_screen_html`),
   backoffice/rider trees contain their action buttons.
3. Server test: fallback handler serves a storefront route with `text/html` + hydrate script; asset
   route 404s cleanly without the dir.
4. `make rust` — 0 errors, no drift (screens.rs committed); wasm32 hydrate build; workspace build.
5. Docker build is CI-verified on merge (build-image.yml); locally at minimum `docker build` if
   feasible, else rely on the gated GHA build before the Render deploy hook fires.

## Workflow

Code + spec work: claim #87 (label + comment + branch `87-frontend-split-4-screens-adoption`), draft
PR `Closes #87` first; DSL changes are pre-approved by this plan. Completion: `make rust` green →
ready + auto-merge as one step → supervise to MERGED.
