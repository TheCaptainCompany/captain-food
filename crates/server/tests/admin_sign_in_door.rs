//! #639 part C step 6-iii — the ADMIN sign-in door, backend half, through the REAL transport:
//! `POST /public/graphql` on the production `graphql_routes` (the `[PUBLIC]` guard, the typed
//! acceptance door, the `AdminSignIn` mailbox row), then the row delivered through the
//! HUMAN-OWNED router (`infrastructure::inbox::route`, the same `AdminSignInInbox` arm the
//! mailbox worker runs) over a SCRIPTED identity port and a scripted `PlatformMember` bridge. No
//! Postgres, never skips -- the `member_sign_in_door.rs` harness, transposed to the platform
//! context (ADR-20260906-023825).
//!
//! What is pinned, in the card's order:
//!   (1) the enumeration oracle -- byte-identical status + body for a granted admin's address and
//!       a stranger's, AND the `PlatformMember` bridge consulted ZERO times on the request leg;
//!   (2) door OFF: typed `AdminSignInDoorClosed` AND the IdP fake saw ZERO calls;
//!   (3) a KNOWN admin is stamped through `stamp_admin_claim` (the subject and nothing else on
//!       the port, `{ role: ADMIN }` and nothing else on the wire) and the POST-STAMP session is
//!       parked for `POST /auth/session`; end to end, the issued token opens `/admin/graphql`,
//!       and stays FORBIDDEN as PUBLIC with no live grant;
//!   (4) a verified email with no grant behind it is refused (`AdminAccessNotGranted`), NOTHING
//!       is stamped, but the session is STILL parked;
//!   (5) `stamp_admin_put_body()` is exactly `{ "role": "ADMIN" }`;
//!   (6) a grant absent after the stamp (the still-valid ADMIN cookie, no grant row) -> the NEXT
//!       request refuses (`Identity::Unbound`, the parent ADR SS8's answer) -- simulated here by
//!       stamping against a `Resolved` seam then re-asking the SAME seam as `NoMapping` (no revoke
//!       command exists to flip a real row, ADR-20260906-023825 follow-up);
//!   (7) [the gate-liveness gauge reads the key at both roots -- its own unit-level proof lives
//!       beside the gauge, `crates/telemetry/src/meters.rs`'s own tests].
//!
//! Mutants planted, seen RED, reverted -- quoted in the hand-back:
//!   m1 board rendered on a 401 (not this file -- `crates/web`'s render tests);
//!   m2 the request leg consults the bridge ((1)'s counting stand-in reds);
//!   m3 stamp without a grant ((4) reds: nothing stamped and AdminAccessNotGranted stays refused);
//!   m4 the stamper writes a body beyond `{role: ADMIN}` (a `supabase_auth.rs` unit test reds);
//!   m6 System resolves a slug (not this file -- `crates/web/src/router.rs`'s own test);
//!   m7 no-grant ADMIN admitted ((6) reds).

use std::sync::{Arc, Mutex};

