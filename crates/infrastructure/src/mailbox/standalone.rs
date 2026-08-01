//! STANDALONE adapter workers (#272 D3, deferred from the #270 review: "standalone adapter
//! binaries also dropped their drain loops — with the monolith down they ACK 200 while facts
//! pile up RECEIVED with no consumer"). A webhook adapter deployed as its own web service
//! (ADR-20260718-213352) can now run MailboxWorkers for exactly the lanes its ingestor feeds,
//! gated by `RUN_MAILBOX_WORKERS` (configuration.yaml, default false — gate-then-stabilize).
//!
//! **Why the gate defaults OFF**: the operation-status bus and the GraphQL subscription event
//! bus are in-process. When BOTH the monolith and an adapter run workers, they lease-compete,
//! and a fact delivered by the adapter process never reaches the monolith's subscribers —
//! `paymentStatusChanged` and `operationStatusChanged` pushes go dark for exactly those
//! deliveries (polls still work: `operationStatus` reads the mailbox row). ON is therefore an
//! explicit deployment posture — an adapter that must survive monolith downtime, accepting
//! degraded push — not a default. Cross-process fan-out (LISTEN/NOTIFY) is the recorded
//! follow-up that would dissolve the trade-off.
//!
//! The delivery semantics are IDENTICAL to the monolith's fleet: same generated router, same
//! staging flush, same fenced completion — the lease/fence machinery makes a competing fleet
//! safe by construction, which is the whole point of the runtime.

use std::sync::Arc;

use application::generated::services::{IdentityService, PaymentService};
use sqlx::PgPool;

use crate::generated::command_router::{CommandDeps, ACTOR_MAILBOXES};
use crate::persistence::mailbox_store::MailboxNudges;
use crate::persistence::status_bus::OperationStatusBus;

/// The `RUN_MAILBOX_WORKERS` gate, read the same way the generated config reads its booleans.
pub fn standalone_workers_enabled() -> bool {
    std::env::var("RUN_MAILBOX_WORKERS")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on"))
        .unwrap_or(false)
}

/// The `PM_MAILBOX_DELIVERY` gate (Runtime D1) — the standalone fleet must chain PM copies of
/// recorded Stripe facts exactly like the monolith's, or a flip would behave differently
/// depending on WHICH process won the Payment lane's lease.
fn pm_mailbox_delivery() -> bool {
    std::env::var("PM_MAILBOX_DELIVERY")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on"))
        .unwrap_or(false)
}

/// Fully Pg-backed [`CommandDeps`] for a standalone adapter process. External services the
/// adapter's lanes never exercise are fail-closed stand-ins; `payments` is injected because only
/// the caller knows whether its deployment carries Stripe credentials (the Stripe adapter does,
/// a delivery adapter does not).
pub fn standalone_deps(pool: &PgPool, payments: Arc<dyn PaymentService>) -> CommandDeps {
    CommandDeps {
        store: Arc::new(crate::persistence::PgEventStore::new(pool.clone())),
        restaurants: Arc::new(crate::persistence::PgRestaurantRepository::new(pool.clone())),
        slugs: Arc::new(crate::PgSlugReservationRepository::new(pool.clone())),
        ownership: Arc::new(crate::FailClosedGoogleOwnershipVerifier),
        probe: Arc::new(crate::UnverifiedGbpOrderLinkProbe),
        prospection: Arc::new(crate::persistence::PgProspectionRepository::new(pool.clone())),
        catalogs: Arc::new(crate::persistence::PgCatalogRepository::new(pool.clone())),
        auth: Arc::new(crate::FailClosedIdentityService) as Arc<dyn IdentityService>,
        customers: Arc::new(crate::persistence::PgCustomerRepository::new(pool.clone())),
        sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
        payments,
        pm_state: Arc::new(crate::persistence::PgPaymentProcessState::new(pool.clone())),
        refund_state: Arc::new(crate::persistence::PgRefundProcessState::new(pool.clone())),
    }
}

