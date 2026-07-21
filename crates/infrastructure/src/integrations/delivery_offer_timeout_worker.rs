//! The delivery OFFER-TIMEOUT worker (#60) — the scheduler that escalates a stale dispatch offer to
//! the next ranked channel, mirroring [`super::retention_sweep_worker`] (periodic, env-gated). It
//! finds `delivery_dispatch_process_manager` rows still `OFFERED` whose `last_update_utc` is older
//! than the RESOLVED TTL and records a `DeliveryOfferTimedOut` fact on the job stream — the
//! DeliveryDispatchProcess advance leg then walks to the next ranked channel (or fails closed when
//! the walk is exhausted).
//!
//! Resolved TTL = `min( global env max , city ttl_override ?? channel catalog default )`. The global
//! max (`DELIVERY_OFFER_MAX_TTL_SECONDS`, default 900) is the hard ceiling; per-channel/per-city
//! values narrow it. A row whose channel resolves no TTL at all falls back to the global max.
//!
//! Idempotency: one timeout per (job, current rank). A pass skips a run that already has a
//! `DeliveryOfferTimedOut` for its current rank on the stream (the saga bumps `last_update_utc` when
//! it advances, so a re-offered run's clock restarts) — erring, like the bounded-decline path, toward
//! escalation rather than a runaway loop.

use std::sync::Arc;
use std::time::Duration;

use application::dispatch_strategy::DispatchStrategyRepository;
use application::ports::{Actor, EventStore};
use application::process_managers::delivery_job_stream;
use application::repository::Repository;
use domain::generated::events::{DeliveryOfferTimedOut, DomainEvent};
use domain::generated::scalars::{DeliveryChannelKey, DeliveryJobId};
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};

use crate::persistence::{db_err, PgDispatchStrategy, PgEventStore};

/// `process_status` ordinal for OFFERED (ADR-0037; matches `enum_sql::DeliveryDispatchProcessStatus`).
const OFFERED_ORDINAL: i32 = 0;

/// Env var for the global maximum offer TTL (the hard ceiling over every channel), in seconds.
pub const MAX_TTL_ENV: &str = "DELIVERY_OFFER_MAX_TTL_SECONDS";
const DEFAULT_MAX_TTL_SECONDS: i64 = 900;

/// Sweep cadence — offers expire on a minutes scale, so a sub-minute pass keeps escalation prompt.
const SWEEP_INTERVAL: Duration = Duration::from_secs(30);

/// One OFFERED run that has aged past its resolved TTL.
struct StaleOffer {
    delivery_job_id: DeliveryJobId,
    current_rank: i32,
    current_channel: DeliveryChannelKey,
    age_seconds: i64,
}

pub struct DeliveryOfferTimeoutWorker {
    pool: PgPool,
    store: PgEventStore,
    strategy: PgDispatchStrategy,
    max_ttl_seconds: i64,
}

impl DeliveryOfferTimeoutWorker {
    pub fn new(pool: PgPool) -> Self {
        let max_ttl_seconds = std::env::var(MAX_TTL_ENV)
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(DEFAULT_MAX_TTL_SECONDS);
        Self {
            store: PgEventStore::new(pool.clone()),
            strategy: PgDispatchStrategy::new(pool.clone()),
            max_ttl_seconds,
            pool,
        }
    }

