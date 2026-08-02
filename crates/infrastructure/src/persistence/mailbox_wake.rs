//! Push-based wake for the ACTOR MAILBOX (PROP-20260802-223522, ADR-20260802-224532): the sibling
//! of [`super::event_wake`] for `inbound_messages`.
//!
//! **Why.** Every mailbox enqueue already raises `pg_notify('inbound_messages', actor_type)`
//! inside its own transaction (the `PgMailbox` door and the PM chain hop). In-process producers
//! additionally nudge their worker directly — but a `tokio::sync::Notify` cannot cross processes,
//! so a standalone adapter's recorded webhook fact (a Stripe capture on the money path) used to
//! wait out the monolith's heartbeat, up to 10 s. This listener closes that gap: one dedicated
//! `LISTEN` connection per consuming process fans each notification out to the SAME per-actor-type
//! nudge the in-process path uses — the workers cannot tell (and need not care) which side woke
//! them.
//!
//! **Payload = the actor type, deliberately** (D2): Postgres deduplicates identical
//! (channel, payload) notifications within a transaction, so a multi-row enqueue coalesces per
//! actor type, and only that type's worker wakes — no thundering herd across the 16 types.
//!
//! **A missed notification is safe** — `inbound_messages` is the durable queue and
//! `status = 'RECEIVED'` is exactly the undelivered set, so the workers' safety-net pass catches
//! anything push lost. Four guards keep the window small: a freshly (re)connected listener
//! nudges EVERY registered actor type once (there is no replay, so assume the worst); sqlx's
//! silent in-place reconnect (`try_recv() -> Ok(None)`) triggers the same catch-up nudge-all;
//! every DOWN transition nudges them all again; and [`MailboxPush::set_live`] drives the workers
//! back to the heartbeat cadence while the connection is down (`MailboxWorker::with_push_live`).
//!
//! **Liveness is canary-verified, never assumed** (#314 review MAJOR-1). A transaction-mode
//! pooler accepts `LISTEN` and then silently delivers nothing — `recv()` never errors, so a
//! connection-error-driven flag would stay `true` forever while every wake is lost and the
//! paid-order path quietly runs on the stretched safety net. The listener therefore notifies
//! ITSELF on the channel every [`CANARY_INTERVAL`] (payload [`CANARY_PAYLOAD`], sent through the
//! pool — NOTIFY is server-wide, so any backend reaches a working listener) and requires the
//! echo before the next tick; a missed echo drops the connection, marks push down
//! (`mailbox_push_down_total{reason="canary_timeout"}`) and reconnects. Detection is bounded by
//! two canary intervals. Any process's canary satisfies any listener — receiving one at all
//! proves THIS connection's LISTEN is delivering.
//!
//! **Session-mode connection required**, same hard constraint as `event_wake`
//! (ADR-20260802-200416): `LISTEN` silently delivers nothing through a transaction-mode pooler —
//! with the canary, that now degrades loudly to the heartbeat cadence instead of silently to
//! the stretched one.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use sqlx::postgres::PgListener;
use sqlx::PgPool;

use super::mailbox_store::MailboxNudges;

/// The Postgres channel the mailbox door notifies and [`spawn_mailbox_listener`] listens on.
pub const MAILBOX_CHANNEL: &str = "inbound_messages";

/// The liveness self-check payload. Not an actor type, so a foreign listener's nudge lookup is a
/// no-op; any listener receiving it (its own or a peer's) has just proven its LISTEN delivers.
pub const CANARY_PAYLOAD: &str = "__canary__";

/// How often the listener proves its own LISTEN still delivers (see module doc). Detection of a
/// silently-deaf connection is bounded by two intervals.
const CANARY_INTERVAL: Duration = Duration::from_secs(30);

/// Reconnect backoff bounds for the listener connection.
const RECONNECT_MIN: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(30);

/// The listener-liveness flag shared with every mailbox worker
/// (`MailboxWorker::with_push_live`): `true` only while a `LISTEN` connection is established.
#[derive(Clone, Default)]
pub struct MailboxPush {
    live: Arc<AtomicBool>,
}

impl MailboxPush {
    pub fn new() -> Self {
        Self::default()
    }

    /// The shared flag itself, for handing to workers.
    pub fn live_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.live)
    }

    pub fn set_live(&self, live: bool) {
        self.live.store(live, Ordering::Relaxed);
    }

    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::Relaxed)
    }
}

/// Spawn the dedicated `LISTEN` connection fanning `inbound_messages` notifications out to the
/// per-actor-type nudges, reconnecting forever with bounded backoff. Takes its own connection
/// (NOT one of the pool's) because a listening session is held open indefinitely and must not
/// occupy a slot the query path needs; the `pool` is used ONLY to send the liveness canary.
///
/// Every (re)connection nudges EVERY registered actor type once before parking: notifications
/// have no replay, and anything enqueued while the listener was down must not wait out the
/// safety net. Every DOWN transition nudges them all again for the same reason.
pub fn spawn_mailbox_listener(
    database_url: String,
    pool: PgPool,
    nudges: Arc<MailboxNudges>,
    push: MailboxPush,
) {
    spawn_mailbox_listener_with(database_url, pool, nudges, push, CANARY_INTERVAL)
}

