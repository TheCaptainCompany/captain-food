//! END-TO-END acceptance-timeout delivery (#167, ADR-20260808-195315 §1.3): the kind-MESSAGE
//! `OrderAcceptanceTimedOut` route against a real Postgres, through the real worker.
//!
//! THE FENCE (PR #586 mob checkpoint, young+vernon binding): `apply_schedules_in_tx` is
//! verdict-blind and the timeout receive declares `schedules: OrderExpired`, so the delivery
//! route applies schedules on the **Recorded/Cancelled arm ONLY** — proved here in both
//! directions:
//!   - gate OFF (shadow), still PLACED → verdict IGNORED, NOTHING appended, and **no OrderExpired
//!     reminder row exists** (a shadow firing must never arm the GDPR deletion clock on a
//!     still-PLACED order);
//!   - gate ON, still PLACED → `OrderAcceptanceTimedOut` appended (cause-chained to the reminder
//!     row) AND the OrderExpired retention reminder is SCHEDULED in the same transaction.
//!
//! Plus beck's keep-conflict test: delivering the birth TWICE through the worker must leave the
//! acceptance deadline's `scheduled_at` untouched — the ONLY real exercise of the in-tx `Keep`
//! arm's `ON CONFLICT DO NOTHING` (mutating it to `DO UPDATE` goes red here, loudly: the first
//! row's sentinel time would be overwritten by `now()+window`).
//!
//! Needs `DATABASE_URL`: since #474 a missing database FAILS this suite; only an explicit
//! `DB_TESTS_REQUIRED=0` skips it (`crates/db_test_gate`).

use std::sync::Arc;

use actor_client::declared_lane;
use actor_runtime::{MailboxWorker, WorkerConfig};
use application::generated::services::{IdentityService, PaymentService};
use application::reminders::reminder_message_id;

use infrastructure::generated::command_router::CommandDeps;
use infrastructure::mailbox::MailboxCommandHandler;
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, FailClosedPaymentGateway,
    PgCatalogRepository, PgCustomerRepository, PgEventStore, PgProspectionRepository,
    PgRestaurantRepository, PgAuthSubjectReservationRepository, PgSlugReservationRepository, UnverifiedGbpOrderLinkProbe,
};
use sqlx::{PgPool, Row};

/// Deps with the acceptance-timeout gate in an explicit position — the ONE knob these tests
/// exercise both ways (read at DELIVERY time, ADR gate-then-stabilize).
fn deps_over(pool: &PgPool, enforce_acceptance_timeout: bool) -> CommandDeps {
    CommandDeps {
        store: Arc::new(PgEventStore::new(pool.clone())),
        // #639 part C step 2c-i: the rider sign-in door's bridge + support route (not exercised here).
        riders: Arc::new(infrastructure::PgRiderRepository::new(pool.clone())),
        support_contact: None,
        restaurants: Arc::new(PgRestaurantRepository::new(pool.clone())),
        slugs: Arc::new(PgSlugReservationRepository::new(pool.clone())),
        auth_subjects: Arc::new(PgAuthSubjectReservationRepository::new(pool.clone())),
        ownership: Arc::new(FailClosedGoogleOwnershipVerifier),
        probe: Arc::new(UnverifiedGbpOrderLinkProbe),
        prospection: Arc::new(PgProspectionRepository::new(pool.clone())),
        catalogs: Arc::new(PgCatalogRepository::new(pool.clone())),
        auth: Arc::new(FailClosedIdentityService) as Arc<dyn IdentityService>,
        customers: Arc::new(PgCustomerRepository::new(pool.clone())),
        sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
        payments: Arc::new(FailClosedPaymentGateway) as Arc<dyn PaymentService>,
        pm_state: Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone())),
        refund_state: Arc::new(infrastructure::persistence::PgRefundProcessState::new(
            pool.clone(),
        )),
        mailbox_requeue: Arc::new(infrastructure::persistence::mailbox_lanes::PgMailboxRequeue::new(pool.clone())),
        enforce_service_hours_guard: false,
        // #588: the Order-lane birth routing at its spec default (OFF = the legacy append).
        route_gates: application::generated::process_managers::RouteGates {
            order_placed_to_order: false,
            place_replacement_order_to_order: false,
            // #807: routed `send:` steps -- OFF, this fixture exercises the birth routes.
            bind_cart_to_customer_to_cart: false,
            grant_customer_credit_to_customer_credit: false,
            mark_order_delivered_to_order: false,
        },
        enforce_acceptance_timeout,
    }
}

