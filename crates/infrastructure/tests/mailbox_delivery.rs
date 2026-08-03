//! END-TO-END mailbox delivery (#242 Runtime C2): a COMMAND row in `inbound_messages` flows
//! through the [`actor_runtime`] worker → the GENERATED command router → the unchanged
//! application handler → the staged-event flush — all committing atomically under the lane's
//! `ownership_version` fence. Proves, against a real Postgres:
//!
//! - a delivered `OpenConversation` appends `ConversationOpened` to `domain_events` with
//!   `cause_id = message_id` (the causality chain) and flips the row SUCCEEDED;
//! - a follow-up `PostMessage` (ADMIN, on the same actor) appends in stream order;
//! - a stranger's `PostMessage` (CUSTOMER principal that resolves to no participant) completes
//!   REJECTED with the catalogued `NotAParticipant` error — and appends NOTHING (the #235
//!   enforcement, now running INSIDE the worker path);
//! - the checkpoint lands on the last delivered position;
//! - the post-commit [`StatusBusObserver`] published one update per command, terminal statuses
//!   matching the verdicts.
//!
//! Needs `DATABASE_URL`; skips otherwise (DB_TESTS_REQUIRED makes the skip loud, #230).

use std::sync::Arc;

use actor_runtime::{MailboxWorker, WorkerConfig};
use application::generated::services::{IdentityService, PaymentService};

use infrastructure::generated::command_router::CommandDeps;
use infrastructure::mailbox::{MailboxCommandHandler, StatusBusObserver};
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, FailClosedPaymentGateway,
    OperationStatusBus, PgCatalogRepository, PgCustomerRepository, PgEventStore,
    PgProspectionRepository, PgRestaurantRepository, PgSlugReservationRepository,
    UnverifiedGbpOrderLinkProbe,
};
use sqlx::{PgPool, Row};

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
    // The customer projection table the #235 by_auth_ref bridge resolves against. EMPTY here —
    // a stranger principal must resolve to Ok(None) (no participant), not to an infrastructure
    // error: the delivery path now ABORTS on a resolution error instead of swallowing it into a
    // wrong-class NotAParticipant rejection (PR #270 review).
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

/// Enqueue one COMMAND row addressed to the Conversation instance `actor_id` on `partition`.
async fn enqueue(
    pool: &PgPool,
    partition: i16,
    n: u128,
    actor_id: uuid::Uuid,
    message_type: &str,
    payload: serde_json::Value,
    user_type: &str,
    user_id: Option<uuid::Uuid>,
) -> uuid::Uuid {
    let id = uuid::Uuid::from_u128(n);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, user_id, correlation_id) \
         VALUES ($1, 'COMMAND', 'Conversation', $2, $3, $4, $5, $6, 'GRAPHQL', $7, $8, $1)",
    )
    .bind(id)
    .bind(actor_id)
    .bind(partition)
    .bind(message_type)
    .bind(&payload)
    .bind(format!("h{n}"))
    .bind(user_type)
    .bind(user_id)
    .execute(pool)
    .await
    .expect("enqueue");
    id
}

