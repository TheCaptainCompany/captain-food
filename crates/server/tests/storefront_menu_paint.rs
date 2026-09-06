//! #749 — the storefront MENU, end to end through the REAL composed schema (beck's missing test).
//!
//! The defect: the router feeds the storefront paint a `slug` param, but the `catalog` query
//! required `restaurantId!` — so the menu read failed GraphQL validation on EVERY paint (SSR and
//! hydrate) and the LIVE MENU never rendered from a real paint. #745 made the failure a declared
//! §25b skip (traceable, uncounted) — but the menu was still missing until the fix this file
//! pins: an optional `restaurantSlug` selector on `catalog`, DSL-declared EXACTLY-ONE-OF with
//! `restaurantId`, resolved through the same slug path as the tenant host (aliases included).
//!
//! RED EVIDENCE (seen before the fix, this branch): the paint test below failed with the menu
//! absent from the HTML — the read was a declared structural skip, so the paint performed only
//! the `restaurant.bySlug` read and rendered the shell around a dead menu; the one-of tests
//! failed schema validation (`Unknown field "restaurantSlug"`). Verbatim reds are in the
//! introducing commit message and the PR body.
//!
//! These tests execute the renderer's ACTUAL query documents through the in-process
//! [`server::SsrExec`] transport (what production SSR serves), against a schema seeded with one
//! restaurant + one catalog item — no FakeTransport anywhere on the read path.

use async_graphql::{Request, Variables};
use async_trait::async_trait;
use domain::generated::scalars as ds;
use domain::shared::errors::DomainError;
use serde_json::json;
use std::sync::Arc;

use actor_client::supervision::{MailboxLaneRow, PoisonedMessageRow};
use application::queries::{
    CartReadRepository, CartRow, CatalogReadRepository, CatalogRow, CustomerReadRepository,
    CustomerRow, DeliveryPartnerAvailabilityFilter, DeliveryPartnerAvailabilityRow,
    DeliverySatisfactionRow, OrderConversationRow, OrderFilter, PricingPolicyReadRepository,
    PricingPolicyRow, ProspectFilter, ProspectionPipelineRow, ProspectionReadRepository,
    ReadScope, ReclamationFilter, ReclamationRow, RefundFilter, RefundRow, RestaurantFilter,
    RestaurantReadRepository, RestaurantRow, UberEstimationPolicyReadRepository,
    UberEstimationPolicyRow, UberSplitPolicyReadRepository, UberSplitPolicyRow,
};
use application::projections::OrderTrackingRow;
use server::graphql_schema::{build_schema, CaptainSchema, ReadDeps};
use server::graphql_tenant::TenantScope;

fn uid(n: u8) -> uuid::Uuid {
    uuid::Uuid::from_u128(n as u128)
}

// --- fixtures ------------------------------------------------------------------------------------

/// The seeded menu: ONE product ("Burger maison") with one 15,00 EUR offer — the item whose
/// display name the paint must carry. The `tree` is spelled in the CatalogProjector's OWN folded
/// format (camelCase, the derived per-offer `stockStatus` included): `catalog_tree_section`
/// deserializes the API read shape from it, and a tree missing the derived field silently drops
/// the product (serde `.ok()` leniency) — which is exactly what an entity-serialized fixture did
/// on the first run of this file.
fn catalog_row(restaurant: ds::RestaurantId) -> CatalogRow {
    let now = chrono::Utc::now();
    CatalogRow {
        catalog_id: ds::CatalogId(uid(50)),
        restaurant_id: restaurant,
        slug: None,
        name: ds::CatalogName("Menu".into()),
        tree: json!({
            "categories": [],
            "products": [{
                "id": uid(120), "categoryRef": null, "name": "Burger maison",
                "description": null, "tags": [], "imageIds": [],
                "taxRate": { "delivery": 10.0, "collection": null, "eatIn": null },
                "offers": [{
                    "id": uid(20), "name": "Default",
                    "price": { "amountCents": 1500, "currency": "EUR" },
                    "uberPrice": null, "uberPriceBasis": null,
                    "availability": "AVAILABLE", "stockStatus": "IN_STOCK",
                    "optionListIds": [uid(200)],
                }],
            }],
            "optionLists": [{
                "id": uid(200), "name": "Extras", "minSelections": 0, "maxSelections": 1,
                "multipleSelection": false,
                "options": [{
                    "id": uid(30), "name": "Cheese",
                    "price": { "amountCents": 200, "currency": "EUR" },
                    "default": false, "availability": "AVAILABLE", "stockStatus": null,
                }],
            }],
        }),
        created_at: now,
        updated_at: now,
    }
}

