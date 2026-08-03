//! DB-gated tests for the mailbox runtime — the Proto.Actor-inspired suite (ADR-20260730-234918
//! D2.1 ports, PROP-20260728-152752 §3.1/§3.3 scenarios), against the REAL migration DDL:
//!
//! 1. **Fencing (S5/S6)**: a stolen lane's previous owner cannot commit — its whole transaction
//!    (handler effects included) rolls back; the thief redelivers and commits. Zero duplicate
//!    deliveries COMMITTED (the zero-duplicate-activation probe, done properly: prevention, not
//!    post-hoc repair).
//! 2. **Ordering under takeover**: head-of-line order per lane survives an ownership change —
//!    the successor resumes after the checkpoint, in position order, no gap and no replay.
//! 3. **Idempotent redelivery**: a completed row can never be delivered again (drain skips
//!    non-RECEIVED; a racing double-complete loses on `status = 'RECEIVED'`).
//! 4. **Crash re-claim**: a dead owner's lane (expired lease, RECEIVED row it picked but never
//!    committed) is taken over and the row is delivered by the successor — takeover bumps
//!    `ownership_version`, so the corpse's late commit is fenced out too.
//!
//! Needs `DATABASE_URL`; skips otherwise (DB_TESTS_REQUIRED makes the skip loud, #230).

use std::sync::Arc;

use actor_runtime::{
    claim_due_lanes, complete_fenced, steal_lane, CompletionError, Delivery, HandlerVerdict,
    InboundMessage, MailboxWorker, MessageHandler, WorkerConfig,
};
use sqlx::{PgPool, Postgres, Row, Transaction};

/// A handler that records each delivery into `delivered_probe` THROUGH THE TRANSACTION — so a
/// fenced-out delivery must leave no probe row behind (the atomicity witness).
struct ProbeHandler;

#[async_trait::async_trait]
impl MessageHandler for ProbeHandler {
    async fn handle(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        message: &InboundMessage,
        _prepared: actor_runtime::Prepared,
    ) -> Result<Delivery, sqlx::Error> {
        sqlx::query("INSERT INTO delivered_probe (message_id, position) VALUES ($1, $2)")
            .bind(message.message_id)
            .bind(message.position)
            .execute(&mut **tx)
            .await?;
        Ok(Delivery::of(HandlerVerdict::Succeeded))
    }
}

async fn setup(pool: &PgPool) {
    sqlx::raw_sql(
        "DROP TABLE IF EXISTS inbound_messages, mailbox_partitions, delivered_probe CASCADE;\n\
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
        "CREATE TABLE delivered_probe (\n\
           delivery BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,\n\
           message_id UUID NOT NULL,\n\
           position BIGINT NOT NULL\n\
         )",
    )
    .execute(pool)
    .await
    .expect("probe table");
}

/// Enqueue one COMMAND row on `partition` for a fixed actor instance; returns (message_id, position).
async fn enqueue(pool: &PgPool, partition: i16, n: u128) -> (uuid::Uuid, i64) {
    let id = uuid::Uuid::from_u128(n);
    let row = sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, correlation_id) \
         VALUES ($1, 'COMMAND', 'Conversation', $2, $3, 'PostMessage', '{}', $4, 'GRAPHQL', \
                 'CUSTOMER', $1) \
         RETURNING position",
    )
    .bind(id)
    .bind(uuid::Uuid::from_u128(0xAC70))
    .bind(partition)
    .bind(format!("h{n}"))
    .fetch_one(pool)
    .await
    .expect("enqueue");
    (id, row.get::<i64, _>("position"))
}

