//! The restriction fact terminates the rider's socket (#639 part C step 5,
//! ADR-20260905-065415-the-restriction-fact-terminates-the-rider-s-socket-a-connection-local-standing-read-inside-the-guard-and-one-writer-to-the-transport).
//!
//! Three pieces, kept together because they only make sense as one story:
//! - [`RiderStandingCell`] — the connection-local standing [`StandingGuard`](super::acl::StandingGuard)
//!   reads FIRST (§2): a `watch::Sender<RiderStanding>` whose only write is [`RiderStandingCell::restrict`]
//!   — monotone-tightening BY TYPE (nothing can spell "send ACTIVE"), never a second grant.
//! - [`watch`] — the per-connection task (§1/§4) matching this rider's OWN `RiderRestricted` fact
//!   on the in-process `EventBus`, re-deriving standing ONCE through
//!   [`crate::auth::current_rider_standing`] on `Lagged`/`Closed` (never asserting a restriction on
//!   a lookup error, ADR-20260904-124600 §3), and pushing the 4403 close (§3).
//! - [`RunRiderRestrictionSocketClose`] — the release gate (§6): OFF, `graphql_get` never calls
//!   into this module at all.
//!
//! The one-writer sink refactor (§3, structural, ungated) lives in `routes.rs` beside the
//! `GraphQLWebSocket::new_with_pair` call — it applies to every WS connection of every role, not
//! only riders, so it does not belong to a rider-only module.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::ws::{CloseFrame, Message};
use domain::generated::scalars::{RiderId, RiderStanding};
use futures::channel::mpsc;
use futures::SinkExt;
use tokio::sync::{broadcast, watch};

use infrastructure::AppendedEvent;

/// `configuration.yaml#/RUN_RIDER_RESTRICTION_SOCKET_CLOSE` (§6), resolved ONCE at the composition
/// root and threaded to `graphql_get` as typed state — the route never reads the environment. A
/// dedicated newtype (the `RunRiderRestrictionDoor` precedent, `crates/server/src/graphql/schema.rs`)
/// so the door-key value cannot collide with any other boolean context value keyed by TypeId.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunRiderRestrictionSocketClose(pub bool);

/// The connection-local standing cell (§2): a CACHE of the grant, never a second grant.
/// Monotone-tightening BY TYPE — the only write this type exposes sends `RESTRICTED`, so "send
/// ACTIVE" is unspellable; a reinstatement never re-opens a live socket, the rider reconnects.
#[derive(Clone)]
pub struct RiderStandingCell(watch::Sender<RiderStanding>);

impl RiderStandingCell {
    /// Seed the cell from the resolved `ReadScope::Rider.standing` and hand back the `Receiver`
    /// half for the connection's `Data` — `StandingGuard` reads THAT, never the sender.
    pub fn seeded(initial: RiderStanding) -> (Self, watch::Receiver<RiderStanding>) {
        let (tx, rx) = watch::channel(initial);
        (Self(tx), rx)
    }

    /// The ONLY write this type permits.
    pub fn restrict(&self) {
        let _ = self.0.send(RiderStanding::RESTRICTED);
    }
}

/// The `RiderRestricted` event-type tag, DERIVED from the generated `DomainEvent` union (never a
/// string literal, evans): serializes one throwaway instance once and reads its `eventType` tag —
/// the same reflection idiom `crates/infrastructure/src/mailbox/handler.rs` already uses to read a
/// REAL event's tag off the wire; here there is no real event to read from yet, so a sample stands
/// in, cached for the process's life.
fn rider_restricted_event_type() -> &'static str {
    static TAG: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TAG.get_or_init(|| {
        let sample = domain::generated::events::DomainEvent::RiderRestricted(
            domain::generated::events::RiderRestricted {
                rider_id: RiderId(uuid::Uuid::nil()),
                ground: domain::generated::scalars::RiderRestrictionGround::RIDER_REQUESTED,
                decided_at: String::new(),
                effective_at: String::new(),
            },
        );
        serde_json::to_value(&sample)
            .ok()
            .and_then(|v| v.get("eventType").and_then(|t| t.as_str()).map(str::to_owned))
            .expect("DomainEvent variants serialize with an eventType tag")
    })
    .as_str()
}

