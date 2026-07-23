//! GraphQL transport + resolver execution — the READ side of the SDUI data layer (split 2/4 of #21).
//!
//! Two layers, deliberately separated:
//!   * [`Transport`] — "send this document + variables, give me back `data`". Object-safe and
//!     async, so the entire data layer is unit-testable by injecting a fake (no network, no
//!     server); the real [`HttpTransport`] is one impl among possible others (an in-process
//!     transport for SSR could bypass HTTP entirely without touching anything above this seam).
//!   * [`execute_resolver`] — the ONLY public read entry point. It dispatches a GENERATED
//!     [`ResolverKey`] (the spec allowlist, `generated/data_layer.rs`), so the renderer can only
//!     ever read data the API serves — and a `gap:` binding FAILS CLOSED with
//!     [`ResolverError::GapBinding`] instead of silently rendering nothing (the rule stated in the
//!     generated file's header).
//!
//! Documents are assembled from two sources: the OPERATION SHAPE (name + input type) follows the
//! api.yaml naming conventions, while the SELECTION SET is GENERATED per resolver from the
//! api.yaml type registry ([`ResolverKey::selection`]) — see [`execute_resolver`] for the honest
//! statement of what that does and does not guarantee.

use serde_json::{json, Map, Value};

use crate::generated::data_layer::ResolverKey;
use crate::session::{SessionId, SESSION_HEADER};

/// The seven role paths (ADR-0006: role = path, one filtered schema each). Mirrors the server's
/// `RequestRole::segment` mapping (`crates/server/src/graphql/acl.rs`) — `web` cannot depend on
/// `server`, so the segment spelling is duplicated here; an unknown segment 404s server-side, which
/// keeps the mirror honest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Public,
    Customer,
    RestaurantAccount,
    Restaurant,
    Rider,
    Admin,
    External,
}

impl Role {
    /// The URL path segment this role's GraphQL is mounted under.
    pub fn segment(&self) -> &'static str {
        match self {
            Role::Public => "public",
            Role::Customer => "customer",
            Role::RestaurantAccount => "restaurant-account",
            Role::Restaurant => "restaurant",
            Role::Rider => "rider",
            Role::Admin => "admin",
            Role::External => "external",
        }
    }
}

/// What can go wrong BELOW the resolver layer — network, HTTP, or the GraphQL envelope itself.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// The request never produced an HTTP response (DNS, refused connection, fetch abort...).
    #[error("transport failure: {0}")]
    Network(String),
    /// A non-2xx HTTP status — the GraphQL layer was never reached (routing/auth/proxy problem).
    #[error("HTTP {status} from the GraphQL endpoint")]
    Status { status: u16 },
    /// The server executed and answered with GraphQL `errors`. NOTE: business rejections are NOT
    /// here (acceptance-first, ADR-20260720-015500) — they surface as `operationStatus` REJECTED.
    /// Anything in `errors` is a contract-level failure (validation, authz, malformed document).
    #[error("GraphQL errors: {0}")]
    Errors(String),
    /// A 2xx response whose body is not the GraphQL envelope we expect.
    #[error("malformed GraphQL response: {0}")]
    Malformed(String),
}

/// The transport seam: one method, JSON in / JSON `data` out. Implementations return the `data`
/// object only — GraphQL `errors` become [`TransportError::Errors`], so callers never inspect the
/// envelope. `?Send` on wasm32: browser futures are single-threaded by construction.
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait Transport {
    async fn execute(&self, document: &str, variables: Value) -> Result<Value, TransportError>;
}

/// The real HTTP transport: POST to `/{role}/graphql`, [`SESSION_HEADER`] on EVERY request (the
/// server's ownership scoping for anonymous users depends on it — a request without the header is a
/// different, session-less identity). The role is fixed at construction (role = path, ADR-0006): a
/// client IS a role's client; talking to another role is a different client, not a per-call flag.
pub struct HttpTransport {
    endpoint: String,
    session: SessionId,
    client: reqwest::Client,
}

