//! ReclamationProcess (`specs/processmanager.yaml#/ReclamationProcess`) — HOOK IMPL + thin wrapper for
//! the GENERATED leg pipeline (`crate::generated::process_managers::reclamation_process`, #158,
//! ADR-20260726-163737). The pipeline (the linear-branch marker, the GrantCustomerCredit send plumbing
//! over `CustomerCredit-<customerId>`, skip/throw semantics) is generated; this module supplies the one
//! non-structural seam — the branch decision — plus the REPLACEMENT and REFUND arms a linear branch
//! cannot express.
//!
//! GOODWILL_CREDIT arm (WIRED): on a claim resolved as GOODWILL_CREDIT with a recorded amount, the saga
//! sends `GrantCustomerCredit` to the CustomerCredit ledger (idempotent per reclamationId — the ledger
//! dedups, so a re-delivered ReclamationResolved never double-grants; no state row needed).
//!
//! REPLACEMENT arm (WIRED, #159, ADR-20260726-171736): on a REPLACEMENT resolution the saga places a
//! NO-CHARGE replacement order — it reads the ORIGINAL order (cross-aggregate, by orderId) and sends
//! `PlaceReplacementOrder` to the Order aggregate, which remakes the same items as a new linked order
//! with a $0 buyer total and no `paymentIntentId` (`OrderPlaced.replacementOf`). It enters the normal
//! fulfilment/dispatch flow (restaurant remakes, rider redelivers). Idempotent per `reclamationId`: the
//! new order id is DERIVED deterministically from the claim id ([`replacement_order_id_for`]).
//!
//! REFUND arm (WIRED, #207, ADR-20260727-090000): on a FULL_REFUND / PARTIAL_REFUND resolution the saga
//! drives the EXISTING refund path (`RefundProcess`) to a SETTLED Stripe refund. The restaurant's
//! resolution IS the approval, so the refund settles WITHOUT a separate manual `ApproveRefund`. In one
//! reaction the saga reads the order (OrderTracking, by orderId) for the captured total + payment intent,
//! then drives RefundProcess's two application legs IN ORDER: [`super::refund::on_refund_requested`]
//! OPENS the `PENDING_APPROVAL` run (delivering `RefundOpened` to the Payment so `View_PendingRefunds`
//! folds it), then — because the resolution IS the approval — [`super::refund::approve_refund`] drives
//! the Stripe refund (delivering `RefundApproved`, run → `APPROVED_AWAITING_SETTLEMENT`); the inbound
//! `PaymentRefunded` later settles it via the normal RefundProcess leg. RefundProcess stays the ONE
//! Stripe-driving mechanism — nothing here duplicates it. FULL refunds the captured total; PARTIAL the
//! recorded amount, rejected with `RefundExceedsCaptured` when it exceeds the captured total. Idempotent
//! per reclamationId: a re-delivered resolution finds the run already APPROVED/settled (the RefundProcess
//! state-row admission guard) so `on_refund_requested` skips and the saga does NOT re-approve.
//!
//! A 4-way credit/refund/replacement/no-op split is not expressible in the current step DSL, so the
//! REFUND and REPLACEMENT arms live in THIS hand-written wrapper, the same seam that owns the branch
//! decision; the generated linear branch below still isolates only the GOODWILL_CREDIT credit grant.

use domain::generated::commands::{ApproveRefund, PlaceReplacementOrder};
use domain::generated::entities::Money;
use domain::generated::events::{ReclamationResolved, RefundRequested};
use domain::generated::scalars::{OrderId, ReclamationId, ReclamationResolution};
use domain::shared::errors::DomainError;
use serde_json::json;

use crate::generated::process_managers::reclamation_process;
use crate::generated::services::PaymentService;
use crate::pm_state::RefundProcessStateStore;
use crate::ports::{is_version_conflict, EventStore};
use crate::process_managers::{saga_actor, Outcome, TriggerEnvelope};
use crate::queries::OrderReadRepository;

/// Fixed UUIDv5 namespace for the replacement-order ids this saga derives. NEVER change it: a derived
/// order id must stay stable across re-reactions and deployments (it IS the idempotency key that makes
/// a re-delivered `ReclamationResolved` re-target the same order stream instead of placing a second).
fn replacement_namespace() -> uuid::Uuid {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        b"https://captain.food/process-managers/reclamation-replacement",
    )
}

