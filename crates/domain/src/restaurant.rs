//! Restaurant aggregate — the PURE write-side state fold (ADR-0035). Command handlers rehydrate a
//! [`RestaurantState`] by folding the stream's events (loaded through the `EventStore` port) and then
//! enforce the invariants declared in `specs/actors.yaml`/`specs/errors.yaml` against it. Deliberately
//! MINIMAL: only the fields those invariants read are folded — the full read model lives in the
//! `Restaurant` projection (ADR-0040), not here. No I/O, no serialization logic (dependency rule).
//!
//! The status machine is the DECLARED lifecycle (`specs/actors.yaml#/Restaurant/lifecycle`,
//! ADR-20260721-093027): the fold moves `status` exclusively through the GENERATED tables
//! ([`lifecycle::initial`] births it DRAFT, [`lifecycle::target`] applies `RestaurantActivated` →
//! ACTIVE and `RestaurantDeactivated`/`RestaurantRemoved`/`RestaurantMarkedClosed` → INACTIVE), so
//! write-side decisions and the projected `status` column can never disagree.

use crate::generated::events::DomainEvent;
pub use crate::generated::lifecycles::restaurant as lifecycle;
use crate::generated::scalars::{
    ExternalReference, OrderAcceptanceMode, RestaurantDisplayName, RestaurantListingStatus,
    RestaurantStatus, Slug, WebUrl,
};

/// What the Restaurant command handlers need to know about the aggregate to accept or reject a
/// command. `None` (from [`fold`]) means the aggregate does not exist → `RestaurantNotFound`.
#[derive(Debug, Clone, PartialEq)]
pub struct RestaurantState {
    /// Operational lifecycle (DRAFT → ACTIVE ⇄ INACTIVE) — `RestaurantNotActive`, activate/deactivate
    /// idempotency.
    pub status: RestaurantStatus,
    /// Live acceptance mode (NORMAL until changed) — `AcceptanceModeUnchanged`.
    pub order_acceptance: OrderAcceptanceMode,
    /// Partnership funnel position (NON_PARTNER → PASSIVE_PARTNER → ACTIVE_PARTNER).
    pub listing_status: RestaurantListingStatus,
    /// Whether an owner already claimed this listing — `ListingAlreadyClaimed`.
    pub listing_claimed: bool,
    /// The configured GBP 'Order online' link, if any — `GbpOrderLinkNotConfigured` (ADR-0021).
    pub gbp_order_url: Option<WebUrl>,
    /// Current slug (registration value; identity of the storefront host).
    pub slug: Slug,
    /// Display name, carried into rejection contexts (errors.yaml `restaurantName`).
    pub display_name: RestaurantDisplayName,
    /// External idempotent import key, when seeded from an external source.
    pub r#ref: Option<ExternalReference>,
}

/// Fold a Restaurant stream (events in version order) into its current state. `None` ⇔ the stream has
/// no `RestaurantRegistered` yet, i.e. the aggregate does not exist.
pub fn fold(events: &[DomainEvent]) -> Option<RestaurantState> {
    events.iter().fold(None, apply)
}

/// Apply one event to the state — a pure transition, total over the whole event union (events not
/// touching the folded fields are no-ops, so a fatter stream never breaks rehydration).
fn apply(state: Option<RestaurantState>, event: &DomainEvent) -> Option<RestaurantState> {
    if let Some(status) = lifecycle::initial(event) {
        if let DomainEvent::RestaurantRegistered(e) = event {
            return Some(RestaurantState {
                status,
                order_acceptance: OrderAcceptanceMode::NORMAL,
                listing_status: e.listing_status,
                listing_claimed: false,
                gbp_order_url: None,
                slug: e.slug.clone(),
                display_name: e.display_name.clone(),
                r#ref: e.r#ref.clone(),
            });
        }
    }
    let mut s = state?;
    // The recorded fact wins at fold time: `target` maps a lifecycle event to its state regardless
    // of the current one (legality is `transition`'s job at append time).
    if let Some(next) = lifecycle::target(event) {
        s.status = next;
    }
    match event {
        DomainEvent::RestaurantAcceptanceModeChanged(e) => s.order_acceptance = e.mode,
        DomainEvent::RestaurantUpdated(e) => {
            if let Some(name) = &e.display_name {
                s.display_name = name.clone();
            }
        }
        DomainEvent::RestaurantListingClaimed(_) => s.listing_claimed = true,
        DomainEvent::RestaurantListingStatusChanged(e) => s.listing_status = e.listing_status,
        DomainEvent::RestaurantGoogleBusinessProfileOrderLinkConfigured(e) => {
            s.gbp_order_url = Some(e.gbp_order_url.clone())
        }
        _ => {}
    }
    Some(s)
}
