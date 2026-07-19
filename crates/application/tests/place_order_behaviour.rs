//! BEHAVIOUR tests for the PlaceOrderProcess COMMAND leg (`placeOrder` → PaymentIntentCreated) — the
//! executable form of the `specs/tests.yaml` PlaceOrderProcess cases whose `when` is the PlaceOrder
//! command (ADR-0032: each test cites the `specs/rules.yaml` rule it asserts).
//!
//! Pure and offline: an in-memory [`EventStore`] (holding the Restaurant, Cart and Order streams the
//! saga touches), a fake `CartReadRepository` standing in for the priced Cart projection, and a fake
//! `PaymentGateway` (declines `pm_declined`, accepts anything else as `pi_123`). The event-driven saga
//! legs (`PaymentCaptured` → OrderPlaced + CartCheckedOut, `PaymentFailed` → abort — tests.yaml
//! TestPlaceOrderPaymentCapturedPlacesOrder / TestPlaceOrderPaymentFailedPlacesNothing) need the
//! process-manager runtime + the Stripe webhook ACL and are a documented TODO(saga) in
//! `application::commands::place_order` — NOT asserted here.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use application::commands::{place_order, rejection_code};
use application::ports::{version_conflict, Actor, CreatedPaymentIntent, EventStore, PaymentGateway};
use application::queries::{CartReadRepository, CartRow};
use domain::generated::commands::PlaceOrder;
use domain::generated::entities::{
    Address, CartLineItem, CheckoutSnapshot, CustomerContact, Money, PaymentBreakdown,
};
use domain::generated::events::{
    CartLineAdded, CartStarted, DomainEvent, PaymentIntentCreated, RestaurantAcceptanceModeChanged,
    RestaurantActivated, RestaurantRegistered,
};
use domain::generated::scalars::*;
use domain::shared::errors::DomainError;

// ------------------------------------------------------------------------------------------------
// Test doubles
// ------------------------------------------------------------------------------------------------

/// In-memory [`EventStore`]: version = number of events on the stream, same optimistic-concurrency
/// semantics as `PgEventStore` (a clash → the canonical `version_conflict`).
#[derive(Default)]
struct MemStore {
    streams: Mutex<HashMap<String, Vec<DomainEvent>>>,
}

impl MemStore {
    /// GIVEN: pre-seed a stream with already-recorded facts.
    fn seed(&self, stream: &str, events: Vec<DomainEvent>) {
        self.streams.lock().unwrap().insert(stream.to_string(), events);
    }

    /// THEN: the full stream after the command ran.
    fn stream(&self, stream: &str) -> Vec<DomainEvent> {
        self.streams.lock().unwrap().get(stream).cloned().unwrap_or_default()
    }
}

#[async_trait]
impl EventStore for MemStore {
    async fn append(
        &self,
        stream_name: &str,
        expected_version: i64,
        events: &[DomainEvent],
        _actor: &Actor,
    ) -> Result<i64, DomainError> {
        let mut streams = self.streams.lock().unwrap();
        let stream = streams.entry(stream_name.to_string()).or_default();
        if stream.len() as i64 != expected_version {
            return Err(version_conflict(stream_name, expected_version));
        }
        stream.extend(events.iter().cloned());
        Ok(stream.len() as i64)
    }

    async fn load(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError> {
        let events = self.stream(stream_name);
        let version = events.len() as i64;
        Ok((events, version))
    }
}

/// Fake priced Cart projection: `by_id` answers from at most one configured row (the total the saga
/// prices the PaymentIntent from — server-computed, never trusted from the client).
#[derive(Default)]
struct FakeCarts {
    row: Option<CartRow>,
}

#[async_trait]
impl CartReadRepository for FakeCarts {
    async fn by_customer(&self, _customer_id: CustomerId) -> Result<Vec<CartRow>, DomainError> {
        Ok(self.row.clone().into_iter().collect())
    }

