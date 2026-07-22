//! Captain.Food shared types (ADR-0035).
//!
//! serde DTOs shared across crates and — once UniFFI is wired — exported to the mobile shells. The SDUI
//! node/component/action types (`sdui_types`) will be GENERATED here from `restaurant_frontoffice.yaml`.

use serde::{Deserialize, Serialize};

/// Minimal health/readiness DTO — a placeholder proving the crate compiles and is consumable downstream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthDto {
    pub status: String,
}

impl HealthDto {
    pub fn ok() -> Self {
        Self { status: "ok".to_string() }
    }
}