/// Schedule one MESSAGE row (a reminder) on `partition`: status SCHEDULED, `position` explicitly
/// NULL — the column default must NOT fire (positions are stamped at promotion, §3.4).
async fn schedule(pool: &PgPool, partition: i16, n: u128, due: chrono::DateTime<chrono::Utc>) -> uuid::Uuid {
    let id = uuid::Uuid::from_u128(n);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, position, kind, actor_type, actor_id, partition, message_type, payload, \
            payload_hash, channel, user_type, correlation_id, scheduled_at, status) \
         VALUES ($1, NULL, 'MESSAGE', 'Conversation', $2, $3, 'OrderExpired', '{}', $4, 'WORKER', \
                 'EXTERNAL', $1, $5, 'SCHEDULED')",
    )
    .bind(id)
    .bind(uuid::Uuid::from_u128(0xAC70))
    .bind(partition)
    .bind(format!("h{n}"))
    .bind(due)
    .execute(pool)
    .await
    .expect("schedule");
    id
}

async fn probe_rows(pool: &PgPool) -> Vec<(uuid::Uuid, i64)> {
    sqlx::query("SELECT message_id, position FROM delivered_probe ORDER BY delivery")
        .fetch_all(pool)
        .await
        .expect("probe rows")
        .iter()
        .map(|r| (r.get("message_id"), r.get("position")))
        .collect()
}

fn gated() -> Option<String> {
    match std::env::var("DATABASE_URL") {
        Ok(url) => Some(url),
        Err(_) => {
            assert!(
                std::env::var("DB_TESTS_REQUIRED").is_err(),
                "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
            );
            eprintln!("SKIP actor_runtime mailbox test: DATABASE_URL not set");
            None
        }
    }
}

/// Serialize the suite: every test resets the same tables.
static DB_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn fenced_out_completion_rolls_back_whole_and_thief_redelivers() {
    let Some(url) = gated() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    actor_runtime::seed_partitions(&pool, "Conversation", 4).await.expect("seed");
    let (mid, pos) = enqueue(&pool, 1, 1).await;

    // A claims the lane and PICKS the message (drain would do this) — but before A completes,
    // B steals the lane (rebalance / A presumed dead).
    let a_lanes = claim_due_lanes(&pool, "Conversation", "w-A", 300, 100).await.expect("A claims");
    let a_lane = a_lanes.iter().find(|l| l.partition == 1).expect("A owns lane 1").clone();
    let b_lane = steal_lane(&pool, "Conversation", 1, "w-B", 300)
        .await
        .expect("steal")
        .expect("lane exists");
    assert_eq!(b_lane.ownership_version, a_lane.ownership_version + 1, "steal bumps the fence");

    // A still BELIEVES it owns the lane and tries to commit its delivery: the fence must reject
    // it, and the probe row A's handler wrote inside the transaction must be gone with it.
    let msg = InboundMessage {
        message_id: mid,
        position: pos,
        kind: "COMMAND".into(),
        actor_type: "Conversation".into(),
        actor_id: uuid::Uuid::from_u128(0xAC70),
        partition: 1,
        message_type: "PostMessage".into(),
        payload: serde_json::json!({}),
        payload_hash: "h1".into(),
        channel: "GRAPHQL".into(),
        user_id: None,
        user_type: "CUSTOMER".into(),
        correlation_id: mid,
        cause_id: None,
        session_id: None,
        received_at: chrono::Utc::now(),
    };
    let out = complete_fenced(&pool, &a_lane, "w-A", &msg, &ProbeHandler).await;
    assert!(matches!(out, Err(CompletionError::FencedOut)), "stale authority must not commit: {out:?}");
    assert!(probe_rows(&pool).await.is_empty(), "A's handler effect rolled back with the fence");
    let status: String =
        sqlx::query("SELECT status FROM inbound_messages WHERE message_id = $1")
            .bind(mid)
            .fetch_one(&pool)
            .await
            .expect("row")
            .get("status");
    assert_eq!(status, "RECEIVED", "the message survived A untouched");

    // B — the live authority — redelivers and commits. Exactly ONE delivery total.
    let verdict = complete_fenced(&pool, &b_lane, "w-B", &msg, &ProbeHandler).await.expect("B commits");
    assert_eq!(verdict, HandlerVerdict::Succeeded);
    assert_eq!(probe_rows(&pool).await, vec![(mid, pos)], "exactly one committed delivery");
}

