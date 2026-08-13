//! The SMS-OTP **send guards** (#516) — the money controls on an endpoint that is anonymous by design.
//!
//! `requestPhoneVerification` takes no identity (a visitor has none yet) and every accepted send
//! spends real money on OUR OVHcloud account (ADR-20260722-174500). That is precisely the shape
//! SMS-pumping automates: drive OTP requests at premium-payout ranges the attacker shares revenue
//! on. The account is a PREPAID credit pack (`specs/common/configuration.yaml`
//! `OVH_SMS_SERVICE_NAME`: with no credits the credentials authenticate and the send still fails),
//! so the failure mode is not an invoice — a burn night drains the pack, every OTP send then fails,
//! nobody can sign up or sign in, and refilling is founder-gated: a phone-login outage of unknown
//! duration. See ADR-20260813-021500 for the posture.
//!
//! Three guards doing three DIFFERENT jobs — none substitutes for another:
//!
//! 1. **The global daily ceiling** is the ECONOMIC control and the only guard that bounds the total
//!    spend, because per-number caps *multiply* under number rotation instead of limiting. At the
//!    default 200/day an attacker's gross is a few euros and their revenue share a fraction of it,
//!    so the ceiling alone destroys the pumping economics for every range. A hard, loud stop.
//! 2. **The country allowlist** contains cost and refuses what we cannot serve — the served-country
//!    decision made executable. Cheap and fail-closed; it is not what makes pumping unprofitable.
//! 3. **Per-number caps** (hour, day) with an escalating cooldown stop one number being pumped.
//!
//! ## What lives here and what does not
//!
//! This module is the **pure policy plus the port**: the allowlist decision, the canonical bucket
//! keys, the escalating cooldown schedule, and the typed refusals. It calls no clock and no database
//! — `now` is a parameter (there is no `Clock` port in this workspace, and a guard that reads the
//! system clock internally cannot have both window edges asserted).
//!
//! The **counter** is a port ([`SmsQuotaStore`]) implemented over Postgres in `infrastructure`,
//! never in memory: a per-pod limiter multiplies the allowance by the replica count and resets on
//! every deploy, which turns a money ceiling into a suggestion.
//!
//! ## Why the counter cannot be a load-modify-save
//!
//! `RequestPhoneVerification` has **no per-phone actor lane** — the GraphQL door mints
//! `actor_id = Uuid::now_v7()` per request, so the mailbox gives this command no one-writer
//! guarantee and nothing serialises two concurrent requests for the same number. The store contract
//! is therefore a single atomic claim, never a read followed by a write.

use serde_json::json;

use domain::generated::scalars::{DialingCode, NationalPhoneNumber, PhoneNumber};
use domain::shared::errors::DomainError;

/// The default served dialing codes (`SMS_ALLOWED_DIALING_CODES`).
///
/// `+33` is V0's traffic (Tours); the rest cover Tours's international students and Loire-valley
/// tourists — a real single-digit share of signups a `+33`-only list would refuse for no additional
/// protection. **A calling code is not a destination.** Before widening this list, the question is
/// what a send to EVERY territory the code reaches pays, because the attacker picks the territory
/// and we pay its rate:
///
/// - `+1` is deliberately absent (#535): it reaches every NANP territory — twenty-odd Caribbean and
///   Pacific jurisdictions with their own operators and rates, including the premium ranges this
///   allowlist exists to exclude — all billed as if they were a Boston number. Serving North
///   America is a market decision nobody has taken; if it ever is taken, the shape is region
///   resolution via a libphonenumber-class dependency, never a hand-maintained NANP area-code table
///   (NANPA reassigns codes several times a year, so a stale table refuses an innocent US customer
///   with a new area code).
/// - `+44` also reaches Guernsey, Jersey and the Isle of Man — separate telecom jurisdictions with
///   their own operators, commonly rated apart from the GB mainland.
/// - `+41` is a COST line rather than a fraud one: Swiss termination is among Europe's more
///   expensive, which is a drain-rate item under a prepaid credit pack.
/// - `+33` is clean BECAUSE the French overseas territories carry their own codes (`+262`, `+590`,
///   `+596`, `+594`, `+687`, `+689`, `+508`) — DOM-TOM is already unreachable here and would be a
///   separate deliberate addition.
pub const DEFAULT_ALLOWED_DIALING_CODES: &[&str] =
    &["+33", "+32", "+41", "+44", "+49", "+34", "+39"];

/// Sends allowed to ONE number per rolling hour (`SMS_MAX_SENDS_PER_NUMBER_PER_HOUR`). Three covers
/// the real fumbling customer: one send plus one or two resends.
pub const DEFAULT_MAX_PER_NUMBER_PER_HOUR: i32 = 3;
/// Sends allowed to ONE number per rolling day (`SMS_MAX_SENDS_PER_NUMBER_PER_DAY`) — the backstop
/// that stops the hourly allowance being harvested 24 times over.
pub const DEFAULT_MAX_PER_NUMBER_PER_DAY: i32 = 5;
/// Cooldown after the Nth send to the same number (`SMS_SEND_BACKOFF_SECONDS`); the last value
/// repeats. Server-side: the client's resend countdown is a courtesy, never an enforcement point.
pub const DEFAULT_BACKOFF_SECONDS: &[i64] = &[30, 120, 600];
/// Platform-wide sends per rolling day (`SMS_MAX_SENDS_PER_DAY_GLOBAL`) — the kill switch and the
/// only ceiling on the spend. DERIVABLE, not a guess: OVH SMS France is €0.06 HT/SMS at 100
/// credits, €0.058 at 1 000 (PROP-20260724-233605, founder-approved 2026-07-24,
/// screenshot-confirmed), so 200/day is €12/day worst case France-rated. Still unknown: OVH's
/// per-destination multipliers and which pack was actually purchased (ADR-20260813-021500).
pub const DEFAULT_MAX_PER_DAY_GLOBAL: i32 = 200;

