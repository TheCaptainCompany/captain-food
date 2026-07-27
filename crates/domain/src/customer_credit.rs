//! CustomerCredit aggregate — the PURE write-side state fold (#158, ADR-20260726-163737), a per-customer
//! store-credit ledger keyed by `customerId` (the aggregate identity IS the customer, like `prospect.rs`
//! is keyed by its RestaurantId). Credits (goodwill, from a resolved claim) and debits (spent at checkout)
//! are recorded as facts on the customer's ledger stream; the AVAILABLE BALANCE is a fold over them
//! (`granted − consumed`, never negative). There is NO status lifecycle — a ledger has a balance, not a
//! state. `GrantCustomerCredit` is idempotent per `reclamationId` (at most one grant per resolved claim),
//! so a re-delivered `ReclamationResolved` from the ReclamationProcess saga never double-grants. No I/O.

use crate::generated::events::DomainEvent;
use crate::generated::scalars::{CurrencyCode, OrderId, ReclamationId};

/// What the CustomerCredit command handlers need to accept or reject a command. `None` (from [`fold`])
/// means the customer has no ledger yet — the balance is 0 and no grant has been recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomerCreditState {
    /// Available balance in minor units: `Σ granted − Σ consumed`, never negative.
    pub balance_cents: i64,
    /// The ledger currency, set from the first grant's amount (a customer's ledger is single-currency —
    /// their local currency, EUR for Tours V0). `None` before any grant.
    pub currency: Option<CurrencyCode>,
    /// The reclamations already granted, so a re-delivered grant for the same claim is a no-op
    /// (grant idempotency key — rules.yaml#/GoodwillCreditGrantedOnResolution).
    pub granted_reclamations: Vec<ReclamationId>,
    /// The orders already debited, with the amount spent per order — the CONSUME idempotency key
    /// (rules.yaml#/CreditConsumedAtMostOncePerOrder): a re-delivered / retried consume for the same
    /// `orderId` is a no-op (no double-spend), and the recorded amount lets checkout reuse the exact
    /// applied amount on a retry rather than recomputing it against a now-lower balance.
    pub consumed_orders: Vec<(OrderId, i64)>,
}

impl CustomerCreditState {
    /// Whether a grant for `reclamation_id` has already been recorded (idempotency guard).
    pub fn already_granted(&self, reclamation_id: &ReclamationId) -> bool {
        self.granted_reclamations.contains(reclamation_id)
    }

    /// The credit amount (minor units) already consumed for `order_id`, if any — the consume
    /// idempotency guard. `Some(amount)` means this order was already debited (a retry must not
    /// consume again, and checkout reuses this exact amount); `None` means it never was.
    pub fn consumed_for(&self, order_id: &OrderId) -> Option<i64> {
        self.consumed_orders.iter().find(|(id, _)| id == order_id).map(|(_, amount)| *amount)
    }
}

/// Fold a CustomerCredit stream (events in version order) into its current balance. `None` ⇔ the stream
/// has no `CustomerCreditGranted` yet, i.e. the customer has no ledger (balance 0).
pub fn fold(events: &[DomainEvent]) -> Option<CustomerCreditState> {
    events.iter().fold(None, apply)
}

