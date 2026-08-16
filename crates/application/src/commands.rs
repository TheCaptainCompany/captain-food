//! CQRS command handlers (write side, ADR-0035). Thin by design: rehydrate the aggregate state by
//! folding its stream (loaded through the [`EventStore`] port), enforce the invariants declared for
//! that message in `specs/actors.yaml` (`throws` → `specs/errors.yaml`), then append the declared
//! `emits` event(s) at the expected version. Ids are client/ACL-generated (ADR-0034), so creation
//! commands are idempotent: replaying one hits the UNIQUE(stream_name, version) guard and is absorbed
//! as an already-registered no-op instead of duplicating the fact.
//!
//! Rejections are STRUCTURED (`DomainError::Rejected { code, context }`, ADR-0046 follow-up): the
//! errors.yaml CODE plus the error's typed context as a JSON object whose keys are the errors.yaml
//! `context` field names (camelCase). The generated catalog (`domain::generated::errors`) owns the
//! localized `{placeholder}` message templates; the GraphQL layer maps a rejection onto
//! `extensions.code` + the interpolated message (error contract P-10). See [`reject`] /
//! [`rejection_code`].
//!
//! Cross-aggregate invariants that still lack a read port are explicit `TODO(invariant)` markers —
//! they are NOT silently skipped semantics, they are the documented gap.

use std::collections::HashSet;

use serde_json::json;

use domain::catalog::CatalogState;
use domain::customer::CustomerState;
use domain::generated::commands::{
    ActivateRestaurant, AddCatalogCategory, AddOptionList, AddProduct, ChangeLanguage,
    ChangeOrderAcceptanceMode, ChangeRestaurantListingStatus, ClaimRestaurantListing,
    ConfigureGoogleBusinessProfileOrderLink, ConfirmEmailVerification, ConfirmPhoneChange,
    ConfigureCatalogSlug, ConfigureRestaurantSlug, CreateCatalog, DeactivateRestaurant,
    DeleteRestaurantAccount,
    ImportCatalog, MarkProspectCold,
    MarkRestaurantAsFavorite, MarkRestaurantClosed, OptOutRestaurantListing, RecordProspectContact,
    RecordProspectReply, RegisterRestaurant, RegisterRestaurantAccount, RemoveCatalogCategory,
    RemoveCustomerAddress, RemoveOptionList, RemoveProduct, RemoveRestaurant,
    RequestEmailVerification, RequestPhoneChange, RequestPhoneVerification, SetCustomerAddress,
    SetCustomerPaymentMethod, SetCustomerPreferences, UnmarkRestaurantAsFavorite,
    UpdateCatalogCategory, UpdateCustomerInfo, UpdateOfferStock, UpdateOptionList, UpdateProduct,
    UpdateRestaurant, UpdateRestaurantAccount, UpdateRestaurantGoogleBusinessProfile, VerifyPhone,
    VerifyGoogleBusinessProfileOrderLink,
};
use domain::generated::entities::{CheckoutSnapshot, Money, PaymentBreakdown, Product, Stock};
use domain::generated::events::{
    CatalogCategoryAdded, CatalogCategoryRemoved, CatalogCategoryUpdated, CatalogCreated,
    CatalogSlugConfigured,
    CatalogImported, CustomerAddressRemoved, CustomerAddressSet, CustomerEmailVerified,
    CustomerIdentified, CustomerInfoUpdated, CustomerLanguageChanged, CustomerPaymentMethodSet,
    CustomerPhoneChanged, CustomerPreferencesSet, CustomerRegistered, DomainEvent, OfferStockUpdated,
    OptionListAdded, OptionListRemoved, OptionListUpdated, ProductAdded, ProductRemoved,
    ProductUpdated, ProspectContacted, ProspectMarkedCold, ProspectReplied,
    RestaurantAcceptanceModeChanged, RestaurantAccountDeleted, RestaurantAccountRegistered,
    RestaurantAccountUpdated, RestaurantActivated, RestaurantDeactivated, RestaurantFavorited,
    RestaurantGoogleBusinessProfileOrderLinkConfigured,
    RestaurantGoogleBusinessProfileOrderLinkVerified, RestaurantGoogleBusinessProfileUpdated,
    RestaurantListingClaimed, RestaurantListingOptedOut, RestaurantListingStatusChanged,
    RestaurantMarkedClosed, RestaurantRegistered, RestaurantRemoved,
    RestaurantSlugConfigured, RestaurantSlugReconfigured, RestaurantUnfavorited, RestaurantUpdated,
};
use domain::generated::scalars::{
    CatalogId, CurrencyCode, CustomerId, DialingCode, ExternalReference, NationalPhoneNumber,
    PhoneNumber, RestaurantAccountId, RestaurantId, RestaurantListingStatus, RestaurantStatus,
    StockStatus,
};
use domain::prospect::ProspectState;
use domain::restaurant::RestaurantState;
use domain::restaurant_account::RestaurantAccountState;
use domain::shared::errors::DomainError;

use crate::ports::{
    is_version_conflict, Actor, EventStore, GbpOrderLinkProbe, GoogleOwnershipVerifier,
};
use crate::queries::{
    CustomerReadRepository, ProspectFilter, ProspectionReadRepository, RestaurantReadRepository,
    SlugReservationRepository,
};

// --- Cart / Order / DeliveryJob / PlaceOrderProcess (checkout→order→delivery flow, ADR-0046 round 2) ---
use domain::cart::{CartState, MAX_LINE_QUANTITY};
use domain::delivery_job::DeliveryJobState;
use domain::generated::commands::{
    AcceptDelivery, AddCartLine, BindCartToCustomer, CancelDelivery,
    ChangeCartLineQuantity, CompleteDelivery, ConfirmPickup, DeclineDelivery, EscalateDelivery,
    PlaceOrder, PlaceReplacementOrder, RateOrder, RateRestaurant, RecordDeliverySatisfaction,
    RegisterRider, RemoveCartLine, ReportDeliveryIssue, RequestRefund, ResolveDeliveryIssue, TipOrder,
    UnassignDeliveryFromPartner, UpdateRiderInfo,
};
use domain::generated::entities::CartLineItem;
use domain::generated::events::{
    CartBoundToCustomer, CartLineAdded, CartLineQuantityChanged, CartLineRemoved, CartStarted,
    DeliveryAcceptedByRider, DeliveryCancelled, DeliveryCompleted,
    DeliveryDeclinedByRider, DeliveryEscalationRequested, DeliveryIssueReported, DeliveryIssueResolved,
    DeliveryPickedUp, DeliverySatisfactionRecorded,
    DeliveryUnassignedFromPartner, OrderPlaced, OrderRated, OrderTipped, PaymentIntentCreated,
    RefundRequested, RestaurantRated as RestaurantRatedEvent, RiderInfoUpdated, RiderRegistered,
};
use domain::generated::scalars::{
    CartId, CartStatus, CatalogItemAvailability, DeliveryJobId, DeliveryStatus, Mode,
    OrderAcceptanceMode, OrderId, OrderStatus, OptionId, PaymentProcessStatus, PaymentStatus,
    RiderId, RiderStatus, ServiceType, ServiceWindowVerdict, TipRecipient, Tipper,
};
use domain::order::OrderState;
use domain::rider::RiderState;

// Delivery partner self-registration (#61) — the DeliveryPartnerRegistration aggregate.
use domain::delivery_partner_registration::DeliveryPartnerRegistrationState;
use domain::generated::commands::{
    ApproveDeliveryPartnerAvailability, RegisterDeliveryPartnerAvailability,
    RevokeDeliveryPartnerAvailability,
};
use domain::generated::events::{
    DeliveryPartnerAvailabilityApproved, DeliveryPartnerAvailabilityRequested,
    DeliveryPartnerAvailabilityRevoked,
};
use domain::generated::scalars::{CityAvailabilityStatus, DeliveryPartnerRegistrationId};

// In-app order conversations (#129) — the Conversation aggregate (id = orderId).
use domain::conversation::ConversationState;
use domain::generated::commands::{
    EscalateToAdmin, MuteParticipant, OpenConversation, PostMessage, RecordMessageTranslation,
    UnmuteParticipant,
};
use domain::generated::events::{
    AdminInvitedToConversation, ConversationOpened, MessagePosted, MessageTranslationAdded,
    ParticipantMuted, ParticipantUnmuted,
};
use domain::generated::scalars::ConversationAuthorRole;

// Reclamations / customer claims (#151) — the Reclamation aggregate (id = reclamationId).
use domain::reclamation::{ReclamationState, ReclamationStatus};
use domain::generated::commands::{
    AttachReclamationEvidence, OpenReclamation, RejectReclamation, ReopenReclamation,
    ResolveReclamation,
};
use domain::generated::events::{
    ReclamationEvidenceAttached, ReclamationOpened, ReclamationRejected, ReclamationReopened,
    ReclamationResolved,
};
use domain::generated::scalars::{ReclamationId, ReclamationResolution};

// Customer store-credit ledger (#158) — the CustomerCredit aggregate (id = customerId).
use domain::customer_credit::CustomerCreditState;
use domain::generated::commands::{ConsumeCustomerCredit, GrantCustomerCredit};
use domain::generated::events::{CustomerCreditConsumed, CustomerCreditGranted};

use crate::generated::services::{
    IdentityRefreshSessionInput, IdentitySendEmailMagicLinkInput, IdentitySendPhoneOtpInput,
    IdentityService, IdentityStampCustomerClaimInput, IdentityVerifyEmailTokenInput,
    IdentityVerifyPhoneOtpInput, PaymentRequestInput, PaymentRequestOutput, PaymentService,
    ServiceCallMeta,
};
use crate::pm_state::{PaymentProcessRow, PaymentProcessStateStore};
use crate::queries::{CatalogReadRepository, OfferView};
use crate::repository::Repository;

// The mechanical "require + guard + append" lifecycle handlers are GENERATED from the specs
// (issue #23, ADR-20260721-093027) and re-exported here so call sites and the behaviour suite are
// unchanged; their seams (require_*/invalid_*/…_stream/canonical_predecessor) stay below as
// pub(crate) hand-written policy.
pub use crate::generated::handlers::{
    accept_order, cancel_order_by_customer, cancel_order_by_restaurant, change_rider_status,
    mark_order_delivered, mark_order_ready, reject_order, start_preparation,
    update_delivery_status,
};

/// Did a creation command actually create the aggregate, or was it already there?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Created {
    /// The stream was empty and the birth events were appended.
    Yes,
    /// The aggregate already existed under this id — nothing was written.
    No,
}

/// Create an aggregate if its stream does not exist yet (ADR-20260728-011344).
///
/// This replaces `idempotent_on_existing`, which answered "does this already exist?" by ATTEMPTING the
/// append and reading the resulting `UNIQUE (stream_name, version)` violation as success. That was
/// wrong twice over. It was expensive — Postgres writes the heap tuple and index entries *before* the
/// constraint fires, so every no-op left dead tuples in the largest table (a SIRENE sweep left ~200k
/// per week, which is what put the database's disk-IO budget on the floor). And it was **silent**: the
/// caller could not tell a real creation from a no-op, which is how `verify_phone` came to report
/// `created: true` for customers who already existed.
///
/// Here the question is asked before anything is written, and answered aggregate-agnostically: an
/// empty stream is version 0. No fold is needed — "does this stream exist" is not a domain question.
///
/// A version conflict is no longer swallowed. Reaching one now means a genuine race (someone created
/// the same aggregate between our load and our append), so it is reported as [`Created::No`] — correct,
/// because they created it and we did not — and left visible to the caller rather than disguised.
async fn create_if_absent(
    store: &dyn EventStore,
    stream_name: &str,
    events: &[DomainEvent],
    actor: &Actor,
) -> Result<Created, DomainError> {
    let (_existing, version) = store.load(stream_name).await?;
    if version > 0 {
        return Ok(Created::No);
    }
    match Repository::new(store).save(stream_name, 0, events, actor).await {
        Ok(_) => Ok(Created::Yes),
        // Lost the race. The aggregate exists — just not because of us.
        Err(e) if is_version_conflict(&e) => Ok(Created::No),
        Err(e) => Err(e),
    }
}

/// Build the canonical rejection for an `errors.yaml` invariant: the stable PascalCase CODE plus its
/// typed context as a JSON object (keys = the error's errors.yaml `context` fields, camelCase).
/// [`rejection_code`] is the matching reader; the GraphQL layer maps the rejection onto
/// `extensions.code` + the interpolated localized message (P-10).
pub(crate) fn reject(code: &str, context: serde_json::Value) -> DomainError {
    DomainError::rejected(code, context)
}

/// The errors.yaml code a command rejection carries, if this is one. Structured rejections carry it
/// first-class; the legacy `"<Code>: <detail>"` [`DomainError::Invariant`] shape (still produced by
/// interim adapters, e.g. the fail-closed payment stand-in) is parsed as before.
pub fn rejection_code(err: &DomainError) -> Option<&str> {
    match err {
        DomainError::Rejected { code, .. } => Some(code),
        DomainError::Invariant(msg) => msg.split(':').next().map(str::trim),
        DomainError::Repository(_) => None,
    }
}

/// The stream a Restaurant aggregate lives on.
fn restaurant_stream(id: &RestaurantId) -> String {
    format!("Restaurant-{}", id.0)
}

/// Rehydrate the Restaurant aggregate: fold its stream into the minimal write-side state and return it
/// with the stream's current version (the expected version for the next append).
async fn load_restaurant(
    store: &dyn EventStore,
    id: &RestaurantId,
) -> Result<(Option<RestaurantState>, i64), DomainError> {
    Repository::new(store).load::<RestaurantState>(*id).await
}

/// Rehydrate and require existence, or reject with `errors.yaml#/RestaurantNotFound`.
async fn require_restaurant(
    store: &dyn EventStore,
    id: &RestaurantId,
) -> Result<(RestaurantState, i64), DomainError> {
    let (state, version) = load_restaurant(store, id).await?;
    match state {
        Some(state) => Ok((state, version)),
        None => Err(reject("RestaurantNotFound", json!({ "restaurantId": id }))),
    }
}

/// The stream a RestaurantAccount aggregate lives on.
fn restaurant_account_stream(id: &RestaurantAccountId) -> String {
    format!("RestaurantAccount-{}", id.0)
}

/// Rehydrate the RestaurantAccount aggregate (fold + current version).
async fn load_restaurant_account(
    store: &dyn EventStore,
    id: &RestaurantAccountId,
) -> Result<(Option<RestaurantAccountState>, i64), DomainError> {
    Repository::new(store).load::<RestaurantAccountState>(*id).await
}

/// Rehydrate and require existence (a deleted account no longer exists), or reject with
/// `errors.yaml#/RestaurantAccountNotFound`.
async fn require_restaurant_account(
    store: &dyn EventStore,
    id: &RestaurantAccountId,
) -> Result<(RestaurantAccountState, i64), DomainError> {
    let (state, version) = load_restaurant_account(store, id).await?;
    match state {
        Some(state) => Ok((state, version)),
        None => Err(reject("RestaurantAccountNotFound", json!({ "restaurantAccountId": id }))),
    }
}

/// `errors.yaml#/InvalidCurrency`: an ISO 4217 code is exactly three ASCII uppercase letters (the
/// shape check catches "EURO"/"eur"; validating against the full ISO code LIST is reference data the
/// pricing referential owns, not a domain constant).
fn is_valid_iso4217(currency: &CurrencyCode) -> bool {
    currency.0.len() == 3 && currency.0.bytes().all(|b| b.is_ascii_uppercase())
}

