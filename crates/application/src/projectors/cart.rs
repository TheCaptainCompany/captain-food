//! Hand-written `CartCompute` (ADR-0040) — a PURE, MONEY-FREE fold (ADR-20260810-112836,
//! PROP-20260810-231500 Option B): the row stores identity, status and the repricing inputs only.
//! NO price is computed or stored here — the read side prices the lines fresh on every read via
//! `application::pricing::price_cart` (the same authority the checkout write path uses), and the
//! one authoritative freeze happens on `PaymentIntentCreated.CheckoutSnapshot`.
#![allow(unused_variables)]

use crate::projections::{CartCompute, CartRow, Envelope};
use domain::generated::events::DomainEvent;
use domain::generated::scalars::CartStatus;
use serde_json::Value;

pub struct CartProjector;

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

    /// The money-free repricing inputs, folded verbatim from the cart events:
    /// `[{ cart_line_id, offer_id, quantity, selected_option_ids }]`.
    /// TODO(#451 Phase 2): fold CartLineAdded/CartLineQuantityChanged/CartLineRemoved into this
    /// shape (today, as before this change, nothing folds lines — the pre-#451 stub carried
    /// forward; Phase 2 lands the fold together with the read-side pricer so the priced read
    /// has real inputs).
    fn lines(&self, prev: Option<&CartRow>, env: &Envelope) -> Value {
        prev.map(|r| r.lines.clone()).unwrap_or_else(|| Value::Array(Vec::new()))
    }
}
