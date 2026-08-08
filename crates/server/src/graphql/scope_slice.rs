//! The subgraph SCOPE SLICE (#385 API-tier wiring, PROP-20260807-174246 D8).
//!
//! A `graphql-{scope}` bin hosts the SAME master schema as the monolith (one generated type
//! layer, one set of resolvers, the per-role ACL applied identically) but serves only ITS
//! domain's operations: this extension rejects any document whose top-level fields belong to
//! another scope, BEFORE validation/execution, using the generated `OPERATION_SCOPES` table
//! (emitted from the per-scope `specs/{scope}/api.yaml` fragments). One domain, one graph —
//! the GRANT-scoped views role (#360) adds the storage wall later; this is the serving wall.
//!
//! Introspection (`__schema`/`__type`/`__typename`) is always allowed: a subgraph legitimately
//! answers the full role-filtered schema shape (the role gateways route introspection to the
//! kernel subgraph, but any subgraph can serve it — the SLICE is about data ownership, not
//! schema visibility, which the per-role ACL already filters).

use std::sync::Arc;

use async_graphql::extensions::{Extension, ExtensionContext, ExtensionFactory, NextParseQuery};
use async_graphql::parser::types::ExecutableDocument;
use async_graphql::{ServerError, ServerResult, Variables};
// The SAME document walk the role gateways route on (one implementation, two walls): a field the
// gateway would route here is exactly a field this slice accepts.
use gateway_runtime::root_fields;

use super::generated::operation_scopes::operation_scope;

/// Attachable factory: `Schema::build(...).extension(ScopeSlice::new("ordering"))`.
pub struct ScopeSlice {
    scope: &'static str,
}

impl ScopeSlice {
    pub fn new(scope: &'static str) -> Self {
        Self { scope }
    }
}

impl ExtensionFactory for ScopeSlice {
    fn create(&self) -> Arc<dyn Extension> {
        Arc::new(ScopeSliceExtension { scope: self.scope })
    }
}

struct ScopeSliceExtension {
    scope: &'static str,
}

#[async_trait::async_trait]
impl Extension for ScopeSliceExtension {
    async fn parse_query(
        &self,
        ctx: &ExtensionContext<'_>,
        query: &str,
        variables: &Variables,
        next: NextParseQuery<'_>,
    ) -> ServerResult<ExecutableDocument> {
        let doc = next.run(ctx, query, variables).await?;
        // Every operation in the document is checked, executed or not: a document naming a
        // foreign field is a routing error whichever operation runs — reject it loudly rather
        // than let an unexecuted operation smuggle an unchecked shape past the slice.
        for (kind, fields) in root_fields(&doc) {
            for field in fields {
                if field.starts_with("__") {
                    continue; // introspection — role-filtered by the ACL, never scope-sliced
                }
                match operation_scope(kind, &field) {
                    Some(owner) if owner == self.scope => {}
                    Some(owner) => {
                        return Err(ServerError::new(
                            format!(
                                "top-level field '{field}' belongs to the '{owner}' subgraph, not '{}' — route via the role gateway (D8)",
                                self.scope
                            ),
                            None,
                        ));
                    }
                    None => {
                        // Not in the composition table: either a typo (validation would reject
                        // it anyway) or a field the emitter missed — both must fail LOUDLY here,
                        // because a slice that silently serves unmapped fields is no slice.
                        return Err(ServerError::new(
                            format!(
                                "top-level field '{field}' is not in the generated composition table (operation_scopes) — not served by any subgraph"
                            ),
                            None,
                        ));
                    }
                }
            }
        }
        Ok(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::super::schema::build_schema_for_scope;

    /// An in-scope operation executes (resolver-level errors are fine — deps are None); an
    /// out-of-scope one is rejected BY THE SLICE with the routing message.
    #[tokio::test]
    async fn slice_rejects_foreign_top_level_fields() {
        // `restaurants` lives in the network scope's api fragment.
        let network = build_schema_for_scope(None, None, None, Some("network"));
        let resp = network
            .execute("query { restaurants(input: {}) { id } }")
            .await;
        assert!(
            !resp.errors.iter().any(|e| e.message.contains("subgraph")),
            "in-scope field must pass the slice: {:?}",
            resp.errors
        );
        let resp = network.execute("query { carts(input: {}) { id } }").await;
        assert!(
            resp.errors.iter().any(|e| e.message.contains("subgraph")),
            "foreign field must be rejected by the slice: {:?}",
            resp.errors
        );
    }

    /// Introspection is never sliced — any subgraph answers the (role-filtered) schema shape.
    #[tokio::test]
    async fn introspection_passes_every_slice() {
        let schema = build_schema_for_scope(None, None, None, Some("ordering"));
        let resp = schema.execute("query { __schema { queryType { name } } }").await;
        assert!(resp.errors.is_empty(), "{:?}", resp.errors);
    }

    /// A fragment spread at the root cannot smuggle a foreign field past the slice.
    #[tokio::test]
    async fn root_fragments_are_resolved_before_the_check() {
        let schema = build_schema_for_scope(None, None, None, Some("network"));
        let resp = schema
            .execute("query { ...f } fragment f on Query { carts(input: {}) { id } }")
            .await;
        assert!(
            resp.errors.iter().any(|e| e.message.contains("subgraph")),
            "fragment-smuggled foreign field must be rejected: {:?}",
            resp.errors
        );
    }
}
