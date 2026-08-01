//! Captain.Food's half of ACTIVATIONS (PROP-20260728-152752 §3.5, #272 D3): what the runtime's
//! generic [`actor_runtime::ActivationCache`] holds here is the delivered actor's OWN stream —
//! its committed events and version, the input of every handler's fold — so a mailbox delivery
//! pays the rehydration `SELECT` once per activation instead of once per message.
//!
//! Scope discipline: only the stream named `{actor_type}-{actor_id}` of the delivered message is
//! ever CACHED. Cross-aggregate loads (a PM leg reading an Order stream) pass through, and a
//! committed delivery INVALIDATES every non-scoped stream it appended to — some other lane may be
//! holding it. Aggregates whose lane id is a surrogate (the `Payment-<intentId>` streams) simply
//! never match the scoped name and stay uncached — correct, just unaccelerated.
//!
//! The §3.5 invariant — *the state the next message sees must equal the fold of COMMITTED
//! events* — is enforced by WHERE the cache is written: fills happen on load (committed data by
//! definition), and promotion happens in the delivery's POST-COMMIT hook (apply-after-commit:
//! the staged events join the held state only once the fenced transaction committed, and a
//! version mismatch at promotion time drops the entry instead of guessing). A lost
//! optimistic-concurrency race at flush time invalidates (the "never wrong" eviction signal);
//! lane loss drops the whole partition's holdings via [`ActivationLaneEvents`]; idle expiry and
//! the LRU byte bound live in the runtime cache itself.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use actor_runtime::{ActivationCache, InboundMessage, Lane, LaneEvents};
use application::ports::{Actor, EventStore};
use application::staging::StagedAppend;
use async_trait::async_trait;
use domain::generated::events::DomainEvent;
use domain::shared::errors::DomainError;

/// One held activation: the scoped stream's committed events and version.
pub struct CachedStream {
    pub events: Vec<DomainEvent>,
    pub version: i64,
}

/// The shared dictionary type (one per process, all actor types).
pub type StreamActivations = ActivationCache<CachedStream>;

/// The composition root's wiring: the cache plus the per-actor policy resolved from the
/// generated `ACTOR_ACTIVATIONS` table and the configuration knobs
/// (`ACTOR_ACTIVATIONS` gate, `ACTOR_ACTIVATION_IDLE_SECONDS`, `ACTOR_ACTIVATION_MAX_MEMORY_MB`).
pub struct ActivationSettings {
    pub cache: Arc<StreamActivations>,
    /// `ACTOR_ACTIVATION_IDLE_SECONDS` — the global passivation default.
    pub idle_default: Duration,
    /// Per-actor overrides (actors.yaml `mailbox.activations`): `(enabled, idle override)`.
    pub per_actor: HashMap<&'static str, (bool, Option<Duration>)>,
}

impl ActivationSettings {
    /// The idle window for one actor type, or `None` when that actor opted out
    /// (`mailbox.activations: false`).
    pub fn idle_for(&self, actor_type: &str) -> Option<Duration> {
        match self.per_actor.get(actor_type) {
            Some((false, _)) => None,
            Some((true, idle)) => Some(idle.unwrap_or(self.idle_default)),
            None => Some(self.idle_default),
        }
    }
}

/// The activation context of ONE delivery: the scoped stream and the lane it is held under.
pub(super) struct DeliveryActivation {
    settings: Arc<ActivationSettings>,
    scoped: String,
    actor_type: String,
    partition: i16,
    idle: Duration,
    /// The held version SERVED to this delivery's fold, when the scoped load was a cache HIT.
    /// `None` = the fold read the log directly (miss, or never loaded) — the pre-activation
    /// freshness window applies and no extra guard is owed.
    served: Arc<std::sync::Mutex<Option<i64>>>,
}

