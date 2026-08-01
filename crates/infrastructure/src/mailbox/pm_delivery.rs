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
    let staging = Arc::new(StagingEventStore::new(deps.store.clone()));
    let store: Arc<dyn EventStore> = staging.clone();
    let payment_staging = Arc::new(StagingPaymentProcessState::new(deps.pm_state.clone()));
    let refund_staging = Arc::new(StagingRefundProcessState::new(deps.refund_state.clone()));

    let run: Result<(), DomainError> = match message.message_type.as_str() {
        "PlaceOrder" => match serde_json::from_value::<domain::generated::commands::PlaceOrder>(
            message.payload.clone(),
        ) {
            // An unparsable payload is deterministic — a terminal FAILED (generic Internal),
            // never a retry (the GraphQL edge validates before enqueueing; defensive arm).
            Err(e) => Err(DomainError::Invariant(format!("PlaceOrder payload: {e}"))),
            Ok(cmd) => application::commands::place_order(
                store.as_ref(),
                deps.catalogs.as_ref(),
                deps.payments.as_ref(),
                payment_staging.as_ref() as &dyn PaymentProcessStateStore,
                cmd,
                message.session_id.map(domain::generated::scalars::SessionId),
                actor,
            )
            .await
            .map(|_| ()),
        },
        "ApproveRefund" => match serde_json::from_value::<domain::generated::commands::ApproveRefund>(
            message.payload.clone(),
        ) {
            Err(e) => Err(DomainError::Invariant(format!("ApproveRefund payload: {e}"))),
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
            Err(e) => Err(DomainError::Invariant(format!("DenyRefund payload: {e}"))),
            Ok(cmd) => application::process_managers::refund::deny_refund(
                store.as_ref(),
                refund_staging.as_ref() as &dyn RefundProcessStateStore,
                cmd,
                actor,
            )
            .await,
        },
        other => {
            return Err(sqlx::Error::Protocol(format!(
                "'{other}' is not a PM command — prepare_pm_command misrouted (wiring bug)"
            )))
        }
    };

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
    deps: &CommandDeps,
    event: &domain::generated::events::DomainEvent,
) -> Result<Option<(&'static str, uuid::Uuid)>, sqlx::Error> {
    use domain::generated::events::DomainEvent as E;
    let by_intent = |intent: &domain::generated::scalars::PaymentIntentId| {
        let intent = intent.clone();
        async move {
            deps.pm_state
                .by_payment_intent(&intent)
                .await
                // A transient lookup failure aborts the delivery (retry) — guessing a lane would
                // scatter one order's facts across lanes and break per-order serialization.
                .map_err(|e| sqlx::Error::Protocol(e.to_string()))
                .map(|row| row.map(|r| r.order_id.0))
        }
    };
    match event {
        E::PaymentCaptured(e) => {
            let order = match e.order_id {
                Some(order_id) => Some(order_id.0),
                None => by_intent(&e.payment_intent_id).await?,
            };
            Ok(Some((
                "PlaceOrderProcess",
                order.unwrap_or_else(|| {
                    super::surrogate_actor_id("PlaceOrderProcess", &e.payment_intent_id.0)
                }),
            )))
        }
        E::PaymentFailed(e) => Ok(Some((
            "PlaceOrderProcess",
            by_intent(&e.payment_intent_id).await?.unwrap_or_else(|| {
                super::surrogate_actor_id("PlaceOrderProcess", &e.payment_intent_id.0)
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
    let Some((actor_type, actor_id)) = chain_target_of(deps, event).await? else {
        return Ok(None);
    };
    // The lane keyspace WIDTH is the actor's seeded registry row count — the same source the
    // workers seeded from, so the chain can never address a partition no worker drains.
    let width: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mailbox_partitions WHERE actor_type = $1")
            .bind(actor_type)
            .fetch_one(&mut **tx)
            .await?;
    if width == 0 {
        return Err(sqlx::Error::Protocol(format!(
            "PM fact chaining is on but '{actor_type}' has no seeded lanes — start its worker first"
        )));
    }
    let chained_id = uuid::Uuid::new_v5(
        &actor_id,
        format!("{}:{}", message.message_type, message.message_id).as_bytes(),
    );
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, \
            payload_hash, channel, user_id, user_type, correlation_id, cause_id) \
         VALUES ($1, 'EVENT', $2, $3, $4, $5, $6, $7, 'WORKER', $8, $9, $10, $11) \
         ON CONFLICT (message_id) DO NOTHING",
    )
    .bind(chained_id)
    .bind(actor_type)
    .bind(actor_id)
    .bind(actor_runtime::stable_partition(&actor_id, width as u16))
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
