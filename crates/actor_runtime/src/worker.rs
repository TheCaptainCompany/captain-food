//! The per-actor-type mailbox worker: promote (due reminders) → claim → drain (head-of-line per
//! lane) → heartbeat, forever.
//! Single-step methods are public so tests (and the host's supervision tooling) can drive each
//! move deterministically; [`MailboxWorker::run`] is the production loop over them.

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::Notify;

use crate::completion::{complete_fenced, CompletionError};
use crate::lease::{
    claim_due_lanes, heartbeat, ownership_census, release_lane, seed_partitions, steal_from, Lane,
};
use crate::message::{DeliveryObserver, InboundMessage, MessageHandler};

/// Tuning knobs — the host wires these from ITS configuration source (specs/configuration.yaml:
/// `MAILBOX_LEASE_SECONDS`, `MAILBOX_HEARTBEAT_SECONDS`); the defaults mirror the spec defaults.
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
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self { lease_seconds: 30, heartbeat_seconds: 10, batch: 100, max_claims_per_pass: 100 }
    }
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
    /// heartbeat poll to ~immediate. Purely an accelerator — the poll is the guarantee.
    nudge: Option<Arc<Notify>>,
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
            lanes: tokio::sync::Mutex::new(Vec::new()),
        }
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
            }
        }
        *lanes = kept;
        Ok(())
    }

    /// Drain one pass over every owned lane, head-of-line per lane. Returns delivered count.
    /// A fenced-out / already-completed delivery drops the lane on the spot (the authority is
    /// gone); an infrastructure error stops THAT lane's pass and moves on to the next lane —
    /// one failing lane (a poisoned head row, a data-dependent flush error) must never starve
    /// the worker's other lanes; the failing lane retries next pass.
    pub async fn drain(&self) -> sqlx::Result<u64> {
        let lanes: Vec<Lane> = self.lanes.lock().await.clone();
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
                let verdict = complete_fenced(
                    &self.pool,
                    lane,
                    &self.worker_id,
                    &message,
                    self.handler.as_ref(),
                )
                .await?;
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

    /// The production loop: promote → claim → drain → beat, then sleep a heartbeat (or until a
    /// producer's nudge). `shutdown` flips the loop off; owned lanes are released so peers take over
    /// immediately instead of waiting out the lease.
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
        loop {
            if *shutdown.borrow() {
                break;
            }
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
            let sleep =
                tokio::time::sleep(std::time::Duration::from_secs(self.config.heartbeat_seconds));
            tokio::pin!(sleep);
            let nudged = async {
                match &self.nudge {
                    Some(n) => n.notified().await,
                    None => std::future::pending().await,
                }
            };
            if sender_gone {
                tokio::select! {
                    _ = &mut sleep => {}
                    _ = nudged => {}
                }
            } else {
                tokio::select! {
                    _ = &mut sleep => {}
                    _ = nudged => {}
                    changed = shutdown.changed() => {
                        if changed.is_err() {
                            sender_gone = true;
                            // The wake that noticed the drop must still pace itself.
                            sleep.await;
                        }
                    }
                }
            }
        }
        let lanes: Vec<Lane> = self.lanes.lock().await.drain(..).collect();
        for lane in &lanes {
            if let Err(e) = release_lane(&self.pool, lane, &self.worker_id).await {
                tracing::warn!(worker = %self.worker_id, actor_type = %lane.actor_type, partition = lane.partition, error = %e, "mailbox: lane release failed -- peers take over at lease expiry");
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
