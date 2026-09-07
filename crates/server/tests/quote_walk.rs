//! THE WALK (beck iii, ADR-20260906-192007 D-A/D-F/D-G/D-J): a DB-gated end-to-end pass over the
//! REAL router/mailbox/projector stack, on the FULL migration chain — the `rider_standing_walk.rs`
//! shape, for the signed-quote checkout instead of rider standing.
//!
//! door-OPEN read → `PlaceOrder` with that quote, write door OPEN → ACCEPTED at the quoted total
//! WITH THE CATALOG HEAD MOVED since the mint (a later `ProductAdded` raises the price) — the
//! charge must be the quoted total (from the fold, at the coordinate the mint used), never HEAD's
//! new number. A second, independent worker fleet built with the write door OFF (`door_off_walk`)
//! proves `quote.verify` never fires and `placeOrder` still scores success — the CLOSED arm's
//! behaviour is untouched by this deliverable.
//!
//! Needs a real Postgres (`DATABASE_URL`); SKIPS (prints and returns) without one.

use std::sync::Arc;

use application::generated::services::{IdentityService, PaymentService};
use application::ports::{EventStore, GbpOrderLinkProbe, GoogleOwnershipVerifier};
use application::queries::{
    CartReadRepository, CatalogReadRepository, CustomerReadRepository,
    DeliveryPartnerAvailabilityReadRepository, DeliverySatisfactionReadRepository,
    DeliveryReadRepository, OrderReadRepository, PricingPolicyReadRepository,
    ProspectionReadRepository, ReadScope, RefundReadRepository, RestaurantReadRepository,
    UberEstimationPolicyReadRepository, UberSplitPolicyReadRepository,
};
use domain::generated::scalars::EmailAddress;
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

fn acting(role: RequestRole) -> server::ActingRole {
    server::Principal::role_binding(role, "test-subject".to_string(), Some(uuid::Uuid::from_u128(0x816)))
        .acting_role(role)
}

/// The SAME key both composition roots resolve — a real deployment resolves it from
/// `QUOTE_SIGNING_KEY_HMAC_SECRET`; this walk fixes it directly so the read-side mint and the
/// write-side verify agree without a shared env var.
fn walk_key() -> application::quote::SigningKey {
    application::quote::SigningKey::from_resolved_secret("walk-key", "walk-signing-secret")
}

/// Full migration chain from disk, in filename order (the `rider_standing_walk.rs` precedent) —
/// every table AND view this walk's resolvers actually query, with no hand-drifted DDL.
async fn apply_all_migrations(pool: &PgPool) {
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;").execute(pool).await.expect("recreate the public schema");
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
        sqlx::raw_sql(&sql).execute(pool).await.unwrap_or_else(|e| panic!("apply migration {}: {e}", f.display()));
    }
}

