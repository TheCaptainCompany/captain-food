//! Captain.Food Crux core (ADR-0035).
//!
//! The pure app-shell shared across web/desktop/mobile: typed model, UI events, declared capabilities
//! (Http, Storage, Render…). It orchestrates the `domain` model and speaks `shared_types` DTOs, with no
//! side effects of its own. Depends on `domain` + `shared_types`; referencing both proves those edges.

use domain::generated::scalars::{CurrencyCode, MoneyCents};
use domain::shared::value_objects::Money;
use shared_types::HealthDto;

/// Placeholder capability check the shells can call — proves `core → domain, shared_types`.
pub fn health() -> HealthDto {
    HealthDto::ok()
}

/// Placeholder using a domain value object — the real model will hold typed aggregate snapshots.
pub fn zero_eur() -> Money {
    Money { amount_cents: MoneyCents(0), currency: CurrencyCode("EUR".to_string()) }
}
