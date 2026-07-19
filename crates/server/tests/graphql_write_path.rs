//! End-to-end test for the GraphQL WRITE path: a `registerRestaurant` mutation executed against the
//! real schema (generated MutationRoot) → command handler → `PgEventStore` → a `domain_events` row,
//! with the payload returning the envelope's `correlationId`. Also proves an invariant rejection
//! surfaces the structured errors.yaml contract through GraphQL — `extensions.code` = the stable
//! PascalCase code, the message = the interpolated catalogued `en` template, the typed context under
//! the extensions (P-10). Needs a real Postgres: set `DATABASE_URL` (e.g. a
//! throwaway `docker run -e POSTGRES_PASSWORD=postgres -p 5433:5432 postgres:16-alpine`, then
//! `DATABASE_URL=postgres://postgres:postgres@localhost:5433/postgres?sslmode=disable`). Without it
//! the test SKIPS (prints and returns) so `cargo test` stays green offline.

use std::sync::Arc;

use application::ports::{
    AuthProviderGateway, EventStore, GbpOrderLinkProbe, GoogleOwnershipVerifier, PaymentGateway,
};
use application::queries::{
    CartReadRepository, CatalogReadRepository, CustomerReadRepository, DeliveryReadRepository,
    OrderReadRepository, PricingPolicyReadRepository, ProspectionReadRepository,
    RestaurantReadRepository, UberEstimationPolicyReadRepository, UberSplitPolicyReadRepository,
};
use infrastructure::{
    FailClosedAuthProviderGateway, FailClosedGoogleOwnershipVerifier, FailClosedPaymentGateway,
    PgCartRepository,
    PgCatalogRepository, PgCustomerRepository, PgDeliveryRepository, PgEventStore,
    PgOrderRepository, PgPricingPolicyRepository, PgProspectionRepository, PgRestaurantRepository,
    PgUberEstimationPolicyRepository, PgUberSplitPolicyRepository, UnverifiedGbpOrderLinkProbe,
};
use sqlx::PgPool;

/// Fresh copies of the tables this slice touches (mirrors restaurant_write_path.rs — the read repos
/// injected into the schema query the `restaurant` projection table, so it must exist).
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

/// The composition-root wiring, materialized for the test (what `server::router()` builds from
/// `DATABASE_URL`): read repos + write ports over the same pool.
fn schema_over(pool: &PgPool) -> server::graphql_schema::CaptainSchema {
    let restaurants: Arc<dyn RestaurantReadRepository> =
        Arc::new(PgRestaurantRepository::new(pool.clone()));
    let prospection: Arc<dyn ProspectionReadRepository> =
        Arc::new(PgProspectionRepository::new(pool.clone()));
    let pricing_policy: Arc<dyn PricingPolicyReadRepository> =
        Arc::new(PgPricingPolicyRepository::new(pool.clone()));
    let uber_estimation_policy: Arc<dyn UberEstimationPolicyReadRepository> =
        Arc::new(PgUberEstimationPolicyRepository::new(pool.clone()));
    let uber_split_policy: Arc<dyn UberSplitPolicyReadRepository> =
        Arc::new(PgUberSplitPolicyRepository::new(pool.clone()));
    let catalogs: Arc<dyn CatalogReadRepository> = Arc::new(PgCatalogRepository::new(pool.clone()));
    let carts: Arc<dyn CartReadRepository> = Arc::new(PgCartRepository::new(pool.clone()));
    let orders: Arc<dyn OrderReadRepository> = Arc::new(PgOrderRepository::new(pool.clone()));
    let customers: Arc<dyn CustomerReadRepository> = Arc::new(PgCustomerRepository::new(pool.clone()));
    let deliveries: Arc<dyn DeliveryReadRepository> = Arc::new(PgDeliveryRepository::new(pool.clone()));
    let event_store: Arc<dyn EventStore> = Arc::new(PgEventStore::new(pool.clone()));
    let ownership: Arc<dyn GoogleOwnershipVerifier> = Arc::new(FailClosedGoogleOwnershipVerifier);
    let gbp_probe: Arc<dyn GbpOrderLinkProbe> = Arc::new(UnverifiedGbpOrderLinkProbe);
    let auth_provider: Arc<dyn AuthProviderGateway> = Arc::new(FailClosedAuthProviderGateway);
    let payments: Arc<dyn PaymentGateway> = Arc::new(FailClosedPaymentGateway);
    let pm_state: Arc<dyn application::pm_state::PaymentProcessStateStore> =
        Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone()));
    let refund_state: Arc<dyn application::pm_state::RefundProcessStateStore> =
        Arc::new(infrastructure::persistence::PgRefundProcessState::new(pool.clone()));
    server::graphql_schema::build_schema(
        Some(server::graphql_schema::ReadDeps {
            restaurants,
            prospection,
            pricing_policy,
            uber_estimation_policy,
            uber_split_policy,
            catalogs,
            carts,
            orders,
            customers,
            deliveries,
        }),
        Some(server::graphql_schema::WriteDeps {
            event_store,
            ownership,
            gbp_probe,
            auth_provider,
            payments,
            pm_state,
            refund_state,
        }),
        // No event bus: this test exercises the POST write path, not subscriptions.
        None,
    )
}

