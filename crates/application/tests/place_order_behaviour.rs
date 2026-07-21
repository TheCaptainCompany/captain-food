//! BEHAVIOUR tests for the PlaceOrderProcess COMMAND leg (`placeOrder` → PaymentIntentCreated) — the
//! executable form of the `specs/tests.yaml` PlaceOrderProcess cases whose `when` is the PlaceOrder
//! command (ADR-0032: each test cites the `specs/rules.yaml` rule it asserts).
//!
//! Pure and offline: an in-memory [`EventStore`] (holding the Restaurant, Cart and Payment streams the
//! saga touches), a fake `CatalogReadRepository` standing in for the LIVE catalog the handler reprices
//! every cart line from (rules.yaml#/ServerPriceAuthority — the server is the only price authority), a
//! fake `PaymentService` (declines `pm_declined`, accepts anything else as `pi_123`), and the in-memory
//! `payment_process_manager` state store (`application::pm_state::mem`) the handler opens the run on
//! (ADR-20260719-193500: PaymentIntentCreated is DELIVERED to `Payment-<intentId>` — the aggregate's
//! birth — and the run row goes AWAITING_PAYMENT_RESULT). The event-driven saga legs
//! (`PaymentCaptured` → OrderPlaced + CartCheckedOut, `PaymentFailed` → abort — tests.yaml
//! TestPlaceOrderPaymentCapturedPlacesOrder / TestPlaceOrderPaymentFailedPlacesNothing) run in the
//! process-manager runtime — NOT asserted here.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use application::commands::{place_order, rejection_code};
use application::pm_state::mem::MemPaymentProcessState;
use application::pm_state::{PaymentProcessRow, PaymentProcessStateStore};
use application::generated::services::{
    PaymentRequestInput, PaymentRequestOutput, PaymentService, ServiceCallMeta,
};
use application::ports::{version_conflict, Actor, EventStore};
use application::queries::{
    CatalogReadRepository, CatalogRow, OfferOptionListView, OfferOptionView, OfferView,
};
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

/// Fake LIVE catalog: `offer_by_id` answers from the configured offer views — the prices the handler
/// recomputes every line from (rules.yaml#/ServerPriceAuthority). An offer absent here has NO
/// resolvable price, so checkout must reject fail-closed (`PriceUnresolvable`).
#[derive(Default)]
struct FakeCatalogs {
    restaurant_id: Option<RestaurantId>,
    offers: Vec<OfferView>,
}

#[async_trait]
impl CatalogReadRepository for FakeCatalogs {
    async fn by_restaurant(
        &self,
        _restaurant_id: RestaurantId,
    ) -> Result<Option<CatalogRow>, DomainError> {
        Ok(None) // offer-level double — `offer_by_id` is overridden below
    }

    async fn offer_by_id(
        &self,
        restaurant_id: RestaurantId,
        offer_id: OfferId,
    ) -> Result<Option<OfferView>, DomainError> {
        if self.restaurant_id != Some(restaurant_id) {
            return Ok(None);
        }
        Ok(self.offers.iter().find(|o| o.offer_id == offer_id).cloned())
    }
}

/// Fake Stripe gateway: `pm_declined` is declined synchronously (the canonical
/// `errors.yaml#/PaymentDeclined` rejection per the PaymentService contract); anything else yields the
/// fixture intent `pi_123`.
struct FakeGateway;

#[async_trait]
impl PaymentService for FakeGateway {
    async fn request(
        &self,
        input: PaymentRequestInput,
        _meta: &ServiceCallMeta,
    ) -> Result<PaymentRequestOutput, DomainError> {
        if input.payment_method_id.0 == "pm_declined" {
            return Err(DomainError::Invariant("PaymentDeclined: card_declined".into()));
        }
        Ok(PaymentRequestOutput {
            payment_intent_id: PaymentIntentId("pi_123".into()),
            client_secret: "pi_123_secret".into(),
        })
    }

