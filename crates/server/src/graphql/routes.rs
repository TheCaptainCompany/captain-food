//! Role-as-path GraphQL endpoints (ADR-0006). The master schema is mounted under `/{role}/graphql`; the
//! role is parsed from the path and injected into the request context (the ACL seam — the runtime guard
//! that filters `@auth` fields per role is deferred). `GET /{role}/graphql` renders GraphiQL, `POST`
//! executes, and `GET /{role}/voyager` renders GraphQL Voyager (interactive schema graph) over that role.

use async_graphql::http::GraphiQLSource;
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};

use super::acl::RequestRole;
use super::schema::CaptainSchema;

/// Mount `/{role}/graphql` for the seven roles (unknown role segments 404). Returns a `Router<()>` (the
/// schema is applied as state) so it can be merged into the main router.
pub fn graphql_routes(schema: CaptainSchema) -> Router {
    Router::new()
        .route("/{role}/graphql", get(graphiql).post(graphql_handler))
        .route("/{role}/voyager", get(voyager))
        .with_state(schema)
}

async fn graphql_handler(
    State(schema): State<CaptainSchema>,
    Path(role_seg): Path<String>,
    req: GraphQLRequest,
) -> Response {
    match RequestRole::from_segment(&role_seg) {
        // Inject the role so a future ACL guard can filter @auth fields per path (ADR-0006).
        Some(role) => {
            let resp: GraphQLResponse = schema.execute(req.into_inner().data(role)).await.into();
            resp.into_response()
        }
        None => (StatusCode::NOT_FOUND, "unknown role path").into_response(),
    }
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
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/graphql-voyager/dist/voyager.css" />
  <style>html, body, #voyager { margin: 0; height: 100vh; overflow: hidden; }</style>
</head>
<body>
  <div id="voyager">Loading GraphQL Voyager…</div>
  <script src="https://cdn.jsdelivr.net/npm/graphql-voyager/dist/voyager.standalone.js"></script>
  <script>
    GraphQLVoyager.renderVoyager(document.getElementById('voyager'), {
      introspection: function (query) {
        return fetch('__ENDPOINT__', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json', 'Accept': 'application/json' },
          body: JSON.stringify({ query: query })
        }).then(function (r) { return r.json(); });
      }
    });
  </script>
</body>
</html>
"#;
