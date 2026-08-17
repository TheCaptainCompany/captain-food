//! PM COMMAND deliveries through the PREPARE phase (#272 Runtime D1, ADR-20260801-023000 R2):
//! `PlaceOrder` / `ApproveRefund` / `DenyRefund` are the three commands whose handlers make an
//! external gateway call (Stripe), so they cannot run inside the fenced completion transaction
//! like every aggregate command. Instead the WHOLE legacy handler runs in the delivery's prepare
//! phase — no transaction open — against staging stores that buffer every write (events AND the
//! PM run row), and the fenced commit only FLUSHES the captured effects:
//!
//! - validate/price via pool reads, call Stripe (idempotency key = orderId — the adapter derives
//!   it from the `orderId` business ref) — all in prepare;
//! - ONE fenced commit records the staged events + the PM row + the verdict atomically;
//! - a deterministic rejection (CartEmpty, PriceMismatch, a sync Stripe DECLINE →
//!   `PaymentDeclined`) is CAPTURED in the prepared value and committed as the same REJECTED
//!   operation outcome the legacy spawn path produces — the client contract is byte-identical;
//! - a crash (or a flush-time version conflict) between the Stripe call and the commit leaves
//!   the row RECEIVED; redelivery re-runs prepare and the idempotency key returns the SAME
//!   intent — no duplicate charge.
//!
//! The handler logic itself is the UNCHANGED application code (`commands::place_order`,
//! `process_managers::refund::{approve_refund, deny_refund}`) — this module only re-homes where
//! its effects land.

use std::sync::Arc;

use actor_runtime::InboundMessage;
use application::pm_state::{
    PaymentProcessRow, PaymentProcessStateStore, RefundProcessRow, RefundProcessStateStore,
};
use application::ports::{Actor, EventStore};
use application::staging::{
    StagedAppend, StagingEventStore, StagingPaymentProcessState, StagingRefundProcessState,
};
use domain::shared::errors::DomainError;
use sqlx::{Postgres, Transaction};

use crate::generated::command_router::CommandDeps;
use crate::generated::pm_state::{upsert_payment_process_with, upsert_refund_process_with};

/// The three PM commands routed through the prepare phase. Everything else runs in-transaction
/// through the generated router, unchanged.
pub(super) fn is_pm_command(message_type: &str) -> bool {
    matches!(message_type, "PlaceOrder" | "ApproveRefund" | "DenyRefund")
}

/// Every effect one prepared PM command wants to commit.
pub(super) struct PmEffects {
    pub staged: Vec<StagedAppend>,
    pub payment_rows: Vec<PaymentProcessRow>,
    pub refund_rows: Vec<RefundProcessRow>,
}

/// The prepared outcome, computed with NO transaction open. `Err` here is a DETERMINISTIC
/// business rejection to commit as the row's REJECTED/FAILED verdict — never a retry (transient
/// failures abort `prepare` itself and redeliver).
pub(super) struct PreparedPmCommand {
    pub outcome: Result<PmEffects, DomainError>,
}

