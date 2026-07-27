//! Role-as-path GraphQL endpoints (ADR-0006). The master schema is mounted under `/{role}/graphql`; the
//! role is parsed from the path and injected into the request context, where the generated per-field
//! `guard`/`visible` ACL bindings (see `acl` + `generated/acl.rs`) enforce it: unauthorized operations
//! are FORBIDDEN, and introspection only shows the fields/types the role can reach. `GET /{role}/graphql`
//! upgrades to GraphQL-over-WebSocket (subscriptions) when the request is a WS handshake and renders
//! GraphiQL otherwise; `POST` executes (introspection included — so `GET /{role}/voyager`, GraphQL
//! Voyager's interactive schema graph, sees that role's filtered schema).
//!
//! Free-tier caveat (subscriptions): the WebSocket — and the in-process event bus feeding it — lives
//! only while the app instance is warm; the uptimerobot ping keeps the free-tier instance from idling,
//! but a restart/redeploy still drops connections, so clients must resubscribe and re-sync via the
//! pull queries.

use std::sync::Arc;

use async_graphql::http::{GraphiQLSource, ALL_WEBSOCKET_PROTOCOLS};
use async_graphql_axum::{GraphQLProtocol, GraphQLRequest, GraphQLResponse, GraphQLWebSocket};
use axum::{
    extract::{ws::WebSocketUpgrade, FromRequestParts, Path, Request, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{any, get, post},
    Extension, Json, Router,
};
use infrastructure::SireneSyncWorker;

use crate::auth::AuthContext;

use super::acl::RequestRole;
use super::schema::CaptainSchema;

/// Mount `/{role}/graphql` for the seven roles (unknown role segments 404). Returns a `Router<()>` (the
/// schema is applied as state) so it can be merged into the main router.
pub fn graphql_routes(schema: CaptainSchema) -> Router {
    Router::new()
        .route("/{role}/graphql", get(graphql_get).post(graphql_handler))
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

/// Internal trigger for the inbound-events drain worker (ADR-20260720-015400) — same auth and
/// fire-and-forget semantics as the SIRENE trigger above. The webhook nudge is the primary wake
/// signal; this ping is the ops/backfill lever.
pub fn inbound_internal_routes(
    worker: Option<Arc<infrastructure::InboundEventsDrainWorker>>,
) -> Router {
    Router::new().route("/internal/inbound/drain", post(inbound_drain)).with_state(worker)
}

async fn inbound_drain(
    State(worker): State<Option<Arc<infrastructure::InboundEventsDrainWorker>>>,
    headers: HeaderMap,
) -> Response {
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
            "inbound drain worker not available (no database configured)",
        )
            .into_response();
    };
    tokio::spawn(async move {
        if let Some(summary) = worker.run_once().await {
            println!("inbound drain worker (ping-triggered): {summary:?}");
        }
    });
    (StatusCode::ACCEPTED, Json(serde_json::json!({ "status": "draining" }))).into_response()
}

async fn graphql_handler(
    State(schema): State<CaptainSchema>,
    Extension(auth): Extension<Arc<AuthContext>>,
    scopes: Option<Extension<crate::auth::ScopeResolver>>,
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
    // Transport envelope (ADR-20260720-015500): the anonymous session id (X-SESSION-ID — a present
    // but malformed value is a client bug, fail-visible 400) and the W3C trace context, injected
    // next to the Principal for the journal envelope + ownership scopes.
    let session = match crate::graphql::session::session_header(&headers) {
        Ok(s) => s,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid X-SESSION-ID (must be a UUID)").into_response(),
    };
    let trace = crate::graphql::session::trace_context(&headers);
    // Per-instance authorization (#144): resolve the verified Principal to a ReadScope ONCE here, not
    // per resolver and not per row. The bridge (auth `sub` -> domain id) is the only part that costs a
    // lookup; every membership test downstream is a primary-key hit. Injected into the GraphQL context
    // so the GENERATED resolvers can pass it into the read ports without any hand-written plumbing.
    // The resolver is always layered; without a database its bridges are empty and every caller
    // degrades to Public — no tenant rows. Fail closed by construction, not by a branch here.
    let scope = match scopes {
        Some(Extension(r)) => r.resolve(&principal).await,
        None => application::queries::ReadScope::Public,
    };
    let resp: GraphQLResponse = schema
        .execute(
            req.into_inner()
                .data(role)
                .data(principal)
                .data(session)
                .data(trace)
                .data(scope),
        )
        .await
        .into();
    resp.into_response()
}

