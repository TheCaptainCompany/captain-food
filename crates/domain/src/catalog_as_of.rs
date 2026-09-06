//! `AsOfCatalog` — the read-side capability to reconstruct catalog PRICES at a past coordinate
//! (PROP-20260831-134539 §2.1 step 3, slice 2 of "the priced quote token"). Reuses the Catalog
//! aggregate's existing [`catalog::apply`]/[`catalog::fold`] — never a second one
//! (ADR-20260810-112836 §1/§3/§5/§6, the ONE-pricer property) — and truncates ITSELF at the
//! requested coordinate before folding: defence in depth against a caller whose read forgot the
//! `version <= $2` predicate, and native testability with no database.
//!
//! The result NARROWS at the boundary into private, price-only fields: unit price, each selected
//! option's price kept SEPARATE (ux — never a summed line total only), and the [`TaxRate`] OBJECT
//! that applied (ADR-20260818-121500: per line, the rate that applied AND the tax amount, frozen at
//! the moment of sale; `TaxRate` is a per-service-mode object — collection/eatIn nullable — never a
//! scalar, or the French 10/5.5/20 split silently collapses to one mode). [`OfferPrice`] carries NO
//! field or method from the availability vocabulary (availability, stock, orderable, existence) and
//! no `From<OfferView>` — that is HEAD's business (ux), never a stale coordinate's. See the
//! compile_fail/passing doctest pair on [`OfferPrice`].
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
//! not pin) stay open. The delivery fee's own VAT is outside this coordinate entirely.

use std::collections::HashMap;

use crate::catalog;
use crate::generated::entities::{Money, TaxRate};
use crate::generated::events::DomainEvent;
use crate::generated::scalars::{OfferId, OptionId};

/// One line's price at the fold's coordinate — a VALUE, not a live catalog reference (evans: not
/// `PricedOffer`, `Offer` is a catalog entity, this is a line price at a coordinate). No labels: a
/// renamed product must not render two names, so names are HEAD's business (ux), never this
/// capability's.
///
/// ```
/// # use domain::catalog_as_of::AsOfCatalog;
/// # use domain::generated::scalars::OfferId;
/// let events: &[domain::generated::events::DomainEvent] = &[];
/// let as_of = AsOfCatalog::from_stream(events, 0);
/// // The offer does not exist in an empty stream: no availability accessor exists to check
/// // instead, because the type carries none.
/// let _price = as_of.price_of(OfferId(uuid::Uuid::nil()), &[]);
/// ```
///
/// `OfferPrice` has no field or method from the availability vocabulary — this must NOT compile:
///
/// ```compile_fail
/// # use domain::catalog_as_of::{AsOfCatalog, OfferPrice};
/// # use domain::generated::scalars::OfferId;
/// # let events: &[domain::generated::events::DomainEvent] = &[];
/// # let as_of = AsOfCatalog::from_stream(events, 0);
/// let price: OfferPrice = as_of.price_of(OfferId(uuid::Uuid::nil()), &[]).unwrap();
/// let _ = price.availability;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct OfferPrice {
    /// The offer's own unit price at the coordinate.
    pub unit_price: Money,
    /// Each selected option's price, SEPARATE from the unit price and from each other (ux: per-line
    /// per-unit granularity, option-level attribution, a zero-delta computable per line).
    pub option_prices: Vec<(OptionId, Money)>,
    /// The `TaxRate` OBJECT that applied at the coordinate, per service mode (never a scalar).
    pub tax_rate: TaxRate,
}

/// One offer's resolved price at the coordinate — the private, narrowed shape [`AsOfCatalog`] keeps
/// internally (never exposed; [`AsOfCatalog::price_of`] is the only accessor).
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
pub struct AsOfCatalog {
    offers: HashMap<OfferId, ResolvedOffer>,
}

