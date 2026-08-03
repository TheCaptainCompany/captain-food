//! END-TO-END activations (#272 D3, PROP-20260728-152752 §3.5, gated `ACTOR_ACTIVATIONS`): the
//! mailbox delivery path folds the delivered actor's own stream through the held-state cache.
//! Proves, against a real Postgres:
//!
//! - **fold-on-first-message**: one rehydration `load` fills the activation; every subsequent
//!   delivery to the same actor folds from the held state (the inner store is never asked again);
//! - **apply-after-commit is correct**: the promoted state carries the committed appends — the
//!   next delivery's flush lands at the right expected version (a stale promotion would abort on
//!   `UNIQUE(stream_name, version)`);
//! - **the never-wrong eviction signal**: an out-of-band writer (the legacy path's shape) moves
//!   the stream under a warm activation → the flush loses the race, the delivery aborts (row
//!   stays RECEIVED), the held state is invalidated, and the RETRY refolds from the log and
//!   commits — correctness never depended on the cache;
//! - **lane loss drops the lane's holdings**: a stolen lane's next heartbeat evicts every held
//!   state under it (the `LaneEvents` seam);
//! - **per-actor opt-out**: `(enabled = false)` for the actor type caches nothing.
//!
//! Needs `DATABASE_URL`; skips otherwise (DB_TESTS_REQUIRED makes the skip loud, #230).

use std::sync::Arc;
use std::time::Duration;

use actor_runtime::{MailboxWorker, WorkerConfig};
use application::generated::services::{IdentityService, PaymentService};
use application::ports::{Actor, EventStore};
use async_trait::async_trait;
use domain::generated::events::DomainEvent;
use domain::shared::errors::DomainError;

use infrastructure::generated::command_router::CommandDeps;
use infrastructure::mailbox::{
    ActivationLaneEvents, ActivationSettings, MailboxCommandHandler, StreamActivations,
};
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, FailClosedPaymentGateway,
    PgCatalogRepository, PgCustomerRepository, PgEventStore, PgProspectionRepository,
    PgRestaurantRepository, PgSlugReservationRepository, UnverifiedGbpOrderLinkProbe,
};
use sqlx::PgPool;

/// Both tests DROP+CREATE the same tables in one shared database — the harness's concurrent
/// threads would clobber each other's schema mid-test (the `staging_upsert` lesson). One
/// suite-wide lock, held for each test's whole body, restores determinism.
static SUITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serialize_suite() -> std::sync::MutexGuard<'static, ()> {
    SUITE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// An [`EventStore`] that counts the loads reaching the REAL store per stream — a load served
/// from the activation cache never shows up here.
struct CountingStore {
    inner: Arc<dyn EventStore>,
    loads: std::sync::Mutex<std::collections::HashMap<String, usize>>,
}

impl CountingStore {
    fn new(inner: Arc<dyn EventStore>) -> Self {
        Self { inner, loads: Default::default() }
    }
    fn loads_of(&self, stream: &str) -> usize {
        self.loads.lock().unwrap().get(stream).copied().unwrap_or(0)
    }
}

