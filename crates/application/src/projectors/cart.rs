//! Hand-written `CartCompute` (ADR-0040) — a PURE, MONEY-FREE fold (ADR-20260810-112836,
//! PROP-20260810-231500 Option B): the row stores identity, status and the repricing inputs only.
//! NO price is computed or stored here — the read side prices the lines fresh on every read via
//! `application::pricing::price_cart` (the same authority the checkout write path uses), and the
//! one authoritative freeze happens on `PaymentIntentCreated.CheckoutSnapshot`.
#![allow(unused_variables)]

use crate::projections::{CartCompute, CartRow, Envelope};
use domain::generated::entities::CartLineItem;
use domain::generated::events::DomainEvent;
use domain::generated::scalars::CartStatus;
use serde_json::Value;

pub struct CartProjector;

/// The typed view of `CartRow.lines` — the money-free repricing inputs, serialized with the SAME
/// serde shape as `CartLineItem` in the event payloads (camelCase: `cartLineId`, `offerId`,
/// `quantity`, `selectedOptionIds`), so the jsonb column and the domain log spell a line
/// identically and the read side deserializes with the same struct it validates against.
fn parse_lines(prev: Option<&CartRow>) -> Vec<CartLineItem> {
    prev.and_then(|r| serde_json::from_value(r.lines.clone()).ok()).unwrap_or_default()
}

fn to_value(lines: Vec<CartLineItem>) -> Value {
    serde_json::to_value(lines).unwrap_or_else(|_| Value::Array(Vec::new()))
}

impl CartCompute for CartProjector {
    // customer_id is MECHANICAL: CartStarted.customer_id / CartBoundToCustomer.customer_id are
    // same-stream properties (CartBindingProcess sends BindCartToCustomer per open cart), so the
    // generated projector maps them without a Compute hook.

    /// OPEN while active, CHECKED_OUT once the cart is checked out.
    fn status(&self, prev: Option<&CartRow>, env: &Envelope) -> CartStatus {
        match &env.event {
            DomainEvent::CartCheckedOut(_) => CartStatus::CHECKED_OUT,
            DomainEvent::CartStarted(_) => CartStatus::OPEN,
            _ => prev.map(|r| r.status.clone()).unwrap_or(CartStatus::OPEN),
        }
    }

    /// The money-free repricing inputs, folded verbatim from the cart events — the line IDENTITY
    /// exactly as the aggregate recorded it, nothing priced, nothing read outside the stream:
    /// - `CartStarted` → an empty list (the aggregate emits the first `CartLineAdded` separately);
    /// - `CartLineAdded` → append the event's line (same `cartLineId` replaces — replay-idempotent);
    /// - `CartLineQuantityChanged` → set that line's quantity;
    /// - `CartLineRemoved` → drop that line;
    /// - anything else → the previous value, untouched.
    fn lines(&self, prev: Option<&CartRow>, env: &Envelope) -> Value {
        match &env.event {
            DomainEvent::CartStarted(_) => Value::Array(Vec::new()),
            DomainEvent::CartLineAdded(e) => {
                let mut lines = parse_lines(prev);
                lines.retain(|l| l.cart_line_id != e.line.cart_line_id);
                lines.push(e.line.clone());
                to_value(lines)
            }
            DomainEvent::CartLineQuantityChanged(e) => {
                let mut lines = parse_lines(prev);
                if let Some(line) = lines.iter_mut().find(|l| l.cart_line_id == e.cart_line_id) {
                    line.quantity = e.quantity;
                }
                to_value(lines)
            }
            DomainEvent::CartLineRemoved(e) => {
                let mut lines = parse_lines(prev);
                lines.retain(|l| l.cart_line_id != e.cart_line_id);
                to_value(lines)
            }
            _ => prev.map(|r| r.lines.clone()).unwrap_or_else(|| Value::Array(Vec::new())),
        }
    }
}

#[cfg(test)]
mod tests {
    //! The money-free fold, replayed event by event (#451 Phase 2, rules.yaml#/
    //! CartPricedFromLiveCatalog): the `lines` column reproduces the LINE IDENTITY of the stream
    //! exactly — ids, quantities, selections — and nothing else. That "nothing else" is
    //! compile-time now: `CartRow` HAS no money fields (ADR-20260810-112836 dropped them), so a
    //! priced fold is unspellable and these tests only need to prove the identity half.
    use super::*;
    use crate::projections::project_cart;
    use domain::generated::events::{
        CartLineAdded, CartLineQuantityChanged, CartLineRemoved, CartStarted,
    };
    use domain::generated::scalars::{CartId, CartLineId, OfferId, OptionId, RestaurantId, SessionId};

