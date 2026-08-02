//! The per-actor-type mailbox worker: promote (due reminders) → claim → drain (head-of-line per
//! lane) → heartbeat, forever.
//! Single-step methods are public so tests (and the host's supervision tooling) can drive each
//! move deterministically; [`MailboxWorker::run`] is the production loop over them.

use std::sync::Arc;

use sqlx::{PgPool, Row};
use tokio::sync::Notify;

use crate::completion::{complete_fenced, CompletionError};
use crate::lease::{
    claim_due_lanes, heartbeat, ownership_census, release_lane, seed_partitions, steal_from, Lane,
};
use crate::message::{DeliveryObserver, InboundMessage, MessageHandler};

/// Tuning knobs — the host wires these from ITS configuration source (specs/configuration.yaml:
/// `MAILBOX_LEASE_SECONDS`, `MAILBOX_HEARTBEAT_SECONDS`, `MAILBOX_MAX_DELIVERY_ATTEMPTS`); the
/// defaults mirror the spec defaults.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// How far ahead a claim/renewal pushes `lease_until` (~30s).
    pub lease_seconds: i64,
    /// The pause between loop passes (~10s) — also the renewal cadence, so keep it well under
    /// `lease_seconds` (a lease must survive several missed beats before takeover).
    pub heartbeat_seconds: u64,
    /// Max rows drained from one lane per pass (backpressure bound).
    pub batch: i64,
    /// Max lanes claimed in one pass (spread bound — leaves claimable lanes for peers).
    pub max_claims_per_pass: i64,
    /// Poison bound (PROP-20260802-223522 D4): after this many delivery attempts whose completion
    /// TRANSACTION failed (infrastructure error — nothing was recorded on the row), the row flips
    /// to terminal FAILED with the error recorded and the lane's head-of-line unblocks. `0` = no
    /// cap (the pre-#313 infinite-retry behaviour, the rollback lever). Handler verdicts are
    /// terminal on their first attempt and never touch the counter.
    pub max_delivery_attempts: i16,
    /// Full-pass cadence while push is CONFIRMED live (see [`MailboxWorker::with_push_live`]):
    /// promotion + claim + drain stretch to this safety net, because enqueues wake the worker
    /// directly. Lease renewal never stretches — `beat` stays on `heartbeat_seconds`.
    pub pushed_pass_seconds: u64,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            lease_seconds: 30,
            heartbeat_seconds: 10,
            batch: 100,
            max_claims_per_pass: 100,
            max_delivery_attempts: 5,
            pushed_pass_seconds: 60,
        }
    }
}

/// Lane-lifecycle notifications — the seam a host hangs its activation cache on
/// (PROP-20260728-152752 §3.5 eviction rules: "lease lost / `ownership_version` bump observed →
/// drop ALL activations of that partition"). Called whenever this worker STOPS believing it owns
/// a lane: heartbeat renewal failed (stolen/re-claimed), a completion fenced out, or a graceful
/// shutdown release. Must be cheap and non-blocking — it runs on the worker loop.
pub trait LaneEvents: Send + Sync {
    fn lane_dropped(&self, lane: &Lane);
}

/// One worker instance consuming ONE actor type's lanes.
pub struct MailboxWorker {
    pool: PgPool,
    /// Stable per-process identity (`claimed_by`) — e.g. `"{hostname}-{pid}"`.
    pub worker_id: String,
    pub actor_type: String,
    config: WorkerConfig,
    handler: Arc<dyn MessageHandler>,
    observer: Option<Arc<dyn DeliveryObserver>>,
    /// Enqueue-side wake signal: a producer's `notify_one` cuts the delivery latency from the
    /// heartbeat poll to ~immediate. Purely an accelerator — the poll is the guarantee. Fed
    /// in-process by `PgMailbox` and (since #313) cross-process by the `inbound_messages`
    /// LISTEN connection, which nudges the same signal.
    nudge: Option<Arc<Notify>>,
    /// Whether a mailbox LISTEN connection is currently up (shared flag set by the listener,
    /// PROP-20260802-223522 D5). While `true`, the FULL pass (promotion + claim + drain)
    /// stretches to `pushed_pass_seconds` — enqueues wake us directly. While `false` (or when
    /// never wired), full passes run at `heartbeat_seconds`: exactly the pre-push cadence.
    push_live: Option<Arc<std::sync::atomic::AtomicBool>>,
    lane_events: Option<Arc<dyn LaneEvents>>,
    lanes: tokio::sync::Mutex<Vec<Lane>>,
}

