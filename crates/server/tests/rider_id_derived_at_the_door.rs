//! #865 — the seam-injection proof (D2, ADR-20260904-015903 §6, PROP-171500; the `ReadScope::Rider`
//! fact realized by #849 "#639 part C step 2b" / ADR-20260830-191457 parts A+B's
//! `auth.rs::resolve_rider_scope`):
//! posting a well-formed `acceptDelivery(input: { deliveryJobId })` through the REAL edge — POST
//! /rider/graphql, a verified RIDER JWT, the scripted `RiderIdentitySource` resolving a row — lands
//! on the mailbox with `riderId` INJECTED from the caller's own `ReadScope::Rider`, never supplied
//! by the client (the field does not even exist on `AcceptDeliveryInput` any more). This is the
//! test that FAILS on the pre-#865 shape: `riderId` was a client-supplied payload field, so there
//! was nothing for a seam to inject INTO and no resolver code read `ReadScope` at all.
//!
//! Combines the two existing harnesses on purpose: the real-edge JWT/JWKS/scripted-seam rig of
//! `rider_without_a_row_is_forbidden_on_the_write_half.rs` (this is the seam that decides
//! `ReadScope`), over the `MemMailbox`-backed schema of `graphql_typed_send.rs#schema_over` (this
//! is what makes the enqueued payload observable) — neither alone proves the injection: the first
//! has no mailbox to inspect, the second has no real seam to derive FROM.

use std::sync::Arc;

use actor_client::mailbox::mem::MemMailbox;
use async_trait::async_trait;
use domain::generated::scalars::RiderId;
use serde_json::{json, Value};
use server::{
    CustomerIdentitySource, IdentitySources, MemberIdentitySource, NoDatabaseMemberIdentity,
    ResolveRiderIdentity, RiderIdentityResolution, RiderIdentitySource,
};
use tower::ServiceExt;

/// TEST-ONLY ES256 keypair — the same material as `crates/server/src/auth.rs`'s own suite and its
/// sibling rider-seam test (duplicated for the same reason stated there: `#[cfg(test)]` items in
/// the lib are invisible to an integration test).
const TEST_EC_PRIVATE_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgTCTNdGfegiVKVsm+
vZXPOa4xJAt5OT8zMSblfCEwtW2hRANCAARto0Dk75fxl2IyLx89vwvjUWkJAb/p
5bKnk8sNetDUBHLVIGpXoxBRFJVNSeDN6QB9IHl6rqDLaZR4iqLatScL
-----END PRIVATE KEY-----
";

const TEST_SUPABASE_URL: &str = "https://captain-under-test.supabase.co";

