//! #440: the one emission this chunk declares — `checkout_degraded_render_total{reason=
//! stripe_key_absent}` — proved FIRING through production's own render path (`host_root` →
//! `app_page` → the checkout screen match), never assumed from the constant in `contract.rs`.
//!
//! Its OWN test binary on purpose: `telemetry::meters` binds `opentelemetry::global::meter` once
//! per process (`OnceLock`), so the spy provider must be installed before the process's FIRST
//! metric call — a guarantee the parallel in-crate harness cannot give (any unit test that
//! renders a key-less checkout would bind the meter to the no-op provider first, and this proof
//! would flake with test order). One process, one provider, deterministic.

use axum::extract::Extension;
use axum::http::{header::HOST, HeaderMap, HeaderValue, Uri};
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, SdkMeterProvider};
use server::graphql_schema::build_schema;
use server::{host_root, SsrExec, TenantLookup};

/// Every `checkout_degraded_render_total` data point the spy collected, as `(reason, count)`.
fn degraded_render_points(exporter: &InMemoryMetricExporter) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for rm in exporter.get_finished_metrics().expect("finished metrics") {
        for scope in rm.scope_metrics() {
            for metric in scope.metrics() {
                if metric.name() != "checkout_degraded_render_total" {
                    continue;
                }
                let AggregatedMetrics::U64(MetricData::Sum(sum)) = metric.data() else {
                    panic!("a defect counter aggregates as a u64 Sum: {:?}", metric.data());
                };
                for dp in sum.data_points() {
                    let reason = dp
                        .attributes()
                        .find(|kv| kv.key.as_str() == "reason")
                        .map(|kv| kv.value.to_string())
                        .unwrap_or_default();
                    out.push((reason, dp.value()));
                }
            }
        }
    }
    out.sort();
    out
}

/// One storefront render through the production fallback. `TenantLookup(None)` is the no-database
/// posture (every slug serves its storefront — the dev fail-open), so the page IS the real
/// storefront SSR, checkout shell included.
async fn render(path: &'static str, exec: SsrExec) {
    let mut headers = HeaderMap::new();
    headers.insert(HOST, HeaderValue::from_static("chez-test.captain.food"));
    let _ = host_root(
        Extension(TenantLookup(None)),
        Extension(exec),
        headers,
        Uri::from_static(path),
    )
    .await;
}

/// The render-boundary emission and its GATE, both directions: exactly one count from the
/// key-less CHECKOUT render, with the contract's canonical reason — and NOTHING from a key-less
/// non-checkout render or a checkout render that has its key (the two ways the counter could lie
/// noisy).
#[tokio::test]
async fn a_key_less_checkout_render_emits_the_degrade_counter_with_its_canonical_reason() {
    // The spy provider FIRST — before any render can bind the process-wide meter.
    let exporter = InMemoryMetricExporter::default();
    let provider =
        SdkMeterProvider::builder().with_periodic_exporter(exporter.clone()).build();
    opentelemetry::global::set_meter_provider(provider.clone());

    let schema = build_schema(None, None, None);

    // The degraded render the server can see: checkout, no usable key.
    render("/checkout", SsrExec { schema: schema.clone(), stripe_publishable_key: None }).await;
    // Must NOT count: a key-less render of a DIFFERENT screen…
    render("/cart", SsrExec { schema: schema.clone(), stripe_publishable_key: None }).await;
    // …and a checkout render that has its key (nothing degraded about it).
    render(
        "/checkout",
        SsrExec {
            schema,
            stripe_publishable_key: web::stripe::PublishableKey::parse(Some("pk_test_abc123")),
        },
    )
    .await;

    provider.force_flush().expect("flush the spy reader");
    assert_eq!(
        degraded_render_points(&exporter),
        vec![("stripe_key_absent".to_string(), 1)],
        "exactly ONE emission — the key-less checkout render — with the canonical reason"
    );
}