/// The wired windows: BOTH Order-lane keys — gate-OFF is a config value, never an absent key
/// (a missing window aborts every delivery on the lane for retry, by design).
fn windows() -> std::collections::HashMap<&'static str, std::time::Duration> {
    std::collections::HashMap::from([
        ("ORDER_ACCEPTANCE_TIMEOUT_SECONDS", std::time::Duration::from_secs(300)),
        ("ORDER_RETENTION_WINDOW_DAYS", std::time::Duration::from_secs(30 * 86_400)),
    ])
}

fn worker_over(pool: &PgPool, enforce: bool, id: &str) -> MailboxWorker {
    MailboxWorker::new(
        pool.clone(),
        id.to_string(),
        "Order",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(
            MailboxCommandHandler::new(deps_over(pool, enforce)).with_reminder_windows(windows()),
        ),
    )
}

fn money(cents: i64) -> serde_json::Value {
    serde_json::json!({ "amountCents": cents, "currency": "EUR" })
}

async fn append_event(
    pool: &PgPool,
    stream: &str,
    version: i32,
    event_type: &str,
    payload: serde_json::Value,
) {
    sqlx::query(
        "INSERT INTO domain_events \
           (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, \
            payload, metadata, occurred_at) \
         VALUES ($1, $2, $3, $4, 'ADMIN', $4, NULL, $5, $6, NULL, now())",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(stream)
    .bind(version)
    .bind(uuid::Uuid::nil())
    .bind(event_type)
    .bind(payload)
    .execute(pool)
    .await
    .expect("seed event");
}

/// Seed one PLACED order (the birth fact only) — the state a due acceptance deadline finds.
async fn seed_placed_order(pool: &PgPool, order: uuid::Uuid, restaurant: uuid::Uuid) {
    append_event(
        pool,
        &format!("Order-{order}"),
        1,
        "OrderPlaced",
        serde_json::json!({
            "orderId": order,
            "restaurantId": restaurant,
            "customerId": uuid::Uuid::new_v4(),
            "customerContact": { "displayName": "Léa", "phone": "+33612345678" },
            "serviceType": "DELIVERY",
            "items": [{
                "offerId": uuid::Uuid::new_v4(),
                "name": "Margherita",
                "quantity": 1,
                "unitPrice": money(980),
                "lineTotal": money(980)
            }],
            "totalAmount": money(1580),
            "breakdown": {
                "articles": money(980),
                "delivery": money(400),
                "serviceFee": money(200),
                "total": money(1580),
                "restaurantContribution": money(80),
                "restaurantPayout": money(900),
                "riderPayout": money(400),
                "captainNet": money(280)
            }
        }),
    )
    .await;
}

/// The REAL scheduled reminder row (deterministic UUIDv5 identity), already due.
async fn schedule_due_timeout(pool: &PgPool, order: uuid::Uuid, partition: i16) -> uuid::Uuid {
    let id = reminder_message_id(order, "OrderAcceptanceTimedOut");
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, \
            payload_hash, channel, user_type, correlation_id, scheduled_at, status) \
         VALUES ($1, 'MESSAGE', 'Order', $2, $3, 'OrderAcceptanceTimedOut', $4, $5, 'WORKER', \
                 'EXTERNAL', $1, now() - interval '1 minute', 'SCHEDULED')",
    )
    .bind(id)
    .bind(order)
    .bind(partition)
    .bind(serde_json::json!({ "eventType": "OrderAcceptanceTimedOut", "payload": { "orderId": order } }))
    .bind(format!("h-{order}"))
    .execute(pool)
    .await
    .expect("schedule due timeout");
    id
}

async fn verdict_of(pool: &PgPool, message_id: uuid::Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM inbound_messages WHERE message_id = $1")
        .bind(message_id)
        .fetch_one(pool)
        .await
        .expect("verdict")
}

async fn count_events(pool: &PgPool, stream: &str, event_type: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM domain_events WHERE stream_name = $1 AND event_type = $2",
    )
    .bind(stream)
    .bind(event_type)
    .fetch_one(pool)
    .await
    .expect("count")
}