impl DeliveryActivation {
    /// Build the context for a delivered message, if activations apply to its actor type.
    pub(super) fn for_message(
        settings: &Option<Arc<ActivationSettings>>,
        message: &InboundMessage,
    ) -> Option<Self> {
        let settings = settings.as_ref()?;
        let idle = settings.idle_for(&message.actor_type)?;
        Some(Self {
            settings: settings.clone(),
            scoped: format!("{}-{}", message.actor_type, message.actor_id),
            actor_type: message.actor_type.clone(),
            partition: message.partition,
            idle,
            served: Arc::new(std::sync::Mutex::new(None)),
        })
    }

    /// The store the handler folds over: scoped-stream loads served from (and filling) the cache.
    pub(super) fn store(&self, inner: Arc<dyn EventStore>) -> Arc<dyn EventStore> {
        Arc::new(ActivationEventStore {
            inner,
            settings: self.settings.clone(),
            scoped: self.scoped.clone(),
            actor_type: self.actor_type.clone(),
            partition: self.partition,
            idle: self.idle,
            served: self.served.clone(),
        })
    }

    /// The version-conflict reaction: the held state is provably stale — drop it so the retry
    /// refolds from the log.
    pub(super) fn invalidate_scoped(&self) {
        self.settings.cache.invalidate(&self.scoped);
    }

    /// THE FRESHNESS GUARD (reviewer CRITICAL, 2026-08-01): when this delivery's fold was served
    /// from the cache, re-assert the scoped stream's live version INSIDE the fenced completion
    /// transaction. Without it, a delivery that terminates WITHOUT appending to its own stream
    /// (a deterministic rejection, an Ignored/Duplicate record verdict) has no
    /// `UNIQUE(stream_name, version)` race to lose — a stale hold (an out-of-band writer: the
    /// saga runner's pool-side legs, a legacy-arm PM spawn, a peer process's fleet, the deletion
    /// engine erasing the stream) would commit a durably WRONG verdict, and each retry's cache
    /// touch would keep the stale entry alive. A mismatch invalidates and aborts — the row stays
    /// RECEIVED and the redelivery refolds from the log. Appends to the scoped stream are also
    /// covered on the deleted-stream edge (erased rows mean no UNIQUE conflict would fire), so
    /// the guard runs for EVERY cache-served delivery, append or not. The comparison matches
    /// `PgEventStore::load`'s version semantics exactly: `MAX(version)` over ALL rows,
    /// `$`-prefixed technical rows included.
    pub(super) async fn guard_freshness_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), sqlx::Error> {
        let served = *self.served.lock().expect("served poisoned");
        let Some(served) = served else { return Ok(()) };
        let live: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(version)::bigint FROM domain_events WHERE stream_name = $1",
        )
        .bind(&self.scoped)
        .fetch_one(&mut **tx)
        .await?;
        let live = live.unwrap_or(0);
        if live != served {
            self.invalidate_scoped();
            return Err(sqlx::Error::Protocol(format!(
                "activation: held fold of '{}' is stale (held v{served}, live v{live}) -- delivery aborted for refold",
                self.scoped
            )));
        }
        Ok(())
    }

    /// The POST-COMMIT promotion closure (apply-after-commit): extend the held state with the
    /// appends that just became durable; invalidate any OTHER stream this delivery wrote (a
    /// cross-lane writer's held copy is now stale). Built before the commit, run strictly after.
    pub(super) fn promote_after_commit(
        settings: &Option<Arc<ActivationSettings>>,
        scoped: Option<&DeliveryActivation>,
        staged: &[StagedAppend],
    ) -> Option<Box<dyn FnOnce() + Send>> {
        let settings = settings.as_ref()?.clone();
        if staged.is_empty() {
            return None;
        }
        let scoped_ctx = scoped.map(|s| (s.scoped.clone(), s.actor_type.clone(), s.partition, s.idle));
        let staged: Vec<(String, i64, Vec<DomainEvent>)> = staged
            .iter()
            .map(|a| (a.stream_name.clone(), a.expected_version, a.events.clone()))
            .collect();
        Some(Box::new(move || {
            for (stream, expected_version, events) in staged {
                match &scoped_ctx {
                    Some((scoped, actor_type, partition, idle)) if *scoped == stream => {
                        match settings.cache.get(&stream) {
                            Some(held) if held.version == expected_version => {
                                let mut all = held.events.clone();
                                all.extend(events.iter().cloned());
                                let version = expected_version + events.len() as i64;
                                let value = CachedStream { events: all, version };
                                let bytes = estimate_bytes(&value.events);
                                settings.cache.put(
                                    &stream, actor_type, *partition, *idle, bytes,
                                    Arc::new(value),
                                );
                            }
                            // Held at a different version (a promotion raced an invalidation,
                            // or the fill happened mid-delivery): never guess — drop it.
                            Some(_) => settings.cache.invalidate(&stream),
                            // Not held (evicted under memory pressure mid-delivery): nothing
                            // to promote; the next delivery refolds and refills.
                            None => {}
                        }
                    }
                    _ => settings.cache.invalidate(&stream),
                }
            }
        }))
    }
}

