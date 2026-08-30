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
use actor_client::status_bus::OperationStatusBus;

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


#[cfg(test)]
mod tests {
    use super::is_money_lane;

    /// The backfill of un-reacted Stripe facts must fire for EXACTLY the three money lanes and no
    /// other -- a lane wrongly included wastes a scan, a lane wrongly excluded leaves a paid order
    /// with no deliverer (the #272 review MAJOR this predicate came from).
    #[test]
    fn the_money_lanes_are_the_payment_aggregate_and_its_two_process_managers() {
        assert!(is_money_lane("Payment"));
        assert!(is_money_lane("PlaceOrderProcess"));
        assert!(is_money_lane("RefundProcess"));
        assert!(!is_money_lane("Conversation"));
        assert!(!is_money_lane("Cart"));
    }
}

/// Fully Pg-backed [`CommandDeps`] for a process hosting mailbox workers OUTSIDE the monolith
/// composition root — a standalone adapter, or a per-actor bin (#385). External services resolve
/// exactly like the monolith's: ENV-GATED with the fail-closed stand-in as the default —
/// identity is the real Supabase ACL when `SUPABASE_URL` + `SUPABASE_PUBLISHABLE_KEY` are set
/// (else auth-dependent deliveries decline), sessions are the encrypted Pg store when
/// `AUTH_SESSION_KEY` is set (else the no-op). `payments` stays injected because only the caller
/// knows whether its deployment carries Stripe credentials AND only bins whose spec declares the
/// Read one boolean gate from the environment, with the spec key's own default when unset.
///
/// The standalone fleet has no generated `Config` (that lives in `crates/server`), so each gate is
/// read here by name. Kept as one function so every gate parses the same tokens — a gate that only
/// understands `true` while its neighbour also understands `1` is a rollback that silently does
/// nothing.
fn env_flag(key: &str, default: bool) -> bool {
    std::env::var(key)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on"))
        .unwrap_or(default)
}

