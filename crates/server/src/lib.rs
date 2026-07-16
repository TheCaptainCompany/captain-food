//! Captain.Food server (Axum BFF) — the composition root (ADR-0035).
//!
//! This is where dependency injection happens: concrete `infrastructure` adapters are constructed and
//! injected behind the `application` ports, then the HTTP/GraphQL/SDUI surface is built over them. Exposed
//! as a library so `desktop` (Tauri) can embed the same server in-process. Referencing `application`,
//! `infrastructure` and `shared_types` proves the server → those-three edges.
//!
//! `/health` is a strict readiness gate (ADR-0043). This build **embeds** its migration set (`MIGRATOR`);
//! readiness means every embedded migration is present-and-successful in sqlx's `_sqlx_migrations` ledger.
//! A 30-second heartbeat computes this and caches it. If any are missing, `/health` returns `503` and
//! **names them** (`version_description`, e.g. `0005_view_order_v5`) so a mis-migrated deploy is diagnosable
//! at a glance. Because the app only requires *its own* embedded set, an older build still passes against a
//! newer DB — preserving rollback-by-redeploy under expand/contract (ADR-0043).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde_json::json;
use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use application::ports::RestaurantRepository;
use infrastructure::PgRestaurantRepository;
use shared_types::HealthDto;

/// The migration set this build depends on, embedded at compile time from the repo-root `migrations/`
/// (ADR-0043). Shared with the `migrate` bin so there is a single source of truth for what must be applied.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// Readiness states published by the heartbeat, read by `/health`.
mod db_state {
    pub const NOT_CONFIGURED: u8 = 0; // DATABASE_URL unset
    pub const DOWN: u8 = 1; // unreachable, or `_sqlx_migrations` does not exist yet
    pub const SCHEMA_BEHIND: u8 = 2; // reachable, but some embedded migration is not applied
    pub const HEALTHY: u8 = 3; // reachable and every embedded migration is applied
}

/// A cached readiness snapshot. Cheap to clone; refreshed every 30s by the heartbeat.
#[derive(Clone)]
struct Snapshot {
    state: u8,
    /// Highest successfully-applied version in the DB (`-1` if none/unknown).
    applied_version: i64,
    /// Highest version this build requires.
    required_version: i64,
    /// Embedded migrations not yet applied, named `version_description`.
    missing: Vec<String>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self { state: db_state::NOT_CONFIGURED, applied_version: -1, required_version: required_version(), missing: Vec::new() }
    }
}

#[derive(Clone)]
pub struct AppState {
    snap: Arc<Mutex<Snapshot>>,
}

/// Build the application wiring (skeleton). Constructs a port impl behind its trait, proving the
/// server → application/infrastructure edges (ADR-0035). The real version returns the fully injected graph.
pub fn wire() -> HealthDto {
    let _restaurants: Box<dyn RestaurantRepository> = Box::new(PgRestaurantRepository);
    HealthDto::ok()
}

/// Build the Axum router. Reads `DATABASE_URL`; when present it opens a *lazy* pool (never blocks or fails
/// at boot) and spawns the 30-second readiness heartbeat that feeds `/health`.
pub fn router() -> Router {
    let _ = wire();

    let snap = Arc::new(Mutex::new(Snapshot::default()));

    match std::env::var("DATABASE_URL") {
        Ok(url) if !url.is_empty() => match PgPoolOptions::new().max_connections(5).connect_lazy(&url) {
            Ok(pool) => {
                // Configured but unconfirmed until the first probe: report DOWN, not NOT_CONFIGURED.
                snap.lock().expect("health snapshot mutex").state = db_state::DOWN;
                spawn_heartbeat(pool, snap.clone());
            }
            Err(e) => eprintln!("DATABASE_URL set but pool init failed: {e}"),
        },
        _ => eprintln!("DATABASE_URL not set — /health will report not_configured (503)"),
    }

    Router::new()
        .route("/health", get(health))
        .with_state(AppState { snap })
}

/// Highest version among the embedded migrations.
fn required_version() -> i64 {
    MIGRATOR.iter().map(|m| m.version).max().unwrap_or(0)
}

/// Every 30 seconds, recompute readiness and publish it. The first run happens immediately (before the
/// initial sleep) so `/health` reflects reality within a moment of boot.
fn spawn_heartbeat(pool: PgPool, snap: Arc<Mutex<Snapshot>>) {
    tokio::spawn(async move {
        loop {
            let next = probe(&pool).await;
            *snap.lock().expect("health snapshot mutex") = next;
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}

/// Compare the embedded migration set against `_sqlx_migrations` (successful rows only). A missing ledger
/// table (migrator not yet run) surfaces as a query error → `DOWN`.
async fn probe(pool: &PgPool) -> Snapshot {
    let required_version = required_version();
    match sqlx::query_scalar::<_, i64>("SELECT version FROM _sqlx_migrations WHERE success")
        .fetch_all(pool)
        .await
    {
        Ok(applied) => {
            let applied_set: HashSet<i64> = applied.iter().copied().collect();
            let applied_version = applied.iter().copied().max().unwrap_or(-1);
            let missing: Vec<String> = MIGRATOR
                .iter()
                .filter(|m| !applied_set.contains(&m.version))
                .map(|m| format!("{:04}_{}", m.version, m.description))
                .collect();
            let state = if missing.is_empty() { db_state::HEALTHY } else { db_state::SCHEMA_BEHIND };
            Snapshot { state, applied_version, required_version, missing }
        }
        Err(_) => Snapshot { state: db_state::DOWN, applied_version: -1, required_version, missing: Vec::new() },
    }
}

/// Strict readiness endpoint. `200` only when the DB is reachable and every embedded migration is applied;
/// otherwise `503` with a machine-readable reason (and, when behind, the names of the missing migrations).
async fn health(State(app): State<AppState>) -> impl IntoResponse {
    let snap = app.snap.lock().expect("health snapshot mutex").clone();
    match snap.state {
        db_state::HEALTHY => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "db": "up", "schemaVersion": snap.applied_version })),
        ),
        db_state::SCHEMA_BEHIND => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "degraded",
                "db": "up",
                "reason": "schema_behind",
                "schemaVersion": snap.applied_version,
                "requiredSchemaVersion": snap.required_version,
                "missing": snap.missing,
            })),
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
