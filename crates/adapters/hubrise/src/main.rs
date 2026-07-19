//! Standalone HubRise webhook web service (ADR-20260718-213352): binds `$PORT` and serves ONLY
//! `POST /adapters/hubrise/webhooks`. Verifies the callback (HMAC) and, when `DATABASE_URL` + `HUBRISE_ACCESS_TOKEN`
//! are set, drives the domain enrichment (OAuth API pull → ACL map → `ImportCatalog` / stock updates) over
//! a Postgres event store. Deploy as its own Render web service isolated from other partners — or mount
//! into the monolith via [`hubrise_adapter::routes`]. Migrations stay out-of-band (ADR-0043).

use std::sync::Arc;
use std::time::Duration;

use hubrise_adapter::api::HubRiseApiClient;
use hubrise_adapter::enrich::Enricher;
use hubrise_adapter::{routes, HubRiseEnricher};
use infrastructure::PgEventStore;

#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());

    // The enricher needs BOTH a database (to append) and a HubRise API token (to pull). Missing either
    // → run ingress-only (verified callbacks ACK as pending), never crash.
    let enricher: Option<Arc<dyn Enricher>> =
        match (std::env::var("DATABASE_URL"), HubRiseApiClient::from_env()) {
            (Ok(url), Ok(api)) if !url.trim().is_empty() => {
                let pool = sqlx::postgres::PgPoolOptions::new()
                    .max_connections(4)
                    .acquire_timeout(Duration::from_secs(10))
                    .connect_lazy(&url)
                    .unwrap_or_else(|e| panic!("DATABASE_URL pool init failed: {e}"));
                let store = Arc::new(PgEventStore::new(pool));
                Some(Arc::new(HubRiseEnricher::new(store, api)))
            }
            _ => {
                eprintln!(
                    "hubrise-webhook: enrichment disabled (need DATABASE_URL + HUBRISE_ACCESS_TOKEN); \
                     verifying callbacks only"
                );
                None
            }
        };

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    println!("hubrise-webhook adapter listening on {addr}");
    axum::serve(listener, routes(enricher)).await.expect("server error");
}
