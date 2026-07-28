//! Integration test for the Catalog read-side slice: events in `domain_events` → projection worker
//! (Catalog-stream registry group) → materialized `catalog` row (the assembled
//! category→product→offer tree with its DERIVED per-offer `stockStatus`) → read repository, including
//! the offer-level `offer_by_id` port the Cart line invariants read
//! (rules.yaml#/CatalogProductManagement, #/CatalogCategoryTreeManagement, #/OfferStockManualOrSynced,
//! #/CartPricedFromLiveCatalog). Needs a real Postgres: set `DATABASE_URL` (see
//! restaurant_projection.rs for a throwaway docker one-liner). Without it the test SKIPS (prints and
//! returns) so `cargo test` stays green offline.

use application::queries::CatalogReadRepository as _;
use domain::generated::scalars::{CatalogItemAvailability, OfferId, RestaurantId, StockStatus};
use infrastructure::{PgCatalogRepository, ProjectionWorker};
use sqlx::PgPool;

/// Fresh copies of the three tables the slice touches (mirrors migrations/20260717120000 + …170000).
async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS domain_events, catalog, projection_checkpoint CASCADE;
        CREATE TABLE domain_events (
          position BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
          id UUID NOT NULL UNIQUE,
          stream_name TEXT NOT NULL,
          version INTEGER NOT NULL,
          user_id UUID NOT NULL,
          user_type INTEGER NOT NULL,
          correlation_id UUID NOT NULL,
          cause_id UUID NULL,
          event_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          metadata JSONB NULL,
          occurred_at TIMESTAMPTZ NOT NULL,
          expired_at TIMESTAMPTZ NULL,
          UNIQUE (stream_name, version)
        );
        CREATE TABLE catalog (
          catalog_id UUID PRIMARY KEY,
          restaurant_id UUID NOT NULL,
          slug TEXT NOT NULL,
          name TEXT NOT NULL,
          tree JSONB NOT NULL,
          created_at TIMESTAMPTZ NOT NULL,
          updated_at TIMESTAMPTZ NOT NULL
        );
        CREATE TABLE projection_checkpoint (
          projector  TEXT        PRIMARY KEY,
          position   BIGINT      NOT NULL DEFAULT 0,
          updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );
        "#,
    )
    .execute(pool)
    .await
    .expect("reset schema");
}

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

