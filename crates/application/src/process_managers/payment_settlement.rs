//! PaymentSettlementProcess (`specs/payments/processmanager.yaml#/PaymentSettlementProcess`) —
//! HOOK IMPLS + wrappers for the GENERATED leg pipelines
//! (`crate::generated::process_managers::payment_settlement_process`), realizing the
//! authorize-then-capture posture (ADR-20260808-195315 §1.2/§1.3): CAPTURE the authorized payment
//! on fulfilment (`OrderDelivered` — the one handover fact for DELIVERY and COLLECTION alike),
//! RELEASE the uncaptured hold on rejection/cancellation ("no need to refund because no capture").
//! The recorded third arm — at-table service captures IN ADVANCE at checkout — is not built yet
//! (no such service type exists); the acceptance-timeout auto-cancel (§1.3, also unbuilt) will
//! ride the release arms through its cancellation fact.
//!
//! The pipelines (read, AUTHORIZED guard, port call) are generated; this module keeps the
//! non-structural seams:
//!
//! - `read_order` — the OrderTracking read, skipping the orders that never charge ($0
//!   replacements: no `payment_intent_id`) and stashing the intent id for the call-input hooks
//!   (the generated `OrderRead` carries only the guard column);
//! - the CAPTURE FAILURE branch (rules.yaml#/CaptureFailureIsRecordedAndPaged), which the step DSL
//!   cannot express ("on call error, deliver X" — the same wrapper-seam pattern as
//!   ReclamationProcess): a failed capture call is classified into the typed
//!   `CaptureFailureReason`, recorded as `PaymentCaptureFailed` on the Payment stream
//!   (idempotently, by the aggregate's own fold), and the ORIGINAL error still propagates so the
//!   saga runner surfaces the anomaly on `/saga` — recorded AND loud, never silent. The paging
//!   counter (`payment_capture_failed_total{reason}`, observability.yaml#/payment-settlement) is
//!   emitted by the Stripe adapter at the framework boundary, keeping this crate telemetry-free.
//! - the release legs need NO failure branch: a failed void self-heals — the authorization expires
//!   on its own within ~7 days and Stripe reports `PaymentReleased` — so the error simply
//!   propagates (surfaced on `/saga`; `payment_release_failed_total` counts it adapter-side).
//!
//! Settlement always arrives asynchronously as the inbound Stripe facts the Payment aggregate
//! records (`PaymentCaptured` / `PaymentReleased`), never as these calls' return values.

use std::sync::Mutex;

use domain::generated::events::{
    DomainEvent, OrderCancelledByCustomer, OrderCancelledByRestaurant, OrderDelivered,
    OrderRejectedByRestaurant, PaymentCaptureFailed,
};
use domain::generated::scalars::{CaptureFailureReason, OrderId, PaymentIntentId, RestaurantId};
use domain::shared::errors::DomainError;

use crate::generated::process_managers::payment_settlement_process::{self, OrderRead};
use crate::generated::process_managers::HookOutcome;
use crate::generated::services::{PaymentCaptureInput, PaymentReleaseInput, PaymentService};
use crate::ports::EventStore;
use crate::process_managers::{saga_actor, Outcome, TriggerEnvelope};
use crate::queries::OrderReadRepository;

/// Hooks shared by all four legs: the OrderTracking read + the call inputs. The read stashes the
/// row's `payment_intent_id` (the generated `OrderRead` carries only the guard column), and the
/// input hooks flip `attempted` — the wrapper's marker that a subsequent error came from the
/// GATEWAY CALL, not from the read (a transient read failure must propagate untouched, never be
/// recorded as a capture failure).
pub struct SettlementHooks<'a> {
    orders: &'a dyn OrderReadRepository,
    /// The intent of the row `read_order` admitted; `None` until the read ran.
    intent: Mutex<Option<PaymentIntentId>>,
    /// Set by the input hooks right before the port call — the only fallible step after them.
    attempted: Mutex<bool>,
}

impl<'a> SettlementHooks<'a> {
    pub fn new(orders: &'a dyn OrderReadRepository) -> Self {
        Self { orders, intent: Mutex::new(None), attempted: Mutex::new(false) }
    }

    /// The intent the leg attempted a gateway call for — `Some` iff the call phase was reached.
    fn attempted_intent(&self) -> Option<PaymentIntentId> {
        if *self.attempted.lock().unwrap() {
            self.intent.lock().unwrap().clone()
        } else {
            None
        }
    }

