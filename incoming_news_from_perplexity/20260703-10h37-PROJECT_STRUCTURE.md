# Captain.Food — Project Structure & Clean Architecture

**Date:** 2026-07-03  
**Status:** Adopted  
**Author:** CTPO, Captain.Food

---

## Architectural Principles

This structure enforces **Clean Architecture** (Robert C. Martin) combined with **Domain-Driven Design** bounded contexts and the **Crux** Ports & Adaptors pattern.

The dependency rule is absolute: **outer layers depend on inner layers, never the reverse.**

```
┌─────────────────────────────────────────────┐
│  Shells (Leptos, SwiftUI, Compose, Tauri)   │  ← knows Core
├─────────────────────────────────────────────┤
│  Infrastructure (Axum, SQLx, Supabase)      │  ← knows Domain
├─────────────────────────────────────────────┤
│  Application (Use Cases, CQRS handlers)     │  ← knows Domain
├─────────────────────────────────────────────┤
│  Domain (Aggregates, Events, Policies)      │  ← knows nothing
└─────────────────────────────────────────────┘
```

**Nothing in Domain or Application may import from Infrastructure or Shells.**  
Violations are caught at compile time via Rust's module visibility rules.

---

## Workspace Layout

```
captain-food/
├── Cargo.toml                         # Workspace root — lists all crates
├── Cargo.lock
├── rust-toolchain.toml                # Pinned Rust version
├── .cargo/
│   └── config.toml                    # Build targets (WASM, aarch64, x86_64)
│
├── specs/                             # Source of truth — edit these, never generated files
│   ├── captain_food_ui_spec.yaml      # UI screens, components, actions, data requirements
│   └── domain_spec.yaml              # Aggregates, commands, events, queries, policies
│
├── scripts/
│   ├── generate.rs                    # Codegen: spec → Rust artifacts + SQL migrations
│   ├── check_coverage.sh              # CI: enforces 80% test coverage minimum
│   └── validate_i18n.sh               # CI: checks all i18n keys are present in all locales
│
├── crates/
│   │
│   ├── domain/                        # ★ INNER CORE — zero dependencies on other crates
│   │   ├── Cargo.toml                 # deps: serde, uuid, chrono, thiserror only
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── restaurant/
│   │       │   ├── mod.rs
│   │       │   ├── aggregate.rs       # Restaurant aggregate root
│   │       │   ├── commands.rs        # CreateRestaurant, UpdateMenu, SetAvailability…
│   │       │   ├── events.rs          # RestaurantCreated, MenuUpdated…
│   │       │   └── policies.rs        # Domain invariants (e.g. menu item must have price)
│   │       ├── order/
│   │       │   ├── aggregate.rs       # Order aggregate root
│   │       │   ├── commands.rs        # PlaceOrder, AcceptOrder, MarkReady, CancelOrder…
│   │       │   ├── events.rs          # OrderPlaced, OrderAccepted, OrderDelivered…
│   │       │   ├── policies.rs        # CanRate only if status=DELIVERED, etc.
│   │       │   └── state_machine.rs   # Order lifecycle transitions
│   │       ├── customer/
│   │       │   ├── aggregate.rs       # Customer aggregate
│   │       │   ├── commands.rs        # RegisterCustomer, AddAddress, UpdateProfile…
│   │       │   └── events.rs          # CustomerRegistered, AddressAdded…
│   │       ├── cart/
│   │       │   ├── aggregate.rs       # Cart aggregate (ephemeral, per-session)
│   │       │   ├── commands.rs        # AddLine, RemoveLine, ApplyPromo, Clear…
│   │       │   └── events.rs          # LineAdded, PromoApplied…
│   │       ├── review/
│   │       │   ├── aggregate.rs
│   │       │   ├── commands.rs        # SubmitReview (guard: order must be DELIVERED)
│   │       │   └── events.rs
│   │       └── shared/
│   │           ├── value_objects.rs   # Money, Address, PhoneNumber, Slug, Rating…
│   │           ├── errors.rs          # Domain error types
│   │           └── identifiers.rs     # Typed IDs (RestaurantId, OrderId, CustomerId…)
│   │
│   ├── application/                   # USE CASES — orchestrates domain, declares ports
│   │   ├── Cargo.toml                 # deps: domain, async-trait, serde
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── ports/                 # Traits (interfaces) that infrastructure must implement
│   │       │   ├── restaurant_repository.rs   # trait RestaurantRepository
│   │       │   ├── order_repository.rs        # trait OrderRepository
│   │       │   ├── customer_repository.rs     # trait CustomerRepository
│   │       │   ├── cart_repository.rs         # trait CartRepository
│   │       │   ├── event_publisher.rs         # trait EventPublisher
│   │       │   ├── payment_gateway.rs         # trait PaymentGateway
│   │       │   ├── notification_service.rs    # trait NotificationService
│   │       │   └── screen_spec_store.rs       # trait ScreenSpecStore (SDUI)
│   │       ├── commands/              # Command handlers (write side — CQRS)
│   │       │   ├── place_order.rs
│   │       │   ├── accept_order.rs
│   │       │   ├── update_menu.rs
│   │       │   ├── apply_promo.rs
│   │       │   └── submit_review.rs
│   │       └── queries/               # Query handlers (read side — CQRS)
│   │           ├── get_screen_spec.rs         # Fetches + hydrates SDUI screen spec
│   │           ├── list_restaurants.rs
│   │           ├── get_restaurant.rs
│   │           ├── get_order.rs
│   │           ├── get_order_history.rs
│   │           └── search_restaurants.rs
│   │
│   ├── infrastructure/                # ADAPTERS — implements ports declared in application
│   │   ├── Cargo.toml                 # deps: application, domain, sqlx, reqwest, serde_json
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── persistence/
│   │       │   ├── postgres/
│   │       │   │   ├── restaurant_repo.rs     # impl RestaurantRepository for PgRestaurantRepo
│   │       │   │   ├── order_repo.rs          # impl OrderRepository for PgOrderRepo
│   │       │   │   ├── customer_repo.rs
│   │       │   │   ├── cart_repo.rs
│   │       │   │   └── screen_spec_store.rs   # impl ScreenSpecStore — reads from Supabase
│   │       │   └── mappers/                   # DB row → Domain aggregate conversions
│   │       │       ├── restaurant_mapper.rs
│   │       │       ├── order_mapper.rs
│   │       │       └── customer_mapper.rs
│   │       ├── payments/
│   │       │   └── stripe_gateway.rs          # impl PaymentGateway for StripeGateway
│   │       ├── notifications/
│   │       │   └── supabase_realtime.rs        # impl NotificationService via Supabase Realtime
│   │       └── events/
│   │           └── postgres_event_publisher.rs # impl EventPublisher — domain events to DB
│   │
│   ├── server/                        # AXUM HTTP SERVER — entry point for web + API
│   │   ├── Cargo.toml                 # deps: application, infrastructure, axum, tokio, tower
│   │   └── src/
│   │       ├── main.rs                # Axum router + dependency injection
│   │       ├── config.rs              # Environment-based config (DATABASE_URL, STRIPE_KEY…)
│   │       ├── middleware/
│   │       │   ├── auth.rs            # JWT extraction + Supabase session validation
│   │       │   ├── tracing.rs         # OpenTelemetry tracing
│   │       │   └── rate_limit.rs
│   │       ├── handlers/
│   │       │   ├── screens.rs         # GET /api/screens/:id — SDUI hydration endpoint
│   │       │   ├── orders.rs          # POST /api/orders, GET /api/orders/:id
│   │       │   ├── cart.rs            # POST /api/cart/lines, DELETE /api/cart/lines/:id
│   │       │   ├── restaurants.rs     # GET /api/restaurants, GET /api/restaurants/:slug
│   │       │   ├── search.rs          # GET /api/search
│   │       │   └── webhooks.rs        # POST /webhooks/stripe
│   │       ├── graphql/
│   │       │   ├── schema.rs          # async-graphql schema root
│   │       │   ├── resolvers/
│   │       │   │   ├── restaurant.rs
│   │       │   │   ├── order.rs
│   │       │   │   └── customer.rs
│   │       │   └── subscriptions.rs   # Real-time order status via GraphQL subscriptions
│   │       └── sdui/
│   │           ├── resolver_registry.rs  # Allowlist of named data resolvers
│   │           ├── hydrator.rs           # Merges resolved data into spec JSON
│   │           └── validator.rs          # Validates spec JSON against schema at runtime
│   │
│   ├── shared_types/                  # Types shared across crates AND via UniFFI to mobile
│   │   ├── Cargo.toml                 # deps: serde, uniffi
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── api_types.rs           # Request/Response DTOs (serde)
│   │       ├── sdui_types.rs          # SDUI node, component, action types (GENERATED)
│   │       └── uniffi.udl             # UniFFI interface definition for mobile bindings
│   │
│   ├── core/                          # CRUX CORE — pure business logic, no side effects
│   │   ├── Cargo.toml                 # deps: crux, domain, shared_types, serde
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── model.rs               # App global state (typed, immutable snapshots)
│   │       ├── events.rs              # All events the UI can send to the core
│   │       ├── capabilities.rs        # Declared capabilities (Http, Storage, Render…)
│   │       └── tests/                 # Pure unit tests — no device, no network needed
│   │           ├── order_tests.rs
│   │           ├── cart_tests.rs
│   │           └── navigation_tests.rs
│   │
│   ├── web/                           # LEPTOS FRONTEND — compiles to WASM
│   │   ├── Cargo.toml                 # deps: leptos, leptos_router, leptos_i18n, core
│   │   ├── index.html
│   │   └── src/
│   │       ├── main.rs
│   │       ├── registry.rs            # GENERATED — SDUI component type → Leptos component
│   │       ├── renderer.rs            # Recursive SDUI renderer (~30 lines)
│   │       ├── action_dispatcher.rs   # Handles all SDUI action types
│   │       ├── components/            # One .rs file per SDUI component type
│   │       │   ├── restaurant_card.rs
│   │       │   ├── promo_banner.rs
│   │       │   ├── category_pill.rs
│   │       │   ├── cart_fab.rs
│   │       │   ├── bottom_sheet.rs
│   │       │   └── ...                # (all types declared in ui_spec.yaml)
│   │       ├── screens/               # Non-SDUI screens (transactional)
│   │       │   ├── checkout.rs        # Stripe Elements integration
│   │       │   └── order_tracking.rs  # Real-time GraphQL subscription
│   │       └── i18n/
│   │           ├── keys.rs            # GENERATED — canonical i18n key list
│   │           ├── en.ftl             # English strings (Fluent format)
│   │           └── fr.ftl             # French strings
│   │
│   └── desktop/                       # TAURI 2.0 SHELL — restaurant manager app
│       ├── Cargo.toml                 # deps: tauri, core, server (embedded)
│       ├── tauri.conf.json
│       └── src/
│           ├── main.rs                # Tauri app entry + embedded Axum server
│           └── commands.rs            # Tauri commands (native OS features)
│
├── ios/                               # SWIFTUI SHELL — thin, calls Rust core via UniFFI
│   ├── CaptainFood.xcodeproj/
│   └── Sources/
│       ├── App.swift
│       ├── Views/                     # SwiftUI views (thin wrappers around Crux events)
│       │   ├── HomeView.swift
│       │   ├── RestaurantView.swift
│       │   └── OrderTrackingView.swift
│       └── Generated/                 # UniFFI-generated Swift bindings (never edit)
│           └── captain_food.swift
│
├── android/                           # COMPOSE SHELL — thin, calls Rust core via UniFFI
│   └── app/src/main/
│       ├── kotlin/com/captainfood/
│       │   ├── MainActivity.kt
│       │   ├── ui/                    # Compose screens (thin wrappers around Crux events)
│       │   │   ├── HomeScreen.kt
│       │   │   ├── RestaurantScreen.kt
│       │   │   └── OrderTrackingScreen.kt
│       │   └── generated/             # UniFFI-generated Kotlin bindings (never edit)
│       │       └── captain_food.kt
│       └── jniLibs/                   # Compiled Rust .so libraries per ABI
│
├── supabase/
│   ├── migrations/                    # GENERATED from domain_spec.yaml — never edit manually
│   │   ├── 20260703_001_initial_schema.sql
│   │   ├── 20260703_002_screen_specs.sql
│   │   └── 20260703_003_rls_policies.sql
│   └── seed/
│       └── demo_restaurants.sql       # Dev seed data
│
└── .github/
    └── workflows/
        ├── ci.yml                     # Lint + test + codegen diff check + i18n check
        ├── deploy_web.yml             # Build WASM + deploy to Cloudflare Pages
        └── deploy_server.yml          # Build Axum binary + deploy to Fly.io / Railway
```

