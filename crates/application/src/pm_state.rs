//! Process-manager STATE persistence ports (ADR-20260719-172821) — the row types and store traits for
//! the four saga state tables (`specs/database/tables/process_managers.yaml`): one row = one saga run,
//! keyed by the run's correlation identity. These tables are PRIVATE to their process manager: no
//! projection reads them and no query serves them — they exist so a run can (a) be idempotent (dedup
//! re-delivered triggers), (b) enforce single-flight on the row, and (c) resume after a crash.
//!
//! The application defines the ports; `infrastructure` implements them over Postgres (dependency rule,
//! ADR-0035). `last_update_utc` is maintained by the RUNTIME ENVELOPE, never by a step: every `upsert`
//! stamps it server-side (`now()`) — the value carried on the row is IGNORED on write and refreshed on
//! the next read. The [`mem`] submodule provides tiny in-memory implementations for orchestrator tests.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::generated::scalars::{
    CartId, CustomerId, DeliveryDispatchProcessStatus, DeliveryJobId, ExternalReference, MoneyCents,
    OrderId, PaymentIntentId, PaymentProcessStatus, PaymentStatus, RefundId, RefundProcessStatus,
    RestaurantId, SessionId,
};
use domain::shared::errors::DomainError;

/// One `payment_process_manager` row: a PlaceOrderProcess checkout run, keyed by cart. The unique
/// `payment_intent_id` correlates inbound Stripe facts back to the run;
/// `last_processed_stripe_event_id` dedups Stripe webhook re-delivery. The
/// `customer_id`/`session_id`/`client_secret` columns back the initiator-scoped `paymentStatus`
/// read (ADR-20260720-015500) — the one declared exception to PM-table privacy.
#[derive(Debug, Clone, PartialEq)]
pub struct PaymentProcessRow {
    pub cart_id: CartId,
    /// Client-generated id of the order the run will materialize on capture.
    pub order_id: OrderId,
    pub payment_intent_id: PaymentIntentId,
    pub process_status: PaymentProcessStatus,
    pub payment_status: PaymentStatus,
    /// Checkout owner (`None` for anonymous) — `paymentStatus` ownership scope.
    pub customer_id: Option<CustomerId>,
    /// Initiating session (`X-SESSION-ID`) — anonymous `paymentStatus` ownership scope.
    pub session_id: Option<SessionId>,
    /// Stripe PaymentIntent client secret, served to the initiator while AWAITING_PAYMENT_RESULT and
    /// NULLed when the run resolves. Never event-sourced (credential, not a business fact).
    pub client_secret: Option<String>,
    /// Dedup key for Stripe webhook re-delivery.
    pub last_processed_stripe_event_id: Option<ExternalReference>,
    /// Maintained by the runtime envelope — ignored on write, stamped `now()` by `upsert`.
    pub last_update_utc: DateTime<Utc>,
}

/// One `refund_process_manager` row: a RefundProcess run, keyed by order. Opened PENDING_APPROVAL by a
/// refundable fact; resolved by the restaurant/admin decision and, on approval, by `PaymentRefunded`.
#[derive(Debug, Clone, PartialEq)]
pub struct RefundProcessRow {
    pub order_id: OrderId,
    /// The captured payment to refund (from the order's payment facts).
    pub payment_intent_id: Option<PaymentIntentId>,
    /// Stripe refund id, set when `PaymentRefunded` settles the run.
    pub refund_id: Option<RefundId>,
    pub process_status: RefundProcessStatus,
    /// Approved amount (may be partial); `None` until approved.
    pub approved_amount_cents: Option<MoneyCents>,
    /// Reason carried by the opening fact / the decision.
    pub reason: Option<String>,
    /// Maintained by the runtime envelope — ignored on write, stamped `now()` by `upsert`.
    pub last_update_utc: DateTime<Utc>,
}

