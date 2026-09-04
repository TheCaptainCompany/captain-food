//! `order_birth_lag_ms{routed="true"}` SEEN RECORDED **for the REPLACEMENT birth** (#595,
//! ADR-20260829-230418 chunk C2).
//!
//! The replacement order's birth IS an `OrderPlaced` on an `Order-{id}` stream, so the handover it
//! introduces is the same fact `order_birth_lag_ms` was declared to measure — enqueue to
//! `Recorded`. It is NOT covered by the existing emission, though, and that is the whole reason
//! this binary exists: the declared emitter's original call site is the inbound-FACT route
//! (`handler.rs`'s `handle_recorded_fact`), while a replacement order is born through the COMMAND
//! route. #595 added the second call site; without a test, a route whose predicate never matches is
//! indistinguishable from a route nobody used — the #758 defect class, one route later.
//!
//! What is asserted:
//!
//! - **the enqueue is not the emission** — after the saga leg commits the replacement door row, the
//!   histogram has ZERO points (the handover has not happened yet);
//! - **the lane delivery is** — after the Order lane drains, EXACTLY ONE point exists, with
//!   `routed="true"` (read from the DECLARED `ROUTED_LANES` table, not from a config flag) and a
//!   sane value.
//!
//! **Its own binary, ONE `#[tokio::test]`**: `telemetry::meters` binds the process-global meter
//! once (`OnceLock`), so the spy provider must be installed before the FIRST metric call — the
//! same constraint `order_birth_lag_metric.rs` carries.
//!
//! Needs `DATABASE_URL`: since #474 a missing database FAILS this suite.

#[path = "main/common.rs"]
mod common;
#[path = "main/spy_meter.rs"]
mod spy_meter;

use std::collections::BTreeMap;
use std::sync::Arc;

use actor_runtime::{MailboxWorker, WorkerConfig};
use application::generated::services::IdentityService;
use infrastructure::generated::command_router::CommandDeps;
use infrastructure::mailbox::MailboxCommandHandler;
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, FailClosedPaymentGateway,
    PgCustomerRepository, PgEventStore, PgProspectionRepository, PgRestaurantRepository,
    PgAuthSubjectReservationRepository, PgSlugReservationRepository, ProcessManagerRunner, UnverifiedGbpOrderLinkProbe,
};
use sqlx::PgPool;
use telemetry::contract::metric;

fn uid(n: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128(n)
}

const ORIGINAL_ORDER: u128 = 0x0_1D_0AD1;
const RESTAURANT: u128 = 0x0E57;
const CUSTOMER: u128 = 0xC057;
const CLAIM: u128 = 0xC1A1_4;

fn money(cents: i64) -> serde_json::Value {
    serde_json::json!({ "amountCents": cents, "currency": "EUR" })
}

async fn seed(pool: &PgPool) {
    for (stream, event_type, payload) in [
        (
            format!("Order-{}", uid(ORIGINAL_ORDER)),
            "OrderPlaced",
            serde_json::json!({
                "orderId": uid(ORIGINAL_ORDER),
                "restaurantId": uid(RESTAURANT),
                "customerId": uid(CUSTOMER),
                "customerContact": { "displayName": "Léa", "phone": "+33612345678" },
                "serviceType": "COLLECTION",
                "items": [{
                    "offerId": uid(0x0FFE),
                    "name": "Margherita",
                    "quantity": 1,
                    "unitPrice": money(980),
                    "lineTotal": money(980)
                }],
                "totalAmount": money(980),
                "breakdown": {
                    "articles": money(980), "delivery": money(0), "serviceFee": money(0),
                    "total": money(980), "restaurantContribution": money(0),
                    "restaurantPayout": money(980), "riderPayout": money(0), "captainNet": money(0)
                }
            }),
        ),
        (
            format!("Reclamation-{}", uid(CLAIM)),
            "ReclamationResolved",
            serde_json::json!({
                "reclamationId": uid(CLAIM),
                "orderId": uid(ORIGINAL_ORDER),
                "customerId": uid(CUSTOMER),
                "resolution": "REPLACEMENT"
            }),
        ),
    ] {
        sqlx::query(
            "INSERT INTO domain_events \
             (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, \
              payload, metadata, occurred_at) \
             VALUES ($1, $2, 0, $3, 5, $4, NULL, $5, $6, NULL, now())",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(stream)
        .bind(uuid::Uuid::nil())
        .bind(uuid::Uuid::new_v4())
        .bind(event_type)
        .bind(payload)
        .execute(pool)
        .await
        .expect("seed");
    }
}

fn order_deps(pool: &PgPool) -> CommandDeps {
    CommandDeps {
        store: Arc::new(PgEventStore::new(pool.clone())),
        // #639 part C step 2c-i: the rider sign-in door's bridge + support route (not exercised here).
        riders: Arc::new(infrastructure::PgRiderRepository::new(pool.clone())),
        support_contact: None,
        run_rider_restriction_door: false,
        restaurants: Arc::new(PgRestaurantRepository::new(pool.clone())),
        slugs: Arc::new(PgSlugReservationRepository::new(pool.clone())),
        auth_subjects: Arc::new(PgAuthSubjectReservationRepository::new(pool.clone())),
        ownership: Arc::new(FailClosedGoogleOwnershipVerifier),
        probe: Arc::new(UnverifiedGbpOrderLinkProbe),
        prospection: Arc::new(PgProspectionRepository::new(pool.clone())),
        catalogs: Arc::new(infrastructure::PgCatalogRepository::new(pool.clone())),
        auth: Arc::new(FailClosedIdentityService) as Arc<dyn IdentityService>,
        customers: Arc::new(PgCustomerRepository::new(pool.clone())),
        sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
        payments: Arc::new(FailClosedPaymentGateway),
        pm_state: Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone())),
        refund_state: Arc::new(infrastructure::persistence::PgRefundProcessState::new(
            pool.clone(),
        )),
        mailbox_requeue: Arc::new(
            infrastructure::persistence::mailbox_lanes::PgMailboxRequeue::new(pool.clone()),
        ),
        enforce_service_hours_guard: false,
        enforce_acceptance_timeout: false,
        route_gates: application::generated::process_managers::RouteGates {
            order_placed_to_order: true,
            place_replacement_order_to_order: false,
            // #807: routed `send:` steps -- OFF, this fixture exercises the birth routes.
            bind_cart_to_customer_to_cart: false,
            grant_customer_credit_to_customer_credit: false,
            mark_order_delivered_to_order: false,
        },
    }
}

