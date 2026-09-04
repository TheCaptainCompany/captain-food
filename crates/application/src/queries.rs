//! Query-side use-case ports (the read side, ADR-0035). Resolvers/handlers depend on these traits;
//! concrete adapters live in `infrastructure` and are injected at the `server` composition root. Read
//! ports return the generated `…Row` DTOs (what the projector writes and the query side returns).

use async_trait::async_trait;

use domain::generated::entities::{Money, OptionList, Product};
use domain::generated::scalars::{
    CartId, CatalogItemAvailability, CityAvailabilityStatus, CityId, CuisineCategory, CurrencyCode,
    CustomerId, DeliveryChannelKey, DeliveryDissatisfactionReason, DeliveryIssueKind, DeliveryJobId, FoodCustody, DeliveryPartnerName,
    DeliveryPartnerRegistrationId, DeliveryProvider, DeliveryStatus, DeliveryTimeliness, EmailAddress,
    ExternalReference, OfferId, OfferName,
    MoneyCents, OptionId, OptionListId, OptionName, OrderId, OrderStatus, PhoneNumber, ProductId,
    ProductName, ProspectPipelineStatus, Quantity, ReclamationCategory, ReclamationDescription,
    ReclamationId, ReclamationReason, ReclamationResolution, ReclamationStatus, RefundId,
    AuthSubject, CatalogId, PrincipalKind, RefundStatus, RestaurantAccountId, RestaurantId, RiderId,
    RiderStanding, ScopeType, SessionId, Slug,
    StockStatus, UserType,
};
use domain::shared::errors::DomainError;

pub use crate::generated::rows::CartRow;
pub use crate::generated::rows::CatalogRow;
pub use crate::generated::rows::CustomerCreditBalanceRow;
/// Superseded storefront labels (ADR-20260728-011344). Read by `hosts.rs` for the 301, not by any
/// GraphQL query -- see `projectors::slug_alias`.
pub use crate::generated::rows::SlugAliasRow;
pub use crate::generated::rows::CustomerRow;
pub use crate::generated::rows::OrderConversationRow;
pub use crate::generated::rows::OrderTrackingRow;
pub use crate::generated::rows::ProspectionPipelineRow;
pub use crate::generated::rows::RestaurantRow;
/// The rider identity read model (#639 part A, ADR-20260818-004646): the `auth_ref -> rider_id`
/// bridge, read by the request seam rather than by any GraphQL query — see `projectors::rider`.
/// Written and not yet read: the resolver lands with the rider sign-in door.
pub use crate::generated::rows::RiderRow;
/// The restriction attribution read model (#639 part C step 4-i, ADR-20260904-081527 §2) — the
/// source of `myStanding`.
pub use crate::generated::rows::RiderRestrictionRow;

/// Optional filters for public restaurant discovery — mirrors the `restaurants` query args in api.yaml.
/// V0 applies a subset (the rest are accepted and ignored until the read model backs them).
#[derive(Debug, Clone, Default)]
pub struct RestaurantFilter {
    pub search: Option<String>,
    pub orderable_only: Option<bool>,
    /// Requested page size (#113); the adapter CLAMPS to [`RESTAURANT_PAGE_MAX`] and defaults to
    /// [`RESTAURANT_PAGE_DEFAULT`] when absent.
    pub limit: Option<i64>,
    /// Rows to skip (#113); `None`/negative → 0.
    pub offset: Option<i64>,
}

/// Default discovery page size when `limit` is unset — a first screen of cards.
pub const RESTAURANT_PAGE_DEFAULT: i64 = 24;
/// Hard ceiling on `restaurants` page size (#113, the #108 OOM guard as a named max): a larger
/// `limit` is clamped to this, never an error, never an unbounded scan.
pub const RESTAURANT_PAGE_MAX: i64 = 200;

/// Read port over the `Restaurant` projection table (ADR-0040). Backs the `restaurants`/`restaurant`
/// GraphQL queries.
/// Write-side arbiter of storefront-slug uniqueness (ADR-20260728-011344 D3).
///
/// NOT a read model. `Restaurant.slug` on the projection is eventually consistent, so two concurrent
/// claims could both pass a projection lookup and only diverge once the projector caught up — by which
/// point both owners were told "yes". This port is backed by a table with a real `UNIQUE` constraint,
/// so the DATABASE decides, once, atomically.
///
/// It also remembers RELEASED labels: renaming frees a restaurant's old address for *redirect*
/// purposes but must never free it for *reuse*, or a competitor could claim it and inherit the 301.
#[async_trait]
pub trait SlugReservationRepository: Send + Sync {
    /// Claim `slug` for `restaurant_id`.
    ///
    /// `Ok(true)` = reserved (or already held by this same restaurant — an idempotent replay).
    /// `Ok(false)` = held by another restaurant, or released by one and therefore still off-limits.
    /// The caller maps `false` to `SlugAlreadyTaken`.
    async fn reserve(&self, slug: Slug, restaurant_id: RestaurantId) -> Result<bool, DomainError>;

    /// Mark `slug` as released by `restaurant_id` on a rename: the row stays (so nobody else may take
    /// it) but stops being the restaurant's current address.
    async fn release(&self, slug: Slug, restaurant_id: RestaurantId) -> Result<(), DomainError>;
}

/// The principal a login credential is being BOUND to (#639 part C step 2a, #794). Each arm carries
/// its own kind, so the `(principal_kind, principal_id)` pair the reservation stores can never
/// disagree -- a `RiderId` under `CUSTOMER` is unspellable (ADR-20260803-234035: the type system
/// before a check). One arm today; the other doors add theirs as they land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundPrincipal {
    Rider(RiderId),
}

impl BoundPrincipal {
    /// The `principal_kind` half of the reservation key.
    pub fn kind(&self) -> PrincipalKind {
        match self {
            BoundPrincipal::Rider(_) => PrincipalKind::RIDER,
        }
    }

    /// The domain id bound to the credential -- typed by [`Self::kind`], never the auth subject.
    pub fn id(&self) -> uuid::Uuid {
        match self {
            BoundPrincipal::Rider(id) => id.0,
        }
    }
}