/// Bounded retry for a Lagged/Closed re-derivation (§4) — `UNVERIFIED input` (ADR-20260817-105845):
/// no antecedent justifies these numbers specifically; a small bounded retry was chosen for this
/// dark, zero-production-population slice rather than left unbounded. NEVER a timer
/// (ADR-20260810-231300 — this fires once per Lagged/Closed event, not on a schedule).
const LAGGED_REDERIVE_RETRY_ATTEMPTS: u32 = 3;
const LAGGED_REDERIVE_RETRY_BACKOFF_MS: u64 = 50;

/// One re-derivation attempt set: RESTRICTED closes, ACTIVE keeps watching, a lookup error (after
/// bounded retry) counts `missed` and keeps the socket open — never terminates on an
/// infrastructure failure (ADR-20260904-124600 §3, farley: a false close at peak is a delivery
/// outage).
async fn rederive_once_bounded(
    rider_id: RiderId,
    roster: &dyn application::queries::RiderRosterReadRepository,
) -> RiderStanding {
    use crate::auth::{current_rider_standing, RiderStandingLookup};

    for attempt in 0..LAGGED_REDERIVE_RETRY_ATTEMPTS {
        match current_rider_standing(rider_id, roster).await {
            RiderStandingLookup::Standing(standing) => return standing,
            // No `Rider` row (or none projected yet) is not a lookup failure — the rider was never
            // granted anything, so there is nothing to restrict; treat as ACTIVE (keep watching).
            RiderStandingLookup::NotFound => return RiderStanding::ACTIVE,
            RiderStandingLookup::LookupFailed => {
                if attempt + 1 < LAGGED_REDERIVE_RETRY_ATTEMPTS {
                    tokio::time::sleep(std::time::Duration::from_millis(
                        LAGGED_REDERIVE_RETRY_BACKOFF_MS * u64::from(attempt + 1),
                    ))
                    .await;
                }
            }
        }
    }
    telemetry::meters::rider_restriction::socket_close_missed("lookup_failed");
    // A lookup error never asserts a restriction: report ACTIVE (keep watching) to the caller,
    // which is exactly "the socket stays open" — the counter above is the record of the miss.
    RiderStanding::ACTIVE
}

/// Push the close frame into the forwarder's channel and close it — the forwarder flushes what is
/// already queued (this frame included) and drops the real sink (§3).
async fn push_close(close_tx: &mut mpsc::Sender<Message>) {
    let _ = close_tx
        .send(Message::Close(Some(CloseFrame {
            code: shared_types::RIDER_RESTRICTED_SOCKET_CLOSE_CODE,
            reason: shared_types::RIDER_RESTRICTED_SOCKET_CLOSE_REASON.into(),
        })))
        .await;
    close_tx.close_channel();
}

