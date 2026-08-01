//! Standalone Stripe webhook web service (ADR-20260718-213352): binds `$PORT` and serves ONLY
//! `POST /adapters/stripe/webhooks` over a Postgres `EventStore`. This lets the Stripe adapter deploy as its own
//! Render web service, fully isolated from the other partners — or it can be mounted into the monolith via
//! [`stripe_adapter::routes`]. Migrations stay out-of-band (ADR-0043); this process only appends events.

use std::sync::Arc;
use std::time::Duration;

use infrastructure::persistence::mailbox_store::PgMailbox;
use stripe_adapter::{routes, PgRawStripeEvents, StripeWebhookIngestor};

#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(10))
        .connect_lazy(&url)
        .unwrap_or_else(|e| panic!("DATABASE_URL pool init failed: {e}"));
    // Standalone deployment (ADR-20260731-122500): mirror + ENQUEUE on the shared mailbox;
    // by default the monolith's per-actor-type MailboxWorkers deliver (same database,
    // lease-competed). With RUN_MAILBOX_WORKERS on (#272 D3), THIS process also runs workers
    // for the lanes its ingestor feeds, so delivery survives monolith downtime.
    let nudges = Arc::new(infrastructure::persistence::mailbox_store::MailboxNudges::default());
    let mailbox = Arc::new(PgMailbox::new(pool.clone()).with_nudges(nudges.clone()));
    let ingestor =
        Arc::new(StripeWebhookIngestor::new(Arc::new(PgRawStripeEvents::new(pool.clone())), mailbox));
    if infrastructure::mailbox::standalone_workers_enabled() {
        // Payment (the ingested Stripe facts) + the two PM lanes their B2-chained copies land
        // on — a capture chained here must also be REACTED to here when the monolith is down.
        let payments: Arc<dyn application::generated::services::PaymentService> =
            match std::env::var("STRIPE_SECRET_KEY") {
                Ok(key) if !key.is_empty() => {
                    Arc::new(stripe_adapter::StripePaymentGateway::new(key))
                }
                _ => {
                    tracing::warn!(
                        "STRIPE_SECRET_KEY unset -- payment-dependent deliveries will decline"
                    );
                    Arc::new(infrastructure::FailClosedPaymentGateway)
                }
            };
        infrastructure::mailbox::spawn_standalone_workers(
            pool,
            "stripe",
            &["Payment", "PlaceOrderProcess", "RefundProcess"],
            payments,
            nudges,
            Default::default(),
        );
    }

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    tracing::info!(adapter = "stripe", %addr, "webhook adapter listening");
    axum::serve(listener, routes(Some(ingestor))).await.expect("server error");
}