use application::auth_sessions::mem::MemAuthSessionStore;
use application::generated::inboxes::ActorInbox;
use application::generated::services::{
    IdentityRefreshSessionInput, IdentityRefreshSessionOutput, IdentitySendAdminSignInLinkInput,
    IdentitySendEmailMagicLinkInput, IdentitySendPhoneOtpInput, IdentityService,
    IdentityStampCustomerClaimInput, IdentityStampMemberClaimInput, IdentityStampRiderClaimInput,
    IdentityVerifyEmailTokenInput, IdentityVerifyEmailTokenOutput, IdentityVerifyPhoneOtpInput,
    IdentityVerifyPhoneOtpOutput, ServiceCallMeta,
};
use application::ports::{Actor, EventStore};
use application::queries::{MemberIdentityRepository, PlatformMemberRepository};
use async_trait::async_trait;
use domain::generated::scalars::{AuthSubject, EmailAddress, MemberId, PlatformMembershipId};
use domain::shared::errors::DomainError;
use infrastructure::inbox::{route, CommandDeps, InboxOutcome, RouterEnv};
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedPaymentGateway, PgAuthSubjectReservationRepository,
    PgCatalogRepository, PgCustomerRepository, PgProspectionRepository, PgRestaurantRepository,
    PgRiderRepository, PgSlugReservationRepository, UnverifiedGbpOrderLinkProbe,
};
use serde_json::{json, Value};
use server::{
    CustomerIdentitySource, IdentitySources, MemberIdentitySource, PlatformIdentityResolution,
    PlatformIdentitySource, ResolvePlatformIdentity, RiderIdentitySource,
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

/// A verified token carrying EXACTLY what the admin stamp writes -- `app_metadata` taken from the
/// stamper's own PUT body, so this is the credential the door issues, not a hand-spelled lookalike.
fn jwt_of_the_admin_stamp(sub: uuid::Uuid) -> String {
    let body = infrastructure::integrations::supabase_auth::stamp_admin_put_body();
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
/// the phone/customer/rider/member legs panic -- the admin door selects its stamper at compile
/// time, and this is that selection observed at runtime.
#[derive(Default)]
struct ScriptedIdentity {
    sent: Mutex<Vec<String>>,
    verified: Mutex<u32>,
    /// Subjects the ADMIN stamper was asked to stamp -- the port input is the subject and nothing
    /// else; the wire shape is the adapter's (`stamp_admin_put_body`, pinned separately).
    admin_stamps: Mutex<Vec<String>>,
    /// Subjects the provider ALREADY holds with a customer claim: the admin stamp refuses them.
    holds_customer_claim: Vec<String>,
}

fn subject_of(email: &str) -> String {
    format!("sub-{email}")
}

#[async_trait]
impl IdentityService for ScriptedIdentity {
    async fn send_phone_otp(&self, _input: IdentitySendPhoneOtpInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        panic!("the admin door never sends SMS")
    }
    async fn verify_phone_otp(&self, _input: IdentityVerifyPhoneOtpInput, _meta: &ServiceCallMeta) -> Result<IdentityVerifyPhoneOtpOutput, DomainError> {
        panic!("the admin door never verifies a phone OTP")
    }
    async fn refresh_session(
        &self,
        _input: IdentityRefreshSessionInput,
        _meta: &ServiceCallMeta,
    ) -> Result<IdentityRefreshSessionOutput, DomainError> {
        let stamped = !self.admin_stamps.lock().expect("scripted identity").is_empty();
        Ok(IdentityRefreshSessionOutput {
            access_token: if stamped { "rotated:ADMIN".into() } else { "rotated:none".into() },
            refresh_token: Some("post-stamp.refresh".into()),
            expires_in: Some(3600),
        })
    }
    async fn stamp_customer_claim(&self, input: IdentityStampCustomerClaimInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        panic!("the admin door reached the CUSTOMER stamper for {} -- the stampers are selected at compile time and must never cross", input.auth_ref.0);
    }
    async fn stamp_rider_claim(&self, input: IdentityStampRiderClaimInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        panic!("the admin door reached the RIDER stamper for {} -- the stampers are selected at compile time and must never cross", input.auth_ref.0);
    }
    async fn stamp_member_claim(&self, input: IdentityStampMemberClaimInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        panic!("the admin door reached the MEMBER stamper for {} -- the stampers are selected at compile time and must never cross", input.auth_ref.0);
    }
    async fn send_email_magic_link(&self, _input: IdentitySendEmailMagicLinkInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        panic!("the admin door never calls the shared member/customer send_email_magic_link -- round 2 R2-3 gave it its own send_admin_sign_in_link call site")
    }
    // Round 2 R2-3: `requestAdminSignInLink` -> `send_admin_sign_in_link` (its OWN call site into
    // the `EmailSendAuthorizer`'s `SignInDoor::Admin` arm), never the shared member/customer port.
    async fn send_admin_sign_in_link(&self, input: IdentitySendAdminSignInLinkInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
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
        // enumeration test can drive a granted admin's address and a stranger's through the SAME
        // leg.
        let email = input.token.0.trim_start_matches("token-for-").to_string();
        Ok(IdentityVerifyEmailTokenOutput {
            auth_ref: AuthSubject(subject_of(&email)),
            email: EmailAddress(email),
            access_token: Some("pre-stamp.access".into()),
            refresh_token: Some("pre-stamp.refresh".into()),
            expires_in: Some(3600),
        })
    }
    async fn stamp_admin_claim(&self, input: application::generated::services::IdentityStampAdminClaimInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        if self.holds_customer_claim.contains(&input.auth_ref.0) {
            return Err(DomainError::rejected("AuthSubjectHoldsAnotherRole", json!({ "authRef": input.auth_ref.0 })));
        }
        self.admin_stamps.lock().expect("scripted identity").push(input.auth_ref.0);
        Ok(())
    }
}

/// The `PlatformMember` bridge, scripted: the known grants, and a COUNT of consultations -- the
/// enumeration property of the request leg is "this was asked zero times". m2's mutant hook.
#[derive(Default)]
struct ScriptedPlatformMembers {
    known: Vec<(String, PlatformMembershipId)>,
    consulted: Mutex<u32>,
}

#[async_trait]
impl PlatformMemberRepository for ScriptedPlatformMembers {
    async fn platform_membership_id_by_auth_subject(
        &self,
        auth_subject: AuthSubject,
    ) -> Result<Option<PlatformMembershipId>, DomainError> {
        *self.consulted.lock().expect("scripted platform members") += 1;
        Ok(self.known.iter().find(|(s, _)| *s == auth_subject.0).map(|(_, id)| *id))
    }
}

/// The request-seam scripted resolution for the `/admin/graphql` leg (3)/(6). `Mutex` so test (6)
/// can flip the SAME seam mid-test (a still-valid ADMIN cookie, the grant withdrawn underneath it
/// -- no revoke command exists to simulate this any other way, ADR-20260906-023825 follow-up).
struct ScriptedSeam(Mutex<PlatformIdentityResolution>);

#[async_trait]
impl ResolvePlatformIdentity for ScriptedSeam {
    async fn resolve(&self, _auth_subject: &str) -> PlatformIdentityResolution {
        self.0.lock().expect("scripted seam").clone()
    }
}

/// An `EventStore` the door must NEVER reach: the sign-in emits nothing.
struct UntouchableEventStore;

#[async_trait]
impl EventStore for UntouchableEventStore {
    async fn append(&self, stream_name: &str, _expected_version: i64, _events: &[domain::generated::events::DomainEvent], _actor: &Actor) -> Result<i64, DomainError> {
        panic!("the admin sign-in door must append NOTHING (stream {stream_name}) -- it identifies, it never registers");
    }
    async fn load(&self, stream_name: &str) -> Result<(Vec<domain::generated::events::DomainEvent>, i64), DomainError> {
        panic!("the admin sign-in door must load NO stream (stream {stream_name}) -- the read model is its source");
    }
}

/// This door never touches the `Member` bridge -- the `UntouchableEventStore` precedent.
struct UntouchableMembers;

#[async_trait]
impl MemberIdentityRepository for UntouchableMembers {
    async fn member_id_by_auth_subject(&self, _auth_subject: AuthSubject) -> Result<Option<MemberId>, DomainError> {
        panic!("the admin sign-in door must never consult the Member bridge");
    }
}

// ─── The fixture: the production router + the production router's deps ──────────────────────────

struct Door {
    mailbox: Arc<actor_client::mailbox::mem::MemMailbox>,
    identity: Arc<ScriptedIdentity>,
    platform_members: Arc<ScriptedPlatformMembers>,
    sessions: Arc<MemAuthSessionStore>,
    seam: Arc<ScriptedSeam>,
    deps: CommandDeps,
    app: axum::Router,
}

const SUPPORT: &str = "support@captain.food";

async fn door(identity: ScriptedIdentity, platform_members: ScriptedPlatformMembers, seam: PlatformIdentityResolution, door_open: bool) -> Door {
    let mailbox = Arc::new(actor_client::mailbox::mem::MemMailbox::default());
    let identity = Arc::new(identity);
    let platform_members = Arc::new(platform_members);
    let sessions = Arc::new(MemAuthSessionStore::default());
    let seam = Arc::new(ScriptedSeam(Mutex::new(seam)));
    let identity_port: Arc<dyn IdentityService> = identity.clone();
    let sessions_port: Arc<dyn application::auth_sessions::AuthSessionStore> = sessions.clone();
    let platform_members_port: Arc<dyn PlatformMemberRepository> = platform_members.clone();
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
            member: MemberIdentitySource::new(Arc::new(server::NoDatabaseMemberIdentity)),
            platform: PlatformIdentitySource::new(seam.clone()),
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
        members: Arc::new(UntouchableMembers),
        support_contact: Some(EmailAddress(SUPPORT.into())),
        run_rider_restriction_door: false,
        run_member_access_grant: false,
        run_member_sign_in_door: false,
        run_restaurant_invitation: false,
        run_platform_access_grant: false,
        platform_members: platform_members_port,
        run_admin_sign_in_door: door_open,
            quote_guard: application::quote::QuoteGuard::closed_for_tests().into(),
};
    Door { mailbox, identity, platform_members, sessions, seam, deps, app }
}

impl Door {
    async fn post_public(&self, query: &str, session: uuid::Uuid) -> Value {
        self.post_public_as(query, Some(session)).await
    }

    async fn post_public_as(&self, query: &str, session: Option<uuid::Uuid>) -> Value {
        let mut builder = axum::http::Request::builder()
            .method("POST")
            .uri("/public/graphql")
            .header(axum::http::header::HOST, "system.captain.food")
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
        assert_eq!(entry.actor_type(), "AdminSignIn", "the sign-in commands ride the AdminSignIn lane");
        let inbox = ActorInbox::parse(entry.actor_type(), entry.message_type(), entry.payload())
            .expect("the row parses into the typed AdminSignIn inbox");
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
        r#"mutation {{ requestAdminSignInLink(input: {{ email: "{email}" }}, metadata: {{ messageId: "{message_id}" }}) {{ messageId correlationId sessionId operationStatus duplicate }} }}"#
    )
}

/// The harness's token-to-email convention: `verify_email_token` recovers the email from the
/// token, so a single scripted identity can serve both a granted admin's address and a
/// stranger's through the SAME command shape.
fn token_of(email: &str) -> String {
    format!("token-for-{email}")
}

fn confirm(token: &str, message_id: uuid::Uuid) -> String {
    format!(
        r#"mutation {{ confirmAdminSignIn(input: {{ token: "{token}" }}, metadata: {{ messageId: "{message_id}" }}) {{ messageId correlationId sessionId operationStatus duplicate }} }}"#
    )
}

fn rejection(result: Result<(), DomainError>) -> (String, Value) {
    match result {
        Err(DomainError::Rejected { code, context }) => (code, context),
        other => panic!("expected a TYPED rejection, got {other:?}"),
    }
}

const ADMIN_EMAIL: &str = "admin@captain.food";
const STRANGER_EMAIL: &str = "stranger@example.com";

fn a_granted_admin() -> ScriptedPlatformMembers {
    ScriptedPlatformMembers {
        known: vec![(subject_of(ADMIN_EMAIL), PlatformMembershipId(uuid::Uuid::from_u128(0x639_3C)))],
        consulted: Mutex::new(0),
    }
}

// ─── (1) the enumeration oracle: byte-identical, bridge consulted zero times ────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_link_request_answers_identically_for_a_granted_admin_and_a_stranger_and_never_consults_the_bridge() {
    let door = door(ScriptedIdentity::default(), a_granted_admin(), PlatformIdentityResolution::NoMapping, true).await;
    let session = uuid::Uuid::from_u128(0x5E56);
    let for_admin = uuid::Uuid::from_u128(0xD3);
    let for_stranger = uuid::Uuid::from_u128(0xD4);

    let mut admin = door.accept("requestAdminSignInLink", &request_link(ADMIN_EMAIL, for_admin), session).await;
    let mut stranger = door.accept("requestAdminSignInLink", &request_link(STRANGER_EMAIL, for_stranger), session).await;
    for shape in [&mut admin, &mut stranger] {
        shape["messageId"] = Value::Null;
        shape["correlationId"] = Value::Null;
    }
    assert_eq!(admin, stranger, "the acceptance is byte-identical whether or not the address holds a platform grant");

    door.deliver(for_admin).await.expect("the send leg succeeds for a granted admin's address");
    door.deliver(for_stranger).await.expect("and identically for a stranger");
    assert_eq!(
        *door.identity.sent.lock().unwrap(),
        vec![ADMIN_EMAIL.to_string(), STRANGER_EMAIL.to_string()],
        "both addresses were sent a link"
    );
    assert_eq!(
        *door.platform_members.consulted.lock().unwrap(),
        0,
        "m2: the PlatformMember bridge is consulted ZERO times on the request leg -- platform standing cannot leak from a path that never reads it"
    );
}

