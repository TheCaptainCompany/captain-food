//! END-TO-END PM command delivery through the PREPARE phase (#272 Runtime D1,
//! ADR-20260801-023000 R2): `PlaceOrder` / `ApproveRefund` flow mailbox → prepare (validate,
//! price, gateway call — NO transaction open) → ONE fenced commit (staged events + PM run row +
//! verdict). Proves, against a real Postgres:
//!
//! - a valid `PlaceOrder` commits `PaymentIntentCreated` (cause-chained to the mailbox row), the
//!   `payment_process_manager` run row and the SUCCEEDED verdict in one transaction, with the
//!   gateway called exactly once;
//! - a deterministic rejection (empty cart) commits REJECTED `CartEmpty` with NOTHING written —
//!   no event, no PM row, no gateway call;
//! - a synchronous gateway DECLINE commits the byte-identical legacy contract: REJECTED
//!   `PaymentDeclined` on the operation, nothing written;
//! - an `ApproveRefund` on a PENDING_APPROVAL run calls the refund gateway in prepare and
//!   commits `RefundApproved` + the APPROVED_AWAITING_SETTLEMENT row atomically.
//!
//! Needs `DATABASE_URL`: since #474 a missing database FAILS this suite; only an explicit
//! `DB_TESTS_REQUIRED=0` skips it, and that leaves a receipt (`crates/db_test_gate`).

use std::sync::{Arc, Mutex};

use actor_runtime::{MailboxWorker, WorkerConfig};
use application::generated::services::{
    IdentityService, PaymentRefundInput, PaymentRequestInput, PaymentRequestOutput,
    PaymentService, ServiceCallMeta,
};
use application::ports::{Actor, EventStore};
use application::queries::{CatalogReadRepository, CatalogRow, OfferView};
use async_trait::async_trait;
use domain::generated::entities::{Address, CartLineItem, Money};
use domain::generated::events::{CartLineAdded, CartStarted, DomainEvent, RestaurantActivated, RestaurantRegistered};
use domain::generated::scalars::*;
use domain::shared::errors::DomainError;
use infrastructure::generated::command_router::CommandDeps;
use infrastructure::mailbox::MailboxCommandHandler;
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, FailClosedPaymentGateway,
    PgCustomerRepository, PgEventStore, PgProspectionRepository,
    PgRestaurantRepository, PgSlugReservationRepository, UnverifiedGbpOrderLinkProbe,
};
use sqlx::{PgPool, Row};

fn uid(n: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128(n)
}
fn eur(cents: i64) -> Money {
    Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
}

const RESTAURANT: u128 = 0x0E57;
const CART: u128 = 0xCA47;
const ORDER: u128 = 0x0AD1;
const OFFER: u128 = 0x0FFE;

/// Fixed-price catalog stub: the only offer is `OFFER` at 19.60 EUR (pricing is exercised, the
/// catalog projection is not what this suite tests).
struct StubCatalog;

#[async_trait]
impl CatalogReadRepository for StubCatalog {
    async fn by_restaurant(
        &self,
        _restaurant_id: RestaurantId,
    ) -> Result<Option<CatalogRow>, DomainError> {
        Ok(None)
    }

    async fn offer_by_id(
        &self,
        _restaurant_id: RestaurantId,
        offer_id: OfferId,
    ) -> Result<Option<OfferView>, DomainError> {
        if offer_id != OfferId(uid(OFFER)) {
            return Ok(None);
        }
        Ok(Some(OfferView {
            offer_id,
            product_id: ProductId(uid(0xF00D)),
            product_name: ProductName("Kebab".into()),
            offer_name: OfferName("Solo".into()),
            price: eur(980),
            availability: CatalogItemAvailability::AVAILABLE,
            stock_status: StockStatus::IN_STOCK,
            stock_quantity: None,
            option_lists: Vec::new(),
        }))
    }
}