/// `payment` port link the Stripe adapter (the adapter crate sits above this one).
pub fn standalone_deps(pool: &PgPool, payments: Arc<dyn PaymentService>) -> CommandDeps {
    let auth: Arc<dyn IdentityService> = match crate::SupabaseIdentityService::from_env() {
        Some(adapter) => {
            tracing::info!(binding = "identity", impl_ = "SupabaseIdentityService", "standalone deps: identity service wired (SUPABASE_URL set)");
            Arc::new(adapter)
        }
        None => {
            tracing::warn!(binding = "identity", impl_ = "FailClosedIdentityService", "standalone deps: SUPABASE_URL/PUBLISHABLE_KEY unset -- identity-dependent deliveries decline");
            Arc::new(crate::FailClosedIdentityService)
        }
    };
    let sessions: Arc<dyn application::auth_sessions::AuthSessionStore> =
        match crate::PgAuthSessionStore::from_env(pool.clone()) {
            Some(store) => {
                tracing::info!(binding = "auth_sessions", impl_ = "encrypted Pg store", "standalone deps: auth session store wired (AUTH_SESSION_KEY set)");
                Arc::new(store)
            }
            None => {
                tracing::warn!(binding = "auth_sessions", "standalone deps: AUTH_SESSION_KEY unset -- session-cookie deliveries are no-ops");
                Arc::new(application::auth_sessions::NoopAuthSessionStore)
            }
        };
    let deps = CommandDeps {
        store: Arc::new(crate::persistence::PgEventStore::new(pool.clone())),
        restaurants: Arc::new(crate::persistence::PgRestaurantRepository::new(pool.clone())),
        slugs: Arc::new(crate::PgSlugReservationRepository::new(pool.clone())),
        ownership: Arc::new(crate::FailClosedGoogleOwnershipVerifier),
        probe: Arc::new(crate::UnverifiedGbpOrderLinkProbe),
        prospection: Arc::new(crate::persistence::PgProspectionRepository::new(pool.clone())),
        catalogs: Arc::new(crate::persistence::PgCatalogRepository::new(pool.clone())),
        auth,
        customers: Arc::new(crate::persistence::PgCustomerRepository::new(pool.clone())),
        sessions,
        payments,
        pm_state: Arc::new(crate::persistence::PgPaymentProcessState::new(pool.clone())),
        refund_state: Arc::new(crate::persistence::PgRefundProcessState::new(pool.clone())),
        mailbox_requeue: Arc::new(crate::persistence::mailbox_lanes::PgMailboxRequeue::new(
            pool.clone(),
        )),
        // RSO-1 Phase 4: the PlaceOrder service-hours enforcement gate — same ENV-GATED posture
        // as the rest of this fn (this crate cannot read the generated Config; the bins' baked
        // profiles surface the key as an env var), same parse as the generated reader's booleans,
        // same default (OFF = shadow) as the spec.
        enforce_service_hours_guard: std::env::var("ENFORCE_SERVICE_HOURS_GUARD")
            .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on"))
            .unwrap_or(false),
        // #167: the acceptance-timeout ACTION gate — same ENV-GATED posture, same boolean
        // parse, same default (OFF = shadow) as the spec.
        enforce_acceptance_timeout: std::env::var("ENFORCE_ACCEPTANCE_TIMEOUT")
            .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on"))
            .unwrap_or(false),
        // #588 (ADR-20260816-040239): route the Order birth through the Order's own lane — same
        // ENV-GATED posture, same boolean parse, same default (ON = the routed birth; the default
        // was flipped by the founder, ADR-20260830-012200) as the spec. This fallback and the
        // generated Config's are the SAME default on purpose — a divergence here is the
        // split-clock hazard below, minted at compile time.
        //
        // A standalone worker fleet and the monolith MUST read the same value, and the hazard is
        // SPLIT-CLOCK, not double-birth (#598, vernon — the earlier wording here said "would
        // birth some orders twice" and was WRONG). Double-birth is unreachable: both routes
        // converge on `Order-{id}` with an expected-version precondition at 0 (OFF gates on
        // `should_deliver_order_placed` and saves at the loaded version; ON stages a door row and
        // `record_inbound_order_placed` returns `AlreadyRecorded` if any `OrderPlaced` is already
        // on the stream), the door row's primary key dedups, and the trigger skips on redelivery.
        // Four absorbers.
        //
        // What a split fleet DOES do: under OFF, `apply_schedules_in_tx` runs against the
        // *PlaceOrderProcess* message (handler.rs, the saga leg), so the Order's own `OrderPlaced`
        // receive schedules never apply; ON arms them (the PM-fact route's `Recorded` arm). So the
        // order is born exactly ONCE either way, and the ACCEPTANCE DEADLINE is a coin-flip, per
        // order, INVISIBLY — invisible because `order_birth_lag_ms` only records on the routed
        // path. Bounded by the rolling-deploy window; `runtime_flag_state` below is what makes it
        // observable while that window is open.
        //
        // #797: one field per DECLARED route, each read from its OWN key. A standalone worker
        // fleet hosts only the OrderPlaced route today (the reclamation seam runs on the polling
        // runner), but the replacement gate is read from its own key here too rather than pinned
        // to a literal — a route whose value is decided by which composition root happens to
        // construct it is a route with no single rollback lever.
        route_gates: application::generated::process_managers::RouteGates {
            order_placed_to_order: env_flag("ROUTE_ORDER_BIRTH_THROUGH_LANE", true),
            place_replacement_order_to_order: env_flag(
                "ROUTE_REPLACEMENT_BIRTH_THROUGH_LANE",
                false,
            ),
        },
    };
    // Deploy-time fleet-parity EVIDENCE (#598): re-assert this process's resolved value for every
    // gate whose split across a fleet has a consequence. Declared HERE, at the standalone
    // composition root, next to where the value is decided — the monolith declares the same three
    // from its Config, so `count(distinct value) by (flag) > 1` is the parity query.
    telemetry::meters::runtime::declare_flag(
        "ENFORCE_SERVICE_HOURS_GUARD",
        deps.enforce_service_hours_guard,
    );
    telemetry::meters::runtime::declare_flag(
        "ENFORCE_ACCEPTANCE_TIMEOUT",
        deps.enforce_acceptance_timeout,
    );
    telemetry::meters::runtime::declare_flag(
        "ROUTE_ORDER_BIRTH_THROUGH_LANE",
        deps.route_gates.order_placed_to_order,
    );
    deps
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
    reminder_windows: std::collections::HashMap<&'static str, std::time::Duration>,
) {
    spawn_standalone_workers_with(pool, adapter, actor_types, payments, nudges, reminder_windows, None)
}

