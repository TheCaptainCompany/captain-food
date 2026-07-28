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
    println!("captain-food server starting — version {}", server::build_version());

    // Configuration gate (PROP-20260729-004500, issue #246) — BEFORE the router, the pool or the port.
    //
    // Missing configuration cannot self-heal, so the app refuses to start; an unavailable DEPENDENCY
    // can, so that case still starts and reports 503 at /health (ADR-0043). On Render an exiting
    // container FAILS THE DEPLOY and the previous version keeps serving — a misconfigured build cannot
    // replace a working one, which is strictly safer than booting into silent degradation.
    //
    // Every missing key is reported at once: one deploy fixes them all, rather than one per cycle.
    let (config, missing) = server::generated::config::Config::resolve();
    if !missing.is_empty() {
        let report =
            server::generated::config::MissingConfig { profile: config.profile, missing };
        eprintln!("{report}");
        if config.config_enforce {
            // 78 = EX_CONFIG (sysexits.h): a configuration error, not a crash.
            std::process::exit(78);
        }
        eprintln!(
            "\nCONFIG_ENFORCE=false — starting anyway (warn-only rollout). Set it true to enforce."
        );
    }
    print!("{}", config.boot_report());

    let addr = format!("0.0.0.0:{}", config.port);

    let app = server::router();

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    println!("captain-food server listening on {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
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
    println!("shutdown signal received — draining");
}
