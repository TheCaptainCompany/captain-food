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

pub mod aggregate;
pub mod cart;
pub mod catalog;
pub mod customer;
pub mod delivery_job;
pub mod delivery_partner_registration;
pub mod generated;
pub mod order;
pub mod payment;
pub mod prospect;
pub mod restaurant;
pub mod restaurant_account;
pub mod rider;
pub mod shared;

#[cfg(test)]
mod scalar_serde_tests {
    //! The generated enums use `#[serde(rename_all = "SCREAMING_SNAKE_CASE")]`; assert the wire values
    //! round-trip exactly to the `scalars.yaml` enum values (a mismatch would silently corrupt event/DB
    //! (de)serialization). Covers the tricky multi-word cases.
    use crate::generated::scalars::{OrderStatus, UserType};

    #[test]
    fn enums_serialize_verbatim_to_spec_values() {
        // Variants are the spec values verbatim (no serde rename) — the identifier IS the wire value.
        assert_eq!(serde_json::to_string(&UserType::RESTAURANT_ACCOUNT).unwrap(), "\"RESTAURANT_ACCOUNT\"");
        assert_eq!(serde_json::to_string(&OrderStatus::OUT_FOR_DELIVERY).unwrap(), "\"OUT_FOR_DELIVERY\"");
        assert_eq!(
            serde_json::from_str::<OrderStatus>("\"CANCELLED_BY_CUSTOMER\"").unwrap(),
            OrderStatus::CANCELLED_BY_CUSTOMER
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

#[cfg(test)]
mod event_command_serde_tests {
    //! The generated event/command payload structs (ADR-0034 #3) are what infrastructure (de)serializes
    //! into the append-only `domain_events` log / across the write side. Assert their wire shape matches
    //! the specs exactly: camelCase keys, scalar/enum values verbatim, and an omitted optional → `None`.
    use crate::generated::commands::ChangeOrderAcceptanceMode;
    use crate::generated::events::RestaurantAccountDeleted;
    use crate::generated::scalars::{OrderAcceptanceMode, RestaurantAccountId, RestaurantId};

    #[test]
    fn event_payload_omits_optional_and_uses_camel_case() {
        let nil = "00000000-0000-0000-0000-000000000000";
        // Only the required field present → the optional `reason` deserializes to None.
        let evt: RestaurantAccountDeleted =
            serde_json::from_value(serde_json::json!({ "restaurantAccountId": nil })).unwrap();
        assert!(evt.reason.is_none());
        // Serialize back: the wire key is the exact camelCase spec property name.
        let v = serde_json::to_value(&evt).unwrap();
        assert_eq!(v["restaurantAccountId"], nil);
    }

    #[test]
    fn command_payload_roundtrips_camel_case_with_enum() {
        let nil = "00000000-0000-0000-0000-000000000000";
        let cmd = ChangeOrderAcceptanceMode {
            restaurant_id: RestaurantId(nil.parse().unwrap()),
            mode: OrderAcceptanceMode::PAUSED,
        };
        let v = serde_json::to_value(&cmd).unwrap();
        assert_eq!(v, serde_json::json!({ "restaurantId": nil, "mode": "PAUSED" }));
        assert_eq!(serde_json::from_value::<ChangeOrderAcceptanceMode>(v).unwrap(), cmd);
        let _ = RestaurantAccountId(nil.parse().unwrap()); // id types are in scope for events too
    }
}