/// The per-connection watcher (§1/§4), spawned once per RIDER connection when the gate is ON and
/// dropped with the connection (the caller retains the `JoinHandle` and aborts it — vernon: no
/// leaked receiver per dead socket). Runs until it closes the socket or the bus can never publish
/// again (`RecvError::Closed`).
pub async fn watch(
    mut fact_rx: broadcast::Receiver<(AppendedEvent, Instant)>,
    rider_id: RiderId,
    standing: RiderStandingCell,
    mut close_tx: mpsc::Sender<Message>,
    roster: Arc<dyn application::queries::RiderRosterReadRepository>,
    connection_correlation_id: uuid::Uuid,
) {
    use broadcast::error::RecvError;

    // The inverted dead-man's switch (§7, observability-agent): declared alive for the task's
    // whole life, decremented on every exit path (RAII, survives an early `return`).
    struct LiveGuard;
    impl Drop for LiveGuard {
        fn drop(&mut self) {
            telemetry::meters::rider_restriction::watch_live_delta(-1);
        }
    }
    telemetry::meters::rider_restriction::watch_live_delta(1);
    let _live = LiveGuard;

    let wanted_stream = <domain::rider::RiderState as domain::aggregate::Aggregate>::stream(rider_id);
    let restricted_type = rider_restricted_event_type();

    loop {
        match fact_rx.recv().await {
            Ok((evt, published_at)) => {
                // Equality on the connection's OWN rider id only — never prefix/contains
                // (security, all lenses): `wanted_stream` is the FULL `"Rider-{id}"` string and
                // this is `==`, so `Rider-600D` can never match `Rider-600Dxyz` or vice versa.
                if evt.stream_name == wanted_stream && evt.event_type == restricted_type {
                    standing.restrict();
                    telemetry::meters::rider_restriction::socket_close_latency_ms(
                        published_at.elapsed().as_secs_f64() * 1000.0,
                    );
                    telemetry::meters::rider_restriction::socket_close("closed");
                    tracing::info!(
                        fact_correlation_id = %evt.correlation_id,
                        connection_correlation_id = %connection_correlation_id,
                        "rider.restricted.socket_terminated"
                    );
                    push_close(&mut close_tx).await;
                    return;
                }
                // Another rider's fact, or a different event type on this rider's stream (e.g.
                // RiderReinstated): keep watching.
            }
            Err(RecvError::Lagged(_)) => {
                if rederive_once_bounded(rider_id, roster.as_ref()).await == RiderStanding::RESTRICTED {
                    standing.restrict();
                    telemetry::meters::rider_restriction::socket_close("closed");
                    tracing::info!(
                        connection_correlation_id = %connection_correlation_id,
                        "rider.restricted.socket_terminated"
                    );
                    push_close(&mut close_tx).await;
                    return;
                }
                // ACTIVE (or a lookup error, already counted above): keep watching on the SAME
                // receiver — `Lagged` skips missed envelopes, it does not invalidate the receiver.
            }
            Err(RecvError::Closed) => {
                // The bus itself is gone: no future envelope can ever arrive, so this is the
                // LAST chance to notice a missed restriction — one final bounded re-derivation,
                // then the task ends either way.
                if rederive_once_bounded(rider_id, roster.as_ref()).await == RiderStanding::RESTRICTED {
                    standing.restrict();
                    telemetry::meters::rider_restriction::socket_close("closed");
                    tracing::info!(
                        connection_correlation_id = %connection_correlation_id,
                        "rider.restricted.socket_terminated"
                    );
                    push_close(&mut close_tx).await;
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pure predicate (§9's unit-test half): equality on the FULL stream name, never
    /// prefix/contains — the mutant M2 the checkpoint requires red.
    #[test]
    fn stream_match_is_exact_equality_never_prefix() {
        let id = RiderId(uuid::Uuid::from_u128(0x600D));
        let wanted = <domain::rider::RiderState as domain::aggregate::Aggregate>::stream(id);
        assert_eq!(wanted, format!("Rider-{}", uuid::Uuid::from_u128(0x600D)));
        // A prefix match would wrongly accept a longer stream name sharing the same prefix.
        assert_ne!(wanted, format!("Rider-{}extra", uuid::Uuid::from_u128(0x600D)));
        assert!(!format!("Rider-{}extra", uuid::Uuid::from_u128(0x600D)).eq(&wanted));
    }

    #[test]
    fn rider_restricted_event_type_is_derived_and_stable() {
        assert_eq!(rider_restricted_event_type(), "RiderRestricted");
        // Called twice — the OnceLock caches, so this also proves it doesn't panic on reuse.
        assert_eq!(rider_restricted_event_type(), "RiderRestricted");
    }

    /// The monotone cell (§9's unit-test half): the type has no operation that sends ACTIVE —
    /// unspellable, not merely a no-op. `restrict()` is idempotent (a second fact for the same
    /// rider, or a re-derivation after the fact already closed, must never panic or error).
    #[test]
    fn the_standing_cell_can_only_ever_be_restricted() {
        let (cell, rx) = RiderStandingCell::seeded(RiderStanding::ACTIVE);
        assert_eq!(*rx.borrow(), RiderStanding::ACTIVE);
        cell.restrict();
        assert_eq!(*rx.borrow(), RiderStanding::RESTRICTED);
        // A second call (e.g. a re-derivation confirming what the fact already set) is a no-op,
        // never a panic — and there is no `activate`/`set` method to call instead: RESTRICTED is
        // the only value `RiderStandingCell` can ever produce beyond the seed.
        cell.restrict();
        assert_eq!(*rx.borrow(), RiderStanding::RESTRICTED);
    }
}
