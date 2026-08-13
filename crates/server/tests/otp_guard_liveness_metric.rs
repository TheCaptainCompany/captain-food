//! `otp_send_guard_enforcing` must be a HEARTBEAT, not a boot stamp (#516 review finding 3;
//! ADR-20260810-231300's inverted dead-man's switch).
//!
//! Why this test exists at all: the whole reason the gauge is declared is that **zero refusals reads
//! identically to a disabled limiter**. A synchronous `Gauge::record` called once at composition
//! satisfies "the metric exists" while proving only that the process once booted — the series goes
//! flat-`1` and stays flat-`1` however broken the guard becomes, which defeats the gauge in exactly
//! the case it was added for. The property that has to hold is therefore not "a point was emitted"
//! but "a point is emitted **again**, on every collection cycle, and it carries the CURRENT state".
//!
//! **What each phase actually proves, stated honestly.** Phases 1-3 pin the exported CONTRACT: absent
//! until declared, a point on every collection, and the current value rather than a replay. They do
//! NOT by themselves discriminate a synchronous gauge from an observable one — under the cumulative
//! temporality this exporter uses, a synchronous `record` is also re-exported each cycle. Verified,
//! not assumed: reverting to the boot-stamp implementation left phases 1-3 GREEN, which is exactly
//! the vacuous-gate trap, so the discriminating assertion is phase 4.
//!
//! **Phase 4 is the one that fails against the defect**: the gauge was only ever set `true`, at
//! composition, and nothing ever set it `false`. So a guard whose shared counter had gone away kept
//! reporting `1` forever — "enforcing", while enforcing nothing. Phase 4 drives a real
//! `SmsSendAuthorizer` whose store is unreachable and asserts the gauge follows it down.

use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, SdkMeterProvider};

/// Every value `otp_send_guard_enforcing` carried, in collection order, flattened across flushes.
fn enforcing_points(exporter: &InMemoryMetricExporter) -> Vec<i64> {
    let mut out = Vec::new();
    for rm in exporter.get_finished_metrics().expect("finished metrics") {
        for sm in rm.scope_metrics() {
            for m in sm.metrics() {
                if m.name() != "otp_send_guard_enforcing" {
                    continue;
                }
                let AggregatedMetrics::I64(MetricData::Gauge(g)) = m.data() else {
                    panic!("otp_send_guard_enforcing must be an i64 gauge, got {:?}", m.data());
                };
                for dp in g.data_points() {
                    out.push(dp.value());
                }
            }
        }
    }
    out
}

/// ONE test, not three: both the process-wide `ENFORCING` state and the global meter provider are
/// process-global, so separate `#[test]`s in one binary would race each other and the "undeclared"
/// case would depend on which test ran first. The phases below are therefore ordered inside a single
/// test, cheapest-precondition first.
#[test]
fn the_gauge_is_a_heartbeat_and_absent_until_declared() {
    let exporter = InMemoryMetricExporter::default();
    let provider = SdkMeterProvider::builder().with_periodic_exporter(exporter.clone()).build();
    opentelemetry::global::set_meter_provider(provider.clone());

    // PHASE 1 — undeclared. An ABSENT series and a `0` series mean different things: "this binary has
    // no OTP guard wired" versus "the guard is wired and currently degraded". A gauge defaulting to 0
    // would make every unrelated binary look like a broken limiter, which is how a real alert gets
    // muted. Touch a sibling meter so the module is initialised but the guard is not declared.
    telemetry::meters::otp_send::requested("+33", true);
    provider.force_flush().expect("collection");
    assert!(
        enforcing_points(&exporter).is_empty(),
        "no declaration ⇒ no data point; absent must stay distinguishable from 0"
    );

    // PHASE 2 — the heartbeat. Composition declares it ONCE; this is the only call a boot-stamp
    // implementation would ever make.
    telemetry::meters::otp_send::guard_enforcing(true);
    provider.force_flush().expect("first collection");
    provider.force_flush().expect("second collection");
    provider.force_flush().expect("third collection");
    assert_eq!(
        enforcing_points(&exporter),
        vec![1, 1, 1],
        "ONE declaration must produce a point on EVERY collection — a boot-time `record` gives [1] \
         here, and a stale `1` is indistinguishable from a live one"
    );

    // PHASE 3 — the degraded transition. The shared counter is unreachable, so the guard is not
    // enforcing. Nothing re-declares `true`, and the gauge must not drift back to it on its own.
    telemetry::meters::otp_send::guard_enforcing(false);
    provider.force_flush().expect("fourth collection");
    provider.force_flush().expect("fifth collection");
    assert_eq!(
        enforcing_points(&exporter),
        vec![1, 1, 1, 0, 0],
        "the callback must report the CURRENT declared state on each cycle, and keep reporting 0"
    );

    // PHASE 4 — THE DISCRIMINATING ONE. Everything above passes against the boot-stamp gauge too.
    // What that implementation could not do is notice: `guard_enforcing(true)` at composition was the
    // only call in the process, so a guard whose counter had vanished reported `1` indefinitely.
    // Enforcement must re-declare the state where it is actually decided.
    telemetry::meters::otp_send::guard_enforcing(true); // back to a healthy claim
    provider.force_flush().expect("sixth collection");

    let authorizer = infrastructure::SmsSendAuthorizer::new(
        application::sms_guard::SmsSendPolicy::default(),
        Box::new(UnreachableStore),
    );
    let refusal = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime")
        .block_on(authorizer.authorize_e164("+33612345678"))
        .expect_err("an unreachable counter must refuse, fail-closed");
    assert!(
        matches!(refusal, application::sms_guard::SmsRefusal::StoreUnavailable { .. }),
        "the store outage must surface as StoreUnavailable, got {refusal:?}"
    );

    provider.force_flush().expect("seventh collection");
    assert_eq!(
        enforcing_points(&exporter),
        vec![1, 1, 1, 0, 0, 1, 0],
        "a send refused because the SHARED counter is unreachable must drop the gauge to 0 — the \
         guard is not enforcing, and a stale `1` here is the exact blindness the gauge exists to \
         remove"
    );
}

/// A quota store that is always down — the outage phase 4 needs, with no database involved.
#[derive(Debug)]
struct UnreachableStore;

#[async_trait::async_trait]
impl application::sms_guard::SmsQuotaStore for UnreachableStore {
    async fn try_claim(
        &self,
        _claim: &application::sms_guard::QuotaClaim,
        _now_unix: i64,
    ) -> Result<application::sms_guard::QuotaGrant, application::sms_guard::QuotaDenial> {
        Err(application::sms_guard::QuotaDenial::Unavailable {
            detail: "connection refused".into(),
        })
    }

    async fn peek(
        &self,
        _claim: &application::sms_guard::QuotaClaim,
        _now_unix: i64,
    ) -> Result<(), application::sms_guard::QuotaDenial> {
        Err(application::sms_guard::QuotaDenial::Unavailable {
            detail: "connection refused".into(),
        })
    }
}
