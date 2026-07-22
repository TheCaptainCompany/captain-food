//! Captain.Food server (Axum BFF) — the composition root (ADR-0035).
//!
//! DI happens here: infrastructure adapters are injected behind application ports, then the HTTP/GraphQL
//! surface is built over them. Exposed as a library so `desktop` (Tauri) can embed it in-process.
//!
//! Endpoints:
//! - `/ping` → `pong` — liveness (process is up; touches nothing). Used by uptime pingers / keep-warm.
//! - `/health` — readiness gate (ADR-0043): `200` only when the DB is reachable AND its schema version is
//!   `>= REQUIRED_SCHEMA_VERSION`; else `503`. Migrations are applied out-of-band by **sqlx-cli in CI**.
//!   Every response carries `version` (the build's git SHA, ADR-20260721-175411) for failure diagnostics.
//! - `/projector` — projection-worker readiness (running / checkpoint / head / lag / lastTickAt).
//! - `/saga` — process-manager (saga) runner readiness, same shape as `/projector`.
//! - `/{role}/graphql` (+ `/{role}/voyager`) — the GraphQL BFF (ADR-0006), see `graphql`.
//! - `POST /internal/sirene/drain` — wakes the SIRENE sync worker after a CI ingestion run (ADR-0045);
//!   secured by the `INTERNAL_TRIGGER_TOKEN` shared secret (`x-internal-token` header).
//! - `POST /adapters/stripe/webhooks` — Stripe webhook ingestion (inbound payment facts through the ACL);
//!   secured by `Stripe-Signature` HMAC verification against `STRIPE_WEBHOOK_SECRET` (fail-closed).
//!
//! Every response (all routes) carries `X-VERSION` = the running build's short git SHA (ADR-20260721-175411),
//! so any client can read which deploy served it without calling `/health` (see `response_timing`).
//!
//! The projection worker (ADR-0040) runs **in-process** here for now (Render Background Workers are paid),
//! gated by `RUN_PROJECTOR` (default on) so it can graduate to a dedicated worker with no logic change.
//! The SIRENE sync worker (ADR-0045) follows the same pattern: in-process, primarily woken by the CI
//! ingestion's ping, with a slow safety-net poll loop gated by `RUN_SIRENE_WORKER` (default on).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    extract::{Request, State},
    http::{HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

use application::queries::{
    CartReadRepository, CatalogReadRepository, CustomerReadRepository,
    DeliveryPartnerAvailabilityReadRepository, DeliverySatisfactionReadRepository,
    DeliveryReadRepository, OrderReadRepository,
    PricingPolicyReadRepository, ProspectionReadRepository, RefundReadRepository,
    RestaurantReadRepository, UberEstimationPolicyReadRepository, UberSplitPolicyReadRepository,
};
use infrastructure::{
    EventBus, FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, FailClosedPaymentGateway,
    PgCartRepository, PgCatalogRepository, PgCustomerRepository,
    PgDeliveryPartnerAvailabilityRepository, PgDeliveryRepository, PgDeliverySatisfactionRepository,
    PgEventStore,
    PgOrderRepository, PgPricingPolicyRepository, PgProspectionRepository, PgRefundQueueRepository,
    PgRestaurantRepository, PgUberEstimationPolicyRepository, PgUberSplitPolicyRepository, ProcessManagerRunner,
    ProcessManagerStatus, ProjectionStatus, ProjectionWorker, SireneSyncWorker,
    UnverifiedGbpOrderLinkProbe,
};
use avelo37_adapter::Avelo37WebhookIngestor;
use coopcycle_adapter::CoopCycleWebhookIngestor;
use uber_direct_adapter::UberDirectWebhookIngestor;
use stripe_adapter::StripeWebhookIngestor;
use shared_types::HealthDto;

use graphql::schema::{ReadDeps, WriteDeps};

mod auth;
/// The expose-gated `/services/*` surface + module index, GENERATED from specs/services.yaml
/// (issue #26, ADR-20260719-214500).
pub mod generated;
mod graphql;
mod hosts;

/// The role-as-path ACL seam (RequestRole/RoleGuard, ADR-0006), re-exported so integration tests can
/// execute the schema under a specific role (the HTTP layer injects it from the URL path).
pub use graphql::acl as graphql_acl;
pub use graphql::session as graphql_session;
// The verified request principal — exposed for the subscription-ownership integration tests
// (the generated resolvers reach it as crate::auth::Principal).
pub use auth::Principal;
/// The schema composition surface (build_schema/ReadDeps/WriteDeps), re-exported so integration tests
/// (and the embedding `desktop` shell) can build the master schema over their own adapters.
pub use graphql::schema as graphql_schema;

/// Minimal health/edge-proof: lets the `desktop` (Tauri) shell embed the server in-process and proves the
/// server → shared_types edge (ADR-0035). The real DI graph is built in `router()`.
pub fn wire() -> HealthDto {
    HealthDto::ok()
}

/// The schema version this build requires. Migrations are applied by **sqlx-cli in CI** (ADR-0043); the app
/// only checks the DB has reached at least this version. Bump when adding a migration this build depends on.
/// The gate is `>=` (never `==`) so an older build still runs against a newer DB (rollback-by-redeploy).
/// `20260720030000` = the command/inbound journals (ADR-20260720-015300/-015400): every mutation now
/// writes `command_journal` at acceptance, so the app cannot serve writes without it.
/// `20260721150000` = the Uber Direct webhook mirror (external_uber_direct_events, #57): the adapter's
/// inbound ingestor stages verified facts into it, so the app must not serve without the table.
pub const REQUIRED_SCHEMA_VERSION: i64 = 20260722000000;

/// The precise build identity, for diagnostics (ADR-20260721-175411). CI bakes `CAPTAIN_BUILD_VERSION`
/// (the short 7-char git commit SHA the image was built from, e.g. `829f4ad`) into the deployed image — see
/// `.github/workflows/build-image.yml` + the `Dockerfile` runtime stage — and `/health` reports it in
/// EVERY state, including `degraded`/`down`: when the app is failing is precisely when you need to know
/// which build is running. Falls back to `dev-<crate version>` for local / uncontainerized runs where the
/// env var is unset. Read once and cached (the value never changes for a process).
pub fn build_version() -> &'static str {
    static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSION.get_or_init(|| {
        std::env::var("CAPTAIN_BUILD_VERSION")
            .unwrap_or_else(|_| format!("dev-{}", env!("CARGO_PKG_VERSION")))
    })
}

