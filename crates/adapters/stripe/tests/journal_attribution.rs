//! TWO SEAMS, ONE TEST (#623 M1) — a failed command must be ATTRIBUTABLE from its journal row.
//!
//! ## Why this is one test and not two
//!
//! The defect #623 reports is not "the context is empty". It is that **two failures with completely
//! different operational responses record the same row**: "Stripe is refusing us" and "the stored
//! payload does not fit the deployed binary" were byte-identical at `{"code":"Internal","context":
//! {}}`, and at Friday peak that difference is the entire diagnosis.
//!
//! DISTINCTNESS is a relational property, so it is inexpressible in a per-seam test. Split this into
//! two tests each asserting its own constant and the mutation that restates the actual defect —
//! label one seam the same as the other — leaves BOTH green if either test asserts only "the context
//! names a seam". Splitting them *is* the vacuity. The mutations this file is written against are
//! recorded in `docs/dispatch/623-placeorder-unattributable.md`; each was run and each turns this
//! test red.
//!
//! ## Why THESE two seams
//!
//! The dispatch card proposed "gateway rejection" against "the cart read". The read seam turned out
//! to be undrivable: `DomainError::Repository` is intercepted above every one of
//! `verdict_of_error`'s three call sites — in `handler.rs` for the two in-transaction arms, and in
//! `pm_delivery::prepare` for the PM arm, which is `PlaceOrder`'s. Measured, not read: see the card's
//! Findings. The second seam is therefore **`COMMAND_PAYLOAD`**, which is live, terminal and
//! genuinely distinct: a journaled payload that does not decode into the deployed command shape
//! (`pm_delivery.rs`'s `PlaceOrder`/`ApproveRefund`/`DenyRefund` arms) — the shape a version skew
//! between an enqueued row and a rolled binary produces.
//!
//! ## What is asserted, and what is deliberately not
//!
//! Never the prose. Four independently falsifiable claims about the context's DISCRIMINATING POWER:
//! distinctness, a declared key whose value is a member of the declared closed set, the gateway
//! status present on one arm and ABSENT on the other, and no provider secret anywhere.

use application::commands::rejection_code;
use domain::generated::scalars::{CommandFailureReason, CommandFailureSeam};
use domain::shared::errors::DomainError;
use infrastructure::mailbox::attribution::command_payload_undecodable;
use infrastructure::mailbox::verdict_of_error;
use stripe_adapter::outbound::decode_create_intent_response;

/// SEAM 1 — the gateway refused deterministically. A recorded Stripe 401 (bad credentials) through
/// the REAL decoder: the failure the #623 walk actually provoked.
fn gateway_refusal() -> DomainError {
    decode_create_intent_response(
        401,
        r#"{"error":{"message":"Invalid API Key provided: sk_test_LEAK_CANARY","type":"invalid_request_error"}}"#,
    )
    .expect_err("a 401 must decode to an error")
}

/// SEAM 2 — the journaled payload does not fit the deployed command shape. Built the same way
/// `pm_delivery.rs` builds it, from a real `serde_json` failure against the real generated command,
/// so a change to either the command shape or the builder is felt here.
fn payload_undecodable() -> DomainError {
    let err = serde_json::from_value::<domain::generated::commands::PlaceOrder>(
        serde_json::json!({ "cartId": "not-a-uuid-shaped-thing" }),
    )
    .expect_err("a truncated payload must fail to decode");
    command_payload_undecodable("PlaceOrder", &err.to_string())
}

/// The journal row's `error` JSON — exactly what `completion.rs` binds to `inbound_messages.error`.
fn journal_error(err: DomainError) -> serde_json::Value {
    verdict_of_error(err).error().cloned().expect("a failure must record an error")
}

