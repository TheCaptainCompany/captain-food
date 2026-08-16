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
    /// INTERNAL — the `inbound_messages` insert: durable RECEIVED, or duplicate/conflict.
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
    /// INTERNAL — one promoted reminder's DELIVERY at the mailbox layer (`acceptance-timeout`
    /// contract, #167): the fire delay against the declared due time, and — while the enforcement
    /// gate is OFF — the shadow would-cancel decision that is the flip ADR's whole evidence set
    /// (the `service_window_verdict` precedent). Never in the pure handler (SDK-free rule).
    pub const REMINDER_PROMOTE: &str = "reminder.promote";
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
    /// RSO-1 (DECISIONS §43): the service-hours verdict a checkout was evaluated at, on
    /// `command.validate` — the accept branch's signal while the guard runs in shadow mode.
    pub const SERVICE_WINDOW_VERDICT: &str = "business.service_window_verdict";
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

    /// `acceptance-timeout` contract keys (#167) — the `reminder.promote` shadow-evidence span.
    /// The reminder's name (`OrderAcceptanceTimedOut`, `OrderExpired`, …).
    pub const REMINDER_TYPE: &str = "business.reminder_type";
    /// The row's declared due time (RFC 3339) — `scheduled_at`, which promotion never clears.
    pub const DUE_AT: &str = "business.due_at";
    /// Delivery time minus due time, ms — the honest promotion-slop measurement.
    pub const FIRE_DELAY_MS: &str = "business.fire_delay_ms";
    /// The gate position at DELIVERY time: `true` = ENFORCE_ACCEPTANCE_TIMEOUT off (shadow mode,
    /// append inert), `false` = enforcement live.
    pub const SHADOW: &str = "business.shadow";
    /// The guard's decision: `true` = the identical still-PLACED fold+predicate would cancel
    /// (Cancelled when enforcing, Ignored-with-evidence in shadow), `false` = benign no-op.
    pub const WOULD_CANCEL: &str = "business.would_cancel";

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
    /// `payment-settlement` contract (ADR-20260808-195315 §1.2): capturing a confirmed
    /// authorization FAILED after fulfilment — the food is cooked and the money did not move, the
    /// inverse of the paid-order-nobody-told-about class. Attribute `reason` =
    /// scalars.yaml#/CaptureFailureReason. PAGES on ANY increment; the recorded
    /// PaymentCaptureFailed fact on the Payment stream is the durable twin this counter taps.
    pub const PAYMENT_CAPTURE_FAILED_TOTAL: &str = "payment_capture_failed_total";
    /// `payment-settlement` contract: voiding an uncaptured authorization failed — warn-level,
    /// never a page: the hold self-heals (Stripe expires it within ~7 days and reports
    /// PaymentReleased). Attribute `reason` (deterministic | transient).
    pub const PAYMENT_RELEASE_FAILED_TOTAL: &str = "payment_release_failed_total";
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
    /// verifier_unavailable | role_not_customer | claim_absent). `invalid_token` is ordinary (a
    /// stale cookie); `verifier_unavailable` is identified customers silently getting the anonymous
    /// view — the storefront's cart disappearing with nothing else logged; `claim_absent` is the
    /// pre-claim-stamp window, which lives here rather than in
    /// [`READ_AUTHORIZATION_BRIDGE_UNRESOLVED_TOTAL`] because on the open path nothing is DENIED
    /// by it — and a rollout must not read as a provisioning incident.
    pub const PUBLIC_CREDENTIAL_DEGRADED_TOTAL: &str = "public_credential_degraded_total";
    /// BAM gauge: projection lag on the ACL index — while it lags, a just-placed order's own
    /// customer is DENIED their order (`read-authorization` business_metrics).
    pub const SCOPE_MEMBERSHIP_LAG_POSITIONS: &str = "scope_membership_lag_positions";
    /// `customer-identification` contract (#437): a claim stamp failed — a DEFECT counter (the
    /// read_authorization_bridge_unresolved_total pattern), attribute `reason` (not_configured |
    /// claim_conflict | provider_error). Each one is a customer whose login silently stayed
    /// anonymous; alert on any sustained non-zero rate.
    pub const CUSTOMER_CLAIM_STAMP_FAILED_TOTAL: &str = "customer_claim_stamp_failed_total";
    /// `customer-identification` contract (#516): OTP sends ASKED FOR, attributes `dialing_code`
    /// (bounded — the allowlist plus `other`) and `allowed`. The phone number is NEVER a label; high
    /// cardinality belongs on the span, and only hashed.
    pub const OTP_SEND_REQUESTED_TOTAL: &str = "otp_send_requested_total";
    /// `customer-identification` contract (#516): OTP sends REFUSED by the send guards, attribute
    /// `reason` (country_not_served | unparseable | cooldown | hourly_cap | daily_cap |
    /// global_ceiling | store_unavailable). `global_ceiling` means real sign-ups are being turned
    /// away. Note the INVERTED dead-man's switch: zero refusals reads identically to "the limiter is
    /// switched off", which is why [`OTP_SEND_GUARD_ENFORCING`] exists beside it.
    pub const OTP_SEND_REFUSED_TOTAL: &str = "otp_send_refused_total";
    /// `customer-identification` contract (#516): THE MONEY SEAM — one per message handed to the OVH
    /// sender, attribute `result` (sent | failed | refused). A persistent gap between this and
    /// `otp_send_requested_total{allowed=true}` means we are being asked to send messages nobody
    /// requested through our own front door.
    pub const SMS_SEND_TOTAL: &str = "sms_send_total";
    /// `customer-identification` contract (#516): 1 while the send guard is enforcing against the
    /// SHARED counter, 0 when degraded — the liveness proof that separates "a quiet night" from "the
    /// guard has been off since the last deploy".
    pub const OTP_SEND_GUARD_ENFORCING: &str = "otp_send_guard_enforcing";
    /// `cart-price` contract (#451): read-side pricing latency at the resolver seam — if it
    /// drifts toward the budget, the per-request memoized catalog read is the lever.
    pub const CART_PRICE_MS: &str = "cart_price_ms";
    /// `cart-price` contract (#451): a cart whose price cannot be resolved at read — a DEFECT
    /// counter (the checkout_degraded_render_total pattern), attribute `reason` (offer_gone |
    /// policy_missing | stock_unknown). Each one is a customer who saw NO payable amount — a
    /// sale silently lost; alert on any sustained non-zero rate.
    pub const CART_PRICE_UNRESOLVABLE_TOTAL: &str = "cart_price_unresolvable_total";
    /// `acceptance-timeout` contract (#167): the promotion machinery's DEAD-MAN'S SWITCH —
    /// emitted on EVERY watch tick per actor type (0 when nothing is due), never only when a
    /// reminder arrives (ADR-20260810-231300: a monitor that can only fire when a signal arrives
    /// goes quiet exactly when it should scream). Value = now minus the oldest DUE SCHEDULED
    /// row's `scheduled_at` (0 when none due). A GROWING value = promotion is dead; the metric
    /// STOPPING = the watcher itself is dead — both are alertable. Attribute `actor_type`.
    pub const REMINDER_PROMOTION_DUE_LAG_MS: &str = "reminder_promotion_due_lag_ms";
    /// `acceptance-timeout` contract (#167, the dba gauge): SCHEDULED row depth by
    /// (`actor_type`, `purpose` = the reminder's message_type) — the cardinality watch on the
    /// reminder table (V0 expectation ≈ single digits; a runaway here is a scheduling leak).
    pub const MAILBOX_SCHEDULED_DEPTH: &str = "mailbox_scheduled_depth";
}

/// Values for `business.journal_status` — the contract comments them as
/// `RECEIVED | duplicate | conflict`, and the sync `conflict` case is what `status_rules.success`
/// excludes (a duplicate IS a successful acceptance; a conflict is not).
pub mod journal_status {
    pub const RECEIVED: &str = "RECEIVED";
    pub const DUPLICATE: &str = "duplicate";
    pub const CONFLICT: &str = "conflict";
}

/// Values for `business.dispatch_outcome` (`enqueued | duplicate_skipped`), matching
/// `specs/observability.yaml` verbatim — this is the vocabulary a dashboard filter may rely on.
/// `spawned` retired with the in-request spawn (#242 Runtime D): it is neither emitted nor declared,
/// so the constant is DELETED rather than kept as a value nothing can produce.
pub mod dispatch_outcome {
    pub const DUPLICATE_SKIPPED: &str = "duplicate_skipped";
    /// The mailbox era (#242): the command was ENQUEUED on the actor mailbox — the partitioned
    /// worker delivers it; nothing is spawned in the request path.
    pub const ENQUEUED: &str = "enqueued";
}