/// The write-side arbiter of "one login credential, one principal of each kind"
/// (`database/tables/reservations.yaml#/auth_subject_reservations`, #794). Keyed by
/// `(principal_kind, auth_subject)` -- NEVER the subject alone: a rider who is also a customer holds
/// two bindings, and a subject-only key would bar a rider from ever becoming a restaurant member.
///
/// There is deliberately NO `release`: revoking a rider must not free the binding, or a later
/// registration would bind the same human to a NEW rider id and orphan their history. That absence
/// is enforced by this trait's shape, not by a check.
#[async_trait]
pub trait AuthSubjectReservationRepository: Send + Sync {
    /// Bind `subject` to `principal` under `principal.kind()`.
    ///
    /// `Ok(true)` = bound (or already bound to this same principal id -- an idempotent replay).
    /// `Ok(false)` = bound to a DIFFERENT principal of that kind. The caller maps `false` to the
    /// population's typed error (`RiderAuthSubjectAlreadyBound` for riders).
    async fn reserve(&self, subject: AuthSubject, principal: BoundPrincipal) -> Result<bool, DomainError>;
}

#[async_trait]
pub trait RestaurantReadRepository: Send + Sync {
    /// Discovery list (public), newest-first, honouring the filter.
    async fn list(&self, filter: RestaurantFilter) -> Result<Vec<RestaurantRow>, DomainError>;
    /// A single restaurant by its slug (the per-restaurant storefront), or `None` if absent.
    async fn by_slug(&self, slug: Slug) -> Result<Option<RestaurantRow>, DomainError>;

    /// The restaurant that USED to hold `previous_slug`, for host resolution after a rename
    /// (ADR-20260728-011344). Returns the restaurant row, so the caller reads its CURRENT slug and
    /// redirects there in one hop — rather than walking the `SlugAlias.current_slug` chain, which after
    /// A→B→C still records B on row A.
    ///
    /// Provided: `Ok(None)` — a store with no alias knowledge simply never redirects. The Pg adapter
    /// overrides it with the `slugalias` join.
    async fn by_previous_slug(&self, _previous_slug: Slug) -> Result<Option<RestaurantRow>, DomainError> {
        Ok(None)
    }
    /// A single restaurant by id — the FK-navigation join other read slices hydrate from.
    async fn by_id(&self, id: RestaurantId) -> Result<Option<RestaurantRow>, DomainError>;

    /// All restaurant locations under an account (back-office; api.yaml `restaurantLocationsByAccount`).
    /// Provided: filters [`Self::list`] in memory; the Pg adapter overrides with an SQL predicate over
    /// the `restaurant_account_id` column.
    async fn by_account(
        &self,
        account_id: RestaurantAccountId,
    ) -> Result<Vec<RestaurantRow>, DomainError> {
        Ok(self
            .list(RestaurantFilter::default())
            .await?
            .into_iter()
            .filter(|r| r.restaurant_account_id.as_ref() == Some(&account_id))
            .collect())
    }
}

/// One selectable option with its LIVE name and price — checkout prices each `SelectedOption` from
/// this (rules.yaml#/ServerPriceAuthority: option prices are read from the live catalog, never from
/// the client).
#[derive(Debug, Clone, PartialEq)]
pub struct OfferOptionView {
    pub id: OptionId,
    pub name: OptionName,
    pub price: Money,
}

/// One option list (modifier group) as the Cart line checks need it: the selection bounds plus the
/// member option ids — enough to prove `selectedOptionIds` ⊆ the offer's lists and within min/max
/// (`errors.yaml#/InvalidOptionSelection`) — and the priced options checkout resolves
/// `selectedOptionIds` against.
#[derive(Debug, Clone, PartialEq)]
pub struct OfferOptionListView {
    pub id: OptionListId,
    /// Minimum number of selections the customer must make from this list.
    pub min_selections: i64,
    /// Maximum number of selections (`None` = unbounded).
    pub max_selections: Option<i64>,
    /// Whether the SAME option may be selected more than once.
    pub multiple_selection: bool,
    /// The options belonging to this list.
    pub option_ids: Vec<OptionId>,
    /// The same options with their live name and price (what checkout prices selections from).
    pub options: Vec<OfferOptionView>,
}

/// Offer-level slice of the projected `Catalog.tree` — what the Cart write side validates a line
/// against (rules.yaml#/CartRejectsUnorderableOrInvalidLine): availability (manual flag), the DERIVED
/// stock status + tracked quantity (availability ≠ stock — two orthogonal concepts), the live price
/// (never trusted from the client) and the offer's option-list constraints.
#[derive(Debug, Clone, PartialEq)]
pub struct OfferView {
    pub offer_id: OfferId,
    pub product_id: ProductId,
    /// The owning product's name — the `{productName}` parameter of the errors.yaml messages.
    pub product_name: ProductName,
    pub offer_name: OfferName,
    /// The live catalog price (the projection prices carts from this, never from the client).
    pub price: Money,
    /// Manual UI flag (`errors.yaml#/OfferUnavailable` when UNAVAILABLE).
    pub availability: CatalogItemAvailability,
    /// DERIVED from quantity vs lowStockThreshold (scalars.yaml#/StockStatus).
    pub stock_status: StockStatus,
    /// The tracked stock quantity, or `None` when the offer does not track stock (never blocks).
    pub stock_quantity: Option<Quantity>,
    /// The option lists attached to this offer (resolved from the tree's `optionLists` section).
    pub option_lists: Vec<OfferOptionListView>,
}

