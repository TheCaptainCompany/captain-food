//! Standalone Avelo37 webhook web service (ADR-20260718-213352): binds `$PORT` and serves ONLY
//! `POST /adapters/avelo37/webhooks` over Postgres staging + the inbound inbox. This lets the
//! Avelo37 adapter deploy as its own Render web service, fully isolated from the other partners — or
//! it can be mounted into the monolith via [`avelo37_adapter::routes`]. Migrations stay out-of-band
//! (ADR-0043); this process only stages + drains inbound facts.

use std::sync::Arc;
use std::time::Duration;

use avelo37_adapter::{routes, Avelo37WebhookIngestor, PgRawAvelo37Events};
use infrastructure::persistence::mailbox_store::PgMailbox;

#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(10))
        .connect_lazy(&url)
        .unwrap_or_else(|e| panic!("DATABASE_URL pool init failed: {e}"));
    // Standalone deployment (ADR-20260731-122500): mirror + ENQUEUE on the shared mailbox; by
    // default the monolith's MailboxWorkers deliver (same database, lease-competed). With
    // RUN_MAILBOX_WORKERS on (#272 D3), THIS process also runs the DeliveryJob lane so delivery
    // facts keep flowing while the monolith is down.
    let nudges = Arc::new(infrastructure::persistence::mailbox_store::MailboxNudges::default());
    let mailbox = Arc::new(PgMailbox::new(pool.clone()).with_nudges(nudges.clone()));
    let ingestor =
        Arc::new(Avelo37WebhookIngestor::new(Arc::new(PgRawAvelo37Events::new(pool.clone())), mailbox));
    if infrastructure::mailbox::standalone_workers_enabled() {
        infrastructure::mailbox::spawn_standalone_workers(
            pool,
            "avelo37",
            &["DeliveryJob"],
            Arc::new(infrastructure::FailClosedPaymentGateway),
            nudges,
            Default::default(),
        );
    }

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    tracing::info!(adapter = "avelo37", %addr, "webhook adapter listening");
    axum::serve(listener, routes(Some(ingestor)))
        .with_graceful_shutdown(infrastructure::mailbox::shutdown_signal())
        .await
        .expect("server error");
}
