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

use application::generated::services::{IdentityService, PaymentService};
use application::ports::{EventStore, GbpOrderLinkProbe, GoogleOwnershipVerifier};
use application::queries::{
    CartReadRepository, CatalogReadRepository, CustomerReadRepository,
    DeliverySatisfactionReadRepository, DeliveryReadRepository, OrderReadRepository,
    PricingPolicyReadRepository, ProspectionReadRepository, RefundReadRepository,
    RestaurantReadRepository, UberEstimationPolicyReadRepository, UberSplitPolicyReadRepository,
};
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, FailClosedPaymentGateway,
    PgCartRepository, PgCatalogRepository, PgCommandJournal, PgCustomerRepository,
    PgDeliveryRepository, PgDeliverySatisfactionRepository, PgEventStore, PgOrderRepository,
    PgPricingPolicyRepository, PgProspectionRepository, PgRefundQueueRepository, PgRestaurantRepository,
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
        DROP TABLE IF EXISTS domain_events, restaurant, prospectionpipeline, projection_checkpoint, command_journal, inbound_messages, mailbox_partitions CASCADE;
        DROP SEQUENCE IF EXISTS inbound_messages_position_seq;
        CREATE TABLE domain_events (
          position BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
          id UUID NOT NULL UNIQUE,
          stream_name TEXT NOT NULL,
          version INTEGER NOT NULL,
          user_id UUID NOT NULL,
          user_type TEXT NOT NULL,
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
          user_type TEXT NOT NULL,
          channel TEXT NOT NULL,
          command_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          payload_hash TEXT NOT NULL,
          status TEXT NOT NULL,
          error JSONB NULL,
          received_at TIMESTAMPTZ NOT NULL,
          completed_at TIMESTAMPTZ NULL
        );
        CREATE TABLE restaurant (
          restaurant_id UUID PRIMARY KEY,
          restaurant_account_id UUID,
          listing_status TEXT NOT NULL,
          external_identifiers JSONB,
          google_place_id TEXT,
          -- NULLABLE since migrations/20260728020000: a prospect has no slug until one is configured.
          slug TEXT UNIQUE,
          display_name TEXT NOT NULL,
          description TEXT,
          tags JSONB,
          margin_rate TEXT,
          cuisine_category TEXT,
          uber_prices_opt_in BOOLEAN,
          website TEXT,
          rating TEXT,
          reviews_count INTEGER,
          gbp_order_url TEXT,
          gbp_link_status TEXT,
          address JSONB NOT NULL,
          location JSONB,
          opening_hours JSONB NOT NULL,
          status TEXT NOT NULL,
          order_acceptance TEXT NOT NULL,
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
    // The mailbox era (#242 flip): the aggregate-routed mutations enqueue on inbound_messages and
    // a worker delivers — the REAL migration DDL, so this test and production cannot drift.
    sqlx::raw_sql(include_str!("../../../migrations/20260731063000_actor_mailbox_tables.sql"))
        .execute(pool)
        .await
        .expect("apply the actor-mailbox migration");
}

/// Spawn the production delivery side for the test schema: one MailboxWorker per actor type over
/// the same CommandDeps the composition root builds, on a fast heartbeat so the poll budget holds.
fn spawn_mailbox_workers(pool: &PgPool, bus: infrastructure::OperationStatusBus) {
    let deps = infrastructure::generated::command_router::CommandDeps {
        store: Arc::new(PgEventStore::new(pool.clone())),
        restaurants: Arc::new(PgRestaurantRepository::new(pool.clone())),
        slugs: Arc::new(infrastructure::PgSlugReservationRepository::new(pool.clone())),
        ownership: Arc::new(FailClosedGoogleOwnershipVerifier),
        probe: Arc::new(UnverifiedGbpOrderLinkProbe),
        prospection: Arc::new(PgProspectionRepository::new(pool.clone())),
        catalogs: Arc::new(PgCatalogRepository::new(pool.clone())),
        auth: Arc::new(FailClosedIdentityService),
        customers: Arc::new(PgCustomerRepository::new(pool.clone())),
        sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
        payments: Arc::new(FailClosedPaymentGateway),
        pm_state: Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone())),
        refund_state: Arc::new(infrastructure::persistence::PgRefundProcessState::new(pool.clone())),
    };
    let handler = Arc::new(infrastructure::mailbox::MailboxCommandHandler::new(deps));
    let observer = Arc::new(infrastructure::mailbox::StatusBusObserver::new(bus));
    for (actor_type, width) in infrastructure::generated::command_router::ACTOR_MAILBOXES {
        let worker = actor_runtime::MailboxWorker::new(
            pool.clone(),
            "w-test",
            *actor_type,
            actor_runtime::WorkerConfig { heartbeat_seconds: 1, ..Default::default() },
            handler.clone(),
        )
        .with_observer(observer.clone());
        let width = *width as i16;
        let (_tx, rx) = tokio::sync::watch::channel(false);
        std::mem::forget(_tx); // keep the shutdown channel open for the test's lifetime
        tokio::spawn(async move {
            worker.seed(width).await.expect("seed");
            let _ = worker.run(rx).await;
        });
    }
}

