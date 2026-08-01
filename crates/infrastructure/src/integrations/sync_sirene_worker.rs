//! The on-app SIRENE translation worker (ADR-0045) — the domain half of the split sync, mirroring the
//! projection worker's drain/checkpoint pattern (ADR-0040).
//!
//! The thin CI ingestion (`sirene_ingest` bin) UPSERTs raw INSEE records into the
//! `external_sirene_restaurants` staging table; THIS worker — running on the deployed server, i.e.
//! versioned exactly like the in-process projector — drains the pending rows, runs the SIRENE
//! Anti-Corruption Layer ([`super::sirene::etablissement_to_registered_event`]) and STAGES the fact
//! handlers (`register_restaurant`, `mark_restaurant_closed`), so ALL domain logic executes on the
//! deployed version (the version-skew hazard of the retired direct-write CI binary is gone).
//!
//! **Checkpoint** = the per-row `processed_at` high-water mark: `processed_at IS NULL OR
//! processed_at < last_seen_at ⇒ pending`. Marking a drained row `processed_at = last_seen_at` (the
//! value READ, not `now()`) makes a concurrent ingestion bump re-pend the row instead of being lost.
//!
//! **The payload is TRANSIENT** (ADR-20260728-143000, #231). It is an input to TRANSLATION — needed
//! from the moment INSEE reports a change until this worker has turned it into a domain fact, and never
//! again. It is dropped only once that is CERTAIN, in the same statement that records the certainty:
//! `reconcile_staged` for the register path (when the aggregate's verdict comes back) and
//! [`SireneSyncWorker::mark_processed`] for the explicit closure (whose command executes inline). Never
//! at hand-over — a staged fact has not been accepted yet, and the raw record is the only original. At ~1.8 kB a row the mirror was 77% of the database (655 MB at department 37 of 101, on
//! a 2 GB disk), which is what gated national coverage. The hash persists; only the payload goes.
//! One class of row keeps it: anything the ACL could not MAP, because that payload is the only evidence
//! of why INSEE's record was unusable.
//!
//! **Deletion reconciliation** (ADR-0045 deletion policy) reuses the already-modeled
//! `MarkRestaurantClosed` → `RestaurantMarkedClosed`:
//! - explicit signal: a staged row whose `etat` is `F` (fermé) is a confident closure;
//! - detect-by-absence: rows not refreshed for [`ABSENCE_GRACE_DAYS`] past their OWN DEPARTMENT's
//!   latest sweep (≈ 3 missed weekly runs — the debounce), guarded by a per-department freshness check
//!   so neither a stalled CI nor a budget-limited rotating sweep (#218) can mass-close anything;
//! - gate by the partnership funnel: only **NON_PARTNER** prospects are auto-closed; PASSIVE/ACTIVE
//!   partners are flagged for manual review (a registry datum must never take down a live partner);
//! - never hard-delete: closure is an event, the projection folds it, the staging mirror keeps the row.
//!
//! Idempotent by construction: `restaurantId = UUIDv5(SIRET)` absorbs re-registrations, and a close is
//! only issued while the aggregate folds to a non-INACTIVE status (an already-closed prospect is a
//! no-op). A SIRET that REAPPEARS active after a closure re-pends its row; the register replay is
//! absorbed and the restaurant stays INACTIVE — reactivation is deliberately manual (logged).
//!
//! **Journaled sends** (ADR-20260720-015300, #15): every command this worker issues goes through the
//! WORKER-channel journaling dispatch ([`application::dispatch::dispatch_journaled`]) — `message_id`
//! = UUIDv5 of (command type, SIRET, the staged row's `last_seen_at`), so re-draining the SAME staged
//! version dedupes on `command_journal` while an ingestion refresh (which bumps `last_seen_at`)
//! journals as a new send; `cause_id` = UUIDv5(`row:<SIRET>`) — the staging-mirror row's identity —
//! making `external_sirene_restaurants → command_journal → domain_events` fully traceable.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use application::journal::{payload_hash, CommandJournalEntry};
use application::ports::Actor;
use application::repository::Repository;
use domain::restaurant::RestaurantState;
use chrono::{DateTime, Utc};
use domain::generated::commands::MarkRestaurantClosed;
use domain::generated::events::DomainEvent;
use domain::generated::scalars::{
    CommandChannel, RestaurantListingStatus, RestaurantStatus,
};

use crate::mailbox::{
    enqueue_inbound_fact, enqueue_worker_command, EnqueueOutcome, InboundFact,
};
use crate::persistence::mailbox_store::PgMailbox;
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};

use crate::integrations::sirene::{
    etablissement_to_registered_event, restaurant_id_for_siret, sirene_system_user_id, sirene_uuid,
    Etablissement,
};
use crate::persistence::db_err;
use crate::{PgEventStore, PgRestaurantRepository};

