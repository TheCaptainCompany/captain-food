//! Ports — traits the infrastructure implements (Ports & Adapters, ADR-0035). A use case that needs I/O
//! depends on one of these, never on a concrete adapter. Referencing `domain` here proves the
//! application → domain edge at compile time.

use async_trait::async_trait;
use domain::generated::events::DomainEvent;
use domain::generated::scalars::{GbpLinkStatus, WebUrl};
use domain::shared::{errors::DomainError, identifiers::RestaurantId};

/// Acting user + correlation for the event envelope (ADR-0041). The actor who performed a change is
/// ENVELOPE metadata on `domain_events` (`user_id`/`user_type`/`correlation_id`/`cause_id`), never a
/// business-payload field.
#[derive(Debug, Clone)]
pub struct Actor {
    pub user_id: uuid::Uuid,
    /// `UserType` TEXT value, stored verbatim (ADR-20260728).
    pub user_type: String,
    pub correlation_id: uuid::Uuid,
    pub cause_id: Option<uuid::Uuid>,
}

/// Message prefix carried by the [`DomainError::Invariant`] an [`EventStore::append`] returns when it
/// loses the optimistic-concurrency race (UNIQUE(stream_name, version)). Shared between the adapter
/// (which builds it via [`version_conflict`]) and the command handlers (which recognize it via
/// [`is_version_conflict`], e.g. to treat a replayed creation command as idempotent).
pub const VERSION_CONFLICT_PREFIX: &str = "version conflict";

/// Build the canonical optimistic-concurrency failure for `stream_name` at `expected_version`.
pub fn version_conflict(stream_name: &str, expected_version: i64) -> DomainError {
    DomainError::Invariant(format!(
        "{VERSION_CONFLICT_PREFIX}: stream '{stream_name}' is past version {expected_version}"
    ))
}

/// Whether `err` is the optimistic-concurrency failure produced by [`version_conflict`].
pub fn is_version_conflict(err: &DomainError) -> bool {
    matches!(err, DomainError::Invariant(msg) if msg.starts_with(VERSION_CONFLICT_PREFIX))
}

/// Write-side port: append business events to the `domain_events` log (CQRS-light, ADR-0035). Command
/// handlers depend on this trait; the Postgres adapter lives in `infrastructure`.
#[async_trait]
pub trait EventStore: Send + Sync {
    /// Append `events` to `stream_name`, expecting it to currently be at `expected_version`
    /// (0 = new stream). Optimistic concurrency via UNIQUE(stream_name, version): a version clash →
    /// Err([`version_conflict`]). Returns the stream's new version.
    async fn append(
        &self,
        stream_name: &str,
        expected_version: i64,
        events: &[DomainEvent],
        actor: &Actor,
    ) -> Result<i64, DomainError>;

    /// Load a stream's events in version order plus its current version (`0` for an empty/unknown
    /// stream). Command handlers rehydrate the aggregate state from this (write-side fold), then append
    /// at the returned version so a concurrent writer conflicts instead of double-applying.
    async fn load(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError>;
}

/// Google Business Profile ownership-proof verification (ADR-0019: "delegate ownership proof to
/// Google"). `ClaimRestaurantListing` / `OptOutRestaurantListing` carry a `googleOwnershipProof`; the
/// backend must validate it server-side before accepting — a `false` maps to
/// `errors.yaml#/ListingOwnershipNotVerified`. The real adapter calls Google; until it lands the
/// composition root injects a fail-closed stand-in (never silently accepts).
#[async_trait]
pub trait GoogleOwnershipVerifier: Send + Sync {
    /// Whether `proof` establishes that the caller owns `restaurant_id`'s Google Business Profile.
    async fn verify(&self, restaurant_id: RestaurantId, proof: &str) -> Result<bool, DomainError>;
}

/// GBP 'Order online' link probe (ADR-0021): `VerifyGoogleBusinessProfileOrderLink` pings the
/// configured `{slug}.captain.food` link and RECORDS the observed status. The adapter owns the ping;
/// the handler only records the reported fact.
#[async_trait]
pub trait GbpOrderLinkProbe: Send + Sync {
    /// Observe the live state of the configured link (`VERIFIED` when it answers as expected).
    async fn probe(&self, url: &WebUrl) -> Result<GbpLinkStatus, DomainError>;
}

// The WRAPPED auth provider port (Supabase Auth, ADR-0015) is GENERATED from the service catalog now
// (issue #50): `crate::generated::services::IdentityService` replaced the hand-written
// `AuthProviderGateway` (services.yaml `identity`). Invalid/expired verifications are the canonical
// typed rejections RAISED BY THE ADAPTER (`InvalidVerificationCode` / `InvalidVerificationToken` /
// `VerificationCodeExpired`), and `verify_email_token`'s output reports the PROVEN email — the
// linked email is never taken from client input.

/// Read-side port: the query handlers resolve restaurants through this. In V0 the adapter reads the
/// `View_Restaurant` SQL view over `domain_events` (ADR-0035, decision 2).
#[async_trait]
pub trait RestaurantRepository: Send + Sync {
    /// Whether a restaurant with this id is visible in the read model.
    async fn exists(&self, id: RestaurantId) -> Result<bool, DomainError>;
}

// The payment / delivery-partner ports are GENERATED from the service catalog now (issue #26,
// ADR-20260719-214500): `crate::generated::services::{PaymentService, DeliveryService}` replaced the
// hand-written `PaymentGateway` / `DeliveryPartner` traits (services.yaml `payment` / `delivery`).
// The Stripe correlation ids the webhook ACL reads back (`orderId`/`restaurantId`/`cartId`) travel on
// the `ServiceCallMeta` ENVELOPE — never in the spec-declared operation input.

/// No-op [`crate::generated::services::DeliveryService`] stand-in until the avelo37 ACL lands: the
/// offer is LOGGED (so a pending dispatch is observable, mirroring the runner's skip log) and
/// reported successful — the job stays PENDING on its stream, open to independent riders, and the run
/// row's OFFERED/FAILED statuses flag the follow-up (FAILED = the bounded re-offer cap was exhausted,
/// ADR-20260720-004556).
pub struct NoopDeliveryService;

#[async_trait]
impl crate::generated::services::DeliveryService for NoopDeliveryService {
    async fn offer_job(
        &self,
        input: crate::generated::services::DeliveryOfferJobInput,
        _meta: &crate::generated::services::ServiceCallMeta,
    ) -> Result<(), DomainError> {
        tracing::warn!(
            delivery_job_id = %input.job.delivery_job_id.0,
            order_id = %input.job.order_id.0,
            "delivery-partner[noop]: job offered nowhere -- the avelo37 ACL is the integration \
             workstream's; independent riders can still accept from the job stream"
        );
        Ok(())
    }
}

/// The validated, server-priced checkout PlaceOrderProcess freezes onto
/// `events.yaml#/PaymentIntentCreated` when it creates the PaymentIntent — everything
/// `events.yaml#/OrderPlaced` + `events.yaml#/CartCheckedOut` need beyond the inbound `PaymentCaptured`
/// fact. It is a generated value object (`entities.yaml#/CheckoutSnapshot`) carried ON the event and
/// re-exported here as the single source of truth. The capture leg reads it BACK from the
/// `Payment-<intentId>` stream (ADR-20260719-193500) — the log alone; the interim
/// `CheckoutSnapshotSource` port this snapshot used to flow through is retired.
pub use domain::generated::entities::CheckoutSnapshot;
