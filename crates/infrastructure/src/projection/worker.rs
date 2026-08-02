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

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use application::projections::{
    project_cart, project_catalog, project_customer, project_customer_credit_balance,
    project_order_conversation, project_order_tracking, project_prospection_pipeline,
    project_restaurant, project_slug_alias, Envelope,
};
use application::projectors::cart::CartProjector;
use application::projectors::catalog::CatalogProjector;
use application::projectors::customer::CustomerProjector;
use application::projectors::customer_credit_balance::CustomerCreditBalanceProjector;
use application::projectors::order_conversation::OrderConversationProjector;
use application::projectors::order_tracking::OrderTrackingProjector;
use application::projectors::prospection_pipeline::ProspectionPipelineProjector;
use application::projectors::restaurant::RestaurantProjector;
use application::projectors::slug_alias::SlugAliasProjector;
use chrono::Utc;
use domain::generated::events::DomainEvent;
use domain::generated::scalars::{CartId, CatalogId, CustomerId, OrderId, RestaurantId};
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};
use tracing::Instrument as _;

use crate::persistence::event_wake::EventWaiter;
use crate::persistence::{
    cart_store, catalog_store, customer_credit_balance_store, customer_store, db_err,
    order_conversation_store, order_tracking_store, prospection_store, restaurant_store,
    slug_alias_store,
};
use crate::projection::ProjectionStatus;

/// Unassisted cadence: how often the loop drains when there is no push (`event_wake` listener down,
/// or a deployment that never wired one). Unchanged from the original always-poll behaviour, so
/// losing push degrades to what this worker always did.
const POLL_INTERVAL: Duration = Duration::from_millis(1500);
/// Safety-net cadence while push IS live. `NOTIFY` has no replay, so the loop still drains on its
/// own in case a signal was missed; it can be slow because the listener normally beats it.
const PUSH_SAFETY_INTERVAL: Duration = Duration::from_secs(60);
/// Events folded per BATCH TRANSACTION (PROP-20260730-230803: "process a group of messages in
/// memory then commit the changes to the database in one transaction for the batch"). Overridable
/// via `PROJECTION_BATCH_SIZE` (specs/configuration.yaml) and per-instance for tests.
const DEFAULT_BATCH_SIZE: i64 = 500;
// If projection appears stuck below the event count, the cause is the app being idle (Render
// free-tier spin-down pauses the in-process worker after ~15 min) — keep it warm with a periodic
// /ping — or, historically, a poison event that wedged the loop (now log-skipped below).

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
    OrderConversation,
    CustomerCreditBalance,
    /// Keyed by the SUPERSEDED slug from the event payload, not by an aggregate id — one row per
    /// rename, so a restaurant renamed N times leaves N rows on the same stream.
    SlugAlias,
}

impl ReadModelProjector {
    /// The read model this projector maintains — `business.projection_name` on the
    /// `event.consume.projection` span, and the label a projection-lag alert is grouped by. Derived
    /// from the variant via `Debug` so a new projector cannot be added without a name.
    fn projection_name(&self) -> String {
        format!("{self:?}")
    }

    /// `event.consume.projection` (CONSUMER) — the instrumentation boundary for one event applied to
    /// one read model.
    ///
    /// The span sits HERE rather than around the whole drain batch: the contract declares
    /// `multiplicity: ">= 1"` for this span, i.e. one per projection touched, and the group for an
    /// `Order-` event fans out to several projectors. A batch-level span would collapse all of them
    /// into one and lose which read model was slow or failing — the only thing the span is for.
    async fn apply(&self, conn: &mut sqlx::PgConnection, env: &Envelope) -> Result<(), DomainError> {
        let span = telemetry::spans::event_consume_projection(&self.projection_name());
        self.apply_inner(conn, env).instrument(span).await
    }

