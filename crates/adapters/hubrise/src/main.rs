//! Standalone HubRise webhook web service (ADR-20260718-213352): binds `$PORT` and serves ONLY
//! `POST /adapters/hubrise/webhooks`. Verifies the callback (HMAC) and, when `DATABASE_URL` + `HUBRISE_ACCESS_TOKEN`
//! are set, drives the domain enrichment (OAuth API pull → ACL map → `ImportCatalog` / stock updates) over
//! a Postgres event store. Deploy as its own Render web service isolated from other partners — or mount
//! into the monolith via [`hubrise_adapter::routes`]. Migrations stay out-of-band (ADR-0043).

use std::sync::Arc;
use std::time::Duration;

use hubrise_adapter::api::HubRiseApiClient;
use hubrise_adapter::{routes, HubRiseEnricher};
use infrastructure::{PgCommandJournal, PgEventStore};

#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());

    // The enricher needs BOTH a database (to append) and a HubRise API token (to pull). Missing either
    // → run ingress-only (verified callbacks ACK as pending), never crash.
    let mut state = hubrise_adapter::HubRiseWebhookState::default();
    match (std::env::var("DATABASE_URL"), HubRiseApiClient::from_env()) {
        (Ok(url), api) if !url.trim().is_empty() => {
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(4)
                .acquire_timeout(Duration::from_secs(10))
                .connect_lazy(&url)
                .unwrap_or_else(|e| panic!("DATABASE_URL pool init failed: {e}"));
            // The raw mirror (external_hubrise_callbacks) needs only the database.
            state.raw = Some(Arc::new(hubrise_adapter::PgRawHubRiseCallbacks::new(pool.clone())));
            match api {
                Ok(api) => {
                    let store = Arc::new(PgEventStore::new(pool.clone()));
                    // WORKER-channel command journal (ADR-20260720-015300): every enricher send is
                    // journaled before handling, so redeliveries dedupe instead of double-applying.
                    let journal = Arc::new(PgCommandJournal::new(pool));
                    state.enricher = Some(Arc::new(HubRiseEnricher::new(store, journal, api)));
                }
                Err(_) => eprintln!(
                    "hubrise-webhook: enrichment disabled (HUBRISE_ACCESS_TOKEN unset); \
                     mirroring + verifying callbacks only"
                ),
            }
        }
        _ => {
            eprintln!(
                "hubrise-webhook: enrichment disabled (need DATABASE_URL + HUBRISE_ACCESS_TOKEN); \
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