#[tokio::test]
async fn head_of_line_order_survives_takeover() {
    let Some(url) = gated() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let handler = Arc::new(ProbeHandler);
    let cfg = WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() };
    let a = MailboxWorker::new(pool.clone(), "w-A", "Conversation", cfg.clone(), handler.clone());
    a.seed(4).await.expect("seed");
    let mut expected = Vec::new();
    for n in 1..=5u128 {
        expected.push(enqueue(&pool, 2, n).await);
    }

    // A claims and drains the first TWO (batch = 2 per pass would need a loop; drive drain_lane
    // manually through a batch-2 config for determinism).
    let a2 = MailboxWorker::new(
        pool.clone(),
        "w-A",
        "Conversation",
        WorkerConfig { batch: 2, lease_seconds: 300, ..WorkerConfig::default() },
        handler.clone(),
    );
    a2.claim().await.expect("A claims");
    let lane_a = a2
        .owned()
        .await
        .into_iter()
        .find(|l| l.partition == 2)
        .expect("A owns lane 2");
    // Deliver exactly the first two, then stop (simulate A dying mid-lane).
    for (mid, pos) in &expected[..2] {
        let row = sqlx::query(
            "SELECT message_id, position, kind, actor_type, actor_id, partition, message_type, \
                    payload, payload_hash, channel, user_id, user_type, correlation_id, cause_id, \
                    session_id, received_at \
             FROM inbound_messages WHERE message_id = $1",
        )
        .bind(mid)
        .fetch_one(&pool)
        .await
        .expect("row");
        let msg = InboundMessage {
            message_id: row.get("message_id"),
            position: row.get("position"),
            kind: row.get("kind"),
            actor_type: row.get("actor_type"),
            actor_id: row.get("actor_id"),
            partition: row.get("partition"),
            message_type: row.get("message_type"),
            payload: row.get("payload"),
            payload_hash: row.get("payload_hash"),
            channel: row.get("channel"),
            user_id: row.get("user_id"),
            user_type: row.get("user_type"),
            correlation_id: row.get("correlation_id"),
            cause_id: row.get("cause_id"),
            session_id: row.get("session_id"),
            received_at: row.get("received_at"),
        };
        complete_fenced(&pool, &lane_a, "w-A", &msg, handler.as_ref()).await.expect("A delivers");
        assert_eq!(msg.position, *pos);
    }

    // B takes over (steal — same effect as expiry takeover, just deterministic) and drains the rest.
    let b = MailboxWorker::new(pool.clone(), "w-B", "Conversation", cfg, handler.clone());
    let lane_b = steal_lane(&pool, "Conversation", 2, "w-B", 300)
        .await
        .expect("steal")
        .expect("lane");
    assert_eq!(lane_b.checkpoint, expected[1].1, "checkpoint = last position A committed");
    let delivered = b.drain_lane(&lane_b).await.expect("B drains");
    assert_eq!(delivered, 3, "B delivers exactly the remaining three");

    // The probe saw all five, in strict position order, exactly once each.
    let seen = probe_rows(&pool).await;
    assert_eq!(seen, expected, "head-of-line order across the takeover, no gap, no replay");
}

#[tokio::test]
async fn completed_rows_are_never_redelivered() {
    let Some(url) = gated() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let handler = Arc::new(ProbeHandler);
    let w = MailboxWorker::new(
        pool.clone(),
        "w-A",
        "Conversation",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        handler.clone(),
    );
    w.seed(4).await.expect("seed");
    let (mid, pos) = enqueue(&pool, 3, 9).await;
    w.claim().await.expect("claim");
    let lane = w.owned().await.into_iter().find(|l| l.partition == 3).expect("lane 3");
    assert_eq!(w.drain_lane(&lane).await.expect("first drain"), 1);

    // A second drain pass sees nothing (status != RECEIVED)…
    assert_eq!(w.drain_lane(&lane).await.expect("second drain"), 0);
    // …and even a racing direct double-complete loses on the `status = 'RECEIVED'` guard.
    let msg = InboundMessage {
        message_id: mid,
        position: pos,
        kind: "COMMAND".into(),
        actor_type: "Conversation".into(),
        actor_id: uuid::Uuid::from_u128(0xAC70),
        partition: 3,
        message_type: "PostMessage".into(),
        payload: serde_json::json!({}),
        payload_hash: "h9".into(),
        channel: "GRAPHQL".into(),
        user_id: None,
        user_type: "CUSTOMER".into(),
        correlation_id: mid,
        cause_id: None,
        session_id: None,
        received_at: chrono::Utc::now(),
    };
    let out = complete_fenced(&pool, &lane, "w-A", &msg, handler.as_ref()).await;
    assert!(matches!(out, Err(CompletionError::AlreadyCompleted)), "{out:?}");
    assert_eq!(probe_rows(&pool).await.len(), 1, "exactly one committed delivery, ever");
}

