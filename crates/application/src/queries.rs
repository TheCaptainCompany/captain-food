//! Query-side use-case ports (the read side, ADR-0035). Resolvers/handlers depend on these traits;
//! concrete adapters live in `infrastructure` and are injected at the `server` composition root. Read
//! ports return the generated `…Row` DTOs (what the projector writes and the query side returns).

use async_trait::async_trait;

use domain::generated::entities::{Money, OptionList, Product};
use domain::generated::scalars::{
    CartId, CatalogItemAvailability, CityAvailabilityStatus, CityId, CuisineCategory, CurrencyCode,
    CustomerId, DeliveryChannelKey, DeliveryDissatisfactionReason, DeliveryJobId, DeliveryPartnerName,
    DeliveryPartnerRegistrationId, DeliveryProvider, DeliveryStatus, DeliveryTimeliness, EmailAddress,
    ExternalReference, OfferId, OfferName,
    OptionId, OptionListId, OptionName, OrderId, OrderStatus, PhoneNumber, ProductId, ProductName,
    ProspectPipelineStatus, Quantity, RefundId, RefundStatus, RestaurantAccountId, RestaurantId,
    RiderId, SessionId, Slug, StockStatus,
};
use domain::shared::errors::DomainError;

pub use crate::generated::rows::CartRow;
pub use crate::generated::rows::CatalogRow;
pub use crate::generated::rows::CustomerRow;
pub use crate::generated::rows::OrderTrackingRow;
pub use crate::generated::rows::ProspectionPipelineRow;
pub use crate::generated::rows::RestaurantRow;

/// Optional filters for public restaurant discovery — mirrors the `restaurants` query args in api.yaml.
/// V0 applies a subset (the rest are accepted and ignored until the read model backs them).
#[derive(Debug, Clone, Default)]
pub struct RestaurantFilter {
    pub search: Option<String>,
    pub orderable_only: Option<bool>,
}

/// Read port over the `Restaurant` projection table (ADR-0040). Backs the `restaurants`/`restaurant`
/// GraphQL queries.
#[async_trait]
pub trait RestaurantReadRepository: Send + Sync {
    /// Discovery list (public), newest-first, honouring the filter.
    async fn list(&self, filter: RestaurantFilter) -> Result<Vec<RestaurantRow>, DomainError>;
    /// A single restaurant by its slug (the per-restaurant storefront), or `None` if absent.
    async fn by_slug(&self, slug: Slug) -> Result<Option<RestaurantRow>, DomainError>;
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
    /// A customer's carts (one OPEN cart per restaurant), most recently updated first.
    async fn by_customer(&self, customer_id: CustomerId) -> Result<Vec<CartRow>, DomainError>;
    /// A single cart by id (session-scoped), or `None` if absent.
    async fn by_id(&self, id: CartId) -> Result<Option<CartRow>, DomainError>;

    /// The session's OPEN carts — CartBindingProcess's `read` step (`where: { session_id, status:
    /// OPEN }`). PROVIDED, empty: existing implementations (query-side fakes owned by concurrent
    /// workstreams) keep compiling and simply serve no carts to bind; the Pg adapter (and the saga
    /// tests' fakes) override with the real predicate.
    async fn open_by_session(&self, session_id: SessionId) -> Result<Vec<CartRow>, DomainError> {
        let _ = session_id;
        Ok(Vec::new())
    }
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
    /// Orders honouring the filter, most recently placed first.
    async fn list(&self, filter: OrderFilter) -> Result<Vec<OrderTrackingRow>, DomainError>;
    /// A single order by id (tracking), or `None` if absent.
    async fn by_id(&self, id: OrderId) -> Result<Option<OrderTrackingRow>, DomainError>;
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
