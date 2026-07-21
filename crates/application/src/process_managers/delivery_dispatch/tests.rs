//! Behaviour tests for the DeliveryDispatchProcess saga (resolve→walk, #60). Each links to its
//! rules.yaml rule. The ranked walk is asserted with a spy `DeliveryService` (records offered
//! channels) and a configurable strategy double.

use super::*;
use crate::dispatch_strategy::{RankedChannel, RestaurantDispatch};
use crate::pm_state::mem::MemDeliveryDispatchState;
use crate::process_managers::test_support::{envelope, MemStore};
use crate::queries::{OrderFilter, OrderTrackingRow};
use async_trait::async_trait;
use domain::generated::entities::{Courier, CustomerContact, Money, PaymentBreakdown};
use domain::generated::events::{OrderDelivered, OrderPlaced, RestaurantRegistered};
use domain::generated::scalars::*;
use std::sync::Mutex;

fn uid(n: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128(n)
}
fn order_id() -> OrderId {
    OrderId(uid(1))
}
fn restaurant_id() -> RestaurantId {
    RestaurantId(uid(3))
}
fn job_id() -> DeliveryJobId {
    delivery_job_id_for(&order_id())
}
fn eur(cents: i64) -> Money {
    Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
}
fn channel(k: &str) -> DeliveryChannelKey {
    DeliveryChannelKey(k.into())
}
fn address(line1: &str) -> Address {
    Address {
        line1: AddressLine(line1.into()),
        line2: None,
        postal_code: PostalCode("37000".into()),
        city: CityName("Tours".into()),
        country: CountryCode("FR".into()),
    }
}

// ---- Doubles --------------------------------------------------------------------------------

/// Records every channel `offer_job` is invoked against (so a test can assert the ranked walk and
/// that self-dispatch offers NOTHING).
#[derive(Default)]
struct SpyDelivery {
    offered: Mutex<Vec<DeliveryChannelKey>>,
}
impl SpyDelivery {
    fn channels(&self) -> Vec<DeliveryChannelKey> {
        self.offered.lock().unwrap().clone()
    }
}
#[async_trait]
impl DeliveryService for SpyDelivery {
    async fn offer_job(
        &self,
        input: DeliveryOfferJobInput,
        _meta: &crate::generated::services::ServiceCallMeta,
    ) -> Result<(), DomainError> {
        self.offered.lock().unwrap().push(input.channel);
        Ok(())
    }
}

/// Configurable dispatch-strategy double. Default: every restaurant is CAPTAIN with the Tours seed
/// ranking `[1 independent, 2 uber_direct]`.
struct FakeStrategy {
    mode: RestaurantDispatchMode,
    ranked: Vec<RankedChannel>,
}
impl Default for FakeStrategy {
    fn default() -> Self {
        Self {
            mode: RestaurantDispatchMode::CAPTAIN,
            ranked: vec![
                RankedChannel { rank: 1, channel: channel("independent"), ttl_override_seconds: None },
                RankedChannel { rank: 2, channel: channel("uber_direct"), ttl_override_seconds: None },
            ],
        }
    }
}
impl FakeStrategy {
    fn self_dispatched() -> Self {
        Self { mode: RestaurantDispatchMode::RESTAURANT, ranked: Vec::new() }
    }
}
#[async_trait]
impl DispatchStrategyRepository for FakeStrategy {
    async fn restaurant_dispatch(
        &self,
        _restaurant_id: RestaurantId,
    ) -> Result<RestaurantDispatch, DomainError> {
        Ok(RestaurantDispatch { mode: self.mode, city_id: Some(CityId(uid(0x70))) })
    }
    async fn ranked_channels(
        &self,
        _city_id: Option<CityId>,
    ) -> Result<Vec<RankedChannel>, DomainError> {
        Ok(self.ranked.clone())
    }
    async fn channel_default_ttl_seconds(
        &self,
        _channel: &DeliveryChannelKey,
    ) -> Result<Option<i32>, DomainError> {
        Ok(Some(120))
    }
}