/// Handle `commands.yaml#/RegisterRestaurantAccount` → emit `events.yaml#/RestaurantAccountRegistered`
/// on the new `RestaurantAccount-<id>` stream (actors.yaml, RestaurantAccount aggregate). Rejects a
/// malformed default currency (`InvalidCurrency`, ISO 4217 shape).
pub async fn register_restaurant_account(
    store: &dyn EventStore,
    cmd: RegisterRestaurantAccount,
    actor: &Actor,
) -> Result<(), DomainError> {
    // TODO(invariant): RefAlreadyUsed — reject when cmd.ref is already owned by another aggregate
    //                  (needs an external-reference read-model lookup).
    if !is_valid_iso4217(&cmd.default_currency) {
        return Err(reject("InvalidCurrency", json!({ "currency": cmd.default_currency })));
    }
    let stream_name = restaurant_account_stream(&cmd.restaurant_account_id);
    let event = DomainEvent::RestaurantAccountRegistered(RestaurantAccountRegistered {
        restaurant_account_id: cmd.restaurant_account_id,
        r#ref: cmd.r#ref,
        legal_name: cmd.legal_name,
        contact: cmd.contact,
        default_currency: cmd.default_currency,
        default_tax_rate: cmd.default_tax_rate,
        timezone: cmd.timezone,
    });
    create_if_absent(store, &stream_name, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/UpdateRestaurantAccount` → emit `events.yaml#/RestaurantAccountUpdated`
/// (replace semantics on the provided account-level fields). An update carrying nothing editable is
/// rejected (`errors.yaml#/NoEditableFieldProvided`).
pub async fn update_restaurant_account(
    store: &dyn EventStore,
    cmd: UpdateRestaurantAccount,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = require_restaurant_account(store, &cmd.restaurant_account_id).await?;
    let has_editable_field = cmd.legal_name.is_some()
        || cmd.contact.is_some()
        || cmd.default_tax_rate.is_some()
        || cmd.timezone.is_some();
    if !has_editable_field {
        return Err(reject("NoEditableFieldProvided", json!({})));
    }
    let stream_name = restaurant_account_stream(&cmd.restaurant_account_id);
    let event = DomainEvent::RestaurantAccountUpdated(RestaurantAccountUpdated {
        restaurant_account_id: cmd.restaurant_account_id,
        legal_name: cmd.legal_name,
        contact: cmd.contact,
        default_tax_rate: cmd.default_tax_rate,
        timezone: cmd.timezone,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/DeleteRestaurantAccount` → emit `events.yaml#/RestaurantAccountDeleted`
/// (the account is closed; the stream and its history remain, but the fold treats it as gone).
pub async fn delete_restaurant_account(
    store: &dyn EventStore,
    cmd: DeleteRestaurantAccount,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = require_restaurant_account(store, &cmd.restaurant_account_id).await?;
    let stream_name = restaurant_account_stream(&cmd.restaurant_account_id);
    let event = DomainEvent::RestaurantAccountDeleted(RestaurantAccountDeleted {
        restaurant_account_id: cmd.restaurant_account_id,
        reason: cmd.reason,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/RegisterRestaurant` → emit `events.yaml#/RestaurantRegistered` on the new
/// `Restaurant-<id>` stream (actors.yaml, Restaurant aggregate). `listingStatus` defaults to
/// NON_PARTNER when omitted (e.g. a Sirene/Google sync-seeded listing), per the command spec.
///
/// No slug, and no slug check (ADR-20260728-011344): a registration does not carry a storefront
/// address, so it cannot collide. Uniqueness moved to `configure_restaurant_slug`, arbitrated by a
/// write-side reservation with a real `UNIQUE` constraint rather than by a read-model lookup — the old
/// check queried the `Restaurant` projection with an unindexed `external_identifiers @> $1` scan, per
/// call, and was eventually consistent besides.
///
/// `_restaurants` is retained for the still-open `RefAlreadyUsed` invariant below, which needs an
/// external-reference lookup port.
pub async fn register_restaurant(
    store: &dyn EventStore,
    _restaurants: &dyn RestaurantReadRepository,
    cmd: RegisterRestaurant,
    actor: &Actor,
) -> Result<(), DomainError> {
    // The owning RestaurantAccount must exist when the location claims one — folded from ITS stream
    // (authoritative, race-free; same cross-aggregate read pattern as place_order).
    if let Some(account_id) = cmd.account_id {
        let (account_events, _) = store.load(&format!("RestaurantAccount-{}", account_id.0)).await?;
        if domain::restaurant_account::fold(&account_events).is_none() {
            return Err(reject("RestaurantAccountNotFound", json!({ "restaurantAccountId": account_id })));
        }
    }
    // TODO(invariant): RefAlreadyUsed — reject when cmd.ref is already owned by another aggregate
    //                  (needs an external-reference read-model lookup port).
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = DomainEvent::RestaurantRegistered(RestaurantRegistered {
        mode: cmd.mode,
        restaurant_id: cmd.restaurant_id,
        account_id: cmd.account_id,
        listing_status: cmd.listing_status.unwrap_or(RestaurantListingStatus::NON_PARTNER),
        r#ref: cmd.r#ref,
        external_identifiers: cmd.external_identifiers,
        display_name: cmd.display_name,
        contact: cmd.contact,
        website: cmd.website,
        tags: cmd.tags,
        margin_rate: cmd.margin_rate,
        cuisine_category: cmd.cuisine_category,
        uber_prices_opt_in: cmd.uber_prices_opt_in,
        address: cmd.address,
        location: cmd.location,
        timezone: cmd.timezone,
        preparation_time_minutes: cmd.preparation_time_minutes,
        opening_hours: cmd.opening_hours,
    });
    create_if_absent(store, &stream_name, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/ConfigureRestaurantSlug` → emit `events.yaml#/RestaurantSlugConfigured` on a
/// first configuration, `events.yaml#/RestaurantSlugReconfigured` on a rename, or NOTHING when the
/// requested label is already the current one (ADR-20260728-011344).
///
/// The aggregate decides which of the three it is, by folding its own stream. Note the shape: one
/// command, two possible facts, and a legitimate no-fact outcome — that no-op is a real event-sourcing
/// decision (`activate_restaurant` has the same shape), not an error to be swallowed.
///
/// Uniqueness is arbitrated by `slugs`, a write-side reservation with a real `UNIQUE` constraint —
/// never by the `Restaurant` projection, which is eventually consistent and would let two concurrent
/// claims both succeed. A label another restaurant RELEASED by renaming stays reserved, so its 301
/// cannot be hijacked.
///
/// Ordering: reserve BEFORE appending. A reservation with no event is a harmless orphan the owner can
/// re-drive by re-submitting; an event with no reservation would mean two restaurants believing they
/// own one host, which is unrecoverable without operator surgery.
pub async fn configure_restaurant_slug(
    store: &dyn EventStore,
    slugs: &dyn SlugReservationRepository,
    cmd: ConfigureRestaurantSlug,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_restaurant(store, &cmd.restaurant_id).await?;

    // Already our current address → nothing happened. No event, no error, no reservation churn.
    if state.slug.as_ref() == Some(&cmd.slug) {
        return Ok(());
    }

    if !slugs.reserve(cmd.slug.clone(), cmd.restaurant_id).await? {
        return Err(reject("SlugAlreadyTaken", json!({ "slug": cmd.slug })));
    }

    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = match state.slug {
        // A rename: carry the previous label so the alias read model can 301 from it, and release it
        // (release keeps the row — released never means reusable).
        Some(previous) => {
            slugs.release(previous.clone(), cmd.restaurant_id).await?;
            DomainEvent::RestaurantSlugReconfigured(RestaurantSlugReconfigured {
                restaurant_id: cmd.restaurant_id,
                slug: cmd.slug,
                previous_slug: previous,
            })
        }
        None => DomainEvent::RestaurantSlugConfigured(RestaurantSlugConfigured {
            restaurant_id: cmd.restaurant_id,
            slug: cmd.slug,
        }),
    };
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/ActivateRestaurant` → emit `events.yaml#/RestaurantActivated`. Idempotent
/// per actors.yaml: activating an already-ACTIVE restaurant is a no-op (no event, no error) — the
/// command ensures the ACTIVE state, it is not a toggle.
pub async fn activate_restaurant(
    store: &dyn EventStore,
    cmd: ActivateRestaurant,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_restaurant(store, &cmd.restaurant_id).await?;
    // No storefront address = no host a customer could reach (ADR-20260728-011344). Checked BEFORE the
    // already-ACTIVE short-circuit is irrelevant either way (an ACTIVE restaurant necessarily has one),
    // but checked before the append so a DRAFT can never go live address-less. Aggregate-local: the
    // answer is in the fold, so no read model is consulted and no race exists.
    if state.slug.is_none() {
        return Err(reject("SlugNotConfigured", json!({ "restaurantId": cmd.restaurant_id })));
    }
    // TODO(invariant): RestaurantNotReadyForActivation — "at least one catalog with one orderable
    //                  offer" is a cross-aggregate (Catalog) check; needs a catalog read-model port.
    if state.status == RestaurantStatus::ACTIVE {
        return Ok(());
    }
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = DomainEvent::RestaurantActivated(RestaurantActivated {
        restaurant_id: cmd.restaurant_id,
        reason: cmd.reason,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/UpdateRestaurant` → emit `events.yaml#/RestaurantUpdated` (full replace of
/// the provided location fields). An update carrying nothing editable is rejected
/// (`errors.yaml#/NoEditableFieldProvided`).
pub async fn update_restaurant(
    store: &dyn EventStore,
    cmd: UpdateRestaurant,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = require_restaurant(store, &cmd.restaurant_id).await?;
    let has_editable_field = cmd.display_name.is_some()
        || cmd.contact.is_some()
        || cmd.website.is_some()
        || !cmd.tags.is_empty()
        || cmd.margin_rate.is_some()
        || cmd.cuisine_category.is_some()
        || cmd.uber_prices_opt_in.is_some()
        || cmd.address.is_some()
        || cmd.location.is_some()
        || cmd.timezone.is_some()
        || cmd.preparation_time_minutes.is_some()
        || !cmd.opening_hours.is_empty();
    if !has_editable_field {
        return Err(reject("NoEditableFieldProvided", json!({})));
    }
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = DomainEvent::RestaurantUpdated(RestaurantUpdated {
        restaurant_id: cmd.restaurant_id,
        display_name: cmd.display_name,
        description: cmd.description,
        contact: cmd.contact,
        website: cmd.website,
        tags: cmd.tags,
        margin_rate: cmd.margin_rate,
        cuisine_category: cmd.cuisine_category,
        uber_prices_opt_in: cmd.uber_prices_opt_in,
        address: cmd.address,
        location: cmd.location,
        timezone: cmd.timezone,
        preparation_time_minutes: cmd.preparation_time_minutes,
        opening_hours: cmd.opening_hours,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/DeactivateRestaurant` → emit `events.yaml#/RestaurantDeactivated`.
/// Idempotent per actors.yaml: deactivating an already-INACTIVE restaurant is a no-op (no event, no
/// error) — the command ensures the INACTIVE state, it is not a toggle.
pub async fn deactivate_restaurant(
    store: &dyn EventStore,
    cmd: DeactivateRestaurant,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_restaurant(store, &cmd.restaurant_id).await?;
    if state.status == RestaurantStatus::INACTIVE {
        return Ok(());
    }
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = DomainEvent::RestaurantDeactivated(RestaurantDeactivated {
        restaurant_id: cmd.restaurant_id,
        reason: cmd.reason,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/ChangeOrderAcceptanceMode` → emit
/// `events.yaml#/RestaurantAcceptanceModeChanged`. Only an ACTIVE restaurant toggles its live mode
/// (`RestaurantNotActive`), and re-requesting the current mode is rejected
/// (`AcceptanceModeUnchanged`).
pub async fn change_order_acceptance_mode(
    store: &dyn EventStore,
    cmd: ChangeOrderAcceptanceMode,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_restaurant(store, &cmd.restaurant_id).await?;
    if state.status != RestaurantStatus::ACTIVE {
        return Err(reject(
            "RestaurantNotActive",
            json!({ "restaurantId": cmd.restaurant_id, "restaurantName": state.display_name }),
        ));
    }
    if state.order_acceptance == cmd.mode {
        return Err(reject(
            "AcceptanceModeUnchanged",
            json!({
                "restaurantId": cmd.restaurant_id,
                "restaurantName": state.display_name,
                "mode": cmd.mode,
            }),
        ));
    }
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = DomainEvent::RestaurantAcceptanceModeChanged(RestaurantAcceptanceModeChanged {
        restaurant_id: cmd.restaurant_id,
        mode: cmd.mode,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/RemoveRestaurant` → emit `events.yaml#/RestaurantRemoved` (the location is
/// delisted from its account; the stream and its history remain).
pub async fn remove_restaurant(
    store: &dyn EventStore,
    cmd: RemoveRestaurant,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = require_restaurant(store, &cmd.restaurant_id).await?;
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = DomainEvent::RestaurantRemoved(RestaurantRemoved {
        restaurant_id: cmd.restaurant_id,
        account_id: cmd.account_id,
        reason: cmd.reason,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/UpdateRestaurantGoogleBusinessProfile` → emit
/// `events.yaml#/RestaurantGoogleBusinessProfileUpdated` (GBP-specific metrics only; issued by the
/// Sirene/Google sync ACL or admin).
pub async fn update_restaurant_google_business_profile(
    store: &dyn EventStore,
    cmd: UpdateRestaurantGoogleBusinessProfile,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = require_restaurant(store, &cmd.restaurant_id).await?;
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event =
        DomainEvent::RestaurantGoogleBusinessProfileUpdated(RestaurantGoogleBusinessProfileUpdated {
            restaurant_id: cmd.restaurant_id,
            google_place_id: cmd.google_place_id,
            rating: cmd.rating,
            reviews_count: cmd.reviews_count,
        });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/MarkRestaurantClosed` → emit `events.yaml#/RestaurantMarkedClosed` (e.g. a
/// Sirene closure reported through the sync ACL).
pub async fn mark_restaurant_closed(
    store: &dyn EventStore,
    cmd: MarkRestaurantClosed,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = require_restaurant(store, &cmd.restaurant_id).await?;
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = DomainEvent::RestaurantMarkedClosed(RestaurantMarkedClosed {
        restaurant_id: cmd.restaurant_id,
        reason: cmd.reason,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/ClaimRestaurantListing` → emit `events.yaml#/RestaurantListingClaimed`.
/// A listing can be claimed once (`ListingAlreadyClaimed`), and only with a Google Business Profile
/// ownership proof the verifier accepts (`ListingOwnershipNotVerified`, ADR-0019).
pub async fn claim_restaurant_listing(
    store: &dyn EventStore,
    ownership: &dyn GoogleOwnershipVerifier,
    cmd: ClaimRestaurantListing,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_restaurant(store, &cmd.restaurant_id).await?;
    if state.listing_claimed {
        return Err(reject("ListingAlreadyClaimed", json!({ "restaurantId": cmd.restaurant_id })));
    }
    if !ownership.verify(cmd.restaurant_id, &cmd.google_ownership_proof).await? {
        return Err(reject(
            "ListingOwnershipNotVerified",
            json!({ "restaurantId": cmd.restaurant_id }),
        ));
    }
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = DomainEvent::RestaurantListingClaimed(RestaurantListingClaimed {
        restaurant_id: cmd.restaurant_id,
        account_id: cmd.account_id,
        proof: Some(cmd.google_ownership_proof),
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/OptOutRestaurantListing` → emit `events.yaml#/RestaurantListingOptedOut`.
/// Requires the same verified GBP ownership proof as a claim (`ListingOwnershipNotVerified`).
pub async fn opt_out_restaurant_listing(
    store: &dyn EventStore,
    ownership: &dyn GoogleOwnershipVerifier,
    cmd: OptOutRestaurantListing,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = require_restaurant(store, &cmd.restaurant_id).await?;
    if !ownership.verify(cmd.restaurant_id, &cmd.google_ownership_proof).await? {
        return Err(reject(
            "ListingOwnershipNotVerified",
            json!({ "restaurantId": cmd.restaurant_id }),
        ));
    }
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = DomainEvent::RestaurantListingOptedOut(RestaurantListingOptedOut {
        restaurant_id: cmd.restaurant_id,
        reason: cmd.reason,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/ChangeRestaurantListingStatus` → emit
/// `events.yaml#/RestaurantListingStatusChanged` (admin moves a listing along the partnership funnel).
pub async fn change_restaurant_listing_status(
    store: &dyn EventStore,
    cmd: ChangeRestaurantListingStatus,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = require_restaurant(store, &cmd.restaurant_id).await?;
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = DomainEvent::RestaurantListingStatusChanged(RestaurantListingStatusChanged {
        restaurant_id: cmd.restaurant_id,
        listing_status: cmd.listing_status,
        reason: cmd.reason,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/ConfigureGoogleBusinessProfileOrderLink` → emit
/// `events.yaml#/RestaurantGoogleBusinessProfileOrderLinkConfigured` (ADR-0021; V1).
pub async fn configure_gbp_order_link(
    store: &dyn EventStore,
    cmd: ConfigureGoogleBusinessProfileOrderLink,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = require_restaurant(store, &cmd.restaurant_id).await?;
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = DomainEvent::RestaurantGoogleBusinessProfileOrderLinkConfigured(
        RestaurantGoogleBusinessProfileOrderLinkConfigured {
            restaurant_id: cmd.restaurant_id,
            gbp_order_url: cmd.gbp_order_url,
        },
    );
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/VerifyGoogleBusinessProfileOrderLink` → emit
/// `events.yaml#/RestaurantGoogleBusinessProfileOrderLinkVerified` (ADR-0021; V1). Requires a
/// configured link (`GbpOrderLinkNotConfigured`); the probe port pings it and the handler records the
/// observed status.
pub async fn verify_gbp_order_link(
    store: &dyn EventStore,
    probe: &dyn GbpOrderLinkProbe,
    cmd: VerifyGoogleBusinessProfileOrderLink,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_restaurant(store, &cmd.restaurant_id).await?;
    let Some(url) = state.gbp_order_url else {
        return Err(reject("GbpOrderLinkNotConfigured", json!({ "restaurantId": cmd.restaurant_id })));
    };
    let status = probe.probe(&url).await?;
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = DomainEvent::RestaurantGoogleBusinessProfileOrderLinkVerified(
        RestaurantGoogleBusinessProfileOrderLinkVerified {
            restaurant_id: cmd.restaurant_id,
            status,
        },
    );
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

// ================================================================================================
// Cart aggregate (actors.yaml#/Cart) — the visitor's pre-checkout selection.
// ================================================================================================

/// The stream a Cart aggregate lives on (matches the projection worker's `Cart-` registry group).
fn cart_stream(id: &CartId) -> String {
    format!("Cart-{}", id.0)
}

/// Rehydrate the Cart aggregate: fold its stream into the minimal write-side state and return it with
/// the stream's current version (the expected version for the next append).
async fn load_cart(
    store: &dyn EventStore,
    id: &CartId,
) -> Result<(Option<CartState>, i64), DomainError> {
    Repository::new(store).load::<CartState>(*id).await
}

/// Rehydrate and require existence, or reject with `errors.yaml#/CartNotFound`.
async fn require_cart(
    store: &dyn EventStore,
    id: &CartId,
) -> Result<(CartState, i64), DomainError> {
    let (state, version) = load_cart(store, id).await?;
    match state {
        Some(state) => Ok((state, version)),
        None => Err(reject("CartNotFound", json!({ "cartId": id }))),
    }
}

/// The `errors.yaml#/CartNotOpen` rejection for `cart_id` in `status`.
fn cart_not_open(cart_id: &CartId, status: CartStatus) -> DomainError {
    reject("CartNotOpen", json!({ "cartId": cart_id, "status": status }))
}

/// Validate a cart line against the LIVE catalog read model (offer-level `CatalogReadRepository`
/// port): the offer must exist (`errors.yaml#/OfferNotFound`), be AVAILABLE
/// (`errors.yaml#/OfferUnavailable` — availability is the manual flag, orthogonal to stock), have
/// enough tracked stock for the requested quantity (`errors.yaml#/InsufficientStock`), and the
/// selected options must belong to the offer's option lists within their selection bounds
/// (`errors.yaml#/InvalidOptionSelection`). Prices are NOT read here — the projection prices the cart
/// from the same live catalog (rules.yaml#/CartPricedFromLiveCatalog).
async fn require_orderable_line(
    catalogs: &dyn CatalogReadRepository,
    restaurant_id: &domain::generated::scalars::RestaurantId,
    line: &CartLineItem,
) -> Result<(), DomainError> {
    let Some(offer) = catalogs.offer_by_id(*restaurant_id, line.offer_id).await? else {
        return Err(reject("OfferNotFound", json!({ "offerId": line.offer_id })));
    };
    if offer.availability == CatalogItemAvailability::UNAVAILABLE {
        return Err(reject(
            "OfferUnavailable",
            json!({
                "offerId": offer.offer_id,
                "productName": offer.product_name,
                "offerName": offer.offer_name,
            }),
        ));
    }
    require_stock_covers(&offer, line.quantity)?;
    require_valid_option_selection(&offer, &line.selected_option_ids)
}

/// The `errors.yaml#/InsufficientStock` guard: a stock-TRACKED offer must cover the requested
/// quantity (`stock_quantity = None` = untracked, never blocks — its derived status is IN_STOCK;
/// availability ≠ stock, the manual flag is checked separately).
fn require_stock_covers(offer: &OfferView, requested: i64) -> Result<(), DomainError> {
    let available = match offer.stock_quantity {
        None => return Ok(()),
        Some(quantity) => quantity.0,
    };
    if (requested as f64) > available {
        return Err(reject(
            "InsufficientStock",
            json!({
                "offerId": offer.offer_id,
                "productName": offer.product_name,
                "offerName": offer.offer_name,
                "requested": requested,
                "available": available,
            }),
        ));
    }
    Ok(())
}

/// The `errors.yaml#/InvalidOptionSelection` guard: every selected option belongs to one of the
/// offer's option lists, and each attached list's selection count respects `minSelections` /
/// `maxSelections` (with duplicates of the same option only when `multipleSelection`).
fn require_valid_option_selection(
    offer: &OfferView,
    selected: &[OptionId],
) -> Result<(), DomainError> {
    // `detail` is a diagnostic beyond the spec'd context (offerId, productName): WHICH option/list
    // violated the bounds — kept for logs/observability, unused by the catalogued message.
    let invalid = |detail: String| {
        reject(
            "InvalidOptionSelection",
            json!({
                "offerId": offer.offer_id,
                "productName": offer.product_name,
                "detail": detail,
            }),
        )
    };
    for option_id in selected {
        if !offer.option_lists.iter().any(|list| list.option_ids.contains(option_id)) {
            return Err(invalid(format!("optionId={} not in the offer's option lists", option_id.0)));
        }
    }
    for list in &offer.option_lists {
        let picked: Vec<&OptionId> =
            selected.iter().filter(|option_id| list.option_ids.contains(option_id)).collect();
        let count = picked.len() as i64;
        if count < list.min_selections {
            return Err(invalid(format!(
                "optionListId={} picked={count} minSelections={}",
                list.id.0, list.min_selections
            )));
        }
        if list.max_selections.map_or(false, |max| count > max) {
            return Err(invalid(format!(
                "optionListId={} picked={count} maxSelections={}",
                list.id.0,
                list.max_selections.unwrap_or_default()
            )));
        }
        if !list.multiple_selection {
            let mut seen = HashSet::new();
            if picked.iter().any(|option_id| !seen.insert(option_id.0)) {
                return Err(invalid(format!(
                    "optionListId={} duplicate selection without multipleSelection",
                    list.id.0
                )));
            }
        }
    }
    Ok(())
}

/// Handle `commands.yaml#/AddCartLine` → emit `events.yaml#/CartStarted` (first line only, creating
/// the cart) + `events.yaml#/CartLineAdded` (actors.yaml, Cart aggregate). The client generates the
/// cartId and the cartLineId: the first add for a new cartId CREATES the cart bound to the restaurant
/// (so `CartNotFound` is unreachable for this command by construction), and re-sending a line id the
/// cart already holds is an idempotent replay (no duplicate fact). The line is validated against the
/// LIVE catalog through the offer-level read port — see [`require_orderable_line`] — AFTER the
/// cart-state invariants, so a closed/mismatched cart rejects with its own code first.
pub async fn add_cart_line(
    store: &dyn EventStore,
    catalogs: &dyn CatalogReadRepository,
    cmd: AddCartLine,
    actor: &Actor,
) -> Result<(), DomainError> {
    if cmd.line.quantity > MAX_LINE_QUANTITY {
        // Spec context also wants productName, but the cap is checked BEFORE the catalog lookup —
        // a known context gap ({productName} stays uninterpolated in the catalogued message).
        return Err(reject("QuantityExceedsLimit", json!({ "offerId": cmd.line.offer_id })));
    }
    let line = CartLineItem {
        cart_line_id: cmd.line.cart_line_id,
        offer_id: cmd.line.offer_id,
        quantity: cmd.line.quantity,
        selected_option_ids: cmd.line.selected_option_ids,
    };
    let (state, version) = load_cart(store, &cmd.cart_id).await?;
    match state {
        // First line: create the cart (CartStarted) and add the line in one append. customerId stays
        // None — a guest builds the cart; CartBindingProcess/checkout binds the customer later.
        None => {
            require_orderable_line(catalogs, &cmd.restaurant_id, &line).await?;
            let events = [
                DomainEvent::CartStarted(CartStarted {
                    cart_id: cmd.cart_id,
                    restaurant_id: cmd.restaurant_id,
                    session_id: cmd.session_id,
                    customer_id: None,
                }),
                DomainEvent::CartLineAdded(CartLineAdded { cart_id: cmd.cart_id, line }),
            ];
            // A version-0 clash here is a REAL race (two concurrent first adds with different lines),
            // not a replay — do not absorb it; the client retries onto the now-existing cart.
            Repository::new(store).save(&cart_stream(&cmd.cart_id), 0, &events, actor).await.map(|_| ())
        }
        Some(s) => {
            if s.status != CartStatus::OPEN {
                return Err(cart_not_open(&cmd.cart_id, s.status));
            }
            if s.restaurant_id != cmd.restaurant_id {
                // Spec context also wants restaurantName; the cart handlers have no Restaurant
                // lookup — a known context gap.
                return Err(reject(
                    "CartRestaurantMismatch",
                    json!({ "cartId": cmd.cart_id, "restaurantId": cmd.restaurant_id }),
                ));
            }
            if s.line_ids.contains(&line.cart_line_id) {
                return Ok(()); // idempotent replay of an already-recorded line (client-generated id)
            }
            require_orderable_line(catalogs, &cmd.restaurant_id, &line).await?;
            let event = DomainEvent::CartLineAdded(CartLineAdded { cart_id: cmd.cart_id, line });
            Repository::new(store).save(&cart_stream(&cmd.cart_id), version, &[event], actor).await.map(|_| ())
        }
    }
}

/// Handle `commands.yaml#/RemoveCartLine` → emit `events.yaml#/CartLineRemoved` (actors.yaml, Cart
/// aggregate). Only an OPEN cart is editable and the line must exist.
pub async fn remove_cart_line(
    store: &dyn EventStore,
    cmd: RemoveCartLine,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_cart(store, &cmd.cart_id).await?;
    if state.status != CartStatus::OPEN {
        return Err(cart_not_open(&cmd.cart_id, state.status));
    }
    if !state.line_ids.contains(&cmd.cart_line_id) {
        return Err(reject(
            "CartLineNotFound",
            json!({ "cartId": cmd.cart_id, "cartLineId": cmd.cart_line_id }),
        ));
    }
    let event = DomainEvent::CartLineRemoved(CartLineRemoved {
        cart_id: cmd.cart_id,
        cart_line_id: cmd.cart_line_id,
    });
    Repository::new(store).save(&cart_stream(&cmd.cart_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/ChangeCartLineQuantity` → emit `events.yaml#/CartLineQuantityChanged`
/// (actors.yaml, Cart aggregate). Only an OPEN cart is editable, the line must exist, the new
/// quantity must respect the per-line cap, and — when the line's offer is still in the live catalog
/// and stock-tracked — the new quantity must be covered by its stock
/// (`errors.yaml#/InsufficientStock`). An offer that has since LEFT the catalog does not block the
/// change (actors.yaml declares no OfferNotFound here); checkout re-validates the whole cart.
pub async fn change_cart_line_quantity(
    store: &dyn EventStore,
    catalogs: &dyn CatalogReadRepository,
    cmd: ChangeCartLineQuantity,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_cart(store, &cmd.cart_id).await?;
    if state.status != CartStatus::OPEN {
        return Err(cart_not_open(&cmd.cart_id, state.status));
    }
    let Some(line) = state.lines.iter().find(|line| line.cart_line_id == cmd.cart_line_id) else {
        return Err(reject(
            "CartLineNotFound",
            json!({ "cartId": cmd.cart_id, "cartLineId": cmd.cart_line_id }),
        ));
    };
    if cmd.quantity > MAX_LINE_QUANTITY {
        // Spec context also wants productName, but the cap is checked BEFORE the catalog lookup —
        // a known context gap ({productName} stays uninterpolated in the catalogued message).
        return Err(reject("QuantityExceedsLimit", json!({ "offerId": line.offer_id })));
    }
    if let Some(offer) = catalogs.offer_by_id(state.restaurant_id, line.offer_id).await? {
        require_stock_covers(&offer, cmd.quantity)?;
    }
    let event = DomainEvent::CartLineQuantityChanged(CartLineQuantityChanged {
        cart_id: cmd.cart_id,
        cart_line_id: cmd.cart_line_id,
        quantity: cmd.quantity,
    });
    Repository::new(store).save(&cart_stream(&cmd.cart_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/BindCartToCustomer` → emit `events.yaml#/CartBoundToCustomer` (actors.yaml,
/// Cart aggregate; sent per OPEN cart by CartBindingProcess reacting to `CustomerIdentified` —
/// rules.yaml#/GuestCartsBoundOnIdentification). The bind is ONE-TIME, first wins: a cart already
/// bound to THIS customer is an idempotent replay (no event), and a cart already bound to a DIFFERENT
/// customer is ALSO a no-op — the earlier bind stands and is never overwritten (the saga may lawfully
/// re-deliver against a cart a previous identification already claimed; there is nothing to reject,
/// so no error is declared for it). Only `CartNotFound` throws.
pub async fn bind_cart_to_customer(
    store: &dyn EventStore,
    cmd: BindCartToCustomer,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_cart(store, &cmd.cart_id).await?;
    if state.customer_id.is_some() {
        // Already bound (same customer = replay; different customer = first-wins) — no new fact.
        return Ok(());
    }
    let event = DomainEvent::CartBoundToCustomer(CartBoundToCustomer {
        cart_id: cmd.cart_id,
        customer_id: cmd.customer_id,
    });
    Repository::new(store).save(&cart_stream(&cmd.cart_id), version, &[event], actor).await.map(|_| ())
}

// ================================================================================================
// Order aggregate (actors.yaml#/Order) — born from OrderPlaced, driven to a terminal state.
// ================================================================================================

/// The stream an Order aggregate lives on (matches the projection worker's `Order-` registry group).
pub(crate) fn order_stream(id: &OrderId) -> String {
    format!("Order-{}", id.0)
}

/// Record the delivered `OrderExpired` reminder fact on its order's stream — the kind-MESSAGE
/// delivery route (ADR-20260731-153000 §1a). Record semantics, NEVER a rejection: the retention
/// deadline's passage cannot be refused. Recorded the first time, `AlreadyRecorded` when the
/// order already expired (a redelivered reminder), `NoChange` when the stream is empty — the
/// deletion journey already erased it (or it never existed), so there is nothing left to expire.
/// The erasure ACTION on this recording is the deletion engine's journey, not this recorder
/// (ADR-20260731-160000; STUB until [#194 "GDPR erasure"] closes the loop).
pub async fn record_inbound_order_event(
    store: &dyn EventStore,
    event: DomainEvent,
    actor: &Actor,
) -> Result<crate::payments::RecordOutcome, DomainError> {
    use crate::payments::RecordOutcome;

    let DomainEvent::OrderExpired(expired) = &event else {
        return Err(DomainError::Repository(format!(
            "record_inbound_order_event routed a non-order fact: {event:?}"
        )));
    };
    let stream_name = order_stream(&expired.order_id);
    let (events, version) = store.load(&stream_name).await?;
    if events.is_empty() {
        return Ok(RecordOutcome::NoChange);
    }
    if events.iter().any(|ev| matches!(ev, DomainEvent::OrderExpired(_))) {
        return Ok(RecordOutcome::AlreadyRecorded);
    }
    Repository::new(store).save(&stream_name, version, &[event], actor).await?;
    Ok(RecordOutcome::Recorded)
}

/// Record the delivered `OrderPlaced` BIRTH fact on its order's stream — the kind-EVENT delivery
/// route for the spec's birth receive (`specs/ordering/actors.yaml`: "Birth: PlaceOrderProcess
/// delivers OrderPlaced; the Order records it (idempotent)"). Recorded exactly once:
/// `AlreadyRecorded` when the birth is already on the stream (a redelivered birth — the caller
/// still RE-APPLIES the receive's `schedules:`, which is safe by design: the acceptance deadline
/// is `reschedule: keep`, so the FIRST scheduled_at wins, #167), `NoChange` when the stream is
/// non-empty yet birthless (an erased or partial stream — absorb, never a second birth).
pub async fn record_inbound_order_placed(
    store: &dyn EventStore,
    event: DomainEvent,
    actor: &Actor,
) -> Result<crate::payments::RecordOutcome, DomainError> {
    use crate::payments::RecordOutcome;

    let DomainEvent::OrderPlaced(placed) = &event else {
        return Err(DomainError::Repository(format!(
            "record_inbound_order_placed routed a non-birth fact: {event:?}"
        )));
    };
    let stream_name = order_stream(&placed.order_id);
    let (events, version) = store.load(&stream_name).await?;
    if events.iter().any(|ev| matches!(ev, DomainEvent::OrderPlaced(_))) {
        return Ok(RecordOutcome::AlreadyRecorded);
    }
    if !events.is_empty() {
        return Ok(RecordOutcome::NoChange);
    }
    Repository::new(store).save(&stream_name, version, &[event], actor).await?;
    Ok(RecordOutcome::Recorded)
}

/// What one delivered acceptance deadline decided (#167). Richer than
/// `payments::RecordOutcome` ON PURPOSE: the mailbox delivery arm labels its OTLP shadow
/// evidence from this — `WouldCancel` is the flip ADR's whole data set — and a coarser outcome
/// would force the infrastructure layer to re-run the guard to know what happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptanceTimeoutOutcome {
    /// Gate ON, order still PLACED: `OrderAcceptanceTimedOut` appended —
    /// PLACED → CANCELLED_BY_TIMEOUT (terminal; the GDPR clock is scheduled by the delivery glue).
    Cancelled,
    /// Gate OFF (shadow), order still PLACED: the IDENTICAL guard decided it would cancel, and
    /// only the append was inert. The delivery lands Ignored; the occurrence is spent forever
    /// (flipping the gate ON is prospective only).
    WouldCancel,
    /// Acceptance/rejection/cancellation won the race — the order is no longer PLACED. Benign
    /// no-op under either gate position (rules.yaml#/AcceptanceTimeoutOnlyCancelsAStillPlacedOrder).
    NotPlaced,
    /// A redelivered deadline: the order already timed out. Benign duplicate no-op.
    AlreadyTimedOut,
    /// The stream has no order (erased, or never born) — nothing left to cancel.
    NoOrder,
}

/// Record the delivered `OrderAcceptanceTimedOut` reminder fact (#167, ADR-20260808-195315
/// §1.3) — the kind-MESSAGE delivery route, mirroring [`record_inbound_order_event`]. Record
/// semantics iff STILL PLACED, NEVER a rejection: the deadline's passage cannot be refused, and
/// an order that moved on absorbs the delivery. `enforce_acceptance_timeout` is the
/// ENFORCE_ACCEPTANCE_TIMEOUT gate read at DELIVERY time (never at scheduling): the FULL guard
/// runs on this one code path in both positions — the gate parks the append alone, so the shadow
/// evidence is the real predicate, not a parallel one. Business code stays SDK-free: this
/// function only DECIDES; the OTLP shadow span lives at the mailbox/promotion layer.
pub async fn record_order_acceptance_timeout(
    store: &dyn EventStore,
    event: DomainEvent,
    enforce_acceptance_timeout: bool,
    actor: &Actor,
) -> Result<AcceptanceTimeoutOutcome, DomainError> {
    let DomainEvent::OrderAcceptanceTimedOut(timed_out) = &event else {
        return Err(DomainError::Repository(format!(
            "record_order_acceptance_timeout routed a non-timeout fact: {event:?}"
        )));
    };
    let stream_name = order_stream(&timed_out.order_id);
    let (events, version) = store.load(&stream_name).await?;
    if events.iter().any(|ev| matches!(ev, DomainEvent::OrderAcceptanceTimedOut(_))) {
        return Ok(AcceptanceTimeoutOutcome::AlreadyTimedOut);
    }
    let Some(state) = domain::order::fold(&events) else {
        return Ok(AcceptanceTimeoutOutcome::NoOrder);
    };
    if state.status != OrderStatus::PLACED {
        return Ok(AcceptanceTimeoutOutcome::NotPlaced);
    }
    // The guard has decided. The gate governs the APPEND alone (action-gate pattern):
    if !enforce_acceptance_timeout {
        return Ok(AcceptanceTimeoutOutcome::WouldCancel);
    }
    Repository::new(store).save(&stream_name, version, &[event], actor).await?;
    Ok(AcceptanceTimeoutOutcome::Cancelled)
}

/// Rehydrate the Order aggregate and require existence UNDER the commanding restaurant: a missing
/// stream — or an order belonging to another restaurant (tenant scoping) — rejects with
/// `errors.yaml#/OrderNotFound`.
pub(crate) async fn require_order(
    store: &dyn EventStore,
    order_id: &OrderId,
    restaurant_id: &domain::generated::scalars::RestaurantId,
) -> Result<(OrderState, i64), DomainError> {
    let (state, version) = Repository::new(store).load::<OrderState>(*order_id).await?;
    match state {
        Some(state) if state.restaurant_id == *restaurant_id => Ok((state, version)),
        _ => Err(reject("OrderNotFound", json!({ "orderId": order_id }))),
    }
}

/// The `errors.yaml#/InvalidOrderStatus` rejection for `order_id` currently in `status`.
pub(crate) fn invalid_order_status(order_id: &OrderId, status: OrderStatus) -> DomainError {
    reject("InvalidOrderStatus", json!({ "orderId": order_id, "currentStatus": status }))
}

/// Handle `commands.yaml#/RateOrder` → emit `events.yaml#/OrderRated`. Only a DELIVERED order, exactly
/// once (rules.yaml#/OrderRatedOnceWhenDelivered).
pub async fn rate_order(
    store: &dyn EventStore,
    cmd: RateOrder,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_order(store, &cmd.order_id, &cmd.restaurant_id).await?;
    if state.status != OrderStatus::DELIVERED {
        return Err(invalid_order_status(&cmd.order_id, state.status));
    }
    if state.delivery_rated {
        return Err(reject("OrderAlreadyRated", json!({ "orderId": cmd.order_id })));
    }
    let event = DomainEvent::OrderRated(OrderRated {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
        customer_id: Some(state.customer_id),
        rider_thumb: cmd.rider_thumb,
    });
    Repository::new(store).save(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/RateRestaurant` → emit `events.yaml#/RestaurantRated`. Only a DELIVERED
/// order, exactly once per order (rules.yaml#/RestaurantRatedOncePerOrder).
pub async fn rate_restaurant(
    store: &dyn EventStore,
    cmd: RateRestaurant,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_order(store, &cmd.order_id, &cmd.restaurant_id).await?;
    if state.status != OrderStatus::DELIVERED {
        return Err(invalid_order_status(&cmd.order_id, state.status));
    }
    if state.restaurant_rated {
        return Err(reject("RestaurantAlreadyRated", json!({ "orderId": cmd.order_id })));
    }
    let event = DomainEvent::RestaurantRated(RestaurantRatedEvent {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
        customer_id: Some(state.customer_id),
        stars: cmd.stars,
        comment: cmd.comment,
    });
    Repository::new(store).save(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/RecordDeliverySatisfaction` → emit
/// `events.yaml#/DeliverySatisfactionRecorded` (#62). Only a DELIVERED order, recorded exactly once
/// (rules.yaml#/DeliverySatisfactionRecordedOncePerDeliveredOrder).
pub async fn record_delivery_satisfaction(
    store: &dyn EventStore,
    cmd: RecordDeliverySatisfaction,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_order(store, &cmd.order_id, &cmd.restaurant_id).await?;
    if state.status != OrderStatus::DELIVERED {
        return Err(invalid_order_status(&cmd.order_id, state.status));
    }
    if state.delivery_satisfaction_recorded {
        return Err(reject(
            "DeliverySatisfactionAlreadyRecorded",
            json!({ "orderId": cmd.order_id }),
        ));
    }
    let event = DomainEvent::DeliverySatisfactionRecorded(DeliverySatisfactionRecorded {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
        timeliness: cmd.timeliness,
        reason: cmd.reason,
    });
    Repository::new(store).save(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/TipOrder` → emit `events.yaml#/OrderTipped` (ADR-012/0029). Additive —
/// multiple tips accumulate; allowed at checkout or post-delivery but never on a rejected/cancelled
/// order. `tippedBy` is DERIVED from the caller's role (never client-supplied), and a restaurant
/// cannot tip itself (rules.yaml#/TipsAdditiveMultiRecipientSeparate).
pub async fn tip_order(
    store: &dyn EventStore,
    cmd: TipOrder,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_order(store, &cmd.order_id, &cmd.restaurant_id).await?;
    if state.is_terminated() {
        return Err(invalid_order_status(&cmd.order_id, state.status));
    }
    if cmd.tips.is_empty() {
        // commands.yaml: `tips` minItems 1 — an intrinsic payload invariant (cross-cutting
        // ValidationError, not an actors.yaml `throws` entry).
        return Err(reject("ValidationError", json!({ "field": "tips" })));
    }
    // The business role that changes semantics (scalars.yaml#/Tipper), derived from the acting user's
    // envelope UserType (ADR-0041; stored as TEXT per ADR-20260728): RESTAURANT_ACCOUNT / RESTAURANT
    // tip as the restaurant; everyone else is the customer.
    let tipped_by = if actor.user_type == "RESTAURANT_ACCOUNT" || actor.user_type == "RESTAURANT" {
        Tipper::RESTAURANT
    } else {
        Tipper::CUSTOMER
    };
    if tipped_by == Tipper::RESTAURANT
        && cmd.tips.iter().any(|t| t.recipient == TipRecipient::RESTAURANT)
    {
        return Err(reject(
            "InvalidTipRecipient",
            json!({ "tippedBy": tipped_by, "recipient": TipRecipient::RESTAURANT }),
        ));
    }
    let event = DomainEvent::OrderTipped(OrderTipped {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
        tipped_by,
        customer_id: if tipped_by == Tipper::CUSTOMER { Some(state.customer_id) } else { None },
        tips: cmd.tips,
    });
    Repository::new(store).save(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/RequestRefund` → emit `events.yaml#/RefundRequested`. Only a DELIVERED order
/// (rejections/cancellations refund automatically via RefundProcess); RefundProcess validates
/// eligibility and drives Stripe from the emitted fact (rules.yaml#/RefundRequestByCustomer).
pub async fn request_refund(
    store: &dyn EventStore,
    cmd: RequestRefund,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_order(store, &cmd.order_id, &cmd.restaurant_id).await?;
    if state.status != OrderStatus::DELIVERED {
        return Err(invalid_order_status(&cmd.order_id, state.status));
    }
    let event = DomainEvent::RefundRequested(RefundRequested {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
        customer_id: Some(state.customer_id),
        reason: cmd.reason,
    });
    Repository::new(store).save(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
}

// ================================================================================================
// DeliveryJob aggregate (actors.yaml#/DeliveryJob) — independent-rider fulfilment (ADR-0031).
// ================================================================================================

/// The stream a DeliveryJob aggregate lives on.
pub(crate) fn delivery_job_stream(id: &DeliveryJobId) -> String {
    format!("DeliveryJob-{}", id.0)
}

/// Rehydrate the DeliveryJob aggregate and require existence, or reject with
/// `errors.yaml#/DeliveryJobNotFound`.
pub(crate) async fn require_delivery_job(
    store: &dyn EventStore,
    id: &DeliveryJobId,
) -> Result<(DeliveryJobState, i64), DomainError> {
    Repository::new(store)
        .require::<DeliveryJobState>(*id, || {
            reject("DeliveryJobNotFound", json!({ "deliveryJobId": id }))
        })
        .await
}

/// The `errors.yaml#/InvalidDeliveryStatus` rejection for `id` currently in `current` when the
/// transition needs `expected`.
pub(crate) fn invalid_delivery_status(
    id: &DeliveryJobId,
    current: DeliveryStatus,
    expected: DeliveryStatus,
) -> DomainError {
    reject(
        "InvalidDeliveryStatus",
        json!({ "deliveryJobId": id, "currentStatus": current, "expectedStatus": expected }),
    )
}

/// Handle `commands.yaml#/AcceptDelivery` → emit `events.yaml#/DeliveryAcceptedByRider`. Only a
/// PENDING job, only once — a job already taken by a rider or partner rejects with
/// `DeliveryAlreadyAssigned` (rules.yaml#/DeliveryAcceptedOnlyWhenPending).
pub async fn accept_delivery(
    store: &dyn EventStore,
    cmd: AcceptDelivery,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_delivery_job(store, &cmd.delivery_job_id).await?;
    match state.status {
        DeliveryStatus::PENDING => {}
        DeliveryStatus::ASSIGNED | DeliveryStatus::PICKED_UP | DeliveryStatus::OUT_FOR_DELIVERY
            if state.assigned =>
        {
            return Err(reject(
                "DeliveryAlreadyAssigned",
                json!({ "deliveryJobId": cmd.delivery_job_id }),
            ));
        }
        other => {
            return Err(invalid_delivery_status(&cmd.delivery_job_id, other, DeliveryStatus::PENDING))
        }
    }
    let event = DomainEvent::DeliveryAcceptedByRider(DeliveryAcceptedByRider {
        delivery_job_id: cmd.delivery_job_id,
        // From the folded birth fact, never from the client (D-QW1 option b).
        order_id: state.order_id,
        rider_id: cmd.rider_id,
    });
    Repository::new(store).save(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/ConfirmPickup` → emit `events.yaml#/DeliveryPickedUp`. The job must be
/// ASSIGNED to THIS rider (rules.yaml#/DeliveryPickupAndCompletionByRider). The pickup time is the
/// envelope's `occurred_at`; the optional payload `at` is reserved for externally reported times.
pub async fn confirm_pickup(
    store: &dyn EventStore,
    cmd: ConfirmPickup,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_delivery_job(store, &cmd.delivery_job_id).await?;
    let event = DomainEvent::DeliveryPickedUp(DeliveryPickedUp {
        delivery_job_id: cmd.delivery_job_id,
        // From the folded birth fact, never from the client (D-QW1 option b) — this is the field
        // that lets the customer's OrderTracking row move to PICKED_UP.
        order_id: state.order_id,
        rider_id: cmd.rider_id,
        at: None,
    });
    // The declared machine allows the pickup only from ASSIGNED (actors.yaml#/DeliveryJob/lifecycle).
    if domain::delivery_job::lifecycle::transition(state.status, &event).is_none() {
        return Err(invalid_delivery_status(
            &cmd.delivery_job_id,
            state.status,
            DeliveryStatus::ASSIGNED,
        ));
    }
    if state.rider_id != Some(cmd.rider_id) {
        // `detail` is a diagnostic beyond the spec'd context: the job is not assigned to THIS rider.
        return Err(reject(
            "InvalidDeliveryStatus",
            json!({
                "deliveryJobId": cmd.delivery_job_id,
                "currentStatus": state.status,
                "expectedStatus": DeliveryStatus::ASSIGNED,
                "detail": format!("job is not assigned to rider {}", cmd.rider_id.0),
            }),
        ));
    }
    Repository::new(store).save(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/CompleteDelivery` → emit `events.yaml#/DeliveryCompleted`. The job must be
/// PICKED_UP (or partner-reported OUT_FOR_DELIVERY) and assigned to THIS rider
/// (rules.yaml#/DeliveryPickupAndCompletionByRider). DeliveryDispatchProcess reacts to the emitted
/// fact to close the order (OrderDelivered) — a saga leg outside this handler.
pub async fn complete_delivery(
    store: &dyn EventStore,
    cmd: CompleteDelivery,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_delivery_job(store, &cmd.delivery_job_id).await?;
    let event = DomainEvent::DeliveryCompleted(DeliveryCompleted {
        delivery_job_id: cmd.delivery_job_id,
        // From the folded birth fact, never from the client (D-QW1 option b).
        order_id: state.order_id,
        at: None,
    });
    // The declared machine allows the completion from PICKED_UP or OUT_FOR_DELIVERY
    // (actors.yaml#/DeliveryJob/lifecycle — the hand-over shortcut).
    if domain::delivery_job::lifecycle::transition(state.status, &event).is_none() {
        return Err(invalid_delivery_status(
            &cmd.delivery_job_id,
            state.status,
            DeliveryStatus::PICKED_UP,
        ));
    }
    if state.rider_id != Some(cmd.rider_id) {
        // `detail` is a diagnostic beyond the spec'd context: the job is not assigned to THIS rider.
        return Err(reject(
            "InvalidDeliveryStatus",
            json!({
                "deliveryJobId": cmd.delivery_job_id,
                "currentStatus": state.status,
                "expectedStatus": DeliveryStatus::PICKED_UP,
                "detail": format!("job is not assigned to rider {}", cmd.rider_id.0),
            }),
        ));
    }
    Repository::new(store).save(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/CancelDelivery` → emit `events.yaml#/DeliveryCancelled`. A job can be
/// cancelled any time BEFORE completion (a DELIVERED job rejects); re-cancelling an already-CANCELLED
/// job is an idempotent no-op — the command ensures the state
/// (rules.yaml#/DeliveryCancellableBeforeCompletion).
pub async fn cancel_delivery(
    store: &dyn EventStore,
    cmd: CancelDelivery,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_delivery_job(store, &cmd.delivery_job_id).await?;
    if state.status == DeliveryStatus::CANCELLED {
        return Ok(()); // ensure-command idempotency: already cancelled, nothing to append
    }
    let event = DomainEvent::DeliveryCancelled(DeliveryCancelled {
        delivery_job_id: cmd.delivery_job_id,
        reason: cmd.reason,
    });
    // The declared machine allows the cancellation from any not-yet-delivered state, including a
    // FAILED dispatch surfaced for manual handling (actors.yaml#/DeliveryJob/lifecycle).
    if domain::delivery_job::lifecycle::transition(state.status, &event).is_none() {
        return Err(invalid_delivery_status(
            &cmd.delivery_job_id,
            state.status,
            DeliveryStatus::PENDING,
        ));
    }
    Repository::new(store).save(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/EscalateDelivery` → emit `events.yaml#/DeliveryEscalationRequested` (#60).
/// A restaurant/admin asks to skip the channel currently offered and advance the ranked walk NOW. Only
/// a known job can be escalated (`DeliveryJobNotFound`); self-dispatch and walk exhaustion are the
/// saga's concern (a benign skip / a terminal fact), not command errors — so this handler records the
/// request unconditionally once the job exists (rules.yaml#/ManualEscalateSkipsChannel).
pub async fn escalate_delivery(
    store: &dyn EventStore,
    cmd: EscalateDelivery,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = require_delivery_job(store, &cmd.delivery_job_id).await?;
    let event = DomainEvent::DeliveryEscalationRequested(DeliveryEscalationRequested {
        delivery_job_id: cmd.delivery_job_id,
        reason: cmd.reason,
    });
    Repository::new(store)
        .save(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor)
        .await
        .map(|_| ())
}

// ================================================================================================
// DeliveryJob ops — decline/issue/status/partner commands (ADR-20260719-193500 command surface).
// ================================================================================================

/// The status a job "needed to be in" for a transition INTO `to` — the `expectedStatus` diagnostic on
/// an `InvalidDeliveryStatus` rejection of an invalid transition (the canonical predecessor in the
/// lifecycle; only `currentStatus` is interpolated into the catalogued message).
pub(crate) fn canonical_predecessor(to: DeliveryStatus) -> DeliveryStatus {
    use DeliveryStatus::*;
    match to {
        PENDING | ASSIGNED | CANCELLED | FAILED => PENDING,
        PICKED_UP => ASSIGNED,
        OUT_FOR_DELIVERY | DELIVERED => PICKED_UP,
    }
}

/// Handle `commands.yaml#/DeclineDelivery` → emit `events.yaml#/DeliveryDeclinedByRider`. Only a
/// PENDING job can be declined; a job already taken (by a rider or partner) rejects with
/// `DeliveryAlreadyAssigned` (rules.yaml#/DeliveryDeclineKeepsJobPending). The decline is a recorded
/// fact only — the fold leaves the job PENDING and re-offerable.
pub async fn decline_delivery(
    store: &dyn EventStore,
    cmd: DeclineDelivery,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_delivery_job(store, &cmd.delivery_job_id).await?;
    match state.status {
        DeliveryStatus::PENDING => {}
        DeliveryStatus::ASSIGNED | DeliveryStatus::PICKED_UP | DeliveryStatus::OUT_FOR_DELIVERY
            if state.assigned =>
        {
            return Err(reject(
                "DeliveryAlreadyAssigned",
                json!({ "deliveryJobId": cmd.delivery_job_id }),
            ));
        }
        other => {
            return Err(invalid_delivery_status(&cmd.delivery_job_id, other, DeliveryStatus::PENDING))
        }
    }
    let event = DomainEvent::DeliveryDeclinedByRider(DeliveryDeclinedByRider {
        delivery_job_id: cmd.delivery_job_id,
        rider_id: cmd.rider_id,
        reason: cmd.reason,
    });
    Repository::new(store).save(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/ReportDeliveryIssue` → emit `events.yaml#/DeliveryIssueReported`. Any
/// non-DELIVERED job can report an issue (rules.yaml#/DeliveryIssueLifecycle); `reportedAt` is stamped
/// server-side (the command carries none — the reporter states the issue, the system records when).
pub async fn report_delivery_issue(
    store: &dyn EventStore,
    cmd: ReportDeliveryIssue,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_delivery_job(store, &cmd.delivery_job_id).await?;
    if state.status == DeliveryStatus::DELIVERED {
        return Err(invalid_delivery_status(
            &cmd.delivery_job_id,
            state.status,
            DeliveryStatus::PENDING,
        ));
    }
    let event = DomainEvent::DeliveryIssueReported(DeliveryIssueReported {
        delivery_job_id: cmd.delivery_job_id,
        rider_id: cmd.rider_id,
        issue: cmd.issue,
        // The report TIME is envelope metadata (`domain_events.occurred_at`, ADR-0041) — never a
        // wall-clock stamp in the business payload (which would break command→event determinism).
        reported_at: None,
    });
    Repository::new(store).save(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/ResolveDeliveryIssue` → emit `events.yaml#/DeliveryIssueResolved`. Requires
/// a non-DELIVERED job with an OPEN issue to resolve (rules.yaml#/DeliveryIssueLifecycle; both arms
/// reject `InvalidDeliveryStatus` — the only status error this message declares); `resolvedAt` is
/// stamped server-side like `reportedAt`.
pub async fn resolve_delivery_issue(
    store: &dyn EventStore,
    cmd: ResolveDeliveryIssue,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_delivery_job(store, &cmd.delivery_job_id).await?;
    if state.status == DeliveryStatus::DELIVERED {
        return Err(invalid_delivery_status(
            &cmd.delivery_job_id,
            state.status,
            DeliveryStatus::PENDING,
        ));
    }
    if !state.open_issue {
        // `detail` is a diagnostic beyond the spec'd context: there is no open issue to resolve.
        return Err(reject(
            "InvalidDeliveryStatus",
            json!({
                "deliveryJobId": cmd.delivery_job_id,
                "currentStatus": state.status,
                "expectedStatus": state.status,
                "detail": "no open issue to resolve",
            }),
        ));
    }
    let event = DomainEvent::DeliveryIssueResolved(DeliveryIssueResolved {
        delivery_job_id: cmd.delivery_job_id,
        resolution: cmd.resolution,
        // Envelope-owned time (ADR-0041) — see reported_at above.
        resolved_at: None,
    });
    Repository::new(store).save(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/UnassignDeliveryFromPartner` → emit
/// `events.yaml#/DeliveryUnassignedFromPartner`. Only a job currently ASSIGNED TO A PARTNER (a
/// rider-assigned or pending/terminal job rejects `InvalidDeliveryStatus`); the fold returns the job
/// to PENDING so it is re-offerable (rules.yaml#/DeliveryPartnerAssignmentLifecycle).
pub async fn unassign_delivery_from_partner(
    store: &dyn EventStore,
    cmd: UnassignDeliveryFromPartner,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_delivery_job(store, &cmd.delivery_job_id).await?;
    let event = DomainEvent::DeliveryUnassignedFromPartner(DeliveryUnassignedFromPartner {
        delivery_job_id: cmd.delivery_job_id,
        reason: cmd.reason,
    });
    // The declared machine allows the unassignment only from ASSIGNED
    // (actors.yaml#/DeliveryJob/lifecycle).
    if domain::delivery_job::lifecycle::transition(state.status, &event).is_none() {
        return Err(invalid_delivery_status(
            &cmd.delivery_job_id,
            state.status,
            DeliveryStatus::ASSIGNED,
        ));
    }
    if state.partner_ref.is_none() {
        // `detail` is a diagnostic beyond the spec'd context: the job is rider-assigned, not
        // partner-assigned — there is no partner to unassign.
        return Err(reject(
            "InvalidDeliveryStatus",
            json!({
                "deliveryJobId": cmd.delivery_job_id,
                "currentStatus": state.status,
                "expectedStatus": DeliveryStatus::ASSIGNED,
                "detail": "job is not assigned to a partner",
            }),
        ));
    }
    Repository::new(store).save(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor).await.map(|_| ())
}

// ================================================================================================
// Rider aggregate (actors.yaml#/Rider) — rider identity + the availability status machine.
// ================================================================================================

/// The stream a Rider aggregate lives on.
pub(crate) fn rider_stream(id: &RiderId) -> String {
    format!("Rider-{}", id.0)
}

/// Rehydrate the Rider aggregate and require existence, or reject with `errors.yaml#/RiderNotFound`.
pub(crate) async fn require_rider(
    store: &dyn EventStore,
    id: &RiderId,
) -> Result<(RiderState, i64), DomainError> {
    Repository::new(store)
        .require::<RiderState>(*id, || reject("RiderNotFound", json!({ "riderId": id })))
        .await
}

/// Handle `commands.yaml#/RegisterRider` → emit `events.yaml#/RiderRegistered` on the new
/// `Rider-<id>` stream. A rider registers ONCE: an existing fold — or losing the version-0 race —
/// rejects with `errors.yaml#/RiderAlreadyRegistered` (the declared throw; unlike the client-id
/// creation commands this is NOT absorbed as a replay, per tests.yaml
/// TestRiderRegisterAgainIsRejected). The initial availability status is OFFLINE — the rider goes
/// AVAILABLE explicitly via ChangeRiderStatus (rules.yaml#/RiderLifecycle).
pub async fn register_rider(
    store: &dyn EventStore,
    cmd: RegisterRider,
    actor: &Actor,
) -> Result<(), DomainError> {
    let already = |rider_id: &RiderId| {
        reject("RiderAlreadyRegistered", json!({ "riderId": rider_id }))
    };
    let (state, _version) = Repository::new(store).load::<RiderState>(cmd.rider_id).await?;
    if state.is_some() {
        return Err(already(&cmd.rider_id));
    }
    let event = DomainEvent::RiderRegistered(RiderRegistered {
        rider_id: cmd.rider_id,
        auth_ref: cmd.auth_ref,
        display_name: cmd.display_name,
        phone: cmd.phone,
        status: RiderStatus::OFFLINE,
    });
    match Repository::new(store).save(&rider_stream(&cmd.rider_id), 0, &[event], actor).await {
        Ok(_) => Ok(()),
        Err(e) if is_version_conflict(&e) => Err(already(&cmd.rider_id)),
        Err(e) => Err(e),
    }
}

/// Handle `commands.yaml#/UpdateRiderInfo` → emit `events.yaml#/RiderInfoUpdated` (partial update of
/// the editable profile fields). An update carrying nothing editable is rejected
/// (`errors.yaml#/NoEditableFieldProvided`).
pub async fn update_rider_info(
    store: &dyn EventStore,
    cmd: UpdateRiderInfo,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = require_rider(store, &cmd.rider_id).await?;
    if cmd.display_name.is_none() && cmd.phone.is_none() {
        return Err(reject("NoEditableFieldProvided", json!({})));
    }
    let event = DomainEvent::RiderInfoUpdated(RiderInfoUpdated {
        rider_id: cmd.rider_id,
        display_name: cmd.display_name,
        phone: cmd.phone,
    });
    Repository::new(store).save(&rider_stream(&cmd.rider_id), version, &[event], actor).await.map(|_| ())
}

// ================================================================================================
// DeliveryPartnerRegistration (actors.yaml#/DeliveryPartnerRegistration) — self-registration (#61).
// ================================================================================================

pub(crate) fn delivery_partner_registration_stream(id: &DeliveryPartnerRegistrationId) -> String {
    format!("DeliveryPartnerRegistration-{}", id.0)
}

/// Rehydrate the DeliveryPartnerRegistration aggregate and require existence, or reject with
/// `errors.yaml#/DeliveryPartnerAvailabilityNotFound`.
pub(crate) async fn require_delivery_partner_registration(
    store: &dyn EventStore,
    id: &DeliveryPartnerRegistrationId,
) -> Result<(DeliveryPartnerRegistrationState, i64), DomainError> {
    Repository::new(store)
        .require::<DeliveryPartnerRegistrationState>(*id, || {
            reject("DeliveryPartnerAvailabilityNotFound", json!({ "registrationId": id }))
        })
        .await
}

/// Handle `commands.yaml#/RegisterDeliveryPartnerAvailability` → emit
/// `events.yaml#/DeliveryPartnerAvailabilityRequested` on the new `DeliveryPartnerRegistration-<id>`
/// stream. A partner self-registers ONCE per (client-generated) registrationId: an existing fold — or
/// losing the version-0 race — rejects with `errors.yaml#/DeliveryPartnerAvailabilityAlreadyRequested`
/// (the declared throw; not absorbed as a replay, per tests.yaml TestDeliveryPartnerRegisterAgainIsRejected).
/// The registration lands PENDING (rules.yaml#/DeliveryPartnerSelfRegistersCityAvailability).
pub async fn register_delivery_partner_availability(
    store: &dyn EventStore,
    cmd: RegisterDeliveryPartnerAvailability,
    actor: &Actor,
) -> Result<(), DomainError> {
    let already = |id: &DeliveryPartnerRegistrationId| {
        reject("DeliveryPartnerAvailabilityAlreadyRequested", json!({ "registrationId": id }))
    };
    let (state, _version) =
        Repository::new(store).load::<DeliveryPartnerRegistrationState>(cmd.registration_id).await?;
    if state.is_some() {
        return Err(already(&cmd.registration_id));
    }
    let event = DomainEvent::DeliveryPartnerAvailabilityRequested(DeliveryPartnerAvailabilityRequested {
        registration_id: cmd.registration_id,
        channel: cmd.channel,
        city_id: cmd.city_id,
        partner_name: cmd.partner_name,
        contact_email: cmd.contact_email,
    });
    let stream = delivery_partner_registration_stream(&cmd.registration_id);
    match Repository::new(store).save(&stream, 0, &[event], actor).await {
        Ok(_) => Ok(()),
        Err(e) if is_version_conflict(&e) => Err(already(&cmd.registration_id)),
        Err(e) => Err(e),
    }
}

/// Handle `commands.yaml#/ApproveDeliveryPartnerAvailability` → emit
/// `events.yaml#/DeliveryPartnerAvailabilityApproved`. Requires an existing PENDING registration:
/// unknown rejects `DeliveryPartnerAvailabilityNotFound`, a non-PENDING one (already approved/revoked)
/// rejects `DeliveryPartnerAvailabilityNotPending` (rules.yaml#/DeliveryPartnerAvailabilityGoesLiveOnlyAfterApproval).
pub async fn approve_delivery_partner_availability(
    store: &dyn EventStore,
    cmd: ApproveDeliveryPartnerAvailability,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_delivery_partner_registration(store, &cmd.registration_id).await?;
    if state.status != CityAvailabilityStatus::PENDING {
        return Err(reject(
            "DeliveryPartnerAvailabilityNotPending",
            json!({ "registrationId": cmd.registration_id, "currentStatus": state.status }),
        ));
    }
    let event = DomainEvent::DeliveryPartnerAvailabilityApproved(DeliveryPartnerAvailabilityApproved {
        registration_id: cmd.registration_id,
    });
    let stream = delivery_partner_registration_stream(&cmd.registration_id);
    Repository::new(store).save(&stream, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/RevokeDeliveryPartnerAvailability` → emit
/// `events.yaml#/DeliveryPartnerAvailabilityRevoked`. Requires an existing registration
/// (`DeliveryPartnerAvailabilityNotFound`); revoking is legal from any live status (withdraw/disable).
pub async fn revoke_delivery_partner_availability(
    store: &dyn EventStore,
    cmd: RevokeDeliveryPartnerAvailability,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = require_delivery_partner_registration(store, &cmd.registration_id).await?;
    let event = DomainEvent::DeliveryPartnerAvailabilityRevoked(DeliveryPartnerAvailabilityRevoked {
        registration_id: cmd.registration_id,
        reason: cmd.reason,
    });
    let stream = delivery_partner_registration_stream(&cmd.registration_id);
    Repository::new(store).save(&stream, version, &[event], actor).await.map(|_| ())
}

// ================================================================================================
// Conversation (actors.yaml#/Conversation) — per-order in-app messaging (#129).
// ================================================================================================

/// The stream a Conversation aggregate lives on — keyed by the orderId (a conversation's identity IS
/// its order, ADR-20260725-015921); byte-identical to `ConversationState::stream`.
fn conversation_stream(id: &OrderId) -> String {
    format!("Conversation-{}", id.0)
}

/// Handle `commands.yaml#/OpenConversation` → emit `events.yaml#/ConversationOpened` on the new
/// `Conversation-<orderId>` stream. The birth is idempotent-guarded: an existing fold — or losing the
/// version-0 race — rejects with `errors.yaml#/ConversationAlreadyOpen`
/// (rules.yaml#/ConversationIdentityIsTheOrder). The snapshot `customerChatEnabled` decides whether a
/// CUSTOMER may later post (see [`post_message`]).
pub async fn open_conversation(
    store: &dyn EventStore,
    cmd: OpenConversation,
    actor: &Actor,
) -> Result<(), DomainError> {
    let already = |id: &OrderId| reject("ConversationAlreadyOpen", json!({ "orderId": id }));
    let (state, _version) = Repository::new(store).load::<ConversationState>(cmd.order_id).await?;
    if state.is_some() {
        return Err(already(&cmd.order_id));
    }
    let event = DomainEvent::ConversationOpened(ConversationOpened {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
        // #235 consequence A: the aggregate folds the customer participant it will authorize
        // against (null for guest orders — acting.CUSTOMER then denies).
        customer_id: cmd.customer_id,
        customer_chat_enabled: cmd.customer_chat_enabled,
    });
    let stream = conversation_stream(&cmd.order_id);
    match Repository::new(store).save(&stream, 0, &[event], actor).await {
        Ok(_) => Ok(()),
        Err(e) if is_version_conflict(&e) => Err(already(&cmd.order_id)),
        Err(e) => Err(e),
    }
}

/// Handle `commands.yaml#/PostMessage` → emit `events.yaml#/MessagePosted` on the order's conversation
/// stream. Requires an opened conversation (`errors.yaml#/ConversationNotFound`,
/// rules.yaml#/ConversationIdentityIsTheOrder); a CUSTOMER author is rejected when the restaurant left
/// customer chat disabled (`errors.yaml#/CustomerChatDisabled`,
/// rules.yaml#/CustomerChatRequiresRestaurantOptIn); and a re-used client-generated messageId is
/// rejected as a duplicate (`errors.yaml#/MessageAlreadyPosted`, rules.yaml#/MessagePostingIsIdempotent).
pub async fn post_message(
    store: &dyn EventStore,
    cmd: PostMessage,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = Repository::new(store)
        .require::<ConversationState>(cmd.order_id, || {
            reject("ConversationNotFound", json!({ "orderId": cmd.order_id }))
        })
        .await?;
    // requires (specs/actors.yaml Conversation/PostMessage — #235): the actor's own precondition,
    // checked against its folded participants BEFORE any business invariant.
    if let Err(v) = domain::conversation::requires_post_message(
        &state,
        &actor.user_type,
        actor.domain_id,
        &cmd.author_role,
    ) {
        return Err(match v {
            domain::conversation::RequiresViolation::NotAParticipant => {
                reject("NotAParticipant", json!({ "orderId": cmd.order_id }))
            }
            domain::conversation::RequiresViolation::RoleMismatch => {
                reject("RoleMismatch", json!({ "orderId": cmd.order_id }))
            }
        });
    }
    if cmd.author_role == ConversationAuthorRole::CUSTOMER && !state.customer_chat_enabled {
        return Err(reject("CustomerChatDisabled", json!({ "orderId": cmd.order_id })));
    }
    if state.message_ids.contains(&cmd.message_id) {
        return Err(reject("MessageAlreadyPosted", json!({ "messageId": cmd.message_id })));
    }
    let event = DomainEvent::MessagePosted(MessagePosted {
        order_id: cmd.order_id,
        message_id: cmd.message_id,
        author_role: cmd.author_role,
        visibility: cmd.visibility,
        body: cmd.body,
        original_locale: cmd.original_locale,
        attachment_refs: cmd.attachment_refs,
    });
    let stream = conversation_stream(&cmd.order_id);
    Repository::new(store).save(&stream, version, &[event], actor).await.map(|_| ())
}

/// A `errors.yaml#/ConversationNotFound` rejection for the order whose conversation was never opened.
fn conversation_not_found(order_id: &OrderId) -> DomainError {
    reject("ConversationNotFound", json!({ "orderId": order_id }))
}

/// Handle `commands.yaml#/RecordMessageTranslation` → emit `events.yaml#/MessageTranslationAdded` on the
/// order's conversation stream (translate once, reuse; #129). Requires an opened conversation
/// (`errors.yaml#/ConversationNotFound`), a message that was actually posted
/// (`errors.yaml#/MessageNotFoundInConversation`, rules.yaml#/TranslationTargetsAPostedMessage), and a
/// (message, locale) pair not already cached — a re-record is rejected as a duplicate
/// (`errors.yaml#/TranslationAlreadyRecorded`, rules.yaml#/TranslationsAreCachedOncePerLocale).
pub async fn record_message_translation(
    store: &dyn EventStore,
    cmd: RecordMessageTranslation,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = Repository::new(store)
        .require::<ConversationState>(cmd.order_id, || conversation_not_found(&cmd.order_id))
        .await?;
    if !state.message_ids.contains(&cmd.message_id) {
        return Err(reject(
            "MessageNotFoundInConversation",
            json!({ "orderId": cmd.order_id, "messageId": cmd.message_id }),
        ));
    }
    if state.translations.iter().any(|(id, locale)| *id == cmd.message_id && *locale == cmd.locale) {
        return Err(reject(
            "TranslationAlreadyRecorded",
            json!({ "orderId": cmd.order_id, "messageId": cmd.message_id, "locale": cmd.locale }),
        ));
    }
    let event = DomainEvent::MessageTranslationAdded(MessageTranslationAdded {
        order_id: cmd.order_id,
        message_id: cmd.message_id,
        locale: cmd.locale,
        text: cmd.text,
    });
    let stream = conversation_stream(&cmd.order_id);
    Repository::new(store).save(&stream, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/EscalateToAdmin` → emit `events.yaml#/AdminInvitedToConversation` on the
/// order's conversation stream. Requires an opened conversation
/// (`errors.yaml#/ConversationNotFound`); the escalation carries a reason
/// (rules.yaml#/AdminJoinsByReasonedEscalation).
pub async fn escalate_to_admin(
    store: &dyn EventStore,
    cmd: EscalateToAdmin,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = Repository::new(store)
        .require::<ConversationState>(cmd.order_id, || conversation_not_found(&cmd.order_id))
        .await?;
    let event = DomainEvent::AdminInvitedToConversation(AdminInvitedToConversation {
        order_id: cmd.order_id,
        reason: cmd.reason,
    });
    let stream = conversation_stream(&cmd.order_id);
    Repository::new(store).save(&stream, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/MuteParticipant` → emit `events.yaml#/ParticipantMuted` on the order's
/// conversation stream. Requires an opened conversation (`errors.yaml#/ConversationNotFound`), and a
/// NON-EMPTY justification `reason` — a mute without one is rejected
/// (`errors.yaml#/MuteReasonRequired`, rules.yaml#/MuteRequiresAReason): the schema leaves `reason`
/// optional on purpose, so the "justified" invariant lives here in the write model.
pub async fn mute_participant(
    store: &dyn EventStore,
    cmd: MuteParticipant,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = Repository::new(store)
        .require::<ConversationState>(cmd.order_id, || conversation_not_found(&cmd.order_id))
        .await?;
    let reason = match cmd.reason {
        Some(reason) if !reason.0.trim().is_empty() => reason,
        _ => return Err(reject("MuteReasonRequired", json!({ "orderId": cmd.order_id }))),
    };
    let event = DomainEvent::ParticipantMuted(ParticipantMuted {
        order_id: cmd.order_id,
        muted_role: cmd.muted_role,
        reason,
        until: cmd.until,
    });
    let stream = conversation_stream(&cmd.order_id);
    Repository::new(store).save(&stream, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/UnmuteParticipant` → emit `events.yaml#/ParticipantUnmuted` on the order's
/// conversation stream. Requires an opened conversation (`errors.yaml#/ConversationNotFound`) and a
/// role that is CURRENTLY muted — unmuting a role that is not muted is rejected
/// (`errors.yaml#/ParticipantNotMuted`, rules.yaml#/OnlyMutedParticipantsCanBeUnmuted).
pub async fn unmute_participant(
    store: &dyn EventStore,
    cmd: UnmuteParticipant,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = Repository::new(store)
        .require::<ConversationState>(cmd.order_id, || conversation_not_found(&cmd.order_id))
        .await?;
    if !state.muted_roles.contains(&cmd.muted_role) {
        return Err(reject(
            "ParticipantNotMuted",
            json!({ "orderId": cmd.order_id, "mutedRole": cmd.muted_role }),
        ));
    }
    let event = DomainEvent::ParticipantUnmuted(ParticipantUnmuted {
        order_id: cmd.order_id,
        muted_role: cmd.muted_role,
    });
    let stream = conversation_stream(&cmd.order_id);
    Repository::new(store).save(&stream, version, &[event], actor).await.map(|_| ())
}

// ================================================================================================
// Reclamation (actors.yaml#/Reclamation) — customer claim/dispute lifecycle (#151, #153).
//
// This slice is the aggregate LIFECYCLE only: open -> resolve/reject -> reopen. `ReclamationResolved`
// records the resolution DECISION (+ amount for a PARTIAL_REFUND) so the downstream refund/credit/
// replacement slices react later; no money-move happens here. The 14-day window and order-eligibility
// (order exists/delivered) are cross-aggregate/temporal invariants enforced in the application layer in
// a follow-up (reading the order's delivered-at vs now), NOT pure-aggregate invariants.
// ================================================================================================

/// A `errors.yaml#/ReclamationNotFound` rejection for a reclamation that was never opened.
fn reclamation_not_found(reclamation_id: &ReclamationId) -> DomainError {
    reject("ReclamationNotFound", json!({ "reclamationId": reclamation_id }))
}

/// Handle `commands.yaml#/OpenReclamation` → emit `events.yaml#/ReclamationOpened` on the new
/// `Reclamation-<reclamationId>` stream. The birth is idempotent-guarded: an existing fold — or losing
/// the version-0 race — rejects with `errors.yaml#/ReclamationAlreadyExists`
/// (rules.yaml#/ReclamationIsUniquePerId). Records the requested resolution when supplied.
pub async fn open_reclamation(
    store: &dyn EventStore,
    cmd: OpenReclamation,
    actor: &Actor,
) -> Result<(), DomainError> {
    let already =
        |id: &ReclamationId| reject("ReclamationAlreadyExists", json!({ "reclamationId": id }));
    let (state, _version) = Repository::new(store).load::<ReclamationState>(cmd.reclamation_id).await?;
    if state.is_some() {
        return Err(already(&cmd.reclamation_id));
    }
    let event = DomainEvent::ReclamationOpened(ReclamationOpened {
        reclamation_id: cmd.reclamation_id,
        order_id: cmd.order_id,
        customer_id: cmd.customer_id,
        restaurant_id: cmd.restaurant_id,
        category: cmd.category,
        description: cmd.description,
        requested_resolution: cmd.requested_resolution,
    });
    let stream = format!("Reclamation-{}", cmd.reclamation_id.0);
    match Repository::new(store).save(&stream, 0, &[event], actor).await {
        Ok(_) => Ok(()),
        Err(e) if is_version_conflict(&e) => Err(already(&cmd.reclamation_id)),
        Err(e) => Err(e),
    }
}

/// Handle `commands.yaml#/ResolveReclamation` → emit `events.yaml#/ReclamationResolved` on the
/// reclamation's stream. Requires an existing reclamation (`errors.yaml#/ReclamationNotFound`) that is
/// currently OPEN (`errors.yaml#/ReclamationNotOpen`, rules.yaml#/OnlyOpenReclamationsAreDecided); a
/// PARTIAL_REFUND without a `refundAmount` is rejected (`errors.yaml#/PartialRefundAmountRequired`,
/// rules.yaml#/PartialRefundResolutionCarriesAnAmount). Records the decision only — no money-move.
pub async fn resolve_reclamation(
    store: &dyn EventStore,
    cmd: ResolveReclamation,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = Repository::new(store)
        .require::<ReclamationState>(cmd.reclamation_id, || reclamation_not_found(&cmd.reclamation_id))
        .await?;
    if state.status != ReclamationStatus::OPEN {
        return Err(reject("ReclamationNotOpen", json!({ "reclamationId": cmd.reclamation_id })));
    }
    if cmd.resolution == ReclamationResolution::PARTIAL_REFUND && cmd.refund_amount.is_none() {
        return Err(reject(
            "PartialRefundAmountRequired",
            json!({ "reclamationId": cmd.reclamation_id }),
        ));
    }
    let event = DomainEvent::ReclamationResolved(ReclamationResolved {
        reclamation_id: cmd.reclamation_id,
        // The order rides along from the aggregate's fold state (established at ReclamationOpened) so the
        // claim lifecycle can be woven into the per-order conversation thread, keyed by order (§2.5, #155).
        order_id: state.order_id,
        resolution: cmd.resolution,
        // The claimant rides along from fold state (established at ReclamationOpened) so the
        // ReclamationProcess saga can grant goodwill credit without a read step (#158).
        customer_id: state.customer_id,
        note: cmd.note,
        refund_amount: cmd.refund_amount,
    });
    let stream = format!("Reclamation-{}", cmd.reclamation_id.0);
    Repository::new(store).save(&stream, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/RejectReclamation` → emit `events.yaml#/ReclamationRejected` on the
/// reclamation's stream. Requires an existing reclamation (`errors.yaml#/ReclamationNotFound`) that is
/// currently OPEN (`errors.yaml#/ReclamationNotOpen`, rules.yaml#/OnlyOpenReclamationsAreDecided) and a
/// NON-EMPTY `reason` — a rejection without one is rejected (`errors.yaml#/RejectionReasonRequired`,
/// rules.yaml#/ReclamationRejectionCarriesAReason): the schema leaves `reason` optional on purpose, so
/// the "reasoned" invariant lives here in the write model (like MuteParticipant).
pub async fn reject_reclamation(
    store: &dyn EventStore,
    cmd: RejectReclamation,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = Repository::new(store)
        .require::<ReclamationState>(cmd.reclamation_id, || reclamation_not_found(&cmd.reclamation_id))
        .await?;
    if state.status != ReclamationStatus::OPEN {
        return Err(reject("ReclamationNotOpen", json!({ "reclamationId": cmd.reclamation_id })));
    }
    let reason = match cmd.reason {
        Some(reason) if !reason.0.trim().is_empty() => reason,
        _ => {
            return Err(reject(
                "RejectionReasonRequired",
                json!({ "reclamationId": cmd.reclamation_id }),
            ))
        }
    };
    let event = DomainEvent::ReclamationRejected(ReclamationRejected {
        reclamation_id: cmd.reclamation_id,
        // Order rides along from fold state so the claim weaves into the order thread, keyed by order (#155).
        order_id: state.order_id,
        reason,
    });
    let stream = format!("Reclamation-{}", cmd.reclamation_id.0);
    Repository::new(store).save(&stream, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/ReopenReclamation` → emit `events.yaml#/ReclamationReopened` on the
/// reclamation's stream. Requires an existing reclamation (`errors.yaml#/ReclamationNotFound`) in a
/// DECIDED (resolved or rejected) state — reopening one still OPEN is rejected
/// (`errors.yaml#/ReclamationNotReopenable`, rules.yaml#/OnlyDecidedReclamationsCanBeReopened). The
/// `reason` is optional (a reopen need not state why).
pub async fn reopen_reclamation(
    store: &dyn EventStore,
    cmd: ReopenReclamation,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = Repository::new(store)
        .require::<ReclamationState>(cmd.reclamation_id, || reclamation_not_found(&cmd.reclamation_id))
        .await?;
    if state.status == ReclamationStatus::OPEN {
        return Err(reject(
            "ReclamationNotReopenable",
            json!({ "reclamationId": cmd.reclamation_id }),
        ));
    }
    let event = DomainEvent::ReclamationReopened(ReclamationReopened {
        reclamation_id: cmd.reclamation_id,
        // Order rides along from fold state so the claim weaves into the order thread, keyed by order (#155).
        order_id: state.order_id,
        reason: cmd.reason,
    });
    let stream = format!("Reclamation-{}", cmd.reclamation_id.0);
    Repository::new(store).save(&stream, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/AttachReclamationEvidence` → emit `events.yaml#/ReclamationEvidenceAttached`
/// on the reclamation's stream. The only guard is that the reclamation exists
/// (`errors.yaml#/ReclamationNotFound`, rules.yaml#/ReclamationEvidenceTargetsAnExistingClaim) — evidence
/// may be attached in ANY lifecycle state (no status guard), so the fold is unchanged. `orderId` is NOT
/// on the command: it rides along from the aggregate's fold state (established at ReclamationOpened) so
/// the evidence weaves into the per-order conversation thread, keyed by order (§2.5, #155/#156). The
/// `attachmentRef` is an opaque, framework-managed ref; the file upload/storage is out of scope (#134).
pub async fn attach_reclamation_evidence(
    store: &dyn EventStore,
    cmd: AttachReclamationEvidence,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = Repository::new(store)
        .require::<ReclamationState>(cmd.reclamation_id, || reclamation_not_found(&cmd.reclamation_id))
        .await?;
    let event = DomainEvent::ReclamationEvidenceAttached(ReclamationEvidenceAttached {
        reclamation_id: cmd.reclamation_id,
        order_id: state.order_id,
        attachment_ref: cmd.attachment_ref,
    });
    let stream = format!("Reclamation-{}", cmd.reclamation_id.0);
    Repository::new(store).save(&stream, version, &[event], actor).await.map(|_| ())
}

// ================================================================================================
// MailboxSupervision (actors.yaml#/MailboxSupervision) — operator actions over the mailbox (#315).
// ================================================================================================

/// Handle `commands.yaml#/RequeueMailboxMessage` → emit `events.yaml#/MailboxMessageRequeued` on
/// the supervised row's stream (#315, ADR-20260803-002712 Q1,
/// rules.yaml#/OnlyCapPoisonedMailboxRowsAreRequeueable). The `MailboxRequeue` port arbitrates
/// AND applies the flip in one statement (no check-then-act window); only a row the
/// delivery-attempts cap poisoned (`DeliveryInfrastructureError`) flips — anything else rejects
/// (`errors.yaml#/MailboxMessageNotFound` / `errors.yaml#/MailboxMessageNotRequeueable`). Like
/// the slug-reservation port, the row write lands alongside (not inside) the event append: the
/// port is idempotent (an already-deliverable row converges as recorded success), so a retried
/// delivery of this command never errors on its own earlier effect.
pub async fn requeue_mailbox_message(
    store: &dyn EventStore,
    requeue: &dyn crate::queries::MailboxRequeue,
    cmd: domain::generated::commands::RequeueMailboxMessage,
    actor: &Actor,
) -> Result<(), DomainError> {
    use crate::queries::RequeueOutcome;
    // THE production mint of the requeue witness (#510): this handler is the one door to the
    // flip, and the mailbox is the one door to this handler.
    let access = crate::queries::MailboxRequeueAccess::granted();
    let actor_type = match requeue.requeue_if_poisoned(cmd.target_message_id.0, access).await? {
        RequeueOutcome::Requeued { actor_type }
        | RequeueOutcome::AlreadyDeliverable { actor_type } => actor_type,
        RequeueOutcome::NotFound => {
            return Err(reject(
                "MailboxMessageNotFound",
                json!({ "targetMessageId": cmd.target_message_id }),
            ));
        }
        RequeueOutcome::NotRequeueable { status } => {
            return Err(reject(
                "MailboxMessageNotRequeueable",
                json!({ "targetMessageId": cmd.target_message_id, "status": status }),
            ));
        }
    };
    let (_ledger, version) = Repository::new(store)
        .load::<domain::mailbox_supervision::MailboxSupervisionState>(cmd.target_message_id)
        .await?;
    let event = DomainEvent::MailboxMessageRequeued(
        domain::generated::events::MailboxMessageRequeued {
            target_message_id: cmd.target_message_id,
            actor_type: domain::generated::scalars::MailboxActorType(actor_type),
        },
    );
    let stream = format!("MailboxSupervision-{}", cmd.target_message_id.0);
    Repository::new(store).save(&stream, version, &[event], actor).await.map(|_| ())
}

// ================================================================================================
// CustomerCredit (actors.yaml#/CustomerCredit) — the per-customer store-credit ledger (#158).
// Both commands are saga-/checkout-driven (no public GraphQL mutation). The available balance is a
// fold over the grant/consume facts; grants are idempotent per reclamationId; a debit never drives
// the balance negative (errors.yaml#/InsufficientCustomerCredit). ADR-20260726-163737.
// ================================================================================================

/// Handle `commands.yaml#/GrantCustomerCredit` → emit `events.yaml#/CustomerCreditGranted` on the
/// customer's ledger stream (`CustomerCredit-<customerId>`). Idempotent per `reclamationId`: a re-grant
/// for a claim already granted (e.g. a re-delivered `ReclamationResolved` from the ReclamationProcess
/// saga) is a benign no-op — no second grant, no double-credit (rules.yaml#/GoodwillCreditGrantedOnResolution).
pub async fn grant_customer_credit(
    store: &dyn EventStore,
    cmd: GrantCustomerCredit,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) =
        Repository::new(store).load::<CustomerCreditState>(cmd.customer_id).await?;
    // Idempotent grant: at most one credit per resolved claim.
    if state.as_ref().is_some_and(|s| s.already_granted(&cmd.reclamation_id)) {
        return Ok(());
    }
    let event = DomainEvent::CustomerCreditGranted(CustomerCreditGranted {
        customer_id: cmd.customer_id,
        amount: cmd.amount,
        reclamation_id: cmd.reclamation_id,
    });
    let stream = format!("CustomerCredit-{}", cmd.customer_id.0);
    Repository::new(store).save(&stream, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/ConsumeCustomerCredit` → emit `events.yaml#/CustomerCreditConsumed` on the
/// customer's ledger stream. Spending more than the available balance is rejected
/// (`errors.yaml#/InsufficientCustomerCredit`, rules.yaml#/CreditCannotBeOverspent) — the balance never
/// goes negative. The available balance is `Σ granted − Σ consumed` (folded from the log).
pub async fn consume_customer_credit(
    store: &dyn EventStore,
    cmd: ConsumeCustomerCredit,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) =
        Repository::new(store).load::<CustomerCreditState>(cmd.customer_id).await?;
    // Exactly-once per order (rules.yaml#/CreditConsumedAtMostOncePerOrder): the debit is keyed by the
    // client-minted `orderId`, so a re-delivered / retried consume for an order already debited is a
    // benign no-op — the credit is never spent twice on the same order (a placeOrder retry with the
    // same orderId consumes nothing more). Checked BEFORE the balance guard: a replay must not read as
    // an overspend just because the balance already fell on the first (recorded) consume.
    if state.as_ref().and_then(|s| s.consumed_for(&cmd.order_id)).is_some() {
        return Ok(());
    }
    let balance = state.as_ref().map(|s| s.balance_cents).unwrap_or(0);
    if cmd.amount.amount_cents.0 > balance {
        return Err(reject(
            "InsufficientCustomerCredit",
            json!({ "customerId": cmd.customer_id }),
        ));
    }
    let event = DomainEvent::CustomerCreditConsumed(CustomerCreditConsumed {
        customer_id: cmd.customer_id,
        amount: cmd.amount,
        order_id: cmd.order_id,
    });
    let stream = format!("CustomerCredit-{}", cmd.customer_id.0);
    Repository::new(store).save(&stream, version, &[event], actor).await.map(|_| ())
}

/// The store credit (minor units) to apply to a checkout, read cross-aggregate from the customer's
/// `CustomerCredit` ledger (#158, Part B of #207). RETRY-STABLE: if this order (client-minted `orderId`)
/// was ALREADY debited, reuse that exact amount — never recompute against a now-lower balance, so a
/// placeOrder retry applies the same credit and consumes nothing more. Otherwise apply
/// `min(available balance, order total)`, and ONLY when the ledger currency matches the order currency
/// (never convert). Zero for a guest checkout (no `customerId`) or a customer with no ledger. The
/// returned amount is `0 ≤ applied ≤ order_total`, so the buyer total (gross − applied) is never
/// negative — money stays exact.
pub(crate) async fn credit_to_apply(
    store: &dyn EventStore,
    customer_id: CustomerId,
    order_id: &OrderId,
    order_total: &Money,
) -> Result<i64, DomainError> {
    let (state, _version) = Repository::new(store).load::<CustomerCreditState>(customer_id).await?;
    let Some(state) = state else { return Ok(0) };
    // Retry-stable: a re-submitted checkout for the same order reuses the amount already consumed.
    if let Some(already) = state.consumed_for(order_id) {
        return Ok(already.clamp(0, order_total.amount_cents.0));
    }
    // Currency must match (no conversion); otherwise no credit applies.
    if state.currency.as_ref() != Some(&order_total.currency) {
        return Ok(0);
    }
    Ok(state.balance_cents.clamp(0, order_total.amount_cents.0))
}

// ================================================================================================
// Replacement order (actors.yaml#/Order) — the ReclamationProcess REPLACEMENT arm's command leg
// (#159, ADR-20260726-171736). Saga-driven, no public GraphQL mutation (command-no-mutation).
// ================================================================================================

/// A zeroed `Money` in `currency` — a replacement order's buyer total and every breakdown line are $0
/// (no money moves, no Stripe): only the ITEMS carry over, so the restaurant knows what to remake.
fn zero_money(currency: &CurrencyCode) -> Money {
    Money { amount_cents: domain::generated::scalars::MoneyCents(0), currency: currency.clone() }
}

/// Handle `commands.yaml#/PlaceReplacementOrder` → emit `events.yaml#/OrderPlaced` on a NEW
/// `Order-<orderId>` stream, carrying the ORIGINAL order's line items + delivery details, a $0 buyer
/// total, a zeroed breakdown, NO `paymentIntentId`, and `replacementOf` = the original order id
/// (rules.yaml#/ReplacementOrderPlacedOnResolution). The replacement then flows through the normal
/// fulfilment/dispatch as any order (the restaurant remakes it, the rider redelivers).
///
/// The original order is read cross-aggregate by folding its stream for the birth `OrderPlaced` (the
/// same pattern as PlaceOrderProcess reads its frozen checkout); a missing original rejects with
/// `errors.yaml#/OrderNotFound`. Idempotent per resolved claim: the saga derives `orderId`
/// deterministically from `reclamationId`, so a re-delivered `ReclamationResolved` re-targets the same
/// new-order stream and the version-0 append is absorbed as a no-op — never a second replacement.
pub async fn place_replacement_order(
    store: &dyn EventStore,
    cmd: PlaceReplacementOrder,
    actor: &Actor,
) -> Result<(), DomainError> {
    // Read the original order's birth fact (cross-aggregate, by orderId) — the source of the items and
    // delivery details to copy. `require_order`-style scoping is not applied: a replacement is placed by
    // the saga, not a tenant, and there is no commanding restaurant to scope against.
    let (original_events, _) = store.load(&order_stream(&cmd.original_order_id)).await?;
    let Some(original) = original_events.iter().find_map(|e| match e {
        DomainEvent::OrderPlaced(p) => Some(p.clone()),
        _ => None,
    }) else {
        return Err(reject("OrderNotFound", json!({ "orderId": cmd.original_order_id })));
    };
    let currency = original.total_amount.currency.clone();
    let event = DomainEvent::OrderPlaced(OrderPlaced {
        mode: original.mode,
        order_id: cmd.order_id,
        // A replacement is a NEW order — never carry the original's external `ref` (it is HubRise's
        // idempotent import key; reusing it would collide).
        r#ref: None,
        restaurant_id: original.restaurant_id,
        customer_id: original.customer_id,
        customer_contact: original.customer_contact.clone(),
        service_type: original.service_type,
        delivery_address: original.delivery_address.clone(),
        items: original.items.clone(),
        total_amount: zero_money(&currency),
        breakdown: PaymentBreakdown {
            articles: zero_money(&currency),
            delivery: zero_money(&currency),
            service_fee: zero_money(&currency),
            total: zero_money(&currency),
            restaurant_contribution: zero_money(&currency),
            restaurant_payout: zero_money(&currency),
            rider_payout: zero_money(&currency),
            captain_net: zero_money(&currency),
        },
        note: original.note.clone(),
        replacement_of: Some(cmd.original_order_id),
        // No charge: a replacement has no Stripe PaymentIntent.
        payment_intent_id: None,
    });
    // Version-0 birth: a re-delivered claim (same deterministic orderId) clashes and is absorbed — one
    // replacement per resolved claim, never two.
    create_if_absent(store, &order_stream(&cmd.order_id), &[event], actor).await.map(|_| ())
}

// ================================================================================================
// PlaceOrderProcess (actors.yaml#/PlaceOrderProcess) — the checkout saga's command leg.
// ================================================================================================

/// Handle `commands.yaml#/PlaceOrder` → DELIVER `events.yaml#/PaymentIntentCreated` to the Payment
/// aggregate's stream (`Payment-<paymentIntentId>`, ADR-20260719-193500 — the Payment is BORN by this
/// fact, carrying the frozen checkout snapshot the capture leg reads back from the log) and open the
/// PlaceOrderProcess run as a `payment_process_manager` row (AWAITING_PAYMENT_RESULT, keyed by cart).
/// This is ONLY the saga's first, command-initiated leg: validate the checkout, price the cart
/// server-side from the LIVE catalog (`crate::pricing::price_cart` —
/// rules.yaml#/ServerPriceAuthority: the server is the only price authority; an unresolvable line
/// price rejects fail-closed with `errors.yaml#/PriceUnresolvable`, and a client `expectedTotal`
/// that diverges from the recomputed total rejects with `errors.yaml#/PriceMismatch`) and create
/// the Stripe PaymentIntent through the generated [`PaymentService`] port for exactly that recomputed amount
/// (a synchronous decline is the canonical `errors.yaml#/PaymentDeclined`). Returns the created
/// intent so the mutation payload can carry `paymentIntentId`/`clientSecret` (api.yaml).
///
/// Single-flight per cart: a live run still AWAITING_PAYMENT_RESULT for this cart means a concurrent
/// (or double-submitted) checkout — rejected with the cross-cutting `errors.yaml#/Conflict` (retry
/// semantics) BEFORE any gateway call, so no second Stripe intent is ever created for the same cart.
/// A previously FAILED/resolved run does not block: the retry upserts a fresh row (same cart pk).
///
/// The remaining PlaceOrderProcess legs are event-driven and live in
/// [`crate::process_managers::place_order`] (run by the infrastructure `ProcessManagerRunner`):
///   * `events.yaml#/PaymentCaptured` (INBOUND Stripe webhook, CLAUDE.md "Commands vs inbound
///     events") → emit `OrderPlaced` on `Order-<orderId>` and `CartCheckedOut` on `Cart-<cartId>`,
///     from the checkout snapshot frozen on the Payment stream;
///   * `events.yaml#/PaymentFailed` (INBOUND) → abort: no OrderPlaced, the cart stays OPEN.
/// Z-normalized RFC3339 (`2026-08-14T17:00:00Z`) — the ONE grammar the RSO-1 snapshot evidence
/// uses, matching `when.at` in specs/tests.yaml (never `+00:00`, which `to_rfc3339()` emits and
/// the fixtures would fail string-equality on).
fn rfc3339_z(at: chrono::DateTime<chrono::Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub async fn place_order(
    store: &dyn EventStore,
    catalogs: &dyn CatalogReadRepository,
    payments: &dyn PaymentService,
    pm_state: &dyn PaymentProcessStateStore,
    cmd: PlaceOrder,
    // Envelope data, not command payload (ADR-0041): the dispatch-layer X-SESSION-ID, stamped onto
    // the PM run row so an anonymous checkout can read paymentStatus after a restart (#12).
    session_id: Option<domain::generated::scalars::SessionId>,
    actor: &Actor,
    // RSO-1 (DECISIONS §43): the EVALUATION INSTANT — read once at the delivery seam and passed
    // as a parameter (the `sms_guard.rs` precedent: a handler reading the system clock internally
    // cannot have both edges of the service window asserted). Phase 3 records the service-window
    // EVIDENCE onto the frozen snapshot with it; Phase 4 adds the refusing guard below.
    when_at: chrono::DateTime<chrono::Utc>,
    // RSO-1 Phase 4: `configuration.yaml#/ENFORCE_SERVICE_HOURS_GUARD`, resolved ONCE at the
    // delivery seam and passed as a parameter, same style as `when_at` — no global/env read
    // inside the handler, so the in-process test suite stays order-independent and both gate
    // edges are assertable (tests.yaml `when.gates`).
    enforce_service_hours_guard: bool,
) -> Result<PaymentRequestOutput, DomainError> {
    // The restaurant must exist, be ACTIVE and not PAUSED — folded from ITS stream (authoritative,
    // race-free; the saga may read other aggregates' streams through the same EventStore port).
    let (restaurant_events, _) =
        Repository::new(store).events::<RestaurantState>(cmd.restaurant_id).await?;
    let Some(restaurant) = domain::restaurant::fold(&restaurant_events) else {
        return Err(reject("RestaurantNotFound", json!({ "restaurantId": cmd.restaurant_id })));
    };
    if restaurant.status != RestaurantStatus::ACTIVE {
        return Err(reject(
            "RestaurantNotActive",
            json!({ "restaurantId": cmd.restaurant_id, "restaurantName": restaurant.display_name }),
        ));
    }
    if restaurant.order_acceptance == OrderAcceptanceMode::PAUSED {
        return Err(reject(
            "RestaurantPaused",
            json!({ "restaurantId": cmd.restaurant_id, "restaurantName": restaurant.display_name }),
        ));
    }
    // Test-mode isolation (ADR-0038, rules.yaml#/OrderTestModeIsolation): a LIVE order (mode absent =
    // LIVE) never reaches a TEST restaurant; a TEST order MAY target a LIVE restaurant.
    let restaurant_is_test = restaurant_events
        .iter()
        .any(|e| matches!(e, DomainEvent::RestaurantRegistered(r) if r.mode == Some(Mode::TEST)));
    if restaurant_is_test && cmd.mode != Some(Mode::TEST) {
        return Err(reject("CannotOrderTestRestaurant", json!({ "restaurantId": cmd.restaurant_id })));
    }
    // The service-hours evaluation AT the injected instant (RSO-1): the ONE domain function the
    // storefront badge also renders from, over the fold's own declared hours + timezone — so the
    // number a refusal would enforce is the number the badge displayed. Cutoff: no source is
    // mapped today (HubRise `cutoff_time`), so `None` degrades explicitly to door-close. The
    // horizon is irrelevant here — the frozen EVIDENCE deliberately has no validUntil (a
    // config-derived TTL is not history), so zero makes the non-use explicit. Phase 3 RECORDS
    // the verdict (even in shadow mode the verdict is recorded — the gate changes whether
    // OUTSIDE_HOURS refuses, never whether it is recorded); the refusal is Phase 4.
    let service_window = domain::service_window::serving_at(
        &restaurant.opening_hours,
        restaurant.timezone.as_ref(),
        None,
        when_at,
        chrono::Duration::zero(),
    );
    // THE REFUSING GUARD (RSO-1 Phase 4, rules.yaml#/CheckoutRefusesOnlyOutsideServiceHours):
    // OUTSIDE_HOURS is the ONLY refusing verdict — OPEN and HOURS_UNDECLARED both accept — and
    // only while the gate is ON (OFF = shadow: the verdict is still frozen onto the snapshot
    // below, it just never refuses). Positioned immediately after the evaluation and BEFORE any
    // external effect (`payments.request` creates a Stripe intent; refusing after it would
    // strand a real intent for an order we never meant to take). Decided off the FOLDED
    // RestaurantState evaluation in hand — never a snapshot, never a projection — so the number
    // this refusal enforces is the number the storefront badge displayed. The context carries the
    // next opening slot (the actionable half of the message) plus the refusal EVIDENCE:
    // window/timezone/instant, so a disputed refusal is provable from the record. Under
    // OUTSIDE_HOURS `serving_at` carries no window instants (they exist under OPEN only), so
    // windowFrom/windowTo are null here by construction — the declared context is nullable for
    // exactly this shape.
    if enforce_service_hours_guard && service_window.verdict == ServiceWindowVerdict::OUTSIDE_HOURS
    {
        return Err(reject(
            "OutsideServiceHours",
            json!({
                "restaurantId": cmd.restaurant_id,
                "restaurantName": restaurant.display_name,
                "nextOpensAt": service_window.opens_at.map(rfc3339_z),
                "windowFrom": service_window.window_from.map(rfc3339_z),
                "windowTo": service_window.last_order_at.map(rfc3339_z),
                // OUTSIDE_HOURS requires a parseable timezone by construction (an unusable one
                // degrades the verdict to HOURS_UNDECLARED), so this is always present here.
                "timezone": restaurant.timezone,
                "evaluatedAt": rfc3339_z(when_at),
            }),
        ));
    }
    // The cart must exist, be OPEN, belong to this restaurant and hold at least one line.
    let (cart, _cart_version) = require_cart(store, &cmd.cart_id).await?;
    if cart.status != CartStatus::OPEN {
        return Err(cart_not_open(&cmd.cart_id, cart.status));
    }
    if cart.restaurant_id != cmd.restaurant_id {
        return Err(reject(
            "CartRestaurantMismatch",
            json!({
                "cartId": cmd.cart_id,
                "restaurantId": cmd.restaurant_id,
                "restaurantName": restaurant.display_name,
            }),
        ));
    }
    if cart.line_ids.is_empty() {
        return Err(reject("CartEmpty", json!({ "cartId": cmd.cart_id })));
    }
    // Single-flight per cart (the row's `by`/`expect` idempotency, ADR-20260719-193500): a run still
    // awaiting its Stripe outcome means this cart's checkout is already in flight — reject before the
    // gateway so no second intent is created and no money can be taken twice.
    if let Some(run) = pm_state.by_cart(cmd.cart_id).await? {
        if run.process_status == PaymentProcessStatus::AWAITING_PAYMENT_RESULT {
            return Err(reject("Conflict", json!({})));
        }
    }
    // DELIVERY requires an address.
    if cmd.service_type == ServiceType::DELIVERY && cmd.delivery_address.is_none() {
        return Err(reject("DeliveryAddressRequired", json!({})));
    }
    // TODO(invariant): OutsideDeliveryArea — needs a delivery-area policy port (the restaurant's
    //                  delivery zone is not modelled in any read port yet).
    // TODO(invariant): OfferUnavailable / InsufficientStock / InvalidOptionSelection — re-validating
    //                  each line's ORDERABILITY at checkout (pricing below already fails closed on a
    //                  line that has left the catalog, but availability/stock re-checks are pending).
    // Price the cart server-side from the LIVE catalog (rules.yaml#/ServerPriceAuthority): the fold's
    // lines (offer + quantity + selected options — authoritative, from the cart's own stream) are
    // repriced through the Catalog read port. Fail-closed: an unresolvable line price rejects with
    // `PriceUnresolvable` — never a fallback to any client number.
    let priced = crate::pricing::price_cart(catalogs, cmd.cart_id, cmd.restaurant_id, &cart.lines).await?;
    // The client's expectedTotal (optional) is a CONFIRMATION only — checked for equality against the
    // recomputed total so the customer is never charged an amount other than the one displayed.
    if let Some(expected) = &cmd.expected_total {
        if *expected != priced.total_amount {
            return Err(reject(
                "PriceMismatch",
                json!({
                    "cartId": cmd.cart_id,
                    "expectedAmountCents": priced.total_amount.amount_cents,
                    "submittedAmountCents": expected.amount_cents,
                    "currency": priced.total_amount.currency,
                }),
            ));
        }
    }
    let gross = priced.total_amount.clone();
    // Store credit (goodwill) is SPENT at checkout (#158, Part B of #207): the available balance reduces
    // the CASH the buyer pays, up to the order total (currency-matched, never negative), keyed by the
    // order for exactly-once spend. The PaymentIntent is created for the REDUCED buyer total; the order's
    // OWN value (the frozen snapshot / breakdown / totalAmount below) stays the GROSS — the credit is a
    // tender covering part of it, recorded as CustomerCreditConsumed on the ledger. Surfacing the
    // applied line on the order receipt (an OrderTracking column) is a flagged follow-up
    // (ADR-20260726-163737 §checkout-consume); the balance drop (queries/customerCredit) is the
    // customer-visible proof today.
    let applied_credit = credit_to_apply(store, cmd.customer_id, &cmd.order_id, &gross).await?;
    let buyer_total = Money {
        amount_cents: domain::generated::scalars::MoneyCents(gross.amount_cents.0 - applied_credit),
        currency: gross.currency.clone(),
    };
    // Create the Stripe PaymentIntent through the generated service port FOR THE BUYER TOTAL (gross −
    // applied credit); a synchronous decline surfaces as the canonical `PaymentDeclined` rejection. The
    // `orderId`/`restaurantId`/`cartId` refs are the ENVELOPE the Stripe ACL copies onto the intent's
    // `metadata` so the inbound webhook facts can be mapped back onto our aggregates (issue #26).
    let intent = payments
        .request(
            PaymentRequestInput {
                amount: buyer_total.clone(),
                payment_method_id: domain::generated::scalars::PaymentMethodId(cmd.payment_method_id.clone()),
            },
            &ServiceCallMeta::new(actor.correlation_id)
                .with_ref("orderId", cmd.order_id.0.to_string())
                .with_ref("restaurantId", cmd.restaurant_id.0.to_string())
                .with_ref("cartId", cmd.cart_id.0.to_string()),
        )
        .await?;
    // The gateway accepted (no synchronous decline) — SPEND the credit now, exactly-once per order
    // (idempotent on a retry with the same orderId; the ledger fold rejects/ignores a second consume).
    // Done AFTER the intent so a declined checkout never consumes credit; the balance guard in the
    // handler still holds should the balance have fallen since it was read (a rare same-customer race).
    if applied_credit > 0 {
        consume_customer_credit(
            store,
            ConsumeCustomerCredit {
                customer_id: cmd.customer_id,
                amount: Money {
                    amount_cents: domain::generated::scalars::MoneyCents(applied_credit),
                    currency: gross.currency.clone(),
                },
                order_id: cmd.order_id,
            },
            actor,
        )
        .await?;
    }
    // Freeze the priced checkout onto the event so PlaceOrderProcess can rebuild OrderPlaced +
    // CartCheckedOut from the log on capture (rules.yaml#/CheckoutSnapshotFrozenAtIntent): the
    // server-priced items, total and breakdown — all recomputed above from the live catalog
    // (the ADR-0016/0017 fee/split policy plugs into `pricing` when it lands).
    let checkout = CheckoutSnapshot {
        order_id: cmd.order_id,
        cart_id: cmd.cart_id,
        restaurant_id: cmd.restaurant_id,
        customer_id: cmd.customer_id,
        mode: cmd.mode,
        r#ref: None,
        customer_contact: cmd.customer_contact.clone(),
        service_type: cmd.service_type,
        delivery_address: cmd.delivery_address.clone(),
        items: priced.items.clone(),
        // The order's OWN value is the GROSS server-priced total; applied store credit reduces only the
        // Stripe charge (buyer_total above), not what the order is worth (rules.yaml#/CreditReducesCheckoutTotal).
        total_amount: gross.clone(),
        breakdown: priced.breakdown.clone(),
        note: cmd.note.clone(),
        // RSO-1 acceptance EVIDENCE: the verdict this checkout was evaluated at, with the
        // window's own instants and timezone — a disputed acceptance is proved from these, not
        // from an enum alone. Z-normalized RFC3339, matching the tests.yaml fixture grammar.
        verdict: Some(service_window.verdict),
        window_from: service_window.window_from.map(rfc3339_z),
        window_to: service_window.last_order_at.map(rfc3339_z),
        timezone: restaurant.timezone.clone(),
        evaluated_at: Some(rfc3339_z(when_at)),
    };
    // Deliver the saga's first fact to the Payment aggregate's stream — its BIRTH (the Order stream
    // stays empty until the capture leg materializes OrderPlaced). `create` absorbs a version-0 clash:
    // the gateway is idempotent per payment method+cart replay windows, so re-hitting an existing
    // `Payment-<intentId>` stream is a re-delivered birth, not a new fact.
    let event = DomainEvent::PaymentIntentCreated(PaymentIntentCreated {
        payment_intent_id: intent.payment_intent_id.clone(),
        restaurant_id: cmd.restaurant_id,
        customer_id: cmd.customer_id,
        // The PaymentIntent charges the BUYER TOTAL (gross − applied store credit); the frozen `checkout`
        // above keeps the gross order value the capture leg materializes OrderPlaced from.
        amount: buyer_total,
        checkout,
    });
    // Create-if-absent rather than the birth-and-swallow `Repository::create`: a retried checkout for
    // the same intent must not write a dead tuple into `domain_events` to discover it already exists
    // (ADR-20260728-011344).
    create_if_absent(store, &domain::payment::stream(&intent.payment_intent_id), &[event], actor)
        .await?;
    // Open the PM run: one `payment_process_manager` row keyed by cart, AWAITING_PAYMENT_RESULT until
    // the inbound Stripe outcome resolves it. `last_update_utc` is stamped server-side by the store
    // (the value below is ignored on write).
    pm_state
        .upsert(&PaymentProcessRow {
            cart_id: cmd.cart_id,
            order_id: cmd.order_id,
            payment_intent_id: intent.payment_intent_id.clone(),
            process_status: PaymentProcessStatus::AWAITING_PAYMENT_RESULT,
            payment_status: PaymentStatus::PENDING,
            // Initiator scope + credential for the paymentStatus read (ADR-20260720-015500). The
            // customer is now ALWAYS present (PlaceOrder.customerId is required as of #144 —
            // checkout verifies a phone, which registers or resolves the Customer), so this is a
            // widening, not a maybe. The anonymous session is still recorded when the header was
            // present: it is a second, device-scoped credential that survives an app restart
            // (#12, ADR-20260720-213000), not a fallback for an unidentified buyer.
            customer_id: Some(cmd.customer_id),
            session_id,
            client_secret: Some(intent.client_secret.clone()),
            last_processed_stripe_event_id: None,
            last_update_utc: chrono::Utc::now(),
        })
        .await?;
    Ok(intent)
}

// ================================================================================================
// Prospect aggregate (ADR-0020) — id = restaurantId; born by its first recorded contact.
// ================================================================================================

/// The stream a Prospect aggregate lives on (id = the prospected restaurant's id).
fn prospect_stream(id: &RestaurantId) -> String {
    format!("Prospect-{}", id.0)
}

/// Rehydrate the Prospect aggregate (fold + current version).
async fn load_prospect(
    store: &dyn EventStore,
    id: &RestaurantId,
) -> Result<(Option<ProspectState>, i64), DomainError> {
    Repository::new(store).load::<ProspectState>(*id).await
}

/// Handle `commands.yaml#/RecordProspectContact` → emit `events.yaml#/ProspectContacted`. The first
/// contact is the prospect's birth. Anti-spam invariants: at most 3 contacts total
/// (`ProspectContactLimitReached`, from the fold) and ≥ 7 days between contacts
/// (`ProspectContactedTooRecently`) — the contact TIME is envelope metadata (`occurred_at`) invisible
/// to the fold, so it is read from the `ProspectionPipeline` projection's `last_contacted_at` (the
/// same read model the prospection worker schedules from; a not-yet-projected prospect passes).
pub async fn record_prospect_contact(
    store: &dyn EventStore,
    prospection: &dyn ProspectionReadRepository,
    cmd: RecordProspectContact,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = load_prospect(store, &cmd.restaurant_id).await?;
    if state.as_ref().map_or(0, |s| s.contacts) >= 3 {
        return Err(reject(
            "ProspectContactLimitReached",
            json!({ "restaurantId": cmd.restaurant_id }),
        ));
    }
    let row = prospection
        .list(ProspectFilter::default())
        .await?
        .into_iter()
        .find(|r| r.restaurant_id == cmd.restaurant_id);
    if let Some(last) = row.and_then(|r| r.last_contacted_at) {
        if chrono::Utc::now().signed_duration_since(last) < chrono::Duration::days(7) {
            return Err(reject(
                "ProspectContactedTooRecently",
                json!({ "restaurantId": cmd.restaurant_id }),
            ));
        }
    }
    let stream_name = prospect_stream(&cmd.restaurant_id);
    let event = DomainEvent::ProspectContacted(ProspectContacted {
        restaurant_id: cmd.restaurant_id,
        channel: cmd.channel,
        sequence_step: cmd.sequence_step,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/MarkProspectCold` → emit `events.yaml#/ProspectMarkedCold`. Requires a
/// contact history (`ProspectNotFound`): a never-contacted listing is not a prospect yet.
pub async fn mark_prospect_cold(
    store: &dyn EventStore,
    cmd: MarkProspectCold,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = load_prospect(store, &cmd.restaurant_id).await?;
    if state.is_none() {
        return Err(reject("ProspectNotFound", json!({ "restaurantId": cmd.restaurant_id })));
    }
    let stream_name = prospect_stream(&cmd.restaurant_id);
    let event = DomainEvent::ProspectMarkedCold(ProspectMarkedCold {
        restaurant_id: cmd.restaurant_id,
        reason: cmd.reason,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/RecordProspectReply` → emit `events.yaml#/ProspectReplied`. Requires a
/// contact history (`ProspectNotFound`).
pub async fn record_prospect_reply(
    store: &dyn EventStore,
    cmd: RecordProspectReply,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = load_prospect(store, &cmd.restaurant_id).await?;
    if state.is_none() {
        return Err(reject("ProspectNotFound", json!({ "restaurantId": cmd.restaurant_id })));
    }
    let stream_name = prospect_stream(&cmd.restaurant_id);
    let event = DomainEvent::ProspectReplied(ProspectReplied {
        restaurant_id: cmd.restaurant_id,
        note: cmd.note,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

// ================================================================================================
// Catalog aggregate — catalog, category tree, products/offers (SKUs), option lists, stock.
// ================================================================================================

/// The stream a Catalog aggregate lives on.
fn catalog_stream(id: &CatalogId) -> String {
    format!("Catalog-{}", id.0)
}

/// Rehydrate the Catalog aggregate (fold + current version).
async fn load_catalog(
    store: &dyn EventStore,
    id: &CatalogId,
) -> Result<(Option<CatalogState>, i64), DomainError> {
    Repository::new(store).load::<CatalogState>(*id).await
}

/// Rehydrate and require existence, or reject with `errors.yaml#/CatalogNotFound`.
async fn require_catalog(
    store: &dyn EventStore,
    id: &CatalogId,
) -> Result<(CatalogState, i64), DomainError> {
    let (state, version) = load_catalog(store, id).await?;
    match state {
        Some(state) => Ok((state, version)),
        None => Err(reject("CatalogNotFound", json!({ "catalogId": id }))),
    }
}

/// `errors.yaml#/RefNotUnique`: every `ref` (idempotent import key) must be unique WITHIN the catalog.
/// Checks the candidate refs against the folded catalog content and against each other.
fn ensure_refs_unique(
    state: &CatalogState,
    catalog_id: &CatalogId,
    candidates: &[&ExternalReference],
) -> Result<(), DomainError> {
    let existing = state.refs_in_use();
    let mut seen: HashSet<&str> = HashSet::new();
    for r in candidates {
        if existing.contains(r.0.as_str()) || !seen.insert(r.0.as_str()) {
            return Err(reject("RefNotUnique", json!({ "ref": r, "catalogId": catalog_id })));
        }
    }
    Ok(())
}

/// `errors.yaml#/CurrencyMismatch`: every offer price must use the restaurant's default currency. The
/// currency authority is the Restaurant projection row (`default_currency`, ADR-0016); a row not yet
/// projected (read-model lag) skips the check rather than failing the write with an undeclared error.
async fn ensure_prices_use_restaurant_currency(
    restaurants: &dyn RestaurantReadRepository,
    restaurant_id: RestaurantId,
    prices: &[&Money],
) -> Result<(), DomainError> {
    let Some(row) = restaurants.by_id(restaurant_id).await? else {
        return Ok(());
    };
    for price in prices {
        if price.currency != row.default_currency {
            return Err(reject(
                "CurrencyMismatch",
                json!({ "restaurantName": row.display_name, "currency": row.default_currency }),
            ));
        }
    }
    Ok(())
}

/// Handle `commands.yaml#/CreateCatalog` → emit `events.yaml#/CatalogCreated` on the new
/// `Catalog-<id>` stream. Requires the owning restaurant to exist in the read model
/// (`RestaurantNotFound`); idempotent on replay (client-generated ids, ADR-0034).
pub async fn create_catalog(
    store: &dyn EventStore,
    restaurants: &dyn RestaurantReadRepository,
    cmd: CreateCatalog,
    actor: &Actor,
) -> Result<(), DomainError> {
    // TODO(invariant): RefNotUnique — the catalog's own ref vs the restaurant's OTHER catalogs needs
    //                  an external-reference read-model index port; within this (new, empty) catalog
    //                  there is nothing to collide with yet.
    if restaurants.by_id(cmd.restaurant_id).await?.is_none() {
        return Err(reject("RestaurantNotFound", json!({ "restaurantId": cmd.restaurant_id })));
    }
    let stream_name = catalog_stream(&cmd.catalog_id);
    let event = DomainEvent::CatalogCreated(CatalogCreated {
        catalog_id: cmd.catalog_id,
        r#ref: cmd.r#ref,
        restaurant_id: cmd.restaurant_id,
        name: cmd.name,
    });
    create_if_absent(store, &stream_name, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/ConfigureCatalogSlug` -> emit `events.yaml#/CatalogSlugConfigured`.
///
/// The catalog's ROUTE is the owner's choice, made AFTER creation -- creation never derives one
/// (a creation that invented a label would pin a public path the merchant never picked). Two
/// outcomes besides success: an unknown catalog is `CatalogNotFound`, and a label already used by
/// another catalog of the SAME restaurant is `CatalogSlugAlreadyTaken`.
///
/// Re-submitting the CURRENT label appends nothing -- decided from the fold, so an idempotent retry
/// costs no event and no read. Unlike the restaurant HOST there is no reservation and no released-label
/// alias: this is a path inside an already-resolved storefront, so no previous label must keep resolving.
pub async fn configure_catalog_slug(
    store: &dyn EventStore,
    catalogs: &dyn CatalogReadRepository,
    cmd: ConfigureCatalogSlug,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_catalog(store, &cmd.catalog_id).await?;

    // Already our current label -> nothing happened. No event, no error, no read.
    if state.slug.as_ref() == Some(&cmd.slug) {
        return Ok(());
    }

    if catalogs.slug_taken(state.restaurant_id, &cmd.slug, cmd.catalog_id).await? {
        return Err(reject(
            "CatalogSlugAlreadyTaken",
            json!({ "catalogId": cmd.catalog_id, "slug": cmd.slug }),
        ));
    }

    let stream_name = catalog_stream(&cmd.catalog_id);
    let event = DomainEvent::CatalogSlugConfigured(CatalogSlugConfigured {
        catalog_id: cmd.catalog_id,
        restaurant_id: state.restaurant_id,
        slug: cmd.slug,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/AddProduct` → emit `events.yaml#/ProductAdded`. Enforces `CatalogNotFound`,
/// `CurrencyMismatch` (offer prices vs the restaurant's default currency),
/// `CatalogCategoryRefNotFound` (the categoryRef must resolve in the folded tree) and `RefNotUnique`
/// (the product's and offers' refs must be fresh within the catalog).
pub async fn add_product(
    store: &dyn EventStore,
    restaurants: &dyn RestaurantReadRepository,
    cmd: AddProduct,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_catalog(store, &cmd.catalog_id).await?;
    let prices: Vec<&Money> = cmd.offers.iter().map(|o| &o.price).collect();
    ensure_prices_use_restaurant_currency(restaurants, state.restaurant_id, &prices).await?;
    if let Some(category_ref) = &cmd.category_ref {
        if state.category_by_ref(category_ref).is_none() {
            return Err(reject("CatalogCategoryRefNotFound", json!({ "ref": category_ref })));
        }
    }
    let candidate_refs: Vec<&ExternalReference> =
        cmd.r#ref.iter().chain(cmd.offers.iter().filter_map(|o| o.r#ref.as_ref())).collect();
    ensure_refs_unique(&state, &cmd.catalog_id, &candidate_refs)?;
    let product = Product {
        id: cmd.product_id,
        r#ref: cmd.r#ref,
        catalog_id: cmd.catalog_id,
        restaurant_id: cmd.restaurant_id,
        category_ref: cmd.category_ref,
        name: cmd.name,
        description: cmd.description,
        tags: cmd.tags,
        image_ids: vec![],
        tax_rate: cmd.tax_rate,
        offers: cmd.offers,
    };
    let stream_name = catalog_stream(&cmd.catalog_id);
    let event = DomainEvent::ProductAdded(ProductAdded {
        catalog_id: cmd.catalog_id,
        restaurant_id: cmd.restaurant_id,
        product,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/UpdateProduct` → emit `events.yaml#/ProductUpdated` (full replace,
/// including offers). Enforces `ProductNotFound`, `ProductMustHaveOffer` (a product keeps ≥ 1 offer)
/// and `CurrencyMismatch`.
pub async fn update_product(
    store: &dyn EventStore,
    restaurants: &dyn RestaurantReadRepository,
    cmd: UpdateProduct,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = load_catalog(store, &cmd.catalog_id).await?;
    let exists = state.as_ref().is_some_and(|s| s.product_by_id(cmd.product.id).is_some());
    if !exists {
        return Err(reject("ProductNotFound", json!({ "productId": cmd.product.id })));
    }
    let state = state.expect("existence checked above");
    if cmd.product.offers.is_empty() {
        return Err(reject(
            "ProductMustHaveOffer",
            json!({ "productId": cmd.product.id, "productName": cmd.product.name }),
        ));
    }
    let prices: Vec<&Money> = cmd.product.offers.iter().map(|o| &o.price).collect();
    ensure_prices_use_restaurant_currency(restaurants, state.restaurant_id, &prices).await?;
    let stream_name = catalog_stream(&cmd.catalog_id);
    let event = DomainEvent::ProductUpdated(ProductUpdated {
        catalog_id: cmd.catalog_id,
        restaurant_id: cmd.restaurant_id,
        product: cmd.product,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/RemoveProduct` → emit `events.yaml#/ProductRemoved`. `ProductNotFound`
/// covers both a missing product and a missing catalog (the only error this message declares).
pub async fn remove_product(
    store: &dyn EventStore,
    cmd: RemoveProduct,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = load_catalog(store, &cmd.catalog_id).await?;
    let exists = state.as_ref().is_some_and(|s| s.product_by_id(cmd.product_id).is_some());
    if !exists {
        return Err(reject("ProductNotFound", json!({ "productId": cmd.product_id })));
    }
    let stream_name = catalog_stream(&cmd.catalog_id);
    let event = DomainEvent::ProductRemoved(ProductRemoved {
        catalog_id: cmd.catalog_id,
        restaurant_id: cmd.restaurant_id,
        product_id: cmd.product_id,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/AddCatalogCategory` → emit `events.yaml#/CatalogCategoryAdded`. Enforces
/// `CatalogNotFound`, `ParentCatalogCategoryNotFound` (parentRef must resolve in the folded tree) and
/// `RefNotUnique` (the category's ref must be fresh within the catalog).
pub async fn add_catalog_category(
    store: &dyn EventStore,
    cmd: AddCatalogCategory,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_catalog(store, &cmd.catalog_id).await?;
    if let Some(parent_ref) = &cmd.category.parent_ref {
        if state.category_by_ref(parent_ref).is_none() {
            return Err(reject("ParentCatalogCategoryNotFound", json!({ "parentRef": parent_ref })));
        }
    }
    let candidate_refs: Vec<&ExternalReference> = cmd.category.r#ref.iter().collect();
    ensure_refs_unique(&state, &cmd.catalog_id, &candidate_refs)?;
    let stream_name = catalog_stream(&cmd.catalog_id);
    let event = DomainEvent::CatalogCategoryAdded(CatalogCategoryAdded {
        catalog_id: cmd.catalog_id,
        restaurant_id: cmd.restaurant_id,
        category: cmd.category,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/UpdateCatalogCategory` → emit `events.yaml#/CatalogCategoryUpdated` (full
/// replace). Enforces `CatalogCategoryNotFound` (also covering a missing catalog — the only not-found
/// this message declares) and `CatalogCategoryCycle` (the new parentRef must not loop the tree).
pub async fn update_catalog_category(
    store: &dyn EventStore,
    cmd: UpdateCatalogCategory,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = load_catalog(store, &cmd.catalog_id).await?;
    let exists = state.as_ref().is_some_and(|s| s.category_by_id(cmd.category.id).is_some());
    if !exists {
        return Err(reject(
            "CatalogCategoryNotFound",
            json!({ "productCategoryId": cmd.category.id }),
        ));
    }
    let state = state.expect("existence checked above");
    if state.would_create_cycle(&cmd.category) {
        return Err(reject(
            "CatalogCategoryCycle",
            json!({ "productCategoryId": cmd.category.id, "categoryName": cmd.category.name }),
        ));
    }
    let stream_name = catalog_stream(&cmd.catalog_id);
    let event = DomainEvent::CatalogCategoryUpdated(CatalogCategoryUpdated {
        catalog_id: cmd.catalog_id,
        restaurant_id: cmd.restaurant_id,
        category: cmd.category,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/RemoveCatalogCategory` → emit `events.yaml#/CatalogCategoryRemoved`.
/// Enforces `CatalogCategoryNotFound` and `CatalogCategoryNotEmpty` (no child category / product may
/// still reference it).
pub async fn remove_catalog_category(
    store: &dyn EventStore,
    cmd: RemoveCatalogCategory,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = load_catalog(store, &cmd.catalog_id).await?;
    let Some(category) =
        state.as_ref().and_then(|s| s.category_by_id(cmd.product_category_id)).cloned()
    else {
        return Err(reject(
            "CatalogCategoryNotFound",
            json!({ "productCategoryId": cmd.product_category_id }),
        ));
    };
    let state = state.expect("existence checked above");
    if state.category_has_dependents(&category) {
        return Err(reject(
            "CatalogCategoryNotEmpty",
            json!({ "productCategoryId": category.id, "categoryName": category.name }),
        ));
    }
    let stream_name = catalog_stream(&cmd.catalog_id);
    let event = DomainEvent::CatalogCategoryRemoved(CatalogCategoryRemoved {
        catalog_id: cmd.catalog_id,
        restaurant_id: cmd.restaurant_id,
        product_category_id: cmd.product_category_id,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/AddOptionList` → emit `events.yaml#/OptionListAdded`. Enforces
/// `CatalogNotFound`, `OptionListMustHaveOption` (≥ 1 option) and `InvalidSelectionBounds`
/// (minSelections must fit within maxSelections and the number of options).
pub async fn add_option_list(
    store: &dyn EventStore,
    cmd: AddOptionList,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = require_catalog(store, &cmd.catalog_id).await?;
    let ol = &cmd.option_list;
    if ol.options.is_empty() {
        return Err(reject(
            "OptionListMustHaveOption",
            json!({ "optionListId": ol.id, "optionListName": ol.name }),
        ));
    }
    let out_of_bounds = ol.min_selections < 0
        || ol.max_selections.is_some_and(|max| ol.min_selections > max)
        || ol.min_selections > ol.options.len() as i64;
    if out_of_bounds {
        return Err(reject(
            "InvalidSelectionBounds",
            json!({ "optionListId": ol.id, "optionListName": ol.name }),
        ));
    }
    let stream_name = catalog_stream(&cmd.catalog_id);
    let event = DomainEvent::OptionListAdded(OptionListAdded {
        catalog_id: cmd.catalog_id,
        restaurant_id: cmd.restaurant_id,
        option_list: cmd.option_list,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/UpdateOptionList` → emit `events.yaml#/OptionListUpdated` (full replace).
/// Enforces `OptionListNotFound` (also covering a missing catalog) and `OptionListMustHaveOption`.
pub async fn update_option_list(
    store: &dyn EventStore,
    cmd: UpdateOptionList,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = load_catalog(store, &cmd.catalog_id).await?;
    let exists = state.as_ref().is_some_and(|s| s.option_list_by_id(cmd.option_list.id).is_some());
    if !exists {
        return Err(reject("OptionListNotFound", json!({ "optionListId": cmd.option_list.id })));
    }
    if cmd.option_list.options.is_empty() {
        return Err(reject(
            "OptionListMustHaveOption",
            json!({ "optionListId": cmd.option_list.id, "optionListName": cmd.option_list.name }),
        ));
    }
    let stream_name = catalog_stream(&cmd.catalog_id);
    let event = DomainEvent::OptionListUpdated(OptionListUpdated {
        catalog_id: cmd.catalog_id,
        restaurant_id: cmd.restaurant_id,
        option_list: cmd.option_list,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/RemoveOptionList` → emit `events.yaml#/OptionListRemoved`. Enforces
/// `OptionListNotFound` and `OptionListInUse` (no offer may still reference it).
pub async fn remove_option_list(
    store: &dyn EventStore,
    cmd: RemoveOptionList,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = load_catalog(store, &cmd.catalog_id).await?;
    let Some(option_list) =
        state.as_ref().and_then(|s| s.option_list_by_id(cmd.option_list_id)).cloned()
    else {
        return Err(reject("OptionListNotFound", json!({ "optionListId": cmd.option_list_id })));
    };
    let state = state.expect("existence checked above");
    if state.option_list_in_use(cmd.option_list_id) {
        return Err(reject(
            "OptionListInUse",
            json!({ "optionListId": option_list.id, "optionListName": option_list.name }),
        ));
    }
    let stream_name = catalog_stream(&cmd.catalog_id);
    let event = DomainEvent::OptionListRemoved(OptionListRemoved {
        catalog_id: cmd.catalog_id,
        restaurant_id: cmd.restaurant_id,
        option_list_id: cmd.option_list_id,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/UpdateOfferStock` → emit `events.yaml#/OfferStockUpdated`. Enforces
/// `OfferNotFound`; the `StockStatus` is DERIVED server-side from quantity vs lowStockThreshold
/// (0 → OUT_OF_STOCK, ≤ threshold → LOW_STOCK, else IN_STOCK). The inbound HubRise inventory sync
/// records the same event WITHOUT this command (actors.yaml event reaction — the ACL appends the
/// already-derived fact; there is nothing to reject).
pub async fn update_offer_stock(
    store: &dyn EventStore,
    cmd: UpdateOfferStock,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = load_catalog(store, &cmd.catalog_id).await?;
    let exists = state.as_ref().is_some_and(|s| s.offer_by_id(cmd.offer_id).is_some());
    if !exists {
        return Err(reject("OfferNotFound", json!({ "offerId": cmd.offer_id })));
    }
    // TODO(invariant): OfferNotStockTracked — the Offer entity carries no stock-tracking flag (an
    //                  offer simply STARTS tracking on its first UpdateOfferStock, per the tests.yaml
    //                  fixture), so this rejection needs a model-level flag to be enforceable.
    let status = if cmd.quantity.0 <= 0.0 {
        StockStatus::OUT_OF_STOCK
    } else if cmd.low_stock_threshold.as_ref().is_some_and(|t| cmd.quantity.0 <= t.0) {
        StockStatus::LOW_STOCK
    } else {
        StockStatus::IN_STOCK
    };
    let stock = Stock {
        quantity: cmd.quantity,
        low_stock_threshold: cmd.low_stock_threshold,
        status,
        expires_at: cmd.expires_at,
    };
    let stream_name = catalog_stream(&cmd.catalog_id);
    let event = DomainEvent::OfferStockUpdated(OfferStockUpdated {
        catalog_id: cmd.catalog_id,
        restaurant_id: cmd.restaurant_id,
        offer_id: cmd.offer_id,
        stock,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/ImportCatalog` → emit `events.yaml#/CatalogImported` (full replace of the
/// catalog content; idempotent via entity refs). Enforces `CatalogNotFound` and `MissingRef` (every
/// imported entity must carry its ref — the idempotency key). `CatalogTranslationFailed` is raised by
/// the HubRise ACL while TRANSLATING the external payload, i.e. before this command exists — it never
/// fires here.
pub async fn import_catalog(
    store: &dyn EventStore,
    cmd: ImportCatalog,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = require_catalog(store, &cmd.catalog_id).await?;
    let missing_ref = cmd.categories.iter().any(|c| c.r#ref.is_none())
        || cmd
            .products
            .iter()
            .any(|p| p.r#ref.is_none() || p.offers.iter().any(|o| o.r#ref.is_none()))
        || cmd
            .option_lists
            .iter()
            .any(|l| l.r#ref.is_none() || l.options.iter().any(|o| o.r#ref.is_none()));
    if missing_ref {
        return Err(reject("MissingRef", json!({})));
    }
    let stream_name = catalog_stream(&cmd.catalog_id);
    let event = DomainEvent::CatalogImported(CatalogImported {
        catalog_id: cmd.catalog_id,
        restaurant_id: cmd.restaurant_id,
        source: cmd.source,
        categories: cmd.categories,
        products: cmd.products,
        option_lists: cmd.option_lists,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

// ================================================================================================
// Customer aggregate — WRAPPED Supabase Auth identity (ADR-0015) + profile/preferences/favorites.
// The request/confirm pairs stay pure: the generated `identity` service port (services.yaml,
// issue #50) is the ACL boundary doing the actual Supabase call; only verified FACTS are appended
// here. Invalid/expired verifications arrive as the canonical typed rejections RAISED BY THE
// ADAPTER (`InvalidVerificationCode` / `InvalidVerificationToken` / `VerificationCodeExpired`).
// ================================================================================================

/// The stream a Customer aggregate lives on.
fn customer_stream(id: &CustomerId) -> String {
    format!("Customer-{}", id.0)
}

/// Rehydrate the Customer aggregate (fold + current version).
async fn load_customer(
    store: &dyn EventStore,
    id: &CustomerId,
) -> Result<(Option<CustomerState>, i64), DomainError> {
    Repository::new(store).load::<CustomerState>(*id).await
}

/// Canonical E.164 from the split phone input: dialing code + national number with the trunk `0`
/// stripped (e.g. `+33` + `0612345678` → `+33612345678`), matching `scalars.yaml#/PhoneNumber`.
/// Carrier-grade validation belongs to the auth provider (it delivers the SMS), not here.
/// `pub` because identity ADAPTERS build the `InvalidVerificationCode` rejection context (`phone`)
/// from their operation input with the SAME canonicalization (issue #50).
pub fn canonical_phone(dialing_code: &DialingCode, national_number: &NationalPhoneNumber) -> PhoneNumber {
    PhoneNumber(format!("{}{}", dialing_code.0, national_number.0.trim_start_matches('0')))
}

/// Handle `commands.yaml#/RequestPhoneVerification` — a pure EFFECT (actors.yaml: emits nothing):
/// delegate the SMS OTP send to the wrapped auth provider (Supabase → Twilio, ADR-0015), localized by
/// the locale the caller provided (pre-identification, so there is no stored locale yet).
pub async fn request_phone_verification(
    _store: &dyn EventStore,
    auth: &dyn IdentityService,
    cmd: RequestPhoneVerification,
    actor: &Actor,
) -> Result<(), DomainError> {
    auth.send_phone_otp(
        IdentitySendPhoneOtpInput {
            dialing_code: cmd.dialing_code,
            national_number: cmd.national_number,
            locale: cmd.locale,
        },
        &ServiceCallMeta::new(actor.correlation_id),
    )
    .await
}

/// What [`verify_phone`] resolved — surfaced in the GraphQL `verifyPhone` payload (api.yaml).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VerifyPhoneOutcome {
    /// The AUTHORITATIVE customer id: the existing customer for a returning phone (the
    /// client-proposed id is discarded), else the newly registered one.
    pub customer_id: CustomerId,
    /// Whether a new Customer was registered (`true`) or a returning one identified (`false`).
    pub created: bool,
}

/// Handle `commands.yaml#/VerifyPhone` → register-or-identify. The OTP is verified through the
/// generated identity service port (`InvalidVerificationCode` / `VerificationCodeExpired` are the
/// adapter's typed rejections); the backend then decides
/// new-vs-returning by resolving the canonical phone in the Customer read model: a known phone emits
/// `CustomerIdentified` on the EXISTING customer's stream (the client-proposed id is discarded), a
/// new phone emits `CustomerRegistered` on the new `Customer-<id>` stream (idempotent on replay).
pub async fn verify_phone(
    store: &dyn EventStore,
    auth: &dyn IdentityService,
    customers: &dyn CustomerReadRepository,
    sessions: &dyn crate::auth_sessions::AuthSessionStore,
    cmd: VerifyPhone,
    actor: &Actor,
) -> Result<VerifyPhoneOutcome, DomainError> {
    let phone = canonical_phone(&cmd.dialing_code, &cmd.national_number);
    let verified = auth
        .verify_phone_otp(
            IdentityVerifyPhoneOtpInput {
                dialing_code: cmd.dialing_code.clone(),
                national_number: cmd.national_number.clone(),
                code: cmd.code.clone(),
            },
            &ServiceCallMeta::new(actor.correlation_id),
        )
        .await?;
    let auth_ref = verified.auth_ref.clone();

    // Resolve new-vs-returning BEFORE any session work (#437): the claim stamp needs the
    // AUTHORITATIVE customer id — the existing customer for a returning phone (the
    // client-proposed id is discarded), else the new registration's id.
    let existing = customers.by_phone(phone.clone()).await?;
    let resolved_customer_id =
        existing.as_ref().map(|row| row.customer_id).unwrap_or(cmd.customer_id);

    // STAMP → rotate → park (#437, epic #429's blocking precondition): the auth user is stamped
    // with the customer domain claim, the session is THEN rotated so the new token carries it,
    // and only that POST-ROTATION token is parked for cookie pickup (#112), keyed by the
    // acceptance messageId (actor.cause_id — the envelope→actor mapping, ADR-0041), owned by the
    // journaling anonymous session.
    //
    // Failure posture (mob verdict on #437): the OTP is a consumed external fact, so verification
    // STANDS whatever happens here — but an UNSTAMPED token is never parked, so any failure in
    // this block skips the rest of it (parked content is claim-bearing or ABSENT, never
    // claimless — the AUTH_SESSION_KEY silent-degrade shape is the named anti-pattern). Recovery
    // is a fresh OTP + idempotent re-stamp. The stamp-failure defect counter
    // (customer_claim_stamp_failed_total) is incremented by the identity ACL — this layer stays
    // telemetry-SDK-free. A claim CONFLICT is deliberately not retried: the handler returns Ok to
    // the mailbox (the verification succeeded), so there is no redelivery loop.
    match auth
        .stamp_customer_claim(
            IdentityStampCustomerClaimInput {
                auth_ref: auth_ref.clone(),
                customer_id: resolved_customer_id,
            },
            &ServiceCallMeta::new(actor.correlation_id),
        )
        .await
    {
        Err(e) => {
            tracing::error!(error = %e, "customer claim stamp failed -- verification stands; session rotation and parking SKIPPED (an unstamped token is never parked; recovery: fresh OTP, idempotent re-stamp)");
        }
        Ok(()) => {
            // The PRE-rotation tokens are never parked — they were minted before the stamp, so
            // they cannot carry the claim. No refresh token (a provider/mock without sessions)
            // means nothing to rotate, hence nothing to park.
            if let (Some(refresh_token), Some(message_id)) =
                (verified.refresh_token.clone(), actor.cause_id)
            {
                match auth
                    .refresh_session(
                        IdentityRefreshSessionInput { refresh_token },
                        &ServiceCallMeta::new(actor.correlation_id),
                    )
                    .await
                {
                    Ok(rotated) => {
                        let parked = crate::auth_sessions::ParkedAuthSession {
                            message_id,
                            session_id: Some(cmd.session_id.0),
                            access_token: rotated.access_token,
                            refresh_token: rotated.refresh_token,
                            expires_in: rotated.expires_in,
                        };
                        if let Err(e) = sessions.park(parked).await {
                            tracing::error!(%message_id, error = %e, "auth session parking failed -- verification stands, cookie pickup unavailable");
                        }
                    }
                    Err(e) => {
                        tracing::error!(%message_id, error = %e, "session rotation after claim stamp failed -- verification stands, cookie pickup unavailable (recovery: fresh OTP)");
                    }
                }
            }
        }
    }

    if let Some(existing) = existing {
        let (_state, version) = load_customer(store, &existing.customer_id).await?;
        let stream_name = customer_stream(&existing.customer_id);
        let event = DomainEvent::CustomerIdentified(CustomerIdentified {
            customer_id: existing.customer_id,
            auth_ref,
            session_id: cmd.session_id,
        });
        Repository::new(store).save(&stream_name, version, &[event], actor).await?;
        return Ok(VerifyPhoneOutcome { customer_id: existing.customer_id, created: false });
    }
    let stream_name = customer_stream(&cmd.customer_id);
    let customer_id = cmd.customer_id;
    let event = DomainEvent::CustomerRegistered(CustomerRegistered {
        mode: None,
        customer_id: cmd.customer_id,
        auth_ref: Some(auth_ref),
        phone,
        display_name: cmd.display_name,
        email: None, // email is verified-only (ConfirmEmailVerification), never set at registration
        locale: cmd.locale,
        timezone: cmd.timezone,
    });
    // `created` now reports what actually happened (ADR-20260728-011344). It used to be a hard-coded
    // `true` sitting immediately after a swallowed version conflict, so a customer who already existed
    // under this id was told they had just been created — a lie the caller had no way to detect, on a
    // live identity flow rather than a batch job.
    let created = create_if_absent(store, &stream_name, &[event], actor).await?;
    Ok(VerifyPhoneOutcome { customer_id, created: created == Created::Yes })
}

/// Handle `commands.yaml#/RequestEmailVerification` — a pure EFFECT (emits nothing): reject an email
/// already owned by ANOTHER customer (`EmailAlreadyInUse`), then delegate the magic-link send to the
/// auth provider, localized via the customer's STORED locale (ADR-0015: no per-call language param).
pub async fn request_email_verification(
    store: &dyn EventStore,
    auth: &dyn IdentityService,
    customers: &dyn CustomerReadRepository,
    cmd: RequestEmailVerification,
    actor: &Actor,
) -> Result<(), DomainError> {
    if let Some(owner) = customers.by_email(cmd.email.clone()).await? {
        if owner.customer_id != cmd.customer_id {
            return Err(reject("EmailAlreadyInUse", json!({ "email": cmd.email })));
        }
    }
    let (state, _version) = load_customer(store, &cmd.customer_id).await?;
    let locale = state.and_then(|s| s.locale);
    auth.send_email_magic_link(
        IdentitySendEmailMagicLinkInput { email: cmd.email, locale },
        &ServiceCallMeta::new(actor.correlation_id),
    )
    .await
}

/// Handle `commands.yaml#/ConfirmEmailVerification` → emit `events.yaml#/CustomerEmailVerified`. The
/// token is verified SERVER-SIDE through the generated identity service port
/// (`InvalidVerificationToken` / `VerificationCodeExpired` are the adapter's typed rejections),
/// whose output reports the email it proves — the linked email is never taken from client input.
pub async fn confirm_email_verification(
    store: &dyn EventStore,
    auth: &dyn IdentityService,
    cmd: ConfirmEmailVerification,
    actor: &Actor,
) -> Result<(), DomainError> {
    let email = auth
        .verify_email_token(
            IdentityVerifyEmailTokenInput { token: cmd.token.clone() },
            &ServiceCallMeta::new(actor.correlation_id),
        )
        .await?
        .email;
    let (_state, version) = load_customer(store, &cmd.customer_id).await?;
    let stream_name = customer_stream(&cmd.customer_id);
    let event = DomainEvent::CustomerEmailVerified(CustomerEmailVerified {
        customer_id: cmd.customer_id,
        email,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/RequestPhoneChange` — a pure EFFECT (emits nothing): reject a new phone
/// already owned by ANOTHER customer (`PhoneAlreadyInUse`), then delegate the OTP send to the new
/// phone (localized via the STORED locale).
pub async fn request_phone_change(
    store: &dyn EventStore,
    auth: &dyn IdentityService,
    customers: &dyn CustomerReadRepository,
    cmd: RequestPhoneChange,
    actor: &Actor,
) -> Result<(), DomainError> {
    let new_phone = canonical_phone(&cmd.new_dialing_code, &cmd.new_national_number);
    if let Some(owner) = customers.by_phone(new_phone.clone()).await? {
        if owner.customer_id != cmd.customer_id {
            return Err(reject("PhoneAlreadyInUse", json!({ "phone": new_phone })));
        }
    }
    let (state, _version) = load_customer(store, &cmd.customer_id).await?;
    let locale = state.and_then(|s| s.locale);
    auth.send_phone_otp(
        IdentitySendPhoneOtpInput {
            dialing_code: cmd.new_dialing_code,
            national_number: cmd.new_national_number,
            locale,
        },
        &ServiceCallMeta::new(actor.correlation_id),
    )
    .await
}

/// Handle `commands.yaml#/ConfirmPhoneChange` → emit `events.yaml#/CustomerPhoneChanged` (canonical
/// E.164). The OTP on the NEW phone is verified through the generated identity service port
/// (`InvalidVerificationCode` / `VerificationCodeExpired` are the adapter's typed rejections) and
/// uniqueness is re-checked at confirm time (`PhoneAlreadyInUse`).
pub async fn confirm_phone_change(
    store: &dyn EventStore,
    auth: &dyn IdentityService,
    customers: &dyn CustomerReadRepository,
    cmd: ConfirmPhoneChange,
    actor: &Actor,
) -> Result<(), DomainError> {
    let new_phone = canonical_phone(&cmd.new_dialing_code, &cmd.new_national_number);
    auth.verify_phone_otp(
        IdentityVerifyPhoneOtpInput {
            dialing_code: cmd.new_dialing_code.clone(),
            national_number: cmd.new_national_number.clone(),
            code: cmd.code.clone(),
        },
        &ServiceCallMeta::new(actor.correlation_id),
    )
    .await?;
    if let Some(owner) = customers.by_phone(new_phone.clone()).await? {
        if owner.customer_id != cmd.customer_id {
            return Err(reject("PhoneAlreadyInUse", json!({ "phone": new_phone })));
        }
    }
    let (_state, version) = load_customer(store, &cmd.customer_id).await?;
    let stream_name = customer_stream(&cmd.customer_id);
    let event = DomainEvent::CustomerPhoneChanged(CustomerPhoneChanged {
        customer_id: cmd.customer_id,
        phone: new_phone,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/ChangeLanguage` → emit `events.yaml#/CustomerLanguageChanged` (the single
/// locale setter; later authenticated SMS/email sends use the stored locale). Declares no throws.
pub async fn change_language(
    store: &dyn EventStore,
    cmd: ChangeLanguage,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = load_customer(store, &cmd.customer_id).await?;
    let stream_name = customer_stream(&cmd.customer_id);
    let event = DomainEvent::CustomerLanguageChanged(CustomerLanguageChanged {
        customer_id: cmd.customer_id,
        locale: cmd.locale,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/MarkRestaurantAsFavorite` → emit `events.yaml#/RestaurantFavorited`. The
/// favorited restaurant must exist in the read model (`RestaurantNotFound`).
pub async fn mark_restaurant_as_favorite(
    store: &dyn EventStore,
    restaurants: &dyn RestaurantReadRepository,
    cmd: MarkRestaurantAsFavorite,
    actor: &Actor,
) -> Result<(), DomainError> {
    if restaurants.by_id(cmd.restaurant_id).await?.is_none() {
        return Err(reject("RestaurantNotFound", json!({ "restaurantId": cmd.restaurant_id })));
    }
    let (_state, version) = load_customer(store, &cmd.customer_id).await?;
    let stream_name = customer_stream(&cmd.customer_id);
    let event = DomainEvent::RestaurantFavorited(RestaurantFavorited {
        customer_id: cmd.customer_id,
        restaurant_id: cmd.restaurant_id,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/UnmarkRestaurantAsFavorite` → emit `events.yaml#/RestaurantUnfavorited`.
/// Idempotent per actors.yaml: unfavoriting a restaurant that is not a favorite is a no-op (no event,
/// no error).
pub async fn unmark_restaurant_as_favorite(
    store: &dyn EventStore,
    cmd: UnmarkRestaurantAsFavorite,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = load_customer(store, &cmd.customer_id).await?;
    let is_favorite = state.is_some_and(|s| s.favorites.contains(&cmd.restaurant_id));
    if !is_favorite {
        return Ok(());
    }
    let stream_name = customer_stream(&cmd.customer_id);
    let event = DomainEvent::RestaurantUnfavorited(RestaurantUnfavorited {
        customer_id: cmd.customer_id,
        restaurant_id: cmd.restaurant_id,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/UpdateCustomerInfo` → emit `events.yaml#/CustomerInfoUpdated`. An update
/// carrying nothing editable is rejected (`errors.yaml#/NoEditableFieldProvided`; displayName is the
/// only editable field — email is verified-only).
pub async fn update_customer_info(
    store: &dyn EventStore,
    cmd: UpdateCustomerInfo,
    actor: &Actor,
) -> Result<(), DomainError> {
    if cmd.display_name.is_none() {
        return Err(reject("NoEditableFieldProvided", json!({})));
    }
    let (_state, version) = load_customer(store, &cmd.customer_id).await?;
    let stream_name = customer_stream(&cmd.customer_id);
    let event = DomainEvent::CustomerInfoUpdated(CustomerInfoUpdated {
        customer_id: cmd.customer_id,
        display_name: cmd.display_name,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/SetCustomerPreferences` → emit `events.yaml#/CustomerPreferencesSet`
/// (discovery + i18n preferences; language is ChangeLanguage). Declares no throws.
pub async fn set_customer_preferences(
    store: &dyn EventStore,
    cmd: SetCustomerPreferences,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = load_customer(store, &cmd.customer_id).await?;
    let stream_name = customer_stream(&cmd.customer_id);
    let event = DomainEvent::CustomerPreferencesSet(CustomerPreferencesSet {
        customer_id: cmd.customer_id,
        timezone: cmd.timezone,
        dietary_tags: cmd.dietary_tags,
        favorite_cuisines: cmd.favorite_cuisines,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/SetCustomerAddress` → emit `events.yaml#/CustomerAddressSet` (add-or-update
/// by addressId, replace semantics). Declares no throws.
pub async fn set_customer_address(
    store: &dyn EventStore,
    cmd: SetCustomerAddress,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = load_customer(store, &cmd.customer_id).await?;
    let stream_name = customer_stream(&cmd.customer_id);
    let event = DomainEvent::CustomerAddressSet(CustomerAddressSet {
        customer_id: cmd.customer_id,
        address_id: cmd.address_id,
        label: cmd.label,
        address: cmd.address,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/RemoveCustomerAddress` → emit `events.yaml#/CustomerAddressRemoved`.
/// Idempotent per actors.yaml: removing an unknown address is a no-op (no event, no error).
pub async fn remove_customer_address(
    store: &dyn EventStore,
    cmd: RemoveCustomerAddress,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = load_customer(store, &cmd.customer_id).await?;
    let is_saved = state.is_some_and(|s| s.addresses.contains(&cmd.address_id));
    if !is_saved {
        return Ok(());
    }
    let stream_name = customer_stream(&cmd.customer_id);
    let event = DomainEvent::CustomerAddressRemoved(CustomerAddressRemoved {
        customer_id: cmd.customer_id,
        address_id: cmd.address_id,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/SetCustomerPaymentMethod` → emit `events.yaml#/CustomerPaymentMethodSet`
/// (the preferred Stripe payment method reference; Stripe owns the instrument). Declares no throws.
pub async fn set_customer_payment_method(
    store: &dyn EventStore,
    cmd: SetCustomerPaymentMethod,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (_state, version) = load_customer(store, &cmd.customer_id).await?;
    let stream_name = customer_stream(&cmd.customer_id);
    let event = DomainEvent::CustomerPaymentMethodSet(CustomerPaymentMethodSet {
        customer_id: cmd.customer_id,
        payment_method_id: cmd.payment_method_id,
    });
    Repository::new(store).save(&stream_name, version, &[event], actor).await.map(|_| ())
}

// ================================================================================================
// Tests — customer store-credit at checkout (#158, Part B of #207): the exactly-once, currency-matched,
// never-negative money math (`credit_to_apply`) and the end-to-end place_order spend (PaymentIntent
// reduced by the applied credit + CustomerCreditConsumed emitted, keyed by orderId).
// ================================================================================================
#[cfg(test)]
mod credit_checkout_tests {
    use super::{credit_to_apply, place_order};
    use crate::behaviour_support::{actor, eur, TestBed};
    use crate::process_managers::test_support::MemStore;
    use crate::queries::OfferView;
    use domain::generated::commands::PlaceOrder;
    use domain::generated::entities::{Address, CartLineItem as Line, CustomerContact, Money};
    use domain::generated::events::{
        CartLineAdded, CartStarted, CustomerCreditConsumed as ConsumedEv,
        CustomerCreditGranted as GrantedEv, DomainEvent, RestaurantActivated, RestaurantRegistered,
    };
    use domain::generated::scalars::{
        AddressLine, CartId, CartLineId, CatalogItemAvailability, CityName, CountryCode, CurrencyCode,
        CustomerDisplayName, CustomerId, MoneyCents, OfferId, OfferName, OrderId, PhoneNumber,
        PostalCode, ProductId, ProductName, ReclamationId, RestaurantDisplayName, RestaurantId,
        RestaurantListingStatus, ServiceType, SessionId, Slug, StockStatus,
    };

    fn cid() -> CustomerId {
        CustomerId(uuid::Uuid::from_u128(0xC1))
    }
    fn credit_stream() -> String {
        format!("CustomerCredit-{}", cid().0)
    }
    fn grant(cents: i64, recl: u128) -> DomainEvent {
        DomainEvent::CustomerCreditGranted(GrantedEv {
            customer_id: cid(),
            amount: eur(cents),
            reclamation_id: ReclamationId(uuid::Uuid::from_u128(recl)),
        })
    }
    fn consumed(cents: i64, ord: u128) -> DomainEvent {
        DomainEvent::CustomerCreditConsumed(ConsumedEv {
            customer_id: cid(),
            amount: eur(cents),
            order_id: OrderId(uuid::Uuid::from_u128(ord)),
        })
    }
    fn order(n: u128) -> OrderId {
        OrderId(uuid::Uuid::from_u128(n))
    }

    /// Applies min(balance, order total): a balance below the total applies in full…
    #[tokio::test]
    async fn applies_balance_up_to_the_order_total() {
        let store = MemStore::default();
        store.seed(&credit_stream(), vec![grant(300, 1)]);
        let applied = credit_to_apply(&store, cid(), &order(9), &eur(1000)).await.unwrap();
        assert_eq!(applied, 300);
    }

    /// …and a balance above the total is capped at the total (buyer pays 0, never negative).
    #[tokio::test]
    async fn caps_applied_credit_at_the_order_total() {
        let store = MemStore::default();
        store.seed(&credit_stream(), vec![grant(1500, 1)]);
        let applied = credit_to_apply(&store, cid(), &order(9), &eur(1000)).await.unwrap();
        assert_eq!(applied, 1000);
    }

    /// A currency mismatch never applies credit (no conversion).
    #[tokio::test]
    async fn currency_mismatch_applies_nothing() {
        let store = MemStore::default();
        store.seed(&credit_stream(), vec![grant(300, 1)]); // ledger EUR
        let usd = Money { amount_cents: MoneyCents(1000), currency: CurrencyCode("USD".into()) };
        let applied = credit_to_apply(&store, cid(), &order(9), &usd).await.unwrap();
        assert_eq!(applied, 0);
    }

    /// A customer with no ledger applies nothing. (The guest case is gone: PlaceOrder.customerId
    /// is REQUIRED as of #144, so an unidentified checkout is structurally unrepresentable.)
    #[tokio::test]
    async fn no_ledger_applies_nothing() {
        let store = MemStore::default();
        assert_eq!(credit_to_apply(&store, cid(), &order(9), &eur(1000)).await.unwrap(), 0);
    }

    /// RETRY-STABLE: once an order was debited, the SAME applied amount is reused — never recomputed
    /// against the now-lower balance (so a placeOrder retry reduces the intent identically).
    #[tokio::test]
    async fn already_consumed_order_reuses_the_exact_amount() {
        let store = MemStore::default();
        store.seed(&credit_stream(), vec![grant(1000, 1), consumed(300, 9)]);
        // Balance is now 700, but order 9 already applied 300 — reuse 300, not min(700, total).
        let applied = credit_to_apply(&store, cid(), &order(9), &eur(1000)).await.unwrap();
        assert_eq!(applied, 300);
    }

    // --- End-to-end place_order spend --------------------------------------------------------------

    fn resto() -> RestaurantId {
        RestaurantId(uuid::Uuid::from_u128(0x1001))
    }
    fn cart() -> CartId {
        CartId(uuid::Uuid::from_u128(0x2002))
    }
    fn offer() -> OfferId {
        OfferId(uuid::Uuid::from_u128(0x3003))
    }
    fn address() -> Address {
        Address {
            line1: AddressLine("9 Rue Colbert".into()),
            line2: None,
            postal_code: PostalCode("37000".into()),
            city: CityName("Tours".into()),
            country: CountryCode("FR".into()),
        }
    }

    /// place_order with store credit reduces the Stripe PaymentIntent by the applied credit and emits
    /// CustomerCreditConsumed for it (keyed by orderId), while the frozen checkout keeps the GROSS order
    /// value. Offer 1000, credit 300 → buyer pays 700, 300 consumed.
    #[tokio::test]
    async fn place_order_spends_credit_and_reduces_the_payment_intent() {
        let bed = TestBed::new();
        bed.seed(
            &format!("Restaurant-{}", resto().0),
            vec![
                DomainEvent::RestaurantRegistered(RestaurantRegistered {
                    mode: None,
                    restaurant_id: resto(),
                    account_id: None,
                    listing_status: RestaurantListingStatus::ACTIVE_PARTNER,
                    r#ref: None,
                    external_identifiers: Vec::new(),
                    display_name: RestaurantDisplayName("Resto".into()),
                    contact: None,
                    website: None,
                    tags: Vec::new(),
                    margin_rate: None,
                    cuisine_category: None,
                    uber_prices_opt_in: None,
                    address: address(),
                    location: None,
                    timezone: None,
                    preparation_time_minutes: None,
                    opening_hours: Vec::new(),
                }),
                DomainEvent::RestaurantActivated(RestaurantActivated {
                    restaurant_id: resto(),
                    reason: None,
                }),
            ],
        )
        .await;
        // Live catalog offer (priced by checkout via the read port).
        bed.catalogs.add_offer(
            resto(),
            OfferView {
                offer_id: offer(),
                product_id: ProductId(uuid::Uuid::from_u128(0x4004)),
                product_name: ProductName("Burger".into()),
                offer_name: OfferName("Solo".into()),
                price: eur(1000),
                availability: CatalogItemAvailability::AVAILABLE,
                stock_status: StockStatus::IN_STOCK,
                stock_quantity: None,
                option_lists: Vec::new(),
            },
        );
        // OPEN cart with one line for that offer.
        bed.seed(
            &format!("Cart-{}", cart().0),
            vec![
                DomainEvent::CartStarted(CartStarted {
                    cart_id: cart(),
                    restaurant_id: resto(),
                    session_id: SessionId(uuid::Uuid::from_u128(0x5005)),
                    customer_id: None,
                }),
                DomainEvent::CartLineAdded(CartLineAdded {
                    cart_id: cart(),
                    line: Line {
                        cart_line_id: CartLineId(uuid::Uuid::from_u128(0x6006)),
                        offer_id: offer(),
                        quantity: 1,
                        selected_option_ids: Vec::new(),
                    },
                }),
            ],
        )
        .await;
        // The customer's store-credit ledger: 300 available.
        bed.seed(&credit_stream(), vec![grant(300, 1)]).await;

        let cmd = PlaceOrder {
            mode: None,
            order_id: order(0x7007),
            restaurant_id: resto(),
            cart_id: cart(),
            customer_id: cid(),
            customer_contact: CustomerContact {
                display_name: CustomerDisplayName("Jo".into()),
                email: None,
                phone: PhoneNumber("+33612345678".into()),
            },
            service_type: ServiceType::DELIVERY,
            delivery_address: Some(address()),
            note: None,
            payment_method_id: "pm_123".into(),
            expected_total: None,
        };
        // The emitter's fixed default instant (tests.yaml header) — this restaurant declares no
        // hours, so the verdict is HOURS_UNDECLARED at any instant and the checkout is accepted.
        let when_at: chrono::DateTime<chrono::Utc> =
            "2026-01-06T12:00:00Z".parse().expect("RFC3339 instant");
        place_order(&bed.store, &bed.catalogs, &bed.payments, &bed.payment_pm, cmd, None, &actor(), when_at, false)
            .await
            .expect("checkout accepted");

        // The PaymentIntent charges the BUYER TOTAL (1000 − 300 = 700); the frozen checkout keeps gross.
        let payment = bed.store.stream("Payment-pi_123");
        let intent = payment
            .iter()
            .find_map(|e| match e {
                DomainEvent::PaymentIntentCreated(p) => Some(p.clone()),
                _ => None,
            })
            .expect("PaymentIntentCreated");
        assert_eq!(intent.amount, eur(700), "PaymentIntent = gross − applied credit");
        assert_eq!(intent.checkout.total_amount, eur(1000), "frozen checkout keeps the gross order value");

        // The credit is spent exactly once, keyed by the order.
        let ledger = bed.store.stream(&credit_stream());
        let debit = ledger
            .iter()
            .find_map(|e| match e {
                DomainEvent::CustomerCreditConsumed(c) => Some(c.clone()),
                _ => None,
            })
            .expect("CustomerCreditConsumed appended");
        assert_eq!(debit.amount, eur(300));
        assert_eq!(debit.order_id, order(0x7007));
    }
}

/// Record an inbound REGISTRY registration (ADR-20260728-011344 D4) — the SIRENE write path.
///
/// The ACL stages `RestaurantRegistered` **unconditionally**: it never decides whether this is a
/// creation or a change, because that is a domain question and an adapter is the wrong place to answer
/// it. This function is where the aggregate answers, by folding its own stream:
///
/// * no stream → record the registration as reported (`Recorded`);
/// * stream exists and the report moves something → emit `RestaurantUpdated` with exactly those fields
///   (`Updated`) — this is the path that was MISSING, which is why INSEE renames were silently dropped;
/// * stream exists and nothing moved → append nothing (`NoChange` → the delivery is IGNORED).
///
/// What it deliberately does NOT do: attempt an append and read a unique-constraint violation as
/// "already exists". That was the old mechanism, and it made every no-op write a heap tuple plus index
/// entries before aborting — ~200k dead tuples in `domain_events` per weekly sweep, for an outcome that
/// is by definition no change. Here the decision precedes any write.
pub async fn record_inbound_restaurant_registration(
    store: &dyn EventStore,
    event: DomainEvent,
    actor: &Actor,
) -> Result<crate::payments::RecordOutcome, DomainError> {
    use crate::payments::RecordOutcome;

    let DomainEvent::RestaurantRegistered(reported) = &event else {
        return Err(DomainError::Repository(format!(
            "record_inbound_restaurant_registration routed a non-registration event: {event:?}"
        )));
    };
    let stream_name = restaurant_stream(&reported.restaurant_id);
    let (events, version) = store.load(&stream_name).await?;

    let Some(state) = domain::restaurant::fold(&events) else {
        // Never registered: the registry is telling us about a restaurant we do not hold. Record it.
        Repository::new(store).save(&stream_name, version, &[event], actor).await?;
        return Ok(RecordOutcome::Recorded);
    };

    match domain::restaurant::changes_from_registry(&state, reported) {
        None => Ok(RecordOutcome::NoChange),
        Some(update) => {
            Repository::new(store)
                .save(&stream_name, version, &[DomainEvent::RestaurantUpdated(update)], actor)
                .await?;
            Ok(RecordOutcome::Updated)
        }
    }
}

#[cfg(test)]
mod create_if_absent_tests {
    use super::*;
    use crate::process_managers::test_support::MemStore;
    use domain::generated::events::CustomerRegistered;
    use domain::generated::scalars::{CustomerId, PhoneNumber};

    fn actor() -> Actor {
        Actor {
            user_id: uuid::Uuid::nil(),
            user_type: "PUBLIC".to_string(),
        domain_id: None,
            correlation_id: uuid::Uuid::nil(),
            cause_id: None,
        }
    }

    fn birth() -> DomainEvent {
        DomainEvent::CustomerRegistered(CustomerRegistered {
            mode: None,
            customer_id: CustomerId(uuid::Uuid::nil()),
            auth_ref: None,
            phone: PhoneNumber("+33600000000".into()),
            display_name: None,
            email: None,
            locale: None,
            timezone: None,
        })
    }

    /// The distinction the old `idempotent_on_existing` destroyed: a caller can now tell a real
    /// creation from a no-op. `verify_phone` reported `created: true` for existing customers precisely
    /// because this answer did not exist.
    #[tokio::test]
    async fn reports_whether_it_actually_created() {
        let store = MemStore::default();
        assert_eq!(
            create_if_absent(&store, "Customer-1", &[birth()], &actor()).await.unwrap(),
            Created::Yes
        );
        assert_eq!(
            create_if_absent(&store, "Customer-1", &[birth()], &actor()).await.unwrap(),
            Created::No,
            "a second call must not claim to have created anything"
        );
    }

    /// And it must not WRITE to find that out. The old path attempted the append and read the
    /// constraint violation — Postgres wrote the heap tuple before rejecting it, which is what left
    /// ~200k dead tuples in `domain_events` per SIRENE sweep.
    #[tokio::test]
    async fn a_no_op_appends_nothing() {
        let store = MemStore::default();
        create_if_absent(&store, "Customer-1", &[birth()], &actor()).await.unwrap();
        create_if_absent(&store, "Customer-1", &[birth()], &actor()).await.unwrap();
        let (events, version) = store.load("Customer-1").await.unwrap();
        assert_eq!(events.len(), 1, "the birth event, exactly once");
        assert_eq!(version, 1);
    }
}

#[cfg(test)]
mod verify_phone_claim_stamp_tests {
    use super::*;
    use crate::behaviour_support::{actor_as, TestBed};
    use base64::Engine as _;
    use domain::generated::scalars::{OtpCode, SessionId};

    /// #429's blocking precondition, pinned as BEHAVIOUR (#437): the token parked for cookie
    /// pickup must CARRY the resolved customer's domain claim. The stateful `FakeIdentity` mints
    /// the rotated JWT from whatever was stamped BEFORE rotation, so this one decode-assertion
    /// pins the whole ordering — park-before-stamp (today's bug) parks the pre-rotation
    /// `fake.access.jwt`, which is not a decodable claim-bearing JWT, and stamp-after-refresh
    /// would mint a claimless payload. No call-log assertions.
    ///
    /// `actor.cause_id = Some(mid)` is the point the generated support never reaches: it
    /// dispatches with `None`, which is why the parking branch was untested until now.
    #[tokio::test]
    async fn parked_token_carries_the_stamped_customer_claim() {
        let bed = TestBed::new();
        let mid = uuid::Uuid::now_v7();
        let mut actor = actor_as("PUBLIC");
        actor.cause_id = Some(mid);
        let cmd = VerifyPhone {
            customer_id: CustomerId(uuid::Uuid::from_u128(0x437)),
            dialing_code: DialingCode("+33".into()),
            national_number: NationalPhoneNumber("0612345678".into()),
            code: OtpCode("123456".into()),
            session_id: SessionId(uuid::Uuid::from_u128(0x5E55)),
            display_name: None,
            locale: None,
            timezone: None,
        };

        let outcome =
            verify_phone(&bed.store, &bed.identity, &bed.customers, &bed.auth_sessions, cmd, &actor)
                .await
                .expect("verification succeeds");

        let parked = bed.auth_sessions.parked();
        let entry = parked
            .iter()
            .find(|p| p.message_id == mid)
            .expect("a session parked under the acceptance messageId");
        let payload_b64 =
            entry.access_token.split('.').nth(1).expect("parked token is JWT-shaped");
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload_b64)
            .expect("parked JWT payload base64url-decodes");
        let payload: serde_json::Value =
            serde_json::from_slice(&bytes).expect("parked JWT payload is JSON");
        assert_eq!(
            payload["app_metadata"]["captain_food"]["customer_id"],
            serde_json::json!(outcome.customer_id.0.to_string()),
            "the parked token must carry the resolved customer's claim \
             (stamp BEFORE rotate; park ONLY the rotated token)"
        );
    }
}