#[tokio::test]
async fn register_restaurant_mutation_appends_a_domain_event() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP register_restaurant_mutation_appends_a_domain_event: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;
    let schema = schema_over(&pool);

    // 1) The mutation → command handler → event store: one RestaurantRegistered row, and the payload
    //    returns the correlation id stamped on the envelope.
    let restaurant_id = uuid::Uuid::new_v4();
    let mutation = format!(
        r#"mutation {{
            registerRestaurant(input: {{
                restaurantId: "{restaurant_id}",
                slug: "chez-marco",
                displayName: "Chez Marco",
                address: {{ line1: "1 Rue Nationale", postalCode: "37000", city: "Tours", country: "FR" }}
            }}) {{ correlationId }}
        }}"#
    );
    // registerRestaurant is [ADMIN, RESTAURANT_ACCOUNT] — execute under the ADMIN role path (the ACL
    // guard fails closed to PUBLIC when no role is in the request context, ADR-0006).
    let resp = schema
        .execute(async_graphql::Request::new(mutation).data(server::graphql_acl::RequestRole::Admin))
        .await;
    assert!(resp.errors.is_empty(), "mutation errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json data");
    let correlation_id: uuid::Uuid = data["registerRestaurant"]["correlationId"]
        .as_str()
        .expect("correlationId in payload")
        .parse()
        .expect("correlationId is a uuid");

    let (stream, event_type, event_correlation, payload): (String, String, uuid::Uuid, serde_json::Value) =
        sqlx::query_as(
            "SELECT stream_name, event_type, correlation_id, payload FROM domain_events",
        )
        .fetch_one(&pool)
        .await
        .expect("one event row");
    assert_eq!(stream, format!("Restaurant-{restaurant_id}"));
    assert_eq!(event_type, "RestaurantRegistered");
    assert_eq!(event_correlation, correlation_id, "payload correlationId = envelope correlation_id");
    assert_eq!(payload["slug"], serde_json::json!("chez-marco"));
    assert_eq!(payload["listingStatus"], serde_json::json!("NON_PARTNER")); // spec default

    // 2) An invariant rejection surfaces the errors.yaml code through GraphQL, and appends nothing.
    let missing = uuid::Uuid::new_v4();
    let resp = schema
        .execute(
            async_graphql::Request::new(format!(
                r#"mutation {{ activateRestaurant(input: {{ restaurantId: "{missing}" }}) {{ correlationId }} }}"#
            ))
            .data(server::graphql_acl::RequestRole::Admin),
        )
        .await;
    assert_eq!(resp.errors.len(), 1, "expected a rejection: {:?}", resp.errors);
    let ext = resp.errors[0].extensions.as_ref().expect("rejection carries extensions (P-10)");
    assert_eq!(
        ext.get("code"),
        Some(&async_graphql::Value::from("RestaurantNotFound")),
        "extensions.code carries the errors.yaml code: {:?}",
        resp.errors[0]
    );
    assert_eq!(
        ext.get("restaurantId"),
        Some(&async_graphql::Value::from(missing.to_string())),
        "the typed context surfaces under the extensions: {:?}",
        resp.errors[0]
    );
    assert_eq!(
        resp.errors[0].message, "Restaurant not found.",
        "the message is the interpolated catalogued en template"
    );
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM domain_events")
        .fetch_one(&pool)
        .await
        .expect("count events");
    assert_eq!(events, 1, "rejection appended nothing");

    // 3) The Customer vertical resolves its ctx deps (AuthProviderGateway + CustomerReadRepository):
    //    the fail-closed auth stand-in rejects the OTP with the canonical errors.yaml code — proving
    //    the resolver got past dependency resolution (no "data does not exist" context error).
    let customer_id = uuid::Uuid::new_v4();
    let resp = schema
        .execute(
            async_graphql::Request::new(format!(
                r#"mutation {{ verifyPhone(input: {{ customerId: "{customer_id}", dialingCode: "+33", nationalNumber: "0612345678", code: "123456" }}) {{ correlationId customerId created }} }}"#
            ))
            .data(server::graphql_acl::RequestRole::Public),
        )
        .await;
    assert_eq!(resp.errors.len(), 1, "expected the fail-closed rejection: {:?}", resp.errors);
    let ext = resp.errors[0].extensions.as_ref().expect("rejection carries extensions (P-10)");
    assert_eq!(
        ext.get("code"),
        Some(&async_graphql::Value::from("InvalidVerificationCode")),
        "extensions.code carries the errors.yaml code (deps resolved from ctx): {:?}",
        resp.errors[0]
    );
}
