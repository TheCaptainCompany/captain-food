//! RefundProcess (`specs/processmanager.yaml#/RefundProcess`) — approved refunds over the
//! `refund_process_manager` state row (ADR-20260719-193500):
//!
//! - The refundable facts (`OrderRejectedByRestaurant`, `OrderCancelledByCustomer`,
//!   `OrderCancelledByRestaurant`, `RefundRequested`) OPEN a pending run when the order's payment is
//!   CAPTURED (read from the OrderTracking read model, per the DSL `read` step) — nothing captured is
//!   the benign skip.
//! - The RESTAURANT (its own orders) or an ADMIN decides with the COMMAND legs [`approve_refund`] /
//!   [`deny_refund`] (authz = api.yaml roles): a non-pending run rejects with
//!   `errors.yaml#/RefundNotPending`; approval requests the outbound Stripe refund
//!   (`PaymentGateway::request_refund`) and records the decision on the Payment aggregate.
//! - Stripe settles with the inbound `PaymentRefunded` fact (recorded by the Payment aggregate);
//!   [`on_payment_refunded`] closes the run — an unknown or already-settled run skips (unsolicited
//!   refunds are benign per the DSL note).

use domain::generated::commands::{ApproveRefund, DenyRefund};
use domain::generated::events::{
    DomainEvent, OrderCancelledByCustomer, OrderCancelledByRestaurant, OrderRejectedByRestaurant,
    PaymentRefunded, RefundApproved, RefundDenied, RefundRequested,
};
use domain::generated::scalars::{OrderId, PaymentIntentId, RefundProcessStatus};
use domain::shared::errors::DomainError;
use serde_json::json;

use crate::pm_state::{RefundProcessRow, RefundProcessStateStore};
use crate::ports::{Actor, EventStore, PaymentGateway};
use crate::queries::OrderReadRepository;
use crate::repository::Repository;

/// The `payment_status` value the OrderTracking projector writes for a captured payment (the row
/// column is a free String in the read model; the projector's vocabulary is PENDING/CAPTURED/REFUNDED).
const CAPTURED: &str = "CAPTURED";

/// Opening EVENT legs, shared: read the order's payment from the OrderTracking read model (the DSL
/// `read` step — NOTE the known feed gap: payment facts land on `Payment-%` streams the projection
/// worker does not yet route to OrderTracking, so `payment_status` may lag; ADR-20260719-193500
/// flags the cross-stream projector route as the follow-up), guard `payment_status = CAPTURED`
/// (benign skip otherwise), then upsert the PENDING_APPROVAL run. A run that was already DECIDED
/// (approved/denied/refunded) is NEVER regressed to pending — re-opening skips.
async fn open_refund(
    state: &dyn RefundProcessStateStore,
    orders: &dyn OrderReadRepository,
    order_id: OrderId,
    reason: Option<String>,
) -> Result<crate::process_managers::Outcome, DomainError> {
    use crate::process_managers::Outcome;
    // read OrderTracking by order_id.
    let Some(order) = orders.by_id(order_id).await? else {
        return Ok(Outcome::Skipped(format!(
            "order {} is not in the OrderTracking read model — nothing captured to refund",
            order_id.0
        )));
    };
    // guard order.payment_status = CAPTURED (skip: true — nothing captured → nothing to refund).
    if order.payment_status != CAPTURED {
        return Ok(Outcome::Skipped(format!(
            "order {} has payment_status {} — nothing captured to refund",
            order_id.0, order.payment_status
        )));
    }
    // Idempotent upsert: keep a DECIDED run as-is (a re-delivered opening fact must not regress an
    // approved/denied/settled run back to pending).
    if let Some(existing) = state.by_order(order_id).await? {
        if existing.process_status != RefundProcessStatus::PENDING_APPROVAL {
            return Ok(Outcome::Skipped(format!(
                "refund run for order {} is already decided ({:?}) — not regressing to PENDING_APPROVAL",
                order_id.0, existing.process_status
            )));
        }
    }
    state
        .upsert(&RefundProcessRow {
            order_id,
            payment_intent_id: order.payment_intent_id.clone(),
            refund_id: None,
            process_status: RefundProcessStatus::PENDING_APPROVAL,
            approved_amount_cents: None,
            reason,
            last_update_utc: chrono::Utc::now(), // ignored on write; stamped by the store
        })
        .await?;
    Ok(Outcome::Completed)
}