/// One hour, in seconds.
pub const HOUR_SECONDS: i64 = 3_600;
/// One day, in seconds.
pub const DAY_SECONDS: i64 = 86_400;

/// Why a send was refused. Each variant maps to ONE `errors.yaml` rejection, because the screen must
/// tell them apart: a rate limit rendered as "code incorrect" is a lie at the highest-friction tap in
/// the funnel, and "invalid number" to a Belgian tourist is an accusation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmsRefusal {
    /// The dialing code is not on the served allowlist.
    CountryNotServed { dialing_code: String },
    /// The number could not be parsed into an E.164 recipient at all — refused, never sent
    /// (fail-closed). Reported as `PhoneCountryNotServed` too: we cannot name a country we could not
    /// read, and an unparseable number is not a country we serve.
    Unparseable { dialing_code: String },
    /// Inside this number's cooldown window; `retry_after_seconds` is the SERVER's remaining backoff.
    TooSoon { retry_after_seconds: i64 },
    /// This number's hourly allowance is spent; still a countdown (the hour turns over).
    HourlyCapReached { retry_after_seconds: i64 },
    /// This number's DAILY allowance is spent — no countdown worth rendering, offer help instead.
    DailyCapReached,
    /// The platform's global daily ceiling is spent. Real sign-ups are being turned away; loud.
    GlobalCeilingReached,
    /// The shared counter could not be reached. FAIL CLOSED: an unreachable limiter must not become
    /// an open spend path, which is the whole failure mode this module exists to prevent.
    StoreUnavailable { detail: String },
}

impl SmsRefusal {
    /// The bounded metric label (`otp_send_refused_total{reason}`). Bounded by construction — one
    /// arm per variant, never a formatted string.
    pub fn reason(&self) -> &'static str {
        match self {
            SmsRefusal::CountryNotServed { .. } => "country_not_served",
            SmsRefusal::Unparseable { .. } => "unparseable",
            SmsRefusal::TooSoon { .. } => "cooldown",
            SmsRefusal::HourlyCapReached { .. } => "hourly_cap",
            SmsRefusal::DailyCapReached => "daily_cap",
            SmsRefusal::GlobalCeilingReached => "global_ceiling",
            SmsRefusal::StoreUnavailable { .. } => "store_unavailable",
        }
    }

    /// The typed `errors.yaml` rejection this refusal is delivered as (customer/common errors.yaml,
    /// declared in `actors.yaml` `RequestPhoneVerification.throws`).
    pub fn into_domain_error(self) -> DomainError {
        match self {
            SmsRefusal::CountryNotServed { dialing_code }
            | SmsRefusal::Unparseable { dialing_code } => DomainError::rejected(
                "PhoneCountryNotServed",
                json!({ "dialingCode": dialing_code }),
            ),
            SmsRefusal::TooSoon { retry_after_seconds }
            | SmsRefusal::HourlyCapReached { retry_after_seconds } => DomainError::rejected(
                "RateLimited",
                json!({ "retryAfterSeconds": retry_after_seconds }),
            ),
            SmsRefusal::DailyCapReached => {
                DomainError::rejected("VerificationSendLimitReached", json!({}))
            }
            SmsRefusal::GlobalCeilingReached => {
                DomainError::rejected("VerificationSendCapacityExhausted", json!({}))
            }
            // NOT a `Rejected`: an unreachable limiter is our defect, not the customer's, and it must
            // not be reported as "you asked too often". It still refuses.
            SmsRefusal::StoreUnavailable { detail } => DomainError::Repository(format!(
                "sms send guard: quota store unavailable, refusing the send (fail-closed): {detail}"
            )),
        }
    }
}

/// One quota bucket claim: the key, the window it counts over, and the ceiling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaClaim {
    /// The bucket key. Derived from the CANONICAL number (never the request), so `0612345678`,
    /// `00612345678` and `612345678` under `+33` share one bucket.
    pub key: String,
    /// Rolling window length in seconds.
    pub window_seconds: i64,
    /// Sends allowed in one window.
    pub limit: i32,
    /// Cooldown schedule applied by position within the window (`[]` = no cooldown for this bucket).
    /// The last value repeats for every further attempt.
    pub backoff_seconds: Vec<i64>,
}

/// The outcome of a granted claim — what the store recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaGrant {
    /// Sends now recorded in this window, INCLUDING the one just granted.
    pub sent_count: i32,
}

/// Why a claim was denied, in the store's own terms (the policy turns this into an [`SmsRefusal`],
/// because only the policy knows which bucket was being claimed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuotaDenial {
    /// The ceiling for this window is reached; `retry_after_seconds` is until the window turns over.
    LimitReached { retry_after_seconds: i64 },
    /// Inside the escalating cooldown since the last granted send.
    Cooldown { retry_after_seconds: i64 },
    /// The store could not answer. Fail closed.
    Unavailable { detail: String },
}

