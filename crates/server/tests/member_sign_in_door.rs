//! #639 part C step 6-ii — the member sign-in door, backend half, through the REAL transport:
//! `POST /public/graphql` on the production `graphql_routes` (the `[PUBLIC]` guard, the typed
//! acceptance door, the `MemMailbox` row), then the row delivered through the HUMAN-OWNED router
//! (`infrastructure::inbox::route`, the same `RestaurantMembershipInbox` arm the mailbox worker
//! runs) over a SCRIPTED identity port and a scripted `Member` bridge. No Postgres, never skips —
//! the `rider_sign_in_door.rs` harness, transposed to email (ADR-20260905-101349 §7-§10).
//!
//! What is pinned, in the card's order:
//!   (1) the enumeration oracle — byte-identical status + body for a roster address and a
//!       stranger's, AND the `Member` bridge consulted ZERO times on the request leg;
//!   (2) a KNOWN member is stamped through `stamp_member_claim` (the subject and nothing else on
//!       the port, `{ role: MEMBER }` and nothing else on the wire) and the POST-STAMP session is
//!       parked for `POST /auth/session`; end to end, the issued token opens `/restaurant/graphql`
//!       with a `ReadScope::Restaurant` matching the seam's resolved scope, and stays FORBIDDEN as
//!       PUBLIC with no row;
//!   (3) a verified email with no member behind it is refused (`MemberNotLinked`), NOTHING is
//!       stamped, but the session is STILL parked (unlike the rider door: an
//!       authenticated-but-unlinked person still gets a real cookie for "Se déconnecter");
//!   (4) the door gate OFF refuses BOTH mutations before the identity provider is touched at all;
//!   (5) an invalid/expired token is refused, nothing stamped;
//!   (6) [the per-role GraphQL limits pair lives in its own suite, `graphql_limits.rs`].
//!
//! Mutants planted, seen RED, reverted — quoted in the hand-back:
//!   M1 a distinct response for unknown addresses ((1) reds);
//!   M2 the stamper writes `member_id` (a `supabase_auth.rs` unit test reds, not this file);
//!   M3 the bridge consulted on the request leg ((1)'s counting stand-in reds);
//!   M5 gate read after the provider call ((4) reds).

use std::sync::{Arc, Mutex};

use application::auth_sessions::mem::MemAuthSessionStore;
use application::generated::inboxes::ActorInbox;
use application::generated::services::{
    IdentityRefreshSessionInput, IdentityRefreshSessionOutput, IdentitySendEmailMagicLinkInput,
    IdentitySendPhoneOtpInput, IdentityService, IdentityStampCustomerClaimInput,
    IdentityStampMemberClaimInput, IdentityStampRiderClaimInput, IdentityVerifyEmailTokenInput,
    IdentityVerifyEmailTokenOutput, IdentityVerifyPhoneOtpInput, IdentityVerifyPhoneOtpOutput,
    ServiceCallMeta,
};
use application::ports::{Actor, EventStore};
use application::queries::MemberIdentityRepository;
use async_trait::async_trait;
use domain::generated::scalars::{AuthSubject, EmailAddress, MemberId, RestaurantId};
use domain::shared::errors::DomainError;
use infrastructure::inbox::{route, CommandDeps, InboxOutcome, RouterEnv};
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedPaymentGateway, PgAuthSubjectReservationRepository,
    PgCatalogRepository, PgCustomerRepository, PgProspectionRepository, PgRestaurantRepository,
    PgRiderRepository, PgSlugReservationRepository, UnverifiedGbpOrderLinkProbe,
};
use serde_json::{json, Value};
use server::{
    CustomerIdentitySource, IdentitySources, MemberIdentityResolution, MemberIdentitySource,
    ResolveMemberIdentity, RiderIdentitySource,
};
use tower::ServiceExt;

// ─── Test-only signing material (the `auth.rs` suite's, duplicated for the reason stated there) ──

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

