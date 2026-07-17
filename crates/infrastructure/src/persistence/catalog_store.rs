//! The 7-column `catalog` table ↔ [`CatalogRow`] mapping, both directions — shared by the read
//! repository (decode) and the projection worker (load current state + upsert the folded row).
//!
//! Column conventions (ADR-0037/0040): `tree` is a NOT NULL jsonb column carrying `serde_json::Value`;
//! scalar newtypes bind via their inner `.0`.

use application::queries::CatalogRow;
use domain::generated::scalars::{CatalogId, CatalogName, RestaurantId, Slug};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use super::db_err;

/// The full column list, in `CatalogRow` field order — keep SELECTs and the upsert in sync with it.
pub(crate) const COLUMNS: &str =
    "catalog_id, restaurant_id, slug, name, tree, created_at, updated_at";

/// Decode one `catalog` row into the generated read-model DTO.
pub(crate) fn decode(row: &PgRow) -> Result<CatalogRow, DomainError> {
    Ok(CatalogRow {
        catalog_id: CatalogId(row.try_get("catalog_id").map_err(db_err)?),
        restaurant_id: RestaurantId(row.try_get("restaurant_id").map_err(db_err)?),
        slug: Slug(row.try_get("slug").map_err(db_err)?),
        name: CatalogName(row.try_get("name").map_err(db_err)?),
        tree: row.try_get("tree").map_err(db_err)?,
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}

/// Load the current projected state for one catalog, or `None` before its creation event.
pub async fn load(pool: &PgPool, id: CatalogId) -> Result<Option<CatalogRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM catalog WHERE catalog_id = $1");
    let row = sqlx::query(&sql).bind(id.0).fetch_optional(pool).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// Write the folded row: `INSERT … ON CONFLICT (catalog_id) DO UPDATE` over all 7 columns.
pub async fn upsert(pool: &PgPool, row: &CatalogRow) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO catalog ({COLUMNS}) VALUES ($1,$2,$3,$4,$5,$6,$7) \
         ON CONFLICT (catalog_id) DO UPDATE SET \
         restaurant_id = EXCLUDED.restaurant_id, \
         slug = EXCLUDED.slug, \
         name = EXCLUDED.name, \
         tree = EXCLUDED.tree, \
         created_at = EXCLUDED.created_at, \
         updated_at = EXCLUDED.updated_at"
    );
    sqlx::query(&sql)
        .bind(row.catalog_id.0)
        .bind(row.restaurant_id.0)
        .bind(row.slug.0.clone())
        .bind(row.name.0.clone())
        .bind(row.tree.clone())
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}
