//! Captain.Food server (Axum BFF) — the composition root (ADR-0035).
//!
//! DI happens here: infrastructure adapters are injected behind application ports, then the HTTP/GraphQL
//! surface is built over them. Exposed as a library so `desktop` (Tauri) can embed it in-process.
//!
//! Endpoints:
//! - `/ping` → `pong` — liveness (process is up; touches nothing). Used by uptime pingers / keep-warm.
//! - `/health` — readiness gate (ADR-0043): `200` only when the DB is reachable AND its schema version is
//!   `>= REQUIRED_SCHEMA_VERSION`; else `503`. Migrations are applied out-of-band by **sqlx-cli in CI**.
//! - `/projector` — projection-worker readiness (running / checkpoint / head / lag / lastTickAt).
//! - `/saga` — process-manager (saga) runner readiness, same shape as `/projector`.
//! - `/{role}/graphql` (+ `/{role}/voyager`) — the GraphQL BFF (ADR-0006), see `graphql`.
//! - `POST /internal/sirene/drain` — wakes the SIRENE sync worker after a CI ingestion run (ADR-0045);
//!   secured by the `INTERNAL_TRIGGER_TOKEN` shared secret (`x-internal-token` header).
//! - `POST /adapters/stripe/webhooks` — Stripe webhook ingestion (inbound payment facts through the ACL);
//!   secured by `Stripe-Signature` HMAC verification against `STRIPE_WEBHOOK_SECRET` (fail-closed).
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
    CartReadRepository, CatalogReadRepository, CustomerReadRepository, DeliveryReadRepository,
    OrderReadRepository, PricingPolicyReadRepository, ProspectionReadRepository,
    RefundReadRepository, RestaurantReadRepository, UberEstimationPolicyReadRepository,
    UberSplitPolicyReadRepository,
};
use infrastructure::{
    EventBus, FailClosedAuthProviderGateway, FailClosedGoogleOwnershipVerifier, FailClosedPaymentGateway,
    PgCartRepository, PgCatalogRepository, PgCustomerRepository, PgDeliveryRepository, PgEventStore,
    PgOrderRepository, PgPricingPolicyRepository, PgProspectionRepository, PgRefundQueueRepository,
    PgRestaurantRepository, PgUberEstimationPolicyRepository, PgUberSplitPolicyRepository, ProcessManagerRunner,
    ProcessManagerStatus, ProjectionStatus, ProjectionWorker, SireneSyncWorker,
    UnverifiedGbpOrderLinkProbe,
};
use stripe_adapter::StripeWebhookIngestor;
use shared_types::HealthDto;

use graphql::schema::{ReadDeps, WriteDeps};

mod auth;
mod graphql;
mod hosts;

/// The role-as-path ACL seam (RequestRole/RoleGuard, ADR-0006), re-exported so integration tests can
/// execute the schema under a specific role (the HTTP layer injects it from the URL path).
pub use graphql::acl as graphql_acl;
pub use graphql::session as graphql_session;
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
pub const REQUIRED_SCHEMA_VERSION: i64 = 20260720030000;

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
                });

                // Write side (CQRS commands): the event store behind the mutation resolvers, plus the
                // Google and Supabase Auth seam adapters (fail-closed stand-ins until the real
                // integrations land).
                write_deps = Some(WriteDeps {
                    event_store: Arc::new(PgEventStore::with_bus(pool.clone(), event_bus.clone())),
                    ownership: Arc::new(FailClosedGoogleOwnershipVerifier),
                    gbp_probe: Arc::new(UnverifiedGbpOrderLinkProbe),
                    auth_provider: Arc::new(FailClosedAuthProviderGateway),
                    // Real outbound Stripe gateway when STRIPE_SECRET_KEY is configured; otherwise the
                    // fail-closed stand-in (placeOrder stays wired end-to-end but declines every checkout).
                    payments: match std::env::var("STRIPE_SECRET_KEY") {
                        Ok(key) if !key.is_empty() => {
                            println!("payment gateway: StripePaymentGateway (STRIPE_SECRET_KEY set)");
                            Arc::new(stripe_adapter::StripePaymentGateway::new(key))
                        }
                        _ => {
                            println!(
                                "payment gateway: FailClosedPaymentGateway (STRIPE_SECRET_KEY unset — every checkout declines)"
                            );
                            Arc::new(FailClosedPaymentGateway)
                        }
                    },
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
                // builds its state-table stores and read models over the pool; the delivery-partner
                // port stays the no-op stand-in until the avelo37 ACL lands (`with_partner`).
                if std::env::var("RUN_PROCESS_MANAGERS").map(|v| v != "false").unwrap_or(true) {
                    let runner = ProcessManagerRunner::new(pool.clone());
                    saga_status = Some(runner.status());
                    tokio::spawn(runner.run_loop());
                    println!("saga runner: running in-process (set RUN_PROCESS_MANAGERS=false to disable)");
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

                // HubRise webhook wiring: the raw mirror (external_hubrise_callbacks) needs only the
                // database; the domain enrichment (ADR-20260718-145856: OAuth API pull → ACL map →
                // `ImportCatalog` / per-SKU stock update) additionally needs `HUBRISE_ACCESS_TOKEN` —
                // otherwise the endpoint stays mirror+verify only (callbacks ACK as pending).
                hubrise_state.raw =
                    Some(Arc::new(hubrise_adapter::PgRawHubRiseCallbacks::new(pool.clone())));
                match hubrise_adapter::api::HubRiseApiClient::from_env() {
                    Ok(api) => {
                        hubrise_state.enricher = Some(Arc::new(hubrise_adapter::HubRiseEnricher::new(
                            Arc::new(PgEventStore::with_bus(pool.clone(), event_bus.clone())),
                            api,
                        )));
                    }
                    Err(_) => eprintln!(
                        "HUBRISE_ACCESS_TOKEN unset — /adapters/hubrise/webhooks verifies callbacks but does not enrich"
                    ),
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
        // (signature-verified inbound payment facts) and `POST /adapters/hubrise/webhooks` (HMAC-verified ingress).
        .merge(stripe_adapter::routes(stripe_ingestor))
        .merge(hubrise_adapter::routes(hubrise_state))
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
/// the standard `Server-Timing` header (shown in browser devtools). Applied as an outer layer over all routes.
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
    match snap.state {
        db_state::HEALTHY => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "db": "up", "schemaVersion": snap.applied_version, "requiredSchemaVersion": REQUIRED_SCHEMA_VERSION })),
        ),
        db_state::SCHEMA_BEHIND => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "degraded", "db": "up", "reason": "schema_behind", "schemaVersion": snap.applied_version, "requiredSchemaVersion": REQUIRED_SCHEMA_VERSION })),
        ),
        db_state::NOT_CONFIGURED => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "degraded", "db": "not_configured" })),
        ),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "degraded", "db": "down" })),
        ),
    }
}
