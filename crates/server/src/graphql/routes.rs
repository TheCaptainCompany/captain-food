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
    http::{
        header::{AUTHORIZATION, CONTENT_SECURITY_POLICY, CONTENT_TYPE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{Html, IntoResponse, Redirect, Response},
    routing::{any, get, post},
    Extension, Json, Router,
};
use infrastructure::SireneSyncWorker;

use crate::auth::AuthContext;

use super::acl::RequestRole;
use super::schema::CaptainSchema;

/// What a GraphQL request is served from: the schema, plus the read the edge resolves this
/// request's `Host` through to a tenant (#469).
///
/// The lookup is a **required argument of [`graphql_routes`]**, not an `Extension` layered on
/// afterwards, deliberately (compiler-first, ADR-20260803-234035): a forgotten extension surfaces as
/// a runtime 500 — or, if it were made lenient, as a silently untenanted read, which is the exact
/// bug this change closes — whereas a parameter makes "mounted the GraphQL surface with no way to
/// resolve the tenant" fail to compile. Both mount sites (the monolith router and the
/// `graphql-{scope}` subgraph bins) already hold a `TenantLookup` from the same composition root.
#[derive(Clone)]
pub struct GraphqlState {
    schema: CaptainSchema,
    tenants: crate::hosts::TenantLookup,
    /// Where a CUSTOMER's domain identity comes from for THIS request (IDENT-1 Phase A,
    /// ADR-20260818-004646, #641) — resolved ONCE at startup/config-load and cloned into every
    /// request's state; `resolve_read_scope` never falls back per request.
    identity: crate::auth::CustomerIdentitySource,
}

/// Mount `/{role}/graphql` for the seven roles (unknown role segments 404). Returns a `Router<()>` (the
/// schema + tenant lookup are applied as state) so it can be merged into the main router.
///
/// `identity` is the gate-then-stabilize choice for CUSTOMER read-scope resolution (#641): pass
/// `CustomerIdentitySource::Claim` for the default (legacy) behaviour, or
/// `CustomerIdentitySource::Postgres(..)` once `RESOLVE_CUSTOMER_IDENTITY_FROM_POSTGRES` is set.
pub fn graphql_routes(
    schema: CaptainSchema,
    tenants: crate::hosts::TenantLookup,
    identity: crate::auth::CustomerIdentitySource,
) -> Router {
    Router::new()
        .route("/{role}/graphql", get(graphql_get).post(graphql_handler))
        .route("/{role}/voyager", get(voyager))
        // Vendored Voyager assets (#695): served same-origin, no role scoping needed (identical for
        // every role — only the introspection ENDPOINT the page fetches varies by role).
        .route("/voyager-assets/voyager.css", get(voyager_css))
        .route("/voyager-assets/voyager.standalone.js", get(voyager_standalone_js))
        .route("/voyager-assets/voyager-init.js", get(voyager_init_js))
        // Convenience: bare paths redirect to the PUBLIC role (307 preserves method/body for POST).
        .route("/graphql", any(|| async { Redirect::temporary("/public/graphql") }))
        .route("/voyager", any(|| async { Redirect::temporary("/public/voyager") }))
        .with_state(GraphqlState { schema, tenants, identity })
        .layer(axum::middleware::map_response(private_no_store))
}

/// Every response of the GraphQL surface is `Cache-Control: private, no-store` (#469, legal lens).
///
/// Since #469 a `/public/graphql` response VARIES by the `captain_auth` cookie — the same query on
/// the same host returns this customer's cart or nobody's. Nothing in the tree said so: a shared
/// cache (a future CDN/ingress rule, a corporate proxy, a service worker) that took a GraphQL POST
/// as cacheable could serve one customer's cart to another — GDPR Art. 32(1)(b) confidentiality,
/// and an Art. 33 notifiable breach when it happens. Today's safety rests on "nothing fronts this
/// with a cache" and "POSTs aren't cached by default", which are organisational assumptions about
/// deployments we have not made yet; this header is the technical measure that replaces them.
///
/// Applied as ONE response layer over the whole surface rather than at the handler's return
/// statements: a new route, or a new early return, cannot forget it.
async fn private_no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, no-store"),
    );
    response
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
    State(state): State<GraphqlState>,
    Extension(auth): Extension<Arc<AuthContext>>,
    Path(role_seg): Path<String>,
    headers: HeaderMap,
    req: GraphQLRequest,
) -> Response {
    let GraphqlState { schema, tenants, identity } = state;
    let Some(role) = RequestRole::from_segment(&role_seg) else {
        return (StatusCode::NOT_FOUND, "unknown role path").into_response();
    };
    // Authn/authz + per-instance authorization at the path boundary (ADR-0047, #144/#433/#641),
    // shared by this handler and the WS `connection_init` closure below — see
    // [`authorize_and_resolve_scope`] for what each step does and why it is one function.
    let (principal, correlation, scope) =
        match authorize_and_resolve_scope(&auth, role, &headers, &identity).await {
            Ok(t) => t,
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
    // The ONE request clock (RSO-1): every serviceWindow this request evaluates agrees on "now"
    // — minted here at the transport boundary, like the correlation id above, never per row.
    let request_now = crate::graphql::service_clock::RequestNow::mint();
    // The request's TENANT (#469), resolved ONCE here from the `Host` — its OWN datum beside the
    // ReadScope, never folded into it (different provenance: claims vs host, and legitimately
    // absent on the marketplace). Tenant-scoped reads take it from the context, so no operation can
    // accept the tenant as a client argument and none can forget to be bounded by it.
    let tenant = crate::graphql::tenant::resolve_tenant(&headers, &tenants).await;
    let resp: GraphQLResponse = schema
        .execute(
            req.into_inner()
                .data(role)
                .data(principal)
                .data(session)
                .data(trace)
                .data(correlation)
                .data(request_now)
                .data(scope)
                .data(tenant),
        )
        .await
        .into();
    resp.into_response()
}

/// Authn/authz + per-instance authorization at the path boundary (ADR-0047, #144/#433), shared by
/// [`graphql_handler`] (HTTP POST) and [`graphql_get`]'s WS `connection_init` closure — the ONE
/// place both transports resolve who the caller is and what they may read, so the two paths can
/// never drift (#641, IDENT-1 Phase A, ADR-20260818-004646: the socket must never widen what a
/// query would refuse).
///
/// Verifies the token for `path_role` — `/public` never refuses (see [`AuthContext::public_principal`]),
/// every other path fails with the mapped [`AuthError`] on an invalid/missing/wrong-role credential
/// — then resolves the caller's [`application::queries::ReadScope`] ONCE (`resolve_read_scope`)
/// under `customer_identity`: the default `CustomerIdentitySource::Claim`, or — once
/// `RESOLVE_CUSTOMER_IDENTITY_FROM_POSTGRES` is set — `CustomerIdentitySource::Postgres`, which
/// resolves a CUSTOMER caller through Postgres instead of trusting the JWT's `captain_food.customer_id`
/// claim.
async fn authorize_and_resolve_scope(
    auth: &AuthContext,
    path_role: RequestRole,
    headers: &HeaderMap,
    customer_identity: &crate::auth::CustomerIdentitySource,
) -> Result<
    (crate::auth::Principal, crate::graphql::session::RequestCorrelationId, application::queries::ReadScope),
    crate::auth::AuthError,
> {
    let principal = auth.authorize(path_role, headers).await?;
    // The ONE `request.correlation_id` of this request/connection (#451): minted here, at the
    // transport boundary, and shared by every read-path span it opens (`auth.read_scope`,
    // `cart.price` at the pricing seam). Reads carry no command envelope, so nothing upstream
    // supplies one — but it must be one PER REQUEST, not one per span, or it correlates nothing.
    let correlation = crate::graphql::session::RequestCorrelationId::mint();
    let scope = crate::auth::resolve_read_scope(&principal, correlation, customer_identity).await;
    Ok((principal, correlation, scope))
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
    State(state): State<GraphqlState>,
    Extension(auth): Extension<Arc<AuthContext>>,
    Path(role_seg): Path<String>,
    req: Request,
) -> Response {
    let GraphqlState { schema, tenants, identity } = state;
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
                // One correlation id per CONNECTION here (the socket is the request): every
                // read-path span served over it shares the id, same posture as POST. The socket
                // resolves its ReadScope ONCE at connection init, through the SAME
                // `authorize_and_resolve_scope` the POST path uses — a subscription must not
                // widen what a query would refuse (#144/#433), and Postgres-mode resolution (#641)
                // applies identically on both transports (the shared function IS the proof).
                let (principal, correlation, scope) =
                    authorize_and_resolve_scope(&auth, role, &headers, &identity)
                        .await
                        .map_err(|e| {
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
                data.insert(correlation);
                // Deliberately NO `RequestNow` here (RSO-1): a socket lives for hours, so a
                // connection-scoped clock would serve every later operation a stale "now". On
                // this transport "the request" is each operation, and `service_clock::evaluation`
                // reads the clock once per execution — the correct per-operation clock.
                data.insert(scope);
                // The socket's TENANT, resolved ONCE at init from the UPGRADE request's Host —
                // same posture as the ReadScope beside it (#469). A live cart or tracking socket
                // must not read wider than the POST that opened the page could.
                data.insert(crate::graphql::tenant::resolve_tenant(&headers, &tenants).await);
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
/// Vendored same-origin (#695, PROP-170500 D4): the bundle used to load from a CDN onto this
/// authenticated admin origin with no CSP; both defects are closed here.
async fn voyager(Path(role_seg): Path<String>) -> Response {
    match RequestRole::from_segment(&role_seg) {
        Some(role) => {
            let endpoint = format!("/{}/graphql", role.segment());
            let mut response = Html(VOYAGER_HTML.replace("__ENDPOINT__", &endpoint)).into_response();
            response
                .headers_mut()
                .insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(VOYAGER_CSP));
            response
        }
        None => (StatusCode::NOT_FOUND, "unknown role path").into_response(),
    }
}

/// The vendored `graphql-voyager@2.1.0` stylesheet, retrieved 2026-08-28 from
/// `https://cdn.jsdelivr.net/npm/graphql-voyager@2.1.0/dist/voyager.css`
/// (sha256 `88105ff1aac63f54d4bf647701247b0dba7f9cf6c1d7cb8c763ec0eb18a44a37`, computed at vendoring
/// time — there is no runtime re-verification of this hash; a residual, per the dispatch).
const VOYAGER_CSS: &str = include_str!("../../assets/voyager/voyager.css");

/// The vendored `graphql-voyager@2.1.0` standalone bundle, retrieved 2026-08-28 from
/// `https://cdn.jsdelivr.net/npm/graphql-voyager@2.1.0/dist/voyager.standalone.js`
/// (sha256 `03777306cecf12701d510a0dff3dfd737a4c395e911c23fcf92498e9c9b1fead`, same residual as above).
const VOYAGER_JS: &str = include_str!("../../assets/voyager/voyager.standalone.js");

/// First-party glue script (never vendored — it is OUR code): reads the role's GraphQL endpoint from
/// `#voyager`'s `data-endpoint` attribute (substituted server-side, same `__ENDPOINT__` mechanism as
/// before) and drives Voyager's introspection fetch. Moved out of an inline `<script type="module">`
/// into its own same-origin file so `script-src 'self'` needs no `'unsafe-inline'`/nonce/hash — the
/// simplest honest CSP for the one piece of markup we author ourselves.
const VOYAGER_INIT_JS: &str = r#"(async () => {
  // Matches the official graphql-voyager v2 example: fetch introspection HERE and pass the RESULT
  // to renderVoyager. The standalone build expects introspection DATA, not a query-taking function
  // (the function form never fires the request -- Voyager just stays on "Transmitting...").
  const { voyagerIntrospectionQuery: query } = GraphQLVoyager;
  const container = document.getElementById('voyager');
  const endpoint = container.dataset.endpoint;
  const response = await fetch(window.location.origin + endpoint, {
    method: 'post',
    headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
    body: JSON.stringify({ query }),
    credentials: 'omit',
  });
  const introspection = await response.json();
  GraphQLVoyager.renderVoyager(container, { introspection });
})();
"#;

/// CSP for the Voyager page and its three same-origin asset routes (#695). Permits only what the
/// vendored bundle actually does, each grant tied to a concrete need found by inspecting the bundle:
/// - `script-src 'self' 'wasm-unsafe-eval'`: no remote/inline script; `'wasm-unsafe-eval'` because the
///   bundle embeds an emscripten-compiled graphviz (`new WebAssembly.Instance(module, info)`, the
///   synchronous form CSP treats like `eval` for WASM) to lay out the graph.
/// - `style-src 'self' 'unsafe-inline'`: the bundle ships `styled-components` in "speedy" mode, which
///   injects `<style>` tags and calls `CSSStyleSheet.insertRule` at runtime with no nonce hook exposed
///   by the prebuilt standalone build; without `'unsafe-inline'` the interactive UI chrome (search,
///   docs panel) silently loses its layout. Bounded residual: style-src cannot execute script, so this
///   does not reopen the defect being closed here (arbitrary REMOTE code execution on an authenticated
///   origin) -- worst case is a CSS-based side channel, not RCE.
/// - `img-src 'self' data:`, `connect-src 'self' data:`: the graph is rendered to a `data:image/...`
///   URI, and the WASM layout path fetches its own embedded module from a `data:application/wasm` URI.
/// - `worker-src 'self' blob:`: graph layout runs in a `new Worker(...)` created from a `Blob`.
/// - `default-src 'none'`, `object-src 'none'`, `base-uri 'none'`, `frame-ancestors 'none'`: no other
///   capability the page needs, and no framing of this admin surface.
///
/// Residual, per the dispatch: this header is enforced by the BROWSER; the test below only proves it
/// is served, not that every browser enforces every directive identically.
const VOYAGER_CSP: &str = "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self' data:; worker-src 'self' blob:; base-uri 'none'; frame-ancestors 'none'; object-src 'none'";

fn static_asset_response(body: &'static str, content_type: &'static str) -> Response {
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .body(axum::body::Body::from(body))
        .expect("static asset response is well-formed");
    response
        .headers_mut()
        .insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(VOYAGER_CSP));
    response
}