/// Run the legacy PM command handler against staging stores. Transient infrastructure failures
/// (repository reads, a Stripe 5xx/transport error) return `Err` — the delivery aborts, the row
/// stays RECEIVED, redelivery retries (the gateway idempotency keys make the re-run safe).
pub(super) async fn prepare(
    deps: &CommandDeps,
    message: &InboundMessage,
    actor: &Actor,
) -> Result<PreparedPmCommand, sqlx::Error> {
    use tracing::Instrument as _;

    // Misroute guard FIRST (a wiring bug, not a business outcome) — outside the instrumented
    // block so the `command.validate` span below only ever wraps a real handler run.
    if !is_pm_command(&message.message_type) {
        return Err(sqlx::Error::Protocol(format!(
            "'{}' is not a PM command — prepare_pm_command misrouted (wiring bug)",
            message.message_type
        )));
    }

    let staging = Arc::new(StagingEventStore::new(deps.store.clone()));
    let store: Arc<dyn EventStore> = staging.clone();
    let payment_staging = Arc::new(StagingPaymentProcessState::new(deps.pm_state.clone()));
    let refund_staging = Arc::new(StagingRefundProcessState::new(deps.refund_state.clone()));

    // `command.validate` (specs/observability.yaml `place-order`, required:true): constructed at
    // THIS dispatch seam — the prepare phase IS the place-order command leg's handler boundary
    // since #242 Runtime D — never inside the aggregate/handler (c4-l3 `command-handlers` is
    // `instrumented: false`). `validation_status` is recorded from the actual outcome below.
    let validate_span = telemetry::spans::command_validate();
    let run: Result<(), DomainError> = async {
        match message.message_type.as_str() {
            "PlaceOrder" => match serde_json::from_value::<domain::generated::commands::PlaceOrder>(
                message.payload.clone(),
            ) {
                // An unparsable payload is deterministic — a terminal FAILED (generic Internal),
                // never a retry (the GraphQL edge validates before enqueueing; defensive arm).
                Err(e) => Err(super::attribution::command_payload_undecodable("PlaceOrder", &e.to_string())),
                Ok(cmd) => application::commands::place_order(
                    store.as_ref(),
                    deps.catalogs.as_ref(),
                    deps.payments.as_ref(),
                    payment_staging.as_ref() as &dyn PaymentProcessStateStore,
                    cmd,
                    message.session_id.map(domain::generated::scalars::SessionId),
                    actor,
                    // RSO-1: the evaluation instant, read ONCE here at the delivery seam (the
                    // framework boundary) — the handler takes `now` as a parameter and never reads
                    // a clock itself (the `sms_guard.rs` precedent).
                    chrono::Utc::now(),
                    // RSO-1 Phase 4: the enforcement gate, resolved at the composition root onto
                    // CommandDeps — a parameter for the same reason as the clock.
                    deps.enforce_service_hours_guard,
                )
                .await
                .map(|_| ()),
            },
            "ApproveRefund" => match serde_json::from_value::<domain::generated::commands::ApproveRefund>(
                message.payload.clone(),
            ) {
                Err(e) => Err(super::attribution::command_payload_undecodable("ApproveRefund", &e.to_string())),
                Ok(cmd) => application::process_managers::refund::approve_refund(
                    store.as_ref(),
                    refund_staging.as_ref() as &dyn RefundProcessStateStore,
                    deps.payments.as_ref(),
                    cmd,
                    actor,
                )
                .await,
            },
            "DenyRefund" => match serde_json::from_value::<domain::generated::commands::DenyRefund>(
                message.payload.clone(),
            ) {
                Err(e) => Err(super::attribution::command_payload_undecodable("DenyRefund", &e.to_string())),
                Ok(cmd) => application::process_managers::refund::deny_refund(
                    store.as_ref(),
                    refund_staging.as_ref() as &dyn RefundProcessStateStore,
                    cmd,
                    actor,
                )
                .await,
            },
            // Unreachable: `is_pm_command` was asserted above.
            other => Err(DomainError::Repository(format!("'{other}' is not a PM command"))),
        }
    }
    .instrument(validate_span.clone())
    .await;

    // The contract's `validation_status`: accepted | rejected, from the ACTUAL outcome. A
    // Repository error is transient infrastructure (the delivery aborts and retries below) — it
    // records NO verdict, because classifying a DB blip as `rejected` would count it as a
    // business refusal.
    match &run {
        Ok(()) => telemetry::spans::record_validation_status(&validate_span, "accepted"),
        Err(DomainError::Repository(_)) => {}
        Err(_) => telemetry::spans::record_validation_status(&validate_span, "rejected"),
    }
    // RSO-1: `business.service_window_verdict` — the accept branch's only signal while the gate
    // is OFF (shadow mode refuses nothing, so without this attribute the decision is invisible
    // in traces). Read from the evidence the run itself produced — the frozen snapshot on the
    // staged PaymentIntentCreated, or the OutsideServiceHours refusal (which is, by
    // rules.yaml#/CheckoutRefusesOnlyOutsideServiceHours, exactly the OUTSIDE_HOURS verdict) —
    // never recomputed here, so span and record can never disagree. Rejections upstream of the
    // evaluation (RestaurantNotFound, RestaurantPaused, …) never computed a verdict and record
    // none.
    if message.message_type == "PlaceOrder" {
        let verdict = match &run {
            Ok(()) => staging
                .peek_staged()
                .iter()
                .flat_map(|a| a.events.iter())
                .find_map(|e| match e {
                    domain::generated::events::DomainEvent::PaymentIntentCreated(p) => {
                        p.checkout.verdict.map(|v| format!("{v:?}"))
                    }
                    _ => None,
                }),
            Err(DomainError::Rejected { code, .. }) if code == "OutsideServiceHours" => {
                Some("OUTSIDE_HOURS".to_string())
            }
            Err(_) => None,
        };
        if let Some(v) = verdict {
            telemetry::spans::record_service_window_verdict(&validate_span, &v);
        }
    }

    let outcome = match run {
        Ok(()) => Ok(PmEffects {
            staged: staging.take_staged(),
            payment_rows: payment_staging.take_staged(),
            refund_rows: refund_staging.take_staged(),
        }),
        // Transient infrastructure failure INSIDE the handler (a pool read, a Stripe transport
        // error / 5xx): abort the delivery for retry — only deterministic outcomes may land a
        // terminal verdict (same discrimination as the in-tx command route).
        Err(DomainError::Repository(detail)) => return Err(sqlx::Error::Protocol(detail)),
        // Deterministic rejection (catalogued errors.yaml code, incl. the sync Stripe decline's
        // `PaymentDeclined: …` invariant form): committed as the row's verdict.
        Err(e) => Err(e),
    };
    Ok(PreparedPmCommand { outcome })
}

