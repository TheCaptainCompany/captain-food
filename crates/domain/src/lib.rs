//! Captain.Food domain — the inner core (ADR-0035).
//!
//! Pure DDD: aggregates, commands, events, policies and value objects. This crate depends on **no other
//! workspace crate** — the dependency rule's innermost ring. Per decision 1 (ADR-0035) domain events and
//! value objects MAY derive `serde` (they are serialized into the append-only `domain_events` log and
//! cross the Crux/UniFFI boundary); serialization *logic* (wire formats, HubRise `"9.80 EUR"` parsing)
//! belongs in the infrastructure ACL, never here.
//!
//! Per-aggregate modules (`restaurant`, `order`, `customer`, `cart`, `review`, …) land here as the domain
//! model is generated/implemented from the specs. Only the `shared` vocabulary is scaffolded for now.

pub mod generated;
pub mod shared;

#[cfg(test)]
mod scalar_serde_tests {
    //! The generated enums use `#[serde(rename_all = "SCREAMING_SNAKE_CASE")]`; assert the wire values
    //! round-trip exactly to the `scalars.yaml` enum values (a mismatch would silently corrupt event/DB
    //! (de)serialization). Covers the tricky multi-word cases.
    use crate::generated::scalars::{OrderStatus, UserType};

    #[test]
    fn enums_roundtrip_to_screaming_snake() {
        assert_eq!(serde_json::to_string(&UserType::RestaurantAccount).unwrap(), "\"RESTAURANT_ACCOUNT\"");
        assert_eq!(serde_json::to_string(&OrderStatus::OutForDelivery).unwrap(), "\"OUT_FOR_DELIVERY\"");
        assert_eq!(
            serde_json::from_str::<OrderStatus>("\"CANCELLED_BY_CUSTOMER\"").unwrap(),
            OrderStatus::CancelledByCustomer
        );
    }
}
