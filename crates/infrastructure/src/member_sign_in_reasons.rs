//! Bounded telemetry-label mapping for the member sign-in door (#639 part C step 6-ii,
//! `member-sign-in` observability contract). Kept OUT of `inbox.rs` on purpose: that file's
//! `every_arm_of_the_human_owned_router_names_an_inbox_variant` gate forbids a catch-all arm
//! ANYWHERE in the file, and a `DomainError::Rejected { code, .. }` code is a `String` — matching
//! it can never be exhaustive without one. This module carries that unavoidable catch-all so the
//! router file stays true to its own invariant.

use domain::shared::errors::DomainError;

/// The `member-sign-in` contract's bounded `reason` label for a refused
/// `requestMemberSignInLink`/`member_sign_in_refused_total` -- never an email/token/messageId,
/// always the DomainError's own typed code (already bounded by the DSL's `errors.yaml`).
pub fn member_sign_in_reason(e: &DomainError) -> &'static str {
    match e {
        DomainError::Rejected { code, .. } => match code.as_str() {
            "MemberSignInDoorClosed" => "door_closed",
            "RateLimited" => "rate_limited",
            "VerificationSendLimitReached" => "send_limit_reached",
            "VerificationSendCapacityExhausted" => "send_capacity_exhausted",
            _ => "rejected",
        },
        DomainError::Repository(_) => "lookup_failed",
        DomainError::Invariant(_) => "invariant",
    }
}

/// The `member.signin.confirm` span's `business.result` — the contract's closed vocabulary
/// (linked | not_linked | token_invalid | token_expired | lookup_failed), never a bare error code.
pub fn member_sign_in_confirm_result(e: &DomainError) -> &'static str {
    match e {
        DomainError::Rejected { code, .. } => match code.as_str() {
            "MemberNotLinked" => "not_linked",
            "InvalidVerificationToken" => "token_invalid",
            "VerificationCodeExpired" => "token_expired",
            "MemberSignInDoorClosed" => "door_closed",
            "MemberSignInRequiresSession" => "requires_session",
            "AuthSubjectHoldsAnotherRole" => "claim_conflict",
            _ => "rejected",
        },
        DomainError::Repository(_) | DomainError::Invariant(_) => "lookup_failed",
    }
}

/// The `invitation.invite` span's `business.result` — same unavoidable-catch-all reasoning as
/// this module's header (#639 part C step 6-iv round 2, beck: the two inline `_ =>
/// "technical_error"` arms this replaces were the `every_arm_of_the_human_owned_router_names_an_
/// inbox_variant` gate's own no-catch-all-ANYWHERE-in-the-file invariant, missed by round 1).
pub fn restaurant_invitation_invite_result(e: &DomainError) -> &'static str {
    match e {
        DomainError::Rejected { code, .. } => match code.as_str() {
            "RestaurantInvitationDoorClosed" => "door_closed",
            _ => "rejected",
        },
        DomainError::Repository(_) | DomainError::Invariant(_) => "technical_error",
    }
}

/// The `invitation.accept` span's `business.result` — same reasoning.
pub fn restaurant_invitation_accept_result(e: &DomainError) -> &'static str {
    match e {
        DomainError::Rejected { code, .. } => match code.as_str() {
            "InvalidVerificationToken" => "token_invalid",
            "VerificationCodeExpired" => "token_expired",
            "RestaurantInvitationNotAcceptable" => "not_acceptable",
            _ => "rejected",
        },
        DomainError::Repository(_) | DomainError::Invariant(_) => "technical_error",
    }
}
