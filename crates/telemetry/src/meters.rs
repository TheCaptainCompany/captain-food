//! The metric instruments the contracts declare, kept separate from the spans.
//!
//! `specs/observability.yaml` splits `metrics` (technical) from `business_metrics` (BAM) and says the
//! split is a requirement, not a style choice: BAM answers "is the business working" and must not be
//! polluted by retry counts. That split is mirrored in the two modules below.
//!
//! Instruments are built once behind `OnceLock`. Creating an instrument per call would allocate on every
//! command and — worse — is how duplicate time series with inconsistent attribute sets appear.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::OnceLock;

use opentelemetry::metrics::{Counter, Gauge, Histogram, Meter, ObservableGauge};
use opentelemetry::KeyValue;

use crate::contract::metric;

fn meter() -> &'static Meter {
    static METER: OnceLock<Meter> = OnceLock::new();
    METER.get_or_init(|| opentelemetry::global::meter("captain-food"))
}

/// Technical metrics for the `command-acceptance` contract.
pub mod acceptance {
    use super::*;

    fn accepted_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::COMMANDS_ACCEPTED_TOTAL).build())
    }

    fn duplicates_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::COMMAND_DUPLICATES_TOTAL).build())
    }

    fn conflicts_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::COMMAND_SYNC_CONFLICTS_TOTAL).build())
    }

    fn completion_histogram() -> &'static Histogram<f64> {
        static H: OnceLock<Histogram<f64>> = OnceLock::new();
        H.get_or_init(|| {
            meter().f64_histogram(metric::COMMAND_COMPLETION_MS).with_unit("ms").build()
        })
    }

    /// One accepted command submission (`commands_accepted_total{channel}`).
    pub fn accepted(channel: &str) {
        accepted_counter().add(1, &[KeyValue::new("channel", channel.to_string())]);
    }

    /// An idempotent replay: same `messageId`, same payload (`command_duplicates_total{channel}`).
    ///
    /// This is a SUCCESSFUL acceptance, counted separately — the contract's comment calls it "client
    /// retry correctness". A rising duplicate rate is a client retrying more than it should, which is
    /// healthy behaviour to see and unhealthy to ignore.
    pub fn duplicate(channel: &str) {
        duplicates_counter().add(1, &[KeyValue::new("channel", channel.to_string())]);
        // Counted as accepted too: from the caller's perspective the submission WAS accepted (the
        // original acceptance is replayed), so excluding it would understate the accept rate.
        accepted(channel);
    }

    /// `messageId` reused with a DIFFERENT payload — a client bug, never a retry
    /// (`command_sync_conflicts_total{command_type}`).
    pub fn sync_conflict(command_type: &str) {
        conflicts_counter().add(1, &[KeyValue::new("command_type", command_type.to_string())]);
    }

    /// Journal insert → terminal status, in ms (`command_completion_ms{status}`), where status is
    /// SUCCEEDED | REJECTED | FAILED. REJECTED and FAILED stay split: a business rejection is the
    /// system working, a technical failure is not, and averaging them together hides both.
    pub fn completed(status: &str, elapsed_ms: f64) {
        completion_histogram()
            .record(elapsed_ms, &[KeyValue::new("status", status.to_string())]);
    }
}

/// Mailbox push + poison signals of the `command-acceptance` contract (PROP-20260802-223522).
pub mod mailbox {
    use super::*;

    fn poison_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::MAILBOX_POISON_FAILED_TOTAL).build())
    }

    fn push_down_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::MAILBOX_PUSH_DOWN_TOTAL).build())
    }

    /// A delivery terminally FAILED by the attempts cap — the operator event D4 names
    /// (`mailbox_poison_failed_total{actor_type}`). The row carries the error evidence; this
    /// counter is what a trigger can watch.
    pub fn poison_failed(actor_type: &str) {
        poison_counter().add(1, &[KeyValue::new("actor_type", actor_type.to_string())]);
    }

    /// The push listener went DOWN (`mailbox_push_down_total{reason}`): connection lost, or the
    /// liveness canary timed out (the transaction-pooler silently-deaf-LISTEN mode). Workers are
    /// on the pre-push cadence until recovery.
    pub fn push_down(reason: &str) {
        push_down_counter().add(1, &[KeyValue::new("reason", reason.to_string())]);
    }
}

