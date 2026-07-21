//! Enum ↔ INTEGER ordinal mapping (ADR-0037): enum columns are stored as the DECLARATION-ORDER ordinal
//! (= `ref_<enum>.sort_order`). One impl per enum the projection tables (`Restaurant`,
//! `ProspectionPipeline`, `Catalog`, `Cart`, `OrderTracking`) need; shared by the read repositories
//! (row → `…Row`) and the projection upserts (row → SQL).
//!
//! The declaration order below MUST match `domain::generated::scalars` (which is generated from
//! `specs/scalars.yaml`, the same source the `ref_*` seed rows come from).

use domain::generated::scalars::{
    CartStatus, CommandChannel, CommandJournalStatus, ComparisonBasis, CuisineCategory,
    DeliveryDispatchProcessStatus, DeliveryProvider, DeliveryStatus, GbpLinkStatus,
    InboundEventStatus, OrderAcceptanceMode, OrderStatus, PaymentProcessStatus, PaymentStatus,
    ProspectPipelineStatus, RefundProcessStatus, RefundStatus, RestaurantDispatchMode,
    RestaurantListingStatus, RestaurantStatus, ServiceType, ThumbRating,
};
use domain::shared::errors::DomainError;

/// i32 ordinal ↔ domain enum, in declaration order.
pub trait EnumOrd: Sized {
    fn to_ord(&self) -> i32;
    fn from_ord(ord: i32) -> Result<Self, DomainError>;
}

macro_rules! enum_ord {
    ($ty:ident { $($variant:ident => $ord:literal),+ $(,)? }) => {
        impl EnumOrd for $ty {
            fn to_ord(&self) -> i32 {
                match self { $( $ty::$variant => $ord, )+ }
            }
            fn from_ord(ord: i32) -> Result<Self, DomainError> {
                match ord {
                    $( $ord => Ok($ty::$variant), )+
                    other => Err(DomainError::Repository(format!(
                        "unknown {} ordinal {other}", stringify!($ty)
                    ))),
                }
            }
        }
    };
}

enum_ord!(RestaurantStatus { DRAFT => 0, ACTIVE => 1, INACTIVE => 2 });
enum_ord!(RestaurantListingStatus { NON_PARTNER => 0, PASSIVE_PARTNER => 1, ACTIVE_PARTNER => 2 });
enum_ord!(GbpLinkStatus { UNSET => 0, CONFIGURED => 1, VERIFIED => 2, BROKEN => 3 });
enum_ord!(OrderAcceptanceMode { NORMAL => 0, BUSY => 1, PAUSED => 2 });
enum_ord!(CuisineCategory {
    FAST_FOOD => 0,
    PIZZA => 1,
    TRADITIONAL => 2,
    BISTRONOMIC => 3,
    FOOD_TRUCK => 4,
});
enum_ord!(ProspectPipelineStatus {
    NEW => 0,
    CONTACTED => 1,
    COLD => 2,
    REPLIED => 3,
    CONVERTED => 4,
});
enum_ord!(CartStatus { OPEN => 0, CHECKED_OUT => 1 });
enum_ord!(ServiceType { DELIVERY => 0, COLLECTION => 1 });
enum_ord!(OrderStatus {
    PLACED => 0,
    ACCEPTED => 1,
    REJECTED => 2,
    PREPARING => 3,
    READY => 4,
    OUT_FOR_DELIVERY => 5,
    DELIVERED => 6,
    CANCELLED_BY_CUSTOMER => 7,
    CANCELLED_BY_RESTAURANT => 8,
});
enum_ord!(DeliveryStatus {
    PENDING => 0,
    ASSIGNED => 1,
    PICKED_UP => 2,
    OUT_FOR_DELIVERY => 3,
    DELIVERED => 4,
    FAILED => 5,
    CANCELLED => 6,
});
enum_ord!(DeliveryProvider { PARTNER => 0, INDEPENDENT => 1 });
enum_ord!(ComparisonBasis { ESTIMATED => 0, REAL => 1 });
enum_ord!(ThumbRating { UP => 0, DOWN => 1 });
enum_ord!(PaymentStatus { PENDING => 0, CAPTURED => 1, FAILED => 2, REFUNDED => 3 });
enum_ord!(PaymentProcessStatus { AWAITING_PAYMENT_RESULT => 0, ORDER_PLACED => 1, FAILED => 2 });
enum_ord!(RefundStatus {
    REQUESTED => 0,
    APPROVED => 1,
    DENIED => 2,
    REFUNDED => 3,
});
enum_ord!(RefundProcessStatus {
    PENDING_APPROVAL => 0,
    APPROVED_AWAITING_SETTLEMENT => 1,
    DENIED => 2,
    REFUNDED => 3,
});
enum_ord!(DeliveryDispatchProcessStatus {
    OFFERED => 0,
    ACCEPTED => 1,
    // FAILED keeps retired REOFFER_REQUIRED's slot (both flag manual handling; ADR-20260720-004556).
    FAILED => 2,
    COMPLETED => 3,
    // SELF_DISPATCHED appended last to preserve the pre-#60 ordinals (ADR-0037).
    SELF_DISPATCHED => 4,
});
enum_ord!(RestaurantDispatchMode { CAPTAIN => 0, RESTAURANT => 1 });
enum_ord!(CommandJournalStatus { RECEIVED => 0, SUCCEEDED => 1, REJECTED => 2, FAILED => 3 });
enum_ord!(CommandChannel { GRAPHQL => 0, WORKER => 1, INTERNAL => 2 });
enum_ord!(InboundEventStatus { RECEIVED => 0, DELIVERED => 1, FAILED => 2 });