/// Flush the buffered PM run rows INTO the completion transaction — the same generated upsert
/// SQL the pool-backed stores run (`upsert_*_with` is executor-generic precisely so this cannot
/// drift from them).
pub(super) async fn flush_pm_rows_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    payment_rows: &[PaymentProcessRow],
    refund_rows: &[RefundProcessRow],
) -> Result<(), DomainError> {
    for row in payment_rows {
        upsert_payment_process_with(&mut **tx, row).await?;
    }
    for row in refund_rows {
        upsert_refund_process_with(&mut **tx, row).await?;
    }
    Ok(())
}

// ================================================================================================
// B2 — chained PM facts (ADR-20260731-203000 D-B): the Payment lane records the inbound Stripe
// fact (unchanged), and the SAME completion transaction enqueues a PM-addressed copy on the
// order's lane, cause-chained to the recording row. In-tx rather than the post-commit hook the
// decision sketched: atomic with the recorded fact, so no crash window can record a payment and
// lose its saga hop. The saga runner's Stripe-fact triggers retire behind the same gate.
// ================================================================================================

/// Which PM lane (if any) one recorded Payment fact chains to, and under which lane key.
/// `None` = not a chained fact type. The lane key is the ORDER (B2: the PM's identity); when the
/// fact does not carry it, the run row correlates intent → order; an orphan fact (no run) falls
/// back to a deterministic surrogate lane so the PM leg still runs — and flags
/// `PaymentEventOrphaned` on the chained row, supervisable, never silently dropped.
pub(super) async fn chain_target_of(
    pm_state: &dyn PaymentProcessStateStore,
    event: &domain::generated::events::DomainEvent,
) -> Result<Option<(&'static str, uuid::Uuid)>, sqlx::Error> {
    use domain::generated::events::DomainEvent as E;
    let by_intent = |intent: &domain::generated::scalars::PaymentIntentId| {
        let intent = intent.clone();
        async move {
            pm_state
                .by_payment_intent(&intent)
                .await
                // A transient lookup failure aborts the delivery (retry) — guessing a lane would
                // scatter one order's facts across lanes and break per-order serialization.
                .map_err(|e| sqlx::Error::Protocol(e.to_string()))
                .map(|row| row.map(|r| r.order_id.0))
        }
    };
    match event {
        // Authorize-then-capture (ADR-20260808-195315 §1.2): the AUTHORIZATION is what
        // materializes the order, so IT chains to PlaceOrderProcess. The later PaymentCaptured is
        // pure settlement — recorded on the Payment stream and folded by OrderTracking, with no
        // saga hop (PlaceOrderProcess's run is already resolved; PaymentSettlementProcess reacts
        // to ORDER events, which the saga runner drains).
        E::PaymentAuthorized(e) => {
            let order = match e.order_id {
                Some(order_id) => Some(order_id.0),
                None => by_intent(&e.payment_intent_id).await?,
            };
            Ok(Some((
                "PlaceOrderProcess",
                order.unwrap_or_else(|| {
                    actor_client::surrogate_actor_id("PlaceOrderProcess", &e.payment_intent_id.0)
                }),
            )))
        }
        E::PaymentFailed(e) => Ok(Some((
            "PlaceOrderProcess",
            by_intent(&e.payment_intent_id).await?.unwrap_or_else(|| {
                actor_client::surrogate_actor_id("PlaceOrderProcess", &e.payment_intent_id.0)
            }),
        ))),
        E::PaymentRefunded(e) => Ok(Some(("RefundProcess", e.order_id.0))),
        _ => Ok(None),
    }
}

