//! Captain.Food server binary (ADR-0035): bind `$PORT`, serve the Axum router, drain on SIGTERM.
//!
//! Migrations are applied out-of-band by **sqlx-cli in CI** (ADR-0043) — the server never runs them; it
//! only checks the schema version via `/health`. Render (ADR-0042) injects `$PORT` and sends SIGTERM on
//! deploy/scale-down; honouring it gives the graceful-drain half of the health/probe contract.

#[tokio::main]
async fn main() {
    // Print the build identity FIRST — before any fallible startup (router build, port bind, DB probe) — so
    // a boot that panics or never binds still names its version in the logs, exactly the case where /health
    // never comes up and cannot help (ADR-20260721-175411). The deployed image tag (`sha-<commit>`, pinned
    // by the deploy hook) is the platform-side source of truth for a container that never execs at all.
    //
    // This one line stays a raw `println!` on purpose, and it is the ONLY one left in the server: the
    // tracing subscriber cannot exist yet, because installing it needs LOG_LEVEL out of a configuration
    // that has not been read. Making this structured would mean deferring it until after config
    // resolution, which is exactly the guarantee ADR-20260721-175411 asked for us not to give up.
    println!("captain-food server starting — version {}", server::build_version());

    // Configuration gate (PROP-20260729-004500, issue #246) — BEFORE the router, the pool or the port.
    //
    // Missing configuration cannot self-heal, so the app refuses to start; an unavailable DEPENDENCY
    // can, so that case still starts and reports 503 at /health (ADR-0043). On Render an exiting
    // container FAILS THE DEPLOY and the previous version keeps serving — a misconfigured build cannot
    // replace a working one, which is strictly safer than booting into silent degradation.
    //
    // Every missing key is reported at once: one deploy fixes them all, rather than one per cycle.
    let (config, problems) = server::generated::config::Config::resolve();

    // Telemetry comes up NEXT — after config (it needs LOG_LEVEL and the ingest key) but before the
    // router, the pool and every worker, so their startup lines are structured and correlated rather
    // than being the one part of the boot nobody can query (issue #191).
    //
    // No telemetry key is `required:`, so this cannot contribute a configuration problem: at worst it
    // degrades to logs-only and says so. The guard is bound for the whole of `main` — dropping it early
    // would shut the exporter down and silently lose every buffered span.
    let mut telemetry_guard = server::init_telemetry(&config);

    // #639 part C step 6-v (ADR-20260905-223957 §3): the ONE-SHOT `bootstrap-platform-admin`
    // subcommand — checked BEFORE the missing-config gate below (its own precondition, the
    // secret, is deliberately `required: []`, never part of the ordinary boot's required set) and
    // BEFORE the router/pool/port, exactly like the config gate itself. Runs inside this SAME
    // OTLP-wired process (observability CATCH: never a bare script outside telemetry) and exits
    // without ever binding a port — this is an operator-invoked command, not a server boot.
    if std::env::args().nth(1).as_deref() == Some("bootstrap-platform-admin") {
        let code = server::bootstrap_platform_admin::run(&config).await;
        telemetry_guard.shutdown();
        std::process::exit(code);
    }

    if !problems.is_empty() {
        let report =
            server::generated::config::MissingConfig { profile: config.profile, problems };
        // Deliberately one multi-line event rather than a line per problem: the whole point of the
        // report is that an operator reads every broken key at once and fixes them in ONE deploy.
        tracing::error!(profile = %config.profile, "{report}");
        if config.must_stop_on_problems() {
            // Flush before exiting, or the error that explains the failed deploy never leaves the box.
            telemetry_guard.shutdown();
            // 78 = EX_CONFIG (sysexits.h): a configuration error, not a crash.
            std::process::exit(78);
        }
        tracing::warn!(
            profile = %config.profile,
            "starting anyway -- production and staging STOP here"
        );
    }
    tracing::info!("{}", config.boot_report());

    let addr = format!("0.0.0.0:{}", config.port);

    let app = server::router().await;

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    tracing::info!(addr = %addr, version = server::build_version(), "captain-food server listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");

    // Render sends SIGTERM on every deploy and scale-down, so this is the NORMAL exit path, not an
    // exceptional one. Flushing here is what makes the shutdown window observable — spans describing a
    // drain that went wrong are exactly the ones a buffered exporter would otherwise discard.
    telemetry_guard.shutdown();
}

/// Resolve on Ctrl-C or SIGTERM (Render sends SIGTERM) so in-flight requests can drain.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        signal(SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received -- draining");
}
