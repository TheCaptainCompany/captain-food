//! CQRS command handlers (write side, ADR-0035). Thin by design: rehydrate the aggregate state by
//! folding its stream (loaded through the [`EventStore`] port), enforce the invariants declared for
//! that message in `specs/actors.yaml` (`throws` → `specs/errors.yaml`), then append the declared
//! `emits` event(s) at the expected version. Ids are client/ACL-generated (ADR-0034), so creation
//! commands are idempotent: replaying one hits the UNIQUE(stream_name, version) guard and is absorbed
//! as an already-registered no-op instead of duplicating the fact.
//!
//! Rejections carry the errors.yaml CODE: [`DomainError::Invariant`] only models a string, so the
//! canonical shape is `"<Code>: <context>"` (see [`reject`] / [`rejection_code`]) until a structured
//! error type lands.
//!
//! Cross-aggregate invariants that still lack a read port are explicit `TODO(invariant)` markers —
//! they are NOT silently skipped semantics, they are the documented gap.

use domain::generated::commands::{
    ActivateRestaurant, ChangeOrderAcceptanceMode, ChangeRestaurantListingStatus,
    ClaimRestaurantListing, ConfigureGoogleBusinessProfileOrderLink, DeactivateRestaurant,
    MarkRestaurantClosed, OptOutRestaurantListing, RegisterRestaurant, RegisterRestaurantAccount,
    RemoveRestaurant, UpdateRestaurant, UpdateRestaurantGoogleBusinessProfile,
    VerifyGoogleBusinessProfileOrderLink,
};
use domain::generated::events::{
    DomainEvent, RestaurantAcceptanceModeChanged, RestaurantAccountRegistered, RestaurantActivated,
    RestaurantDeactivated, RestaurantGoogleBusinessProfileOrderLinkConfigured,
    RestaurantGoogleBusinessProfileOrderLinkVerified, RestaurantGoogleBusinessProfileUpdated,
    RestaurantListingClaimed, RestaurantListingOptedOut, RestaurantListingStatusChanged,
    RestaurantMarkedClosed, RestaurantRegistered, RestaurantRemoved, RestaurantUpdated,
};
use domain::generated::scalars::{RestaurantId, RestaurantListingStatus, RestaurantStatus};
use domain::restaurant::RestaurantState;
use domain::shared::errors::DomainError;

use crate::ports::{is_version_conflict, Actor, EventStore, GbpOrderLinkProbe, GoogleOwnershipVerifier};
use crate::queries::RestaurantReadRepository;

// --- Cart / Order / DeliveryJob / PlaceOrderProcess (checkout→order→delivery flow, ADR-0046 round 2) ---
use domain::cart::{CartState, MAX_LINE_QUANTITY};
use domain::delivery_job::DeliveryJobState;
use domain::generated::commands::{
    AcceptDelivery, AcceptOrder, AddCartLine, CancelDelivery, CancelOrderByCustomer,
    CancelOrderByRestaurant, ChangeCartLineQuantity, CompleteDelivery, ConfirmPickup,
    MarkOrderDelivered, MarkOrderReady, PlaceOrder, RateOrder, RateRestaurant, RejectOrder,
    RemoveCartLine, RequestRefund, StartPreparation, TipOrder,
};
use domain::generated::entities::{CartLineItem, Money};
use domain::generated::events::{
    CartLineAdded, CartLineQuantityChanged, CartLineRemoved, CartStarted, DeliveryAcceptedByRider,
    DeliveryCancelled, DeliveryCompleted, DeliveryPickedUp, OrderAcceptedByRestaurant,
    OrderCancelledByCustomer, OrderCancelledByRestaurant, OrderDelivered, OrderMarkedReady,
    OrderPreparationStarted, OrderRated, OrderRejectedByRestaurant, OrderTipped,
    PaymentIntentCreated, RefundRequested, RestaurantRated as RestaurantRatedEvent,
};
use domain::generated::scalars::{
    CartId, CartStatus, DeliveryJobId, DeliveryStatus, Mode, OrderAcceptanceMode, OrderId,
    OrderStatus, ServiceType, TipRecipient, Tipper,
};
use domain::order::OrderState;

