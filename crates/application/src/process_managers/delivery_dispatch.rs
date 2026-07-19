//! DeliveryDispatchProcess (`specs/processmanager.yaml#/DeliveryDispatchProcess`, ADR-0031) —
//! dispatches and tracks deliveries over the `delivery_dispatch_process_manager` state row
//! (ADR-20260719-193500):
//!
//! - `OrderMarkedReady` → for a DELIVERY order (read from OrderTracking), deliver the
//!   `DeliveryRequested` birth to `DeliveryJob-<uuidv5(orderId)>` (the DETERMINISTIC id IS the
//!   idempotency key), offer the job through the [`DeliveryPartner`] port, row → OFFERED.
//! - `DeliveryAcceptedByPartner` / `DeliveryRejectedByPartner` (inbound, recorded by the DeliveryJob
//!   aggregate) → advance the run (ACCEPTED / REOFFER_REQUIRED + re-offer); an unknown run throws
//!   `errors.yaml#/DeliveryJobNotFound` (abort and surface, never a silent skip).
//! - `DeliveryStatusUpdated` (only DELIVERED) / `DeliveryCompleted` → SEND
//!   `commands.yaml#/MarkOrderDelivered` — the Order validates the transition, so a terminal
//!   cancelled/rejected order is never resurrected (a rejection is logged and skipped, per the DSL
//!   send semantics on an event leg); row → COMPLETED.

use domain::generated::commands::MarkOrderDelivered;
use domain::generated::entities::Address;
use domain::generated::events::{
    DeliveryAcceptedByPartner, DeliveryCompleted, DeliveryRejectedByPartner, DeliveryRequested,
    DeliveryStatusUpdated, DomainEvent, OrderMarkedReady,
};
use domain::generated::scalars::{
    DeliveryDispatchProcessStatus, DeliveryJobId, DeliveryStatus, OrderId, ServiceType,
};
use domain::shared::errors::DomainError;
use serde_json::json;

use crate::pm_state::{DeliveryDispatchRow, DeliveryDispatchStateStore};
use crate::ports::{is_version_conflict, DeliveryPartner, EventStore};
use crate::process_managers::{
    delivery_job_stream, saga_actor, Outcome, TriggerEnvelope,
};
use crate::queries::OrderReadRepository;
use crate::repository::Repository;

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
pub fn delivery_job_id_for(order_id: &OrderId) -> DeliveryJobId {
    DeliveryJobId(uuid::Uuid::new_v5(&dispatch_namespace(), order_id.0.as_bytes()))
}