/// A verified token carrying EXACTLY what the member stamp writes — `app_metadata` taken from the
/// stamper's own PUT body, so this is the credential the door issues, not a hand-spelled lookalike.
fn jwt_of_the_member_stamp(sub: uuid::Uuid) -> String {
    let body = infrastructure::integrations::supabase_auth::stamp_member_put_body();
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
        "app_metadata": body["app_metadata"],
    });
    let key = jsonwebtoken::EncodingKey::from_ec_pem(TEST_EC_PRIVATE_KEY_PEM.as_bytes())
        .expect("test EC key parses");
    jsonwebtoken::encode(&header, &claims, &key).expect("sign test JWT")
}

// ─── The scripted ports ───────────────────────────────────────────────────────────────────────────

/// The identity provider, scripted: an email verifies to a deterministic subject (`sub-<email>`),
/// every stamp and every send is RECORDED, the rotated token reflects only what was stamped, and
/// the phone/customer legs panic — the member door selects its stamper at compile time, and this
/// is that selection observed at runtime.
#[derive(Default)]
struct ScriptedIdentity {
    sent: Mutex<Vec<String>>,
    verified: Mutex<u32>,
    /// Subjects the MEMBER stamper was asked to stamp — the port input is the subject and nothing
    /// else; the wire shape is the adapter's (`stamp_member_put_body`, pinned separately).
    member_stamps: Mutex<Vec<String>>,
    /// Subjects the provider ALREADY holds with a customer claim: the member stamp refuses them.
    holds_customer_claim: Vec<String>,
}

fn subject_of(email: &str) -> String {
    format!("sub-{email}")
}

#[async_trait]
impl IdentityService for ScriptedIdentity {
    async fn send_phone_otp(&self, _input: IdentitySendPhoneOtpInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        panic!("the member door never sends SMS")
    }
    async fn verify_phone_otp(&self, _input: IdentityVerifyPhoneOtpInput, _meta: &ServiceCallMeta) -> Result<IdentityVerifyPhoneOtpOutput, DomainError> {
        panic!("the member door never verifies a phone OTP")
    }
    async fn refresh_session(
        &self,
        _input: IdentityRefreshSessionInput,
        _meta: &ServiceCallMeta,
    ) -> Result<IdentityRefreshSessionOutput, DomainError> {
        let stamped = !self.member_stamps.lock().expect("scripted identity").is_empty();
        Ok(IdentityRefreshSessionOutput {
            access_token: if stamped { "rotated:MEMBER".into() } else { "rotated:none".into() },
            refresh_token: Some("post-stamp.refresh".into()),
            expires_in: Some(3600),
        })
    }
    async fn stamp_customer_claim(&self, input: IdentityStampCustomerClaimInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        panic!("the member door reached the CUSTOMER stamper for {} -- the stampers are selected at compile time and must never cross", input.auth_ref.0);
    }
    async fn stamp_rider_claim(&self, input: IdentityStampRiderClaimInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        panic!("the member door reached the RIDER stamper for {} -- the stampers are selected at compile time and must never cross", input.auth_ref.0);
    }
    async fn send_email_magic_link(&self, input: IdentitySendEmailMagicLinkInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        self.sent.lock().expect("scripted identity").push(input.email.0);
        Ok(())
    }
    async fn verify_email_token(
        &self,
        input: IdentityVerifyEmailTokenInput,
        _meta: &ServiceCallMeta,
    ) -> Result<IdentityVerifyEmailTokenOutput, DomainError> {
        *self.verified.lock().expect("scripted identity") += 1;
        if input.token.0 == "bad-token" {
            return Err(DomainError::rejected("InvalidVerificationToken", json!({})));
        }
        // The token IS the email in this harness (`token_of`, below) -- deterministic, so the
        // enumeration test can drive a roster address and a stranger's through the SAME leg.
        let email = input.token.0.trim_start_matches("token-for-").to_string();
        Ok(IdentityVerifyEmailTokenOutput {
            auth_ref: AuthSubject(subject_of(&email)),
            email: EmailAddress(email),
            access_token: Some("pre-stamp.access".into()),
            refresh_token: Some("pre-stamp.refresh".into()),
            expires_in: Some(3600),
        })
    }
    async fn stamp_member_claim(&self, input: IdentityStampMemberClaimInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        if self.holds_customer_claim.contains(&input.auth_ref.0) {
            return Err(DomainError::rejected("AuthSubjectHoldsAnotherRole", json!({ "authRef": input.auth_ref.0 })));
        }
        self.member_stamps.lock().expect("scripted identity").push(input.auth_ref.0);
        Ok(())
    }
}

