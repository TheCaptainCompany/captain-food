use crate::*;

// ─── Ref-KIND contract (§1b) ────────────────────────────────────────────────────────────────────
// Resolving is not enough: a `$ref` must point at the right KIND of thing. `state_table` must be a
// process-manager state table — not merely "some table under database/tables/"; a screen resolver must
// be a query, not a mutation; an actor `emits` must be an event, not a command. §1b makes that a
// declared, exhaustive contract instead of the ad-hoc per-site checks scattered through §2–§11.
//
// It is FAIL-CLOSED: a `$ref` site not covered by REF_CONTRACT is an error, so a new ref-carrying field
// cannot be added to the DSL without declaring what it may point at.

/// What a `$ref` target IS — finer than the file it lives in (a table file holds several kinds).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Command,
    /// A `commands.yaml` definition NO actor receives: a shared payload sub-object (e.g. `CartLine`),
    /// not a business intention. Legal inside `properties`, never as an actor's message.
    PayloadObject,
    Event,
    /// A single property of a command/event/entity — `<file>#/<Def>/properties/<p>`.
    MessageProperty,
    Error,
    Rule,
    Scalar,
    /// A scalar with an `enum` member list (a state/status type).
    EnumScalar,
    Entity,
    /// An `actors.yaml` event-sourced aggregate.
    Aggregate,
    /// A per-actor typed self-message — `actors.yaml#/<Actor>/reminders/<Name>` (ADR-20260731-214500).
    Reminder,
    /// A declared aggregate state field — `actors.yaml#/<Actor>/state/<field>` (PROP-20260728-135632 §2.1).
    StateField,
    /// An actor answer operation — `actors.yaml#/<Actor>/answers/<op>` (PROP-20260815-142349 §2).
    /// Referenced ONLY by a PM `ask:` step; every other site is refused (`ans-ref-isolation`).
    Answer,
    /// One declared reply property — `actors.yaml#/<Actor>/answers/<op>/reply/<prop>`.
    AnswerProperty,
    /// A `configuration.yaml#/keys/<KEY>` runtime-configuration key (PROP-20260729-004500).
    ConfigKey,
    /// A `configuration.yaml#/retention_windows/<NAME>` APPROVED LEGAL RETENTION WINDOW (MET-W,
    /// CLOSED 2026-08-11). Distinct from `ConfigKey` on purpose: a config key is an environment
    /// variable an operator sets, a retention window is a legal instrument nobody may set at
    /// runtime — so the two must never be substitutable at a `$ref` site.
    RetentionWindow,
    /// A `processmanager.yaml` state-table orchestrator.
    ProcessManager,
    Service,
    ServiceOperation,
    Query,
    /// One declared ARG of an api.yaml query — `api.yaml#/queries/<q>/args/<a>` (#745): what a
    /// screen's `skipped_reads` declaration names as the required arg it has no source for.
    QueryArg,
    Mutation,
    Subscription,
    ApiType,
    ApiInput,
    Test,
    /// A `tests.yaml#/fixtures/<f>` expected-outcome fixture.
    Fixture,
    /// A `translations.yaml` / `<surface>.translations.yaml` i18n key.
    TranslationKey,
    /// A generated fold VIEW over `domain_events` (`database/projection_views.yaml`).
    ProjectionView,
    /// A MATERIALIZED read-model table fed by an app projector (`tables/projection_tables.yaml`).
    ProjectionTable,
    /// A process manager's private state table (`tables/process_managers.yaml`).
    PmStateTable,
    /// A seed/config table configured by the repo seed script (`tables/referential.yaml`).
    ReferentialTable,
    /// A write-path journal — the actor mailbox `inbound_messages` + `mailbox_partitions`
    /// (`tables/journals.yaml`).
    JournalTable,
    /// Adapter-owned raw staging (`tables/integration_staging.yaml`).
    StagingTable,
    /// Integration connection storage (`tables/integration_connections.yaml`).
    ConnectionTable,
    /// `domain_events` / `domain_stream` (`tables/eventstore.yaml`).
    EventStoreTable,
    /// A write-side uniqueness reservation table (`tables/reservations.yaml`, ADR-20260728-011344).
    ReservationTable,
    /// A declared DATABASE (`database/databases.yaml`, #494 slice 1) — what a table's `database:`
    /// placement key points at. Without this arm the catalog fails closed at validate
    /// (`ref-kind-unknown` on every placement — the `reservations.yaml` precedent).
    Database,
    /// A column of any of the table kinds above.
    TableColumn,
    Screen,
    /// A `screens/<surface>.yaml#/resolvers/<key>` data-layer read binding (#745): what a screen's
    /// `skipped_reads` declaration points at with a same-file `$ref`.
    Resolver,
    Persona,
    /// A `stories.yaml#/<persona>/activities/<Activity>` persona activity — the unit a business
    /// metric binds (ADR-20260810-234225 Decision 1 / ADR-20260811-014129).
    Activity,
    /// An `observability.yaml` workflow contract.
    ObservabilityWorkflow,
    /// A `business_metrics.yaml#/projections/<Name>` declared BAM fold (ADR-20260811-014129).
    BamProjection,
    /// One declared measure of a BAM projection —
    /// `business_metrics.yaml#/projections/<Name>/measures/<measure>`.
    BamMeasure,
}

