//! CartBindingProcess (`specs/processmanager.yaml#/CartBindingProcess`) — HOOK IMPL + thin wrapper
//! for the GENERATED `CustomerIdentified` leg (`crate::generated::process_managers::cart_binding_process`,
//! issue #25). The pipeline (for-each send with rejection-skip semantics, state.set) is generated;
//! this module keeps only the `read open_carts` seam over the Cart read model.
//!
//! Send semantics on an EVENT leg (generated): a rejection by the Cart (e.g. already bound) is
//! LOGGED and SKIPPED for that cart, never re-thrown — the remaining carts still bind.

use domain::generated::events::CustomerIdentified;
use domain::generated::scalars::SessionId;
use domain::shared::errors::DomainError;

use crate::generated::process_managers::cart_binding_process::{self, OpenCartsRead};
use crate::pm_state::CartBindingStateStore;
use crate::ports::EventStore;
use crate::process_managers::{Outcome, TriggerEnvelope};
use crate::queries::CartReadRepository;

/// Hooks for the `CustomerIdentified` leg: the OPEN-carts read (session_id + status = OPEN).
pub struct CartBindingHooks<'a> {
    pub carts: &'a dyn CartReadRepository,
}

#[async_trait::async_trait]
impl cart_binding_process::CustomerIdentifiedHooks for CartBindingHooks<'_> {
    async fn read_open_carts(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<OpenCartsRead>, DomainError> {
        Ok(self
            .carts
            .open_by_session(session_id)
            .await?
            .into_iter()
            .map(|c| OpenCartsRead { cart_id: c.cart_id })
            .collect())
    }
}

/// EVENT leg `events.yaml#/CustomerIdentified` (rules.yaml#/GuestCartsBoundOnIdentification) — the
/// generated pipeline with this module's hooks: bind every OPEN cart of the identified session, then
/// record the session→customer binding row (idempotency for re-delivered triggers).
pub async fn on_customer_identified(
    store: &dyn EventStore,
    state: &dyn CartBindingStateStore,
    carts: &dyn CartReadRepository,
    event: &CustomerIdentified,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    cart_binding_process::on_customer_identified(store, state, &CartBindingHooks { carts }, event, env)
        .await
}

// ================================================================================================
// Behaviour tests (specs/tests.yaml, CartBindingProcess saga).
// ================================================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm_state::mem::MemCartBindingState;
    use crate::process_managers::test_support::{envelope, MemStore};
    use crate::queries::CartRow;
    use async_trait::async_trait;
    use domain::generated::events::{CartStarted, DomainEvent};
    use domain::generated::scalars::*;

    fn uid(n: u128) -> uuid::Uuid {
        uuid::Uuid::from_u128(n)
    }
    fn session_id() -> SessionId {
        SessionId(uid(9))
    }
    fn customer_id() -> CustomerId {
        CustomerId(uid(7))
    }

    fn cart_row(cart: u128) -> CartRow {
        CartRow {
            cart_id: CartId(uid(cart)),
            restaurant_id: RestaurantId(uid(3)),
            session_id: session_id(),
            customer_id: None,
            status: CartStatus::OPEN,
            lines: serde_json::json!([]),
            total_amount_cents: MoneyCents(0),
            currency: CurrencyCode("EUR".into()),
            estimated_breakdown: None,
            uber_comparison: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    /// Fake Cart read port answering `open_by_session` from a fixed list.
    struct FakeCarts {
        open: Vec<CartRow>,
    }

    #[async_trait]
    impl CartReadRepository for FakeCarts {
        async fn by_customer(&self, _customer_id: CustomerId) -> Result<Vec<CartRow>, DomainError> {
            Ok(Vec::new())
        }
        async fn by_id(&self, id: CartId) -> Result<Option<CartRow>, DomainError> {
            Ok(self.open.iter().find(|c| c.cart_id == id).cloned())
        }
        async fn open_by_session(
            &self,
            session_id: SessionId,
        ) -> Result<Vec<CartRow>, DomainError> {
            Ok(self.open.iter().filter(|c| c.session_id == session_id).cloned().collect())
        }
    }

    fn started(cart: u128) -> Vec<DomainEvent> {
        vec![DomainEvent::CartStarted(CartStarted {
            cart_id: CartId(uid(cart)),
            restaurant_id: RestaurantId(uid(3)),
            session_id: session_id(),
            customer_id: None,
        })]
    }

    fn identified() -> CustomerIdentified {
        CustomerIdentified {
            customer_id: customer_id(),
            auth_ref: ExternalReference("auth-supabase-1".into()),
            session_id: session_id(),
        }
    }

    /// tests.yaml#/TestCartBindingOnCustomerIdentified —
    /// rules.yaml#/GuestCartsBoundOnIdentification: every OPEN cart of the session receives the
    /// BindCartToCustomer command (→ CartBoundToCustomer on its stream) and the binding row is
    /// recorded.
    #[tokio::test]
    async fn open_carts_are_bound_and_the_binding_is_recorded() {
        let store = MemStore::default();
        let state = MemCartBindingState::default();
        store.seed(&format!("Cart-{}", uid(21)), started(21));
        store.seed(&format!("Cart-{}", uid(22)), started(22));
        let carts = FakeCarts { open: vec![cart_row(21), cart_row(22)] };

        let outcome =
            on_customer_identified(&store, &state, &carts, &identified(), &envelope()).await.unwrap();
        assert_eq!(outcome, Outcome::Completed);

        for cart in [21u128, 22] {
            let events = store.stream(&format!("Cart-{}", uid(cart)));
            let bound = domain::cart::fold(&events).unwrap();
            assert_eq!(bound.customer_id, Some(customer_id()), "cart {cart} bound");
        }
        let row = state.by_session(session_id()).await.unwrap().unwrap();
        assert_eq!(row.customer_id, customer_id());
    }

    /// rules.yaml#/GuestCartsBoundOnIdentification (idempotency + per-cart rejection corollary): a
    /// re-delivered CustomerIdentified finds the carts already bound — the Cart aggregate rejects the
    /// second bind, the leg logs and skips it, and the streams gain nothing.
    #[tokio::test]
    async fn re_delivery_is_absorbed_by_the_one_time_bind() {
        let store = MemStore::default();
        let state = MemCartBindingState::default();
        store.seed(&format!("Cart-{}", uid(21)), started(21));
        let carts = FakeCarts { open: vec![cart_row(21)] };

        on_customer_identified(&store, &state, &carts, &identified(), &envelope()).await.unwrap();
        let first = store.stream(&format!("Cart-{}", uid(21)));
        // Second delivery: the read model may still report the cart OPEN — the aggregate's one-time
        // bind rejects, the leg completes anyway.
        let outcome =
            on_customer_identified(&store, &state, &carts, &identified(), &envelope()).await.unwrap();
        assert_eq!(outcome, Outcome::Completed);
        assert_eq!(store.stream(&format!("Cart-{}", uid(21))), first);
    }

    /// rules.yaml#/GuestCartsBoundOnIdentification (no-carts corollary): a session with no OPEN carts
    /// still records the binding row and completes.
    #[tokio::test]
    async fn no_open_carts_still_records_the_binding() {
        let store = MemStore::default();
        let state = MemCartBindingState::default();
        let carts = FakeCarts { open: Vec::new() };
        let outcome =
            on_customer_identified(&store, &state, &carts, &identified(), &envelope()).await.unwrap();
        assert_eq!(outcome, Outcome::Completed);
        assert!(state.by_session(session_id()).await.unwrap().is_some());
    }
}
