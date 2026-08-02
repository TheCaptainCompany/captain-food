//! The WORKER/EXTERNAL-channel enqueue helpers (ADR-20260731-122500 "the mailbox is the only
//! door"): fire-and-forget hand-off for every non-GraphQL producer. A producer records the
//! HAND-OFF, not the outcome — deterministic `message_id`s keep redelivery idempotent (a
//! re-enqueue dedupes on the mailbox pk), and the mailbox ledger owns what happens next.

use application::mailbox::{Mailbox, MailboxEntry, MailboxInsertOutcome, MailboxScheduleOutcome};
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
    // A DECLARED identity property that is missing or unparsable is a keying bug and must fail
    // at the door: minting a random id would park the command on an arbitrary lane, silently
    // breaking the per-aggregate serialization the mailbox exists to give. Only a command whose
    // actor declares NO identity property (id minted at delivery) gets a fresh lane id.
    let actor_id = match identity_prop {
        Some(prop) => payload
            .get(prop)
            .and_then(|v| v.as_str())
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .ok_or_else(|| {
                DomainError::Repository(format!(
                    "command '{command_type}': identity property '{prop}' missing or not a uuid — unaddressable"
                ))
            })?,
        None => uuid::Uuid::now_v7(),
    };
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
/// principals — deterministic, stable across deliveries and deployments). Owned by
/// `application::reminders` since the reminders runtime landed (#272 D2) — one namespace, one
/// dedupe axis; re-exported here so every adapter keeps its import path.
pub use application::reminders::inbound_namespace;

/// The mailbox identity of an inbound fact: `UUIDv5(source:external_id)`.
pub fn inbound_message_id(source: &str, external_id: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(&inbound_namespace(), format!("{source}:{external_id}").as_bytes())
}

/// Enqueue one adapted inbound fact (adapter ACLs, the SIRENE registry sweep).
pub async fn enqueue_inbound_fact(
    mailbox: &dyn Mailbox,
    fact: InboundFact,
) -> Result<EnqueueOutcome, DomainError> {
    let entry = inbound_entry(fact)?;
    let payload_hash = entry.payload_hash.clone();
    insert_mapped(mailbox, entry, &payload_hash).await
}

/// The one place an [`InboundFact`] becomes a [`MailboxEntry`]. Shared by the singular and batched
/// enqueue so they cannot drift on lane, principal or channel.
fn inbound_entry(fact: InboundFact) -> Result<MailboxEntry, DomainError> {
    let Some((_, width)) = ACTOR_MAILBOXES.iter().find(|(a, _)| *a == fact.actor_type) else {
        return Err(DomainError::Repository(format!(
            "'{}' is not a mailbox actor — inbound fact '{}' has no lane",
            fact.actor_type, fact.event_type
        )));
    };
    let message_id = inbound_message_id(&fact.source, &fact.external_id);
    let payload_hash = application::journal::payload_hash(&fact.payload);
    Ok(MailboxEntry {
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
    })
}

/// Enqueue MANY inbound facts in one round-trip — the batched form of [`enqueue_inbound_fact`].
///
/// Returns, per input fact and in the SAME order, whether it was newly enqueued. `false` means the
/// identity was already on the mailbox: either awaiting delivery or already decided. Both are
/// terminal for the producer — it has handed the fact over either way — which is exactly why this
/// does not distinguish `Deduplicated` from `PayloadConflict` the way the singular form does.
/// A producer keyed on a content hash (`{siret}:{payload_hash}`) cannot produce a payload conflict
/// for an identity it already wrote.
///
/// The entry construction is shared with [`enqueue_inbound_fact`] via [`inbound_entry`], so the two
/// paths cannot drift on partition, principal or channel — a batched row and a singular row for the
/// same fact are byte-identical.
pub async fn enqueue_inbound_facts(
    mailbox: &dyn Mailbox,
    facts: Vec<InboundFact>,
) -> Result<Vec<bool>, DomainError> {
    if facts.is_empty() {
        return Ok(Vec::new());
    }
    let entries: Vec<MailboxEntry> =
        facts.into_iter().map(inbound_entry).collect::<Result<_, _>>()?;
    let inserted: std::collections::HashSet<uuid::Uuid> =
        mailbox.insert_many(&entries).await?.into_iter().collect();
    Ok(entries.iter().map(|e| inserted.contains(&e.message_id)).collect())
}