#[tokio::test]
async fn commands_flow_mailbox_to_domain_events_atomically() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP mailbox_delivery: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let order = uuid::Uuid::from_u128(0x0AD1);
    let restaurant = uuid::Uuid::from_u128(0x0E57);
    let stranger_auth = uuid::Uuid::from_u128(0xBAD);

    // The three deliveries, in mailbox order on ONE lane (head-of-line = stream order).
    let open_id = enqueue(
        &pool,
        2, // any lane < the width-5 keyspace (ADR-20260802-220402); all four rows share it so head-of-line order holds
        0x11,
        order,
        "OpenConversation",
        serde_json::json!({
            "orderId": order,
            "restaurantId": restaurant,
            "customerId": null,
            "customerChatEnabled": true,
        }),
        "ADMIN",
        None,
    )
    .await;
    let post_id = enqueue(
        &pool,
        2, // any lane < the width-5 keyspace (ADR-20260802-220402); all four rows share it so head-of-line order holds
        0x22,
        order,
        "PostMessage",
        serde_json::json!({
            "orderId": order,
            "messageId": uuid::Uuid::from_u128(0x1001),
            "authorRole": "ADMIN",
            "visibility": "INTERNAL",
            "body": "checking in",
            "originalLocale": "en-US",
        }),
        "ADMIN",
        None,
    )
    .await;
    // A CUSTOMER principal whose auth subject resolves to no Customer row: requires.acting
    // (#235) must reject it INSIDE the worker path — and append nothing.
    let stranger_id = enqueue(
        &pool,
        2, // any lane < the width-5 keyspace (ADR-20260802-220402); all four rows share it so head-of-line order holds
        0x33,
        order,
        "PostMessage",
        serde_json::json!({
            "orderId": order,
            "messageId": uuid::Uuid::from_u128(0x1002),
            "authorRole": "CUSTOMER",
            "visibility": "PUBLIC",
            "body": "let me in",
            "originalLocale": "en-US",
        }),
        "CUSTOMER",
        Some(stranger_auth),
    )
    .await;

    let bus = OperationStatusBus::default();
    let mut updates = bus.subscribe();
    let worker = MailboxWorker::new(
        pool.clone(),
        "w-A",
        "Conversation",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(deps_over(&pool))),
    )
    .with_observer(Arc::new(StatusBusObserver::new(bus.clone())));
    worker.seed(5).await.expect("seed");
    worker.claim().await.expect("claim");
    let delivered = worker.drain().await.expect("drain");
    assert_eq!(delivered, 3, "all three commands delivered in one pass");

    // The event log: exactly the two legitimate facts, in stream order, causally chained.
    let events = sqlx::query(
        "SELECT event_type, version, cause_id, user_type FROM domain_events \
         WHERE stream_name = $1 ORDER BY version",
    )
    .bind(format!("Conversation-{order}"))
    .fetch_all(&pool)
    .await
    .expect("events");
    assert_eq!(events.len(), 2, "the stranger's post appended NOTHING");
    assert_eq!(events[0].get::<String, _>("event_type"), "ConversationOpened");
    assert_eq!(events[0].get::<Option<uuid::Uuid>, _>("cause_id"), Some(open_id));
    assert_eq!(events[1].get::<String, _>("event_type"), "MessagePosted");
    assert_eq!(events[1].get::<Option<uuid::Uuid>, _>("cause_id"), Some(post_id));

    // The mailbox verdicts + the recorded rejection.
    let verdicts: Vec<(uuid::Uuid, String, Option<serde_json::Value>)> =
        sqlx::query("SELECT message_id, status, error FROM inbound_messages ORDER BY position")
            .fetch_all(&pool)
            .await
            .expect("verdicts")
            .iter()
            .map(|r| (r.get("message_id"), r.get("status"), r.get("error")))
            .collect();
    assert_eq!(verdicts[0], (open_id, "SUCCEEDED".into(), None));
    assert_eq!(verdicts[1], (post_id, "SUCCEEDED".into(), None));
    assert_eq!(verdicts[2].0, stranger_id);
    assert_eq!(verdicts[2].1, "REJECTED");
    assert_eq!(
        verdicts[2].2.as_ref().and_then(|e| e.get("code")).and_then(|c| c.as_str()),
        Some("NotAParticipant"),
        "the #235 rejection is catalogued on the row: {:?}",
        verdicts[2].2
    );

    // The checkpoint bounds the drain at the last delivered position.
    let checkpoint: i64 = sqlx::query(
        "SELECT checkpoint FROM mailbox_partitions WHERE actor_type = 'Conversation' AND partition = 2",
    )
    .fetch_one(&pool)
    .await
    .expect("lane")
    .get("checkpoint");
    let last_pos: i64 =
        sqlx::query("SELECT max(position) AS p FROM inbound_messages")
            .fetch_one(&pool)
            .await
            .expect("max")
            .get("p");
    assert_eq!(checkpoint, last_pos);

    // The post-commit status fan-out: one update per command, statuses matching the verdicts.
    use domain::generated::scalars::CommandJournalStatus as J;
    let mut seen = Vec::new();
    while let Ok(u) = updates.try_recv() {
        seen.push((u.message_id, u.status, u.error_code));
    }
    assert_eq!(
        seen,
        vec![
            (open_id, J::SUCCEEDED, None),
            (post_id, J::SUCCEEDED, None),
            (stranger_id, J::REJECTED, Some("NotAParticipant".into())),
        ]
    );

    // Atomicity spot-check: re-draining delivers nothing (all rows terminal).
    assert_eq!(worker.drain().await.expect("re-drain"), 0);

    // And the handler is idempotency-honest end to end: a REDELIVERED PostMessage (same
    // business messageId under a fresh mailbox identity) is REJECTED by the aggregate's own
    // MessageAlreadyPosted guard — the fold-based dedupe stayed authoritative.
    let replay_id = enqueue(
        &pool,
        2, // any lane < the width-5 keyspace (ADR-20260802-220402); all four rows share it so head-of-line order holds
        0x44,
        order,
        "PostMessage",
        serde_json::json!({
            "orderId": order,
            "messageId": uuid::Uuid::from_u128(0x1001),
            "authorRole": "ADMIN",
            "visibility": "INTERNAL",
            "body": "checking in",
            "originalLocale": "en-US",
        }),
        "ADMIN",
        None,
    )
    .await;
    assert_eq!(worker.drain().await.expect("replay drain"), 1);
    let (status, error): (String, Option<serde_json::Value>) =
        sqlx::query("SELECT status, error FROM inbound_messages WHERE message_id = $1")
            .bind(replay_id)
            .fetch_one(&pool)
            .await
            .map(|r| (r.get("status"), r.get("error")))
            .expect("replay row");
    assert_eq!(status, "REJECTED");
    assert_eq!(
        error.as_ref().and_then(|e| e.get("code")).and_then(|c| c.as_str()),
        Some("MessageAlreadyPosted")
    );
}