// ─── (2) the door gate: refuses BOTH mutations before the provider is touched, IdP sees ZERO calls

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_door_closed_refuses_both_mutations_before_the_identity_provider_is_touched() {
    let door = door(ScriptedIdentity::default(), a_granted_admin(), PlatformIdentityResolution::NoMapping, false).await;
    let session = uuid::Uuid::from_u128(0x5E56);
    let req_id = uuid::Uuid::from_u128(0xC3);
    let confirm_id = uuid::Uuid::from_u128(0xC4);

    door.accept("requestAdminSignInLink", &request_link(ADMIN_EMAIL, req_id), session).await;
    let (code, _) = rejection(door.deliver(req_id).await);
    assert_eq!(code, "AdminSignInDoorClosed");

    door.accept("confirmAdminSignIn", &confirm(&token_of(ADMIN_EMAIL), confirm_id), session).await;
    let (code, _) = rejection(door.deliver(confirm_id).await);
    assert_eq!(code, "AdminSignInDoorClosed");

    // The gate must be read BEFORE the provider is touched -- these assertions are what a "gate
    // read after the provider call" mutant would break, and the beck instruction "the IdP fake saw
    // ZERO calls" directly: send/verify/stamp all fire first under that mutant.
    assert!(door.identity.sent.lock().unwrap().is_empty(), "nothing sent -- the door refuses before the send leg");
    assert_eq!(*door.identity.verified.lock().unwrap(), 0, "nothing verified -- the door refuses before the token is spent");
    assert!(door.identity.admin_stamps.lock().unwrap().is_empty(), "nothing stamped");
    assert!(door.sessions.parked().is_empty(), "nothing parked");
}

