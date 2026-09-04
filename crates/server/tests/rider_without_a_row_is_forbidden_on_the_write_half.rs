//! #639 part C step 2b — the re-presentation of PR #849 (the independent reviewer's ONE blocking
//! finding): a signed ES256 `role: RIDER` JWT whose subject has NO `Rider` row must be FORBIDDEN on
//! the write half — `acceptDelivery`'s `RoleGuard::new(ALLOW_RIDER)` — proved through the REAL
//! seam: `POST /rider/graphql` on the production `graphql_routes`, `AuthContext::authorize` over a
//! loopback JWKS, `resolve_read_scope` over a scripted no-row `RiderIdentitySource`. No
//! hand-injected scope, no hand-built witness: the only `ActingRole` in play is the one the edge
//! mints for this caller.
//!
//! Why this test exists: as first pushed, 2b minted the `ActingRole` from `Identity::Rider`
//! BEFORE the seam resolved, so a bare RIDER token with no row read `Public` and still ACTED as
//! RIDER on every `ALLOW_RIDER` guard — and `AcceptDelivery` took its target from the CLIENT
//! PAYLOAD (`deliveryJobId`, `riderId`), never from the caller, so that token could enqueue an
//! acceptance naming any rider, with `RIDER` stamped as its author in `domain_events.user_type`.
//! That is the exact shape ADR-20260830-191457 closed for RESTAURANT (unbound => PUBLIC on both
//! halves; ADR-20260818-101500: `Identity::Unbound` denies on the money path and never stamps a
//! role). **#865** closed the OTHER half of the same hole: `riderId` is no longer a client input
//! at all — `AcceptDeliveryInput` carries no such field — it is `derived:` from THIS SAME seam's
//! `ReadScope::Rider` at the resolver, so the guard below and the derived-field injection are two
//! readings of the identical resolution: a bare token with no row is `ReadScope::Public`, which
//! the `RoleGuard` refuses before the injection code ever runs.
//!
//! Seen RED against `a2fcb93f` (the HEAD the finding was raised on) before the runtime changed —
//! the failure text is in the PR body's "Re-presentation" section.
//!
//! Asserted as a PAIR (the `graphql_acl.rs` posture): the SAME request through a seam that
//! answers a row is NOT forbidden — the guard passed and the resolver ran (and then failed on the
//! mailbox `build_schema(None, None, None)` does not provide, which is fine: it proves the guard
//! is behind us). So the refusal below is the seam's answer, not a blanket 403 on the rider path.

use async_trait::async_trait;
use domain::generated::scalars::RiderId;
use serde_json::{json, Value};
use server::{
    CustomerIdentitySource, IdentitySources, ResolveRiderIdentity, RiderIdentityResolution,
    RiderIdentitySource,
};
use tower::ServiceExt;

/// TEST-ONLY ES256 keypair — the same material as `crates/server/src/auth.rs`'s own suite,
/// duplicated for the reason stated there: `#[cfg(test)]` items in the lib are invisible to an
/// integration test, and a `Principal` can only be produced by a REAL verified token.
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

/// A verified token asserting `role: RIDER` and NOTHING else — the exact credential of the
/// finding: no `rider_id` (the product parses none anyway), no row behind the subject.
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

/// The production router: the real `graphql_routes` behind the real JWT verifier, the rider seam
/// scripted to `outcome`.
async fn router(outcome: RiderIdentityResolution) -> axum::Router {
    server::graphql_routes(
        server::graphql_schema::build_schema(None, None, None),
        server::TenantLookup(None),
        IdentitySources {
            customer: CustomerIdentitySource::Claim,
            rider: RiderIdentitySource::new(std::sync::Arc::new(ScriptedRiderTable(outcome))),
        },
    )
    .layer(axum::Extension(server::AuthContext::from_config(
        jwks_endpoint().await,
        TEST_SUPABASE_URL.into(),
    )))
}

