//! The 11-column `cart` table ↔ [`CartRow`] mapping, both directions — shared by the read repository
//! (decode) and the projection worker (load current state + upsert the folded row).
//!
//! Column conventions (ADR-0037/0040): `status` is an INTEGER ordinal (see
//! [`crate::persistence::enum_sql`]); `lines`/`estimated_breakdown`/`uber_comparison` are jsonb columns
//! carrying `serde_json::Value`; `total_amount_cents` is a BIGINT bound via the `MoneyCents` newtype's
//! inner `.0`; the other scalar newtypes bind via `.0` too.

use application::queries::CartRow;
use domain::generated::scalars::{CartId, CurrencyCode, CustomerId, MoneyCents, RestaurantId};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use super::db_err;
use super::enum_sql::EnumOrd;

/// The full column list, in `CartRow` field order — keep SELECTs and the upsert in sync with it.
pub(crate) const COLUMNS: &str = "cart_id, restaurant_id, customer_id, status, lines, \
     total_amount_cents, currency, estimated_breakdown, uber_comparison, created_at, updated_at";

/// Normalize a nullable jsonb: a JSON `null` in the column (or in the row) means "no value".
fn opt_json(v: Option<serde_json::Value>) -> Option<serde_json::Value> {
    v.filter(|j| !j.is_null())
}

/// Decode one `cart` row into the generated read-model DTO.
pub(crate) fn decode(row: &PgRow) -> Result<CartRow, DomainError> {
    Ok(CartRow {
        cart_id: CartId(row.try_get("cart_id").map_err(db_err)?),
        restaurant_id: RestaurantId(row.try_get("restaurant_id").map_err(db_err)?),
        customer_id: row
            .try_get::<Option<uuid::Uuid>, _>("customer_id")
            .map_err(db_err)?
            .map(CustomerId),
        status: EnumOrd::from_ord(row.try_get::<i32, _>("status").map_err(db_err)?)?,
        lines: row.try_get("lines").map_err(db_err)?,
        total_amount_cents: MoneyCents(row.try_get("total_amount_cents").map_err(db_err)?),
        currency: CurrencyCode(row.try_get("currency").map_err(db_err)?),
        estimated_breakdown: opt_json(row.try_get("estimated_breakdown").map_err(db_err)?),
        uber_comparison: opt_json(row.try_get("uber_comparison").map_err(db_err)?),
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}

/// Load the current projected state for one cart, or `None` before its creation event.
pub async fn load(pool: &PgPool, id: CartId) -> Result<Option<CartRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM cart WHERE cart_id = $1");
    let row = sqlx::query(&sql).bind(id.0).fetch_optional(pool).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// Write the folded row: `INSERT … ON CONFLICT (cart_id) DO UPDATE` over all 11 columns.
pub async fn upsert(pool: &PgPool, row: &CartRow) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO cart ({COLUMNS}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) \
         ON CONFLICT (cart_id) DO UPDATE SET \
         restaurant_id = EXCLUDED.restaurant_id, \
         customer_id = EXCLUDED.customer_id, \
         status = EXCLUDED.status, \
         lines = EXCLUDED.lines, \
         total_amount_cents = EXCLUDED.total_amount_cents, \
         currency = EXCLUDED.currency, \
         estimated_breakdown = EXCLUDED.estimated_breakdown, \
         uber_comparison = EXCLUDED.uber_comparison, \
         created_at = EXCLUDED.created_at, \
         updated_at = EXCLUDED.updated_at"
    );
    sqlx::query(&sql)
        .bind(row.cart_id.0)
        .bind(row.restaurant_id.0)
        .bind(row.customer_id.as_ref().map(|v| v.0))
        .bind(row.status.to_ord())
        .bind(row.lines.clone())
        .bind(row.total_amount_cents.0)
        .bind(row.currency.0.clone())
        .bind(opt_json(row.estimated_breakdown.clone()))
        .bind(opt_json(row.uber_comparison.clone()))
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}
