//! COMBINED actor + projector integration test (PR #270 review follow-up): live mailbox workers
//! deliver inbound facts through the REAL fenced write path WHILE the batched projection worker
//! (#267) drains `domain_events` concurrently — the two halves the other suites prove separately
//! (`actor_runtime/tests/concurrency.rs`: actors alone; `projection_batching.rs`: projector alone
//! over a pre-seeded log) working against each other on one real Postgres.
//!
//! Scenario: N restaurants × K registry revisions arrive as kind-EVENT `RestaurantRegistered`
//! facts (the SIRENE shape — revision k renames the restaurant, so the delivery route appends
//! `RestaurantRegistered` then K-1 `RestaurantUpdated`s per stream). TWO live `MailboxWorker`s
//! race over the Restaurant lanes while the projector folds in `batch_size`-bounded transactions
//! that land MID-STREAM on purpose (batch_size deliberately not a divisor of the event count).
//!
//! Proven, end to end:
//! 1. **Exactly-once through the fence under two live workers**: every stream holds exactly K
//!    events with K distinct versions — no duplicate append ever committed.
//! 2. **Per-key order survives actors + batching**: each restaurant's projected `display_name`
//!    is its LAST revision, even though its events were delivered by racing workers and folded
//!    across several projection batches.
//! 3. **The ledger converges**: every mailbox row lands SUCCEEDED; the projection checkpoint
//!    reaches the log head; a further projector pass changes nothing (idempotent resume).
//! 4. **Graceful drain**: flipping the shutdown channel releases every lane (`claimed_by NULL`).
//!
//! Needs `DATABASE_URL` (docker `postgres:16-alpine` or any real Postgres); skips otherwise
//! (DB_TESTS_REQUIRED makes the skip loud, #230).

use std::sync::Arc;

use actor_runtime::{MailboxWorker, WorkerConfig};
use application::generated::services::{IdentityService, PaymentService};
use actor_client::mailbox::Mailbox;
use domain::generated::events::RestaurantRegistered;
use domain::generated::scalars::{
    AddressLine, CityName, CountryCode, PostalCode, RestaurantDisplayName, RestaurantId,
    RestaurantListingStatus,
};
use actor_client::generated::actor_clients::RestaurantClient;
use infrastructure::generated::command_router::CommandDeps;
use actor_client::EnqueueOutcome;
use infrastructure::mailbox::MailboxCommandHandler;
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, FailClosedPaymentGateway,
    PgCatalogRepository, PgCustomerRepository, PgEventStore, PgProspectionRepository,
    PgRestaurantRepository, PgSlugReservationRepository, ProjectionWorker,
    UnverifiedGbpOrderLinkProbe,
};
use sqlx::{PgPool, Row};

const N_RESTAURANTS: usize = 12;
const K_REVISIONS: usize = 5;

async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS inbound_messages, mailbox_partitions, domain_events, restaurant,
          prospectionpipeline, projection_checkpoint CASCADE;
        DROP SEQUENCE IF EXISTS inbound_messages_position_seq;

        CREATE TABLE domain_events (
          position BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
          id UUID NOT NULL UNIQUE,
          stream_name TEXT NOT NULL,
          version INTEGER NOT NULL,
          user_id UUID NOT NULL,
          user_type TEXT NOT NULL,
          correlation_id UUID NOT NULL,
          cause_id UUID NULL,
          event_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          metadata JSONB NULL,
          occurred_at TIMESTAMPTZ NOT NULL,
          expired_at TIMESTAMPTZ NULL,
          UNIQUE (stream_name, version)
        );
        CREATE TABLE restaurant (
          restaurant_id UUID PRIMARY KEY,
          restaurant_account_id UUID,
          listing_status TEXT NOT NULL,
          external_identifiers JSONB,
          google_place_id TEXT,
          slug TEXT UNIQUE,
          display_name TEXT NOT NULL,
          description TEXT,
          tags JSONB,
          margin_rate TEXT,
          cuisine_category TEXT,
          uber_prices_opt_in BOOLEAN,
          website TEXT,
          rating TEXT,
          reviews_count INTEGER,
          gbp_order_url TEXT,
          gbp_link_status TEXT,
          address JSONB NOT NULL,
          location JSONB,
          opening_hours JSONB NOT NULL,
          status TEXT NOT NULL,
          order_acceptance TEXT NOT NULL,
          default_currency TEXT NOT NULL,
          timezone TEXT,
          preparation_time_minutes INTEGER,
          created_at TIMESTAMPTZ NOT NULL,
          updated_at TIMESTAMPTZ NOT NULL
        );
        CREATE TABLE prospectionpipeline (
          restaurant_id UUID PRIMARY KEY,
          score INTEGER NOT NULL,
          pipeline_status TEXT NOT NULL,
          contacts_count INTEGER NOT NULL,
          last_contacted_at TIMESTAMPTZ,
          replied_at TIMESTAMPTZ,
          created_at TIMESTAMPTZ NOT NULL,
          updated_at TIMESTAMPTZ NOT NULL
        );
        CREATE TABLE projection_checkpoint (
          projector  TEXT        PRIMARY KEY,
          position   BIGINT      NOT NULL DEFAULT 0,
          updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );
        "#,
    )
    .execute(pool)
    .await
    .expect("reset schema");
    sqlx::raw_sql(include_str!("../../../migrations/20260731063000_actor_mailbox_tables.sql"))
        .execute(pool)
        .await
        .expect("apply the actor-mailbox migration");
}