/// The one operation under test: `acceptDelivery` (`guard = RoleGuard::new(ALLOW_RIDER)`), with a
/// well-formed input so the guard — not argument parsing — is what answers. `riderId` carries no
/// field on `AcceptDeliveryInput` at all since #865 (`derived: { riderId: rider }`): the caller's
/// identity is read from `ReadScope`, never from this literal.
const ACCEPT_DELIVERY: &str = r#"mutation {
  acceptDelivery(input: {
    deliveryJobId: "00000000-0000-0000-0000-00000000000d"
  }) { messageId }
}"#;

/// #639 part C step 3-i: the issue door, same shape — `reportDeliveryIssue` is `[RIDER, ADMIN]`.
/// `riderId` is `derived: { riderId: rider }` too (#865), NULLABLE — the resolver injects it only
/// on the RIDER path; this literal carries no such field either.
const REPORT_DELIVERY_ISSUE: &str = r#"mutation {
  reportDeliveryIssue(input: {
    deliveryJobId: "00000000-0000-0000-0000-00000000000d",
    kind: CUSTOMER_UNREACHABLE
  }) { messageId }
}"#;

/// #639 part C step 3-ii: the handback door — `handBackDelivery` is `[RIDER]`. `riderId` is
/// `derived: { riderId: rider }` too (#865): this literal carries no such field.
const HAND_BACK_DELIVERY: &str = r#"mutation {
  handBackDelivery(input: {
    deliveryJobId: "00000000-0000-0000-0000-00000000000d",
    foodLocation: NOT_COLLECTED
  }) { messageId }
}"#;

/// One real request through the edge: `POST /rider/graphql` with the bearer token, the response
/// body as JSON.
async fn post_as_rider(jwt: &str, query: &str, outcome: RiderIdentityResolution) -> Value {
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/rider/graphql")
        .header(axum::http::header::HOST, "chez-test.captain.food")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(axum::body::Body::from(json!({ "query": query }).to_string()))
        .expect("request builds");
    let response = router(outcome).await.oneshot(request).await.expect("router answers");
    assert_eq!(
        response.status(),
        axum::http::StatusCode::OK,
        "a verified RIDER token authorizes on /rider — the refusal under test is the GUARD's, a \
         GraphQL error, never a transport 401/403"
    );
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body");
    serde_json::from_slice(&bytes).expect("a GraphQL response body")
}