---

## Dependency Graph (Rust crates)

```
         ┌─────────┐
         │ domain  │   ← no internal deps
         └────┬────┘
              │
    ┌─────────▼──────────┐
    │    application     │   ← depends on: domain
    │  (ports + use cases)│
    └──────┬──────┬──────┘
           │      │
  ┌────────▼──┐  ┌▼───────────────┐
  │  infra-   │  │  shared_types  │
  │ structure │  └────────────────┘
  └────────┬──┘         │
           │            │
  ┌────────▼────────┐   │
  │     server      │◄──┘
  │  (Axum + GraphQL│
  │   + SDUI layer) │
  └─────────────────┘
           ▲
           │ (HTTP / WASM boundary)
  ┌────────┴────────┐     ┌──────────┐
  │      web        │     │  core    │ (Crux)
  │   (Leptos/WASM) │     └────┬─────┘
  └─────────────────┘          │ UniFFI
                         ┌─────┴──────┐
                     ┌───▼───┐   ┌────▼────┐
                     │  ios  │   │ android │
                     └───────┘   └─────────┘
           ┌─────────────────────┐
           │      desktop        │ (embeds server + web)
           └─────────────────────┘
```

---

## Clean Architecture Rules for Claude Code

### ✅ Allowed dependency directions
- `server` → `application`, `infrastructure`, `shared_types`
- `application` → `domain`
- `infrastructure` → `application`, `domain`
- `web` → `shared_types`, `core`
- `core` → `domain`, `shared_types`
- `desktop` → `server`, `web`

