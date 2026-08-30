//! Emit `crates/application/src/generated/inboxes.rs` — ONE `<Actor>Inbox` enum per mailbox actor,
//! GENERATED from that actor's `receives:` set in `specs/*/actors.yaml` (#771, founder directive
//! 2026-08-30: *"Go for the generated per-actor enum."*).
//!
//! WHAT THIS FIXES. `specs/*/actors.yaml` already declares, per actor, the exact set of messages it
//! receives — and until #771 the runtime threw that declaration away at the door: the router was a
//! flat `match` over a `&str` message type across ALL actors, ending in `_ => None` →
//! `FAILED "unroutable command type"`. Two failures followed, both silent at every existing gate:
//!
//! 1. **Unconsumed message.** A message an actor declares it receives, with no dispatch arm, shipped
//!    green. #595 hit it by hand: `PlaceReplacementOrder` had no arm and a replacement order was
//!    silently never born.
//! 2. **Cross-actor dispatch.** The router took `message.message_type` and never `message.actor_type`,
//!    so a row on lane A could drive a handler that writes aggregate B — under A's fence. That is
//!    ADR-20260829-230418 ("Aggregates own the facts") violated by the transport itself.
//!
//! THE DIVISION OF LABOUR — read this before adding anything here. **This emitter writes the ENUM.
//! It must NEVER write the routing `match`.** If one walk over the model generated both the variants
//! and the arms, the match would be exhaustive BY CONSTRUCTION and the compiler would catch exactly
//! nothing — the guard would be a decoration. The arms live in the HUMAN-OWNED
//! `crates/infrastructure/src/inbox.rs`, and `rustc` E0004 is what makes a new `receives:` entry
//! impossible to ignore. `tests.rs::a_widened_receives_set_is_a_compile_error` proves that guard RED
//! against real `rustc` output; a guard never seen red is an unverified claim.
//!
//! CLOSED ENUM ON DISPATCH, OPEN PARSE AT THE BOUNDARY. The wire stays a string (`inbound_messages`
//! is untouched — no stored shape and no wire format changes here). `ActorInbox::parse` is the single
//! fallible edge, and it takes the actor_type AND the message_type, so a row on lane A carrying B's
//! message cannot parse at all: the cross-actor hole closes in the type, not in a check.

use crate::*;

/// What kind of message an inbox variant is, from the ref path — the same encoding every other
/// actors.yaml consumer uses (`commands.yaml#/…` → COMMAND, `events.yaml#/…` → inbound FACT,
/// `#/<Actor>/reminders/<Name>` → a REMINDER this actor scheduled for itself).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum InboxKind {
    Command,
    Fact,
    Reminder,
}

impl InboxKind {
    fn rust(self) -> &'static str {
        match self {
            InboxKind::Command => "InboxKind::Command",
            InboxKind::Fact => "InboxKind::Fact",
            InboxKind::Reminder => "InboxKind::Reminder",
        }
    }
}

/// One variant of one actor's inbox.
#[derive(Clone, Debug)]
pub(crate) struct InboxMessage {
    /// The variant name — VERBATIM from the `$ref` (the commands.yaml/events.yaml key, or the
    /// reminder's own name). The wire `message_type` is this same string, so the enum reads as the
    /// spec reads.
    pub(crate) name: String,
    pub(crate) kind: InboxKind,
    /// The fully-qualified payload type the variant carries.
    pub(crate) payload_type: String,
    /// `deferred: { reason, issue }` on the `receives:` entry — the DSL successor of the retired
    /// `UNWIRED_MUTATIONS` const (#771). The declaration stays (the actor DOES receive this), so
    /// the variant and its arm both exist; what is deferred is the HANDLER, and the reason and
    /// tracking issue are reviewable spec content instead of a Rust const nobody reads.
    pub(crate) deferred: Option<(String, String)>,
}

/// One mailbox actor's inbox, scanned from actors.yaml.
#[derive(Clone, Debug)]
pub(crate) struct InboxActor {
    pub(crate) name: String,
    pub(crate) messages: Vec<InboxMessage>,
}

impl InboxActor {
    pub(crate) fn enum_name(&self) -> String {
        format!("{}Inbox", self.name)
    }
}

