//! The card's "walk" (farley): a DB-gated end-to-end pass over the REAL router/mailbox/guard/
//! projector stack, on the FULL migration chain (#639 part C step 4-i, ADR-20260904-081527) — not
//! the scripted seam of `rider_restricted_is_refused_on_the_write_half.rs` (which proves the
//! `StandingGuard` in isolation against a fabricated `ReadScope`) and not the raw-fold assertions of
//! `rider_projection.rs` (which prove the projector against `append_event` alone). This proves the
//! THREE layers compose: a real GraphQL mutation enqueues on the real mailbox, a real
//! `MailboxWorker` appends the fact, and a real `ProjectionWorker::run_once()` fold flips both the
//! ACL door (`StandingGuard`, evaluated fresh on the NEXT request — RIDER-REVOCATION-TTL) and the
//! read side (`myStanding`, `delivery`) in the SAME pass.
//!
//! Registration/order/delivery-job BIRTH facts are seeded by raw `domain_events` insert (the
//! `rider_projection.rs`/`delivery_read_model.rs` `append_event` idiom) rather than driven through
//! checkout/catalog/dispatch, which are unrelated to standing and would multiply the surface this
//! test can break on for no proof gained; the `OrderPlaced`/`DeliveryRequested` payloads are
//! `specs/tests.yaml` fixtures verbatim (`orderPlaced`/`deliveryRequested`), so they are exactly the
//! shape the generated projector is already proven against. `RegisterRider` has no public mutation
//! (the sign-in bridge issues it, #639 part C step 2c-i, out of scope here) so it is seeded the same
//! way. The rider's `ReadScope` is injected directly (`.data(ReadScope::Rider { .. })`), the same
//! established idiom `graphql_cart_read.rs`/`graphql_subscriptions.rs` already use for `Customer`
//! scopes — real JWT verification is a different seam (`auth.rs`, exercised by
//! `rider_sign_in_door.rs`), not what this slice changed.
//!
//! Needs a real Postgres (`DATABASE_URL`); SKIPS (prints and returns) without one, same as every
//! other DB-gated suite here.

use std::sync::Arc;

use application::generated::services::{IdentityService, PaymentService};
use application::ports::{EventStore, GbpOrderLinkProbe, GoogleOwnershipVerifier};
use application::queries::{
    CartReadRepository, CatalogReadRepository, CustomerReadRepository,
    DeliverySatisfactionReadRepository, DeliveryReadRepository, OrderReadRepository,
    PricingPolicyReadRepository, ProspectionReadRepository, ReadScope, RefundReadRepository,
    RestaurantReadRepository, UberEstimationPolicyReadRepository, UberSplitPolicyReadRepository,
};
use domain::generated::scalars::{RiderId, RiderStanding};
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, FailClosedPaymentGateway,
    PgCartRepository, PgCatalogRepository, PgCustomerRepository, PgDeliveryRepository,
    PgDeliverySatisfactionRepository, PgEventStore, PgOrderRepository, PgPricingPolicyRepository,
    PgProspectionRepository, PgRefundQueueRepository, PgRestaurantRepository,
    PgUberEstimationPolicyRepository, PgUberSplitPolicyRepository, ProjectionWorker,
    UnverifiedGbpOrderLinkProbe,
};
use serde_json::json;
use server::graphql_acl::RequestRole;
use sqlx::PgPool;

/// The role-guard witness (mirrors `graphql_write_path.rs::acting`).
fn acting(role: RequestRole) -> server::ActingRole {
    server::Principal::role_binding(role, "test-subject".to_string(), Some(uuid::Uuid::from_u128(0x6394_2)))
        .acting_role(role)
}

