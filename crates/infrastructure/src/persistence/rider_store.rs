//! The `rider` table ↔ [`RiderRow`] mapping — the rider identity read model (#639 part A;
//! `projector: app`, ADR-0040; `specs/database/tables/projection_tables.yaml#/Rider`).
//!
//! What this table is FOR is the `auth_ref -> rider_id` bridge that
//! [ADR-20260818-004646](../../../../docs/adr/ADR-20260818-004646-no-business-identifier-lives-in-the-identity-provider.md)
//! requires to live in our Postgres rather than in the identity provider's claims. `RiderRegistered`
//! has carried `authRef` as required since it was written; until now nothing projected it, so the
//! RIDER role had no resolvable identity at all.
//!
//! **Deliberately no `by_auth_ref` here yet.** The resolver arrives with the rider sign-in door
//! (#639 part C), and a lookup nothing calls is a port that cannot be verified — the shape the
//! erasure chunk already paid for once. When it lands, two things are not negotiable: it selects
//! `rider_id` and NOTHING else (the table answers *who this connection is*, never *what it may
//! see* — see the read model's own `rules:`), and it must never `LIMIT 1`, because picking a row
//! is an elevation decision made by row order. The `UNIQUE` on `auth_ref` is what lets the query be
//! written without one.

use application::queries::RiderRow;
use domain::generated::scalars::{ExternalReference, PhoneNumber, RiderId};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::Row;

use super::enum_sql::EnumText;
use super::db_err;

/// The full column list, in `RiderRow` field order — keep SELECTs and the upsert in sync.
pub(crate) const COLUMNS: &str =
    "rider_id, auth_ref, display_name, phone, status, created_at, updated_at";

pub(crate) fn decode(row: &PgRow) -> Result<RiderRow, DomainError> {
    Ok(RiderRow {
        rider_id: RiderId(row.try_get::<uuid::Uuid, _>("rider_id").map_err(db_err)?),
        auth_ref: ExternalReference(row.try_get::<String, _>("auth_ref").map_err(db_err)?),
        display_name: row.try_get::<String, _>("display_name").map_err(db_err)?,
        phone: PhoneNumber(row.try_get::<String, _>("phone").map_err(db_err)?),
        status: EnumText::from_text(&row.try_get::<String, _>("status").map_err(db_err)?)?,
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}

/// Load the current projected state for one rider, or `None` before its `RiderRegistered`.
pub async fn load(exec: impl sqlx::PgExecutor<'_>, id: RiderId) -> Result<Option<RiderRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM rider WHERE rider_id = $1");
    let row = sqlx::query(&sql).bind(id.0).fetch_optional(exec).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// Write the folded row. Idempotent on re-projection: replaying the same ordered facts over the
/// current row is a deterministic fold, which is what makes this table's recovery a REPLAY rather
/// than a restore — and why a rebuild resets the checkpoint instead of truncating (the read model's
/// `rules:`): every row is rewritten in place, so no rider is ever missing mid-drain.
///
/// `created_at` is deliberately absent from the `DO UPDATE SET` list (the `slugalias` shape, not the
/// `customer` one): the creation instant belongs to `RiderRegistered` and no later fact may move it.
pub async fn upsert(exec: impl sqlx::PgExecutor<'_>, row: &RiderRow) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO rider ({COLUMNS}) VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (rider_id) DO UPDATE SET \
           auth_ref = EXCLUDED.auth_ref, \
           display_name = EXCLUDED.display_name, \
           phone = EXCLUDED.phone, \
           status = EXCLUDED.status, \
           updated_at = EXCLUDED.updated_at"
    );
    sqlx::query(&sql)
        .bind(row.rider_id.0)
        .bind(row.auth_ref.0.clone())
        .bind(row.display_name.clone())
        .bind(row.phone.0.clone())
        .bind(row.status.to_text())
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(exec)
        .await
        .map(|_| ())
        .map_err(db_err)
}
