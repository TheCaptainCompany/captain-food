//! Delivery DISPATCH STRATEGY resolution (#60) — the runtime "content the spec cannot anticipate":
//! WHICH restaurant self-dispatches, and HOW Captain routes a city's deliveries (its ordered channel
//! walk). The spec declares the channel CATALOG + defaults (`referential.yaml`); this port reads the
//! per-scope USAGE config the DeliveryDispatchProcess saga hooks (`process_managers::delivery_dispatch`)
//! resolve at runtime — deliberately OUTSIDE the event-sourced value forms (they read config tables,
//! not `domain_events`). The Postgres adapter lives in `infrastructure`; a pure double backs the tests.

use async_trait::async_trait;

use domain::generated::scalars::{CityId, DeliveryChannelKey, RestaurantDispatchMode, RestaurantId};
use domain::shared::errors::DomainError;

/// One ranked channel of a city's walk (a resolved `CityDeliveryRanking` row).
#[derive(Debug, Clone, PartialEq)]
pub struct RankedChannel {
    /// 1-based position in the walk order.
    pub rank: i32,
    pub channel: DeliveryChannelKey,
    /// Per-city offer-TTL override for this channel (INDEPENDENT's TTL lives here); `None` = the
    /// catalog default. Capped by the global env max (#60).
    pub ttl_override_seconds: Option<i32>,
}

/// A restaurant's resolved dispatch config (`RestaurantDispatchConfig`; ABSENT ⇒ CAPTAIN, no city —
/// today's default behaviour).
#[derive(Debug, Clone, PartialEq)]
pub struct RestaurantDispatch {
    pub mode: RestaurantDispatchMode,
    pub city_id: Option<CityId>,
}

/// Read-side port over the delivery-strategy config tables (referential.yaml). NOT a `View_*` read
/// model — these are seeded/managed config (later API-writable via partner self-registration, #61).
#[async_trait]
pub trait DispatchStrategyRepository: Send + Sync {
    /// A restaurant's dispatch config: mode + city. An absent config row resolves to
    /// `{ CAPTAIN, city_id: None }` (the platform default — today's behaviour, no config needed).
    async fn restaurant_dispatch(
        &self,
        restaurant_id: RestaurantId,
    ) -> Result<RestaurantDispatch, DomainError>;

    /// A city's ranked channel walk (the latest `effective_from` set, ordered by `rank` ascending).
    /// Falls back to the platform default (rows with `city_id IS NULL`) when `city_id` is `None` or
    /// the city has no ranking of its own.
    async fn ranked_channels(
        &self,
        city_id: Option<CityId>,
    ) -> Result<Vec<RankedChannel>, DomainError>;

    /// A channel's catalog default offer TTL in seconds (`DeliveryChannelCatalog`), for the offer
    /// timeout worker. `None` when the channel has no catalog row.
    async fn channel_default_ttl_seconds(
        &self,
        channel: &DeliveryChannelKey,
    ) -> Result<Option<i32>, DomainError>;
}

/// The resolved plan for ONE dispatch run: the restaurant's mode + (for CAPTAIN) the city's ranked
/// walk. A RESTAURANT-dispatch plan carries no channels (Captain offers nothing, only tracks).
#[derive(Debug, Clone, PartialEq)]
pub struct DispatchPlan {
    pub mode: RestaurantDispatchMode,
    pub ranked: Vec<RankedChannel>,
}

impl DispatchPlan {
    /// The restaurant self-dispatches (Captain offers no channel, only tracks).
    pub fn is_self_dispatched(&self) -> bool {
        self.mode == RestaurantDispatchMode::RESTAURANT
    }

    /// The channel offered at 1-based `rank`, if the walk has one.
    pub fn channel_at(&self, rank: i32) -> Option<DeliveryChannelKey> {
        self.ranked.iter().find(|c| c.rank == rank).map(|c| c.channel.clone())
    }

    /// The ranked entry at 1-based `rank` (channel + its TTL override), if any.
    pub fn entry_at(&self, rank: i32) -> Option<&RankedChannel> {
        self.ranked.iter().find(|c| c.rank == rank)
    }
}

/// Resolve the dispatch plan for a run: read the restaurant's mode/city, then (CAPTAIN only) the
/// city's ranked walk. A RESTAURANT-dispatch restaurant short-circuits with an empty walk.
pub async fn resolve_plan(
    repo: &dyn DispatchStrategyRepository,
    restaurant_id: RestaurantId,
) -> Result<DispatchPlan, DomainError> {
    let dispatch = repo.restaurant_dispatch(restaurant_id).await?;
    let ranked = if dispatch.mode == RestaurantDispatchMode::RESTAURANT {
        Vec::new()
    } else {
        repo.ranked_channels(dispatch.city_id).await?
    };
    Ok(DispatchPlan { mode: dispatch.mode, ranked })
}
