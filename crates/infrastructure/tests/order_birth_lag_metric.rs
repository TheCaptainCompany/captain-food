//! `order_birth_lag_ms{routed="true"}` SEEN RECORDED — beck's never-seen-red closure (#758, C1a of
//! ADR-20260829-230418; flip evidence for `ROUTE_ORDER_BIRTH_THROUGH_LANE`, dispatch card 598 §7/§9).
//!
//! The emitter has been live since #588/#598 (`flush.rs::record_order_birth_lag`, sole call site
//! `handler.rs`'s inbound-fact `Recorded` arm) and is **silent by design while the flag is OFF** —
//! so until this binary existed, no test had ever observed the histogram RECORD a point, and
//! disconnecting the emitter redded nothing. This suite drives the FULL routed checkout through the
//! real lanes — `PlaceOrder` on the `PlaceOrderProcess` COMMAND lane → the inbound Stripe
//! `PaymentAuthorized` on the `Payment` lane (recorded + PM copy chained in one commit) → the saga
//! leg staging the Order-lane birth ENQUEUE → the Order's own lane worker appending the birth — and
//! asserts the series at each seam:
//!
//! - **the enqueue is not the emission**: after the saga leg commits the birth *message*, the
//!   histogram has ZERO points (the handover has not happened yet, and no `routed="false"` point
//!   fires anywhere on the routed path);
//! - **the lane delivery is**: after the Order lane drains, EXACTLY ONE measurement exists, with
//!   `routed="true"` and a sane value (`0 ≤ lag < 300 000 ms` — the mailbox row's `received_at` is
//!   the enqueue commit, so anything above minutes means the clock, not the lane);
//! - **a redelivery measures nothing**: the absorbed `AlreadyRecorded` arm (verdict IGNORED)
//!   emits no point — a redelivery that appended nothing must not report "however long ago the
//!   first delivery was" (`flush.rs`'s own contract).
//!
//! **Its own binary, ONE `#[tokio::test]`**: `telemetry::meters` binds the process-global meter
//! once (`OnceLock`), so the spy provider must be installed before the FIRST metric call — the
//! `mailbox_liveness_metrics.rs` / `orders_placed_metric.rs` / `authorized_no_birth_metric.rs`
//! constraint, unchanged.
//!
//! **SEEN RED** (the emitter disconnected — the semantic mutant, never a line range): delete the
//! `super::record_order_birth_lag(...)` call at its sole `handler.rs` call site and the routed
//! assertion dies on `left: []` vs one `routed="true"` measurement. Red output is quoted in PR
//! #761's body; the mutant was applied to the committed tree and reverted with `git checkout --`.
//!
//! Needs `DATABASE_URL`: since #474 a missing database FAILS this suite; only an explicit
//! `DB_TESTS_REQUIRED=0` skips it, and that leaves a receipt (`crates/db_test_gate`).

#[path = "main/common.rs"]
mod common;
#[path = "main/spy_meter.rs"]
mod spy_meter;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use actor_runtime::{MailboxWorker, WorkerConfig};
use application::generated::services::{
    IdentityService, PaymentCaptureInput, PaymentRefundInput, PaymentReleaseInput,
    PaymentRequestInput, PaymentRequestOutput, PaymentService, ServiceCallMeta,
};
use application::ports::{Actor, EventStore};
use async_trait::async_trait;
use domain::generated::entities::{Address, CartLineItem, Money};
use domain::generated::events::{
    CartLineAdded, CartStarted, DomainEvent, RestaurantActivated, RestaurantRegistered,
};
use domain::generated::scalars::*;
use domain::shared::errors::DomainError;
use infrastructure::generated::command_router::CommandDeps;
use infrastructure::mailbox::MailboxCommandHandler;
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, PgCustomerRepository,
    PgEventStore, PgProspectionRepository, PgRestaurantRepository, PgAuthSubjectReservationRepository, PgSlugReservationRepository,
    UnverifiedGbpOrderLinkProbe,
};
use sqlx::PgPool;
use telemetry::contract::metric;

