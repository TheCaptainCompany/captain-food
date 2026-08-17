//! WHAT A FAILED COMMAND LEAVES BEHIND (#623) — bounded by construction, not by review.
//!
//! ## The defect, and why the obvious fix was the wrong one
//!
//! A failed `PlaceOrder` recorded `{"code":"Internal","context":{}}` and nothing at ERROR or WARN.
//! The diagnostic string existed — the Stripe adapter had produced
//! `PaymentGatewayRefused: (HTTP 401) …` — and was discarded three lines from a sibling branch
//! that kept it. Ten handler sites already spell the "obvious" repair, `context: { detail:
//! e.to_string() }`.
//!
//! That repair leaks. `inbound_messages.error` is a jsonb column that persists for the retention
//! window, and Stripe's error `message` is provider-controlled: for an authentication failure it is
//! literally `"Invalid API Key provided: sk_test_…"`. The `{}` that #623 reports is, measurably,
//! the only branch on that path that was NOT already leaking — the catalogued arm copied the
//! provider's prose verbatim (`crates/adapters/stripe/tests/journal_leak_canary.rs` proves it).
//!
//! **Where it leaked TO, exactly** (`young`, #623 review — in a chunk about unverified claims, an
//! earlier draft of this paragraph was itself one). Not to callers: no `errors.yaml` en/fr template
//! in any scope fragment interpolates `{detail}` — the complete placeholder set across every
//! fragment is 21 tokens and `detail` is not among them — and `context` was never on the wire.
//! It leaked to **the column and therefore to the backups**, which is enough: a secret in a durable
//! store is a secret you must now rotate and prove gone, and nothing about that needs a reader.
//!
//! ## The fence, and why it is a TYPE and not a rule
//!
//! [`domain::generated::entities::CommandFailureAttribution`] is generated from
//! `specs/common/entities.yaml` and every one of its fields is a closed set or a number. There is
//! no `String` to put a provider body in, so the leak is **unspellable** rather than forbidden —
//! CLAUDE.md's compiler-first floor. Widening it takes a spec change, a regenerate and a
//! `SPEC-LOG.md` sentence, which is exactly the amount of friction "let me just add a detail field"
//! should meet.
//!
//! The free text has not been thrown away: it goes to the LOG at the discard site in `handler.rs`,
//! which is where an operator with a correlation id can find it and where nothing retains it for
//! ninety days.
//!
//! ## What is deliberately NOT here
//!
//! - **No new `errors.yaml` code.** Catalogue membership is the REJECTED-vs-FAILED discriminator in
//!   [`super::handler::verdict_of_error`], so declaring one flips a verdict silently.
//! - **No typed gateway-failure enum on `DomainError`.** That is
//!   [#625](https://github.com/TheCaptainCompany/captain-food/issues/625): it would let the adapter
//!   hand the status over as a field instead of encoding it in a message the reader parses back.
//!   The builder/reader pair in `application::ports` is the floor available without reshaping a
//!   kernel type.

use application::commands::rejection_code;
use application::ports::{gateway_status_of, is_payment_declined, is_payment_gateway_refused};
use domain::generated::entities::CommandFailureAttribution;
use domain::generated::scalars::{
    CommandFailureReason as Reason, CommandFailureSeam as Seam, GatewayStatusCode,
};
use domain::shared::errors::DomainError;

/// Message prefix for a journaled command payload that does not fit the deployed command shape.
///
/// Shared for the same reason as `application::ports::PAYMENT_GATEWAY_REFUSED_PREFIX`: it is minted
/// in `pm_delivery.rs` and read here, and until #623 that coupling was two independent string
/// literals in two files. It sits BEFORE the first colon so [`rejection_code`] still reads a stable
/// (non-catalogued) code from it.
pub const COMMAND_PAYLOAD_PREFIX: &str = "command payload";

/// Build the canonical undecodable-payload failure. Deterministic by construction — the bytes in
/// the row will not decode on the next attempt either — so it is an `Invariant` (terminal), never a
/// `Repository` (retry).
pub fn command_payload_undecodable(command_type: &str, detail: &str) -> DomainError {
    DomainError::Invariant(format!(
        "{COMMAND_PAYLOAD_PREFIX}: {command_type} did not decode: {detail}"
    ))
}

