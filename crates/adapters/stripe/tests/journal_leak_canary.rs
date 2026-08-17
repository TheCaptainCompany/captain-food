//! THE LEAK CANARY (#623 / #625): a provider-controlled string must never reach
//! `inbound_messages.error`.
//!
//! ## What this guards, and why it is not a hypothetical
//!
//! `inbound_messages.error` is a jsonb column that persists for the retention window (90 days) and
//! is served to callers as `Operation.errorCode` / `Operation.message`. Stripe's error `message` is
//! a **provider-controlled** string, and this adapter copies it VERBATIM into the `DomainError` it
//! returns (`outbound.rs::map_error`). On the catalogued arm the mailbox then copies that message
//! straight into `context.detail`.
//!
//! For an authentication failure Stripe's own message is literally
//! `"Invalid API Key provided: sk_test_…"`. The walk that produced #623 was running with a
//! placeholder key, so this is the exact live configuration in which the "obvious" fix — widen
//! `detail`, as ten handler sites already do — would have written key-shaped text into a 90-day
//! journal row.
//!
//! ## How it is built, and the three things it deliberately refuses
//!
//! - **not a live call** — the bodies below are recorded Stripe error envelopes;
//! - **not a hand-written error string** — every case goes through the REAL decoder
//!   (`decode_create_intent_response`), so a change to the adapter's classification is felt here;
//! - **not a database** — it asserts on `verdict_of_error`'s output, which `completion.rs` binds
//!   directly to the `error` column (`SET status = $2, error = $3` with `verdict.error()`). A
//!   DB-gated canary can SKIP, and a skipped canary on a secrets path is worse than no canary
//!   (`docs/claude/sessions.md` §"libtest captures a passing test's stderr").
//!
//! ## Honesty about the marker
//!
//! `sk_test_LEAK_CANARY` stands in for whatever secret-shaped text a provider message can carry.
//! The 401 envelope is real in BOTH shape and content — that is the message Stripe actually sends.
//! The 402 envelope is real in shape with the marker planted in the provider-controlled `message`
//! field: the leak is the VERBATIM COPY, and the copy is byte-identical on both arms. Which
//! particular Stripe error happens to carry a key today is not the invariant; "we never copy the
//! provider's prose into a durable, customer-servable row" is.
//!
//! ## The trap this canary exists to keep shut
//!
//! [#625](https://github.com/TheCaptainCompany/captain-food/issues/625) proposes declaring
//! `PaymentGatewayRefused` in `errors.yaml`. On today's code that would route the REAL 401 body —
//! the one whose message contains a real key — down the catalogued arm that copies `message` into
//! `context.detail`. That is a real key in a 90-day row, and it is one line of YAML away. This test
//! is what makes that land red instead of landing quietly.

use domain::shared::errors::DomainError;
use infrastructure::mailbox::verdict_of_error;
use stripe_adapter::outbound::decode_create_intent_response;

/// A key-shaped marker. Any occurrence of `sk_` in a journal row is a failure, whatever produced it.
const KEY_MARKER: &str = "sk_test_LEAK_CANARY";

/// One recorded Stripe error response: `(what it is, HTTP status, body)`.
struct RecordedRefusal {
    what: &'static str,
    status: u16,
    body: String,
}

fn recorded_refusals() -> Vec<RecordedRefusal> {
    vec![
        // Stripe's real authentication failure. `type: invalid_request_error` with no `code`, and
        // the key echoed in `message` — this is the body the #623 walk provoked.
        RecordedRefusal {
            what: "HTTP 401, invalid API key (the #623 walk's own failure)",
            status: 401,
            body: format!(
                r#"{{"error":{{"message":"Invalid API Key provided: {KEY_MARKER}","type":"invalid_request_error"}}}}"#
            ),
        },
        // A card decline: the catalogued `PaymentDeclined` arm. Same verbatim copy of a
        // provider-controlled `message`, but onto a code the catalogue knows — which is the arm
        // that keeps the string today.
        RecordedRefusal {
            what: "HTTP 402, card_declined (the catalogued arm)",
            status: 402,
            body: format!(
                r#"{{"error":{{"message":"Your card was declined. (trace {KEY_MARKER})","type":"card_error","code":"card_declined"}}}}"#
            ),
        },
        // A deterministic request refusal: the arm that produced the empty `{}` in #623.
        RecordedRefusal {
            what: "HTTP 400, parameter_missing (the deterministic-refusal arm)",
            status: 400,
            body: format!(
                r#"{{"error":{{"message":"No such payment_method (key {KEY_MARKER})","type":"invalid_request_error","code":"parameter_missing"}}}}"#
            ),
        },
    ]
}

/// The journal row's `error` JSON for one recorded refusal, produced by the real decoder and the
/// real verdict mapping — nothing between them is re-implemented here.
fn journal_error_json(refusal: &RecordedRefusal) -> serde_json::Value {
    let err: DomainError = decode_create_intent_response(refusal.status, &refusal.body)
        .expect_err("a non-2xx Stripe body must decode to an error");
    verdict_of_error(err)
        .error()
        .cloned()
        .unwrap_or_else(|| panic!("{}: a refusal must produce a terminal error verdict", refusal.what))
}

/// THE CANARY. Every recorded provider refusal, through the real decoder, into the journal row's
/// `error` JSON: no key-shaped text may survive the trip.
#[test]
fn no_provider_secret_reaches_the_journal_row() {
    let mut leaked: Vec<String> = Vec::new();
    for refusal in recorded_refusals() {
        let json = journal_error_json(&refusal);
        let rendered = serde_json::to_string(&json).expect("journal error json is serialisable");
        if rendered.contains("sk_") {
            leaked.push(format!("  {} -> {rendered}", refusal.what));
        }
    }
    assert!(
        leaked.is_empty(),
        "a provider-controlled string reached `inbound_messages.error` — a jsonb column that \
         persists 90 days and is served as Operation.errorCode/message:\n{}\n\
         Free text belongs in the log, never in the journal row. Attribute the failure with the \
         bounded shape (`mailbox::attribution`) instead of copying the provider's prose.",
        leaked.join("\n")
    );
}

/// The canary's own fixture must be able to fail: if the marker ever stops appearing in what the
/// decoder produces, the test above passes for the wrong reason (nothing to leak). This asserts the
/// provider text really does survive as far as the `DomainError` — the input side of the leak.
#[test]
fn the_provider_message_really_does_reach_the_domain_error() {
    for refusal in recorded_refusals() {
        let err = decode_create_intent_response(refusal.status, &refusal.body)
            .expect_err("a non-2xx Stripe body must decode to an error");
        let msg = match &err {
            DomainError::Invariant(m) | DomainError::Repository(m) => m.clone(),
            DomainError::Rejected { code, context } => format!("{code} {context}"),
        };
        assert!(
            msg.contains(KEY_MARKER),
            "{}: the fixture is inert — the decoder no longer carries the provider message, so \
             `no_provider_secret_reaches_the_journal_row` would pass vacuously. Got: {msg}",
            refusal.what
        );
    }
}
