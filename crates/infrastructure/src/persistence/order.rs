//! sqlx read-model repository over the materialized `ordertracking` projection table (ADR-0040). Backs
//! the `orders` / `order` GraphQL queries via `application::queries::OrderReadRepository` — the single
//! canonical Order read model (customer history, back-office queue and tracking).

use application::queries::{OrderFilter, OrderReadRepository, OrderTrackingRow};
use async_trait::async_trait;
use domain::generated::scalars::OrderId;
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::db_err;
use super::enum_sql::EnumOrd;
use super::order_tracking_store;

/// Postgres adapter for the OrderTracking read model.
pub struct PgOrderRepository {
    pool: PgPool,
}

impl PgOrderRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OrderReadRepository for PgOrderRepository {
    /// Orders, most recently placed first. `customer_id` scopes the customer's history,
    /// `restaurant_id`/`status` the back-office queue (`status` bound as its INTEGER ordinal, ADR-0037).
    async fn list(&self, filter: OrderFilter) -> Result<Vec<OrderTrackingRow>, DomainError> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
            "SELECT {} FROM ordertracking WHERE TRUE",
            order_tracking_store::COLUMNS
        ));
        if let Some(customer_id) = filter.customer_id {
            qb.push(" AND customer_id = ").push_bind(customer_id.0);
        }
        if let Some(restaurant_id) = filter.restaurant_id {
            qb.push(" AND restaurant_id = ").push_bind(restaurant_id.0);
        }
        if let Some(status) = filter.status {
            qb.push(" AND status = ").push_bind(status.to_ord());
        }
        qb.push(" ORDER BY placed_at DESC");
        let rows = qb.build().fetch_all(&self.pool).await.map_err(db_err)?;
        rows.iter().map(order_tracking_store::decode).collect()
    }

    async fn by_id(&self, id: OrderId) -> Result<Option<OrderTrackingRow>, DomainError> {
        order_tracking_store::load(&self.pool, id).await
    }
}
