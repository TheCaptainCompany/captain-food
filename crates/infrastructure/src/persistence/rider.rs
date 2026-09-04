//! sqlx read adapter over the `rider` projection's identity bridge (#639 part C step 2b): the one
//! reader `rider_store.rs` announced — `auth_ref -> rider_id, standing`, for the request seam
//! (`application::queries::RiderIdentityRepository`).
//!
//! Two properties are not negotiable and are stated on the port too: it selects `rider_id,
//! standing` and NOTHING else (the table answers *who this connection is* and, since #639 part C
//! step 4-i, the platform's GRANT test — `standing` — never *what it may see*), and it never
//! `LIMIT 1`s — the `UNIQUE` on `auth_ref` is what lets `fetch_optional` be written without one.
//! `status` sits in the same row and stays UNREAD here — a `SUSPENDED => deny` check would be a
//! second, wrong authorization path; the real one is `standing`, folded independently.
//!
//! Beside `customer.rs`'s `by_auth_ref` on purpose, not folded into it: that port is still typed
//! `ExternalReference` (step 1b, #836, retypes it); this one is born `AuthSubject`.

use application::queries::RiderIdentityRepository;
use async_trait::async_trait;
use domain::generated::scalars::{AuthSubject, RiderId, RiderStanding};
use domain::shared::errors::DomainError;
use sqlx::PgPool;
use sqlx::Row;

use super::db_err;
use super::enum_sql::EnumText;

/// Postgres adapter for the rider identity bridge.
pub struct PgRiderRepository {
    pool: PgPool,
}

impl PgRiderRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RiderIdentityRepository for PgRiderRepository {
    async fn rider_id_by_auth_subject(
        &self,
        auth_subject: AuthSubject,
    ) -> Result<Option<(RiderId, RiderStanding)>, DomainError> {
        // One btree probe on the UNIQUE constraint's own index; one read, two columns out.
        let row = sqlx::query("SELECT rider_id, standing FROM rider WHERE auth_ref = $1")
            .bind(auth_subject.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        let Some(row) = row else { return Ok(None) };
        let id: uuid::Uuid = row.try_get("rider_id").map_err(db_err)?;
        let standing: RiderStanding = EnumText::from_text(&row.try_get::<String, _>("standing").map_err(db_err)?)?;
        Ok(Some((RiderId(id), standing)))
    }
}
