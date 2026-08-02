//! The WORKER/EXTERNAL-channel enqueue machinery (ADR-20260731-122500 "the mailbox is the only
//! door"): fire-and-forget hand-off for every non-GraphQL producer. A producer records the
//! HAND-OFF, not the outcome — deterministic `message_id`s keep redelivery idempotent (a
//! re-enqueue dedupes on the mailbox pk), and the mailbox ledger owns what happens next.
//!
//! Since #284 slice 3 this module is `pub(crate)` PLUMBING: the only doors offered outside the
//! crate are the GENERATED typed actor clients (`crate::generated::actor_clients`), which
//! delegate to the shared constructors here. The free functions remain in-crate as the shared
//! reference implementations (and, for `enqueue_inbound_facts`, as the D8-deferred bulk door the
//! SIRENE sweep batches through); the codegen guard `mailbox_entry_is_constructed_only_behind_the_typed_doors`
//! keeps `MailboxEntry` construction from reappearing anywhere else.

use application::mailbox::{
    Envelope, Mailbox, MailboxEntry, MailboxInsertOutcome, MailboxScheduleOutcome,
};
// Only the test-only `enqueue_worker_command` reference implementation still takes an `Actor`.
#[cfg(test)]
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
/// The payload's DECLARED identity, per the actor's `identity` in actors.yaml — the ONE derivation
/// every door shares, free-function and typed client alike, so no channel can address a command
/// differently than another.
///
/// `Ok(Some(id))` = the declared property was present and parsed; `Ok(None)` = this actor declares
/// NO identity property (the lane id is minted by the caller); `Err` = the property is DECLARED but
/// missing or unparsable — a keying bug that must fail at the door, because minting a random id
/// would park the command on an arbitrary lane and silently break the per-aggregate serialization
/// the mailbox exists to give.
pub(crate) fn declared_identity(
    command_type: &str,
    payload: &serde_json::Value,
) -> Result<Option<uuid::Uuid>, DomainError> {
    let Some((_, identity_prop, _)) = mailbox_address(command_type) else {
        return Err(DomainError::Repository(format!(
            "command '{command_type}' has no mailbox address (not received by any mailbox actor)"
        )));
    };
    match identity_prop {
        Some(prop) => payload
            .get(prop)
            .and_then(|v| v.as_str())
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .ok_or_else(|| {
                DomainError::Repository(format!(
                    "command '{command_type}': identity property '{prop}' missing or not a uuid — unaddressable"
                ))
            })
            .map(Some),
        None => Ok(None),
    }
}

/// TEST-ONLY since #284 slice 3 (the typed clients are the door, and no production caller
/// remains): kept as the string-keyed reference implementation the drift guard (`drift_guard`
/// tests below) compares the typed `send` against, field for field.
#[cfg(test)]
pub(crate) async fn enqueue_worker_command(
    mailbox: &dyn Mailbox,
    message_id: uuid::Uuid,
    command_type: &str,
    payload: serde_json::Value,
    actor: &Actor,
) -> Result<EnqueueOutcome, DomainError> {
    let Some((actor_type, _, width)) = mailbox_address(command_type) else {
        return Err(DomainError::Repository(format!(
            "command '{command_type}' has no mailbox address (not received by any mailbox actor)"
        )));
    };
    let actor_id = declared_identity(command_type, &payload)?.unwrap_or_else(uuid::Uuid::now_v7);
    let entry = command_entry(
        actor_type,
        width,
        actor_id,
        command_type,
        payload,
        Envelope {
            message_id,
            correlation_id: actor.correlation_id,
            cause_id: actor.cause_id,
            session_id: None,
            trace_id: None,
            user_id: Some(actor.user_id),
            user_type: actor.user_type.clone(),
            channel: "WORKER".into(),
        },
    );
    let payload_hash = entry.payload_hash.clone();
    insert_mapped(mailbox, entry, &payload_hash).await
}

