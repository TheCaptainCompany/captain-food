//! BEHAVIOUR tests for the Catalog aggregate — the executable form of the `specs/tests.yaml`
//! Given/When/Then cases whose `when` is a Catalog command (ADR-0032: each test cites the
//! `specs/rules.yaml` rule it asserts). Given = pre-seeded stream events (in-memory event store),
//! When = the command handler, Then = the emitted event(s) / the errors.yaml rejection code.
//!
//! Pure and offline: an in-memory [`EventStore`] plus a fake `RestaurantReadRepository` (existence +
//! the default-currency authority for `CurrencyMismatch`). Invariants still lacking support are
//! documented `TODO(invariant)`s in `application::commands` and NOT asserted here: `RefNotUnique` on
//! CreateCatalog (cross-catalog ref index), `OfferNotStockTracked` (no model flag) and
//! `CatalogTranslationFailed` (raised inside the HubRise ACL, before the command exists).

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use application::commands::{
    add_catalog_category, add_option_list, add_product, create_catalog, import_catalog,
    rejection_code, remove_catalog_category, remove_option_list, remove_product,
    update_catalog_category, update_offer_stock, update_option_list, update_product,
};
use application::ports::{version_conflict, Actor, EventStore};
use application::queries::{RestaurantFilter, RestaurantReadRepository, RestaurantRow};
use domain::generated::commands::{
    AddCatalogCategory, AddOptionList, AddProduct, CreateCatalog, ImportCatalog,
    RemoveCatalogCategory, RemoveOptionList, RemoveProduct, UpdateCatalogCategory, UpdateOfferStock,
    UpdateOptionList, UpdateProduct,
};
use domain::generated::entities::{
    CatalogCategory, Money, Offer, OptionList, Product, ProductItemOption, TaxRate,
};
use domain::generated::events::{
    CatalogCategoryAdded, CatalogCreated, DomainEvent, OptionListAdded, ProductAdded,
};
use domain::generated::scalars::*;
use domain::shared::errors::DomainError;

// ------------------------------------------------------------------------------------------------
// Test doubles
// ------------------------------------------------------------------------------------------------

/// In-memory [`EventStore`]: version = number of events on the stream, same optimistic-concurrency
/// semantics as `PgEventStore` (a clash → the canonical `version_conflict`).
#[derive(Default)]
struct MemStore {
    streams: Mutex<HashMap<String, Vec<DomainEvent>>>,
}

impl MemStore {
    /// GIVEN: pre-seed a stream with already-recorded facts.
    fn seed(&self, stream: &str, events: Vec<DomainEvent>) {
        self.streams.lock().unwrap().insert(stream.to_string(), events);
    }

    /// THEN: the full stream after the command ran.
    fn stream(&self, stream: &str) -> Vec<DomainEvent> {
        self.streams.lock().unwrap().get(stream).cloned().unwrap_or_default()
    }
}

#[async_trait]
impl EventStore for MemStore {
    async fn append(
        &self,
        stream_name: &str,
        expected_version: i64,
        events: &[DomainEvent],
        _actor: &Actor,
    ) -> Result<i64, DomainError> {
        let mut streams = self.streams.lock().unwrap();
        let stream = streams.entry(stream_name.to_string()).or_default();
        if stream.len() as i64 != expected_version {
            return Err(version_conflict(stream_name, expected_version));
        }
        stream.extend(events.iter().cloned());
        Ok(stream.len() as i64)
    }

    async fn load(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError> {
        let events = self.stream(stream_name);
        let version = events.len() as i64;
        Ok((events, version))
    }
}

/// Fake Restaurant read model: at most one projected row (the EUR currency authority).
#[derive(Default)]
struct FakeRestaurants {
    row: Option<RestaurantRow>,
}

#[async_trait]
impl RestaurantReadRepository for FakeRestaurants {
    async fn list(&self, _filter: RestaurantFilter) -> Result<Vec<RestaurantRow>, DomainError> {
        Ok(self.row.clone().into_iter().collect())
    }

    async fn by_slug(&self, slug: Slug) -> Result<Option<RestaurantRow>, DomainError> {
        Ok(self.row.clone().filter(|r| r.slug == slug))
    }