impl Kind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Kind::Command => "command",
            Kind::PayloadObject => "payload object",
            Kind::Event => "event",
            Kind::MessageProperty => "message property",
            Kind::Error => "error",
            Kind::Rule => "rule",
            Kind::Scalar => "scalar",
            Kind::EnumScalar => "enum scalar",
            Kind::Entity => "entity",
            Kind::Aggregate => "aggregate",
            Kind::Reminder => "actor reminder",
            Kind::StateField => "actor state field",
            Kind::Answer => "actor answer operation",
            Kind::AnswerProperty => "actor answer reply property",
            Kind::ConfigKey => "configuration key",
            Kind::RetentionWindow => "approved retention window",
            Kind::ProcessManager => "process manager",
            Kind::Service => "service",
            Kind::ServiceOperation => "service operation",
            Kind::Query => "query",
            Kind::QueryArg => "query arg",
            Kind::Mutation => "mutation",
            Kind::Subscription => "subscription",
            Kind::ApiType => "api output type",
            Kind::ApiInput => "api input type",
            Kind::Test => "behaviour test",
            Kind::Fixture => "test fixture",
            Kind::TranslationKey => "translation key",
            Kind::ProjectionView => "projection view",
            Kind::ProjectionTable => "projection table",
            Kind::PmStateTable => "process-manager state table",
            Kind::ReferentialTable => "referential table",
            Kind::JournalTable => "journal table",
            Kind::StagingTable => "staging table",
            Kind::ConnectionTable => "connection table",
            Kind::EventStoreTable => "event-store table",
            Kind::ReservationTable => "write-side reservation table",
            Kind::Database => "database",
            Kind::TableColumn => "table column",
            Kind::Screen => "screen",
            Kind::Resolver => "screen resolver binding",
            Kind::Persona => "persona",
            Kind::Activity => "persona activity",
            Kind::ObservabilityWorkflow => "observability workflow",
            Kind::BamProjection => "business-metric projection",
            Kind::BamMeasure => "business-metric measure",
        }
    }
}

pub(crate) fn kind_list(kinds: &[Kind]) -> String {
    kinds.iter().map(|k| k.name()).collect::<Vec<_>>().join(" or ")
}

/// The ONLY store kinds a GraphQL output type may bind to with api.yaml `reads:` — the CQRS READ
/// side. A [`Kind::ReferentialTable`] additionally opts in per table with `reference: true`.
pub(crate) const READ_TARGET_KINDS: &[Kind] =
    &[Kind::ProjectionView, Kind::ProjectionTable, Kind::ReferentialTable];

/// Every OTHER store kind: write-path state owned by infrastructure or by an adapter — the actor
/// mailbox and its registry, process-manager saga rows, adapter staging mirrors and credential
/// stores, the event log itself, the uniqueness reservations. A business read model may never BE one,
/// because retiring one must stay an implementation detail: `command_journal` had leaked into resolver
/// bodies, and deleting it cost 110 files.
///
/// This is the PARTITION, not an allowlist — nothing grants access from it. The kinds a transient type
/// may actually name are the strictly narrower [`TRANSIENT_READ_KINDS`].
pub(crate) const INFRASTRUCTURE_TABLE_KINDS: &[Kind] = &[
    Kind::JournalTable,
    Kind::PmStateTable,
    Kind::StagingTable,
    Kind::ConnectionTable,
    Kind::EventStoreTable,
    Kind::ReservationTable,
];

/// What a TRANSIENT output type may name under `readsInfrastructure:` — the two categories a resolver
/// is served from TODAY, and nothing else: the actor mailbox (`operationStatus` and the two ADMIN
/// supervision screens) and a process-manager saga row (`paymentStatus`, ADR-20260720-015500's
/// declared exception).
///
/// Deliberately NOT [`INFRASTRUCTURE_TABLE_KINDS`]. Handing the whole partition to the new key would
/// have made it strictly WEAKER than the `reads:` contract it sits beside: `reads:` already refused a
/// `$ref` into `hubrise_connections` or `domain_events`, so a permissive `readsInfrastructure:` would
/// have OPENED a door that was previously shut — on `Operation`, which is PUBLIC-reachable — to exactly
/// the leaked-OAuth-token exposure this wall exists to prevent, and to the raw event log the doctrine
/// comment on the `reads:` row calls "never". Found by independent review of this very change, which is
/// the argument for the shape rather than against it: serving a query from a credential store, a
/// staging mirror or `domain_events` must cost a reviewed edit to THIS line.
pub(crate) const TRANSIENT_READ_KINDS: &[Kind] = &[Kind::JournalTable, Kind::PmStateTable];

/// Is this kind a STORE (a table or view), and if so may a `reads:` binding name it?
/// `Some(true)` = read model / reference table, `Some(false)` = infrastructure- or adapter-owned,
/// `None` = not a store at all.
///
/// The `match` is EXHAUSTIVE on purpose (compiler first, ADR-20260803-234035): a new `Kind` variant
/// does not compile until it is classified here, so the wall cannot be widened by omission the way
/// `reference: true` could. Precisely: that is a compile-time guarantee about a new **`Kind`**, not
/// about a new catalog FILE — [`classify`]'s `_ => None` arm accepts an unknown
/// `database/tables/*.yaml` with no code change at all, and such a file then fails closed at VALIDATE
/// (`ref-kind-unknown` on any `$ref` into it, plus `reads-unknown-view`) rather than at compile.
/// Level 4 for the kind, level 3 for the file.
pub(crate) fn read_target_kind(k: Kind) -> Option<bool> {
    match k {
        Kind::ProjectionView | Kind::ProjectionTable | Kind::ReferentialTable => Some(true),
        Kind::JournalTable
        | Kind::PmStateTable
        | Kind::StagingTable
        | Kind::ConnectionTable
        | Kind::EventStoreTable
        | Kind::ReservationTable => Some(false),
        Kind::Command
        | Kind::PayloadObject
        | Kind::Event
        | Kind::MessageProperty
        | Kind::Error
        | Kind::Rule
        | Kind::Scalar
        | Kind::EnumScalar
        | Kind::Entity
        | Kind::Aggregate
        | Kind::Reminder
        | Kind::StateField
        // An answer is a VALUE served on Ask — constitutionally unpersistable (PROP-20260815-142349
        // §4 rule 3): no journal, no `domain_events`, no View_* may name one, so it is not a store.
        | Kind::Answer
        | Kind::AnswerProperty
        | Kind::ConfigKey
        | Kind::RetentionWindow
        | Kind::ProcessManager
        | Kind::Service
        | Kind::ServiceOperation
        | Kind::Query
        | Kind::QueryArg
        | Kind::Mutation
        | Kind::Subscription
        | Kind::ApiType
        | Kind::ApiInput
        | Kind::Test
        | Kind::Fixture
        | Kind::TranslationKey
        // A database is a PLACEMENT target, not a store a `reads:` may bind — the tables inside it
        // keep their own per-kind classification above.
        | Kind::Database
        | Kind::TableColumn
        | Kind::Screen
        | Kind::Resolver
        | Kind::Persona
        | Kind::Activity
        | Kind::ObservabilityWorkflow
        // A BAM projection is read through its generated tenant-scoped query (ADR-20260811-014129
        // D9), never bound by an api.yaml `reads:` until that generation exists — refusing it here
        // keeps the door closed rather than open-by-default.
        | Kind::BamProjection
        | Kind::BamMeasure => None,
    }
}

