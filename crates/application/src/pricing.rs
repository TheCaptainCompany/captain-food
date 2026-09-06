//! Server-side pricing (rules.yaml#/ServerPriceAuthority, ADR server-side-pricing): the server is
//! the ONLY price authority on the write path. At checkout every cart line is repriced from the
//! LIVE catalog (Offer price + selected Option prices, through the `CatalogReadRepository` read
//! port) into the priced `OrderLineItem`s, the order total and the `PaymentBreakdown` that feed the
//! payment-intent amount and the frozen `CheckoutSnapshot`. No client-supplied amount is ever read.
//!
//! FAIL-CLOSED: a line whose price cannot be resolved from the live catalog — offer gone, a selected
//! option gone, or a currency clash across the resolved prices — rejects the checkout with the typed
//! `errors.yaml#/PriceUnresolvable`; there is NO fallback to a client number.
//!
//! The V0 breakdown mirrors the checkout shape already frozen on `PaymentIntentCreated`: articles =
//! total, all fee/split legs zero, restaurant_payout = total. The real ADR-0016/0017 fee/split policy
//! plugs in here without changing any caller.

use domain::generated::entities::{
    CartLineItem, Money, OrderLineItem, PaymentBreakdown, SelectedOption,
};
use domain::generated::scalars::{CartId, CurrencyCode, MoneyCents, OfferId, RestaurantId};
use domain::shared::errors::DomainError;
use serde_json::json;

use async_trait::async_trait;

use crate::queries::{offer_view_from_tree, CatalogReadRepository, CatalogRow, OfferView};

/// The server-priced checkout: the priced order lines, their total, and the fee/split breakdown.
/// Invariant: `total_amount == breakdown.total` (== the payment-intent amount).
#[derive(Debug, Clone, PartialEq)]
pub struct PricedCart {
    pub items: Vec<OrderLineItem>,
    pub total_amount: Money,
    pub breakdown: PaymentBreakdown,
}

/// The fail-closed rejection: this line's price cannot be resolved from the live catalog
/// (`errors.yaml#/PriceUnresolvable`).
fn unresolvable(cart_id: CartId, offer_id: OfferId) -> DomainError {
    DomainError::rejected("PriceUnresolvable", json!({ "cartId": cart_id, "offerId": offer_id }))
}

/// Reprice the cart's lines from the LIVE catalog (never from the client): each line's unit price is
/// the offer's live price, each selected option is priced from its live option list, and
/// `lineTotal = (unitPrice + sum(selectedOptions.price)) * quantity` (entities.yaml#/OrderLineItem).
/// Any unresolved offer/option — or a currency clash — is `PriceUnresolvable` (fail-closed).
pub async fn price_cart(
    catalogs: &dyn CatalogReadRepository,
    cart_id: CartId,
    restaurant_id: RestaurantId,
    lines: &[CartLineItem],
) -> Result<PricedCart, DomainError> {
    let mut items: Vec<OrderLineItem> = Vec::with_capacity(lines.len());
    let mut total_cents: i64 = 0;
    let mut currency: Option<CurrencyCode> = None;

    for line in lines {
        let Some(offer) = catalogs.offer_by_id(restaurant_id, line.offer_id).await? else {
            return Err(unresolvable(cart_id, line.offer_id));
        };
        // One currency across the whole cart (the restaurant's) — a clash means the catalog is
        // inconsistent, so the price is unresolvable (never guess).
        let cur = currency.get_or_insert_with(|| offer.price.currency.clone());
        if offer.price.currency != *cur {
            return Err(unresolvable(cart_id, line.offer_id));
        }
        let mut selected_options: Vec<SelectedOption> = Vec::with_capacity(line.selected_option_ids.len());
        let mut options_cents: i64 = 0;
        for option_id in &line.selected_option_ids {
            let Some((list_id, option)) = offer.option_lists.iter().find_map(|list| {
                list.options.iter().find(|o| o.id == *option_id).map(|o| (list.id, o))
            }) else {
                return Err(unresolvable(cart_id, line.offer_id));
            };
            if option.price.currency != *cur {
                return Err(unresolvable(cart_id, line.offer_id));
            }
            options_cents += option.price.amount_cents.0;
            selected_options.push(SelectedOption {
                option_id: option.id,
                option_list_id: Some(list_id),
                name: option.name.clone(),
                price: option.price.clone(),
            });
        }
        let line_total_cents = (offer.price.amount_cents.0 + options_cents) * line.quantity;
        total_cents += line_total_cents;
        items.push(OrderLineItem {
            offer_id: line.offer_id,
            product_id: Some(offer.product_id),
            name: offer.product_name.clone(),
            offer_name: Some(offer.offer_name.clone()),
            quantity: line.quantity,
            unit_price: offer.price.clone(),
            selected_options,
            line_total: Money { amount_cents: MoneyCents(line_total_cents), currency: cur.clone() },
        });
    }

    // An empty cart never reaches pricing (`CartEmpty` guards checkout first) — but stay fail-closed
    // rather than fabricate a zero total in a guessed currency.
    let Some(currency) = currency else {
        return Err(DomainError::rejected("CartEmpty", json!({ "cartId": cart_id })));
    };
    let total_amount = Money { amount_cents: MoneyCents(total_cents), currency: currency.clone() };
    let zero = Money { amount_cents: MoneyCents(0), currency };
    let breakdown = PaymentBreakdown {
        articles: total_amount.clone(),
        delivery: zero.clone(),
        service_fee: zero.clone(),
        total: total_amount.clone(),
        restaurant_contribution: zero.clone(),
        restaurant_payout: total_amount.clone(),
        rider_payout: zero.clone(),
        captain_net: zero,
    };
    Ok(PricedCart { items, total_amount, breakdown })
}

