//! Captain.Food server (Axum BFF) — the composition root (ADR-0035).
//!
//! DI happens here: infrastructure adapters are injected behind application ports, then the HTTP/GraphQL
//! surface is built over them. Exposed as a library so `desktop` (Tauri) can embed it in-process.
//!
//! Endpoints:
//! - `/ping` → `pong` — liveness (process is up; touches nothing). Used by uptime pingers / keep-warm.
//! - `/health` — readiness gate (ADR-0043): `200` only when the DB is reachable AND its schema version is
//!   `>= REQUIRED_SCHEMA_VERSION`; else `503`. Migrations are applied out-of-band by **sqlx-cli in CI** —
//!   the app never applies them, it only checks the version (so an older build still runs against a newer
//!   DB, preserving rollback-by-redeploy under expand/contract).
//! - `/{role}/graphql` — the GraphQL BFF (ADR-0006), see `graphql`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    extract::{Request, State},
    http::{HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

use application::ports::RestaurantRepository;
use infrastructure::PgRestaurantRepository;
use shared_types::HealthDto;

mod graphql;

/// The schema version this build requires. Migrations are applied by **sqlx-cli in CI** (ADR-0043); the app
/// only checks the DB has reached at least this version. Bump when adding a migration this build depends on.
/// The gate is `>=` (never `==`) so an older build still runs against a newer DB (rollback-by-redeploy).
pub const REQUIRED_SCHEMA_VERSION: i64 = 20260717120000;

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
}

/// Build the application wiring (skeleton). Constructs a port impl behind its trait, proving the
/// server → application/infrastructure edges (ADR-0035).
pub fn wire() -> HealthDto {
    let _restaurants: Box<dyn RestaurantRepository> = Box::new(PgRestaurantRepository);
    HealthDto::ok()
}

/// Build the Axum router: `/ping`, `/health`, and the role-as-path GraphQL routes.
pub fn router() -> Router {
    let _ = wire();

    let snap = Arc::new(Mutex::new(Snapshot::default()));

    match std::env::var("DATABASE_URL") {
        Ok(url) if !url.is_empty() => match PgPoolOptions::new()
            .max_connections(5)
            .min_connections(1) // keep one connection warm so the 30s heartbeat reuses it
            .acquire_timeout(Duration::from_secs(10))
            .idle_timeout(Duration::from_secs(240)) // recycle cleanly before the pooler drops idle conns
            .max_lifetime(Duration::from_secs(1800))
            .connect_lazy(&url)
        {
            Ok(pool) => {
                // Configured but unconfirmed until the first probe: report DOWN, not NOT_CONFIGURED.
                snap.lock().expect("health snapshot mutex").state = db_state::DOWN;
                spawn_heartbeat(pool, snap.clone());
            }
            Err(e) => eprintln!("DATABASE_URL set but pool init failed: {e}"),
        },
        _ => eprintln!("DATABASE_URL not set — /health will report not_configured (503)"),
    }

    let health_router = Router::new()
        .route("/ping", get(ping))
        .route("/health", get(health))
        .with_state(AppState { snap });

    // GraphQL BFF (ADR-0006). Scaffold schema needs no DB, so it mounts regardless of DATABASE_URL.
    health_router
        .merge(graphql::routes::graphql_routes(graphql::schema::build_schema()))
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

/// Liveness: the process is up. No dependencies (does not touch the DB) — for uptime pingers and to keep
/// the free-tier instance warm. Distinct from `/health`, which gates on DB + schema version.
async fn ping() -> &'static str {
    "pong"
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
/// A missing ledger table (migrations never applied) surfaces as a query error → `DOWN`.
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