/// What KIND the target of a resolved `$ref` is: `(file, pointer segments, resolved node)` → `Kind`.
/// `None` = the pointer lands somewhere with no declared kind (e.g. mid-tree) — §1b reports it, which
/// keeps the classifier honest as the DSL grows.
pub(crate) fn classify(file: &str, path: &[String], node: &Value, handled: &BTreeSet<String>) -> Option<Kind> {
    let seg = |i: usize| path.get(i).map(|s| s.as_str());
    let top = path.len() == 1;
    // A table column: `<table>/columns/<col>` in any database/tables/*.yaml file.
    let table_column = path.len() == 3 && seg(1) == Some("columns");
    let table_kind = |k: Kind| -> Option<Kind> {
        if top {
            Some(k)
        } else if table_column {
            Some(Kind::TableColumn)
        } else {
            None
        }
    };
    match file {
        "commands.yaml" | "events.yaml" | "entities.yaml" => {
            let base = match file {
                // A commands.yaml entry is a COMMAND when an actor receives it; otherwise it is a
                // shared payload sub-object (mirrors §3's value-object derivation). A genuinely
                // unhandled command is reported by §3's `command-unhandled`.
                "commands.yaml" => match path.first() {
                    Some(n) if handled.contains(n.as_str()) => Kind::Command,
                    _ => Kind::PayloadObject,
                },
                "events.yaml" => Kind::Event,
                _ => Kind::Entity,
            };
            if top {
                Some(base)
            } else if path.len() == 3 && seg(1) == Some("properties") {
                Some(Kind::MessageProperty)
            } else {
                None
            }
        }
        "errors.yaml" => top.then_some(Kind::Error),
        "rules.yaml" => top.then_some(Kind::Rule),
        "scalars.yaml" => top.then(|| {
            if node.get("enum").is_some() { Kind::EnumScalar } else { Kind::Scalar }
        }),
        // `principals` is the file-header role → identity-scalar map (PROP-20260728-152752 §2.4),
        // not an actor — excluded so it never registers as a phantom aggregate. Below the actor:
        // `<Actor>/reminders/<Name>` is a typed self-message (ADR-20260731-214500) and
        // `<Actor>/state/<field>` a declared state field (what a deletion `match.state` binds to).
        "actors.yaml" => match (top, seg(1), path.len()) {
            (true, _, _) => (path.first().map(String::as_str) != Some("principals")).then_some(Kind::Aggregate),
            (false, Some("reminders"), 3) => Some(Kind::Reminder),
            (false, Some("state"), 3) => Some(Kind::StateField),
            // `<Actor>/answers/<op>` names an ANSWER operation, `…/reply/<prop>` one declared
            // reply property (#582) — resolvable, versionable, checkable (PROP §4 rule 1).
            (false, Some("answers"), 3) => Some(Kind::Answer),
            (false, Some("answers"), 5) if seg(3) == Some("reply") => Some(Kind::AnswerProperty),
            _ => None,
        },
        // Runtime configuration (PROP-20260729-004500): `keys/<KEY>` is what a `deletion.after` /
        // `reminders.*.after` window binds to (ADR-20260731-214500 — a $ref, never a bare string).
        // `retention_windows/<NAME>` is the MET-W approved catalog an event's `legalRetention:`
        // binds to. A $ref, never a bare token: a plain string is invisible to the refs walker, so
        // no rule could see it (ADR-20260811-014129 clause 2).
        "configuration.yaml" => match (path.len(), seg(0)) {
            (2, Some("keys")) => Some(Kind::ConfigKey),
            (2, Some("retention_windows")) => Some(Kind::RetentionWindow),
            _ => None,
        },
        "processmanager.yaml" => top.then_some(Kind::ProcessManager),
        "services.yaml" => {
            if top {
                Some(Kind::Service)
            } else if path.len() == 3 && seg(1) == Some("operations") {
                Some(Kind::ServiceOperation)
            } else {
                None
            }
        }
        "api.yaml" => match (seg(0), path.len()) {
            (Some("queries"), 2) => Some(Kind::Query),
            // `queries/<q>/args/<a>` — one declared query arg (#745, `skipped_reads.missing_arg`).
            (Some("queries"), 4) if seg(2) == Some("args") => Some(Kind::QueryArg),
            (Some("mutations"), 2) => Some(Kind::Mutation),
            (Some("subscriptions"), 2) => Some(Kind::Subscription),
            (Some("types"), 2) => Some(Kind::ApiType),
            (Some("inputs"), 2) => Some(Kind::ApiInput),
            _ => None,
        },
        "stories.yaml" => match (top, seg(1), path.len()) {
            (true, _, _) => Some(Kind::Persona),
            // `<persona>/activities/<Activity>` — what a business metric's `activity:` binds.
            (false, Some("activities"), 3) => Some(Kind::Activity),
            _ => None,
        },
        "tests.yaml" => match (seg(0), path.len()) {
            (Some("fixtures"), 2) => Some(Kind::Fixture),
            (Some("tests"), 2) => Some(Kind::Test),
            _ => None,
        },
        "observability.yaml" => top.then_some(Kind::ObservabilityWorkflow),
        // The business-metric catalog (ADR-20260811-014129 D8): a projection is a declaration,
        // a measure a same-file declaration a fold/groupBy/value row references by `$ref`.
        "business_metrics.yaml" => match (seg(0), seg(2), path.len()) {
            (Some("projections"), _, 2) => Some(Kind::BamProjection),
            (Some("projections"), Some("measures"), 4) => Some(Kind::BamMeasure),
            _ => None,
        },
        "database/projection_views.yaml" => {
            if top {
                Some(Kind::ProjectionView)
            } else if table_column {
                Some(Kind::TableColumn)
            } else {
                None
            }
        }
        // The database catalog (#494 slice 1): each top-level entry is one declared database.
        "database/databases.yaml" => top.then_some(Kind::Database),
        "database/tables/projection_tables.yaml" => table_kind(Kind::ProjectionTable),
        "database/tables/process_managers.yaml" => table_kind(Kind::PmStateTable),
        "database/tables/referential.yaml" => table_kind(Kind::ReferentialTable),
        "database/tables/journals.yaml" => table_kind(Kind::JournalTable),
        "database/tables/integration_staging.yaml" => table_kind(Kind::StagingTable),
        "database/tables/integration_connections.yaml" => table_kind(Kind::ConnectionTable),
        "database/tables/eventstore.yaml" => table_kind(Kind::EventStoreTable),
        "database/tables/reservations.yaml" => table_kind(Kind::ReservationTable),
        f if f.ends_with(".translations.yaml") || f == "translations.yaml" => {
            top.then_some(Kind::TranslationKey)
        }
        f if f.starts_with("screens/") => match (seg(0), path.len()) {
            (Some("screens"), 2) => Some(Kind::Screen),
            // `resolvers/<key>` — one data-layer read binding (#745, `skipped_reads.resolver`,
            // referenced same-file as `#/resolvers/<key>`).
            (Some("resolvers"), 2) => Some(Kind::Resolver),
            _ => None,
        },
        _ => None,
    }
}