### ❌ Forbidden dependency directions
- `domain` → anything else
- `application` → `infrastructure` (use traits/ports instead)
- `application` → `server`, `web`, `desktop`
- `domain` → `serde` (serialization is infrastructure concern — use mappers)
- Any crate → circular dependency

### Ports & Adapters rule
When a use case needs to read/write data or call an external service:
1. Declare a `trait` in `application/src/ports/`
2. Implement the `trait` in `infrastructure/src/`
3. Inject the implementation in `server/src/main.rs` via constructor injection

Never instantiate infrastructure types directly inside application or domain code.

---

## GENERATED Files — Never Edit Manually

| File | Generated by | Trigger |
|---|---|---|
| `crates/web/src/registry.rs` | `scripts/generate.rs` | `pnpm generate` / spec change |
| `crates/shared_types/src/sdui_types.rs` | `scripts/generate.rs` | spec change |
| `crates/web/src/i18n/keys.rs` | `scripts/generate.rs` | spec change |
| `supabase/migrations/*.sql` | `scripts/generate.rs` | spec change |
| `ios/Sources/Generated/captain_food.swift` | `uniffi-bindgen` | `cargo build` |
| `android/app/.../generated/captain_food.kt` | `uniffi-bindgen` | `cargo build` |

CI re-runs `generate` and diffs against the commit. Any mismatch fails the build.

---

## Non-SDUI Screens (implemented as standard Leptos pages)

| Screen | Location | Reason |
|---|---|---|
| Checkout | `crates/web/src/screens/checkout.rs` | Stripe Elements — JS interop boundary |
| Order tracking | `crates/web/src/screens/order_tracking.rs` | GraphQL subscription, real-time state machine |
| Auth (OTP/Passkey) | Bottom sheet components | Supabase Auth flow integrity |

These screens are **not** driven by the SDUI renderer. Do not attempt to move them into `renderer.rs`.