/// The `Member` read model's bridge, scripted: the known logins, and a COUNT of consultations --
/// the enumeration property of the request leg is "this was asked zero times". M3's mutant hook.
#[derive(Default)]
struct ScriptedMembers {
    known: Vec<(String, MemberId)>,
    consulted: Mutex<u32>,
}

#[async_trait]
impl MemberIdentityRepository for ScriptedMembers {
    async fn member_id_by_auth_subject(&self, auth_subject: AuthSubject) -> Result<Option<MemberId>, DomainError> {
        *self.consulted.lock().expect("scripted members") += 1;
        Ok(self.known.iter().find(|(s, _)| *s == auth_subject.0).map(|(_, id)| *id))
    }
}

/// The request-seam scripted resolution for the `/restaurant/graphql` leg (2).
struct ScriptedSeam(MemberIdentityResolution);

#[async_trait]
impl ResolveMemberIdentity for ScriptedSeam {
    async fn resolve(&self, _auth_subject: &str) -> MemberIdentityResolution {
        self.0.clone()
    }
}

/// An `EventStore` the door must NEVER reach: the sign-in emits nothing.
struct UntouchableEventStore;

#[async_trait]
impl EventStore for UntouchableEventStore {
    async fn append(&self, stream_name: &str, _expected_version: i64, _events: &[domain::generated::events::DomainEvent], _actor: &Actor) -> Result<i64, DomainError> {
        panic!("the member sign-in door must append NOTHING (stream {stream_name}) -- it identifies, it never registers");
    }
    async fn load(&self, stream_name: &str) -> Result<(Vec<domain::generated::events::DomainEvent>, i64), DomainError> {
        panic!("the member sign-in door must load NO stream (stream {stream_name}) -- the read model is its source");
    }
}

/// #639 part C step 6-v: this door never touches the platform grant bridge -- the
/// `UntouchableEventStore` precedent.
struct UntouchablePlatformMembers;

#[async_trait]
impl application::queries::PlatformMemberRepository for UntouchablePlatformMembers {
    async fn platform_membership_id_by_auth_subject(
        &self,
        _auth_subject: domain::generated::scalars::AuthSubject,
    ) -> Result<Option<domain::generated::scalars::PlatformMembershipId>, DomainError> {
        panic!("the member sign-in door must never consult the PlatformMember bridge");
    }
}

// ─── The fixture: the production router + the production router's deps ──────────────────────────

struct Door {
    mailbox: Arc<actor_client::mailbox::mem::MemMailbox>,
    identity: Arc<ScriptedIdentity>,
    members: Arc<ScriptedMembers>,
    sessions: Arc<MemAuthSessionStore>,
    deps: CommandDeps,
    app: axum::Router,
}

const SUPPORT: &str = "support@captain.food";