/// What one reminder declaration did. A separate enum from [`EnqueueOutcome`]: `Rescheduled` is a
/// reminder-only outcome, and widening the shared enum would force every adapter's exhaustive
/// match to name a case it can never see.
#[derive(Debug, Clone, PartialEq)]
pub enum ScheduleOutcome {
    /// Fresh SCHEDULED row — the promotion pass delivers it when due.
    Scheduled,
    /// The identity was still SCHEDULED: `scheduled_at` + payload moved in place — the
    /// "declare the reminder with the time you NOW want" contract (ADR-20260731-150500).
    Rescheduled,
    /// The pending occurrence is spent (promoted or terminal) and the payload matches — the
    /// idempotent re-declaration; nothing to do.
    Deduplicated(InboundMessageStatus),
    /// Spent occurrence AND a different payload — never written; the caller logs and skips
    /// (a genuinely new occurrence needs occurrence-scoped identity, open per ADR-150500 §2).
    PayloadConflict(InboundMessageStatus),
}

/// The mailbox identity of a reminder: `UUIDv5(actor_id, purpose)` in the inbound namespace
/// (PROP-20260728-152752 §3.4) — deterministic, so every re-declaration of the same purpose
/// converges on ONE pending row (ADR-20260731-150500 §1). Owned by `application::reminders`.
pub use application::reminders::reminder_message_id;

/// Declare (or re-declare) one reminder: kind MESSAGE, channel WORKER, `message_type` = the
/// reminder's payload FACT type (ADR-20260731-153000 §1a — the scheduled message is an event,
/// recorded with record semantics at delivery). `payload_event_tagged` is the adjacently-tagged
/// `{"eventType","payload"}` form, exactly like an adapted inbound fact.
pub async fn schedule_reminder(
    mailbox: &dyn Mailbox,
    actor_type: &str,
    actor_id: uuid::Uuid,
    reminder_name: &str,
    payload_event_tagged: serde_json::Value,
    scheduled_at: chrono::DateTime<chrono::Utc>,
    correlation_id: uuid::Uuid,
) -> Result<ScheduleOutcome, DomainError> {
    let Some((_, width)) = ACTOR_MAILBOXES.iter().find(|(a, _)| *a == actor_type) else {
        return Err(DomainError::Repository(format!(
            "'{actor_type}' is not a mailbox actor — reminder '{reminder_name}' has no lane"
        )));
    };
    let event_type = payload_event_tagged
        .get("eventType")
        .and_then(|t| t.as_str())
        .ok_or_else(|| {
            DomainError::Repository(format!(
                "reminder '{reminder_name}': payload without eventType tag — not a fact"
            ))
        })?
        .to_owned();
    let message_id = reminder_message_id(actor_id, reminder_name);
    let payload_hash = application::journal::payload_hash(&payload_event_tagged);
    let entry = MailboxEntry {
        message_id,
        kind: "MESSAGE".into(),
        actor_type: actor_type.to_owned(),
        actor_id,
        partition: actor_runtime::stable_partition(&actor_id, *width),
        message_type: event_type,
        payload: payload_event_tagged,
        payload_hash: payload_hash.clone(),
        channel: "WORKER".into(),
        // The scheduling principal is the system scheduler (ADR-0041 envelope metadata): a
        // deterministic system user, like the per-source principals on adapted facts.
        user_id: Some(uuid::Uuid::new_v5(&inbound_namespace(), b"system:scheduler")),
        user_type: "EXTERNAL".into(),
        correlation_id,
        cause_id: None,
        session_id: None,
        trace_id: None,
        source: None,
        external_id: None,
    };
    match mailbox.schedule(&entry, scheduled_at).await? {
        MailboxScheduleOutcome::Scheduled => Ok(ScheduleOutcome::Scheduled),
        MailboxScheduleOutcome::Rescheduled => Ok(ScheduleOutcome::Rescheduled),
        MailboxScheduleOutcome::Duplicate { status, payload_hash: existing } => {
            if existing == payload_hash {
                Ok(ScheduleOutcome::Deduplicated(status))
            } else {
                Ok(ScheduleOutcome::PayloadConflict(status))
            }
        }
    }
}

