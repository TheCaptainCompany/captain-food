//! The `member` table <-> [`MemberRow`] mapping — the staff-authentication bridge (#639 part C
//! step 6-i, ADR-20260905-101349 §5; `projector: app`, ADR-0040;
//! `specs/database/tables/projection_tables.yaml#/Member`). `internal: true` — no GraphQL `reads`
//! target reaches this table directly in 6-i (the roster, 6-iv, reads its own table); this is the
//! seam's future `auth_subject -> member_id` lookup (6-ii).

use application::queries::MemberRow;
use domain::generated::scalars::MemberId;
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::Row;

use super::db_err;

pub(crate) const COLUMNS: &str = "member_id, auth_subject, created_at, updated_at";

pub(crate) fn decode(row: &PgRow) -> Result<MemberRow, DomainError> {
    Ok(MemberRow {
        member_id: MemberId(row.try_get::<uuid::Uuid, _>("member_id").map_err(db_err)?),
        auth_subject: domain::generated::scalars::AuthSubject(
            row.try_get::<String, _>("auth_subject").map_err(db_err)?,
        ),
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}

/// Load the current projected state for one member, or `None` before its `RestaurantAccessGranted`.
pub async fn load(
    exec: impl sqlx::PgExecutor<'_>,
    id: MemberId,
) -> Result<Option<MemberRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM member WHERE member_id = $1");
    let row = sqlx::query(&sql).bind(id.0).fetch_optional(exec).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// Write the folded row. Idempotent on re-projection (the `rider_roster_store` shape):
/// `created_at` is absent from `DO UPDATE SET`. `RestaurantAccessRevoked` touches NOTHING here
/// (the table's own `rules:`), so the only writer is the one creating arm below.
///
/// `auth_subject` is FIRST-WRITE-WINS, like `created_at` (round-2 dba finding, R2-8): a second
/// grant for the same `member_id` under a fresh `membershipId` and a different `authSubject` must
/// never rebind the bridge -- "the binding OUTLIVES any one grant" (the table's own `rules:`). If
/// `auth_subject` were in `DO UPDATE SET`, that second grant would pass every belt (the
/// idempotency key is `membershipId`; the reservation keys on the fresh subject) and silently
/// orphan the first credential. Replay-deterministic: a full rebuild folds the same events in the
/// same order and lands on the same first subject every time.
pub async fn upsert(exec: impl sqlx::PgExecutor<'_>, row: &MemberRow) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO member ({COLUMNS}) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (member_id) DO UPDATE SET \
           updated_at = EXCLUDED.updated_at"
    );
    sqlx::query(&sql)
        .bind(row.member_id.0)
        .bind(row.auth_subject.0.clone())
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(exec)
        .await
        .map(|_| ())
        .map_err(db_err)
}