impl MailboxWorker {
    pub fn new(
        pool: PgPool,
        worker_id: impl Into<String>,
        actor_type: impl Into<String>,
        config: WorkerConfig,
        handler: Arc<dyn MessageHandler>,
    ) -> Self {
        Self {
            pool,
            worker_id: worker_id.into(),
            actor_type: actor_type.into(),
            config,
            handler,
            observer: None,
            nudge: None,
            push_live: None,
            lane_events: None,
            lanes: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    /// Attach a lane-lifecycle observer (activation-cache eviction).
    pub fn with_lane_events(mut self, events: Arc<dyn LaneEvents>) -> Self {
        self.lane_events = Some(events);
        self
    }

    /// Attach a post-commit observer (status bus / subscription fan-out).
    pub fn with_observer(mut self, observer: Arc<dyn DeliveryObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    /// Attach an enqueue-side wake signal: producers `notify_one` after a successful insert so a
    /// fresh message is drained on the next pass instead of waiting out the heartbeat sleep.
    pub fn with_nudge(mut self, nudge: Arc<Notify>) -> Self {
        self.nudge = Some(nudge);
        self
    }

    /// Attach the listener-liveness flag (PROP-20260802-223522 D5): while it reads `true` the
    /// full pass stretches to `pushed_pass_seconds`; the moment it reads `false` the worker is
    /// back on the heartbeat cadence — losing push degrades to exactly the previous behaviour.
    pub fn with_push_live(mut self, live: Arc<std::sync::atomic::AtomicBool>) -> Self {
        self.push_live = Some(live);
        self
    }

    /// Idempotently seed this actor type's registry rows (run once at startup).
    pub async fn seed(&self, width: i16) -> sqlx::Result<()> {
        seed_partitions(&self.pool, &self.actor_type, width).await
    }

    /// Promote this actor type's due reminders (SCHEDULED → RECEIVED + a fresh position,
    /// [`crate::schedule::promote_due`]) so the SAME pass's drain can deliver them. Leaseless on
    /// purpose — see the function's doc.
    pub async fn promote(&self) -> sqlx::Result<u64> {
        crate::schedule::promote_due(&self.pool, &self.actor_type).await
    }

    /// Claim every claimable lane (up to the pass bound) and remember the authority.
    pub async fn claim(&self) -> sqlx::Result<usize> {
        let claimed = claim_due_lanes(
            &self.pool,
            &self.actor_type,
            &self.worker_id,
            self.config.lease_seconds,
            self.config.max_claims_per_pass,
        )
        .await?;
        let n = claimed.len();
        if n > 0 {
            tracing::info!(
                worker = %self.worker_id,
                actor_type = %self.actor_type,
                lanes = n,
                "mailbox: claimed lanes"
            );
        }
        self.lanes.lock().await.extend(claimed);
        Ok(n)
    }

    /// The FAIR-SHARE REBALANCE (PROP-20260728-152752 §3.1, deferred from the #270 review: a
    /// first instance claims every lane and renews forever, so a second instance would idle
    /// without this): while this worker holds fewer than `total / instances` lanes (floored) and
    /// some LIVE peer holds more than that share, steal ONE of the largest peer's lanes — then
    /// re-take the census and decide again, up to `max_claims_per_pass` steals per pass. Fresh
    /// census per steal + stop-at-the-floor is what makes this converge instead of thrash: a
    /// worker at its floor is never a thief, a worker above the floor is always the first
    /// victim, and no decision is ever made on stale counts. The victim keeps believing until
    /// its next heartbeat or completion, both of which fail on the bumped `ownership_version` —
    /// dual belief, never dual authority.
    ///
    /// Returns how many lanes were stolen. Only called when a pass claimed nothing — while
    /// UNCLAIMED lanes exist, [`Self::claim`] is the (cheaper, uncontended) path to fair.
    pub async fn rebalance(&self) -> sqlx::Result<u64> {
        let mut stolen = 0u64;
        while (stolen as i64) < self.config.max_claims_per_pass {
            let census = ownership_census(&self.pool, &self.actor_type, &self.worker_id).await?;
            let fair = census.fair_share();
            if census.mine >= fair {
                break;
            }
            let Some((victim, lanes)) = census.largest_other else {
                break;
            };
            if lanes <= fair {
                break;
            }
            let Some(lane) = steal_from(
                &self.pool,
                &self.actor_type,
                &victim,
                &self.worker_id,
                self.config.lease_seconds,
            )
            .await?
            else {
                // The victim's live lanes vanished under us (concurrent steal / expiry) — a
                // fresh census next iteration or next pass sorts it out.
                break;
            };
            tracing::info!(
                worker = %self.worker_id,
                actor_type = %self.actor_type,
                partition = lane.partition,
                from = %victim,
                mine = census.mine,
                fair,
                "mailbox: rebalance -- stole one lane from the largest owner"
            );
            self.lanes.lock().await.push(lane);
            stolen += 1;
        }
        Ok(stolen)
    }

    /// Renew every owned lease; DROP lanes whose renewal fails (stolen/re-claimed — in-flight
    /// work on them would be fenced out anyway, so forgetting them immediately is the correct
    /// reaction, Proto.Actor's topology-validity-token move).
    pub async fn beat(&self) -> sqlx::Result<()> {
        let mut lanes = self.lanes.lock().await;
        let mut kept = Vec::with_capacity(lanes.len());
        for lane in lanes.drain(..) {
            if heartbeat(&self.pool, &lane, &self.worker_id, self.config.lease_seconds).await? {
                kept.push(lane);
            } else {
                tracing::warn!(
                    worker = %self.worker_id,
                    actor_type = %lane.actor_type,
                    partition = lane.partition,
                    "mailbox: lane lost (stolen or re-claimed) -- dropping"
                );
                if let Some(events) = &self.lane_events {
                    events.lane_dropped(&lane);
                }
            }
        }
        *lanes = kept;
        Ok(())
    }

    /// The IDLE GATE (PROP-20260802-223522 D3): which of this actor type's partitions hold
    /// deliverable rows, in ONE query (served by the partial index `idx_inbound_messages_drain`).
    /// An idle pass costs this single round trip instead of one SELECT per owned lane; a row
    /// landing between this snapshot and the per-lane drain is caught by the next wake or the
    /// safety net — exactly the same window the pre-gate per-lane SELECT had after ITS read.
    async fn lanes_with_work(&self) -> sqlx::Result<std::collections::HashSet<i16>> {
        let rows = sqlx::query(
            "SELECT DISTINCT partition FROM inbound_messages \
             WHERE actor_type = $1 AND status = 'RECEIVED'",
        )
        .bind(&self.actor_type)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(|r| r.try_get::<i16, _>("partition")).collect::<Result<_, _>>()
    }

    /// Drain one pass over every owned lane that HAS WORK (the [`Self::lanes_with_work`] gate),
    /// head-of-line per lane. Returns delivered count.
    /// A fenced-out / already-completed delivery drops the lane on the spot (the authority is
    /// gone); an infrastructure error stops THAT lane's pass and moves on to the next lane —
    /// one failing lane (a poisoned head row, a data-dependent flush error) must never starve
    /// the worker's other lanes; the failing lane retries next pass.
    pub async fn drain(&self) -> sqlx::Result<u64> {
        let working = self.lanes_with_work().await?;
        if working.is_empty() {
            return Ok(0);
        }
        let lanes: Vec<Lane> = {
            let owned = self.lanes.lock().await;
            owned.iter().filter(|l| working.contains(&l.partition)).cloned().collect()
        };
        let mut delivered = 0u64;
        for lane in lanes {
            match self.drain_lane(&lane).await {
                Ok(n) => delivered += n,
                Err(CompletionError::FencedOut) | Err(CompletionError::AlreadyCompleted) => {
                    // Drop exactly the authority that was fenced — never a fresher claim of the
                    // same partition a concurrent claim() may have added.
                    self.lanes.lock().await.retain(|l| {
                        !(l.actor_type == lane.actor_type
                            && l.partition == lane.partition
                            && l.ownership_version == lane.ownership_version)
                    });
                    if let Some(events) = &self.lane_events {
                        events.lane_dropped(&lane);
                    }
                }
                Err(CompletionError::Db(e)) => {
                    tracing::warn!(
                        worker = %self.worker_id,
                        actor_type = %lane.actor_type,
                        partition = lane.partition,
                        error = %e,
                        "mailbox: lane drain failed -- retrying next pass"
                    );
                }
            }
        }
        Ok(delivered)
    }

    /// Head-of-line drain of ONE lane: RECEIVED rows strictly in `position` order, each committed
    /// through [`complete_fenced`] before the next is looked at.
    ///
    /// The drain filters on `status = 'RECEIVED'` ALONE — never `position > checkpoint`. Positions
    /// are sequence-allocated at INSERT, and Postgres commits are not sequence-ordered: a row can
    /// become visible with a position BELOW one already delivered, so a checkpoint-filtered drain
    /// would hide it from every future owner the moment the lane changes hands. The status flip is
    /// transactional with the delivery, so RECEIVED is exactly the set of undelivered rows; the
    /// checkpoint is a monotonic high-water mark (supervision + the fence's write target), not a
    /// consumption cursor.
    pub async fn drain_lane(&self, lane: &Lane) -> Result<u64, CompletionError> {
        let mut delivered = 0u64;
        loop {
            let rows = sqlx::query(
                "SELECT message_id, position, kind, actor_type, actor_id, partition, message_type, \
                        payload, payload_hash, channel, user_id, user_type, correlation_id, cause_id, \
                        session_id, received_at \
                 FROM inbound_messages \
                 WHERE actor_type = $1 AND partition = $2 AND status = 'RECEIVED' \
                 ORDER BY position \
                 LIMIT $3",
            )
            .bind(&lane.actor_type)
            .bind(lane.partition)
            .bind(self.config.batch)
            .fetch_all(&self.pool)
            .await
            .map_err(CompletionError::Db)?;
            if rows.is_empty() {
                return Ok(delivered);
            }
            for row in &rows {
                let message = InboundMessage::decode(row).map_err(CompletionError::Db)?;
                let verdict = match complete_fenced(
                    &self.pool,
                    lane,
                    &self.worker_id,
                    &message,
                    self.handler.as_ref(),
                )
                .await
                {
                    Ok(v) => v,
                    Err(CompletionError::Db(e)) => {
                        // POISON PATH (PROP-20260802-223522 D4): the completion TRANSACTION
                        // failed, so the row itself recorded nothing. Count the attempt in its
                        // own statement (outside the aborted tx); at the cap, flip the row to
                        // terminal FAILED and keep draining — the lane's head-of-line unblocks.
                        // Below the cap, stop THIS lane's pass (head-of-line: never deliver
                        // past an undelivered row) and retry on the next wake.
                        if self.poison(&message, &e).await? {
                            continue;
                        }
                        return Err(CompletionError::Db(e));
                    }
                    Err(other) => return Err(other),
                };
                if let Some(obs) = &self.observer {
                    obs.committed(&message, &verdict);
                }
                tracing::debug!(
                    worker = %self.worker_id,
                    actor_type = %message.actor_type,
                    partition = message.partition,
                    position = message.position,
                    message_type = %message.message_type,
                    verdict = verdict.status(),
                    "mailbox: delivered"
                );
                delivered += 1;
            }
            if (rows.len() as i64) < self.config.batch {
                return Ok(delivered);
            }
            // A full batch means a backlog deeper than one pass. Renew THIS lane's lease before
            // the next batch — an unbounded drain that never heartbeats would expire mid-pass
            // under any backlog worth more than `lease_seconds` of handler time, and every
            // completion after the takeover would run the full handler only to fence out.
            let renewed =
                heartbeat(&self.pool, lane, &self.worker_id, self.config.lease_seconds).await?;
            if !renewed {
                return Err(CompletionError::FencedOut);
            }
        }
    }

    /// Record one failed delivery ATTEMPT and, at the configured cap, flip the row to terminal
    /// FAILED with the error recorded (its only evidence — the aborted completion transaction
    /// wrote nothing). Returns `true` when the row was flipped (the lane may continue past it).
    /// `max_delivery_attempts == 0` disables the cap: attempts are still counted (diagnostic),
    /// the row never flips — the pre-#313 behaviour. The status predicate keeps both statements
    /// no-ops if a concurrent owner completed the row meanwhile. No observer callback on the
    /// flip: there is no handler verdict, and the status bus is best-effort — the row is the
    /// record.
    async fn poison(&self, message: &InboundMessage, err: &sqlx::Error) -> Result<bool, CompletionError> {
        let attempts: i16 = sqlx::query_scalar(
            "UPDATE inbound_messages SET attempts = attempts + 1 \
             WHERE message_id = $1 AND status = 'RECEIVED' \
             RETURNING attempts",
        )
        .bind(message.message_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(CompletionError::Db)?
        .unwrap_or(0);
        let cap = self.config.max_delivery_attempts;
        if cap <= 0 || attempts < cap {
            tracing::warn!(
                worker = %self.worker_id,
                actor_type = %message.actor_type,
                partition = message.partition,
                message_type = %message.message_type,
                attempts,
                cap,
                error = %err,
                "mailbox: delivery attempt failed -- will retry"
            );
            return Ok(false);
        }
        let flipped = sqlx::query(
            "UPDATE inbound_messages \
             SET status = 'FAILED', error = $2, completed_at = now() \
             WHERE message_id = $1 AND status = 'RECEIVED'",
        )
        .bind(message.message_id)
        .bind(serde_json::json!({
            "code": "DeliveryInfrastructureError",
            "context": { "detail": err.to_string(), "attempts": attempts },
        }))
        .execute(&self.pool)
        .await
        .map_err(CompletionError::Db)?
        .rows_affected()
            == 1;
        if flipped {
            tracing::error!(
                worker = %self.worker_id,
                actor_type = %message.actor_type,
                partition = message.partition,
                message_type = %message.message_type,
                message_id = %message.message_id,
                attempts,
                error = %err,
                "mailbox: delivery attempts exhausted -- row FAILED, lane unblocked (operator attention needed)"
            );
        }
        Ok(flipped)
    }

    /// One FULL pass: promotion, claim (or the fair-share rebalance when nothing is claimable),
    /// gated drain, lease renewal. Errors are logged and retried next pass, never propagated —
    /// see the liveness note on [`Self::run`].
    async fn full_pass(&self) {
        if let Err(e) = self.promote().await {
            tracing::warn!(worker = %self.worker_id, actor_type = %self.actor_type, error = %e, "mailbox: promotion failed -- retrying next pass");
        }
        match self.claim().await {
            Ok(0) => {
                // Nothing claimable — every lane is live-owned. If the spread is unfair
                // (deploy overlap, a scaled-up replica set), take one lane from the largest
                // owner; the next passes converge the rest.
                if let Err(e) = self.rebalance().await {
                    tracing::warn!(worker = %self.worker_id, actor_type = %self.actor_type, error = %e, "mailbox: rebalance failed -- retrying next pass");
                }
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(worker = %self.worker_id, actor_type = %self.actor_type, error = %e, "mailbox: claim failed -- retrying next pass");
            }
        }
        if let Err(e) = self.drain().await {
            tracing::error!(worker = %self.worker_id, error = %e, "mailbox: drain pass failed");
        }
        if let Err(e) = self.beat().await {
            tracing::warn!(worker = %self.worker_id, actor_type = %self.actor_type, error = %e, "mailbox: heartbeat failed -- retrying next pass");
        }
    }

    /// How long until the next FULL pass: the stretched safety net while a mailbox listener is
    /// confirmed live (enqueues wake us directly), the heartbeat cadence otherwise — losing push
    /// reverts to exactly the pre-push behaviour, never past it.
    fn full_pass_interval(&self, heartbeat_pause: std::time::Duration) -> std::time::Duration {
        match &self.push_live {
            Some(live) if live.load(std::sync::atomic::Ordering::Relaxed) => {
                std::time::Duration::from_secs(self.config.pushed_pass_seconds)
            }
            _ => heartbeat_pause,
        }
    }

    /// The production loop. Two clocks and one wake signal (PROP-20260802-223522):
    ///
    /// - the FULL pass (promote → claim/rebalance → gated drain → beat) runs on
    ///   [`Self::full_pass_interval`] — the heartbeat cadence normally, the stretched safety net
    ///   while push is confirmed live;
    /// - `beat` ALSO runs alone on the heartbeat cadence between stretched full passes — the
    ///   lease clock never depends on push being healthy, or the fence would die with the
    ///   listener;
    /// - a NUDGE (in-process producer or the LISTEN fan-out) triggers claim + gated drain
    ///   immediately — delivery starts on commit, not on a timer.
    ///
    /// `shutdown` flips the loop off; owned lanes are released so peers take over immediately
    /// instead of waiting out the lease.
    ///
    /// LIVENESS OVER PROPAGATION: a transient claim/beat/drain error is logged and retried next
    /// pass — a momentary pool exhaustion (most likely exactly at peak) must never permanently
    /// end an actor type's delivery. The loop only exits via `shutdown`. A DROPPED shutdown
    /// sender means no shutdown can ever arrive — it must behave like a channel that never fires
    /// (fall through to the sleep), NOT like a signal: `watch::Receiver::changed()` resolves
    /// `Err` immediately once the sender is gone, and treating that as a wake turns every pass
    /// into a zero-sleep busy loop against the database.
    pub async fn run(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) -> sqlx::Result<()> {
        let mut sender_gone = false;
        let heartbeat_pause = std::time::Duration::from_secs(self.config.heartbeat_seconds);
        // First full pass is due immediately; every full pass beats, so the beat clock trails it.
        let mut next_full = tokio::time::Instant::now();
        let mut next_beat = next_full + heartbeat_pause;
        loop {
            if *shutdown.borrow() {
                break;
            }
            let now = tokio::time::Instant::now();
            if now >= next_full {
                self.full_pass().await;
                let after = tokio::time::Instant::now();
                next_full = after + self.full_pass_interval(heartbeat_pause);
                next_beat = after + heartbeat_pause;
            } else if now >= next_beat {
                if let Err(e) = self.beat().await {
                    tracing::warn!(worker = %self.worker_id, actor_type = %self.actor_type, error = %e, "mailbox: heartbeat failed -- retrying next beat");
                }
                next_beat = tokio::time::Instant::now() + heartbeat_pause;
            }
            let sleep = tokio::time::sleep_until(next_full.min(next_beat));
            tokio::pin!(sleep);
            let nudged = async {
                match &self.nudge {
                    Some(n) => n.notified().await,
                    None => std::future::pending().await,
                }
            };
            let mut deliver_now = false;
            if sender_gone {
                tokio::select! {
                    _ = &mut sleep => {}
                    _ = nudged => { deliver_now = true; }
                }
            } else {
                tokio::select! {
                    _ = &mut sleep => {}
                    _ = nudged => { deliver_now = true; }
                    changed = shutdown.changed() => {
                        if changed.is_err() {
                            sender_gone = true;
                            // The wake that noticed the drop must still pace itself.
                            sleep.await;
                        }
                    }
                }
            }
            if deliver_now && !*shutdown.borrow() {
                // A producer says a row landed. Claim first — the nudge may precede this
                // worker's first full pass over a claimable lane — then drain, gated. The full
                // machinery (promotion, rebalance) stays on its own clock.
                if let Err(e) = self.claim().await {
                    tracing::warn!(worker = %self.worker_id, actor_type = %self.actor_type, error = %e, "mailbox: nudge claim failed -- draining owned lanes anyway");
                }
                if let Err(e) = self.drain().await {
                    tracing::error!(worker = %self.worker_id, error = %e, "mailbox: nudge drain failed");
                }
            }
        }
        let lanes: Vec<Lane> = self.lanes.lock().await.drain(..).collect();
        for lane in &lanes {
            if let Err(e) = release_lane(&self.pool, lane, &self.worker_id).await {
                tracing::warn!(worker = %self.worker_id, actor_type = %lane.actor_type, partition = lane.partition, error = %e, "mailbox: lane release failed -- peers take over at lease expiry");
            }
            if let Some(events) = &self.lane_events {
                events.lane_dropped(lane);
            }
        }
        tracing::info!(worker = %self.worker_id, released = lanes.len(), "mailbox: shut down");
        Ok(())
    }

    /// The currently owned lanes (test/supervision visibility).
    pub async fn owned(&self) -> Vec<Lane> {
        self.lanes.lock().await.clone()
    }
}
