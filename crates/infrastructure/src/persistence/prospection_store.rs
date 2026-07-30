//! The 8-column `prospectionpipeline` table ↔ [`ProspectionPipelineRow`] mapping, both directions —
//! shared by the read repository (decode) and the projection worker (load current state + upsert the
//! folded row).
//!
//! Column conventions (ADR-20260728/0040): `pipeline_status` is a TEXT value (see
//! [`crate::persistence::enum_sql`]); `score` is an INTEGER column widened into the
//! `ProspectionScore(i64)` newtype; `contacts_count` is an INTEGER column widened to `i64` in the row.

use application::queries::ProspectionPipelineRow;
use domain::generated::scalars::{ProspectionScore, RestaurantId};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use super::db_err;
use super::enum_sql::EnumText;

/// The full column list, in `ProspectionPipelineRow` field order — keep SELECTs and the upsert in sync
/// with it.
pub(crate) const COLUMNS: &str = "restaurant_id, score, pipeline_status, contacts_count, \
     last_contacted_at, replied_at, created_at, updated_at";

/// Decode one `prospectionpipeline` row into the generated read-model DTO.
pub(crate) fn decode(row: &PgRow) -> Result<ProspectionPipelineRow, DomainError> {
    Ok(ProspectionPipelineRow {
        restaurant_id: RestaurantId(row.try_get("restaurant_id").map_err(db_err)?),
        score: ProspectionScore(i64::from(row.try_get::<i32, _>("score").map_err(db_err)?)),
        pipeline_status: EnumText::from_text(&row.try_get::<String, _>("pipeline_status").map_err(db_err)?)?,
        contacts_count: i64::from(row.try_get::<i32, _>("contacts_count").map_err(db_err)?),
        last_contacted_at: row.try_get("last_contacted_at").map_err(db_err)?,
        replied_at: row.try_get("replied_at").map_err(db_err)?,
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}

/// Load the current projected state for one prospect, or `None` before its creation event.
pub async fn load(pool: &PgPool, id: RestaurantId) -> Result<Option<ProspectionPipelineRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM prospectionpipeline WHERE restaurant_id = $1");
    let row = sqlx::query(&sql).bind(id.0).fetch_optional(pool).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// Write the folded row: `INSERT … ON CONFLICT (restaurant_id) DO UPDATE` over all 8 columns.
pub async fn upsert(pool: &PgPool, row: &ProspectionPipelineRow) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO prospectionpipeline ({COLUMNS}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) \
         ON CONFLICT (restaurant_id) DO UPDATE SET \
         score = EXCLUDED.score, \
         pipeline_status = EXCLUDED.pipeline_status, \
         contacts_count = EXCLUDED.contacts_count, \
         last_contacted_at = EXCLUDED.last_contacted_at, \
         replied_at = EXCLUDED.replied_at, \
         created_at = EXCLUDED.created_at, \
         updated_at = EXCLUDED.updated_at"
    );
    sqlx::query(&sql)
        .bind(row.restaurant_id.0)
        .bind(row.score.0 as i32)
        .bind(row.pipeline_status.to_text())
        .bind(row.contacts_count as i32)
        .bind(row.last_contacted_at)
        .bind(row.replied_at)
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}