/// Recording gateway: intents succeed with a fixed id; refunds succeed. Counts calls — the
/// exactly-once witness for the prepare phase.
#[derive(Default)]
struct StubGateway {
    intents: Mutex<Vec<PaymentRequestInput>>,
    refunds: Mutex<Vec<PaymentRefundInput>>,
}

#[async_trait]
impl PaymentService for StubGateway {
    async fn request(
        &self,
        input: PaymentRequestInput,
        _meta: &ServiceCallMeta,
    ) -> Result<PaymentRequestOutput, DomainError> {
        self.intents.lock().unwrap().push(input);
        Ok(PaymentRequestOutput {
            payment_intent_id: PaymentIntentId("pi_prepare_test".into()),
            client_secret: "pi_prepare_test_secret".into(),
        })
    }

    async fn refund(
        &self,
        input: PaymentRefundInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        self.refunds.lock().unwrap().push(input);
        Ok(())
    }
}

fn address() -> Address {
    Address {
        line1: AddressLine("1 rue Nationale".into()),
        line2: None,
        postal_code: PostalCode("37000".into()),
        city: CityName("Tours".into()),
        country: CountryCode("FR".into()),
    }
}

/// Seed an ACTIVE restaurant and an OPEN cart with one line, through the ordinary event store.
async fn seed_checkout_world(pool: &PgPool, with_line: bool) {
    let store = PgEventStore::new(pool.clone());
    let actor = Actor {
        user_id: uid(0xAD),
        user_type: "ADMIN".into(),
        domain_id: None,
        correlation_id: uid(0xC0),
        cause_id: None,
    };
    store
        .append(
            &format!("Restaurant-{}", uid(RESTAURANT)),
            0,
            &[
                DomainEvent::RestaurantRegistered(RestaurantRegistered {
                    mode: None,
                    restaurant_id: RestaurantId(uid(RESTAURANT)),
                    account_id: None,
                    listing_status: RestaurantListingStatus::NON_PARTNER,
                    r#ref: None,
                    external_identifiers: Vec::new(),
                    display_name: RestaurantDisplayName("Chez Test".into()),
                    contact: None,
                    website: None,
                    tags: Vec::new(),
                    margin_rate: None,
                    cuisine_category: None,
                    uber_prices_opt_in: None,
                    address: address(),
                    location: None,
                    timezone: None,
                    preparation_time_minutes: None,
                    opening_hours: Vec::new(),
                }),
                DomainEvent::RestaurantActivated(RestaurantActivated {
                    restaurant_id: RestaurantId(uid(RESTAURANT)),
                    reason: None,
                }),
            ],
            &actor,
        )
        .await
        .expect("restaurant stream");
    let mut cart_events = vec![DomainEvent::CartStarted(CartStarted {
        cart_id: CartId(uid(CART)),
        restaurant_id: RestaurantId(uid(RESTAURANT)),
        session_id: SessionId(uid(0x5E55)),
        customer_id: None,
    })];
    if with_line {
        cart_events.push(DomainEvent::CartLineAdded(CartLineAdded {
            cart_id: CartId(uid(CART)),
            line: CartLineItem {
                cart_line_id: CartLineId(uid(0x11E)),
                offer_id: OfferId(uid(OFFER)),
                quantity: 2,
                selected_option_ids: Vec::new(),
            },
        }));
    }
    store
        .append(&format!("Cart-{}", uid(CART)), 0, &cart_events, &actor)
        .await
        .expect("cart stream");
}