/// Withdraw a reminder that has not been promoted yet: `SCHEDULED → CANCELLED`
/// (ADR-20260731-150500 §3). `false` = the row is absent, already delivered, or already
/// cancelled — the caller decides whether losing that race matters.
pub async fn cancel_reminder(
    mailbox: &dyn Mailbox,
    actor_id: uuid::Uuid,
    reminder_name: &str,
) -> Result<bool, DomainError> {
    mailbox.cancel_scheduled(reminder_message_id(actor_id, reminder_name)).await
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

/// Surrogate lane id for aggregates whose identity is NOT a uuid (the `Payment-<intentId>`
/// streams): UUIDv5 over `"{actor_type}:{key}"` in the inbound namespace. FROZEN like the
/// partition hash — the same aggregate must land on the same lane forever, or its in-flight
/// facts reorder across lanes.
pub fn surrogate_actor_id(actor_type: &str, key: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(&inbound_namespace(), format!("{actor_type}:{key}").as_bytes())
}

/// Build the [`InboundFact`] for one ADAPTED domain event (the tagged `{"eventType","payload"}`
/// form every ACL produces): resolves the addressed LANE from the event family — the same
/// families the kind-EVENT delivery route recognizes, so a fact that cannot be addressed here
/// could not be delivered there either (fail at the door, not in the worker).
pub fn inbound_fact_for(
    source: &str,
    external_id: &str,
    correlation_id: uuid::Uuid,
    tagged: serde_json::Value,
) -> Result<InboundFact, DomainError> {
    let event_type = tagged
        .get("eventType")
        .and_then(|t| t.as_str())
        .ok_or_else(|| DomainError::Repository("adapted event without eventType tag".into()))?
        .to_owned();
    let payload_str = |key: &str| {
        tagged
            .get("payload")
            .and_then(|p| p.get(key))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .ok_or_else(|| {
                DomainError::Repository(format!("{event_type}: payload lacks '{key}' — unaddressable"))
            })
    };
    let (actor_type, actor_id) = match event_type.as_str() {
        "PaymentCaptured" | "PaymentFailed" | "PaymentRefunded" => {
            ("Payment", surrogate_actor_id("Payment", &payload_str("paymentIntentId")?))
        }
        "DeliveryAcceptedByPartner" | "DeliveryRejectedByPartner" | "DeliveryStatusUpdated" => {
            let id = payload_str("deliveryJobId")?;
            (
                "DeliveryJob",
                uuid::Uuid::parse_str(&id).map_err(|e| {
                    DomainError::Repository(format!("{event_type}: deliveryJobId '{id}': {e}"))
                })?,
            )
        }
        "RestaurantRegistered" => {
            let id = payload_str("restaurantId")?;
            (
                "Restaurant",
                uuid::Uuid::parse_str(&id).map_err(|e| {
                    DomainError::Repository(format!("{event_type}: restaurantId '{id}': {e}"))
                })?,
            )
        }
        other => {
            return Err(DomainError::Repository(format!(
                "no mailbox lane for inbound event type '{other}' — extend inbound_fact_for + the kind-EVENT route together"
            )))
        }
    };
    Ok(InboundFact {
        source: source.to_owned(),
        external_id: external_id.to_owned(),
        event_type,
        payload: tagged,
        correlation_id,
        actor_type: actor_type.to_owned(),
        actor_id,
    })
}