/// Safety-net poll interval. The PRIMARY trigger is the ingestion's ping on
/// `POST /internal/sirene/drain`; the loop only catches missed pings, so it can be slow.
const POLL_INTERVAL: Duration = Duration::from_secs(3600);
/// Pending rows are drained in keyset batches (ordered by SIRET) of this size.
const BATCH_SIZE: i64 = 200;
/// Detect-by-absence debounce: a row must be unseen for this long past ITS DEPARTMENT's latest sweep
/// before a closure is inferred (≈ 3 missed weekly runs, per ADR-0045).
const ABSENCE_GRACE_DAYS: i64 = 21;
/// Absence is only meaningful against a FRESHLY-SWEPT department: a department whose latest sweep is
/// older than this yields NO absence signal (#218). Scoped per department rather than globally because
/// the sweep is budget-limited and rotates — most departments are legitimately stale at any moment, and
/// a global freshness check would read that as mass closure.
const FRESH_INGESTION_DAYS: i64 = 10;
/// `UserType::EXTERNAL` TEXT value for the event envelope (enums stored verbatim, ADR-20260728) —
/// the sync writes as the fixed external system principal.
const EXTERNAL_USER_TYPE: &str = "EXTERNAL";
/// Consecutive failed sync attempts after which a row is QUARANTINED as `POISON` and the drain stops
/// selecting it (#231).
///
/// A failed sync deliberately leaves the row pending WITH its payload, because the retry needs
/// something to translate — which means a permanently-failing row is otherwise retried on every pass
/// forever, burning budget and emitting an error nobody acts on. That failure mode is not theoretical
/// here: the 605-row `SlugAlreadyTaken` log storm was exactly this shape. Ten is generous enough that
/// no transient outage (a deploy, a lock, a brief connection loss) can quarantine a healthy row, and
/// small enough that a genuinely broken one stops costing anything within a sweep or two.
const MAX_SYNC_ATTEMPTS: i32 = 10;

/// The `inbound_messages.source` this adapter owns. `(source, external_id)` is the delivery-level dedupe,
/// and unlike `command_journal`'s `last_seen_at`-seeded message_id it is STABLE across sweeps
/// (ADR-20260728-011344).
const SIRENE_SOURCE: &str = "sirene";

/// The WORKER-channel journal entry for one command a staged row caused (#15). `message_id` is
/// deterministic over (command type, SIRET, the row's `last_seen_at` as READ) — a re-drain of the
/// same staged version replays the same id and dedupes; a refreshed row (bumped `last_seen_at`) is a
/// new send. `cause_id` names the mirror row, closing the staging → journal → events chain.
fn journal_entry(
    command_type: &str,
    siret: &str,
    last_seen_at: DateTime<Utc>,
    correlation_id: uuid::Uuid,
    payload: serde_json::Value,
) -> CommandJournalEntry {
    let seed = format!("command:{command_type}:{siret}:{}", last_seen_at.to_rfc3339());
    CommandJournalEntry {
        message_id: sirene_uuid(&seed),
        correlation_id,
        cause_id: Some(sirene_uuid(&format!("row:{siret}"))),
        session_id: None,
        trace_id: None,
        user_id: Some(sirene_system_user_id()),
        user_type: EXTERNAL_USER_TYPE.to_string(),
        channel: CommandChannel::WORKER,
        command_type: command_type.to_string(),
        payload_hash: payload_hash(&payload),
        payload,
    }
}

/// Envelope → `Actor` (ADR-0041) for the ENQUEUE: the mailbox row's `cause_id` is the PARENT
/// (the staging-mirror row's identity) — the delivery side rebuilds the event-append actor with
/// `cause_id = message_id` itself, exactly like the GraphQL dispatch.
fn send_actor(entry: &CommandJournalEntry) -> Actor {
    Actor {
        user_id: sirene_system_user_id(),
        user_type: EXTERNAL_USER_TYPE.to_string(),
        domain_id: None,
        correlation_id: entry.correlation_id,
        cause_id: entry.cause_id,
    }
}

fn serialize_command<T: serde::Serialize>(command_type: &str, cmd: &T) -> Result<serde_json::Value, DomainError> {
    serde_json::to_value(cmd)
        .map_err(|e| DomainError::Repository(format!("serialize {command_type}: {e}")))
}

/// What one drain pass did — logged by the loop and by the ping-triggered run.
#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SireneSyncSummary {
    /// Pending staging rows drained (marked processed or left for retry).
    pub processed: u64,
    /// Inbound registry facts STAGED for the drain (ADR-20260728-011344). No longer "commands issued",
    /// and no longer conflating new prospects with replays: what each staged fact turned out to MEAN
    /// (created / updated / no-change) is the drain's verdict, readable as
    /// `SELECT status, count(*) FROM inbound_messages WHERE source = 'sirene'`. This counter only says how
    /// many rows we handed over.
    pub registered: u64,
    /// `MarkRestaurantClosed` issued (NON_PARTNER prospects only).
    pub closed: u64,
    /// Closure signals on PASSIVE/ACTIVE partners — NOT auto-closed, raised for manual review.
    pub flagged_for_review: u64,
    /// Unmappable rows (bad SIRET, no name/address, unparsable payload) — skipped, marked processed.
    pub skipped: u64,
    /// Previously-STAGED rows whose aggregate verdict arrived and was written back (SYNCED / FAILED).
    /// Lags the staging by at least one drain, by design — the verdict does not exist at hand-over.
    pub resolved: u64,
    /// Write/load failures — the row stays pending and is retried on the next run.
    pub failed: u64,
}

