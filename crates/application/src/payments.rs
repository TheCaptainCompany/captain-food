//! Recording INBOUND Stripe payment facts on the Payment aggregate (ADR-20260719-193500 §3): the
//! stateless Stripe ACL translates a webhook into `PaymentCaptured`/`PaymentFailed`/`PaymentRefunded`
//! and delivers it HERE — no `StripeEvent-%` envelope streams, no adapter idempotency table. Dedup is
//! the AGGREGATE's business decision: `domain::payment::already_records` answers "is this re-delivered
//! fact already reflected?", so a Stripe webhook retry appends nothing.
//!
//! A payment fact for a stream with NO `PaymentIntentCreated` birth is STILL recorded (facts are
//! never dropped — CLAUDE.md "Commands vs inbound events": there is nothing to reject); it is the
//! PlaceOrderProcess ORCHESTRATOR's `PaymentEventOrphaned` guard that flags the orphan for ops, not
//! this recording path.

use domain::generated::events::DomainEvent;
use domain::generated::scalars::PaymentIntentId;
use domain::shared::errors::DomainError;

use crate::ports::{Actor, EventStore};
use crate::repository::Repository;

/// What recording an inbound payment fact did: appended it, or found it already reflected in the
/// Payment's fold (idempotent webhook re-delivery).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordOutcome {
    Recorded,
    AlreadyRecorded,
}

/// The PaymentIntent a payment fact belongs to — the Payment aggregate's stream key. `None` for any
/// event outside the Payment inbox (a routing bug in the caller).
fn payment_intent_of(event: &DomainEvent) -> Option<PaymentIntentId> {
    match event {
        DomainEvent::PaymentCaptured(e) => Some(e.payment_intent_id.clone()),
        DomainEvent::PaymentFailed(e) => Some(e.payment_intent_id.clone()),
        DomainEvent::PaymentRefunded(e) => Some(e.payment_intent_id.clone()),
        _ => None,
    }
}