/// Resolve one `receives:` message `$ref` to `(variant name, kind, payload type)`.
///
/// A reminder ref (`#/<Actor>/reminders/<Name>`) resolves its payload through the reminder's own
/// `payload:` ref — the reminder NAME is the variant (it is what the wire carries as the promoted
/// row's message_type, ADR-20260731-153000 §1a) and the EVENT is the payload.
fn resolve_message(model: &Model, actor: &str, r: &str) -> Option<(String, InboxKind, String)> {
    if let Some(cmd) = r.strip_prefix("commands.yaml#/") {
        return Some((
            cmd.to_string(),
            InboxKind::Command,
            format!("domain::generated::commands::{cmd}"),
        ));
    }
    if let Some(ev) = r.strip_prefix("events.yaml#/") {
        return Some((
            ev.to_string(),
            InboxKind::Fact,
            format!("domain::generated::events::{ev}"),
        ));
    }
    // `#/<Actor>/reminders/<Name>` — intra-file, so the actor segment is the declaring actor.
    let rest = r.strip_prefix("#/")?;
    let mut parts = rest.split('/');
    let owner = parts.next()?;
    if parts.next()? != "reminders" {
        return None;
    }
    let reminder = parts.next()?;
    let payload = model
        .defs
        .get("actors.yaml")
        .and_then(|m| m.get(owner))
        .and_then(|d| d.get("reminders"))
        .and_then(|m| m.get(reminder))
        .and_then(|d| d.get("payload"))
        .and_then(|p| p.get("$ref"))
        .and_then(|r| r.as_str())
        .and_then(|r| r.strip_prefix("events.yaml#/"))?;
    debug_assert_eq!(owner, actor, "a reminder ref names its own declaring actor");
    let _ = reminder;
    // THE VARIANT IS THE PAYLOAD EVENT, NOT THE REMINDER NAME. A promoted reminder row carries
    // `message_type = spec.payload_event` (`actor_client::reminders::scheduled_entry`), so naming
    // the variant after the reminder would produce an enum `parse` could never match. They happen
    // to coincide for all three reminders declared today; that is a coincidence, not the contract.
    //
    // A reminder whose payload event the actor ALSO receives as a fact is therefore the same wire
    // triple and de-duplicates to one variant — correct, because the router cannot tell them apart
    // either, and the kind of the first declaration wins.
    Some((
        payload.to_string(),
        InboxKind::Reminder,
        format!("domain::generated::events::{payload}"),
    ))
}

/// Every actor with a declared mailbox and its de-duplicated `receives:` set, sorted by actor name
/// then message name so the emitted file is stable. Skips the file-level `version`/`description`
/// keys and the `principals` map like every other actors.yaml consumer.
pub(crate) fn inbox_actors(model: &Model) -> Vec<InboxActor> {
    let mut out: Vec<InboxActor> = Vec::new();
    let Some(Value::Mapping(actors)) = model.defs.get("actors.yaml") else { return out };
    for (k, def) in actors {
        let Some(name) = k.as_str().filter(|s| *s != "principals") else { continue };
        if !matches!(def.get("type").and_then(|t| t.as_str()), Some("aggregate" | "process-manager"))
        {
            continue;
        }
        if def.get("mailbox").and_then(|m| m.get("partitions")).and_then(|p| p.as_u64()).is_none() {
            continue;
        }
        let mut messages: Vec<InboxMessage> = Vec::new();
        if let Some(receives) = def.get("receives").and_then(|r| r.as_sequence()) {
            for entry in receives {
                let Some(r) =
                    entry.get("message").and_then(|m| m.get("$ref")).and_then(|r| r.as_str())
                else {
                    continue;
                };
                let Some((vname, kind, payload_type)) = resolve_message(model, name, r) else {
                    continue;
                };
                if messages.iter().any(|m| m.name == vname) {
                    continue;
                }
                // `deferred: { reason, issue }` — the DSL successor of UNWIRED_MUTATIONS. Shape is
                // validator-enforced (`receives-deferred-shape`); this read is deliberately total
                // so a malformed block cannot silently emit as "wired".
                let deferred = entry.get("deferred").and_then(|d| {
                    let reason = d.get("reason").and_then(|v| v.as_str())?;
                    let issue = d.get("issue").and_then(|v| v.as_str())?;
                    // A YAML folded scalar keeps its newlines; the reason is rendered into a `///`
                    // doc comment, where one newline would silently truncate it and turn the rest
                    // into a syntax error. Flatten to one line.
                    Some((reason.split_whitespace().collect::<Vec<_>>().join(" "), issue.to_string()))
                });
                messages.push(InboxMessage { name: vname, kind, payload_type, deferred });
            }
        }
        messages.sort_by(|a, b| a.name.cmp(&b.name));
        out.push(InboxActor { name: name.to_string(), messages });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The ENUM DECLARATION for one actor — deliberately separated from [`inbox_enum_impl`] so the
/// E0004 proof test can compile a mutated declaration against an unmutated arm set with no serde
/// dependency (`tests.rs::a_widened_receives_set_is_a_compile_error`).
///
/// NOT `#[non_exhaustive]`, ON PURPOSE: `#[non_exhaustive]` would FORCE every downstream match to
/// carry a wildcard arm, which is the precise opposite of what this enum exists to do. The router
/// lives in the same workspace, so there is no cross-crate-stability argument for it either.
pub(crate) fn inbox_enum_decl(actor: &InboxActor) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "/// GENERATED from `actors.yaml#/{name}/receives` — the CLOSED set of messages the `{name}`\n/// actor's ONE mailbox queue can carry, spanning every kind (COMMAND / inbound FACT / REMINDER),\n/// each variant carrying its typed payload.\n///\n/// Adding a `receives:` entry adds a variant here, and the human-owned `match` in\n/// `infrastructure::inbox` then fails to compile with E0004 until someone decides what the new\n/// message DOES. That compile error is the whole point: before #771 the same omission shipped\n/// green and surfaced as a `FAILED \"unroutable command type\"` row in production.\n#[derive(Debug, Clone, PartialEq)]\npub enum {enum_name} {{\n",
        name = actor.name,
        enum_name = actor.enum_name()
    ));
    for m in &actor.messages {
        let tag = match m.kind {
            InboxKind::Command => "COMMAND",
            InboxKind::Fact => "inbound FACT",
            InboxKind::Reminder => "REMINDER",
        };
        match &m.deferred {
            Some((reason, issue)) => out.push_str(&format!(
                "    /// {tag} `{name}` — HANDLER DEFERRED (actors.yaml `deferred:`): {reason} Tracked by {issue}.\n    {name}({ty}),\n",
                tag = tag,
                name = m.name,
                reason = reason,
                issue = issue,
                ty = m.payload_type
            )),
            None => out.push_str(&format!(
                "    /// {tag} `{name}`.\n    {name}({ty}),\n",
                tag = tag,
                name = m.name,
                ty = m.payload_type
            )),
        }
    }
    out.push_str("}\n");
    out
}

