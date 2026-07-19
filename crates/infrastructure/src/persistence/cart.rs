//! sqlx read-model repository over the materialized `cart` projection table (ADR-0040). Backs the
//! `carts` / `cart` GraphQL queries via `application::queries::CartReadRepository`.

use application::queries::{CartReadRepository, CartRow};
use async_trait::async_trait;
use domain::generated::scalars::{CartId, CartStatus, CustomerId, SessionId};
use domain::shared::errors::DomainError;
use sqlx::PgPool;

use super::cart_store;
use super::db_err;
use super::enum_sql::EnumOrd;

/// Postgres adapter for the Cart read model.
pub struct PgCartRepository {
    pool: PgPool,
}

impl PgCartRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CartReadRepository for PgCartRepository {
    /// The customer's carts, most recently updated first.
    async fn by_customer(&self, customer_id: CustomerId) -> Result<Vec<CartRow>, DomainError> {
        let sql = format!(
            "SELECT {} FROM cart WHERE customer_id = $1 ORDER BY updated_at DESC",
            cart_store::COLUMNS
        );
        let rows = sqlx::query(&sql)
            .bind(customer_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        rows.iter().map(cart_store::decode).collect()
    }

    async fn by_id(&self, id: CartId) -> Result<Option<CartRow>, DomainError> {
        cart_store::load(&self.pool, id).await
    }

    /// The session's OPEN carts (CartBindingProcess's `read` step): a real SQL predicate over the
    /// projected `status` ordinal, overriding the provided empty default.
    async fn open_by_session(&self, session_id: SessionId) -> Result<Vec<CartRow>, DomainError> {
        let sql = format!(
            "SELECT {} FROM cart WHERE session_id = $1 AND status = $2 ORDER BY updated_at DESC",
            cart_store::COLUMNS
        );
        let rows = sqlx::query(&sql)
            .bind(session_id.0)
            .bind(CartStatus::OPEN.to_ord())
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        rows.iter().map(cart_store::decode).collect()
    }
}