/// EVENT leg `events.yaml#/OrderRejectedByRestaurant` (rules.yaml#/RefundOnRejectionOrCancellation):
/// the restaurant rejected a paid order — open a pending refund for a restaurant/admin decision.
pub async fn on_order_rejected(
    state: &dyn RefundProcessStateStore,
    orders: &dyn OrderReadRepository,
    event: &OrderRejectedByRestaurant,
) -> Result<crate::process_managers::Outcome, DomainError> {
    open_refund(state, orders, event.order_id, Some(event.reason.clone())).await
}

/// EVENT leg `events.yaml#/OrderCancelledByCustomer` (rules.yaml#/RefundOnRejectionOrCancellation).
pub async fn on_order_cancelled_by_customer(
    state: &dyn RefundProcessStateStore,
    orders: &dyn OrderReadRepository,
    event: &OrderCancelledByCustomer,
) -> Result<crate::process_managers::Outcome, DomainError> {
    open_refund(state, orders, event.order_id, event.reason.clone()).await
}

/// EVENT leg `events.yaml#/OrderCancelledByRestaurant` (rules.yaml#/RefundOnRejectionOrCancellation).
pub async fn on_order_cancelled_by_restaurant(
    state: &dyn RefundProcessStateStore,
    orders: &dyn OrderReadRepository,
    event: &OrderCancelledByRestaurant,
) -> Result<crate::process_managers::Outcome, DomainError> {
    open_refund(state, orders, event.order_id, Some(event.reason.clone())).await
}

/// EVENT leg `events.yaml#/RefundRequested` (rules.yaml#/RefundOnRejectionOrCancellation): the
/// customer asked (the Order aggregate already validated RequestRefund) — open a pending refund.
pub async fn on_refund_requested(
    state: &dyn RefundProcessStateStore,
    orders: &dyn OrderReadRepository,
    event: &RefundRequested,
) -> Result<crate::process_managers::Outcome, DomainError> {
    open_refund(state, orders, event.order_id, event.reason.clone()).await
}

/// `state.by` the order and require a PENDING_APPROVAL run, or reject with
/// `errors.yaml#/RefundNotPending` ("also thrown when no run exists for the order").
async fn require_pending(
    state: &dyn RefundProcessStateStore,
    order_id: OrderId,
) -> Result<RefundProcessRow, DomainError> {
    match state.by_order(order_id).await? {
        Some(row) if row.process_status == RefundProcessStatus::PENDING_APPROVAL => Ok(row),
        _ => Err(DomainError::rejected("RefundNotPending", json!({ "orderId": order_id }))),
    }
}

/// Record a refund decision on the Payment aggregate's stream — idempotent via the Payment's own
/// fold ([`domain::payment::already_records`]); a stream with no birth still records the fact
/// (facts are never dropped).
async fn deliver_to_payment(
    store: &dyn EventStore,
    payment_intent_id: &PaymentIntentId,
    event: DomainEvent,
    actor: &Actor,
) -> Result<(), DomainError> {
    let stream = domain::payment::stream(payment_intent_id);
    let (events, version) = store.load(&stream).await?;
    if let Some(payment) = domain::payment::fold(&events) {
        if domain::payment::already_records(&payment, &event) {
            return Ok(()); // re-delivered decision — already reflected
        }
    }
    Repository::new(store).save(&stream, version, &[event], actor).await.map(|_| ())
}