/// The inherent impl for one actor's inbox: the OPEN parse edge, plus the total projections
/// (`message_type`, `kind`) the runtime reads back off a parsed value.
///
/// The `match`es in here are generated ON PURPOSE and are not the routing match: they are total
/// projections FROM the variant set (name, kind), so they carry no decision a human could get
/// wrong. The one match that encodes a decision — what a message DOES — is hand-written in
/// `infrastructure::inbox`, and this emitter must never grow it.
pub(crate) fn inbox_enum_impl(actor: &InboxActor) -> String {
    let enum_name = actor.enum_name();
    let mut out = String::new();
    out.push_str(&format!("impl {enum_name} {{\n"));
    out.push_str(&format!(
        "    /// The actors.yaml key this inbox belongs to — the `inbound_messages.actor_type` a row\n    /// must carry to be parseable here.\n    pub const ACTOR_TYPE: &'static str = \"{}\";\n\n",
        actor.name
    ));
    out.push_str(&format!(
        "    /// Every message type this actor DECLARES it receives, in emission order — the\n    /// operator-facing answer to \"is this row's type one we know?\" without constructing a value.\n    pub const DECLARED: &'static [&'static str] = &[{}];\n\n",
        actor.messages.iter().map(|m| format!("\"{}\"", m.name)).collect::<Vec<_>>().join(", ")
    ));
    // parse — the single fallible edge.
    out.push_str("    /// Parse one wire `(message_type, payload)` pair into this actor's inbox. The ONLY\n    /// fallible edge of the typed dispatch path: past it the router matches a closed enum.\n    ///\n    /// An UNDECLARED message type is NOT an error about this payload — during a rolling deploy an\n    /// old consumer legitimately meets a message type a newer producer already emits. The caller\n    /// must treat it as TRANSIENT (retry, then park loudly), never as a terminal failure:\n    /// terminal-failing it buries a paid order.\n    pub fn parse(\n        message_type: &str,\n        payload: &serde_json::Value,\n    ) -> Result<Self, InboxParseError> {\n        match message_type {\n");
    for m in &actor.messages {
        match m.kind {
            // A COMMAND row's `payload` column is the BARE command struct (the GraphQL door
            // serializes the domain command's own serde form straight into it).
            InboxKind::Command => out.push_str(&format!(
                "            \"{name}\" => serde_json::from_value::<{ty}>(payload.clone())\n                .map(Self::{name})\n                .map_err(|e| InboxParseError::Payload {{\n                    actor_type: Self::ACTOR_TYPE,\n                    message_type: \"{name}\",\n                    detail: e.to_string(),\n                }}),\n",
                name = m.name,
                ty = m.payload_type
            )),
            // A FACT or promoted REMINDER row's `payload` column is the ADJACENTLY-TAGGED
            // `DomainEvent` (`{ eventType, payload }`) — a different wire shape from a command's,
            // which is why this parse cannot be one uniform `from_value`.
            //
            // And it cross-checks the tag against the row's `message_type`. Nothing did before:
            // a row could carry `message_type: "OrderPlaced"` with an `eventType: "OrderRejected"`
            // body and the generic record route would have appended the body under the wrong name.
            InboxKind::Fact | InboxKind::Reminder => out.push_str(&format!(
                "            \"{name}\" => match serde_json::from_value::<domain::generated::events::DomainEvent>(payload.clone()) {{\n                Ok(domain::generated::events::DomainEvent::{name}(e)) => Ok(Self::{name}(e)),\n                Ok(other) => Err(InboxParseError::Payload {{\n                    actor_type: Self::ACTOR_TYPE,\n                    message_type: \"{name}\",\n                    detail: format!(\n                        \"row message_type is '{name}' but the staged DomainEvent is {{other:?}}\"\n                    ),\n                }}),\n                Err(e) => Err(InboxParseError::Payload {{\n                    actor_type: Self::ACTOR_TYPE,\n                    message_type: \"{name}\",\n                    detail: e.to_string(),\n                }}),\n            }},\n",
                name = m.name
            )),
        }
    }
    out.push_str("            other => Err(InboxParseError::UndeclaredMessage {\n                actor_type: Self::ACTOR_TYPE,\n                message_type: other.to_string(),\n            }),\n        }\n    }\n\n");
    // message_type projection
    out.push_str("    /// The wire `message_type` this value came from — a total projection of the variant set.\n    pub fn message_type(&self) -> &'static str {\n        match self {\n");
    for m in &actor.messages {
        out.push_str(&format!(
            "            Self::{name}(_) => \"{name}\",\n",
            name = m.name
        ));
    }
    out.push_str("        }\n    }\n\n");
    // kind projection
    out.push_str("    /// The message kind — a total projection of the variant set.\n    pub fn kind(&self) -> InboxKind {\n        match self {\n");
    for m in &actor.messages {
        out.push_str(&format!(
            "            Self::{name}(_) => {kind},\n",
            name = m.name,
            kind = m.kind.rust()
        ));
    }
    out.push_str("        }\n    }\n}\n");
    out
}