impl HttpTransport {
    /// `base_url` is the origin (no trailing slash needed): on the browser path pass the window
    /// origin (reqwest's wasm backend needs absolute URLs), on the SSR path the BFF's loopback
    /// origin. The endpoint becomes `{base_url}/{role}/graphql`.
    pub fn new(base_url: &str, role: Role, session: SessionId) -> Self {
        Self {
            endpoint: format!("{}/{}/graphql", base_url.trim_end_matches('/'), role.segment()),
            session,
            client: reqwest::Client::new(),
        }
    }

    /// The resolved endpoint (diagnostics/tests).
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl Transport for HttpTransport {
    async fn execute(&self, document: &str, variables: Value) -> Result<Value, TransportError> {
        let response = self
            .client
            .post(&self.endpoint)
            .header(SESSION_HEADER, self.session.to_string())
            .json(&json!({ "query": document, "variables": variables }))
            .send()
            .await
            .map_err(|e| TransportError::Network(e.to_string()))?;

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(TransportError::Status { status });
        }
        let body: Value =
            response.json().await.map_err(|e| TransportError::Malformed(e.to_string()))?;

        // Per the GraphQL spec a response can carry BOTH data and errors (partial success); the
        // acceptance-first contract leaves nothing business-meaningful in `errors`, so any error
        // is treated as a failure of the whole read — no partial-data heroics.
        if let Some(errors) = body.get("errors").filter(|e| e.as_array().is_some_and(|a| !a.is_empty())) {
            return Err(TransportError::Errors(errors.to_string()));
        }
        match body.get("data") {
            Some(data) if !data.is_null() => Ok(data.clone()),
            _ => Err(TransportError::Malformed("response carries neither data nor errors".into())),
        }
    }
}