    async fn refund(
        &self,
        _input: application::generated::services::PaymentRefundInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        unreachable!("the checkout leg never refunds")
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
/// The Payment aggregate stream the fixture gateway's `pi_123` intent is born on.
fn payment_stream() -> String {
    domain::payment::stream(&PaymentIntentId("pi_123".into()))
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

/// Fixtures `cartStarted` + `cartLineAdded` — an OPEN cart holding one line of `offer` × 2 (with
/// optional selected options).
fn open_cart_with_line(
    cart: CartId,
    resto: RestaurantId,
    offer: OfferId,
    selected_option_ids: Vec<OptionId>,
) -> Vec<DomainEvent> {
    vec![
        DomainEvent::CartStarted(CartStarted { cart_id: cart, restaurant_id: resto, session_id: SessionId(uuid::Uuid::new_v4()), customer_id: None }),
        DomainEvent::CartLineAdded(CartLineAdded {
            cart_id: cart,
            line: CartLineItem {
                cart_line_id: CartLineId(uuid::Uuid::new_v4()),
                offer_id: offer,
                quantity: 2,
                selected_option_ids,
            },
        }),
    ]
}

fn eur(cents: i64) -> Money {
    Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
}

/// The live-catalog offer view checkout reprices the fixture line from: 'Margherita' at 9.80 EUR
/// (tests.yaml `productAdded` fixture), with the given option lists.
fn offer_view(offer: OfferId, option_lists: Vec<OfferOptionListView>) -> OfferView {
    OfferView {
        offer_id: offer,
        product_id: ProductId(uuid::Uuid::new_v4()),
        product_name: ProductName("Margherita".into()),
        offer_name: OfferName("Default".into()),
        price: eur(980),
        availability: CatalogItemAvailability::AVAILABLE,
        stock_status: StockStatus::IN_STOCK,
        stock_quantity: None,
        option_lists,
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
        expected_total: None,
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

/// GIVEN an ACTIVE restaurant + an OPEN cart holding 2 × the 9.80 EUR offer, live in the catalog —
/// the server-recomputed total is 19.60 EUR.
fn checkout_given(store: &MemStore, resto: RestaurantId, cart: CartId) -> FakeCatalogs {
    let offer = OfferId(uuid::Uuid::new_v4());
    store.seed(&restaurant_stream(resto), vec![registered(resto, None), activated(resto)]);
    store.seed(&cart_stream(cart), open_cart_with_line(cart, resto, offer, vec![]));
    FakeCatalogs { restaurant_id: Some(resto), offers: vec![offer_view(offer, vec![])] }
}

// ------------------------------------------------------------------------------------------------
// Happy path (rules.yaml#/CheckoutPricesCartCreatesPaymentIntent)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestPlaceOrderCreatesPaymentIntent —
/// rules.yaml#/CheckoutPricesCartCreatesPaymentIntent
#[tokio::test]
async fn checkout_prices_the_open_cart_and_creates_a_payment_intent() {
    let store = MemStore::default();
    let pm = MemPaymentProcessState::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let catalogs = checkout_given(&store, resto, cart);

    let intent = place_order(&store, &catalogs, &FakeGateway, &pm, place_cmd(order, resto, cart), None, &actor())
        .await
        .expect("checkout");

    assert_eq!(intent.payment_intent_id, PaymentIntentId("pi_123".into()));
    assert_eq!(intent.client_secret, "pi_123_secret");
    // PaymentIntentCreated is DELIVERED to the Payment aggregate's stream — its birth
    // (ADR-20260719-193500). The Order stream stays EMPTY until the capture leg materializes it.
    let events = store.stream(&payment_stream());
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        DomainEvent::PaymentIntentCreated(e)
            if e.payment_intent_id == PaymentIntentId("pi_123".into())
                && e.restaurant_id == resto
                && e.amount.amount_cents == MoneyCents(1960)
                && e.checkout.order_id == order
    ));
    assert!(store.stream(&order_stream(order)).is_empty(), "no fact on the Order stream yet");
    // The PM run is OPEN: one payment_process_manager row keyed by cart, AWAITING_PAYMENT_RESULT.
    let run = pm.by_cart(cart).await.unwrap().expect("run row opened");
    assert_eq!(run.order_id, order);
    assert_eq!(run.payment_intent_id, PaymentIntentId("pi_123".into()));
    assert_eq!(run.process_status, PaymentProcessStatus::AWAITING_PAYMENT_RESULT);
    assert_eq!(run.payment_status, PaymentStatus::PENDING);
    assert_eq!(run.last_processed_stripe_event_id, None);
    // The cart stays OPEN: CartCheckedOut only lands on payment capture (the saga's event leg).
    assert_eq!(store.stream(&cart_stream(cart)).len(), 2);
}

// ------------------------------------------------------------------------------------------------
// Server-side pricing (rules.yaml#/ServerPriceAuthority) — the server is the only price authority.
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestPlaceOrderRecomputesPriceServerSide — rules.yaml#/ServerPriceAuthority:
/// every line (offer + selected options) is repriced from the LIVE catalog into the payment-intent
/// amount and the frozen snapshot's items/breakdown; a MATCHING client confirmation total passes.
#[tokio::test]
async fn checkout_reprices_lines_and_options_from_the_live_catalog() {
    let store = MemStore::default();
    let pm = MemPaymentProcessState::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    // GIVEN: 2 × (9.80 EUR offer + 1.00 EUR selected option) — server total 21.60 EUR.
    let (offer, option, list) =
        (OfferId(uuid::Uuid::new_v4()), OptionId(uuid::Uuid::new_v4()), OptionListId(uuid::Uuid::new_v4()));
    let option_lists = vec![OfferOptionListView {
        id: list,
        min_selections: 0,
        max_selections: Some(1),
        multiple_selection: false,
        option_ids: vec![option],
        options: vec![OfferOptionView { id: option, name: OptionName("Extra".into()), price: eur(100) }],
    }];
    store.seed(&restaurant_stream(resto), vec![registered(resto, None), activated(resto)]);
    store.seed(&cart_stream(cart), open_cart_with_line(cart, resto, offer, vec![option]));
    let catalogs = FakeCatalogs { restaurant_id: Some(resto), offers: vec![offer_view(offer, option_lists)] };

    // WHEN: the client confirms the total it displayed (21.60 EUR) — equality, so the checkout passes.
    let mut cmd = place_cmd(order, resto, cart);
    cmd.expected_total = Some(eur(2160));
    place_order(&store, &catalogs, &FakeGateway, &pm, cmd, None, &actor()).await.expect("checkout");

    // THEN: the intent amount and the frozen snapshot carry ONLY server-recomputed money.
    let events = store.stream(&payment_stream());
    let DomainEvent::PaymentIntentCreated(e) = &events[0] else { panic!("PaymentIntentCreated") };
    assert_eq!(e.amount, eur(2160));
    assert_eq!(e.checkout.total_amount, eur(2160));
    assert_eq!(e.checkout.breakdown.total, eur(2160));
    assert_eq!(e.checkout.breakdown.articles, eur(2160));
    assert_eq!(e.checkout.items.len(), 1);
    let item = &e.checkout.items[0];
    assert_eq!(item.offer_id, offer);
    assert_eq!(item.name, ProductName("Margherita".into()));
    assert_eq!(item.quantity, 2);
    assert_eq!(item.unit_price, eur(980));
    assert_eq!(item.line_total, eur(2160)); // (980 + 100) × 2
    assert_eq!(item.selected_options.len(), 1);
    assert_eq!(item.selected_options[0].option_id, option);
    assert_eq!(item.selected_options[0].price, eur(100));
}

/// tests.yaml#/cases/TestPlaceOrderRejectsPriceMismatch — rules.yaml#/ServerPriceAuthority: a client
/// confirmation total diverging from the server-recomputed total rejects the checkout — the client
/// number is NEVER charged, no intent is created, no run is opened.
#[tokio::test]
async fn a_diverging_client_total_is_rejected_with_price_mismatch() {
    let store = MemStore::default();
    let pm = MemPaymentProcessState::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let catalogs = checkout_given(&store, resto, cart); // server total: 19.60 EUR

    let mut cmd = place_cmd(order, resto, cart);
    cmd.expected_total = Some(eur(100)); // the client claims 1.00 EUR
    let err = place_order(&store, &catalogs, &FakeGateway, &pm, cmd, None, &actor())
        .await
        .expect_err("price mismatch");
    assert_eq!(rejection_code(&err), Some("PriceMismatch"));
    assert!(store.stream(&payment_stream()).is_empty(), "no intent on rejection");
    assert!(pm.by_cart(cart).await.unwrap().is_none(), "no run row on rejection");
}

/// tests.yaml#/cases/TestPlaceOrderRejectsUnresolvablePrice — rules.yaml#/ServerPriceAuthority
/// (fail-closed): a line whose offer — or selected option — has no resolvable live-catalog price
/// rejects the checkout; the server never falls back to a client amount.
#[tokio::test]
async fn an_unresolvable_line_price_rejects_the_checkout_fail_closed() {
    let pm = MemPaymentProcessState::default();

    // The cart's offer is not in the live catalog anymore → PriceUnresolvable.
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    store.seed(&restaurant_stream(resto), vec![registered(resto, None), activated(resto)]);
    store.seed(&cart_stream(cart), open_cart_with_line(cart, resto, OfferId(uuid::Uuid::new_v4()), vec![]));
    let empty = FakeCatalogs { restaurant_id: Some(resto), offers: vec![] };
    let err = place_order(&store, &empty, &FakeGateway, &pm, place_cmd(order, resto, cart), None, &actor())
        .await
        .expect_err("offer gone");
    assert_eq!(rejection_code(&err), Some("PriceUnresolvable"));
    assert!(store.stream(&payment_stream()).is_empty(), "no intent on rejection");

    // The offer resolves but a SELECTED OPTION does not → same fail-closed rejection.
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let offer = OfferId(uuid::Uuid::new_v4());
    store.seed(&restaurant_stream(resto), vec![registered(resto, None), activated(resto)]);
    store.seed(
        &cart_stream(cart),
        open_cart_with_line(cart, resto, offer, vec![OptionId(uuid::Uuid::new_v4())]),
    );
    let catalogs = FakeCatalogs { restaurant_id: Some(resto), offers: vec![offer_view(offer, vec![])] };
    let err = place_order(&store, &catalogs, &FakeGateway, &pm, place_cmd(order, resto, cart), None, &actor())
        .await
        .expect_err("option gone");
    assert_eq!(rejection_code(&err), Some("PriceUnresolvable"));
    assert!(store.stream(&payment_stream()).is_empty(), "no intent on rejection");
}

/// A re-delivered checkout whose intent already birthed the Payment stream is absorbed instead of
/// duplicating the fact (the `create` version-0 clash = re-delivered birth) —
/// rules.yaml#/CheckoutPricesCartCreatesPaymentIntent.
#[tokio::test]
async fn replaying_the_checkout_onto_an_existing_payment_stream_is_absorbed() {
    let store = MemStore::default();
    let pm = MemPaymentProcessState::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let catalogs = checkout_given(&store, resto, cart);
    store.seed(
        &payment_stream(),
        vec![DomainEvent::PaymentIntentCreated(PaymentIntentCreated {
            payment_intent_id: PaymentIntentId("pi_123".into()),
            restaurant_id: resto,
            customer_id: None,
            amount: Money { amount_cents: MoneyCents(1960), currency: CurrencyCode("EUR".into()) },
            checkout: checkout_snapshot(order, resto, cart),
        })],
    );

    place_order(&store, &catalogs, &FakeGateway, &pm, place_cmd(order, resto, cart), None, &actor())
        .await
        .expect("replay absorbed");
    assert_eq!(store.stream(&payment_stream()).len(), 1, "no duplicate fact");
    // The run row is (re)opened so the awaited Stripe outcome can resolve it.
    let run = pm.by_cart(cart).await.unwrap().expect("run row opened");
    assert_eq!(run.process_status, PaymentProcessStatus::AWAITING_PAYMENT_RESULT);
}

/// Single-flight per cart (the run row's `by`/`expect` idempotency): a SECOND checkout of a cart whose
/// run is still AWAITING_PAYMENT_RESULT rejects with the cross-cutting `Conflict` BEFORE any gateway
/// call — no second Stripe intent, no second Payment stream —
/// rules.yaml#/CheckoutPricesCartCreatesPaymentIntent.
#[tokio::test]
async fn a_second_concurrent_checkout_of_the_same_cart_is_rejected() {
    let store = MemStore::default();
    let pm = MemPaymentProcessState::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let catalogs = checkout_given(&store, resto, cart);
    pm.upsert(&PaymentProcessRow {
        cart_id: cart,
        order_id: oid(), // the FIRST checkout's order — still awaiting its Stripe outcome
        payment_intent_id: PaymentIntentId("pi_first".into()),
        process_status: PaymentProcessStatus::AWAITING_PAYMENT_RESULT,
        payment_status: PaymentStatus::PENDING,
        customer_id: None,
        session_id: None,
        client_secret: Some("pi_first_secret".into()),
        last_processed_stripe_event_id: None,
        last_update_utc: chrono::Utc::now(),
    })
    .await
    .unwrap();

    let err = place_order(&store, &catalogs, &FakeGateway, &pm, place_cmd(order, resto, cart), None, &actor())
        .await
        .expect_err("second checkout in flight");
    assert_eq!(rejection_code(&err), Some("Conflict"));
    assert!(store.stream(&payment_stream()).is_empty(), "no second intent recorded");
    // The original run is untouched.
    let run = pm.by_cart(cart).await.unwrap().unwrap();
    assert_eq!(run.payment_intent_id, PaymentIntentId("pi_first".into()));
}

// ------------------------------------------------------------------------------------------------
// Rejections (rules.yaml#/CheckoutPricesCartCreatesPaymentIntent, #/OrderTestModeIsolation)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestPlaceOrderIsRejected (RestaurantPaused / CartEmpty /
/// DeliveryAddressRequired / PaymentDeclined arms; OutsideDeliveryArea is TODO(invariant) — needs a
/// delivery-area policy port) — rules.yaml#/CheckoutPricesCartCreatesPaymentIntent
#[tokio::test]
async fn rejects_checkout_when_paused_empty_addressless_or_declined() {
    let pm = MemPaymentProcessState::default();

    // Paused restaurant → RestaurantPaused.
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let catalogs = checkout_given(&store, resto, cart);
    store.seed(&restaurant_stream(resto), vec![registered(resto, None), activated(resto), paused(resto)]);
    let err = place_order(&store, &catalogs, &FakeGateway, &pm, place_cmd(order, resto, cart), None, &actor())
        .await
        .expect_err("paused");
    assert_eq!(rejection_code(&err), Some("RestaurantPaused"));

    // Empty cart (started, no line) → CartEmpty.
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let catalogs = checkout_given(&store, resto, cart);
    store.seed(
        &cart_stream(cart),
        vec![DomainEvent::CartStarted(CartStarted { cart_id: cart, restaurant_id: resto, session_id: SessionId(uuid::Uuid::new_v4()), customer_id: None })],
    );
    let err = place_order(&store, &catalogs, &FakeGateway, &pm, place_cmd(order, resto, cart), None, &actor())
        .await
        .expect_err("empty cart");
    assert_eq!(rejection_code(&err), Some("CartEmpty"));

    // DELIVERY without an address → DeliveryAddressRequired.
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let catalogs = checkout_given(&store, resto, cart);
    let mut cmd = place_cmd(order, resto, cart);
    cmd.delivery_address = None;
    let err = place_order(&store, &catalogs, &FakeGateway, &pm, cmd, None, &actor()).await.expect_err("no address");
    assert_eq!(rejection_code(&err), Some("DeliveryAddressRequired"));

    // Synchronous Stripe decline → PaymentDeclined; nothing is recorded and no run is opened.
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let catalogs = checkout_given(&store, resto, cart);
    let mut cmd = place_cmd(order, resto, cart);
    cmd.payment_method_id = "pm_declined".into();
    let err = place_order(&store, &catalogs, &FakeGateway, &pm, cmd, None, &actor()).await.expect_err("declined");
    assert_eq!(rejection_code(&err), Some("PaymentDeclined"));
    assert!(store.stream(&payment_stream()).is_empty(), "no event on rejection");
    assert!(store.stream(&order_stream(order)).is_empty(), "no event on rejection");
    assert!(pm.by_cart(cart).await.unwrap().is_none(), "no run row on rejection");
}

/// Further actors.yaml throws arms: RestaurantNotFound / RestaurantNotActive / CartNotFound /
/// CartRestaurantMismatch — rules.yaml#/CheckoutPricesCartCreatesPaymentIntent
#[tokio::test]
async fn rejects_checkout_against_a_missing_or_inactive_restaurant_or_an_invalid_cart() {
    let pm = MemPaymentProcessState::default();

    // Unknown restaurant → RestaurantNotFound.
    let store = MemStore::default();
    let err = place_order(&store, &FakeCatalogs::default(), &FakeGateway, &pm, place_cmd(oid(), rid(), cid()), None, &actor())
        .await
        .expect_err("missing restaurant");
    assert_eq!(rejection_code(&err), Some("RestaurantNotFound"));

    // Registered-but-DRAFT restaurant → RestaurantNotActive.
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    store.seed(&restaurant_stream(resto), vec![registered(resto, None)]);
    let err = place_order(&store, &FakeCatalogs::default(), &FakeGateway, &pm, place_cmd(order, resto, cart), None, &actor())
        .await
        .expect_err("not active");
    assert_eq!(rejection_code(&err), Some("RestaurantNotActive"));

    // Active restaurant but no cart stream → CartNotFound.
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    store.seed(&restaurant_stream(resto), vec![registered(resto, None), activated(resto)]);
    let err = place_order(&store, &FakeCatalogs::default(), &FakeGateway, &pm, place_cmd(order, resto, cart), None, &actor())
        .await
        .expect_err("missing cart");
    assert_eq!(rejection_code(&err), Some("CartNotFound"));

    // A cart bound to ANOTHER restaurant → CartRestaurantMismatch.
    let store = MemStore::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let other = rid();
    store.seed(&restaurant_stream(resto), vec![registered(resto, None), activated(resto)]);
    store.seed(&cart_stream(cart), open_cart_with_line(cart, other, OfferId(uuid::Uuid::new_v4()), vec![]));
    let err = place_order(&store, &FakeCatalogs::default(), &FakeGateway, &pm, place_cmd(order, resto, cart), None, &actor())
        .await
        .expect_err("mismatched cart");
    assert_eq!(rejection_code(&err), Some("CartRestaurantMismatch"));
}

/// tests.yaml#/cases/TestPlaceOrderRejectsTestRestaurantForLiveOrder —
/// rules.yaml#/OrderTestModeIsolation (ADR-0038)
#[tokio::test]
async fn a_live_order_against_a_test_restaurant_is_rejected() {
    let store = MemStore::default();
    let pm = MemPaymentProcessState::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let catalogs = checkout_given(&store, resto, cart);
    store.seed(&restaurant_stream(resto), vec![registered(resto, Some(Mode::TEST)), activated(resto)]);

    // LIVE (mode absent = LIVE) against TEST → CannotOrderTestRestaurant.
    let mut cmd = place_cmd(order, resto, cart);
    cmd.mode = Some(Mode::LIVE);
    let err = place_order(&store, &catalogs, &FakeGateway, &pm, cmd, None, &actor()).await.expect_err("live on test");
    assert_eq!(rejection_code(&err), Some("CannotOrderTestRestaurant"));

    // A TEST order MAY target the TEST restaurant (receipt-path validation).
    let mut cmd = place_cmd(order, resto, cart);
    cmd.mode = Some(Mode::TEST);
    place_order(&store, &catalogs, &FakeGateway, &pm, cmd, None, &actor()).await.expect("test on test");
    assert_eq!(store.stream(&payment_stream()).len(), 1);
}

// ------------------------------------------------------------------------------------------------
// Anonymous session scope (#12, ADR-20260720-213000) — the dispatch-layer X-SESSION-ID is stamped
// onto the PM run row so a guest checkout survives an app restart (paymentStatus session ownership).
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestPlaceOrderCreatesPaymentIntent (session-scope facet) —
/// rules.yaml#/CheckoutPricesCartCreatesPaymentIntent
#[tokio::test]
async fn checkout_stamps_the_anonymous_session_onto_the_run_row() {
    let store = MemStore::default();
    let pm = MemPaymentProcessState::default();
    let (order, resto, cart) = (oid(), rid(), cid());
    let catalogs = checkout_given(&store, resto, cart);
    let session = domain::generated::scalars::SessionId(uuid::Uuid::new_v4());

    place_order(&store, &catalogs, &FakeGateway, &pm, place_cmd(order, resto, cart), Some(session), &actor())
        .await
        .expect("checkout");

    // The run row carries the envelope session — the guest's only durable identity: after an app
    // restart, the SAME persisted session id re-owns paymentStatus(orderId).
    let run = pm.by_cart(cart).await.unwrap().expect("run row opened");
    assert_eq!(run.session_id, Some(session), "run row must carry the dispatch session id");

    // And a sessionless (identified-customer) checkout stays None — no phantom scope.
    let (order2, resto2, cart2) = (oid(), rid(), cid());
    let catalogs2 = checkout_given(&store, resto2, cart2);
    place_order(&store, &catalogs2, &FakeGateway, &pm, place_cmd(order2, resto2, cart2), None, &actor())
        .await
        .expect("checkout without session");
    let run2 = pm.by_cart(cart2).await.unwrap().expect("second run row");
    assert_eq!(run2.session_id, None);
}
