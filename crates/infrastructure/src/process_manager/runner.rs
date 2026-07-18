//! The saga runner (process-manager runtime), mirroring `projection::worker::ProjectionWorker`:
//!
//! - a REGISTRY of process managers, each with its OWN `projection_checkpoint` row (`pm:<Name>`) and
//!   the trigger `event_type`s its actors.yaml inbox declares;
//! - each tick, every PM drains `domain_events` past its checkpoint for those event types (a PM's
//!   triggers cross stream categories — e.g. `PaymentCaptured` lands on `StripeEvent-%` streams — so
//!   the slice is by EVENT TYPE, not stream prefix);
//! - each pending event is dispatched to the PM's PURE decision (`application::process_managers`),
//!   with the streams the decision needs pre-loaded through the `EventStore` port, and the returned
//!   appends are executed under the saga's system actor (trigger-correlated envelope, ADR-0041);
//! - the checkpoint is committed after each event; a poison event (unparseable payload, decision
//!   plumbing error) is LOGGED and SKIPPED like the projection worker's, so it never wedges the loop.
//!
//! Idempotency: a PM re-reacting to the same trigger (redelivery, checkpoint replay after a crash
//! between append and commit) is absorbed by the decisions themselves — they fold the TARGET stream
//! first and return `Nothing` when the reaction's fact is already recorded, and stream-birthing
//! reactions use deterministic ids (see the module doc in `application::process_managers`). A VERSION
//! CONFLICT on an executed append is a lost race, not poison: the drain aborts WITHOUT advancing the
//! checkpoint and the whole reaction re-runs next tick over the fresh stream state.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use application::ports::{is_version_conflict, Actor, CheckoutSnapshotSource, EventStore};
use application::process_managers::{
    cart_binding, cart_stream, delivery_dispatch, delivery_job_stream, order_stream, place_order,
    refund, restaurant_stream, Decision,
};
use chrono::Utc;
use domain::generated::events::DomainEvent;
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};

use crate::persistence::{db_err, PgEventStore};
use crate::process_manager::ProcessManagerStatus;

const POLL_INTERVAL: Duration = Duration::from_millis(1500);

/// `UserType::EXTERNAL` ordinal (declaration-order ints, ADR-0037): the envelope principal for
/// non-human system appends — the same convention the SIRENE/Stripe ACLs use (scalars.yaml has no
/// dedicated SYSTEM member; adding one would be a DSL change).
const EXTERNAL_USER_TYPE: i32 = 6;

/// Fixed system user id stamping saga-emitted events (`domain_events.user_id`, ADR-0041).
fn saga_system_user_id() -> uuid::Uuid {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"https://captain.food/process-managers")
}

/// The registered process managers (actors.yaml `type: process-manager`).
#[derive(Clone, Copy, Debug)]
enum ProcessManager {
    PlaceOrder,
    Refund,
    CartBinding,
    DeliveryDispatch,
}

/// One drained unit: a process manager with its own checkpoint row and the trigger event types its
/// actors.yaml inbox declares (the EVENT entries only — command legs run in the mutation handlers).
struct PmGroup {
    /// The `projection_checkpoint.projector` key (`pm:` prefix keeps saga rows apart from projector rows).
    checkpoint: &'static str,
    pm: ProcessManager,
    /// The `event_type` slice this PM drains (serde tags of the trigger events).
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

/// One trigger row as read from `domain_events` — the envelope bits a saga reaction needs.
struct Trigger {
    position: i64,
    event_id: uuid::Uuid,
    correlation_id: uuid::Uuid,
    event_type: String,
    event: DomainEvent,
}

pub struct ProcessManagerRunner {
    pool: PgPool,
    /// Appends/loads go through the ordinary event-store port (same adapter the command handlers use).
    store: PgEventStore,
    /// PlaceOrderProcess's checkout-resolution seam (fail-closed stand-in until the Stripe adapter lands).
    snapshots: Arc<dyn CheckoutSnapshotSource>,
    status: Arc<Mutex<ProcessManagerStatus>>,
}

impl ProcessManagerRunner {
    pub fn new(pool: PgPool, snapshots: Arc<dyn CheckoutSnapshotSource>) -> Self {
        Self {
            store: PgEventStore::new(pool.clone()),
            pool,
            snapshots,
            status: Arc::new(Mutex::new(ProcessManagerStatus::default())),
        }
    }

    /// Shared status handle — the server reads this for its `/saga` health endpoint.
    pub fn status(&self) -> Arc<Mutex<ProcessManagerStatus>> {
        Arc::clone(&self.status)
    }

    fn status_mut(&self) -> MutexGuard<'_, ProcessManagerStatus> {
        self.status.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Drain every registered PM once, updating the per-PM checkpoints and the status snapshot.
    pub async fn run_once(&self) -> Result<(), DomainError> {
        let outcome = self.tick().await;
        let mut st = self.status_mut();
        st.last_tick_at = Some(Utc::now());
        match &outcome {
            Ok((checkpoint, head)) => {
                st.checkpoint = *checkpoint;
                st.head = *head;
                st.lag = (*head - *checkpoint).max(0);
                st.last_error = None;
            }
            Err(e) => st.last_error = Some(e.to_string()),
        }
        outcome.map(|_| ())
    }

