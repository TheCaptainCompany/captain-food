//! DB-gated tests for the MAILBOX push wake (#313, PROP-20260802-223522 D1/D2): the three
//! Postgres properties the design rests on — each fails SILENTLY if untrue — plus the
//! cross-process end-to-end the feature exists for.
//!
//! 1. **Delivered at COMMIT**: an enqueue through the real `PgMailbox` door raises
//!    `pg_notify('inbound_messages', actor_type)` that a listener receives, payload = the type.
//! 2. **NOT delivered on ROLLBACK**: the notify rides the insert's transaction.
//! 3. **Coalesced per actor type**: identical (channel, payload) notifications inside one
//!    transaction dedupe to a single wake; distinct types stay distinct.
//! 4. **Cross-process delivery**: a producer with NO in-process nudges (a standalone adapter's
//!    posture) enqueues; the consuming side's LISTEN connection nudges the worker and the row is
//!    delivered while the heartbeat is far too slow to explain it.
//!
//! Needs `DATABASE_URL`; skips otherwise (DB_TESTS_REQUIRED makes the skip loud, #230).

use std::sync::Arc;
use std::time::Duration;

use actor_client::generated::actor_clients::CartClient;
use actor_client::mailbox::Envelope;
use actor_runtime::{
    Delivery, HandlerVerdict, InboundMessage, MailboxWorker, MessageHandler, WorkerConfig,
};
use domain::generated::commands::{AddCartLine, CartLine};
use domain::generated::scalars::{CartId, CartLineId, OfferId, RestaurantId, SessionId};
use infrastructure::persistence::mailbox_store::{MailboxNudges, PgMailbox};
use infrastructure::persistence::mailbox_wake::{
    spawn_mailbox_listener, spawn_mailbox_listener_with, MailboxPush, MAILBOX_CHANNEL,
};
use sqlx::postgres::PgListener;
use sqlx::{PgPool, Postgres, Row, Transaction};

/// Serialize the suite: every test resets the same tables.
static DB_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn database_url() -> Option<String> {
    match std::env::var("DATABASE_URL") {
        Ok(url) => Some(url),
        Err(_) => {
            assert!(
                std::env::var("DB_TESTS_REQUIRED").is_err(),
                "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
            );
            eprintln!("SKIP mailbox_wake: DATABASE_URL not set");
            None
        }
    }
}

