//! The GENERIC deletion engine (ADR-20260731-214500 §4, journey ADR-20260731-160000 §5/§6,
//! Order-pilot shape ADR-20260801-010134): ONE infrastructure worker runs the decided journey for
//! every actor declaring `deletion:`, parameterized entirely by the generated
//! [`DELETION_POLICIES`] table — no per-aggregate erasure process managers.
//!
//! The engine is a LOG CONSUMER, like a projector: it polls `domain_events` past its own
//! `projection_checkpoint` row (`projector = 'DeletionEngine'`) for the declared trigger facts,
//! and its scan window is BOUNDED by the slowest projection checkpoint — so by construction every
//! fact it acts on has already been folded by every read model (the phase-1 tombstone
//! verification of ADR-20260731-160000 §4, expressed as a scan bound instead of a per-fact
//! check).
//!
//! Per trigger fact, the journey is two transactions, restart-safe at every boundary:
//!
//!  1. **tx1 — the durable instruction**: append the envelope-level technical event
//!     `$StreamTombstoned` to the doomed stream (idempotent — an existing tombstone is reused).
//!     `$`-prefixed rows are not events.yaml vocabulary: folds and projectors skip them.
//!  2. **tx2 — the deletion + the receipt + the cursor, atomically**: delete the stream's rows
//!     from `domain_events` AND its `domain_stream` config row, record the declared receipt fact
//!     on the per-actor-type deletion ledger stream (`DeletionLedger-<Actor>` — never on the
//!     deleted streams), and advance the engine checkpoint past the fact. A crash between tx1
//!     and tx2 re-runs the journey from the scan; a crash after tx2 never re-sees the fact.
//!
//! The receipt payload is the one per-actor piece a generic engine cannot derive: each declaring
//! actor registers a RECEIPT BUILDER (fold the stream, extract the pseudonymous references —
//! ADR-20260731-160000 §6). A `deletion:` block without a builder, or a trigger shape the engine
//! does not serve yet (windowed engine delays, `cancelled_on` undo, child-projection
//! enumeration), FAILS `DeletionEngine::new` — loudly, at boot, behind the `RUN_DELETION_ENGINE`
//! gate — instead of silently skipping a declared erasure.

use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, Utc};
use domain::generated::events::DomainEvent;
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};

use crate::generated::deletion_policy::{DeletionPolicy, DELETION_POLICIES};

/// The engine's own checkpoint row key — excluded from the projection bound it waits on.
const CHECKPOINT_KEY: &str = "DeletionEngine";

/// The technical tombstone instruction (ADR-20260731-160000 §5): envelope-level, `$`-prefixed so
/// no fold or projector ever parses it as domain vocabulary.
const TOMBSTONE: &str = "$StreamTombstoned";

fn db_err(e: impl std::fmt::Display) -> DomainError {
    DomainError::Repository(e.to_string())
}

/// Readiness snapshot for the `/deletion` endpoint (ADR-20260728-224500: every background loop
/// publishes readiness).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionEngineStatus {
    pub running: bool,
    /// The engine's high-water mark over `domain_events.position`.
    pub checkpoint: i64,
    /// The projection bound the last tick scanned up to (the slowest read model's checkpoint).
    pub bound: i64,
    pub last_tick_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    /// Streams deleted since this process started.
    pub deleted_streams: u64,
}

/// One supported trigger, pre-validated at construction.
struct ArmedTrigger {
    policy: &'static DeletionPolicy,
    /// The trigger-fact property that carries the doomed instance's id.
    key_property: &'static str,
}

pub struct DeletionEngine {
    pool: PgPool,
    status: Arc<Mutex<DeletionEngineStatus>>,
    /// event_type → the armed trigger it fires (built once from [`DELETION_POLICIES`]).
    routes: std::collections::HashMap<&'static str, ArmedTrigger>,
    /// The ACTIVATION cache to evict erased streams from (`ACTOR_ACTIVATIONS` on): a held fold
    /// of a stream tx2 just deleted would keep GDPR-erased events in memory for its idle window
    /// AND let an append from the held version recreate a gapped stream (the erased rows mean no
    /// `UNIQUE(stream_name, version)` conflict fires) — reviewer MAJOR, 2026-08-01. `None` when
    /// the activation gate is off.
    activations: Option<Arc<crate::mailbox::StreamActivations>>,
}

