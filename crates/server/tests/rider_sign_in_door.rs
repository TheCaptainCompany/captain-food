//! #639 part C step 2c-i — the rider sign-in door, backend half, through the REAL transport:
//! `POST /public/graphql` on the production `graphql_routes` (the `[PUBLIC]` guard, the typed
//! acceptance door, the `MemMailbox` row), then the row delivered through the HUMAN-OWNED router
//! (`infrastructure::inbox::route`, the same `RiderInbox` arm the mailbox worker runs) over a
//! SCRIPTED identity port and a scripted `Rider` bridge. No Postgres, never skips.
//!
//! What is pinned, in the card's order:
//!   (a) an UNKNOWN phone — verified, no rider behind it — is REFUSED (`RiderNotRegistered`,
//!       naming the support route), nothing is stamped, nothing is parked, nothing is appended:
//!       the door is IDENTIFY-ONLY, never register (the whole reason `verifyPhone` is not reused);
//!   (b) a KNOWN rider is stamped through `stamp_rider_claim` — the subject and nothing else on the
//!       port, `{ role: RIDER }` and nothing else on the wire — and the POST-STAMP session is
//!       parked for `POST /auth/session`, owned by the request's X-SESSION-ID; the customer stamper
//!       is never reached (compile-time selection, observed at runtime);
//!   (c) a subject the provider already holds with a `customer_id` is REFUSED
//!       (`AuthSubjectHoldsAnotherRole`) and nothing is overwritten — the one-subject-one-role
//!       collision, registered as a Concern on PROP-20260831-180622, fails CLOSED until decided;
//!   (d) the code REQUEST answers with the identical shape for a rider's phone and a stranger's,
//!       and the rider bridge is consulted ZERO times on that leg — no enumeration oracle;
//!   (e) end to end: a token carrying exactly what the rider stamp writes, signed, reaches
//!       `acceptDelivery` on `/rider/graphql` past `RoleGuard` once the seam resolves a row — the
//!       door actually opens — and stays FORBIDDEN as PUBLIC with no row (the 2b control leg);
//!   (f) a confirm carrying NO `X-SESSION-ID` is REFUSED (`RiderSignInRequiresSession`) BEFORE the
//!       OTP is spent — the verifier is never called, nothing is stamped, nothing is parked: a
//!       parked rider session always has an owner, so no header-less `POST /auth/session` can
//!       claim one (B1 of the independent review of #852; the `AuthSessionStore` both-`None`
//!       claim is another channel's contract and is untouched);
//!   (g) `SUPPORT_CONTACT` unset (development) fails CLOSED before the OTP is spent, with the
//!       loud unconfigured `Repository` error — nothing verified, sent, stamped or parked.
//!
//! Seen RED first on (a): before the handler existed, the router had no arm for
//! `RiderInbox::ConfirmRiderSignIn` (E0004 — the human-owned router's whole mechanism); the failure
//! text is in the PR body.

use std::sync::{Arc, Mutex};