    /// The shared `read order` body: OrderTracking by id. THE CAPTAIN-AUTHORIZATION GUARD
    /// (rules.yaml#/PaymentCapturedOnFulfilment, ADR-20260813-233418 AR-2): a settlement leg keys
    /// on the presence of a Captain authorization, NEVER on the order-lifecycle fact alone — an
    /// order with no `payment_intent_id` skips here structurally. Today that is the $0
    /// replacement; post-V0 it is the EXTERNAL order ingested from Uber Eats (already paid on the
    /// partner's rails, no Captain PaymentIntent), whose delivery must never trip a phantom
    /// capture. The concrete `ExternalOrderReceived` regression test lands with that slice — the
    /// event does not exist yet.
    async fn load_order(&self, order_id: OrderId) -> Result<HookOutcome<OrderRead>, DomainError> {
        let Some(o) = self.orders.by_id(order_id, &crate::queries::ReadScope::System).await? else {
            return Ok(HookOutcome::Skip(format!(
                "order {} is not in the OrderTracking read model — nothing to settle",
                order_id.0
            )));
        };
        let Some(payment_intent_id) = o.payment_intent_id else {
            return Ok(HookOutcome::Skip(format!(
                "order {} carries no Captain payment intent (a $0 replacement, or an external \
                 order paid on the partner's rails) — nothing held to capture or release",
                order_id.0
            )));
        };
        *self.intent.lock().unwrap() = Some(payment_intent_id);
        Ok(HookOutcome::Ready(OrderRead { payment_status: o.payment_status }))
    }

    fn capture_input(&self) -> Result<HookOutcome<PaymentCaptureInput>, DomainError> {
        let intent = self.intent.lock().unwrap().clone().expect("read_order ran before the call");
        *self.attempted.lock().unwrap() = true;
        Ok(HookOutcome::Ready(PaymentCaptureInput { payment_intent_id: intent }))
    }

    fn release_input(&self) -> Result<HookOutcome<PaymentReleaseInput>, DomainError> {
        let intent = self.intent.lock().unwrap().clone().expect("read_order ran before the call");
        *self.attempted.lock().unwrap() = true;
        Ok(HookOutcome::Ready(PaymentReleaseInput { payment_intent_id: intent }))
    }
}

#[async_trait::async_trait]
impl payment_settlement_process::OrderDeliveredHooks for SettlementHooks<'_> {
    async fn read_order(&self, order_id: OrderId) -> Result<HookOutcome<OrderRead>, DomainError> {
        self.load_order(order_id).await
    }
    async fn input_payment_capture(
        &self,
        _event: &OrderDelivered,
        _order: &OrderRead,
    ) -> Result<HookOutcome<PaymentCaptureInput>, DomainError> {
        self.capture_input()
    }
}

macro_rules! impl_release_hooks {
    ($trait_:ident, $event:ty) => {
        #[async_trait::async_trait]
        impl payment_settlement_process::$trait_ for SettlementHooks<'_> {
            async fn read_order(
                &self,
                order_id: OrderId,
            ) -> Result<HookOutcome<OrderRead>, DomainError> {
                self.load_order(order_id).await
            }
            async fn input_payment_release(
                &self,
                _event: &$event,
                _order: &OrderRead,
            ) -> Result<HookOutcome<PaymentReleaseInput>, DomainError> {
                self.release_input()
            }
        }
    };
}

impl_release_hooks!(OrderRejectedByRestaurantHooks, OrderRejectedByRestaurant);
impl_release_hooks!(OrderCancelledByCustomerHooks, OrderCancelledByCustomer);
impl_release_hooks!(OrderCancelledByRestaurantHooks, OrderCancelledByRestaurant);

