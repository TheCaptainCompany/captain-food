//! sqlx read adapter for `application::queries::MemberAuthorityRepository` (#639 part C step 6-iv
//! round 2, ADR-20260905-101349 §2 amendment): the `AuthorityGuard`'s source of truth.
//!
//! Deliberately reads the ALREADY-LANDED `member` identity bridge (6-i/6-ii) joined straight to the
//! WRITE-SIDE `domain_events` log itself — NEVER the `RestaurantRoster` projection this round adds.
//! A roster rebuild (checkpoint reset + replay) must never change what this write-path guard
//! accepts mid-drain; `domain_events` is the one artifact that is never mid-rebuild.

use application::queries::MemberAuthorityRepository;
use async_trait::async_trait;
use domain::generated::scalars::{AuthSubject, MemberAuthority, RestaurantId};
use domain::shared::errors::DomainError;
use sqlx::PgPool;
use sqlx::Row;

use super::db_err;

pub struct PgMemberAuthorityRepository {
    pool: PgPool,
}

impl PgMemberAuthorityRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MemberAuthorityRepository for PgMemberAuthorityRepository {
    async fn authority_for_subject(
        &self,
        auth_subject: AuthSubject,
        restaurant_id: RestaurantId,
    ) -> Result<Option<MemberAuthority>, DomainError> {
        // `member.auth_subject` -> `member_id` (the 6-i/6-ii bridge, UNIQUE), joined to the
        // NOT-YET-REVOKED `RestaurantAccessGranted` fact for that member on THIS restaurant scope.
        // `NOT EXISTS` here checks the WRITE-SIDE log's own revocation fact, never a read
        // projection's derived status column -- there is no projection in this query at all.
        let row = sqlx::query(
            "SELECT g.payload->>'authority' AS authority \
             FROM member m \
             JOIN domain_events g \
               ON g.event_type = 'RestaurantAccessGranted' \
              AND g.payload->>'memberId' = m.member_id::text \
              AND g.payload->>'scopeId' = $2 \
             WHERE m.auth_subject = $1 \
               AND NOT EXISTS ( \
                 SELECT 1 FROM domain_events r \
                 WHERE r.event_type = 'RestaurantAccessRevoked' \
                   AND r.stream_name = g.stream_name \
               ) \
             LIMIT 1",
        )
        .bind(auth_subject.0)
        .bind(restaurant_id.0.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        let Some(row) = row else { return Ok(None) };
        let authority: String = row.try_get("authority").map_err(db_err)?;
        Ok(match authority.as_str() {
            "MANAGER" => Some(MemberAuthority::MANAGER),
            "OPERATOR" => Some(MemberAuthority::OPERATOR),
            _ => None,
        })
    }
}
