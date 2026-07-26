//! ReclamationProcess (`specs/processmanager.yaml#/ReclamationProcess`) — HOOK IMPL + thin wrapper for
//! the GENERATED leg pipeline (`crate::generated::process_managers::reclamation_process`, #158,
//! ADR-20260726-163737). The pipeline (the linear-branch marker, the GrantCustomerCredit send plumbing
//! over `CustomerCredit-<customerId>`, skip/throw semantics) is generated; this module supplies the one
//! non-structural seam — the branch decision.
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
//! new order id is DERIVED deterministically from the claim id ([`replacement_order_id_for`]), so a
//! re-delivered `ReclamationResolved` re-targets the same order stream and the version-0 birth is
//! absorbed — never a second replacement. This arm lives in THIS wrapper (not the generated linear
//! branch below): a 3-way credit/replacement/no-op split is not expressible in the current step DSL, so
//! it is carried in the same hand-written seam that owns the branch decision.
//!
//! Refund arm (FLAGGED follow-up): a FULL_REFUND / PARTIAL_REFUND resolution is a benign no-op here. The
//! existing refund path opens a PENDING_APPROVAL refund from a refundable fact and requires a SEPARATE
//! explicit ApproveRefund decision (its own state-row guard + Stripe call) to move money; a single 2-way
//! saga branch cannot isolate credit / refund without either blindly refunding a REPLACEMENT
//! (a wrong money-move) or duplicating the approval mechanism (forbidden). Wiring the refund arm through
//! the canonical `RequestRefund → RefundRequested → RefundProcess` path with correct per-resolution
//! dispatch is the flagged follow-up.

use domain::generated::commands::PlaceReplacementOrder;
use domain::generated::events::ReclamationResolved;
use domain::generated::scalars::{OrderId, ReclamationId, ReclamationResolution};
use domain::shared::errors::DomainError;

use crate::generated::process_managers::reclamation_process;
use crate::ports::{is_version_conflict, EventStore};
use crate::process_managers::{saga_actor, Outcome, TriggerEnvelope};

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
/// grant; `false` is a benign no-op (refund/replacement arms are flagged follow-ups). Acts only when the
/// resolution is GOODWILL_CREDIT AND a credit amount was recorded (a GOODWILL_CREDIT with no amount is a
/// no-op rather than a runtime unwrap panic).
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
/// - GOODWILL_CREDIT (rules.yaml#/GoodwillCreditGrantedOnResolution) → grant the claimant store credit
///   (the generated pipeline's linear branch).
/// - FULL_REFUND / PARTIAL_REFUND → a benign no-op (the refund arm is a flagged follow-up).
pub async fn on_reclamation_resolved(
    store: &dyn EventStore,
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
    reclamation_process::on_reclamation_resolved(store, &ReclamationResolvedHooks, event, env).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_managers::test_support::{envelope, MemStore};
    use domain::generated::entities::{
        CustomerContact, Money, OrderLineItem, PaymentBreakdown,
    };
    use domain::generated::events::{DomainEvent, OrderPlaced};
    use domain::generated::scalars::{
        CurrencyCode, CustomerDisplayName, CustomerId, MoneyCents, OfferId, OrderId, PhoneNumber,
        ProductName, ServiceType,
    };

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
            restaurant_id: domain::generated::scalars::RestaurantId(uid(9)),
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
            payment_intent_id: Some(domain::generated::scalars::PaymentIntentId("pi_123".into())),
        })
    }
    fn resolved(resolution: ReclamationResolution, amount: Option<Money>) -> ReclamationResolved {
        ReclamationResolved {
            reclamation_id: ReclamationId(uid(1)),
            order_id: OrderId(uid(2)),
            customer_id: CustomerId(uid(3)),
            resolution,
            note: None,
            refund_amount: amount,
        }
    }

    /// tests.yaml#/TestReclamationProcessGrantsGoodwillCredit —
    /// rules.yaml#/GoodwillCreditGrantedOnResolution: a GOODWILL_CREDIT resolution grants the claimant
    /// store credit, idempotently under re-delivery (the ledger dedups by reclamationId).
    #[tokio::test]
    async fn goodwill_credit_grants_store_credit() {
        let store = MemStore::default();
        let event = resolved(ReclamationResolution::GOODWILL_CREDIT, Some(eur(500)));
        let outcome = on_reclamation_resolved(&store, &event, &envelope()).await.unwrap();
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
        on_reclamation_resolved(&store, &event, &envelope()).await.unwrap();
        let stream = store.stream(&format!("CustomerCredit-{}", uid(3)));
        assert_eq!(
            stream.iter().filter(|e| matches!(e, DomainEvent::CustomerCreditGranted(_))).count(),
            1
        );
    }

    /// A refund resolution (FULL_REFUND / PARTIAL_REFUND) is a benign no-op in this slice (the refund arm
    /// is a flagged follow-up): no credit is granted and no replacement is placed.
    #[tokio::test]
    async fn refund_resolutions_are_noops() {
        for resolution in
            [ReclamationResolution::FULL_REFUND, ReclamationResolution::PARTIAL_REFUND]
        {
            let store = MemStore::default();
            let event = resolved(resolution, Some(eur(500)));
            let outcome = on_reclamation_resolved(&store, &event, &envelope()).await.unwrap();
            assert_eq!(outcome, Outcome::Completed);
            assert!(store.stream(&format!("CustomerCredit-{}", uid(3))).is_empty());
        }
    }

    /// rules.yaml#/ReplacementOrderPlacedOnResolution: a REPLACEMENT resolution places a NO-CHARGE
    /// replacement order — same items as the original, $0 buyer total, no PaymentIntent, linked via
    /// `replacementOf` — on a deterministic new stream, idempotently under re-delivery.
    #[tokio::test]
    async fn replacement_places_a_no_charge_linked_order() {
        let store = MemStore::default();
        // GIVEN the original paid order on its stream.
        store.seed(&format!("Order-{}", uid(2)), vec![original_order()]);
        let event = resolved(ReclamationResolution::REPLACEMENT, None);
        let new_id = replacement_order_id_for(&ReclamationId(uid(1)));

        let outcome = on_reclamation_resolved(&store, &event, &envelope()).await.unwrap();
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
        assert_eq!(p.restaurant_id, domain::generated::scalars::RestaurantId(uid(9)));

        // Re-delivered resolution: same deterministic id → the version-0 birth is absorbed, no second order.
        on_reclamation_resolved(&store, &event, &envelope()).await.unwrap();
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
        let event = resolved(ReclamationResolution::REPLACEMENT, None);
        let new_id = replacement_order_id_for(&ReclamationId(uid(1)));
        let outcome = on_reclamation_resolved(&store, &event, &envelope()).await.unwrap();
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
