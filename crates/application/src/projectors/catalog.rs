//! Hand-written `CatalogCompute` (ADR-0040). `tree` is the assembled category→product→offer(+option
//! lists) read model (specs/database/tables/projection_tables.yaml#/Catalog): a pure per-event fold
//! over `prev.tree`, keyed by each node's `id`. `*Updated` events carry the full entity (replace
//! semantics, specs/events.yaml), `CatalogImported` replaces the whole content
//! (rules.yaml#/CatalogImportReplacesContent), and `OfferStockUpdated` patches one offer's stock in
//! place. Each offer/option node is enriched with the DERIVED `stockStatus`
//! (quantity vs lowStockThreshold, scalars.yaml#/StockStatus) so the GraphQL `Offer.stockStatus`
//! deserializes straight out of the jsonb. `slug` is now carried by `CatalogCreated` and mapped by the
//! generator (it was a spec hole until then); the per-offer `uberPrice` /
//! `uberPriceBasis` (ADR-0022/0024) need the restaurant's cuisine_category + UberEstimationPolicy —
//! cross-stream + referential state — and stay TODO(runtime) (the GraphQL field is nullable).
#![allow(unused_variables)]

use crate::projections::{CatalogCompute, CatalogRow, Envelope};
use domain::generated::entities::{CatalogCategory, OptionList, Product, Stock};
use domain::generated::events::DomainEvent;
use domain::generated::scalars::{Slug, StockStatus};
use serde_json::{json, Value};

pub struct CatalogProjector;

/// The DERIVED stock status (scalars.yaml#/StockStatus, entities.yaml#/Stock): `None` = the item does
/// not track stock, so it never blocks ordering (IN_STOCK); quantity ≤ 0 = OUT_OF_STOCK; quantity at
/// or under `lowStockThreshold` = LOW_STOCK. The carried `Stock.status` is ignored — this derivation
/// is canonical (projection_tables.yaml#/Catalog rules).
pub fn derive_stock_status(stock: Option<&Stock>) -> StockStatus {
    match stock {
        None => StockStatus::IN_STOCK,
        Some(s) if s.quantity.0 <= 0.0 => StockStatus::OUT_OF_STOCK,
        Some(s) if s.low_stock_threshold.map_or(false, |t| s.quantity.0 <= t.0) => {
            StockStatus::LOW_STOCK
        }
        Some(_) => StockStatus::IN_STOCK,
    }
}

/// The empty tree — the shape the GraphQL layer reads sections out of (camelCase keys).
fn empty_tree() -> Value {
    json!({ "categories": [], "products": [], "optionLists": [] })
}

/// Mutable access to one section array, healing a legacy/empty `{}` tree into the canonical shape.
fn section_mut<'a>(tree: &'a mut Value, key: &str) -> &'a mut Vec<Value> {
    if !tree.is_object() {
        *tree = empty_tree();
    }
    let obj = tree.as_object_mut().expect("tree is a JSON object");
    let slot = obj.entry(key.to_string()).or_insert_with(|| Value::Array(Vec::new()));
    if !slot.is_array() {
        *slot = Value::Array(Vec::new());
    }
    slot.as_array_mut().expect("section is a JSON array")
}

/// Insert-or-replace a node in a section, keyed by its `id` (replace semantics for `*Updated`; an
/// `*Added` replay of an existing id is absorbed the same way).
fn upsert_node(tree: &mut Value, key: &str, node: Value) {
    let items = section_mut(tree, key);
    let id = node.get("id").cloned();
    match items.iter_mut().find(|item| item.get("id") == id.as_ref()) {
        Some(existing) => *existing = node,
        None => items.push(node),
    }
}

/// Remove a node from a section by `id` (a no-op when absent — removal replays are idempotent).
fn remove_node(tree: &mut Value, key: &str, id: &Value) {
    section_mut(tree, key).retain(|item| item.get("id") != Some(id));
}

/// A category node — the domain entity's camelCase serialization as-is.
fn category_node(category: &CatalogCategory) -> Value {
    serde_json::to_value(category).unwrap_or(Value::Null)
}

/// A product node: the domain entity plus the derived `stockStatus` on each of its offers.
fn product_node(product: &Product) -> Value {
    let mut node = serde_json::to_value(product).unwrap_or(Value::Null);
    if let Some(offers) = node.get_mut("offers").and_then(Value::as_array_mut) {
        for (offer_node, offer) in offers.iter_mut().zip(&product.offers) {
            if let Some(map) = offer_node.as_object_mut() {
                map.insert("stockStatus".into(), status_value(derive_stock_status(offer.stock.as_ref())));
            }
        }
    }
    node
}

