//! sqlx read-model repository over the materialized `customercreditbalance` projection table (ADR-0040;
//! #158, Part B of #207). Backs the `customerCredit` GraphQL query via
//! `application::queries::CustomerCreditReadRepository` — the customer's spendable store-credit balance,
//! scoped to the caller's Customer identity (the me-pattern).

use application::queries::{CustomerCreditBalanceRow, CustomerCreditReadRepository};
use async_trait::async_trait;
use domain::generated::scalars::CustomerId;
use domain::shared::errors::DomainError;
use sqlx::PgPool;

use super::customer_credit_balance_store;

/// Postgres adapter for the CustomerCreditBalance read model.
pub struct PgCustomerCreditRepository {
    pool: PgPool,
}

impl PgCustomerCreditRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CustomerCreditReadRepository for PgCustomerCreditRepository {
    async fn by_customer(
        &self,
        customer_id: CustomerId,
    ) -> Result<Option<CustomerCreditBalanceRow>, DomainError> {
        customer_credit_balance_store::load(&self.pool, customer_id).await
    }
}
