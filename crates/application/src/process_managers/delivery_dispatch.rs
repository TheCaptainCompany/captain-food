//! DeliveryDispatchProcess (actors.yaml#/DeliveryDispatchProcess, ADR-0031) — dispatches and tracks
//! deliveries (bounded context: delivery):
//!
//! - `OrderMarkedReady` → for a DELIVERY order, birth the delivery job (`DeliveryRequested` on
//!   `DeliveryJob-<id>`, PENDING) and offer it; no-op for COLLECTION.
//! - `DeliveryAcceptedByPartner` → nothing to append: the inbound fact is already on the job stream
//!   (courier + ETAs fold onto the job).
//! - `DeliveryRejectedByPartner` → re-offer / manual handling — external effect, `TODO(saga)`.
//! - `DeliveryStatusUpdated` (status DELIVERED) / `DeliveryCompleted` → close the order
//!   (`OrderDelivered` on `Order-<id>`).
//!
//! The delivery-job id is a DETERMINISTIC UUIDv5 of the order id, so a re-reaction to the same
//! `OrderMarkedReady` targets the same stream and folds to a no-op (idempotency, see the module doc in
//! `process_managers`).

use domain::generated::entities::Address;
use domain::generated::events::{
    DeliveryAcceptedByPartner, DeliveryCompleted, DeliveryRejectedByPartner, DeliveryRequested,
    DeliveryStatusUpdated, DomainEvent, OrderDelivered, OrderMarkedReady,
};
use domain::generated::scalars::{DeliveryJobId, DeliveryStatus, OrderId, OrderStatus, RestaurantId};

use crate::process_managers::{delivery_job_stream, order_stream, Decision, StreamAppend};

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

/// The order + restaurant a delivery job was requested for — scanned from the job stream's birth event
/// (the write-side `DeliveryJobState` fold deliberately does not keep them). The runner also uses this
/// to know WHICH order stream to load before the close-order decisions.
pub fn requested_order(job_events: &[DomainEvent]) -> Option<(OrderId, RestaurantId)> {
    job_events.iter().find_map(|e| match e {
        DomainEvent::DeliveryRequested(r) => Some((r.order_id, r.restaurant_id)),
        _ => None,
    })
}

/// The restaurant's current address — the job's pickup point. Registration carries it (required);
/// a later `RestaurantUpdated` may replace it.
fn restaurant_address(restaurant_events: &[DomainEvent]) -> Option<Address> {
    restaurant_events.iter().fold(None, |addr, e| match e {
        DomainEvent::RestaurantRegistered(r) => Some(r.address.clone()),
        DomainEvent::RestaurantUpdated(u) => u.address.clone().or(addr),
        _ => addr,
    })
}

/// React to `OrderMarkedReady` (rules.yaml#/ReadyDeliveryOrderTriggersDispatch): for a DELIVERY order,
/// birth the delivery job and offer it (partner dispatch via the avelo37 ACL and/or the independent
/// rider pool — the OFFER itself is carried by the `DeliveryRequested` fact those channels consume).
/// No-op for COLLECTION.
///
/// - `order_events` — the `Order-<orderId>` stream (service type + dropoff address).
/// - `restaurant_events` — the `Restaurant-<restaurantId>` stream (pickup address).
/// - `job_events` — the `DeliveryJob-<derived id>` stream (idempotent re-reaction guard).
pub fn on_order_marked_ready(
    event: &OrderMarkedReady,
    order_events: &[DomainEvent],
    restaurant_events: &[DomainEvent],
    job_events: &[DomainEvent],
) -> Decision {
    // Idempotent re-reaction: the job (deterministic id) was already requested.
    if domain::delivery_job::fold(job_events).is_some() {
        return Decision::Nothing;
    }
    let Some(placed) = order_events.iter().find_map(|e| match e {
        DomainEvent::OrderPlaced(p) => Some(p),
        _ => None,
    }) else {
        return Decision::Skip(format!(
            "order {} has no OrderPlaced on its stream — cannot dispatch its delivery",
            event.order_id.0
        ));
    };
    if placed.service_type != domain::generated::scalars::ServiceType::DELIVERY {
        return Decision::Nothing; // COLLECTION: the customer picks up — no job (actors.yaml effect).
    }
    let Some(dropoff) = placed.delivery_address.clone() else {
        return Decision::Skip(format!(
            "DELIVERY order {} carries no delivery address — cannot dispatch",
            event.order_id.0
        ));
    };
    let Some(pickup) = restaurant_address(restaurant_events) else {
        return Decision::Skip(format!(
            "restaurant {} has no address on its stream — cannot set the pickup for order {}",
            event.restaurant_id.0, event.order_id.0
        ));
    };
    let job_id = delivery_job_id_for(&event.order_id);
    Decision::Act(vec![StreamAppend {
        stream_name: delivery_job_stream(&job_id),
        expected_version: 0, // the reaction births the job stream
        events: vec![DomainEvent::DeliveryRequested(DeliveryRequested {
            mode: placed.mode,
            delivery_job_id: job_id,
            order_id: event.order_id,
            restaurant_id: event.restaurant_id,
            pickup,
            dropoff,
            // TODO(saga): partner pre-selection (e.g. Avelo37 vs independent riders) is a dispatch
            // policy the delivery-partner ACL will own; V0 leaves the job open to both channels.
            provider: None,
        })],
    }])
}