    async fn by_id(&self, id: RestaurantId) -> Result<Option<RestaurantRow>, DomainError> {
        Ok(self.row.clone().filter(|r| r.restaurant_id == id))
    }
}

// ------------------------------------------------------------------------------------------------
// Fixtures (tests.yaml `fixtures`, with UUIDs instead of the sample string ids)
// ------------------------------------------------------------------------------------------------

fn actor() -> Actor {
    Actor {
        user_id: uuid::Uuid::new_v4(),
        user_type: 5, // UserType::ADMIN ordinal
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
    }
}

fn stream(id: CatalogId) -> String {
    format!("Catalog-{}", id.0)
}

/// A projected `restaurant` row for `id` (EUR is the restaurant's default currency).
fn projected_restaurant(id: RestaurantId) -> RestaurantRow {
    RestaurantRow {
        restaurant_id: id,
        restaurant_account_id: None,
        listing_status: RestaurantListingStatus::ACTIVE_PARTNER,
        external_identifiers: None,
        google_place_id: None,
        slug: Slug("chez-marco".into()),
        display_name: RestaurantDisplayName("Chez Marco".into()),
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
        address: serde_json::json!({ "line1": "1 Rue Nationale", "postalCode": "37000", "city": "Tours", "country": "FR" }),
        location: None,
        opening_hours: serde_json::json!([]),
        status: RestaurantStatus::ACTIVE,
        order_acceptance: OrderAcceptanceMode::NORMAL,
        default_currency: CurrencyCode("EUR".into()),
        timezone: None,
        preparation_time_minutes: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

/// Fixture `catalogCreated`.
fn created_event(catalog_id: CatalogId, restaurant_id: RestaurantId) -> DomainEvent {
    DomainEvent::CatalogCreated(CatalogCreated {
        catalog_id,
        r#ref: None,
        restaurant_id,
        name: CatalogName("Main".into()),
    })
}

fn money(cents: i64, currency: &str) -> Money {
    Money { amount_cents: MoneyCents(cents), currency: CurrencyCode(currency.into()) }
}

fn offer(id: OfferId, product_id: ProductId, currency: &str) -> Offer {
    Offer {
        id,
        r#ref: None,
        product_id,
        name: OfferName("Default".into()),
        price: money(980, currency),
        availability: CatalogItemAvailability::AVAILABLE,
        stock: None,
        option_list_ids: vec![],
    }
}

fn product(id: ProductId, catalog_id: CatalogId, restaurant_id: RestaurantId, offer_id: OfferId) -> Product {
    Product {
        id,
        r#ref: None,
        catalog_id,
        restaurant_id,
        category_ref: None,
        name: ProductName("Margherita".into()),
        description: None,
        tags: vec![],
        image_ids: vec![],
        tax_rate: TaxRate { delivery: TaxRatePercent(10.0), collection: None, eat_in: None },
        offers: vec![offer(offer_id, id, "EUR")],
    }
}

/// Fixture `productAdded`.
fn product_added_event(
    catalog_id: CatalogId,
    restaurant_id: RestaurantId,
    product_id: ProductId,
    offer_id: OfferId,
) -> DomainEvent {
    DomainEvent::ProductAdded(ProductAdded {
        catalog_id,
        restaurant_id,
        product: product(product_id, catalog_id, restaurant_id, offer_id),
    })
}

fn category(id: ProductCategoryId, catalog_id: CatalogId, r: Option<&str>, parent: Option<&str>, name: &str) -> CatalogCategory {
    CatalogCategory {
        id,
        r#ref: r.map(|s| ExternalReference(s.into())),
        catalog_id,
        parent_ref: parent.map(|s| ExternalReference(s.into())),
        name: CatalogCategoryName(name.into()),
        description: None,
        tags: vec![],
        image_ids: vec![],
    }
}

fn option_list(id: OptionListId, min: i64, max: Option<i64>) -> OptionList {
    OptionList {
        id,
        r#ref: None,
        name: OptionListName("Size".into()),
        min_selections: min,
        max_selections: max,
        multiple_selection: false,
        options: vec![ProductItemOption {
            id: OptionId(uuid::Uuid::new_v4()),
            r#ref: None,
            option_list_id: id,
            name: OptionName("Large".into()),
            price: money(200, "EUR"),
            r#default: true,
            availability: CatalogItemAvailability::AVAILABLE,
            stock: None,
        }],
    }
}

fn cid() -> CatalogId {
    CatalogId(uuid::Uuid::new_v4())
}

fn rid() -> RestaurantId {
    RestaurantId(uuid::Uuid::new_v4())
}

/// GIVEN a created catalog for a projected restaurant; returns (store, repo, catalog_id, restaurant_id).
fn given_catalog() -> (MemStore, FakeRestaurants, CatalogId, RestaurantId) {
    let store = MemStore::default();
    let catalog_id = cid();
    let restaurant_id = rid();
    store.seed(&stream(catalog_id), vec![created_event(catalog_id, restaurant_id)]);
    let repo = FakeRestaurants { row: Some(projected_restaurant(restaurant_id)) };
    (store, repo, catalog_id, restaurant_id)
}

fn add_product_cmd(catalog_id: CatalogId, restaurant_id: RestaurantId, currency: &str) -> AddProduct {
    let product_id = ProductId(uuid::Uuid::new_v4());
    AddProduct {
        product_id,
        catalog_id,
        restaurant_id,
        category_ref: None,
        name: ProductName("Margherita".into()),
        description: None,
        tags: vec![],
        tax_rate: TaxRate { delivery: TaxRatePercent(10.0), collection: None, eat_in: None },
        offers: vec![offer(OfferId(uuid::Uuid::new_v4()), product_id, currency)],
        r#ref: None,
    }
}

// ------------------------------------------------------------------------------------------------
// Creation (rules.yaml#/CatalogCreationForRestaurant)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestCatalogCreated — rules.yaml#/CatalogCreationForRestaurant
#[tokio::test]
async fn creates_a_catalog_for_a_registered_restaurant() {
    let store = MemStore::default();
    let catalog_id = cid();
    let restaurant_id = rid();
    let repo = FakeRestaurants { row: Some(projected_restaurant(restaurant_id)) };

    create_catalog(
        &store,
        &repo,
        CreateCatalog { catalog_id, restaurant_id, name: CatalogName("Main".into()), r#ref: None },
        &actor(),
    )
    .await
    .expect("create");

    let events = store.stream(&stream(catalog_id));
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        DomainEvent::CatalogCreated(e) if e.catalog_id == catalog_id && e.name.0 == "Main"
    ));

    // Idempotent replay (client-generated ids, ADR-0034): no duplicate fact.
    create_catalog(
        &store,
        &repo,
        CreateCatalog { catalog_id, restaurant_id, name: CatalogName("Main".into()), r#ref: None },
        &actor(),
    )
    .await
    .expect("replay absorbed");
    assert_eq!(store.stream(&stream(catalog_id)).len(), 1);
}

/// tests.yaml#/cases/TestCatalogCreateIsRejected (RestaurantNotFound arm) —
/// rules.yaml#/CatalogCreationForRestaurant. The RefNotUnique arm is TODO(invariant) until an
/// external-reference index port exists.
#[tokio::test]
async fn rejects_creating_a_catalog_for_a_missing_restaurant() {
    let store = MemStore::default();
    let catalog_id = cid();

    let err = create_catalog(
        &store,
        &FakeRestaurants::default(),
        CreateCatalog { catalog_id, restaurant_id: rid(), name: CatalogName("Main".into()), r#ref: None },
        &actor(),
    )
    .await
    .expect_err("missing restaurant");
    assert_eq!(rejection_code(&err), Some("RestaurantNotFound"));
    assert!(store.stream(&stream(catalog_id)).is_empty(), "no event on rejection");
}

// ------------------------------------------------------------------------------------------------
// Products (rules.yaml#/CatalogProductManagement)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestCatalogProductAdded — rules.yaml#/CatalogProductManagement
#[tokio::test]
async fn adds_a_product_with_one_offer() {
    let (store, repo, catalog_id, restaurant_id) = given_catalog();

    add_product(&store, &repo, add_product_cmd(catalog_id, restaurant_id, "EUR"), &actor())
        .await
        .expect("add product");

    let events = store.stream(&stream(catalog_id));
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[1],
        DomainEvent::ProductAdded(e)
            if e.product.name.0 == "Margherita" && e.product.offers.len() == 1
    ));
}

/// tests.yaml#/cases/TestCatalogAddProductIsRejected (all four arms) —
/// rules.yaml#/CatalogProductManagement
#[tokio::test]
async fn rejects_adding_a_product_on_missing_catalog_currency_mismatch_bad_category_or_dup_ref() {
    let (store, repo, catalog_id, restaurant_id) = given_catalog();

    // Missing catalog → CatalogNotFound.
    let err = add_product(&store, &repo, add_product_cmd(cid(), restaurant_id, "EUR"), &actor())
        .await
        .expect_err("missing catalog");
    assert_eq!(rejection_code(&err), Some("CatalogNotFound"));

    // Offer priced in USD against an EUR restaurant → CurrencyMismatch.
    let err = add_product(&store, &repo, add_product_cmd(catalog_id, restaurant_id, "USD"), &actor())
        .await
        .expect_err("currency mismatch");
    assert_eq!(rejection_code(&err), Some("CurrencyMismatch"));

    // A categoryRef that resolves to no category → CatalogCategoryRefNotFound.
    let mut cmd = add_product_cmd(catalog_id, restaurant_id, "EUR");
    cmd.category_ref = Some(ExternalReference("cat-ghost".into()));
    let err = add_product(&store, &repo, cmd, &actor()).await.expect_err("bad category ref");
    assert_eq!(rejection_code(&err), Some("CatalogCategoryRefNotFound"));

    // A product ref already used in the catalog → RefNotUnique.
    let taken = ProductId(uuid::Uuid::new_v4());
    let mut existing = product(taken, catalog_id, restaurant_id, OfferId(uuid::Uuid::new_v4()));
    existing.r#ref = Some(ExternalReference("PIZ-MARG".into()));
    let mut given = store.stream(&stream(catalog_id));
    given.push(DomainEvent::ProductAdded(ProductAdded {
        catalog_id,
        restaurant_id,
        product: existing,
    }));
    store.seed(&stream(catalog_id), given);
    let mut cmd = add_product_cmd(catalog_id, restaurant_id, "EUR");
    cmd.r#ref = Some(ExternalReference("PIZ-MARG".into()));
    let err = add_product(&store, &repo, cmd, &actor()).await.expect_err("dup ref");
    assert_eq!(rejection_code(&err), Some("RefNotUnique"));
}

/// tests.yaml#/cases/TestCatalogProductUpdated — rules.yaml#/CatalogProductManagement
#[tokio::test]
async fn updates_an_existing_product_full_replace() {
    let (store, repo, catalog_id, restaurant_id) = given_catalog();
    let product_id = ProductId(uuid::Uuid::new_v4());
    let offer_id = OfferId(uuid::Uuid::new_v4());
    let mut given = store.stream(&stream(catalog_id));
    given.push(product_added_event(catalog_id, restaurant_id, product_id, offer_id));
    store.seed(&stream(catalog_id), given);

    let mut updated = product(product_id, catalog_id, restaurant_id, offer_id);
    updated.name = ProductName("Margherita (large)".into());
    updated.offers[0].price = money(1180, "EUR");
    update_product(
        &store,
        &repo,
        UpdateProduct { catalog_id, restaurant_id, product: updated },
        &actor(),
    )
    .await
    .expect("update product");

    let events = store.stream(&stream(catalog_id));
    assert!(matches!(
        &events[2],
        DomainEvent::ProductUpdated(e)
            if e.product.name.0 == "Margherita (large)" && e.product.offers[0].price.amount_cents.0 == 1180
    ));
}

/// tests.yaml#/cases/TestCatalogUpdateProductIsRejected (all three arms) —
/// rules.yaml#/CatalogProductManagement
#[tokio::test]
async fn rejects_updating_a_missing_product_offerless_product_or_currency_mismatch() {
    let (store, repo, catalog_id, restaurant_id) = given_catalog();
    let product_id = ProductId(uuid::Uuid::new_v4());
    let offer_id = OfferId(uuid::Uuid::new_v4());
    let mut given = store.stream(&stream(catalog_id));
    given.push(product_added_event(catalog_id, restaurant_id, product_id, offer_id));
    store.seed(&stream(catalog_id), given);

    // A product id that is not in the catalog → ProductNotFound.
    let ghost = product(ProductId(uuid::Uuid::new_v4()), catalog_id, restaurant_id, offer_id);
    let err = update_product(
        &store,
        &repo,
        UpdateProduct { catalog_id, restaurant_id, product: ghost },
        &actor(),
    )
    .await
    .expect_err("missing product");
    assert_eq!(rejection_code(&err), Some("ProductNotFound"));

    // Removing the last offer → ProductMustHaveOffer.
    let mut offerless = product(product_id, catalog_id, restaurant_id, offer_id);
    offerless.offers.clear();
    let err = update_product(
        &store,
        &repo,
        UpdateProduct { catalog_id, restaurant_id, product: offerless },
        &actor(),
    )
    .await
    .expect_err("no offer left");
    assert_eq!(rejection_code(&err), Some("ProductMustHaveOffer"));

    // Repricing in USD against an EUR restaurant → CurrencyMismatch.
    let mut mismatch = product(product_id, catalog_id, restaurant_id, offer_id);
    mismatch.offers[0].price = money(980, "USD");
    let err = update_product(
        &store,
        &repo,
        UpdateProduct { catalog_id, restaurant_id, product: mismatch },
        &actor(),
    )
    .await
    .expect_err("currency mismatch");
    assert_eq!(rejection_code(&err), Some("CurrencyMismatch"));
    assert_eq!(store.stream(&stream(catalog_id)).len(), 2, "no event on rejection");
}

/// tests.yaml#/cases/TestCatalogProductRemoved — rules.yaml#/CatalogProductManagement
#[tokio::test]
async fn removes_a_product_from_a_catalog() {
    let (store, _repo, catalog_id, restaurant_id) = given_catalog();
    let product_id = ProductId(uuid::Uuid::new_v4());
    let mut given = store.stream(&stream(catalog_id));
    given.push(product_added_event(catalog_id, restaurant_id, product_id, OfferId(uuid::Uuid::new_v4())));
    store.seed(&stream(catalog_id), given);

    remove_product(&store, RemoveProduct { catalog_id, restaurant_id, product_id }, &actor())
        .await
        .expect("remove product");
    let events = store.stream(&stream(catalog_id));
    assert!(matches!(&events[2], DomainEvent::ProductRemoved(e) if e.product_id == product_id));

    // And a missing product rejects (actors.yaml throws ProductNotFound).
    let err = remove_product(
        &store,
        RemoveProduct { catalog_id, restaurant_id, product_id: ProductId(uuid::Uuid::new_v4()) },
        &actor(),
    )
    .await
    .expect_err("missing product");
    assert_eq!(rejection_code(&err), Some("ProductNotFound"));
}

// ------------------------------------------------------------------------------------------------
// Category tree (rules.yaml#/CatalogCategoryTreeManagement)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestCatalogCategoryAdded — rules.yaml#/CatalogCategoryTreeManagement
#[tokio::test]
async fn adds_a_category_to_a_catalog() {
    let (store, _repo, catalog_id, restaurant_id) = given_catalog();
    let id = ProductCategoryId(uuid::Uuid::new_v4());

    add_catalog_category(
        &store,
        AddCatalogCategory {
            catalog_id,
            restaurant_id,
            category: category(id, catalog_id, None, None, "Pizzas"),
        },
        &actor(),
    )
    .await
    .expect("add category");

    let events = store.stream(&stream(catalog_id));
    assert!(matches!(
        &events[1],
        DomainEvent::CatalogCategoryAdded(e) if e.category.name.0 == "Pizzas"
    ));
}

/// tests.yaml#/cases/TestCatalogAddCategoryIsRejected (all three arms) —
/// rules.yaml#/CatalogCategoryTreeManagement
#[tokio::test]
async fn rejects_adding_a_category_on_missing_catalog_missing_parent_or_dup_ref() {
    let (store, _repo, catalog_id, restaurant_id) = given_catalog();

    // Missing catalog → CatalogNotFound.
    let ghost_catalog = cid();
    let err = add_catalog_category(
        &store,
        AddCatalogCategory {
            catalog_id: ghost_catalog,
            restaurant_id,
            category: category(ProductCategoryId(uuid::Uuid::new_v4()), ghost_catalog, None, None, "Pizzas"),
        },
        &actor(),
    )
    .await
    .expect_err("missing catalog");
    assert_eq!(rejection_code(&err), Some("CatalogNotFound"));

    // A parentRef that resolves to no category → ParentCatalogCategoryNotFound.
    let err = add_catalog_category(
        &store,
        AddCatalogCategory {
            catalog_id,
            restaurant_id,
            category: category(
                ProductCategoryId(uuid::Uuid::new_v4()),
                catalog_id,
                None,
                Some("cat-ghost"),
                "Calzones",
            ),
        },
        &actor(),
    )
    .await
    .expect_err("missing parent");
    assert_eq!(rejection_code(&err), Some("ParentCatalogCategoryNotFound"));

    // A ref already used in the catalog → RefNotUnique.
    let mut given = store.stream(&stream(catalog_id));
    given.push(DomainEvent::CatalogCategoryAdded(CatalogCategoryAdded {
        catalog_id,
        restaurant_id,
        category: category(ProductCategoryId(uuid::Uuid::new_v4()), catalog_id, Some("CAT-PIZZA"), None, "Pizzas"),
    }));
    store.seed(&stream(catalog_id), given);
    let err = add_catalog_category(
        &store,
        AddCatalogCategory {
            catalog_id,
            restaurant_id,
            category: category(
                ProductCategoryId(uuid::Uuid::new_v4()),
                catalog_id,
                Some("CAT-PIZZA"),
                None,
                "Pizzas bis",
            ),
        },
        &actor(),
    )
    .await
    .expect_err("dup ref");
    assert_eq!(rejection_code(&err), Some("RefNotUnique"));
}

/// tests.yaml#/cases/TestCatalogCategoryUpdated — rules.yaml#/CatalogCategoryTreeManagement
#[tokio::test]
async fn updates_a_category_full_replace() {
    let (store, _repo, catalog_id, restaurant_id) = given_catalog();
    let id = ProductCategoryId(uuid::Uuid::new_v4());
    let mut given = store.stream(&stream(catalog_id));
    given.push(DomainEvent::CatalogCategoryAdded(CatalogCategoryAdded {
        catalog_id,
        restaurant_id,
        category: category(id, catalog_id, None, None, "Pizzas"),
    }));
    store.seed(&stream(catalog_id), given);

    update_catalog_category(
        &store,
        UpdateCatalogCategory {
            catalog_id,
            restaurant_id,
            category: category(id, catalog_id, None, None, "Pizzas & Calzones"),
        },
        &actor(),
    )
    .await
    .expect("update category");

    let events = store.stream(&stream(catalog_id));
    assert!(matches!(
        &events[2],
        DomainEvent::CatalogCategoryUpdated(e) if e.category.name.0 == "Pizzas & Calzones"
    ));
}

/// tests.yaml#/cases/TestCatalogUpdateCategoryIsRejected (both arms) —
/// rules.yaml#/CatalogCategoryTreeManagement
#[tokio::test]
async fn rejects_updating_a_missing_category_or_creating_a_cycle() {
    let (store, _repo, catalog_id, restaurant_id) = given_catalog();

    // Unknown category id → CatalogCategoryNotFound.
    let err = update_catalog_category(
        &store,
        UpdateCatalogCategory {
            catalog_id,
            restaurant_id,
            category: category(ProductCategoryId(uuid::Uuid::new_v4()), catalog_id, None, None, "Ghost"),
        },
        &actor(),
    )
    .await
    .expect_err("missing category");
    assert_eq!(rejection_code(&err), Some("CatalogCategoryNotFound"));

    // parent → child loop: re-parenting "Pizzas" under its own child → CatalogCategoryCycle.
    let parent = ProductCategoryId(uuid::Uuid::new_v4());
    let child = ProductCategoryId(uuid::Uuid::new_v4());
    let mut given = store.stream(&stream(catalog_id));
    given.push(DomainEvent::CatalogCategoryAdded(CatalogCategoryAdded {
        catalog_id,
        restaurant_id,
        category: category(parent, catalog_id, Some("CAT-PIZZA"), None, "Pizzas"),
    }));
    given.push(DomainEvent::CatalogCategoryAdded(CatalogCategoryAdded {
        catalog_id,
        restaurant_id,
        category: category(child, catalog_id, Some("CAT-CALZONE"), Some("CAT-PIZZA"), "Calzones"),
    }));
    store.seed(&stream(catalog_id), given);
    let err = update_catalog_category(
        &store,
        UpdateCatalogCategory {
            catalog_id,
            restaurant_id,
            category: category(parent, catalog_id, Some("CAT-PIZZA"), Some("CAT-CALZONE"), "Pizzas"),
        },
        &actor(),
    )
    .await
    .expect_err("cycle");
    assert_eq!(rejection_code(&err), Some("CatalogCategoryCycle"));
}

/// tests.yaml#/cases/TestCatalogCategoryRemoved + TestCatalogRemoveCategoryIsRejected —
/// rules.yaml#/CatalogCategoryTreeManagement
#[tokio::test]
async fn removes_an_empty_category_and_rejects_missing_or_non_empty_ones() {
    let (store, _repo, catalog_id, restaurant_id) = given_catalog();
    let parent = ProductCategoryId(uuid::Uuid::new_v4());
    let child = ProductCategoryId(uuid::Uuid::new_v4());
    let mut given = store.stream(&stream(catalog_id));
    given.push(DomainEvent::CatalogCategoryAdded(CatalogCategoryAdded {
        catalog_id,
        restaurant_id,
        category: category(parent, catalog_id, Some("CAT-PIZZA"), None, "Pizzas"),
    }));
    given.push(DomainEvent::CatalogCategoryAdded(CatalogCategoryAdded {
        catalog_id,
        restaurant_id,
        category: category(child, catalog_id, Some("CAT-CALZONE"), Some("CAT-PIZZA"), "Calzones"),
    }));
    store.seed(&stream(catalog_id), given);

    // The parent still has a child → CatalogCategoryNotEmpty.
    let err = remove_catalog_category(
        &store,
        RemoveCatalogCategory { catalog_id, restaurant_id, product_category_id: parent },
        &actor(),
    )
    .await
    .expect_err("not empty");
    assert_eq!(rejection_code(&err), Some("CatalogCategoryNotEmpty"));

    // The (childless) child removes fine.
    remove_catalog_category(
        &store,
        RemoveCatalogCategory { catalog_id, restaurant_id, product_category_id: child },
        &actor(),
    )
    .await
    .expect("remove leaf");
    let events = store.stream(&stream(catalog_id));
    assert!(matches!(
        events.last().unwrap(),
        DomainEvent::CatalogCategoryRemoved(e) if e.product_category_id == child
    ));

    // Unknown category id → CatalogCategoryNotFound.
    let err = remove_catalog_category(
        &store,
        RemoveCatalogCategory {
            catalog_id,
            restaurant_id,
            product_category_id: ProductCategoryId(uuid::Uuid::new_v4()),
        },
        &actor(),
    )
    .await
    .expect_err("missing category");
    assert_eq!(rejection_code(&err), Some("CatalogCategoryNotFound"));
}

// ------------------------------------------------------------------------------------------------
// Option lists (rules.yaml#/CatalogOptionListManagement)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestCatalogOptionListAdded + TestCatalogAddOptionListIsRejected —
/// rules.yaml#/CatalogOptionListManagement
#[tokio::test]
async fn adds_an_option_list_and_rejects_empty_or_out_of_bounds_ones() {
    let (store, _repo, catalog_id, restaurant_id) = given_catalog();
    let id = OptionListId(uuid::Uuid::new_v4());

    add_option_list(
        &store,
        AddOptionList { catalog_id, restaurant_id, option_list: option_list(id, 1, None) },
        &actor(),
    )
    .await
    .expect("add option list");
    let events = store.stream(&stream(catalog_id));
    assert!(matches!(
        &events[1],
        DomainEvent::OptionListAdded(e) if e.option_list.id == id && e.option_list.options.len() == 1
    ));

    // Missing catalog → CatalogNotFound.
    let ghost = cid();
    let err = add_option_list(
        &store,
        AddOptionList {
            catalog_id: ghost,
            restaurant_id,
            option_list: option_list(OptionListId(uuid::Uuid::new_v4()), 1, None),
        },
        &actor(),
    )
    .await
    .expect_err("missing catalog");
    assert_eq!(rejection_code(&err), Some("CatalogNotFound"));

    // No option at all → OptionListMustHaveOption.
    let mut empty = option_list(OptionListId(uuid::Uuid::new_v4()), 1, None);
    empty.options.clear();
    let err = add_option_list(
        &store,
        AddOptionList { catalog_id, restaurant_id, option_list: empty },
        &actor(),
    )
    .await
    .expect_err("no option");
    assert_eq!(rejection_code(&err), Some("OptionListMustHaveOption"));

    // minSelections 5 with a single option → InvalidSelectionBounds (tests.yaml data).
    let err = add_option_list(
        &store,
        AddOptionList {
            catalog_id,
            restaurant_id,
            option_list: option_list(OptionListId(uuid::Uuid::new_v4()), 5, None),
        },
        &actor(),
    )
    .await
    .expect_err("bounds");
    assert_eq!(rejection_code(&err), Some("InvalidSelectionBounds"));
}

/// tests.yaml#/cases/TestCatalogOptionListUpdated + TestCatalogUpdateOptionListIsRejected —
/// rules.yaml#/CatalogOptionListManagement
#[tokio::test]
async fn updates_an_option_list_and_rejects_missing_or_optionless_ones() {
    let (store, _repo, catalog_id, restaurant_id) = given_catalog();
    let id = OptionListId(uuid::Uuid::new_v4());
    let mut given = store.stream(&stream(catalog_id));
    given.push(DomainEvent::OptionListAdded(OptionListAdded {
        catalog_id,
        restaurant_id,
        option_list: option_list(id, 1, None),
    }));
    store.seed(&stream(catalog_id), given);

    let mut updated = option_list(id, 1, None);
    updated.options[0].name = OptionName("Extra Large".into());
    updated.options[0].price = money(300, "EUR");
    update_option_list(
        &store,
        UpdateOptionList { catalog_id, restaurant_id, option_list: updated },
        &actor(),
    )
    .await
    .expect("update option list");
    let events = store.stream(&stream(catalog_id));
    assert!(matches!(
        &events[2],
        DomainEvent::OptionListUpdated(e) if e.option_list.options[0].name.0 == "Extra Large"
    ));

    // Unknown option-list id → OptionListNotFound.
    let err = update_option_list(
        &store,
        UpdateOptionList {
            catalog_id,
            restaurant_id,
            option_list: option_list(OptionListId(uuid::Uuid::new_v4()), 1, None),
        },
        &actor(),
    )
    .await
    .expect_err("missing option list");
    assert_eq!(rejection_code(&err), Some("OptionListNotFound"));

    // Leaving it optionless → OptionListMustHaveOption.
    let mut empty = option_list(id, 1, None);
    empty.options.clear();
    let err = update_option_list(
        &store,
        UpdateOptionList { catalog_id, restaurant_id, option_list: empty },
        &actor(),
    )
    .await
    .expect_err("no option left");
    assert_eq!(rejection_code(&err), Some("OptionListMustHaveOption"));
}

/// tests.yaml#/cases/TestCatalogOptionListRemoved + TestCatalogRemoveOptionListIsRejected —
/// rules.yaml#/CatalogOptionListManagement
#[tokio::test]
async fn removes_an_option_list_and_rejects_missing_or_in_use_ones() {
    let (store, _repo, catalog_id, restaurant_id) = given_catalog();
    let used = OptionListId(uuid::Uuid::new_v4());
    let unused = OptionListId(uuid::Uuid::new_v4());
    let product_id = ProductId(uuid::Uuid::new_v4());
    let mut with_option_list = product(product_id, catalog_id, restaurant_id, OfferId(uuid::Uuid::new_v4()));
    with_option_list.offers[0].option_list_ids = vec![used];
    let mut given = store.stream(&stream(catalog_id));
    given.push(DomainEvent::OptionListAdded(OptionListAdded {
        catalog_id,
        restaurant_id,
        option_list: option_list(used, 1, None),
    }));
    given.push(DomainEvent::OptionListAdded(OptionListAdded {
        catalog_id,
        restaurant_id,
        option_list: option_list(unused, 1, None),
    }));
    given.push(DomainEvent::ProductAdded(ProductAdded {
        catalog_id,
        restaurant_id,
        product: with_option_list,
    }));
    store.seed(&stream(catalog_id), given);

    // Still referenced by an offer → OptionListInUse.
    let err = remove_option_list(
        &store,
        RemoveOptionList { catalog_id, restaurant_id, option_list_id: used },
        &actor(),
    )
    .await
    .expect_err("in use");
    assert_eq!(rejection_code(&err), Some("OptionListInUse"));

    // The unused one removes fine.
    remove_option_list(
        &store,
        RemoveOptionList { catalog_id, restaurant_id, option_list_id: unused },
        &actor(),
    )
    .await
    .expect("remove unused");
    let events = store.stream(&stream(catalog_id));
    assert!(matches!(
        events.last().unwrap(),
        DomainEvent::OptionListRemoved(e) if e.option_list_id == unused
    ));

    // Unknown option-list id → OptionListNotFound.
    let err = remove_option_list(
        &store,
        RemoveOptionList {
            catalog_id,
            restaurant_id,
            option_list_id: OptionListId(uuid::Uuid::new_v4()),
        },
        &actor(),
    )
    .await
    .expect_err("missing option list");
    assert_eq!(rejection_code(&err), Some("OptionListNotFound"));
}

// ------------------------------------------------------------------------------------------------
// Offer stock (rules.yaml#/OfferStockManualOrSynced)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestCatalogOfferStockUpdated + TestCatalogUpdateOfferStockIsRejected —
/// rules.yaml#/OfferStockManualOrSynced. The OfferNotStockTracked arm is TODO(invariant): the Offer
/// entity has no stock-tracking flag (an offer starts tracking on its first UpdateOfferStock).
#[tokio::test]
async fn sets_the_stock_of_an_offer_with_a_derived_status() {
    let (store, _repo, catalog_id, restaurant_id) = given_catalog();
    let product_id = ProductId(uuid::Uuid::new_v4());
    let offer_id = OfferId(uuid::Uuid::new_v4());
    let mut given = store.stream(&stream(catalog_id));
    given.push(product_added_event(catalog_id, restaurant_id, product_id, offer_id));
    store.seed(&stream(catalog_id), given);

    // quantity 5, no threshold → IN_STOCK (derived server-side).
    update_offer_stock(
        &store,
        UpdateOfferStock {
            catalog_id,
            restaurant_id,
            offer_id,
            quantity: Quantity(5.0),
            low_stock_threshold: None,
            expires_at: None,
        },
        &actor(),
    )
    .await
    .expect("set stock");
    let events = store.stream(&stream(catalog_id));
    assert!(matches!(
        events.last().unwrap(),
        DomainEvent::OfferStockUpdated(e)
            if e.offer_id == offer_id
                && e.stock.quantity.0 == 5.0
                && e.stock.status == StockStatus::IN_STOCK
    ));

    // quantity 3 with threshold 4 → LOW_STOCK; quantity 0 → OUT_OF_STOCK.
    update_offer_stock(
        &store,
        UpdateOfferStock {
            catalog_id,
            restaurant_id,
            offer_id,
            quantity: Quantity(3.0),
            low_stock_threshold: Some(Quantity(4.0)),
            expires_at: None,
        },
        &actor(),
    )
    .await
    .expect("low stock");
    let events = store.stream(&stream(catalog_id));
    assert!(matches!(
        events.last().unwrap(),
        DomainEvent::OfferStockUpdated(e) if e.stock.status == StockStatus::LOW_STOCK
    ));
    update_offer_stock(
        &store,
        UpdateOfferStock {
            catalog_id,
            restaurant_id,
            offer_id,
            quantity: Quantity(0.0),
            low_stock_threshold: None,
            expires_at: None,
        },
        &actor(),
    )
    .await
    .expect("out of stock");
    let events = store.stream(&stream(catalog_id));
    assert!(matches!(
        events.last().unwrap(),
        DomainEvent::OfferStockUpdated(e) if e.stock.status == StockStatus::OUT_OF_STOCK
    ));

    // Unknown offer id → OfferNotFound.
    let err = update_offer_stock(
        &store,
        UpdateOfferStock {
            catalog_id,
            restaurant_id,
            offer_id: OfferId(uuid::Uuid::new_v4()),
            quantity: Quantity(5.0),
            low_stock_threshold: None,
            expires_at: None,
        },
        &actor(),
    )
    .await
    .expect_err("missing offer");
    assert_eq!(rejection_code(&err), Some("OfferNotFound"));
}

// ------------------------------------------------------------------------------------------------
// Import (rules.yaml#/CatalogImportReplacesContent)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestCatalogImported + TestCatalogImportIsRejected —
/// rules.yaml#/CatalogImportReplacesContent. The CatalogTranslationFailed arm belongs to the HubRise
/// ACL translation step (before the command exists) and is not assertable here.
#[tokio::test]
async fn imports_a_catalog_and_rejects_a_missing_catalog_or_a_missing_ref() {
    let (store, _repo, catalog_id, restaurant_id) = given_catalog();

    import_catalog(
        &store,
        ImportCatalog {
            catalog_id,
            restaurant_id,
            source: "HUBRISE".into(),
            categories: vec![],
            products: vec![],
            option_lists: vec![],
        },
        &actor(),
    )
    .await
    .expect("import");
    let events = store.stream(&stream(catalog_id));
    assert!(matches!(
        events.last().unwrap(),
        DomainEvent::CatalogImported(e) if e.source == "HUBRISE"
    ));

    // Missing catalog → CatalogNotFound.
    let err = import_catalog(
        &store,
        ImportCatalog {
            catalog_id: cid(),
            restaurant_id,
            source: "HUBRISE".into(),
            categories: vec![],
            products: vec![],
            option_lists: vec![],
        },
        &actor(),
    )
    .await
    .expect_err("missing catalog");
    assert_eq!(rejection_code(&err), Some("CatalogNotFound"));

    // An imported category without its ref (the idempotency key) → MissingRef.
    let err = import_catalog(
        &store,
        ImportCatalog {
            catalog_id,
            restaurant_id,
            source: "HUBRISE".into(),
            categories: vec![category(
                ProductCategoryId(uuid::Uuid::new_v4()),
                catalog_id,
                None, // no ref
                None,
                "Pizzas",
            )],
            products: vec![],
            option_lists: vec![],
        },
        &actor(),
    )
    .await
    .expect_err("missing ref");
    assert_eq!(rejection_code(&err), Some("MissingRef"));
}