use crate::ports::{CreatedPaymentIntent, PaymentGateway};
use crate::queries::CartReadRepository;

/// Absorb the optimistic-concurrency clash of a CREATION command (expected_version = 0) as success:
/// the aggregate already exists under this client-generated id, so re-running the command is a no-op.
fn idempotent_on_existing(result: Result<i64, DomainError>) -> Result<(), DomainError> {
    match result {
        Ok(_) => Ok(()),
        Err(e) if is_version_conflict(&e) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Build the canonical rejection for an `errors.yaml` invariant: the CODE, then the context detail —
/// `"<Code>: <detail>"`. [`rejection_code`] is the matching reader.
fn reject(code: &str, detail: impl std::fmt::Display) -> DomainError {
    DomainError::Invariant(format!("{code}: {detail}"))
}

/// The errors.yaml code a command rejection carries (`"<Code>: <detail>"`), if this is one.
pub fn rejection_code(err: &DomainError) -> Option<&str> {
    match err {
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
    let (events, version) = store.load(&restaurant_stream(id)).await?;
    Ok((domain::restaurant::fold(&events), version))
}

/// Rehydrate and require existence, or reject with `errors.yaml#/RestaurantNotFound`.
async fn require_restaurant(
    store: &dyn EventStore,
    id: &RestaurantId,
) -> Result<(RestaurantState, i64), DomainError> {
    let (state, version) = load_restaurant(store, id).await?;
    match state {
        Some(state) => Ok((state, version)),
        None => Err(reject("RestaurantNotFound", format!("restaurantId={}", id.0))),
    }
}

/// Handle `commands.yaml#/RegisterRestaurantAccount` → emit `events.yaml#/RestaurantAccountRegistered`
/// on the new `RestaurantAccount-<id>` stream (actors.yaml, RestaurantAccount aggregate).
pub async fn register_restaurant_account(
    store: &dyn EventStore,
    cmd: RegisterRestaurantAccount,
    actor: &Actor,
) -> Result<(), DomainError> {
    // TODO(invariant): RefAlreadyUsed — reject when cmd.ref is already owned by another aggregate
    //                  (needs an external-reference read-model lookup).
    // TODO(invariant): InvalidCurrency — reject when cmd.default_currency is not a valid ISO 4217 code.
    let stream_name = format!("RestaurantAccount-{}", cmd.restaurant_account_id.0);
    let event = DomainEvent::RestaurantAccountRegistered(RestaurantAccountRegistered {
        restaurant_account_id: cmd.restaurant_account_id,
        r#ref: cmd.r#ref,
        legal_name: cmd.legal_name,
        contact: cmd.contact,
        default_currency: cmd.default_currency,
        default_tax_rate: cmd.default_tax_rate,
        timezone: cmd.timezone,
    });
    idempotent_on_existing(store.append(&stream_name, 0, &[event], actor).await)
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
            return Err(reject("SlugAlreadyTaken", format!("slug={}", cmd.slug.0)));
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
    idempotent_on_existing(store.append(&stream_name, 0, &[event], actor).await)
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
    store.append(&stream_name, version, &[event], actor).await.map(|_| ())
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
        return Err(reject("NoEditableFieldProvided", "update carried no editable field"));
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
    store.append(&stream_name, version, &[event], actor).await.map(|_| ())
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
    store.append(&stream_name, version, &[event], actor).await.map(|_| ())
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
            format!("restaurantId={} restaurantName={}", cmd.restaurant_id.0, state.display_name.0),
        ));
    }
    if state.order_acceptance == cmd.mode {
        return Err(reject(
            "AcceptanceModeUnchanged",
            format!(
                "restaurantId={} restaurantName={} mode={:?}",
                cmd.restaurant_id.0, state.display_name.0, cmd.mode
            ),
        ));
    }
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = DomainEvent::RestaurantAcceptanceModeChanged(RestaurantAcceptanceModeChanged {
        restaurant_id: cmd.restaurant_id,
        mode: cmd.mode,
    });
    store.append(&stream_name, version, &[event], actor).await.map(|_| ())
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
    store.append(&stream_name, version, &[event], actor).await.map(|_| ())
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
    store.append(&stream_name, version, &[event], actor).await.map(|_| ())
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
    store.append(&stream_name, version, &[event], actor).await.map(|_| ())
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
        return Err(reject(
            "ListingAlreadyClaimed",
            format!("restaurantId={}", cmd.restaurant_id.0),
        ));
    }
    if !ownership.verify(cmd.restaurant_id, &cmd.google_ownership_proof).await? {
        return Err(reject(
            "ListingOwnershipNotVerified",
            format!("restaurantId={}", cmd.restaurant_id.0),
        ));
    }
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = DomainEvent::RestaurantListingClaimed(RestaurantListingClaimed {
        restaurant_id: cmd.restaurant_id,
        account_id: cmd.account_id,
        proof: Some(cmd.google_ownership_proof),
    });
    store.append(&stream_name, version, &[event], actor).await.map(|_| ())
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
            format!("restaurantId={}", cmd.restaurant_id.0),
        ));
    }
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = DomainEvent::RestaurantListingOptedOut(RestaurantListingOptedOut {
        restaurant_id: cmd.restaurant_id,
        reason: cmd.reason,
    });
    store.append(&stream_name, version, &[event], actor).await.map(|_| ())
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
    store.append(&stream_name, version, &[event], actor).await.map(|_| ())
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
    store.append(&stream_name, version, &[event], actor).await.map(|_| ())
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
        return Err(reject(
            "GbpOrderLinkNotConfigured",
            format!("restaurantId={}", cmd.restaurant_id.0),
        ));
    };
    let status = probe.probe(&url).await?;
    let stream_name = restaurant_stream(&cmd.restaurant_id);
    let event = DomainEvent::RestaurantGoogleBusinessProfileOrderLinkVerified(
        RestaurantGoogleBusinessProfileOrderLinkVerified {
            restaurant_id: cmd.restaurant_id,
            status,
        },
    );
    store.append(&stream_name, version, &[event], actor).await.map(|_| ())
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
    let (events, version) = store.load(&cart_stream(id)).await?;
    Ok((domain::cart::fold(&events), version))
}

