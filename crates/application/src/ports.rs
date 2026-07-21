//! Ports — traits the infrastructure implements (Ports & Adapters, ADR-0035). A use case that needs I/O
//! depends on one of these, never on a concrete adapter. Referencing `domain` here proves the
//! application → domain edge at compile time.

use async_trait::async_trait;
use domain::generated::events::DomainEvent;
use domain::generated::scalars::{
    DialingCode, EmailAddress, EmailVerificationToken, ExternalReference, GbpLinkStatus, Locale,
    NationalPhoneNumber, OtpCode, WebUrl,
};
use domain::shared::{errors::DomainError, identifiers::RestaurantId};

/// Acting user + correlation for the event envelope (ADR-0041). The actor who performed a change is
/// ENVELOPE metadata on `domain_events` (`user_id`/`user_type`/`correlation_id`/`cause_id`), never a
/// business-payload field.
#[derive(Debug, Clone)]
pub struct Actor {
    pub user_id: uuid::Uuid,
    /// `UserType` ordinal (enums are stored as declaration-order integers, ADR-0037).
    pub user_type: i32,
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

/// Outcome of a phone-OTP verification by the wrapped auth provider (Supabase Auth, ADR-0015).
/// `Verified` carries the provider's user reference (`authRef`) so the domain can link the identity.
#[derive(Debug, Clone, PartialEq)]
pub enum PhoneOtpCheck {
    /// The code matched — the provider resolved/created its user and reports its reference.
    Verified { auth_ref: ExternalReference },
    /// The code does not match → `errors.yaml#/InvalidVerificationCode`.
    Invalid,
    /// The code is past its validity window → `errors.yaml#/VerificationCodeExpired`.
    Expired,
}

/// Outcome of an email magic-link token verification by the wrapped auth provider (ADR-0015).
#[derive(Debug, Clone, PartialEq)]
pub enum EmailTokenCheck {
    /// The token verified server-side — the provider reports WHICH email it proves.
    Verified { email: EmailAddress },
    /// The token failed verification → `errors.yaml#/InvalidVerificationToken`.
    Invalid,
    /// The token is past its validity window → `errors.yaml#/VerificationCodeExpired`.
    Expired,
}

/// The WRAPPED auth provider (Supabase Auth behind our GraphQL, ADR-0015): passwordless phone-OTP and
/// email magic-link identity. The Customer command handlers stay pure and free of the Supabase SDK —
/// this port IS the ACL boundary; the `supabase-acl` adapter (infrastructure) owns the actual HTTP/SDK
/// calls, Twilio SMS delivery and token semantics. Until it lands the composition root injects a
/// fail-closed stand-in (sends error, verifications report `Invalid` — never silently accept).
#[async_trait]
pub trait AuthProviderGateway: Send + Sync {
    /// Send an SMS OTP to this phone (Twilio via Supabase; mock in dev), localized by `locale`.
    async fn send_phone_otp(
        &self,
        dialing_code: &DialingCode,
        national_number: &NationalPhoneNumber,
        locale: Option<&Locale>,
    ) -> Result<(), DomainError>;

    /// Verify an SMS OTP for this phone with the provider.
    async fn verify_phone_otp(
        &self,
        dialing_code: &DialingCode,
        national_number: &NationalPhoneNumber,
        code: &OtpCode,
    ) -> Result<PhoneOtpCheck, DomainError>;

    /// Email a magic link to verify/link `email`, localized by the customer's STORED `locale`.
    async fn send_email_magic_link(
        &self,
        email: &EmailAddress,
        locale: Option<&Locale>,
    ) -> Result<(), DomainError>;

    /// Verify a returned magic-link token server-side with the provider.
    async fn verify_email_token(
        &self,
        token: &EmailVerificationToken,
    ) -> Result<EmailTokenCheck, DomainError>;
}

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
        eprintln!(
            "delivery-partner[noop]: job {} (order {}) offered nowhere — the avelo37 ACL is the \
             integration workstream's; independent riders can still accept from the job stream",
            input.job.delivery_job_id.0, input.job.order_id.0
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
