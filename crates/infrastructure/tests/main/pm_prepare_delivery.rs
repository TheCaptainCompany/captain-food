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
    PgRestaurantRepository, PgAuthSubjectReservationRepository, PgSlugReservationRepository, UnverifiedGbpOrderLinkProbe,
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

    async fn capture(
        &self,
        _input: application::generated::services::PaymentCaptureInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        unreachable!("the PM command prepare path never captures (PaymentSettlementProcess's op)")
    }

    async fn release(
        &self,
        _input: application::generated::services::PaymentReleaseInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        unreachable!("the PM command prepare path never voids (PaymentSettlementProcess's op)")
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
        // #639 part C step 2c-i: the rider sign-in door's bridge + support route (not exercised here).
        riders: Arc::new(infrastructure::PgRiderRepository::new(pool.clone())),
        members: Arc::new(infrastructure::PgMemberRepository::new(pool.clone())),
        support_contact: None,
        run_rider_restriction_door: false,
        run_member_access_grant: false,
        run_member_sign_in_door: false,
        run_restaurant_invitation: false,
        store: Arc::new(PgEventStore::new(pool.clone())),
        restaurants: Arc::new(PgRestaurantRepository::new(pool.clone())),
        slugs: Arc::new(PgSlugReservationRepository::new(pool.clone())),
        auth_subjects: Arc::new(PgAuthSubjectReservationRepository::new(pool.clone())),
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
        // RSO-1: the service-hours enforcement gate at its spec default (OFF = shadow).
        enforce_service_hours_guard: false,
        // #167: the acceptance-timeout gate at its spec default (OFF = shadow).
        enforce_acceptance_timeout: false,
        // #588: the Order-lane birth routing at its spec default (OFF = the legacy append).
        route_gates: application::generated::process_managers::RouteGates {
            order_placed_to_order: false,
            place_replacement_order_to_order: false,
            // #807: routed `send:` steps -- OFF, this fixture exercises the birth routes.
            bind_cart_to_customer_to_cart: false,
            grant_customer_credit_to_customer_credit: false,
            mark_order_delivered_to_order: false,
        },
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
    .bind(
        actor_client::declared_lane(actor_type, &actor_id)
            .unwrap_or_else(|| panic!("{actor_type} declares a mailbox")),
    )
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

/// The full UC1 second half under B2 (ADR-20260731-203000), authorize-then-capture form
/// (ADR-20260808-195315 §1.2): the Payment lane records the inbound `PaymentAuthorized` (funds
/// held) AND enqueues the PM-addressed copy in the SAME commit; the PlaceOrderProcess lane then
/// materializes the order from the frozen checkout — durable, fenced, cause-chained. Capture is a
/// LATER fact (fulfilment) with no saga hop.
#[tokio::test]
async fn payment_authorized_chains_to_the_pm_lane_and_materializes_the_order() {
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
    let authorized = serde_json::json!({
        "eventType": "PaymentAuthorized",
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
         VALUES ($1, 'EVENT', 'Payment', $2, $3, 'PaymentAuthorized', $4, 'hFACT', 'EXTERNAL', \
                 'EXTERNAL', $1, 'stripe', 'evt_1')",
    )
    .bind(fact_id)
    .bind(payment_actor)
    .bind(
        actor_client::declared_lane("Payment", &payment_actor)
            .expect("Payment declares a mailbox"),
    )
    .bind(&authorized)
    .execute(&pool)
    .await
    .expect("enqueue payment fact");

    // The Payment lane's worker: the recorded fact must chain its PM copy in-tx (B2 — unconditional
    // since #242 Runtime D retired the gate that used to make it optional).
    let chaining_handler = Arc::new(MailboxCommandHandler::new(deps_over(&pool, gateway.clone())));
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
         AND event_type = 'PaymentAuthorized'",
    )
    .fetch_one(&pool)
    .await
    .expect("count")
    .get("n");
    assert_eq!(recorded, 1);
    let chained = sqlx::query(
        "SELECT message_id, actor_id, cause_id, status FROM inbound_messages \
         WHERE actor_type = 'PlaceOrderProcess' AND kind = 'EVENT' AND message_type = 'PaymentAuthorized'",
    )
    .fetch_one(&pool)
    .await
    .expect("the chained PM copy exists");
    assert_eq!(chained.get::<uuid::Uuid, _>("actor_id"), uid(ORDER), "lane = the order");
    assert_eq!(chained.get::<Option<uuid::Uuid>, _>("cause_id"), Some(fact_id), "cause-chained");
    let chained_id: uuid::Uuid = chained.get("message_id");
    assert_eq!(
        chained_id,
        uuid::Uuid::new_v5(&uid(ORDER), format!("PaymentAuthorized:{fact_id}").as_bytes()),
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
    assert_eq!(run.get::<String, _>("payment_status"), "AUTHORIZED");
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
    assert_eq!(redelivered, "IGNORED", "a re-delivered authorization is a benign skip");
}

// ================================================================================================
// #588 — the Order-lane birth enqueue (ADR-20260816-040239)
//
// The `deliver: OrderPlaced to: Order` step is a TELL: the saga decides the birth, the Order's own
// lane appends it. These three tests are the red-first proof, authored against the PRE-CHANGE HEAD
// where the PM still calls `Repository::save` on the foreign `Order-{id}` stream.
//
// Why the lane matters and not just the append: `record_inbound_order_placed` is what returns the
// `Recorded` verdict `apply_schedules_in_tx` keys the acceptance deadline on. A birth that never
// rides the lane leaves the clock unarmed for every real order — a paid order nobody is told about.
// ================================================================================================

/// Fetch the single Order-lane birth message, or fail loudly with how many there were.
async fn birth_row(pool: &PgPool) -> sqlx::postgres::PgRow {
    let rows = sqlx::query(
        "SELECT message_id, actor_id, partition, cause_id, status, source, external_id, payload \
         FROM inbound_messages WHERE actor_type = 'Order' AND message_type = 'OrderPlaced'",
    )
    .fetch_all(pool)
    .await
    .expect("query the Order lane");
    assert_eq!(
        rows.len(),
        1,
        "exactly ONE Order-lane birth message must exist after the authorized checkout \
         (found {}) — the `deliver: OrderPlaced to: Order` step is a lane ENQUEUE, not a \
         foreign-stream append (ADR-20260816-040239)",
        rows.len()
    );
    rows.into_iter().next().unwrap()
}

/// [`deps_over`] with the #588 birth routing ON.
///
/// The ONE knob these tests turn, and turning it is the point: ADR-20260816-040239 ships the
/// routing behind `configuration.yaml#/ROUTE_ORDER_BIRTH_THROUGH_LANE`, default OFF, because it
/// changes a money-path saga's commit path (gate-then-stabilize — rollback must be a flip, not a
/// redeploy). The flag-OFF path is not left unproven: it is exactly what
/// `payment_authorized_chains_to_the_pm_lane_and_materializes_the_order` above asserts, on plain
/// [`deps_over`] — the saga appends `OrderPlaced` itself and no Order-lane message exists. So the
/// two postures are covered by two tests, and neither is a weakened version of the other.
fn routed_deps(pool: &PgPool, payments: Arc<dyn PaymentService>) -> CommandDeps {
    CommandDeps {
        // #639 part C step 2c-i: the rider sign-in door's bridge + support route (not exercised here).
        riders: Arc::new(infrastructure::PgRiderRepository::new(pool.clone())),
        members: Arc::new(infrastructure::PgMemberRepository::new(pool.clone())),
        support_contact: None,
        run_rider_restriction_door: false,
        run_member_access_grant: false,
        run_member_sign_in_door: false,
        run_restaurant_invitation: false,
        route_gates: application::generated::process_managers::RouteGates {
            order_placed_to_order: true,
            place_replacement_order_to_order: false,
            // #807: routed `send:` steps -- OFF, this fixture exercises the birth routes.
            bind_cart_to_customer_to_cart: false,
            grant_customer_credit_to_customer_credit: false,
            mark_order_delivered_to_order: false,
        },
        ..deps_over(pool, payments)
    }
}

/// An Order-lane worker with the reminder windows wired — a missing window aborts every delivery
/// on the lane by design, so the acceptance clock cannot be observed without them.
async fn order_worker(pool: &PgPool, id: &str, deps: CommandDeps) -> MailboxWorker {
    let windows = std::collections::HashMap::from([
        ("ORDER_ACCEPTANCE_TIMEOUT_SECONDS", std::time::Duration::from_secs(300)),
        ("ORDER_RETENTION_WINDOW_DAYS", std::time::Duration::from_secs(30 * 86_400)),
    ]);
    let w = MailboxWorker::new(
        pool.clone(),
        id,
        "Order",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(deps).with_reminder_windows(windows)),
    );
    w.seed(5).await.expect("seed order lanes");
    w.claim().await.expect("claim order lanes");
    w
}

/// Run the checkout up to and including the PM leg that decides the birth: `PlaceOrder` (prepare,
/// intent + run row) → the inbound Stripe `PaymentAuthorized` on the Payment lane (recorded +
/// chained) → the `PlaceOrderProcess` lane delivering the chained copy. Returns
/// `(pm_worker, chained_message_id)`.
async fn checkout_through_authorization(
    pool: &PgPool,
    gateway: Arc<StubGateway>,
    stripe_event: &str,
) -> (MailboxWorker, uuid::Uuid) {
    enqueue_pm(pool, "PlaceOrderProcess", uid(ORDER), 0xC1, "PlaceOrder", place_order_payload(None))
        .await;
    let pm_worker = worker_over(pool, "PlaceOrderProcess", routed_deps(pool, gateway.clone())).await;
    assert_eq!(drain_all(&pm_worker).await, 1, "the checkout leg delivers");

    let payment_actor = actor_client::surrogate_actor_id("Payment", "pi_prepare_test");
    let authorized = serde_json::json!({
        "eventType": "PaymentAuthorized",
        "payload": {
            "paymentIntentId": "pi_prepare_test",
            "orderId": uid(ORDER),
            "restaurantId": uid(RESTAURANT),
            "amount": { "amountCents": 1960, "currency": "EUR" },
        }
    });
    let fact_id = uid(0xFAC8);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, correlation_id, source, external_id) \
         VALUES ($1, 'EVENT', 'Payment', $2, $3, 'PaymentAuthorized', $4, 'hFACT588', 'EXTERNAL', \
                 'EXTERNAL', $1, 'stripe', $5)",
    )
    .bind(fact_id)
    .bind(payment_actor)
    .bind(
        actor_client::declared_lane("Payment", &payment_actor)
            .expect("Payment declares a mailbox"),
    )
    .bind(&authorized)
    .bind(stripe_event)
    .execute(pool)
    .await
    .expect("enqueue the authorization");

    let pay_worker = MailboxWorker::new(
        pool.clone(),
        "w-PAY588",
        "Payment",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(routed_deps(pool, gateway.clone()))),
    );
    pay_worker.seed(5).await.expect("seed payment lanes");
    pay_worker.claim().await.expect("claim payment lanes");
    assert_eq!(drain_all(&pay_worker).await, 1, "the authorization delivers");

    let chained_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT message_id FROM inbound_messages WHERE actor_type = 'PlaceOrderProcess' \
         AND kind = 'EVENT' AND message_type = 'PaymentAuthorized'",
    )
    .fetch_one(pool)
    .await
    .expect("the chained PM copy exists");

    pm_worker.claim().await.expect("re-claim pm lanes");
    assert_eq!(drain_all(&pm_worker).await, 1, "the saga leg delivers");
    (pm_worker, chained_id)
}

