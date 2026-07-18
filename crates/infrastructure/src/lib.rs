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
pub mod projection;

pub use integrations::google::{FailClosedGoogleOwnershipVerifier, UnverifiedGbpOrderLinkProbe};
pub use integrations::hubrise::{
    verify_hubrise_signature, HubRiseCallback, HubRiseSignatureError, HUBRISE_SIGNATURE_HEADER,
    HUBRISE_WEBHOOK_SECRET_ENV,
};
pub use integrations::stripe::{
    verify_signature as verify_stripe_signature, SignatureError as StripeSignatureError,
    StripeEvent, StripeIngestOutcome, StripeWebhookIngestor, STRIPE_WEBHOOK_SECRET_ENV,
};
pub use integrations::supabase_auth::FailClosedAuthProviderGateway;
pub use integrations::sync_sirene_worker::{SireneSyncSummary, SireneSyncWorker};
pub use persistence::{
    PgCartRepository, PgCatalogRepository, PgCustomerRepository, PgEventStore, PgOrderRepository,
    PgPricingPolicyRepository, PgProspectionRepository, PgRestaurantRepository,
    PgUberEstimationPolicyRepository, PgUberSplitPolicyRepository,
};
pub use projection::{ProjectionStatus, ProjectionWorker};
