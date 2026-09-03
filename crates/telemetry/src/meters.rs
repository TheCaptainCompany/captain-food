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

    fn fact_unrecorded_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::MAILBOX_FACT_UNRECORDED_TOTAL).build())
    }

    /// The CLOSED `reason` set of `mailbox_fact_unrecorded_total`, spelled once
    /// (`specs/observability.yaml#/mailbox-delivery`). Constants rather than call-site literals so
    /// a typo cannot silently open a third series that no dashboard is watching.
    ///
    /// The fact is DECLARED and the receiving aggregate has no fold rule for it, so the delivery
    /// PARKED. Permanently zero in production.
    pub const FACT_UNRECORDED_DEFERRED: &str = "deferred";
    /// A DECLARED fact whose payload does not deserialize, or whose staged `eventType` disagrees
    /// with the row's `message_type`. Deterministic, so the row is terminally failed.
    pub const FACT_UNRECORDED_UNPARSABLE: &str = "unparsable_payload";

    /// A declared inbound fact did NOT reach its aggregate
    /// (`mailbox_fact_unrecorded_total{actor_type, message_type, reason}`).
    ///
    /// `message_type` is a label ON PURPOSE: it answers *which fact*, which is the entire question,
    /// and its population is bounded by that lane's declared `receives:` set. The row's
    /// `message_id`/`correlation_id` are NOT labels -- they ride the `message.deliver` span.
    pub fn fact_unrecorded(actor_type: &str, message_type: &str, reason: &'static str) {
        fact_unrecorded_counter().add(
            1,
            &[
                KeyValue::new("actor_type", actor_type.to_string()),
                KeyValue::new("message_type", message_type.to_string()),
                KeyValue::new("reason", reason),
            ],
        );
    }

    /// The push listener went DOWN (`mailbox_push_down_total{reason}`): connection lost, or the
    /// liveness canary timed out (the transaction-pooler silently-deaf-LISTEN mode). Workers are
    /// on the pre-push cadence until recovery.
    pub fn push_down(reason: &str) {
        push_down_counter().add(1, &[KeyValue::new("reason", reason.to_string())]);
    }

    fn promotion_due_lag_histogram() -> &'static Histogram<f64> {
        static H: OnceLock<Histogram<f64>> = OnceLock::new();
        H.get_or_init(|| {
            meter().f64_histogram(metric::REMINDER_PROMOTION_DUE_LAG_MS).with_unit("ms").build()
        })
    }

    fn scheduled_depth_gauge() -> &'static Gauge<i64> {
        static G: OnceLock<Gauge<i64>> = OnceLock::new();
        G.get_or_init(|| meter().i64_gauge(metric::MAILBOX_SCHEDULED_DEPTH).build())
    }

    /// The promotion machinery's DEAD-MAN'S SWITCH (`reminder_promotion_due_lag_ms{actor_type}`,
    /// #167 / ADR-20260810-231300): recorded on EVERY watch tick, 0 included — a monitor that
    /// only fires when a reminder arrives goes quiet exactly when it should scream. A growing
    /// value = the promotion pass is dead while due reminders pile up; the series STOPPING = the
    /// watcher itself died. Emitted by the promotion watch (a sampler OUTSIDE the worker, so a
    /// dead worker cannot silence its own alarm — the ADR's monitoring carve-out).
    pub fn promotion_due_lag(actor_type: &str, lag_ms: f64) {
        promotion_due_lag_histogram()
            .record(lag_ms, &[KeyValue::new("actor_type", actor_type.to_string())]);
    }

    /// SCHEDULED row depth by (actor_type, purpose) (`mailbox_scheduled_depth`, the #167 dba
    /// gauge): the reminder table's cardinality watch — V0 expectation is single digits, and a
    /// runaway here is a scheduling leak, not load.
    pub fn scheduled_depth(actor_type: &str, purpose: &str, depth: i64) {
        scheduled_depth_gauge().record(
            depth,
            &[
                KeyValue::new("actor_type", actor_type.to_string()),
                KeyValue::new("purpose", purpose.to_string()),
            ],
        );
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

    fn birth_lag_histogram() -> &'static Histogram<f64> {
        static H: OnceLock<Histogram<f64>> = OnceLock::new();
        H.get_or_init(|| {
            meter().f64_histogram(metric::ORDER_BIRTH_LAG_MS).with_unit("ms").build()
        })
    }

    /// `order_birth_lag_ms{routed}` (#588) — enqueue to `Recorded`, the handover the routed birth
    /// introduces. Recorded by the Order lane's delivery, at the framework boundary, from the
    /// mailbox row's own `received_at`: the row that carries the birth IS the enqueue instant, so
    /// this needs no clock the domain can see. A GROWING p95 is the acceptance clock arming late.
    pub fn birth_lag(routed: bool, elapsed_ms: f64) {
        birth_lag_histogram()
            .record(elapsed_ms, &[KeyValue::new("routed", routed.to_string())]);
    }

    fn lane_heartbeat_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::ORDER_LANE_WATCH_HEARTBEAT_TOTAL).build())
    }

    fn lane_oldest_pending_gauge() -> &'static Gauge<i64> {
        static G: OnceLock<Gauge<i64>> = OnceLock::new();
        G.get_or_init(|| meter().i64_gauge(metric::ORDER_LANE_OLDEST_PENDING_AGE_MS).build())
    }

    /// The Order lane's DEAD-MAN'S SWITCH, one tick (#598): `order_lane_watch_heartbeat_total`
    /// increments and `order_lane_oldest_pending_age_ms` re-reports, for EVERY declared
    /// routed-birth lane, on EVERY tick, whatever `ROUTE_ORDER_BIRTH_THROUGH_LANE` says.
    ///
    /// Emitted as ONE fn because the two series are one fact: the gauge is only readable while the
    /// counter proves the reporter alive, and a tick that emitted one without the other would be
    /// a monitor whose two halves can disagree. Alert on the ABSENCE OF AN INCREMENT — never a
    /// threshold: [`birth_lag`] is silent by design while the flag is OFF, so without this pair
    /// "flag off" and "the Order lane worker is dead" are the same observation.
    ///
    /// Never a zero-seeding of [`birth_lag`] itself: injected zeros would poison the p95 the flip
    /// is judged on.
    pub fn order_lane_watch(lane: &str, oldest_pending_age_ms: i64) {
        let labels = [KeyValue::new("lane", lane.to_string())];
        lane_heartbeat_counter().add(1, &labels);
        lane_oldest_pending_gauge().record(oldest_pending_age_ms, &labels);
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

/// DEPLOY-TIME FLEET PARITY (#598, farley): `runtime_flag_state{flag,value,bin}`.
///
/// The problem it solves is not "is the flag on" — a reviewer can read that. It is that a rolling
/// deploy runs SEVERAL processes at once (the monolith, standalone adapter fleets, per-actor bins)
/// and, while it is in flight, they can resolve the SAME flag to DIFFERENT values. For
/// `ROUTE_ORDER_BIRTH_THROUGH_LANE` that split is invisible in every other series: both halves
/// birth exactly one order (four absorbers make double-birth unreachable), but only the routed
/// half arms the acceptance deadline — so a split fleet yields a coin-flip on the deadline, per
/// order, and the only histogram that could see it (`order_birth_lag_ms`) records nothing on the
/// unrouted path. Review-time parity is an assertion; this is EVIDENCE, and
/// `count(distinct value) by (flag) > 1` is a condition a flip can be BLOCKED on.
///
/// An OBSERVABLE gauge, for the reason `otp_send::enforcing_gauge` documents at length: a value
/// written once at boot says "this process once started" and then says nothing however wrong the
/// fleet becomes. The callback re-asserts every declaration on every export cycle, so a process
/// that dies stops contributing its value with no timer of our own.
pub mod runtime {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use super::*;

    /// `(flag, bin)` → the resolved value, re-read by the gauge callback on every export cycle.
    fn declared() -> &'static Mutex<BTreeMap<(String, String), bool>> {
        static D: OnceLock<Mutex<BTreeMap<(String, String), bool>>> = OnceLock::new();
        D.get_or_init(|| Mutex::new(BTreeMap::new()))
    }

    fn flag_state_gauge() -> &'static ObservableGauge<i64> {
        static G: OnceLock<ObservableGauge<i64>> = OnceLock::new();
        G.get_or_init(|| {
            meter()
                .i64_observable_gauge(metric::RUNTIME_FLAG_STATE)
                .with_callback(|observer| {
                    // Poisoned is not a reason to go silent: a monitor that stops reporting
                    // because one writer panicked reads exactly like the fleet being gone.
                    let d = declared().lock().unwrap_or_else(|e| e.into_inner());
                    for ((flag, bin), value) in d.iter() {
                        observer.observe(
                            1,
                            &[
                                KeyValue::new("flag", flag.clone()),
                                KeyValue::new("value", value.to_string()),
                                KeyValue::new("bin", bin.clone()),
                            ],
                        );
                    }
                })
                .build()
        })
    }

    /// This process's binary name — the `bin` label. Derived, never passed: a caller that has to
    /// name itself eventually names itself wrong, and one deploy runs several binaries whose
    /// DISAGREEMENT is the whole hazard.
    pub fn current_bin() -> String {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// Declare this process's RESOLVED value for one runtime flag. Call at the composition root,
    /// where the value is decided — the callback re-asserts it from there on.
    ///
    /// The value is a LABEL, not the measurement: the gauge always observes `1`, so
    /// `count(distinct value) by (flag)` is a straight parity query and a flag nobody declares is
    /// an absent series rather than a `0` that reads as "false".
    pub fn declare_flag(flag: &str, value: bool) {
        declared()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert((flag.to_string(), current_bin()), value);
        // Registers the callback on first declaration; idempotent afterwards.
        let _ = flag_state_gauge();
    }
}

