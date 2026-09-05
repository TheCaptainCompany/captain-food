//! #639 part C step 6-v (ADR-20260905-223957 §2, the #849/rider-without-a-row re-presentation
//! shape, transposed to ADMIN): a signed ES256 `role: ADMIN` JWT whose subject holds NO live
//! `PlatformMembership` grant must be FORBIDDEN on the write half — `grantPlatformAccess`'s
//! `RoleGuard::new(ALLOW_ADMIN)` — proved through the REAL seam: `POST /admin/graphql` on the
//! production `graphql_routes`, `AuthContext::authorize` over a loopback JWKS,
//! `resolve_read_scope` over a scripted `PlatformIdentitySource`. No hand-injected scope, no
//! hand-built witness: the only `ActingRole` in play is the one the edge mints for this caller.
//!
//! Before this seam existed, `RequestRole::Admin` minted `Identity::Admin { sub }` straight from
//! the token's claimed role (`crates/server/src/auth.rs:~308`) — a role claim alone, with no
//! binding row, was the only thing between an anonymous token and every ADMIN operation
//! (`ReadScope::Admin` is `ScopePredicate::All`). This test is RED against that shape and GREEN
//! against the seam this slice lands: an ADMIN token is `Identity::Unbound` (acts PUBLIC, reads
//! `Public`) until `resolve_platform_scope` answers a live grant row.
//!
//! Asserted as a PAIR (the `graphql_acl.rs`/rider-without-a-row posture): the SAME request through
//! a seam that answers a row is NOT forbidden — the guard passes, past it into the (here, absent)
//! mailbox.

use async_trait::async_trait;
use serde_json::{json, Value};
use server::{
    CustomerIdentitySource, IdentitySources, LookupFailureReason, MemberIdentitySource,
    NoDatabaseMemberIdentity, NoDatabaseRiderIdentity, PlatformIdentityResolution,
    PlatformIdentitySource, ResolvePlatformIdentity,
    RiderIdentitySource,
};
use tower::ServiceExt;

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

/// A verified token asserting `role: ADMIN` and NOTHING else -- ADMIN carries no domain claim by
/// design, so this is the ENTIRE shape a real hand-provisioned admin's token carries too.
fn bare_admin_jwt(sub: uuid::Uuid) -> String {
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
        "app_metadata": { "captain_food": { "role": "ADMIN" } },
    });
    let key = jsonwebtoken::EncodingKey::from_ec_pem(TEST_EC_PRIVATE_KEY_PEM.as_bytes())
        .expect("test EC key parses");
    jsonwebtoken::encode(&header, &claims, &key).expect("sign test JWT")
}

/// A scripted platform-grant seam: one fixed outcome for every subject, and a counter proving
/// exactly how many times the bridge was actually consulted (the enumeration/no-oracle shape
/// `member-sign-in`'s own tests use, transposed: here the count matters for the "never consulted
/// on a non-ADMIN request" assertion, not for enumeration).
struct ScriptedPlatformMembers {
    outcome: PlatformIdentityResolution,
    calls: std::sync::atomic::AtomicUsize,
}

impl ScriptedPlatformMembers {
    fn new(outcome: PlatformIdentityResolution) -> Self {
        Self { outcome, calls: std::sync::atomic::AtomicUsize::new(0) }
    }
}

#[async_trait]
impl ResolvePlatformIdentity for ScriptedPlatformMembers {
    async fn resolve(&self, _auth_subject: &str) -> PlatformIdentityResolution {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.outcome.clone()
    }
}

/// The production router: the real `graphql_routes` behind the real JWT verifier, the platform
/// seam scripted to `outcome`. Returns the scripted seam so its call counter can be inspected.
async fn router(
    outcome: PlatformIdentityResolution,
) -> (axum::Router, std::sync::Arc<ScriptedPlatformMembers>) {
    let scripted = std::sync::Arc::new(ScriptedPlatformMembers::new(outcome));
    let app = server::graphql_routes(
        server::graphql_schema::build_schema(None, None, None),
        server::TenantLookup(None),
        IdentitySources {
            customer: CustomerIdentitySource::Claim,
            rider: RiderIdentitySource::new(std::sync::Arc::new(NoDatabaseRiderIdentity)),
            member: MemberIdentitySource::new(std::sync::Arc::new(NoDatabaseMemberIdentity)),
            platform: PlatformIdentitySource::new(scripted.clone()),
        },
    )
    .layer(axum::Extension(server::AuthContext::from_config(
        jwks_endpoint().await,
        TEST_SUPABASE_URL.into(),
    )));
    (app, scripted)
}

