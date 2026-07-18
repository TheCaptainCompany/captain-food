//! CartBindingProcess (actors.yaml#/CartBindingProcess) — binds a returning visitor's OPEN guest carts
//! to their Customer when the inbound `CustomerIdentified` fact arrives (authRef → customerId).
//!
//! Per the DSL the inbox entry declares `emits: []` and the effect is "Bind customerId onto the
//! visitor's OPEN carts (Cart projection); no new event in V0" — i.e. the bind is a READ-MODEL update,
//! not a new domain fact. A process manager only appends events, so there is nothing for it to emit;
//! the actual stamping is the Cart projection's cross-stream fold
//! (`projectors::cart::CartProjector::customer_id`, documented `TODO(runtime)`: the projection worker's
//! Cart group drains only `Cart-%` streams today, so Customer-stream events never reach it). Until that
//! routing lands, the reaction reports [`Decision::Skip`] so the pending bind stays observable.

use domain::generated::events::CustomerIdentified;

use crate::process_managers::Decision;

/// React to `CustomerIdentified` (rules.yaml#/GuestCartsBoundOnIdentification). Emits nothing (V0 —
/// no binding event is modelled); the bind itself is the Cart projection's job.
pub fn on_customer_identified(event: &CustomerIdentified) -> Decision {
    Decision::Skip(format!(
        "TODO(runtime): bind customer {} (authRef {}) onto their OPEN guest carts — no binding event \
         is modelled in V0 (actors.yaml emits []); the bind is the Cart projection's cross-stream fold \
         (CartProjector::customer_id), which the projection worker does not route Customer-stream \
         events to yet",
        event.customer_id.0, event.auth_ref.0
    ))
}

// ================================================================================================
// Behaviour tests (specs/tests.yaml, CartBindingProcess saga).
// ================================================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use domain::generated::scalars::{CustomerId, ExternalReference};

    /// tests.yaml#/TestCartBindingOnCustomerIdentified — rules.yaml#/GuestCartsBoundOnIdentification:
    /// the reaction emits NO domain event (`then: []`); the bind is delegated to the Cart projection
    /// and stays observable as a logged pending effect until the cross-stream routing lands.
    #[test]
    fn customer_identified_emits_nothing_and_delegates_to_cart_projection() {
        let d = on_customer_identified(&CustomerIdentified {
            customer_id: CustomerId(uuid::Uuid::from_u128(7)),
            auth_ref: ExternalReference("auth-supabase-1".into()),
        });
        assert!(d.appends().is_empty());
        assert!(matches!(d, Decision::Skip(ref m) if m.contains("auth-supabase-1")), "{d:?}");
    }
}
