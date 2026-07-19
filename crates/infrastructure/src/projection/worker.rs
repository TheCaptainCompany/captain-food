//! The multi-aggregate projection worker (ADR-0040): a REGISTRY of stream-prefix groups, each with its
//! OWN `projection_checkpoint` row, drained independently every tick. A group polls `domain_events`
//! past its checkpoint for its `stream_name LIKE '<Category>-%'` slice and folds each event into every
//! read model fed by that stream — e.g. the Restaurant stream feeds BOTH the `restaurant` row (generated
//! `project_restaurant` dispatch + hand-written `RestaurantProjector` hooks) and the `prospectionpipeline`
//! row (`project_prospection_pipeline` + `ProspectionPipelineProjector`). Idempotent on restart: replaying
//! an event over the current row state is a deterministic fold (`*Updated` events carry replace semantics).
//!
//! Scope note: a group folds its declared stream categories. Most groups slice a single
//! `<Category>-%` prefix; the Order group also slices `Payment-%` (same checkpoint, so global
//! `position` order is preserved across both categories) because `PaymentCaptured`/`PaymentRefunded`
//! land on `Payment-{intentId}` streams but feed `OrderTracking.payment_status` — the row key is then
//! resolved from the payload's `orderId`, not the stream name. The remaining documented cross-stream
//! holes stay preserved by the hand-written `…Compute` impls — `Restaurant.default_currency` (owning
//! account's currency, set on the RestaurantAccount stream), the ProspectionPipeline outreach columns
//! fed by `Prospect-%` streams, and `Cart.customer_id` from `CustomerIdentified` (Customer stream,
//! keyed by authRef) — exactly the TODO(runtime) notes in `application::projectors::*`.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use application::projections::{
    project_cart, project_catalog, project_customer, project_order_tracking,
    project_prospection_pipeline, project_restaurant, Envelope,
};
use application::projectors::cart::CartProjector;
use application::projectors::catalog::CatalogProjector;
use application::projectors::customer::CustomerProjector;
use application::projectors::order_tracking::OrderTrackingProjector;
use application::projectors::prospection_pipeline::ProspectionPipelineProjector;
use application::projectors::restaurant::RestaurantProjector;
use chrono::Utc;
use domain::generated::events::DomainEvent;
use domain::generated::scalars::{CartId, CatalogId, CustomerId, OrderId, RestaurantId};
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};

use crate::persistence::{
    cart_store, catalog_store, customer_store, db_err, order_tracking_store, prospection_store,
    restaurant_store,
};
use crate::projection::ProjectionStatus;

const POLL_INTERVAL: Duration = Duration::from_millis(1500);
// NOTE: the drain fetches ALL pending events per tick (no LIMIT) and loops, so there is no
// batch cap. If projection appears stuck below the event count, the cause is the app being idle
// (Render free-tier spin-down pauses the in-process worker after ~15 min) — keep it warm with a
// periodic /ping — or, historically, a poison event that wedged the loop (now log-skipped below).

/// One materialized read model: resolves the aggregate id from the envelope, loads the current row via
/// its store, folds the event through its generated `project_*` dispatch + hand-written `…Compute` impl,
/// and upserts the result. Fed-but-unmatched events fall through the dispatch's `_ => state` arm.
#[derive(Clone, Copy, Debug)]
enum ReadModelProjector {
    Restaurant,
    ProspectionPipeline,
    Customer,
    Catalog,
    Cart,
    OrderTracking,
}