/// **#588 / beck's named red.** The normal checkout path must ENQUEUE the birth on the Order lane
/// and arm the acceptance clock off the lane's own `Recorded` verdict. Three claims, in order:
///
/// (a) the saga's `deliver:` step produces exactly ONE `inbound_messages` row
///     `('Order','OrderPlaced')`, committed in the SAME transaction as the `payment_process_manager`
///     row — both or neither — and appends NOTHING to `Order-{id}` itself (vernon: being the birth
///     AUTHORITY licenses the decision, never the append). Its door identity is derived from the
///     ORDER ID, never from the trigger's message id (`inbound_message_id(source, external_id)`,
///     FROZEN like `surrogate_actor_id`), so redelivery dedups at the door.
/// (b) after an EXPLICIT lane drain (never a sleep) the verdict is the `Recorded` arm — SUCCEEDED,
///     with the birth on the stream exactly once. This holds on FIRST delivery only; the
///     redelivery case is `redelivered_authorization_dedups_the_birth_at_the_door`.
/// (c) EXACTLY ONE acceptance-deadline row exists, SCHEDULED, caused by THAT birth message and due
///     one window after it — not merely "a row exists".
///
/// Deviation from the dispatch's letter, stated on purpose: `apply_schedules_in_tx` computes
/// `chrono::Utc::now() + window` inside the delivery transaction, not `occurred_at + window`
/// (`crates/infrastructure/src/mailbox/mod.rs`). So the deadline is asserted against a BRACKET the
/// test itself takes around the drain — `scheduled_at ∈ [t0 + window, t1 + window]`, which is exact
/// under any CI pause where a fixed tolerance would eventually flake — PLUS the exact `cause_id`
/// link, the stronger half of "keyed to that birth" anyway, which cannot pass by coincidence.
#[tokio::test]
async fn authorized_payment_enqueues_the_order_birth_and_arms_the_clock() {
    let Some(db) = crate::common::TestDb::acquire("pm_prepare_delivery").await else { return };
    let pool = db.pool();
    seed_checkout_world(&pool, true).await;

    let gateway = Arc::new(StubGateway::default());
    let (_pm_worker, chained_id) =
        checkout_through_authorization(&pool, gateway, "evt_588_a").await;

    // --- (a) the birth is an ENQUEUE, and the saga wrote no foreign stream -----------------------
    let direct: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM domain_events WHERE stream_name = $1 AND event_type = 'OrderPlaced'",
    )
    .bind(format!("Order-{}", uid(ORDER)))
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(
        direct, 0,
        "PlaceOrderProcess must NOT append OrderPlaced to the Order's stream: the birth passes \
         the Order's own mailbox, which is the serialization point for its writer. Today the \
         saga saves Order-{{id}} AND Cart-{{id}} in ONE transaction (vernon, #588)"
    );

    let birth = birth_row(&pool).await;
    // Both or neither, asserted about the COMMIT and not about row counts (beck): Postgres exposes
    // the inserting transaction id as the system column `xmin`, so two rows written by the same
    // transaction share it. If the door insert ever moves off the passed `&mut Transaction` onto a
    // pool handle — the exact mistake vernon's constraint (ii) exists to prevent, and one no count
    // of rows can see — the two xmins diverge and this goes red.
    let (run_status, run_xmin): (String, i64) = sqlx::query_as(
        "SELECT process_status, xmin::text::bigint FROM payment_process_manager WHERE cart_id = $1",
    )
    .bind(uid(CART))
    .fetch_one(&pool)
    .await
    .expect("the run row committed with the delivery");
    assert_eq!(run_status, "ORDER_PLACED", "the saga leg resolved its run");
    let birth_xmin: i64 = sqlx::query_scalar(
        "SELECT xmin::text::bigint FROM inbound_messages WHERE message_id = $1",
    )
    .bind(birth.get::<uuid::Uuid, _>("message_id"))
    .fetch_one(&pool)
    .await
    .expect("the birth row's inserting transaction");
    assert_eq!(
        birth_xmin, run_xmin,
        "the birth message and the run row were written by the SAME transaction — the door insert \
         rides the delivery's `&mut Transaction`, never `self.deps`' pool (ADR-20260816-040239 \
         constraint 2)"
    );
    assert_eq!(birth.get::<uuid::Uuid, _>("actor_id"), uid(ORDER), "lane = the Order");
    assert_eq!(
        birth.get::<Option<uuid::Uuid>, _>("cause_id"),
        Some(chained_id),
        "cause-chained to the saga hop that decided it"
    );
    let source: String = birth.get::<Option<String>, _>("source").expect("a door row has a source");
    let external_id: String =
        birth.get::<Option<String>, _>("external_id").expect("a door row has an external_id");
    assert_eq!(
        external_id,
        uid(ORDER).to_string(),
        "external_id MUST be the ORDER ID — deriving it from the trigger's message id mints a \
         fresh identity on every redelivery and births the order twice (vernon, FROZEN)"
    );
    assert_eq!(
        birth.get::<uuid::Uuid, _>("message_id"),
        actor_client::inbound_message_id(&source, &external_id),
        "the door identity is UUIDv5(source:external_id) — FROZEN, like surrogate_actor_id"
    );
    assert_ne!(
        birth.get::<uuid::Uuid, _>("message_id"),
        actor_client::inbound_message_id(&source, &chained_id.to_string()),
        "and it is NOT derived from the trigger's message id"
    );

    // --- (b) explicit drain → the canonical Recorded arm -----------------------------------------
    let worker = order_worker(&pool, "w-ORD588a", routed_deps(&pool, Arc::new(StubGateway::default())))
        .await;
    // Bracket the drain: `apply_schedules_in_tx` computes `now() + window` INSIDE the delivery
    // transaction, so the deadline must land in `[t0 + window, t1 + window]` — deterministic under
    // any CI pause, where a fixed tolerance is a flake waiting for a slow runner (beck).
    let t0 = chrono::Utc::now();
    assert_eq!(drain_all(&worker).await, 1, "the Order lane delivers the birth");
    let t1 = chrono::Utc::now();
    let birth_id: uuid::Uuid = birth.get("message_id");
    let (status, error) = verdict_of(&pool, birth_id).await;
    assert_eq!(
        (status.as_str(), error),
        ("SUCCEEDED", None),
        "first delivery = record_inbound_order_placed's `Recorded` arm, never `AlreadyRecorded` \
         (no dependency on #590's verdict-blind arm)"
    );
    let placed = sqlx::query(
        "SELECT occurred_at FROM domain_events WHERE stream_name = $1 AND event_type = 'OrderPlaced'",
    )
    .bind(format!("Order-{}", uid(ORDER)))
    .fetch_all(&pool)
    .await
    .expect("order stream");
    assert_eq!(placed.len(), 1, "one birth on the stream, appended by the Order's own lane");
    let occurred_at: chrono::DateTime<chrono::Utc> = placed[0].get("occurred_at");

    // --- (c) exactly one acceptance deadline, keyed to THAT birth --------------------------------
    let deadlines = sqlx::query(
        "SELECT message_id, status, scheduled_at, cause_id FROM inbound_messages \
         WHERE actor_id = $1 AND message_type = 'OrderAcceptanceTimedOut'",
    )
    .bind(uid(ORDER))
    .fetch_all(&pool)
    .await
    .expect("query the deadline");
    assert_eq!(deadlines.len(), 1, "exactly ONE pending occurrence per (actor, purpose)");
    assert_eq!(deadlines[0].get::<String, _>("status"), "SCHEDULED");
    assert_eq!(
        deadlines[0].get::<uuid::Uuid, _>("message_id"),
        application::reminders::reminder_message_id(uid(ORDER), "OrderAcceptanceTimedOut")
    );
    assert_eq!(
        deadlines[0].get::<Option<uuid::Uuid>, _>("cause_id"),
        Some(birth_id),
        "the clock is armed BY the birth delivery, not by some other route"
    );
    let scheduled_at: chrono::DateTime<chrono::Utc> = deadlines[0].get("scheduled_at");
    let window = chrono::Duration::seconds(300);
    assert!(
        (t0 + window..=t1 + window).contains(&scheduled_at),
        "due one ORDER_ACCEPTANCE_TIMEOUT_SECONDS window after the birth delivery: expected \
         within [{}, {}], got {scheduled_at} (the birth appended at {occurred_at})",
        t0 + window,
        t1 + window
    );
}