/// [`spawn_standalone_workers`] with an explicit [`actor_runtime::WorkerConfig`] — the per-actor
/// bins (#385) build it from their GENERATED configuration (the declared `MAILBOX_*` keys),
/// exactly like the monolith composition root does, so cadence/lease/poison-cap tuning reaches a
/// bin the same way it reaches the monolith. `None` keeps the adapter behaviour: spec defaults
/// with the `MAILBOX_MAX_DELIVERY_ATTEMPTS` env override (an adapter binary has no Config reader).
pub fn spawn_standalone_workers_with(
    pool: PgPool,
    adapter: &'static str,
    actor_types: &'static [&'static str],
    payments: Arc<dyn PaymentService>,
    nudges: Arc<MailboxNudges>,
    reminder_windows: std::collections::HashMap<&'static str, std::time::Duration>,
    worker_config: Option<actor_runtime::WorkerConfig>,
) {
    // The whole fleet spawn rides ONE task so the caller's startup is never blocked by the
    // per-lane seeding and the backfill scan.
    tokio::spawn(async move {
        run_standalone_workers(pool, adapter, actor_types, payments, nudges, reminder_windows, worker_config)
            .await;
    });
}

/// The lanes that carry money: the Payment aggregate that records Stripe facts and the two process
/// managers that react to them. Only these need the startup backfill of un-reacted facts.
fn is_money_lane(actor_type: &str) -> bool {
    matches!(actor_type, "Payment" | "PlaceOrderProcess" | "RefundProcess")
}

