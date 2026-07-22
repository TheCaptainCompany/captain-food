//! Captain.Food web frontend (Leptos → WASM) — renderer skeleton (ADR-0033/0035).
//!
//! Holds the GENERATED SDUI component `registry` (from `restaurant_frontoffice.yaml#/component_registry`,
//! codegen roadmap item 6) and the `renderer` that dispatches on it. Split 1/4 of #21 wires the
//! registry + a single static screen rendered server-side; the resolver/action wiring (#17) and the
//! Leptos SSR/hydration runtime land in later splits. Depends on `shared_types` + `app_core`.

use app_core::health;
use shared_types::HealthDto;

pub mod generated;
pub mod renderer;

/// Placeholder boot hook — proves the frontend can drive the shared core. Becomes the Leptos mount.
pub fn boot() -> HealthDto {
    health()
}
