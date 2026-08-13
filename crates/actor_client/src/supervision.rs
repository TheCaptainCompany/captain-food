//! The SUPERVISION READ surface over the mailbox (#510, the last piece of the "one journal, one
//! door" directive): the `MailboxLaneRepository` query port behind the ADMIN `mailboxLanes` /
//! `poisonedMailboxMessages` screens, moved here from `application::queries` so both of its
//! methods can demand the same [`MailboxAccess`] witness every other `inbound_messages` read
//! pays (#304). Holding the port is no longer holding the door: an `Arc<dyn
//! MailboxLaneRepository>` holder outside this crate cannot spell a call — the two minting door
//! functions below are the whole sanctioned read path, exactly like `operationStatus` reads
//! through [`crate::ActorClient::get_operation_status`].
//!
//! The WRITE half of the supervision pair (`MailboxRequeue`) deliberately did NOT move: its one
//! legitimate caller is `application::commands::requeue_mailbox_message`, which sits BELOW this
//! crate on the dependency arrow, so it seals in place with its own `application`-minted witness.

use async_trait::async_trait;
use domain::shared::errors::DomainError;

use crate::mailbox::MailboxAccess;

/// One actor-supervision lane (#242 Runtime B, PROP-20260728-152752): a `(actor_type, partition)`
/// row from the `mailbox_partitions` registry joined with the live pending/scheduled backlog counted
/// out of `inbound_messages`. Write-path infrastructure, not a business read model — served by the
/// ADMIN-only `mailboxLanes` query, no backing `View_*`.
#[derive(Debug, Clone)]
pub struct MailboxLaneRow {
    pub actor_type: String,
    /// 0 .. the actor's declared mailbox.partitions - 1 (SMALLINT in the registry).
    pub partition: i16,
    /// The fencing counter (§3.1) — increments on every ownership change, NOT a date.
    pub ownership_version: i64,
    /// Worker instance id; `None` = unowned (claimable).
    pub claimed_by: Option<String>,
    /// Past or `None` = claimable; heartbeat-renewed while owned.
    pub lease_until: Option<chrono::DateTime<chrono::Utc>>,
    /// Largest mailbox position with everything at or below it terminal.
    pub checkpoint: i64,
    /// RECEIVED rows on the lane — the live backlog a worker pass would drain.
    pub pending: i64,
    /// SCHEDULED rows on the lane — future work (reminders) awaiting promotion.
    pub scheduled: i64,
    /// `received_at` of the oldest RECEIVED row — the lane's staleness signal.
    pub oldest_pending_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Largest `attempts` among the lane's RECEIVED rows — > 0 means a head row is failing its
    /// completion transaction and being re-paced toward the cap (PROP-20260802-223522 D4).
    pub retrying_attempts: i64,
    /// Rows terminally FAILED by the delivery-attempts cap — each one is an operator event.
    pub poisoned: i64,
}

/// One cap-poisoned mailbox row (#315): an `inbound_messages` row the delivery-attempts cap
/// flipped to terminal FAILED with error code `DeliveryInfrastructureError` — the per-row detail
/// behind [`MailboxLaneRow::poisoned`]'s count, carrying the id the requeue recovery needs.
#[derive(Debug, Clone)]
pub struct PoisonedMessageRow {
    pub message_id: uuid::Uuid,
    pub actor_type: String,
    pub partition: i16,
    pub message_type: String,
    /// Delivery attempts consumed before the cap flipped the row (SMALLINT on the table).
    pub attempts: i16,
    /// `error->>'code'` — always `DeliveryInfrastructureError` for a poisoned row today, carried
    /// anyway so the screen never has to hard-code the predicate it filters by.
    pub error_code: Option<String>,
    pub correlation_id: Option<uuid::Uuid>,
    pub received_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Read port over the mailbox registry + backlog. Backs the ADMIN `mailboxLanes` supervision query;
/// the adapter joins `mailbox_partitions` with per-lane counts from `inbound_messages`.
///
/// Every method demands the [`MailboxAccess`] witness (#510, the #304 mechanic): implementors
/// outside this crate (`infrastructure::persistence::mailbox_lanes::PgMailboxLaneRepository`, the
/// test doubles) name the type in their signatures and ignore the value — naming a type is not
/// constructing one. Callers go through [`mailbox_lanes`] / [`poisoned_messages`] below.
#[async_trait]
pub trait MailboxLaneRepository: Send + Sync {
    /// Every registered lane, `(actor_type, partition)` order — empty until a worker seeds the registry.
    async fn list(&self, access: MailboxAccess) -> Result<Vec<MailboxLaneRow>, DomainError>;
    /// Every cap-poisoned row (#315), newest first, optionally filtered to one actor type;
    /// `limit` is the resolver-clamped page size. Backs the ADMIN `poisonedMailboxMessages` query.
    async fn poisoned(
        &self,
        actor_type: Option<String>,
        limit: i64,
        access: MailboxAccess,
    ) -> Result<Vec<PoisonedMessageRow>, DomainError>;
}

/// The `mailboxLanes` DOOR (#510): mints the witness for the one sanctioned lane read — the
/// generated ADMIN resolver. Declared in the codegen door allowlist
/// (`every_public_mailbox_door_is_declared`), so opening a second route is a reviewed edit.
pub async fn mailbox_lanes(
    repo: &dyn MailboxLaneRepository,
) -> Result<Vec<MailboxLaneRow>, DomainError> {
    repo.list(MailboxAccess::granted()).await
}

/// The `poisonedMailboxMessages` DOOR (#510): same contract as [`mailbox_lanes`], for the
/// per-row poison detail behind each lane's count (#315). The `limit` arrives already clamped by
/// the resolver (page cap 200) — this door adds no policy, only the witness.
pub async fn poisoned_messages(
    repo: &dyn MailboxLaneRepository,
    actor_type: Option<String>,
    limit: i64,
) -> Result<Vec<PoisonedMessageRow>, DomainError> {
    repo.poisoned(actor_type, limit, MailboxAccess::granted()).await
}
