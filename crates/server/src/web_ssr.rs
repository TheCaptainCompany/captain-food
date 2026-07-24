//! The in-process SSR transport (#92) — `web`'s `Transport` seam executed DIRECTLY against the
//! role-filtered schema, no loopback HTTP.
//!
//! `crates/web/src/graphql.rs` designed the seam for exactly this ("an in-process transport for
//! SSR could bypass HTTP entirely without touching anything above this seam"): the page renderer
//! resolves a screen's `data_requirements` through [`SchemaTransport`], which injects the SAME
//! execution context the HTTP route would — the PUBLIC role, the anonymous [`Principal`], no
//! session — so SSR can never see more than an anonymous first request would (the per-field ACL
//! applies identically). `requires_auth` screens skip server-side resolution entirely
//! (`web::router::render_path_with`); their data is the client's.

use async_trait::async_trait;
use serde_json::Value;

use crate::auth::Principal;
use crate::graphql::acl::RequestRole;
use crate::graphql::schema::CaptainSchema;
use crate::graphql::session::{SessionHeader, TraceContext};
use web::graphql::{Transport, TransportError};

/// The BFF's SSR executor: the schema + the anonymous PUBLIC context, shared with the host
/// fallback via an Extension.
#[derive(Clone)]
pub struct SsrExec {
    pub schema: CaptainSchema,
}

impl SsrExec {
    /// The transport one page render executes through.
    pub fn transport(&self) -> SchemaTransport {
        SchemaTransport { schema: self.schema.clone() }
    }
}

/// One render's transport: PUBLIC + anonymous + sessionless, per the module contract.
pub struct SchemaTransport {
    schema: CaptainSchema,
}

#[async_trait]
impl Transport for SchemaTransport {
    async fn execute(&self, document: &str, variables: Value) -> Result<Value, TransportError> {
        let request = async_graphql::Request::new(document)
            .variables(async_graphql::Variables::from_json(variables))
            .data(RequestRole::Public)
            .data(Principal::anonymous())
            .data(SessionHeader(None))
            .data(TraceContext(None));
        let response = self.schema.execute(request).await;
        if !response.errors.is_empty() {
            // Same envelope rule as the HTTP transport: anything in `errors` fails the whole read
            // (acceptance-first leaves nothing business-meaningful there).
            return Err(TransportError::Errors(format!("{:?}", response.errors)));
        }
        serde_json::to_value(response.data)
            .map_err(|e| TransportError::Malformed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphql::schema::build_schema;

    #[tokio::test]
    async fn executes_against_the_schema_without_http() {
        let exec = SsrExec { schema: build_schema(None, None, None) };
        let data = exec
            .transport()
            .execute("query { __schema { queryType { name } } }", serde_json::json!({}))
            .await
            .expect("introspection executes in-process");
        assert_eq!(data["__schema"]["queryType"]["name"], "Query");
    }

    #[tokio::test]
    async fn graphql_errors_fail_the_read_like_the_http_transport() {
        let exec = SsrExec { schema: build_schema(None, None, None) };
        let err = exec
            .transport()
            .execute("query { notAField }", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, TransportError::Errors(_)));
    }
}
