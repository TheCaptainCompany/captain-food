//! Catalog aggregate — the PURE write-side state fold (ADR-0035), mirroring `restaurant.rs`. Command
//! handlers rehydrate a [`CatalogState`] by folding the stream's events and enforce the invariants
//! declared in `specs/actors.yaml`/`specs/errors.yaml` against it: entity existence (product /
//! category / option list / offer), `ref` uniqueness within the catalog (the HubRise idempotent
//! import key), the category-tree shape (parent exists, no cycle, no removal while referenced) and
//! option-list usage. The full read model lives in the `Catalog` projection (ADR-0040), not here.
//! No I/O, no serialization logic (dependency rule).

use std::collections::HashSet;

use crate::generated::entities::{CatalogCategory, Offer, OptionList, Product};
use crate::generated::events::DomainEvent;
use crate::generated::scalars::{
    ExternalReference, OfferId, OptionListId, ProductCategoryId, ProductId, RestaurantId,
};

/// What the Catalog command handlers need to know to accept or reject a command. `None` (from
/// [`fold`]) means the catalog does not exist → `CatalogNotFound` (or the entity-level not-found the
/// message declares).
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogState {
    /// Owning restaurant — the currency authority for `CurrencyMismatch` lives on its projection row.
    pub restaurant_id: RestaurantId,
    /// The catalog's own external idempotent import key, when imported.
    pub r#ref: Option<ExternalReference>,
    /// Category tree (flat, parent links by `parentRef`) — tree invariants fold from here.
    pub categories: Vec<CatalogCategory>,
    /// Products with their offers (SKUs) — existence, refs, offer stock and option-list usage.
    pub products: Vec<Product>,
    /// Option lists (modifier groups).
    pub option_lists: Vec<OptionList>,
}

impl CatalogState {
    /// The category with this id, if present.
    pub fn category_by_id(&self, id: ProductCategoryId) -> Option<&CatalogCategory> {
        self.categories.iter().find(|c| c.id == id)
    }