/// One actor's FACT sub-inbox: the `receives:` entries of kind Fact/Reminder ONLY.
///
/// **WHY A SECOND ENUM AND NOT A MATCH OVER `ActorInbox` (#780).** The obvious implementation of
/// the fact-record route is a match over the composite with lane arms — and it defeats the whole
/// mechanism. `ActorInbox::Payment(_) => Failed("no route")` is a total catch-all over every
/// message the Payment lane can ever carry, it compiles clean, and #776's scan approves it
/// (`is_catch_all` reads only the pattern's top level, `names_inbox_variant` only its head
/// segment). Rather than gate that shape, this REMOVES THE TEMPTATION: over `<Actor>FactInbox` a
/// command variant is UNSPELLABLE, so no arm ever needs to say "not a fact" and no lane wildcard is
/// ever wanted (compiler-first, ADR-20260803-234035; the scan stays as the fallback for the
/// residue).
///
/// Same division of labour as [`inbox_enum_decl`]: this emitter writes the VARIANTS and the total
/// projections; the human-owned `infrastructure::inbox::fact_route` writes what each one DOES, and
/// E0004 is what makes a new declared fact impossible to ignore.
pub(crate) fn fact_enum_name(actor: &InboxActor) -> String {
    format!("{}FactInbox", actor.name)
}

/// This actor's FACT/REMINDER messages, in emission order. Empty = the actor's lane carries
/// commands only, and no fact enum is emitted for it (an uninhabited enum would buy nothing and
/// would force every consumer to reason about a variant that cannot exist).
pub(crate) fn facts_of(actor: &InboxActor) -> Vec<&InboxMessage> {
    actor.messages.iter().filter(|m| m.kind != InboxKind::Command).collect()
}