/// The typed error every partner/rider leg throws when the job matches no dispatch run
/// (`errors.yaml#/DeliveryJobNotFound`).
fn job_not_found(delivery_job_id: &DeliveryJobId) -> DomainError {
    DomainError::rejected("DeliveryJobNotFound", json!({ "deliveryJobId": delivery_job_id }))
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

/// EVENT leg `events.yaml#/OrderMarkedReady` (rules.yaml#/ReadyDeliveryOrderTriggersDispatch): for a
/// DELIVERY order, birth the delivery job and offer it; COLLECTION orders need no dispatch (benign
/// skip).
pub async fn on_order_marked_ready(
    store: &dyn EventStore,
    state: &dyn DeliveryDispatchStateStore,
    orders: &dyn OrderReadRepository,
    partner: &dyn DeliveryPartner,
    event: &OrderMarkedReady,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    // read OrderTracking by order_id (service type + dropoff address).
    let Some(order) = orders.by_id(event.order_id).await? else {
        return Ok(Outcome::Skipped(format!(
            "order {} is not in the OrderTracking read model yet — cannot dispatch its delivery",
            event.order_id.0
        )));
    };
    // guard order.service_type = DELIVERY (skip: true — COLLECTION orders need no dispatch).
    if order.service_type != ServiceType::DELIVERY {
        return Ok(Outcome::Skipped(format!(
            "order {} is {:?} — no dispatch needed",
            event.order_id.0, order.service_type
        )));
    }
    let Some(dropoff) = order
        .delivery_address
        .clone()
        .and_then(|v| serde_json::from_value::<Address>(v).ok())
    else {
        return Ok(Outcome::Skipped(format!(
            "DELIVERY order {} carries no decodable delivery address — cannot dispatch",
            event.order_id.0
        )));
    };
    // read Restaurant (the pickup address) — folded from the aggregate's own stream, like the
    // restaurant command handlers (the projected row carries the same address as jsonb).
    let (restaurant_events, _) = store
        .load(&crate::process_managers::restaurant_stream(&event.restaurant_id))
        .await?;
    let Some(pickup) = restaurant_address(&restaurant_events) else {
        return Ok(Outcome::Skipped(format!(
            "restaurant {} has no address on its stream — cannot set the pickup for order {}",
            event.restaurant_id.0, event.order_id.0
        )));
    };
    // Test-mode lineage (ADR-0038): the job inherits the ORDER's mode, read from the authoritative
    // OrderPlaced fact (OrderTracking does not carry `mode`).
    let (order_events, _) =
        store.load(&crate::process_managers::order_stream(&event.order_id)).await?;
    let mode = order_events.iter().find_map(|e| match e {
        DomainEvent::OrderPlaced(p) => Some(p.mode),
        _ => None,
    });

    // deliver DeliveryRequested → DeliveryJob-<uuidv5(orderId)> (birth; idempotent re-dispatch:
    // the deterministic id targets the same stream, whose fold absorbs the replay).
    let job_id = delivery_job_id_for(&event.order_id);
    let actor = saga_actor(env);
    let (job_events, job_version) = store.load(&delivery_job_stream(&job_id)).await?;
    let requested = DeliveryRequested {
        mode: mode.flatten(),
        delivery_job_id: job_id,
        order_id: event.order_id,
        restaurant_id: event.restaurant_id,
        pickup,
        dropoff,
        // TODO(saga): partner pre-selection (e.g. Avelo37 vs independent riders) is a dispatch
        // policy the delivery-partner ACL will own; V0 leaves the job open to both channels.
        provider: None,
    };
    if domain::delivery_job::fold(&job_events).is_none() {
        Repository::new(store)
            .save(
                &delivery_job_stream(&job_id),
                job_version,
                &[DomainEvent::DeliveryRequested(requested.clone())],
                &actor,
            )
            .await?;
    }
    // call delivery_partner.offer_job (the no-op stand-in logs until the avelo37 ACL lands).
    partner.offer_job(&requested).await?;
    // state.set — the run is OFFERED.
    state
        .upsert(&DeliveryDispatchRow {
            order_id: event.order_id,
            restaurant_id: event.restaurant_id,
            delivery_job_id: job_id,
            process_status: DeliveryDispatchProcessStatus::OFFERED,
            last_update_utc: chrono::Utc::now(), // ignored on write; stamped by the store
        })
        .await?;
    Ok(Outcome::Completed)
}

/// EVENT leg `events.yaml#/DeliveryAcceptedByPartner` (rules.yaml#/PartnerAcceptanceRecordsCourier):
/// the fact (courier + ETAs) is recorded by the DeliveryJob aggregate — just advance the run.
pub async fn on_delivery_accepted_by_partner(
    state: &dyn DeliveryDispatchStateStore,
    event: &DeliveryAcceptedByPartner,
) -> Result<Outcome, DomainError> {
    let Some(row) = state.by_job(event.delivery_job_id).await? else {
        return Err(job_not_found(&event.delivery_job_id));
    };
    state
        .upsert(&DeliveryDispatchRow {
            process_status: DeliveryDispatchProcessStatus::ACCEPTED,
            ..row
        })
        .await?;
    Ok(Outcome::Completed)
}

/// EVENT leg `events.yaml#/DeliveryRejectedByPartner` (rules.yaml#/PartnerRejectionReoffers): flag
/// the run REOFFER_REQUIRED and re-offer through the port.
pub async fn on_delivery_rejected_by_partner(
    store: &dyn EventStore,
    state: &dyn DeliveryDispatchStateStore,
    partner: &dyn DeliveryPartner,
    event: &DeliveryRejectedByPartner,
) -> Result<Outcome, DomainError> {
    let Some(row) = state.by_job(event.delivery_job_id).await? else {
        return Err(job_not_found(&event.delivery_job_id));
    };
    state
        .upsert(&DeliveryDispatchRow {
            process_status: DeliveryDispatchProcessStatus::REOFFER_REQUIRED,
            ..row
        })
        .await?;
    // call delivery_partner.offer_job — TODO(saga): re-offer needs a partner-selection policy;
    // REOFFER_REQUIRED flags manual handling meanwhile. The job's birth fact carries the offer.
    let (job_events, _) = store.load(&delivery_job_stream(&event.delivery_job_id)).await?;
    if let Some(requested) = job_events.iter().find_map(|e| match e {
        DomainEvent::DeliveryRequested(r) => Some(r.clone()),
        _ => None,
    }) {
        partner.offer_job(&requested).await?;
    }
    Ok(Outcome::Completed)
}

/// Close the order for a completed delivery: SEND `MarkOrderDelivered` (the Order validates — a
/// rejection such as `InvalidOrderStatus` on a terminal order is logged and skipped, never
/// re-thrown), then resolve the run COMPLETED.
async fn close_order(
    store: &dyn EventStore,
    state: &dyn DeliveryDispatchStateStore,
    row: DeliveryDispatchRow,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    let cmd = MarkOrderDelivered { order_id: row.order_id, restaurant_id: row.restaurant_id };
    let send_outcome = match crate::commands::mark_order_delivered(store, cmd, &saga_actor(env)).await
    {
        Ok(()) => Outcome::Completed,
        // A lost optimistic-concurrency race is plumbing, not a rejection: retry the whole leg.
        Err(e) if is_version_conflict(&e) => return Err(e),
        Err(DomainError::Repository(e)) => return Err(DomainError::Repository(e)),
        Err(rejection) => {
            let reason = format!(
                "MarkOrderDelivered rejected for order {} (job {}): {rejection} — the Order's own \
                 invariants stand (a terminal order is never resurrected); run resolved COMPLETED",
                row.order_id.0, row.delivery_job_id.0
            );
            eprintln!("saga[DeliveryDispatchProcess]: {reason}");
            Outcome::Skipped(reason)
        }
    };
    state
        .upsert(&DeliveryDispatchRow {
            process_status: DeliveryDispatchProcessStatus::COMPLETED,
            ..row
        })
        .await?;
    Ok(send_outcome)
}

/// EVENT leg `events.yaml#/DeliveryStatusUpdated` (rules.yaml#/OrderClosedOnDeliveryCompletion):
/// only the terminal DELIVERED report closes the order — intermediate tracking folds onto the job
/// from the log (benign skip here).
pub async fn on_delivery_status_updated(
    store: &dyn EventStore,
    state: &dyn DeliveryDispatchStateStore,
    event: &DeliveryStatusUpdated,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    // guard message.status = DELIVERED (skip: true).
    if event.status != DeliveryStatus::DELIVERED {
        return Ok(Outcome::Skipped(format!(
            "job {} reported {:?} — intermediate tracking folds from the log, nothing to close",
            event.delivery_job_id.0, event.status
        )));
    }
    let Some(row) = state.by_job(event.delivery_job_id).await? else {
        return Err(job_not_found(&event.delivery_job_id));
    };
    close_order(store, state, row, env).await
}

/// EVENT leg `events.yaml#/DeliveryCompleted` (rules.yaml#/OrderClosedOnDeliveryCompletion): an
/// independent rider completed the delivery — same close path.
pub async fn on_delivery_completed(
    store: &dyn EventStore,
    state: &dyn DeliveryDispatchStateStore,
    event: &DeliveryCompleted,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    let Some(row) = state.by_job(event.delivery_job_id).await? else {
        return Err(job_not_found(&event.delivery_job_id));
    };
    close_order(store, state, row, env).await
}

// ================================================================================================
// Behaviour tests (specs/tests.yaml, DeliveryDispatchProcess saga) — each linked to its rules.yaml
// rule.
// ================================================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm_state::mem::MemDeliveryDispatchState;
    use crate::ports::NoopDeliveryPartner;
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
        on_order_marked_ready(store, state, &orders, &NoopDeliveryPartner, &ready(), &envelope())
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
            &NoopDeliveryPartner,
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
        let row = state.by_job(job_id()).await.unwrap().unwrap();
        assert_eq!(row.process_status, DeliveryDispatchProcessStatus::OFFERED);
        assert_eq!(row.order_id, order_id());
        assert_eq!(row.restaurant_id, restaurant_id());

        // Idempotent re-reaction: the deterministic job stream absorbs the replay.
        let again = on_order_marked_ready(
            &store,
            &state,
            &orders,
            &NoopDeliveryPartner,
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
            &NoopDeliveryPartner,
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
        let row = state.by_job(job_id()).await.unwrap().unwrap();
        assert_eq!(row.process_status, DeliveryDispatchProcessStatus::ACCEPTED);
    }

    /// tests.yaml#/TestDispatchPartnerRejected — rules.yaml#/PartnerRejectionReoffers: the run flags
    /// REOFFER_REQUIRED (re-offer policy is the delivery-partner ACL's TODO).
    #[tokio::test]
    async fn partner_rejection_flags_the_reoffer() {
        let store = MemStore::default();
        let state = MemDeliveryDispatchState::default();
        given_offered(&store, &state).await;

        let outcome = on_delivery_rejected_by_partner(
            &store,
            &state,
            &NoopDeliveryPartner,
            &DeliveryRejectedByPartner {
                delivery_job_id: job_id(),
                partner_ref: None,
                reason: Some("No courier available".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome, Outcome::Completed);
        let row = state.by_job(job_id()).await.unwrap().unwrap();
        assert_eq!(row.process_status, DeliveryDispatchProcessStatus::REOFFER_REQUIRED);
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
        let row = state.by_job(job_id()).await.unwrap().unwrap();
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
            state.by_job(job_id()).await.unwrap().unwrap().process_status,
            DeliveryDispatchProcessStatus::COMPLETED
        );

        // Re-delivery: the Order rejects the second MarkOrderDelivered (already DELIVERED) — the
        // leg logs and skips, and the stream gains nothing.
        let again = on_delivery_completed(&store, &state, &trigger, &envelope()).await.unwrap();
        assert!(matches!(again, Outcome::Skipped(ref m) if m.contains("rejected")), "{again:?}");
        assert_eq!(store.stream(&format!("Order-{}", order_id().0)), order_events);
    }
}
