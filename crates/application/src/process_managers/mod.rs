//! Process managers / sagas (ADR-0035, actors.yaml `type: process-manager`) — the event-driven side of
//! the write model. An aggregate receives COMMANDS; a process manager RECEIVES EVENTS (its actors.yaml
//! inbox) and reacts by emitting the declared `emits` events onto other aggregates' streams (and, where
//! modelled, by requesting external effects through ports).
//!
//! Shape mirrors the read-model projectors (`projectors/*`): each PM is a set of PURE decision
//! functions — `(trigger event, pre-loaded stream states) -> Decision` — with no I/O, so the behaviour
//! tests (tests.yaml) assert them directly. The I/O half lives in `infrastructure::process_manager::
//! ProcessManagerRunner`: it polls `domain_events` past a per-PM checkpoint (like the projection
//! worker), loads the streams a decision needs through the `EventStore` port, executes the returned
//! appends, and advances the checkpoint.
//!
//! Idempotency: every decision first folds the TARGET stream and returns [`Decision::Nothing`] when the
//! reaction's fact is already recorded (e.g. the Order stream already holds `OrderPlaced`, the
//! DeliveryJob stream already holds `DeliveryRequested`). Emitted appends carry the loaded
//! `expected_version`, so a concurrent writer conflicts (UNIQUE(stream_name, version)) instead of
//! double-applying; the runner retries the whole reaction next tick, where the fold then sees the fact.
//! Where a stream must be BORN by the reaction, the aggregate id is DETERMINISTIC (client-generated
//! order/cart ids from the checkout; a UUIDv5 of the order id for the delivery job), so a re-reaction
//! targets the same stream and is absorbed.

pub mod cart_binding;
pub mod delivery_dispatch;
pub mod place_order;
pub mod refund;

use domain::generated::events::DomainEvent;
use domain::generated::scalars::{CartId, DeliveryJobId, OrderId, RestaurantId};

/// One append a process-manager reaction wants executed: `events` onto `stream_name`, expecting the
/// stream at `expected_version` (0 = the reaction births the stream). The runner executes it through
/// the `EventStore` port under the saga's system actor (trigger-correlated envelope, ADR-0041).
#[derive(Debug, Clone, PartialEq)]
pub struct StreamAppend {
    pub stream_name: String,
    pub expected_version: i64,
    pub events: Vec<DomainEvent>,
}

/// What a process manager decided for one trigger event.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// React: execute these appends (atomically per stream; the whole decision re-runs on a version
    /// conflict).
    Act(Vec<StreamAppend>),
    /// The PM wanted to react but a required input or external effect is not available yet — the
    /// runner LOGS the precise reason and advances (the trigger stays in `domain_events` for a future
    /// full re-run once the gap closes).
    Skip(String),
    /// Nothing to do — by design (e.g. a COLLECTION order needs no dispatch) or because the reaction's
    /// fact is already recorded (idempotent re-reaction).
    Nothing,
}

impl Decision {
    /// The appends this decision carries (empty for Skip/Nothing) — test helper.
    pub fn appends(&self) -> &[StreamAppend] {
        match self {
            Decision::Act(appends) => appends,
            _ => &[],
        }
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
