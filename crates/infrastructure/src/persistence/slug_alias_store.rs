//! The `slugalias` table ↔ [`SlugAliasRow`] mapping (ADR-20260728-011344; `projector: app`,
//! ADR-0040) — plus the one lookup `hosts.rs` performs on a host-resolution miss.
//!
//! Unusual key: this projection is keyed by the **superseded slug**, not by an aggregate id. One row per
//! rename, so a restaurant renamed N times leaves N rows on the same Restaurant stream. The projection
//! worker therefore resolves this row's key from the event payload's `previousSlug`.

use application::queries::SlugAliasRow;
use domain::generated::scalars::{RestaurantId, Slug};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use super::db_err;

/// The full column list, in `SlugAliasRow` field order — keep SELECTs and the upsert in sync.
pub(crate) const COLUMNS: &str =
    "previous_slug, restaurant_id, current_slug, created_at, updated_at";

pub(crate) fn decode(row: &PgRow) -> Result<SlugAliasRow, DomainError> {
    Ok(SlugAliasRow {
        previous_slug: Slug(row.try_get::<String, _>("previous_slug").map_err(db_err)?),
        restaurant_id: RestaurantId(row.try_get::<uuid::Uuid, _>("restaurant_id").map_err(db_err)?),
        current_slug: Slug(row.try_get::<String, _>("current_slug").map_err(db_err)?),
        created_at: row.try_get::<chrono::DateTime<chrono::Utc>, _>("created_at").map_err(db_err)?,
        updated_at: row.try_get::<chrono::DateTime<chrono::Utc>, _>("updated_at").map_err(db_err)?,
    })
}

/// Load the alias row for one superseded label, or `None` if that label was never a storefront address.
pub async fn load(pool: &PgPool, previous_slug: Slug) -> Result<Option<SlugAliasRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM slugalias WHERE previous_slug = $1");
    let row = sqlx::query(&sql).bind(previous_slug.0).fetch_optional(pool).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// Write the folded row. Idempotent on re-projection: replaying the same rename rewrites identical
/// values.
pub async fn upsert(pool: &PgPool, row: &SlugAliasRow) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO slugalias ({COLUMNS}) VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (previous_slug) DO UPDATE SET \
           restaurant_id = EXCLUDED.restaurant_id, \
           current_slug = EXCLUDED.current_slug, \
           updated_at = EXCLUDED.updated_at"
    );
    sqlx::query(&sql)
        .bind(row.previous_slug.0.clone())
        .bind(row.restaurant_id.0)
        .bind(row.current_slug.0.clone())
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(db_err)
}