impl AsOfCatalog {
    /// Fold `events` (in stream/version order) up to and including index `up_to` (0-based: `up_to`
    /// itself means `up_to + 1` events are applied) with the SAME [`catalog::fold`] the write path
    /// uses — never a second, price-only apply (ADR-20260810-112836). Truncating HERE, not only at
    /// the SQL boundary, is defence in depth: a caller whose read forgot `version <= $2` still gets
    /// only the events up to the requested coordinate folded, never the live head.
    ///
    /// `up_to < 0` folds nothing (the catalog does not exist yet at that coordinate); `up_to` past
    /// the end of `events` is clamped to the whole slice (the coordinate IS head).
    pub fn from_stream(events: &[DomainEvent], up_to: i64) -> AsOfCatalog {
        let len = if up_to < 0 { 0 } else { (up_to + 1).min(events.len() as i64) as usize };
        let truncated = &events[..len];
        Self::narrow(catalog::fold(truncated))
    }

    /// Narrow a fully-folded [`catalog::CatalogState`] into the price-only shape this capability
    /// exposes — the boundary where availability/stock/existence are dropped for good.
    fn narrow(state: Option<catalog::CatalogState>) -> AsOfCatalog {
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
        AsOfCatalog { offers }
    }

    /// This offer's price at the coordinate, with `options` priced separately — `None` if the offer
    /// does not exist at the coordinate, or any requested option does not resolve through it
    /// (fail-closed: same posture as `pricing::price_cart`'s `PriceUnresolvable`, no client-supplied
    /// number is ever guessed). Returns no availability/stock/existence signal — that is HEAD's.
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

    /// PROP-20260831-134539:547 — folding to V reproduces the price AT V, not at HEAD. Mutant: `from_stream`
    /// ignores `up_to` and folds the whole slice.
    #[test]
    fn folding_to_v_reproduces_the_price_at_v() {
        let events = vec![fx_catalog_created(), fx_product_added(1500), fx_product_added(1900)];
        let as_of = AsOfCatalog::from_stream(&events, 1); // created + first ProductAdded(1500)
        let price = as_of.price_of(OfferId(uid(20)), &[]).expect("offer exists at V");
        assert_eq!(price.unit_price, eur(1500), "price {} got {}", 1500, price.unit_price.amount_cents.0);
    }

    /// PROP-20260831-134539:547 — `OfferStockUpdated` never moves a price. Mutant: the arm rewrites
    /// the offer price.
    #[test]
    fn offer_stock_updated_never_moves_a_price() {
        let events = vec![
            fx_catalog_created(),
            fx_product_added(1500),
            fx_offer_stock_updated(5.0),
            fx_offer_stock_updated(2.0),
        ];
        let at_v = AsOfCatalog::from_stream(&events, 1); // before either stock update
        let at_v_plus_2 = AsOfCatalog::from_stream(&events, 3); // after both
        let price_v = at_v.price_of(OfferId(uid(20)), &[]).unwrap().unit_price;
        let price_v_plus_2 = at_v_plus_2.price_of(OfferId(uid(20)), &[]).unwrap().unit_price;
        assert_eq!(price_v, price_v_plus_2, "price at V+2 differs from price at V");
    }

    /// PROP-20260831-134539:547 — an offer added AFTER V is absent AT V. Mutant: truncation off by
    /// one.
    #[test]
    fn an_offer_added_after_v_is_absent_at_v() {
        let events = vec![fx_catalog_created(), fx_product_added(1500)];
        let as_of = AsOfCatalog::from_stream(&events, 0); // only CatalogCreated
        assert_eq!(as_of.price_of(OfferId(uid(20)), &[]), None, "price_of returned Some, expected None");
    }

    /// PROP-20260831-134539:547 — `CatalogImported` before V replaces content WHOLESALE. Mutant:
    /// `CatalogImported` falls into the catch-all arm (a no-op).
    #[test]
    fn catalog_imported_before_v_replaces_content_wholesale() {
        let events = vec![fx_catalog_created(), fx_product_added(1500), fx_catalog_imported(2500)];
        let as_of = AsOfCatalog::from_stream(&events, 2); // at/after the import
        let price = as_of.price_of(OfferId(uid(20)), &[]).expect("offer exists post-import");
        assert_eq!(price.unit_price, eur(2500), "the pre-import price is returned at V");
    }

