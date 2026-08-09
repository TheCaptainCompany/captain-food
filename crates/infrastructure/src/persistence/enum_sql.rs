//! Enum ↔ TEXT mapping (ADR-20260728, superseding the ADR-0037 INTEGER ordinals): enum columns store
//! the scalars.yaml value verbatim, so a row is self-describing and needs no `ref_*` lookup join.
//! One impl per enum the projection tables (`Restaurant`, `ProspectionPipeline`, `Catalog`, `Cart`,
//! `OrderTracking`) need; shared by the read repositories (row → `…Row`) and the projection upserts
//! (row → SQL).
//!
//! The variant names below are EXACTLY the stored values (they mirror `domain::generated::scalars`,
//! which is generated from `specs/scalars.yaml`).

use domain::generated::scalars::{
    CartStatus, CityAvailabilityStatus, CommandChannel, CommandJournalStatus, ComparisonBasis,
    CuisineCategory, DeliveryDispatchProcessStatus, DeliveryProvider, DeliveryStatus,
    DeliveryTimeliness, GbpLinkStatus,
    InboundMessageStatus, OrderAcceptanceMode, OrderStatus, PaymentProcessStatus, PaymentStatus,
    ProspectPipelineStatus, ReclamationCategory, ReclamationResolution, ReclamationStatus,
    RefundProcessStatus, RefundStatus, RestaurantDispatchMode, RestaurantListingStatus,
    RestaurantStatus, ScopeType, ServiceType, ThumbRating, UserType,
};
use domain::shared::errors::DomainError;

/// TEXT value ↔ domain enum — the stored string IS the variant name.
pub trait EnumText: Sized {
    fn to_text(&self) -> &'static str;
    fn from_text(s: &str) -> Result<Self, DomainError>;
}

macro_rules! enum_text {
    ($ty:ident { $($variant:ident),+ $(,)? }) => {
        impl EnumText for $ty {
            fn to_text(&self) -> &'static str {
                match self { $( $ty::$variant => stringify!($variant), )+ }
            }
            fn from_text(s: &str) -> Result<Self, DomainError> {
                match s {
                    $( stringify!($variant) => Ok($ty::$variant), )+
                    other => Err(DomainError::Repository(format!(
                        "unknown {} value '{other}'", stringify!($ty)
                    ))),
                }
            }
        }
    };
}

enum_text!(RestaurantStatus { DRAFT, ACTIVE, INACTIVE });
enum_text!(RestaurantListingStatus { NON_PARTNER, PASSIVE_PARTNER, ACTIVE_PARTNER });
enum_text!(GbpLinkStatus { UNSET, CONFIGURED, VERIFIED, BROKEN });
enum_text!(OrderAcceptanceMode { NORMAL, BUSY, PAUSED });
enum_text!(CuisineCategory { FAST_FOOD, PIZZA, TRADITIONAL, BISTRONOMIC, FOOD_TRUCK });
enum_text!(ProspectPipelineStatus { NEW, CONTACTED, COLD, REPLIED, CONVERTED });
enum_text!(CartStatus { OPEN, CHECKED_OUT });
enum_text!(ServiceType { DELIVERY, COLLECTION });
enum_text!(OrderStatus {
    PLACED,
    ACCEPTED,
    REJECTED,
    PREPARING,
    READY,
    OUT_FOR_DELIVERY,
    DELIVERED,
    CANCELLED_BY_CUSTOMER,
    CANCELLED_BY_RESTAURANT,
});
enum_text!(DeliveryStatus {
    PENDING,
    ASSIGNED,
    PICKED_UP,
    OUT_FOR_DELIVERY,
    DELIVERED,
    FAILED,
    CANCELLED,
});
enum_text!(DeliveryProvider { PARTNER, INDEPENDENT });
enum_text!(InboundMessageStatus {
    SCHEDULED,
    CANCELLED,
    RECEIVED,
    SUCCEEDED,
    REJECTED,
    FAILED,
    IGNORED,
    DUPLICATE,
});
enum_text!(ComparisonBasis { ESTIMATED, REAL });
enum_text!(ThumbRating { UP, DOWN });
enum_text!(DeliveryTimeliness { ON_TIME, ACCEPTABLE_DELAY, TOO_LATE });
enum_text!(PaymentStatus { PENDING, CAPTURED, FAILED, REFUNDED });
enum_text!(PaymentProcessStatus { AWAITING_PAYMENT_RESULT, ORDER_PLACED, FAILED });
enum_text!(RefundStatus { REQUESTED, APPROVED, DENIED, REFUNDED });
enum_text!(RefundProcessStatus {
    PENDING_APPROVAL,
    APPROVED_AWAITING_SETTLEMENT,
    DENIED,
    REFUNDED,
});
enum_text!(DeliveryDispatchProcessStatus { OFFERED, ACCEPTED, FAILED, COMPLETED, SELF_DISPATCHED });
enum_text!(RestaurantDispatchMode { CAPTAIN, RESTAURANT });
enum_text!(CityAvailabilityStatus { PENDING, APPROVED, REVOKED });
enum_text!(ReclamationStatus { OPEN, RESOLVED, REJECTED });
enum_text!(ReclamationCategory {
    MISSING_ITEM,
    WRONG_ITEM,
    QUALITY,
    LATE_DELIVERY,
    DAMAGED,
    NOT_DELIVERED,
    OTHER,
});
enum_text!(ReclamationResolution {
    FULL_REFUND,
    PARTIAL_REFUND,
    REPLACEMENT,
    GOODWILL_CREDIT,
    REJECTED,
});
enum_text!(CommandJournalStatus { RECEIVED, SUCCEEDED, REJECTED, FAILED });
enum_text!(CommandChannel { GRAPHQL, WORKER, INTERNAL });
// --- Read-side per-instance authorization (#144) ---
// ScopeMembership stores BOTH of these as TEXT (the variant name verbatim, like every enum column
// since ADR-20260728). UserType's stored values are shared vocabulary with
// `domain_events.user_type` / `command_journal.user_type` — a scalars.yaml variant RENAME would
// strand historical rows, which is also why the UUIDv5 membership key pins its own literal
// (`membership_id_is_pinned` in the projector).
enum_text!(ScopeType { ORDER, RESTAURANT });
enum_text!(UserType {
    PUBLIC,
    CUSTOMER,
    RESTAURANT_ACCOUNT,
    RESTAURANT,
    RIDER,
    ADMIN,
    EXTERNAL,
});