    /// The worker's system identity for the timeout facts it authors (EXTERNAL, fresh correlation).
    fn actor() -> Actor {
        Actor {
            user_id: uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"captain.food/delivery-offer-timeout-worker"),
            user_type: 6, // UserType::EXTERNAL ordinal (ADR-0037)
            correlation_id: uuid::Uuid::new_v4(),
            cause_id: None,
        }
    }

    /// Resolve a channel's TTL for a run's city: `min(global max, city override ?? channel default)`.
    async fn resolved_ttl_seconds(
        &self,
        restaurant_id: domain::generated::scalars::RestaurantId,
        channel: &DeliveryChannelKey,
    ) -> Result<i64, DomainError> {
        let dispatch = self.strategy.restaurant_dispatch(restaurant_id).await?;
        let ranked = self.strategy.ranked_channels(dispatch.city_id).await?;
        let city_override = ranked
            .iter()
            .find(|c| &c.channel == channel)
            .and_then(|c| c.ttl_override_seconds);
        let base = match city_override {
            Some(v) => Some(v),
            None => self.strategy.channel_default_ttl_seconds(channel).await?,
        };
        Ok(base.map(i64::from).unwrap_or(self.max_ttl_seconds).min(self.max_ttl_seconds))
    }

    /// One sweep pass: escalate every OFFERED run older than its resolved TTL. Returns the count of
    /// timeout facts recorded.
    pub async fn run_once(&self) -> Result<u64, DomainError> {
        let rows = sqlx::query(
            "SELECT delivery_job_id, restaurant_id, current_rank, current_channel, \
                    EXTRACT(EPOCH FROM (now() - last_update_utc))::bigint AS age_seconds \
             FROM delivery_dispatch_process_manager \
             WHERE process_status = $1 AND current_channel IS NOT NULL AND current_rank IS NOT NULL",
        )
        .bind(OFFERED_ORDINAL)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let mut recorded = 0u64;
        for row in &rows {
            let restaurant_id = domain::generated::scalars::RestaurantId(
                row.try_get::<uuid::Uuid, _>("restaurant_id").map_err(db_err)?,
            );
            let stale = StaleOffer {
                delivery_job_id: DeliveryJobId(row.try_get::<uuid::Uuid, _>("delivery_job_id").map_err(db_err)?),
                current_rank: row.try_get::<i32, _>("current_rank").map_err(db_err)?,
                current_channel: DeliveryChannelKey(row.try_get("current_channel").map_err(db_err)?),
                age_seconds: row.try_get::<i64, _>("age_seconds").map_err(db_err)?,
            };
            let ttl = self.resolved_ttl_seconds(restaurant_id, &stale.current_channel).await?;
            if stale.age_seconds < ttl {
                continue;
            }
            if self.record_timeout(&stale).await? {
                recorded += 1;
            }
        }
        Ok(recorded)
    }

    /// Record one `DeliveryOfferTimedOut` fact, unless the run already has one for its current rank
    /// (idempotency guard). Returns whether a fact was appended.
    async fn record_timeout(&self, stale: &StaleOffer) -> Result<bool, DomainError> {
        let stream = delivery_job_stream(&stale.delivery_job_id);
        let (events, version) = self.store.load(&stream).await?;
        let already = events.iter().any(|e| {
            matches!(e, DomainEvent::DeliveryOfferTimedOut(t)
                if t.delivery_job_id == stale.delivery_job_id && t.rank == i64::from(stale.current_rank))
        });
        if already {
            return Ok(false);
        }
        let event = DomainEvent::DeliveryOfferTimedOut(DeliveryOfferTimedOut {
            delivery_job_id: stale.delivery_job_id,
            channel: stale.current_channel.clone(),
            rank: i64::from(stale.current_rank),
            reason: Some(format!("offer on channel '{}' expired", stale.current_channel.0)),
        });
        Repository::new(&self.store).save(&stream, version, &[event], &Self::actor()).await?;
        Ok(true)
    }

    /// Poll loop (env-gated in the composition root, like the other in-process workers).
    pub async fn run_loop(self: Arc<Self>) {
        loop {
            match self.run_once().await {
                Ok(n) if n > 0 => println!("delivery offer timeout: escalated {n} stale offer(s)"),
                Ok(_) => {}
                Err(e) => eprintln!("delivery offer timeout sweep failed: {e}"),
            }
            tokio::time::sleep(SWEEP_INTERVAL).await;
        }
    }
}
