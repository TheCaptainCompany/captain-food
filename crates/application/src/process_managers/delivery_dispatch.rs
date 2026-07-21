//! DeliveryDispatchProcess (`specs/processmanager.yaml#/DeliveryDispatchProcess`, ADR-0031;
//! resolve→walk strategy, #60) — HOOK IMPLS + thin wrappers for the GENERATED leg pipelines
//! (`crate::generated::process_managers::delivery_dispatch_process`, issue #25). The pipelines
//! (state by/expect/set, the linear-branch marker, deliver/send plumbing) are generated; this module
//! keeps only the non-structural seams:
//!
//! - the OrderTracking / Restaurant reads (the pickup address folds from the restaurant's own
//!   stream, like the restaurant command handlers);
//! - `build_delivery_requested` — the DETERMINISTIC UUIDv5 job id ([`delivery_job_id_for`], the
//!   idempotency key), the order's test-mode lineage (ADR-0038), and the pickup/dropoff unwraps;
//! - the DISPATCH-STRATEGY resolution (#60): the birth leg resolves the restaurant's mode + the
//!   city's rank-1 channel (self-dispatch short-circuits — `offer_job` is SKIPPED); each advance leg
//!   WALKS to the next ranked channel (`branch` = "channels remain?", `advance_current_*` = the walk
//!   step). All of it reads the config tables through [`DispatchStrategyRepository`], not the log.
//! - the DeliveryJob fold predicates (birth exists / not already FAILED).

use domain::generated::entities::Address;
use domain::generated::events::{
    DeliveryAcceptedByPartner, DeliveryCompleted, DeliveryDispatchFailed, DeliveryEscalationRequested,
    DeliveryOfferTimedOut, DeliveryRejectedByPartner, DeliveryRequested, DeliveryStatusUpdated,
    DomainEvent, OrderMarkedReady,
};
use domain::generated::scalars::{
    DeliveryChannelKey, DeliveryDispatchProcessStatus, DeliveryStatus, OrderId, RestaurantId,
};
use domain::shared::errors::DomainError;

use crate::dispatch_strategy::{resolve_plan, DispatchPlan, DispatchStrategyRepository};
use crate::generated::process_managers::delivery_dispatch_process::{self, OrderRead, RestaurantRead};
use crate::generated::process_managers::HookOutcome;
use crate::generated::services::{DeliveryOfferJobInput, DeliveryService};
use crate::pm_state::{DeliveryDispatchRow, DeliveryDispatchStateStore};
use crate::ports::EventStore;
use crate::process_managers::{
    delivery_job_stream, order_stream, restaurant_stream, Outcome, TriggerEnvelope,
};
use crate::queries::OrderReadRepository;

/// Fixed UUIDv5 namespace for the ids this saga derives. NEVER change it: derived job ids must stay
/// stable across re-reactions and deployments (they ARE the idempotency key).
fn dispatch_namespace() -> uuid::Uuid {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        b"https://captain.food/process-managers/delivery-dispatch",
    )
}

/// The deterministic delivery job for an order (one V0 job per order; a future re-dispatch policy
/// would version the derivation, not randomize it).
pub fn delivery_job_id_for(order_id: &OrderId) -> domain::generated::scalars::DeliveryJobId {
    domain::generated::scalars::DeliveryJobId(uuid::Uuid::new_v5(&dispatch_namespace(), order_id.0.as_bytes()))
}

/// The restaurant's current address — the job's pickup point, folded from ITS stream (registration
/// carries it, a later `RestaurantUpdated` may replace it).
fn restaurant_address(restaurant_events: &[DomainEvent]) -> Option<Address> {
    restaurant_events.iter().fold(None, |addr, e| match e {
        DomainEvent::RestaurantRegistered(r) => Some(r.address.clone()),
        DomainEvent::RestaurantUpdated(u) => u.address.clone().or(addr),
        _ => addr,
    })
}

/// Load the job's `DeliveryRequested` birth fact (the `offer_job` payload) from its stream.
async fn load_delivery_requested(
    store: &dyn EventStore,
    job_id: &domain::generated::scalars::DeliveryJobId,
) -> Result<Option<DeliveryRequested>, DomainError> {
    let (events, _) = store.load(&delivery_job_stream(job_id)).await?;
    Ok(events.into_iter().find_map(|e| match e {
        DomainEvent::DeliveryRequested(r) => Some(r),
        _ => None,
    }))
}

