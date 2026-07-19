//! HTTP shell for the HubRise adapter: `POST /adapters/hubrise/webhooks`. Thin — reads the raw body + signature,
//! delegates verification/parsing to the (framework-free) [`crate::acl`], and, when an [`Enricher`] is
//! wired and the callback needs a pull (catalog/inventory), drives the domain enrichment
//! (`api` pull → ACL map → command). Verification runs over the RAW body bytes.

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
    verify_hubrise_signature, HubRiseCallback, HUBRISE_SIGNATURE_HEADER, HUBRISE_WEBHOOK_SECRET_ENV,
};
use crate::enrich::{EnrichOutcome, Enricher};

/// Mount `POST /adapters/hubrise/webhooks`. The [`Enricher`] is `None` when no database / API token is configured
/// — verified callbacks are then ACKed as `verified_pending_enrichment` (ingress-only, as before).
pub fn routes(enricher: Option<Arc<dyn Enricher>>) -> Router {
    Router::new().route("/adapters/hubrise/webhooks", post(hubrise_webhook)).with_state(enricher)
}

async fn hubrise_webhook(
    State(enricher): State<Option<Arc<dyn Enricher>>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Fail closed: without the client secret nothing can be authenticated.
    let secret = match std::env::var(HUBRISE_WEBHOOK_SECRET_ENV) {
        Ok(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "hubrise webhooks not configured (HUBRISE_WEBHOOK_SECRET unset)",
            )
                .into_response()
        }
    };
    let Some(signature) = headers.get(HUBRISE_SIGNATURE_HEADER).and_then(|v| v.to_str().ok()) else {
        return (StatusCode::BAD_REQUEST, "missing X-HubRise-Hmac-SHA256 header").into_response();
    };
    if let Err(e) = verify_hubrise_signature(&secret, signature, &body) {
        return (StatusCode::BAD_REQUEST, format!("invalid HubRise signature: {e}")).into_response();
    }
    let callback: HubRiseCallback = match serde_json::from_slice(&body) {
        Ok(c) => c,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("unparsable HubRise callback: {e}"))
                .into_response()
        }
    };

    // No enricher wired (or the callback carries no pullable resource): ingress-only ACK, as before.
    let Some(enricher) = enricher.filter(|_| callback.needs_pull()) else {
        println!(
            "hubrise webhook: verified {}.{} (id {}){}",
            callback.resource_type,
            callback.event_type,
            callback.id,
            if callback.needs_pull() { " [needs enricher — none wired]" } else { "" }
        );
        return (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({ "received": true, "status": "verified_pending_enrichment" })),
        )
            .into_response();
    };

    // Enrich: pull the changed resource, map it, apply the domain write.
    match enricher.enrich(&callback).await {
        Ok(outcome) => {
            let status = match &outcome {
                EnrichOutcome::CatalogImported { catalog_id } => {
                    println!("hubrise webhook: imported catalog {} (cb {})", catalog_id.0, callback.id);
                    "catalog_imported"
                }
                EnrichOutcome::InventoryApplied { applied, skipped } => {
                    println!(
                        "hubrise webhook: inventory applied={applied} skipped={skipped} (cb {})",
                        callback.id
                    );
                    "inventory_applied"
                }
                EnrichOutcome::Ignored { resource_type } => {
                    println!("hubrise webhook: ignored {resource_type} (cb {})", callback.id);
                    "ignored"
                }
                EnrichOutcome::Skipped { reason } | EnrichOutcome::MapFailed { reason } => {
                    // Definitive: retrying the same payload would not help (logged, ACKed).
                    eprintln!("hubrise webhook: skipped (cb {}): {reason}", callback.id);
                    "skipped"
                }
                EnrichOutcome::PullFailed { reason } => {
                    // The pull itself failed — ask HubRise to redeliver.
                    eprintln!("hubrise webhook: pull failed (cb {}): {reason}", callback.id);
                    return (StatusCode::BAD_GATEWAY, "hubrise API pull failed").into_response();
                }
            };
            (StatusCode::OK, Json(serde_json::json!({ "received": true, "status": status })))
                .into_response()
        }
        // Infrastructure failure (event store unreachable): 5xx so HubRise redelivers the callback.
        Err(e) => {
            eprintln!("hubrise webhook: enrichment append failed (cb {}): {e}", callback.id);
            (StatusCode::INTERNAL_SERVER_ERROR, "failed to record enrichment").into_response()
        }
    }
}
