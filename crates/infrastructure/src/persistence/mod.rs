//! Persistence adapters (sqlx over Postgres): read-model repositories over the materialized projection
//! tables (ADR-0040) plus the shared row↔SQL mapping helpers they and the projection worker reuse.

pub mod cart;
pub mod cart_store;
pub mod catalog;
pub mod catalog_store;
pub mod enum_sql;
pub mod event_store;
pub mod order;
pub mod order_tracking_store;
pub mod prospection;
pub mod prospection_store;
pub mod referential;
pub mod restaurant;
pub mod restaurant_store;

pub use cart::PgCartRepository;
pub use catalog::PgCatalogRepository;
pub use event_store::PgEventStore;
pub use order::PgOrderRepository;
pub use prospection::PgProspectionRepository;
pub use referential::{
    PgPricingPolicyRepository, PgUberEstimationPolicyRepository, PgUberSplitPolicyRepository,
};
pub use restaurant::PgRestaurantRepository;

use domain::shared::errors::DomainError;

/// Map any adapter-level failure (sqlx, serde, parsing) onto the repository variant of [`DomainError`],
/// so read ports never leak the adapter's error types.
pub(crate) fn db_err(e: impl std::fmt::Display) -> DomainError {
    DomainError::Repository(e.to_string())
}
