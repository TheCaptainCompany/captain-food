//! The in-process SSR transport (#92) — `web`'s `Transport` seam executed DIRECTLY against the
//! role-filtered schema, no loopback HTTP.
//!
//! `crates/web/src/graphql.rs` designed the seam for exactly this ("an in-process transport for
//! SSR could bypass HTTP entirely without touching anything above this seam"): the page renderer
//! resolves a screen's `data_requirements` through [`SchemaTransport`], which injects the SAME
//! execution context the HTTP route would — the PUBLIC role, the anonymous [`Principal`], no
//! session — so SSR can never see more than an anonymous first request would (the per-field ACL
//! applies identically). Since #420 `render_path_with` resolves the `data_requirements` of EVERY
//! matched screen, including `requires_auth` ones: whether a read is permitted is a fact about the
//! TRANSPORT, which the renderer cannot know, so it asks and lets a refusal degrade that one
//! binding. On this anonymous transport a `requires_auth` screen's reads are refused by the
//! per-field ACL during validation — no resolver, no query, output unchanged — so the behaviour
//! here is identical to the old skip. **The day this transport carries identity that stops being
//! true**: the same code path will emit personalised HTML, and nothing in `hosts.rs` sets
//! `Cache-Control`. That is a caching decision to take deliberately, not to discover.

use async_trait::async_trait;
use serde_json::Value;

use crate::auth::Principal;
use crate::graphql::acl::RequestRole;
use crate::graphql::schema::CaptainSchema;
use crate::graphql::session::{RequestCorrelationId, SessionHeader, TraceContext};
use web::graphql::{Transport, TransportError};

/// The BFF's SSR executor: the schema + the anonymous PUBLIC context, shared with the host
/// fallback via an Extension.
#[derive(Clone)]
pub struct SsrExec {
    pub schema: CaptainSchema,
    /// The Stripe publishable TEST key the checkout shell delivers to the browser (#440), parsed
    /// ONCE at the composition root: `None` = absent/empty/malformed (all one state — the
    /// development profile continues past an INVALID config value, so re-gating at this seam is
    /// what keeps a bad value from ever reaching stripe.js). `None` renders the degraded checkout
    /// and is counted at the render boundary (`hosts::app_page`).
    pub stripe_publishable_key: Option<web::stripe::PublishableKey>,
}

impl SsrExec {
    /// The transport one page render executes through.
    ///
    /// The render's correlation id is minted HERE, once, and carried on the transport — NOT inside
    /// `execute`. A page render resolves EVERY screen binding through this one transport
    /// (`web::router::render_path_with` loops `execute_resolver` once per `data_requirements`
    /// entry — three calls for the checkout screen), so minting per `execute` would emit N
    /// unrelated ids for a single render and correlate none of them to each other. That is the
    /// exact defect this type was introduced to fix on the HTTP path.
    pub fn transport(&self) -> SchemaTransport {
        SchemaTransport {
            schema: self.schema.clone(),
            correlation_id: RequestCorrelationId::mint(),
            // The render's ONE service-window clock (RSO-1), minted with the correlation id for
            // the same reason: every binding of this render agrees on "now". NOTE: a
            // `serviceWindow` baked into SSR HTML goes STALE at its `validUntil` — any future
            // HTML caching in `hosts.rs` must bound its TTL by min(validUntil) of the verdicts
            // it embeds, or a "Ferme à 23:00" badge outlives the kitchen.
            request_now: crate::graphql::service_clock::RequestNow::mint(),
        }
    }
}

/// One render's transport: PUBLIC + anonymous + sessionless, per the module contract, carrying the
/// ONE correlation id shared by every binding that render resolves.
pub struct SchemaTransport {
    schema: CaptainSchema,
    correlation_id: RequestCorrelationId,
    request_now: crate::graphql::service_clock::RequestNow,
}

#[async_trait]
impl Transport for SchemaTransport {
    async fn execute(&self, document: &str, variables: Value) -> Result<Value, TransportError> {
        let request = async_graphql::Request::new(document)
            .variables(async_graphql::Variables::from_json(variables))
            .data(RequestRole::Public)
            .data(Principal::anonymous())
            .data(SessionHeader(None))
            .data(TraceContext(None))
            // The RENDER's id, minted once in `transport()` and shared by every binding this render
            // resolves — see the note there. Says nothing about identity: the transport stays
            // PUBLIC, anonymous and sessionless per the module contract above.
            .data(self.correlation_id)
            .data(self.request_now);
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
        let exec = SsrExec { schema: build_schema(None, None, None), stripe_publishable_key: None };
        let data = exec
            .transport()
            .execute("query { __schema { queryType { name } } }", serde_json::json!({}))
            .await
            .expect("introspection executes in-process");
        assert_eq!(data["__schema"]["queryType"]["name"], "Query");
    }

    #[tokio::test]
    async fn graphql_errors_fail_the_read_like_the_http_transport() {
        let exec = SsrExec { schema: build_schema(None, None, None), stripe_publishable_key: None };
        let err = exec
            .transport()
            .execute("query { notAField }", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, TransportError::Errors(_)));
    }
}
