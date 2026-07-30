//! The 27-column `restaurant` table ↔ [`RestaurantRow`] mapping, both directions — shared by the read
//! repository (decode) and the projection worker (load current state + upsert the folded row).
//!
//! Column conventions (ADR-20260728/0040): enum columns are TEXT values (see
//! [`crate::persistence::enum_sql`]); jsonb columns carry `serde_json::Value`; scalar newtypes bind via
//! their inner `.0`; `margin_rate`/`rating` are TEXT columns round-tripped through `f64` display/parse;
//! `reviews_count`/`preparation_time_minutes` are INTEGER columns widened to `i64` in the row.

use application::queries::RestaurantRow;
use domain::generated::scalars::{
    GooglePlaceId, GoogleRating, MarginPercent, RestaurantAccountId, RestaurantDisplayName,
    RestaurantId, Slug, TimeZone, WebUrl,
};
use domain::generated::scalars::CurrencyCode;
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use super::db_err;
use super::enum_sql::{opt_from_text, opt_to_text, EnumText};

/// The full column list, in `RestaurantRow` field order — keep SELECTs and the upsert in sync with it.
pub(crate) const COLUMNS: &str = "restaurant_id, restaurant_account_id, listing_status, \
     external_identifiers, google_place_id, slug, display_name, description, tags, margin_rate, \
     cuisine_category, uber_prices_opt_in, website, rating, reviews_count, gbp_order_url, \
     gbp_link_status, address, location, opening_hours, status, order_acceptance, default_currency, \
     timezone, preparation_time_minutes, created_at, updated_at";

/// Normalize a nullable jsonb: a JSON `null` in the column (or in the row) means "no value".
fn opt_json(v: Option<serde_json::Value>) -> Option<serde_json::Value> {
    v.filter(|j| !j.is_null())
}

fn parse_f64_col(col: &str, v: Option<String>) -> Result<Option<f64>, DomainError> {
    v.map(|s| {
        s.parse::<f64>()
            .map_err(|e| db_err(format!("column {col}: invalid numeric text {s:?}: {e}")))
    })
    .transpose()
}

