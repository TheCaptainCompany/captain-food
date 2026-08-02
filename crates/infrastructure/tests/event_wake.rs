//! Integration test for the push wake path: `pg_notify` raised inside the append transaction →
//! Postgres `LISTEN` → [`infrastructure::EventWake`] → a parked drain loop.
//!
//! The whole design rests on three properties of `NOTIFY` that are worth PROVING rather than
//! trusting, because every one of them fails silently:
//!
//! 1. a notification raised inside a transaction is delivered on COMMIT;
//! 2. it is NOT delivered when the transaction rolls back (else a drain would chase events that
//!    were never durably written);
//! 3. identical empty-payload notifications within one transaction COALESCE, so a multi-event
//!    append wakes the drains once rather than once per event.
//!
//! Needs a real Postgres: set `DATABASE_URL`. Without it the test SKIPS so `cargo test` stays green
//! offline (same convention as the other DB-backed tests in this folder).

use std::time::Duration;

use infrastructure::{spawn_event_listener, EventWake};
use sqlx::PgPool;

/// These tests share one DATABASE_URL and notify on the same channel — serialize them.
static DB_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
fn db_lock() -> &'static tokio::sync::Mutex<()> {
    DB_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// How long a wake is allowed to take before we call it lost. Generous — the assertion is about
/// delivery, not latency.
const WAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long we wait to be convinced NOTHING will arrive.
const SILENCE_WINDOW: Duration = Duration::from_millis(750);

/// Connect, start the listener, and wait for it to report live so the test never races the LISTEN.
async fn wake_listening(url: &str) -> EventWake {
    let wake = EventWake::new();
    spawn_event_listener(url.to_string(), wake.clone());
    for _ in 0..100 {
        if wake.is_live() {
            return wake;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("listener never reported live — LISTEN unavailable on this connection");
}

#[tokio::test]
async fn a_committed_notify_wakes_a_parked_drain_loop() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP a_committed_notify_wakes_a_parked_drain_loop: DATABASE_URL not set");
        return;
    };
    let _guard = db_lock().lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    let wake = wake_listening(&url).await;

    // The listener signals once on connect (it may have missed notifications while down); consume
    // that so the assertion below is about OUR notify, not the connect signal.
    let mut waiter = wake.waiter();
    waiter.wait(SILENCE_WINDOW).await;

    // Exactly what `PgEventStore::append` does: notify inside the transaction, then commit.
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT pg_notify($1, '')")
        .bind("domain_events")
        .execute(&mut *tx)
        .await
        .expect("pg_notify");
    tx.commit().await.expect("commit");

    tokio::time::timeout(WAKE_TIMEOUT, waiter.wait(Duration::from_secs(60)))
        .await
        .expect("a committed append must wake the drain loop well before the safety net");
}

#[tokio::test]
async fn a_rolled_back_notify_wakes_nobody() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP a_rolled_back_notify_wakes_nobody: DATABASE_URL not set");
        return;
    };
    let _guard = db_lock().lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    let wake = wake_listening(&url).await;

    let mut waiter = wake.waiter();
    waiter.wait(SILENCE_WINDOW).await; // drain the connect signal

    // A version conflict rolls the whole append back — nothing durable happened, so nothing should
    // be woken. (If NOTIFY leaked out of an aborted transaction, drains would spin on phantom work.)
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT pg_notify($1, '')")
        .bind("domain_events")
        .execute(&mut *tx)
        .await
        .expect("pg_notify");
    tx.rollback().await.expect("rollback");

    // The waiter must sit out the whole window: a wake here means it observed a phantom append.
    let woke = tokio::time::timeout(SILENCE_WINDOW, waiter.wait(Duration::from_secs(60)))
        .await
        .is_ok();
    assert!(!woke, "a rolled-back append must not wake any drain loop");
}

#[tokio::test]
async fn a_multi_event_append_coalesces_into_one_wake() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP a_multi_event_append_coalesces_into_one_wake: DATABASE_URL not set");
        return;
    };
    let _guard = db_lock().lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    let wake = wake_listening(&url).await;

    let mut waiter = wake.waiter();
    waiter.wait(SILENCE_WINDOW).await; // drain the connect signal

    // Three notifies in one transaction — the shape of a command emitting several events. The EMPTY
    // payload is what lets Postgres collapse them; this is why the signal deliberately carries no
    // position.
    let mut tx = pool.begin().await.expect("begin");
    for _ in 0..3 {
        sqlx::query("SELECT pg_notify($1, '')")
            .bind("domain_events")
            .execute(&mut *tx)
            .await
            .expect("pg_notify");
    }
    tx.commit().await.expect("commit");

    // First wake: the append.
    tokio::time::timeout(WAKE_TIMEOUT, waiter.wait(Duration::from_secs(60)))
        .await
        .expect("the append must wake the drain loop");
    // And then silence — the other two were coalesced away rather than queued as extra drains.
    let woke_again = tokio::time::timeout(SILENCE_WINDOW, waiter.wait(Duration::from_secs(60)))
        .await
        .is_ok();
    assert!(!woke_again, "identical notifies in one transaction must coalesce into a single wake");
}

#[tokio::test]
async fn both_drain_loops_wake_on_the_same_append() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP both_drain_loops_wake_on_the_same_append: DATABASE_URL not set");
        return;
    };
    let _guard = db_lock().lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    let wake = wake_listening(&url).await;

    // The projector and the saga runner share ONE listener; an append must reach both, or the money
    // path silently keeps its old latency while only projections get faster.
    let mut projector = wake.waiter();
    let mut saga = wake.waiter();
    projector.wait(SILENCE_WINDOW).await;
    saga.wait(SILENCE_WINDOW).await;

    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT pg_notify($1, '')")
        .bind("domain_events")
        .execute(&mut *tx)
        .await
        .expect("pg_notify");
    tx.commit().await.expect("commit");

    tokio::time::timeout(WAKE_TIMEOUT, projector.wait(Duration::from_secs(60)))
        .await
        .expect("projection worker must wake");
    tokio::time::timeout(WAKE_TIMEOUT, saga.wait(Duration::from_secs(60)))
        .await
        .expect("saga runner must wake");
}
