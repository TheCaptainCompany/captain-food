//! **P7** — `read_authorization_scope_mismatch_total` fires for the population it names and stays
//! silent for the one it does not (#618).
//!
//! **Its OWN test binary**, for the reason `checkout_degraded_metric.rs` and
//! `public_credential_degraded_metric.rs` both state: `telemetry::meters` binds
//! `opentelemetry::global::meter` once per process (`OnceLock`), so the spy provider must be
//! installed before the process's FIRST metric call — a guarantee the parallel in-crate harness
//! cannot give.
//!
//! **And it holds EXACTLY ONE `#[test]`**, for the reason `otp_guard_liveness_metric.rs:53-57`
//! states: the global meter provider is process-global, so separate `#[test]`s in one binary would
//! race each other and the counts would depend on which ran first. Two tests here is a coin-flip
//! red on a `HOLD: human` PR that a human is watching. The two arms are two assertions inside one
//! test fn.
//!
//! **Both ways, not one.** `public_credential_degraded_metric.rs` states the reason in its own doc
//! comment: an "it stays zero" assertion whose metric name is simply wrong passes VACUOUSLY. So the
//! counter is asserted to fire for the mismatch population **and** not to fire for the unbound one,
//! and the firing arm is what makes the silent arm an observation rather than a typo.
//!
//! What the silent arm protects: the unbound population is the ONLY RESTAURANT population that
//! exists today. Counting its denial here would sit an expected pre-claim population inside a
//! signal whose whole meaning is "a bound caller reached for another tenant" — and would make the
//! series unreadable on the exact day it starts to matter.

use std::sync::{Arc, Mutex};

use application::queries::{RefundFilter, RefundReadRepository, RefundRow};
use async_graphql::Request;
use async_trait::async_trait;
use domain::generated::scalars as ds;
use domain::shared::errors::DomainError;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, SdkMeterProvider};
use server::graphql_acl::RequestRole;
use server::graphql_read_binding::ReadScopeBindingMode as Mode;
use server::graphql_schema::build_schema;

const MISMATCH: &str = "read_authorization_scope_mismatch_total";

fn restaurant(n: u128) -> ds::RestaurantId {
    ds::RestaurantId(uuid::Uuid::from_u128(0x1000 + n))
}

/// The same card-defined fixture as `graphql_pending_refunds_scope.rs`: O1,O2 → R1 and O3,O4,O5 →
/// R2. Unequal on purpose, so nothing passes by luck.
fn fixture() -> Vec<RefundRow> {
    let row = |n: u128, r: ds::RestaurantId| RefundRow {
        order_id: ds::OrderId(uuid::Uuid::from_u128(n)),
        restaurant_id: r,
        status: ds::RefundStatus::REQUESTED,
        amount_cents: ds::MoneyCents(1_000),
        currency: ds::CurrencyCode("EUR".into()),
        approved_amount_cents: None,
        reason: None,
        refund_id: None,
        requested_at: chrono::Utc::now(),
        decided_at: None,
    };
    vec![
        row(1, restaurant(1)),
        row(2, restaurant(1)),
        row(3, restaurant(2)),
        row(4, restaurant(2)),
        row(5, restaurant(2)),
    ]
}

/// `ReadScope`-BLIND by construction: `RefundReadRepository::list` takes only a `RefundFilter`, so
/// this stand-in cannot enforce the policy under test even by accident.
#[derive(Clone)]
struct FakeRefundQueue(Arc<Vec<RefundRow>>, Arc<Mutex<Vec<RefundFilter>>>);

#[async_trait]
impl RefundReadRepository for FakeRefundQueue {
    async fn list(&self, filter: RefundFilter) -> Result<Vec<RefundRow>, DomainError> {
        self.1.lock().unwrap().push(filter.clone());
        Ok(self
            .0
            .iter()
            .filter(|r| filter.restaurant_id.is_none_or(|want| want == r.restaurant_id))
            .cloned()
            .collect())
    }
}

