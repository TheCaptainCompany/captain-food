//! DeliveryDispatchProcess (`specs/processmanager.yaml#/DeliveryDispatchProcess`, ADR-0031) — HOOK
//! IMPLS + thin wrappers for the GENERATED leg pipelines
//! (`crate::generated::process_managers::delivery_dispatch_process`, issue #25). The pipelines
//! (state by/expect/set, the linear-branch marker, deliver/send plumbing) are generated; this module
//! keeps only the non-structural seams:
//!
//! - the OrderTracking / Restaurant reads (the pickup address folds from the restaurant's own
//!   stream, like the restaurant command handlers);
//! - `build_delivery_requested` — the DETERMINISTIC UUIDv5 job id ([`delivery_job_id_for`], the
//!   idempotency key), the order's test-mode lineage (ADR-0038), and the pickup/dropoff unwraps;
//! - the bounded re-offer `branch` + `compute_offer_attempts` (cap [`OFFER_ATTEMPT_CAP`],
//!   ADR-20260720-004556, rules.yaml#/DispatchRetriesAreBounded);
//! - the DeliveryJob fold predicates (birth exists / not already FAILED).

use domain::generated::entities::Address;
use domain::generated::events::{
    DeliveryAcceptedByPartner, DeliveryCompleted, DeliveryDispatchFailed, DeliveryRejectedByPartner,
    DeliveryRequested, DeliveryStatusUpdated, DomainEvent, OrderMarkedReady,
};
use domain::generated::scalars::{DeliveryStatus, OrderId, RestaurantId};
use domain::shared::errors::DomainError;

use crate::generated::process_managers::delivery_dispatch_process::{self, OrderRead, RestaurantRead};
use crate::generated::process_managers::HookOutcome;
use crate::generated::services::{DeliveryOfferJobInput, DeliveryService};
use crate::pm_state::{DeliveryDispatchRow, DeliveryDispatchStateStore};
use crate::ports::EventStore;
use crate::process_managers::{
    delivery_job_stream, order_stream, restaurant_stream, Outcome, TriggerEnvelope,
};
use crate::queries::OrderReadRepository;

/// The bounded re-offer cap (ADR-20260720-004556, rules.yaml#/DispatchRetriesAreBounded): at most
/// this many TOTAL offers per dispatch run (the birth offer counts as 1). The decline that finds the
/// counter already at the cap fails the dispatch closed — never a retry loop beyond it.
pub const OFFER_ATTEMPT_CAP: i32 = 3;

/// Fixed UUIDv5 namespace for the ids this saga derives. NEVER change it: derived job ids must stay
/// stable across re-reactions and deployments (they ARE the idempotency key).
fn dispatch_namespace() -> uuid::Uuid {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        b"https://captain.food/process-managers/delivery-dispatch",
    )
}

/// The deterministic delivery job for an order (one V0 job per order; a future re-dispatch policy
/// would version the derivation, not randomize it).
pub fn delivery_job_id_for(order_id: &OrderId) -> domain::generated::scalars::DeliveryJobId {
    domain::generated::scalars::DeliveryJobId(uuid::Uuid::new_v5(&dispatch_namespace(), order_id.0.as_bytes()))
}

/// The restaurant's current address — the job's pickup point, folded from ITS stream (registration
/// carries it, a later `RestaurantUpdated` may replace it).
fn restaurant_address(restaurant_events: &[DomainEvent]) -> Option<Address> {
    restaurant_events.iter().fold(None, |addr, e| match e {
        DomainEvent::RestaurantRegistered(r) => Some(r.address.clone()),
        DomainEvent::RestaurantUpdated(u) => u.address.clone().or(addr),
        _ => addr,
    })
}

/// Hooks for the `OrderMarkedReady` leg: reads, the derived job birth, the offer input.
pub struct DispatchOpenHooks<'a> {
    pub store: &'a dyn EventStore,
    pub orders: &'a dyn OrderReadRepository,
}