async fn door(identity: ScriptedIdentity, members: ScriptedMembers, seam: MemberIdentityResolution, door_open: bool) -> Door {
    let mailbox = Arc::new(actor_client::mailbox::mem::MemMailbox::default());
    let identity = Arc::new(identity);
    let members = Arc::new(members);
    let sessions = Arc::new(MemAuthSessionStore::default());
    let identity_port: Arc<dyn IdentityService> = identity.clone();
    let sessions_port: Arc<dyn application::auth_sessions::AuthSessionStore> = sessions.clone();
    let members_port: Arc<dyn MemberIdentityRepository> = members.clone();
    let unused: sqlx::PgPool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .expect("a lazy pool connects to nothing");
    let schema = server::graphql_schema::build_schema(
        None,
        Some(server::graphql_schema::WriteDeps {
            event_store: Arc::new(UntouchableEventStore),
            ownership: Arc::new(FailClosedGoogleOwnershipVerifier),
            gbp_probe: Arc::new(UnverifiedGbpOrderLinkProbe),
            auth_provider: identity_port.clone(),
            payments: Arc::new(FailClosedPaymentGateway),
            pm_state: Arc::new(application::generated::pm_state::mem::MemPaymentProcessState::default()),
            refund_state: Arc::new(application::generated::pm_state::mem::MemRefundProcessState::default()),
            mailbox: mailbox.clone(),
            status_bus: actor_client::OperationStatusBus::default(),
            auth_sessions: sessions_port.clone(),
            slug_reservations: Arc::new(PgSlugReservationRepository::new(unused.clone())),
        }),
        None,
    );
    let app = server::graphql_routes(
        schema,
        server::TenantLookup(None),
        IdentitySources {
            customer: CustomerIdentitySource::Claim,
            rider: RiderIdentitySource::new(Arc::new(server::NoDatabaseRiderIdentity)),
            member: MemberIdentitySource::new(Arc::new(ScriptedSeam(seam))),
            platform: server::PlatformIdentitySource::new(Arc::new(server::NoDatabasePlatformIdentity)),
        },
    )
    .layer(axum::Extension(server::AuthContext::from_config(
        jwks_endpoint().await,
        TEST_SUPABASE_URL.into(),
    )));
    let deps = CommandDeps {
        store: Arc::new(UntouchableEventStore),
        restaurants: Arc::new(PgRestaurantRepository::new(unused.clone())),
        slugs: Arc::new(PgSlugReservationRepository::new(unused.clone())),
        auth_subjects: Arc::new(PgAuthSubjectReservationRepository::new(unused.clone())),
        ownership: Arc::new(FailClosedGoogleOwnershipVerifier),
        probe: Arc::new(UnverifiedGbpOrderLinkProbe),
        prospection: Arc::new(PgProspectionRepository::new(unused.clone())),
        catalogs: Arc::new(PgCatalogRepository::new(unused.clone())),
        auth: identity_port,
        customers: Arc::new(PgCustomerRepository::new(unused.clone())),
        sessions: sessions_port,
        payments: Arc::new(FailClosedPaymentGateway),
        pm_state: Arc::new(application::generated::pm_state::mem::MemPaymentProcessState::default()),
        refund_state: Arc::new(application::generated::pm_state::mem::MemRefundProcessState::default()),
        mailbox_requeue: Arc::new(infrastructure::persistence::mailbox_lanes::PgMailboxRequeue::new(unused.clone())),
        enforce_service_hours_guard: false,
        enforce_acceptance_timeout: false,
        route_gates: application::generated::process_managers::RouteGates {
            order_placed_to_order: true,
            place_replacement_order_to_order: false,
            bind_cart_to_customer_to_cart: false,
            grant_customer_credit_to_customer_credit: false,
            mark_order_delivered_to_order: false,
        },
        riders: Arc::new(PgRiderRepository::new(unused.clone())),
        members: members_port,
        support_contact: Some(EmailAddress(SUPPORT.into())),
        run_rider_restriction_door: false,
        run_member_access_grant: false,
        run_member_sign_in_door: door_open,
        run_restaurant_invitation: false,
        run_platform_access_grant: false,
        platform_members: Arc::new(UntouchablePlatformMembers),
    };
    Door { mailbox, identity, members, sessions, deps, app }
}

