//! Process-manager (saga) runtime — the I/O half of `application::process_managers` (actors.yaml
//! `type: process-manager`), mirroring the projection worker's shape (ADR-0040): a REGISTRY of process
//! managers, each with its OWN `projection_checkpoint` row (`pm:<Name>` keys), drained independently
//! every tick over the trigger event types its actors.yaml inbox declares.

pub mod runner;

pub use runner::ProcessManagerRunner;

/// Live health snapshot of the saga runner, exposed by the server's `/saga` endpoint (the
/// process-manager counterpart of `/projector`'s `ProjectionStatus`).
#[derive(Clone, Debug, serde::Serialize)]
pub struct ProcessManagerStatus {
    /// Whether the polling loop is running.
    pub running: bool,
    /// The log position every registered process manager has drained up to after the last successful
    /// tick (each PM's own conservative checkpoint row lives in `projection_checkpoint`).
    pub checkpoint: i64,
    /// Highest `domain_events.position` seen at the last tick.
    pub head: i64,
    /// `head - checkpoint`: how many log positions the sagas are behind.
    pub lag: i64,
    /// When the runner last completed a tick (successful or not).
    pub last_tick_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The last tick's error, cleared on the next successful tick.
    pub last_error: Option<String>,
}

impl Default for ProcessManagerStatus {
    fn default() -> Self {
        Self { running: false, checkpoint: 0, head: 0, lag: 0, last_tick_at: None, last_error: None }
    }
}
