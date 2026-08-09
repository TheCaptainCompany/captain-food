//! Integration test for the Restaurant read-side slice: event in `domain_events` → projection worker →
//! materialized `restaurant` row → read repository. Needs a real Postgres: set `DATABASE_URL` (e.g. a
//! throwaway `docker run -e POSTGRES_PASSWORD=postgres -p 5433:5432 postgres:16-alpine`, then
//! `DATABASE_URL=postgres://postgres:postgres@localhost:5433/postgres?sslmode=disable`). Without it the
//! test SKIPS (prints and returns) so `cargo test` stays green offline.
//!
//! One test function on purpose: the tables are shared state, so the scenario must run sequentially.

use application::ports::RestaurantRepository as _;
use application::queries::{RestaurantFilter, RestaurantReadRepository as _};
use domain::generated::scalars::{
    OrderAcceptanceMode, RestaurantId, RestaurantListingStatus, RestaurantStatus, Slug,
};
use infrastructure::{PgRestaurantRepository, ProjectionWorker};
use sqlx::PgPool;

async fn append_event(
    pool: &PgPool,
    stream_name: &str,
    version: i32,
    event_type: &str,
    payload: serde_json::Value,
) {
    sqlx::query(
        "INSERT INTO domain_events \
         (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, payload, metadata, occurred_at) \
         VALUES ($1, $2, $3, $4, 5, $5, NULL, $6, $7, NULL, now())",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(stream_name)
    .bind(version)
    .bind(uuid::Uuid::nil()) // acting user (ADMIN=5 above) — envelope metadata, ADR-0041
    .bind(uuid::Uuid::new_v4())
    .bind(event_type)
    .bind(payload)
    .execute(pool)
    .await
    .expect("append event");
}

async fn checkpoint(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT position FROM projection_checkpoint WHERE projector = 'Restaurant'")
        .fetch_optional(pool)
        .await
        .expect("read checkpoint")
        .unwrap_or(0)
}

#[tokio::test]
async fn restaurant_event_folds_into_the_read_model() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_projection").await else { return };
    let pool = db.pool();

    let restaurant_id = uuid::Uuid::new_v4();
    let stream = format!("Restaurant-{restaurant_id}");

    // 1) The creation fact, camelCase payload matching domain::generated::events::RestaurantRegistered.
    append_event(
        &pool,
        &stream,
        1,
        "RestaurantRegistered",
        serde_json::json!({
            "restaurantId": restaurant_id,
            "listingStatus": "ACTIVE_PARTNER",
            "displayName": "Chez Marco",
            "marginRate": 62.0,
            "cuisineCategory": "TRADITIONAL",
            "address": {
                "line1": "1 rue Nationale",
                "postalCode": "37000",
                "city": "Tours",
                "country": "FR"
            },
            "openingHours": [{"weekday": "MONDAY", "from": "11:30", "to": "14:00"}],
            "tags": []
        }),
    )
    .await;

    let worker = ProjectionWorker::new(pool.clone());
    worker.run_once().await.expect("run_once (registered)");

    // The row materialized, enums stored as their TEXT values. No slug yet: registration no
    // longer carries one (the #220 slug lifecycle) — the storefront address is configured separately.
    let (slug, display_name, status, listing_status, acceptance): (
        Option<String>,
        String,
        String,
        String,
        String,
    ) = sqlx::query_as(
        "SELECT slug, display_name, status, listing_status, order_acceptance \
             FROM restaurant WHERE restaurant_id = $1",
    )
    .bind(restaurant_id)
    .fetch_one(&pool)
    .await
    .expect("projected restaurant row");
    assert_eq!(slug, None, "a freshly-registered listing has no storefront address yet (#220)");
    assert_eq!(display_name, "Chez Marco");
    assert_eq!(status, "DRAFT"); // RestaurantStatus::DRAFT
    assert_eq!(listing_status, "ACTIVE_PARTNER"); // RestaurantListingStatus::ACTIVE_PARTNER
    assert_eq!(acceptance, "NORMAL"); // OrderAcceptanceMode::NORMAL
    assert_eq!(checkpoint(&pool).await, 1);

    let status_snapshot = worker.status();
    {
        let st = status_snapshot.lock().unwrap();
        assert_eq!((st.checkpoint, st.head, st.lag), (1, 1, 0));
        assert!(st.last_error.is_none());
        assert!(st.last_tick_at.is_some());
    }

    // 2) Follow-up lifecycle facts fold over the existing row (and run_once is idempotent past them):
    //    the storefront address is configured (#220), then the listing goes live.
    append_event(
        &pool,
        &stream,
        2,
        "RestaurantSlugConfigured",
        serde_json::json!({ "restaurantId": restaurant_id, "slug": "chez-marco" }),
    )
    .await;
    append_event(
        &pool,
        &stream,
        3,
        "RestaurantActivated",
        serde_json::json!({ "restaurantId": restaurant_id }),
    )
    .await;
    worker.run_once().await.expect("run_once (slug configured + activated)");
    worker.run_once().await.expect("run_once (no-op)");
    assert_eq!(checkpoint(&pool).await, 3);

    // 3) The read repository sees the folded state through the typed row.
    let repo = PgRestaurantRepository::new(pool.clone());

    let row = repo
        .by_slug(Slug("chez-marco".into()))
        .await
        .expect("by_slug")
        .expect("restaurant exists by slug");
    assert_eq!(row.restaurant_id, RestaurantId(restaurant_id));
    assert_eq!(row.status, RestaurantStatus::ACTIVE);
    assert_eq!(row.listing_status, RestaurantListingStatus::ACTIVE_PARTNER);
    assert_eq!(row.order_acceptance, OrderAcceptanceMode::NORMAL);
    assert_eq!(row.margin_rate.map(|m| m.0), Some(62.0));
    assert_eq!(row.created_at, row.created_at.min(row.updated_at)); // created ≤ updated

    assert!(repo.exists(RestaurantId(restaurant_id)).await.expect("exists"));
    assert!(!repo.exists(RestaurantId(uuid::Uuid::new_v4())).await.expect("exists (absent)"));

    // list(): search + orderable_only (ACTIVE_PARTNER + ACTIVE + acceptance ≠ PAUSED after activation).
    let all = repo.list(RestaurantFilter::default()).await.expect("list all");
    assert_eq!(all.len(), 1);
    let orderable = repo
        .list(RestaurantFilter { search: Some("marco".into()), orderable_only: Some(true), ..Default::default() })
        .await
        .expect("list orderable");
    assert_eq!(orderable.len(), 1);
    let none = repo
        .list(RestaurantFilter { search: Some("zzz".into()), orderable_only: None, ..Default::default() })
        .await
        .expect("list no match");
    assert!(none.is_empty());

    // Pagination (#113): limit clamps to the max, offset skips. With one row: limit=1 returns it,
    // offset=1 skips past it, and an over-max limit is clamped (still returns the single row).
    let page = repo
        .list(RestaurantFilter { limit: Some(1), ..Default::default() })
        .await
        .expect("first page");
    assert_eq!(page.len(), 1);
    let skipped = repo
        .list(RestaurantFilter { offset: Some(1), ..Default::default() })
        .await
        .expect("offset past the only row");
    assert!(skipped.is_empty());
    let clamped = repo
        .list(RestaurantFilter { limit: Some(100_000), ..Default::default() })
        .await
        .expect("over-max limit clamps, never errors");
    assert_eq!(clamped.len(), 1);
}
