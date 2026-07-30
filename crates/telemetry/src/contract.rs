//! The span, attribute and metric NAMES declared by `specs/observability.yaml`, as constants.
//!
//! Why constants rather than string literals at each call site: the contracts are the spec, and a typo
//! in a span name is invisible at runtime — the span is still emitted, it just no longer satisfies the
//! contract that names it, so the observability-agent reports a missing required span and the operator
//! concludes instrumentation is broken when in fact one letter is wrong. Naming them once here means the
//! conformance test in `tests/contract_conformance.rs` can read `specs/observability.yaml` and assert
//! that every required span and attribute of the `command-acceptance` and `place-order` contracts has a
//! constant, and vice versa.
//!
//! This module is data, not behaviour: adding a span here does not emit it.

/// Span names. `command.*` come from the `command-acceptance` contract (which binds the WHOLE GraphQL
/// dispatch surface, `surface: graphql`); the rest are the `place-order` deep-dive contract.
pub mod span {
    /// SERVER — the synchronous acceptance entry point (one per mutation resolver).
    pub const COMMAND_RECEIVE: &str = "command.receive";
    /// INTERNAL — the `command_journal` insert: durable RECEIVED, or duplicate/conflict.
    pub const COMMAND_JOURNAL: &str = "command.journal";
    /// INTERNAL — handing the command to the async handler (or declining to, on a duplicate).
    pub const COMMAND_DISPATCH: &str = "command.dispatch";
    /// INTERNAL — invariant validation inside the handler's boundary wrapper (never in the aggregate).
    pub const COMMAND_VALIDATE: &str = "command.validate";
    /// INTERNAL — loading the cart aggregate for repricing.
    pub const CART_READ: &str = "cart.read";
    /// INTERNAL — the 3-way split computation (ADR-0017).
    pub const PRICING_COMPUTE: &str = "pricing.compute";
    /// CLIENT — the outbound Stripe PaymentIntent call, the riskiest leg of checkout.
    pub const PAYMENT_INTENT_CREATE: &str = "payment.intent.create";
    /// INTERNAL — appending to `domain_events`.
    pub const EVENT_STORE_APPEND: &str = "event.store.append";
    /// PRODUCER — publishing an appended event onto the bus.
    pub const EVENT_PUBLISH: &str = "event.publish";
    /// CONSUMER — a projector applying an event to a read model.
    pub const EVENT_CONSUME_PROJECTION: &str = "event.consume.projection";
}

/// Attribute keys. All business context is `business.*` per `docs/claude/observability.md`; the two
/// `messaging.system` keys are OTel semantic-convention names the contracts ask for verbatim.
pub mod attr {
    pub const COMMAND_TYPE: &str = "business.command_type";
    pub const ACTOR: &str = "business.actor";
    pub const CHANNEL: &str = "business.channel";
    pub const MESSAGE_ID: &str = "business.message_id";
    pub const JOURNAL_STATUS: &str = "business.journal_status";
    pub const DISPATCH_OUTCOME: &str = "business.dispatch_outcome";
    pub const VALIDATION_STATUS: &str = "business.validation_status";
    pub const AGGREGATE_ID: &str = "business.aggregate_id";
    pub const SERVICE_FEE: &str = "business.service_fee";
    pub const SPLIT_OK: &str = "business.split_ok";
    pub const RESULT: &str = "business.result";
    pub const EVENT_TYPE: &str = "business.event_type";
    pub const STREAM_ID: &str = "business.stream_id";
    pub const PROJECTION_NAME: &str = "business.projection_name";
    pub const MESSAGING_SYSTEM: &str = "messaging.system";

    /// `run_identity` keys — mandatory in EVERY contract, which is precisely what did not exist before
    /// issue #191. `correlation_id` is business-facing and survives the whole causality chain;
    /// `trace_id` is technical and may rotate across async boundaries, so both are recorded.
    pub const CORRELATION_ID: &str = "business.correlation_id";
    pub const ORDER_ID: &str = "business.order_id";
}

/// Metric names, split exactly as the contracts split them: `metrics` are technical, `business_metrics`
/// feed BAM. Keeping the two apart is a contract requirement, not a style choice.
pub mod metric {
    pub const COMMANDS_ACCEPTED_TOTAL: &str = "commands_accepted_total";
    pub const COMMAND_DUPLICATES_TOTAL: &str = "command_duplicates_total";
    pub const COMMAND_SYNC_CONFLICTS_TOTAL: &str = "command_sync_conflicts_total";
    pub const COMMAND_COMPLETION_MS: &str = "command_completion_ms";
    pub const PLACE_ORDER_DURATION_MS: &str = "place_order_duration_ms";
    pub const ORDERS_PLACED_TOTAL: &str = "orders_placed_total";
    pub const CHECKOUT_PAYMENT_FAILURES_TOTAL: &str = "checkout_payment_failures_total";
}

/// Values for `business.journal_status` — the contract comments them as
/// `RECEIVED | duplicate | conflict`, and the sync `conflict` case is what `status_rules.success`
/// excludes (a duplicate IS a successful acceptance; a conflict is not).
pub mod journal_status {
    pub const RECEIVED: &str = "RECEIVED";
    pub const DUPLICATE: &str = "duplicate";
    pub const CONFLICT: &str = "conflict";
}

/// Values for `business.dispatch_outcome` (`spawned | duplicate_skipped`).
pub mod dispatch_outcome {
    pub const SPAWNED: &str = "spawned";
    pub const DUPLICATE_SKIPPED: &str = "duplicate_skipped";
}
