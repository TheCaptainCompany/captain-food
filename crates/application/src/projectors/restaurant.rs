//! Hand-written `RestaurantCompute` (ADR-0040). The generator maps ~21/27 of Restaurant's columns; these
//! are the 4 that need derivation or cross-stream state.
#![allow(unused_variables)]

use crate::projections::{Envelope, RestaurantCompute, RestaurantRow};
use domain::generated::events::DomainEvent;
use domain::generated::scalars::{CurrencyCode, OrderAcceptanceMode, RestaurantStatus};

pub struct RestaurantProjector;

impl RestaurantCompute for RestaurantProjector {
    /// ⚠️ HOLE: no event carries a restaurant description (spec) — preserve whatever is there.
    fn description(&self, prev: Option<&RestaurantRow>, env: &Envelope) -> Option<String> {
        prev.and_then(|r| r.description.clone())
    }

    /// Lifecycle status, derived from the event type.
    fn status(&self, prev: Option<&RestaurantRow>, env: &Envelope) -> RestaurantStatus {
        match &env.event {
            DomainEvent::RestaurantRegistered(_) => RestaurantStatus::DRAFT,
            DomainEvent::RestaurantActivated(_) => RestaurantStatus::ACTIVE,
            DomainEvent::RestaurantDeactivated(_)
            | DomainEvent::RestaurantRemoved(_)
            | DomainEvent::RestaurantMarkedClosed(_) => RestaurantStatus::INACTIVE,
            _ => prev.map(|r| r.status.clone()).unwrap_or(RestaurantStatus::DRAFT),
        }
    }

    /// Order-acceptance mode: NORMAL until the restaurant changes it.
    fn order_acceptance(&self, prev: Option<&RestaurantRow>, env: &Envelope) -> OrderAcceptanceMode {
        match &env.event {
            DomainEvent::RestaurantAcceptanceModeChanged(e) => e.mode.clone(),
            _ => prev.map(|r| r.order_acceptance.clone()).unwrap_or(OrderAcceptanceMode::NORMAL),
        }
    }

    /// CROSS-STREAM: the owning account's default currency (set on the account stream's
    /// RestaurantAccountRegistered). TODO(runtime): resolve via a RestaurantAccount read-model port;
    /// preserved meanwhile.
    fn default_currency(&self, prev: Option<&RestaurantRow>, env: &Envelope) -> CurrencyCode {
        prev.map(|r| r.default_currency.clone()).unwrap_or_else(|| CurrencyCode("EUR".into()))
    }
}