    /// PROP-20260831-134539:547 — an `OptionListUpdated` after V prices AT V (the old option price).
    /// Mutant: option price read from the final state.
    #[test]
    fn an_option_list_updated_after_v_prices_at_v() {
        let events = vec![
            fx_catalog_created(),
            fx_product_added_with_option_list(1500),
            fx_option_list_added(200),
            fx_option_list_updated(300),
        ];
        let as_of = AsOfCatalog::from_stream(&events, 2); // before the update to 300
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
        let events = vec![fx_catalog_created(), DomainEvent::ProductAdded(e)];
        let as_of = AsOfCatalog::from_stream(&events, 1);
        let price = as_of.price_of(OfferId(uid(20)), &[]).unwrap();
        assert_eq!(price.tax_rate, full_rate, "collection rate lost");
    }

    /// ux — per-line UNIT and OPTION prices stay separate, never a summed line total only.
    #[test]
    fn price_of_answers_unit_and_option_prices_separately() {
        let events = vec![
            fx_catalog_created(),
            fx_product_added_with_option_list(1500),
            fx_option_list_added(200),
        ];
        let as_of = AsOfCatalog::from_stream(&events, 2);
        let price = as_of.price_of(OfferId(uid(20)), &[OptionId(uid(40))]).unwrap();
        assert_eq!(price.unit_price, eur(1500));
        assert_eq!(price.option_prices, vec![(OptionId(uid(40)), eur(200))]);
    }

    /// An id that only ever appears in a DIFFERENT offer's stream is never confused with `off-1`'s
    /// state (basic isolation sanity for the fixtures above).
    #[test]
    fn two_offers_price_independently() {
        let events = vec![fx_catalog_created(), fx_product_added(1500), fx_second_product_added(900)];
        let as_of = AsOfCatalog::from_stream(&events, 2);
        assert_eq!(as_of.price_of(OfferId(uid(20)), &[]).unwrap().unit_price, eur(1500));
        assert_eq!(as_of.price_of(OfferId(uid(21)), &[]).unwrap().unit_price, eur(900));
    }

    /// THE BENCHMARK (a) — a native, ALWAYS-RUN ceiling test, not an SLO: a magnitude-regression
    /// detector only. L = 2,000 events is `UNVERIFIED input` (no measured Tours catalog stream
    /// length exists; derived from "the largest realistic HubRise import (~500 products x ~3 offers)
    /// plus a Friday's worth of stock syncs", itself a judgement call, not a measurement). The
    /// ceiling is ~10x the median measured at authoring on this container — if this test goes red,
    /// something got an order of magnitude slower, not "a millisecond over".
    #[test]
    fn fold_to_v_stays_under_ceiling_natively() {
        const L: usize = 2_000;
        let mut events = vec![fx_catalog_created()];
        for i in 0..L {
            events.push(fx_offer_stock_updated(i as f64));
        }
        events.insert(1, fx_product_added(1500));

        const ITERATIONS: u32 = 20;
        let mut samples = Vec::with_capacity(ITERATIONS as usize);
        for _ in 0..ITERATIONS {
            let start = std::time::Instant::now();
            let as_of = AsOfCatalog::from_stream(&events, events.len() as i64 - 1);
            std::hint::black_box(&as_of);
            samples.push(start.elapsed());
        }
        samples.sort();
        let median = samples[samples.len() / 2];
        // ~10x the ~50-150us median measured at authoring on this container (debug build).
        const CEILING: std::time::Duration = std::time::Duration::from_millis(15);
        assert!(
            median < CEILING,
            "fold of {L} events took {median:?} (median of {ITERATIONS}), ceiling {CEILING:?} -- \
             not an SLO, a magnitude-regression detector"
        );
    }
}
