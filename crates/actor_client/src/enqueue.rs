//! The shared mailbox-entry CONSTRUCTORS (ADR-20260731-122500 "the mailbox is the only door"):
//! the one place a typed message becomes a [`MailboxEntry`] row. Producers never see this module —
//! the only doors offered outside the crate are the GENERATED typed actor clients — since phase 2
//! (#306) one crate per actor under `crates/clients/`, reaching these constructors through the
//! opaque [`crate::ActorDoor`] — so a typed send and any other channel can never drift on lane,
//! partition, principal or channel.
//!
//! Since #290 phase 1 (PROP-20260802-130500 D1) the boundary is the CRATE: `MailboxEntry` fields
//! are `pub(crate)`, so these constructors physically cannot be re-implemented anywhere else. The
//! codegen guard `mailbox_entry_is_constructed_only_behind_the_typed_doors` stays as the textual
//! tripwire on this crate itself.

use crate::mailbox::{
    Envelope, Mailbox, MailboxAccess, MailboxEntry, MailboxInsertOutcome, MailboxScheduleOutcome,
};
// Only the test-only `enqueue_worker_command` reference implementation still takes an `Actor`.
#[cfg(any(test, feature = "test-fixtures"))]
use application::ports::Actor;
use domain::generated::scalars::InboundMessageStatus;
use domain::shared::errors::DomainError;

use crate::generated::addresses::{mailbox_address, ACTOR_INBOUND_FACTS, ACTOR_MAILBOXES};

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

/// The payload's DECLARED identity, per the actor's `identity` in actors.yaml — the ONE derivation
/// every door shares, so no channel can address a command differently than another.
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
/// remains): kept as the string-keyed reference implementation the drift guard compares the typed
/// `send` against, field for field. Exported under `test-fixtures` since phase 2 (#306) — the
/// guard moved to `tests/drift_guard.rs` when the clients it exercises became separate crates.
#[cfg(any(test, feature = "test-fixtures"))]
pub async fn enqueue_worker_command(
    mailbox: &dyn Mailbox,
    message_id: uuid::Uuid,
    command_type: &str,
    payload: serde_json::Value,
    actor: &Actor,
) -> Result<EnqueueOutcome, DomainError> {
    // The width element of `mailbox_address`'s tuple is deliberately ignored (#596): addressing
    // reads the declaration through `command_entry`, and nothing may pass a width in.
    let Some((actor_type, _, _)) = mailbox_address(command_type) else {
        return Err(DomainError::Repository(format!(
            "command '{command_type}' has no mailbox address (not received by any mailbox actor)"
        )));
    };
    let actor_id = declared_identity(command_type, &payload)?.unwrap_or_else(uuid::Uuid::now_v7);
    let entry = command_entry(
        actor_type,
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
    )?;
    let payload_hash = entry.payload_hash.clone();
    insert_mapped(mailbox, entry, &payload_hash).await
}

/// The one place a typed COMMAND becomes a [`MailboxEntry`] (kind COMMAND). Shared by
/// [`enqueue_worker_command`] and the generated typed actor clients (one crate per actor since
/// phase 2, reaching it via [`crate::ActorDoor`]), so the two doors cannot drift on lane, partition
/// or envelope columns — the clients only assemble typed inputs and delegate here.
pub(crate) fn command_entry(
    actor_type: &str,
    actor_id: uuid::Uuid,
    command_type: &str,
    payload: serde_json::Value,
    env: Envelope,
) -> Result<MailboxEntry, DomainError> {
    // #596: the lane comes from `declared_lane`, and this constructor no longer TAKES a width.
    // It used to receive one from each caller — the typed door passed a literal the emitter had
    // written into every generated client, so the routing constant existed in as many copies as
    // there are actors. A parameter is a decision point, and the whole finding of #596 is that
    // deciding where a width comes from is what broke one-writer.
    let Some(partition) = crate::partition::declared_lane(actor_type, &actor_id) else {
        return Err(DomainError::Repository(format!(
            "'{actor_type}' is not a mailbox actor — command '{command_type}' has no lane"
        )));
    };
    let payload_hash = application::journal::payload_hash(&payload);
    Ok(MailboxEntry {
        message_id: env.message_id,
        kind: "COMMAND".into(),
        actor_type: actor_type.into(),
        actor_id,
        partition,
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
    })
}