/// Spawn the supervised worker fleet for `actor_types` — the standalone mirror of the monolith's
/// composition-root loop: seed, run, respawn on error/panic with backoff, graceful drain on
/// SIGTERM/ctrl-c (lanes released so a peer takes over immediately). Returns after SPAWNING; the
/// workers live on the runtime.
///
/// `reminder_windows` mirrors the monolith handler's wiring; adapters pass the windows for the
/// lanes they run (empty when none of those lanes declares `schedules:` — a delivery that then
/// needs one aborts loudly for retry rather than silently mis-scheduling).
pub fn spawn_standalone_workers(
    pool: PgPool,
    adapter: &'static str,
    actor_types: &'static [&'static str],
    payments: Arc<dyn PaymentService>,
    nudges: Arc<MailboxNudges>,
    reminder_windows: std::collections::HashMap<&'static str, i64>,
) {
    let deps = standalone_deps(&pool, payments);
    let handler = Arc::new(
        super::MailboxCommandHandler::new(deps)
            .with_reminder_windows(reminder_windows)
            .with_pm_fact_chaining(pm_mailbox_delivery())
            .with_nudges(nudges.clone()),
    );
    // A local bus with no subscribers: publishes vanish, but the observer's
    // `command_completion_ms` emission stays live for COMMAND lanes run here.
    let observer = Arc::new(super::StatusBusObserver::new(OperationStatusBus::default()));
    let worker_id = format!(
        "{}-{}-{}",
        adapter,
        std::process::id(),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        let ctrl_c = async {
            let _ = tokio::signal::ctrl_c().await;
        };
        #[cfg(unix)]
        let terminate = async {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut sig) => {
                    sig.recv().await;
                }
                Err(_) => std::future::pending().await,
            }
        };
        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();
        tokio::select! {
            _ = ctrl_c => {}
            _ = terminate => {}
        }
        tracing::info!(adapter, "standalone mailbox: shutdown signal -- draining workers");
        let _ = shutdown_tx.send(true);
        // Hold the sender until the process ends: a dropped sender cannot deliver a shutdown
        // (PR #270 review C1 — `changed()` resolving Err must mean no-signal, never a wake).
        std::future::pending::<()>().await;
    });

    for actor_type in actor_types {
        let Some((_, width)) = ACTOR_MAILBOXES.iter().find(|(a, _)| a == actor_type) else {
            tracing::error!(adapter, actor_type, "standalone mailbox: not a mailbox actor -- worker not started");
            continue;
        };
        let worker = Arc::new({
            let mut w = actor_runtime::MailboxWorker::new(
                pool.clone(),
                worker_id.clone(),
                *actor_type,
                actor_runtime::WorkerConfig::default(),
                handler.clone(),
            )
            .with_observer(observer.clone());
            if let Some(nudge) = nudges.get(actor_type) {
                w = w.with_nudge(nudge);
            }
            w
        });
        let width = *width as i16;
        let rx = shutdown_rx.clone();
        tokio::spawn(async move {
            if let Err(e) = worker.seed(width).await {
                tracing::error!(worker = %worker.worker_id, actor_type = %worker.actor_type, error = %e, "standalone mailbox: seed failed -- worker not started");
                return;
            }
            loop {
                let run = {
                    let w = worker.clone();
                    let rx = rx.clone();
                    tokio::spawn(async move { w.run(rx).await })
                };
                match run.await {
                    Ok(Ok(())) => break, // graceful shutdown
                    Ok(Err(e)) => {
                        tracing::error!(worker = %worker.worker_id, actor_type = %worker.actor_type, error = %e, "standalone mailbox: worker loop exited -- respawning");
                    }
                    Err(join_err) => {
                        tracing::error!(worker = %worker.worker_id, actor_type = %worker.actor_type, error = %join_err, "standalone mailbox: worker loop panicked -- respawning");
                    }
                }
                if *rx.borrow() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        });
    }
    tracing::info!(
        adapter,
        workers = actor_types.len(),
        "standalone mailbox: adapter-owned workers running (RUN_MAILBOX_WORKERS)"
    );
}