#[tokio::test]
async fn expired_lease_is_reclaimed_and_the_orphan_row_delivered() {
    let Some(url) = gated() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    actor_runtime::seed_partitions(&pool, "Conversation", 4).await.expect("seed");
    let (_mid, pos) = enqueue(&pool, 0, 5).await;

    // A claims with a 0-second lease (instantly expired — the crash surrogate) and dies without
    // completing the row it picked.
    let a_lanes = claim_due_lanes(&pool, "Conversation", "w-A", 0, 100).await.expect("A claims");
    let a_lane = a_lanes.iter().find(|l| l.partition == 0).expect("A owned lane 0").clone();

    // B's ordinary expiry takeover finds the lane claimable and re-claims it (fence bumps).
    let b_lanes = claim_due_lanes(&pool, "Conversation", "w-B", 300, 100).await.expect("B claims");
    let b_lane = b_lanes.iter().find(|l| l.partition == 0).expect("B took over lane 0").clone();
    assert!(b_lane.ownership_version > a_lane.ownership_version, "takeover is an ownership change");

    // B drains the orphaned RECEIVED row.
    let handler = Arc::new(ProbeHandler);
    let b = MailboxWorker::new(
        pool.clone(),
        "w-B",
        "Conversation",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        handler,
    );
    assert_eq!(b.drain_lane(&b_lane).await.expect("B drains"), 1);
    assert_eq!(probe_rows(&pool).await.len(), 1);
    let (_, delivered_pos) = probe_rows(&pool).await[0];
    assert_eq!(delivered_pos, pos);
}

/// PR #270 review C2: positions are sequence-allocated at INSERT, not commit-ordered, so a row
/// can be RECEIVED *below* the lane's checkpoint (its insert committed after a higher-position
/// row was delivered). The drain must find it after a takeover — a `position > checkpoint`
/// filter would strand it forever.
#[tokio::test]
async fn below_checkpoint_received_row_survives_takeover() {
    let Some(url) = gated() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    actor_runtime::seed_partitions(&pool, "Conversation", 4).await.expect("seed");
    let (_m1, p1) = enqueue(&pool, 0, 21).await;
    let (m2, p2) = enqueue(&pool, 0, 22).await;
    assert!(p1 < p2);

    // A owns the lane and completes ONLY the higher-position row (the lower one's insert is the
    // late-committing transaction in the real race — here it simply sits RECEIVED).
    let a_lanes = claim_due_lanes(&pool, "Conversation", "w-A", 300, 100).await.expect("A claims");
    let a_lane = a_lanes.iter().find(|l| l.partition == 0).expect("lane 0").clone();
    let row2 = sqlx::query(
        "SELECT message_id, position, kind, actor_type, actor_id, partition, message_type, \
                payload, payload_hash, channel, user_id, user_type, correlation_id, cause_id, \
                session_id, received_at \
         FROM inbound_messages WHERE message_id = $1",
    )
    .bind(m2)
    .fetch_one(&pool)
    .await
    .expect("row2");
    let msg2 = decode_for_test(&row2);
    complete_fenced(&pool, &a_lane, "w-A", &msg2, &ProbeHandler).await.expect("A completes row2");

    // The checkpoint now sits at p2, ABOVE the still-RECEIVED p1.
    let ckpt: i64 = sqlx::query(
        "SELECT checkpoint FROM mailbox_partitions WHERE actor_type = 'Conversation' AND partition = 0",
    )
    .fetch_one(&pool)
    .await
    .expect("ckpt")
    .get("checkpoint");
    assert_eq!(ckpt, p2);

    // B takes over: its fresh Lane carries checkpoint p2 from the registry. The drain must STILL
    // deliver p1 — status alone defines what is undelivered.
    let b_lane = steal_lane(&pool, "Conversation", 0, "w-B", 300)
        .await
        .expect("steal")
        .expect("lane exists");
    let b = MailboxWorker::new(
        pool.clone(),
        "w-B",
        "Conversation",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(ProbeHandler),
    );
    assert_eq!(b.drain_lane(&b_lane).await.expect("B drains"), 1, "the below-checkpoint row was delivered");
    let delivered: Vec<i64> = probe_rows(&pool).await.iter().map(|(_, p)| *p).collect();
    assert!(delivered.contains(&p1), "p1 must not be stranded below the checkpoint");
}