// ─── (3) a granted admin: ADMIN role only, parked session, and the door opens ───────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_granted_admin_is_stamped_with_the_admin_role_only_and_the_post_stamp_session_is_parked() {
    let door = door(ScriptedIdentity::default(), a_granted_admin(), PlatformIdentityResolution::NoMapping, true).await;
    let session = uuid::Uuid::from_u128(0x5E56);
    let message_id = uuid::Uuid::from_u128(0xB2);

    door.accept("confirmAdminSignIn", &confirm(&token_of(ADMIN_EMAIL), message_id), session).await;
    door.deliver(message_id).await.expect("a granted admin signs in");

    assert_eq!(
        *door.identity.admin_stamps.lock().unwrap(),
        vec![subject_of(ADMIN_EMAIL)],
        "stamp_admin_claim was called ONCE, with the verified subject and nothing else on the port"
    );
    // (5) stamp_admin_put_body() is exactly { "role": "ADMIN" }.
    let body = infrastructure::integrations::supabase_auth::stamp_admin_put_body();
    assert_eq!(body["app_metadata"]["captain_food"], json!({ "role": "ADMIN" }), "m4's shape, pinned again here");

    let parked = door.sessions.parked();
    assert_eq!(parked.len(), 1, "exactly one session parked for POST /auth/session");
    assert_eq!(parked[0].message_id, message_id);
    assert_eq!(parked[0].session_id, Some(session));
    assert_eq!(parked[0].access_token, "rotated:ADMIN", "the POST-STAMP token is what gets parked");
}