impl ReadModelProjector {
    async fn apply(&self, pool: &PgPool, env: &Envelope) -> Result<(), DomainError> {
        match self {
            Self::Restaurant => {
                let id = RestaurantId(aggregate_uuid_of(env, "Restaurant-", "restaurantId")?);
                let state = restaurant_store::load(pool, id).await?;
                if let Some(next) = project_restaurant(&RestaurantProjector, state, env) {
                    restaurant_store::upsert(pool, &next).await?;
                }
            }
            Self::ProspectionPipeline => {
                let id = RestaurantId(aggregate_uuid_of(env, "Restaurant-", "restaurantId")?);
                let state = prospection_store::load(pool, id).await?;
                if let Some(next) = project_prospection_pipeline(&ProspectionPipelineProjector, state, env)
                {
                    prospection_store::upsert(pool, &next).await?;
                }
            }
            Self::Customer => {
                let id = CustomerId(aggregate_uuid_of(env, "Customer-", "customerId")?);
                let state = customer_store::load(pool, id).await?;
                if let Some(next) = project_customer(&CustomerProjector, state, env) {
                    customer_store::upsert(pool, &next).await?;
                }
            }
            Self::Catalog => {
                let id = CatalogId(aggregate_uuid_of(env, "Catalog-", "catalogId")?);
                let state = catalog_store::load(pool, id).await?;
                if let Some(next) = project_catalog(&CatalogProjector, state, env) {
                    catalog_store::upsert(pool, &next).await?;
                }
            }
            Self::Cart => {
                let id = CartId(aggregate_uuid_of(env, "Cart-", "cartId")?);
                let state = cart_store::load(pool, id).await?;
                if let Some(next) = project_cart(&CartProjector, state, env) {
                    cart_store::upsert(pool, &next).await?;
                }
            }
            Self::OrderTracking => {
                // Cross-stream feed: the group also slices `Payment-%`, whose facts key the Order
                // row from the payload's `orderId` (PaymentRefunded always carries it; a
                // PaymentCaptured not yet tied to an order has no row to feed, and PaymentFailed
                // never references one — both are skipped with a log, not treated as poison).
                let uuid = if env.stream_name.starts_with("Order-") {
                    aggregate_uuid_of(env, "Order-", "orderId")?
                } else {
                    match payload_uuid_of(env, "orderId") {
                        Some(uuid) => uuid,
                        None => {
                            eprintln!(
                                "projection[Order]: no orderId in payload for stream {} at position {} — skipped",
                                env.stream_name, env.position
                            );
                            return Ok(());
                        }
                    }
                };
                let id = OrderId(uuid);
                let state = order_tracking_store::load(pool, id).await?;
                if let Some(next) = project_order_tracking(&OrderTrackingProjector, state, env) {
                    order_tracking_store::upsert(pool, &next).await?;
                }
            }
        }
        Ok(())
    }
}

/// One drained unit: one or more stream categories sharing a checkpoint row and the read models
/// they feed. A single checkpoint over several prefixes keeps the fold ordered by global
/// `position` across those categories (no per-category race), which is exactly what the
/// Order + Payment cross-stream feed needs.
struct ProjectorGroup {
    /// The `projection_checkpoint.projector` key — the (primary) stream category name.
    checkpoint: &'static str,
    /// The `stream_name LIKE ANY('{<prefix>%, …}')` slice this group folds.
    stream_prefixes: &'static [&'static str],
    /// Every read model fed by these streams, folded in order for each event.
    projectors: &'static [ReadModelProjector],
}

/// The projector registry: one group per aggregate stream feeding materialized read models. The
/// Restaurant group keeps its historical `'Restaurant'` checkpoint covering both of its folds.
const REGISTRY: &[ProjectorGroup] = &[
    ProjectorGroup {
        checkpoint: "Restaurant",
        stream_prefixes: &["Restaurant-"],
        projectors: &[ReadModelProjector::Restaurant, ReadModelProjector::ProspectionPipeline],
    },
    ProjectorGroup {
        checkpoint: "Customer",
        stream_prefixes: &["Customer-"],
        projectors: &[ReadModelProjector::Customer],
    },
    ProjectorGroup {
        checkpoint: "Catalog",
        stream_prefixes: &["Catalog-"],
        projectors: &[ReadModelProjector::Catalog],
    },
    ProjectorGroup {
        checkpoint: "Cart",
        stream_prefixes: &["Cart-"],
        projectors: &[ReadModelProjector::Cart],
    },
    // The Payment-% slice closes the OrderTracking.payment_status feed gap (docs/sagas.md;
    // ADR-20260719-193500): PaymentCaptured/PaymentRefunded live on Payment-{intentId} streams
    // but are declared in the ordertracking fedBy. Same 'Order' checkpoint = one ordered fold.
    ProjectorGroup {
        checkpoint: "Order",
        stream_prefixes: &["Order-", "Payment-"],
        projectors: &[ReadModelProjector::OrderTracking],
    },
];

pub struct ProjectionWorker {
    pool: PgPool,
    status: Arc<Mutex<ProjectionStatus>>,
}

