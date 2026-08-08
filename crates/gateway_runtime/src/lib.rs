//! The `gateway-{role}` runtime (#385 API-tier wiring, PROP-20260807-174246 D8).
//!
//! A role gateway serves `/{role}/graphql` for exactly ONE role path and routes each request's
//! TOP-LEVEL fields to the owning subgraph, resolved from the codegen-emitted composition table
//! its generated main embeds (`OPERATION_SCOPES` rows — the same derivation the subgraphs' scope
//! slice enforces, so gateway routing and subgraph acceptance cannot disagree). No database, no
//! business logic, no state, NO AUTH: the subgraph is the schema boundary — it verifies the JWT
//! and applies the per-field ACL exactly as the monolith does; the gateway forwards credentials
//! untouched. Static stitching: no runtime discovery, no query planner.
//!
//! ROUTING RULE (v0, honest): all non-introspection top-level fields of a document must belong
//! to ONE scope — the SDUI clients' generated documents are single-field by construction, so
//! this covers the product's real traffic. A cross-scope document is answered 400 with the field
//! → scope breakdown (split-and-merge is the recorded follow-up on #385; composition of
//! cross-scope DATA happens in the projector, never at query time — D8's cheapness argument).
//! Introspection-only documents route to the kernel subgraph (`graphql-common`), which serves
//! the full role-filtered schema shape like any subgraph.
//!
//! RECORDED GAP: GraphQL-over-WebSocket (subscriptions) is not proxied — a WS handshake is
//! answered 501. Push subscriptions in the bin topology already await the cross-process fan-out
//! follow-up (#385); poll reads are unaffected.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{any, post},
    Router,
};

use async_graphql::parser::types::{DocumentOperations, ExecutableDocument, OperationType, Selection};

/// The composition data one generated gateway main embeds.
#[derive(Clone)]
pub struct GatewayTable {
    /// The ONE role path segment this gateway serves (e.g. `"customer"`, `"restaurant-account"`).
    pub role_segment: &'static str,
    /// `(kind, field, owning scope)` — the generated `OPERATION_SCOPES` rows.
    pub fields: &'static [(&'static str, &'static str, &'static str)],
    /// Every subgraph scope (service `graphql-{scope}` must exist for each).
    pub scopes: &'static [&'static str],
    /// The kernel scope — the introspection default (`"common"`).
    pub kernel_scope: &'static str,
}

/// One resolved gateway: table + subgraph base URLs + HTTP client.
#[derive(Clone)]
pub struct Gateway {
    table: GatewayTable,
    /// `scope → base url` (no trailing slash), e.g. `ordering → http://graphql-ordering`.
    urls: std::collections::HashMap<&'static str, String>,
    client: reqwest::Client,
}

impl Gateway {
    /// Resolve subgraph addresses: the in-cluster default is the generated Service DNS name
    /// (`http://graphql-{scope}`, port 80 → the pod's $PORT); `overrides` is the declared
    /// `GATEWAY_SUBGRAPH_URLS` value — comma-separated `scope=url` pairs for local/off-cluster
    /// runs. An override naming an unknown scope is a LOUD startup warning, never a silent typo.
    pub fn new(table: GatewayTable, overrides: &str) -> Self {
        let mut urls: std::collections::HashMap<&'static str, String> = table
            .scopes
            .iter()
            .map(|s| (*s, format!("http://graphql-{s}")))
            .collect();
        for pair in overrides.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match pair.split_once('=') {
                Some((scope, url)) if !url.trim().is_empty() => {
                    match table.scopes.iter().find(|s| **s == scope.trim()) {
                        Some(key) => {
                            urls.insert(*key, url.trim().trim_end_matches('/').to_string());
                        }
                        None => tracing::warn!(
                            scope = scope.trim(),
                            "GATEWAY_SUBGRAPH_URLS names an unknown scope -- override ignored"
                        ),
                    }
                }
                _ => tracing::warn!(pair, "GATEWAY_SUBGRAPH_URLS entry is not 'scope=url' -- ignored"),
            }
        }
        Self {
            table,
            urls,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
        }
    }

    /// The resolved `scope → url` map (health detail).
    pub fn urls(&self) -> &std::collections::HashMap<&'static str, String> {
        &self.urls
    }

    /// The axum app: `/{role}/graphql` (POST executes via the owning subgraph; GET serves
    /// GraphiQL, WS handshakes are 501 — see the module docs). Unknown role segments 404, and so
    /// does every OTHER role: one gateway, one role path (the deploy topology routes each
    /// `/{role}/graphql` host path to its own gateway Service).
    pub fn app(self) -> Router {
        Router::new()
            .route("/{role}/graphql", post(proxy_handler).get(get_handler))
            .route(
                "/graphql",
                any(|State(gw): State<Gateway>| async move {
                    axum::response::Redirect::temporary(&format!("/{}/graphql", gw.table.role_segment))
                }),
            )
            .with_state(self)
    }
}