fn deps_over(pool: &PgPool, payments: Arc<dyn PaymentService>) -> CommandDeps {
    CommandDeps {
        store: Arc::new(PgEventStore::new(pool.clone())),
        restaurants: Arc::new(PgRestaurantRepository::new(pool.clone())),
        slugs: Arc::new(PgSlugReservationRepository::new(pool.clone())),
        ownership: Arc::new(FailClosedGoogleOwnershipVerifier),
        probe: Arc::new(UnverifiedGbpOrderLinkProbe),
        prospection: Arc::new(PgProspectionRepository::new(pool.clone())),
        catalogs: Arc::new(StubCatalog),
        auth: Arc::new(FailClosedIdentityService) as Arc<dyn IdentityService>,
        customers: Arc::new(PgCustomerRepository::new(pool.clone())),
        sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
        payments,
        pm_state: Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone())),
        refund_state: Arc::new(infrastructure::persistence::PgRefundProcessState::new(
            pool.clone(),
        )),
        mailbox_requeue: Arc::new(infrastructure::persistence::mailbox_lanes::PgMailboxRequeue::new(pool.clone())),
    }
}

/// Enqueue one PM COMMAND row (kind COMMAND, actor lane = the PM's identity).
async fn enqueue_pm(
    pool: &PgPool,
    actor_type: &str,
    actor_id: uuid::Uuid,
    n: u128,
    message_type: &str,
    payload: serde_json::Value,
) -> uuid::Uuid {
    let id = uid(n);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, correlation_id, session_id) \
         VALUES ($1, 'COMMAND', $2, $3, $4, $5, $6, $7, 'GRAPHQL', 'PUBLIC', $1, $8)",
    )
    .bind(id)
    .bind(actor_type)
    .bind(actor_id)
    .bind(actor_client::stable_partition(&actor_id, 5))
    .bind(message_type)
    .bind(&payload)
    .bind(format!("h{n}"))
    .bind(uid(0x5E55))
    .execute(pool)
    .await
    .expect("enqueue");
    id
}

fn place_order_payload(expected_cents: Option<i64>) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "orderId": uid(ORDER),
        "restaurantId": uid(RESTAURANT),
        "cartId": uid(CART),
        "customerId": uid(0xC057),
        "customerContact": {
            "displayName": "Johnny",
            "phone": "+33612345678",
        },
        "serviceType": "COLLECTION",
        "paymentMethodId": "pm_card_visa",
    });
    if let Some(cents) = expected_cents {
        payload["expectedTotal"] = serde_json::json!({ "amountCents": cents, "currency": "EUR" });
    }
    payload
}

async fn worker_over(pool: &PgPool, actor_type: &str, deps: CommandDeps) -> MailboxWorker {
    let w = MailboxWorker::new(
        pool.clone(),
        "w-PM",
        actor_type,
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(deps)),
    );
    w.seed(5).await.expect("seed");
    w.claim().await.expect("claim");
    w
}

/// Drain every owned lane, SURFACING a delivery error (the worker's own `drain` logs-and-retries,
/// which would hide a broken prepare as `delivered == 0`).
async fn drain_all(worker: &MailboxWorker) -> u64 {
    let mut delivered = 0;
    for lane in worker.owned().await {
        delivered += worker.drain_lane(&lane).await.expect("lane drains clean");
    }
    delivered
}

async fn verdict_of(pool: &PgPool, message_id: uuid::Uuid) -> (String, Option<serde_json::Value>) {
    let row = sqlx::query("SELECT status, error FROM inbound_messages WHERE message_id = $1")
        .bind(message_id)
        .fetch_one(pool)
        .await
        .expect("row");
    (row.get("status"), row.get("error"))
}

