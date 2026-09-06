//! Bounded telemetry-label mapping for the ADMIN sign-in door (#639 part C step 6-iii,
//! `admin-sign-in-door` observability contract) -- the `member_sign_in_reasons.rs` precedent,
//! transposed. Kept OUT of `inbox.rs` on purpose: that file's
//! `every_arm_of_the_human_owned_router_names_an_inbox_variant` gate forbids a catch-all arm
//! ANYWHERE in the file, and a `DomainError::Rejected { code, .. }` code is a `String` --
//! matching it can never be exhaustive without one. This module carries that unavoidable
//! catch-all so the router file stays true to its own invariant.

use domain::shared::errors::DomainError;

/// The `admin-sign-in-door` contract's bounded `reason` label for a refused
/// `requestAdminSignInLink`/`admin_sign_in_refused_total` -- never an email/token/messageId,
/// always the DomainError's own typed code (already bounded by the DSL's `errors.yaml`).
pub fn admin_sign_in_reason(e: &DomainError) -> &'static str {
    match e {
        DomainError::Rejected { code, .. } => match code.as_str() {
            "AdminSignInDoorClosed" => "door_closed",
            "RateLimited" => "rate_limited",
            "VerificationSendLimitReached" => "send_limit_reached",
            "VerificationSendCapacityExhausted" => "send_capacity_exhausted",
            _ => "rejected",
        },
        DomainError::Repository(_) => "lookup_failed",
        DomainError::Invariant(_) => "invariant",
    }
}

/// The `admin.signin.confirm` span's `business.result` -- the contract's closed vocabulary
/// (linked | not_granted | token_invalid | token_expired | lookup_failed | door_closed |
/// requires_session | claim_conflict | rejected), never a bare error code.
pub fn admin_sign_in_confirm_result(e: &DomainError) -> &'static str {
    match e {
        DomainError::Rejected { code, .. } => match code.as_str() {
            "AdminAccessNotGranted" => "not_granted",
            "InvalidVerificationToken" => "token_invalid",
            "VerificationCodeExpired" => "token_expired",
            "AdminSignInDoorClosed" => "door_closed",
            "AdminSignInRequiresSession" => "requires_session",
            "AuthSubjectHoldsAnotherRole" => "claim_conflict",
            _ => "rejected",
        },
        DomainError::Repository(_) | DomainError::Invariant(_) => "lookup_failed",
    }
}
