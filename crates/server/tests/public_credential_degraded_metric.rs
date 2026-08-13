//! #469 review round 2: the S3 re-route PROVED, not argued.
//!
//! The whole S3 decision is *which counter fires* when a verified CUSTOMER token carries no
//! `captain_food.customer_id` — the pre-claim-stamp window, i.e. every signed-in customer for one token
//! lifetime after rollout. It must be `public_credential_degraded_total{reason=claim_absent}` (a
//! visible degrade) and NOT `read_authorization_bridge_unresolved_total` (whose contract says
//! *"a provisioning gap or staleness — never ordinary user denial"*, and which an operator reads as
//! an incident). Until this test, neither branch was observed anywhere: the only occurrences of
//! either name in the tree were the emit site and the constant.
//!
//! The proof is deliberately BOTH-WAYS and non-vacuous: the SAME token is presented to `/public`
//! (degrade, no bridge counter) and to `/customer` (bridge counter fires). An "it stays zero"
//! assertion whose metric name is simply wrong would pass the first half and fail the second.
//!
//! Its OWN test binary, for the reason `checkout_degraded_metric.rs` states: `telemetry::meters`
//! binds `opentelemetry::global::meter` once per process (`OnceLock`), so the spy provider must be
//! installed before the process's FIRST metric call — a guarantee the parallel in-crate harness
//! cannot give. One process, one provider, one test, deterministic counts.

use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, SdkMeterProvider};
use serde_json::json;
use tower::ServiceExt;

/// TEST-ONLY ES256 keypair — the same material as `crates/server/src/auth.rs` and
/// `graphql_cart_read.rs`, duplicated for the reason stated there: `#[cfg(test)]` items in the lib
/// are invisible to an integration test, and the alternatives put a signing fixture in the crate's
/// PUBLIC API to save four lines. Never a deployed key.
const TEST_EC_PRIVATE_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgTCTNdGfegiVKVsm+
vZXPOa4xJAt5OT8zMSblfCEwtW2hRANCAARto0Dk75fxl2IyLx89vwvjUWkJAb/p
5bKnk8sNetDUBHLVIGpXoxBRFJVNSeDN6QB9IHl6rqDLaZR4iqLatScL
-----END PRIVATE KEY-----
";

/// The public half of that key, served as the JWKS the verifier fetches.
async fn jwks_endpoint() -> String {
    let body = json!({"keys":[{"kty":"EC","crv":"P-256","use":"sig","kid":"captain-test-es256",
        "alg":"ES256","x":"baNA5O-X8ZdiMi8fPb8L41FpCQG_6eWyp5PLDXrQ1AQ",
        "y":"ctUgalejEFEUlU1J4M3pAH0geXquoMtplHiKotq1Jws"}]});
    let app = axum::Router::new()
        .route("/jwks", axum::routing::get(move || {
            let body = body.clone();
            async move { axum::Json(body) }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}/jwks")
}

/// OUR identity project. `iss` is MANDATORY since #519 -- this fixture used to mint tokens with no
/// `iss` at all, against a verifier built with an empty `SUPABASE_URL`, which is precisely the
/// configuration in which a staging or sibling-product token verified in production.
const TEST_SUPABASE_URL: &str = "https://captain-under-test.supabase.co";

/// A Supabase-shaped CUSTOMER token with **no** `captain_food.customer_id`: exactly the credential
/// every already-signed-in customer's browser holds for one token lifetime after the claim stamp
/// ships.
fn claimless_customer_jwt(sub: uuid::Uuid) -> String {
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
        "app_metadata": { "captain_food": { "role": "CUSTOMER" } },
    });
    let key = jsonwebtoken::EncodingKey::from_ec_pem(TEST_EC_PRIVATE_KEY_PEM.as_bytes())
        .expect("test EC key parses");
    jsonwebtoken::encode(&header, &claims, &key).expect("sign test JWT")
}

/// The production router: the real `graphql_routes` behind the real JWT verifier.
async fn router() -> axum::Router {
    server::graphql_routes(
        server::graphql_schema::build_schema(None, None, None),
        server::TenantLookup(None),
    )
    .layer(axum::Extension(server::AuthContext::from_config(
        jwks_endpoint().await,
        TEST_SUPABASE_URL.into(),
    )))
}

