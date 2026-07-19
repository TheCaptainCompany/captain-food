//! The on-app SIRENE translation worker (ADR-0045) — the domain half of the split sync, mirroring the
//! projection worker's drain/checkpoint pattern (ADR-0040).
//!
//! The thin CI ingestion (`sirene_ingest` bin) UPSERTs raw INSEE records into the
//! `external_sirene_restaurants` staging table; THIS worker — running on the deployed server, i.e.
//! versioned exactly like the in-process projector — drains the pending rows, runs the SIRENE
//! Anti-Corruption Layer ([`super::sirene::etablissement_to_command`]) and calls the ordinary command
//! handlers (`register_restaurant`, `mark_restaurant_closed`), so ALL domain logic executes on the
//! deployed version (the version-skew hazard of the retired direct-write CI binary is gone).
//!
//! **Checkpoint** = the per-row `processed_at` high-water mark: `processed_at IS NULL OR
//! processed_at < last_seen_at ⇒ pending`. Marking a drained row `processed_at = last_seen_at` (the
//! value READ, not `now()`) makes a concurrent ingestion bump re-pend the row instead of being lost.
//!
//! **Deletion reconciliation** (ADR-0045 deletion policy) reuses the already-modeled
//! `MarkRestaurantClosed` → `RestaurantMarkedClosed`:
//! - explicit signal: a staged row whose `etat` is `F` (fermé) is a confident closure;
//! - detect-by-absence: rows not refreshed for [`ABSENCE_GRACE_DAYS`] past the LATEST ingestion
//!   (≈ 3 missed weekly runs — the debounce), guarded by a freshness check so a stalled CI can never
//!   mass-close anything;
//! - gate by the partnership funnel: only **NON_PARTNER** prospects are auto-closed; PASSIVE/ACTIVE
//!   partners are flagged for manual review (a registry datum must never take down a live partner);
//! - never hard-delete: closure is an event, the projection folds it, the staging mirror keeps the row.
//!
//! Idempotent by construction: `restaurantId = UUIDv5(SIRET)` absorbs re-registrations, and a close is
//! only issued while the aggregate folds to a non-INACTIVE status (an already-closed prospect is a
//! no-op). A SIRET that REAPPEARS active after a closure re-pends its row; the register replay is
//! absorbed and the restaurant stays INACTIVE — reactivation is deliberately manual (logged).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use application::commands::{mark_restaurant_closed, register_restaurant};
use application::ports::Actor;
use application::repository::Repository;
use domain::restaurant::RestaurantState;
use chrono::{DateTime, Utc};
use domain::generated::commands::MarkRestaurantClosed;
use domain::generated::scalars::{RestaurantListingStatus, RestaurantStatus};
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};

use crate::integrations::sirene::{
    etablissement_to_command, restaurant_id_for_siret, sirene_system_user_id, Etablissement,
};
use crate::persistence::db_err;
use crate::{PgEventStore, PgRestaurantRepository};

/// Safety-net poll interval. The PRIMARY trigger is the ingestion's ping on
/// `POST /internal/sirene/drain`; the loop only catches missed pings, so it can be slow.
const POLL_INTERVAL: Duration = Duration::from_secs(3600);
/// Pending rows are drained in keyset batches (ordered by SIRET) of this size.
const BATCH_SIZE: i64 = 200;
/// Detect-by-absence debounce: a row must be unseen for this long PAST the latest ingestion before a
/// closure is inferred (≈ 3 missed weekly runs, per ADR-0045).
const ABSENCE_GRACE_DAYS: i64 = 21;
/// Absence is only meaningful against a FRESH mirror: skip the absence pass entirely unless the
/// latest ingestion ran within this window (a stalled CI must never look like mass closures).
const FRESH_INGESTION_DAYS: i64 = 10;
/// `UserType::EXTERNAL` ordinal for the event envelope (enums stored as declaration-order ints,
/// ADR-0037/0041) — the sync writes as the fixed external system principal.
const EXTERNAL_USER_TYPE: i32 = 6;

/// What one drain pass did — logged by the loop and by the ping-triggered run.
#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SireneSyncSummary {
    /// Pending staging rows drained (marked processed or left for retry).
    pub processed: u64,
    /// `RegisterRestaurant` issued — new prospects AND idempotent replays of known SIRETs.
    pub registered: u64,
    /// `MarkRestaurantClosed` issued (NON_PARTNER prospects only).
    pub closed: u64,
    /// Closure signals on PASSIVE/ACTIVE partners — NOT auto-closed, raised for manual review.
    pub flagged_for_review: u64,
    /// Unmappable rows (bad SIRET, no name/address, unparsable payload) — skipped, marked processed.
    pub skipped: u64,
    /// Write/load failures — the row stays pending and is retried on the next run.
    pub failed: u64,
}

