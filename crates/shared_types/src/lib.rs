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

/// The ADMIN-seam refusal reason (#639 part C step 6-iii, ADR-20260906-023825): the ONE string
/// constant `server` (`RoleGuard`'s `extensions.reason`, set only when the underlying identity is
/// `Identity::Unbound { role: RequestRole::Admin, .. }` -- an ADMIN-claimed token with no live
/// platform grant) and `web` (the client bounce decision, `crates/web/src/bounce.rs`) both depend
/// on -- the [`RIDER_RESTRICTED`] precedent, transposed. `code: FORBIDDEN` is UNCHANGED -- this is
/// an ADDITIVE `reason` beside it, present only on this specific refusal, never on an ordinary
/// role-mismatch `RoleGuard` rejection (a CUSTOMER token hitting `/admin/graphql` gets no reason).
pub const ADMIN_ACCESS_NOT_GRANTED: &str = "ADMIN_ACCESS_NOT_GRANTED";

/// The graphql-transport-ws close code the restriction fact terminates a rider's socket with
/// (#639 part C step 5, ADR-20260905-065415 §3): the protocol's OWN `Forbidden` code, which
/// async-graphql never emits itself, so a client can tell "the platform closed this on purpose"
/// from an ordinary transport drop. Named beside [`RIDER_RESTRICTED`] because both are the ONE
/// signal a restricted rider's socket carries — the reason string is deliberately the SAME token,
/// never a hand-copied duplicate a future edit could drift from.
pub const RIDER_RESTRICTED_SOCKET_CLOSE_CODE: u16 = 4403;

/// The close reason: a short English token, never a French sentence and never legal wording (the
/// legal-specialist condition, ADR-20260905-065415 §3) — the statement of grounds lives on the
/// `/restricted` screen (ADR-20260904-124600 §4), not on a close frame most clients never surface
/// to a human. Reuses [`RIDER_RESTRICTED`] verbatim rather than a second literal that could drift.
pub const RIDER_RESTRICTED_SOCKET_CLOSE_REASON: &str = RIDER_RESTRICTED;

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