impl Door {
    async fn post_public(&self, query: &str, session: uuid::Uuid) -> Value {
        self.post_public_as(query, Some(session)).await
    }

    async fn post_public_as(&self, query: &str, session: Option<uuid::Uuid>) -> Value {
        let mut builder = axum::http::Request::builder()
            .method("POST")
            .uri("/public/graphql")
            .header(axum::http::header::HOST, "chez-test.captain.food")
            .header(axum::http::header::CONTENT_TYPE, "application/json");
        if let Some(session) = session {
            builder = builder.header("X-SESSION-ID", session.to_string());
        }
        let request = builder
            .body(axum::body::Body::from(json!({ "query": query }).to_string()))
            .expect("request builds");
        let response = self.app.clone().oneshot(request).await.expect("router answers");
        assert_eq!(response.status(), axum::http::StatusCode::OK, "the public path never 401s");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body");
        serde_json::from_slice(&bytes).expect("a GraphQL response body")
    }

    async fn accept(&self, field: &str, query: &str, session: uuid::Uuid) -> Value {
        let body = self.post_public(query, session).await;
        assert!(body["errors"].is_null(), "{field} must be ACCEPTED on /public/graphql, got errors: {}", body["errors"]);
        let acceptance = body["data"][field].clone();
        assert_eq!(acceptance["operationStatus"], "PENDING", "acceptance-first: {acceptance}");
        acceptance
    }

    async fn deliver(&self, message_id: uuid::Uuid) -> Result<(), DomainError> {
        let entry = self.mailbox.entry(message_id).expect("the acceptance landed one mailbox row");
        assert_eq!(entry.actor_type(), "RestaurantMembership", "the sign-in commands ride the RestaurantMembership lane");
        let inbox = ActorInbox::parse(entry.actor_type(), entry.message_type(), entry.payload())
            .expect("the row parses into the typed RestaurantMembership inbox");
        let actor = Actor {
            user_id: entry.user_id().unwrap_or_else(uuid::Uuid::nil),
            user_type: entry.user_type().to_string(),
            domain_id: None,
            correlation_id: entry.correlation_id(),
            cause_id: Some(entry.message_id()),
        };
        match route(&self.deps, inbox, &actor, &RouterEnv { session_id: entry.session_id() }).await {
            InboxOutcome::Handled(result) => result,
            _ => panic!("a sign-in command is a HANDLED command, never a fact or a PM leg"),
        }
    }
}

fn request_link(email: &str, message_id: uuid::Uuid) -> String {
    format!(
        r#"mutation {{ requestMemberSignInLink(input: {{ email: "{email}" }}, metadata: {{ messageId: "{message_id}" }}) {{ messageId correlationId sessionId operationStatus duplicate }} }}"#
    )
}

/// The harness's token-to-email convention: `verify_email_token` recovers the email from the
/// token, so a single scripted identity can serve both the roster address and the stranger's
/// through the SAME command shape.
fn token_of(email: &str) -> String {
    format!("token-for-{email}")
}

fn confirm(token: &str, message_id: uuid::Uuid) -> String {
    format!(
        r#"mutation {{ confirmMemberSignIn(input: {{ token: "{token}" }}, metadata: {{ messageId: "{message_id}" }}) {{ messageId correlationId sessionId operationStatus duplicate }} }}"#
    )
}

fn rejection(result: Result<(), DomainError>) -> (String, Value) {
    match result {
        Err(DomainError::Rejected { code, context }) => (code, context),
        other => panic!("expected a TYPED rejection, got {other:?}"),
    }
}

const MEMBER_EMAIL: &str = "owner@pizzaroma.fr";
const STRANGER_EMAIL: &str = "stranger@example.com";

fn a_known_member() -> ScriptedMembers {
    ScriptedMembers {
        known: vec![(subject_of(MEMBER_EMAIL), MemberId(uuid::Uuid::from_u128(0x639_2C)))],
        consulted: Mutex::new(0),
    }
}

