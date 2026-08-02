//! DB-gated test for the ADMIN `mailboxLanes` supervision query (#242 Runtime B,
//! PROP-20260728-152752): applies the REAL actor-mailbox migration
//! (`migrations/20260731063000_actor_mailbox_tables.sql`, via `include_str!` so this test and the
//! deployed DDL cannot drift), seeds two lanes plus a mixed backlog, and proves three things:
//! (1) the lane join counts only LIVE rows — RECEIVED as pending, SCHEDULED as scheduled, terminal
//! rows invisible — and reports the oldest pending `received_at`; (2) the GraphQL surface serves
//! the lanes to ADMIN with the BIGINT counters rendered as decimal strings; (3) any other role is
//! refused by the generated guard (FORBIDDEN), so the supervision surface never leaks. Needs a real
//! Postgres via `DATABASE_URL`; without it the test SKIPS so `cargo test` stays green offline.

use std::sync::Arc;

use application::queries::{MailboxLaneRepository, MailboxLaneRow};
use infrastructure::persistence::mailbox_lanes::PgMailboxLaneRepository;
use infrastructure::{
    PgCartRepository, PgCatalogRepository, PgCustomerCreditRepository, PgCustomerRepository,
    PgDeliveryPartnerAvailabilityRepository, PgDeliveryRepository, PgDeliverySatisfactionRepository,
    PgOrderConversationRepository, PgOrderRepository, PgPricingPolicyRepository,
    PgProspectionRepository, PgReclamationRepository, PgRefundQueueRepository,
    PgRestaurantRepository, PgUberEstimationPolicyRepository, PgUberSplitPolicyRepository,
};
use server::graphql_acl::RequestRole;
use sqlx::PgPool;

/// Fresh mailbox tables from the ACTUAL migration file — dropped first so the test is re-runnable
/// against a dirty database (the migration itself is forward-only and runs once in production).
async fn reset_mailbox_tables(pool: &PgPool) {
    sqlx::raw_sql(
        "DROP TABLE IF EXISTS inbound_messages, mailbox_partitions CASCADE;\n\
         DROP SEQUENCE IF EXISTS inbound_messages_position_seq;",
    )
    .execute(pool)
    .await
    .expect("drop mailbox tables");
    sqlx::raw_sql(include_str!("../../../migrations/20260731063000_actor_mailbox_tables.sql"))
        .execute(pool)
        .await
        .expect("apply the actor-mailbox migration");
    sqlx::raw_sql(include_str!("../../../migrations/20260802230000_mailbox_attempts_column.sql"))
        .execute(pool)
        .await
        .expect("apply the mailbox attempts migration");
}

/// A minimal seeded world: two Conversation lanes; lane 0 carries two RECEIVED commands (one old),
/// one SCHEDULED reminder and one SUCCEEDED (terminal — must be invisible); lane 1 is empty.
async fn seed(pool: &PgPool) {
    sqlx::raw_sql(
        "INSERT INTO mailbox_partitions (actor_type, partition, ownership_version, claimed_by, lease_until, checkpoint) VALUES\n\
           ('Conversation', 0, 3, 'w-A', now() + interval '30 seconds', 18000),\n\
           ('Conversation', 1, 0, NULL, NULL, 0);\n\
         INSERT INTO inbound_messages (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, channel, user_type, correlation_id, status, scheduled_at, received_at, completed_at) VALUES\n\
           ('00000000-0000-0000-0000-000000000001', 'COMMAND', 'Conversation', '10000000-0000-0000-0000-000000000001', 0, 'PostMessage', '{}', 'h1', 'GRAPHQL', 'CUSTOMER', '20000000-0000-0000-0000-000000000001', 'RECEIVED', NULL, now() - interval '60 seconds', NULL),\n\
           ('00000000-0000-0000-0000-000000000002', 'COMMAND', 'Conversation', '10000000-0000-0000-0000-000000000001', 0, 'PostMessage', '{}', 'h2', 'GRAPHQL', 'CUSTOMER', '20000000-0000-0000-0000-000000000002', 'RECEIVED', NULL, now(), NULL),\n\
           ('00000000-0000-0000-0000-000000000003', 'MESSAGE', 'Conversation', '10000000-0000-0000-0000-000000000001', 0, 'CheckPreparationDelay', '{}', 'h3', 'WORKER', 'ADMIN', '20000000-0000-0000-0000-000000000003', 'SCHEDULED', now() + interval '10 minutes', now(), NULL),\n\
           ('00000000-0000-0000-0000-000000000004', 'COMMAND', 'Conversation', '10000000-0000-0000-0000-000000000001', 0, 'PostMessage', '{}', 'h4', 'GRAPHQL', 'CUSTOMER', '20000000-0000-0000-0000-000000000004', 'SUCCEEDED', NULL, now() - interval '120 seconds', now());\n\
         UPDATE inbound_messages SET position = NULL WHERE status = 'SCHEDULED';",
    )
    .execute(pool)
    .await
    .expect("seed lanes + backlog");
}