/// The deterministic replacement order id for a resolved claim (one replacement per reclamationId).
pub fn replacement_order_id_for(reclamation_id: &ReclamationId) -> OrderId {
    OrderId(uuid::Uuid::new_v5(&replacement_namespace(), reclamation_id.0.as_bytes()))
}

/// The one non-structural seam: the leg's linear-branch decision. `true` runs the GOODWILL_CREDIT credit
/// grant; `false` is a benign no-op. The refund/replacement resolutions never reach this hook — they are
/// dispatched by the wrapper BEFORE the generated pipeline runs. Acts only when the resolution is
/// GOODWILL_CREDIT AND a credit amount was recorded (a GOODWILL_CREDIT with no amount is a no-op rather
/// than a runtime unwrap panic).
struct ReclamationResolvedHooks;

#[async_trait::async_trait]
impl reclamation_process::ReclamationResolvedHooks for ReclamationResolvedHooks {
    async fn branch(&self, event: &ReclamationResolved) -> Result<bool, DomainError> {
        Ok(event.resolution == ReclamationResolution::GOODWILL_CREDIT && event.refund_amount.is_some())
    }
}

/// EVENT leg `events.yaml#/ReclamationResolved`: dispatch the resolution to its automation.
/// - REPLACEMENT (rules.yaml#/ReplacementOrderPlacedOnResolution) → place a no-charge replacement order
///   (this wrapper seam, #159); idempotent per reclamationId via a deterministic order id.
/// - FULL_REFUND / PARTIAL_REFUND (rules.yaml#/RefundSettledOnResolution) → drive the existing refund
///   path to a settled Stripe refund (this wrapper seam, #207); idempotent per reclamationId via the
///   RefundProcess state-row admission guard.
/// - GOODWILL_CREDIT (rules.yaml#/GoodwillCreditGrantedOnResolution) → grant the claimant store credit
///   (the generated pipeline's linear branch).
pub async fn on_reclamation_resolved(
    store: &dyn EventStore,
    refund_state: &dyn RefundProcessStateStore,
    orders: &dyn OrderReadRepository,
    payments: &dyn PaymentService,
    event: &ReclamationResolved,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    if event.resolution == ReclamationResolution::REPLACEMENT {
        let actor = saga_actor(env);
        let cmd = PlaceReplacementOrder {
            // Deterministic per claim → a re-delivered resolution re-targets the same stream (no double).
            order_id: replacement_order_id_for(&event.reclamation_id),
            original_order_id: event.order_id,
            reclamation_id: event.reclamation_id,
        };
        return match crate::commands::place_replacement_order(store, cmd, &actor).await {
            Ok(()) => Ok(Outcome::Completed),
            // Concurrency/infra failures must NOT be swallowed — the runner retries them.
            Err(e) if is_version_conflict(&e) => Err(e),
            Err(DomainError::Repository(e)) => Err(DomainError::Repository(e)),
            // An anticipated rejection (e.g. the original order not found) is logged and skipped, never
            // fatal — mirrors the credit arm's target-rejection handling.
            Err(rejection) => {
                let reason = format!(
                    "PlaceReplacementOrder rejected: {rejection} — the Order aggregate's invariants stand; skipped"
                );
                eprintln!("saga[ReclamationProcess]: {reason}");
                Ok(Outcome::Skipped(reason))
            }
        };
    }
    if matches!(
        event.resolution,
        ReclamationResolution::FULL_REFUND | ReclamationResolution::PARTIAL_REFUND
    ) {
        return on_refund_resolution(store, refund_state, orders, payments, event, env).await;
    }
    reclamation_process::on_reclamation_resolved(store, &ReclamationResolvedHooks, event, env).await
}