/// The FENCE's observable: does an OrderExpired reminder row exist for this order?
async fn gdpr_clock_armed(pool: &PgPool, order: uuid::Uuid) -> bool {
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM inbound_messages \
         WHERE message_id = $1 AND message_type = 'OrderExpired'",
    )
    .bind(reminder_message_id(order, "OrderExpired"))
    .fetch_one(pool)
    .await
    .expect("count OrderExpired reminder");
    n > 0
}

/// Gate OFF (shadow): the due deadline on a still-PLACED order lands IGNORED — the full guard
/// ran, the append was inert — and THE FENCE holds: no OrderExpired reminder is scheduled, so a
/// shadow firing can never start the GDPR deletion clock on a live order.
#[tokio::test]
async fn shadow_would_cancel_lands_ignored_and_never_arms_the_gdpr_clock() {
    let Some(db) = crate::common::TestDb::acquire("mailbox_acceptance_timeout").await else { return };
    let pool = db.pool();

    let order = uuid::Uuid::from_u128(0x51AD0);
    let partition = declared_lane("Order", &order).expect("Order declares a mailbox");
    seed_placed_order(&pool, order, uuid::Uuid::from_u128(0x0E57)).await;
    let reminder = schedule_due_timeout(&pool, order, partition).await;

    let worker = worker_over(&pool, false, "w-shadow");
    worker.seed(5).await.expect("seed");
    assert_eq!(worker.promote().await.expect("promote"), 1, "the due deadline promotes");
    worker.claim().await.expect("claim");
    assert_eq!(worker.drain().await.expect("drain"), 1, "the promoted deadline delivered");

    assert_eq!(
        verdict_of(&pool, reminder).await,
        "IGNORED",
        "shadow would-cancel is record semantics: Ignored, never Rejected"
    );
    assert_eq!(
        count_events(&pool, &format!("Order-{order}"), "OrderAcceptanceTimedOut").await,
        0,
        "the append is inert while the gate is OFF"
    );
    assert!(
        !gdpr_clock_armed(&pool, order).await,
        "THE FENCE: a shadow WouldCancel→Ignored delivery must NEVER schedule the OrderExpired \
         GDPR reminder on a still-PLACED order"
    );
}

/// Gate ON: the due deadline on a still-PLACED order records the cancellation (cause-chained to
/// the reminder row) AND — the fence's positive side — starts the OrderExpired retention clock
/// in the SAME delivery, like every other terminal. A redelivery is then a Duplicate; a deadline
/// racing a faster acceptance is Ignored and schedules nothing.
#[tokio::test]
async fn enforced_timeout_cancels_a_placed_order_and_starts_the_gdpr_clock() {
    let Some(db) = crate::common::TestDb::acquire("mailbox_acceptance_timeout").await else { return };
    let pool = db.pool();

    let order = uuid::Uuid::from_u128(0x51AD1);
    let partition = declared_lane("Order", &order).expect("Order declares a mailbox");
    let stream = format!("Order-{order}");
    seed_placed_order(&pool, order, uuid::Uuid::from_u128(0x0E57)).await;
    let reminder = schedule_due_timeout(&pool, order, partition).await;

    let worker = worker_over(&pool, true, "w-enforce");
    worker.seed(5).await.expect("seed");
    assert_eq!(worker.promote().await.expect("promote"), 1);
    worker.claim().await.expect("claim");
    assert_eq!(worker.drain().await.expect("drain"), 1);

    assert_eq!(verdict_of(&pool, reminder).await, "SUCCEEDED");
    let cancelled = sqlx::query(
        "SELECT version, cause_id FROM domain_events \
         WHERE stream_name = $1 AND event_type = 'OrderAcceptanceTimedOut'",
    )
    .bind(&stream)
    .fetch_one(&pool)
    .await
    .expect("OrderAcceptanceTimedOut recorded");
    assert_eq!(cancelled.get::<i32, _>("version"), 2, "recorded after the birth (v1)");
    assert_eq!(
        cancelled.get::<Option<uuid::Uuid>, _>("cause_id"),
        Some(reminder),
        "cause-chained to the reminder row that carried the deadline"
    );
    assert!(
        gdpr_clock_armed(&pool, order).await,
        "the Recorded/Cancelled arm — and ONLY it — schedules the OrderExpired retention \
         reminder: a timed-out order is terminal and must still be erased on schedule"
    );

    // A redelivered deadline under a fresh identity: DUPLICATE, nothing appended.
    let replay = uuid::Uuid::from_u128(0x51AD2);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, \
            payload_hash, channel, user_type, correlation_id) \
         VALUES ($1, 'MESSAGE', 'Order', $2, $3, 'OrderAcceptanceTimedOut', $4, 'h-replay', \
                 'WORKER', 'EXTERNAL', $1)",
    )
    .bind(replay)
    .bind(order)
    .bind(partition)
    .bind(serde_json::json!({ "eventType": "OrderAcceptanceTimedOut", "payload": { "orderId": order } }))
    .execute(&pool)
    .await
    .expect("enqueue replay");
    assert_eq!(worker.drain().await.expect("drain (replay)"), 1);
    assert_eq!(verdict_of(&pool, replay).await, "DUPLICATE");
    assert_eq!(count_events(&pool, &stream, "OrderAcceptanceTimedOut").await, 1);

    // The race that matters: acceptance won before the deadline fired — Ignored, no append,
    // and NO GDPR clock from this arm either (an ACCEPTED order is not terminal).
    let racer = uuid::Uuid::from_u128(0x51AD3);
    let racer_partition = declared_lane("Order", &racer).expect("Order declares a mailbox");
    let racer_stream = format!("Order-{racer}");
    seed_placed_order(&pool, racer, uuid::Uuid::from_u128(0x0E57)).await;
    append_event(
        &pool,
        &racer_stream,
        2,
        "OrderAcceptedByRestaurant",
        serde_json::json!({ "orderId": racer, "restaurantId": uuid::Uuid::from_u128(0x0E57) }),
    )
    .await;
    let racer_reminder = schedule_due_timeout(&pool, racer, racer_partition).await;
    assert_eq!(worker.promote().await.expect("promote (racer)"), 1);
    worker.claim().await.expect("claim (racer lane)");
    assert!(worker.drain().await.expect("drain (racer)") >= 1);
    assert_eq!(
        verdict_of(&pool, racer_reminder).await,
        "IGNORED",
        "acceptance won the race: benign no-op even with the gate ON"
    );
    assert_eq!(count_events(&pool, &racer_stream, "OrderAcceptanceTimedOut").await, 0);
    assert!(
        !gdpr_clock_armed(&pool, racer).await,
        "a no-append arm never schedules — the fence covers NotPlaced too"
    );
}

