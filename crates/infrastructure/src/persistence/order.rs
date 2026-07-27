//! sqlx read-model repository over the materialized `ordertracking` projection table (ADR-0040). Backs
//! the `orders` / `order` GraphQL queries via `application::queries::OrderReadRepository` — the single
//! canonical Order read model (customer history, back-office queue and tracking).

use application::queries::{OrderFilter, OrderReadRepository, OrderTrackingRow};
use async_trait::async_trait;
use domain::generated::scalars::OrderId;
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Postgres, QueryBuilder};
use application::queries::ReadScope;
use domain::generated::scalars::ScopeType;
use crate::persistence::scope_membership_store::{self, scope_predicate, ScopePredicate};

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
    async fn list(
        &self,
        filter: OrderFilter,
        scope: &ReadScope,
    ) -> Result<Vec<OrderTrackingRow>, DomainError> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
            "SELECT {} FROM ordertracking WHERE TRUE",
            order_tracking_store::COLUMNS
        ));
        // The authorization predicate goes in the WHERE, ahead of the caller's own filters (#144):
        // rows outside the scope never enter the result set, so this cannot be forgotten downstream
        // and leaks no existence. ADMIN adds nothing; PUBLIC can see no orders at all.
        match scope_predicate(scope) {
            ScopePredicate::All => {}
            ScopePredicate::None => return Ok(Vec::new()),
            ScopePredicate::Member(principal_type, principal_id) => {
                qb.push(
                    " AND EXISTS (SELECT 1 FROM scopemembership m                        WHERE m.scope_type = ",
                )
                .push_bind(ScopeType::ORDER.to_ord())
                .push(" AND m.scope_id = ordertracking.order_id AND m.principal_type = ")
                .push_bind(principal_type)
                .push(" AND m.principal_id = ")
                .push_bind(principal_id)
                .push(")");
            }
        }
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

    async fn by_id(
        &self,
        id: OrderId,
        scope: &ReadScope,
    ) -> Result<Option<OrderTrackingRow>, DomainError> {
        // A by-id read is the one shape that CAN be a membership check rather than a filter, because
        // the instance is named. Out-of-scope resolves to None, not to a distinct error — the read
        // side must not become an existence oracle.
        match scope_predicate(scope) {
            ScopePredicate::All => {}
            ScopePredicate::None => return Ok(None),
            ScopePredicate::Member(principal_type, principal_id) => {
                let member = scope_membership_store::is_member(
                    &self.pool,
                    ScopeType::ORDER,
                    id.0,
                    EnumOrd::from_ord(principal_type)?,
                    principal_id,
                )
                .await?;
                if !member {
                    return Ok(None);
                }
            }
        }
        order_tracking_store::load(&self.pool, id).await
    }
}