/// `to_text` through an `Option` (nullable enum column).
pub fn opt_to_text<E: EnumText>(v: &Option<E>) -> Option<&'static str> {
    v.as_ref().map(EnumText::to_text)
}

/// `from_text` through an `Option` (nullable enum column).
pub fn opt_from_text<E: EnumText>(s: Option<String>) -> Result<Option<E>, DomainError> {
    s.as_deref().map(E::from_text).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_values_round_trip_as_the_variant_name() {
        for (text, v) in [
            ("DRAFT", RestaurantStatus::DRAFT),
            ("ACTIVE", RestaurantStatus::ACTIVE),
            ("INACTIVE", RestaurantStatus::INACTIVE),
        ] {
            assert_eq!(v.to_text(), text);
            assert_eq!(RestaurantStatus::from_text(text).unwrap(), v);
        }
        assert_eq!(RestaurantListingStatus::ACTIVE_PARTNER.to_text(), "ACTIVE_PARTNER");
        assert_eq!(OrderAcceptanceMode::PAUSED.to_text(), "PAUSED");
        assert_eq!(CuisineCategory::FOOD_TRUCK.to_text(), "FOOD_TRUCK");
        assert_eq!(GbpLinkStatus::BROKEN.to_text(), "BROKEN");
        assert_eq!(ProspectPipelineStatus::CONVERTED.to_text(), "CONVERTED");
        assert_eq!(
            ProspectPipelineStatus::from_text("CONTACTED").unwrap(),
            ProspectPipelineStatus::CONTACTED
        );
        assert_eq!(CartStatus::CHECKED_OUT.to_text(), "CHECKED_OUT");
        assert_eq!(ServiceType::COLLECTION.to_text(), "COLLECTION");
        assert_eq!(OrderStatus::CANCELLED_BY_RESTAURANT.to_text(), "CANCELLED_BY_RESTAURANT");
        assert_eq!(OrderStatus::from_text("READY").unwrap(), OrderStatus::READY);
        assert_eq!(DeliveryStatus::CANCELLED.to_text(), "CANCELLED");
        assert_eq!(DeliveryProvider::PARTNER.to_text(), "PARTNER");
        assert_eq!(DeliveryProvider::from_text("INDEPENDENT").unwrap(), DeliveryProvider::INDEPENDENT);
        assert_eq!(ComparisonBasis::REAL.to_text(), "REAL");
        assert_eq!(ThumbRating::DOWN.to_text(), "DOWN");
        assert_eq!(PaymentStatus::REFUNDED.to_text(), "REFUNDED");
        assert_eq!(PaymentStatus::from_text("CAPTURED").unwrap(), PaymentStatus::CAPTURED);
        assert_eq!(PaymentProcessStatus::FAILED.to_text(), "FAILED");
        assert_eq!(
            PaymentProcessStatus::from_text("AWAITING_PAYMENT_RESULT").unwrap(),
            PaymentProcessStatus::AWAITING_PAYMENT_RESULT
        );
        assert_eq!(RefundProcessStatus::REFUNDED.to_text(), "REFUNDED");
        assert_eq!(
            RefundProcessStatus::from_text("APPROVED_AWAITING_SETTLEMENT").unwrap(),
            RefundProcessStatus::APPROVED_AWAITING_SETTLEMENT
        );
        assert_eq!(DeliveryDispatchProcessStatus::COMPLETED.to_text(), "COMPLETED");
        assert_eq!(
            DeliveryDispatchProcessStatus::from_text("FAILED").unwrap(),
            DeliveryDispatchProcessStatus::FAILED
        );
        assert_eq!(CommandJournalStatus::FAILED.to_text(), "FAILED");
        assert_eq!(
            CommandJournalStatus::from_text("RECEIVED").unwrap(),
            CommandJournalStatus::RECEIVED
        );
        assert_eq!(CommandChannel::INTERNAL.to_text(), "INTERNAL");
        assert_eq!(CommandChannel::from_text("WORKER").unwrap(), CommandChannel::WORKER);
        assert!(RestaurantStatus::from_text("BOGUS").is_err());
    }
}
