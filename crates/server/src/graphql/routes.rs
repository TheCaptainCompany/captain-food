//! Role-as-path GraphQL endpoints (ADR-0006). The master schema is mounted under `/{role}/graphql`; the
//! role is parsed from the path and injected into the request context, where the generated per-field
//! `guard`/`visible` ACL bindings (see `acl` + `generated/acl.rs`) enforce it: unauthorized operations
//! are FORBIDDEN, and introspection only shows the fields/types the role can reach. `GET /{role}/graphql`
//! renders GraphiQL, `POST` executes (introspection included — so `GET /{role}/voyager`, GraphQL Voyager's
//! interactive schema graph, sees that role's filtered schema).

use std::sync::Arc;

use async_graphql::http::GraphiQLSource;
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{any, get, post},
    Extension, Json, Router,
};
use axum::body::Bytes;
use infrastructure::{
    verify_hubrise_signature, verify_stripe_signature, HubRiseCallback, SireneSyncWorker, StripeEvent,
    StripeIngestOutcome, StripeWebhookIngestor, HUBRISE_SIGNATURE_HEADER, HUBRISE_WEBHOOK_SECRET_ENV,
    STRIPE_WEBHOOK_SECRET_ENV,
};

use crate::auth::AuthContext;

use super::acl::RequestRole;
use super::schema::CaptainSchema;

/// Mount `/{role}/graphql` for the seven roles (unknown role segments 404). Returns a `Router<()>` (the
/// schema is applied as state) so it can be merged into the main router.
pub fn graphql_routes(schema: CaptainSchema) -> Router {
    Router::new()
        .route("/{role}/graphql", get(graphiql).post(graphql_handler))
        .route("/{role}/voyager", get(voyager))
        // Convenience: bare paths redirect to the PUBLIC role (307 preserves method/body for POST).
        .route("/graphql", any(|| async { Redirect::temporary("/public/graphql") }))
        .route("/voyager", any(|| async { Redirect::temporary("/public/voyager") }))
        .with_state(schema)
}

/// Internal trigger endpoints (ADR-0045) — NOT part of the GraphQL surface, mounted here alongside it.
/// `POST /internal/sirene/drain` wakes the SIRENE sync worker after a CI ingestion run: it spawns
/// `run_once` in the background (a France-wide first drain outlives any request timeout) and answers
/// `202 Accepted` immediately. Secured by a shared secret: the request must carry the
/// `x-internal-token` header matching the `INTERNAL_TRIGGER_TOKEN` env var — rejected when the env is
/// unset (503, fail closed) or the token mismatches (401).
pub fn sirene_internal_routes(worker: Option<Arc<SireneSyncWorker>>) -> Router {
    Router::new().route("/internal/sirene/drain", post(sirene_drain)).with_state(worker)
}

async fn sirene_drain(
    State(worker): State<Option<Arc<SireneSyncWorker>>>,
    headers: HeaderMap,
) -> Response {
    // Fail closed: without a configured secret there is no way to authenticate the ping.
    let expected = match std::env::var("INTERNAL_TRIGGER_TOKEN") {
        Ok(token) if !token.trim().is_empty() => token.trim().to_string(),
        _ => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "internal trigger not configured (INTERNAL_TRIGGER_TOKEN unset)",
            )
                .into_response()
        }
    };
    let presented = headers.get("x-internal-token").and_then(|v| v.to_str().ok());
    if presented != Some(expected.as_str()) {
        return (StatusCode::UNAUTHORIZED, "invalid or missing x-internal-token").into_response();
    }
    let Some(worker) = worker else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "sirene sync worker not available (no database configured)",
        )
            .into_response();
    };
    // Drain in the background; an already-running pass is fine (it will pick the same rows up).
    tokio::spawn(async move {
        match worker.run_once().await {
            Ok(summary) => println!("sirene sync worker (ping-triggered): {summary:?}"),
            Err(e) => eprintln!("sirene sync worker (ping-triggered): {e}"),
        }
    });
    (StatusCode::ACCEPTED, Json(serde_json::json!({ "status": "draining" }))).into_response()
}

/// Stripe webhook ingestion (INBOUND integration events, CLAUDE.md) — NOT part of the GraphQL surface,
/// mounted here alongside it like the SIRENE trigger. `POST /webhooks/stripe` receives Stripe's signed
/// deliveries; the ACL (`infrastructure::integrations::stripe`) verifies the `Stripe-Signature` header
/// against the RAW body and records `PaymentCaptured`/`PaymentFailed`/`PaymentRefunded` facts,
/// idempotently keyed on the Stripe event id (redeliveries are 200-ACKed no-ops).
pub fn stripe_webhook_routes(ingestor: Option<Arc<StripeWebhookIngestor>>) -> Router {
    Router::new().route("/webhooks/stripe", post(stripe_webhook)).with_state(ingestor)
}

