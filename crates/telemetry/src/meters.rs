//! The metric instruments the contracts declare, kept separate from the spans.
//!
//! `specs/observability.yaml` splits `metrics` (technical) from `business_metrics` (BAM) and says the
//! split is a requirement, not a style choice: BAM answers "is the business working" and must not be
//! polluted by retry counts. That split is mirrored in the two modules below.
//!
//! Instruments are built once behind `OnceLock`. Creating an instrument per call would allocate on every
//! command and — worse — is how duplicate time series with inconsistent attribute sets appear.

use std::sync::OnceLock;

use opentelemetry::metrics::{Counter, Histogram, Meter};
use opentelemetry::KeyValue;

use crate::contract::metric;

fn meter() -> &'static Meter {
    static METER: OnceLock<Meter> = OnceLock::new();
    METER.get_or_init(|| opentelemetry::global::meter("captain-food"))
}

/// Technical metrics for the `command-acceptance` contract.
pub mod acceptance {
    use super::*;

    fn accepted_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::COMMANDS_ACCEPTED_TOTAL).build())
    }

    fn duplicates_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::COMMAND_DUPLICATES_TOTAL).build())
    }

    fn conflicts_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::COMMAND_SYNC_CONFLICTS_TOTAL).build())
    }

    fn completion_histogram() -> &'static Histogram<f64> {
        static H: OnceLock<Histogram<f64>> = OnceLock::new();
        H.get_or_init(|| {
            meter().f64_histogram(metric::COMMAND_COMPLETION_MS).with_unit("ms").build()
        })
    }

    /// One accepted command submission (`commands_accepted_total{channel}`).
    pub fn accepted(channel: &str) {
        accepted_counter().add(1, &[KeyValue::new("channel", channel.to_string())]);
    }

    /// An idempotent replay: same `messageId`, same payload (`command_duplicates_total{channel}`).
    ///
    /// This is a SUCCESSFUL acceptance, counted separately — the contract's comment calls it "client
    /// retry correctness". A rising duplicate rate is a client retrying more than it should, which is
    /// healthy behaviour to see and unhealthy to ignore.
    pub fn duplicate(channel: &str) {
        duplicates_counter().add(1, &[KeyValue::new("channel", channel.to_string())]);
        // Counted as accepted too: from the caller's perspective the submission WAS accepted (the
        // original acceptance is replayed), so excluding it would understate the accept rate.
        accepted(channel);
    }

    /// `messageId` reused with a DIFFERENT payload — a client bug, never a retry
    /// (`command_sync_conflicts_total{command_type}`).
    pub fn sync_conflict(command_type: &str) {
        conflicts_counter().add(1, &[KeyValue::new("command_type", command_type.to_string())]);
    }

    /// Journal insert → terminal status, in ms (`command_completion_ms{status}`), where status is
    /// SUCCEEDED | REJECTED | FAILED. REJECTED and FAILED stay split: a business rejection is the
    /// system working, a technical failure is not, and averaging them together hides both.
    pub fn completed(status: &str, elapsed_ms: f64) {
        completion_histogram()
            .record(elapsed_ms, &[KeyValue::new("status", status.to_string())]);
    }
}

/// The `place-order` contract: one technical histogram plus the two BAM counters.
pub mod place_order {
    use super::*;

    fn duration_histogram() -> &'static Histogram<f64> {
        static H: OnceLock<Histogram<f64>> = OnceLock::new();
        H.get_or_init(|| {
            meter().f64_histogram(metric::PLACE_ORDER_DURATION_MS).with_unit("ms").build()
        })
    }

    fn placed_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::ORDERS_PLACED_TOTAL).build())
    }

    fn payment_failures_counter() -> &'static Counter<u64> {
        static C: OnceLock<Counter<u64>> = OnceLock::new();
        C.get_or_init(|| meter().u64_counter(metric::CHECKOUT_PAYMENT_FAILURES_TOTAL).build())
    }

    /// `place_order_duration_ms{result}` — the checkout saga end to end.
    pub fn duration(result: &str, elapsed_ms: f64) {
        duration_histogram().record(elapsed_ms, &[KeyValue::new("result", result.to_string())]);
    }

    /// BUSINESS metric: `orders_placed_total{status}`.
    pub fn placed(status: &str) {
        placed_counter().add(1, &[KeyValue::new("status", status.to_string())]);
    }

    /// BUSINESS metric: `checkout_payment_failures_total{reason}`.
    ///
    /// The one number that answers "did we take money and fail to tell anyone" at a glance — the worst
    /// failure mode this product has.
    pub fn payment_failure(reason: &str) {
        payment_failures_counter().add(1, &[KeyValue::new("reason", reason.to_string())]);
    }
}

#[cfg(test)]
mod tests {
    /// Recording against the global no-op provider (no `init`) must not panic. This is the state of
    /// every unit test and every local run without an ingest key, so if it panicked, instrumenting a
    /// boundary would break the test suite and the instrumentation would get reverted rather than fixed.
    #[test]
    fn recording_without_an_initialised_provider_is_a_no_op() {
        super::acceptance::accepted("GRAPHQL");
        super::acceptance::duplicate("WORKER");
        super::acceptance::sync_conflict("PlaceOrder");
        super::acceptance::completed("SUCCEEDED", 12.5);
        super::place_order::duration("captured", 431.0);
        super::place_order::placed("PLACED");
        super::place_order::payment_failure("card_declined");
    }
}