use application::auth_sessions::mem::MemAuthSessionStore;
use application::auth_sessions::AuthSessionStore;
use application::generated::inboxes::ActorInbox;
use application::generated::services::{
    IdentityRefreshSessionInput, IdentityRefreshSessionOutput, IdentitySendEmailMagicLinkInput,
    IdentitySendPhoneOtpInput, IdentityService, IdentityStampCustomerClaimInput,
    IdentityStampRiderClaimInput, IdentityVerifyEmailTokenInput, IdentityVerifyEmailTokenOutput,
    IdentityVerifyPhoneOtpInput, IdentityVerifyPhoneOtpOutput, ServiceCallMeta,
};
use application::ports::{Actor, EventStore};
use application::queries::RiderIdentityRepository;
use async_trait::async_trait;
use domain::generated::scalars::{AuthSubject, EmailAddress, RiderId};
use domain::shared::errors::DomainError;
use infrastructure::inbox::{route, CommandDeps, InboxOutcome, RouterEnv};
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedPaymentGateway, PgAuthSubjectReservationRepository,
    PgCatalogRepository, PgCustomerRepository, PgProspectionRepository, PgRestaurantRepository,
    PgSlugReservationRepository, UnverifiedGbpOrderLinkProbe,
};
use serde_json::{json, Value};
use server::{
    CustomerIdentitySource, IdentitySources, ResolveRiderIdentity, RiderIdentityResolution,
    RiderIdentitySource,
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

/// A verified token carrying EXACTLY what the rider stamp writes — `app_metadata` taken from the
/// stamper's own PUT body, so this is the credential the door issues, not a hand-spelled lookalike.
fn jwt_of_the_rider_stamp(sub: uuid::Uuid) -> String {
    let body = infrastructure::integrations::supabase_auth::stamp_rider_put_body();
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

/// The identity provider, scripted: a phone verifies to a deterministic subject (`sub-<national>`),
/// every stamp and every send is RECORDED, the rotated token reflects only what was stamped, and
/// the CUSTOMER stamper panics — the rider door selects its stamper at compile time, and this is
/// that selection observed at runtime.
#[derive(Default)]
struct ScriptedIdentity {
    /// Canonical phones an OTP was sent to.
    sent: Mutex<Vec<String>>,
    /// How many OTPs were VERIFIED (spent): a refusal that must not cost the rider a code asserts
    /// this stayed at zero.
    verified: Mutex<u32>,
    /// Subjects the RIDER stamper was asked to stamp — the port input is the subject and nothing
    /// else; the wire shape is the adapter's (`stamp_rider_put_body`, pinned separately).
    rider_stamps: Mutex<Vec<String>>,
    /// Subjects the provider ALREADY holds with a customer claim: the rider stamp refuses them.
    holds_customer_claim: Vec<String>,
}

fn subject_of(national_number: &str) -> String {
    format!("sub-{}", national_number.trim_start_matches('0'))
}

#[async_trait]
impl IdentityService for ScriptedIdentity {
    async fn send_phone_otp(
        &self,
        input: IdentitySendPhoneOtpInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        self.sent
            .lock()
            .expect("scripted identity")
            .push(application::commands::canonical_phone(&input.dialing_code, &input.national_number).0);
        Ok(())
    }
    async fn verify_phone_otp(
        &self,
        input: IdentityVerifyPhoneOtpInput,
        _meta: &ServiceCallMeta,
    ) -> Result<IdentityVerifyPhoneOtpOutput, DomainError> {
        *self.verified.lock().expect("scripted identity") += 1;
        Ok(IdentityVerifyPhoneOtpOutput {
            auth_ref: AuthSubject(subject_of(&input.national_number.0)),
            access_token: Some("pre-stamp.access".into()),
            refresh_token: Some("pre-stamp.refresh".into()),
            expires_in: Some(3600),
        })
    }
    async fn refresh_session(
        &self,
        _input: IdentityRefreshSessionInput,
        _meta: &ServiceCallMeta,
    ) -> Result<IdentityRefreshSessionOutput, DomainError> {
        // A real provider re-mints the JWT from the user's CURRENT metadata at rotation.
        let stamped = !self.rider_stamps.lock().expect("scripted identity").is_empty();
        Ok(IdentityRefreshSessionOutput {
            access_token: if stamped { "rotated:RIDER".into() } else { "rotated:none".into() },
            refresh_token: Some("post-stamp.refresh".into()),
            expires_in: Some(3600),
        })
    }
    async fn stamp_customer_claim(
        &self,
        input: IdentityStampCustomerClaimInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        panic!(
            "the rider door reached the CUSTOMER stamper for {} -- the stampers are selected at \
             compile time and must never cross",
            input.auth_ref.0
        );
    }
    async fn stamp_rider_claim(
        &self,
        input: IdentityStampRiderClaimInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        if self.holds_customer_claim.contains(&input.auth_ref.0) {
            return Err(DomainError::rejected(
                "AuthSubjectHoldsAnotherRole",
                json!({ "authRef": input.auth_ref.0 }),
            ));
        }
        self.rider_stamps.lock().expect("scripted identity").push(input.auth_ref.0);
        Ok(())
    }
    async fn send_email_magic_link(
        &self,
        _input: IdentitySendEmailMagicLinkInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        panic!("the rider door never sends email")
    }
    async fn verify_email_token(
        &self,
        _input: IdentityVerifyEmailTokenInput,
        _meta: &ServiceCallMeta,
    ) -> Result<IdentityVerifyEmailTokenOutput, DomainError> {
        panic!("the rider door never verifies email")
    }
}

/// The `Rider` read model's bridge, scripted: the known logins, and a COUNT of consultations — the
/// enumeration property of the request leg is "this was asked zero times".
#[derive(Default)]
struct ScriptedRiders {
    known: Vec<(String, RiderId)>,
    consulted: Mutex<u32>,
}

#[async_trait]
impl RiderIdentityRepository for ScriptedRiders {
    async fn rider_id_by_auth_subject(
        &self,
        auth_subject: AuthSubject,
    ) -> Result<Option<(RiderId, domain::generated::scalars::RiderStanding)>, DomainError> {
        *self.consulted.lock().expect("scripted riders") += 1;
        Ok(self
            .known
            .iter()
            .find(|(s, _)| *s == auth_subject.0)
            .map(|(_, id)| (*id, domain::generated::scalars::RiderStanding::ACTIVE)))
    }
}

/// An `EventStore` the door must NEVER reach: the sign-in emits nothing, and an identify-only door
/// that appended anything would be registering. Any call is the test failing loudly.
struct UntouchableEventStore;

#[async_trait]
impl EventStore for UntouchableEventStore {
    async fn append(
        &self,
        stream_name: &str,
        _expected_version: i64,
        _events: &[domain::generated::events::DomainEvent],
        _actor: &Actor,
    ) -> Result<i64, DomainError> {
        panic!("the rider sign-in door must append NOTHING (stream {stream_name}) -- it identifies, it never registers");
    }
    async fn load(
        &self,
        stream_name: &str,
    ) -> Result<(Vec<domain::generated::events::DomainEvent>, i64), DomainError> {
        panic!("the rider sign-in door must load NO stream (stream {stream_name}) -- the read model is its source");
    }
}

/// A scripted request seam for the `/rider/graphql` leg (e).
struct ScriptedSeam(RiderIdentityResolution);

#[async_trait]
impl ResolveRiderIdentity for ScriptedSeam {
    async fn resolve(&self, _auth_subject: &str) -> RiderIdentityResolution {
        self.0.clone()
    }
}

// ─── The fixture: the production router + the production router's deps ──────────────────────────

struct Door {
    mailbox: Arc<actor_client::mailbox::mem::MemMailbox>,
    identity: Arc<ScriptedIdentity>,
    riders: Arc<ScriptedRiders>,
    sessions: Arc<MemAuthSessionStore>,
    deps: CommandDeps,
    app: axum::Router,
}

const SUPPORT: &str = "support@captain.food";

async fn door(identity: ScriptedIdentity, riders: ScriptedRiders, seam: RiderIdentityResolution) -> Door {
    let mailbox = Arc::new(actor_client::mailbox::mem::MemMailbox::default());
    let identity = Arc::new(identity);
    let riders = Arc::new(riders);
    let sessions = Arc::new(MemAuthSessionStore::default());
    let identity_port: Arc<dyn IdentityService> = identity.clone();
    let sessions_port: Arc<dyn application::auth_sessions::AuthSessionStore> = sessions.clone();
    let riders_port: Arc<dyn RiderIdentityRepository> = riders.clone();
    // NEVER connected: `connect_lazy` opens no connection, so every port the sign-in path does not
    // read keeps its PRODUCTION type without a database -- and any port that IS read by mistake
    // errors on a socket that does not exist, loudly, instead of answering from a lookalike.
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
            rider: RiderIdentitySource::new(Arc::new(ScriptedSeam(seam))),
        },
    )
    .layer(axum::Extension(server::AuthContext::from_config(
        jwks_endpoint().await,
        TEST_SUPABASE_URL.into(),
    )));
    // The worker side's deps, over the SAME ports the transport injected — the production
    // `CommandDeps`, every field, so a new port cannot be forgotten here either.
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
        riders: riders_port,
        support_contact: Some(EmailAddress(SUPPORT.into())),
        run_rider_restriction_door: false,
    };
    Door { mailbox, identity, riders, sessions, deps, app }
}

