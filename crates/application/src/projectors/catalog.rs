//! Hand-written `CatalogCompute` (ADR-0040). The category→product→offer tree is a nested merge over many
//! catalog events; it lands with the runtime. `slug` is a spec hole.
#![allow(unused_variables)]

use crate::projections::{CatalogCompute, CatalogRow, Envelope};
use domain::generated::scalars::Slug;
use serde_json::Value;

pub struct CatalogProjector;

impl CatalogCompute for CatalogProjector {
    /// ⚠️ HOLE: CatalogCreated carries no slug (spec) — preserve, else empty. TODO(spec): add a slug to the
    /// event, or derive it from the restaurant.
    fn slug(&self, prev: Option<&CatalogRow>, env: &Envelope) -> Slug {
        prev.map(|r| r.slug.clone()).unwrap_or_else(|| Slug(String::new()))
    }

    /// The assembled category→product→offer tree (+ derived stock_status / uberPrice) — a nested merge of
    /// CatalogCategoryAdded/Updated/Removed, Product*, OptionList*, OfferStockUpdated, CatalogImported.
    /// TODO(runtime): full tree merge; preserved meanwhile.
    fn tree(&self, prev: Option<&CatalogRow>, env: &Envelope) -> Value {
        prev.map(|r| r.tree.clone()).unwrap_or_else(|| Value::Object(Default::default()))
    }
}
