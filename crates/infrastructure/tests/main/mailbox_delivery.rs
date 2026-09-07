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
//! Needs `DATABASE_URL`: since #474 a missing database FAILS this suite; only an explicit
//! `DB_TESTS_REQUIRED=0` skips it, and that leaves a receipt (`crates/db_test_gate`).

use std::sync::Arc;

use actor_runtime::{MailboxWorker, WorkerConfig};
use application::generated::services::{IdentityService, PaymentService};

use infrastructure::generated::command_router::CommandDeps;
use infrastructure::mailbox::{MailboxCommandHandler, StatusBusObserver};
use actor_client::{ActorClient, OperationStatusBus, OperationWatchEvent};
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, FailClosedPaymentGateway,
    PgCatalogRepository, PgCustomerRepository, PgEventStore,
    PgProspectionRepository, PgRestaurantRepository, PgAuthSubjectReservationRepository, PgSlugReservationRepository,
    UnverifiedGbpOrderLinkProbe,
};
use sqlx::{PgPool, Row};

fn deps_over(pool: &PgPool) -> CommandDeps {
    CommandDeps {
        store: Arc::new(PgEventStore::new(pool.clone())),
        // #639 part C step 2c-i: the rider sign-in door's bridge + support route (not exercised here).
        riders: Arc::new(infrastructure::PgRiderRepository::new(pool.clone())),
        members: Arc::new(infrastructure::PgMemberRepository::new(pool.clone())),
        support_contact: None,
        run_rider_restriction_door: false,
        run_member_access_grant: false,
        run_member_sign_in_door: false,
        run_restaurant_invitation: false,
        run_platform_access_grant: false,
        run_admin_sign_in_door: false,
        platform_members: Arc::new(infrastructure::PgPlatformMemberRepository::new(pool.clone())),
        restaurants: Arc::new(PgRestaurantRepository::new(pool.clone())),
        slugs: Arc::new(PgSlugReservationRepository::new(pool.clone())),
        auth_subjects: Arc::new(PgAuthSubjectReservationRepository::new(pool.clone())),
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
        // RSO-1: the service-hours enforcement gate at its spec default (OFF = shadow).
        enforce_service_hours_guard: false,
        // #167: the acceptance-timeout gate at its spec default (OFF = shadow).
        enforce_acceptance_timeout: false,
        // #588: the Order-lane birth routing at its spec default (OFF = the legacy append).
        route_gates: application::generated::process_managers::RouteGates {
            order_placed_to_order: false,
            place_replacement_order_to_order: false,
            // #807: routed `send:` steps -- OFF, this fixture exercises the birth routes.
            bind_cart_to_customer_to_cart: false,
            grant_customer_credit_to_customer_credit: false,
            mark_order_delivered_to_order: false,
        },
            quote_guard: application::quote::QuoteGuard::closed_for_tests().into(),
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
    let Some(db) = crate::common::TestDb::acquire("mailbox_delivery").await else { return };
    let pool = db.pool();

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
    // The typed §2.1 watch is the only consumer surface of the bus (#303): one watch per
    // acceptance handle, all subscribed BEFORE the drain so no transition is missed.
    let reader = ActorClient::new(
        Arc::new(infrastructure::persistence::mailbox_store::PgMailbox::new(pool.clone())),
        bus.clone(),
    );
    let mut watches =
        [open_id, post_id, stranger_id].map(|message_id| {
            (message_id, reader.watch(message_id).expect("a bus-backed door watches"))
        });
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

    // The post-commit status fan-out, consumed through the typed watch (#303): one update per
    // command — already buffered, each watch filtered to its handle — statuses matching the
    // verdicts in the mailbox-native enum.
    use domain::generated::scalars::InboundMessageStatus as M;
    let mut seen = Vec::new();
    for (message_id, watch) in &mut watches {
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), watch.next())
            .await
            .expect("the committed verdict was published");
        match event {
            Some(OperationWatchEvent::Update(u)) => {
                assert_eq!(u.message_id, *message_id, "the watch never leaks another handle");
                seen.push((u.message_id, u.status, u.error_code));
            }
            other => panic!("expected an update for {message_id}, got {other:?}"),
        }
    }
    assert_eq!(
        seen,
        vec![
            (open_id, M::SUCCEEDED, None),
            (post_id, M::SUCCEEDED, None),
            (stranger_id, M::REJECTED, Some("NotAParticipant".into())),
        ]
    );
    // …and EXACTLY one: a second next() on any watch must starve (every other update on the
    // shared bus is filtered out), so a double-publish for one command cannot pass unnoticed.
    for (message_id, watch) in &mut watches {
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(200), watch.next())
                .await
                .is_err(),
            "a second update was published for {message_id}"
        );
    }

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