/// Readiness states published by the heartbeat, read by `/health`.
mod db_state {
    pub const NOT_CONFIGURED: u8 = 0; // DATABASE_URL unset
    pub const DOWN: u8 = 1; // unreachable, or `_sqlx_migrations` does not exist yet
    pub const SCHEMA_BEHIND: u8 = 2; // reachable, but max(applied version) < REQUIRED_SCHEMA_VERSION
    pub const HEALTHY: u8 = 3; // reachable and schema is at/after the required version
}

/// Cached readiness snapshot; refreshed every 30s by the heartbeat.
#[derive(Clone)]
struct Snapshot {
    state: u8,
    /// Highest successfully-applied migration version in the DB (`-1` if none/unknown).
    applied_version: i64,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self { state: db_state::NOT_CONFIGURED, applied_version: -1 }
    }
}

#[derive(Clone)]
pub struct AppState {
    snap: Arc<Mutex<Snapshot>>,
    /// Live projection-worker status when the worker runs in-process; `None` when not started.
    projector_status: Option<Arc<Mutex<ProjectionStatus>>>,
    /// Live saga-runner (process managers, actors.yaml) status when it runs in-process; `None` when
    /// not started.
    saga_status: Option<Arc<Mutex<ProcessManagerStatus>>>,
}

