//! HTTP shell for the HubRise adapter: `POST /adapters/hubrise/webhooks` plus the CONNECT flow's two
//! routes (issue #20): `GET /adapters/hubrise/connect` (302 → HubRise authorize) and
//! `GET /adapters/hubrise/oauth/callback` (code → token → provision + store). Thin — reads the raw
//! body + signature, delegates verification/parsing to the (framework-free) [`crate::acl`], and, when
//! an [`Enricher`] is wired and the callback needs a pull (catalog/inventory), drives the domain
//! enrichment (`api` pull → ACL map → command). Verification runs over the RAW body bytes.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::acl::{
    verify_hubrise_signature, HubRiseCallback, HUBRISE_SIGNATURE_HEADER, HUBRISE_WEBHOOK_SECRET_ENV,
};
use crate::connect::{ConnectError, ConnectService};
use crate::enrich::{EnrichOutcome, Enricher};
use crate::raw::RawHubRiseCallbacks;

/// HubRise app client id — enables the connect flow (with the client secret, which doubles as the
/// webhook HMAC key: `HUBRISE_WEBHOOK_SECRET`).
pub const HUBRISE_CLIENT_ID_ENV: &str = "HUBRISE_CLIENT_ID";
/// OAuth scope requested on connect. Default: account-wide catalog+inventory read — one token covers
/// every location of the account (`docs/integrations/hubrise-process.md` §0).
pub const HUBRISE_OAUTH_SCOPE_ENV: &str = "HUBRISE_OAUTH_SCOPE";
/// The public URL HubRise redirects back to (this deployable's `/adapters/hubrise/oauth/callback`).
pub const HUBRISE_CONNECT_REDIRECT_URL_ENV: &str = "HUBRISE_CONNECT_REDIRECT_URL";

const DEFAULT_OAUTH_SCOPE: &str = "account[catalog.read,inventory.read]";
const AUTHORIZE_URL: &str = "https://manager.hubrise.com/oauth2/v1/authorize";
/// Connect `state` validity — generous for a human OAuth round-trip, small enough to bound replay.
const STATE_VALIDITY_SECS: i64 = 900;

/// The endpoint's wiring: the adapter-owned raw mirror (`external_hubrise_callbacks`,
/// ADR-20260720-015400), the optional domain enrichment, and the optional connect flow (issue #20).
/// Any may be absent (no database) — the endpoints degrade fail-closed.
#[derive(Clone, Default)]
pub struct HubRiseWebhookState {
    pub raw: Option<Arc<dyn RawHubRiseCallbacks>>,
    pub enricher: Option<Arc<dyn Enricher>>,
    pub connect: Option<Arc<dyn ConnectService>>,
}

/// Mount `POST /adapters/hubrise/webhooks` + the connect-flow routes. The [`Enricher`] is `None` when
/// no database is configured — verified callbacks are then ACKed as `verified_pending_enrichment`
/// (ingress-only); the connect routes answer 503 without a database or app credentials.
pub fn routes(state: HubRiseWebhookState) -> Router {
    Router::new()
        .route("/adapters/hubrise/webhooks", post(hubrise_webhook))
        .route("/adapters/hubrise/connect", get(hubrise_connect))
        .route("/adapters/hubrise/oauth/callback", get(hubrise_oauth_callback))
        .with_state(state)
}

// ================================================================================================
// Connect flow (issue #20)
// ================================================================================================

/// Anti-CSRF `state`: `"<unix ts>.<hex HMAC-SHA256(client_secret, ts)>"` — stateless, verifiable by
/// any replica, expires after [`STATE_VALIDITY_SECS`]. The secret is the app client secret (the same
/// one HubRise signs webhooks with), so no extra key material is introduced.
fn make_state(secret: &str, now_unix: i64) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts keys of any length");
    mac.update(now_unix.to_string().as_bytes());
    format!("{now_unix}.{}", hex::encode(mac.finalize().into_bytes()))
}