#[tokio::test]
async fn place_order_commits_intent_pm_row_and_verdict_in_one_transaction() {
    let Some(db) = crate::common::TestDb::acquire("pm_prepare_delivery").await else { return };
    let pool = db.pool();
    seed_checkout_world(&pool, true).await;

    let gateway = Arc::new(StubGateway::default());
    let deps = deps_over(&pool, gateway.clone());
    // 2 × 9.80 EUR — the server-priced total the client confirms.
    let mid = enqueue_pm(
        &pool,
        "PlaceOrderProcess",
        uid(ORDER),
        0x91,
        "PlaceOrder",
        place_order_payload(Some(1960)),
    )
    .await;

    let worker = worker_over(&pool, "PlaceOrderProcess", deps).await;
    assert_eq!(drain_all(&worker).await, 1);

    // The gateway was called exactly once, for the recomputed total, in the PREPARE phase.
    assert_eq!(gateway.intents.lock().unwrap().len(), 1);
    assert_eq!(gateway.intents.lock().unwrap()[0].amount, eur(1960));

    // ONE commit: the Payment birth (cause-chained to the mailbox row) + the PM run row + SUCCEEDED.
    let (status, error) = verdict_of(&pool, mid).await;
    assert_eq!((status.as_str(), error), ("SUCCEEDED", None));
    let event = sqlx::query(
        "SELECT event_type, cause_id FROM domain_events WHERE stream_name = $1 ORDER BY version",
    )
    .bind("Payment-pi_prepare_test")
    .fetch_all(&pool)
    .await
    .expect("payment stream");
    assert_eq!(event.len(), 1);
    assert_eq!(event[0].get::<String, _>("event_type"), "PaymentIntentCreated");
    assert_eq!(event[0].get::<Option<uuid::Uuid>, _>("cause_id"), Some(mid));
    let run = sqlx::query(
        "SELECT order_id, process_status, client_secret, session_id FROM payment_process_manager \
         WHERE cart_id = $1",
    )
    .bind(uid(CART))
    .fetch_one(&pool)
    .await
    .expect("pm row committed with the delivery");
    assert_eq!(run.get::<uuid::Uuid, _>("order_id"), uid(ORDER));
    assert_eq!(run.get::<String, _>("process_status"), "AWAITING_PAYMENT_RESULT");
    assert_eq!(run.get::<Option<String>, _>("client_secret").as_deref(), Some("pi_prepare_test_secret"));
    assert_eq!(run.get::<Option<uuid::Uuid>, _>("session_id"), Some(uid(0x5E55)));
}

#[tokio::test]
async fn deterministic_rejection_commits_rejected_and_writes_nothing() {
    let Some(db) = crate::common::TestDb::acquire("pm_prepare_delivery").await else { return };
    let pool = db.pool();
    seed_checkout_world(&pool, false).await; // cart exists but is EMPTY

    let gateway = Arc::new(StubGateway::default());
    let deps = deps_over(&pool, gateway.clone());
    let mid = enqueue_pm(
        &pool,
        "PlaceOrderProcess",
        uid(ORDER),
        0x92,
        "PlaceOrder",
        place_order_payload(None),
    )
    .await;

    let worker = worker_over(&pool, "PlaceOrderProcess", deps).await;
    assert_eq!(drain_all(&worker).await, 1);

    let (status, error) = verdict_of(&pool, mid).await;
    assert_eq!(status, "REJECTED");
    assert_eq!(
        error.as_ref().and_then(|e| e.get("code")).and_then(|c| c.as_str()),
        Some("CartEmpty"),
        "{error:?}"
    );
    assert!(gateway.intents.lock().unwrap().is_empty(), "rejected before any gateway call");
    let events: i64 =
        sqlx::query("SELECT count(*) AS n FROM domain_events WHERE stream_name LIKE 'Payment-%'")
            .fetch_one(&pool)
            .await
            .expect("count")
            .get("n");
    assert_eq!(events, 0, "no Payment fact for a rejected checkout");
    let runs: i64 = sqlx::query("SELECT count(*) AS n FROM payment_process_manager")
        .fetch_one(&pool)
        .await
        .expect("count")
        .get("n");
    assert_eq!(runs, 0, "no PM run for a rejected checkout");
}