/// **#588 mutation kill — redelivery.** Deliver the authorization TWICE. Because the door identity
/// is `UUIDv5(source:order_id)`, the second saga pass collides on the primary key: ONE birth
/// message, ONE birth on the stream, and the acceptance deadline does NOT move (`reschedule: keep`).
/// A duplicate enqueue is a SUCCESS outcome to the saga, never an error — the run must not fail or
/// wedge its lane because the door deduped.
///
/// Mutants this kills: deriving the door id from the trigger's message id; treating the dedup as a
/// PM error; turning the `Keep` arm's `ON CONFLICT DO NOTHING` into `DO UPDATE`. It is also the
/// producer-driven replacement for the hand-INSERT `enqueue_birth` crutch in
/// `mailbox_acceptance_timeout.rs` — that crutch exists ONLY because no producer did this, and it
/// is deleted in the same change (leaving it would keep the producer-free path green forever).
#[tokio::test]
async fn redelivered_authorization_dedups_the_birth_at_the_door() {
    let Some(db) = crate::common::TestDb::acquire("pm_prepare_delivery").await else { return };
    let pool = db.pool();
    seed_checkout_world(&pool, true).await;

    let gateway = Arc::new(StubGateway::default());
    let (pm_worker, chained_id) =
        checkout_through_authorization(&pool, gateway, "evt_588_b").await;

    let birth = birth_row(&pool).await;
    let birth_id: uuid::Uuid = birth.get("message_id");

    // Arm the clock through the Order lane, then park the deadline at a time NO fresh declaration
    // would compute — a Keep→InPlace mutation becomes an unmissable seven-day move, not jitter.
    let worker = order_worker(&pool, "w-ORD588b", routed_deps(&pool, Arc::new(StubGateway::default())))
        .await;
    assert_eq!(drain_all(&worker).await, 1);
    let deadline_id = application::reminders::reminder_message_id(uid(ORDER), "OrderAcceptanceTimedOut");
    let sentinel = chrono::Utc::now() + chrono::Duration::days(7);
    sqlx::query("UPDATE inbound_messages SET scheduled_at = $2 WHERE message_id = $1")
        .bind(deadline_id)
        .bind(sentinel)
        .execute(&pool)
        .await
        .expect("park the sentinel");

    // MUTATION: the same authorization is delivered a second time (at-least-once is the contract).
    sqlx::query(
        "UPDATE inbound_messages SET status = 'RECEIVED', completed_at = NULL WHERE message_id = $1",
    )
    .bind(chained_id)
    .execute(&pool)
    .await
    .expect("force saga redelivery");
    pm_worker.claim().await.expect("re-claim pm lanes");
    assert_eq!(drain_all(&pm_worker).await, 1, "the redelivered hop completes — never a wedged lane");

    let again = birth_row(&pool).await;
    assert_eq!(
        again.get::<uuid::Uuid, _>("message_id"),
        birth_id,
        "the redelivered saga pass re-derives the SAME door identity and dedups at the door"
    );

    // Drain again: whatever the door left, the stream still holds exactly one birth.
    worker.claim().await.expect("re-claim order lanes");
    let _ = drain_all(&worker).await;
    let placed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM domain_events WHERE stream_name = $1 AND event_type = 'OrderPlaced'",
    )
    .bind(format!("Order-{}", uid(ORDER)))
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(placed, 1, "one birth, never two — commands.rs's absorber stays authoritative");

    let after: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT scheduled_at FROM inbound_messages WHERE message_id = $1")
            .bind(deadline_id)
            .fetch_one(&pool)
            .await
            .expect("the deadline row survives");
    assert_eq!(
        after.timestamp_micros(),
        sentinel.timestamp_micros(),
        "reschedule: keep — the FIRST scheduled_at wins; a redelivered birth must never push the \
         acceptance deadline out"
    );
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM inbound_messages WHERE actor_id = $1 \
         AND message_type = 'OrderAcceptanceTimedOut'",
    )
    .bind(uid(ORDER))
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(pending, 1, "one pending occurrence per (actor, purpose)");
}

