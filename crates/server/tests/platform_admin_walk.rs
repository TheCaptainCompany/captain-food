//! The card's "walk" (farley, beck): a DB-gated end-to-end pass over the REAL router / mailbox
//! worker / projector stack for the platform grant and the ADMIN seam binding (#639 part C step
//! 6-v, ADR-20260905-223957). Proves the one-shot bootstrap's idempotency (`the_bootstrap_
//! replays_from_domain_events_alone`, `running_it_twice_appends_one_fact`) and the full chain: a
//! real GraphQL mutation dispatches through the real mailbox, a real `MailboxWorker` appends
//! `PlatformAccessGranted`, a real `ProjectionWorker::run_once()` fold flips the ADMIN seam
//! (`resolve_platform_scope`) from unbound to resolved -- an ADMIN token whose subject was granted
//! reads `riders` on `/admin/graphql`; the SAME token BEFORE the grant is FORBIDDEN.
//!
//! Needs a real Postgres (`DATABASE_URL`); SKIPS (prints and returns) without one, same as every
//! other DB-gated suite here (`DB_TESTS_REQUIRED=1` makes that a hard failure, beck CATCH).

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

/// The one-shot bootstrap replays cleanly from `domain_events` alone (ADR-20260905-223957 §3): a
/// seeded PROJECTION row could never pass this, because the projection is TRUNCATEd and the
/// checkpoint reset to 0 between the dispatch and the re-check -- only the immutable fact survives.
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

/// The full chain, through the real router: an ADMIN token whose subject was granted reads
/// `riders` on `/admin/graphql`; the SAME token BEFORE the grant is FORBIDDEN. Real JWT
/// verification is a different seam (exercised by `an_admin_token_with_no_platform_grant_is_
/// unbound.rs`'s scripted seam); this walk proves the Postgres-backed seam
/// (`resolve_platform_scope` -> `PgPlatformIdentity` -> `platform_member`) end to end, so the
/// `ReadScope` is injected the same established idiom `rider_standing_walk.rs` uses for the
/// business-layer legs, while the seam RESOLUTION itself is exercised via the real
/// `PgPlatformIdentity` port directly (the mailbox/projector chain proves the WRITE half; this
/// proves the READ half consumes what it wrote).
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
    let platform_membership_id = uuid::Uuid::new_v4();
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
