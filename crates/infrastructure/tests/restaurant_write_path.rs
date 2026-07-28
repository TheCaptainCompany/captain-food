//! Integration test for the Restaurant WRITE path: command handler → `PgEventStore` append into
//! `domain_events` → projection worker → materialized `restaurant` row. Needs a real Postgres: set
//! `DATABASE_URL` (e.g. a throwaway `docker run -e POSTGRES_PASSWORD=postgres -p 5433:5432
//! postgres:16-alpine`, then `DATABASE_URL=postgres://postgres:postgres@localhost:5433/postgres?sslmode=disable`).
//! Without it the test SKIPS (prints and returns) so `cargo test` stays green offline.
//!
//! One test function on purpose: the tables are shared state, so the scenario must run sequentially.

use application::commands::{register_restaurant, register_restaurant_account};
use application::ports::{Actor, EventStore};
use domain::generated::commands::{RegisterRestaurant, RegisterRestaurantAccount};
use domain::generated::entities::{Address, OpeningHoursSlot, TaxRate};
use domain::generated::events::{DomainEvent, RestaurantActivated};
use domain::generated::scalars::*;
use infrastructure::{PgEventStore, PgRestaurantRepository, ProjectionWorker};
use sqlx::PgPool;

/// Fresh copies of the four tables the slice touches (mirrors migrations/20260717120000 + …170000 and
/// the read-side test's DDL; the worker folds every Restaurant-stream event into `prospectionpipeline`
/// too, so it must exist).
async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS domain_events, restaurant, prospectionpipeline, projection_checkpoint CASCADE;
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
        CREATE TABLE restaurant (
          restaurant_id UUID PRIMARY KEY,
          restaurant_account_id UUID,
          listing_status INTEGER NOT NULL,
          external_identifiers JSONB,
          google_place_id TEXT,
          -- NULLABLE since migrations/20260728020000: a prospect has no slug until one is configured.
          slug TEXT UNIQUE,
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
        CREATE TABLE prospectionpipeline (
          restaurant_id UUID PRIMARY KEY,
          score INTEGER NOT NULL,
          pipeline_status INTEGER NOT NULL,
          contacts_count INTEGER NOT NULL,
          last_contacted_at TIMESTAMPTZ,
          replied_at TIMESTAMPTZ,
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

fn admin_actor() -> Actor {
    Actor {
        user_id: uuid::Uuid::new_v4(),
        user_type: 5, // UserType::ADMIN ordinal
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
    }
}

fn register_restaurant_cmd(restaurant_id: uuid::Uuid) -> RegisterRestaurant {
    RegisterRestaurant {
        mode: None,
        restaurant_id: RestaurantId(restaurant_id),
        account_id: None,
        listing_status: None, // → defaults to NON_PARTNER (sync-seeded listing path)
        display_name: RestaurantDisplayName("Chez Marco".into()),
        contact: None,
        website: None,
        tags: vec![],
        margin_rate: Some(MarginPercent(62.0)),
        cuisine_category: Some(CuisineCategory::TRADITIONAL),
        uber_prices_opt_in: None,
        address: Address {
            line1: AddressLine("1 rue Nationale".into()),
            line2: None,
            postal_code: PostalCode("37000".into()),
            city: CityName("Tours".into()),
            country: CountryCode("FR".into()),
        },
        location: None,
        timezone: Some(TimeZone("Europe/Paris".into())),
        preparation_time_minutes: Some(20),
        opening_hours: vec![OpeningHoursSlot {
            weekday: Weekday::MONDAY,
            from: TimeOfDay("11:30".into()),
            to: TimeOfDay("14:00".into()),
        }],
        external_identifiers: vec![],
        r#ref: None,
    }
}

#[tokio::test]
async fn command_appends_event_and_projects_the_restaurant_row() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP command_appends_event_and_projects_the_restaurant_row: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

    let store = PgEventStore::new(pool.clone());
    let restaurants = PgRestaurantRepository::new(pool.clone()); // backs the SlugAlreadyTaken check
    let actor = admin_actor();
    let restaurant_id = uuid::Uuid::new_v4();
    let account_id = uuid::Uuid::new_v4();
    let stream = format!("Restaurant-{restaurant_id}");

    // 1) RegisterRestaurantAccount → one RestaurantAccountRegistered on its own stream.
    register_restaurant_account(
        &store,
        RegisterRestaurantAccount {
            restaurant_account_id: RestaurantAccountId(account_id),
            legal_name: RestaurantLegalName("SARL Chez Marco".into()),
            contact: None,
            default_currency: CurrencyCode("EUR".into()),
            default_tax_rate: TaxRate {
                delivery: TaxRatePercent(10.0),
                collection: Some(TaxRatePercent(10.0)),
                eat_in: None,
            },
            timezone: None,
            r#ref: None,
        },
        &actor,
    )
    .await
    .expect("register_restaurant_account");

    let (account_stream, account_event_type): (String, String) = sqlx::query_as(
        "SELECT stream_name, event_type FROM domain_events WHERE stream_name LIKE 'RestaurantAccount-%'",
    )
    .fetch_one(&pool)
    .await
    .expect("account event row");
    assert_eq!(account_stream, format!("RestaurantAccount-{account_id}"));
    assert_eq!(account_event_type, "RestaurantAccountRegistered");

    // 2) RegisterRestaurant → one RestaurantRegistered at version 1, business payload + envelope split.
    register_restaurant(&store, &restaurants, register_restaurant_cmd(restaurant_id), &actor)
        .await
        .expect("register_restaurant");

    let (version, user_id, user_type, event_type, payload): (
        i32,
        uuid::Uuid,
        i32,
        String,
        serde_json::Value,
    ) = sqlx::query_as(
        "SELECT version, user_id, user_type, event_type, payload FROM domain_events \
         WHERE stream_name = $1",
    )
    .bind(&stream)
    .fetch_one(&pool)
    .await
    .expect("restaurant event row");
    assert_eq!(version, 1);
    assert_eq!(user_id, actor.user_id); // envelope metadata (ADR-0041)…
    assert_eq!(user_type, 5);
    assert_eq!(event_type, "RestaurantRegistered");
    assert_eq!(payload["restaurantId"], serde_json::json!(restaurant_id));
    // No slug in the fact: registration and the storefront address are separate lifecycles since
    // #220 — a listing gets its slug from ConfigureRestaurantSlug, not from being registered.
    assert!(payload.get("slug").is_none());
    assert_eq!(payload["listingStatus"], serde_json::json!("NON_PARTNER")); // spec default
    assert!(payload.get("userId").is_none()); // …never a payload field
    assert!(payload.get("occurredAt").is_none());

    // 3) The existing projection worker folds it into the materialized `restaurant` table.
    let worker = ProjectionWorker::new(pool.clone());
    worker.run_once().await.expect("run_once");
    let (slug, display_name, status, listing_status): (Option<String>, String, i32, i32) =
        sqlx::query_as(
            "SELECT slug, display_name, status, listing_status FROM restaurant WHERE restaurant_id = $1",
        )
        .bind(restaurant_id)
        .fetch_one(&pool)
        .await
        .expect("projected restaurant row");
    assert_eq!(slug, None, "no storefront address until one is configured (#220)");
    assert_eq!(display_name, "Chez Marco");
    assert_eq!(status, 0); // RestaurantStatus::DRAFT
    assert_eq!(listing_status, 0); // RestaurantListingStatus::NON_PARTNER

    // 4) Idempotent replay: same client-generated id → version clash absorbed as Ok, no duplicate fact.
    register_restaurant(&store, &restaurants, register_restaurant_cmd(restaurant_id), &actor)
        .await
        .expect("register_restaurant replay is idempotent");
    let events_in_stream: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM domain_events WHERE stream_name = $1")
            .bind(&stream)
            .fetch_one(&pool)
            .await
            .expect("count stream events");
    assert_eq!(events_in_stream, 1);

    // 5) Raw port semantics: appending PAST the head (expected_version = 1) succeeds and returns the
    //    new version; a stale expected_version surfaces the canonical version-conflict error.
    let follow_up = DomainEvent::RestaurantActivated(RestaurantActivated {
        restaurant_id: RestaurantId(restaurant_id),
        reason: None,
    });
    let new_version = store
        .append(&stream, 1, std::slice::from_ref(&follow_up), &actor)
        .await
        .expect("append at head");
    assert_eq!(new_version, 2);
    let stale = store.append(&stream, 1, std::slice::from_ref(&follow_up), &actor).await;
    assert!(matches!(&stale, Err(e) if application::ports::is_version_conflict(e)));

    worker.run_once().await.expect("run_once (activated)");
    let status_after: i32 =
        sqlx::query_scalar("SELECT status FROM restaurant WHERE restaurant_id = $1")
            .bind(restaurant_id)
            .fetch_one(&pool)
            .await
            .expect("status after activation");
    assert_eq!(status_after, 1); // RestaurantStatus::ACTIVE
}
