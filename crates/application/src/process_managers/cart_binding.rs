//! CartBindingProcess (`specs/processmanager.yaml#/CartBindingProcess`) — binds a returning
//! visitor's OPEN guest carts to their Customer when `CustomerIdentified` arrives (authRef →
//! customerId, ADR-20260719-193500 §6): read the session's OPEN carts from the Cart read model,
//! SEND `commands.yaml#/BindCartToCustomer` per cart — the Cart aggregate validates the one-time
//! bind and owns the `CartBoundToCustomer` fact (its `customer_id` folds from the same stream,
//! deleting the impossible cross-stream projector routing) — then record the binding on the
//! `cart_binding_process_manager` row so a re-delivered `CustomerIdentified` is a no-op.
//!
//! Send semantics on an EVENT leg: a rejection by the Cart (e.g. already bound) is LOGGED and
//! SKIPPED for that cart, never re-thrown — the remaining carts still bind.

use domain::generated::commands::BindCartToCustomer;
use domain::generated::events::CustomerIdentified;
use domain::shared::errors::DomainError;

use crate::pm_state::{CartBindingRow, CartBindingStateStore};
use crate::ports::EventStore;
use crate::process_managers::{saga_actor, Outcome, TriggerEnvelope};
use crate::queries::CartReadRepository;

/// EVENT leg `events.yaml#/CustomerIdentified` (rules.yaml#/GuestCartsBoundOnIdentification): bind
/// every OPEN cart of the identified session to the customer, then record the session→customer
/// binding on the state row (idempotency for re-delivered triggers).
pub async fn on_customer_identified(
    store: &dyn EventStore,
    state: &dyn CartBindingStateStore,
    carts: &dyn CartReadRepository,
    event: &CustomerIdentified,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    // read Cart where session_id = <trigger> AND status = OPEN.
    let open_carts = carts.open_by_session(event.session_id).await?;
    let actor = saga_actor(env);
    // send BindCartToCustomer for_each open cart — the Cart aggregate validates (one-time bind) and
    // emits CartBoundToCustomer; a per-cart rejection is logged and skipped (DSL note), a plumbing
    // failure (repository/version conflict) propagates so the runner retries the whole leg.
    for cart in &open_carts {
        let cmd = BindCartToCustomer { cart_id: cart.cart_id, customer_id: event.customer_id };
        match crate::commands::bind_cart_to_customer(store, cmd, &actor).await {
            Ok(()) => {}
            Err(DomainError::Rejected { code, context }) => {
                eprintln!(
                    "saga[CartBindingProcess]: BindCartToCustomer rejected for cart {} ({code}: {context}) — skipped",
                    cart.cart_id.0
                );
            }
            Err(other) => return Err(other),
        }
    }
    // state.set — the session's carts are bound; a re-delivered CustomerIdentified is a no-op
    // (the Cart aggregate's one-time bind is the second idempotency line).
    state
        .upsert(&CartBindingRow {
            session_id: event.session_id,
            customer_id: event.customer_id,
            last_update_utc: chrono::Utc::now(), // ignored on write; stamped by the store
        })
        .await?;
    Ok(Outcome::Completed)
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
