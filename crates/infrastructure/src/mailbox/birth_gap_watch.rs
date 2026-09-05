//! THE BIRTH-GAP DEAD-MAN'S SWITCH (#608, the `place-order` and `payment-settlement` contracts in
//! specs/observability.yaml).
//!
//! *Is a customer's card authorized right now with no order behind it, and for how long?* That is
//! the product's worst failure mode with the money already moved, and until this sweep existed
//! nothing in the system could answer it: `payment_authorized_unsettled_age_seconds` is computed
//! over the OrderTracking read model, and an order that was NEVER BORN has no OrderTracking row.
//! Both that gauge and the new one had ZERO emit sites in `crates/**`.
//!
//! **The source is the saga's own run state, not an anti-join of two aggregates**
//! (ADR-20260816-213000, vernon). The deciding fact is that the `payment_process_manager` row is
//! created at CHECKOUT — the `PlaceOrder` handler opens the run at `AWAITING_PAYMENT_RESULT` before
//! any Stripe outcome exists, and the PM legs only `expect` an existing row. So an authorization
//! the saga never processed STILL HAS A RUN ROW: one owner, one write path, no cross-aggregate
//! simultaneity, and no other consumer's projection becomes an input to the money path.
//!
//! **Three reasons, one gauge, one bounded label set** ([`telemetry::meters::birth_gap::REASONS`]):
//!
//! - `retry_pending` — the `PaymentAuthorized` hop is on the `PlaceOrderProcess` lane and still
//!   DELIVERABLE while the run is `AWAITING_PAYMENT_RESULT`. Time may still fix it, so its
//!   threshold is the whole exponential retry schedule (declared in the contract).
//! - `delivery_exhausted` — the hop reached a TERMINAL status and the run is still
//!   `AWAITING_PAYMENT_RESULT`. Nothing will redeliver it; waiting is pointless, threshold 0.
//! - `no_run` — the RESIDUE the run state cannot cover: `PlaceOrder` performs two sequential
//!   unfenced durable writes (the Stripe intent-create, then the run-row upsert), and a crash
//!   between them leaves funds held with NO run row. The webhook leg then finds nothing by intent
//!   and skips, while the Payment aggregate still records `PaymentAuthorized`. Visible only in
//!   `domain_events`.
//!
//! **Zeros are the contract.** Every member reports on EVERY tick, and
//! `payment_birth_gap_sweep_heartbeat_total` increments only after a COMPLETE sweep. An early
//! return on an empty population is the specific bug designed against here: a monitor that can
//! only fire when a signal ARRIVES goes quiet exactly when it should scream (ADR-20260810-231300).
//!
//! **The fold is over OPENNESS, never AGE** (young). Nothing materialises `ageSeconds` or
//! `isOrphaned`: the queries read the open set and its STORED timestamps, and `now` is applied at
//! query time. A stored age would be rewritten by rebuild-time `now` — truncate, replay, diff, and
//! the open set with its timestamps must come back identical.
//!
//! `now` is a PARAMETER, never SQL's own `now()` (round 3, beck): production still passes
//! `Utc::now()` once per tick, but binding it lets a caller pin an exact instant instead of racing
//! the database's clock against whatever wall-clock time its own setup burned — the difference a
//! rounding `extract(epoch ...)::bigint` turns into an off-by-one.
//!
//! Like `promotion_watch` and `order_lane_watch`, this is a MONITOR outside the workers it watches:
//! a watcher living inside the PlaceOrderProcess worker would be silenced by the very death it
//! exists to report. Time-triggered, so explicitly outside the no-polling rule's scope.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};

/// Hops the mailbox may still deliver. Every other `InboundMessageStatus` is terminal, and a
/// terminal hop whose run never resolved is `delivery_exhausted` — the distinction is "will
/// anything try again", not "did it fail", which is why SCHEDULED counts as pending and IGNORED
/// does not.
const DELIVERABLE: [&str; 2] = ["RECEIVED", "SCHEDULED"];

/// The two hop-derived reasons, in one grouped query over the saga's own run state joined to the
/// durable record of its own trigger. Grouped rather than one round trip per reason: the monitor's
/// cost must not grow with the thing it monitors.
///
/// `payload->'payload'->>'paymentIntentId'`, not `payload->>...`: a mailbox EVENT row carries the
/// ADJACENTLY-TAGGED `DomainEvent` (`{eventType, payload}`), while `domain_events.payload` below is
/// the bare business payload. The two shapes differ by one hop and a `->>` at the wrong depth reads
/// NULL — i.e. finds nothing, forever, silently.
const HOP_SQL: &str = "SELECT CASE WHEN m.status = ANY($1) THEN 'retry_pending' \
                                   ELSE 'delivery_exhausted' END AS reason, \
                              max(extract(epoch FROM $2::timestamptz - m.received_at))::bigint AS age_seconds \
                       FROM inbound_messages m \
                       JOIN payment_process_manager p \
                         ON p.payment_intent_id = m.payload->'payload'->>'paymentIntentId' \
                       WHERE m.actor_type = 'PlaceOrderProcess' \
                         AND m.message_type = 'PaymentAuthorized' \
                         AND p.process_status = 'AWAITING_PAYMENT_RESULT' \
                       GROUP BY 1";