/// Glob over a `$ref` LOCATION: `*` matches any run of characters except `.` (so it stands for one
/// definition name / list index / map key), `**` matches anything including `.`.
pub(crate) fn glob_match(pat: &[u8], s: &[u8]) -> bool {
    if pat.starts_with(b"**") {
        let rest = &pat[2..];
        if rest.is_empty() {
            return true;
        }
        return (0..=s.len()).any(|i| glob_match(rest, &s[i..]));
    }
    match (pat.first(), s.first()) {
        (None, None) => true,
        (None, _) => false,
        (Some(b'*'), _) => {
            let rest = &pat[1..];
            let mut i = 0usize;
            loop {
                if glob_match(rest, &s[i..]) {
                    return true;
                }
                if i >= s.len() || s[i] == b'.' {
                    return false;
                }
                i += 1;
            }
        }
        (Some(pc), Some(sc)) if pc == sc => glob_match(&pat[1..], &s[1..]),
        _ => false,
    }
}

pub(crate) fn glob(pat: &str, s: &str) -> bool {
    glob_match(pat.as_bytes(), s.as_bytes())
}

/// The contract: `(source-file glob, ref-site location glob, allowed target kinds)`.
/// The location is the `$ref`'s path INSIDE its file (the leading `<file>.` is stripped), with list
/// indices as `[n]`. Order matters only for readability — every entry is tried, and a site with no
/// entry is an error (`ref-site-undeclared`).
#[rustfmt::skip]
pub(crate) const REF_CONTRACT: &[(&str, &str, &[Kind])] = &[
    // Payload shapes: a property/context/arg is a scalar, a value object, or (in api.yaml) a declared type.
    ("commands.yaml",  "*.properties.**",  &[Kind::Scalar, Kind::EnumScalar, Kind::Entity, Kind::PayloadObject]),
    ("events.yaml",    "*.properties.**",  &[Kind::Scalar, Kind::EnumScalar, Kind::Entity, Kind::PayloadObject]),
    ("entities.yaml",  "*.properties.**",  &[Kind::Scalar, Kind::EnumScalar, Kind::Entity]),
    ("errors.yaml",    "*.context.**",     &[Kind::Scalar, Kind::EnumScalar, Kind::Entity]),

    // Configuration keys are TYPED (PROP-20260729-004500): each binds the scalar whose `pattern` the
    // generated reader enforces at startup, so "present" is checked against "usable".
    ("configuration.yaml", "keys.*.scalar", &[Kind::Scalar, Kind::EnumScalar]),

    // An event DECLARES its own legal retention obligation (PROP-20260829-150752 §3.4; the shape
    // BRIEF-20260811 §3 specified). The window is a $ref into the MET-W approved catalog — never a
    // bare token, never a free duration string — which is what lets `deletion-undercuts-retention`
    // and `legal-retention-window-uncatalogued` be spec-keyed gates instead of reviewed-by-eye prose.
    ("events.yaml", "*.legalRetention.window", &[Kind::RetentionWindow]),

    // Actors (aggregates): the inbox and the lifecycle state machine. A `message` may also be the
    // actor's own reminder (`#/<Actor>/reminders/<Name>` — ADR-20260731-214500; §2f proves same-actor).
    ("actors.yaml", "*.receives[*].message",            &[Kind::Command, Kind::Event, Kind::Reminder]),
    ("actors.yaml", "*.receives[*].emits[*]",           &[Kind::Event]),
    ("actors.yaml", "*.receives[*].throws[*]",          &[Kind::Error]),
    // Reminders (typed self-messages, ADR-20260731-120825/-150500/-153000/-214500): the payload is an
    // events.yaml FACT (record semantics — never a command), the optional window a configuration key.
    ("actors.yaml", "*.reminders.*.payload",            &[Kind::Event]),
    ("actors.yaml", "*.reminders.*.after",              &[Kind::ConfigKey]),
    // A receive declares the reminders it (re)schedules — the handler's third observable effect.
    ("actors.yaml", "*.receives[*].schedules[*]",       &[Kind::Reminder]),
    // Declarative deletion (ADR-20260731-214500): triggers/undo/receipt are events, the window a
    // configuration key, and a propagation `match` is STRONGLY TYPED (event property ↔ state field).
    ("actors.yaml", "*.deletion.triggers[*].on[*]",           &[Kind::Event]),
    ("actors.yaml", "*.deletion.triggers[*].after",           &[Kind::ConfigKey]),
    ("actors.yaml", "*.deletion.triggers[*].cancelled_on[*]", &[Kind::Event]),
    ("actors.yaml", "*.deletion.triggers[*].match.event",     &[Kind::MessageProperty]),
    ("actors.yaml", "*.deletion.triggers[*].match.state",     &[Kind::StateField]),
    ("actors.yaml", "*.deletion.receipt",                     &[Kind::Event]),
    ("actors.yaml", "*.lifecycle.status",               &[Kind::EnumScalar]),
    // The actor-mailbox addressing layer (ADR-20260730-231500, PROP-20260728-152752 §2/§2.4):
    // `principals` maps each authenticated role to its resolved domain-identity scalar; `identity`
    // is a TYPED same-actor state-field ref (ADR-20260731-214500 consequences — the field is
    // implicitly declared by the ref itself, see `is_implicit_identity_state_ref`; §2d proves it).
    ("actors.yaml", "principals.*.id",                  &[Kind::Scalar]),
    ("actors.yaml", "*.identity",                       &[Kind::StateField]),
    // Write-side per-instance authorization (#235): a non-`any` acting entry binds the role to a
    // DECLARED state field of the same actor (`any` stays a bare keyword, not a ref).
    ("actors.yaml", "*.receives[*].requires.acting.*",  &[Kind::StateField]),
    // Declared aggregate state (PROP-20260728-135632 §2.1): typed fields with event(-property)
    // lineage — `from`/`removedBy` carry properties (latest/set) or whole events (flag/count);
    // `of` is the set element type (single scalar, or a named map for composite elements).
    ("actors.yaml", "*.state.*.type",                   &[Kind::Scalar, Kind::EnumScalar]),
    ("actors.yaml", "*.state.*.from[*]",                &[Kind::MessageProperty, Kind::Event]),
    ("actors.yaml", "*.state.*.removedBy[*]",           &[Kind::MessageProperty, Kind::Event]),
    ("actors.yaml", "*.state.*.of",                     &[Kind::Scalar, Kind::EnumScalar]),
    ("actors.yaml", "*.state.*.of.*",                   &[Kind::Scalar, Kind::EnumScalar]),
    // Actor `answers:` blocks (PROP-20260815-142349 §2, #582): a reply property COMPOSES the
    // actor's own declared state (never declares — `ans-reply-ref`/`ans-inline-type` own the
    // same-actor and no-inline-scalar proofs). There is NO `params` row on purpose: the key is
    // refused this slice (`ans-params-refused`, #583 hardening — nothing can consume a param on
    // the local ask), so a ref there also fails closed as `ref-site-undeclared`; the row returns
    // with the slice that gives params semantics.
    ("actors.yaml", "*.answers.*.reply.*",              &[Kind::StateField]),
    ("actors.yaml", "*.lifecycle.initial[*].event",     &[Kind::Event]),
    ("actors.yaml", "*.lifecycle.transitions[*].event", &[Kind::Event]),

    // Process managers: state-table orchestrators (ADR-20260719-…). The state table is a PM state
    // table — not any table; reads hit read models; deliver/send target aggregates.
    ("processmanager.yaml", "*.state_table",                            &[Kind::PmStateTable]),
    ("processmanager.yaml", "*.ports.*",                                &[Kind::Service]),
    ("processmanager.yaml", "*.receives[*].message",                    &[Kind::Command, Kind::Event]),
    // Wrapper-seam arms a linear step pipeline cannot express (REPLACEMENT/REFUND, #159/#207): a leg may
    // DECLARE the events it emits / errors it throws from its hand-written wrapper, merged with the
    // step-derived set, so the behaviour-test coverage checks (test-then-not-emitted / -thrown) see them.
    ("processmanager.yaml", "*.receives[*].emits[*]",                   &[Kind::Event]),
    ("processmanager.yaml", "*.receives[*].throws[*]",                  &[Kind::Error]),
    // …and the COMMANDS it sends from that wrapper (#595): a step-derived `send:` is already a
    // `$ref` site below, but an arm whose payload is COMPUTED (the replacement order's id is minted
    // from reclamationId, not mapped from a property) has no step form, so before this row the send
    // was a hand-written call the walker could not see at all. `pm-sends-kind` / `pm-sends-no-inbox`
    // hold the declaration to a real inbox.
    ("processmanager.yaml", "*.receives[*].sends[*]",                   &[Kind::Command]),
    ("processmanager.yaml", "*.receives[*].steps[*].read.model",        &[Kind::ProjectionTable, Kind::ProjectionView]),
    ("processmanager.yaml", "*.receives[*].steps[*].read.where.*.from", &[Kind::MessageProperty]),
    ("processmanager.yaml", "*.receives[*].steps[*].guard.throws",      &[Kind::Error]),
    ("processmanager.yaml", "*.receives[*].steps[*].deliver.event",     &[Kind::Event]),
    ("processmanager.yaml", "*.receives[*].steps[*].deliver.to",        &[Kind::Aggregate]),
    ("processmanager.yaml", "*.receives[*].steps[*].deliver.with.*.from", &[Kind::MessageProperty]),
    ("processmanager.yaml", "*.receives[*].steps[*].send.command",      &[Kind::Command]),
    ("processmanager.yaml", "*.receives[*].steps[*].send.to",           &[Kind::Aggregate]),
    ("processmanager.yaml", "*.receives[*].steps[*].send.with.*.from",  &[Kind::MessageProperty]),
    ("processmanager.yaml", "*.receives[*].steps[*].state.by.*.from",   &[Kind::MessageProperty]),
    ("processmanager.yaml", "*.receives[*].steps[*].state.expect.*.from", &[Kind::MessageProperty]),
    ("processmanager.yaml", "*.receives[*].steps[*].state.set.*.from",  &[Kind::MessageProperty]),

    // Service catalog (outbound ports). An input may be a domain EVENT: an outbound call sometimes
    // hands the adapter the FACT verbatim (`delivery.offer_job` takes the DeliveryRequested birth
    // fact that carries pickup/dropoff) rather than a parallel entity that would drift from it.
    ("services.yaml", "*.operations.*.input.*",  &[Kind::Scalar, Kind::EnumScalar, Kind::Entity, Kind::Event]),
    ("services.yaml", "*.operations.*.output.*", &[Kind::Scalar, Kind::EnumScalar, Kind::Entity]),
    ("services.yaml", "*.operations.*.errors[*]", &[Kind::Error]),

    // GraphQL surface. A mutation dispatches a COMMAND; a type binds to a READ MODEL (never to
    // domain_events, never to a journal/staging table).
    ("api.yaml", "types.*.properties.**",   &[Kind::Scalar, Kind::EnumScalar, Kind::Entity, Kind::ApiType]),
    ("api.yaml", "types.*.reads[*]",        READ_TARGET_KINDS),
    // The declared exception (#242 Runtime D fallout): a TRANSIENT type — one a query returns that
    // has no read model behind it — names the infrastructure it is served from, explicitly, instead
    // of certifying itself by leaving `reads:` off. A projection belongs under `reads:`, never here;
    // and this set is NARROWER than "not a projection" on purpose (see TRANSIENT_READ_KINDS) — a
    // credential store, a staging mirror and the event log are refused here exactly as under `reads:`,
    // so the new key can never be the weaker door.
    ("api.yaml", "types.*.readsInfrastructure[*]", TRANSIENT_READ_KINDS),
    ("api.yaml", "inputs.*.properties.**",  &[Kind::Scalar, Kind::EnumScalar, Kind::Entity, Kind::ApiInput]),
    ("api.yaml", "queries.*.args.*",        &[Kind::Scalar, Kind::EnumScalar, Kind::ApiInput]),
    // `argsExactlyOneOf` (#749): the one-of group's members point back at the query's OWN args
    // (same-query-ness is the `api-args-exactly-one-of` rule's check); `throws` names the typed
    // error the GENERATED zero-of/two-of check rejects with.
    ("api.yaml", "queries.*.argsExactlyOneOf.of[*]",  &[Kind::QueryArg]),
    ("api.yaml", "queries.*.argsExactlyOneOf.throws", &[Kind::Error]),
    ("api.yaml", "queries.*.returns",       &[Kind::ApiType]),
    ("api.yaml", "mutations.*.command",     &[Kind::Command]),
    ("api.yaml", "mutations.*.args.*",      &[Kind::Scalar, Kind::EnumScalar, Kind::ApiInput]),
    ("api.yaml", "mutations.*.returns",     &[Kind::ApiType]),
    ("api.yaml", "subscriptions.*.args.*",  &[Kind::Scalar, Kind::EnumScalar, Kind::ApiInput]),
    ("api.yaml", "subscriptions.*.returns", &[Kind::ApiType]),

    // Story map: every step is an API operation the persona performs.
    ("stories.yaml", "*.activities.*.steps.*", &[Kind::Query, Kind::Mutation, Kind::Subscription]),

    // Business metrics (ADR-20260811-014129 D8: two layers, every field reference a `$ref`).
    // A projection key/fold source is a specific EVENT property (provably on that event —
    // `bam-fold-key-not-on-every-event`); a measure's element type may also be a domain scalar;
    // fold `on` is the event; `set`/`add` name the projection's OWN declared measure; a metric
    // binds a persona ACTIVITY, reads `over` ONE projection, and groups/values by its measures.
    ("business_metrics.yaml", "projections.*.key[*].from",   &[Kind::MessageProperty]),
    ("business_metrics.yaml", "projections.*.measures.*.of", &[Kind::MessageProperty, Kind::Scalar, Kind::EnumScalar]),
    ("business_metrics.yaml", "projections.*.fold[*].on",    &[Kind::Event]),
    ("business_metrics.yaml", "projections.*.fold[*].set",   &[Kind::BamMeasure]),
    ("business_metrics.yaml", "projections.*.fold[*].add",   &[Kind::BamMeasure]),
    ("business_metrics.yaml", "projections.*.fold[*].from",  &[Kind::MessageProperty]),
    ("business_metrics.yaml", "metrics.*.activity",          &[Kind::Activity]),
    ("business_metrics.yaml", "metrics.*.over",              &[Kind::BamProjection]),
    ("business_metrics.yaml", "metrics.*.groupBy[*]",        &[Kind::BamMeasure]),
    ("business_metrics.yaml", "metrics.*.value.**",          &[Kind::BamMeasure]),

    // Behaviour tests (ADR-0032).
    ("tests.yaml", "fixtures.*.type",   &[Kind::Event]),
    ("tests.yaml", "tests.*.rules[*]",  &[Kind::Rule]),
    ("tests.yaml", "tests.*.actor",     &[Kind::Aggregate, Kind::ProcessManager]),
    ("tests.yaml", "tests.*.when.type", &[Kind::Command, Kind::Event]),
    // `when.gates` (RSO-1 Phase 4): a boolean configuration gate switched ON for one dispatch —
    // a $ref so the walker sees it (the #413 "silently invisible" class); boolean-ness is the
    // `test-when-gate-not-bool` rule's check.
    ("tests.yaml", "tests.*.when.gates[*]", &[Kind::ConfigKey]),
    ("tests.yaml", "tests.*.given[*]",  &[Kind::Fixture]),
    ("tests.yaml", "tests.*.then[*]",   &[Kind::Fixture]),
    ("tests.yaml", "tests.*.thrown[*]", &[Kind::Error]),

    // Observability contracts bind to the domain they diagnose.
    ("observability.yaml", "*.workflow.saga",           &[Kind::ProcessManager]),
    ("observability.yaml", "*.workflow.aggregate",      &[Kind::Aggregate]),
    ("observability.yaml", "*.workflow.command",        &[Kind::Command]),
    ("observability.yaml", "*.workflow.emits[*]",       &[Kind::Event]),
    ("observability.yaml", "*.workflow.inbound[*]",     &[Kind::Event]),
    ("observability.yaml", "*.run_identity[*].businessKey", &[Kind::Scalar, Kind::EnumScalar]),
    // A span attribute's VALUE SET, by $ref to the domain enum scalar that declares it (RSO-1,
    // "one name = one dedicated scalar" applied to value sets — never a hand-copied enum list).
    ("observability.yaml", "*.spans[*].attributes[*].values", &[Kind::EnumScalar]),
    // A metric threshold's ANTECEDENTS (#608): the declared configuration keys the number is
    // derived from. "A threshold citing a number no contract owns is not a threshold"
    // (observability) — so the derivation names its inputs as `$ref`s into configuration.yaml,
    // which makes a renamed or deleted key a red validator instead of a silently stale bound.
    ("observability.yaml", "*.metrics[*].thresholds[*].derived_from[*]", &[Kind::ConfigKey]),

    // Read models. `from` is event LINEAGE (a whole event for occurrence columns, a property
    // otherwise); `fk` is the read-navigation graph, so it must name a COLUMN.
    ("database/projection_views.yaml", "nonProjectedEvents[*]", &[Kind::Event]),
    ("database/projection_views.yaml", "*.tombstone",           &[Kind::Event]),
    ("database/projection_views.yaml", "*.fedBy[*]",            &[Kind::Event]),
    ("database/projection_views.yaml", "*.columns.*.type",      &[Kind::Scalar, Kind::EnumScalar]),
    ("database/projection_views.yaml", "*.columns.*.from[*]",   &[Kind::Event, Kind::MessageProperty]),
    ("database/projection_views.yaml", "*.columns.*.fk",        &[Kind::TableColumn]),

    // Real tables (globbed): every column types to a domain scalar; FKs name a column. `database`
    // is the single-home PLACEMENT key (#494 slice 1) — a `$ref` into database/databases.yaml,
    // never a bare string (ADR-20260811-014129 D2: a bare name is the #413 invisible-to-the-walker
    // class — a typo'd or truncated database name would place a credential store nowhere, silently;
    // a `$ref` makes it a resolved reference or a red validator).
    ("database/tables/*.yaml", "*.database",          &[Kind::Database]),
    ("database/tables/*.yaml", "*.tombstone",         &[Kind::Event]),
    ("database/tables/*.yaml", "*.fedBy[*]",          &[Kind::Event]),
    ("database/tables/*.yaml", "*.columns.*.type",    &[Kind::Scalar, Kind::EnumScalar]),
    ("database/tables/*.yaml", "*.columns.*.from[*]", &[Kind::Event, Kind::MessageProperty]),
    ("database/tables/*.yaml", "*.columns.*.fk",      &[Kind::TableColumn]),

    // SDUI screens (ADR-0033/0037): reads are queries, writes are mutations, live updates are
    // subscriptions — and EVERY other ref in the (free-form, deeply nested) UI tree is an i18n key,
    // which is what `screen-ref-out-of-scope` already asserts. Order matters: first match wins.
    ("screens/*.yaml", "resolvers.**",     &[Kind::Query]),
    ("screens/*.yaml", "actions.**",       &[Kind::Mutation]),
    ("screens/*.yaml", "**.subscription",  &[Kind::Subscription]),
    // A `skipped_reads` declaration (#745, §25b): `resolver` names the surface's own data-layer
    // binding (same-file `#/resolvers/<key>`), `missing_arg` the REQUIRED api.yaml query arg the
    // screen has no paint-time source for. Declared BEFORE the `**` catch-all — first match wins.
    ("screens/*.yaml", "screens[*].skipped_reads[*].resolver",    &[Kind::Resolver]),
    ("screens/*.yaml", "screens[*].skipped_reads[*].missing_arg", &[Kind::QueryArg]),
    ("screens/*.yaml", "**",               &[Kind::TranslationKey]),

    // C4 model (source DSL, not generated): containers/components bind to the actors they realize.
    ("architecture/c4-l2.yaml", "boundedContexts.*.aggregates[*]",      &[Kind::Aggregate]),
    ("architecture/c4-l2.yaml", "containers.*.realizes[*]",             &[Kind::Aggregate, Kind::ProcessManager]),
    ("architecture/c4-l2.yaml", "boundedContexts.*.processManagers[*]", &[Kind::ProcessManager]),
    ("architecture/c4-l3.yaml", "components.*.handles[*]", &[Kind::Aggregate, Kind::ProcessManager]),
    ("architecture/c4-l3.yaml", "components.*.updates[*]", &[Kind::ProjectionView, Kind::ProjectionTable]),
    ("architecture/c4-l3.yaml", "components.*.reads[*]", &[Kind::ProjectionView, Kind::ProjectionTable]),
];