#[tokio::test]
async fn sync_gateway_decline_commits_the_legacy_payment_declined_rejection() {
    let Some(db) = crate::common::TestDb::acquire("pm_prepare_delivery").await else { return };
    let pool = db.pool();
    seed_checkout_world(&pool, true).await;

    // The fail-closed stand-in declines synchronously — the canonical PaymentDeclined.
    let deps = deps_over(&pool, Arc::new(FailClosedPaymentGateway));
    let mid = enqueue_pm(
        &pool,
        "PlaceOrderProcess",
        uid(ORDER),
        0x93,
        "PlaceOrder",
        place_order_payload(None),
    )
    .await;

    let worker = worker_over(&pool, "PlaceOrderProcess", deps).await;
    assert_eq!(drain_all(&worker).await, 1);

    let (status, error) = verdict_of(&pool, mid).await;
    assert_eq!(status, "REJECTED", "byte-identical legacy contract: an operation REJECTION");
    assert_eq!(
        error.as_ref().and_then(|e| e.get("code")).and_then(|c| c.as_str()),
        Some("PaymentDeclined"),
        "{error:?}"
    );
    let events: i64 =
        sqlx::query("SELECT count(*) AS n FROM domain_events WHERE stream_name LIKE 'Payment-%'")
            .fetch_one(&pool)
            .await
            .expect("count")
            .get("n");
    assert_eq!(events, 0, "a declined checkout records nothing");
}

#[tokio::test]
async fn approve_refund_flushes_fact_and_run_row_in_one_commit() {
    let Some(db) = crate::common::TestDb::acquire("pm_prepare_delivery").await else { return };
    let pool = db.pool();

    // GIVEN: a PENDING_APPROVAL refund run with a captured intent.
    use application::pm_state::{RefundProcessRow, RefundProcessStateStore as _};
    infrastructure::persistence::PgRefundProcessState::new(pool.clone())
        .upsert(&RefundProcessRow {
            order_id: OrderId(uid(ORDER)),
            payment_intent_id: PaymentIntentId("pi_prepare_test".into()),
            refund_id: None,
            process_status: RefundProcessStatus::PENDING_APPROVAL,
            approved_amount_cents: None,
            reason: Some("wrong order".into()),
            last_update_utc: chrono::Utc::now(),
        })
        .await
        .expect("seed refund run");

    let gateway = Arc::new(StubGateway::default());
    let deps = deps_over(&pool, gateway.clone());
    let mid = enqueue_pm(
        &pool,
        "RefundProcess",
        uid(ORDER),
        0x94,
        "ApproveRefund",
        serde_json::json!({
            "orderId": uid(ORDER),
            "amount": { "amountCents": 500, "currency": "EUR" },
            "reason": "partial goodwill",
        }),
    )
    .await;

    let worker = worker_over(&pool, "RefundProcess", deps).await;
    assert_eq!(drain_all(&worker).await, 1);

    // The refund gateway was called once, in prepare.
    assert_eq!(gateway.refunds.lock().unwrap().len(), 1);
    assert_eq!(gateway.refunds.lock().unwrap()[0].amount, eur(500));

    // ONE commit: RefundApproved on the Payment stream + the decided run row + SUCCEEDED.
    let (status, error) = verdict_of(&pool, mid).await;
    assert_eq!((status.as_str(), error), ("SUCCEEDED", None));
    let facts = sqlx::query(
        "SELECT event_type, cause_id FROM domain_events WHERE stream_name = $1 ORDER BY version",
    )
    .bind("Payment-pi_prepare_test")
    .fetch_all(&pool)
    .await
    .expect("payment stream");
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].get::<String, _>("event_type"), "RefundApproved");
    assert_eq!(facts[0].get::<Option<uuid::Uuid>, _>("cause_id"), Some(mid));
    let run = sqlx::query(
        "SELECT process_status, approved_amount_cents FROM refund_process_manager WHERE order_id = $1",
    )
    .bind(uid(ORDER))
    .fetch_one(&pool)
    .await
    .expect("run row");
    assert_eq!(run.get::<String, _>("process_status"), "APPROVED_AWAITING_SETTLEMENT");
    assert_eq!(run.get::<Option<i64>, _>("approved_amount_cents"), Some(500));
}

