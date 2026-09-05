//! The `restaurantroster` table <-> [`RestaurantRosterRow`] mapping — the restaurant's own team
//! roster read model (#639 part C step 6-iv round 2, ADR-20260905-101349 §2 amendment;
//! `projector: app`, ADR-0040; `specs/database/tables/projection_tables.yaml#/RestaurantRoster`).
//! The source of `restaurantRoster`.

use application::queries::{RestaurantRosterReadRepository, RestaurantRosterRow};
use async_trait::async_trait;
use domain::generated::scalars::{MembershipId, RestaurantId};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::PgPool;
use sqlx::Row;

use super::db_err;

/// The full column list, in `RestaurantRosterRow` field order — keep SELECTs and the upsert in sync.
pub(crate) const COLUMNS: &str = "membership_id, scope_id, member_id, authority, since, created_at, updated_at";

pub(crate) fn decode(row: &PgRow) -> Result<RestaurantRosterRow, DomainError> {
    Ok(RestaurantRosterRow {
        membership_id: MembershipId(row.try_get::<uuid::Uuid, _>("membership_id").map_err(db_err)?),
        scope_id: RestaurantId(row.try_get::<uuid::Uuid, _>("scope_id").map_err(db_err)?),
        member_id: domain::generated::scalars::MemberId(row.try_get::<uuid::Uuid, _>("member_id").map_err(db_err)?),
        authority: match row.try_get::<String, _>("authority").map_err(db_err)?.as_str() {
            "MANAGER" => domain::generated::scalars::MemberAuthority::MANAGER,
            _ => domain::generated::scalars::MemberAuthority::OPERATOR,
        },
        since: row.try_get("since").map_err(db_err)?,
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}

fn authority_text(a: domain::generated::scalars::MemberAuthority) -> &'static str {
    match a {
        domain::generated::scalars::MemberAuthority::MANAGER => "MANAGER",
        domain::generated::scalars::MemberAuthority::OPERATOR => "OPERATOR",
    }
}

/// Load the current projected state for one membership, or `None` before its `RestaurantAccessGranted`.
pub async fn load(
    exec: impl sqlx::PgExecutor<'_>,
    id: MembershipId,
) -> Result<Option<RestaurantRosterRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM restaurantroster WHERE membership_id = $1");
    let row = sqlx::query(&sql).bind(id.0).fetch_optional(exec).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// Write the folded row. Rebuild = checkpoint reset, NEVER TRUNCATE (the table's own `rules:`):
/// idempotent on re-projection, `created_at` absent from `DO UPDATE SET`.
pub async fn upsert(exec: impl sqlx::PgExecutor<'_>, row: &RestaurantRosterRow) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO restaurantroster ({COLUMNS}) VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (membership_id) DO UPDATE SET \
           scope_id = EXCLUDED.scope_id, \
           member_id = EXCLUDED.member_id, \
           authority = EXCLUDED.authority, \
           since = EXCLUDED.since, \
           updated_at = EXCLUDED.updated_at"
    );
    sqlx::query(&sql)
        .bind(row.membership_id.0)
        .bind(row.scope_id.0)
        .bind(row.member_id.0)
        .bind(authority_text(row.authority))
        .bind(row.since)
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(exec)
        .await
        .map(|_| ())
        .map_err(db_err)
}

/// Round 3 (#639 part C step 6-iv, dba BLOCKING): the `RestaurantAccessRevoked` DELETE arm — the
/// `scope_membership_store::revoke_member` shape. Deliberately a real DELETE, not a soft-status
/// column: the table's own `rules:` name this ADDITIVE to the mechanical GRANT arm above (`fedBy`
/// gains a second event), never a replacement of it, and the checkpoint-reset-never-TRUNCATE
/// rebuild discipline is unaffected — a fresh replay applies the grant, then this delete, in the
/// SAME global `position` order the live path already folds in.
pub async fn delete(
    exec: impl sqlx::PgExecutor<'_>,
    id: MembershipId,
) -> Result<u64, DomainError> {
    let deleted = sqlx::query("DELETE FROM restaurantroster WHERE membership_id = $1")
        .bind(id.0)
        .execute(exec)
        .await
        .map_err(db_err)?
        .rows_affected();
    Ok(deleted)
}

/// One restaurant's team, `ORDER BY scope_id, member_id` (the `(scope_id, member_id)` index),
/// paged. `limit`/`offset` are already clamped by the resolver.
pub async fn by_scope(
    exec: impl sqlx::PgExecutor<'_>,
    scope_id: RestaurantId,
    limit: i64,
    offset: i64,
) -> Result<Vec<RestaurantRosterRow>, DomainError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM restaurantroster WHERE scope_id = $1 ORDER BY scope_id, member_id LIMIT $2 OFFSET $3"
    );
    let rows = sqlx::query(&sql).bind(scope_id.0).bind(limit).bind(offset).fetch_all(exec).await.map_err(db_err)?;
    rows.iter().map(decode).collect()
}

/// Postgres read adapter — the `restaurantRoster` resolver's port.
pub struct PgRestaurantRosterRepository {
    pool: PgPool,
}

impl PgRestaurantRosterRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RestaurantRosterReadRepository for PgRestaurantRosterRepository {
    async fn by_scope(
        &self,
        scope_id: RestaurantId,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<RestaurantRosterRow>, DomainError> {
        by_scope(&self.pool, scope_id, limit, offset).await
    }
}
