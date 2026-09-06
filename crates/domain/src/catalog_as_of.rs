//! `AsOfCatalog` — the read-side capability to reconstruct catalog PRICES at a past coordinate
//! (PROP-20260831-134539 §2.1 step 3, slice 2 of "the priced quote token"). Reuses the Catalog
//! aggregate's existing [`catalog::apply`]/[`catalog::fold`] — never a second one
//! (ADR-20260810-112836 §1/§3/§5/§6, the ONE-pricer property) — and truncates ITSELF at the
//! requested coordinate before folding: defence in depth against a caller whose read forgot the
//! `version <= $2` predicate, and native testability with no database.
//!
//! The result NARROWS at the boundary into private, price-only fields: unit price, each selected
//! option's price kept SEPARATE (ux — never a summed line total only), and the [`TaxRate`] OBJECT
//! that applied — the RATE LEG ONLY (ADR-20260818-121500 also names the tax amount and the receipt
//! line; those stay open, per-service-mode object — collection/eatIn nullable — never a scalar, or
//! the French 10/5.5/20 split silently collapses to one mode). [`OfferPrice`] carries NO field or
//! method from the availability vocabulary (availability, stock, orderable, existence) and no
//! `From<OfferView>` — that is HEAD's business (ux), never a stale coordinate's. See the
//! compile_fail/passing doctest pair on [`OfferPrice`].
//!
//! **The coordinate is [`CatalogVersion`]: the 1-based `domain_events.version` verbatim** — the SAME
//! number `EventStore::append`/`EventStore::load` already use (`event_store.rs:90`: the first event
//! on a stream is version 1; ADR-20260808-171056, accepted 2026-08-08 over `PROP-170000` D5). There
//! is exactly ONE spelling of "which coordinate" in this module: not a slice index, not a 0-based
//! port convention, not `up_to + 1`. `CatalogVersion::try_new` refuses anything less than 1 so a
//! caller cannot construct the coordinate "before the stream exists" by accident.
//!
//! [`AsOfCatalog::from_stream`] takes `&[(CatalogVersion, DomainEvent)]` — each event carries its
//! OWN stream version — and folds only the events whose own version is `<= up_to`. This is real
//! defence in depth at the production call site: a `$`-prefixed technical row occupies a version
//! slot but is dropped before it ever reaches this list (the adapter's decode step), so filtering by
//! each event's own version (never by its position in the slice) means a gap in the version sequence
//! can never shift which events land on which side of the coordinate.
//!
//! DARK in this slice (mob decision, no SPLIT survivor — beck/holub/vernon): no caller, no cache, no
//! checkout wiring. `pricing::price_cart` is byte-identical. The one-pricer property is protected by
//! an EQUIVALENCE test instead:
//! `crates/application/src/pricing.rs::as_of_price_at_head_equals_the_head_snapshot_price`.
//!
//! Legal notes recorded, not resolved here (never clearance): only `Product` carries `taxRate`,
//! `ProductItemOption` does not, so an alcohol option inherits the food rate — "one rate per priced
//! line" is NOT typed as an invariant. Mode selection (which of delivery/collection/eatIn applies)
//! and the null-fallback to the account's `defaultTaxRate` (a DIFFERENT stream this coordinate does
//! not pin) stay open. The delivery fee's own VAT is outside this coordinate entirely. Four counsel
//! questions on this rate leg (mode selection; the null-mode `defaultTaxRate` fallback on another
//! stream; option-level rates; a statutory rate change between coordinate and sale) are carried at
//! `docs/legal/BRIEF-20260831-repricing-and-price-quote-counsel-packet.md` §7 (d), CQ-1..CQ-4.

use std::collections::HashMap;

use crate::catalog;
use crate::generated::entities::{Money, TaxRate};
use crate::generated::events::DomainEvent;
use crate::generated::scalars::{OfferId, OptionId};

/// The as-of coordinate: `domain_events.version` verbatim (1-based — the first event appended to a
/// stream is version 1, `EventStore::append`/`load` already use this number, ADR-20260808-171056).
/// Private field, so the only way to get one is [`CatalogVersion::try_new`] — a caller cannot smuggle
/// a slice index or a 0-based port convention past the type. There is exactly one spelling of "which
/// coordinate" anywhere on this capability's boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CatalogVersion(i64);