/// The full UC1 second half under B2 (ADR-20260731-203000): the Payment lane records the inbound
/// `PaymentCaptured` AND enqueues the PM-addressed copy in the SAME commit; the PlaceOrderProcess
/// lane then materializes the order from the frozen checkout — durable, fenced, cause-chained.
#[tokio::test]
async fn payment_captured_chains_to_the_pm_lane_and_materializes_the_order() {
    let Some(db) = crate::common::TestDb::acquire("pm_prepare_delivery").await else { return };
    let pool = db.pool();
    seed_checkout_world(&pool, true).await;

    let gateway = Arc::new(StubGateway::default());

    // Leg 1 — the checkout: PlaceOrder through the prepare phase (frozen checkout + PM row).
    enqueue_pm(&pool, "PlaceOrderProcess", uid(ORDER), 0xA1, "PlaceOrder", place_order_payload(None))
        .await;
    let pm_worker = worker_over(&pool, "PlaceOrderProcess", deps_over(&pool, gateway.clone())).await;
    assert_eq!(drain_all(&pm_worker).await, 1);

    // Leg 2 — the inbound Stripe fact on the Payment lane, with B2 chaining ON.
    let payment_actor = actor_client::surrogate_actor_id("Payment", "pi_prepare_test");
    let captured = serde_json::json!({
        "eventType": "PaymentCaptured",
        "payload": {
            "paymentIntentId": "pi_prepare_test",
            "orderId": uid(ORDER),
            "restaurantId": uid(RESTAURANT),
            "amount": { "amountCents": 1960, "currency": "EUR" },
        }
    });
    let fact_id = uid(0xFAC7);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, correlation_id, source, external_id) \
         VALUES ($1, 'EVENT', 'Payment', $2, $3, 'PaymentCaptured', $4, 'hFACT', 'EXTERNAL', \
                 'EXTERNAL', $1, 'stripe', 'evt_1')",
    )
    .bind(fact_id)
    .bind(payment_actor)
    .bind(actor_client::stable_partition(&payment_actor, 5))
    .bind(&captured)
    .execute(&pool)
    .await
    .expect("enqueue payment fact");

    // The Payment lane's worker, WITH B2 chaining on: the recorded fact must chain in-tx.
    let chaining_handler = Arc::new(
        MailboxCommandHandler::new(deps_over(&pool, gateway.clone())).with_pm_fact_chaining(true),
    );
    let chained_worker = MailboxWorker::new(
        pool.clone(),
        "w-PAY",
        "Payment",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        chaining_handler,
    );
    chained_worker.seed(5).await.expect("seed payment lanes");
    chained_worker.claim().await.expect("claim payment lanes");
    assert_eq!(drain_all(&chained_worker).await, 1, "the payment fact delivers");

    // The fact is recorded on the Payment stream AND the chained hop exists — one commit.
    let recorded: i64 = sqlx::query(
        "SELECT count(*) AS n FROM domain_events WHERE stream_name = 'Payment-pi_prepare_test' \
         AND event_type = 'PaymentCaptured'",
    )
    .fetch_one(&pool)
    .await
    .expect("count")
    .get("n");
    assert_eq!(recorded, 1);
    let chained = sqlx::query(
        "SELECT message_id, actor_id, cause_id, status FROM inbound_messages \
         WHERE actor_type = 'PlaceOrderProcess' AND kind = 'EVENT' AND message_type = 'PaymentCaptured'",
    )
    .fetch_one(&pool)
    .await
    .expect("the chained PM copy exists");
    assert_eq!(chained.get::<uuid::Uuid, _>("actor_id"), uid(ORDER), "lane = the order");
    assert_eq!(chained.get::<Option<uuid::Uuid>, _>("cause_id"), Some(fact_id), "cause-chained");
    let chained_id: uuid::Uuid = chained.get("message_id");
    assert_eq!(
        chained_id,
        uuid::Uuid::new_v5(&uid(ORDER), format!("PaymentCaptured:{fact_id}").as_bytes()),
        "deterministic chain identity"
    );

    // Leg 3 — the PM lane delivers the chained copy: OrderPlaced + CartCheckedOut + resolved run.
    pm_worker.claim().await.expect("re-claim pm lanes");
    assert_eq!(drain_all(&pm_worker).await, 1, "the chained hop delivers");

    let order_events: Vec<String> = sqlx::query(
        "SELECT event_type FROM domain_events WHERE stream_name = $1 ORDER BY version",
    )
    .bind(format!("Order-{}", uid(ORDER)))
    .fetch_all(&pool)
    .await
    .expect("order stream")
    .iter()
    .map(|r| r.get("event_type"))
    .collect();
    assert_eq!(order_events, vec!["OrderPlaced".to_string()], "the order materialized");
    let cart_status: Vec<String> = sqlx::query(
        "SELECT event_type FROM domain_events WHERE stream_name = $1 ORDER BY version",
    )
    .bind(format!("Cart-{}", uid(CART)))
    .fetch_all(&pool)
    .await
    .expect("cart stream")
    .iter()
    .map(|r| r.get("event_type"))
    .collect();
    assert!(cart_status.contains(&"CartCheckedOut".to_string()), "{cart_status:?}");
    let run = sqlx::query(
        "SELECT process_status, payment_status, client_secret FROM payment_process_manager WHERE cart_id = $1",
    )
    .bind(uid(CART))
    .fetch_one(&pool)
    .await
    .expect("run row");
    assert_eq!(run.get::<String, _>("process_status"), "ORDER_PLACED");
    assert_eq!(run.get::<String, _>("payment_status"), "CAPTURED");
    assert_eq!(run.get::<Option<String>, _>("client_secret"), None, "spent credential NULLed");

    // Redelivering the chained hop is benign: the run row's expect skips it (IGNORED).
    sqlx::query("UPDATE inbound_messages SET status = 'RECEIVED', completed_at = NULL WHERE message_id = $1")
        .bind(chained_id)
        .execute(&pool)
        .await
        .expect("force redelivery");
    assert_eq!(drain_all(&pm_worker).await, 1);
    let redelivered: String =
        sqlx::query("SELECT status FROM inbound_messages WHERE message_id = $1")
            .bind(chained_id)
            .fetch_one(&pool)
            .await
            .expect("row")
            .get("status");
    assert_eq!(redelivered, "IGNORED", "a re-delivered capture is a benign skip");
}