/// Build the Axum router: `/ping`, `/health`, `/projector`, and the role-as-path GraphQL routes. Reads
/// `DATABASE_URL`; when present it opens a lazy pool used by the heartbeat, the read-model repo (injected
/// into GraphQL), and the in-process projection worker.
pub fn router() -> Router {
    let snap = Arc::new(Mutex::new(Snapshot::default()));
    // In-process appended-event bus: every event-store append in THIS process (GraphQL mutations,
    // Stripe/HubRise inbound facts) is broadcast after commit, feeding the GraphQL subscriptions.
    // Constructed unconditionally so the schema always carries a bus (subscriptions without a DB
    // simply never receive anything).
    let event_bus = EventBus::default();
    // Journal-transition broadcast (ADR-20260720-015500): the acceptance-first dispatch publishes
    // every command_journal transition here; operationStatusChanged streams it. Like the event bus,
    // constructed unconditionally so the schema always carries one.
    let operation_status_bus = infrastructure::OperationStatusBus::default();
    let mut read_deps: Option<ReadDeps> = None;
    let mut write_deps: Option<WriteDeps> = None;
    let mut projector_status: Option<Arc<Mutex<ProjectionStatus>>> = None;
    let mut saga_status: Option<Arc<Mutex<ProcessManagerStatus>>> = None;
    let mut sirene_worker: Option<Arc<SireneSyncWorker>> = None;
    let mut stripe_ingestor: Option<Arc<StripeWebhookIngestor>> = None;
    let mut avelo37_ingestor: Option<Arc<Avelo37WebhookIngestor>> = None;
    let mut coopcycle_ingestor: Option<Arc<CoopCycleWebhookIngestor>> = None;
    // The CoopCycle federation registry (COOPCYCLE_INSTANCES) — shared by the outbound gateway (base
    // URL + OAuth per instance) and the inbound webhook route (per-instance secret). Empty ⇒ no-op.
    let coopcycle_registry = coopcycle_adapter::CoopCycleRegistry::from_env()
        .unwrap_or_else(|e| {
            eprintln!("COOPCYCLE_INSTANCES misconfigured, treating as unset: {e}");
            None
        })
        .unwrap_or_default();
    let mut uber_direct_ingestor: Option<Arc<UberDirectWebhookIngestor>> = None;
    // The Uber Direct config (UBER_DIRECT_*) — shared by the outbound gateway (OAuth2 + create
    // delivery) and the inbound webhook route (signing secret). None ⇒ unconfigured (no-op stand-in).
    let uber_direct_config = uber_direct_adapter::UberDirectConfig::from_env().unwrap_or_else(|e| {
        eprintln!("UBER_DIRECT_* misconfigured, treating as unset: {e}");
        None
    });
    let mut hubrise_state = hubrise_adapter::HubRiseWebhookState::default();
    let mut inbound_drain: Option<Arc<infrastructure::InboundEventsDrainWorker>> = None;

    match std::env::var("DATABASE_URL") {
        Ok(url) if !url.is_empty() => match PgPoolOptions::new()
            .max_connections(5)
            .min_connections(1)
            .acquire_timeout(Duration::from_secs(10))
            .idle_timeout(Duration::from_secs(240))
            .max_lifetime(Duration::from_secs(1800))
            .connect_lazy(&url)
        {
            Ok(pool) => {
                // Configured but unconfirmed until the first probe: report DOWN, not NOT_CONFIGURED.
                snap.lock().expect("health snapshot mutex").state = db_state::DOWN;
                spawn_heartbeat(pool.clone(), snap.clone());

                // Read-model repositories injected into GraphQL resolvers (ADR-0035 composition root).
                let restaurants: Arc<dyn RestaurantReadRepository> =
                    Arc::new(PgRestaurantRepository::new(pool.clone()));
                // The HubRise connect flow (wired below) shares the restaurant read model.
                let hubrise_restaurants = restaurants.clone();
                let prospection: Arc<dyn ProspectionReadRepository> =
                    Arc::new(PgProspectionRepository::new(pool.clone()));
                let pricing_policy: Arc<dyn PricingPolicyReadRepository> =
                    Arc::new(PgPricingPolicyRepository::new(pool.clone()));
                let uber_estimation_policy: Arc<dyn UberEstimationPolicyReadRepository> =
                    Arc::new(PgUberEstimationPolicyRepository::new(pool.clone()));
                let uber_split_policy: Arc<dyn UberSplitPolicyReadRepository> =
                    Arc::new(PgUberSplitPolicyRepository::new(pool.clone()));
                let catalogs: Arc<dyn CatalogReadRepository> =
                    Arc::new(PgCatalogRepository::new(pool.clone()));
                let carts: Arc<dyn CartReadRepository> =
                    Arc::new(PgCartRepository::new(pool.clone()));
                let orders: Arc<dyn OrderReadRepository> =
                    Arc::new(PgOrderRepository::new(pool.clone()));
                let customers: Arc<dyn CustomerReadRepository> =
                    Arc::new(PgCustomerRepository::new(pool.clone()));
                let deliveries: Arc<dyn DeliveryReadRepository> =
                    Arc::new(PgDeliveryRepository::new(pool.clone()));
                let refunds: Arc<dyn RefundReadRepository> =
                    Arc::new(PgRefundQueueRepository::new(pool.clone()));
                let delivery_satisfaction: Arc<dyn DeliverySatisfactionReadRepository> =
                    Arc::new(PgDeliverySatisfactionRepository::new(pool.clone()));
                let delivery_partner_availabilities: Arc<dyn DeliveryPartnerAvailabilityReadRepository> =
                    Arc::new(PgDeliveryPartnerAvailabilityRepository::new(pool.clone()));
                read_deps = Some(ReadDeps {
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
                    delivery_satisfaction,
                    delivery_partner_availabilities,
                });

                // Write side (CQRS commands): the event store behind the mutation resolvers, plus the
                // Google and Supabase Auth seam adapters (fail-closed stand-ins until the real
                // integrations land).
                write_deps = Some(WriteDeps {
                    event_store: Arc::new(PgEventStore::with_bus(pool.clone(), event_bus.clone())),
                    ownership: Arc::new(FailClosedGoogleOwnershipVerifier),
                    gbp_probe: Arc::new(UnverifiedGbpOrderLinkProbe),
                    // The `identity` service resolved through the GENERATED topology binding
                    // (services.yaml `binding: local`, issue #50): fail-closed stand-in until the
                    // real Supabase ACL adapter lands.
                    auth_provider: infrastructure::generated::service_bindings::identity_service(
                        || Arc::new(FailClosedIdentityService),
                    )
                    .expect("identity service binding (services.yaml)"),
                    // The `payment` service resolved through the GENERATED topology binding
                    // (services.yaml `binding: local`, issue #26): the composition root only supplies
                    // the in-process constructor — the real outbound Stripe adapter when
                    // STRIPE_SECRET_KEY is configured, otherwise the fail-closed stand-in (placeOrder
                    // stays wired end-to-end but declines every checkout).
                    payments: infrastructure::generated::service_bindings::payment_service(|| {
                        match std::env::var("STRIPE_SECRET_KEY") {
                            Ok(key) if !key.is_empty() => {
                                println!("payment service: StripePaymentGateway (STRIPE_SECRET_KEY set)");
                                Arc::new(stripe_adapter::StripePaymentGateway::new(key))
                            }
                            _ => {
                                println!(
                                    "payment service: FailClosedPaymentGateway (STRIPE_SECRET_KEY unset — every checkout declines)"
                                );
                                Arc::new(FailClosedPaymentGateway)
                            }
                        }
                    })
                    .expect("payment service binding (services.yaml)"),
                    // The payment_process_manager state rows placeOrder opens/single-flights on
                    // (ADR-20260719-193500).
                    pm_state: Arc::new(infrastructure::persistence::PgPaymentProcessState::new(
                        pool.clone(),
                    )),
                    // The refund_process_manager rows the approveRefund/denyRefund decisions run on.
                    refund_state: Arc::new(infrastructure::persistence::PgRefundProcessState::new(
                        pool.clone(),
                    )),
                    // Acceptance-first dispatch (ADR-20260720-015300/-015500): the durable command
                    // journal + the journal-transition broadcast behind operationStatus(+Changed).
                    journal: Arc::new(infrastructure::PgCommandJournal::new(pool.clone())),
                    status_bus: operation_status_bus.clone(),
                });

                // In-process projection worker (ADR-0040). RUN_PROJECTOR=false hands it to a dedicated worker.
                if std::env::var("RUN_PROJECTOR").map(|v| v != "false").unwrap_or(true) {
                    let worker = ProjectionWorker::new(pool.clone());
                    projector_status = Some(worker.status());
                    tokio::spawn(worker.run_loop());
                    println!("projection worker: running in-process (set RUN_PROJECTOR=false to disable)");
                } else {
                    println!("RUN_PROJECTOR=false — projection worker not started in-process");
                }

                // In-process saga runner (the state-table process managers of
                // specs/processmanager.yaml, ADR-20260719-193500) — same pattern as the projection
                // worker: RUN_PROCESS_MANAGERS=false hands it to a dedicated worker. The runner
                // builds its state-table stores and read models over the pool; the `delivery`
                // service resolves through the GENERATED topology binding (services.yaml, issue #26):
                // the composition root supplies the in-process constructor — the real outbound
                // Avelo37 gateway when AVELO37_API_KEY is configured (issue #28), otherwise the
                // logged no-op stand-in (jobs stay open to independent riders; the bounded re-offer
                // run row still terminates ACCEPTED/FAILED). The partner's answers always arrive
                // asynchronously through the webhook inbox below, never this outbound call.
                if std::env::var("RUN_PROCESS_MANAGERS").map(|v| v != "false").unwrap_or(true) {
                    // Composite delivery gateway (#60): the saga offers a job on a strategy-resolved
                    // CHANNEL, so the single Avelo-vs-Noop choice becomes a registry of channel →
                    // adapter. `independent` is the rider POOL (a deliberate no-op — jobs stay open to
                    // riders); `avelo37` is wired when AVELO37_API_KEY is set, and `coopcycle` when its
                    // federation registry (COOPCYCLE_INSTANCES) is configured (issue #58). Unwired
                    // channels (e.g. uber_direct in an unconfigured Tours) fall through: the offer times
                    // out and the saga escalates to the next ranked channel (today's deployments unchanged).
                    let partner = infrastructure::generated::service_bindings::delivery_service(|| {
                        let mut gateway = infrastructure::CompositeDeliveryGateway::new().with_channel(
                            "independent",
                            Arc::new(application::ports::NoopDeliveryService),
                        );
                        if let Some(avelo) = avelo37_adapter::Avelo37DeliveryGateway::from_env() {
                            gateway = gateway.with_channel("avelo37", Arc::new(avelo));
                        }
                        if !coopcycle_registry.is_empty() {
                            gateway = gateway.with_channel(
                                "coopcycle",
                                Arc::new(coopcycle_adapter::CoopCycleDeliveryGateway::new(
                                    coopcycle_registry.clone(),
                                )),
                            );
                        }
                        if let Some(config) = uber_direct_config.clone() {
                            gateway = gateway.with_channel(
                                "uber_direct",
                                Arc::new(uber_direct_adapter::UberDirectDeliveryGateway::new(config)),
                            );
                        }
                        println!(
                            "delivery gateway: composite — wired channels {:?} (unwired channels fall through via offer timeout)",
                            gateway.wired_channels()
                        );
                        Arc::new(gateway)
                    })
                    .expect("delivery service binding (services.yaml)");
                    let runner = ProcessManagerRunner::new(pool.clone()).with_partner(partner);
                    saga_status = Some(runner.status());
                    tokio::spawn(runner.run_loop());
                    println!("saga runner: running in-process (set RUN_PROCESS_MANAGERS=false to disable)");

                    // Delivery offer-timeout worker (#60): escalates a stale OFFERED run to the next
                    // ranked channel. Env-gated like the other in-process workers.
                    if std::env::var("RUN_DELIVERY_OFFER_TIMEOUT").map(|v| v != "false").unwrap_or(true) {
                        let timeout_worker =
                            Arc::new(infrastructure::DeliveryOfferTimeoutWorker::new(pool.clone()));
                        tokio::spawn(timeout_worker.run_loop());
                        println!("delivery offer-timeout worker: running in-process (set RUN_DELIVERY_OFFER_TIMEOUT=false to disable)");
                    } else {
                        println!("RUN_DELIVERY_OFFER_TIMEOUT=false — delivery offer-timeout worker not started");
                    }
                } else {
                    println!("RUN_PROCESS_MANAGERS=false — saga runner not started in-process");
                }

                // SIRENE sync worker (ADR-0045): drains the `external_sirene_restaurants` staging
                // table through the ACL into the ordinary write path. Always constructed (the
                // /internal/sirene/drain ping needs it); the slow safety-net poll loop is gated by
                // RUN_SIRENE_WORKER (default on) like the projector.
                // Inbound-events drain worker (ADR-20260720-015400): delivers adapter-staged
                // business events through the normal write path, and runs the command_journal
                // stale-RECEIVED sweep (ADR-20260720-015300). Always constructed (the webhook nudge
                // + /internal/inbound/drain need it); the safety-net poll loop is gated by
                // RUN_INBOUND_DRAIN (default on) like the projector.
                let drain = Arc::new(infrastructure::InboundEventsDrainWorker::new(
                    Arc::new(infrastructure::PgInboundEvents::new(pool.clone())),
                    Arc::new(infrastructure::PgCommandJournal::new(pool.clone())),
                    Arc::new(PgEventStore::with_bus(pool.clone(), event_bus.clone())),
                ));
                inbound_drain = Some(drain.clone());
                if std::env::var("RUN_INBOUND_DRAIN").map(|v| v != "false").unwrap_or(true) {
                    tokio::spawn(drain.clone().run_loop());
                    println!("inbound drain worker: running in-process (set RUN_INBOUND_DRAIN=false to disable)");
                } else {
                    println!("RUN_INBOUND_DRAIN=false — inbound drain poll loop not started (nudge trigger stays active)");
                }

                // Stripe webhook ingestor (ADR-20260720-015400 inbound event sourcing): verify →
                // mirror the verbatim delivery into external_stripe_events → ACL → stage the adapted
                // business event in inbound_events → ACK, nudging the drain worker. The HTTP endpoint
                // (`POST /adapters/stripe/webhooks`) is mounted below with the other non-GraphQL routes.
                let nudge_worker = drain.clone();
                stripe_ingestor = Some(Arc::new(
                    StripeWebhookIngestor::new(
                        Arc::new(stripe_adapter::PgRawStripeEvents::new(pool.clone())),
                        Arc::new(infrastructure::PgInboundEvents::new(pool.clone())),
                    )
                    .with_nudge(Arc::new(move || {
                        let w = nudge_worker.clone();
                        tokio::spawn(async move { w.run_once().await });
                    })),
                ));

                // Avelo37 delivery-partner webhook ingestor (issue #28, same two-layer inbox as
                // Stripe): verify → mirror the verbatim delivery into external_avelo37_events → ACL
                // → stage the adapted DeliveryAcceptedByPartner/RejectedByPartner/StatusUpdated fact
                // in inbound_events → ACK, nudging the drain worker (which now routes delivery facts
                // onto the DeliveryJob stream). Mounted at `POST /adapters/avelo37/webhooks` below.
                let nudge_worker = drain.clone();
                avelo37_ingestor = Some(Arc::new(
                    Avelo37WebhookIngestor::new(
                        Arc::new(avelo37_adapter::PgRawAvelo37Events::new(pool.clone())),
                        Arc::new(infrastructure::PgInboundEvents::new(pool.clone())),
                    )
                    .with_nudge(Arc::new(move || {
                        let w = nudge_worker.clone();
                        tokio::spawn(async move { w.run_once().await });
                    })),
                ));

                // CoopCycle delivery-partner webhook ingestor (issue #58, same two-layer inbox): the
                // federation twist is that the verified webhook arrives per-instance at
                // `POST /adapters/coopcycle/{instance}/webhooks` and is namespaced by instance; the
                // ingestor itself is provider-shaped like Avelo37's (mirror → ACL → inbound_events →
                // drain routes onto the DeliveryJob stream). Mounted below with the registry (secrets).
                let nudge_worker = drain.clone();
                coopcycle_ingestor = Some(Arc::new(
                    CoopCycleWebhookIngestor::new(
                        Arc::new(coopcycle_adapter::PgRawCoopCycleEvents::new(pool.clone())),
                        Arc::new(infrastructure::PgInboundEvents::new(pool.clone())),
                    )
                    .with_nudge(Arc::new(move || {
                        let w = nudge_worker.clone();
                        tokio::spawn(async move { w.run_once().await });
                    })),
                ));

                // Uber Direct delivery-partner webhook ingestor (issue #57, same two-layer inbox as
                // Avelo37/CoopCycle): verify the X-Uber-Signature → mirror the verbatim delivery into
                // external_uber_direct_events → ACL → stage the adapted DeliveryAcceptedByPartner/
                // RejectedByPartner/StatusUpdated fact in inbound_events → ACK, nudging the drain
                // worker. Mounted at `POST /adapters/uber-direct/webhooks` below with the signing secret.
                let nudge_worker = drain.clone();
                uber_direct_ingestor = Some(Arc::new(
                    UberDirectWebhookIngestor::new(
                        Arc::new(uber_direct_adapter::PgRawUberDirectEvents::new(pool.clone())),
                        Arc::new(infrastructure::PgInboundEvents::new(pool.clone())),
                    )
                    .with_nudge(Arc::new(move || {
                        let w = nudge_worker.clone();
                        tokio::spawn(async move { w.run_once().await });
                    })),
                ));

                // HubRise wiring (issue #20): the raw mirror (external_hubrise_callbacks), the
                // enrichment, AND the connect flow all need only the database — the pull token is
                // resolved per connected account from `hubrise_connections` (the global
                // `HUBRISE_ACCESS_TOKEN` fallback is retired). The connect routes additionally
                // require the app credentials (HUBRISE_CLIENT_ID + HUBRISE_WEBHOOK_SECRET +
                // HUBRISE_CONNECT_REDIRECT_URL), checked per request fail-closed.
                hubrise_state.raw =
                    Some(Arc::new(hubrise_adapter::PgRawHubRiseCallbacks::new(pool.clone())));
                {
                    let hubrise_store =
                        Arc::new(PgEventStore::with_bus(pool.clone(), event_bus.clone()));
                    let hubrise_journal = Arc::new(infrastructure::PgCommandJournal::new(pool.clone()));
                    let hubrise_connections =
                        Arc::new(hubrise_adapter::PgHubRiseConnections::new(pool.clone()));
                    // Enricher/connect sends journal on the WORKER channel (ADR-20260720-015300, #15):
                    // callback redeliveries dedupe on command_journal instead of double-applying.
                    hubrise_state.enricher = Some(Arc::new(hubrise_adapter::HubRiseEnricher::new(
                        hubrise_store.clone(),
                        hubrise_journal.clone(),
                        hubrise_connections.clone(),
                        hubrise_adapter::api::HubRiseApi::from_env(),
                    )));
                    hubrise_state.connect = Some(Arc::new(hubrise_adapter::HubRiseConnectFlow::new(
                        hubrise_store,
                        hubrise_journal,
                        hubrise_restaurants,
                        hubrise_connections,
                        hubrise_adapter::connect::HttpHubRiseConnectGateway {
                            api: hubrise_adapter::api::HubRiseApi::from_env(),
                            client_id: std::env::var("HUBRISE_CLIENT_ID").unwrap_or_default(),
                            client_secret: std::env::var("HUBRISE_WEBHOOK_SECRET").unwrap_or_default(),
                        },
                    )));
                }

                // Retention sweep worker (ADR-20260721-025159): periodically calls the
                // sweep_retention() SQL function — journal/mirror retention windows live in the
                // function, never here. Env-gated like the other workers; a pg_cron job calling
                // the same function is the alternative where DB-side scheduling is preferred.
                if std::env::var("RUN_RETENTION_SWEEP").map(|v| v != "false").unwrap_or(true) {
                    let sweeper =
                        Arc::new(infrastructure::RetentionSweepWorker::new(pool.clone()));
                    tokio::spawn(sweeper.run_loop());
                    println!("retention sweep worker: running in-process (set RUN_RETENTION_SWEEP=false to disable)");
                } else {
                    println!("RUN_RETENTION_SWEEP=false — retention sweep worker not started");
                }

                let worker = Arc::new(SireneSyncWorker::new(pool.clone()));
                sirene_worker = Some(worker.clone());
                if std::env::var("RUN_SIRENE_WORKER").map(|v| v != "false").unwrap_or(true) {
                    tokio::spawn(worker.run_loop());
                    println!(
                        "sirene sync worker: running in-process (set RUN_SIRENE_WORKER=false to keep only the ping trigger)"
                    );
                } else {
                    println!("RUN_SIRENE_WORKER=false — sirene sync worker poll loop not started (ping trigger stays active)");
                }
            }
            Err(e) => eprintln!("DATABASE_URL set but pool init failed: {e}"),
        },
        _ => eprintln!("DATABASE_URL not set — /health will report not_configured (503)"),
    }

    let base = Router::new()
        .route("/ping", get(ping))
        .route("/health", get(health))
        .route("/projector", get(projector))
        .route("/saga", get(saga))
        .with_state(AppState { snap, projector_status, saga_status });

    base.merge(graphql::routes::graphql_routes(graphql::schema::build_schema(
        read_deps,
        write_deps,
        Some(event_bus),
    )))
        // Internal trigger (ADR-0045): the CI ingestion pings this to wake the SIRENE sync worker.
        .merge(graphql::routes::sirene_internal_routes(sirene_worker))
        // Internal trigger (ADR-20260720-015400): ops ping to wake the inbound-events drain worker.
        .merge(graphql::routes::inbound_internal_routes(inbound_drain))
        // Partner webhook adapters (ADR-20260718-213352): self-contained crates under crates/adapters/*,
        // each mountable here (monolith) or deployable as its own web service. `POST /adapters/stripe/webhooks`
        // (signature-verified inbound payment facts), `POST /adapters/avelo37/webhooks` (signature-verified
        // inbound delivery-partner facts, issue #28) and `POST /adapters/hubrise/webhooks` (HMAC-verified ingress).
        .merge(stripe_adapter::routes(stripe_ingestor))
        .merge(avelo37_adapter::routes(avelo37_ingestor))
        // CoopCycle per-instance webhooks (issue #58): `POST /adapters/coopcycle/{instance}/webhooks`,
        // verified with the instance's registry secret. State carries the ingestor + the registry.
        .merge(coopcycle_adapter::routes(coopcycle_adapter::CoopCycleWebhookState {
            ingestor: coopcycle_ingestor,
            registry: Arc::new(coopcycle_registry),
        }))
        // Uber Direct webhooks (issue #57): `POST /adapters/uber-direct/webhooks`, verified with the
        // X-Uber-Signature raw-body HMAC. State carries the ingestor + the signing secret.
        .merge(uber_direct_adapter::routes(uber_direct_adapter::UberDirectWebhookState {
            ingestor: uber_direct_ingestor,
            webhook_secret: uber_direct_config.map(|c| Arc::new(c.webhook_secret)),
        }))
        .merge(hubrise_adapter::routes(hubrise_state))
        // The DERIVED `/services/<service>/<op>` surface (issue #26): emitted per the spec's
        // `expose` flags — empty while every service declares `expose: false` (V0).
        .merge(generated::services_routes::services_router())
        // Host-based landing (ADR-0036): any path not matched above is dispatched by the request `Host`
        // to its per-audience/tenant placeholder. Explicit routes (/health, /ping, /{role}/graphql) win,
        // so Render's health check (internal *.onrender.com host) is unaffected. Covers `/` too.
        .fallback(hosts::host_root)
        // API auth (ADR-0047): the Supabase-JWT verifier, available to the `/{role}/graphql` handler which
        // gates every non-public path. Shared as an Extension so the JWKS cache is process-wide.
        .layer(Extension(auth::AuthContext::from_env()))
        // Outer layer: stamp every response with its server-side build time.
        .layer(middleware::from_fn(response_timing))
}