/// Every data point of `MISMATCH` in the LATEST export, as `(operation|role|mode, count)`. Reading
/// the last batch keeps the assertions valid whichever temporality the SDK defaults to.
fn points(exporter: &InMemoryMetricExporter) -> Vec<(String, u64)> {
    let batches = exporter.get_finished_metrics().expect("finished metrics");
    let Some(latest) = batches.last() else { return Vec::new() };
    let mut out = Vec::new();
    for scope in latest.scope_metrics() {
        for metric in scope.metrics() {
            if metric.name() != MISMATCH {
                continue;
            }
            let AggregatedMetrics::U64(MetricData::Sum(sum)) = metric.data() else {
                panic!("a defect counter aggregates as a u64 Sum: {:?}", metric.data());
            };
            for dp in sum.data_points() {
                let mut kv: Vec<String> =
                    dp.attributes().map(|kv| format!("{}={}", kv.key, kv.value)).collect();
                kv.sort();
                out.push((kv.join("|"), dp.value()));
            }
        }
    }
    out.sort();
    out
}

/// Execute `pendingRefunds` once, returning the order-id count so an arm cannot be green over a
/// request that was rejected outright.
async fn query(
    repo: &FakeRefundQueue,
    role: RequestRole,
    scope: application::queries::ReadScope,
    filter: Option<ds::RestaurantId>,
    mode: Mode,
) -> usize {
    let args = match filter {
        Some(r) => format!("(input: {{ restaurantId: \"{}\" }})", r.0),
        None => String::new(),
    };
    let port: Arc<dyn RefundReadRepository> = Arc::new(repo.clone());
    let resp = build_schema(None, None, None)
        .execute(
            Request::new(format!("{{ pendingRefunds{args} {{ orderId }} }}"))
                .data(role)
                .data(scope)
                .data(port)
                .data(mode),
        )
        .await;
    assert!(resp.errors.is_empty(), "pendingRefunds errored: {:?}", resp.errors);
    resp.data.into_json().expect("json")["pendingRefunds"]
        .as_array()
        .expect("a list")
        .len()
}

/// ONE test, two arms — see the module doc for why it may not be two.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_mismatch_counter_fires_for_the_bound_population_and_not_the_unbound_one() {
    // The spy provider FIRST — before any request can bind the process-wide meter.
    let exporter = InMemoryMetricExporter::default();
    let provider = SdkMeterProvider::builder().with_periodic_exporter(exporter.clone()).build();
    opentelemetry::global::set_meter_provider(provider.clone());

    let repo = FakeRefundQueue(Arc::new(fixture()), Arc::new(Mutex::new(Vec::new())));

    // ARM 1 — the population the counter NAMES: a caller bound to R1 asking for R2, under OBSERVE.
    // It is still SERVED R2's three rows (that is what OBSERVE means) and the mismatch is counted
    // exactly once, carrying the mode — without which a count cannot tell a served mismatch from a
    // refused one.
    let served = query(
        &repo,
        RequestRole::Restaurant,
        application::queries::ReadScope::Restaurant(restaurant(1)),
        Some(restaurant(2)),
        Mode::Observe,
    )
    .await;
    provider.force_flush().expect("flush the spy reader");
    assert_eq!(
        (points(&exporter), served),
        (
            vec![(
                "mode=observe|operation=pendingRefunds|role=RESTAURANT".to_string(),
                1u64
            )],
            3
        ),
        "OBSERVE counts the mismatch ONCE per request and still serves what was asked for"
    );

    // ARM 2 — the population it must NOT name. An unbound RESTAURANT credential is denied, but it
    // has no bound scope to conflict with: that denial belongs to
    // read_authorization_bridge_unresolved_total, and putting today's only real RESTAURANT
    // population into a cross-tenant-attempt signal would drown the signal before it has anything
    // to say. The count must be UNCHANGED, not merely non-zero.
    let denied = query(
        &repo,
        RequestRole::Restaurant,
        application::queries::ReadScope::Public,
        None,
        Mode::Observe,
    )
    .await;
    provider.force_flush().expect("flush the spy reader");
    assert_eq!(
        (points(&exporter), denied),
        (
            vec![(
                "mode=observe|operation=pendingRefunds|role=RESTAURANT".to_string(),
                1u64
            )],
            0
        ),
        "the unbound denial must NOT increment the mismatch counter -- and the arm above is what \
         proves this assertion is reading a real series rather than a misspelled name"
    );
}
