//! HTTP shell for the Avelo37 adapter: `POST /adapters/avelo37/webhooks`. Thin — it reads the raw
//! body + signature, delegates verification/mapping/ingestion to the (framework-free) [`crate::acl`],
//! and turns the outcome into a status code. Verification runs over the RAW body bytes (a
//! re-serialized JSON would never verify).

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
    verify_signature, Avelo37Event, Avelo37IngestOutcome, Avelo37WebhookIngestor,
    AVELO37_WEBHOOK_SECRET_ENV,
};

/// Mount `POST /adapters/avelo37/webhooks`. The ingestor is `None` when no database is configured (→ 503).
pub fn routes(ingestor: Option<Arc<Avelo37WebhookIngestor>>) -> Router {
    Router::new().route("/adapters/avelo37/webhooks", post(avelo37_webhook)).with_state(ingestor)
}

async fn avelo37_webhook(
    State(ingestor): State<Option<Arc<Avelo37WebhookIngestor>>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Fail closed: without the signing secret nothing can be authenticated.
    let secret = match std::env::var(AVELO37_WEBHOOK_SECRET_ENV) {
        Ok(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "avelo37 webhooks not configured (AVELO37_WEBHOOK_SECRET unset)",
            )
                .into_response()
        }
    };
    let Some(signature) = headers.get("avelo37-signature").and_then(|v| v.to_str().ok()) else {
        return (StatusCode::BAD_REQUEST, "missing Avelo37-Signature header").into_response();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if let Err(e) = verify_signature(&secret, signature, &body, now) {
        // 4xx (not 2xx): the partner surfaces the failure and retries — correct for a mis-rolled
        // secret; an attacker just gets a rejection.
        return (StatusCode::BAD_REQUEST, format!("invalid Avelo37 signature: {e}")).into_response();
    }

    let Some(ingestor) = ingestor else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "avelo37 webhook ingestor not available (no database configured)",
        )
            .into_response();
    };
    // The verbatim body is mirrored into `external_avelo37_events` (ADR-20260720-015400); the typed
    // subset drives the ACL. Parse the raw JSON once, derive the typed view from it.
    let raw_body: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("unparsable Avelo37 event: {e}"))
                .into_response()
        }
    };
    let event: Avelo37Event = match serde_json::from_value(raw_body.clone()) {
        Ok(e) => e,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("unparsable Avelo37 event: {e}"))
                .into_response()
        }
    };

    match ingestor.ingest(&event, &raw_body).await {
        // Definitive outcomes ACK with 200 so the partner stops redelivering; `unmappable`
        // (verified but missing our job reference) is logged — a retry would carry the same payload.
        Ok(outcome) => {
            let status = match &outcome {
                Avelo37IngestOutcome::Recorded { event_type } => {
                    println!("avelo37 webhook: recorded {event_type} ({})", event.id);
                    "recorded"
                }
                Avelo37IngestOutcome::Duplicate => "duplicate",
                Avelo37IngestOutcome::Ignored { .. } => "ignored",
                Avelo37IngestOutcome::Unmappable { reason } => {
                    eprintln!(
                        "avelo37 webhook: unmappable {} ({}): {reason}",
                        event.event_type, event.id
                    );
                    "unmappable"
                }
            };
            (StatusCode::OK, Json(serde_json::json!({ "received": true, "status": status })))
                .into_response()
        }
        // Infrastructure failure (staging unreachable): 5xx so the partner retries the delivery.
        Err(e) => {
            eprintln!("avelo37 webhook: ingest failed for {}: {e}", event.id);
            (StatusCode::INTERNAL_SERVER_ERROR, "failed to record event").into_response()
        }
    }
}