impl CatalogVersion {
    /// `None` for anything less than 1 — `domain_events.version` starts at 1, so "version 0" or
    /// negative is not a coordinate this stream can ever have produced.
    pub fn try_new(version: i64) -> Option<Self> {
        if version >= 1 {
            Some(Self(version))
        } else {
            None
        }
    }

    /// The raw `domain_events.version` value.
    pub fn get(&self) -> i64 {
        self.0
    }
}

/// One offer's UNIT price and its selected options' prices at a coordinate — a VALUE, not a live
/// catalog reference (evans: not `PricedOffer`, `Offer` is a catalog entity, this is a price at a
/// coordinate; never `line` — a line belongs to a cart, not to this capability). No labels: a
/// renamed product must not render two names, so names are HEAD's business (ux), never this
/// capability's.
///
/// ```
/// # use domain::catalog_as_of::{AsOfCatalog, CatalogVersion, OfferPrice};
/// # use domain::generated::scalars::OfferId;
/// let events: &[(CatalogVersion, domain::generated::events::DomainEvent)] = &[];
/// let as_of = AsOfCatalog::from_stream(events, CatalogVersion::try_new(1).unwrap());
/// // The offer does not exist in an empty stream: no availability accessor exists to check
/// // instead, because the type carries none.
/// let _price = as_of.price_of(OfferId(uuid::Uuid::nil()), &[]).map(|p: OfferPrice| p.unit_price);
/// ```
///
/// `OfferPrice` has no field or method from the availability vocabulary — none of the four must
/// compile. Each is IDENTICAL to the passing twin above except its last line, so a rename of
/// `OfferPrice` itself breaks the TWIN too (an ordinary doctest failure, caught by `cargo test --doc`
/// like any other regression) rather than letting one of these "pass" (fail to compile) for the
/// wrong reason:
///
/// ```compile_fail
/// # use domain::catalog_as_of::{AsOfCatalog, CatalogVersion, OfferPrice};
/// # use domain::generated::scalars::OfferId;
/// # let events: &[(CatalogVersion, domain::generated::events::DomainEvent)] = &[];
/// # let as_of = AsOfCatalog::from_stream(events, CatalogVersion::try_new(1).unwrap());
/// let _price = as_of.price_of(OfferId(uuid::Uuid::nil()), &[]).map(|p: OfferPrice| p.availability);
/// ```
///
/// No `stock` field either — the boundary is field-shaped on purpose (reviewer NB7): each excluded
/// TERM gets its own guard rather than trusting one field to stand for the whole vocabulary.
///
/// ```compile_fail
/// # use domain::catalog_as_of::{AsOfCatalog, CatalogVersion, OfferPrice};
/// # use domain::generated::scalars::OfferId;
/// # let events: &[(CatalogVersion, domain::generated::events::DomainEvent)] = &[];
/// # let as_of = AsOfCatalog::from_stream(events, CatalogVersion::try_new(1).unwrap());
/// let _price = as_of.price_of(OfferId(uuid::Uuid::nil()), &[]).map(|p: OfferPrice| p.stock);
/// ```
///
/// No `orderable` field.
///
/// ```compile_fail
/// # use domain::catalog_as_of::{AsOfCatalog, CatalogVersion, OfferPrice};
/// # use domain::generated::scalars::OfferId;
/// # let events: &[(CatalogVersion, domain::generated::events::DomainEvent)] = &[];
/// # let as_of = AsOfCatalog::from_stream(events, CatalogVersion::try_new(1).unwrap());
/// let _price = as_of.price_of(OfferId(uuid::Uuid::nil()), &[]).map(|p: OfferPrice| p.orderable);
/// ```
///
/// No `availability()` METHOD either — not only the field.
///
/// ```compile_fail
/// # use domain::catalog_as_of::{AsOfCatalog, CatalogVersion, OfferPrice};
/// # use domain::generated::scalars::OfferId;
/// # let events: &[(CatalogVersion, domain::generated::events::DomainEvent)] = &[];
/// # let as_of = AsOfCatalog::from_stream(events, CatalogVersion::try_new(1).unwrap());
/// let _price = as_of.price_of(OfferId(uuid::Uuid::nil()), &[]).map(|p: OfferPrice| p.availability());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct OfferPrice {
    /// The offer's own unit price at the coordinate.
    pub unit_price: Money,
    /// Each selected option's price, SEPARATE from the unit price and from each other (ux: per-line
    /// per-unit granularity, option-level attribution, a zero-delta computable per line).
    pub option_prices: Vec<(OptionId, Money)>,
    /// The `TaxRate` OBJECT that applied at the coordinate, per service mode (never a scalar) — the
    /// rate leg only (ADR-20260818-121500 also names the tax amount and the receipt line; those
    /// remain open, see the module doc).
    pub tax_rate: TaxRate,
}

