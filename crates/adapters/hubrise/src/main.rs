//! Standalone HubRise webhook + connect web service (ADR-20260718-213352): binds `$PORT` and serves
//! `POST /adapters/hubrise/webhooks` plus the connect flow (`GET /adapters/hubrise/connect`,
//! `GET /adapters/hubrise/oauth/callback`, issue #20). Verifies callbacks (HMAC) and, when
//! `DATABASE_URL` is set, drives the domain enrichment (per-account token from `hubrise_connections`
//! → API pull → ACL map → `ImportCatalog` / stock updates) over a Postgres event store. Deploy as its
//! own Render web service isolated from other partners — or mount into the monolith via
//! [`hubrise_adapter::routes`]. Migrations stay out-of-band (ADR-0043).

use std::sync::Arc;
use std::time::Duration;

use hubrise_adapter::api::HubRiseApi;
use hubrise_adapter::connect::HttpHubRiseConnectGateway;
use hubrise_adapter::{routes, HubRiseConnectFlow, HubRiseEnricher, PgHubRiseConnections};
use infrastructure::{PgCommandJournal, PgEventStore, PgRestaurantRepository};

#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());

    // Enrichment + connect need only the database now (issue #20): the pull token is resolved per
    // connected account from `hubrise_connections` — the global HUBRISE_ACCESS_TOKEN is retired.
    let mut state = hubrise_adapter::HubRiseWebhookState::default();
    match std::env::var("DATABASE_URL") {
        Ok(url) if !url.trim().is_empty() => {
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(4)
                .acquire_timeout(Duration::from_secs(10))
                .connect_lazy(&url)
                .unwrap_or_else(|e| panic!("DATABASE_URL pool init failed: {e}"));
            state.raw = Some(Arc::new(hubrise_adapter::PgRawHubRiseCallbacks::new(pool.clone())));
            let store = Arc::new(PgEventStore::new(pool.clone()));
            // WORKER-channel command journal (ADR-20260720-015300): every enricher/connect send is
            // journaled before handling, so redeliveries dedupe instead of double-applying.
            let journal = Arc::new(PgCommandJournal::new(pool.clone()));
            let connections = Arc::new(PgHubRiseConnections::new(pool.clone()));
            let restaurants = Arc::new(PgRestaurantRepository::new(pool));
            state.enricher = Some(Arc::new(HubRiseEnricher::new(
                store.clone(),
                journal.clone(),
                connections.clone(),
                HubRiseApi::from_env(),
            )));
            // The connect routes additionally need the app credentials (HUBRISE_CLIENT_ID +
            // HUBRISE_WEBHOOK_SECRET + HUBRISE_CONNECT_REDIRECT_URL) — checked per request,
            // fail-closed in http.rs.
            state.connect = Some(Arc::new(HubRiseConnectFlow::new(
                store,
                journal,
                restaurants,
                connections,
                HttpHubRiseConnectGateway {
                    api: HubRiseApi::from_env(),
                    client_id: std::env::var("HUBRISE_CLIENT_ID").unwrap_or_default(),
                    client_secret: std::env::var("HUBRISE_WEBHOOK_SECRET").unwrap_or_default(),
                },
            )));
        }
        _ => {
            eprintln!(
                "hubrise-webhook: enrichment + connect disabled (DATABASE_URL unset); \
                 verifying callbacks only"
            );
        }
    };

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    println!("hubrise-webhook adapter listening on {addr}");
    axum::serve(listener, routes(state)).await.expect("server error");
}