/// Apply one event — a pure transition, total over the whole event union.
fn apply(state: Option<CustomerCreditState>, event: &DomainEvent) -> Option<CustomerCreditState> {
    match event {
        // A grant births the ledger (or adds to it); the balance rises by the granted amount.
        DomainEvent::CustomerCreditGranted(e) => {
            let mut s = state.unwrap_or(CustomerCreditState {
                balance_cents: 0,
                currency: None,
                granted_reclamations: Vec::new(),
                consumed_orders: Vec::new(),
            });
            s.balance_cents += e.amount.amount_cents.0;
            s.currency.get_or_insert_with(|| e.amount.currency.clone());
            s.granted_reclamations.push(e.reclamation_id);
            Some(s)
        }
        // A debit lowers the balance and records the order it was applied to. Impossible before a grant
        // (guarded in the handler). A second debit for the same order never reaches the fold — the
        // handler no-ops it — but the recorded (order, amount) stays the consume idempotency key.
        DomainEvent::CustomerCreditConsumed(e) => {
            let mut s = state?;
            s.balance_cents -= e.amount.amount_cents.0;
            s.consumed_orders.push((e.order_id, e.amount.amount_cents.0));
            Some(s)
        }
        _ => state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::Aggregate;
    use crate::generated::entities::Money;
    use crate::generated::events::{CustomerCreditConsumed, CustomerCreditGranted};
    use crate::generated::scalars::{CurrencyCode, CustomerId, MoneyCents, OrderId};

    fn eur(cents: i64) -> Money {
        Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
    }
    fn granted(cents: i64, recl: u128) -> DomainEvent {
        DomainEvent::CustomerCreditGranted(CustomerCreditGranted {
            customer_id: CustomerId(uuid::Uuid::nil()),
            amount: eur(cents),
            reclamation_id: ReclamationId(uuid::Uuid::from_u128(recl)),
        })
    }
    fn consumed(cents: i64) -> DomainEvent {
        DomainEvent::CustomerCreditConsumed(CustomerCreditConsumed {
            customer_id: CustomerId(uuid::Uuid::nil()),
            amount: eur(cents),
            order_id: OrderId(uuid::Uuid::nil()),
        })
    }

    #[test]
    fn no_grant_means_no_ledger() {
        assert_eq!(fold(&[]), None);
        // A debit without a grant never materializes a ledger.
        assert_eq!(fold(&[consumed(100)]), None);
    }

    #[test]
    fn grant_increases_the_balance() {
        assert_eq!(fold(&[granted(500, 1)]).unwrap().balance_cents, 500);
        assert_eq!(fold(&[granted(500, 1), granted(300, 2)]).unwrap().balance_cents, 800);
    }

    #[test]
    fn consume_decreases_the_balance() {
        let s = fold(&[granted(500, 1), consumed(300)]).unwrap();
        assert_eq!(s.balance_cents, 200);
    }

    #[test]
    fn grants_are_tracked_for_idempotency() {
        let s = fold(&[granted(500, 1)]).unwrap();
        assert!(s.already_granted(&ReclamationId(uuid::Uuid::from_u128(1))));
        assert!(!s.already_granted(&ReclamationId(uuid::Uuid::from_u128(2))));
    }

    #[test]
    fn currency_is_set_from_the_first_grant() {
        let s = fold(&[granted(500, 1)]).unwrap();
        assert_eq!(s.currency, Some(CurrencyCode("EUR".into())));
        assert!(fold(&[]).is_none());
    }

    fn consumed_for_order(cents: i64, order: u128) -> DomainEvent {
        DomainEvent::CustomerCreditConsumed(CustomerCreditConsumed {
            customer_id: CustomerId(uuid::Uuid::nil()),
            amount: eur(cents),
            order_id: OrderId(uuid::Uuid::from_u128(order)),
        })
    }

    /// The consume idempotency key (rules.yaml#/CreditConsumedAtMostOncePerOrder): the fold records the
    /// order + amount so a retry for the same order can be recognized (no double-spend) and the exact
    /// applied amount reused.
    #[test]
    fn consumes_are_tracked_per_order_for_idempotency() {
        let s = fold(&[granted(500, 1), consumed_for_order(300, 7)]).unwrap();
        assert_eq!(s.balance_cents, 200);
        assert_eq!(s.consumed_for(&OrderId(uuid::Uuid::from_u128(7))), Some(300));
        assert_eq!(s.consumed_for(&OrderId(uuid::Uuid::from_u128(8))), None);
    }

    #[test]
    fn stream_name_matches_the_aggregate_format() {
        let id = uuid::Uuid::nil();
        assert_eq!(CustomerCreditState::stream(CustomerId(id)), format!("CustomerCredit-{id}"));
    }
}
