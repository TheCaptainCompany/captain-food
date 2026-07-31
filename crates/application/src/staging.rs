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