/// PR #270 review C1: a DROPPED shutdown sender must behave like a shutdown that never fires —
/// the loop keeps its heartbeat pacing (no zero-sleep busy loop) and keeps running.
#[tokio::test]
async fn dropped_shutdown_sender_keeps_heartbeat_pacing() {
    let Some(url) = gated() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let worker = Arc::new(MailboxWorker::new(
        pool.clone(),
        "w-drop",
        "Conversation",
        WorkerConfig { lease_seconds: 30, heartbeat_seconds: 2, ..WorkerConfig::default() },
        Arc::new(ProbeHandler),
    ));
    worker.seed(4).await.expect("seed");
    let (_tx, rx) = tokio::sync::watch::channel(false);
    drop(_tx); // the C1 production bug, on purpose
    let w = worker.clone();
    tokio::spawn(async move { w.run(rx).await });

    // Let the first (empty) pass complete, then enqueue mid-sleep.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    enqueue(&pool, 0, 31).await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(
        probe_rows(&pool).await.is_empty(),
        "delivered mid-sleep: the loop is spinning instead of sleeping (C1 busy loop)"
    );
    // ...and after the heartbeat elapses the loop is still alive and delivers.
    tokio::time::sleep(std::time::Duration::from_millis(2600)).await;
    assert_eq!(probe_rows(&pool).await.len(), 1, "the loop must keep running after the sender drop");
}

/// The enqueue-side nudge wakes the worker ahead of the heartbeat: with a 30s heartbeat, a
/// nudged enqueue is delivered in well under a second.
#[tokio::test]
async fn nudge_wakes_the_worker_before_the_heartbeat() {
    let Some(url) = gated() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let nudge = Arc::new(tokio::sync::Notify::new());
    let worker = Arc::new(
        MailboxWorker::new(
            pool.clone(),
            "w-nudge",
            "Conversation",
            WorkerConfig { lease_seconds: 30, heartbeat_seconds: 30, ..WorkerConfig::default() },
            Arc::new(ProbeHandler),
        )
        .with_nudge(nudge.clone()),
    );
    worker.seed(4).await.expect("seed");
    let (tx, rx) = tokio::sync::watch::channel(false);
    let w = worker.clone();
    let run = tokio::spawn(async move { w.run(rx).await });

    tokio::time::sleep(std::time::Duration::from_millis(300)).await; // first pass done, now sleeping
    enqueue(&pool, 0, 41).await;
    nudge.notify_one();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if !probe_rows(&pool).await.is_empty() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "nudge did not wake the worker");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    tx.send(true).expect("shutdown");
    run.await.expect("join").expect("clean shutdown");
}

