//! #451 Phase 2 — the priced cart READ through the REAL GraphQL path (schema → generated
//! resolver → `cart_read` seam → `price_cart` over the one-read catalog snapshot):
//! - `current` two-leg resolution (ADR-20260810-120531): claim leg, session leg, and the
//!   NULL-or-claim ownership filter between them;
//! - the by-id `cart` claim-ownership narrowing (#144/#434 — the dispatch DONE-WHEN);
//! - the priced shape: a cart of (offer 15,00 EUR + option 2,00 EUR) × 2 answers 34,00 EUR —
//!   never the pre-#451 fabricated 0,00 EUR.
//!
//! Beck red-first: written BEFORE the resolvers were wired. Seen RED as: `current` errored
//! "not implemented" (both legs), the stranger's by-id read returned the FULL cart (the IDOR's
//! last breath — spec-gated but not yet body-enforced), and the admin's by-id read priced to 0.

use async_graphql::{Request, Variables};
use async_trait::async_trait;
use domain::generated::entities::{CartLineItem, Offer, OptionList, Product, ProductItemOption, TaxRate};
use domain::generated::scalars as ds;
use domain::shared::errors::DomainError;
use serde_json::json;
use std::sync::Arc;

use application::queries::{
    CartReadRepository, CartRow, CatalogReadRepository, CatalogRow, CustomerReadRepository,
    CustomerRow, DeliveryPartnerAvailabilityFilter, DeliveryPartnerAvailabilityRow,
    DeliverySatisfactionRow, MailboxLaneRow, OrderConversationRow, OrderFilter,
    PoisonedMessageRow, PricingPolicyRow, ProspectFilter, ProspectionPipelineRow,
    ProspectionReadRepository, PricingPolicyReadRepository, ReadScope, ReclamationFilter,
    ReclamationRow, RefundFilter, RefundRow, RestaurantFilter, RestaurantReadRepository,
    RestaurantRow, UberEstimationPolicyReadRepository, UberEstimationPolicyRow,
    UberSplitPolicyReadRepository, UberSplitPolicyRow,
};
use application::projections::OrderTrackingRow;
use server::graphql_acl::RequestRole;
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
    async fn by_id(&self, id: ds::CartId) -> Result<Option<CartRow>, DomainError> {
        Ok(self.0.iter().find(|r| r.cart_id == id).cloned())
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
    async fn by_restaurant(
        &self,
        _r: ds::RestaurantId,
        _s: Option<ds::DeliveryStatus>,
    ) -> Result<Vec<application::queries::DeliveryJobRow>, DomainError> {
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
impl application::queries::MailboxLaneRepository for Empty {
    async fn list(&self) -> Result<Vec<MailboxLaneRow>, DomainError> {
        Ok(vec![])
    }
    async fn poisoned(
        &self,
        _actor_type: Option<String>,
        _limit: i64,
    ) -> Result<Vec<PoisonedMessageRow>, DomainError> {
        Ok(vec![])
    }
}

// --- harness -----------------------------------------------------------------------------------

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
            refunds: Arc::new(Empty),
            delivery_satisfaction: Arc::new(Empty),
            delivery_partner_availabilities: Arc::new(Empty),
            reclamations: Arc::new(Empty),
            customer_credit: Arc::new(Empty),
            mailbox_lanes: Arc::new(Empty),
        }),
        None,
        None,
    )
}

const CURRENT_Q: &str = "query { current { id status totalAmount { amountCents currency } \
                         breakdown { total { amountCents } } \
                         lines { quantity lineTotal { amountCents } } } }";
const CART_Q: &str = "query($id: CartId!) { cart(input: { id: $id }) { id totalAmount { amountCents } } }";

fn cart_vars(cart: u8) -> Variables {
    Variables::from_json(json!({ "id": uid(cart) }))
}

// --- tests ---------------------------------------------------------------------------------------

