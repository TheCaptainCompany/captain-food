//! BEHAVIOUR tests for the DeliveryJob aggregate (ADR-0031) — the executable form of the
//! `specs/tests.yaml` Given/When/Then cases whose `when` is a DeliveryJob command (ADR-0032: each test
//! cites the `specs/rules.yaml` rule it asserts). Given = pre-seeded stream events (in-memory event
//! store), When = the command handler, Then = the emitted event(s) / the errors.yaml rejection code.
//!
//! Pure and offline: an in-memory [`EventStore`]. `DeliveryRequested` in the GIVENs stands for the
//! DeliveryDispatchProcess outcome (that saga reacts to OrderMarkedReady and is a separate runtime leg).

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use application::commands::{
    accept_delivery, cancel_delivery, complete_delivery, confirm_pickup, rejection_code,
};
use application::ports::{version_conflict, Actor, EventStore};
use domain::generated::commands::{AcceptDelivery, CancelDelivery, CompleteDelivery, ConfirmPickup};
use domain::generated::entities::Address;
use domain::generated::events::{
    DeliveryAcceptedByRider, DeliveryCancelled, DeliveryCompleted, DeliveryPickedUp,
    DeliveryRequested, DomainEvent,
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

// ------------------------------------------------------------------------------------------------
// Fixtures (tests.yaml `fixtures`, with UUIDs instead of the sample string ids)
// ------------------------------------------------------------------------------------------------

fn actor() -> Actor {
    Actor {
        user_id: uuid::Uuid::new_v4(),
        user_type: 4, // UserType::RIDER ordinal
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
    }
}

fn stream(id: DeliveryJobId) -> String {
    format!("DeliveryJob-{}", id.0)
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

/// Fixture `deliveryRequested` — the job is born PENDING.
fn requested(id: DeliveryJobId) -> DomainEvent {
    DomainEvent::DeliveryRequested(DeliveryRequested {
        mode: None,
        delivery_job_id: id,
        order_id: OrderId(uuid::Uuid::new_v4()),
        restaurant_id: RestaurantId(uuid::Uuid::new_v4()),
        pickup: address("1 Rue Nationale"),
        dropoff: address("9 Rue Colbert"),
        provider: None,
    })
}

/// Fixture `deliveryAcceptedByRider`.
fn accepted(id: DeliveryJobId, rider: RiderId) -> DomainEvent {
    DomainEvent::DeliveryAcceptedByRider(DeliveryAcceptedByRider { delivery_job_id: id, rider_id: rider })
}

/// Fixture `deliveryPickedUp`.
fn picked_up(id: DeliveryJobId, rider: RiderId) -> DomainEvent {
    DomainEvent::DeliveryPickedUp(DeliveryPickedUp { delivery_job_id: id, rider_id: rider, at: None })
}

/// Fixture `deliveryCompleted`.
fn completed(id: DeliveryJobId) -> DomainEvent {
    DomainEvent::DeliveryCompleted(DeliveryCompleted { delivery_job_id: id, at: None })
}

/// Fixture `deliveryCancelled`.
fn cancelled(id: DeliveryJobId) -> DomainEvent {
    DomainEvent::DeliveryCancelled(DeliveryCancelled { delivery_job_id: id, reason: None })
}

fn jid() -> DeliveryJobId {
    DeliveryJobId(uuid::Uuid::new_v4())
}
fn rider() -> RiderId {
    RiderId(uuid::Uuid::new_v4())
}

// ------------------------------------------------------------------------------------------------
// Acceptance (rules.yaml#/DeliveryAcceptedOnlyWhenPending)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestAcceptDelivery — rules.yaml#/DeliveryAcceptedOnlyWhenPending
#[tokio::test]
async fn an_independent_rider_accepts_a_pending_job() {
    let store = MemStore::default();
    let (job, r) = (jid(), rider());
    store.seed(&stream(job), vec![requested(job)]);

    accept_delivery(&store, AcceptDelivery { delivery_job_id: job, rider_id: r }, &actor())
        .await
        .expect("accept");

    let events = store.stream(&stream(job));
    assert!(matches!(&events[1], DomainEvent::DeliveryAcceptedByRider(e) if e.rider_id == r));
}

/// tests.yaml#/cases/TestAcceptDeliveryIsRejected (all three arms) —
/// rules.yaml#/DeliveryAcceptedOnlyWhenPending
#[tokio::test]
async fn rejects_accepting_a_missing_taken_or_cancelled_job() {
    let store = MemStore::default();

    // Missing job → DeliveryJobNotFound.
    let err = accept_delivery(&store, AcceptDelivery { delivery_job_id: jid(), rider_id: rider() }, &actor())
        .await
        .expect_err("missing");
    assert_eq!(rejection_code(&err), Some("DeliveryJobNotFound"));

    // Already taken by rider-1 → DeliveryAlreadyAssigned for rider-2.
    let (job, r1) = (jid(), rider());
    store.seed(&stream(job), vec![requested(job), accepted(job, r1)]);
    let err = accept_delivery(&store, AcceptDelivery { delivery_job_id: job, rider_id: rider() }, &actor())
        .await
        .expect_err("already taken");
    assert_eq!(rejection_code(&err), Some("DeliveryAlreadyAssigned"));
    assert_eq!(store.stream(&stream(job)).len(), 2, "no event on rejection");

    // Cancelled job → InvalidDeliveryStatus (only a PENDING job can be accepted).
    let job = jid();
    store.seed(&stream(job), vec![requested(job), cancelled(job)]);
    let err = accept_delivery(&store, AcceptDelivery { delivery_job_id: job, rider_id: rider() }, &actor())
        .await
        .expect_err("cancelled");
    assert_eq!(rejection_code(&err), Some("InvalidDeliveryStatus"));
}

// ------------------------------------------------------------------------------------------------
// Pickup & completion (rules.yaml#/DeliveryPickupAndCompletionByRider)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestConfirmPickup — rules.yaml#/DeliveryPickupAndCompletionByRider
#[tokio::test]
async fn the_assigned_rider_confirms_pickup() {
    let store = MemStore::default();
    let (job, r) = (jid(), rider());
    store.seed(&stream(job), vec![requested(job), accepted(job, r)]);

    confirm_pickup(&store, ConfirmPickup { delivery_job_id: job, rider_id: r }, &actor())
        .await
        .expect("pickup");
    assert!(matches!(&store.stream(&stream(job))[2], DomainEvent::DeliveryPickedUp(e) if e.rider_id == r));

    // Another rider cannot confirm the pickup (must be ASSIGNED to this rider).
    let (job2, r2) = (jid(), rider());
    store.seed(&stream(job2), vec![requested(job2), accepted(job2, r2)]);
    let err = confirm_pickup(&store, ConfirmPickup { delivery_job_id: job2, rider_id: rider() }, &actor())
        .await
        .expect_err("wrong rider");
    assert_eq!(rejection_code(&err), Some("InvalidDeliveryStatus"));

    // A PENDING (unassigned) job cannot be picked up.
    let job3 = jid();
    store.seed(&stream(job3), vec![requested(job3)]);
    let err = confirm_pickup(&store, ConfirmPickup { delivery_job_id: job3, rider_id: rider() }, &actor())
        .await
        .expect_err("not assigned");
    assert_eq!(rejection_code(&err), Some("InvalidDeliveryStatus"));
}

/// tests.yaml#/cases/TestCompleteDelivery — rules.yaml#/DeliveryPickupAndCompletionByRider
#[tokio::test]
async fn the_assigned_rider_records_hand_over() {
    let store = MemStore::default();
    let (job, r) = (jid(), rider());
    store.seed(&stream(job), vec![requested(job), accepted(job, r), picked_up(job, r)]);

    complete_delivery(&store, CompleteDelivery { delivery_job_id: job, rider_id: r }, &actor())
        .await
        .expect("complete");
    assert!(matches!(&store.stream(&stream(job))[3], DomainEvent::DeliveryCompleted(_)));

    // Completion before pickup is out of order (InvalidDeliveryStatus).
    let (job2, r2) = (jid(), rider());
    store.seed(&stream(job2), vec![requested(job2), accepted(job2, r2)]);
    let err = complete_delivery(&store, CompleteDelivery { delivery_job_id: job2, rider_id: r2 }, &actor())
        .await
        .expect_err("not picked up");
    assert_eq!(rejection_code(&err), Some("InvalidDeliveryStatus"));
}

// ------------------------------------------------------------------------------------------------
// Cancellation (rules.yaml#/DeliveryCancellableBeforeCompletion)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestCancelDelivery — rules.yaml#/DeliveryCancellableBeforeCompletion
#[tokio::test]
async fn the_restaurant_cancels_a_pending_job() {
    let store = MemStore::default();
    let job = jid();
    store.seed(&stream(job), vec![requested(job)]);

    cancel_delivery(
        &store,
        CancelDelivery { delivery_job_id: job, reason: Some("Restaurant closed".into()) },
        &actor(),
    )
    .await
    .expect("cancel");
    assert!(matches!(
        &store.stream(&stream(job))[1],
        DomainEvent::DeliveryCancelled(e) if e.reason.as_deref() == Some("Restaurant closed")
    ));

    // Re-cancelling an already-cancelled job is an idempotent no-op (the command ensures the state).
    cancel_delivery(&store, CancelDelivery { delivery_job_id: job, reason: None }, &actor())
        .await
        .expect("idempotent");
    assert_eq!(store.stream(&stream(job)).len(), 2, "no event emitted");
}

/// tests.yaml#/cases/TestCancelDeliveryIsRejected (both arms) —
/// rules.yaml#/DeliveryCancellableBeforeCompletion
#[tokio::test]
async fn rejects_cancelling_a_missing_or_delivered_job() {
    let store = MemStore::default();

    // Missing job → DeliveryJobNotFound.
    let err = cancel_delivery(&store, CancelDelivery { delivery_job_id: jid(), reason: None }, &actor())
        .await
        .expect_err("missing");
    assert_eq!(rejection_code(&err), Some("DeliveryJobNotFound"));

    // Delivered job → InvalidDeliveryStatus.
    let (job, r) = (jid(), rider());
    store.seed(&stream(job), vec![requested(job), accepted(job, r), picked_up(job, r), completed(job)]);
    let err = cancel_delivery(
        &store,
        CancelDelivery { delivery_job_id: job, reason: Some("Too late".into()) },
        &actor(),
    )
    .await
    .expect_err("delivered");
    assert_eq!(rejection_code(&err), Some("InvalidDeliveryStatus"));
    assert_eq!(store.stream(&stream(job)).len(), 4, "no event on rejection");
}