/// Reminders (ADR-20260731-150500, PROP-20260728-152752 §3.4): a SCHEDULED row is invisible to
/// claim/drain until [`actor_runtime::promote_due`] stamps it RECEIVED with a FRESH position —
/// after which it is an ordinary mailbox row and a worker pass delivers it (kind MESSAGE included:
/// the ProbeHandler accepts anything; the host's MESSAGE route lands with the spec deltas).
#[tokio::test]
async fn scheduled_row_is_invisible_until_promoted_then_delivered() {
    let Some(url) = gated() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let handler = Arc::new(ProbeHandler);
    let w = MailboxWorker::new(
        pool.clone(),
        "w-A",
        "Conversation",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        handler,
    );
    w.seed(4).await.expect("seed");
    let due = schedule(&pool, 1, 0x51, chrono::Utc::now() - chrono::Duration::seconds(1)).await;
    let future = schedule(&pool, 1, 0x52, chrono::Utc::now() + chrono::Duration::hours(1)).await;

    // Before promotion: a full claim + drain pass delivers NOTHING — SCHEDULED is not RECEIVED.
    w.claim().await.expect("claim");
    assert_eq!(w.drain().await.expect("pre-promotion drain"), 0);
    assert!(probe_rows(&pool).await.is_empty());

    // Promotion stamps exactly the due row: RECEIVED + a fresh position; the future row keeps
    // status SCHEDULED and position NULL.
    assert_eq!(w.promote().await.expect("promote"), 1);
    let rows: Vec<(uuid::Uuid, String, Option<i64>)> =
        sqlx::query("SELECT message_id, status, position FROM inbound_messages ORDER BY message_id")
            .fetch_all(&pool)
            .await
            .expect("rows")
            .iter()
            .map(|r| (r.get("message_id"), r.get("status"), r.get("position")))
            .collect();
    assert_eq!(rows[0].0, due);
    assert_eq!(rows[0].1, "RECEIVED");
    assert!(rows[0].2.is_some(), "promotion stamps a position");
    assert_eq!(rows[1], (future, "SCHEDULED".into(), None), "the undue row is untouched");

    // Idempotent: nothing left to promote.
    assert_eq!(w.promote().await.expect("re-promote"), 0);

    // An ordinary drain pass now delivers the promoted row — and only it.
    assert_eq!(w.drain().await.expect("post-promotion drain"), 1);
    let delivered = probe_rows(&pool).await;
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].0, due);
}

/// Test-local decode of a full mailbox row (the crate's own decode is private).
fn decode_for_test(row: &sqlx::postgres::PgRow) -> InboundMessage {
    InboundMessage {
        message_id: row.get("message_id"),
        position: row.get("position"),
        kind: row.get("kind"),
        actor_type: row.get("actor_type"),
        actor_id: row.get("actor_id"),
        partition: row.get("partition"),
        message_type: row.get("message_type"),
        payload: row.get("payload"),
        payload_hash: row.get("payload_hash"),
        channel: row.get("channel"),
        user_id: row.get("user_id"),
        user_type: row.get("user_type"),
        correlation_id: row.get("correlation_id"),
        cause_id: row.get("cause_id"),
        session_id: row.get("session_id"),
        received_at: row.get("received_at"),
    }
}

// ================================================================================================
// PREPARE phase (ADR-20260801-023000): host work with NO transaction open, before the fenced
// commit — the R2 "prepare-outside-tx single delivery" primitive.
// ================================================================================================

/// A handler whose prepare produces a value; handle records it through the transaction — proving
/// the prepared value crosses the phase boundary and commits atomically with the delivery.
struct PreparingHandler;

#[async_trait::async_trait]
impl MessageHandler for PreparingHandler {
    async fn prepare(
        &self,
        message: &InboundMessage,
    ) -> Result<actor_runtime::Prepared, sqlx::Error> {
        // Stand-in for the host's no-tx-open work (pool reads, an external gateway call).
        Ok(Some(Box::new(format!("prepared:{}", message.message_id))))
    }