/// An option-list node: the domain entity plus the derived `stockStatus` on each of its options.
fn option_list_node(option_list: &OptionList) -> Value {
    let mut node = serde_json::to_value(option_list).unwrap_or(Value::Null);
    if let Some(options) = node.get_mut("options").and_then(Value::as_array_mut) {
        for (option_node, option) in options.iter_mut().zip(&option_list.options) {
            if let Some(map) = option_node.as_object_mut() {
                map.insert("stockStatus".into(), status_value(derive_stock_status(option.stock.as_ref())));
            }
        }
    }
    node
}

fn status_value(status: StockStatus) -> Value {
    serde_json::to_value(status).unwrap_or(Value::Null)
}

/// Patch one offer's `stock` + derived `stockStatus` in place (`OfferStockUpdated`, e.g. HubRise
/// inventory sync). An unknown offer id is a no-op — the fact is kept in the log for a later import.
fn update_offer_stock(tree: &mut Value, offer_id: &Value, stock: &Stock) {
    for product in section_mut(tree, "products").iter_mut() {
        let Some(offers) = product.get_mut("offers").and_then(Value::as_array_mut) else { continue };
        for offer in offers.iter_mut() {
            if offer.get("id") == Some(offer_id) {
                if let Some(map) = offer.as_object_mut() {
                    map.insert("stock".into(), serde_json::to_value(stock).unwrap_or(Value::Null));
                    map.insert("stockStatus".into(), status_value(derive_stock_status(Some(stock))));
                }
            }
        }
    }
}

impl CatalogCompute for CatalogProjector {
    /// The assembled category→product→offer tree (+ derived per-offer/option `stockStatus`) — see the
    /// module doc for the fold semantics. `uberPrice`/`uberPriceBasis` are TODO(runtime) (cross-stream).
    fn tree(&self, prev: Option<&CatalogRow>, env: &Envelope) -> Value {
        let mut tree = prev.map(|r| r.tree.clone()).unwrap_or_else(empty_tree);
        match &env.event {
            DomainEvent::CatalogCreated(_) => empty_tree(),
            DomainEvent::CatalogCategoryAdded(e) => {
                upsert_node(&mut tree, "categories", category_node(&e.category));
                tree
            }
            DomainEvent::CatalogCategoryUpdated(e) => {
                upsert_node(&mut tree, "categories", category_node(&e.category));
                tree
            }
            // NOTE: no cascade — products reference their category by `categoryRef` (an optional
            // ExternalReference), and the spec defines no orphan semantics; the products stay.
            DomainEvent::CatalogCategoryRemoved(e) => {
                remove_node(&mut tree, "categories", &json!(e.product_category_id));
                tree
            }
            DomainEvent::ProductAdded(e) => {
                upsert_node(&mut tree, "products", product_node(&e.product));
                tree
            }
            DomainEvent::ProductUpdated(e) => {
                upsert_node(&mut tree, "products", product_node(&e.product));
                tree
            }
            DomainEvent::ProductRemoved(e) => {
                remove_node(&mut tree, "products", &json!(e.product_id));
                tree
            }
            DomainEvent::OptionListAdded(e) => {
                upsert_node(&mut tree, "optionLists", option_list_node(&e.option_list));
                tree
            }
            DomainEvent::OptionListUpdated(e) => {
                upsert_node(&mut tree, "optionLists", option_list_node(&e.option_list));
                tree
            }
            DomainEvent::OptionListRemoved(e) => {
                remove_node(&mut tree, "optionLists", &json!(e.option_list_id));
                tree
            }
            DomainEvent::OfferStockUpdated(e) => {
                update_offer_stock(&mut tree, &json!(e.offer_id), &e.stock);
                tree
            }
            // Full replace (rules.yaml#/CatalogImportReplacesContent): the imported content IS the
            // catalog — anything previously projected and not re-imported is gone.
            DomainEvent::CatalogImported(e) => {
                let mut imported = empty_tree();
                for category in &e.categories {
                    upsert_node(&mut imported, "categories", category_node(category));
                }
                for product in &e.products {
                    upsert_node(&mut imported, "products", product_node(product));
                }
                for option_list in &e.option_lists {
                    upsert_node(&mut imported, "optionLists", option_list_node(option_list));
                }
                imported
            }
            _ => tree,
        }
    }
}