/// The `place-order` contract: one technical histogram plus the two BAM counters.
pub mod place_order {
    use super::*;

    fn duration_histogram() -> &'static Histogram<f64> {
        static H: OnceLock<Histogram<f64>> = OnceLock::new();
        H.get_or_init(|| {
            meter().f64_histogram(metric::PLACE_ORDER_DURATION_MS).with_unit("ms").build()
        })
    }

    fn placed_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::ORDERS_PLACED_TOTAL).build())
    }

    fn payment_failures_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::CHECKOUT_PAYMENT_FAILURES_TOTAL).build())
    }

    /// `place_order_duration_ms{result}` — the checkout saga end to end.
    pub fn duration(result: &str, elapsed_ms: f64) {
        duration_histogram().record(elapsed_ms, &[KeyValue::new("result", result.to_string())]);
    }

    /// BUSINESS metric: `orders_placed_total{status}`.
    pub fn placed(status: &str) {
        placed_counter().add(1, &[KeyValue::new("status", status.to_string())]);
    }

    /// BUSINESS metric: `checkout_payment_failures_total{reason}`.
    ///
    /// The one number that answers "did we take money and fail to tell anyone" at a glance — the worst
    /// failure mode this product has.
    pub fn payment_failure(reason: &str) {
        payment_failures_counter().add(1, &[KeyValue::new("reason", reason.to_string())]);
    }

    fn degraded_render_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::CHECKOUT_DEGRADED_RENDER_TOTAL).build())
    }

    /// `checkout_degraded_render_total{reason}` (#440) — a DEFECT counter, the
    /// `customer_identification::claim_stamp_failed` pattern: the checkout shell rendered WITHOUT a
    /// mountable payment element, so the customer cannot even try and the place-order contract sees
    /// ZERO runs by construction. `reason` is bounded (the contract's canonical set):
    /// `stripe_key_absent` — emitted today, at the SSR render boundary (the only server-observable
    /// leg); `stripe_js_load_failed` | `mount_threw` — reserved for the client legs, unemitted until
    /// a browser beacon exists (no OTel in WASM). Alert on any sustained non-zero rate. NOT
    /// [`payment_failure`], which counts payments that RAN and failed.
    pub fn degraded_render(reason: &str) {
        degraded_render_counter().add(1, &[KeyValue::new("reason", reason.to_string())]);
    }
}

/// Technical + BAM metrics for the `read-authorization` contract (#144).
pub mod read_authorization {
    use opentelemetry::metrics::Gauge;

    use super::*;

