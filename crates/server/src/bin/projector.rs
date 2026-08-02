//! Standalone projection worker (ADR-0040/0043) — the graduation path off the in-process worker.
//!
//! `--once` drains pending events and exits (free GitHub Actions cron / CI); `--loop` (default) polls
//! forever (a paid Render Background Worker). The web service runs the worker in-process today
//! (RUN_PROJECTOR); set RUN_PROJECTOR=false there once this runs separately.

use infrastructure::ProjectionWorker;

#[tokio::main]
async fn main() {
    // This binary has its OWN composition root, so it installs its own subscriber. Without this the
    // worker's `tracing` calls would compile, run, and go nowhere -- a silent worker looks like an idle
    // one, which is precisely how a paused pipeline stayed invisible for four hours (issue #220).
    let (config, _problems) = server::generated::config::Config::resolve();
    let mut telemetry_guard = server::init_telemetry(&config);

    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(15))
        .connect(&url)
        .await
        .expect("connect to DATABASE_URL");

    let worker = ProjectionWorker::new(pool);
    if std::env::args().any(|a| a == "--once") {
        match worker.run_once().await {
            Ok(()) => tracing::info!(worker = "projection", mode = "once", "drained pending events"),
            Err(e) => {
                tracing::error!(worker = "projection", mode = "once", error = %e, "drain failed");
                // Flush BEFORE exiting. `process::exit` runs no destructors, so without this the
                // error that explains a failed CI drain is buffered in the exporter and discarded --
                // the one log line anyone would go looking for.
                telemetry_guard.shutdown();
                std::process::exit(1);
            }
        }
        telemetry_guard.shutdown();
    } else {
        // Push wake (`infrastructure::persistence::event_wake`): the point of doing this at the
        // database rather than over the in-process EventBus is precisely this binary — a projector
        // in its OWN process still hears every append, which an in-process fan-out could never do.
        // The loop keeps its safety-net drain and reverts to fast polling if the listener drops.
        let wake = infrastructure::EventWake::new();
        infrastructure::spawn_event_listener(url.clone(), wake.clone());
        tracing::info!(worker = "projection", mode = "loop", "push-driven loop started -- Ctrl-C to stop");
        worker.run_loop_with(Some(wake.waiter())).await;
        telemetry_guard.shutdown();
    }
}