/// **A refused checkout leaves nothing behind — ONE load-bearing half, one negative control, and
/// they are not the same thing.** The gateway refuses the checkout, so the `PlaceOrder` leg throws
/// and the delivery transaction rolls back.
///
/// - `runs == 0` is the **mutation kill**: the leg reached `payment_process_manager` staging before
///   it threw, so a flush that did not roll back with the verdict shows up here. Both or neither.
/// - `births == 0` is a **negative control**, and stays one in both flag states: this leg carries no
///   routed `deliver:` at all, so no code path could have written a birth row to begin with. Kept
///   because a future leg that DID enqueue here would be caught, not because it proves something
///   today.
///
/// **It does NOT fence "the enqueue is never in `prepare`"** (ADR-20260816-040239 constraint 1),
/// and the claim that it did was deleted with #597. It could never have: the refusal fails the
/// *`PlaceOrder`* leg, so `on_payment_authorized` — the only leg carrying the routed `deliver:` —
/// never runs, in either flag state. That constraint is carried elsewhere, in two pieces: the
/// COMPILER refuses an anonymous `lanes: Some(..)` field write (`TriggerEnvelope` is private-field
/// and alone in its own module), and the GUARD
/// `trigger_envelope_laned_call_sites_are_audited` holds the remaining hole — `prepare` calling
/// `TriggerEnvelope::laned(..)` compiles, because `application` cannot name a transaction to demand
/// as proof (ADR-20260803-234035: compiler first, a check where types cannot reach). Neither piece
/// is a thing a delivery test could have provided.
#[tokio::test]
async fn a_refused_checkout_enqueues_no_birth_and_leaves_no_run_row() {
    let Some(db) = crate::common::TestDb::acquire("pm_prepare_delivery").await else { return };
    let pool = db.pool();
    seed_checkout_world(&pool, true).await;

    // MUTATION: the payment service fails, so the delivery transaction rolls back.
    let deps = deps_over(&pool, Arc::new(RefusingGateway));
    let mid = enqueue_pm(
        &pool,
        "PlaceOrderProcess",
        uid(ORDER),
        0xC2,
        "PlaceOrder",
        place_order_payload(None),
    )
    .await;
    let worker = worker_over(&pool, "PlaceOrderProcess", deps).await;
    assert_eq!(drain_all(&worker).await, 1);
    assert_eq!(verdict_of(&pool, mid).await.0, "FAILED");

    let births: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM inbound_messages WHERE actor_type = 'Order' \
         AND message_type = 'OrderPlaced'",
    )
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(
        births, 0,
        "no birth message for a checkout that never authorized — the rolled-back delivery must \
         leave the door as empty as it found it"
    );
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM payment_process_manager")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(runs, 0, "and no run row — both or neither");
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
        // Built by the SHARED builder the real adapter uses (#623), not a hand-written literal:
        // a stub that restates a message format is a stub that keeps passing after the format
        // moves. The provider prose here deliberately carries a key-shaped marker — the row
        // asserted below must not contain it.
        Err(application::ports::payment_gateway_refused(
            Some(400),
            "stripe create_payment_intent refused deterministically (code 'parameter_missing'): \
             No such payment_method (sk_test_STUB_MARKER)",
        ))
    }

    async fn capture(
        &self,
        _input: application::generated::services::PaymentCaptureInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        unreachable!("not exercised")
    }

    async fn release(
        &self,
        _input: application::generated::services::PaymentReleaseInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        unreachable!("not exercised")
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

    // #623 — THE ROW ITSELF, read back out of Postgres. The canary and the two-seam test assert on
    // `verdict_of_error`'s output; this is the only place that proves what `completion.rs` actually
    // BINDS to `inbound_messages.error`, which is the column that persists for the retention window
    // and is served as `Operation.errorCode`.
    let error = error.expect("a FAILED delivery records an error");
    assert_eq!(error.get("code").and_then(|c| c.as_str()), Some("Internal"), "{error}");
    let context = error.get("context").expect("the row carries a context");
    assert_eq!(
        context.get("seam").and_then(|v| v.as_str()),
        Some("PAYMENT_GATEWAY"),
        "an operator reading this row must be able to tell 'Stripe is refusing us' from 'our \
         database is wedged' — that was the whole of #623\n{error}"
    );
    assert_eq!(
        context.get("reason").and_then(|v| v.as_str()),
        Some("GATEWAY_REFUSED"),
        "{error}"
    );
    assert_eq!(context.get("gatewayStatus").and_then(|v| v.as_i64()), Some(400), "{error}");
    assert!(
        !error.to_string().contains("sk_"),
        "the provider's prose reached the durable journal row\n{error}"
    );

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

    // The pre-flip world: a checkout ran (intent + PM row), Stripe reported the AUTHORIZATION
    // (authorize-then-capture, ADR-20260808-195315 §1.2), and the fact was RECORDED on the
    // Payment stream — but the runner died before reacting.
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
            &[DomainEvent::PaymentAuthorized(domain::generated::events::PaymentAuthorized {
                payment_intent_id: PaymentIntentId("pi_prepare_test".into()),
                order_id: Some(OrderId(uid(ORDER))),
                restaurant_id: RestaurantId(uid(RESTAURANT)),
                amount: eur(1960),
            })],
            &actor,
        )
        .await
        .expect("record the authorization pre-flip");
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
    assert_eq!(enqueued, 1, "exactly the un-reacted authorization");
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
    assert_eq!(placed, 1, "the authorized order got told about");
}

