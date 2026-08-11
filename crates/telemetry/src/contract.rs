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
    /// INTERNAL — the sub -> domain-id bridge, ONE per request (`read-authorization` contract, #144).
    pub const AUTH_READ_SCOPE: &str = "auth.read_scope";
    /// INTERNAL — one discrete membership check (a by-id read or the subscription guard, #144).
    pub const AUTH_SCOPE_MEMBERSHIP: &str = "auth.scope_membership";
    /// CLIENT — the customer claim stamp onto the auth provider's user (`customer-identification`
    /// contract, #437): admin GET+PUT inside the identity ACL, `business.result` stamped | failed.
    pub const CLAIMS_STAMP: &str = "claims.stamp";
    /// INTERNAL — one priced cart READ at the GraphQL resolver seam (`cart-price` contract, #451):
    /// the money-free Cart row priced fresh from the live catalog via `price_cart`. The pricer
    /// itself stays SDK-free; only the resolver boundary constructs this span.
    pub const CART_PRICE: &str = "cart.price";
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

    /// `read-authorization` contract keys (#144).
    pub const ROLE: &str = "business.role";
    pub const BRIDGE_RESOLVED: &str = "business.bridge_resolved";
    pub const SCOPE_TYPE: &str = "business.scope_type";
    pub const AUTHORIZED: &str = "business.authorized";
}

/// Metric names, split exactly as the contracts split them: `metrics` are technical, `business_metrics`
/// feed BAM. Keeping the two apart is a contract requirement, not a style choice.
pub mod metric {
    pub const COMMANDS_ACCEPTED_TOTAL: &str = "commands_accepted_total";
    pub const COMMAND_DUPLICATES_TOTAL: &str = "command_duplicates_total";
    pub const COMMAND_SYNC_CONFLICTS_TOTAL: &str = "command_sync_conflicts_total";
    pub const COMMAND_COMPLETION_MS: &str = "command_completion_ms";
    /// A mailbox delivery terminally FAILED by the attempts cap (PROP-20260802-223522 D4) — an
    /// operator event, attribute `actor_type`.
    pub const MAILBOX_POISON_FAILED_TOTAL: &str = "mailbox_poison_failed_total";
    /// The mailbox push listener lost delivery continuity (attribute `reason`: connection_lost |
    /// canary_timeout | connection_healed — the last is sqlx's silent in-place reconnect, where
    /// `live` never flapped but the gap's notifications are gone and a catch-up nudge ran).
    pub const MAILBOX_PUSH_DOWN_TOTAL: &str = "mailbox_push_down_total";
    pub const PLACE_ORDER_DURATION_MS: &str = "place_order_duration_ms";
    pub const ORDERS_PLACED_TOTAL: &str = "orders_placed_total";
    pub const CHECKOUT_PAYMENT_FAILURES_TOTAL: &str = "checkout_payment_failures_total";
    /// `place-order` contract (#440): the checkout shell rendered WITHOUT a mountable payment
    /// element — a DEFECT counter (the customer_claim_stamp_failed_total pattern), attribute
    /// `reason`. A degraded render produces ZERO place-order runs (the customer cannot even try),
    /// so the saga contract cannot see it by construction; alert on any sustained non-zero rate.
    /// Emitted from the checkout render/mount seam (the framework boundary that owns it), never
    /// from domain code. Distinct from CHECKOUT_PAYMENT_FAILURES_TOTAL, which counts payments that
    /// RAN and failed.
    pub const CHECKOUT_DEGRADED_RENDER_TOTAL: &str = "checkout_degraded_render_total";
    /// `read-authorization` contract (#144). Denials only ever fire on by-id/subscription paths —
    /// the list path enforces via a fused SQL predicate, so a list "denial" is structurally
    /// invisible (rows are simply absent); see the contract comment before "fixing" that.
    pub const READ_AUTHORIZATION_DENIED_TOTAL: &str = "read_authorization_denied_total";
    pub const READ_AUTHORIZATION_CHECKS_TOTAL: &str = "read_authorization_checks_total";
    pub const READ_AUTHORIZATION_BRIDGE_UNRESOLVED_TOTAL: &str =
        "read_authorization_bridge_unresolved_total";
    pub const READ_AUTHORIZATION_CHECK_MS: &str = "read_authorization_check_ms";
    /// `read-authorization` contract (#469): the OPEN path read a credential and could not act on
    /// it, so the request was served ANONYMOUS — attribute `reason` (invalid_token |
    /// verifier_unavailable | role_not_customer). `invalid_token` is ordinary (a stale cookie);
    /// `verifier_unavailable` is identified customers silently getting the anonymous view — the
    /// storefront's cart disappearing with nothing else logged.
    pub const PUBLIC_CREDENTIAL_DEGRADED_TOTAL: &str = "public_credential_degraded_total";
    /// BAM gauge: projection lag on the ACL index — while it lags, a just-placed order's own
    /// customer is DENIED their order (`read-authorization` business_metrics).
    pub const SCOPE_MEMBERSHIP_LAG_POSITIONS: &str = "scope_membership_lag_positions";
    /// `customer-identification` contract (#437): a claim stamp failed — a DEFECT counter (the
    /// read_authorization_bridge_unresolved_total pattern), attribute `reason` (not_configured |
    /// claim_conflict | provider_error). Each one is a customer whose login silently stayed
    /// anonymous; alert on any sustained non-zero rate.
    pub const CUSTOMER_CLAIM_STAMP_FAILED_TOTAL: &str = "customer_claim_stamp_failed_total";
    /// `cart-price` contract (#451): read-side pricing latency at the resolver seam — if it
    /// drifts toward the budget, the per-request memoized catalog read is the lever.
    pub const CART_PRICE_MS: &str = "cart_price_ms";
    /// `cart-price` contract (#451): a cart whose price cannot be resolved at read — a DEFECT
    /// counter (the checkout_degraded_render_total pattern), attribute `reason` (offer_gone |
    /// policy_missing | stock_unknown). Each one is a customer who saw NO payable amount — a
    /// sale silently lost; alert on any sustained non-zero rate.
    pub const CART_PRICE_UNRESOLVABLE_TOTAL: &str = "cart_price_unresolvable_total";
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
    /// The mailbox era (#242): the command was ENQUEUED on the actor mailbox — the partitioned
    /// worker delivers it; nothing is spawned in the request path.
    pub const ENQUEUED: &str = "enqueued";
}
