//! The card's "walk" (farley, beck): a DB-gated end-to-end pass over the REAL mailbox worker /
//! projector stack for the platform grant and the ADMIN seam binding (#639 part C step 6-v,
//! ADR-20260905-223957). Proves the one-shot bootstrap's idempotency (`the_bootstrap_
//! replays_from_domain_events_alone`, `running_it_twice_appends_one_fact`) and the full chain: a
//! real `GrantPlatformAccess` dispatch through the real mailbox, a real `MailboxWorker` appends
//! `PlatformAccessGranted`, a real `ProjectionWorker::run_once()` fold flips the ADMIN seam
//! (`resolve_platform_scope` -> `PgPlatformIdentity` -> `platform_member`) from `NoMapping` to
//! resolved. Round 2, R2-7(a) (beck): this file exercises the seam's PORT directly
//! (`server::ResolvePlatformIdentity::resolve`), never an actual `/admin/graphql riders` HTTP
//! request -- no test here issues one; the HTTP/GraphQL-layer scripted equivalent lives in
//! `an_admin_token_with_no_platform_grant_is_unbound.rs`.
//!
//! Needs a real Postgres (`DATABASE_URL`); SKIPS (prints and returns) without one, same as every
//! other DB-gated suite here (`DB_TESTS_REQUIRED=1` makes that a hard failure, beck CATCH).
//!
//! #639 part C step 6-iii RESUME (R-1, ADR-20260906-023825 fenced-off item 8): a SECOND walk,
//! `requesting_and_confirming_an_admin_sign_in_opens_the_admin_door_end_to_end`, extends the SAME
//! real mailbox-worker/real-projector stack one lane further, to the ADMIN sign-in door itself:
//! bootstrap -> `requestAdminSignInLink` -> `confirmAdminSignIn` over the REAL `/public/graphql`
//! router, delivered by the SAME real `MailboxWorker` (the `AdminSignIn` lane joins
//! `ACTOR_MAILBOXES` for free) -> `POST /auth/session` claims the parked cookie -> `/admin/graphql
//! mailboxLanes` is admitted. Only the identity PROVIDER is scripted (no real Supabase in this
//! test run); the mailbox, the `PlatformMember` bridge and the ADMIN seam (`PgPlatformIdentity`)
//! are all real Postgres, and the scripted rotation mints a REAL, independently verifiable JWT for
//! the subject it just stamped (the `admin_sign_in_door.rs::jwt_of_the_admin_stamp` shape, driven
//! through the actual rotation call instead of hand-substituted at the assertion) -- so the cookie
//! `POST /auth/session` sets is the credential the door actually issued, not a lookalike.

use std::sync::Arc;

use infrastructure::persistence::mailbox_store::PgMailbox;
use infrastructure::ProjectionWorker;
use serde_json::json;
use sqlx::PgPool;

/// Drop `public` and replay the REAL migration chain from disk, in filename order -- the
/// `rider_standing_walk.rs` precedent.
async fn apply_all_migrations(pool: &PgPool) {
    sqlx::raw_sql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .execute(pool)
        .await
        .expect("recreate the public schema");
    let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../migrations"));
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .expect("read migrations/")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "sql"))
        .collect();
    files.sort();
    for f in files {
        let sql = std::fs::read_to_string(&f).unwrap_or_else(|e| panic!("read {}: {e}", f.display()));
        sqlx::raw_sql(&sql)
            .execute(pool)
            .await
            .unwrap_or_else(|e| panic!("apply migration {}: {e}", f.display()));
    }
}

/// Spawn a REAL mailbox worker for the `PlatformMembership` lane, the door gated ON -- the
/// `rider_standing_walk.rs::spawn_mailbox_workers_with_door` precedent, narrowed to what this
/// slice's walk needs.
fn spawn_mailbox_workers(pool: &PgPool, bus: actor_client::OperationStatusBus) {
    let deps = infrastructure::generated::command_router::CommandDeps {
        store: Arc::new(infrastructure::PgEventStore::new(pool.clone())),
        restaurants: Arc::new(infrastructure::PgRestaurantRepository::new(pool.clone())),
        slugs: Arc::new(infrastructure::PgSlugReservationRepository::new(pool.clone())),
        auth_subjects: Arc::new(infrastructure::PgAuthSubjectReservationRepository::new(pool.clone())),
        ownership: Arc::new(infrastructure::FailClosedGoogleOwnershipVerifier),
        probe: Arc::new(infrastructure::UnverifiedGbpOrderLinkProbe),
        prospection: Arc::new(infrastructure::PgProspectionRepository::new(pool.clone())),
        catalogs: Arc::new(infrastructure::PgCatalogRepository::new(pool.clone())),
        auth: Arc::new(infrastructure::FailClosedIdentityService),
        customers: Arc::new(infrastructure::PgCustomerRepository::new(pool.clone())),
        sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
        payments: Arc::new(infrastructure::FailClosedPaymentGateway),
        pm_state: Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone())),
        refund_state: Arc::new(infrastructure::persistence::PgRefundProcessState::new(pool.clone())),
        mailbox_requeue: Arc::new(infrastructure::persistence::mailbox_lanes::PgMailboxRequeue::new(pool.clone())),
        riders: Arc::new(infrastructure::PgRiderRepository::new(pool.clone())),
        members: Arc::new(infrastructure::PgMemberRepository::new(pool.clone())),
        support_contact: None,
        run_rider_restriction_door: false,
        run_member_access_grant: false,
        run_member_sign_in_door: false,
        run_restaurant_invitation: false,
        // THE DOOR UNDER TEST, ON: this walk proves the grant path, not the door-closed refusal
        // (that is `TestGrantPlatformAccessDoorClosed` in the behaviour suite).
        run_platform_access_grant: true,
        run_admin_sign_in_door: false,
        platform_members: Arc::new(infrastructure::PgPlatformMemberRepository::new(pool.clone())),
        enforce_service_hours_guard: false,
        enforce_acceptance_timeout: false,
        route_gates: application::generated::process_managers::RouteGates {
            order_placed_to_order: true,
            place_replacement_order_to_order: false,
            bind_cart_to_customer_to_cart: false,
            grant_customer_credit_to_customer_credit: false,
            mark_order_delivered_to_order: false,
        },
    };
    let handler = Arc::new(infrastructure::mailbox::MailboxCommandHandler::new(deps));
    let observer = Arc::new(infrastructure::mailbox::StatusBusObserver::new(bus));
    for (actor_type, width) in infrastructure::generated::command_router::ACTOR_MAILBOXES {
        let worker = actor_runtime::MailboxWorker::new(
            pool.clone(),
            "w-platform-admin-walk",
            *actor_type,
            actor_runtime::WorkerConfig { heartbeat_seconds: 1, ..Default::default() },
            handler.clone(),
        )
        .with_observer(observer.clone());
        let width = *width as i16;
        let (_tx, rx) = tokio::sync::watch::channel(false);
        std::mem::forget(_tx);
        tokio::spawn(async move {
            worker.seed(width).await.expect("seed");
            let _ = worker.run(rx).await;
        });
    }
}