/// One `cart_binding_process_manager` row: records that the visitor's OPEN carts for `session_id` were
/// bound to `customer_id`, so a re-delivered `CustomerIdentified` is a no-op.
#[derive(Debug, Clone, PartialEq)]
pub struct CartBindingRow {
    pub session_id: SessionId,
    pub customer_id: CustomerId,
    /// Maintained by the runtime envelope — ignored on write, stamped `now()` by `upsert`.
    pub last_update_utc: DateTime<Utc>,
}

/// One `delivery_dispatch_process_manager` row: a DeliveryDispatchProcess run per DELIVERY order.
/// `delivery_job_id` is the deterministic UUIDv5 idempotency key of the dispatch (unique).
#[derive(Debug, Clone, PartialEq)]
pub struct DeliveryDispatchRow {
    pub order_id: OrderId,
    /// Kept so the close-order leg can send `MarkOrderDelivered` without re-reading.
    pub restaurant_id: RestaurantId,
    pub delivery_job_id: DeliveryJobId,
    pub process_status: DeliveryDispatchProcessStatus,
    /// TOTAL offers made to the delivery channel (the birth offer = 1). Capped at 3
    /// (rules.yaml#/DispatchRetriesAreBounded, ADR-20260720-004556): the 3rd partner decline closes
    /// the run FAILED instead of re-offering.
    pub offer_attempts: i32,
    /// Maintained by the runtime envelope — ignored on write, stamped `now()` by `upsert`.
    pub last_update_utc: DateTime<Utc>,
}

/// State store for PlaceOrderProcess checkout runs (`payment_process_manager`).
#[async_trait]
pub trait PaymentProcessStateStore: Send + Sync {
    /// The live run for this cart, if any (pk lookup).
    async fn by_cart(&self, cart_id: CartId) -> Result<Option<PaymentProcessRow>, DomainError>;

    /// Correlate an inbound Stripe fact back to its run (UNIQUE `payment_intent_id`).
    async fn by_payment_intent(
        &self,
        payment_intent_id: &PaymentIntentId,
    ) -> Result<Option<PaymentProcessRow>, DomainError>;

    /// The run that will materialize this order — the `paymentStatus(orderId)` read
    /// (ADR-20260720-015500; the caller enforces the initiator ownership scope).
    async fn by_order(&self, order_id: OrderId) -> Result<Option<PaymentProcessRow>, DomainError>;

    /// Insert or replace the run's row; `last_update_utc` is stamped server-side (`now()`).
    async fn upsert(&self, row: &PaymentProcessRow) -> Result<(), DomainError>;
}

/// State store for RefundProcess runs (`refund_process_manager`).
#[async_trait]
pub trait RefundProcessStateStore: Send + Sync {
    /// The live run for this order, if any (pk lookup).
    async fn by_order(&self, order_id: OrderId) -> Result<Option<RefundProcessRow>, DomainError>;

    /// Insert or replace the run's row; `last_update_utc` is stamped server-side (`now()`).
    async fn upsert(&self, row: &RefundProcessRow) -> Result<(), DomainError>;
}

/// State store for CartBindingProcess runs (`cart_binding_process_manager`).
#[async_trait]
pub trait CartBindingStateStore: Send + Sync {
    /// The binding recorded for this session, if any (pk lookup).
    async fn by_session(&self, session_id: SessionId)
        -> Result<Option<CartBindingRow>, DomainError>;

    /// Insert or replace the binding; `last_update_utc` is stamped server-side (`now()`).
    async fn upsert(&self, row: &CartBindingRow) -> Result<(), DomainError>;
}

/// State store for DeliveryDispatchProcess runs (`delivery_dispatch_process_manager`).
#[async_trait]
pub trait DeliveryDispatchStateStore: Send + Sync {
    /// The live run for this order, if any (pk lookup).
    async fn by_order(&self, order_id: OrderId)
        -> Result<Option<DeliveryDispatchRow>, DomainError>;

