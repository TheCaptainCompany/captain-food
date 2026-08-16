//! Span constructors for the instrumented boundaries.
//!
//! Every span required by `specs/observability.yaml` is built here, in ONE place, rather than by a
//! `tracing::info_span!` at each call site. Two reasons, both practical:
//!
//! - The `command.*` spans are emitted from ~100 GENERATED mutation resolvers. Inlining the macro would
//!   copy the field list a hundred times into `mutation.rs`, so a contract change would mean re-reading
//!   generated output to check it landed everywhere.
//! - `tracing` field names must be literals in the macro, so they cannot be the `contract::attr`
//!   constants. Confining the literals to this file means the conformance test has exactly one file to
//!   check against the spec, instead of trusting that a literal somewhere in generated code is right.
//!
//! `otel.kind` is the field `tracing-opentelemetry` reads to set the OTel span kind, so the
//! SERVER/INTERNAL/CLIENT/PRODUCER/CONSUMER kinds the contracts declare are set through it.
//!
//! ## Holding a span across an await
//!
//! These return a `Span`; they do not enter it. Callers use `.instrument(span)` on the future, or
//! `span.in_scope(|| ...)` for synchronous work. Calling `Span::enter()` and holding the guard across an
//! `.await` attributes every later span on that worker thread to the wrong parent — a bug whose symptom
//! is a trace tree that looks plausible and is wrong, which is worse than a missing span.

use tracing::field::Empty;
use tracing::Span;

use crate::contract::attr;

/// `command.receive` (SERVER) — the synchronous acceptance entry point.
///
/// Opened BEFORE any work, so a resolver that fails on input deserialization still produces a span
/// naming the command that was attempted — the failures you most want a trace for are the ones that
/// never reach the journal.
///
/// `correlation_id` and `message_id` are late-bound on purpose: both may be server-generated (UUIDv7)
/// inside `request_envelope`, so neither is knowable when the span opens.
///
/// `business.journal_status` is NOT declared here; it belongs to `command.journal`. Late-bound fields
/// that do belong to this span are declared `Empty` so `record` can fill them: `tracing` cannot add a
/// field that was not declared when the span was created, and a silent no-op `record` is exactly the
/// kind of hole that makes a contract pass review and fail in production.
pub fn command_receive(command_type: &str, actor: &str, channel: &str) -> Span {
    tracing::info_span!(
        "command.receive",
        otel.kind = "server",
        business.command_type = command_type,
        business.actor = actor,
        business.channel = channel,
        business.correlation_id = Empty,
        business.message_id = Empty,
        business.order_id = Empty,
    )
}

/// `command.journal` (INTERNAL) — the `inbound_messages` insert (the span keeps its contract
/// name; the acceptance contract is unchanged, ADR-20260720-015500).
pub fn command_journal(message_id: &str) -> Span {
    tracing::info_span!(
        "command.journal",
        otel.kind = "internal",
        business.message_id = message_id,
        business.journal_status = Empty,
    )
}

/// `command.dispatch` (INTERNAL) — handing the command to the async handler, or declining to.
pub fn command_dispatch(message_id: &str, outcome: &str) -> Span {
    tracing::info_span!(
        "command.dispatch",
        otel.kind = "internal",
        business.message_id = message_id,
        business.dispatch_outcome = outcome,
    )
}

/// `command.validate` (INTERNAL) — the handler-boundary wrapper's invariant check. Emitted by the
/// dispatch wrapper, never by the aggregate: `c4-l3.yaml` marks `command-handlers`
/// `instrumented: false`, and an aggregate that imports a telemetry SDK stops being testable without a
/// subscriber.
///
/// `business.service_window_verdict` (RSO-1, DECISIONS §43) is late-bound: the place-order
/// contract requires the service-hours verdict the checkout was evaluated at — the ACCEPT
/// branch's signal (shadow mode refuses nothing, so without this attribute the decision is
/// invisible in traces). Recorded by the dispatch wrapper via [`record_service_window_verdict`],
/// never by the domain function (`serving_at` stays SDK-free).
pub fn command_validate() -> Span {
    tracing::info_span!(
        "command.validate",
        otel.kind = "internal",
        business.validation_status = Empty,
        business.service_window_verdict = Empty,
    )
}

/// `event.store.append` (INTERNAL) — appending to `domain_events`.
///
/// `event_count` is not in the contract; it is added because the contract's `business.event_type` is
/// singular while an append is one transaction over possibly several events. Without the count, a span
/// naming `OrderPlaced` reads as "one event was appended" when `CartCheckedOut` went with it.
pub fn event_store_append(event_type: &str, stream_id: &str) -> Span {
    tracing::info_span!(
        "event.store.append",
        otel.kind = "internal",
        business.event_type = event_type,
        business.stream_id = stream_id,
        event_count = Empty,
    )
}

