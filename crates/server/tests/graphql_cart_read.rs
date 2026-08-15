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
        // RSO-1: the spec-default horizon (900 s) -- tests assert behaviour, not config.
        service_window_horizon: Default::default(),
        }),
        None,
        None,
    )
}

const CURRENT_Q: &str = "query { current { id restaurantId status totalAmount { amountCents currency } \
                         breakdown { total { amountCents } } \
                         lines { quantity lineTotal { amountCents } } } }";
const CART_Q: &str = "query($id: CartId!) { cart(input: { id: $id }) { id totalAmount { amountCents } } }";

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
                .data(RequestRole::Customer)
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
                .data(RequestRole::Public)
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
                .data(RequestRole::Public)
                .data(SessionHeader(Some(uid(11))))
                .data(tenant_of(restaurant)),
        )
        .await;
    assert!(other.errors.is_empty());
    assert_eq!(other.data.into_json().unwrap()["current"], json!(null), "session B sees nothing");

    let headerless = schema
        .execute(Request::new(CURRENT_Q).data(RequestRole::Public).data(tenant_of(restaurant)))
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
                .data(RequestRole::Public)
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
                .data(RequestRole::Customer)
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
                .data(RequestRole::Customer)
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
                .data(RequestRole::Customer)
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
                .data(RequestRole::Customer)
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
                .data(RequestRole::Customer)
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
            refunds: Arc::new(Empty),
            delivery_satisfaction: Arc::new(Empty),
            delivery_partner_availabilities: Arc::new(Empty),
            reclamations: Arc::new(Empty),
            customer_credit: Arc::new(Empty),
            mailbox_lanes: Arc::new(Empty),
        // RSO-1: the spec-default horizon (900 s) -- tests assert behaviour, not config.
        service_window_horizon: Default::default(),
        }),
        None,
        None,
    );
    server::graphql_routes(schema, server::TenantLookup(Some(restaurants)))
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
