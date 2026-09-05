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
use domain::generated::scalars::{EmailAddress, RiderId, RiderStanding};
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

/// Round 3 (#639 part C step 4-iii-A R3-5, beck + dba): the SAME helper, `occurred_at` pinned to an
/// EXPLICIT timestamp rather than `now()` — the only way to force a genuine tie on `requested_at`
/// (a `DeliveryRequested` event's occurrence time, ADR-20260904-152807 §2's read), which is exactly
/// what exercises the `delivery_job_id DESC` tie-break `persistence/delivery.rs` adds after
/// `requested_at DESC`. A wall-clock sleep between two `now()`-stamped events (the round-2 test's
/// approach) can never produce a tie, so it never exercised that second ORDER BY key at all.
async fn append_event_at(pool: &PgPool, stream_name: &str, version: i32, event_type: &str, payload: serde_json::Value, occurred_at: chrono::DateTime<chrono::Utc>) {
    sqlx::query(
        "INSERT INTO domain_events \
         (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, payload, metadata, occurred_at) \
         VALUES ($1, $2, $3, $4, 5, $5, NULL, $6, $7, NULL, $8)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(stream_name)
    .bind(version)
    .bind(uuid::Uuid::nil())
    .bind(uuid::Uuid::new_v4())
    .bind(event_type)
    .bind(payload)
    .bind(occurred_at)
    .execute(pool)
    .await
    .expect("append event at explicit occurred_at");
}

/// The production delivery side (mirrors `graphql_write_path.rs::spawn_mailbox_workers`).
fn spawn_mailbox_workers(pool: &PgPool, bus: actor_client::OperationStatusBus) {
    spawn_mailbox_workers_with_door(pool, bus, true)
}

/// #639 part C step 4-iii-A (ADR-20260904-152807 §7): the SAME composition, parameterised on the
/// restrict door's release gate — the walk's main leg runs it ON (door open, `restrictRider`
/// reaches the store); a second, independent worker fleet built with the key OFF proves the typed
/// refusal end to end, through the real router, never a unit-test shortcut.
fn spawn_mailbox_workers_with_door(pool: &PgPool, bus: actor_client::OperationStatusBus, run_rider_restriction_door: bool) {
    let deps = infrastructure::generated::command_router::CommandDeps {
        store: Arc::new(PgEventStore::new(pool.clone())),
        riders: Arc::new(infrastructure::PgRiderRepository::new(pool.clone())),
        members: Arc::new(infrastructure::PgMemberRepository::new(pool.clone())),
        support_contact: None,
        run_rider_restriction_door,
        run_member_access_grant: false,
        run_member_sign_in_door: false,
        run_restaurant_invitation: false,
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
///
/// `run_rider_restriction_door_read` (round 2, beck): the READ half of the SAME key, parameterised
/// independently of the write door parameter `spawn_mailbox_workers_with_door` takes — the two
/// composition roots resolve the same configuration value, but a test proving "the key never
/// touches the read guard" (§7) must be able to run the read side with the key OFF too, otherwise
/// the door-closed leg exercises a schema that never had a chance to consult it.
fn schema_over(
    pool: &PgPool,
    status_bus: actor_client::OperationStatusBus,
    run_rider_restriction_door_read: bool,
) -> server::graphql_schema::CaptainSchema {
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
    let rider_roster: Arc<dyn application::queries::RiderRosterReadRepository> = Arc::new(
        infrastructure::persistence::rider_roster_store::PgRiderRosterRepository::new(pool.clone()),
    );
    let member_authority: Arc<dyn application::queries::MemberAuthorityRepository> =
        Arc::new(infrastructure::PgMemberAuthorityRepository::new(pool.clone()));
    let restaurant_roster: Arc<dyn application::queries::RestaurantRosterReadRepository> =
        Arc::new(infrastructure::PgRestaurantRosterRepository::new(pool.clone()));
    let restaurant_invitations: Arc<dyn application::queries::RestaurantInvitationListReadRepository> =
        Arc::new(infrastructure::PgRestaurantInvitationListRepository::new(pool.clone()));
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
            rider_roster,
            member_authority,
            restaurant_roster,
            restaurant_invitations,
            refunds,
            delivery_satisfaction,
            delivery_partner_availabilities,
            reclamations,
            customer_credit,
            mailbox_lanes,
            service_window_horizon: Default::default(),
            // #882 R2 addendum item 13: every OTHER fixture in the corpus threads `None` here —
            // this is the ONE walk that actually reads `myStanding.contestContact` back, so it is
            // the walk that must prove the configured value reaches the wire.
            support_contact: Some(EmailAddress("support@captain.food".to_string())),
            // #639 part C step 4-iii-A (round 2 item 1): parameterised on the caller, not
            // hard-coded -- the main walk passes `true` (matching the write door above, so its
            // `riders`/`rider` assertions exercise `restrictionDoorOpen: true`); the door-closed
            // leg passes `false` so the read side genuinely has the key OFF too.
            run_rider_restriction_door: server::graphql_schema::RunRiderRestrictionDoor(run_rider_restriction_door_read),
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
    let schema = schema_over(&pool, status_bus, true);

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
    // #639 part C step 4-ii (ADR-20260904-124600 §4/§5, card D): both dates non-null (the notice
    // shows BOTH even though V0 stamps them equal, ADR-081527 §5) and `heldDelivery` answers
    // through the NEW `held_by_rider` port (#879).
    let my_standing = "query { myStanding { standing restriction { ground decidedAt effectiveAt } heldDelivery { id status } contestContact } }";
    let resp = schema
        .execute(async_graphql::Request::new(my_standing).data(acting(RequestRole::Rider)).data(restricted.clone()))
        .await;
    assert!(resp.errors.is_empty(), "myStanding errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    assert_eq!(data["myStanding"]["standing"], "RESTRICTED");
    assert_eq!(data["myStanding"]["restriction"]["ground"], "RIDER_REQUESTED");
    assert_ne!(data["myStanding"]["restriction"]["decidedAt"], serde_json::Value::Null, "decidedAt must be folded: {data:?}");
    assert_ne!(data["myStanding"]["restriction"]["effectiveAt"], serde_json::Value::Null, "effectiveAt must be folded: {data:?}");
    assert_eq!(data["myStanding"]["heldDelivery"]["status"], "ASSIGNED");
    assert_eq!(data["myStanding"]["heldDelivery"]["id"], job_id.to_string());
    // #882 R2 addendum item 13: `support_contact` threaded `Some(..)` through `ReadDeps` above —
    // every other fixture in the corpus passes `None`, which never proves the binding reaches
    // `myStanding.contestContact` at all.
    assert_eq!(data["myStanding"]["contestContact"], "support@captain.food", "the configured SUPPORT_CONTACT must reach the wire: {data:?}");

    // 9b) rider(riderId) as ADMIN (#639 part C step 4-iii-A): reads RESTRICTED + ground +
    //     heldDelivery { id status }, the admin detail's own composition.
    let rider_detail = format!(
        r#"query {{ rider(input: {{ riderId: "{rider_id}" }}) {{ riderId standing ground heldDelivery {{ id status }} restrictionDoorOpen }} }}"#
    );
    let resp = schema.execute(async_graphql::Request::new(rider_detail.clone()).data(acting(RequestRole::Admin))).await;
    assert!(resp.errors.is_empty(), "rider (ADMIN) errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    assert_eq!(data["rider"]["standing"], "RESTRICTED");
    assert_eq!(data["rider"]["ground"], "RIDER_REQUESTED");
    assert_eq!(data["rider"]["heldDelivery"]["id"], job_id.to_string());
    assert_eq!(data["rider"]["heldDelivery"]["status"], "ASSIGNED");
    assert_eq!(data["rider"]["restrictionDoorOpen"], true, "the walk's ReadDeps carries the door ON");

    // 9c) riders as ADMIN (#639 part C step 4-iii-A, ADR-20260904-152807 §2/§4): the ONE rider this
    //     walk registered is held AND restricted, so it lands in the FIRST (held) group regardless
    //     of standing — the contract order's own precedence.
    let riders_q = "query { riders { riderId standing heldDelivery { id } } }";
    let resp = schema.execute(async_graphql::Request::new(riders_q).data(acting(RequestRole::Admin))).await;
    assert!(resp.errors.is_empty(), "riders (ADMIN) errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    let list = data["riders"].as_array().expect("riders array");
    assert_eq!(list[0]["riderId"], rider_id.to_string(), "the held rider must lead the list: {list:?}");
    assert_eq!(list[0]["standing"], "RESTRICTED");
    assert_eq!(list[0]["heldDelivery"]["id"], job_id.to_string());

    // 9d) riders as RIDER: FORBIDDEN, synchronously — the ACL door admits ADMIN only.
    let resp = schema
        .execute(async_graphql::Request::new(riders_q).data(acting(RequestRole::Rider)).data(restricted.clone()))
        .await;
    assert_eq!(resp.errors.len(), 1, "expected FORBIDDEN on riders as RIDER: {:?}", resp.errors);
    let ext = resp.errors[0].extensions.as_ref().expect("extensions");
    assert_eq!(ext.get("code"), Some(&async_graphql::Value::from("FORBIDDEN")), "wrong code: {resp:?}");

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

    // 10c) rider(riderId) as ADMIN after reinstatement: ACTIVE, but `heldDelivery` STAYS present —
    //      reinstatement releases the ACCESS restriction, never the CUSTODY the rider still holds
    //      (ADR-20260904-152807 §2: one custody truth, read at query time, never a folded column).
    let resp = schema.execute(async_graphql::Request::new(rider_detail).data(acting(RequestRole::Admin))).await;
    assert!(resp.errors.is_empty(), "rider (ADMIN, post-reinstate) errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    assert_eq!(data["rider"]["standing"], "ACTIVE");
    assert_eq!(data["rider"]["heldDelivery"]["id"], job_id.to_string(), "custody is not released by reinstatement: {data:?}");

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

    // 12) A SECOND restriction cycle (ADR-20260904-081527 §2: "a second ground needs a
    //     reinstatement first" — this rider WAS reinstated in step 10, so restricting again is a
    //     valid lifecycle transition) proves the held-job custody carve-out end to end: the job
    //     this walk has held throughout is STILL `ASSIGNED` to this rider (every mutation on it
    //     above was either refused synchronously by the guard or REJECTED business-side — never
    //     released), so a fresh restriction sees it held again.
    let restrict2 = format!(
        r#"mutation {{ restrictRider(input: {{ riderId: "{rider_id}", ground: IDENTITY_MISMATCH }}) {{ messageId operationStatus }} }}"#
    );
    let resp = schema.execute(async_graphql::Request::new(restrict2).data(acting(RequestRole::Admin))).await;
    assert!(resp.errors.is_empty(), "restrictRider (second cycle) errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    let message_id = data["restrictRider"]["messageId"].as_str().unwrap().to_string();
    let op = poll_operation(&schema, &message_id, RequestRole::Admin, None).await;
    assert_eq!(op["status"], "SUCCEEDED", "restrictRider (second cycle) operation: {op:?}");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (restrict 2)");

    let restricted2 = ReadScope::Rider { id: RiderId(rider_id), standing: RiderStanding::RESTRICTED };

    // 13) handBackDelivery as the (again) restricted rider — restriction of ACCESS is not
    //     release of CUSTODY (ADR-20260904-081527 §4/§7): the carve-out (`whileRestricted:
    //     [RIDER]`) reaches the business layer, never a synchronous FORBIDDEN.
    let hand_back = format!(
        r#"mutation {{ handBackDelivery(input: {{ deliveryJobId: "{job_id}", foodLocation: NOT_COLLECTED }}) {{ messageId operationStatus }} }}"#
    );
    let resp = schema
        .execute(async_graphql::Request::new(hand_back).data(acting(RequestRole::Rider)).data(restricted2.clone()))
        .await;
    assert!(resp.errors.is_empty(), "handBackDelivery (RESTRICTED) errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    assert_eq!(data["handBackDelivery"]["operationStatus"], "PENDING", "the carve-out reaches the business layer: {data:?}");
    let message_id = data["handBackDelivery"]["messageId"].as_str().unwrap().to_string();
    let op = poll_operation(&schema, &message_id, RequestRole::Admin, None).await;
    assert_eq!(op["status"], "SUCCEEDED", "handBackDelivery (RESTRICTED) operation: {op:?}");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (handback)");

    // 14) myStanding.heldDelivery == null (card D, #879): `held_by_rider` narrows on status
    //     (ASSIGNED/PICKED_UP/OUT_FOR_DELIVERY) and the handback moved this job out of all three.
    let my_standing_after_handback = "query { myStanding { heldDelivery { id } } }";
    let resp = schema
        .execute(async_graphql::Request::new(my_standing_after_handback).data(acting(RequestRole::Rider)).data(restricted2))
        .await;
    assert!(resp.errors.is_empty(), "myStanding (post-handback) errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    assert_eq!(
        data["myStanding"]["heldDelivery"],
        serde_json::Value::Null,
        "held_by_rider must narrow to nothing once handed back: {data:?}"
    );
}

/// #639 part C step 4-iii-A (ADR-20260904-152807 §7): the door-OFF leg, through the REAL router —
/// a SEPARATE fleet (its own registered rider) rather than a second worker racing the main walk's
/// lanes, so the two never contend over the same mailbox rows. Two assertions in one pass: (1) the
/// write door refuses `restrictRider` with the typed `RiderRestrictionDoorClosed` while OFF, before
/// the store is even touched; (2) the named mutant — a rider ALREADY RESTRICTED (seeded by a raw
/// fact, never through the closed door) is STILL refused by `StandingGuard` with the key OFF: the
/// key never reaches the read side.
#[tokio::test]
async fn the_restrict_door_refuses_while_closed_and_the_read_guard_never_consults_it() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let Some(url) = db_test_gate::database_url("rider_standing_walk_door_closed") else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    apply_all_migrations(&pool).await;

    let status_bus = actor_client::OperationStatusBus::default();
    spawn_mailbox_workers_with_door(&pool, status_bus.clone(), false);
    let schema = schema_over(&pool, status_bus, false);

    // An ACTIVE rider, seeded directly (no public registration mutation, #639 part C step 2c-i).
    let rider_id = uuid::Uuid::new_v4();
    append_event(
        &pool,
        &format!("Rider-{rider_id}"),
        1,
        "RiderRegistered",
        json!({
            "riderId": rider_id, "authRef": "auth-door-closed-1", "displayName": "Door Closed Rider",
            "phone": "+33611113333", "status": "OFFLINE"
        }),
    )
    .await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (rider birth)");

    // (1) restrictRider with the door OFF: REJECTED, typed, before any RiderRestricted exists.
    let restrict = format!(
        r#"mutation {{ restrictRider(input: {{ riderId: "{rider_id}", ground: RIDER_REQUESTED }}) {{ messageId operationStatus }} }}"#
    );
    let resp = schema.execute(async_graphql::Request::new(restrict).data(acting(RequestRole::Admin))).await;
    assert!(resp.errors.is_empty(), "restrictRider (door OFF) errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    let message_id = data["restrictRider"]["messageId"].as_str().unwrap().to_string();
    let op = poll_operation(&schema, &message_id, RequestRole::Admin, None).await;
    assert_eq!(op["status"], "REJECTED", "restrictRider (door OFF) operation: {op:?}");
    assert_eq!(op["errorCode"], "RiderRestrictionDoorClosed", "the typed refusal: {op:?}");

    // (2) the named mutant: seed RiderRestricted as a raw fact (bypassing the closed door
    //     entirely, exactly as a from-zero replay or a direct store append would), fold it, and
    //     prove the READ guard still refuses — the key never touches `StandingGuard`.
    append_event(
        &pool,
        &format!("Rider-{rider_id}"),
        2,
        "RiderRestricted",
        json!({
            "riderId": rider_id, "ground": "IDENTITY_MISMATCH",
            "decidedAt": "2026-01-06T12:00:00Z", "effectiveAt": "2026-01-06T12:00:00Z"
        }),
    )
    .await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (restricted fact)");

    let restricted = ReadScope::Rider { id: RiderId(rider_id), standing: RiderStanding::RESTRICTED };
    let my_standing = "query { myStanding { standing } }";
    let resp = schema
        .execute(async_graphql::Request::new(my_standing).data(acting(RequestRole::Rider)).data(restricted.clone()))
        .await;
    assert!(resp.errors.is_empty(), "myStanding (door OFF, restricted) errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    assert_eq!(data["myStanding"]["standing"], "RESTRICTED", "the read side folded the fact regardless of the write door: {data:?}");

    // A door-gated door key must never widen a READ-side guard: `myDeliveries` (a whileRestricted
    // carve-out door) stays reachable, but an UN-carved write door (`changeRiderStatus` ->
    // AVAILABLE) must still be refused for this restricted rider -- the guard's OWN authority,
    // never delegated to the write door's release key.
    let change_status = r#"mutation { changeRiderStatus(input: { status: AVAILABLE }) { messageId operationStatus } }"#;
    let resp = schema
        .execute(async_graphql::Request::new(change_status).data(acting(RequestRole::Rider)).data(restricted))
        .await;
    assert_eq!(resp.errors.len(), 1, "expected the synchronous FORBIDDEN even with the restrict door OFF: {:?}", resp.errors);
    let ext = resp.errors[0].extensions.as_ref().expect("extensions");
    assert_eq!(ext.get("code"), Some(&async_graphql::Value::from("FORBIDDEN")), "wrong code: {resp:?}");
}

/// #639 part C step 4-iii-A round 2 item 3 (dba): TWO ASSIGNED jobs on the SAME rider proves the
/// list (`riders`, a set-based `held_by_riders` folded FIRST-wins) and the detail (`rider`, a
/// single `held_by_rider` `LIMIT 1`) name the SAME held job. Before this round's fix the two
/// queries picked OPPOSITE ends of `requested_at DESC`: a plain `.collect()` on the list was
/// LAST-wins (the OLDEST held job survives), while the detail's `LIMIT 1 ORDER BY requested_at
/// DESC` picks the NEWEST — an admin could see one delivery on the triage row and a DIFFERENT one
/// on the detail page for the identical rider (ADR-20260904-152807 §2, "one custody truth").
#[tokio::test]
async fn the_list_and_the_detail_name_the_same_held_job() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let Some(url) = db_test_gate::database_url("rider_standing_walk_same_held_job") else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    apply_all_migrations(&pool).await;

    let status_bus = actor_client::OperationStatusBus::default();
    spawn_mailbox_workers(&pool, status_bus.clone());
    let schema = schema_over(&pool, status_bus, true);

    // A restaurant, through the real router (the jobs' owner for the read joins).
    let restaurant_id = uuid::Uuid::new_v4();
    let mutation = format!(
        r#"mutation {{
            registerRestaurant(input: {{
                restaurantId: "{restaurant_id}",
                displayName: "Chez Deux Courses",
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

    // The rider's birth fact (no public `registerRider` mutation, #639 part C step 2c-i).
    let rider_id = uuid::Uuid::new_v4();
    append_event(
        &pool,
        &format!("Rider-{rider_id}"),
        1,
        "RiderRegistered",
        json!({
            "riderId": rider_id, "authRef": "auth-two-jobs-1", "displayName": "Two Jobs Rider",
            "phone": "+33611114444", "status": "AVAILABLE"
        }),
    )
    .await;

    // TWO orders + TWO delivery jobs, both ASSIGNED to the SAME rider. Round 3 (R3-5): both
    // `DeliveryRequested` facts share the SAME `occurred_at` (`append_event_at`, not a wall-clock
    // sleep) -- a genuine `requested_at` TIE, which is the only way to actually exercise the
    // `delivery_job_id DESC` tie-break `persistence/delivery.rs` adds after `requested_at DESC`.
    // Round 2's 20ms sleep gave the two jobs DISTINCT timestamps, so `requested_at DESC` alone
    // always decided the order and the tie-break line was never reached by this test.
    let same_requested_at = chrono::Utc::now();
    let mut job_ids: Vec<uuid::Uuid> = Vec::new();
    for i in 0..2u8 {
        let order_id = uuid::Uuid::new_v4();
        let job_id = uuid::Uuid::new_v4();
        job_ids.push(job_id);
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
                "items": [{ "offerId": uuid::Uuid::new_v4(), "productId": uuid::Uuid::new_v4(), "name": "Margherita", "offerName": "Default", "quantity": 1, "unitPrice": { "amountCents": 980, "currency": "EUR" }, "lineTotal": { "amountCents": 980, "currency": "EUR" } }],
                "totalAmount": { "amountCents": 980, "currency": "EUR" },
                "breakdown": {
                    "articles": { "amountCents": 980, "currency": "EUR" },
                    "delivery": { "amountCents": 0, "currency": "EUR" },
                    "serviceFee": { "amountCents": 0, "currency": "EUR" },
                    "total": { "amountCents": 980, "currency": "EUR" },
                    "restaurantContribution": { "amountCents": 0, "currency": "EUR" },
                    "restaurantPayout": { "amountCents": 980, "currency": "EUR" },
                    "riderPayout": { "amountCents": 0, "currency": "EUR" },
                    "captainNet": { "amountCents": 0, "currency": "EUR" }
                },
                "paymentIntentId": format!("pi_walk_two_jobs_{i}")
            }),
        )
        .await;
        append_event_at(
            &pool,
            &format!("DeliveryJob-{job_id}"),
            1,
            "DeliveryRequested",
            json!({
                "deliveryJobId": job_id, "orderId": order_id, "restaurantId": restaurant_id,
                "pickup": { "line1": "1 Rue Nationale", "postalCode": "37000", "city": "Tours", "country": "FR" },
                "dropoff": { "line1": "9 Rue Colbert", "postalCode": "37000", "city": "Tours", "country": "FR" }
            }),
            same_requested_at,
        )
        .await;
        append_event(
            &pool,
            &format!("DeliveryJob-{job_id}"),
            2,
            "DeliveryAcceptedByRider",
            json!({ "deliveryJobId": job_id, "orderId": order_id, "riderId": rider_id }),
        )
        .await;
    }
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (two held jobs)");

    let riders_query = "query { riders(input: { limit: 10, offset: 0 }) { riderId heldDelivery { id } } }";
    let resp = schema.execute(async_graphql::Request::new(riders_query).data(acting(RequestRole::Admin))).await;
    assert!(resp.errors.is_empty(), "riders errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    let list_row = data["riders"]
        .as_array()
        .expect("riders array")
        .iter()
        .find(|r| r["riderId"] == rider_id.to_string())
        .expect("the rider appears in the list");
    let list_held = list_row["heldDelivery"]["id"].as_str().expect("the list names a held job").to_string();

    let rider_query = format!(r#"query {{ rider(input: {{ riderId: "{rider_id}" }}) {{ heldDelivery {{ id }} }} }}"#);
    let resp = schema.execute(async_graphql::Request::new(rider_query).data(acting(RequestRole::Admin))).await;
    assert!(resp.errors.is_empty(), "rider errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    let detail_held = data["rider"]["heldDelivery"]["id"].as_str().expect("the detail names a held job").to_string();

    assert_eq!(
        list_held, detail_held,
        "the list and the detail must name the SAME held job for one rider (ADR-20260904-152807 §2, one custody truth); seeded jobs were {job_ids:?}"
    );
    // dba (correction pass): pin the DOCUMENTED tie-break itself, not just "list == detail" — the
    // two queries could still agree by planner happenstance (observed in this environment even
    // with the tie-break removed) without either one actually implementing
    // `ORDER BY requested_at DESC, delivery_job_id DESC`. With a genuine `requested_at` tie, the
    // contract says the GREATER `delivery_job_id` wins; asserting that exact value makes the
    // guard falsifiable regardless of what the planner happens to do.
    let expected_winner = job_ids.iter().max().expect("two seeded jobs").to_string();
    assert_eq!(
        list_held, expected_winner,
        "the tie-break contract (`requested_at DESC, delivery_job_id DESC`) says the GREATER delivery_job_id wins; seeded jobs were {job_ids:?}"
    );
}