impl Door {
    /// One real request: `POST /public/graphql`, anonymous, with the X-SESSION-ID that will own the
    /// parked session. Returns the GraphQL response body.
    async fn post_public(&self, query: &str, session: uuid::Uuid) -> Value {
        self.post_public_as(query, Some(session)).await
    }

    /// The same request with the X-SESSION-ID header OPTIONAL — `None` sends no header at all,
    /// the shape a non-SDUI caller can produce (f).
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

    /// The typed acceptance of a mutation, or a panic naming the GraphQL error (a `[PUBLIC]` guard
    /// refusing the anonymous path would show up here, as FORBIDDEN).
    async fn accept(&self, field: &str, query: &str, session: uuid::Uuid) -> Value {
        self.accept_as(field, query, Some(session)).await
    }

    async fn accept_as(&self, field: &str, query: &str, session: Option<uuid::Uuid>) -> Value {
        let body = self.post_public_as(query, session).await;
        assert!(
            body["errors"].is_null(),
            "{field} must be ACCEPTED on /public/graphql, got errors: {}",
            body["errors"]
        );
        let acceptance = body["data"][field].clone();
        assert_eq!(acceptance["operationStatus"], "PENDING", "acceptance-first: {acceptance}");
        acceptance
    }

    /// Deliver the accepted row exactly as the mailbox worker would: parse the wire triple into
    /// the typed inbox, mint the actor from the envelope, run the human-owned router.
    async fn deliver(&self, message_id: uuid::Uuid) -> Result<(), DomainError> {
        let entry = self.mailbox.entry(message_id).expect("the acceptance landed one mailbox row");
        assert_eq!(entry.actor_type(), "Rider", "the sign-in commands ride the Rider lane");
        let inbox = ActorInbox::parse(entry.actor_type(), entry.message_type(), entry.payload())
            .expect("the row parses into the typed Rider inbox");
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

fn request_code(national: &str, message_id: uuid::Uuid) -> String {
    format!(
        r#"mutation {{ requestRiderSignInCode(input: {{ dialingCode: "+33", nationalNumber: "{national}" }}, metadata: {{ messageId: "{message_id}" }}) {{ messageId correlationId sessionId operationStatus duplicate }} }}"#
    )
}

fn confirm(national: &str, message_id: uuid::Uuid) -> String {
    format!(
        r#"mutation {{ confirmRiderSignIn(input: {{ dialingCode: "+33", nationalNumber: "{national}", code: "123456" }}, metadata: {{ messageId: "{message_id}" }}) {{ messageId correlationId sessionId operationStatus duplicate }} }}"#
    )
}

fn rejection(result: Result<(), DomainError>) -> (String, Value) {
    match result {
        Err(DomainError::Rejected { code, context }) => (code, context),
        other => panic!("expected a TYPED rejection, got {other:?}"),
    }
}

const RIDER_PHONE: &str = "611223344";
const STRANGER_PHONE: &str = "612345678";

fn a_known_rider() -> ScriptedRiders {
    ScriptedRiders {
        known: vec![(subject_of(RIDER_PHONE), RiderId(uuid::Uuid::from_u128(0x600D)))],
        consulted: Mutex::new(0),
    }
}

// ─── (a) identify-only: an unknown phone is refused and creates nothing ─────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_phone_is_refused_and_nothing_is_stamped_parked_or_appended() {
    let door = door(ScriptedIdentity::default(), ScriptedRiders::default(), RiderIdentityResolution::NoMapping).await;
    let session = uuid::Uuid::from_u128(0x5E55);
    let message_id = uuid::Uuid::from_u128(0xA1);

    let acceptance = door.accept("confirmRiderSignIn", &confirm(STRANGER_PHONE, message_id), session).await;
    assert_eq!(acceptance["messageId"], message_id.to_string());

    let (code, context) = rejection(door.deliver(message_id).await);
    assert_eq!(code, "RiderNotRegistered", "a verified phone with no rider behind it is REFUSED");
    assert_eq!(
        context["supportContact"],
        json!(SUPPORT),
        "and the refusal names the support route from SUPPORT_CONTACT, never a hard-coded string"
    );
    assert!(door.identity.rider_stamps.lock().unwrap().is_empty(), "nothing stamped");
    assert!(door.sessions.parked().is_empty(), "nothing parked -- an unstamped token is never parked");
    // Nothing appended: `UntouchableEventStore` would have panicked. Registering is unspellable here.
    assert_eq!(*door.riders.consulted.lock().unwrap(), 1, "the bridge was asked exactly once");
}

// ─── (b) a known rider: the RIDER role only, and the post-stamp session parked ──────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_known_rider_is_stamped_with_the_rider_role_only_and_the_post_stamp_session_is_parked() {
    let door = door(ScriptedIdentity::default(), a_known_rider(), RiderIdentityResolution::NoMapping).await;
    let session = uuid::Uuid::from_u128(0x5E55);
    let message_id = uuid::Uuid::from_u128(0xB1);