/// Record one inbound Stripe payment fact (`PaymentCaptured` | `PaymentFailed` | `PaymentRefunded`)
/// on its `Payment-<intentId>` stream, idempotently by the aggregate's own fold. The `actor` is the
/// ACL's system identity (EXTERNAL, correlation = the webhook's, ADR-0041).
pub async fn record_inbound_payment_event(
    store: &dyn EventStore,
    event: DomainEvent,
    actor: &Actor,
) -> Result<RecordOutcome, DomainError> {
    let Some(intent) = payment_intent_of(&event) else {
        return Err(DomainError::Repository(format!(
            "record_inbound_payment_event routed a non-payment event: {event:?}"
        )));
    };
    let stream = domain::payment::stream(&intent);
    let (events, version) = store.load(&stream).await?;
    if let Some(payment) = domain::payment::fold(&events) {
        if domain::payment::already_records(&payment, &event) {
            return Ok(RecordOutcome::AlreadyRecorded);
        }
    } else if events.iter().any(|e| e == &event) {
        // Birthless (orphan) stream: no fold to consult, so a webhook re-delivery dedups by
        // structural equality — the no-op guarantee holds even before the anomaly is resolved.
        return Ok(RecordOutcome::AlreadyRecorded);
    }
    // No birth on the stream? Record anyway — the fact happened; the orchestrator's
    // PaymentEventOrphaned guard is what surfaces the anomaly (never this recording path).
    Repository::new(store).save(&stream, version, &[event], actor).await?;
    Ok(RecordOutcome::Recorded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_managers::test_support::MemStore;
    use domain::generated::entities::{CheckoutSnapshot, CustomerContact, Money, PaymentBreakdown};
    use domain::generated::events::{PaymentCaptured, PaymentIntentCreated, PaymentRefunded};
    use domain::generated::scalars::*;

    fn uid(n: u128) -> uuid::Uuid {
        uuid::Uuid::from_u128(n)
    }
    fn eur(cents: i64) -> Money {
        Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
    }
    fn actor() -> Actor {
        Actor { user_id: uid(0xAC), user_type: 6, correlation_id: uid(0xC0), cause_id: None }
    }
    fn intent() -> PaymentIntentId {
        PaymentIntentId("pi_123".into())
    }
    fn birth() -> DomainEvent {
        let z = eur(0);
        DomainEvent::PaymentIntentCreated(PaymentIntentCreated {
            payment_intent_id: intent(),
            restaurant_id: RestaurantId(uid(3)),
            customer_id: None,
            amount: eur(1960),
            checkout: CheckoutSnapshot {
                order_id: OrderId(uid(1)),
                cart_id: CartId(uid(2)),
                restaurant_id: RestaurantId(uid(3)),
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
                    delivery: z.clone(),
                    service_fee: z.clone(),
                    total: eur(1960),
                    restaurant_contribution: z.clone(),
                    restaurant_payout: eur(1960),
                    rider_payout: z.clone(),
                    captain_net: z,
                },
                note: None,
            },
        })
    }
    fn captured() -> DomainEvent {
        DomainEvent::PaymentCaptured(PaymentCaptured {
            payment_intent_id: intent(),
            order_id: Some(OrderId(uid(1))),
            restaurant_id: RestaurantId(uid(3)),
            amount: eur(1960),
        })
    }

    /// rules.yaml#/OrderMaterializedOnPaymentCapture (recording half): the inbound fact lands on the
    /// Payment stream; a webhook re-delivery is absorbed by the aggregate's own fold.
    #[tokio::test]
    async fn records_once_and_absorbs_re_delivery() {
        let store = MemStore::default();
        let stream = domain::payment::stream(&intent());
        store.seed(&stream, vec![birth()]);

        assert_eq!(
            record_inbound_payment_event(&store, captured(), &actor()).await.unwrap(),
            RecordOutcome::Recorded
        );
        assert_eq!(
            record_inbound_payment_event(&store, captured(), &actor()).await.unwrap(),
            RecordOutcome::AlreadyRecorded
        );
        let events = store.stream(&stream);
        assert_eq!(events.len(), 2); // birth + ONE capture
        assert_eq!(
            domain::payment::fold(&events).unwrap().status,
            PaymentStatus::CAPTURED
        );

        // A DIFFERENT refund fact is a new fact, keyed by its Stripe refund id.
        let refund = |id: &str| {
            DomainEvent::PaymentRefunded(PaymentRefunded {
                refund_id: RefundId(id.into()),
                payment_intent_id: intent(),
                order_id: OrderId(uid(1)),
                restaurant_id: RestaurantId(uid(3)),
                amount: eur(1960),
                reason: None,
            })
        };
        assert_eq!(
            record_inbound_payment_event(&store, refund("re_1"), &actor()).await.unwrap(),
            RecordOutcome::Recorded
        );
        assert_eq!(
            record_inbound_payment_event(&store, refund("re_1"), &actor()).await.unwrap(),
            RecordOutcome::AlreadyRecorded
        );
    }

    /// Facts are never dropped: a payment fact for a stream with no `PaymentIntentCreated` birth is
    /// still recorded — the ORCHESTRATOR's PaymentEventOrphaned guard flags the orphan, not this path.
    #[tokio::test]
    async fn birthless_facts_are_still_recorded() {
        let store = MemStore::default();
        assert_eq!(
            record_inbound_payment_event(&store, captured(), &actor()).await.unwrap(),
            RecordOutcome::Recorded
        );
        let events = store.stream(&domain::payment::stream(&intent()));
        assert_eq!(events.len(), 1);
        assert_eq!(domain::payment::fold(&events), None); // no birth — folds to nothing, by design
    }

    /// A non-payment event routed here is a caller bug, not a droppable fact.
    #[tokio::test]
    async fn non_payment_events_are_refused() {
        let store = MemStore::default();
        let err = record_inbound_payment_event(
            &store,
            DomainEvent::OrderMarkedReady(domain::generated::events::OrderMarkedReady {
                order_id: OrderId(uid(1)),
                restaurant_id: RestaurantId(uid(3)),
            }),
            &actor(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DomainError::Repository(_)), "{err:?}");
    }
}