    /// The category owning this `ref`, if any (categories are referenced by ref in `parentRef` /
    /// `categoryRef`).
    pub fn category_by_ref(&self, r: &ExternalReference) -> Option<&CatalogCategory> {
        self.categories.iter().find(|c| c.r#ref.as_ref() == Some(r))
    }

    /// The product with this id, if present.
    pub fn product_by_id(&self, id: ProductId) -> Option<&Product> {
        self.products.iter().find(|p| p.id == id)
    }

    /// The option list with this id, if present.
    pub fn option_list_by_id(&self, id: OptionListId) -> Option<&OptionList> {
        self.option_lists.iter().find(|l| l.id == id)
    }

    /// The offer with this id (and its owning product), if present.
    pub fn offer_by_id(&self, id: OfferId) -> Option<(&Product, &Offer)> {
        self.products
            .iter()
            .find_map(|p| p.offers.iter().find(|o| o.id == id).map(|o| (p, o)))
    }

    /// Every `ref` currently used in the catalog (catalog itself, categories, products, offers,
    /// option lists and options) — the `RefNotUnique` uniqueness scope.
    pub fn refs_in_use(&self) -> HashSet<&str> {
        let mut refs: HashSet<&str> = HashSet::new();
        refs.extend(self.r#ref.iter().map(|r| r.0.as_str()));
        refs.extend(self.categories.iter().filter_map(|c| c.r#ref.as_deref_str()));
        for p in &self.products {
            refs.extend(p.r#ref.as_deref_str());
            refs.extend(p.offers.iter().filter_map(|o| o.r#ref.as_deref_str()));
        }
        for l in &self.option_lists {
            refs.extend(l.r#ref.as_deref_str());
            refs.extend(l.options.iter().filter_map(|o| o.r#ref.as_deref_str()));
        }
        refs
    }

    /// Whether replacing `updated`'s previous version with it would make the category tree cyclic
    /// (`CatalogCategoryCycle`): walk the `parentRef` chain from the candidate; reaching the candidate
    /// itself — or looping longer than the tree — is a cycle. A dangling parent ref is NOT a cycle
    /// (that is `ParentCatalogCategoryNotFound`'s concern, and only on add).
    pub fn would_create_cycle(&self, updated: &CatalogCategory) -> bool {
        let view: Vec<&CatalogCategory> = self
            .categories
            .iter()
            .filter(|c| c.id != updated.id)
            .chain(std::iter::once(updated))
            .collect();
        let mut current = updated.parent_ref.as_ref();
        let mut hops = 0usize;
        while let Some(r) = current {
            hops += 1;
            if hops > view.len() {
                return true; // defensive: a pre-existing ref loop not passing through `updated`
            }
            let Some(parent) = view.iter().find(|c| c.r#ref.as_ref() == Some(r)) else {
                return false;
            };
            if parent.id == updated.id {
                return true;
            }
            current = parent.parent_ref.as_ref();
        }
        false
    }

    /// Whether this category still has child categories or products referencing it
    /// (`CatalogCategoryNotEmpty`). A category without a `ref` cannot be referenced, hence is empty.
    pub fn category_has_dependents(&self, category: &CatalogCategory) -> bool {
        let Some(r) = &category.r#ref else { return false };
        self.categories.iter().any(|c| c.id != category.id && c.parent_ref.as_ref() == Some(r))
            || self.products.iter().any(|p| p.category_ref.as_ref() == Some(r))
    }

    /// Whether any offer still references this option list (`OptionListInUse`).
    pub fn option_list_in_use(&self, id: OptionListId) -> bool {
        self.products
            .iter()
            .any(|p| p.offers.iter().any(|o| o.option_list_ids.contains(&id)))
    }
}

/// Borrow an optional `ExternalReference` as `Option<&str>` (helper for [`CatalogState::refs_in_use`]).
trait AsDerefStr {
    fn as_deref_str(&self) -> Option<&str>;
}

impl AsDerefStr for Option<ExternalReference> {
    fn as_deref_str(&self) -> Option<&str> {
        self.as_ref().map(|r| r.0.as_str())
    }
}

/// Fold a Catalog stream (events in version order) into its current state. `None` ⇔ the stream has no
/// `CatalogCreated` yet, i.e. the catalog does not exist.
pub fn fold(events: &[DomainEvent]) -> Option<CatalogState> {
    events.iter().fold(None, apply)
}

/// Apply one event — a pure transition, total over the whole event union. `*Added`/`*Updated` upsert
/// by id (replace semantics per the specs), `*Removed` deletes by id, `CatalogImported` replaces the
/// whole content, `OfferStockUpdated` sets the offer's stock in place.
fn apply(state: Option<CatalogState>, event: &DomainEvent) -> Option<CatalogState> {
    if let DomainEvent::CatalogCreated(e) = event {
        return Some(CatalogState {
            restaurant_id: e.restaurant_id,
            r#ref: e.r#ref.clone(),
            categories: vec![],
            products: vec![],
            option_lists: vec![],
        });
    }
    let mut s = state?;
    match event {
        DomainEvent::CatalogCategoryAdded(e) => upsert_category(&mut s, &e.category),
        DomainEvent::CatalogCategoryUpdated(e) => upsert_category(&mut s, &e.category),
        DomainEvent::CatalogCategoryRemoved(e) => {
            s.categories.retain(|c| c.id != e.product_category_id);
        }
        DomainEvent::ProductAdded(e) => upsert_product(&mut s, &e.product),
        DomainEvent::ProductUpdated(e) => upsert_product(&mut s, &e.product),
        DomainEvent::ProductRemoved(e) => {
            s.products.retain(|p| p.id != e.product_id);
        }
        DomainEvent::OptionListAdded(e) => upsert_option_list(&mut s, &e.option_list),
        DomainEvent::OptionListUpdated(e) => upsert_option_list(&mut s, &e.option_list),
        DomainEvent::OptionListRemoved(e) => {
            s.option_lists.retain(|l| l.id != e.option_list_id);
        }
        DomainEvent::OfferStockUpdated(e) => {
            for p in &mut s.products {
                for o in &mut p.offers {
                    if o.id == e.offer_id {
                        o.stock = Some(e.stock.clone());
                    }
                }
            }
        }
        DomainEvent::CatalogImported(e) => {
            s.categories = e.categories.clone();
            s.products = e.products.clone();
            s.option_lists = e.option_lists.clone();
        }
        _ => {}
    }
    Some(s)
}

/// Upsert-by-id (replace semantics for `*Added`/`*Updated`, per the specs).
fn upsert_category(s: &mut CatalogState, category: &CatalogCategory) {
    s.categories.retain(|c| c.id != category.id);
    s.categories.push(category.clone());
}

/// Upsert-by-id (replace semantics for `*Added`/`*Updated`, per the specs).
fn upsert_product(s: &mut CatalogState, product: &Product) {
    s.products.retain(|p| p.id != product.id);
    s.products.push(product.clone());
}

/// Upsert-by-id (replace semantics for `*Added`/`*Updated`, per the specs).
fn upsert_option_list(s: &mut CatalogState, option_list: &OptionList) {
    s.option_lists.retain(|l| l.id != option_list.id);
    s.option_lists.push(option_list.clone());
}
