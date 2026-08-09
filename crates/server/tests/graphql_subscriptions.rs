//! GraphQL SUBSCRIPTIONS over the in-process buses (no external deps — no DB, no WebSocket): the
//! generated `SubscriptionRoot` executed directly via `schema.execute_stream` with a `RequestRole` in
//! the request context (what the `/{role}/graphql` WS handshake injects at `connection_init`).
//!
//! - `orderStatusChanged(correlationId)`: a published EventBus envelope whose correlation matches
//!   re-resolves the CURRENT Order from the read model and pushes it; identical consecutive states
//!   are deduped; a terminal status completes the stream. A non-matching correlation yields nothing.
//! - `operationStatusChanged(messageId)` (ADR-20260720-015500): snapshot-first from the
//!   command_journal, then every OperationStatusBus transition; ownership-scoped (session/actor/
//!   ADMIN) — a non-owned messageId yields an EMPTY stream; a terminal status completes it.
//! - `paymentStatusChanged(orderId)`: re-resolves the PlaceOrderProcess run row on Payment-stream
//!   envelopes; initiator-scoped; completes when the run resolves.
//! - ACL: the per-field guard rejects roles outside api.yaml `roles` (FORBIDDEN) before any streaming.
//!
//! Free-tier caveat (documented contract): the buses and the WebSocket live only while the app
//! instance is warm — a restart drops connections and clients resubscribe + re-sync via the pull
//! queries (`order` / `operationStatus` / `paymentStatus`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_graphql::futures_util::StreamExt;
use async_graphql::Request;
use async_trait::async_trait;
use domain::generated::scalars as ds;
use domain::shared::errors::DomainError;

use application::queries::{
    CartReadRepository, CartRow, CatalogReadRepository, CatalogRow, CustomerReadRepository,
    CustomerRow, OrderFilter, OrderReadRepository, OrderTrackingRow, PricingPolicyReadRepository,
    PricingPolicyRow, ProspectFilter, ProspectionPipelineRow, ProspectionReadRepository,
    RestaurantFilter, RestaurantReadRepository, RestaurantRow, UberEstimationPolicyReadRepository,
    UberEstimationPolicyRow, UberSplitPolicyReadRepository, UberSplitPolicyRow,
};
use infrastructure::{AppendedEvent, EventBus};
use server::graphql_acl::RequestRole;
use server::graphql_schema::{build_schema, CaptainSchema, ReadDeps};

// ---------------------------------------------------------------------------------------------
// In-memory read-model stand-ins (only orders + restaurants matter to the wired subscriptions).
// ---------------------------------------------------------------------------------------------

#[derive(Clone)]
struct InMemoryOrders(Arc<Mutex<HashMap<uuid::Uuid, OrderTrackingRow>>>);

/// Emulates the REAL adapter's #144 scoping direction so the ownership tests stay honest:
/// Customer sees only its own rows, Public sees nothing, Admin/System/other tenant scopes pass
/// through (membership emulation is the DB suite's job, not this stand-in's).
fn scoped(row: Option<OrderTrackingRow>, scope: &application::queries::ReadScope) -> Option<OrderTrackingRow> {
    use application::queries::ReadScope;
    row.filter(|r| match scope {
        ReadScope::Customer(c) => r.customer_id.as_ref() == Some(c),
        ReadScope::Public => false,
        _ => true,
    })
}

#[async_trait]
impl OrderReadRepository for InMemoryOrders {
    async fn list(
        &self,
        _filter: OrderFilter,
        scope: &application::queries::ReadScope,
    ) -> Result<Vec<OrderTrackingRow>, DomainError> {
        let rows = self.0.lock().unwrap().values().cloned().collect::<Vec<_>>();
        Ok(rows.into_iter().filter_map(|r| scoped(Some(r), scope)).collect())
    }
    async fn by_id(
        &self,
        id: ds::OrderId,
        scope: &application::queries::ReadScope,
    ) -> Result<Option<OrderTrackingRow>, DomainError> {
        Ok(scoped(self.0.lock().unwrap().get(&id.0).cloned(), scope))
    }
}

#[derive(Clone)]
struct InMemoryRestaurants(RestaurantRow);

#[async_trait]
impl RestaurantReadRepository for InMemoryRestaurants {
    async fn list(&self, _filter: RestaurantFilter) -> Result<Vec<RestaurantRow>, DomainError> {
        Ok(vec![self.0.clone()])
    }
    async fn by_slug(&self, _slug: ds::Slug) -> Result<Option<RestaurantRow>, DomainError> {
        Ok(Some(self.0.clone()))
    }
    async fn by_id(&self, _id: ds::RestaurantId) -> Result<Option<RestaurantRow>, DomainError> {
        Ok(Some(self.0.clone()))
    }
}

/// Empty stand-ins for the read models the subscription resolvers never touch.
struct Empty;

