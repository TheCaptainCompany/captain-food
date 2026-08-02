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

/// Resolves on SIGTERM/ctrl-c — the adapter mains hand this to axum's `with_graceful_shutdown`.
/// Installing the fleet's own signal task REPLACES the default SIGTERM disposition for the whole
/// process, so without this the HTTP server would keep serving after the workers drained and
/// every deploy would eat the full kill grace period (#272 review, 2026-08-01).
pub async fn shutdown_signal() {
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
}

/// The `RUN_MAILBOX_WORKERS` gate, read the same way the generated config reads its booleans.
pub fn standalone_workers_enabled() -> bool {
    std::env::var("RUN_MAILBOX_WORKERS")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on"))
        .unwrap_or(false)
}

/// The `PM_MAILBOX_DELIVERY` posture (Runtime D1), EXPLICIT presence and all: the standalone
/// fleet must chain PM copies of recorded Stripe facts exactly like the monolith's, or a flip
/// behaves differently depending on WHICH process won the Payment lane's lease — a capture the
/// adapter records without chaining is a saga hop nobody reacts to until a monolith restart
/// (#272 review MAJOR, 2026-08-01). `None` = the key is UNSET in this environment, which for a
/// money lane is an unprovable posture, not a default.
fn pm_gate_posture() -> Option<bool> {
    std::env::var("PM_MAILBOX_DELIVERY")
        .ok()
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on"))
}

/// The lanes whose behavior depends on the PM gate posture — recording Stripe facts (chaining)
/// and reacting to them (the PM event legs).
fn is_money_lane(actor_type: &str) -> bool {
    matches!(actor_type, "Payment" | "PlaceOrderProcess" | "RefundProcess")
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
    mut reminder_windows: std::collections::HashMap<&'static str, i64>,
) {
    // MONEY-LANE POSTURE CHECK: a fleet whose PM_MAILBOX_DELIVERY is merely UNSET must not
    // lease-compete on the Payment/PM lanes — an implicit `false` against a monolith running
    // `true` records captures without chaining, and the hop is lost until a monolith restart.
    // The operator states the posture explicitly (matching the monolith's) or those lanes stay
    // with the monolith. An explicitly WRONG value is still an operator error — recorded in the
    // RUN_MAILBOX_WORKERS gate prose; a DB-persisted posture is the follow-up that removes it.
    let posture = pm_gate_posture();
    let actor_types: Vec<&'static str> = actor_types
        .iter()
        .copied()
        .filter(|a| {
            if is_money_lane(a) && posture.is_none() {
                tracing::error!(
                    adapter,
                    actor_type = a,
                    "standalone mailbox: PM_MAILBOX_DELIVERY is UNSET -- refusing this money lane (set it explicitly, matching the monolith)"
                );
                false
            } else {
                true
            }
        })
        .collect();
    // Any lane with declared `schedules:` needs its window; fall back to the SPEC DEFAULT when
    // the caller wired none (the monolith reads Config; an adapter fleet has no Config reader) —
    // a missing window would otherwise abort every delivery on that lane while its heartbeat
    // keeps the lease: a permanent head-of-line wedge, not a retry.
    for schedule in application::generated::reminders::REMINDER_SCHEDULES {
        if actor_types.contains(&schedule.actor_type) {
            reminder_windows.entry(schedule.after_days_key).or_insert_with(|| {
                tracing::info!(
                    adapter,
                    key = schedule.after_days_key,
                    days = schedule.after_default_days,
                    "standalone mailbox: reminder window from spec default"
                );
                schedule.after_default_days
            });
        }
    }
    let deps = standalone_deps(&pool, payments);
    // Gate-ON parity with the monolith (#272 review MAJOR): a fleet that runs the PM lanes must
    // also run the startup backfill, or a fact the retired saga runner accepted but never
    // reacted to stays unreacted when THIS process is the one delivering.
    if posture == Some(true) && actor_types.iter().any(|a| is_money_lane(a)) {
        let backfill_pool = pool.clone();
        let pm_state = deps.pm_state.clone();
        tokio::spawn(async move {
            match super::backfill_stripe_facts_to_pm_lanes(&backfill_pool, pm_state.as_ref()).await
            {
                Ok(n) if n > 0 => {
                    tracing::info!(enqueued = n, "standalone mailbox: PM backfill enqueued un-reacted Stripe facts");
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::error!(error = %e, "standalone mailbox: PM backfill failed -- un-reacted facts wait for the next restart");
                }
            }
        });
    }
    let handler = Arc::new(
        super::MailboxCommandHandler::new(deps)
            .with_reminder_windows(reminder_windows)
            .with_pm_fact_chaining(posture.unwrap_or(false))
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

    // MAILBOX PUSH (#313): the standalone fleet listens exactly like the monolith's — an
    // adapter fleet that only wakes on ITS OWN inserts would still poll for hops chained by
    // other processes. Env-read (an adapter binary has no Config reader), spec defaults.
    let push = if std::env::var("RUN_MAILBOX_PUSH")
        .map(|v| !matches!(v.trim().to_ascii_lowercase().as_str(), "false" | "0" | "no" | "off"))
        .unwrap_or(true)
    {
        match std::env::var("DATABASE_URL") {
            Ok(db_url) => {
                let push = crate::persistence::mailbox_wake::MailboxPush::new();
                crate::persistence::mailbox_wake::spawn_mailbox_listener(
                    db_url,
                    nudges.clone(),
                    push.clone(),
                );
                Some(push)
            }
            Err(_) => {
                tracing::warn!(adapter, "standalone mailbox: DATABASE_URL unset -- push listener not started");
                None
            }
        }
    } else {
        tracing::warn!(adapter, toggle = "RUN_MAILBOX_PUSH", "mailbox push OFF -- workers poll at the heartbeat cadence");
        None
    };
    let worker_config = actor_runtime::WorkerConfig {
        max_delivery_attempts: std::env::var("MAILBOX_MAX_DELIVERY_ATTEMPTS")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .map(|v| v.clamp(0, i16::MAX as i64) as i16)
            .unwrap_or(actor_runtime::WorkerConfig::default().max_delivery_attempts),
        ..actor_runtime::WorkerConfig::default()
    };
    let worker_count = actor_types.len();
    for actor_type in actor_types {
        let Some((_, width)) = ACTOR_MAILBOXES.iter().find(|(a, _)| *a == actor_type) else {
            tracing::error!(adapter, actor_type, "standalone mailbox: not a mailbox actor -- worker not started");
            continue;
        };
        let worker = Arc::new({
            let mut w = actor_runtime::MailboxWorker::new(
                pool.clone(),
                worker_id.clone(),
                actor_type,
                worker_config.clone(),
                handler.clone(),
            )
            .with_observer(observer.clone());
            if let Some(nudge) = nudges.get(actor_type) {
                w = w.with_nudge(nudge);
            }
            if let Some(push) = &push {
                w = w.with_push_live(push.live_flag());
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
        workers = worker_count,
        "standalone mailbox: adapter-owned workers running (RUN_MAILBOX_WORKERS)"
    );
}
