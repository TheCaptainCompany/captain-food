//! Hand-written `CustomerCreditBalanceCompute` (ADR-0040; #158, Part B of #207). The mechanical
//! `customer_id` column is set by the generated `project_customer_credit_balance` dispatch (born on
//! `CustomerCreditGranted`); this fold owns the two computed columns:
//!   * `balance_cents` — the running SUM over the ledger stream: `+= amount` on a grant, `-= amount` on
//!     a consume (the pure write-side fold in `domain::customer_credit` mirrors it, never negative);
//!   * `currency` — set from the first grant's amount, then preserved (single-currency per customer).
//! Projection LOGIC stays here (tested app code), never in SQL (ADR-0040).

use crate::projections::{CustomerCreditBalanceRow, CustomerCreditBalanceCompute, Envelope};
use domain::generated::events::DomainEvent;
use domain::generated::scalars::{CurrencyCode, MoneyCents};

pub struct CustomerCreditBalanceProjector;

impl CustomerCreditBalanceCompute for CustomerCreditBalanceProjector {
    /// Σ granted − Σ consumed (minor units), never negative (the write side rejects an overspend).
    /// `+= amount` on a grant (incl. the birth grant, `prev == None` ⇒ start from 0), `-= amount` on a
    /// consume.
    fn balance_cents(&self, prev: Option<&CustomerCreditBalanceRow>, env: &Envelope) -> MoneyCents {
        let base = prev.map(|r| r.balance_cents.0).unwrap_or(0);
        let next = match &env.event {
            DomainEvent::CustomerCreditGranted(e) => base + e.amount.amount_cents.0,
            DomainEvent::CustomerCreditConsumed(e) => base - e.amount.amount_cents.0,
            _ => base,
        };
        MoneyCents(next)
    }

    /// The ledger currency: set from the birth grant's amount; preserved thereafter (the column is only
    /// fed by `CustomerCreditGranted`, so this is called on the birth event with `prev == None`).
    fn currency(&self, prev: Option<&CustomerCreditBalanceRow>, env: &Envelope) -> CurrencyCode {
        match &env.event {
            DomainEvent::CustomerCreditGranted(e) => e.amount.currency.clone(),
            _ => prev.map(|r| r.currency.clone()).unwrap_or_else(|| CurrencyCode("EUR".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projections::project_customer_credit_balance;
    use domain::generated::entities::Money;
    use domain::generated::events::{CustomerCreditConsumed, CustomerCreditGranted};
    use domain::generated::scalars::{CustomerId, OrderId, ReclamationId};

    fn eur(cents: i64) -> Money {
        Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
    }
    fn env(event: DomainEvent) -> Envelope {
        Envelope {
            stream_name: "CustomerCredit-1".into(),
            position: 1,
            occurred_at: chrono::DateTime::from_timestamp(1, 0).unwrap(),
            event,
        }
    }
    fn granted(cents: i64, recl: u128) -> DomainEvent {
        DomainEvent::CustomerCreditGranted(CustomerCreditGranted {
            customer_id: CustomerId(uuid::Uuid::nil()),
            amount: eur(cents),
            reclamation_id: ReclamationId(uuid::Uuid::from_u128(recl)),
        })
    }
    fn consumed(cents: i64, order: u128) -> DomainEvent {
        DomainEvent::CustomerCreditConsumed(CustomerCreditConsumed {
            customer_id: CustomerId(uuid::Uuid::nil()),
            amount: eur(cents),
            order_id: OrderId(uuid::Uuid::from_u128(order)),
        })
    }

    /// A grant births the balance row (rules.yaml#/CreditGrantIncreasesBalance): balance = amount,
    /// currency set from the grant.
    #[test]
    fn grant_births_the_balance_row() {
        let c = CustomerCreditBalanceProjector;
        let row = project_customer_credit_balance(&c, None, &env(granted(500, 1))).unwrap();
        assert_eq!(row.balance_cents, MoneyCents(500));
        assert_eq!(row.currency, CurrencyCode("EUR".into()));
        assert_eq!(row.customer_id, CustomerId(uuid::Uuid::nil()));
    }

    /// A consume decreases the balance (rules.yaml#/CreditConsumeDecreasesBalance); a further grant adds.
    #[test]
    fn consume_decreases_and_grant_adds() {
        let c = CustomerCreditBalanceProjector;
        let row = project_customer_credit_balance(&c, None, &env(granted(500, 1))).unwrap();
        let row = project_customer_credit_balance(&c, Some(row), &env(consumed(300, 7))).unwrap();
        assert_eq!(row.balance_cents, MoneyCents(200));
        let row = project_customer_credit_balance(&c, Some(row), &env(granted(100, 2))).unwrap();
        assert_eq!(row.balance_cents, MoneyCents(300));
        assert_eq!(row.currency, CurrencyCode("EUR".into())); // preserved across the consume
    }

    /// A consume with no prior grant never materializes a row (mirrors the domain fold's `None`).
    #[test]
    fn consume_before_any_grant_is_no_row() {
        let c = CustomerCreditBalanceProjector;
        assert!(project_customer_credit_balance(&c, None, &env(consumed(300, 7))).is_none());
    }
}