    /// Poll forever: `run_once` then sleep ~1.5s. Consumes the runner (spawn it as a task); the shared
    /// [`ProcessManagerStatus`] handle stays readable through [`Self::status`] clones taken before.
    pub async fn run_loop(self) {
        self.status_mut().running = true;
        loop {
            // Errors are recorded on the status snapshot by run_once; the loop keeps polling.
            let _ = self.run_once().await;
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// One drain pass over every PM. Returns the AGGREGATE `(checkpoint, head)` like the projection
    /// worker: a successful pass means every PM reacted to everything pending for its triggers.
    async fn tick(&self) -> Result<(i64, i64), DomainError> {
        let head: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(position), 0) FROM domain_events")
            .fetch_one(&self.pool)
            .await
            .map_err(db_err)?;
        for group in REGISTRY {
            self.drain_group(group).await?;
        }
        Ok((head, head))
    }

    /// Drain one PM's pending trigger slice, committing its checkpoint after each event. A version
    /// conflict from an executed append PROPAGATES (abort without advancing — the reaction re-runs
    /// next tick over fresh state); anything else per-event is logged and skipped (poison guard).
    async fn drain_group(&self, group: &PmGroup) -> Result<(), DomainError> {
        let checkpoint: i64 =
            sqlx::query_scalar("SELECT position FROM projection_checkpoint WHERE projector = $1")
                .bind(group.checkpoint)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?
                .unwrap_or(0);

        let triggers: Vec<String> = group.triggers.iter().map(|t| t.to_string()).collect();
        let pending = sqlx::query(
            "SELECT position, id, correlation_id, event_type, payload FROM domain_events \
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
                Ok(()) => {}
                // Lost optimistic-concurrency race: retry the WHOLE reaction next tick (the pure
                // decision then folds the fresh stream state — idempotent by construction).
                Err(e) if is_version_conflict(&e) => return Err(e),
                Err(e) => {
                    let event_type: String = record.try_get("event_type").unwrap_or_default();
                    eprintln!(
                        "saga[{}]: skipped position {position} ({event_type}): {e}",
                        group.checkpoint
                    );
                }
            }
            self.commit_checkpoint(group.checkpoint, position).await?;
        }
        Ok(())
    }

    /// Rebuild the typed trigger from one `domain_events` row, dispatch it to the PM, and execute the
    /// decision. Returns a per-event error so the caller can poison-skip (or retry, for conflicts).
    async fn apply_record(
        &self,
        group: &PmGroup,
        record: &sqlx::postgres::PgRow,
    ) -> Result<(), DomainError> {
        let position: i64 = record.try_get("position").map_err(db_err)?;
        let event_id: uuid::Uuid = record.try_get("id").map_err(db_err)?;
        let correlation_id: uuid::Uuid = record.try_get("correlation_id").map_err(db_err)?;
        let event_type: String = record.try_get("event_type").map_err(db_err)?;
        let payload: serde_json::Value = record.try_get("payload").map_err(db_err)?;
        let event: DomainEvent = serde_json::from_value(serde_json::json!({
            "eventType": event_type,
            "payload": payload,
        }))
        .map_err(|e| db_err(format!("position {position} ({event_type}): {e}")))?;

        let trigger = Trigger { position, event_id, correlation_id, event_type, event };
        let decision = self.decide(group.pm, &trigger).await?;
        self.execute(group, &trigger, decision).await
    }