/// The first error of a GraphQL response as `(message, extensions.code)`.
fn first_error(body: &Value) -> (String, Option<String>) {
    let err = body["errors"]
        .as_array()
        .and_then(|errors| errors.first())
        .unwrap_or_else(|| panic!("the mutation must not succeed against an absent mailbox: {body}"));
    (
        err["message"].as_str().unwrap_or_default().to_string(),
        err["extensions"]["code"].as_str().map(str::to_string),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rider_token_with_no_rider_row_is_forbidden_on_accept_delivery() {
    let sub = uuid::Uuid::from_u128(0x639_2B);
    let jwt = bare_rider_jwt(sub);

    // NO ROW: the seam answers `NoMapping`. The caller is nobody — PUBLIC on the read half AND on
    // the write half — so the RIDER guard refuses before the resolver runs.
    let (message, code) = first_error(&post_as_rider(&jwt, ACCEPT_DELIVERY, RiderIdentityResolution::NoMapping).await);
    assert_eq!(
        code.as_deref(),
        Some("FORBIDDEN"),
        "a RIDER token with no Rider row must be refused by the role guard, not reach the \
         resolver — got: {message}"
    );
    assert_eq!(
        message,
        "forbidden: role PUBLIC is not authorized for this operation (allowed: RIDER)",
        "and refused AS PUBLIC: the witness is minted from the seam's outcome, not from the role \
         the token asserts"
    );

    // THE SEAM CANNOT ANSWER: `LookupFailed` fails closed identically at this boundary (the
    // difference is telemetry — PAGE, not OBSERVE — never authorization).
    let (message, code) = first_error(
        &post_as_rider(
            &jwt,
            ACCEPT_DELIVERY,
            RiderIdentityResolution::LookupFailed(server::LookupFailureReason::Repository),
        )
        .await,
    );
    assert_eq!(code.as_deref(), Some("FORBIDDEN"), "a seam outage never widens: {message}");
    // #639 part C step 4-i: a seam outage never reaches the StandingGuard either — refused AS
    // PUBLIC by RoleGuard first, exactly like NoMapping, never a RESTRICTED-flavoured message.
    assert_eq!(
        message,
        "forbidden: role PUBLIC is not authorized for this operation (allowed: RIDER)",
        "a lookup failure denies as PUBLIC, never as a restricted rider — got: {message}"
    );

    // A ROW: the SAME token, the SAME request, and the guard passes — the resolver runs and fails
    // on the mailbox this schema does not carry. This is the control that keeps the two refusals
    // above honest: the rider path is not blanket-refused, the seam's outcome decides.
    let (message, code) = first_error(
        &post_as_rider(&jwt, ACCEPT_DELIVERY, RiderIdentityResolution::Resolved((RiderId(uuid::Uuid::from_u128(0x600D)), domain::generated::scalars::RiderStanding::ACTIVE)))
            .await,
    );
    assert_ne!(
        code.as_deref(),
        Some("FORBIDDEN"),
        "a rider the seam RESOLVED acts as RIDER: the guard must have passed — got: {message}"
    );
    assert!(
        !message.starts_with("forbidden:"),
        "and the failure that remains is the resolver's, past the guard — got: {message}"
    );
}

/// The issue door (#639 part C step 3-i): the same bare RIDER JWT with no row is refused AS PUBLIC
/// on `reportDeliveryIssue` — the door step 4 will carve out for a restricted rider must still be
/// a door only a rider the seam resolved can knock on.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rider_token_with_no_rider_row_is_forbidden_on_report_delivery_issue() {
    let jwt = bare_rider_jwt(uuid::Uuid::from_u128(0x6393_1));
    let (message, code) =
        first_error(&post_as_rider(&jwt, REPORT_DELIVERY_ISSUE, RiderIdentityResolution::NoMapping).await);
    assert_eq!(code.as_deref(), Some("FORBIDDEN"), "no row ⇒ the guard refuses — got: {message}");
    assert_eq!(
        message,
        "forbidden: role PUBLIC is not authorized for this operation (allowed: RIDER, ADMIN)",
        "and refused AS PUBLIC, against the door's own literal roles list"
    );
    // The control: a resolved row passes the guard (and fails on the absent mailbox, past it).
    let (message, code) = first_error(
        &post_as_rider(&jwt, REPORT_DELIVERY_ISSUE, RiderIdentityResolution::Resolved((RiderId(uuid::Uuid::from_u128(0x600D)), domain::generated::scalars::RiderStanding::ACTIVE)))
            .await,
    );
    assert_ne!(code.as_deref(), Some("FORBIDDEN"), "a resolved rider acts as RIDER — got: {message}");
}

/// The handback door (#639 part C step 3-ii): the same bare RIDER JWT with no row is refused AS
/// PUBLIC on `handBackDelivery` — revocation of ACCESS is not release of CUSTODY (ADR-20260830-234532),
/// but this door only opens for a rider the seam actually resolved, same as every other write.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rider_token_with_no_rider_row_is_forbidden_on_hand_back_delivery() {
    let jwt = bare_rider_jwt(uuid::Uuid::from_u128(0x6393_2));
    let (message, code) =
        first_error(&post_as_rider(&jwt, HAND_BACK_DELIVERY, RiderIdentityResolution::NoMapping).await);
    assert_eq!(code.as_deref(), Some("FORBIDDEN"), "no row ⇒ the guard refuses — got: {message}");
    assert_eq!(
        message,
        "forbidden: role PUBLIC is not authorized for this operation (allowed: RIDER)",
        "and refused AS PUBLIC, against the door's own literal roles list"
    );
    // The control: a resolved row passes the guard (and fails on the absent mailbox, past it).
    let (message, code) = first_error(
        &post_as_rider(&jwt, HAND_BACK_DELIVERY, RiderIdentityResolution::Resolved((RiderId(uuid::Uuid::from_u128(0x600D)), domain::generated::scalars::RiderStanding::ACTIVE)))
            .await,
    );
    assert_ne!(code.as_deref(), Some("FORBIDDEN"), "a resolved rider acts as RIDER — got: {message}");
}
