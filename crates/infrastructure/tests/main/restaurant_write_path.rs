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

fn admin_actor() -> Actor {
    Actor {
        user_id: uuid::Uuid::new_v4(),
        user_type: "ADMIN".to_string(),
        domain_id: None,
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
    let Some(db) = crate::common::TestDb::acquire("restaurant_write_path").await else { return };
    let pool = db.pool();

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
        String,
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
    assert_eq!(user_type, "ADMIN");
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
    let (slug, display_name, status, listing_status): (Option<String>, String, String, String) =
        sqlx::query_as(
            "SELECT slug, display_name, status, listing_status FROM restaurant WHERE restaurant_id = $1",
        )
        .bind(restaurant_id)
        .fetch_one(&pool)
        .await
        .expect("projected restaurant row");
    assert_eq!(slug, None, "no storefront address until one is configured (#220)");
    assert_eq!(display_name, "Chez Marco");
    assert_eq!(status, "DRAFT"); // RestaurantStatus::DRAFT
    assert_eq!(listing_status, "NON_PARTNER"); // RestaurantListingStatus::NON_PARTNER

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
    let status_after: String =
        sqlx::query_scalar("SELECT status FROM restaurant WHERE restaurant_id = $1")
            .bind(restaurant_id)
            .fetch_one(&pool)
            .await
            .expect("status after activation");
    assert_eq!(status_after, "ACTIVE"); // RestaurantStatus::ACTIVE
}
