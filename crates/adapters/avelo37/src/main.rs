//! Standalone Avelo37 webhook web service (ADR-20260718-213352): binds `$PORT` and serves ONLY
//! `POST /adapters/avelo37/webhooks` over Postgres staging + the inbound inbox. This lets the
//! Avelo37 adapter deploy as its own Render web service, fully isolated from the other partners — or
//! it can be mounted into the monolith via [`avelo37_adapter::routes`]. Migrations stay out-of-band
//! (ADR-0043); this process only stages + drains inbound facts.

use std::sync::Arc;
use std::time::Duration;

use avelo37_adapter::{routes, Avelo37WebhookIngestor, PgRawAvelo37Events};
use infrastructure::{PgCommandJournal, PgEventStore, PgInboundEvents};

#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(10))
        .connect_lazy(&url)
        .unwrap_or_else(|e| panic!("DATABASE_URL pool init failed: {e}"));
    // Standalone deployment: mirror + stage on ingest, and run our OWN drain worker (the monolith's
    // worker is a different process here) delivering staged facts through the normal write path.
    let inbox = Arc::new(PgInboundEvents::new(pool.clone()));
    let drain = Arc::new(infrastructure::InboundEventsDrainWorker::new(
        inbox.clone(),
        Arc::new(PgCommandJournal::new(pool.clone())),
        Arc::new(PgEventStore::new(pool.clone())),
    ));
    tokio::spawn(drain.clone().run_loop());
    let nudge_worker = drain.clone();
    let ingestor = Arc::new(
        Avelo37WebhookIngestor::new(Arc::new(PgRawAvelo37Events::new(pool)), inbox).with_nudge(
            Arc::new(move || {
                let w = nudge_worker.clone();
                tokio::spawn(async move { w.run_once().await });
            }),
        ),
    );

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    println!("avelo37-webhook adapter listening on {addr}");
    axum::serve(listener, routes(Some(ingestor))).await.expect("server error");
}
