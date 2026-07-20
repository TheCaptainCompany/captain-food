//! End-to-end test for the ACCEPTANCE-FIRST GraphQL write path (ADR-20260720-015500): a
//! `registerRestaurant` mutation executed against the real schema (generated MutationRoot) journals
//! the command, returns the uniform `MutationAcceptance` (PENDING), and the spawned handler appends
//! the `domain_events` row — with `correlation_id` = the acceptance's correlationId and `cause_id` =
//! its messageId (the causality chain, ADR-20260720-015300). Outcomes are read by polling
//! `operationStatus` (ownership-scoped): a business rejection surfaces as `Operation.errorCode` (the
//! async P-10 home), NOT as a GraphQL error. Also proves the idempotency contract (same messageId +
//! same payload → `duplicate: true` with the original's status; a different payload → the
//! synchronous Conflict) and the session ownership scope. Needs a real Postgres: set `DATABASE_URL`
//! (e.g. a throwaway `docker run -e POSTGRES_PASSWORD=postgres -p 5433:5432 postgres:16-alpine`,
//! then `DATABASE_URL=postgres://postgres:postgres@localhost:5433/postgres?sslmode=disable`).
//! Without it the test SKIPS (prints and returns) so `cargo test` stays green offline.

use std::sync::Arc;

use application::ports::{
    AuthProviderGateway, EventStore, GbpOrderLinkProbe, GoogleOwnershipVerifier, PaymentGateway,
};
use application::queries::{
    CartReadRepository, CatalogReadRepository, CustomerReadRepository, DeliveryReadRepository,
    OrderReadRepository, PricingPolicyReadRepository, ProspectionReadRepository,
    RefundReadRepository, RestaurantReadRepository, UberEstimationPolicyReadRepository,
    UberSplitPolicyReadRepository,
};
use infrastructure::{
    FailClosedAuthProviderGateway, FailClosedGoogleOwnershipVerifier, FailClosedPaymentGateway,
    PgCartRepository, PgCatalogRepository, PgCommandJournal, PgCustomerRepository,
    PgDeliveryRepository, PgEventStore, PgOrderRepository, PgPricingPolicyRepository,
    PgProspectionRepository, PgRefundQueueRepository, PgRestaurantRepository,
    PgUberEstimationPolicyRepository, PgUberSplitPolicyRepository, UnverifiedGbpOrderLinkProbe,
};
use server::graphql_acl::RequestRole;
use sqlx::PgPool;

/// Fresh copies of the tables this slice touches (mirrors restaurant_write_path.rs — the read repos
/// injected into the schema query the `restaurant` projection table, so it must exist; the
/// acceptance-first dispatch writes `command_journal`).
async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS domain_events, restaurant, prospectionpipeline, projection_checkpoint, command_journal CASCADE;
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
        CREATE TABLE command_journal (
          message_id UUID PRIMARY KEY,
          correlation_id UUID NOT NULL,
          cause_id UUID NULL,
          session_id UUID NULL,
          trace_id TEXT NULL,
          user_id UUID NULL,
          user_type INTEGER NOT NULL,
          channel INTEGER NOT NULL,
          command_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          payload_hash TEXT NOT NULL,
          status INTEGER NOT NULL,
          error JSONB NULL,
          received_at TIMESTAMPTZ NOT NULL,
          completed_at TIMESTAMPTZ NULL
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
/// `DATABASE_URL`): read repos + write ports (incl. the command journal + status bus) over the pool.
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
    let refunds: Arc<dyn RefundReadRepository> = Arc::new(PgRefundQueueRepository::new(pool.clone()));
    let event_store: Arc<dyn EventStore> = Arc::new(PgEventStore::new(pool.clone()));
    let ownership: Arc<dyn GoogleOwnershipVerifier> = Arc::new(FailClosedGoogleOwnershipVerifier);
    let gbp_probe: Arc<dyn GbpOrderLinkProbe> = Arc::new(UnverifiedGbpOrderLinkProbe);
    let auth_provider: Arc<dyn AuthProviderGateway> = Arc::new(FailClosedAuthProviderGateway);
    let payments: Arc<dyn PaymentGateway> = Arc::new(FailClosedPaymentGateway);
    let pm_state: Arc<dyn application::pm_state::PaymentProcessStateStore> =
        Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone()));
    let refund_state: Arc<dyn application::pm_state::RefundProcessStateStore> =
        Arc::new(infrastructure::persistence::PgRefundProcessState::new(pool.clone()));
    let journal: Arc<dyn application::journal::CommandJournal> =
        Arc::new(PgCommandJournal::new(pool.clone()));
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
            refunds,
        }),
        Some(server::graphql_schema::WriteDeps {
            event_store,
            ownership,
            gbp_probe,
            auth_provider,
            payments,
            pm_state,
            refund_state,
            journal,
            status_bus: infrastructure::OperationStatusBus::default(),
        }),
        // No event bus: this test exercises the POST write path, not the domain-fact subscriptions.
        None,
    )
}