/// `grantPlatformAccess` (`RoleGuard::new(ALLOW_ADMIN)`) -- an ADMIN-only door this SAME slice
/// adds, exercised here purely as the guard's target (the schema carries no mailbox, so a passed
/// guard fails past it, which is the control half of the pair).
const GRANT_PLATFORM_ACCESS: &str = r#"mutation {
  grantPlatformAccess(input: {
    platformMembershipId: "00000000-0000-0000-0000-00000000000a"
    authSubject: "auth-new-admin"
    basis: CAPTAIN_ONBOARDING
  }) { messageId }
}"#;

async fn post_as_admin(jwt: &str, outcome: PlatformIdentityResolution) -> (Value, usize) {
    let (app, scripted) = router(outcome).await;
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/admin/graphql")
        .header(axum::http::header::HOST, "chez-test.captain.food")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(axum::body::Body::from(json!({ "query": GRANT_PLATFORM_ACCESS }).to_string()))
        .expect("request builds");
    let response = app.oneshot(request).await.expect("router answers");
    assert_eq!(
        response.status(),
        axum::http::StatusCode::OK,
        "a verified ADMIN token authorizes on /admin -- the refusal under test is the GUARD's, a \
         GraphQL error, never a transport 401/403"
    );
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("a GraphQL response body");
    (body, scripted.calls.load(std::sync::atomic::Ordering::SeqCst))
}

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
async fn an_admin_token_with_no_platform_grant_is_unbound() {
    let sub = uuid::Uuid::from_u128(0x639_6D);
    let jwt = bare_admin_jwt(sub);

    // NO GRANT: the seam answers `NoMapping`. `Identity::Admin` is unspellable without a row --
    // the caller is PUBLIC on both halves, so the ADMIN guard refuses before the resolver runs.
    let (body, calls) = post_as_admin(&jwt, PlatformIdentityResolution::NoMapping).await;
    assert_eq!(calls, 1, "the seam consults the bridge exactly once per request");
    let (message, code) = first_error(&body);
    assert_eq!(
        code.as_deref(),
        Some("FORBIDDEN"),
        "an ADMIN token with no live PlatformMembership grant must be refused by the role guard, \
         not reach the resolver -- got: {message}"
    );
    assert_eq!(
        message,
        "forbidden: role PUBLIC is not authorized for this operation (allowed: ADMIN)",
        "and refused AS PUBLIC: the witness is minted from the seam's outcome, not from the role \
         the token asserts -- the exact hole a role-only mint would reopen"
    );

    // THE SEAM CANNOT ANSWER: `LookupFailed` fails closed identically at this boundary.
    let (body, _) = post_as_admin(
        &jwt,
        PlatformIdentityResolution::LookupFailed(LookupFailureReason::Repository),
    )
    .await;
    let (message, code) = first_error(&body);
    assert_eq!(code.as_deref(), Some("FORBIDDEN"), "a seam outage never widens: {message}");
    assert_eq!(
        message,
        "forbidden: role PUBLIC is not authorized for this operation (allowed: ADMIN)",
        "a lookup failure denies as PUBLIC -- got: {message}"
    );

    // A LIVE GRANT: the SAME token, the SAME request, and the guard passes -- the resolver runs
    // and fails on the mailbox this schema does not carry. The control that keeps the two
    // refusals above honest: the ADMIN path is not blanket-refused, the seam's outcome decides.
    let (body, _) = post_as_admin(&jwt, PlatformIdentityResolution::Resolved(())).await;
    let (message, code) = first_error(&body);
    assert_ne!(
        code.as_deref(),
        Some("FORBIDDEN"),
        "an admin the seam RESOLVED acts as ADMIN: the guard must have passed -- got: {message}"
    );
    assert!(
        !message.starts_with("forbidden:"),
        "and the failure that remains is the resolver's, past the guard -- got: {message}"
    );
}

/// The bridge is consulted on the ADMIN path and NOWHERE else: a CUSTOMER-role token never even
/// looks at the `PlatformMember` seam (there is no cross-role leak to close).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_bridge_is_not_consulted_for_a_non_admin_request() {
    let (app, scripted) = router(PlatformIdentityResolution::NoMapping).await;
    // The open path never carries an Authorization header at all -- an anonymous storefront read.
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/public/graphql")
        .header(axum::http::header::HOST, "chez-test.captain.food")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            json!({ "query": "query { __typename }" }).to_string(),
        ))
        .expect("request builds");
    let response = app.oneshot(request).await.expect("router answers");
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        scripted.calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "an anonymous /public request must never consult the PlatformMember bridge"
    );
}
