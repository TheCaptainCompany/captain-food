//! THE SECRET-SHAPE PREDICATE: what a leaked Stripe credential looks like in a string.
//!
//! # Why this is a module and not a `contains("sk_")` inside one test
//!
//! It was one, in `tests/journal_leak_canary.rs` (#623). The problem is not that the literal is
//! ugly — it is that the NEXT leak site re-authors it. [#627] is the same defect one tier worse
//! (500 characters of a provider message into `PaymentCaptureFailed.detail`, a STORED EVENT that is
//! immutable, permanent and replayed), and whoever writes its canary writes this predicate again
//! from memory. A predicate written from memory is written narrower: `sk_` is the one everybody
//! remembers, and it is not the only credential Stripe issues.
//!
//! So the list lives here, once, and adding a leak canary is a call rather than a recollection.
//!
//! # What is on the list, and why `sk_live_` is on it twice
//!
//! `sk_live_` is subsumed by `sk_` and is listed anyway. That redundancy is deliberate: this list
//! is read by someone asking *"is the live secret key covered?"*, and the answer has to be visible
//! rather than derived. The cost of the extra entry is one redundant `starts_with`; the cost of its
//! absence is a reader concluding it is not covered and adding a second, narrower list somewhere
//! else.
//!
//! `pk_live_` is a PUBLISHABLE key and is not a secret. It is here because a live publishable key
//! appearing in an error row means a live-mode credential reached a diagnostic path, which is worth
//! the same alarm even though the value itself is safe to publish.
//!
//! # What this is NOT
//!
//! Not a redaction function, and deliberately not one. It answers "did a credential shape reach a
//! place it must never reach", which is a TEST question with a binary answer. A redactor invites
//! the wrong fix — sanitise the free text and keep writing it — where the fix that holds is the one
//! #623 landed: a shape with nowhere to put free text at all. Redaction is a filter someone can
//! forget; a type with no `String` field is not.
//!
//! [#627]: https://github.com/TheCaptainCompany/captain-food/issues/627

/// The credential shapes Stripe issues, as they appear at the start of the value.
///
/// Ordered most-general first so [`provider_secret_in`] reports the broadest match it can — a
/// finding that says `sk_` is as actionable as one that says `sk_live_`, and the general entry is
/// the one that catches a prefix nobody thought of.
pub const PROVIDER_SECRET_PREFIXES: [&str; 5] = ["sk_", "rk_", "whsec_", "pk_live_", "sk_live_"];

/// The first credential shape found anywhere in `haystack`, if any.
///
/// Substring rather than prefix matching, because the string under test is a rendered journal row
/// or event payload and the credential is embedded in provider prose:
/// `"Invalid API Key provided: sk_test_…"`.
pub fn provider_secret_in(haystack: &str) -> Option<&'static str> {
    PROVIDER_SECRET_PREFIXES.into_iter().find(|prefix| haystack.contains(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every listed shape is actually detected. A list nothing exercises is a list that can lose an
    /// entry in a refactor without anything noticing — the same class of defect as a declared enum
    /// member with no producer.
    #[test]
    fn every_listed_prefix_is_detected() {
        for prefix in PROVIDER_SECRET_PREFIXES {
            let embedded = format!("Invalid API Key provided: {prefix}abc123def");
            assert!(
                provider_secret_in(&embedded).is_some(),
                "{prefix} is on the list but is not detected in {embedded:?}"
            );
        }
    }

    /// Stripe's real 401 message, which is the whole reason this exists.
    #[test]
    fn the_real_authentication_failure_message_is_caught() {
        assert_eq!(
            provider_secret_in("Invalid API Key provided: sk_test_51H8xY2abcdef"),
            Some("sk_")
        );
    }

    /// Ordinary provider prose is not a finding. A predicate that fires on everything is one that
    /// gets an exclusion added to it, and an exclusion is how a canary comes to certify a leak.
    #[test]
    fn ordinary_provider_prose_is_not_a_finding() {
        for benign in [
            "Your card was declined.",
            "No such payment_method: 'pm_1234'",
            "an idempotency key was reused with different parameters",
            "",
        ] {
            assert_eq!(provider_secret_in(benign), None, "false positive on {benign:?}");
        }
    }
}
