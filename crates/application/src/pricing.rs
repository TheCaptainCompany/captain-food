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

use crate::queries::CatalogReadRepository;

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
