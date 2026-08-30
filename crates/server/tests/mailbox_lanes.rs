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

use actor_client::mailbox::MailboxAccess;
use actor_client::supervision::{MailboxLaneRepository, MailboxLaneRow};
use domain::generated::scalars::MailboxLaneRegistration;
use infrastructure::persistence::mailbox_lanes::PgMailboxLaneRepository;
use infrastructure::{
    PgCartRepository, PgCatalogRepository, PgCustomerCreditRepository, PgCustomerRepository,
    PgDeliveryPartnerAvailabilityRepository, PgDeliveryRepository, PgDeliverySatisfactionRepository,
    PgOrderConversationRepository, PgOrderRepository, PgPricingPolicyRepository,
    PgProspectionRepository, PgReclamationRepository, PgRefundQueueRepository,
    PgRestaurantRepository, PgUberEstimationPolicyRepository, PgUberSplitPolicyRepository,
};
use server::graphql_acl::RequestRole;

/// The role-guard witness the transports inject (#639 part B). There is no way to fabricate an
/// `ActingRole`: it comes from a `Principal` or it does not exist, so a test that exercises a role
/// has to name a caller actually BOUND to it. Roles carrying no domain binding by design (ADMIN,
/// EXTERNAL, PUBLIC) ignore the uuid, exactly as `Principal::role_path` does.
fn acting(role: RequestRole) -> server::ActingRole {
    server::Principal::role_binding(role, "test-subject".to_string(), Some(uuid::Uuid::from_u128(0x639)))
        .acting_role(role)
}
use sqlx::PgPool;

/// Both tests in this binary drop and reseed the SAME mailbox tables; cargo runs them on
/// separate threads, so without this lock one test's reset races the other's assertions.
static DB_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Every `RequestRole` except ADMIN — exhaustive BY CONSTRUCTION: the match in the tripwire
/// stops compiling when a variant is added to the enum, so this list cannot silently fall
/// behind it. Earned on this very PR (#536 review): both refusal loops iterated 4 of the 6
/// non-admin roles while their docstrings claimed "every non-ADMIN role is refused" —
/// `RestaurantAccount` and `External` were covered by nobody.
fn every_non_admin_role() -> [RequestRole; 6] {
    // Compile-time exhaustiveness tripwire: when this match stops compiling because a new
    // variant appeared, extend the array below (and decide whether the new role may see the
    // supervision surface — refused is the default).
    fn _exhaustive(r: RequestRole) {
        match r {
            RequestRole::Public
            | RequestRole::Customer
            | RequestRole::RestaurantAccount
            | RequestRole::Restaurant
            | RequestRole::Rider
            | RequestRole::Admin
            | RequestRole::External => {}
        }
    }
    [
        RequestRole::Public,
        RequestRole::Customer,
        RequestRole::RestaurantAccount,
        RequestRole::Restaurant,
        RequestRole::Rider,
        RequestRole::External,
    ]
}

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
    sqlx::raw_sql(include_str!("../../../migrations/20260803004500_mailbox_backoff_next_attempt.sql"))
        .execute(pool)
        .await
        .expect("apply the mailbox backoff migration");
}