    async fn by_id(&self, id: CartId) -> Result<Option<CartRow>, DomainError> {
        Ok(self.row.clone().filter(|r| r.cart_id == id))
    }
}

/// Fake Stripe gateway: `pm_declined` is declined synchronously (the canonical
/// `errors.yaml#/PaymentDeclined` rejection per the PaymentGateway contract); anything else yields the
/// fixture intent `pi_123`.
struct FakeGateway;

#[async_trait]
impl PaymentGateway for FakeGateway {
    async fn create_payment_intent(
        &self,
        _amount: &Money,
        payment_method_id: &str,
    ) -> Result<CreatedPaymentIntent, DomainError> {
        if payment_method_id == "pm_declined" {
            return Err(DomainError::Invariant("PaymentDeclined: card_declined".into()));
        }
        Ok(CreatedPaymentIntent {
            payment_intent_id: PaymentIntentId("pi_123".into()),
            client_secret: "pi_123_secret".into(),
        })
    }
}

// ------------------------------------------------------------------------------------------------
// Fixtures (tests.yaml `fixtures`, with UUIDs instead of the sample string ids)
// ------------------------------------------------------------------------------------------------

fn actor() -> Actor {
    Actor {
        user_id: uuid::Uuid::new_v4(),
        user_type: 1, // UserType::CUSTOMER ordinal — checkout requires the verified customer
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
    }
}

fn address() -> Address {
    Address {
        line1: AddressLine("1 Rue Nationale".into()),
        line2: None,
        postal_code: PostalCode("37000".into()),
        city: CityName("Tours".into()),
        country: CountryCode("FR".into()),
    }
}

fn restaurant_stream(id: RestaurantId) -> String {
    format!("Restaurant-{}", id.0)
}
fn cart_stream(id: CartId) -> String {
    format!("Cart-{}", id.0)
}
fn order_stream(id: OrderId) -> String {
    format!("Order-{}", id.0)
}

/// Fixture `restaurantRegistered`, parameterized on `mode` (None/LIVE vs TEST, ADR-0038).
fn registered(id: RestaurantId, mode: Option<Mode>) -> DomainEvent {
    DomainEvent::RestaurantRegistered(RestaurantRegistered {
        mode,
        restaurant_id: id,
        account_id: None,
        listing_status: RestaurantListingStatus::ACTIVE_PARTNER,
        r#ref: None,
        external_identifiers: vec![],
        slug: Slug("chez-marco".into()),
        display_name: RestaurantDisplayName("Chez Marco".into()),
        contact: None,
        website: None,
        tags: vec![],
        margin_rate: None,
        cuisine_category: None,
        uber_prices_opt_in: None,
        address: address(),
        location: None,
        timezone: Some(TimeZone("Europe/Paris".into())),
        preparation_time_minutes: None,
        opening_hours: vec![],
    })
}

fn activated(id: RestaurantId) -> DomainEvent {
    DomainEvent::RestaurantActivated(RestaurantActivated { restaurant_id: id, reason: None })
}

fn paused(id: RestaurantId) -> DomainEvent {
    DomainEvent::RestaurantAcceptanceModeChanged(RestaurantAcceptanceModeChanged {
        restaurant_id: id,
        mode: OrderAcceptanceMode::PAUSED,
    })
}

/// Fixtures `cartStarted` + `cartLineAdded` — an OPEN cart holding one line.
fn open_cart_with_line(cart: CartId, resto: RestaurantId) -> Vec<DomainEvent> {
    vec![
        DomainEvent::CartStarted(CartStarted { cart_id: cart, restaurant_id: resto, customer_id: None }),
        DomainEvent::CartLineAdded(CartLineAdded {
            cart_id: cart,
            line: CartLineItem {
                cart_line_id: CartLineId(uuid::Uuid::new_v4()),
                offer_id: OfferId(uuid::Uuid::new_v4()),
                quantity: 2,
                selected_option_ids: vec![],
            },
        }),
    ]
}

/// The priced `cart` projection row the saga reads the server-computed total from (19.60 EUR).
fn priced_row(cart: CartId, resto: RestaurantId) -> CartRow {
    CartRow {
        cart_id: cart,
        restaurant_id: resto,
        customer_id: None,
        status: CartStatus::OPEN,
        lines: serde_json::json!([]),
        total_amount_cents: MoneyCents(1960),
        currency: CurrencyCode("EUR".into()),
        estimated_breakdown: None,
        uber_comparison: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

/// The `PlaceOrder` fixture command (tests.yaml TestPlaceOrderCreatesPaymentIntent data).
fn place_cmd(order: OrderId, resto: RestaurantId, cart: CartId) -> PlaceOrder {
    PlaceOrder {
        mode: None,
        order_id: order,
        restaurant_id: resto,
        cart_id: cart,
        customer_id: None,
        customer_contact: CustomerContact {
            display_name: CustomerDisplayName("Johnny".into()),
            email: None,
            phone: PhoneNumber("+33612345678".into()),
        },
        service_type: ServiceType::DELIVERY,
        delivery_address: Some(address()),
        note: None,
        payment_method_id: "pm_123".into(),
    }
}

/// The checkout snapshot PlaceOrderProcess freezes onto PaymentIntentCreated (best-available breakdown;
/// items empty until server-side line pricing lands — mirrors `application::commands::place_order`).
fn checkout_snapshot(order: OrderId, resto: RestaurantId, cart: CartId) -> CheckoutSnapshot {
    let eur = |c: i64| Money { amount_cents: MoneyCents(c), currency: CurrencyCode("EUR".into()) };
    CheckoutSnapshot {
        order_id: order,
        cart_id: cart,
        restaurant_id: resto,
        customer_id: None,
        mode: None,
        r#ref: None,
        customer_contact: CustomerContact {
            display_name: CustomerDisplayName("Johnny".into()),
            email: None,
            phone: PhoneNumber("+33612345678".into()),
        },
        service_type: ServiceType::DELIVERY,
        delivery_address: None,
        items: Vec::new(),
        total_amount: eur(1960),
        breakdown: PaymentBreakdown {
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
    }
}

fn oid() -> OrderId {
    OrderId(uuid::Uuid::new_v4())
}
fn rid() -> RestaurantId {
    RestaurantId(uuid::Uuid::new_v4())
}
fn cid() -> CartId {
    CartId(uuid::Uuid::new_v4())
}

/// GIVEN an ACTIVE restaurant + an OPEN one-line cart, priced in the projection.
fn checkout_given(store: &MemStore, resto: RestaurantId, cart: CartId) -> FakeCarts {
    store.seed(&restaurant_stream(resto), vec![registered(resto, None), activated(resto)]);
    store.seed(&cart_stream(cart), open_cart_with_line(cart, resto));
    FakeCarts { row: Some(priced_row(cart, resto)) }
}

// ------------------------------------------------------------------------------------------------
// Happy path (rules.yaml#/CheckoutPricesCartCreatesPaymentIntent)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestPlaceOrderCreatesPaymentIntent —
/// rules.yaml#/CheckoutPricesCartCreatesPaymentIntent
#[tokio::test]
async fn checkout_prices_the_open_cart_and_creates_a_payment_intent() {
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let carts = checkout_given(&store, resto, cart);

    let intent = place_order(&store, &carts, &FakeGateway, place_cmd(order, resto, cart), &actor())
        .await
        .expect("checkout");

    assert_eq!(intent.payment_intent_id, PaymentIntentId("pi_123".into()));
    assert_eq!(intent.client_secret, "pi_123_secret");
    let events = store.stream(&order_stream(order));
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        DomainEvent::PaymentIntentCreated(e)
            if e.payment_intent_id == PaymentIntentId("pi_123".into())
                && e.restaurant_id == resto
                && e.amount.amount_cents == MoneyCents(1960)
    ));
    // The cart stays OPEN: CartCheckedOut only lands on payment capture (the TODO(saga) leg).
    assert_eq!(store.stream(&cart_stream(cart)).len(), 2);
}

/// Client-generated orderId: replaying the checkout for the same order id is absorbed instead of
/// duplicating the saga's first fact — rules.yaml#/CheckoutPricesCartCreatesPaymentIntent.
#[tokio::test]
async fn replaying_the_checkout_for_the_same_order_is_absorbed() {
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let carts = checkout_given(&store, resto, cart);
    store.seed(
        &order_stream(order),
        vec![DomainEvent::PaymentIntentCreated(PaymentIntentCreated {
            payment_intent_id: PaymentIntentId("pi_123".into()),
            restaurant_id: resto,
            customer_id: None,
            amount: Money { amount_cents: MoneyCents(1960), currency: CurrencyCode("EUR".into()) },
            checkout: checkout_snapshot(order, resto, cart),
        })],
    );

    place_order(&store, &carts, &FakeGateway, place_cmd(order, resto, cart), &actor())
        .await
        .expect("replay absorbed");
    assert_eq!(store.stream(&order_stream(order)).len(), 1, "no duplicate fact");
}

// ------------------------------------------------------------------------------------------------
// Rejections (rules.yaml#/CheckoutPricesCartCreatesPaymentIntent, #/OrderTestModeIsolation)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestPlaceOrderIsRejected (RestaurantPaused / CartEmpty /
/// DeliveryAddressRequired / PaymentDeclined arms; OutsideDeliveryArea is TODO(invariant) — needs a
/// delivery-area policy port) — rules.yaml#/CheckoutPricesCartCreatesPaymentIntent
#[tokio::test]
async fn rejects_checkout_when_paused_empty_addressless_or_declined() {
    // Paused restaurant → RestaurantPaused.
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let carts = checkout_given(&store, resto, cart);
    store.seed(&restaurant_stream(resto), vec![registered(resto, None), activated(resto), paused(resto)]);
    let err = place_order(&store, &carts, &FakeGateway, place_cmd(order, resto, cart), &actor())
        .await
        .expect_err("paused");
    assert_eq!(rejection_code(&err), Some("RestaurantPaused"));

    // Empty cart (started, no line) → CartEmpty.
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let carts = checkout_given(&store, resto, cart);
    store.seed(
        &cart_stream(cart),
        vec![DomainEvent::CartStarted(CartStarted { cart_id: cart, restaurant_id: resto, customer_id: None })],
    );
    let err = place_order(&store, &carts, &FakeGateway, place_cmd(order, resto, cart), &actor())
        .await
        .expect_err("empty cart");
    assert_eq!(rejection_code(&err), Some("CartEmpty"));

    // DELIVERY without an address → DeliveryAddressRequired.
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let carts = checkout_given(&store, resto, cart);
    let mut cmd = place_cmd(order, resto, cart);
    cmd.delivery_address = None;
    let err = place_order(&store, &carts, &FakeGateway, cmd, &actor()).await.expect_err("no address");
    assert_eq!(rejection_code(&err), Some("DeliveryAddressRequired"));

    // Synchronous Stripe decline → PaymentDeclined; nothing is recorded.
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let carts = checkout_given(&store, resto, cart);
    let mut cmd = place_cmd(order, resto, cart);
    cmd.payment_method_id = "pm_declined".into();
    let err = place_order(&store, &carts, &FakeGateway, cmd, &actor()).await.expect_err("declined");
    assert_eq!(rejection_code(&err), Some("PaymentDeclined"));
    assert!(store.stream(&order_stream(order)).is_empty(), "no event on rejection");
}

/// Further actors.yaml throws arms: RestaurantNotFound / RestaurantNotActive / CartNotFound /
/// CartRestaurantMismatch — rules.yaml#/CheckoutPricesCartCreatesPaymentIntent
#[tokio::test]
async fn rejects_checkout_against_a_missing_or_inactive_restaurant_or_an_invalid_cart() {
    // Unknown restaurant → RestaurantNotFound.
    let store = MemStore::default();
    let err = place_order(&store, &FakeCarts::default(), &FakeGateway, place_cmd(oid(), rid(), cid()), &actor())
        .await
        .expect_err("missing restaurant");
    assert_eq!(rejection_code(&err), Some("RestaurantNotFound"));

    // Registered-but-DRAFT restaurant → RestaurantNotActive.
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    store.seed(&restaurant_stream(resto), vec![registered(resto, None)]);
    let err = place_order(&store, &FakeCarts::default(), &FakeGateway, place_cmd(order, resto, cart), &actor())
        .await
        .expect_err("not active");
    assert_eq!(rejection_code(&err), Some("RestaurantNotActive"));

    // Active restaurant but no cart stream → CartNotFound.
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    store.seed(&restaurant_stream(resto), vec![registered(resto, None), activated(resto)]);
    let err = place_order(&store, &FakeCarts::default(), &FakeGateway, place_cmd(order, resto, cart), &actor())
        .await
        .expect_err("missing cart");
    assert_eq!(rejection_code(&err), Some("CartNotFound"));

    // A cart bound to ANOTHER restaurant → CartRestaurantMismatch.
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let other = rid();
    store.seed(&restaurant_stream(resto), vec![registered(resto, None), activated(resto)]);
    store.seed(&cart_stream(cart), open_cart_with_line(cart, other));
    let err = place_order(&store, &FakeCarts::default(), &FakeGateway, place_cmd(order, resto, cart), &actor())
        .await
        .expect_err("mismatched cart");
    assert_eq!(rejection_code(&err), Some("CartRestaurantMismatch"));
}

/// tests.yaml#/cases/TestPlaceOrderRejectsTestRestaurantForLiveOrder —
/// rules.yaml#/OrderTestModeIsolation (ADR-0038)
#[tokio::test]
async fn a_live_order_against_a_test_restaurant_is_rejected() {
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let carts = checkout_given(&store, resto, cart);
    store.seed(&restaurant_stream(resto), vec![registered(resto, Some(Mode::TEST)), activated(resto)]);

    // LIVE (mode absent = LIVE) against TEST → CannotOrderTestRestaurant.
    let mut cmd = place_cmd(order, resto, cart);
    cmd.mode = Some(Mode::LIVE);
    let err = place_order(&store, &carts, &FakeGateway, cmd, &actor()).await.expect_err("live on test");
    assert_eq!(rejection_code(&err), Some("CannotOrderTestRestaurant"));

    // A TEST order MAY target the TEST restaurant (receipt-path validation).
    let mut cmd = place_cmd(order, resto, cart);
    cmd.mode = Some(Mode::TEST);
    place_order(&store, &carts, &FakeGateway, cmd, &actor()).await.expect("test on test");
    assert_eq!(store.stream(&order_stream(order)).len(), 1);
}