/// Stamp every response with how long the server took to build it: `x-response-time-ms` (milliseconds) and
/// the standard `Server-Timing` header (shown in browser devtools), plus `X-VERSION` — the running build's
/// identity (`build_version()`, the short git SHA) on **every** response, so any HTTP client can read which
/// deploy served it without hitting `/health` (ADR-20260721-175411). Applied as an outer layer over all routes.
async fn response_timing(req: Request, next: Next) -> Response {
    let start = std::time::Instant::now();
    let mut resp = next.run(req).await;
    let ms = format!("{:.2}", start.elapsed().as_secs_f64() * 1000.0);
    let headers = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&ms) {
        headers.insert("x-response-time-ms", v);
    }
    if let Ok(v) = HeaderValue::from_str(&format!("app;dur={ms}")) {
        headers.insert("server-timing", v);
    }
    if let Ok(v) = HeaderValue::from_str(build_version()) {
        headers.insert("x-version", v);
    }
    resp
}

/// Liveness: the process is up. No dependencies (does not touch the DB).
async fn ping() -> &'static str {
    "pong"
}

/// Projection-worker readiness. `200` when the worker is running, `503` otherwise (not started / not
/// caught up is still `200` with `lag > 0` — inspect the body). Reports checkpoint/head/lag/lastTickAt.
async fn projector(State(app): State<AppState>) -> impl IntoResponse {
    match &app.projector_status {
        Some(handle) => {
            let status = handle.lock().expect("projector status mutex").clone();
            let code = if status.running { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
            let body = serde_json::to_value(&status).unwrap_or_else(|_| json!({ "running": false }));
            (code, Json(body))
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "running": false, "reason": "projector_not_started" })),
        ),
    }
}