/// Record how many events one `event.store.append` transaction carried.
pub fn record_event_count(span: &Span, count: usize) {
    span.record("event_count", count);
}

/// `event.publish` (PRODUCER) — publishing an appended event onto the bus.
pub fn event_publish(event_type: &str) -> Span {
    tracing::info_span!(
        "event.publish",
        otel.kind = "producer",
        messaging.system = "captain.eventbus",
        business.event_type = event_type,
    )
}

/// `event.consume.projection` (CONSUMER) — a projector applying events to a read model.
pub fn event_consume_projection(projection_name: &str) -> Span {
    tracing::info_span!(
        "event.consume.projection",
        otel.kind = "consumer",
        business.projection_name = projection_name,
    )
}

/// `payment.intent.create` (CLIENT) — the outbound Stripe call, the riskiest leg of checkout.
pub fn payment_intent_create() -> Span {
    tracing::info_span!(
        "payment.intent.create",
        otel.kind = "client",
        messaging.system = "stripe",
        business.result = Empty,
    )
}

/// `pricing.compute` (INTERNAL) — the 3-way split (ADR-0017).
pub fn pricing_compute() -> Span {
    tracing::info_span!(
        "pricing.compute",
        otel.kind = "internal",
        business.service_fee = Empty,
        business.split_ok = Empty,
    )
}

/// `cart.read` (INTERNAL) — loading the cart aggregate for repricing.
pub fn cart_read(aggregate_id: &str) -> Span {
    tracing::info_span!(
        "cart.read",
        otel.kind = "internal",
        business.aggregate_id = aggregate_id,
    )
}

/// `cart.price` (INTERNAL) — ONE priced cart READ at the GraphQL resolver seam (`cart-price`
/// contract, #451): the money-free Cart row priced fresh from the live catalog via `price_cart`.
/// `business.aggregate_id` = the cartId being priced. `business.correlation_id` is the
/// REQUEST-scoped id (contract `run_identity.correlation_id.source: request.correlation_id`),
/// recorded by the caller via [`record_correlation_id`].
///
/// `otel.status_code` is late-bound and DECLARED here for the same reason `claims.stamp` declares
/// it: the contract classifies an unresolvable price as `technical_error` via
/// `status_rules.technical_error.any_span_errors`, and a span that never carries an ERROR status
/// can never satisfy that rule — every failed price would export as a plain success and the
/// "alert on any sustained non-zero rate" posture would be watching a counter with no span-side
/// twin. A field not declared at construction cannot be `record`ed later by `tracing`.
pub fn cart_price(aggregate_id: &str) -> Span {
    tracing::info_span!(
        "cart.price",
        otel.kind = "internal",
        business.aggregate_id = aggregate_id,
        business.correlation_id = Empty,
        otel.status_code = Empty,
    )
}

/// Mark a priced-cart read as FAILED (the `PriceUnresolvable` branch). Sets OTel ERROR status so
/// the `cart-price` contract's `technical_error: any_span_errors` rule can classify the run — the
/// counter twin is `cart_price_unresolvable_total{reason}`.
pub fn record_cart_price_error(span: &Span) {
    span.record("otel.status_code", "ERROR");
}

/// Record the read-path `business.correlation_id` minted at the resolver seam.
pub fn record_correlation_id(span: &Span, correlation_id: &str) {
    span.record(attr::CORRELATION_ID, correlation_id);
}

// --- late-bound recorders -----------------------------------------------------------------------
//
// Each takes the span explicitly rather than using `Span::current()`. After an `.instrument(..).await`
// the instrumented span is no longer current, so `Span::current()` would silently record onto the
// caller's span instead — a wrong value, not a missing one.

/// Record `business.journal_status` (`RECEIVED` | `duplicate` | `conflict`). The `conflict` value is
/// what `status_rules.success` excludes: a duplicate IS a successful acceptance, a conflict is not.
pub fn record_journal_status(span: &Span, status: &str) {
    span.record(attr::JOURNAL_STATUS, status);
}

/// Record `business.message_id` on `command.receive` once the effective envelope is resolved (the id may
/// be server-generated, so it is not known when the span opens).
pub fn record_message_id(span: &Span, message_id: &str) {
    span.record(attr::MESSAGE_ID, message_id);
}

