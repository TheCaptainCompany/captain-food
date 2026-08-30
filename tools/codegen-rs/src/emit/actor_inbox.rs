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
//! `crates/application/src/inbox.rs`, and `rustc` E0004 is what makes a new `receives:` entry
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
    Some((
        reminder.to_string(),
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
                    Some((reason.to_string(), issue.to_string()))
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
        "/// GENERATED from `actors.yaml#/{name}/receives` — the CLOSED set of messages the `{name}`\n/// actor's ONE mailbox queue can carry, spanning every kind (COMMAND / inbound FACT / REMINDER),\n/// each variant carrying its typed payload.\n///\n/// Adding a `receives:` entry adds a variant here, and the human-owned `match` in\n/// `application::inbox` then fails to compile with E0004 until someone decides what the new\n/// message DOES. That compile error is the whole point: before #771 the same omission shipped\n/// green and surfaced as a `FAILED \"unroutable command type\"` row in production.\n#[derive(Debug, Clone, PartialEq)]\npub enum {enum_name} {{\n",
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
/// `application::inbox`, and this emitter must never grow it.
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
        out.push_str(&format!(
            "            \"{name}\" => serde_json::from_value::<{ty}>(payload.clone())\n                .map(Self::{name})\n                .map_err(|e| InboxParseError::Payload {{\n                    actor_type: Self::ACTOR_TYPE,\n                    message_type: \"{name}\",\n                    detail: e.to_string(),\n                }}),\n",
            name = m.name,
            ty = m.payload_type
        ));
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

/// The whole emitted file.
pub(crate) fn emit_app_inboxes(model: &Model) -> String {
    let actors = inbox_actors(model);
    let mut out = String::from(
        "// GENERATED by the Captain.Food codegen from specs/*/actors.yaml `receives:` — do not edit\n// by hand (#771, founder directive 2026-08-30: \"Go for the generated per-actor enum\").\n//\n// ONE `<Actor>Inbox` enum per mailbox actor: the CLOSED set of messages that actor's single queue\n// can carry, spanning COMMAND / inbound FACT / REMINDER, each variant carrying its typed payload.\n//\n// THIS FILE IS THE ENUM ONLY. The routing `match` — what a message DOES — is HUMAN-OWNED, in\n// `crates/application/src/inbox.rs`. Generating both halves from one walk would make the match\n// exhaustive by construction and the compiler would catch nothing; keeping them apart is what makes\n// a new `receives:` entry an E0004 compile error instead of a `FAILED` row in production (#595).\n//\n// The wire stays a string: `ActorInbox::parse(actor_type, message_type, payload)` is the single\n// fallible edge, and it consumes the ACTOR TYPE as well as the message type — so a row enqueued on\n// lane A carrying lane B's message cannot parse at all (ADR-20260829-230418, \"Aggregates own the\n// facts\": the transport must not be able to violate the isolation the aggregates declare).\n\n/// What kind of message an inbox variant carries. The inbox is ONE queue, so one type spans all\n/// three; the kind is a property of the variant, never a second enum to keep in sync.\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum InboxKind {\n    /// A `commands.yaml#/…` entry: write-side input, may be REJECTED.\n    Command,\n    /// An `events.yaml#/…` entry: a fact that already happened, recorded — never rejected.\n    Fact,\n    /// A `#/<Actor>/reminders/<Name>` entry: a reminder this actor scheduled for itself, promoted\n    /// to the queue when due (its payload is the declared fact, ADR-20260731-153000 §1a).\n    Reminder,\n}\n\n/// Why a wire triple could not become a typed inbox value.\n///\n/// The three arms are DELIBERATELY distinct, because the runtime owes them different postures:\n/// `UnknownActor` and `UndeclaredMessage` are TRANSIENT (a rolling deploy legitimately produces\n/// them — retry, then park loudly), while `Payload` is a genuine shape failure of a message this\n/// build does understand.\n#[derive(Debug, Clone, PartialEq, Eq)]\npub enum InboxParseError {\n    /// No mailbox actor by this `actor_type` exists in THIS build.\n    UnknownActor { actor_type: String },\n    /// The actor exists and does not declare this message in its `receives:` set — in THIS build.\n    UndeclaredMessage { actor_type: &'static str, message_type: String },\n    /// A DECLARED message whose payload does not deserialize into its typed shape.\n    Payload { actor_type: &'static str, message_type: &'static str, detail: String },\n}\n\nimpl InboxParseError {\n    /// TRANSIENT means: this build cannot route the row, but a build on the other side of a rolling\n    /// deploy can — so the row must be RETRIED and then PARKED (the poison path), never terminally\n    /// FAILED. Terminal-failing an unknown type during a deploy buries a paid order.\n    pub fn is_transient(&self) -> bool {\n        matches!(self, Self::UnknownActor { .. } | Self::UndeclaredMessage { .. })\n    }\n}\n\nimpl std::fmt::Display for InboxParseError {\n    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n        match self {\n            Self::UnknownActor { actor_type } => {\n                write!(f, \"no mailbox actor '{actor_type}' in this build\")\n            }\n            Self::UndeclaredMessage { actor_type, message_type } => write!(\n                f,\n                \"actor '{actor_type}' does not declare message '{message_type}' in this build\"\n            ),\n            Self::Payload { actor_type, message_type, detail } => {\n                write!(f, \"{actor_type}/{message_type} payload: {detail}\")\n            }\n        }\n    }\n}\n\nimpl std::error::Error for InboxParseError {}\n",
    );
    for actor in &actors {
        out.push('\n');
        out.push_str(&inbox_enum_decl(actor));
        out.push('\n');
        out.push_str(&inbox_enum_impl(actor));
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

    // The DEFERRED table — the DSL successor of the retired `UNWIRED_MUTATIONS` const (#771).
    out.push_str("\n/// Messages an actor DECLARES it receives whose handler is deliberately not built yet\n/// (`actors.yaml` `receives[].deferred: { reason, issue }`) — `(actor_type, message_type, reason,\n/// issue)`.\n///\n/// This replaces the retired `UNWIRED_MUTATIONS` const in the codegen crate. A deferral is now\n/// REVIEWABLE SPEC CONTENT — it sits next to the declaration it qualifies, carries a reason and a\n/// tracking issue, and shows up in the generated documentation — instead of a Rust const in an\n/// emitter that nobody reads. The variant and its router arm still exist: what is deferred is what\n/// the arm DOES, and the compiler still refuses to let the message go unconsumed.\npub const DEFERRED_MESSAGES: &[(&str, &str, &str, &str)] = &[\n");
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