/// What can go wrong AT the resolver layer (above the transport).
#[derive(Debug, thiserror::Error)]
pub enum ResolverError {
    /// The resolver is a declared spec `gap` — the UI names data no API query serves yet. Fail
    /// closed, loudly and distinctly (never a silent empty result): the caller must render the
    /// gap's fallback, and the fix is a spec change, not a client workaround.
    #[error("resolver `{key}` is a declared gap (no bound query): {note}")]
    GapBinding { key: &'static str, note: &'static str },
    #[error(transparent)]
    Transport(#[from] TransportError),
    /// `data` came back without the operation's field — a contract drift between the generated
    /// allowlist and the served schema (should be impossible while the validator gates both).
    #[error("response data has no `{operation}` field")]
    MissingOperation { operation: &'static str },
}

/// Execute an allowlisted resolver: the ONLY public read entry point of the crate.
///
/// Variables: the DSL's pinned static `.args()` are inserted FIRST, then `extra_variables` — so a
/// caller-supplied key overrides a pin (the pin is the binding's default, e.g. `restaurants.featured`
/// → `list: RECOMMENDED`; a screen passing its own `list` is asking a different question on
/// purpose). Everything lands under the single `$input` variable per the api.yaml convention
/// (`<Query>QueryInput` — args are never inlined on the field).
///
/// HONESTY NOTE on the document (do not mistake this for full schema-derivation): the OPERATION
/// SHAPE is still naming-CONVENTION-derived — operation `restaurants` takes
/// `input: RestaurantsQueryInput` because api.yaml freezes that convention, not because the SDL was
/// read. The SELECTION SET however is GENERATED per resolver from the api.yaml type registry
/// ([`ResolverKey::selection`]): every query-bound resolver expands its return type's full field
/// tree (depth-bounded and cycle-guarded in the codegen; FK navigation edges are not selected), so
/// every one of them builds a VALID document and can run against the live server. The only
/// resolvers that cannot run live are the declared `gap:` bindings (`promotions.active`,
/// `dishes.search`, `rewards.balance`) — they bind no query at all and fail closed with
/// [`ResolverError::GapBinding`] before any network.
pub async fn execute_resolver(
    transport: &dyn Transport,
    key: ResolverKey,
    extra_variables: Map<String, Value>,
) -> Result<Value, ResolverError> {
    let Some(operation) = key.query() else {
        return Err(ResolverError::GapBinding {
            key: key.as_str(),
            note: key.gap().unwrap_or("unbound resolver with no gap note"),
        });
    };

    // Pinned DSL args first, caller's own after (caller wins on collision — see doc above).
    let mut input = Map::new();
    for (name, value) in key.args() {
        // Pins are enum tokens/strings; GraphQL variables encode enum values as JSON strings, so
        // the string form is the correct wire shape.
        input.insert((*name).to_string(), Value::String((*value).to_string()));
    }
    input.extend(extra_variables);

    let document = query_document(operation, !input.is_empty(), key.selection());
    let variables = if input.is_empty() { json!({}) } else { json!({ "input": input }) };

    let data = transport.execute(&document, variables).await?;
    match data.get(operation) {
        Some(subtree) => Ok(subtree.clone()),
        None => Err(ResolverError::MissingOperation { operation }),
    }
}

/// Build the query document from the api.yaml conventions (single `input` arg named
/// `<PascalOperation>QueryInput`). `$input` is declared non-null — we only bind it when we actually
/// have variables, and a non-null variable is accepted at both nullable and non-null arg positions.
fn query_document(operation: &str, with_input: bool, selection: Option<&str>) -> String {
    let selection = selection.map(|s| format!(" {s}")).unwrap_or_default();
    if with_input {
        format!(
            "query Resolver($input: {}QueryInput!) {{ {operation}(input: $input){selection} }}",
            pascal(operation)
        )
    } else {
        format!("query Resolver {{ {operation}{selection} }}")
    }
}

/// camelCase operation name → the PascalCase prefix of its generated input type.
pub(crate) fn pascal(operation: &str) -> String {
    let mut chars = operation.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// Shared test double: a scripted [`Transport`] that records every request and pops canned
/// responses in order — the whole data layer tests against it with zero network/server.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::sync::Mutex;

    pub struct FakeTransport {
        calls: Mutex<Vec<(String, Value)>>,
        responses: Mutex<Vec<Result<Value, TransportError>>>,
    }

    impl FakeTransport {
        pub fn scripted(responses: Vec<Result<Value, TransportError>>) -> Self {
            Self { calls: Mutex::new(Vec::new()), responses: Mutex::new(responses) }
        }

        pub fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }

        /// The (document, variables) of call `i` — panics out-of-range (a test bug).
        pub fn call(&self, i: usize) -> (String, Value) {
            self.calls.lock().unwrap()[i].clone()
        }
    }

    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    impl Transport for FakeTransport {
        async fn execute(&self, document: &str, variables: Value) -> Result<Value, TransportError> {
            self.calls.lock().unwrap().push((document.to_string(), variables));
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                panic!("FakeTransport: unscripted call: {document}");
            }
            responses.remove(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::FakeTransport;
    use super::*;

    #[tokio::test]
    async fn gap_bound_resolver_is_refused_before_any_network() {
        let fake = FakeTransport::scripted(vec![]);
        let err = execute_resolver(&fake, ResolverKey::PromotionsActive, Map::new())
            .await
            .unwrap_err();
        // Distinct variant + the spec's own gap note — never a silent empty result.
        match err {
            ResolverError::GapBinding { key, note } => {
                assert_eq!(key, "promotions.active");
                assert!(note.contains("promotions"), "gap note should be the spec's: {note}");
            }
            other => panic!("expected GapBinding, got {other:?}"),
        }
        assert_eq!(fake.call_count(), 0, "a gap must fail closed, not reach the transport");
    }

    #[tokio::test]
    async fn pinned_static_args_merge_before_the_callers_own() {
        let fake = FakeTransport::scripted(vec![Ok(json!({ "restaurants": [] }))]);
        let mut extra = Map::new();
        extra.insert("city".into(), json!("tours"));
        let result = execute_resolver(&fake, ResolverKey::RestaurantsFeatured, extra).await.unwrap();
        assert_eq!(result, json!([]));

        let (document, variables) = fake.call(0);
        // The convention-derived document: single $input of <Pascal>QueryInput.
        assert!(document.contains("$input: RestaurantsQueryInput!"), "{document}");
        assert!(document.contains("restaurants(input: $input)"), "{document}");
        // DSL pin AND caller variable are both in the merged input.
        assert_eq!(variables["input"]["list"], json!("RECOMMENDED"));
        assert_eq!(variables["input"]["city"], json!("tours"));
    }

    #[tokio::test]
    async fn caller_variables_override_a_pinned_arg() {
        let fake = FakeTransport::scripted(vec![Ok(json!({ "restaurants": [] }))]);
        let mut extra = Map::new();
        extra.insert("list".into(), json!("TOP_DEALS"));
        execute_resolver(&fake, ResolverKey::RestaurantsFeatured, extra).await.unwrap();
        // The pin is a default, not a lock — the caller's own value wins.
        assert_eq!(fake.call(0).1["input"]["list"], json!("TOP_DEALS"));
    }

    #[tokio::test]
    async fn variable_free_resolver_still_selects_its_generated_field_tree() {
        let fake = FakeTransport::scripted(vec![Ok(json!({ "me": null }))]);
        execute_resolver(&fake, ResolverKey::MeProfile, Map::new()).await.unwrap();
        let (document, variables) = fake.call(0);
        // No args → no $input declaration, but the CustomerProfile selection set is still there
        // (a bare `{ me }` would be invalid GraphQL — CustomerProfile is an object type).
        assert!(!document.contains("$input"), "{document}");
        assert!(document.starts_with("query Resolver { me { "), "{document}");
        assert!(document.contains("customerId"), "{document}");
        assert_eq!(variables, json!({}));
    }

    #[test]
    fn every_query_bound_resolver_carries_a_selection_set() {
        // Every api.yaml query the screens bind today returns an OBJECT type, so a bound resolver
        // without a selection set would build an invalid document — the generated allowlist must
        // never put us there (selection() is None only for gaps / scalar returns).
        for key in ResolverKey::ALL {
            assert_eq!(
                key.query().is_some(),
                key.selection().is_some(),
                "resolver `{}` breaks the query↔selection pairing",
                key.as_str()
            );
        }
    }

    #[tokio::test]
    async fn operation_status_selects_what_the_write_dispatcher_reads() {
        // The two-step write flow depends on this resolver actually working — the GENERATED
        // Operation selection must keep covering what actions.rs consumes (status, errorCode,
        // message, messageId).
        let fake = FakeTransport::scripted(vec![Ok(json!({ "operationStatus": null }))]);
        let mut vars = Map::new();
        vars.insert("messageId".into(), json!("00000000-0000-0000-0000-000000000000"));
        execute_resolver(&fake, ResolverKey::OperationStatusByMessage, vars).await.unwrap();
        let (document, _) = fake.call(0);
        assert!(document.contains("$input: OperationStatusQueryInput!"), "{document}");
        assert!(document.contains("{ messageId correlationId status errorCode message occurredAt }"), "{document}");
    }

    #[tokio::test]
    async fn missing_operation_field_is_a_contract_error() {
        let fake = FakeTransport::scripted(vec![Ok(json!({ "somethingElse": 1 }))]);
        let err = execute_resolver(&fake, ResolverKey::MeProfile, Map::new()).await.unwrap_err();
        assert!(matches!(err, ResolverError::MissingOperation { operation: "me" }));
    }

    #[test]
    fn http_transport_builds_the_role_path_endpoint() {
        let t = HttpTransport::new("https://tours.captain.food/", Role::Public, SessionId::mint());
        assert_eq!(t.endpoint(), "https://tours.captain.food/public/graphql");
        let t = HttpTransport::new("http://127.0.0.1:8080", Role::Customer, SessionId::mint());
        assert_eq!(t.endpoint(), "http://127.0.0.1:8080/customer/graphql");
    }
}
