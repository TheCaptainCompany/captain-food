//! The per-actor-type mailbox worker: claim → drain (head-of-line per lane) → heartbeat, forever.
//! Single-step methods are public so tests (and the host's supervision tooling) can drive each
//! move deterministically; [`MailboxWorker::run`] is the production loop over them.

use std::sync::Arc;

use sqlx::PgPool;

use crate::completion::{complete_fenced, CompletionError};
use crate::lease::{claim_due_lanes, heartbeat, release_lane, seed_partitions, Lane};
use crate::message::{InboundMessage, MessageHandler};

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
            lanes: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    /// Idempotently seed this actor type's registry rows (run once at startup).
    pub async fn seed(&self, width: i16) -> sqlx::Result<()> {
        seed_partitions(&self.pool, &self.actor_type, width).await
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
    /// gone); an infrastructure error stops the lane's pass (redelivery retries next pass).
    pub async fn drain(&self) -> sqlx::Result<u64> {
        let lanes: Vec<Lane> = self.lanes.lock().await.clone();
        let mut delivered = 0u64;
        for lane in lanes {
            match self.drain_lane(&lane).await {
                Ok(n) => delivered += n,
                Err(CompletionError::FencedOut) | Err(CompletionError::AlreadyCompleted) => {
                    self.lanes
                        .lock()
                        .await
                        .retain(|l| !(l.actor_type == lane.actor_type && l.partition == lane.partition));
                }
                Err(CompletionError::Db(e)) => return Err(e),
            }
        }
        Ok(delivered)
    }

    /// Head-of-line drain of ONE lane: RECEIVED rows above the checkpoint, strictly in `position`
    /// order, each committed through [`complete_fenced`] before the next is looked at.
    pub async fn drain_lane(&self, lane: &Lane) -> Result<u64, CompletionError> {
        let mut delivered = 0u64;
        loop {
            let rows = sqlx::query(
                "SELECT message_id, position, kind, actor_type, actor_id, partition, message_type, \
                        payload, payload_hash, channel, user_id, user_type, correlation_id, cause_id, \
                        session_id, received_at \
                 FROM inbound_messages \
                 WHERE actor_type = $1 AND partition = $2 AND status = 'RECEIVED' AND position > $3 \
                 ORDER BY position \
                 LIMIT $4",
            )
            .bind(&lane.actor_type)
            .bind(lane.partition)
            .bind(lane.checkpoint)
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
            // A full batch: keep going from the last committed position — the checkpoint moved
            // with every completion, but our Lane snapshot did not; re-query picks up after it.
        }
    }

    /// The production loop: claim → drain → beat, then sleep a heartbeat. `shutdown` flips the
    /// loop off; owned lanes are released so peers take over immediately instead of waiting out
    /// the lease.
    pub async fn run(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) -> sqlx::Result<()> {
        loop {
            if *shutdown.borrow() {
                break;
            }
            self.claim().await?;
            if let Err(e) = self.drain().await {
                tracing::error!(worker = %self.worker_id, error = %e, "mailbox: drain pass failed");
            }
            self.beat().await?;
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(self.config.heartbeat_seconds)) => {}
                _ = shutdown.changed() => {}
            }
        }
        let lanes: Vec<Lane> = self.lanes.lock().await.drain(..).collect();
        for lane in &lanes {
            release_lane(&self.pool, lane, &self.worker_id).await?;
        }
        tracing::info!(worker = %self.worker_id, released = lanes.len(), "mailbox: shut down");
        Ok(())
    }

    /// The currently owned lanes (test/supervision visibility).
    pub async fn owned(&self) -> Vec<Lane> {
        self.lanes.lock().await.clone()
    }
}