// beck (PR #586 checkpoint) used to prove the in-tx `Keep` arm here, driving the birth twice
// through a HAND-WRITTEN `enqueue_birth` INSERT. That crutch existed only because no PRODUCER
// enqueued the birth (#588), and leaving it would have kept the producer-free path green
// forever. The same proof now runs off the real producer, end-to-end from the authorized
// checkout: `pm_prepare_delivery.rs::redelivered_authorization_dedups_the_birth_at_the_door`
// (ADR-20260816-040239). Same sentinel, same `DO NOTHING` mutant, real path.

/// The promotion watch's SQL against the real DDL (#167, ADR-20260810-231300): one tick over a
/// mixed backlog (a due row, an undue row) completes — the emission side is a global-meter
/// no-op here (no provider installed); what this pins is that the aggregate query the
/// dead-man's switch depends on cannot drift from the schema unnoticed.
#[tokio::test]
async fn promotion_watch_tick_reads_the_real_scheduled_backlog() {
    let Some(db) = crate::common::TestDb::acquire("mailbox_acceptance_timeout").await else { return };
    let pool = db.pool();

    let due = uuid::Uuid::from_u128(0x0900);
    schedule_due_timeout(&pool, due, declared_lane("Order", &due).expect("Order declares a mailbox"))
        .await;
    // An undue OrderExpired reminder beside it (different purpose, same lane).
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, \
            payload_hash, channel, user_type, correlation_id, scheduled_at, status) \
         VALUES ($1, 'MESSAGE', 'Order', $2, $3, 'OrderExpired', $4, 'h-undue', 'WORKER', \
                 'EXTERNAL', $1, now() + interval '30 days', 'SCHEDULED')",
    )
    .bind(reminder_message_id(due, "OrderExpired"))
    .bind(due)
    .bind(declared_lane("Order", &due).expect("Order declares a mailbox"))
    .bind(serde_json::json!({ "eventType": "OrderExpired", "payload": { "orderId": due } }))
    .execute(&pool)
    .await
    .expect("schedule undue expiry");

    infrastructure::mailbox::promotion_watch_tick(&pool)
        .await
        .expect("one watch tick over the real schema");
}