/// What the worker decided about one staged row — the value behind
/// `external_sirene_restaurants.status`, which answers "has this row been SYNCED, or not?" (#231).
///
/// It exists because the payload became transient. Once `payload` is nullable, a row that HAS one is
/// either still awaiting translation or was kept as evidence of an unmappable record, and nothing
/// distinguishes the two — so the status is not decoration, it is what makes the table readable.
/// `GROUP BY status` is the operator's per-sweep report.
///
/// Deliberately NOT a domain scalar: the value is also written by the CI `sirene_ingest` crate, which
/// cannot see domain types (ADR-0045) — this adapter-owned staging status simply stays outside
/// scalars.yaml (since ADR-20260728 domain enums persist as TEXT too, so the conventions agree).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowOutcome {
    /// HANDED OVER to the inbox, verdict not yet known. The ACL translated the record and staged an
    /// inbound fact; the aggregate has not decided anything yet — the Restaurant lane's MailboxWorker
    /// delivers it and the verdict lands on `inbound_messages.status`. Calling this SYNCED would claim more than
    /// this worker knows: a later delivery failure or rejection would leave the mirror asserting a
    /// success that never happened. `reconcile_staged` resolves it on a later pass.
    ///
    /// The payload is KEPT. It was previously dropped here on the argument that the inbound row carries
    /// its own translated copy — true, but it is the TRANSLATED copy, and if the ACL turns out to have
    /// mistranslated the record, re-running it needs the RAW one. Dropping at hand-over destroys the
    /// only original before anything has confirmed the translation was even accepted. The payload goes
    /// in `reconcile_staged`, at the moment the verdict makes the sync certain.
    Staged,
    /// Actually completed. Only the explicit-closure path reaches this synchronously: `MarkRestaurantClosed`
    /// is a real command executed inline, so its outcome is known when this is written.
    Synced,
    /// Parsed but rejected by the ACL, or not parsable at all. Payload KEPT as evidence. Still
    /// checkpointed, so it is not retried forever — a refresh from INSEE re-pends it.
    Unmappable,
}

impl RowOutcome {
    fn status(self) -> &'static str {
        match self {
            RowOutcome::Staged => "STAGED",
            RowOutcome::Synced => "SYNCED",
            RowOutcome::Unmappable => "UNMAPPABLE",
        }
    }

    /// A payload is dropped ONLY once the sync is certain. `Synced` qualifies because the only outcome
    /// that reaches it synchronously is the explicit closure, where `MarkRestaurantClosed` has actually
    /// executed. `Staged` does not: the aggregate has not decided yet. `Unmappable` does not: the
    /// payload is the evidence of why the record was unusable.
    fn clears_payload(self) -> bool {
        matches!(self, RowOutcome::Synced)
    }

    /// `synced_at` is a claim that the record REACHED THE DOMAIN, so only a known-complete outcome may
    /// stamp it. A staged fact has not been decided yet; `reconcile_staged` stamps it when the verdict
    /// arrives.
    fn stamps_synced_at(self) -> bool {
        matches!(self, RowOutcome::Synced)
    }
}

/// Readiness snapshot published by the worker and served by `GET /sirene` — the counterpart of
/// [`crate::projection::ProjectionStatus`] behind `/projector` (issue #244).
///
/// It exists because a paused sync loop and a crashing one look IDENTICAL from outside the process:
/// during the department-37 pilot (#238) 6,649 rows sat `PENDING` for four hours and answering "is the
/// worker even running?" needed the Render boot log. `running` answers it; `last_error` separates a
/// loop that never started from one failing every pass; `last_summary` shows what the last pass did.
#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SireneSyncStatus {
    /// Whether the polling loop is running. FALSE also covers "constructed for the ping endpoint but
    /// `RUN_SIRENE_WORKER` never started the loop" — the case the pilot could not diagnose.
    pub running: bool,
    /// When the worker last completed a pass, successful or not.
    pub last_tick_at: Option<DateTime<Utc>>,
    /// The last pass's error, cleared by the next successful pass. A pass rejected because another was
    /// already draining is NOT an error and does not touch this.
    pub last_error: Option<String>,
    /// What the last successful pass did — the same counters the loop logs.
    pub last_summary: Option<SireneSyncSummary>,
}

/// The worker: owns the pool, guards against overlapping drains (the ping endpoint and the poll loop
/// share one instance behind an `Arc`).
pub struct SireneSyncWorker {
    pool: PgPool,
    draining: AtomicBool,
    status: Arc<Mutex<SireneSyncStatus>>,
}

