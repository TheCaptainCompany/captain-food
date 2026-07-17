//! App-layer projection runtime (ADR-0040): the worker that folds `domain_events` into the materialized
//! read-model tables using the hand-written `…Compute` projectors, checkpointed per stream-prefix group
//! in `projection_checkpoint`. The registry covers the Restaurant stream (the `restaurant` +
//! `prospectionpipeline` read models), the Catalog stream (`catalog`), the Cart stream (`cart`) and the
//! Order stream (`ordertracking`).

pub mod worker;

pub use worker::ProjectionWorker;

/// Live health snapshot of the projection worker, exposed by the server's `/projector` endpoint.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ProjectionStatus {
    /// Whether the polling loop is running.
    pub running: bool,
    /// The log position every registry group has drained up to after the last successful tick (each
    /// group's own conservative checkpoint row lives in `projection_checkpoint`).
    pub checkpoint: i64,
    /// Highest `domain_events.position` seen at the last tick.
    pub head: i64,
    /// `head - checkpoint`: how many log positions the read models are behind (0 after a successful
    /// full drain; > 0 only transiently or when a tick errors).
    pub lag: i64,
    /// When the worker last completed a tick (successful or not).
    pub last_tick_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The last tick's error, cleared on the next successful tick.
    pub last_error: Option<String>,
}

impl Default for ProjectionStatus {
    fn default() -> Self {
        Self { running: false, checkpoint: 0, head: 0, lag: 0, last_tick_at: None, last_error: None }
    }
}