/// A schema whose read side is entirely Pg-backed over `pool` — only `mailboxLanes` is queried, so
/// the other repositories never touch their (absent) tables. No write side, no event bus.
fn schema_over(pool: &PgPool) -> server::graphql_schema::CaptainSchema {
    server::graphql_schema::build_schema(
        Some(server::graphql_schema::ReadDeps {
            restaurants: Arc::new(PgRestaurantRepository::new(pool.clone())),
            prospection: Arc::new(PgProspectionRepository::new(pool.clone())),
            pricing_policy: Arc::new(PgPricingPolicyRepository::new(pool.clone())),
            uber_estimation_policy: Arc::new(PgUberEstimationPolicyRepository::new(pool.clone())),
            uber_split_policy: Arc::new(PgUberSplitPolicyRepository::new(pool.clone())),
            catalogs: Arc::new(PgCatalogRepository::new(pool.clone())),
            carts: Arc::new(PgCartRepository::new(pool.clone())),
            orders: Arc::new(PgOrderRepository::new(pool.clone())),
            order_conversations: Arc::new(PgOrderConversationRepository::new(pool.clone())),
            customers: Arc::new(PgCustomerRepository::new(pool.clone())),
            deliveries: Arc::new(PgDeliveryRepository::new(pool.clone())),
            refunds: Arc::new(PgRefundQueueRepository::new(pool.clone())),
            delivery_satisfaction: Arc::new(PgDeliverySatisfactionRepository::new(pool.clone())),
            delivery_partner_availabilities: Arc::new(PgDeliveryPartnerAvailabilityRepository::new(
                pool.clone(),
            )),
            reclamations: Arc::new(PgReclamationRepository::new(pool.clone())),
            customer_credit: Arc::new(PgCustomerCreditRepository::new(pool.clone())),
            mailbox_lanes: Arc::new(PgMailboxLaneRepository::new(pool.clone())),
        }),
        None,
        None,
    )
}

#[tokio::test]
async fn mailbox_lanes_join_counts_and_admin_guard() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP mailbox_lanes: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_mailbox_tables(&pool).await;
    seed(&pool).await;

    // 1) The repository join: live rows only, per lane, (actor_type, partition) order.
    let repo = PgMailboxLaneRepository::new(pool.clone());
    let lanes: Vec<MailboxLaneRow> = repo.list().await.expect("list lanes");
    assert_eq!(lanes.len(), 2, "two registered lanes: {lanes:?}");

    let lane0 = &lanes[0];
    assert_eq!((lane0.actor_type.as_str(), lane0.partition), ("Conversation", 0));
    assert_eq!(lane0.ownership_version, 3);
    assert_eq!(lane0.claimed_by.as_deref(), Some("w-A"));
    assert!(lane0.lease_until.is_some(), "lane 0 holds a live lease");
    assert_eq!(lane0.checkpoint, 18000);
    assert_eq!(lane0.pending, 2, "two RECEIVED rows — the SUCCEEDED one is terminal, invisible");
    assert_eq!(lane0.scheduled, 1, "one SCHEDULED reminder");
    let oldest = lane0.oldest_pending_at.expect("oldest pending timestamp");
    let age = chrono::Utc::now() - oldest;
    assert!(
        age > chrono::Duration::seconds(50) && age < chrono::Duration::seconds(120),
        "oldest pending is the 60s-old RECEIVED row (not the 120s-old SUCCEEDED one): age {age}"
    );

    let lane1 = &lanes[1];
    assert_eq!((lane1.actor_type.as_str(), lane1.partition), ("Conversation", 1));
    assert_eq!((lane1.pending, lane1.scheduled), (0, 0), "empty lane counts zero");
    assert!(lane1.claimed_by.is_none() && lane1.lease_until.is_none(), "lane 1 unowned");
    assert!(lane1.oldest_pending_at.is_none());

    // 2) The GraphQL surface, as ADMIN: lanes serialize with the BIGINT counters as strings.
    let schema = schema_over(&pool);
    let query = "{ mailboxLanes { actorType partition ownershipVersion claimedBy checkpoint pending scheduled oldestPendingAt } }";
    let resp = schema
        .execute(async_graphql::Request::new(query).data(RequestRole::Admin))
        .await;
    assert!(resp.errors.is_empty(), "admin mailboxLanes errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json data");
    let lanes = data["mailboxLanes"].as_array().expect("lanes array");
    assert_eq!(lanes.len(), 2);
    assert_eq!(lanes[0]["actorType"], "Conversation");
    assert_eq!(lanes[0]["partition"], 0);
    assert_eq!(lanes[0]["ownershipVersion"], "3", "BIGINT renders as a decimal string");
    assert_eq!(lanes[0]["checkpoint"], "18000", "BIGINT renders as a decimal string");
    assert_eq!(lanes[0]["pending"], 2);
    assert_eq!(lanes[0]["scheduled"], 1);
    assert!(lanes[0]["oldestPendingAt"].is_string());
    assert_eq!(lanes[1]["claimedBy"], serde_json::Value::Null);

    // 3) The guard: every non-ADMIN role is refused — the supervision surface never leaks.
    for role in [RequestRole::Public, RequestRole::Customer, RequestRole::Restaurant, RequestRole::Rider] {
        let resp = schema
            .execute(async_graphql::Request::new(query).data(role))
            .await;
        assert_eq!(resp.errors.len(), 1, "{role:?} should be refused: {:?}", resp.errors);
    }
}