    door.accept("confirmRiderSignIn", &confirm(RIDER_PHONE, message_id), session).await;
    door.deliver(message_id).await.expect("a known rider signs in");

    assert_eq!(
        *door.identity.rider_stamps.lock().unwrap(),
        vec![subject_of(RIDER_PHONE)],
        "stamp_rider_claim was called ONCE, with the verified subject and nothing else on the port"
    );
    // The WIRE shape is the adapter's, pinned here beside the port call so the two read together:
    // the whole `captain_food` object is `{ role: RIDER }` -- no rider_id, no id of any kind.
    let body = infrastructure::integrations::supabase_auth::stamp_rider_put_body();
    assert_eq!(body["app_metadata"]["captain_food"], json!({ "role": "RIDER" }));

    let parked = door.sessions.parked();
    assert_eq!(parked.len(), 1, "exactly one session parked for POST /auth/session");
    assert_eq!(parked[0].message_id, message_id, "keyed by the acceptance messageId");
    assert_eq!(parked[0].session_id, Some(session), "owned by the request's X-SESSION-ID (envelope, not payload)");
    assert_eq!(
        parked[0].access_token, "rotated:RIDER",
        "the POST-STAMP token is what gets parked -- never the pre-stamp one"
    );
}

// ─── (c) one-subject-one-role: refuse, never overwrite ───────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_subject_already_holding_a_customer_claim_is_refused_and_nothing_is_overwritten() {
    let identity = ScriptedIdentity { holds_customer_claim: vec![subject_of(RIDER_PHONE)], ..Default::default() };
    let door = door(identity, a_known_rider(), RiderIdentityResolution::NoMapping).await;
    let session = uuid::Uuid::from_u128(0x5E55);
    let message_id = uuid::Uuid::from_u128(0xC1);

    door.accept("confirmRiderSignIn", &confirm(RIDER_PHONE, message_id), session).await;
    let (code, context) = rejection(door.deliver(message_id).await);
    assert_eq!(code, "AuthSubjectHoldsAnotherRole", "a rider who also orders dinner is refused, fail closed");
    assert_eq!(context["authRef"], json!(subject_of(RIDER_PHONE)));
    assert!(door.identity.rider_stamps.lock().unwrap().is_empty(), "the customer claim was NOT overwritten");
    assert!(door.sessions.parked().is_empty(), "and no session was parked");
}