static DB_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Poll `domain_events` for `stream` until it carries `at_least` rows or the timeout elapses --
/// the bootstrap path returns no `messageId` to poll via `operationStatus` (an operator command,
/// not a GraphQL client), so this is the walk's own wait-for-drain.
async fn wait_for_events(pool: &PgPool, stream: &str, at_least: i64) -> i64 {
    for _ in 0..100 {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM domain_events WHERE stream_name = $1")
            .bind(stream)
            .fetch_one(pool)
            .await
            .expect("count events");
        if count >= at_least {
            return count;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("stream {stream} did not reach {at_least} event(s) in time");
}

/// Round 2, R2-4 (dba): the NO-SEEDED-ROW proof, NOT the table's operational rebuild recipe --
/// `platform_member`'s rule 1 FORBIDS TRUNCATE as the operational rebuild (the denial window it
/// opens); this test's job is narrower and different: proving the one-shot bootstrap replays
/// cleanly from `domain_events` alone, with NO seeded projection row able to fake a pass, because
/// the projection is TRUNCATEd and the checkpoint reset to 0 between the dispatch and the
/// re-check -- only the immutable fact survives. The DECLARED operational recipe (checkpoint
/// reset, never TRUNCATE, no denial window) has its OWN test right below,
/// `platform_admin_resolves_at_every_point_of_a_checkpoint_reset_replay` -- the
/// `restaurant_membership.rs:~216` precedent transposed.
#[tokio::test]
async fn the_bootstrap_replays_from_domain_events_alone() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let Some(url) = db_test_gate::database_url("platform_admin_bootstrap_replay") else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    apply_all_migrations(&pool).await;

    let auth_subject = "auth-first-admin-replay";
    let code = server::bootstrap_platform_admin::dispatch(&url, auth_subject).await;
    assert_eq!(code, 0, "the bootstrap dispatch must succeed");

    // Drain the mailbox row the dispatch enqueued -- the bootstrap itself only enqueues
    // (acceptance-first, PENDING); a REAL worker is what appends the fact, exactly as production
    // requires one running.
    let status_bus = actor_client::OperationStatusBus::default();
    spawn_mailbox_workers(&pool, status_bus);

    let platform_membership_id = server::bootstrap_platform_admin::platform_membership_id_for(auth_subject);
    let stream = format!("PlatformMembership-{}", platform_membership_id.0);
    let events_before = wait_for_events(&pool, &stream, 1).await;
    assert_eq!(events_before, 1, "exactly one PlatformAccessGranted fact must exist");

    // TRUNCATE the projection and reset its checkpoint to 0 -- the ONLY way a row can be seeded
    // here is by folding `domain_events` again from scratch (never a seeded row).
    sqlx::query("TRUNCATE platform_member")
        .execute(&pool)
        .await
        .expect("truncate platform_member");
    sqlx::query("UPDATE projection_checkpoint SET position = 0 WHERE projector = 'PlatformMember'")
        .execute(&pool)
        .await
        .expect("reset the PlatformMember checkpoint");

    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (replay)");

    let row: Option<(String,)> =
        sqlx::query_as("SELECT auth_subject FROM platform_member WHERE platform_membership_id = $1")
            .bind(platform_membership_id.0)
            .fetch_optional(&pool)
            .await
            .expect("query platform_member");
    assert_eq!(
        row.map(|(s,)| s),
        Some(auth_subject.to_string()),
        "the admin must resolve again after a from-zero replay -- the fact alone is the source of truth"
    );
}

/// Round 2, R2-4 (dba): the DECLARED operational rebuild recipe for `platform_member`, as an
/// executable test -- checkpoint reset, NEVER TRUNCATE (the `restaurant_membership.rs:~216`
/// `member_resolves_at_every_point_of_a_checkpoint_reset_replay` precedent transposed). The row is
/// never deleted by a reset alone, so it resolves at every point of the drain -- including the
/// instant immediately after the reset, BEFORE any replay has run at all -- and the replay then
/// rewrites it in place with no denial window. `the_bootstrap_replays_from_domain_events_alone`
/// above is the DIFFERENT, narrower no-seeded-row proof (TRUNCATE + reset); this test is the one
/// that proves the table's own rule 1 (rebuild = checkpoint reset, never TRUNCATE).
#[tokio::test]
async fn platform_admin_resolves_at_every_point_of_a_checkpoint_reset_replay() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let Some(url) = db_test_gate::database_url("platform_admin_checkpoint_reset") else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    apply_all_migrations(&pool).await;

    let auth_subject = "auth-first-admin-checkpoint-reset";
    let code = server::bootstrap_platform_admin::dispatch(&url, auth_subject).await;
    assert_eq!(code, 0, "the bootstrap dispatch must succeed");

    let status_bus = actor_client::OperationStatusBus::default();
    spawn_mailbox_workers(&pool, status_bus);

    let platform_membership_id = server::bootstrap_platform_admin::platform_membership_id_for(auth_subject);
    let stream = format!("PlatformMembership-{}", platform_membership_id.0);
    wait_for_events(&pool, &stream, 1).await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (first drain)");

    async fn platform_member_auth_subject(pool: &PgPool, id: uuid::Uuid) -> Option<String> {
        sqlx::query_as::<_, (String,)>(
            "SELECT auth_subject FROM platform_member WHERE platform_membership_id = $1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .expect("query platform_member")
        .map(|(s,)| s)
    }

    assert_eq!(
        platform_member_auth_subject(&pool, platform_membership_id.0).await.as_deref(),
        Some(auth_subject),
        "the admin resolves after the first drain"
    );

    // Checkpoint reset, NEVER TRUNCATE: rewind the PlatformMember checkpoint to 0.
    let reset = sqlx::query("UPDATE projection_checkpoint SET position = 0 WHERE projector = 'PlatformMember'")
        .execute(&pool)
        .await
        .expect("rewind the PlatformMember checkpoint");
    assert_eq!(
        reset.rows_affected(),
        1,
        "the projector name must match the registered 'PlatformMember' group -- a rename here \
         would pass vacuously with 0 rows touched (the round-2 R2-4 beck finding, transposed)"
    );

    // The claim, checked at its strongest point: RIGHT AFTER the reset, BEFORE the replay runs.
    assert_eq!(
        platform_member_auth_subject(&pool, platform_membership_id.0).await.as_deref(),
        Some(auth_subject),
        "a checkpoint reset alone must never deny -- the row was never touched, no denial window"
    );

    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (replay)");
    assert_eq!(
        platform_member_auth_subject(&pool, platform_membership_id.0).await.as_deref(),
        Some(auth_subject),
        "the replay reproduces the same row -- rewritten in place, never seeded"
    );
}

/// Running the bootstrap TWICE against the SAME subject appends exactly ONE fact
/// (ADR-20260905-223957 §3): the deterministic `platformMembershipId` targets the SAME stream, and
/// the aggregate's own idempotency (`PlatformAccessGrantIsIdempotent`) makes the second call a
/// no-op, never a duplicate grant.
#[tokio::test]
async fn running_it_twice_appends_one_fact() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let Some(url) = db_test_gate::database_url("platform_admin_bootstrap_twice") else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    apply_all_migrations(&pool).await;

    let status_bus = actor_client::OperationStatusBus::default();
    spawn_mailbox_workers(&pool, status_bus);

    let auth_subject = "auth-first-admin-twice";
    let code1 = server::bootstrap_platform_admin::dispatch(&url, auth_subject).await;
    assert_eq!(code1, 0, "the first dispatch must succeed");
    let platform_membership_id = server::bootstrap_platform_admin::platform_membership_id_for(auth_subject);
    let stream = format!("PlatformMembership-{}", platform_membership_id.0);
    wait_for_events(&pool, &stream, 1).await;

    let code2 = server::bootstrap_platform_admin::dispatch(&url, auth_subject).await;
    assert_eq!(code2, 0, "the second dispatch must succeed too -- deduplicated, never an error");
    // No SECOND fact should ever land; give the (already-drained) worker a moment in case the
    // second dispatch mistakenly enqueued a fresh row, then assert the count stayed at 1.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM domain_events WHERE stream_name = $1")
        .bind(&stream)
        .fetch_one(&pool)
        .await
        .expect("count events");
    assert_eq!(events, 1, "running the bootstrap twice must append exactly ONE fact");

    // The named mutant (beck): "the bootstrap appending twice" would show 2 here -- this
    // assertion is what a regression to that shape fails on.
    let mailbox_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM inbound_messages WHERE actor_type = 'PlatformMembership'",
    )
    .fetch_one(&pool)
    .await
    .expect("count mailbox rows");
    assert_eq!(
        mailbox_rows, 1,
        "the deterministic message_id must deduplicate at the mailbox layer too -- a re-run is \
         the SAME row, never a second enqueue"
    );
}

