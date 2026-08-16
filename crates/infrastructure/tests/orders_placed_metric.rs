//! #456: the one emission this chunk declares — `orders_placed_total{status="PLACED"}`, the "a
//! stranger paid us" BAM counter — proved FIRING through the real emit seam, never assumed from
//! the constant in `contract.rs`. Before this chunk there were ZERO emission sites, so the
//! un-told-order alarm could never fire.
//!
//! **What drives the proof: `flush_staged_in_tx` itself, against real Postgres.** The emit
//! (`mailbox::flush::record_order_placements`) is PRIVATE to its module since #597 — no delivery
//! route can name it — and this binary does not get an exception, because it does not need one:
//! the emit is the LAST thing the flush does once the staged appends are in the transaction, so
//! driving the flush drives the emit. That is strictly the stronger proof (#597 review): it fires
//! from the real, only path a staged event takes to `domain_events`, not from a test-only alias
//! that could silently drift from it.
//!
//! Its OWN test binary on purpose: `telemetry::meters` binds `opentelemetry::global::meter` once
//! per process (`OnceLock`), so the spy provider must be installed before the process's FIRST
//! metric call. The crate's `main` integration binary shares one process across ~30 suites, any of
//! which may touch a meter first and bind it to the no-op provider — so the proof would flake with
//! test order. One process, one provider, one metric test fn (a second one would race the shared
//! cumulative counter across threads). Same standalone-binary reason as
//! `crates/server/tests/checkout_degraded_metric.rs`. Using the flush changed none of that: the
//! database is per-test state, the meter binding is per-PROCESS state, and only the second one
//! forces a binary of its own.
//!
//! The pool comes from the same `TestDb` witness the `main` binary uses (included by `#[path]`, not
//! copied — a second hand-rolled migration chain is exactly what ADR-20260808-224500 item 5
//! deleted). Each shape gets its own stream so the four flushes cannot collide on
//! `UNIQUE (stream_name, version)`.

// The TestDb witness, shared with the `main` binary by path rather than duplicated. It carries one
// extra offline test (the migration-manifest check) which runs here too; it touches no meter, so
// it cannot perturb the cumulative counter this binary asserts on.
#[path = "main/common.rs"]
mod common;

use application::ports::Actor;
use application::staging::StagedAppend;
use domain::generated::entities as ent;
use domain::generated::events::{self as evs, DomainEvent};
use domain::generated::scalars as sc;
use infrastructure::mailbox::flush_staged_in_tx;
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

/// One staged append on its OWN stream — each shape below flushes for real, so two shapes sharing
/// a stream name would collide on `UNIQUE (stream_name, version)` and turn a metric assertion into
/// a confusing version conflict.
fn staged(stream: &str, events: Vec<DomainEvent>) -> Vec<StagedAppend> {
    vec![StagedAppend {
        stream_name: stream.into(),
        expected_version: 0,
        events,
        actor: actor(),
    }]
}

/// Drive the REAL seam: open a transaction, flush the staged set through the one function that
/// puts events in `domain_events`, commit. The emit is the last thing `flush_staged_in_tx` does,
/// so a shape that must not count is a shape this returns from having counted nothing.
async fn flush(pool: &sqlx::PgPool, staged: &[StagedAppend]) {
    let mut tx = pool.begin().await.expect("begin the delivery transaction");
    flush_staged_in_tx(&mut tx, staged).await.expect("the flush succeeds");
    tx.commit().await.expect("commit the delivery transaction");
}

/// The seam and its GATE, exercised over the four delivery shapes the emit must discriminate. One
/// process, one spy provider, one cumulative counter: after driving all four, exactly ONE PLACED
/// count — from the single real append. T2a/T2b/T4 each drive a shape a replay or a non-placement
/// delivery produces and must NOT advance the counter (the naive `Outcome::Completed` keying would
/// count T2a and T2b too — a monotonic BAM counter that lies).
#[tokio::test]
async fn orders_placed_total_fires_once_per_real_placement_and_never_on_a_replay() {
    // The spy provider FIRST — before any code path can bind the process-wide meter, and before
    // touching the database, since `TestDb::acquire` runs the migration chain.
    let exporter = InMemoryMetricExporter::default();
    let provider = SdkMeterProvider::builder().with_periodic_exporter(exporter.clone()).build();
    opentelemetry::global::set_meter_provider(provider.clone());

    let Some(db) = common::TestDb::acquire("orders_placed_metric").await else { return };
    let pool = db.pool();

    // T1 — a real placement: the staged set carries OrderPlaced → the one legitimate increment.
    flush(&pool, &staged("Order-0001", vec![order_placed()])).await;

    // T2a — the load-bearing guard shape: a re-delivery / partial-reaction replay finds the guard
    // (order fold is Some) false and stages a NON-OrderPlaced set. Must NOT advance.
    flush(&pool, &staged("Cart-0002", vec![cart_started()])).await;

    // T2b — a full re-delivery the runtime skipped: nothing staged at all. Must NOT advance.
    flush(&pool, &[]).await;

    // T4 — the PaymentFailed leg: no OrderPlaced ever reaches staging. Must NOT advance.
    flush(&pool, &staged("Cart-0003", vec![cart_started()])).await;

    provider.force_flush().expect("flush the spy reader");
    assert_eq!(
        orders_placed_points(&exporter),
        vec![("PLACED".to_string(), 1)],
        "exactly ONE PLACED count — the single real OrderPlaced append; replays and non-placements \
         leave the monotonic counter untouched"
    );

    // The counter counted what the log records: one OrderPlaced row, from the flush that emitted.
    let appended: i64 =
        sqlx::query_scalar("SELECT count(*) FROM domain_events WHERE event_type = 'OrderPlaced'")
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(appended, 1, "one append, one count — the emit rides the real flush");
}