/// The DECLARATION of one actor's fact sub-inbox — separated from its impl for the same reason
/// [`inbox_enum_decl`] is: the E0004 proof compiles a mutated declaration against an unmutated arm
/// set (`tests.rs::a_widened_receives_set_of_a_fact_is_a_compile_error_in_the_fact_match`).
pub(crate) fn fact_enum_decl(actor: &InboxActor) -> String {
    let facts = facts_of(actor);
    let mut out = String::new();
    out.push_str(&format!(
        "/// GENERATED — the FACT half of `{name}`'s inbox: every `receives:` entry of kind FACT or\n/// REMINDER, and nothing else. The fact-record route matches on THIS, so a COMMAND variant is\n/// unspellable there and no arm ever needs a lane wildcard (#780).\n///\n/// Adding a `receives:` FACT adds a variant here, and the human-owned `fact_route` in\n/// `infrastructure::inbox` then fails to compile with E0004 until someone decides whether the\n/// aggregate records it. Before #780 the same omission shipped green: the fact route was a match\n/// over `DomainEvent` ending in `_ => Failed(\"no delivery route\")`, so a declared fact nobody\n/// consumed was LOST with a terminal verdict — invisible to the poison queue and refused by\n/// `RequeueMailboxMessage`.\n#[derive(Debug, Clone, PartialEq)]\npub enum {enum_name} {{\n",
        name = actor.name,
        enum_name = fact_enum_name(actor)
    ));
    for m in &facts {
        let tag = match m.kind {
            InboxKind::Reminder => "REMINDER",
            _ => "inbound FACT",
        };
        match &m.deferred {
            Some((reason, issue)) => out.push_str(&format!(
                "    /// {tag} `{name}` — HANDLER DEFERRED (actors.yaml `deferred:`): {reason} Tracked by {issue}.\n    {name}({ty}),\n",
                tag = tag, name = m.name, reason = reason, issue = issue, ty = m.payload_type
            )),
            None => out.push_str(&format!(
                "    /// {tag} `{name}`.\n    {name}({ty}),\n",
                tag = tag, name = m.name, ty = m.payload_type
            )),
        }
    }
    out.push_str("}\n");
    out
}

/// The fact sub-inbox's inherent impl plus the OWNING inbox's `into_fact` projection.
///
/// Every match in here is a TOTAL PROJECTION from the variant set — name, lane, the carried
/// `DomainEvent` — so none of them encodes a decision a human could get wrong. `into_fact` is the
/// one the whole design rests on: it is generated precisely BECAUSE "is this variant a fact?" is
/// answered by the `receives:` ref path and by nothing else, exactly like `message_type()`.
pub(crate) fn fact_enum_impl(actor: &InboxActor) -> String {
    let facts = facts_of(actor);
    let enum_name = fact_enum_name(actor);
    let owner = actor.enum_name();
    let mut out = String::new();
    out.push_str(&format!("impl {enum_name} {{\n"));
    out.push_str(&format!(
        "    /// The actors.yaml key this fact inbox belongs to — the lane a row must be ON.\n    pub const ACTOR_TYPE: &'static str = \"{}\";\n\n",
        actor.name
    ));
    out.push_str("    /// The wire `message_type` — a total projection of the variant set.\n    pub fn message_type(&self) -> &'static str {\n        match self {\n");
    for m in &facts {
        out.push_str(&format!("            Self::{name}(_) => \"{name}\",\n", name = m.name));
    }
    out.push_str("        }\n    }\n\n");
    out.push_str("    /// The carried business fact as the tagged `DomainEvent` the recorders take.\n    /// A total projection: the variant IS the tag, so this can never disagree with\n    /// `message_type()` the way a re-parse of the raw payload could.\n    pub fn into_domain_event(self) -> domain::generated::events::DomainEvent {\n        match self {\n");
    for m in &facts {
        out.push_str(&format!(
            "            Self::{name}(e) => domain::generated::events::DomainEvent::{name}(e),\n",
            name = m.name
        ));
    }
    out.push_str("        }\n    }\n}\n\n");
    // The owning inbox's projection.
    out.push_str(&format!(
        "impl {owner} {{\n    /// The FACT half of this lane's inbox, or `None` for a COMMAND — a total projection of the\n    /// variant set, generated for the same reason `message_type()` is: it carries no decision.\n    pub fn into_fact(self) -> Option<{enum_name}> {{\n        match self {{\n"
    ));
    for m in &actor.messages {
        if m.kind == InboxKind::Command {
            out.push_str(&format!("            Self::{name}(_) => None,\n", name = m.name));
        } else {
            out.push_str(&format!(
                "            Self::{name}(e) => Some({enum_name}::{name}(e)),\n",
                name = m.name
            ));
        }
    }
    out.push_str("        }\n    }\n}\n");
    out
}