/// One adapted inbound BUSINESS fact, ready for the mailbox (kind EVENT). The producer names the
/// lane: `actor_type` must be a mailbox actor; `actor_id` is the addressed instance (or a stable
/// UUIDv5 surrogate when the aggregate id is not a uuid — e.g. `Payment-<intentId>` lanes). The
/// dedupe identity is `(source, external_id)`: `message_id = UUIDv5(source:external_id)` in the
/// inbound namespace, so a webhook redelivery collides on the pk instead of double-applying.
///
/// Reachable outside this crate ONLY through the `bulk-door` cargo feature (#290 review
/// BLOCKING-1a), as the input of [`enqueue_inbound_facts`] — and only `infrastructure` may enable
/// that feature (guard test `bulk_door_feature_is_granted_only_to_infrastructure`; its one
/// producer is the SIRENE sweep). It cannot set kind, identity, partition or principal — those
/// are derived by the shared [`inbound_entry`] constructor, exactly as for a typed client
/// `record` — and its `event_type` is validated against the actor's declared `receives` at the
/// door. Singular producers hold TYPED facts and go through a client's `record`.
/// Always `pub` because the shared constructors and the door take it, but only RE-EXPORTED under
/// `bulk-door` — so in a build that does not light that feature it is unreachable from the crate
/// root and the boundary crate's `unreachable_pub = deny` fires. Phase 2 (#306) made that build
/// configuration real: a client crate's dependency graph is `actor_client` with no features at
/// all, whereas before only the whole workspace (where `infrastructure` lights the feature) was
/// ever compiled. The item is deliberately not feature-gated itself — `record` needs it in every
/// configuration.
#[cfg_attr(
    not(any(feature = "bulk-door", feature = "test-fixtures", test)),
    allow(unreachable_pub)
)]
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