#[async_trait]
impl EventStore for CountingStore {
    async fn append(
        &self,
        stream_name: &str,
        expected_version: i64,
        events: &[DomainEvent],
        actor: &Actor,
    ) -> Result<i64, DomainError> {
        self.inner.append(stream_name, expected_version, events, actor).await
    }
    async fn load(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError> {
        *self.loads.lock().unwrap().entry(stream_name.to_owned()).or_default() += 1;
        self.inner.load(stream_name).await
    }
}

async fn setup(pool: &PgPool) {
    sqlx::raw_sql(
        "DROP TABLE IF EXISTS inbound_messages, mailbox_partitions, domain_events, customer CASCADE;\n\
         DROP SEQUENCE IF EXISTS inbound_messages_position_seq;",
    )
    .execute(pool)
    .await
    .expect("drop");
    sqlx::raw_sql(include_str!("../../../migrations/20260731063000_actor_mailbox_tables.sql"))
        .execute(pool)
        .await
        .expect("apply the actor-mailbox migration");
    sqlx::raw_sql(include_str!("../../../migrations/20260802230000_mailbox_attempts_column.sql"))
        .execute(pool)
        .await
        .expect("apply the mailbox attempts migration");
    sqlx::raw_sql(include_str!("../../../migrations/20260803004500_mailbox_backoff_next_attempt.sql"))
        .execute(pool)
        .await
        .expect("apply the mailbox backoff migration");
    sqlx::raw_sql(
        "CREATE TABLE domain_events (\n\
           position BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,\n\
           id UUID NOT NULL UNIQUE,\n\
           stream_name TEXT NOT NULL,\n\
           version INTEGER NOT NULL,\n\
           user_id UUID NOT NULL,\n\
           user_type TEXT NOT NULL,\n\
           correlation_id UUID NOT NULL,\n\
           cause_id UUID NULL,\n\
           event_type TEXT NOT NULL,\n\
           payload JSONB NOT NULL,\n\
           metadata JSONB NULL,\n\
           occurred_at TIMESTAMPTZ NOT NULL,\n\
           expired_at TIMESTAMPTZ NULL,\n\
           UNIQUE (stream_name, version)\n\
         )",
    )
    .execute(pool)
    .await
    .expect("domain_events");
    sqlx::raw_sql(
        "CREATE TABLE customer (\n\
           customer_id UUID PRIMARY KEY,\n\
           phone TEXT NOT NULL UNIQUE,\n\
           auth_ref TEXT,\n\
           display_name TEXT,\n\
           email TEXT,\n\
           email_verified BOOLEAN NOT NULL,\n\
           locale TEXT,\n\
           timezone TEXT,\n\
           ratings JSONB NOT NULL,\n\
           favorite_restaurant_ids JSONB NOT NULL,\n\
           preferences JSONB,\n\
           addresses JSONB NOT NULL,\n\
           payment_method_id TEXT,\n\
           created_at TIMESTAMPTZ NOT NULL,\n\
           updated_at TIMESTAMPTZ NOT NULL\n\
         )",
    )
    .execute(pool)
    .await
    .expect("customer");
}

fn deps_over(store: Arc<dyn EventStore>, pool: &PgPool) -> CommandDeps {
    CommandDeps {
        store,
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
        mailbox_requeue: Arc::new(infrastructure::persistence::mailbox_lanes::PgMailboxRequeue::new(pool.clone())),
    }
}

fn settings(per_actor_enabled: bool) -> Arc<ActivationSettings> {
    Arc::new(ActivationSettings {
        cache: Arc::new(StreamActivations::new(64 * 1024 * 1024)),
        idle_default: Duration::from_secs(300),
        per_actor: [("Conversation", (per_actor_enabled, None))].into_iter().collect(),
    })
}

async fn enqueue(
    pool: &PgPool,
    partition: i16,
    n: u128,
    actor_id: uuid::Uuid,
    message_type: &str,
    payload: serde_json::Value,
) -> uuid::Uuid {
    let id = uuid::Uuid::from_u128(n);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, correlation_id) \
         VALUES ($1, 'COMMAND', 'Conversation', $2, $3, $4, $5, $6, 'GRAPHQL', 'ADMIN', $1)",
    )
    .bind(id)
    .bind(actor_id)
    .bind(partition)
    .bind(message_type)
    .bind(&payload)
    .bind(format!("h{n}"))
    .execute(pool)
    .await
    .expect("enqueue");
    id
}

fn open_payload(order: uuid::Uuid, restaurant: uuid::Uuid) -> serde_json::Value {
    serde_json::json!({
        "orderId": order,
        "restaurantId": restaurant,
        "customerId": null,
        "customerChatEnabled": true,
    })
}

fn post_payload(order: uuid::Uuid, message_n: u128, body: &str) -> serde_json::Value {
    serde_json::json!({
        "orderId": order,
        "messageId": uuid::Uuid::from_u128(message_n),
        "authorRole": "ADMIN",
        "visibility": "INTERNAL",
        "body": body,
        "originalLocale": "en-US",
    })
}

