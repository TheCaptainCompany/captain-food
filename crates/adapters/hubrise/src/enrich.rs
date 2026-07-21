//! HubRise **domain enrichment** — the Anti-Corruption Layer that turns a pulled HubRise catalog /
//! inventory into Captain.Food domain writes (ADR-20260718-145856 / -213352). This is the "remaining
//! seam" the ingress ACL (`acl.rs`) and the outbound client (`api.rs`) were built for:
//!
//! ```text
//! POST /adapters/hubrise/webhooks──(verified, needs_pull)──▶ api pull ──▶ THIS ACL (map) ──▶ journal ──▶ domain write
//!   catalog callback   → get_catalog(catalogId)  → map_catalog  → command_journal → ImportCatalog handler
//!   inventory callback → get_inventory(locationId)→ map_inventory→ command_journal → update_offer_stock handler (per sku)
//! ```
//!
//! # Journaled sends (ADR-20260720-015300, #15)
//!
//! Every command this enricher issues goes through the WORKER-channel journaling dispatch
//! ([`application::dispatch::dispatch_journaled`]), never straight to a handler: `message_id` =
//! UUIDv5 of (callback id, command type[, offer id]) so a HubRise redelivery replays the SAME id and
//! dedupes on `command_journal` instead of double-applying; `cause_id` = UUIDv5(callback id) — the
//! mirrored callback's identity — so the chain `external_hubrise_callbacks → command_journal →
//! domain_events` is fully traceable end to end.
//!
//! # Why the two directions differ (CLAUDE.md "Commands vs inbound events")
//!
//! - **Catalog** is an *orchestrated* import we can reject (ACL validation, `MissingRef`, `CatalogNotFound`)
//!   → it goes through the **`ImportCatalog` command** handler, which emits `CatalogImported`.
//! - **Inventory** is a *reported fact* (stock already changed on the POS) → the stock update is an inbound
//!   event. We still route it through the `update_offer_stock` handler because that handler is the single
//!   source of truth for the `Catalog-<id>` stream/version and the `StockStatus` derivation; its only
//!   rejection, `OfferNotFound`, is exactly the "we don't know this SKU yet" case, which we **skip**
//!   (never surfaced as an error). So no fact is ever rejected — the request/report split holds.
//!
//! # Deterministic ids — the reconciliation with the Catalog aggregate
//!
//! HubRise ids never enter the domain. Every domain id is a **UUIDv5** of the HubRise identifier under a
//! fixed namespace (like the SIRENE ACL derives a `RestaurantId` from the SIRET), so a re-sync maps to the
//! SAME ids (idempotent) and — crucially — the `OfferId` an inventory update targets EQUALS the one
//! `ImportCatalog` assigned. The seed per entity is the identifier OTHER HubRise objects use to reference
//! it, so the graph re-joins after translation:
//!
//! | domain id          | seed (`kind:value`)                    | why that seed |
//! |--------------------|----------------------------------------|---------------|
//! | `CatalogId`        | `catalog:<hubrise catalog id>`         | the connect flow must create the `Catalog` with this id |
//! | `RestaurantId`     | `location:<hubrise location id>`       | idem — the location IS the restaurant |
//! | category id + ref  | `category:<hubrise category id>`       | products join by `category_id`; sub-categories by `parent_id` |
//! | product id + ref   | `product:<hubrise product id>`         | — |
//! | option-list id+ref | `option_list:<hubrise option_list id>` | SKUs join by `option_list_ids` |
//! | option id + ref    | `option:<hubrise option id>`           | — |
//! | **offer** id + ref | `sku:<hubrise SKU ref, else its id>`   | **inventory joins by `sku_ref`** — the SKU's *ref*, not its id |
//!
//! The one asymmetry: an Offer is seeded from the SKU **`ref`** (falling back to the SKU `id` when null),
//! because the inventory endpoint reports stock keyed by `sku_ref`. A SKU with no `ref` therefore also has
//! no reportable inventory, so the id fallback is never queried — consistent by construction.
//!
//! Everything HubRise-shaped (the `"9.80 EUR"` price string, decimal tax-rate strings, the `data` envelope)
//! is translated here and nowhere else (CLAUDE.md: keep external vocab out of the domain).

use std::sync::Arc;

use application::commands::{import_catalog, rejection_code, update_offer_stock};
use application::dispatch::{dispatch_journaled, JournaledOutcome};
use application::journal::{payload_hash, CommandJournal, CommandJournalEntry};
use application::ports::{Actor, EventStore};
use domain::generated::commands::{ImportCatalog, UpdateOfferStock};
use domain::shared::errors::DomainError;
use domain::generated::entities::{CatalogCategory, Money, Offer, OptionList, Product, ProductItemOption, TaxRate};
use domain::generated::scalars::{
    CatalogCategoryName, CatalogId, CatalogItemAvailability, CommandChannel, CommandJournalStatus,
    CurrencyCode, ExternalReference, MoneyCents,
    OfferId, OfferName, OptionId, OptionListId, OptionListName, OptionName, ProductCategoryId,
    ProductDescription, ProductId, ProductName, Quantity, RestaurantId, Tag, TaxRatePercent,
};

use crate::acl::HubRiseCallback;
use crate::api::HubRiseApiClient;

/// The `source` recorded on every `CatalogImported` from this ACL.
pub const HUBRISE_SOURCE: &str = "hubrise";

/// `UserType::EXTERNAL` ordinal for the event envelope (enums stored as declaration-order ints,
/// ADR-0037/0041) — HubRise facts are recorded as the fixed external-system principal, like Stripe's.
const EXTERNAL_USER_TYPE: i32 = 6;

// ================================================================================================
// Deterministic identity (ADR-0041) — UUIDv5 of the HubRise identifier, like the SIRENE ACL's
// ================================================================================================

