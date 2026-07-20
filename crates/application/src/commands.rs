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
    CreateCatalog, DeactivateRestaurant, DeleteRestaurantAccount, ImportCatalog, MarkProspectCold,
    MarkRestaurantAsFavorite, MarkRestaurantClosed, OptOutRestaurantListing, RecordProspectContact,
    RecordProspectReply, RegisterRestaurant, RegisterRestaurantAccount, RemoveCatalogCategory,
    RemoveCustomerAddress, RemoveOptionList, RemoveProduct, RemoveRestaurant,
    RequestEmailVerification, RequestPhoneChange, RequestPhoneVerification, SetCustomerAddress,
    SetCustomerPaymentMethod, SetCustomerPreferences, UnmarkRestaurantAsFavorite,
    UpdateCatalogCategory, UpdateCustomerInfo, UpdateOfferStock, UpdateOptionList, UpdateProduct,
    UpdateRestaurant, UpdateRestaurantAccount, UpdateRestaurantGoogleBusinessProfile, VerifyPhone,
    VerifyGoogleBusinessProfileOrderLink,
};
use domain::generated::entities::{CheckoutSnapshot, Money, Product, Stock};
use domain::generated::events::{
    CatalogCategoryAdded, CatalogCategoryRemoved, CatalogCategoryUpdated, CatalogCreated,
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
    RestaurantMarkedClosed, RestaurantRegistered, RestaurantRemoved, RestaurantUnfavorited,
    RestaurantUpdated,
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
    is_version_conflict, Actor, AuthProviderGateway, EmailTokenCheck, EventStore, GbpOrderLinkProbe,
    GoogleOwnershipVerifier, PhoneOtpCheck,
};
use crate::queries::{
    CustomerReadRepository, ProspectFilter, ProspectionReadRepository, RestaurantReadRepository,
};

// --- Cart / Order / DeliveryJob / PlaceOrderProcess (checkout→order→delivery flow, ADR-0046 round 2) ---
use domain::cart::{CartState, MAX_LINE_QUANTITY};
use domain::delivery_job::DeliveryJobState;
use domain::generated::commands::{
    AcceptDelivery, AcceptOrder, AddCartLine, AssignDeliveryToPartner, BindCartToCustomer,
    CancelDelivery, CancelOrderByCustomer, CancelOrderByRestaurant, ChangeCartLineQuantity,
    ChangeRiderStatus, CompleteDelivery, ConfirmPickup, DeclineDelivery, MarkOrderDelivered,
    MarkOrderReady, PlaceOrder, RateOrder, RateRestaurant, RegisterRider, RejectOrder,
    RemoveCartLine, ReportDeliveryIssue, RequestRefund, ResolveDeliveryIssue, StartPreparation,
    TipOrder, UnassignDeliveryFromPartner, UpdateDeliveryPartnerStatus, UpdateDeliveryStatus,
    UpdateRiderInfo,
};
use domain::generated::entities::CartLineItem;
use domain::generated::events::{
    CartBoundToCustomer, CartLineAdded, CartLineQuantityChanged, CartLineRemoved, CartStarted,
    DeliveryAcceptedByRider, DeliveryAssignedToPartner, DeliveryCancelled, DeliveryCompleted,
    DeliveryDeclinedByRider, DeliveryIssueReported, DeliveryIssueResolved,
    DeliveryPartnerStatusUpdated, DeliveryPickedUp, DeliveryStatusUpdated,
    DeliveryUnassignedFromPartner, OrderAcceptedByRestaurant, OrderCancelledByCustomer,
    OrderCancelledByRestaurant, OrderDelivered, OrderMarkedReady, OrderPreparationStarted,
    OrderRated, OrderRejectedByRestaurant, OrderTipped, PaymentIntentCreated, RefundRequested,
    RestaurantRated as RestaurantRatedEvent, RiderInfoUpdated, RiderRegistered, RiderStatusChanged,
};
use domain::generated::scalars::{
    CartId, CartStatus, CatalogItemAvailability, DeliveryJobId, DeliveryStatus, Mode,
    OrderAcceptanceMode, OrderId, OrderStatus, OptionId, PaymentProcessStatus, PaymentStatus,
    RiderId, RiderStatus, ServiceType, TipRecipient, Tipper,
};
use domain::order::OrderState;
use domain::rider::RiderState;