    async fn handle(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        message: &InboundMessage,
        prepared: actor_runtime::Prepared,
    ) -> Result<Delivery, sqlx::Error> {
        let value = prepared
            .and_then(|p| p.downcast::<String>().ok())
            .map(|s| *s)
            .unwrap_or_else(|| "MISSING".into());
        assert_eq!(value, format!("prepared:{}", message.message_id), "prepared value must reach handle");
        sqlx::query("INSERT INTO delivered_probe (message_id, position) VALUES ($1, $2)")
            .bind(message.message_id)
            .bind(message.position)
            .execute(&mut **tx)
            .await?;
        Ok(Delivery::of(HandlerVerdict::Succeeded))
    }
}

/// A handler whose prepare fails (the gateway is down): the delivery must abort with the row
/// still RECEIVED — redelivery is the retry — and handle must never run.
struct FailingPrepareHandler;

#[async_trait::async_trait]
impl MessageHandler for FailingPrepareHandler {
    async fn prepare(
        &self,
        _message: &InboundMessage,
    ) -> Result<actor_runtime::Prepared, sqlx::Error> {
        Err(sqlx::Error::Protocol("gateway unreachable during prepare".into()))
    }

    async fn handle(
        &self,
        _tx: &mut Transaction<'_, Postgres>,
        _message: &InboundMessage,
        _prepared: actor_runtime::Prepared,
    ) -> Result<Delivery, sqlx::Error> {
        panic!("handle must not run when prepare failed");
    }
}

#[tokio::test]
async fn prepared_value_reaches_handle_and_commits() {
    let Some(url) = gated() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    actor_runtime::seed_partitions(&pool, "Conversation", 4).await.expect("seed");
    let (mid, pos) = enqueue(&pool, 1, 71).await;
    let lanes = claim_due_lanes(&pool, "Conversation", "w-P", 300, 100).await.expect("claim");
    let lane = lanes.iter().find(|l| l.partition == 1).expect("lane 1").clone();

    let row = sqlx::query(
        "SELECT message_id, position, kind, actor_type, actor_id, partition, message_type, \
                payload, payload_hash, channel, user_id, user_type, correlation_id, cause_id, \
                session_id, received_at \
         FROM inbound_messages WHERE message_id = $1",
    )
    .bind(mid)
    .fetch_one(&pool)
    .await
    .expect("row");
    let msg = decode_for_test(&row);

    let verdict =
        complete_fenced(&pool, &lane, "w-P", &msg, &PreparingHandler).await.expect("commit");
    assert_eq!(verdict, HandlerVerdict::Succeeded);
    assert_eq!(probe_rows(&pool).await, vec![(mid, pos)]);
}

#[tokio::test]
async fn failed_prepare_aborts_delivery_and_row_stays_received() {
    let Some(url) = gated() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    actor_runtime::seed_partitions(&pool, "Conversation", 4).await.expect("seed");
    let (mid, _pos) = enqueue(&pool, 1, 72).await;
    let lanes = claim_due_lanes(&pool, "Conversation", "w-F", 300, 100).await.expect("claim");
    let lane = lanes.iter().find(|l| l.partition == 1).expect("lane 1").clone();

    let row = sqlx::query(
        "SELECT message_id, position, kind, actor_type, actor_id, partition, message_type, \
                payload, payload_hash, channel, user_id, user_type, correlation_id, cause_id, \
                session_id, received_at \
         FROM inbound_messages WHERE message_id = $1",
    )
    .bind(mid)
    .fetch_one(&pool)
    .await
    .expect("row");
    let msg = decode_for_test(&row);

    let out = complete_fenced(&pool, &lane, "w-F", &msg, &FailingPrepareHandler).await;
    assert!(matches!(out, Err(CompletionError::Db(_))), "prepare error aborts the delivery: {out:?}");
    assert!(probe_rows(&pool).await.is_empty(), "nothing committed");
    let status: String = sqlx::query("SELECT status FROM inbound_messages WHERE message_id = $1")
        .bind(mid)
        .fetch_one(&pool)
        .await
        .expect("row")
        .get("status");
    assert_eq!(status, "RECEIVED", "the row awaits redelivery — retry is the recovery");
}