/// Saga-runner readiness (the `/projector` counterpart for the process managers). `200` when the
/// runner is running, `503` otherwise. Reports checkpoint/head/lag/lastTickAt.
async fn saga(State(app): State<AppState>) -> impl IntoResponse {
    match &app.saga_status {
        Some(handle) => {
            let status = handle.lock().expect("saga status mutex").clone();
            let code = if status.running { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
            let body = serde_json::to_value(&status).unwrap_or_else(|_| json!({ "running": false }));
            (code, Json(body))
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "running": false, "reason": "saga_runner_not_started" })),
        ),
    }
}

/// Every 30s, recompute readiness and cache it. The first run happens immediately.
fn spawn_heartbeat(pool: PgPool, snap: Arc<Mutex<Snapshot>>) {
    tokio::spawn(async move {
        loop {
            let next = probe(&pool).await;
            *snap.lock().expect("health snapshot mutex") = next;
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}

/// Read `max(version)` from `_sqlx_migrations` (successful rows only) and compare to the required version.
/// Simple query protocol (`raw_sql`, no prepared statement) → safe on any Supabase pooler mode (ADR-0043).
async fn probe(pool: &PgPool) -> Snapshot {
    match sqlx::raw_sql("SELECT max(version) AS v FROM _sqlx_migrations WHERE success")
        .fetch_one(pool)
        .await
    {
        Ok(row) => {
            let applied = row.try_get::<Option<i64>, _>("v").ok().flatten().unwrap_or(-1);
            let state = if applied >= REQUIRED_SCHEMA_VERSION {
                db_state::HEALTHY
            } else {
                db_state::SCHEMA_BEHIND
            };
            Snapshot { state, applied_version: applied }
        }
        Err(_) => Snapshot { state: db_state::DOWN, applied_version: -1 },
    }
}

/// Readiness endpoint (point Render's Health Check Path here). `200` only when reachable and the schema is
/// at/after the required version; otherwise `503` with a machine-readable reason.
async fn health(State(app): State<AppState>) -> impl IntoResponse {
    let snap = app.snap.lock().expect("health snapshot mutex").clone();
    // `version` is included in EVERY branch (esp. degraded/down) so a failing instance always names its build.
    match snap.state {
        db_state::HEALTHY => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "db": "up", "version": build_version(), "schemaVersion": snap.applied_version, "requiredSchemaVersion": REQUIRED_SCHEMA_VERSION })),
        ),
        db_state::SCHEMA_BEHIND => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "degraded", "db": "up", "reason": "schema_behind", "version": build_version(), "schemaVersion": snap.applied_version, "requiredSchemaVersion": REQUIRED_SCHEMA_VERSION })),
        ),
        db_state::NOT_CONFIGURED => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "degraded", "db": "not_configured", "version": build_version() })),
        ),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "degraded", "db": "down", "version": build_version() })),
        ),
    }
}
