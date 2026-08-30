//! Recording INBOUND Stripe payment facts on the Payment aggregate (ADR-20260719-193500 §3): the
//! stateless Stripe ACL translates a webhook into `PaymentAuthorized`/`PaymentCaptured`/
//! `PaymentReleased`/`PaymentFailed`/`PaymentRefunded` and delivers it HERE — no `StripeEvent-%`
//! envelope streams, no adapter idempotency table. Dedup is
//! the AGGREGATE's business decision: `domain::payment::already_records` answers "is this re-delivered
//! fact already reflected?", so a Stripe webhook retry appends nothing.
//!
//! A payment fact for a stream with NO `PaymentIntentCreated` birth is STILL recorded (facts are
//! never dropped — CLAUDE.md "Commands vs inbound events": there is nothing to reject); it is the
//! PlaceOrderProcess ORCHESTRATOR's `PaymentEventOrphaned` guard that flags the orphan for ops, not
//! this recording path.

use crate::generated::inboxes::PaymentFactInbox;
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
    /// The aggregate decided the inbound fact changes NOTHING, so no event was appended
    /// (ADR-20260728-011344 D6). A legitimate outcome, not a failure: an external system re-reported a
    /// record we already hold, identically. Distinct from [`Self::AlreadyRecorded`], where we have seen
    /// this very fact before — here the fact is new but semantically inert. The drain persists the
    /// difference as IGNORED vs DUPLICATE, because they have different causes and different fixes.
    NoChange,
    /// The aggregate decided a DIFFERENT fact from the one reported — an inbound registration for a
    /// restaurant we already hold becomes `RestaurantUpdated`. Appended, so it delivers.
    Updated,
}

