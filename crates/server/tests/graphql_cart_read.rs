//! #451 Phase 2 — the priced cart READ through the REAL GraphQL path (schema → generated
//! resolver → `cart_read` seam → `price_cart` over the one-read catalog snapshot):
//! - `current` two-leg resolution (ADR-20260810-120531): claim leg, session leg, and the
//!   NULL-or-claim ownership filter between them;
//! - the by-id `cart` claim-ownership narrowing (#144/#434 — the dispatch DONE-WHEN);
//! - the priced shape: a cart of (offer 15,00 EUR + option 2,00 EUR) × 2 answers 34,00 EUR —
//!   never the pre-#451 fabricated 0,00 EUR.
//!
//! PROVENANCE, stated precisely because the honest-telemetry rule this file enforces applies to
//! the file itself. These tests were written BEFORE the resolvers were wired, in commit `57b7330`,
//! whose own message records that NO gates were run on that tree. So nobody watched them fail, and
//! this file makes no claim that anyone did.
//!
//! What IS verifiable from the tree rather than from memory: in `57b7330` the generated
//! `crates/server/src/graphql/generated/query.rs` still carried `async fn current(&self) ->
//! ... Err(async_graphql::Error::new("not implemented"))`, and the by-id `cart` body had no
//! ownership check at all. Those resolvers could not have satisfied the assertions below. The
//! wiring landed separately, in Phase 2a, by regenerating from the emitter — and the first
//! OBSERVED run of this file is the green one that followed.
//!
//! The distinction matters: "seen red then green" is evidence a test can fail, and only an actual
//! run produces it. Reconstructing it afterwards from what the code must have done is an
//! inference wearing the costume of an observation.

use async_graphql::{Request, Variables};
use application::ports::AsOfPriceAuthority;
use async_trait::async_trait;
use domain::generated::entities::{CartLineItem, Offer, OptionList, Product, ProductItemOption, TaxRate};
use domain::generated::scalars as ds;
use domain::shared::errors::DomainError;
use serde_json::json;
use std::sync::Arc;

use actor_client::supervision::{MailboxLaneRow, PoisonedMessageRow};
use application::queries::{
    CartReadRepository, CartRow, CatalogReadRepository, CatalogRow, CustomerReadRepository,
    CustomerRow, DeliveryPartnerAvailabilityFilter, DeliveryPartnerAvailabilityRow,
    DeliverySatisfactionRow, OrderConversationRow, OrderFilter,
    PricingPolicyRow, ProspectFilter, ProspectionPipelineRow,
    ProspectionReadRepository, PricingPolicyReadRepository, ReadScope, ReclamationFilter,
    ReclamationRow, RefundFilter, RefundRow, RestaurantFilter, RestaurantReadRepository,
    RestaurantRow, UberEstimationPolicyReadRepository, UberEstimationPolicyRow,
    UberSplitPolicyReadRepository, UberSplitPolicyRow,
};
use application::projections::OrderTrackingRow;
use server::graphql_acl::RequestRole;

/// The role-guard witness the transports inject (#639 part B). There is no way to fabricate an
/// `ActingRole`: it comes from a `Principal` or it does not exist, so a test that exercises a role
/// has to name a caller actually BOUND to it. Roles carrying no domain binding by design (ADMIN,
/// EXTERNAL, PUBLIC) ignore the uuid, exactly as `Principal::role_path` does.
fn acting(role: RequestRole) -> server::ActingRole {
    server::Principal::role_binding(role, "test-subject".to_string(), Some(uuid::Uuid::from_u128(0x639)))
        .acting_role(role)
}
use server::graphql_schema::{build_schema, CaptainSchema, ReadDeps};
use server::graphql_session::SessionHeader;

fn uid(n: u8) -> uuid::Uuid {
    uuid::Uuid::from_u128(n as u128)
}
fn eur(cents: i64) -> domain::generated::entities::Money {
    domain::generated::entities::Money {
        amount_cents: ds::MoneyCents(cents),
        currency: ds::CurrencyCode("EUR".into()),
    }
}

// --- fakes -----------------------------------------------------------------------------------

struct MemCarts(Vec<CartRow>);

#[async_trait]
impl CartReadRepository for MemCarts {
    async fn by_customer(&self, customer_id: ds::CustomerId) -> Result<Vec<CartRow>, DomainError> {
        let mut rows: Vec<CartRow> =
            self.0.iter().filter(|r| r.customer_id == Some(customer_id)).cloned().collect();
        rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(rows)
    }
    /// OPEN-only, mirroring the Pg adapter's predicate (#451).
    async fn by_id(&self, id: ds::CartId) -> Result<Option<CartRow>, DomainError> {
        Ok(self
            .0
            .iter()
            .find(|r| r.cart_id == id && r.status == ds::CartStatus::OPEN)
            .cloned())
    }
    async fn open_by_session(&self, session_id: ds::SessionId) -> Result<Vec<CartRow>, DomainError> {
        let mut rows: Vec<CartRow> = self
            .0
            .iter()
            .filter(|r| r.session_id == session_id && r.status == ds::CartStatus::OPEN)
            .cloned()
            .collect();
        rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(rows)
    }
    /// Tenant-scoped leg 1 (#469): the restaurant predicate is a PORT obligation, so the double
    /// honours it. That the SQL honours it too is asserted where it has to be — against a real
    /// Postgres (`infrastructure/tests/main/cart_projection.rs`), because a well-behaved fake is
    /// precisely what would let unfiltered SQL ship green.
    async fn open_by_customer_at(
        &self,
        customer_id: ds::CustomerId,
        restaurant_id: ds::RestaurantId,
    ) -> Result<Vec<CartRow>, DomainError> {
        let mut rows: Vec<CartRow> = self
            .0
            .iter()
            .filter(|r| {
                r.customer_id == Some(customer_id)
                    && r.restaurant_id == restaurant_id
                    && r.status == ds::CartStatus::OPEN
            })
            .cloned()
            .collect();
        rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(rows)
    }
    /// Tenant-scoped leg 2 (#469), same obligation.
    async fn open_by_session_at(
        &self,
        session_id: ds::SessionId,
        restaurant_id: ds::RestaurantId,
    ) -> Result<Vec<CartRow>, DomainError> {
        let mut rows: Vec<CartRow> = self
            .0
            .iter()
            .filter(|r| {
                r.session_id == session_id
                    && r.restaurant_id == restaurant_id
                    && r.status == ds::CartStatus::OPEN
            })
            .cloned()
            .collect();
        rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(rows)
    }
}

/// A catalog store serving ONE projected row whose `tree` is built from the REAL entity types
/// through serde — `offer_view_from_tree` walks exactly what the CatalogProjector writes.
struct TreeCatalogs(CatalogRow);

#[async_trait]
impl CatalogReadRepository for TreeCatalogs {
    async fn by_restaurant(&self, id: ds::RestaurantId) -> Result<Option<CatalogRow>, DomainError> {
        Ok((id == self.0.restaurant_id).then(|| self.0.clone()))
    }
}

/// The door defaults CLOSED and most of this file's tests never open it; a real `at_head`/`as_of`
/// call here would be a defect (the closed arm falling through to the fold), so both refuse loudly
/// rather than answering with an empty catalog. The four door-OPEN tests below inject
/// [`FakeAsOf`]/`FailingAsOf` instead.
#[async_trait]
impl application::ports::AsOfPriceAuthority for Empty {
    async fn as_of(
        &self,
        _catalog_id: ds::CatalogId,
        _version: domain::catalog_as_of::CatalogVersion,
    ) -> Result<domain::catalog_as_of::AsOfCatalog, DomainError> {
        Err(DomainError::Repository("Empty never folds".into()))
    }
    async fn at_head(
        &self,
        _catalog_id: ds::CatalogId,
        _correlation_id: uuid::Uuid,
    ) -> Result<domain::catalog_as_of::AsOfCatalog, DomainError> {
        Err(DomainError::Repository("Empty never folds".into()))
    }
}

/// A fold-priced authority over an IN-MEMORY, append-only event log matching [`catalog_row`]'s own
/// fixture exactly (catalog 50, restaurant 90, offer 20 @ 15,00 EUR, option 30 @ 2,00 EUR in list
/// 200) -- built through the SAME `AsOfCatalog::from_stream` fold the real adapter uses, never a
/// hand-assembled value (the one-pricer property, ADR-20260810-112836 §1/§3/§5/§6). `reads` counts
/// `at_head` calls ONLY -- the mint's own read, never the (unrelated) `catalogs`/projection read
/// (PROP-20260831-134539 slice 3a, D2's "exactly one catalog read" is about the fold leg).
struct FakeAsOf {
    events: std::sync::Mutex<Vec<domain::generated::events::DomainEvent>>,
    catalog_id: ds::CatalogId,
    reads: std::sync::atomic::AtomicUsize,
    /// The coordinate the MOST RECENT `at_head` call returned -- test-only bookkeeping, honestly
    /// recorded (never on the reply, D3), so a test can learn "the coordinate the mint used"
    /// without hardcoding it.
    last_coordinate: std::sync::atomic::AtomicI64,
    /// The `correlation_id` the MOST RECENT `at_head` call received -- beck NB6: both fakes used to
    /// discard `_correlation_id`, so a caller passing `Uuid::nil()` was indistinguishable from one
    /// passing the request's own id. Captured honestly, never on the reply.
    last_correlation_id: std::sync::Mutex<Option<uuid::Uuid>>,
}