#[async_trait]
impl ProspectionReadRepository for Empty {
    async fn list(&self, _f: ProspectFilter) -> Result<Vec<ProspectionPipelineRow>, DomainError> {
        Ok(Vec::new())
    }
}
#[async_trait]
impl PricingPolicyReadRepository for Empty {
    async fn list(&self) -> Result<Vec<PricingPolicyRow>, DomainError> {
        Ok(Vec::new())
    }
}
#[async_trait]
impl UberEstimationPolicyReadRepository for Empty {
    async fn list(&self) -> Result<Vec<UberEstimationPolicyRow>, DomainError> {
        Ok(Vec::new())
    }
}
#[async_trait]
impl UberSplitPolicyReadRepository for Empty {
    async fn list(&self) -> Result<Vec<UberSplitPolicyRow>, DomainError> {
        Ok(Vec::new())
    }
}
#[async_trait]
impl CatalogReadRepository for Empty {
    async fn by_restaurant(&self, _id: ds::RestaurantId) -> Result<Option<CatalogRow>, DomainError> {
        Ok(None)
    }
}
#[async_trait]
impl CartReadRepository for Empty {
    async fn by_customer(&self, _id: ds::CustomerId) -> Result<Vec<CartRow>, DomainError> {
        Ok(Vec::new())
    }
    async fn by_id(&self, _id: ds::CartId) -> Result<Option<CartRow>, DomainError> {
        Ok(None)
    }
}
#[async_trait]
impl CustomerReadRepository for Empty {
    async fn by_phone(&self, _p: ds::PhoneNumber) -> Result<Option<CustomerRow>, DomainError> {
        Ok(None)
    }
    async fn by_email(&self, _e: ds::EmailAddress) -> Result<Option<CustomerRow>, DomainError> {
        Ok(None)
    }
    async fn by_id(&self, _id: ds::CustomerId) -> Result<Option<CustomerRow>, DomainError> {
        Ok(None)
    }
    async fn by_auth_ref(&self, _r: ds::ExternalReference) -> Result<Option<CustomerRow>, DomainError> {
        Ok(None)
    }
}
#[async_trait]
impl application::queries::DeliveryReadRepository for Empty {
    async fn by_order(
        &self,
        _o: ds::OrderId,
    ) -> Result<Option<application::queries::DeliveryJobRow>, DomainError> {
        Ok(None)
    }
    async fn for_rider(
        &self,
        _r: ds::RiderId,
        _s: Option<ds::DeliveryStatus>,
    ) -> Result<Vec<application::queries::DeliveryJobRow>, DomainError> {
        Ok(vec![])
    }
    async fn by_restaurant(
        &self,
        _r: ds::RestaurantId,
        _s: Option<ds::DeliveryStatus>,
    ) -> Result<Vec<application::queries::DeliveryJobRow>, DomainError> {
        Ok(vec![])
    }
}

#[async_trait]
impl application::queries::OrderConversationReadRepository for Empty {
    async fn by_order(
        &self,
        _o: ds::OrderId,
    ) -> Result<Option<application::queries::OrderConversationRow>, DomainError> {
        Ok(None)
    }
}
#[async_trait]
impl application::queries::CustomerCreditReadRepository for Empty {
    async fn by_customer(
        &self,
        _c: ds::CustomerId,
    ) -> Result<Option<application::queries::CustomerCreditBalanceRow>, DomainError> {
        Ok(None)
    }
}
#[async_trait]
impl application::queries::RefundReadRepository for Empty {
    async fn list(
        &self,
        _f: application::queries::RefundFilter,
    ) -> Result<Vec<application::queries::RefundRow>, DomainError> {
        Ok(vec![])
    }
}
#[async_trait]
impl application::queries::DeliveryPartnerAvailabilityReadRepository for Empty {
    async fn list(
        &self,
        _f: application::queries::DeliveryPartnerAvailabilityFilter,
    ) -> Result<Vec<application::queries::DeliveryPartnerAvailabilityRow>, DomainError> {
        Ok(vec![])
    }
}

#[async_trait]
impl application::queries::ReclamationReadRepository for Empty {
    async fn by_customer(
        &self,
        _c: ds::CustomerId,
    ) -> Result<Vec<application::queries::ReclamationRow>, DomainError> {
        Ok(vec![])
    }
    async fn list(
        &self,
        _f: application::queries::ReclamationFilter,
    ) -> Result<Vec<application::queries::ReclamationRow>, DomainError> {
        Ok(vec![])
    }
    async fn by_id(
        &self,
        _id: ds::ReclamationId,
    ) -> Result<Option<application::queries::ReclamationRow>, DomainError> {
        Ok(None)
    }
}

#[async_trait]
impl application::queries::DeliverySatisfactionReadRepository for Empty {
    async fn by_restaurant(
        &self,
        _r: ds::RestaurantId,
        _t: Option<ds::DeliveryTimeliness>,
    ) -> Result<Vec<application::queries::DeliverySatisfactionRow>, DomainError> {
        Ok(vec![])
    }
}

#[async_trait]
impl application::queries::MailboxLaneRepository for Empty {
    async fn list(&self) -> Result<Vec<application::queries::MailboxLaneRow>, DomainError> {
        Ok(vec![])
    }
    async fn poisoned(
        &self,
        _actor_type: Option<String>,
        _limit: i64,
    ) -> Result<Vec<application::queries::PoisonedMessageRow>, DomainError> {
        Ok(vec![])
    }
}

// ---------------------------------------------------------------------------------------------
// Fixtures.
// ---------------------------------------------------------------------------------------------

fn restaurant_row(restaurant_id: uuid::Uuid) -> RestaurantRow {
    let now = chrono::Utc::now();
    RestaurantRow {
        restaurant_id: ds::RestaurantId(restaurant_id),
        restaurant_account_id: None,
        listing_status: ds::RestaurantListingStatus::ACTIVE_PARTNER,
        external_identifiers: None,
        google_place_id: None,
        slug: Some(ds::Slug("chez-marco".into())),
        display_name: ds::RestaurantDisplayName("Chez Marco".into()),
        description: None,
        tags: None,
        margin_rate: None,
        cuisine_category: None,
        uber_prices_opt_in: None,
        website: None,
        rating: None,
        reviews_count: None,
        gbp_order_url: None,
        gbp_link_status: None,
        address: serde_json::json!({ "line1": "1 Rue Nationale", "postalCode": "37000", "city": "Tours", "country": "FR" }),
        location: None,
        opening_hours: serde_json::json!([]),
        status: ds::RestaurantStatus::ACTIVE,
        order_acceptance: ds::OrderAcceptanceMode::NORMAL,
        default_currency: ds::CurrencyCode("EUR".into()),
        timezone: None,
        preparation_time_minutes: None,
        created_at: now,
        updated_at: now,
    }
}

