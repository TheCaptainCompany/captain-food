//! HTTP shell for the Stripe adapter: `POST /adapters/stripe/webhooks`. Thin — it reads the raw body + signature,
//! delegates verification/mapping/ingestion to the (framework-free) [`crate::acl`], and turns the outcome
//! into a status code. Verification runs over the RAW body bytes (a re-serialized JSON would never verify).

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};

use crate::acl::{
    verify_signature, StripeEvent, StripeIngestOutcome, StripeWebhookIngestor,
    STRIPE_WEBHOOK_SECRET_ENV,
};

/// Mount `POST /adapters/stripe/webhooks`. The ingestor is `None` when no database is configured (→ 503).
pub fn routes(ingestor: Option<Arc<StripeWebhookIngestor>>) -> Router {
    Router::new().route("/adapters/stripe/webhooks", post(stripe_webhook)).with_state(ingestor)
}

async fn stripe_webhook(
    State(ingestor): State<Option<Arc<StripeWebhookIngestor>>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Fail closed: without the signing secret nothing can be authenticated.
    let secret = match std::env::var(STRIPE_WEBHOOK_SECRET_ENV) {
        Ok(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "stripe webhooks not configured (STRIPE_WEBHOOK_SECRET unset)",
            )
                .into_response()
        }
    };
    let Some(signature) = headers.get("stripe-signature").and_then(|v| v.to_str().ok()) else {
        return (StatusCode::BAD_REQUEST, "missing Stripe-Signature header").into_response();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if let Err(e) = verify_signature(&secret, signature, &body, now) {
        // 4xx (not 2xx): Stripe surfaces the failure in its dashboard and retries — correct for a
        // mis-rolled secret; an attacker just gets a rejection.
        return (StatusCode::BAD_REQUEST, format!("invalid Stripe signature: {e}")).into_response();
    }

    let Some(ingestor) = ingestor else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "stripe webhook ingestor not available (no database configured)",
        )
            .into_response();
    };
    let event: StripeEvent = match serde_json::from_slice(&body) {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("unparsable Stripe event: {e}"))
                .into_response()
        }
    };

    match ingestor.ingest(&event).await {
        // Definitive outcomes ACK with 200 so Stripe stops redelivering; `unmappable` (verified but
        // missing our metadata) is logged — a retry would carry the same payload.
        Ok(outcome) => {
            let status = match &outcome {
                StripeIngestOutcome::Recorded { event_type } => {
                    println!("stripe webhook: recorded {event_type} ({})", event.id);
                    "recorded"
                }
                StripeIngestOutcome::Duplicate => "duplicate",
                StripeIngestOutcome::Ignored { .. } => "ignored",
                StripeIngestOutcome::Unmappable { reason } => {
                    eprintln!("stripe webhook: unmappable {} ({}): {reason}", event.event_type, event.id);
                    "unmappable"
                }
            };
            (StatusCode::OK, Json(serde_json::json!({ "received": true, "status": status })))
                .into_response()
        }
        // Infrastructure failure (event store unreachable): 5xx so Stripe retries the delivery.
        Err(e) => {
            eprintln!("stripe webhook: append failed for {}: {e}", event.id);
            (StatusCode::INTERNAL_SERVER_ERROR, "failed to record event").into_response()
        }
    }
}
