//! sqlx read-model repository over the materialized `ordertracking` projection table (ADR-0040). Backs
//! the `orders` / `order` GraphQL queries via `application::queries::OrderReadRepository` — the single
//! canonical Order read model (customer history, back-office queue and tracking).

use application::queries::{OrderFilter, OrderReadRepository, OrderTrackingRow, ReadScope};
use async_trait::async_trait;
use domain::generated::scalars::{OrderId, ScopeType};
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::db_err;
use super::enum_sql::EnumText;
use super::order_tracking_store;
use super::scope_membership_store::{self, scope_predicate, ScopePredicate};

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
    /// `restaurant_id`/`status` the back-office queue (`status` bound as its TEXT value, ADR-20260728).
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
        // and leaks no existence. ADMIN/SYSTEM add nothing; PUBLIC can see no orders at all. There is
        // no per-row decision to record here — a list "denial" is structurally invisible (rows are
        // simply absent), which is why denial telemetry lives on the by-id/subscription paths only.
        match scope_predicate(scope) {
            ScopePredicate::All => {}
            ScopePredicate::None => return Ok(Vec::new()),
            ScopePredicate::Member(principal_type, principal_id) => {
                qb.push(
                    " AND EXISTS (SELECT 1 FROM scopemembership m \
                       WHERE m.scope_type = ",
                )
                .push_bind(ScopeType::ORDER.to_text())
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
            qb.push(" AND status = ").push_bind(status.to_text());
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
        // side must not become an existence oracle. Telemetry is therefore the ONLY place "denied"
        // and "not found" are distinguishable (specs/observability.yaml#read-authorization), so the
        // denial is recorded here, before it collapses to None.
        match scope_predicate(scope) {
            ScopePredicate::All => {}
            ScopePredicate::None => {
                telemetry::meters::read_authorization::checked("ORDER", "PUBLIC", false, 0.0);
                return Ok(None);
            }
            ScopePredicate::Member(principal_type, principal_id) => {
                let span = telemetry::spans::auth_scope_membership("ORDER", principal_type);
                let started = std::time::Instant::now();
                let member = scope_membership_store::is_member(
                    &self.pool,
                    ScopeType::ORDER,
                    id.0,
                    EnumText::from_text(principal_type)?,
                    principal_id,
                )
                .await?;
                let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                telemetry::spans::record_authorized(&span, member);
                telemetry::meters::read_authorization::checked(
                    "ORDER",
                    principal_type,
                    member,
                    elapsed_ms,
                );
                if !member {
                    return Ok(None);
                }
            }
        }
        order_tracking_store::load(&self.pool, id).await
    }
}