/// The one place a typed COMMAND becomes a [`MailboxEntry`] (kind COMMAND). Shared by
/// [`enqueue_worker_command`] and the generated typed actor clients
/// (`crate::generated::actor_clients`), so the two doors cannot drift on lane, partition or
/// envelope columns — the clients only assemble typed inputs and delegate here.
pub(crate) fn command_entry(
    actor_type: &str,
    width: u16,
    actor_id: uuid::Uuid,
    command_type: &str,
    payload: serde_json::Value,
    env: Envelope,
) -> MailboxEntry {
    let payload_hash = application::journal::payload_hash(&payload);
    MailboxEntry {
        message_id: env.message_id,
        kind: "COMMAND".into(),
        actor_type: actor_type.into(),
        actor_id,
        partition: actor_runtime::stable_partition(&actor_id, width),
        message_type: command_type.into(),
        payload,
        payload_hash,
        channel: env.channel,
        user_id: env.user_id,
        user_type: env.user_type,
        correlation_id: env.correlation_id,
        cause_id: env.cause_id,
        session_id: env.session_id,
        trace_id: env.trace_id,
        source: None,
        external_id: None,
    }
}

/// One adapted inbound BUSINESS fact, ready for the mailbox (kind EVENT). The producer names the
/// lane: `actor_type` must be a mailbox actor; `actor_id` is the addressed instance (or a stable
/// UUIDv5 surrogate when the aggregate id is not a uuid — e.g. `Payment-<intentId>` lanes). The
/// dedupe identity is `(source, external_id)`: `message_id = UUIDv5(source:external_id)` in the
/// inbound namespace, so a webhook redelivery collides on the pk instead of double-applying.
/// CRATE-INTERNAL since #284 slice 3: producers outside the crate hold TYPED facts and go through
/// a client's `record`; only the generated clients and the SIRENE bulk path assemble this.
#[derive(Debug, Clone)]
pub(crate) struct InboundFact {
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

/// Enqueue one adapted inbound fact. CRATE-INTERNAL since #284 slice 3: the generated clients'
/// `record` delegates here (and the drift guard compares against it); adapters record TYPED facts
/// through those clients.
pub(crate) async fn enqueue_inbound_fact(
    mailbox: &dyn Mailbox,
    fact: InboundFact,
) -> Result<EnqueueOutcome, DomainError> {
    let entry = inbound_entry(fact)?;
    let payload_hash = entry.payload_hash.clone();
    insert_mapped(mailbox, entry, &payload_hash).await
}

/// The one place an [`InboundFact`] becomes a [`MailboxEntry`]. Shared by the singular and batched
/// enqueue so they cannot drift on lane, principal or channel.
pub(crate) fn inbound_entry(fact: InboundFact) -> Result<MailboxEntry, DomainError> {
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
///
/// CRATE-INTERNAL: the infrastructure-internal bulk door (PROP-20260728-152752 D8 deferred — no
/// batched client API). The SIRENE sweep is its one producer.
pub(crate) async fn enqueue_inbound_facts(
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
///
/// TEST-ONLY since #284 slice 3: production reminders are declared by the in-tx `schedules:`
/// upsert (`apply_schedules_in_tx`); this remains the reference implementation for the reminder
/// row shape, exercised by the DB-gated `schedule_pg` tests below.
#[cfg(test)]
pub(crate) async fn schedule_reminder(
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
    schedule_mapped(mailbox, entry, scheduled_at, &payload_hash).await
}

/// Schedule one entry and map the port outcome onto [`ScheduleOutcome`] — shared by
/// [`schedule_reminder`] and the generated typed actor clients, so both scheduling doors carry the
/// same replay-vs-conflict contract.
pub(crate) async fn schedule_mapped(
    mailbox: &dyn Mailbox,
    entry: MailboxEntry,
    scheduled_at: chrono::DateTime<chrono::Utc>,
    payload_hash: &str,
) -> Result<ScheduleOutcome, DomainError> {
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
/// TEST-ONLY since #284 slice 3: the typed clients' `cancel` is the public withdrawal door; this
/// remains the reference the `schedule_pg` tests exercise against the real DDL.
#[cfg(test)]
pub(crate) async fn cancel_reminder(
    mailbox: &dyn Mailbox,
    actor_id: uuid::Uuid,
    reminder_name: &str,
) -> Result<bool, DomainError> {
    mailbox.cancel_scheduled(reminder_message_id(actor_id, reminder_name)).await
}

/// Insert one entry and map the port outcome onto [`EnqueueOutcome`] — shared by the free-function
/// enqueue helpers and the generated typed actor clients.
pub(crate) async fn insert_mapped(
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

// `inbound_fact_for` (the string-keyed family→lane switch) is GONE (#284 slice 3): every ACL now
// holds its TYPED fact and records through the generated client, whose sealed Fact traits enforce
// family membership at compile time — the runtime switch had nothing left to check.

// ================================================================================================
// Tests
// ================================================================================================

/// Drift guards for the GENERATED typed actor clients (#284 slice 1, PROP-20260728-152752 §2.1):
/// a typed `send` must produce the very mailbox row `enqueue_worker_command` builds for the same
/// inputs, and a typed `record` the very row `enqueue_inbound_fact` builds — field for field.
/// If either assertion ever fails, the clients stopped delegating to the shared constructors in
/// this module and the two doors have drifted. In-memory mailbox double; no Postgres.
///
/// These live as UNIT tests (not `tests/actor_clients.rs`) since #284 slice 3: the free-function
/// door is `pub(crate)` now — the guard deliberately compares the public typed door against the
/// crate-internal reference implementation, which only an in-crate test can still name.
#[cfg(test)]
mod drift_guard {
    use std::sync::Arc;

    use application::mailbox::mem::MemMailbox;
    use application::mailbox::{Envelope, MailboxEntry};
    use application::ports::Actor;
    use domain::generated::commands::MarkRestaurantClosed;
    use domain::generated::entities::Money;
    use domain::generated::events::PaymentCaptured;
    use domain::generated::scalars::{CurrencyCode, MoneyCents, PaymentIntentId, RestaurantId};

    use super::{
        enqueue_inbound_fact, enqueue_worker_command, inbound_message_id, surrogate_actor_id,
        EnqueueOutcome, InboundFact,
    };
    use crate::generated::actor_clients::{PaymentClient, RestaurantClient};

    /// Field-for-field equality (MailboxEntry deliberately does not derive PartialEq), by FULL
    /// destructuring with no `..` rest pattern — so adding an 18th column to `MailboxEntry` is a
    /// COMPILE error here, not a silently-unasserted field. That structural exhaustiveness is the
    /// guard's guarantee; a named-field comparison list only covers the columns someone remembered.
    fn assert_same_entry(typed: &MailboxEntry, free: &MailboxEntry) {
        let MailboxEntry {
            message_id,
            kind,
            actor_type,
            actor_id,
            partition,
            message_type,
            payload,
            payload_hash,
            channel,
            user_id,
            user_type,
            correlation_id,
            cause_id,
            session_id,
            trace_id,
            source,
            external_id,
        } = typed;
        let MailboxEntry {
            message_id: f_message_id,
            kind: f_kind,
            actor_type: f_actor_type,
            actor_id: f_actor_id,
            partition: f_partition,
            message_type: f_message_type,
            payload: f_payload,
            payload_hash: f_payload_hash,
            channel: f_channel,
            user_id: f_user_id,
            user_type: f_user_type,
            correlation_id: f_correlation_id,
            cause_id: f_cause_id,
            session_id: f_session_id,
            trace_id: f_trace_id,
            source: f_source,
            external_id: f_external_id,
        } = free;
        assert_eq!(message_id, f_message_id, "message_id");
        assert_eq!(kind, f_kind, "kind");
        assert_eq!(actor_type, f_actor_type, "actor_type");
        assert_eq!(actor_id, f_actor_id, "actor_id");
        assert_eq!(partition, f_partition, "partition");
        assert_eq!(message_type, f_message_type, "message_type");
        assert_eq!(payload, f_payload, "payload");
        assert_eq!(payload_hash, f_payload_hash, "payload_hash");
        assert_eq!(channel, f_channel, "channel");
        assert_eq!(user_id, f_user_id, "user_id");
        assert_eq!(user_type, f_user_type, "user_type");
        assert_eq!(correlation_id, f_correlation_id, "correlation_id");
        assert_eq!(cause_id, f_cause_id, "cause_id");
        assert_eq!(session_id, f_session_id, "session_id");
        assert_eq!(trace_id, f_trace_id, "trace_id");
        assert_eq!(source, f_source, "source");
        assert_eq!(external_id, f_external_id, "external_id");
    }

    #[tokio::test]
    async fn typed_send_builds_the_same_row_as_enqueue_worker_command() {
        let restaurant_id = uuid::Uuid::from_u128(0xF00D);
        let cmd = MarkRestaurantClosed {
            restaurant_id: RestaurantId(restaurant_id),
            reason: Some("SIRENE closure".into()),
        };
        let message_id = uuid::Uuid::from_u128(0x1);
        let actor = Actor {
            user_id: uuid::Uuid::from_u128(0x2),
            user_type: "EXTERNAL".into(),
            domain_id: None,
            correlation_id: uuid::Uuid::from_u128(0x3),
            cause_id: Some(uuid::Uuid::from_u128(0x4)),
        };

        let free = MemMailbox::default();
        let outcome = enqueue_worker_command(
            &free,
            message_id,
            "MarkRestaurantClosed",
            serde_json::to_value(&cmd).expect("serialize command"),
            &actor,
        )
        .await
        .expect("free-function enqueue");
        assert_eq!(outcome, EnqueueOutcome::Enqueued);

        let typed = Arc::new(MemMailbox::default());
        let client = RestaurantClient::new(typed.clone(), restaurant_id);
        let env = Envelope {
            message_id,
            correlation_id: actor.correlation_id,
            cause_id: actor.cause_id,
            session_id: None,
            trace_id: None,
            user_id: Some(actor.user_id),
            user_type: actor.user_type.clone(),
            channel: "WORKER".into(),
        };
        assert_eq!(client.send(cmd, env).await.expect("typed send"), EnqueueOutcome::Enqueued);

        assert_same_entry(
            &typed.entry(message_id).expect("typed row"),
            &free.entry(message_id).expect("free row"),
        );
    }

    #[tokio::test]
    async fn typed_record_builds_the_same_row_as_enqueue_inbound_fact() {
        let fact = PaymentCaptured {
            payment_intent_id: PaymentIntentId("pi_84".into()),
            order_id: None,
            restaurant_id: RestaurantId(uuid::Uuid::from_u128(0xF00D)),
            amount: Money { amount_cents: MoneyCents(1990), currency: CurrencyCode("EUR".into()) },
        };
        // The Payment lane id is the UUIDv5 surrogate over the gateway's intent id — the same key
        // the Stripe ACL uses, so the typed client and the adapter land on the same lane.
        let actor_id = surrogate_actor_id("Payment", "pi_84");
        let correlation_id = uuid::Uuid::from_u128(0xC0);
        let tagged = serde_json::json!({
            "eventType": "PaymentCaptured",
            "payload": serde_json::to_value(&fact).expect("serialize fact"),
        });

        let free = MemMailbox::default();
        let outcome = enqueue_inbound_fact(
            &free,
            InboundFact {
                source: "stripe".into(),
                external_id: "evt_84".into(),
                event_type: "PaymentCaptured".into(),
                payload: tagged,
                correlation_id,
                actor_type: "Payment".into(),
                actor_id,
            },
        )
        .await
        .expect("free-function enqueue");
        assert_eq!(outcome, EnqueueOutcome::Enqueued);

        let typed = Arc::new(MemMailbox::default());
        let client = PaymentClient::new(typed.clone(), actor_id);
        assert_eq!(
            client.record(fact, "stripe", "evt_84", correlation_id).await.expect("typed record"),
            EnqueueOutcome::Enqueued
        );

        // The identity MUST be the deterministic (source, external_id) key — never caller-minted —
        // or webhook redelivery double-applies instead of colliding on the pk.
        let message_id = inbound_message_id("stripe", "evt_84");
        let typed_row =
            typed.entry(message_id).expect("typed row keyed by the deterministic inbound id");
        assert_same_entry(&typed_row, &free.entry(message_id).expect("free row"));

        // The wire form must be the DOMAIN ENUM's own representation, not a hand-built literal that
        // happens to match it today: round-trip through `DomainEvent` so a tag/content rename in
        // the domain emitter fails HERE instead of at delivery time in production.
        let round_tripped: domain::generated::events::DomainEvent =
            serde_json::from_value(typed_row.payload.clone())
                .expect("recorded payload deserializes as DomainEvent — the delivery route's own type");
        assert!(
            matches!(round_tripped, domain::generated::events::DomainEvent::PaymentCaptured(_)),
            "the adjacent tag routes back to the variant that was recorded"
        );
    }

    /// Fix-1 invariant (#288 review): a payload whose DECLARED identity names a DIFFERENT
    /// aggregate than the client's lane must be REFUSED at the door. Accepting it would park the
    /// command on one lane while the handler acts on another aggregate — per-aggregate
    /// serialization silently broken, which is the exact failure the mailbox exists to prevent.
    #[tokio::test]
    async fn a_mis_addressed_send_is_refused() {
        let lane = uuid::Uuid::from_u128(0xAAAA);
        let other = uuid::Uuid::from_u128(0xBBBB);
        let cmd = MarkRestaurantClosed { restaurant_id: RestaurantId(other), reason: None };
        let mailbox = Arc::new(MemMailbox::default());
        let client = RestaurantClient::new(mailbox.clone(), lane);

        let err = client
            .send(cmd, test_envelope(uuid::Uuid::from_u128(0x10)))
            .await
            .expect_err("identity mismatch must refuse, not mis-file");
        assert!(err.to_string().contains("does not match"), "the error names the mismatch: {err}");
        assert!(mailbox.entries().is_empty(), "nothing may reach the mailbox on a refused send");
    }

    /// Typed `schedule` has no free-function counterpart (it mints the first kind-COMMAND
    /// SCHEDULED rows), so its guard is ABSOLUTE assertions instead of a drift comparison: the row
    /// must carry the same `command_entry` columns as an immediate send plus the `scheduled_at`
    /// the caller gave — and `cancel` must withdraw it exactly once.
    #[tokio::test]
    async fn typed_schedule_parks_a_command_row_and_cancel_withdraws_it_once() {
        let restaurant_id = uuid::Uuid::from_u128(0xF00D);
        let cmd = MarkRestaurantClosed {
            restaurant_id: RestaurantId(restaurant_id),
            reason: Some("scheduled closure".into()),
        };
        let message_id = uuid::Uuid::from_u128(0x5C);
        let at = chrono::DateTime::parse_from_rfc3339("2026-08-03T06:00:00Z")
            .expect("fixed timestamp")
            .with_timezone(&chrono::Utc);

        let mailbox = Arc::new(MemMailbox::default());
        let client = RestaurantClient::new(mailbox.clone(), restaurant_id);
        client.schedule(cmd, test_envelope(message_id), at).await.expect("typed schedule");

        let row = mailbox.entry(message_id).expect("scheduled row");
        assert_eq!(row.kind, "COMMAND");
        assert_eq!(row.actor_type, "Restaurant");
        assert_eq!(row.actor_id, restaurant_id);
        assert_eq!(row.message_type, "MarkRestaurantClosed");
        assert_eq!(row.partition, actor_runtime::stable_partition(&restaurant_id, 100));
        assert_eq!(mailbox.scheduled_at(message_id), Some(at), "parked until due, not delivered now");

        assert!(client.cancel(message_id).await.expect("cancel"), "a SCHEDULED row cancels");
        assert!(
            !client.cancel(message_id).await.expect("second cancel"),
            "a spent cancellation reports false, not an error"
        );
    }

    /// The envelope every test hands the client — deterministic ids, WORKER channel.
    fn test_envelope(message_id: uuid::Uuid) -> Envelope {
        Envelope {
            message_id,
            correlation_id: uuid::Uuid::from_u128(0x3),
            cause_id: None,
            session_id: None,
            trace_id: None,
            user_id: Some(uuid::Uuid::from_u128(0x2)),
            user_type: "EXTERNAL".into(),
            channel: "WORKER".into(),
        }
    }
}

/// DB-gated tests for the REMINDER write path (#272 Runtime D, ADR-20260731-150500 /
/// ADR-20260731-153000): `schedule_reminder` over the real `PgMailbox` against the real
/// migration DDL — the deterministic `UUIDv5(actor_id, purpose)` identity, the SCHEDULED row
/// with `position` NULL (the sequence must NOT be consumed at declare time), reschedule IN PLACE
/// while SCHEDULED, Duplicate once the occurrence is spent, and `SCHEDULED → CANCELLED` beating
/// the promotion pass.
///
/// Unit tests since #284 slice 3 (`schedule_reminder`/`cancel_reminder` are `pub(crate)` now).
/// Needs `DATABASE_URL`; skips otherwise (DB_TESTS_REQUIRED makes the skip loud, #230).
#[cfg(test)]
mod schedule_pg {
    use domain::generated::scalars::InboundMessageStatus;
    use sqlx::{PgPool, Row};

    use super::{cancel_reminder, reminder_message_id, schedule_reminder, ScheduleOutcome};
    use crate::persistence::mailbox_store::PgMailbox;

    async fn setup(pool: &PgPool) {
        sqlx::raw_sql(
            "DROP TABLE IF EXISTS inbound_messages, mailbox_partitions CASCADE;\n\
             DROP SEQUENCE IF EXISTS inbound_messages_position_seq;",
        )
        .execute(pool)
        .await
        .expect("drop");
        sqlx::raw_sql(include_str!(
            "../../../../migrations/20260731063000_actor_mailbox_tables.sql"
        ))
        .execute(pool)
        .await
        .expect("apply the actor-mailbox migration");
    }

    fn gated() -> Option<String> {
        match std::env::var("DATABASE_URL") {
            Ok(url) => Some(url),
            Err(_) => {
                assert!(
                    std::env::var("DB_TESTS_REQUIRED").is_err(),
                    "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
                );
                eprintln!("SKIP mailbox schedule_pg test: DATABASE_URL not set");
                None
            }
        }
    }

    /// Serialize the suite: every test resets the same tables.
    static DB_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn tagged(window: &str) -> serde_json::Value {
        serde_json::json!({
            "eventType": "OrderExpired",
            "payload": { "orderId": "9b2f8f3e-0000-0000-0000-000000000ad1", "window": window },
        })
    }

    async fn row(pool: &PgPool, message_id: uuid::Uuid) -> sqlx::postgres::PgRow {
        sqlx::query(
            "SELECT kind, message_type, channel, status, position, scheduled_at, completed_at, \
                    payload, payload_hash \
             FROM inbound_messages WHERE message_id = $1",
        )
        .bind(message_id)
        .fetch_one(pool)
        .await
        .expect("row")
    }

    /// §1 (with ADR-153000 §1a): declaring the reminder writes ONE SCHEDULED row — kind MESSAGE,
    /// message_type = the payload FACT, position NULL and the sequence untouched — and
    /// re-declaring with a later time moves the SAME row in place.
    #[tokio::test]
    async fn schedule_then_redeclare_reschedules_the_same_row_in_place() {
        let Some(url) = gated() else { return };
        let _guard = DB_LOCK.lock().await;
        let pool = PgPool::connect(&url).await.expect("connect");
        setup(&pool).await;
        let mailbox = PgMailbox::new(pool.clone());

        let order = uuid::Uuid::from_u128(0x0AD1);
        let t1 = chrono::Utc::now() + chrono::Duration::days(365);
        let out = schedule_reminder(&mailbox, "Order", order, "expire", tagged("P1Y"), t1, order)
            .await
            .expect("schedule");
        assert_eq!(out, ScheduleOutcome::Scheduled);

        let mid = reminder_message_id(order, "expire");
        let r = row(&pool, mid).await;
        assert_eq!(r.get::<String, _>("kind"), "MESSAGE");
        assert_eq!(r.get::<String, _>("message_type"), "OrderExpired");
        assert_eq!(r.get::<String, _>("channel"), "WORKER");
        assert_eq!(r.get::<String, _>("status"), "SCHEDULED");
        assert_eq!(r.get::<Option<i64>, _>("position"), None, "no position until promotion");
        assert_eq!(
            r.get::<chrono::DateTime<chrono::Utc>, _>("scheduled_at").timestamp_micros(),
            t1.timestamp_micros()
        );
        // The declare must not have consumed the position sequence: the next immediate row takes 1.
        let next: i64 = sqlx::query("SELECT nextval('inbound_messages_position_seq') AS v")
            .fetch_one(&pool)
            .await
            .expect("nextval")
            .get("v");
        assert_eq!(next, 1, "declaring a reminder must not burn a position");

        // Re-declare with a LATER time and a newer payload: Rescheduled — same identity, one row,
        // scheduled_at and payload moved (ADR-150500 §1: the row "always carries the latest time").
        let t2 = t1 + chrono::Duration::days(365);
        let out = schedule_reminder(&mailbox, "Order", order, "expire", tagged("P2Y"), t2, order)
            .await
            .expect("reschedule");
        assert_eq!(out, ScheduleOutcome::Rescheduled);
        let count: i64 = sqlx::query("SELECT count(*) AS n FROM inbound_messages")
            .fetch_one(&pool)
            .await
            .expect("count")
            .get("n");
        assert_eq!(count, 1, "reschedule converges on ONE pending row");
        let r = row(&pool, mid).await;
        assert_eq!(
            r.get::<String, _>("status"),
            "SCHEDULED",
            "a reschedule never transitions status"
        );
        assert_eq!(r.get::<Option<i64>, _>("position"), None);
        assert_eq!(
            r.get::<chrono::DateTime<chrono::Utc>, _>("scheduled_at").timestamp_micros(),
            t2.timestamp_micros()
        );
        assert_eq!(
            r.get::<serde_json::Value, _>("payload").pointer("/payload/window"),
            Some(&serde_json::json!("P2Y")),
            "the re-declaration's payload replaced the original"
        );
    }

    /// §2: once the promotion pass has stamped a position the occurrence is SPENT — a
    /// re-declaration is a Duplicate (same payload) or a PayloadConflict (different payload), and
    /// the row is untouched either way.
    #[tokio::test]
    async fn redeclaring_a_promoted_reminder_is_a_duplicate_untouched() {
        let Some(url) = gated() else { return };
        let _guard = DB_LOCK.lock().await;
        let pool = PgPool::connect(&url).await.expect("connect");
        setup(&pool).await;
        let mailbox = PgMailbox::new(pool.clone());

        let order = uuid::Uuid::from_u128(0x0AD2);
        let due = chrono::Utc::now() - chrono::Duration::seconds(1);
        schedule_reminder(&mailbox, "Order", order, "expire", tagged("P1Y"), due, order)
            .await
            .expect("schedule");
        assert_eq!(actor_runtime::promote_due(&pool, "Order").await.expect("promote"), 1);

        // Same payload → the idempotent redelivery case, carrying the row's live status.
        let later = chrono::Utc::now() + chrono::Duration::days(30);
        let out = schedule_reminder(&mailbox, "Order", order, "expire", tagged("P1Y"), later, order)
            .await
            .expect("re-declare");
        assert_eq!(out, ScheduleOutcome::Deduplicated(InboundMessageStatus::RECEIVED));
        // Different payload → the conflict case. Untouched all the same.
        let out = schedule_reminder(&mailbox, "Order", order, "expire", tagged("P2Y"), later, order)
            .await
            .expect("re-declare, conflicting");
        assert_eq!(out, ScheduleOutcome::PayloadConflict(InboundMessageStatus::RECEIVED));

        let r = row(&pool, reminder_message_id(order, "expire")).await;
        assert_eq!(r.get::<String, _>("status"), "RECEIVED");
        assert_eq!(
            r.get::<chrono::DateTime<chrono::Utc>, _>("scheduled_at").timestamp_micros(),
            due.timestamp_micros(),
            "a spent occurrence's scheduled_at never moves"
        );
        assert_eq!(
            r.get::<serde_json::Value, _>("payload").pointer("/payload/window"),
            Some(&serde_json::json!("P1Y")),
            "a spent occurrence's payload never moves"
        );
    }

    /// §3: cancellation is the explicit withdrawal — `SCHEDULED → CANCELLED`, terminal, never
    /// promoted, and idempotently `false` on a second attempt (or after promotion won the race).
    #[tokio::test]
    async fn cancelled_reminder_is_never_promoted() {
        let Some(url) = gated() else { return };
        let _guard = DB_LOCK.lock().await;
        let pool = PgPool::connect(&url).await.expect("connect");
        setup(&pool).await;
        let mailbox = PgMailbox::new(pool.clone());

        let order = uuid::Uuid::from_u128(0x0AD3);
        // Already due — so a promotion pass WOULD take it if cancellation did not win first.
        let due = chrono::Utc::now() - chrono::Duration::seconds(1);
        schedule_reminder(&mailbox, "Order", order, "expire", tagged("P1Y"), due, order)
            .await
            .expect("schedule");

        assert!(cancel_reminder(&mailbox, order, "expire").await.expect("cancel"));
        assert!(
            !cancel_reminder(&mailbox, order, "expire").await.expect("re-cancel"),
            "already CANCELLED"
        );
        assert_eq!(actor_runtime::promote_due(&pool, "Order").await.expect("promote"), 0);

        let r = row(&pool, reminder_message_id(order, "expire")).await;
        assert_eq!(r.get::<String, _>("status"), "CANCELLED");
        assert_eq!(r.get::<Option<i64>, _>("position"), None, "never promoted, never deliverable");
        assert!(r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("completed_at").is_some());
    }
}