/// Constant-time verification of [`make_state`] within the validity window.
fn verify_state(secret: &str, state: &str, now_unix: i64) -> bool {
    let Some((ts_str, sig_hex)) = state.split_once('.') else { return false };
    let Ok(ts) = ts_str.parse::<i64>() else { return false };
    if now_unix - ts > STATE_VALIDITY_SECS || ts > now_unix + 60 {
        return false;
    }
    let Ok(sig) = hex::decode(sig_hex) else { return false };
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts keys of any length");
    mac.update(ts_str.as_bytes());
    mac.verify_slice(&sig).is_ok()
}

fn connect_config() -> Result<(String, String, String, String), Response> {
    let missing = |what: &str| {
        (StatusCode::SERVICE_UNAVAILABLE, format!("hubrise connect not configured ({what} unset)"))
            .into_response()
    };
    let client_id = match std::env::var(HUBRISE_CLIENT_ID_ENV) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => return Err(missing(HUBRISE_CLIENT_ID_ENV)),
    };
    let secret = match std::env::var(HUBRISE_WEBHOOK_SECRET_ENV) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => return Err(missing(HUBRISE_WEBHOOK_SECRET_ENV)),
    };
    let redirect = match std::env::var(HUBRISE_CONNECT_REDIRECT_URL_ENV) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => return Err(missing(HUBRISE_CONNECT_REDIRECT_URL_ENV)),
    };
    let scope = std::env::var(HUBRISE_OAUTH_SCOPE_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_OAUTH_SCOPE.to_string());
    Ok((client_id, secret, redirect, scope))
}

fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}

/// `GET /adapters/hubrise/connect` — start the OAuth round-trip: 302 to the HubRise authorize page
/// with our client id, redirect URL, scope, and a signed anti-CSRF `state`.
async fn hubrise_connect(State(state): State<HubRiseWebhookState>) -> Response {
    if state.connect.is_none() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "hubrise connect not available (no database configured)",
        )
            .into_response();
    }
    let (client_id, secret, redirect, scope) = match connect_config() {
        Ok(cfg) => cfg,
        Err(resp) => return resp,
    };
    let url = format!(
        "{AUTHORIZE_URL}?client_id={}&redirect_uri={}&scope={}&state={}",
        urlencode(&client_id),
        urlencode(&redirect),
        urlencode(&scope),
        urlencode(&make_state(&secret, now_unix())),
    );
    Redirect::temporary(&url).into_response()
}

#[derive(serde::Deserialize)]
struct OAuthCallbackParams {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// `GET /adapters/hubrise/oauth/callback` — verify the state, exchange the code, provision the
/// account/locations/catalogs and store the token (see [`crate::connect`]).
async fn hubrise_oauth_callback(
    State(state): State<HubRiseWebhookState>,
    Query(params): Query<OAuthCallbackParams>,
) -> Response {
    let Some(connect) = state.connect else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "hubrise connect not available (no database configured)",
        )
            .into_response();
    };
    let (_client_id, secret, _redirect, _scope) = match connect_config() {
        Ok(cfg) => cfg,
        Err(resp) => return resp,
    };
    if let Some(err) = params.error {
        return (StatusCode::BAD_REQUEST, format!("hubrise authorization refused: {err}"))
            .into_response();
    }
    let ok_state =
        params.state.as_deref().map(|s| verify_state(&secret, s, now_unix())).unwrap_or(false);
    if !ok_state {
        return (StatusCode::BAD_REQUEST, "invalid or expired connect state").into_response();
    }
    let Some(code) = params.code.filter(|c| !c.trim().is_empty()) else {
        return (StatusCode::BAD_REQUEST, "missing authorization code").into_response();
    };

    match connect.connect(code.trim()).await {
        Ok(summary) => {
            println!(
                "hubrise connect: account {} → RestaurantAccount {} ({} locations, {} catalogs created, {} imported){}",
                summary.hubrise_account_id,
                summary.restaurant_account_id,
                summary.locations,
                summary.catalogs_created,
                summary.catalogs_imported,
                if summary.warnings.is_empty() {
                    String::new()
                } else {
                    format!("; warnings: {}", summary.warnings.join(" | "))
                }
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "connected": true,
                    "hubriseAccountId": summary.hubrise_account_id,
                    "restaurantAccountId": summary.restaurant_account_id,
                    "accountName": summary.account_name,
                    "locations": summary.locations,
                    "catalogsCreated": summary.catalogs_created,
                    "catalogsImported": summary.catalogs_imported,
                    "warnings": summary.warnings,
                })),
            )
                .into_response()
        }
        Err(e @ (ConnectError::Exchange(_) | ConnectError::NoAccountInScope)) => {
            eprintln!("hubrise connect failed: {e}");
            (StatusCode::BAD_REQUEST, format!("hubrise connect failed: {e}")).into_response()
        }
        Err(e @ ConnectError::Pull(_)) => {
            eprintln!("hubrise connect failed: {e}");
            (StatusCode::BAD_GATEWAY, format!("hubrise connect failed: {e} — retry the connect"))
                .into_response()
        }
        Err(e @ ConnectError::Infra(_)) => {
            eprintln!("hubrise connect failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "hubrise connect failed to record — retry")
                .into_response()
        }
    }
}