/// Resolve one offer out of a projected `Catalog.tree` jsonb (camelCase, as written by the
/// `CatalogProjector` fold): walk `products[].offers[]` for the id, re-derive `stock_status` from the
/// node's `stock`, and hydrate the offer's option lists from the `optionLists` section. `None` when
/// the offer is not in the tree (`errors.yaml#/OfferNotFound`).
pub fn offer_view_from_tree(tree: &serde_json::Value, offer_id: OfferId) -> Option<OfferView> {
    let products = tree.get("products").and_then(|v| v.as_array())?;
    for product_node in products {
        let Ok(product) = serde_json::from_value::<Product>(product_node.clone()) else {
            continue; // a malformed node never panics the write side — the offer just isn't found
        };
        let Some(offer) = product.offers.iter().find(|o| o.id == offer_id) else { continue };
        let option_lists = tree
            .get("optionLists")
            .and_then(|v| v.as_array())
            .map(|lists| {
                lists
                    .iter()
                    .filter_map(|node| serde_json::from_value::<OptionList>(node.clone()).ok())
                    .filter(|list| offer.option_list_ids.contains(&list.id))
                    .map(|list| OfferOptionListView {
                        id: list.id,
                        min_selections: list.min_selections,
                        max_selections: list.max_selections,
                        multiple_selection: list.multiple_selection,
                        option_ids: list.options.iter().map(|o| o.id).collect(),
                        options: list
                            .options
                            .iter()
                            .map(|o| OfferOptionView {
                                id: o.id,
                                name: o.name.clone(),
                                price: o.price.clone(),
                            })
                            .collect(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        return Some(OfferView {
            offer_id,
            product_id: product.id,
            product_name: product.name.clone(),
            offer_name: offer.name.clone(),
            price: offer.price.clone(),
            availability: offer.availability,
            stock_status: crate::projectors::catalog::derive_stock_status(offer.stock.as_ref()),
            stock_quantity: offer.stock.as_ref().map(|s| s.quantity),
            option_lists,
        });
    }
    None
}

/// Read port over the `Catalog` projection table (ADR-0040). Backs the public `catalog` and
/// `categories` GraphQL queries (`categories` derives from the same row's `tree`) plus the Cart
/// write side's offer-level line checks ([`Self::offer_by_id`]).
#[async_trait]
pub trait CatalogReadRepository: Send + Sync {
    /// A restaurant's catalog (newest first when several exist), or `None` before CatalogCreated.
    async fn by_restaurant(&self, restaurant_id: RestaurantId) -> Result<Option<CatalogRow>, DomainError>;

    /// Whether ANOTHER catalog of this restaurant already uses `slug` -- the per-restaurant
    /// uniqueness behind `CatalogSlugAlreadyTaken`. A catalog slug is a PATH inside one storefront,
    /// not a global host, so it is checked against the read model rather than reserved in Postgres
    /// the way the restaurant host is. Provided: derived from [`Self::by_restaurant`]; override for a
    /// store that holds several catalogs per restaurant.
    async fn slug_taken(
        &self,
        restaurant_id: RestaurantId,
        slug: &Slug,
        excluding: CatalogId,
    ) -> Result<bool, DomainError> {
        Ok(self
            .by_restaurant(restaurant_id)
            .await?
            .filter(|c| c.catalog_id != excluding)
            .and_then(|c| c.slug)
            .as_ref()
            == Some(slug))
    }

    /// One offer of the restaurant's live catalog, or `None` when the restaurant has no catalog or
    /// the offer is not in it. Provided: every adapter (Pg included) reads the projected `tree` via
    /// [`Self::by_restaurant`] + [`offer_view_from_tree`]; override only for a normalized store.
    async fn offer_by_id(
        &self,
        restaurant_id: RestaurantId,
        offer_id: OfferId,
    ) -> Result<Option<OfferView>, DomainError> {
        Ok(self
            .by_restaurant(restaurant_id)
            .await?
            .and_then(|row| offer_view_from_tree(&row.tree, offer_id)))
    }
}

/// Read port over the `Cart` projection table (ADR-0040). Backs the `carts`/`cart` GraphQL queries
/// plus CartBindingProcess's session read (`specs/processmanager.yaml#/CartBindingProcess`).
#[async_trait]
pub trait CartReadRepository: Send + Sync {
    /// A customer's **OPEN** carts (one per restaurant), most recently updated first, bounded.
    ///
    /// OPEN-only is part of the PORT contract, not an adapter detail (#451): every consumer prices
    /// what this returns against the LIVE catalog, and a CHECKED_OUT cart's money was frozen at
    /// payment intent — repricing it would show a number that never matched what was charged.
    /// Implementations must filter, and must bound the row count (the caller pays one catalog read
    /// per row).
    async fn by_customer(&self, customer_id: CustomerId) -> Result<Vec<CartRow>, DomainError>;
    /// A single **OPEN** cart by id, or `None` when absent OR no longer open.
    ///
    /// OPEN-only for the same reason as [`Self::by_customer`] and stated here for the same reason:
    /// it is a PORT obligation, so both lookups give the same answer to "may this be repriced?".
    /// A CHECKED_OUT cart resolving `None` is correct, not a gap — post-checkout money is read from
    /// the Order (the aggregate that owns what was charged), never re-derived from a cart.
    async fn by_id(&self, id: CartId) -> Result<Option<CartRow>, DomainError>;

    /// The session's OPEN carts, most recently updated first — CartBindingProcess's `read` step
    /// (`where: { session_id, status: OPEN }`) AND leg 2 of `cart.current` (ADR-20260810-120531).
    ///
    /// REQUIRED, deliberately (#451 Phase 2b). This used to be a PROVIDED method returning
    /// `Ok(Vec::new())` so in-flight fakes kept compiling. That default is a trap once `current`
    /// depends on it: an implementation that simply forgets to override serves NO carts, so the
    /// entire ANONYMOUS cart path — every guest mini-cart, the whole pre-identification flow —
    /// silently resolves empty, with nothing to compile-fail and no error to observe. Making it
    /// required turns "I forgot" into a build failure (compiler-first, ADR-20260803-234035).
    async fn open_by_session(&self, session_id: SessionId) -> Result<Vec<CartRow>, DomainError>;

    /// The customer's OPEN carts **at ONE restaurant**, most recently updated first — leg 1 of
    /// `cart.current` (#469).
    ///
    /// A SEPARATE method rather than an optional filter on [`Self::by_customer`], deliberately: the
    /// tenant is not optional here. `current` is served on a storefront host and must answer with
    /// THAT restaurant's cart or with nothing — a customer with an open cart at `b.captain.food`
    /// seeing it, being priced for it and paying for it on `a.captain.food` is precisely what this
    /// signature makes unspellable. `by_customer` stays unbounded because the query IT serves
    /// (`carts`, "my carts") legitimately spans restaurants; the distinction lives in the type
    /// rather than in a caller remembering to pass `Some(id)`.
    ///
    /// Implementations MUST filter on the restaurant in the STORE, not in the caller: a Rust-side
    /// filter over an unbounded read would let unfiltered SQL ship under a passing fake.
    async fn open_by_customer_at(
        &self,
        customer_id: CustomerId,
        restaurant_id: RestaurantId,
    ) -> Result<Vec<CartRow>, DomainError>;

    /// The session's OPEN carts **at ONE restaurant**, most recently updated first — leg 2 of
    /// `cart.current` (#469). Same store-side tenant obligation as [`Self::open_by_customer_at`].
    ///
    /// The anonymous leg is not cross-tenant REACHABLE on the web (the session id lives in
    /// per-origin `localStorage`, so two tenant hosts never share one), and it is scoped anyway:
    /// the scoping of a read must not rest on a client-side storage detail staying true, and the
    /// native app — one session id across restaurants — is the case where it does not.
    /// [`Self::open_by_session`] stays unscoped for CartBindingProcess, which binds every cart of a
    /// session at identification and is right to span restaurants.
    async fn open_by_session_at(
        &self,
        session_id: SessionId,
        restaurant_id: RestaurantId,
    ) -> Result<Vec<CartRow>, DomainError>;
}

/// Read port over the `Customer` projection table (ADR-0040) — the identity/lookup read model. Backs
/// the write-side uniqueness/resolution invariants of the Customer aggregate (VerifyPhone
/// register-vs-identify, `PhoneAlreadyInUse`, `EmailAlreadyInUse`) plus the `me` (session authRef →
/// Customer) and `favoriteRestaurants` GraphQL queries.
#[async_trait]
pub trait CustomerReadRepository: Send + Sync {
    /// The customer owning this canonical E.164 phone (the primary identifier), or `None`.
    async fn by_phone(&self, phone: PhoneNumber) -> Result<Option<CustomerRow>, DomainError>;
    /// The customer whose verified email this is, or `None`.
    async fn by_email(&self, email: EmailAddress) -> Result<Option<CustomerRow>, DomainError>;
    /// A single customer by id — backs `favoriteRestaurants` (and profile lookups by id).
    async fn by_id(&self, id: CustomerId) -> Result<Option<CustomerRow>, DomainError>;
    /// The customer linked to this auth-provider user reference (Supabase `sub`, ADR-0015) — how the
    /// `me` query resolves the verified session identity to its Customer row.
    async fn by_auth_ref(&self, auth_ref: ExternalReference) -> Result<Option<CustomerRow>, DomainError>;
}

/// Read port over the `Rider` projection's identity bridge (#639 part C step 2b,
/// ADR-20260818-004646): the verified auth subject → `rider_id`, read by the request seam
/// (`resolve_read_scope`) once per GraphQL request and once per WS connect — never by a GraphQL
/// query (the table is `internal: true`, and no api.yaml type can speak for this reader).
///
/// Deliberately its OWN port beside [`CustomerReadRepository::by_auth_ref`], typed `AuthSubject`
/// from birth: the customer port still takes `ExternalReference` (the HubRise catalog-ref scalar)
/// and step 1b (#836) retypes it; this one is born with the right scalar so #836 unifies two
/// `AuthSubject` ports instead of retyping a third site.
///
/// It answers WHO this connection is and, since #639 part C step 4-i (ADR-20260904-081527 §1),
/// WHETHER the platform grants it anything — `standing` is a GRANT TEST ONLY, never the arbiter of
/// an append (the write side keeps its own authority folding the `Rider-{id}` stream) — and
/// nothing else: still never `status` (availability is not an authorization signal; the LEGACY
/// `SUSPENDED` value must never be read here).
#[async_trait]
pub trait RiderIdentityRepository: Send + Sync {
    /// The rider bound to this verified auth subject with its current standing, or `None` when no
    /// `RiderRegistered` has been projected for it. ONE read (`SELECT rider_id, standing`, never
    /// two). The adapter must never `LIMIT 1`: picking a row is an elevation decision made by row
    /// order, and `rider.auth_ref UNIQUE` is what makes a bare `fetch_optional` honest.
    async fn rider_id_by_auth_subject(
        &self,
        auth_subject: AuthSubject,
    ) -> Result<Option<(RiderId, RiderStanding)>, DomainError>;
}

/// Read port over the `RiderRestriction` attribution table (#639 part C step 4-i,
/// ADR-20260904-081527 §2/§4) — the source of `myStanding`.
#[async_trait]
pub trait RiderRestrictionReadRepository: Send + Sync {
    async fn by_rider_id(&self, rider_id: RiderId) -> Result<Option<RiderRestrictionRow>, DomainError>;
}

/// Optional filters for the order list — mirrors the `orders` query args in api.yaml
/// (`customerId` / `restaurantId` / `status`); ownership/scope is enforced server-side.
#[derive(Debug, Clone, Default)]
pub struct OrderFilter {
    pub customer_id: Option<CustomerId>,
    pub restaurant_id: Option<RestaurantId>,
    pub status: Option<OrderStatus>,
}

/// Read port over the `OrderTracking` projection table (ADR-0040). Backs the `orders`/`order`
/// GraphQL queries — the single canonical Order read model (history + back-office queue + tracking).
#[async_trait]
pub trait OrderReadRepository: Send + Sync {
    /// Orders honouring the filter, most recently placed first, **restricted to `scope`** (#144).
    ///
    /// FILTERED, not checked: rows outside the scope are absent, not denied — so a list cannot leak
    /// existence and cannot "forget" to be scoped. The `scope` parameter is mandatory precisely so
    /// that an unscoped list is not expressible; before #144 it was, and `orders` with no arguments
    /// returned the entire tracking table to any authenticated customer.
    async fn list(
        &self,
        filter: OrderFilter,
        scope: &ReadScope,
    ) -> Result<Vec<OrderTrackingRow>, DomainError>;

    /// A single order by id, or `None` — **including when it exists but is outside `scope`** (#144).
    ///
    /// Returning `None` rather than a distinct "forbidden" keeps the read side free of an existence
    /// oracle; the by-id GraphQL surface renders it as "not found". The `/files` route is the case
    /// that deliberately differs (403, to preserve the probing signal) — see PROP-20260725-185140.
    async fn by_id(
        &self,
        id: OrderId,
        scope: &ReadScope,
    ) -> Result<Option<OrderTrackingRow>, DomainError>;
}

/// Read port over the `OrderConversation` projection table (ADR-0040; #131, epic #129). Backs the
/// `orderConversation` (PUBLIC thread) and `orderConversationInternalNotes` (staff-only) GraphQL
/// queries — both read the one per-order conversation row (the visibility split is a column split).
#[async_trait]
pub trait OrderConversationReadRepository: Send + Sync {
    /// The conversation for one order, or `None` before it has been opened.
    async fn by_order(&self, order_id: OrderId) -> Result<Option<OrderConversationRow>, DomainError>;
}

/// One `View_DeliveryJob` row (ADR-0031/0039) — hand-written: this read model is a SQL VIEW
/// (projection-on-read over `domain_events`), not a materialized projection table, so no `…Row` is
/// generated for it (`generated/rows.rs` covers `tables/projection_tables.yaml` only). Field order and
/// types mirror the view's columns: enum columns come back as INTEGER ordinals (ADR-0037), addresses
/// and the courier as jsonb.
#[derive(Debug, Clone, PartialEq)]
pub struct DeliveryJobRow {
    pub delivery_job_id: DeliveryJobId,
    pub order_id: OrderId,
    pub restaurant_id: RestaurantId,
    pub status: DeliveryStatus,
    /// INDEPENDENT (rider accepted) or PARTNER (partner accepted); `None` while PENDING.
    pub provider: Option<DeliveryProvider>,
    /// Set for an independent-rider delivery; `None` for a partner delivery.
    pub rider_id: Option<RiderId>,
    /// Courier `{ displayName, phone?, riderId? }` jsonb; from the partner on acceptance.
    pub courier: Option<serde_json::Value>,
    /// Partner-side delivery id; idempotent key for inbound updates.
    pub partner_ref: Option<ExternalReference>,
    pub pickup_address: serde_json::Value,
    pub dropoff_address: serde_json::Value,
    pub estimated_pickup_at: Option<chrono::DateTime<chrono::Utc>>,
    pub estimated_dropoff_at: Option<chrono::DateTime<chrono::Utc>>,
    pub requested_at: chrono::DateTime<chrono::Utc>,
    pub picked_up_at: Option<chrono::DateTime<chrono::Utc>>,
    pub delivered_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The kind of the OPEN delivery issue (#639 part C step 3-i): set by
    /// `DeliveryIssueReported.kind`, cleared by `DeliveryIssueResolved`; `None` when nothing is
    /// open — the column through which the restaurant is told.
    pub open_issue_kind: Option<DeliveryIssueKind>,
    /// Where the food is, set by a handback (#639 part C step 3-ii); reset to `None` by the next
    /// acceptance — the board's pinned card headline and the customer tracking banner's predicate.
    pub food_location: Option<FoodCustody>,
    /// Occurrence time of the latest handback (envelope `occurredAt`); `None` until one occurs.
    pub handed_back_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Read port over the `View_DeliveryJob` read model (ADR-0031/0039). Backs the `delivery` /
/// `myDeliveries` / `restaurantDeliveries` GraphQL queries — order tracking, the rider job list and
/// the restaurant delivery board.
#[async_trait]
pub trait DeliveryReadRepository: Send + Sync {
    /// The delivery job of an order (tracking), or `None` before dispatch / for a COLLECTION order.
    /// A re-dispatched order keeps one live job per DeliveryRequested; the latest wins.
    async fn by_order(&self, order_id: OrderId) -> Result<Option<DeliveryJobRow>, DomainError>;
    /// The independent rider's job list (rider app): jobs assigned to them PLUS the available pool
    /// (PENDING, unassigned), honouring the optional status filter, newest first.
    async fn for_rider(
        &self,
        rider_id: RiderId,
        status: Option<DeliveryStatus>,
    ) -> Result<Vec<DeliveryJobRow>, DomainError>;
    /// A restaurant's delivery board, honouring the optional status filter, newest first.
    async fn by_restaurant(
        &self,
        restaurant_id: RestaurantId,
        status: Option<DeliveryStatus>,
    ) -> Result<Vec<DeliveryJobRow>, DomainError>;
}

/// One `View_PendingRefunds` fold-view row (the refund queue, ADR-0039) — hand-written: view-backed
/// read models get no generated row (`generated/rows.rs` covers `tables/projection_tables.yaml`
/// only). Field order and types mirror the view's columns: `status` comes back as its INTEGER
/// ordinal (ADR-0037); the Money value object splits into `amount_cents` + `currency`.
#[derive(Debug, Clone, PartialEq)]
pub struct RefundRow {
    pub order_id: OrderId,
    pub restaurant_id: RestaurantId,
    /// REQUESTED (awaiting decision) → APPROVED / DENIED → REFUNDED (Stripe settled).
    pub status: RefundStatus,
    /// The captured order total eligible for refund (RefundOpened.amount).
    pub amount_cents: domain::generated::scalars::MoneyCents,
    pub currency: CurrencyCode,
    /// The (possibly partial) approved amount; `None` until approved.
    pub approved_amount_cents: Option<domain::generated::scalars::MoneyCents>,
    /// The latest recorded reason (the opening fact's, then the decision's).
    pub reason: Option<String>,
    /// The Stripe Refund id once settled; `None` before PaymentRefunded.
    pub refund_id: Option<RefundId>,
    pub requested_at: chrono::DateTime<chrono::Utc>,
    /// The decision's occurrence time; `None` while REQUESTED.
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Optional filters for the refund queue — mirrors the `pendingRefunds` query args in api.yaml
/// (`restaurantId` / `status`; status REQUESTED = the pending, awaiting-decision queue).
#[derive(Debug, Clone, Default)]
pub struct RefundFilter {
    pub restaurant_id: Option<RestaurantId>,
    pub status: Option<RefundStatus>,
}

/// Read port over the `View_PendingRefunds` read model (the RefundProcess refund queue). Backs the
/// `pendingRefunds` GraphQL query for the restaurant (own orders) and the arbitrating admin.
#[async_trait]
pub trait RefundReadRepository: Send + Sync {
    /// The refund queue, newest-request-first, honouring the filter.
    async fn list(&self, filter: RefundFilter) -> Result<Vec<RefundRow>, DomainError>;
}

/// One customer's delivery-delay satisfaction answer for an order (#62) — a row of the
/// `View_DeliverySatisfaction` fold view (`DeliverySatisfactionRecorded` on the Order stream).
pub struct DeliverySatisfactionRow {
    pub order_id: OrderId,
    pub restaurant_id: RestaurantId,
    /// The customer's timeliness verdict (ON_TIME / ACCEPTABLE_DELAY / TOO_LATE).
    pub timeliness: DeliveryTimeliness,
    /// The optional reason given for a TOO_LATE verdict; `None` otherwise.
    pub reason: Option<DeliveryDissatisfactionReason>,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
}

/// Read port over the `View_DeliverySatisfaction` read model (#62). Backs the
/// `restaurantDeliverySatisfaction` GraphQL query — the restaurant's timeliness insight
/// (the self-dispatch-vs-Captain signal), scoped to one restaurant and optionally one verdict.
#[async_trait]
pub trait DeliverySatisfactionReadRepository: Send + Sync {
    /// The restaurant's delivery-satisfaction answers, newest-first; filtered to one `timeliness`
    /// verdict when given.
    async fn by_restaurant(
        &self,
        restaurant_id: RestaurantId,
        timeliness: Option<DeliveryTimeliness>,
    ) -> Result<Vec<DeliverySatisfactionRow>, DomainError>;
}

/// One `View_DeliveryPartnerAvailability` fold-view row (delivery partner self-registration, #61 —
/// ADR-0039). Hand-written (view-backed read models get no generated row): field order/types mirror the
/// view's columns; `status` comes back as its INTEGER ordinal (ADR-0037); set-once identity is carried
/// by the Requested birth fact, `decided_at` is null while PENDING.
#[derive(Debug, Clone, PartialEq)]
pub struct DeliveryPartnerAvailabilityRow {
    pub registration_id: DeliveryPartnerRegistrationId,
    pub channel: DeliveryChannelKey,
    pub city_id: CityId,
    pub partner_name: DeliveryPartnerName,
    pub contact_email: EmailAddress,
    /// PENDING (awaiting review) → APPROVED / REVOKED.
    pub status: CityAvailabilityStatus,
    pub requested_at: chrono::DateTime<chrono::Utc>,
    /// The decision's occurrence time; `None` while PENDING.
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Optional filters for the delivery-partner availability queue — mirrors the
/// `deliveryPartnerAvailabilities` query args in api.yaml (`cityId` / `channel` / `status`;
/// status PENDING = the admin review queue).
#[derive(Debug, Clone, Default)]
pub struct DeliveryPartnerAvailabilityFilter {
    pub city_id: Option<CityId>,
    pub channel: Option<DeliveryChannelKey>,
    pub status: Option<CityAvailabilityStatus>,
}

/// Read port over the `View_DeliveryPartnerAvailability` read model (delivery partner self-registration,
/// #61). Backs the EXTERNAL/admin `deliveryPartnerAvailabilities` GraphQL query.
#[async_trait]
pub trait DeliveryPartnerAvailabilityReadRepository: Send + Sync {
    /// The availability registrations, newest-request-first, honouring the filter.
    async fn list(
        &self,
        filter: DeliveryPartnerAvailabilityFilter,
    ) -> Result<Vec<DeliveryPartnerAvailabilityRow>, DomainError>;
}

/// One `View_Reclamation` fold-view row (customer claims/disputes, #154 — ADR-0039). Hand-written
/// (view-backed read models get no generated row): field order/types mirror the view's columns;
/// `status` (and the two nullable enum columns) come back as their INTEGER ordinal (ADR-0037). The
/// set-once identity is carried by the ReclamationOpened birth fact; the decision fields fill in on
/// the resolve/reject fact and are `None` while OPEN. The refund amount is the minor-units column +
/// row currency (both `None` unless a refund amount was recorded).
#[derive(Debug, Clone, PartialEq)]
pub struct ReclamationRow {
    pub reclamation_id: ReclamationId,
    pub order_id: OrderId,
    pub customer_id: CustomerId,
    pub restaurant_id: RestaurantId,
    pub category: ReclamationCategory,
    pub description: ReclamationDescription,
    /// The resolution the customer asked for at open time, if any.
    pub requested_resolution: Option<ReclamationResolution>,
    /// OPEN (awaiting a decision) → RESOLVED / REJECTED → OPEN again on reopen.
    pub status: ReclamationStatus,
    /// The decided resolution once resolved; `None` while OPEN or if rejected.
    pub resolution: Option<ReclamationResolution>,
    /// The PARTIAL_REFUND amount (minor units) + its currency; both `None` unless recorded.
    pub refund_amount_cents: Option<MoneyCents>,
    pub currency: Option<CurrencyCode>,
    /// The reason recorded on rejection; `None` unless rejected.
    pub reject_reason: Option<ReclamationReason>,
    pub opened_at: chrono::DateTime<chrono::Utc>,
    /// The decision's occurrence time; `None` while OPEN.
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
    /// First-response SLA flag (#160), computed AT READ TIME by the repo (never a stored/folded
    /// column): `status == OPEN && opened_at < now() − RECLAMATION_FIRST_RESPONSE_TARGET`. An OPEN
    /// claim not answered past the target is overdue so the staff queue surfaces it.
    pub overdue: bool,
}

/// First-response SLA target (#160): an OPEN claim whose `opened_at` is older than this is flagged
/// `overdue`. "First response" for V0 = the claim being decided (resolved/rejected), so overdue is
/// simply an OPEN claim older than the target. A single named constant — configurable (per-restaurant
/// / per-category, or a referential policy) later; there is NO domain clock (time is read-time here,
/// event-envelope `occurred_at` in the domain — #154).
pub const RECLAMATION_FIRST_RESPONSE_TARGET_HOURS: i64 = 24;

/// Optional filters for the restaurant claims queue — mirrors the `restaurantReclamations` query args
/// in api.yaml (`status` / `category`; status OPEN = the outstanding queue). `overdue: Some(true)`
/// narrows to OPEN claims past the first-response target (#160).
#[derive(Debug, Clone, Default)]
pub struct ReclamationFilter {
    pub status: Option<ReclamationStatus>,
    pub category: Option<ReclamationCategory>,
    /// When `Some(true)`, return only overdue claims (`status == OPEN && opened_at < now() − target`).
    pub overdue: Option<bool>,
}

/// Read port over the `View_Reclamation` read model (customer claims, #154). Backs the customer
/// `myReclamations` / `reclamation` reads and the restaurant/admin `restaurantReclamations` queue.
#[async_trait]
pub trait ReclamationReadRepository: Send + Sync {
    /// A customer's own reclamations, newest-first (the `myReclamations` list).
    async fn by_customer(
        &self,
        customer_id: CustomerId,
    ) -> Result<Vec<ReclamationRow>, DomainError>;

    /// The restaurant/admin claims queue, newest-first, honouring the status/category filter.
    async fn list(&self, filter: ReclamationFilter) -> Result<Vec<ReclamationRow>, DomainError>;

    /// A single reclamation by id (claim detail); `None` when unknown.
    async fn by_id(&self, id: ReclamationId) -> Result<Option<ReclamationRow>, DomainError>;
}

/// Read port over the `CustomerCreditBalance` projection table (ADR-0040; #158, Part B of #207). Backs
/// the `customerCredit` GraphQL query — the customer's spendable store-credit balance, scoped to the
/// caller's Customer identity (the me-pattern, like `myReclamations`).
#[async_trait]
pub trait CustomerCreditReadRepository: Send + Sync {
    /// A customer's store-credit balance row, or `None` when they have no ledger yet (no grant).
    async fn by_customer(
        &self,
        customer_id: CustomerId,
    ) -> Result<Option<CustomerCreditBalanceRow>, DomainError>;
}

/// Optional filters for the admin prospection pipeline — mirrors the `prospectionPipeline` query args
/// in api.yaml (`minScore` / `status`).
#[derive(Debug, Clone, Default)]
pub struct ProspectFilter {
    pub min_score: Option<i32>,
    pub status: Option<ProspectPipelineStatus>,
}

/// Read port over the `ProspectionPipeline` projection table (ADR-0020/0040). Backs the admin
/// `prospectionPipeline` GraphQL query.
#[async_trait]
pub trait ProspectionReadRepository: Send + Sync {
    /// Scored prospect list (admin), best-score-first, honouring the filter.
    async fn list(&self, filter: ProspectFilter) -> Result<Vec<ProspectionPipelineRow>, DomainError>;
}

/// One `pricingpolicy` referential row (ADR-0016/0017/0037) — hand-written: referential tables are
/// seeded configuration, not projections, so no `…Row` is generated for them.
#[derive(Debug, Clone)]
pub struct PricingPolicyRow {
    pub currency: CurrencyCode,
    pub fee_rate: f64,
    pub buyer_share: f64,
    pub margin_low: f64,
    pub margin_high: f64,
    pub effective_from: chrono::DateTime<chrono::Utc>,
}

/// Read port over the seeded `PricingPolicy` referential table. Backs the admin `pricingPolicy`
/// GraphQL query.
#[async_trait]
pub trait PricingPolicyReadRepository: Send + Sync {
    /// The active fee-policy rows (one per currency), stable order.
    async fn list(&self) -> Result<Vec<PricingPolicyRow>, DomainError>;
}

/// One `uberestimationpolicy` referential row (ADR-0024/0030/0037) — hand-written, like
/// [`PricingPolicyRow`].
#[derive(Debug, Clone)]
pub struct UberEstimationPolicyRow {
    pub cuisine_category: CuisineCategory,
    pub price_coefficient: f64,
    pub effective_from: chrono::DateTime<chrono::Utc>,
}

/// Read port over the seeded `UberEstimationPolicy` referential table. Backs the admin
/// `uberEstimationPolicy` GraphQL query.
#[async_trait]
pub trait UberEstimationPolicyReadRepository: Send + Sync {
    /// The per-cuisine mark-up coefficients (one per CuisineCategory), stable order.
    async fn list(&self) -> Result<Vec<UberEstimationPolicyRow>, DomainError>;
}

/// One `ubersplitpolicy` referential row (ADR-0024/0025/0037) — hand-written, like
/// [`PricingPolicyRow`].
#[derive(Debug, Clone)]
pub struct UberSplitPolicyRow {
    pub currency: CurrencyCode,
    pub uber_commission_pct: f64,
    pub rider_base_cents: i64,
    pub rider_per_km_cents: i64,
    pub avg_delivery_fee_cents: i64,
    pub platform_fee_pct: f64,
    pub effective_from: chrono::DateTime<chrono::Utc>,
}

/// Read port over the seeded `UberSplitPolicy` referential table. Backs the admin `uberSplitPolicy`
/// GraphQL query.
#[async_trait]
pub trait UberSplitPolicyReadRepository: Send + Sync {
    /// The active split/fee assumption rows (one per currency), stable order.
    async fn list(&self) -> Result<Vec<UberSplitPolicyRow>, DomainError>;
}

// The supervision READ port (`MailboxLaneRepository` + its two row types) moved to
// `actor_client::supervision` under #510: its methods demand the `MailboxAccess` witness only
// that crate can mint, so it has to live where the mint lives. The WRITE half below stays —
// its one legitimate caller is `crate::commands::requeue_mailbox_message`, in THIS crate.

/// Outcome of a [`MailboxRequeue::requeue_if_poisoned`] arbitration (#315) — what the database
/// said about the target row, decided and applied in ONE statement so no check-then-act window
/// exists between "is it poisoned" and "flip it".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequeueOutcome {
    /// The row WAS cap-poisoned and is now RECEIVED again (attempts reset, error + backoff
    /// schedule cleared, lane nudged); carries the lane's actor type for the audit fact.
    Requeued { actor_type: String },
    /// The row is ALREADY deliverable (RECEIVED) — a redelivered/raced requeue converges here:
    /// recorded as success, the row untouched.
    AlreadyDeliverable { actor_type: String },
    /// No row with this id (e.g. a stale supervision screen after retention swept it).
    NotFound,
    /// The row exists in a state that must never be requeued (a handler verdict, a scheduled
    /// row, an already-succeeded run) — `status` names it for the error context.
    NotRequeueable { status: String },
}

/// The capability WITNESS [`MailboxRequeue::requeue_if_poisoned`] demands (#510, the write half
/// of the #304 posture): a value only THIS crate can mint, because its single field is
/// `pub(crate)`. Holding the port — every `CommandDeps` bundle and GraphQL context does — is no
/// longer holding the door: the one production mint sits inside
/// `crate::commands::requeue_mailbox_message`, the handler the `RequeueMailboxMessage` command
/// reaches through the mailbox, so a resolver or worker holding the `Arc<dyn MailboxRequeue>`
/// cannot spell the flip. Implementors outside the crate (`infrastructure::PgMailboxRequeue`)
/// name the type in their signatures and ignore the value — naming a type is not constructing one.
///
/// Honest limit, same as the read witness's (`actor_client::mailbox::MailboxAccess`): any code
/// INSIDE `application` can mint. The crate is the permission boundary, not the function.
#[derive(Debug, Clone, Copy)]
pub struct MailboxRequeueAccess(pub(crate) ());

impl MailboxRequeueAccess {
    /// Mint the witness. `pub(crate)` IS the enforcement — every caller of the requeue port
    /// stands inside this crate, behind the command handler.
    pub(crate) fn granted() -> Self {
        Self(())
    }
}

/// The TEST-ONLY mint (#510, mirroring `actor_client::mailbox::fixtures::MailboxAccess::for_tests`):
/// an integration test asserting the port's arbitration matrix IS the thing behind the door, so it
/// needs one. Compiled only under `test-fixtures`, which only `[dev-dependencies]` may enable
/// (CI-guarded by `test_fixtures_feature_never_reaches_a_release_artifact`) — a door for tests,
/// never a door in a release artifact.
#[cfg(any(test, feature = "test-fixtures"))]
impl MailboxRequeueAccess {
    pub fn for_tests() -> Self {
        Self(())
    }
}

/// WRITE port over the poisoned-row recovery (#315, ADR-20260803-002712 Q1): the arbiter of
/// "only cap-poisoned rows are requeueable" (rules.yaml#/OnlyCapPoisonedMailboxRowsAreRequeueable).
/// Deliberately NOT part of the read repository: the flip must be a single atomic arbitration.
/// Like the slug-reservation port, the write happens alongside (not inside) the event append —
/// idempotent by construction, so a retried delivery of the requeue command converges.
#[async_trait]
pub trait MailboxRequeue: Send + Sync {
    /// Flip the row `FAILED → RECEIVED` iff it is cap-poisoned (error code
    /// `DeliveryInfrastructureError`); report what the database found otherwise. Never guesses:
    /// the predicate and the flip are one statement.
    async fn requeue_if_poisoned(
        &self,
        message_id: uuid::Uuid,
        access: MailboxRequeueAccess,
    ) -> Result<RequeueOutcome, DomainError>;
}

// =====================================================================
// Read-side per-instance authorization (#144, PROP-20260725-185140)
// =====================================================================

/// The verified caller, resolved to DOMAIN ids once per request.
///
/// Constructed only from an authenticated `Principal` (the edge does the `sub` → domain-id bridge and
/// the JWT-claim reads exactly once — ADR-20260809-050000 CARD-11: the login-to-domain bridge lives in
/// the token's claims, not in per-request lookups), so nothing downstream can invent a scope for
/// itself. The resolution is deliberately NOT repeated per check: a thread rendering five attachments
/// pays for the bridge once, and every membership test after it is a primary-key lookup.
#[derive(Debug, Clone, PartialEq)]
pub enum ReadScope {
    /// Unauthenticated. Reaches only genuinely public read models (discovery, catalog, referential).
    Public,
    Customer(CustomerId),
    Restaurant(RestaurantId),
    RestaurantAccount(RestaurantAccountId),
    /// #639 part C step 4-i (ADR-20260904-081527 §1): a STRUCT variant, deliberately — `standing`
    /// rides WITH the id so a guard that ignores it does not compile (compiler-first,
    /// ADR-20260803-234035). Resolved once at the edge (`auth.rs::resolve_rider_scope`) from the
    /// SAME `Rider` row the id comes from: one `SELECT rider_id, standing`, never two.
    Rider { id: RiderId, standing: RiderStanding },
    /// Unrestricted by role alone — ADMIN holds no membership rows at all, by design.
    Admin,
    /// The system itself: a process manager or worker acting with no user principal (#144).
    ///
    /// Deliberately NOT `Admin`. A saga is not an administrator, and conflating them would mean that
    /// the day admin reads become audited or restricted, every process manager silently inherits the
    /// restriction — or, worse, silently evades the audit. Same predicate today, different meaning,
    /// and the difference is the kind that only bites once.
    System,
}

impl ReadScope {
    /// The `(member_type, member_id)` half of a membership question, or `None` for the scopes
    /// that are answered without consulting the index (`Admin`/`System` short-circuit, `Public` denies).
    pub fn member(&self) -> Option<(UserType, uuid::Uuid)> {
        match self {
            ReadScope::Customer(id) => Some((UserType::CUSTOMER, id.0)),
            ReadScope::Restaurant(id) => Some((UserType::RESTAURANT, id.0)),
            ReadScope::RestaurantAccount(id) => Some((UserType::RESTAURANT_ACCOUNT, id.0)),
            ReadScope::Rider { id, .. } => Some((UserType::RIDER, id.0)),
            ReadScope::Admin | ReadScope::System | ReadScope::Public => None,
        }
    }
}

/// The one authorization question, asked the same way by every surface (#144).
///
/// GraphQL reads FILTER through it (a scope-less query returns fewer rows); a by-id fetch such as
/// `/files/<uuid>` CHECKS through it (one object, one yes/no). Both call this port rather than
/// growing their own logic, which is why it lives in `application` and not beside either transport.
#[async_trait]
pub trait ScopeMembershipRepository: Send + Sync {
    /// May this member see this instance?
    async fn is_member(
        &self,
        scope_type: ScopeType,
        scope_id: uuid::Uuid,
        scope: &ReadScope,
    ) -> Result<bool, DomainError>;

    /// Every scope of this type the member may see — the list-query filter.
    async fn scopes_for(
        &self,
        scope_type: ScopeType,
        scope: &ReadScope,
    ) -> Result<Vec<uuid::Uuid>, DomainError>;
}
