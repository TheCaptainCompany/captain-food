//! Ports — traits the infrastructure implements (Ports & Adapters, ADR-0035). A use case that needs I/O
//! depends on one of these, never on a concrete adapter. Referencing `domain` here proves the
//! application → domain edge at compile time.

use async_trait::async_trait;
use domain::catalog_as_of::AsOfCatalog;
use domain::generated::events::DomainEvent;
use domain::generated::scalars::{CatalogId, GbpLinkStatus, WebUrl};
use domain::shared::{errors::DomainError, identifiers::RestaurantId};

/// Acting user + correlation for the event envelope (ADR-0041). The actor who performed a change is
/// ENVELOPE metadata on `domain_events` (`user_id`/`user_type`/`correlation_id`/`cause_id`), never a
/// business-payload field.
#[derive(Debug, Clone)]
pub struct Actor {
    pub user_id: uuid::Uuid,
    /// `UserType` TEXT value, stored verbatim (ADR-20260728).
    pub user_type: String,
    /// The RESOLVED domain identity of the acting principal (#235 consequence B,
    /// PROP-20260728-135632): for a CUSTOMER, the CustomerId resolved from the auth subject via
    /// `by_auth_ref` at the edge — the value `requires.acting` compares against the aggregate's
    /// folded participants. `None` for roles whose bridge has not landed (RESTAURANT/RIDER — #144)
    /// and for system/worker actors. NEVER the raw auth subject: those are different value spaces,
    /// and comparing them would silently always deny.
    pub domain_id: Option<uuid::Uuid>,
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

/// Message prefix carried by the [`DomainError::Invariant`] a payment gateway returns when it
/// refuses a request DETERMINISTICALLY — a bad payment method, a reused idempotency key, bad
/// credentials. Terminal on both dispatch arms: retrying can never succeed.
///
/// **Why this is a shared const and not five `format!` literals** (#623 M3, measured rather than
/// assumed). Before this change the prefix was minted at five sites and matched at **zero** — the
/// experiment was to rename it in `stripe-adapter` and see what went red: `crates/application`'s
/// 363 unit tests all passed, and the only failures were the adapter's own three tests, which
/// simply restate the literal they mint. So the token carried no machine meaning at all; it was
/// prose that happened to sit in a durable row.
///
/// [`crate::ports::payment_gateway_refused`] is what gives it meaning: the mailbox now READS it to
/// attribute a failed command (`infrastructure::mailbox::attribution`), so the mint sites and the
/// reader must agree, and a shared builder/reader pair is the cheapest place the compiler can help.
/// The stronger form — a typed gateway-failure variant on `DomainError`, where the status is a
/// field instead of something to parse — is
/// [#625](https://github.com/TheCaptainCompany/captain-food/issues/625) and is deliberately not
/// this change.
pub const PAYMENT_GATEWAY_REFUSED_PREFIX: &str = "PaymentGatewayRefused";

/// Build the canonical deterministic-gateway-refusal error.
///
/// The `(HTTP {status})` group sits AFTER the first colon on purpose: [`rejection_code`] splits an
/// `Invariant` message on `:` to read its errors.yaml code, so a status folded into the head would
/// make the code unreadable the day
/// [#625](https://github.com/TheCaptainCompany/captain-food/issues/625) catalogues this prefix —
/// a trap that would surface as a silently wrong verdict, not as a compile error.
///
/// `detail` is the gateway's own prose. It reaches the LOG only: the mailbox writes
/// [`payment_gateway_refused_status`] into the journal row and nothing else, because a provider
/// message can contain the API key (`"Invalid API Key provided: sk_test_…"`, #623).
pub fn payment_gateway_refused(status: Option<u16>, detail: &str) -> DomainError {
    DomainError::Invariant(match status {
        Some(s) => format!("{PAYMENT_GATEWAY_REFUSED_PREFIX}: (HTTP {s}) {detail}"),
        None => format!("{PAYMENT_GATEWAY_REFUSED_PREFIX}: {detail}"),
    })
}

/// Whether `err` is a deterministic gateway refusal built by [`payment_gateway_refused`].
pub fn is_payment_gateway_refused(err: &DomainError) -> bool {
    matches!(err, DomainError::Invariant(msg) if msg.starts_with(PAYMENT_GATEWAY_REFUSED_PREFIX))
}

/// The errors.yaml code a card-type decline carries. CATALOGUED, unlike
/// [`PAYMENT_GATEWAY_REFUSED_PREFIX`] — and that difference is load-bearing: catalogue membership is
/// what `verdict_of_error` uses to tell a REJECTED business outcome from a FAILED defect, so these
/// two prefixes travel down different arms and must never be conflated.
pub const PAYMENT_DECLINED_CODE: &str = "PaymentDeclined";

/// Build the canonical card-decline rejection, carrying the gateway's HTTP status in the SAME
/// `(HTTP n)` group [`payment_gateway_refused`] uses.
///
/// Why the status is here at all (#623 review, `young`): the decline arm used to record
/// `{"code":"PaymentDeclined","context":{}}` once its provider prose was dropped, which answers
/// "was it declined?" and not "declined how?" — a downgrade on the money path, where the operator's
/// next question is always which of the gateway's refusal classes this was. Encoding it structurally
/// is what lets the row say `402` without saying a word the gateway wrote.
pub fn payment_declined(status: Option<u16>, detail: &str) -> DomainError {
    DomainError::Invariant(match status {
        Some(s) => format!("{PAYMENT_DECLINED_CODE}: (HTTP {s}) {detail}"),
        None => format!("{PAYMENT_DECLINED_CODE}: {detail}"),
    })
}

/// Whether `err` is a card decline built by [`payment_declined`].
pub fn is_payment_declined(err: &DomainError) -> bool {
    matches!(err, DomainError::Invariant(msg) if msg.starts_with(PAYMENT_DECLINED_CODE))
}

/// Read back the HTTP status one of this module's gateway builders encoded, if it had one.
///
/// **Code-agnostic on purpose.** It matches the `(HTTP n)` group immediately after the first
/// `": "`, whatever prefix precedes it, so one reader serves both the FAILED refusal
/// ([`payment_gateway_refused`]) and the REJECTED decline ([`payment_declined`]). A per-prefix
/// reader would have to be remembered each time a builder is added, and the thing that gets
/// forgotten is the one that ships a row with no status in it.
///
/// The round trip is pinned by a test in this module for BOTH builders: reader and builders are one
/// contract, and the whole reason the journal row can name `401` (our credentials) versus `400`
/// (our request) versus `402` (the customer's card) without carrying a byte of the gateway's prose.
/// It anchors on the FIRST colon rather than searching the message, so a gateway's own prose
/// containing `(HTTP 500)` further along cannot be read back as our status.
pub fn gateway_status_of(err: &DomainError) -> Option<u16> {
    let DomainError::Invariant(msg) = err else { return None };
    let rest = msg.split_once(':')?.1.strip_prefix(" (HTTP ")?;
    let end = rest.find(')')?;
    rest[..end].parse::<u16>().ok()
}

#[cfg(test)]
mod gateway_refusal_tests {
    use super::*;