impl FakeAsOf {
    /// Three events -> head coordinate 3: `CatalogCreated`, `OptionListAdded` (option 30 @ 2,00),
    /// `ProductAdded` (offer 20 @ 15,00, linked to option list 200) -- the exact fixture
    /// [`catalog_row`] builds via the real projector fold, so a test comparing the fold-priced
    /// path against the projection-priced path is comparing two folds of the SAME facts.
    fn seeded(catalog_id: ds::CatalogId, restaurant: ds::RestaurantId) -> Self {
        use domain::generated::entities::{
            Offer, OptionList, Product, ProductItemOption, TaxRate,
        };
        use domain::generated::events::{CatalogCreated, DomainEvent, OptionListAdded, ProductAdded};
        let option_list_id = ds::OptionListId(uid(200));
        let option_id = ds::OptionId(uid(30));
        let product_id = ds::ProductId(uid(120));
        let offer_id = ds::OfferId(uid(20));
        let events = vec![
            DomainEvent::CatalogCreated(CatalogCreated {
                catalog_id,
                r#ref: None,
                restaurant_id: restaurant,
                name: ds::CatalogName("Menu".into()),
            }),
            DomainEvent::OptionListAdded(OptionListAdded {
                catalog_id,
                restaurant_id: restaurant,
                option_list: OptionList {
                    id: option_list_id,
                    r#ref: None,
                    name: ds::OptionListName("Extras".into()),
                    min_selections: 0,
                    max_selections: Some(1),
                    multiple_selection: false,
                    options: vec![ProductItemOption {
                        id: option_id,
                        r#ref: None,
                        option_list_id,
                        name: ds::OptionName("Cheese".into()),
                        price: eur(200),
                        r#default: false,
                        availability: ds::CatalogItemAvailability::AVAILABLE,
                        stock: None,
                    }],
                },
            }),
            DomainEvent::ProductAdded(ProductAdded {
                catalog_id,
                restaurant_id: restaurant,
                product: Product {
                    id: product_id,
                    r#ref: None,
                    catalog_id,
                    restaurant_id: restaurant,
                    category_ref: None,
                    name: ds::ProductName("Burger".into()),
                    description: None,
                    tags: Vec::new(),
                    image_ids: Vec::new(),
                    tax_rate: TaxRate { delivery: ds::TaxRatePercent(10.0), collection: None, eat_in: None },
                    offers: vec![Offer {
                        id: offer_id,
                        r#ref: None,
                        product_id,
                        name: ds::OfferName("Default".into()),
                        price: eur(1500),
                        availability: ds::CatalogItemAvailability::AVAILABLE,
                        stock: None,
                        option_list_ids: vec![option_list_id],
                    }],
                },
            }),
        ];
        Self {
            events: std::sync::Mutex::new(events),
            catalog_id,
            reads: std::sync::atomic::AtomicUsize::new(0),
            last_coordinate: std::sync::atomic::AtomicI64::new(0),
            last_correlation_id: std::sync::Mutex::new(None),
        }
    }

    /// The coordinate the most recent `at_head` call returned (test-only introspection).
    fn last_returned_coordinate(&self) -> i64 {
        self.last_coordinate.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// The `correlation_id` the most recent `at_head` call received (test-only introspection).
    fn last_correlation_id(&self) -> Option<uuid::Uuid> {
        *self.last_correlation_id.lock().expect("lock")
    }

    /// Appends a fourth event bumping offer 20's price to `new_price_cents` -- simulates a
    /// concurrent catalog change that happened AFTER an earlier read (repricing-at-coordinate
    /// tests use this to prove a bounded re-read at the ORIGINAL coordinate is unaffected).
    fn append_price_update(&self, new_price_cents: i64) {
        use domain::generated::events::{DomainEvent, ProductUpdated};
        let mut events = self.events.lock().expect("lock");
        let restaurant_id = match events.first() {
            Some(DomainEvent::CatalogCreated(c)) => c.restaurant_id,
            _ => panic!("seeded() always starts with CatalogCreated"),
        };
        events.push(DomainEvent::ProductUpdated(ProductUpdated {
            catalog_id: self.catalog_id,
            restaurant_id,
            product: domain::generated::entities::Product {
                id: ds::ProductId(uid(120)),
                r#ref: None,
                catalog_id: self.catalog_id,
                restaurant_id,
                category_ref: None,
                name: ds::ProductName("Burger".into()),
                description: None,
                tags: Vec::new(),
                image_ids: Vec::new(),
                tax_rate: TaxRate { delivery: ds::TaxRatePercent(10.0), collection: None, eat_in: None },
                offers: vec![Offer {
                    id: ds::OfferId(uid(20)),
                    r#ref: None,
                    product_id: ds::ProductId(uid(120)),
                    name: ds::OfferName("Default".into()),
                    price: eur(new_price_cents),
                    availability: ds::CatalogItemAvailability::AVAILABLE,
                    stock: None,
                    option_list_ids: vec![ds::OptionListId(uid(200))],
                }],
            },
        }));
    }
}

#[async_trait]
impl application::ports::AsOfPriceAuthority for FakeAsOf {
    async fn as_of(
        &self,
        catalog_id: ds::CatalogId,
        version: domain::catalog_as_of::CatalogVersion,
    ) -> Result<domain::catalog_as_of::AsOfCatalog, DomainError> {
        if catalog_id != self.catalog_id {
            return Err(DomainError::Repository("unknown catalog".into()));
        }
        let events = self.events.lock().expect("lock");
        let versioned: Vec<_> = events
            .iter()
            .enumerate()
            .filter(|(i, _)| (*i as i64 + 1) <= version.get())
            .map(|(i, e)| (domain::catalog_as_of::CatalogVersion::try_new(i as i64 + 1).unwrap(), e.clone()))
            .collect();
        Ok(domain::catalog_as_of::AsOfCatalog::from_stream(&versioned, version))
    }