// ─── (1) the enumeration oracle: byte-identical, bridge consulted zero times ────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_link_request_answers_identically_for_a_member_and_a_stranger_and_never_consults_the_bridge() {
    let door = door(ScriptedIdentity::default(), a_known_member(), MemberIdentityResolution::NoMapping, true).await;
    let session = uuid::Uuid::from_u128(0x5E55);
    let for_member = uuid::Uuid::from_u128(0xD1);
    let for_stranger = uuid::Uuid::from_u128(0xD2);

    let mut member = door.accept("requestMemberSignInLink", &request_link(MEMBER_EMAIL, for_member), session).await;
    let mut stranger = door.accept("requestMemberSignInLink", &request_link(STRANGER_EMAIL, for_stranger), session).await;
    for shape in [&mut member, &mut stranger] {
        shape["messageId"] = Value::Null;
        shape["correlationId"] = Value::Null;
    }
    assert_eq!(member, stranger, "M1: the acceptance is byte-identical whether or not the address is on the roster");

    door.deliver(for_member).await.expect("the send leg succeeds for a roster address");
    door.deliver(for_stranger).await.expect("and identically for a stranger");
    assert_eq!(
        *door.identity.sent.lock().unwrap(),
        vec![MEMBER_EMAIL.to_string(), STRANGER_EMAIL.to_string()],
        "both addresses were sent a link"
    );
    assert_eq!(
        *door.members.consulted.lock().unwrap(),
        0,
        "M3: the Member bridge is consulted ZERO times on the request leg -- roster membership cannot leak from a path that never reads it"
    );
}

// M1 ("a distinct response for an unknown address") is recorded here as STRUCTURALLY NOT
// MUTATION-TESTABLE at the request leg (round 2, R2-B1): `request_member_sign_in_link` (see
// `crates/application/src/commands.rs`) has no branch that reads whether the address is on the
// roster — it calls `send_email_magic_link` unconditionally and returns the SAME acceptance
// shape regardless — so there is no per-address behaviour to plant a distinct-response mutant
// ON. That absence of a branch IS the enumeration-oracle guarantee, not a gap in coverage: a
// mutant that introduced such a branch would be caught by
// `the_link_request_answers_identically_for_a_member_and_a_stranger_and_never_consults_the_bridge`
// below, which is the real (and only) test of this property. A prior revision of this file
// asserted on the test DOUBLE's own scripted "distinct response" flag rather than on anything
// the handler could produce — a tautology (it could never fail unless the double's own dead
// code path were exercised) — and has been deleted rather than kept as false assurance.

// ─── (2) a known member: MEMBER role only, parked session, and the door opens ───────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_known_member_is_stamped_with_the_member_role_only_and_the_post_stamp_session_is_parked() {
    let door = door(ScriptedIdentity::default(), a_known_member(), MemberIdentityResolution::NoMapping, true).await;
    let session = uuid::Uuid::from_u128(0x5E55);
    let message_id = uuid::Uuid::from_u128(0xB1);

    door.accept("confirmMemberSignIn", &confirm(&token_of(MEMBER_EMAIL), message_id), session).await;
    door.deliver(message_id).await.expect("a known member signs in");

    assert_eq!(
        *door.identity.member_stamps.lock().unwrap(),
        vec![subject_of(MEMBER_EMAIL)],
        "stamp_member_claim was called ONCE, with the verified subject and nothing else on the port"
    );
    let body = infrastructure::integrations::supabase_auth::stamp_member_put_body();
    assert_eq!(body["app_metadata"]["captain_food"], json!({ "role": "MEMBER" }), "no member_id -- M2's shape, pinned again here");

    let parked = door.sessions.parked();
    assert_eq!(parked.len(), 1, "exactly one session parked for POST /auth/session");
    assert_eq!(parked[0].message_id, message_id);
    assert_eq!(parked[0].session_id, Some(session));
    assert_eq!(parked[0].access_token, "rotated:MEMBER", "the POST-STAMP token is what gets parked");
}