/// One real request through the edge: `POST /{role}/graphql`, the credential delivered the way a
/// browser delivers it (the `captain_auth` cookie, no Authorization header).
async fn post(role: &str, jwt: &str) -> axum::http::StatusCode {
    let request = axum::http::Request::builder()
        .method("POST")
        .uri(format!("/{role}/graphql"))
        .header(axum::http::header::HOST, "chez-test.captain.food")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header(axum::http::header::COOKIE, format!("captain_auth={jwt}"))
        .body(axum::body::Body::from(json!({ "query": "{ __typename }" }).to_string()))
        .expect("request builds");
    router().await.oneshot(request).await.expect("router answers").status()
}

/// Every data point of `metric` in the LATEST export, as `(attribute value, count)`. Reading the
/// last batch keeps the assertions valid whichever temporality the SDK defaults to.
fn points(exporter: &InMemoryMetricExporter, metric_name: &str, key: &str) -> Vec<(String, u64)> {
    let batches = exporter.get_finished_metrics().expect("finished metrics");
    let Some(latest) = batches.last() else { return Vec::new() };
    let mut out = Vec::new();
    for scope in latest.scope_metrics() {
        for metric in scope.metrics() {
            if metric.name() != metric_name {
                continue;
            }
            let AggregatedMetrics::U64(MetricData::Sum(sum)) = metric.data() else {
                panic!("a defect counter aggregates as a u64 Sum: {:?}", metric.data());
            };
            for dp in sum.data_points() {
                let label = dp
                    .attributes()
                    .find(|kv| kv.key.as_str() == key)
                    .map(|kv| kv.value.to_string())
                    .unwrap_or_default();
                out.push((label, dp.value()));
            }
        }
    }
    out.sort();
    out
}

const DEGRADED: &str = "public_credential_degraded_total";
const BRIDGE_UNRESOLVED: &str = "read_authorization_bridge_unresolved_total";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_pre_claim_window_counts_as_a_degrade_on_the_open_path_and_as_a_gap_on_the_role_path() {
    // The spy provider FIRST — before any request can bind the process-wide meter.
    let exporter = InMemoryMetricExporter::default();
    let provider = SdkMeterProvider::builder().with_periodic_exporter(exporter.clone()).build();
    opentelemetry::global::set_meter_provider(provider.clone());

    let jwt = claimless_customer_jwt(uuid::Uuid::from_u128(0xA3));

    // 1. THE OPEN PATH. The storefront's own request shape: a verified CUSTOMER credential with no
    //    domain claim. It is served — `/public` never refuses — and the degrade is COUNTED.
    assert_eq!(post("public", &jwt).await, axum::http::StatusCode::OK, "/public serves 200");
    provider.force_flush().expect("flush the spy reader");
    assert_eq!(
        points(&exporter, DEGRADED, "reason"),
        vec![("claim_absent".to_string(), 1)],
        "exactly ONE degrade, with the contract's canonical reason"
    );
    assert_eq!(
        points(&exporter, BRIDGE_UNRESOLVED, "role"),
        Vec::new(),
        "the provisioning-gap alarm stays SILENT: the open path denied nobody, and a normal \
         rollout must not page an operator on every storefront request"
    );

    // 2. THE ROLE PATH, same token. Here the caller IS denied something for want of a domain
    //    binding, so the bridge counter is exactly right — and its firing is what makes the
    //    assertion above a real observation rather than a misspelled metric name.
    assert_eq!(post("customer", &jwt).await, axum::http::StatusCode::OK, "/customer authorizes");
    provider.force_flush().expect("flush the spy reader");
    assert_eq!(
        points(&exporter, BRIDGE_UNRESOLVED, "role"),
        vec![("CUSTOMER".to_string(), 1)],
        "the SAME credential on a role path is a provisioning gap, counted as one"
    );
    assert_eq!(
        points(&exporter, DEGRADED, "reason"),
        vec![("claim_absent".to_string(), 1)],
        "and the open-path degrade did not double-count"
    );
}