// ─── (d) no enumeration oracle on the request leg ────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_code_request_answers_identically_for_a_rider_and_a_stranger() {
    let door = door(ScriptedIdentity::default(), a_known_rider(), RiderIdentityResolution::NoMapping).await;
    let session = uuid::Uuid::from_u128(0x5E55);
    let for_rider = uuid::Uuid::from_u128(0xD1);
    let for_stranger = uuid::Uuid::from_u128(0xD2);

    let mut rider = door.accept("requestRiderSignInCode", &request_code(RIDER_PHONE, for_rider), session).await;
    let mut stranger = door.accept("requestRiderSignInCode", &request_code(STRANGER_PHONE, for_stranger), session).await;
    // The only fields allowed to differ are the ids the CALLER chose.
    for shape in [&mut rider, &mut stranger] {
        shape["messageId"] = Value::Null;
        shape["correlationId"] = Value::Null;
    }
    assert_eq!(rider, stranger, "the acceptance is byte-identical whether or not the phone is a rider's");

    door.deliver(for_rider).await.expect("the send leg succeeds for a rider");
    door.deliver(for_stranger).await.expect("and identically for a stranger");
    assert_eq!(
        *door.identity.sent.lock().unwrap(),
        vec!["+33611223344".to_string(), "+33612345678".to_string()],
        "both phones were sent a code"
    );
    assert_eq!(
        *door.riders.consulted.lock().unwrap(),
        0,
        "the rider bridge is consulted ZERO times on the request leg -- rider-ness cannot leak from a path that never reads it"
    );
}