#[cfg(test)]
mod tests {
    //! Offline fold tests for the Catalog `tree` projection, driven through the GENERATED
    //! `project_catalog` dispatch exactly like the projection worker does. They assert the read side
    //! of rules.yaml#/CatalogCategoryTreeManagement, #/CatalogProductManagement,
    //! #/CatalogOptionListManagement, #/OfferStockManualOrSynced and #/CatalogImportReplacesContent
    //! (the command side lives in crates/application/tests/catalog_behaviour.rs; the DB slice in
    //! crates/infrastructure/tests/catalog_projection.rs).

    use super::*;
    use crate::projections::project_catalog;
    use domain::generated::entities::{Money, Offer, TaxRate};
    use domain::generated::events::{
        CatalogCategoryAdded, CatalogCategoryRemoved, CatalogCreated, CatalogImported,
        OfferStockUpdated, OptionListAdded, ProductAdded, ProductUpdated,
    };
    use domain::generated::scalars::*;

    fn ts(secs: i64) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn envelope(event: DomainEvent, position: i64) -> Envelope {
        Envelope {
            stream_name: "Catalog-test".into(),
            position,
            occurred_at: ts(1_700_000_000 + position),
            event,
        }
    }

    /// Fold `events` through the generated dispatch + this Compute impl, like the worker does.
    fn project(events: Vec<DomainEvent>) -> CatalogRow {
        let mut row = None;
        for (i, event) in events.into_iter().enumerate() {
            row = project_catalog(&CatalogProjector, row, &envelope(event, i as i64 + 1));
        }
        row.expect("catalog projected")
    }

    fn catalog_id() -> CatalogId {
        CatalogId(uuid::Uuid::from_u128(1))
    }
    fn restaurant_id() -> RestaurantId {
        RestaurantId(uuid::Uuid::from_u128(2))
    }

    fn created() -> DomainEvent {
        DomainEvent::CatalogCreated(CatalogCreated {
            catalog_id: catalog_id(),
            r#ref: None,
            restaurant_id: restaurant_id(),
            name: CatalogName("Main menu".into()),
            slug: Slug("main-menu".into()),
        })
    }

    fn category(id: u128, name: &str) -> CatalogCategory {
        CatalogCategory {
            id: ProductCategoryId(uuid::Uuid::from_u128(id)),
            r#ref: None,
            catalog_id: catalog_id(),
            parent_ref: None,
            name: CatalogCategoryName(name.into()),
            description: None,
            tags: vec![],
            image_ids: vec![],
        }
    }

    fn stock(quantity: f64, low: Option<f64>) -> Stock {
        Stock {
            quantity: Quantity(quantity),
            low_stock_threshold: low.map(Quantity),
            // The carried status is deliberately WRONG — the projector must re-derive it.
            status: StockStatus::IN_STOCK,
            expires_at: None,
        }
    }

    fn offer(id: u128, stock: Option<Stock>) -> Offer {
        Offer {
            id: OfferId(uuid::Uuid::from_u128(id)),
            r#ref: None,
            product_id: ProductId(uuid::Uuid::from_u128(100)),
            name: OfferName("Regular".into()),
            price: Money { amount_cents: MoneyCents(980), currency: CurrencyCode("EUR".into()) },
            availability: CatalogItemAvailability::AVAILABLE,
            stock,
            option_list_ids: vec![],
        }
    }

    fn product(name: &str, offers: Vec<Offer>) -> Product {
        Product {
            id: ProductId(uuid::Uuid::from_u128(100)),
            r#ref: None,
            catalog_id: catalog_id(),
            restaurant_id: restaurant_id(),
            category_ref: None,
            name: ProductName(name.into()),
            description: None,
            tags: vec![],
            image_ids: vec![],
            tax_rate: TaxRate { delivery: TaxRatePercent(10.0), collection: None, eat_in: None },
            offers,
        }
    }

