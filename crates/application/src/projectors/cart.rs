//! Hand-written `CartCompute` (ADR-0040). `status`/`customer_id` fold from the events; the priced columns
//! need the live catalog + pricing policies, which arrive with the runtime read-side.
#![allow(unused_variables)]

use crate::projections::{CartCompute, CartRow, Envelope};
use domain::generated::events::DomainEvent;
use domain::generated::scalars::{CartStatus, CurrencyCode, CustomerId, MoneyCents};
use serde_json::Value;

pub struct CartProjector;

impl CartCompute for CartProjector {
    /// Cart owner: from CartStarted; CustomerIdentified is cross-stream (keyed by authRef, no cart id) and
    /// is routed to a visitor's carts by the runtime — preserved here. TODO(runtime).
    fn customer_id(&self, prev: Option<&CartRow>, env: &Envelope) -> Option<CustomerId> {
        match &env.event {
            DomainEvent::CartStarted(e) => e.customer_id.clone(),
            _ => prev.and_then(|r| r.customer_id.clone()),
        }
    }

    /// OPEN while active, CHECKED_OUT once the cart is checked out.
    fn status(&self, prev: Option<&CartRow>, env: &Envelope) -> CartStatus {
        match &env.event {
            DomainEvent::CartCheckedOut(_) => CartStatus::CHECKED_OUT,
            DomainEvent::CartStarted(_) => CartStatus::OPEN,
            _ => prev.map(|r| r.status.clone()).unwrap_or(CartStatus::OPEN),
        }
    }

    // lines / total / currency / estimated_breakdown / uber_comparison are COMPUTED from the live catalog
    // (prices, options) + View_PricingPolicy / Uber*Policy — none of which are in the cart event payload.
    // TODO(runtime): compute via the catalog + policy read-model ports (never trust prices from the client).
    fn lines(&self, prev: Option<&CartRow>, env: &Envelope) -> Value {
        prev.map(|r| r.lines.clone()).unwrap_or_else(|| Value::Array(Vec::new()))
    }
    fn total_amount_cents(&self, prev: Option<&CartRow>, env: &Envelope) -> MoneyCents {
        prev.map(|r| r.total_amount_cents.clone()).unwrap_or(MoneyCents(0))
    }
    fn currency(&self, prev: Option<&CartRow>, env: &Envelope) -> CurrencyCode {
        prev.map(|r| r.currency.clone()).unwrap_or_else(|| CurrencyCode("EUR".into()))
    }
    fn estimated_breakdown(&self, prev: Option<&CartRow>, env: &Envelope) -> Option<Value> {
        prev.and_then(|r| r.estimated_breakdown.clone())
    }
    fn uber_comparison(&self, prev: Option<&CartRow>, env: &Envelope) -> Option<Value> {
        prev.and_then(|r| r.uber_comparison.clone())
    }
}