/// The residue: an authorization recorded on a Payment stream whose intent has no run row at all.
/// A LEFT JOIN anti-join against ONE table, not two aggregates — `payment_process_manager` is the
/// saga's state, and its absence is the whole finding.
const NO_RUN_SQL: &str = "SELECT coalesce(max(extract(epoch FROM $1::timestamptz - e.occurred_at)), 0)::bigint \
                          FROM domain_events e \
                          LEFT JOIN payment_process_manager p \
                            ON p.payment_intent_id = e.payload->>'paymentIntentId' \
                          WHERE e.event_type = 'PaymentAuthorized' AND p.cart_id IS NULL";

/// The `payment-settlement` gauge this sweep also carries: BORN orders still AUTHORIZED, aged from
/// their immutable `placed_at`. Complementary to the three above and NOT a substitute for them.
const UNSETTLED_SQL: &str = "SELECT coalesce(max(extract(epoch FROM $1::timestamptz - placed_at)), 0)::bigint \
                             FROM OrderTracking WHERE payment_status = 'AUTHORIZED'";

/// One sweep: emit the birth-gap gauge for EVERY declared reason (0 included), the
/// born-never-captured gauge, and — last, only on a complete pass — the heartbeat.
///
/// `now` is a PARAMETER (round 3, beck): every age computed here reads `now - <stored timestamp>`
/// against this ONE bound instant rather than SQL's own `now()`, so all three age queries in one
/// sweep — and a caller pinning `now` to the exact instant it used to synthesize a fixture's age —
/// agree byte-for-byte. Calling SQL's `now()` at query time let ANY wall-clock time between a
/// fixture ageing a row and this tick reading it round `extract(epoch ...)::bigint` up by one.
pub async fn birth_gap_watch_tick(pool: &PgPool, now: DateTime<Utc>) -> Result<(), sqlx::Error> {
    // Seeded with every declared member at 0 BEFORE the query runs. This is the zero contract in
    // code: whatever the database returns, the emit loop below has three entries to report.
    let mut age_by_reason: BTreeMap<&'static str, i64> =
        telemetry::meters::birth_gap::REASONS.iter().map(|r| (*r, 0i64)).collect();

    for row in
        sqlx::query(HOP_SQL).bind(&DELIVERABLE[..]).bind(now).fetch_all(pool).await?
    {
        let reason: String = row.try_get("reason")?;
        let age: i64 = row.try_get("age_seconds")?;
        // Look the reason up in the DECLARED set rather than inserting it: a value the contract
        // does not declare is a bug in this file, and silently widening the label set is how a
        // bounded population stops being bounded.
        if let Some(slot) = age_by_reason.get_mut(reason.as_str()) {
            *slot = (*slot).max(age);
        }
    }

    let no_run: i64 = sqlx::query_scalar(NO_RUN_SQL).bind(now).fetch_one(pool).await?;
    if let Some(slot) = age_by_reason.get_mut("no_run") {
        *slot = no_run.max(0);
    }

    for (reason, age) in &age_by_reason {
        telemetry::meters::birth_gap::no_order_birth_age(reason, *age);
    }

    let unsettled: i64 = sqlx::query_scalar(UNSETTLED_SQL).bind(now).fetch_one(pool).await?;
    telemetry::meters::birth_gap::authorized_unsettled_age(unsettled.max(0));

    // LAST. The heartbeat certifies a COMPLETE sweep; every `?` above returns before it, so a
    // half-run pass leaves the counter flat — which is the alertable condition.
    telemetry::meters::birth_gap::sweep_completed();
    Ok(())
}

/// Spawn the sweep on its own clock. A failed tick logs and keeps ticking — a watcher that dies
/// with its patient defeats its purpose; its OWN death is observable as the heartbeat stopping,
/// which is the whole contract.
pub fn spawn_birth_gap_watch(pool: PgPool, every: std::time::Duration) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(e) = birth_gap_watch_tick(&pool, Utc::now()).await {
                tracing::warn!(error = %e, "birth gap watch: tick failed -- retrying next tick");
            }
        }
    });
}