    fn checks_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::READ_AUTHORIZATION_CHECKS_TOTAL).build())
    }

    fn denied_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::READ_AUTHORIZATION_DENIED_TOTAL).build())
    }

    fn public_degraded_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::PUBLIC_CREDENTIAL_DEGRADED_TOTAL).build())
    }

    fn bridge_unresolved_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| {
            meter().u64_counter(metric::READ_AUTHORIZATION_BRIDGE_UNRESOLVED_TOTAL).build()
        })
    }

    fn check_histogram() -> &'static Histogram<f64> {
        static H: OnceLock<Histogram<f64>> = OnceLock::new();
        H.get_or_init(|| {
            meter().f64_histogram(metric::READ_AUTHORIZATION_CHECK_MS).with_unit("ms").build()
        })
    }

    fn lag_gauge() -> &'static Gauge<i64> {
        static G: OnceLock<Gauge<i64>> = OnceLock::new();
        G.get_or_init(|| meter().i64_gauge(metric::SCOPE_MEMBERSHIP_LAG_POSITIONS).build())
    }

    /// One discrete membership check (a by-id read or the subscription guard): counts it, counts the
    /// denial when `!authorized`, and records its latency. The LIST path never calls this — it
    /// enforces via a fused SQL predicate, so a list "denial" is structurally invisible.
    pub fn checked(scope_type: &str, role: &str, authorized: bool, elapsed_ms: f64) {
        let labels = [
            KeyValue::new("scope_type", scope_type.to_string()),
            KeyValue::new("role", role.to_string()),
        ];
        checks_counter().add(1, &labels);
        if !authorized {
            denied_counter().add(1, &labels);
        }
        check_histogram()
            .record(elapsed_ms, &[KeyValue::new("scope_type", scope_type.to_string())]);
    }

    /// An authenticated caller could not be mapped to a domain id (`bridge_resolved=false`). It
    /// denies — safe — but it is a DEFECT (a missing Customer projection row), not a policy outcome,
    /// so it must never sit inside the ordinary denial count where it would look like noise.
    pub fn bridge_unresolved(role: &str) {
        bridge_unresolved_counter().add(1, &[KeyValue::new("role", role.to_string())]);
    }

    /// The OPEN path read a credential and served the request ANONYMOUS anyway (#469): the token
    /// did not verify, the verifier was unavailable, it was not a CUSTOMER token, or it carried no
    /// customer claim. Never an error to the caller — `/public` degrades rather than refusing —
    /// which is precisely why it must be counted: without it, the storefront's identified-cart path
    /// can be down platform-wide and look like nothing at all.
    ///
    /// `claim_absent` is deliberately NOT [`bridge_unresolved`]: that counter means an
    /// authenticated caller was DENIED something for want of a domain binding, and the open path
    /// denies nobody. Routing the pre-claim-stamp window through it would sit an expected rollout
    /// burst inside a provisioning-gap alarm — on every storefront request.
    pub fn public_credential_degraded(reason: &str) {
        public_degraded_counter().add(1, &[KeyValue::new("reason", reason.to_string())]);
    }

    /// BAM gauge: projection lag on the ACL index, in log positions. While it lags, a just-placed
    /// order's own customer is DENIED their order — a user-visible denial, not stale data, which is
    /// why it is watched separately from generic projection lag.
    pub fn lag_positions(lag: i64) {
        lag_gauge().record(lag, &[]);
    }
}

/// Technical metrics for the `cart-price` contract (#451): the read-side pricing seam.
pub mod cart_price {
    use super::*;

    fn duration_histogram() -> &'static Histogram<f64> {
        static H: OnceLock<Histogram<f64>> = OnceLock::new();
        H.get_or_init(|| meter().f64_histogram(metric::CART_PRICE_MS).with_unit("ms").build())
    }

    fn unresolvable_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::CART_PRICE_UNRESOLVABLE_TOTAL).build())
    }

    /// `cart_price_ms` — one priced cart read, resolver seam to priced shape. The budget lever is
    /// the per-request memoized catalog read (`application::pricing::CatalogSnapshot`).
    pub fn duration(elapsed_ms: f64) {
        duration_histogram().record(elapsed_ms, &[]);
    }

    /// `cart_price_unresolvable_total{reason}` — a DEFECT counter (the
    /// `place_order::degraded_render` pattern): the cart's price could not be resolved at read,
    /// so the customer saw NO payable amount (the query errors — never a partial or wrong total)
    /// and a sale is silently lost. `reason` is the contract's canonical bounded set:
    /// `offer_gone` — a line's offer (or a selected option) no longer resolves in the live
    /// catalog (emitted today); `policy_missing` | `stock_unknown` — reserved for the
    /// fee/comparison-policy and orderability legs, unemitted until those land on this seam.
    /// Classification: technical_error, never business_rejected — the customer asked to see
    /// their cart (the write-path twin stays a rejection under place-order). Alert on any
    /// sustained non-zero rate.
    pub fn unresolvable(reason: &str) {
        unresolvable_counter().add(1, &[KeyValue::new("reason", reason.to_string())]);
    }
}