/// A minimal seeded world, covering the three states the page must distinguish (#596):
///
/// - **declared AND seeded**: two Conversation registry rows. Lane 0 carries two RECEIVED commands
///   (one old), one SCHEDULED reminder and one SUCCEEDED (terminal — must be invisible); lane 1 is
///   empty. Conversation declares FIVE lanes, so 2, 3 and 4 are declared-but-unseeded.
/// - **declared, NOT seeded, and carrying work**: an `Order` RECEIVED row with no registry row at
///   all. This is the state #596's fix creates and `dba` caught: before the fix a hop addressed to
///   an unseeded lane errored loudly, after it the row simply waits — and a page driven by
///   `mailbox_partitions` would have shown nothing at all for it.
/// - **NOT declared, carrying work**: a Conversation row on partition 7, beyond the declared five.
///   The orphan a width DECREASE strands; nothing else in the system would ever mention it.
async fn seed(pool: &PgPool) {
    sqlx::raw_sql(
        "INSERT INTO mailbox_partitions (actor_type, partition, ownership_version, claimed_by, lease_until, checkpoint) VALUES\n\
           ('Conversation', 0, 3, 'w-A', now() + interval '30 seconds', 18000),\n\
           ('Conversation', 1, 0, NULL, NULL, 0);\n\
         INSERT INTO inbound_messages (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, channel, user_type, correlation_id, status, scheduled_at, received_at, completed_at) VALUES\n\
           ('00000000-0000-0000-0000-000000000001', 'COMMAND', 'Conversation', '10000000-0000-0000-0000-000000000001', 0, 'PostMessage', '{}', 'h1', 'GRAPHQL', 'CUSTOMER', '20000000-0000-0000-0000-000000000001', 'RECEIVED', NULL, now() - interval '60 seconds', NULL),\n\
           ('00000000-0000-0000-0000-000000000002', 'COMMAND', 'Conversation', '10000000-0000-0000-0000-000000000001', 0, 'PostMessage', '{}', 'h2', 'GRAPHQL', 'CUSTOMER', '20000000-0000-0000-0000-000000000002', 'RECEIVED', NULL, now(), NULL),\n\
           ('00000000-0000-0000-0000-000000000003', 'MESSAGE', 'Conversation', '10000000-0000-0000-0000-000000000001', 0, 'CheckPreparationDelay', '{}', 'h3', 'WORKER', 'ADMIN', '20000000-0000-0000-0000-000000000003', 'SCHEDULED', now() + interval '10 minutes', now(), NULL),\n\
           ('00000000-0000-0000-0000-000000000004', 'COMMAND', 'Conversation', '10000000-0000-0000-0000-000000000001', 0, 'PostMessage', '{}', 'h4', 'GRAPHQL', 'CUSTOMER', '20000000-0000-0000-0000-000000000004', 'SUCCEEDED', NULL, now() - interval '120 seconds', now()),\n\
           ('00000000-0000-0000-0000-000000000005', 'EVENT', 'Order', '10000000-0000-0000-0000-000000000005', 2, 'OrderPlaced', '{}', 'h5', 'WORKER', 'EXTERNAL', '20000000-0000-0000-0000-000000000005', 'RECEIVED', NULL, now(), NULL),\n\
           ('00000000-0000-0000-0000-000000000006', 'COMMAND', 'Conversation', '10000000-0000-0000-0000-000000000006', 7, 'PostMessage', '{}', 'h6', 'GRAPHQL', 'CUSTOMER', '20000000-0000-0000-0000-000000000006', 'RECEIVED', NULL, now(), NULL);\n\
         UPDATE inbound_messages SET position = NULL WHERE status = 'SCHEDULED';",
    )
    .execute(pool)
    .await
    .expect("seed lanes + backlog");
}

/// Every lane the DECLARATION says exists — the population this page is now driven by.
fn declared_lane_count() -> usize {
    infrastructure::generated::command_router::ACTOR_MAILBOXES
        .iter()
        .map(|(_, width)| *width as usize)
        .sum()
}

/// Find one lane by its key, failing with the whole population rather than an index panic.
fn lane<'a>(lanes: &'a [MailboxLaneRow], actor_type: &str, partition: i16) -> &'a MailboxLaneRow {
    lanes
        .iter()
        .find(|l| l.actor_type == actor_type && l.partition == partition)
        .unwrap_or_else(|| panic!("no lane {actor_type}/{partition} among {} lanes", lanes.len()))
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
        // RSO-1: the spec-default horizon (900 s) -- tests assert behaviour, not config.
        service_window_horizon: Default::default(),
        }),
        None,
        None,
    )
}