fn order_row(order_id: uuid::Uuid, restaurant_id: uuid::Uuid, status: ds::OrderStatus) -> OrderTrackingRow {
    let now = chrono::Utc::now();
    let cents = |v: i64| ds::MoneyCents(v);
    OrderTrackingRow {
        order_id: ds::OrderId(order_id),
        r#ref: ds::ExternalReference("ORD-1".into()),
        restaurant_id: ds::RestaurantId(restaurant_id),
        customer_id: None,
        status,
        service_type: ds::ServiceType::DELIVERY,
        items: serde_json::json!([]),
        total_amount_cents: cents(2000),
        currency: ds::CurrencyCode("EUR".into()),
        articles_cents: cents(1500),
        delivery_cents: cents(400),
        service_fee_cents: cents(100),
        restaurant_payout_cents: cents(1400),
        rider_payout_cents: cents(400),
        captain_net_cents: cents(200),
        uber_total_cents: None,
        uber_restaurant_cents: None,
        uber_rider_cents: None,
        uber_platform_cents: None,
        uber_basis: None,
        delivery_address: None,
        estimated_ready_at: None,
        placed_at: now,
        status_changed_at: now,
        payment_intent_id: None,
        payment_status: "PENDING".into(),
        restaurant_stars: None,
        rating_comment: None,
        rider_thumb: None,
        delivery_timeliness: None,
        rider_tip_cents: None,
        restaurant_tip_cents: None,
        captain_tip_cents: None,
        rated_at: None,
        delivery_status: None,
        courier: None,
        estimated_dropoff_at: None,
        created_at: now,
        updated_at: now,
    }
}

/// The order↔delivery-job binding the `orderStatusChanged` subscription learns lazily (#420), as a
/// stand-in: `None` = this order has no delivery job (pre-dispatch, or COLLECTION).
#[derive(Clone)]
struct InMemoryDeliveries(Arc<Mutex<Option<application::queries::DeliveryJobRow>>>);

#[async_trait]
impl application::queries::DeliveryReadRepository for InMemoryDeliveries {
    async fn by_order(
        &self,
        order_id: ds::OrderId,
    ) -> Result<Option<application::queries::DeliveryJobRow>, DomainError> {
        Ok(self.0.lock().unwrap().clone().filter(|job| job.order_id == order_id))
    }
    async fn for_rider(
        &self,
        _r: ds::RiderId,
        _s: Option<ds::DeliveryStatus>,
    ) -> Result<Vec<application::queries::DeliveryJobRow>, DomainError> {
        Ok(vec![])
    }
    async fn by_restaurant(
        &self,
        _r: ds::RestaurantId,
        _s: Option<ds::DeliveryStatus>,
    ) -> Result<Vec<application::queries::DeliveryJobRow>, DomainError> {
        Ok(vec![])
    }
}

fn delivery_job_row(
    job_id: uuid::Uuid,
    order_id: uuid::Uuid,
    restaurant_id: uuid::Uuid,
) -> application::queries::DeliveryJobRow {
    application::queries::DeliveryJobRow {
        delivery_job_id: ds::DeliveryJobId(job_id),
        order_id: ds::OrderId(order_id),
        restaurant_id: ds::RestaurantId(restaurant_id),
        status: ds::DeliveryStatus::ASSIGNED,
        provider: None,
        rider_id: None,
        courier: None,
        partner_ref: None,
        pickup_address: serde_json::json!({}),
        dropoff_address: serde_json::json!({}),
        estimated_pickup_at: None,
        estimated_dropoff_at: None,
        requested_at: chrono::Utc::now(),
        picked_up_at: None,
        delivered_at: None,
    }
}

fn schema_over(orders: InMemoryOrders, restaurants: InMemoryRestaurants, bus: EventBus) -> CaptainSchema {
    schema_over_with_deliveries(
        orders,
        restaurants,
        Arc::new(InMemoryDeliveries(Arc::new(Mutex::new(None)))),
        bus,
    )
}

fn schema_over_with_deliveries(
    orders: InMemoryOrders,
    restaurants: InMemoryRestaurants,
    deliveries: Arc<dyn application::queries::DeliveryReadRepository>,
    bus: EventBus,
) -> CaptainSchema {
    build_schema(
        Some(ReadDeps {
            restaurants: Arc::new(restaurants),
            prospection: Arc::new(Empty),
            pricing_policy: Arc::new(Empty),
            uber_estimation_policy: Arc::new(Empty),
            uber_split_policy: Arc::new(Empty),
            catalogs: Arc::new(Empty),
            carts: Arc::new(Empty),
            orders: Arc::new(orders),
            order_conversations: Arc::new(Empty),
            customers: Arc::new(Empty),
            deliveries,
            refunds: Arc::new(Empty),
            delivery_satisfaction: Arc::new(Empty),
            delivery_partner_availabilities: Arc::new(Empty),
            reclamations: Arc::new(Empty),
            customer_credit: Arc::new(Empty),
            mailbox_lanes: Arc::new(Empty),
        }),
        None,
        Some(bus),
    )
}

fn order_envelope(order_id: uuid::Uuid, correlation_id: uuid::Uuid, event_type: &str, position: i64) -> AppendedEvent {
    AppendedEvent {
        stream_name: format!("Order-{order_id}"),
        event_type: event_type.into(),
        correlation_id,
        position,
    }
}