/// The REFUND arm (#207): drive the EXISTING refund path to a SETTLED refund without a manual second
/// step and without a parallel Stripe mechanism (rules.yaml#/RefundSettledOnResolution).
async fn on_refund_resolution(
    store: &dyn EventStore,
    refund_state: &dyn RefundProcessStateStore,
    orders: &dyn OrderReadRepository,
    payments: &dyn PaymentService,
    event: &ReclamationResolved,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    // Read the order (cross-aggregate, by orderId) for the captured total + payment state — mirrors how
    // the refund legs read order state from OrderTracking.
    let Some(order) = orders.by_id(event.order_id).await? else {
        return Ok(Outcome::Skipped(format!(
            "order {} is not in the OrderTracking read model — nothing captured to refund",
            event.order_id.0
        )));
    };
    // Nothing captured → nothing to refund (the same guard RefundProcess's opening legs apply).
    if order.payment_status != "CAPTURED" {
        return Ok(Outcome::Skipped(format!(
            "order {} payment_status is {:?}, not CAPTURED — nothing to refund",
            event.order_id.0, order.payment_status
        )));
    }
    let captured = Money { amount_cents: order.total_amount_cents, currency: order.currency.clone() };
    // The refund amount: FULL_REFUND = the captured total; PARTIAL_REFUND = the recorded amount.
    let amount = match event.resolution {
        ReclamationResolution::FULL_REFUND => captured.clone(),
        ReclamationResolution::PARTIAL_REFUND => match event.refund_amount.clone() {
            Some(a) => a,
            // A PARTIAL_REFUND always carries an amount (ResolveReclamation rejects otherwise); a missing
            // one is a benign no-op rather than a panic.
            None => {
                return Ok(Outcome::Skipped(format!(
                    "PARTIAL_REFUND resolution for reclamation {} carries no amount — no-op",
                    event.reclamation_id.0
                )))
            }
        },
        // Unreachable: this fn is only called for FULL_REFUND / PARTIAL_REFUND.
        other => {
            return Ok(Outcome::Skipped(format!("resolution {other:?} is not a refund — no-op")))
        }
    };
    // A non-positive amount is a no-op (never a Stripe call for nothing).
    if amount.amount_cents.0 <= 0 {
        return Ok(Outcome::Skipped(format!(
            "refund amount for reclamation {} is not positive — no-op",
            event.reclamation_id.0
        )));
    }
    // Amount safety (rules.yaml#/RefundResolutionCappedAtCaptured): never refund more than the captured
    // total — a PARTIAL over the total (or a currency mismatch) is rejected BEFORE any Stripe call.
    if amount.currency != captured.currency || amount.amount_cents.0 > captured.amount_cents.0 {
        return Err(DomainError::rejected(
            "RefundExceedsCaptured",
            json!({
                "reclamationId": event.reclamation_id,
                "orderId": event.order_id,
                "requestedAmountCents": amount.amount_cents,
                "capturedAmountCents": captured.amount_cents,
            }),
        ));
    }
    // Drive the EXISTING refund path (RefundProcess), reusing its two legs — the ONE Stripe-driving
    // mechanism, never a parallel one. The synthesized RefundRequested OPENS the run (origin = this
    // claim resolution; the RefundRequested fact is not itself recorded — ReclamationResolved is the
    // recorded origin, RefundOpened/RefundApproved are the recorded refund facts).
    // The claim note carries through as the refund reason (ReclamationReason -> the events' String reason).
    let reason = event.note.clone().map(|r| r.0);
    let synth = RefundRequested {
        order_id: event.order_id,
        restaurant_id: order.restaurant_id,
        customer_id: Some(event.customer_id),
        reason: reason.clone(),
    };
    match super::refund::on_refund_requested(store, refund_state, orders, &synth, env).await? {
        // Freshly opened PENDING run: the resolution IS the approval → drive the Stripe refund now.
        Outcome::Completed => {
            let actor = saga_actor(env);
            let cmd = ApproveRefund {
                order_id: event.order_id,
                amount,
                reason,
            };
            super::refund::approve_refund(store, refund_state, payments, cmd, &actor).await?;
            Ok(Outcome::Completed)
        }
        // Redelivery (run already decided/settled) or nothing captured → do NOT open/approve a second
        // refund; the admission guard already vetoed re-opening a decided run.
        skipped @ Outcome::Skipped(_) => Ok(skipped),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::services::{
        PaymentRefundInput, PaymentRequestInput, PaymentRequestOutput, ServiceCallMeta,
    };
    use crate::pm_state::mem::MemRefundProcessState;
    use crate::process_managers::test_support::{envelope, MemStore};
    use crate::queries::{OrderFilter, OrderTrackingRow};
    use async_trait::async_trait;
    use domain::generated::entities::{CustomerContact, Money, OrderLineItem, PaymentBreakdown};
    use domain::generated::events::{DomainEvent, OrderPlaced};
    use domain::generated::scalars::{
        CurrencyCode, CustomerDisplayName, CustomerId, ExternalReference, MoneyCents, OfferId, OrderId,
        OrderStatus, PaymentIntentId, PhoneNumber, ProductName, ReclamationReason, RefundProcessStatus,
        RestaurantId, ServiceType,
    };
    use std::sync::Mutex;

    fn uid(n: u128) -> uuid::Uuid {
        uuid::Uuid::from_u128(n)
    }
    fn eur(cents: i64) -> Money {
        Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
    }
    fn breakdown(articles: i64, total: i64) -> PaymentBreakdown {
        PaymentBreakdown {
            articles: eur(articles),
            delivery: eur(0),
            service_fee: eur(0),
            total: eur(total),
            restaurant_contribution: eur(0),
            restaurant_payout: eur(articles),
            rider_payout: eur(0),
            captain_net: eur(0),
        }
    }
    /// The original PAID order (Order-uid(2), the id `resolved()` claims against) the REPLACEMENT arm
    /// reads back and remakes.
    fn original_order() -> DomainEvent {
        DomainEvent::OrderPlaced(OrderPlaced {
            mode: None,
            order_id: OrderId(uid(2)),
            r#ref: None,
            restaurant_id: RestaurantId(uid(9)),
            customer_id: Some(CustomerId(uid(3))),
            customer_contact: CustomerContact {
                display_name: CustomerDisplayName("Johnny".into()),
                email: None,
                phone: PhoneNumber("+33612345678".into()),
            },
            service_type: ServiceType::DELIVERY,
            delivery_address: None,
            items: vec![OrderLineItem {
                offer_id: OfferId(uid(8)),
                product_id: None,
                name: ProductName("Margherita".into()),
                offer_name: None,
                quantity: 2,
                unit_price: eur(980),
                selected_options: Vec::new(),
                line_total: eur(1960),
            }],
            total_amount: eur(1960),
            breakdown: breakdown(1960, 1960),
            note: None,
            replacement_of: None,
            payment_intent_id: Some(PaymentIntentId("pi_123".into())),
        })
    }
    fn resolved(resolution: ReclamationResolution, amount: Option<Money>) -> ReclamationResolved {
        ReclamationResolved {
            reclamation_id: ReclamationId(uid(1)),
            order_id: OrderId(uid(2)),
            customer_id: CustomerId(uid(3)),
            resolution,
            note: Some(ReclamationReason("Order arrived damaged".into())),
            refund_amount: amount,
        }
    }

    /// A fake OrderTracking read port serving a single canned row (or none) — mirrors RefundProcess's
    /// test double. `by_id` matches the resolved order id (uid(2)).
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
    fn tracking_row(payment_status: &str) -> OrderTrackingRow {
        OrderTrackingRow {
            order_id: OrderId(uid(2)),
            r#ref: ExternalReference("order-2".into()),
            restaurant_id: RestaurantId(uid(9)),
            customer_id: Some(CustomerId(uid(3))),
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
            payment_intent_id: Some(PaymentIntentId("pi_123".into())),
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
    fn captured_orders() -> FakeOrders {
        FakeOrders { row: Some(tracking_row("CAPTURED")) }
    }
    fn no_orders() -> FakeOrders {
        FakeOrders { row: None }
    }

    /// A Stripe gateway double recording refund requests (payment_intent_id, amount_cents).
    #[derive(Default)]
    struct RecordingGateway {
        refunds: Mutex<Vec<(String, i64)>>,
    }
    #[async_trait]
    impl PaymentService for RecordingGateway {
        async fn request(
            &self,
            _input: PaymentRequestInput,
            _meta: &ServiceCallMeta,
        ) -> Result<PaymentRequestOutput, DomainError> {
            unreachable!("ReclamationProcess never creates intents")
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

    fn payment_stream() -> String {
        domain::payment::stream(&PaymentIntentId("pi_123".into()))
    }
    fn refund_facts(store: &MemStore) -> (usize, usize) {
        let stream = store.stream(&payment_stream());
        (
            stream.iter().filter(|e| matches!(e, DomainEvent::RefundOpened(_))).count(),
            stream.iter().filter(|e| matches!(e, DomainEvent::RefundApproved(_))).count(),
        )
    }

    /// tests.yaml#/TestReclamationProcessGrantsGoodwillCredit —
    /// rules.yaml#/GoodwillCreditGrantedOnResolution: a GOODWILL_CREDIT resolution grants the claimant
    /// store credit, idempotently under re-delivery (the ledger dedups by reclamationId).
    #[tokio::test]
    async fn goodwill_credit_grants_store_credit() {
        let store = MemStore::default();
        let state = MemRefundProcessState::default();
        let gw = RecordingGateway::default();
        let event = resolved(ReclamationResolution::GOODWILL_CREDIT, Some(eur(500)));
        let outcome =
            on_reclamation_resolved(&store, &state, &no_orders(), &gw, &event, &envelope()).await.unwrap();
        assert_eq!(outcome, Outcome::Completed);
        let stream = store.stream(&format!("CustomerCredit-{}", uid(3)));
        let grants: Vec<_> = stream
            .iter()
            .filter_map(|e| match e {
                DomainEvent::CustomerCreditGranted(g) => Some(g.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].amount, eur(500));
        assert_eq!(grants[0].customer_id, CustomerId(uid(3)));
        // Re-delivered resolution: the ledger already granted this claim — no double-credit.
        on_reclamation_resolved(&store, &state, &no_orders(), &gw, &event, &envelope()).await.unwrap();
        let stream = store.stream(&format!("CustomerCredit-{}", uid(3)));
        assert_eq!(
            stream.iter().filter(|e| matches!(e, DomainEvent::CustomerCreditGranted(_))).count(),
            1
        );
        // The refund path was never touched.
        assert!(gw.refunds.lock().unwrap().is_empty());
    }

    /// tests.yaml#/TestReclamationProcessSettlesFullRefund — rules.yaml#/RefundSettledOnResolution: a
    /// FULL_REFUND resolution drives the existing refund path to a settled Stripe refund of the captured
    /// total (RefundOpened then RefundApproved on the Payment; one Stripe refund) — the resolution IS the
    /// approval. Idempotent under re-delivery: no second refund.
    #[tokio::test]
    async fn full_refund_settles_the_captured_total() {
        let store = MemStore::default();
        let state = MemRefundProcessState::default();
        let gw = RecordingGateway::default();
        let event = resolved(ReclamationResolution::FULL_REFUND, None);

        let outcome =
            on_reclamation_resolved(&store, &state, &captured_orders(), &gw, &event, &envelope()).await.unwrap();
        assert_eq!(outcome, Outcome::Completed);

        // One RefundOpened + one RefundApproved on the Payment; the run reached APPROVED_AWAITING_SETTLEMENT.
        assert_eq!(refund_facts(&store), (1, 1));
        assert_eq!(*gw.refunds.lock().unwrap(), vec![("pi_123".to_string(), 1960)]); // full captured total
        let row = state.by_order(OrderId(uid(2))).await.unwrap().expect("refund run row");
        assert_eq!(row.process_status, RefundProcessStatus::APPROVED_AWAITING_SETTLEMENT);
        assert_eq!(row.approved_amount_cents, Some(MoneyCents(1960)));

        // Re-delivered resolution: the run is already APPROVED → not re-opened, not re-approved.
        let outcome =
            on_reclamation_resolved(&store, &state, &captured_orders(), &gw, &event, &envelope()).await.unwrap();
        assert!(matches!(outcome, Outcome::Skipped(_)), "{outcome:?}");
        assert_eq!(refund_facts(&store), (1, 1));
        assert_eq!(gw.refunds.lock().unwrap().len(), 1); // no double-refund
    }

    /// tests.yaml#/TestReclamationProcessSettlesPartialRefund — rules.yaml#/RefundSettledOnResolution: a
    /// PARTIAL_REFUND resolution settles the RECORDED amount (RefundOpened for the full eligible total,
    /// RefundApproved + Stripe for the partial).
    #[tokio::test]
    async fn partial_refund_settles_the_recorded_amount() {
        let store = MemStore::default();
        let state = MemRefundProcessState::default();
        let gw = RecordingGateway::default();
        let event = resolved(ReclamationResolution::PARTIAL_REFUND, Some(eur(500)));

        let outcome =
            on_reclamation_resolved(&store, &state, &captured_orders(), &gw, &event, &envelope()).await.unwrap();
        assert_eq!(outcome, Outcome::Completed);

        assert_eq!(refund_facts(&store), (1, 1));
        assert_eq!(*gw.refunds.lock().unwrap(), vec![("pi_123".to_string(), 500)]); // the partial amount
        let row = state.by_order(OrderId(uid(2))).await.unwrap().expect("refund run row");
        assert_eq!(row.approved_amount_cents, Some(MoneyCents(500)));
    }

    /// tests.yaml#/TestReclamationProcessRefundOverCapturedRejected —
    /// rules.yaml#/RefundResolutionCappedAtCaptured: a PARTIAL_REFUND over the captured total is rejected
    /// BEFORE any Stripe call, and no refund fact is recorded.
    #[tokio::test]
    async fn partial_refund_over_captured_is_rejected() {
        let store = MemStore::default();
        let state = MemRefundProcessState::default();
        let gw = RecordingGateway::default();
        let event = resolved(ReclamationResolution::PARTIAL_REFUND, Some(eur(5000)));
        let err = on_reclamation_resolved(&store, &state, &captured_orders(), &gw, &event, &envelope())
            .await
            .unwrap_err();
        assert_eq!(err.code(), Some("RefundExceedsCaptured"), "{err:?}");
        assert_eq!(refund_facts(&store), (0, 0));
        assert!(gw.refunds.lock().unwrap().is_empty()); // rejected before any Stripe call
        assert!(state.by_order(OrderId(uid(2))).await.unwrap().is_none()); // no run opened
    }

    /// A refund resolution whose order captured nothing is a benign no-op (nothing to refund).
    #[tokio::test]
    async fn refund_with_nothing_captured_skips() {
        let store = MemStore::default();
        let state = MemRefundProcessState::default();
        let gw = RecordingGateway::default();
        let event = resolved(ReclamationResolution::FULL_REFUND, None);
        let orders = FakeOrders { row: Some(tracking_row("PENDING")) };
        let outcome =
            on_reclamation_resolved(&store, &state, &orders, &gw, &event, &envelope()).await.unwrap();
        assert!(matches!(outcome, Outcome::Skipped(_)), "{outcome:?}");
        assert_eq!(refund_facts(&store), (0, 0));
        assert!(gw.refunds.lock().unwrap().is_empty());
    }

    /// rules.yaml#/ReplacementOrderPlacedOnResolution: a REPLACEMENT resolution places a NO-CHARGE
    /// replacement order — same items as the original, $0 buyer total, no PaymentIntent, linked via
    /// `replacementOf` — on a deterministic new stream, idempotently under re-delivery.
    #[tokio::test]
    async fn replacement_places_a_no_charge_linked_order() {
        let store = MemStore::default();
        let state = MemRefundProcessState::default();
        let gw = RecordingGateway::default();
        // GIVEN the original paid order on its stream.
        store.seed(&format!("Order-{}", uid(2)), vec![original_order()]);
        let event = resolved(ReclamationResolution::REPLACEMENT, None);
        let new_id = replacement_order_id_for(&ReclamationId(uid(1)));

        let outcome =
            on_reclamation_resolved(&store, &state, &no_orders(), &gw, &event, &envelope()).await.unwrap();
        assert_eq!(outcome, Outcome::Completed);

        // THEN a single OrderPlaced lands on the derived stream: same items, $0, no payment, linked.
        let placed: Vec<_> = store
            .stream(&format!("Order-{}", new_id.0))
            .iter()
            .filter_map(|e| match e {
                DomainEvent::OrderPlaced(p) => Some(p.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(placed.len(), 1);
        let p = &placed[0];
        assert_eq!(p.order_id, new_id);
        assert_eq!(p.replacement_of, Some(OrderId(uid(2))));
        assert_eq!(p.total_amount, eur(0));
        assert_eq!(p.breakdown.total, eur(0));
        assert_eq!(p.payment_intent_id, None);
        assert_eq!(p.items, original_order_items()); // items copied verbatim
        assert_eq!(p.restaurant_id, RestaurantId(uid(9)));

        // Re-delivered resolution: same deterministic id → the version-0 birth is absorbed, no second order.
        on_reclamation_resolved(&store, &state, &no_orders(), &gw, &event, &envelope()).await.unwrap();
        assert_eq!(
            store
                .stream(&format!("Order-{}", new_id.0))
                .iter()
                .filter(|e| matches!(e, DomainEvent::OrderPlaced(_)))
                .count(),
            1
        );
    }

    /// A REPLACEMENT whose original order is missing rejects inside the command and is skipped (never
    /// fatal, never a half-placed order) — mirrors the credit arm's target-rejection handling.
    #[tokio::test]
    async fn replacement_with_missing_original_is_skipped() {
        let store = MemStore::default();
        let state = MemRefundProcessState::default();
        let gw = RecordingGateway::default();
        let event = resolved(ReclamationResolution::REPLACEMENT, None);
        let new_id = replacement_order_id_for(&ReclamationId(uid(1)));
        let outcome =
            on_reclamation_resolved(&store, &state, &no_orders(), &gw, &event, &envelope()).await.unwrap();
        assert!(matches!(outcome, Outcome::Skipped(_)), "{outcome:?}");
        assert!(store.stream(&format!("Order-{}", new_id.0)).is_empty());
    }

    fn original_order_items() -> Vec<OrderLineItem> {
        match original_order() {
            DomainEvent::OrderPlaced(p) => p.items,
            _ => unreachable!(),
        }
    }
}