const MAILBOX_LANES_QUERY: &str = r#"query { mailboxLanes { actorType partition } }"#;

async fn post_as_admin(door: &Door, jwt: &str) -> (String, Option<String>) {
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/admin/graphql")
        .header(axum::http::header::HOST, "system.captain.food")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(axum::body::Body::from(json!({ "query": MAILBOX_LANES_QUERY }).to_string()))
        .expect("request builds");
    let response = door.app.clone().oneshot(request).await.expect("router answers");
    assert_eq!(response.status(), axum::http::StatusCode::OK, "a verified ADMIN token authorizes on /admin");
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("a GraphQL response body");
    let err = body["errors"].as_array().and_then(|e| e.first()).cloned().unwrap_or(Value::Null);
    (
        err["message"].as_str().unwrap_or_default().to_string(),
        err["extensions"]["code"].as_str().map(str::to_string),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_token_the_admin_stamp_writes_opens_the_admin_door_once_the_seam_resolves_a_grant() {
    let sub = uuid::Uuid::from_u128(0x639_3D);
    let jwt = jwt_of_the_admin_stamp(sub);

    let resolved = door(
        ScriptedIdentity::default(),
        ScriptedPlatformMembers::default(),
        PlatformIdentityResolution::Resolved(()),
        true,
    ).await;
    let (message, code) = post_as_admin(&resolved, &jwt).await;
    assert_ne!(code.as_deref(), Some("FORBIDDEN"), "the stamped credential acts as ADMIN once the seam resolves a grant -- got: {message}");

    // THE CONTROL: the same credential with NO resolved grant is nobody -- m7's mutant hook.
    let unbound = door(ScriptedIdentity::default(), ScriptedPlatformMembers::default(), PlatformIdentityResolution::NoMapping, true).await;
    let (message, code) = post_as_admin(&unbound, &jwt).await;
    assert_eq!(code.as_deref(), Some("FORBIDDEN"), "no resolved grant => PUBLIC, whatever the token says -- got: {message}");
}

/// (6) the parent ADR SS8's answer, pinned end to end: a caller stamped ADMIN while a grant WAS
/// live, then the grant is withdrawn underneath the SAME still-valid cookie (no revoke command
/// exists to do this for real yet, so the scripted seam is flipped directly, which is exactly
/// what the seam's per-request re-derivation is FOR) -- the NEXT request refuses. m7's mutant
/// ("no-grant ADMIN admitted") is exactly what this test catches if the seam ever stopped
/// re-deriving and cached its first answer instead.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_grant_withdrawn_after_the_stamp_refuses_the_very_next_request_on_the_same_cookie() {
    let sub = uuid::Uuid::from_u128(0x639_3E);
    let jwt = jwt_of_the_admin_stamp(sub);

    let door = door(
        ScriptedIdentity::default(),
        ScriptedPlatformMembers::default(),
        PlatformIdentityResolution::Resolved(()),
        true,
    ).await;
    let (message, code) = post_as_admin(&door, &jwt).await;
    assert_ne!(code.as_deref(), Some("FORBIDDEN"), "while the grant is live, the SAME cookie opens the door -- got: {message}");

    // The grant is withdrawn (no revoke command exists yet -- the seam is re-asked and answers
    // differently, exactly as a real Postgres re-read would after a real revoke).
    *door.seam.0.lock().unwrap() = PlatformIdentityResolution::NoMapping;

    let (message, code) = post_as_admin(&door, &jwt).await;
    assert_eq!(
        code.as_deref(),
        Some("FORBIDDEN"),
        "the STILL-VALID ADMIN cookie must refuse the moment the seam re-derives no grant -- got: {message}"
    );
}

// ─── (4) the not-yet-granted refusal: nothing stamped, but the session IS parked ────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_email_is_refused_not_granted_but_the_session_is_still_parked_for_a_real_cookie() {
    let door = door(ScriptedIdentity::default(), ScriptedPlatformMembers::default(), PlatformIdentityResolution::NoMapping, true).await;
    let session = uuid::Uuid::from_u128(0x5E56);
    let message_id = uuid::Uuid::from_u128(0xA2);

    door.accept("confirmAdminSignIn", &confirm(&token_of(STRANGER_EMAIL), message_id), session).await;
    let (code, context) = rejection(door.deliver(message_id).await);
    assert_eq!(code, "AdminAccessNotGranted", "m3: a verified email with no live grant is REFUSED, nothing stamped");
    assert_eq!(context["email"], json!(STRANGER_EMAIL));
    assert_eq!(context["supportContact"], json!(SUPPORT));
    assert!(door.identity.admin_stamps.lock().unwrap().is_empty(), "m3: nothing stamped");
    let parked = door.sessions.parked();
    assert_eq!(parked.len(), 1, "the session IS parked");
    assert_eq!(parked[0].message_id, message_id);
}

