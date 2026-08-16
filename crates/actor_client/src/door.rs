//! The OPAQUE mailbox door (PROP-20260802-130500 phase 2, #306) — what a per-actor client crate
//! holds instead of the entry constructors.
//!
//! WHY THIS EXISTS. Phase 2 moves every `{Actor}Client` out of this crate into its own
//! `crates/clients/{actor}` crate, so a dependent's `Cargo.toml` names exactly the actors it may
//! address. Those crates must be able to ENQUEUE, and the two things enqueuing needs —
//! [`crate::mailbox::MailboxEntry`] (private fields) and [`crate::mailbox::MailboxAccess`]
//! (`pub(crate)` mint) — are precisely what D1 keeps inside this crate. The proposal named the two
//! exits: widen `command_entry`/`inbound_entry` to `pub`, or hand out an OPAQUE facade. Widening
//! is the level-4 → level-3 slide the witness exists to prevent (it is the assertion
//! `every_mailbox_port_method_demands_the_access_witness` fails on), so the facade is what phase 2
//! ships: [`ActorDoor`] takes primitives, builds the row HERE, mints the witness HERE, and hands
//! back only an outcome. No entry, no witness and no `Arc<dyn Mailbox>` method ever crosses the
//! crate line.
//!
//! WHAT THIS COSTS, STATED PLAINLY. `ActorDoor` is a PUBLIC, string-keyed door: its `actor_type` /
//! `message_type` are `&str`, so naming it is enough to address any actor with any message —
//! exactly what the sealed `{Actor}Command`/`{Actor}Fact` traits make impossible on the typed
//! path. Before phase 2 the equivalent capability (`command_entry`) was `pub(crate)` and no such
//! bypass existed. That is a real widening, and it is contained the way this repo contains
//! widenings it cannot buy at compile time: the codegen guard
//! `actor_door_is_named_only_by_generated_client_crates` fails the build if any code outside the
//! GENERATED client crates names this type. Level 3 (executable gate) for the addressing,
//! level 4 (compiler) still for the entry and the witness — see ADR-20260803-214500.

use std::sync::Arc;

use domain::shared::errors::DomainError;

use crate::enqueue::{
    cancel_scheduled_mapped, command_entry, declared_identity, enqueue_inbound_fact, insert_mapped,
    schedule_mapped, InboundFact,
};
use crate::mailbox::{Envelope, Mailbox};
use crate::{EnqueueOutcome, ScheduleOutcome};

/// The one handle a generated per-actor client crate holds. Wraps the mailbox port so that the
/// row construction and the `MailboxAccess` mint both stay in this crate: a client crate assembles
/// TYPED inputs, proves addressing, and delegates here.
pub struct ActorDoor {
    mailbox: Arc<dyn Mailbox>,
}

impl ActorDoor {
    /// A door over this mailbox. Held by exactly one `{Actor}Client`, which pairs it with the
    /// `actor_type`/width its own generated code hard-codes from actors.yaml.
    pub fn new(mailbox: Arc<dyn Mailbox>) -> Self {
        Self { mailbox }
    }

    /// Enqueue one COMMAND row (kind COMMAND) — the delegate behind every generated
    /// `{Actor}Client::send`. The entry is built by the shared [`command_entry`] constructor, so a
    /// typed send still produces the row `enqueue_worker_command` builds for the same inputs,
    /// field for field (the out-of-crate drift guard in `tests/drift_guard.rs` proves it across
    /// the new crate line).
    pub async fn send_command(
        &self,
        actor_type: &str,
        width: u16,
        actor_id: uuid::Uuid,
        message_type: &str,
        payload: serde_json::Value,
        env: Envelope,
    ) -> Result<EnqueueOutcome, DomainError> {
        let entry = command_entry(actor_type, width, actor_id, message_type, payload, env);
        let payload_hash = entry.payload_hash.clone();
        insert_mapped(self.mailbox.as_ref(), entry, &payload_hash).await
    }

    /// Park one COMMAND row for LATER delivery (`position` NULL) — the delegate behind
    /// `{Actor}Client::schedule`. Same entry construction as [`Self::send_command`]: a scheduled
    /// row and an immediate row for the same command differ only in when they are delivered.
    pub async fn schedule_command(
        &self,
        actor_type: &str,
        width: u16,
        actor_id: uuid::Uuid,
        message_type: &str,
        payload: serde_json::Value,
        env: Envelope,
        at: chrono::DateTime<chrono::Utc>,
    ) -> Result<ScheduleOutcome, DomainError> {
        let entry = command_entry(actor_type, width, actor_id, message_type, payload, env);
        let payload_hash = entry.payload_hash.clone();
        // A scheduled COMMAND keeps the historical in-place semantics — `keep` exists for
        // reminder deadlines (#167), and no command door declares a policy today.
        schedule_mapped(
            self.mailbox.as_ref(),
            entry,
            at,
            crate::mailbox::ReschedulePolicy::InPlace,
            &payload_hash,
        )
        .await
    }

    /// Withdraw one SCHEDULED row — the delegate behind `{Actor}Client::cancel_scheduling`. This
    /// is the method #304 kept in `crate::enqueue` precisely so phase 2 would not have to widen
    /// the mint: the witness is minted inside [`cancel_scheduled_mapped`], never out here.
    pub async fn cancel_scheduling(&self, message_id: uuid::Uuid) -> Result<bool, DomainError> {
        cancel_scheduled_mapped(self.mailbox.as_ref(), message_id).await
    }

    /// Record one inbound FACT (kind EVENT, channel EXTERNAL) — the delegate behind
    /// `{Actor}Client::record`. `tagged` is the adjacently-tagged `DomainEvent` form the caller
    /// obtained from the domain enum itself; identity, partition, principal and channel are all
    /// derived by the shared `inbound_entry` constructor, and the `(actor_type, event_type)` pair
    /// is re-proved against the actor's declared `receives` there.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_fact(
        &self,
        actor_type: &str,
        actor_id: uuid::Uuid,
        event_type: &str,
        tagged: serde_json::Value,
        source: &str,
        external_id: &str,
        correlation_id: uuid::Uuid,
    ) -> Result<EnqueueOutcome, DomainError> {
        enqueue_inbound_fact(
            self.mailbox.as_ref(),
            InboundFact {
                source: source.to_owned(),
                external_id: external_id.to_owned(),
                event_type: event_type.to_owned(),
                payload: tagged,
                correlation_id,
                actor_type: actor_type.to_owned(),
                actor_id,
            },
        )
        .await
    }

    /// The addressing invariant, with the SAME `declared_identity` derivation every other door
    /// uses: when the command declares an identity property, its value MUST equal the client's
    /// lane. A mismatch would park the command on one lane while the handler acts on another
    /// aggregate — per-aggregate serialization silently broken, which is the failure the mailbox
    /// exists to prevent. `client` names the calling client type so the error points at the lane
    /// that refused.
    pub fn assert_addressed(
        &self,
        client: &str,
        actor_id: uuid::Uuid,
        message_type: &str,
        payload: &serde_json::Value,
    ) -> Result<(), DomainError> {
        match declared_identity(message_type, payload)? {
            Some(id) if id != actor_id => Err(DomainError::Repository(format!(
                "{message_type}: payload identity {id} does not match this {client} lane \
                 {actor_id} — refusing a mis-addressed send"
            ))),
            _ => Ok(()),
        }
    }
}
