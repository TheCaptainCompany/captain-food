//! Postgres `SlugReservationRepository` — the write-side arbiter of storefront-slug uniqueness
//! (ADR-20260728-011344 D3).
//!
//! The whole point is that **Postgres decides, not us**. `reserve` is a single
//! `INSERT … ON CONFLICT DO NOTHING` against a table whose primary key IS the slug: exactly one of two
//! concurrent claims inserts a row, and the loser gets `rows_affected == 0`. No read-then-write window
//! exists, so the outcome cannot be raced — which is precisely what a lookup against the eventually
//! consistent `Restaurant` projection could not promise.
//!
//! RELEASED IS NOT FREE. `release` stamps `released_at` and keeps the row, so a label a restaurant
//! renamed away from stays un-claimable. Freeing it would let a competitor take the old address and
//! inherit the 301 the alias read model serves from it — along with its printed menus, QR codes and
//! search results.

use application::queries::SlugReservationRepository;
use async_trait::async_trait;
use domain::generated::scalars::{RestaurantId, Slug};
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};

use super::db_err;

pub struct PgSlugReservationRepository {
    pool: PgPool,
}

impl PgSlugReservationRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SlugReservationRepository for PgSlugReservationRepository {
    async fn reserve(&self, slug: Slug, restaurant_id: RestaurantId) -> Result<bool, DomainError> {
        // One statement, no read-then-write window. `DO NOTHING` rather than `DO UPDATE` because a row
        // held by someone else must NOT be overwritten -- losing is the whole signal.
        let inserted = sqlx::query(
            "INSERT INTO slug_reservations (slug, restaurant_id, reserved_at, released_at) \
             VALUES ($1, $2, now(), NULL) ON CONFLICT (slug) DO NOTHING",
        )
        .bind(slug.0.clone())
        .bind(restaurant_id.0)
        .execute(&self.pool)
        .await
        .map_err(db_err)?
        .rows_affected();
        if inserted == 1 {
            return Ok(true);
        }
        // We lost the insert. That is a conflict UNLESS the existing row is already ours -- a replay of
        // this same reservation, which must stay idempotent (the handler may have crashed between
        // reserving and appending, and the owner simply re-submits).
        let holder: Option<uuid::Uuid> =
            sqlx::query("SELECT restaurant_id FROM slug_reservations WHERE slug = $1")
                .bind(slug.0)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?
                .map(|row| row.try_get("restaurant_id"))
                .transpose()
                .map_err(db_err)?;
        Ok(holder == Some(restaurant_id.0))
    }

    async fn release(&self, slug: Slug, restaurant_id: RestaurantId) -> Result<(), DomainError> {
        // The row STAYS -- only its `released_at` moves. Scoped to the holder so a stray release can
        // never mark someone else's live address as superseded.
        sqlx::query(
            "UPDATE slug_reservations SET released_at = now() \
             WHERE slug = $1 AND restaurant_id = $2 AND released_at IS NULL",
        )
        .bind(slug.0)
        .bind(restaurant_id.0)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(db_err)
    }
}
