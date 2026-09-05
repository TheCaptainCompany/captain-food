//! sqlx read adapter over the `member` projection's identity bridge (#639 part C step 6-ii,
//! ADR-20260905-101349 §7/§C): `auth_subject -> member_id`, for `confirm_member_sign_in`'s
//! identify-only lookup (`application::queries::MemberIdentityRepository`).
//!
//! One property is not negotiable and is stated on the port too: it never `LIMIT 1`s -- the
//! `UNIQUE` on `auth_subject` (the table's own rule, `specs/database/tables/projection_tables.yaml`)
//! is what lets `fetch_optional` be written without one (the `Rider.auth_ref` precedent).

use application::queries::MemberIdentityRepository;
use async_trait::async_trait;
use domain::generated::scalars::{AuthSubject, MemberId};
use domain::shared::errors::DomainError;
use sqlx::PgPool;
use sqlx::Row;

use super::db_err;

/// Postgres adapter for the member identity bridge.
pub struct PgMemberRepository {
    pool: PgPool,
}

impl PgMemberRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MemberIdentityRepository for PgMemberRepository {
    async fn member_id_by_auth_subject(
        &self,
        auth_subject: AuthSubject,
    ) -> Result<Option<MemberId>, DomainError> {
        // One btree probe on the UNIQUE constraint's own index.
        let row = sqlx::query("SELECT member_id FROM member WHERE auth_subject = $1")
            .bind(auth_subject.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        let Some(row) = row else { return Ok(None) };
        let id: uuid::Uuid = row.try_get("member_id").map_err(db_err)?;
        Ok(Some(MemberId(id)))
    }
}
