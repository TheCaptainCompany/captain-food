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
    project_restaurant, project_rider, project_rider_restriction, project_rider_roster,
    project_slug_alias, Envelope,
};
use application::projectors::cart::CartProjector;
use application::projectors::catalog::CatalogProjector;
use application::projectors::customer::CustomerProjector;
use application::projectors::customer_credit_balance::CustomerCreditBalanceProjector;
use application::projectors::order_conversation::OrderConversationProjector;
use application::projectors::order_tracking::OrderTrackingProjector;
use application::projectors::prospection_pipeline::ProspectionPipelineProjector;
use application::projectors::restaurant::RestaurantProjector;
use application::projectors::rider::RiderProjector;
use application::projectors::rider_restriction::RiderRestrictionProjector;
use application::projectors::rider_roster::RiderRosterProjector;
use application::projectors::slug_alias::SlugAliasProjector;
use chrono::Utc;
use domain::generated::events::DomainEvent;
use domain::generated::scalars::{CartId, CatalogId, CustomerId, OrderId, RestaurantId, RiderId};
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};
use tracing::Instrument as _;

use crate::persistence::event_wake::EventWaiter;
use application::projectors::scope_membership;
use crate::persistence::enum_sql::EnumText as _;
use crate::persistence::{
    cart_store, catalog_store, customer_credit_balance_store, customer_store, db_err,
    order_conversation_store, order_tracking_store, prospection_store, restaurant_store,
    rider_restriction_store, rider_roster_store, rider_store, scope_membership_store,
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

/// Why ONE fold failed, classified at the site that produced it (#474 test 1).
///
/// The drain loop must treat two failures differently and the old code could not tell them apart:
/// `apply_record` returned `DomainError`, and EVERY path built `DomainError::Repository` — a
/// malformed payload, a panicking accessor and a `NOT NULL` violation were the same value. So
/// log-and-skip applied to all three, and a schema/writer disagreement that fails EVERY event of a
/// type (the [#451] shape) advanced the checkpoint past all of them while reporting a healthy drain.
///
/// This is an enum rather than a predicate over the error string on purpose (CLAUDE.md
/// "compiler first"): the classification is a fact known where the error is CONSTRUCTED, so the
/// compiler makes every new failure site declare which one it is, and no later refactor can
/// silently reclassify by rewording a message.
///
/// There is deliberately **no `From<DomainError>`**. A blanket conversion would let `?` choose a
/// class on the author's behalf, which is the whole property this type is for — and is exactly how
/// the first cut of it went wrong: `apply_record` wrapped every fold error in one
/// `map_err(FoldFault::Database)`, so `aggregate_uuid_of`'s per-record resolution failure was
/// reported as a schema fault and would have wedged its group under [`DbFaultPolicy::Halt`]. Adding
/// a `From` impl re-opens that hole for every future call site at once.
#[derive(Debug)]
enum FoldFault {
    /// This RECORD is bad — an unparseable payload, or a fold that panicked reading a legacy shape.
    /// Genuinely per-event: the next event is unaffected, so skipping keeps the group live and the
    /// checkpoint legitimately advances past it. This is the case [#230]'s log-and-skip was built
    /// for and its behaviour is unchanged.
    PayloadShape(DomainError),
    /// The DATABASE rejected the write — constraint violation, cast error, missing column. Almost
    /// never about this one record: it is the schema disagreeing with the writer, so the next event
    /// of the same type fails identically and skipping silently destroys the read model.
    Database(DomainError),
}

impl FoldFault {
    fn error(self) -> DomainError {
        match self {
            Self::PayloadShape(e) | Self::Database(e) => e,
        }
    }
}

/// What the drain loop does when a fold fails with [`FoldFault::Database`].
///
/// A GATE, not a fix (CLAUDE.md gate-then-stabilize): [`Self::Skip`] is the default and reproduces
/// today's behaviour exactly, so this change lands inert on every deployed path. [`Self::Halt`] is
/// finished, tested behaviour that is not yet the default — flipping it is a separate, recorded
/// decision, because halting has a real cost of its own (one genuinely-poisoned row wedges the
/// group until an operator intervenes, which is the wedge [#230] removed on purpose).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DbFaultPolicy {
    /// Log the failure and advance past it — the read model is permanently behind for that record
    /// until a reprojection. Today's behaviour, unchanged.
    #[default]
    Skip,
    /// Roll the batch back and return the error, leaving the checkpoint EXACTLY where it was. The
    /// group retries the same slice next tick and the failure stays visible instead of scrolling
    /// past. Nothing is lost: the events are still in `domain_events` behind an unmoved checkpoint.
    Halt,
}

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
    /// The rider identity read model (#639 part A): the `auth_ref -> rider_id` bridge
    /// ADR-20260818-004646 puts in our Postgres rather than in the provider's claims. `standing`
    /// (#639 part C step 4-i) is the one computed column — `RiderCompute::standing`.
    Rider,
    /// The restriction attribution read model (#639 part C step 4-i, ADR-20260904-081527 §2):
    /// ground/decidedAt/effectiveAt/reinstatedAt, the source of `myStanding`. Same stream (`Rider-`)
    /// as [`Self::Rider`], same checkpoint — the Restaurant/ProspectionPipeline precedent.
    RiderRestriction,
    /// The admin roster read model (#639 part C step 4-iii-A, ADR-20260904-152807 §1): name,
    /// phone, availability and the platform grant for EVERY rider -- `riders`/`rider`'s source.
    /// Its OWN checkpoint group (see the registry below), never a prefix under `Rider` or
    /// `RiderRestriction`.
    RiderRoster,
    /// Keyed by the SUPERSEDED slug from the event payload, not by an aggregate id — one row per
    /// rename, so a restaurant renamed N times leaves N rows on the same stream.
    SlugAlias,
    /// SET-shaped (#144): one event GRANTS/REVOKES N membership rows, so there is no
    /// `load -> project -> upsert` triple and no generated dispatch (`emit_projectors` skips a
    /// table whose pk lineage carries no property path). The pure fold lives in
    /// `application::projectors::scope_membership`; this arm resolves its two lookups and applies
    /// the changes on the batch transaction.
    ScopeMembership,
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
    async fn apply(&self, conn: &mut sqlx::PgConnection, env: &Envelope) -> Result<(), FoldFault> {
        let span = telemetry::spans::event_consume_projection(&self.projection_name());
        self.apply_inner(conn, env).instrument(span).await
    }

    /// Runs on the BATCH TRANSACTION's connection: loads see the batch's own uncommitted upserts
    /// (read-your-writes within a batch — event N+1 for a row folds over event N's result), and
    /// every upsert commits (or rolls back) with the batch checkpoint.
    ///
    /// Returns [`FoldFault`], not `DomainError`, so the CLASSIFICATION is made where the failure
    /// happens. There is deliberately no `From<DomainError> for FoldFault`: `?` therefore cannot
    /// pick a class on the author's behalf, and every fallible step here has to say which one it
    /// is — a store call is `Database`, a row key that will not resolve is `PayloadShape`. That is
    /// the property the enum's own doc claims ("the compiler makes every new failure site declare
    /// which one it is"), and until this signature changed it was not true: the caller wrapped the
    /// WHOLE of this function in one `map_err(FoldFault::Database)`, so `aggregate_uuid_of`'s
    /// per-record resolution failure was reported as a database rejection and, under
    /// [`DbFaultPolicy::Halt`], one unparseable stream name would have wedged its group forever —
    /// the [#230] wedge, in the code added to avoid it.
    async fn apply_inner(&self, conn: &mut sqlx::PgConnection, env: &Envelope) -> Result<(), FoldFault> {
        match self {
            Self::Restaurant => {
                let id = RestaurantId(aggregate_uuid_of(env, "Restaurant-", "restaurantId")?);
                let state = restaurant_store::load(&mut *conn, id).await.map_err(FoldFault::Database)?;
                if let Some(next) = project_restaurant(&RestaurantProjector, state, env) {
                    restaurant_store::upsert(&mut *conn, &next).await.map_err(FoldFault::Database)?;
                }
            }
            Self::ProspectionPipeline => {
                let id = RestaurantId(aggregate_uuid_of(env, "Restaurant-", "restaurantId")?);
                let state = prospection_store::load(&mut *conn, id).await.map_err(FoldFault::Database)?;
                if let Some(next) = project_prospection_pipeline(&ProspectionPipelineProjector, state, env)
                {
                    prospection_store::upsert(&mut *conn, &next).await.map_err(FoldFault::Database)?;
                }
            }
            Self::Customer => {
                let id = CustomerId(aggregate_uuid_of(env, "Customer-", "customerId")?);
                let state = customer_store::load(&mut *conn, id).await.map_err(FoldFault::Database)?;
                if let Some(next) = project_customer(&CustomerProjector, state, env) {
                    customer_store::upsert(&mut *conn, &next).await.map_err(FoldFault::Database)?;
                }
            }
            Self::Rider => {
                let id = RiderId(aggregate_uuid_of(env, "Rider-", "riderId")?);
                let state = rider_store::load(&mut *conn, id).await.map_err(FoldFault::Database)?;
                if let Some(next) = project_rider(&RiderProjector, state, env) {
                    rider_store::upsert(&mut *conn, &next).await.map_err(FoldFault::Database)?;
                }
            }
            Self::RiderRestriction => {
                let id = RiderId(aggregate_uuid_of(env, "Rider-", "riderId")?);
                let state =
                    rider_restriction_store::load(&mut *conn, id).await.map_err(FoldFault::Database)?;
                if let Some(next) = project_rider_restriction(&RiderRestrictionProjector, state, env) {
                    rider_restriction_store::upsert(&mut *conn, &next).await.map_err(FoldFault::Database)?;
                }
            }
            Self::RiderRoster => {
                let id = RiderId(aggregate_uuid_of(env, "Rider-", "riderId")?);
                let state = rider_roster_store::load(&mut *conn, id).await.map_err(FoldFault::Database)?;
                if let Some(next) = project_rider_roster(&RiderRosterProjector, state, env) {
                    rider_roster_store::upsert(&mut *conn, &next).await.map_err(FoldFault::Database)?;
                }
            }
            Self::Catalog => {
                let id = CatalogId(aggregate_uuid_of(env, "Catalog-", "catalogId")?);
                let state = catalog_store::load(&mut *conn, id).await.map_err(FoldFault::Database)?;
                if let Some(next) = project_catalog(&CatalogProjector, state, env) {
                    catalog_store::upsert(&mut *conn, &next).await.map_err(FoldFault::Database)?;
                }
            }
            Self::Cart => {
                let id = CartId(aggregate_uuid_of(env, "Cart-", "cartId")?);
                let state = cart_store::load(&mut *conn, id).await.map_err(FoldFault::Database)?;
                if let Some(next) = project_cart(&CartProjector, state, env) {
                    cart_store::upsert(&mut *conn, &next).await.map_err(FoldFault::Database)?;
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
                            // DEBUG, not warn: since the DeliveryJob-% slice joined this group, an
                            // unkeyable event is the ORDINARY case (every delivery fact except the
                            // three rider ones carries no `orderId`), so a warn would be one line
                            // per such event per drain and would stop meaning "anomaly".
                            tracing::debug!(
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
                let state = order_tracking_store::load(&mut *conn, id).await.map_err(FoldFault::Database)?;
                if let Some(next) = project_order_tracking(&OrderTrackingProjector, state, env) {
                    order_tracking_store::upsert(&mut *conn, &next).await.map_err(FoldFault::Database)?;
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
                let state = order_conversation_store::load(&mut *conn, id).await.map_err(FoldFault::Database)?;
                if let Some(next) = project_order_conversation(&OrderConversationProjector, state, env) {
                    order_conversation_store::upsert(&mut *conn, &next).await.map_err(FoldFault::Database)?;
                }
            }
            Self::SlugAlias => {
                // Only a rename produces an alias. Every other Restaurant-stream event falls through
                // the generated dispatch's `_ => state` arm, so there is nothing to key or load.
                let previous_slug = match &env.event {
                    DomainEvent::RestaurantSlugReconfigured(e) => e.previous_slug.clone(),
                    _ => return Ok(()),
                };
                let state = slug_alias_store::load(&mut *conn, previous_slug).await.map_err(FoldFault::Database)?;
                if let Some(next) = project_slug_alias(&SlugAliasProjector, state, env) {
                    slug_alias_store::upsert(&mut *conn, &next).await.map_err(FoldFault::Database)?;
                }
            }
            Self::ScopeMembership => {
                // Two lookups the PURE fold cannot do are resolved here and passed in:
                //   * DeliveryCancelled carries only a deliveryJobId -> which order is it?
                //     (DeliveryAcceptedByRider / DeliveryDispatchFailed carry `orderId` since
                //     D-QW1 option b, so only the cancel needs the View_DeliveryJob lookup — the
                //     view folds straight off `domain_events`, so it is always consistent with the
                //     log and the job's birth precedes its cancel in position order.)
                //   * OrderPlaced carries no restaurantAccountId -> resolved from THIS table's own
                //     RESTAURANT-scope grant (written by RestaurantRegistered, which precedes any
                //     OrderPlaced for that restaurant in position order and is read-your-writes on
                //     the batch transaction). Reading a sibling projection (the restaurant table)
                //     instead would make a full rebuild non-deterministic: that table belongs to an
                //     INDEPENDENT checkpoint group, which may not have caught up during a DR replay.
                // An unresolved lookup yields NO change (the pure layer's choice) — see the fold's
                // orphan-cancel note for why that is allow-stale-but-acceptable, not deny-safe.
                let resolved = scope_membership::Resolved {
                    order_id: resolve_cancelled_delivery_order(&mut *conn, env).await.map_err(FoldFault::Database)?,
                    restaurant_account_id: resolve_restaurant_account(&mut *conn, env).await.map_err(FoldFault::Database)?,
                };
                let changes = scope_membership::membership_changes(env, &resolved);
                // All of one event's changes ride the SAME transaction (the per-event savepoint):
                // a reassignment is a revoke followed by a grant; applying them separately would
                // leave a window in which the order has no rider — or two.
                for change in &changes {
                    match change {
                        scope_membership::MembershipChange::Grant {
                            scope_type,
                            scope_id,
                            member_type,
                            member_id,
                        } => {
                            scope_membership_store::grant(
                                &mut *conn,
                                *scope_type,
                                *scope_id,
                                *member_type,
                                *member_id,
                                env.occurred_at,
                            )
                            .await.map_err(FoldFault::Database)?;
                        }
                        scope_membership::MembershipChange::RevokeRole {
                            scope_type,
                            scope_id,
                            member_type,
                        } => {
                            // A failed REVOKE is not an ordinary skipped fold: the drain loop's
                            // log-and-skip advances the checkpoint past it, and on THIS table a
                            // missed revoke is a STANDING STALE GRANT — the silent-breach failure
                            // mode, not stale data. The generic skip error stays (liveness), but
                            // it gets a dedicated, searchable line naming the consequence and the
                            // repair (delete the ScopeMembership checkpoint row → full replay).
                            // Accepted risk + procedure: ADR-20260809-160000 addendum.
                            if let Err(e) = scope_membership_store::revoke_role(
                                &mut *conn,
                                *scope_type,
                                *scope_id,
                                *member_type,
                            )
                            .await
                            {
                                tracing::error!(
                                    scope_type = ?scope_type,
                                    scope_id = %scope_id,
                                    member_type = ?member_type,
                                    error = %e,
                                    "SCOPE-MEMBERSHIP REVOKE FAILED -- if the batch skips this event a STALE GRANT STANDS until a ScopeMembership checkpoint-reset replay"
                                );
                                return Err(FoldFault::Database(e));
                            }
                        }
                    }
                }
            }
            Self::CustomerCreditBalance => {
                // Single-stream: the ledger lives on `CustomerCredit-{customerId}`; both fed events
                // carry customerId, so the row key resolves from the stream uuid (payload fallback).
                let id = CustomerId(aggregate_uuid_of(env, "CustomerCredit-", "customerId")?);
                let state = customer_credit_balance_store::load(&mut *conn, id).await.map_err(FoldFault::Database)?;
                if let Some(next) =
                    project_customer_credit_balance(&CustomerCreditBalanceProjector, state, env)
                {
                    customer_credit_balance_store::upsert(&mut *conn, &next).await.map_err(FoldFault::Database)?;
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
    /// The OWNING scope (ADR-20260807-183024 D4): the spec scope of the read models this group
    /// maintains — which `projector-{scope}` bin drains it once the per-scope projectors host
    /// projection (#385). Derived by hand from the primary stream category's owning aggregate;
    /// the `registry_scopes_match_the_spec_placement` test below ties each label to the
    /// GENERATED `ACTOR_SCOPES` table so it cannot drift from `specs/{scope}/` placement.
    scope: &'static str,
}

/// The projector registry: one group per aggregate stream feeding materialized read models. The
/// Restaurant group keeps its historical `'Restaurant'` checkpoint covering both of its folds.
const REGISTRY: &[ProjectorGroup] = &[
    ProjectorGroup {
        checkpoint: "Restaurant",
        stream_prefixes: &["Restaurant-"],
        projectors: &[ReadModelProjector::Restaurant, ReadModelProjector::ProspectionPipeline],
        scope: "network",
    },
    ProjectorGroup {
        checkpoint: "Customer",
        stream_prefixes: &["Customer-"],
        projectors: &[ReadModelProjector::Customer],
        scope: "customer",
    },
    // The rider identity read model (#639 part A, ADR-20260818-004646). Its OWN checkpoint and its
    // OWN group, never a prefix bolted onto an existing one: a prefix joined below an already
    // advanced checkpoint is never folded (the #424 lesson, stated again on the ScopeMembership
    // group below). Backfill is free for the same reason it is there — a group with no
    // `projection_checkpoint` row starts at position 0 — and here that costs nothing, because no
    // `RiderRegistered` has ever been appended.
    //
    // `DeliveryAcceptedByRider` lands on `DeliveryJob-%` and is therefore INVISIBLE to this group.
    // Correct today: this table holds identity and availability, not job assignment. The day a
    // "current job" column is wanted, adding that prefix requires DELETING the Rider checkpoint row
    // in the same migration.
    ProjectorGroup {
        checkpoint: "Rider",
        stream_prefixes: &["Rider-"],
        projectors: &[ReadModelProjector::Rider],
        scope: "delivery",
    },
    // Round 2 item 7 (dba, young — a migration-class defect, #639 part C step 4-i): `RiderRestriction`
    // is born by THIS migration, weeks after the `Rider` checkpoint above first advanced — folding
    // it under that already-advanced checkpoint means a rider registered before this migration NEVER
    // gets a row (the checkpoint never rewinds to replay their `RiderRegistered`), so a LATER
    // `RiderRestricted` on that same stream is silently dropped by the projector's own
    // `let mut row = state?;` short-circuit: the door closes, `myStanding` shows `restriction: null`,
    // the notice has no ground. Its OWN checkpoint, its OWN group — the #424 lesson the comment above
    // already states for exactly this shape — starting at 0 (a group with no `projection_checkpoint`
    // row starts there for free): the whole `Rider-` log replays through THIS projector alone, so a
    // table born after the log is backfilled by replay rather than a migration-time copy. The lag
    // gauge stays on the `"Rider"` checkpoint (unchanged — `RiderRestriction`'s own replay lag is not
    // this contract's subject).
    ProjectorGroup {
        checkpoint: "RiderRestriction",
        stream_prefixes: &["Rider-"],
        projectors: &[ReadModelProjector::RiderRestriction],
        scope: "delivery",
    },
    // The admin roster (#639 part C step 4-iii-A, ADR-20260904-152807 §1/§3): its OWN checkpoint,
    // its OWN group, starting at 0 — the SAME #424 lesson `RiderRestriction`'s own comment states,
    // generalised a second time: a table born after the log is backfilled by replay, never a
    // migration-time copy. Rebuild by resetting the `RiderRoster` checkpoint (this group's own
    // rebuild discipline is TRUNCATE-together, unlike `Rider`/`RiderRestriction` — see the table's
    // own `rules:` in projection_tables.yaml for why the mechanical `derive:` on `standing` is
    // safe here and would not be on those two).
    ProjectorGroup {
        checkpoint: "RiderRoster",
        stream_prefixes: &["Rider-"],
        projectors: &[ReadModelProjector::RiderRoster],
        scope: "delivery",
    },
    ProjectorGroup {
        checkpoint: "Catalog",
        stream_prefixes: &["Catalog-"],
        projectors: &[ReadModelProjector::Catalog],
        scope: "catalog",
    },
    ProjectorGroup {
        checkpoint: "Cart",
        stream_prefixes: &["Cart-"],
        projectors: &[ReadModelProjector::Cart],
        scope: "ordering",
    },
    // The Payment-% slice closes the OrderTracking.payment_status feed gap (docs/sagas.md;
    // ADR-20260719-193500): PaymentCaptured/PaymentRefunded live on Payment-{intentId} streams
    // but are declared in the ordertracking fedBy. Same 'Order' checkpoint = one ordered fold.
    // The DeliveryJob-% slice does the same for the delivery mirror (delivery_status/courier/
    // estimated_dropoff_at, spec'd but unfed until now -- docs/sagas.md): the WHOLE DeliveryJob-%
    // family under this checkpoint, so order, payment and delivery facts fold in global `position`
    // order. Keying needs no code for the RIDER path: the three rider facts carry `orderId` in
    // their payload (D-QW1 option b, ADR-20260808-234907), which the branch above already reads.
    // The PARTNER path is NOT fed by this: `DeliveryAcceptedByPartner` is in the fedBy (it feeds
    // courier/estimated_dropoff_at) yet carries no `orderId`, so it keys to nothing and those two
    // columns stay unfed on a partner delivery -- slice 8 (#420) owns closing that. Consequently
    // the unkeyed branch is a DEBUG, not a warn: it is the ordinary case for every DeliveryJob-%
    // fact outside the three rider ones, not an anomaly worth a line per event per drain.
    ProjectorGroup {
        checkpoint: "Order",
        stream_prefixes: &["Order-", "Payment-", "DeliveryJob-"],
        projectors: &[ReadModelProjector::OrderTracking],
        scope: "ordering",
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
        scope: "comms",
    },
    // The SlugAlias read model (ADR-20260728-011344): superseded storefront labels, so a renamed
    // restaurant's old host keeps resolving. Its own checkpoint on the Restaurant category, because the
    // row key is the payload's `previousSlug` -- NOT the aggregate id -- so it cannot share the
    // Restaurant group's per-row resolution.
    ProjectorGroup {
        checkpoint: "SlugAlias",
        stream_prefixes: &["Restaurant-"],
        projectors: &[ReadModelProjector::SlugAlias],
        scope: "network",
    },
    // The CustomerCreditBalance read model (#158, Part B of #207): the per-customer store-credit
    // balance, folded from the ledger stream `CustomerCredit-{customerId}` (CustomerCreditGranted /
    // CustomerCreditConsumed). Single-stream, keyed by the customer uuid.
    ProjectorGroup {
        checkpoint: "CustomerCreditBalance",
        stream_prefixes: &["CustomerCredit-"],
        projectors: &[ReadModelProjector::CustomerCreditBalance],
        scope: "payments",
    },
    // The ScopeMembership ACL index (#144) spans THREE categories under ONE checkpoint, and the
    // single checkpoint is the load-bearing part — not a convenience. Grants arrive on `Order-%`
    // and `Restaurant-%`; the rider revoke/grant pair arrives on `DeliveryJob-%`. With independent
    // checkpoints those categories could interleave out of global `position` order, folding a
    // revoke BEFORE the grant it supersedes and leaving a stale grant behind — the one failure mode
    // of an ACL cache that is a silent breach rather than a visible denial. One checkpoint = one
    // totally ordered fold.
    //
    // BACKFILL IS FREE, and deliberately so: a group with no `projection_checkpoint` row starts at
    // position 0 (`drain_group`'s `unwrap_or(0)`), so the first tick replays the whole log and
    // materializes every historical membership — no separate backfill job before enforcement goes
    // live (which matters because enforcement lands immediately, not behind a shadow mode:
    // product-owner decision 2026-07-25). Replay is safe because both operations are idempotent
    // over the same ordered log: `grant` upserts on a key DERIVED from the membership itself, and
    // grant -> revoke -> grant folds to the same end state the live path produced. (Historical
    // OrderPlaced events predating #144 carry no customerId and fail to deserialize — they are
    // log-skipped, so pre-#144 orders simply hold no grants and stay Admin-only. Throwaway smoke
    // data today; recorded so the first such denial is not read as a bug.)
    //
    // `"Order-"` FIRST is load-bearing: `registry_scopes_match_the_spec_placement` derives the
    // owning scope from `stream_prefixes[0]`. And ADDING a prefix to this group later requires
    // deleting the ScopeMembership checkpoint row in the same migration — a prefix joined below an
    // advanced checkpoint is never folded (the #424 lesson).
    ProjectorGroup {
        checkpoint: "ScopeMembership",
        stream_prefixes: &["Order-", "DeliveryJob-", "Restaurant-"],
        projectors: &[ReadModelProjector::ScopeMembership],
        scope: "ordering",
    },
];

/// Which order does this `DeliveryCancelled` job belong to? Only the cancel needs this —
/// `DeliveryAcceptedByRider` and `DeliveryDispatchFailed` carry `orderId` in the payload (D-QW1
/// option b). `View_DeliveryJob` is a projection-on-read over `domain_events` itself, so the answer
/// is always consistent with the log; job -> order is immutable, so there is no race. `None`
/// (an orphan stream with no birth fact) yields no membership change at all.
async fn resolve_cancelled_delivery_order(
    conn: &mut sqlx::PgConnection,
    env: &Envelope,
) -> Result<Option<uuid::Uuid>, DomainError> {
    let job_id = match &env.event {
        DomainEvent::DeliveryCancelled(e) => e.delivery_job_id.0,
        _ => return Ok(None),
    };
    let row: Option<(Option<uuid::Uuid>,)> =
        sqlx::query_as("SELECT order_id FROM view_deliveryjob WHERE delivery_job_id = $1")
            .bind(job_id)
            .fetch_optional(&mut *conn)
            .await
            .map_err(db_err)?;
    Ok(row.and_then(|(id,)| id))
}

/// Which account owns the restaurant on this `OrderPlaced`? Resolved from the ScopeMembership
/// table's OWN RestaurantRegistered/RestaurantListingClaimed grants (scope RESTAURANT, member
/// RESTAURANT_ACCOUNT), which this group folds in the same total order — deterministic under a
/// full rebuild, unlike reading the sibling `restaurant` projection, whose independent checkpoint
/// may lag during a DR replay. An unresolved account simply omits that one grant.
///
/// FIRST ACCOUNT WINS, deterministically (review finding on #430): a restaurant CAN hold two
/// RESTAURANT_ACCOUNT rows — `claim_restaurant_listing` guards only `listing_claimed`, so a
/// partner-onboarded restaurant (account granted at registration) claimed later by a second
/// Google-verified account grants both. An unordered `LIMIT 1` would then pick arbitrarily per
/// order, silently starving one account's back office. `ORDER BY granted_at, membership_id` makes
/// the double-grant degrade to a DEFINED, replay-stable choice (both keys are stored, not
/// wall-clock). Whether the write side should REJECT the second claim outright is a domain
/// invariant question recorded on #432, not improvised here.
async fn resolve_restaurant_account(
    conn: &mut sqlx::PgConnection,
    env: &Envelope,
) -> Result<Option<uuid::Uuid>, DomainError> {
    let restaurant_id = match &env.event {
        DomainEvent::OrderPlaced(e) => e.restaurant_id.0,
        _ => return Ok(None),
    };
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(
        "SELECT member_id FROM scopemembership \
          WHERE scope_type = $1 AND scope_id = $2 AND member_type = $3 \
          ORDER BY granted_at ASC, membership_id ASC LIMIT 1",
    )
    .bind(domain::generated::scalars::ScopeType::RESTAURANT.to_text())
    .bind(restaurant_id)
    .bind(domain::generated::scalars::UserType::RESTAURANT_ACCOUNT.to_text())
    .fetch_optional(&mut *conn)
    .await
    .map_err(db_err)?;
    Ok(row.map(|(id,)| id))
}

pub struct ProjectionWorker {
    pool: PgPool,
    status: Arc<Mutex<ProjectionStatus>>,
    /// Events per batch transaction — `PROJECTION_BATCH_SIZE` (declared, specs/configuration.yaml).
    batch_size: i64,
    /// Idle gate: `MAX(position)` as observed at the end of the last pass that drained EVERY group.
    /// `-1` means nothing observed yet, so the first tick after start always drains.
    last_head: Arc<AtomicI64>,
    /// Registry slice this worker drains: `None` = every group (the monolith's in-process worker);
    /// `Some(scope)` = only the groups whose read models the scope owns — the `projector-{scope}`
    /// bins (#385, ADR-20260807-183024 D4). Per-group checkpoints make the split free: a scoped
    /// worker advances exactly the rows a full worker would for those groups, so monolith and
    /// per-scope deployments can hand over without a re-projection.
    scope: Option<&'static str>,
    /// What a database-rejected fold does (#474). Defaults to [`DbFaultPolicy::Skip`] --
    /// today's behaviour -- so this lands inert; see [`DbFaultPolicy`] for why the flip is a
    /// separate decision.
    db_fault_policy: DbFaultPolicy,
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
            scope: None,
            db_fault_policy: DbFaultPolicy::default(),
        }
    }

    /// Choose what a database-rejected fold does (#474). Left at [`DbFaultPolicy::Skip`] everywhere
    /// in production today; the halting arm is exercised by
    /// `crates/infrastructure/tests/main/projection_checkpoint_halt.rs`.
    pub fn with_db_fault_policy(mut self, policy: DbFaultPolicy) -> Self {
        self.db_fault_policy = policy;
        self
    }

    /// Test/tuning override of the per-transaction batch bound.
    pub fn with_batch_size(mut self, batch_size: i64) -> Self {
        self.batch_size = batch_size.max(1);
        self
    }

    /// Restrict this worker to the registry groups OWNED by `scope` (the `projector-{scope}` bin
    /// posture, D4). A scope owning no group today (e.g. `delivery`, whose read models are
    /// maintained by the saga's state tables rather than log projection) yields an idle worker
    /// that reports itself caught up — deliberately: the bin is honest about having nothing to
    /// fold rather than crashing a deployable the topology declares.
    pub fn with_scope(mut self, scope: &'static str) -> Self {
        self.scope = Some(scope);
        self
    }

    /// The registry groups this worker drains (scope-filtered when set).
    fn groups(&self) -> impl Iterator<Item = &'static ProjectorGroup> {
        let scope = self.scope;
        REGISTRY.iter().filter(move |g| scope.is_none_or(|s| g.scope == s))
    }

    /// How many groups the given scope owns — the bins log this at boot so "idle because empty"
    /// is distinguishable from "idle because caught up" without reading the registry source.
    pub fn scope_group_count(scope: &str) -> usize {
        REGISTRY.iter().filter(|g| g.scope == scope).count()
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
        for group in self.groups() {
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
            // The ACL index's lag is a USER-VISIBLE DENIAL, not stale data (a just-placed order's
            // own customer is denied their order while it lags), so it gets the contract's
            // dedicated gauge (`read-authorization` business_metrics). Emitted per scan: the
            // pending batch length is a lower bound while draining, and an exact 0 when caught up.
            if group.checkpoint == "ScopeMembership" {
                telemetry::meters::read_authorization::lag_positions(pending.len() as i64);
            }
            // `rider-restriction` contract (#639 part C step 4-i, ADR-20260904-081527 §9): the
            // `scope_membership_lag_positions` mirror — while the Rider projector lags, a
            // restricted rider is still GRANTED, so "immediately" is a measured claim only with
            // this gauge. Round 2 item 6(b) (dba, farley): "0 when caught up" was FALSE as
            // written — this loop returns on a short page and the idle gate below never
            // re-records, so an idle platform exports the PRE-DRAIN page length until the log next
            // moves. True shape: the pending page length at each scan — a lower bound capped at
            // `batch_size` while draining, re-recorded only when the log moves, 0 only on the
            // first scan that finds nothing. Pre-existing, shared `drain_group` behaviour (not
            // fixed here — architect-owned issue).
            if group.checkpoint == "Rider" {
                telemetry::meters::rider_restriction::lag(pending.len() as i64);
            }
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
                        Ok(()) => sp.commit().await.map_err(|e| FoldFault::Database(db_err(e))),
                        Err(e) => {
                            let _ = sp.rollback().await;
                            Err(e)
                        }
                    },
                    Err(e) => Err(FoldFault::Database(db_err(e))),
                };
                if let Err(fault) = applied {
                    let event_type: String = record.try_get("event_type").unwrap_or_default();
                    // HALT (#474): a database fault is the schema disagreeing with the writer, so
                    // the next event of this type fails identically. Return WITHOUT committing --
                    // `tx` drops here and rolls back, so the checkpoint stays exactly where it was
                    // and this slice is retried next tick with the failure still in front of us.
                    // Advancing would report progress the read model did not make.
                    if matches!(fault, FoldFault::Database(_)) && self.db_fault_policy == DbFaultPolicy::Halt {
                        let e = fault.error();
                        tracing::error!(
                            projection_group = group.checkpoint,
                            position,
                            event_type = %event_type,
                            error = %e,
                            "PROJECTION HALTED -- database rejected this fold; checkpoint left at {checkpoint} rather than advancing past an event that did not land"
                        );
                        return Err(e);
                    }
                    let e = fault.error();
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
    ) -> Result<(), FoldFault> {
        // Decoding the ENVELOPE columns is a database contract, not a payload shape: if
        // `domain_events.position` stops decoding as `i64` every record fails, so it classifies
        // with the database faults.
        let db = |e: sqlx::Error| FoldFault::Database(db_err(e));
        let position: i64 = record.try_get("position").map_err(db)?;
        let stream_name: String = record.try_get("stream_name").map_err(db)?;
        let event_type: String = record.try_get("event_type").map_err(db)?;
        let payload: serde_json::Value = record.try_get("payload").map_err(db)?;
        let occurred_at: chrono::DateTime<Utc> = record.try_get("occurred_at").map_err(db)?;

        // `$`-prefixed rows are TECHNICAL, envelope-level events (the deletion engine's
        // `$StreamTombstoned`, ADR-20260731-160000 §5) — not events.yaml vocabulary, nothing to
        // fold. Skipping them here keeps them from masquerading as poison records (the
        // log-and-skip path below is an ERROR, and a tombstone is not one).
        if event_type.starts_with('$') {
            return Ok(());
        }

        // Rebuild the typed event from the (event_type, payload) columns via the adjacent tag.
        // The ONE genuinely per-record failure: this row's payload does not fit its event type.
        let event: DomainEvent = serde_json::from_value(serde_json::json!({
            "eventType": event_type,
            "payload": payload,
        }))
        .map_err(|e| {
            FoldFault::PayloadShape(db_err(format!("position {position} ({event_type}): {e}")))
        })?;

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
            Ok::<(), FoldFault>(())
        })
        .catch_unwind()
        .await
        {
            // Already classified at the site that produced it -- `apply` returns `FoldFault`, so
            // there is nothing to guess here. This arm used to be `map_err(FoldFault::Database)`,
            // which asserted that every fold error "came out of a generated store's upsert"; it did
            // not (`aggregate_uuid_of` reached it too), and one blanket conversion silently
            // reclassified a per-record failure as a schema fault.
            Ok(result) => result,
            // A PANIC classifies as PayloadShape, keeping the liveness property #230 added it for:
            // the observed case is a legacy payload hitting a panicking accessor, which is this
            // record being bad, not the schema being wrong. Halting on it would restore exactly the
            // boot-time refold wedge that motivated catch_unwind.
            Err(_panic) => Err(FoldFault::PayloadShape(DomainError::Repository(format!(
                "projector panicked at position {position}"
            )))),
        }
    }

}

/// The aggregate id an event belongs to: parsed from the `<Category>-<uuid>` stream name, falling back
/// to the payload's own id field (every same-stream event carries its aggregate id).
///
/// Failure is [`FoldFault::PayloadShape`], never `Database`: BOTH halves of the resolution are
/// per-record reads of THIS envelope — a stream name that does not end in a uuid and a payload that
/// does not carry the key. No database is involved, the next event is unaffected, and the failure is
/// structurally the same as a payload that will not deserialize into its event type. Classifying it
/// as a database fault (which the caller's blanket `map_err` used to do) would, under
/// [`DbFaultPolicy::Halt`], let one hand-repaired or mis-routed stream name wedge its whole group
/// until an operator intervened — exactly the wedge [#230] removed and `PayloadShape` exists to
/// avoid.
fn aggregate_uuid_of(env: &Envelope, prefix: &str, payload_key: &str) -> Result<uuid::Uuid, FoldFault> {
    if let Some(suffix) = env.stream_name.strip_prefix(prefix) {
        if let Ok(id) = uuid::Uuid::parse_str(suffix) {
            return Ok(id);
        }
    }
    payload_uuid_of(env, payload_key).ok_or_else(|| {
        FoldFault::PayloadShape(DomainError::Repository(format!(
            "cannot resolve {payload_key} for stream {}",
            env.stream_name
        )))
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

    /// The hand-written registry's scope labels are tied to the GENERATED spec-placement table
    /// (`ACTOR_SCOPES`, from specs/{scope}/ folders): each group's scope must be the owning
    /// scope of its PRIMARY stream category's aggregate. A label drifting from a spec move is a
    /// red test here, not a projector silently draining under the wrong `projector-{scope}` bin.
    #[test]
    fn registry_scopes_match_the_spec_placement() {
        for group in REGISTRY {
            let category = group.stream_prefixes[0]
                .strip_suffix('-')
                .expect("stream prefixes are '<Category>-'");
            let owning = crate::generated::scopes::actor_scope(category).unwrap_or_else(|| {
                panic!("registry group '{}': primary category '{category}' is not a spec-declared actor", group.checkpoint)
            });
            assert_eq!(
                group.scope, owning,
                "registry group '{}' labels scope '{}' but specs/{{scope}}/ places its primary aggregate '{}' in '{}'",
                group.checkpoint, group.scope, category, owning
            );
        }
    }

    /// The ScopeMembership group's SHAPE is the ordering guarantee (#144): ONE checkpoint over the
    /// three categories whose interleaving could otherwise fold a revoke before the grant it
    /// supersedes — a stale grant, the silent-breach failure mode of an ACL cache. Deliberately
    /// structure-sensitive: forcing the interleaving dynamically costs more than it proves, and the
    /// structure IS the claim. `"Order-"` first is also load-bearing (the scope-placement test
    /// derives the owning scope from `stream_prefixes[0]`).
    #[test]
    fn scope_membership_folds_three_categories_under_one_checkpoint() {
        let group = REGISTRY
            .iter()
            .find(|g| g.checkpoint == "ScopeMembership")
            .expect("the ScopeMembership ACL index has a registry group");
        assert_eq!(group.stream_prefixes, &["Order-", "DeliveryJob-", "Restaurant-"]);
        assert_eq!(group.projectors.len(), 1);
        assert!(matches!(group.projectors[0], ReadModelProjector::ScopeMembership));
        assert_eq!(group.scope, "ordering");
    }

    /// Every non-kernel scope with registry groups is reachable through the scope filter, and
    /// the filter partitions the registry (no group lost between the per-scope bins).
    #[test]
    fn scope_filter_partitions_the_registry() {
        let scopes: std::collections::BTreeSet<&str> = REGISTRY.iter().map(|g| g.scope).collect();
        let total: usize = scopes.iter().map(|s| ProjectionWorker::scope_group_count(s)).sum();
        assert_eq!(total, REGISTRY.len(), "scope counts must partition the registry");
        assert_eq!(ProjectionWorker::scope_group_count("no-such-scope"), 0);
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
        // ...and it fails as PAYLOAD SHAPE, not as a database fault. The class is what decides the
        // liveness of the whole group under `DbFaultPolicy::Halt`: a row key that will not resolve
        // is one bad record, so the group must skip it and keep draining. Until the blanket
        // `map_err(FoldFault::Database)` in `apply_record` was deleted, this failure arrived at the
        // drain loop as a schema fault and would have frozen the group on it forever.
        assert!(
            matches!(
                aggregate_uuid_of(&env, "Order-", "orderId"),
                Err(FoldFault::PayloadShape(_))
            ),
            "an unresolvable row key is per-record, not a database rejection: {:?}",
            aggregate_uuid_of(&env, "Order-", "orderId")
        );
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
