//! Captain.Food server (Axum BFF) — the composition root (ADR-0035).
//!
//! This is where dependency injection happens: concrete `infrastructure` adapters are constructed and
//! injected behind the `application` ports, then the HTTP/GraphQL/SDUI surface is built over them. Exposed
//! as a library so `desktop` (Tauri) can embed the same server in-process. Referencing `application`,
//! `infrastructure` and `shared_types` proves the server → those-three edges.
//!
//! Today the surface is just `/health`, which reflects database connectivity via a 30-second `SELECT 1`
//! heartbeat (deploy target, ADR-0042). GraphQL (role-as-path, ADR-0006) and tenant/auth middleware land
//! on top of this same router later.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use application::ports::RestaurantRepository;
use infrastructure::PgRestaurantRepository;
use shared_types::HealthDto;

/// Shared handler state. `Clone` is cheap: `PgPool` is an `Arc` internally and `db_up` is an `Arc`.
#[derive(Clone)]
pub struct AppState {
    /// `None` when `DATABASE_URL` is unset (e.g. the very first boot before Supabase is wired).
    db: Option<PgPool>,
    /// Latest result of the 30-second `SELECT 1` heartbeat.
    db_up: Arc<AtomicBool>,
}

/// Build the application wiring (skeleton). Constructs a port impl behind its trait, proving the
/// server → application/infrastructure edges (ADR-0035). The real version returns the fully injected graph.
pub fn wire() -> HealthDto {
    let _restaurants: Box<dyn RestaurantRepository> = Box::new(PgRestaurantRepository);
    HealthDto::ok()
}

/// Build the Axum router. Reads `DATABASE_URL`; when present it opens a *lazy* pool (never blocks or fails
/// at boot — it connects on first use) and spawns the 30-second `SELECT 1` heartbeat that feeds `/health`.
pub fn router() -> Router {
    // Prove the DI graph wires up at startup.
    let _ = wire();

    let db_up = Arc::new(AtomicBool::new(false));
    let db = match std::env::var("DATABASE_URL") {
        Ok(url) if !url.is_empty() => match PgPoolOptions::new().max_connections(5).connect_lazy(&url) {
            Ok(pool) => {
                spawn_db_heartbeat(pool.clone(), db_up.clone());
                Some(pool)
            }
            Err(e) => {
                eprintln!("DATABASE_URL is set but pool init failed: {e}");
                None
            }
        },
        _ => {
            eprintln!("DATABASE_URL not set — /health will report db: not_configured");
            None
        }
    };

    Router::new()
        .route("/health", get(health))
        .with_state(AppState { db, db_up })
}

/// Ping the database with `SELECT 1` every 30 seconds and publish the result to `db_up`. The first ping
/// runs immediately (before the initial sleep), so `/health` reflects reality within a moment of boot.
fn spawn_db_heartbeat(pool: PgPool, db_up: Arc<AtomicBool>) {
    tokio::spawn(async move {
        loop {
            let ok = sqlx::query("SELECT 1").execute(&pool).await.is_ok();
            db_up.store(ok, Ordering::Relaxed);
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}

/// Health endpoint. `200` when the DB is reachable (or not yet configured), `503` when it is configured
/// but the last heartbeat failed. Point Render's Health Check Path here.
async fn health(State(state): State<AppState>) -> impl IntoResponse {
    match state.db {
        None => (StatusCode::OK, Json(json!({ "status": "ok", "db": "not_configured" }))),
        Some(_) if state.db_up.load(Ordering::Relaxed) => {
            (StatusCode::OK, Json(json!({ "status": "ok", "db": "up" })))
        }
        Some(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "degraded", "db": "down" })),
        ),
    }
}
