//! Integration test for the referential policy read slice (ADR-0016/0017/0024/0025/0030/0037): the
//! three seeded policy tables → `PgPricingPolicyRepository` / `PgUberEstimationPolicyRepository` /
//! `PgUberSplitPolicyRepository` return the spec values from `specs/database/tables/referential.yaml`.
//! Needs a real Postgres: set `DATABASE_URL` (see restaurant_write_path.rs for a throwaway docker
//! one-liner). Without it the test SKIPS so `cargo test` stays green offline.

use application::queries::{
    PricingPolicyReadRepository, UberEstimationPolicyReadRepository, UberSplitPolicyReadRepository,
};
use domain::generated::scalars::CuisineCategory;
use infrastructure::{
    PgPricingPolicyRepository, PgUberEstimationPolicyRepository, PgUberSplitPolicyRepository,
};
use sqlx::PgPool;

/// Fresh copies of the three referential tables (mirroring the domain-schema migration, where
/// unquoted PascalCase names lowercase to `pricingpolicy`/`uberestimationpolicy`/`ubersplitpolicy`),
/// seeded with the exact rows from the seed migration.
async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS pricingpolicy, uberestimationpolicy, ubersplitpolicy CASCADE;
        CREATE TABLE pricingpolicy (
          currency TEXT PRIMARY KEY,
          fee_rate NUMERIC NOT NULL,
          buyer_share NUMERIC NOT NULL,
          margin_low NUMERIC NOT NULL,
          margin_high NUMERIC NOT NULL,
          effective_from TIMESTAMPTZ NOT NULL
        );
        CREATE TABLE uberestimationpolicy (
          cuisine_category TEXT PRIMARY KEY,
          price_coefficient NUMERIC NOT NULL,
          effective_from TIMESTAMPTZ NOT NULL
        );
        CREATE TABLE ubersplitpolicy (
          currency TEXT PRIMARY KEY,
          uber_commission_pct NUMERIC NOT NULL,
          rider_base_cents INTEGER NOT NULL,
          rider_per_km_cents INTEGER NOT NULL,
          avg_delivery_fee_cents INTEGER NOT NULL,
          platform_fee_pct NUMERIC NOT NULL,
          effective_from TIMESTAMPTZ NOT NULL
        );
        INSERT INTO pricingpolicy (currency, fee_rate, buyer_share, margin_low, margin_high, effective_from)
        VALUES ('EUR', 5.0, 60.0, 55.0, 70.0, TIMESTAMPTZ '2026-01-01 00:00:00+00');
        INSERT INTO uberestimationpolicy (cuisine_category, price_coefficient, effective_from)
        VALUES
          ('FAST_FOOD', 1.30, TIMESTAMPTZ '2026-01-01 00:00:00+00'),
          ('PIZZA', 1.35, TIMESTAMPTZ '2026-01-01 00:00:00+00'),
          ('TRADITIONAL', 1.40, TIMESTAMPTZ '2026-01-01 00:00:00+00'),
          ('BISTRONOMIC', 1.45, TIMESTAMPTZ '2026-01-01 00:00:00+00'),
          ('FOOD_TRUCK', 1.35, TIMESTAMPTZ '2026-01-01 00:00:00+00');
        INSERT INTO ubersplitpolicy (currency, uber_commission_pct, rider_base_cents, rider_per_km_cents,
                                     avg_delivery_fee_cents, platform_fee_pct, effective_from)
        VALUES ('EUR', 30.0, 285, 80, 399, 10.0, TIMESTAMPTZ '2026-01-01 00:00:00+00');
        "#,
    )
    .execute(pool)
    .await
    .expect("reset schema");
}

#[tokio::test]
async fn seeded_policy_tables_round_trip_through_the_read_repositories() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!(
            "SKIP seeded_policy_tables_round_trip_through_the_read_repositories: DATABASE_URL not set"
        );
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

    // PricingPolicy: the single EUR row with the ADR-0017 indicative values.
    let pricing = PgPricingPolicyRepository::new(pool.clone()).list().await.expect("pricing list");
    assert_eq!(pricing.len(), 1);
    let p = &pricing[0];
    assert_eq!(p.currency.0, "EUR");
    assert_eq!(p.fee_rate, 5.0);
    assert_eq!(p.buyer_share, 60.0);
    assert_eq!(p.margin_low, 55.0);
    assert_eq!(p.margin_high, 70.0);

    // UberEstimationPolicy: five rows in stable text order (alphabetical), incl. TRADITIONAL → 1.40.
    let estimation =
        PgUberEstimationPolicyRepository::new(pool.clone()).list().await.expect("estimation list");
    assert_eq!(estimation.len(), 5);
    assert_eq!(estimation[0].cuisine_category, CuisineCategory::BISTRONOMIC);
    assert_eq!(estimation[0].price_coefficient, 1.45);
    let traditional = estimation
        .iter()
        .find(|r| r.cuisine_category == CuisineCategory::TRADITIONAL)
        .expect("TRADITIONAL row");
    assert_eq!(traditional.price_coefficient, 1.40);
    assert_eq!(estimation[4].cuisine_category, CuisineCategory::TRADITIONAL);

    // UberSplitPolicy: the single EUR row with the ADR-0024/0025 assumptions (cents widened to i64).
    let split = PgUberSplitPolicyRepository::new(pool.clone()).list().await.expect("split list");
    assert_eq!(split.len(), 1);
    let s = &split[0];
    assert_eq!(s.currency.0, "EUR");
    assert_eq!(s.uber_commission_pct, 30.0);
    assert_eq!(s.rider_base_cents, 285);
    assert_eq!(s.rider_per_km_cents, 80);
    assert_eq!(s.avg_delivery_fee_cents, 399);
    assert_eq!(s.platform_fee_pct, 10.0);
}