/// [`spawn_mailbox_listener`] with the canary interval exposed — tests shrink it to prove the
/// canary round-trips without waiting out the production 30 s.
pub fn spawn_mailbox_listener_with(
    database_url: String,
    pool: PgPool,
    nudges: Arc<MailboxNudges>,
    push: MailboxPush,
    canary_interval: Duration,
) {
    tokio::spawn(async move {
        let mut backoff = RECONNECT_MIN;
        loop {
            match PgListener::connect(&database_url).await {
                Ok(mut listener) => match listener.listen(MAILBOX_CHANNEL).await {
                    Ok(()) => {
                        push.set_live(true);
                        // Catch up on whatever landed while we were not listening.
                        nudges.nudge_all();
                        backoff = RECONNECT_MIN;
                        tracing::info!(
                            channel = MAILBOX_CHANNEL,
                            "mailbox push: LISTEN established -- deliveries are push-driven"
                        );
                        let mut ticks = tokio::time::interval(canary_interval);
                        ticks.set_missed_tick_behavior(
                            tokio::time::MissedTickBehavior::Delay,
                        );
                        ticks.tick().await; // the interval's immediate first tick
                        let mut awaiting_echo = false;
                        let down_reason = loop {
                            tokio::select! {
                                // try_recv, NOT recv: sqlx's recv() reconnects INTERNALLY on a
                                // lost connection and silently drops whatever was notified in
                                // the gap — the exact no-replay window this listener exists to
                                // close. try_recv surfaces that heal as Ok(None), our cue to
                                // catch up with a nudge-all.
                                r = listener.try_recv() => match r {
                                    Ok(Some(n)) if n.payload() == CANARY_PAYLOAD => {
                                        // Ours or a peer's — either way THIS connection's LISTEN
                                        // just demonstrably delivered.
                                        awaiting_echo = false;
                                    }
                                    Ok(Some(n)) => {
                                        let actor_type = n.payload();
                                        if actor_type.is_empty() {
                                            // Defensive: an empty payload names nobody — wake all.
                                            nudges.nudge_all();
                                        } else {
                                            nudges.nudge(actor_type);
                                        }
                                    }
                                    Ok(None) => {
                                        // The connection died and sqlx re-established it under
                                        // us: LISTEN is armed again but the gap's notifications
                                        // are gone — drain everything, and give the in-flight
                                        // canary the benefit of the doubt (it may have died
                                        // with the old connection).
                                        tracing::warn!(
                                            "mailbox push: connection healed after a drop -- catching up with a full nudge"
                                        );
                                        telemetry::meters::mailbox::push_down("connection_healed");
                                        nudges.nudge_all();
                                        awaiting_echo = false;
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            error = %e,
                                            "mailbox push: listener dropped -- workers fall back to heartbeat cadence, reconnecting"
                                        );
                                        break "connection_lost";
                                    }
                                },
                                _ = ticks.tick() => {
                                    if awaiting_echo {
                                        // The previous canary never came back: LISTEN is deaf
                                        // (the transaction-pooler mode) even though recv() is
                                        // happy. Tear down and reconnect rather than silently
                                        // stretching the money path's delivery cadence.
                                        tracing::warn!(
                                            "mailbox push: liveness canary missed -- LISTEN is not delivering, reconnecting"
                                        );
                                        break "canary_timeout";
                                    }
                                    awaiting_echo = true;
                                    if let Err(e) = sqlx::query("SELECT pg_notify($1, $2)")
                                        .bind(MAILBOX_CHANNEL)
                                        .bind(CANARY_PAYLOAD)
                                        .execute(&pool)
                                        .await
                                    {
                                        // Can't SEND the canary (pool trouble) — that is not
                                        // evidence LISTEN is deaf; skip this round rather than
                                        // condemning a healthy connection.
                                        tracing::warn!(error = %e, "mailbox push: canary send failed -- skipping this liveness round");
                                        awaiting_echo = false;
                                    }
                                }
                            }
                        };
                        telemetry::meters::mailbox::push_down(down_reason);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, channel = MAILBOX_CHANNEL, "mailbox push: LISTEN failed")
                    }
                },
                Err(e) => tracing::warn!(error = %e, "mailbox push: listener connect failed"),
            }
            // Down: workers revert to the heartbeat cadence until we are back — and whatever
            // was enqueued during the collapse drains now instead of waiting a stretched pass.
            push.set_live(false);
            nudges.nudge_all();
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(RECONNECT_MAX);
        }
    });
}
