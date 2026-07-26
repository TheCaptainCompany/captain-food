//! The `rider` table ↔ [`RiderRow`] mapping (#144) — the RIDER identity bridge.
//!
//! Its reason to exist is one question, asked once per request at the edge: **which rider is this
//! token?** `auth_ref` is the Supabase subject; `rider_id` is the domain id. They are deliberately
//! different values (the same split as `Customer`), because welding a domain id to the auth provider
//! would invalidate every id already written into the immutable log if the provider ever changed.

use application::queries::{RiderReadRepository, RiderRow};
use domain::generated::scalars::{ExternalReference, PhoneNumber, RiderId};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use super::db_err;
use super::enum_sql::EnumOrd;

pub(crate) const COLUMNS: &str =
    "rider_id, auth_ref, display_name, phone, status, created_at, updated_at";

pub(crate) fn decode(row: &PgRow) -> Result<RiderRow, DomainError> {
    Ok(RiderRow {
        rider_id: RiderId(row.try_get("rider_id").map_err(db_err)?),
        auth_ref: ExternalReference(row.try_get("auth_ref").map_err(db_err)?),
        display_name: row.try_get("display_name").map_err(db_err)?,
        phone: PhoneNumber(row.try_get("phone").map_err(db_err)?),
        status: EnumOrd::from_ord(row.try_get::<i32, _>("status").map_err(db_err)?)?,
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}

pub async fn load(pool: &PgPool, id: RiderId) -> Result<Option<RiderRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM rider WHERE rider_id = $1");
    let row = sqlx::query(&sql).bind(id.0).fetch_optional(pool).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// The bridge lookup: auth subject → rider. `None` means no identity, which the caller must treat as
/// a DENIAL — never as "unrestricted".
pub async fn by_auth_ref(
    pool: &PgPool,
    auth_ref: &str,
) -> Result<Option<RiderRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM rider WHERE auth_ref = $1");
    let row = sqlx::query(&sql).bind(auth_ref).fetch_optional(pool).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

pub async fn upsert(pool: &PgPool, row: &RiderRow) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO rider ({COLUMNS}) VALUES ($1,$2,$3,$4,$5,$6,$7) \
         ON CONFLICT (rider_id) DO UPDATE SET \
         auth_ref = EXCLUDED.auth_ref, \
         display_name = EXCLUDED.display_name, \
         phone = EXCLUDED.phone, \
         status = EXCLUDED.status, \
         created_at = EXCLUDED.created_at, \
         updated_at = EXCLUDED.updated_at"
    );
    sqlx::query(&sql)
        .bind(row.rider_id.0)
        .bind(&row.auth_ref.0)
        .bind(&row.display_name)
        .bind(&row.phone.0)
        .bind(row.status.to_ord())
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// The [`RiderReadRepository`] adapter — the rider identity bridge behind the port.
pub struct PgRiderRepository {
    pool: PgPool,
}

impl PgRiderRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl RiderReadRepository for PgRiderRepository {
    async fn by_auth_ref(
        &self,
        auth_ref: ExternalReference,
    ) -> Result<Option<RiderRow>, DomainError> {
        by_auth_ref(&self.pool, &auth_ref.0).await
    }
}
