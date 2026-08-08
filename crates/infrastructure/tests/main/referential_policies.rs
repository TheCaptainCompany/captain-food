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

#[tokio::test]
async fn seeded_policy_tables_round_trip_through_the_read_repositories() {
    let Some(db) = crate::common::TestDb::acquire("referential_policies").await else { return };
    let pool = db.pool();

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