async fn jwks_endpoint() -> String {
    let body = json!({"keys":[{"kty":"EC","crv":"P-256","use":"sig","kid":"captain-test-es256",
        "alg":"ES256","x":"baNA5O-X8ZdiMi8fPb8L41FpCQG_6eWyp5PLDXrQ1AQ",
        "y":"ctUgalejEFEUlU1J4M3pAH0geXquoMtplHiKotq1Jws"}]});
    let app = axum::Router::new().route(
        "/jwks",
        axum::routing::get(move || {
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

/// A verified token asserting `role: RIDER` and nothing else — no `rider_id` claim (the product
/// parses none), no row behind the subject until the scripted seam says so.
fn bare_rider_jwt(sub: uuid::Uuid) -> String {
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
        "iss": format!("{TEST_SUPABASE_URL}/auth/v1"),
        "exp": exp,
        "app_metadata": { "captain_food": { "role": "RIDER" } },
    });
    let key = jsonwebtoken::EncodingKey::from_ec_pem(TEST_EC_PRIVATE_KEY_PEM.as_bytes())
        .expect("test EC key parses");
    jsonwebtoken::encode(&header, &claims, &key).expect("sign test JWT")
}

/// A scripted `Rider` table: one fixed outcome for every subject.
struct ScriptedRiderTable(RiderIdentityResolution);

#[async_trait]
impl ResolveRiderIdentity for ScriptedRiderTable {
    async fn resolve(&self, _auth_subject: &str) -> RiderIdentityResolution {
        self.0.clone()
    }
}

/// An `EventStore` the acceptance path must NEVER reach: the mailbox-routed resolvers enqueue and
/// answer PENDING — the worker (absent here) owns delivery (mirrors `graphql_typed_send.rs`).
struct UntouchableEventStore;

#[async_trait::async_trait]
impl application::ports::EventStore for UntouchableEventStore {
    async fn append(
        &self,
        stream_name: &str,
        _expected_version: i64,
        _events: &[domain::generated::events::DomainEvent],
        _actor: &application::ports::Actor,
    ) -> Result<i64, domain::shared::errors::DomainError> {
        panic!("the acceptance path must not append events (stream {stream_name})");
    }

    async fn load(
        &self,
        stream_name: &str,
    ) -> Result<(Vec<domain::generated::events::DomainEvent>, i64), domain::shared::errors::DomainError>
    {
        panic!("the acceptance path must not load streams (stream {stream_name})");
    }
}

/// A `SlugReservationRepository` that grants every request — this test never configures a slug;
/// the field only has to be inhabited (mirrors `graphql_typed_send.rs`).
struct AlwaysFreeSlugs;

#[async_trait::async_trait]
impl application::queries::SlugReservationRepository for AlwaysFreeSlugs {
    async fn reserve(
        &self,
        _slug: domain::generated::scalars::Slug,
        _restaurant_id: domain::generated::scalars::RestaurantId,
    ) -> Result<bool, domain::shared::errors::DomainError> {
        Ok(true)
    }
    async fn release(
        &self,
        _slug: domain::generated::scalars::Slug,
        _restaurant_id: domain::generated::scalars::RestaurantId,
    ) -> Result<(), domain::shared::errors::DomainError> {
        Ok(())
    }
}

/// The production router: the real `graphql_routes` behind the real JWT verifier, the rider seam
/// scripted to `outcome`, over a `MemMailbox`-backed write side — the harness this test exists to
/// combine (see the module doc).
fn schema_over(mailbox: Arc<dyn actor_client::mailbox::Mailbox>) -> server::graphql_schema::CaptainSchema {
    server::graphql_schema::build_schema(
        None,
        Some(server::graphql_schema::WriteDeps {
            event_store: Arc::new(UntouchableEventStore),
            ownership: Arc::new(infrastructure::FailClosedGoogleOwnershipVerifier),
            gbp_probe: Arc::new(infrastructure::UnverifiedGbpOrderLinkProbe),
            auth_provider: Arc::new(infrastructure::FailClosedIdentityService),
            payments: Arc::new(infrastructure::FailClosedPaymentGateway),
            pm_state: Arc::new(application::generated::pm_state::mem::MemPaymentProcessState::default()),
            refund_state: Arc::new(application::generated::pm_state::mem::MemRefundProcessState::default()),
            mailbox,
            status_bus: actor_client::OperationStatusBus::default(),
            auth_sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
            slug_reservations: Arc::new(AlwaysFreeSlugs),
        }),
        None,
    )
}

async fn router(outcome: RiderIdentityResolution, mailbox: Arc<dyn actor_client::mailbox::Mailbox>) -> axum::Router {
    let schema = schema_over(mailbox);
    server::graphql_routes(
        schema,
        server::TenantLookup(None),
        IdentitySources {
            customer: CustomerIdentitySource::Claim,
            rider: RiderIdentitySource::new(std::sync::Arc::new(ScriptedRiderTable(outcome))),
            member: MemberIdentitySource::new(std::sync::Arc::new(NoDatabaseMemberIdentity)),
        },
    )
    .layer(axum::Extension(server::AuthContext::from_config(
        jwks_endpoint().await,
        TEST_SUPABASE_URL.into(),
    )))
}

/// `riderId` carries no field on `AcceptDeliveryInput` (#865, `derived: { riderId: rider }`) — the
/// literal supplies ONLY `deliveryJobId`, a fixed `metadata.messageId` so the mailbox row is
/// keyed and retrievable.
const MESSAGE_ID: &str = "00000000-0000-0000-0000-0000000000aa";
fn accept_delivery_mutation() -> String {
    format!(
        r#"mutation {{ acceptDelivery(input: {{ deliveryJobId: "00000000-0000-0000-0000-00000000000d" }}, metadata: {{ messageId: "{MESSAGE_ID}" }}) {{ messageId operationStatus }} }}"#
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_seam_injects_riderid_into_the_enqueued_payload() {
    let sub = uuid::Uuid::from_u128(0x865_01);
    let jwt = bare_rider_jwt(sub);
    let rider_id = uuid::Uuid::from_u128(0x600D_1D);
    let mem = Arc::new(MemMailbox::default());

    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/rider/graphql")
        .header(axum::http::header::HOST, "chez-test.captain.food")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(axum::body::Body::from(json!({ "query": accept_delivery_mutation() }).to_string()))
        .expect("request builds");

    let response = router(RiderIdentityResolution::Resolved((RiderId(rider_id), domain::generated::scalars::RiderStanding::ACTIVE)), mem.clone())
        .await
        .oneshot(request)
        .await
        .expect("router answers");
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("a GraphQL response body");
    let errors_empty = body["errors"].as_array().map(|e| e.is_empty()).unwrap_or(true);
    assert!(errors_empty, "the resolved rider's acceptDelivery must not error: {body}");
    assert_eq!(body["data"]["acceptDelivery"]["operationStatus"], "PENDING");

    let message_id = uuid::Uuid::parse_str(MESSAGE_ID).expect("fixed uuid");
    let row = mem.entry(message_id).expect("one mailbox row keyed by the supplied messageId");
    let payload = row.payload();
    assert_eq!(
        payload.get("riderId").and_then(|v| v.as_str()),
        Some(rider_id.to_string().as_str()),
        "the enqueued payload's riderId must be the SEAM's resolved rider, never a client-suppliable \
         value the literal never carried -- payload: {payload}"
    );
    assert_eq!(payload.get("deliveryJobId").and_then(|v| v.as_str()), Some("00000000-0000-0000-0000-00000000000d"));
    assert_eq!(
        row.payload_hash(),
        application::journal::payload_hash(payload),
        "payload_hash is taken from the TYPED command AFTER injection -- a hash over the \
         pre-injection client form (missing riderId) would not match the stored payload"
    );
}

/// Fail-closed control: NO `ReadScope::Rider` (the seam answered `NoMapping`) enqueues NOTHING —
/// the `RoleGuard` refuses the caller AS PUBLIC before the resolver's derived-field injection code
/// ever runs, so the mailbox stays empty (never a Public default that enqueues with no rider).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_resolved_rider_scope_enqueues_nothing() {
    let sub = uuid::Uuid::from_u128(0x865_02);
    let jwt = bare_rider_jwt(sub);
    let mem = Arc::new(MemMailbox::default());

    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/rider/graphql")
        .header(axum::http::header::HOST, "chez-test.captain.food")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(axum::body::Body::from(json!({ "query": accept_delivery_mutation() }).to_string()))
        .expect("request builds");

    let response = router(RiderIdentityResolution::NoMapping, mem.clone())
        .await
        .oneshot(request)
        .await
        .expect("router answers");
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("a GraphQL response body");
    let code = body["errors"][0]["extensions"]["code"].as_str().map(str::to_string);
    assert_eq!(code.as_deref(), Some("FORBIDDEN"), "no row -> the role guard refuses: {body}");
    assert!(mem.entries().is_empty(), "a refused caller must enqueue nothing at all");
}

/// Review round 2, BLOCKING: the REQUIRED-derived seam's OWN fail-closed branch
/// (`let Some(ReadScope::Rider(__derived_id)) = __derived_scope else { return
/// Err(forbidden_error()) }`) had no test that reached it. `no_resolved_rider_scope_enqueues_nothing`
/// uses `NoMapping`, which the `RoleGuard` refuses AS PUBLIC before the resolver body ever runs —
/// that proves the GUARD, not this branch. This test binds `ActingRole` to RIDER directly through
/// `schema.execute` (no HTTP transport, so no `ReadScope` is ever inserted into the context at
/// all) — the guard passes (the caller genuinely acts as RIDER), the resolver body runs, and the
/// ONLY thing left to refuse the call is the derived seam's own `else` branch reading
/// `ctx.data_opt::<ReadScope>() == None`. Seen RED first: temporarily mutating the emitter's
/// `required` branch to inject a nil UUID instead of erroring (regenerated, reverted) made this
/// test fail with `mem.entries()` non-empty and `resp.errors` empty — the exact failure this
/// gate exists to catch. Failure text recorded verbatim in the PR/journal, not repeated here (a
/// stale copy would drift from the current assertion).
#[tokio::test]
async fn a_rider_bound_caller_with_no_readscope_in_context_is_refused_by_the_derived_seam_itself() {
    let mem = Arc::new(MemMailbox::default());
    let schema = schema_over(mem.clone());
    let acting = server::Principal::role_binding(
        server::graphql_acl::RequestRole::Rider,
        "no-scope-test".to_string(),
        Some(uuid::Uuid::from_u128(0x639)),
    )
    .acting_role(server::graphql_acl::RequestRole::Rider);

    let resp = schema.execute(async_graphql::Request::new(accept_delivery_mutation()).data(acting)).await;

    assert_eq!(resp.errors.len(), 1, "expected exactly the derived seam's own refusal: {:?}", resp.errors);
    let ext = resp.errors[0].extensions.as_ref().expect("extensions");
    assert_eq!(
        ext.get("code"),
        Some(&async_graphql::Value::from("Forbidden")),
        "the REQUIRED derived property's OWN fail-closed branch (errors.yaml#/Forbidden, PascalCase \
         -- distinct from the role guard's literal FORBIDDEN, which already passed) -- got: {:?}",
        resp.errors[0]
    );
    assert!(mem.entries().is_empty(), "the derived seam's fail-closed branch must enqueue nothing");
}

/// The smuggled-field mutant: a client posts `riderId` alongside the well-formed
/// `deliveryJobId`, BOTH inline in the query text and via GraphQL `variables`. Executed through
/// `schema.execute` directly (never `Input::parse`, whose serde derive silently IGNORES unknown
/// keys and would let this smuggling through unnoticed) — `AcceptDeliveryInput` no longer
/// declares the field at all (#865), so async-graphql's OWN document validation refuses BEFORE
/// the role guard or any resolver code runs, on EITHER leg, and nothing is ever enqueued.
#[tokio::test]
async fn a_smuggled_riderid_is_refused_by_schema_validation_inline_and_via_variables() {
    let acting = |role: server::graphql_acl::RequestRole| {
        server::Principal::role_binding(role, "smuggle-test".to_string(), Some(uuid::Uuid::from_u128(0x639)))
            .acting_role(role)
    };

    // Leg 1 -- inline literal.
    let mem = Arc::new(MemMailbox::default());
    let schema = schema_over(mem.clone());
    let inline = r#"mutation { acceptDelivery(input: {
        deliveryJobId: "00000000-0000-0000-0000-00000000000d",
        riderId: "00000000-0000-0000-0000-000000000bad"
    }) { messageId } }"#;
    let resp = schema
        .execute(async_graphql::Request::new(inline).data(acting(server::graphql_acl::RequestRole::Rider)))
        .await;
    assert_eq!(resp.errors.len(), 1, "expected exactly the validation refusal: {:?}", resp.errors);
    let message = resp.errors[0].message.clone();
    assert!(
        message.contains("riderId") && message.contains("AcceptDeliveryInput"),
        "expected async-graphql's own unknown-field refusal naming both -- got verbatim: {message}"
    );
    assert!(mem.entries().is_empty(), "a validation-refused document must never enqueue -- got: {message}");

    // Leg 2 -- via variables.
    let mem = Arc::new(MemMailbox::default());
    let schema = schema_over(mem.clone());
    let via_vars = "mutation($input: AcceptDeliveryInput!) { acceptDelivery(input: $input) { messageId } }";
    let variables = async_graphql::Variables::from_json(json!({
        "input": {
            "deliveryJobId": "00000000-0000-0000-0000-00000000000d",
            "riderId": "00000000-0000-0000-0000-000000000bad"
        }
    }));
    let resp = schema
        .execute(
            async_graphql::Request::new(via_vars)
                .variables(variables)
                .data(acting(server::graphql_acl::RequestRole::Rider)),
        )
        .await;
    assert_eq!(resp.errors.len(), 1, "expected exactly the validation refusal: {:?}", resp.errors);
    let message = resp.errors[0].message.clone();
    assert!(
        message.contains("riderId"),
        "expected async-graphql's own unknown-field refusal on the variables leg -- got verbatim: {message}"
    );
    assert!(mem.entries().is_empty(), "a validation-refused document must never enqueue -- got: {message}");
}