// ─── An invalid/expired token: refused, nothing stamped ─────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_invalid_token_is_rejected_and_nothing_is_stamped() {
    let door = door(ScriptedIdentity::default(), a_granted_admin(), PlatformIdentityResolution::NoMapping, true).await;
    let session = uuid::Uuid::from_u128(0x5E56);
    let message_id = uuid::Uuid::from_u128(0xE2);

    door.accept("confirmAdminSignIn", &confirm("bad-token", message_id), session).await;
    let (code, _) = rejection(door.deliver(message_id).await);
    assert_eq!(code, "InvalidVerificationToken");
    assert!(door.identity.admin_stamps.lock().unwrap().is_empty());
    assert!(door.sessions.parked().is_empty(), "an invalid token is never spent, so nothing is ever parked for it");
}

// ─── One-subject-one-role: refuse, never overwrite ───────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_subject_already_holding_a_customer_claim_is_refused_and_nothing_is_overwritten() {
    let identity = ScriptedIdentity { holds_customer_claim: vec![subject_of(ADMIN_EMAIL)], ..Default::default() };
    let door = door(identity, a_granted_admin(), PlatformIdentityResolution::NoMapping, true).await;
    let session = uuid::Uuid::from_u128(0x5E56);
    let message_id = uuid::Uuid::from_u128(0xF3);

    door.accept("confirmAdminSignIn", &confirm(&token_of(ADMIN_EMAIL), message_id), session).await;
    let (code, context) = rejection(door.deliver(message_id).await);
    assert_eq!(code, "AuthSubjectHoldsAnotherRole");
    assert_eq!(context["authRef"], json!(subject_of(ADMIN_EMAIL)));
    assert!(door.identity.admin_stamps.lock().unwrap().is_empty());
    assert!(door.sessions.parked().is_empty());
}

// ─── Missing session: refused before the token is spent ─────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_confirm_without_a_session_header_is_refused_before_the_token_is_spent_and_nothing_is_parked() {
    let door = door(ScriptedIdentity::default(), a_granted_admin(), PlatformIdentityResolution::NoMapping, true).await;
    let message_id = uuid::Uuid::from_u128(0xF4);

    let acceptance = door.post_public_as(&confirm(&token_of(ADMIN_EMAIL), message_id), None).await;
    assert!(acceptance["errors"].is_null());
    let acceptance = acceptance["data"]["confirmAdminSignIn"].clone();
    assert!(acceptance["sessionId"].is_null());

    let (code, context) = rejection(door.deliver(message_id).await);
    assert_eq!(code, "AdminSignInRequiresSession");
    assert_eq!(context, json!({}));
    assert_eq!(*door.identity.verified.lock().unwrap(), 0, "refused BEFORE the token is spent");
    assert!(door.identity.admin_stamps.lock().unwrap().is_empty());
    assert!(door.sessions.parked().is_empty());
}