const ORDERS_QUERY: &str = r#"query { orders(input: { restaurantId: "00000000-0000-0000-0000-000000000dd1" }) { id } }"#;

async fn post_as_member(door: &Door, jwt: &str) -> (String, Option<String>) {
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/restaurant/graphql")
        .header(axum::http::header::HOST, "chez-test.captain.food")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(axum::body::Body::from(json!({ "query": ORDERS_QUERY }).to_string()))
        .expect("request builds");
    let response = door.app.clone().oneshot(request).await.expect("router answers");
    assert_eq!(response.status(), axum::http::StatusCode::OK, "a verified MEMBER token authorizes on /restaurant");
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("a GraphQL response body");
    let err = body["errors"].as_array().and_then(|e| e.first()).cloned().unwrap_or(Value::Null);
    (
        err["message"].as_str().unwrap_or_default().to_string(),
        err["extensions"]["code"].as_str().map(str::to_string),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_token_the_member_stamp_writes_opens_the_restaurant_door_once_the_seam_resolves_a_scope() {
    let sub = uuid::Uuid::from_u128(0x639_2D);
    let jwt = jwt_of_the_member_stamp(sub);
    let resolved_restaurant = RestaurantId(uuid::Uuid::from_u128(0x00_00_dd_1));

    let resolved = door(
        ScriptedIdentity::default(),
        ScriptedMembers::default(),
        MemberIdentityResolution::Resolved(resolved_restaurant),
        true,
    ).await;
    let (message, code) = post_as_member(&resolved, &jwt).await;
    assert_ne!(code.as_deref(), Some("FORBIDDEN"), "the stamped credential acts as RESTAURANT (via MEMBER) -- got: {message}");

    // THE CONTROL: the same credential with NO resolved scope is nobody.
    let unbound = door(ScriptedIdentity::default(), ScriptedMembers::default(), MemberIdentityResolution::NoMapping, true).await;
    let (message, code) = post_as_member(&unbound, &jwt).await;
    assert_eq!(code.as_deref(), Some("FORBIDDEN"), "no resolved scope => PUBLIC, whatever the token says -- got: {message}");
}

// ─── (3) the not-yet-linked refusal: nothing stamped, but the session IS parked ─────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_email_is_refused_not_linked_but_the_session_is_still_parked_for_a_real_cookie() {
    let door = door(ScriptedIdentity::default(), ScriptedMembers::default(), MemberIdentityResolution::NoMapping, true).await;
    let session = uuid::Uuid::from_u128(0x5E55);
    let message_id = uuid::Uuid::from_u128(0xA1);

    door.accept("confirmMemberSignIn", &confirm(&token_of(STRANGER_EMAIL), message_id), session).await;
    let (code, context) = rejection(door.deliver(message_id).await);
    assert_eq!(code, "MemberNotLinked", "a verified email with no member behind it is REFUSED");
    assert_eq!(context["email"], json!(STRANGER_EMAIL));
    assert_eq!(context["supportContact"], json!(SUPPORT));
    assert!(door.identity.member_stamps.lock().unwrap().is_empty(), "nothing stamped");
    let parked = door.sessions.parked();
    assert_eq!(parked.len(), 1, "the session IS parked -- unlike the rider door, so \"Se deconnecter\" has a real cookie");
    assert_eq!(parked[0].message_id, message_id);
}

// ─── (4) the door gate: refuses BOTH mutations before the provider is touched ───────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_door_closed_refuses_both_mutations_before_the_identity_provider_is_touched() {
    let door = door(ScriptedIdentity::default(), a_known_member(), MemberIdentityResolution::NoMapping, false).await;
    let session = uuid::Uuid::from_u128(0x5E55);
    let req_id = uuid::Uuid::from_u128(0xC1);
    let confirm_id = uuid::Uuid::from_u128(0xC2);

    door.accept("requestMemberSignInLink", &request_link(MEMBER_EMAIL, req_id), session).await;
    let (code, _) = rejection(door.deliver(req_id).await);
    assert_eq!(code, "MemberSignInDoorClosed");

    door.accept("confirmMemberSignIn", &confirm(&token_of(MEMBER_EMAIL), confirm_id), session).await;
    let (code, _) = rejection(door.deliver(confirm_id).await);
    assert_eq!(code, "MemberSignInDoorClosed");

    // M5: the gate must be read BEFORE the provider is touched -- these three assertions are what
    // a "gate read after the provider call" mutant would break (send/verify/stamp all fire first).
    assert!(door.identity.sent.lock().unwrap().is_empty(), "M5: nothing sent -- the door refuses before the send leg");
    assert_eq!(*door.identity.verified.lock().unwrap(), 0, "M5: nothing verified -- the door refuses before the OTP-equivalent is spent");
    assert!(door.identity.member_stamps.lock().unwrap().is_empty(), "M5: nothing stamped");
    assert!(door.sessions.parked().is_empty(), "M5: nothing parked");
}