fn deps_over(pool: &PgPool) -> CommandDeps {
    CommandDeps {
        store: Arc::new(PgEventStore::new(pool.clone())),
        restaurants: Arc::new(PgRestaurantRepository::new(pool.clone())),
        slugs: Arc::new(PgSlugReservationRepository::new(pool.clone())),
        ownership: Arc::new(FailClosedGoogleOwnershipVerifier),
        probe: Arc::new(UnverifiedGbpOrderLinkProbe),
        prospection: Arc::new(PgProspectionRepository::new(pool.clone())),
        catalogs: Arc::new(PgCatalogRepository::new(pool.clone())),
        auth: Arc::new(FailClosedIdentityService) as Arc<dyn IdentityService>,
        customers: Arc::new(PgCustomerRepository::new(pool.clone())),
        sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
        payments: Arc::new(FailClosedPaymentGateway) as Arc<dyn PaymentService>,
        pm_state: Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone())),
        refund_state: Arc::new(infrastructure::persistence::PgRefundProcessState::new(pool.clone())),
    }
}

fn restaurant_uuid(i: usize) -> uuid::Uuid {
    uuid::Uuid::from_u128(0xFEED_0000 + i as u128)
}

fn revision_fact(i: usize, k: usize) -> RestaurantRegistered {
    use domain::generated::entities::Address;
    RestaurantRegistered {
        mode: None,
        restaurant_id: RestaurantId(restaurant_uuid(i)),
        account_id: None,
        listing_status: RestaurantListingStatus::PASSIVE_PARTNER,
        r#ref: None,
        external_identifiers: vec![],
        display_name: RestaurantDisplayName(format!("R{i} v{k}")),
        contact: None,
        website: None,
        tags: vec![],
        margin_rate: None,
        cuisine_category: None,
        uber_prices_opt_in: None,
        address: Address {
            line1: AddressLine("1 rue Nationale".into()),
            line2: None,
            postal_code: PostalCode("37000".into()),
            city: CityName("Tours".into()),
            country: CountryCode("FR".into()),
        },
        location: None,
        timezone: None,
        preparation_time_minutes: None,
        opening_hours: vec![],
    }
}

