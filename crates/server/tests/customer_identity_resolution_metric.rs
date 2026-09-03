//! IDENT-1 Phase A (ADR-20260818-004646, #641) — `resolve_read_scope` stops trusting the JWT
//! `captain_food.customer_id` claim under `CustomerIdentitySource::Postgres`, proved through the
//! REAL verified `Principal` (`AuthContext::authorize`, a signed JWT, no hand-assembled identity —
//! `Principal`'s constructors are module-private on purpose).
//!
//! **ONE sequential test, not several** — the `otp_refusal_region_metric.rs` / `otp_guard_liveness_metric.rs`
//! precedent: `telemetry::meters` binds `opentelemetry::global::meter` once per process behind a
//! `OnceLock`, and separate `#[tokio::test]` functions in one binary run on different threads by
//! default, so a second test's `set_meter_provider` can swap the global provider mid-flight. Own
//! test binary too, for the same reason every metrics-spy suite in this tree is its own binary.
//!
//! Covers the dispatch card's five behaviour scenarios in order:
//! (a) mapping hit -- the scope carries the POSTGRES-resolved id, proven to differ from a
//!     deliberately WRONG claim value in the fixture token (the claim is unread);
//! (b) no mapping row -- Public, `customer_identity_not_found_total` fires;
//! (c) a repository error injected through the port -- Public, `customer_identity_lookup_failed_total{reason}`
//!     fires, on a DIFFERENT counter from (b) -- the two failure classes can never collapse to one
//!     label because they are not even the same metric NAME;
//! (d) `CustomerIdentitySource::Claim` (the toggle OFF path) -- unchanged legacy behaviour, no
//!     lookup, no telemetry from this contract at all.

use async_trait::async_trait;
use domain::generated::scalars::CustomerId;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, SdkMeterProvider};
use serde_json::json;
use server::{
    graphql_acl::RequestRole, CustomerIdentityResolution, CustomerIdentitySource, IdentitySources,
    LookupFailureReason, ResolveCustomerIdentity, ResolveRiderIdentity, RiderIdentityResolution,
    RiderIdentitySource,
};

/// TEST-ONLY ES256 keypair -- the same material as `crates/server/src/auth.rs`'s own suite,
/// duplicated for the reason stated there: `#[cfg(test)]` items in the lib are invisible to an
/// integration test, and `Principal` can only be produced by a REAL verified token.
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

/// A verified CUSTOMER token carrying a `customer_id` claim -- the value the OFF path trusts, and
/// the value the ON path must NEVER read.
fn customer_jwt(sub: uuid::Uuid, claim_customer_id: uuid::Uuid) -> String {
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
        "app_metadata": { "captain_food": { "role": "CUSTOMER", "customer_id": claim_customer_id.to_string() } },
    });
    let key = jsonwebtoken::EncodingKey::from_ec_pem(TEST_EC_PRIVATE_KEY_PEM.as_bytes())
        .expect("test EC key parses");
    jsonwebtoken::encode(&header, &claims, &key).expect("sign test JWT")
}

/// A scripted seam: one fixed [`CustomerIdentityResolution`] answered to every call, regardless of
/// `auth_ref` -- deciders receive the RESULT, only this fake performs (fake) I/O.
struct ScriptedResolver(CustomerIdentityResolution);

#[async_trait]
impl ResolveCustomerIdentity for ScriptedResolver {
    async fn resolve(&self, _auth_ref: &str) -> CustomerIdentityResolution {
        self.0.clone()
    }
}

/// The RIDER seam this suite does not exercise: a table with no rows (fail closed).
struct NoRiderRows;

#[async_trait]
impl ResolveRiderIdentity for NoRiderRows {
    async fn resolve(&self, _auth_subject: &str) -> RiderIdentityResolution {
        RiderIdentityResolution::NoMapping
    }
}

/// The seams under test: the CUSTOMER one as scripted, the RIDER one inert.
fn sources(customer: CustomerIdentitySource) -> IdentitySources {
    IdentitySources { customer, rider: RiderIdentitySource::new(std::sync::Arc::new(NoRiderRows)) }
}

/// Every data point of `metric_name` in the LATEST export, as `(attribute value, count)` — the
/// `public_credential_degraded_metric.rs` reading pattern.
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