/// Drop `public` and replay the REAL migration chain from disk, in filename order — the exact
/// chain `crates/infrastructure/tests/main/common.rs::reset_schema` embeds, read dynamically here
/// instead of duplicated as ~55 `include_str!` lines a second time in a different crate. `View_*`
/// (e.g. `View_DeliveryJob`) is a SQL view over `domain_events`/`ordertracking`, so this is the only
/// way to get every table AND view this test's resolvers actually query, with no hand-drifted DDL.
async fn apply_all_migrations(pool: &PgPool) {
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .execute(pool)
        .await
        .expect("recreate the public schema");
    let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../migrations"));
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .expect("read migrations/")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "sql"))
        .collect();
    files.sort();
    for f in files {
        let sql = std::fs::read_to_string(&f).unwrap_or_else(|e| panic!("read {}: {e}", f.display()));
        sqlx::raw_sql(&sql)
            .execute(pool)
            .await
            .unwrap_or_else(|e| panic!("apply migration {}: {e}", f.display()));
    }
}

/// Append one fact directly to `domain_events` (the `rider_projection.rs`/`delivery_read_model.rs`
/// idiom) — a birth fact this slice's public API cannot produce (no `registerRider` mutation) or
/// that would otherwise require driving an unrelated checkout/dispatch pipeline just to exist.
async fn append_event(pool: &PgPool, stream_name: &str, version: i32, event_type: &str, payload: serde_json::Value) {
    sqlx::query(
        "INSERT INTO domain_events \
         (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, payload, metadata, occurred_at) \
         VALUES ($1, $2, $3, $4, 5, $5, NULL, $6, $7, NULL, now())",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(stream_name)
    .bind(version)
    .bind(uuid::Uuid::nil())
    .bind(uuid::Uuid::new_v4())
    .bind(event_type)
    .bind(payload)
    .execute(pool)
    .await
    .expect("append event");
}

/// The production delivery side (mirrors `graphql_write_path.rs::spawn_mailbox_workers`).
fn spawn_mailbox_workers(pool: &PgPool, bus: actor_client::OperationStatusBus) {
    let deps = infrastructure::generated::command_router::CommandDeps {
        store: Arc::new(PgEventStore::new(pool.clone())),
        riders: Arc::new(infrastructure::PgRiderRepository::new(pool.clone())),
        support_contact: None,
        restaurants: Arc::new(PgRestaurantRepository::new(pool.clone())),
        slugs: Arc::new(infrastructure::PgSlugReservationRepository::new(pool.clone())),
        auth_subjects: Arc::new(infrastructure::PgAuthSubjectReservationRepository::new(pool.clone())),
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
        mailbox_requeue: Arc::new(infrastructure::persistence::mailbox_lanes::PgMailboxRequeue::new(pool.clone())),
        enforce_service_hours_guard: false,
        enforce_acceptance_timeout: false,
        route_gates: application::generated::process_managers::RouteGates {
            order_placed_to_order: true,
            place_replacement_order_to_order: false,
            bind_cart_to_customer_to_cart: false,
            grant_customer_credit_to_customer_credit: false,
            mark_order_delivered_to_order: false,
        },
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
        std::mem::forget(_tx);
        tokio::spawn(async move {
            worker.seed(width).await.expect("seed");
            let _ = worker.run(rx).await;
        });
    }
}

/// The composition-root wiring (mirrors `graphql_write_path.rs::schema_over`).
fn schema_over(pool: &PgPool, status_bus: actor_client::OperationStatusBus) -> server::graphql_schema::CaptainSchema {
    let restaurants: Arc<dyn RestaurantReadRepository> = Arc::new(PgRestaurantRepository::new(pool.clone()));
    let prospection: Arc<dyn ProspectionReadRepository> = Arc::new(PgProspectionRepository::new(pool.clone()));
    let pricing_policy: Arc<dyn PricingPolicyReadRepository> = Arc::new(PgPricingPolicyRepository::new(pool.clone()));
    let uber_estimation_policy: Arc<dyn UberEstimationPolicyReadRepository> =
        Arc::new(PgUberEstimationPolicyRepository::new(pool.clone()));
    let uber_split_policy: Arc<dyn UberSplitPolicyReadRepository> =
        Arc::new(PgUberSplitPolicyRepository::new(pool.clone()));
    let catalogs: Arc<dyn CatalogReadRepository> = Arc::new(PgCatalogRepository::new(pool.clone()));
    let carts: Arc<dyn CartReadRepository> = Arc::new(PgCartRepository::new(pool.clone()));
    let orders: Arc<dyn OrderReadRepository> = Arc::new(PgOrderRepository::new(pool.clone()));
    let customers: Arc<dyn CustomerReadRepository> = Arc::new(PgCustomerRepository::new(pool.clone()));
    let deliveries: Arc<dyn DeliveryReadRepository> = Arc::new(PgDeliveryRepository::new(pool.clone()));
    let rider_restrictions: Arc<dyn application::queries::RiderRestrictionReadRepository> = Arc::new(
        infrastructure::persistence::rider_restriction_store::PgRiderRestrictionRepository::new(pool.clone()),
    );
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
    let mailbox_lanes: Arc<dyn actor_client::supervision::MailboxLaneRepository> =
        Arc::new(infrastructure::persistence::mailbox_lanes::PgMailboxLaneRepository::new(pool.clone()));
    let event_store: Arc<dyn EventStore> = Arc::new(PgEventStore::new(pool.clone()));
    let ownership: Arc<dyn GoogleOwnershipVerifier> = Arc::new(FailClosedGoogleOwnershipVerifier);
    let gbp_probe: Arc<dyn GbpOrderLinkProbe> = Arc::new(UnverifiedGbpOrderLinkProbe);
    let auth_provider: Arc<dyn IdentityService> = Arc::new(FailClosedIdentityService);
    let payments: Arc<dyn PaymentService> = Arc::new(FailClosedPaymentGateway);
    let pm_state: Arc<dyn application::pm_state::PaymentProcessStateStore> =
        Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone()));
    let refund_state: Arc<dyn application::pm_state::RefundProcessStateStore> =
        Arc::new(infrastructure::persistence::PgRefundProcessState::new(pool.clone()));
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
            rider_restrictions,
            refunds,
            delivery_satisfaction,
            delivery_partner_availabilities,
            reclamations,
            customer_credit,
            mailbox_lanes,
            service_window_horizon: Default::default(),
        }),
        Some(server::graphql_schema::WriteDeps {
            event_store,
            ownership,
            gbp_probe,
            auth_provider,
            payments,
            pm_state,
            refund_state,
            mailbox,
            status_bus,
            auth_sessions: std::sync::Arc::new(application::auth_sessions::NoopAuthSessionStore),
            slug_reservations: std::sync::Arc::new(AlwaysFreeSlugs),
        }),
        None,
    )
}