/// Review CRITICAL-1: a DETERMINISTIC gateway refusal (Stripe 4xx invalid_request /
/// idempotency_error — mapped to a non-catalogued Invariant by the adapter) must land a TERMINAL
/// verdict, never abort-for-retry: a Repository-classed outcome would retry the head row forever
/// and wedge the whole PlaceOrderProcess partition behind one bogus paymentMethodId.
struct RefusingGateway;

#[async_trait]
impl PaymentService for RefusingGateway {
    async fn request(
        &self,
        _input: PaymentRequestInput,
        _meta: &ServiceCallMeta,
    ) -> Result<PaymentRequestOutput, DomainError> {
        Err(DomainError::Invariant(
            "PaymentGatewayRefused: stripe create_payment_intent refused deterministically (HTTP 400, code 'parameter_missing'): No such payment_method".into(),
        ))
    }

    async fn refund(
        &self,
        _input: PaymentRefundInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        unreachable!("not exercised")
    }
}

#[tokio::test]
async fn deterministic_gateway_refusal_is_terminal_never_a_wedged_lane() {
    let Some(db) = crate::common::TestDb::acquire("pm_prepare_delivery").await else { return };
    let pool = db.pool();
    seed_checkout_world(&pool, true).await;

    let deps = deps_over(&pool, Arc::new(RefusingGateway));
    let mid = enqueue_pm(
        &pool,
        "PlaceOrderProcess",
        uid(ORDER),
        0x95,
        "PlaceOrder",
        place_order_payload(None),
    )
    .await;

    let worker = worker_over(&pool, "PlaceOrderProcess", deps).await;
    assert_eq!(drain_all(&worker).await, 1, "the delivery COMPLETES — terminal, not retried");

    let (status, error) = verdict_of(&pool, mid).await;
    assert_eq!(status, "FAILED", "deterministic refusal = terminal FAILED, lane free: {error:?}");
    let events: i64 =
        sqlx::query("SELECT count(*) AS n FROM domain_events WHERE stream_name LIKE 'Payment-%'")
            .fetch_one(&pool)
            .await
            .expect("count")
            .get("n");
    assert_eq!(events, 0);
}