#[tokio::test]
async fn the_replacement_birth_records_the_handover_on_the_lane_delivery() {
    // BEFORE the database: the meter binding is a `OnceLock` and the loser observes nothing.
    let spy = spy_meter::SpyMeter::install();
    let Some(db) = common::TestDb::acquire("replacement_birth_lag_metric").await else { return };
    let pool = db.pool();
    seed(&pool).await;

    // ── the ENQUEUE is not the emission ─────────────────────────────────────────────────────────
    ProcessManagerRunner::new(pool.clone())
        .with_only("ReclamationProcess")
        .with_route_gates(application::generated::process_managers::RouteGates {
            order_placed_to_order: false,
            place_replacement_order_to_order: true,
            // #807: routed `send:` steps -- OFF, this fixture exercises the birth routes.
            bind_cart_to_customer_to_cart: false,
            grant_customer_credit_to_customer_credit: false,
            mark_order_delivered_to_order: false,
        })
        .run_once()
        .await
        .expect("the reclamation group drains clean");
    assert!(
        spy.drain().points(metric::ORDER_BIRTH_LAG_MS).is_empty(),
        "no birth-lag point before the Order lane delivers: the handover is enqueue -> Recorded, \
         and the enqueue alone has measured nothing"
    );

    // ── the LANE DELIVERY is ────────────────────────────────────────────────────────────────────
    let windows = std::collections::HashMap::from([
        ("ORDER_ACCEPTANCE_TIMEOUT_SECONDS", std::time::Duration::from_secs(300)),
        ("ORDER_RETENTION_WINDOW_DAYS", std::time::Duration::from_secs(30 * 86_400)),
    ]);
    let worker = MailboxWorker::new(
        pool.clone(),
        "w-REPL-LAG",
        "Order",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(order_deps(&pool)).with_reminder_windows(windows)),
    );
    worker.seed(5).await.expect("seed order lanes");
    worker.claim().await.expect("claim order lanes");
    let mut delivered = 0;
    for lane in worker.owned().await {
        delivered += worker.drain_lane(&lane).await.expect("lane drains clean");
    }
    assert_eq!(delivered, 1, "the Order lane delivers the replacement command");

    let recorded = spy.drain();
    let routed_true: BTreeMap<String, String> =
        BTreeMap::from([("routed".to_string(), "true".to_string())]);
    let points = recorded.points(metric::ORDER_BIRTH_LAG_MS);
    assert_eq!(
        points.len(),
        1,
        "EXACTLY ONE measurement for the replacement birth. Empty here means the COMMAND route's \
         emitter never fired — a replacement order born through the lane that no histogram can \
         see, which is the state #595 inherited for the whole route: {points:?}"
    );
    let (attrs, lag_ms) = &points[0];
    assert_eq!(
        attrs, &routed_true,
        "routed=\"true\" comes from the DECLARED ROUTED_LANES table, so a row enqueued before a \
         rollback and delivered after it still reports how it actually got here"
    );
    assert!(
        (0.0..300_000.0).contains(lag_ms),
        "a sane handover lag: now - received_at of a row enqueued moments ago must be \
         0 <= lag < 300000 ms, got {lag_ms}"
    );
    eprintln!("EVIDENCE: replacement order_birth_lag_ms{{routed=\"true\"}} recorded {lag_ms} ms");
}
