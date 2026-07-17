//! The 37-column `ordertracking` table ↔ [`OrderTrackingRow`] mapping, both directions — shared by the
//! read repository (decode) and the projection worker (load current state + upsert the folded row).
//!
//! Column conventions (ADR-0037/0040): `status`/`service_type`/`uber_basis`/`rider_thumb`/
//! `delivery_status` are INTEGER ordinals (see [`crate::persistence::enum_sql`]); `items`/
//! `delivery_address`/`courier` are jsonb columns carrying `serde_json::Value`; the `*_cents` BIGINT
//! columns bind via the `MoneyCents` newtype's inner `.0`; `restaurant_stars` is an INTEGER column
//! widened into the `StarRating(i64)` newtype; `payment_status` is a plain TEXT column.

use application::queries::OrderTrackingRow;
use domain::generated::scalars::{
    CurrencyCode, CustomerId, ExternalReference, MoneyCents, OrderId, RatingComment, RestaurantId,
    StarRating,
};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use super::db_err;
use super::enum_sql::{opt_from_ord, opt_to_ord, EnumOrd};

/// The full column list, in `OrderTrackingRow` field order — keep SELECTs and the upsert in sync with it.
pub(crate) const COLUMNS: &str = "order_id, ref, restaurant_id, customer_id, status, service_type, \
     items, total_amount_cents, currency, articles_cents, delivery_cents, service_fee_cents, \
     restaurant_payout_cents, rider_payout_cents, captain_net_cents, uber_total_cents, \
     uber_restaurant_cents, uber_rider_cents, uber_platform_cents, uber_basis, delivery_address, \
     estimated_ready_at, placed_at, status_changed_at, payment_status, restaurant_stars, \
     rating_comment, rider_thumb, rider_tip_cents, restaurant_tip_cents, captain_tip_cents, rated_at, \
     delivery_status, courier, estimated_dropoff_at, created_at, updated_at";

/// Normalize a nullable jsonb: a JSON `null` in the column (or in the row) means "no value".
fn opt_json(v: Option<serde_json::Value>) -> Option<serde_json::Value> {
    v.filter(|j| !j.is_null())
}

fn opt_cents(v: Option<i64>) -> Option<MoneyCents> {
    v.map(MoneyCents)
}