fn restaurant_row(restaurant_id: uuid::Uuid, slug: &str) -> RestaurantRow {
    let now = chrono::Utc::now();
    RestaurantRow {
        restaurant_id: ds::RestaurantId(restaurant_id),
        restaurant_account_id: None,
        listing_status: ds::RestaurantListingStatus::ACTIVE_PARTNER,
        external_identifiers: None,
        google_place_id: None,
        slug: Some(ds::Slug(slug.into())),
        display_name: ds::RestaurantDisplayName("Chez Test".into()),
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

/// One registered restaurant: current slug + a superseded label (the SlugAlias path,
/// ADR-20260728-011344) — `by_previous_slug` resolves the renamed-away label to the SAME row,
/// exactly like `hosts.rs` does for the host path.
#[derive(Clone)]
struct OneRestaurant {
    row: RestaurantRow,
    renamed_from: Option<&'static str>,
}

#[async_trait]
impl RestaurantReadRepository for OneRestaurant {
    async fn list(&self, _f: RestaurantFilter) -> Result<Vec<RestaurantRow>, DomainError> {
        Ok(vec![self.row.clone()])
    }
    async fn by_slug(&self, slug: ds::Slug) -> Result<Option<RestaurantRow>, DomainError> {
        Ok((Some(&slug) == self.row.slug.as_ref()).then(|| self.row.clone()))
    }
    async fn by_id(&self, id: ds::RestaurantId) -> Result<Option<RestaurantRow>, DomainError> {
        Ok((id == self.row.restaurant_id).then(|| self.row.clone()))
    }
    async fn by_previous_slug(&self, slug: ds::Slug) -> Result<Option<RestaurantRow>, DomainError> {
        Ok((Some(slug.0.as_str()) == self.renamed_from).then(|| self.row.clone()))
    }
}

struct SeededCatalogs(CatalogRow);

#[async_trait]
impl CatalogReadRepository for SeededCatalogs {
    async fn by_restaurant(&self, id: ds::RestaurantId) -> Result<Option<CatalogRow>, DomainError> {
        Ok((id == self.0.restaurant_id).then(|| self.0.clone()))
    }
}

// --- Empty stand-ins for the read models these tests never touch ---------------------------------

struct Empty;

/// The door defaults CLOSED and this suite never opens it (the menu paint runs no priced read at
/// all -- ux's own premise for PROP-20260831-134539 slice 3a D1); a real fold call here would be a
/// defect, so both refuse loudly rather than answering with an empty catalog.
#[async_trait]
impl application::ports::AsOfPriceAuthority for Empty {
    async fn as_of(
        &self,
        _catalog_id: domain::generated::scalars::CatalogId,
        _version: domain::catalog_as_of::CatalogVersion,
    ) -> Result<domain::catalog_as_of::AsOfCatalog, DomainError> {
        Err(DomainError::Repository("Empty never folds".into()))
    }
    async fn at_head(
        &self,
        _catalog_id: domain::generated::scalars::CatalogId,
        _correlation_id: uuid::Uuid,
    ) -> Result<domain::catalog_as_of::AsOfCatalog, DomainError> {
        Err(DomainError::Repository("Empty never folds".into()))
    }
}

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
impl CartReadRepository for Empty {
    async fn by_customer(&self, _c: ds::CustomerId) -> Result<Vec<CartRow>, DomainError> {
        Ok(Vec::new())
    }
    async fn by_id(&self, _id: ds::CartId) -> Result<Option<CartRow>, DomainError> {
        Ok(None)
    }
    async fn open_by_session(&self, _s: ds::SessionId) -> Result<Vec<CartRow>, DomainError> {
        Ok(Vec::new())
    }
    async fn open_by_customer_at(
        &self,
        _c: ds::CustomerId,
        _r: ds::RestaurantId,
    ) -> Result<Vec<CartRow>, DomainError> {
        Ok(Vec::new())
    }
    async fn open_by_session_at(
        &self,
        _s: ds::SessionId,
        _r: ds::RestaurantId,
    ) -> Result<Vec<CartRow>, DomainError> {
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

// --- harness -------------------------------------------------------------------------------------

const SLUG: &str = "chez-test";
const OLD_SLUG: &str = "chez-test-old";

fn restaurant_id() -> ds::RestaurantId {
    ds::RestaurantId(uid(90))
}

fn schema() -> CaptainSchema {
    let rid = restaurant_id();
    build_schema(
        Some(ReadDeps {
            restaurants: Arc::new(OneRestaurant {
                row: restaurant_row(rid.0, SLUG),
                renamed_from: Some(OLD_SLUG),
            }),
            prospection: Arc::new(Empty),
            pricing_policy: Arc::new(Empty),
            uber_estimation_policy: Arc::new(Empty),
            uber_split_policy: Arc::new(Empty),
            catalogs: Arc::new(SeededCatalogs(catalog_row(rid))),
            carts: Arc::new(Empty),
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
        as_of_price_authority: Arc::new(Empty),
        run_fold_priced_cart_read: server::graphql_schema::RunFoldPricedCartRead(false),
        }),
        None,
        None,
    )
}

const CATALOG_Q: &str =
    "query($input: CatalogQueryInput) { catalog(input: $input) { id restaurantId name } }";

async fn catalog_with(schema: &CaptainSchema, input: serde_json::Value) -> async_graphql::Response {
    schema
        .execute(Request::new(CATALOG_Q).variables(Variables::from_json(json!({ "input": input }))))
        .await
}

fn error_code(resp: &async_graphql::Response) -> Option<String> {
    resp.errors.first().and_then(|e| {
        e.extensions
            .as_ref()
            .and_then(|x| x.get("code"))
            .map(|v| v.to_string().trim_matches('"').to_string())
    })
}

// --- the paint (beck's missing end-to-end) -------------------------------------------------------

/// THE test #749 exists for: a stranger opens `chez-test.captain.food` and the initial HTML —
/// rendered through production's own SSR path (real composed schema, the renderer's actual query
/// documents, the router's host-injected slug) — carries the seeded catalog item's display name.
/// Seen RED before the fix: the catalog read was a declared §25b structural skip, so the page
/// shell rendered around a MISSING menu.
#[tokio::test]
async fn the_storefront_paint_carries_the_menu_item_from_a_real_schema() {
    let exec = server::SsrExec { schema: schema(), stripe_publishable_key: None };
    let page = web::router::render_path_with(
        &exec.transport(),
        "chez-test.captain.food",
        "/",
        "fr",
        None,
    )
    .await
    .expect("the tenant root renders");
    assert!(
        page.html.contains("Burger maison"),
        "the MENU item's display name must be in the initial HTML — the money path (degraded: {:?}, skipped: {:?}): {}",
        page.degraded,
        page.skipped,
        page.html
    );
    assert!(
        page.html.contains("15,00 EUR"),
        "the item's price renders with the name (the number the customer decides on): {}",
        page.html
    );
    assert!(page.html.contains("Chez Test"), "the shell still renders: {}", page.html);
    assert!(
        page.degraded.is_empty(),
        "a working menu read must not degrade the paint: {:?}",
        page.degraded
    );
    assert!(
        !page.skipped.iter().any(|s| s.resolver == "catalog.byRestaurant"),
        "the catalog read must RUN, not ride a stale skip declaration: {:?}",
        page.skipped
    );
}

/// #755 (founder-decided, red-first): the SAME paint through a `{slug}.localhost` host — dev's
/// zero-config tenant space (browsers resolve `*.localhost` to loopback, no /etc/hosts entry).
/// The Host carries the slug exactly as in production, so the #745 host-slug injection must feed
/// the catalog read unchanged and the initial HTML must carry the live menu. Seen RED before the
/// change: `.localhost` was not audience space, so the paint served the MARKETPLACE home instead
/// of the storefront.
#[tokio::test]
async fn the_storefront_paint_works_identically_through_a_slug_localhost_host() {
    let exec = server::SsrExec { schema: schema(), stripe_publishable_key: None };
    let page = web::router::render_path_with(
        &exec.transport(),
        "chez-test.localhost:8080",
        "/",
        "fr",
        None,
    )
    .await
    .expect("the localhost tenant root renders");
    assert!(
        page.html.contains("data-hydrate=\"restaurant\""),
        "a {{slug}}.localhost host must serve the STOREFRONT, not the marketplace: {}",
        page.html
    );
    assert!(
        page.html.contains("Burger maison"),
        "the menu item's display name must be in the initial HTML, exactly like the apex paint \
         (degraded: {:?}, skipped: {:?}): {}",
        page.degraded,
        page.skipped,
        page.html
    );
    assert!(page.html.contains("15,00 EUR"), "the item's price renders with the name: {}", page.html);
    assert!(page.degraded.is_empty(), "a working menu read must not degrade: {:?}", page.degraded);
}

// --- the exactly-one-of triple (DSL-declared, codegen-emitted) -----------------------------------

/// slug-only → the catalog answers, resolved through the SAME slug path as the tenant host.
#[tokio::test]
async fn slug_only_resolves_the_catalog() {
    let resp = catalog_with(&schema(), json!({ "restaurantSlug": SLUG })).await;
    assert!(resp.errors.is_empty(), "slug-only must resolve: {:?}", resp.errors);
    let data = resp.data.into_json().unwrap();
    assert_eq!(data["catalog"]["restaurantId"], json!(restaurant_id().0));
    assert_eq!(data["catalog"]["name"], json!("Menu"));
}

/// A RENAMED slug behaves identically on the arg path and the host path (dba/young): the
/// superseded label still resolves the SAME restaurant's catalog, through `by_previous_slug` —
/// the SlugAlias fallback `hosts.rs` uses for the 301.
#[tokio::test]
async fn a_superseded_slug_resolves_like_the_host_path_does() {
    let resp = catalog_with(&schema(), json!({ "restaurantSlug": OLD_SLUG })).await;
    assert!(resp.errors.is_empty(), "the alias must resolve: {:?}", resp.errors);
    let data = resp.data.into_json().unwrap();
    assert_eq!(data["catalog"]["restaurantId"], json!(restaurant_id().0));
}

/// An UNKNOWN slug is null — an answered miss, never an error (the read stays nullable).
#[tokio::test]
async fn an_unknown_slug_is_null_not_an_error() {
    let resp = catalog_with(&schema(), json!({ "restaurantSlug": "nobody" })).await;
    assert!(resp.errors.is_empty(), "unknown slug answers null: {:?}", resp.errors);
    assert_eq!(resp.data.into_json().unwrap()["catalog"], json!(null));
}

/// restaurantId-only keeps working — the relaxation is ADDITIVE (required→optional).
#[tokio::test]
async fn restaurant_id_only_still_resolves_the_catalog() {
    let resp = catalog_with(&schema(), json!({ "restaurantId": restaurant_id().0 })).await;
    assert!(resp.errors.is_empty(), "id-only must keep resolving: {:?}", resp.errors);
    let data = resp.data.into_json().unwrap();
    assert_eq!(data["catalog"]["name"], json!("Menu"));
}

/// ZERO-OF → the declared typed error (P-10 extensions shape), never a null that hides the
/// caller's bug.
#[tokio::test]
async fn zero_of_rejects_with_the_typed_error() {
    let resp = catalog_with(&schema(), json!({})).await;
    assert_eq!(
        error_code(&resp).as_deref(),
        Some("CatalogSelectorInvalid"),
        "zero-of must reject typed: {:?}",
        resp.errors
    );
}

/// TWO-OF → the same typed error, even when both name the SAME restaurant (mutation plant: an
/// implementation that accepts an agreeing pair passes zero-of and disagreement checks — this is
/// the assertion that kills it). Exactly-one-of means exactly one.
#[tokio::test]
async fn two_of_rejects_with_the_typed_error_even_when_they_agree() {
    for slug in [SLUG, "somebody-else"] {
        let resp = catalog_with(
            &schema(),
            json!({ "restaurantId": restaurant_id().0, "restaurantSlug": slug }),
        )
        .await;
        assert_eq!(
            error_code(&resp).as_deref(),
            Some("CatalogSelectorInvalid"),
            "two-of ({slug}) must reject typed: {:?}",
            resp.errors
        );
    }
}

// --- host precedence (young's hard rule) ---------------------------------------------------------

/// On a tenant host the Host is the tenant selector: a client-sent selector naming ANOTHER
/// restaurant REJECTS with a typed error — never a silent pick (a silent pick is a cross-tenant
/// read whichever side wins).
#[tokio::test]
async fn a_selector_disagreeing_with_the_tenant_host_rejects() {
    // The request arrives on a host whose tenant is a DIFFERENT restaurant than the one the
    // selector names — both selector shapes must reject identically.
    let other_tenant = TenantScope::Restaurant(ds::RestaurantId(uid(91)));
    for input in
        [json!({ "restaurantSlug": SLUG }), json!({ "restaurantId": restaurant_id().0 })]
    {
        let resp = schema()
            .execute(
                Request::new(CATALOG_Q)
                    .variables(Variables::from_json(json!({ "input": input })))
                    .data(other_tenant),
            )
            .await;
        assert_eq!(
            error_code(&resp).as_deref(),
            Some("TenantSelectorMismatch"),
            "a selector disagreeing with the Host must reject ({input}): {:?}",
            resp.errors
        );
    }
}

/// The AGREEING selector on a tenant host resolves normally (the storefront's own paint), and on
/// a host with NO tenant (the marketplace apex) any selector is legitimate.
#[tokio::test]
async fn an_agreeing_or_untenanted_selector_resolves() {
    let resp = schema()
        .execute(
            Request::new(CATALOG_Q)
                .variables(Variables::from_json(json!({ "input": { "restaurantSlug": SLUG } })))
                .data(TenantScope::Restaurant(restaurant_id())),
        )
        .await;
    assert!(resp.errors.is_empty(), "the agreeing selector resolves: {:?}", resp.errors);
    let resp = schema()
        .execute(
            Request::new(CATALOG_Q)
                .variables(Variables::from_json(json!({ "input": { "restaurantSlug": SLUG } })))
                .data(TenantScope::None),
        )
        .await;
    assert!(resp.errors.is_empty(), "no tenant, no constraint (the apex): {:?}", resp.errors);
}
