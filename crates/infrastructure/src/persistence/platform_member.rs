//! sqlx read adapter over the `platform_member` projection's bridge (#639 part C step 6-v,
//! ADR-20260905-223957 §1/§2): `auth_subject -> platformMembershipId`, for
//! `grant_platform_access`'s pre-append arbiter (`application::queries::PlatformMemberRepository`)
//! -- the `PgMemberRepository` precedent, transposed.
//!
//! One property is not negotiable and is stated on the port too: it never `LIMIT 1`s -- the
//! `UNIQUE` on `auth_subject` (the table's own rule, `specs/database/tables/projection_tables.yaml`)
//! is what lets `fetch_optional` be written without one (the `Rider.auth_ref`/`Member.auth_subject`
//! precedent).

use application::queries::PlatformMemberRepository;
use async_trait::async_trait;
use domain::generated::scalars::{AuthSubject, PlatformMembershipId};
use domain::shared::errors::DomainError;
use sqlx::PgPool;

use super::platform_member_store;

/// Postgres adapter for the platform grant bridge.
pub struct PgPlatformMemberRepository {
    pool: PgPool,
}

impl PgPlatformMemberRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PlatformMemberRepository for PgPlatformMemberRepository {
    async fn platform_membership_id_by_auth_subject(
        &self,
        auth_subject: AuthSubject,
    ) -> Result<Option<PlatformMembershipId>, DomainError> {
        platform_member_store::platform_membership_id_by_auth_subject(&self.pool, &auth_subject.0)
            .await
    }
}