/// Whether `err` is the undecodable-payload failure built by [`command_payload_undecodable`].
pub fn is_command_payload_undecodable(err: &DomainError) -> bool {
    matches!(err, DomainError::Invariant(msg) if msg.starts_with(COMMAND_PAYLOAD_PREFIX))
}

/// Attribute a handler failure that is about to be recorded as the row's terminal verdict.
///
/// The classification is COARSE on purpose: each member of
/// [`domain::generated::scalars::CommandFailureSeam`] earns its place by changing what an operator
/// does next. "Stripe is refusing us" and "our database is wedged" were the same string before this
/// existed, and at peak that difference is the whole diagnosis.
///
/// It reads the error, never the call site, because the call site cannot tell a gateway refusal
/// from an aggregate invariant — both arrive as `DomainError::Invariant` from the same `await`.
pub fn attribute(err: &DomainError) -> CommandFailureAttribution {
    // The CATALOGUED classification first, so `Reason::CARD_DECLINED` has exactly one producer and
    // the two entry points ([`catalogued_context`] and this one) can never disagree about it.
    if let Some(attribution) = attribute_catalogued(err) {
        return attribution;
    }
    if is_payment_gateway_refused(err) {
        return CommandFailureAttribution {
            seam: Seam::PAYMENT_GATEWAY,
            reason: Reason::GATEWAY_REFUSED,
            gateway_status: gateway_status_of(err).map(|s| GatewayStatusCode(s.into())),
        };
    }
    if is_command_payload_undecodable(err) {
        return CommandFailureAttribution {
            seam: Seam::COMMAND_PAYLOAD,
            reason: Reason::PAYLOAD_UNDECODABLE,
            // No call was made, so there is no status. Its ABSENCE here is asserted by the
            // two-seam test: a status on a payload-decode failure would mean the classifier guessed.
            gateway_status: None,
        };
    }
    match err {
        // Unreachable on every live path, and MEASURED so rather than assumed (#623): every one of
        // `verdict_of_error`'s three call sites has a `DomainError::Repository` interception above
        // it — `handler.rs` for the two in-transaction arms, and `pm_delivery::prepare` for the PM
        // arm, which is `PlaceOrder`'s. The arm stays because the enum has it, and it is attributed
        // rather than dropped so that if an interception is ever removed the row says which one.
        DomainError::Repository(_) => CommandFailureAttribution {
            seam: Seam::INFRASTRUCTURE,
            reason: Reason::TRANSIENT_INFRASTRUCTURE,
            gateway_status: None,
        },
        // The GENERAL CASE, and the one worth naming: an invariant nobody declared. It is a DEFECT
        // rather than a refusal — a business outcome arriving here is a missing `errors.yaml`
        // declaration, and the row saying so is how anyone finds out. Asserted in `tests` below;
        // it is the least exotic input on this whole path and was the one nothing covered.
        _ => CommandFailureAttribution {
            seam: Seam::DOMAIN_INVARIANT,
            reason: Reason::UNCATALOGUED_INVARIANT,
            gateway_status: None,
        },
    }
}

/// Attribute a CATALOGUED refusal — one whose `errors.yaml` code already names the outcome.
///
/// `None` means "nothing to add beyond the code", which is the honest answer for most of the
/// catalogue: the code IS the attribution and `errors.yaml` declares the context empty.
///
/// The one member that earns more is the card decline, and it earns it by EVIDENCE: the gateway's
/// HTTP status, which exists only if a gateway actually answered. That is deliberately strict —
/// `infrastructure::integrations::payments`'s fail-closed stand-in mints the same catalogued code
/// when NO gateway is configured, and attributing "we never called anyone" as "the customer's card
/// was declined" is precisely the misattribution #623 exists to stop. No status, no claim.
fn attribute_catalogued(err: &DomainError) -> Option<CommandFailureAttribution> {
    if !is_payment_declined(err) {
        return None;
    }
    Some(CommandFailureAttribution {
        seam: Seam::PAYMENT_GATEWAY,
        reason: Reason::CARD_DECLINED,
        gateway_status: Some(GatewayStatusCode(gateway_status_of(err)?.into())),
    })
}