// ================================================================================================
// Birth leg — OrderMarkedReady: create the job, resolve the strategy, offer the rank-1 channel.
// ================================================================================================

/// Hooks for the `OrderMarkedReady` leg: reads, the derived job birth, and the strategy resolution.
pub struct DispatchOpenHooks<'a> {
    store: &'a dyn EventStore,
    orders: &'a dyn OrderReadRepository,
    strategy: &'a dyn DispatchStrategyRepository,
}

impl<'a> DispatchOpenHooks<'a> {
    pub fn new(
        store: &'a dyn EventStore,
        orders: &'a dyn OrderReadRepository,
        strategy: &'a dyn DispatchStrategyRepository,
    ) -> Self {
        Self { store, orders, strategy }
    }

    /// The restaurant's mode + the city's ranked walk (resolved from the config tables per call —
    /// the birth leg's hooks each re-resolve; cheap, and the read model can cache).
    async fn plan(&self, restaurant_id: RestaurantId) -> Result<DispatchPlan, DomainError> {
        resolve_plan(self.strategy, restaurant_id).await
    }
}

#[async_trait::async_trait]
impl delivery_dispatch_process::OrderMarkedReadyHooks for DispatchOpenHooks<'_> {
    async fn read_order(&self, order_id: OrderId) -> Result<HookOutcome<OrderRead>, DomainError> {
        let Some(order) = self.orders.by_id(order_id).await? else {
            return Ok(HookOutcome::Skip(format!(
                "order {} is not in the OrderTracking read model yet — cannot dispatch its delivery",
                order_id.0
            )));
        };
        Ok(HookOutcome::Ready(OrderRead {
            service_type: order.service_type,
            delivery_address: order
                .delivery_address
                .and_then(|v| serde_json::from_value::<Address>(v).ok()),
        }))
    }

    /// The pickup address — folded from the aggregate's own stream, like the restaurant command
    /// handlers (the projected row carries the same address as jsonb).
    async fn read_restaurant(
        &self,
        restaurant_id: RestaurantId,
    ) -> Result<HookOutcome<RestaurantRead>, DomainError> {
        let (restaurant_events, _) = self.store.load(&restaurant_stream(&restaurant_id)).await?;
        Ok(HookOutcome::Ready(RestaurantRead { address: restaurant_address(&restaurant_events) }))
    }

    async fn build_delivery_requested(
        &self,
        event: &OrderMarkedReady,
        order: &OrderRead,
        restaurant: &RestaurantRead,
    ) -> Result<HookOutcome<DeliveryRequested>, DomainError> {
        let Some(dropoff) = order.delivery_address.clone() else {
            return Ok(HookOutcome::Skip(format!(
                "DELIVERY order {} carries no decodable delivery address — cannot dispatch",
                event.order_id.0
            )));
        };
        let Some(pickup) = restaurant.address.clone() else {
            return Ok(HookOutcome::Skip(format!(
                "restaurant {} has no address on its stream — cannot set the pickup for order {}",
                event.restaurant_id.0, event.order_id.0
            )));
        };
        // Test-mode lineage (ADR-0038): the job inherits the ORDER's mode, read from the
        // authoritative OrderPlaced fact (OrderTracking does not carry `mode`).
        let (order_events, _) = self.store.load(&order_stream(&event.order_id)).await?;
        let mode = order_events.iter().find_map(|e| match e {
            DomainEvent::OrderPlaced(p) => Some(p.mode),
            _ => None,
        });
        Ok(HookOutcome::Ready(DeliveryRequested {
            mode: mode.flatten(),
            delivery_job_id: delivery_job_id_for(&event.order_id),
            order_id: event.order_id,
            restaurant_id: event.restaurant_id,
            pickup,
            dropoff,
            // Partner pre-selection is now the resolve→walk strategy; the birth fact stays
            // provider-agnostic (the channel is chosen at offer time, #60).
            provider: None,
        }))
    }

    /// Idempotent re-dispatch: the deterministic id targets the same stream, whose fold absorbs the replay.
    fn should_deliver_delivery_requested(&self, stream: &[DomainEvent], _event: &DeliveryRequested) -> bool {
        domain::delivery_job::fold(stream).is_none()
    }

    /// Resolve the strategy and offer the rank-1 channel — OR skip the call entirely for a
    /// RESTAURANT-dispatch order (the self-dispatch short-circuit: Captain offers no channel).
    async fn input_delivery_offer_job(
        &self,
        event: &OrderMarkedReady,
        _order: &OrderRead,
        _restaurant: &RestaurantRead,
        delivery_requested: &DeliveryRequested,
    ) -> Result<HookOutcome<DeliveryOfferJobInput>, DomainError> {
        let plan = self.plan(event.restaurant_id).await?;
        if plan.is_self_dispatched() {
            return Ok(HookOutcome::Skip(format!(
                "order {} is RESTAURANT-dispatched — Captain offers no channel (self-dispatch), only tracks",
                event.order_id.0
            )));
        }
        let Some(channel) = plan.channel_at(1) else {
            return Ok(HookOutcome::Skip(format!(
                "order {} is CAPTAIN-dispatched but its city has no ranked channels — nothing to offer",
                event.order_id.0
            )));
        };
        Ok(HookOutcome::Ready(DeliveryOfferJobInput { job: delivery_requested.clone(), channel }))
    }

    /// A CAPTAIN-dispatch order opens OFFERED; a RESTAURANT-dispatch order opens SELF_DISPATCHED.
    async fn open_process_status(
        &self,
        event: &OrderMarkedReady,
        _order: &OrderRead,
        _restaurant: &RestaurantRead,
        _delivery_requested: &DeliveryRequested,
    ) -> Result<DeliveryDispatchProcessStatus, DomainError> {
        let plan = self.plan(event.restaurant_id).await?;
        Ok(if plan.is_self_dispatched() {
            DeliveryDispatchProcessStatus::SELF_DISPATCHED
        } else {
            DeliveryDispatchProcessStatus::OFFERED
        })
    }

    /// CAPTAIN → rank 1; RESTAURANT (self-dispatch) → null.
    async fn open_current_rank(
        &self,
        event: &OrderMarkedReady,
        _order: &OrderRead,
        _restaurant: &RestaurantRead,
        _delivery_requested: &DeliveryRequested,
    ) -> Result<Option<i32>, DomainError> {
        let plan = self.plan(event.restaurant_id).await?;
        Ok((!plan.is_self_dispatched() && plan.channel_at(1).is_some()).then_some(1))
    }

    /// CAPTAIN → the rank-1 channel offer_job was invoked against; RESTAURANT → null.
    async fn open_current_channel(
        &self,
        event: &OrderMarkedReady,
        _order: &OrderRead,
        _restaurant: &RestaurantRead,
        _delivery_requested: &DeliveryRequested,
    ) -> Result<Option<DeliveryChannelKey>, DomainError> {
        let plan = self.plan(event.restaurant_id).await?;
        Ok(if plan.is_self_dispatched() { None } else { plan.channel_at(1) })
    }
}