/// One offer's resolved price at the coordinate — the private, narrowed shape [`AsOfCatalog`] keeps
/// internally (never exposed; [`AsOfCatalog::price_of`] is the only accessor).
#[derive(Debug)]
struct ResolvedOffer {
    unit_price: Money,
    tax_rate: TaxRate,
    /// Only the options actually reachable from THIS offer's linked option lists — an option id from
    /// a different offer's list is not resolvable through it (mirrors `pricing::price_cart`'s
    /// per-offer option resolution).
    option_prices: HashMap<OptionId, Money>,
}

/// The catalog's priced state at a fixed coordinate (`up_to`, inclusive) — a RESOLVED value, no repo
/// handle inside (vernon Q1). Built once via [`AsOfCatalog::from_stream`], read many times via
/// [`AsOfCatalog::price_of`].
///
/// **Carries its own coordinate** (PROP-20260831-134539 slice 3a, D2): the CEILING the fold was
/// bounded at (`up_to`, verbatim — never "the highest applied business version", which can be
/// LOWER than `up_to` whenever no business event occupies the exact requested slot, e.g. a
/// technical row or a gap). There is no constructor that omits it — [`AsOfCatalog::narrow`] is the
/// only place the struct is built, and it always takes one — so a caller can never end up holding a
/// priced value with no coordinate, and there is deliberately no `Option<CatalogVersion>` anywhere
/// on this type: "which prefix priced this" is answered exactly once, at construction, always.
#[derive(Debug)]
pub struct AsOfCatalog {
    coordinate: CatalogVersion,
    offers: HashMap<OfferId, ResolvedOffer>,
}

impl AsOfCatalog {
    /// Fold `events` (each carrying its OWN stream version) up to and including `up_to`, with the
    /// SAME [`catalog::fold`] the write path uses — never a second, price-only apply
    /// (ADR-20260810-112836). Filtering by each event's own version (never by its position in the
    /// slice) means a `$`-prefixed technical row that occupies a version slot — already dropped
    /// before this list is built — can never shift which business events land on which side of the
    /// coordinate: this is real defence in depth at the production call site, not merely documented
    /// intent, because there is no index arithmetic anywhere in this function to get wrong.
    pub fn from_stream(events: &[(CatalogVersion, DomainEvent)], up_to: CatalogVersion) -> AsOfCatalog {
        let truncated: Vec<DomainEvent> =
            events.iter().filter(|(version, _)| *version <= up_to).map(|(_, event)| event.clone()).collect();
        Self::narrow(catalog::fold(&truncated), up_to)
    }

    /// The coordinate this value was bounded at — the CEILING [`AsOfCatalog::from_stream`] was
    /// called with, never "the highest version that actually had a business event at or below it".
    /// A stream whose last applied business event is version 2, folded with `up_to = 3` (version 3
    /// a technical row, or simply absent), still carries coordinate 3: the fold answers "what was
    /// priced at V=3", and 3 is the V, whatever did or did not land on that exact slot.
    pub fn coordinate(&self) -> CatalogVersion {
        self.coordinate
    }

    /// Narrow a fully-folded [`catalog::CatalogState`] into the price-only shape this capability
    /// exposes — the boundary where availability/stock/existence are dropped for good. `coordinate`
    /// is threaded straight through: this is the ONLY constructor of [`AsOfCatalog`], so there is no
    /// path to a value that does not carry one.
    fn narrow(state: Option<catalog::CatalogState>, coordinate: CatalogVersion) -> AsOfCatalog {
        let mut offers = HashMap::new();
        if let Some(state) = state {
            for product in &state.products {
                for offer in &product.offers {
                    let mut option_prices = HashMap::new();
                    for list_id in &offer.option_list_ids {
                        if let Some(list) = state.option_lists.iter().find(|l| l.id == *list_id) {
                            for option in &list.options {
                                option_prices.insert(option.id, option.price.clone());
                            }
                        }
                    }
                    offers.insert(
                        offer.id,
                        ResolvedOffer {
                            unit_price: offer.price.clone(),
                            tax_rate: product.tax_rate.clone(),
                            option_prices,
                        },
                    );
                }
            }
        }
        AsOfCatalog { coordinate, offers }
    }