/// The `payment-settlement` contract (ADR-20260808-195315 §1.2/§1.3): capture on fulfilment,
/// release on abort. Emitted at the Stripe ADAPTER boundary (the same seam as
/// [`place_order::payment_failure`]) — application saga code stays telemetry-free.
pub mod payment_settlement {
    use super::*;

    fn capture_failed_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::PAYMENT_CAPTURE_FAILED_TOTAL).build())
    }

    fn release_failed_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::PAYMENT_RELEASE_FAILED_TOTAL).build())
    }

    /// `payment_capture_failed_total{reason}` — PAGES on ANY increment: food cooked, money did not
    /// move. `reason` is the typed scalars.yaml#/CaptureFailureReason set; the recorded
    /// PaymentCaptureFailed fact on the Payment stream is this counter's durable twin.
    pub fn capture_failed(reason: &str) {
        capture_failed_counter().add(1, &[KeyValue::new("reason", reason.to_string())]);
    }

    /// `payment_release_failed_total{reason}` — warn-level, never a page: a failed void self-heals
    /// (the authorization expires within ~7 days and Stripe reports PaymentReleased).
    pub fn release_failed(reason: &str) {
        release_failed_counter().add(1, &[KeyValue::new("reason", reason.to_string())]);
    }
}

/// THE BIRTH-GAP DEAD-MAN'S SWITCH (#608): *is a customer's money held right now with no order
/// behind it, and for how long?*
///
/// Emitted by `infrastructure::mailbox::birth_gap_watch_tick`, a MONITOR on its own clock outside
/// every worker it watches (ADR-20260810-231300's monitoring carve-out — for a monitor, silence is
/// ambiguous, so it keeps a poll permanently and with no exit).
///
/// The three functions below are separate on purpose, unlike `place_order::order_lane_watch`'s
/// fused pair: the two gauges have DIFFERENT populations (never-born vs born-never-captured) and
/// the heartbeat must be able to lag both — it certifies a COMPLETE sweep, so the sweep calls it
/// last, after every gauge has reported.
pub mod birth_gap {
    use super::*;