// ================================================================================================
// #596 — the chained hop's lane WIDTH comes from the DECLARATION, never from the seeded registry
//
// `chain_pm_copy_in_tx` derived the target actor's keyspace width from
// `SELECT count(*) FROM mailbox_partitions WHERE actor_type = $1`. Two failures ride on that one
// read, and both are on the money path:
//
// 1. **Zero seeded rows => the completion transaction ERRORS.** The authorization's record and its
//    saga hop never commit, so the order is never born: a paid order nobody is told about, the
//    worst failure mode this product has. Migrations do not seed the registry -- only a worker
//    START does -- so "no rows for the target actor" is reachable by simply not running that
//    actor's worker, and after the #358 per-bin cutover it stops being a seconds-long startup
//    window and becomes indefinite: a Payment bin can run while the PlaceOrderProcess bin is not
//    deployed at all.
// 2. **A PARTIALLY seeded actor => a silently different keyspace**, which is a ONE-WRITER
//    violation and not a queueing nuisance. The lease is keyed by LANE (`actor_runtime/src/lease.rs`
//    `Lane { actor_type, partition, .. }`) and `completion.rs` fences on the LANE's checkpoint, so
//    `stable_partition(actor_id, width)` is the ONLY thing mapping an aggregate to exactly one
//    lane. Two producers disagreeing on the width put the same `Order-{id}` in two lanes, each with
//    a live lease, each passing its own fence -- the mailbox's serialisation promise breaks at the
//    addressing function, upstream of anything a fence can see.
//
// Why the existing suite was blind to both: every PM-chain test above calls `worker.seed(5)` and
// every declared width IS 5, so the seeded count and the declared count agree and the two
// implementations are indistinguishable. These two tests break that coincidence on purpose.
// ================================================================================================