    /// Builder and reader are ONE contract. A test that only checked the reader against a
    /// hand-written string would keep passing while the builder drifted away from it.
    #[test]
    fn the_status_round_trips_through_the_message() {
        for status in [400u16, 401, 402, 409, 429] {
            let err = payment_gateway_refused(Some(status), "stripe create_payment_intent refused");
            assert!(is_payment_gateway_refused(&err), "{err:?}");
            assert_eq!(gateway_status_of(&err), Some(status), "{err:?}");
        }
    }

    /// The SAME reader must serve the decline builder, or the decline arm silently records no
    /// status while looking as if it does.
    #[test]
    fn the_status_round_trips_for_a_decline_too() {
        for status in [402u16, 400] {
            let err = payment_declined(Some(status), "Your card was declined. (card_declined)");
            assert!(is_payment_declined(&err), "{err:?}");
            assert!(!is_payment_gateway_refused(&err), "a decline is not a refusal: {err:?}");
            assert_eq!(gateway_status_of(&err), Some(status), "{err:?}");
        }
    }

    /// A decline stays CATALOGUED with a status attached — the same trap as
    /// [`the_code_stays_readable_with_a_status_attached`], and here it is not hypothetical: this
    /// code IS in `errors.yaml` today, so a status folded into the head would flip the row from
    /// REJECTED to FAILED the moment it landed.
    #[test]
    fn a_decline_stays_readable_as_its_catalogued_code() {
        let err = payment_declined(Some(402), "Your card was declined.");
        assert_eq!(crate::commands::rejection_code(&err), Some(PAYMENT_DECLINED_CODE));
    }