#[async_trait::async_trait]
impl delivery_dispatch_process::OrderMarkedReadyHooks for DispatchOpenHooks<'_> {
    async fn read_order(&self, order_id: OrderId) -> Result<HookOutcome<OrderRead>, DomainError> {
        let Some(order) = self.orders.by_id(order_id).await? else {
            return Ok(HookOutcome::Skip(format!(
                "order {} is not in the OrderTracking read model yet — cannot dispatch its delivery",
                order_id.0
            )));
        };
        Ok(HookOutcome::Ready(OrderRead {
            service_type: order.service_type,
            delivery_address: order
                .delivery_address
                .and_then(|v| serde_json::from_value::<Address>(v).ok()),
        }))
    }

    /// The pickup address — folded from the aggregate's own stream, like the restaurant command
    /// handlers (the projected row carries the same address as jsonb).
    async fn read_restaurant(
        &self,
        restaurant_id: RestaurantId,
    ) -> Result<HookOutcome<RestaurantRead>, DomainError> {
        let (restaurant_events, _) = self.store.load(&restaurant_stream(&restaurant_id)).await?;
        Ok(HookOutcome::Ready(RestaurantRead { address: restaurant_address(&restaurant_events) }))
    }

    async fn build_delivery_requested(
        &self,
        event: &OrderMarkedReady,
        order: &OrderRead,
        restaurant: &RestaurantRead,
    ) -> Result<HookOutcome<DeliveryRequested>, DomainError> {
        let Some(dropoff) = order.delivery_address.clone() else {
            return Ok(HookOutcome::Skip(format!(
                "DELIVERY order {} carries no decodable delivery address — cannot dispatch",
                event.order_id.0
            )));
        };
        let Some(pickup) = restaurant.address.clone() else {
            return Ok(HookOutcome::Skip(format!(
                "restaurant {} has no address on its stream — cannot set the pickup for order {}",
                event.restaurant_id.0, event.order_id.0
            )));
        };
        // Test-mode lineage (ADR-0038): the job inherits the ORDER's mode, read from the
        // authoritative OrderPlaced fact (OrderTracking does not carry `mode`).
        let (order_events, _) = self.store.load(&order_stream(&event.order_id)).await?;
        let mode = order_events.iter().find_map(|e| match e {
            DomainEvent::OrderPlaced(p) => Some(p.mode),
            _ => None,
        });
        Ok(HookOutcome::Ready(DeliveryRequested {
            mode: mode.flatten(),
            delivery_job_id: delivery_job_id_for(&event.order_id),
            order_id: event.order_id,
            restaurant_id: event.restaurant_id,
            pickup,
            dropoff,
            // TODO(saga): partner pre-selection (e.g. Avelo37 vs independent riders) is a dispatch
            // policy the delivery-partner ACL will own; V0 leaves the job open to both channels.
            provider: None,
        }))
    }

    /// Idempotent re-dispatch: the deterministic id targets the same stream, whose fold absorbs the replay.
    fn should_deliver_delivery_requested(&self, stream: &[DomainEvent], _event: &DeliveryRequested) -> bool {
        domain::delivery_job::fold(stream).is_none()
    }

    async fn input_delivery_offer_job(
        &self,
        _event: &OrderMarkedReady,
        _order: &OrderRead,
        _restaurant: &RestaurantRead,
        delivery_requested: &DeliveryRequested,
    ) -> Result<HookOutcome<DeliveryOfferJobInput>, DomainError> {
        Ok(HookOutcome::Ready(DeliveryOfferJobInput { job: delivery_requested.clone() }))
    }
}

/// Hooks for the `DeliveryRejectedByPartner` leg: the bounded re-offer branch (ADR-20260720-004556).
pub struct DispatchRejectedHooks<'a> {
    pub store: &'a dyn EventStore,
}