impl DeletionEngine {
    /// Build the engine, PROVING it can serve every declared policy — a declaration the engine
    /// would silently skip is a GDPR erasure that half-happens, so an unsupported shape refuses
    /// to construct instead. Extend the engine (windowed delays, undo, child enumeration, a new
    /// receipt builder) in the SAME change as the spec delta that needs it.
    pub fn new(pool: PgPool) -> Result<Self, String> {
        let mut routes: std::collections::HashMap<&'static str, ArmedTrigger> =
            std::collections::HashMap::new();
        for policy in DELETION_POLICIES {
            if !has_receipt_builder(policy.actor_type) {
                return Err(format!(
                    "deletion: no receipt builder for actor '{}' — register one in infrastructure::deletion::receipt_payload alongside the spec delta",
                    policy.actor_type
                ));
            }
            for trigger in policy.triggers {
                if trigger.after_config_key.is_some() {
                    return Err(format!(
                        "deletion: '{}' declares a windowed trigger (`after`) — the engine's own delay pass is not built yet (the Order pilot windows ride the REMINDER, ADR-20260801-010134)",
                        policy.actor_type
                    ));
                }
                if !trigger.cancelled_on.is_empty() {
                    return Err(format!(
                        "deletion: '{}' declares `cancelled_on` — the undo pass is not built yet",
                        policy.actor_type
                    ));
                }
                let (Some(event_prop), Some(state_field)) =
                    (trigger.match_event_property, trigger.match_state_field)
                else {
                    return Err(format!(
                        "deletion: '{}' trigger without a typed match — the validator should have caught this",
                        policy.actor_type
                    ));
                };
                if state_field != policy.identity {
                    return Err(format!(
                        "deletion: '{}' matches on state field '{}' (identity is '{}') — child-projection enumeration is not built yet",
                        policy.actor_type, state_field, policy.identity
                    ));
                }
                for on in trigger.on {
                    if routes
                        .insert(on, ArmedTrigger { policy, key_property: event_prop })
                        .is_some()
                    {
                        return Err(format!(
                            "deletion: trigger fact '{on}' is claimed by two policies — the engine cannot route it"
                        ));
                    }
                }
            }
        }
        Ok(Self { pool, status: Arc::default(), routes, activations: None })
    }

    /// Wire the activation cache (composition root, `ACTOR_ACTIVATIONS` on) so an erased stream's
    /// held fold is evicted the moment the deletion commits.
    pub fn with_activations(mut self, cache: Arc<crate::mailbox::StreamActivations>) -> Self {
        self.activations = Some(cache);
        self
    }

    /// Shared status handle — the server reads this for its `/deletion` endpoint.
    pub fn status(&self) -> Arc<Mutex<DeletionEngineStatus>> {
        Arc::clone(&self.status)
    }

    fn status_mut(&self) -> MutexGuard<'_, DeletionEngineStatus> {
        self.status.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// One pass: scan the bounded window for trigger facts and run each journey in position
    /// order. Returns the number of streams deleted. Public so tests (and an operator shell)
    /// drive it deterministically; [`Self::run_loop`] is the production loop.
    pub async fn run_once(&self) -> Result<u64, DomainError> {
        let outcome = self.tick().await;
        let mut st = self.status_mut();
        st.last_tick_at = Some(Utc::now());
        match &outcome {
            Ok(deleted) => {
                st.deleted_streams += *deleted;
                st.last_error = None;
            }
            Err(e) => st.last_error = Some(e.to_string()),
        }
        outcome
    }

