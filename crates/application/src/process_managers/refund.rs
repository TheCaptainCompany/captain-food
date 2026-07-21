//! RefundProcess (`specs/processmanager.yaml#/RefundProcess`) — HOOK IMPLS + thin wrappers for the
//! GENERATED leg pipelines (`crate::generated::process_managers::refund_process`, issue #25). The
//! pipelines (guards, admission, deliver plumbing over `Payment-<intentId>`, state.set) are
//! generated; this module keeps only the non-structural seams:
//!
//! - `read_order` — the OrderTracking read (NOTE the known feed gap: payment facts land on
//!   `Payment-%` streams the projection worker does not yet route to OrderTracking, so
//!   `payment_status` may lag; ADR-20260719-193500 flags the cross-stream projector route);
//! - `admit` — a re-delivered opening fact never regresses a DECIDED run back to pending;
//! - the Payment record-idempotency predicates (`domain::payment::already_records`);
//! - `input_payment_refund` — the Stripe refund input; a pending run without an intent cannot be
//!   refunded (same typed `RefundNotPending` rejection, fail closed before any gateway call).

use domain::generated::commands::ApproveRefund;
use domain::generated::entities::Money;
use domain::generated::events::{
    DomainEvent, OrderCancelledByCustomer, OrderCancelledByRestaurant, OrderRejectedByRestaurant,
    PaymentRefunded, RefundApproved, RefundDenied, RefundOpened, RefundRequested,
};
use domain::generated::scalars::{OrderId, RefundProcessStatus};
use domain::shared::errors::DomainError;
use serde_json::json;

use crate::generated::process_managers::refund_process::{self, OrderRead};
use crate::generated::process_managers::HookOutcome;
use crate::generated::services::{PaymentRefundInput, PaymentService};
use crate::pm_state::{RefundProcessRow, RefundProcessStateStore};
use crate::ports::{Actor, EventStore};
use crate::process_managers::{Outcome, TriggerEnvelope};
use crate::queries::OrderReadRepository;

/// Record-idempotency over the Payment aggregate's own fold ([`domain::payment::already_records`]);
/// a stream with no birth still records the fact (facts are never dropped).
fn payment_not_recorded(stream: &[DomainEvent], event: &DomainEvent) -> bool {
    match domain::payment::fold(stream) {
        Some(payment) => !domain::payment::already_records(&payment, event),
        None => true,
    }
}

/// Hooks shared by the four OPENING event legs (rejection / cancellations / customer request).
pub struct RefundOpenHooks<'a> {
    pub orders: &'a dyn OrderReadRepository,
}

impl RefundOpenHooks<'_> {
    /// The shared `read order` body: OrderTracking by id, coerced to the generated sink types
    /// (`total_amount_cents` carries the full [`Money`] the RefundOpened amount needs).
    async fn load_order(&self, order_id: OrderId) -> Result<HookOutcome<OrderRead>, DomainError> {
        let Some(o) = self.orders.by_id(order_id).await? else {
            return Ok(HookOutcome::Skip(format!(
                "order {} is not in the OrderTracking read model — nothing captured to refund",
                order_id.0
            )));
        };
        Ok(HookOutcome::Ready(OrderRead {
            payment_status: o.payment_status,
            total_amount_cents: Money { amount_cents: o.total_amount_cents, currency: o.currency },
            payment_intent_id: o.payment_intent_id,
        }))
    }
}

/// A re-delivered opening fact must not regress an approved/denied/settled run back to pending.
fn admit_only_pending(existing: &RefundProcessRow) -> Option<String> {
    (existing.process_status != RefundProcessStatus::PENDING_APPROVAL).then(|| {
        format!(
            "refund run for order {} is already decided ({:?}) — not regressing to PENDING_APPROVAL",
            existing.order_id.0, existing.process_status
        )
    })
}

macro_rules! impl_refund_open_hooks {
    ($trait_:ident) => {
        #[async_trait::async_trait]
        impl refund_process::$trait_ for RefundOpenHooks<'_> {
            async fn read_order(
                &self,
                order_id: OrderId,
            ) -> Result<HookOutcome<OrderRead>, DomainError> {
                self.load_order(order_id).await
            }

            fn admit(&self, existing: &RefundProcessRow) -> Option<String> {
                admit_only_pending(existing)
            }

            fn should_deliver_refund_opened(
                &self,
                stream: &[DomainEvent],
                event: &RefundOpened,
            ) -> bool {
                payment_not_recorded(stream, &DomainEvent::RefundOpened(event.clone()))
            }
        }
    };
}