/// Record the envelope's `run_identity` on `command.receive` — both ids, in one call, because the
/// contracts mark both mandatory and it is the pair that makes a run findable: `correlation_id` is
/// business-facing and survives the whole causality chain, `trace_id` is technical and may rotate at an
/// async boundary. `trace_id` itself is set by the OTel layer, so only the correlation is recorded here.
pub fn record_envelope(span: &Span, message_id: &str, correlation_id: &str) {
    span.record(attr::MESSAGE_ID, message_id);
    span.record(attr::CORRELATION_ID, correlation_id);
}

/// Record `business.validation_status` (`accepted` | `rejected`).
pub fn record_validation_status(span: &Span, status: &str) {
    span.record(attr::VALIDATION_STATUS, status);
}

/// Record `business.service_window_verdict` on `command.validate` (RSO-1): the value set is
/// `scalars.yaml#/ServiceWindowVerdict` (`OPEN` | `OUTSIDE_HOURS` | `HOURS_UNDECLARED`), by
/// contract — never a hand-invented string.
pub fn record_service_window_verdict(span: &Span, verdict: &str) {
    span.record(attr::SERVICE_WINDOW_VERDICT, verdict);
}

/// Record `business.result` on `payment.intent.create` (`captured` is what the contract's success
/// condition tests for).
pub fn record_payment_result(span: &Span, result: &str) {
    span.record(attr::RESULT, result);
}

/// Record the `pricing.compute` outputs.
pub fn record_pricing(span: &Span, service_fee: i64, split_ok: bool) {
    span.record(attr::SERVICE_FEE, service_fee);
    span.record(attr::SPLIT_OK, if split_ok { "true" } else { "false" });
}

/// Record `business.order_id` — a `place-order` `run_identity` key, known only once the order exists.
pub fn record_order_id(span: &Span, order_id: &str) {
    span.record(attr::ORDER_ID, order_id);
}

/// `reminder.promote` (INTERNAL) — one promoted reminder's DELIVERY at the mailbox layer
/// (`acceptance-timeout` contract, #167): the shadow-evidence span the flip ADR reads. Lives at
/// the infrastructure delivery seam, NEVER in the pure record handler (`record_order_acceptance_timeout`
/// only DECIDES; business code stays SDK-free — the `service_window_verdict` precedent).
///
/// `due_at`/`fire_delay_ms` measure the honest promotion slop against the row's declared
/// `scheduled_at` (absent on a redelivery under a fresh identity, hence late-bound). `shadow` is
/// the ENFORCE_ACCEPTANCE_TIMEOUT gate position read at DELIVERY time; `would_cancel` is the
/// still-PLACED guard's decision — the outcome label split (`would_cancel` vs `noop`) that turns
/// shadow traffic into flip evidence.
pub fn reminder_promote(reminder_type: &str, shadow: bool) -> Span {
    tracing::info_span!(
        "reminder.promote",
        otel.kind = "internal",
        business.reminder_type = reminder_type,
        business.shadow = shadow,
        business.due_at = Empty,
        business.fire_delay_ms = Empty,
        business.would_cancel = Empty,
        business.order_id = Empty,
    )
}

/// Record the promoted row's due time + measured fire delay on `reminder.promote`.
pub fn record_reminder_due(span: &Span, due_at: &str, fire_delay_ms: i64) {
    span.record(attr::DUE_AT, due_at);
    span.record(attr::FIRE_DELAY_MS, fire_delay_ms);
}

/// Record the guard's decision on `reminder.promote` — `true` iff the identical fold+predicate
/// decided the order would be cancelled (appended when enforcing, inert in shadow).
pub fn record_would_cancel(span: &Span, would_cancel: bool) {
    span.record(attr::WOULD_CANCEL, would_cancel);
}

/// `auth.read_scope` (INTERNAL) — the sub -> domain-id bridge, ONE per request (#144). If this span
/// appears N times for a single request, the once-per-request caching contract of
/// PROP-20260725-185140 §3.3 has regressed.
///
/// `bridge_resolved` and `correlation_id` are late-bound: the correlation id is MINTED at the server
/// boundary (reads carry no command envelope), and whether the bridge resolved is only known after
/// the lookup.
pub fn auth_read_scope(role: &str) -> Span {
    tracing::info_span!(
        "auth.read_scope",
        otel.kind = "internal",
        business.role = role,
        business.bridge_resolved = Empty,
        business.correlation_id = Empty,
    )
}