#[async_trait::async_trait]
impl delivery_dispatch_process::DeliveryRejectedByPartnerHooks for DispatchRejectedHooks<'_> {
    /// RE-OFFER while the run has made fewer than [`OFFER_ATTEMPT_CAP`] total offers; otherwise the
    /// EXHAUSTED branch fails the dispatch closed.
    fn branch(&self, row: &DeliveryDispatchRow) -> bool {
        row.offer_attempts < OFFER_ATTEMPT_CAP
    }

    /// The re-offer carries the job's own birth fact (the offer payload IS DeliveryRequested).
    async fn input_delivery_offer_job(
        &self,
        event: &DeliveryRejectedByPartner,
        _row: &DeliveryDispatchRow,
    ) -> Result<HookOutcome<DeliveryOfferJobInput>, DomainError> {
        let (job_events, _) = self.store.load(&delivery_job_stream(&event.delivery_job_id)).await?;
        match job_events.iter().find_map(|e| match e {
            DomainEvent::DeliveryRequested(r) => Some(r.clone()),
            _ => None,
        }) {
            Some(requested) => Ok(HookOutcome::Ready(DeliveryOfferJobInput { job: requested })),
            None => Ok(HookOutcome::Skip(format!(
                "job {} has no DeliveryRequested birth to re-offer",
                event.delivery_job_id.0
            ))),
        }
    }

    /// offer_attempts := offer_attempts + 1 — the arithmetic the DSL value forms cannot carry.
    fn compute_offer_attempts(&self, row: &DeliveryDispatchRow) -> i32 {
        row.offer_attempts + 1
    }

    /// Idempotent: skipped when the job already folds FAILED.
    fn should_deliver_delivery_dispatch_failed(
        &self,
        stream: &[DomainEvent],
        _event: &DeliveryDispatchFailed,
    ) -> bool {
        !domain::delivery_job::fold(stream)
            .map(|s| s.status == DeliveryStatus::FAILED)
            .unwrap_or(false)
    }
}

/// Default-only hooks for the advance/close legs (accepted, status-updated, completed).
pub struct DispatchAdvanceHooks;

impl delivery_dispatch_process::DeliveryAcceptedByPartnerHooks for DispatchAdvanceHooks {}
impl delivery_dispatch_process::DeliveryStatusUpdatedHooks for DispatchAdvanceHooks {}
impl delivery_dispatch_process::DeliveryCompletedHooks for DispatchAdvanceHooks {}

/// EVENT leg `events.yaml#/OrderMarkedReady` (rules.yaml#/ReadyDeliveryOrderTriggersDispatch).
pub async fn on_order_marked_ready(
    store: &dyn EventStore,
    state: &dyn DeliveryDispatchStateStore,
    orders: &dyn OrderReadRepository,
    partner: &dyn DeliveryService,
    event: &OrderMarkedReady,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    delivery_dispatch_process::on_order_marked_ready(
        store,
        state,
        partner,
        &DispatchOpenHooks { store, orders },
        event,
        env,
    )
    .await
}

/// EVENT leg `events.yaml#/DeliveryAcceptedByPartner` (rules.yaml#/PartnerAcceptanceRecordsCourier).
pub async fn on_delivery_accepted_by_partner(
    state: &dyn DeliveryDispatchStateStore,
    event: &DeliveryAcceptedByPartner,
) -> Result<Outcome, DomainError> {
    delivery_dispatch_process::on_delivery_accepted_by_partner(state, &DispatchAdvanceHooks, event).await
}

/// EVENT leg `events.yaml#/DeliveryRejectedByPartner` (rules.yaml#/PartnerRejectionReoffers +
/// rules.yaml#/DispatchRetriesAreBounded, ADR-20260720-004556).
pub async fn on_delivery_rejected_by_partner(
    store: &dyn EventStore,
    state: &dyn DeliveryDispatchStateStore,
    partner: &dyn DeliveryService,
    event: &DeliveryRejectedByPartner,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    delivery_dispatch_process::on_delivery_rejected_by_partner(
        store,
        state,
        partner,
        &DispatchRejectedHooks { store },
        event,
        env,
    )
    .await
}

/// EVENT leg `events.yaml#/DeliveryStatusUpdated` (rules.yaml#/OrderClosedOnDeliveryCompletion):
/// only the terminal DELIVERED report closes the order.
pub async fn on_delivery_status_updated(
    store: &dyn EventStore,
    state: &dyn DeliveryDispatchStateStore,
    event: &DeliveryStatusUpdated,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    delivery_dispatch_process::on_delivery_status_updated(store, state, &DispatchAdvanceHooks, event, env)
        .await
}

