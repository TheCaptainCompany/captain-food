//! sqlx read-model repository over the materialized `restaurant` projection table (ADR-0040). Backs the
//! `restaurants` / `restaurant` GraphQL queries via `application::queries::RestaurantReadRepository`,
//! and the write-side existence check via `application::ports::RestaurantRepository`.

use application::ports::RestaurantRepository;
use application::queries::{RestaurantFilter, RestaurantReadRepository, RestaurantRow};
use async_trait::async_trait;
use domain::generated::scalars::{
    OrderAcceptanceMode, RestaurantAccountId, RestaurantListingStatus, RestaurantStatus, Slug,
};
use domain::shared::errors::DomainError;
use domain::shared::identifiers::RestaurantId;
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::db_err;
use super::enum_sql::EnumOrd;
use super::restaurant_store;

/// Postgres adapter for the Restaurant read model.
pub struct PgRestaurantRepository {
    pool: PgPool,
}

impl PgRestaurantRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Resolve a listing by one of its `external_identifiers` entries (JSONB containment on the
    /// projection). NOT part of the query/port surface: integration synchronizers use it to find the
    /// aggregate that already represents an external record — production predates today's
    /// deterministic id derivations, so the projection, not the derivation, is the source of truth
    /// for "is this SIRET/place already registered, and under which id".
    pub async fn by_external_identifier(
        &self,
        key: &str,
        value: &str,
    ) -> Result<Option<RestaurantRow>, DomainError> {
        let sql = format!(
            "SELECT {} FROM restaurant WHERE external_identifiers @> $1 LIMIT 1",
            restaurant_store::COLUMNS
        );
        let needle = serde_json::json!([{ "key": key, "value": value }]);
        let row = sqlx::query(&sql)
            .bind(needle)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        row.as_ref().map(restaurant_store::decode).transpose()
    }
}

#[async_trait]
impl RestaurantRepository for PgRestaurantRepository {
    async fn exists(&self, id: RestaurantId) -> Result<bool, DomainError> {
        let found = sqlx::query("SELECT 1 FROM restaurant WHERE restaurant_id = $1")
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(found.is_some())
    }
}

#[async_trait]
impl RestaurantReadRepository for PgRestaurantRepository {
    /// Discovery list, newest-first. `search` matches display_name/slug (ILIKE substring; wildcards in
    /// the input are not escaped — acceptable for V0 discovery). `orderable_only` applies the spec's
    /// orderable definition: listing ACTIVE_PARTNER + status ACTIVE + acceptance ≠ PAUSED.
    async fn list(&self, filter: RestaurantFilter) -> Result<Vec<RestaurantRow>, DomainError> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
            "SELECT {} FROM restaurant WHERE TRUE",
            restaurant_store::COLUMNS
        ));
        if let Some(search) = filter.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let pattern = format!("%{search}%");
            qb.push(" AND (display_name ILIKE ")
                .push_bind(pattern.clone())
                .push(" OR slug ILIKE ")
                .push_bind(pattern)
                .push(")");
        }
        if filter.orderable_only == Some(true) {
            qb.push(" AND listing_status = ")
                .push_bind(RestaurantListingStatus::ACTIVE_PARTNER.to_ord())
                .push(" AND status = ")
                .push_bind(RestaurantStatus::ACTIVE.to_ord())
                .push(" AND order_acceptance <> ")
                .push_bind(OrderAcceptanceMode::PAUSED.to_ord());
        }
        qb.push(" ORDER BY created_at DESC");
        let rows = qb.build().fetch_all(&self.pool).await.map_err(db_err)?;
        rows.iter().map(restaurant_store::decode).collect()
    }

    async fn by_slug(&self, slug: Slug) -> Result<Option<RestaurantRow>, DomainError> {
        let sql = format!(
            "SELECT {} FROM restaurant WHERE slug = $1",
            restaurant_store::COLUMNS
        );
        let row = sqlx::query(&sql)
            .bind(slug.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        row.as_ref().map(restaurant_store::decode).transpose()
    }

    async fn by_id(&self, id: RestaurantId) -> Result<Option<RestaurantRow>, DomainError> {
        let sql = format!(
            "SELECT {} FROM restaurant WHERE restaurant_id = $1",
            restaurant_store::COLUMNS
        );
        let row = sqlx::query(&sql)
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        row.as_ref().map(restaurant_store::decode).transpose()
    }

    /// All locations under an account (back-office `restaurantLocationsByAccount`), newest-first —
    /// overrides the provided in-memory default with an SQL predicate.
    async fn by_account(
        &self,
        account_id: RestaurantAccountId,
    ) -> Result<Vec<RestaurantRow>, DomainError> {
        let sql = format!(
            "SELECT {} FROM restaurant WHERE restaurant_account_id = $1 ORDER BY created_at DESC",
            restaurant_store::COLUMNS
        );
        let rows = sqlx::query(&sql)
            .bind(account_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        rows.iter().map(restaurant_store::decode).collect()
    }
}
