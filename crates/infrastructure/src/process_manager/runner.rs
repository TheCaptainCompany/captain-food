//! The saga runner — the RUNTIME ENVELOPE of the state-table process managers
//! (`specs/processmanager.yaml`, ADR-20260719-193500), mirroring `projection::worker::ProjectionWorker`:
//!
//! - a REGISTRY of process managers, each with its OWN `projection_checkpoint` row (`pm:<Name>`) and
//!   the trigger `event_type`s its EVENT legs declare (COMMAND legs run in the mutation handlers);
//! - each tick, every PM drains `domain_events` past its checkpoint for those event types and
//!   dispatches each trigger to its ORCHESTRATOR (`application::process_managers`), handing it the
//!   event-store port, its state-table store, the read models and outbound ports its steps declare,
//!   plus the [`TriggerEnvelope`] (the orchestrators build the saga actor from it — correlation
//!   propagated, cause = trigger event id, ADR-0041);
//! - guard outcomes (the DSL contract): `Ok(Completed)` advances; `Ok(Skipped)` is a benign
//!   alternative, LOGGED and advanced; `Err` is a THROWN event-leg guard — the typed error is
//!   surfaced on the runner status (`/saga` `last_error`) and logged, then the checkpoint advances
//!   (poison never wedges the group, anomalies are never silently skipped);
//! - a VERSION CONFLICT on an executed append is a lost race, not poison: the drain aborts WITHOUT
//!   advancing the checkpoint and the whole leg re-runs next tick over fresh state (the state row's
//!   `by`/`expect` checks and the aggregates' record-idempotency absorb the replay).

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use application::ports::{is_version_conflict, DeliveryPartner, NoopDeliveryPartner};
use application::process_managers::{
    cart_binding, delivery_dispatch, place_order, refund, Outcome, TriggerEnvelope,
};
use chrono::Utc;
use domain::generated::events::DomainEvent;
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};

use crate::persistence::{
    db_err, PgCartBindingState, PgCartRepository, PgDeliveryDispatchState, PgEventStore,
    PgOrderRepository, PgPaymentProcessState, PgRefundProcessState,
};
use crate::process_manager::ProcessManagerStatus;

const POLL_INTERVAL: Duration = Duration::from_millis(1500);

/// The registered process managers (`specs/processmanager.yaml`).
#[derive(Clone, Copy, Debug)]
enum ProcessManager {
    PlaceOrder,
    Refund,
    CartBinding,
    DeliveryDispatch,
}

/// One drained unit: a process manager with its own checkpoint row and the trigger event types its
/// EVENT legs declare (the serde tags of the trigger events).
struct PmGroup {
    /// The `projection_checkpoint.projector` key (`pm:` prefix keeps saga rows apart from projector rows).
    checkpoint: &'static str,
    pm: ProcessManager,
    /// The `event_type` slice this PM drains.
    triggers: &'static [&'static str],
}

const REGISTRY: &[PmGroup] = &[
    PmGroup {
        checkpoint: "pm:PlaceOrderProcess",
        pm: ProcessManager::PlaceOrder,
        triggers: &["PaymentCaptured", "PaymentFailed"],
    },
    PmGroup {
        checkpoint: "pm:RefundProcess",
        pm: ProcessManager::Refund,
        triggers: &[
            "OrderRejectedByRestaurant",
            "OrderCancelledByCustomer",
            "OrderCancelledByRestaurant",
            "RefundRequested",
            "PaymentRefunded",
        ],
    },
    PmGroup {
        checkpoint: "pm:CartBindingProcess",
        pm: ProcessManager::CartBinding,
        triggers: &["CustomerIdentified"],
    },
    PmGroup {
        checkpoint: "pm:DeliveryDispatchProcess",
        pm: ProcessManager::DeliveryDispatch,
        triggers: &[
            "OrderMarkedReady",
            "DeliveryAcceptedByPartner",
            "DeliveryRejectedByPartner",
            "DeliveryStatusUpdated",
            "DeliveryCompleted",
        ],
    },
];

/// One trigger row as read from `domain_events` — the typed event plus its envelope bits.
struct Trigger {
    event_type: String,
    event: DomainEvent,
    envelope: TriggerEnvelope,
}