/// COMMAND leg `commands.yaml#/ApproveRefund` (rules.yaml#/RefundRequiresApproval): the restaurant
/// or an admin approves the pending refund (possibly partial) — request the Stripe refund through
/// the gateway, record `RefundApproved` on the Payment, and move the run to
/// APPROVED_AWAITING_SETTLEMENT (Stripe's `PaymentRefunded` settles it).
pub async fn approve_refund(
    store: &dyn EventStore,
    state: &dyn RefundProcessStateStore,
    gateway: &dyn PaymentGateway,
    cmd: ApproveRefund,
    actor: &Actor,
) -> Result<(), DomainError> {
    let row = require_pending(state, cmd.order_id).await?;
    // The captured payment to refund. A pending run without an intent cannot be refunded — the same
    // typed rejection (no refundable payment is pending for this order).
    let Some(intent) = row.payment_intent_id.clone() else {
        return Err(DomainError::rejected("RefundNotPending", json!({ "orderId": cmd.order_id })));
    };
    // call payment_gateway.request_refund — a gateway refusal rejects the command (fail closed;
    // nothing is recorded and the run stays PENDING_APPROVAL).
    gateway.request_refund(&intent, &cmd.amount).await?;
    // deliver RefundApproved → Payment (the aggregate records the decision).
    deliver_to_payment(
        store,
        &intent,
        DomainEvent::RefundApproved(RefundApproved {
            order_id: cmd.order_id,
            amount: cmd.amount.clone(),
            reason: cmd.reason.clone(),
        }),
        actor,
    )
    .await?;
    // state.set — approved, awaiting the inbound settlement.
    state
        .upsert(&RefundProcessRow {
            process_status: RefundProcessStatus::APPROVED_AWAITING_SETTLEMENT,
            approved_amount_cents: Some(cmd.amount.amount_cents),
            reason: cmd.reason,
            ..row
        })
        .await
}

/// COMMAND leg `commands.yaml#/DenyRefund` (rules.yaml#/RefundRequiresApproval): the restaurant or
/// an admin denies the pending refund — record `RefundDenied` on the Payment and close the run.
/// No gateway call.
pub async fn deny_refund(
    store: &dyn EventStore,
    state: &dyn RefundProcessStateStore,
    cmd: DenyRefund,
    actor: &Actor,
) -> Result<(), DomainError> {
    let row = require_pending(state, cmd.order_id).await?;
    if let Some(intent) = row.payment_intent_id.clone() {
        deliver_to_payment(
            store,
            &intent,
            DomainEvent::RefundDenied(RefundDenied {
                order_id: cmd.order_id,
                reason: cmd.reason.clone(),
            }),
            actor,
        )
        .await?;
    }
    state
        .upsert(&RefundProcessRow {
            process_status: RefundProcessStatus::DENIED,
            reason: Some(cmd.reason),
            ..row
        })
        .await
}

/// EVENT leg `events.yaml#/PaymentRefunded` (rules.yaml#/RefundSettledFactRecorded): Stripe reported
/// the settled refund (the fact is already recorded by the Payment aggregate) — close the run.
/// `state.by` order + `state.expect` APPROVED_AWAITING_SETTLEMENT: an unknown or already-settled run
/// skips (idempotent under Stripe re-delivery; an unsolicited refund is benign per the DSL note).
pub async fn on_payment_refunded(
    state: &dyn RefundProcessStateStore,
    event: &PaymentRefunded,
) -> Result<crate::process_managers::Outcome, DomainError> {
    use crate::process_managers::Outcome;
    let row = match state.by_order(event.order_id).await? {
        Some(row) if row.process_status == RefundProcessStatus::APPROVED_AWAITING_SETTLEMENT => row,
        Some(row) => {
            return Ok(Outcome::Skipped(format!(
                "refund run for order {} is {:?}, not awaiting settlement — skip (idempotent re-delivery / unsolicited refund)",
                event.order_id.0, row.process_status
            )))
        }
        None => {
            return Ok(Outcome::Skipped(format!(
                "no refund run for order {} — unsolicited refund settles on the Payment alone",
                event.order_id.0
            )))
        }
    };
    state
        .upsert(&RefundProcessRow {
            refund_id: Some(event.refund_id.clone()),
            process_status: RefundProcessStatus::REFUNDED,
            ..row
        })
        .await?;
    Ok(crate::process_managers::Outcome::Completed)
}