#[tokio::test]
async fn mailbox_lanes_join_counts_and_admin_guard() {
    let Some(url) = db_test_gate::database_url("mailbox_lanes") else { return };
    let _serialized = DB_GATE.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_mailbox_tables(&pool).await;
    seed(&pool).await;

    // 1) The repository join: live rows only, per lane, (actor_type, partition) order — over the
    //    DECLARED population plus anything carrying work outside it (#596).
    let repo = PgMailboxLaneRepository::new(pool.clone());
    let lanes: Vec<MailboxLaneRow> = repo.list(MailboxAccess::for_tests()).await.expect("list lanes");
    assert_eq!(
        lanes.len(),
        declared_lane_count() + 1,
        "every DECLARED lane, plus the one undeclared orphan carrying work. The population is the \
         declaration, NOT the registry: a page driven by `mailbox_partitions` would have shown 2 \
         rows here and hidden both the unseeded Order lane holding an order and the stranded \
         Conversation/7 backlog: {lanes:?}"
    );

    let lane0 = lane(&lanes, "Conversation", 0);
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

    let lane1 = lane(&lanes, "Conversation", 1);
    assert_eq!((lane1.pending, lane1.scheduled), (0, 0), "empty lane counts zero");
    assert!(lane1.claimed_by.is_none() && lane1.lease_until.is_none(), "lane 1 unowned");
    assert!(lane1.oldest_pending_at.is_none());

    // Declared but never seeded, and EMPTY: present, with the registry's absence rendered as
    // zeroes rather than as a missing row. Seeing the declared topology is the point — an
    // operator cannot notice that a lane is missing from a list they have never seen complete.
    let unseeded = lane(&lanes, "Conversation", 4);
    assert_eq!((unseeded.ownership_version, unseeded.checkpoint), (0, 0));
    assert!(unseeded.claimed_by.is_none() && unseeded.lease_until.is_none());
    assert_eq!((unseeded.pending, unseeded.scheduled), (0, 0));
    assert_eq!(unseeded.registration, MailboxLaneRegistration::DECLARED_UNSEEDED);

    // THE REASON `registration` EXISTS: lane 1 is SEEDED and merely unclaimed, lane 4 was NEVER
    // seeded, and every OTHER field on the two rows is identical. One of them will be drained by
    // the next claim pass and the other will never be drained by anybody, and without this field
    // an operator staring at the page cannot tell which is which.
    assert_eq!(
        (
            lane1.ownership_version, lane1.checkpoint,
            lane1.claimed_by.is_none(), lane1.pending, lane1.scheduled,
        ),
        (
            unseeded.ownership_version, unseeded.checkpoint,
            unseeded.claimed_by.is_none(), unseeded.pending, unseeded.scheduled,
        ),
        "if these ever differ, re-read whether `registration` is still load-bearing"
    );
    assert_ne!(
        lane1.registration, unseeded.registration,
        "seeded-but-unclaimed and never-seeded must NOT render the same"
    );
    assert_eq!(lane1.registration, MailboxLaneRegistration::SEEDED);

    // THE CASE #596's FIX CREATES: declared, never seeded, and HOLDING AN ORDER. Its worker has
    // not started, so nothing claims it and nothing will drain it — and the chained hop no longer
    // errors, so this page is the only place that says so.
    let waiting = lane(&lanes, "Order", 2);
    assert_eq!(waiting.pending, 1, "an order waiting on a worker that never started: {waiting:?}");
    assert_eq!(
        waiting.registration,
        MailboxLaneRegistration::DECLARED_UNSEEDED,
        "declared, never seeded, and holding a paid order -- the row the page exists for"
    );
    assert!(waiting.claimed_by.is_none(), "nobody owns an unseeded lane");
    assert_eq!(waiting.ownership_version, 0, "no registry row -> no fencing counter yet");
    assert!(waiting.oldest_pending_at.is_some(), "and it has been waiting since a knowable time");

    // THE ORPHAN a width DECREASE strands: undeclared, unseeded, carrying work. `seed_partitions`'
    // drift check refuses the start that would create it; this row is where the operator sees what
    // was already stranded before the check existed.
    let orphan = lane(&lanes, "Conversation", 7);
    assert_eq!(orphan.pending, 1, "beyond the declared five, and still holding a message");
    assert_eq!(orphan.registration, MailboxLaneRegistration::UNDECLARED_ORPHAN);
    assert_eq!(orphan.ownership_version, 0);
    assert!(orphan.claimed_by.is_none());

    // 2) The GraphQL surface, as ADMIN: lanes serialize with the BIGINT counters as strings.
    let schema = schema_over(&pool);
    let query = "{ mailboxLanes { actorType partition ownershipVersion claimedBy checkpoint pending scheduled oldestPendingAt registration } }";
    let resp = schema
        .execute(async_graphql::Request::new(query).data(acting(RequestRole::Admin)))
        .await;
    assert!(resp.errors.is_empty(), "admin mailboxLanes errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json data");
    let lanes = data["mailboxLanes"].as_array().expect("lanes array");
    assert_eq!(lanes.len(), declared_lane_count() + 1);
    let gql = |actor_type: &str, partition: i64| -> serde_json::Value {
        lanes
            .iter()
            .find(|l| l["actorType"] == actor_type && l["partition"] == partition)
            .unwrap_or_else(|| panic!("no {actor_type}/{partition} lane in the GraphQL response"))
            .clone()
    };
    let gql0 = gql("Conversation", 0);
    assert_eq!(gql0["ownershipVersion"], "3", "BIGINT renders as a decimal string");
    assert_eq!(gql0["checkpoint"], "18000", "BIGINT renders as a decimal string");
    assert_eq!(gql0["pending"], 2);
    assert_eq!(gql0["scheduled"], 1);
    assert!(gql0["oldestPendingAt"].is_string());
    assert_eq!(gql("Conversation", 1)["claimedBy"], serde_json::Value::Null);
    // The unseeded lane holding an order serializes too — the whole point is that an operator
    // reading the ADMIN page, not a Rust test, is the one who has to see it.
    let waiting = gql("Order", 2);
    assert_eq!(waiting["pending"], 1);
    assert_eq!(waiting["claimedBy"], serde_json::Value::Null);
    assert_eq!(waiting["ownershipVersion"], "0");
    assert_eq!(waiting["registration"], "DECLARED_UNSEEDED", "the badge the guide tells them to read first");
    assert_eq!(gql("Conversation", 1)["registration"], "SEEDED");
    assert_eq!(gql("Conversation", 7)["registration"], "UNDECLARED_ORPHAN");

    // 3) The guard: every non-ADMIN role is refused — the supervision surface never leaks.
    for role in every_non_admin_role() {
        let resp = schema
            .execute(async_graphql::Request::new(query).data(acting(role)))
            .await;
        assert_eq!(resp.errors.len(), 1, "{role:?} should be refused: {:?}", resp.errors);
    }
}