/// The adjacently-tagged NAME of an event, and NOTHING else from it.
///
/// **PAYLOAD-FREE ON PURPOSE** (#623 leak class, PR #783 review N-B1-secondary). The refusal below
/// rides `DomainError::Repository` -> `sqlx::Error::Protocol` -> the worker's
/// `"context": {"detail": err}`, which lands in the 90-day durable `inbound_messages.error` column.
/// A `{event:?}` there wrote a full money fact — amounts, order and restaurant ids — into an
/// operational column, which is exactly what this very PR tightens for the sirene path. The name
/// alone is what an operator needs to act; the payload is already in the row's own `payload`.
/// Same idiom as the ACLs' `event_type_of`.
fn event_name_of(event: &DomainEvent) -> String {
    serde_json::to_value(event)
        .ok()
        .and_then(|v| v.get("eventType").and_then(|t| t.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_string())
}

/// Narrow an untyped [`DomainEvent`] onto the Payment lane's declared fact set, or hand it back.
///
/// **THE UNTYPED DOOR'S ONE NARROWING STEP** (PR #783 review B1). Its predecessor was
/// `payment_intent_of`, a `&DomainEvent -> Option<PaymentIntentId>` ending in `_ => None` that
/// covered FIVE of the ten facts the `Payment` actor declares it receives: `PaymentCaptureFailed`,
/// `PaymentIntentCreated`, `RefundApproved`, `RefundDenied` and `RefundOpened` all fell into the
/// wildcard and were REFUSED with a `Repository` error rather than recorded. Narrowing to
/// [`PaymentFactInbox`] first means the stream lookup that follows is [`intent_of_fact`] — total
/// over the lane by construction — so there is exactly ONE stream lookup on the money path and an
/// eleventh declared Payment fact is an E0004 in it.
///
/// The `other => Err(other)` arm is a REFUSAL, never a route: it cannot mis-file an event, it can
/// only decline one. The mailbox money path does not reach it at all — `fact_route` hands
/// [`record_inbound_payment_fact`] a typed lane value, so the only callers left here are the ones
/// holding an untyped event (the generated behaviour suite and the orphan-metric harness).
fn payment_fact_of(event: DomainEvent) -> Result<PaymentFactInbox, DomainEvent> {
    match event {
        DomainEvent::PaymentAuthorized(e) => Ok(PaymentFactInbox::PaymentAuthorized(e)),
        DomainEvent::PaymentCaptureFailed(e) => Ok(PaymentFactInbox::PaymentCaptureFailed(e)),
        DomainEvent::PaymentCaptured(e) => Ok(PaymentFactInbox::PaymentCaptured(e)),
        DomainEvent::PaymentFailed(e) => Ok(PaymentFactInbox::PaymentFailed(e)),
        DomainEvent::PaymentIntentCreated(e) => Ok(PaymentFactInbox::PaymentIntentCreated(e)),
        DomainEvent::PaymentRefunded(e) => Ok(PaymentFactInbox::PaymentRefunded(e)),
        DomainEvent::PaymentReleased(e) => Ok(PaymentFactInbox::PaymentReleased(e)),
        DomainEvent::RefundApproved(e) => Ok(PaymentFactInbox::RefundApproved(e)),
        DomainEvent::RefundDenied(e) => Ok(PaymentFactInbox::RefundDenied(e)),
        DomainEvent::RefundOpened(e) => Ok(PaymentFactInbox::RefundOpened(e)),
        other => Err(other),
    }
}

/// Record one inbound payment fact carried as an untyped [`DomainEvent`] on its
/// `Payment-<intentId>` stream, idempotently by the aggregate's own fold. The `actor` is the ACL's
/// system identity (EXTERNAL, correlation = the webhook's, ADR-0041).
///
/// A THIN ADAPTER over [`record_inbound_payment_fact`] and deliberately not a second recorder: it
/// narrows to the typed lane value and delegates, so both doors share one stream lookup and one
/// idempotency rule. All TEN declared facts are accepted here, not the five the wildcard used to
/// leave standing.
pub async fn record_inbound_payment_event(
    store: &dyn EventStore,
    event: DomainEvent,
    actor: &Actor,
) -> Result<RecordOutcome, DomainError> {
    match payment_fact_of(event) {
        Ok(fact) => record_inbound_payment_fact(store, fact, actor).await,
        Err(other) => Err(DomainError::Repository(format!(
            "record_inbound_payment_event routed a non-payment event: {}",
            event_name_of(&other)
        ))),
    }
}

/// The intent a Payment-lane fact belongs to — **TOTAL over the lane's declared fact set** (#780),
/// and **THE ONLY STREAM LOOKUP ON THE MONEY PATH** (PR #783 review B1).
///
/// Both doors reach it. `fact_route` hands [`record_inbound_payment_fact`] a typed
/// [`PaymentFactInbox`] straight off the mailbox row, and the untyped
/// [`record_inbound_payment_event`] narrows onto the same type before delegating — so no Payment
/// fact resolves its stream through a wildcard any more. The predecessor did: a
/// `&DomainEvent -> Option<PaymentIntentId>` must end in `_ => None`, five of the ten declared
/// facts fell into it, and each was refused with a `Repository` error rather than recorded.
///
/// A new `receives:` FACT on the Payment lane is an E0004 HERE, at the place that knows how to find
/// its stream, instead of a runtime surprise on the money path — and the claim is load-bearing only
/// because this function is on the path the mailbox actually runs.
pub fn intent_of_fact(fact: &PaymentFactInbox) -> PaymentIntentId {
    match fact {
        PaymentFactInbox::PaymentAuthorized(e) => e.payment_intent_id.clone(),
        PaymentFactInbox::PaymentCaptureFailed(e) => e.payment_intent_id.clone(),
        PaymentFactInbox::PaymentCaptured(e) => e.payment_intent_id.clone(),
        PaymentFactInbox::PaymentFailed(e) => e.payment_intent_id.clone(),
        PaymentFactInbox::PaymentIntentCreated(e) => e.payment_intent_id.clone(),
        PaymentFactInbox::PaymentRefunded(e) => e.payment_intent_id.clone(),
        PaymentFactInbox::PaymentReleased(e) => e.payment_intent_id.clone(),
        PaymentFactInbox::RefundApproved(e) => e.payment_intent_id.clone(),
        PaymentFactInbox::RefundDenied(e) => e.payment_intent_id.clone(),
        PaymentFactInbox::RefundOpened(e) => e.payment_intent_id.clone(),
    }
}

/// Record one TYPED Payment-lane fact on its `Payment-<intentId>` stream.
///
/// **THE MAILBOX MONEY PATH'S RECORDER**: `inbox::fact_route` returns `FactLeg::Record(
/// RecordLeg::Payment(..))` carrying this lane's typed fact, and `mailbox::handler` calls exactly
/// this. The untyped [`record_inbound_payment_event`] narrows onto the same type and delegates here
/// — NOT a second recorder: both resolve to
/// [`record_on_payment_stream`], so there is exactly ONE idempotency rule on the money path
/// (`domain::payment::already_records`, folded from the aggregate's own stream — never a `View_*`
/// read, which would make a projection rebuild change what the write side records,
/// ADR-20260815-030206). What this entry point adds is that the STREAM lookup is total.
pub async fn record_inbound_payment_fact(
    store: &dyn EventStore,
    fact: PaymentFactInbox,
    actor: &Actor,
) -> Result<RecordOutcome, DomainError> {
    let intent = intent_of_fact(&fact);
    record_on_payment_stream(store, intent, fact.into_domain_event(), actor).await
}

/// The ONE append: fold the aggregate's own stream, consult its own dedupe, append the fact it
/// received and nothing else.
async fn record_on_payment_stream(
    store: &dyn EventStore,
    intent: PaymentIntentId,
    event: DomainEvent,
    actor: &Actor,
) -> Result<RecordOutcome, DomainError> {
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
        Actor { user_id: uid(0xAC), user_type: "EXTERNAL".to_string(), domain_id: None, correlation_id: uid(0xC0), cause_id: None }
    }
    fn intent() -> PaymentIntentId {
        PaymentIntentId("pi_123".into())
    }
    fn birth() -> DomainEvent {
        let z = eur(0);
        DomainEvent::PaymentIntentCreated(PaymentIntentCreated {
            payment_intent_id: intent(),
            restaurant_id: RestaurantId(uid(3)),
            customer_id: CustomerId(uuid::Uuid::nil()),
            amount: eur(1960),
            checkout: CheckoutSnapshot {
                order_id: OrderId(uid(1)),
                cart_id: CartId(uid(2)),
                restaurant_id: RestaurantId(uid(3)),
                customer_id: CustomerId(uuid::Uuid::nil()),
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
                // RSO-1 evidence fields: absent — a pre-RSO-1 legacy-shape snapshot fixture.
                verdict: None,
                window_from: None,
                window_to: None,
                timezone: None,
                evaluated_at: None,
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

    /// rules.yaml#/PaymentCapturedOnFulfilment (recording half): the inbound fact lands on the
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