/// Enqueue the PM-addressed copy INSIDE the completion transaction. Identity is deterministic —
/// `UUIDv5(lane key, "{factType}:{causing message_id}")` — so a redelivered recording collides on
/// the pk instead of double-chaining, while two DISTINCT same-type facts for one order (a second
/// payment attempt's failure, a second partial refund's settlement) each keep their own hop
/// (the decided `UUIDv5(orderId, factType)` key alone would silently drop the second).
/// Returns the chained actor type for the post-commit nudge, or `None` when nothing chains.
pub(super) async fn chain_pm_copy_in_tx(
    deps: &CommandDeps,
    tx: &mut Transaction<'_, Postgres>,
    message: &InboundMessage,
    event: &domain::generated::events::DomainEvent,
) -> Result<Option<&'static str>, sqlx::Error> {
    let Some((actor_type, actor_id)) = chain_target_of(deps.pm_state.as_ref(), event).await?
    else {
        return Ok(None);
    };
    // The lane comes from the DECLARED routing contract (#596) — `actor_client::declared_lane`
    // over `ACTOR_MAILBOXES`, the same source `flush_lane_enqueues_in_tx` and every typed client
    // use. It used to be `count(*) FROM mailbox_partitions`, and that was wrong twice over:
    //
    //   - at ZERO rows it returned `sqlx::Error::Protocol`, INSIDE this completion transaction.
    //     The registry is written only by `MailboxWorker::seed` at worker startup — migrations
    //     never touch it — so "the target actor's worker has not started" aborted the recording of
    //     an authorization Stripe had already accepted. And it is worse than a delay: a failed
    //     completion transaction takes the poison path (`worker.rs`), which holds the Payment lane
    //     head-of-line below the attempt cap and, AT the cap, flips the authorization row itself
    //     to terminal FAILED. A paid customer whose order can then never be born, even once the
    //     worker comes up.
    //   - at a PARTIAL row count it silently addressed a narrower keyspace than every other
    //     producer, which is a one-writer violation: see `declared_lane`'s docs.
    //
    // Behaviour now: the hop is addressed correctly and simply WAITS on its lane. Waiting is the
    // correct answer to "no worker yet"; failing never was.
    let Some(partition) = actor_client::declared_lane(actor_type, &actor_id) else {
        // Unreachable through the emitter (`chain_target_of` only names actors with mailboxes);
        // a wiring bug, never a business outcome.
        return Err(sqlx::Error::Protocol(format!(
            "PM fact chaining targets '{actor_type}', which declares no mailbox — wiring bug"
        )));
    };
    let chained_id = uuid::Uuid::new_v5(
        &actor_id,
        format!("{}:{}", message.message_type, message.message_id).as_bytes(),
    );
    // The chained hop rides the COMPLETION transaction, and so does its pg_notify
    // (PROP-20260802-223522 D1): the PM lane's worker — in this process or any other — wakes
    // when the recording commits, never on a rolled-back delivery. The in-process nudge
    // (post-commit, `with_nudges`) stays as the zero-latency local path.
    sqlx::query(
        "WITH ins AS ( \
           INSERT INTO inbound_messages \
             (message_id, kind, actor_type, actor_id, partition, message_type, payload, \
              payload_hash, channel, user_id, user_type, correlation_id, cause_id) \
           VALUES ($1, 'EVENT', $2, $3, $4, $5, $6, $7, 'WORKER', $8, $9, $10, $11) \
           ON CONFLICT (message_id) DO NOTHING \
           RETURNING actor_type \
         ) \
         SELECT pg_notify('inbound_messages', actor_type) FROM ins",
    )
    .bind(chained_id)
    .bind(actor_type)
    .bind(actor_id)
    .bind(partition)
    .bind(&message.message_type)
    .bind(&message.payload)
    .bind(&message.payload_hash)
    .bind(message.user_id)
    .bind(&message.user_type)
    .bind(message.correlation_id)
    // The causality chain: the chained hop's cause is the Payment-lane row that recorded it.
    .bind(message.message_id)
    .execute(&mut **tx)
    .await?;
    Ok(Some(actor_type))
}