/// The PINNING test for #510 (Tidy First, commit 1): the ADMIN `poisonedMailboxMessages` GraphQL
/// surface, executed end to end BEFORE the supervision read port moves behind the capability
/// witness — green on this tree and green after the move is the proof the refactor preserved
/// behaviour rather than merely compiling. Asserts three things: (1) a seeded cap-poisoned row
/// (terminal FAILED + `DeliveryInfrastructureError`) is served to ADMIN with its messageId and
/// errorCode; (2) a terminal handler REJECTION with another error code is NOT listed — the poison
/// predicate, not "any failure"; (3) every non-ADMIN role is refused by the generated guard.
#[tokio::test]
async fn poisoned_mailbox_messages_detail_and_admin_guard() {
    let Some(url) = db_test_gate::database_url("mailbox_lanes") else { return };
    let _serialized = DB_GATE.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_mailbox_tables(&pool).await;
    sqlx::raw_sql(
        "INSERT INTO inbound_messages (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, channel, user_type, correlation_id, status, attempts, error, completed_at) VALUES\n\
           ('00000000-0000-0000-0000-00000000b001', 'COMMAND', 'Cart', '10000000-0000-0000-0000-000000000002', 1, 'AddCartLine', '{}', 'p1', 'GRAPHQL', 'CUSTOMER', '20000000-0000-0000-0000-00000000b001', 'FAILED', 5, '{\"code\": \"DeliveryInfrastructureError\", \"context\": {\"error\": \"transient dependency outage\"}}', now()),\n\
           ('00000000-0000-0000-0000-00000000b002', 'COMMAND', 'Cart', '10000000-0000-0000-0000-000000000002', 1, 'AddCartLine', '{}', 'p2', 'GRAPHQL', 'CUSTOMER', '20000000-0000-0000-0000-00000000b002', 'REJECTED', 1, '{\"code\": \"CartNotFound\", \"context\": {}}', now());",
    )
    .execute(&pool)
    .await
    .expect("seed one poisoned + one rejected row");

    let schema = schema_over(&pool);
    let query = "{ poisonedMailboxMessages { messageId errorCode } }";
    let resp = schema
        .execute(async_graphql::Request::new(query).data(acting(RequestRole::Admin)))
        .await;
    assert!(resp.errors.is_empty(), "admin poisonedMailboxMessages errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("json data");
    let rows = data["poisonedMailboxMessages"].as_array().expect("rows array");
    assert_eq!(rows.len(), 1, "only the cap-poisoned row is listed (never a handler verdict): {rows:?}");
    assert_eq!(rows[0]["messageId"], "00000000-0000-0000-0000-00000000b001");
    assert_eq!(rows[0]["errorCode"], "DeliveryInfrastructureError");

    for role in every_non_admin_role() {
        let resp = schema
            .execute(async_graphql::Request::new(query).data(acting(role)))
            .await;
        assert_eq!(resp.errors.len(), 1, "{role:?} should be refused: {:?}", resp.errors);
    }
}
