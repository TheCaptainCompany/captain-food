//! HAND-WRITTEN pin over the GENERATED `REMINDER_SCHEDULES` table (#167 Phase 0, beck's net).
//!
//! The generated behaviour tests regenerate FROM the same emitter as the table, so they CANNOT
//! see the duration-widening go wrong — a days→seconds emitter mutation regenerates table and
//! tests together and stays green. This test is the independent witness: it pins the exact row
//! set, each window as a typed `Duration`, and each reschedule policy. Deliberately
//! STRUCTURE-SENSITIVE — the table IS the behaviour (a GDPR clock landing 3650 SECONDS out is an
//! erasure ~10 years early). It also catches the both-birth vacuous-pass trap: deleting a
//! `schedules:` from one birth receive makes the generated test DISAPPEAR rather than fail; this
//! pin is what turns that into a red.
//!
//! When a `schedules:` declaration legitimately changes, update the expected set here IN THE SAME
//! commit — that forced diff is the point.

use application::generated::reminders::{ReschedulePolicy, REMINDER_SCHEDULES};

const DAY: u64 = 86_400;

#[test]
fn reminder_schedule_table_is_exactly_the_declared_set() {
    // (actor, on_message, reminder, payload_event, identity_prop, after_key, window, policy)
    let expected: &[(&str, &str, &str, &str, &str, &str, std::time::Duration, ReschedulePolicy)] = &[
        // #167 acceptance deadline: BOTH birth receives schedule it (apply_schedules_in_tx keys
        // on the delivered message type, so each birth needs its own row — deleting one makes
        // the generated test DISAPPEAR, not fail; THIS pin is the red), `keep` (the first
        // deadline wins), ORDER_ACCEPTANCE_TIMEOUT_SECONDS = 300 SECONDS.
        (
            "Order",
            "OrderPlaced",
            "OrderAcceptanceTimedOut",
            "OrderAcceptanceTimedOut",
            "orderId",
            "ORDER_ACCEPTANCE_TIMEOUT_SECONDS",
            std::time::Duration::from_secs(300),
            ReschedulePolicy::Keep,
        ),
        (
            "Order",
            "PlaceReplacementOrder",
            "OrderAcceptanceTimedOut",
            "OrderAcceptanceTimedOut",
            "orderId",
            "ORDER_ACCEPTANCE_TIMEOUT_SECONDS",
            std::time::Duration::from_secs(300),
            ReschedulePolicy::Keep,
        ),
        // …and the timed-out terminal starts the GDPR clock like every other terminal — without
        // this row a timed-out order would never be erased.
        (
            "Order",
            "OrderAcceptanceTimedOut",
            "OrderExpired",
            "OrderExpired",
            "orderId",
            "ORDER_RETENTION_WINDOW_DAYS",
            std::time::Duration::from_secs(3650 * DAY),
            ReschedulePolicy::InPlace,
        ),
        // GDPR retention clock: every Order terminal receive re-declares OrderExpired in place
        // (the LAST terminal fact wins the clock), ORDER_RETENTION_WINDOW_DAYS = 3650 DAYS.
        (
            "Order",
            "MarkOrderDelivered",
            "OrderExpired",
            "OrderExpired",
            "orderId",
            "ORDER_RETENTION_WINDOW_DAYS",
            std::time::Duration::from_secs(3650 * DAY),
            ReschedulePolicy::InPlace,
        ),
        (
            "Order",
            "RejectOrder",
            "OrderExpired",
            "OrderExpired",
            "orderId",
            "ORDER_RETENTION_WINDOW_DAYS",
            std::time::Duration::from_secs(3650 * DAY),
            ReschedulePolicy::InPlace,
        ),
        (
            "Order",
            "CancelOrderByCustomer",
            "OrderExpired",
            "OrderExpired",
            "orderId",
            "ORDER_RETENTION_WINDOW_DAYS",
            std::time::Duration::from_secs(3650 * DAY),
            ReschedulePolicy::InPlace,
        ),
        (
            "Order",
            "CancelOrderByRestaurant",
            "OrderExpired",
            "OrderExpired",
            "orderId",
            "ORDER_RETENTION_WINDOW_DAYS",
            std::time::Duration::from_secs(3650 * DAY),
            ReschedulePolicy::InPlace,
        ),
    ];

    assert_eq!(
        REMINDER_SCHEDULES.len(),
        expected.len(),
        "the schedule table gained or lost a row — pin the new declared set here, in the same commit"
    );
    for (actor, on_message, reminder, payload_event, identity_prop, after_key, window, policy) in
        expected
    {
        let found = REMINDER_SCHEDULES
            .iter()
            .find(|s| s.actor_type == *actor && s.on_message == *on_message && s.reminder == *reminder)
            .unwrap_or_else(|| {
                panic!("missing declared schedule ({actor}, {on_message}, {reminder})")
            });
        assert_eq!(found.payload_event, *payload_event, "({actor}, {on_message}, {reminder})");
        assert_eq!(found.identity_prop, *identity_prop, "({actor}, {on_message}, {reminder})");
        assert_eq!(found.after_key, *after_key, "({actor}, {on_message}, {reminder})");
        assert_eq!(
            found.after_default, *window,
            "({actor}, {on_message}, {reminder}): the WINDOW is wrong — a unit slip here is a \
             GDPR clock firing ~10 years early or a 5-minute timeout waiting 5 days"
        );
        assert_eq!(found.reschedule, *policy, "({actor}, {on_message}, {reminder})");
    }
}
