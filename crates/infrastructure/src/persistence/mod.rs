//! Persistence adapters (sqlx over Postgres): read-model repositories over the materialized projection
//! tables (ADR-0040) plus the shared row↔SQL mapping helpers they and the projection worker reuse.

pub mod cart;
pub mod auth_sessions;
pub mod cart_store;
pub mod catalog;
pub mod catalog_store;
pub mod customer;
pub mod customer_credit_balance;
pub mod customer_credit_balance_store;
pub mod customer_store;
pub mod delivery;
pub mod delivery_partner_availability;
pub mod delivery_satisfaction;
pub mod enum_sql;
pub mod event_bus;
pub mod event_store;
pub mod event_wake;
pub mod mailbox_wake;
pub mod mailbox_lanes;
pub mod mailbox_store;
pub mod order;
pub mod order_conversation;
pub mod order_conversation_store;
pub mod order_tracking_store;
pub mod prospection;
pub mod prospection_store;
pub mod reclamation;
pub mod referential;
pub mod refund_queue;
pub mod restaurant;
pub mod restaurant_store;
pub mod rider;
pub mod rider_restriction_store;
pub mod rider_store;
pub mod scope_membership_store;
pub mod runtime_posture;
pub mod slug_alias_store;
pub mod sms_send_quota;
pub mod slug_reservation;
pub mod auth_subject_reservation;

pub use auth_sessions::PgAuthSessionStore;
pub use sms_send_quota::PgSmsQuotaStore;
pub use cart::PgCartRepository;
pub use catalog::PgCatalogRepository;
pub use customer::PgCustomerRepository;
pub use rider::PgRiderRepository;
pub use customer_credit_balance::PgCustomerCreditRepository;
pub use slug_reservation::PgSlugReservationRepository;
pub use auth_subject_reservation::PgAuthSubjectReservationRepository;
pub use delivery::PgDeliveryRepository;
pub use delivery_satisfaction::PgDeliverySatisfactionRepository;
pub use event_bus::{AppendedEvent, EventBus};
pub use event_store::PgEventStore;
pub use event_wake::{spawn_event_listener, EventWake, EventWaiter, EVENT_CHANNEL};
pub use order::PgOrderRepository;
pub use order_conversation::PgOrderConversationRepository;
// The Postgres PM state stores are GENERATED from specs/database/tables/process_managers.yaml
// (issue #27); re-exported here so the stable `persistence::Pg…State` paths survive the move.
pub use crate::generated::pm_state::{
    PgCartBindingState, PgDeliveryDispatchState, PgPaymentProcessState, PgRefundProcessState,
};
pub use prospection::PgProspectionRepository;
pub use referential::PgDispatchStrategy;
pub use referential::{
    PgPricingPolicyRepository, PgUberEstimationPolicyRepository, PgUberSplitPolicyRepository,
};
pub use delivery_partner_availability::PgDeliveryPartnerAvailabilityRepository;
pub use reclamation::PgReclamationRepository;
pub use refund_queue::PgRefundQueueRepository;
pub use restaurant::PgRestaurantRepository;

use domain::shared::errors::DomainError;

/// Map any adapter-level failure (sqlx, serde, parsing) onto the repository variant of [`DomainError`],
/// so read ports never leak the adapter's error types.
pub(crate) fn db_err(e: impl std::fmt::Display) -> DomainError {
    DomainError::Repository(e.to_string())
}