/// The `context` a CATALOGUED rejection records alongside its code.
///
/// Before the #623 review this was the literal `{}` — safe, because the provider prose it replaced
/// was the live leak, but a downgrade: a declined card left a row saying `PaymentDeclined` and
/// nothing else, so "declined how?" needed the log rather than the row, on the money path. It now
/// carries the same bounded [`CommandFailureAttribution`] the FAILED arm does, which is also why
/// there is no second shape for a reader to learn.
pub fn catalogued_context(err: &DomainError) -> serde_json::Value {
    match attribute_catalogued(err) {
        Some(attribution) => context_of(&attribution),
        None => serde_json::json!({}),
    }
}

/// The attribution as the journal row's `error.context`.
///
/// Built key-by-key rather than by `serde_json::to_value` for one reason: an absent gateway status
/// must be ABSENT, not `null`. `null` reads as "we looked and there was none"; absence reads as
/// "this seam has no such thing", and the two-seam test asserts the difference.
pub fn context_of(attribution: &CommandFailureAttribution) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert("seam".into(), json_of_seam(attribution.seam));
    map.insert("reason".into(), json_of_reason(attribution.reason));
    if let Some(status) = &attribution.gateway_status {
        map.insert("gatewayStatus".into(), serde_json::json!(status.0));
    }
    serde_json::Value::Object(map)
}

/// The seam's declared spelling. An exhaustive `match` rather than `serde_json::to_value`: adding a
/// member to the scalar must not compile until this file has been told what to call it.
fn json_of_seam(seam: Seam) -> serde_json::Value {
    serde_json::Value::String(
        match seam {
            Seam::PAYMENT_GATEWAY => "PAYMENT_GATEWAY",
            Seam::COMMAND_PAYLOAD => "COMMAND_PAYLOAD",
            Seam::DOMAIN_INVARIANT => "DOMAIN_INVARIANT",
            Seam::INFRASTRUCTURE => "INFRASTRUCTURE",
        }
        .to_string(),
    )
}

/// The reason's declared spelling — same exhaustiveness argument as [`json_of_seam`].
fn json_of_reason(reason: Reason) -> serde_json::Value {
    serde_json::Value::String(
        match reason {
            Reason::GATEWAY_REFUSED => "GATEWAY_REFUSED",
            Reason::CARD_DECLINED => "CARD_DECLINED",
            Reason::PAYLOAD_UNDECODABLE => "PAYLOAD_UNDECODABLE",
            Reason::UNCATALOGUED_INVARIANT => "UNCATALOGUED_INVARIANT",
            Reason::TRANSIENT_INFRASTRUCTURE => "TRANSIENT_INFRASTRUCTURE",
        }
        .to_string(),
    )
}

/// The full diagnostic text for the LOG — never for the journal row. Separated out so the two
/// destinations are visibly different calls at the discard site: whatever ends up here is exactly
/// what does NOT end up in `context_of`.
pub fn log_detail(err: &DomainError) -> String {
    match err {
        DomainError::Invariant(msg) | DomainError::Repository(msg) => msg.clone(),
        DomainError::Rejected { code, context } => format!("{code} {context}"),
    }
}

