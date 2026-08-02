//! Captain.Food infrastructure — adapters (ADR-0035).
//!
//! Implements the traits declared in `application::ports` / `application::queries` using real I/O:
//! `persistence/` (the `PgEventStore` write adapter appending to `domain_events`, plus sqlx read-model
//! repos over the materialized projection tables, ADR-0040) and
//! `projection/` (the app-layer projection worker that folds `domain_events` into those tables via the
//! hand-written `…Compute` projectors) and `integrations/` (the Anti-Corruption Layer — today the
//! SIRENE prospect sync; later HubRise/Stripe/delivery, incl. recording inbound facts). Depends on
//! `application` + `domain`; referencing both proves the infrastructure → application, domain edges.

pub mod deletion;
pub mod generated;
pub mod integrations;
pub mod mailbox;
pub mod persistence;
pub mod process_manager;
pub mod projection;

pub use integrations::google::{FailClosedGoogleOwnershipVerifier, UnverifiedGbpOrderLinkProbe};
// Stripe/HubRise webhook adapters moved to their own crates (`crates/adapters/*`, ADR-20260718-213352).
pub use integrations::payments::FailClosedPaymentGateway;
pub use integrations::ovh_sms::OvhSmsClient;
pub use integrations::supabase_sms_hook;
pub use integrations::supabase_auth::{FailClosedIdentityService, SupabaseIdentityService};
pub use integrations::delivery_gateway::CompositeDeliveryGateway;
pub use integrations::delivery_offer_timeout_worker::DeliveryOfferTimeoutWorker;
pub use integrations::retention_sweep_worker::{RetentionSweepSummary, RetentionSweepWorker};
pub use integrations::sync_sirene_worker::{SireneSyncStatus, SireneSyncSummary, SireneSyncWorker};
pub use persistence::{
    spawn_event_listener, AppendedEvent, EventBus, EventWake, EventWaiter, OperationStatusBus,
    OperationUpdate, PgCartRepository,
    PgAuthSessionStore, PgCatalogRepository, PgCommandJournal, PgCustomerCreditRepository,
    PgCustomerRepository,
    PgDeliveryPartnerAvailabilityRepository, PgDeliveryRepository, PgDeliverySatisfactionRepository,
    PgEventStore, PgOrderConversationRepository, PgOrderRepository,
    PgPricingPolicyRepository,
    PgProspectionRepository, PgReclamationRepository, PgRefundQueueRepository,
    PgRestaurantRepository, PgSlugReservationRepository, PgUberEstimationPolicyRepository,
    PgUberSplitPolicyRepository,
};
pub use deletion::{DeletionEngine, DeletionEngineStatus};
pub use process_manager::{ProcessManagerRunner, ProcessManagerStatus};
pub use projection::{ProjectionStatus, ProjectionWorker};
