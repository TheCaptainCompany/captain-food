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

#[cfg(test)]
mod entity_serde_tests {
    //! The generated entity structs use `#[serde(rename_all = "camelCase")]` over transparent scalar
    //! newtypes; assert the wire shape matches `entities.yaml` exactly (camelCase keys, scalars as bare
    //! values) and that an optional array (`#[serde(default)]`) tolerates a missing key.
    use crate::generated::entities::{CartLineItem, Money};
    use crate::generated::scalars::{CurrencyCode, MoneyCents};

    #[test]
    fn money_roundtrips_camel_case_with_transparent_scalars() {
        let money = Money { amount_cents: MoneyCents(980), currency: CurrencyCode("EUR".into()) };
        let json = serde_json::json!({ "amountCents": 980, "currency": "EUR" });
        assert_eq!(serde_json::to_value(&money).unwrap(), json);
        assert_eq!(serde_json::from_value::<Money>(json).unwrap(), money);
    }

    #[test]
    fn missing_optional_array_deserializes_to_empty() {
        let nil = "00000000-0000-0000-0000-000000000000";
        let line: CartLineItem = serde_json::from_value(serde_json::json!({
            "cartLineId": nil,
            "offerId": nil,
            "quantity": 2,
        }))
        .unwrap();
        assert_eq!(line.quantity, 2);
        assert!(line.selected_option_ids.is_empty());
    }
}