// ─── (5) an invalid/expired token: refused, nothing stamped ─────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_invalid_token_is_rejected_and_nothing_is_stamped() {
    let door = door(ScriptedIdentity::default(), a_known_member(), MemberIdentityResolution::NoMapping, true).await;
    let session = uuid::Uuid::from_u128(0x5E55);
    let message_id = uuid::Uuid::from_u128(0xE1);

    door.accept("confirmMemberSignIn", &confirm("bad-token", message_id), session).await;
    let (code, _) = rejection(door.deliver(message_id).await);
    assert_eq!(code, "InvalidVerificationToken");
    assert!(door.identity.member_stamps.lock().unwrap().is_empty());
    assert!(door.sessions.parked().is_empty(), "an invalid token is never spent, so nothing is ever parked for it");
}

// ─── One-subject-one-role: refuse, never overwrite ───────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_subject_already_holding_a_customer_claim_is_refused_and_nothing_is_overwritten() {
    let identity = ScriptedIdentity { holds_customer_claim: vec![subject_of(MEMBER_EMAIL)], ..Default::default() };
    let door = door(identity, a_known_member(), MemberIdentityResolution::NoMapping, true).await;
    let session = uuid::Uuid::from_u128(0x5E55);
    let message_id = uuid::Uuid::from_u128(0xF1);

    door.accept("confirmMemberSignIn", &confirm(&token_of(MEMBER_EMAIL), message_id), session).await;
    let (code, context) = rejection(door.deliver(message_id).await);
    assert_eq!(code, "AuthSubjectHoldsAnotherRole");
    assert_eq!(context["authRef"], json!(subject_of(MEMBER_EMAIL)));
    assert!(door.identity.member_stamps.lock().unwrap().is_empty());
    assert!(door.sessions.parked().is_empty());
}

// ─── Missing session: refused before the token is spent ─────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_confirm_without_a_session_header_is_refused_before_the_token_is_spent_and_nothing_is_parked() {
    let door = door(ScriptedIdentity::default(), a_known_member(), MemberIdentityResolution::NoMapping, true).await;
    let message_id = uuid::Uuid::from_u128(0xF2);

    let acceptance = door.post_public_as(&confirm(&token_of(MEMBER_EMAIL), message_id), None).await;
    assert!(acceptance["errors"].is_null());
    let acceptance = acceptance["data"]["confirmMemberSignIn"].clone();
    assert!(acceptance["sessionId"].is_null());

    let (code, context) = rejection(door.deliver(message_id).await);
    assert_eq!(code, "MemberSignInRequiresSession");
    assert_eq!(context, json!({}));
    assert_eq!(*door.identity.verified.lock().unwrap(), 0, "refused BEFORE the token is spent");
    assert!(door.identity.member_stamps.lock().unwrap().is_empty());
    assert!(door.sessions.parked().is_empty());
}