/// Enqueue one adapted inbound fact. The generated clients' `record` delegates here through
/// [`crate::ActorDoor::record_fact`] (and the drift guard compares against it); adapters record
/// TYPED facts through those clients.
///
/// Declared `pub` with the RE-EXPORT cfg-gated (rather than a cfg-duplicated `pub`/`pub(crate)`
/// pair) so there is exactly one definition: outside `test-fixtures` it is unreachable from the
/// crate root and stays crate-internal in practice, which is what the allow below records.
///
/// **What this shape buys, exactly — read before copying it (#597).** It makes the item unreachable
/// from OUTSIDE the crate. It does NOT make it unreachable from inside: Rust privacy is
/// hierarchical, so a `pub` item in a private module is nameable by every descendant of that
/// module's parent — every sibling module keeps the path `super::enqueue::enqueue_inbound_fact`.
/// That is correct HERE, because the constraint is "no other crate may build a mailbox row" and the
/// siblings that can reach it are the door itself. It is the WRONG shape when the constraint names
/// a caller in this crate ("no delivery route may decide when a placement counts"): there the item
/// must be a plain private `fn` in the module that owns it, and the out-of-crate test must drive a
/// PUBLIC function that already calls it rather than get a seam of its own — a cfg-gated
/// `*_spy` seam is another `pub` in a private module and reopens the same door in the build where
/// it exists (`infrastructure::mailbox::flush` is the worked example: no seam, the #456 proof
/// drives `flush_staged_in_tx`). Either way, prove it: write the violating call, compile it, keep
/// the rustc error, in every configuration you claim — a privacy change that compiles when violated
/// has done nothing (PROP-20260802-130500 §1).
#[cfg_attr(not(any(test, feature = "test-fixtures")), allow(unreachable_pub))]
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
///
/// The RECEIVES check (#290 review BLOCKING-1b): the typed clients prove "this actor records this
/// fact" at COMPILE time through the sealed `{Actor}Fact` traits; this constructor is also fed by
/// the UNTYPED bulk door, so it re-proves membership at runtime against the generated
/// [`ACTOR_INBOUND_FACTS`] table (the same actors.yaml `receives` scan the traits come from). An
/// undeclared (actor, event) pair is refused at the door — the interim containment while the D8
/// typed-batch API stays deferred.
pub(crate) fn inbound_entry(fact: InboundFact) -> Result<MailboxEntry, DomainError> {
    let Some(partition) = crate::partition::declared_lane(&fact.actor_type, &fact.actor_id) else {
        return Err(DomainError::Repository(format!(
            "'{}' is not a mailbox actor — inbound fact '{}' has no lane",
            fact.actor_type, fact.event_type
        )));
    };
    let received = ACTOR_INBOUND_FACTS
        .iter()
        .find(|(a, _)| *a == fact.actor_type)
        .is_some_and(|(_, facts)| facts.contains(&fact.event_type.as_str()));
    if !received {
        return Err(DomainError::Repository(format!(
            "'{}' does not receive inbound fact '{}' (actors.yaml `receives`) — refusing an \
             undeclared event type at the mailbox door",
            fact.actor_type, fact.event_type
        )));
    }
    // TAG COHERENCE (#290 re-review): delivery routes on the payload's adjacent `eventType` tag,
    // while the row's `message_type` column comes from `fact.event_type` — if they disagree, the
    // row lies about what it carries and the receives check above proved the wrong thing. The
    // typed `record` path cannot diverge (the tag is serialized FROM `into_domain_event`); the
    // untyped bulk path must prove it here.
    let tag = fact.payload.get("eventType").and_then(|t| t.as_str());
    if tag != Some(fact.event_type.as_str()) {
        return Err(DomainError::Repository(format!(
            "inbound fact '{}' carries payload tag {:?} — the row's message_type and the \
             delivery route would disagree; refusing the incoherent fact at the mailbox door",
            fact.event_type, tag
        )));
    }
    let message_id = inbound_message_id(&fact.source, &fact.external_id);
    let payload_hash = application::journal::payload_hash(&fact.payload);
    Ok(MailboxEntry {
        message_id,
        kind: "EVENT".into(),
        actor_type: fact.actor_type.clone(),
        actor_id: fact.actor_id,
        partition,
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
/// The D8-deferred BULK door (PROP-20260728-152752 — no batched client API): its ONE sanctioned
/// producer is the SIRENE sweep in `infrastructure`. INTERIM CONTAINMENT until the typed-batch
/// design lands (#290 review BLOCKING-1): (a) reachable outside this crate only through the
/// `bulk-door` cargo feature, which the guard test allows `infrastructure` alone to enable — a
/// loud, D3-style manifest grant (cargo feature unification means a sibling crate in the same
/// build could technically NAME the item once infrastructure lit the feature, which is exactly
/// why the guard fails the MANIFEST grant, the reviewable act); (b) every fact's `event_type` is
/// validated against the target actor's declared `receives` in [`inbound_entry`] — the runtime
/// re-proof of what the sealed `{Actor}Fact` traits prove at compile time on the typed path.
/// Unreachable AND uncalled when `bulk-door` is off (see [`InboundFact`] for why that build now
/// exists): the feature is what exports it, and `infrastructure` is the only crate that may light
/// it. Both allows are scoped to the feature-off configuration so that lighting the feature still
/// gets the full lint treatment.
#[cfg_attr(not(feature = "bulk-door"), allow(unreachable_pub, dead_code))]
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
        mailbox.insert_many(&entries, MailboxAccess::granted()).await?.into_iter().collect();
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
    /// The identity was still SCHEDULED under `reschedule: keep` (#167): the first occurrence
    /// stands untouched — a deadline that re-declaring never extends.
    Kept,
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
/// upsert (`infrastructure::mailbox::apply_schedules_in_tx`); this remains the reference
/// implementation for the reminder row shape, exercised by infrastructure's DB-gated
/// `mailbox_schedule_pg` tests against the real DDL — hence `test-fixtures`, not `cfg(test)`.
#[cfg(any(test, feature = "test-fixtures"))]
pub async fn schedule_reminder(
    mailbox: &dyn Mailbox,
    actor_type: &str,
    actor_id: uuid::Uuid,
    reminder_name: &str,
    payload_event_tagged: serde_json::Value,
    scheduled_at: chrono::DateTime<chrono::Utc>,
    policy: crate::mailbox::ReschedulePolicy,
    correlation_id: uuid::Uuid,
) -> Result<ScheduleOutcome, DomainError> {
    let Some(partition) = crate::partition::declared_lane(actor_type, &actor_id) else {
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
        partition,
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
    schedule_mapped(mailbox, entry, scheduled_at, policy, &payload_hash).await
}

/// Schedule one entry and map the port outcome onto [`ScheduleOutcome`] — shared by
/// [`schedule_reminder`] and the generated typed actor clients, so both scheduling doors carry the
/// same replay-vs-conflict contract.
pub(crate) async fn schedule_mapped(
    mailbox: &dyn Mailbox,
    entry: MailboxEntry,
    scheduled_at: chrono::DateTime<chrono::Utc>,
    policy: crate::mailbox::ReschedulePolicy,
    payload_hash: &str,
) -> Result<ScheduleOutcome, DomainError> {
    match mailbox.schedule(&entry, scheduled_at, policy, MailboxAccess::granted()).await? {
        MailboxScheduleOutcome::Scheduled => Ok(ScheduleOutcome::Scheduled),
        MailboxScheduleOutcome::Rescheduled => Ok(ScheduleOutcome::Rescheduled),
        MailboxScheduleOutcome::Kept => Ok(ScheduleOutcome::Kept),
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
/// TEST-ONLY since #284 slice 3: the typed clients' `cancel_scheduling` is the public withdrawal door; this
/// remains the reference the DB-gated `mailbox_schedule_pg` tests exercise against the real DDL.
#[cfg(any(test, feature = "test-fixtures"))]
pub async fn cancel_reminder(
    mailbox: &dyn Mailbox,
    actor_id: uuid::Uuid,
    reminder_name: &str,
) -> Result<bool, DomainError> {
    mailbox
        .cancel_scheduled(reminder_message_id(actor_id, reminder_name), MailboxAccess::granted())
        .await
}

/// Withdraw one SCHEDULED row through the port — the delegate behind every generated
/// `{Actor}Client::cancel_scheduling`.
///
/// It exists so that NO generated per-actor client ever mints a [`MailboxAccess`] itself (#304,
/// asserted by `every_mailbox_port_method_demands_the_access_witness`). Every other client method already
/// delegated to [`insert_mapped`]/[`schedule_mapped`]; `cancel_scheduling` was the one that spoke
/// to the port directly, which would have been the single line that failed to compile when
/// PROP-20260802-130500 phase 2 moves each client into its own crate — and the "fix" there is to
/// widen the mint, which is exactly the level-4 → level-3 slide the witness exists to prevent.
/// With the mint kept in this core module, phase 2 only has to widen these delegates.
pub(crate) async fn cancel_scheduled_mapped(
    mailbox: &dyn Mailbox,
    message_id: uuid::Uuid,
) -> Result<bool, DomainError> {
    mailbox.cancel_scheduled(message_id, MailboxAccess::granted()).await
}

/// Insert one entry and map the port outcome onto [`EnqueueOutcome`] — shared by the free-function
/// enqueue helpers and the generated typed actor clients.
pub(crate) async fn insert_mapped(
    mailbox: &dyn Mailbox,
    entry: MailboxEntry,
    payload_hash: &str,
) -> Result<EnqueueOutcome, DomainError> {
    match mailbox.insert(&entry, MailboxAccess::granted()).await? {
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

// ================================================================================================
// Tests
// ================================================================================================

/// The one door guard that still belongs INSIDE the crate: it is about the UNTYPED bulk path, so
/// it needs no typed client and gains nothing from crossing the crate line. Its typed siblings
/// moved to `tests/drift_guard.rs` with phase 2 (#306), where they exercise the per-actor client
/// crates exactly as a consumer does.
#[cfg(test)]
mod bulk_door_guard {
    use crate::mailbox::mem::MemMailbox;

    use super::{enqueue_inbound_fact, InboundFact};

    /// BLOCKING-1b invariant (#290 review): the UNTYPED bulk door must refuse an event type the
    /// target actor does not `receive` — singular and batched forms alike, since both feed the
    /// shared `inbound_entry`. Without this, the bulk path would skip the membership check the
    /// sealed `{Actor}Fact` traits enforce on the typed path, and an arbitrary event type could
    /// be parked on an arbitrary lane. Nothing may reach the mailbox on a refusal.
    #[tokio::test]
    async fn an_undeclared_inbound_fact_is_refused_at_the_door() {
        let actor_id = super::surrogate_actor_id("Payment", "pi_bad");
        let bad = |event_type: &str, actor_type: &str| InboundFact {
            source: "stripe".into(),
            external_id: "evt_bad".into(),
            event_type: event_type.into(),
            payload: serde_json::json!({ "eventType": event_type, "payload": {} }),
            correlation_id: uuid::Uuid::from_u128(0xC0),
            actor_type: actor_type.into(),
            actor_id,
        };

        // Payment does NOT receive OrderPlaced: the singular form refuses...
        let mailbox = MemMailbox::default();
        let err = enqueue_inbound_fact(&mailbox, bad("OrderPlaced", "Payment"))
            .await
            .expect_err("an undeclared (actor, event) pair must refuse");
        assert!(err.to_string().contains("does not receive"), "the error names the check: {err}");
        assert!(mailbox.entries().is_empty(), "nothing reaches the mailbox on a refusal");

        // ...and the batched form refuses the WHOLE batch (all-or-nothing entry construction).
        let err = super::enqueue_inbound_facts(
            &mailbox,
            vec![bad("PaymentCaptured", "Payment"), bad("OrderPlaced", "Payment")],
        )
        .await
        .expect_err("a batch carrying one undeclared fact must refuse");
        assert!(err.to_string().contains("does not receive"), "{err}");
        assert!(mailbox.entries().is_empty(), "a refused batch enqueues nothing");

        // TAG COHERENCE (#290 re-review): a RECEIVED event_type whose payload carries a DIFFERENT
        // adjacent tag must refuse too — delivery routes on the tag, so accepting it would land a
        // row whose message_type lies about what the worker will actually deserialize.
        let mut incoherent = bad("PaymentCaptured", "Payment");
        incoherent.payload = serde_json::json!({ "eventType": "PaymentFailed", "payload": {} });
        let err = enqueue_inbound_fact(&mailbox, incoherent)
            .await
            .expect_err("a tag/event_type mismatch must refuse");
        assert!(err.to_string().contains("payload tag"), "the error names the mismatch: {err}");
        assert!(mailbox.entries().is_empty(), "nothing reaches the mailbox on a refusal");
    }
}
