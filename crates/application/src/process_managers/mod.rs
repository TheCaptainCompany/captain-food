//! Process managers / sagas — STATE-TABLE ORCHESTRATORS executing their `specs/processmanager.yaml`
//! legs (ADR-20260719-172821 the typed step DSL, ADR-20260719-193500 this runtime). A process manager
//! is NOT an event-sourced actor: it reacts to a message (a command it handles, or an event already
//! recorded in the log), runs the DSL's ORDERED TYPED STEPS (read a read model, guard, call an
//! outbound port, deliver an event / send a command to an aggregate, read/update its own state row),
//! and keeps its private state in its declared `*_process_manager` table (`crate::pm_state`).
//! AGGREGATES OWN THE FACTS — a PM never invents a stream of its own; it delivers events for the
//! owning aggregate to record (through the write-side [`crate::repository::Repository`]) or sends
//! commands the aggregate may reject.
//!
//! Guard semantics (the DSL contract):
//! - COMMAND legs (`approve_refund`/`deny_refund`, and `commands::place_order` for the checkout leg)
//!   REJECT with the typed `errors.yaml` error — ordinary `Result<(), DomainError>` handlers.
//! - EVENT legs return `Result<Outcome, DomainError>`: `Err` is a THROWN guard (the recorded fact
//!   stands — facts are never rejected — but the run aborts and the runner SURFACES the typed error);
//!   `Ok(Outcome::Skipped)` is a BENIGN alternative (idempotent re-delivery, COLLECTION no-op,
//!   failed `state.expect`) the runner merely logs.
//!
//! The RUNTIME ENVELOPE (single-flight per row, checkpointing, correlation/cause propagation,
//! poison skip, `last_update_utc` stamping) is the runner's contract
//! (`infrastructure::process_manager::ProcessManagerRunner`), never a business step.

pub mod cart_binding;
pub mod delivery_dispatch;
pub mod place_order;
pub mod refund;

use domain::generated::scalars::{CartId, DeliveryJobId, OrderId, RestaurantId};

use crate::ports::Actor;

/// The trigger's ENVELOPE bits an event leg may reference (`from_envelope`, ADR-0041): the
/// `domain_events` row's id (dedup keys, `cause_id`), its correlation and its occurrence time.
/// Infrastructure metadata — never business payload.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerEnvelope {
    /// `domain_events.id` of the trigger — `from_envelope: event_id`; also the `cause_id` stamped on
    /// everything the reaction delivers/sends.
    pub event_id: uuid::Uuid,
    /// `domain_events.correlation_id` of the trigger, propagated onto the reaction's appends.
    pub correlation_id: uuid::Uuid,
    /// `domain_events.occurred_at` of the trigger — `from_envelope: occurred_at`.
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}

/// How an EVENT leg ended when it did NOT throw: the run either executed its steps to the end, or hit
/// a benign `skip` guard / failed `state.expect` (a no-op the runner logs, never an error).
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Every step ran; the state row reflects the leg's final `state.set`.
    Completed,
    /// A benign expected alternative ended the run as a no-op (the reason is for the runner's log).
    Skipped(String),
}

/// `UserType::EXTERNAL` ordinal (declaration-order ints, ADR-0037): the envelope principal for
/// non-human system appends — the same convention the SIRENE/Stripe ACLs use (scalars.yaml has no
/// dedicated SYSTEM member; adding one would be a DSL change).
pub const EXTERNAL_USER_TYPE: i32 = 6;

/// Fixed system user id stamping saga-emitted events (`domain_events.user_id`, ADR-0041).
pub fn saga_system_user_id() -> uuid::Uuid {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"https://captain.food/process-managers")
}

/// The saga's acting identity for one trigger (ADR-0041): the fixed system user under the EXTERNAL
/// user type, correlated with the trigger and caused by the trigger event — the SAME envelope
/// contract the runner has always stamped on saga appends, now built by the orchestrators themselves
/// so every `deliver`/`send` carries it regardless of which shell drives the leg.
pub fn saga_actor(env: &TriggerEnvelope) -> Actor {
    Actor {
        user_id: saga_system_user_id(),
        user_type: EXTERNAL_USER_TYPE,
        correlation_id: env.correlation_id,
        cause_id: Some(env.event_id),
    }
}

/// The stream an Order aggregate lives on (same convention as the command handlers / projection worker).
pub fn order_stream(id: &OrderId) -> String {
    format!("Order-{}", id.0)
}

/// The stream a Cart aggregate lives on.
pub fn cart_stream(id: &CartId) -> String {
    format!("Cart-{}", id.0)
}

/// The stream a DeliveryJob aggregate lives on.
pub fn delivery_job_stream(id: &DeliveryJobId) -> String {
    format!("DeliveryJob-{}", id.0)
}

/// The stream a Restaurant aggregate lives on.
pub fn restaurant_stream(id: &RestaurantId) -> String {
    format!("Restaurant-{}", id.0)
}

/// Shared test doubles for the orchestrator behaviour tests (tests.yaml): an in-memory [`EventStore`]
/// with `PgEventStore` optimistic-concurrency semantics, plus a canned [`TriggerEnvelope`].
#[cfg(test)]
pub(crate) mod test_support {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use domain::generated::events::DomainEvent;
    use domain::shared::errors::DomainError;

    use crate::ports::{version_conflict, Actor, EventStore};

    use super::TriggerEnvelope;

    /// In-memory [`EventStore`]: version = number of events on the stream; a clash → the canonical
    /// [`version_conflict`] (same semantics as `PgEventStore`).
    #[derive(Default)]
    pub struct MemStore {
        streams: Mutex<HashMap<String, Vec<DomainEvent>>>,
    }

    impl MemStore {
        /// GIVEN: pre-seed a stream with already-recorded facts.
        pub fn seed(&self, stream: &str, events: Vec<DomainEvent>) {
            self.streams.lock().unwrap().insert(stream.to_string(), events);
        }

        /// THEN: the full stream after the reaction ran.
        pub fn stream(&self, stream: &str) -> Vec<DomainEvent> {
            self.streams.lock().unwrap().get(stream).cloned().unwrap_or_default()
        }
    }

    #[async_trait::async_trait]
    impl EventStore for MemStore {
        async fn append(
            &self,
            stream_name: &str,
            expected_version: i64,
            events: &[DomainEvent],
            _actor: &Actor,
        ) -> Result<i64, DomainError> {
            let mut streams = self.streams.lock().unwrap();
            let stream = streams.entry(stream_name.to_string()).or_default();
            if stream.len() as i64 != expected_version {
                return Err(version_conflict(stream_name, expected_version));
            }
            stream.extend(events.iter().cloned());
            Ok(stream.len() as i64)
        }

        async fn load(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError> {
            let events = self.stream(stream_name);
            let version = events.len() as i64;
            Ok((events, version))
        }
    }

    /// A canned trigger envelope (fixed ids so tests can assert the dedup key).
    pub fn envelope() -> TriggerEnvelope {
        TriggerEnvelope {
            event_id: uuid::Uuid::from_u128(0xEE),
            correlation_id: uuid::Uuid::from_u128(0xCC),
            occurred_at: chrono::Utc::now(),
        }
    }
}