/// THE TEST. Two seams, one context each, and the four claims that make the pair useful.
#[test]
fn two_failing_seams_record_two_distinguishable_journal_rows() {
    let gateway = journal_error(gateway_refusal());
    let payload = journal_error(payload_undecodable());

    // The FENCE first: this chunk changes what the row says, never what the row means. Both
    // failures were FAILED/Internal before #623 and both still are.
    for (what, row) in [("gateway", &gateway), ("payload", &payload)] {
        assert_eq!(
            row.get("code").and_then(|c| c.as_str()),
            Some("Internal"),
            "{what}: the verdict must be unchanged by #623 — a new code here would flip FAILED to \
             REJECTED, because catalogue membership is the discriminator.\n{row}"
        );
    }

    let gateway_ctx = gateway.get("context").expect("gateway row has a context");
    let payload_ctx = payload.get("context").expect("payload row has a context");

    // CLAIM 1 — DISTINCTNESS. The whole defect, stated as a property. Red under "label one seam the
    // same as the other", and red under "restore the empty context" (two empties are equal).
    assert_ne!(
        gateway_ctx, payload_ctx,
        "two seams, one context: the failure is recorded but not attributable. A gateway refusal \
         and an undecodable payload need different operational responses and the row distinguishes \
         neither.\n  gateway: {gateway_ctx}\n  payload: {payload_ctx}"
    );
    assert_ne!(
        gateway_ctx.get("seam"),
        payload_ctx.get("seam"),
        "the two rows differ, but not in the field an operator triages on.\n  gateway: \
         {gateway_ctx}\n  payload: {payload_ctx}"
    );

    // CLAIM 2 — A DECLARED KEY WITH A CLOSED-SET VALUE. Round-tripping through the generated scalar
    // is what proves the value is a MEMBER of the declared set rather than a string that reads like
    // one: an invented `"STRIPE"` would deserialize to nothing.
    for (what, ctx, expect_seam, expect_reason) in [
        (
            "gateway",
            gateway_ctx,
            CommandFailureSeam::PAYMENT_GATEWAY,
            CommandFailureReason::GATEWAY_REFUSED,
        ),
        (
            "payload",
            payload_ctx,
            CommandFailureSeam::COMMAND_PAYLOAD,
            CommandFailureReason::PAYLOAD_UNDECODABLE,
        ),
    ] {
        let seam: CommandFailureSeam = serde_json::from_value(
            ctx.get("seam").unwrap_or(&serde_json::Value::Null).clone(),
        )
        .unwrap_or_else(|e| panic!("{what}: `seam` is not a member of the declared closed set: {e}\n{ctx}"));
        let reason: CommandFailureReason = serde_json::from_value(
            ctx.get("reason").unwrap_or(&serde_json::Value::Null).clone(),
        )
        .unwrap_or_else(|e| panic!("{what}: `reason` is not a member of the declared closed set: {e}\n{ctx}"));
        assert_eq!(seam, expect_seam, "{what}: wrong seam\n{ctx}");
        assert_eq!(reason, expect_reason, "{what}: wrong reason\n{ctx}");
    }

    // CLAIM 3 — THE GATEWAY STATUS, PRESENT ON ONE ARM AND ABSENT ON THE OTHER. 401 is the
    // discrimination that matters most here: our credentials, not the customer's card and not our
    // request. Its ABSENCE on the payload arm is equally load-bearing — a status there would mean
    // the classifier had guessed, and `null` would read as "we looked and found none".
    assert_eq!(
        gateway_ctx.get("gatewayStatus").and_then(|v| v.as_i64()),
        Some(401),
        "the gateway seam must carry the status it refused with\n{gateway_ctx}"
    );
    assert!(
        payload_ctx.get("gatewayStatus").is_none(),
        "a payload-decode failure has no gateway status: the key must be ABSENT, not null\n{payload_ctx}"
    );

    // CLAIM 4 — THE CANARY, restated at the attribution site. `journal_leak_canary.rs` owns this
    // invariant across every recorded refusal; it is repeated here because THIS is the diff that
    // populates the context, and a leak introduced by an attribution field must fail the
    // attribution test, not only a neighbouring one.
    for (what, ctx) in [("gateway", gateway_ctx), ("payload", payload_ctx)] {
        let rendered = ctx.to_string();
        assert!(
            !rendered.contains("sk_"),
            "{what}: provider-controlled text reached the journal row\n{rendered}"
        );
    }
}

/// The gateway seam's error must stay readable as a (non-catalogued) code. If it ever becomes
/// catalogued — [#625](https://github.com/TheCaptainCompany/captain-food/issues/625) proposes
/// exactly that — the lookup has to find `PaymentGatewayRefused`, and the HTTP status folded into
/// the message must not corrupt it. Getting this wrong flips a FAILED into a REJECTED silently,
/// which is the one outcome this whole change is fenced against.
#[test]
fn the_gateway_refusal_still_reads_as_an_uncatalogued_code() {
    let err = gateway_refusal();
    assert_eq!(rejection_code(&err), Some("PaymentGatewayRefused"), "{err:?}");
    assert!(
        domain::generated::errors::find("PaymentGatewayRefused").is_none(),
        "PaymentGatewayRefused became a catalogued code — that flips this path's verdict from \
         FAILED to REJECTED. It is deferred to #625 on purpose and must not ride along."
    );
}
