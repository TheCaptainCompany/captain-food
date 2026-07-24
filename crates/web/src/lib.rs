//! Captain.Food web frontend (Leptos → WASM) — renderer skeleton + data layer (ADR-0033/0035).
//!
//! Holds the GENERATED SDUI allowlists (`generated/registry.rs` components,
//! `generated/data_layer.rs` resolvers/actions — codegen roadmap item 6) and the hand-written
//! layers over them. Split 1/4 of #21 wired the registry + a single static screen; split 2/4 adds
//! the DATA LAYER: `session` (the persistent anonymous identity, #12), `graphql` (the transport
//! seam + `execute_resolver`, the only read entry point) and `actions` (the acceptance-first
//! two-step `dispatch`, #17). The Leptos SSR/hydration runtime that consumes them (live screens,
//! checkout, subscriptions) lands in later splits. Depends on `shared_types` + `app_core`.
//!
//! Split 3/4 adds the NON-SDUI money path: `subscriptions` (the graphql-transport-ws client —
//! sans-IO state machine + browser driver), `checkout` (the acceptance-first place-order flow +
//! screen), `stripe` (the payment-element interop seam) and `tracking` (the pull-then-push order
//! fold + confirmation screen).

use app_core::health;
use shared_types::HealthDto;

pub mod actions;
pub mod checkout;
pub mod executor;
pub mod generated;
pub mod graphql;
pub mod i18n;
#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
pub mod interact;
pub mod pending;
pub mod renderer;
pub mod router;
pub mod session;
pub mod stripe;
pub mod subscriptions;
pub mod tracking;

/// Placeholder boot hook — proves the frontend can drive the shared core. Becomes the Leptos mount.
pub fn boot() -> HealthDto {
    health()
}
