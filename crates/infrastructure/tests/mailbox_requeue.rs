//! E2E of the poisoned-row recovery (#315, ADR-20260803-002712 Q1) on real Postgres: a
//! cap-poisoned `inbound_messages` row is listed by the supervision read
//! ([`application::queries::MailboxLaneRepository::poisoned`]), a `RequeueMailboxMessage`
//! command delivered through a REAL worker fleet (the same standalone spawn the adapters use)
//! flips it back to RECEIVED and records the `MailboxMessageRequeued` audit fact — and the
//! flipped row is then REDELIVERED by the fleet to a terminal verdict (the review's blind-spot
//! check: a flip the drain never picks back up would be worse than no tooling). The port's
//! arbitration matrix (already-deliverable converges, handler verdicts refuse, unknown refuses)
//! is asserted directly on [`PgMailboxRequeue`].
//!
//! The poisoned TARGET row is itself a `RequeueMailboxMessage` naming an unknown id: once
//! requeued and redelivered it terminates REJECTED with `MailboxMessageNotFound` — a handler
//! verdict, first attempt, no external tables needed — proving the full circle on one lane.
//!
//! Needs `DATABASE_URL`; skips otherwise (DB_TESTS_REQUIRED makes the skip loud, #230).

use std::sync::Arc;

use application::queries::{MailboxLaneRepository, MailboxRequeue, RequeueOutcome};
use infrastructure::persistence::mailbox_lanes::{PgMailboxLaneRepository, PgMailboxRequeue};
use infrastructure::persistence::mailbox_store::MailboxNudges;
use infrastructure::FailClosedPaymentGateway;
use sqlx::PgPool;

async fn setup(pool: &PgPool) {
    sqlx::raw_sql(
        "DROP TABLE IF EXISTS inbound_messages, mailbox_partitions, domain_events CASCADE;\n\
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
}

/// Insert one cap-poisoned row and return nothing; the caller owns the ids.
async fn seed_poisoned(
    pool: &PgPool,
    message_id: uuid::Uuid,
    actor_type: &str,
    actor_id: uuid::Uuid,
    partition: i16,
    message_type: &str,
    payload: serde_json::Value,
) {
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, correlation_id, status, attempts, error, completed_at) \
         VALUES ($1, 'COMMAND', $2, $3, $4, $5, $6, $7, \
            'GRAPHQL', 'ADMIN', $1, 'FAILED', 5, \
            '{\"code\": \"DeliveryInfrastructureError\", \"context\": {\"error\": \"transient dependency outage\"}}'::jsonb, now())",
    )
    .bind(message_id)
    .bind(actor_type)
    .bind(actor_id)
    .bind(partition)
    .bind(message_type)
    .bind(payload)
    .bind(format!("h-{message_id}"))
    .execute(pool)
    .await
    .expect("seed poisoned row");
}