/// The whole emitted file.
pub(crate) fn emit_app_inboxes(model: &Model) -> String {
    let actors = inbox_actors(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/*/actors.yaml `receives:` — do not edit\n// by hand (#771, founder directive 2026-08-30: \"Go for the generated per-actor enum\").\n//\n// ONE `<Actor>Inbox` enum per mailbox actor: the CLOSED set of messages that actor's single queue\n// can carry, spanning COMMAND / inbound FACT / REMINDER, each variant carrying its typed payload.\n//\n// THIS FILE IS THE ENUM ONLY. The routing `match` — what a message DOES — is HUMAN-OWNED, in\n// `crates/infrastructure/src/inbox.rs`. Generating both halves from one walk would make the match\n// exhaustive by construction and the compiler would catch nothing; keeping them apart is what makes\n// a new `receives:` entry an E0004 compile error instead of a `FAILED` row in production (#595).\n//\n// The wire stays a string: `ActorInbox::parse(actor_type, message_type, payload)` is the single\n// fallible edge, and it consumes the ACTOR TYPE as well as the message type — so a row enqueued on\n// lane A carrying lane B's message cannot parse at all (ADR-20260829-230418, \"Aggregates own the\n// facts\": the transport must not be able to violate the isolation the aggregates declare).\n\n/// What kind of message an inbox variant carries. The inbox is ONE queue, so one type spans all\n/// three; the kind is a property of the variant, never a second enum to keep in sync.\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum InboxKind {\n    /// A `commands.yaml#/…` entry: write-side input, may be REJECTED.\n    Command,\n    /// An `events.yaml#/…` entry: a fact that already happened, recorded — never rejected.\n    Fact,\n    /// A `#/<Actor>/reminders/<Name>` entry: a reminder this actor scheduled for itself, promoted\n    /// to the queue when due (its payload is the declared fact, ADR-20260731-153000 §1a).\n    Reminder,\n}\n\n/// Why a wire triple could not become a typed inbox value.\n///\n/// The three arms are DELIBERATELY distinct, because the runtime owes them different postures:\n/// `UnknownActor` and `UndeclaredMessage` are TRANSIENT (a rolling deploy legitimately produces\n/// them — retry, then park loudly), while `Payload` is a genuine shape failure of a message this\n/// build does understand.\n#[derive(Debug, Clone, PartialEq, Eq)]\npub enum InboxParseError {\n    /// No mailbox actor by this `actor_type` exists in THIS build.\n    UnknownActor { actor_type: String },\n    /// The actor exists and does not declare this message in its `receives:` set — in THIS build.\n    UndeclaredMessage { actor_type: &'static str, message_type: String },\n    /// A DECLARED message whose payload does not deserialize into its typed shape.\n    Payload { actor_type: &'static str, message_type: &'static str, detail: String },\n}\n\nimpl InboxParseError {\n    /// TRANSIENT means: this build cannot route the row, but a build on the other side of a rolling\n    /// deploy can — so the row must be RETRIED and then PARKED (the poison path), never terminally\n    /// FAILED. Terminal-failing an unknown type during a deploy buries a paid order.\n    pub fn is_transient(&self) -> bool {\n        matches!(self, Self::UnknownActor { .. } | Self::UndeclaredMessage { .. })\n    }\n}\n\nimpl std::fmt::Display for InboxParseError {\n    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n        match self {\n            Self::UnknownActor { actor_type } => {\n                write!(f, \"no mailbox actor '{actor_type}' in this build\")\n            }\n            Self::UndeclaredMessage { actor_type, message_type } => write!(\n                f,\n                \"actor '{actor_type}' does not declare message '{message_type}' in this build\"\n            ),\n            Self::Payload { actor_type, message_type, detail } => {\n                write!(f, \"{actor_type}/{message_type} payload: {detail}\")\n            }\n        }\n    }\n}\n\nimpl std::error::Error for InboxParseError {}\n",
    );
    for actor in &actors {
        out.push('\n');
        out.push_str(&inbox_enum_decl(actor));
        out.push('\n');
        out.push_str(&inbox_enum_impl(actor));
        // The FACT half (#780) — only for lanes that actually declare one. An actor whose
        // `receives:` set is commands only gets NO fact enum: an uninhabited one would force every
        // consumer to reason about a variant that cannot exist, and `ActorInbox::into_fact`
        // answers `None` for that lane directly.
        if !facts_of(actor).is_empty() {
            out.push('\n');
            out.push_str(&fact_enum_decl(actor));
            out.push('\n');
            out.push_str(&fact_enum_impl(actor));
        }
    }
    // The cross-actor envelope.
    out.push_str("\n/// One parsed mailbox row, ATTRIBUTED TO ITS LANE: the actor type is not a string the router\n/// carries alongside the message any more, it is the outer variant. A `PlaceOrder` payload on a\n/// `Cart` lane is not a mis-routed value here — it is a value that cannot be constructed.\n#[derive(Debug, Clone, PartialEq)]\npub enum ActorInbox {\n");
    for actor in &actors {
        out.push_str(&format!(
            "    /// A row on a `{name}` lane.\n    {name}({enum_name}),\n",
            name = actor.name,
            enum_name = actor.enum_name()
        ));
    }
    out.push_str("}\n\nimpl ActorInbox {\n");
    out.push_str("    /// The ONE door from the wire into the typed dispatch path. Takes the lane's `actor_type`\n    /// AND the row's `message_type`: before #771 the router took only the message type, so a row on\n    /// lane A could drive a handler that writes aggregate B under A's fence.\n    pub fn parse(\n        actor_type: &str,\n        message_type: &str,\n        payload: &serde_json::Value,\n    ) -> Result<Self, InboxParseError> {\n        match actor_type {\n");
    for actor in &actors {
        out.push_str(&format!(
            "            \"{name}\" => {enum_name}::parse(message_type, payload).map(Self::{name}),\n",
            name = actor.name,
            enum_name = actor.enum_name()
        ));
    }
    out.push_str("            other => Err(InboxParseError::UnknownActor { actor_type: other.to_string() }),\n        }\n    }\n\n");
    out.push_str("    /// The lane this row belongs to — a total projection of the variant set.\n    pub fn actor_type(&self) -> &'static str {\n        match self {\n");
    for actor in &actors {
        out.push_str(&format!(
            "            Self::{name}(_) => {enum_name}::ACTOR_TYPE,\n",
            name = actor.name,
            enum_name = actor.enum_name()
        ));
    }
    out.push_str("        }\n    }\n\n");
    out.push_str("    /// The wire `message_type` — a total projection of the variant set.\n    pub fn message_type(&self) -> &'static str {\n        match self {\n");
    for actor in &actors {
        out.push_str(&format!("            Self::{name}(m) => m.message_type(),\n", name = actor.name));
    }
    out.push_str("        }\n    }\n\n");
    out.push_str("    /// The message kind — a total projection of the variant set.\n    pub fn kind(&self) -> InboxKind {\n        match self {\n");
    for actor in &actors {
        out.push_str(&format!("            Self::{name}(m) => m.kind(),\n", name = actor.name));
    }
    out.push_str("        }\n    }\n}\n");

    // ─── The FACT envelope (#780) ────────────────────────────────────────────────────────────
    let fact_actors: Vec<&InboxActor> =
        actors.iter().filter(|a| !facts_of(a).is_empty()).collect();
    out.push_str("\n/// One parsed mailbox row that carries a FACT, attributed to its lane — the composite the\n/// human-owned `fact_route` dispatches on (#780).\n///\n/// It exists so that a COMMAND variant is UNSPELLABLE in the fact match. Matching the fact route\n/// over `ActorInbox` instead would need a lane arm per command-only lane and, worse, would make\n/// `ActorInbox::Payment(_) => Failed(\"no route\")` both compilable and gate-clean while absorbing\n/// every message the Payment lane can ever carry. Removing the temptation beats gating it\n/// (compiler-first, ADR-20260803-234035).\n///\n/// A lane whose `receives:` set is commands only has no variant here at all.\n#[derive(Debug, Clone, PartialEq)]\npub enum ActorFactInbox {\n");
    for actor in &fact_actors {
        out.push_str(&format!(
            "    /// A FACT row on a `{name}` lane.\n    {name}({enum_name}),\n",
            name = actor.name,
            enum_name = fact_enum_name(actor)
        ));
    }
    out.push_str("}\n\nimpl ActorFactInbox {\n");
    out.push_str("    /// The lane this fact was delivered ON — read from the ROW's `actor_type` through\n    /// `ActorInbox::parse`, never derived from the payload. A fact parsed into the wrong lane is\n    /// the foreign-stream append wearing a typed hat, and the enum cannot catch it because the\n    /// enum would be right and the lane wrong (vernon, #780).\n    pub fn actor_type(&self) -> &'static str {\n        match self {\n");
    for actor in &fact_actors {
        out.push_str(&format!(
            "            Self::{name}(_) => {enum_name}::ACTOR_TYPE,\n",
            name = actor.name,
            enum_name = fact_enum_name(actor)
        ));
    }
    out.push_str("        }\n    }\n\n");
    out.push_str("    /// The wire `message_type` — a total projection of the variant set.\n    pub fn message_type(&self) -> &'static str {\n        match self {\n");
    for actor in &fact_actors {
        out.push_str(&format!("            Self::{name}(m) => m.message_type(),\n", name = actor.name));
    }
    out.push_str("        }\n    }\n\n");
    out.push_str("    /// The carried business fact as the tagged `DomainEvent` the recorders take.\n    pub fn into_domain_event(self) -> domain::generated::events::DomainEvent {\n        match self {\n");
    for actor in &fact_actors {
        out.push_str(&format!("            Self::{name}(m) => m.into_domain_event(),\n", name = actor.name));
    }
    out.push_str("        }\n    }\n}\n\n");
    out.push_str("impl ActorInbox {\n    /// The FACT half of this row, or `None` for a COMMAND — the TOTAL generated projection the\n    /// fact route runs on (#780). Generated because it carries no decision: whether a `receives:`\n    /// entry is a fact is answered by its `$ref` path and by nothing else, exactly like\n    /// `message_type()`. The DECISION — what each fact DOES — stays human-owned, and E0004 over\n    /// `ActorFactInbox` is what makes a new declared fact impossible to ignore.\n    pub fn into_fact(self) -> Option<ActorFactInbox> {\n        match self {\n");
    for actor in &actors {
        if facts_of(actor).is_empty() {
            out.push_str(&format!(
                "            // The `{name}` lane declares no fact.\n            Self::{name}(_) => None,\n",
                name = actor.name
            ));
        } else {
            out.push_str(&format!(
                "            Self::{name}(m) => m.into_fact().map(ActorFactInbox::{name}),\n",
                name = actor.name
            ));
        }
    }
    out.push_str("        }\n    }\n}\n");

    out.push_str("\n/// Every FACT an actor DECLARES it receives — `(actor_type, message_type)`, sorted.\n///\n/// The DECLARED population a route gate measures itself against: a routed `deliver:` target must\n/// be in here AND must have a real record arm, and a gate that derives its own population from the\n/// artifact under test is measuring nothing (#780).\npub const DECLARED_FACTS: &[(&str, &str)] = &[\n");
    for actor in &fact_actors {
        for m in facts_of(actor) {
            out.push_str(&format!("    (\"{}\", \"{}\"),\n", actor.name, m.name));
        }
    }
    out.push_str("];\n");

    // The DEFERRED table — the DSL successor of the retired `UNWIRED_MUTATIONS` const (#771).
    out.push_str("\n/// Messages an actor DECLARES it receives whose handler is deliberately not built yet\n/// (`actors.yaml` `receives[].deferred: { reason, issue }`) — `(actor_type, message_type, reason,\n/// issue)`.\n///\n/// This replaces the retired `UNWIRED_MUTATIONS` const in the codegen crate. A deferral is now\n/// REVIEWABLE SPEC CONTENT — it sits next to the declaration it qualifies, carries a reason and a\n/// tracking issue, and is rendered onto the variant it qualifies — instead of a Rust const in an\n/// emitter that nobody reads. (It reaches the GENERATED Rust doc comment and this table; it does\n/// NOT reach `specs/generated/documentation.generated.md`, which has no `deferred:` reader.) The\n/// variant and its router arm still exist: what is deferred is what the arm DOES, and the compiler\n/// still refuses to let the message go unconsumed.\npub const DEFERRED_MESSAGES: &[(&str, &str, &str, &str)] = &[\n");
    for actor in &actors {
        for m in &actor.messages {
            if let Some((reason, issue)) = &m.deferred {
                out.push_str(&format!(
                    "    (\"{}\", \"{}\", {:?}, \"{}\"),\n",
                    actor.name, m.name, reason, issue
                ));
            }
        }
    }
    out.push_str("];\n");
    out
}
