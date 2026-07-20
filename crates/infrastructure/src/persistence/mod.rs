//! Persistence adapters (sqlx over Postgres): read-model repositories over the materialized projection
//! tables (ADR-0040) plus the shared row↔SQL mapping helpers they and the projection worker reuse.

pub mod cart;
pub mod cart_store;
pub mod catalog;
pub mod catalog_store;
pub mod command_journal;
pub mod customer;
pub mod customer_store;
pub mod delivery;
pub mod enum_sql;
pub mod event_bus;
pub mod event_store;
pub mod inbound_events;
pub mod order;
pub mod order_tracking_store;
pub mod pm_state;
pub mod prospection;
pub mod prospection_store;
pub mod referential;
pub mod refund_queue;
pub mod restaurant;
pub mod restaurant_store;
pub mod status_bus;

pub use cart::PgCartRepository;
pub use catalog::PgCatalogRepository;
pub use command_journal::PgCommandJournal;
pub use customer::PgCustomerRepository;
pub use delivery::PgDeliveryRepository;
pub use event_bus::{AppendedEvent, EventBus};
pub use event_store::PgEventStore;
pub use inbound_events::PgInboundEvents;
pub use order::PgOrderRepository;
pub use pm_state::{
    PgCartBindingState, PgDeliveryDispatchState, PgPaymentProcessState, PgRefundProcessState,
};
pub use status_bus::{OperationStatusBus, OperationUpdate};
pub use prospection::PgProspectionRepository;
pub use referential::{
    PgPricingPolicyRepository, PgUberEstimationPolicyRepository, PgUberSplitPolicyRepository,
};
pub use refund_queue::PgRefundQueueRepository;
pub use restaurant::PgRestaurantRepository;

use domain::shared::errors::DomainError;

/// Map any adapter-level failure (sqlx, serde, parsing) onto the repository variant of [`DomainError`],
/// so read ports never leak the adapter's error types.
pub(crate) fn db_err(e: impl std::fmt::Display) -> DomainError {
    DomainError::Repository(e.to_string())
}
