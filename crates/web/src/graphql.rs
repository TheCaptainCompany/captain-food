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

    /// The inverse of [`Role::user_type`] (#639 2c-ii, R1): the role a screen's `graphql_role:`
    /// token names. `None` for a token outside the closed set — unreachable for generated screen
    /// tables (validator §26 refuses it), kept total so a caller must fall back explicitly.
    pub fn from_user_type(token: &str) -> Option<Role> {
        [
            Role::Public,
            Role::Customer,
            Role::RestaurantAccount,
            Role::Restaurant,
            Role::Rider,
            Role::Admin,
            Role::External,
        ]
        .into_iter()
        .find(|r| r.user_type() == token)
    }

    /// The `scalars.yaml#/UserType` token this role path carries (#472) — the vocabulary
    /// `ResolverKey::roles()` (api.yaml `roles:`, verbatim) speaks. Mirrors `segment()`'s
    /// mirror-honesty rule: one closed set, spelled once.
    pub fn user_type(&self) -> &'static str {
        match self {
            Role::Public => "PUBLIC",
            Role::Customer => "CUSTOMER",
            Role::RestaurantAccount => "RESTAURANT_ACCOUNT",
            Role::Restaurant => "RESTAURANT",
            Role::Rider => "RIDER",
            Role::Admin => "ADMIN",
            Role::External => "EXTERNAL",
        }
    }
}

/// One GraphQL error's `extensions`, typed (#639 part C step 4-ii, ADR-20260904-124600 §1): `code`
/// (the guard's literal, e.g. `FORBIDDEN`) and `reason` (`StandingGuard`'s ADDITIVE discriminator,
/// `shared_types::RIDER_RESTRICTED` — absent on a bare `RoleGuard` rejection or a business
/// rejection's typed `context`). Parsed ONCE at the transport boundary (`HttpTransport::execute`)
/// so the bounce decision (`crate::bounce`) never re-stringifies the `errors` array to classify a
/// refusal — the exact thing the pre-4-ii transport did (`errors.to_string()`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ErrorExtensions {
    pub code: Option<String>,
    pub reason: Option<String>,
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
    /// `message` keeps the raw array's text for DISPLAY (unchanged); `extensions` is the TYPED
    /// per-error `extensions` object (#639 4-ii) the bounce decision classifies on.
    #[error("GraphQL errors: {message}")]
    Errors { message: String, extensions: Vec<ErrorExtensions> },
    /// A 2xx response whose body is not the GraphQL envelope we expect.
    #[error("malformed GraphQL response: {0}")]
    Malformed(String),
}