// ================================================================================================
// Advance legs — decline / timeout / escalate: walk to the next ranked channel, else fail closed.
// ================================================================================================

/// Shared hooks for the three IDENTICAL advance legs (`DeliveryRejectedByPartner`,
/// `DeliveryEscalationRequested`, `DeliveryOfferTimedOut`): the ranked-channel walk. Each trigger
/// advances the run to the NEXT ranked channel; when the walk is exhausted the leg falls through to
/// the terminal `DeliveryDispatchFailed` (fail-closed, rules.yaml#/DispatchExhaustionFailsClosed).
pub struct DispatchAdvanceHooks<'a> {
    store: &'a dyn EventStore,
    strategy: &'a dyn DispatchStrategyRepository,
}

impl<'a> DispatchAdvanceHooks<'a> {
    pub fn new(store: &'a dyn EventStore, strategy: &'a dyn DispatchStrategyRepository) -> Self {
        Self { store, strategy }
    }

    async fn plan(&self, restaurant_id: RestaurantId) -> Result<DispatchPlan, DomainError> {
        resolve_plan(self.strategy, restaurant_id).await
    }

    /// The next rank in the walk after the one currently offered (`current_rank` + 1; 1 when unset).
    fn next_rank(row: &DeliveryDispatchRow) -> i32 {
        row.current_rank.unwrap_or(0) + 1
    }

