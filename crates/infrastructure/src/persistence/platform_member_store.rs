//! The `platform_member` table <-> [`PlatformMemberRow`] mapping — the platform grant bridge
//! (#639 part C step 6-v, ADR-20260905-223957 §1/§2; `projector: app`, ADR-0040;
//! `specs/database/tables/projection_tables.yaml#/PlatformMember`). `internal: true` — no GraphQL
//! `reads` target reaches this table directly; this is the ADMIN seam's `resolve_platform_scope`
//! lookup and the write-side grant handler's pre-append arbiter.

use application::queries::PlatformMemberRow;
use domain::generated::scalars::PlatformMembershipId;
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::Row;

use super::db_err;

pub(crate) const COLUMNS: &str = "platform_membership_id, auth_subject, created_at, updated_at";

pub(crate) fn decode(row: &PgRow) -> Result<PlatformMemberRow, DomainError> {
    Ok(PlatformMemberRow {
        platform_membership_id: PlatformMembershipId(
            row.try_get::<uuid::Uuid, _>("platform_membership_id").map_err(db_err)?,
        ),
        auth_subject: domain::generated::scalars::AuthSubject(
            row.try_get::<String, _>("auth_subject").map_err(db_err)?,
        ),
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}

/// Load the current projected state for one platform membership, or `None` before its
/// `PlatformAccessGranted`.
pub async fn load(
    exec: impl sqlx::PgExecutor<'_>,
    id: PlatformMembershipId,
) -> Result<Option<PlatformMemberRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM platform_member WHERE platform_membership_id = $1");
    let row = sqlx::query(&sql).bind(id.0).fetch_optional(exec).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// Read the LIVE `platformMembershipId` a given `authSubject` already resolves to, if any -- the
/// write-side grant handler's pre-append arbiter (`application::queries::PlatformMemberRepository`,
/// ADR-20260905-223957 §1) and the seam's own existence probe. `auth_subject UNIQUE` is what makes
/// a bare `fetch_optional` honest (the `member_store`/`Rider.auth_ref` precedent) -- never `LIMIT 1`.
pub async fn platform_membership_id_by_auth_subject(
    exec: impl sqlx::PgExecutor<'_>,
    auth_subject: &str,
) -> Result<Option<PlatformMembershipId>, DomainError> {
    let row = sqlx::query("SELECT platform_membership_id FROM platform_member WHERE auth_subject = $1")
        .bind(auth_subject)
        .fetch_optional(exec)
        .await
        .map_err(db_err)?;
    row.map(|r| Ok(PlatformMembershipId(r.try_get::<uuid::Uuid, _>("platform_membership_id").map_err(db_err)?)))
        .transpose()
}

/// Write the folded row. Idempotent on re-projection (the `member_store` shape): `created_at` is
/// absent from `DO UPDATE SET`. `platform_membership_id` is the row's PK and NEVER changes across
/// re-projection (unlike `Member`, there is no separate person-id scalar this bridge could rebind).
pub async fn upsert(
    exec: impl sqlx::PgExecutor<'_>,
    row: &PlatformMemberRow,
) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO platform_member ({COLUMNS}) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (platform_membership_id) DO UPDATE SET \
           updated_at = EXCLUDED.updated_at"
    );
    sqlx::query(&sql)
        .bind(row.platform_membership_id.0)
        .bind(row.auth_subject.0.clone())
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(exec)
        .await
        .map(|_| ())
        .map_err(db_err)
}
