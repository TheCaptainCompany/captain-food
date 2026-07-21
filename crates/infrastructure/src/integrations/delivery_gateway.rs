//! The COMPOSITE delivery gateway (#60) — a registry of channel → adapter that routes
//! `delivery.offer_job(channel, job)` to the adapter wired for that channel. It replaces the
//! composition root's single Avelo-vs-Noop choice: the DeliveryDispatchProcess saga now offers a job
//! on a STRATEGY-RESOLVED channel (the city's ranked walk), and each channel has its own adapter
//! (`avelo37`, later `uber_direct`/`coopcycle`) or is the independent-rider POOL.
//!
//! A channel with NO wired adapter is fail-closed: the offer is a logged no-op that the partner never
//! answers, so the offer TIMEOUT worker escalates the run to the next ranked channel (an unconfigured
//! `uber_direct` in Tours simply falls through — today's unconfigured deployments are unchanged, and
//! `independent` = the rider pool, a deliberate no-op that keeps the job open to riders).

use std::collections::HashMap;
use std::sync::Arc;

use application::generated::services::{DeliveryOfferJobInput, DeliveryService, ServiceCallMeta};
use async_trait::async_trait;
use domain::shared::errors::DomainError;

/// Routes each `offer_job` to the adapter registered for `input.channel`; an unregistered channel is
/// a logged no-op (the offer times out → the saga escalates to the next ranked channel).
pub struct CompositeDeliveryGateway {
    adapters: HashMap<String, Arc<dyn DeliveryService>>,
}

impl CompositeDeliveryGateway {
    pub fn new() -> Self {
        Self { adapters: HashMap::new() }
    }

    /// Register `adapter` as the wired outbound for `channel` (the `DeliveryChannelCatalog` key).
    pub fn with_channel(mut self, channel: &str, adapter: Arc<dyn DeliveryService>) -> Self {
        self.adapters.insert(channel.to_string(), adapter);
        self
    }

    /// The channels with a wired adapter (for the boot log).
    pub fn wired_channels(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.adapters.keys().cloned().collect();
        keys.sort();
        keys
    }
}

impl Default for CompositeDeliveryGateway {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DeliveryService for CompositeDeliveryGateway {
    async fn offer_job(
        &self,
        input: DeliveryOfferJobInput,
        meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        match self.adapters.get(&input.channel.0) {
            Some(adapter) => adapter.offer_job(input, meta).await,
            None => {
                // Fail-closed: no wired adapter for this channel. The offer is a no-op the partner
                // never answers, so the DeliveryOfferTimeoutWorker escalates to the next ranked
                // channel (unconfigured channels fall through; the independent-rider POOL keeps the
                // job open to riders).
                eprintln!(
                    "delivery gateway: channel '{}' has no wired adapter — job {} (order {}) left \
                     open (offer will time out and escalate to the next ranked channel)",
                    input.channel.0, input.job.delivery_job_id.0, input.job.order_id.0
                );
                Ok(())
            }
        }
    }
}