/// The inbound Stripe `PaymentAuthorized` fact on the Payment lane (funds held for `ORDER`).
async fn enqueue_authorization_fact(pool: &PgPool, n: u128, external_id: &str) -> uuid::Uuid {
    let payment_actor = actor_client::surrogate_actor_id("Payment", "pi_prepare_test");
    let authorized = serde_json::json!({
        "eventType": "PaymentAuthorized",
        "payload": {
            "paymentIntentId": "pi_prepare_test",
            "orderId": uid(ORDER),
            "restaurantId": uid(RESTAURANT),
            "amount": { "amountCents": 1960, "currency": "EUR" },
        }
    });
    let fact_id = uid(n);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, correlation_id, source, external_id) \
         VALUES ($1, 'EVENT', 'Payment', $2, $3, 'PaymentAuthorized', $4, 'hFACT', 'EXTERNAL', \
                 'EXTERNAL', $1, 'stripe', $5)",
    )
    .bind(fact_id)
    .bind(payment_actor)
    .bind(
        actor_client::declared_lane("Payment", &payment_actor)
            .expect("Payment declares a mailbox"),
    )
    .bind(&authorized)
    .bind(external_id)
    .execute(pool)
    .await
    .expect("enqueue payment fact");
    fact_id
}

/// A Payment-lane worker: the one that records the Stripe fact and chains the PM copy in-tx.
async fn payment_worker(pool: &PgPool, gateway: Arc<StubGateway>) -> MailboxWorker {
    let w = MailboxWorker::new(
        pool.clone(),
        "w-PAY",
        "Payment",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(deps_over(pool, gateway))),
    );
    w.seed(5).await.expect("seed payment lanes");
    w.claim().await.expect("claim payment lanes");
    w
}

/// [`drain_all`], but RETURNING the completion error instead of panicking on it: the test below
/// asserts the ABSENCE of one, and `.expect()`'s panic would bury which error it actually was.
async fn try_drain_all(worker: &MailboxWorker) -> Result<u64, actor_runtime::CompletionError> {
    let mut delivered = 0;
    for lane in worker.owned().await {
        delivered += worker.drain_lane(&lane).await?;
    }
    Ok(delivered)
}

/// Remove the target actor's registry rows -- the deployment where its worker has never started.
///
/// The checkout leg above had to seed them (a worker cannot claim an unseeded lane), so the state
/// is reached by deletion here; in production it is simply the default, because MIGRATIONS DO NOT
/// SEED `mailbox_partitions` -- `MailboxWorker::seed` at worker startup is the only writer.
async fn unseed(pool: &PgPool, actor_type: &str) -> u64 {
    sqlx::query("DELETE FROM mailbox_partitions WHERE actor_type = $1")
        .bind(actor_type)
        .execute(pool)
        .await
        .expect("unseed the target actor")
        .rows_affected()
}

/// Leg 1 of both tests: the checkout, so the saga has an `AWAITING_PAYMENT_RESULT` run row for the
/// authorization to resolve. Returns after the PlaceOrderProcess worker has drained it.
async fn checkout_leg(pool: &PgPool, gateway: Arc<StubGateway>, n: u128) {
    enqueue_pm(pool, "PlaceOrderProcess", uid(ORDER), n, "PlaceOrder", place_order_payload(None))
        .await;
    let worker = worker_over(pool, "PlaceOrderProcess", deps_over(pool, gateway)).await;
    assert_eq!(drain_all(&worker).await, 1, "the checkout leg commits its intent and run row");
}