    /// Whether a next ranked channel remains to offer.
    async fn channels_remain(&self, row: &DeliveryDispatchRow) -> Result<bool, DomainError> {
        let plan = self.plan(row.restaurant_id).await?;
        Ok(plan.channel_at(Self::next_rank(row)).is_some())
    }

    /// The re-offer input for the next ranked channel (skips if the walk is exhausted or the birth
    /// fact is missing).
    async fn next_offer(
        &self,
        row: &DeliveryDispatchRow,
    ) -> Result<HookOutcome<DeliveryOfferJobInput>, DomainError> {
        let plan = self.plan(row.restaurant_id).await?;
        let Some(channel) = plan.channel_at(Self::next_rank(row)) else {
            return Ok(HookOutcome::Skip(format!(
                "job {} has walked every ranked channel — nothing left to offer",
                row.delivery_job_id.0
            )));
        };
        match load_delivery_requested(self.store, &row.delivery_job_id).await? {
            Some(job) => Ok(HookOutcome::Ready(DeliveryOfferJobInput { job, channel })),
            None => Ok(HookOutcome::Skip(format!(
                "job {} has no DeliveryRequested birth to re-offer",
                row.delivery_job_id.0
            ))),
        }
    }

    async fn next_channel(&self, row: &DeliveryDispatchRow) -> Result<Option<DeliveryChannelKey>, DomainError> {
        let plan = self.plan(row.restaurant_id).await?;
        Ok(plan.channel_at(Self::next_rank(row)))
    }

    /// Idempotent: skipped when the job already folds FAILED.
    fn already_failed(stream: &[DomainEvent]) -> bool {
        domain::delivery_job::fold(stream)
            .map(|s| s.status == DeliveryStatus::FAILED)
            .unwrap_or(false)
    }
}

/// Implement the three advance-leg hook traits (they differ only in the trigger event type) with the
/// same ranked-walk logic — the event is unused (the walk is driven by the state row).
macro_rules! impl_advance_hooks {
    ($trait:ident, $event:ty) => {
        #[async_trait::async_trait]
        impl delivery_dispatch_process::$trait for DispatchAdvanceHooks<'_> {
            async fn branch(&self, _event: &$event, row: &DeliveryDispatchRow) -> Result<bool, DomainError> {
                self.channels_remain(row).await
            }
            async fn input_delivery_offer_job(
                &self,
                _event: &$event,
                row: &DeliveryDispatchRow,
            ) -> Result<HookOutcome<DeliveryOfferJobInput>, DomainError> {
                self.next_offer(row).await
            }
            fn compute_offer_attempts(&self, row: &DeliveryDispatchRow) -> i32 {
                row.offer_attempts + 1
            }
            async fn advance_current_rank(
                &self,
                _event: &$event,
                row: &DeliveryDispatchRow,
            ) -> Result<Option<i32>, DomainError> {
                Ok(Some(Self::next_rank(row)))
            }
            async fn advance_current_channel(
                &self,
                _event: &$event,
                row: &DeliveryDispatchRow,
            ) -> Result<Option<DeliveryChannelKey>, DomainError> {
                self.next_channel(row).await
            }
            fn should_deliver_delivery_dispatch_failed(
                &self,
                stream: &[DomainEvent],
                _event: &DeliveryDispatchFailed,
            ) -> bool {
                !Self::already_failed(stream)
            }
        }
    };
}

impl_advance_hooks!(DeliveryRejectedByPartnerHooks, DeliveryRejectedByPartner);
impl_advance_hooks!(DeliveryEscalationRequestedHooks, DeliveryEscalationRequested);
impl_advance_hooks!(DeliveryOfferTimedOutHooks, DeliveryOfferTimedOut);

/// Default-only hooks for the non-walking advance/close legs (accepted, status-updated, completed).
pub struct DispatchTrackHooks;

impl delivery_dispatch_process::DeliveryAcceptedByPartnerHooks for DispatchTrackHooks {}
impl delivery_dispatch_process::DeliveryStatusUpdatedHooks for DispatchTrackHooks {}
impl delivery_dispatch_process::DeliveryCompletedHooks for DispatchTrackHooks {}

