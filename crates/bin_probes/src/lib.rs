//! The probe + telemetry FLOOR every bin stands on (#385, ADR-20260807-183024).
//!
//! Factored out of `bin_runtime` for the API-tier wiring: the pure families (gateway-{role},
//! fo-*/bo-* surfaces) must serve the probe contract their generated Deployments declare and emit
//! telemetry WITHOUT linking `infrastructure` — a gateway that can transitively spell a domain
//! type is a boundary violation (D8: no DB, no business logic, no state). `bin_runtime` re-exports
//! everything here, so the CQRS-spine mains (PR #387) keep their `bin_runtime::…` paths unchanged.
//!
//! This crate deliberately has NO database, NO domain and NO application dependency. Adding one is
//! a review flag, not a convenience.

use std::sync::Arc;

/// The precise build identity, for diagnostics (ADR-20260721-175411): CI bakes
/// `CAPTAIN_BUILD_VERSION` (short git SHA) into the image; `dev-…` covers uncontainerized runs.
/// Read once and cached — the value never changes for a process.
pub fn build_version() -> &'static str {
    static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSION.get_or_init(|| {
        std::env::var("CAPTAIN_BUILD_VERSION")
            .unwrap_or_else(|_| format!("dev-{}", env!("CARGO_PKG_VERSION")))
    })
}

/// What telemetry needs from a bin's GENERATED Config — the same translation the monolith's
/// `server::init_telemetry` performs, kept here as plain owned data because every bin's `Config`
/// is its own generated type (scope-filtered keys, #374 Q4) and this crate must not know any of
/// them.
pub struct TelemetrySettings {
    pub api_key: Option<String>,
    pub endpoint: String,
    pub dataset: String,
    /// Parsed leniently to 1.0 (keep everything): a malformed ratio was already reported as a
    /// configuration problem by the generated reader; degrading to ZERO here would turn one bad
    /// value into total silent trace loss (the failure telemetry exists to end).
    pub sample_ratio: String,
    pub log_level: String,
    pub profile: String,
}

/// Install telemetry for a bin process. The guard must be held for the process lifetime and
/// `shutdown()` on exit — dropping it early silently loses every buffered span.
pub fn init_telemetry(settings: TelemetrySettings) -> telemetry::TelemetryGuard {
    let cfg = telemetry::TelemetryConfig {
        api_key: settings.api_key,
        endpoint: settings.endpoint,
        dataset: settings.dataset,
        sample_ratio: settings.sample_ratio.parse().unwrap_or(1.0),
        log_level: settings.log_level,
        service_version: build_version().to_string(),
        profile: settings.profile,
    };
    let (guard, _emission) = telemetry::init(&cfg);
    guard
}

/// One JSON health snapshot: `(ready, detail)`. `ready=false` renders 503 — the readiness probe
/// holds the pod out until the runtime it hosts is actually running.
pub type HealthFn = Arc<dyn Fn() -> (bool, serde_json::Value) + Send + Sync>;

/// Serve the probe contract the generated Deployment declares (`/health` + `/ping`), draining on
/// SIGTERM/ctrl-c, then return (the caller flushes telemetry). `wired:true` — this is the probe
/// half of a BUSINESS runtime, not the step-4 shell.
pub async fn serve_probes(bin: &'static str, family: &'static str, port: &str, health: HealthFn) {
    serve_with_app(bin, family, port, health, axum::Router::new()).await
}

/// Serve the probe contract PLUS a bin's own routes on the same `$PORT` (the API-tier families:
/// a subgraph's `/{role}/graphql`, a gateway's proxy, a surface's assets + SSR fallback). The
/// probe routes are merged LAST so `/health` + `/ping` always win over an app fallback — the
/// same precedence the monolith router gives its explicit routes.
pub async fn serve_with_app(
    bin: &'static str,
    family: &'static str,
    port: &str,
    health: HealthFn,
    app: axum::Router,
) {
    let handler = move || {
        let health = health.clone();
        async move {
            let (ready, detail) = health();
            let mut body = serde_json::json!({
                "bin": bin,
                "family": family,
                "version": build_version(),
                "wired": true,
            });
            if let (Some(obj), Some(extra)) = (body.as_object_mut(), detail.as_object()) {
                for (k, v) in extra {
                    obj.insert(k.clone(), v.clone());
                }
            }
            let code = if ready {
                axum::http::StatusCode::OK
            } else {
                axum::http::StatusCode::SERVICE_UNAVAILABLE
            };
            (
                code,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                body.to_string(),
            )
        }
    };
    let app = app
        .route("/health", axum::routing::get(handler))
        .route("/ping", axum::routing::get(|| async { "ok" }));
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("{bin}: failed to bind {addr}: {e}"));
    tracing::info!(bin, addr = %addr, version = build_version(), "listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(bin))
        .await
        .expect("server error");
    tracing::info!(bin, "drained -- bye");
}

/// Resolve on Ctrl-C or SIGTERM (Kubernetes sends SIGTERM on every Recreate rollout) so in-flight
/// requests drain instead of RSTing mid-deploy. The mailbox fleet installs its own watcher for
/// the same signals (lane release), so both drains run concurrently.
async fn shutdown_signal(bin: &'static str) {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("install Ctrl-C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        signal(SignalKind::terminate()).expect("install SIGTERM handler").recv().await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!(bin, "shutdown signal received -- draining");
}