/// React to `DeliveryAcceptedByPartner` (rules.yaml#/PartnerAcceptanceRecordsCourier): the inbound
/// fact — courier + ETAs — is already recorded on the job stream and folds onto the job (write-side
/// `delivery_job::fold` marks it ASSIGNED); nothing further to append (`emits: []`).
pub fn on_delivery_accepted_by_partner(_event: &DeliveryAcceptedByPartner) -> Decision {
    Decision::Nothing
}

/// React to `DeliveryRejectedByPartner` (rules.yaml#/PartnerRejectionReoffers): re-offer to another
/// partner / the independent rider pool or flag for manual handling — an external dispatch effect the
/// delivery-partner ACL will own (`emits: []`).
pub fn on_delivery_rejected_by_partner(event: &DeliveryRejectedByPartner) -> Decision {
    Decision::Skip(format!(
        "TODO(saga): re-offer delivery job {} after the partner declined ({}) — needs the \
         delivery-partner ACL / rider offer channel (integration workstream); the job stays PENDING",
        event.delivery_job_id.0,
        event.reason.as_deref().unwrap_or("no reason reported")
    ))
}

/// React to `DeliveryStatusUpdated` (inbound partner tracking): the status itself folds onto the job
/// from the log; when it reaches DELIVERED, close the order
/// (rules.yaml#/OrderClosedOnDeliveryCompletion).
pub fn on_delivery_status_updated(
    event: &DeliveryStatusUpdated,
    job_events: &[DomainEvent],
    order_events: &[DomainEvent],
    order_version: i64,
) -> Decision {
    if event.status != DeliveryStatus::DELIVERED {
        return Decision::Nothing; // intermediate tracking folds from the log; nothing to emit
    }
    close_order(&event.delivery_job_id, job_events, order_events, order_version)
}

/// React to `DeliveryCompleted` (independent rider hand-over): close the order
/// (rules.yaml#/OrderClosedOnDeliveryCompletion).
pub fn on_delivery_completed(
    event: &DeliveryCompleted,
    job_events: &[DomainEvent],
    order_events: &[DomainEvent],
    order_version: i64,
) -> Decision {
    close_order(&event.delivery_job_id, job_events, order_events, order_version)
}

/// Emit `OrderDelivered` on the job's order, unless the order is already closed (idempotent) or in a
/// terminal cancel/reject state (a late delivery report must not resurrect it).
fn close_order(
    job_id: &DeliveryJobId,
    job_events: &[DomainEvent],
    order_events: &[DomainEvent],
    order_version: i64,
) -> Decision {
    let Some((order_id, restaurant_id)) = requested_order(job_events) else {
        return Decision::Skip(format!(
            "delivery job {} has no DeliveryRequested on its stream — cannot resolve its order",
            job_id.0
        ));
    };
    match domain::order::fold(order_events) {
        None => Decision::Skip(format!(
            "order {} (job {}) has no OrderPlaced on its stream — cannot close it",
            order_id.0, job_id.0
        )),
        Some(state) if state.status == OrderStatus::DELIVERED => Decision::Nothing, // already closed
        Some(state) if state.is_terminated() => Decision::Skip(format!(
            "order {} is terminal ({:?}) — a delivery completion for job {} does not resurrect it",
            order_id.0, state.status, job_id.0
        )),
        Some(_) => Decision::Act(vec![StreamAppend {
            stream_name: order_stream(&order_id),
            expected_version: order_version,
            events: vec![DomainEvent::OrderDelivered(OrderDelivered { order_id, restaurant_id })],
        }]),
    }
}