    fn uid(n: u8) -> uuid::Uuid {
        uuid::Uuid::from_u128(n as u128)
    }
    fn env(event: DomainEvent, position: i64) -> Envelope {
        Envelope {
            stream_name: "Cart-test".into(),
            position,
            occurred_at: chrono::DateTime::from_timestamp(1_700_000_000 + position, 0).unwrap(),
            event,
        }
    }
    fn line(id: u8, offer: u8, quantity: i64, options: &[u8]) -> CartLineItem {
        CartLineItem {
            cart_line_id: CartLineId(uid(id)),
            offer_id: OfferId(uid(offer)),
            quantity,
            selected_option_ids: options.iter().map(|o| OptionId(uid(*o))).collect(),
        }
    }

    /// Replay: start → add two lines (one with a selected option) → change a quantity → remove a
    /// line. After each step the folded `lines` equals the stream's line identity, spelled with
    /// the event payload's own serde shape (camelCase), so a replay reproduces the column exactly.
    #[test]
    fn the_fold_reproduces_the_line_identity_of_the_stream_exactly() {
        let p = CartProjector;
        let started = env(
            DomainEvent::CartStarted(CartStarted {
                cart_id: CartId(uid(1)),
                restaurant_id: RestaurantId(uid(2)),
                session_id: SessionId(uid(3)),
                customer_id: None,
            }),
            1,
        );
        let row = project_cart(&p, None, &started).expect("CartStarted materializes the row");
        assert_eq!(row.lines, serde_json::json!([]), "a cart starts with no lines");

        let added_a = env(
            DomainEvent::CartLineAdded(CartLineAdded {
                cart_id: CartId(uid(1)),
                line: line(10, 20, 1, &[]),
            }),
            2,
        );
        let row = project_cart(&p, Some(row), &added_a).expect("line A folds");
        let added_b = env(
            DomainEvent::CartLineAdded(CartLineAdded {
                cart_id: CartId(uid(1)),
                line: line(11, 21, 2, &[30]),
            }),
            3,
        );
        let row = project_cart(&p, Some(row), &added_b).expect("line B folds");
        assert_eq!(
            row.lines,
            serde_json::to_value(vec![line(10, 20, 1, &[]), line(11, 21, 2, &[30])]).unwrap(),
            "both lines, in stream order, with the event payload's own serde spelling"
        );

        let changed = env(
            DomainEvent::CartLineQuantityChanged(CartLineQuantityChanged {
                cart_id: CartId(uid(1)),
                cart_line_id: CartLineId(uid(10)),
                quantity: 3,
            }),
            4,
        );
        let row = project_cart(&p, Some(row), &changed).expect("quantity change folds");
        assert_eq!(
            row.lines,
            serde_json::to_value(vec![line(10, 20, 3, &[]), line(11, 21, 2, &[30])]).unwrap(),
            "the quantity change lands on line A only"
        );

        let removed = env(
            DomainEvent::CartLineRemoved(CartLineRemoved {
                cart_id: CartId(uid(1)),
                cart_line_id: CartLineId(uid(11)),
            }),
            5,
        );
        let row = project_cart(&p, Some(row), &removed).expect("removal folds");
        assert_eq!(
            row.lines,
            serde_json::to_value(vec![line(10, 20, 3, &[])]).unwrap(),
            "line B is gone; line A survives with its changed quantity"
        );
    }

    /// Replaying the SAME CartLineAdded over a row that already holds the line replaces rather
    /// than duplicates — the at-least-once projection dispatch must stay idempotent per event.
    #[test]
    fn a_replayed_line_add_replaces_instead_of_duplicating() {
        let p = CartProjector;
        let started = env(
            DomainEvent::CartStarted(CartStarted {
                cart_id: CartId(uid(1)),
                restaurant_id: RestaurantId(uid(2)),
                session_id: SessionId(uid(3)),
                customer_id: None,
            }),
            1,
        );
        let row = project_cart(&p, None, &started).unwrap();
        let added = env(
            DomainEvent::CartLineAdded(CartLineAdded {
                cart_id: CartId(uid(1)),
                line: line(10, 20, 2, &[31]),
            }),
            2,
        );
        let row = project_cart(&p, Some(row), &added).unwrap();
        let row = project_cart(&p, Some(row), &added).unwrap();
        assert_eq!(
            row.lines,
            serde_json::to_value(vec![line(10, 20, 2, &[31])]).unwrap(),
            "one line, not two — same cartLineId replaces"
        );
    }
}