/// Leg 1 through the REAL path: an identified customer's bound cart prices to 34,00 EUR — the
/// known-price fixture (offer 15,00 + option 2,00, × 2) — with breakdown.total identical.
/// SEEN RED: `current` answered "not implemented".
#[tokio::test]
async fn leg1_the_customers_bound_cart_prices_live_to_3400() {
    let restaurant = ds::RestaurantId(uid(90));
    let schema = schema_over(vec![cart_row(1, restaurant, 10, Some(5), vec![line_3400()], 100)], restaurant);
    let resp = schema
        .execute(
            Request::new(CURRENT_Q)
                .data(RequestRole::Customer)
                .data(ReadScope::Customer(ds::CustomerId(uid(5)))),
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
/// SEEN RED: "not implemented".
#[tokio::test]
async fn leg2_an_anonymous_session_resolves_its_own_cart_and_only_its_own() {
    let restaurant = ds::RestaurantId(uid(90));
    let schema = schema_over(vec![cart_row(1, restaurant, 10, None, vec![line_3400()], 100)], restaurant);

    let own = schema
        .execute(Request::new(CURRENT_Q).data(RequestRole::Public).data(SessionHeader(Some(uid(10)))))
        .await;
    assert!(own.errors.is_empty(), "no errors expected, got {:?}", own.errors);
    let data = own.data.into_json().unwrap();
    assert_eq!(data["current"]["totalAmount"]["amountCents"], json!(3400));

    let other = schema
        .execute(Request::new(CURRENT_Q).data(RequestRole::Public).data(SessionHeader(Some(uid(11)))))
        .await;
    assert!(other.errors.is_empty());
    assert_eq!(other.data.into_json().unwrap()["current"], json!(null), "session B sees nothing");

    let headerless = schema.execute(Request::new(CURRENT_Q).data(RequestRole::Public)).await;
    assert!(headerless.errors.is_empty());
    assert_eq!(headerless.data.into_json().unwrap()["current"], json!(null));
}

/// Leg 2's NULL-or-claim filter through the REAL path: once the cart is BOUND to customer 5, an
/// anonymous replay of the same session id sees null — the session id is a correlator, not an
/// identity. SEEN RED: "not implemented".
#[tokio::test]
async fn leg2_a_bound_cart_is_invisible_to_an_anonymous_session_replay() {
    let restaurant = ds::RestaurantId(uid(90));
    let schema = schema_over(vec![cart_row(1, restaurant, 10, Some(5), vec![line_3400()], 100)], restaurant);
    let resp = schema
        .execute(Request::new(CURRENT_Q).data(RequestRole::Public).data(SessionHeader(Some(uid(10)))))
        .await;
    assert!(resp.errors.is_empty());
    assert_eq!(resp.data.into_json().unwrap()["current"], json!(null));
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
                .data(RequestRole::Customer)
                .data(ReadScope::Customer(ds::CustomerId(uid(5)))),
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
/// SEEN RED: the stranger got the FULL cart (body not yet enforcing the spec's narrowing) and
/// the admin's read priced to 0.
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
                .data(RequestRole::Customer)
                .data(ReadScope::Customer(ds::CustomerId(uid(5)))),
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
                .data(RequestRole::Customer)
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
                .data(RequestRole::Customer)
                .data(ReadScope::Customer(ds::CustomerId(uid(5)))),
        )
        .await;
    assert!(session_cart_by_id.errors.is_empty());
    assert_eq!(
        session_cart_by_id.data.into_json().unwrap()["cart"],
        json!(null),
        "an unbound session cart has no by-id reader below ADMIN (the guest path is `current`)"
    );

    let admin = schema
        .execute(Request::new(CART_Q).variables(cart_vars(1)).data(RequestRole::Admin).data(ReadScope::Admin))
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
                .data(RequestRole::Customer)
                .data(ReadScope::Customer(ds::CustomerId(uid(5)))),
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