// ─── (e) end to end: the issued credential opens the rider door ──────────────────────────────────

// #865: `riderId` carries no field on `AcceptDeliveryInput` any more (`derived: { riderId: rider
// }`) — the seam this door resolves supplies it, never the literal.
const ACCEPT_DELIVERY: &str = r#"mutation {
  acceptDelivery(input: {
    deliveryJobId: "00000000-0000-0000-0000-00000000000d"
  }) { messageId }
}"#;

async fn post_as_rider(door: &Door, jwt: &str) -> (String, Option<String>) {
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/rider/graphql")
        .header(axum::http::header::HOST, "chez-test.captain.food")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {jwt}"))
        .body(axum::body::Body::from(json!({ "query": ACCEPT_DELIVERY }).to_string()))
        .expect("request builds");
    let response = door.app.clone().oneshot(request).await.expect("router answers");
    assert_eq!(response.status(), axum::http::StatusCode::OK, "a verified RIDER token authorizes on /rider");
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body");
    let body: Value = serde_json::from_slice(&bytes).expect("a GraphQL response body");
    let err = body["errors"].as_array().and_then(|e| e.first()).cloned().unwrap_or(Value::Null);
    (
        err["message"].as_str().unwrap_or_default().to_string(),
        err["extensions"]["code"].as_str().map(str::to_string),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_token_the_rider_stamp_writes_opens_the_rider_door_once_the_seam_resolves_a_row() {
    let sub = uuid::Uuid::from_u128(0x639_2C);
    let jwt = jwt_of_the_rider_stamp(sub);

    // THE DOOR OPENS: the seam answers a row, the guard passes, the resolver runs -- injecting
    // `riderId` from the SAME resolved `ReadScope::Rider` (#865) and enqueuing on this door's real
    // mailbox. Whether that lands PENDING or fails on something downstream is not the point; not
    // FORBIDDEN is.
    let resolved = door(ScriptedIdentity::default(), ScriptedRiders::default(),
        RiderIdentityResolution::Resolved((RiderId(uuid::Uuid::from_u128(0x600D)), domain::generated::scalars::RiderStanding::ACTIVE))).await;
    let (message, code) = post_as_rider(&resolved, &jwt).await;
    assert_ne!(code.as_deref(), Some("FORBIDDEN"), "the stamped credential acts as RIDER -- got: {message}");
    assert!(!message.starts_with("forbidden:"), "past the guard -- got: {message}");
    // #865 review round 2: this harness carries a REAL mailbox -- assert the enqueued payload
    // directly, not just "not forbidden". The derived seam injected `riderId` from the SAME
    // resolved `ReadScope::Rider` this test scripted (0x600D), never from the literal (which
    // carries no such field at all since #865).
    let entries = resolved.mailbox.entries();
    assert_eq!(
        entries.len(),
        1,
        "exactly one command should have enqueued: {:?}",
        entries.iter().map(|e| e.message_type().to_string()).collect::<Vec<_>>()
    );
    assert_eq!(
        entries[0].payload().get("riderId").and_then(|v| v.as_str()),
        Some(uuid::Uuid::from_u128(0x600D).to_string().as_str()),
        "the enqueued payload's riderId must be the seam's resolved rider -- got: {:?}",
        entries[0].payload()
    );

    // THE CONTROL (2b): the same credential with NO row is nobody -- the stamp carries no binding,
    // Postgres does, so a stamped token alone opens nothing.
    let unbound = door(ScriptedIdentity::default(), ScriptedRiders::default(), RiderIdentityResolution::NoMapping).await;
    let (message, code) = post_as_rider(&unbound, &jwt).await;
    assert_eq!(code.as_deref(), Some("FORBIDDEN"), "no row => PUBLIC, whatever the token says -- got: {message}");
}

// ─── (f) no session, no credential: refused before the OTP is spent ─────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_confirm_without_a_session_header_is_refused_before_the_otp_is_spent_and_nothing_is_parked() {
    // A KNOWN rider with the RIGHT code: the missing session is the ONLY reason to refuse.
    let door = door(ScriptedIdentity::default(), a_known_rider(), RiderIdentityResolution::NoMapping).await;
    let message_id = uuid::Uuid::from_u128(0xF1);

    // The transport ACCEPTS a header-less confirm (the generated dispatch yields `Option`), and
    // the acceptance itself says so: no session travelled with the row.
    let acceptance = door.accept_as("confirmRiderSignIn", &confirm(RIDER_PHONE, message_id), None).await;
    assert!(acceptance["sessionId"].is_null(), "no X-SESSION-ID was sent, none is echoed: {acceptance}");

    let (code, context) = rejection(door.deliver(message_id).await);
    assert_eq!(
        code, "RiderSignInRequiresSession",
        "a confirm with no session to OWN the parked credential is refused -- never parked ownerless"
    );
    assert_eq!(context, json!({}), "the refusal carries nothing");
    assert_eq!(
        *door.identity.verified.lock().unwrap(),
        0,
        "refused BEFORE the OTP is spent -- the code stays usable for a correct retry"
    );
    assert_eq!(*door.riders.consulted.lock().unwrap(), 0, "the bridge was never asked");
    assert!(door.identity.rider_stamps.lock().unwrap().is_empty(), "nothing stamped");
    assert!(door.sessions.parked().is_empty(), "nothing parked -- a parked rider session always has an owner");
    // And the ownerless claim the review described cannot happen: a header-less claim finds nothing.
    assert!(
        door.sessions.claim(message_id, None).await.expect("claim answers").is_none(),
        "no header-less POST /auth/session can claim a credential for this messageId"
    );
}

// ─── (g) SUPPORT_CONTACT unset: fail closed, before the OTP is spent ─────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn support_contact_unset_fails_closed_before_the_otp_is_spent_and_nothing_is_sent_stamped_or_parked() {
    use domain::generated::scalars::{DialingCode, NationalPhoneNumber, OtpCode, SessionId};

    let identity = ScriptedIdentity::default();
    let riders = a_known_rider();
    let sessions = MemAuthSessionStore::default();
    let cmd = domain::generated::commands::ConfirmRiderSignIn {
        dialing_code: DialingCode("+33".into()),
        national_number: NationalPhoneNumber(RIDER_PHONE.into()),
        code: OtpCode("123456".into()),
    };
    let actor = Actor {
        user_id: uuid::Uuid::nil(),
        user_type: "PUBLIC".into(),
        domain_id: None,
        correlation_id: uuid::Uuid::from_u128(0x61),
        cause_id: Some(uuid::Uuid::from_u128(0x62)),
    };
    // Everything a sign-in needs is present -- a known rider, the right code, an owning session --
    // EXCEPT the support route the refusal path would name.
    let result = application::commands::confirm_rider_sign_in(
        &UntouchableEventStore,
        &identity,
        &riders,
        &sessions,
        None,
        cmd,
        Some(SessionId(uuid::Uuid::from_u128(0x5E55))),
        &actor,
    )
    .await;

    match result {
        Err(DomainError::Repository(msg)) => assert!(
            msg.contains("SUPPORT_CONTACT"),
            "the loud unconfigured error names the key: {msg}"
        ),
        other => panic!("SUPPORT_CONTACT unset must fail CLOSED with the Repository error, got {other:?}"),
    }
    assert_eq!(*identity.verified.lock().unwrap(), 0, "refused BEFORE the OTP is spent");
    assert!(identity.sent.lock().unwrap().is_empty(), "nothing sent");
    assert!(identity.rider_stamps.lock().unwrap().is_empty(), "nothing stamped");
    assert_eq!(*riders.consulted.lock().unwrap(), 0, "the bridge was never asked");
    assert!(sessions.parked().is_empty(), "nothing parked");
    assert!(
        sessions.claim(uuid::Uuid::from_u128(0x62), Some(uuid::Uuid::from_u128(0x5E55))).await.expect("claim answers").is_none(),
        "and nothing is claimable"
    );
}
