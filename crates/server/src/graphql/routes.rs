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
            Ok(summary) => tracing::info!(worker = "sirene_sync", trigger = "ping", summary = ?summary, "drain pass complete"),
            Err(e) => tracing::error!(worker = "sirene_sync", trigger = "ping", error = %e, "drain pass failed"),
        }
    });
    (StatusCode::ACCEPTED, Json(serde_json::json!({ "status": "draining" }))).into_response()
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
    // Transport envelope (ADR-20260720-015500): the anonymous session id (X-SESSION-ID — a present
    // but malformed value is a client bug, fail-visible 400) and the W3C trace context, injected
    // next to the Principal for the journal envelope + ownership scopes.
    let session = match crate::graphql::session::session_header(&headers) {
        Ok(s) => s,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid X-SESSION-ID (must be a UUID)").into_response(),
    };
    let trace = crate::graphql::session::trace_context(&headers);
    // The ONE `request.correlation_id` of this request (#451): minted here, at the transport
    // boundary, and shared by every read-path span the request opens (`auth.read_scope` below,
    // `cart.price` at the pricing seam). Reads carry no command envelope, so nothing upstream
    // supplies one — but it must be one PER REQUEST, not one per span, or it correlates nothing.
    let correlation = crate::graphql::session::RequestCorrelationId::mint();
    // Per-instance authorization (#144/#433): resolve the verified Principal to a ReadScope ONCE
    // here — a PURE function of the token's claims (CARD-11), no lookup, no dependency that could
    // be missing. Injected into the GraphQL context so the GENERATED resolvers pass it into the
    // read ports without hand-written plumbing; a missing claim fails closed inside read_scope.
    let scope = crate::auth::resolve_read_scope(&principal, correlation);
    let resp: GraphQLResponse = schema
        .execute(
            req.into_inner()
                .data(role)
                .data(principal)
                .data(session)
                .data(trace)
                .data(correlation)
                .data(scope),
        )
        .await
        .into();
    resp.into_response()
}

/// The effective auth headers for a WS connection, PURE (#437 makes this composition load-bearing:
/// the storefront's live order tracking authenticates through it). Start from the UPGRADE request's
/// headers — for a same-origin browser socket these carry the httpOnly `captain_auth` cookie, the
/// storefront customer's ONLY credential (PROP-20260724-150500: "no `connection_init` token needed")
/// — then overlay the init payload: an `Authorization` token there wins (the channel for
/// header-incapable-but-token-holding clients), and `X-SESSION-ID` rides the payload too (browsers
/// cannot set WS headers — anonymous ownership scope, ADR-20260720-015500). No payload token means
/// the upgrade headers pass through UNTOUCHED, cookie included.
fn ws_auth_headers(mut headers: HeaderMap, payload: &serde_json::Value) -> HeaderMap {
    if let Some(token) = payload
        .get("Authorization")
        .or_else(|| payload.get("authorization"))
        .and_then(|v| v.as_str())
    {
        if let Ok(value) = token.parse() {
            headers.insert(AUTHORIZATION, value);
        }
    }
    if let Some(session) = payload
        .get("X-SESSION-ID")
        .or_else(|| payload.get("x-session-id"))
        .and_then(|v| v.as_str())
    {
        if let Ok(value) = session.parse() {
            headers.insert(crate::graphql::session::SESSION_HEADER, value);
        }
    }
    headers
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
                let headers = ws_auth_headers(headers, &payload);
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
                // One correlation id per CONNECTION here (the socket is the request): every
                // read-path span served over it shares the id, same posture as POST.
                let correlation = crate::graphql::session::RequestCorrelationId::mint();
                data.insert(correlation);
                // The socket resolves its ReadScope ONCE at connection init, from the same pure
                // claims function the POST path uses — a subscription must not widen what a query
                // would refuse (#144/#433).
                data.insert(crate::auth::resolve_read_scope(&principal, correlation));
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn upgrade_headers_with_cookie() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::COOKIE,
            "captain_auth=jwt.from.cookie; other=1".parse().unwrap(),
        );
        h
    }

    /// The storefront path (#437 / PROP-20260724-150500): the browser sends NO init-payload token —
    /// its only credential is the httpOnly `captain_auth` cookie on the upgrade request — so the
    /// merge must pass the upgrade headers through UNTOUCHED (cookie included, no Authorization
    /// key materialized). This is the composition `AuthContext::token()`'s cookie fallback relies on.
    #[test]
    fn no_payload_token_keeps_upgrade_cookie_untouched() {
        let out = ws_auth_headers(
            upgrade_headers_with_cookie(),
            &json!({ "X-SESSION-ID": "00000000-0000-7000-8000-000000000112" }),
        );
        assert!(out.get(AUTHORIZATION).is_none(), "no payload token -> no Authorization key at all");
        assert_eq!(
            out.get(axum::http::header::COOKIE).unwrap().to_str().unwrap(),
            "captain_auth=jwt.from.cookie; other=1",
            "the httpOnly cookie -- the storefront customer's only credential -- must survive"
        );
        assert_eq!(
            out.get(crate::graphql::session::SESSION_HEADER).unwrap().to_str().unwrap(),
            "00000000-0000-7000-8000-000000000112",
            "X-SESSION-ID rides the payload (browsers cannot set WS headers)"
        );
    }

    /// A payload `Authorization` wins over whatever the upgrade carried (the deliberate-override
    /// rule, same as HTTP where header beats cookie), lowercase key accepted per graphql-ws practice.
    #[test]
    fn payload_token_overrides_and_lowercase_is_accepted() {
        let mut upgrade = upgrade_headers_with_cookie();
        upgrade.insert(AUTHORIZATION, "Bearer stale.upgrade.token".parse().unwrap());
        let out = ws_auth_headers(upgrade, &json!({ "Authorization": "Bearer from.payload" }));
        assert_eq!(out.get(AUTHORIZATION).unwrap().to_str().unwrap(), "Bearer from.payload");
        let out = ws_auth_headers(HeaderMap::new(), &json!({ "authorization": "Bearer lower.case" }));
        assert_eq!(out.get(AUTHORIZATION).unwrap().to_str().unwrap(), "Bearer lower.case");
    }

    /// An empty payload against empty upgrade headers stays empty — the anonymous PUBLIC socket
    /// gains no phantom credentials from the merge itself.
    #[test]
    fn empty_payload_and_headers_stay_empty() {
        let out = ws_auth_headers(HeaderMap::new(), &json!({}));
        assert!(out.get(AUTHORIZATION).is_none());
        assert!(out.get(crate::graphql::session::SESSION_HEADER).is_none());
    }
}
