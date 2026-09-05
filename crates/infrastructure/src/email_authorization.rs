//! The **send seam** for magic-link email (#639 part C step 6-ii, ADR-20260905-101349 §9) — the
//! `sms_authorization` shape, simplified: unlike SMS there is no separate inbound hook where the
//! euro is spent, so `identity.send_email_magic_link` IS both the shedding point and the wall.
//!
//! `EmailSendAuthorizer` bundles the pure [`application::email_guard::EmailSendPolicy`] with the
//! shared Postgres counter and records the `member-sign-in` contract's `member_sign_in_refused_total`
//! on every refusal — never silent, exactly like `otp_send_refused_total`.

use application::email_guard::EmailSendPolicy;
use application::sms_guard::SmsQuotaStore;
use domain::generated::scalars::EmailAddress;

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

    /// **The wall.** Claim one send against the shared budget. Emits
    /// `member_sign_in_link_requested_total` / `member_sign_in_refused_total`, so a refusal is
    /// never silent.
    pub async fn authorize(&self, email: &EmailAddress) -> Result<(), application::email_guard::EmailSendRefusal> {
        let result = self.policy.authorize(self.store.as_ref(), email, Self::now_unix()).await;
        telemetry::meters::member_sign_in::link_requested(if result.is_ok() { "accepted" } else { "refused" });
        if let Err(refusal) = &result {
            telemetry::meters::member_sign_in::refused(refusal.reason());
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
        a.authorize(&EmailAddress("owner@pizzaroma.fr".into())).await.expect("first send authorised");
    }

    #[tokio::test]
    async fn the_per_address_cap_refuses_the_next_send() {
        // `from_config`, not the struct-update literal: `hmac_key` (round 2 R2-V1) is a private
        // field, deliberately not settable from outside `application::email_guard`.
        let policy = EmailSendPolicy::from_config(Some(1), None, None, None);
        let a = EmailSendAuthorizer::new(policy, Box::new(InMemorySmsQuotaStore::default()));
        let email = EmailAddress("owner@pizzaroma.fr".into());
        a.authorize(&email).await.expect("first send authorised");
        a.authorize(&email).await.expect_err("second send refused");
    }
}