fn gated() -> Option<String> {
    match std::env::var("DATABASE_URL") {
        Ok(url) => Some(url),
        Err(_) => {
            assert!(
                std::env::var("DB_TESTS_REQUIRED").is_err(),
                "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
            );
            eprintln!("SKIP mailbox_activations: DATABASE_URL not set");
            None
        }
    }
}

#[tokio::test]
async fn activation_folds_once_promotes_after_commit_and_survives_a_foreign_writer() {
    let Some(url) = gated() else { return };
    let _suite = serialize_suite();
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let order = uuid::Uuid::from_u128(0xAC71);
    let restaurant = uuid::Uuid::from_u128(0x0E57);
    let stream = format!("Conversation-{order}");
    let partition = actor_client::stable_partition(&order, 5);

    let counting = Arc::new(CountingStore::new(Arc::new(PgEventStore::new(pool.clone()))));
    let settings = settings(true);
    let handler = MailboxCommandHandler::new(deps_over(
        counting.clone() as Arc<dyn EventStore>,
        &pool,
    ))
    .with_activations(settings.clone());

    let worker = MailboxWorker::new(
        pool.clone(),
        "w-A",
        "Conversation",
        // Pacing off: these tests redeliver after induced failures on the very next pass;
        // the spacing window (#313 D4) has its own poison tests.
        WorkerConfig { lease_seconds: 300, retry_spacing_seconds: 0, ..WorkerConfig::default() },
        Arc::new(handler),
    )
    .with_lane_events(Arc::new(ActivationLaneEvents(settings.cache.clone())));
    worker.seed(5).await.expect("seed");
    worker.claim().await.expect("claim");

    // ── Fold-on-first-message: three deliveries, ONE rehydration load. ──
    enqueue(&pool, partition, 0x11, order, "OpenConversation", open_payload(order, restaurant)).await;
    enqueue(&pool, partition, 0x22, order, "PostMessage", post_payload(order, 0x1001, "one")).await;
    enqueue(&pool, partition, 0x33, order, "PostMessage", post_payload(order, 0x1002, "two")).await;
    assert_eq!(worker.drain().await.expect("drain"), 3);
    assert_eq!(
        counting.loads_of(&stream),
        1,
        "the second and third deliveries must fold from the held state, not the log"
    );
    let versions: Vec<i32> = sqlx::query_scalar(
        "SELECT version FROM domain_events WHERE stream_name = $1 ORDER BY version",
    )
    .bind(&stream)
    .fetch_all(&pool)
    .await
    .expect("events");
    assert_eq!(versions, vec![1, 2, 3], "promoted state committed at the right versions");

    // ── The never-wrong signal: a FOREIGN writer moves the stream under the warm activation. ──
    let foreign = PgEventStore::new(pool.clone());
    let actor = Actor {
        user_id: uuid::Uuid::nil(),
        user_type: "ADMIN".into(),
        domain_id: None,
        correlation_id: uuid::Uuid::from_u128(0xF0),
        cause_id: None,
    };
    let (_, live_version) = foreign.load(&stream).await.expect("foreign load");
    foreign
        .append(
            &stream,
            live_version,
            &[DomainEvent::MessagePosted(domain::generated::events::MessagePosted {
                order_id: domain::generated::scalars::OrderId(order),
                message_id: domain::generated::scalars::ConversationMessageId(
                    uuid::Uuid::from_u128(0x1003),
                ),
                author_role: domain::generated::scalars::ConversationAuthorRole::ADMIN,
                visibility: domain::generated::scalars::MessageVisibility::INTERNAL,
                body: domain::generated::scalars::MessageBody("foreign".to_owned()),
                original_locale: domain::generated::scalars::Locale("en-US".to_owned()),
                attachment_refs: Vec::new(),
            })],
            &actor,
        )
        .await
        .expect("foreign append");

    // The next delivery flushes from the (now stale) held state → loses the UNIQUE race →
    // aborts (row stays RECEIVED, held state dropped) → the SAME pass's retry is next drain.
    enqueue(&pool, partition, 0x44, order, "PostMessage", post_payload(order, 0x1004, "after")).await;
    let first = worker.drain().await.expect("drain");
    assert_eq!(first, 0, "the stale flush must abort, not commit or complete");
    let status: String =
        sqlx::query_scalar("SELECT status FROM inbound_messages WHERE message_id = $1")
            .bind(uuid::Uuid::from_u128(0x44))
            .fetch_one(&pool)
            .await
            .expect("status");
    assert_eq!(status, "RECEIVED", "aborted delivery leaves the row undelivered");

    // Retry: refolds from the log (one more inner load) and commits AFTER the foreign event.
    assert_eq!(worker.drain().await.expect("retry drain"), 1);
    assert_eq!(counting.loads_of(&stream), 2, "the retry refolded from the log exactly once");
    let versions: Vec<i32> = sqlx::query_scalar(
        "SELECT version FROM domain_events WHERE stream_name = $1 ORDER BY version",
    )
    .bind(&stream)
    .fetch_all(&pool)
    .await
    .expect("events");
    assert_eq!(versions, vec![1, 2, 3, 4, 5], "no hole, no duplicate around the foreign write");

    // ── Lane loss evicts the lane's holdings. ──
    assert!(settings.cache.len() > 0, "the retry re-filled the activation");
    actor_runtime::steal_lane(&pool, "Conversation", partition, "w-thief", 300)
        .await
        .expect("steal");
    worker.beat().await.expect("beat");
    assert_eq!(settings.cache.len(), 0, "a lost lane drops every held state under it");
}