#[tokio::test]
async fn catalog_event_folds_into_the_read_model() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP catalog_event_folds_into_the_read_model: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

    let catalog_id = uuid::Uuid::new_v4();
    let restaurant_id = uuid::Uuid::new_v4();
    let category_id = uuid::Uuid::new_v4();
    let product_id = uuid::Uuid::new_v4();
    let offer_id = uuid::Uuid::new_v4();
    let option_list_id = uuid::Uuid::new_v4();
    let stream = format!("Catalog-{catalog_id}");

    // The creation fact, camelCase payload matching domain::generated::events::CatalogCreated.
    append_event(
        &pool,
        &stream,
        1,
        "CatalogCreated",
        serde_json::json!({
            "catalogId": catalog_id,
            "ref": "hubrise-cat-1",
            "restaurantId": restaurant_id,
            "name": "Main menu"
        }),
    )
    .await;
    // Content facts: a category, a product with one offer (+ option-list binding), the option list,
    // and a HubRise-style inventory sync — the tree fold assembles them all.
    append_event(
        &pool,
        &stream,
        2,
        "CatalogCategoryAdded",
        serde_json::json!({
            "catalogId": catalog_id,
            "restaurantId": restaurant_id,
            "category": { "id": category_id, "catalogId": catalog_id, "name": "Pizzas" }
        }),
    )
    .await;
    append_event(
        &pool,
        &stream,
        3,
        "ProductAdded",
        serde_json::json!({
            "catalogId": catalog_id,
            "restaurantId": restaurant_id,
            "product": {
                "id": product_id,
                "catalogId": catalog_id,
                "restaurantId": restaurant_id,
                "name": "Margherita",
                "taxRate": { "delivery": 10.0 },
                "offers": [{
                    "id": offer_id,
                    "productId": product_id,
                    "name": "Regular",
                    "price": { "amountCents": 980, "currency": "EUR" },
                    "availability": "AVAILABLE",
                    "optionListIds": [option_list_id]
                }]
            }
        }),
    )
    .await;
    append_event(
        &pool,
        &stream,
        4,
        "OptionListAdded",
        serde_json::json!({
            "catalogId": catalog_id,
            "restaurantId": restaurant_id,
            "optionList": {
                "id": option_list_id,
                "name": "Sauces",
                "minSelections": 0,
                "maxSelections": 2,
                "multipleSelection": false,
                "options": []
            }
        }),
    )
    .await;
    append_event(
        &pool,
        &stream,
        5,
        "OfferStockUpdated",
        serde_json::json!({
            "catalogId": catalog_id,
            "restaurantId": restaurant_id,
            "offerId": offer_id,
            // quantity 1 ≤ lowStockThreshold 2 → the projector DERIVES LOW_STOCK (the carried
            // status is deliberately wrong to prove the re-derivation).
            "stock": { "quantity": 1.0, "lowStockThreshold": 2.0, "status": "IN_STOCK" }
        }),
    )
    .await;

    let worker = ProjectionWorker::new(pool.clone());
    worker.run_once().await.expect("run_once (folded)");
    worker.run_once().await.expect("run_once (no-op)");

    // The row materialized under the group's own 'Catalog' checkpoint.
    let checkpoint: i64 =
        sqlx::query_scalar("SELECT position FROM projection_checkpoint WHERE projector = 'Catalog'")
            .fetch_one(&pool)
            .await
            .expect("Catalog checkpoint");
    assert_eq!(checkpoint, 5);

    let status_snapshot = worker.status();
    {
        let st = status_snapshot.lock().unwrap();
        assert_eq!((st.checkpoint, st.head, st.lag), (5, 5, 0));
        assert!(st.last_error.is_none());
    }

    // The read repository serves the projected metadata + the assembled tree (the shape the GraphQL
    // `catalog`/`categories` queries deserialize). `slug` stays the empty string — the documented
    // projector hole (CatalogCreated carries no slug, TODO(spec)).
    let repo = PgCatalogRepository::new(pool.clone());
    let row = repo
        .by_restaurant(RestaurantId(restaurant_id))
        .await
        .expect("by_restaurant")
        .expect("catalog exists for restaurant");
    assert_eq!(row.catalog_id.0, catalog_id);
    assert_eq!(row.restaurant_id.0, restaurant_id);
    assert_eq!(row.name.0, "Main menu");
    assert_eq!(row.slug.0, "");
    assert!(row.created_at <= row.updated_at, "content events stamped updated_at");
    assert_eq!(row.tree["categories"][0]["name"], serde_json::json!("Pizzas"));
    assert_eq!(row.tree["products"][0]["name"], serde_json::json!("Margherita"));
    let offer_node = &row.tree["products"][0]["offers"][0];
    assert_eq!(offer_node["price"]["amountCents"], serde_json::json!(980));
    assert_eq!(offer_node["availability"], serde_json::json!("AVAILABLE"));
    assert_eq!(offer_node["stockStatus"], serde_json::json!("LOW_STOCK"), "derived, not carried");
    assert_eq!(row.tree["optionLists"][0]["name"], serde_json::json!("Sauces"));

    // The offer-level read port (the Cart line invariants' view of the same row): availability,
    // derived stock status/quantity, live price and the option-list constraints.
    let offer_view = repo
        .offer_by_id(RestaurantId(restaurant_id), OfferId(offer_id))
        .await
        .expect("offer_by_id")
        .expect("offer resolved from the tree");
    assert_eq!(offer_view.availability, CatalogItemAvailability::AVAILABLE);
    assert_eq!(offer_view.stock_status, StockStatus::LOW_STOCK);
    assert_eq!(offer_view.stock_quantity.map(|q| q.0), Some(1.0));
    assert_eq!(offer_view.price.amount_cents.0, 980);
    assert_eq!(offer_view.product_name.0, "Margherita");
    assert_eq!(offer_view.option_lists.len(), 1);
    assert_eq!(offer_view.option_lists[0].max_selections, Some(2));
    let missing_offer = repo
        .offer_by_id(RestaurantId(restaurant_id), OfferId(uuid::Uuid::new_v4()))
        .await
        .expect("offer_by_id (absent)");
    assert!(missing_offer.is_none());

    let absent = repo
        .by_restaurant(RestaurantId(uuid::Uuid::new_v4()))
        .await
        .expect("by_restaurant (absent)");
    assert!(absent.is_none());
}