pub struct ProcessManagerRunner {
    pool: PgPool,
    /// Appends/loads go through the ordinary event-store port (same adapter the command handlers use).
    store: PgEventStore,
    // The four state-table stores (`application::pm_state` ports over `*_process_manager`).
    payment_state: PgPaymentProcessState,
    refund_state: PgRefundProcessState,
    cart_binding_state: PgCartBindingState,
    dispatch_state: PgDeliveryDispatchState,
    // The read models the DSL `read` steps declare.
    orders: PgOrderRepository,
    carts: PgCartRepository,
    /// DeliveryDispatchProcess's outbound port (no-op stand-in until the avelo37 ACL lands).
    partner: Arc<dyn DeliveryPartner>,
    status: Arc<Mutex<ProcessManagerStatus>>,
}

impl ProcessManagerRunner {
    pub fn new(pool: PgPool) -> Self {
        Self {
            store: PgEventStore::new(pool.clone()),
            payment_state: PgPaymentProcessState::new(pool.clone()),
            refund_state: PgRefundProcessState::new(pool.clone()),
            cart_binding_state: PgCartBindingState::new(pool.clone()),
            dispatch_state: PgDeliveryDispatchState::new(pool.clone()),
            orders: PgOrderRepository::new(pool.clone()),
            carts: PgCartRepository::new(pool.clone()),
            partner: Arc::new(NoopDeliveryPartner),
            pool,
            status: Arc::new(Mutex::new(ProcessManagerStatus::default())),
        }
    }

    /// Replace the delivery-partner port (the composition root injects the real ACL when it lands).
    pub fn with_partner(mut self, partner: Arc<dyn DeliveryPartner>) -> Self {
        self.partner = partner;
        self
    }

    /// Shared status handle — the server reads this for its `/saga` health endpoint.
    pub fn status(&self) -> Arc<Mutex<ProcessManagerStatus>> {
        Arc::clone(&self.status)
    }

    fn status_mut(&self) -> MutexGuard<'_, ProcessManagerStatus> {
        self.status.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Drain every registered PM once, updating the per-PM checkpoints and the status snapshot. A
    /// surfaced event-leg guard error (thrown, checkpoint advanced) lands on `last_error` even when
    /// the tick itself succeeds — `/saga` shows the anomaly until a fully clean tick.
    pub async fn run_once(&self) -> Result<(), DomainError> {
        let outcome = self.tick().await;
        let mut st = self.status_mut();
        st.last_tick_at = Some(Utc::now());
        match &outcome {
            Ok(tick) => {
                st.checkpoint = tick.checkpoint;
                st.head = tick.head;
                st.lag = (tick.head - tick.checkpoint).max(0);
                st.last_error = tick.surfaced.last().cloned();
            }
            Err(e) => st.last_error = Some(e.to_string()),
        }
        outcome.map(|_| ())
    }