async fn voyager_css() -> Response {
    static_asset_response(VOYAGER_CSS, "text/css; charset=utf-8")
}

async fn voyager_standalone_js() -> Response {
    static_asset_response(VOYAGER_JS, "text/javascript; charset=utf-8")
}

async fn voyager_init_js() -> Response {
    static_asset_response(VOYAGER_INIT_JS, "text/javascript; charset=utf-8")
}

/// Standalone GraphQL Voyager page (graphql-voyager v2), served entirely same-origin (#695): styles,
/// script bundle and the introspection-driving glue script all resolve under `/voyager-assets/*`, and
/// `Content-Security-Policy` (see [`VOYAGER_CSP`]) is set alongside this HTML. Drives introspection
/// against `__ENDPOINT__` (replaced per role) via the `data-endpoint` attribute below.
const VOYAGER_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8" />
  <title>Captain.Food GraphQL — Voyager</title>
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <link rel="stylesheet" href="/voyager-assets/voyager.css" />
  <style>html, body, #voyager { margin: 0; height: 100vh; overflow: hidden; }</style>
</head>
<body>
  <div id="voyager" data-endpoint="__ENDPOINT__">Loading GraphQL Voyager…</div>
  <script src="/voyager-assets/voyager.standalone.js"></script>
  <script src="/voyager-assets/voyager-init.js"></script>
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

    /// #469, legal lens: a `/public/graphql` response now varies by the `captain_auth` cookie, so
    /// it must never be stored by a shared cache. Asserted on the REAL router (the layer, not a
    /// handler's return statement), and on the anonymous request too — a response that carries no
    /// cart today is served from the same URL as one that does, and a cache does not re-read the
    /// policy per request.
    #[tokio::test]
    async fn every_graphql_response_forbids_shared_caching() {
        use tower::ServiceExt;
        let router = graphql_routes(
            crate::graphql::schema::build_schema(None, None, None),
            crate::hosts::TenantLookup(None),
            crate::auth::CustomerIdentitySource::Claim,
        )
        .layer(Extension(crate::auth::AuthContext::from_config(
            String::new(),
            String::new(),
        )));
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/public/graphql")
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(json!({ "query": "{ __typename }" }).to_string()))
            .expect("request builds");
        let response = router.oneshot(request).await.expect("router answers");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(axum::http::header::CACHE_CONTROL).map(|v| v.to_str().unwrap()),
            Some("private, no-store"),
            "a credential-varying response must not be storable by a shared cache"
        );
    }

    fn voyager_test_router() -> Router {
        graphql_routes(
            crate::graphql::schema::build_schema(None, None, None),
            crate::hosts::TenantLookup(None),
            crate::auth::CustomerIdentitySource::Claim,
        )
        .layer(Extension(crate::auth::AuthContext::from_config(
            String::new(),
            String::new(),
        )))
    }

    /// #695 (PROP-170500 D4): the Voyager page must be entirely same-origin — no `cdn.jsdelivr.net`,
    /// no `https://` reference of any kind — must reference the vendored same-origin asset paths, and
    /// must carry the CSP header. The green control (c) keeps the `__ENDPOINT__` role-substitution
    /// behaviour asserted in the same test, so a fix for (a)/(b) cannot silently break (c).
    #[tokio::test]
    async fn voyager_page_is_same_origin_with_csp_and_endpoint_wiring() {
        use tower::ServiceExt;
        let router = voyager_test_router();
        let request = axum::http::Request::builder()
            .method("GET")
            .uri("/restaurant/voyager")
            .body(axum::body::Body::empty())
            .expect("request builds");
        let response = router.oneshot(request).await.expect("router answers");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_SECURITY_POLICY).map(|v| v.to_str().unwrap()),
            Some(VOYAGER_CSP),
            "the voyager page must carry the CSP header"
        );
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.expect("body");
        let body = String::from_utf8(bytes.to_vec()).expect("utf8 body");
        assert!(
            !body.contains("cdn.jsdelivr.net"),
            "the CDN must be gone from the served page: {body}"
        );
        assert!(
            !body.contains("https://"),
            "no remote asset reference of any kind may remain in the page: {body}"
        );
        assert!(
            body.contains("/voyager-assets/voyager.css") &&
                body.contains("/voyager-assets/voyager.standalone.js") &&
                body.contains("/voyager-assets/voyager-init.js"),
            "the page must reference the same-origin vendored/first-party asset paths: {body}"
        );
        // (c) the green control: role-specific endpoint wiring survives the same-origin rewrite.
        assert!(
            body.contains(r#"data-endpoint="/restaurant/graphql""#),
            "the role's GraphQL endpoint must still be wired into the page: {body}"
        );
    }

    /// #695: each vendored/first-party asset route serves 200 with the right content type, and (as a
    /// consequence of them existing at all) the HTML's references to them are not dangling.
    #[tokio::test]
    async fn voyager_asset_routes_serve_200_with_content_type_and_csp() {
        use tower::ServiceExt;
        for (path, expected_content_type) in [
            ("/voyager-assets/voyager.css", "text/css; charset=utf-8"),
            ("/voyager-assets/voyager.standalone.js", "text/javascript; charset=utf-8"),
            ("/voyager-assets/voyager-init.js", "text/javascript; charset=utf-8"),
        ] {
            let router = voyager_test_router();
            let request = axum::http::Request::builder()
                .method("GET")
                .uri(path)
                .body(axum::body::Body::empty())
                .expect("request builds");
            let response = router.oneshot(request).await.expect("router answers");
            assert_eq!(response.status(), StatusCode::OK, "{path} must serve 200");
            assert_eq!(
                response.headers().get(CONTENT_TYPE).map(|v| v.to_str().unwrap()),
                Some(expected_content_type),
                "{path} must serve the correct content type"
            );
            assert_eq!(
                response.headers().get(CONTENT_SECURITY_POLICY).map(|v| v.to_str().unwrap()),
                Some(VOYAGER_CSP),
                "{path} must also carry the CSP header (dispatch scope: voyager route AND asset routes)"
            );
        }
    }

    // -----------------------------------------------------------------------------------------
    // IDENT-1 Phase A (#641, ADR-20260818-004646): `authorize_and_resolve_scope` is the ONE
    // function both `graphql_handler` (HTTP POST) and `graphql_get`'s WS `connection_init`
    // closure call — testing it here covers BOTH call sites' identity-resolution logic; the WS
    // closure's ONLY additional work is the already independently-tested `ws_auth_headers` merge
    // before it. A signed JWT is required to reach it (`Principal`'s constructors are
    // module-private), so the key material is duplicated from `crate::auth`'s own suite — the
    // established `#[cfg(test)]`-is-crate-invisible-to-integration-tests reason does not apply
    // here (this IS the same crate), but auth's signing helpers sit in a SIBLING test module with
    // no shared visibility, so duplicating the ~10 lines is still the cheapest correct path.
    // -----------------------------------------------------------------------------------------

    const WS_TEST_EC_PRIVATE_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgTCTNdGfegiVKVsm+
vZXPOa4xJAt5OT8zMSblfCEwtW2hRANCAARto0Dk75fxl2IyLx89vwvjUWkJAb/p
5bKnk8sNetDUBHLVIGpXoxBRFJVNSeDN6QB9IHl6rqDLaZR4iqLatScL
-----END PRIVATE KEY-----
";
    const WS_TEST_SUPABASE_URL: &str = "https://captain-under-test.supabase.co";

    async fn ws_jwks_endpoint() -> String {
        let body = json!({"keys":[{"kty":"EC","crv":"P-256","use":"sig","kid":"captain-test-es256",
            "alg":"ES256","x":"baNA5O-X8ZdiMi8fPb8L41FpCQG_6eWyp5PLDXrQ1AQ",
            "y":"ctUgalejEFEUlU1J4M3pAH0geXquoMtplHiKotq1Jws"}]});
        let app = Router::new().route(
            "/jwks",
            get(move || {
                let body = body.clone();
                async move { axum::Json(body) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{addr}/jwks")
    }

    fn ws_customer_jwt(sub: uuid::Uuid, claim_customer_id: uuid::Uuid) -> String {
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::ES256);
        header.kid = Some("captain-test-es256".into());
        let exp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_secs()
            + 3600;
        let claims = json!({
            "sub": sub.to_string(),
            "aud": "authenticated",
            "iss": format!("{WS_TEST_SUPABASE_URL}/auth/v1"),
            "exp": exp,
            "app_metadata": { "captain_food": { "role": "CUSTOMER", "customer_id": claim_customer_id.to_string() } },
        });
        let key = jsonwebtoken::EncodingKey::from_ec_pem(WS_TEST_EC_PRIVATE_KEY_PEM.as_bytes())
            .expect("test EC key parses");
        jsonwebtoken::encode(&header, &claims, &key).expect("sign test JWT")
    }

    struct WsScriptedResolver(crate::auth::CustomerIdentityResolution);

    #[async_trait::async_trait]
    impl crate::auth::ResolveCustomerIdentity for WsScriptedResolver {
        async fn resolve(&self, _auth_ref: &str) -> crate::auth::CustomerIdentityResolution {
            self.0.clone()
        }
    }

    /// (e) WS connect path covered: the SAME `authorize_and_resolve_scope` the WS `connection_init`
    /// closure calls resolves a CUSTOMER through Postgres — the token's claim carries a
    /// deliberately WRONG id, the seam answers the RIGHT one, and the resolved scope carries the
    /// RIGHT one. Proves the Postgres arm is reachable from BOTH transports' shared entry point,
    /// not re-derived per transport.
    #[tokio::test]
    async fn ws_connection_init_resolves_customer_through_postgres_not_the_claim() {
        use domain::generated::scalars::CustomerId;

        let auth = crate::auth::AuthContext::from_config(
            ws_jwks_endpoint().await,
            WS_TEST_SUPABASE_URL.into(),
        );
        let sub = uuid::Uuid::from_u128(0x437);
        let wrong_claim = uuid::Uuid::from_u128(0xBAD);
        let right_id = uuid::Uuid::from_u128(0x600D);
        let jwt = ws_customer_jwt(sub, wrong_claim);
        // The WS transport's own header shape (`ws_auth_headers`): the connection_init payload
        // carries the bearer token, exactly as `graphql_get`'s closure builds it before calling
        // `authorize_and_resolve_scope`.
        let headers = ws_auth_headers(
            HeaderMap::new(),
            &json!({ "Authorization": format!("Bearer {jwt}") }),
        );
        let identity = crate::auth::CustomerIdentitySource::Postgres(Arc::new(WsScriptedResolver(
            crate::auth::CustomerIdentityResolution::Resolved(CustomerId(right_id)),
        )));
        let (_, _, scope) =
            authorize_and_resolve_scope(&auth, RequestRole::Customer, &headers, &identity)
                .await
                .expect("a well-formed CUSTOMER token authorizes");
        assert_eq!(
            scope,
            application::queries::ReadScope::Customer(CustomerId(right_id)),
            "the WS connect path must resolve through Postgres, not the claim"
        );
        assert_ne!(
            scope,
            application::queries::ReadScope::Customer(CustomerId(wrong_claim)),
            "and never the claim's id"
        );
    }
}