use crate::pm_state::{PaymentProcessRow, PaymentProcessStateStore};
use crate::ports::{CreatedPaymentIntent, PaymentGateway};
use crate::queries::{CatalogReadRepository, OfferView};
use crate::repository::Repository;

/// Absorb the optimistic-concurrency clash of a CREATION command (expected_version = 0) as success:
/// the aggregate already exists under this client-generated id, so re-running the command is a no-op.
fn idempotent_on_existing(result: Result<i64, DomainError>) -> Result<(), DomainError> {
    match result {
        Ok(_) => Ok(()),
        Err(e) if is_version_conflict(&e) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Build the canonical rejection for an `errors.yaml` invariant: the stable PascalCase CODE plus its
/// typed context as a JSON object (keys = the error's errors.yaml `context` fields, camelCase).
/// [`rejection_code`] is the matching reader; the GraphQL layer maps the rejection onto
/// `extensions.code` + the interpolated localized message (P-10).
fn reject(code: &str, context: serde_json::Value) -> DomainError {
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
    idempotent_on_existing(Repository::new(store).save(&stream_name, 0, &[event], actor).await)
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
/// `restaurants` backs the `SlugAlreadyTaken` uniqueness check (the Restaurant projection is the only
/// slug index we have). A row already owning the slug under the SAME restaurant id is the idempotent
/// replay of this very registration and is not a conflict.
pub async fn register_restaurant(
    store: &dyn EventStore,
    restaurants: &dyn RestaurantReadRepository,
    cmd: RegisterRestaurant,
    actor: &Actor,
) -> Result<(), DomainError> {
    // TODO(invariant): RestaurantAccountNotFound — when cmd.account_id is set, reject if the owning
    //                  RestaurantAccount does not exist (needs an account read-model lookup port).
    // TODO(invariant): RefAlreadyUsed — reject when cmd.ref is already owned by another aggregate
    //                  (needs an external-reference read-model lookup port).
    if let Some(existing) = restaurants.by_slug(cmd.slug.clone()).await? {
        if existing.restaurant_id != cmd.restaurant_id {
            return Err(reject("SlugAlreadyTaken", json!({ "slug": cmd.slug })));
        }
    }
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = DomainEvent::RestaurantRegistered(RestaurantRegistered {
        mode: cmd.mode,
        restaurant_id: cmd.restaurant_id,
        account_id: cmd.account_id,
        listing_status: cmd.listing_status.unwrap_or(RestaurantListingStatus::NON_PARTNER),
        r#ref: cmd.r#ref,
        external_identifiers: cmd.external_identifiers,
        slug: cmd.slug,
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
    idempotent_on_existing(Repository::new(store).save(&stream_name, 0, &[event], actor).await)
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
fn order_stream(id: &OrderId) -> String {
    format!("Order-{}", id.0)
}

/// Rehydrate the Order aggregate and require existence UNDER the commanding restaurant: a missing
/// stream — or an order belonging to another restaurant (tenant scoping) — rejects with
/// `errors.yaml#/OrderNotFound`.
async fn require_order(
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
fn invalid_order_status(order_id: &OrderId, status: OrderStatus) -> DomainError {
    reject("InvalidOrderStatus", json!({ "orderId": order_id, "currentStatus": status }))
}

/// Guard an Order lifecycle move with the GENERATED transition table
/// (`domain::order::lifecycle::transition`, from `specs/actors.yaml#/Order/lifecycle`,
/// ADR-20260720-004419): a move the declared machine does not contain rejects with
/// `errors.yaml#/InvalidOrderStatus` (rules.yaml#/OrderLifecycleIsExplicit).
fn require_order_transition(
    order_id: &OrderId,
    status: OrderStatus,
    event: &DomainEvent,
) -> Result<(), DomainError> {
    match domain::order::lifecycle::transition(status, event) {
        Some(_) => Ok(()),
        None => Err(invalid_order_status(order_id, status)),
    }
}

/// Handle `commands.yaml#/AcceptOrder` → emit `events.yaml#/OrderAcceptedByRestaurant`. Only a PLACED
/// order can be accepted (rules.yaml#/OrderLifecycleStatusMachine).
pub async fn accept_order(
    store: &dyn EventStore,
    cmd: AcceptOrder,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_order(store, &cmd.order_id, &cmd.restaurant_id).await?;
    let event = DomainEvent::OrderAcceptedByRestaurant(OrderAcceptedByRestaurant {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
        estimated_ready_at: cmd.estimated_ready_at,
    });
    require_order_transition(&cmd.order_id, state.status, &event)?;
    Repository::new(store).save(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/StartPreparation` → emit `events.yaml#/OrderPreparationStarted`. Only an
/// ACCEPTED order moves to PREPARING (rules.yaml#/OrderLifecycleStatusMachine).
pub async fn start_preparation(
    store: &dyn EventStore,
    cmd: StartPreparation,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_order(store, &cmd.order_id, &cmd.restaurant_id).await?;
    let event = DomainEvent::OrderPreparationStarted(OrderPreparationStarted {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
    });
    require_order_transition(&cmd.order_id, state.status, &event)?;
    Repository::new(store).save(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/MarkOrderReady` → emit `events.yaml#/OrderMarkedReady`. Allowed from
/// ACCEPTED or PREPARING — a restaurant may skip the explicit preparation step
/// (rules.yaml#/OrderLifecycleStatusMachine).
pub async fn mark_order_ready(
    store: &dyn EventStore,
    cmd: MarkOrderReady,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_order(store, &cmd.order_id, &cmd.restaurant_id).await?;
    let event = DomainEvent::OrderMarkedReady(OrderMarkedReady {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
    });
    require_order_transition(&cmd.order_id, state.status, &event)?;
    Repository::new(store).save(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/MarkOrderDelivered` → emit `events.yaml#/OrderDelivered`. Allowed from READY
/// (hand-over/collection) per the declared machine (rules.yaml#/OrderLifecycleStatusMachine;
/// OUT_FOR_DELIVERY is a read-side presentation status, unreachable in the write-side fold).
pub async fn mark_order_delivered(
    store: &dyn EventStore,
    cmd: MarkOrderDelivered,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_order(store, &cmd.order_id, &cmd.restaurant_id).await?;
    let event = DomainEvent::OrderDelivered(OrderDelivered {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
    });
    require_order_transition(&cmd.order_id, state.status, &event)?;
    Repository::new(store).save(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/RejectOrder` → emit `events.yaml#/OrderRejectedByRestaurant`. Only a PLACED
/// (not-yet-accepted) order can be rejected; the refund is driven by RefundProcess reacting to the
/// emitted fact (rules.yaml#/OrderLifecycleStatusMachine, #/RefundOnRejectionOrCancellation).
pub async fn reject_order(
    store: &dyn EventStore,
    cmd: RejectOrder,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_order(store, &cmd.order_id, &cmd.restaurant_id).await?;
    let event = DomainEvent::OrderRejectedByRestaurant(OrderRejectedByRestaurant {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
        reason: cmd.reason,
    });
    require_order_transition(&cmd.order_id, state.status, &event)?;
    Repository::new(store).save(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/CancelOrderByCustomer` → emit `events.yaml#/OrderCancelledByCustomer`. Only
/// BEFORE the restaurant accepted (status PLACED); the refund is RefundProcess's reaction
/// (rules.yaml#/OrderLifecycleStatusMachine).
pub async fn cancel_order_by_customer(
    store: &dyn EventStore,
    cmd: CancelOrderByCustomer,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_order(store, &cmd.order_id, &cmd.restaurant_id).await?;
    let event = DomainEvent::OrderCancelledByCustomer(OrderCancelledByCustomer {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
        reason: cmd.reason,
    });
    require_order_transition(&cmd.order_id, state.status, &event)?;
    Repository::new(store).save(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/CancelOrderByRestaurant` → emit `events.yaml#/OrderCancelledByRestaurant`.
/// Only an order the restaurant had already taken on (ACCEPTED/PREPARING/READY) and not yet delivered;
/// the refund is RefundProcess's reaction (rules.yaml#/OrderLifecycleStatusMachine).
pub async fn cancel_order_by_restaurant(
    store: &dyn EventStore,
    cmd: CancelOrderByRestaurant,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_order(store, &cmd.order_id, &cmd.restaurant_id).await?;
    let event = DomainEvent::OrderCancelledByRestaurant(OrderCancelledByRestaurant {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
        reason: cmd.reason,
    });
    require_order_transition(&cmd.order_id, state.status, &event)?;
    Repository::new(store).save(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
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
        customer_id: state.customer_id,
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
        customer_id: state.customer_id,
        stars: cmd.stars,
        comment: cmd.comment,
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
    // envelope UserType ordinal (ADR-0037/0041): RESTAURANT_ACCOUNT (2) / RESTAURANT (3) tip as the
    // restaurant; everyone else is the customer.
    let tipped_by = if actor.user_type == 2 || actor.user_type == 3 {
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
        customer_id: if tipped_by == Tipper::CUSTOMER { state.customer_id } else { None },
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
        customer_id: state.customer_id,
        reason: cmd.reason,
    });
    Repository::new(store).save(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
}

// ================================================================================================
// DeliveryJob aggregate (actors.yaml#/DeliveryJob) — independent-rider fulfilment (ADR-0031).
// ================================================================================================

/// The stream a DeliveryJob aggregate lives on.
fn delivery_job_stream(id: &DeliveryJobId) -> String {
    format!("DeliveryJob-{}", id.0)
}

/// Rehydrate the DeliveryJob aggregate and require existence, or reject with
/// `errors.yaml#/DeliveryJobNotFound`.
async fn require_delivery_job(
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
fn invalid_delivery_status(
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
    if state.status != DeliveryStatus::ASSIGNED {
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
    let event = DomainEvent::DeliveryPickedUp(DeliveryPickedUp {
        delivery_job_id: cmd.delivery_job_id,
        rider_id: cmd.rider_id,
        at: None,
    });
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
    if !matches!(state.status, DeliveryStatus::PICKED_UP | DeliveryStatus::OUT_FOR_DELIVERY) {
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
    let event = DomainEvent::DeliveryCompleted(DeliveryCompleted {
        delivery_job_id: cmd.delivery_job_id,
        at: None,
    });
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
    match state.status {
        DeliveryStatus::DELIVERED => {
            return Err(invalid_delivery_status(
                &cmd.delivery_job_id,
                state.status,
                DeliveryStatus::PENDING,
            ));
        }
        DeliveryStatus::CANCELLED => return Ok(()),
        _ => {}
    }
    let event = DomainEvent::DeliveryCancelled(DeliveryCancelled {
        delivery_job_id: cmd.delivery_job_id,
        reason: cmd.reason,
    });
    Repository::new(store).save(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor).await.map(|_| ())
}

// ================================================================================================
// DeliveryJob ops — decline/issue/status/partner commands (ADR-20260719-193500 command surface).
// ================================================================================================

/// Whether `from` → `to` is a legal delivery status transition: forward along
/// PENDING → ASSIGNED → PICKED_UP → OUT_FOR_DELIVERY → DELIVERED (with the same PICKED_UP → DELIVERED
/// shortcut [`complete_delivery`] allows — a hand-over may skip the explicit OUT_FOR_DELIVERY step),
/// plus CANCELLED/FAILED from any non-terminal state. DELIVERED/CANCELLED/FAILED are terminal.
fn delivery_can_transition(from: DeliveryStatus, to: DeliveryStatus) -> bool {
    use DeliveryStatus::*;
    matches!(
        (from, to),
        (PENDING, ASSIGNED)
            | (ASSIGNED, PICKED_UP)
            | (PICKED_UP, OUT_FOR_DELIVERY)
            | (PICKED_UP, DELIVERED)
            | (OUT_FOR_DELIVERY, DELIVERED)
    ) || (matches!(to, CANCELLED | FAILED) && !matches!(from, DELIVERED | CANCELLED | FAILED))
}

/// The status a job "needed to be in" for a transition INTO `to` — the `expectedStatus` diagnostic on
/// an `InvalidDeliveryStatus` rejection of an invalid transition (the canonical predecessor in the
/// lifecycle; only `currentStatus` is interpolated into the catalogued message).
fn canonical_predecessor(to: DeliveryStatus) -> DeliveryStatus {
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
        reported_at: Some(chrono::Utc::now().to_rfc3339()),
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
        resolved_at: Some(chrono::Utc::now().to_rfc3339()),
    });
    Repository::new(store).save(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/UpdateDeliveryStatus` → emit `events.yaml#/DeliveryStatusUpdated` (the
/// independent-rider path / admin correction). The requested status must be a legal transition from
/// the current one per [`delivery_can_transition`]
/// (rules.yaml#/DeliveryPickupAndCompletionByRider).
pub async fn update_delivery_status(
    store: &dyn EventStore,
    cmd: UpdateDeliveryStatus,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_delivery_job(store, &cmd.delivery_job_id).await?;
    if !delivery_can_transition(state.status, cmd.status) {
        return Err(invalid_delivery_status(
            &cmd.delivery_job_id,
            state.status,
            canonical_predecessor(cmd.status),
        ));
    }
    let event = DomainEvent::DeliveryStatusUpdated(DeliveryStatusUpdated {
        delivery_job_id: cmd.delivery_job_id,
        partner_ref: None,
        status: cmd.status,
        occurred_at: None, // the record time is the envelope's occurred_at; the payload field is for externally reported times
        note: None,
    });
    Repository::new(store).save(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/AssignDeliveryToPartner` → emit `events.yaml#/DeliveryAssignedToPartner`.
/// Only a PENDING job; a job already taken (rider or partner) rejects with `DeliveryAlreadyAssigned`
/// (rules.yaml#/DeliveryPartnerAssignmentLifecycle).
pub async fn assign_delivery_to_partner(
    store: &dyn EventStore,
    cmd: AssignDeliveryToPartner,
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
    let event = DomainEvent::DeliveryAssignedToPartner(DeliveryAssignedToPartner {
        delivery_job_id: cmd.delivery_job_id,
        partner_ref: cmd.partner_ref,
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
    if state.status != DeliveryStatus::ASSIGNED {
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
    let event = DomainEvent::DeliveryUnassignedFromPartner(DeliveryUnassignedFromPartner {
        delivery_job_id: cmd.delivery_job_id,
        reason: cmd.reason,
    });
    Repository::new(store).save(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/UpdateDeliveryPartnerStatus` → emit
/// `events.yaml#/DeliveryPartnerStatusUpdated` (the avelo37-acl relays the inbound partner report as
/// this command so the aggregate applies it). The reported status must be a legal transition from the
/// current one per [`delivery_can_transition`] (rules.yaml#/DeliveryPartnerAssignmentLifecycle).
pub async fn update_delivery_partner_status(
    store: &dyn EventStore,
    cmd: UpdateDeliveryPartnerStatus,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_delivery_job(store, &cmd.delivery_job_id).await?;
    if !delivery_can_transition(state.status, cmd.status) {
        return Err(invalid_delivery_status(
            &cmd.delivery_job_id,
            state.status,
            canonical_predecessor(cmd.status),
        ));
    }
    let event = DomainEvent::DeliveryPartnerStatusUpdated(DeliveryPartnerStatusUpdated {
        delivery_job_id: cmd.delivery_job_id,
        partner_ref: cmd.partner_ref,
        status: cmd.status,
        occurred_at: None, // set only when the partner reported the time; the envelope records ours
    });
    Repository::new(store).save(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor).await.map(|_| ())
}

// ================================================================================================
// Rider aggregate (actors.yaml#/Rider) — rider identity + the availability status machine.
// ================================================================================================

/// The stream a Rider aggregate lives on.
fn rider_stream(id: &RiderId) -> String {
    format!("Rider-{}", id.0)
}

/// Rehydrate the Rider aggregate and require existence, or reject with `errors.yaml#/RiderNotFound`.
async fn require_rider(
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

/// Handle `commands.yaml#/ChangeRiderStatus` → emit `events.yaml#/RiderStatusChanged`. The move must
/// be legal per the availability machine (`domain::rider::can_transition`) or it rejects with
/// `errors.yaml#/InvalidRiderStatusTransition` (rules.yaml#/RiderLifecycle).
pub async fn change_rider_status(
    store: &dyn EventStore,
    cmd: ChangeRiderStatus,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_rider(store, &cmd.rider_id).await?;
    if !domain::rider::can_transition(state.status, cmd.status) {
        return Err(reject(
            "InvalidRiderStatusTransition",
            json!({
                "riderId": cmd.rider_id,
                "currentStatus": state.status,
                "targetStatus": cmd.status,
            }),
        ));
    }
    let event = DomainEvent::RiderStatusChanged(RiderStatusChanged {
        rider_id: cmd.rider_id,
        status: cmd.status,
    });
    Repository::new(store).save(&rider_stream(&cmd.rider_id), version, &[event], actor).await.map(|_| ())
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
/// the Stripe PaymentIntent through the [`PaymentGateway`] seam for exactly that recomputed amount
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
pub async fn place_order(
    store: &dyn EventStore,
    catalogs: &dyn CatalogReadRepository,
    payments: &dyn PaymentGateway,
    pm_state: &dyn PaymentProcessStateStore,
    cmd: PlaceOrder,
    actor: &Actor,
) -> Result<CreatedPaymentIntent, DomainError> {
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
    let amount = priced.total_amount.clone();
    // Create the Stripe PaymentIntent through the gateway seam FOR THE RECOMPUTED AMOUNT; a
    // synchronous decline surfaces as the canonical `PaymentDeclined` rejection.
    let intent = payments
        .create_payment_intent(&crate::ports::PaymentIntentRequest {
            amount: amount.clone(),
            payment_method_id: cmd.payment_method_id.clone(),
            order_id: cmd.order_id,
            restaurant_id: cmd.restaurant_id,
            cart_id: cmd.cart_id,
        })
        .await?;
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
        total_amount: amount.clone(),
        breakdown: priced.breakdown.clone(),
        note: cmd.note.clone(),
    };
    // Deliver the saga's first fact to the Payment aggregate's stream — its BIRTH (the Order stream
    // stays empty until the capture leg materializes OrderPlaced). `create` absorbs a version-0 clash:
    // the gateway is idempotent per payment method+cart replay windows, so re-hitting an existing
    // `Payment-<intentId>` stream is a re-delivered birth, not a new fact.
    let event = DomainEvent::PaymentIntentCreated(PaymentIntentCreated {
        payment_intent_id: intent.payment_intent_id.clone(),
        restaurant_id: cmd.restaurant_id,
        customer_id: cmd.customer_id,
        amount,
        checkout,
    });
    Repository::new(store)
        .create(&domain::payment::stream(&intent.payment_intent_id), &[event], actor)
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
            // anonymous session is dispatch-layer knowledge (X-SESSION-ID) — the acceptance-first
            // dispatch stamps it when it wires through (placeOrder requires CUSTOMER, so the
            // customer_id scope is the one that matters here).
            customer_id: cmd.customer_id,
            session_id: None,
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
    idempotent_on_existing(Repository::new(store).save(&stream_name, 0, &[event], actor).await)
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
// The request/confirm pairs stay pure: the AuthProviderGateway port is the ACL boundary doing the
// actual Supabase call; only verified FACTS are appended here.
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
fn canonical_phone(dialing_code: &DialingCode, national_number: &NationalPhoneNumber) -> PhoneNumber {
    PhoneNumber(format!("{}{}", dialing_code.0, national_number.0.trim_start_matches('0')))
}

/// Handle `commands.yaml#/RequestPhoneVerification` — a pure EFFECT (actors.yaml: emits nothing):
/// delegate the SMS OTP send to the wrapped auth provider (Supabase → Twilio, ADR-0015), localized by
/// the locale the caller provided (pre-identification, so there is no stored locale yet).
pub async fn request_phone_verification(
    _store: &dyn EventStore,
    auth: &dyn AuthProviderGateway,
    cmd: RequestPhoneVerification,
    _actor: &Actor,
) -> Result<(), DomainError> {
    auth.send_phone_otp(&cmd.dialing_code, &cmd.national_number, cmd.locale.as_ref()).await
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

/// Handle `commands.yaml#/VerifyPhone` → register-or-identify. The OTP is verified through the auth
/// provider port (`InvalidVerificationCode` / `VerificationCodeExpired`); the backend then decides
/// new-vs-returning by resolving the canonical phone in the Customer read model: a known phone emits
/// `CustomerIdentified` on the EXISTING customer's stream (the client-proposed id is discarded), a
/// new phone emits `CustomerRegistered` on the new `Customer-<id>` stream (idempotent on replay).
pub async fn verify_phone(
    store: &dyn EventStore,
    auth: &dyn AuthProviderGateway,
    customers: &dyn CustomerReadRepository,
    cmd: VerifyPhone,
    actor: &Actor,
) -> Result<VerifyPhoneOutcome, DomainError> {
    let phone = canonical_phone(&cmd.dialing_code, &cmd.national_number);
    let auth_ref = match auth
        .verify_phone_otp(&cmd.dialing_code, &cmd.national_number, &cmd.code)
        .await?
    {
        PhoneOtpCheck::Verified { auth_ref } => auth_ref,
        PhoneOtpCheck::Invalid => {
            return Err(reject("InvalidVerificationCode", json!({ "phone": phone })));
        }
        PhoneOtpCheck::Expired => {
            return Err(reject("VerificationCodeExpired", json!({})));
        }
    };
    if let Some(existing) = customers.by_phone(phone.clone()).await? {
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
    idempotent_on_existing(Repository::new(store).save(&stream_name, 0, &[event], actor).await)?;
    Ok(VerifyPhoneOutcome { customer_id, created: true })
}

/// Handle `commands.yaml#/RequestEmailVerification` — a pure EFFECT (emits nothing): reject an email
/// already owned by ANOTHER customer (`EmailAlreadyInUse`), then delegate the magic-link send to the
/// auth provider, localized via the customer's STORED locale (ADR-0015: no per-call language param).
pub async fn request_email_verification(
    store: &dyn EventStore,
    auth: &dyn AuthProviderGateway,
    customers: &dyn CustomerReadRepository,
    cmd: RequestEmailVerification,
    _actor: &Actor,
) -> Result<(), DomainError> {
    if let Some(owner) = customers.by_email(cmd.email.clone()).await? {
        if owner.customer_id != cmd.customer_id {
            return Err(reject("EmailAlreadyInUse", json!({ "email": cmd.email })));
        }
    }
    let (state, _version) = load_customer(store, &cmd.customer_id).await?;
    let locale = state.and_then(|s| s.locale);
    auth.send_email_magic_link(&cmd.email, locale.as_ref()).await
}

/// Handle `commands.yaml#/ConfirmEmailVerification` → emit `events.yaml#/CustomerEmailVerified`. The
/// token is verified SERVER-SIDE through the auth provider port (`InvalidVerificationToken` /
/// `VerificationCodeExpired`), which reports the email it proves — the linked email is never taken
/// from client input.
pub async fn confirm_email_verification(
    store: &dyn EventStore,
    auth: &dyn AuthProviderGateway,
    cmd: ConfirmEmailVerification,
    actor: &Actor,
) -> Result<(), DomainError> {
    let email = match auth.verify_email_token(&cmd.token).await? {
        EmailTokenCheck::Verified { email } => email,
        EmailTokenCheck::Invalid => {
            return Err(reject("InvalidVerificationToken", json!({})));
        }
        EmailTokenCheck::Expired => {
            return Err(reject("VerificationCodeExpired", json!({})));
        }
    };
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
    auth: &dyn AuthProviderGateway,
    customers: &dyn CustomerReadRepository,
    cmd: RequestPhoneChange,
    _actor: &Actor,
) -> Result<(), DomainError> {
    let new_phone = canonical_phone(&cmd.new_dialing_code, &cmd.new_national_number);
    if let Some(owner) = customers.by_phone(new_phone.clone()).await? {
        if owner.customer_id != cmd.customer_id {
            return Err(reject("PhoneAlreadyInUse", json!({ "phone": new_phone })));
        }
    }
    let (state, _version) = load_customer(store, &cmd.customer_id).await?;
    let locale = state.and_then(|s| s.locale);
    auth.send_phone_otp(&cmd.new_dialing_code, &cmd.new_national_number, locale.as_ref()).await
}

/// Handle `commands.yaml#/ConfirmPhoneChange` → emit `events.yaml#/CustomerPhoneChanged` (canonical
/// E.164). The OTP on the NEW phone is verified through the auth provider port
/// (`InvalidVerificationCode` / `VerificationCodeExpired`) and uniqueness is re-checked at confirm
/// time (`PhoneAlreadyInUse`).
pub async fn confirm_phone_change(
    store: &dyn EventStore,
    auth: &dyn AuthProviderGateway,
    customers: &dyn CustomerReadRepository,
    cmd: ConfirmPhoneChange,
    actor: &Actor,
) -> Result<(), DomainError> {
    let new_phone = canonical_phone(&cmd.new_dialing_code, &cmd.new_national_number);
    match auth
        .verify_phone_otp(&cmd.new_dialing_code, &cmd.new_national_number, &cmd.code)
        .await?
    {
        PhoneOtpCheck::Verified { .. } => {}
        PhoneOtpCheck::Invalid => {
            return Err(reject("InvalidVerificationCode", json!({ "phone": new_phone })));
        }
        PhoneOtpCheck::Expired => {
            return Err(reject("VerificationCodeExpired", json!({})));
        }
    }
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
