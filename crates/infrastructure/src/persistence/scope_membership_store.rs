//! The `scopemembership` table (#144, PROP-20260725-185140 §3.4) — the single index every read-side
//! authorization question resolves against.
//!
//! Three operations, and the asymmetry between them is the whole safety story:
//!   * [`grant`]   — idempotent upsert, keyed by the UUIDv5-derived `membership_id`;
//!   * [`revoke_role`] — drops every principal of one role on one scope;
//!   * [`is_member`]   — the guard's read: one primary-key `EXISTS`, no joins, ever.
//!
//! A MISSING row denies (visible, safe). A STALE row grants (a silent breach). So `revoke_role`
//! deletes broadly and `grant` writes narrowly — never the other way round.
//!
//! Column conventions (ADR-0037/0040): `scope_type` and `principal_type` are INTEGER ordinals (see
//! [`crate::persistence::enum_sql`]), not text.

use application::projectors::scope_membership::membership_id;
use domain::generated::scalars::{ScopeType, UserType};
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::db_err;
use super::enum_sql::EnumOrd;

/// Record one membership. Idempotent by construction: `membership_id` is derived from the natural key,
/// so replaying the same grant (a projection replay, a redelivered event) upserts onto the same row
/// rather than duplicating it. `granted_at` is preserved on conflict — the audit answer to "since when
/// could this principal see this?" must not drift on every replay.
pub async fn grant(
    tx: &mut Transaction<'_, Postgres>,
    scope_type: ScopeType,
    scope_id: Uuid,
    principal_type: UserType,
    principal_id: Uuid,
    granted_at: chrono::DateTime<chrono::Utc>,
) -> Result<(), DomainError> {
    let id = membership_id(scope_type, scope_id, principal_type, principal_id);
    sqlx::query(
        "INSERT INTO scopemembership \
           (membership_id, scope_type, scope_id, principal_type, principal_id, granted_at, created_at, updated_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$7) \
         ON CONFLICT (membership_id) DO UPDATE SET updated_at = EXCLUDED.updated_at",
    )
    .bind(id)
    .bind(scope_type.to_ord())
    .bind(scope_id)
    .bind(principal_type.to_ord())
    .bind(principal_id)
    .bind(granted_at)
    .bind(chrono::Utc::now())
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Drop EVERY principal holding `principal_type` on this scope.
///
/// Deliberately not "revoke this one rider": if a reassignment ever left two rider rows on one order,
/// a targeted delete would strip one and leave the other holding access — the stale-grant breach. The
/// broad delete is safe because the replacement grant is re-applied by the very next event.
pub async fn revoke_role(
    tx: &mut Transaction<'_, Postgres>,
    scope_type: ScopeType,
    scope_id: Uuid,
    principal_type: UserType,
) -> Result<u64, DomainError> {
    let deleted = sqlx::query(
        "DELETE FROM scopemembership \
          WHERE scope_type = $1 AND scope_id = $2 AND principal_type = $3",
    )
    .bind(scope_type.to_ord())
    .bind(scope_id)
    .bind(principal_type.to_ord())
    .execute(&mut **tx)
    .await
    .map_err(db_err)?
    .rows_affected();
    Ok(deleted)
}

/// The guard's question, for every role and every surface: may this principal see this instance?
///
/// One primary-key lookup — the id is derived in-process from the four parts, so this never scans,
/// never joins, and costs the same whatever the scope type is.
pub async fn is_member(
    pool: &PgPool,
    scope_type: ScopeType,
    scope_id: Uuid,
    principal_type: UserType,
    principal_id: Uuid,
) -> Result<bool, DomainError> {
    let id = membership_id(scope_type, scope_id, principal_type, principal_id);
    let found: Option<(bool,)> =
        sqlx::query_as("SELECT true FROM scopemembership WHERE membership_id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    Ok(found.is_some())
}

/// Every scope of one type this principal may see — the list-query filter, served by the
/// `(principal_type, principal_id, scope_type)` index rather than by a second mechanism.
pub async fn scopes_for(
    pool: &PgPool,
    principal_type: UserType,
    principal_id: Uuid,
    scope_type: ScopeType,
) -> Result<Vec<Uuid>, DomainError> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT scope_id FROM scopemembership \
          WHERE principal_type = $1 AND principal_id = $2 AND scope_type = $3",
    )
    .bind(principal_type.to_ord())
    .bind(principal_id)
    .bind(scope_type.to_ord())
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}
