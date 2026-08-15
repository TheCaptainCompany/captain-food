//! Emit ONE CRATE PER MAILBOX ACTOR under `crates/clients/` — `client-restaurant`,
//! `client-cart`, …, including `client-place-order-process` and `client-refund-process`
//! (PROP-20260802-130500 phase 2, #306; the per-actor scope covering both process managers is the
//! product-owner directive of 2026-08-02).
//!
//! WHAT THE SPLIT BUYS. Before phase 2 the 17 typed clients shared one crate, so any crate
//! depending on `actor_client` could address every actor — the "who may address which actor"
//! level-1 row of the proposal's §2 table. Now the CRATE IS THE PERMISSION at actor granularity:
//! an adapter's `Cargo.toml` names exactly the actors it may reach, and an actor it does not
//! declare is an actor whose client type does not exist for it. Adding one is a manifest diff —
//! the loud, reviewable act.
//!
//! WHAT STAYS BEHIND. The per-actor crates cannot build a `MailboxEntry` (private fields) or mint
//! a `MailboxAccess` (`pub(crate)`), and phase 2 deliberately does NOT widen either — that is the
//! level-4 → level-3 slide #304 installed the witness to prevent. They enqueue through
//! `actor_client::ActorDoor`, the opaque facade the proposal's §6 named as the recommended exit.
//! Per-actor SEALED marker traits still make "send a message the actor does not `receive`" a
//! COMPILE error, and each crate now owns its own `sealed::Sealed`, so an actor's receive set is
//! not even nameable from a sibling client crate.
//!
//! Also emits `crates/actor_client/src/generated/addresses.rs` — the frozen command-addressing
//! tables (`mailbox_address`, `ACTOR_MAILBOXES`) the constructors and the composition root share;
//! the infrastructure `command_router` re-exports them so worker-side consumers keep one path.

use crate::*;

/// One mailbox actor's typed surface, scanned from actors.yaml: its frozen partition width and its
/// `receives` split by ref kind — `commands.yaml#/…` → COMMAND, `events.yaml#/…` → inbound FACT
/// (the ref path encodes the kind, §1b).
struct ClientActor {
    name: String,
    width: u16,
    commands: Vec<String>,
    facts: Vec<String>,
    /// The actor declares ≥1 reminder in actors.yaml (`reminders:`) — the SPEC permission for the
    /// scheduling surface (product-owner directive, 2026-08-02: no `schedule` method exposed
    /// without a usage declaration in the spec; `cancel_scheduling` withdraws only what
    /// `schedule`/the reminder upsert could have parked, so it rides the same condition).
    reminders: bool,
    /// The actor's declared `answers:` operations (PROP-20260815-142349 §7, #582) — the SPEC
    /// permission for the typed `ask` surface: trait + method exist IFF non-empty, absent
    /// otherwise (never uncallable-but-present).
    answers: Vec<AnswerOp>,
}