/// Record whether the bridge resolved the caller to a domain identity. `false` means the request
/// degrades to Public — denied, safely, but a DEFECT worth its own counter
/// (`read_authorization_bridge_unresolved_total`), never noise inside the ordinary denial count.
pub fn record_bridge_resolved(span: &Span, resolved: bool) {
    span.record(attr::BRIDGE_RESOLVED, if resolved { "true" } else { "false" });
}

/// `auth.scope_membership` (INTERNAL) — one discrete membership check (#144): a by-id read or the
/// subscription guard. The LIST path never emits this — it enforces via a fused SQL predicate, so
/// there is no per-row decision to record (see the contract comment).
pub fn auth_scope_membership(scope_type: &str, role: &str) -> Span {
    tracing::info_span!(
        "auth.scope_membership",
        otel.kind = "internal",
        business.scope_type = scope_type,
        business.role = role,
        business.authorized = Empty,
    )
}

/// Record the check's outcome. A denial is an ORDINARY outcome (`status_rules.business_rejected`),
/// never a technical error.
pub fn record_authorized(span: &Span, authorized: bool) {
    span.record(attr::AUTHORIZED, if authorized { "true" } else { "false" });
}

/// `claims.stamp` (CLIENT) — the customer claim stamp onto the auth provider's user
/// (`customer-identification` contract, #437): the admin GET+PUT inside the identity ACL that
/// writes `app_metadata.captain_food` = `{ role, customer_id }` at phone verification. Emitted by the ACL, so it
/// nests under the ambient verify-flow span and shares its trace/correlation.
///
/// `business.result` (stamped | failed) and `otel.status_code` are late-bound: the outcome is only
/// known after the provider answers.
pub fn claims_stamp() -> Span {
    tracing::info_span!(
        "claims.stamp",
        otel.kind = "client",
        messaging.system = "supabase.auth",
        business.result = Empty,
        otel.status_code = Empty,
    )
}

