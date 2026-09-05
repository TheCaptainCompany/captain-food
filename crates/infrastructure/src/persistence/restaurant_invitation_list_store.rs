//! The `restaurantinvitationlist` table <-> [`RestaurantInvitationListRow`] mapping — the
//! restaurant's own invitation list read model (#639 part C step 6-iv round 2, ADR-20260905-101349
//! §2 amendment; `projector: app`, ADR-0040;
//! `specs/database/tables/projection_tables.yaml#/RestaurantInvitationList`). The source of
//! `restaurantInvitations`.

use application::queries::{RestaurantInvitationListReadRepository, RestaurantInvitationListRow};
use async_trait::async_trait;
use domain::generated::scalars::{
    EmailAddress, MemberAuthority, RestaurantId, RestaurantInvitationId, RestaurantInvitationStatus,
};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::PgPool;
use sqlx::Row;

use super::db_err;

pub(crate) const COLUMNS: &str =
    "invitation_id, scope_id, invited_email, authority, status, expires_at, created_at, updated_at";

fn authority_text(a: MemberAuthority) -> &'static str {
    match a {
        MemberAuthority::MANAGER => "MANAGER",
        MemberAuthority::OPERATOR => "OPERATOR",
    }
}

fn status_text(s: RestaurantInvitationStatus) -> &'static str {
    match s {
        RestaurantInvitationStatus::PENDING => "PENDING",
        RestaurantInvitationStatus::ACCEPTED_PENDING_ACCESS => "ACCEPTED_PENDING_ACCESS",
        RestaurantInvitationStatus::ACCEPTED => "ACCEPTED",
        RestaurantInvitationStatus::REVOKED => "REVOKED",
        RestaurantInvitationStatus::EXPIRED => "EXPIRED",
    }
}

fn status_from_text(s: &str) -> RestaurantInvitationStatus {
    match s {
        "PENDING" => RestaurantInvitationStatus::PENDING,
        "ACCEPTED_PENDING_ACCESS" => RestaurantInvitationStatus::ACCEPTED_PENDING_ACCESS,
        "ACCEPTED" => RestaurantInvitationStatus::ACCEPTED,
        "REVOKED" => RestaurantInvitationStatus::REVOKED,
        _ => RestaurantInvitationStatus::EXPIRED,
    }
}

pub(crate) fn decode(row: &PgRow) -> Result<RestaurantInvitationListRow, DomainError> {
    Ok(RestaurantInvitationListRow {
        invitation_id: RestaurantInvitationId(row.try_get::<uuid::Uuid, _>("invitation_id").map_err(db_err)?),
        scope_id: RestaurantId(row.try_get::<uuid::Uuid, _>("scope_id").map_err(db_err)?),
        invited_email: EmailAddress(row.try_get::<String, _>("invited_email").map_err(db_err)?),
        authority: match row.try_get::<String, _>("authority").map_err(db_err)?.as_str() {
            "MANAGER" => MemberAuthority::MANAGER,
            _ => MemberAuthority::OPERATOR,
        },
        status: status_from_text(&row.try_get::<String, _>("status").map_err(db_err)?),
        expires_at: row.try_get("expires_at").map_err(db_err)?,
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}

/// Load the current projected state for one invitation, or `None` before its `RestaurantInvitationSent`.
pub async fn load(
    exec: impl sqlx::PgExecutor<'_>,
    id: RestaurantInvitationId,
) -> Result<Option<RestaurantInvitationListRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM restaurantinvitationlist WHERE invitation_id = $1");
    let row = sqlx::query(&sql).bind(id.0).fetch_optional(exec).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// Write the folded row. Rebuild = TRUNCATE + reset TOGETHER (the table's own `rules:`), so a plain
/// upsert is correct: a from-zero replay after truncation rebuilds every row from a clean slate.
pub async fn upsert(exec: impl sqlx::PgExecutor<'_>, row: &RestaurantInvitationListRow) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO restaurantinvitationlist ({COLUMNS}) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (invitation_id) DO UPDATE SET \
           scope_id = EXCLUDED.scope_id, \
           invited_email = EXCLUDED.invited_email, \
           authority = EXCLUDED.authority, \
           status = EXCLUDED.status, \
           expires_at = EXCLUDED.expires_at, \
           updated_at = EXCLUDED.updated_at"
    );
    sqlx::query(&sql)
        .bind(row.invitation_id.0)
        .bind(row.scope_id.0)
        .bind(row.invited_email.0.clone())
        .bind(authority_text(row.authority))
        .bind(status_text(row.status))
        .bind(row.expires_at)
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(exec)
        .await
        .map(|_| ())
        .map_err(db_err)
}

/// Truncate the whole table — the rebuild recipe's OTHER half (checkpoint reset ALONE is not
/// enough: the table's own `rules:` require TRUNCATE + reset together).
pub async fn truncate(exec: impl sqlx::PgExecutor<'_>) -> Result<(), DomainError> {
    sqlx::query("TRUNCATE TABLE restaurantinvitationlist").execute(exec).await.map(|_| ()).map_err(db_err)
}

/// One restaurant's invitations, `ORDER BY scope_id, status, created_at` (the declared index),
/// paged. `limit`/`offset` are already clamped by the resolver.
pub async fn by_scope(
    exec: impl sqlx::PgExecutor<'_>,
    scope_id: RestaurantId,
    limit: i64,
    offset: i64,
) -> Result<Vec<RestaurantInvitationListRow>, DomainError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM restaurantinvitationlist WHERE scope_id = $1 ORDER BY status, created_at LIMIT $2 OFFSET $3"
    );
    let rows = sqlx::query(&sql).bind(scope_id.0).bind(limit).bind(offset).fetch_all(exec).await.map_err(db_err)?;
    rows.iter().map(decode).collect()
}

/// Postgres read adapter — the `restaurantInvitations` resolver's port.
pub struct PgRestaurantInvitationListRepository {
    pool: PgPool,
}

impl PgRestaurantInvitationListRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RestaurantInvitationListReadRepository for PgRestaurantInvitationListRepository {
    async fn by_scope(
        &self,
        scope_id: RestaurantId,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<RestaurantInvitationListRow>, DomainError> {
        by_scope(&self.pool, scope_id, limit, offset).await
    }
}