    async fn at_head(
        &self,
        catalog_id: ds::CatalogId,
        correlation_id: uuid::Uuid,
    ) -> Result<domain::catalog_as_of::AsOfCatalog, DomainError> {
        self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *self.last_correlation_id.lock().expect("lock") = Some(correlation_id);
        if catalog_id != self.catalog_id {
            return Err(DomainError::Repository("unknown catalog".into()));
        }
        let events = self.events.lock().expect("lock");
        if events.is_empty() {
            return Err(DomainError::Repository("catalog not created".into()));
        }
        let head = domain::catalog_as_of::CatalogVersion::try_new(events.len() as i64).unwrap();
        let versioned: Vec<_> = events
            .iter()
            .enumerate()
            .map(|(i, e)| (domain::catalog_as_of::CatalogVersion::try_new(i as i64 + 1).unwrap(), e.clone()))
            .collect();
        self.last_coordinate.store(head.get(), std::sync::atomic::Ordering::SeqCst);
        Ok(domain::catalog_as_of::AsOfCatalog::from_stream(&versioned, head))
    }
}

/// Always refuses (D4's "fold Err" case): an empty, never-created catalog stream.
struct FailingAsOf;

#[async_trait]
impl application::ports::AsOfPriceAuthority for FailingAsOf {
    async fn as_of(
        &self,
        _catalog_id: ds::CatalogId,
        _version: domain::catalog_as_of::CatalogVersion,
    ) -> Result<domain::catalog_as_of::AsOfCatalog, DomainError> {
        Err(DomainError::Repository("catalog not created".into()))
    }
    async fn at_head(
        &self,
        _catalog_id: ds::CatalogId,
        _correlation_id: uuid::Uuid,
    ) -> Result<domain::catalog_as_of::AsOfCatalog, DomainError> {
        Err(DomainError::Repository("catalog not created".into()))
    }
}

/// The fixture menu: offer 20 at 15,00 EUR with option 30 (2,00 EUR) in option list 200.
fn catalog_row(restaurant: ds::RestaurantId) -> CatalogRow {
    let product = Product {
        id: ds::ProductId(uid(120)),
        r#ref: None,
        catalog_id: ds::CatalogId(uid(50)),
        restaurant_id: restaurant,
        category_ref: None,
        name: ds::ProductName("Burger".into()),
        description: None,
        tags: Vec::new(),
        image_ids: Vec::new(),
        tax_rate: TaxRate { delivery: ds::TaxRatePercent(10.0), collection: None, eat_in: None },
        offers: vec![Offer {
            id: ds::OfferId(uid(20)),
            r#ref: None,
            product_id: ds::ProductId(uid(120)),
            name: ds::OfferName("Default".into()),
            price: eur(1500),
            availability: ds::CatalogItemAvailability::AVAILABLE,
            stock: None,
            option_list_ids: vec![ds::OptionListId(uid(200))],
        }],
    };
    let extras = OptionList {
        id: ds::OptionListId(uid(200)),
        r#ref: None,
        name: ds::OptionListName("Extras".into()),
        min_selections: 0,
        max_selections: Some(1),
        multiple_selection: false,
        options: vec![ProductItemOption {
            id: ds::OptionId(uid(30)),
            r#ref: None,
            option_list_id: ds::OptionListId(uid(200)),
            name: ds::OptionName("Cheese".into()),
            price: eur(200),
            r#default: false,
            availability: ds::CatalogItemAvailability::AVAILABLE,
            stock: None,
        }],
    };
    let now = chrono::Utc::now();
    CatalogRow {
        catalog_id: ds::CatalogId(uid(50)),
        restaurant_id: restaurant,
        slug: None,
        name: ds::CatalogName("Menu".into()),
        tree: json!({
            "products": serde_json::to_value(vec![product]).unwrap(),
            "optionLists": serde_json::to_value(vec![extras]).unwrap(),
        }),
        created_at: now,
        updated_at: now,
    }
}

#[derive(Clone)]
struct OneRestaurant(RestaurantRow);

#[async_trait]
impl RestaurantReadRepository for OneRestaurant {
    async fn list(&self, _f: RestaurantFilter) -> Result<Vec<RestaurantRow>, DomainError> {
        Ok(vec![self.0.clone()])
    }
    async fn by_slug(&self, _s: ds::Slug) -> Result<Option<RestaurantRow>, DomainError> {
        Ok(Some(self.0.clone()))
    }
    async fn by_id(&self, id: ds::RestaurantId) -> Result<Option<RestaurantRow>, DomainError> {
        Ok((id == self.0.restaurant_id).then(|| self.0.clone()))
    }
}

fn restaurant_row(restaurant_id: uuid::Uuid) -> RestaurantRow {
    let now = chrono::Utc::now();
    RestaurantRow {
        restaurant_id: ds::RestaurantId(restaurant_id),
        restaurant_account_id: None,
        listing_status: ds::RestaurantListingStatus::ACTIVE_PARTNER,
        external_identifiers: None,
        google_place_id: None,
        slug: Some(ds::Slug("chez-marco".into())),
        display_name: ds::RestaurantDisplayName("Chez Marco".into()),
        description: None,
        tags: None,
        margin_rate: None,
        cuisine_category: None,
        uber_prices_opt_in: None,
        website: None,
        rating: None,
        reviews_count: None,
        gbp_order_url: None,
        gbp_link_status: None,
        address: json!({ "line1": "1 Rue Nationale", "postalCode": "37000", "city": "Tours", "country": "FR" }),
        location: None,
        opening_hours: json!([]),
        status: ds::RestaurantStatus::ACTIVE,
        order_acceptance: ds::OrderAcceptanceMode::NORMAL,
        default_currency: ds::CurrencyCode("EUR".into()),
        timezone: None,
        preparation_time_minutes: None,
        created_at: now,
        updated_at: now,
    }
}

fn cart_row(
    cart: u8,
    restaurant: ds::RestaurantId,
    session: u8,
    customer: Option<u8>,
    lines: Vec<CartLineItem>,
    at: i64,
) -> CartRow {
    CartRow {
        cart_id: ds::CartId(uid(cart)),
        restaurant_id: restaurant,
        session_id: ds::SessionId(uid(session)),
        customer_id: customer.map(|c| ds::CustomerId(uid(c))),
        status: ds::CartStatus::OPEN,
        lines: serde_json::to_value(lines).unwrap(),
        created_at: chrono::DateTime::from_timestamp(at, 0).unwrap(),
        updated_at: chrono::DateTime::from_timestamp(at, 0).unwrap(),
    }
}

/// The 34,00 EUR line: offer 20 (15,00) + option 30 (2,00), × 2.
fn line_3400() -> CartLineItem {
    CartLineItem {
        cart_line_id: ds::CartLineId(uid(10)),
        offer_id: ds::OfferId(uid(20)),
        quantity: 2,
        selected_option_ids: vec![ds::OptionId(uid(30))],
    }
}

/// The same fixture menu, ONE unit: 17,00 EUR. Exists so the two storefronts in the #469 path test
/// answer DIFFERENT totals -- an assertion on the cart id alone would pass an implementation that
/// serves the right row and prices it from the wrong restaurant's menu.
fn line_1700() -> CartLineItem {
    CartLineItem {
        cart_line_id: ds::CartLineId(uid(11)),
        offer_id: ds::OfferId(uid(20)),
        quantity: 1,
        selected_option_ids: vec![ds::OptionId(uid(30))],
    }
}

// --- Empty stand-ins for the read models these resolvers never touch --------------------------

struct Empty;

#[async_trait]
impl ProspectionReadRepository for Empty {
    async fn list(&self, _f: ProspectFilter) -> Result<Vec<ProspectionPipelineRow>, DomainError> {
        Ok(Vec::new())
    }
}
#[async_trait]
impl PricingPolicyReadRepository for Empty {
    async fn list(&self) -> Result<Vec<PricingPolicyRow>, DomainError> {
        Ok(Vec::new())
    }
}
#[async_trait]
impl UberEstimationPolicyReadRepository for Empty {
    async fn list(&self) -> Result<Vec<UberEstimationPolicyRow>, DomainError> {
        Ok(Vec::new())
    }
}
#[async_trait]
impl UberSplitPolicyReadRepository for Empty {
    async fn list(&self) -> Result<Vec<UberSplitPolicyRow>, DomainError> {
        Ok(Vec::new())
    }
}
#[async_trait]
impl application::queries::OrderReadRepository for Empty {
    async fn list(
        &self,
        _f: OrderFilter,
        _scope: &ReadScope,
    ) -> Result<Vec<OrderTrackingRow>, DomainError> {
        Ok(Vec::new())
    }
    async fn by_id(
        &self,
        _id: ds::OrderId,
        _scope: &ReadScope,
    ) -> Result<Option<OrderTrackingRow>, DomainError> {
        Ok(None)
    }
}
#[async_trait]
impl CustomerReadRepository for Empty {
    async fn by_phone(&self, _p: ds::PhoneNumber) -> Result<Option<CustomerRow>, DomainError> {
        Ok(None)
    }
    async fn by_email(&self, _e: ds::EmailAddress) -> Result<Option<CustomerRow>, DomainError> {
        Ok(None)
    }
    async fn by_id(&self, _id: ds::CustomerId) -> Result<Option<CustomerRow>, DomainError> {
        Ok(None)
    }
    async fn by_auth_ref(&self, _r: ds::ExternalReference) -> Result<Option<CustomerRow>, DomainError> {
        Ok(None)
    }
}
#[async_trait]
impl application::queries::DeliveryReadRepository for Empty {
    async fn by_order(
        &self,
        _o: ds::OrderId,
    ) -> Result<Option<application::queries::DeliveryJobRow>, DomainError> {
        Ok(None)
    }
    async fn for_rider(
        &self,
        _r: ds::RiderId,
        _s: Option<ds::DeliveryStatus>,
    ) -> Result<Vec<application::queries::DeliveryJobRow>, DomainError> {
        Ok(vec![])
    }
    async fn held_by_rider(
        &self,
        _r: ds::RiderId,
    ) -> Result<Option<application::queries::DeliveryJobRow>, DomainError> {
        Ok(None)
    }
    async fn held_by_riders(
        &self,
        _r: &[ds::RiderId],
    ) -> Result<Vec<application::queries::DeliveryJobRow>, DomainError> {
        Ok(vec![])
    }
    async fn by_restaurant(
        &self,
        _r: ds::RestaurantId,
        _s: Option<ds::DeliveryStatus>,
    ) -> Result<Vec<application::queries::DeliveryJobRow>, DomainError> {
        Ok(vec![])
    }
}
#[async_trait]
impl application::queries::RiderRestrictionReadRepository for Empty {
    async fn by_rider_id(
        &self,
        _r: ds::RiderId,
    ) -> Result<Option<application::queries::RiderRestrictionRow>, DomainError> {
        Ok(None)
    }
}
#[async_trait]
impl application::queries::RiderRosterReadRepository for Empty {
    async fn all(&self) -> Result<Vec<application::queries::RiderRosterRow>, DomainError> {
        Ok(vec![])
    }
    async fn by_id(
        &self,
        _r: ds::RiderId,
    ) -> Result<Option<application::queries::RiderRosterRow>, DomainError> {
        Ok(None)
    }
}

#[async_trait]
impl application::queries::MemberAuthorityRepository for Empty {
    async fn authority_for_subject(
        &self,
        _s: domain::generated::scalars::AuthSubject,
        _r: ds::RestaurantId,
    ) -> Result<Option<domain::generated::scalars::MemberAuthority>, DomainError> {
        Ok(None)
    }
}

#[async_trait]
impl application::queries::RestaurantRosterReadRepository for Empty {
    async fn by_scope(
        &self,
        _s: ds::RestaurantId,
        _limit: i64,
        _offset: i64,
    ) -> Result<Vec<application::queries::RestaurantRosterRow>, DomainError> {
        Ok(vec![])
    }
}
#[async_trait]
impl application::queries::RestaurantInvitationListReadRepository for Empty {
    async fn by_scope(
        &self,
        _s: ds::RestaurantId,
        _limit: i64,
        _offset: i64,
    ) -> Result<Vec<application::queries::RestaurantInvitationListRow>, DomainError> {
        Ok(vec![])
    }
}
#[async_trait]
impl application::queries::OrderConversationReadRepository for Empty {
    async fn by_order(&self, _o: ds::OrderId) -> Result<Option<OrderConversationRow>, DomainError> {
        Ok(None)
    }
}
#[async_trait]
impl application::queries::CustomerCreditReadRepository for Empty {
    async fn by_customer(
        &self,
        _c: ds::CustomerId,
    ) -> Result<Option<application::queries::CustomerCreditBalanceRow>, DomainError> {
        Ok(None)
    }
}
#[async_trait]
impl application::queries::RefundReadRepository for Empty {
    async fn list(&self, _f: RefundFilter) -> Result<Vec<RefundRow>, DomainError> {
        Ok(vec![])
    }
}
#[async_trait]
impl application::queries::DeliveryPartnerAvailabilityReadRepository for Empty {
    async fn list(
        &self,
        _f: DeliveryPartnerAvailabilityFilter,
    ) -> Result<Vec<DeliveryPartnerAvailabilityRow>, DomainError> {
        Ok(vec![])
    }
}
#[async_trait]
impl application::queries::ReclamationReadRepository for Empty {
    async fn by_customer(&self, _c: ds::CustomerId) -> Result<Vec<ReclamationRow>, DomainError> {
        Ok(vec![])
    }
    async fn list(&self, _f: ReclamationFilter) -> Result<Vec<ReclamationRow>, DomainError> {
        Ok(vec![])
    }
    async fn by_id(&self, _id: ds::ReclamationId) -> Result<Option<ReclamationRow>, DomainError> {
        Ok(None)
    }
}
#[async_trait]
impl application::queries::DeliverySatisfactionReadRepository for Empty {
    async fn by_restaurant(
        &self,
        _r: ds::RestaurantId,
        _t: Option<ds::DeliveryTimeliness>,
    ) -> Result<Vec<DeliverySatisfactionRow>, DomainError> {
        Ok(vec![])
    }
}
#[async_trait]
impl actor_client::supervision::MailboxLaneRepository for Empty {
    async fn list(
        &self,
        _access: actor_client::mailbox::MailboxAccess,
    ) -> Result<Vec<MailboxLaneRow>, DomainError> {
        Ok(vec![])
    }
    async fn poisoned(
        &self,
        _actor_type: Option<String>,
        _limit: i64,
        _access: actor_client::mailbox::MailboxAccess,
    ) -> Result<Vec<PoisonedMessageRow>, DomainError> {
        Ok(vec![])
    }
}

// --- harness -----------------------------------------------------------------------------------

/// A fixed test key -- beck (ii): every schema this file builds mints under it, so a test that
/// wants to VERIFY a minted quote (rather than merely assert its presence) builds a
/// `QuoteVerifier` over the SAME key.
fn test_minter() -> Arc<application::quote::QuoteMinter> {
    Arc::new(application::quote::QuoteMinter::new(
        application::quote::SigningKey::from_resolved_secret("test-key", "test-signing-secret"),
    ))
}

fn schema_over(carts: Vec<CartRow>, restaurant: ds::RestaurantId) -> CaptainSchema {
    build_schema(
        Some(ReadDeps {
            restaurants: Arc::new(OneRestaurant(restaurant_row(restaurant.0))),
            prospection: Arc::new(Empty),
            pricing_policy: Arc::new(Empty),
            uber_estimation_policy: Arc::new(Empty),
            uber_split_policy: Arc::new(Empty),
            catalogs: Arc::new(TreeCatalogs(catalog_row(restaurant))),
            carts: Arc::new(MemCarts(carts)),
            orders: Arc::new(Empty),
            order_conversations: Arc::new(Empty),
            customers: Arc::new(Empty),
            deliveries: Arc::new(Empty),
            rider_restrictions: Arc::new(Empty),
            rider_roster: Arc::new(Empty),
            member_authority: Arc::new(Empty),
            restaurant_roster: Arc::new(Empty),
            restaurant_invitations: Arc::new(Empty),
            refunds: Arc::new(Empty),
            delivery_satisfaction: Arc::new(Empty),
            delivery_partner_availabilities: Arc::new(Empty),
            reclamations: Arc::new(Empty),
            customer_credit: Arc::new(Empty),
            mailbox_lanes: Arc::new(Empty),
        // RSO-1: the spec-default horizon (900 s) -- tests assert behaviour, not config.
        service_window_horizon: Default::default(),
        support_contact: None,
        run_rider_restriction_door: server::graphql_schema::RunRiderRestrictionDoor(false),
        as_of_price_authority: Arc::new(Empty),
        run_fold_priced_cart_read: server::graphql_schema::RunFoldPricedCartRead(false),
        quote_minter: test_minter(),
        }),
        None,
        None,
    )
}

/// The same schema, with the door's authority and position INJECTABLE — the four PROP-20260831-134539
/// slice 3a tests below (D2/D4) need to swap in [`FakeAsOf`]/`FailingAsOf` and flip
/// `RUN_FOLD_PRICED_CART_READ` ON.
fn schema_over_with_door(
    carts: Vec<CartRow>,
    restaurant: ds::RestaurantId,
    authority: Arc<dyn application::ports::AsOfPriceAuthority>,
    door_open: bool,
) -> CaptainSchema {
    build_schema(
        Some(ReadDeps {
            restaurants: Arc::new(OneRestaurant(restaurant_row(restaurant.0))),
            prospection: Arc::new(Empty),
            pricing_policy: Arc::new(Empty),
            uber_estimation_policy: Arc::new(Empty),
            uber_split_policy: Arc::new(Empty),
            catalogs: Arc::new(TreeCatalogs(catalog_row(restaurant))),
            carts: Arc::new(MemCarts(carts)),
            orders: Arc::new(Empty),
            order_conversations: Arc::new(Empty),
            customers: Arc::new(Empty),
            deliveries: Arc::new(Empty),
            rider_restrictions: Arc::new(Empty),
            rider_roster: Arc::new(Empty),
            member_authority: Arc::new(Empty),
            restaurant_roster: Arc::new(Empty),
            restaurant_invitations: Arc::new(Empty),
            refunds: Arc::new(Empty),
            delivery_satisfaction: Arc::new(Empty),
            delivery_partner_availabilities: Arc::new(Empty),
            reclamations: Arc::new(Empty),
            customer_credit: Arc::new(Empty),
            mailbox_lanes: Arc::new(Empty),
            service_window_horizon: Default::default(),
            support_contact: None,
            run_rider_restriction_door: server::graphql_schema::RunRiderRestrictionDoor(false),
            as_of_price_authority: authority,
            run_fold_priced_cart_read: server::graphql_schema::RunFoldPricedCartRead(door_open),
            quote_minter: test_minter(),
        }),
        None,
        None,
    )
}

const CURRENT_Q: &str = "query { current { id restaurantId status totalAmount { amountCents currency } \
                         breakdown { total { amountCents } } \
                         lines { quantity lineTotal { amountCents } } } }";
const CURRENT_WITH_QUOTE_Q: &str = "query { current { id totalAmount { amountCents } quote } }";
const CARTS_WITH_QUOTE_Q: &str = "query($customerId: CustomerId!) { carts(input: { customerId: $customerId }) { id quote } }";
const CART_Q: &str = "query($id: CartId!) { cart(input: { id: $id }) { id totalAmount { amountCents } } }";
const CARTS_Q: &str = "query($customerId: CustomerId!) { carts(input: { customerId: $customerId }) { id lines { lineTotal { amountCents } } } }";

/// The tenant the HTTP edge resolves from the `Host` (#469). The schema-level tests below supply
/// it directly because they execute the SCHEMA; the PATH-level tests at the end of this file supply
/// a `Host` instead and prove the edge resolves it -- which is the half no `.data()` can assert.
fn tenant_of(restaurant: ds::RestaurantId) -> server::graphql_tenant::TenantScope {
    server::graphql_tenant::TenantScope::Restaurant(restaurant)
}

fn cart_vars(cart: u8) -> Variables {
    Variables::from_json(json!({ "id": uid(cart) }))
}

// --- tests ---------------------------------------------------------------------------------------

/// Leg 1 through the REAL path: an identified customer's bound cart prices to 34,00 EUR — the
/// known-price fixture (offer 15,00 + option 2,00, × 2) — with breakdown.total identical.
/// Pre-wiring, the generated `current` resolver was the `not implemented` stub (see the module
/// header on provenance): it could not have answered this.
#[tokio::test]
async fn leg1_the_customers_bound_cart_prices_live_to_3400() {
    let restaurant = ds::RestaurantId(uid(90));
    let schema = schema_over(vec![cart_row(1, restaurant, 10, Some(5), vec![line_3400()], 100)], restaurant);
    let resp = schema
        .execute(
            Request::new(CURRENT_Q)
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(5))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(resp.errors.is_empty(), "no errors expected, got {:?}", resp.errors);
    let data = resp.data.into_json().unwrap();
    assert_eq!(data["current"]["totalAmount"]["amountCents"], json!(3400));
    assert_eq!(data["current"]["totalAmount"]["currency"], json!("EUR"));
    assert_eq!(data["current"]["breakdown"]["total"]["amountCents"], json!(3400));
    assert_eq!(data["current"]["lines"][0]["lineTotal"]["amountCents"], json!(3400));
    assert_eq!(data["current"]["status"], json!("OPEN"));
}

/// Leg 2 through the REAL path: an ANONYMOUS caller (PUBLIC role, no claim) with the session's
/// X-SESSION-ID resolves the unbound session cart, priced LIVE — the storefront guest mini-cart
/// (the flow Phase 1 broke). A different session id resolves null, and no session header
/// resolves null (the empty state, never a fabricated 0,00 EUR).
/// Pre-wiring, `current` was the `not implemented` stub.
#[tokio::test]
async fn leg2_an_anonymous_session_resolves_its_own_cart_and_only_its_own() {
    let restaurant = ds::RestaurantId(uid(90));
    let schema = schema_over(vec![cart_row(1, restaurant, 10, None, vec![line_3400()], 100)], restaurant);

    let own = schema
        .execute(
            Request::new(CURRENT_Q)
                .data(acting(RequestRole::Public))
                .data(SessionHeader(Some(uid(10))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(own.errors.is_empty(), "no errors expected, got {:?}", own.errors);
    let data = own.data.into_json().unwrap();
    assert_eq!(data["current"]["totalAmount"]["amountCents"], json!(3400));

    let other = schema
        .execute(
            Request::new(CURRENT_Q)
                .data(acting(RequestRole::Public))
                .data(SessionHeader(Some(uid(11))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(other.errors.is_empty());
    assert_eq!(other.data.into_json().unwrap()["current"], json!(null), "session B sees nothing");

    let headerless = schema
        .execute(Request::new(CURRENT_Q).data(acting(RequestRole::Public)).data(tenant_of(restaurant)))
        .await;
    assert!(headerless.errors.is_empty());
    assert_eq!(headerless.data.into_json().unwrap()["current"], json!(null));
}

/// Leg 2's NULL-or-claim filter through the REAL path: once the cart is BOUND to customer 5, an
/// anonymous replay of the same session id sees null — the session id is a correlator, not an
/// identity. Pre-wiring, `current` was the `not implemented` stub.
#[tokio::test]
async fn leg2_a_bound_cart_is_invisible_to_an_anonymous_session_replay() {
    let restaurant = ds::RestaurantId(uid(90));
    let schema = schema_over(vec![cart_row(1, restaurant, 10, Some(5), vec![line_3400()], 100)], restaurant);
    let resp = schema
        .execute(
            Request::new(CURRENT_Q)
                .data(acting(RequestRole::Public))
                .data(SessionHeader(Some(uid(10))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(resp.errors.is_empty());
    assert_eq!(resp.data.into_json().unwrap()["current"], json!(null));
}

/// The same filter under a FOREIGN CLAIM, through the REAL path: customer 6 authenticates and
/// rides session 10, whose cart is bound to customer 5. Both legs must refuse it — leg 1 because
/// customer 6 owns no cart, leg 2 because the row's `customer_id` is neither NULL nor 6.
///
/// This duplicates the seam unit test at `cart_read::tests` DELIBERATELY, one layer up. That test
/// proves the PREDICATE; this one proves the predicate is still reached with the caller's real
/// claim after the resolver has assembled the context. A resolver that dropped the ReadScope on
/// the way in — passing `Public`, say — would leave the unit test green and hand customer 6 a leak
/// only this test can see. Session hijacking by cookie replay is precisely the attack the
/// NULL-or-claim filter exists to stop, so the wiring deserves its own witness.
#[tokio::test]
async fn leg2_a_bound_cart_is_invisible_to_a_different_customers_claim_on_that_session() {
    let restaurant = ds::RestaurantId(uid(90));
    let schema = schema_over(vec![cart_row(1, restaurant, 10, Some(5), vec![line_3400()], 100)], restaurant);
    let resp = schema
        .execute(
            Request::new(CURRENT_Q)
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(6))))
                .data(SessionHeader(Some(uid(10))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(resp.errors.is_empty(), "no errors expected, got {:?}", resp.errors);
    assert_eq!(
        resp.data.into_json().unwrap()["current"],
        json!(null),
        "customer 6 riding customer 5's session must see nothing"
    );

    // The control: the SAME request as its rightful owner resolves the cart, priced. Without this
    // the assertion above would also pass if `current` were broken for everyone.
    let owner = schema
        .execute(
            Request::new(CURRENT_Q)
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(5))))
                .data(SessionHeader(Some(uid(10))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(owner.errors.is_empty(), "no errors expected, got {:?}", owner.errors);
    assert_eq!(
        owner.data.into_json().unwrap()["current"]["totalAmount"]["amountCents"],
        json!(3400),
        "the owner still reads their own cart, priced live"
    );
}

/// An EMPTY open cart (every line removed) is the true sum of zero lines: 0 EUR, no breakdown —
/// arithmetic, not a fabricated payable.
#[tokio::test]
async fn an_empty_open_cart_answers_zero_with_no_breakdown() {
    let restaurant = ds::RestaurantId(uid(90));
    let schema = schema_over(vec![cart_row(1, restaurant, 10, Some(5), vec![], 100)], restaurant);
    let resp = schema
        .execute(
            Request::new(CURRENT_Q)
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(5))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(resp.errors.is_empty(), "{:?}", resp.errors);
    let data = resp.data.into_json().unwrap();
    assert_eq!(data["current"]["totalAmount"]["amountCents"], json!(0));
    assert_eq!(data["current"]["breakdown"], json!(null));
    assert_eq!(data["current"]["lines"], json!([]));
}

/// The by-id narrowing DONE-WHEN, all four verdicts through the REAL path: the owner reads their
/// cart (priced 34,00), a STRANGER customer reads null (no existence oracle), ADMIN reads any
/// cart priced, and an unbound session cart is by-id invisible even to a customer.
/// Pre-wiring, the by-id body carried NO ownership check and no pricing: it returned every
/// caller the full cart via the fabricating `Cart::from` (0,00 EUR) — the IDOR the spec had
/// already closed on paper but the body had not.
#[tokio::test]
async fn by_id_cart_admits_the_owner_and_admin_only() {
    let restaurant = ds::RestaurantId(uid(90));
    let schema = schema_over(
        vec![
            cart_row(1, restaurant, 10, Some(5), vec![line_3400()], 100),
            cart_row(2, restaurant, 11, None, vec![line_3400()], 200),
        ],
        restaurant,
    );

    let owner = schema
        .execute(
            Request::new(CART_Q)
                .variables(cart_vars(1))
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(5))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(owner.errors.is_empty(), "{:?}", owner.errors);
    assert_eq!(
        owner.data.into_json().unwrap()["cart"]["totalAmount"]["amountCents"],
        json!(3400),
        "the owner reads their cart, priced LIVE"
    );

    let stranger = schema
        .execute(
            Request::new(CART_Q)
                .variables(cart_vars(1))
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(6)))),
        )
        .await;
    assert!(stranger.errors.is_empty(), "{:?}", stranger.errors);
    assert_eq!(
        stranger.data.into_json().unwrap()["cart"],
        json!(null),
        "customer 6 requesting customer 5's cart by id gets null — the retired IDOR"
    );

    let session_cart_by_id = schema
        .execute(
            Request::new(CART_Q)
                .variables(cart_vars(2))
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(5))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(session_cart_by_id.errors.is_empty());
    assert_eq!(
        session_cart_by_id.data.into_json().unwrap()["cart"],
        json!(null),
        "an unbound session cart has no by-id reader below ADMIN (the guest path is `current`)"
    );

    let admin = schema
        .execute(Request::new(CART_Q).variables(cart_vars(1)).data(acting(RequestRole::Admin)).data(ReadScope::Admin))
        .await;
    assert!(admin.errors.is_empty(), "{:?}", admin.errors);
    assert_eq!(
        admin.data.into_json().unwrap()["cart"]["totalAmount"]["amountCents"],
        json!(3400),
        "ADMIN reads any cart, priced by the same one authority"
    );
}

/// Fail-closed pricing through the REAL path: a cart line whose offer is GONE from the live
/// catalog ERRORS the query — the customer sees no price, never a partial or wrong total
/// (technical_error classification; its counter twin lives in the spy-binary test).
#[tokio::test]
async fn an_unresolvable_line_errors_the_read_instead_of_lying() {
    let restaurant = ds::RestaurantId(uid(90));
    let gone = CartLineItem {
        cart_line_id: ds::CartLineId(uid(10)),
        offer_id: ds::OfferId(uid(99)), // not in the fixture menu
        quantity: 1,
        selected_option_ids: vec![],
    };
    let schema = schema_over(vec![cart_row(1, restaurant, 10, Some(5), vec![gone], 100)], restaurant);
    let resp = schema
        .execute(
            Request::new(CURRENT_Q)
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(5))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(
        !resp.errors.is_empty(),
        "an unresolvable price must ERROR the read, got data {:?}",
        resp.data.into_json()
    );
    assert!(
        resp.errors[0].message.contains("PriceUnresolvable"),
        "the typed code surfaces: {:?}",
        resp.errors[0].message
    );
}

/// A projection row whose `lines` jsonb does not match the repricing-input shape ERRORS the read
/// rather than rendering anything. This is the failure mode a bad fold or a half-applied migration
/// produces, and it is on a money path: the customer must see no price, never a cart silently
/// missing the lines that could not be parsed.
///
/// It is also the exit that used to sit BEFORE the `cart.price` span existed, so it returned an
/// error to the customer while the trace exported a clean success. The span placement itself is
/// pinned durably by the observability suite (#471); this test pins the customer-visible half —
/// that the read fails closed at all.
#[tokio::test]
async fn a_malformed_lines_row_errors_the_read_instead_of_rendering_a_partial_cart() {
    let restaurant = ds::RestaurantId(uid(90));
    let mut row = cart_row(1, restaurant, 10, Some(5), vec![line_3400()], 100);
    // Shaped like something, but not like CartLineItem — a quantity where the object should be.
    row.lines = json!([{ "cartLineId": "not-a-uuid", "quantity": "two" }]);
    let schema = schema_over(vec![row], restaurant);

    let resp = schema
        .execute(
            Request::new(CURRENT_Q)
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(5))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(
        !resp.errors.is_empty(),
        "malformed lines must ERROR the read, got data {:?}",
        resp.data.into_json()
    );
    assert!(
        resp.errors[0].message.contains("cart lines are malformed"),
        "the failure names its cause: {:?}",
        resp.errors[0].message
    );
}

// --- PROP-20260831-134539:547 slice 3a (D2/D4): the door-gated fold-priced read ----------------

/// The 1500-cent line: offer 20 alone (no options), quantity 1 — so `lineTotal == unit_price`
/// exactly, the number the fixture's price update in [`FakeAsOf::append_price_update`] targets.
fn line_1500() -> CartLineItem {
    CartLineItem {
        cart_line_id: ds::CartLineId(uid(12)),
        offer_id: ds::OfferId(uid(20)),
        quantity: 1,
        selected_option_ids: vec![],
    }
}

/// D2: the coordinate a bounded re-read reproduces is the SAME coordinate the mint returned — the
/// property `AsOfCatalog`'s whole design exists for (repricing at a fixed coordinate is stable
/// under later catalog changes). The mint's own coordinate is not on the reply (D3), so this test
/// knows it analytically: three seeded events -> head is coordinate 3, and nothing else appends
/// until AFTER the read below returns.
///
/// Also D5/beck NB6: `at_head` receives the REQUEST's own correlation id, not `Uuid::nil()` — both
/// fakes used to discard `_correlation_id`, so this went unnoticed everywhere. The test injects one
/// explicitly (`server::graphql_session::RequestCorrelationId`) and reads it back off the fake.
#[tokio::test]
async fn repricing_at_the_returned_coordinate_reproduces_the_returned_prices() {
    let restaurant = ds::RestaurantId(uid(90));
    let catalog_id = ds::CatalogId(uid(50));
    let fake = std::sync::Arc::new(FakeAsOf::seeded(catalog_id, restaurant));
    let schema = schema_over_with_door(
        vec![cart_row(1, restaurant, 10, Some(5), vec![line_1500()], 100)],
        restaurant,
        fake.clone(),
        true,
    );
    let request_correlation_id = uuid::Uuid::from_u128(0xC0FFEE);
    let resp = schema
        .execute(
            Request::new(CURRENT_Q)
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(5))))
                .data(tenant_of(restaurant))
                .data(server::graphql_session::RequestCorrelationId(request_correlation_id)),
        )
        .await;
    assert!(resp.errors.is_empty(), "no errors expected, got {:?}", resp.errors);
    let reply_price = resp.data.into_json().unwrap()["current"]["lines"][0]["lineTotal"]["amountCents"]
        .as_i64()
        .expect("a priced line");
    assert_eq!(reply_price, 1500, "the mint prices offer 20 at 15,00 EUR (head, coordinate 3)");

    // The coordinate the mint ACTUALLY used, read honestly off the fake (never off the reply --
    // D3 keeps it off the wire entirely).
    let minted_coordinate = fake.last_returned_coordinate();
    assert_eq!(minted_coordinate, 3, "three seeded events -> head coordinate 3");

    // The correlation id `at_head` ACTUALLY received is the request's own id, never `Uuid::nil()`.
    assert_eq!(
        fake.last_correlation_id(),
        Some(request_correlation_id),
        "the mint must carry the request's own correlation id, not a nil placeholder"
    );

    // The catalog changes AFTER the mint returned (a concurrent write, or simply time passing).
    fake.append_price_update(1900);

    // Repricing at the coordinate the mint used reproduces the SAME price the reply carried,
    // unaffected by the later change: the coordinate pins a REPLAYABLE prefix, not "whatever the
    // catalog says now".
    let coordinate = domain::catalog_as_of::CatalogVersion::try_new(minted_coordinate).unwrap();
    let as_of = fake.as_of(catalog_id, coordinate).await.expect("bounded re-read at the minted coordinate");
    let reprice = as_of.price_of(ds::OfferId(uid(20)), &[]).expect("offer resolves at the minted coordinate");
    assert_eq!(
        reprice.unit_price.amount_cents.0, reply_price,
        "repricing at the returned coordinate must reproduce the returned price"
    );
}

/// D2: the mint is ONE catalog read — one call to [`AsOfPriceAuthority::at_head`] per priced read,
/// never a second read to "get the coordinate" separately from "get the prices" (the exact
/// mixed-authority mint every lens's STOP forbids).
#[tokio::test]
async fn the_priced_read_performs_exactly_one_catalog_read() {
    let restaurant = ds::RestaurantId(uid(90));
    let catalog_id = ds::CatalogId(uid(50));
    let fake = std::sync::Arc::new(FakeAsOf::seeded(catalog_id, restaurant));
    let schema = schema_over_with_door(
        vec![cart_row(1, restaurant, 10, Some(5), vec![line_1500()], 100)],
        restaurant,
        fake.clone(),
        true,
    );
    let resp = schema
        .execute(
            Request::new(CURRENT_Q)
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(5))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(resp.errors.is_empty(), "no errors expected, got {:?}", resp.errors);
    assert_eq!(
        fake.reads.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the mint must call at_head exactly once per priced read"
    );
}

/// D4: with the door CLOSED, the priced read is the projection read, PERIOD — the fold-priced
/// authority is never touched (`fake.reads` stays 0), and the price comes from `TreeCatalogs`'
/// projection fixture (15,00 EUR for offer 20), never from the fold's stream (which this test
/// seeds with a DIFFERENT price to make a fold-shaped answer detectable).
#[tokio::test]
async fn with_the_door_closed_the_priced_read_is_the_projection_read_and_carries_no_coordinate() {
    let restaurant = ds::RestaurantId(uid(90));
    let catalog_id = ds::CatalogId(uid(50));
    let fake = std::sync::Arc::new(FakeAsOf::seeded(catalog_id, restaurant));
    // If the closed arm ever fell through to the fold, THIS price (1900) would leak into the
    // reply instead of the projection's 1500 -- a decoy, not merely an unread value.
    fake.append_price_update(1900);
    let schema = schema_over_with_door(
        vec![cart_row(1, restaurant, 10, Some(5), vec![line_1500()], 100)],
        restaurant,
        fake.clone(),
        false, // CLOSED
    );
    let resp = schema
        .execute(
            Request::new(CURRENT_Q)
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(5))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(resp.errors.is_empty(), "no errors expected, got {:?}", resp.errors);
    let data = resp.data.into_json().unwrap();
    assert_eq!(
        data["current"]["lines"][0]["lineTotal"]["amountCents"],
        json!(1500),
        "the closed arm prices from the PROJECTION (TreeCatalogs), never the fold's stream"
    );
    assert_eq!(
        fake.reads.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "the closed arm must never call at_head -- it is the projection read, not a fold fallback"
    );
}

/// beck (ii): `priced` actually MINTS -- the OPEN arm's `quote` field is non-null, decodes and
/// signature-verifies under a `QuoteVerifier` holding the SAME key `test_minter()` mints under,
/// and binds the coordinate `at_head` actually returned. Mutant: `cart_read::priced`'s `quote:`
/// field stays hardcoded `None` on the OPEN arm -- expected red: `quote` is `null`.
#[tokio::test]
async fn priced_mints_a_verifiable_quote_on_the_open_arm() {
    let restaurant = ds::RestaurantId(uid(90));
    let catalog_id = ds::CatalogId(uid(50));
    let fake = std::sync::Arc::new(FakeAsOf::seeded(catalog_id, restaurant));
    let schema = schema_over_with_door(
        vec![cart_row(1, restaurant, 10, Some(5), vec![line_1500()], 100)],
        restaurant,
        fake.clone(),
        true, // OPEN
    );
    let resp = schema
        .execute(
            Request::new(CURRENT_WITH_QUOTE_Q)
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(5))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(resp.errors.is_empty(), "no errors expected, got {:?}", resp.errors);
    let data = resp.data.into_json().unwrap();
    let quote = data["current"]["quote"].as_str().expect("a non-null quote on the OPEN arm").to_string();
    let verifier = application::quote::QuoteVerifier::new(
        application::quote::SigningKey::from_resolved_secret("test-key", "test-signing-secret"),
        None,
    );
    let verified = verifier
        .decode_and_check_signature(&domain::generated::scalars::CartQuote(quote), chrono::Utc::now())
        .expect("the minted quote verifies cleanly under the SAME key");
    assert_eq!(verified.cart_id, uid(1), "quote binds the cart it was minted for");
    assert_eq!(verified.catalog_version, fake.last_returned_coordinate(), "quote binds the coordinate the fold actually returned");
}

/// beck (ii): the CLOSED arm mints nothing -- `quote` is `null` (one of `CartQuote`'s three
/// documented null causes, scalars.yaml#/CartQuote). Mutant: `priced` mints unconditionally
/// (ignoring `door`) -- expected red: `quote` is non-null with the door closed.
#[tokio::test]
async fn priced_mints_nothing_on_the_closed_arm() {
    let restaurant = ds::RestaurantId(uid(90));
    let catalog_id = ds::CatalogId(uid(50));
    let fake = std::sync::Arc::new(FakeAsOf::seeded(catalog_id, restaurant));
    let schema = schema_over_with_door(
        vec![cart_row(1, restaurant, 10, Some(5), vec![line_1500()], 100)],
        restaurant,
        fake.clone(),
        false, // CLOSED
    );
    let resp = schema
        .execute(
            Request::new(CURRENT_WITH_QUOTE_Q)
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(5))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(resp.errors.is_empty(), "no errors expected, got {:?}", resp.errors);
    let data = resp.data.into_json().unwrap();
    assert!(data["current"]["quote"].is_null(), "the closed arm must mint no quote");
}

/// D-L, RED-FIRST (ADR-20260906-192007:34): the `carts` LIST resolver must never open the
/// fold-priced door, even when `RUN_FOLD_PRICED_CART_READ` is ON -- structurally, via
/// `priced_list`'s witness-less signature, never by convention. Two carts, same customer, same
/// restaurant, door TRUE, fold seeded to answer 19,00 EUR if it were EVER consulted (a decoy
/// unreachable from the projection). Mutant (per the coordinator's addition): pass the door witness
/// from the `carts` arm in the emitter literal, i.e. make `carts` call `priced` with `Some(door)`
/// instead of `priced_list`'s hard `None` -- expected red: either lineTotal 1900 where 1500 is
/// expected, or `fake.reads` 2 where 0 is expected (both carts would fold).
#[tokio::test]
async fn the_carts_list_never_opens_the_fold_even_with_the_door_on() {
    let restaurant = ds::RestaurantId(uid(90));
    let catalog_id = ds::CatalogId(uid(50));
    let fake = std::sync::Arc::new(FakeAsOf::seeded(catalog_id, restaurant));
    // If the fan-out ever fell through to the fold, THIS price (1900) would leak into the reply
    // instead of the projection's 1500 -- a decoy, not merely an unread value (same idiom as
    // `with_the_door_closed_...` above, but here the DOOR IS ON).
    fake.append_price_update(1900);
    let schema = schema_over_with_door(
        vec![
            cart_row(1, restaurant, 10, Some(5), vec![line_1500()], 100),
            cart_row(2, restaurant, 11, Some(5), vec![line_1500()], 200),
        ],
        restaurant,
        fake.clone(),
        true, // ON -- the carts fan-out must ignore it structurally (D-L), not by convention
    );
    let resp = schema
        .execute(
            Request::new(CARTS_Q)
                .variables(Variables::from_json(json!({ "customerId": uid(5) })))
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(5))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(resp.errors.is_empty(), "no errors expected, got {:?}", resp.errors);
    let data = resp.data.into_json().unwrap();
    let carts = data["carts"].as_array().expect("carts array");
    assert_eq!(carts.len(), 2, "both carts belong to customer 5: {carts:?}");
    for c in carts {
        assert_eq!(
            c["lines"][0]["lineTotal"]["amountCents"],
            json!(1500),
            "the carts list must price from the live catalog PROJECTION, never the fold's decoy: {c:?}"
        );
    }
    assert_eq!(
        fake.reads.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "the carts fan-out must NEVER call AsOfPriceAuthority::at_head, door open or not (D-L)"
    );
}

/// beck (ii): `priced_list` mints NOTHING, door open or not -- structurally, since its own
/// signature carries no witness (D-L). Mutant: `priced_list` passes `Some(door)` into `priced`
/// instead of its hard `None` -- expected red: `quote` is non-null on a `carts` row.
#[tokio::test]
async fn the_carts_list_mints_no_quote_even_with_the_door_on() {
    let restaurant = ds::RestaurantId(uid(90));
    let catalog_id = ds::CatalogId(uid(50));
    let fake = std::sync::Arc::new(FakeAsOf::seeded(catalog_id, restaurant));
    let schema = schema_over_with_door(
        vec![cart_row(1, restaurant, 10, Some(5), vec![line_1500()], 100)],
        restaurant,
        fake.clone(),
        true, // ON
    );
    let resp = schema
        .execute(
            Request::new(CARTS_WITH_QUOTE_Q)
                .variables(Variables::from_json(json!({ "customerId": uid(5) })))
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(5))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(resp.errors.is_empty(), "no errors expected, got {:?}", resp.errors);
    let data = resp.data.into_json().unwrap();
    let carts = data["carts"].as_array().expect("carts array");
    assert_eq!(carts.len(), 1);
    assert!(carts[0]["quote"].is_null(), "the carts fan-out must mint no quote, structurally");
}

/// D2: the mixed-authority mutant this whole slice exists to forbid — `at_head` called (so the fold
/// IS read) but priced from the PROJECTION anyway (beck mutant C: `price_cart(&snapshot, ..)`
/// instead of `price_cart_at(&snapshot, &as_of, ..)`). Every OTHER door-OPEN test seeds `FakeAsOf`
/// and `TreeCatalogs` at the SAME 1500, so the two authorities never disagree and this mutant would
/// survive them all; this test is the mirror of
/// [`with_the_door_closed_the_priced_read_is_the_projection_read_and_carries_no_coordinate`], with
/// the decoy price on the FOLD instead of the projection — the open arm must show the FOLD's 1900,
/// never the projection's 1500.
#[tokio::test]
async fn the_open_arm_prices_from_the_fold_never_the_projection() {
    let restaurant = ds::RestaurantId(uid(90));
    let catalog_id = ds::CatalogId(uid(50));
    let fake = std::sync::Arc::new(FakeAsOf::seeded(catalog_id, restaurant));
    // The fold's price moves to 1900 BEFORE the read -- the projection (`TreeCatalogs`) still
    // reports 1500, so a mint that mixes authorities (fold coordinate + projection price) is
    // directly observable as a leaked 1500 instead of the fold's 1900.
    fake.append_price_update(1900);
    let schema = schema_over_with_door(
        vec![cart_row(1, restaurant, 10, Some(5), vec![line_1500()], 100)],
        restaurant,
        fake.clone(),
        true, // OPEN
    );
    let resp = schema
        .execute(
            Request::new(CURRENT_Q)
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(5))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(resp.errors.is_empty(), "no errors expected, got {:?}", resp.errors);
    let data = resp.data.into_json().unwrap();
    assert_eq!(
        data["current"]["lines"][0]["lineTotal"]["amountCents"],
        json!(1900),
        "the open arm must price from the FOLD (1900), never the projection (1500) -- the \
         mixed-authority mint every lens's STOP forbids"
    );
    assert_eq!(
        fake.last_returned_coordinate(),
        4,
        "the fourth event (the price update) is the fold's head coordinate"
    );
}

/// D4: a fold failure with the door OPEN is a `technical_error` (the read ERRORS), NEVER a
/// HEAD/projection fallback. `FailingAsOf` always refuses -- the catalog "does not exist" from the
/// fold's point of view -- and `TreeCatalogs` still projects a perfectly good 15,00 EUR row, so any
/// leak into the reply (a fallback) is directly observable as a priced, error-free response.
#[tokio::test]
async fn a_fold_failure_with_the_door_open_is_a_technical_error_never_a_head_price() {
    let restaurant = ds::RestaurantId(uid(90));
    let schema = schema_over_with_door(
        vec![cart_row(1, restaurant, 10, Some(5), vec![line_1500()], 100)],
        restaurant,
        std::sync::Arc::new(FailingAsOf),
        true, // OPEN
    );
    let resp = schema
        .execute(
            Request::new(CURRENT_Q)
                .data(acting(RequestRole::Customer))
                .data(ReadScope::Customer(ds::CustomerId(uid(5))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(
        !resp.errors.is_empty(),
        "a fold failure must ERROR the read, never fall back to a price, got data {:?}",
        resp.data.into_json()
    );
    // The classification its own name promises: `technical_error`, never the business-shaped
    // `PriceUnresolvable` rejection (`:1103`'s pattern, mirrored) — a fold failure is a repository
    // failure, not a business decision that this specific offer is unresolvable.
    assert!(
        !resp.errors[0].message.contains("PriceUnresolvable"),
        "a fold failure is technical_error, never the business PriceUnresolvable classification: {:?}",
        resp.errors[0].message
    );
}

// --- the PATH-level test (#469) ----------------------------------------------------------------
//
// Everything above executes the SCHEMA and injects `ReadScope` (and now `TenantScope`) by hand.
// That is the right shape for the pricing and lookup semantics — and it is exactly why the two
// #469 defects survived a green suite: nothing exercised
// `POST /{role}/graphql -> authorize -> resolve_read_scope -> resolve_tenant -> resolver`, so a
// `/public` path that returned `Principal::anonymous()` WITHOUT reading the cookie, and a claim leg
// bounded by nothing, both looked fine.
//
// The rule this section adopts, stated where the next person will read it: **a test of an
// auth-derived value may not `.data()` that value.** These drive a real HTTP request through the
// production router — cookie, `Host`, JWKS verification and all — and inject nothing.

/// TEST-ONLY ES256 keypair. Deliberately the SAME material as `crates/server/src/auth.rs`'s unit
/// fixture and duplicated rather than shared: `#[cfg(test)]` items in the lib are invisible to an
/// integration test, and the alternatives (a public test-support module, or a cargo feature) would
/// put a signing fixture in the crate's PUBLIC API to save four lines. Never a deployed key.
const TEST_EC_PRIVATE_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgTCTNdGfegiVKVsm+
vZXPOa4xJAt5OT8zMSblfCEwtW2hRANCAARto0Dk75fxl2IyLx89vwvjUWkJAb/p
5bKnk8sNetDUBHLVIGpXoxBRFJVNSeDN6QB9IHl6rqDLaZR4iqLatScL
-----END PRIVATE KEY-----
";

/// The public half of that key, as the JWKS the verifier fetches.
fn jwks_body() -> serde_json::Value {
    json!({"keys":[{"kty":"EC","crv":"P-256","use":"sig","kid":"captain-test-es256","alg":"ES256",
        "x":"baNA5O-X8ZdiMi8fPb8L41FpCQG_6eWyp5PLDXrQ1AQ","y":"ctUgalejEFEUlU1J4M3pAH0geXquoMtplHiKotq1Jws"}]})
}

/// A loopback JWKS endpoint, so `AuthContext` is built exactly as production builds it
/// (`from_config`) instead of through a test-only door into its private cache. Returns the URL.
async fn jwks_endpoint() -> String {
    let app = axum::Router::new().route("/jwks", axum::routing::get(|| async { axum::Json(jwks_body()) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}/jwks")
}

/// OUR identity project. `iss` is MANDATORY since #519 -- this fixture used to mint tokens with no
/// `iss` at all, against a verifier built with an empty `SUPABASE_URL`.
const TEST_SUPABASE_URL: &str = "https://captain-under-test.supabase.co";

/// The credential a signed-in storefront customer's browser actually sends: a Supabase-shaped JWT
/// whose `sub` is the auth subject and whose `app_metadata.captain_food.customer_id` is the DOMAIN
/// id — two DIFFERENT uuids, so an implementation deriving identity from `sub` cannot pass.
fn signed_customer_jwt(sub: uuid::Uuid, customer: uuid::Uuid) -> String {
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::ES256);
    header.kid = Some("captain-test-es256".into());
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs()
        + 3600;
    let claims = json!({
        "sub": sub.to_string(),
        "aud": "authenticated",
        "iss": format!("{TEST_SUPABASE_URL}/auth/v1"),
        "exp": exp,
        "app_metadata": { "captain_food": { "role": "CUSTOMER", "customer_id": customer.to_string() } },
    });
    let key = jsonwebtoken::EncodingKey::from_ec_pem(TEST_EC_PRIVATE_KEY_PEM.as_bytes())
        .expect("test EC key parses");
    jsonwebtoken::encode(&header, &claims, &key).expect("sign test JWT")
}

/// Two storefronts on two hosts, over ONE read model — the multi-tenant shape the bug lives in.
#[derive(Clone)]
struct TwoRestaurants(RestaurantRow, RestaurantRow);

#[async_trait]
impl RestaurantReadRepository for TwoRestaurants {
    async fn list(&self, _f: RestaurantFilter) -> Result<Vec<RestaurantRow>, DomainError> {
        Ok(vec![self.0.clone(), self.1.clone()])
    }
    /// The host resolution the edge performs — slug in, restaurant out. Unknown slugs are absent.
    async fn by_slug(&self, s: ds::Slug) -> Result<Option<RestaurantRow>, DomainError> {
        Ok([&self.0, &self.1].into_iter().find(|r| r.slug.as_ref() == Some(&s)).cloned())
    }
    async fn by_id(&self, id: ds::RestaurantId) -> Result<Option<RestaurantRow>, DomainError> {
        Ok([&self.0, &self.1].into_iter().find(|r| r.restaurant_id == id).cloned())
    }
    async fn by_previous_slug(&self, _s: ds::Slug) -> Result<Option<RestaurantRow>, DomainError> {
        Ok(None)
    }
}

/// A catalog store over several restaurants (each storefront prices from its own menu).
struct ManyCatalogs(Vec<CatalogRow>);

#[async_trait]
impl CatalogReadRepository for ManyCatalogs {
    async fn by_restaurant(&self, id: ds::RestaurantId) -> Result<Option<CatalogRow>, DomainError> {
        Ok(self.0.iter().find(|c| c.restaurant_id == id).cloned())
    }
}

fn named_restaurant(id: ds::RestaurantId, slug: &str) -> RestaurantRow {
    let mut row = restaurant_row(id.0);
    row.slug = Some(ds::Slug(slug.into()));
    row
}

/// The production router: the real `graphql_routes` + the real JWT verifier + the real tenant
/// lookup. Nothing here is a test seam — the request below is the browser's request.
async fn storefront_router(carts: Vec<CartRow>) -> axum::Router {
    let a = named_restaurant(ds::RestaurantId(uid(90)), "resto-a");
    let b = named_restaurant(ds::RestaurantId(uid(91)), "resto-b");
    let restaurants = Arc::new(TwoRestaurants(a.clone(), b.clone()));
    let schema = build_schema(
        Some(ReadDeps {
            restaurants: restaurants.clone(),
            prospection: Arc::new(Empty),
            pricing_policy: Arc::new(Empty),
            uber_estimation_policy: Arc::new(Empty),
            uber_split_policy: Arc::new(Empty),
            catalogs: Arc::new(ManyCatalogs(vec![
                catalog_row(a.restaurant_id),
                catalog_row(b.restaurant_id),
            ])),
            carts: Arc::new(MemCarts(carts)),
            orders: Arc::new(Empty),
            order_conversations: Arc::new(Empty),
            customers: Arc::new(Empty),
            deliveries: Arc::new(Empty),
            rider_restrictions: Arc::new(Empty),
            rider_roster: Arc::new(Empty),
            member_authority: Arc::new(Empty),
            restaurant_roster: Arc::new(Empty),
            restaurant_invitations: Arc::new(Empty),
            refunds: Arc::new(Empty),
            delivery_satisfaction: Arc::new(Empty),
            delivery_partner_availabilities: Arc::new(Empty),
            reclamations: Arc::new(Empty),
            customer_credit: Arc::new(Empty),
            mailbox_lanes: Arc::new(Empty),
        // RSO-1: the spec-default horizon (900 s) -- tests assert behaviour, not config.
        service_window_horizon: Default::default(),
        support_contact: None,
        run_rider_restriction_door: server::graphql_schema::RunRiderRestrictionDoor(false),
        as_of_price_authority: Arc::new(Empty),
        run_fold_priced_cart_read: server::graphql_schema::RunFoldPricedCartRead(false),
        quote_minter: test_minter(),
        }),
        None,
        None,
    );
    server::graphql_routes(
        schema,
        server::TenantLookup(Some(restaurants)),
        claim_only_sources(),
    )
        .layer(axum::Extension(server::AuthContext::from_config(
            jwks_endpoint().await,
            TEST_SUPABASE_URL.into(),
        )))
}

/// `POST /public/graphql` as the storefront sends it: the anonymous role path, the `captain_auth`
/// cookie (the browser's ONLY credential — no Authorization header exists on this request), and the
/// tenant carried where it belongs, in the `Host`.
async fn post_current(router: axum::Router, host: &str, jwt: Option<&str>) -> serde_json::Value {
    use tower::ServiceExt;
    let mut req = axum::http::Request::builder()
        .method("POST")
        .uri("/public/graphql")
        .header(axum::http::header::HOST, host)
        .header(axum::http::header::CONTENT_TYPE, "application/json");
    if let Some(jwt) = jwt {
        req = req.header(axum::http::header::COOKIE, format!("captain_auth={jwt}"));
    }
    let body = json!({ "query": CURRENT_Q }).to_string();
    let response = router
        .oneshot(req.body(axum::body::Body::from(body)).expect("request builds"))
        .await
        .expect("router answers");
    assert_eq!(response.status(), axum::http::StatusCode::OK, "the open path always answers 200");
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.expect("body");
    serde_json::from_slice(&bytes).expect("json response")
}

/// **The test the suite structurally could not express** (#469). One customer, two open carts at
/// two restaurants, one browser: each STOREFRONT HOST serves its own cart, priced from its own
/// menu — and the identity comes from the cookie the browser sends, not from a `.data()` call.
///
/// This is red on `main` twice over: leg 1 never fired (so both hosts answered `null`), and had it
/// fired it was bounded by nothing (so both hosts would have answered with whichever cart was
/// touched last). The assertion is on the PRICED payload — `restaurantId` and the line total —
/// because the harm in this issue is being priced for restaurant A's cart on restaurant B.
#[tokio::test]
async fn the_storefront_host_decides_which_of_the_customers_carts_is_served_and_priced() {
    let customer = uid(5);
    let jwt = signed_customer_jwt(uid(0xA0), customer);
    // Two OPEN carts: A's is older, B's newer — so "newest OPEN anywhere" would serve B on BOTH.
    let carts = vec![
        cart_row(1, ds::RestaurantId(uid(90)), 10, Some(5), vec![line_3400()], 100),
        cart_row(2, ds::RestaurantId(uid(91)), 10, Some(5), vec![line_1700()], 300),
    ];

    let on_a =
        post_current(storefront_router(carts.clone()).await, "resto-a.captain.food", Some(&jwt)).await;
    let cart_a = &on_a["data"]["current"];
    assert!(on_a.get("errors").is_none(), "unexpected errors: {on_a}");
    assert_eq!(cart_a["id"], json!(uid(1).to_string()), "resto-a serves A's cart: {on_a}");
    assert_eq!(cart_a["restaurantId"], json!(uid(90).to_string()), "priced AS resto-a's cart");
    assert_eq!(cart_a["totalAmount"]["amountCents"], json!(3400), "A's total, not B's");
    assert_eq!(cart_a["lines"][0]["lineTotal"]["amountCents"], json!(3400));

    let on_b = post_current(storefront_router(carts).await, "resto-b.captain.food", Some(&jwt)).await;
    let cart_b = &on_b["data"]["current"];
    assert_eq!(cart_b["id"], json!(uid(2).to_string()), "resto-b serves B's cart: {on_b}");
    assert_eq!(cart_b["restaurantId"], json!(uid(91).to_string()));
    assert_eq!(cart_b["totalAmount"]["amountCents"], json!(1700), "B's total, not A's");
}

/// The DECOY-FREE case at the path level (mob, testing lens 3): with a cart at resto-A **only**,
/// resto-B's storefront serves `null`. An implementation of "this host's cart, else the newest
/// anywhere" passes the two-cart test above and fails here — which is the whole point of it.
#[tokio::test]
async fn a_cart_at_another_storefront_is_never_served_as_a_fallback() {
    let jwt = signed_customer_jwt(uid(0xA0), uid(5));
    let carts = vec![cart_row(1, ds::RestaurantId(uid(90)), 10, Some(5), vec![line_3400()], 100)];

    let on_b = post_current(storefront_router(carts).await, "resto-b.captain.food", Some(&jwt)).await;
    assert!(on_b.get("errors").is_none(), "unexpected errors: {on_b}");
    assert_eq!(on_b["data"]["current"], json!(null), "no cart HERE is null, never A's cart: {on_b}");
}

/// The marketplace host names no restaurant, so `current` is null there — decided explicitly
/// (#469), not "newest anywhere" wearing a fallback's clothes. Same request, same cookie, same
/// carts; only the `Host` differs.
#[tokio::test]
async fn the_marketplace_host_serves_no_cart_at_all() {
    let jwt = signed_customer_jwt(uid(0xA0), uid(5));
    let carts = vec![cart_row(1, ds::RestaurantId(uid(90)), 10, Some(5), vec![line_3400()], 100)];

    for host in ["live.captain.food", "nobody.captain.food"] {
        let resp = post_current(storefront_router(carts.clone()).await, host, Some(&jwt)).await;
        assert!(resp.get("errors").is_none(), "{host}: unexpected errors: {resp}");
        assert_eq!(resp["data"]["current"], json!(null), "{host}: no tenant in the host, no cart");
    }
}

/// The credential is what makes leg 1 fire: the SAME request without the cookie — and with no
/// `X-SESSION-ID` either — is anonymous, and an anonymous browser sees no bound cart. Keeps the
/// test above honest about WHY it passes (it is not the host alone doing the work).
#[tokio::test]
async fn without_the_cookie_the_same_request_is_anonymous() {
    let carts = vec![cart_row(1, ds::RestaurantId(uid(90)), 10, Some(5), vec![line_3400()], 100)];
    let resp = post_current(storefront_router(carts).await, "resto-a.captain.food", None).await;
    assert!(resp.get("errors").is_none(), "unexpected errors: {resp}");
    assert_eq!(resp["data"]["current"], json!(null), "no credential, no claim leg: {resp}");
}

/// The RIDER seam these tests do not exercise: a table with no rows, so every rider subject is
/// nobody (fail closed). `IdentitySources` refuses to be built without one on purpose.
struct NoRiderRows;

#[async_trait::async_trait]
impl server::ResolveRiderIdentity for NoRiderRows {
    async fn resolve(&self, _auth_subject: &str) -> server::RiderIdentityResolution {
        server::RiderIdentityResolution::NoMapping
    }
}

fn claim_only_sources() -> server::IdentitySources {
    server::IdentitySources {
        customer: server::CustomerIdentitySource::Claim,
        rider: server::RiderIdentitySource::new(std::sync::Arc::new(NoRiderRows)),
        member: server::MemberIdentitySource::new(std::sync::Arc::new(server::NoDatabaseMemberIdentity)),
            platform: server::PlatformIdentitySource::new(std::sync::Arc::new(server::NoDatabasePlatformIdentity)),
    }
}