/// Minimal percent-encoding for query values (no extra dependency): unreserved chars pass through.
fn urlencode(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for b in v.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

async fn hubrise_webhook(
    State(state): State<HubRiseWebhookState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let HubRiseWebhookState { raw, enricher, connect: _ } = state;
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

    // Mirror the VERBATIM verified callback first (ADR-20260720-015400). The callback id is the
    // dedupe key (UUIDv5 of the raw body when HubRise sends none); an already-enriched redelivery
    // ACKs without re-running the pull.
    let callback_key = if callback.id.trim().is_empty() {
        uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, &body).to_string()
    } else {
        callback.id.clone()
    };
    if let Some(raw) = &raw {
        let raw_body: serde_json::Value =
            serde_json::from_slice(&body).unwrap_or_else(|_| serde_json::json!({}));
        match raw
            .upsert(
                &callback_key,
                &callback.resource_type,
                &callback.event_type,
                callback.location_id.as_deref(),
                &raw_body,
            )
            .await
        {
            Ok(state) if state.already_processed => {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({ "received": true, "status": "duplicate" })),
                )
                    .into_response();
            }
            Ok(_) => {}
            // Infra failure mirroring the receipt: 5xx so HubRise redelivers.
            Err(e) => {
                eprintln!("hubrise webhook: raw mirror failed (cb {callback_key}): {e}");
                return (StatusCode::INTERNAL_SERVER_ERROR, "failed to mirror callback")
                    .into_response();
            }
        }
    }

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
                    // The pull itself failed — ask HubRise to redeliver (mirror stays unprocessed).
                    eprintln!("hubrise webhook: pull failed (cb {}): {reason}", callback.id);
                    return (StatusCode::BAD_GATEWAY, "hubrise API pull failed").into_response();
                }
            };
            // Every branch reaching here is definitive — stamp the enrichment high-water mark.
            if let Some(raw) = &raw {
                if let Err(e) = raw.mark_processed(&callback_key).await {
                    eprintln!("hubrise webhook: mark_processed failed (cb {callback_key}): {e}");
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trips_within_the_window() {
        let now = 1_800_000_000;
        let s = make_state("secret", now);
        assert!(verify_state("secret", &s, now));
        assert!(verify_state("secret", &s, now + STATE_VALIDITY_SECS - 1));
    }

    #[test]
    fn state_expires_and_rejects_tampering() {
        let now = 1_800_000_000;
        let s = make_state("secret", now);
        assert!(!verify_state("secret", &s, now + STATE_VALIDITY_SECS + 1), "expired");
        assert!(!verify_state("other", &s, now), "wrong secret");
        assert!(!verify_state("secret", "1800000000.deadbeef", now), "forged sig");
        assert!(!verify_state("secret", "not-a-state", now), "malformed");
        // A timestamp from the future (beyond clock skew) is refused too.
        let future = make_state("secret", now + 3600);
        assert!(!verify_state("secret", &future, now));
    }

    #[test]
    fn urlencode_escapes_reserved_chars() {
        assert_eq!(urlencode("abc-123_.~"), "abc-123_.~");
        assert_eq!(urlencode("account[catalog.read]"), "account%5Bcatalog.read%5D");
        assert_eq!(urlencode("https://x/y?z=1"), "https%3A%2F%2Fx%2Fy%3Fz%3D1");
    }
}