/// Decode one `ordertracking` row into the generated read-model DTO.
pub(crate) fn decode(row: &PgRow) -> Result<OrderTrackingRow, DomainError> {
    Ok(OrderTrackingRow {
        order_id: OrderId(row.try_get("order_id").map_err(db_err)?),
        r#ref: ExternalReference(row.try_get("ref").map_err(db_err)?),
        restaurant_id: RestaurantId(row.try_get("restaurant_id").map_err(db_err)?),
        customer_id: row
            .try_get::<Option<uuid::Uuid>, _>("customer_id")
            .map_err(db_err)?
            .map(CustomerId),
        status: EnumOrd::from_ord(row.try_get::<i32, _>("status").map_err(db_err)?)?,
        service_type: EnumOrd::from_ord(row.try_get::<i32, _>("service_type").map_err(db_err)?)?,
        items: row.try_get("items").map_err(db_err)?,
        total_amount_cents: MoneyCents(row.try_get("total_amount_cents").map_err(db_err)?),
        currency: CurrencyCode(row.try_get("currency").map_err(db_err)?),
        articles_cents: MoneyCents(row.try_get("articles_cents").map_err(db_err)?),
        delivery_cents: MoneyCents(row.try_get("delivery_cents").map_err(db_err)?),
        service_fee_cents: MoneyCents(row.try_get("service_fee_cents").map_err(db_err)?),
        restaurant_payout_cents: MoneyCents(row.try_get("restaurant_payout_cents").map_err(db_err)?),
        rider_payout_cents: MoneyCents(row.try_get("rider_payout_cents").map_err(db_err)?),
        captain_net_cents: MoneyCents(row.try_get("captain_net_cents").map_err(db_err)?),
        uber_total_cents: opt_cents(row.try_get("uber_total_cents").map_err(db_err)?),
        uber_restaurant_cents: opt_cents(row.try_get("uber_restaurant_cents").map_err(db_err)?),
        uber_rider_cents: opt_cents(row.try_get("uber_rider_cents").map_err(db_err)?),
        uber_platform_cents: opt_cents(row.try_get("uber_platform_cents").map_err(db_err)?),
        uber_basis: opt_from_ord(row.try_get("uber_basis").map_err(db_err)?)?,
        delivery_address: opt_json(row.try_get("delivery_address").map_err(db_err)?),
        estimated_ready_at: row.try_get("estimated_ready_at").map_err(db_err)?,
        placed_at: row.try_get("placed_at").map_err(db_err)?,
        status_changed_at: row.try_get("status_changed_at").map_err(db_err)?,
        payment_status: row.try_get("payment_status").map_err(db_err)?,
        restaurant_stars: row
            .try_get::<Option<i32>, _>("restaurant_stars")
            .map_err(db_err)?
            .map(|v| StarRating(i64::from(v))),
        rating_comment: row
            .try_get::<Option<String>, _>("rating_comment")
            .map_err(db_err)?
            .map(RatingComment),
        rider_thumb: opt_from_ord(row.try_get("rider_thumb").map_err(db_err)?)?,
        rider_tip_cents: opt_cents(row.try_get("rider_tip_cents").map_err(db_err)?),
        restaurant_tip_cents: opt_cents(row.try_get("restaurant_tip_cents").map_err(db_err)?),
        captain_tip_cents: opt_cents(row.try_get("captain_tip_cents").map_err(db_err)?),
        rated_at: row.try_get("rated_at").map_err(db_err)?,
        delivery_status: opt_from_ord(row.try_get("delivery_status").map_err(db_err)?)?,
        courier: opt_json(row.try_get("courier").map_err(db_err)?),
        estimated_dropoff_at: row.try_get("estimated_dropoff_at").map_err(db_err)?,
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}

/// Load the current projected state for one order, or `None` before its creation event.
pub async fn load(pool: &PgPool, id: OrderId) -> Result<Option<OrderTrackingRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM ordertracking WHERE order_id = $1");
    let row = sqlx::query(&sql).bind(id.0).fetch_optional(pool).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// Write the folded row: `INSERT … ON CONFLICT (order_id) DO UPDATE` over all 37 columns.
pub async fn upsert(pool: &PgPool, row: &OrderTrackingRow) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO ordertracking ({COLUMNS}) VALUES \
         ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,\
          $25,$26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,$37) \
         ON CONFLICT (order_id) DO UPDATE SET \
         ref = EXCLUDED.ref, \
         restaurant_id = EXCLUDED.restaurant_id, \
         customer_id = EXCLUDED.customer_id, \
         status = EXCLUDED.status, \
         service_type = EXCLUDED.service_type, \
         items = EXCLUDED.items, \
         total_amount_cents = EXCLUDED.total_amount_cents, \
         currency = EXCLUDED.currency, \
         articles_cents = EXCLUDED.articles_cents, \
         delivery_cents = EXCLUDED.delivery_cents, \
         service_fee_cents = EXCLUDED.service_fee_cents, \
         restaurant_payout_cents = EXCLUDED.restaurant_payout_cents, \
         rider_payout_cents = EXCLUDED.rider_payout_cents, \
         captain_net_cents = EXCLUDED.captain_net_cents, \
         uber_total_cents = EXCLUDED.uber_total_cents, \
         uber_restaurant_cents = EXCLUDED.uber_restaurant_cents, \
         uber_rider_cents = EXCLUDED.uber_rider_cents, \
         uber_platform_cents = EXCLUDED.uber_platform_cents, \
         uber_basis = EXCLUDED.uber_basis, \
         delivery_address = EXCLUDED.delivery_address, \
         estimated_ready_at = EXCLUDED.estimated_ready_at, \
         placed_at = EXCLUDED.placed_at, \
         status_changed_at = EXCLUDED.status_changed_at, \
         payment_status = EXCLUDED.payment_status, \
         restaurant_stars = EXCLUDED.restaurant_stars, \
         rating_comment = EXCLUDED.rating_comment, \
         rider_thumb = EXCLUDED.rider_thumb, \
         rider_tip_cents = EXCLUDED.rider_tip_cents, \
         restaurant_tip_cents = EXCLUDED.restaurant_tip_cents, \
         captain_tip_cents = EXCLUDED.captain_tip_cents, \
         rated_at = EXCLUDED.rated_at, \
         delivery_status = EXCLUDED.delivery_status, \
         courier = EXCLUDED.courier, \
         estimated_dropoff_at = EXCLUDED.estimated_dropoff_at, \
         created_at = EXCLUDED.created_at, \
         updated_at = EXCLUDED.updated_at"
    );
    sqlx::query(&sql)
        .bind(row.order_id.0)
        .bind(row.r#ref.0.clone())
        .bind(row.restaurant_id.0)
        .bind(row.customer_id.as_ref().map(|v| v.0))
        .bind(row.status.to_ord())
        .bind(row.service_type.to_ord())
        .bind(row.items.clone())
        .bind(row.total_amount_cents.0)
        .bind(row.currency.0.clone())
        .bind(row.articles_cents.0)
        .bind(row.delivery_cents.0)
        .bind(row.service_fee_cents.0)
        .bind(row.restaurant_payout_cents.0)
        .bind(row.rider_payout_cents.0)
        .bind(row.captain_net_cents.0)
        .bind(row.uber_total_cents.as_ref().map(|v| v.0))
        .bind(row.uber_restaurant_cents.as_ref().map(|v| v.0))
        .bind(row.uber_rider_cents.as_ref().map(|v| v.0))
        .bind(row.uber_platform_cents.as_ref().map(|v| v.0))
        .bind(opt_to_ord(&row.uber_basis))
        .bind(opt_json(row.delivery_address.clone()))
        .bind(row.estimated_ready_at)
        .bind(row.placed_at)
        .bind(row.status_changed_at)
        .bind(row.payment_status.clone())
        .bind(row.restaurant_stars.as_ref().map(|v| v.0 as i32))
        .bind(row.rating_comment.as_ref().map(|v| v.0.clone()))
        .bind(opt_to_ord(&row.rider_thumb))
        .bind(row.rider_tip_cents.as_ref().map(|v| v.0))
        .bind(row.restaurant_tip_cents.as_ref().map(|v| v.0))
        .bind(row.captain_tip_cents.as_ref().map(|v| v.0))
        .bind(row.rated_at)
        .bind(opt_to_ord(&row.delivery_status))
        .bind(opt_json(row.courier.clone()))
        .bind(row.estimated_dropoff_at)
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}
