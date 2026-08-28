//! #696 — the OTP guard's refusal cohort, made countable for codes we do not serve, without
//! widening the per-code label set the requested-side metric bounds on purpose.
//!
//! ADR-20260813-021500 named the gap directly: dropping bare `+1` from the served allowlist made
//! the North American refusal cohort unmeasurable — every refused `+1` collapses into `other` on
//! `dialing_code_label`, which is the metric that answers "which codes are we SERVING", not "which
//! codes are attacking us". `otp_send_refused_total` now carries a THIRD, hand-declared, 3-value
//! `region` attribute for exactly that question, proved firing here through the real send-guard
//! seam (`SmsSendAuthorizer::authorize`), never assumed from the `region_label` unit tests in
//! `telemetry::meters` alone — those prove the mapping is closed, this proves it reaches the wire.
//!
//! **ONE test, not two** — the `otp_guard_liveness_metric.rs` precedent, and for the identical
//! reason: `opentelemetry::global::set_meter_provider` and `telemetry::meters`'s process-wide
//! `OnceLock`-cached meter/instruments are process-global state. Two separate `#[tokio::test]`
//! functions in one binary run on DIFFERENT THREADS by default, so a second test's
//! `set_meter_provider` call can swap the global provider WHILE the first test's async body is
//! still running — proved the hard way here first: split into two tests, the first refusal
//! (`+1`) silently routed to the SECOND test's provider once it raced ahead and called
//! `set_meter_provider`, so the first test's own exporter read empty and the second read BOTH
//! `north_america` and `rest_of_world` on one flush. Merging into one sequential test removes the
//! race entirely.
//!
//! Own test binary, same reason as `otp_guard_liveness_metric.rs`: `telemetry::meters` binds the
//! process-wide meter once (`OnceLock`), so the spy provider must be installed before this
//! process's first metric call, and a shared `main` binary running ~30 suites cannot guarantee
//! that ordering.

use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, SdkMeterProvider};

/// Every `otp_send_refused_total` data point collected SO FAR, as `(reason, region)` pairs —
/// attributes only, the count is not the property under test here (the counter's arithmetic is
/// proved elsewhere). Cumulative temporality means later flushes still carry earlier data points,
/// which is why each assertion below states the FULL expected set, not just the new arrival.
fn refusal_region_pairs(exporter: &InMemoryMetricExporter) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for rm in exporter.get_finished_metrics().expect("finished metrics") {
        for scope in rm.scope_metrics() {
            for metric in scope.metrics() {
                if metric.name() != "otp_send_refused_total" {
                    continue;
                }
                let AggregatedMetrics::U64(MetricData::Sum(sum)) = metric.data() else {
                    panic!("otp_send_refused_total must aggregate as a u64 Sum: {:?}", metric.data());
                };
                for dp in sum.data_points() {
                    let reason = dp
                        .attributes()
                        .find(|kv| kv.key.as_str() == "reason")
                        .map(|kv| kv.value.to_string())
                        .unwrap_or_default();
                    let region = dp
                        .attributes()
                        .find(|kv| kv.key.as_str() == "region")
                        .map(|kv| kv.value.to_string())
                        .unwrap_or_default();
                    out.push((reason, region));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn authorizer() -> infrastructure::SmsSendAuthorizer {
    infrastructure::SmsSendAuthorizer::new(
        application::sms_guard::SmsSendPolicy::default(),
        Box::new(application::sms_guard::InMemorySmsQuotaStore::default()),
    )
}

async fn refuse(authorizer: &infrastructure::SmsSendAuthorizer, code: &str) {
    let refusal = authorizer
        .authorize(
            &domain::generated::scalars::DialingCode(code.into()),
            &domain::generated::scalars::NationalPhoneNumber("5551234567".into()),
        )
        .await
        .expect_err("an unserved code must be refused");
    assert!(
        matches!(refusal, application::sms_guard::SmsRefusal::CountryNotServed { .. }),
        "expected CountryNotServed for {code}, got {refusal:?}"
    );
}

#[tokio::test]
async fn otp_send_refused_total_carries_a_closed_three_value_region_bucket() {
    let exporter = InMemoryMetricExporter::default();
    let provider = SdkMeterProvider::builder().with_periodic_exporter(exporter.clone()).build();
    opentelemetry::global::set_meter_provider(provider.clone());

    let authorizer = authorizer();

    // PHASE 1 — THE DEFECT ADR-20260813-021500 OWED A FIX FOR: a refused `+1` send must be
    // attributed to `north_america` on the metric an operator would actually alert from, not
    // swallowed into a bucket that also holds every other unserved code in the world.
    refuse(&authorizer, "+1").await;
    provider.force_flush().expect("flush after +1");
    assert_eq!(
        refusal_region_pairs(&exporter),
        vec![("country_not_served".to_string(), "north_america".to_string())],
        "a +1 refusal must land in the north_america bucket -- if this reads rest_of_world (or \
         'other'), the bucket mapping regressed to the pre-#696 collapse ADR-20260813-021500 named"
    );

    // PHASE 2 — THE CLOSED-SET PROPERTY, proved through the wire rather than only unit-tested
    // against `region_label`: several codes that are NEITHER served NOR in the
    // North-America / non-EU-Europe tables must all land on the SAME `rest_of_world` value --
    // proving the fallback is one shared constant and not a per-code mint, which is the
    // attacker-cardinality guarantee the whole table exists for.
    for code in ["+998", "+212", "+61"] {
        refuse(&authorizer, code).await;
    }
    provider.force_flush().expect("flush after the rest_of_world codes");
    assert_eq!(
        refusal_region_pairs(&exporter),
        vec![
            ("country_not_served".to_string(), "north_america".to_string()),
            ("country_not_served".to_string(), "rest_of_world".to_string()),
        ],
        "three distinct never-declared codes must collapse onto ONE additional data point \
         sharing the SAME region value alongside the earlier north_america point -- a mutant that \
         mints a per-code label instead of the shared constant would instead produce three more \
         distinct series here"
    );

    // PHASE 3 — a served-but-refused-for-another-reason code (`+41`, cooldown after the first
    // send) stays OUT of `non_eu_europe`: a served code is never reclassified by this table.
    // `authorize` claims the budget on success, so the SAME number refused twice in a row hits
    // the cooldown branch, not CountryNotServed -- proving the region bucket is populated for
    // EVERY refusal reason, not only country_not_served.
    let served = domain::generated::scalars::DialingCode("+41".into());
    let number = domain::generated::scalars::NationalPhoneNumber("791234567".into());
    authorizer.authorize(&served, &number).await.expect("the first +41 send is allowed");
    let cooldown_refusal = authorizer
        .authorize(&served, &number)
        .await
        .expect_err("the immediate resend must hit the cooldown");
    assert!(
        matches!(cooldown_refusal, application::sms_guard::SmsRefusal::TooSoon { .. }),
        "expected a cooldown refusal, got {cooldown_refusal:?}"
    );
    provider.force_flush().expect("flush after the served-code cooldown refusal");
    assert_eq!(
        refusal_region_pairs(&exporter),
        vec![
            ("cooldown".to_string(), "rest_of_world".to_string()),
            ("country_not_served".to_string(), "north_america".to_string()),
            ("country_not_served".to_string(), "rest_of_world".to_string()),
        ],
        "a cooldown refusal on the SERVED +41 number must bucket as rest_of_world (never \
         non_eu_europe, which is reserved for codes we do NOT serve) -- proving region is \
         populated for every refusal reason, not only country_not_served"
    );
}