/// Classify a failed capture call into the typed reason (scalars.yaml#/CaptureFailureReason).
/// The adapter's error contract (crates/adapters/stripe/outbound.rs `map_error`): a card decline
/// is the catalogued `PaymentDeclined: …` invariant; a deterministic gateway refusal is
/// `PaymentGatewayRefused: …` (an EXPIRED authorization surfaces here — Stripe cancels the intent
/// and refuses the capture); everything transient is `Repository` — where the capture MAY have
/// succeeded provider-side (the settled webhook supersedes the recorded failure when it did).
fn classify_capture_error(err: &DomainError) -> (CaptureFailureReason, String) {
    let detail: String = err.to_string().chars().take(500).collect();
    let reason = match err {
        DomainError::Invariant(msg) if msg.starts_with("PaymentDeclined") => {
            CaptureFailureReason::CARD_DECLINED
        }
        // The catalogued rejection form of the same decline (errors.yaml#/PaymentDeclined).
        DomainError::Rejected { code, .. } if code == "PaymentDeclined" => {
            CaptureFailureReason::CARD_DECLINED
        }
        DomainError::Invariant(msg)
            if msg.contains("expired") || msg.contains("canceled") || msg.contains("cancelled") =>
        {
            CaptureFailureReason::AUTHORIZATION_EXPIRED
        }
        DomainError::Invariant(_) | DomainError::Rejected { .. } => {
            CaptureFailureReason::GATEWAY_REFUSED
        }
        _ => CaptureFailureReason::GATEWAY_UNAVAILABLE,
    };
    (reason, detail)
}

/// Record `PaymentCaptureFailed` on the Payment stream, idempotently by the aggregate's own fold
/// (a redelivered trigger whose capture fails again appends nothing — the page fired once).
async fn record_capture_failed(
    store: &dyn EventStore,
    payment_intent_id: PaymentIntentId,
    order_id: OrderId,
    restaurant_id: RestaurantId,
    reason: CaptureFailureReason,
    detail: String,
    env: &TriggerEnvelope,
) -> Result<(), DomainError> {
    let fact = DomainEvent::PaymentCaptureFailed(PaymentCaptureFailed {
        payment_intent_id: payment_intent_id.clone(),
        order_id,
        restaurant_id,
        reason,
        detail: Some(detail),
    });
    let stream = domain::payment::stream(&payment_intent_id);
    let (events, version) = store.load(&stream).await?;
    if let Some(payment) = domain::payment::fold(&events) {
        if domain::payment::already_records(&payment, &fact) {
            return Ok(());
        }
    }
    crate::repository::Repository::new(store)
        .save(&stream, version, &[fact], &saga_actor(env))
        .await?;
    Ok(())
}

/// EVENT leg `events.yaml#/OrderDelivered` (rules.yaml#/PaymentCapturedOnFulfilment +
/// #/CaptureFailureIsRecordedAndPaged): the generated pipeline, plus the wrapper-seam failure
/// branch — a failed CAPTURE CALL is recorded as `PaymentCaptureFailed` (typed reason) and the
/// original error still propagates, so the saga runner surfaces it on `/saga` (recorded AND loud).
/// A failure BEFORE the call (a read error) propagates untouched — nothing to record.
pub async fn on_order_delivered(
    store: &dyn EventStore,
    orders: &dyn OrderReadRepository,
    payments: &dyn PaymentService,
    event: &OrderDelivered,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    let hooks = SettlementHooks::new(orders);
    match payment_settlement_process::on_order_delivered(payments, &hooks, event, env).await {
        Err(e) => {
            let Some(intent) = hooks.attempted_intent() else {
                return Err(e); // pre-call failure (the read) — nothing was attempted at the gateway
            };
            let (reason, detail) = classify_capture_error(&e);
            record_capture_failed(
                store,
                intent,
                event.order_id,
                event.restaurant_id,
                reason,
                detail,
                env,
            )
            .await?;
            Err(e)
        }
        ok => ok,
    }
}

/// EVENT leg `events.yaml#/OrderRejectedByRestaurant` (rules.yaml#/AuthorizationReleasedWithoutCapture).
pub async fn on_order_rejected(
    orders: &dyn OrderReadRepository,
    payments: &dyn PaymentService,
    event: &OrderRejectedByRestaurant,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    let hooks = SettlementHooks::new(orders);
    payment_settlement_process::on_order_rejected_by_restaurant(payments, &hooks, event, env).await
}

/// EVENT leg `events.yaml#/OrderCancelledByCustomer` (rules.yaml#/AuthorizationReleasedWithoutCapture).
pub async fn on_order_cancelled_by_customer(
    orders: &dyn OrderReadRepository,
    payments: &dyn PaymentService,
    event: &OrderCancelledByCustomer,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    let hooks = SettlementHooks::new(orders);
    payment_settlement_process::on_order_cancelled_by_customer(payments, &hooks, event, env).await
}