    /// The status must be OUR encoding, never a number found in the gateway's prose. Stripe's
    /// message is provider-controlled, so this is the difference between a reader and a guesser.
    #[test]
    fn a_status_shaped_group_later_in_the_prose_is_not_read_back() {
        let err = payment_declined(None, "issuer replied (HTTP 500) while authorising");
        assert!(is_payment_declined(&err));
        assert_eq!(gateway_status_of(&err), None, "{err:?}");
    }

    /// No status is a real case (the fail-closed stand-in never made a call), and it must read as
    /// ABSENT rather than as a guess.
    #[test]
    fn a_refusal_with_no_call_carries_no_status() {
        let err = payment_gateway_refused(None, "capture gateway not configured");
        assert!(is_payment_gateway_refused(&err));
        assert_eq!(gateway_status_of(&err), None);
    }

    /// The prefix must stay readable by [`rejection_code`]: the day #625 catalogues it, the
    /// catalogue lookup has to find `PaymentGatewayRefused`, not `PaymentGatewayRefused (HTTP 401)`.
    /// Getting this wrong flips a verdict silently, which is the one failure mode this whole change
    /// is fenced against.
    #[test]
    fn the_code_stays_readable_with_a_status_attached() {
        let err = payment_gateway_refused(Some(401), "stripe create_payment_intent refused");
        assert_eq!(crate::commands::rejection_code(&err), Some(PAYMENT_GATEWAY_REFUSED_PREFIX));
    }

    /// A neighbouring error must not be mistaken for a gateway refusal.
    #[test]
    fn an_unrelated_invariant_is_not_a_gateway_refusal() {
        let err = DomainError::Invariant("cart is empty".into());
        assert!(!is_payment_gateway_refused(&err));
        assert!(!is_payment_declined(&err));
        assert_eq!(gateway_status_of(&err), None);
    }
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

/// The as-of price-fold read authority (PROP-20260831-134539 §2.1 step 3, slice 2 of "the priced
/// quote token"; binds to `specs/ordering/rules.yaml#/ServerPriceAuthority`): reconstruct a catalog's
/// prices at a FIXED past coordinate, never the live head. `version` is REQUIRED — never `Option` —
/// because "as-of nothing in particular" is not a read this port offers; there is deliberately no
/// `latest_version()` on this trait (vernon CATCH: that would let a handler quietly ask for HEAD
/// through the back door). The returned [`AsOfCatalog`] is a RESOLVED VALUE: it holds no repo handle,
/// so a caller cannot use it to keep reading past the coordinate it was built at.
///
/// DARK in this slice: no production caller. The Postgres adapter
/// (`crates/infrastructure/src/persistence/...`) reads through a RANGE read on the event store
/// (`WHERE stream_name = $1 AND version <= $2`), decoding the event PAYLOAD only — never retaining
/// the envelope `user_id` (ADR-0041) — and reuses `domain::catalog::stream`/`CATEGORY` for the stream
/// name (never a fourth `format!("Catalog-{}")` literal).
#[async_trait]
pub trait AsOfPriceAuthority: Send + Sync {
    /// The catalog's prices at `version` (inclusive), reconstructed from the `Catalog-<catalogId>`
    /// stream up to and including that coordinate.
    async fn as_of(&self, catalog_id: CatalogId, version: i64) -> Result<AsOfCatalog, DomainError>;
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