    /// Poll forever (~5s cadence — deletion is a days-scale concern; the cadence only bounds how
    /// fast the supervision surface converges). Consumes the engine; take [`Self::status`] clones
    /// before spawning.
    pub async fn run_loop(self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        self.status_mut().running = true;
        loop {
            if *shutdown.borrow() {
                break;
            }
            if let Err(e) = self.run_once().await {
                tracing::warn!(error = %e, "deletion engine: tick failed -- retrying next pass");
            }
            let sleep = tokio::time::sleep(std::time::Duration::from_secs(5));
            tokio::pin!(sleep);
            tokio::select! {
                _ = &mut sleep => {}
                changed = shutdown.changed() => {
                    if changed.is_err() {
                        // Sender dropped: no shutdown can ever arrive — keep pacing (the C1
                        // lesson from the #270 review: a dropped sender must not busy-loop).
                        sleep.await;
                    }
                }
            }
        }
        self.status_mut().running = false;
        tracing::info!("deletion engine: shut down");
    }

    async fn tick(&self) -> Result<u64, DomainError> {
        if self.routes.is_empty() {
            return Ok(0);
        }
        // Own checkpoint row (idempotent).
        sqlx::query(
            "INSERT INTO projection_checkpoint (projector, position, updated_at) \
             VALUES ($1, 0, now()) ON CONFLICT (projector) DO NOTHING",
        )
        .bind(CHECKPOINT_KEY)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        let checkpoint: i64 =
            sqlx::query_scalar("SELECT position FROM projection_checkpoint WHERE projector = $1")
                .bind(CHECKPOINT_KEY)
                .fetch_one(&self.pool)
                .await
                .map_err(db_err)?;

        // The scan bound: the SLOWEST projection checkpoint — every fact at or below it has been
        // folded by every read model (phase-1 verification). No projector rows at all (a bare
        // test database) means nothing to wait for.
        let bound: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MIN(position), 9223372036854775807) FROM projection_checkpoint \
             WHERE projector <> $1",
        )
        .bind(CHECKPOINT_KEY)
        .fetch_one(&self.pool)
        .await
        .map_err(db_err)?;
        let head: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(position), 0) FROM domain_events")
                .fetch_one(&self.pool)
                .await
                .map_err(db_err)?;
        let bound = bound.min(head);
        {
            let mut st = self.status_mut();
            st.checkpoint = checkpoint;
            st.bound = bound;
        }
        if bound <= checkpoint {
            return Ok(0);
        }

        let trigger_types: Vec<String> =
            self.routes.keys().map(|k| (*k).to_string()).collect();
        let facts = sqlx::query(
            "SELECT position, id, event_type, payload, correlation_id FROM domain_events \
             WHERE event_type = ANY($1) AND position > $2 AND position <= $3 \
             ORDER BY position",
        )
        .bind(&trigger_types)
        .bind(checkpoint)
        .bind(bound)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let mut deleted = 0u64;
        for fact in &facts {
            let position: i64 = fact.try_get("position").map_err(db_err)?;
            let event_type: String = fact.try_get("event_type").map_err(db_err)?;
            let payload: serde_json::Value = fact.try_get("payload").map_err(db_err)?;
            let fact_id: uuid::Uuid = fact.try_get("id").map_err(db_err)?;
            let correlation: uuid::Uuid = fact.try_get("correlation_id").map_err(db_err)?;
            let armed = self
                .routes
                .get(event_type.as_str())
                .ok_or_else(|| db_err(format!("unrouted trigger fact '{event_type}'")))?;
            self.journey(armed, &event_type, position, fact_id, correlation, &payload).await?;
            deleted += 1;
        }

        // Quiet window (or all journeys done): move the cursor to the bound so the scan never
        // re-reads it. Piggybacked per-journey advances make this a no-op after a busy tick.
        sqlx::query(
            "UPDATE projection_checkpoint SET position = GREATEST(position, $2), updated_at = now() \
             WHERE projector = $1",
        )
        .bind(CHECKPOINT_KEY)
        .bind(bound)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        self.status_mut().checkpoint = bound;
        Ok(deleted)
    }

    /// One instance's journey: tombstone (tx1), then delete + receipt + cursor (tx2).
    async fn journey(
        &self,
        armed: &ArmedTrigger,
        trigger_type: &str,
        fact_position: i64,
        fact_id: uuid::Uuid,
        correlation: uuid::Uuid,
        fact_payload: &serde_json::Value,
    ) -> Result<(), DomainError> {
        let key = fact_payload
            .get(armed.key_property)
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                db_err(format!(
                    "trigger fact for '{}' lacks its match property '{}'",
                    armed.policy.actor_type, armed.key_property
                ))
            })?
            .to_owned();
        let stream = format!("{}-{}", armed.policy.actor_type, key);

        // The stream's rows — the LAST read before erasure feeds the receipt builder.
        let rows = sqlx::query(
            "SELECT id, version, event_type, payload FROM domain_events \
             WHERE stream_name = $1 ORDER BY version",
        )
        .bind(&stream)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        if rows.is_empty() {
            // Already erased (a rescan across a crash after tx2, or a foreign deletion): nothing
            // left to do — the receipt was recorded atomically with the deletion.
            return self.advance_checkpoint(fact_position).await;
        }

        let mut events: Vec<DomainEvent> = Vec::with_capacity(rows.len());
        let mut max_version: i64 = 0;
        let mut tombstone_id: Option<uuid::Uuid> = None;
        for row in &rows {
            let event_type: String = row.try_get("event_type").map_err(db_err)?;
            let version: i32 = row.try_get("version").map_err(db_err)?;
            max_version = max_version.max(i64::from(version));
            if event_type.starts_with('$') {
                if event_type == TOMBSTONE {
                    tombstone_id = Some(row.try_get("id").map_err(db_err)?);
                }
                continue;
            }
            let payload: serde_json::Value = row.try_get("payload").map_err(db_err)?;
            let event: DomainEvent = serde_json::from_value(
                serde_json::json!({ "eventType": event_type, "payload": payload }),
            )
            .map_err(|e| db_err(format!("{stream} v{version} ({event_type}): {e}")))?;
            events.push(event);
        }

        // tx1 — the durable instruction (skipped when a crashed run already appended it).
        let tombstone_id = match tombstone_id {
            Some(id) => id,
            None => {
                let id = uuid::Uuid::new_v4();
                sqlx::query(
                    "INSERT INTO domain_events \
                       (id, stream_name, version, user_id, user_type, correlation_id, cause_id, \
                        event_type, payload, metadata, occurred_at) \
                     VALUES ($1, $2, $3, $4, 'EXTERNAL', $5, $6, $7, $8, NULL, now())",
                )
                .bind(id)
                .bind(&stream)
                .bind(i32::try_from(max_version + 1).map_err(db_err)?)
                .bind(engine_principal())
                .bind(correlation)
                .bind(fact_id)
                .bind(TOMBSTONE)
                .bind(serde_json::json!({
                    "triggeredBy": { "eventType": trigger_type, "position": fact_position },
                    "receipt": armed.policy.receipt,
                }))
                .execute(&self.pool)
                .await
                .map_err(db_err)?;
                id
            }
        };

        // The receipt, built from the stream's last readable state (pseudonymous refs only). A
        // stream the builder cannot read (no birth fact to fold) ABORTS the journey for retry —
        // recording a receipt that later fails to deserialize would poison the ledger stream.
        let receipt = receipt_payload(armed.policy, &key, &events)?
            // The tombstone stamp: "deleted, thanks to tombstone <id>" (ADR-20260731-160000 §6).
            .stamp(tombstone_id);

        // tx2 — deletion + receipt + cursor, atomically.
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query("DELETE FROM domain_events WHERE stream_name = $1")
            .bind(&stream)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        sqlx::query("DELETE FROM domain_stream WHERE stream_name = $1")
            .bind(&stream)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        let ledger = format!("DeletionLedger-{}", armed.policy.actor_type);
        let ledger_version: i32 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0) FROM domain_events WHERE stream_name = $1",
        )
        .bind(&ledger)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query(
            "INSERT INTO domain_events \
               (id, stream_name, version, user_id, user_type, correlation_id, cause_id, \
                event_type, payload, metadata, occurred_at) \
             VALUES ($1, $2, $3, $4, 'EXTERNAL', $5, $6, $7, $8, NULL, now())",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(&ledger)
        .bind(ledger_version + 1)
        .bind(engine_principal())
        .bind(correlation)
        .bind(tombstone_id)
        .bind(armed.policy.receipt)
        .bind(receipt.payload)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query(
            "UPDATE projection_checkpoint SET position = GREATEST(position, $2), updated_at = now() \
             WHERE projector = $1",
        )
        .bind(CHECKPOINT_KEY)
        .bind(fact_position)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        // Evict the erased stream's held activation the moment the deletion is durable — erased
        // events must not survive in memory, and a held version over deleted rows could append a
        // gapped resurrection (no UNIQUE conflict left to catch it). The in-tx freshness guard
        // is the backstop for a peer process's cache; this is the same-process fast path.
        if let Some(cache) = &self.activations {
            cache.invalidate(&stream);
        }
        tracing::info!(
            actor_type = armed.policy.actor_type,
            stream = %stream,
            receipt = armed.policy.receipt,
            tombstone = %tombstone_id,
            "deletion engine: stream erased, receipt on the ledger"
        );
        Ok(())
    }

    async fn advance_checkpoint(&self, position: i64) -> Result<(), DomainError> {
        sqlx::query(
            "UPDATE projection_checkpoint SET position = GREATEST(position, $2), updated_at = now() \
             WHERE projector = $1",
        )
        .bind(CHECKPOINT_KEY)
        .bind(position)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }
}