/// Record the stamp outcome. A FAILED stamp also sets OTel ERROR status (mob obligation on #437):
/// the contract's `success` rule requires `business.result == 'stamped'`, and without the ERROR
/// status a failed run would be unclassifiable by `status_rules` — neither success nor
/// `technical_error`.
pub fn record_claims_stamp_result(span: &Span, stamped: bool) {
    span.record(attr::RESULT, if stamped { "stamped" } else { "failed" });
    if !stamped {
        span.record("otel.status_code", "ERROR");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `f` with a subscriber that enables every level.
    ///
    /// Without one, `info_span!` returns a DISABLED span whose `metadata()` is `None` — so a test that
    /// inspected fields without this would pass vacuously no matter what the spans declared, which is
    /// worse than having no test. Learned the direct way: the first version of these two tests failed
    /// with `metadata() == None` rather than on any real assertion.
    fn with_subscriber<T>(f: impl FnOnce() -> T) -> T {
        let subscriber =
            tracing_subscriber::fmt().with_max_level(tracing::Level::TRACE).with_test_writer().finish();
        tracing::subscriber::with_default(subscriber, f)
    }

    /// A field must be DECLARED at span creation for `record` to work; `tracing` silently ignores a
    /// `record` for an undeclared field. That silence is the trap: the contract would list the
    /// attribute, the code would look like it sets it, and the span would ship without it. This asserts
    /// the late-bound fields really are declared, by recording them and checking the span is still the
    /// one we think it is.
    #[test]
    fn late_bound_fields_are_declared_on_their_spans() {
        with_subscriber(late_bound_fields_body);
    }

    fn late_bound_fields_body() {
        let journal = command_journal("m-1");
        record_journal_status(&journal, crate::contract::journal_status::RECEIVED);
        assert_eq!(journal.metadata().map(|m| m.name()), Some("command.journal"));
        assert!(journal.metadata().unwrap().fields().field(attr::JOURNAL_STATUS).is_some());

        let receive = command_receive("PlaceOrder", "CUSTOMER", "GRAPHQL");
        record_envelope(&receive, "m-1", "c-1");
        record_order_id(&receive, "o-1");
        let fields = receive.metadata().unwrap().fields();
        assert!(fields.field(attr::MESSAGE_ID).is_some(), "message_id is late-bound but declared");
        assert!(fields.field(attr::CORRELATION_ID).is_some(), "correlation_id is late-bound but declared");
        assert!(fields.field(attr::ORDER_ID).is_some(), "order_id is late-bound but declared");
        assert!(fields.field(attr::COMMAND_TYPE).is_some());
        assert!(fields.field(attr::ACTOR).is_some());
        assert!(fields.field(attr::CHANNEL).is_some());

        let payment = payment_intent_create();
        record_payment_result(&payment, "captured");
        assert!(payment.metadata().unwrap().fields().field(attr::RESULT).is_some());

        let pricing = pricing_compute();
        record_pricing(&pricing, 250, true);
        let pf = pricing.metadata().unwrap().fields();
        assert!(pf.field(attr::SERVICE_FEE).is_some());
        assert!(pf.field(attr::SPLIT_OK).is_some());

        let validate = command_validate();
        record_validation_status(&validate, "accepted");
        record_service_window_verdict(&validate, "OPEN");
        let vf = validate.metadata().unwrap().fields();
        assert!(vf.field(attr::VALIDATION_STATUS).is_some());
        assert!(
            vf.field(attr::SERVICE_WINDOW_VERDICT).is_some(),
            "service_window_verdict is late-bound but declared -- an undeclared field records \
             SILENTLY into nothing, and the place-order contract marks it required"
        );

        let promote = reminder_promote("OrderAcceptanceTimedOut", true);
        record_reminder_due(&promote, "2026-08-16T00:00:00Z", 1234);
        record_would_cancel(&promote, true);
        record_order_id(&promote, "o-1");
        let pf = promote.metadata().unwrap().fields();
        assert!(pf.field(attr::REMINDER_TYPE).is_some());
        assert!(pf.field(attr::SHADOW).is_some());
        assert!(pf.field(attr::DUE_AT).is_some(), "due_at is late-bound but declared");
        assert!(pf.field(attr::FIRE_DELAY_MS).is_some(), "fire_delay_ms is late-bound but declared");
        assert!(
            pf.field(attr::WOULD_CANCEL).is_some(),
            "would_cancel is late-bound but declared -- an undeclared field records SILENTLY \
             into nothing, and it is the flip ADR's whole evidence set"
        );
        assert!(pf.field(attr::ORDER_ID).is_some());

        let bridge = auth_read_scope("CUSTOMER");
        record_bridge_resolved(&bridge, false);
        let bf = bridge.metadata().unwrap().fields();
        assert!(bf.field(attr::BRIDGE_RESOLVED).is_some(), "bridge_resolved is late-bound but declared");
        assert!(bf.field(attr::CORRELATION_ID).is_some(), "correlation_id is late-bound but declared");

        let membership = auth_scope_membership("ORDER", "CUSTOMER");
        record_authorized(&membership, false);
        assert!(membership.metadata().unwrap().fields().field(attr::AUTHORIZED).is_some());

        let stamp = claims_stamp();
        record_claims_stamp_result(&stamp, false);
        let sf = stamp.metadata().unwrap().fields();
        assert!(sf.field(attr::RESULT).is_some(), "business.result is late-bound but declared");
        assert!(
            sf.field("otel.status_code").is_some(),
            "otel.status_code is late-bound but declared -- without it a failed stamp could not export ERROR status"
        );

        let price = cart_price("cart-1");
        record_correlation_id(&price, "corr-1");
        record_cart_price_error(&price);
        let cf = price.metadata().unwrap().fields();
        assert!(
            cf.field(attr::CORRELATION_ID).is_some(),
            "business.correlation_id is late-bound but declared"
        );
        assert!(
            cf.field("otel.status_code").is_some(),
            "otel.status_code is late-bound but declared -- without it the cart-price contract's \
             technical_error rule (any_span_errors) could NEVER fire and every unresolvable price \
             would classify as a success"
        );
    }

    /// The OTel span KIND is part of each contract (`kind: SERVER` etc). It travels as the `otel.kind`
    /// field, so a span missing that field exports as INTERNAL regardless of what the contract says.
    #[test]
    fn every_span_declares_its_otel_kind() {
        with_subscriber(every_span_kind_body);
    }

    fn every_span_kind_body() {
        let spans = [
            command_receive("C", "A", "GRAPHQL"),
            command_journal("m"),
            command_dispatch("m", crate::contract::dispatch_outcome::ENQUEUED),
            command_validate(),
            event_store_append("E", "s"),
            event_publish("E"),
            event_consume_projection("P"),
            payment_intent_create(),
            pricing_compute(),
            cart_read("a"),
            cart_price("a"),
            auth_read_scope("CUSTOMER"),
            auth_scope_membership("ORDER", "CUSTOMER"),
            claims_stamp(),
            reminder_promote("OrderAcceptanceTimedOut", true),
        ];
        for s in spans {
            let meta = s.metadata().expect("span has metadata");
            assert!(
                meta.fields().field("otel.kind").is_some(),
                "{} declares no otel.kind -- it would export as INTERNAL",
                meta.name()
            );
        }
    }
}
