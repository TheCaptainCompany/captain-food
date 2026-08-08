//! Integration test for the `restaurantLocationsByAccount` read slice: rows in the materialized
//! `restaurant` projection table → `RestaurantReadRepository::by_account` (the SQL predicate over
//! `restaurant_account_id`). Needs a real Postgres: set `DATABASE_URL` (see restaurant_projection.rs
//! for a throwaway docker one-liner). Without it the test SKIPS (prints and returns) so `cargo test`
//! stays green offline.
//!
//! One test function on purpose: the table is shared state, so the scenario must run sequentially.

use application::queries::RestaurantReadRepository as _;
use domain::generated::scalars::RestaurantAccountId;
use infrastructure::PgRestaurantRepository;
use sqlx::PgPool;

/// Insert a minimal projected restaurant row (only the NOT NULL columns + the account under test).
async fn seed_restaurant(
    pool: &PgPool,
    id: uuid::Uuid,
    account_id: Option<uuid::Uuid>,
    slug: &str,
    created_offset_minutes: i64,
) {
    sqlx::query(
        "INSERT INTO restaurant \
         (restaurant_id, restaurant_account_id, listing_status, slug, display_name, address, \
          opening_hours, status, order_acceptance, default_currency, created_at, updated_at) \
         VALUES ($1, $2, 'ACTIVE_PARTNER', $3, $4, '{\"city\":\"Tours\"}'::jsonb, '[]'::jsonb, 'ACTIVE', 'NORMAL', 'EUR', \
                 now() + make_interval(mins => $5), now())",
    )
    .bind(id)
    .bind(account_id)
    .bind(slug)
    .bind(format!("Restaurant {slug}"))
    .bind(created_offset_minutes as i32)
    .execute(pool)
    .await
    .expect("seed restaurant");
}

#[tokio::test]
async fn by_account_returns_only_the_accounts_locations() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_locations_by_account").await else { return };
    let pool = db.pool();

    let account = uuid::Uuid::new_v4();
    let other_account = uuid::Uuid::new_v4();
    let (a1, a2, b1, unlisted) =
        (uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    seed_restaurant(&pool, a1, Some(account), "chez-nous-centre", 0).await;
    seed_restaurant(&pool, a2, Some(account), "chez-nous-nord", 5).await;
    seed_restaurant(&pool, b1, Some(other_account), "other-place", 10).await;
    seed_restaurant(&pool, unlisted, None, "non-partner-listing", 15).await;

    let repo = PgRestaurantRepository::new(pool.clone());
    let rows = repo.by_account(RestaurantAccountId(account)).await.expect("by_account");

    // Exactly the two locations under the account, newest-first, each carrying the account id.
    assert_eq!(
        rows.iter().map(|r| r.restaurant_id.0).collect::<Vec<_>>(),
        vec![a2, a1],
        "the account's two locations, newest first"
    );
    assert!(rows.iter().all(|r| r.restaurant_account_id == Some(RestaurantAccountId(account))));

    // A different account and an unknown account see only their own (or no) locations.
    let other = repo.by_account(RestaurantAccountId(other_account)).await.expect("by_account");
    assert_eq!(other.iter().map(|r| r.restaurant_id.0).collect::<Vec<_>>(), vec![b1]);
    assert!(repo
        .by_account(RestaurantAccountId(uuid::Uuid::new_v4()))
        .await
        .expect("by_account")
        .is_empty());
}
