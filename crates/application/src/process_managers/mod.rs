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
pub mod payment_settlement;
pub mod place_order;
pub mod reclamation;
pub mod refund;

use domain::generated::scalars::{CartId, DeliveryJobId, OrderId, RestaurantId};

use crate::ports::Actor;

/// The trigger's ENVELOPE bits an event leg may reference (`from_envelope`, ADR-0041): the
/// `domain_events` row's id (dedup keys, `cause_id`), its correlation and its occurrence time.
/// Infrastructure metadata — never business payload.
#[derive(Debug, Clone)]
pub struct TriggerEnvelope {
    /// `domain_events.id` of the trigger — `from_envelope: event_id`; also the `cause_id` stamped on
    /// everything the reaction delivers/sends.
    pub event_id: uuid::Uuid,
    /// `domain_events.correlation_id` of the trigger, propagated onto the reaction's appends.
    pub correlation_id: uuid::Uuid,
    /// `domain_events.occurred_at` of the trigger — `from_envelope: occurred_at`.
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    /// Where a ROUTED `deliver:` step stages its lane enqueue (ADR-20260816-040239). It lives on
    /// the envelope rather than in a leg parameter because it is a property of the INVOCATION
    /// ROUTE, not of the saga: a mailbox delivery owns a fenced transaction to stage into, the
    /// polling `ProcessManagerRunner` owns none.
    ///
    /// `None` — the DEFAULT, and the only value on any route that cannot stage — means the routed
    /// branch is off and the legacy foreign-stream append runs unchanged. That is the
    /// gate-then-stabilize rollback path: `configuration.yaml#/ROUTE_ORDER_BIRTH_THROUGH_LANE`
    /// resolves to `None` here when OFF, so flipping it back is a config flip, never a redeploy.
    ///
    /// **PRIVATE on purpose (#597)** — ADR-20260816-040239's constraint 1 ("the enqueue is never in
    /// `prepare`") used to hold by structural reading alone, guarded by nothing: any construction
    /// site could write `lanes: Some(...)`, including one in `prepare`, which
    /// `actor_runtime::completion` re-runs with NO transaction open — a birth message stranded
    /// outside the delivery's commit. There is now no field write to make: [`Self::unlaned`] and
    /// [`Self::laned`] are the only ways to build an envelope, and attaching a sink means NAMING
    /// `laned` at a single greppable, documented seam. Compiler first, a check is the fallback
    /// (ADR-20260803-234035).
    lanes: Option<std::sync::Arc<dyn crate::lanes::LaneSink>>,
}

/// Hand-written because the lane sink is a trait object with no meaningful identity: two
/// envelopes are the same TRIGGER when their ids and instant match. The sink is delivery-route
/// plumbing, not part of what the envelope IS.
impl PartialEq for TriggerEnvelope {
    fn eq(&self, other: &Self) -> bool {
        self.event_id == other.event_id
            && self.correlation_id == other.correlation_id
            && self.occurred_at == other.occurred_at
    }
}

impl TriggerEnvelope {
    /// The envelope of a trigger delivered on a route with NO lane sink (the polling runner, unit
    /// tests): routed `deliver:` steps fall back to the legacy append.
    ///
    /// The DEFAULT shape, and the only one a route that owns no transaction can build.
    pub fn unlaned(
        event_id: uuid::Uuid,
        correlation_id: uuid::Uuid,
        occurred_at: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        Self { event_id, correlation_id, occurred_at, lanes: None }
    }

    /// The envelope of a trigger delivered on a route that OWNS THE DELIVERY TRANSACTION the sink's
    /// staged enqueues will be flushed into (`infrastructure::mailbox::handler::handle_pm_fact`).
    ///
    /// Calling this is a claim about the CALLER, and the claim is exactly ADR-20260816-040239's
    /// constraint 1: *I hold a fenced transaction, and whatever this sink buffers I will flush into
    /// it before I commit.* A phase that cannot make that claim — `prepare`, which
    /// `actor_runtime::completion` re-runs with no transaction and throws away — must call
    /// [`Self::unlaned`]; there is no third option, because the field is private (#597).
    ///
    /// What the compiler carries here is that a sink cannot be attached by an anonymous field
    /// write, so a lane enqueue can only appear on a route that names this constructor. What it
    /// cannot carry is the claim itself: `application` cannot name a Postgres transaction without
    /// inverting the dependency rule, so there is no value for `laned` to demand as proof. The
    /// single call site is the review surface, and this doc is its contract.
    pub fn laned(
        event_id: uuid::Uuid,
        correlation_id: uuid::Uuid,
        occurred_at: chrono::DateTime<chrono::Utc>,
        lanes: std::sync::Arc<dyn crate::lanes::LaneSink>,
    ) -> Self {
        Self { event_id, correlation_id, occurred_at, lanes: Some(lanes) }
    }

    /// The lane sink a ROUTED `deliver:` step stages into, or `None` on a route that cannot stage.
    /// Crate-internal: the generated step pipeline is the only reader, and the only writers are the
    /// two constructors above.
    pub(crate) fn lane_sink(&self) -> Option<&std::sync::Arc<dyn crate::lanes::LaneSink>> {
        self.lanes.as_ref()
    }
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

/// `UserType::EXTERNAL` TEXT value (stored verbatim, ADR-20260728): the envelope principal for
/// non-human system appends — the same convention the SIRENE/Stripe ACLs use (scalars.yaml has no
/// dedicated SYSTEM member; adding one would be a DSL change).
pub const EXTERNAL_USER_TYPE: &str = "EXTERNAL";

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
        user_type: EXTERNAL_USER_TYPE.to_string(),
        domain_id: None,
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

        /// Every stream's current length — the generated behaviour suite's diff baseline.
        pub fn lengths(&self) -> std::collections::HashMap<String, usize> {
            self.streams.lock().unwrap().iter().map(|(k, v)| (k.clone(), v.len())).collect()
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

    /// A canned trigger envelope (fixed ids so tests can assert the dedup key). NO lane sink: the
    /// in-memory bed has no transaction to stage into, so routed `deliver:` steps take the legacy
    /// append branch and the generated behaviour tests keep asserting on the stream.
    pub fn envelope() -> TriggerEnvelope {
        TriggerEnvelope::unlaned(
            uuid::Uuid::from_u128(0xEE),
            uuid::Uuid::from_u128(0xCC),
            chrono::Utc::now(),
        )
    }
}
