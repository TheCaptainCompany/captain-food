//! The `rider_roster` table <-> [`RiderRosterRow`] mapping — the admin's rider roster read model
//! (#639 part C step 4-iii-A; `projector: app`, ADR-0040;
//! `specs/database/tables/projection_tables.yaml#/RiderRoster`). The source of `riders`/`rider`.

use application::queries::{RiderRosterReadRepository, RiderRosterRow};
use async_trait::async_trait;
use domain::generated::scalars::RiderId;
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::PgPool;
use sqlx::Row;

use super::db_err;
use super::enum_sql::{opt_from_text, opt_to_text, EnumText};

/// The full column list, in `RiderRosterRow` field order — keep SELECTs and the upsert in sync.
pub(crate) const COLUMNS: &str = "rider_id, display_name, phone, status, standing, ground, \
     decided_at, effective_at, reinstated_at, created_at, updated_at";

pub(crate) fn decode(row: &PgRow) -> Result<RiderRosterRow, DomainError> {
    Ok(RiderRosterRow {
        rider_id: RiderId(row.try_get::<uuid::Uuid, _>("rider_id").map_err(db_err)?),
        display_name: row.try_get("display_name").map_err(db_err)?,
        phone: domain::generated::scalars::PhoneNumber(row.try_get::<String, _>("phone").map_err(db_err)?),
        status: EnumText::from_text(&row.try_get::<String, _>("status").map_err(db_err)?)?,
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
) -> Result<Option<RiderRosterRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM rider_roster WHERE rider_id = $1");
    let row = sqlx::query(&sql).bind(id.0).fetch_optional(exec).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// Write the folded row. Idempotent on re-projection (the `rider_restriction_store` shape):
/// `created_at` is absent from `DO UPDATE SET`.
pub async fn upsert(exec: impl sqlx::PgExecutor<'_>, row: &RiderRosterRow) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO rider_roster ({COLUMNS}) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
         ON CONFLICT (rider_id) DO UPDATE SET \
           display_name = EXCLUDED.display_name, \
           phone = EXCLUDED.phone, \
           status = EXCLUDED.status, \
           standing = EXCLUDED.standing, \
           ground = EXCLUDED.ground, \
           decided_at = EXCLUDED.decided_at, \
           effective_at = EXCLUDED.effective_at, \
           reinstated_at = EXCLUDED.reinstated_at, \
           updated_at = EXCLUDED.updated_at"
    );
    sqlx::query(&sql)
        .bind(row.rider_id.0)
        .bind(row.display_name.clone())
        .bind(row.phone.0.clone())
        .bind(row.status.to_text())
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

/// Every row, `ORDER BY display_name, rider_id` (the `(display_name, rider_id)` index, #113) — the
/// resolver's own read of the whole roster to fold the held-first/restricted-first contract order
/// before paging (ADR-20260904-152807 §2/§4).
pub async fn all(exec: impl sqlx::PgExecutor<'_>) -> Result<Vec<RiderRosterRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM rider_roster ORDER BY display_name, rider_id");
    let rows = sqlx::query(&sql).fetch_all(exec).await.map_err(db_err)?;
    rows.iter().map(decode).collect()
}

/// Postgres read adapter — the `riders`/`rider` resolvers' port.
pub struct PgRiderRosterRepository {
    pool: PgPool,
}

impl PgRiderRosterRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RiderRosterReadRepository for PgRiderRosterRepository {
    async fn all(&self) -> Result<Vec<RiderRosterRow>, DomainError> {
        all(&self.pool).await
    }
    async fn by_id(&self, rider_id: RiderId) -> Result<Option<RiderRosterRow>, DomainError> {
        load(&self.pool, rider_id).await
    }
}
