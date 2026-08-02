//! Push-based wake for the drain loops: Postgres `NOTIFY` replaces the 1.5-second poll as the
//! PRIMARY signal that `domain_events` has moved.
//!
//! **Why.** The projection worker and the saga runner each polled every 1.5 s, and each pass costs
//! `1 + 2 x GROUPS` queries — together ~70,900 queries an hour on a completely idle platform. Every
//! one is an outbound round trip to Supabase, which made background polling cost an order of
//! magnitude more egress than all customer-facing traffic combined. Pushing the wake signal instead
//! collapses that to the safety-net cadence AND cuts the reaction time on the money path from
//! "up to 1.5 s" to "as fast as the commit lands".
//!
//! **How.** [`super::event_store::PgEventStore::append`] issues `pg_notify` INSIDE the append
//! transaction, so the signal is delivered to listeners at COMMIT and a rolled-back append signals
//! nobody. [`spawn_event_listener`] holds one dedicated `LISTEN` connection and turns each
//! notification into an [`EventWake::signal`], which every parked drain loop observes.
//!
//! **The signal carries no data, deliberately.** Payload is empty so Postgres coalesces the
//! duplicates a multi-event append would otherwise produce into a single wake; the drains re-read
//! from their own `projection_checkpoint` row regardless, so there is nothing to transmit.
//!
//! **A missed notification is safe, and that is the whole design.** `NOTIFY` is fire-and-forget with
//! no replay: a signal that arrives while the listener is reconnecting is simply gone. That cannot
//! lose work here because `domain_events` IS the durable queue and `projection_checkpoint` is the
//! consumer offset — the next drain reads from the checkpoint and catches up. Two guards keep the
//! window small:
//!
//! - every loop still drains on its own safety-net interval (slow while push is confirmed live), and
//! - a freshly (re)connected listener [`EventWake::signal`]s once unconditionally, so whatever
//!   landed while it was down is drained immediately rather than waiting for the safety net.
//!
//! **Push is never assumed.** [`EventWaiter::safety_interval`] reports the FAST interval whenever the
//! listener is down, so losing push (a dropped connection, or a deployment moved onto a pooler that
//! cannot do `LISTEN`) degrades to exactly today's polling behaviour rather than to minute-late
//! projections.
//!
//! **Session-mode connection required.** `LISTEN` needs a session that survives between statements:
//! it works on Supabase's *session* pooler (port 5432, what `render.yaml` specifies) and on a direct
//! connection, and silently delivers nothing through a *transaction* pooler (6543). This is a hard
//! deployment constraint, not a tuning knob — see the ADR.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use sqlx::postgres::PgListener;
use tokio::sync::watch;

/// The Postgres channel the event store notifies and [`spawn_event_listener`] listens on.
pub const EVENT_CHANNEL: &str = "domain_events";

/// Reconnect backoff bounds for the listener connection.
const RECONNECT_MIN: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(30);

/// The publish side of the wake: cloneable, cheap, and safe to signal with no listeners attached.
///
/// Implemented over `tokio::sync::watch` rather than `Notify` on purpose — a `watch` receiver
/// records the value it last observed, so a signal raised while a loop was busy draining (rather
/// than parked) is still seen on its next wait. With `Notify::notify_waiters` that wake would be
/// lost, which is exactly the race a drain loop spends most of its time in.
#[derive(Clone)]
pub struct EventWake {
    tx: watch::Sender<u64>,
    live: Arc<AtomicBool>,
}

impl Default for EventWake {
    fn default() -> Self {
        Self::new()
    }
}

impl EventWake {
    pub fn new() -> Self {
        let (tx, _rx) = watch::channel(0);
        Self { tx, live: Arc::new(AtomicBool::new(false)) }
    }

    /// Signal that the log has moved. Never blocks and never fails, including with zero waiters.
    pub fn signal(&self) {
        self.tx.send_modify(|n| *n = n.wrapping_add(1));
    }

    /// Mark the listener connection up or down — drives [`EventWaiter::safety_interval`].
    pub fn set_live(&self, live: bool) {
        self.live.store(live, Ordering::Relaxed);
    }

    /// Whether a `LISTEN` connection is currently established.
    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::Relaxed)
    }

    /// A waiter for one drain loop. Every waiter observes every signal.
    pub fn waiter(&self) -> EventWaiter {
        EventWaiter { rx: self.tx.subscribe(), live: Arc::clone(&self.live) }
    }
}

/// The consume side of the wake, held by one drain loop.
pub struct EventWaiter {
    rx: watch::Receiver<u64>,
    live: Arc<AtomicBool>,
}

impl EventWaiter {
    /// How long this loop may park before draining anyway: `pushed` while the listener is up (the
    /// safety net behind push), `polled` while it is down (today's unassisted cadence).
    pub fn safety_interval(&self, pushed: Duration, polled: Duration) -> Duration {
        if self.live.load(Ordering::Relaxed) {
            pushed
        } else {
            polled
        }
    }

