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
    /// PRODUCER — the routed birth's HANDOVER ACT (`place-order` contract, #598): the saga
    /// staging a `deliver:` onto the TARGET aggregate's mailbox lane inside its own fenced
    /// transaction, instead of appending to that aggregate's stream itself
    /// (ADR-20260816-040239). It is the second branch of `place-order`'s success ALTERNATION:
    /// with ROUTE_ORDER_BIRTH_THROUGH_LANE ON no [`EVENT_STORE_APPEND`] happens in the saga's
    /// trace, and requiring neither would score a checkout that lost the birth as a success.
    pub const ORDER_LANE_ENQUEUE: &str = "order.lane.enqueue";
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
    /// INTERNAL — one Postgres-mode CUSTOMER identity resolution at the request seam
    /// (`customer-identity` contract, #641, IDENT-1 Phase A): never emitted in claim mode (the
    /// DEFAULT), never for a non-CUSTOMER role. A CHILD of `auth.read_scope`'s span, ONE per
    /// resolution.
    pub const CUSTOMER_IDENTITY_RESOLVE: &str = "customer.identity.resolve";
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

    /// `customer-identity` contract key (#641, IDENT-1 Phase A): set only when
    /// `business.result = lookup_failed` — the coarse `DomainError` class, never the query text or
    /// driver message (unbounded cardinality on a labeled series).
    pub const FAILURE_REASON: &str = "business.failure_reason";

    /// `catalog.as_of.fold` keys (PROP-20260831-134539 slice 2, DARK — no
    /// `specs/observability.yaml` contract row yet; lands with slice 4 once a caller supplies a real
    /// correlation id). The as-of coordinate the read was bounded to (`CatalogVersion::get()`).
    pub const VERSION: &str = "business.version";
    /// Rows the SQL leg returned, technical rows included (before `$`-prefixed rows are dropped by
    /// decode) — the read's own cost signal, distinct from `events_applied`.
    pub const STREAM_LENGTH: &str = "business.stream_length";
    /// Business events the fold actually applied (`<= stream_length`, `<= version`).
    pub const EVENTS_APPLIED: &str = "business.events_applied";
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
    /// A DECLARED inbound FACT that did not reach its aggregate (#780, `mailbox-delivery`).
    /// Attributes `actor_type`, `message_type` and a CLOSED `reason`
    /// (`deferred` | `unparsable_payload`). `deferred` must be permanently zero in production:
    /// every increment is a routed `deliver:` that landed before the receiving aggregate's fold
    /// rule did. The two reasons stay in SEPARATE series -- collapsing them is how a
    /// must-be-zero counter becomes noise and gets ignored.
    pub const MAILBOX_FACT_UNRECORDED_TOTAL: &str = "mailbox_fact_unrecorded_total";
    /// The mailbox push listener lost delivery continuity (attribute `reason`: connection_lost |
    /// canary_timeout | connection_healed — the last is sqlx's silent in-place reconnect, where
    /// `live` never flapped but the gap's notifications are gone and a catch-up nudge ran).
    pub const MAILBOX_PUSH_DOWN_TOTAL: &str = "mailbox_push_down_total";
    pub const PLACE_ORDER_DURATION_MS: &str = "place_order_duration_ms";
    pub const ORDERS_PLACED_TOTAL: &str = "orders_placed_total";
    pub const CHECKOUT_PAYMENT_FAILURES_TOTAL: &str = "checkout_payment_failures_total";
    /// `place-order` contract (#588, ADR-20260816-040239): the HANDOVER latency the routed Order
    /// birth introduces — the saga's lane enqueue committing, to the Order lane's delivery
    /// recording `OrderPlaced`. Nothing measured this before #588 because there was no handover:
    /// the saga appended inline. Attribute `routed` (`true`|`false`). Technical, not BAM: it is a
    /// property of the runtime (lane depth, worker liveness, head-of-line blocking), and it must
    /// keep working when Postgres is degraded.
    pub const ORDER_BIRTH_LAG_MS: &str = "order_birth_lag_ms";
    /// `place-order` contract (#598): the Order-lane DEAD-MAN'S SWITCH, monotonic, attribute
    /// `lane`. Emitted on EVERY watch tick for EVERY declared routed-birth lane — including while
    /// ROUTE_ORDER_BIRTH_THROUGH_LANE is OFF, because a switch first proved on the day of the flip
    /// is a switch proved BY the flip. ALERT ON THE ABSENCE OF AN INCREMENT, never a threshold:
    /// [`ORDER_BIRTH_LAG_MS`] is silent by design while the flag is off, so without this counter
    /// "flag off" and "the Order lane worker is dead" are the same observation.
    pub const ORDER_LANE_WATCH_HEARTBEAT_TOTAL: &str = "order_lane_watch_heartbeat_total";
    /// `place-order` contract (#598): age of the OLDEST still-pending message on a routed-birth
    /// lane, in ms, attribute `lane`; 0 when the lane is drained. Deliberately NOT a zero-seeding
    /// of [`ORDER_BIRTH_LAG_MS`] — injected zeros would poison the p95 the flip is judged on, and
    /// a liveness signal and a latency measurement are different facts. Only readable because
    /// [`ORDER_LANE_WATCH_HEARTBEAT_TOTAL`] proves the reporter alive.
    pub const ORDER_LANE_OLDEST_PENDING_AGE_MS: &str = "order_lane_oldest_pending_age_ms";
    /// `place-order` contract (#598, farley): each process's RESOLVED value for a declared runtime
    /// flag, attributes `flag` | `value` | `bin`. Deploy-time fleet-parity EVIDENCE: review-time
    /// parity is an assertion, `count(distinct value) by (flag) > 1` is a fact, and it is the
    /// condition that blocks a flip mid-rolling-deploy. An OBSERVABLE gauge — a value written once
    /// at boot only says "this process once started" (the `otp_send_guard_enforcing` lesson).
    /// `version` is NOT a label: `service.version` is already a resource attribute.
    pub const RUNTIME_FLAG_STATE: &str = "runtime_flag_state";
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
    /// `payment-settlement` contract: the age of the OLDEST OrderTracking row still
    /// `payment_status = AUTHORIZED`, from its `placed_at`. BORN-but-never-CAPTURED only — an order
    /// that was never born has no OrderTracking row, so this gauge is structurally blind to it
    /// (#608 corrected the contract header that claimed otherwise; the never-born population is
    /// [`PAYMENT_AUTHORIZED_NO_ORDER_BIRTH_AGE_SECONDS`]). Emitted by the #608 birth-gap sweep,
    /// which is also the first thing that ever emitted it at all.
    pub const PAYMENT_AUTHORIZED_UNSETTLED_AGE_SECONDS: &str =
        "payment_authorized_unsettled_age_seconds";
    /// `place-order` contract (#608): THE BIRTH-GAP DEAD-MAN'S SWITCH — the age in seconds of the
    /// OLDEST authorization with no order behind it, 0 when the class is empty. Attribute `reason`,
    /// over the DECLARED bounded set [`crate::meters::birth_gap::REASONS`]: `retry_pending` (the
    /// saga hop is still deliverable), `delivery_exhausted` (terminal hop, run never resolved),
    /// `no_run` (a PaymentAuthorized fact with no `payment_process_manager` row — the crash window
    /// between PlaceOrder's two durable writes). EVERY member is emitted EVERY tick: an absent
    /// series must never read as "nothing stranded".
    pub const PAYMENT_AUTHORIZED_NO_ORDER_BIRTH_AGE_SECONDS: &str =
        "payment_authorized_no_order_birth_age_seconds";
    /// `place-order` contract (#608): the birth-gap sweep's own liveness, incremented once per
    /// COMPLETED sweep. ALERT ON THE ABSENCE OF AN INCREMENT, never a threshold — the gauges it
    /// accompanies read 0 on a healthy system, and 0 is indistinguishable from a dead sweep
    /// without this counter (the `order_lane_watch_heartbeat_total` shape, same reason).
    pub const PAYMENT_BIRTH_GAP_SWEEP_HEARTBEAT_TOTAL: &str =
        "payment_birth_gap_sweep_heartbeat_total";
    /// `custody-handback` contract (#639 part C step 3-ii, ADR-20260904-015903 §8): THE
    /// CUSTODY-HANDBACK DEAD-MAN'S SWITCH — mirrors [`PAYMENT_AUTHORIZED_NO_ORDER_BIRTH_AGE_SECONDS`]'s
    /// shape. The age in seconds of the OLDEST delivery job whose latest lifecycle fact is a
    /// handback with no LATER acceptance re-offering it, 0 when the class is empty. No attributes
    /// (unlike the birth-gap gauge's `reason` — there is exactly one class here). Emitted every
    /// sweep by the non-fenced `delivery_handback_watch.rs`.
    pub const DELIVERY_HANDED_BACK_UNREASSIGNED_AGE_SECONDS: &str =
        "delivery_handed_back_unreassigned_age_seconds";
    /// `custody-handback` contract: the sweep's own liveness, incremented once per COMPLETED sweep
    /// — same reason as [`PAYMENT_BIRTH_GAP_SWEEP_HEARTBEAT_TOTAL`].
    pub const DELIVERY_HANDBACK_SWEEP_HEARTBEAT_TOTAL: &str = "delivery_handback_sweep_heartbeat_total";
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
    /// `read-authorization` contract (#472): the SSR render boundary's DEFECT counter — a page
    /// whose declared read FAILED for real (never a role-refused skip-by-design) or whose declared
    /// condition expression could not be parsed shipped its degraded/error state. The
    /// [`CHECKOUT_DEGRADED_RENDER_TOTAL`] pattern, generalized to every SDUI page.
    pub const SDUI_DEGRADED_RENDER_TOTAL: &str = "sdui_degraded_render_total";
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
    /// `customer-identity` contract (#641, IDENT-1 Phase A): the resolution latency, attribute
    /// `result` (resolved | not_found | lookup_failed). Rides the existing `Customer.auth_ref`
    /// index — one indexed lookup, same budget discipline as `read_authorization_check_ms`.
    pub const CUSTOMER_IDENTITY_RESOLVE_MS: &str = "customer_identity_resolve_ms";
    /// `customer-identity` contract (#641): an ordinary provisioning gap — the verified subject
    /// carries no Postgres mapping row (yet). Fails closed to Public, same population as
    /// [`READ_AUTHORIZATION_BRIDGE_UNRESOLVED_TOTAL`] would watch on the role-path side, but scoped
    /// to THIS seam. OBSERVE, never PAGE — this is expected traffic, not an outage.
    pub const CUSTOMER_IDENTITY_NOT_FOUND_TOTAL: &str = "customer_identity_not_found_total";
    /// `customer-identity` contract (#641): the seam itself could not be asked — a DEFECT counter
    /// (the `read_authorization_bridge_unresolved_total` pattern), attribute `reason` (the coarse
    /// `DomainError` class: repository | invariant | rejected). The OPPOSITE operator response from
    /// [`CUSTOMER_IDENTITY_NOT_FOUND_TOTAL`]: PAGE on any sustained non-zero rate.
    pub const CUSTOMER_IDENTITY_LOOKUP_FAILED_TOTAL: &str = "customer_identity_lookup_failed_total";
    /// `customer-identity` contract (#641): attribute `source` (db | request_reuse) — so
    /// REQUEST-SCOPED REUSE can never hide an outage behind a cache hit. Phase A's two call sites
    /// each resolve read scope once per request, so only `db` fires today; `request_reuse` is
    /// declared for a later resolver reusing this seam's result within the same request.
    pub const CUSTOMER_IDENTITY_LOOKUP_SOURCE_TOTAL: &str = "customer_identity_lookup_source_total";
    /// `rider-identity` contract (#639 part C step 2b — the rider sign-in door): the resolution
    /// latency, attribute `result` (resolved | not_found | lookup_failed). One btree probe on
    /// `rider.auth_ref UNIQUE`, the same budget discipline as the customer seam.
    pub const RIDER_IDENTITY_RESOLVE_MS: &str = "rider_identity_resolve_ms";
    /// `rider-identity` contract: the verified subject has no `rider` row — a provisioning gap, a
    /// projector not yet caught up, or a registration the reservation refused. Fails closed to
    /// Public. OBSERVE, never PAGE.
    pub const RIDER_IDENTITY_NOT_FOUND_TOTAL: &str = "rider_identity_not_found_total";
    /// `rider-identity` contract: the seam itself could not be asked — a DEFECT counter, attribute
    /// `reason` (repository | invariant | rejected). The OPPOSITE operator response from
    /// [`RIDER_IDENTITY_NOT_FOUND_TOTAL`]: PAGE on any sustained non-zero rate. Its OWN name, not a
    /// `role` label on the customer counter, so a paging rule keyed on the customer seam cannot go
    /// quiet while riders fail on this one.
    pub const RIDER_IDENTITY_LOOKUP_FAILED_TOTAL: &str = "rider_identity_lookup_failed_total";
    /// `rider-identity` contract: attribute `source` (db | request_reuse) — declared for the same
    /// reason as the customer twin, so an in-request reuse can never hide an outage; only `db`
    /// fires today.
    pub const RIDER_IDENTITY_LOOKUP_SOURCE_TOTAL: &str = "rider_identity_lookup_source_total";
    /// `rider-restriction` contract (#639 part C step 4-i, ADR-20260904-081527 §9): the first
    /// guard-level emitter — a plain FORBIDDEN emits nothing today. Attribute `operation`, bounded
    /// by the closed `api-operation-key` set; deliberately NO `rider_id` label — the rider goes in
    /// the paired INFO trace event, joinable by `correlation_id` (the #748 skip-trace pattern).
    pub const RIDER_RESTRICTED_DENIED_TOTAL: &str = "rider_restricted_denied_total";
    /// `rider-restriction` contract: the `scope_membership_lag_positions` mirror for the Rider
    /// projector group — while it lags, a restricted rider is still GRANTED, so "immediately" is a
    /// measured claim only with this gauge.
    pub const RIDER_STANDING_LAG_POSITIONS: &str = "rider_standing_lag_positions";
    /// `rider-restriction` contract (#639 part C step 4-iii-B, ADR-20260904-152807 §8): THE
    /// RIDER-CUSTODY DEAD-MAN'S SWITCH — the age in seconds of the OLDEST stranded row (a job in
    /// ASSIGNED | PICKED_UP | OUT_FOR_DELIVERY whose rider is RESTRICTED, literally
    /// `rider_restriction.standing = 'RESTRICTED'`), anchored on `now - effective_at`, 0 when the
    /// population is empty. NO `rider_id` label — the info event `rider.restricted.holding_job`
    /// carries it, joined by aggregate ids. Emitted every sweep from
    /// `delivery_handback_watch_tick`, certified by [`DELIVERY_HANDBACK_SWEEP_HEARTBEAT_TOTAL`].
    pub const RIDER_RESTRICTED_HOLDING_JOB_AGE_SECONDS: &str =
        "rider_restricted_holding_job_age_seconds";
    /// `rider-identity` contract (#639 part C step 2c-i): the RIDER claim stamp
    /// (`identity.stamp_rider_claim`) failed -- a DEFECT counter, attribute `reason`
    /// (not_configured | claim_conflict | provider_error). The `customer_claim_stamp_failed_total`
    /// pattern under this contract's own name: each one is a rider whose verified sign-in issued
    /// no credential. `claim_conflict` is the one-subject-one-role refusal (PROP-20260831-180622
    /// Concern): fail closed, never an overwrite.
    pub const RIDER_CLAIM_STAMP_FAILED_TOTAL: &str = "rider_claim_stamp_failed_total";
    /// `rider-restriction` contract (#639 part C step 5, ADR-20260905-065415 §8), behind
    /// `RUN_RIDER_RESTRICTION_SOCKET_CLOSE`: the socket watcher's own outcome, attribute `outcome`
    /// (closed | no_open_socket | missed — RESERVED, never emitted: the dedicated
    /// `RIDER_RESTRICTION_SOCKET_CLOSE_MISSED_TOTAL` counter below is the signal for that case).
    /// NO `rider_id` label — the nested INFO event `rider.restricted.socket_terminated` carries
    /// the connection correlation id and the aggregate correlation id only, never `rider_id`.
    pub const RIDER_RESTRICTION_SOCKET_CLOSE_TOTAL: &str = "rider_restriction_socket_close_total";
    /// `rider-restriction` contract (step 5): t0 = the `EventBus` publish instant, never
    /// `occurred_at` (cross-host skew post-#358) — time from the fact landing on this process's bus
    /// to the Close frame entering the transport sink.
    pub const RIDER_RESTRICTION_SOCKET_CLOSE_LATENCY_MS: &str =
        "rider_restriction_socket_close_latency_ms";
    /// `rider-restriction` contract (step 5): a Lagged/Closed re-derivation could not complete after
    /// bounded retry (ADR-20260904-124600 §3: a lookup error never terminates) — the socket STAYS
    /// open. Attribute `reason` (lookup_failed | not_found — the identity seam has no mapping for
    /// this auth subject, distinct from a genuine lookup failure).
    pub const RIDER_RESTRICTION_SOCKET_CLOSE_MISSED_TOTAL: &str =
        "rider_restriction_socket_close_missed_total";
    /// `rider-restriction` contract (step 5): THE WATCHER'S OWN LIVENESS PROOF — the
    /// `otp_send_guard_enforcing` inverted dead-man precedent. 1 while at least one watcher task is
    /// alive in this process, 0 the instant none is; re-asserted every export cycle.
    pub const RIDER_RESTRICTION_SOCKET_WATCH_LIVE: &str = "rider_restriction_socket_watch_live";

    // --- member-sign-in (#639 part C step 6-ii, ADR-20260905-101349 §10) ------------------------

    /// The MEMBER seam's own resolution histogram -- the `rider_identity_resolve_ms` shape.
    pub const MEMBER_IDENTITY_RESOLVE_MS: &str = "member_identity_resolve_ms";
    /// OBSERVE: no `Member` row (or no single restaurant scope) for the verified subject.
    pub const MEMBER_IDENTITY_NOT_FOUND_TOTAL: &str = "member_identity_not_found_total";
    /// PAGE: the seam could not be asked. `reason` is the coarse DomainError class.
    pub const MEMBER_IDENTITY_LOOKUP_FAILED_TOTAL: &str = "member_identity_lookup_failed_total";
    pub const MEMBER_IDENTITY_LOOKUP_SOURCE_TOTAL: &str = "member_identity_lookup_source_total";
    /// `requestMemberSignInLink`'s outcome, attribute `result` (accepted | refused).
    pub const MEMBER_SIGN_IN_LINK_REQUESTED_TOTAL: &str = "member_sign_in_link_requested_total";
    /// `confirmMemberSignIn`'s outcome, attribute `result` (linked | not_linked | token_invalid |
    /// token_expired | lookup_failed).
    pub const MEMBER_SIGN_IN_CONFIRMED_TOTAL: &str = "member_sign_in_confirmed_total";
    /// The MEMBER claim stamp (`identity.stamp_member_claim`) failed -- the
    /// `rider_claim_stamp_failed_total` pattern under this contract's own name. Attribute `reason`
    /// (not_configured | claim_conflict | provider_error).
    pub const MEMBER_CLAIM_STAMP_FAILED_TOTAL: &str = "member_claim_stamp_failed_total";
    /// Either mutation refused, attribute `reason` (bounded, never an email/token/messageId).
    pub const MEMBER_SIGN_IN_REFUSED_TOTAL: &str = "member_sign_in_refused_total";
    /// GATE LIVENESS (the `otp_send_guard_enforcing` shape): 1 while `RUN_MEMBER_SIGN_IN_DOOR`
    /// enforces its refusal on a request, 0 the moment the composition root boots with the key OFF.
    pub const MEMBER_SIGN_IN_DOOR_ENFORCING: &str = "member_sign_in_door_enforcing";

    // --- admin-sign-in (#639 part C step 6-v, ADR-20260905-223957 §5) ----------------------------

    /// The ADMIN/platform seam's own resolution outcome -- ONE bounded counter (card F), never a
    /// per-role widening of `member_identity_resolve_ms`/`rider_identity_resolve_ms`'s histogram
    /// shape: the platform population (1-3) is too small for a latency distribution to mean
    /// anything. Attribute `result` (resolved | not_found | lookup_failed).
    pub const ADMIN_IDENTITY_RESOLVE_TOTAL: &str = "admin_identity_resolve_total";
    /// `GrantPlatformAccess`'s outcome, attribute `basis` (bounded, `PlatformAccessBasis` -- one
    /// value today).
    pub const PLATFORM_ACCESS_GRANTED_TOTAL: &str = "platform_access_granted_total";
    /// GATE LIVENESS (the `member_sign_in_door_enforcing` shape): 1 while
    /// `RUN_PLATFORM_ACCESS_GRANT` enforces its refusal, 0 the moment the composition root boots
    /// with the key OFF -- registered at BOTH composition roots unconditionally.
    pub const PLATFORM_ACCESS_GRANT_ENFORCING: &str = "platform_access_grant_enforcing";

    // --- admin-sign-in-door (#639 part C step 6-iii, ADR-20260906-023825) -- the ADMIN sign-in
    // door itself, its OWN contract split from `admin-sign-in`'s seam-probe latency budget --------

    /// `requestAdminSignInLink`'s outcome, attribute `result` (accepted | refused).
    pub const ADMIN_SIGN_IN_LINK_REQUESTED_TOTAL: &str = "admin_sign_in_link_requested_total";
    /// `confirmAdminSignIn`'s outcome, attribute `result` (granted | not_granted | token_invalid |
    /// token_expired | lookup_failed | door_closed | requires_session | claim_conflict | rejected).
    pub const ADMIN_SIGN_IN_CONFIRMED_TOTAL: &str = "admin_sign_in_confirmed_total";
    /// The ADMIN claim stamp (`identity.stamp_admin_claim`) failed -- the
    /// `member_claim_stamp_failed_total` pattern under this contract's own name. Attribute `reason`
    /// (not_configured | claim_conflict | provider_error).
    pub const ADMIN_CLAIM_STAMP_FAILED_TOTAL: &str = "admin_claim_stamp_failed_total";
    /// Either mutation refused, attribute `reason` (bounded, never an email/token/messageId).
    pub const ADMIN_SIGN_IN_REFUSED_TOTAL: &str = "admin_sign_in_refused_total";
    /// GATE LIVENESS (the `member_sign_in_door_enforcing` shape): 1 while `RUN_ADMIN_SIGN_IN_DOOR`
    /// enforces its refusal on a request, 0 the moment the composition root boots with the key OFF
    /// -- registered at BOTH composition roots unconditionally.
    pub const ADMIN_SIGN_IN_DOOR_ENFORCING: &str = "admin_sign_in_door_enforcing";

    // --- restaurant-invitation (#639 part C step 6-iv, ADR-20260905-101349 §2/§3) ----------------
    /// `inviteRestaurantMember`'s outcome, attribute `authority` (the INVITED authority, never the
    /// caller's).
    pub const RESTAURANT_INVITATION_SENT_TOTAL: &str = "restaurant_invitation_sent_total";
    /// `acceptRestaurantInvitation`'s ACCEPTED outcome.
    pub const RESTAURANT_INVITATION_ACCEPTED_TOTAL: &str = "restaurant_invitation_accepted_total";
    /// The TTL reminder's recorded expiry.
    pub const RESTAURANT_INVITATION_EXPIRED_TOTAL: &str = "restaurant_invitation_expired_total";
    /// `revokeRestaurantInvitation`'s recorded withdrawal.
    pub const RESTAURANT_INVITATION_REVOKED_TOTAL: &str = "restaurant_invitation_revoked_total";
    /// GATE LIVENESS (the `member_sign_in_door_enforcing` shape): 1 while
    /// `RUN_RESTAURANT_INVITATION` enforces its refusal on a request, 0 the moment the composition
    /// root boots with the key OFF.
    pub const RESTAURANT_INVITATION_DOOR_ENFORCING: &str = "restaurant_invitation_door_enforcing";
    /// Round 3: the `RestaurantRoster` projector group's own replay lag (the
    /// `rider_standing_lag_positions` shape).
    pub const RESTAURANT_ROSTER_LAG_POSITIONS: &str = "restaurant_roster_lag_positions";
    /// Round 3: the `RestaurantInvitationList` projector group's own replay lag.
    pub const RESTAURANT_INVITATION_LIST_LAG_POSITIONS: &str =
        "restaurant_invitation_list_lag_positions";

    // --- graphql-limits (#639 part C step 6-ii, ADR-20260905-101349 §9) --------------------------

    /// Refused BEFORE any resolver runs, attributes `role` + `reason` (depth | complexity).
    pub const GRAPHQL_REQUEST_REJECTED_TOTAL: &str = "graphql_request_rejected_total";
    /// The observed depth of every parsed document, attribute `role`.
    pub const GRAPHQL_QUERY_DEPTH: &str = "graphql_query_depth";
    /// The observed complexity (total selected field count) of every parsed document, attribute `role`.
    pub const GRAPHQL_QUERY_COMPLEXITY: &str = "graphql_query_complexity";
    /// The CONFIGURED ceiling this process enforces, attributes `role` + `kind` (depth | complexity).
    pub const GRAPHQL_LIMIT_MAX: &str = "graphql_limit_max";
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