impl ProjectionWorker {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, status: Arc::new(Mutex::new(ProjectionStatus::default())) }
    }

    /// Shared status handle — the server reads this for its `/projector` health endpoint.
    pub fn status(&self) -> Arc<Mutex<ProjectionStatus>> {
        Arc::clone(&self.status)
    }

    fn status_mut(&self) -> MutexGuard<'_, ProjectionStatus> {
        // A poisoned lock only means a reader panicked mid-inspection; the snapshot stays usable.
        self.status.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Drain every registry group once, updating the per-group checkpoints and the status snapshot.
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

    /// Poll forever: `run_once` then sleep ~1.5s. Consumes the worker (spawn it as a task); the shared
    /// [`ProjectionStatus`] handle stays readable through [`Self::status`] clones taken before spawning.
    pub async fn run_loop(self) {
        self.status_mut().running = true;
        loop {
            // Errors are recorded on the status snapshot by run_once; the loop keeps polling.
            let _ = self.run_once().await;
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// One drain pass over every group. Returns the AGGREGATE `(checkpoint, head)`: a successful pass
    /// means every group folded everything pending for its streams, so the read models are caught up to
    /// the `head` observed at the start of the pass (each group's DB checkpoint stays conservative — it
    /// only advances on folded events; foreign-stream positions re-scan as cheap no-ops next tick).
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

    /// Drain one group's pending slice, folding each event into every read model the group feeds and
    /// committing the group's checkpoint after each event.
    async fn drain_group(&self, group: &ProjectorGroup) -> Result<(), DomainError> {
        let checkpoint: i64 =
            sqlx::query_scalar("SELECT position FROM projection_checkpoint WHERE projector = $1")
                .bind(group.checkpoint)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?
                .unwrap_or(0);

        let patterns: Vec<String> =
            group.stream_prefixes.iter().map(|prefix| format!("{prefix}%")).collect();
        let pending = sqlx::query(
            "SELECT position, stream_name, event_type, payload, occurred_at FROM domain_events \
             WHERE position > $1 AND stream_name LIKE ANY($2) ORDER BY position",
        )
        .bind(checkpoint)
        .bind(&patterns)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        for record in pending {
            let position: i64 = record.try_get("position").map_err(db_err)?;
            // A per-event failure (unparseable payload, fold/upsert error) is LOGGED and SKIPPED rather
            // than wedging the whole group: with the old `?` a single poison event re-failed every tick
            // and halted ALL further projection (one bad SIRENE record could freeze the other ~800). The
            // checkpoint still advances so the pipeline makes progress; the event stays in domain_events
            // for a future full reprojection. A failure committing the checkpoint itself DOES propagate —
            // that's a transient DB error worth retrying next tick, not a poison record.
            if let Err(e) = self.apply_record(group, &record).await {
                let event_type: String = record.try_get("event_type").unwrap_or_default();
                eprintln!(
                    "projection[{}]: skipped position {position} ({event_type}): {e}",
                    group.checkpoint
                );
            }
            self.commit_checkpoint(group.checkpoint, position).await?;
        }
        Ok(())
    }

    /// Fold one `domain_events` row into every read model the group feeds. Returns a per-event error so
    /// the caller can log-and-skip a poison record without halting the group.
    async fn apply_record(
        &self,
        group: &ProjectorGroup,
        record: &sqlx::postgres::PgRow,
    ) -> Result<(), DomainError> {
        let position: i64 = record.try_get("position").map_err(db_err)?;
        let stream_name: String = record.try_get("stream_name").map_err(db_err)?;
        let event_type: String = record.try_get("event_type").map_err(db_err)?;
        let payload: serde_json::Value = record.try_get("payload").map_err(db_err)?;
        let occurred_at: chrono::DateTime<Utc> = record.try_get("occurred_at").map_err(db_err)?;

        // Rebuild the typed event from the (event_type, payload) columns via the adjacent tag.
        let event: DomainEvent = serde_json::from_value(serde_json::json!({
            "eventType": event_type,
            "payload": payload,
        }))
        .map_err(|e| db_err(format!("position {position} ({event_type}): {e}")))?;

        let env = Envelope { stream_name, position, occurred_at, event };
        for projector in group.projectors {
            projector.apply(&self.pool, &env).await?;
        }
        Ok(())
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

/// The aggregate id an event belongs to: parsed from the `<Category>-<uuid>` stream name, falling back
/// to the payload's own id field (every same-stream event carries its aggregate id).
fn aggregate_uuid_of(env: &Envelope, prefix: &str, payload_key: &str) -> Result<uuid::Uuid, DomainError> {
    if let Some(suffix) = env.stream_name.strip_prefix(prefix) {
        if let Ok(id) = uuid::Uuid::parse_str(suffix) {
            return Ok(id);
        }
    }
    payload_uuid_of(env, payload_key).ok_or_else(|| {
        DomainError::Repository(format!(
            "cannot resolve {payload_key} for stream {}",
            env.stream_name
        ))
    })
}

/// The uuid carried by the event payload under `payload_key`, if any — the row key for
/// cross-stream feeds (e.g. `Payment-%` facts keying the Order row by their `orderId`).
fn payload_uuid_of(env: &Envelope, payload_key: &str) -> Option<uuid::Uuid> {
    serde_json::to_value(&env.event).ok().and_then(|v| {
        v.get("payload")
            .and_then(|p| p.get(payload_key))
            .and_then(|id| id.as_str())
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::generated::events::{self, DomainEvent};
    use domain::generated::entities::Money;
    use domain::generated::scalars::{CurrencyCode, MoneyCents, PaymentIntentId, RefundId};

    fn envelope(stream_name: &str, event: DomainEvent) -> Envelope {
        Envelope { stream_name: stream_name.to_string(), position: 1, occurred_at: Utc::now(), event }
    }

    fn money() -> Money {
        Money { amount_cents: MoneyCents(1000), currency: CurrencyCode("EUR".to_string()) }
    }

    /// A Payment-stream capture keys the Order row from the payload's `orderId`, not the stream.
    #[test]
    fn payment_captured_row_id_comes_from_payload_order_id() {
        let order_id = uuid::Uuid::new_v4();
        let env = envelope(
            "Payment-pi_test_123",
            DomainEvent::PaymentCaptured(events::PaymentCaptured {
                payment_intent_id: PaymentIntentId("pi_test_123".to_string()),
                order_id: Some(OrderId(order_id)),
                restaurant_id: RestaurantId(uuid::Uuid::new_v4()),
                amount: money(),
            }),
        );
        assert_eq!(payload_uuid_of(&env, "orderId"), Some(order_id));
        // The strict resolver reaches the same id through its payload fallback.
        assert_eq!(aggregate_uuid_of(&env, "Order-", "orderId").unwrap(), order_id);
    }

    /// A capture not (yet) tied to an order resolves to no row key — the worker log-skips it.
    #[test]
    fn payment_captured_without_order_id_resolves_to_none() {
        let env = envelope(
            "Payment-pi_test_456",
            DomainEvent::PaymentCaptured(events::PaymentCaptured {
                payment_intent_id: PaymentIntentId("pi_test_456".to_string()),
                order_id: None,
                restaurant_id: RestaurantId(uuid::Uuid::new_v4()),
                amount: money(),
            }),
        );
        assert_eq!(payload_uuid_of(&env, "orderId"), None);
        assert!(aggregate_uuid_of(&env, "Order-", "orderId").is_err());
    }

    /// PaymentRefunded always carries its order id, so the Order row key always resolves.
    #[test]
    fn payment_refunded_row_id_comes_from_payload_order_id() {
        let order_id = uuid::Uuid::new_v4();
        let env = envelope(
            "Payment-pi_test_789",
            DomainEvent::PaymentRefunded(events::PaymentRefunded {
                refund_id: RefundId("re_test_1".to_string()),
                payment_intent_id: PaymentIntentId("pi_test_789".to_string()),
                order_id: OrderId(order_id),
                restaurant_id: RestaurantId(uuid::Uuid::new_v4()),
                amount: money(),
                reason: None,
            }),
        );
        assert_eq!(payload_uuid_of(&env, "orderId"), Some(order_id));
    }

    /// Same-stream events keep keying off the stream uuid (payload untouched).
    #[test]
    fn order_stream_row_id_comes_from_stream_name() {
        let order_id = uuid::Uuid::new_v4();
        let env = envelope(
            &format!("Order-{order_id}"),
            // Any event will do: the stream uuid wins before the payload is consulted.
            DomainEvent::PaymentRefunded(events::PaymentRefunded {
                refund_id: RefundId("re_test_2".to_string()),
                payment_intent_id: PaymentIntentId("pi_test_000".to_string()),
                order_id: OrderId(uuid::Uuid::new_v4()),
                restaurant_id: RestaurantId(uuid::Uuid::new_v4()),
                amount: money(),
                reason: None,
            }),
        );
        assert_eq!(aggregate_uuid_of(&env, "Order-", "orderId").unwrap(), order_id);
    }
}
