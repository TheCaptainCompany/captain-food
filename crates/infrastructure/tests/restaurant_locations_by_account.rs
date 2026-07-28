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

/// Fresh copy of the `restaurant` projection table (mirrors migrations/20260717120000).
async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS restaurant CASCADE;
        CREATE TABLE restaurant (
          restaurant_id UUID PRIMARY KEY,
          restaurant_account_id UUID,
          listing_status INTEGER NOT NULL,
          external_identifiers JSONB,
          google_place_id TEXT,
          slug TEXT NOT NULL UNIQUE,
          display_name TEXT NOT NULL,
          description TEXT,
          tags JSONB,
          margin_rate TEXT,
          cuisine_category INTEGER,
          uber_prices_opt_in BOOLEAN,
          website TEXT,
          rating TEXT,
          reviews_count INTEGER,
          gbp_order_url TEXT,
          gbp_link_status INTEGER,
          address JSONB NOT NULL,
          location JSONB,
          opening_hours JSONB NOT NULL,
          status INTEGER NOT NULL,
          order_acceptance INTEGER NOT NULL,
          default_currency TEXT NOT NULL,
          timezone TEXT,
          preparation_time_minutes INTEGER,
          created_at TIMESTAMPTZ NOT NULL,
          updated_at TIMESTAMPTZ NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await
    .expect("reset schema");
}

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
         VALUES ($1, $2, 2, $3, $4, '{\"city\":\"Tours\"}'::jsonb, '[]'::jsonb, 1, 0, 'EUR', \
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
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP by_account_returns_only_the_accounts_locations: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

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