/// #639 part C step 4-i (ADR-20260904-081527 §6): `ChangeRiderStatus.status` is
/// `RiderAvailabilityTarget` (OFFLINE/AVAILABLE/ON_DELIVERY), NOT the stored `RiderStatus` —
/// `SUSPENDED` is unspellable at this door BY THE ENUM ITSELF, so async-graphql's own document
/// validation refuses BEFORE the role guard or any resolver runs, on EITHER leg (inline literal,
/// GraphQL variables), UNIFORMLY across every role (no `extensions.code` at all — the #865 trap:
/// a static-validation refusal is indistinguishable from a role guard's FORBIDDEN only by the
/// ABSENCE of a code, so this test asserts that absence explicitly rather than assuming it).
/// Structure-sensitive by design: the closed enum IS the policy.
#[tokio::test]
async fn a_suspended_status_is_unspellable_on_change_rider_status_inline_and_via_variables() {
    let acting = |role: server::graphql_acl::RequestRole| {
        server::Principal::role_binding(role, "suspended-unspellable-test".to_string(), Some(uuid::Uuid::from_u128(0x639)))
            .acting_role(role)
    };

    // Leg 1 -- inline literal, RIDER (the door's own role).
    let mem = Arc::new(MemMailbox::default());
    let schema = schema_over(mem.clone());
    let inline = r#"mutation { changeRiderStatus(input: { status: SUSPENDED }) { messageId } }"#;
    let resp = schema
        .execute(async_graphql::Request::new(inline).data(acting(server::graphql_acl::RequestRole::Rider)))
        .await;
    assert_eq!(resp.errors.len(), 1, "expected exactly the validation refusal: {:?}", resp.errors);
    let message = resp.errors[0].message.clone();
    assert!(
        message.contains("SUSPENDED"),
        "expected async-graphql's own unknown-enum-value refusal naming the value -- got verbatim: {message}"
    );
    assert!(
        resp.errors[0].extensions.is_none(),
        "a STATIC validation refusal carries no extensions.code at all -- got: {:?}",
        resp.errors[0]
    );
    assert!(mem.entries().is_empty(), "a validation-refused document must never enqueue -- got: {message}");

    // Leg 2 -- via variables.
    let mem = Arc::new(MemMailbox::default());
    let schema = schema_over(mem.clone());
    let via_vars = "mutation($input: ChangeRiderStatusInput!) { changeRiderStatus(input: $input) { messageId } }";
    let variables = async_graphql::Variables::from_json(json!({ "input": { "status": "SUSPENDED" } }));
    let resp = schema
        .execute(
            async_graphql::Request::new(via_vars)
                .variables(variables)
                .data(acting(server::graphql_acl::RequestRole::Rider)),
        )
        .await;
    assert_eq!(resp.errors.len(), 1, "expected exactly the validation refusal: {:?}", resp.errors);
    let message = resp.errors[0].message.clone();
    assert!(
        message.contains("SUSPENDED"),
        "expected async-graphql's own unknown-enum-value refusal on the variables leg -- got verbatim: {message}"
    );
    assert!(mem.entries().is_empty(), "a validation-refused document must never enqueue -- got: {message}");

    // UNIFORM across roles: the refusal is the same for a role this door does not even list
    // (RESTAURANT) -- proving it is the ENUM, never the role guard, that refuses.
    let mem = Arc::new(MemMailbox::default());
    let schema = schema_over(mem.clone());
    let resp = schema
        .execute(async_graphql::Request::new(inline).data(acting(server::graphql_acl::RequestRole::Restaurant)))
        .await;
    assert_eq!(resp.errors.len(), 1, "expected exactly the validation refusal: {:?}", resp.errors);
    assert!(
        resp.errors[0].extensions.is_none(),
        "still no extensions.code for a role this door does not list -- got: {:?}",
        resp.errors[0]
    );
}