/// The errors.yaml code this error carries, if the catalogue knows it. Delegates to
/// [`rejection_code`] rather than re-splitting the message: `verdict_of_error` used to carry its
/// own copy of that parse, which is two places for one protocol.
pub fn catalogued_code(err: &DomainError) -> Option<&str> {
    rejection_code(err).filter(|code| domain::generated::errors::find(code).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::ports::{payment_declined, payment_gateway_refused};

    /// EVERY declared seam must have a producer, and this is the executable form of that rule.
    ///
    /// The `match` is exhaustive, so adding a member to `CommandFailureSeam` does not compile until
    /// someone writes down which error produces it; the `assert` then fails until one actually
    /// does. That pairing is the whole point — a declared-but-unemitted member is the defect class
    /// this module exists to fix, and #623 shipped one of its own (`EVENT_APPEND`, withdrawn from
    /// the scalar in review because its producers are [#628]'s staged-flush arms, out of scope
    /// here). A value nothing can emit is a dashboard filter that is always empty and a runbook
    /// paragraph nobody can trigger.
    #[test]
    fn every_declared_seam_has_a_producer() {
        for seam in [
            Seam::PAYMENT_GATEWAY,
            Seam::COMMAND_PAYLOAD,
            Seam::DOMAIN_INVARIANT,
            Seam::INFRASTRUCTURE,
        ] {
            let driver: DomainError = match seam {
                Seam::PAYMENT_GATEWAY => payment_gateway_refused(Some(401), "stripe refused"),
                Seam::COMMAND_PAYLOAD => command_payload_undecodable("PlaceOrder", "missing field"),
                Seam::DOMAIN_INVARIANT => DomainError::Invariant("cart is empty".into()),
                Seam::INFRASTRUCTURE => DomainError::Repository("connection reset".into()),
            };
            assert_eq!(
                attribute(&driver).seam,
                seam,
                "no error in this repository produces {seam:?}; either produce it or remove it \
                 from specs/common/scalars.yaml#/CommandFailureSeam"
            );
        }
    }

    /// The `_` arm is the GENERAL CASE of the bug this module fixes, not a defensive default, and
    /// until the #623 review nothing asserted it — the two covered seams were both exotic.
    #[test]
    fn an_undeclared_invariant_is_a_domain_defect_not_a_refusal() {
        let attributed = attribute(&DomainError::Invariant("order already accepted".into()));
        assert_eq!(attributed.seam, Seam::DOMAIN_INVARIANT);
        assert_eq!(attributed.reason, Reason::UNCATALOGUED_INVARIANT);
        assert!(attributed.gateway_status.is_none(), "no call was made");
    }

    /// Measured-dead, not assumed-dead (#623): every `verdict_of_error` call site intercepts
    /// `Repository` above it. The arm is still asserted, because "unreachable today" is a property
    /// of three interceptions in two files and the day one is removed this row is the evidence.
    #[test]
    fn a_repository_failure_is_attributed_to_infrastructure() {
        let attributed = attribute(&DomainError::Repository("pool timed out".into()));
        assert_eq!(attributed.seam, Seam::INFRASTRUCTURE);
        assert_eq!(attributed.reason, Reason::TRANSIENT_INFRASTRUCTURE);
        assert!(attributed.gateway_status.is_none());
    }

    /// A card decline is a REJECTED business outcome, and its row now says which gateway answer it
    /// was. `context: {}` was the safe answer in the first draft, not the right one.
    #[test]
    fn a_card_decline_carries_its_gateway_status() {
        let err = payment_declined(Some(402), "Your card was declined. (card_declined)");
        let context = catalogued_context(&err);
        assert_eq!(context["seam"], "PAYMENT_GATEWAY");
        assert_eq!(context["reason"], "CARD_DECLINED");
        assert_eq!(context["gatewayStatus"], 402);
        assert!(
            !context.to_string().contains("declined."),
            "the provider's prose must not reach the row: {context}"
        );
    }

    /// The strictness that keeps the previous test honest: the fail-closed stand-in mints the SAME
    /// catalogued code with no gateway behind it, and calling that "the customer's card was
    /// declined" would be a misattribution on the money path.
    #[test]
    fn a_decline_with_no_gateway_behind_it_claims_nothing() {
        let stand_in = DomainError::Invariant(
            "PaymentDeclined: payment gateway not configured (fail-closed stand-in)".into(),
        );
        assert_eq!(catalogued_context(&stand_in), serde_json::json!({}));
    }

    /// Any other catalogued code keeps the empty context `errors.yaml` declares for it.
    #[test]
    fn a_catalogued_code_that_is_not_a_decline_adds_nothing() {
        let err = DomainError::Invariant("NotAParticipant: not your order".into());
        assert_eq!(catalogued_context(&err), serde_json::json!({}));
    }
}