/// A ONE-READ catalog memo for the read-side pricing seam (#451 Phase 2, the cart-price contract's
/// `cart_price_ms` comment): [`price_cart`] resolves every line through
/// [`CatalogReadRepository::offer_by_id`], whose provided implementation re-reads the WHOLE
/// projected catalog row per line — N lines, N reads of the same jsonb tree. At peak (Fri/Sat
/// 19:00–21:30, CLAUDE.md) every /checkout render and mini-cart pays that, so the resolver seam
/// loads the row ONCE and walks it in memory for all lines: N→1 catalog reads per priced cart,
/// with `price_cart` itself unchanged, pure and SDK-free.
///
/// The snapshot is scoped to ONE restaurant on purpose: a lookup for any other restaurant answers
/// `None` (fail-closed → `PriceUnresolvable`), never a silent fallthrough to the live store —
/// a cross-restaurant cart line is a data defect the pricer must surface, not paper over.
pub struct CatalogSnapshot {
    restaurant_id: RestaurantId,
    row: Option<CatalogRow>,
}

impl CatalogSnapshot {
    /// The one catalog read: load `restaurant_id`'s projected catalog row (or its absence) once.
    pub async fn load(
        catalogs: &dyn CatalogReadRepository,
        restaurant_id: RestaurantId,
    ) -> Result<Self, DomainError> {
        Ok(Self { restaurant_id, row: catalogs.by_restaurant(restaurant_id).await? })
    }
}

#[async_trait]
impl CatalogReadRepository for CatalogSnapshot {
    async fn by_restaurant(
        &self,
        restaurant_id: RestaurantId,
    ) -> Result<Option<CatalogRow>, DomainError> {
        Ok((restaurant_id == self.restaurant_id).then(|| self.row.clone()).flatten())
    }

    /// The in-memory walk — no row clone per line, unlike the provided implementation.
    async fn offer_by_id(
        &self,
        restaurant_id: RestaurantId,
        offer_id: OfferId,
    ) -> Result<Option<OfferView>, DomainError> {
        if restaurant_id != self.restaurant_id {
            return Ok(None);
        }
        Ok(self.row.as_ref().and_then(|row| offer_view_from_tree(&row.tree, offer_id)))
    }
}

#[cfg(test)]
mod tests {
    //! #451 Phase 2 (rules.yaml#/CartPricedFromLiveCatalog, /ServerPriceAuthority): the ONE pricing
    //! authority, unit-tested at its own seam, plus the [`CatalogSnapshot`] memo that makes the
    //! read side call it with ONE catalog read instead of one per line.
    use std::sync::atomic::{AtomicUsize, Ordering};

    use domain::generated::entities::{
        Offer, OptionList, Product, ProductItemOption, Stock, TaxRate,
    };
    use domain::generated::scalars::{
        CartLineId, CatalogId, CatalogItemAvailability, CatalogName, OfferName, OptionId,
        OptionListId, OptionListName, OptionName, ProductId, ProductName, Quantity, StockStatus,
        TaxRatePercent,
    };

    use super::*;
    use crate::behaviour_support::SpecCatalogs;
    use crate::queries::{OfferOptionListView, OfferOptionView};

