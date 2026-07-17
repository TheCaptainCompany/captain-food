//! sqlx read-model repository over the materialized `catalog` projection table (ADR-0040). Backs the
//! public `catalog` / `categories` GraphQL queries via `application::queries::CatalogReadRepository`.

use application::queries::{CatalogReadRepository, CatalogRow};
use async_trait::async_trait;
use domain::shared::errors::DomainError;
use domain::shared::identifiers::RestaurantId;
use sqlx::PgPool;

use super::catalog_store;
use super::db_err;

/// Postgres adapter for the Catalog read model.
pub struct PgCatalogRepository {
    pool: PgPool,
}

impl PgCatalogRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CatalogReadRepository for PgCatalogRepository {
    /// The restaurant's catalog — newest first when several exist (api.yaml exposes one per restaurant).
    async fn by_restaurant(&self, restaurant_id: RestaurantId) -> Result<Option<CatalogRow>, DomainError> {
        let sql = format!(
            "SELECT {} FROM catalog WHERE restaurant_id = $1 ORDER BY created_at DESC LIMIT 1",
            catalog_store::COLUMNS
        );
        let row = sqlx::query(&sql)
            .bind(restaurant_id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        row.as_ref().map(catalog_store::decode).transpose()
    }
}