/// Poll `operationStatus(messageId)` (as `role`, optionally with a session) until non-PENDING;
/// panics after ~5s — the spawned handler must complete.
async fn poll_operation(
    schema: &server::graphql_schema::CaptainSchema,
    message_id: &str,
    role: RequestRole,
    session: Option<uuid::Uuid>,
) -> serde_json::Value {
    for _ in 0..100 {
        let query = format!(
            r#"query {{ operationStatus(input: {{ messageId: "{message_id}" }}) {{ messageId correlationId status errorCode message }} }}"#
        );
        let mut req = async_graphql::Request::new(query).data(role);
        req = req.data(server::graphql_session::SessionHeader(session));
        let resp = schema.execute(req).await;
        assert!(resp.errors.is_empty(), "operationStatus errored: {:?}", resp.errors);
        let data = resp.data.into_json().expect("json data");
        let op = data["operationStatus"].clone();
        if op.is_object() && op["status"] != "PENDING" {
            return op;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("operation {message_id} did not reach a terminal status in time");
}

#[tokio::test]
async fn acceptance_first_write_path_journals_dispatches_and_serves_status() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP acceptance_first_write_path: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;
    let schema = schema_over(&pool);

    // 1) The mutation returns the uniform acceptance (PENDING, not duplicate); the spawned handler
    //    appends the RestaurantRegistered row with correlation_id = acceptance.correlationId and
    //    cause_id = acceptance.messageId (ADR-20260720-015300 causality).
    let restaurant_id = uuid::Uuid::new_v4();
    let mutation = format!(
        r#"mutation {{
            registerRestaurant(input: {{
                restaurantId: "{restaurant_id}",
                slug: "chez-marco",
                displayName: "Chez Marco",
                address: {{ line1: "1 Rue Nationale", postalCode: "37000", city: "Tours", country: "FR" }}
            }}) {{ messageId correlationId sessionId operationStatus duplicate }}
        }}"#
    );
    // registerRestaurant is [ADMIN, RESTAURANT_ACCOUNT] — execute under the ADMIN role path (the ACL
    // guard fails closed to PUBLIC when no role is in the request context, ADR-0006).
    let resp = schema
        .execute(async_graphql::Request::new(mutation.clone()).data(RequestRole::Admin))
        .await;
    assert!(resp.errors.is_empty(), "mutation errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json data");
    let acceptance = &data["registerRestaurant"];
    assert_eq!(acceptance["operationStatus"], "PENDING");
    assert_eq!(acceptance["duplicate"], false);
    let message_id = acceptance["messageId"].as_str().expect("messageId").to_string();
    let correlation_id: uuid::Uuid =
        acceptance["correlationId"].as_str().expect("correlationId").parse().expect("uuid");
    // No metadata supplied → the server defaulted correlationId = messageId (echoed envelope).
    assert_eq!(acceptance["correlationId"], acceptance["messageId"]);

    let op = poll_operation(&schema, &message_id, RequestRole::Admin, None).await;
    assert_eq!(op["status"], "SUCCEEDED", "operation: {op:?}");
    assert!(op["errorCode"].is_null());

    let (stream, event_type, event_correlation, event_cause, payload): (
        String,
        String,
        uuid::Uuid,
        Option<uuid::Uuid>,
        serde_json::Value,
    ) = sqlx::query_as(
        "SELECT stream_name, event_type, correlation_id, cause_id, payload FROM domain_events",
    )
    .fetch_one(&pool)
    .await
    .expect("one event row");
    assert_eq!(stream, format!("Restaurant-{restaurant_id}"));
    assert_eq!(event_type, "RestaurantRegistered");
    assert_eq!(event_correlation, correlation_id, "envelope correlation = acceptance correlationId");
    assert_eq!(
        event_cause.map(|c| c.to_string()).as_deref(),
        Some(message_id.as_str()),
        "domain_events.cause_id = the command's messageId"
    );
    assert_eq!(payload["slug"], serde_json::json!("chez-marco"));
    assert_eq!(payload["listingStatus"], serde_json::json!("NON_PARTNER")); // spec default

    // 2) Idempotent replay: the SAME messageId with the SAME input acknowledges against the original
    //    (duplicate: true, the original's terminal status) and appends nothing new.
    let replayed = format!(
        r#"mutation {{
            registerRestaurant(input: {{
                restaurantId: "{restaurant_id}",
                slug: "chez-marco",
                displayName: "Chez Marco",
                address: {{ line1: "1 Rue Nationale", postalCode: "37000", city: "Tours", country: "FR" }}
            }}, metadata: {{ messageId: "{message_id}" }}) {{ messageId operationStatus duplicate }}
        }}"#
    );
    let resp = schema
        .execute(async_graphql::Request::new(replayed).data(RequestRole::Admin))
        .await;
    assert!(resp.errors.is_empty(), "replay errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json data");
    assert_eq!(data["registerRestaurant"]["duplicate"], true);
    assert_eq!(data["registerRestaurant"]["operationStatus"], "SUCCEEDED");
    assert_eq!(data["registerRestaurant"]["messageId"].as_str(), Some(message_id.as_str()));

    // 3) The SAME messageId with a DIFFERENT payload is a client bug: synchronous Conflict.
    let conflicting = format!(
        r#"mutation {{
            registerRestaurant(input: {{
                restaurantId: "{restaurant_id}",
                slug: "other-slug",
                displayName: "Someone Else",
                address: {{ line1: "1 Rue Nationale", postalCode: "37000", city: "Tours", country: "FR" }}
            }}, metadata: {{ messageId: "{message_id}" }}) {{ messageId }}
        }}"#
    );
    let resp = schema
        .execute(async_graphql::Request::new(conflicting).data(RequestRole::Admin))
        .await;
    assert_eq!(resp.errors.len(), 1, "expected the Conflict: {:?}", resp.errors);
    let ext = resp.errors[0].extensions.as_ref().expect("extensions");
    assert_eq!(ext.get("code"), Some(&async_graphql::Value::from("Conflict")));
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM domain_events")
        .fetch_one(&pool)
        .await
        .expect("count events");
    assert_eq!(events, 1, "replay + conflict appended nothing");

    // 4) A business rejection is ASYNC (ADR-20260720-015500): the mutation still accepts (PENDING),
    //    and the rejection surfaces as Operation.errorCode + the interpolated catalogued message.
    let missing = uuid::Uuid::new_v4();
    let resp = schema
        .execute(
            async_graphql::Request::new(format!(
                r#"mutation {{ activateRestaurant(input: {{ restaurantId: "{missing}" }}) {{ messageId operationStatus }} }}"#
            ))
            .data(RequestRole::Admin),
        )
        .await;
    assert!(resp.errors.is_empty(), "acceptance must not error: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json data");
    assert_eq!(data["activateRestaurant"]["operationStatus"], "PENDING");
    let rejected_id = data["activateRestaurant"]["messageId"].as_str().expect("messageId").to_string();
    let op = poll_operation(&schema, &rejected_id, RequestRole::Admin, None).await;
    assert_eq!(op["status"], "REJECTED", "operation: {op:?}");
    assert_eq!(op["errorCode"], "RestaurantNotFound");
    assert_eq!(op["message"], "Restaurant not found.", "the interpolated catalogued en template");
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM domain_events")
        .fetch_one(&pool)
        .await
        .expect("count events");
    assert_eq!(events, 1, "rejection appended nothing");

    // 5) Ownership scope: an anonymous session journals under its X-SESSION-ID; the operation is
    //    visible to THAT session (and ADMIN), and resolves null for another session (no oracle).
    let session_a = uuid::Uuid::new_v4();
    let session_b = uuid::Uuid::new_v4();
    let cart_id = uuid::Uuid::new_v4();
    let anon = format!(
        r#"mutation {{ addCartLine(input: {{ cartId: "{cart_id}", restaurantId: "{restaurant_id}", sessionId: "{session_a}", line: {{ cartLineId: "{}", offerId: "{}", quantity: 1 }} }}) {{ messageId sessionId }} }}"#,
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
    );
    let resp = schema
        .execute(
            async_graphql::Request::new(anon)
                .data(RequestRole::Public)
                .data(server::graphql_session::SessionHeader(Some(session_a))),
        )
        .await;
    assert!(resp.errors.is_empty(), "anonymous mutation errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json data");
    let anon_message = data["addCartLine"]["messageId"].as_str().expect("messageId").to_string();
    assert_eq!(data["addCartLine"]["sessionId"], session_a.to_string(), "session echoed");

    // Owner session sees it (terminal: the offer doesn't exist → REJECTED, which is fine — the
    // point is visibility); a stranger session gets null; ADMIN sees it too.
    let op = poll_operation(&schema, &anon_message, RequestRole::Public, Some(session_a)).await;
    assert!(op["errorCode"].is_string(), "cart line against a fake offer rejects: {op:?}");
    let stranger = schema
        .execute(
            async_graphql::Request::new(format!(
                r#"query {{ operationStatus(input: {{ messageId: "{anon_message}" }}) {{ status }} }}"#
            ))
            .data(RequestRole::Public)
            .data(server::graphql_session::SessionHeader(Some(session_b))),
        )
        .await;
    assert!(stranger.errors.is_empty());
    assert!(
        stranger.data.into_json().expect("json")["operationStatus"].is_null(),
        "another session must not see the operation"
    );
    let admin = schema
        .execute(
            async_graphql::Request::new(format!(
                r#"query {{ operationStatus(input: {{ messageId: "{anon_message}" }}) {{ status }} }}"#
            ))
            .data(RequestRole::Admin),
        )
        .await;
    assert!(admin.errors.is_empty());
    assert!(
        admin.data.into_json().expect("json")["operationStatus"].is_object(),
        "ADMIN sees every operation"
    );
}