/// Which subgraph one document routes to: `Ok(scope)` or the loud routing error body.
fn route_document(table: &GatewayTable, query: &str) -> Result<&'static str, (StatusCode, String)> {
    let doc = async_graphql::parser::parse_query(query).map_err(|e| {
        (StatusCode::BAD_REQUEST, format!("GraphQL parse error: {e}"))
    })?;
    let mut scopes: Vec<(&'static str, String)> = Vec::new(); // (scope, field) — first hit per scope
    for (kind, fields) in root_fields(&doc) {
        for field in fields {
            if field.starts_with("__") {
                continue; // introspection — any subgraph answers the role-filtered shape
            }
            match table.fields.iter().find(|(k, f, _)| *k == kind && *f == field) {
                Some((_, _, scope)) => {
                    if !scopes.iter().any(|(s, _)| s == scope) {
                        scopes.push((scope, field.clone()));
                    }
                }
                None => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        format!("unknown top-level field '{field}' — not in the composition table"),
                    ))
                }
            }
        }
    }
    match scopes.len() {
        0 => Ok(table.kernel_scope), // introspection-only
        1 => Ok(scopes[0].0),
        _ => Err((
            StatusCode::BAD_REQUEST,
            format!(
                "cross-scope document: {} — one scope per request (composition happens in the projector, not the query; D8)",
                scopes
                    .iter()
                    .map(|(s, f)| format!("'{f}' → {s}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

/// The ROOT field names of every operation in the document, with the operation kind — fragment
/// spreads and inline fragments at the root are resolved (depth-bounded; cycles are a validation
/// error downstream). Shared with the subgraphs' scope slice, so routing and acceptance walk the
/// document identically.
pub fn root_fields(doc: &ExecutableDocument) -> Vec<(&'static str, Vec<String>)> {
    let mut out = Vec::new();
    let mut walk_op = |op: &async_graphql::Positioned<async_graphql::parser::types::OperationDefinition>| {
        let kind = match op.node.ty {
            OperationType::Query => "query",
            OperationType::Mutation => "mutation",
            OperationType::Subscription => "subscription",
        };
        let mut fields = Vec::new();
        collect(&op.node.selection_set.node, doc, &mut fields, 0);
        out.push((kind, fields));
    };
    match &doc.operations {
        DocumentOperations::Single(op) => walk_op(op),
        DocumentOperations::Multiple(map) => {
            for (_name, op) in map.iter() {
                walk_op(op);
            }
        }
    }
    out
}

fn collect(
    set: &async_graphql::parser::types::SelectionSet,
    doc: &ExecutableDocument,
    out: &mut Vec<String>,
    depth: usize,
) {
    if depth > 16 {
        return;
    }
    for sel in &set.items {
        match &sel.node {
            Selection::Field(f) => out.push(f.node.name.node.to_string()),
            Selection::FragmentSpread(spread) => {
                if let Some(frag) = doc.fragments.get(&spread.node.fragment_name.node) {
                    collect(&frag.node.selection_set.node, doc, out, depth + 1);
                }
            }
            Selection::InlineFragment(inline) => {
                collect(&inline.node.selection_set.node, doc, out, depth + 1);
            }
        }
    }
}

/// Headers forwarded to the subgraph — credentials and correlation, nothing else. The gateway
/// never reads them (no auth here — the subgraph is the schema boundary). `cookie` carries the
/// httpOnly `captain_auth` token, the ONLY credential a browser client has (crates/web/src/auth.rs
/// never sees the token, so it cannot send `authorization`); dropping it 401s every
/// authenticated browser request at the subgraph.
const FORWARDED_HEADERS: &[&str] = &[
    "authorization",
    "cookie",
    "x-external-api-key",
    "x-session-id",
    "traceparent",
    "tracestate",
    "accept-language",
];

async fn proxy_handler(
    State(gw): State<Gateway>,
    Path(role_seg): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if role_seg != gw.table.role_segment {
        return (StatusCode::NOT_FOUND, "unknown role path").into_response();
    }
    // Single-request envelope only (parity: the monolith's extractor takes one request; the SDUI
    // clients send one operation per HTTP request by construction).
    let envelope: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v @ serde_json::Value::Object(_)) => v,
        Ok(_) => {
            return (StatusCode::BAD_REQUEST, "batch requests are not supported").into_response()
        }
        Err(e) => return (StatusCode::BAD_REQUEST, format!("invalid JSON body: {e}")).into_response(),
    };
    let Some(query) = envelope.get("query").and_then(|q| q.as_str()) else {
        return (StatusCode::BAD_REQUEST, "missing 'query'").into_response();
    };
    let scope = match route_document(&gw.table, query) {
        Ok(scope) => scope,
        Err((code, msg)) => {
            tracing::warn!(role = gw.table.role_segment, error = %msg, "gateway routing rejected the document");
            // GraphQL-shaped error body so clients surface the message like any transport error.
            return (
                code,
                axum::Json(serde_json::json!({ "errors": [{ "message": msg }] })),
            )
                .into_response();
        }
    };
    let url = format!(
        "{}/{}/graphql",
        gw.urls.get(scope).map(String::as_str).unwrap_or_default(),
        gw.table.role_segment
    );
    let mut req = gw.client.post(&url).json(&envelope);
    for name in FORWARDED_HEADERS {
        if let Some(v) = headers.get(*name) {
            req = req.header(*name, v);
        }
    }
    match req.send().await {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status().as_u16())
                .unwrap_or(StatusCode::BAD_GATEWAY);
            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/json")
                .to_string();
            match resp.bytes().await {
                Ok(bytes) => (
                    status,
                    [(axum::http::header::CONTENT_TYPE, content_type)],
                    bytes,
                )
                    .into_response(),
                Err(e) => subgraph_unreachable(scope, &url, &e.to_string()),
            }
        }
        Err(e) => subgraph_unreachable(scope, &url, &e.to_string()),
    }
}

/// 502 with the owning subgraph named — an operator reading the body knows WHICH pod to look at.
fn subgraph_unreachable(scope: &str, url: &str, error: &str) -> Response {
    tracing::error!(scope, url, error, "subgraph unreachable");
    (
        StatusCode::BAD_GATEWAY,
        axum::Json(serde_json::json!({
            "errors": [{ "message": format!("subgraph '{scope}' unreachable: {error}") }]
        })),
    )
        .into_response()
}

async fn get_handler(
    State(gw): State<Gateway>,
    Path(role_seg): Path<String>,
    headers: HeaderMap,
) -> Response {
    if role_seg != gw.table.role_segment {
        return (StatusCode::NOT_FOUND, "unknown role path").into_response();
    }
    let is_ws = headers
        .get("upgrade")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    if is_ws {
        // Recorded #385 gap: WS proxying rides the cross-process push fan-out follow-up.
        return (
            StatusCode::NOT_IMPLEMENTED,
            "GraphQL-over-WebSocket is not proxied by the role gateway yet (#385) — use POST; poll reads are authoritative",
        )
            .into_response();
    }
    let endpoint = format!("/{}/graphql", gw.table.role_segment);
    Html(
        async_graphql::http::GraphiQLSource::build()
            .endpoint(&endpoint)
            .finish(),
    )
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: GatewayTable = GatewayTable {
        role_segment: "public",
        fields: &[
            ("query", "restaurants", "network"),
            ("query", "carts", "ordering"),
            ("mutation", "placeOrder", "ordering"),
            ("query", "operationStatus", "common"),
        ],
        scopes: &["network", "ordering", "common"],
        kernel_scope: "common",
    };

    #[test]
    fn single_scope_documents_route_to_the_owner() {
        assert_eq!(route_document(&TABLE, "query { restaurants { id } }").unwrap(), "network");
        assert_eq!(
            route_document(&TABLE, "mutation { placeOrder(input: {}) { messageId } }").unwrap(),
            "ordering"
        );
    }

    #[test]
    fn introspection_only_routes_to_the_kernel() {
        assert_eq!(
            route_document(&TABLE, "query { __schema { queryType { name } } }").unwrap(),
            "common"
        );
    }

    #[test]
    fn cross_scope_documents_are_rejected_with_the_breakdown() {
        let (code, msg) =
            route_document(&TABLE, "query { restaurants { id } carts { id } }").unwrap_err();
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert!(msg.contains("'restaurants' → network"), "{msg}");
        assert!(msg.contains("'carts' → ordering"), "{msg}");
    }

    #[test]
    fn unknown_fields_are_rejected_loudly() {
        let (_, msg) = route_document(&TABLE, "query { nope { id } }").unwrap_err();
        assert!(msg.contains("composition table"), "{msg}");
    }

    #[test]
    fn root_fragments_are_resolved_for_routing() {
        assert_eq!(
            route_document(&TABLE, "query { ...f } fragment f on Query { carts { id } }").unwrap(),
            "ordering"
        );
    }

    #[test]
    fn overrides_replace_defaults_and_unknown_scopes_are_ignored() {
        let gw = Gateway::new(TABLE, "ordering=http://127.0.0.1:7101, bogus=http://x");
        assert_eq!(gw.urls()["ordering"], "http://127.0.0.1:7101");
        assert_eq!(gw.urls()["network"], "http://graphql-network");
        assert!(!gw.urls().contains_key("bogus"));
    }

    #[test]
    fn both_auth_carriers_are_forwarded() {
        // The browser's ONLY credential is the httpOnly captain_auth cookie; wasm clients cannot
        // send `authorization`. Dropping either carrier 401s a whole client class at the subgraph.
        assert!(FORWARDED_HEADERS.contains(&"authorization"));
        assert!(FORWARDED_HEADERS.contains(&"cookie"));
    }
}
