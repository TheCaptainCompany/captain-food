//! Captain.Food shared types (ADR-0035).
//!
//! serde DTOs shared across crates and — once UniFFI is wired — exported to the mobile shells. The SDUI
//! node/component/action types (`sdui_types`) will be GENERATED here from `restaurant_frontoffice.yaml`.

use serde::{Deserialize, Serialize};

/// The standing-guard refusal reason (#639 part C step 4-ii, ADR-20260904-124600 §1): the ONE
/// string constant `server` (`StandingGuard`'s `extensions.reason`) and `web` (the client bounce
/// decision, `crates/web/src/bounce.rs`) both depend on, so a hand-copied string can never drift
/// between the guard that emits it and the client that keys navigation on it — the exact defect a
/// re-typed literal in either crate would be invisible to `make validate`. `shared_types` is the
/// one crate both `server` and `web` name directly in their `Cargo.toml` (`app-core`/`domain` are
/// not a direct `web` dependency). `code: FORBIDDEN` is UNCHANGED (ADR-20260904-081527 §4) — this
/// is an ADDITIVE `reason` beside it, present only on a standing refusal, never on a bare `RoleGuard`
/// rejection.
pub const RIDER_RESTRICTED: &str = "RIDER_RESTRICTED";

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