/// The shared, cross-replica counter behind the guards.
///
/// **Contract — an atomic claim, never a read-then-write.** `try_claim` must decide and record in ONE
/// statement: this command has no per-phone actor lane, so two concurrent requests for the same
/// number are only serialised by the store. An implementation that reads, decides in Rust and then
/// writes is wrong however careful it looks.
///
/// **Refusals must not consume budget.** A denied claim increments nothing — otherwise a refused
/// send, which costs no money, would burn the ceiling that exists to bound money.
#[async_trait::async_trait]
pub trait SmsQuotaStore: Send + Sync {
    /// Claim one send against `claim`, at instant `now_unix` (seconds). `Ok(grant)` means the caller
    /// may send; `Err(denial)` means it may not, and nothing was recorded.
    async fn try_claim(
        &self,
        claim: &QuotaClaim,
        now_unix: i64,
    ) -> Result<QuotaGrant, QuotaDenial>;

    /// Non-consuming look at `claim`: would it be granted right now? Used for CHEAP EDGE SHEDDING
    /// (see [`SmsSendPolicy::peek`]) — it must never record anything, and a `Ok(())` from it is
    /// advisory: the authoritative decision is always [`Self::try_claim`] at the send seam.
    async fn peek(&self, claim: &QuotaClaim, now_unix: i64) -> Result<(), QuotaDenial>;
}

/// The pure send policy: the allowlist, the bucket keys and the ceilings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmsSendPolicy {
    /// Served dialing codes, matched by EXACT MEMBERSHIP. Never a prefix test: `+337…` passes
    /// `starts_with("+33")` and a prefix test is therefore not an allowlist.
    pub allowed_dialing_codes: Vec<String>,
    pub max_per_number_per_hour: i32,
    pub max_per_number_per_day: i32,
    pub backoff_seconds: Vec<i64>,
    pub max_per_day_global: i32,
}

impl Default for SmsSendPolicy {
    fn default() -> Self {
        Self {
            allowed_dialing_codes: DEFAULT_ALLOWED_DIALING_CODES
                .iter()
                .map(|s| s.to_string())
                .collect(),
            max_per_number_per_hour: DEFAULT_MAX_PER_NUMBER_PER_HOUR,
            max_per_number_per_day: DEFAULT_MAX_PER_NUMBER_PER_DAY,
            backoff_seconds: DEFAULT_BACKOFF_SECONDS.to_vec(),
            max_per_day_global: DEFAULT_MAX_PER_DAY_GLOBAL,
        }
    }
}

