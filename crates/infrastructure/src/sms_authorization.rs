//! The **send seam** for SMS (#516): the authorizer, and the witness that makes an unguarded send
//! unspellable.
//!
//! ## Compiler first (ADR-20260803-234035)
//!
//! The obvious shape here is "remember to call the guard before calling the sender", enforced by
//! review. That fails the moment a second call site appears — and this path already has two doors
//! (the identity ACL and the Supabase send-SMS hook), so "someone added a caller that forgot the
//! check" is not hypothetical.
//!
//! Instead, [`OvhSmsClient::send`](crate::OvhSmsClient::send) takes an
//! [`AuthorizedSmsRecipient`], whose only field is private to THIS module and which has no public
//! constructor. The single way to obtain one is [`SmsSendAuthorizer::authorize`], which claims the
//! shared budget first. A caller holding a phone number and a message therefore has no path to the
//! sender at all: forgetting the guard is a type error, not a review finding.
//!
//! ## Where the money actually moves, and why the wall is here
//!
//! The command handler's `send_phone_otp` only asks the identity provider to send. **The euro is spent
//! later and INBOUND**: Supabase calls our `/auth/sms-hook`, and that route hands the message to OVH
//! on our own account. So a check at the BFF edge or in the handler is *present, not unbypassable* —
//! anyone able to make the provider send (including through the provider's own surfaces) reaches the
//! hook without passing our command path.
//!
//! Hence two enforcement points with different jobs:
//! * the identity ACL adapter runs [`SmsSendAuthorizer::peek`] — **cheap shedding**, so an obviously
//!   doomed request is refused with a typed, renderable reason before a provider round-trip;
//! * this module's [`SmsSendAuthorizer::authorize`] runs on the **hook path**, where the send becomes
//!   a euro. That is the authoritative claim, and it is the one the witness enforces.

use application::sms_guard::{SmsQuotaStore, SmsRefusal, SmsSendPolicy};
use domain::generated::scalars::{DialingCode, NationalPhoneNumber, PhoneNumber};

/// A recipient the guards have **authorised and paid for** out of the shared budget.
///
/// The field is private to this module and there is no public constructor, so this type cannot be
/// forged outside it — that is the whole point. Holding one is proof that the allowlist passed and
/// that one send was claimed against the per-number and global counters.
///
/// **One claim, one send — and that is enforced by MOVE, not by the absence of `Clone`.**
/// `OvhSmsClient::send` takes the witness **by value**, so sending consumes it and a second send
/// needs a second claim. Not being `Clone` is necessary but nowhere near sufficient: this type was
/// briefly passed by shared reference, and `&self`-style borrowing already permits
/// `for _ in 0..1000 { sms.send(&w, m) }` — one claim, a thousand messages, budget spent once.
/// A missing `Clone` impl cannot stop that; only consuming the value can. If you ever find yourself
/// adding `&` to a signature that takes this type, you are re-opening exactly that hole.
#[derive(Debug)]
pub struct AuthorizedSmsRecipient(PhoneNumber);

impl AuthorizedSmsRecipient {
    /// The E.164 recipient to hand to the provider. Borrowing the STRING is fine — it is the witness
    /// itself that must be consumed, and only the sender (which takes it by value) can do so.
    pub fn as_str(&self) -> &str {
        &self.0 .0
    }
}

/// Policy + shared counter: the object both enforcement points hold.
pub struct SmsSendAuthorizer {
    policy: SmsSendPolicy,
    store: Box<dyn SmsQuotaStore>,
}

impl SmsSendAuthorizer {
    pub fn new(policy: SmsSendPolicy, store: Box<dyn SmsQuotaStore>) -> Self {
        Self { policy, store }
    }

    /// The configured policy (read-only) — the composition root logs it at boot so an operator can
    /// see which allowlist and which ceiling are live without reading the deployment.
    pub fn policy(&self) -> &SmsSendPolicy {
        &self.policy
    }

    fn now_unix() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    /// **The wall.** Claim one send against the shared budget and, on success, mint the witness the
    /// sender demands. Emits `otp_send_requested_total` / `otp_send_refused_total`, so a refusal is
    /// never silent — a guard whose firing is invisible is indistinguishable from one that is off.
    pub async fn authorize(
        &self,
        dialing_code: &DialingCode,
        national_number: &NationalPhoneNumber,
    ) -> Result<AuthorizedSmsRecipient, SmsRefusal> {
        let result = self
            .policy
            .authorize(self.store.as_ref(), dialing_code, national_number, Self::now_unix())
            .await;
        telemetry::meters::otp_send::requested(&dialing_code.0, result.is_ok());
        match result {
            Ok(phone) => {
                // Enforcement just happened against the shared counter, so re-assert liveness HERE —
                // where it is actually decided — not only at composition. A boot-time `true` that is
                // never refreshed proves the process once started, which is not the property the
                // gauge exists for (ADR-20260810-231300).
                telemetry::meters::otp_send::guard_enforcing(true);
                Ok(AuthorizedSmsRecipient(phone))
            }
            Err(refusal) => {
                // A refusal we COMPUTED is enforcement working; an unreachable counter is the guard
                // not enforcing at all, and the gauge must say so rather than sit on a stale `1`.
                telemetry::meters::otp_send::guard_enforcing(!matches!(
                    refusal,
                    SmsRefusal::StoreUnavailable { .. }
                ));
                Self::record_refusal(&refusal);
                Err(refusal)
            }
        }
    }

