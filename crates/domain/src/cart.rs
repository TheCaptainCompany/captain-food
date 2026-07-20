//! Cart aggregate — the PURE write-side state fold (ADR-0035/0046). Command handlers rehydrate a
//! [`CartState`] by folding the stream's events (loaded through the `EventStore` port) and then enforce
//! the invariants declared in `specs/actors.yaml`/`specs/errors.yaml` against it. Deliberately MINIMAL:
//! only the fields those invariants read are folded — the priced read model lives in the `Cart`
//! projection (ADR-0040), not here. No I/O, no serialization logic (dependency rule).
//!
//! The lifecycle mapping mirrors the read-side `CartProjector` so write-side decisions and the projected
//! `status` column can never disagree: `CartStarted` → OPEN, `CartCheckedOut` → CHECKED_OUT.

use crate::generated::entities::CartLineItem;
use crate::generated::events::DomainEvent;
use crate::generated::scalars::{CartLineId, CartStatus, CustomerId, RestaurantId};

/// Per-line quantity cap enforced on AddCartLine / ChangeCartLineQuantity
/// (`errors.yaml#/QuantityExceedsLimit`). V0 policy default: the spec declares the error but no
/// configurable limit; promote to a seeded referential policy table when one lands (ADR-0037).
pub const MAX_LINE_QUANTITY: i64 = 50;

/// What the Cart command handlers need to know about the aggregate to accept or reject a command.
/// `None` (from [`fold`]) means the cart does not exist yet — for `AddCartLine` that is the
/// create-on-first-add path (`CartStarted`), for the other commands it is `CartNotFound`.
#[derive(Debug, Clone, PartialEq)]
pub struct CartState {
    /// OPEN accepts line edits/checkout; CHECKED_OUT is final — `CartNotOpen`.
    pub status: CartStatus,
    /// The single restaurant this cart is bound to (no mixing) — `CartRestaurantMismatch`.
    pub restaurant_id: RestaurantId,
    /// Ids of the lines currently in the cart — `CartLineNotFound`, `CartEmpty`, idempotent re-adds.
    /// (Derivable from [`Self::lines`]; kept as its own field so existing call sites stay stable.)
    pub line_ids: Vec<CartLineId>,
    /// The full lines (offer, current quantity, selected options): `ChangeCartLineQuantity`
    /// re-checks the new quantity against the offer's LIVE stock, and checkout reprices the whole
    /// selection from the live catalog (rules.yaml#/ServerPriceAuthority — the cart events carry no
    /// money, so the fold must remember the complete selection for the server to price).
    pub lines: Vec<CartLineItem>,
    /// The customer the cart belongs to — set at `CartStarted` for a signed-in visitor, or later by
    /// `CartBoundToCustomer` when a guest cart is claimed after sign-in; `None` on a guest cart.
    pub customer_id: Option<CustomerId>,
}

/// Fold a Cart stream (events in version order) into its current state. `None` ⇔ the stream has no
/// `CartStarted` yet, i.e. the cart does not exist.
pub fn fold(events: &[DomainEvent]) -> Option<CartState> {
    events.iter().fold(None, apply)
}

/// Apply one event to the state — a pure transition, total over the whole event union (events not
/// touching the folded fields are no-ops, so a fatter stream never breaks rehydration).
fn apply(state: Option<CartState>, event: &DomainEvent) -> Option<CartState> {
    if let DomainEvent::CartStarted(e) = event {
        return Some(CartState {
            status: CartStatus::OPEN,
            restaurant_id: e.restaurant_id,
            line_ids: Vec::new(),
            lines: Vec::new(),
            customer_id: e.customer_id,
        });
    }
    let mut s = state?;
    match event {
        DomainEvent::CartLineAdded(e) => {
            if !s.line_ids.contains(&e.line.cart_line_id) {
                s.line_ids.push(e.line.cart_line_id);
                s.lines.push(e.line.clone());
            }
        }
        DomainEvent::CartLineQuantityChanged(e) => {
            if let Some(line) = s.lines.iter_mut().find(|l| l.cart_line_id == e.cart_line_id) {
                line.quantity = e.quantity;
            }
        }
        DomainEvent::CartLineRemoved(e) => {
            s.line_ids.retain(|id| id != &e.cart_line_id);
            s.lines.retain(|line| line.cart_line_id != e.cart_line_id);
        }
        DomainEvent::CartBoundToCustomer(e) => s.customer_id = Some(e.customer_id),
        DomainEvent::CartCheckedOut(_) => s.status = CartStatus::CHECKED_OUT,
        _ => {}
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::events::{CartBoundToCustomer, CartStarted};
    use crate::generated::scalars::{CartId, SessionId};

    fn started(customer_id: Option<CustomerId>) -> DomainEvent {
        DomainEvent::CartStarted(CartStarted {
            cart_id: CartId(uuid::Uuid::nil()),
            restaurant_id: RestaurantId(uuid::Uuid::nil()),
            session_id: SessionId(uuid::Uuid::nil()),
            customer_id,
        })
    }
    fn bound(customer_id: CustomerId) -> DomainEvent {
        DomainEvent::CartBoundToCustomer(CartBoundToCustomer {
            cart_id: CartId(uuid::Uuid::nil()),
            customer_id,
        })
    }

    #[test]
    fn guest_cart_has_no_customer_until_bound() {
        let customer = CustomerId(uuid::Uuid::nil());
        assert_eq!(fold(&[started(None)]).unwrap().customer_id, None);
        let s = fold(&[started(None), bound(customer)]).unwrap();
        assert_eq!(s.customer_id, Some(customer));
    }

    #[test]
    fn signed_in_start_carries_the_customer_and_rebinding_folds_to_the_same() {
        let customer = CustomerId(uuid::Uuid::nil());
        let s = fold(&[started(Some(customer))]).unwrap();
        assert_eq!(s.customer_id, Some(customer));
        // Re-delivering the bind is a harmless duplicate: same customer, same state.
        assert_eq!(fold(&[started(Some(customer)), bound(customer), bound(customer)]), Some(s));
    }
}