/// Review MAJOR-2: the flip-time backfill enqueues PM-addressed copies of Stripe facts the saga
/// runner never reacted to (recorded pre-flip), idempotently — so no paid order is ever left
/// with nobody told about it across the gate flip.
#[tokio::test]
async fn flip_backfill_enqueues_unreacted_stripe_facts_idempotently() {
    let Some(db) = crate::common::TestDb::acquire("pm_prepare_delivery").await else { return };
    let pool = db.pool();
    seed_checkout_world(&pool, true).await;

    // The pre-flip world: a checkout ran (intent + PM row), Stripe reported the capture, and the
    // fact was RECORDED on the Payment stream — but the runner died before reacting.
    let gateway = Arc::new(StubGateway::default());
    enqueue_pm(&pool, "PlaceOrderProcess", uid(ORDER), 0xB1, "PlaceOrder", place_order_payload(None))
        .await;
    let pm_worker = worker_over(&pool, "PlaceOrderProcess", deps_over(&pool, gateway.clone())).await;
    assert_eq!(drain_all(&pm_worker).await, 1);
    let store = PgEventStore::new(pool.clone());
    let actor = Actor {
        user_id: uid(0xE0),
        user_type: "EXTERNAL".into(),
        domain_id: None,
        correlation_id: uid(0xC1),
        cause_id: None,
    };
    store
        .append(
            "Payment-pi_prepare_test",
            1,
            &[DomainEvent::PaymentCaptured(domain::generated::events::PaymentCaptured {
                payment_intent_id: PaymentIntentId("pi_prepare_test".into()),
                order_id: Some(OrderId(uid(ORDER))),
                restaurant_id: RestaurantId(uid(RESTAURANT)),
                amount: eur(1960),
            })],
            &actor,
        )
        .await
        .expect("record the capture pre-flip");
    // The runner's checkpoint table exists in the full schema; create the minimal one here.
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS projection_checkpoint (\n\
           projector TEXT PRIMARY KEY, position BIGINT NOT NULL, updated_at TIMESTAMPTZ NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("checkpoint table");

    // The flip: backfill (lanes already seeded by worker_over).
    let pm_state = infrastructure::persistence::PgPaymentProcessState::new(pool.clone());
    let enqueued =
        infrastructure::mailbox::backfill_stripe_facts_to_pm_lanes(&pool, &pm_state)
            .await
            .expect("backfill");
    assert_eq!(enqueued, 1, "exactly the un-reacted capture");
    // Idempotent: a restart re-scan collides on the deterministic pk.
    let again = infrastructure::mailbox::backfill_stripe_facts_to_pm_lanes(&pool, &pm_state)
        .await
        .expect("re-backfill");
    assert_eq!(again, 0, "re-scan enqueues nothing");

    // And the backfilled hop DELIVERS: the order materializes.
    pm_worker.claim().await.expect("claim");
    assert_eq!(drain_all(&pm_worker).await, 1);
    let placed: i64 = sqlx::query("SELECT count(*) AS n FROM domain_events WHERE event_type = 'OrderPlaced'")
        .fetch_one(&pool)
        .await
        .expect("count")
        .get("n");
    assert_eq!(placed, 1, "the paid order got told about");
}