    /// The DECLARED bounded label set, mirroring `specs/observability.yaml`'s
    /// `payment_authorized_no_order_birth_age_seconds.attribute_values.reason`.
    ///
    /// It is a constant rather than three call sites because the contract is "every member reports
    /// on every tick, 0 included": a sweep that iterates this list cannot forget a member, and a
    /// member that stops reporting is the exact silence the switch exists to abolish. Sorted, so
    /// an equality assertion over an emitted point set is order-stable.
    pub const REASONS: [&str; 3] = ["delivery_exhausted", "no_run", "retry_pending"];

    fn no_order_birth_gauge() -> &'static Gauge<i64> {
        static G: OnceLock<Gauge<i64>> = OnceLock::new();
        G.get_or_init(|| {
            meter()
                .i64_gauge(metric::PAYMENT_AUTHORIZED_NO_ORDER_BIRTH_AGE_SECONDS)
                .with_unit("s")
                .build()
        })
    }

    fn unsettled_gauge() -> &'static Gauge<i64> {
        static G: OnceLock<Gauge<i64>> = OnceLock::new();
        G.get_or_init(|| {
            meter()
                .i64_gauge(metric::PAYMENT_AUTHORIZED_UNSETTLED_AGE_SECONDS)
                .with_unit("s")
                .build()
        })
    }

    fn sweep_heartbeat_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::PAYMENT_BIRTH_GAP_SWEEP_HEARTBEAT_TOTAL).build())
    }

    /// `payment_authorized_no_order_birth_age_seconds{reason}` — the age of the OLDEST stranded
    /// authorization in that class, **0 when the class is empty**. Call it for EVERY member of
    /// [`REASONS`] on every tick; an early return on an empty population is the defect
    /// ADR-20260810-231300 names, one level down.
    pub fn no_order_birth_age(reason: &str, age_seconds: i64) {
        no_order_birth_gauge()
            .record(age_seconds, &[KeyValue::new("reason", reason.to_string())]);
    }

    /// `payment_authorized_unsettled_age_seconds` — the age of the oldest BORN order still
    /// AUTHORIZED (never captured). Declared since ADR-20260808-195315 and emitted by nothing until
    /// #608; it rides this sweep because it is the same question one hop later, and a second
    /// declared-but-silent money-path contract is what this chunk exists to stop shipping.
    pub fn authorized_unsettled_age(age_seconds: i64) {
        unsettled_gauge().record(age_seconds, &[]);
    }

    /// `payment_birth_gap_sweep_heartbeat_total` — one COMPLETED sweep. Called LAST, so it can
    /// never certify a pass that failed halfway. Alert on the absence of an increment.
    pub fn sweep_completed() {
        sweep_heartbeat_counter().add(1, &[]);
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

    fn sdui_degraded_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::SDUI_DEGRADED_RENDER_TOTAL).build())
    }

    /// `sdui_degraded_render_total{screen, resolver, reason, correlation_id}` (#472) — a DEFECT
    /// counter, the `checkout_degraded_render_total` pattern at the SSR render boundary: a page
    /// whose declared read FAILED for real (or whose declared condition expression could not be
    /// parsed) shipped its degraded/error state. A role-refused read on the anonymous SSR
    /// transport is a SKIP BY DESIGN and is never counted. `reason` is the contract's bounded set
    /// (`resolver_failed` | `condition_unparseable`; `client_*` legs reserved, no OTel in WASM);
    /// `correlation_id` is the render's own id so a degraded page joins its read-path spans.
    /// Emitted from `server::hosts` (the framework boundary) — `web` compiles to wasm and stays
    /// telemetry-free. Alert on any sustained non-zero rate.
    pub fn sdui_degraded_render(screen: &str, resolver: &str, reason: &str, correlation_id: &str) {
        sdui_degraded_counter().add(
            1,
            &[
                KeyValue::new("screen", screen.to_string()),
                KeyValue::new("resolver", resolver.to_string()),
                KeyValue::new("reason", reason.to_string()),
                KeyValue::new("correlation_id", correlation_id.to_string()),
            ],
        );
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

/// Technical metrics for the `customer-identity` contract (#641, IDENT-1 Phase A): the
/// request-seam CUSTOMER identity resolution (`RESOLVE_CUSTOMER_IDENTITY_FROM_POSTGRES`).
pub mod customer_identity {
    use super::*;

    fn resolve_histogram() -> &'static Histogram<f64> {
        static H: OnceLock<Histogram<f64>> = OnceLock::new();
        H.get_or_init(|| {
            meter().f64_histogram(metric::CUSTOMER_IDENTITY_RESOLVE_MS).with_unit("ms").build()
        })
    }

    fn not_found_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::CUSTOMER_IDENTITY_NOT_FOUND_TOTAL).build())
    }

    fn lookup_failed_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::CUSTOMER_IDENTITY_LOOKUP_FAILED_TOTAL).build())
    }

    fn lookup_source_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::CUSTOMER_IDENTITY_LOOKUP_SOURCE_TOTAL).build())
    }

    /// `customer_identity_resolve_ms{result}` — one Postgres-mode CUSTOMER resolution.
    pub fn duration(elapsed_ms: f64, result: &str) {
        resolve_histogram().record(elapsed_ms, &[KeyValue::new("result", result.to_string())]);
    }

    /// `customer_identity_not_found_total` — the verified subject carries no mapping row (yet): an
    /// ORDINARY provisioning gap, fails closed to Public. OBSERVE, never PAGE.
    pub fn not_found() {
        not_found_counter().add(1, &[]);
    }

    /// `customer_identity_lookup_failed_total{reason}` — the seam itself could not be asked, a
    /// DEFECT distinct from [`not_found`] because the operator response is the OPPOSITE one: PAGE
    /// on any sustained non-zero rate.
    pub fn lookup_failed(reason: &str) {
        lookup_failed_counter().add(1, &[KeyValue::new("reason", reason.to_string())]);
    }

    /// `customer_identity_lookup_source_total{source}` (`db` | `request_reuse`) — so request-scoped
    /// reuse can never hide an outage behind a cache hit.
    pub fn lookup_source(source: &str) {
        lookup_source_counter().add(1, &[KeyValue::new("source", source.to_string())]);
    }
}