// ================================================================================================
// Behaviour tests (specs/tests.yaml, DeliveryDispatchProcess saga) — each linked to its rules.yaml
// rule.
// ================================================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use domain::generated::entities::{
        Address, Courier, CustomerContact, Money, PaymentBreakdown,
    };
    use domain::generated::events::{
        DeliveryAcceptedByRider, DeliveryPickedUp, OrderMarkedReady, OrderPlaced,
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
    fn placed(service_type: ServiceType) -> Vec<DomainEvent> {
        vec![DomainEvent::OrderPlaced(OrderPlaced {
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
        })]
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
    fn requested_job() -> Vec<DomainEvent> {
        let job_id = delivery_job_id_for(&order_id());
        vec![DomainEvent::DeliveryRequested(DeliveryRequested {
            mode: None,
            delivery_job_id: job_id,
            order_id: order_id(),
            restaurant_id: restaurant_id(),
            pickup: address("1 Rue Nationale"),
            dropoff: address("9 Rue Colbert"),
            provider: None,
        })]
    }
    /// Order stream state after the restaurant marked it ready (PLACED → … → READY).
    fn ready_order_events() -> Vec<DomainEvent> {
        let mut events = placed(ServiceType::DELIVERY);
        events.push(DomainEvent::OrderMarkedReady(ready()));
        events
    }

    /// tests.yaml#/TestDispatchOnOrderReady — rules.yaml#/ReadyDeliveryOrderTriggersDispatch:
    /// a ready DELIVERY order births the delivery job (DeliveryRequested, PENDING).
    #[test]
    fn ready_delivery_order_requests_a_delivery_job() {
        let d = on_order_marked_ready(&ready(), &ready_order_events(), &registered_restaurant(), &[]);
        let appends = d.appends();
        assert_eq!(appends.len(), 1, "{d:?}");
        assert_eq!(appends[0].expected_version, 0);
        let DomainEvent::DeliveryRequested(req) = &appends[0].events[0] else {
            panic!("expected DeliveryRequested, got {:?}", appends[0].events[0]);
        };
        assert_eq!(req.order_id, order_id());
        assert_eq!(req.delivery_job_id, delivery_job_id_for(&order_id())); // deterministic id
        assert_eq!(req.pickup, address("1 Rue Nationale")); // restaurant address
        assert_eq!(req.dropoff, address("9 Rue Colbert")); // order delivery address
                                                           // Born PENDING per the write-side fold.
        assert_eq!(
            domain::delivery_job::fold(&appends[0].events).unwrap().status,
            DeliveryStatus::PENDING
        );
    }

    /// rules.yaml#/ReadyDeliveryOrderTriggersDispatch (no-op corollaries): COLLECTION orders are not
    /// dispatched, and a re-reaction onto the already-requested job emits nothing.
    #[test]
    fn collection_orders_and_re_reactions_do_not_dispatch() {
        let mut collection = placed(ServiceType::COLLECTION);
        collection.push(DomainEvent::OrderMarkedReady(ready()));
        assert_eq!(
            on_order_marked_ready(&ready(), &collection, &registered_restaurant(), &[]),
            Decision::Nothing
        );
        assert_eq!(
            on_order_marked_ready(
                &ready(),
                &ready_order_events(),
                &registered_restaurant(),
                &requested_job()
            ),
            Decision::Nothing
        );
    }

    /// tests.yaml#/TestDispatchPartnerAccepted — rules.yaml#/PartnerAcceptanceRecordsCourier: the
    /// inbound acceptance is already on the job stream; the saga appends nothing further.
    #[test]
    fn partner_acceptance_needs_no_further_reaction() {
        let d = on_delivery_accepted_by_partner(&DeliveryAcceptedByPartner {
            delivery_job_id: delivery_job_id_for(&order_id()),
            partner_ref: ExternalReference("avelo-77".into()),
            courier: Courier {
                display_name: "Léa".into(),
                phone: Some(PhoneNumber("+33611223344".into())),
                rider_id: None,
            },
            estimated_pickup_at: None,
            estimated_dropoff_at: None,
        });
        assert_eq!(d, Decision::Nothing);
    }

    /// tests.yaml#/TestDispatchPartnerRejected — rules.yaml#/PartnerRejectionReoffers: the re-offer is
    /// a pending external effect (no domain event emitted).
    #[test]
    fn partner_rejection_flags_the_reoffer_and_emits_nothing() {
        let d = on_delivery_rejected_by_partner(&DeliveryRejectedByPartner {
            delivery_job_id: delivery_job_id_for(&order_id()),
            partner_ref: None,
            reason: Some("No courier available".into()),
        });
        assert!(d.appends().is_empty());
        assert!(matches!(d, Decision::Skip(ref m) if m.contains("re-offer")), "{d:?}");
    }

    /// tests.yaml#/TestDispatchClosesOrderOnPartnerDelivered —
    /// rules.yaml#/OrderClosedOnDeliveryCompletion: a partner-reported DELIVERED closes the order.
    #[test]
    fn partner_delivered_status_closes_the_order() {
        let trigger = DeliveryStatusUpdated {
            delivery_job_id: delivery_job_id_for(&order_id()),
            partner_ref: None,
            status: DeliveryStatus::DELIVERED,
            occurred_at: None,
            note: None,
        };
        let d = on_delivery_status_updated(&trigger, &requested_job(), &ready_order_events(), 2);
        assert_eq!(
            d.appends(),
            &[StreamAppend {
                stream_name: format!("Order-{}", order_id().0),
                expected_version: 2,
                events: vec![DomainEvent::OrderDelivered(OrderDelivered {
                    order_id: order_id(),
                    restaurant_id: restaurant_id(),
                })],
            }]
        );
        // A non-terminal partner status (e.g. PICKED_UP) folds from the log — nothing to emit.
        let picked_up = DeliveryStatusUpdated { status: DeliveryStatus::PICKED_UP, ..trigger };
        assert_eq!(
            on_delivery_status_updated(&picked_up, &requested_job(), &ready_order_events(), 2),
            Decision::Nothing
        );
    }

    /// tests.yaml#/TestDispatchClosesOrderOnRiderCompleted —
    /// rules.yaml#/OrderClosedOnDeliveryCompletion: an independent rider's completion closes the
    /// order; re-reacting once the order is DELIVERED emits nothing (idempotent).
    #[test]
    fn rider_completion_closes_the_order_once() {
        let mut job_events = requested_job();
        job_events.push(DomainEvent::DeliveryAcceptedByRider(DeliveryAcceptedByRider {
            delivery_job_id: delivery_job_id_for(&order_id()),
            rider_id: RiderId(uid(9)),
        }));
        job_events.push(DomainEvent::DeliveryPickedUp(DeliveryPickedUp {
            delivery_job_id: delivery_job_id_for(&order_id()),
            rider_id: RiderId(uid(9)),
            at: None,
        }));
        let trigger = DeliveryCompleted {
            delivery_job_id: delivery_job_id_for(&order_id()),
            at: None,
        };
        let d = on_delivery_completed(&trigger, &job_events, &ready_order_events(), 2);
        let appends = d.appends();
        assert_eq!(appends.len(), 1, "{d:?}");
        assert!(matches!(appends[0].events[0], DomainEvent::OrderDelivered(_)));

        // Idempotent re-reaction: fold the emitted close back into the order stream, react again.
        let mut closed_order = ready_order_events();
        closed_order.extend(appends[0].events.iter().cloned());
        assert_eq!(
            on_delivery_completed(&trigger, &job_events, &closed_order, 3),
            Decision::Nothing
        );
    }
}