    /// Poll forever: `run_once` then sleep ~1.5s. Consumes the runner (spawn it as a task); the shared
    /// [`ProcessManagerStatus`] handle stays readable through [`Self::status`] clones taken before.
    /// Each tick runs in its own task so a PANIC escaping a drain (poison event, legacy payload in a
    /// fold) kills only that tick, never the loop — a dead saga runner silently stops the money path.
    pub async fn run_loop(self) {
        self.status_mut().running = true;
        let runner = std::sync::Arc::new(self);
        loop {
            // Errors are recorded on the status snapshot by run_once; the loop keeps polling.
            let r = std::sync::Arc::clone(&runner);
            if let Err(join) = tokio::spawn(async move { let _ = r.run_once().await; }).await {
                eprintln!("saga runner: tick panicked — resuming next tick: {join}");
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// One drain pass over every PM. `surfaced` collects the thrown event-leg guard errors (the runs
    /// aborted-and-advanced) so `run_once` can expose them on `/saga`.
    async fn tick(&self) -> Result<TickOutcome, DomainError> {
        let head: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(position), 0) FROM domain_events")
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)?;
        let mut surfaced = Vec::new();
        for group in REGISTRY {
            self.drain_group(group, &mut surfaced).await?;
        }
        Ok(TickOutcome { checkpoint: head, head, surfaced })
    }

    /// Drain one PM's pending trigger slice, committing its checkpoint after each event. A version
    /// conflict from an executed append PROPAGATES (abort without advancing — the leg re-runs next
    /// tick over fresh state); a thrown guard or poison event is logged, surfaced and advanced.
    async fn drain_group(
        &self,
        group: &PmGroup,
        surfaced: &mut Vec<String>,
    ) -> Result<(), DomainError> {
        let checkpoint: i64 =
            sqlx::query_scalar("SELECT position FROM projection_checkpoint WHERE projector = $1")
                .bind(group.checkpoint)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?
                .unwrap_or(0);

        let triggers: Vec<String> = group.triggers.iter().map(|t| t.to_string()).collect();
        let pending = sqlx::query(
            "SELECT position, id, correlation_id, occurred_at, event_type, payload FROM domain_events \
             WHERE position > $1 AND event_type = ANY($2) ORDER BY position",
        )
        .bind(checkpoint)
        .bind(&triggers)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        for record in pending {
            let position: i64 = record.try_get("position").map_err(db_err)?;
            match self.apply_record(group, &record).await {
                Ok(Outcome::Completed) => {}
                Ok(Outcome::Skipped(reason)) => {
                    // Benign expected alternative (idempotent re-delivery, COLLECTION no-op, failed
                    // state.expect) — logged, never an error.
                    eprintln!(
                        "saga[{}]: position {position} skipped — {reason}",
                        group.checkpoint
                    );
                }
                // Lost optimistic-concurrency race: retry the WHOLE leg next tick (the state row's
                // expect and the aggregates' record-idempotency absorb the replay).
                Err(e) if is_version_conflict(&e) => return Err(e),
                Err(e) => {
                    // A THROWN event-leg guard (typed anomaly, e.g. PaymentEventOrphaned) or a poison
                    // event: the recorded fact stands, the run aborts, the error is SURFACED on the
                    // group status and the checkpoint advances — never wedging, never silent.
                    let event_type: String = record.try_get("event_type").unwrap_or_default();
                    let msg = format!(
                        "saga[{}]: position {position} ({event_type}) aborted: {e}",
                        group.checkpoint
                    );
                    eprintln!("{msg}");
                    surfaced.push(msg);
                }
            }
            self.commit_checkpoint(group.checkpoint, position).await?;
        }
        Ok(())
    }

    /// Rebuild the typed trigger (+ its envelope) from one `domain_events` row and dispatch it to the
    /// PM's orchestrator.
    async fn apply_record(
        &self,
        group: &PmGroup,
        record: &sqlx::postgres::PgRow,
    ) -> Result<Outcome, DomainError> {
        let position: i64 = record.try_get("position").map_err(db_err)?;
        let event_id: uuid::Uuid = record.try_get("id").map_err(db_err)?;
        let correlation_id: uuid::Uuid = record.try_get("correlation_id").map_err(db_err)?;
        let occurred_at: chrono::DateTime<Utc> = record.try_get("occurred_at").map_err(db_err)?;
        let event_type: String = record.try_get("event_type").map_err(db_err)?;
        let payload: serde_json::Value = record.try_get("payload").map_err(db_err)?;
        let event: DomainEvent = serde_json::from_value(serde_json::json!({
            "eventType": event_type,
            "payload": payload,
        }))
        .map_err(|e| db_err(format!("position {position} ({event_type}): {e}")))?;

        let trigger = Trigger {
            event_type,
            event,
            envelope: TriggerEnvelope { event_id, correlation_id, occurred_at },
        };
        self.dispatch(group.pm, &trigger).await
    }

    /// Route one trigger to its orchestrator EVENT leg with the ports/stores/read models the DSL
    /// steps declare (registry and dispatch must agree).
    async fn dispatch(&self, pm: ProcessManager, trigger: &Trigger) -> Result<Outcome, DomainError> {
        let env = &trigger.envelope;
        match (pm, &trigger.event) {
            // --- PlaceOrderProcess ---------------------------------------------------------------
            (ProcessManager::PlaceOrder, DomainEvent::PaymentCaptured(e)) => {
                place_order::on_payment_captured(&self.store, &self.payment_state, e, env).await
            }
            (ProcessManager::PlaceOrder, DomainEvent::PaymentFailed(e)) => {
                place_order::on_payment_failed(&self.payment_state, e, env).await
            }
            // --- RefundProcess -------------------------------------------------------------------
            (ProcessManager::Refund, DomainEvent::OrderRejectedByRestaurant(e)) => {
                refund::on_order_rejected(&self.store, &self.refund_state, &self.orders, e, env)
                    .await
            }
            (ProcessManager::Refund, DomainEvent::OrderCancelledByCustomer(e)) => {
                refund::on_order_cancelled_by_customer(
                    &self.store,
                    &self.refund_state,
                    &self.orders,
                    e,
                    env,
                )
                .await
            }
            (ProcessManager::Refund, DomainEvent::OrderCancelledByRestaurant(e)) => {
                refund::on_order_cancelled_by_restaurant(
                    &self.store,
                    &self.refund_state,
                    &self.orders,
                    e,
                    env,
                )
                .await
            }
            (ProcessManager::Refund, DomainEvent::RefundRequested(e)) => {
                refund::on_refund_requested(&self.store, &self.refund_state, &self.orders, e, env)
                    .await
            }
            (ProcessManager::Refund, DomainEvent::PaymentRefunded(e)) => {
                refund::on_payment_refunded(&self.refund_state, e).await
            }
            // --- CartBindingProcess --------------------------------------------------------------
            (ProcessManager::CartBinding, DomainEvent::CustomerIdentified(e)) => {
                cart_binding::on_customer_identified(
                    &self.store,
                    &self.cart_binding_state,
                    &self.carts,
                    e,
                    env,
                )
                .await
            }
            // --- DeliveryDispatchProcess ---------------------------------------------------------
            (ProcessManager::DeliveryDispatch, DomainEvent::OrderMarkedReady(e)) => {
                delivery_dispatch::on_order_marked_ready(
                    &self.store,
                    &self.dispatch_state,
                    &self.orders,
                    self.partner.as_ref(),
                    e,
                    env,
                )
                .await
            }
            (ProcessManager::DeliveryDispatch, DomainEvent::DeliveryAcceptedByPartner(e)) => {
                delivery_dispatch::on_delivery_accepted_by_partner(&self.dispatch_state, e).await
            }
            (ProcessManager::DeliveryDispatch, DomainEvent::DeliveryRejectedByPartner(e)) => {
                delivery_dispatch::on_delivery_rejected_by_partner(
                    &self.store,
                    &self.dispatch_state,
                    self.partner.as_ref(),
                    e,
                    env,
                )
                .await
            }
            (ProcessManager::DeliveryDispatch, DomainEvent::DeliveryStatusUpdated(e)) => {
                delivery_dispatch::on_delivery_status_updated(&self.store, &self.dispatch_state, e, env)
                    .await
            }
            (ProcessManager::DeliveryDispatch, DomainEvent::DeliveryCompleted(e)) => {
                delivery_dispatch::on_delivery_completed(&self.store, &self.dispatch_state, e, env)
                    .await
            }
            // A trigger type outside this PM's inbox (registry and dispatch must agree).
            (pm, _) => Err(DomainError::Repository(format!(
                "saga registry/dispatch mismatch: {pm:?} does not handle {}",
                trigger.event_type
            ))),
        }
    }

    async fn commit_checkpoint(&self, projector: &str, position: i64) -> Result<(), DomainError> {
        sqlx::query(
            "INSERT INTO projection_checkpoint (projector, position, updated_at) VALUES ($1, $2, now()) \
             ON CONFLICT (projector) DO UPDATE SET position = EXCLUDED.position, updated_at = now()",
        )
        .bind(projector)
        .bind(position)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }
}

/// What one tick reports back to `run_once`.
struct TickOutcome {
    checkpoint: i64,
    head: i64,
    /// Thrown event-leg guard errors surfaced during the pass (runs aborted, checkpoints advanced).
    surfaced: Vec<String>,
}