/// Every actor with a declared mailbox, sorted by name (stable output), receives de-duplicated.
/// Skips the file-level `version`/`description` keys and the `principals` map like every other
/// actors.yaml consumer.
fn client_actors(model: &Model) -> Vec<ClientActor> {
    let mut out: Vec<ClientActor> = Vec::new();
    let mut all_answers = answer_ops(model);
    let Some(Value::Mapping(actors)) = model.defs.get("actors.yaml") else { return out };
    for (k, def) in actors {
        let Some(name) = k.as_str().filter(|s| *s != "principals") else { continue };
        if !matches!(def.get("type").and_then(|t| t.as_str()), Some("aggregate" | "process-manager"))
        {
            continue;
        }
        let Some(width) =
            def.get("mailbox").and_then(|m| m.get("partitions")).and_then(|p| p.as_u64())
        else {
            continue;
        };
        let mut commands: Vec<String> = Vec::new();
        let mut facts: Vec<String> = Vec::new();
        if let Some(receives) = def.get("receives").and_then(|r| r.as_sequence()) {
            for entry in receives {
                let Some(r) =
                    entry.get("message").and_then(|m| m.get("$ref")).and_then(|r| r.as_str())
                else {
                    continue;
                };
                if let Some(cmd) = r.strip_prefix("commands.yaml#/") {
                    if !commands.iter().any(|c| c == cmd) {
                        commands.push(cmd.to_string());
                    }
                } else if let Some(ev) = r.strip_prefix("events.yaml#/") {
                    if !facts.iter().any(|f| f == ev) {
                        facts.push(ev.to_string());
                    }
                }
            }
        }
        commands.sort();
        facts.sort();
        let reminders = def
            .get("reminders")
            .and_then(|r| r.as_mapping())
            .is_some_and(|m| !m.is_empty());
        let (answers, rest): (Vec<AnswerOp>, Vec<AnswerOp>) =
            all_answers.drain(..).partition(|a| a.actor == name);
        all_answers = rest;
        out.push(ClientActor {
            name: name.to_string(),
            width: width as u16,
            commands,
            facts,
            reminders,
            answers,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// One generated per-actor client crate: where it lives, and the two files that make it. Both are
/// GENERATED — the manifest as much as the code, so "which actors exist" and "which crates exist"
/// cannot drift (the `check-drift` gate diffs the whole tree).
pub(crate) struct ClientCrate {
    /// The actors.yaml key this crate is the door to, e.g. `RestaurantAccount`.
    pub(crate) actor: String,
    /// Directory relative to the repo root, e.g. `crates/clients/restaurant-account`.
    pub(crate) dir: String,
    pub(crate) manifest: String,
    pub(crate) lib: String,
}

/// `RestaurantAccount` -> `restaurant-account`: the crate name and directory spelling. Cargo maps
/// the dashes back to underscores for the `use` path (`client_restaurant_account`).
pub(crate) fn kebab(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i != 0 {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// `RestaurantAccount` -> `client_restaurant_account`: the crate as a Rust path segment, i.e. what
/// a consumer writes in a `use`. Cargo turns the manifest's dashes into underscores, so this is the
/// one place that mapping is spelled out — generated consumers (the GraphQL resolvers) call it
/// rather than re-deriving it, so the manifest name and the import cannot drift.
pub(crate) fn client_crate_ident(actor: &str) -> String {
    format!("client_{}", kebab(actor).replace('-', "_"))
}

/// The manifest of one client crate. Deliberately minimal: `domain` for the message types,
/// `actor_client` for the door, serde/uuid to build the payload — NO sqlx, NO reqwest, and no
/// dependency on any sibling client crate. `chrono` appears only for an actor that earned the
/// scheduling surface, so `cargo machete` stays green.
fn client_manifest(a: &ClientActor) -> String {
    let actor = &a.name;
    let k = kebab(actor);
    let chrono = if a.reminders {
        "\n# Only a `reminders:` actor gets `schedule`, whose signature names a timestamp.\nchrono = { workspace = true }"
    } else {
        ""
    };
    let application = if a.answers.is_empty() {
        ""
    } else {
        "\n# Only an `answers:` actor gets `ask` (PROP-20260815-142349 s7, #582): its LOCAL adapter\n# reads the instance's stream through the application EventStore PORT (outer -> inner is the\n# legal direction; actor_client already sits above application in the graph). No transport --\n# the actor declares no topology key, and its absence MEANS local (D6).\napplication = { path = \"../../application\" }"
    };
    format!(
        r#"# GENERATED by the Captain.Food codegen from specs/actors.yaml -- do not edit by hand
# (PROP-20260802-130500 phase 2, #306).
#
# ONE CRATE PER MAILBOX ACTOR: depending on this crate is the permission to address the `{actor}`
# actor -- and nothing else. An actor a crate does not name in its manifest is an actor whose
# client type does not exist for it, so widening reach is a Cargo.toml diff (loud, reviewable)
# rather than an import nobody notices.
#
# This crate cannot build a mailbox row or mint a `MailboxAccess`: both stay private to
# `actor_client`, and everything here enqueues through the opaque `ActorDoor`. That is what keeps
# the entry and the witness at compile-time enforcement while the split happens around them.
[package]
name = "client-{k}"
version = "0.1.0"
edition.workspace = true
license-file.workspace = true
publish = false
description = "Captain.Food typed mailbox client for the `{actor}` actor -- the write door to its lane, and the permission to address it (PROP-20260802-130500 phase 2)."

[dependencies]
actor_client = {{ path = "../../actor_client" }}
domain = {{ path = "../../domain" }}
serde = {{ workspace = true }}
serde_json = {{ workspace = true }}
uuid = {{ workspace = true }}{chrono}{application}

# D6 lint floor (PROP-20260802-130500, #302). A client crate is a BOUNDARY crate -- it exists to
# be the door -- so it restates the workspace baseline and additionally denies `unreachable_pub`.
[lints.rust]
unsafe_code = "forbid"
unreachable_pub = "deny"
"#
    )
}

/// The `src/lib.rs` of one client crate: the sealed marker traits for what this actor receives,
/// and the client that enqueues through the door.
fn client_lib(a: &ClientActor) -> String {
    let actor = &a.name;
    let width = a.width;
    let mut out = String::new();

    out.push_str(&HEADER.replace("{ACTOR}", actor));

    // Imports follow the SAME spec gating as the methods, so a crate never carries an import its
    // actor's declarations did not earn — `Envelope` is a send/schedule parameter, `ScheduleOutcome`
    // a reminders-only return. An unused import would be a warning in every consumer's build, and
    // (worse) a hint that the surface and its declarations have drifted apart.
    out.push_str(if a.commands.is_empty() {
        "use actor_client::mailbox::Mailbox;\n"
    } else {
        "use actor_client::mailbox::{Envelope, Mailbox};\n"
    });
    out.push_str(if a.reminders {
        "use actor_client::{ActorDoor, EnqueueOutcome, ScheduleOutcome};\n"
    } else {
        "use actor_client::{ActorDoor, EnqueueOutcome};\n"
    });
    out.push_str("use domain::shared::errors::DomainError;\n");

    out.push_str(SEALED);
    for c in &a.commands {
        out.push_str(&format!("impl sealed::Sealed for domain::generated::commands::{c} {{}}\n"));
    }
    for f in &a.facts {
        out.push_str(&format!("impl sealed::Sealed for domain::generated::events::{f} {{}}\n"));
    }
    for ans in &a.answers {
        out.push_str(&format!(
            "impl sealed::Sealed for domain::generated::answers::{}Request {{}}\n",
            ans.pair
        ));
    }

    // THE DECLARATION IS THE PERMISSION (product-owner directive, 2026-08-02, generalized):
    // every client method -- and its sealed marker trait -- is emitted only when the actor's spec
    // declarations justify it. An unjustified surface is ABSENT (a compile error at any call
    // site), never uncallable-but-present.
    if !a.commands.is_empty() {
        out.push_str(&COMMAND_TRAIT.replace("{ACTOR}", actor));
        for c in &a.commands {
            out.push_str(&format!(
                "\nimpl {actor}Command for domain::generated::commands::{c} {{\n    const MESSAGE_TYPE: &'static str = \"{c}\";\n}}\n"
            ));
        }
    }
    if !a.facts.is_empty() {
        // The "same reason as [`{Actor}Command`]" cross-reference only resolves when that trait
        // was actually emitted — a facts-only actor (Payment) has no command trait, and a rustdoc
        // link to a type that does not exist in this crate is a broken link in generated code.
        let seal_why = if a.commands.is_empty() {
            "SEALED for the same reason: the private supertrait cannot be implemented downstream."
        } else {
            "SEALED for the same reason as [`{ACTOR}Command`]."
        };
        out.push_str(&FACT_TRAIT.replace("{SEAL_WHY}", seal_why).replace("{ACTOR}", actor));
        for f in &a.facts {
            out.push_str(&format!(
                "\nimpl {actor}Fact for domain::generated::events::{f} {{\n    const EVENT_TYPE: &'static str = \"{f}\";\n    fn into_domain_event(self) -> domain::generated::events::DomainEvent {{\n        domain::generated::events::DomainEvent::{f}(self)\n    }}\n}}\n"
            ));
        }
    }

    // The ASK surface (PROP-20260815-142349 §7, #582): trait + per-op impls, emitted only for a
    // declared `answers:` block — the declaration is the permission, same law as send/record.
    if !a.answers.is_empty() {
        let module = snake_type(actor);
        out.push_str(
            &ANSWER_TRAIT.replace("{ACTOR}", actor).replace("{MODULE}", &module),
        );
        for ans in &a.answers {
            let projections = ans
                .projections
                .iter()
                .map(|(reply_field, state_field)| {
                    format!("            {}: state.{}.clone(),\n", reply_field, state_field)
                })
                .collect::<String>();
            out.push_str(&format!(
                "\nimpl {actor}Answer for domain::generated::answers::{pair}Request {{\n    const QUERY_TYPE: &'static str = \"actors.yaml#/{actor}/answers/{op}\";\n    type Reply = domain::generated::answers::{pair}Reply;\n    fn stream_name(&self) -> String {{\n        format!(\"{actor}-{{}}\", self.{id}.0)\n    }}\n    fn project(state: &domain::{module}::{actor}State) -> Self::Reply {{\n        domain::generated::answers::{pair}Reply {{\n{projections}        }}\n    }}\n}}\n",
                actor = actor,
                pair = ans.pair,
                op = ans.op,
                id = ans.identity_rust,
                module = module,
                projections = projections,
            ));
        }
    }

    out.push_str(&CLIENT_STRUCT.replace("{ACTOR}", actor).replace("{WIDTH}", &width.to_string()));
    if !a.commands.is_empty() {
        out.push_str(&SEND.replace("{ACTOR}", actor).replace("{WIDTH}", &width.to_string()));
    }
    if !a.facts.is_empty() {
        out.push_str(&RECORD.replace("{ACTOR}", actor));
    }
    if a.reminders {
        out.push_str(&SCHEDULE.replace("{ACTOR}", actor).replace("{WIDTH}", &width.to_string()));
    }
    if !a.answers.is_empty() {
        out.push_str(&ASK.replace("{ACTOR}", actor).replace("{MODULE}", &snake_type(actor)));
    }
    out.push_str("}\n");
    out
}

/// The sealed per-actor ANSWER trait (PROP-20260815-142349 §7): what this actor serves on Ask.
const ANSWER_TRAIT: &str = r#"
/// GENERATED from actors.yaml `{ACTOR}.answers` (PROP-20260815-142349 s7, #582): marker for
/// every ask this actor ANSWERS -- the declaration is the permission, so this trait (and the
/// `ask` method) exist iff the spec declares `answers:`, and are ABSENT otherwise. SEALED like
/// the send/record traits: the answer set is the answer set, period. `Reply` carries
/// UNCONDITIONAL serde -- the wire shape may not rot while the ask is local -- and the reader
/// is TOLERANT (additive-only producer / tolerant reader; a breaking reshape takes a NEW
/// answer name).
pub trait {ACTOR}Answer: sealed::Sealed {
    /// The declared operation address -- the ref path IS the whole address (the actor is
    /// encoded in it, never restated).
    const QUERY_TYPE: &'static str;
    type Reply: serde::Serialize + serde::de::DeserializeOwned + Send;
    /// The answering instance's stream -- the identity IS the argument (PROP s3).
    fn stream_name(&self) -> String;
    /// Project the declared reply properties off the actor's own fold state. THE compiled
    /// YAML-state <-> hand-fold parity proof: rename a fold field and this stops building.
    fn project(state: &domain::{MODULE}::{ACTOR}State) -> Self::Reply;
}
"#;

/// The typed local `ask` (PROP-20260815-142349 s7): EventStore load -> fold -> project ->
/// envelope. No transport, no mailbox involvement.
const ASK: &str = r#"
    /// Ask this actor for a DECLARED answer (PROP-20260815-142349 s7, #582) -- the LOCAL
    /// adapter: load the instance's stream through the EventStore PORT, fold with the actor's
    /// own hand fold, project the declared reply, envelope with the fold's version. `Absent`
    /// (no birth event on the stream) and `Deadline` (the CALLER's deadline elapsed -- the
    /// deadline lives on the call site, never on this actor, D5) are Ok-channel arms the
    /// caller must match exhaustively; `Err` stays reserved for infrastructure failure. The
    /// reply's authority EXPIRES AT SEND: a fold gives freshness, not atomicity (CHK-1).
    pub async fn ask<Q: {ACTOR}Answer>(
        &self,
        q: Q,
        store: &dyn application::ports::EventStore,
        deadline: std::time::Duration,
    ) -> Result<actor_client::ask::AskOutcome<Q::Reply>, DomainError> {
        actor_client::ask::ask_local(store, &q.stream_name(), deadline, domain::{MODULE}::fold, Q::project)
            .await
    }
"#;

const HEADER: &str = r#"// GENERATED by the Captain.Food codegen from specs/actors.yaml (`receives` +
// `mailbox.partitions` + `reminders`) -- do not edit by hand (PROP-20260802-130500 phase 2,
// #306). The typed mailbox client for ONE actor, in its OWN crate: depending on this crate is the
// permission to address `{ACTOR}`, and nothing else.
//
// The client constructs NOTHING itself. Every row is assembled behind `actor_client::ActorDoor`,
// whose delegates own the lane, partition, principal and channel derivation -- so a typed send
// here and the string-keyed reference implementation in `actor_client` cannot drift (the
// out-of-crate drift guard in `crates/actor_client/tests/drift_guard.rs` proves it field for
// field). `MailboxEntry` and `MailboxAccess` are not nameable from this crate at all.
//
// EVERY method is spec-gated (product-owner directive, 2026-08-02 -- the declaration is the
// permission): `send` + `{ACTOR}Command` exist iff the actor receives >=1 COMMAND, `record` +
// `{ACTOR}Fact` iff it receives >=1 inbound FACT, `schedule`/`cancel_scheduling` iff it declares
// `reminders:`. An unjustified surface is ABSENT -- a compile error at any call site -- never
// uncallable-but-present.

use std::sync::Arc;

"#;

const SEALED: &str = r#"
/// The privacy SEAL: `Sealed` lives in a private module, so it cannot be named -- let alone
/// implemented -- outside this GENERATED crate. Every marker trait below requires it, so the
/// receive set this actor declares in actors.yaml is the receive set, period: widening it takes a
/// spec change and a regeneration, never a stray downstream `impl`. Since phase 2 each actor owns
/// its own seal, so a sibling client crate cannot even name this actor's marker supertrait.
mod sealed {
    pub trait Sealed {}
}

"#;

const COMMAND_TRAIT: &str = r#"
/// GENERATED from actors.yaml `{ACTOR}.receives`: marker for every COMMAND the `{ACTOR}` actor
/// receives. SEALED (private supertrait) so no impl can exist outside this generated crate --
/// sending a command this actor does not `receive` is a COMPILE error, not a runtime rejection.
pub trait {ACTOR}Command: sealed::Sealed + serde::Serialize + Send {
    /// The mailbox `message_type` (the commands.yaml key).
    const MESSAGE_TYPE: &'static str;
}
"#;

const FACT_TRAIT: &str = r#"
/// GENERATED from actors.yaml `{ACTOR}.receives`: marker for every inbound FACT (an
/// `events.yaml#/...` ref -- an external fact that already happened) the `{ACTOR}` actor records.
/// {SEAL_WHY}
pub trait {ACTOR}Fact: sealed::Sealed + serde::Serialize + Send {
    /// The mailbox `message_type` (the events.yaml key) -- also the `DomainEvent` adjacent tag.
    const EVENT_TYPE: &'static str;
    /// The fact wrapped in ITS `DomainEvent` variant -- so the adjacently-tagged wire form comes
    /// from the domain enum's own serde representation, never from a hand-built literal that
    /// could drift from it.
    fn into_domain_event(self) -> domain::generated::events::DomainEvent;
}
"#;

const CLIENT_STRUCT: &str = r#"
/// GENERATED from actors.yaml: the strongly-typed client for ONE `{ACTOR}` mailbox lane --
/// `actor_id` is the addressed instance, the partition is the FROZEN `stable_partition` over
/// width {WIDTH} (`mailbox.partitions`). The only door to this actor.
pub struct {ACTOR}Client {
    door: ActorDoor,
    actor_id: uuid::Uuid,
}

impl {ACTOR}Client {
    /// A client addressing the `{ACTOR}` instance `actor_id`.
    pub fn new(mailbox: Arc<dyn Mailbox>, actor_id: uuid::Uuid) -> Self {
        Self { door: ActorDoor::new(mailbox), actor_id }
    }
"#;

const SEND: &str = r#"
    /// Enqueue one typed COMMAND on this lane (kind COMMAND). Delegates to the door's
    /// `send_command`, which builds the row with the shared `command_entry` constructor -- the
    /// very row `enqueue_worker_command` builds for the same inputs, field for field.
    pub async fn send<M: {ACTOR}Command>(
        &self,
        msg: M,
        env: Envelope,
    ) -> Result<EnqueueOutcome, DomainError> {
        let payload = serde_json::to_value(&msg)
            .map_err(|e| DomainError::Repository(format!("{} payload: {e}", M::MESSAGE_TYPE)))?;
        self.door.assert_addressed("{ACTOR}Client", self.actor_id, M::MESSAGE_TYPE, &payload)?;
        self.door
            .send_command("{ACTOR}", {WIDTH}, self.actor_id, M::MESSAGE_TYPE, payload, env)
            .await
    }
"#;

const RECORD: &str = r#"
    /// Record one inbound FACT (kind EVENT, channel EXTERNAL, adjacently-tagged `DomainEvent`
    /// payload). `message_id` is ALWAYS the deterministic `inbound_message_id(source,
    /// external_id)` -- never caller-supplied -- and provenance is a REQUIRED parameter, because
    /// the `(source, external_id)` dedupe key is what makes redelivery safe. The acting principal
    /// is the per-source system user, stamped by the shared `inbound_entry` constructor.
    pub async fn record<F: {ACTOR}Fact>(
        &self,
        fact: F,
        source: &str,
        external_id: &str,
        correlation_id: uuid::Uuid,
    ) -> Result<EnqueueOutcome, DomainError> {
        let tagged = serde_json::to_value(fact.into_domain_event())
            .map_err(|e| DomainError::Repository(format!("{} payload: {e}", F::EVENT_TYPE)))?;
        self.door
            .record_fact(
                "{ACTOR}",
                self.actor_id,
                F::EVENT_TYPE,
                tagged,
                source,
                external_id,
                correlation_id,
            )
            .await
    }
"#;

const SCHEDULE: &str = r#"
    /// Schedule one typed COMMAND for LATER delivery: a SCHEDULED row (`position` NULL) the
    /// promotion pass delivers when due; a still-SCHEDULED identity reschedules IN PLACE
    /// (ADR-20260731-150500).
    pub async fn schedule<M: {ACTOR}Command>(
        &self,
        msg: M,
        env: Envelope,
        at: chrono::DateTime<chrono::Utc>,
    ) -> Result<ScheduleOutcome, DomainError> {
        let payload = serde_json::to_value(&msg)
            .map_err(|e| DomainError::Repository(format!("{} payload: {e}", M::MESSAGE_TYPE)))?;
        self.door.assert_addressed("{ACTOR}Client", self.actor_id, M::MESSAGE_TYPE, &payload)?;
        self.door
            .schedule_command("{ACTOR}", {WIDTH}, self.actor_id, M::MESSAGE_TYPE, payload, env, at)
            .await
    }

    /// Withdraw a SCHEDULED reminder (`SCHEDULED -> CANCELLED`, ADR-20260731-150500 section 3).
    /// `false` = absent, delivered or already cancelled -- the caller decides whether losing that
    /// race matters. Named `cancel_scheduling` (product-owner decision 2026-08-02, #308): the name
    /// says exactly what it withdraws -- a scheduled reminder, never an in-flight command. Keyed
    /// by `message_id` like the port beneath it: the id is minted by `schedule`, so holding it IS
    /// the capability (decided over lane-scoping, #308).
    pub async fn cancel_scheduling(&self, message_id: uuid::Uuid) -> Result<bool, DomainError> {
        self.door.cancel_scheduling(message_id).await
    }
"#;

/// Every generated client crate, one per mailbox actor, sorted by actor name (stable output).
pub(crate) fn emit_client_crates(model: &Model) -> Vec<ClientCrate> {
    let actors = client_actors(model);
    let mut out = Vec::new();
    for a in &actors {
        let actor = &a.name;
        // Latent edge, failed LOUDLY at generation (#290 re-review NOTE): `schedule`'s bound is
        // the `{Actor}Command` trait, which only exists with >=1 received command -- a facts-only
        // actor declaring `reminders:` would emit non-compiling code. If a spec ever gets here,
        // the fix is a spec decision, not a silent drop of the scheduling surface.
        assert!(
            !(a.reminders && a.commands.is_empty()),
            "actors.yaml: `{actor}` declares `reminders:` but receives no COMMAND — the generated \
             `schedule` method needs the `{actor}Command` trait, which only exists with >=1 \
             received command, so the generated crate would not compile. Declare a received \
             command or move the reminder."
        );
        out.push(ClientCrate {
            actor: actor.clone(),
            dir: format!("crates/clients/{}", kebab(actor)),
            manifest: client_manifest(a),
            lib: client_lib(a),
        });
    }
    out
}

/// Emit `crates/actor_client/src/generated/addresses.rs` — the frozen command-addressing tables,
/// moved out of the infrastructure `command_router` with #290 phase 1 (the entry constructors that
/// consume them live in `actor_client` now; the router re-exports them for worker-side callers so
/// there is exactly ONE definition).
pub(crate) fn emit_actor_addresses(model: &Model) -> String {
    let actors = client_actors(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/actors.yaml (`identity` + `mailbox.partitions`\n// + `receives`) — do not edit by hand (#290 phase 1, PROP-20260802-130500 D1). The frozen command\n// ADDRESSING of the mailbox: which actor receives a command, through which payload property its\n// instance id is read, over how many partitions. One definition, consumed by the entry\n// constructors in `crate::enqueue` and re-exported by `infrastructure::generated::command_router`\n// for the composition root — so no channel can address a command differently than another.\n",
    );
    let addressing = command_addressing(model);
    out.push_str(
        "\n/// A command's mailbox address: `(actor_type, identity payload key, partition width)`.\n/// `None` identity key = a birth command whose id the handler mints (the enqueue helper mints an\n/// addressing-only actor_id). `None` overall = a command no mailbox actor receives (the PM legs,\n/// until Runtime D).\npub fn mailbox_address(command_type: &str) -> Option<(&'static str, Option<&'static str>, u16)> {\n    match command_type {\n",
    );
    for (command, addr) in &addressing {
        let prop = match &addr.identity_prop {
            Some(p) => format!("Some(\"{}\")", p),
            None => "None".to_string(),
        };
        out.push_str(&format!(
            "        \"{}\" => Some((\"{}\", {}, {})),\n",
            command, addr.actor_type, prop, addr.partitions
        ));
    }
    out.push_str("        _ => None,\n    }\n}\n");
    // Scanned from the ACTOR CATALOG directly, not from the command map: an actor that receives
    // only inbound EVENTS (Payment) has a mailbox but no command address, and deriving the widths
    // from commands silently dropped its lane (found by the Stripe ACL tests).
    out.push_str(
        "\n/// Every actor type with a declared mailbox, and its partition WIDTH (actors.yaml\n/// `mailbox.partitions`) — the composition root spawns one MailboxWorker per entry and seeds the\n/// registry with these widths. The widths are part of the frozen routing contract: narrowing one\n/// re-routes in-flight rows.\npub const ACTOR_MAILBOXES: &[(&str, u16)] = &[\n",
    );
    for (actor, width) in &actor_mailbox_widths(model) {
        out.push_str(&format!("    (\"{}\", {}),\n", actor, width));
    }
    out.push_str("];\n");
    // The declared inbound-FACT set per mailbox actor — the same `receives` scan the sealed
    // `{Actor}Fact` traits are emitted from, rendered as data so the one UNTYPED path (the
    // bulk door) can re-prove membership at runtime (#290 review BLOCKING-1b).
    out.push_str(
        "\n/// Every mailbox actor's declared inbound FACT set (the `events.yaml#/…` refs in its\n/// actors.yaml `receives`) — the SAME scan the sealed `{Actor}Fact` marker traits are generated\n/// from. The typed clients prove membership at COMPILE time; this table exists so the one\n/// untyped path (`enqueue_inbound_facts`, the feature-gated bulk door) re-proves it at RUNTIME\n/// in `inbound_entry` — an undeclared (actor, event) pair is refused at the door, never\n/// enqueued. Actors that receive no inbound facts render an empty slice on purpose: absent and\n/// empty must both refuse.\npub const ACTOR_INBOUND_FACTS: &[(&str, &[&str])] = &[\n",
    );
    for a in &actors {
        let list = a
            .facts
            .iter()
            .map(|f| format!("\"{}\"", f))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("    (\"{}\", &[{}]),\n", a.name, list));
    }
    out.push_str("];\n");
    out
}

/// Every actor with a declared mailbox and its partition width, sorted (stable output) — shared
/// by the addresses emitter here and the ACTOR_ACTIVATIONS table the router keeps.
pub(crate) fn actor_mailbox_widths(model: &Model) -> BTreeMap<String, u16> {
    let mut widths: BTreeMap<String, u16> = BTreeMap::new();
    if let Some(Value::Mapping(actors)) = model.defs.get("actors.yaml") {
        for (k, def) in actors {
            let Some(actor) = k.as_str().filter(|s| *s != "principals") else { continue };
            if let Some(w) =
                def.get("mailbox").and_then(|m| m.get("partitions")).and_then(|p| p.as_u64())
            {
                widths.insert(actor.to_string(), w as u16);
            }
        }
    }
    widths
}