/// HubRise callback ingress (ADR-20260718-145856) — NOT the GraphQL surface. `POST /webhooks/hubrise`
/// verifies the `X-HubRise-Hmac-SHA256` HMAC over the RAW body (fail-closed if the client secret is
/// unset) and parses the envelope. This is the verified ingress only: domain translation
/// (OAuth pull → `OfferStockUpdated` / `ImportCatalog`) is a deliberate follow-up. Stateless.
pub fn hubrise_webhook_routes() -> Router {
    Router::new().route("/webhooks/hubrise", post(hubrise_webhook))
}

async fn hubrise_webhook(headers: HeaderMap, body: Bytes) -> Response {
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
    // Verified ingress only. Domain enrichment (OAuth pull → OfferStockUpdated / ImportCatalog) is the
    // next chapter; acknowledge so HubRise stops redelivering.
    println!(
        "hubrise webhook: verified {}.{} (id {}){}",
        callback.resource_type,
        callback.event_type,
        callback.id,
        if callback.needs_pull() { " [needs API pull to enrich]" } else { "" }
    );
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "received": true, "status": "verified_pending_enrichment" })),
    )
        .into_response()
}

async fn stripe_webhook(
    State(ingestor): State<Option<Arc<StripeWebhookIngestor>>>,
    headers: HeaderMap,
    // RAW body bytes: the signature covers the exact bytes Stripe sent — a re-serialized JSON would
    // never verify. Parsing happens only AFTER the MAC checks out.
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
    if let Err(e) = verify_stripe_signature(&secret, signature, &body, now) {
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
        // All definitive outcomes ACK with 200 so Stripe stops redelivering; `unmappable` (verified
        // but missing our metadata) is logged — a retry would carry the same payload.
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

async fn graphql_handler(
    State(schema): State<CaptainSchema>,
    Extension(auth): Extension<Arc<AuthContext>>,
    Path(role_seg): Path<String>,
    headers: HeaderMap,
    req: GraphQLRequest,
) -> Response {
    let Some(role) = RequestRole::from_segment(&role_seg) else {
        return (StatusCode::NOT_FOUND, "unknown role path").into_response();
    };
    // Authn/authz at the path boundary (ADR-0047): /public is open; every other path needs a valid
    // Supabase JWT whose `captain_role` matches this path — so the role is now VERIFIED, not merely
    // self-asserted by the URL. On success we inject BOTH the RequestRole — read by the generated
    // guard/visible ACL bindings that enforce per-field authz + filter introspection (ADR-0006) — and the
    // verified Principal (identity for resolvers).
    let principal = match auth.authorize(role, &headers).await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    let resp: GraphQLResponse =
        schema.execute(req.into_inner().data(role).data(principal)).await.into();
    resp.into_response()
}

async fn graphiql(Path(role_seg): Path<String>) -> Response {
    match RequestRole::from_segment(&role_seg) {
        Some(role) => Html(
            GraphiQLSource::build()
                .endpoint(&format!("/{}/graphql", role.segment()))
                .finish(),
        )
        .into_response(),
        None => (StatusCode::NOT_FOUND, "unknown role path").into_response(),
    }
}

/// GraphQL Voyager — an interactive graph of the schema — introspecting this role's `/{role}/graphql`.
/// Loads Voyager from a CDN; it visualizes types/relationships (the FK-derived navigation shows as edges).
async fn voyager(Path(role_seg): Path<String>) -> Response {
    match RequestRole::from_segment(&role_seg) {
        Some(role) => {
            let endpoint = format!("/{}/graphql", role.segment());
            Html(VOYAGER_HTML.replace("__ENDPOINT__", &endpoint)).into_response()
        }
        None => (StatusCode::NOT_FOUND, "unknown role path").into_response(),
    }
}

/// Standalone GraphQL Voyager page (graphql-voyager v2). Loads the bundle from jsdelivr and drives
/// introspection against `__ENDPOINT__` (replaced per role). Served by our own origin (no CSP set).
const VOYAGER_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8" />
  <title>Captain.Food GraphQL — Voyager</title>
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/graphql-voyager@2.1.0/dist/voyager.css" />
  <style>html, body, #voyager { margin: 0; height: 100vh; overflow: hidden; }</style>
</head>
<body>
  <div id="voyager">Loading GraphQL Voyager…</div>
  <script src="https://cdn.jsdelivr.net/npm/graphql-voyager@2.1.0/dist/voyager.standalone.js"></script>
  <script type="module">
    // Matches the official graphql-voyager v2 CDN example: fetch introspection HERE and pass the RESULT
    // to renderVoyager. The standalone build expects introspection DATA, not a query-taking function
    // (the function form never fires the request — Voyager just stays on "Transmitting…").
    const { voyagerIntrospectionQuery: query } = GraphQLVoyager;
    const response = await fetch(window.location.origin + '__ENDPOINT__', {
      method: 'post',
      headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
      body: JSON.stringify({ query }),
      credentials: 'omit',
    });
    const introspection = await response.json();
    GraphQLVoyager.renderVoyager(document.getElementById('voyager'), { introspection });
  </script>
</body>
</html>
"#;