impl_refund_open_hooks!(OrderRejectedByRestaurantHooks);
impl_refund_open_hooks!(OrderCancelledByCustomerHooks);
impl_refund_open_hooks!(OrderCancelledByRestaurantHooks);
impl_refund_open_hooks!(RefundRequestedHooks);

/// Hooks for the decision COMMAND legs and the inbound settlement leg.
pub struct RefundDecisionHooks;

#[async_trait::async_trait]
impl refund_process::ApproveRefundHooks for RefundDecisionHooks {
    /// The captured payment to refund. A pending run without an intent cannot be refunded — the
    /// same typed rejection, BEFORE any gateway call (fail closed).
    async fn input_payment_refund(
        &self,
        cmd: &ApproveRefund,
        row: &RefundProcessRow,
    ) -> Result<HookOutcome<PaymentRefundInput>, DomainError> {
        let Some(intent) = row.payment_intent_id.clone() else {
            return Err(DomainError::rejected("RefundNotPending", json!({ "orderId": cmd.order_id })));
        };
        Ok(HookOutcome::Ready(PaymentRefundInput {
            payment_intent_id: intent,
            amount: cmd.amount.clone(),
        }))
    }

    fn should_deliver_refund_approved(&self, stream: &[DomainEvent], event: &RefundApproved) -> bool {
        payment_not_recorded(stream, &DomainEvent::RefundApproved(event.clone()))
    }
}

impl refund_process::DenyRefundHooks for RefundDecisionHooks {
    fn should_deliver_refund_denied(&self, stream: &[DomainEvent], event: &RefundDenied) -> bool {
        payment_not_recorded(stream, &DomainEvent::RefundDenied(event.clone()))
    }
}

impl refund_process::PaymentRefundedHooks for RefundDecisionHooks {}

/// EVENT leg `events.yaml#/OrderRejectedByRestaurant` (rules.yaml#/RefundOnRejectionOrCancellation).
pub async fn on_order_rejected(
    store: &dyn EventStore,
    state: &dyn RefundProcessStateStore,
    orders: &dyn OrderReadRepository,
    event: &OrderRejectedByRestaurant,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    refund_process::on_order_rejected_by_restaurant(store, state, &RefundOpenHooks { orders }, event, env)
        .await
}

/// EVENT leg `events.yaml#/OrderCancelledByCustomer` (rules.yaml#/RefundOnRejectionOrCancellation).
pub async fn on_order_cancelled_by_customer(
    store: &dyn EventStore,
    state: &dyn RefundProcessStateStore,
    orders: &dyn OrderReadRepository,
    event: &OrderCancelledByCustomer,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    refund_process::on_order_cancelled_by_customer(store, state, &RefundOpenHooks { orders }, event, env)
        .await
}

/// EVENT leg `events.yaml#/OrderCancelledByRestaurant` (rules.yaml#/RefundOnRejectionOrCancellation).
pub async fn on_order_cancelled_by_restaurant(
    store: &dyn EventStore,
    state: &dyn RefundProcessStateStore,
    orders: &dyn OrderReadRepository,
    event: &OrderCancelledByRestaurant,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    refund_process::on_order_cancelled_by_restaurant(store, state, &RefundOpenHooks { orders }, event, env)
        .await
}

/// EVENT leg `events.yaml#/RefundRequested` (rules.yaml#/RefundOnRejectionOrCancellation).
pub async fn on_refund_requested(
    store: &dyn EventStore,
    state: &dyn RefundProcessStateStore,
    orders: &dyn OrderReadRepository,
    event: &RefundRequested,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    refund_process::on_refund_requested(store, state, &RefundOpenHooks { orders }, event, env).await
}

/// COMMAND leg `commands.yaml#/ApproveRefund` (rules.yaml#/RefundRequiresApproval): approval requests
/// the Stripe refund through the generated `payment.refund` port, records `RefundApproved` on the
/// Payment, and moves the run to APPROVED_AWAITING_SETTLEMENT.
pub async fn approve_refund(
    store: &dyn EventStore,
    state: &dyn RefundProcessStateStore,
    gateway: &dyn PaymentService,
    cmd: ApproveRefund,
    actor: &Actor,
) -> Result<(), DomainError> {
    refund_process::approve_refund(store, state, gateway, &RefundDecisionHooks, cmd, actor).await
}