// ================================================================================================
// Behaviour tests (specs/tests.yaml, RefundProcess) — each linked to its rules.yaml rule.
// ================================================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm_state::mem::MemRefundProcessState;
    use crate::process_managers::test_support::MemStore;
    use crate::process_managers::Outcome;
    use crate::queries::{OrderFilter, OrderTrackingRow};
    use async_trait::async_trait;
    use domain::generated::entities::{CheckoutSnapshot, CustomerContact, Money, PaymentBreakdown};
    use domain::generated::events::{PaymentCaptured, PaymentIntentCreated};
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
    fn eur(cents: i64) -> Money {
        Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
    }

    /// Fake OrderTracking read port: one row, keyed by [`order_id`].
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

    /// An OrderTracking row with the given payment state (all other columns are inert fixtures).
    fn tracking_row(payment_status: &str, intent: Option<&str>) -> OrderTrackingRow {
        OrderTrackingRow {
            order_id: order_id(),
            r#ref: ExternalReference("order-1".into()),
            restaurant_id: restaurant_id(),
            customer_id: None,
            status: OrderStatus::PLACED,
            service_type: ServiceType::DELIVERY,
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
            delivery_address: None,
            estimated_ready_at: None,
            placed_at: chrono::Utc::now(),
            status_changed_at: chrono::Utc::now(),
            payment_intent_id: intent.map(|s| PaymentIntentId(s.into())),
            payment_status: payment_status.to_string(),
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

    fn captured_orders() -> FakeOrders {
        FakeOrders { row: Some(tracking_row("CAPTURED", Some("pi_123"))) }
    }
    fn pending_orders() -> FakeOrders {
        FakeOrders { row: Some(tracking_row("PENDING", Some("pi_123"))) }
    }

    /// Fake Stripe gateway recording refund requests; `create_payment_intent` is out of scope here.
    #[derive(Default)]
    struct RecordingGateway {
        refunds: Mutex<Vec<(String, i64)>>,
    }

    #[async_trait]
    impl PaymentGateway for RecordingGateway {
        async fn create_payment_intent(
            &self,
            _amount: &Money,
            _payment_method_id: &str,
        ) -> Result<crate::ports::CreatedPaymentIntent, DomainError> {
            unreachable!("RefundProcess never creates intents")
        }
        async fn request_refund(
            &self,
            payment_intent_id: &PaymentIntentId,
            amount: &Money,
        ) -> Result<(), DomainError> {
            self.refunds
                .lock()
                .unwrap()
                .push((payment_intent_id.0.clone(), amount.amount_cents.0));
            Ok(())
        }
    }

    fn admin_actor() -> Actor {
        Actor {
            user_id: uid(0xAD),
            user_type: 5, // ADMIN ordinal
            correlation_id: uid(0xC0),
            cause_id: None,
        }
    }

    /// A minimal captured Payment stream so the decision facts have a home.
    fn seed_payment(store: &MemStore) {
        let z = eur(0);
        store.seed(
            &domain::payment::stream(&PaymentIntentId("pi_123".into())),
            vec![
                DomainEvent::PaymentIntentCreated(PaymentIntentCreated {
                    payment_intent_id: PaymentIntentId("pi_123".into()),
                    restaurant_id: restaurant_id(),
                    customer_id: None,
                    amount: eur(1960),
                    checkout: CheckoutSnapshot {
                        order_id: order_id(),
                        cart_id: CartId(uid(2)),
                        restaurant_id: restaurant_id(),
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
                }),
                DomainEvent::PaymentCaptured(PaymentCaptured {
                    payment_intent_id: PaymentIntentId("pi_123".into()),
                    order_id: Some(order_id()),
                    restaurant_id: restaurant_id(),
                    amount: eur(1960),
                }),
            ],
        );
    }

    async fn pending_row(state: &MemRefundProcessState) -> RefundProcessRow {
        state.by_order(order_id()).await.unwrap().expect("refund run row")
    }

    /// tests.yaml#/TestRefundOnOrderRejected — rules.yaml#/RefundOnRejectionOrCancellation: a
    /// rejection of a CAPTURED order opens a PENDING_APPROVAL run (no domain event emitted).
    #[tokio::test]
    async fn rejection_of_a_captured_order_opens_a_pending_refund() {
        let state = MemRefundProcessState::default();
        let outcome = on_order_rejected(
            &state,
            &captured_orders(),
            &OrderRejectedByRestaurant {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                reason: "Out of ingredients".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome, Outcome::Completed);
        let row = pending_row(&state).await;
        assert_eq!(row.process_status, RefundProcessStatus::PENDING_APPROVAL);
        assert_eq!(row.payment_intent_id, Some(PaymentIntentId("pi_123".into())));
        assert_eq!(row.reason.as_deref(), Some("Out of ingredients"));
    }

    /// rules.yaml#/RefundOnRejectionOrCancellation (nothing-captured corollary): no captured payment →
    /// benign skip, no run opened.
    #[tokio::test]
    async fn rejection_with_nothing_captured_skips() {
        let state = MemRefundProcessState::default();
        let outcome = on_order_rejected(
            &state,
            &pending_orders(),
            &OrderRejectedByRestaurant {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                reason: "Out of ingredients".into(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(outcome, Outcome::Skipped(ref m) if m.contains("nothing captured")), "{outcome:?}");
        assert!(state.by_order(order_id()).await.unwrap().is_none());
    }

    /// tests.yaml#/TestRefundOnOrderCancelledByCustomer — rules.yaml#/RefundOnRejectionOrCancellation.
    #[tokio::test]
    async fn customer_cancellation_opens_a_pending_refund() {
        let state = MemRefundProcessState::default();
        let outcome = on_order_cancelled_by_customer(
            &state,
            &captured_orders(),
            &OrderCancelledByCustomer {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                reason: Some("Changed my mind".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome, Outcome::Completed);
        assert_eq!(pending_row(&state).await.process_status, RefundProcessStatus::PENDING_APPROVAL);
    }

    /// tests.yaml#/TestRefundOnOrderCancelledByRestaurant — rules.yaml#/RefundOnRejectionOrCancellation.
    #[tokio::test]
    async fn restaurant_cancellation_opens_a_pending_refund() {
        let state = MemRefundProcessState::default();
        let outcome = on_order_cancelled_by_restaurant(
            &state,
            &captured_orders(),
            &OrderCancelledByRestaurant {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                reason: "Kitchen closed".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome, Outcome::Completed);
        assert_eq!(pending_row(&state).await.process_status, RefundProcessStatus::PENDING_APPROVAL);
    }

    /// tests.yaml#/TestRefundOnRefundRequested — rules.yaml#/RefundOnRejectionOrCancellation.
    #[tokio::test]
    async fn customer_refund_request_opens_a_pending_refund() {
        let state = MemRefundProcessState::default();
        let outcome = on_refund_requested(
            &state,
            &captured_orders(),
            &RefundRequested {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                customer_id: None,
                reason: Some("Late delivery".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome, Outcome::Completed);
        assert_eq!(pending_row(&state).await.process_status, RefundProcessStatus::PENDING_APPROVAL);
    }

    /// tests.yaml#/TestRefundApprovedByAdmin — rules.yaml#/RefundRequiresApproval: approval requests
    /// the Stripe refund, records RefundApproved on the Payment, and awaits settlement.
    #[tokio::test]
    async fn approval_requests_the_refund_and_records_the_decision() {
        let store = MemStore::default();
        let state = MemRefundProcessState::default();
        let gateway = RecordingGateway::default();
        seed_payment(&store);
        on_order_rejected(
            &state,
            &captured_orders(),
            &OrderRejectedByRestaurant {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                reason: "Out of ingredients".into(),
            },
        )
        .await
        .unwrap();

        approve_refund(
            &store,
            &state,
            &gateway,
            ApproveRefund {
                order_id: order_id(),
                amount: eur(1960),
                reason: Some("Order rejected by the restaurant".into()),
            },
            &admin_actor(),
        )
        .await
        .unwrap();

        assert_eq!(*gateway.refunds.lock().unwrap(), vec![("pi_123".to_string(), 1960)]);
        let payment = domain::payment::fold(
            &store.stream(&domain::payment::stream(&PaymentIntentId("pi_123".into()))),
        )
        .unwrap();
        assert_eq!(payment.refund_decision, Some(domain::payment::RefundDecision::Approved));
        let row = pending_row(&state).await;
        assert_eq!(row.process_status, RefundProcessStatus::APPROVED_AWAITING_SETTLEMENT);
        assert_eq!(row.approved_amount_cents, Some(MoneyCents(1960)));
    }

    /// tests.yaml#/TestRefundDeniedByAdmin — rules.yaml#/RefundRequiresApproval: denial records
    /// RefundDenied on the Payment and closes the run; no gateway call.
    #[tokio::test]
    async fn denial_records_the_decision_and_closes_the_run() {
        let store = MemStore::default();
        let state = MemRefundProcessState::default();
        seed_payment(&store);
        on_refund_requested(
            &state,
            &captured_orders(),
            &RefundRequested {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                customer_id: None,
                reason: Some("Late delivery".into()),
            },
        )
        .await
        .unwrap();

        deny_refund(
            &store,
            &state,
            DenyRefund { order_id: order_id(), reason: "Outside the refund window".into() },
            &admin_actor(),
        )
        .await
        .unwrap();

        let payment = domain::payment::fold(
            &store.stream(&domain::payment::stream(&PaymentIntentId("pi_123".into()))),
        )
        .unwrap();
        assert_eq!(payment.refund_decision, Some(domain::payment::RefundDecision::Denied));
        let row = pending_row(&state).await;
        assert_eq!(row.process_status, RefundProcessStatus::DENIED);
        assert_eq!(row.reason.as_deref(), Some("Outside the refund window"));
    }

    /// tests.yaml#/TestRefundDecisionRejectedWhenNotPending — rules.yaml#/RefundRequiresApproval:
    /// a decision on an order with no PENDING_APPROVAL run rejects with RefundNotPending.
    #[tokio::test]
    async fn decision_without_a_pending_run_is_rejected() {
        let store = MemStore::default();
        let state = MemRefundProcessState::default();
        let gateway = RecordingGateway::default();
        let err = approve_refund(
            &store,
            &state,
            &gateway,
            ApproveRefund { order_id: order_id(), amount: eur(1960), reason: None },
            &admin_actor(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code(), Some("RefundNotPending"), "{err:?}");
        assert!(gateway.refunds.lock().unwrap().is_empty()); // rejected BEFORE any Stripe call
        let err = deny_refund(
            &store,
            &state,
            DenyRefund { order_id: order_id(), reason: "n/a".into() },
            &admin_actor(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code(), Some("RefundNotPending"), "{err:?}");
    }

    /// tests.yaml#/TestRefundSettledFactRecorded — rules.yaml#/RefundSettledFactRecorded: the inbound
    /// settlement closes an APPROVED_AWAITING_SETTLEMENT run; anything else skips benignly.
    #[tokio::test]
    async fn settlement_closes_the_approved_run() {
        let store = MemStore::default();
        let state = MemRefundProcessState::default();
        let gateway = RecordingGateway::default();
        seed_payment(&store);
        on_order_rejected(
            &state,
            &captured_orders(),
            &OrderRejectedByRestaurant {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                reason: "Out of ingredients".into(),
            },
        )
        .await
        .unwrap();
        approve_refund(
            &store,
            &state,
            &gateway,
            ApproveRefund { order_id: order_id(), amount: eur(1960), reason: None },
            &admin_actor(),
        )
        .await
        .unwrap();

        let refunded = PaymentRefunded {
            refund_id: RefundId("re_1".into()),
            payment_intent_id: PaymentIntentId("pi_123".into()),
            order_id: order_id(),
            restaurant_id: restaurant_id(),
            amount: eur(1960),
            reason: None,
        };
        let outcome = on_payment_refunded(&state, &refunded).await.unwrap();
        assert_eq!(outcome, Outcome::Completed);
        let row = pending_row(&state).await;
        assert_eq!(row.process_status, RefundProcessStatus::REFUNDED);
        assert_eq!(row.refund_id, Some(RefundId("re_1".into())));

        // Re-delivered settlement (and an unsolicited refund with no run) → benign skip.
        assert!(matches!(
            on_payment_refunded(&state, &refunded).await.unwrap(),
            Outcome::Skipped(_)
        ));
        let mut other = refunded;
        other.order_id = OrderId(uid(99));
        assert!(matches!(on_payment_refunded(&state, &other).await.unwrap(), Outcome::Skipped(_)));
    }

    /// rules.yaml#/RefundRequiresApproval (no-regression corollary): a re-delivered opening fact
    /// never regresses a decided run back to PENDING_APPROVAL.
    #[tokio::test]
    async fn reopening_a_decided_run_is_skipped() {
        let store = MemStore::default();
        let state = MemRefundProcessState::default();
        seed_payment(&store);
        on_order_rejected(
            &state,
            &captured_orders(),
            &OrderRejectedByRestaurant {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                reason: "Out of ingredients".into(),
            },
        )
        .await
        .unwrap();
        deny_refund(
            &store,
            &state,
            DenyRefund { order_id: order_id(), reason: "Outside the refund window".into() },
            &admin_actor(),
        )
        .await
        .unwrap();

        let outcome = on_order_rejected(
            &state,
            &captured_orders(),
            &OrderRejectedByRestaurant {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                reason: "Out of ingredients".into(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(outcome, Outcome::Skipped(ref m) if m.contains("already decided")), "{outcome:?}");
        assert_eq!(pending_row(&state).await.process_status, RefundProcessStatus::DENIED);
    }
}
