//! Captain.Food infrastructure — adapters (ADR-0035).
//!
//! Implements the traits declared in `application::ports` / `application::queries` using real I/O:
//! `persistence/` (the `PgEventStore` write adapter appending to `domain_events`, plus sqlx read-model
//! repos over the materialized projection tables, ADR-0040) and
//! `projection/` (the app-layer projection worker that folds `domain_events` into those tables via the
//! hand-written `…Compute` projectors) and `integrations/` (the Anti-Corruption Layer — today the
//! SIRENE prospect sync; later HubRise/Stripe/delivery, incl. recording inbound facts). Depends on
//! `application` + `domain`; referencing both proves the infrastructure → application, domain edges.

pub mod integrations;
pub mod persistence;
pub mod process_manager;
pub mod projection;

pub use integrations::google::{FailClosedGoogleOwnershipVerifier, UnverifiedGbpOrderLinkProbe};
// Stripe/HubRise webhook adapters moved to their own crates (`crates/adapters/*`, ADR-20260718-213352).
pub use integrations::payments::FailClosedPaymentGateway;
pub use integrations::supabase_auth::FailClosedAuthProviderGateway;
pub use integrations::inbound_drain_worker::{InboundDrainSummary, InboundEventsDrainWorker};
pub use integrations::retention_sweep_worker::{RetentionSweepSummary, RetentionSweepWorker};
pub use integrations::sync_sirene_worker::{SireneSyncSummary, SireneSyncWorker};
pub use persistence::{
    AppendedEvent, EventBus, OperationStatusBus, OperationUpdate, PgCartRepository,
    PgCatalogRepository, PgCommandJournal, PgCustomerRepository, PgDeliveryRepository, PgEventStore,
    PgInboundEvents, PgOrderRepository, PgPricingPolicyRepository, PgProspectionRepository,
    PgRefundQueueRepository, PgRestaurantRepository, PgUberEstimationPolicyRepository,
    PgUberSplitPolicyRepository,
};
pub use process_manager::{ProcessManagerRunner, ProcessManagerStatus};
pub use projection::{ProjectionStatus, ProjectionWorker};