impl SireneSyncWorker {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            draining: AtomicBool::new(false),
            status: Arc::new(Mutex::new(SireneSyncStatus::default())),
        }
    }

    /// Shared status handle — the server reads this for its `/sirene` endpoint. Take a clone BEFORE
    /// spawning the loop (which consumes an `Arc<Self>`), exactly as `ProjectionWorker::status` is used.
    pub fn status(&self) -> Arc<Mutex<SireneSyncStatus>> {
        Arc::clone(&self.status)
    }

    fn status_mut(&self) -> MutexGuard<'_, SireneSyncStatus> {
        // A poisoned lock only means a reader panicked mid-inspection; the snapshot stays usable.
        self.status.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// One full drain pass: translate every pending staging row, then reconcile disappearances.
    /// Errors early only on infrastructural failures (DB unreachable); per-record failures are
    /// counted on the summary and retried next run. A concurrent drain is rejected (the caller may
    /// simply skip — the running pass will pick the same rows up).
    pub async fn run_once(&self) -> Result<SireneSyncSummary, DomainError> {
        if self.draining.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Err(DomainError::Repository(
                "sirene sync worker: a drain pass is already running".to_string(),
            ));
        }
        let result = self.drain().await;
        self.draining.store(false, Ordering::SeqCst);
        // Publish the outcome for `/sirene` (#244). Deliberately AFTER releasing the drain guard and
        // recorded for ping-triggered passes too — the endpoint's job is to describe the worker, not
        // just the loop. The rejected-because-already-draining path returns above without touching the
        // snapshot: a skipped duplicate pass is not an error and must not clear a real one.
        {
            let mut st = self.status_mut();
            st.last_tick_at = Some(Utc::now());
            match &result {
                Ok(summary) => {
                    st.last_summary = Some(summary.clone());
                    st.last_error = None;
                }
                Err(e) => st.last_error = Some(e.to_string()),
            }
        }
        result
    }

    /// Poll forever as a safety net behind the ping trigger: `run_once` then sleep [`POLL_INTERVAL`].
    /// Takes `Arc<Self>` (unlike the projection worker's consuming loop) so the same instance stays
    /// shared with the `/internal/sirene/drain` endpoint.
    pub async fn run_loop(self: Arc<Self>) {
        self.status_mut().running = true;
        loop {
            match self.run_once().await {
                Ok(summary) if summary.processed > 0 || summary.closed > 0 => {
                    tracing::info!(worker = "sirene_sync", summary = ?summary, "sweep pass complete");
                }
                Ok(_) => {} // nothing pending — stay quiet
                Err(e) => tracing::error!(worker = "sirene_sync", error = %e, "sweep pass failed"),
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    async fn drain(&self) -> Result<SireneSyncSummary, DomainError> {
        let mut summary = SireneSyncSummary::default();
        let store = PgEventStore::new(self.pool.clone());
        // Backs register_restaurant's SlugAlreadyTaken check; a re-synced SIRET matches its own row
        // (same deterministic id) and stays an idempotent no-op.
        let restaurants = PgRestaurantRepository::new(self.pool.clone());
        // THE MAILBOX IS THE ONLY DOOR (ADR-20260731-122500): closures enqueue a WORKER-channel
        // command, registrations enqueue an inbound FACT (ADR-20260728-011344 D4) — both
        // fire-and-forget; the mailbox worker delivers and the ledger owns the outcome.
        let mailbox = PgMailbox::new(self.pool.clone());
        // Fresh correlation id per pass so all mailbox rows + events of one drain are traceable
        // together; each send derives its own message_id/cause_id ([`journal_entry`]).
        let correlation_id = uuid::Uuid::new_v4();

        // 1) Pending rows, keyset-paginated by SIRET: a row left pending after a write failure is
        //    behind the cursor, so one pass always terminates (it is retried on the NEXT pass).
        let mut after = String::new();
        loop {
            let rows = sqlx::query(
                // `status <> 'POISON'` is what makes the quarantine real. Without it the row still
                // matches the pending predicate (a failure never advances `processed_at`) and would be
                // re-attempted on every pass forever. A changed record from INSEE resets the status to
                // PENDING through the ingestion's conflict arm, so recovery needs no operator action.
                "SELECT siret, payload, etat, last_seen_at, payload_hash FROM external_sirene_restaurants \
                 WHERE siret > $1 AND (processed_at IS NULL OR processed_at < last_seen_at) \
                   AND status <> 'POISON' \
                 ORDER BY siret LIMIT $2",
            )
            .bind(&after)
            .bind(BATCH_SIZE)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                let siret: String = row.try_get("siret").map_err(db_err)?;
                // Nullable since #231: the payload lives only while the row is pending, so a pending
                // row normally HAS one — a NULL here is an anomaly, handled in `process_row`.
                let payload: Option<serde_json::Value> = row.try_get("payload").map_err(db_err)?;
                let etat: String = row.try_get("etat").map_err(db_err)?;
                let last_seen_at: DateTime<Utc> = row.try_get("last_seen_at").map_err(db_err)?;
                let staged_hash: String = row.try_get("payload_hash").map_err(db_err)?;
                after = siret.clone();
                self.process_row(&store, &restaurants, &mailbox, correlation_id, &siret, payload, &etat, last_seen_at, &staged_hash, &mut summary)
                    .await?;
            }
        }

        // 2) Resolve rows whose fact has since been decided by the aggregate.
        self.reconcile_staged(&mut summary).await?;

        // 3) Deletion reconciliation by absence (debounced, freshness-guarded, partner-gated).
        self.reconcile_absent(&store, &restaurants, &mailbox, correlation_id, &mut summary).await?;

        Ok(summary)
    }

    /// Translate one pending staging row. Only mark-processed/SELECT/journal failures propagate; ACL
    /// and write-path failures are counted (`skipped`/`failed`) so the pass keeps going.
    #[allow(clippy::too_many_arguments)]
    async fn process_row(
        &self,
        store: &PgEventStore,
        restaurants: &PgRestaurantRepository,
        mailbox: &PgMailbox,
        correlation_id: uuid::Uuid,
        siret: &str,
        payload: Option<serde_json::Value>,
        etat: &str,
        last_seen_at: DateTime<Utc>,
        staged_hash: &str,
        summary: &mut SireneSyncSummary,
    ) -> Result<(), DomainError> {
        summary.processed += 1;

        // A pending row with no payload cannot happen by design (the ingestion writes the payload for
        // exactly the rows it pends, #231) — but if it ever does, leaving it pending would spin the
        // worker on it forever. Mark it processed, keep nothing to clear, and say so loudly.
        let Some(payload) = payload else {
            summary.skipped += 1;
            tracing::error!(
                worker = "sirene_sync",
                %siret,
                "pending row has NO payload -- nothing to translate. This should be impossible; \
                 the next ingestion refresh will re-pend it with one."
            );
            return self.mark_processed(siret, last_seen_at, RowOutcome::Unmappable).await;
        };

        let etablissement: Etablissement = match serde_json::from_value(payload) {
            Ok(e) => e,
            Err(e) => {
                // A poison payload will not fix itself on retry — skip until the ingestion
                // refreshes it (which re-pends the row). The payload is KEPT: it is the evidence of
                // what could not be parsed (#231 D3).
                summary.skipped += 1;
                tracing::error!(worker = "sirene_sync", %siret, error = %e, "unparsable staged payload -- row marked Unmappable");
                return self.mark_processed(siret, last_seen_at, RowOutcome::Unmappable).await;
            }
        };

        // Explicit closure signal — the staging `etat` column (stamped at ingestion) or the payload's
        // own current period (`F` fermé / `C` cessation).
        let explicitly_closed = etat == "F" || matches!(etablissement.etat(), Some("F") | Some("C"));
        if explicitly_closed {
            match self
                .close_if_prospect(store, restaurants, mailbox, correlation_id, siret, last_seen_at, "SIRENE: establishment administratively closed (etat=F)", summary)
                .await
            {
                Ok(()) => return self.mark_processed(siret, last_seen_at, RowOutcome::Synced).await,
                Err(e) => {
                    summary.failed += 1; // left pending → retried next pass
                    tracing::error!(worker = "sirene_sync", %siret, error = %e, "close failed");
                    self.mark_failed(siret).await;
                    return Ok(());
                }
            }
        }

        // Active record → ACL → STAGED as an inbound fact (ADR-20260728-011344 D4).
        //
        // No command, no command_journal, and no read-model lookup. What changed and why:
        //
        // * INSEE cannot be told "no", so a command was always the wrong shape — the old path's
        //   rejections went to an eprintln! and nobody. This stages the fact and lets the AGGREGATE
        //   decide: record / update / nothing (see `record_inbound_restaurant_registration`).
        // * The legacy-id adoption lookup is GONE. It ran `external_identifiers @> $1` against the
        //   `Restaurant` projection — a JSONB containment scan with no GIN index, once per staged SIRET,
        //   which made it quadratic and very likely the single largest disk-IO consumer of a sweep. It
        //   was also asking the eventually-consistent READ side to make a write decision. The aggregate
        //   id is `UUIDv5(SIRET)`, deterministic, so no lookup is needed to find "the same restaurant".
        // * `external_id` is `{siret}:{payload_hash}` — a STABLE dedupe key. `command_journal`'s
        //   message_id was seeded on `last_seen_at`, so it could never dedupe across sweeps and every
        //   SIRET produced a fresh journal row every week.
        match etablissement_to_registered_event(&etablissement) {
            Ok(event) => {
                let external_id = format!("{siret}:{staged_hash}");
                // The addressed lane: the Restaurant aggregate this fact belongs to — its id is
                // deterministic (UUIDv5 of the SIRET), carried on the event itself.
                let restaurant_uuid = event.restaurant_id.0;
                let fact = InboundFact {
                    source: SIRENE_SOURCE.to_string(),
                    external_id: external_id.clone(),
                    event_type: "RestaurantRegistered".to_string(),
                    // The ADJACENTLY-TAGGED union form (`{"eventType", "payload"}`) — the EVENT
                    // route deserializes `DomainEvent`, so a bare payload here is undeliverable.
                    payload: serde_json::to_value(DomainEvent::RestaurantRegistered(event))
                        .map_err(|e| {
                            DomainError::Repository(format!("serialize RestaurantRegistered: {e}"))
                        })?,
                    // The established ACL convention: UUIDv5 of the external id, so the whole chain
                    // (staging row → mailbox row → appended fact) shares one correlation.
                    correlation_id: sirene_uuid(&format!("inbound:{external_id}")),
                    actor_type: "Restaurant".to_string(),
                    actor_id: restaurant_uuid,
                };
                match enqueue_inbound_fact(mailbox, fact).await {
                    Ok(EnqueueOutcome::Enqueued) => {
                        summary.registered += 1;
                        self.mark_processed(siret, last_seen_at, RowOutcome::Staged).await
                    }
                    // Already on the mailbox — this exact (siret, payload_hash) is either awaiting
                    // delivery or already decided. Reconciliation reads the verdict off the row
                    // that is already there.
                    Ok(EnqueueOutcome::Deduplicated(_)) | Ok(EnqueueOutcome::PayloadConflict(_)) => {
                        summary.skipped += 1;
                        self.mark_processed(siret, last_seen_at, RowOutcome::Staged).await
                    }
                    Err(e) => {
                        summary.failed += 1; // left pending → retried next pass
                        tracing::error!(worker = "sirene_sync", %siret, error = %e, "mailbox enqueue failed");
                        self.mark_failed(siret).await;
                        Ok(())
                    }
                }
            }
            Err(e) => {
                // Unusable record (redacted, nameless, no address…) — log + mark processed; the next
                // ingestion refresh re-pends it if INSEE's data improves.
                // UNMAPPABLE — payload deliberately RETAINED (#231 D3): it is the only evidence of
                // why INSEE's record was unusable, and a silent unmappable row with no evidence is
                // how a systematic mapping bug hides.
                summary.skipped += 1;
                tracing::warn!(worker = "sirene_sync", error = %e, "row skipped");
                self.mark_processed(siret, last_seen_at, RowOutcome::Unmappable).await
            }
        }
    }

    /// Advance the per-row checkpoint to the `last_seen_at` we READ (not `now()`): if an ingestion
    /// bumped the row meanwhile, `processed_at < last_seen_at` still holds and it is re-drained.
    ///
    /// `outcome` carries BOTH the recorded status and the payload lifetime (ADR-20260728-143000, #231),
    /// written in the SAME statement as the checkpoint so the three can never disagree — a row can
    /// never end up marked processed while still holding a spent payload, or stripped without being
    /// marked, or stripped without saying why.
    ///
    /// The payload is an INPUT TO TRANSLATION: once this row has been turned into a domain fact it is
    /// never read again, and at ~1.8 kB a row it was 77% of the database. [`RowOutcome::Unmappable`] is
    /// the one case that keeps it, deliberately — that payload is the only evidence of why INSEE's
    /// record was unusable, and deleting it is how a systematic mapping bug hides (~5.4k rows
    /// historically).
    ///
    /// `synced_at` moves on the SAME condition as the payload drop — a fact was produced — which is what
    /// makes it mean something `processed_at` cannot. `processed_at` is a checkpoint the ingestion also
    /// advances on unchanged rows; this is a wall clock that only a real sync moves. An unmappable row
    /// keeps whatever `synced_at` it had, so "last synced 3 weeks ago, unmappable since yesterday" stays
    /// readable.
    async fn mark_processed(
        &self,
        siret: &str,
        last_seen_at: DateTime<Utc>,
        outcome: RowOutcome,
    ) -> Result<(), DomainError> {
        sqlx::query(
            "UPDATE external_sirene_restaurants \
                SET processed_at = $2, status = $3, \
                    payload = CASE WHEN $4 THEN NULL ELSE payload END, \
                    synced_at = CASE WHEN $5 THEN now() ELSE synced_at END, \
                    last_attempt_sync_at = now(), \
                    attempt_sync_retry_count = 0 \
              WHERE siret = $1",
        )
        .bind(siret)
        .bind(last_seen_at)
        .bind(outcome.status())
        .bind(outcome.clears_payload())
        .bind(outcome.stamps_synced_at())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    /// Record that the last attempt on this row FAILED, WITHOUT advancing the checkpoint — the row stays
    /// pending, keeps its payload (the retry needs something to translate) and is picked up next pass.
    ///
    /// Counting matters because of that: a row left pending on failure is indistinguishable from one not
    /// yet reached, so a permanently-broken record would retry forever, burn the sweep's budget and emit
    /// an error nobody acts on. After [`MAX_SYNC_ATTEMPTS`] consecutive failures the row becomes
    /// `POISON` and the drain stops selecting it. The count RESETS on any checkpointed outcome, so it
    /// means "consecutive failures since the last success" rather than a lifetime tally — which is what
    /// makes it answer "is this row stuck NOW?".
    ///
    /// Best-effort by design: this runs on a path that is ALREADY failing, so letting the annotation
    /// turn a retryable error into a hard one would be strictly worse than losing it.
    async fn mark_failed(&self, siret: &str) {
        // Referencing the column on the right-hand side of SET reads its OLD value, so `+ 1` is the new
        // count and the CASE decides on the same number that lands — one statement, no read-modify-write
        // race with a concurrent pass.
        let result = sqlx::query(
            "UPDATE external_sirene_restaurants \
                SET attempt_sync_retry_count = attempt_sync_retry_count + 1, \
                    last_attempt_sync_at = now(), \
                    status = CASE WHEN attempt_sync_retry_count + 1 >= $2 THEN 'POISON' ELSE 'FAILED' END \
              WHERE siret = $1 \
          RETURNING attempt_sync_retry_count, status",
        )
        .bind(siret)
        .bind(MAX_SYNC_ATTEMPTS)
        .fetch_optional(&self.pool)
        .await;
        match result {
            Ok(Some(row)) => {
                let count: i32 = row.try_get("attempt_sync_retry_count").unwrap_or_default();
                let status: String = row.try_get("status").unwrap_or_default();
                if status == "POISON" {
                    // Loud on the transition, because this is the moment the row stops being retried
                    // and starts needing a human (or a corrected record from INSEE).
                    tracing::error!(
                        worker = "sirene_sync",
                        %siret,
                        attempts = count,
                        "QUARANTINED as POISON after consecutive failed attempts -- skipped until \
                         INSEE sends a changed record. Payload retained as evidence."
                    );
                }
            }
            Ok(None) => {}
            Err(e) => {
                tracing::error!(worker = "sirene_sync", %siret, error = %e, "could not record the failed attempt");
            }
        }
    }

    /// Resolve `STAGED` rows against the aggregate's actual verdict.
    ///
    /// This closes the gap between "we handed the fact over" and "the domain accepted it". Since
    /// ADR-20260728-011344 the register path stages an inbound FACT rather than issuing a command, so at
    /// hand-over time this worker genuinely does not know what happened — `InboundEventsDrainWorker`
    /// delivers it and the AGGREGATE decides. Without this step the mirror would permanently claim a
    /// success it never observed.
    ///
    /// The join key is the one the ACL already writes: `inbound_messages.external_id = '{siret}:{hash}'`,
    /// which is exactly `siret || ':' || payload_hash` on the staging row. One statement, no per-row
    /// round-trip.
    ///
    /// Verdict mapping (`InboundMessageStatus`, stored as TEXT):
    /// - SUCCEEDED — the aggregate decided a fact and it was appended;
    /// - IGNORED — the aggregate decided nothing had changed;
    /// - DUPLICATE — this delivery had already been consumed.
    ///
    /// All three are SUCCESSFUL terminal outcomes: the record reached the domain and the domain is now
    /// correct about it. "Nothing changed" is a real answer, not a failure — conflating it with one is
    /// what made a sweep unable to distinguish 200,000 registrations from 200,000 no-ops. ANY other
    /// terminal verdict (FAILED today; REJECTED/CANCELLED should a recorded fact ever produce them)
    /// surfaces as FAILED and keeps the payload — success is enumerated, never inferred from
    /// "not FAILED". RECEIVED/SCHEDULED are still in flight and left alone.
    async fn reconcile_staged(&self, summary: &mut SireneSyncSummary) -> Result<(), DomainError> {
        let resolved = sqlx::query(
            // The payload is dropped HERE, in the same statement that records the verdict — never
            // before it. A FAILED verdict keeps the payload: that is the row that may need
            // re-translating, and it is the only original we hold.
            // DEFENSIVE verdict mapping: only the three SUCCESS verdicts resolve SYNCED and drop
            // the payload; anything else terminal (FAILED today; REJECTED/CANCELLED should they
            // ever reach a recorded fact) lands FAILED and KEEPS the payload — the only original
            // we hold, and the row that may need re-translating. An enumerated "not FAILED means
            // success" would silently assert a sync that never happened the day a new terminal
            // verdict appears (#272 review, 2026-08-01).
            "UPDATE external_sirene_restaurants s \
                SET status = CASE WHEN i.status IN ('SUCCEEDED', 'IGNORED', 'DUPLICATE') \
                                  THEN 'SYNCED' ELSE 'FAILED' END, \
                    synced_at = CASE WHEN i.status IN ('SUCCEEDED', 'IGNORED', 'DUPLICATE') \
                                     THEN COALESCE(i.completed_at, now()) ELSE s.synced_at END, \
                    payload = CASE WHEN i.status IN ('SUCCEEDED', 'IGNORED', 'DUPLICATE') \
                                   THEN NULL ELSE s.payload END \
               FROM inbound_messages i \
              WHERE s.status = 'STAGED' \
                AND i.source = $1 \
                AND i.external_id = s.siret || ':' || s.payload_hash \
                AND i.status NOT IN ('RECEIVED', 'SCHEDULED') \
          RETURNING s.status",
        )
        .bind(SIRENE_SOURCE)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;
        summary.resolved = resolved.len() as u64;
        Ok(())
    }

    /// Detect-by-absence (ADR-0045): SIRENE never sends deletes — the active-only ingestion means a
    /// closed établissement simply stops appearing. Rows unseen for [`ABSENCE_GRACE_DAYS`] past their
    /// OWN DEPARTMENT's latest sweep are treated as closures, with the same partner gate as the
    /// explicit signal. Already-INACTIVE prospects fold to a no-op, so re-scanning stale rows every
    /// pass is idempotent.
    async fn reconcile_absent(
        &self,
        store: &PgEventStore,
        restaurants: &PgRestaurantRepository,
        mailbox: &PgMailbox,
        correlation_id: uuid::Uuid,
        summary: &mut SireneSyncSummary,
    ) -> Result<(), DomainError> {
        // PER-DEPARTMENT, not global (#218). The sweep is budget-limited and resumes at the stalest
        // partition, so at any moment most departments were last swept days or weeks ago — that is
        // NORMAL, not evidence that their restaurants closed.
        //
        // The previous global form (`max(last_seen_at)` over the whole mirror, cutoff = latest − 21d)
        // was safe only because coverage never rotated: departments 38–101 had no rows and 01–37 were
        // re-swept every run. Under a rotating sweep it would close every establishment in every
        // department not reached in 21 days — entire regions dropped from discovery, from a job doing
        // exactly what it was designed to do.
        //
        // So absence is judged relative to a row's OWN department: the department must itself have been
        // swept recently enough to trust (`FRESH_INGESTION_DAYS`), and the row must have missed that
        // department's own latest sweep by the grace period. A department nobody has swept lately
        // simply yields no absence signal, which is the correct answer rather than a dangerous one.
        let absent: Vec<(String, DateTime<Utc>)> = sqlx::query_as(
            "WITH swept AS ( \
                 SELECT department, max(last_seen_at) AS swept_at \
                   FROM external_sirene_restaurants \
                  WHERE department IS NOT NULL \
                  GROUP BY department \
             ) \
             SELECT s.siret, s.last_seen_at \
               FROM external_sirene_restaurants s \
               JOIN swept d ON d.department = s.department \
              WHERE s.etat <> 'F' \
                AND now() - d.swept_at <= make_interval(days => $1) \
                AND s.last_seen_at < d.swept_at - make_interval(days => $2) \
              ORDER BY s.siret",
        )
        .bind(FRESH_INGESTION_DAYS as i32)
        .bind(ABSENCE_GRACE_DAYS as i32)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        for (siret, last_seen_at) in absent {
            if let Err(e) = self
                .close_if_prospect(
                    store,
                    restaurants,
                    mailbox,
                    correlation_id,
                    &siret,
                    last_seen_at,
                    "SIRENE: establishment disappeared from the active registry (detect-by-absence)",
                    summary,
                )
                .await
            {
                summary.failed += 1;
                tracing::error!(worker = "sirene_sync", %siret, error = %e, "absence close failed");
            }
        }
        Ok(())
    }

    /// The gated close (ADR-0045 deletion policy): rehydrate the aggregate; nothing registered or
    /// already INACTIVE ⇒ no-op; NON_PARTNER prospect ⇒ `MarkRestaurantClosed` through the journaled
    /// WORKER dispatch + the ordinary handler; PASSIVE/ACTIVE partner ⇒ never auto-close — flag for
    /// manual review.
    #[allow(clippy::too_many_arguments)]
    async fn close_if_prospect(
        &self,
        store: &PgEventStore,
        restaurants: &PgRestaurantRepository,
        mailbox: &PgMailbox,
        correlation_id: uuid::Uuid,
        siret: &str,
        last_seen_at: DateTime<Utc>,
        reason: &str,
        summary: &mut SireneSyncSummary,
    ) -> Result<(), DomainError> {
        // KEPT deliberately, unlike the register path (ADR-20260728-011344). This is the legacy-id
        // adoption: listings registered before the UUIDv5(SIRET) derivation live under other aggregate
        // ids, and the projection row carrying this SIRET is the only thing that names them. Deriving
        // the id instead would silently fail to close them — a closed restaurant left orderable is a
        // worse outcome than a scan.
        //
        // The cost argument that killed this call on the register path does not apply here: that ran
        // once per staged SIRET (~200k a sweep, quadratic against the projection). This runs only for
        // rows ABSENT for 21+ days, behind a freshness guard — a handful per sweep, bounded by actual
        // closures. If it ever stops being bounded, the fix is a GIN index on
        // `Restaurant.external_identifiers` (there is none today), not another derivation.
        let restaurant_id = match restaurants.by_external_identifier("siret", siret).await? {
            Some(row) => row.restaurant_id,
            None => restaurant_id_for_siret(siret),
        };
        let (state, _version) =
            Repository::new(store).load::<RestaurantState>(restaurant_id).await?;
        let Some(state) = state else {
            return Ok(()); // never registered — nothing to close
        };
        if state.status == RestaurantStatus::INACTIVE {
            return Ok(()); // already closed/deactivated — idempotent no-op
        }
        match state.listing_status {
            RestaurantListingStatus::NON_PARTNER => {
                let cmd = MarkRestaurantClosed { restaurant_id, reason: Some(reason.to_string()) };
                let payload = serialize_command("MarkRestaurantClosed", &cmd)?;
                // The deterministic identity is unchanged (the compaction parity below pins it);
                // only the DOOR moved: fire-and-forget enqueue, the mailbox worker delivers
                // (ADR-20260731-122500). `closed` now counts HAND-OFFS; the delivered outcome
                // lives on the mailbox row and the supervision lanes.
                let entry = journal_entry(
                    "MarkRestaurantClosed",
                    siret,
                    last_seen_at,
                    correlation_id,
                    payload,
                );
                let actor = send_actor(&entry);
                match enqueue_worker_command(mailbox, entry.message_id, "MarkRestaurantClosed", entry.payload, &actor).await? {
                    EnqueueOutcome::Enqueued => {
                        summary.closed += 1;
                        tracing::info!(worker = "sirene_sync", %siret, prospect = %state.display_name.0, "close signal handed to the mailbox");
                    }
                    // This exact closure signal (SIRET + staged version) was already consumed —
                    // e.g. closed, then manually reactivated: a spent signal never re-closes.
                    EnqueueOutcome::Deduplicated(status) => {
                        tracing::info!(
                            worker = "sirene_sync",
                            %siret,
                            mailbox = ?status,
                            "close signal already on the mailbox -- not re-sent (a spent signal never re-closes)"
                        );
                    }
                    EnqueueOutcome::PayloadConflict(status) => {
                        tracing::warn!(
                            worker = "sirene_sync",
                            %siret,
                            mailbox = ?status,
                            "close payload conflict at the same staged version -- not re-sent"
                        );
                    }
                }
            }
            RestaurantListingStatus::PASSIVE_PARTNER | RestaurantListingStatus::ACTIVE_PARTNER => {
                // A live partner is NEVER auto-closed on a registry signal (bad SIRENE datum, SIRET
                // change on relocation…) — surfaced for a human every time the signal re-pends.
                summary.flagged_for_review += 1;
                tracing::warn!(
                    worker = "sirene_sync",
                    %siret,
                    partner = %state.display_name.0,
                    listing_status = ?state.listing_status,
                    "MANUAL REVIEW -- closure signal for a live PARTNER; not auto-closing"
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The compaction's journal arm (#238) re-derives, in the `sirene_ingest` crate, the deterministic
    /// `command_journal.message_id` this worker seeds — same namespace, same seed shape. The two crates
    /// cannot share the code (ADR-0045: the CI ingestion sees no domain layers), so this pins them
    /// together: if either derivation moves, the journal evidence silently stops matching and the
    /// historical payloads stop being reclaimable — a drift this test turns into a loud failure.
    #[test]
    fn journal_message_id_parity_with_the_compaction() {
        let last_seen_at = chrono::DateTime::parse_from_rfc3339("2026-07-20T12:00:00.123456+00:00")
            .expect("fixed timestamp")
            .with_timezone(&Utc);
        for command_type in ["RegisterRestaurant", "MarkRestaurantClosed"] {
            let entry = journal_entry(
                command_type,
                "85242109900021",
                last_seen_at,
                uuid::Uuid::nil(),
                serde_json::json!({}),
            );
            assert_eq!(
                entry.message_id,
                sirene_ingest::journal_message_id(command_type, "85242109900021", last_seen_at),
                "the {command_type} message_id derivations drifted apart"
            );
        }
    }
}