    /// Load whatever streams the PM's pure decision needs, then take the decision. Mirrors the
    /// projection worker's per-read-model dispatch, but over the saga inboxes (actors.yaml).
    async fn decide(&self, pm: ProcessManager, trigger: &Trigger) -> Result<Decision, DomainError> {
        Ok(match (pm, &trigger.event) {
            // --- PlaceOrderProcess ---------------------------------------------------------------
            (ProcessManager::PlaceOrder, DomainEvent::PaymentCaptured(e)) => {
                let snapshot = self.snapshots.by_payment_intent(&e.payment_intent_id).await?;
                match &snapshot {
                    None => place_order::on_payment_captured(e, None, &[], 0, &[], 0),
                    Some(snap) => {
                        let (order_events, order_version) =
                            self.store.load(&order_stream(&snap.order_id)).await?;
                        let (cart_events, cart_version) =
                            self.store.load(&cart_stream(&snap.cart_id)).await?;
                        place_order::on_payment_captured(
                            e,
                            Some(snap),
                            &order_events,
                            order_version,
                            &cart_events,
                            cart_version,
                        )
                    }
                }
            }
            (ProcessManager::PlaceOrder, DomainEvent::PaymentFailed(e)) => {
                place_order::on_payment_failed(e)
            }
            // --- RefundProcess -------------------------------------------------------------------
            (ProcessManager::Refund, DomainEvent::OrderRejectedByRestaurant(e)) => {
                refund::on_order_rejected(e)
            }
            (ProcessManager::Refund, DomainEvent::OrderCancelledByCustomer(e)) => {
                refund::on_order_cancelled_by_customer(e)
            }
            (ProcessManager::Refund, DomainEvent::OrderCancelledByRestaurant(e)) => {
                refund::on_order_cancelled_by_restaurant(e)
            }
            (ProcessManager::Refund, DomainEvent::RefundRequested(e)) => {
                refund::on_refund_requested(e)
            }
            (ProcessManager::Refund, DomainEvent::PaymentRefunded(e)) => {
                refund::on_payment_refunded(e)
            }
            // --- CartBindingProcess --------------------------------------------------------------
            (ProcessManager::CartBinding, DomainEvent::CustomerIdentified(e)) => {
                cart_binding::on_customer_identified(e)
            }
            // --- DeliveryDispatchProcess ---------------------------------------------------------
            (ProcessManager::DeliveryDispatch, DomainEvent::OrderMarkedReady(e)) => {
                let (order_events, _) = self.store.load(&order_stream(&e.order_id)).await?;
                let (restaurant_events, _) =
                    self.store.load(&restaurant_stream(&e.restaurant_id)).await?;
                let job_id = delivery_dispatch::delivery_job_id_for(&e.order_id);
                let (job_events, _) = self.store.load(&delivery_job_stream(&job_id)).await?;
                delivery_dispatch::on_order_marked_ready(
                    e,
                    &order_events,
                    &restaurant_events,
                    &job_events,
                )
            }
            (ProcessManager::DeliveryDispatch, DomainEvent::DeliveryAcceptedByPartner(e)) => {
                delivery_dispatch::on_delivery_accepted_by_partner(e)
            }
            (ProcessManager::DeliveryDispatch, DomainEvent::DeliveryRejectedByPartner(e)) => {
                delivery_dispatch::on_delivery_rejected_by_partner(e)
            }
            (ProcessManager::DeliveryDispatch, DomainEvent::DeliveryStatusUpdated(e)) => {
                let (job_events, _) =
                    self.store.load(&delivery_job_stream(&e.delivery_job_id)).await?;
                let (order_events, order_version) =
                    self.load_job_order(&job_events).await?;
                delivery_dispatch::on_delivery_status_updated(
                    e,
                    &job_events,
                    &order_events,
                    order_version,
                )
            }
            (ProcessManager::DeliveryDispatch, DomainEvent::DeliveryCompleted(e)) => {
                let (job_events, _) =
                    self.store.load(&delivery_job_stream(&e.delivery_job_id)).await?;
                let (order_events, order_version) =
                    self.load_job_order(&job_events).await?;
                delivery_dispatch::on_delivery_completed(
                    e,
                    &job_events,
                    &order_events,
                    order_version,
                )
            }
            // A trigger type outside this PM's inbox (registry and dispatch must agree).
            (pm, event) => {
                return Err(DomainError::Repository(format!(
                    "saga registry/dispatch mismatch: {pm:?} does not handle {}",
                    event_type_of(event)
                )))
            }
        })
    }

    /// The order stream a delivery job points at (via its `DeliveryRequested` birth event); an
    /// unresolvable job yields an empty stream so the pure decision reports the precise Skip.
    async fn load_job_order(
        &self,
        job_events: &[DomainEvent],
    ) -> Result<(Vec<DomainEvent>, i64), DomainError> {
        match delivery_dispatch::requested_order(job_events) {
            Some((order_id, _)) => self.store.load(&order_stream(&order_id)).await,
            None => Ok((Vec::new(), 0)),
        }
    }

    /// Execute one decision: run the appends under the saga's system actor — trigger-correlated and
    /// caused-by the trigger event (ADR-0041) — or log the Skip.
    async fn execute(
        &self,
        group: &PmGroup,
        trigger: &Trigger,
        decision: Decision,
    ) -> Result<(), DomainError> {
        match decision {
            Decision::Nothing => Ok(()),
            Decision::Skip(reason) => {
                eprintln!(
                    "saga[{}]: position {} ({}) — {reason}",
                    group.checkpoint, trigger.position, trigger.event_type
                );
                Ok(())
            }
            Decision::Act(appends) => {
                let actor = Actor {
                    user_id: saga_system_user_id(),
                    user_type: EXTERNAL_USER_TYPE,
                    correlation_id: trigger.correlation_id,
                    cause_id: Some(trigger.event_id),
                };
                for append in appends {
                    self.store
                        .append(&append.stream_name, append.expected_version, &append.events, &actor)
                        .await?;
                }
                Ok(())
            }
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

/// The serde tag of a domain event (its `event_type` column value) — for the mismatch diagnostic.
fn event_type_of(event: &DomainEvent) -> String {
    serde_json::to_value(event)
        .ok()
        .and_then(|v| v.get("eventType").and_then(|t| t.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "<unknown>".to_string())
}