/// The worker: owns the pool, guards against overlapping drains (the ping endpoint and the poll loop
/// share one instance behind an `Arc`).
pub struct SireneSyncWorker {
    pool: PgPool,
    draining: AtomicBool,
}

impl SireneSyncWorker {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, draining: AtomicBool::new(false) }
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
        result
    }

    /// Poll forever as a safety net behind the ping trigger: `run_once` then sleep [`POLL_INTERVAL`].
    /// Takes `Arc<Self>` (unlike the projection worker's consuming loop) so the same instance stays
    /// shared with the `/internal/sirene/drain` endpoint.
    pub async fn run_loop(self: Arc<Self>) {
        loop {
            match self.run_once().await {
                Ok(summary) if summary.processed > 0 || summary.closed > 0 => {
                    println!("sirene sync worker: {summary:?}");
                }
                Ok(_) => {} // nothing pending — stay quiet
                Err(e) => eprintln!("sirene sync worker: {e}"),
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
        // Fixed system principal (ADR-0041); fresh correlation id per pass so all events of one drain
        // are traceable together.
        let actor = Actor {
            user_id: sirene_system_user_id(),
            user_type: EXTERNAL_USER_TYPE,
            correlation_id: uuid::Uuid::new_v4(),
            cause_id: None,
        };

        // 1) Pending rows, keyset-paginated by SIRET: a row left pending after a write failure is
        //    behind the cursor, so one pass always terminates (it is retried on the NEXT pass).
        let mut after = String::new();
        loop {
            let rows = sqlx::query(
                "SELECT siret, payload, etat, last_seen_at FROM external_sirene_restaurants \
                 WHERE siret > $1 AND (processed_at IS NULL OR processed_at < last_seen_at) \
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
                let payload: serde_json::Value = row.try_get("payload").map_err(db_err)?;
                let etat: String = row.try_get("etat").map_err(db_err)?;
                let last_seen_at: DateTime<Utc> = row.try_get("last_seen_at").map_err(db_err)?;
                after = siret.clone();
                self.process_row(&store, &restaurants, &actor, &siret, payload, &etat, last_seen_at, &mut summary)
                    .await?;
            }
        }

        // 2) Deletion reconciliation by absence (debounced, freshness-guarded, partner-gated).
        self.reconcile_absent(&store, &actor, &mut summary).await?;

        Ok(summary)
    }

    /// Translate one pending staging row. Only mark-processed/SELECT failures propagate; ACL and
    /// write-path failures are counted (`skipped`/`failed`) so the pass keeps going.
    #[allow(clippy::too_many_arguments)]
    async fn process_row(
        &self,
        store: &PgEventStore,
        restaurants: &PgRestaurantRepository,
        actor: &Actor,
        siret: &str,
        payload: serde_json::Value,
        etat: &str,
        last_seen_at: DateTime<Utc>,
        summary: &mut SireneSyncSummary,
    ) -> Result<(), DomainError> {
        summary.processed += 1;

        let etablissement: Etablissement = match serde_json::from_value(payload) {
            Ok(e) => e,
            Err(e) => {
                // A poison payload will not fix itself on retry — skip until the ingestion
                // refreshes it (which re-pends the row).
                summary.skipped += 1;
                eprintln!("sirene sync worker: siret {siret}: unparsable staged payload: {e}");
                return self.mark_processed(siret, last_seen_at).await;
            }
        };

        // Explicit closure signal — the staging `etat` column (stamped at ingestion) or the payload's
        // own current period (`F` fermé / `C` cessation).
        let explicitly_closed = etat == "F" || matches!(etablissement.etat(), Some("F") | Some("C"));
        if explicitly_closed {
            match self
                .close_if_prospect(store, actor, siret, "SIRENE: establishment administratively closed (etat=F)", summary)
                .await
            {
                Ok(()) => return self.mark_processed(siret, last_seen_at).await,
                Err(e) => {
                    summary.failed += 1; // left pending → retried next pass
                    eprintln!("sirene sync worker: close failed for siret {siret}: {e}");
                    return Ok(());
                }
            }
        }

        // Active record → ACL → the ordinary registration write path (idempotent by UUIDv5 id).
        match etablissement_to_command(&etablissement) {
            Ok(cmd) => match register_restaurant(store, restaurants, cmd, actor).await {
                Ok(()) => {
                    summary.registered += 1;
                    self.mark_processed(siret, last_seen_at).await
                }
                Err(e) => {
                    summary.failed += 1; // left pending → retried next pass
                    eprintln!("sirene sync worker: register failed for siret {siret}: {e}");
                    Ok(())
                }
            },
            Err(e) => {
                // Unusable record (redacted, nameless, no address…) — log + mark processed; the next
                // ingestion refresh re-pends it if INSEE's data improves.
                summary.skipped += 1;
                eprintln!("sirene sync worker: skipped: {e}");
                self.mark_processed(siret, last_seen_at).await
            }
        }
    }

    /// Advance the per-row checkpoint to the `last_seen_at` we READ (not `now()`): if an ingestion
    /// bumped the row meanwhile, `processed_at < last_seen_at` still holds and it is re-drained.
    async fn mark_processed(&self, siret: &str, last_seen_at: DateTime<Utc>) -> Result<(), DomainError> {
        sqlx::query("UPDATE external_sirene_restaurants SET processed_at = $2 WHERE siret = $1")
            .bind(siret)
            .bind(last_seen_at)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// Detect-by-absence (ADR-0045): SIRENE never sends deletes — the active-only ingestion means a
    /// closed établissement simply stops appearing. Rows unseen for [`ABSENCE_GRACE_DAYS`] past the
    /// latest ingestion are treated as closures, with the same partner gate as the explicit signal.
    /// Already-INACTIVE prospects fold to a no-op, so re-scanning stale rows every pass is idempotent.
    async fn reconcile_absent(
        &self,
        store: &PgEventStore,
        actor: &Actor,
        summary: &mut SireneSyncSummary,
    ) -> Result<(), DomainError> {
        let latest: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT max(last_seen_at) FROM external_sirene_restaurants")
                .fetch_one(&self.pool)
                .await
                .map_err(db_err)?;
        let Some(latest) = latest else {
            return Ok(()); // empty mirror — nothing ingested yet
        };
        if Utc::now() - latest > chrono::Duration::days(FRESH_INGESTION_DAYS) {
            // The mirror itself is stale (CI not running) — absence means nothing; never mass-close.
            return Ok(());
        }
        let cutoff = latest - chrono::Duration::days(ABSENCE_GRACE_DAYS);
        let absent: Vec<String> = sqlx::query_scalar(
            "SELECT siret FROM external_sirene_restaurants \
             WHERE etat <> 'F' AND last_seen_at < $1 ORDER BY siret",
        )
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        for siret in absent {
            if let Err(e) = self
                .close_if_prospect(
                    store,
                    actor,
                    &siret,
                    "SIRENE: establishment disappeared from the active registry (detect-by-absence)",
                    summary,
                )
                .await
            {
                summary.failed += 1;
                eprintln!("sirene sync worker: absence close failed for siret {siret}: {e}");
            }
        }
        Ok(())
    }

    /// The gated close (ADR-0045 deletion policy): rehydrate the aggregate; nothing registered or
    /// already INACTIVE ⇒ no-op; NON_PARTNER prospect ⇒ `MarkRestaurantClosed` through the ordinary
    /// handler; PASSIVE/ACTIVE partner ⇒ never auto-close — flag for manual review.
    async fn close_if_prospect(
        &self,
        store: &PgEventStore,
        actor: &Actor,
        siret: &str,
        reason: &str,
        summary: &mut SireneSyncSummary,
    ) -> Result<(), DomainError> {
        let restaurant_id = restaurant_id_for_siret(siret);
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
                mark_restaurant_closed(
                    store,
                    MarkRestaurantClosed { restaurant_id, reason: Some(reason.to_string()) },
                    actor,
                )
                .await?;
                summary.closed += 1;
                println!("sirene sync worker: closed prospect {} (siret {siret})", state.display_name.0);
            }
            RestaurantListingStatus::PASSIVE_PARTNER | RestaurantListingStatus::ACTIVE_PARTNER => {
                // A live partner is NEVER auto-closed on a registry signal (bad SIRENE datum, SIRET
                // change on relocation…) — surfaced for a human every time the signal re-pends.
                summary.flagged_for_review += 1;
                eprintln!(
                    "sirene sync worker: MANUAL REVIEW — closure signal for partner '{}' \
                     (siret {siret}, listing {:?}); not auto-closing",
                    state.display_name.0, state.listing_status
                );
            }
        }
        Ok(())
    }
}
