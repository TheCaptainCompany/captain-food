//! HTTP shell for the CoopCycle adapter: `POST /adapters/coopcycle/{instance}/webhooks`. Thin — it
//! reads the `{instance}` path segment, looks up THAT instance's signing secret in the registry
//! (federation: each co-op has its own secret), verifies the signature over the RAW body, then
//! delegates mapping/ingestion to the (framework-free) [`crate::acl`]. Fails CLOSED when the instance
//! is unknown or the adapter has no database configured.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};

use crate::acl::{
    verify_signature, CoopCycleEvent, CoopCycleIngestOutcome, CoopCycleWebhookIngestor,
};
use crate::config::CoopCycleRegistry;

/// Route state: the ingestor (present when a database is configured) + the instance registry (for
/// per-instance webhook secrets). Cheap to clone (both behind `Arc`).
#[derive(Clone, Default)]
pub struct CoopCycleWebhookState {
    pub ingestor: Option<Arc<CoopCycleWebhookIngestor>>,
    pub registry: Arc<CoopCycleRegistry>,
}

/// Mount `POST /adapters/coopcycle/{instance}/webhooks`.
pub fn routes(state: CoopCycleWebhookState) -> Router {
    Router::new()
        .route("/adapters/coopcycle/{instance}/webhooks", post(coopcycle_webhook))
        .with_state(state)
}

async fn coopcycle_webhook(
    State(state): State<CoopCycleWebhookState>,
    Path(instance): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Fail closed: without THIS instance's signing secret nothing can be authenticated (unknown
    // instance, or registry not configured).
    let Some(secret) = state.registry.webhook_secret(&instance).map(str::to_string) else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("coopcycle webhooks not configured for instance '{instance}'"),
        )
            .into_response();
    };
    let Some(signature) = headers.get("coopcycle-signature").and_then(|v| v.to_str().ok()) else {
        return (StatusCode::BAD_REQUEST, "missing CoopCycle-Signature header").into_response();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if let Err(e) = verify_signature(&secret, signature, &body, now) {
        return (StatusCode::BAD_REQUEST, format!("invalid CoopCycle signature: {e}")).into_response();
    }

    let Some(ingestor) = state.ingestor else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "coopcycle webhook ingestor not available (no database configured)",
        )
            .into_response();
    };
    // The verbatim body is mirrored into `external_coopcycle_events`; the typed subset drives the ACL.
    let raw_body: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("unparsable CoopCycle event: {e}"))
                .into_response()
        }
    };
    let event: CoopCycleEvent = match serde_json::from_value(raw_body.clone()) {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("unparsable CoopCycle event: {e}"))
                .into_response()
        }
    };

    match ingestor.ingest(&instance, &event, &raw_body).await {
        Ok(outcome) => {
            let status = match &outcome {
                CoopCycleIngestOutcome::Recorded { event_type } => {
                    tracing::info!(adapter = "coopcycle", %instance, %event_type, event_id = %event.id, "webhook fact recorded");
                    "recorded"
                }
                CoopCycleIngestOutcome::Duplicate => "duplicate",
                CoopCycleIngestOutcome::Ignored { .. } => "ignored",
                CoopCycleIngestOutcome::Unmappable { reason } => {
                    tracing::warn!(
                        adapter = "coopcycle",
                        %instance,
                        event_type = %event.event_type,
                        event_id = %event.id,
                        %reason,
                        "webhook verified but unmappable -- a retry would carry the same payload"
                    );
                    "unmappable"
                }
            };
            (StatusCode::OK, Json(serde_json::json!({ "received": true, "status": status })))
                .into_response()
        }
        Err(e) => {
            tracing::error!(adapter = "coopcycle", %instance, event_id = %event.id, error = %e, "ingest failed -- 5xx so the instance retries");
            (StatusCode::INTERNAL_SERVER_ERROR, "failed to record event").into_response()
        }
    }
}
