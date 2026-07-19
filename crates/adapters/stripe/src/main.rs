//! Standalone Stripe webhook web service (ADR-20260718-213352): binds `$PORT` and serves ONLY
//! `POST /adapters/stripe/webhooks` over a Postgres `EventStore`. This lets the Stripe adapter deploy as its own
//! Render web service, fully isolated from the other partners — or it can be mounted into the monolith via
//! [`stripe_adapter::routes`]. Migrations stay out-of-band (ADR-0043); this process only appends events.

use std::sync::Arc;
use std::time::Duration;

use infrastructure::PgEventStore;
use stripe_adapter::{routes, StripeWebhookIngestor};

#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(10))
        .connect_lazy(&url)
        .unwrap_or_else(|e| panic!("DATABASE_URL pool init failed: {e}"));
    let ingestor = Arc::new(StripeWebhookIngestor::new(Arc::new(PgEventStore::new(pool))));

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    println!("stripe-webhook adapter listening on {addr}");
    axum::serve(listener, routes(Some(ingestor))).await.expect("server error");
}
