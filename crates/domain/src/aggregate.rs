//! The `Aggregate` trait — the event-sourced-actor contract.
//!
//! Each write-side aggregate is an "actor" (`specs/actors.yaml`): it has an identity (its event stream),
//! rehydrates from that stream (`fold`), and its behaviour (command/event handlers) decides new events.
//! This trait unifies **identity + rehydration** in one place so the write-side `Repository` (application
//! layer) can load/save any aggregate generically, instead of every command handler and the saga runner
//! re-deriving the `"<Category>-<id>"` stream name and calling `fold` by hand.
//!
//! It says nothing about persistence: the aggregate OWNS emission (pure `fold` + decide); the `Repository`
//! owns persistence; the `EventStore`/`PgEventStore` is the adapter behind it.

use crate::generated::events::DomainEvent;
use crate::generated::scalars::{
    CartId, CatalogId, CustomerId, DeliveryJobId, DeliveryPartnerRegistrationId, OrderId,
    RestaurantAccountId, RestaurantId, RiderId,
};

/// An event-sourced aggregate (a persistent actor): a typed identity + rehydration from its own stream.
pub trait Aggregate: Sized {
    /// The aggregate's typed identifier — its stream key.
    type Id: Copy;
    /// The stream-category prefix (`"Order"`, `"Cart"`, …); the stream is `"<category>-<id>"`.
    fn category() -> &'static str;
    /// This aggregate's event-stream name for `id`.
    fn stream(id: Self::Id) -> String;
    /// Rehydrate the write-side state from its stream events. `None` = the aggregate does not exist yet.
    fn fold(events: &[DomainEvent]) -> Option<Self>;
}

/// Implement [`Aggregate`] for a `<Module>State` by delegating to the module's existing `fold`. The stream
/// name is `"<Category>-<uuid>"` — byte-identical to the hand-written `*_stream` helpers it replaces.
macro_rules! impl_aggregate {
    ($state:ty, $id:ty, $category:literal, $fold:path) => {
        impl Aggregate for $state {
            type Id = $id;
            fn category() -> &'static str {
                $category
            }
            fn stream(id: Self::Id) -> String {
                format!("{}-{}", $category, id.0)
            }
            fn fold(events: &[DomainEvent]) -> Option<Self> {
                $fold(events)
            }
        }
    };
}

impl_aggregate!(crate::restaurant::RestaurantState, RestaurantId, "Restaurant", crate::restaurant::fold);
impl_aggregate!(
    crate::restaurant_account::RestaurantAccountState,
    RestaurantAccountId,
    "RestaurantAccount",
    crate::restaurant_account::fold
);
impl_aggregate!(crate::cart::CartState, CartId, "Cart", crate::cart::fold);
impl_aggregate!(crate::order::OrderState, OrderId, "Order", crate::order::fold);
impl_aggregate!(crate::catalog::CatalogState, CatalogId, "Catalog", crate::catalog::fold);
impl_aggregate!(crate::customer::CustomerState, CustomerId, "Customer", crate::customer::fold);
impl_aggregate!(crate::delivery_job::DeliveryJobState, DeliveryJobId, "DeliveryJob", crate::delivery_job::fold);
impl_aggregate!(crate::rider::RiderState, RiderId, "Rider", crate::rider::fold);
impl_aggregate!(
    crate::delivery_partner_registration::DeliveryPartnerRegistrationState,
    DeliveryPartnerRegistrationId,
    "DeliveryPartnerRegistration",
    crate::delivery_partner_registration::fold
);
// Payment is NOT an `Aggregate` impl: its identity is the Stripe PaymentIntentId (a String provider
// reference, not a Copy uuid newtype) — see `crate::payment::{CATEGORY, stream, fold}`.
// A Prospect is keyed by the prospected Restaurant's id (ADR-0020) — same id type as Restaurant, distinct stream.
impl_aggregate!(crate::prospect::ProspectState, RestaurantId, "Prospect", crate::prospect::fold);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_names_match_the_legacy_format() {
        let id = uuid::Uuid::nil();
        assert_eq!(crate::order::OrderState::stream(OrderId(id)), format!("Order-{id}"));
        assert_eq!(crate::cart::CartState::stream(CartId(id)), format!("Cart-{id}"));
        assert_eq!(
            crate::prospect::ProspectState::stream(RestaurantId(id)),
            format!("Prospect-{id}")
        );
    }
}
