//! The STAGING write-side store (#242 Runtime C, PROP-20260728-152752 §3.5 "the actor STAGES its
//! events; nothing becomes true before the commit"): an [`EventStore`] whose `append` BUFFERS
//! instead of writing. The mailbox delivery glue hands command handlers a `StagingEventStore`,
//! runs them unchanged, then flushes the staged appends INTO the fenced completion transaction —
//! so the domain events, the mailbox row's terminal flip and the checkpoint advance commit (or
//! roll back) as one, which is the four-effect contract.
//!
//! Loads pass through to the real store, OVERLAID with whatever is already staged for the stream
//! (read-your-writes within one delivery). Optimistic concurrency is NOT weakened: the staged
//! `expected_version` is asserted by the flush's `UNIQUE (stream_name, version)` inserts at commit
//! time, exactly where the pool-backed store asserts it.

use std::sync::Arc;

use async_trait::async_trait;
use domain::generated::events::DomainEvent;
use domain::shared::errors::DomainError;

use crate::pm_state::{
    PaymentProcessRow, PaymentProcessStateStore, RefundProcessRow, RefundProcessStateStore,
};
use crate::ports::{Actor, EventStore};

/// One buffered `append` call, replayed verbatim by the flush.
#[derive(Debug, Clone)]
pub struct StagedAppend {
    pub stream_name: String,
    pub expected_version: i64,
    pub events: Vec<DomainEvent>,
    pub actor: Actor,
}

/// An [`EventStore`] that stages appends in memory. One instance per delivery — never shared
/// across messages (the buffer IS the delivery's uncommitted truth).
pub struct StagingEventStore {
    inner: Arc<dyn EventStore>,
    staged: std::sync::Mutex<Vec<StagedAppend>>,
}

impl StagingEventStore {
    pub fn new(inner: Arc<dyn EventStore>) -> Self {
        Self { inner, staged: std::sync::Mutex::new(Vec::new()) }
    }

    /// Drain the buffer for the flush (called once, after the handler returned Ok).
    pub fn take_staged(&self) -> Vec<StagedAppend> {
        std::mem::take(&mut self.staged.lock().expect("staging buffer poisoned"))
    }
}

#[async_trait]
impl EventStore for StagingEventStore {
    async fn append(
        &self,
        stream_name: &str,
        expected_version: i64,
        events: &[DomainEvent],
        actor: &Actor,
    ) -> Result<i64, DomainError> {
        let mut staged = self.staged.lock().expect("staging buffer poisoned");
        staged.push(StagedAppend {
            stream_name: stream_name.to_owned(),
            expected_version,
            events: events.to_vec(),
            actor: actor.clone(),
        });
        Ok(expected_version + events.len() as i64)
    }

    async fn load(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError> {
        let (mut events, mut version) = self.inner.load(stream_name).await?;
        // Overlay the staged appends for this stream, in staging order (read-your-writes).
        let staged = self.staged.lock().expect("staging buffer poisoned").clone();
        for s in staged.iter().filter(|s| s.stream_name == stream_name) {
            version = version.max(s.expected_version + s.events.len() as i64);
            events.extend(s.events.iter().cloned());
        }
        Ok((events, version))
    }
}

// ================================================================================================
// Staging PM-state stores (#272 Runtime D1, ADR-20260801-023000): the PM command handlers run
// UNCHANGED in the delivery's PREPARE phase — their `upsert`s buffer here instead of writing, and
// the mailbox glue flushes the buffered rows INTO the fenced completion transaction, so the PM
// row commits (or rolls back) atomically with the staged events and the row's verdict.
// Reads pass through to the pool-backed store, OVERLAID with the buffered row (read-your-writes
// within one delivery), mirroring `StagingEventStore` exactly.
// ================================================================================================

/// A [`PaymentProcessStateStore`] that stages upserts in memory. One instance per delivery.
pub struct StagingPaymentProcessState {
    inner: Arc<dyn PaymentProcessStateStore>,
    staged: std::sync::Mutex<Vec<PaymentProcessRow>>,
}

impl StagingPaymentProcessState {
    pub fn new(inner: Arc<dyn PaymentProcessStateStore>) -> Self {
        Self { inner, staged: std::sync::Mutex::new(Vec::new()) }
    }

    /// Drain the buffered rows for the in-tx flush (called once, after the handler returned).
    pub fn take_staged(&self) -> Vec<PaymentProcessRow> {
        std::mem::take(&mut self.staged.lock().expect("staging buffer poisoned"))
    }

    fn last_staged(&self, pick: impl Fn(&PaymentProcessRow) -> bool) -> Option<PaymentProcessRow> {
        self.staged
            .lock()
            .expect("staging buffer poisoned")
            .iter()
            .rev()
            .find(|r| pick(r))
            .cloned()
    }
}

#[async_trait]
impl PaymentProcessStateStore for StagingPaymentProcessState {
    async fn by_cart(
        &self,
        cart_id: domain::generated::scalars::CartId,
    ) -> Result<Option<PaymentProcessRow>, DomainError> {
        if let Some(row) = self.last_staged(|r| r.cart_id == cart_id) {
            return Ok(Some(row));
        }
        self.inner.by_cart(cart_id).await
    }

    async fn by_payment_intent(
        &self,
        payment_intent_id: &domain::generated::scalars::PaymentIntentId,
    ) -> Result<Option<PaymentProcessRow>, DomainError> {
        if let Some(row) = self.last_staged(|r| &r.payment_intent_id == payment_intent_id) {
            return Ok(Some(row));
        }
        self.inner.by_payment_intent(payment_intent_id).await
    }

    async fn by_order(
        &self,
        order_id: domain::generated::scalars::OrderId,
    ) -> Result<Option<PaymentProcessRow>, DomainError> {
        if let Some(row) = self.last_staged(|r| r.order_id == order_id) {
            return Ok(Some(row));
        }
        self.inner.by_order(order_id).await
    }

    async fn upsert(&self, row: &PaymentProcessRow) -> Result<(), DomainError> {
        self.staged.lock().expect("staging buffer poisoned").push(row.clone());
        Ok(())
    }
}

/// A [`RefundProcessStateStore`] that stages upserts in memory. One instance per delivery.
pub struct StagingRefundProcessState {
    inner: Arc<dyn RefundProcessStateStore>,
    staged: std::sync::Mutex<Vec<RefundProcessRow>>,
}

impl StagingRefundProcessState {
    pub fn new(inner: Arc<dyn RefundProcessStateStore>) -> Self {
        Self { inner, staged: std::sync::Mutex::new(Vec::new()) }
    }

    /// Drain the buffered rows for the in-tx flush (called once, after the handler returned).
    pub fn take_staged(&self) -> Vec<RefundProcessRow> {
        std::mem::take(&mut self.staged.lock().expect("staging buffer poisoned"))
    }
}

#[async_trait]
impl RefundProcessStateStore for StagingRefundProcessState {
    async fn by_order(
        &self,
        order_id: domain::generated::scalars::OrderId,
    ) -> Result<Option<RefundProcessRow>, DomainError> {
        let staged = self
            .staged
            .lock()
            .expect("staging buffer poisoned")
            .iter()
            .rev()
            .find(|r| r.order_id == order_id)
            .cloned();
        if let Some(row) = staged {
            return Ok(Some(row));
        }
        self.inner.by_order(order_id).await
    }

    async fn upsert(&self, row: &RefundProcessRow) -> Result<(), DomainError> {
        self.staged.lock().expect("staging buffer poisoned").push(row.clone());
        Ok(())
    }
}
