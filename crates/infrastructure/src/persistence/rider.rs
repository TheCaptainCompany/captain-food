//! sqlx read adapter over the `rider` projection's identity bridge (#639 part C step 2b): the one
//! reader `rider_store.rs` announced — `auth_ref -> rider_id`, for the request seam
//! (`application::queries::RiderIdentityRepository`).
//!
//! Two properties are not negotiable and are stated on the port too: it selects `rider_id` and
//! NOTHING else (the table answers *who this connection is*, never *what it may see*), and it never
//! `LIMIT 1`s — the `UNIQUE` on `auth_ref` is what lets `fetch_optional` be written without one.
//! `status` sits in the same row and will tempt a `SUSPENDED => deny` onto this path; that check
//! belongs in the handler folding the `Rider-{id}` stream (the read model's own `rules:`).
//!
//! Beside `customer.rs`'s `by_auth_ref` on purpose, not folded into it: that port is still typed
//! `ExternalReference` (step 1b, #836, retypes it); this one is born `AuthSubject`.

use application::queries::RiderIdentityRepository;
use async_trait::async_trait;
use domain::generated::scalars::{AuthSubject, RiderId};
use domain::shared::errors::DomainError;
use sqlx::PgPool;

use super::db_err;

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
    ) -> Result<Option<RiderId>, DomainError> {
        // One btree probe on the UNIQUE constraint's own index; one column out.
        let id: Option<uuid::Uuid> =
            sqlx::query_scalar("SELECT rider_id FROM rider WHERE auth_ref = $1")
                .bind(auth_subject.0)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?;
        Ok(id.map(RiderId))
    }
}