/// Rehydrate and require existence, or reject with `errors.yaml#/CartNotFound`.
async fn require_cart(
    store: &dyn EventStore,
    id: &CartId,
) -> Result<(CartState, i64), DomainError> {
    let (state, version) = load_cart(store, id).await?;
    match state {
        Some(state) => Ok((state, version)),
        None => Err(reject("CartNotFound", format!("cartId={}", id.0))),
    }
}

/// The `errors.yaml#/CartNotOpen` rejection for `cart_id` in `status`.
fn cart_not_open(cart_id: &CartId, status: CartStatus) -> DomainError {
    reject("CartNotOpen", format!("cartId={} status={:?}", cart_id.0, status))
}

/// Handle `commands.yaml#/AddCartLine` → emit `events.yaml#/CartStarted` (first line only, creating
/// the cart) + `events.yaml#/CartLineAdded` (actors.yaml, Cart aggregate). The client generates the
/// cartId and the cartLineId: the first add for a new cartId CREATES the cart bound to the restaurant
/// (so `CartNotFound` is unreachable for this command by construction), and re-sending a line id the
/// cart already holds is an idempotent replay (no duplicate fact).
pub async fn add_cart_line(
    store: &dyn EventStore,
    cmd: AddCartLine,
    actor: &Actor,
) -> Result<(), DomainError> {
    // TODO(invariant): OfferNotFound / OfferUnavailable / InsufficientStock / InvalidOptionSelection —
    //                  validating the line against the LIVE catalog (availability, stock, option-list
    //                  min/max) needs an offer-level Catalog read port; the Catalog projection's `tree`
    //                  is not yet queryable per offer (projector TODO(runtime)).
    if cmd.line.quantity > MAX_LINE_QUANTITY {
        return Err(reject(
            "QuantityExceedsLimit",
            format!("offerId={} quantity={}", cmd.line.offer_id.0, cmd.line.quantity),
        ));
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
            let events = [
                DomainEvent::CartStarted(CartStarted {
                    cart_id: cmd.cart_id,
                    restaurant_id: cmd.restaurant_id,
                    customer_id: None,
                }),
                DomainEvent::CartLineAdded(CartLineAdded { cart_id: cmd.cart_id, line }),
            ];
            // A version-0 clash here is a REAL race (two concurrent first adds with different lines),
            // not a replay — do not absorb it; the client retries onto the now-existing cart.
            store.append(&cart_stream(&cmd.cart_id), 0, &events, actor).await.map(|_| ())
        }
        Some(s) => {
            if s.status != CartStatus::OPEN {
                return Err(cart_not_open(&cmd.cart_id, s.status));
            }
            if s.restaurant_id != cmd.restaurant_id {
                return Err(reject(
                    "CartRestaurantMismatch",
                    format!("cartId={} restaurantId={}", cmd.cart_id.0, cmd.restaurant_id.0),
                ));
            }
            if s.line_ids.contains(&line.cart_line_id) {
                return Ok(()); // idempotent replay of an already-recorded line (client-generated id)
            }
            let event = DomainEvent::CartLineAdded(CartLineAdded { cart_id: cmd.cart_id, line });
            store.append(&cart_stream(&cmd.cart_id), version, &[event], actor).await.map(|_| ())
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
            format!("cartId={} cartLineId={}", cmd.cart_id.0, cmd.cart_line_id.0),
        ));
    }
    let event = DomainEvent::CartLineRemoved(CartLineRemoved {
        cart_id: cmd.cart_id,
        cart_line_id: cmd.cart_line_id,
    });
    store.append(&cart_stream(&cmd.cart_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/ChangeCartLineQuantity` → emit `events.yaml#/CartLineQuantityChanged`
/// (actors.yaml, Cart aggregate). Only an OPEN cart is editable, the line must exist and the new
/// quantity must respect the per-line cap.
pub async fn change_cart_line_quantity(
    store: &dyn EventStore,
    cmd: ChangeCartLineQuantity,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_cart(store, &cmd.cart_id).await?;
    if state.status != CartStatus::OPEN {
        return Err(cart_not_open(&cmd.cart_id, state.status));
    }
    if !state.line_ids.contains(&cmd.cart_line_id) {
        return Err(reject(
            "CartLineNotFound",
            format!("cartId={} cartLineId={}", cmd.cart_id.0, cmd.cart_line_id.0),
        ));
    }
    // TODO(invariant): InsufficientStock — checking the new quantity against the offer's live stock
    //                  needs the same offer-level Catalog read port as add_cart_line.
    if cmd.quantity > MAX_LINE_QUANTITY {
        return Err(reject(
            "QuantityExceedsLimit",
            format!("cartLineId={} quantity={}", cmd.cart_line_id.0, cmd.quantity),
        ));
    }
    let event = DomainEvent::CartLineQuantityChanged(CartLineQuantityChanged {
        cart_id: cmd.cart_id,
        cart_line_id: cmd.cart_line_id,
        quantity: cmd.quantity,
    });
    store.append(&cart_stream(&cmd.cart_id), version, &[event], actor).await.map(|_| ())
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
    let (events, version) = store.load(&order_stream(order_id)).await?;
    match domain::order::fold(&events) {
        Some(state) if state.restaurant_id == *restaurant_id => Ok((state, version)),
        _ => Err(reject("OrderNotFound", format!("orderId={}", order_id.0))),
    }
}

/// The `errors.yaml#/InvalidOrderStatus` rejection for `order_id` currently in `status`.
fn invalid_order_status(order_id: &OrderId, status: OrderStatus) -> DomainError {
    reject("InvalidOrderStatus", format!("orderId={} currentStatus={:?}", order_id.0, status))
}

/// Handle `commands.yaml#/AcceptOrder` → emit `events.yaml#/OrderAcceptedByRestaurant`. Only a PLACED
/// order can be accepted (rules.yaml#/OrderLifecycleStatusMachine).
pub async fn accept_order(
    store: &dyn EventStore,
    cmd: AcceptOrder,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_order(store, &cmd.order_id, &cmd.restaurant_id).await?;
    if state.status != OrderStatus::PLACED {
        return Err(invalid_order_status(&cmd.order_id, state.status));
    }
    let event = DomainEvent::OrderAcceptedByRestaurant(OrderAcceptedByRestaurant {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
        estimated_ready_at: cmd.estimated_ready_at,
    });
    store.append(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/StartPreparation` → emit `events.yaml#/OrderPreparationStarted`. Only an
/// ACCEPTED order moves to PREPARING (rules.yaml#/OrderLifecycleStatusMachine).
pub async fn start_preparation(
    store: &dyn EventStore,
    cmd: StartPreparation,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_order(store, &cmd.order_id, &cmd.restaurant_id).await?;
    if state.status != OrderStatus::ACCEPTED {
        return Err(invalid_order_status(&cmd.order_id, state.status));
    }
    let event = DomainEvent::OrderPreparationStarted(OrderPreparationStarted {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
    });
    store.append(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
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
    if !matches!(state.status, OrderStatus::ACCEPTED | OrderStatus::PREPARING) {
        return Err(invalid_order_status(&cmd.order_id, state.status));
    }
    let event = DomainEvent::OrderMarkedReady(OrderMarkedReady {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
    });
    store.append(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
}

/// Handle `commands.yaml#/MarkOrderDelivered` → emit `events.yaml#/OrderDelivered`. Allowed from READY
/// (hand-over/collection) or OUT_FOR_DELIVERY (rules.yaml#/OrderLifecycleStatusMachine).
pub async fn mark_order_delivered(
    store: &dyn EventStore,
    cmd: MarkOrderDelivered,
    actor: &Actor,
) -> Result<(), DomainError> {
    let (state, version) = require_order(store, &cmd.order_id, &cmd.restaurant_id).await?;
    if !matches!(state.status, OrderStatus::READY | OrderStatus::OUT_FOR_DELIVERY) {
        return Err(invalid_order_status(&cmd.order_id, state.status));
    }
    let event = DomainEvent::OrderDelivered(OrderDelivered {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
    });
    store.append(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
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
    if state.status != OrderStatus::PLACED {
        return Err(invalid_order_status(&cmd.order_id, state.status));
    }
    let event = DomainEvent::OrderRejectedByRestaurant(OrderRejectedByRestaurant {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
        reason: cmd.reason,
    });
    store.append(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
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
    if state.status != OrderStatus::PLACED {
        return Err(invalid_order_status(&cmd.order_id, state.status));
    }
    let event = DomainEvent::OrderCancelledByCustomer(OrderCancelledByCustomer {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
        reason: cmd.reason,
    });
    store.append(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
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
    if !matches!(state.status, OrderStatus::ACCEPTED | OrderStatus::PREPARING | OrderStatus::READY) {
        return Err(invalid_order_status(&cmd.order_id, state.status));
    }
    let event = DomainEvent::OrderCancelledByRestaurant(OrderCancelledByRestaurant {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
        reason: cmd.reason,
    });
    store.append(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
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
        return Err(reject("OrderAlreadyRated", format!("orderId={}", cmd.order_id.0)));
    }
    let event = DomainEvent::OrderRated(OrderRated {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
        customer_id: state.customer_id,
        rider_thumb: cmd.rider_thumb,
    });
    store.append(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
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
        return Err(reject("RestaurantAlreadyRated", format!("orderId={}", cmd.order_id.0)));
    }
    let event = DomainEvent::RestaurantRated(RestaurantRatedEvent {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
        customer_id: state.customer_id,
        stars: cmd.stars,
        comment: cmd.comment,
    });
    store.append(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
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
        return Err(reject("ValidationError", "tips must contain at least one tip"));
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
            format!("tippedBy={:?} recipient={:?}", tipped_by, TipRecipient::RESTAURANT),
        ));
    }
    let event = DomainEvent::OrderTipped(OrderTipped {
        order_id: cmd.order_id,
        restaurant_id: cmd.restaurant_id,
        tipped_by,
        customer_id: if tipped_by == Tipper::CUSTOMER { state.customer_id } else { None },
        tips: cmd.tips,
    });
    store.append(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
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
    store.append(&order_stream(&cmd.order_id), version, &[event], actor).await.map(|_| ())
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
    let (events, version) = store.load(&delivery_job_stream(id)).await?;
    match domain::delivery_job::fold(&events) {
        Some(state) => Ok((state, version)),
        None => Err(reject("DeliveryJobNotFound", format!("deliveryJobId={}", id.0))),
    }
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
        format!("deliveryJobId={} currentStatus={:?} expectedStatus={:?}", id.0, current, expected),
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
                format!("deliveryJobId={}", cmd.delivery_job_id.0),
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
    store.append(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor).await.map(|_| ())
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
        return Err(reject(
            "InvalidDeliveryStatus",
            format!(
                "deliveryJobId={} currentStatus={:?} expectedStatus={:?} (job is not assigned to rider {})",
                cmd.delivery_job_id.0,
                state.status,
                DeliveryStatus::ASSIGNED,
                cmd.rider_id.0
            ),
        ));
    }
    let event = DomainEvent::DeliveryPickedUp(DeliveryPickedUp {
        delivery_job_id: cmd.delivery_job_id,
        rider_id: cmd.rider_id,
        at: None,
    });
    store.append(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor).await.map(|_| ())
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
        return Err(reject(
            "InvalidDeliveryStatus",
            format!(
                "deliveryJobId={} currentStatus={:?} expectedStatus={:?} (job is not assigned to rider {})",
                cmd.delivery_job_id.0,
                state.status,
                DeliveryStatus::PICKED_UP,
                cmd.rider_id.0
            ),
        ));
    }
    let event = DomainEvent::DeliveryCompleted(DeliveryCompleted {
        delivery_job_id: cmd.delivery_job_id,
        at: None,
    });
    store.append(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor).await.map(|_| ())
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
    store.append(&delivery_job_stream(&cmd.delivery_job_id), version, &[event], actor).await.map(|_| ())
}

// ================================================================================================
// PlaceOrderProcess (actors.yaml#/PlaceOrderProcess) — the checkout saga's command leg.
// ================================================================================================

/// Handle `commands.yaml#/PlaceOrder` → emit `events.yaml#/PaymentIntentCreated` on the (future)
/// order's stream (actors.yaml, PlaceOrderProcess). This is ONLY the saga's first, command-initiated
/// leg: validate the checkout, price the cart server-side and create the Stripe PaymentIntent through
/// the [`PaymentGateway`] seam (a synchronous decline is the canonical
/// `errors.yaml#/PaymentDeclined`). Returns the created intent so the mutation payload can carry
/// `paymentIntentId`/`clientSecret` (api.yaml).
///
/// TODO(saga): the remaining PlaceOrderProcess legs are event-driven and NOT implemented here —
///   * `events.yaml#/PaymentCaptured` (INBOUND Stripe webhook, CLAUDE.md "Commands vs inbound
///     events") → emit `OrderPlaced` (priced line items + `PaymentBreakdown`, ADR-0016/0017, binding
///     `customerId` onto the order) on `Order-<orderId>` and `CartCheckedOut` on `Cart-<cartId>`;
///   * `events.yaml#/PaymentFailed` (INBOUND) → abort: no OrderPlaced, the cart stays OPEN.
///   They need the process-manager runtime + the Stripe webhook ACL (`crates/infrastructure`).
pub async fn place_order(
    store: &dyn EventStore,
    carts: &dyn CartReadRepository,
    payments: &dyn PaymentGateway,
    cmd: PlaceOrder,
    actor: &Actor,
) -> Result<CreatedPaymentIntent, DomainError> {
    // The restaurant must exist, be ACTIVE and not PAUSED — folded from ITS stream (authoritative,
    // race-free; the saga may read other aggregates' streams through the same EventStore port).
    let (restaurant_events, _) = store.load(&restaurant_stream(&cmd.restaurant_id)).await?;
    let Some(restaurant) = domain::restaurant::fold(&restaurant_events) else {
        return Err(reject("RestaurantNotFound", format!("restaurantId={}", cmd.restaurant_id.0)));
    };
    if restaurant.status != RestaurantStatus::ACTIVE {
        return Err(reject(
            "RestaurantNotActive",
            format!("restaurantId={} restaurantName={}", cmd.restaurant_id.0, restaurant.display_name.0),
        ));
    }
    if restaurant.order_acceptance == OrderAcceptanceMode::PAUSED {
        return Err(reject(
            "RestaurantPaused",
            format!("restaurantId={} restaurantName={}", cmd.restaurant_id.0, restaurant.display_name.0),
        ));
    }
    // Test-mode isolation (ADR-0038, rules.yaml#/OrderTestModeIsolation): a LIVE order (mode absent =
    // LIVE) never reaches a TEST restaurant; a TEST order MAY target a LIVE restaurant.
    let restaurant_is_test = restaurant_events
        .iter()
        .any(|e| matches!(e, DomainEvent::RestaurantRegistered(r) if r.mode == Some(Mode::TEST)));
    if restaurant_is_test && cmd.mode != Some(Mode::TEST) {
        return Err(reject(
            "CannotOrderTestRestaurant",
            format!("restaurantId={}", cmd.restaurant_id.0),
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
            format!("cartId={} restaurantId={}", cmd.cart_id.0, cmd.restaurant_id.0),
        ));
    }
    if cart.line_ids.is_empty() {
        return Err(reject("CartEmpty", format!("cartId={}", cmd.cart_id.0)));
    }
    // DELIVERY requires an address.
    if cmd.service_type == ServiceType::DELIVERY && cmd.delivery_address.is_none() {
        return Err(reject("DeliveryAddressRequired", "serviceType=DELIVERY without deliveryAddress"));
    }
    // TODO(invariant): OutsideDeliveryArea — needs a delivery-area policy port (the restaurant's
    //                  delivery zone is not modelled in any read port yet).
    // TODO(invariant): OfferUnavailable / InsufficientStock / InvalidOptionSelection — re-validating
    //                  each cart line against the LIVE catalog needs an offer-level Catalog read port
    //                  (same gap as add_cart_line).
    // Price the cart server-side: the projected Cart total (never trust client prices).
    // TODO(runtime): the Cart projector does not price lines yet (total stays 0 until the
    //                catalog+policy pricing lands) — the seam is in place so this handler is unchanged.
    let cart_row = carts.by_id(cmd.cart_id).await?.ok_or_else(|| {
        DomainError::Repository(format!("cart projection not yet available for cart {}", cmd.cart_id.0))
    })?;
    let amount = Money { amount_cents: cart_row.total_amount_cents, currency: cart_row.currency };
    // Create the Stripe PaymentIntent through the gateway seam; a synchronous decline surfaces as the
    // canonical `PaymentDeclined` rejection (see the PaymentGateway contract).
    let intent = payments.create_payment_intent(&amount, &cmd.payment_method_id).await?;
    // Record the saga's first fact on the order's stream (client-generated orderId ⇒ replaying the
    // checkout for the same order id is absorbed instead of duplicating the fact).
    let event = DomainEvent::PaymentIntentCreated(PaymentIntentCreated {
        payment_intent_id: intent.payment_intent_id.clone(),
        restaurant_id: cmd.restaurant_id,
        customer_id: cmd.customer_id,
        amount,
    });
    idempotent_on_existing(store.append(&order_stream(&cmd.order_id), 0, &[event], actor).await)?;
    Ok(intent)
}
