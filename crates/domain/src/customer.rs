//! Customer aggregate — the PURE write-side state fold (ADR-0035), mirroring `restaurant.rs`. The
//! identity flows (phone OTP / email magic link) are WRAPPED Supabase Auth (ADR-0015): verification
//! happens in the auth-provider port at the ACL boundary, and only the verified FACTS land on this
//! stream — so the fold tracks just what the declared invariants and no-op idempotencies read
//! (favorites, address book, stored locale). No I/O, no serialization logic (dependency rule).

use std::collections::HashSet;

use crate::generated::events::DomainEvent;
use crate::generated::scalars::{AddressId, Locale, PhoneNumber, RestaurantId};

/// What the Customer command handlers need to know to accept or reject a command. `None` (from
/// [`fold`]) means no `CustomerRegistered` yet on this stream.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomerState {
    /// Current canonical E.164 phone (primary identifier; changed via ConfirmPhoneChange).
    pub phone: PhoneNumber,
    /// Stored preferred locale — localizes later authenticated SMS/email sends (ADR-0015).
    pub locale: Option<Locale>,
    /// Favorited restaurants — makes UnmarkRestaurantAsFavorite an idempotent no-op on non-favorites.
    pub favorites: HashSet<RestaurantId>,
    /// Saved address-book ids — makes RemoveCustomerAddress an idempotent no-op on unknown addresses.
    pub addresses: HashSet<AddressId>,
}

/// Fold a Customer stream (events in version order) into its current state. `None` ⇔ the stream has
/// no `CustomerRegistered` yet, i.e. the customer does not exist.
pub fn fold(events: &[DomainEvent]) -> Option<CustomerState> {
    events.iter().fold(None, apply)
}

/// Apply one event — a pure transition, total over the whole event union.
fn apply(state: Option<CustomerState>, event: &DomainEvent) -> Option<CustomerState> {
    if let DomainEvent::CustomerRegistered(e) = event {
        return Some(CustomerState {
            phone: e.phone.clone(),
            locale: e.locale.clone(),
            favorites: HashSet::new(),
            addresses: HashSet::new(),
        });
    }
    let mut s = state?;
    match event {
        DomainEvent::CustomerPhoneChanged(e) => s.phone = e.phone.clone(),
        DomainEvent::CustomerLanguageChanged(e) => s.locale = Some(e.locale.clone()),
        DomainEvent::RestaurantFavorited(e) => {
            s.favorites.insert(e.restaurant_id);
        }
        DomainEvent::RestaurantUnfavorited(e) => {
            s.favorites.remove(&e.restaurant_id);
        }
        DomainEvent::CustomerAddressSet(e) => {
            s.addresses.insert(e.address_id);
        }
        DomainEvent::CustomerAddressRemoved(e) => {
            s.addresses.remove(&e.address_id);
        }
        _ => {}
    }
    Some(s)
}