/// THE FRESHNESS GUARD (#272 review CRITICAL): a delivery that terminates WITHOUT appending (a
/// deterministic rejection) has no `UNIQUE(stream_name, version)` race to lose — a stale held
/// fold would commit a durably WRONG verdict, and each retry's cache touch would keep the entry
/// alive. The guard re-asserts the stream version inside the fenced transaction: mismatch →
/// abort + invalidate, and the redelivery refolds and decides correctly.
#[tokio::test]
async fn stale_hold_cannot_commit_a_wrong_rejection() {
    let Some(url) = gated() else { return };
    let _suite = serialize_suite();
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let order = uuid::Uuid::from_u128(0xAC73);
    let restaurant = uuid::Uuid::from_u128(0x0E59);
    let stream = format!("Conversation-{order}");
    let partition = actor_client::stable_partition(&order, 5);

    let counting = Arc::new(CountingStore::new(Arc::new(PgEventStore::new(pool.clone()))));
    let settings = settings(true);
    let handler = MailboxCommandHandler::new(deps_over(
        counting.clone() as Arc<dyn EventStore>,
        &pool,
    ))
    .with_activations(settings.clone());
    let worker = MailboxWorker::new(
        pool.clone(),
        "w-A",
        "Conversation",
        // Pacing off: these tests redeliver after induced failures on the very next pass;
        // the spacing window (#313 D4) has its own poison tests.
        WorkerConfig { lease_seconds: 300, retry_spacing_seconds: 0, ..WorkerConfig::default() },
        Arc::new(handler),
    );
    worker.seed(5).await.expect("seed");
    worker.claim().await.expect("claim");

    // 1. A PostMessage against the not-yet-existing conversation: correctly REJECTED
    //    (ConversationNotFound) — and the EMPTY stream is now the held activation.
    enqueue(&pool, partition, 0x11, order, "PostMessage", post_payload(order, 0x3001, "early")).await;
    assert_eq!(worker.drain().await.expect("drain"), 1);
    let status: String =
        sqlx::query_scalar("SELECT status FROM inbound_messages WHERE message_id = $1")
            .bind(uuid::Uuid::from_u128(0x11))
            .fetch_one(&pool)
            .await
            .expect("status");
    assert_eq!(status, "REJECTED", "the genuinely-absent conversation rejects");

    // 2. An OUT-OF-BAND writer (the saga runner's shape: pool-side, no cache signal) opens the
    //    conversation under the warm empty hold.
    let foreign = PgEventStore::new(pool.clone());
    let actor = Actor {
        user_id: uuid::Uuid::nil(),
        user_type: "ADMIN".into(),
        domain_id: None,
        correlation_id: uuid::Uuid::from_u128(0xF1),
        cause_id: None,
    };
    foreign
        .append(
            &stream,
            0,
            &[DomainEvent::ConversationOpened(domain::generated::events::ConversationOpened {
                order_id: domain::generated::scalars::OrderId(order),
                restaurant_id: domain::generated::scalars::RestaurantId(restaurant),
                customer_id: None,
                customer_chat_enabled: true,
            })],
            &actor,
        )
        .await
        .expect("foreign open");

    // 3. The next PostMessage folds the STALE empty hold and would terminally reject a
    //    conversation that EXISTS — the guard must abort instead (row stays RECEIVED).
    enqueue(&pool, partition, 0x22, order, "PostMessage", post_payload(order, 0x3002, "after")).await;
    assert_eq!(worker.drain().await.expect("drain"), 0, "the stale-fold rejection must not commit");
    let status: String =
        sqlx::query_scalar("SELECT status FROM inbound_messages WHERE message_id = $1")
            .bind(uuid::Uuid::from_u128(0x22))
            .fetch_one(&pool)
            .await
            .expect("status");
    assert_eq!(status, "RECEIVED", "aborted delivery leaves the row undelivered");

    // 4. The redelivery refolds from the log and SUCCEEDS — the customer's message posts.
    assert_eq!(worker.drain().await.expect("retry drain"), 1);
    let status: String =
        sqlx::query_scalar("SELECT status FROM inbound_messages WHERE message_id = $1")
            .bind(uuid::Uuid::from_u128(0x22))
            .fetch_one(&pool)
            .await
            .expect("status");
    assert_eq!(status, "SUCCEEDED", "the refolded delivery sees the opened conversation");
    let types: Vec<String> = sqlx::query_scalar(
        "SELECT event_type FROM domain_events WHERE stream_name = $1 ORDER BY version",
    )
    .bind(&stream)
    .fetch_all(&pool)
    .await
    .expect("events");
    assert_eq!(types, vec!["ConversationOpened", "MessagePosted"]);
}

