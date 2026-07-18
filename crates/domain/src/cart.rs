//! Cart aggregate — the PURE write-side state fold (ADR-0035/0046). Command handlers rehydrate a
//! [`CartState`] by folding the stream's events (loaded through the `EventStore` port) and then enforce
//! the invariants declared in `specs/actors.yaml`/`specs/errors.yaml` against it. Deliberately MINIMAL:
//! only the fields those invariants read are folded — the priced read model lives in the `Cart`
//! projection (ADR-0040), not here. No I/O, no serialization logic (dependency rule).
//!
//! The lifecycle mapping mirrors the read-side `CartProjector` so write-side decisions and the projected
//! `status` column can never disagree: `CartStarted` → OPEN, `CartCheckedOut` → CHECKED_OUT.

use crate::generated::events::DomainEvent;
use crate::generated::scalars::{CartLineId, CartStatus, RestaurantId};

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
    pub line_ids: Vec<CartLineId>,
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
        });
    }
    let mut s = state?;
    match event {
        DomainEvent::CartLineAdded(e) => {
            if !s.line_ids.contains(&e.line.cart_line_id) {
                s.line_ids.push(e.line.cart_line_id);
            }
        }
        DomainEvent::CartLineRemoved(e) => s.line_ids.retain(|id| id != &e.cart_line_id),
        DomainEvent::CartCheckedOut(_) => s.status = CartStatus::CHECKED_OUT,
        _ => {}
    }
    Some(s)
}