/// Fixed UUIDv5 namespace for every id this ACL derives. NEVER change it: derived ids must stay stable
/// across deliveries and deployments so re-syncs are idempotent and inventory targets the imported offer.
fn hubrise_namespace() -> uuid::Uuid {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"https://captain.food/integrations/hubrise")
}

/// `UUIDv5(namespace, "<kind>:<seed>")` — the `kind` prefix keeps two entity types that happen to share a
/// HubRise id string from colliding on the same domain uuid.
fn derive(kind: &str, seed: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(&hubrise_namespace(), format!("{kind}:{seed}").as_bytes())
}

/// Our `CatalogId` for a HubRise catalog id. The HubRise **connect flow** must `CreateCatalog` with THIS
/// id, otherwise `ImportCatalog` rejects `CatalogNotFound` (handled as a skip, logged).
pub fn derive_catalog_id(hubrise_catalog_id: &str) -> CatalogId {
    CatalogId(derive("catalog", hubrise_catalog_id))
}

/// Our `RestaurantId` for a HubRise location id (the location IS the restaurant).
pub fn derive_restaurant_id(hubrise_location_id: &str) -> RestaurantId {
    RestaurantId(derive("location", hubrise_location_id))
}

/// Our `OfferId` for a SKU seed — the SKU **`ref`** (what inventory's `sku_ref` carries), or its `id` when
/// the SKU has no ref. `map_inventory` calls this with `sku_ref`; `map_catalog` with the ref-or-id seed.
pub fn derive_offer_id(sku_seed: &str) -> OfferId {
    OfferId(derive("sku", sku_seed))
}

fn derive_category_id(hubrise_category_id: &str) -> ProductCategoryId {
    ProductCategoryId(derive("category", hubrise_category_id))
}
fn derive_product_id(hubrise_product_id: &str) -> ProductId {
    ProductId(derive("product", hubrise_product_id))
}
fn derive_option_list_id(hubrise_option_list_id: &str) -> OptionListId {
    OptionListId(derive("option_list", hubrise_option_list_id))
}
fn derive_option_id(hubrise_option_id: &str) -> OptionId {
    OptionId(derive("option", hubrise_option_id))
}

// ================================================================================================
// Boundary value parsing — the ONLY place HubRise's string encodings are allowed to exist
// ================================================================================================

/// Why a HubRise payload could not be translated (the `CatalogTranslationFailed` boundary, CLAUDE.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapError {
    /// The pulled JSON did not match the expected HubRise shape.
    Shape(String),
    /// A `"<amount> <CURRENCY>"` money string was malformed.
    Money(String),
    /// A required identifier (catalog / location) was missing from the callback + payload.
    MissingIdentifier(String),
}

impl std::fmt::Display for MapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shape(e) => write!(f, "unexpected HubRise payload shape: {e}"),
            Self::Money(e) => write!(f, "malformed HubRise money string: {e}"),
            Self::MissingIdentifier(e) => write!(f, "missing HubRise identifier: {e}"),
        }
    }
}
impl std::error::Error for MapError {}

/// Parse a HubRise money string (`"9.80 EUR"`, `"10 EUR"`, `"9.8 EUR"`) into the domain `Money` value
/// object — integer minor units + uppercased ISO currency. Decimals are handled by string arithmetic (no
/// float rounding): the fractional part is padded/truncated to exactly two digits.
pub fn parse_money(raw: &str) -> Result<Money, MapError> {
    let mut parts = raw.split_whitespace();
    let amount = parts.next().ok_or_else(|| MapError::Money(format!("empty: {raw:?}")))?;
    let currency = parts.next().ok_or_else(|| MapError::Money(format!("no currency: {raw:?}")))?;
    let cents = amount_to_cents(amount).ok_or_else(|| MapError::Money(format!("amount: {raw:?}")))?;
    Ok(Money { amount_cents: MoneyCents(cents), currency: CurrencyCode(currency.to_uppercase()) })
}

/// `"9.80"` → `980`, `"10"` → `1000`, `"-1.5"` → `-150`. `None` on non-numeric input.
fn amount_to_cents(amount: &str) -> Option<i64> {
    let (neg, digits) = match amount.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, amount.strip_prefix('+').unwrap_or(amount)),
    };
    let (int_part, frac_part) = match digits.split_once('.') {
        Some((i, f)) => (i, f),
        None => (digits, ""),
    };
    // Pad/truncate the fraction to exactly two digits.
    let mut frac = frac_part.to_string();
    frac.truncate(2);
    while frac.len() < 2 {
        frac.push('0');
    }
    let int_val: i64 = if int_part.is_empty() { 0 } else { int_part.parse().ok()? };
    let frac_val: i64 = frac.parse().ok()?;
    let magnitude = int_val.checked_mul(100)?.checked_add(frac_val)?;
    Some(if neg { -magnitude } else { magnitude })
}

/// Parse a HubRise decimal tax-rate string (`"10.0"` = 10%) into `TaxRatePercent`; a missing/blank rate is
/// treated as 0%.
fn parse_tax_percent(raw: Option<&str>) -> TaxRatePercent {
    TaxRatePercent(raw.and_then(|s| s.trim().parse::<f64>().ok()).unwrap_or(0.0))
}

