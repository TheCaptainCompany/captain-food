//! The `rider_restriction` table ↔ [`RiderRestrictionRow`] mapping — the restriction attribution
//! read model (#639 part C step 4-i; `projector: app`, ADR-0040;
//! `specs/database/tables/projection_tables.yaml#/RiderRestriction`). The source of `myStanding`.

use application::queries::{RiderRestrictionReadRepository, RiderRestrictionRow};
use async_trait::async_trait;
use domain::generated::scalars::RiderId;
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::PgPool;
use sqlx::Row;

use super::db_err;
use super::enum_sql::{opt_from_text, opt_to_text, EnumText};

/// The full column list, in `RiderRestrictionRow` field order — keep SELECTs and the upsert in sync.
pub(crate) const COLUMNS: &str =
    "rider_id, standing, ground, decided_at, effective_at, reinstated_at, created_at, updated_at";

pub(crate) fn decode(row: &PgRow) -> Result<RiderRestrictionRow, DomainError> {
    Ok(RiderRestrictionRow {
        rider_id: RiderId(row.try_get::<uuid::Uuid, _>("rider_id").map_err(db_err)?),
        standing: EnumText::from_text(&row.try_get::<String, _>("standing").map_err(db_err)?)?,
        ground: opt_from_text(row.try_get::<Option<String>, _>("ground").map_err(db_err)?)?,
        decided_at: row.try_get("decided_at").map_err(db_err)?,
        effective_at: row.try_get("effective_at").map_err(db_err)?,
        reinstated_at: row.try_get("reinstated_at").map_err(db_err)?,
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}

/// Load the current projected state for one rider, or `None` before its `RiderRegistered`.
pub async fn load(
    exec: impl sqlx::PgExecutor<'_>,
    id: RiderId,
) -> Result<Option<RiderRestrictionRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM rider_restriction WHERE rider_id = $1");
    let row = sqlx::query(&sql).bind(id.0).fetch_optional(exec).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// Write the folded row. Idempotent on re-projection (the `rider_store` shape): `created_at` is
/// absent from `DO UPDATE SET`.
pub async fn upsert(exec: impl sqlx::PgExecutor<'_>, row: &RiderRestrictionRow) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO rider_restriction ({COLUMNS}) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (rider_id) DO UPDATE SET \
           standing = EXCLUDED.standing, \
           ground = EXCLUDED.ground, \
           decided_at = EXCLUDED.decided_at, \
           effective_at = EXCLUDED.effective_at, \
           reinstated_at = EXCLUDED.reinstated_at, \
           updated_at = EXCLUDED.updated_at"
    );
    sqlx::query(&sql)
        .bind(row.rider_id.0)
        .bind(row.standing.to_text())
        .bind(opt_to_text(&row.ground))
        .bind(row.decided_at)
        .bind(row.effective_at)
        .bind(row.reinstated_at)
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(exec)
        .await
        .map(|_| ())
        .map_err(db_err)
}

/// Postgres read adapter — the `myStanding` resolver's port.
pub struct PgRiderRestrictionRepository {
    pool: PgPool,
}

impl PgRiderRestrictionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RiderRestrictionReadRepository for PgRiderRestrictionRepository {
    async fn by_rider_id(&self, rider_id: RiderId) -> Result<Option<RiderRestrictionRow>, DomainError> {
        load(&self.pool, rider_id).await
    }
}