/// Append one fact directly to `domain_events` (the `rider_standing_walk.rs`/`delivery_read_model.rs`
/// idiom) — the birth facts this walk needs (Restaurant/Catalog/Cart) with no public mutation
/// shaped to seed them directly.
async fn append_event(pool: &PgPool, stream_name: &str, version: i32, event_type: &str, payload: serde_json::Value) {
    sqlx::query(
        "INSERT INTO domain_events \
         (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, payload, metadata, occurred_at) \
         VALUES ($1, $2, $3, $4, 'ADMIN', $5, NULL, $6, $7, NULL, now())",
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
    .unwrap_or_else(|e| panic!("append {event_type} on {stream_name}: {e}"));
}

/// The fold authority the door-OFF fleet is built over (checkpoint 2, beck item C): `verify_quote`
/// returns `Ok(None)` before ever touching `guard.fold_authority` while `guard.is_open()` is
/// false, so a fold that PANICS if called is the door-CLOSED fleet's own proof that "the fold is
/// never consulted with the door closed" — a silent, undetected read here would previously have
/// been indistinguishable from the fold agreeing with HEAD by coincidence.
struct PanickingFoldAuthority;
#[async_trait::async_trait]
impl application::ports::AsOfPriceAuthority for PanickingFoldAuthority {
    async fn as_of(
        &self,
        _catalog_id: domain::generated::scalars::CatalogId,
        _version: domain::catalog_as_of::CatalogVersion,
    ) -> Result<domain::catalog_as_of::AsOfCatalog, domain::shared::errors::DomainError> {
        panic!("PanickingFoldAuthority::as_of -- the write door is CLOSED, the fold must never be consulted");
    }
    async fn at_head(
        &self,
        _catalog_id: domain::generated::scalars::CatalogId,
        _correlation_id: uuid::Uuid,
    ) -> Result<domain::catalog_as_of::AsOfCatalog, domain::shared::errors::DomainError> {
        panic!("PanickingFoldAuthority::at_head -- the write door is CLOSED, the fold must never be consulted");
    }
}

/// The write-side worker fleet, parameterised on the quote guard's two interlocked gates (the
/// `rider_standing_walk.rs::spawn_mailbox_workers_with_door` shape) — the walk's main leg runs
/// both ON; `door_off_walk` below runs both OFF, over a SEPARATE fleet (two fleets never share a
/// lease on the same actor type in one test process). The door-OFF fleet's `QuoteGuard` is built
/// over `PanickingFoldAuthority` rather than the real Postgres adapter (beck item C): the CLOSED
/// arm must never reach the fold at all, so wiring a fold that panics on any call is a stronger
/// assertion than merely observing the right charge.
fn spawn_mailbox_workers_with_quote_door(pool: &PgPool, bus: actor_client::OperationStatusBus, quote_door_open: bool) {
    let fold_authority: Arc<dyn application::ports::AsOfPriceAuthority> = if quote_door_open {
        let fold_pool = infrastructure::PgAsOfCatalogRepository::bulkhead_pool(pool);
        Arc::new(infrastructure::PgAsOfCatalogRepository::new(fold_pool))
    } else {
        Arc::new(PanickingFoldAuthority)
    };
    let quote_guard: Arc<application::quote::QuoteGuard> = Arc::new(
        application::quote::QuoteGuard::resolve_at_boot(
            quote_door_open,
            quote_door_open,
            false,
            walk_key(),
            None,
            fold_authority,
        )
        .expect("both gates move together, the interlock cannot refuse"),
    );
    let deps = infrastructure::generated::command_router::CommandDeps {
        store: Arc::new(PgEventStore::new(pool.clone())),
        riders: Arc::new(infrastructure::PgRiderRepository::new(pool.clone())),
        members: Arc::new(infrastructure::PgMemberRepository::new(pool.clone())),
        support_contact: None,
        run_rider_restriction_door: false,
        run_member_access_grant: false,
        run_member_sign_in_door: false,
        run_restaurant_invitation: false,
        run_platform_access_grant: false,
        run_admin_sign_in_door: false,
        platform_members: Arc::new(infrastructure::PgPlatformMemberRepository::new(pool.clone())),
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
        payments: Arc::new(WalkGateway::default()),
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
        quote_guard,
    };
    let handler = Arc::new(infrastructure::mailbox::MailboxCommandHandler::new(deps));
    let observer = Arc::new(infrastructure::mailbox::StatusBusObserver::new(bus));
    for (actor_type, width) in infrastructure::generated::command_router::ACTOR_MAILBOXES {
        let worker = actor_runtime::MailboxWorker::new(
            pool.clone(),
            "w-quote-walk",
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

/// A REAL Stripe-shaped double -- the walk's own proof that the charge equals the quoted total is
/// the PERSISTED `PaymentIntentCreated.checkout.totalAmount` row (queried directly from
/// `domain_events` below), never a spy field on this gateway: a prior `last_amount_cents` mutex
/// here recorded the authorized amount but nothing ever asserted it (checkpoint 2, beck item C —
/// removed as dead).
#[derive(Default)]
struct WalkGateway;
#[async_trait::async_trait]
impl PaymentService for WalkGateway {
    async fn request(
        &self,
        input: application::generated::services::PaymentRequestInput,
        _meta: &application::generated::services::ServiceCallMeta,
    ) -> Result<application::generated::services::PaymentRequestOutput, domain::shared::errors::DomainError> {
        let _ = input;
        Ok(application::generated::services::PaymentRequestOutput {
            payment_intent_id: domain::generated::scalars::PaymentIntentId("pi_quote_walk".into()),
            client_secret: "pi_quote_walk_secret".into(),
        })
    }
    async fn capture(&self, _i: application::generated::services::PaymentCaptureInput, _m: &application::generated::services::ServiceCallMeta) -> Result<(), domain::shared::errors::DomainError> {
        Ok(())
    }
    async fn release(&self, _i: application::generated::services::PaymentReleaseInput, _m: &application::generated::services::ServiceCallMeta) -> Result<(), domain::shared::errors::DomainError> {
        Ok(())
    }
    async fn refund(&self, _i: application::generated::services::PaymentRefundInput, _m: &application::generated::services::ServiceCallMeta) -> Result<(), domain::shared::errors::DomainError> {
        Ok(())
    }
}

/// The read-side composition, parameterised on the read door (`RUN_FOLD_PRICED_CART_READ`) — OFF
/// for `door_off_walk` (mirrors `rider_standing_walk.rs::schema_over`'s per-leg parameter).
fn schema_over(pool: &PgPool, status_bus: actor_client::OperationStatusBus, read_door_open: bool) -> server::graphql_schema::CaptainSchema {
    let restaurants: Arc<dyn RestaurantReadRepository> = Arc::new(PgRestaurantRepository::new(pool.clone()));
    let prospection: Arc<dyn ProspectionReadRepository> = Arc::new(PgProspectionRepository::new(pool.clone()));
    let pricing_policy: Arc<dyn PricingPolicyReadRepository> = Arc::new(PgPricingPolicyRepository::new(pool.clone()));
    let uber_estimation_policy: Arc<dyn UberEstimationPolicyReadRepository> = Arc::new(PgUberEstimationPolicyRepository::new(pool.clone()));
    let uber_split_policy: Arc<dyn UberSplitPolicyReadRepository> = Arc::new(PgUberSplitPolicyRepository::new(pool.clone()));
    let catalogs: Arc<dyn CatalogReadRepository> = Arc::new(PgCatalogRepository::new(pool.clone()));
    let carts: Arc<dyn CartReadRepository> = Arc::new(PgCartRepository::new(pool.clone()));
    let orders: Arc<dyn OrderReadRepository> = Arc::new(PgOrderRepository::new(pool.clone()));
    let customers: Arc<dyn CustomerReadRepository> = Arc::new(PgCustomerRepository::new(pool.clone()));
    let deliveries: Arc<dyn DeliveryReadRepository> = Arc::new(PgDeliveryRepository::new(pool.clone()));
    let rider_restrictions: Arc<dyn application::queries::RiderRestrictionReadRepository> =
        Arc::new(infrastructure::persistence::rider_restriction_store::PgRiderRestrictionRepository::new(pool.clone()));
    let rider_roster: Arc<dyn application::queries::RiderRosterReadRepository> =
        Arc::new(infrastructure::persistence::rider_roster_store::PgRiderRosterRepository::new(pool.clone()));
    let member_authority: Arc<dyn application::queries::MemberAuthorityRepository> = Arc::new(infrastructure::PgMemberAuthorityRepository::new(pool.clone()));
    let restaurant_roster: Arc<dyn application::queries::RestaurantRosterReadRepository> = Arc::new(infrastructure::PgRestaurantRosterRepository::new(pool.clone()));
    let restaurant_invitations: Arc<dyn application::queries::RestaurantInvitationListReadRepository> = Arc::new(infrastructure::PgRestaurantInvitationListRepository::new(pool.clone()));
    let refunds: Arc<dyn RefundReadRepository> = Arc::new(PgRefundQueueRepository::new(pool.clone()));
    let delivery_satisfaction: Arc<dyn DeliverySatisfactionReadRepository> = Arc::new(PgDeliverySatisfactionRepository::new(pool.clone()));
    let delivery_partner_availabilities: Arc<dyn DeliveryPartnerAvailabilityReadRepository> = Arc::new(infrastructure::PgDeliveryPartnerAvailabilityRepository::new(pool.clone()));
    let reclamations: Arc<dyn application::queries::ReclamationReadRepository> = Arc::new(infrastructure::PgReclamationRepository::new(pool.clone()));
    let order_conversations: Arc<dyn application::queries::OrderConversationReadRepository> = Arc::new(infrastructure::PgOrderConversationRepository::new(pool.clone()));
    let customer_credit: Arc<dyn application::queries::CustomerCreditReadRepository> = Arc::new(infrastructure::PgCustomerCreditRepository::new(pool.clone()));
    let mailbox_lanes: Arc<dyn actor_client::supervision::MailboxLaneRepository> = Arc::new(infrastructure::persistence::mailbox_lanes::PgMailboxLaneRepository::new(pool.clone()));
    let event_store: Arc<dyn EventStore> = Arc::new(PgEventStore::new(pool.clone()));
    let ownership: Arc<dyn GoogleOwnershipVerifier> = Arc::new(FailClosedGoogleOwnershipVerifier);
    let gbp_probe: Arc<dyn GbpOrderLinkProbe> = Arc::new(UnverifiedGbpOrderLinkProbe);
    let auth_provider: Arc<dyn IdentityService> = Arc::new(FailClosedIdentityService);
    let payments: Arc<dyn PaymentService> = Arc::new(FailClosedPaymentGateway);
    let pm_state: Arc<dyn application::pm_state::PaymentProcessStateStore> = Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone()));
    let refund_state: Arc<dyn application::pm_state::RefundProcessStateStore> = Arc::new(infrastructure::persistence::PgRefundProcessState::new(pool.clone()));
    let mailbox: Arc<dyn actor_client::mailbox::Mailbox> = Arc::new(infrastructure::persistence::mailbox_store::PgMailbox::new(pool.clone()));
    server::graphql_schema::build_schema(
        Some(server::graphql_schema::ReadDeps {
            restaurants, prospection, pricing_policy, uber_estimation_policy, uber_split_policy,
            catalogs, carts, orders, order_conversations, customers, deliveries,
            rider_restrictions, rider_roster, member_authority, restaurant_roster,
            restaurant_invitations, refunds, delivery_satisfaction, delivery_partner_availabilities,
            reclamations, customer_credit, mailbox_lanes,
            service_window_horizon: Default::default(),
            support_contact: Some(EmailAddress("support@captain.food".to_string())),
            run_rider_restriction_door: server::graphql_schema::RunRiderRestrictionDoor(false),
            as_of_price_authority: Arc::new(infrastructure::PgAsOfCatalogRepository::new(
                infrastructure::PgAsOfCatalogRepository::bulkhead_pool(pool),
            )),
            run_fold_priced_cart_read: server::graphql_schema::RunFoldPricedCartRead(read_door_open),
            quote_minter: Arc::new(application::quote::QuoteMinter::new(walk_key())),
        }),
        Some(server::graphql_schema::WriteDeps {
            event_store, ownership, gbp_probe, auth_provider, payments, pm_state, refund_state,
            mailbox, status_bus,
            auth_sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
            slug_reservations: Arc::new(AlwaysFreeSlugs),
        }),
        None,
    )
}

struct AlwaysFreeSlugs;
#[async_trait::async_trait]
impl application::queries::SlugReservationRepository for AlwaysFreeSlugs {
    async fn reserve(&self, _slug: domain::generated::scalars::Slug, _restaurant_id: domain::generated::scalars::RestaurantId) -> Result<bool, domain::shared::errors::DomainError> {
        Ok(true)
    }
    async fn release(&self, _slug: domain::generated::scalars::Slug, _restaurant_id: domain::generated::scalars::RestaurantId) -> Result<(), domain::shared::errors::DomainError> {
        Ok(())
    }
}

async fn poll_operation(schema: &server::graphql_schema::CaptainSchema, message_id: &str) -> serde_json::Value {
    for _ in 0..100 {
        let query = format!(r#"query {{ operationStatus(input: {{ messageId: "{message_id}" }}) {{ messageId status errorCode message }} }}"#);
        // Polled as ADMIN, regardless of which role placed the order: `operationStatus` is
        // ownership-scoped (JWT subject / session match, or ADMIN) and this harness injects
        // neither a real `Principal` nor a `SessionHeader` for the CUSTOMER role -- ADMIN is the
        // one path the `mailbox_operation_owned` check admits unconditionally (the
        // `graphql_write_path.rs`/`rider_standing_walk.rs` precedent, which polls its OWN
        // ADMIN-run mutations the same way).
        let resp = schema.execute(async_graphql::Request::new(query).data(acting(RequestRole::Admin))).await;
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

fn tenant_of(restaurant: uuid::Uuid) -> server::graphql_tenant::TenantScope {
    server::graphql_tenant::TenantScope::Restaurant(domain::generated::scalars::RestaurantId(restaurant))
}

/// Seed an ACTIVE restaurant + a catalog with one offer @ `price_cents` + an OPEN cart (one line,
/// bound to `customer_id`) — the shared world both legs of the walk build on.
async fn seed_checkout_world(pool: &PgPool, restaurant_id: uuid::Uuid, catalog_id: uuid::Uuid, cart_id: uuid::Uuid, offer_id: uuid::Uuid, customer_id: uuid::Uuid, price_cents: i64) {
    append_event(pool, &format!("Restaurant-{restaurant_id}"), 1, "RestaurantRegistered", json!({
        "restaurantId": restaurant_id, "displayName": "Chez Quote",
        "address": { "line1": "1 Rue Nationale", "postalCode": "37000", "city": "Tours", "country": "FR" },
        "listingStatus": "ACTIVE_PARTNER", "openingHours": [],
    })).await;
    append_event(pool, &format!("Restaurant-{restaurant_id}"), 2, "RestaurantActivated", json!({ "restaurantId": restaurant_id })).await;
    append_event(pool, &format!("Catalog-{catalog_id}"), 1, "CatalogCreated", json!({
        "catalogId": catalog_id, "restaurantId": restaurant_id, "name": "Menu",
    })).await;
    append_event(pool, &format!("Catalog-{catalog_id}"), 2, "ProductAdded", json!({
        "catalogId": catalog_id, "restaurantId": restaurant_id,
        "product": {
            "id": uuid::Uuid::from_u128(0xD001), "catalogId": catalog_id, "restaurantId": restaurant_id,
            "name": "Burger", "tags": [], "imageIds": [], "taxRate": { "delivery": 10.0 },
            "offers": [{
                "id": offer_id, "productId": uuid::Uuid::from_u128(0xD001), "name": "Solo",
                "price": { "amountCents": price_cents, "currency": "EUR" },
                "availability": "AVAILABLE", "optionListIds": [],
            }],
        },
    })).await;
    append_event(pool, &format!("Cart-{cart_id}"), 1, "CartStarted", json!({
        "cartId": cart_id, "restaurantId": restaurant_id, "sessionId": uuid::Uuid::from_u128(0xD002), "customerId": customer_id,
    })).await;
    append_event(pool, &format!("Cart-{cart_id}"), 2, "CartLineAdded", json!({
        "cartId": cart_id,
        "line": { "cartLineId": uuid::Uuid::from_u128(0xD003), "offerId": offer_id, "quantity": 1, "selectedOptionIds": [] },
    })).await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (world seed)");
}

/// THE WALK, door-OPEN: mint at the read door, checkout through the write door, both interlocked
/// gates ON. The catalog HEAD moves (a second `ProductAdded` at 1900) AFTER the mint but BEFORE
/// `placeOrder` — the charge must be the QUOTED total (1500, the coordinate the mint used), never
/// HEAD's new 1900. Named `the_quote_walk_charges_the_price_shown_even_after_head_moves` until
/// checkpoint 2 (beck item C), which renamed it to the product's own outcome sentence, the SAME
/// name `commands::quote_guard_tests::the_customer_is_charged_the_price_they_were_shown` uses at
/// the unit level -- this is its end-to-end twin, over the real router/mailbox/projector stack.
#[tokio::test]
async fn the_customer_is_charged_the_price_they_were_shown() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let Some(url) = db_test_gate::database_url("quote_walk") else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    apply_all_migrations(&pool).await;

    let restaurant_id = uuid::Uuid::new_v4();
    let catalog_id = uuid::Uuid::new_v4();
    let cart_id = uuid::Uuid::new_v4();
    let offer_id = uuid::Uuid::new_v4();
    let customer_id = uuid::Uuid::new_v4();
    seed_checkout_world(&pool, restaurant_id, catalog_id, cart_id, offer_id, customer_id, 1500).await;

    let status_bus = actor_client::OperationStatusBus::default();
    spawn_mailbox_workers_with_quote_door(&pool, status_bus.clone(), true);
    let schema = schema_over(&pool, status_bus, true);

    // 1) The door-OPEN read: `current` mints a quote at the fold's coordinate.
    let current_q = r#"query { current { id totalAmount { amountCents } quote } }"#;
    let resp = schema
        .execute(
            async_graphql::Request::new(current_q)
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(domain::generated::scalars::CustomerId(customer_id)))
                .data(tenant_of(restaurant_id)),
        )
        .await;
    assert!(resp.errors.is_empty(), "current errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    assert_eq!(data["current"]["totalAmount"]["amountCents"], json!(1500), "the recap total at mint time");
    let quote = data["current"]["quote"].as_str().expect("a non-null quote (door OPEN)").to_string();
    eprintln!("TRANSCRIPT door-OPEN: current.totalAmount=1500 quote-minted-bytes={}", quote.len());

    // 2) HEAD MOVES: a second ProductAdded raises the SAME offer to 1900 -- AFTER the mint.
    append_event(&pool, &format!("Catalog-{catalog_id}"), 3, "ProductAdded", json!({
        "catalogId": catalog_id, "restaurantId": restaurant_id,
        "product": {
            "id": uuid::Uuid::from_u128(0xD001), "catalogId": catalog_id, "restaurantId": restaurant_id,
            "name": "Burger", "tags": [], "imageIds": [], "taxRate": { "delivery": 10.0 },
            "offers": [{
                "id": offer_id, "productId": uuid::Uuid::from_u128(0xD001), "name": "Solo",
                "price": { "amountCents": 1900, "currency": "EUR" },
                "availability": "AVAILABLE", "optionListIds": [],
            }],
        },
    })).await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (head move)");

    // 2b) Prove HEAD actually moved (checkpoint 2, beck item C): the LIVE projection's `current`
    // read now answers 1900 -- without this the test could pass vacuously if the ProductAdded
    // above never reached the projection at all (e.g. a broken `run_once`), which would make the
    // "1500 survives a head move" proof below meaningless (there would be no move to survive).
    let resp = schema
        .execute(
            async_graphql::Request::new(current_q)
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(domain::generated::scalars::CustomerId(customer_id)))
                .data(tenant_of(restaurant_id)),
        )
        .await;
    assert!(resp.errors.is_empty(), "current (post-head-move) errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    assert_eq!(data["current"]["totalAmount"]["amountCents"], json!(1900), "HEAD must have actually moved to 1900 before placeOrder -- otherwise this test proves nothing");

    // 3) `placeOrder` with the STALE-HEAD quote, write door OPEN.
    let mutation = format!(
        r#"mutation {{
            placeOrder(input: {{
                orderId: "{order_id}", restaurantId: "{restaurant_id}", cartId: "{cart_id}",
                customerId: "{customer_id}",
                customerContact: {{ displayName: "Jo", phone: "+33612345678" }},
                serviceType: COLLECTION, paymentMethodId: "pm_123", quote: "{quote}"
            }}) {{ messageId operationStatus }}
        }}"#,
        order_id = uuid::Uuid::new_v4(),
    );
    let resp = schema.execute(async_graphql::Request::new(mutation).data(acting(RequestRole::Customer))).await;
    assert!(resp.errors.is_empty(), "placeOrder errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    let message_id = data["placeOrder"]["messageId"].as_str().unwrap().to_string();
    let op = poll_operation(&schema, &message_id).await;
    assert_eq!(op["status"], "SUCCEEDED", "placeOrder operation: {op:?}");

    let intent_rows = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload FROM domain_events WHERE event_type = 'PaymentIntentCreated' ORDER BY position DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("the PaymentIntentCreated row");
    let charged = intent_rows["checkout"]["totalAmount"]["amountCents"].as_i64();
    eprintln!("TRANSCRIPT door-OPEN: HEAD moved to 1900, placeOrder operationStatus={:?}, charged={:?}", op["status"], charged);
    assert_eq!(charged, Some(1500), "the customer is charged the price they were shown (1500), never HEAD's new 1900 -- transcript: quote minted at 1500, HEAD moved to 1900, charged {charged:?}");
}

/// THE WALK, door-OFF: a SEPARATE worker fleet + schema with BOTH gates OFF. `placeOrder` with NO
/// quote still scores SUCCEEDED at the (now-1900) HEAD price — exactly today's behaviour, proving
/// the CLOSED arm is untouched and `quote.verify` never fires (farley's checkpoint verdict (a)).
#[tokio::test]
async fn the_quote_walk_door_off_still_places_the_order_at_head() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let Some(url) = db_test_gate::database_url("quote_walk_door_off") else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    apply_all_migrations(&pool).await;

    let restaurant_id = uuid::Uuid::new_v4();
    let catalog_id = uuid::Uuid::new_v4();
    let cart_id = uuid::Uuid::new_v4();
    let offer_id = uuid::Uuid::new_v4();
    let customer_id = uuid::Uuid::new_v4();
    seed_checkout_world(&pool, restaurant_id, catalog_id, cart_id, offer_id, customer_id, 1500).await;

    let status_bus = actor_client::OperationStatusBus::default();
    spawn_mailbox_workers_with_quote_door(&pool, status_bus.clone(), false);
    let schema = schema_over(&pool, status_bus, false);

    let mutation = format!(
        r#"mutation {{
            placeOrder(input: {{
                orderId: "{order_id}", restaurantId: "{restaurant_id}", cartId: "{cart_id}",
                customerId: "{customer_id}",
                customerContact: {{ displayName: "Jo", phone: "+33612345678" }},
                serviceType: COLLECTION, paymentMethodId: "pm_123"
            }}) {{ messageId operationStatus }}
        }}"#,
        order_id = uuid::Uuid::new_v4(),
    );
    let resp = schema.execute(async_graphql::Request::new(mutation).data(acting(RequestRole::Customer))).await;
    assert!(resp.errors.is_empty(), "placeOrder errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    let message_id = data["placeOrder"]["messageId"].as_str().unwrap().to_string();
    let op = poll_operation(&schema, &message_id).await;
    eprintln!("TRANSCRIPT door-OFF: placeOrder operationStatus={:?} (quote never submitted, door closed)", op["status"]);
    assert_eq!(op["status"], "SUCCEEDED", "door-OFF placeOrder operation: {op:?} -- the CLOSED arm must score success exactly as before this deliverable");

    // checkpoint 2, beck item C: the charge equals HEAD (1500 -- unmoved in this test) because the
    // CLOSED arm charges `priced.total_amount` (the live projection), never a quote. The fleet's
    // `QuoteGuard` was built over `PanickingFoldAuthority` (never the real Postgres adapter) --
    // had `verify_quote` consulted the fold with the door closed, this worker would have panicked
    // instead of reaching SUCCEEDED at all, so reaching this assertion is itself part of the proof.
    let intent_rows = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload FROM domain_events WHERE event_type = 'PaymentIntentCreated' ORDER BY position DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("the PaymentIntentCreated row");
    let charged = intent_rows["checkout"]["totalAmount"]["amountCents"].as_i64();
    assert_eq!(charged, Some(1500), "door-OFF charges HEAD (1500), the fold never consulted: {charged:?}");
}