const NOT_FOUND: &str = "customer_identity_not_found_total";
const LOOKUP_FAILED: &str = "customer_identity_lookup_failed_total";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resolve_read_scope_under_postgres_mode() {
    // The spy provider FIRST -- before any request can bind the process-wide meter.
    let exporter = InMemoryMetricExporter::default();
    let provider = SdkMeterProvider::builder().with_periodic_exporter(exporter.clone()).build();
    opentelemetry::global::set_meter_provider(provider.clone());

    let auth = server::AuthContext::from_config(jwks_endpoint().await, TEST_SUPABASE_URL.into());

    // (a) MAPPING HIT: the token's claim is a WRONG id; the seam answers the RIGHT one. The
    // resolved scope must carry the RIGHT id -- proof the claim was never consulted, not just
    // "some Customer scope came back".
    let sub_a = uuid::Uuid::from_u128(0xA);
    let wrong_claim = uuid::Uuid::from_u128(0xBAD);
    let right_id = uuid::Uuid::from_u128(0x600D);
    assert_ne!(wrong_claim, right_id, "the fixture must actually disagree, or this proves nothing");
    let jwt_a = customer_jwt(sub_a, wrong_claim);
    let mut headers_a = axum::http::HeaderMap::new();
    headers_a.insert(axum::http::header::COOKIE, format!("captain_auth={jwt_a}").parse().unwrap());
    let principal_a = auth
        .authorize(RequestRole::Customer, &headers_a)
        .await
        .expect("a well-formed CUSTOMER token authorizes");
    let resolver_a = std::sync::Arc::new(ScriptedResolver(CustomerIdentityResolution::Resolved(
        CustomerId(right_id),
    )));
    let (_, scope_a) = server::resolve_read_scope(
        principal_a.clone(),
        server::graphql_session::RequestCorrelationId(uuid::Uuid::from_u128(1)),
        &sources(CustomerIdentitySource::Postgres(resolver_a)),
    )
    .await;
    assert_eq!(
        scope_a,
        application::queries::ReadScope::Customer(CustomerId(right_id)),
        "the scope must carry the POSTGRES-resolved id"
    );
    assert_ne!(
        scope_a,
        application::queries::ReadScope::Customer(CustomerId(wrong_claim)),
        "and it must NOT be the claim's id -- proof the claim went unread"
    );

    // (b) NO MAPPING ROW: an ordinary provisioning gap. Public, and ONLY the not_found counter
    // fires -- never the lookup_failed one.
    let sub_b = uuid::Uuid::from_u128(0xB);
    let jwt_b = customer_jwt(sub_b, uuid::Uuid::from_u128(0xB0B));
    let mut headers_b = axum::http::HeaderMap::new();
    headers_b.insert(axum::http::header::COOKIE, format!("captain_auth={jwt_b}").parse().unwrap());
    let principal_b = auth.authorize(RequestRole::Customer, &headers_b).await.expect("authorizes");
    let resolver_b =
        std::sync::Arc::new(ScriptedResolver(CustomerIdentityResolution::NoMapping));
    let (_, scope_b) = server::resolve_read_scope(
        principal_b,
        server::graphql_session::RequestCorrelationId(uuid::Uuid::from_u128(2)),
        &sources(CustomerIdentitySource::Postgres(resolver_b)),
    )
    .await;
    assert_eq!(scope_b, application::queries::ReadScope::Public, "no mapping row -- fails closed");
    provider.force_flush().expect("flush the spy reader");
    assert_eq!(
        points(&exporter, NOT_FOUND, "reason"),
        vec![(String::new(), 1)],
        "exactly ONE not_found -- the ordinary-provisioning-gap counter, no `reason` attribute"
    );
    assert_eq!(
        points(&exporter, LOOKUP_FAILED, "reason"),
        Vec::new(),
        "and the OUTAGE counter stays silent -- a missing row is not an outage"
    );

    // (c) A REPOSITORY ERROR INJECTED THROUGH THE PORT: the seam itself could not be asked. Public
    // too (fails closed IDENTICALLY at this boundary), but on the OPPOSITE-response counter --
    // `lookup_failed`, never `not_found`. The two classes cannot collapse: they are different
    // METRIC NAMES, not different label values of one counter.
    let sub_c = uuid::Uuid::from_u128(0xC);
    let jwt_c = customer_jwt(sub_c, uuid::Uuid::from_u128(0xC0C));
    let mut headers_c = axum::http::HeaderMap::new();
    headers_c.insert(axum::http::header::COOKIE, format!("captain_auth={jwt_c}").parse().unwrap());
    let principal_c = auth.authorize(RequestRole::Customer, &headers_c).await.expect("authorizes");
    let resolver_c = std::sync::Arc::new(ScriptedResolver(CustomerIdentityResolution::LookupFailed(
        LookupFailureReason::Repository,
    )));
    let (_, scope_c) = server::resolve_read_scope(
        principal_c,
        server::graphql_session::RequestCorrelationId(uuid::Uuid::from_u128(3)),
        &sources(CustomerIdentitySource::Postgres(resolver_c)),
    )
    .await;
    assert_eq!(
        scope_c,
        application::queries::ReadScope::Public,
        "a failed lookup fails closed IDENTICALLY to a missing mapping at this boundary"
    );
    provider.force_flush().expect("flush the spy reader");
    assert_eq!(
        points(&exporter, LOOKUP_FAILED, "reason"),
        vec![("repository".to_string(), 1)],
        "exactly ONE lookup_failed, the coarse DomainError class as `reason`"
    );
    assert_eq!(
        points(&exporter, NOT_FOUND, "reason"),
        vec![(String::new(), 1)],
        "and not_found did NOT double-count -- (b)'s single increment stands alone"
    );

    // (d) TOGGLE OFF (`CustomerIdentitySource::Claim`): unchanged legacy behaviour. The SAME
    // principal from (a) -- whose claim carries `wrong_claim` -- now resolves via the claim,
    // because there is no other source to consult. No fixture ever exercises the Postgres seam
    // here: this proves the OFF path takes NO lookup at all, not merely "a lookup that happens to
    // agree".
    let (_, scope_d) = server::resolve_read_scope(
        principal_a,
        server::graphql_session::RequestCorrelationId(uuid::Uuid::from_u128(4)),
        &sources(CustomerIdentitySource::Claim),
    )
    .await;
    assert_eq!(
        scope_d,
        application::queries::ReadScope::Customer(CustomerId(wrong_claim)),
        "OFF -- the DEFAULT -- trusts the claim, byte for byte, exactly as before this change"
    );
}