    /// Correlate a delivery-partner fact back to its run (UNIQUE `delivery_job_id`).
    async fn by_job(
        &self,
        delivery_job_id: DeliveryJobId,
    ) -> Result<Option<DeliveryDispatchRow>, DomainError>;

    /// Insert or replace the run's row; `last_update_utc` is stamped server-side (`now()`).
    async fn upsert(&self, row: &DeliveryDispatchRow) -> Result<(), DomainError>;
}

/// In-memory implementations of the state-store ports (plain `Mutex<HashMap>`), for the process-manager
/// orchestrator tests. They mirror the Postgres semantics: `upsert` replaces the whole row and stamps
/// `last_update_utc = now()` (the row's own value is ignored), reads return the stored row.
pub mod mem {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory [`PaymentProcessStateStore`], keyed by cart.
    #[derive(Default)]
    pub struct MemPaymentProcessState {
        rows: Mutex<HashMap<uuid::Uuid, PaymentProcessRow>>,
    }

    #[async_trait]
    impl PaymentProcessStateStore for MemPaymentProcessState {
        async fn by_cart(
            &self,
            cart_id: CartId,
        ) -> Result<Option<PaymentProcessRow>, DomainError> {
            Ok(self.rows.lock().unwrap().get(&cart_id.0).cloned())
        }

