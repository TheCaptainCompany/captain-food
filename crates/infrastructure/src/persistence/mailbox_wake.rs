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
//! anything push lost. Two guards keep the window small: a freshly (re)connected listener nudges
//! EVERY registered actor type once (there is no replay, so assume the worst), and
//! [`MailboxPush::set_live`] drives the workers back to the heartbeat cadence the moment the
//! connection drops (`MailboxWorker::with_push_live`) — losing push degrades to exactly the
//! pre-push behaviour, never past it.
//!
//! **Session-mode connection required**, same hard constraint as `event_wake`
//! (ADR-20260802-200416): `LISTEN` silently delivers nothing through a transaction-mode pooler.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use sqlx::postgres::PgListener;

use super::mailbox_store::MailboxNudges;

/// The Postgres channel the mailbox door notifies and [`spawn_mailbox_listener`] listens on.
pub const MAILBOX_CHANNEL: &str = "inbound_messages";

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
/// occupy a slot the query path needs.
///
/// Every (re)connection nudges EVERY registered actor type once before parking: notifications
/// have no replay, and anything enqueued while the listener was down must not wait out the
/// safety net.
pub fn spawn_mailbox_listener(
    database_url: String,
    nudges: Arc<MailboxNudges>,
    push: MailboxPush,
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
                        loop {
                            match listener.recv().await {
                                Ok(n) => {
                                    let actor_type = n.payload();
                                    if actor_type.is_empty() {
                                        // Defensive: an empty payload names nobody — wake all.
                                        nudges.nudge_all();
                                    } else {
                                        nudges.nudge(actor_type);
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        error = %e,
                                        "mailbox push: listener dropped -- workers fall back to heartbeat cadence, reconnecting"
                                    );
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, channel = MAILBOX_CHANNEL, "mailbox push: LISTEN failed")
                    }
                },
                Err(e) => tracing::warn!(error = %e, "mailbox push: listener connect failed"),
            }
            // Down: workers revert to the heartbeat cadence until we are back.
            push.set_live(false);
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(RECONNECT_MAX);
        }
    });
}
