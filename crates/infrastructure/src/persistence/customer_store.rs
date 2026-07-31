//! The 15-column `customer` table ↔ [`CustomerRow`] mapping, both directions — shared by the read
//! repository (decode) and the projection worker (load current state + upsert the folded row).
//!
//! Column conventions (ADR-0037/0040): `ratings`/`favorite_restaurant_ids`/`addresses` are NOT NULL
//! jsonb accumulation columns carrying `serde_json::Value`; `preferences` is a nullable jsonb; the
//! scalar newtypes (phone/auth_ref/display_name/email/locale/timezone/payment_method_id) bind via
//! their inner `.0`. The table has no enum columns.

use application::queries::CustomerRow;
use domain::generated::scalars::{
    CustomerDisplayName, CustomerId, EmailAddress, ExternalReference, Locale, PaymentMethodId,
    PhoneNumber, TimeZone,
};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::Row;

use super::db_err;

/// The full column list, in `CustomerRow` field order — keep SELECTs and the upsert in sync with it.
pub(crate) const COLUMNS: &str = "customer_id, phone, auth_ref, display_name, email, \
     email_verified, locale, timezone, ratings, favorite_restaurant_ids, preferences, addresses, \
     payment_method_id, created_at, updated_at";

/// Normalize a nullable jsonb: a JSON `null` in the column (or in the row) means "no value".
fn opt_json(v: Option<serde_json::Value>) -> Option<serde_json::Value> {
    v.filter(|j| !j.is_null())
}

/// Decode one `customer` row into the generated read-model DTO.
pub(crate) fn decode(row: &PgRow) -> Result<CustomerRow, DomainError> {
    Ok(CustomerRow {
        customer_id: CustomerId(row.try_get("customer_id").map_err(db_err)?),
        phone: PhoneNumber(row.try_get("phone").map_err(db_err)?),
        auth_ref: row
            .try_get::<Option<String>, _>("auth_ref")
            .map_err(db_err)?
            .map(ExternalReference),
        display_name: row
            .try_get::<Option<String>, _>("display_name")
            .map_err(db_err)?
            .map(CustomerDisplayName),
        email: row
            .try_get::<Option<String>, _>("email")
            .map_err(db_err)?
            .map(EmailAddress),
        email_verified: row.try_get("email_verified").map_err(db_err)?,
        locale: row.try_get::<Option<String>, _>("locale").map_err(db_err)?.map(Locale),
        timezone: row.try_get::<Option<String>, _>("timezone").map_err(db_err)?.map(TimeZone),
        ratings: row.try_get("ratings").map_err(db_err)?,
        favorite_restaurant_ids: row.try_get("favorite_restaurant_ids").map_err(db_err)?,
        preferences: opt_json(row.try_get("preferences").map_err(db_err)?),
        addresses: row.try_get("addresses").map_err(db_err)?,
        payment_method_id: row
            .try_get::<Option<String>, _>("payment_method_id")
            .map_err(db_err)?
            .map(PaymentMethodId),
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}

/// Load the current projected state for one customer, or `None` before its creation event.
pub async fn load(exec: impl sqlx::PgExecutor<'_>, id: CustomerId) -> Result<Option<CustomerRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM customer WHERE customer_id = $1");
    let row = sqlx::query(&sql).bind(id.0).fetch_optional(exec).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// Write the folded row: `INSERT … ON CONFLICT (customer_id) DO UPDATE` over all 15 columns.
pub async fn upsert(exec: impl sqlx::PgExecutor<'_>, row: &CustomerRow) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO customer ({COLUMNS}) VALUES \
         ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) \
         ON CONFLICT (customer_id) DO UPDATE SET \
         phone = EXCLUDED.phone, \
         auth_ref = EXCLUDED.auth_ref, \
         display_name = EXCLUDED.display_name, \
         email = EXCLUDED.email, \
         email_verified = EXCLUDED.email_verified, \
         locale = EXCLUDED.locale, \
         timezone = EXCLUDED.timezone, \
         ratings = EXCLUDED.ratings, \
         favorite_restaurant_ids = EXCLUDED.favorite_restaurant_ids, \
         preferences = EXCLUDED.preferences, \
         addresses = EXCLUDED.addresses, \
         payment_method_id = EXCLUDED.payment_method_id, \
         created_at = EXCLUDED.created_at, \
         updated_at = EXCLUDED.updated_at"
    );
    sqlx::query(&sql)
        .bind(row.customer_id.0)
        .bind(row.phone.0.clone())
        .bind(row.auth_ref.as_ref().map(|v| v.0.clone()))
        .bind(row.display_name.as_ref().map(|v| v.0.clone()))
        .bind(row.email.as_ref().map(|v| v.0.clone()))
        .bind(row.email_verified)
        .bind(row.locale.as_ref().map(|v| v.0.clone()))
        .bind(row.timezone.as_ref().map(|v| v.0.clone()))
        .bind(row.ratings.clone())
        .bind(row.favorite_restaurant_ids.clone())
        .bind(opt_json(row.preferences.clone()))
        .bind(row.addresses.clone())
        .bind(row.payment_method_id.as_ref().map(|v| v.0.clone()))
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(exec)
        .await
        .map_err(db_err)?;
    Ok(())
}
