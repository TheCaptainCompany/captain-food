//! sqlx read-model repository over the materialized `orderconversation` projection table (ADR-0040;
//! #131, epic #129). Backs the `orderConversation` / `orderConversationInternalNotes` GraphQL queries
//! via `application::queries::OrderConversationReadRepository` — both read the one per-order row (the
//! PUBLIC/INTERNAL split is a column split, not a row split).

use application::queries::{OrderConversationReadRepository, OrderConversationRow};
use async_trait::async_trait;
use domain::generated::scalars::OrderId;
use domain::shared::errors::DomainError;
use sqlx::PgPool;

use super::order_conversation_store;

/// Postgres adapter for the OrderConversation read model.
pub struct PgOrderConversationRepository {
    pool: PgPool,
}

impl PgOrderConversationRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OrderConversationReadRepository for PgOrderConversationRepository {
    async fn by_order(&self, order_id: OrderId) -> Result<Option<OrderConversationRow>, DomainError> {
        order_conversation_store::load(&self.pool, order_id).await
    }
}