    fn uid(n: u8) -> uuid::Uuid {
        uuid::Uuid::from_u128(n as u128)
    }
    fn eur(cents: i64) -> Money {
        Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
    }
    fn cart_line(id: u8, offer: u8, quantity: i64, options: &[u8]) -> CartLineItem {
        CartLineItem {
            cart_line_id: CartLineId(uid(id)),
            offer_id: OfferId(uid(offer)),
            quantity,
            selected_option_ids: options.iter().map(|o| OptionId(uid(*o))).collect(),
        }
    }

    /// An [`OfferView`] as the write-path fake ([`SpecCatalogs`]) serves it: one offer with one
    /// option list holding one priced option.
    fn offer_view(offer: u8, price_cents: i64, option: Option<(u8, i64)>) -> OfferView {
        let option_lists = option
            .map(|(id, cents)| {
                vec![OfferOptionListView {
                    id: OptionListId(uid(200)),
                    min_selections: 0,
                    max_selections: Some(1),
                    multiple_selection: false,
                    option_ids: vec![OptionId(uid(id))],
                    options: vec![OfferOptionView {
                        id: OptionId(uid(id)),
                        name: OptionName("Extra".into()),
                        price: eur(cents),
                    }],
                }]
            })
            .unwrap_or_default();
        OfferView {
            offer_id: OfferId(uid(offer)),
            product_id: ProductId(uid(100 + offer)),
            product_name: ProductName("Burger".into()),
            offer_name: OfferName("Default".into()),
            price: eur(price_cents),
            availability: CatalogItemAvailability::AVAILABLE,
            stock_status: StockStatus::IN_STOCK,
            stock_quantity: Some(Quantity(10.0)),
            option_lists,
        }
    }

    /// THE known-price case from the mob brief: one line, offer 15,00 EUR + selected option
    /// 2,00 EUR, quantity ×2 → line total and cart total 34,00 EUR, breakdown.total identical
    /// (the payment-intent amount), currency EUR throughout.
    ///
    /// HONESTY NOTE: this test was BORN GREEN — `price_cart` already computed this correctly for
    /// the checkout write path; the #451 bug was the projection stub that never called it on reads.
    ///
    /// It previously claimed the dispatch's RED lived in `projectors::cart::tests` "seen red
    /// against the stub fold". That is not true and was never observed: `main` carries no test
    /// module in `projectors/cart.rs` at all, and `57b7330` added the fold AND its tests in the
    /// same commit — the commit whose own message records that no gates were run on that tree. The
    /// test therefore never existed against the stub it was said to have failed against.
    ///
    /// What can honestly be said: no red was observed anywhere in this slice. The evidence that
    /// these tests can fail is the ordinary kind — they assert specific values (34,00 EUR, a
    /// matching breakdown total) that a wrong implementation would not produce. Claiming a
    /// red-green cycle that nobody ran is worth less than claiming nothing.
    #[tokio::test]
    async fn a_line_with_an_option_at_quantity_two_prices_to_3400() {
        let catalogs = SpecCatalogs::default();
        let restaurant = RestaurantId(uid(1));
        catalogs.add_offer(restaurant, offer_view(20, 1500, Some((30, 200))));

        let priced = price_cart(
            &catalogs,
            CartId(uid(2)),
            restaurant,
            &[cart_line(10, 20, 2, &[30])],
        )
        .await
        .expect("both the offer and the option resolve from the live catalog");

        assert_eq!(priced.total_amount, eur(3400), "(1500 + 200) * 2");
        assert_eq!(priced.items.len(), 1);
        assert_eq!(priced.items[0].unit_price, eur(1500));
        assert_eq!(priced.items[0].line_total, eur(3400));
        assert_eq!(priced.items[0].selected_options.len(), 1);
        assert_eq!(priced.items[0].selected_options[0].price, eur(200));
        assert_eq!(
            priced.breakdown.total, priced.total_amount,
            "the breakdown total IS the payment-intent amount (PricedCart invariant)"
        );
    }

    /// A live-catalog store that COUNTS its row reads — the N+1 evidence. Its tree is built from
    /// the real entity types (`Product`/`Offer`/`OptionList`) through serde, so
    /// `offer_view_from_tree` walks exactly what the CatalogProjector would have written.
    struct CountingCatalog {
        row: CatalogRow,
        reads: AtomicUsize,
    }