/// The composition-root wiring, materialized for the test (what `server::router()` builds from
/// `DATABASE_URL`): read repos + write ports (incl. the command journal + status bus) over the pool.
fn schema_over(pool: &PgPool, status_bus: infrastructure::OperationStatusBus) -> server::graphql_schema::CaptainSchema {
    schema_over_with_gate(pool, status_bus, false)
}

/// Same wiring with the Runtime D1 gate chosen by the test (#272 review MAJOR-3).
fn schema_over_with_gate(pool: &PgPool, status_bus: infrastructure::OperationStatusBus, pm_mailbox_delivery: bool) -> server::graphql_schema::CaptainSchema {
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
    let delivery_satisfaction: Arc<dyn DeliverySatisfactionReadRepository> =
        Arc::new(PgDeliverySatisfactionRepository::new(pool.clone()));
    let delivery_partner_availabilities: Arc<dyn application::queries::DeliveryPartnerAvailabilityReadRepository> =
        Arc::new(infrastructure::PgDeliveryPartnerAvailabilityRepository::new(pool.clone()));
    let reclamations: Arc<dyn application::queries::ReclamationReadRepository> =
        Arc::new(infrastructure::PgReclamationRepository::new(pool.clone()));
    let order_conversations: Arc<dyn application::queries::OrderConversationReadRepository> =
        Arc::new(infrastructure::PgOrderConversationRepository::new(pool.clone()));
    let customer_credit: Arc<dyn application::queries::CustomerCreditReadRepository> =
        Arc::new(infrastructure::PgCustomerCreditRepository::new(pool.clone()));
    let mailbox_lanes: Arc<dyn application::queries::MailboxLaneRepository> = Arc::new(
        infrastructure::persistence::mailbox_lanes::PgMailboxLaneRepository::new(pool.clone()),
    );
    let event_store: Arc<dyn EventStore> = Arc::new(PgEventStore::new(pool.clone()));
    let ownership: Arc<dyn GoogleOwnershipVerifier> = Arc::new(FailClosedGoogleOwnershipVerifier);
    let gbp_probe: Arc<dyn GbpOrderLinkProbe> = Arc::new(UnverifiedGbpOrderLinkProbe);
    let auth_provider: Arc<dyn IdentityService> = Arc::new(FailClosedIdentityService);
    let payments: Arc<dyn PaymentService> = Arc::new(FailClosedPaymentGateway);
    let pm_state: Arc<dyn application::pm_state::PaymentProcessStateStore> =
        Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone()));
    let refund_state: Arc<dyn application::pm_state::RefundProcessStateStore> =
        Arc::new(infrastructure::persistence::PgRefundProcessState::new(pool.clone()));
    let journal: Arc<dyn application::journal::CommandJournal> =
        Arc::new(PgCommandJournal::new(pool.clone()));
    let mailbox: Arc<dyn actor_client::mailbox::Mailbox> =
        Arc::new(infrastructure::persistence::mailbox_store::PgMailbox::new(pool.clone()));
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
            order_conversations,
            customers,
            deliveries,
            refunds,
            delivery_satisfaction,
            delivery_partner_availabilities,
            reclamations,
            customer_credit,
            mailbox_lanes,
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
            mailbox,
            status_bus,
            // #112: no session storage needed for the write-path test — the noop store.
            auth_sessions: std::sync::Arc::new(application::auth_sessions::NoopAuthSessionStore),
            // ADR-20260728-011344: this test drives no slug configuration, so a permissive local
            // double is enough (see `AlwaysFreeSlugs` — deliberately NOT reused in production, where
            // the arbiter is a real UNIQUE constraint).
            slug_reservations: std::sync::Arc::new(AlwaysFreeSlugs),
            // Runtime D1 (#272): the flag under test — the legacy arm by default; the cross-arm
            // duplicate test flips it (mailbox delivery itself is covered end-to-end by
            // infrastructure/tests/pm_prepare_delivery.rs).
            pm_mailbox_delivery,
        }),
        // No event bus: this test exercises the POST write path, not the domain-fact subscriptions.
        None,
    )
}