/// The full write-then-read chain: a grant dispatched through the real mailbox is folded by a
/// real projector, and the seam's PORT (`server::ResolvePlatformIdentity::resolve`, backed by the
/// real `PgPlatformIdentity` -> `platform_member`) answers `NoMapping` BEFORE the grant lands and
/// resolves AFTER. Round 2, R2-7(a) (beck): this test issues no HTTP request at all -- it never
/// calls `/admin/graphql` or reads `riders` (that GraphQL/HTTP-layer scripted equivalent is
/// `an_admin_token_with_no_platform_grant_is_unbound.rs`, exercising a different, non-Postgres
/// seam). The mailbox/projector chain here proves the WRITE half; the direct port call proves the
/// READ half consumes what it wrote.
#[tokio::test]
async fn the_admin_seam_resolves_only_after_the_grant_lands() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let Some(url) = db_test_gate::database_url("platform_admin_walk_seam") else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    apply_all_migrations(&pool).await;

    let status_bus = actor_client::OperationStatusBus::default();
    spawn_mailbox_workers(&pool, status_bus.clone());

    let auth_subject = "auth-walk-admin-1";
    let seam = server::PgPlatformIdentity::new(Arc::new(infrastructure::PgPlatformMemberRepository::new(pool.clone())));

    // BEFORE the grant: NoMapping.
    let before = server::ResolvePlatformIdentity::resolve(&seam, auth_subject).await;
    assert_eq!(
        before,
        server::PlatformIdentityResolution::NoMapping,
        "before any grant, the seam must answer NoMapping"
    );

    // Dispatch the grant through the REAL router -- `grantPlatformAccess`, ADMIN-only, so this
    // dispatch is enqueued as if an EXISTING admin (the fixed `acting()` witness) issued it.
    let mailbox: Arc<dyn actor_client::mailbox::Mailbox> = Arc::new(PgMailbox::new(pool.clone()));
    // Round 2, R2-5: platformMembershipId must equal platform_membership_id_for(authSubject) --
    // a random uuid::Uuid::new_v4() would now be refused with PlatformMembershipIdMismatch before
    // the store is even touched, and the seam would never see a grant land at all.
    let platform_membership_id =
        server::bootstrap_platform_admin::platform_membership_id_for(auth_subject).0;
    let client = client_platform_membership::PlatformMembershipClient::new(mailbox.clone(), platform_membership_id);
    let cmd = domain::generated::commands::GrantPlatformAccess {
        platform_membership_id: domain::generated::scalars::PlatformMembershipId(platform_membership_id),
        auth_subject: domain::generated::scalars::AuthSubject(auth_subject.to_string()),
        basis: domain::generated::scalars::PlatformAccessBasis::CAPTAIN_ONBOARDING,
    };
    let env = actor_client::mailbox::Envelope {
        message_id: uuid::Uuid::new_v4(),
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
        session_id: None,
        trace_id: None,
        user_id: Some(uuid::Uuid::from_u128(0xAD)),
        user_type: "ADMIN".to_string(),
        channel: "GRAPHQL".to_string(),
    };
    client.send(cmd, env).await.expect("enqueue GrantPlatformAccess");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (grant)");

    // AFTER the grant: Resolved.
    let after = server::ResolvePlatformIdentity::resolve(&seam, auth_subject).await;
    assert_eq!(
        after,
        server::PlatformIdentityResolution::Resolved(()),
        "after the grant lands and the projector folds it, the seam must resolve"
    );

    // A DIFFERENT subject never granted stays unresolved -- the bridge answers per-subject, never
    // "any grant exists at all".
    let stranger = server::ResolvePlatformIdentity::resolve(&seam, "auth-never-granted").await;
    assert_eq!(stranger, server::PlatformIdentityResolution::NoMapping);
}