impl SmsSendPolicy {
    /// Build from the resolved configuration strings (`SMS_ALLOWED_DIALING_CODES`,
    /// `SMS_SEND_BACKOFF_SECONDS`) and ints. An EMPTY or unparseable list falls back to the default
    /// rather than to "everything allowed": a typo in configuration must not open the spend path.
    pub fn from_config(
        allowed_dialing_codes: Option<&str>,
        max_per_number_per_hour: Option<i32>,
        max_per_number_per_day: Option<i32>,
        backoff_seconds: Option<&str>,
        max_per_day_global: Option<i32>,
    ) -> Self {
        let default = Self::default();
        let codes: Vec<String> = allowed_dialing_codes
            .map(|raw| {
                raw.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .filter(|v: &Vec<String>| !v.is_empty())
            .unwrap_or(default.allowed_dialing_codes);
        let backoff: Vec<i64> = backoff_seconds
            .map(|raw| raw.split(',').filter_map(|s| s.trim().parse::<i64>().ok()).collect())
            .filter(|v: &Vec<i64>| !v.is_empty())
            .unwrap_or(default.backoff_seconds);
        Self {
            allowed_dialing_codes: codes,
            // A non-positive cap would mean "no sends ever"; that IS a legal setting (it is the
            // per-number kill switch), so it is taken verbatim. Only ABSENT falls back.
            max_per_number_per_hour: max_per_number_per_hour.unwrap_or(default.max_per_number_per_hour),
            max_per_number_per_day: max_per_number_per_day.unwrap_or(default.max_per_number_per_day),
            backoff_seconds: backoff,
            max_per_day_global: max_per_day_global.unwrap_or(default.max_per_day_global),
        }
    }

    /// FAIL-CLOSED canonicalization + allowlist. `Ok` yields the E.164 recipient the counters are
    /// keyed on; every other outcome is a refusal, and no caller can reach a sender without one.
    ///
    /// The checks, and why each exists:
    /// * exact membership of the dialing code (a prefix test is not an allowlist);
    /// * the national part must be ASCII digits only — `canonical_phone("+33", "6abc12")` composes
    ///   `"+336abc12"`, which passes any `starts_with("+33")` test and is not a phone number;
    /// * length must be plausible E.164 (max 15 digits total, and a subscriber part of at least 6)
    ///   — `canonical_phone("+33", "0000000")` composes the bare `"+33"` once the trunk zero run is
    ///   stripped, i.e. a *country code* handed to a sender as a recipient.
    pub fn authorize_recipient(
        &self,
        dialing_code: &DialingCode,
        national_number: &NationalPhoneNumber,
    ) -> Result<PhoneNumber, SmsRefusal> {
        let code = dialing_code.0.trim().to_string();
        if !self.allowed_dialing_codes.iter().any(|allowed| allowed == &code) {
            return Err(SmsRefusal::CountryNotServed { dialing_code: code });
        }
        // The dialing code itself must be well formed even when allowlisted, or the allowlist is
        // trusting its own configuration to be a phone number.
        if !code.starts_with('+') || !code[1..].chars().all(|c| c.is_ascii_digit()) || code.len() < 2
        {
            return Err(SmsRefusal::Unparseable { dialing_code: code });
        }
        let national = national_number.0.trim();
        // ASCII digits ONLY: `char::is_numeric` accepts Unicode digits (٦, ６), which are not
        // dialable and which a downstream provider may normalise in a way we did not authorise.
        if national.is_empty() || !national.chars().all(|c| c.is_ascii_digit()) {
            return Err(SmsRefusal::Unparseable { dialing_code: code });
        }
        let subscriber = national.trim_start_matches('0');
        if subscriber.len() < 6 {
            return Err(SmsRefusal::Unparseable { dialing_code: code });
        }
        // E.164: at most 15 digits including the country code.
        if code[1..].len() + subscriber.len() > 15 {
            return Err(SmsRefusal::Unparseable { dialing_code: code });
        }
        Ok(PhoneNumber(format!("{code}{subscriber}")))
    }

    /// The bucket claims for one send to `phone`, **in enforcement order**.
    ///
    /// The order is a money decision, not a style one: the per-number buckets are claimed BEFORE the
    /// global ceiling, so a request that a per-number cap will refuse never burns the global budget.
    /// The residual asymmetry is deliberate and always in the strict direction — if the hourly bucket
    /// grants and the daily one then denies, the hourly count is one higher than sends actually made.
    pub fn claims(&self, phone: &PhoneNumber) -> Vec<QuotaClaim> {
        vec![
            QuotaClaim {
                key: format!("phone:{}:hour", phone.0),
                window_seconds: HOUR_SECONDS,
                limit: self.max_per_number_per_hour,
                backoff_seconds: self.backoff_seconds.clone(),
            },
            QuotaClaim {
                key: format!("phone:{}:day", phone.0),
                window_seconds: DAY_SECONDS,
                limit: self.max_per_number_per_day,
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

    /// The global daily ceiling's bucket key. Named because it is the row an operator edits to stop
    /// or restore sends on a live system with no deploy at all.
    pub const GLOBAL_DAY_KEY: &'static str = "global:day";

    /// Map a store denial to the refusal for the bucket it came from. `index` is the position in
    /// [`Self::claims`].
    fn refusal_for(index: usize, denial: QuotaDenial) -> SmsRefusal {
        match (index, denial) {
            (_, QuotaDenial::Unavailable { detail }) => SmsRefusal::StoreUnavailable { detail },
            (_, QuotaDenial::Cooldown { retry_after_seconds }) => {
                SmsRefusal::TooSoon { retry_after_seconds }
            }
            (0, QuotaDenial::LimitReached { retry_after_seconds }) => {
                SmsRefusal::HourlyCapReached { retry_after_seconds }
            }
            (1, QuotaDenial::LimitReached { .. }) => SmsRefusal::DailyCapReached,
            // The global bucket, and anything a future bucket adds, is treated as the ceiling —
            // erring towards the LOUD refusal rather than towards a soft countdown.
            (_, QuotaDenial::LimitReached { .. }) => SmsRefusal::GlobalCeilingReached,
        }
    }

    /// **The authoritative decision**: allowlist, then claim every bucket. `Ok` is a promise that a
    /// send has been paid for out of the shared budget.
    pub async fn authorize(
        &self,
        store: &dyn SmsQuotaStore,
        dialing_code: &DialingCode,
        national_number: &NationalPhoneNumber,
        now_unix: i64,
    ) -> Result<PhoneNumber, SmsRefusal> {
        let phone = self.authorize_recipient(dialing_code, national_number)?;
        for (index, claim) in self.claims(&phone).iter().enumerate() {
            store
                .try_claim(claim, now_unix)
                .await
                .map_err(|denial| Self::refusal_for(index, denial))?;
        }
        Ok(phone)
    }

    /// **Cheap edge shedding, NOT the wall.** Same decision, recording nothing — used where a request
    /// arrives (the command path) so an obviously doomed request is refused with a typed, renderable
    /// reason before it becomes an `inbound_messages` row and a worker delivery.
    ///
    /// It is advisory by construction: between a `peek` and the send, another replica may spend the
    /// last of the budget. The authoritative claim happens at the send seam.
    pub async fn peek(
        &self,
        store: &dyn SmsQuotaStore,
        dialing_code: &DialingCode,
        national_number: &NationalPhoneNumber,
        now_unix: i64,
    ) -> Result<PhoneNumber, SmsRefusal> {
        let phone = self.authorize_recipient(dialing_code, national_number)?;
        for (index, claim) in self.claims(&phone).iter().enumerate() {
            store
                .peek(claim, now_unix)
                .await
                .map_err(|denial| Self::refusal_for(index, denial))?;
        }
        Ok(phone)
    }
}

/// An in-memory [`SmsQuotaStore`] for TEST DOUBLES ONLY, and the reference semantics the Postgres
/// implementation must match statement-for-statement.
///
/// **Never wire this into a served process.** It is per-process state: with N replicas the allowance
/// becomes cap x N, and every deploy resets it — for the global ceiling that is the difference between
/// a ceiling and a suggestion. It exists so the window edges and the cooldown ladder can be asserted
/// with an injected clock, which the real store also supports but only against a live database.
#[derive(Default)]
pub struct InMemorySmsQuotaStore {
    rows: std::sync::Mutex<std::collections::HashMap<String, QuotaRow>>,
}

/// One in-memory bucket row — the same four columns as `sms_send_quota`.
#[derive(Debug, Clone, Copy)]
struct QuotaRow {
    window_start: i64,
    sent_count: i32,
    last_granted_at: i64,
}

impl InMemorySmsQuotaStore {
    /// The shared decision, written once so `try_claim` and `peek` can never disagree — a peek that
    /// says yes where a claim says no (or worse, the reverse) would make the edge check a liar.
    /// `consume` distinguishes the authoritative claim from the advisory look.
    fn decide(&self, claim: &QuotaClaim, now_unix: i64, consume: bool) -> Result<QuotaGrant, QuotaDenial> {
        let mut rows = self.rows.lock().expect("sms quota store poisoned");
        let existing = rows.get(&claim.key).copied();
        // A window whose end has passed resets. `now < window_start` (a clock that went BACKWARDS)
        // deliberately does NOT reset: treating it as expiry would let a skewed replica hand out a
        // fresh allowance, so a backwards clock keeps counting against the live window.
        let expired = existing
            .map(|row| now_unix >= row.window_start.saturating_add(claim.window_seconds))
            .unwrap_or(true);
        // A ceiling of zero refuses the FIRST send, fresh window or not — that is the kill switch,
        // and it must not be reachable only once a row happens to exist.
        if claim.limit < 1 {
            return Err(QuotaDenial::LimitReached {
                retry_after_seconds: claim.window_seconds,
            });
        }
        let row = match existing {
            Some(row) if !expired => row,
            _ => {
                if consume {
                    rows.insert(
                        claim.key.clone(),
                        QuotaRow { window_start: now_unix, sent_count: 1, last_granted_at: now_unix },
                    );
                }
                return Ok(QuotaGrant { sent_count: 1 });
            }
        };
        if row.sent_count >= claim.limit {
            return Err(QuotaDenial::LimitReached {
                retry_after_seconds: (row.window_start + claim.window_seconds - now_unix).max(0),
            });
        }
        let cooldown = cooldown_after(&claim.backoff_seconds, row.sent_count);
        let ready_at = row.last_granted_at.saturating_add(cooldown);
        if now_unix < ready_at {
            return Err(QuotaDenial::Cooldown { retry_after_seconds: (ready_at - now_unix).max(0) });
        }
        let next = QuotaRow { sent_count: row.sent_count + 1, last_granted_at: now_unix, ..row };
        if consume {
            rows.insert(claim.key.clone(), next);
        }
        Ok(QuotaGrant { sent_count: next.sent_count })
    }
}

#[async_trait::async_trait]
impl SmsQuotaStore for InMemorySmsQuotaStore {
    async fn try_claim(&self, claim: &QuotaClaim, now_unix: i64) -> Result<QuotaGrant, QuotaDenial> {
        self.decide(claim, now_unix, true)
    }

    async fn peek(&self, claim: &QuotaClaim, now_unix: i64) -> Result<(), QuotaDenial> {
        self.decide(claim, now_unix, false).map(|_| ())
    }
}

/// The cooldown owed after `sent_count` granted sends, from a schedule whose last value repeats.
/// `sent_count == 0` owes nothing (nobody has been sent to yet).
pub fn cooldown_after(backoff_seconds: &[i64], sent_count: i32) -> i64 {
    if sent_count <= 0 || backoff_seconds.is_empty() {
        return 0;
    }
    let index = (sent_count as usize - 1).min(backoff_seconds.len() - 1);
    backoff_seconds[index]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::services::{IdentityService, IdentitySendPhoneOtpInput, ServiceCallMeta};

    const T0: i64 = 1_760_000_000;

    fn dc(s: &str) -> DialingCode {
        DialingCode(s.into())
    }
    fn nn(s: &str) -> NationalPhoneNumber {
        NationalPhoneNumber(s.into())
    }
    /// A policy with NO cooldown, for the tests that are about counts rather than about the ladder.
    fn no_backoff() -> SmsSendPolicy {
        SmsSendPolicy { backoff_seconds: vec![], ..SmsSendPolicy::default() }
    }

    // ---- The allowlist: exact membership, fail-closed -------------------------------------------

    #[test]
    fn the_allowlist_is_exact_membership_never_a_prefix() {
        let p = SmsSendPolicy::default();
        // '+337…' is a DIFFERENT country code that passes `starts_with("+33")`. A prefix test is not
        // an allowlist, and this is the case that proves the difference.
        assert!(matches!(
            p.authorize_recipient(&dc("+337"), &nn("612345678")),
            Err(SmsRefusal::CountryNotServed { .. })
        ));
        assert_eq!(
            p.authorize_recipient(&dc("+33"), &nn("612345678")).unwrap(),
            PhoneNumber("+33612345678".into())
        );
    }

    #[test]
    fn a_high_payout_range_is_refused_and_names_the_country() {
        let p = SmsSendPolicy::default();
        match p.authorize_recipient(&dc("+212"), &nn("612345678")) {
            Err(SmsRefusal::CountryNotServed { dialing_code }) => assert_eq!(dialing_code, "+212"),
            other => panic!("expected a country refusal naming +212, got {other:?}"),
        }
    }

    #[test]
    fn the_low_payout_allowlist_serves_real_visitors() {
        // Tours has international students and tourists: refusing these buys no protection, since
        // none of these ranges pays an attacker anything.
        let p = SmsSendPolicy::default();
        for code in ["+33", "+32", "+41", "+44", "+49", "+34", "+39"] {
            assert!(
                p.authorize_recipient(&dc(code), &nn("612345678")).is_ok(),
                "{code} must be served"
            );
        }
    }

    #[test]
    fn a_calling_code_is_not_a_destination_bare_plus_one_is_refused() {
        // `+1` reaches every NANP territory — including the premium-payout Caribbean ranges this
        // allowlist exists to exclude — all billed as if they were a Boston number, so the DEFAULT
        // list does not serve it (#535). Serving North America is a market decision, not a default.
        let p = SmsSendPolicy::default();
        match p.authorize_recipient(&dc("+1"), &nn("6175551234")) {
            Err(SmsRefusal::CountryNotServed { dialing_code }) => assert_eq!(dialing_code, "+1"),
            other => panic!("expected +1 to be refused as not served, got {other:?}"),
        }
    }

    #[test]
    fn an_unparseable_number_is_refused_never_sent() {
        let p = SmsSendPolicy::default();
        // Each of these composes something that PASSES a `starts_with("+33")` check via
        // `canonical_phone`, which is exactly why the guard cannot be a prefix test.
        let bad: &[(&str, &str)] = &[
            ("+33", "0000000"),      // canonical_phone -> the bare "+33": a country code as a recipient
            ("+33", "6abc12"),       // canonical_phone -> "+336abc12"
            ("+33", ""),             // empty subscriber
            ("+33", "٦١٢٣٤٥٦٧٨"),    // Unicode digits: `is_numeric` accepts them, dialing does not
            ("+33", "612345678901234567890123"), // 24 digits: beyond E.164's 15
            ("+33", "12345"),        // too short to be a subscriber number
        ];
        for (code, national) in bad {
            match p.authorize_recipient(&dc(code), &nn(national)) {
                Err(SmsRefusal::Unparseable { .. }) => {}
                other => panic!("({code},{national}) must be refused as unparseable, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_configuration_typo_does_not_open_the_spend_path() {
        // An empty or blank allowlist falls back to the DEFAULT, never to "everything allowed".
        for raw in ["", "   ", ",,"] {
            let p = SmsSendPolicy::from_config(Some(raw), None, None, None, None);
            assert_eq!(p.allowed_dialing_codes, SmsSendPolicy::default().allowed_dialing_codes);
            assert!(p.authorize_recipient(&dc("+212"), &nn("612345678")).is_err());
        }
        // Same for an unparseable backoff schedule.
        let p = SmsSendPolicy::from_config(None, None, None, Some("soon,later"), None);
        assert_eq!(p.backoff_seconds, DEFAULT_BACKOFF_SECONDS.to_vec());
    }

    // ---- The budget is keyed on the CANONICAL number --------------------------------------------

    #[test]
    fn every_formatting_of_one_number_shares_one_budget() {
        let p = SmsSendPolicy::default();
        let keys: Vec<String> = ["0612345678", "00612345678", "612345678"]
            .iter()
            .map(|n| p.claims(&p.authorize_recipient(&dc("+33"), &nn(n)).unwrap())[0].key.clone())
            .collect();
        assert_eq!(
            keys.iter().collect::<std::collections::HashSet<_>>().len(),
            1,
            "three spellings of one number must share one bucket, got {keys:?}"
        );
        assert_eq!(keys[0], "phone:+33612345678:hour");
    }

    // ---- Per-number caps, both window edges -----------------------------------------------------

    #[tokio::test]
    async fn the_second_send_inside_the_cooldown_is_refused_then_allowed() {
        let store = InMemorySmsQuotaStore::default();
        let p = SmsSendPolicy::default();
        p.authorize(&store, &dc("+33"), &nn("612345678"), T0).await.expect("first send");
        // 30s ladder: 1ms short is REFUSED, the boundary itself is ALLOWED. Asserting only one edge
        // lets an off-by-one and a never-expiring window both pass.
        match p.authorize(&store, &dc("+33"), &nn("612345678"), T0 + 29).await {
            Err(SmsRefusal::TooSoon { retry_after_seconds }) => assert_eq!(retry_after_seconds, 1),
            other => panic!("inside the cooldown must be TooSoon, got {other:?}"),
        }
        p.authorize(&store, &dc("+33"), &nn("612345678"), T0 + 30).await.expect("at the boundary");
    }

    #[tokio::test]
    async fn the_cooldown_ladder_escalates_and_the_last_step_repeats() {
        assert_eq!(cooldown_after(DEFAULT_BACKOFF_SECONDS, 1), 30);
        assert_eq!(cooldown_after(DEFAULT_BACKOFF_SECONDS, 2), 120);
        assert_eq!(cooldown_after(DEFAULT_BACKOFF_SECONDS, 3), 600);
        assert_eq!(cooldown_after(DEFAULT_BACKOFF_SECONDS, 9), 600, "the last step repeats");
        assert_eq!(cooldown_after(DEFAULT_BACKOFF_SECONDS, 0), 0, "nobody has been sent to yet");
    }

    #[tokio::test]
    async fn the_hourly_cap_refuses_the_fourth_send_and_resets_with_the_window() {
        let store = InMemorySmsQuotaStore::default();
        let p = no_backoff();
        for i in 0..p.max_per_number_per_hour {
            p.authorize(&store, &dc("+33"), &nn("612345678"), T0 + i as i64)
                .await
                .unwrap_or_else(|e| panic!("send {i} must be allowed, got {e:?}"));
        }
        match p.authorize(&store, &dc("+33"), &nn("612345678"), T0 + 10).await {
            Err(SmsRefusal::HourlyCapReached { retry_after_seconds }) => {
                assert_eq!(retry_after_seconds, HOUR_SECONDS - 10)
            }
            other => panic!("the 4th send in the hour must be refused, got {other:?}"),
        }
        // One second short of the window: still refused. At the window: allowed again.
        assert!(p.authorize(&store, &dc("+33"), &nn("612345678"), T0 + HOUR_SECONDS - 1).await.is_err());
        p.authorize(&store, &dc("+33"), &nn("612345678"), T0 + HOUR_SECONDS)
            .await
            .expect("the window turned over");
    }

    #[tokio::test]
    async fn the_daily_cap_backstops_the_hourly_allowance() {
        let store = InMemorySmsQuotaStore::default();
        let p = no_backoff();
        // Walk one send per hour: the hourly bucket resets every time, the daily one does not.
        for h in 0..p.max_per_number_per_day as i64 {
            p.authorize(&store, &dc("+33"), &nn("612345678"), T0 + h * HOUR_SECONDS)
                .await
                .unwrap_or_else(|e| panic!("hourly send {h} must be allowed, got {e:?}"));
        }
        let next = T0 + p.max_per_number_per_day as i64 * HOUR_SECONDS;
        assert_eq!(
            p.authorize(&store, &dc("+33"), &nn("612345678"), next).await,
            Err(SmsRefusal::DailyCapReached),
            "the daily cap must stop the hourly allowance being harvested 24 times"
        );
    }

    #[tokio::test]
    async fn a_clock_that_goes_backwards_denies_rather_than_resetting_the_counter() {
        let store = InMemorySmsQuotaStore::default();
        let p = no_backoff();
        for i in 0..p.max_per_number_per_hour {
            p.authorize(&store, &dc("+33"), &nn("612345678"), T0 + i as i64).await.unwrap();
        }
        // A replica whose clock is an hour behind must NOT be handed a fresh allowance: treating
        // `now < window_start` as expiry would make skew a bypass.
        assert!(
            p.authorize(&store, &dc("+33"), &nn("612345678"), T0 - HOUR_SECONDS).await.is_err(),
            "a backwards clock must deny, never reset"
        );
    }

    // ---- The global ceiling: the only guard that bounds the bill --------------------------------

    #[tokio::test]
    async fn the_global_ceiling_stops_every_number_once_the_day_is_spent() {
        let store = InMemorySmsQuotaStore::default();
        // A tiny ceiling, generous per-number caps: this is the number-ROTATION attack, which the
        // per-number caps cannot see at all.
        let p = SmsSendPolicy {
            max_per_day_global: 2,
            max_per_number_per_hour: 99,
            max_per_number_per_day: 99,
            backoff_seconds: vec![],
            ..SmsSendPolicy::default()
        };
        p.authorize(&store, &dc("+33"), &nn("612345671"), T0).await.expect("1st number");
        p.authorize(&store, &dc("+33"), &nn("612345672"), T0).await.expect("2nd number");
        assert_eq!(
            p.authorize(&store, &dc("+33"), &nn("612345673"), T0).await,
            Err(SmsRefusal::GlobalCeilingReached),
            "a fresh number must NOT get a fresh budget once the platform ceiling is spent"
        );
        // And it does turn over with the day — a ceiling, not a permanent outage.
        p.authorize(&store, &dc("+33"), &nn("612345673"), T0 + DAY_SECONDS).await.expect("next day");
    }

    #[tokio::test]
    async fn a_refused_send_does_not_burn_the_global_budget() {
        let store = InMemorySmsQuotaStore::default();
        let p = SmsSendPolicy { max_per_day_global: 1, ..SmsSendPolicy::default() };
        // A refusal on the per-number cooldown must leave the global ceiling untouched, or refusals
        // would spend the money budget they exist to protect.
        p.authorize(&store, &dc("+33"), &nn("612345678"), T0).await.expect("first send");
        assert!(p.authorize(&store, &dc("+33"), &nn("612345678"), T0 + 1).await.is_err());
        // The ceiling of 1 was consumed by the ONE real send, so a different number is refused —
        // but the point is the count, checked directly.
        let global = QuotaClaim {
            key: SmsSendPolicy::GLOBAL_DAY_KEY.into(),
            window_seconds: DAY_SECONDS,
            limit: 99,
            backoff_seconds: vec![],
        };
        assert_eq!(
            store.try_claim(&global, T0 + 2).await.unwrap().sent_count,
            2,
            "exactly ONE send should have been charged before this claim"
        );
    }

    #[tokio::test]
    async fn the_kill_switch_stops_everything() {
        let store = InMemorySmsQuotaStore::default();
        let p = SmsSendPolicy { max_per_day_global: 0, ..SmsSendPolicy::default() };
        assert_eq!(
            p.authorize(&store, &dc("+33"), &nn("612345678"), T0).await,
            Err(SmsRefusal::GlobalCeilingReached),
            "a ceiling of zero must refuse the very first send"
        );
    }

    // ---- peek() is advisory but never disagrees with the claim ----------------------------------

    #[tokio::test]
    async fn peek_records_nothing_and_agrees_with_the_claim() {
        let store = InMemorySmsQuotaStore::default();
        let p = SmsSendPolicy::default();
        for _ in 0..5 {
            p.peek(&store, &dc("+33"), &nn("612345678"), T0).await.expect("peek is free");
        }
        // Five peeks charged nothing, so the first real send is still the first.
        p.authorize(&store, &dc("+33"), &nn("612345678"), T0).await.expect("first send");
        assert!(
            p.peek(&store, &dc("+33"), &nn("612345678"), T0).await.is_err(),
            "a peek inside the cooldown must refuse, or the edge check is a liar"
        );
    }

    // ---- The refusal is a TYPED, renderable fact ------------------------------------------------

    #[test]
    fn every_refusal_maps_to_its_declared_error_and_a_bounded_reason() {
        let cases: Vec<(SmsRefusal, &str, &str)> = vec![
            (
                SmsRefusal::CountryNotServed { dialing_code: "+212".into() },
                "PhoneCountryNotServed",
                "country_not_served",
            ),
            (
                SmsRefusal::Unparseable { dialing_code: "+33".into() },
                "PhoneCountryNotServed",
                "unparseable",
            ),
            (SmsRefusal::TooSoon { retry_after_seconds: 30 }, "RateLimited", "cooldown"),
            (SmsRefusal::HourlyCapReached { retry_after_seconds: 9 }, "RateLimited", "hourly_cap"),
            (SmsRefusal::DailyCapReached, "VerificationSendLimitReached", "daily_cap"),
            (
                SmsRefusal::GlobalCeilingReached,
                "VerificationSendCapacityExhausted",
                "global_ceiling",
            ),
        ];
        for (refusal, code, reason) in cases {
            assert_eq!(refusal.reason(), reason);
            let err = refusal.into_domain_error();
            assert_eq!(err.code(), Some(code), "{reason} must be delivered as {code}");
        }
    }

    #[test]
    fn a_countdown_refusal_carries_the_servers_own_seconds() {
        // The screen must render the SERVER's remaining window, not a client guess.
        let err = SmsRefusal::TooSoon { retry_after_seconds: 42 }.into_domain_error();
        match err {
            DomainError::Rejected { context, .. } => {
                assert_eq!(context["retryAfterSeconds"], 42)
            }
            other => panic!("expected a typed rejection, got {other:?}"),
        }
    }

    #[test]
    fn an_unreachable_counter_refuses_and_is_not_blamed_on_the_customer() {
        // Fail-closed: an unreachable limiter must not become an open spend path — and it must not be
        // reported as "you asked too often" either, because it is our defect.
        let err = SmsRefusal::StoreUnavailable { detail: "connection refused".into() }
            .into_domain_error();
        assert_eq!(err.code(), None, "a store outage is not an anticipated business rejection");
        assert!(format!("{err}").contains("fail-closed"));
    }

    // ---- Through the HANDLER: the money assertion ----------------------------------------------
    //
    // Enforced at the handler/port, not the resolver: the GraphQL door only ENQUEUES, and the worker
    // that sends also RETRIES — so a guard that counted enqueues would be wrong on every retry.

    #[tokio::test]
    async fn the_handler_refuses_an_unserved_country_and_attempts_no_sms() {
        let bed = crate::behaviour_support::TestBed::new();
        let cmd = domain::generated::commands::RequestPhoneVerification {
            dialing_code: dc("+212"),
            national_number: nn("612345678"),
            locale: None,
        };
        let err = crate::commands::request_phone_verification(
            &bed.store,
            &bed.identity,
            cmd,
            &crate::behaviour_support::actor(),
        )
        .await
        .expect_err("an unserved country must be refused");
        assert_eq!(err.code(), Some("PhoneCountryNotServed"));
        // THE assertion. The response code alone would not prove the money stayed put.
        assert!(bed.identity.sent().is_empty(), "no SMS may be attempted for a refused country");
    }

    #[tokio::test]
    async fn the_handler_lets_the_first_send_through_and_refuses_the_immediate_resend() {
        let bed = crate::behaviour_support::TestBed::new();
        let actor = crate::behaviour_support::actor();
        let send = || {
            crate::commands::request_phone_verification(
                &bed.store,
                &bed.identity,
                domain::generated::commands::RequestPhoneVerification {
                    dialing_code: dc("+33"),
                    national_number: nn("612345678"),
                    locale: None,
                },
                &actor,
            )
        };
        send().await.expect("the first send is the product working");
        let err = send().await.expect_err("an immediate resend is inside the cooldown");
        assert_eq!(err.code(), Some("RateLimited"));
        assert_eq!(bed.identity.sent().len(), 1, "exactly ONE SMS may have been attempted");
    }

    #[tokio::test]
    async fn a_phone_change_draws_on_the_same_budget_as_a_verification() {
        // Two doors, one number, one budget (#516): keying the counter on the command would double
        // the cap for anyone who asks through both.
        let bed = crate::behaviour_support::TestBed::new();
        bed.identity
            .send_phone_otp(
                IdentitySendPhoneOtpInput {
                    dialing_code: dc("+33"),
                    national_number: nn("612345678"),
                    locale: None,
                },
                &ServiceCallMeta::new(uuid::Uuid::now_v7()),
            )
            .await
            .expect("first send");
        // The SAME number, differently spelled, through the change door.
        let err = bed
            .identity
            .send_phone_otp(
                IdentitySendPhoneOtpInput {
                    dialing_code: dc("+33"),
                    national_number: nn("0612345678"),
                    locale: None,
                },
                &ServiceCallMeta::new(uuid::Uuid::now_v7()),
            )
            .await
            .expect_err("the same number must share the budget whatever the spelling or the door");
        assert_eq!(err.code(), Some("RateLimited"));
        assert_eq!(bed.identity.sent().len(), 1);
    }
}
