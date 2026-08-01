//! The mailbox row as the runtime sees it, and the handler contract a host product implements.

use sqlx::postgres::PgRow;
use sqlx::{Postgres, Row, Transaction};

/// One `inbound_messages` row picked for delivery. The BUSINESS payload is opaque JSON here — the
/// host's [`MessageHandler`] owns its vocabulary; the runtime owns only the envelope columns.
#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub message_id: uuid::Uuid,
    /// The consumption/checkpoint axis — never `None` for a delivered row (SCHEDULED rows have no
    /// position until promotion, and the drain only selects RECEIVED).
    pub position: i64,
    /// COMMAND | EVENT | MESSAGE — the request/report split as a column.
    pub kind: String,
    pub actor_type: String,
    pub actor_id: uuid::Uuid,
    pub partition: i16,
    /// The host's message vocabulary key (e.g. `PostMessage`, `PaymentCaptured`).
    pub message_type: String,
    pub payload: serde_json::Value,
    pub payload_hash: String,
    pub channel: String,
    pub user_id: Option<uuid::Uuid>,
    pub user_type: String,
    pub correlation_id: uuid::Uuid,
    pub cause_id: Option<uuid::Uuid>,
    pub session_id: Option<uuid::Uuid>,
    pub received_at: chrono::DateTime<chrono::Utc>,
}

impl InboundMessage {
    pub(crate) fn decode(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            message_id: row.try_get("message_id")?,
            position: row.try_get("position")?,
            kind: row.try_get("kind")?,
            actor_type: row.try_get("actor_type")?,
            actor_id: row.try_get("actor_id")?,
            partition: row.try_get("partition")?,
            message_type: row.try_get("message_type")?,
            payload: row.try_get("payload")?,
            payload_hash: row.try_get("payload_hash")?,
            channel: row.try_get("channel")?,
            user_id: row.try_get("user_id")?,
            user_type: row.try_get("user_type")?,
            correlation_id: row.try_get("correlation_id")?,
            cause_id: row.try_get("cause_id")?,
            session_id: row.try_get("session_id")?,
            received_at: row.try_get("received_at")?,
        })
    }
}

/// Output of the PREPARE phase, opaque to the runtime (ADR-20260801-023000): whatever the host
/// computed with NO transaction open — validation outcomes, priced totals, an external gateway's
/// response — handed to [`MessageHandler::handle`] to commit. `None` = the handler declared no
/// prepare work for this message (the default).
pub type Prepared = Option<Box<dyn std::any::Any + Send>>;

/// The handler's verdict on one delivery — the RECEIVED row's terminal transition
/// (`SCHEDULED -> (CANCELLED | RECEIVED); RECEIVED -> SUCCEEDED | REJECTED | FAILED | IGNORED |
/// DUPLICATE`). `Rejected`/`Failed` carry the `{ code, context }` error surfaced to the status
/// query; `Ignored` = a no-change decision; `Duplicate` = a redelivered fact the aggregate's own
/// dedupe recognized.
#[derive(Debug, Clone, PartialEq)]
pub enum HandlerVerdict {
    Succeeded,
    Rejected(serde_json::Value),
    Failed(serde_json::Value),
    Ignored,
    Duplicate,
}

impl HandlerVerdict {
    /// The `inbound_messages.status` TEXT value this verdict commits.
    pub fn status(&self) -> &'static str {
        match self {
            HandlerVerdict::Succeeded => "SUCCEEDED",
            HandlerVerdict::Rejected(_) => "REJECTED",
            HandlerVerdict::Failed(_) => "FAILED",
            HandlerVerdict::Ignored => "IGNORED",
            HandlerVerdict::Duplicate => "DUPLICATE",
        }
    }

    /// The error json committed alongside (REJECTED/FAILED only).
    pub fn error(&self) -> Option<&serde_json::Value> {
        match self {
            HandlerVerdict::Rejected(e) | HandlerVerdict::Failed(e) => Some(e),
            _ => None,
        }
    }
}

/// The host product's delivery logic — its command dispatch / event application, whatever its
/// domain is. THE FOUR-EFFECT CONTRACT (PROP-20260728-152752 §3.4): every write the handler makes
/// MUST go through the transaction it is handed — the runtime commits the handler's effects, the
/// row's terminal flip and the checkpoint advance ATOMICALLY under the partition's
/// `ownership_version` fence, so a fenced-out delivery rolls back WHOLE (handler effects
/// included). A handler that writes through its own connection instead breaks exactly the
/// guarantee this crate exists to give.
///
/// A returned `Err` is an infrastructure failure (DB unreachable mid-handling): the delivery
/// aborts, the row stays RECEIVED, and redelivery is the retry.
/// One handled delivery: the verdict to commit, plus an optional action to run strictly AFTER the
/// commit succeeds (in-process fan-out of the effects that just became durable — an event-bus
/// publish, a cache invalidation). A fenced-out or rolled-back delivery never runs it.
pub struct Delivery {
    pub verdict: HandlerVerdict,
    pub post_commit: Option<Box<dyn FnOnce() + Send>>,
}

impl Delivery {
    pub fn of(verdict: HandlerVerdict) -> Self {
        Self { verdict, post_commit: None }
    }

    pub fn then(verdict: HandlerVerdict, post_commit: impl FnOnce() + Send + 'static) -> Self {
        Self { verdict, post_commit: Some(Box::new(post_commit)) }
    }
}

#[async_trait::async_trait]
pub trait MessageHandler: Send + Sync {
    /// The PREPARE phase (ADR-20260801-023000): runs BEFORE the fenced completion transaction
    /// opens — the one place a delivery may do slow or external work (pool reads, an outbound
    /// HTTP call) without holding a transaction across it. Whatever it returns is handed to
    /// [`Self::handle`] verbatim.
    ///
    /// Retry contract: an `Err` aborts the delivery — the row stays RECEIVED and redelivery
    /// re-runs prepare from scratch, so any external call made here MUST be idempotent under
    /// re-execution (e.g. a deterministic gateway idempotency key). A crash between prepare and
    /// the commit has exactly the same shape: nothing was written, redelivery re-prepares.
    /// Deterministic business rejections belong in the PREPARED VALUE (committed as a REJECTED
    /// verdict by `handle`), never in `Err` — `Err` means "retry", and a rejection retried
    /// forever is a stuck lane.
    ///
    /// Default: no prepare work (`Ok(None)`) — existing handlers run unchanged.
    async fn prepare(&self, _message: &InboundMessage) -> Result<Prepared, sqlx::Error> {
        Ok(None)
    }

    async fn handle(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        message: &InboundMessage,
        prepared: Prepared,
    ) -> Result<Delivery, sqlx::Error>;
}

/// Post-COMMIT notification of one delivery — the seam a host hangs its status bus / subscription
/// fan-out on (PROP-20260728-152752: "the status bus publishes AFTER the commit — subscribers only
/// ever hear about durable facts"). Called once per committed delivery, never for a fenced-out or
/// rolled-back one. Must be cheap and non-blocking: it runs on the drain path.
pub trait DeliveryObserver: Send + Sync {
    fn committed(&self, message: &InboundMessage, verdict: &HandlerVerdict);
}