#[tokio::test]
async fn per_actor_opt_out_caches_nothing() {
    let Some(url) = gated() else { return };
    let _suite = serialize_suite();
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let order = uuid::Uuid::from_u128(0xAC72);
    let restaurant = uuid::Uuid::from_u128(0x0E58);
    let stream = format!("Conversation-{order}");
    let partition = actor_client::stable_partition(&order, 5);

    let counting = Arc::new(CountingStore::new(Arc::new(PgEventStore::new(pool.clone()))));
    let settings = settings(false); // Conversation opted out (mailbox.activations: false)
    let handler = MailboxCommandHandler::new(deps_over(
        counting.clone() as Arc<dyn EventStore>,
        &pool,
    ))
    .with_activations(settings.clone());

    let worker = MailboxWorker::new(
        pool.clone(),
        "w-A",
        "Conversation",
        // Pacing off: these tests redeliver after induced failures on the very next pass;
        // the spacing window (#313 D4) has its own poison tests.
        WorkerConfig { lease_seconds: 300, retry_spacing_seconds: 0, ..WorkerConfig::default() },
        Arc::new(handler),
    );
    worker.seed(5).await.expect("seed");
    worker.claim().await.expect("claim");

    enqueue(&pool, partition, 0x11, order, "OpenConversation", open_payload(order, restaurant)).await;
    enqueue(&pool, partition, 0x22, order, "PostMessage", post_payload(order, 0x2001, "one")).await;
    assert_eq!(worker.drain().await.expect("drain"), 2);
    assert!(
        counting.loads_of(&stream) >= 2,
        "an opted-out actor folds from the log on every delivery"
    );
    assert_eq!(settings.cache.len(), 0, "nothing held for an opted-out actor");
}