    #[test]
    fn folds_category_product_and_option_list_into_the_nested_tree() {
        let option_list = OptionList {
            id: OptionListId(uuid::Uuid::from_u128(300)),
            r#ref: None,
            name: OptionListName("Sauces".into()),
            min_selections: 0,
            max_selections: Some(2),
            multiple_selection: false,
            options: vec![],
        };
        let row = project(vec![
            created(),
            DomainEvent::CatalogCategoryAdded(CatalogCategoryAdded {
                catalog_id: catalog_id(),
                restaurant_id: restaurant_id(),
                category: category(10, "Pizzas"),
            }),
            DomainEvent::ProductAdded(ProductAdded {
                catalog_id: catalog_id(),
                restaurant_id: restaurant_id(),
                product: product("Margherita", vec![offer(200, Some(stock(5.0, Some(2.0))))]),
            }),
            DomainEvent::OptionListAdded(OptionListAdded {
                catalog_id: catalog_id(),
                restaurant_id: restaurant_id(),
                option_list,
            }),
        ]);

        assert_eq!(row.tree["categories"][0]["name"], json!("Pizzas"));
        assert_eq!(row.tree["products"][0]["name"], json!("Margherita"));
        let offer_node = &row.tree["products"][0]["offers"][0];
        assert_eq!(offer_node["price"]["amountCents"], json!(980));
        assert_eq!(offer_node["availability"], json!("AVAILABLE"));
        // quantity 5 > threshold 2 → IN_STOCK, DERIVED (not read from the carried status).
        assert_eq!(offer_node["stockStatus"], json!("IN_STOCK"));
        assert_eq!(row.tree["optionLists"][0]["name"], json!("Sauces"));
    }

    #[test]
    fn product_updated_replaces_and_offer_stock_updated_patches_the_derived_status() {
        let base = vec![
            created(),
            DomainEvent::ProductAdded(ProductAdded {
                catalog_id: catalog_id(),
                restaurant_id: restaurant_id(),
                product: product("Margherita", vec![offer(200, None)]),
            }),
        ];

        // Full-replace semantics on ProductUpdated (same id, new name).
        let mut replaced = base.clone();
        replaced.push(DomainEvent::ProductUpdated(ProductUpdated {
            catalog_id: catalog_id(),
            restaurant_id: restaurant_id(),
            product: product("Regina", vec![offer(200, None)]),
        }));
        let row = project(replaced);
        assert_eq!(row.tree["products"].as_array().unwrap().len(), 1, "replaced, not duplicated");
        assert_eq!(row.tree["products"][0]["name"], json!("Regina"));

        // OfferStockUpdated patches stock + re-derives the status: 1 ≤ threshold 2 → LOW_STOCK.
        let mut low = base.clone();
        low.push(DomainEvent::OfferStockUpdated(OfferStockUpdated {
            catalog_id: catalog_id(),
            restaurant_id: restaurant_id(),
            offer_id: OfferId(uuid::Uuid::from_u128(200)),
            stock: stock(1.0, Some(2.0)),
        }));
        let row = project(low);
        assert_eq!(row.tree["products"][0]["offers"][0]["stockStatus"], json!("LOW_STOCK"));

        // Quantity 0 → OUT_OF_STOCK.
        let mut out = base;
        out.push(DomainEvent::OfferStockUpdated(OfferStockUpdated {
            catalog_id: catalog_id(),
            restaurant_id: restaurant_id(),
            offer_id: OfferId(uuid::Uuid::from_u128(200)),
            stock: stock(0.0, None),
        }));
        let row = project(out);
        assert_eq!(row.tree["products"][0]["offers"][0]["stockStatus"], json!("OUT_OF_STOCK"));
    }

    #[test]
    fn removals_and_import_replace_the_content() {
        // CatalogCategoryRemoved deletes the node (idempotent on replay).
        let row = project(vec![
            created(),
            DomainEvent::CatalogCategoryAdded(CatalogCategoryAdded {
                catalog_id: catalog_id(),
                restaurant_id: restaurant_id(),
                category: category(10, "Pizzas"),
            }),
            DomainEvent::CatalogCategoryRemoved(CatalogCategoryRemoved {
                catalog_id: catalog_id(),
                restaurant_id: restaurant_id(),
                product_category_id: ProductCategoryId(uuid::Uuid::from_u128(10)),
            }),
        ]);
        assert_eq!(row.tree["categories"], json!([]));

        // CatalogImported REPLACES whatever was there (rules.yaml#/CatalogImportReplacesContent).
        let row = project(vec![
            created(),
            DomainEvent::ProductAdded(ProductAdded {
                catalog_id: catalog_id(),
                restaurant_id: restaurant_id(),
                product: product("Old product", vec![offer(200, None)]),
            }),
            DomainEvent::CatalogImported(CatalogImported {
                catalog_id: catalog_id(),
                restaurant_id: restaurant_id(),
                source: "hubrise".into(),
                categories: vec![category(11, "Burgers")],
                products: vec![product("Imported", vec![offer(201, None)])],
                option_lists: vec![],
            }),
        ]);
        assert_eq!(row.tree["categories"][0]["name"], json!("Burgers"));
        let products = row.tree["products"].as_array().unwrap();
        assert_eq!(products.len(), 1, "import replaced the old content");
        assert_eq!(products[0]["name"], json!("Imported"));
    }
}