    /// Park until the next signal or `timeout`, whichever comes first.
    pub async fn wait(&mut self, timeout: Duration) {
        match tokio::time::timeout(timeout, self.rx.changed()).await {
            // Signalled.
            Ok(Ok(())) => {}
            // The sender is gone (listener task dropped): hold the cadence instead of spinning on an
            // immediately-ready `changed()`.
            Ok(Err(_)) => tokio::time::sleep(timeout).await,
            // Safety net elapsed.
            Err(_) => {}
        }
    }
}

/// Spawn the dedicated `LISTEN` connection feeding `wake`, reconnecting forever with bounded
/// backoff. Takes its own connection (NOT one of the pool's) because a listening session is held
/// open indefinitely and must not occupy a slot the query path needs.
///
/// Every (re)connection signals once before parking: a listener that was down may have missed
/// notifications, and `NOTIFY` has no replay.
pub fn spawn_event_listener(database_url: String, wake: EventWake) {
    tokio::spawn(async move {
        let mut backoff = RECONNECT_MIN;
        loop {
            match PgListener::connect(&database_url).await {
                Ok(mut listener) => match listener.listen(EVENT_CHANNEL).await {
                    Ok(()) => {
                        wake.set_live(true);
                        // Drain whatever landed while we were not listening.
                        wake.signal();
                        backoff = RECONNECT_MIN;
                        println!(
                            "event push: LISTEN {EVENT_CHANNEL} established -- drains are push-driven"
                        );
                        loop {
                            match listener.recv().await {
                                Ok(_) => wake.signal(),
                                Err(e) => {
                                    eprintln!(
                                        "event push: listener dropped ({e}) -- drains fall back to polling, reconnecting"
                                    );
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!("event push: LISTEN {EVENT_CHANNEL} failed ({e})"),
                },
                Err(e) => eprintln!("event push: listener connect failed ({e})"),
            }
            // Down: the drain loops revert to their fast poll interval until we are back.
            wake.set_live(false);
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(RECONNECT_MAX);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAST: Duration = Duration::from_millis(1500);
    const SLOW: Duration = Duration::from_secs(60);

    #[test]
    fn a_fresh_wake_is_not_live_so_loops_keep_polling_fast() {
        let wake = EventWake::new();
        assert!(!wake.is_live());
        assert_eq!(wake.waiter().safety_interval(SLOW, FAST), FAST);
    }

    #[test]
    fn liveness_switches_the_safety_interval_both_ways() {
        let wake = EventWake::new();
        let waiter = wake.waiter();
        wake.set_live(true);
        assert_eq!(waiter.safety_interval(SLOW, FAST), SLOW, "push live -- park long");
        // A dropped listener must restore the fast cadence, not leave reads minutes stale.
        wake.set_live(false);
        assert_eq!(waiter.safety_interval(SLOW, FAST), FAST, "push down -- degrade to polling");
    }

    #[test]
    fn signalling_without_waiters_is_a_noop() {
        EventWake::new().signal();
    }

    #[tokio::test]
    async fn a_signal_wakes_a_parked_waiter_before_the_safety_net() {
        let wake = EventWake::new();
        let mut waiter = wake.waiter();
        tokio::spawn({
            let wake = wake.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                wake.signal();
            }
        });
        // The safety net is 30 s; returning promptly proves the push, not the timeout, woke us.
        tokio::time::timeout(Duration::from_secs(5), waiter.wait(Duration::from_secs(30)))
            .await
            .expect("the signal must wake the waiter well before the safety net");
    }

    #[tokio::test]
    async fn a_signal_raised_while_busy_is_not_lost() {
        // The race the design exists for: the log moves while the loop is draining, not parked.
        let wake = EventWake::new();
        let mut waiter = wake.waiter();
        wake.signal(); // raised BEFORE anyone waits
        tokio::time::timeout(Duration::from_secs(5), waiter.wait(Duration::from_secs(30)))
            .await
            .expect("a signal raised while the loop was busy must still be observed");
    }

    #[tokio::test]
    async fn every_waiter_sees_every_signal() {
        // Both drain loops must wake — not one of them.
        let wake = EventWake::new();
        let mut projector = wake.waiter();
        let mut saga = wake.waiter();
        wake.signal();
        tokio::time::timeout(Duration::from_secs(5), projector.wait(Duration::from_secs(30)))
            .await
            .expect("projector waiter");
        tokio::time::timeout(Duration::from_secs(5), saga.wait(Duration::from_secs(30)))
            .await
            .expect("saga waiter");
    }

    #[tokio::test]
    async fn wait_returns_on_the_safety_net_when_nothing_is_signalled() {
        let wake = EventWake::new();
        let mut waiter = wake.waiter();
        tokio::time::timeout(Duration::from_secs(5), waiter.wait(Duration::from_millis(20)))
            .await
            .expect("the safety net must release the loop even with push silent");
    }

    #[tokio::test]
    async fn wait_holds_its_cadence_after_the_sender_is_dropped() {
        // A dropped sender makes `changed()` ready immediately; the loop must not spin on it.
        let wake = EventWake::new();
        let mut waiter = wake.waiter();
        drop(wake);
        let started = tokio::time::Instant::now();
        waiter.wait(Duration::from_millis(50)).await;
        assert!(started.elapsed() >= Duration::from_millis(40), "must not busy-spin");
    }
}