/// EVENT leg `events.yaml#/DeliveryCompleted` (rules.yaml#/OrderClosedOnDeliveryCompletion).
pub async fn on_delivery_completed(
    store: &dyn EventStore,
    state: &dyn DeliveryDispatchStateStore,
    event: &DeliveryCompleted,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    delivery_dispatch_process::on_delivery_completed(store, state, &DispatchAdvanceHooks, event, env)
        .await
}

// ================================================================================================
// Behaviour tests (specs/tests.yaml, DeliveryDispatchProcess saga) — each linked to its rules.yaml
// rule.
// ================================================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm_state::mem::MemDeliveryDispatchState;
    use crate::ports::NoopDeliveryService;
    use crate::process_managers::test_support::{envelope, MemStore};
    use crate::queries::{OrderFilter, OrderTrackingRow};
    use async_trait::async_trait;
    use domain::generated::entities::{Courier, CustomerContact, Money, PaymentBreakdown};
    use domain::generated::events::{
        DeliveryAcceptedByRider, DeliveryPickedUp, OrderDelivered, OrderPlaced,
        RestaurantRegistered,
    };
    use domain::generated::scalars::*;

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
    fn address(line1: &str) -> Address {
        Address {
            line1: AddressLine(line1.into()),
            line2: None,
            postal_code: PostalCode("37000".into()),
            city: CityName("Tours".into()),
            country: CountryCode("FR".into()),
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
            delivery_address: (service_type == ServiceType::DELIVERY)
                .then(|| address("9 Rue Colbert")),
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

    /// GIVEN wiring: READY DELIVERY order (stream + read model) and the registered restaurant.
    fn given_ready(store: &MemStore) {
        store.seed(
            &format!("Order-{}", order_id().0),
            vec![placed(ServiceType::DELIVERY), DomainEvent::OrderMarkedReady(ready())],
        );
        store.seed(&format!("Restaurant-{}", restaurant_id().0), registered_restaurant());
    }

    /// GIVEN: a dispatched (OFFERED) run — the state after [`on_order_marked_ready`].
    async fn given_offered(store: &MemStore, state: &MemDeliveryDispatchState) {
        given_ready(store);
        let orders = FakeOrders { row: Some(tracking_row(ServiceType::DELIVERY)) };
        on_order_marked_ready(store, state, &orders, &NoopDeliveryService, &ready(), &envelope())
            .await
            .unwrap();
    }

    /// tests.yaml#/TestDispatchOnOrderReady — rules.yaml#/ReadyDeliveryOrderTriggersDispatch: a
    /// ready DELIVERY order births the delivery job (deterministic id), offers it, row → OFFERED.
    #[tokio::test]
    async fn ready_delivery_order_requests_and_offers_a_job() {
        let store = MemStore::default();
        let state = MemDeliveryDispatchState::default();
        given_ready(&store);
        let orders = FakeOrders { row: Some(tracking_row(ServiceType::DELIVERY)) };

        let outcome = on_order_marked_ready(
            &store,
            &state,
            &orders,
            &NoopDeliveryService,
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
        assert_eq!(requested.delivery_job_id, job_id()); // deterministic id
        assert_eq!(requested.pickup, address("1 Rue Nationale")); // restaurant address
        assert_eq!(requested.dropoff, address("9 Rue Colbert")); // order delivery address
        assert_eq!(domain::delivery_job::fold(&job_events).unwrap().status, DeliveryStatus::PENDING);
        let row = state.by_delivery_job(job_id()).await.unwrap().unwrap();
        assert_eq!(row.process_status, DeliveryDispatchProcessStatus::OFFERED);
        assert_eq!(row.order_id, order_id());
        assert_eq!(row.restaurant_id, restaurant_id());

        // Idempotent re-reaction: the deterministic job stream absorbs the replay.
        let again = on_order_marked_ready(
            &store,
            &state,
            &orders,
            &NoopDeliveryService,
            &ready(),
            &envelope(),
        )
        .await
        .unwrap();
        assert_eq!(again, Outcome::Completed);
        assert_eq!(store.stream(&format!("DeliveryJob-{}", job_id().0)).len(), 1);
    }

    /// rules.yaml#/ReadyDeliveryOrderTriggersDispatch (COLLECTION corollary): no dispatch.
    #[tokio::test]
    async fn collection_orders_are_not_dispatched() {
        let store = MemStore::default();
        let state = MemDeliveryDispatchState::default();
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
            &NoopDeliveryService,
            &ready(),
            &envelope(),
        )
        .await
        .unwrap();
        assert!(matches!(outcome, Outcome::Skipped(_)), "{outcome:?}");
        assert!(store.stream(&format!("DeliveryJob-{}", job_id().0)).is_empty());
        assert!(state.by_order(order_id()).await.unwrap().is_none());
    }

    /// tests.yaml#/TestDispatchPartnerAccepted — rules.yaml#/PartnerAcceptanceRecordsCourier: the
    /// inbound acceptance (fact on the job stream) advances the run to ACCEPTED.
    #[tokio::test]
    async fn partner_acceptance_advances_the_run() {
        let store = MemStore::default();
        let state = MemDeliveryDispatchState::default();
        given_offered(&store, &state).await;

        let outcome = on_delivery_accepted_by_partner(
            &state,
            &DeliveryAcceptedByPartner {
                delivery_job_id: job_id(),
                partner_ref: ExternalReference("avelo-77".into()),
                courier: Courier {
                    display_name: "Léa".into(),
                    phone: Some(PhoneNumber("+33611223344".into())),
                    rider_id: None,
                },
                estimated_pickup_at: None,
                estimated_dropoff_at: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome, Outcome::Completed);
        let row = state.by_delivery_job(job_id()).await.unwrap().unwrap();
        assert_eq!(row.process_status, DeliveryDispatchProcessStatus::ACCEPTED);
    }

    fn declined(reason: &str) -> DeliveryRejectedByPartner {
        DeliveryRejectedByPartner {
            delivery_job_id: job_id(),
            partner_ref: None,
            reason: Some(reason.into()),
        }
    }

    /// tests.yaml#/TestDispatchPartnerRejected — rules.yaml#/PartnerRejectionReoffers +
    /// rules.yaml#/DispatchRetriesAreBounded: a decline under the cap re-offers the job — the run
    /// stays OFFERED, the attempt counter increments, and no failure fact is recorded.
    #[tokio::test]
    async fn partner_rejection_reoffers_while_under_the_cap() {
        let store = MemStore::default();
        let state = MemDeliveryDispatchState::default();
        given_offered(&store, &state).await;

        let outcome = on_delivery_rejected_by_partner(
            &store,
            &state,
            &NoopDeliveryService,
            &declined("No courier available"),
            &envelope(),
        )
        .await
        .unwrap();
        assert_eq!(outcome, Outcome::Completed);
        let row = state.by_delivery_job(job_id()).await.unwrap().unwrap();
        assert_eq!(row.process_status, DeliveryDispatchProcessStatus::OFFERED);
        assert_eq!(row.offer_attempts, 2);
        assert!(!store
            .stream(&format!("DeliveryJob-{}", job_id().0))
            .iter()
            .any(|e| matches!(e, DomainEvent::DeliveryDispatchFailed(_))));
    }

    /// tests.yaml#/TestDispatchAcceptedAfterReoffer — rules.yaml#/DispatchRetriesAreBounded +
    /// rules.yaml#/PartnerAcceptanceRecordsCourier: the happy path after a retry — the partner
    /// accepts the re-offered job and the run advances to ACCEPTED.
    #[tokio::test]
    async fn partner_acceptance_after_reoffer_advances_the_run() {
        let store = MemStore::default();
        let state = MemDeliveryDispatchState::default();
        given_offered(&store, &state).await;
        on_delivery_rejected_by_partner(
            &store,
            &state,
            &NoopDeliveryService,
            &declined("No courier available"),
            &envelope(),
        )
        .await
        .unwrap();

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
        let row = state.by_delivery_job(job_id()).await.unwrap().unwrap();
        assert_eq!(row.process_status, DeliveryDispatchProcessStatus::ACCEPTED);
        assert_eq!(row.offer_attempts, 2); // the retry stays counted
    }

    /// tests.yaml#/TestDispatchFailsAfterOfferCap — rules.yaml#/DispatchRetriesAreBounded: the 3rd
    /// decline exhausts the cap ([`OFFER_ATTEMPT_CAP`] total offers): DeliveryDispatchFailed is
    /// recorded on the job stream (job folds FAILED) and the run closes FAILED; a re-delivered
    /// decline after that is a benign skip and appends nothing (fail-closed, no retry loop).
    #[tokio::test]
    async fn third_decline_fails_the_dispatch_closed() {
        let store = MemStore::default();
        let state = MemDeliveryDispatchState::default();
        given_offered(&store, &state).await;

        // Declines 1 and 2 re-offer (offers 2 and 3).
        for n in [2, 3] {
            let outcome = on_delivery_rejected_by_partner(
                &store,
                &state,
                &NoopDeliveryService,
                &declined("No courier available"),
                &envelope(),
            )
            .await
            .unwrap();
            assert_eq!(outcome, Outcome::Completed);
            let row = state.by_delivery_job(job_id()).await.unwrap().unwrap();
            assert_eq!(row.process_status, DeliveryDispatchProcessStatus::OFFERED);
            assert_eq!(row.offer_attempts, n);
        }

        // Decline 3 (the cap is exhausted): terminal failure, no 4th offer.
        let outcome = on_delivery_rejected_by_partner(
            &store,
            &state,
            &NoopDeliveryService,
            &declined("Storm — no couriers tonight"),
            &envelope(),
        )
        .await
        .unwrap();
        assert_eq!(outcome, Outcome::Completed);
        let row = state.by_delivery_job(job_id()).await.unwrap().unwrap();
        assert_eq!(row.process_status, DeliveryDispatchProcessStatus::FAILED);
        assert_eq!(row.offer_attempts, 3);
        let job_events = store.stream(&format!("DeliveryJob-{}", job_id().0));
        let failed = job_events
            .iter()
            .find_map(|e| match e {
                DomainEvent::DeliveryDispatchFailed(f) => Some(f.clone()),
                _ => None,
            })
            .expect("DeliveryDispatchFailed recorded");
        assert_eq!(failed.order_id, order_id());
        assert_eq!(failed.restaurant_id, restaurant_id());
        assert_eq!(failed.attempts, 3);
        assert_eq!(failed.last_reason.as_deref(), Some("Storm — no couriers tonight"));
        // The job folds FAILED — the restaurant board (View_DeliveryJob) surfaces it.
        assert_eq!(
            domain::delivery_job::fold(&job_events).unwrap().status,
            DeliveryStatus::FAILED
        );

        // Re-delivered decline on the FAILED run: benign skip, nothing appended.
        let again = on_delivery_rejected_by_partner(
            &store,
            &state,
            &NoopDeliveryService,
            &declined("No courier available"),
            &envelope(),
        )
        .await
        .unwrap();
        assert!(matches!(again, Outcome::Skipped(_)), "{again:?}");
        assert_eq!(store.stream(&format!("DeliveryJob-{}", job_id().0)), job_events);
    }

    /// Error guard shared by the partner/rider legs: an unknown dispatch run throws
    /// `errors.yaml#/DeliveryJobNotFound` (abort and surface, never a silent skip).
    #[tokio::test]
    async fn unknown_job_is_flagged_with_the_typed_error() {
        let store = MemStore::default();
        let state = MemDeliveryDispatchState::default();
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

        let err = on_delivery_status_updated(
            &store,
            &state,
            &DeliveryStatusUpdated {
                delivery_job_id: unknown,
                partner_ref: None,
                status: DeliveryStatus::DELIVERED,
                occurred_at: None,
                note: None,
            },
            &envelope(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code(), Some("DeliveryJobNotFound"), "{err:?}");

        let err = on_delivery_completed(
            &store,
            &state,
            &DeliveryCompleted { delivery_job_id: unknown, at: None },
            &envelope(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code(), Some("DeliveryJobNotFound"), "{err:?}");
    }

    /// tests.yaml#/TestDispatchClosesOrderOnPartnerDelivered —
    /// rules.yaml#/OrderClosedOnDeliveryCompletion: a partner-reported DELIVERED sends
    /// MarkOrderDelivered (→ OrderDelivered) and resolves the run COMPLETED; intermediate statuses
    /// skip.
    #[tokio::test]
    async fn partner_delivered_status_closes_the_order() {
        let store = MemStore::default();
        let state = MemDeliveryDispatchState::default();
        given_offered(&store, &state).await;

        let trigger = DeliveryStatusUpdated {
            delivery_job_id: job_id(),
            partner_ref: None,
            status: DeliveryStatus::DELIVERED,
            occurred_at: None,
            note: None,
        };
        let outcome =
            on_delivery_status_updated(&store, &state, &trigger, &envelope()).await.unwrap();
        assert_eq!(outcome, Outcome::Completed);
        let order_events = store.stream(&format!("Order-{}", order_id().0));
        assert!(order_events.iter().any(|e| matches!(
            e,
            DomainEvent::OrderDelivered(OrderDelivered { order_id: o, .. }) if *o == order_id()
        )));
        assert_eq!(domain::order::fold(&order_events).unwrap().status, OrderStatus::DELIVERED);
        let row = state.by_delivery_job(job_id()).await.unwrap().unwrap();
        assert_eq!(row.process_status, DeliveryDispatchProcessStatus::COMPLETED);

        // An intermediate status never closes the order.
        let store2 = MemStore::default();
        let state2 = MemDeliveryDispatchState::default();
        given_offered(&store2, &state2).await;
        let picked_up = DeliveryStatusUpdated { status: DeliveryStatus::PICKED_UP, ..trigger };
        let outcome =
            on_delivery_status_updated(&store2, &state2, &picked_up, &envelope()).await.unwrap();
        assert!(matches!(outcome, Outcome::Skipped(_)), "{outcome:?}");
        assert_eq!(
            domain::order::fold(&store2.stream(&format!("Order-{}", order_id().0))).unwrap().status,
            OrderStatus::READY
        );
    }

    /// tests.yaml#/TestDispatchClosesOrderOnRiderCompleted —
    /// rules.yaml#/OrderClosedOnDeliveryCompletion: an independent rider's completion closes the
    /// order once; the re-delivered completion is absorbed (the Order rejects, the leg skips).
    #[tokio::test]
    async fn rider_completion_closes_the_order_once() {
        let store = MemStore::default();
        let state = MemDeliveryDispatchState::default();
        given_offered(&store, &state).await;
        // The rider lifecycle facts fold onto the job stream (accepted + picked up).
        let mut job_events = store.stream(&format!("DeliveryJob-{}", job_id().0));
        job_events.push(DomainEvent::DeliveryAcceptedByRider(DeliveryAcceptedByRider {
            delivery_job_id: job_id(),
            rider_id: RiderId(uid(9)),
        }));
        job_events.push(DomainEvent::DeliveryPickedUp(DeliveryPickedUp {
            delivery_job_id: job_id(),
            rider_id: RiderId(uid(9)),
            at: None,
        }));
        store.seed(&format!("DeliveryJob-{}", job_id().0), job_events);

        let trigger = DeliveryCompleted { delivery_job_id: job_id(), at: None };
        let outcome = on_delivery_completed(&store, &state, &trigger, &envelope()).await.unwrap();
        assert_eq!(outcome, Outcome::Completed);
        let order_events = store.stream(&format!("Order-{}", order_id().0));
        assert_eq!(domain::order::fold(&order_events).unwrap().status, OrderStatus::DELIVERED);
        assert_eq!(
            state.by_delivery_job(job_id()).await.unwrap().unwrap().process_status,
            DeliveryDispatchProcessStatus::COMPLETED
        );

        // Re-delivery: the Order rejects the second MarkOrderDelivered (already DELIVERED) — the
        // leg logs and skips, and the stream gains nothing.
        let again = on_delivery_completed(&store, &state, &trigger, &envelope()).await.unwrap();
        assert!(matches!(again, Outcome::Skipped(ref m) if m.contains("rejected")), "{again:?}");
        assert_eq!(store.stream(&format!("Order-{}", order_id().0)), order_events);
    }
}