/// Technical metrics for the `rider-identity` contract (#639 part C step 2b): the request-seam
/// RIDER identity resolution — ungated, Postgres on every request.
pub mod rider_identity {
    use super::*;

    fn resolve_histogram() -> &'static Histogram<f64> {
        static H: OnceLock<Histogram<f64>> = OnceLock::new();
        H.get_or_init(|| {
            meter().f64_histogram(metric::RIDER_IDENTITY_RESOLVE_MS).with_unit("ms").build()
        })
    }

    fn not_found_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::RIDER_IDENTITY_NOT_FOUND_TOTAL).build())
    }

    fn lookup_failed_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::RIDER_IDENTITY_LOOKUP_FAILED_TOTAL).build())
    }

    fn lookup_source_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::RIDER_IDENTITY_LOOKUP_SOURCE_TOTAL).build())
    }

    /// `rider_identity_resolve_ms{result}` — one RIDER resolution.
    pub fn duration(elapsed_ms: f64, result: &str) {
        resolve_histogram().record(elapsed_ms, &[KeyValue::new("result", result.to_string())]);
    }

    /// `rider_identity_not_found_total` — no `rider` row for the verified subject. OBSERVE.
    pub fn not_found() {
        not_found_counter().add(1, &[]);
    }

    /// `rider_identity_lookup_failed_total{reason}` — the seam could not be asked. PAGE.
    pub fn lookup_failed(reason: &str) {
        lookup_failed_counter().add(1, &[KeyValue::new("reason", reason.to_string())]);
    }

    /// `rider_identity_lookup_source_total{source}` (`db` | `request_reuse`).
    pub fn lookup_source(source: &str) {
        lookup_source_counter().add(1, &[KeyValue::new("source", source.to_string())]);
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

    /// The refusal-cohort fix OWED in ADR-20260813-021500 (#535, #696): a HAND-DECLARED business
    /// segmentation, not ITU/geographic data — it exists only to make refused-but-unserved codes
    /// countable without widening `dialing_code_label`'s per-code label set (which stays bounded to
    /// what we SERVE, `LABELLED_DIALING_CODES` above, untouched by this table).
    ///
    /// `north_america` = `+1` and `+52`. `+1` is the code the ADR names directly (NANP's
    /// premium-payout ranges billed as if they were a Boston number). `+52` (Mexico) is a DELIBERATE
    /// CHOICE, not an omission: Mexican mobile termination is commonly bundled into the same
    /// SMS-pumping campaigns the ADR's `+1` finding is about, and a reader must not have to guess
    /// whether it was considered.
    ///
    /// `non_eu_europe` = European calling codes NOT already in `LABELLED_DIALING_CODES` above: `+7`
    /// (Russia/Kazakhstan share the code), `+380` (Ukraine), `+90` (Turkey). Checked against the
    /// served list before this table was written: `+41` (Switzerland) IS in `LABELLED_DIALING_CODES`,
    /// so it is deliberately absent here — a served code keeps its per-code label on the REQUESTED
    /// side and falls through to `rest_of_world` here, which is fine: this table exists to bucket the
    /// codes we do NOT serve, not to re-describe the ones we do.
    ///
    /// Everything else — the true default — is `rest_of_world`.
    ///
    /// ATTACKER-CARDINALITY PROPERTY: whatever code arrives, this function returns ONE of exactly
    /// three values. An attacker rotating dialing codes cannot mint a fourth label, however many
    /// distinct codes they try — the same bound `dialing_code_label` gives the requested-side metric,
    /// extended to refusals without reopening the cardinality risk that bounded it in the first place.
    const NORTH_AMERICA_DIALING_CODES: &[&str] = &["+1", "+52"];
    const NON_EU_EUROPE_DIALING_CODES: &[&str] = &["+7", "+380", "+90"];

    fn region_label(dialing_code: &str) -> &'static str {
        if NORTH_AMERICA_DIALING_CODES.contains(&dialing_code) {
            "north_america"
        } else if NON_EU_EUROPE_DIALING_CODES.contains(&dialing_code) {
            "non_eu_europe"
        } else {
            "rest_of_world"
        }
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

    /// A send was refused by the guards (`otp_send_refused_total{reason, region}`). `reason` comes
    /// from `SmsRefusal::reason()`, bounded by construction. `region` is [`region_label`] applied to
    /// the dialing code the refusal was decided against (#696) — every refusal has one at the call
    /// site (`SmsSendAuthorizer::authorize`/`peek`/`authorize_e164` all hold the code that was being
    /// evaluated, even when the `SmsRefusal` variant itself does not carry it, e.g. a quota refusal on
    /// an already-served number), so the label is populated for every reason, not only
    /// `country_not_served`.
    pub fn refused(reason: &str, dialing_code: &str) {
        refused_counter().add(
            1,
            &[
                KeyValue::new("reason", reason.to_string()),
                KeyValue::new("region", region_label(dialing_code)),
            ],
        );
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

    #[cfg(test)]
    mod region_label_tests {
        use super::region_label;

        /// The #696 mutant: "the bucket mapping is deleted, everything collapses to `rest_of_world`
        /// again" — the exact shape ADR-20260813-021500 owed a fix for on the REQUESTED side
        /// (`dialing_code_label` collapsing `+1` into `other`). If the `north_america` arm is removed,
        /// this is the assertion that reds.
        #[test]
        fn a_north_american_code_lands_in_north_america() {
            assert_eq!(region_label("+1"), "north_america");
            assert_eq!(region_label("+52"), "north_america", "Mexico is a declared, deliberate member");
        }

        #[test]
        fn a_non_eu_european_code_lands_in_non_eu_europe() {
            assert_eq!(region_label("+7"), "non_eu_europe");
            assert_eq!(region_label("+380"), "non_eu_europe");
            assert_eq!(region_label("+90"), "non_eu_europe");
        }

        /// `+41` is IN `LABELLED_DIALING_CODES` (Switzerland is served) and must NOT be reclassified
        /// as `non_eu_europe` by this table — that would silently duplicate the served label with a
        /// second, contradictory bucket for the same code.
        #[test]
        fn a_served_code_is_not_reclassified_as_non_eu_europe() {
            assert_eq!(region_label("+41"), "rest_of_world");
            assert_eq!(region_label("+33"), "rest_of_world");
        }

        /// The #696 mutant: "a code outside every table mints its own label" (e.g. formatting the
        /// unmatched code into the returned string instead of returning the shared constant). Several
        /// UNRELATED, never-declared codes must all collapse onto the SAME `rest_of_world` value —
        /// proving the fallback is a closed constant, not a per-code mint — and every value produced,
        /// across every case in this file, must be one of exactly the three declared buckets.
        #[test]
        fn an_unlabelled_code_never_mints_its_own_label() {
            let never_declared = ["+999", "+888", "+212", "+61", "+81", "+combined-nonsense"];
            for code in never_declared {
                assert_eq!(
                    region_label(code),
                    "rest_of_world",
                    "code {code} must collapse to the shared rest_of_world constant, not a label of its own"
                );
            }
            let closed_set = ["north_america", "non_eu_europe", "rest_of_world"];
            for code in ["+1", "+52", "+7", "+380", "+90", "+41", "+999", "+212"] {
                assert!(
                    closed_set.contains(&region_label(code)),
                    "region_label({code}) = {} is outside the declared 3-value set",
                    region_label(code)
                );
            }
        }
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
        super::place_order::duration("authorized", 431.0);
        super::place_order::placed("PLACED");
        super::place_order::payment_failure("card_declined");
        super::payment_settlement::capture_failed("CARD_DECLINED");
        super::payment_settlement::release_failed("transient");
        super::read_authorization::checked("ORDER", "CUSTOMER", false, 1.5);
        super::read_authorization::bridge_unresolved("CUSTOMER");
        super::read_authorization::lag_positions(0);
        super::customer_identification::claim_stamp_failed("not_configured");
        super::cart_price::duration(3.2);
        super::cart_price::unresolvable("offer_gone");
    }
}