#[tokio::test]
async fn actors_and_batching_projector_converge_concurrently() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP actors_projector_batching: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect");
    reset_schema(&pool).await;

    // The typed clients are the only public door to the mailbox (#284 slice 3) — this suite
    // records through them, exactly as the SIRENE producer does.
    let mailbox: Arc<dyn Mailbox> =
        Arc::new(infrastructure::persistence::mailbox_store::PgMailbox::new(pool.clone()));

    // Enqueue N×K revision facts BEFORE and DURING the workers' lifetime. Per restaurant the
    // revisions are enqueued sequentially (per-lane position order = revision order); across
    // restaurants they interleave.
    for k in 0..K_REVISIONS / 2 {
        for i in 0..N_RESTAURANTS {
            let outcome = RestaurantClient::new(mailbox.clone(), restaurant_uuid(i))
                .record(
                    revision_fact(i, k),
                    "sirene",
                    &format!("r{i}-rev{k}"),
                    uuid::Uuid::new_v4(),
                )
                .await
                .expect("enqueue");
            assert_eq!(outcome, EnqueueOutcome::Enqueued);
        }
    }

    // TWO live workers over the same actor type — the claim split leaves lanes for the peer, so
    // both genuinely deliver. Real handler, real router, real fenced completion.
    let handler = Arc::new(MailboxCommandHandler::new(deps_over(&pool)));
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let mut worker_tasks = Vec::new();
    for name in ["w-A", "w-B"] {
        let worker = Arc::new(MailboxWorker::new(
            pool.clone(),
            name,
            "Restaurant",
            WorkerConfig {
                lease_seconds: 30,
                heartbeat_seconds: 1,
                batch: 10,
                max_claims_per_pass: 60,
            },
            handler.clone(),
        ));
        worker.seed(5).await.expect("seed");
        let rx = shutdown_rx.clone();
        let w = worker.clone();
        worker_tasks.push((worker, tokio::spawn(async move { w.run(rx).await })));
    }

    // The batching projector runs CONCURRENTLY with the deliveries. batch_size 7 deliberately
    // does not divide the event count, so batch boundaries land mid-restaurant.
    let projector = ProjectionWorker::new(pool.clone()).with_batch_size(7);
    let (proj_stop_tx, proj_stop_rx) = tokio::sync::watch::channel(false);
    let proj_pool = pool.clone();
    let proj_task = tokio::spawn(async move {
        let worker = projector;
        while !*proj_stop_rx.borrow() {
            let _ = worker.run_once().await;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let _ = proj_pool; // keep the pool alive for the loop's lifetime
        worker
    });

    // The remaining revisions arrive WHILE workers drain and the projector folds.
    for k in K_REVISIONS / 2..K_REVISIONS {
        for i in 0..N_RESTAURANTS {
            RestaurantClient::new(mailbox.clone(), restaurant_uuid(i))
                .record(
                    revision_fact(i, k),
                    "sirene",
                    &format!("r{i}-rev{k}"),
                    uuid::Uuid::new_v4(),
                )
                .await
                .expect("enqueue");
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    // Convergence: all mailbox rows terminal, all restaurants projected, checkpoint at the head.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let pending: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM inbound_messages WHERE status IN ('RECEIVED', 'SCHEDULED')",
        )
        .fetch_one(&pool)
        .await
        .expect("pending");
        let projected: i64 =
            sqlx::query_scalar("SELECT count(*) FROM restaurant").fetch_one(&pool).await.expect("count");
        let head: i64 =
            sqlx::query_scalar("SELECT COALESCE(max(position), 0) FROM domain_events")
                .fetch_one(&pool)
                .await
                .expect("head");
        let ckpt: i64 = sqlx::query_scalar(
            "SELECT COALESCE(max(position), 0) FROM projection_checkpoint",
        )
        .fetch_one(&pool)
        .await
        .expect("ckpt");
        if pending == 0 && projected == N_RESTAURANTS as i64 && head > 0 && ckpt >= head {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no convergence: pending={pending} projected={projected} ckpt={ckpt}/{head}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    // 1. Exactly-once through the fence: K events with K distinct versions per stream.
    let streams = sqlx::query(
        "SELECT stream_name, count(*) AS n, count(DISTINCT version) AS v FROM domain_events \
         GROUP BY stream_name",
    )
    .fetch_all(&pool)
    .await
    .expect("streams");
    assert_eq!(streams.len(), N_RESTAURANTS);
    for row in &streams {
        let n: i64 = row.get("n");
        let v: i64 = row.get("v");
        assert_eq!(n, K_REVISIONS as i64, "stream {}: duplicate or lost append", row.get::<String, _>("stream_name"));
        assert_eq!(v, K_REVISIONS as i64);
    }

    // 2. Per-key order survived actors + batching: the LAST revision's name is projected.
    for i in 0..N_RESTAURANTS {
        let name: String =
            sqlx::query_scalar("SELECT display_name FROM restaurant WHERE restaurant_id = $1")
                .bind(restaurant_uuid(i))
                .fetch_one(&pool)
                .await
                .expect("projected row");
        assert_eq!(name, format!("R{i} v{}", K_REVISIONS - 1));
    }

    // 3. Every mailbox row committed SUCCEEDED (Recorded / Updated — never Failed, never stuck).
    let statuses: Vec<(String, i64)> = sqlx::query(
        "SELECT status, count(*) AS n FROM inbound_messages GROUP BY status",
    )
    .fetch_all(&pool)
    .await
    .expect("statuses")
    .iter()
    .map(|r| (r.get("status"), r.get("n")))
    .collect();
    assert_eq!(
        statuses,
        vec![("SUCCEEDED".to_string(), (N_RESTAURANTS * K_REVISIONS) as i64)],
        "every delivery must commit SUCCEEDED"
    );

    // Idempotent resume: one more projector pass changes nothing.
    let _ = proj_stop_tx.send(true);
    let projector = proj_task.await.expect("projector task");
    let before: Vec<(uuid::Uuid, String)> =
        sqlx::query("SELECT restaurant_id, display_name FROM restaurant ORDER BY restaurant_id")
            .fetch_all(&pool)
            .await
            .expect("before")
            .iter()
            .map(|r| (r.get("restaurant_id"), r.get("display_name")))
            .collect();
    projector.run_once().await.expect("idempotent pass");
    let after: Vec<(uuid::Uuid, String)> =
        sqlx::query("SELECT restaurant_id, display_name FROM restaurant ORDER BY restaurant_id")
            .fetch_all(&pool)
            .await
            .expect("after")
            .iter()
            .map(|r| (r.get("restaurant_id"), r.get("display_name")))
            .collect();
    assert_eq!(before, after, "a re-run over the drained log must change nothing");

    // 4. Graceful drain: shutdown releases every claimed lane.
    let _ = shutdown_tx.send(true);
    for (_, task) in worker_tasks {
        task.await.expect("worker task").expect("clean shutdown");
    }
    let still_claimed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mailbox_partitions WHERE claimed_by IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("claims");
    assert_eq!(still_claimed, 0, "graceful shutdown must release every lane");
}