    /// This offer's price at the coordinate, with `options` priced separately. `None` means no price
    /// exists AT THIS COORDINATE — never a statement about the offer's existence TODAY (that re-check
    /// is HEAD's, always, whatever this fold answers) — for either of two reasons: the offer had not
    /// been added yet, or a requested option does not resolve through it (fail-closed: same posture
    /// as `pricing::price_cart`'s `PriceUnresolvable`, no client-supplied number is ever guessed).
    /// Returns no availability/stock/existence signal — that is HEAD's.
    pub fn price_of(&self, offer: OfferId, options: &[OptionId]) -> Option<OfferPrice> {
        let resolved = self.offers.get(&offer)?;
        let mut option_prices = Vec::with_capacity(options.len());
        for option_id in options {
            let price = resolved.option_prices.get(option_id)?.clone();
            option_prices.push((*option_id, price));
        }
        Some(OfferPrice {
            unit_price: resolved.unit_price.clone(),
            option_prices,
            tax_rate: resolved.tax_rate.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::entities::{Offer, OptionList, Product, ProductItemOption, Stock};
    use crate::generated::events::{
        CatalogCreated, CatalogImported, OfferStockUpdated, OptionListAdded, OptionListUpdated,
        ProductAdded,
    };
    use crate::generated::scalars::{
        CatalogId, CatalogItemAvailability, CatalogName, CurrencyCode, MoneyCents, OfferName,
        OptionListId, OptionListName, OptionName, ProductId, ProductName, Quantity, RestaurantId,
        StockStatus, TaxRatePercent,
    };

    fn uid(n: u128) -> uuid::Uuid {
        uuid::Uuid::from_u128(n)
    }
    fn eur(cents: i64) -> Money {
        Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
    }
    fn tax(delivery: f64) -> TaxRate {
        TaxRate { delivery: TaxRatePercent(delivery), collection: None, eat_in: None }
    }
    fn v(n: i64) -> CatalogVersion {
        CatalogVersion::try_new(n).unwrap()
    }

    /// Assign sequential stream versions starting at 1 (mirrors `domain_events.version`) to a plain
    /// `Vec<DomainEvent>` fixture — the shape [`AsOfCatalog::from_stream`] actually takes.
    fn versioned(events: Vec<DomainEvent>) -> Vec<(CatalogVersion, DomainEvent)> {
        events.into_iter().enumerate().map(|(i, e)| (v(i as i64 + 1), e)).collect()
    }

    fn fx_catalog_created() -> DomainEvent {
        DomainEvent::CatalogCreated(CatalogCreated {
            catalog_id: CatalogId(uid(1)),
            r#ref: None,
            restaurant_id: RestaurantId(uid(2)),
            name: CatalogName("Main".into()),
        })
    }

    /// One product with one offer, no options — `price_cents` on the single offer, `off-1`.
    fn fx_product_added(price_cents: i64) -> DomainEvent {
        DomainEvent::ProductAdded(ProductAdded {
            catalog_id: CatalogId(uid(1)),
            restaurant_id: RestaurantId(uid(2)),
            product: Product {
                id: ProductId(uid(10)),
                r#ref: None,
                catalog_id: CatalogId(uid(1)),
                restaurant_id: RestaurantId(uid(2)),
                category_ref: None,
                name: ProductName("Margherita".into()),
                description: None,
                tags: Vec::new(),
                image_ids: Vec::new(),
                tax_rate: tax(10.0),
                offers: vec![Offer {
                    id: OfferId(uid(20)),
                    r#ref: None,
                    product_id: ProductId(uid(10)),
                    name: OfferName("Default".into()),
                    price: eur(price_cents),
                    availability: CatalogItemAvailability::AVAILABLE,
                    stock: None,
                    option_list_ids: Vec::new(),
                }],
            },
        })
    }

    /// A second product ("prod-2", offer "off-2") — used to assert absence-before-creation.
    fn fx_second_product_added(price_cents: i64) -> DomainEvent {
        DomainEvent::ProductAdded(ProductAdded {
            catalog_id: CatalogId(uid(1)),
            restaurant_id: RestaurantId(uid(2)),
            product: Product {
                id: ProductId(uid(11)),
                r#ref: None,
                catalog_id: CatalogId(uid(1)),
                restaurant_id: RestaurantId(uid(2)),
                category_ref: None,
                name: ProductName("Calzone".into()),
                description: None,
                tags: Vec::new(),
                image_ids: Vec::new(),
                tax_rate: tax(10.0),
                offers: vec![Offer {
                    id: OfferId(uid(21)),
                    r#ref: None,
                    product_id: ProductId(uid(11)),
                    name: OfferName("Default".into()),
                    price: eur(price_cents),
                    availability: CatalogItemAvailability::AVAILABLE,
                    stock: None,
                    option_list_ids: Vec::new(),
                }],
            },
        })
    }

    /// The product from [`fx_product_added`], with its offer now linked to option list `ol-1`.
    fn fx_product_added_with_option_list(price_cents: i64) -> DomainEvent {
        let DomainEvent::ProductAdded(mut e) = fx_product_added(price_cents) else { unreachable!() };
        e.product.offers[0].option_list_ids = vec![OptionListId(uid(30))];
        DomainEvent::ProductAdded(e)
    }

    fn fx_option_list_added(option_price_cents: i64) -> DomainEvent {
        DomainEvent::OptionListAdded(OptionListAdded {
            catalog_id: CatalogId(uid(1)),
            restaurant_id: RestaurantId(uid(2)),
            option_list: OptionList {
                id: OptionListId(uid(30)),
                r#ref: None,
                name: OptionListName("Size".into()),
                min_selections: 0,
                max_selections: Some(1),
                multiple_selection: false,
                options: vec![ProductItemOption {
                    id: OptionId(uid(40)),
                    r#ref: None,
                    option_list_id: OptionListId(uid(30)),
                    name: OptionName("Large".into()),
                    price: eur(option_price_cents),
                    r#default: false,
                    availability: CatalogItemAvailability::AVAILABLE,
                    stock: None,
                }],
            },
        })
    }

    fn fx_option_list_updated(option_price_cents: i64) -> DomainEvent {
        let DomainEvent::OptionListAdded(added) = fx_option_list_added(option_price_cents) else {
            unreachable!()
        };
        DomainEvent::OptionListUpdated(OptionListUpdated {
            catalog_id: added.catalog_id,
            restaurant_id: added.restaurant_id,
            option_list: added.option_list,
        })
    }

    fn fx_offer_stock_updated(quantity: f64) -> DomainEvent {
        DomainEvent::OfferStockUpdated(OfferStockUpdated {
            catalog_id: CatalogId(uid(1)),
            restaurant_id: RestaurantId(uid(2)),
            offer_id: OfferId(uid(20)),
            stock: Stock {
                quantity: Quantity(quantity),
                low_stock_threshold: None,
                status: StockStatus::IN_STOCK,
                expires_at: None,
            },
        })
    }

    fn fx_catalog_imported(price_cents: i64) -> DomainEvent {
        DomainEvent::CatalogImported(CatalogImported {
            catalog_id: CatalogId(uid(1)),
            restaurant_id: RestaurantId(uid(2)),
            source: "HUBRISE".to_string(),
            categories: vec![],
            products: vec![Product {
                id: ProductId(uid(10)),
                r#ref: None,
                catalog_id: CatalogId(uid(1)),
                restaurant_id: RestaurantId(uid(2)),
                category_ref: None,
                name: ProductName("Margherita (imported)".into()),
                description: None,
                tags: Vec::new(),
                image_ids: Vec::new(),
                tax_rate: tax(10.0),
                offers: vec![Offer {
                    id: OfferId(uid(20)),
                    r#ref: None,
                    product_id: ProductId(uid(10)),
                    name: OfferName("Default".into()),
                    price: eur(price_cents),
                    availability: CatalogItemAvailability::AVAILABLE,
                    stock: None,
                    option_list_ids: Vec::new(),
                }],
            }],
            option_lists: vec![],
        })
    }

    /// A product with a distinct id/offer id, for the native benchmark's realistic mix (D2).
    fn fx_product_added_n(n: u64, price_cents: i64) -> DomainEvent {
        DomainEvent::ProductAdded(ProductAdded {
            catalog_id: CatalogId(uid(1)),
            restaurant_id: RestaurantId(uid(2)),
            product: Product {
                id: ProductId(uid(1_000 + n as u128)),
                r#ref: None,
                catalog_id: CatalogId(uid(1)),
                restaurant_id: RestaurantId(uid(2)),
                category_ref: None,
                name: ProductName(format!("Product {n}")),
                description: None,
                tags: Vec::new(),
                image_ids: Vec::new(),
                tax_rate: tax(10.0),
                offers: vec![Offer {
                    id: OfferId(uid(2_000 + n as u128)),
                    r#ref: None,
                    product_id: ProductId(uid(1_000 + n as u128)),
                    name: OfferName("Default".into()),
                    price: eur(price_cents),
                    availability: CatalogItemAvailability::AVAILABLE,
                    stock: None,
                    option_list_ids: Vec::new(),
                }],
            },
        })
    }

    /// A `CatalogImported` carrying `products` distinct products — the OTHER expensive replace arm
    /// (a full HubRise resync), for the native benchmark's realistic mix (D2, beck NB4).
    fn fx_catalog_imported_n(products: usize) -> DomainEvent {
        DomainEvent::CatalogImported(CatalogImported {
            catalog_id: CatalogId(uid(1)),
            restaurant_id: RestaurantId(uid(2)),
            source: "HUBRISE".to_string(),
            categories: vec![],
            products: (0..products)
                .map(|n| {
                    let DomainEvent::ProductAdded(e) = fx_product_added_n(n as u64, 1_000 + n as i64)
                    else {
                        unreachable!()
                    };
                    e.product
                })
                .collect(),
            option_lists: vec![],
        })
    }

    /// PROP-20260831-134539:547 — folding to V reproduces the price AT V, not at HEAD. Mutant:
    /// `from_stream` ignores `up_to` and folds the whole slice.
    #[test]
    fn folding_to_v_reproduces_the_price_at_v() {
        let events = versioned(vec![fx_catalog_created(), fx_product_added(1500), fx_product_added(1900)]);
        let as_of = AsOfCatalog::from_stream(&events, v(2)); // created + first ProductAdded(1500)
        let price = as_of.price_of(OfferId(uid(20)), &[]).expect("offer exists at V");
        assert_eq!(price.unit_price, eur(1500), "price {} got {}", 1500, price.unit_price.amount_cents.0);
    }

    /// PROP-20260831-134539:547 (red-first, round 2) — an event past the coordinate that IS present
    /// in the slice must not be applied: this is what makes the fold real, not merely truncated at
    /// the SQL boundary. Mutant: `from_stream` ignores each event's own version and folds the whole
    /// slice regardless of `up_to`.
    #[test]
    fn an_event_past_the_coordinate_in_the_slice_is_not_applied() {
        let events = versioned(vec![fx_catalog_created(), fx_product_added(1500), fx_product_added(1900)]);
        let as_of = AsOfCatalog::from_stream(&events, v(2)); // up to the 1500 ProductAdded only
        let price = as_of.price_of(OfferId(uid(20)), &[]).expect("offer exists at V");
        assert_eq!(price.unit_price, eur(1500), "price {} got {}", 1500, price.unit_price.amount_cents.0);
    }

    /// PROP-20260831-134539:547 (red-first, round 2) — a technical row (`$`-prefixed, already
    /// dropped by the decoder before this list is built) that occupies a version slot must not shift
    /// the coordinate: filtering by each event's OWN version must still land on the price the
    /// coordinate names, even though the row's position in the slice no longer lines up with its
    /// stream version. Mutant: truncation by slice index instead of the event's own version.
    #[test]
    fn a_technical_row_in_range_does_not_shift_the_coordinate() {
        let events = vec![
            (v(1), fx_catalog_created()),
            // version 2 would be a technical row -- already dropped by the decoder, absent here.
            (v(3), fx_product_added(1500)),
            (v(4), fx_product_added(1900)),
        ];
        let as_of = AsOfCatalog::from_stream(&events, v(3));
        let price = as_of.price_of(OfferId(uid(20)), &[]).expect("offer exists at V=3");
        assert_eq!(
            price.unit_price.amount_cents.0, 1500,
            "price at V differs after a technical row in range: got {}",
            price.unit_price.amount_cents.0
        );
    }

    /// PROP-20260831-134539:547 — `OfferStockUpdated` never moves a price. Mutant: the arm rewrites
    /// the offer price.
    #[test]
    fn offer_stock_updated_never_moves_a_price() {
        let events = versioned(vec![
            fx_catalog_created(),
            fx_product_added(1500),
            fx_offer_stock_updated(5.0),
            fx_offer_stock_updated(2.0),
        ]);
        let at_v = AsOfCatalog::from_stream(&events, v(2)); // before either stock update
        let at_v_plus_2 = AsOfCatalog::from_stream(&events, v(4)); // after both
        let price_v = at_v.price_of(OfferId(uid(20)), &[]).unwrap().unit_price;
        let price_v_plus_2 = at_v_plus_2.price_of(OfferId(uid(20)), &[]).unwrap().unit_price;
        assert_eq!(price_v, price_v_plus_2, "price at V+2 differs from price at V");
    }

    /// PROP-20260831-134539:547 — an offer added AFTER V is absent AT V. Mutant: truncation off by
    /// one.
    #[test]
    fn an_offer_added_after_v_is_absent_at_v() {
        let events = versioned(vec![fx_catalog_created(), fx_product_added(1500)]);
        let as_of = AsOfCatalog::from_stream(&events, v(1)); // only CatalogCreated
        assert_eq!(as_of.price_of(OfferId(uid(20)), &[]), None, "price_of returned Some, expected None");
    }

    /// PROP-20260831-134539:547 — `CatalogImported` at/before V replaces content WHOLESALE. Mutant:
    /// `CatalogImported` falls into the catch-all arm (a no-op).
    #[test]
    fn catalog_imported_before_v_replaces_content_wholesale() {
        let events =
            versioned(vec![fx_catalog_created(), fx_product_added(1500), fx_catalog_imported(2500)]);
        let as_of = AsOfCatalog::from_stream(&events, v(3)); // at/after the import
        let price = as_of.price_of(OfferId(uid(20)), &[]).expect("offer exists post-import");
        assert_eq!(price.unit_price, eur(2500), "the post-import price must be returned at V");
    }

    /// PROP-20260831-134539:547 — an `OptionListUpdated` after V prices AT V (the old option price).
    /// Mutant: option price read from the final state.
    #[test]
    fn an_option_list_updated_after_v_prices_at_v() {
        let events = versioned(vec![
            fx_catalog_created(),
            fx_product_added_with_option_list(1500),
            fx_option_list_added(200),
            fx_option_list_updated(300),
        ]);
        let as_of = AsOfCatalog::from_stream(&events, v(3)); // before the update to 300
        let price = as_of.price_of(OfferId(uid(20)), &[OptionId(uid(40))]).expect("resolves at V");
        assert_eq!(
            price.option_prices,
            vec![(OptionId(uid(40)), eur(200))],
            "option price 200 got {:?}",
            price.option_prices
        );
    }

    /// PROP-20260831-134539:140 — the `TaxRate` OBJECT is pinned at V, all modes (never collapsed to
    /// one scalar). Mutant: the fold returns the delivery rate only.
    #[test]
    fn tax_rate_object_is_pinned_at_v_all_modes() {
        let full_rate = TaxRate {
            delivery: TaxRatePercent(20.0),
            collection: Some(TaxRatePercent(10.0)),
            eat_in: Some(TaxRatePercent(5.5)),
        };
        let DomainEvent::ProductAdded(mut e) = fx_product_added(1500) else { unreachable!() };
        e.product.tax_rate = full_rate.clone();
        let events = versioned(vec![fx_catalog_created(), DomainEvent::ProductAdded(e)]);
        let as_of = AsOfCatalog::from_stream(&events, v(2));
        let price = as_of.price_of(OfferId(uid(20)), &[]).unwrap();
        assert_eq!(price.tax_rate, full_rate, "collection rate lost");
    }

    /// ux — per-line UNIT and OPTION prices stay separate, never a summed line total only.
    #[test]
    fn price_of_answers_unit_and_option_prices_separately() {
        let events = versioned(vec![
            fx_catalog_created(),
            fx_product_added_with_option_list(1500),
            fx_option_list_added(200),
        ]);
        let as_of = AsOfCatalog::from_stream(&events, v(3));
        let price = as_of.price_of(OfferId(uid(20)), &[OptionId(uid(40))]).unwrap();
        assert_eq!(price.unit_price, eur(1500));
        assert_eq!(price.option_prices, vec![(OptionId(uid(40)), eur(200))]);
    }

    /// PROP-20260831-134539:547 (slice 3a, D2) — the coordinate carried is the CEILING the fold was
    /// bounded at, never "the highest applied business version". Mutant: `from_stream` stores the
    /// max applied business version instead of `up_to`.
    #[test]
    fn the_coordinate_carried_is_the_ceiling_the_fold_was_bounded_at() {
        let events = vec![
            (v(1), fx_catalog_created()),
            (v(2), fx_product_added(1500)),
            // Version 3 is a technical row -- already dropped by the decoder, absent here. The
            // last APPLIED business event is version 2; the requested ceiling is 3.
        ];
        let as_of = AsOfCatalog::from_stream(&events, v(3));
        assert_eq!(
            as_of.coordinate(),
            v(3),
            "coordinate must be the ceiling the fold was bounded at (3), not the highest applied \
             business version (2)"
        );
    }

    /// An id that only ever appears in a DIFFERENT offer's stream is never confused with `off-1`'s
    /// state (basic isolation sanity for the fixtures above).
    #[test]
    fn two_offers_price_independently() {
        let events =
            versioned(vec![fx_catalog_created(), fx_product_added(1500), fx_second_product_added(900)]);
        let as_of = AsOfCatalog::from_stream(&events, v(3));
        assert_eq!(as_of.price_of(OfferId(uid(20)), &[]).unwrap().unit_price, eur(1500));
        assert_eq!(as_of.price_of(OfferId(uid(21)), &[]).unwrap().unit_price, eur(900));
    }

    /// THE BENCHMARK (a) — a native, ALWAYS-RUN ceiling test, not an SLO: a magnitude-regression
    /// detector only. L = 2,000 events is `UNVERIFIED input` (no measured Tours catalog stream
    /// length exists; derived from "the largest realistic HubRise import (~500 products x ~3 offers)
    /// plus a Friday's worth of stock syncs", itself a judgement call, not a measurement). The mix
    /// (D2, beck NB4) carries the EXPENSIVE replace arms `ProductAdded` (500 distinct products) and
    /// ONE `CatalogImported` (a full HubRise resync of another 500), not only cheap
    /// `OfferStockUpdated` -- a synthetic mix of 2,000 identical stock rows priced a cost profile the
    /// write path never actually produces.
    #[test]
    fn fold_to_v_stays_under_ceiling_natively() {
        const L: usize = 2_000;
        const PRODUCTS: usize = 500;
        let mut events = vec![fx_catalog_created()];
        for n in 0..PRODUCTS {
            events.push(fx_product_added_n(n as u64, 1_000 + n as i64));
        }
        events.push(fx_catalog_imported_n(PRODUCTS)); // one wholesale replace (a HubRise resync)
        while events.len() < L {
            let i = events.len();
            events.push(fx_offer_stock_updated(i as f64));
        }
        let events = versioned(events);
        let head = v(events.len() as i64);

        const ITERATIONS: u32 = 20;
        let mut samples = Vec::with_capacity(ITERATIONS as usize);
        for _ in 0..ITERATIONS {
            let start = std::time::Instant::now();
            let as_of = AsOfCatalog::from_stream(&events, head);
            std::hint::black_box(&as_of);
            samples.push(start.elapsed());
        }
        samples.sort();
        let median = samples[samples.len() / 2];
        // Restated honestly (D2, holub NB2): the OLD "~10x the median" comment was never true even
        // for the cheap-only mix it described (that mix measured ~50-150us against a 15ms ceiling --
        // ~100-300x). The heavier, D2-mandated mix here (500 distinct products via `upsert_product`'s
        // O(n) retain-then-push, plus one 500-product `CatalogImported` wholesale replace) measures a
        // ~15ms median at authoring on this container (debug build) -- the CONSTANT below is set to
        // the STATED multiple this time, ~10x that measurement, rather than left stale again. Not an
        // SLO -- a magnitude-regression detector: if this test goes red, something got an order of
        // magnitude slower, not "a millisecond over".
        const CEILING: std::time::Duration = std::time::Duration::from_millis(150);
        assert!(
            median < CEILING,
            "fold of {L} events took {median:?} (median of {ITERATIONS}), ceiling {CEILING:?} -- \
             not an SLO, a magnitude-regression detector"
        );
    }
}