/// Publish `envelope` every 20ms for ~1s: the subscription's bus receiver only exists once the
/// response stream is first polled, so a single early publish could be missed — repetition absorbs
/// that race (the resolver dedupes identical states, so at most ONE item results).
fn spawn_publisher(bus: EventBus, envelope: AppendedEvent) {
    tokio::spawn(async move {
        for _ in 0..50 {
            bus.publish(envelope.clone());
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
}

fn is_forbidden(err: &async_graphql::ServerError) -> bool {
    serde_json::to_value(err)
        .ok()
        .and_then(|v| v.get("extensions").and_then(|e| e.get("code")).cloned())
        == Some(serde_json::json!("FORBIDDEN"))
}

// ---------------------------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------------------------

/// A matching envelope pushes the CURRENT Order; duplicates are deduped; a terminal status pushes one
/// final Order and then COMPLETES the stream.
#[tokio::test(flavor = "multi_thread")]
async fn order_status_changed_streams_updates_dedupes_and_completes() {
    let restaurant_id = uuid::Uuid::new_v4();
    let order_id = uuid::Uuid::new_v4();
    let correlation = uuid::Uuid::new_v4();
    let store = Arc::new(Mutex::new(HashMap::from([(
        order_id,
        order_row(order_id, restaurant_id, ds::OrderStatus::PLACED),
    )])));
    let orders = InMemoryOrders(store.clone());
    let bus = EventBus::default();
    let schema = schema_over(orders, InMemoryRestaurants(restaurant_row(restaurant_id)), bus.clone());

    // Tracked by orderId (#14); ownership rides the context ReadScope for every role (#144).
    let query = format!(
        r#"subscription {{ orderStatusChanged(input: {{ orderId: "{order_id}" }}) {{ id status }} }}"#
    );
    // ReadScope is transport-injected in production (#144); System here because these tests
    // exercise the stream machinery, not the guard (the guard has its own ownership test below).
    let mut stream = schema.execute_stream(
        Request::new(query).data(RequestRole::Restaurant).data(application::queries::ReadScope::System),
    );

    // Many identical OrderPlaced envelopes → exactly one PLACED push (dedupe).
    spawn_publisher(bus.clone(), order_envelope(order_id, correlation, "OrderPlaced", 1));
    let first = tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .expect("first push in time")
        .expect("stream item");
    assert!(first.errors.is_empty(), "first push errored: {:?}", first.errors);
    let data = first.data.into_json().expect("json");
    assert_eq!(data["orderStatusChanged"]["status"], serde_json::json!("PLACED"));
    assert_eq!(data["orderStatusChanged"]["id"], serde_json::json!(order_id.to_string()));

    // The order reaches a TERMINAL status in the read model; the next matching envelope pushes it...
    {
        let mut rows = store.lock().unwrap();
        let row = rows.get_mut(&order_id).expect("row");
        row.status = ds::OrderStatus::DELIVERED;
        row.updated_at = chrono::Utc::now();
    }
    spawn_publisher(bus.clone(), order_envelope(order_id, correlation, "OrderDelivered", 2));
    let second = tokio::time::timeout(Duration::from_secs(15), stream.next())
        .await
        .expect("terminal push in time")
        .expect("stream item");
    assert!(second.errors.is_empty(), "terminal push errored: {:?}", second.errors);
    let data = second.data.into_json().expect("json");
    assert_eq!(data["orderStatusChanged"]["status"], serde_json::json!("DELIVERED"));

    // ...and completes the subscription (terminal status → stream end).
    let end = tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .expect("completion in time");
    assert!(end.is_none(), "stream must complete after a terminal status");
}

/// #420 / PROP-20260809-021351 §2 (G7): the rider hops are appended to `DeliveryJob-<id>`, never to
/// the Order stream, and they fold `delivery_status` / `courier` / `estimated_dropoff_at` onto the
/// SAME OrderTracking row while leaving `status` alone (#424). The subscription used to fail this
/// TWICE over — the filter matched only `Order-<id>`, and the dedupe compared `status` — so the
/// confirmation page went silent at the exact moment the customer is watching hardest. Both gates
/// are exercised here: the envelope arrives on the delivery stream AND the order status is
/// unchanged across the push.
#[tokio::test(flavor = "multi_thread")]
async fn a_status_unchanged_fold_still_reaches_the_confirmation_page() {
    let restaurant_id = uuid::Uuid::new_v4();
    let order_id = uuid::Uuid::new_v4();
    let job_id = uuid::Uuid::new_v4();
    let correlation = uuid::Uuid::new_v4();
    let store = Arc::new(Mutex::new(HashMap::from([(
        order_id,
        order_row(order_id, restaurant_id, ds::OrderStatus::ACCEPTED),
    )])));
    let deliveries =
        InMemoryDeliveries(Arc::new(Mutex::new(Some(delivery_job_row(job_id, order_id, restaurant_id)))));
    let bus = EventBus::default();
    let schema = schema_over_with_deliveries(
        InMemoryOrders(store.clone()),
        InMemoryRestaurants(restaurant_row(restaurant_id)),
        Arc::new(deliveries),
        bus.clone(),
    );

    let query = format!(
        r#"subscription {{ orderStatusChanged(input: {{ orderId: "{order_id}" }}) {{ id status deliveryStatus }} }}"#
    );
    // ReadScope is transport-injected in production (#144); System here because these tests
    // exercise the stream machinery, not the guard (the guard has its own ownership test below).
    let mut stream = schema.execute_stream(
        Request::new(query).data(RequestRole::Restaurant).data(application::queries::ReadScope::System),
    );

    // Establish the subscriber's baseline on the order's own stream: ACCEPTED, no delivery yet.
    spawn_publisher(bus.clone(), order_envelope(order_id, correlation, "OrderAccepted", 1));
    let first = tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .expect("baseline push in time")
        .expect("stream item");
    assert!(first.errors.is_empty(), "{:?}", first.errors);
    let data = first.data.into_json().expect("json");
    assert_eq!(data["orderStatusChanged"]["status"], serde_json::json!("ACCEPTED"));
    assert_eq!(data["orderStatusChanged"]["deliveryStatus"], serde_json::Value::Null);

    // The rider picks the order up. ONLY the delivery fields and the row's fold clock move — the
    // OrderStatus stays ACCEPTED, which is precisely what the old dedupe keyed on.
    {
        let mut rows = store.lock().unwrap();
        let row = rows.get_mut(&order_id).expect("row");
        row.delivery_status = Some(ds::DeliveryStatus::PICKED_UP);
        row.courier = Some(serde_json::json!({ "displayName": "Camille" }));
        row.updated_at = chrono::Utc::now();
    }
    let delivery_envelope = AppendedEvent {
        stream_name: format!("DeliveryJob-{job_id}"),
        event_type: "DeliveryPickedUp".into(),
        correlation_id: uuid::Uuid::new_v4(),
        position: 2,
    };
    spawn_publisher(bus.clone(), delivery_envelope);

    let second = tokio::time::timeout(Duration::from_secs(15), stream.next())
        .await
        .expect("the delivery movement must reach the screen")
        .expect("stream item");
    assert!(second.errors.is_empty(), "{:?}", second.errors);
    let data = second.data.into_json().expect("json");
    assert_eq!(data["orderStatusChanged"]["status"], serde_json::json!("ACCEPTED"), "status unchanged");
    assert_eq!(
        data["orderStatusChanged"]["deliveryStatus"],
        serde_json::json!("PICKED_UP"),
        "the hop the customer is waiting for"
    );
}

/// The `DeliveryJob-` half of the filter widening, ISOLATED — the one thing its sibling above
/// cannot prove.
///
/// The mob review of #427 established this by mutation: revert the filter to `Order-<id>` only, or
/// make the `by_order` binding structurally impossible, and the sibling stays GREEN. The reason is
/// `spawn_publisher`, which pumps 50 copies of an envelope over ~1s while each delivered envelope
/// opens a ~3s re-poll window on the row — so a lingering `Order-` envelope re-reads the mutated
/// row and delivers the second frame, and the delivery branch is never entered. A test whose name
/// claims a behaviour it does not exercise is worse than no test: the next reader counts it as
/// coverage and stops looking.
///
/// Here the ONLY envelope ever published names the order's delivery job. Nothing else can wake the
/// subscriber, so the first frame is proof the widened filter and the lazy `by_order` binding both
/// work. Red on `main` (filter matches `Order-<id>` only -> no frame -> timeout).
#[tokio::test(flavor = "multi_thread")]
async fn a_delivery_job_envelope_alone_reaches_the_confirmation_page() {
    let restaurant_id = uuid::Uuid::new_v4();
    let order_id = uuid::Uuid::new_v4();
    let job_id = uuid::Uuid::new_v4();
    // The fold has ALREADY landed: the rider picked the order up, the row carries it, and the
    // OrderStatus never moved. Only the delivery envelope remains to wake anyone.
    let mut row = order_row(order_id, restaurant_id, ds::OrderStatus::ACCEPTED);
    row.delivery_status = Some(ds::DeliveryStatus::PICKED_UP);
    row.courier = Some(serde_json::json!({ "displayName": "Camille" }));
    let store = Arc::new(Mutex::new(HashMap::from([(order_id, row)])));
    let deliveries =
        InMemoryDeliveries(Arc::new(Mutex::new(Some(delivery_job_row(job_id, order_id, restaurant_id)))));
    let bus = EventBus::default();
    let schema = schema_over_with_deliveries(
        InMemoryOrders(store.clone()),
        InMemoryRestaurants(restaurant_row(restaurant_id)),
        Arc::new(deliveries),
        bus.clone(),
    );

    let query = format!(
        r#"subscription {{ orderStatusChanged(input: {{ orderId: "{order_id}" }}) {{ id status deliveryStatus }} }}"#
    );
    // ReadScope is transport-injected in production (#144); System here because these tests
    // exercise the stream machinery, not the guard (the guard has its own ownership test below).
    let mut stream = schema.execute_stream(
        Request::new(query).data(RequestRole::Restaurant).data(application::queries::ReadScope::System),
    );

    // The ONLY publish in this test. `spawn_publisher` (not a one-shot) because the bus receiver
    // exists only once the response stream is first polled.
    spawn_publisher(
        bus.clone(),
        AppendedEvent {
            stream_name: format!("DeliveryJob-{job_id}"),
            event_type: "DeliveryPickedUp".into(),
            correlation_id: uuid::Uuid::new_v4(),
            position: 1,
        },
    );

    let first = tokio::time::timeout(Duration::from_secs(15), stream.next())
        .await
        .expect("a delivery-job envelope alone must reach the screen")
        .expect("stream item");
    assert!(first.errors.is_empty(), "{:?}", first.errors);
    let data = first.data.into_json().expect("json");
    assert_eq!(data["orderStatusChanged"]["status"], serde_json::json!("ACCEPTED"), "status never moved");
    assert_eq!(
        data["orderStatusChanged"]["deliveryStatus"],
        serde_json::json!("PICKED_UP"),
        "the delivery fold reached the customer, woken by the DeliveryJob- envelope alone"
    );
}

/// The widened filter must NOT become "every delivery on the platform wakes every tracking page":
/// an envelope on ANOTHER order's delivery job is ignored, and the binding is resolved through the
/// order's own delivery job rather than by prefix alone.
#[tokio::test(flavor = "multi_thread")]
async fn another_orders_delivery_job_never_reaches_this_subscriber() {
    let restaurant_id = uuid::Uuid::new_v4();
    let order_id = uuid::Uuid::new_v4();
    let store = Arc::new(Mutex::new(HashMap::from([(
        order_id,
        order_row(order_id, restaurant_id, ds::OrderStatus::ACCEPTED),
    )])));
    // This order HAS a delivery job — just not the one the envelope names.
    let mine = uuid::Uuid::new_v4();
    let deliveries =
        InMemoryDeliveries(Arc::new(Mutex::new(Some(delivery_job_row(mine, order_id, restaurant_id)))));
    let bus = EventBus::default();
    let schema = schema_over_with_deliveries(
        InMemoryOrders(store.clone()),
        InMemoryRestaurants(restaurant_row(restaurant_id)),
        Arc::new(deliveries),
        bus.clone(),
    );
    let query = format!(
        r#"subscription {{ orderStatusChanged(input: {{ orderId: "{order_id}" }}) {{ id status }} }}"#
    );
    // ReadScope is transport-injected in production (#144); System here because these tests
    // exercise the stream machinery, not the guard (the guard has its own ownership test below).
    let mut stream = schema.execute_stream(
        Request::new(query).data(RequestRole::Restaurant).data(application::queries::ReadScope::System),
    );

    // Someone else's rider moves, and this order's row moves too (the worst case for a filter that
    // only looked at "did the row change?"): the subscriber must still hear nothing.
    {
        let mut rows = store.lock().unwrap();
        let row = rows.get_mut(&order_id).expect("row");
        row.updated_at = chrono::Utc::now();
    }
    spawn_publisher(
        bus.clone(),
        AppendedEvent {
            stream_name: format!("DeliveryJob-{}", uuid::Uuid::new_v4()),
            event_type: "DeliveryPickedUp".into(),
            correlation_id: uuid::Uuid::new_v4(),
            position: 1,
        },
    );
    let nothing = tokio::time::timeout(Duration::from_millis(1500), stream.next()).await;
    assert!(nothing.is_err(), "another order's delivery job must yield nothing: {nothing:?}");
}

/// An envelope with a DIFFERENT correlationId never reaches the subscriber.
#[tokio::test(flavor = "multi_thread")]
async fn order_status_changed_ignores_other_orders() {
    let restaurant_id = uuid::Uuid::new_v4();
    let order_id = uuid::Uuid::new_v4();
    let store = Arc::new(Mutex::new(HashMap::from([(
        order_id,
        order_row(order_id, restaurant_id, ds::OrderStatus::PLACED),
    )])));
    let bus = EventBus::default();
    let schema = schema_over(
        InMemoryOrders(store),
        InMemoryRestaurants(restaurant_row(restaurant_id)),
        bus.clone(),
    );

    // Subscribe on order B; publish only order A envelopes (#14: the stream key is the orderId).
    let other_order = uuid::Uuid::new_v4();
    let query = format!(
        r#"subscription {{ orderStatusChanged(input: {{ orderId: "{other_order}" }}) {{ id status }} }}"#
    );
    // ReadScope is transport-injected in production (#144); System here because these tests
    // exercise the stream machinery, not the guard (the guard has its own ownership test below).
    let mut stream = schema.execute_stream(
        Request::new(query).data(RequestRole::Restaurant).data(application::queries::ReadScope::System),
    );
    spawn_publisher(bus.clone(), order_envelope(order_id, uuid::Uuid::new_v4(), "OrderPlaced", 1));

    let nothing = tokio::time::timeout(Duration::from_millis(1500), stream.next()).await;
    assert!(nothing.is_err(), "another order's envelope must yield nothing: {nothing:?}");
}

/// The journal entry an acceptance would have written (RECEIVED, session-owned).
fn journal_entry(message_id: uuid::Uuid, session: uuid::Uuid) -> application::journal::CommandJournalEntry {
    application::journal::CommandJournalEntry {
        message_id,
        correlation_id: message_id,
        cause_id: None,
        session_id: Some(session),
        trace_id: None,
        user_id: None,
        user_type: "PUBLIC".to_string(),
        channel: ds::CommandChannel::GRAPHQL,
        command_type: "AddCartLine".into(),
        payload: serde_json::json!({}),
        payload_hash: "h".into(),
    }
}

/// Snapshot-first + status-bus transitions, completing on a terminal status (ADR-20260720-015500).
#[tokio::test(flavor = "multi_thread")]
async fn operation_status_changed_streams_the_journal_lifecycle() {
    let schema = build_schema(None, None, None);
    let journal: Arc<dyn application::journal::CommandJournal> =
        Arc::new(application::journal::mem::MemCommandJournal::default());
    // The mailbox-first snapshot read (#242 flip): empty here — these tests drive the journal
    // (the legacy PM-leg path), and the arm falls back to it when the mailbox has no row.
    let mailbox: Arc<dyn actor_client::mailbox::Mailbox> =
        Arc::new(actor_client::mailbox::mem::MemMailbox::default());
    let status_bus = actor_client::OperationStatusBus::default();

    let message_id = uuid::Uuid::new_v4();
    let session = uuid::Uuid::new_v4();
    journal.insert(&journal_entry(message_id, session)).await.unwrap();

    let query = format!(
        r#"subscription {{ operationStatusChanged(input: {{ messageId: "{message_id}" }}) {{ messageId status errorCode }} }}"#
    );
    // PUBLIC + the owning session (what connection_init injects from the X-SESSION-ID payload).
    let mut stream = schema.execute_stream(
        Request::new(query)
            .data(RequestRole::Public)
            .data(server::graphql_session::SessionHeader(Some(session)))
            .data(journal.clone())
            .data(mailbox.clone())
            .data(status_bus.clone()),
    );

    // Snapshot-first: the current (PENDING) state arrives without any bus publish.
    let first = tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .expect("snapshot in time")
        .expect("stream item");
    assert!(first.errors.is_empty(), "snapshot errored: {:?}", first.errors);
    let data = first.data.into_json().expect("json");
    assert_eq!(data["operationStatusChanged"]["status"], serde_json::json!("PENDING"));
    assert_eq!(
        data["operationStatusChanged"]["messageId"],
        serde_json::json!(message_id.to_string())
    );

    // The handler completes REJECTED → the transition is pushed and the stream completes.
    let bus = status_bus.clone();
    tokio::spawn(async move {
        for _ in 0..50 {
            bus.publish(actor_client::OperationUpdate {
                message_id,
                correlation_id: message_id,
                status: ds::InboundMessageStatus::REJECTED,
                error_code: Some("OfferNotFound".into()),
                message: Some("Offer not found.".into()),
            });
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    let second = tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .expect("transition in time")
        .expect("stream item");
    assert!(second.errors.is_empty(), "transition errored: {:?}", second.errors);
    let data = second.data.into_json().expect("json");
    assert_eq!(data["operationStatusChanged"]["status"], serde_json::json!("REJECTED"));
    assert_eq!(data["operationStatusChanged"]["errorCode"], serde_json::json!("OfferNotFound"));
    let end = tokio::time::timeout(Duration::from_secs(10), stream.next()).await.expect("ends");
    assert!(end.is_none(), "stream must complete after a terminal status");
}

/// A messageId journaled under ANOTHER session yields an empty stream (no existence oracle).
#[tokio::test(flavor = "multi_thread")]
async fn operation_status_changed_hides_non_owned_operations() {
    let schema = build_schema(None, None, None);
    let journal: Arc<dyn application::journal::CommandJournal> =
        Arc::new(application::journal::mem::MemCommandJournal::default());
    // The mailbox-first snapshot read (#242 flip): empty here — these tests drive the journal
    // (the legacy PM-leg path), and the arm falls back to it when the mailbox has no row.
    let mailbox: Arc<dyn actor_client::mailbox::Mailbox> =
        Arc::new(actor_client::mailbox::mem::MemMailbox::default());
    let status_bus = actor_client::OperationStatusBus::default();

    let message_id = uuid::Uuid::new_v4();
    journal.insert(&journal_entry(message_id, uuid::Uuid::new_v4())).await.unwrap();

    let query = format!(
        r#"subscription {{ operationStatusChanged(input: {{ messageId: "{message_id}" }}) {{ status }} }}"#
    );
    let mut stream = schema.execute_stream(
        Request::new(query)
            .data(RequestRole::Public)
            .data(server::graphql_session::SessionHeader(Some(uuid::Uuid::new_v4())))
            .data(journal.clone())
            .data(mailbox.clone())
            .data(status_bus.clone()),
    );
    // The non-owned stream completes EMPTY (Ok(None)) — no item, no error, no oracle.
    let nothing = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("empty stream completes promptly");
    assert!(nothing.is_none(), "a stranger session must receive nothing: {nothing:?}");
}

/// paymentStatusChanged re-resolves the run row on Payment-stream envelopes, pushes the
/// clientSecret while in flight, and completes (secret NULLed) when the run resolves.
#[tokio::test(flavor = "multi_thread")]
async fn payment_status_changed_streams_the_checkout_run() {
    use application::pm_state::{mem::MemPaymentProcessState, PaymentProcessRow, PaymentProcessStateStore};

    let bus = EventBus::default();
    let schema = build_schema(None, None, Some(bus.clone()));
    let pm = Arc::new(MemPaymentProcessState::default());
    let pm_port: Arc<dyn PaymentProcessStateStore> = pm.clone();

    let (cart_id, order_id, session) =
        (uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let row = PaymentProcessRow {
        cart_id: ds::CartId(cart_id),
        order_id: ds::OrderId(order_id),
        payment_intent_id: ds::PaymentIntentId("pi_1".into()),
        process_status: ds::PaymentProcessStatus::AWAITING_PAYMENT_RESULT,
        payment_status: ds::PaymentStatus::PENDING,
        customer_id: None,
        session_id: Some(ds::SessionId(session)),
        client_secret: Some("pi_1_secret".into()),
        last_processed_stripe_event_id: None,
        last_update_utc: chrono::Utc::now(),
    };
    pm.upsert(&row).await.unwrap();

    let query = format!(
        r#"subscription {{ paymentStatusChanged(input: {{ orderId: "{order_id}" }}) {{ paymentIntentId clientSecret status }} }}"#
    );
    let mut stream = schema.execute_stream(
        Request::new(query)
            .data(RequestRole::Customer)
            .data(server::graphql_session::SessionHeader(Some(session)))
            .data(pm_port.clone()),
    );

    // Initial resolve: the in-flight run with its clientSecret.
    let first = tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .expect("initial in time")
        .expect("stream item");
    assert!(first.errors.is_empty(), "initial errored: {:?}", first.errors);
    let data = first.data.into_json().expect("json");
    assert_eq!(data["paymentStatusChanged"]["clientSecret"], serde_json::json!("pi_1_secret"));
    assert_eq!(data["paymentStatusChanged"]["status"], serde_json::json!("PENDING"));

    // The run resolves (capture leg): secret NULLed, terminal → one final push, then completion.
    pm.upsert(&PaymentProcessRow {
        process_status: ds::PaymentProcessStatus::ORDER_PLACED,
        payment_status: ds::PaymentStatus::CAPTURED,
        client_secret: None,
        ..row
    })
    .await
    .unwrap();
    spawn_publisher(
        bus.clone(),
        AppendedEvent {
            stream_name: "Payment-pi_1".into(),
            event_type: "PaymentCaptured".into(),
            correlation_id: uuid::Uuid::new_v4(),
            position: 1,
        },
    );
    let second = tokio::time::timeout(Duration::from_secs(15), stream.next())
        .await
        .expect("terminal in time")
        .expect("stream item");
    assert!(second.errors.is_empty(), "terminal errored: {:?}", second.errors);
    let data = second.data.into_json().expect("json");
    assert_eq!(data["paymentStatusChanged"]["status"], serde_json::json!("CAPTURED"));
    assert!(data["paymentStatusChanged"]["clientSecret"].is_null(), "secret NULLed on resolve");
    let end = tokio::time::timeout(Duration::from_secs(10), stream.next()).await.expect("ends");
    assert!(end.is_none(), "stream must complete once the run resolves");
}

/// The generated guard rejects roles outside the subscription's api.yaml `roles`
/// ([CUSTOMER, RESTAURANT, RESTAURANT_ACCOUNT]) with FORBIDDEN — before any streaming starts.
#[tokio::test(flavor = "multi_thread")]
async fn unauthorized_role_is_forbidden() {
    let bus = EventBus::default();
    let schema = build_schema(None, None, Some(bus));
    let query = format!(
        r#"subscription {{ orderStatusChanged(input: {{ orderId: "{}" }}) {{ id }} }}"#,
        uuid::Uuid::new_v4()
    );

    for role in [RequestRole::Rider, RequestRole::Public, RequestRole::External] {
        let mut stream = schema.execute_stream(Request::new(query.clone()).data(role));
        let resp = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("guard answers immediately")
            .expect("one error response");
        assert_eq!(resp.errors.len(), 1, "expected one error for {role:?}: {:?}", resp.errors);
        assert!(is_forbidden(&resp.errors[0]), "expected FORBIDDEN for {role:?}: {:?}", resp.errors[0]);
        // The rejected stream terminates.
        let end = tokio::time::timeout(Duration::from_secs(5), stream.next()).await.expect("ends");
        assert!(end.is_none(), "rejected subscription must not keep streaming");
    }
}

/// Ownership on the CUSTOMER path (#14): the order's own customer receives pushes; a DIFFERENT
/// customer — and an anonymous CUSTOMER-path caller — receives nothing (silence, no oracle).
#[tokio::test(flavor = "multi_thread")]
async fn order_status_changed_is_owned_by_the_orders_customer() {
    let restaurant_id = uuid::Uuid::new_v4();
    let order_id = uuid::Uuid::new_v4();
    let customer_id = ds::CustomerId(uuid::Uuid::new_v4());
    let mut row = order_row(order_id, restaurant_id, ds::OrderStatus::PLACED);
    row.customer_id = Some(customer_id);
    let store = Arc::new(Mutex::new(HashMap::from([(order_id, row)])));
    let bus = EventBus::default();
    let schema = schema_over(
        InMemoryOrders(store),
        InMemoryRestaurants(restaurant_row(restaurant_id)),
        bus.clone(),
    );
    let query = format!(
        r#"subscription {{ orderStatusChanged(input: {{ orderId: "{order_id}" }}) {{ id status }} }}"#
    );

    // The owning customer receives the push. The ReadScope is what connection_init injects in
    // production (#144) — resolved ONCE at the edge; the resolver never re-derives identity.
    let mut stream = schema.execute_stream(
        Request::new(query.clone())
            .data(RequestRole::Customer)
            .data(application::queries::ReadScope::Customer(customer_id)),
    );
    spawn_publisher(bus.clone(), order_envelope(order_id, uuid::Uuid::new_v4(), "OrderPlaced", 1));
    let first = tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .expect("owner push in time")
        .expect("stream item");
    assert!(first.errors.is_empty(), "owner push errored: {:?}", first.errors);
    let data = first.data.into_json().expect("json");
    assert_eq!(data["orderStatusChanged"]["status"], serde_json::json!("PLACED"));

    // A DIFFERENT customer stays silent — their scope holds no membership on this order, so the
    // scoped read resolves None and the stream never yields (no oracle).
    let mut stream = schema.execute_stream(
        Request::new(query.clone())
            .data(RequestRole::Customer)
            .data(application::queries::ReadScope::Customer(ds::CustomerId(uuid::Uuid::new_v4()))),
    );
    spawn_publisher(bus.clone(), order_envelope(order_id, uuid::Uuid::new_v4(), "OrderAccepted", 2));
    let nothing = tokio::time::timeout(Duration::from_millis(1500), stream.next()).await;
    assert!(nothing.is_err(), "a stranger customer must receive nothing: {nothing:?}");

    // An anonymous CUSTOMER-path caller (no ReadScope in context) falls back to Public and stays
    // silent too — the fail-closed direction the transport fallback must never widen.
    let mut stream = schema.execute_stream(Request::new(query).data(RequestRole::Customer));
    spawn_publisher(bus.clone(), order_envelope(order_id, uuid::Uuid::new_v4(), "OrderReady", 3));
    let nothing = tokio::time::timeout(Duration::from_millis(1500), stream.next()).await;
    assert!(nothing.is_err(), "an anonymous customer-path caller must receive nothing: {nothing:?}");
}

// ---------------------------------------------------------------------------------------------
// #144 — the transport fallback DIRECTION, pinned. The generated resolvers fall back to
// `unwrap_or(Public)` when no ReadScope is in the context; the compiler would accept
// `unwrap_or(Admin)` just as happily, and that one-word change is the classic authorization
// inversion (the most exposed caller becomes the most privileged). A spy repository records the
// scope the port actually received.
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Default)]
struct SpyOrders(Arc<Mutex<Vec<application::queries::ReadScope>>>);

#[async_trait]
impl OrderReadRepository for SpyOrders {
    async fn list(
        &self,
        _filter: OrderFilter,
        scope: &application::queries::ReadScope,
    ) -> Result<Vec<OrderTrackingRow>, DomainError> {
        self.0.lock().unwrap().push(scope.clone());
        Ok(Vec::new())
    }
    async fn by_id(
        &self,
        _id: ds::OrderId,
        scope: &application::queries::ReadScope,
    ) -> Result<Option<OrderTrackingRow>, DomainError> {
        self.0.lock().unwrap().push(scope.clone());
        Ok(None)
    }
}

fn schema_over_spy(spy: SpyOrders) -> CaptainSchema {
    build_schema(
        Some(ReadDeps {
            restaurants: Arc::new(InMemoryRestaurants(restaurant_row(uuid::Uuid::new_v4()))),
            prospection: Arc::new(Empty),
            pricing_policy: Arc::new(Empty),
            uber_estimation_policy: Arc::new(Empty),
            uber_split_policy: Arc::new(Empty),
            catalogs: Arc::new(Empty),
            carts: Arc::new(Empty),
            orders: Arc::new(spy),
            order_conversations: Arc::new(Empty),
            customers: Arc::new(Empty),
            deliveries: Arc::new(InMemoryDeliveries(Arc::new(Mutex::new(None)))),
            refunds: Arc::new(Empty),
            delivery_satisfaction: Arc::new(Empty),
            delivery_partner_availabilities: Arc::new(Empty),
            reclamations: Arc::new(Empty),
            customer_credit: Arc::new(Empty),
            mailbox_lanes: Arc::new(Empty),
        }),
        None,
        None,
    )
}

/// A request that carries NO ReadScope (a context assembly bug, or the schema executed outside a
/// transport) must reach the read port as PUBLIC — never as anything wider.
#[tokio::test]
async fn a_scopeless_request_reaches_the_order_port_as_public() {
    let spy = SpyOrders::default();
    let schema = schema_over_spy(spy.clone());

    let resp = schema
        .execute(Request::new("query { orders { id } }").data(RequestRole::Customer))
        .await;
    assert!(resp.errors.is_empty(), "orders errored: {:?}", resp.errors);
    let resp = schema
        .execute(
            Request::new(format!(
                r#"query {{ order(input: {{ id: "{}" }}) {{ id }} }}"#,
                uuid::Uuid::new_v4()
            ))
            .data(RequestRole::Customer),
        )
        .await;
    assert!(resp.errors.is_empty(), "order errored: {:?}", resp.errors);

    let seen = spy.0.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec![application::queries::ReadScope::Public, application::queries::ReadScope::Public],
        "the fallback direction must be Public (deny), never a wider scope"
    );
}

/// Positive control for the spy: an injected scope travels through unchanged.
#[tokio::test]
async fn an_injected_scope_reaches_the_order_port_verbatim() {
    let spy = SpyOrders::default();
    let schema = schema_over_spy(spy.clone());
    let customer = ds::CustomerId(uuid::Uuid::new_v4());

    let resp = schema
        .execute(
            Request::new("query { orders { id } }")
                .data(RequestRole::Customer)
                .data(application::queries::ReadScope::Customer(customer)),
        )
        .await;
    assert!(resp.errors.is_empty(), "orders errored: {:?}", resp.errors);
    let seen = spy.0.lock().unwrap().clone();
    assert_eq!(seen, vec![application::queries::ReadScope::Customer(customer)]);
}
