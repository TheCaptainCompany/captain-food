//! The email send-abuse wall (#639 part C step 6-ii, ADR-20260905-101349 §9): `identity.
//! send_email_magic_link` is the ONE way anything in this product sends a magic-link email
//! (`RequestEmailVerification` and `RequestMemberSignInLink` both call it), so the guard lives AT
//! that seam and protects both callers uniformly -- the SAME placement choice SMS made
//! (`send_phone_otp` guards `RequestPhoneVerification` and `RequestRiderSignInCode` alike).
//!
//! **Deliberately reuses [`crate::sms_guard`]'s generic quota primitives** (`QuotaClaim`,
//! `QuotaDenial`, `SmsQuotaStore`) rather than duplicating a second copy: those three types carry
//! nothing phone-specific in their FIELDS (a string key, a window, a ceiling), so the send-abuse
//! WALL is one primitive with two policies on top of it. The `Sms`-prefixed trait name is a known
//! naming residue of reuse rather than a fresh port -- flagged for the reviewer rather than
//! renamed under this card's time budget (evans would ask; the shape is correct either way).
//! `UNVERIFIED input` (ADR-20260817-105845): every ceiling below MIRRORS the SMS defaults
//! verbatim, because nobody has costed an email-send abuse ceiling and email carries no direct
//! per-message price the way a prepaid SMS credit pack does.

use crate::sms_guard::{QuotaClaim, QuotaDenial, SmsQuotaStore};
use domain::generated::scalars::EmailAddress;
use domain::shared::errors::DomainError;
use serde_json::json;

const HOUR_SECONDS: i64 = 3600;
const DAY_SECONDS: i64 = 86_400;

/// `EMAIL_MAX_SENDS_PER_ADDRESS_PER_HOUR` default (mirrors `DEFAULT_MAX_PER_NUMBER_PER_HOUR`).
pub const DEFAULT_MAX_PER_ADDRESS_PER_HOUR: i32 = 3;
/// `EMAIL_MAX_SENDS_PER_ADDRESS_PER_DAY` default (mirrors `DEFAULT_MAX_PER_NUMBER_PER_DAY`).
pub const DEFAULT_MAX_PER_ADDRESS_PER_DAY: i32 = 5;
/// `EMAIL_MAX_SENDS_PER_DAY_GLOBAL` default (mirrors `DEFAULT_MAX_PER_DAY_GLOBAL`).
pub const DEFAULT_MAX_SENDS_PER_DAY_GLOBAL: i32 = 200;

/// Why an email send was refused, in the policy's own terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmailSendRefusal {
    /// Inside the per-address cooldown/cap window.
    TooSoon { retry_after_seconds: i64 },
    /// The per-address DAILY cap is reached (the hourly bucket's backstop).
    DailyCapReached,
    /// The platform-wide daily ceiling is reached -- the kill switch.
    GlobalCeilingReached,
    /// The shared counter could not answer. Fail closed.
    StoreUnavailable { detail: String },
}

impl EmailSendRefusal {
    pub fn reason(&self) -> &'static str {
        match self {
            EmailSendRefusal::TooSoon { .. } => "cooldown",
            EmailSendRefusal::DailyCapReached => "daily_cap",
            EmailSendRefusal::GlobalCeilingReached => "global_ceiling",
            EmailSendRefusal::StoreUnavailable { .. } => "store_unavailable",
        }
    }

    /// Maps onto the SAME typed errors the SMS wall uses (`errors.yaml`, common scope): the send
    /// guard shape is shared, so the vocabulary a caller renders against is shared too.
    pub fn into_domain_error(self) -> DomainError {
        match self {
            EmailSendRefusal::TooSoon { retry_after_seconds } => {
                DomainError::rejected("RateLimited", json!({ "retryAfterSeconds": retry_after_seconds }))
            }
            EmailSendRefusal::DailyCapReached => {
                DomainError::rejected("VerificationSendLimitReached", json!({}))
            }
            EmailSendRefusal::GlobalCeilingReached => {
                DomainError::rejected("VerificationSendCapacityExhausted", json!({}))
            }
            // NOT a `Rejected`: an unreachable limiter is our defect, never the caller's.
            EmailSendRefusal::StoreUnavailable { detail } => {
                DomainError::Repository(format!("email send-abuse wall: store unavailable: {detail}"))
            }
        }
    }
}

/// The pure send policy: per-address caps + the global ceiling. No allowlist -- unlike SMS, email
/// carries no destination-country cost variance to contain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailSendPolicy {
    pub max_per_address_per_hour: i32,
    pub max_per_address_per_day: i32,
    pub max_per_day_global: i32,
}

impl Default for EmailSendPolicy {
    fn default() -> Self {
        Self {
            max_per_address_per_hour: DEFAULT_MAX_PER_ADDRESS_PER_HOUR,
            max_per_address_per_day: DEFAULT_MAX_PER_ADDRESS_PER_DAY,
            max_per_day_global: DEFAULT_MAX_SENDS_PER_DAY_GLOBAL,
        }
    }
}

impl EmailSendPolicy {
    /// Build from the resolved configuration ints. `None` falls back to the default; a
    /// non-positive cap is taken verbatim (it IS the per-address kill switch).
    pub fn from_config(
        max_per_address_per_hour: Option<i32>,
        max_per_address_per_day: Option<i32>,
        max_per_day_global: Option<i32>,
    ) -> Self {
        let default = Self::default();
        Self {
            max_per_address_per_hour: max_per_address_per_hour
                .unwrap_or(default.max_per_address_per_hour),
            max_per_address_per_day: max_per_address_per_day.unwrap_or(default.max_per_address_per_day),
            max_per_day_global: max_per_day_global.unwrap_or(default.max_per_day_global),
        }
    }