/// A `SlugReservationRepository` that grants every request. Fine here because this test never issues
/// `configureRestaurantSlug` — the field only has to be inhabited. Real uniqueness is arbitrated by the
/// `slug_reservations` UNIQUE constraint (ADR-20260728-011344 D3), and the behaviour tests cover the
/// conflict path against an in-memory double that actually refuses.
struct AlwaysFreeSlugs;

#[async_trait::async_trait]
impl application::queries::SlugReservationRepository for AlwaysFreeSlugs {
    async fn reserve(
        &self,
        _slug: domain::generated::scalars::Slug,
        _restaurant_id: domain::generated::scalars::RestaurantId,
    ) -> Result<bool, domain::shared::errors::DomainError> {
        Ok(true)
    }
    async fn release(
        &self,
        _slug: domain::generated::scalars::Slug,
        _restaurant_id: domain::generated::scalars::RestaurantId,
    ) -> Result<(), domain::shared::errors::DomainError> {
        Ok(())
    }
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

/// Serialize the suite: every test resets the same tables.
static DB_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn acceptance_first_write_path_journals_dispatches_and_serves_status() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP acceptance_first_write_path: DATABASE_URL not set");
        return;
    };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;
    // One shared status bus: the workers publish terminal transitions on it, the schema's
    // operationStatusChanged streams from it.
    let status_bus = infrastructure::OperationStatusBus::default();
    spawn_mailbox_workers(&pool, status_bus.clone());
    let schema = schema_over(&pool, status_bus);

    // 1) The mutation returns the uniform acceptance (PENDING, not duplicate); the spawned handler
    //    appends the RestaurantRegistered row with correlation_id = acceptance.correlationId and
    //    cause_id = acceptance.messageId (ADR-20260720-015300 causality).
    let restaurant_id = uuid::Uuid::new_v4();
    let mutation = format!(
        r#"mutation {{
            registerRestaurant(input: {{
                restaurantId: "{restaurant_id}",
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
    // No slug in the fact: the storefront address is its own lifecycle since #220
    // (ConfigureRestaurantSlug), so registration neither takes nor emits one.
    assert!(payload.get("slug").is_none());
    assert_eq!(payload["listingStatus"], serde_json::json!("NON_PARTNER")); // spec default

    // 2) Idempotent replay: the SAME messageId with the SAME input acknowledges against the original
    //    (duplicate: true, the original's terminal status) and appends nothing new.
    let replayed = format!(
        r#"mutation {{
            registerRestaurant(input: {{
                restaurantId: "{restaurant_id}",
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

/// #272 review MAJOR-3 — cross-arm duplicate identity: a messageId accepted on ONE arm must
/// replay as `duplicate: true` on the OTHER arm (and Conflict on a different payload), never
/// re-execute. Both directions: journal-accepted → mailbox arm (gate ON), mailbox-accepted →
/// legacy arm (gate OFF).
#[tokio::test]
async fn pm_gate_cross_arm_duplicate_replays_instead_of_reexecuting() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP pm_gate_cross_arm_duplicate: DATABASE_URL not set");
        return;
    };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;
    let status_bus = infrastructure::OperationStatusBus::default();

    // --- Direction 1: accepted on the LEGACY arm (command_journal), retried on the MAILBOX arm.
    use application::journal::CommandJournal as _;
    let journal = PgCommandJournal::new(pool.clone());
    let order_id = uuid::Uuid::new_v4();
    let message_id = uuid::Uuid::new_v4();
    let payload = serde_json::json!({ "orderId": order_id, "reason": "wrong order" });
    journal
        .insert(&application::journal::CommandJournalEntry {
            message_id,
            correlation_id: message_id,
            cause_id: None,
            session_id: None,
            trace_id: None,
            user_id: None,
            user_type: "ADMIN".into(),
            channel: domain::generated::scalars::CommandChannel::GRAPHQL,
            command_type: "DenyRefund".into(),
            payload_hash: application::journal::payload_hash(&payload),
            payload: payload.clone(),
        })
        .await
        .expect("seed pre-flip journal acceptance");
    journal
        .complete(message_id, domain::generated::scalars::CommandJournalStatus::SUCCEEDED, None)
        .await
        .expect("terminal");

    let gated = schema_over_with_gate(&pool, status_bus.clone(), true);
    let replay = format!(
        r#"mutation {{
            denyRefund(input: {{ orderId: "{order_id}", reason: "wrong order" }},
                       metadata: {{ messageId: "{message_id}" }})
            {{ messageId operationStatus duplicate }}
        }}"#
    );
    let resp = gated
        .execute(async_graphql::Request::new(replay).data(RequestRole::Admin))
        .await;
    assert!(resp.errors.is_empty(), "cross-arm replay errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    assert_eq!(data["denyRefund"]["duplicate"], true, "{data:?}");
    assert_eq!(data["denyRefund"]["operationStatus"], "SUCCEEDED");
    let mailbox_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inbound_messages")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(mailbox_rows, 0, "the replay never re-enqueued (no re-execution)");

    // Same id, DIFFERENT payload across arms: the synchronous Conflict, exactly like same-store.
    let conflicting = format!(
        r#"mutation {{
            denyRefund(input: {{ orderId: "{order_id}", reason: "changed my mind" }},
                       metadata: {{ messageId: "{message_id}" }})
            {{ messageId }}
        }}"#
    );
    let resp = gated
        .execute(async_graphql::Request::new(conflicting).data(RequestRole::Admin))
        .await;
    assert_eq!(resp.errors.len(), 1, "expected Conflict: {:?}", resp.errors);
    let ext = resp.errors[0].extensions.as_ref().expect("extensions");
    assert_eq!(ext.get("code"), Some(&async_graphql::Value::from("Conflict")));

    // --- Direction 2: accepted on the MAILBOX arm, retried on the LEGACY arm (gate rolled back).
    let mailbox = infrastructure::persistence::mailbox_store::PgMailbox::new(pool.clone());
    use actor_client::mailbox::Mailbox as _;
    let order2 = uuid::Uuid::new_v4();
    let message2 = uuid::Uuid::new_v4();
    let payload2 = serde_json::json!({ "orderId": order2, "reason": "cold food" });
    // Seed through the D5 EntryFixture door (PROP-20260802-130500): raw MailboxEntry
    // construction does not compile outside the actor_client crate.
    mailbox
        .insert(&actor_client::mailbox::fixtures::EntryFixture {
            message_id: message2,
            kind: "COMMAND".into(),
            actor_type: "RefundProcess".into(),
            actor_id: order2,
            partition: 0,
            message_type: "DenyRefund".into(),
            payload: payload2.clone(),
            payload_hash: application::journal::payload_hash(&payload2),
            channel: "GRAPHQL".into(),
            user_id: None,
            user_type: "ADMIN".into(),
            correlation_id: message2,
            cause_id: None,
            session_id: None,
            trace_id: None,
            source: None,
            external_id: None,
        }
        .into())
        .await
        .expect("seed mailbox acceptance");
    sqlx::query("UPDATE inbound_messages SET status = 'REJECTED', error = '{\"code\":\"RefundNotPending\"}', completed_at = now() WHERE message_id = $1")
        .bind(message2)
        .execute(&pool)
        .await
        .expect("terminal");

    let legacy = schema_over_with_gate(&pool, status_bus, false);
    let replay2 = format!(
        r#"mutation {{
            denyRefund(input: {{ orderId: "{order2}", reason: "cold food" }},
                       metadata: {{ messageId: "{message2}" }})
            {{ messageId operationStatus duplicate }}
        }}"#
    );
    let resp = legacy
        .execute(async_graphql::Request::new(replay2).data(RequestRole::Admin))
        .await;
    assert!(resp.errors.is_empty(), "reverse replay errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    assert_eq!(data["denyRefund"]["duplicate"], true, "{data:?}");
    assert_eq!(data["denyRefund"]["operationStatus"], "REJECTED");
    let journal_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM command_journal WHERE message_id = $1")
            .bind(message2)
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(journal_rows, 0, "the reverse replay never re-journaled (no re-execution)");
}
