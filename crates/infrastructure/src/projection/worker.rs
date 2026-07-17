//! The multi-aggregate projection worker (ADR-0040): a REGISTRY of stream-prefix groups, each with its
//! OWN `projection_checkpoint` row, drained independently every tick. A group polls `domain_events`
//! past its checkpoint for its `stream_name LIKE '<Category>-%'` slice and folds each event into every
//! read model fed by that stream — e.g. the Restaurant stream feeds BOTH the `restaurant` row (generated
//! `project_restaurant` dispatch + hand-written `RestaurantProjector` hooks) and the `prospectionpipeline`
//! row (`project_prospection_pipeline` + `ProspectionPipelineProjector`). Idempotent on restart: replaying
//! an event over the current row state is a deterministic fold (`*Updated` events carry replace semantics).
//!
//! Scope note: each group folds only its own stream category, so the documented cross-stream holes stay
//! preserved by the hand-written `…Compute` impls — `Restaurant.default_currency` (owning account's
//! currency, set on the RestaurantAccount stream), the ProspectionPipeline outreach columns fed by
//! `Prospect-%` streams, `Cart.customer_id` from `CustomerIdentified` (Customer stream, keyed by
//! authRef), and any Order rating/tip/delivery/payment facts that land outside the `Order-%` stream —
//! exactly the TODO(runtime) notes in `application::projectors::*`.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use application::projections::{
    project_cart, project_catalog, project_order_tracking, project_prospection_pipeline,
    project_restaurant, Envelope,
};
use application::projectors::cart::CartProjector;
use application::projectors::catalog::CatalogProjector;
use application::projectors::order_tracking::OrderTrackingProjector;
use application::projectors::prospection_pipeline::ProspectionPipelineProjector;
use application::projectors::restaurant::RestaurantProjector;
use chrono::Utc;
use domain::generated::events::DomainEvent;
use domain::generated::scalars::{CartId, CatalogId, OrderId, RestaurantId};
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};

use crate::persistence::{
    cart_store, catalog_store, db_err, order_tracking_store, prospection_store, restaurant_store,
};
use crate::projection::ProjectionStatus;

const POLL_INTERVAL: Duration = Duration::from_millis(1500);

/// One materialized read model: resolves the aggregate id from the envelope, loads the current row via
/// its store, folds the event through its generated `project_*` dispatch + hand-written `…Compute` impl,
/// and upserts the result. Fed-but-unmatched events fall through the dispatch's `_ => state` arm.
#[derive(Clone, Copy, Debug)]
enum ReadModelProjector {
    Restaurant,
    ProspectionPipeline,
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
                let id = OrderId(aggregate_uuid_of(env, "Order-", "orderId")?);
                let state = order_tracking_store::load(pool, id).await?;
                if let Some(next) = project_order_tracking(&OrderTrackingProjector, state, env) {
                    order_tracking_store::upsert(pool, &next).await?;
                }
            }
        }
        Ok(())
    }
}

/// One drained unit: a stream category with its own checkpoint row and the read models it feeds.
struct ProjectorGroup {
    /// The `projection_checkpoint.projector` key — the stream category name.
    checkpoint: &'static str,
    /// The `stream_name LIKE '<prefix>%'` slice this group folds.
    stream_prefix: &'static str,
    /// Every read model fed by this stream, folded in order for each event.
    projectors: &'static [ReadModelProjector],
}

/// The projector registry: one group per aggregate stream feeding materialized read models. The
/// Restaurant group keeps its historical `'Restaurant'` checkpoint covering both of its folds.
const REGISTRY: &[ProjectorGroup] = &[
    ProjectorGroup {
        checkpoint: "Restaurant",
        stream_prefix: "Restaurant-",
        projectors: &[ReadModelProjector::Restaurant, ReadModelProjector::ProspectionPipeline],
    },
    ProjectorGroup {
        checkpoint: "Catalog",
        stream_prefix: "Catalog-",
        projectors: &[ReadModelProjector::Catalog],
    },
    ProjectorGroup {
        checkpoint: "Cart",
        stream_prefix: "Cart-",
        projectors: &[ReadModelProjector::Cart],
    },
    ProjectorGroup {
        checkpoint: "Order",
        stream_prefix: "Order-",
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

        let pending = sqlx::query(
            "SELECT position, stream_name, event_type, payload, occurred_at FROM domain_events \
             WHERE position > $1 AND stream_name LIKE $2 ORDER BY position",
        )
        .bind(checkpoint)
        .bind(format!("{}%", group.stream_prefix))
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        for record in pending {
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
            self.commit_checkpoint(group.checkpoint, position).await?;
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
    serde_json::to_value(&env.event)
        .ok()
        .and_then(|v| {
            v.get("payload")
                .and_then(|p| p.get(payload_key))
                .and_then(|id| id.as_str())
                .and_then(|s| uuid::Uuid::parse_str(s).ok())
        })
        .ok_or_else(|| {
            DomainError::Repository(format!(
                "cannot resolve {payload_key} for stream {}",
                env.stream_name
            ))
        })
}