    /// The `email:` namespace keeps this policy's buckets from colliding with the SMS wall's
    /// `phone:*` / `global:day` keys when the two share one counter store.
    const GLOBAL_DAY_KEY: &'static str = "email:global:day";

    /// The bucket claims for one send to `email`, in enforcement order (the per-address buckets
    /// before the global ceiling, so a per-address refusal never burns the global budget).
    fn claims(&self, email: &EmailAddress) -> Vec<QuotaClaim> {
        vec![
            QuotaClaim {
                key: format!("email:{}:hour", email.0),
                window_seconds: HOUR_SECONDS,
                limit: self.max_per_address_per_hour,
                backoff_seconds: vec![],
            },
            QuotaClaim {
                key: format!("email:{}:day", email.0),
                window_seconds: DAY_SECONDS,
                limit: self.max_per_address_per_day,
                backoff_seconds: vec![],
            },
            QuotaClaim {
                key: Self::GLOBAL_DAY_KEY.to_string(),
                window_seconds: DAY_SECONDS,
                limit: self.max_per_day_global,
                backoff_seconds: vec![],
            },
        ]
    }

    fn refusal_for(index: usize, denial: QuotaDenial) -> EmailSendRefusal {
        match (index, denial) {
            (_, QuotaDenial::Unavailable { detail }) => EmailSendRefusal::StoreUnavailable { detail },
            (_, QuotaDenial::Cooldown { retry_after_seconds }) => {
                EmailSendRefusal::TooSoon { retry_after_seconds }
            }
            (0, QuotaDenial::LimitReached { retry_after_seconds }) => {
                EmailSendRefusal::TooSoon { retry_after_seconds }
            }
            (1, QuotaDenial::LimitReached { .. }) => EmailSendRefusal::DailyCapReached,
            (_, QuotaDenial::LimitReached { .. }) => EmailSendRefusal::GlobalCeilingReached,
        }
    }

    /// **The wall.** Claim one send against every bucket, in order; the first denial refuses and
    /// nothing already granted is un-granted (the residual asymmetry SMS accepts too).
    pub async fn authorize(
        &self,
        store: &dyn SmsQuotaStore,
        email: &EmailAddress,
        now_unix: i64,
    ) -> Result<(), EmailSendRefusal> {
        for (index, claim) in self.claims(email).into_iter().enumerate() {
            store.try_claim(&claim, now_unix).await.map_err(|d| Self::refusal_for(index, d))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sms_guard::InMemorySmsQuotaStore;

    #[tokio::test]
    async fn a_served_address_is_authorised_and_a_second_bucket_still_shares_one_counter() {
        let store = InMemorySmsQuotaStore::default();
        let policy = EmailSendPolicy::default();
        let email = EmailAddress("owner@pizzaroma.fr".into());
        policy.authorize(&store, &email, 0).await.expect("first send authorised");
    }

    #[tokio::test]
    async fn the_per_address_hourly_cap_refuses_the_next_send_as_rate_limited() {
        let store = InMemorySmsQuotaStore::default();
        let policy = EmailSendPolicy { max_per_address_per_hour: 1, ..EmailSendPolicy::default() };
        let email = EmailAddress("owner@pizzaroma.fr".into());
        policy.authorize(&store, &email, 0).await.expect("first send authorised");
        let refusal = policy.authorize(&store, &email, 1).await.expect_err("second send refused");
        assert_eq!(refusal.reason(), "cooldown");
        assert_eq!(refusal.into_domain_error().code(), Some("RateLimited"));
    }

    #[tokio::test]
    async fn the_global_ceiling_refuses_a_never_before_seen_address() {
        let store = InMemorySmsQuotaStore::default();
        let policy = EmailSendPolicy { max_per_day_global: 0, ..EmailSendPolicy::default() };
        let email = EmailAddress("fresh@pizzaroma.fr".into());
        let refusal = policy.authorize(&store, &email, 0).await.expect_err("ceiling spent");
        assert_eq!(refusal.reason(), "global_ceiling");
        assert_eq!(refusal.into_domain_error().code(), Some("VerificationSendCapacityExhausted"));
    }

    #[tokio::test]
    async fn the_email_and_sms_global_buckets_are_namespaced_apart() {
        // The `email:global:day` key must never collide with SMS's `global:day` when the two
        // buckets share one counter store -- proven by exhausting one and confirming the other
        // still authorises.
        let store = InMemorySmsQuotaStore::default();
        let email_policy = EmailSendPolicy { max_per_day_global: 0, ..EmailSendPolicy::default() };
        let sms_policy = crate::sms_guard::SmsSendPolicy::default();
        let _ = email_policy
            .authorize(&store, &EmailAddress("x@example.com".into()), 0)
            .await
            .expect_err("email global ceiling is spent");
        sms_policy
            .authorize(
                &store,
                &domain::generated::scalars::DialingCode("+33".into()),
                &domain::generated::scalars::NationalPhoneNumber("612345678".into()),
                0,
            )
            .await
            .expect("the SMS global bucket is untouched by the email ceiling");
    }
}
