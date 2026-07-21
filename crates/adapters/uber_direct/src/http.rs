//! HTTP shell for the Uber Direct adapter: `POST /adapters/uber-direct/webhooks`. Thin — it reads the
//! configured webhook secret, verifies the `X-Uber-Signature` (raw-body HMAC) over the RAW body, then
//! delegates mapping/ingestion to the (framework-free) [`crate::acl`]. Fails CLOSED when the adapter
//! has no signing secret (unconfigured) or no database configured.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};

use crate::acl::{verify_signature, UberDirectWebhookIngestor, UberEvent, UberIngestOutcome};

/// Route state: the ingestor (present when a database is configured) + the webhook signing secret
/// (present when `UBER_DIRECT_*` is configured). Cheap to clone.
#[derive(Clone, Default)]
pub struct UberDirectWebhookState {
    pub ingestor: Option<Arc<UberDirectWebhookIngestor>>,
    pub webhook_secret: Option<Arc<String>>,
}

/// Mount `POST /adapters/uber-direct/webhooks`.
pub fn routes(state: UberDirectWebhookState) -> Router {
    Router::new()
        .route("/adapters/uber-direct/webhooks", post(uber_direct_webhook))
        .with_state(state)
}

async fn uber_direct_webhook(
    State(state): State<UberDirectWebhookState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Fail closed: without a signing secret nothing can be authenticated (adapter not configured).
    let Some(secret) = state.webhook_secret.as_deref() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "uber_direct webhooks not configured").into_response();
    };
    let Some(signature) = headers.get("x-uber-signature").and_then(|v| v.to_str().ok()) else {
        return (StatusCode::BAD_REQUEST, "missing X-Uber-Signature header").into_response();
    };
    if let Err(e) = verify_signature(secret, signature, &body) {
        return (StatusCode::BAD_REQUEST, format!("invalid Uber signature: {e}")).into_response();
    }

    let Some(ingestor) = state.ingestor else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "uber_direct webhook ingestor not available (no database configured)",
        )
            .into_response();
    };
    // The verbatim body is mirrored into `external_uber_direct_events`; the typed subset drives the ACL.
    let raw_body: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("unparsable Uber event: {e}")).into_response()
        }
    };
    let event: UberEvent = match serde_json::from_value(raw_body.clone()) {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("unparsable Uber event: {e}")).into_response()
        }
    };

    match ingestor.ingest(&event, &raw_body).await {
        Ok(outcome) => {
            let status = match &outcome {
                UberIngestOutcome::Recorded { event_type } => {
                    println!("uber_direct webhook: recorded {event_type} ({})", event.event_id);
                    "recorded"
                }
                UberIngestOutcome::Duplicate => "duplicate",
                UberIngestOutcome::Ignored { .. } => "ignored",
                UberIngestOutcome::Unmappable { reason } => {
                    eprintln!(
                        "uber_direct webhook: unmappable {} ({}): {reason}",
                        event.kind, event.event_id
                    );
                    "unmappable"
                }
            };
            (StatusCode::OK, Json(serde_json::json!({ "received": true, "status": status })))
                .into_response()
        }
        Err(e) => {
            eprintln!("uber_direct webhook: ingest failed for {}: {e}", event.event_id);
            (StatusCode::INTERNAL_SERVER_ERROR, "failed to record event").into_response()
        }
    }
}