/// Platform-conditional `Sync` requirement: NATIVE transports must be `Sync` so futures holding a
/// `&dyn Transport` across an await are `Send` (the axum SSR handler's requirement, #92); browser
/// transports are single-threaded by construction and carry no such bound (reqwest's wasm client
/// is not `Sync`). Blanket-implemented — never implement it by hand.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSync: Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Sync + ?Sized> MaybeSync for T {}
#[cfg(target_arch = "wasm32")]
pub trait MaybeSync {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> MaybeSync for T {}

/// The transport seam: one method, JSON in / JSON `data` out. Implementations return the `data`
/// object only — GraphQL `errors` become [`TransportError::Errors`], so callers never inspect the
/// envelope. `?Send` on wasm32: browser futures are single-threaded by construction.
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait Transport: MaybeSync {
    async fn execute(&self, document: &str, variables: Value) -> Result<Value, TransportError>;
}

/// The real HTTP transport: POST to `/{role}/graphql`, [`SESSION_HEADER`] on EVERY request (the
/// server's ownership scoping for anonymous users depends on it — a request without the header is a
/// different, session-less identity). The role is fixed at construction (role = path, ADR-0006): a
/// client IS a role's client; talking to another role is a different client, not a per-call flag.
///
/// Customer identity (#437/#112): this transport NEVER builds an `Authorization` header — the
/// signed-in customer's only credential is the httpOnly `captain_auth` cookie, which JS cannot
/// read and the browser attaches to same-origin fetches on its own (the endpoint is the window
/// origin on the browser path, so `fetch`'s default `same-origin` credentials mode sends it).
/// Anonymous visitors simply have no cookie: same request shape, no Authorization key, ever.
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
            // #639 4-ii (ADR-20260904-124600 §1): parse `extensions` into the typed shape HERE —
            // the ONE place the raw JSON array exists — so no caller re-stringifies it to classify
            // a refusal ever again.
            let extensions = errors
                .as_array()
                .into_iter()
                .flatten()
                .map(|e| ErrorExtensions {
                    code: e
                        .get("extensions")
                        .and_then(|x| x.get("code"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    reason: e
                        .get("extensions")
                        .and_then(|x| x.get("reason"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                })
                .collect();
            return Err(TransportError::Errors { message: errors.to_string(), extensions });
        }
        match body.get("data") {
            Some(data) if !data.is_null() => Ok(data.clone()),
            _ => Err(TransportError::Malformed("response carries neither data nor errors".into())),
        }
    }
}

/// The one-shot 401-refresh seam (#904, ADR-20260905-101349 §13 — the member door's flip
/// precondition). One method so the production POST to `/auth/refresh` and the test double's
/// call-counting are the ONLY two implementations anyone ever needs — the decorator below never
/// inspects the cookie itself. `true` = the session rotated; the caller reissues the ORIGINAL
/// document once, `false` = the refresh itself failed and the original 401 goes to the caller.
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait Refresher: MaybeSync {
    async fn refresh(&self) -> bool;
}

/// Wraps any [`Transport`] with the EXACTLY-ONCE 401-refresh-and-reissue rule (#904): on the first
/// HTTP 401 of the load, call the [`Refresher`] once and — if it rotated the session — reissue the
/// SAME document + variables once; every subsequent 401 of the same load (whether the refresh
/// itself failed, or a second request 401s after a successful refresh — a provider outage doesn't
/// get re-tried per request) returns straight through untouched. **Only a bare HTTP 401 arms it —
/// a 403 NEVER does** (ADR-20260904-081527 §4/`server/src/auth.rs`'s `AuthError::Forbidden` arm: a
/// rotated token carries the exact same role, so a `RoleGuard` refusal would loop forever if it
/// re-armed a refresh). GraphQL `errors`,
/// network failures and every other status pass through unexamined — refreshing a cookie can never
/// fix a malformed document or a role a fresh cookie still lacks.
///
/// The one-shot state is an `Arc<AtomicBool>`, not the `Rc<Cell<bool>>` a wasm32-only decorator
/// would reach for first: [`Transport`] requires [`MaybeSync`] (== `Sync` off wasm32, so the native
/// `#[tokio::test]`s this module is tested with — beck's Q1 — can hold a `&dyn Transport` across an
/// `.await`), and `Rc`/`Cell` are `!Sync` on every target. `AtomicBool` costs nothing extra on
/// wasm32's single thread and satisfies the bound everywhere. Composing ONE instance per screen
/// load (`renderer::hydrate`) and sharing its `Arc` with the SAME load's mutation dispatcher
/// (`interact::install`) is what makes a refresh failure "remembered for the page", not just for
/// one resolver — the hydrate loop is sequential (one `.await` at a time), so there is no
/// concurrent-refresh race to additionally guard against.
pub struct RefreshingTransport {
    inner: Box<dyn Transport>,
    refresher: Box<dyn Refresher>,
    used: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl RefreshingTransport {
    pub fn new(
        inner: Box<dyn Transport>,
        refresher: Box<dyn Refresher>,
        used: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self { inner, refresher, used }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl Transport for RefreshingTransport {
    async fn execute(&self, document: &str, variables: Value) -> Result<Value, TransportError> {
        let result = self.inner.execute(document, variables.clone()).await;
        let Err(TransportError::Status { status: 401 }) = &result else {
            // Not a bare 401 (a 403, a network failure, GraphQL `errors`, malformed, or success):
            // never arms or consumes the one shot.
            return result;
        };
        if self.used.swap(true, std::sync::atomic::Ordering::SeqCst) {
            // Already spent this load's one shot — a second 401 (failed refresh, or a fresh one
            // after a successful refresh) goes straight to the caller.
            return result;
        }
        if !self.refresher.refresh().await {
            // The refresh call itself failed: the ORIGINAL 401 is what the caller sees — never a
            // different error, never a retry loop.
            return result;
        }
        self.inner.execute(document, variables).await
    }
}

/// The production [`Refresher`]: POST `/auth/refresh` with the httpOnly `captain_refresh` cookie
/// the browser attaches on its own (JS never reads it — same posture as [`HttpTransport`]'s doc
/// comment on customer identity). wasm32-only, like [`crate::auth::claim_session`] — the browser
/// `fetch` credentials mode it needs has no native equivalent this crate uses.
#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
pub struct HttpRefresher {
    origin: String,
    session: SessionId,
}

#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
impl HttpRefresher {
    pub fn new(origin: &str, session: SessionId) -> Self {
        Self { origin: origin.trim_end_matches('/').to_string(), session }
    }
}

#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
#[async_trait::async_trait(?Send)]
impl Refresher for HttpRefresher {
    async fn refresh(&self) -> bool {
        let url = format!("{}/auth/refresh", self.origin);
        let client = reqwest::Client::new();
        matches!(
            client
                .post(&url)
                .header(SESSION_HEADER, self.session.to_string())
                .fetch_credentials_include()
                .send()
                .await,
            Ok(resp) if resp.status().is_success()
        )
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

/// WHY a read was skipped by design (#745). Every reason renders IDENTICALLY (the binding stays
/// silently unresolved — empty state, hydrate/dispatch owns the data); the reason exists for the
/// TRACE, as an attribute on the render's boundary event alongside the correlation id — never as
/// a metric label (a zero-weight metric reason is a signal wired never to scream, the exact
/// defect class the #472 counter contract calls out).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// A declared spec `gap:` binding — fails closed at the dispatcher before any network.
    DeclaredGap,
    /// The bound query does not admit this path's role (`ResolverKey::roles()`) — the refusal is
    /// the documented posture of the anonymous transport, not an incident.
    RoleRefused,
    /// The §25b verdict (#745): a required arg with no paint-time source — route param, pin or
    /// tenant-host slug — declared on the binding (`skipped_reads:` in the screens DSL) and
    /// emitted onto `Screen::skipped_reads`. Skipped BEFORE any network: the read would fail
    /// GraphQL validation on every paint, which is a spec fact, not a runtime incident.
    StructurallyUnfulfillable,
}

impl SkipReason {
    /// The trace-attribute token.
    pub fn as_str(&self) -> &'static str {
        match self {
            SkipReason::DeclaredGap => "declared_gap",
            SkipReason::RoleRefused => "role_refused",
            SkipReason::StructurallyUnfulfillable => "structurally_unfulfillable",
        }
    }
}

/// One resolver read, CLASSIFIED (#472): the type that makes "skip" and "failure" different
/// states, so no render path can conflate them again. Before this type existed every call site
/// read `if let Ok(value) = execute_resolver(...)` — the else-branch was unwritable because a
/// role-guard refusal (expected, documented: the anonymous SSR transport asking a
/// CUSTOMER-guarded read) and a real transport failure arrived as the same `Err`.
#[derive(Debug)]
pub enum ResolveOutcome {
    /// The read answered — bind it.
    Resolved(Value),
    /// The read was never answerable ON THIS PATH, by design — the [`SkipReason`] says why, for
    /// the trace only. Silent — the shell renders and hydration owns the data (the #92/#420
    /// anonymous-SSR posture).
    SkippedByDesign(SkipReason),
    /// A REAL failure on a read this role IS allowed to ask — network, HTTP, GraphQL contract,
    /// malformed envelope. The caller renders the ERROR state (distinct from the empty state) and
    /// the SSR boundary counts it. Never silent.
    Failed(ResolverError),
}

/// Classify one resolver outcome for the role path that asked (#472). Structural, not textual:
/// the skip/failure split reads the spec's own `roles:` declaration (emitted onto
/// [`ResolverKey::roles`]), never the error string.
pub fn classify_resolve(
    role: Role,
    key: ResolverKey,
    result: Result<Value, ResolverError>,
) -> ResolveOutcome {
    match result {
        Ok(value) => ResolveOutcome::Resolved(value),
        // A declared gap fails closed before any network — the screen's gap fallback owns it.
        Err(ResolverError::GapBinding { .. }) => {
            ResolveOutcome::SkippedByDesign(SkipReason::DeclaredGap)
        }
        Err(e) => {
            let roles = key.roles();
            if !roles.is_empty() && !roles.contains(&role.user_type()) {
                // This path's role may not ask this query at all: the refusal is the documented
                // posture, not an incident. (Empty `roles` = open to every path.)
                ResolveOutcome::SkippedByDesign(SkipReason::RoleRefused)
            } else {
                ResolveOutcome::Failed(e)
            }
        }
    }
}

/// Execute an allowlisted resolver: the ONLY public read entry point of the crate.
///
/// Variables: the DSL's pinned static `.args()` are inserted FIRST, then `extra_variables` — so a
/// caller-supplied key overrides a pin (the pin is the binding's default, e.g. `restaurants.featured`
/// → `list: RECOMMENDED`; a screen passing its own `list` is asking a different question on
/// purpose). Everything lands under the single `$input` variable per the api.yaml convention
/// (`<Query>QueryInput` — args are never inlined on the field).
///
/// The document is fully GENERATED-name-driven (#97 closed the last convention gap): the input
/// TYPE comes from [`ResolverKey::input_type`] — emitted by the same codegen that emits the SDL,
/// so the name the client sends is read from the source of truth, never re-derived — and the
/// SELECTION SET is GENERATED per resolver from the api.yaml type registry
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

    // Input is bound only when there is something to send AND the SDL declares an input type for
    // this query (#97: `input_type()` is generated FROM the schema emitter — an argless query has
    // no input type, so caller-supplied variables for one are unsendable by construction).
    let input_type = key.input_type().filter(|_| !input.is_empty());
    let document = query_document(operation, input_type, key.selection());
    let variables = if input_type.is_some() { json!({ "input": input }) } else { json!({}) };

    let data = transport.execute(&document, variables).await?;
    match data.get(operation) {
        Some(subtree) => Ok(subtree.clone()),
        None => Err(ResolverError::MissingOperation { operation }),
    }
}

/// Build the query document. `input_type` is the SDL's OWN input-type name (generated,
/// `ResolverKey::input_type`, #97) — never re-derived from the operation name here. `$input` is
/// declared non-null — a non-null variable is accepted at both nullable and non-null arg positions.
fn query_document(operation: &str, input_type: Option<&str>, selection: Option<&str>) -> String {
    let selection = selection.map(|s| format!(" {s}")).unwrap_or_default();
    match input_type {
        Some(ty) => format!(
            "query Resolver($input: {ty}!) {{ {operation}(input: $input){selection} }}"
        ),
        None => format!("query Resolver {{ {operation}{selection} }}"),
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

    /// So a test can keep its own `Arc<FakeTransport>` handle (to read `call_count()`/`call(i)`)
    /// after moving a clone of it into a `Box<dyn Transport>` — the SAME need
    /// [`super::Refresher`]'s `Arc<CountingRefresher>` impl below exists for (#904, needed once
    /// `RefreshingTransport` wraps a `FakeTransport` in `pending.rs`'s own tests).
    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    impl Transport for std::sync::Arc<FakeTransport> {
        async fn execute(&self, document: &str, variables: Value) -> Result<Value, TransportError> {
            FakeTransport::execute(self, document, variables).await
        }
    }

    /// The [`super::Refresher`] test double (#904): counts calls, answers a scripted outcome —
    /// never touches a network. `AtomicUsize`, not `Cell`, for the same `Sync` reason
    /// [`super::RefreshingTransport`] itself uses `AtomicBool` (see its doc comment).
    pub struct CountingRefresher {
        calls: std::sync::atomic::AtomicUsize,
        succeeds: bool,
    }

    impl CountingRefresher {
        pub fn new(succeeds: bool) -> Self {
            Self { calls: std::sync::atomic::AtomicUsize::new(0), succeeds }
        }

        pub fn call_count(&self) -> usize {
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    impl super::Refresher for CountingRefresher {
        async fn refresh(&self) -> bool {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.succeeds
        }
    }

    /// So a test can keep its own `Arc<CountingRefresher>` handle (to read `call_count()`) after
    /// moving a clone of it into a `Box<dyn Refresher>`.
    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    impl super::Refresher for std::sync::Arc<CountingRefresher> {
        async fn refresh(&self) -> bool {
            CountingRefresher::refresh(self).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::FakeTransport;
    use super::*;

    #[test]
    fn generated_input_types_are_total_over_the_allowlists() {
        use crate::generated::data_layer::{subscription_input_type, ActionKey, ActionKind};
        // Every mutation-kind action carries its SDL input-type name; every other kind carries none
        // — the invariant `dispatch` leans on (#97).
        for key in ActionKey::ALL {
            match key.kind() {
                ActionKind::Mutation => {
                    assert!(key.input_type().is_some(), "{} lacks an input type", key.as_str())
                }
                _ => assert!(key.input_type().is_none(), "{} must not have one", key.as_str()),
            }
        }
        // A resolver has an input type exactly when its bound query takes args (gap ⇒ none).
        for key in ResolverKey::ALL {
            if key.query().is_none() {
                assert!(key.input_type().is_none(), "{} is a gap", key.as_str());
            }
        }
        // The three allowlisted subscriptions all resolve; unknown operations never do.
        for op in ["orderStatusChanged", "paymentStatusChanged", "operationStatusChanged"] {
            assert!(subscription_input_type(op).is_some(), "{op}");
        }
        assert_eq!(subscription_input_type("notASubscription"), None);
    }

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

    /// #472 checkpoint (beck): classification is decided by the ROLE-vs-`roles()` check, never by
    /// the error VARIANT. The renderer/router tests alone would let a mutant classifying
    /// "Errors → skip, Network → fail" pass (their skip case happened to feed `Errors` and their
    /// fail case `Network`), so this test crosses the variants both directions:
    /// a role OUTSIDE the resolver's roles skips even on a Network error, and a role INSIDE them
    /// fails even on an authorization-flavoured `Errors` payload.
    #[test]
    fn classification_is_decided_by_roles_not_by_the_error_variant() {
        use crate::generated::data_layer::ResolverKey;
        let network = || TransportError::Network("connection reset by peer".into()).into();
        let auth_errors = || {
            TransportError::Errors { message: "Unauthorized: not for you".into(), extensions: vec![] }
                .into()
        };

        // orders.byRestaurant admits [CUSTOMER, RESTAURANT, RESTAURANT_ACCOUNT, ADMIN] — never
        // PUBLIC — so on the anonymous path EVERY error variant is a skip by design.
        assert!(!ResolverKey::OrdersByRestaurant.roles().contains(&Role::Public.user_type()));
        assert!(matches!(
            classify_resolve(Role::Public, ResolverKey::OrdersByRestaurant, Err(network())),
            ResolveOutcome::SkippedByDesign(SkipReason::RoleRefused)
        ), "role outside roles(): even a NETWORK error is a skip, not a failure");
        assert!(matches!(
            classify_resolve(Role::Public, ResolverKey::OrdersByRestaurant, Err(auth_errors())),
            ResolveOutcome::SkippedByDesign(SkipReason::RoleRefused)
        ));

        // cart.current admits [PUBLIC, CUSTOMER] — so on the SAME anonymous path every error
        // variant is a REAL failure, including one whose text screams authorization.
        assert!(ResolverKey::CartCurrent.roles().contains(&Role::Public.user_type()));
        assert!(matches!(
            classify_resolve(Role::Public, ResolverKey::CartCurrent, Err(auth_errors())),
            ResolveOutcome::Failed(_)
        ), "role inside roles(): even an 'Unauthorized' Errors payload is a failure, not a skip");
        assert!(matches!(
            classify_resolve(Role::Public, ResolverKey::CartCurrent, Err(network())),
            ResolveOutcome::Failed(_)
        ));

        // And the SAME resolver flips on the role alone: a CUSTOMER asking orders.byRestaurant
        // that gets a Network error is a real failure.
        assert!(matches!(
            classify_resolve(Role::Customer, ResolverKey::OrdersByRestaurant, Err(network())),
            ResolveOutcome::Failed(_)
        ));

        // A declared gap skips before any role question arises.
        assert!(matches!(
            classify_resolve(
                Role::Public,
                ResolverKey::PromotionsActive,
                Err(ResolverError::GapBinding { key: "promotions.active", note: "gap" }),
            ),
            ResolveOutcome::SkippedByDesign(SkipReason::DeclaredGap)
        ));
    }

    #[test]
    fn http_transport_builds_the_role_path_endpoint() {
        let t = HttpTransport::new("https://tours.captain.food/", Role::Public, SessionId::mint());
        assert_eq!(t.endpoint(), "https://tours.captain.food/public/graphql");
        let t = HttpTransport::new("http://127.0.0.1:8080", Role::Customer, SessionId::mint());
        assert_eq!(t.endpoint(), "http://127.0.0.1:8080/customer/graphql");
    }

    // ---- D1 (#904, ADR-20260905-101349 §13): the one-shot 401-refresh decorator ----

    use super::test_support::CountingRefresher;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    fn refreshing(
        responses: Vec<Result<Value, TransportError>>,
        refresher: CountingRefresher,
    ) -> (RefreshingTransport, Arc<CountingRefresher>) {
        let refresher = Arc::new(refresher);
        let t = RefreshingTransport::new(
            Box::new(FakeTransport::scripted(responses)),
            Box::new(Arc::clone(&refresher)),
            Arc::new(AtomicBool::new(false)),
        );
        (t, refresher)
    }

    /// Red-first (ADR-20260905-101349:171): a mutant that returns the 401 without ever calling the
    /// refresher fails this with "refresher count 0 != 1" — quoted verbatim in the hand-back.
    #[tokio::test]
    async fn refresh_once_then_reissues_the_same_request() {
        let (t, refresher) = refreshing(
            vec![
                Err(TransportError::Status { status: 401 }),
                Ok(json!({ "me": { "customerId": "c-1" } })),
            ],
            CountingRefresher::new(true),
        );
        let data = t.execute("query { me { customerId } }", json!({})).await.unwrap();
        assert_eq!(data, json!({ "me": { "customerId": "c-1" } }));
        assert_eq!(refresher.call_count(), 1, "refresher count {} != 1", refresher.call_count());
    }

    /// Red-first: a mutant that refreshes PER REQUEST (instead of once per load) fails this with
    /// "refresher count 3 != 1" once three 401 reads have gone through the same transport.
    #[tokio::test]
    async fn three_401_reads_refresh_exactly_once() {
        let (t, refresher) = refreshing(
            vec![
                Err(TransportError::Status { status: 401 }),
                Ok(json!({ "a": 1 })),
                Err(TransportError::Status { status: 401 }),
                Err(TransportError::Status { status: 401 }),
            ],
            CountingRefresher::new(true),
        );
        // First read: 401 -> refresh -> reissue -> succeeds.
        assert!(t.execute("query { a }", json!({})).await.is_ok());
        // Second and third reads: the load's one shot is already spent (`used` stays true whether
        // the FIRST refresh succeeded or not) — a second 401 is returned untouched, never retried.
        assert!(matches!(
            t.execute("query { b }", json!({})).await,
            Err(TransportError::Status { status: 401 })
        ));
        assert!(matches!(
            t.execute("query { c }", json!({})).await,
            Err(TransportError::Status { status: 401 })
        ));
        assert_eq!(refresher.call_count(), 1, "refresher count {} != 1", refresher.call_count());
    }

    /// Red-first: a mutant that refreshes on ANY non-2xx (not just a bare 401) fails this with
    /// "refresher count 1 != 0" — a 403 (a rotated token carrying the SAME role, ADR-081527 §4)
    /// must never arm a refresh, or a role-mismatch would loop forever.
    #[tokio::test]
    async fn a_403_never_refreshes() {
        let (t, refresher) = refreshing(
            vec![Err(TransportError::Status { status: 403 })],
            CountingRefresher::new(true),
        );
        assert!(matches!(
            t.execute("query { a }", json!({})).await,
            Err(TransportError::Status { status: 403 })
        ));
        assert_eq!(refresher.call_count(), 0, "refresher count {} != 0", refresher.call_count());
    }

    /// Red-first: a mutant that swallows a failed refresh into `Malformed` (or loops trying again)
    /// fails this with "error kind != Status 401" or a refresher count of 2. The caller must see
    /// the ORIGINAL 401 — never a different error, never a second attempt.
    #[tokio::test]
    async fn a_failed_refresh_returns_the_original_401_and_arms_no_second_refresh() {
        let (t, refresher) = refreshing(
            vec![
                Err(TransportError::Status { status: 401 }),
                Err(TransportError::Status { status: 401 }), // a second read, same load
            ],
            CountingRefresher::new(false),
        );
        let err = t.execute("query { a }", json!({})).await.unwrap_err();
        assert!(matches!(err, TransportError::Status { status: 401 }), "error kind != Status 401: {err:?}");
        // A second read in the same load must NOT re-arm the refresh (the failure is remembered).
        let err2 = t.execute("query { b }", json!({})).await.unwrap_err();
        assert!(matches!(err2, TransportError::Status { status: 401 }));
        assert_eq!(refresher.call_count(), 1, "refresher count {} != 1 (arming a second refresh)", refresher.call_count());
    }
}