/// Byte estimate for the LRU bound: the serialized size of the held events (close enough to the
/// heap cost, cheap to compute on fill/promote only).
fn estimate_bytes(events: &[DomainEvent]) -> usize {
    events
        .iter()
        .map(|e| serde_json::to_vec(e).map(|v| v.len()).unwrap_or(1024))
        .sum::<usize>()
        + 128
}

/// An [`EventStore`] serving the SCOPED stream's loads from the activation cache (filling it on
/// miss) and passing everything else — every other stream, every append — to the inner store.
struct ActivationEventStore {
    inner: Arc<dyn EventStore>,
    settings: Arc<ActivationSettings>,
    scoped: String,
    actor_type: String,
    partition: i16,
    idle: Duration,
    /// Records the held version served on a scoped cache HIT — what
    /// [`DeliveryActivation::guard_freshness_in_tx`] re-asserts at commit time.
    served: Arc<std::sync::Mutex<Option<i64>>>,
}

#[async_trait]
impl EventStore for ActivationEventStore {
    async fn append(
        &self,
        stream_name: &str,
        expected_version: i64,
        events: &[DomainEvent],
        actor: &Actor,
    ) -> Result<i64, DomainError> {
        // Delivery paths stage appends above this store; a direct append (defensive) must not
        // leave a stale held state behind.
        self.settings.cache.invalidate(stream_name);
        self.inner.append(stream_name, expected_version, events, actor).await
    }

    async fn load(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError> {
        if stream_name != self.scoped {
            return self.inner.load(stream_name).await;
        }
        if let Some(held) = self.settings.cache.get(stream_name) {
            *self.served.lock().expect("served poisoned") = Some(held.version);
            return Ok((held.events.clone(), held.version));
        }
        // TOCTOU fence: capture the invalidation epoch BEFORE the pool read — an invalidation
        // racing this fill (a cross-lane writer's post-commit signal) means the snapshot below
        // may predate it, and installing it would resurrect exactly the announced staleness.
        let epoch = self.settings.cache.epoch();
        let (events, version) = self.inner.load(stream_name).await?;
        let bytes = estimate_bytes(&events);
        self.settings.cache.put_at_epoch(
            stream_name,
            &self.actor_type,
            self.partition,
            self.idle,
            bytes,
            Arc::new(CachedStream { events: events.clone(), version }),
            epoch,
        );
        Ok((events, version))
    }
}

/// The worker-side eviction hook: a lane this worker stops believing it owns drops every held
/// state under it (the new owner may write those actors).
pub struct ActivationLaneEvents(pub Arc<StreamActivations>);

impl LaneEvents for ActivationLaneEvents {
    fn lane_dropped(&self, lane: &Lane) {
        self.0.drop_partition(&lane.actor_type, lane.partition);
    }
}