/// **The paid order with no birth.** Stripe authorizes; the target saga actor has NOT started, so
/// its registry is empty. The completion transaction must COMMIT -- the fact recorded, the hop
/// enqueued at the DECLARED partition -- and the hop must simply WAIT until a worker starts.
#[tokio::test]
async fn chained_fact_is_enqueued_with_no_seeded_lanes_and_drains_once_the_worker_starts() {
    let Some(db) = crate::common::TestDb::acquire("pm_prepare_delivery").await else { return };
    let pool = db.pool();
    seed_checkout_world(&pool, true).await;
    let gateway = Arc::new(StubGateway::default());

    checkout_leg(&pool, gateway.clone(), 0xD1).await;
    assert_eq!(
        unseed(&pool, "PlaceOrderProcess").await,
        5,
        "the checkout leg had seeded the declared five lanes; the state under test is zero"
    );

    // Leg 2 -- Stripe's authorization on the Payment lane, target lane unseeded.
    let fact_id = enqueue_authorization_fact(&pool, 0xFAC8, "evt_596_unseeded").await;
    let pay = payment_worker(&pool, gateway.clone()).await;
    let drained = try_drain_all(&pay).await;
    assert!(
        drained.is_ok(),
        "the authorization's completion transaction must COMMIT while the target actor has no \
         seeded lanes -- erroring here is a held payment whose order is never born: {:?}",
        drained.err()
    );
    assert_eq!(drained.unwrap(), 1, "the payment fact delivers");

    let recorded: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM domain_events WHERE stream_name = 'Payment-pi_prepare_test' \
         AND event_type = 'PaymentAuthorized'",
    )
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(recorded, 1, "the fact is on the Payment stream -- the money moved, and we know it");

    let chained = sqlx::query(
        "SELECT message_id, actor_id, partition, status, cause_id FROM inbound_messages \
         WHERE actor_type = 'PlaceOrderProcess' AND kind = 'EVENT' \
           AND message_type = 'PaymentAuthorized'",
    )
    .fetch_one(&pool)
    .await
    .expect("the chained PM copy exists even with an unseeded target");
    assert_eq!(chained.get::<uuid::Uuid, _>("actor_id"), uid(ORDER), "lane = the order");
    assert_eq!(chained.get::<Option<uuid::Uuid>, _>("cause_id"), Some(fact_id), "cause-chained");
    assert_eq!(
        chained.get::<i16, _>("partition"),
        actor_client::declared_lane("PlaceOrderProcess", &uid(ORDER))
            .expect("PlaceOrderProcess declares a mailbox"),
        "addressed by the DECLARED width (ACTOR_MAILBOXES), never by a registry with no rows"
    );
    assert_eq!(
        chained.get::<String, _>("status"),
        "RECEIVED",
        "the hop WAITS for its worker -- waiting is the correct behaviour, failing is not"
    );

    // Leg 3 -- the worker starts (seeds + claims) and the waiting hop drains. This also pins the
    // other half of the startup drift check: an EMPTY registry is a first boot, not drift, so
    // `seed` succeeds here. Getting that backwards would turn every fresh environment into a
    // crash loop.
    let pm_worker = worker_over(&pool, "PlaceOrderProcess", deps_over(&pool, gateway)).await;
    assert_eq!(drain_all(&pm_worker).await, 1, "the waiting hop delivers the moment a worker starts");
    let placed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM domain_events WHERE stream_name = $1 AND event_type = 'OrderPlaced'",
    )
    .bind(format!("Order-{}", uid(ORDER)))
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(placed, 1, "the authorized order got told about");
    let status: String = sqlx::query_scalar(
        "SELECT process_status FROM payment_process_manager WHERE cart_id = $1",
    )
    .bind(uid(CART))
    .fetch_one(&pool)
    .await
    .expect("run row");
    assert_eq!(status, "ORDER_PLACED");
}

/// **The one-writer violation.** A partially seeded actor yields a non-zero width SMALLER than the
/// declared one, so the hop is stamped into a keyspace no other producer shares -- the same
/// aggregate reachable through two leases. The declared width is the only correct answer.
#[tokio::test]
async fn a_partially_seeded_actor_still_routes_to_the_declared_partition() {
    let Some(db) = crate::common::TestDb::acquire("pm_prepare_delivery").await else { return };
    let pool = db.pool();

    // GUARD THE DATA FIRST. If the declared lane fell INSIDE the lanes left seeded below, a
    // narrowed producer could stamp it too and the assertion further down would be vacuous --
    // which is exactly how the existing suite (every seed 5, every declared width 5) stayed green
    // over the defect for as long as it did.
    //
    // Stated as `declared >= SEEDED_LANES` rather than against a second, hand-computed lane
    // (#609). Be precise about what is universal here, because the difference is the difference
    // between this guard and a vacuous one: the IMPLICATION holds for every id -- a producer that
    // thinks the keyspace is SEEDED_LANES wide can only ever stamp `0..SEEDED_LANES`, so ANY
    // declared lane at or above it is unreachable under the narrowed keyspace. The ASSERTION does
    // not: `declared` is one of {0,1,2,3,4} and two of those five would fail it, so this line is
    // still falsifiable on the id under test and still has to be re-checked if `ORDER` changes.
    // What it buys over `declared != stable_partition(id, 2)` is that it names the SEEDED KEYSPACE
    // the row must avoid rather than one arbitrary lane inside it, and needs no second opinion
    // about the width in test code at all. This test asserts the ABSENCE of a misroute; it never
    // stamps one, so it never needs to be able to compute one.
    const SEEDED_LANES: i16 = 2;
    let declared = actor_client::declared_lane("PlaceOrderProcess", &uid(ORDER))
        .expect("PlaceOrderProcess declares a mailbox");
    assert!(
        declared >= SEEDED_LANES,
        "test data must place the declared lane ({declared}) OUTSIDE the {SEEDED_LANES} lanes left \
         seeded, or it proves nothing"
    );

    seed_checkout_world(&pool, true).await;
    let gateway = Arc::new(StubGateway::default());
    checkout_leg(&pool, gateway.clone(), 0xD2).await;

    // THE STATE UNDER TEST: 2 of the declared 5 lanes present. Reachable in production from a
    // half-applied width change, a partial seed failure, or an older binary seeding a narrower
    // width -- `seed_partitions` is `ON CONFLICT DO NOTHING`, so it never deletes and the registry
    // can lag the declaration in either direction.
    let removed = sqlx::query(
        "DELETE FROM mailbox_partitions WHERE actor_type = 'PlaceOrderProcess' AND partition >= $1",
    )
    .bind(SEEDED_LANES)
    .execute(&pool)
    .await
    .expect("partially unseed")
    .rows_affected();
    assert_eq!(removed, 3, "{SEEDED_LANES} of the declared lanes remain");

    let _ = enqueue_authorization_fact(&pool, 0xFAC9, "evt_596_partial").await;
    let pay = payment_worker(&pool, gateway.clone()).await;
    assert_eq!(try_drain_all(&pay).await.expect("the payment fact delivers"), 1);

    let partition: i16 = sqlx::query_scalar(
        "SELECT partition FROM inbound_messages WHERE actor_type = 'PlaceOrderProcess' \
         AND kind = 'EVENT' AND message_type = 'PaymentAuthorized'",
    )
    .fetch_one(&pool)
    .await
    .expect("the chained PM copy exists");
    assert_eq!(
        partition, declared,
        "the hop must land on the DECLARED lane ({declared}), not on one of the {SEEDED_LANES} \
         lanes the seeded row count happens to describe -- two widths for one aggregate is two \
         leases for one writer"
    );

    // And the STARTUP DRIFT CHECK refuses to paper over the partial registry: a worker brought up
    // against 2 of the declared 5 lanes must not come up at all. Papering over it is what let the
    // two keyspaces coexist in the first place.
    let stuck = MailboxWorker::new(
        pool.clone(),
        "w-DRIFT",
        "PlaceOrderProcess",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(deps_over(&pool, gateway.clone()))),
    );
    let refused = stuck.seed(5).await.expect_err("a partial registry must refuse the start");
    let refused = refused.to_string();
    assert!(refused.contains("DRIFT for 'PlaceOrderProcess'"), "{refused}");
    assert!(
        refused.contains("NOT the 'not seeded yet' case"),
        "the message must distinguish drift from a first boot, or an operator reads it as a \
         crash loop and clears the registry of a live system: {refused}"
    );

    // Once the stale rows are cleared per the cutover procedure, the declared lane drains it.
    unseed(&pool, "PlaceOrderProcess").await;
    let pm_worker = worker_over(&pool, "PlaceOrderProcess", deps_over(&pool, gateway)).await;
    assert_eq!(drain_all(&pm_worker).await, 1, "the declared lane owns the hop");
    let placed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM domain_events WHERE stream_name = $1 AND event_type = 'OrderPlaced'",
    )
    .bind(format!("Order-{}", uid(ORDER)))
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(placed, 1, "the authorized order got told about");
}

