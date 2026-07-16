//! Captain.Food server binary (ADR-0035): bind `$PORT`, serve the Axum router, drain on SIGTERM.
//!
//! Render (ADR-0042) injects `$PORT` and sends SIGTERM on deploy/scale-down; honouring it gives the
//! graceful-drain half of the reinterpreted health/probe contract (ADR-0042, was P-04).

#[tokio::main]
async fn main() {
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("0.0.0.0:{port}");

    // Free tier has no Pre-Deploy step, so apply migrations at startup (ADR-0043). Serve regardless of the
    // outcome — /health reports the true schema state, so a failed migration is held back by Render's health
    // check rather than crash-looping the process.
    if let Err(e) = server::run_migrations_if_enabled().await {
        eprintln!("startup migrations FAILED (serving anyway; /health reports the schema state): {e}");
    }

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
