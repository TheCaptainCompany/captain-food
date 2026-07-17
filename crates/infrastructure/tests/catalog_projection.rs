//! Integration test for the Catalog read-side slice: event in `domain_events` → projection worker
//! (Catalog-stream registry group) → materialized `catalog` row → read repository. Needs a real
//! Postgres: set `DATABASE_URL` (see restaurant_projection.rs for a throwaway docker one-liner).
//! Without it the test SKIPS (prints and returns) so `cargo test` stays green offline.

use application::queries::CatalogReadRepository as _;
use domain::generated::scalars::RestaurantId;
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
        eprintln!("SKIP catalog_event_folds_into_the_read_model: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

    let catalog_id = uuid::Uuid::new_v4();
    let restaurant_id = uuid::Uuid::new_v4();
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

    let worker = ProjectionWorker::new(pool.clone());
    worker.run_once().await.expect("run_once (created)");
    worker.run_once().await.expect("run_once (no-op)");

    // The row materialized under the group's own 'Catalog' checkpoint.
    let checkpoint: i64 =
        sqlx::query_scalar("SELECT position FROM projection_checkpoint WHERE projector = 'Catalog'")
            .fetch_one(&pool)
            .await
            .expect("Catalog checkpoint");
    assert_eq!(checkpoint, 1);

    let status_snapshot = worker.status();
    {
        let st = status_snapshot.lock().unwrap();
        assert_eq!((st.checkpoint, st.head, st.lag), (1, 1, 0));
        assert!(st.last_error.is_none());
    }

    // The read repository serves the projected metadata. The `tree` stays the empty object and `slug`
    // the empty string — both documented projector holes (the tree merge is TODO(runtime);
    // CatalogCreated carries no slug, TODO(spec)) — while the mechanical columns fold for real.
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
    assert_eq!(row.tree, serde_json::json!({}));
    assert_eq!(row.created_at, row.updated_at);

    let absent = repo
        .by_restaurant(RestaurantId(uuid::Uuid::new_v4()))
        .await
        .expect("by_restaurant (absent)");
    assert!(absent.is_none());
}