/// `to_ord` through an `Option` (nullable enum column).
pub fn opt_to_ord<E: EnumOrd>(v: &Option<E>) -> Option<i32> {
    v.as_ref().map(EnumOrd::to_ord)
}

/// `from_ord` through an `Option` (nullable enum column).
pub fn opt_from_ord<E: EnumOrd>(ord: Option<i32>) -> Result<Option<E>, DomainError> {
    ord.map(E::from_ord).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinals_round_trip_in_declaration_order() {
        for (ord, v) in [
            (0, RestaurantStatus::DRAFT),
            (1, RestaurantStatus::ACTIVE),
            (2, RestaurantStatus::INACTIVE),
        ] {
            assert_eq!(v.to_ord(), ord);
            assert_eq!(RestaurantStatus::from_ord(ord).unwrap(), v);
        }
        assert_eq!(RestaurantListingStatus::ACTIVE_PARTNER.to_ord(), 2);
        assert_eq!(OrderAcceptanceMode::PAUSED.to_ord(), 2);
        assert_eq!(CuisineCategory::FOOD_TRUCK.to_ord(), 4);
        assert_eq!(GbpLinkStatus::BROKEN.to_ord(), 3);
        assert_eq!(ProspectPipelineStatus::CONVERTED.to_ord(), 4);
        assert_eq!(ProspectPipelineStatus::from_ord(1).unwrap(), ProspectPipelineStatus::CONTACTED);
        assert_eq!(CartStatus::CHECKED_OUT.to_ord(), 1);
        assert_eq!(ServiceType::COLLECTION.to_ord(), 1);
        assert_eq!(OrderStatus::CANCELLED_BY_RESTAURANT.to_ord(), 8);
        assert_eq!(OrderStatus::from_ord(4).unwrap(), OrderStatus::READY);
        assert_eq!(DeliveryStatus::CANCELLED.to_ord(), 6);
        assert_eq!(DeliveryProvider::PARTNER.to_ord(), 0);
        assert_eq!(DeliveryProvider::from_ord(1).unwrap(), DeliveryProvider::INDEPENDENT);
        assert_eq!(ComparisonBasis::REAL.to_ord(), 1);
        assert_eq!(ThumbRating::DOWN.to_ord(), 1);
        assert_eq!(PaymentStatus::REFUNDED.to_ord(), 3);
        assert_eq!(PaymentStatus::from_ord(1).unwrap(), PaymentStatus::CAPTURED);
        assert_eq!(PaymentProcessStatus::FAILED.to_ord(), 2);
        assert_eq!(
            PaymentProcessStatus::from_ord(0).unwrap(),
            PaymentProcessStatus::AWAITING_PAYMENT_RESULT
        );
        assert_eq!(RefundProcessStatus::REFUNDED.to_ord(), 3);
        assert_eq!(
            RefundProcessStatus::from_ord(1).unwrap(),
            RefundProcessStatus::APPROVED_AWAITING_SETTLEMENT
        );
        assert_eq!(DeliveryDispatchProcessStatus::COMPLETED.to_ord(), 3);
        assert_eq!(
            DeliveryDispatchProcessStatus::from_ord(2).unwrap(),
            DeliveryDispatchProcessStatus::FAILED
        );
        assert_eq!(CommandJournalStatus::FAILED.to_ord(), 3);
        assert_eq!(CommandJournalStatus::from_ord(0).unwrap(), CommandJournalStatus::RECEIVED);
        assert_eq!(CommandChannel::INTERNAL.to_ord(), 2);
        assert_eq!(CommandChannel::from_ord(1).unwrap(), CommandChannel::WORKER);
        assert_eq!(InboundEventStatus::DELIVERED.to_ord(), 1);
        assert_eq!(InboundEventStatus::from_ord(2).unwrap(), InboundEventStatus::FAILED);
        assert!(RestaurantStatus::from_ord(99).is_err());
    }
}