/// The engine's acting principal (ADR-0041 envelope metadata): a deterministic system user, like
/// the scheduler's.
fn engine_principal() -> uuid::Uuid {
    uuid::Uuid::new_v5(
        &application::reminders::inbound_namespace(),
        b"system:deletion-engine",
    )
}

/// A built receipt, pre-stamp.
struct Receipt {
    payload: serde_json::Value,
}

impl Receipt {
    fn stamp(mut self, tombstone_id: uuid::Uuid) -> Self {
        if let Some(map) = self.payload.as_object_mut() {
            map.insert(
                "tombstoneEventId".into(),
                serde_json::Value::String(tombstone_id.to_string()),
            );
        }
        self
    }
}

/// Whether [`receipt_payload`] carries an arm for this actor type — proven at engine
/// construction so a `deletion:` block can never outrun its builder.
fn has_receipt_builder(actor_type: &str) -> bool {
    matches!(actor_type, "Order")
}

/// The per-actor receipt builders — the ONE piece of the journey a generic engine cannot derive
/// (which pseudonymous references the accountability duty needs, ADR-20260731-160000 §6). A new
/// `deletion:` block lands together with its arm here (and in [`has_receipt_builder`]). The
/// `policy` field names the window key the erasure ran under — resolved from the reminder that
/// recorded the trigger fact, falling back to the trigger's own window, else `IMMEDIATE`.
fn receipt_payload(
    policy: &DeletionPolicy,
    key: &str,
    events: &[DomainEvent],
) -> Result<Receipt, DomainError> {
    let window_key = |trigger_events: &[&str]| -> String {
        application::generated::reminders::REMINDER_SCHEDULES
            .iter()
            .find(|s| s.actor_type == policy.actor_type && trigger_events.contains(&s.payload_event))
            .map(|s| s.after_key.to_string())
            .or_else(|| {
                policy
                    .triggers
                    .first()
                    .and_then(|t| t.after_config_key)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "IMMEDIATE".to_string())
    };
    match policy.actor_type {
        "Order" => {
            let Some(state) = domain::order::fold(events) else {
                return Err(db_err(format!(
                    "Order-{key}: no foldable state (birthless stream) — cannot build the OrderDeleted receipt"
                )));
            };
            let on: Vec<&str> =
                policy.triggers.iter().flat_map(|t| t.on.iter().copied()).collect();
            Ok(Receipt {
                payload: serde_json::json!({
                    "orderId": key,
                    "restaurantId": state.restaurant_id.0.to_string(),
                    "customerId": state.customer_id.0.to_string(),
                    "policy": window_key(&on),
                }),
            })
        }
        other => Err(db_err(format!("no receipt builder for actor '{other}'"))),
    }
}