/// A `SlugReservationRepository` that grants every request (mirrors `graphql_write_path.rs`).
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

/// Poll `operationStatus(messageId)` until non-PENDING (mirrors `graphql_write_path.rs`).
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
        let mut req = async_graphql::Request::new(query).data(acting(role));
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

static DB_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The card's walk (Section D "The walk as a DB-gated integration test", farley): registered rider
/// -> `acceptDelivery` allowed -> `restrictRider` as ADMIN through the real router -> fold ->
/// `acceptDelivery` FORBIDDEN, `delivery(orderId)` allowed, `myStanding` shows RESTRICTED + the held
/// job -> `reinstateRider` -> allowed again (the guard reopens; the eventual business outcome is a
/// SEPARATE question, proven by the terminal `DeliveryAlreadyAssigned`).
#[tokio::test]
async fn the_restriction_walk_forbids_and_reopens_the_real_doors() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let Some(url) = db_test_gate::database_url("rider_standing_walk") else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    apply_all_migrations(&pool).await;

    let status_bus = actor_client::OperationStatusBus::default();
    spawn_mailbox_workers(&pool, status_bus.clone());
    let schema = schema_over(&pool, status_bus);

    // 1) A real restaurant, through the real router (the order/job's owner for the read joins).
    let restaurant_id = uuid::Uuid::new_v4();
    let mutation = format!(
        r#"mutation {{
            registerRestaurant(input: {{
                restaurantId: "{restaurant_id}",
                displayName: "Chez Marco",
                address: {{ line1: "1 Rue Nationale", postalCode: "37000", city: "Tours", country: "FR" }}
            }}) {{ messageId operationStatus }}
        }}"#
    );
    let resp = schema.execute(async_graphql::Request::new(mutation).data(acting(RequestRole::Admin))).await;
    assert!(resp.errors.is_empty(), "registerRestaurant errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    let message_id = data["registerRestaurant"]["messageId"].as_str().unwrap().to_string();
    let op = poll_operation(&schema, &message_id, RequestRole::Admin, None).await;
    assert_eq!(op["status"], "SUCCEEDED", "registerRestaurant operation: {op:?}");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (restaurant)");

    // 2) The rider's birth fact (no public `registerRider` mutation exists — the sign-in bridge
    //    issues it, #639 part C step 2c-i, not this slice).
    let rider_id = uuid::Uuid::new_v4();
    append_event(
        &pool,
        &format!("Rider-{rider_id}"),
        1,
        "RiderRegistered",
        json!({
            "riderId": rider_id, "authRef": "auth-walk-1", "displayName": "Walk Rider",
            "phone": "+33611112222", "status": "OFFLINE"
        }),
    )
    .await;

    // 3) The order + delivery-job birth facts — `specs/tests.yaml` fixtures `orderPlaced` /
    //    `deliveryRequested` verbatim, re-keyed onto this test's ids.
    let order_id = uuid::Uuid::new_v4();
    let job_id = uuid::Uuid::new_v4();
    append_event(
        &pool,
        &format!("Order-{order_id}"),
        1,
        "OrderPlaced",
        json!({
            "orderId": order_id, "restaurantId": restaurant_id, "customerId": uuid::Uuid::new_v4(),
            "customerContact": { "displayName": "Johnny", "phone": "+33612345678" },
            "serviceType": "DELIVERY",
            "deliveryAddress": { "line1": "9 Rue Colbert", "postalCode": "37000", "city": "Tours", "country": "FR" },
            // Real UUIDs, unlike the `specs/tests.yaml` fixture's human-readable placeholders
            // ("off-1"/"prod-1"): those decode fine through the TYPED behaviour-test harness, but
            // `OfferId`/`ProductId` are UUID scalars, and this path is the REAL Postgres JSONB ->
            // struct decode (`aggregate_uuid_of`/serde), which parses them for real.
            "items": [{ "offerId": uuid::Uuid::new_v4(), "productId": uuid::Uuid::new_v4(), "name": "Margherita", "offerName": "Default", "quantity": 2, "unitPrice": { "amountCents": 980, "currency": "EUR" }, "lineTotal": { "amountCents": 1960, "currency": "EUR" } }],
            "totalAmount": { "amountCents": 1960, "currency": "EUR" },
            "breakdown": {
                "articles": { "amountCents": 1960, "currency": "EUR" },
                "delivery": { "amountCents": 0, "currency": "EUR" },
                "serviceFee": { "amountCents": 0, "currency": "EUR" },
                "total": { "amountCents": 1960, "currency": "EUR" },
                "restaurantContribution": { "amountCents": 0, "currency": "EUR" },
                "restaurantPayout": { "amountCents": 1960, "currency": "EUR" },
                "riderPayout": { "amountCents": 0, "currency": "EUR" },
                "captainNet": { "amountCents": 0, "currency": "EUR" }
            },
            "paymentIntentId": "pi_walk_1"
        }),
    )
    .await;
    append_event(
        &pool,
        &format!("DeliveryJob-{job_id}"),
        1,
        "DeliveryRequested",
        json!({
            "deliveryJobId": job_id, "orderId": order_id, "restaurantId": restaurant_id,
            "pickup": { "line1": "1 Rue Nationale", "postalCode": "37000", "city": "Tours", "country": "FR" },
            "dropoff": { "line1": "9 Rue Colbert", "postalCode": "37000", "city": "Tours", "country": "FR" }
        }),
    )
    .await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (order + job birth)");

    let active = ReadScope::Rider { id: RiderId(rider_id), standing: RiderStanding::ACTIVE };

    // 4) acceptDelivery as an ACTIVE rider: the guard admits, the business layer accepts.
    let accept = format!(r#"mutation {{ acceptDelivery(input: {{ deliveryJobId: "{job_id}" }}) {{ messageId operationStatus }} }}"#);
    let resp = schema
        .execute(async_graphql::Request::new(accept.clone()).data(acting(RequestRole::Rider)).data(active.clone()))
        .await;
    assert!(resp.errors.is_empty(), "acceptDelivery (ACTIVE) errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    assert_eq!(data["acceptDelivery"]["operationStatus"], "PENDING", "the ACTIVE guard reaches the business layer");
    let message_id = data["acceptDelivery"]["messageId"].as_str().unwrap().to_string();
    // Polled as ADMIN: `mailbox_operation_owned` recognizes ADMIN or a Principal/session match,
    // and this harness (like `graphql_write_path.rs`) injects only the `ActingRole` guard witness,
    // never a full `Principal` -- so a RIDER poll of its OWN operation is invisible by the same
    // ownership rule real riders rely on (a real rider polls under a REAL Principal the auth seam
    // sets, which this harness does not reconstruct). Waiting for completion is orthogonal to what
    // this test proves (the guard decision already happened at enqueue, under RequestRole::Rider).
    let op = poll_operation(&schema, &message_id, RequestRole::Admin, None).await;
    assert_eq!(op["status"], "SUCCEEDED", "acceptDelivery (ACTIVE) operation: {op:?}");
    // The scope-membership grant (ORDER, RIDER) rides the SAME "Order" checkpoint's
    // ScopeMembership fold, over the DeliveryAcceptedByRider fact this just appended.
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (accept)");

    // 5) delivery(orderId) as the SAME ACTIVE rider: visible, ASSIGNED.
    let delivery_q = format!(r#"query {{ delivery(input: {{ orderId: "{order_id}" }}) {{ id status }} }}"#);
    let resp = schema
        .execute(async_graphql::Request::new(delivery_q.clone()).data(acting(RequestRole::Rider)).data(active.clone()))
        .await;
    assert!(resp.errors.is_empty(), "delivery (ACTIVE) errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    assert_eq!(data["delivery"]["status"], "ASSIGNED", "accept folded: {data:?}");

    // 6) restrictRider as ADMIN, through the real router.
    let restrict = format!(
        r#"mutation {{ restrictRider(input: {{ riderId: "{rider_id}", ground: RIDER_REQUESTED }}) {{ messageId operationStatus }} }}"#
    );
    let resp = schema.execute(async_graphql::Request::new(restrict).data(acting(RequestRole::Admin))).await;
    assert!(resp.errors.is_empty(), "restrictRider errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    assert_eq!(data["restrictRider"]["operationStatus"], "PENDING");
    let message_id = data["restrictRider"]["messageId"].as_str().unwrap().to_string();
    let op = poll_operation(&schema, &message_id, RequestRole::Admin, None).await;
    assert_eq!(op["status"], "SUCCEEDED", "restrictRider operation: {op:?}");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (restrict)");

    let restricted = ReadScope::Rider { id: RiderId(rider_id), standing: RiderStanding::RESTRICTED };

    // 7) acceptDelivery as the NOW-restricted rider: the StandingGuard denies it SYNCHRONOUSLY —
    //    never reaches the mailbox (no PENDING; a GraphQL FORBIDDEN, before any command exists).
    let resp = schema
        .execute(async_graphql::Request::new(accept.clone()).data(acting(RequestRole::Rider)).data(restricted.clone()))
        .await;
    assert_eq!(resp.errors.len(), 1, "expected the synchronous FORBIDDEN: {:?}", resp.errors);
    let ext = resp.errors[0].extensions.as_ref().expect("extensions");
    assert_eq!(ext.get("code"), Some(&async_graphql::Value::from("FORBIDDEN")), "wrong code: {resp:?}");

    // 8) delivery(orderId) STAYS reachable while restricted (the `whileRestricted: [RIDER]` carve).
    let resp = schema
        .execute(async_graphql::Request::new(delivery_q).data(acting(RequestRole::Rider)).data(restricted.clone()))
        .await;
    assert!(resp.errors.is_empty(), "delivery must stay reachable while restricted: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    assert_eq!(data["delivery"]["status"], "ASSIGNED", "the held job stays visible: {data:?}");

    // 9) myStanding as the restricted rider: standing + attribution + the held job, one query.
    let my_standing = "query { myStanding { standing restriction { ground } heldDelivery { id status } } }";
    let resp = schema
        .execute(async_graphql::Request::new(my_standing).data(acting(RequestRole::Rider)).data(restricted.clone()))
        .await;
    assert!(resp.errors.is_empty(), "myStanding errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    assert_eq!(data["myStanding"]["standing"], "RESTRICTED");
    assert_eq!(data["myStanding"]["restriction"]["ground"], "RIDER_REQUESTED");
    assert_eq!(data["myStanding"]["heldDelivery"]["status"], "ASSIGNED");
    assert_eq!(data["myStanding"]["heldDelivery"]["id"], job_id.to_string());

    // 10) reinstateRider as ADMIN.
    let reinstate = format!(r#"mutation {{ reinstateRider(input: {{ riderId: "{rider_id}" }}) {{ messageId operationStatus }} }}"#);
    let resp = schema.execute(async_graphql::Request::new(reinstate).data(acting(RequestRole::Admin))).await;
    assert!(resp.errors.is_empty(), "reinstateRider errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    let message_id = data["reinstateRider"]["messageId"].as_str().unwrap().to_string();
    let op = poll_operation(&schema, &message_id, RequestRole::Admin, None).await;
    assert_eq!(op["status"], "SUCCEEDED", "reinstateRider operation: {op:?}");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (reinstate)");

    // 10b) myStanding as the reinstated (ACTIVE) rider: round-2 item 2 (graphql-architect) — the
    //      RiderRestriction row keeps its ground/dates after ReinstateRider by design
    //      (projection_tables.yaml:578, the Art. 11 log), so the resolver must gate `restriction`
    //      on `standing == RESTRICTED` rather than on the row's mere existence, or a reinstated
    //      rider's OWN standing answer would keep exposing a stale attribution api.yaml/ADR §4
    //      never promised past reinstatement.
    let my_standing_reinstated = "query { myStanding { standing restriction { ground } } }";
    let resp = schema
        .execute(async_graphql::Request::new(my_standing_reinstated).data(acting(RequestRole::Rider)).data(active.clone()))
        .await;
    assert!(resp.errors.is_empty(), "myStanding (reinstated) errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    assert_eq!(data["myStanding"]["standing"], "ACTIVE");
    assert_eq!(data["myStanding"]["restriction"], serde_json::Value::Null, "a reinstated rider's own standing answer must carry no restriction attribution: {data:?}");

    // 11) acceptDelivery as the reinstated (ACTIVE) rider: the GUARD reopens — the mutation reaches
    //     the business layer again (PENDING, never a synchronous FORBIDDEN); the eventual REJECTED
    //     is the business layer's own, unrelated call (already ASSIGNED to this same rider) —
    //     proving what reopened is the door, not a side effect of the business outcome.
    let resp = schema.execute(async_graphql::Request::new(accept).data(acting(RequestRole::Rider)).data(active)).await;
    assert!(resp.errors.is_empty(), "acceptDelivery (reinstated) errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    assert_eq!(data["acceptDelivery"]["operationStatus"], "PENDING", "the guard reopens: {data:?}");
    let message_id = data["acceptDelivery"]["messageId"].as_str().unwrap().to_string();
    let op = poll_operation(&schema, &message_id, RequestRole::Admin, None).await;
    assert_eq!(op["status"], "REJECTED", "acceptDelivery (reinstated) operation: {op:?}");
    assert_eq!(op["errorCode"], "DeliveryAlreadyAssigned", "the business layer's own, unrelated rejection: {op:?}");
}
