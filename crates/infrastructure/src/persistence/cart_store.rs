//! The 8-column `cart` table ↔ [`CartRow`] mapping, both directions — shared by the read repository
//! (decode) and the projection worker (load current state + upsert the folded row).
//!
//! The row is a MONEY-FREE pure fold (ADR-20260810-112836): identity, status and the repricing
//! inputs (`lines` jsonb: `[{ cart_line_id, offer_id, quantity, selected_option_ids }]`) — no price
//! columns exist; the read side prices via `application::pricing::price_cart`.
//!
//! Column conventions (ADR-20260728/0040): `status` is a TEXT value (see
//! [`crate::persistence::enum_sql`]); `lines` is a jsonb column carrying `serde_json::Value`; the
//! scalar newtypes bind via `.0`.

use application::queries::CartRow;
use domain::generated::scalars::{CartId, CustomerId, RestaurantId, SessionId};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::Row;

use super::db_err;
use super::enum_sql::EnumText;

/// The full column list, in `CartRow` field order — keep SELECTs and the upsert in sync with it.
pub(crate) const COLUMNS: &str =
    "cart_id, restaurant_id, session_id, customer_id, status, lines, created_at, updated_at";

/// Decode one `cart` row into the generated read-model DTO.
pub(crate) fn decode(row: &PgRow) -> Result<CartRow, DomainError> {
    Ok(CartRow {
        cart_id: CartId(row.try_get("cart_id").map_err(db_err)?),
        restaurant_id: RestaurantId(row.try_get("restaurant_id").map_err(db_err)?),
        session_id: SessionId(row.try_get("session_id").map_err(db_err)?),
        customer_id: row
            .try_get::<Option<uuid::Uuid>, _>("customer_id")
            .map_err(db_err)?
            .map(CustomerId),
        status: EnumText::from_text(&row.try_get::<String, _>("status").map_err(db_err)?)?,
        lines: row.try_get("lines").map_err(db_err)?,
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}

/// Load the current projected state for one cart, or `None` before its creation event.
pub async fn load(exec: impl sqlx::PgExecutor<'_>, id: CartId) -> Result<Option<CartRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM cart WHERE cart_id = $1");
    let row = sqlx::query(&sql).bind(id.0).fetch_optional(exec).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// Write the folded row: `INSERT … ON CONFLICT (cart_id) DO UPDATE` over all columns.
pub async fn upsert(exec: impl sqlx::PgExecutor<'_>, row: &CartRow) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO cart ({COLUMNS}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) \
         ON CONFLICT (cart_id) DO UPDATE SET \
         restaurant_id = EXCLUDED.restaurant_id, \
         session_id = EXCLUDED.session_id, \
         customer_id = EXCLUDED.customer_id, \
         status = EXCLUDED.status, \
         lines = EXCLUDED.lines, \
         created_at = EXCLUDED.created_at, \
         updated_at = EXCLUDED.updated_at"
    );
    sqlx::query(&sql)
        .bind(row.cart_id.0)
        .bind(row.restaurant_id.0)
        .bind(row.session_id.0)
        .bind(row.customer_id.as_ref().map(|v| v.0))
        .bind(row.status.to_text())
        .bind(row.lines.clone())
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(exec)
        .await
        .map_err(db_err)?;
    Ok(())
}
