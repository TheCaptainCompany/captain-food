//! The WORKER/EXTERNAL-channel enqueue helpers (ADR-20260731-122500 "the mailbox is the only
//! door"): fire-and-forget hand-off for every non-GraphQL producer. A producer records the
//! HAND-OFF, not the outcome — deterministic `message_id`s keep redelivery idempotent (a
//! re-enqueue dedupes on the mailbox pk), and the mailbox ledger owns what happens next.

use application::mailbox::{Mailbox, MailboxEntry, MailboxInsertOutcome};
use application::ports::Actor;
use domain::generated::scalars::InboundMessageStatus;
use domain::shared::errors::DomainError;

use crate::generated::command_router::{mailbox_address, ACTOR_MAILBOXES};

/// What one fire-and-forget enqueue did.
#[derive(Debug, Clone, PartialEq)]
pub enum EnqueueOutcome {
    /// Fresh hand-off: the mailbox worker will deliver it.
    Enqueued,
    /// Same `message_id` + same payload already in the mailbox (any status) — the idempotent
    /// redelivery case; nothing to do. Carries the existing row's status for the producer's log.
    Deduplicated(InboundMessageStatus),
    /// Same `message_id`, DIFFERENT payload — the source data changed under a redelivered
    /// identity (or a keying bug). Never enqueued; the producer logs and skips (changed data
    /// arrives under its own fresh identity).
    PayloadConflict(InboundMessageStatus),
}

/// Enqueue one WORKER-channel command (SIRENE close signals, HubRise imports). The address —
/// which actor, which payload key carries its id, how many partitions — comes from the SAME
/// generated map the GraphQL resolvers were built from, so no channel can address differently
/// than another. `Err` only on infrastructure failure or an unroutable command type.
pub async fn enqueue_worker_command(
    mailbox: &dyn Mailbox,
    message_id: uuid::Uuid,
    command_type: &str,
    payload: serde_json::Value,
    actor: &Actor,
) -> Result<EnqueueOutcome, DomainError> {
    let Some((actor_type, identity_prop, width)) = mailbox_address(command_type) else {
        return Err(DomainError::Repository(format!(
            "command '{command_type}' has no mailbox address (not received by any mailbox actor)"
        )));
    };
    let actor_id = identity_prop
        .and_then(|prop| payload.get(prop))
        .and_then(|v| v.as_str())
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .unwrap_or_else(uuid::Uuid::now_v7);
    let payload_hash = application::journal::payload_hash(&payload);
    let entry = MailboxEntry {
        message_id,
        kind: "COMMAND".into(),
        actor_type: actor_type.into(),
        actor_id,
        partition: actor_runtime::stable_partition(&actor_id, width),
        message_type: command_type.into(),
        payload,
        payload_hash: payload_hash.clone(),
        channel: "WORKER".into(),
        user_id: Some(actor.user_id),
        user_type: actor.user_type.clone(),
        correlation_id: actor.correlation_id,
        cause_id: actor.cause_id,
        session_id: None,
        trace_id: None,
        source: None,
        external_id: None,
    };
    insert_mapped(mailbox, entry, &payload_hash).await
}

/// One adapted inbound BUSINESS fact, ready for the mailbox (kind EVENT). The producer names the
/// lane: `actor_type` must be a mailbox actor; `actor_id` is the addressed instance (or a stable
/// UUIDv5 surrogate when the aggregate id is not a uuid — e.g. `Payment-<intentId>` lanes). The
/// dedupe identity is `(source, external_id)`: `message_id = UUIDv5(source:external_id)` in the
/// inbound namespace, so a webhook redelivery collides on the pk instead of double-applying.
#[derive(Debug, Clone)]
pub struct InboundFact {
    pub source: String,
    pub external_id: String,
    /// events.yaml key (`PaymentCaptured`, …).
    pub event_type: String,
    /// The ADJACENTLY-TAGGED `DomainEvent` form (`{"eventType", "payload"}`) — the delivery
    /// route deserializes the union, exactly as the retired drain worker did.
    pub payload: serde_json::Value,
    pub correlation_id: uuid::Uuid,
    pub actor_type: String,
    pub actor_id: uuid::Uuid,
}

/// The fixed UUIDv5 namespace for inbound identities (same as the retired drain worker's system
/// principals — deterministic, stable across deliveries and deployments).
pub fn inbound_namespace() -> uuid::Uuid {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"https://captain.food/integrations/inbound")
}

/// The mailbox identity of an inbound fact: `UUIDv5(source:external_id)`.
pub fn inbound_message_id(source: &str, external_id: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(&inbound_namespace(), format!("{source}:{external_id}").as_bytes())
}

/// Enqueue one adapted inbound fact (adapter ACLs, the SIRENE registry sweep).
pub async fn enqueue_inbound_fact(
    mailbox: &dyn Mailbox,
    fact: InboundFact,
) -> Result<EnqueueOutcome, DomainError> {
    let Some((_, width)) = ACTOR_MAILBOXES.iter().find(|(a, _)| *a == fact.actor_type) else {
        return Err(DomainError::Repository(format!(
            "'{}' is not a mailbox actor — inbound fact '{}' has no lane",
            fact.actor_type, fact.event_type
        )));
    };
    let message_id = inbound_message_id(&fact.source, &fact.external_id);
    let payload_hash = application::journal::payload_hash(&fact.payload);
    let entry = MailboxEntry {
        message_id,
        kind: "EVENT".into(),
        actor_type: fact.actor_type.clone(),
        actor_id: fact.actor_id,
        partition: actor_runtime::stable_partition(&fact.actor_id, *width),
        message_type: fact.event_type,
        payload: fact.payload,
        payload_hash: payload_hash.clone(),
        channel: "EXTERNAL".into(),
        // The acting principal for an inbound fact is the external system (ADR-0041): a
        // deterministic per-source system user, stamped at delivery by the EVENT route.
        user_id: Some(uuid::Uuid::new_v5(
            &inbound_namespace(),
            format!("system:{}", fact.source).as_bytes(),
        )),
        user_type: "EXTERNAL".into(),
        correlation_id: fact.correlation_id,
        cause_id: None,
        session_id: None,
        trace_id: None,
        source: Some(fact.source),
        external_id: Some(fact.external_id),
    };
    insert_mapped(mailbox, entry, &payload_hash).await
}

async fn insert_mapped(
    mailbox: &dyn Mailbox,
    entry: MailboxEntry,
    payload_hash: &str,
) -> Result<EnqueueOutcome, DomainError> {
    match mailbox.insert(&entry).await? {
        MailboxInsertOutcome::Inserted => Ok(EnqueueOutcome::Enqueued),
        MailboxInsertOutcome::Duplicate { status, payload_hash: existing } => {
            if existing == payload_hash {
                Ok(EnqueueOutcome::Deduplicated(status))
            } else {
                Ok(EnqueueOutcome::PayloadConflict(status))
            }
        }
    }
}