async fn setup(pool: &PgPool) {
    sqlx::raw_sql(
        "DROP TABLE IF EXISTS inbound_messages, mailbox_partitions CASCADE;\n\
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
}

fn cart_command(cart_id: uuid::Uuid) -> AddCartLine {
    AddCartLine {
        cart_id: CartId(cart_id),
        restaurant_id: RestaurantId(uuid::Uuid::from_u128(0xF00D)),
        line: CartLine {
            cart_line_id: CartLineId(uuid::Uuid::new_v4()),
            offer_id: OfferId(uuid::Uuid::new_v4()),
            quantity: 1,
            selected_option_ids: vec![],
        },
        session_id: SessionId(uuid::Uuid::new_v4()),
    }
}

fn envelope(message_id: uuid::Uuid) -> Envelope {
    Envelope {
        message_id,
        correlation_id: message_id,
        cause_id: None,
        session_id: None,
        trace_id: None,
        user_id: None,
        user_type: "CUSTOMER".into(),
        channel: "GRAPHQL".into(),
    }
}

/// 1. The real door's enqueue notifies at COMMIT, payload = the actor type.
#[tokio::test]
async fn an_enqueue_through_the_door_notifies_with_the_actor_type() {
    let Some(url) = database_url() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let mut listener = PgListener::connect(&url).await.expect("listener");
    listener.listen(MAILBOX_CHANNEL).await.expect("listen");

    let cart = uuid::Uuid::new_v4();
    let mailbox = Arc::new(PgMailbox::new(pool.clone()));
    let client = CartClient::new(mailbox, cart);
    client.send(cart_command(cart), envelope(uuid::Uuid::new_v4())).await.expect("enqueue");

    let n = tokio::time::timeout(Duration::from_secs(5), listener.recv())
        .await
        .expect("a committed enqueue must notify")
        .expect("recv");
    assert_eq!(n.channel(), MAILBOX_CHANNEL);
    assert_eq!(n.payload(), "Cart", "payload names the actor type so only its workers wake");
}

/// 2 + 3. Transaction semantics: a rolled-back notify reaches nobody; identical payloads inside
/// one transaction coalesce to a single wake while distinct types stay distinct wakes.
#[tokio::test]
async fn rollback_notifies_nobody_and_one_transaction_coalesces_per_type() {
    let Some(url) = database_url() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let mut listener = PgListener::connect(&url).await.expect("listener");
    listener.listen(MAILBOX_CHANNEL).await.expect("listen");

    // ROLLBACK: the notification dies with the transaction.
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT pg_notify($1, 'Cart')")
        .bind(MAILBOX_CHANNEL)
        .execute(&mut *tx)
        .await
        .expect("notify in tx");
    tx.rollback().await.expect("rollback");

    // COMMIT: three notifies, two distinct payloads -> exactly two deliveries.
    let mut tx = pool.begin().await.expect("begin");
    for payload in ["Cart", "Cart", "Payment"] {
        sqlx::query("SELECT pg_notify($1, $2)")
            .bind(MAILBOX_CHANNEL)
            .bind(payload)
            .execute(&mut *tx)
            .await
            .expect("notify in tx");
    }
    tx.commit().await.expect("commit");

    let first = tokio::time::timeout(Duration::from_secs(5), listener.recv())
        .await
        .expect("committed notify must arrive")
        .expect("recv");
    let second = tokio::time::timeout(Duration::from_secs(5), listener.recv())
        .await
        .expect("second distinct payload must arrive")
        .expect("recv");
    let mut got = vec![first.payload().to_owned(), second.payload().to_owned()];
    got.sort();
    assert_eq!(got, ["Cart", "Payment"], "the rolled-back notify must not appear, duplicates coalesce");

    // Nothing else is pending — the duplicate 'Cart' coalesced and the rollback vanished.
    assert!(
        tokio::time::timeout(Duration::from_millis(500), listener.recv()).await.is_err(),
        "exactly two notifications expected"
    );
}

/// The delivery witness for the end-to-end test: flips rows SUCCEEDED, nothing else.
struct AckHandler;

#[async_trait::async_trait]
impl MessageHandler for AckHandler {
    async fn handle(
        &self,
        _tx: &mut Transaction<'_, Postgres>,
        _message: &InboundMessage,
        _prepared: actor_runtime::Prepared,
    ) -> Result<Delivery, sqlx::Error> {
        Ok(Delivery::of(HandlerVerdict::Succeeded))
    }
}

/// 4. Cross-process shape: the producer has NO nudges wired (a standalone adapter enqueuing into
/// the shared mailbox); the consuming side's listener turns the commit's NOTIFY into a nudge and
/// the worker delivers — while the heartbeat (300 s) is far too slow to explain it.
#[tokio::test]
async fn a_foreign_process_enqueue_is_delivered_by_push_not_the_heartbeat() {
    let Some(url) = database_url() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    // Consuming side: nudges registry + listener + one Cart worker parked on a glacial heartbeat.
    let nudges = {
        let mut n = MailboxNudges::default();
        n.register("Cart");
        Arc::new(n)
    };
    let push = MailboxPush::new();
    spawn_mailbox_listener(url.clone(), pool.clone(), nudges.clone(), push.clone());
    for _ in 0..100 {
        if push.is_live() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(push.is_live(), "listener must establish within 5s");

    let worker = Arc::new(
        MailboxWorker::new(
            pool.clone(),
            "w-push",
            "Cart",
            WorkerConfig { heartbeat_seconds: 300, ..WorkerConfig::default() },
            Arc::new(AckHandler),
        )
        .with_nudge(nudges.get("Cart").expect("registered"))
        .with_push_live(push.live_flag()),
    );
    worker.seed(5).await.expect("seed");
    let (_tx, rx) = tokio::sync::watch::channel(false);
    std::mem::forget(_tx);
    tokio::spawn({
        let worker = worker.clone();
        async move {
            let _ = worker.run(rx).await;
        }
    });
    // Let the worker's first full pass claim the lanes.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Producing side: a PgMailbox with NO nudges — only the NOTIFY can wake the worker.
    let cart = uuid::Uuid::new_v4();
    let foreign = Arc::new(PgMailbox::new(pool.clone()));
    let client = CartClient::new(foreign, cart);
    let message_id = uuid::Uuid::new_v4();
    client.send(cart_command(cart), envelope(message_id)).await.expect("enqueue");

    let mut delivered = false;
    for _ in 0..100 {
        let status: String =
            sqlx::query("SELECT status FROM inbound_messages WHERE message_id = $1")
                .bind(message_id)
                .fetch_one(&pool)
                .await
                .expect("row")
                .get("status");
        if status == "SUCCEEDED" {
            delivered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(delivered, "push must deliver within ~10s while the heartbeat is 300s");
}

/// 5. The liveness canary round-trips on a healthy connection: `live` holds `true` across many
/// canary intervals (no flapping), proving the self-notify loop works — the guard that catches
/// a silently-deaf LISTEN (#314 review MAJOR-1) must not itself take a healthy listener down.
#[tokio::test]
async fn the_canary_holds_a_healthy_listener_live() {
    let Some(url) = database_url() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let nudges = Arc::new(MailboxNudges::default());
    let push = MailboxPush::new();
    spawn_mailbox_listener_with(
        url.clone(),
        pool.clone(),
        nudges,
        push.clone(),
        Duration::from_millis(150),
    );
    for _ in 0..100 {
        if push.is_live() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(push.is_live(), "listener must establish");
    // ~10 canary rounds; a broken echo path would flap `live` to false within two intervals.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(push.is_live(), "a healthy connection must stay live across many canary rounds");
}

/// 6. Kill-and-recover: sqlx heals a terminated LISTEN backend IN PLACE (`try_recv` surfaces it
/// as `Ok(None)`, `live` need never flap), and the healed listener's catch-up nudge-all delivers
/// a row enqueued DURING the gap — the no-replay window is closed by the catch-up, not luck.
#[tokio::test]
async fn a_killed_listener_recovers_and_catches_up() {
    let Some(url) = database_url() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let nudges = {
        let mut n = MailboxNudges::default();
        n.register("Cart");
        Arc::new(n)
    };
    let push = MailboxPush::new();
    spawn_mailbox_listener(url.clone(), pool.clone(), nudges.clone(), push.clone());
    for _ in 0..100 {
        if push.is_live() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(push.is_live(), "listener must establish");

    let worker = Arc::new(
        MailboxWorker::new(
            pool.clone(),
            "w-kill",
            "Cart",
            WorkerConfig { heartbeat_seconds: 300, ..WorkerConfig::default() },
            Arc::new(AckHandler),
        )
        .with_nudge(nudges.get("Cart").expect("registered"))
        .with_push_live(push.live_flag()),
    );
    worker.seed(5).await.expect("seed");
    let (_tx, rx) = tokio::sync::watch::channel(false);
    std::mem::forget(_tx);
    tokio::spawn({
        let worker = worker.clone();
        async move {
            let _ = worker.run(rx).await;
        }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Kill the LISTEN backend from outside — the crash/pooler-failover shape.
    sqlx::query(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
         WHERE query ILIKE 'LISTEN%' AND pid <> pg_backend_pid()",
    )
    .execute(&pool)
    .await
    .expect("terminate listener backend");
    // Enqueue IMMEDIATELY after the kill: the NOTIFY races the heal and may land in the gap —
    // exactly the no-replay window; only the healed listener's catch-up nudge-all covers it.
    let cart = uuid::Uuid::new_v4();
    let client = CartClient::new(Arc::new(PgMailbox::new(pool.clone())), cart);
    let message_id = uuid::Uuid::new_v4();
    client.send(cart_command(cart), envelope(message_id)).await.expect("enqueue during the gap");

    // The heal + catch-up must deliver it long before the 300 s heartbeat could.
    let mut delivered = false;
    for _ in 0..200 {
        let status: String =
            sqlx::query("SELECT status FROM inbound_messages WHERE message_id = $1")
                .bind(message_id)
                .fetch_one(&pool)
                .await
                .expect("row")
                .get("status");
        if status == "SUCCEEDED" {
            delivered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(push.is_live(), "the healed listener stays live");
    assert!(delivered, "the heal's catch-up nudge-all must deliver what landed in the gap");
}
