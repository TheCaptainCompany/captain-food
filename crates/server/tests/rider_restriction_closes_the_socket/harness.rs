//! Shared fixture for `rider_restriction_closes_the_socket.rs` (#639 part C step 5): the real
//! router/mailbox/EventBus/WS stack, bound to a genuine `127.0.0.1:0` listener. Mirrors
//! `rider_standing_walk.rs`'s composition (`apply_all_migrations`, `spawn_mailbox_workers_with_door`,
//! `schema_over`, `poll_operation`) extended with the `EventBus` wiring and a real bound TCP
//! listener + `graphql-transport-ws` client this record's own test needs.

use std::sync::Arc;

use application::generated::services::{IdentityService, PaymentService};
use application::ports::{EventStore, GbpOrderLinkProbe, GoogleOwnershipVerifier};
use application::queries::{
    CartReadRepository, CatalogReadRepository, CustomerReadRepository,
    DeliveryPartnerAvailabilityReadRepository, DeliveryReadRepository,
    DeliverySatisfactionReadRepository, OrderConversationReadRepository, OrderReadRepository,
    PricingPolicyReadRepository, ProspectionReadRepository, ReclamationReadRepository,
    RefundReadRepository, RestaurantReadRepository, RiderRosterReadRepository,
    UberEstimationPolicyReadRepository, UberSplitPolicyReadRepository,
};
use application::generated::rows::RiderRosterRow;
use async_trait::async_trait;
use domain::generated::scalars::{EmailAddress, RiderId};
use domain::shared::errors::DomainError;
use futures::{SinkExt, StreamExt};
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, FailClosedPaymentGateway,
    PgCartRepository, PgCatalogRepository, PgCustomerRepository, PgDeliveryRepository,
    PgDeliverySatisfactionRepository, PgEventStore, PgOrderRepository, PgPricingPolicyRepository,
    PgProspectionRepository, PgRefundQueueRepository, PgRestaurantRepository, PgRiderRepository,
    PgUberEstimationPolicyRepository, PgUberSplitPolicyRepository, ProjectionWorker,
    UnverifiedGbpOrderLinkProbe,
};
use server::graphql_acl::RequestRole;
use server::{
    AuthContext, CustomerIdentitySource, IdentitySources, LookupFailureReason,
    MemberIdentitySource, NoDatabaseMemberIdentity, PgRiderIdentity, ResolveRiderIdentity,
    RiderIdentityResolution, RiderIdentitySource,
};
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::client::ClientRequestBuilder;
use tokio_tungstenite::tungstenite::protocol::frame::CloseFrame;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::MaybeTlsStream;

pub static DB_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub type WsStream = tokio_tungstenite::WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

// ─── test-only signing material (the `rider_sign_in_door.rs`/`auth.rs` suite's, duplicated for the
// same sibling-test-module reason those files already document) ─────────────────────────────────

const TEST_EC_PRIVATE_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgTCTNdGfegiVKVsm+
vZXPOa4xJAt5OT8zMSblfCEwtW2hRANCAARto0Dk75fxl2IyLx89vwvjUWkJAb/p
5bKnk8sNetDUBHLVIGpXoxBRFJVNSeDN6QB9IHl6rqDLaZR4iqLatScL
-----END PRIVATE KEY-----
";
const TEST_SUPABASE_URL: &str = "https://captain-under-test.supabase.co";