// ================================================================================================
// The FLIP-TIME BACKFILL (#272 review MAJOR-2): B2 chaining only happens at RECORD time, so a
// Stripe fact recorded before the gate flipped ON — one the saga runner had accepted but not yet
// reacted to — would have NO deliverer after the flip: a PaymentCaptured with no OrderPlaced is
// a paid order nobody is told about, the product's worst failure mode. At every startup with the
// gate ON, this pass enqueues a PM-addressed copy of every Stripe fact past the runner's group
// checkpoints. Idempotent end to end: the chain identity is UUIDv5(lane, "{factType}:{event id}")
// (deterministic per fact — a restart re-scan collides on the pk), and the saga legs absorb a
// hop that record-time chaining already delivered (run-row expect → IGNORED). Gate ROLLBACK
// stays sound for the same reason: the runner re-processing a mailbox-delivered fact skips on
// the same idempotency.
// ================================================================================================

/// Enqueue PM-addressed copies of the un-reacted Stripe facts (called at startup, gate ON, after
/// idempotently seeding the PM lanes). Returns how many copies were enqueued (dedup collisions
/// excluded).
pub async fn backfill_stripe_facts_to_pm_lanes(
    pool: &sqlx::PgPool,
    pm_state: &dyn PaymentProcessStateStore,
) -> Result<u64, DomainError> {
    let checkpoint = |projector: &'static str| async move {
        sqlx::query_scalar::<_, i64>(
            "SELECT position FROM projection_checkpoint WHERE projector = $1",
        )
        .bind(projector)
        .fetch_optional(pool)
        .await
        .map(|c| c.unwrap_or(0))
        .map_err(|e| DomainError::Repository(e.to_string()))
    };
    let cp_place = checkpoint("pm:PlaceOrderProcess").await?;
    let cp_refund = checkpoint("pm:RefundProcess").await?;

    let rows = sqlx::query(
        "SELECT id, position, event_type, payload, correlation_id, user_id, user_type FROM domain_events \
         WHERE (event_type IN ('PaymentAuthorized', 'PaymentFailed') AND position > $1) \
            OR (event_type = 'PaymentRefunded' AND position > $2) \
         ORDER BY position",
    )
    .bind(cp_place)
    .bind(cp_refund)
    .fetch_all(pool)
    .await
    .map_err(|e| DomainError::Repository(e.to_string()))?;

    let mut enqueued = 0u64;
    // The highest position this pass SAW per group — advanced onto the frozen `pm:*` checkpoints
    // after a clean pass (#272 review MAJOR: the runner no longer moves them behind the gate, so
    // without this every restart re-scans the whole post-flip history and enqueues O(history)
    // idempotent-but-position-fresh dead hops ahead of live reactions on the money lanes).
    // Rollback-safe: everything at or below the advanced value was either runner-processed
    // pre-flip or enqueued onto the PM lanes by this pass (and the lanes deliver regardless of
    // the gate); facts recorded after it while the gate is ON are chained at record time, and a
    // duplicate replay after a rollback is absorbed by the run rows' expects (IGNORED).
    let mut seen_place: i64 = 0;
    let mut seen_refund: i64 = 0;
    for row in rows {
        use sqlx::Row as _;
        let event_id: uuid::Uuid = row.try_get("id").map_err(|e| DomainError::Repository(e.to_string()))?;
        let position: i64 = row.try_get("position").map_err(|e| DomainError::Repository(e.to_string()))?;
        let event_type: String = row.try_get("event_type").map_err(|e| DomainError::Repository(e.to_string()))?;
        // Every deterministically-handled row advances its group's watermark (skips mirror the
        // runner's own advance-past-poison/orphan semantics); a transient error below returns
        // Err before any advance is written.
        match event_type.as_str() {
            "PaymentRefunded" => seen_refund = seen_refund.max(position),
            _ => seen_place = seen_place.max(position),
        }
        let payload: serde_json::Value = row.try_get("payload").map_err(|e| DomainError::Repository(e.to_string()))?;
        let tagged = serde_json::json!({ "eventType": event_type, "payload": payload });
        let event: domain::generated::events::DomainEvent = match serde_json::from_value(tagged.clone()) {
            Ok(e) => e,
            Err(e) => {
                // A log row this build cannot parse (legacy payload shape): loud, never wedging —
                // the runner would have surfaced the same poison on /saga and advanced.
                tracing::error!(%event_id, %event_type, error = %e, "pm backfill: unparsable fact skipped");
                continue;
            }
        };
        let Some((actor_type, actor_id)) =
            chain_target_of(pm_state, &event).await.map_err(|e| DomainError::Repository(e.to_string()))?
        else {
            continue;
        };
        // THE SAME declared source as the record-time chain above (#596). This site carried an
        // identical `count(*)` read and an identical zero-width error, and the card that
        // dispatched the fix named only two sites — `dba` and `beck` found this one independently.
        // It matters more here, not less: the backfill exists precisely to rescue facts nobody
        // reacted to, and it runs at STARTUP, where "the other actor's worker has not seeded yet"
        // is the likeliest state in the process's life. A rescue pass that refuses to run when the
        // system is cold is no rescue.
        let Some(partition) = actor_client::declared_lane(actor_type, &actor_id) else {
            return Err(DomainError::Repository(format!(
                "pm backfill: '{actor_type}' declares no mailbox — wiring bug"
            )));
        };
        let chained_id =
            uuid::Uuid::new_v5(&actor_id, format!("{event_type}:{event_id}").as_bytes());
        let correlation_id: uuid::Uuid = row.try_get("correlation_id").map_err(|e| DomainError::Repository(e.to_string()))?;
        let user_id: uuid::Uuid = row.try_get("user_id").map_err(|e| DomainError::Repository(e.to_string()))?;
        let user_type: String = row.try_get("user_type").map_err(|e| DomainError::Repository(e.to_string()))?;
        let inserted = sqlx::query(
            "INSERT INTO inbound_messages \
               (message_id, kind, actor_type, actor_id, partition, message_type, payload, \
                payload_hash, channel, user_id, user_type, correlation_id, cause_id) \
             VALUES ($1, 'EVENT', $2, $3, $4, $5, $6, $7, 'WORKER', $8, $9, $10, $11) \
             ON CONFLICT (message_id) DO NOTHING",
        )
        .bind(chained_id)
        .bind(actor_type)
        .bind(actor_id)
        .bind(partition)
        .bind(&event_type)
        .bind(&tagged)
        .bind(application::journal::payload_hash(&tagged))
        .bind(user_id)
        .bind(&user_type)
        .bind(correlation_id)
        // The causality link: the backfilled hop's cause is the recorded fact itself.
        .bind(event_id)
        .execute(pool)
        .await
        .map_err(|e| DomainError::Repository(e.to_string()))?
        .rows_affected();
        enqueued += inserted;
    }
    // A CLEAN pass advances the frozen checkpoints to what it saw, ending the O(history)
    // restart re-scan. `GREATEST` keeps a concurrent peer's larger advance.
    for (projector, seen) in
        [("pm:PlaceOrderProcess", seen_place), ("pm:RefundProcess", seen_refund)]
    {
        if seen > 0 {
            sqlx::query(
                "INSERT INTO projection_checkpoint (projector, position, updated_at) VALUES ($1, $2, now()) \
                 ON CONFLICT (projector) DO UPDATE \
                 SET position = GREATEST(projection_checkpoint.position, EXCLUDED.position), updated_at = now()",
            )
            .bind(projector)
            .bind(seen)
            .execute(pool)
            .await
            .map_err(|e| DomainError::Repository(e.to_string()))?;
        }
    }
    Ok(enqueued)
}