    #[async_trait]
    impl CatalogReadRepository for CountingCatalog {
        async fn by_restaurant(
            &self,
            restaurant_id: RestaurantId,
        ) -> Result<Option<CatalogRow>, DomainError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok((restaurant_id == self.row.restaurant_id).then(|| self.row.clone()))
        }
    }

    fn tree_catalog(restaurant: RestaurantId, offers: &[(u8, i64)]) -> CatalogRow {
        let products: Vec<Product> = offers
            .iter()
            .map(|(offer, cents)| Product {
                id: ProductId(uid(100 + offer)),
                r#ref: None,
                catalog_id: CatalogId(uid(50)),
                restaurant_id: restaurant,
                category_ref: None,
                name: ProductName(format!("Product {offer}")),
                description: None,
                tags: Vec::new(),
                image_ids: Vec::new(),
                tax_rate: TaxRate {
                    delivery: TaxRatePercent(10.0),
                    collection: None,
                    eat_in: None,
                },
                offers: vec![Offer {
                    id: OfferId(uid(*offer)),
                    r#ref: None,
                    product_id: ProductId(uid(100 + offer)),
                    name: OfferName("Default".into()),
                    price: eur(*cents),
                    availability: CatalogItemAvailability::AVAILABLE,
                    stock: Some(Stock {
                        quantity: Quantity(10.0),
                        low_stock_threshold: None,
                        status: StockStatus::IN_STOCK,
                        expires_at: None,
                    }),
                    option_list_ids: Vec::new(),
                }],
            })
            .collect();
        let _ = OptionList {
            id: OptionListId(uid(200)),
            r#ref: None,
            name: OptionListName("Extras".into()),
            min_selections: 0,
            max_selections: None,
            multiple_selection: false,
            options: Vec::<ProductItemOption>::new(),
        }; // shape pin: the tree's optionLists section serializes from this entity
        CatalogRow {
            catalog_id: CatalogId(uid(50)),
            restaurant_id: restaurant,
            slug: None,
            name: CatalogName("Menu".into()),
            tree: serde_json::json!({
                "products": serde_json::to_value(&products).unwrap(),
                "optionLists": [],
            }),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    /// The memo evidence, both directions: pricing a 3-line cart straight through the store's
    /// provided `offer_by_id` costs THREE catalog-row reads (the N+1 this seam retires); through
    /// [`CatalogSnapshot`] it costs exactly ONE — and the priced result is identical.
    #[tokio::test]
    async fn the_snapshot_prices_a_multi_line_cart_with_one_catalog_read() {
        let restaurant = RestaurantId(uid(1));
        let lines =
            [cart_line(10, 20, 1, &[]), cart_line(11, 21, 2, &[]), cart_line(12, 22, 1, &[])];

        // Baseline: the provided per-line lookup — N reads for N lines.
        let store = CountingCatalog {
            row: tree_catalog(restaurant, &[(20, 1000), (21, 250), (22, 500)]),
            reads: AtomicUsize::new(0),
        };
        let direct = price_cart(&store, CartId(uid(2)), restaurant, &lines)
            .await
            .expect("all three offers resolve");
        assert_eq!(
            store.reads.load(Ordering::SeqCst),
            3,
            "the provided offer_by_id re-reads the catalog row once PER LINE — the N+1"
        );

        // The seam: load once, walk in memory.
        store.reads.store(0, Ordering::SeqCst);
        let snapshot = CatalogSnapshot::load(&store, restaurant).await.expect("one load");
        let memoized = price_cart(&snapshot, CartId(uid(2)), restaurant, &lines)
            .await
            .expect("the same three offers resolve from the snapshot");
        assert_eq!(store.reads.load(Ordering::SeqCst), 1, "N→1: one catalog read per request");
        assert_eq!(memoized, direct, "the memo changes the read count, never the price");
        assert_eq!(memoized.total_amount, eur(2000), "1000 + 250*2 + 500");
    }

    /// Fail-closed scoping: a snapshot loaded for restaurant A answers `None` for restaurant B's
    /// offers, so a cross-restaurant line surfaces as `PriceUnresolvable` instead of silently
    /// pricing from the wrong catalog.
    #[tokio::test]
    async fn the_snapshot_never_answers_for_another_restaurant() {
        let restaurant_a = RestaurantId(uid(1));
        let restaurant_b = RestaurantId(uid(9));
        let store = CountingCatalog {
            row: tree_catalog(restaurant_a, &[(20, 1000)]),
            reads: AtomicUsize::new(0),
        };
        let snapshot = CatalogSnapshot::load(&store, restaurant_a).await.expect("loads");

        let err = price_cart(&snapshot, CartId(uid(2)), restaurant_b, &[cart_line(10, 20, 1, &[])])
            .await
            .expect_err("restaurant B's line must not price from restaurant A's snapshot");
        assert!(
            format!("{err:?}").contains("PriceUnresolvable"),
            "fail-closed: PriceUnresolvable, got {err:?}"
        );
    }

    /// THE ONE-PRICER PROPERTY (ADR-20260810-112836 §1/§3/§5/§6), protected by an EQUIVALENCE test
    /// instead of a caller (PROP-20260831-134539 slice 2, DARK — mob consent, no SPLIT survivor): for
    /// a fixture catalog folded to HEAD, [`domain::catalog_as_of::AsOfCatalog::price_of`] at
    /// `V = HEAD` must answer the SAME unit price and option price [`price_cart`] computes through
    /// the real projector + [`CatalogSnapshot`]/[`OfferView`] path, for the same line and options.
    /// Two independent folds over the SAME events (the real `CatalogProjector` and
    /// `AsOfCatalog::from_stream`) must never diverge into two prices.
    #[tokio::test]
    async fn as_of_price_at_head_equals_the_head_snapshot_price() {
        use domain::catalog_as_of::AsOfCatalog;
        use domain::generated::events::{CatalogCreated, DomainEvent, OptionListAdded, ProductAdded};

        use crate::projections::{project_catalog, Envelope};
        use crate::projectors::catalog::CatalogProjector;

        let catalog_id = CatalogId(uid(1));
        let restaurant = RestaurantId(uid(2));
        let product_id = ProductId(uid(10));
        let offer_id = OfferId(uid(20));
        let option_list_id = OptionListId(uid(30));
        let option_id = OptionId(uid(40));

        let events: Vec<DomainEvent> = vec![
            DomainEvent::CatalogCreated(CatalogCreated {
                catalog_id,
                r#ref: None,
                restaurant_id: restaurant,
                name: CatalogName("Main".into()),
            }),
            DomainEvent::OptionListAdded(OptionListAdded {
                catalog_id,
                restaurant_id: restaurant,
                option_list: OptionList {
                    id: option_list_id,
                    r#ref: None,
                    name: OptionListName("Size".into()),
                    min_selections: 0,
                    max_selections: Some(1),
                    multiple_selection: false,
                    options: vec![ProductItemOption {
                        id: option_id,
                        r#ref: None,
                        option_list_id,
                        name: OptionName("Large".into()),
                        price: eur(200),
                        r#default: false,
                        availability: CatalogItemAvailability::AVAILABLE,
                        stock: None,
                    }],
                },
            }),
            DomainEvent::ProductAdded(ProductAdded {
                catalog_id,
                restaurant_id: restaurant,
                product: Product {
                    id: product_id,
                    r#ref: None,
                    catalog_id,
                    restaurant_id: restaurant,
                    category_ref: None,
                    name: ProductName("Margherita".into()),
                    description: None,
                    tags: Vec::new(),
                    image_ids: Vec::new(),
                    tax_rate: TaxRate {
                        delivery: TaxRatePercent(10.0),
                        collection: None,
                        eat_in: None,
                    },
                    offers: vec![Offer {
                        id: offer_id,
                        r#ref: None,
                        product_id,
                        name: OfferName("Default".into()),
                        price: eur(1500),
                        availability: CatalogItemAvailability::AVAILABLE,
                        stock: None,
                        option_list_ids: vec![option_list_id],
                    }],
                },
            }),
        ];

        // HEAD via the REAL projector fold (the same one the projection worker runs).
        let mut row: Option<CatalogRow> = None;
        for (i, event) in events.iter().enumerate() {
            let env = Envelope {
                stream_name: domain::catalog::stream(catalog_id),
                position: i as i64 + 1,
                occurred_at: chrono::Utc::now(),
                event: event.clone(),
            };
            row = project_catalog(&CatalogProjector, row, &env);
        }
        let row = row.expect("catalog row exists at HEAD");

        let store = CountingCatalog { row, reads: AtomicUsize::new(0) };
        let snapshot = CatalogSnapshot::load(&store, restaurant).await.expect("loads");
        let priced = price_cart(
            &snapshot,
            CartId(uid(99)),
            restaurant,
            &[cart_line(1, 20, 1, &[40])],
        )
        .await
        .expect("HEAD prices the line");

        // The as-of fold, independently, at V = HEAD.
        let as_of = AsOfCatalog::from_stream(&events, events.len() as i64 - 1);
        let as_of_price =
            as_of.price_of(offer_id, &[option_id]).expect("as-of prices the same offer");

        assert_eq!(
            priced.items[0].unit_price, as_of_price.unit_price,
            "totals differ on an options-bearing cart (unit price)"
        );
        assert_eq!(
            priced.items[0].selected_options[0].price,
            as_of_price.option_prices[0].1,
            "totals differ on an options-bearing cart (option price)"
        );
    }
}