/// COMMAND leg `commands.yaml#/DenyRefund` (rules.yaml#/RefundRequiresApproval): denial records
/// `RefundDenied` on the Payment and closes the run. No gateway call.
pub async fn deny_refund(
    store: &dyn EventStore,
    state: &dyn RefundProcessStateStore,
    cmd: domain::generated::commands::DenyRefund,
    actor: &Actor,
) -> Result<(), DomainError> {
    refund_process::deny_refund(store, state, &RefundDecisionHooks, cmd, actor).await
}

/// EVENT leg `events.yaml#/PaymentRefunded` (rules.yaml#/RefundSettledFactRecorded): the inbound
/// settlement closes an APPROVED_AWAITING_SETTLEMENT run; anything else skips benignly.
pub async fn on_payment_refunded(
    state: &dyn RefundProcessStateStore,
    event: &PaymentRefunded,
) -> Result<Outcome, DomainError> {
    refund_process::on_payment_refunded(state, &RefundDecisionHooks, event).await
}

// ================================================================================================
// Behaviour tests (specs/tests.yaml, RefundProcess) — each linked to its rules.yaml rule.
// ================================================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::services::ServiceCallMeta;
    use domain::generated::commands::DenyRefund;
    use crate::pm_state::mem::MemRefundProcessState;
    use crate::process_managers::test_support::{envelope, MemStore};
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

    /// Fake Stripe gateway recording refund requests; `request` (create-intent) is out of scope here.
    #[derive(Default)]
    struct RecordingGateway {
        refunds: Mutex<Vec<(String, i64)>>,
    }

    #[async_trait]
    impl PaymentService for RecordingGateway {
        async fn request(
            &self,
            _input: crate::generated::services::PaymentRequestInput,
            _meta: &ServiceCallMeta,
        ) -> Result<crate::generated::services::PaymentRequestOutput, DomainError> {
            unreachable!("RefundProcess never creates intents")
        }
        async fn refund(
            &self,
            input: PaymentRefundInput,
            _meta: &ServiceCallMeta,
        ) -> Result<(), DomainError> {
            self.refunds
                .lock()
                .unwrap()
                .push((input.payment_intent_id.0.clone(), input.amount.amount_cents.0));
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

    /// tests.yaml#/TestRefundOnOrderRejected — rules.yaml#/RefundOnRejectionOrCancellation +
    /// rules.yaml#/PendingRefundVisibleUntilDecided: a rejection of a CAPTURED order opens a
    /// PENDING_APPROVAL run AND records the RefundOpened refund-queue fact on the Payment
    /// (View_PendingRefunds folds it as REQUESTED), idempotently under re-delivery.
    #[tokio::test]
    async fn rejection_of_a_captured_order_opens_a_pending_refund() {
        let store = MemStore::default();
        let state = MemRefundProcessState::default();
        seed_payment(&store);
        let rejected = OrderRejectedByRestaurant {
            order_id: order_id(),
            restaurant_id: restaurant_id(),
            reason: "Out of ingredients".into(),
        };
        let outcome = on_order_rejected(&store, &state, &captured_orders(), &rejected, &envelope())
            .await
            .unwrap();
        assert_eq!(outcome, Outcome::Completed);
        let row = pending_row(&state).await;
        assert_eq!(row.process_status, RefundProcessStatus::PENDING_APPROVAL);
        assert_eq!(row.payment_intent_id, Some(PaymentIntentId("pi_123".into())));
        assert_eq!(row.reason.as_deref(), Some("Out of ingredients"));
        // The refund-queue fact landed on the Payment stream (tests.yaml then: refundOpened).
        let stream = store.stream(&domain::payment::stream(&PaymentIntentId("pi_123".into())));
        let opened: Vec<_> = stream
            .iter()
            .filter_map(|e| match e {
                DomainEvent::RefundOpened(o) => Some(o.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0].order_id, order_id());
        assert_eq!(opened[0].restaurant_id, restaurant_id());
        assert_eq!(opened[0].amount, eur(1960));
        assert_eq!(opened[0].reason.as_deref(), Some("Out of ingredients"));
        // Re-delivered opening fact: the Payment already records it — nothing appended twice.
        on_order_rejected(&store, &state, &captured_orders(), &rejected, &envelope())
            .await
            .unwrap();
        let stream = store.stream(&domain::payment::stream(&PaymentIntentId("pi_123".into())));
        assert_eq!(
            stream.iter().filter(|e| matches!(e, DomainEvent::RefundOpened(_))).count(),
            1
        );
    }

    /// rules.yaml#/RefundOnRejectionOrCancellation (nothing-captured corollary): no captured payment →
    /// benign skip, no run opened.
    #[tokio::test]
    async fn rejection_with_nothing_captured_skips() {
        let store = MemStore::default();
        let state = MemRefundProcessState::default();
        let outcome = on_order_rejected(
            &store,
            &state,
            &pending_orders(),
            &OrderRejectedByRestaurant {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                reason: "Out of ingredients".into(),
            },
            &envelope(),
        )
        .await
        .unwrap();
        assert!(matches!(outcome, Outcome::Skipped(ref m) if m.contains("nothing to refund")), "{outcome:?}");
        assert!(state.by_order(order_id()).await.unwrap().is_none());
        // No refund-queue fact either — nothing captured, nothing opened.
        assert!(store
            .stream(&domain::payment::stream(&PaymentIntentId("pi_123".into())))
            .is_empty());
    }

    /// tests.yaml#/TestRefundOnOrderCancelledByCustomer — rules.yaml#/RefundOnRejectionOrCancellation.
    #[tokio::test]
    async fn customer_cancellation_opens_a_pending_refund() {
        let store = MemStore::default();
        let state = MemRefundProcessState::default();
        seed_payment(&store);
        let outcome = on_order_cancelled_by_customer(
            &store,
            &state,
            &captured_orders(),
            &OrderCancelledByCustomer {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                reason: Some("Changed my mind".into()),
            },
            &envelope(),
        )
        .await
        .unwrap();
        assert_eq!(outcome, Outcome::Completed);
        assert_eq!(pending_row(&state).await.process_status, RefundProcessStatus::PENDING_APPROVAL);
    }

    /// tests.yaml#/TestRefundOnOrderCancelledByRestaurant — rules.yaml#/RefundOnRejectionOrCancellation.
    #[tokio::test]
    async fn restaurant_cancellation_opens_a_pending_refund() {
        let store = MemStore::default();
        let state = MemRefundProcessState::default();
        seed_payment(&store);
        let outcome = on_order_cancelled_by_restaurant(
            &store,
            &state,
            &captured_orders(),
            &OrderCancelledByRestaurant {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                reason: "Kitchen closed".into(),
            },
            &envelope(),
        )
        .await
        .unwrap();
        assert_eq!(outcome, Outcome::Completed);
        assert_eq!(pending_row(&state).await.process_status, RefundProcessStatus::PENDING_APPROVAL);
    }

    /// tests.yaml#/TestRefundOnRefundRequested — rules.yaml#/RefundOnRejectionOrCancellation.
    #[tokio::test]
    async fn customer_refund_request_opens_a_pending_refund() {
        let store = MemStore::default();
        let state = MemRefundProcessState::default();
        seed_payment(&store);
        let outcome = on_refund_requested(
            &store,
            &state,
            &captured_orders(),
            &RefundRequested {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                customer_id: None,
                reason: Some("Late delivery".into()),
            },
            &envelope(),
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
            &store,
            &state,
            &captured_orders(),
            &OrderRejectedByRestaurant {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                reason: "Out of ingredients".into(),
            },
            &envelope(),
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
            &store,
            &state,
            &captured_orders(),
            &RefundRequested {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                customer_id: None,
                reason: Some("Late delivery".into()),
            },
            &envelope(),
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
            &store,
            &state,
            &captured_orders(),
            &OrderRejectedByRestaurant {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                reason: "Out of ingredients".into(),
            },
            &envelope(),
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
            &store,
            &state,
            &captured_orders(),
            &OrderRejectedByRestaurant {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                reason: "Out of ingredients".into(),
            },
            &envelope(),
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
            &store,
            &state,
            &captured_orders(),
            &OrderRejectedByRestaurant {
                order_id: order_id(),
                restaurant_id: restaurant_id(),
                reason: "Out of ingredients".into(),
            },
            &envelope(),
        )
        .await
        .unwrap();
        assert!(matches!(outcome, Outcome::Skipped(ref m) if m.contains("already decided")), "{outcome:?}");
        assert_eq!(pending_row(&state).await.process_status, RefundProcessStatus::DENIED);
    }
}
