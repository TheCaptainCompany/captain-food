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
//! Refund arm (FLAGGED follow-up): a FULL_REFUND / PARTIAL_REFUND resolution is a benign no-op here. The
//! existing refund path opens a PENDING_APPROVAL refund from a refundable fact and requires a SEPARATE
//! explicit ApproveRefund decision (its own state-row guard + Stripe call) to move money; a single 2-way
//! saga branch cannot isolate credit / refund / REPLACEMENT without either blindly refunding a REPLACEMENT
//! (a wrong money-move) or duplicating the approval mechanism (forbidden). Wiring the refund arm through
//! the canonical `RequestRefund → RefundRequested → RefundProcess` path with correct per-resolution
//! dispatch is the flagged follow-up. REPLACEMENT is likewise a reserved no-op (#159).

use domain::generated::events::ReclamationResolved;
use domain::generated::scalars::ReclamationResolution;
use domain::shared::errors::DomainError;

use crate::generated::process_managers::reclamation_process;
use crate::ports::EventStore;
use crate::process_managers::{Outcome, TriggerEnvelope};

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

/// EVENT leg `events.yaml#/ReclamationResolved` (rules.yaml#/GoodwillCreditGrantedOnResolution): grant
/// the claimant store credit when the claim is resolved as GOODWILL_CREDIT.
pub async fn on_reclamation_resolved(
    store: &dyn EventStore,
    event: &ReclamationResolved,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    reclamation_process::on_reclamation_resolved(store, &ReclamationResolvedHooks, event, env).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_managers::test_support::{envelope, MemStore};
    use domain::generated::entities::Money;
    use domain::generated::events::DomainEvent;
    use domain::generated::scalars::{CurrencyCode, CustomerId, MoneyCents, OrderId, ReclamationId};

    fn uid(n: u128) -> uuid::Uuid {
        uuid::Uuid::from_u128(n)
    }
    fn eur(cents: i64) -> Money {
        Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
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

    /// A refund / replacement resolution is a benign no-op in this slice (flagged follow-ups): no credit
    /// is granted (rules.yaml#/GoodwillCreditGrantedOnResolution — the credit arm is GOODWILL_CREDIT-only).
    #[tokio::test]
    async fn refund_and_replacement_resolutions_are_noops() {
        for resolution in
            [ReclamationResolution::FULL_REFUND, ReclamationResolution::REPLACEMENT]
        {
            let store = MemStore::default();
            let event = resolved(resolution, Some(eur(500)));
            let outcome = on_reclamation_resolved(&store, &event, &envelope()).await.unwrap();
            assert_eq!(outcome, Outcome::Completed);
            assert!(store.stream(&format!("CustomerCredit-{}", uid(3))).is_empty());
        }
    }
}