/// `GET /{role}/graphql`: the GraphQL-over-WebSocket upgrade (subscriptions, `graphql-ws` /
/// `graphql-transport-ws`) when the request is a WS handshake; GraphiQL otherwise (its subscription
/// endpoint points back at this same URL, so subscriptions work in the IDE).
///
/// Auth on the WS leg (ADR-0047): browsers cannot set an `Authorization` header on a WebSocket, so the
/// token is taken from the `connection_init` payload (`{"Authorization": "Bearer …"}`, the graphql-ws
/// convention) with the upgrade request's headers as fallback for header-capable clients — then
/// verified by the SAME `AuthContext` as POST. The verified `RequestRole` + `Principal` are injected
/// into the connection data, so the generated per-field `guard`/`visible` ACL applies identically to
/// every operation on the socket; a failed verification rejects the connection at init.
async fn graphql_get(
    State(schema): State<CaptainSchema>,
    Extension(auth): Extension<Arc<AuthContext>>,
    Path(role_seg): Path<String>,
    req: Request,
) -> Response {
    let Some(role) = RequestRole::from_segment(&role_seg) else {
        return (StatusCode::NOT_FOUND, "unknown role path").into_response();
    };
    // Run the WS extractors by hand (neither implements axum's `OptionalFromRequestParts`, so they
    // can't be `Option<...>` handler params): both succeed only on a WS handshake carrying a GraphQL
    // subprotocol.
    let (mut parts, _body) = req.into_parts();
    let headers = parts.headers.clone();
    let protocol = GraphQLProtocol::from_request_parts(&mut parts, &()).await.ok();
    let upgrade = WebSocketUpgrade::from_request_parts(&mut parts, &()).await.ok();
    let (Some(upgrade), Some(protocol)) = (upgrade, protocol) else {
        // Not a WebSocket handshake → GraphiQL for this role.
        let endpoint = format!("/{}/graphql", role.segment());
        return Html(
            GraphiQLSource::build()
                .endpoint(&endpoint)
                .subscription_endpoint(&endpoint)
                .finish(),
        )
        .into_response();
    };
    upgrade.protocols(ALL_WEBSOCKET_PROTOCOLS).on_upgrade(move |stream| async move {
        GraphQLWebSocket::new(stream, schema, protocol)
            .on_connection_init(move |payload| async move {
                // Prefer the init-payload token (the only channel a browser has); fall back to the
                // upgrade request's headers so header-capable (server-side) clients keep working.
                let mut headers = headers;
                if let Some(token) = payload
                    .get("Authorization")
                    .or_else(|| payload.get("authorization"))
                    .and_then(|v| v.as_str())
                {
                    if let Ok(value) = token.parse() {
                        headers.insert(AUTHORIZATION, value);
                    }
                }
                // X-SESSION-ID rides the init payload too (browsers cannot set WS headers) — the
                // anonymous ownership scope of operationStatusChanged (ADR-20260720-015500).
                if let Some(session) = payload
                    .get("X-SESSION-ID")
                    .or_else(|| payload.get("x-session-id"))
                    .and_then(|v| v.as_str())
                {
                    if let Ok(value) = session.parse() {
                        headers.insert(crate::graphql::session::SESSION_HEADER, value);
                    }
                }
                let principal = auth.authorize(role, &headers).await.map_err(|e| {
                    async_graphql::Error::new(match e {
                        crate::auth::AuthError::Unauthorized => {
                            "unauthorized: valid bearer token required (connection_init payload `Authorization`)"
                        }
                        crate::auth::AuthError::Forbidden => {
                            "forbidden: token role not permitted for this path"
                        }
                        crate::auth::AuthError::Unavailable => "auth unavailable",
                    })
                })?;
                let mut data = async_graphql::Data::default();
                data.insert(role);
                data.insert(principal);
                // A malformed session id rejects the connection (fail-visible, like a bad token).
                let session = crate::graphql::session::session_header(&headers)
                    .map_err(|_| async_graphql::Error::new("invalid X-SESSION-ID (must be a UUID)"))?;
                data.insert(session);
                data.insert(crate::graphql::session::trace_context(&headers));
                Ok(data)
            })
            .serve()
            .await
    })
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
