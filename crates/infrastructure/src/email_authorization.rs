//! The **send seam** for magic-link email (#639 part C step 6-ii, ADR-20260905-101349 §9) — the
//! `sms_authorization` shape, simplified: unlike SMS there is no separate inbound hook where the
//! euro is spent, so `identity.send_email_magic_link` IS both the shedding point and the wall.
//!
//! `EmailSendAuthorizer` bundles the pure [`application::email_guard::EmailSendPolicy`] with the
//! shared Postgres counter and records the calling door's OWN contract's `*_link_requested_total`/
//! `*_refused_total` on every outcome — never silent, exactly like `otp_send_refused_total`.
//!
//! **Round 2 R2-3** (obs B1 + reviewer B1): before this round the wall HARDCODED the MEMBER
//! counters regardless of caller, so an admin's magic-link send silently landed on
//! `member_sign_in_link_requested_total`/`member_sign_in_refused_total` — a contract's own
//! population leaking into another's (ADR-20260905-223957 §5/§6). The fix is compiler-first
//! (never a string parameter): [`SignInDoor`], a closed two-value enum, is CHOSEN AT THE CALL SITE
//! -- `send_email_magic_link` (the member/customer path, unchanged) hardcodes `SignInDoor::Member`;
//! the NEW `send_admin_sign_in_link` (its own call site, `identity.send_admin_sign_in_link`,
//! ADR-20260818-101500's "one send per door, hardcoded" precedent) hardcodes `SignInDoor::Admin`.

use application::email_guard::EmailSendPolicy;
use application::sms_guard::SmsQuotaStore;
use domain::generated::scalars::EmailAddress;

/// Which sign-in door is asking the shared wall for a send — chosen at the call site, never
/// threaded as a runtime string (round 2 R2-3, compiler-first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignInDoor {
    Member,
    Admin,
}

/// Policy + shared counter, the object the ACL boundary holds.
pub struct EmailSendAuthorizer {
    policy: EmailSendPolicy,
    store: Box<dyn SmsQuotaStore>,
}

impl EmailSendAuthorizer {
    pub fn new(policy: EmailSendPolicy, store: Box<dyn SmsQuotaStore>) -> Self {
        Self { policy, store }
    }

    fn now_unix() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    /// **The wall.** Claim one send against the shared budget, attributed to the CALLING door's
    /// own contract.
    ///
    /// The `Member` arm is the pre-existing, unchanged shape: it names BOTH
    /// `member_sign_in_link_requested_total` (accepted|refused) and `member_sign_in_refused_total`
    /// (round 2 R2-3: "the member path unchanged" -- no behaviour here moves).
    ///
    /// The `Admin` arm names only `admin_sign_in_refused_total` here. It deliberately does NOT
    /// also call `admin_sign_in::link_requested`: the `admin_sign_in` dispatch arm in
    /// `crates/infrastructure/src/inbox.rs` already emits `admin_sign_in_link_requested_total`
    /// from the FULL command outcome (door-closed refusals that never reach this wall included,
    /// and the rare case where the wall accepts but the provider POST after it still fails) --
    /// emitting it a second time here would double-count the identical accepted/refused fact on
    /// the common path. `refused()` mirrors the Member arm's existing (pre-round-2, unflagged)
    /// double-emission shape for parity: both the wall's own quota refusal AND the inbox arm's
    /// catch-all record a refusal, exactly as already happens for the member door today.
    pub async fn authorize(
        &self,
        door: SignInDoor,
        email: &EmailAddress,
    ) -> Result<(), application::email_guard::EmailSendRefusal> {
        let result = self.policy.authorize(self.store.as_ref(), email, Self::now_unix()).await;
        match door {
            SignInDoor::Member => {
                telemetry::meters::member_sign_in::link_requested(if result.is_ok() { "accepted" } else { "refused" });
                if let Err(refusal) = &result {
                    telemetry::meters::member_sign_in::refused(refusal.reason());
                }
            }
            SignInDoor::Admin => {
                if let Err(refusal) = &result {
                    telemetry::meters::admin_sign_in::refused(refusal.reason());
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::sms_guard::InMemorySmsQuotaStore;

    #[tokio::test]
    async fn a_served_address_is_authorised() {
        let a = EmailSendAuthorizer::new(EmailSendPolicy::default(), Box::new(InMemorySmsQuotaStore::default()));
        a.authorize(SignInDoor::Member, &EmailAddress("owner@pizzaroma.fr".into())).await.expect("first send authorised");
    }

    #[tokio::test]
    async fn the_per_address_cap_refuses_the_next_send() {
        // `from_config`, not the struct-update literal: `hmac_key` (round 2 R2-V1) is a private
        // field, deliberately not settable from outside `application::email_guard`.
        let policy = EmailSendPolicy::from_config(Some(1), None, None, None);
        let a = EmailSendAuthorizer::new(policy, Box::new(InMemorySmsQuotaStore::default()));
        let email = EmailAddress("owner@pizzaroma.fr".into());
        a.authorize(SignInDoor::Member, &email).await.expect("first send authorised");
        a.authorize(SignInDoor::Member, &email).await.expect_err("second send refused");
    }
}