    /// Runs on the BATCH TRANSACTION's connection: loads see the batch's own uncommitted upserts
    /// (read-your-writes within a batch — event N+1 for a row folds over event N's result), and
    /// every upsert commits (or rolls back) with the batch checkpoint.
    async fn apply_inner(&self, conn: &mut sqlx::PgConnection, env: &Envelope) -> Result<(), DomainError> {
        match self {
            Self::Restaurant => {
                let id = RestaurantId(aggregate_uuid_of(env, "Restaurant-", "restaurantId")?);
                let state = restaurant_store::load(&mut *conn, id).await?;
                if let Some(next) = project_restaurant(&RestaurantProjector, state, env) {
                    restaurant_store::upsert(&mut *conn, &next).await?;
                }
            }
            Self::ProspectionPipeline => {
                let id = RestaurantId(aggregate_uuid_of(env, "Restaurant-", "restaurantId")?);
                let state = prospection_store::load(&mut *conn, id).await?;
                if let Some(next) = project_prospection_pipeline(&ProspectionPipelineProjector, state, env)
                {
                    prospection_store::upsert(&mut *conn, &next).await?;
                }
            }
            Self::Customer => {
                let id = CustomerId(aggregate_uuid_of(env, "Customer-", "customerId")?);
                let state = customer_store::load(&mut *conn, id).await?;
                if let Some(next) = project_customer(&CustomerProjector, state, env) {
                    customer_store::upsert(&mut *conn, &next).await?;
                }
            }
            Self::Catalog => {
                let id = CatalogId(aggregate_uuid_of(env, "Catalog-", "catalogId")?);
                let state = catalog_store::load(&mut *conn, id).await?;
                if let Some(next) = project_catalog(&CatalogProjector, state, env) {
                    catalog_store::upsert(&mut *conn, &next).await?;
                }
            }
            Self::Cart => {
                let id = CartId(aggregate_uuid_of(env, "Cart-", "cartId")?);
                let state = cart_store::load(&mut *conn, id).await?;
                if let Some(next) = project_cart(&CartProjector, state, env) {
                    cart_store::upsert(&mut *conn, &next).await?;
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
                            tracing::warn!(
                                projection = "OrderTracking",
                                stream = %env.stream_name,
                                position = env.position,
                                "no orderId in payload -- event skipped (not poison)"
                            );
                            return Ok(());
                        }
                    }
                };
                let id = OrderId(uuid);
                let state = order_tracking_store::load(&mut *conn, id).await?;
                if let Some(next) = project_order_tracking(&OrderTrackingProjector, state, env) {
                    order_tracking_store::upsert(&mut *conn, &next).await?;
                }
            }
            Self::OrderConversation => {
                // Cross-stream feed: the conversation's own events live on `Conversation-{orderId}`
                // (the aggregate is keyed by order_id), while the folded order STATUS arrives on the
                // `Order-{orderId}` stream AND the woven claim lifecycle on `Reclamation-{reclamationId}`
                // streams (§2.5, #155) — same checkpoint, so all three categories fold in global
                // `position` order. Both cross-stream feeds key the row from the payload's `orderId`
                // (every Reclamation* event carries it, sourced from the aggregate's fold state); the
                // Conversation-% events key from the stream uuid.
                let uuid = if env.stream_name.starts_with("Conversation-") {
                    aggregate_uuid_of(env, "Conversation-", "orderId")?
                } else {
                    match payload_uuid_of(env, "orderId") {
                        Some(uuid) => uuid,
                        None => {
                            tracing::warn!(
                                projection = "OrderConversation",
                                stream = %env.stream_name,
                                position = env.position,
                                "no orderId in payload -- event skipped (not poison)"
                            );
                            return Ok(());
                        }
                    }
                };
                let id = OrderId(uuid);
                let state = order_conversation_store::load(&mut *conn, id).await?;
                if let Some(next) = project_order_conversation(&OrderConversationProjector, state, env) {
                    order_conversation_store::upsert(&mut *conn, &next).await?;
                }
            }
            Self::SlugAlias => {
                // Only a rename produces an alias. Every other Restaurant-stream event falls through
                // the generated dispatch's `_ => state` arm, so there is nothing to key or load.
                let previous_slug = match &env.event {
                    DomainEvent::RestaurantSlugReconfigured(e) => e.previous_slug.clone(),
                    _ => return Ok(()),
                };
                let state = slug_alias_store::load(&mut *conn, previous_slug).await?;
                if let Some(next) = project_slug_alias(&SlugAliasProjector, state, env) {
                    slug_alias_store::upsert(&mut *conn, &next).await?;
                }
            }
            Self::CustomerCreditBalance => {
                // Single-stream: the ledger lives on `CustomerCredit-{customerId}`; both fed events
                // carry customerId, so the row key resolves from the stream uuid (payload fallback).
                let id = CustomerId(aggregate_uuid_of(env, "CustomerCredit-", "customerId")?);
                let state = customer_credit_balance_store::load(&mut *conn, id).await?;
                if let Some(next) =
                    project_customer_credit_balance(&CustomerCreditBalanceProjector, state, env)
                {
                    customer_credit_balance_store::upsert(&mut *conn, &next).await?;
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
    // The OrderConversation read model (#131, epic #129) folds the conversation's own messaging events
    // (`Conversation-{orderId}` streams), the order status lifecycle (`Order-%`), AND the claim lifecycle
    // woven into the thread (`Reclamation-%`, §2.5, #155), so the group slices all three categories under
    // one checkpoint — keeping the message timeline, folded status and claim entries ordered by global
    // `position`. The row is keyed by order_id (the Conversation aggregate id; the Order and Reclamation
    // events' payload `orderId`).
    ProjectorGroup {
        checkpoint: "OrderConversation",
        stream_prefixes: &["Conversation-", "Order-", "Reclamation-"],
        projectors: &[ReadModelProjector::OrderConversation],
    },
    // The SlugAlias read model (ADR-20260728-011344): superseded storefront labels, so a renamed
    // restaurant's old host keeps resolving. Its own checkpoint on the Restaurant category, because the
    // row key is the payload's `previousSlug` -- NOT the aggregate id -- so it cannot share the
    // Restaurant group's per-row resolution.
    ProjectorGroup {
        checkpoint: "SlugAlias",
        stream_prefixes: &["Restaurant-"],
        projectors: &[ReadModelProjector::SlugAlias],
    },
    // The CustomerCreditBalance read model (#158, Part B of #207): the per-customer store-credit
    // balance, folded from the ledger stream `CustomerCredit-{customerId}` (CustomerCreditGranted /
    // CustomerCreditConsumed). Single-stream, keyed by the customer uuid.
    ProjectorGroup {
        checkpoint: "CustomerCreditBalance",
        stream_prefixes: &["CustomerCredit-"],
        projectors: &[ReadModelProjector::CustomerCreditBalance],
    },
];

pub struct ProjectionWorker {
    pool: PgPool,
    status: Arc<Mutex<ProjectionStatus>>,
    /// Events per batch transaction — `PROJECTION_BATCH_SIZE` (declared, specs/configuration.yaml).
    batch_size: i64,
    /// Idle gate: `MAX(position)` as observed at the end of the last pass that drained EVERY group.
    /// `-1` means nothing observed yet, so the first tick after start always drains.
    last_head: Arc<AtomicI64>,
}

impl ProjectionWorker {
    pub fn new(pool: PgPool) -> Self {
        let batch_size = std::env::var("PROJECTION_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_BATCH_SIZE);
        Self {
            pool,
            status: Arc::new(Mutex::new(ProjectionStatus::default())),
            batch_size,
            last_head: Arc::new(AtomicI64::new(-1)),
        }
    }

    /// Test/tuning override of the per-transaction batch bound.
    pub fn with_batch_size(mut self, batch_size: i64) -> Self {
        self.batch_size = batch_size.max(1);
        self
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

    /// Poll forever with no push assistance: `run_once` then sleep [`POLL_INTERVAL`].
    pub async fn run_loop(self) {
        self.run_loop_with(None).await
    }

    /// Drain forever, woken by `wake` when the log moves and by the safety net otherwise.
    ///
    /// Consumes the worker (spawn it as a task); the shared [`ProjectionStatus`] handle stays
    /// readable through [`Self::status`] clones taken before spawning. Each tick runs in its own
    /// task so a PANIC escaping a drain (poison event) kills only that tick, never the loop — the
    /// production alternative was a projector frozen until the next deploy.
    ///
    /// `None` keeps the unassisted 1.5 s cadence. With a waiter the loop parks on the wake signal
    /// instead, and the interval it falls back to tracks whether the listener is actually up
    /// ([`EventWaiter::safety_interval`]) — so a dropped listener restores fast polling rather than
    /// leaving read models a minute stale.
    pub async fn run_loop_with(self, mut wake: Option<EventWaiter>) {
        self.status_mut().running = true;
        let worker = Arc::new(self);
        loop {
            // Errors are recorded on the status snapshot by run_once; the loop keeps going.
            let w = Arc::clone(&worker);
            if let Err(join) = tokio::spawn(async move { let _ = w.run_once().await; }).await {
                tracing::error!(worker = "projection", error = %join, "tick panicked -- resuming next tick");
            }
            match wake.as_mut() {
                Some(waiter) => {
                    let timeout = waiter.safety_interval(PUSH_SAFETY_INTERVAL, POLL_INTERVAL);
                    waiter.wait(timeout).await;
                }
                None => tokio::time::sleep(POLL_INTERVAL).await,
            }
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
        // Idle gate: the log has not moved since the last pass that drained every group, so no group
        // can have anything pending and the per-group queries would all come back empty. Skipping
        // them turns an idle tick from `1 + 2 x REGISTRY.len()` queries into 1 — the difference
        // between ~41k and ~2.4k queries an hour at the unassisted cadence, and the reason a
        // completely idle platform was the single largest consumer of outbound bandwidth.
        if self.last_head.load(Ordering::Relaxed) == head {
            return Ok((head, head));
        }
        for group in REGISTRY {
            self.drain_group(group).await?;
        }
        // Only a pass that drained EVERY group may arm the gate — a group that errored returns above
        // with the gate untouched, so it is retried on the next tick rather than skipped away.
        self.last_head.store(head, Ordering::Relaxed);
        Ok((head, head))
    }

    /// Drain one group's pending slice in BATCH TRANSACTIONS (PROP-20260730-230803): each batch
    /// (`batch_size`-bounded scan, global `position` order) folds in one `BEGIN … COMMIT` carrying
    /// every upsert AND the checkpoint advance — the batch lands whole or not at all, so a crash
    /// mid-batch replays the whole batch (idempotent folds) instead of leaving rows ahead of the
    /// checkpoint. Loads run on the same transaction, so within a batch the fold reads its own
    /// uncommitted writes (the unit-of-work property; the generated identity map — load each row
    /// ONCE per batch — is the #267 follow-up, an optimization not a correctness change).
    async fn drain_group(&self, group: &ProjectorGroup) -> Result<(), DomainError> {
        let mut checkpoint: i64 =
            sqlx::query_scalar("SELECT position FROM projection_checkpoint WHERE projector = $1")
                .bind(group.checkpoint)
                .fetch_optional(&self.pool)
                .await
                .map_err(db_err)?
                .unwrap_or(0);

        let patterns: Vec<String> =
            group.stream_prefixes.iter().map(|prefix| format!("{prefix}%")).collect();
        loop {
            let pending = sqlx::query(
                "SELECT position, stream_name, event_type, payload, occurred_at FROM domain_events \
                 WHERE position > $1 AND stream_name LIKE ANY($2) ORDER BY position LIMIT $3",
            )
            .bind(checkpoint)
            .bind(&patterns)
            .bind(self.batch_size)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
            if pending.is_empty() {
                return Ok(());
            }
            let batch_len = pending.len() as i64;

            let mut tx = self.pool.begin().await.map_err(db_err)?;
            let mut last_position = checkpoint;
            for record in &pending {
                let position: i64 = record.try_get("position").map_err(db_err)?;
                // A per-event failure (unparseable payload, fold/upsert error, PANIC) is LOGGED and
                // SKIPPED rather than wedging the whole group: with the old `?` a single poison event
                // re-failed every tick and halted ALL further projection. The batch (and with it the
                // checkpoint) still advances; the event stays in domain_events for a future full
                // reprojection. A failure on the batch commit itself DOES propagate — that's a
                // transient DB error worth retrying next tick, not a poison record.
                // Each event folds inside a SAVEPOINT (a nested sqlx transaction): a SQL-level
                // failure (constraint violation, cast error) aborts ONLY the savepoint, not the
                // batch — without it, PostgreSQL poisons the whole batch transaction on the first
                // failed statement and every later event (and the checkpoint) would fail with
                // "current transaction is aborted", turning one poison record into a wedge again.
                let applied = match sqlx::Acquire::begin(&mut *tx).await {
                    Ok(mut sp) => match self.apply_record(&mut sp, group, record).await {
                        Ok(()) => sp.commit().await.map_err(db_err),
                        Err(e) => {
                            let _ = sp.rollback().await;
                            Err(e)
                        }
                    },
                    Err(e) => Err(db_err(e)),
                };
                if let Err(e) = applied {
                    let event_type: String = record.try_get("event_type").unwrap_or_default();
                    // A skipped event means the read model is now permanently behind the log for this
                    // record until a full reprojection. That is a deliberate liveness choice, not a
                    // non-event -- so it is an ERROR, and it names the position needed to replay it.
                    tracing::error!(
                        projection_group = group.checkpoint,
                        position,
                        event_type = %event_type,
                        error = %e,
                        "event skipped -- read model is behind the log at this position until reprojection"
                    );
                }
                last_position = position;
            }
            // The checkpoint advance rides the SAME transaction as the batch's upserts — the
            // unit-of-work boundary.
            sqlx::query(
                "INSERT INTO projection_checkpoint (projector, position, updated_at) VALUES ($1, $2, now()) \
                 ON CONFLICT (projector) DO UPDATE SET position = EXCLUDED.position, updated_at = now()",
            )
            .bind(group.checkpoint)
            .bind(last_position)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
            tx.commit().await.map_err(db_err)?;
            checkpoint = last_position;
            if batch_len < self.batch_size {
                return Ok(());
            }
        }
    }

    /// Fold one `domain_events` row into every read model the group feeds, ON the batch
    /// transaction. Returns a per-event error so the caller can log-and-skip a poison record
    /// without halting the group.
    async fn apply_record(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        group: &ProjectorGroup,
        record: &sqlx::postgres::PgRow,
    ) -> Result<(), DomainError> {
        let position: i64 = record.try_get("position").map_err(db_err)?;
        let stream_name: String = record.try_get("stream_name").map_err(db_err)?;
        let event_type: String = record.try_get("event_type").map_err(db_err)?;
        let payload: serde_json::Value = record.try_get("payload").map_err(db_err)?;
        let occurred_at: chrono::DateTime<Utc> = record.try_get("occurred_at").map_err(db_err)?;

        // `$`-prefixed rows are TECHNICAL, envelope-level events (the deletion engine's
        // `$StreamTombstoned`, ADR-20260731-160000 §5) — not events.yaml vocabulary, nothing to
        // fold. Skipping them here keeps them from masquerading as poison records (the
        // log-and-skip path below is an ERROR, and a tombstone is not one).
        if event_type.starts_with('$') {
            return Ok(());
        }

        // Rebuild the typed event from the (event_type, payload) columns via the adjacent tag.
        let event: DomainEvent = serde_json::from_value(serde_json::json!({
            "eventType": event_type,
            "payload": payload,
        }))
        .map_err(|e| db_err(format!("position {position} ({event_type}): {e}")))?;

        let env = Envelope { stream_name, position, occurred_at, event };
        // catch_unwind (not tokio::spawn — a spawned task cannot borrow the batch transaction) so
        // a PANIC inside a fold degrades to a per-event error the caller log-skips: an unwinding
        // panic would otherwise kill the tick and freeze projection at this position forever (the
        // production refold wedge: a legacy payload hitting a panicking accessor on every boot).
        // AssertUnwindSafe is sound here: on a caught panic the whole batch transaction is either
        // continued (this event skipped) or rolled back — no state written by the panicking fold
        // survives outside the transaction.
        use futures::FutureExt as _;
        let conn: &mut sqlx::PgConnection = &mut *tx;
        match std::panic::AssertUnwindSafe(async move {
            for projector in group.projectors {
                projector.apply(conn, &env).await?;
            }
            Ok::<(), DomainError>(())
        })
        .catch_unwind()
        .await
        {
            Ok(result) => result,
            Err(_panic) => Err(DomainError::Repository(format!(
                "projector panicked at position {position}"
            ))),
        }
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