async fn jwks_endpoint() -> String {
    let body = serde_json::json!({"keys":[{"kty":"EC","crv":"P-256","use":"sig","kid":"captain-test-es256",
        "alg":"ES256","x":"baNA5O-X8ZdiMi8fPb8L41FpCQG_6eWyp5PLDXrQ1AQ",
        "y":"ctUgalejEFEUlU1J4M3pAH0geXquoMtplHiKotq1Jws"}]});
    let app = axum::Router::new().route(
        "/jwks",
        axum::routing::get(move || {
            let body = body.clone();
            async move { axum::Json(body) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}/jwks")
}

/// A verified RIDER token carrying exactly `{ role: RIDER }` — the shape `stamp_rider_claim`
/// actually writes (no `rider_id`, no id of any kind; the seam supplies it from Postgres).
fn rider_jwt(sub: &str) -> String {
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::ES256);
    header.kid = Some("captain-test-es256".into());
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs()
        + 3600;
    let claims = serde_json::json!({
        "sub": sub,
        "aud": "authenticated",
        "iss": format!("{TEST_SUPABASE_URL}/auth/v1"),
        "exp": exp,
        "app_metadata": { "captain_food": { "role": "RIDER" } },
    });
    let key = jsonwebtoken::EncodingKey::from_ec_pem(TEST_EC_PRIVATE_KEY_PEM.as_bytes())
        .expect("test EC key parses");
    jsonwebtoken::encode(&header, &claims, &key).expect("sign test JWT")
}

// ─── migrations + fixtures (the `rider_standing_walk.rs` idiom, verbatim) ───────────────────────

pub async fn apply_all_migrations(pool: &PgPool) {
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

/// A rider's birth fact (no public `registerRider` mutation exists — the sign-in bridge issues it,
/// #639 part C step 2c-i, not this slice), keyed by `auth_ref` so the REAL Postgres identity seam
/// (`PgRiderIdentity`) resolves this test's signed JWT `sub` to `rider_id`. Runs the `Rider`
/// projector pass too: `PgRiderRepository::rider_id_by_auth_subject` reads the PROJECTED `rider`
/// table (`SELECT rider_id, standing FROM rider WHERE auth_ref = $1`), never `domain_events`
/// directly — unlike `rider_standing_walk.rs`, which injects `ReadScope` by hand and so never
/// exercises this seam, THIS suite's `connection_init` goes through the real seam end to end, so
/// the row must actually exist before the first connect.
pub async fn seed_rider(pool: &PgPool, rider_id: uuid::Uuid, auth_ref: &str) {
    append_event(
        pool,
        &format!("Rider-{rider_id}"),
        1,
        "RiderRegistered",
        serde_json::json!({
            "riderId": rider_id, "authRef": auth_ref, "displayName": "Socket Test Rider",
            "phone": "+33611112222", "status": "OFFLINE"
        }),
    )
    .await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (rider birth)");
}

// ─── the write side: the real mailbox worker fleet, restrict door ON, sharing ONE EventBus ──────

pub fn spawn_mailbox_workers(
    pool: &PgPool,
    bus: actor_client::OperationStatusBus,
    event_bus: infrastructure::EventBus,
) {
    let deps = infrastructure::generated::command_router::CommandDeps {
        // `CommandDeps.store` is a STAGING event store for the "Handled" delivery path — the real
        // Postgres write and bus publish for THAT path happen in `MailboxCommandHandler`'s own
        // flush, wired below via `.with_event_bus(...)`, never through this store's own `append`.
        // `with_bus` here is harmless (this store's OWN append/publish path is simply unused by
        // the staged-delivery flow) but kept for parity with the real composition root.
        store: Arc::new(PgEventStore::with_bus(pool.clone(), event_bus.clone())),
        riders: Arc::new(PgRiderRepository::new(pool.clone())),
        members: Arc::new(infrastructure::PgMemberRepository::new(pool.clone())),
        support_contact: None,
        run_rider_restriction_door: true,
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
    // THE actual bus wiring for a "Handled" delivery (mailbox/handler.rs's own doc comment):
    // `deps.store` backs a per-delivery `StagingEventStore`, and the REAL Postgres write +
    // publish happen in `flush_staged_in_tx` + `fanout_delivery`, gated on THIS handler's OWN
    // `event_bus` field — `CommandDeps.store`'s `with_bus` is unused by this delivery path.
    let handler =
        Arc::new(infrastructure::mailbox::MailboxCommandHandler::new(deps).with_event_bus(event_bus));
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

/// #639 part C step 5's own roster injection point — the ADMIN `riders`/`rider` queries only.
/// Round 2 R2-0 moved the watcher's Lagged re-derivation OFF this read model entirely (onto the
/// identity seam, [`FirstResolveThenAlwaysErr`] below), so the only variant left is the real
/// Postgres repository: nothing in this suite injects a roster failure any more.
pub enum Roster {
    Real(PgPool),
}

#[async_trait]
impl RiderRosterReadRepository for Roster {
    async fn all(&self) -> Result<Vec<RiderRosterRow>, DomainError> {
        match self {
            Roster::Real(pool) => {
                infrastructure::persistence::rider_roster_store::PgRiderRosterRepository::new(pool.clone())
                    .all()
                    .await
            }
        }
    }
    async fn by_id(&self, rider_id: RiderId) -> Result<Option<RiderRosterRow>, DomainError> {
        match self {
            Roster::Real(pool) => {
                infrastructure::persistence::rider_roster_store::PgRiderRosterRepository::new(pool.clone())
                    .by_id(rider_id)
                    .await
            }
        }
    }
}

/// #639 part C step 5 round 2 (R2-0): the watcher's Lagged/Closed re-derivation now reads through
/// the SAME identity seam `connection_init` resolves through, so scenario (6c)'s "a lookup error
/// never terminates" (ADR-20260904-124600 §3) must inject the failure THERE. The connection's own
/// initial `connection_init` resolution (the FIRST call) must still succeed against real Postgres
/// — a rider that never connects as a rider proves nothing about the re-derivation — and every
/// call after that (the watcher's bounded retries) fails, never a static always-err resolver.
pub struct FirstResolveThenAlwaysErr {
    real: PgRiderIdentity,
    calls: std::sync::atomic::AtomicUsize,
}

impl FirstResolveThenAlwaysErr {
    pub fn new(pool: PgPool) -> Self {
        Self {
            real: PgRiderIdentity::new(Arc::new(PgRiderRepository::new(pool))),
            calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl ResolveRiderIdentity for FirstResolveThenAlwaysErr {
    async fn resolve(&self, auth_subject: &str) -> RiderIdentityResolution {
        if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            self.real.resolve(auth_subject).await
        } else {
            RiderIdentityResolution::LookupFailed(LookupFailureReason::Repository)
        }
    }
}

/// A `SlugReservationRepository` that grants every request (mirrors `rider_standing_walk.rs`).
struct AlwaysFreeSlugs;

#[async_trait]
impl application::queries::SlugReservationRepository for AlwaysFreeSlugs {
    async fn reserve(
        &self,
        _slug: domain::generated::scalars::Slug,
        _restaurant_id: domain::generated::scalars::RestaurantId,
    ) -> Result<bool, DomainError> {
        Ok(true)
    }
    async fn release(
        &self,
        _slug: domain::generated::scalars::Slug,
        _restaurant_id: domain::generated::scalars::RestaurantId,
    ) -> Result<(), DomainError> {
        Ok(())
    }
}

/// The composition-root wiring (mirrors `rider_standing_walk.rs::schema_over`), extended with the
/// `EventBus` the socket watcher subscribes to and the injectable [`Roster`].
pub fn schema_over(
    pool: &PgPool,
    status_bus: actor_client::OperationStatusBus,
    event_bus: infrastructure::EventBus,
    roster: Roster,
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
    let rider_roster: Arc<dyn RiderRosterReadRepository> = Arc::new(roster);
    let refunds: Arc<dyn RefundReadRepository> = Arc::new(PgRefundQueueRepository::new(pool.clone()));
    let delivery_satisfaction: Arc<dyn DeliverySatisfactionReadRepository> =
        Arc::new(PgDeliverySatisfactionRepository::new(pool.clone()));
    let delivery_partner_availabilities: Arc<dyn DeliveryPartnerAvailabilityReadRepository> =
        Arc::new(infrastructure::PgDeliveryPartnerAvailabilityRepository::new(pool.clone()));
    let reclamations: Arc<dyn ReclamationReadRepository> =
        Arc::new(infrastructure::PgReclamationRepository::new(pool.clone()));
    let order_conversations: Arc<dyn OrderConversationReadRepository> =
        Arc::new(infrastructure::PgOrderConversationRepository::new(pool.clone()));
    let customer_credit: Arc<dyn application::queries::CustomerCreditReadRepository> =
        Arc::new(infrastructure::PgCustomerCreditRepository::new(pool.clone()));
    let mailbox_lanes: Arc<dyn actor_client::supervision::MailboxLaneRepository> =
        Arc::new(infrastructure::persistence::mailbox_lanes::PgMailboxLaneRepository::new(pool.clone()));
    let event_store: Arc<dyn EventStore> = Arc::new(PgEventStore::with_bus(pool.clone(), event_bus.clone()));
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
            refunds,
            delivery_satisfaction,
            delivery_partner_availabilities,
            reclamations,
            customer_credit,
            mailbox_lanes,
            service_window_horizon: Default::default(),
            support_contact: Some(EmailAddress("support@captain.food".to_string())),
            run_rider_restriction_door: server::graphql_schema::RunRiderRestrictionDoor(true),
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
            auth_sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
            slug_reservations: Arc::new(AlwaysFreeSlugs),
        }),
        Some(event_bus),
    )
}

/// The role-guard witness for the ADMIN `restrictRider` mutation (mirrors `rider_standing_walk.rs`).
fn acting(role: RequestRole) -> server::ActingRole {
    server::Principal::role_binding(role, "test-subject".to_string(), Some(uuid::Uuid::from_u128(0x6394_5)))
        .acting_role(role)
}

/// Poll `operationStatus(messageId)` until non-PENDING (mirrors `rider_standing_walk.rs`).
async fn poll_operation(schema: &server::graphql_schema::CaptainSchema, message_id: &str) -> serde_json::Value {
    for _ in 0..100 {
        let query = format!(
            r#"query {{ operationStatus(input: {{ messageId: "{message_id}" }}) {{ messageId status errorCode message }} }}"#
        );
        let resp = schema
            .execute(async_graphql::Request::new(query).data(acting(RequestRole::Admin)))
            .await;
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

/// `restrictRider` as ADMIN, through the schema — appends `RiderRestricted` through the REAL
/// `PgEventStore::with_bus`, which publishes on the SAME `EventBus` the WS server's watcher
/// subscribed to. Waits for the acceptance to reach SUCCEEDED before returning (the fact is
/// durably appended AND published by then — `PgEventStore::append` publishes AFTER commit).
pub async fn restrict_rider_and_wait(
    schema: &server::graphql_schema::CaptainSchema,
    pool: &PgPool,
    rider_id: uuid::Uuid,
) {
    let mutation = format!(
        r#"mutation {{ restrictRider(input: {{ riderId: "{rider_id}", ground: RIDER_REQUESTED }}) {{ messageId operationStatus }} }}"#
    );
    let resp = schema
        .execute(async_graphql::Request::new(mutation).data(acting(RequestRole::Admin)))
        .await;
    assert!(resp.errors.is_empty(), "restrictRider errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json");
    let message_id = data["restrictRider"]["messageId"].as_str().unwrap().to_string();
    let op = poll_operation(schema, &message_id).await;
    assert_eq!(op["status"], "SUCCEEDED", "restrictRider operation: {op:?}");
    // The read model (for `myStanding`/roster reads after a reconnect) — the watcher itself needs
    // NO projector pass, since it reads the bus directly, but the reconnect scenario's fresh
    // `connection_init` resolution reads through `RiderIdentityRepository`, which is `Rider.standing`
    // itself (the SAME checkpoint `RiderRegistered` advanced), not a separate projection.
    let _ = ProjectionWorker::new(pool.clone()).run_once().await;
}

/// Flood `n` UNRELATED riders' `RiderRestricted` envelopes directly onto the bus — the harness's
/// way to force `RecvError::Lagged` on a tiny-capacity bus without touching Postgres (§9's "tiny
/// bus capacity or flood").
pub async fn flood_other_riders_restricted(bus: &infrastructure::EventBus, n: usize) {
    for _ in 0..n {
        bus.publish(infrastructure::AppendedEvent {
            stream_name: format!("Rider-{}", uuid::Uuid::new_v4()),
            event_type: "RiderRestricted".to_string(),
            correlation_id: uuid::Uuid::new_v4(),
            position: 1,
        });
    }
    // Give the watcher tasks a scheduling window to actually observe the Lagged error before the
    // test moves on to its own assertions.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
}

/// The production rider-identity seam over a real Postgres pool — every scenario except (6c).
pub fn real_rider_identity(pool: &PgPool) -> Arc<dyn ResolveRiderIdentity> {
    Arc::new(PgRiderIdentity::new(Arc::new(PgRiderRepository::new(pool.clone()))))
}

// ─── the WS server + client (§9: the real transport this record's own test needs) ───────────────

/// `rider_identity` is the SAME seam the watcher's Lagged/Closed re-derivation now reads through
/// (R2-0): [`real_rider_identity`] for every scenario except (6c), which hands in
/// [`FirstResolveThenAlwaysErr`] instead.
pub async fn bind_ws_server(
    schema: server::graphql_schema::CaptainSchema,
    socket_close_gate: bool,
    rider_identity: Arc<dyn ResolveRiderIdentity>,
) -> std::net::SocketAddr {
    let auth = AuthContext::from_config(jwks_endpoint().await, TEST_SUPABASE_URL.into());
    let app = server::graphql_routes_with_socket_close_gate(
        schema,
        server::TenantLookup(None),
        IdentitySources {
            customer: CustomerIdentitySource::Claim,
            rider: RiderIdentitySource::new(rider_identity),
            member: MemberIdentitySource::new(std::sync::Arc::new(NoDatabaseMemberIdentity)),
        },
        server::graphql_rider_socket::RunRiderRestrictionSocketClose(socket_close_gate),
    )
    .layer(axum::Extension(auth));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    addr
}

/// Connect a rider's WS client to `/rider/graphql` on `graphql-transport-ws`, send
/// `connection_init` with a freshly signed token for `sub`, and wait for `connection_ack` — the
/// three steps every scenario needs before it can do anything else.
pub async fn connect_and_init(addr: std::net::SocketAddr, sub: &str) -> WsStream {
    let uri: tokio_tungstenite::tungstenite::http::Uri =
        format!("ws://{addr}/rider/graphql").parse().expect("uri parses");
    let builder = ClientRequestBuilder::new(uri).with_sub_protocol("graphql-transport-ws");
    let (mut ws, _response) = tokio_tungstenite::connect_async(builder).await.expect("ws connects");

    let jwt = rider_jwt(sub);
    let init = serde_json::json!({
        "type": "connection_init",
        "payload": { "Authorization": format!("Bearer {jwt}") }
    });
    ws.send(Message::text(init.to_string())).await.expect("send connection_init");

    let msg = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
        .await
        .expect("connection_ack within 5s")
        .expect("stream open")
        .expect("no transport error");
    let Message::Text(text) = msg else { panic!("expected a text frame, got {msg:?}") };
    let v: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(v["type"], "connection_ack", "expected connection_ack, got {v:?}");
    ws
}

/// Send a `subscribe` frame and read frames until this `id`'s `next` (or `error`) message arrives,
/// skipping any interleaved frame for a DIFFERENT id (e.g. a keepalive ping).
pub async fn send_operation_and_await_next(ws: &mut WsStream, id: &str, query: &str) -> serde_json::Value {
    let payload = serde_json::json!({
        "type": "subscribe",
        "id": id,
        "payload": { "query": query }
    });
    ws.send(Message::text(payload.to_string())).await.expect("send subscribe");
    loop {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(10), ws.next())
            .await
            .expect("a response within 10s")
            .expect("stream open")
            .expect("no transport error");
        match msg {
            Message::Text(text) => {
                let v: serde_json::Value = serde_json::from_str(&text).expect("json");
                if v["id"] == id && (v["type"] == "next" || v["type"] == "error") {
                    return v["payload"].clone();
                }
            }
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected frame while awaiting {id}: {other:?}"),
        }
    }
}

/// Either half of the race in scenario (3): the operation's own response, or the socket closing
/// before one could be sent (the watcher's close and the operation's own resolution are two
/// independently scheduled tasks sharing one output channel — see the caller's doc comment).
pub enum OperationOrClose {
    Response(serde_json::Value),
    Closed(CloseFrame),
}

/// [`send_operation_and_await_next`]'s tolerant twin: a Close frame while awaiting this `id`'s
/// response is a VALID outcome here, not a panic.
pub async fn send_operation_or_close(
    ws: &mut WsStream,
    id: &str,
    query: &str,
    timeout: std::time::Duration,
) -> OperationOrClose {
    let payload = serde_json::json!({
        "type": "subscribe",
        "id": id,
        "payload": { "query": query }
    });
    ws.send(Message::text(payload.to_string())).await.expect("send subscribe");
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let msg = tokio::time::timeout(remaining, ws.next())
            .await
            .unwrap_or_else(|_| panic!("expected a response or a Close within {timeout:?}, got neither"))
            .expect("stream open")
            .expect("no transport error");
        match msg {
            Message::Text(text) => {
                let v: serde_json::Value = serde_json::from_str(&text).expect("json");
                if v["id"] == id && (v["type"] == "next" || v["type"] == "error") {
                    return OperationOrClose::Response(v["payload"].clone());
                }
            }
            Message::Close(Some(frame)) => return OperationOrClose::Closed(frame),
            Message::Close(None) => panic!("Close frame carried no code/reason"),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected frame while awaiting {id} or Close: {other:?}"),
        }
    }
}

/// POSITIVELY assert a Close frame arrives within `timeout` — never "nothing arrived" (beck).
pub async fn expect_close(ws: &mut WsStream, timeout: std::time::Duration) -> CloseFrame {
    loop {
        let msg = tokio::time::timeout(timeout, ws.next())
            .await
            .unwrap_or_else(|_| panic!("expected a Close frame within {timeout:?}, got nothing"));
        match msg {
            Some(Ok(Message::Close(Some(frame)))) => return frame,
            Some(Ok(Message::Close(None))) => panic!("Close frame carried no code/reason"),
            Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Text(_))) => continue,
            Some(Ok(other)) => panic!("unexpected frame while awaiting Close: {other:?}"),
            Some(Err(e)) => panic!("transport error while awaiting Close: {e}"),
            None => panic!("stream ended with no Close frame"),
        }
    }
}

/// The negative twin: `true` iff NO Close frame arrived within `timeout` (any other frame is
/// drained and ignored) — used only where the record's own text calls for the negative
/// ("stays open"), always alongside a POSITIVE round trip proving the socket still answers.
pub async fn expect_no_close_within(ws: &mut WsStream, timeout: std::time::Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return true;
        }
        match tokio::time::timeout(remaining, ws.next()).await {
            Ok(Some(Ok(Message::Close(_)))) => return false,
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => panic!("transport error: {e}"),
            Ok(None) => return true,
            Err(_) => return true,
        }
    }
}