    /// [`Self::authorize`] for a caller that only has a composed **E.164** number — which is the hook
    /// path's situation, because the provider calls us back with `+33612345678`, not with the split
    /// the command carried.
    ///
    /// The split is done by taking the LONGEST allowlisted dialing code that prefixes the number. Note
    /// what this is and is not: it is *parsing*, and it can only ever produce a code that is already on
    /// the allowlist — a number matching none of them is refused outright. So the "a prefix test is not
    /// an allowlist" trap does not apply here: membership is still exact, and the prefix is only used
    /// to find where the country code ends.
    pub async fn authorize_e164(
        &self,
        phone: &str,
    ) -> Result<AuthorizedSmsRecipient, SmsRefusal> {
        let phone = phone.trim();
        let matched = self
            .policy
            .allowed_dialing_codes
            .iter()
            .filter(|code| phone.starts_with(code.as_str()))
            .max_by_key(|code| code.len());
        let Some(code) = matched else {
            // Name what we can without leaking the rest of the number into a refusal or a label: the
            // leading digits up to a plausible country-code length.
            let head: String = phone.chars().take(4).collect();
            // The metric label is bounded by `otp_send::requested` itself (anything unknown collapses
            // to `other`), so passing the observed head here cannot mint time series.
            telemetry::meters::otp_send::requested(&head, false);
            let refusal = SmsRefusal::CountryNotServed { dialing_code: head };
            Self::record_refusal(&refusal);
            return Err(refusal);
        };
        let national = phone[code.len()..].to_string();
        self.authorize(&DialingCode(code.clone()), &NationalPhoneNumber(national)).await
    }

    /// **Cheap shedding, not the wall.** The same decision, recording nothing; used at the identity
    /// ACL so a doomed request is refused before a provider round-trip. Advisory by construction: the
    /// authoritative claim happens at [`Self::authorize`].
    pub async fn peek(
        &self,
        dialing_code: &DialingCode,
        national_number: &NationalPhoneNumber,
    ) -> Result<(), SmsRefusal> {
        match self
            .policy
            .peek(self.store.as_ref(), dialing_code, national_number, Self::now_unix())
            .await
        {
            Ok(_) => Ok(()),
            Err(refusal) => {
                Self::record_refusal(&refusal);
                Err(refusal)
            }
        }
    }

    /// One place decides how loud a refusal is. The global ceiling is the only one that turns real
    /// sign-ups away, so it is the only one that logs at ERROR: if it fires, either the ceiling is too
    /// low for real traffic or an attack is under way, and both need a human tonight.
    fn record_refusal(refusal: &SmsRefusal) {
        telemetry::meters::otp_send::refused(refusal.reason());
        match refusal {
            SmsRefusal::GlobalCeilingReached => tracing::error!(
                reason = refusal.reason(),
                "otp send REFUSED: the global daily SMS ceiling is spent -- real sign-ups are being \
                 turned away. Raise SMS_MAX_SENDS_PER_DAY_GLOBAL, or clear the 'global:day' row in \
                 sms_send_quota, once you know whether this is traffic or an attack (#516)."
            ),
            SmsRefusal::StoreUnavailable { detail } => tracing::error!(
                reason = refusal.reason(),
                detail = %detail,
                "otp send REFUSED: the shared send-quota counter is unreachable, so the guard is \
                 failing closed -- no OTP can be delivered until the database answers (#516)."
            ),
            other => tracing::info!(reason = other.reason(), "otp send refused by the send guards"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::sms_guard::InMemorySmsQuotaStore;

    fn authorizer(policy: SmsSendPolicy) -> SmsSendAuthorizer {
        SmsSendAuthorizer::new(policy, Box::new(InMemorySmsQuotaStore::default()))
    }

    #[tokio::test]
    async fn only_the_authorizer_can_produce_a_recipient_for_the_sender() {
        // The compile-time half of this guarantee cannot be asserted at runtime -- there is no
        // constructor to call and no field to set, which is the point. What IS assertable: the
        // authorizer yields the canonical E.164 the provider will be handed.
        let a = authorizer(SmsSendPolicy::default());
        let witness = a
            .authorize(&DialingCode("+33".into()), &NationalPhoneNumber("0612345678".into()))
            .await
            .expect("a served number is authorised");
        assert_eq!(witness.as_str(), "+33612345678");
    }

    #[tokio::test]
    async fn a_refused_country_yields_no_witness_at_all() {
        let a = authorizer(SmsSendPolicy::default());
        let refusal = a
            .authorize(&DialingCode("+212".into()), &NationalPhoneNumber("612345678".into()))
            .await
            .expect_err("an unserved country must be refused");
        assert_eq!(refusal.reason(), "country_not_served");
    }

    #[tokio::test]
    async fn the_hook_path_and_the_edge_share_one_budget() {
        // A peek must not spend, and a claim must be visible to the next peek -- otherwise the edge
        // check and the wall would drift apart and the edge would start lying.
        let a = authorizer(SmsSendPolicy::default());
        let (code, national) = (DialingCode("+33".into()), NationalPhoneNumber("612345678".into()));
        a.peek(&code, &national).await.expect("a peek is free");
        a.authorize(&code, &national).await.expect("first send");
        assert!(a.peek(&code, &national).await.is_err(), "the edge must see the claim");
    }
}
