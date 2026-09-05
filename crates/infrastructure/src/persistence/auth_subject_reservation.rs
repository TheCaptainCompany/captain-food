//! Postgres `AuthSubjectReservationRepository` — the write-side arbiter of "one login credential,
//! one principal of each kind" (#639 part C step 2a, #794; the `slug_reservations` copy job of
//! ADR-20260728-011344 D3).
//!
//! **Postgres decides, not us.** `reserve` is a single `INSERT … ON CONFLICT DO NOTHING` against a
//! table whose composite primary key IS `(principal_kind, auth_subject)`: exactly one of two
//! concurrent claims inserts a row, and the loser gets `rows_affected == 0`. No read-then-write
//! window exists, so the outcome cannot be raced — which is precisely what the eventually consistent
//! `Rider` projection's `auth_ref UNIQUE` cannot promise: that constraint fires in the PROJECTOR,
//! after `RiderRegistered` is already in the immutable log.
//!
//! THE KEY IS THE PAIR. A rider who is also a customer holds two rows on one credential; the
//! conflict target names both columns so a `CUSTOMER` binding never blocks a `RIDER` one.
//!
//! NO RELEASE. There is no `release` method and no `released_at` column — stronger than the slug
//! sibling's "released is not free". Revoking a rider must not free the binding, or a later
//! registration would bind the same human to a NEW rider id and orphan their history.

use application::queries::{AuthSubjectReservationRepository, BoundPrincipal};
use async_trait::async_trait;
use domain::generated::scalars::AuthSubject;
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};

use super::db_err;
use super::enum_sql::EnumText;

pub struct PgAuthSubjectReservationRepository {
    pool: PgPool,
}

impl PgAuthSubjectReservationRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AuthSubjectReservationRepository for PgAuthSubjectReservationRepository {
    async fn reserve(
        &self,
        subject: AuthSubject,
        principal: BoundPrincipal,
    ) -> Result<bool, DomainError> {
        let kind = principal.kind().to_text();
        // One statement, no read-then-write window. `DO NOTHING` rather than `DO UPDATE` because a
        // row held by someone else must NOT be overwritten -- losing is the whole signal.
        let inserted = sqlx::query(
            "INSERT INTO auth_subject_reservations \
               (principal_kind, auth_subject, principal_id, reserved_at) \
             VALUES ($1, $2, $3, now()) \
             ON CONFLICT (principal_kind, auth_subject) DO NOTHING",
        )
        .bind(kind)
        .bind(subject.0.clone())
        .bind(principal.id())
        .execute(&self.pool)
        .await
        .map_err(db_err)?
        .rows_affected();
        if inserted == 1 {
            return Ok(true);
        }
        // We lost the insert. That is a conflict UNLESS the existing row is already ours -- a replay
        // of this same registration, which must stay idempotent (the handler may have crashed
        // between reserving and appending, and the same rider simply re-submits with the same id).
        let holder: Option<uuid::Uuid> = sqlx::query(
            "SELECT principal_id FROM auth_subject_reservations \
             WHERE principal_kind = $1 AND auth_subject = $2",
        )
        .bind(kind)
        .bind(subject.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        .map(|row| row.try_get("principal_id"))
        .transpose()
        .map_err(db_err)?;
        Ok(holder == Some(principal.id()))
    }

    async fn holder_of(
        &self,
        subject: AuthSubject,
        kind: domain::generated::scalars::PrincipalKind,
    ) -> Result<Option<uuid::Uuid>, DomainError> {
        let holder: Option<uuid::Uuid> = sqlx::query(
            "SELECT principal_id FROM auth_subject_reservations \
             WHERE principal_kind = $1 AND auth_subject = $2",
        )
        .bind(kind.to_text())
        .bind(subject.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?
        .map(|row| row.try_get("principal_id"))
        .transpose()
        .map_err(db_err)?;
        Ok(holder)
    }
}