/// Technical metrics for the `customer-identification` contract (#437).
pub mod customer_identification {
    use super::*;

    fn stamp_failed_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::CUSTOMER_CLAIM_STAMP_FAILED_TOTAL).build())
    }

    /// A claim stamp failed (`customer_claim_stamp_failed_total{reason}`) — a DEFECT counter, the
    /// `read_authorization::bridge_unresolved` pattern: never ordinary user error, each one a
    /// customer whose login silently stayed anonymous (verification stood, nothing was parked).
    /// `reason` is bounded: not_configured | claim_conflict | provider_error — `claim_conflict`
    /// stays distinguishable because it is a data defect to INVESTIGATE (an auth user already
    /// bound to a different customer), never retried (mob verdict, #437).
    pub fn claim_stamp_failed(reason: &str) {
        stamp_failed_counter().add(1, &[KeyValue::new("reason", reason.to_string())]);
    }
}

/// The OTP **send** leg of `customer-identification` (#516) — the money half of identity.
///
/// OTLP-only, deliberately: an OTP send emits NO domain event (`actors.yaml` `emits: []`), so there is
/// nothing for a BAM fold to see — and writing a `domain_events` row per refused send would create a
/// log an anonymous attacker gets to drive.
pub mod otp_send {
    use super::*;

    /// Dialing codes we are willing to put in a LABEL. Bounded on purpose and independently of
    /// configuration: the attacker chooses the dialing code, so labelling it verbatim would let them
    /// mint unbounded time series — a metrics-cost incident driven from an anonymous endpoint.
    /// Everything else collapses to `other`, which is all an operator needs; the specific range
    /// belongs on the span. This set MUST match the served default
    /// (`application::sms_guard::DEFAULT_ALLOWED_DIALING_CODES`): a label for a code that cannot be
    /// served is a series that can never move, and a served code without a label is invisible.
    /// Known cost, owed and recorded in ADR-20260813-021500 (#535): every refused `+1` now collapses
    /// into `other`, so the North American refusal cohort is unmeasurable until an
    /// observed-but-not-served label set exists.
    const LABELLED_DIALING_CODES: &[&str] = &["+33", "+32", "+41", "+44", "+49", "+34", "+39"];