/// EVENT leg `events.yaml#/OrderCancelledByRestaurant` (rules.yaml#/AuthorizationReleasedWithoutCapture).
pub async fn on_order_cancelled_by_restaurant(
    orders: &dyn OrderReadRepository,
    payments: &dyn PaymentService,
    event: &OrderCancelledByRestaurant,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    let hooks = SettlementHooks::new(orders);
    payment_settlement_process::on_order_cancelled_by_restaurant(payments, &hooks, event, env).await
}

// ================================================================================================
// Unit tests — the port-call outcomes the generated bed cannot see (specs/tests.yaml notes this
// module as the home of the capture/release call assertions + the failure branch).
// ================================================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::services::{
        PaymentRefundInput, PaymentRequestInput, PaymentRequestOutput, ServiceCallMeta,
    };
    use crate::process_managers::test_support::{envelope, MemStore};
    use crate::queries::{OrderFilter, OrderTrackingRow, ReadScope};
    use async_trait::async_trait;
    use domain::generated::scalars::*;

    fn uid(n: u128) -> uuid::Uuid {
        uuid::Uuid::from_u128(n)
    }

    struct FakeOrders {
        row: Option<OrderTrackingRow>,
    }
    #[async_trait]
    impl OrderReadRepository for FakeOrders {
        async fn list(
            &self,
            _filter: OrderFilter,
            _scope: &ReadScope,
        ) -> Result<Vec<OrderTrackingRow>, DomainError> {
            Ok(self.row.clone().into_iter().collect())
        }
        async fn by_id(
            &self,
            id: OrderId,
            _scope: &ReadScope,
        ) -> Result<Option<OrderTrackingRow>, DomainError> {
            Ok(self.row.clone().filter(|r| r.order_id == id))
        }
    }

    fn tracking_row(payment_status: &str, intent: Option<&str>) -> OrderTrackingRow {
        OrderTrackingRow {
            order_id: OrderId(uid(1)),
            r#ref: ExternalReference("order-1".into()),
            restaurant_id: RestaurantId(uid(3)),
            customer_id: Some(CustomerId(uid(4))),
            status: OrderStatus::DELIVERED,
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
            payment_intent_id: intent.map(|i| PaymentIntentId(i.into())),
            payment_status: payment_status.to_string(),
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
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    /// A gateway double recording capture/release calls; `capture` fails with the configured error.
    #[derive(Default)]
    struct RecordingGateway {
        captures: Mutex<Vec<String>>,
        releases: Mutex<Vec<String>>,
        capture_error: Option<fn() -> DomainError>,
    }
    #[async_trait]
    impl PaymentService for RecordingGateway {
        async fn request(
            &self,
            _input: PaymentRequestInput,
            _meta: &ServiceCallMeta,
        ) -> Result<PaymentRequestOutput, DomainError> {
            unreachable!("settlement legs never open intents")
        }
        async fn capture(
            &self,
            input: PaymentCaptureInput,
            _meta: &ServiceCallMeta,
        ) -> Result<(), DomainError> {
            self.captures.lock().unwrap().push(input.payment_intent_id.0.clone());
            match self.capture_error {
                Some(make) => Err(make()),
                None => Ok(()),
            }
        }
        async fn release(
            &self,
            input: PaymentReleaseInput,
            _meta: &ServiceCallMeta,
        ) -> Result<(), DomainError> {
            self.releases.lock().unwrap().push(input.payment_intent_id.0.clone());
            Ok(())
        }
        async fn refund(
            &self,
            _input: PaymentRefundInput,
            _meta: &ServiceCallMeta,
        ) -> Result<(), DomainError> {
            unreachable!("settlement legs never refund — that is RefundProcess's port use")
        }
    }

    fn delivered() -> OrderDelivered {
        OrderDelivered { order_id: OrderId(uid(1)), restaurant_id: RestaurantId(uid(3)) }
    }
    /// The Payment stream's birth fact (frozen checkout, minimal shape) — enough for the fold.
    fn birth() -> DomainEvent {
        use domain::generated::entities::{CheckoutSnapshot, CustomerContact, Money, PaymentBreakdown};
        let eur = |c: i64| Money { amount_cents: MoneyCents(c), currency: CurrencyCode("EUR".into()) };
        DomainEvent::PaymentIntentCreated(domain::generated::events::PaymentIntentCreated {
            payment_intent_id: PaymentIntentId("pi_123".into()),
            restaurant_id: RestaurantId(uid(3)),
            customer_id: CustomerId(uid(4)),
            amount: eur(1960),
            checkout: CheckoutSnapshot {
                order_id: OrderId(uid(1)),
                cart_id: CartId(uid(2)),
                restaurant_id: RestaurantId(uid(3)),
                customer_id: CustomerId(uid(4)),
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
                    delivery: eur(0),
                    service_fee: eur(0),
                    total: eur(1960),
                    restaurant_contribution: eur(0),
                    restaurant_payout: eur(1960),
                    rider_payout: eur(0),
                    captain_net: eur(0),
                },
                note: None,
            },
        })
    }
    fn rejected() -> OrderRejectedByRestaurant {
        OrderRejectedByRestaurant {
            order_id: OrderId(uid(1)),
            restaurant_id: RestaurantId(uid(3)),
            reason: "Out of ingredients".into(),
        }
    }

    /// tests.yaml#/TestCaptureRequestedOnOrderDelivered — rules.yaml#/PaymentCapturedOnFulfilment
    /// (the port-call half): an AUTHORIZED order's delivery captures the FULL authorized intent.
    #[tokio::test]
    async fn delivered_authorized_order_requests_the_capture() {
        let store = MemStore::default();
        let orders = FakeOrders { row: Some(tracking_row("AUTHORIZED", Some("pi_123"))) };
        let gw = RecordingGateway::default();
        let out = on_order_delivered(&store, &orders, &gw, &delivered(), &envelope()).await.unwrap();
        assert_eq!(out, Outcome::Completed);
        assert_eq!(*gw.captures.lock().unwrap(), vec!["pi_123".to_string()]);
        assert!(store.stream("Payment-pi_123").is_empty(), "no fact on a clean capture request");
    }

    /// rules.yaml#/PaymentCapturedOnFulfilment (guard half) + tests.yaml#/TestNoCaptureWithoutCaptainAuthorization:
    /// the delivery fact ALONE never captures — only an order carrying a Captain authorization
    /// (payment_intent_id present AND payment_status AUTHORIZED) does. Non-AUTHORIZED payments —
    /// already captured (redelivery), released, refunded — skip; so does an order with NO Captain
    /// intent: today the $0 replacement, post-V0 the Uber Eats external order paid on the
    /// partner's rails (ADR-20260813-233418 AR-2 — its concrete ExternalOrderReceived regression
    /// test lands with that slice).
    #[tokio::test]
    async fn non_authorized_or_unpaid_orders_are_benign_skips() {
        let store = MemStore::default();
        let gw = RecordingGateway::default();
        for row in [
            Some(tracking_row("CAPTURED", Some("pi_123"))),
            Some(tracking_row("RELEASED", Some("pi_123"))),
            Some(tracking_row("AUTHORIZED", None)), // $0 replacement: no intent
            None,                                   // no read-model row at all
        ] {
            let orders = FakeOrders { row };
            let out =
                on_order_delivered(&store, &orders, &gw, &delivered(), &envelope()).await.unwrap();
            assert!(matches!(out, Outcome::Skipped(_)), "{out:?}");
        }
        assert!(gw.captures.lock().unwrap().is_empty());
    }

    /// tests.yaml#/TestPaymentCaptureFailedRecorded (saga half) —
    /// rules.yaml#/CaptureFailureIsRecordedAndPaged: a DECLINED capture after fulfilment records
    /// PaymentCaptureFailed (typed reason) on the Payment stream AND the original error still
    /// propagates (surfaced on /saga — recorded and loud, never silent). Idempotent: a redelivered
    /// trigger whose capture fails again appends nothing.
    #[tokio::test]
    async fn declined_capture_records_the_typed_failure_and_stays_loud() {
        let store = MemStore::default();
        let orders = FakeOrders { row: Some(tracking_row("AUTHORIZED", Some("pi_123"))) };
        let gw = RecordingGateway {
            capture_error: Some(|| {
                DomainError::Invariant("PaymentDeclined: Your card was declined. (card_declined)".into())
            }),
            ..Default::default()
        };
        let err = on_order_delivered(&store, &orders, &gw, &delivered(), &envelope())
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::Invariant(_)), "{err:?}");
        let events = store.stream("Payment-pi_123");
        let fact = events
            .iter()
            .find_map(|e| match e {
                DomainEvent::PaymentCaptureFailed(f) => Some(f.clone()),
                _ => None,
            })
            .expect("PaymentCaptureFailed recorded");
        assert_eq!(fact.reason, CaptureFailureReason::CARD_DECLINED);
        assert_eq!(fact.order_id, OrderId(uid(1)));

        // Redelivery: the fold absorbs the second failure fact (birthless stream still dedupes
        // through the recorded fact once a birth exists; here the guard is the fold-less append
        // path, so assert via a seeded birth in the classification test below instead).
        let _ = on_order_delivered(&store, &orders, &gw, &delivered(), &envelope()).await;
        let count = store
            .stream("Payment-pi_123")
            .iter()
            .filter(|e| matches!(e, DomainEvent::PaymentCaptureFailed(_)))
            .count();
        assert_eq!(count, 2, "birthless stream has no fold to dedupe by — see seeded test");
    }

    /// The idempotency half on a REAL stream (with its PaymentIntentCreated birth): the second
    /// failed delivery finds `capture_failed` folded and appends nothing.
    #[tokio::test]
    async fn capture_failure_is_recorded_once_on_a_born_stream() {
        let store = MemStore::default();
        store.seed("Payment-pi_123", vec![birth()]);
        let orders = FakeOrders { row: Some(tracking_row("AUTHORIZED", Some("pi_123"))) };
        let gw = RecordingGateway {
            capture_error: Some(|| DomainError::Invariant("PaymentDeclined: declined".into())),
            ..Default::default()
        };
        let _ = on_order_delivered(&store, &orders, &gw, &delivered(), &envelope()).await;
        let _ = on_order_delivered(&store, &orders, &gw, &delivered(), &envelope()).await;
        let count = store
            .stream("Payment-pi_123")
            .iter()
            .filter(|e| matches!(e, DomainEvent::PaymentCaptureFailed(_)))
            .count();
        assert_eq!(count, 1, "the fold's capture_failed absorbs the redelivered failure");
    }

    /// A TRANSIENT gateway failure (Stripe 5xx / transport) is the GATEWAY_UNAVAILABLE class: the
    /// capture MAY have succeeded provider-side, so the fact is recorded for the operator and the
    /// settled webhook supersedes it if the money did move.
    #[tokio::test]
    async fn transient_capture_failure_is_classified_gateway_unavailable() {
        let store = MemStore::default();
        let orders = FakeOrders { row: Some(tracking_row("AUTHORIZED", Some("pi_123"))) };
        let gw = RecordingGateway {
            capture_error: Some(|| DomainError::Repository("stripe: capture failed (HTTP 500)".into())),
            ..Default::default()
        };
        let err =
            on_order_delivered(&store, &orders, &gw, &delivered(), &envelope()).await.unwrap_err();
        assert!(matches!(err, DomainError::Repository(_)), "{err:?}");
        let fact = store
            .stream("Payment-pi_123")
            .iter()
            .find_map(|e| match e {
                DomainEvent::PaymentCaptureFailed(f) => Some(f.clone()),
                _ => None,
            })
            .expect("recorded");
        assert_eq!(fact.reason, CaptureFailureReason::GATEWAY_UNAVAILABLE);
    }

    /// tests.yaml#/TestReleaseRequestedOnOrderRejected —
    /// rules.yaml#/AuthorizationReleasedWithoutCapture: a pre-capture rejection VOIDS the hold (no
    /// refund machinery); a post-capture one skips here (RefundProcess's CAPTURED arm owns it).
    #[tokio::test]
    async fn rejection_releases_the_authorized_hold_and_skips_after_capture() {
        let orders = FakeOrders { row: Some(tracking_row("AUTHORIZED", Some("pi_123"))) };
        let gw = RecordingGateway::default();
        let out = on_order_rejected(&orders, &gw, &rejected(), &envelope()).await.unwrap();
        assert_eq!(out, Outcome::Completed);
        assert_eq!(*gw.releases.lock().unwrap(), vec!["pi_123".to_string()]);

        let captured = FakeOrders { row: Some(tracking_row("CAPTURED", Some("pi_123"))) };
        let out = on_order_rejected(&captured, &gw, &rejected(), &envelope()).await.unwrap();
        assert!(matches!(out, Outcome::Skipped(_)), "{out:?}");
        assert_eq!(gw.releases.lock().unwrap().len(), 1, "no second void");
    }
}