// ================================================================================================
// Wrappers — the runner / behaviour-test harness call these (they inject the hook structs).
// ================================================================================================

/// EVENT leg `events.yaml#/OrderMarkedReady` (rules.yaml#/ReadyDeliveryOrderTriggersDispatch +
/// rules.yaml#/RestaurantDispatchBypassesRouting).
#[allow(clippy::too_many_arguments)]
pub async fn on_order_marked_ready(
    store: &dyn EventStore,
    state: &dyn DeliveryDispatchStateStore,
    orders: &dyn OrderReadRepository,
    partner: &dyn DeliveryService,
    strategy: &dyn DispatchStrategyRepository,
    event: &OrderMarkedReady,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    delivery_dispatch_process::on_order_marked_ready(
        store,
        state,
        partner,
        &DispatchOpenHooks::new(store, orders, strategy),
        event,
        env,
    )
    .await
}

/// EVENT leg `events.yaml#/DeliveryAcceptedByPartner` (rules.yaml#/PartnerAcceptanceRecordsCourier).
pub async fn on_delivery_accepted_by_partner(
    state: &dyn DeliveryDispatchStateStore,
    event: &DeliveryAcceptedByPartner,
) -> Result<Outcome, DomainError> {
    delivery_dispatch_process::on_delivery_accepted_by_partner(state, &DispatchTrackHooks, event).await
}

/// EVENT leg `events.yaml#/DeliveryRejectedByPartner` (rules.yaml#/CityRankingWalkedInOrder +
/// rules.yaml#/DispatchExhaustionFailsClosed, #60).
pub async fn on_delivery_rejected_by_partner(
    store: &dyn EventStore,
    state: &dyn DeliveryDispatchStateStore,
    partner: &dyn DeliveryService,
    strategy: &dyn DispatchStrategyRepository,
    event: &DeliveryRejectedByPartner,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    delivery_dispatch_process::on_delivery_rejected_by_partner(
        store,
        state,
        partner,
        &DispatchAdvanceHooks::new(store, strategy),
        event,
        env,
    )
    .await
}

/// EVENT leg `events.yaml#/DeliveryEscalationRequested` (rules.yaml#/ManualEscalateSkipsChannel, #60).
pub async fn on_delivery_escalation_requested(
    store: &dyn EventStore,
    state: &dyn DeliveryDispatchStateStore,
    partner: &dyn DeliveryService,
    strategy: &dyn DispatchStrategyRepository,
    event: &DeliveryEscalationRequested,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    delivery_dispatch_process::on_delivery_escalation_requested(
        store,
        state,
        partner,
        &DispatchAdvanceHooks::new(store, strategy),
        event,
        env,
    )
    .await
}

/// EVENT leg `events.yaml#/DeliveryOfferTimedOut` (rules.yaml#/TimeoutEscalatesToNextChannel, #60).
pub async fn on_delivery_offer_timed_out(
    store: &dyn EventStore,
    state: &dyn DeliveryDispatchStateStore,
    partner: &dyn DeliveryService,
    strategy: &dyn DispatchStrategyRepository,
    event: &DeliveryOfferTimedOut,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    delivery_dispatch_process::on_delivery_offer_timed_out(
        store,
        state,
        partner,
        &DispatchAdvanceHooks::new(store, strategy),
        event,
        env,
    )
    .await
}

/// EVENT leg `events.yaml#/DeliveryStatusUpdated` (rules.yaml#/OrderClosedOnDeliveryCompletion):
/// only the terminal DELIVERED report closes the order.
pub async fn on_delivery_status_updated(
    store: &dyn EventStore,
    state: &dyn DeliveryDispatchStateStore,
    event: &DeliveryStatusUpdated,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    delivery_dispatch_process::on_delivery_status_updated(store, state, &DispatchTrackHooks, event, env)
        .await
}

/// EVENT leg `events.yaml#/DeliveryCompleted` (rules.yaml#/OrderClosedOnDeliveryCompletion).
pub async fn on_delivery_completed(
    store: &dyn EventStore,
    state: &dyn DeliveryDispatchStateStore,
    event: &DeliveryCompleted,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    delivery_dispatch_process::on_delivery_completed(store, state, &DispatchTrackHooks, event, env)
        .await
}

#[cfg(test)]
mod tests;