/// **The BACKFILL with the target unseeded** — review blocker B3.
///
/// `flip_backfill_enqueues_unreacted_stripe_facts_idempotently` above proves the backfill works,
/// but it runs with the PM lanes already seeded (its own comment says so), so it carries exactly
/// the coincidence this branch indicts: reverting the backfill to the seeded `count(*)` read left
/// all 84 infrastructure tests green. That made the site with the LOUDEST justification in the ADR
/// — "a rescue pass for facts nobody reacted to, running at startup, that refused to run when the
/// system was cold" — the one site with no negative verification at all.
///
/// This is the cold case, and it is the realistic one: the backfill runs AT STARTUP, which is
/// precisely when another actor's worker has not seeded yet. Under the old code it returned
/// `Err("has no seeded lanes — seed before backfilling")` and the rescue never happened.
#[tokio::test]
async fn backfill_enqueues_to_the_declared_partition_with_the_target_unseeded() {
    let Some(db) = crate::common::TestDb::acquire("pm_prepare_delivery").await else { return };
    let pool = db.pool();
    seed_checkout_world(&pool, true).await;
    let gateway = Arc::new(StubGateway::default());

    // A checkout ran and Stripe's authorization was RECORDED, but the runner never reacted to it.
    checkout_leg(&pool, gateway.clone(), 0xB2).await;
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
            &[DomainEvent::PaymentAuthorized(domain::generated::events::PaymentAuthorized {
                payment_intent_id: PaymentIntentId("pi_prepare_test".into()),
                order_id: Some(OrderId(uid(ORDER))),
                restaurant_id: RestaurantId(uid(RESTAURANT)),
                amount: eur(1960),
            })],
            &actor,
        )
        .await
        .expect("record the authorization pre-flip");
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS projection_checkpoint (\n\
           projector TEXT PRIMARY KEY, position BIGINT NOT NULL, updated_at TIMESTAMPTZ NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("checkpoint table");

    // THE STATE UNDER TEST, and the difference from the test above: the target actor has NO
    // registry rows when the rescue pass runs.
    assert_eq!(unseed(&pool, "PlaceOrderProcess").await, 5, "the target is now cold");

    let pm_state = infrastructure::persistence::PgPaymentProcessState::new(pool.clone());
    let enqueued = infrastructure::mailbox::backfill_stripe_facts_to_pm_lanes(&pool, &pm_state)
        .await
        .expect(
            "the backfill must RESCUE the un-reacted authorization with the target lane unseeded \
             -- refusing here is the rescue pass declining to run at exactly the moment it exists \
             for, leaving a paid order with nobody told about it",
        );
    assert_eq!(enqueued, 1, "exactly the un-reacted authorization");

    let partition: i16 = sqlx::query_scalar(
        "SELECT partition FROM inbound_messages WHERE actor_type = 'PlaceOrderProcess' \
         AND kind = 'EVENT' AND message_type = 'PaymentAuthorized'",
    )
    .fetch_one(&pool)
    .await
    .expect("the backfilled copy exists");
    assert_eq!(
        partition,
        actor_client::declared_lane("PlaceOrderProcess", &uid(ORDER))
            .expect("PlaceOrderProcess declares a mailbox"),
        "stamped with the DECLARED width, so the worker that eventually starts drains it"
    );

    // And it delivers once a worker comes up -- the rescue completes.
    let pm_worker = worker_over(&pool, "PlaceOrderProcess", deps_over(&pool, gateway)).await;
    assert_eq!(drain_all(&pm_worker).await, 1, "the rescued hop delivers");
    let placed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM domain_events WHERE stream_name = $1 AND event_type = 'OrderPlaced'",
    )
    .bind(format!("Order-{}", uid(ORDER)))
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(placed, 1, "the authorized order got told about");
}