/// The DSL's own FIELD names — every other segment of a `$ref` location is a definition/instance name
/// (a command, a screen, a column, a persona…). Used only to turn an undeclared site into a suggested
/// contract pattern: field names stay literal, name positions become `*`.
pub(crate) const STRUCTURAL_SEGMENTS: &[&str] = &[
    "actions", "activities", "actor", "after", "args", "by", "call", "cancelled_on", "columns",
    "command", "content", "context", "deletion", "deliver", "emits", "event", "expect", "fixtures",
    "database", "from", "from_hook", "given", "guard", "inputs",
    "acting", "claims", "identity", "lifecycle", "mailbox", "match", "message", "messages", "model",
    "mutations", "of", "on", "operations", "params", "payload", "ports", "principals", "receipt",
    "reminders", "removedBy", "requires",
    "projections", "measures", "fold", "groupBy", "over", "activity", "value", "key",
    "ofDurationMs", "start", "end",
    "missing_arg", "resolver", "skipped_reads",
    "properties", "queries", "read", "reads", "receives", "resolvers", "returns", "rules",
    "schedules", "screens", "send", "set", "state", "state_table", "status", "steps",
    "subscriptions", "tests", "then", "throws", "thrown", "to", "transitions", "triggers", "type",
    "types", "when", "where", "with", "workflows",
];

