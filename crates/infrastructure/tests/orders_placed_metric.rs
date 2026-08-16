//! #456: the one emission this chunk declares — `orders_placed_total{status="PLACED"}`, the "a
//! stranger paid us" BAM counter — proved FIRING through the real emit seam, never assumed from
//! the constant in `contract.rs`. Before this chunk there were ZERO emission sites, so the
//! un-told-order alarm could never fire.
//!
//! The seam is `mailbox::flush::record_order_placements`, the LAST thing `flush_staged_in_tx` does
//! once the staged appends are in the completion transaction — the single place the counter's WHEN
//! is decided, for every delivery route there is or ever will be (#588). Since #597 it is PRIVATE
//! to that module, so this binary drives it through `record_order_placements_spy`, a delegating
//! seam compiled only under `test-fixtures`: the test keeps its proof, and no delivery route can
//! reach the decision.
//!
//! Its OWN test binary on purpose: `telemetry::meters` binds `opentelemetry::global::meter` once
//! per process (`OnceLock`), so the spy provider must be installed before the process's FIRST
//! metric call. The crate's `main` integration binary shares one process across ~30 suites, any of
//! which may touch a meter first and bind it to the no-op provider — so the proof would flake with
//! test order. One process, one provider, one test fn (multiple `#[test]`s in this binary would
//! race the shared cumulative counter across threads). Same standalone-binary reason as
//! `crates/server/tests/checkout_degraded_metric.rs`.
//!
//! No database: the emit is a PURE decision over the delivery's staged set (the transitive output
//! of the place-order guard — a replay stages no `OrderPlaced`), so the seam is exercised directly
//! over hand-built `StagedAppend`s. The guard's own staging behaviour on replay is proved against
//! real Postgres by `tests/main/pm_prepare_delivery.rs`.

use application::ports::Actor;
use application::staging::StagedAppend;
use domain::generated::entities as ent;
use domain::generated::events::{self as evs, DomainEvent};
use domain::generated::scalars as sc;
use infrastructure::mailbox::record_order_placements_spy as record_order_placements;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, SdkMeterProvider};

/// Every `orders_placed_total` data point the spy collected, as `(status, count)`.
fn orders_placed_points(exporter: &InMemoryMetricExporter) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for rm in exporter.get_finished_metrics().expect("finished metrics") {
        for scope in rm.scope_metrics() {
            for metric in scope.metrics() {
                if metric.name() != "orders_placed_total" {
                    continue;
                }
                let AggregatedMetrics::U64(MetricData::Sum(sum)) = metric.data() else {
                    panic!("a BAM counter aggregates as a u64 Sum: {:?}", metric.data());
                };
                for dp in sum.data_points() {
                    let status = dp
                        .attributes()
                        .find(|kv| kv.key.as_str() == "status")
                        .map(|kv| kv.value.to_string())
                        .unwrap_or_default();
                    out.push((status, dp.value()));
                }
            }
        }
    }
    out.sort();
    out
}

fn actor() -> Actor {
    Actor {
        user_id: uuid::Uuid::from_u128(0xC057),
        user_type: "CUSTOMER".into(),
        domain_id: None,
        correlation_id: uuid::Uuid::from_u128(0xC0),
        cause_id: None,
    }
}

fn eur(cents: i64) -> ent::Money {
    ent::Money { amount_cents: sc::MoneyCents(cents), currency: sc::CurrencyCode("EUR".into()) }
}

/// A real `OrderPlaced` fact — what the place-order guard appends exactly once per order, and the
/// ONLY event whose presence in the staged set must move the counter.
fn order_placed() -> DomainEvent {
    DomainEvent::OrderPlaced(evs::OrderPlaced {
        mode: None,
        order_id: sc::OrderId(uuid::Uuid::from_u128(0x0AD1)),
        r#ref: None,
        restaurant_id: sc::RestaurantId(uuid::Uuid::from_u128(0x0E57)),
        customer_id: sc::CustomerId(uuid::Uuid::from_u128(0xC057)),
        customer_contact: ent::CustomerContact {
            display_name: sc::CustomerDisplayName("Johnny".into()),
            email: None,
            phone: sc::PhoneNumber("+33612345678".into()),
        },
        service_type: sc::ServiceType::COLLECTION,
        delivery_address: None,
        items: Vec::new(),
        total_amount: eur(1960),
        breakdown: ent::PaymentBreakdown {
            articles: eur(1960),
            delivery: eur(0),
            service_fee: eur(0),
            total: eur(1960),
            restaurant_contribution: eur(0),
            restaurant_payout: eur(1960),
            rider_payout: eur(0),
            captain_net: eur(0),
        },
        note: None,
        replacement_of: None,
        payment_intent_id: Some(sc::PaymentIntentId("pi_test".into())),
    })
}

/// A non-order append (a cart fact): a delivery that reacted without placing an order.
fn cart_started() -> DomainEvent {
    DomainEvent::CartStarted(evs::CartStarted {
        cart_id: sc::CartId(uuid::Uuid::from_u128(0xCA47)),
        restaurant_id: sc::RestaurantId(uuid::Uuid::from_u128(0x0E57)),
        session_id: sc::SessionId(uuid::Uuid::from_u128(0x5E55)),
        customer_id: None,
    })
}

fn staged(events: Vec<DomainEvent>) -> Vec<StagedAppend> {
    vec![StagedAppend {
        stream_name: "Order-0000".into(),
        expected_version: 0,
        events,
        actor: actor(),
    }]
}

/// The seam and its GATE, exercised over the four delivery shapes the emit must discriminate. One
/// process, one spy provider, one cumulative counter: after driving all four, exactly ONE PLACED
/// count — from the single real append. T2a/T2b/T4 each drive a shape a replay or a non-placement
/// delivery produces and must NOT advance the counter (the naive `Outcome::Completed` keying would
/// count T2a and T2b too — a monotonic BAM counter that lies).
#[test]
fn orders_placed_total_fires_once_per_real_placement_and_never_on_a_replay() {
    // The spy provider FIRST — before any code path can bind the process-wide meter.
    let exporter = InMemoryMetricExporter::default();
    let provider = SdkMeterProvider::builder().with_periodic_exporter(exporter.clone()).build();
    opentelemetry::global::set_meter_provider(provider.clone());

    // T1 — a real placement: the staged set carries OrderPlaced → the one legitimate increment.
    record_order_placements(&staged(vec![order_placed()]));

    // T2a — the load-bearing guard shape: a re-delivery / partial-reaction replay finds the guard
    // (order fold is Some) false and stages a NON-OrderPlaced set. Must NOT advance.
    record_order_placements(&staged(vec![cart_started()]));

    // T2b — a full re-delivery the runtime skipped: nothing staged at all. Must NOT advance.
    record_order_placements(&[]);

    // T4 — the PaymentFailed leg: no OrderPlaced ever reaches staging. Must NOT advance.
    record_order_placements(&staged(vec![cart_started()]));

    provider.force_flush().expect("flush the spy reader");
    assert_eq!(
        orders_placed_points(&exporter),
        vec![("PLACED".to_string(), 1)],
        "exactly ONE PLACED count — the single real OrderPlaced append; replays and non-placements \
         leave the monotonic counter untouched"
    );
}