const RESTAURANT: u128 = 0x0E57;
const CART: u128 = 0xCA47;
const ORDER: u128 = 0x0AD1;
const OFFER: u128 = 0x0FFE;

fn uid(n: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128(n)
}
fn eur(cents: i64) -> Money {
    Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
}

/// Fixed-price catalog stub: the only offer is `OFFER` at 9.80 EUR (pricing is exercised, the
/// catalog projection is not what this suite tests).
struct StubCatalog;

#[async_trait]
impl application::queries::CatalogReadRepository for StubCatalog {
    async fn by_restaurant(
        &self,
        _restaurant_id: RestaurantId,
    ) -> Result<Option<application::queries::CatalogRow>, DomainError> {
        Ok(None)
    }

    async fn offer_by_id(
        &self,
        _restaurant_id: RestaurantId,
        offer_id: OfferId,
    ) -> Result<Option<application::queries::OfferView>, DomainError> {
        if offer_id != OfferId(uid(OFFER)) {
            return Ok(None);
        }
        Ok(Some(application::queries::OfferView {
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

/// Fixed-intent gateway: one checkout, one intent id the chain correlates on.
#[derive(Default)]
struct StubGateway {
    intents: Mutex<Vec<PaymentRequestInput>>,
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
            payment_intent_id: PaymentIntentId("pi_758".into()),
            client_secret: "pi_758_secret".into(),
        })
    }
    async fn capture(&self, _i: PaymentCaptureInput, _m: &ServiceCallMeta) -> Result<(), DomainError> {
        unreachable!("the checkout path never captures")
    }
    async fn release(&self, _i: PaymentReleaseInput, _m: &ServiceCallMeta) -> Result<(), DomainError> {
        unreachable!("the checkout path never voids")
    }
    async fn refund(&self, _i: PaymentRefundInput, _m: &ServiceCallMeta) -> Result<(), DomainError> {
        unreachable!("the checkout path never refunds")
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
async fn seed_checkout_world(pool: &PgPool) {
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
    store
        .append(
            &format!("Cart-{}", uid(CART)),
            0,
            &[
                DomainEvent::CartStarted(CartStarted {
                    cart_id: CartId(uid(CART)),
                    restaurant_id: RestaurantId(uid(RESTAURANT)),
                    session_id: SessionId(uid(0x5E55)),
                    customer_id: None,
                }),
                DomainEvent::CartLineAdded(CartLineAdded {
                    cart_id: CartId(uid(CART)),
                    line: CartLineItem {
                        cart_line_id: CartLineId(uid(0x11E)),
                        offer_id: OfferId(uid(OFFER)),
                        quantity: 2,
                        selected_option_ids: Vec::new(),
                    },
                }),
            ],
            &actor,
        )
        .await
        .expect("cart stream");
}

/// [`CommandDeps`] with the #588 birth routing **ON** — the whole point of this binary: the
/// histogram records only on the routed path, so the routed path is what gets driven.
fn routed_deps(pool: &PgPool, payments: Arc<dyn PaymentService>) -> CommandDeps {
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
        catalogs: Arc::new(StubCatalog),
        auth: Arc::new(FailClosedIdentityService) as Arc<dyn IdentityService>,
        customers: Arc::new(PgCustomerRepository::new(pool.clone())),
        sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
        payments,
        pm_state: Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone())),
        refund_state: Arc::new(infrastructure::persistence::PgRefundProcessState::new(pool.clone())),
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

async fn worker_over(pool: &PgPool, actor_type: &str, deps: CommandDeps) -> MailboxWorker {
    let w = MailboxWorker::new(
        pool.clone(),
        &format!("w-758-{actor_type}"),
        actor_type,
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(deps)),
    );
    w.seed(5).await.expect("seed");
    w.claim().await.expect("claim");
    w
}

/// An Order-lane worker with the reminder windows wired — a missing window aborts every delivery
/// on the lane by design.
async fn order_worker(pool: &PgPool, deps: CommandDeps) -> MailboxWorker {
    let windows = std::collections::HashMap::from([
        ("ORDER_ACCEPTANCE_TIMEOUT_SECONDS", std::time::Duration::from_secs(300)),
        ("ORDER_RETENTION_WINDOW_DAYS", std::time::Duration::from_secs(30 * 86_400)),
    ]);
    let w = MailboxWorker::new(
        pool.clone(),
        "w-758-Order",
        "Order",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(deps).with_reminder_windows(windows)),
    );
    w.seed(5).await.expect("seed order lanes");
    w.claim().await.expect("claim order lanes");
    w
}

/// Drain every owned lane, SURFACING a delivery error (the worker's own `drain` logs-and-retries,
/// which would hide a broken leg as `delivered == 0` and turn a metric assertion into a mystery).
async fn drain_all(worker: &MailboxWorker) -> u64 {
    let mut delivered = 0;
    for lane in worker.owned().await {
        delivered += worker.drain_lane(&lane).await.expect("lane drains clean");
    }
    delivered
}

#[tokio::test]
async fn a_routed_birth_records_exactly_one_routed_true_lag_point() {
    // The spy FIRST — before the database, before anything can bind the process-wide meter.
    let spy = spy_meter::SpyMeter::install();
    let Some(db) = common::TestDb::acquire("order_birth_lag_metric").await else { return };
    let pool = db.pool();
    seed_checkout_world(&pool).await;

    let gateway = Arc::new(StubGateway::default());

    // ── the full routed checkout, through the real lanes ────────────────────────────────────────
    // Leg 1: PlaceOrder on the PlaceOrderProcess COMMAND lane (prepare: intent + run row).
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, correlation_id, session_id) \
         VALUES ($1, 'COMMAND', 'PlaceOrderProcess', $2, $3, 'PlaceOrder', $4, 'h758a', 'GRAPHQL', \
                 'PUBLIC', $1, $5)",
    )
    .bind(uid(0x91))
    .bind(uid(ORDER))
    .bind(
        actor_client::declared_lane("PlaceOrderProcess", &uid(ORDER))
            .expect("PlaceOrderProcess declares a mailbox"),
    )
    .bind(serde_json::json!({
        "orderId": uid(ORDER),
        "restaurantId": uid(RESTAURANT),
        "cartId": uid(CART),
        "customerId": uid(0xC057),
        "customerContact": { "displayName": "Johnny", "phone": "+33612345678" },
        "serviceType": "COLLECTION",
        "paymentMethodId": "pm_card_visa",
    }))
    .bind(uid(0x5E55))
    .execute(&pool)
    .await
    .expect("enqueue PlaceOrder");
    let pm_worker = worker_over(&pool, "PlaceOrderProcess", routed_deps(&pool, gateway.clone())).await;
    assert_eq!(drain_all(&pm_worker).await, 1, "the checkout leg delivers");
    assert_eq!(gateway.intents.lock().unwrap().len(), 1, "one intent minted");

    // Leg 2: the inbound Stripe authorization on the Payment lane — recorded + PM copy chained.
    let payment_actor = actor_client::surrogate_actor_id("Payment", "pi_758");
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, correlation_id, source, external_id) \
         VALUES ($1, 'EVENT', 'Payment', $2, $3, 'PaymentAuthorized', $4, 'h758b', 'EXTERNAL', \
                 'EXTERNAL', $1, 'stripe', 'evt_758')",
    )
    .bind(uid(0xFAC7))
    .bind(payment_actor)
    .bind(actor_client::declared_lane("Payment", &payment_actor).expect("Payment declares a mailbox"))
    .bind(serde_json::json!({
        "eventType": "PaymentAuthorized",
        "payload": {
            "paymentIntentId": "pi_758",
            "orderId": uid(ORDER),
            "restaurantId": uid(RESTAURANT),
            "amount": { "amountCents": 1960, "currency": "EUR" },
        }
    }))
    .execute(&pool)
    .await
    .expect("enqueue the authorization");
    let pay_worker = worker_over(&pool, "Payment", routed_deps(&pool, gateway.clone())).await;
    assert_eq!(drain_all(&pay_worker).await, 1, "the authorization delivers");

    // Leg 3: the saga leg delivers the chained copy and stages the Order-lane birth ENQUEUE.
    pm_worker.claim().await.expect("re-claim pm lanes");
    assert_eq!(drain_all(&pm_worker).await, 1, "the saga leg delivers");
    let birth_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT message_id FROM inbound_messages WHERE actor_type = 'Order' \
         AND message_type = 'OrderPlaced'",
    )
    .fetch_one(&pool)
    .await
    .expect("exactly one Order-lane birth message exists");

    // ── the ENQUEUE is not the emission ─────────────────────────────────────────────────────────
    // Three legs ran, the birth message exists, the birth is NOT appended yet: the histogram has
    // ZERO points — and in particular no routed="false" point fired anywhere on the routed path.
    let staged = spy.drain();
    assert_eq!(
        staged.points(metric::ORDER_BIRTH_LAG_MS),
        vec![],
        "no birth-lag point before the Order lane delivers: the handover is enqueue -> Recorded, \
         and the enqueue alone has measured nothing"
    );

    // ── the LANE DELIVERY is ────────────────────────────────────────────────────────────────────
    let order = order_worker(&pool, routed_deps(&pool, gateway.clone())).await;
    assert_eq!(drain_all(&order).await, 1, "the Order lane delivers the birth");
    let appended: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM domain_events WHERE stream_name = $1 AND event_type = 'OrderPlaced'",
    )
    .bind(format!("Order-{}", uid(ORDER)))
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(appended, 1, "the Order's own lane appended the birth exactly once");

    let recorded = spy.drain();
    let routed_true: BTreeMap<String, String> =
        BTreeMap::from([("routed".to_string(), "true".to_string())]);
    assert_eq!(
        recorded.records(metric::ORDER_BIRTH_LAG_MS),
        vec![(routed_true.clone(), 1)],
        "EXACTLY ONE measurement, routed=\"true\" -- the flip evidence beck named: the histogram \
         has now been SEEN to record, not merely to exist (#598's never-seen-red closure)"
    );
    let points = recorded.points(metric::ORDER_BIRTH_LAG_MS);
    assert_eq!(points.len(), 1, "one point set for one delivery: {points:?}");
    let (attrs, lag_ms) = &points[0];
    assert_eq!(attrs, &routed_true);
    assert!(
        (0.0..300_000.0).contains(lag_ms),
        "a sane handover lag: now - received_at of a row enqueued moments ago must be \
         0 <= lag < 300000 ms, got {lag_ms}"
    );
    // Walk evidence for the flip decision (dispatch card 598 §9; run with --nocapture to see it).
    eprintln!("WALK EVIDENCE: order_birth_lag_ms{{routed=\"true\"}} recorded {lag_ms} ms");

    // ── a REDELIVERY measures nothing ───────────────────────────────────────────────────────────
    // The absorbed arm (AlreadyRecorded -> IGNORED) appends nothing, so it must not report a lag
    // of "however long ago the first delivery was" (flush.rs's own contract).
    sqlx::query(
        "UPDATE inbound_messages SET status = 'RECEIVED', completed_at = NULL WHERE message_id = $1",
    )
    .bind(birth_id)
    .execute(&pool)
    .await
    .expect("force birth redelivery");
    order.claim().await.expect("re-claim order lanes");
    assert_eq!(drain_all(&order).await, 1, "the redelivered birth completes");
    let verdict: String =
        sqlx::query_scalar("SELECT status FROM inbound_messages WHERE message_id = $1")
            .bind(birth_id)
            .fetch_one(&pool)
            .await
            .expect("row");
    assert!(
        matches!(verdict.as_str(), "IGNORED" | "DUPLICATE"),
        "an absorbed arm — the runtime's payload-hash dedup (DUPLICATE) or the aggregate's \
         AlreadyRecorded skip (IGNORED); either way nothing was appended: {verdict}"
    );
    let still_one: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM domain_events WHERE stream_name = $1 AND event_type = 'OrderPlaced'",
    )
    .bind(format!("Order-{}", uid(ORDER)))
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(still_one, 1, "one birth on the stream, before and after the redelivery");
    assert_eq!(
        spy.drain().points(metric::ORDER_BIRTH_LAG_MS),
        vec![],
        "a redelivery that appended nothing measured nothing"
    );
}