/// Turn a concrete `$ref` site into the contract pattern that would cover it: list indices → `[*]`,
/// definition/instance names → `*`, DSL field names kept literal.
pub(crate) fn normalize_site(site: &str) -> String {
    site.split('.')
        .map(|part| {
            // Split a segment into its name and any trailing `[index]` suffixes.
            let (name, idx) = match part.find('[') {
                Some(i) => (&part[..i], &part[i..]),
                None => (part, ""),
            };
            let name = if STRUCTURAL_SEGMENTS.contains(&name) { name } else { "*" };
            let mut idx_out = String::new();
            let mut depth = 0;
            for ch in idx.chars() {
                match ch {
                    '[' => {
                        depth += 1;
                        idx_out.push_str("[*");
                    }
                    ']' => {
                        depth -= 1;
                        idx_out.push(']');
                    }
                    _ if depth > 0 => {}
                    c => idx_out.push(c),
                }
            }
            format!("{}{}", name, idx_out)
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// §1b — every `$ref` site must be declared, and its target must be of an allowed kind.
pub(crate) fn validate_ref_kinds(model: &Model, issues: &mut Vec<Issue>) {
    // Which commands.yaml entries are real COMMANDS (received by an actor or a process manager) —
    // the rest are shared payload sub-objects. See `Kind::PayloadObject`.
    let mut handled: BTreeSet<String> = BTreeSet::new();
    for f in ["actors.yaml", "processmanager.yaml"] {
        let mut refs = Vec::new();
        if let Some(v) = model.defs.get(f) {
            collect_refs(v, f, &mut refs);
        }
        for (loc, r) in refs {
            let site = loc.strip_prefix(f).and_then(|s| s.strip_prefix('.')).unwrap_or(&loc);
            if glob("*.receives[*].message", site) && ref_target_file(&r, f).as_deref() == Some("commands.yaml") {
                if let Some(n) = ref_name(&r) {
                    handled.insert(n);
                }
            }
        }
    }
    // Undeclared sites are reported once per NORMALIZED pattern (definition name and list indices
    // wildcarded), so the message doubles as the contract line to add.
    let mut undeclared: BTreeMap<String, (String, String, usize)> = BTreeMap::new();
    for (f, v) in &model.defs {
        let file = f.as_str();
        let mut refs = Vec::new();
        collect_refs(v, file, &mut refs);
        for (loc, r) in refs {
            let site = loc.strip_prefix(file).and_then(|s| s.strip_prefix('.')).unwrap_or(&loc);
            let allowed: Option<&[Kind]> = REF_CONTRACT
                .iter()
                .find(|(fg, lg, _)| glob(fg, file) && glob(lg, site))
                .map(|(_, _, k)| *k);
            let allowed = match allowed {
                Some(k) => k,
                None => {
                    let e = undeclared
                        .entry(format!("{}|{}", file, normalize_site(site)))
                        .or_insert((loc.clone(), site.to_string(), 0));
                    e.2 += 1;
                    continue;
                }
            };
            // Kind check (dangling/malformed refs are §1's job — skip what does not resolve).
            let pr = match parse_ref(&r) {
                Some(p) => p,
                None => continue,
            };
            let target_file = if pr.file.is_empty() { file.to_string() } else { pr.file.clone() };
            let node = match resolve_ref(model, &r, file) {
                Some(n) => n,
                None => continue,
            };
            match classify(&target_file, &pr.path, node, &handled) {
                Some(k) if allowed.contains(&k) => {}
                Some(k) => {
                    // A commands.yaml entry only counts as a COMMAND once an actor receives it —
                    // spell that out rather than leaving "is a payload object" to be decoded.
                    let hint = if k == Kind::PayloadObject && allowed.contains(&Kind::Command) {
                        " (no actor or process manager receives it — wire it into an inbox, or move it to entities.yaml if it is a payload shape)"
                    } else {
                        ""
                    };
                    issues.push(err(
                        "ref-kind",
                        loc.clone(),
                        format!("$ref '{}' is a {}; this site requires a {}{}.", r, k.name(), kind_list(allowed), hint),
                    ))
                }
                None => issues.push(err(
                    "ref-kind-unknown",
                    loc.clone(),
                    format!("$ref '{}' does not name a classifiable definition (expected a {}).", r, kind_list(allowed)),
                )),
            }
        }
    }
    for (key, (example, example_site, count)) in undeclared {
        let (file, norm) = key.split_once('|').unwrap_or(("?", "?"));
        issues.push(err(
            "ref-site-undeclared",
            example,
            format!(
                "no ref-kind contract for the $ref site '{}' ({} occurrence(s)) — declare what it may point at, e.g. (\"{}\", \"{}\", &[…]) in REF_CONTRACT.",
                example_site, count, file, norm
            ),
        ));
    }
}