fn tracking_row(service_type: ServiceType) -> OrderTrackingRow {
    OrderTrackingRow {
        order_id: order_id(),
        r#ref: ExternalReference("order-1".into()),
        restaurant_id: restaurant_id(),
        customer_id: None,
        status: OrderStatus::READY,
        service_type,
        items: serde_json::json!([]),
        total_amount_cents: MoneyCents(1960),
        currency: CurrencyCode("EUR".into()),
        articles_cents: MoneyCents(1960),
        delivery_cents: MoneyCents(0),
        service_fee_cents: MoneyCents(0),
        restaurant_payout_cents: MoneyCents(1960),
        rider_payout_cents: MoneyCents(0),
        captain_net_cents: MoneyCents(0),
        uber_total_cents: None,
        uber_restaurant_cents: None,
        uber_rider_cents: None,
        uber_platform_cents: None,
        uber_basis: None,
        delivery_address: (service_type == ServiceType::DELIVERY)
            .then(|| serde_json::to_value(address("9 Rue Colbert")).unwrap()),
        estimated_ready_at: None,
        placed_at: chrono::Utc::now(),
        status_changed_at: chrono::Utc::now(),
        payment_intent_id: Some(PaymentIntentId("pi_123".into())),
        payment_status: "CAPTURED".into(),
        restaurant_stars: None,
        rating_comment: None,
        rider_thumb: None,
        rider_tip_cents: None,
        restaurant_tip_cents: None,
        captain_tip_cents: None,
        rated_at: None,
        delivery_status: None,
        courier: None,
        estimated_dropoff_at: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

struct FakeOrders {
    row: Option<OrderTrackingRow>,
}
#[async_trait]
impl OrderReadRepository for FakeOrders {
    async fn list(&self, _filter: OrderFilter) -> Result<Vec<OrderTrackingRow>, DomainError> {
        Ok(self.row.clone().into_iter().collect())
    }
    async fn by_id(&self, id: OrderId) -> Result<Option<OrderTrackingRow>, DomainError> {
        Ok(self.row.clone().filter(|r| r.order_id == id))
    }
}

fn placed(service_type: ServiceType) -> DomainEvent {
    DomainEvent::OrderPlaced(OrderPlaced {
        mode: None,
        order_id: order_id(),
        r#ref: None,
        restaurant_id: restaurant_id(),
        customer_id: None,
        customer_contact: CustomerContact {
            display_name: CustomerDisplayName("Johnny".into()),
            email: None,
            phone: PhoneNumber("+33612345678".into()),
        },
        service_type,
        delivery_address: (service_type == ServiceType::DELIVERY).then(|| address("9 Rue Colbert")),
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
        payment_intent_id: PaymentIntentId("pi_123".into()),
    })
}

fn registered_restaurant() -> Vec<DomainEvent> {
    vec![DomainEvent::RestaurantRegistered(RestaurantRegistered {
        mode: None,
        restaurant_id: restaurant_id(),
        account_id: None,
        listing_status: RestaurantListingStatus::ACTIVE_PARTNER,
        r#ref: None,
        external_identifiers: Vec::new(),
        slug: Slug("chez-marco".into()),
        display_name: RestaurantDisplayName("Chez Marco".into()),
        contact: None,
        website: None,
        tags: Vec::new(),
        margin_rate: None,
        cuisine_category: None,
        uber_prices_opt_in: None,
        address: address("1 Rue Nationale"),
        location: None,
        timezone: None,
        preparation_time_minutes: None,
        opening_hours: Vec::new(),
    })]
}

fn ready() -> OrderMarkedReady {
    OrderMarkedReady { order_id: order_id(), restaurant_id: restaurant_id() }
}

fn given_ready(store: &MemStore) {
    store.seed(
        &format!("Order-{}", order_id().0),
        vec![placed(ServiceType::DELIVERY), DomainEvent::OrderMarkedReady(ready())],
    );
    store.seed(&format!("Restaurant-{}", restaurant_id().0), registered_restaurant());
}

/// GIVEN: a CAPTAIN-dispatched OFFERED run (state after the birth leg) with the default ranking.
async fn given_offered(store: &MemStore, state: &MemDeliveryDispatchState, delivery: &SpyDelivery) {
    given_ready(store);
    let orders = FakeOrders { row: Some(tracking_row(ServiceType::DELIVERY)) };
    on_order_marked_ready(store, state, &orders, delivery, &FakeStrategy::default(), &ready(), &envelope())
        .await
        .unwrap();
}

fn declined(reason: &str) -> DeliveryRejectedByPartner {
    DeliveryRejectedByPartner { delivery_job_id: job_id(), partner_ref: None, reason: Some(reason.into()) }
}

// ---- Birth leg ------------------------------------------------------------------------------

/// rules.yaml#/ReadyDeliveryOrderTriggersDispatch + rules.yaml#/CityRankingWalkedInOrder: a ready
/// CAPTAIN DELIVERY order births the job, offers the RANK-1 channel, row → OFFERED at rank 1.
#[tokio::test]
async fn ready_delivery_order_offers_rank1_and_opens_offered() {
    let store = MemStore::default();
    let state = MemDeliveryDispatchState::default();
    let delivery = SpyDelivery::default();
    given_ready(&store);
    let orders = FakeOrders { row: Some(tracking_row(ServiceType::DELIVERY)) };

    let outcome = on_order_marked_ready(
        &store,
        &state,
        &orders,
        &delivery,
        &FakeStrategy::default(),
        &ready(),
        &envelope(),
    )
    .await
    .unwrap();
    assert_eq!(outcome, Outcome::Completed);

    let job_events = store.stream(&format!("DeliveryJob-{}", job_id().0));
    let requested = job_events
        .iter()
        .find_map(|e| match e {
            DomainEvent::DeliveryRequested(r) => Some(r.clone()),
            _ => None,
        })
        .expect("DeliveryRequested birth");
    assert_eq!(requested.delivery_job_id, job_id());
    assert_eq!(requested.pickup, address("1 Rue Nationale"));
    assert_eq!(requested.dropoff, address("9 Rue Colbert"));
    assert_eq!(delivery.channels(), vec![channel("independent")]); // rank-1 offered
    let row = state.by_delivery_job(job_id()).await.unwrap().unwrap();
    assert_eq!(row.process_status, DeliveryDispatchProcessStatus::OFFERED);
    assert_eq!(row.current_rank, Some(1));
    assert_eq!(row.current_channel, Some(channel("independent")));

    // Idempotent re-reaction: the deterministic job stream absorbs the replay.
    let again = on_order_marked_ready(
        &store,
        &state,
        &orders,
        &delivery,
        &FakeStrategy::default(),
        &ready(),
        &envelope(),
    )
    .await
    .unwrap();
    assert_eq!(again, Outcome::Completed);
    assert_eq!(store.stream(&format!("DeliveryJob-{}", job_id().0)).len(), 1);
}

/// rules.yaml#/RestaurantDispatchBypassesRouting: a RESTAURANT-dispatch order still creates the job
/// (customer tracking) but Captain offers NO channel; the run opens SELF_DISPATCHED.
#[tokio::test]
async fn self_dispatched_order_creates_job_but_offers_nothing() {
    let store = MemStore::default();
    let state = MemDeliveryDispatchState::default();
    let delivery = SpyDelivery::default();
    given_ready(&store);
    let orders = FakeOrders { row: Some(tracking_row(ServiceType::DELIVERY)) };

    let outcome = on_order_marked_ready(
        &store,
        &state,
        &orders,
        &delivery,
        &FakeStrategy::self_dispatched(),
        &ready(),
        &envelope(),
    )
    .await
    .unwrap();
    assert_eq!(outcome, Outcome::Completed);

    // The job is born (tracking) but nothing was offered.
    assert!(store
        .stream(&format!("DeliveryJob-{}", job_id().0))
        .iter()
        .any(|e| matches!(e, DomainEvent::DeliveryRequested(_))));
    assert!(delivery.channels().is_empty(), "self-dispatch offers no channel");
    let row = state.by_delivery_job(job_id()).await.unwrap().unwrap();
    assert_eq!(row.process_status, DeliveryDispatchProcessStatus::SELF_DISPATCHED);
    assert_eq!(row.current_rank, None);
    assert_eq!(row.current_channel, None);
}

/// rules.yaml#/ReadyDeliveryOrderTriggersDispatch (COLLECTION corollary): no dispatch.
#[tokio::test]
async fn collection_orders_are_not_dispatched() {
    let store = MemStore::default();
    let state = MemDeliveryDispatchState::default();
    let delivery = SpyDelivery::default();
    store.seed(
        &format!("Order-{}", order_id().0),
        vec![placed(ServiceType::COLLECTION), DomainEvent::OrderMarkedReady(ready())],
    );
    store.seed(&format!("Restaurant-{}", restaurant_id().0), registered_restaurant());
    let orders = FakeOrders { row: Some(tracking_row(ServiceType::COLLECTION)) };

    let outcome = on_order_marked_ready(
        &store,
        &state,
        &orders,
        &delivery,
        &FakeStrategy::default(),
        &ready(),
        &envelope(),
    )
    .await
    .unwrap();
    assert!(matches!(outcome, Outcome::Skipped(_)), "{outcome:?}");
    assert!(store.stream(&format!("DeliveryJob-{}", job_id().0)).is_empty());
    assert!(state.by_order(order_id()).await.unwrap().is_none());
}

// ---- Advance walk ---------------------------------------------------------------------------

/// rules.yaml#/PartnerAcceptanceRecordsCourier: the inbound acceptance advances the run to ACCEPTED.
#[tokio::test]
async fn partner_acceptance_advances_the_run() {
    let store = MemStore::default();
    let state = MemDeliveryDispatchState::default();
    given_offered(&store, &state, &SpyDelivery::default()).await;

    let outcome = on_delivery_accepted_by_partner(
        &state,
        &DeliveryAcceptedByPartner {
            delivery_job_id: job_id(),
            partner_ref: ExternalReference("avelo-77".into()),
            courier: Courier { display_name: "Léa".into(), phone: None, rider_id: None },
            estimated_pickup_at: None,
            estimated_dropoff_at: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(outcome, Outcome::Completed);
    assert_eq!(
        state.by_delivery_job(job_id()).await.unwrap().unwrap().process_status,
        DeliveryDispatchProcessStatus::ACCEPTED
    );
}

/// rules.yaml#/CityRankingWalkedInOrder: a decline at rank 1 offers the RANK-2 channel — the run
/// stays OFFERED, advances to rank 2, and records no failure.
#[tokio::test]
async fn decline_walks_to_the_next_ranked_channel() {
    let store = MemStore::default();
    let state = MemDeliveryDispatchState::default();
    let delivery = SpyDelivery::default();
    given_offered(&store, &state, &delivery).await;

    let outcome = on_delivery_rejected_by_partner(
        &store,
        &state,
        &delivery,
        &FakeStrategy::default(),
        &declined("No courier available"),
        &envelope(),
    )
    .await
    .unwrap();
    assert_eq!(outcome, Outcome::Completed);
    let row = state.by_delivery_job(job_id()).await.unwrap().unwrap();
    assert_eq!(row.process_status, DeliveryDispatchProcessStatus::OFFERED);
    assert_eq!(row.offer_attempts, 2);
    assert_eq!(row.current_rank, Some(2));
    assert_eq!(row.current_channel, Some(channel("uber_direct")));
    assert_eq!(delivery.channels(), vec![channel("independent"), channel("uber_direct")]);
    assert!(!store
        .stream(&format!("DeliveryJob-{}", job_id().0))
        .iter()
        .any(|e| matches!(e, DomainEvent::DeliveryDispatchFailed(_))));
}

/// rules.yaml#/DispatchExhaustionFailsClosed: a decline of the LAST ranked channel fails the
/// dispatch closed (DeliveryDispatchFailed + FAILED); a re-delivered decline then skips.
#[tokio::test]
async fn exhausting_the_walk_fails_the_dispatch_closed() {
    let store = MemStore::default();
    let state = MemDeliveryDispatchState::default();
    let delivery = SpyDelivery::default();
    given_offered(&store, &state, &delivery).await;

    // Decline 1 → walk to rank 2 (uber_direct).
    on_delivery_rejected_by_partner(
        &store,
        &state,
        &delivery,
        &FakeStrategy::default(),
        &declined("No courier"),
        &envelope(),
    )
    .await
    .unwrap();

    // Decline 2 (last ranked channel) → fail closed, no third offer.
    let outcome = on_delivery_rejected_by_partner(
        &store,
        &state,
        &delivery,
        &FakeStrategy::default(),
        &declined("Storm — no couriers tonight"),
        &envelope(),
    )
    .await
    .unwrap();
    assert_eq!(outcome, Outcome::Completed);
    let row = state.by_delivery_job(job_id()).await.unwrap().unwrap();
    assert_eq!(row.process_status, DeliveryDispatchProcessStatus::FAILED);
    assert_eq!(delivery.channels(), vec![channel("independent"), channel("uber_direct")]);
    let job_events = store.stream(&format!("DeliveryJob-{}", job_id().0));
    let failed = job_events
        .iter()
        .find_map(|e| match e {
            DomainEvent::DeliveryDispatchFailed(f) => Some(f.clone()),
            _ => None,
        })
        .expect("DeliveryDispatchFailed recorded");
    assert_eq!(failed.order_id, order_id());
    assert_eq!(failed.attempts, 2);
    assert_eq!(failed.last_reason.as_deref(), Some("Storm — no couriers tonight"));
    assert_eq!(domain::delivery_job::fold(&job_events).unwrap().status, DeliveryStatus::FAILED);

    // Re-delivered decline on the FAILED run: benign skip, nothing appended.
    let again = on_delivery_rejected_by_partner(
        &store,
        &state,
        &delivery,
        &FakeStrategy::default(),
        &declined("No courier"),
        &envelope(),
    )
    .await
    .unwrap();
    assert!(matches!(again, Outcome::Skipped(_)), "{again:?}");
    assert_eq!(store.stream(&format!("DeliveryJob-{}", job_id().0)), job_events);
}

/// rules.yaml#/TimeoutEscalatesToNextChannel: an offer timeout advances the walk identically to a
/// decline (rank 1 → rank 2).
#[tokio::test]
async fn timeout_advances_the_walk() {
    let store = MemStore::default();
    let state = MemDeliveryDispatchState::default();
    let delivery = SpyDelivery::default();
    given_offered(&store, &state, &delivery).await;

    let outcome = on_delivery_offer_timed_out(
        &store,
        &state,
        &delivery,
        &FakeStrategy::default(),
        &DeliveryOfferTimedOut {
            delivery_job_id: job_id(),
            channel: channel("independent"),
            rank: 1,
            reason: Some("offer expired".into()),
        },
        &envelope(),
    )
    .await
    .unwrap();
    assert_eq!(outcome, Outcome::Completed);
    let row = state.by_delivery_job(job_id()).await.unwrap().unwrap();
    assert_eq!(row.process_status, DeliveryDispatchProcessStatus::OFFERED);
    assert_eq!(row.current_rank, Some(2));
    assert_eq!(delivery.channels(), vec![channel("independent"), channel("uber_direct")]);
}

/// rules.yaml#/ManualEscalateSkipsChannel: a manual escalate advances the walk identically to a
/// decline (rank 1 → rank 2).
#[tokio::test]
async fn escalate_advances_the_walk() {
    let store = MemStore::default();
    let state = MemDeliveryDispatchState::default();
    let delivery = SpyDelivery::default();
    given_offered(&store, &state, &delivery).await;

    let outcome = on_delivery_escalation_requested(
        &store,
        &state,
        &delivery,
        &FakeStrategy::default(),
        &DeliveryEscalationRequested { delivery_job_id: job_id(), reason: Some("too slow".into()) },
        &envelope(),
    )
    .await
    .unwrap();
    assert_eq!(outcome, Outcome::Completed);
    let row = state.by_delivery_job(job_id()).await.unwrap().unwrap();
    assert_eq!(row.process_status, DeliveryDispatchProcessStatus::OFFERED);
    assert_eq!(row.current_rank, Some(2));
    assert_eq!(delivery.channels(), vec![channel("independent"), channel("uber_direct")]);
}

/// An unknown dispatch run throws `errors.yaml#/DeliveryJobNotFound` (abort and surface).
#[tokio::test]
async fn unknown_job_is_flagged_with_the_typed_error() {
    let store = MemStore::default();
    let state = MemDeliveryDispatchState::default();
    let delivery = SpyDelivery::default();
    let unknown = DeliveryJobId(uid(0xBAD));

    let err = on_delivery_accepted_by_partner(
        &state,
        &DeliveryAcceptedByPartner {
            delivery_job_id: unknown,
            partner_ref: ExternalReference("avelo-77".into()),
            courier: Courier { display_name: "Léa".into(), phone: None, rider_id: None },
            estimated_pickup_at: None,
            estimated_dropoff_at: None,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), Some("DeliveryJobNotFound"), "{err:?}");

    let err = on_delivery_offer_timed_out(
        &store,
        &state,
        &delivery,
        &FakeStrategy::default(),
        &DeliveryOfferTimedOut {
            delivery_job_id: unknown,
            channel: channel("independent"),
            rank: 1,
            reason: None,
        },
        &envelope(),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code(), Some("DeliveryJobNotFound"), "{err:?}");
}

/// rules.yaml#/OrderClosedOnDeliveryCompletion: a partner DELIVERED closes the order COMPLETED;
/// intermediate statuses skip.
#[tokio::test]
async fn partner_delivered_status_closes_the_order() {
    let store = MemStore::default();
    let state = MemDeliveryDispatchState::default();
    given_offered(&store, &state, &SpyDelivery::default()).await;

    let trigger = DeliveryStatusUpdated {
        delivery_job_id: job_id(),
        partner_ref: None,
        status: DeliveryStatus::DELIVERED,
        occurred_at: None,
        note: None,
    };
    let outcome = on_delivery_status_updated(&store, &state, &trigger, &envelope()).await.unwrap();
    assert_eq!(outcome, Outcome::Completed);
    let order_events = store.stream(&format!("Order-{}", order_id().0));
    assert!(order_events.iter().any(|e| matches!(
        e,
        DomainEvent::OrderDelivered(OrderDelivered { order_id: o, .. }) if *o == order_id()
    )));
    assert_eq!(domain::order::fold(&order_events).unwrap().status, OrderStatus::DELIVERED);
    assert_eq!(
        state.by_delivery_job(job_id()).await.unwrap().unwrap().process_status,
        DeliveryDispatchProcessStatus::COMPLETED
    );
}