/// Any lane with declared `schedules:` needs its window; fall back to the SPEC DEFAULT when the
/// caller wired none (the monolith reads Config; an adapter fleet has no Config reader) — a
/// missing window would otherwise abort every delivery on that lane while its heartbeat keeps
/// the lease: a permanent head-of-line wedge, not a retry. A window the caller DID wire is never
/// overwritten. Extracted from `run_standalone_workers` so the fallback has its own unit test
/// (#167 Phase 0 — it had none while the day-granular table sat underneath it).
fn seed_spec_default_windows(
    adapter: &str,
    actor_types: &[&'static str],
    reminder_windows: &mut std::collections::HashMap<&'static str, std::time::Duration>,
) {
    for schedule in application::generated::reminders::REMINDER_SCHEDULES {
        if actor_types.contains(&schedule.actor_type) {
            reminder_windows.entry(schedule.after_key).or_insert_with(|| {
                tracing::info!(
                    adapter,
                    key = schedule.after_key,
                    window = ?schedule.after_default,
                    "standalone mailbox: reminder window from spec default"
                );
                schedule.after_default
            });
        }
    }
}

#[cfg(test)]
mod seed_tests {
    use super::seed_spec_default_windows;

    /// The pre-existing gap the #167 widening touches: an adapter fleet hosting a `schedules:`
    /// lane with NO wired window gets the SPEC DEFAULT (typed Duration), a wired window is never
    /// overwritten, and a fleet without that lane seeds nothing.
    #[test]
    fn spec_default_fallback_fills_only_missing_windows_of_hosted_lanes() {
        let spec = application::generated::reminders::REMINDER_SCHEDULES
            .first()
            .expect("the catalog declares at least one schedule");

        // Missing window on a hosted lane → seeded with the spec default.
        let mut windows = std::collections::HashMap::new();
        seed_spec_default_windows("test", &[spec.actor_type], &mut windows);
        assert_eq!(windows.get(spec.after_key), Some(&spec.after_default));

        // A wired window wins over the default (the monolith's Config value must survive).
        let wired = std::time::Duration::from_secs(60);
        let mut windows = std::collections::HashMap::from([(spec.after_key, wired)]);
        seed_spec_default_windows("test", &[spec.actor_type], &mut windows);
        assert_eq!(windows.get(spec.after_key), Some(&wired), "never overwrite a wired window");

        // A fleet that does not host the lane seeds nothing for it.
        let mut windows = std::collections::HashMap::new();
        seed_spec_default_windows("test", &["NoSuchLane"], &mut windows);
        assert!(windows.is_empty());
    }
}

async fn run_standalone_workers(
    pool: PgPool,
    adapter: &'static str,
    actor_types: &'static [&'static str],
    payments: Arc<dyn PaymentService>,
    nudges: Arc<MailboxNudges>,
    mut reminder_windows: std::collections::HashMap<&'static str, std::time::Duration>,
    worker_config: Option<actor_runtime::WorkerConfig>,
) {
    // No posture read since #242 Runtime D: the PM_MAILBOX_DELIVERY gate retired with
    // `command_journal`, so a fleet spawns exactly the lane set it was handed. There is no value
    // a peer could hold differently, hence nothing to fail closed against.
    let actor_types: Vec<&'static str> = actor_types.to_vec();
    seed_spec_default_windows(adapter, &actor_types, &mut reminder_windows);
    let deps = standalone_deps(&pool, payments);
    // Parity with the monolith (#272 review MAJOR): a fleet that runs the PM lanes must also run
    // the startup backfill, or a fact the retired saga runner accepted but never reacted to stays
    // unreacted when THIS process is the one delivering.
    if actor_types.iter().any(|a| is_money_lane(a)) {
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
    // #167: a fleet hosting a `schedules:` lane also runs the promotion dead-man's switch —
    // the SCHEDULED backlog is per-database, so whichever process drains the lane watches it
    // (monitor outside the worker, ADR-20260810-231300 monitoring carve-out).
    if actor_types.iter().any(|a| {
        application::generated::reminders::REMINDER_SCHEDULES.iter().any(|s| s.actor_type == *a)
    }) {
        super::spawn_promotion_watch(pool.clone(), std::time::Duration::from_secs(30));
    }
    // #598: the ROUTED-BIRTH lane dead-man's switch, for a fleet that hosts a routed lane. Same
    // monitor-outside-the-worker posture as the promotion watch, and UNCONDITIONAL on
    // ROUTE_ORDER_BIRTH_THROUGH_LANE — a liveness signal that starts with the flip has never been
    // seen to work at the moment it is first needed.
    if actor_types.iter().any(|a| super::declared_lanes().contains(a)) {
        super::spawn_order_lane_watch(pool.clone(), std::time::Duration::from_secs(30));
    }
    // #608: THE BIRTH-GAP dead-man's switch, for a fleet that hosts a MONEY lane — whichever
    // process delivers the saga's Stripe facts is the one that must notice when it stops. Same
    // monitor-outside-the-worker posture as the two watches above; the sweep reads database-wide
    // state, so it is gated on hosting the lane, never on being the monolith.
    if actor_types.iter().any(|a| is_money_lane(a)) {
        super::spawn_birth_gap_watch(pool.clone(), std::time::Duration::from_secs(30));
    }
    let handler = Arc::new(
        super::MailboxCommandHandler::new(deps)
            .with_reminder_windows(reminder_windows)
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
        .map(|v| match v.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => true,
            "false" | "0" | "no" | "off" => false,
            other => {
                tracing::warn!(adapter, value = other, "RUN_MAILBOX_PUSH unparsable -- using the default (true)");
                true
            }
        })
        .unwrap_or(true)
    {
        match std::env::var("DATABASE_URL") {
            Ok(db_url) => {
                let push = crate::persistence::mailbox_wake::MailboxPush::new();
                crate::persistence::mailbox_wake::spawn_mailbox_listener(
                    db_url,
                    pool.clone(),
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
    let worker_config = worker_config.unwrap_or_else(|| actor_runtime::WorkerConfig {
        max_delivery_attempts: std::env::var("MAILBOX_MAX_DELIVERY_ATTEMPTS")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .map(|v| v.clamp(0, i16::MAX as i64) as i16)
            .unwrap_or(actor_runtime::WorkerConfig::default().max_delivery_attempts),
        ..actor_runtime::WorkerConfig::default()
    });
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
