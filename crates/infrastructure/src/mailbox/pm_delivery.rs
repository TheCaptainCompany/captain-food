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