    fn dialing_code_label(dialing_code: &str) -> &'static str {
        LABELLED_DIALING_CODES
            .iter()
            .find(|known| **known == dialing_code)
            .copied()
            .unwrap_or("other")
    }

    fn requested_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::OTP_SEND_REQUESTED_TOTAL).build())
    }

    fn refused_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::OTP_SEND_REFUSED_TOTAL).build())
    }

    fn sms_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::SMS_SEND_TOTAL).build())
    }

    /// The guard's last DECLARED state, re-read by the gauge callback on every export cycle.
    /// `-1` = never declared, so nothing is observed at all — an absent series and a `0` series mean
    /// different things and must stay distinguishable.
    static ENFORCING: AtomicI64 = AtomicI64::new(-1);

    /// An **observable** gauge, not a synchronous one — and that is the whole fix.
    ///
    /// A synchronous `Gauge::record` writes one data point when it is called. Called once at
    /// composition, it says "the guard was wired when this process booted" and then says nothing ever
    /// again, so the series goes flat-`1` and STAYS flat-`1` however broken the guard becomes. That
    /// defeats the gauge in precisely the case it exists for: zero refusals is indistinguishable from
    /// a disabled limiter, and a stale `1` is indistinguishable from a live `1`
    /// (ADR-20260810-231300 — a liveness signal must be RE-ASSERTED, or it only proves the process
    /// once booted).
    ///
    /// The callback is invoked by the SDK on every collection cycle, so the value is re-asserted by
    /// construction with no timer of our own, and it stops being emitted the moment the process dies
    /// — a dead-man's switch rather than a threshold. This is the metrics export path, not a poll of
    /// another component's state, so it is outside that ADR's push rule.
    fn enforcing_gauge() -> &'static ObservableGauge<i64> {
        static G: OnceLock<ObservableGauge<i64>> = OnceLock::new();
        G.get_or_init(|| {
            meter()
                .i64_observable_gauge(metric::OTP_SEND_GUARD_ENFORCING)
                .with_callback(|observer| {
                    let state = ENFORCING.load(Ordering::Relaxed);
                    if state >= 0 {
                        observer.observe(state, &[]);
                    }
                })
                .build()
        })
    }

    /// An OTP send was asked for (`otp_send_requested_total{dialing_code, allowed}`).
    ///
    /// ALARM SHAPE: never alert on this rate alone — Friday 19:00-21:30 looks exactly like an attack
    /// by volume. Alert on the RATIO of sends to verifications (an attack moves only the numerator,
    /// because nobody verifies the codes) plus burn of the daily ceiling.
    pub fn requested(dialing_code: &str, allowed: bool) {
        requested_counter().add(
            1,
            &[
                KeyValue::new("dialing_code", dialing_code_label(dialing_code)),
                KeyValue::new("allowed", allowed),
            ],
        );
    }

    /// A send was refused by the guards (`otp_send_refused_total{reason}`). `reason` comes from
    /// `SmsRefusal::reason()`, bounded by construction.
    pub fn refused(reason: &str) {
        refused_counter().add(1, &[KeyValue::new("reason", reason.to_string())]);
    }

    /// THE MONEY SEAM (`sms_send_total{result}`): one per message handed to the OVH sender.
    /// `result`: sent | failed | refused.
    pub fn sms_send(result: &str) {
        sms_counter().add(1, &[KeyValue::new("result", result.to_string())]);
    }

    /// Declare the guard's liveness (`otp_send_guard_enforcing`): 1 while enforcing against the SHARED
    /// counter, 0 when the counter is unreachable and the guard therefore is not enforcing.
    ///
    /// Call it at composition AND wherever enforcement is actually decided — the value is then
    /// re-asserted on every export cycle by [`enforcing_gauge`]'s callback. "No refusals" and "no
    /// limiter" are otherwise the same observation, which is the inverted dead-man's switch
    /// (ADR-20260810-231300).
    pub fn guard_enforcing(enforcing: bool) {
        ENFORCING.store(i64::from(enforcing), Ordering::Relaxed);
        // Registers the callback on first declaration; idempotent afterwards.
        let _ = enforcing_gauge();
    }
}

#[cfg(test)]
mod tests {
    /// Recording against the global no-op provider (no `init`) must not panic. This is the state of
    /// every unit test and every local run without an ingest key, so if it panicked, instrumenting a
    /// boundary would break the test suite and the instrumentation would get reverted rather than fixed.
    #[test]
    fn recording_without_an_initialised_provider_is_a_no_op() {
        super::acceptance::accepted("GRAPHQL");
        super::acceptance::duplicate("WORKER");
        super::acceptance::sync_conflict("PlaceOrder");
        super::acceptance::completed("SUCCEEDED", 12.5);
        super::place_order::duration("captured", 431.0);
        super::place_order::placed("PLACED");
        super::place_order::payment_failure("card_declined");
        super::read_authorization::checked("ORDER", "CUSTOMER", false, 1.5);
        super::read_authorization::bridge_unresolved("CUSTOMER");
        super::read_authorization::lag_positions(0);
        super::customer_identification::claim_stamp_failed("not_configured");
        super::cart_price::duration(3.2);
        super::cart_price::unresolvable("offer_gone");
    }
}