/// Decode one `restaurant` row into the generated read-model DTO.
pub(crate) fn decode(row: &PgRow) -> Result<RestaurantRow, DomainError> {
    Ok(RestaurantRow {
        restaurant_id: RestaurantId(row.try_get("restaurant_id").map_err(db_err)?),
        restaurant_account_id: row
            .try_get::<Option<uuid::Uuid>, _>("restaurant_account_id")
            .map_err(db_err)?
            .map(RestaurantAccountId),
        listing_status: EnumText::from_text(&row.try_get::<String, _>("listing_status").map_err(db_err)?)?,
        external_identifiers: opt_json(row.try_get("external_identifiers").map_err(db_err)?),
        google_place_id: row
            .try_get::<Option<String>, _>("google_place_id")
            .map_err(db_err)?
            .map(GooglePlaceId),
        slug: row.try_get::<Option<String>, _>("slug").map_err(db_err)?.map(Slug),
        display_name: RestaurantDisplayName(row.try_get("display_name").map_err(db_err)?),
        description: row.try_get("description").map_err(db_err)?,
        tags: opt_json(row.try_get("tags").map_err(db_err)?),
        margin_rate: parse_f64_col("margin_rate", row.try_get("margin_rate").map_err(db_err)?)?
            .map(MarginPercent),
        cuisine_category: opt_from_text(row.try_get("cuisine_category").map_err(db_err)?)?,
        uber_prices_opt_in: row.try_get("uber_prices_opt_in").map_err(db_err)?,
        website: row.try_get::<Option<String>, _>("website").map_err(db_err)?.map(WebUrl),
        rating: parse_f64_col("rating", row.try_get("rating").map_err(db_err)?)?.map(GoogleRating),
        reviews_count: row
            .try_get::<Option<i32>, _>("reviews_count")
            .map_err(db_err)?
            .map(i64::from),
        gbp_order_url: row
            .try_get::<Option<String>, _>("gbp_order_url")
            .map_err(db_err)?
            .map(WebUrl),
        gbp_link_status: opt_from_text(row.try_get("gbp_link_status").map_err(db_err)?)?,
        address: row.try_get("address").map_err(db_err)?,
        location: opt_json(row.try_get("location").map_err(db_err)?),
        opening_hours: row.try_get("opening_hours").map_err(db_err)?,
        status: EnumText::from_text(&row.try_get::<String, _>("status").map_err(db_err)?)?,
        order_acceptance: EnumText::from_text(&row.try_get::<String, _>("order_acceptance").map_err(db_err)?)?,
        default_currency: CurrencyCode(row.try_get("default_currency").map_err(db_err)?),
        timezone: row.try_get::<Option<String>, _>("timezone").map_err(db_err)?.map(TimeZone),
        preparation_time_minutes: row
            .try_get::<Option<i32>, _>("preparation_time_minutes")
            .map_err(db_err)?
            .map(i64::from),
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}

/// Load the current projected state for one restaurant, or `None` before its creation event.
pub async fn load(pool: &PgPool, id: RestaurantId) -> Result<Option<RestaurantRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM restaurant WHERE restaurant_id = $1");
    let row = sqlx::query(&sql).bind(id.0).fetch_optional(pool).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// Write the folded row: `INSERT … ON CONFLICT (restaurant_id) DO UPDATE` over all 27 columns.
pub async fn upsert(pool: &PgPool, row: &RestaurantRow) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO restaurant ({COLUMNS}) VALUES \
         ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27) \
         ON CONFLICT (restaurant_id) DO UPDATE SET \
         restaurant_account_id = EXCLUDED.restaurant_account_id, \
         listing_status = EXCLUDED.listing_status, \
         external_identifiers = EXCLUDED.external_identifiers, \
         google_place_id = EXCLUDED.google_place_id, \
         slug = EXCLUDED.slug, \
         display_name = EXCLUDED.display_name, \
         description = EXCLUDED.description, \
         tags = EXCLUDED.tags, \
         margin_rate = EXCLUDED.margin_rate, \
         cuisine_category = EXCLUDED.cuisine_category, \
         uber_prices_opt_in = EXCLUDED.uber_prices_opt_in, \
         website = EXCLUDED.website, \
         rating = EXCLUDED.rating, \
         reviews_count = EXCLUDED.reviews_count, \
         gbp_order_url = EXCLUDED.gbp_order_url, \
         gbp_link_status = EXCLUDED.gbp_link_status, \
         address = EXCLUDED.address, \
         location = EXCLUDED.location, \
         opening_hours = EXCLUDED.opening_hours, \
         status = EXCLUDED.status, \
         order_acceptance = EXCLUDED.order_acceptance, \
         default_currency = EXCLUDED.default_currency, \
         timezone = EXCLUDED.timezone, \
         preparation_time_minutes = EXCLUDED.preparation_time_minutes, \
         created_at = EXCLUDED.created_at, \
         updated_at = EXCLUDED.updated_at"
    );
    sqlx::query(&sql)
        .bind(row.restaurant_id.0)
        .bind(row.restaurant_account_id.as_ref().map(|v| v.0))
        .bind(row.listing_status.to_text())
        .bind(opt_json(row.external_identifiers.clone()))
        .bind(row.google_place_id.as_ref().map(|v| v.0.clone()))
        .bind(row.slug.as_ref().map(|s| s.0.clone()))
        .bind(row.display_name.0.clone())
        .bind(row.description.clone())
        .bind(opt_json(row.tags.clone()))
        .bind(row.margin_rate.as_ref().map(|v| v.0.to_string()))
        .bind(opt_to_text(&row.cuisine_category))
        .bind(row.uber_prices_opt_in)
        .bind(row.website.as_ref().map(|v| v.0.clone()))
        .bind(row.rating.as_ref().map(|v| v.0.to_string()))
        .bind(row.reviews_count.map(|v| v as i32))
        .bind(row.gbp_order_url.as_ref().map(|v| v.0.clone()))
        .bind(opt_to_text(&row.gbp_link_status))
        .bind(row.address.clone())
        .bind(opt_json(row.location.clone()))
        .bind(row.opening_hours.clone())
        .bind(row.status.to_text())
        .bind(row.order_acceptance.to_text())
        .bind(row.default_currency.0.clone())
        .bind(row.timezone.as_ref().map(|v| v.0.clone()))
        .bind(row.preparation_time_minutes.map(|v| v as i32))
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}
