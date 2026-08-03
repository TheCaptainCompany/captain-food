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
            let nudges =
                Arc::new(infrastructure::persistence::mailbox_store::MailboxNudges::default());
            let mailbox = Arc::new(
                infrastructure::persistence::mailbox_store::PgMailbox::new(pool.clone())
                    .with_nudges(nudges.clone()),
            );
            // RUN_MAILBOX_WORKERS (#272 D3): the connect/import lanes this adapter's enricher
            // and connect flow enqueue on — delivered HERE when the monolith is down.
            if infrastructure::mailbox::standalone_workers_enabled() {
                infrastructure::mailbox::spawn_standalone_workers(
                    pool.clone(),
                    "hubrise",
                    &["RestaurantAccount", "Restaurant", "Catalog"],
                    Arc::new(infrastructure::FailClosedPaymentGateway),
                    nudges.clone(),
                    Default::default(),
                );
            }
            let connections = Arc::new(PgHubRiseConnections::new(pool.clone()));
            let restaurants = Arc::new(PgRestaurantRepository::new(pool));
            state.enricher = Some(Arc::new(HubRiseEnricher::new(
                mailbox.clone(),
                connections.clone(),
                HubRiseApi::from_env(),
            )));
            // The connect routes additionally need the app credentials (HUBRISE_CLIENT_ID +
            // HUBRISE_WEBHOOK_SECRET + HUBRISE_CONNECT_REDIRECT_URL) — checked per request,
            // fail-closed in http.rs.
            state.connect = Some(Arc::new(HubRiseConnectFlow::new(
                mailbox.clone(),
                // The D4 read door (#304). The bus is process-local and subscriber-less in a
                // standalone adapter -- exactly as `run_standalone_workers` builds its own -- and
                // that is fine here: the connect flow only ever PULLS a terminal status, which
                // reads the durable `inbound_messages` row, never the stream.
                actor_client::ActorClient::new(mailbox, Default::default()),
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
            tracing::warn!(
                adapter = "hubrise",
                "enrichment + connect disabled (DATABASE_URL unset) -- verifying callbacks only"
            );
        }
    };

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    tracing::info!(adapter = "hubrise", %addr, "webhook adapter listening");
    axum::serve(listener, routes(state))
        .with_graceful_shutdown(infrastructure::mailbox::shutdown_signal())
        .await
        .expect("server error");
}