// ─── #639 part C step 6-iii RESUME (R-1): the ADMIN sign-in door, end to end over Postgres ────────

const SIGN_IN_TEST_EC_PRIVATE_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgTCTNdGfegiVKVsm+
vZXPOa4xJAt5OT8zMSblfCEwtW2hRANCAARto0Dk75fxl2IyLx89vwvjUWkJAb/p
5bKnk8sNetDUBHLVIGpXoxBRFJVNSeDN6QB9IHl6rqDLaZR4iqLatScL
-----END PRIVATE KEY-----
";
const SIGN_IN_TEST_SUPABASE_URL: &str = "https://captain-walk-under-test.supabase.co";

async fn sign_in_jwks_endpoint() -> String {
    let body = json!({"keys":[{"kty":"EC","crv":"P-256","use":"sig","kid":"captain-walk-es256",
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

fn sign_in_subject_of(email: &str) -> String {
    format!("sub-{email}")
}

fn sign_in_token_of(email: &str) -> String {
    format!("token-for-{email}")
}

/// Mints the SAME shape a real `/auth/refresh` rotation would deliver for a stamped ADMIN subject
/// -- `app_metadata` read from the production stamper's OWN wire body (`stamp_admin_put_body`), so
/// the walk's cookie is the credential the door actually issues, never a hand-spelled lookalike.
fn sign_in_jwt_for_subject(sub: &str) -> String {
    let body = infrastructure::integrations::supabase_auth::stamp_admin_put_body();
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::ES256);
    header.kid = Some("captain-walk-es256".into());
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs()
        + 3600;
    let claims = json!({
        "sub": sub,
        "aud": "authenticated",
        "iss": format!("{SIGN_IN_TEST_SUPABASE_URL}/auth/v1"),
        "exp": exp,
        "app_metadata": body["app_metadata"],
    });
    let key = jsonwebtoken::EncodingKey::from_ec_pem(SIGN_IN_TEST_EC_PRIVATE_KEY_PEM.as_bytes())
        .expect("test EC key parses");
    jsonwebtoken::encode(&header, &claims, &key).expect("sign test JWT")
}

/// The ONE scripted port in this walk -- the identity PROVIDER (no real Supabase in a test run).
/// The mailbox, the `PlatformMember` bridge and the ADMIN seam stay real Postgres. `verify_email_token`
/// recovers the email from the token deterministically (the `admin_sign_in_door.rs` harness
/// convention); `refresh_session` mints a REAL, independently verifiable JWT for the subject it
/// just saw stamped -- never a placeholder string -- so the cookie this walk claims is the actual
/// credential, not a shortcut substituted at the assertion.
#[derive(Default)]
struct WalkScriptedIdentity {
    sent: std::sync::Mutex<Vec<String>>,
    admin_stamps: std::sync::Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl application::generated::services::IdentityService for WalkScriptedIdentity {
    async fn send_phone_otp(
        &self,
        _input: application::generated::services::IdentitySendPhoneOtpInput,
        _meta: &application::generated::services::ServiceCallMeta,
    ) -> Result<(), domain::shared::errors::DomainError> {
        panic!("the admin sign-in walk never sends SMS")
    }
    async fn verify_phone_otp(
        &self,
        _input: application::generated::services::IdentityVerifyPhoneOtpInput,
        _meta: &application::generated::services::ServiceCallMeta,
    ) -> Result<application::generated::services::IdentityVerifyPhoneOtpOutput, domain::shared::errors::DomainError> {
        panic!("the admin sign-in walk never verifies a phone OTP")
    }
    async fn stamp_customer_claim(
        &self,
        _input: application::generated::services::IdentityStampCustomerClaimInput,
        _meta: &application::generated::services::ServiceCallMeta,
    ) -> Result<(), domain::shared::errors::DomainError> {
        panic!("the admin sign-in walk never reaches the CUSTOMER stamper")
    }
    async fn stamp_rider_claim(
        &self,
        _input: application::generated::services::IdentityStampRiderClaimInput,
        _meta: &application::generated::services::ServiceCallMeta,
    ) -> Result<(), domain::shared::errors::DomainError> {
        panic!("the admin sign-in walk never reaches the RIDER stamper")
    }
    async fn stamp_member_claim(
        &self,
        _input: application::generated::services::IdentityStampMemberClaimInput,
        _meta: &application::generated::services::ServiceCallMeta,
    ) -> Result<(), domain::shared::errors::DomainError> {
        panic!("the admin sign-in walk never reaches the MEMBER stamper")
    }
    async fn send_email_magic_link(
        &self,
        _input: application::generated::services::IdentitySendEmailMagicLinkInput,
        _meta: &application::generated::services::ServiceCallMeta,
    ) -> Result<(), domain::shared::errors::DomainError> {
        panic!("the admin sign-in walk never calls the MEMBER/customer magic-link send -- round 2 R2-3 gave requestAdminSignInLink its own send_admin_sign_in_link call site")
    }
    // Round 2 R2-3 (obs/reviewer): `requestAdminSignInLink` now calls its OWN `send_admin_sign_in_link`
    // (the `EmailSendAuthorizer`'s `SignInDoor::Admin` arm), never the shared `send_email_magic_link`
    // the member/customer paths keep using -- so admin traffic can never land on member's counters.
    async fn send_admin_sign_in_link(
        &self,
        input: application::generated::services::IdentitySendAdminSignInLinkInput,
        _meta: &application::generated::services::ServiceCallMeta,
    ) -> Result<(), domain::shared::errors::DomainError> {
        self.sent.lock().expect("walk scripted identity").push(input.email.0);
        Ok(())
    }
    async fn verify_email_token(
        &self,
        input: application::generated::services::IdentityVerifyEmailTokenInput,
        _meta: &application::generated::services::ServiceCallMeta,
    ) -> Result<application::generated::services::IdentityVerifyEmailTokenOutput, domain::shared::errors::DomainError> {
        let email = input.token.0.trim_start_matches("token-for-").to_string();
        Ok(application::generated::services::IdentityVerifyEmailTokenOutput {
            auth_ref: domain::generated::scalars::AuthSubject(sign_in_subject_of(&email)),
            email: domain::generated::scalars::EmailAddress(email),
            access_token: Some("pre-stamp.access".into()),
            refresh_token: Some("pre-stamp.refresh".into()),
            expires_in: Some(3600),
        })
    }
    async fn stamp_admin_claim(
        &self,
        input: application::generated::services::IdentityStampAdminClaimInput,
        _meta: &application::generated::services::ServiceCallMeta,
    ) -> Result<(), domain::shared::errors::DomainError> {
        self.admin_stamps.lock().expect("walk scripted identity").push(input.auth_ref.0);
        Ok(())
    }
    async fn refresh_session(
        &self,
        _input: application::generated::services::IdentityRefreshSessionInput,
        _meta: &application::generated::services::ServiceCallMeta,
    ) -> Result<application::generated::services::IdentityRefreshSessionOutput, domain::shared::errors::DomainError> {
        let stamped = self.admin_stamps.lock().expect("walk scripted identity").last().cloned();
        let access_token = match stamped {
            Some(sub) => sign_in_jwt_for_subject(&sub),
            None => "rotated:none".into(),
        };
        Ok(application::generated::services::IdentityRefreshSessionOutput {
            access_token,
            refresh_token: Some("post-stamp.refresh".into()),
            expires_in: Some(3600),
        })
    }
}

/// Spawn the real mailbox workers for the ADMIN sign-in walk: the `spawn_mailbox_workers` precedent
/// above, narrowed to the THREE fields this walk overrides (`auth`, `sessions`, the door gate) --
/// everything else (the real `platform_members` bridge in particular) is identical.
fn spawn_mailbox_workers_for_admin_sign_in(
    pool: &PgPool,
    bus: actor_client::OperationStatusBus,
    identity: Arc<WalkScriptedIdentity>,
    sessions: Arc<application::auth_sessions::mem::MemAuthSessionStore>,
) {
    let deps = infrastructure::generated::command_router::CommandDeps {
        store: Arc::new(infrastructure::PgEventStore::new(pool.clone())),
        restaurants: Arc::new(infrastructure::PgRestaurantRepository::new(pool.clone())),
        slugs: Arc::new(infrastructure::PgSlugReservationRepository::new(pool.clone())),
        auth_subjects: Arc::new(infrastructure::PgAuthSubjectReservationRepository::new(pool.clone())),
        ownership: Arc::new(infrastructure::FailClosedGoogleOwnershipVerifier),
        probe: Arc::new(infrastructure::UnverifiedGbpOrderLinkProbe),
        prospection: Arc::new(infrastructure::PgProspectionRepository::new(pool.clone())),
        catalogs: Arc::new(infrastructure::PgCatalogRepository::new(pool.clone())),
        // The ONE scripted port -- everything else on this deps struct is real Postgres.
        auth: identity,
        customers: Arc::new(infrastructure::PgCustomerRepository::new(pool.clone())),
        sessions,
        payments: Arc::new(infrastructure::FailClosedPaymentGateway),
        pm_state: Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone())),
        refund_state: Arc::new(infrastructure::persistence::PgRefundProcessState::new(pool.clone())),
        mailbox_requeue: Arc::new(infrastructure::persistence::mailbox_lanes::PgMailboxRequeue::new(pool.clone())),
        riders: Arc::new(infrastructure::PgRiderRepository::new(pool.clone())),
        members: Arc::new(infrastructure::PgMemberRepository::new(pool.clone())),
        support_contact: Some(domain::generated::scalars::EmailAddress("support@captain.food".into())),
        run_rider_restriction_door: false,
        run_member_access_grant: false,
        run_member_sign_in_door: false,
        run_restaurant_invitation: false,
        // The bootstrap's OWN `GrantPlatformAccess` dispatch needs this door open too -- the
        // walk's mailbox worker is the SAME one that must deliver the sign-in commands, so both
        // doors are ON together (the `spawn_mailbox_workers` precedent above sets this true too).
        run_platform_access_grant: true,
        // THE DOOR UNDER TEST, ON.
        run_admin_sign_in_door: true,
        platform_members: Arc::new(infrastructure::PgPlatformMemberRepository::new(pool.clone())),
        enforce_service_hours_guard: false,
        enforce_acceptance_timeout: false,
        route_gates: application::generated::process_managers::RouteGates {
            order_placed_to_order: true,
            place_replacement_order_to_order: false,
            bind_cart_to_customer_to_cart: false,
            grant_customer_credit_to_customer_credit: false,
            mark_order_delivered_to_order: false,
        },
    };
    let handler = Arc::new(infrastructure::mailbox::MailboxCommandHandler::new(deps));
    let observer = Arc::new(infrastructure::mailbox::StatusBusObserver::new(bus));
    for (actor_type, width) in infrastructure::generated::command_router::ACTOR_MAILBOXES {
        let worker = actor_runtime::MailboxWorker::new(
            pool.clone(),
            "w-admin-sign-in-walk",
            *actor_type,
            actor_runtime::WorkerConfig { heartbeat_seconds: 1, ..Default::default() },
            handler.clone(),
        )
        .with_observer(observer.clone());
        let width = *width as i16;
        let (_tx, rx) = tokio::sync::watch::channel(false);
        std::mem::forget(_tx);
        tokio::spawn(async move {
            worker.seed(width).await.expect("seed");
            let _ = worker.run(rx).await;
        });
    }
}

/// The real `/public/graphql` + `/auth/session` + `/admin/graphql` router, PgMailbox-backed (the
/// `admin_sign_in_door.rs::door()` shape, transposed off the in-memory mailbox onto the pool the
/// real workers above are draining).
fn sign_in_walk_app(
    pool: &PgPool,
    mailbox: Arc<dyn actor_client::mailbox::Mailbox>,
    identity: Arc<WalkScriptedIdentity>,
    sessions: Arc<dyn application::auth_sessions::AuthSessionStore>,
    seam: Arc<dyn server::ResolvePlatformIdentity>,
    jwks_url: String,
) -> axum::Router {
    let identity_port: Arc<dyn application::generated::services::IdentityService> = identity;
    let mailbox_lanes: Arc<dyn actor_client::supervision::MailboxLaneRepository> =
        Arc::new(infrastructure::persistence::mailbox_lanes::PgMailboxLaneRepository::new(pool.clone()));
    // A REAL `ReadDeps` (the `rider_standing_walk.rs::schema_over` precedent) rather than `None`:
    // `mailboxLanes` must genuinely resolve and answer with data, not merely fail differently than
    // FORBIDDEN -- that is the bar this walk raises over `admin_sign_in_door.rs`'s narrower proof.
    let schema = server::graphql_schema::build_schema(
        Some(server::graphql_schema::ReadDeps {
            restaurants: Arc::new(infrastructure::PgRestaurantRepository::new(pool.clone())),
            prospection: Arc::new(infrastructure::PgProspectionRepository::new(pool.clone())),
            pricing_policy: Arc::new(infrastructure::PgPricingPolicyRepository::new(pool.clone())),
            uber_estimation_policy: Arc::new(infrastructure::PgUberEstimationPolicyRepository::new(pool.clone())),
            uber_split_policy: Arc::new(infrastructure::PgUberSplitPolicyRepository::new(pool.clone())),
            catalogs: Arc::new(infrastructure::PgCatalogRepository::new(pool.clone())),
            carts: Arc::new(infrastructure::PgCartRepository::new(pool.clone())),
            orders: Arc::new(infrastructure::PgOrderRepository::new(pool.clone())),
            order_conversations: Arc::new(infrastructure::PgOrderConversationRepository::new(pool.clone())),
            customers: Arc::new(infrastructure::PgCustomerRepository::new(pool.clone())),
            deliveries: Arc::new(infrastructure::PgDeliveryRepository::new(pool.clone())),
            rider_restrictions: Arc::new(infrastructure::persistence::rider_restriction_store::PgRiderRestrictionRepository::new(pool.clone())),
            rider_roster: Arc::new(infrastructure::persistence::rider_roster_store::PgRiderRosterRepository::new(pool.clone())),
            member_authority: Arc::new(infrastructure::PgMemberAuthorityRepository::new(pool.clone())),
            restaurant_roster: Arc::new(infrastructure::PgRestaurantRosterRepository::new(pool.clone())),
            restaurant_invitations: Arc::new(infrastructure::PgRestaurantInvitationListRepository::new(pool.clone())),
            refunds: Arc::new(infrastructure::PgRefundQueueRepository::new(pool.clone())),
            delivery_satisfaction: Arc::new(infrastructure::PgDeliverySatisfactionRepository::new(pool.clone())),
            delivery_partner_availabilities: Arc::new(infrastructure::PgDeliveryPartnerAvailabilityRepository::new(pool.clone())),
            reclamations: Arc::new(infrastructure::PgReclamationRepository::new(pool.clone())),
            customer_credit: Arc::new(infrastructure::PgCustomerCreditRepository::new(pool.clone())),
            mailbox_lanes: mailbox_lanes.clone(),
            service_window_horizon: Default::default(),
            support_contact: Some(domain::generated::scalars::EmailAddress("support@captain.food".into())),
            run_rider_restriction_door: server::graphql_schema::RunRiderRestrictionDoor(false),
        }),
        Some(server::graphql_schema::WriteDeps {
            event_store: Arc::new(infrastructure::PgEventStore::new(pool.clone())),
            ownership: Arc::new(infrastructure::FailClosedGoogleOwnershipVerifier),
            gbp_probe: Arc::new(infrastructure::UnverifiedGbpOrderLinkProbe),
            auth_provider: identity_port.clone(),
            payments: Arc::new(infrastructure::FailClosedPaymentGateway),
            pm_state: Arc::new(application::generated::pm_state::mem::MemPaymentProcessState::default()),
            refund_state: Arc::new(application::generated::pm_state::mem::MemRefundProcessState::default()),
            mailbox: mailbox.clone(),
            status_bus: actor_client::OperationStatusBus::default(),
            auth_sessions: sessions.clone(),
            slug_reservations: Arc::new(infrastructure::PgSlugReservationRepository::new(pool.clone())),
        }),
        None,
    );
    server::graphql_routes(
        schema,
        server::TenantLookup(None),
        server::IdentitySources {
            customer: server::CustomerIdentitySource::Claim,
            rider: server::RiderIdentitySource::new(Arc::new(server::NoDatabaseRiderIdentity)),
            member: server::MemberIdentitySource::new(Arc::new(server::NoDatabaseMemberIdentity)),
            platform: server::PlatformIdentitySource::new(seam),
        },
    )
    .layer(axum::Extension(server::AuthContext::from_config(jwks_url, SIGN_IN_TEST_SUPABASE_URL.into())))
}

async fn sign_in_walk_post_public(app: &axum::Router, query: &str, session: uuid::Uuid) -> serde_json::Value {
    use tower::ServiceExt;
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/public/graphql")
        .header(axum::http::header::HOST, "system.captain.food")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header("X-SESSION-ID", session.to_string())
        .body(axum::body::Body::from(json!({ "query": query }).to_string()))
        .expect("request builds");
    let response = app.clone().oneshot(request).await.expect("router answers");
    assert_eq!(response.status(), axum::http::StatusCode::OK, "the public path never 401s");
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body");
    serde_json::from_slice(&bytes).expect("a GraphQL response body")
}

/// Poll a `Mutex<Vec<_>>`-backed fake until it has recorded `at_least` entries -- the real mailbox
/// worker delivers on its own schedule, and the sign-in lane emits no `domain_events` fact this
/// walk's `wait_for_events` could poll instead (ADR-20260906-023825: the actor is pure routing).
async fn wait_for_len<T>(mutex: &std::sync::Mutex<Vec<T>>, at_least: usize) {
    for _ in 0..100 {
        if mutex.lock().expect("poll mutex").len() >= at_least {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("mutex did not reach {at_least} entries in time");
}

/// The `AuthSessionStore` sibling of [`wait_for_len`] -- `MemAuthSessionStore::parked()` returns a
/// snapshot `Vec`, so polling it needs its own loop rather than a borrow `wait_for_len` could share.
async fn wait_for_parked(sessions: &application::auth_sessions::mem::MemAuthSessionStore, at_least: usize) {
    for _ in 0..100 {
        if sessions.parked().len() >= at_least {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("sessions store did not reach {at_least} parked entries in time");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requesting_and_confirming_an_admin_sign_in_opens_the_admin_door_end_to_end() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let Some(url) = db_test_gate::database_url("admin_sign_in_walk") else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    apply_all_migrations(&pool).await;

    let email = "walk-admin@captain.food";
    let auth_subject = sign_in_subject_of(email);

    // Bootstrap: grant platform access to the SAME subject the scripted identity provider will
    // resolve the email to on `verify_email_token` -- otherwise this walk would prove nothing about
    // a GRANTED admin at all.
    let code = server::bootstrap_platform_admin::dispatch(&url, &auth_subject).await;
    assert_eq!(code, 0, "the bootstrap dispatch must succeed");

    let identity = Arc::new(WalkScriptedIdentity::default());
    let sessions = Arc::new(application::auth_sessions::mem::MemAuthSessionStore::default());
    let status_bus = actor_client::OperationStatusBus::default();
    spawn_mailbox_workers_for_admin_sign_in(&pool, status_bus, identity.clone(), sessions.clone());

    // Drain the bootstrap's own grant so the seam resolves BEFORE the sign-in leg is exercised --
    // this walk's claim is about the sign-in door, not a re-proof of `the_admin_seam_resolves_
    // only_after_the_grant_lands` above.
    let platform_membership_id = server::bootstrap_platform_admin::platform_membership_id_for(&auth_subject);
    let stream = format!("PlatformMembership-{}", platform_membership_id.0);
    wait_for_events(&pool, &stream, 1).await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (grant)");

    let mailbox: Arc<dyn actor_client::mailbox::Mailbox> = Arc::new(PgMailbox::new(pool.clone()));
    let seam: Arc<dyn server::ResolvePlatformIdentity> = Arc::new(server::PgPlatformIdentity::new(Arc::new(
        infrastructure::PgPlatformMemberRepository::new(pool.clone()),
    )));
    let jwks_url = sign_in_jwks_endpoint().await;
    let app = sign_in_walk_app(&pool, mailbox, identity.clone(), sessions.clone(), seam, jwks_url);

    let session = uuid::Uuid::now_v7();
    let req_id = uuid::Uuid::now_v7();
    let request_query = format!(
        r#"mutation {{ requestAdminSignInLink(input: {{ email: "{email}" }}, metadata: {{ messageId: "{req_id}" }}) {{ operationStatus }} }}"#
    );
    let acceptance = sign_in_walk_post_public(&app, &request_query, session).await;
    assert!(acceptance["errors"].is_null(), "requestAdminSignInLink must be accepted: {}", acceptance["errors"]);
    assert_eq!(acceptance["data"]["requestAdminSignInLink"]["operationStatus"], "PENDING");

    // RED-BEFORE-GREEN, quoted in the hand-back: before the real worker has drained the row, the
    // scripted provider has seen ZERO sends -- this is the seam the door-closed test at
    // `admin_sign_in_door.rs` proves synchronously; here it is the SAME fact proven against a real
    // asynchronous worker, so it must be awaited rather than asserted immediately.
    wait_for_len(&identity.sent, 1).await;
    assert_eq!(*identity.sent.lock().unwrap(), vec![email.to_string()], "the real worker delivered the request leg");

    let confirm_id = uuid::Uuid::now_v7();
    let confirm_query = format!(
        r#"mutation {{ confirmAdminSignIn(input: {{ token: "{}" }}, metadata: {{ messageId: "{confirm_id}" }}) {{ operationStatus }} }}"#,
        sign_in_token_of(email)
    );
    let acceptance = sign_in_walk_post_public(&app, &confirm_query, session).await;
    assert!(acceptance["errors"].is_null(), "confirmAdminSignIn must be accepted: {}", acceptance["errors"]);
    assert_eq!(acceptance["data"]["confirmAdminSignIn"]["operationStatus"], "PENDING");

    wait_for_len(&identity.admin_stamps, 1).await;
    assert_eq!(
        *identity.admin_stamps.lock().unwrap(),
        vec![auth_subject.clone()],
        "the real worker stamped the granted admin's own subject, and nothing else"
    );
    wait_for_parked(&sessions, 1).await;
    let parked = sessions.parked();
    assert_eq!(parked.len(), 1, "exactly one session parked for POST /auth/session");
    assert_eq!(parked[0].message_id, confirm_id);
    assert_eq!(parked[0].session_id, Some(session));

    // `POST /auth/session`: claim the parked cookie the SAME way a browser would.
    let claim_request = axum::http::Request::builder()
        .method("POST")
        .uri("/auth/session")
        .header(axum::http::header::HOST, "system.captain.food")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header("X-SESSION-ID", session.to_string())
        .body(axum::body::Body::from(json!({ "messageId": confirm_id }).to_string()))
        .expect("request builds");
    // `/auth/session` is not mounted on the GraphQL-only `app` above (`admin_sign_in_door.rs`
    // never needed it); this walk needs it live, so it is merged on here, once, right before use.
    let full_app = app.merge(server::auth_routes(server::AuthRoutesState {
        sessions: Some(sessions.clone()),
        identity: identity.clone(),
        sms: None,
        sms_hook_secret: None,
        sms_guard: None,
    }));
    use tower::ServiceExt;
    let claim_response = full_app.clone().oneshot(claim_request).await.expect("router answers");
    assert_eq!(claim_response.status(), axum::http::StatusCode::NO_CONTENT, "the claim must succeed -- same messageId, same X-SESSION-ID");
    let set_cookie = claim_response
        .headers()
        .get_all(axum::http::header::SET_COOKIE)
        .iter()
        .find_map(|v| v.to_str().ok().filter(|s| s.starts_with("captain_auth=")))
        .expect("the access cookie is set")
        .to_string();
    let jwt = set_cookie
        .strip_prefix("captain_auth=")
        .and_then(|rest| rest.split(';').next())
        .expect("cookie value parses")
        .to_string();

    // `/admin/graphql mailboxLanes`, riding the cookie `POST /auth/session` just set -- the REAL
    // credential the door issued, claimed the way a browser would, opens the ADMIN board.
    let admin_request = axum::http::Request::builder()
        .method("POST")
        .uri("/admin/graphql")
        .header(axum::http::header::HOST, "system.captain.food")
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .header(axum::http::header::COOKIE, format!("captain_auth={jwt}"))
        .body(axum::body::Body::from(json!({ "query": "query { mailboxLanes { actorType partition } }" }).to_string()))
        .expect("request builds");
    let admin_response = full_app.clone().oneshot(admin_request).await.expect("router answers");
    assert_eq!(admin_response.status(), axum::http::StatusCode::OK);
    let bytes = axum::body::to_bytes(admin_response.into_body(), usize::MAX).await.expect("body");
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("a GraphQL response body");
    assert!(
        body["errors"].is_null(),
        "a granted admin's real, issued cookie must open /admin/graphql -- got errors: {}",
        body["errors"]
    );
    assert!(body["data"]["mailboxLanes"].is_array(), "mailboxLanes must be admitted and answer with data: {body}");
}