/// Coerce a HubRise stock quantity that may arrive as a JSON number OR a decimal string.
fn coerce_quantity(v: &serde_json::Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

// ================================================================================================
// Wire types — the HubRise subset this ACL reads (unknown fields ignored by serde)
// ================================================================================================

/// The content of a catalog — either the top-level object's `data` envelope or the object itself.
#[derive(Debug, Clone, Default, serde::Deserialize)]
struct HrCatalogContent {
    #[serde(default)]
    categories: Vec<HrCategory>,
    #[serde(default)]
    products: Vec<HrProduct>,
    // HubRise spells this `options_lists`; accept the singular-plural variant too, defensively.
    #[serde(default, alias = "option_lists")]
    options_lists: Vec<HrOptionList>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct HrCategory {
    id: String,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct HrProduct {
    id: String,
    #[serde(default)]
    category_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    tax_rate: Option<HrTaxRate>,
    #[serde(default)]
    skus: Vec<HrSku>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct HrTaxRate {
    #[serde(default)]
    delivery: Option<String>,
    #[serde(default)]
    collection: Option<String>,
    #[serde(default)]
    eat_in: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct HrSku {
    id: String,
    #[serde(rename = "ref", default)]
    hr_ref: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    price: Option<String>,
    #[serde(default)]
    option_list_ids: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct HrOptionList {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    min_selections: Option<i64>,
    #[serde(default)]
    max_selections: Option<i64>,
    #[serde(default)]
    multiple_selection: Option<bool>,
    #[serde(default)]
    options: Vec<HrOption>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct HrOption {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    price: Option<String>,
    #[serde(default)]
    default: bool,
}

/// One inventory line: stock for a SKU (`sku_ref`) or an option (`option_ref`). V0 maps SKU lines only.
#[derive(Debug, Clone, serde::Deserialize)]
struct HrInventoryLine {
    #[serde(default)]
    sku_ref: Option<String>,
    #[serde(default)]
    stock: serde_json::Value,
    #[serde(default)]
    expires_at: Option<String>,
}

// ================================================================================================
// Mapping — pure, the actual Anti-Corruption boundary (no I/O, unit-testable)
// ================================================================================================

/// Translate a pulled HubRise catalog into the `ImportCatalog` command. `hubrise_catalog_id` /
/// `hubrise_location_id` come from the verified callback and seed the (deterministic) `CatalogId` /
/// `RestaurantId`. Prices become `Money`; every entity gets a stable `ref` (its HubRise id, or the SKU's
/// ref) so the domain's `MissingRef` never fires and the tree re-joins.
pub fn map_catalog(
    catalog_json: &serde_json::Value,
    hubrise_catalog_id: &str,
    hubrise_location_id: &str,
) -> Result<ImportCatalog, MapError> {
    let catalog_id = derive_catalog_id(hubrise_catalog_id);
    let restaurant_id = derive_restaurant_id(hubrise_location_id);

    // Content is under `data` on the full object; tolerate a bare content object too.
    let content_val = catalog_json.get("data").cloned().unwrap_or_else(|| catalog_json.clone());
    let content: HrCatalogContent = serde_json::from_value(content_val)
        .map_err(|e| MapError::Shape(e.to_string()))?;

    let categories = content
        .categories
        .iter()
        .map(|c| CatalogCategory {
            id: derive_category_id(&c.id),
            r#ref: Some(ExternalReference(c.id.clone())),
            catalog_id,
            parent_ref: c.parent_id.clone().map(ExternalReference),
            name: CatalogCategoryName(c.name.clone().unwrap_or_default()),
            description: c.description.clone().map(ProductDescription),
            tags: c.tags.iter().cloned().map(Tag).collect(),
            image_ids: vec![], // HubRise image ids are opaque strings; not mapped in V0.
        })
        .collect();

    let mut products = Vec::with_capacity(content.products.len());
    for p in &content.products {
        let product_id = derive_product_id(&p.id);
        let mut offers = Vec::with_capacity(p.skus.len());
        for sku in &p.skus {
            // Seed the offer from the SKU ref (what inventory reports), falling back to its id.
            let seed = sku.hr_ref.clone().unwrap_or_else(|| sku.id.clone());
            let price = parse_money(
                sku.price
                    .as_deref()
                    .ok_or_else(|| MapError::Money(format!("SKU {} has no price", sku.id)))?,
            )?;
            offers.push(Offer {
                id: derive_offer_id(&seed),
                r#ref: Some(ExternalReference(seed)),
                product_id,
                name: OfferName(sku.name.clone().or_else(|| p.name.clone()).unwrap_or_default()),
                price,
                availability: CatalogItemAvailability::AVAILABLE, // manual UI flag; import defaults available.
                stock: None,                                      // inventory sync sets stock separately.
                option_list_ids: sku
                    .option_list_ids
                    .iter()
                    .map(|id| derive_option_list_id(id))
                    .collect(),
            });
        }
        products.push(Product {
            id: product_id,
            r#ref: Some(ExternalReference(p.id.clone())),
            catalog_id,
            restaurant_id,
            category_ref: p.category_id.clone().map(ExternalReference),
            name: ProductName(p.name.clone().unwrap_or_default()),
            description: p.description.clone().map(ProductDescription),
            tags: p.tags.iter().cloned().map(Tag).collect(),
            image_ids: vec![],
            tax_rate: map_tax_rate(p.tax_rate.as_ref()),
            offers,
        });
    }

    let mut option_lists = Vec::with_capacity(content.options_lists.len());
    for l in &content.options_lists {
        let option_list_id = derive_option_list_id(&l.id);
        let mut options = Vec::with_capacity(l.options.len());
        for o in &l.options {
            let price = parse_money(
                o.price
                    .as_deref()
                    .ok_or_else(|| MapError::Money(format!("option {} has no price", o.id)))?,
            )?;
            options.push(ProductItemOption {
                id: derive_option_id(&o.id),
                r#ref: Some(ExternalReference(o.id.clone())),
                option_list_id,
                name: OptionName(o.name.clone().unwrap_or_default()),
                price,
                r#default: o.default,
                availability: CatalogItemAvailability::AVAILABLE,
                stock: None,
            });
        }
        option_lists.push(OptionList {
            id: option_list_id,
            r#ref: Some(ExternalReference(l.id.clone())),
            name: OptionListName(l.name.clone().unwrap_or_default()),
            min_selections: l.min_selections.unwrap_or(0),
            max_selections: l.max_selections,
            multiple_selection: l.multiple_selection.unwrap_or(false),
            options,
        });
    }

    Ok(ImportCatalog {
        catalog_id,
        restaurant_id,
        source: HUBRISE_SOURCE.to_string(),
        categories,
        products,
        option_lists,
    })
}

fn map_tax_rate(raw: Option<&HrTaxRate>) -> TaxRate {
    match raw {
        Some(t) => TaxRate {
            delivery: parse_tax_percent(t.delivery.as_deref()),
            collection: t.collection.as_deref().map(|s| parse_tax_percent(Some(s))),
            eat_in: t.eat_in.as_deref().map(|s| parse_tax_percent(Some(s))),
        },
        None => TaxRate { delivery: TaxRatePercent(0.0), collection: None, eat_in: None },
    }
}

/// Translate a pulled HubRise inventory into per-offer `UpdateOfferStock` commands. Only SKU lines are
/// mapped (option-level stock has no command in V0). `low_stock_threshold` is `None` — HubRise inventory
/// carries no threshold, so the handler derives `IN_STOCK`/`OUT_OF_STOCK` from the quantity alone. Lines
/// whose quantity is unparseable are dropped defensively (an inbound fact is never rejected).
pub fn map_inventory(
    inventory_json: &serde_json::Value,
    catalog_id: CatalogId,
    restaurant_id: RestaurantId,
) -> Result<Vec<UpdateOfferStock>, MapError> {
    // The endpoint returns a bare array; tolerate a `{ "inventory": [...] }`/`{ "skus": [...] }` wrapper.
    let lines_val = inventory_json
        .get("inventory")
        .or_else(|| inventory_json.get("skus"))
        .cloned()
        .unwrap_or_else(|| inventory_json.clone());
    let lines: Vec<HrInventoryLine> =
        serde_json::from_value(lines_val).map_err(|e| MapError::Shape(e.to_string()))?;

    let mut updates = Vec::new();
    for line in &lines {
        let Some(sku_ref) = line.sku_ref.as_deref() else { continue }; // option_ref lines: skipped in V0.
        let Some(quantity) = coerce_quantity(&line.stock) else { continue };
        updates.push(UpdateOfferStock {
            catalog_id,
            restaurant_id,
            offer_id: derive_offer_id(sku_ref),
            quantity: Quantity(quantity),
            low_stock_threshold: None,
            expires_at: line.expires_at.clone(),
        });
    }
    Ok(updates)
}

// ================================================================================================
// Puller — the outbound HubRise API, behind a trait so the enricher is testable without HTTP
// ================================================================================================

/// Pulls the changed resource from HubRise after a (stateless) callback. Implemented by the real
/// [`HubRiseApiClient`]; faked in tests.
#[async_trait::async_trait]
pub trait HubRisePuller: Send + Sync {
    async fn pull_catalog(&self, hubrise_catalog_id: &str) -> Result<serde_json::Value, String>;
    async fn pull_inventory(&self, hubrise_location_id: &str) -> Result<serde_json::Value, String>;
}

#[async_trait::async_trait]
impl HubRisePuller for HubRiseApiClient {
    async fn pull_catalog(&self, hubrise_catalog_id: &str) -> Result<serde_json::Value, String> {
        self.get_catalog(hubrise_catalog_id).await.map_err(|e| e.to_string())
    }
    async fn pull_inventory(&self, hubrise_location_id: &str) -> Result<serde_json::Value, String> {
        self.get_inventory(hubrise_location_id).await.map_err(|e| e.to_string())
    }
}

// ================================================================================================
// Enricher — callback → pull → map → domain write
// ================================================================================================

/// What the enricher did with one verified callback (all are ACKed 2xx by the HTTP shell).
#[derive(Debug, Clone, PartialEq)]
pub enum EnrichOutcome {
    /// A catalog was imported (`CatalogImported` appended).
    CatalogImported { catalog_id: CatalogId },
    /// Inventory applied: `applied` offers updated, `skipped` unknown to our catalog (`OfferNotFound`).
    InventoryApplied { applied: usize, skipped: usize },
    /// The command was rejected in a definitive way (e.g. `CatalogNotFound` — the connect flow has not
    /// created the catalog yet). Logged, not retried.
    Skipped { reason: String },
    /// A resource type this ACL does not enrich.
    Ignored { resource_type: String },
    /// The API pull failed (transport/status) — the caller answers 5xx so HubRise redelivers.
    PullFailed { reason: String },
    /// The pulled payload could not be translated (`CatalogTranslationFailed`). Logged, not retried.
    MapFailed { reason: String },
}

/// Fixed system user id stamping the event envelope for facts HubRise reports (`domain_events.user_id`).
pub fn hubrise_system_user_id() -> uuid::Uuid {
    uuid::Uuid::new_v5(&hubrise_namespace(), b"system:hubrise-webhook")
}

/// Records HubRise-driven catalog/inventory writes through the WORKER-channel journaling dispatch and
/// the ordinary command handlers. Generic over the [`HubRisePuller`] so the dispatch (import, per-SKU
/// stock, `OfferNotFound` skip, journal dedup) is unit-testable in memory; `dyn EventStore` /
/// `dyn CommandJournal` keep it off the concrete Postgres adapters.
pub struct HubRiseEnricher<P: HubRisePuller> {
    store: Arc<dyn EventStore>,
    journal: Arc<dyn CommandJournal>,
    puller: P,
}

impl<P: HubRisePuller> HubRiseEnricher<P> {
    pub fn new(store: Arc<dyn EventStore>, journal: Arc<dyn CommandJournal>, puller: P) -> Self {
        Self { store, journal, puller }
    }

    /// The WORKER-channel journal entry for one command a callback caused (#15): `message_id` =
    /// UUIDv5(callback id, command type[, discriminator]) — deterministic, so a HubRise redelivery
    /// replays the same id and dedupes; the per-SKU discriminator keeps one inventory callback's many
    /// `UpdateOfferStock` sends distinct. `cause_id` = `correlation_id` = UUIDv5(callback id), the
    /// identity `external_hubrise_callbacks` mirrors — closing the mirror → journal → events chain.
    fn journal_entry(
        callback_id: &str,
        command_type: &str,
        discriminator: Option<&str>,
        payload: serde_json::Value,
    ) -> CommandJournalEntry {
        let seed = match discriminator {
            Some(d) => format!("{callback_id}:{command_type}:{d}"),
            None => format!("{callback_id}:{command_type}"),
        };
        let callback_uuid = derive("callback", callback_id);
        CommandJournalEntry {
            message_id: derive("command", &seed),
            correlation_id: callback_uuid,
            cause_id: Some(callback_uuid),
            session_id: None,
            trace_id: None,
            user_id: Some(hubrise_system_user_id()),
            user_type: EXTERNAL_USER_TYPE,
            channel: CommandChannel::WORKER,
            command_type: command_type.to_string(),
            payload_hash: payload_hash(&payload),
            payload,
        }
    }

    /// Envelope → `Actor` (ADR-0041): events appended by this journaled send carry
    /// `cause_id = message_id`, exactly like the GraphQL dispatch.
    fn actor_for(entry: &CommandJournalEntry) -> Actor {
        Actor {
            user_id: hubrise_system_user_id(),
            user_type: EXTERNAL_USER_TYPE,
            correlation_id: entry.correlation_id,
            cause_id: Some(entry.message_id),
        }
    }

    /// Enrich one verified callback. Only `Err(DomainError)` (event store / journal unreachable)
    /// should make the endpoint answer 5xx; every other outcome is definitive and ACKed.
    pub async fn enrich(
        &self,
        callback: &HubRiseCallback,
    ) -> Result<EnrichOutcome, DomainError> {
        match callback.resource_type.as_str() {
            "catalog" => self.enrich_catalog(callback).await,
            "inventory" => self.enrich_inventory(callback).await,
            other => Ok(EnrichOutcome::Ignored { resource_type: other.to_string() }),
        }
    }

    async fn enrich_catalog(
        &self,
        callback: &HubRiseCallback,
    ) -> Result<EnrichOutcome, DomainError> {
        let Some(catalog_id) = callback.catalog_id.as_deref() else {
            return Ok(EnrichOutcome::MapFailed {
                reason: "catalog callback has no catalog_id".to_string(),
            });
        };
        let Some(location_id) = callback.location_id.as_deref() else {
            return Ok(EnrichOutcome::MapFailed {
                reason: "catalog callback has no location_id (restaurant unknown)".to_string(),
            });
        };
        let json = match self.puller.pull_catalog(catalog_id).await {
            Ok(j) => j,
            Err(reason) => return Ok(EnrichOutcome::PullFailed { reason }),
        };
        let cmd = match map_catalog(&json, catalog_id, location_id) {
            Ok(c) => c,
            Err(e) => return Ok(EnrichOutcome::MapFailed { reason: e.to_string() }),
        };
        let payload = serde_json::to_value(&cmd)
            .map_err(|e| DomainError::Repository(format!("serialize ImportCatalog: {e}")))?;
        let entry = Self::journal_entry(&callback.id, "ImportCatalog", None, payload);
        let actor = Self::actor_for(&entry);
        let derived = cmd.catalog_id;
        let store = self.store.clone();
        let outcome = dispatch_journaled(self.journal.as_ref(), entry, move || async move {
            import_catalog(store.as_ref(), cmd, &actor).await
        })
        .await?;
        match outcome {
            JournaledOutcome::Executed(Ok(())) => {
                Ok(EnrichOutcome::CatalogImported { catalog_id: derived })
            }
            // A rejection (CatalogNotFound / MissingRef) is definitive — retrying won't help.
            JournaledOutcome::Executed(Err(e)) if rejection_code(&e).is_some() => {
                Ok(EnrichOutcome::Skipped { reason: e.to_string() })
            }
            JournaledOutcome::Executed(Err(e)) => Err(e),
            // Redelivery of an already-imported callback: same acknowledgement, no double-apply.
            JournaledOutcome::Deduplicated(CommandJournalStatus::SUCCEEDED) => {
                Ok(EnrichOutcome::CatalogImported { catalog_id: derived })
            }
            JournaledOutcome::Deduplicated(status) => Ok(EnrichOutcome::Skipped {
                reason: format!("redelivered callback deduplicated on the journal ({status:?})"),
            }),
            // The re-pull returned different content under a redelivered callback id: never
            // re-dispatched — the changed catalog arrives under its own fresh callback.
            JournaledOutcome::PayloadConflict { existing_status } => Ok(EnrichOutcome::Skipped {
                reason: format!(
                    "redelivered callback re-pulled a DIFFERENT catalog (journaled {existing_status:?}): not re-imported"
                ),
            }),
        }
    }

    async fn enrich_inventory(
        &self,
        callback: &HubRiseCallback,
    ) -> Result<EnrichOutcome, DomainError> {
        let Some(location_id) = callback.location_id.as_deref() else {
            return Ok(EnrichOutcome::MapFailed {
                reason: "inventory callback has no location_id".to_string(),
            });
        };
        let Some(catalog_ref) = callback.catalog_id.as_deref() else {
            return Ok(EnrichOutcome::MapFailed {
                reason: "inventory callback has no catalog_id".to_string(),
            });
        };
        let json = match self.puller.pull_inventory(location_id).await {
            Ok(j) => j,
            Err(reason) => return Ok(EnrichOutcome::PullFailed { reason }),
        };
        let catalog_id = derive_catalog_id(catalog_ref);
        let restaurant_id = derive_restaurant_id(location_id);
        let updates = match map_inventory(&json, catalog_id, restaurant_id) {
            Ok(u) => u,
            Err(e) => return Ok(EnrichOutcome::MapFailed { reason: e.to_string() }),
        };
        let (mut applied, mut skipped) = (0usize, 0usize);
        for cmd in updates {
            let payload = serde_json::to_value(&cmd)
                .map_err(|e| DomainError::Repository(format!("serialize UpdateOfferStock: {e}")))?;
            let offer = cmd.offer_id.0.to_string();
            let entry =
                Self::journal_entry(&callback.id, "UpdateOfferStock", Some(&offer), payload);
            let actor = Self::actor_for(&entry);
            let store = self.store.clone();
            let outcome = dispatch_journaled(self.journal.as_ref(), entry, move || async move {
                update_offer_stock(store.as_ref(), cmd, &actor).await
            })
            .await?;
            match outcome {
                JournaledOutcome::Executed(Ok(()))
                | JournaledOutcome::Deduplicated(CommandJournalStatus::SUCCEEDED) => applied += 1,
                // `OfferNotFound` = a SKU we haven't imported (or a different catalog): skip the fact.
                JournaledOutcome::Executed(Err(e)) if rejection_code(&e).is_some() => skipped += 1,
                JournaledOutcome::Executed(Err(e)) => return Err(e),
                // Journal dedup/conflict on a redelivered callback: skipped, never double-applied.
                JournaledOutcome::Deduplicated(_)
                | JournaledOutcome::PayloadConflict { .. } => skipped += 1,
            }
        }
        Ok(EnrichOutcome::InventoryApplied { applied, skipped })
    }
}

/// Object-safe façade so the HTTP shell can hold `Option<Arc<dyn Enricher>>` without leaking the puller
/// type parameter.
#[async_trait::async_trait]
pub trait Enricher: Send + Sync {
    async fn enrich(
        &self,
        callback: &HubRiseCallback,
    ) -> Result<EnrichOutcome, DomainError>;
}

#[async_trait::async_trait]
impl<P: HubRisePuller> Enricher for HubRiseEnricher<P> {
    async fn enrich(
        &self,
        callback: &HubRiseCallback,
    ) -> Result<EnrichOutcome, DomainError> {
        HubRiseEnricher::enrich(self, callback).await
    }
}

// ================================================================================================
// Tests
// ================================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use application::journal::mem::MemCommandJournal;
    use application::ports::version_conflict;
    use domain::generated::events::{CatalogCreated, DomainEvent};
    use domain::generated::scalars::CatalogName;

    fn enricher_with(
        store: Arc<InMemoryEventStore>,
        puller: FakePuller,
    ) -> (HubRiseEnricher<FakePuller>, Arc<MemCommandJournal>) {
        let journal = Arc::new(MemCommandJournal::default());
        (HubRiseEnricher::new(store, journal.clone(), puller), journal)
    }

    // ----- boundary value parsing -----

    #[test]
    fn parses_hubrise_money_without_float_rounding() {
        assert_eq!(
            parse_money("9.80 EUR").unwrap(),
            Money { amount_cents: MoneyCents(980), currency: CurrencyCode("EUR".into()) }
        );
        assert_eq!(parse_money("10 EUR").unwrap().amount_cents, MoneyCents(1000));
        assert_eq!(parse_money("9.8 eur").unwrap().amount_cents, MoneyCents(980));
        assert_eq!(parse_money("0.05 GBP").unwrap(), Money { amount_cents: MoneyCents(5), currency: CurrencyCode("GBP".into()) });
        // Currency is uppercased at the boundary.
        assert_eq!(parse_money("1.00 usd").unwrap().currency, CurrencyCode("USD".into()));
        assert!(parse_money("free").is_err());
    }

    // ----- catalog mapping -----

    fn sample_catalog_json() -> serde_json::Value {
        serde_json::json!({
            "id": "cat_1", "location_id": "loc_1", "name": "Menu",
            "data": {
                "categories": [
                    { "id": "c_burgers", "name": "Burgers" },
                    { "id": "c_veggie", "parent_id": "c_burgers", "name": "Veggie" }
                ],
                "products": [{
                    "id": "p_cheese", "category_id": "c_burgers", "name": "Cheeseburger",
                    "description": "Classic", "tags": ["beef"],
                    "tax_rate": { "delivery": "10.0", "eat_in": "5.5" },
                    "skus": [{
                        "id": "s_internal_1", "ref": "SKU-CHEESE", "name": "Regular",
                        "price": "9.80 EUR", "option_list_ids": ["ol_sauce"]
                    }]
                }],
                "options_lists": [{
                    "id": "ol_sauce", "name": "Sauce", "min_selections": 0, "max_selections": 1,
                    "multiple_selection": false,
                    "options": [{ "id": "o_ketchup", "name": "Ketchup", "price": "0.00 EUR", "default": true }]
                }]
            }
        })
    }

    #[test]
    fn maps_catalog_with_consistent_deterministic_ids() {
        let cmd = map_catalog(&sample_catalog_json(), "cat_1", "loc_1").unwrap();

        assert_eq!(cmd.catalog_id, derive_catalog_id("cat_1"));
        assert_eq!(cmd.restaurant_id, derive_restaurant_id("loc_1"));
        assert_eq!(cmd.source, "hubrise");

        // Category tree re-joins: the sub-category's parent_ref equals the parent category's ref.
        let parent = cmd.categories.iter().find(|c| c.name.0 == "Burgers").unwrap();
        let child = cmd.categories.iter().find(|c| c.name.0 == "Veggie").unwrap();
        assert_eq!(child.parent_ref, parent.r#ref);
        assert_eq!(parent.r#ref, Some(ExternalReference("c_burgers".into())));

        // Product → category join is by ref.
        let product = &cmd.products[0];
        assert_eq!(product.category_ref, Some(ExternalReference("c_burgers".into())));
        assert_eq!(product.tax_rate.delivery, TaxRatePercent(10.0));
        assert_eq!(product.tax_rate.eat_in, Some(TaxRatePercent(5.5)));
        assert_eq!(product.tax_rate.collection, None);

        // The offer is seeded from the SKU *ref*, and its price is the domain Money.
        let offer = &product.offers[0];
        assert_eq!(offer.id, derive_offer_id("SKU-CHEESE"));
        assert_eq!(offer.r#ref, Some(ExternalReference("SKU-CHEESE".into())));
        assert_eq!(offer.price, Money { amount_cents: MoneyCents(980), currency: CurrencyCode("EUR".into()) });
        assert_eq!(offer.availability, CatalogItemAvailability::AVAILABLE);
        assert_eq!(offer.stock, None);
        // SKU→option-list join: the derived id matches the imported option list's id.
        assert_eq!(offer.option_list_ids, vec![derive_option_list_id("ol_sauce")]);
        assert_eq!(cmd.option_lists[0].id, derive_option_list_id("ol_sauce"));

        // Every entity carries a ref, so the domain's MissingRef never fires.
        assert!(cmd.categories.iter().all(|c| c.r#ref.is_some()));
        assert!(cmd.products.iter().all(|p| p.r#ref.is_some() && p.offers.iter().all(|o| o.r#ref.is_some())));
        assert!(cmd.option_lists.iter().all(|l| l.r#ref.is_some() && l.options.iter().all(|o| o.r#ref.is_some())));
    }

    #[test]
    fn sku_without_ref_falls_back_to_its_id_for_the_offer_seed() {
        let json = serde_json::json!({
            "data": { "products": [{
                "id": "p1", "name": "X",
                "skus": [{ "id": "s_noref", "price": "1.00 EUR" }]
            }] }
        });
        let cmd = map_catalog(&json, "cat_1", "loc_1").unwrap();
        let offer = &cmd.products[0].offers[0];
        assert_eq!(offer.id, derive_offer_id("s_noref"));
        assert_eq!(offer.r#ref, Some(ExternalReference("s_noref".into())));
    }

    // ----- inventory mapping -----

    #[test]
    fn maps_inventory_sku_lines_and_skips_options() {
        let json = serde_json::json!([
            { "sku_ref": "SKU-CHEESE", "stock": 5, "expires_at": "2026-08-01T00:00:00Z" },
            { "sku_ref": "SKU-FRIES", "stock": "0" },
            { "option_ref": "OPT-KETCHUP", "stock": 3 }
        ]);
        let updates = map_inventory(&json, derive_catalog_id("cat_1"), derive_restaurant_id("loc_1")).unwrap();
        assert_eq!(updates.len(), 2, "the option_ref line is skipped");

        // Inventory targets the SAME OfferId the catalog import assigned (join by sku_ref).
        assert_eq!(updates[0].offer_id, derive_offer_id("SKU-CHEESE"));
        assert_eq!(updates[0].quantity, Quantity(5.0));
        assert_eq!(updates[0].expires_at.as_deref(), Some("2026-08-01T00:00:00Z"));
        assert_eq!(updates[0].low_stock_threshold, None);
        assert_eq!(updates[1].offer_id, derive_offer_id("SKU-FRIES"));
        assert_eq!(updates[1].quantity, Quantity(0.0)); // string "0" coerced.
    }

    // ----- in-memory event store (mirrors the Postgres UNIQUE(stream,version) guard) -----

    #[derive(Default)]
    struct InMemoryEventStore {
        streams: std::sync::Mutex<std::collections::HashMap<String, Vec<DomainEvent>>>,
    }

    #[async_trait::async_trait]
    impl EventStore for InMemoryEventStore {
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
            stream.extend_from_slice(events);
            Ok(stream.len() as i64)
        }

        async fn load(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError> {
            let streams = self.streams.lock().unwrap();
            let events = streams.get(stream_name).cloned().unwrap_or_default();
            let version = events.len() as i64;
            Ok((events, version))
        }
    }

    struct FakePuller {
        catalog: serde_json::Value,
        inventory: serde_json::Value,
    }

    #[async_trait::async_trait]
    impl HubRisePuller for FakePuller {
        async fn pull_catalog(&self, _id: &str) -> Result<serde_json::Value, String> {
            Ok(self.catalog.clone())
        }
        async fn pull_inventory(&self, _id: &str) -> Result<serde_json::Value, String> {
            Ok(self.inventory.clone())
        }
    }

    fn callback(resource_type: &str) -> HubRiseCallback {
        serde_json::from_value(serde_json::json!({
            "id": "cb_1", "resource_type": resource_type, "event_type": "update",
            "location_id": "loc_1", "catalog_id": "cat_1"
        }))
        .unwrap()
    }

    async fn seed_catalog(store: &InMemoryEventStore) {
        // A catalog must exist (CatalogCreated) before ImportCatalog is accepted — the connect flow's job.
        let created = DomainEvent::CatalogCreated(CatalogCreated {
            catalog_id: derive_catalog_id("cat_1"),
            r#ref: Some(ExternalReference("cat_1".into())),
            restaurant_id: derive_restaurant_id("loc_1"),
            name: CatalogName("Menu".into()),
        });
        let stream = format!("Catalog-{}", derive_catalog_id("cat_1").0);
        store
            .append(
                &stream,
                0,
                &[created],
                &Actor {
                    user_id: uuid::Uuid::nil(),
                    user_type: EXTERNAL_USER_TYPE,
                    correlation_id: uuid::Uuid::nil(),
                    cause_id: None,
                },
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn catalog_then_inventory_flows_end_to_end() {
        let store = Arc::new(InMemoryEventStore::default());
        seed_catalog(&store).await;
        let (enricher, journal) = enricher_with(
            store.clone(),
            FakePuller {
                catalog: sample_catalog_json(),
                inventory: serde_json::json!([{ "sku_ref": "SKU-CHEESE", "stock": 7 }]),
            },
        );

        // 1) Catalog callback → journaled ImportCatalog → CatalogImported appended.
        let out = enricher.enrich(&callback("catalog")).await.unwrap();
        assert_eq!(out, EnrichOutcome::CatalogImported { catalog_id: derive_catalog_id("cat_1") });

        // The send is journaled on the WORKER channel, keyed by (callback id, command type), caused
        // by the mirrored callback's identity (#15).
        let message_id = derive("command", "cb_1:ImportCatalog");
        let row = journal.by_message(message_id).await.unwrap().expect("journaled");
        assert_eq!(row.status, CommandJournalStatus::SUCCEEDED);
        assert_eq!(row.entry.channel, CommandChannel::WORKER);
        assert_eq!(row.entry.command_type, "ImportCatalog");
        assert_eq!(row.entry.cause_id, Some(derive("callback", "cb_1")));
        assert_eq!(row.entry.correlation_id, derive("callback", "cb_1"));

        // 2) Inventory callback → the imported offer's stock is updated (join by sku_ref succeeds).
        let out = enricher.enrich(&callback("inventory")).await.unwrap();
        assert_eq!(out, EnrichOutcome::InventoryApplied { applied: 1, skipped: 0 });

        // Each per-SKU send has its own journal row, discriminated by the derived offer id.
        let stock_message = derive(
            "command",
            &format!("cb_1:UpdateOfferStock:{}", derive_offer_id("SKU-CHEESE").0),
        );
        let row = journal.by_message(stock_message).await.unwrap().expect("journaled");
        assert_eq!(row.status, CommandJournalStatus::SUCCEEDED);
        assert_eq!(row.entry.command_type, "UpdateOfferStock");

        let stream = format!("Catalog-{}", derive_catalog_id("cat_1").0);
        let (events, _) = store.load(&stream).await.unwrap();
        let stock_events: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                DomainEvent::OfferStockUpdated(s) => Some(s),
                _ => None,
            })
            .collect();
        assert_eq!(stock_events.len(), 1);
        assert_eq!(stock_events[0].offer_id, derive_offer_id("SKU-CHEESE"));
        assert_eq!(stock_events[0].stock.quantity, Quantity(7.0));
    }

    #[tokio::test]
    async fn inventory_for_unknown_sku_is_skipped_not_rejected() {
        let store = Arc::new(InMemoryEventStore::default());
        seed_catalog(&store).await; // catalog exists but nothing imported → no offers.
        let (enricher, journal) = enricher_with(
            store.clone(),
            FakePuller {
                catalog: serde_json::json!({}),
                inventory: serde_json::json!([{ "sku_ref": "SKU-UNKNOWN", "stock": 3 }]),
            },
        );
        let out = enricher.enrich(&callback("inventory")).await.unwrap();
        assert_eq!(out, EnrichOutcome::InventoryApplied { applied: 0, skipped: 1 });

        // The rejected send still leaves a journal trace (REJECTED) — rejections are visible now.
        let stock_message = derive(
            "command",
            &format!("cb_1:UpdateOfferStock:{}", derive_offer_id("SKU-UNKNOWN").0),
        );
        let row = journal.by_message(stock_message).await.unwrap().expect("journaled");
        assert_eq!(row.status, CommandJournalStatus::REJECTED);
        assert_eq!(row.error.unwrap()["code"], "OfferNotFound");
    }

    #[tokio::test]
    async fn catalog_import_before_connect_is_skipped() {
        // No CatalogCreated seeded → ImportCatalog rejects CatalogNotFound → definitive skip, not 5xx.
        let store = Arc::new(InMemoryEventStore::default());
        let (enricher, journal) = enricher_with(
            store.clone(),
            FakePuller { catalog: sample_catalog_json(), inventory: serde_json::json!([]) },
        );
        let out = enricher.enrich(&callback("catalog")).await.unwrap();
        assert!(matches!(out, EnrichOutcome::Skipped { reason } if reason.contains("CatalogNotFound")));
        let row = journal
            .by_message(derive("command", "cb_1:ImportCatalog"))
            .await
            .unwrap()
            .expect("journaled");
        assert_eq!(row.status, CommandJournalStatus::REJECTED);
    }

    #[tokio::test]
    async fn unenriched_resource_type_is_ignored() {
        let store = Arc::new(InMemoryEventStore::default());
        let (enricher, _journal) = enricher_with(
            store.clone(),
            FakePuller { catalog: serde_json::json!({}), inventory: serde_json::json!([]) },
        );
        let out = enricher.enrich(&callback("order")).await.unwrap();
        assert_eq!(out, EnrichOutcome::Ignored { resource_type: "order".into() });
    }

    #[tokio::test]
    async fn redelivered_callback_dedupes_on_the_journal_no_double_apply() {
        let store = Arc::new(InMemoryEventStore::default());
        seed_catalog(&store).await;
        let (enricher, _journal) = enricher_with(
            store.clone(),
            FakePuller {
                catalog: sample_catalog_json(),
                inventory: serde_json::json!([{ "sku_ref": "SKU-CHEESE", "stock": 7 }]),
            },
        );
        enricher.enrich(&callback("catalog")).await.unwrap();
        enricher.enrich(&callback("inventory")).await.unwrap();
        let stream = format!("Catalog-{}", derive_catalog_id("cat_1").0);
        let (_, version_before) = store.load(&stream).await.unwrap();

        // HubRise redelivers BOTH callbacks (same callback id, same pulled content): the enricher
        // still acknowledges them, but the journal dedup means NOTHING new is appended.
        let out = enricher.enrich(&callback("catalog")).await.unwrap();
        assert_eq!(out, EnrichOutcome::CatalogImported { catalog_id: derive_catalog_id("cat_1") });
        let out = enricher.enrich(&callback("inventory")).await.unwrap();
        assert_eq!(out, EnrichOutcome::InventoryApplied { applied: 1, skipped: 0 });
        let (_, version_after) = store.load(&stream).await.unwrap();
        assert_eq!(version_after, version_before, "redelivery must not double-apply");
    }
}