async fn wait_for_status(pool: &PgPool, message_id: uuid::Uuid, wanted: &str, what: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM inbound_messages WHERE message_id = $1")
                .bind(message_id)
                .fetch_optional(pool)
                .await
                .expect("status");
        if status.as_deref() == Some(wanted) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{what}: never reached {wanted} (status {status:?})"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requeue_recovers_a_poisoned_row_end_to_end() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP mailbox_requeue: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    // TWO poisoned rows: one on the Cart lane (listing + the port's arbitration matrix), and
    // one on the MailboxSupervision lane itself — a RequeueMailboxMessage naming an UNKNOWN id,
    // so its redelivery terminates REJECTED(MailboxMessageNotFound) with no external tables.
    let cart_poisoned = uuid::Uuid::from_u128(0xB0150);
    seed_poisoned(
        &pool,
        cart_poisoned,
        "Cart",
        uuid::Uuid::from_u128(0xCA57),
        3,
        "AddCartLine",
        serde_json::json!({}),
    )
    .await;
    let unknown_target = uuid::Uuid::from_u128(0xDEAD);
    let sup_poisoned = uuid::Uuid::from_u128(0xB0151);
    let partition = actor_client::stable_partition(&unknown_target, 1);
    seed_poisoned(
        &pool,
        sup_poisoned,
        "MailboxSupervision",
        unknown_target,
        partition,
        "RequeueMailboxMessage",
        serde_json::json!({ "targetMessageId": unknown_target }),
    )
    .await;

    // (1) The supervision read surfaces both — id, lane, attempts, the poison error code — and
    // the lane filter matches and misses correctly.
    let repo = PgMailboxLaneRepository::new(pool.clone());
    let listed = repo.poisoned(None, 200).await.expect("poisoned listing");
    assert_eq!(listed.len(), 2);
    let cart_row = listed.iter().find(|r| r.message_id == cart_poisoned).expect("cart row listed");
    assert_eq!(cart_row.actor_type, "Cart");
    assert_eq!(cart_row.attempts, 5);
    assert_eq!(cart_row.error_code.as_deref(), Some("DeliveryInfrastructureError"));
    assert_eq!(repo.poisoned(Some("Cart".into()), 200).await.expect("filtered").len(), 1);
    assert_eq!(repo.poisoned(Some("Payment".into()), 200).await.expect("filtered").len(), 0);

    // (2) Deliver RequeueMailboxMessage (targeting the supervision-lane poisoned row) through a
    // REAL fleet — the adapters' spawn, the generated router, the fenced completion.
    let requeue_cmd = uuid::Uuid::from_u128(0xB0152);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, correlation_id) \
         VALUES ($1, 'COMMAND', 'MailboxSupervision', $2, $3, 'RequeueMailboxMessage', $4, 'h-requeue', \
            'GRAPHQL', 'ADMIN', $1)",
    )
    .bind(requeue_cmd)
    .bind(sup_poisoned)
    .bind(actor_client::stable_partition(&sup_poisoned, 1))
    .bind(serde_json::json!({ "targetMessageId": sup_poisoned }))
    .execute(&pool)
    .await
    .expect("enqueue requeue command");

    // REGISTER the lane's nudge (the monolith root's wiring): an unregistered MailboxNudges
    // no-ops every nudge, so the flip's pg_notify would never wake the worker and redelivery
    // would wait for the 60 s safety-net pass. (The adapter mains share this gap — filed
    // separately; this test wires what the design intends.)
    let nudges = {
        let mut n = MailboxNudges::default();
        n.register("MailboxSupervision");
        Arc::new(n)
    };
    infrastructure::mailbox::spawn_standalone_workers(
        pool.clone(),
        "smoke",
        &["MailboxSupervision"],
        Arc::new(FailClosedPaymentGateway),
        nudges,
        Default::default(),
    );
    wait_for_status(&pool, requeue_cmd, "SUCCEEDED", "the requeue command").await;

    // (3) THE POINT (#315 review finding 3): the flipped row is not just RECEIVED — the fleet
    // actually REDELIVERS it (drain filters on status, never position-vs-checkpoint) to its
    // terminal handler verdict: REJECTED(MailboxMessageNotFound), a first-attempt business
    // rejection, never the attempts cap.
    wait_for_status(&pool, sup_poisoned, "REJECTED", "the requeued row's redelivery").await;
    let (attempts, error) = sqlx::query_as::<_, (i16, Option<serde_json::Value>)>(
        "SELECT attempts, error FROM inbound_messages WHERE message_id = $1",
    )
    .bind(sup_poisoned)
    .fetch_one(&pool)
    .await
    .expect("redelivered row");
    assert_eq!(attempts, 0, "the requeue reset the cap counter; a handler verdict never touches it");
    assert_eq!(
        error.as_ref().and_then(|e| e["code"].as_str()),
        Some("MailboxMessageNotFound"),
        "the redelivery ran the real handler and recorded its verdict"
    );

    // (4) The audit fact is on the supervised row's stream, naming the lane.
    let (event_type, payload) = sqlx::query_as::<_, (String, serde_json::Value)>(
        "SELECT event_type, payload FROM domain_events WHERE stream_name = $1",
    )
    .bind(format!("MailboxSupervision-{sup_poisoned}"))
    .fetch_one(&pool)
    .await
    .expect("audit fact");
    assert_eq!(event_type, "MailboxMessageRequeued");
    assert_eq!(payload["actorType"], "MailboxSupervision");
    assert_eq!(payload["targetMessageId"], serde_json::json!(sup_poisoned));

    // (5) The arbitration matrix, directly on the port: a flip returns the lane; a duplicate
    // converges (the retried-delivery case); a handler verdict refuses with its status;
    // unknown refuses.
    let port = PgMailboxRequeue::new(pool.clone());
    assert_eq!(
        port.requeue_if_poisoned(cart_poisoned).await.expect("flip"),
        RequeueOutcome::Requeued { actor_type: "Cart".into() }
    );
    assert_eq!(
        port.requeue_if_poisoned(cart_poisoned).await.expect("re-requeue"),
        RequeueOutcome::AlreadyDeliverable { actor_type: "Cart".into() },
        "a duplicate/retried requeue converges instead of erroring"
    );
    assert_eq!(
        port.requeue_if_poisoned(requeue_cmd).await.expect("succeeded row"),
        RequeueOutcome::NotRequeueable { status: "SUCCEEDED".into() },
        "a row that already ran is never requeued"
    );
    assert_eq!(
        port.requeue_if_poisoned(uuid::Uuid::from_u128(0xF00D)).await.expect("unknown row"),
        RequeueOutcome::NotFound
    );
    // And the listing is empty again — the screen's count and detail agree (the cart row is
    // RECEIVED, the supervision row REJECTED: neither is poisoned any more).
    assert_eq!(repo.poisoned(None, 200).await.expect("relist").len(), 0);
}
