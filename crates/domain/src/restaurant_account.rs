//! RestaurantAccount aggregate — the PURE write-side state fold (ADR-0035), mirroring
//! `restaurant.rs`. Command handlers rehydrate a [`RestaurantAccountState`] by folding the stream's
//! events and enforce the invariants declared in `specs/actors.yaml`/`specs/errors.yaml` against it.
//! Deliberately MINIMAL: only what the invariants read is folded. No I/O, no serialization logic.

use crate::generated::events::DomainEvent;
use crate::generated::scalars::{CurrencyCode, ExternalReference, RestaurantLegalName};

/// What the RestaurantAccount command handlers need to know to accept or reject a command. `None`
/// (from [`fold`]) means the account does not exist (never registered, or deleted) →
/// `RestaurantAccountNotFound`.
#[derive(Debug, Clone, PartialEq)]
pub struct RestaurantAccountState {
    /// Legal name, carried into rejection contexts.
    pub legal_name: RestaurantLegalName,
    /// Account-level default currency (set once at registration, ADR-0016).
    pub default_currency: CurrencyCode,
    /// External idempotent import key (HubRise `restaurant`), when seeded from an external source.
    pub r#ref: Option<ExternalReference>,
}

/// Fold a RestaurantAccount stream (events in version order) into its current state. `None` ⇔ no
/// `RestaurantAccountRegistered` yet, or the account was deleted (`RestaurantAccountDeleted` closes
/// it — later commands reject with `RestaurantAccountNotFound`).
pub fn fold(events: &[DomainEvent]) -> Option<RestaurantAccountState> {
    events.iter().fold(None, apply)
}

/// Apply one event — a pure transition, total over the whole event union (events not touching the
/// folded fields are no-ops, so a fatter stream never breaks rehydration).
fn apply(state: Option<RestaurantAccountState>, event: &DomainEvent) -> Option<RestaurantAccountState> {
    if let DomainEvent::RestaurantAccountRegistered(e) = event {
        return Some(RestaurantAccountState {
            legal_name: e.legal_name.clone(),
            default_currency: e.default_currency.clone(),
            r#ref: e.r#ref.clone(),
        });
    }
    let mut s = state?;
    match event {
        DomainEvent::RestaurantAccountUpdated(e) => {
            if let Some(name) = &e.legal_name {
                s.legal_name = name.clone();
            }
        }
        DomainEvent::RestaurantAccountDeleted(_) => return None,
        _ => {}
    }
    Some(s)
}
