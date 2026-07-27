//! The `customercreditbalance` table ↔ [`CustomerCreditBalanceRow`] mapping, both directions — shared
//! by the read repository (decode) and the projection worker (load current state + upsert the folded
//! row). #158, Part B of #207; `projector: app` (ADR-0040).
//!
//! Column conventions: `customer_id` is the UUID pk; `balance_cents` is a BIGINT (the `MoneyCents`
//! newtype over i64); `currency` is TEXT (the `CurrencyCode` newtype); `created_at`/`updated_at` are
//! the implicit projection timestamps.

use application::queries::CustomerCreditBalanceRow;
use domain::generated::scalars::{CurrencyCode, CustomerId, MoneyCents};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use super::db_err;

/// The full column list, in `CustomerCreditBalanceRow` field order — keep SELECTs and the upsert in sync.
pub(crate) const COLUMNS: &str = "customer_id, balance_cents, currency, created_at, updated_at";

/// Decode one `customercreditbalance` row into the generated read-model DTO.
pub(crate) fn decode(row: &PgRow) -> Result<CustomerCreditBalanceRow, DomainError> {
    Ok(CustomerCreditBalanceRow {
        customer_id: CustomerId(row.try_get("customer_id").map_err(db_err)?),
        balance_cents: MoneyCents(row.try_get::<i64, _>("balance_cents").map_err(db_err)?),
        currency: CurrencyCode(row.try_get::<String, _>("currency").map_err(db_err)?),
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}

/// Load the current projected balance for one customer, or `None` before their first CustomerCreditGranted.
pub async fn load(pool: &PgPool, id: CustomerId) -> Result<Option<CustomerCreditBalanceRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM customercreditbalance WHERE customer_id = $1");
    let row = sqlx::query(&sql).bind(id.0).fetch_optional(pool).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// Write the folded row: `INSERT … ON CONFLICT (customer_id) DO UPDATE` over the computed columns.
pub async fn upsert(pool: &PgPool, row: &CustomerCreditBalanceRow) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO customercreditbalance ({COLUMNS}) VALUES ($1,$2,$3,$4,$5) \
         ON CONFLICT (customer_id) DO UPDATE SET \
         balance_cents = EXCLUDED.balance_cents, \
         currency = EXCLUDED.currency, \
         created_at = EXCLUDED.created_at, \
         updated_at = EXCLUDED.updated_at"
    );
    sqlx::query(&sql)
        .bind(row.customer_id.0)
        .bind(row.balance_cents.0)
        .bind(row.currency.0.clone())
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}