        async fn by_payment_intent(
            &self,
            payment_intent_id: &PaymentIntentId,
        ) -> Result<Option<PaymentProcessRow>, DomainError> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .values()
                .find(|r| &r.payment_intent_id == payment_intent_id)
                .cloned())
        }

        async fn by_order(
            &self,
            order_id: OrderId,
        ) -> Result<Option<PaymentProcessRow>, DomainError> {
            Ok(self.rows.lock().unwrap().values().find(|r| r.order_id == order_id).cloned())
        }

        async fn upsert(&self, row: &PaymentProcessRow) -> Result<(), DomainError> {
            let mut stamped = row.clone();
            stamped.last_update_utc = Utc::now();
            self.rows.lock().unwrap().insert(stamped.cart_id.0, stamped);
            Ok(())
        }
    }

    /// In-memory [`RefundProcessStateStore`], keyed by order.
    #[derive(Default)]
    pub struct MemRefundProcessState {
        rows: Mutex<HashMap<uuid::Uuid, RefundProcessRow>>,
    }

    #[async_trait]
    impl RefundProcessStateStore for MemRefundProcessState {
        async fn by_order(
            &self,
            order_id: OrderId,
        ) -> Result<Option<RefundProcessRow>, DomainError> {
            Ok(self.rows.lock().unwrap().get(&order_id.0).cloned())
        }

        async fn upsert(&self, row: &RefundProcessRow) -> Result<(), DomainError> {
            let mut stamped = row.clone();
            stamped.last_update_utc = Utc::now();
            self.rows.lock().unwrap().insert(stamped.order_id.0, stamped);
            Ok(())
        }
    }

    /// In-memory [`CartBindingStateStore`], keyed by session.
    #[derive(Default)]
    pub struct MemCartBindingState {
        rows: Mutex<HashMap<uuid::Uuid, CartBindingRow>>,
    }

    #[async_trait]
    impl CartBindingStateStore for MemCartBindingState {
        async fn by_session(
            &self,
            session_id: SessionId,
        ) -> Result<Option<CartBindingRow>, DomainError> {
            Ok(self.rows.lock().unwrap().get(&session_id.0).cloned())
        }

        async fn upsert(&self, row: &CartBindingRow) -> Result<(), DomainError> {
            let mut stamped = row.clone();
            stamped.last_update_utc = Utc::now();
            self.rows.lock().unwrap().insert(stamped.session_id.0, stamped);
            Ok(())
        }
    }

    /// In-memory [`DeliveryDispatchStateStore`], keyed by order.
    #[derive(Default)]
    pub struct MemDeliveryDispatchState {
        rows: Mutex<HashMap<uuid::Uuid, DeliveryDispatchRow>>,
    }

    #[async_trait]
    impl DeliveryDispatchStateStore for MemDeliveryDispatchState {
        async fn by_order(
            &self,
            order_id: OrderId,
        ) -> Result<Option<DeliveryDispatchRow>, DomainError> {
            Ok(self.rows.lock().unwrap().get(&order_id.0).cloned())
        }

        async fn by_job(
            &self,
            delivery_job_id: DeliveryJobId,
        ) -> Result<Option<DeliveryDispatchRow>, DomainError> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .values()
                .find(|r| r.delivery_job_id == delivery_job_id)
                .cloned())
        }

        async fn upsert(&self, row: &DeliveryDispatchRow) -> Result<(), DomainError> {
            let mut stamped = row.clone();
            stamped.last_update_utc = Utc::now();
            self.rows.lock().unwrap().insert(stamped.order_id.0, stamped);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mem::*;
    use super::*;

    fn payment_row(cart: uuid::Uuid, intent: &str) -> PaymentProcessRow {
        PaymentProcessRow {
            cart_id: CartId(cart),
            order_id: OrderId(uuid::Uuid::new_v4()),
            payment_intent_id: PaymentIntentId(intent.into()),
            process_status: PaymentProcessStatus::AWAITING_PAYMENT_RESULT,
            payment_status: PaymentStatus::PENDING,
            customer_id: None,
            session_id: None,
            client_secret: Some("pi_secret".into()),
            last_processed_stripe_event_id: None,
            last_update_utc: DateTime::<Utc>::MIN_UTC,
        }
    }

    #[tokio::test]
    async fn mem_payment_store_upserts_and_correlates_by_intent() {
        let store = MemPaymentProcessState::default();
        let cart = uuid::Uuid::new_v4();
        let row = payment_row(cart, "pi_1");
        store.upsert(&row).await.unwrap();

        let by_cart = store.by_cart(CartId(cart)).await.unwrap().unwrap();
        assert_eq!(by_cart.payment_intent_id.0, "pi_1");
        // The envelope stamped last_update_utc server-side (the row's own value was ignored).
        assert!(by_cart.last_update_utc > DateTime::<Utc>::MIN_UTC);

        let by_intent = store
            .by_payment_intent(&PaymentIntentId("pi_1".into()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(by_intent.cart_id.0, cart);
        assert!(store.by_cart(CartId(uuid::Uuid::new_v4())).await.unwrap().is_none());

        // Upsert replaces the whole row (same pk).
        let mut second = payment_row(cart, "pi_1");
        second.process_status = PaymentProcessStatus::ORDER_PLACED;
        second.payment_status = PaymentStatus::CAPTURED;
        store.upsert(&second).await.unwrap();
        let replaced = store.by_cart(CartId(cart)).await.unwrap().unwrap();
        assert_eq!(replaced.process_status, PaymentProcessStatus::ORDER_PLACED);
    }

    #[tokio::test]
    async fn mem_dispatch_store_finds_by_order_and_job() {
        let store = MemDeliveryDispatchState::default();
        let order = uuid::Uuid::new_v4();
        let job = uuid::Uuid::new_v4();
        let row = DeliveryDispatchRow {
            order_id: OrderId(order),
            restaurant_id: RestaurantId(uuid::Uuid::new_v4()),
            delivery_job_id: DeliveryJobId(job),
            process_status: DeliveryDispatchProcessStatus::OFFERED,
            offer_attempts: 1,
            last_update_utc: DateTime::<Utc>::MIN_UTC,
        };
        store.upsert(&row).await.unwrap();
        assert!(store.by_order(OrderId(order)).await.unwrap().is_some());
        let by_job = store.by_job(DeliveryJobId(job)).await.unwrap().unwrap();
        assert_eq!(by_job.order_id.0, order);
        assert!(store.by_job(DeliveryJobId(uuid::Uuid::new_v4())).await.unwrap().is_none());
    }
}
