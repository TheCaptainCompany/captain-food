//! #639 part C step 4-i (ADR-20260904-081527 §4/§9) — the standing carve-out proved through the
//! REAL seam: `POST /rider/graphql` on the production `graphql_routes`, `AuthContext::authorize`
//! over a loopback JWKS, `resolve_read_scope` over a scripted `RiderIdentitySource` answering a
//! RESTRICTED row. Cloned from `rider_without_a_row_is_forbidden_on_the_write_half.rs` (the
//! harness this file exists to reuse against a DIFFERENT seam outcome — a row that resolves, but
//! whose standing is RESTRICTED rather than ACTIVE).
//!
//! (1) `a_restricted_rider_with_a_fresh_token_is_refused_on_accept_delivery`: the scripted seam
//! answers `Resolved { id, standing: RESTRICTED }` on `acceptDelivery` — FORBIDDEN; the SAME
//! request with the SAME token but `standing: ACTIVE` is the control — not FORBIDDEN.
//!
//! (2) `the_carve_out_admits_exactly_my_standing_delivery_report_issue_and_hand_back_while_restricted`:
//! structure-sensitive by design — the carved set IS the policy (ADR-20260904-081527 §4:
//! `{ myStanding, delivery, reportDeliveryIssue, handBackDelivery }`). Positive AND negative halves
//! in ONE test against the SAME restricted seam.

use async_trait::async_trait;
use domain::generated::scalars::{RiderId, RiderStanding};
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

/// A verified token asserting `role: RIDER` and nothing else — the row and its standing come
/// entirely from the scripted seam.
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

const ACCEPT_DELIVERY: &str = r#"mutation {
  acceptDelivery(input: {
    deliveryJobId: "00000000-0000-0000-0000-00000000000d"
  }) { messageId }
}"#;

const DECLINE_DELIVERY: &str = r#"mutation {
  declineDelivery(input: {
    deliveryJobId: "00000000-0000-0000-0000-00000000000d"
  }) { messageId }
}"#;

const CHANGE_RIDER_STATUS: &str = r#"mutation {
  changeRiderStatus(input: {
    status: AVAILABLE
  }) { messageId }
}"#;

const REPORT_DELIVERY_ISSUE: &str = r#"mutation {
  reportDeliveryIssue(input: {
    deliveryJobId: "00000000-0000-0000-0000-00000000000d",
    kind: CUSTOMER_UNREACHABLE
  }) { messageId }
}"#;

const HAND_BACK_DELIVERY: &str = r#"mutation {
  handBackDelivery(input: {
    deliveryJobId: "00000000-0000-0000-0000-00000000000d",
    foodLocation: NOT_COLLECTED
  }) { messageId }
}"#;

const MY_DELIVERIES: &str = r#"{ myDeliveries { id } }"#;
const MY_STANDING: &str = r#"{ myStanding { standing } }"#;
const DELIVERY: &str = r#"{ delivery(input: { orderId: "00000000-0000-0000-0000-00000000000e" }) { id } }"#;

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

/// The first error of a GraphQL response as `(message, extensions.code)`, or `None` when the
/// response carries no error (a data-only response — never expected here, since every schema in
/// this file carries no mailbox/read repos).
fn first_error(body: &Value) -> Option<(String, Option<String>)> {
    let err = body["errors"].as_array().and_then(|errors| errors.first())?;
    Some((
        err["message"].as_str().unwrap_or_default().to_string(),
        err["extensions"]["code"].as_str().map(str::to_string),
    ))
}

fn resolved(id: uuid::Uuid, standing: RiderStanding) -> RiderIdentityResolution {
    RiderIdentityResolution::Resolved((RiderId(id), standing))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restricted_rider_with_a_fresh_token_is_refused_on_accept_delivery() {
    let sub = uuid::Uuid::from_u128(0x6394_1);
    let jwt = bare_rider_jwt(sub);
    let rider_id = uuid::Uuid::from_u128(0x600D);

    // RESTRICTED: the StandingGuard refuses — acceptDelivery carries no `whileRestricted:`.
    let body = post_as_rider(&jwt, ACCEPT_DELIVERY, resolved(rider_id, RiderStanding::RESTRICTED)).await;
    let (message, code) = first_error(&body).expect("a restricted rider must be refused");
    assert_eq!(
        code.as_deref(),
        Some("FORBIDDEN"),
        "a RESTRICTED rider must be refused by the standing guard — got: {message}"
    );
    assert_eq!(
        message,
        "forbidden: your access is restricted",
        "and the standing guard's own message, distinct from the role guard's — got: {message}"
    );

    // THE CONTROL: the SAME token, the SAME request, ACTIVE standing — not FORBIDDEN. Proves the
    // refusal above is the standing guard's, not a blanket refusal of every rider on this door.
    let body = post_as_rider(&jwt, ACCEPT_DELIVERY, resolved(rider_id, RiderStanding::ACTIVE)).await;
    if let Some((message, code)) = first_error(&body) {
        assert_ne!(
            code.as_deref(),
            Some("FORBIDDEN"),
            "an ACTIVE rider acts as RIDER: the guard must have passed — got: {message}"
        );
    }
}

/// Structure-sensitive by design (ADR-20260904-081527 §4): the carved set IS the policy —
/// `{ myStanding, delivery, reportDeliveryIssue, handBackDelivery }`, and NOT
/// `{ changeRiderStatus, acceptDelivery, declineDelivery, myDeliveries }` (`myDeliveries` hands the
/// unassigned PENDING pool to whoever holds the session — the ACCOUNT_COMPROMISE exposure the
/// restriction closes). Positive AND negative halves against the SAME restricted seam.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_carve_out_admits_exactly_my_standing_delivery_report_issue_and_hand_back_while_restricted() {
    let rider_id = uuid::Uuid::from_u128(0x600D);
    let outcome = || resolved(rider_id, RiderStanding::RESTRICTED);

    for (name, query) in [
        ("myStanding", MY_STANDING),
        ("delivery", DELIVERY),
        ("reportDeliveryIssue", REPORT_DELIVERY_ISSUE),
        ("handBackDelivery", HAND_BACK_DELIVERY),
    ] {
        let sub = uuid::Uuid::new_v4();
        let jwt = bare_rider_jwt(sub);
        let body = post_as_rider(&jwt, query, outcome()).await;
        if let Some((message, code)) = first_error(&body) {
            assert_ne!(
                code.as_deref(),
                Some("FORBIDDEN"),
                "{name} must stay open to a RESTRICTED rider (the carve-out) — got: {message}"
            );
        }
    }

    for (name, query) in [
        ("changeRiderStatus", CHANGE_RIDER_STATUS),
        ("declineDelivery", DECLINE_DELIVERY),
        ("acceptDelivery", ACCEPT_DELIVERY),
        ("myDeliveries", MY_DELIVERIES),
    ] {
        let sub = uuid::Uuid::new_v4();
        let jwt = bare_rider_jwt(sub);
        let body = post_as_rider(&jwt, query, outcome()).await;
        let (message, code) = first_error(&body).unwrap_or_else(|| panic!("{name} must be refused for a RESTRICTED rider"));
        assert_eq!(
            code.as_deref(),
            Some("FORBIDDEN"),
            "{name} is NOT in the carve-out — a RESTRICTED rider must be refused — got: {message}"
        );
        assert_eq!(
            message,
            "forbidden: your access is restricted",
            "{name}: the standing guard's own message — got: {message}"
        );
    }
}
