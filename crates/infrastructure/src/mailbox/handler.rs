//! The mailbox delivery glue: [`MailboxCommandHandler`] implements the runtime's
//! [`actor_runtime::MessageHandler`] over the GENERATED command router — deserialize, resolve the
//! acting principal (the SAME #235 by_auth_ref bridge the GraphQL edge applies), run the handler
//! against a per-delivery [`StagingEventStore`], flush the staged events into the completion
//! transaction, and map the outcome onto the row's terminal verdict. [`StatusBusObserver`]
//! publishes each COMMITTED command verdict on the [`actor_client::OperationStatusBus`] (the
//! §2.1 response bus, behind the boundary crate since #303) — after the commit, never before, so
//! `ActorClient::watch` consumers only ever hear durable facts.

use super::attribution;
use std::sync::Arc;

use actor_runtime::{Delivery, DeliveryObserver, HandlerVerdict, InboundMessage, MessageHandler};
use application::ports::{is_version_conflict, Actor, EventStore};
use application::staging::StagingEventStore;
use domain::shared::errors::DomainError;
use sqlx::{Postgres, Transaction};

use application::payments::RecordOutcome;

use actor_client::status_bus::{OperationStatusBus, OperationUpdate};

use crate::generated::command_router::CommandDeps;
use crate::inbox::InboxOutcome;
use crate::persistence::event_bus::{AppendedEvent, EventBus};

use super::activation::{ActivationSettings, DeliveryActivation};
use super::{flush_staged_in_tx, pm_delivery};

/// The delivery glue for all three kinds: COMMAND (generated router), EVENT (adapted inbound
/// facts) and MESSAGE (promoted reminders, ADR-20260731-153000 — record semantics, like EVENT).
/// An unroutable kind or type completes FAILED with a routing error so a misrouted row is loud,
/// never silently swallowed.
pub struct MailboxCommandHandler {
    deps: CommandDeps,
    /// When present, each committed delivery publishes its appended events on the in-process bus
    /// (the GraphQL domain-fact subscriptions) — POST-COMMIT, via the runtime's Delivery hook,
    /// exactly where the pool-backed PgEventStore publishes.
    event_bus: Option<EventBus>,
    /// Reminder window keys → typed Duration (`Config::reminder_windows()`, which applies each
    /// key's declared `unit:` — #167): what `apply_schedules_in_tx` resolves a `schedules:`
    /// declaration's `after` against. Empty = no windows wired; a delivery that then needs one
    /// aborts for retry (wiring bug, never a terminal verdict).
    reminder_windows: std::collections::HashMap<&'static str, std::time::Duration>,
    /// Enqueue-side wake signals: a committed chain hop nudges the PM lane's worker post-commit,
    /// cutting the saga hop from the heartbeat poll to ~immediate (B2's "nudged" property).
    nudges: Option<std::sync::Arc<crate::persistence::mailbox_store::MailboxNudges>>,
    /// ACTIVATIONS (#272 D3, PROP-20260728-152752 §3.5, gated `ACTOR_ACTIVATIONS`): when wired,
    /// each delivery folds its own stream through the held-state cache (fill on miss, promote
    /// post-commit, invalidate on a lost version race) — fold-on-first-message instead of every
    /// message. `None` = the gate is off; every delivery folds from the log, exactly as before.
    activations: Option<Arc<ActivationSettings>>,
}

impl MailboxCommandHandler {
    pub fn new(deps: CommandDeps) -> Self {
        Self {
            deps,
            event_bus: None,
            reminder_windows: Default::default(),
            nudges: None,
            activations: None,
        }
    }

    /// Wire the activation cache (the `ACTOR_ACTIVATIONS` gate resolved at the composition root).
    pub fn with_activations(mut self, settings: Arc<ActivationSettings>) -> Self {
        self.activations = Some(settings);
        self
    }

    pub fn with_event_bus(mut self, bus: EventBus) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// Wire the configured reminder windows (composition root: `Config::reminder_windows()`).
    pub fn with_reminder_windows(
        mut self,
        windows: std::collections::HashMap<&'static str, std::time::Duration>,
    ) -> Self {
        self.reminder_windows = windows;
        self
    }

    /// Wire the per-actor-type wake signals so chained hops deliver ~immediately.
    pub fn with_nudges(
        mut self,
        nudges: std::sync::Arc<crate::persistence::mailbox_store::MailboxNudges>,
    ) -> Self {
        self.nudges = Some(nudges);
        self
    }
}

/// What the door does with a row it could not turn into a typed value.
///
/// Pure and separate from the handler so the verdict table can assert the POSTURE — the property
/// that decides whether a row survives — without a database and without reading any message text
/// (`mailbox::handler::tests::the_door_verdict_table`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorPosture {
    /// ABORT the delivery: the row stays RECEIVED, the attempt counter advances, the runtime
    /// retries under backoff and — if nothing improves — flips it into the POISON queue at the
    /// cap, where `poisonedMailboxMessages` shows it and `RequeueMailboxMessage` recovers it.
    Park,
    /// COMMIT a terminal FAILED verdict: the bytes in the row are wrong and no retry, and no
    /// deploy on the other side, can change them.
    Terminal,
}

/// The door's posture for a parse failure.
///
/// `UnknownActor` / `UndeclaredMessage` are PARK because a build on the other side of a rolling
/// deploy can route them — terminal-failing an unknown type during a deploy buries a paid order.
/// `Payload` is TERMINAL because it is a deterministic shape failure of a message this build DOES
/// understand, including the case where the row's `message_type` disagrees with the staged
/// `eventType` tag.
pub fn parse_posture(err: &application::generated::inboxes::InboxParseError) -> DoorPosture {
    if err.is_transient() {
        DoorPosture::Park
    } else {
        DoorPosture::Terminal
    }
}

#[async_trait::async_trait]
impl MessageHandler for MailboxCommandHandler {
    /// The PREPARE phase (ADR-20260801-023000): the three PM commands run their WHOLE legacy
    /// handler here — pool reads and the Stripe call, no transaction open — capturing every
    /// effect for the fenced commit. Every other message has no prepare work.
    async fn prepare(
        &self,
        message: &InboundMessage,
    ) -> Result<actor_runtime::Prepared, sqlx::Error> {
        if message.kind == "COMMAND" && pm_delivery::is_pm_command(&message.message_type) {
            let actor = self.resolve_actor(message).await?;
            let prepared = pm_delivery::prepare(&self.deps, message, &actor).await?;
            return Ok(Some(Box::new(prepared)));
        }
        Ok(None)
    }

    async fn handle(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        message: &InboundMessage,
        prepared: actor_runtime::Prepared,
    ) -> Result<Delivery, sqlx::Error> {
        // THE DOOR (#771). One parse, taking the LANE and the message type together, before any
        // routing decision. Until #771 the router took `message.message_type` and never
        // `message.actor_type`, so a row on lane A could drive a handler that writes aggregate B —
        // under A's fence. That is ADR-20260829-230418 ("Aggregates own the facts") violated by the
        // transport itself, and the fix is that a lane/message pair the spec does not declare is a
        // value that cannot be constructed. The enqueue side already refused undeclared pairs
        // (`mailbox_address`, `ACTOR_INBOUND_FACTS`); this closes the consuming side.
        let inbox = match application::generated::inboxes::ActorInbox::parse(
            &message.actor_type,
            &message.message_type,
            &message.payload,
        ) {
            Ok(inbox) => inbox,
            // TRANSIENT, NEVER TERMINAL. During a rolling deploy an OLD consumer legitimately meets
            // a message type a NEWER producer already emits; terminal-failing it buries a paid
            // order. Aborting the delivery leaves the row RECEIVED, so the runtime retries it with
            // exponential backoff and — if the other side of the deploy never arrives — flips it at
            // `max_delivery_attempts` into the POISON queue, which is loud by construction:
            // `mailbox_poison_failed_total{actor_type}`, the ADMIN `poisonedMailboxMessages` read
            // and `RequeueMailboxMessage` as the operator's way back. No new status and no
            // migration: park is the existing poison path, reached by aborting instead of failing.
            Err(e) if parse_posture(&e) == DoorPosture::Park => {
                return Err(sqlx::Error::Protocol(format!(
                    "mailbox: {e} -- TRANSIENT (rolling deploy?); retrying, then parking on the \
                     poison queue rather than burying the row"
                )));
            }
            // A DECLARED message whose payload does not deserialize is a deterministic shape
            // failure of a message this build DOES understand: retrying cannot fix it.
            //
            // THE CONTEXT GOES THROUGH `attribution` (#623, #780). `inbound_messages.error` is a
            // 90-day durable jsonb column, and `e.to_string()` here was one of the legacy
            // `detail: <free text>` sites the leak canary's header names. The bounded
            // `CommandFailureAttribution` says the same thing in a closed vocabulary — seam
            // COMMAND_PAYLOAD, reason PAYLOAD_UNDECODABLE — and the diagnostic text goes to the LOG,
            // which is where an operator with a correlation id looks and where nothing retains it.
            Err(e) => {
                // The detail rides the DomainError for the log's benefit only: `context_of` emits
                // seam + reason and nothing else, so the free text cannot reach the column.
                let undecodable =
                    attribution::command_payload_undecodable(&message.message_type, &e.to_string());
                tracing::error!(
                    actor_type = %message.actor_type,
                    message_type = %message.message_type,
                    message_id = %message.message_id,
                    kind = %message.kind,
                    detail = %e,
                    "mailbox: DECLARED message payload did not decode -- terminal (retrying cannot \
                     change the bytes in the row)"
                );
                // A FACT that cannot be decoded never reaches its aggregate either, so it belongs
                // to the same `mailbox-delivery` counter as a parked one, under its own reason.
                // A COMMAND's undecodable payload is already the `command-acceptance` contract's.
                if message.kind == "EVENT" || message.kind == "MESSAGE" {
                    telemetry::meters::mailbox::fact_unrecorded(
                        &message.actor_type,
                        &message.message_type,
                        telemetry::meters::mailbox::FACT_UNRECORDED_UNPARSABLE,
                    );
                }
                return Ok(Delivery::of(HandlerVerdict::Failed(serde_json::json!({
                    "code": "Internal",
                    "context": attribution::context_of(&attribution::attribute(&undecodable))
                }))));
            }
        };
        if message.kind == "COMMAND" && pm_delivery::is_pm_command(&message.message_type) {
            // A PM command's effects were computed in prepare; this phase only commits them.
            let prepared = prepared
                .and_then(|p| p.downcast::<pm_delivery::PreparedPmCommand>().ok())
                .ok_or_else(|| {
                    sqlx::Error::Protocol(
                        "PM command delivered without its prepared phase (wiring bug)".into(),
                    )
                })?;
            return self.commit_prepared_pm(tx, message, *prepared).await;
        }
        if message.kind == "EVENT" || message.kind == "MESSAGE" {
            // Both kinds RECORD facts: EVENT carries an adapted inbound business fact, MESSAGE a
            // promoted reminder's payload fact (ADR-20260731-153000 §1a) — same record semantics,
            // same route.
            //
            // THE TYPED FACT DOOR (#780). The already-parsed value is projected onto its lane's
            // FACT half; a COMMAND row on an EVENT/MESSAGE kind yields `None` and is a wiring bug,
            // aborted for retry rather than terminally failed. Nothing re-parses the payload here
            // any more: `ActorInbox::parse` did the `DomainEvent` deserialize AND the
            // `eventType`-vs-`message_type` cross-check, so the old "unparsable staged DomainEvent"
            // terminal arm — a third silent loss, invisible to the poison queue — is gone by
            // construction rather than instrumented.
            let Some(fact) = inbox.into_fact() else {
                return Err(sqlx::Error::Protocol(format!(
                    "mailbox: '{}' on the '{}' lane arrived with kind '{}' but is a COMMAND (wiring \
                     bug); aborting for retry rather than recording a verdict for our own mistake",
                    message.message_type, message.actor_type, message.kind
                )));
            };
            return self.handle_recorded_fact(tx, message, fact).await;
        }
        if message.kind != "COMMAND" {
            return Ok(Delivery::of(HandlerVerdict::Failed(serde_json::json!({
                "code": "Internal",
                "context": { "detail": format!("unroutable mailbox kind '{}'", message.kind) }
            }))));
        }

        let actor = self.resolve_actor(message).await?;

        // Stage-don't-write: the handler runs unchanged over a buffering store; its events only
        // become true when the runtime commits the fenced transaction this flush joins. With
        // activations wired, the delivered actor's OWN stream folds through the held-state cache.
        let activation = DeliveryActivation::for_message(&self.activations, message);
        let base_store: Arc<dyn EventStore> = match &activation {
            Some(a) => a.store(self.deps.store.clone()),
            None => self.deps.store.clone(),
        };
        let staging = Arc::new(StagingEventStore::new(base_store));
        let mut deps = self.deps.clone();
        deps.store = staging.clone() as Arc<dyn EventStore>;

        // The typed route (#771): a CLOSED enum into a match the compiler proves exhaustive. There
        // is no "unroutable command type" arm any more, because there is no unroutable command:
        // a message an actor declares it receives and nobody consumes is now an E0004 build
        // failure in `crate::inbox`, not a FAILED row a customer pays for.
        let outcome = crate::inbox::route(
            &deps,
            inbox,
            &actor,
            &crate::inbox::RouterEnv { session_id: message.session_id },
        )
        .await;

        let delivery = match outcome {
            // DECLARED received, handler deliberately not built yet (actors.yaml `deferred:`).
            // Terminal on purpose: unlike an unknown type, no deploy on the other side will grow a
            // handler for it, so retrying would only burn the poison budget. The deferral carries
            // its reason and tracking issue in the spec.
            InboxOutcome::Deferred => Delivery::of(HandlerVerdict::Failed(serde_json::json!({
                "code": "Internal",
                "context": {
                    "detail": format!(
                        "'{}' is DECLARED on the '{}' inbox with its handler deferred (actors.yaml `deferred:`)",
                        message.message_type, message.actor_type
                    )
                }
            }))),
            // A fact/PM leg cannot reach the COMMAND door: the kind branches above return first.
            // Reaching here is a wiring bug, not a business outcome — abort so it retries and then
            // parks loudly, rather than recording a terminal verdict for our own mistake.
            InboxOutcome::RecordFact | InboxOutcome::ProcessManagerLeg => {
                return Err(sqlx::Error::Protocol(format!(
                    "mailbox: '{}' on the '{}' COMMAND door routed to a non-command outcome (wiring bug)",
                    message.message_type, message.actor_type
                )));
            }
            InboxOutcome::Handled(Ok(())) => {
                let staged = staging.take_staged();
                // Freshness guard BEFORE the flush: after it, MAX(version) would include this
                // delivery's own appends and a legitimate append would read as stale.
                if let Some(a) = &activation {
                    a.guard_freshness_in_tx(tx).await?;
                }
                match flush_staged_in_tx(tx, &staged).await {
                    Ok(()) => {
                        // A routed birth can arrive on the COMMAND door too (#595: the replacement
                        // order is born by `PlaceReplacementOrder`, not by a delivered fact), and
                        // it is the SAME handover `order_birth_lag_ms` was declared to measure —
                        // enqueue to `Recorded`. `routed` is read from the DECLARED route table
                        // rather than from a config flag: the flag says what the NEXT enqueue will
                        // do, while the table says whether THIS (actor, message) pair is one a
                        // lane route produces — the honest answer for a row that was enqueued
                        // before a rollback and delivered after it.
                        super::record_order_birth_lag(
                            message,
                            &staged,
                            application::generated::process_managers::ROUTED_LANES.iter().any(|l| {
                                l.actor_type == message.actor_type
                                    && l.event_type == message.message_type
                            }),
                        );
                        // The handler's third observable effect (ADR-20260731-214500 §2): a
                        // successful delivery (re)declares its `schedules:` reminders in the
                        // SAME transaction — commit and clock start together or not at all.
                        super::apply_schedules_in_tx(tx, message, &self.reminder_windows)
                            .await?;
                        let promote = DeliveryActivation::promote_after_commit(
                            &self.activations,
                            activation.as_ref(),
                            &staged,
                        );
                        self.fanout_delivery(&staged, None, promote)
                    }
                    // A version clash at commit time: a concurrent writer (a legacy-path PM leg,
                    // another lane) moved the stream between the handler's load and this flush.
                    // ABORT the delivery — the row stays RECEIVED and the retry re-runs the
                    // handler against the moved stream. Contention is transient by construction
                    // (each retry reloads), so retry-in-place converges; a terminal FAILED here
                    // would make a peak-time clash cost the client a manual resubmit. The held
                    // state provably lost the race: drop it so the retry refolds.
                    Err(e) if is_version_conflict(&e) => {
                        if let Some(a) = &activation {
                            a.invalidate_scoped();
                        }
                        return Err(sqlx::Error::Protocol(e.to_string()));
                    }
                    Err(DomainError::Repository(detail)) => {
                        // Infrastructure failure mid-flush: abort the delivery (row stays RECEIVED,
                        // redelivery retries) rather than recording a verdict we are not sure of.
                        return Err(sqlx::Error::Protocol(detail));
                    }
                    Err(e) => Delivery::of(HandlerVerdict::Failed(serde_json::json!({
                        "code": "Internal",
                        "context": { "detail": e.to_string() }
                    }))),
                }
            }
            // A transient infrastructure failure INSIDE the handler (a repository read, a gateway
            // call) aborts the delivery for retry — only deterministic outcomes may land a
            // terminal verdict. A terminal FAILED here would be absorbed by the enqueue-side pk
            // dedupe on redelivery, turning one DB blip into a permanently lost message.
            InboxOutcome::Handled(Err(DomainError::Repository(detail))) => {
                return Err(sqlx::Error::Protocol(detail));
            }
            InboxOutcome::Handled(Err(e)) => {
                // A deterministic rejection stages nothing, so no UNIQUE race can catch a stale
                // fold — the freshness guard is the ONLY fence between a held state and a
                // durably wrong REJECTED (reviewer CRITICAL, 2026-08-01).
                if let Some(a) = &activation {
                    a.guard_freshness_in_tx(tx).await?;
                }
                Delivery::of(verdict_of_error(e))
            }
        };
        Ok(delivery)
    }
}

impl MailboxCommandHandler {
    /// The acting principal, envelope → Actor (ADR-0041) with the #235 domain-identity bridge:
    /// a CUSTOMER's auth subject resolves to its CustomerId — the value `requires.acting`
    /// compares against folded participants. Same logic as the GraphQL edge; other roles stay
    /// None until their bridges land (#144). An infrastructure failure resolving the principal
    /// ABORTS the delivery (row stays RECEIVED, redelivery retries) — swallowing it into `None`
    /// would durably record a legitimate customer's command as REJECTED NotAParticipant, a
    /// wrong-class terminal verdict for a transient DB blip.
    async fn resolve_actor(&self, message: &InboundMessage) -> Result<Actor, sqlx::Error> {
        let domain_id = if message.user_type == "CUSTOMER" {
            match message.user_id {
                Some(uid) => match self
                    .deps
                    .customers
                    .by_auth_ref(domain::generated::scalars::ExternalReference(uid.to_string()))
                    .await
                {
                    Ok(customer) => customer.map(|c| c.customer_id.0),
                    Err(e) => return Err(sqlx::Error::Protocol(e.to_string())),
                },
                None => None,
            }
        } else {
            None
        };
        Ok(Actor {
            user_id: message.user_id.unwrap_or_else(uuid::Uuid::nil),
            user_type: message.user_type.clone(),
            domain_id,
            correlation_id: message.correlation_id,
            cause_id: Some(message.message_id),
        })
    }

    /// Commit one prepared PM command (ADR-20260801-023000): flush the staged events + the PM
    /// run rows into the fenced transaction, or land the captured deterministic rejection as the
    /// row's verdict — the SAME operation outcome the legacy spawn path produced, byte-identical
    /// to the client.
    async fn commit_prepared_pm(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        message: &InboundMessage,
        prepared: pm_delivery::PreparedPmCommand,
    ) -> Result<Delivery, sqlx::Error> {
        match prepared.outcome {
            // Deterministic rejection captured in prepare (validation, sync Stripe decline):
            // committed as the terminal verdict; no effect to flush.
            Err(e) => Ok(Delivery::of(verdict_of_error(e))),
            Ok(effects) => match flush_staged_in_tx(tx, &effects.staged).await {
                Ok(()) => {
                    pm_delivery::flush_pm_rows_in_tx(
                        tx,
                        &effects.payment_rows,
                        &effects.refund_rows,
                    )
                    .await
                    .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
                    super::apply_schedules_in_tx(tx, message, &self.reminder_windows).await?;
                    // No scoped activation on a PM lane (its prepare folds through pool
                    // stores), but the committed appends touch AGGREGATE streams other lanes
                    // may hold — the promotion closure invalidates them.
                    let promote = DeliveryActivation::promote_after_commit(
                        &self.activations,
                        None,
                        &effects.staged,
                    );
                    Ok(self.fanout_delivery(&effects.staged, None, promote))
                }
                // A version clash at flush: a concurrent writer moved a stream between prepare's
                // load and this commit. ABORT for retry — redelivery re-runs prepare against the
                // moved stream, and the Stripe idempotency key returns the SAME intent/refund,
                // so the retry can never double-charge (the ADR's crash-window argument).
                Err(e) if is_version_conflict(&e) => Err(sqlx::Error::Protocol(e.to_string())),
                Err(DomainError::Repository(detail)) => Err(sqlx::Error::Protocol(detail)),
                Err(e) => Ok(Delivery::of(HandlerVerdict::Failed(serde_json::json!({
                    "code": "Internal",
                    "context": { "detail": e.to_string() }
                })))),
            },
        }
    }

    /// The kind-EVENT / kind-MESSAGE delivery route (ADR-20260731-122500): adapted inbound
    /// BUSINESS facts (the mailbox-era home of the retired InboundEventsDrainWorker's routing)
    /// and promoted reminder facts (ADR-20260731-153000 §1a — record semantics, never Rejected).
    /// The aggregate's fold-based dedupe stays authoritative — its verdict is PERSISTED on the
    /// row (a no-change decision lands IGNORED, a redelivered fact DUPLICATE, per
    /// ADR-20260728-011344 D6), and the staged events flush into the SAME fenced transaction as
    /// every command delivery.
    async fn handle_recorded_fact(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        message: &InboundMessage,
        fact: application::generated::inboxes::ActorFactInbox,
    ) -> Result<Delivery, sqlx::Error> {
        use tracing::Instrument as _;
        // `message.deliver` (specs/observability.yaml#/mailbox-delivery, #780). The whole route was
        // uninstrumented: no span here, and `StatusBusObserver::committed` returns early for any
        // row whose kind is not COMMAND, so a lost fact was a ZERO-SIGNAL event.
        let span = telemetry::spans::message_deliver(
            fact.actor_type(),
            fact.message_type(),
            &message.kind,
            &message.message_id.to_string(),
            &message.correlation_id.to_string(),
        );
        let leg = crate::inbox::fact_route(fact);
        let leg = match leg {
            // PARKED: recorded on the span BEFORE the return, because an aborted delivery has no
            // committed verdict for anything downstream to read.
            crate::inbox::FactLeg::Unrecorded(u) => {
                telemetry::spans::record_message_deliver_verdict(&span, "parked");
                telemetry::spans::record_message_deliver_error(&span);
                let _enter = span.enter();
                return self.park_unrecorded_fact(message, u);
            }
            other => other,
        };
        let result = self.deliver_fact_leg(tx, message, leg).instrument(span.clone()).await;
        // KNOWN GAP, tracked in
        // [#791](https://github.com/TheCaptainCompany/captain-food/issues/791): the `Err` arm
        // records NEITHER a verdict NOR a span error, so an aborted delivery exports with
        // `business.verdict` unset and satisfies this contract's `verdict != 'parked'` success
        // condition vacuously. Not fixed here because the fix is the contract's, not this call
        // site's: a success condition written as "!= one member" is satisfied by ABSENCE.
        if let Ok(delivery) = &result {
            telemetry::spans::record_message_deliver_verdict(
                &span,
                match delivery.verdict {
                    HandlerVerdict::Failed(_) => {
                        telemetry::spans::record_message_deliver_error(&span);
                        "failed"
                    }
                    HandlerVerdict::Succeeded => "recorded",
                    HandlerVerdict::Duplicate => "duplicate",
                    HandlerVerdict::Ignored => "ignored",
                    HandlerVerdict::Rejected(_) => "rejected",
                },
            );
        }
        result
    }

    /// A DECLARED fact the receiving aggregate has no fold rule for: PARK it.
    ///
    /// **NEVER TERMINAL** (ADR-20260830-224500). A COMMAND may be refused — that is what makes it a
    /// command (ADR-0004) — but a FACT already happened somewhere else and cannot be. A terminal
    /// verdict here would also be unrecoverable in practice: `error->>'code' = 'Internal'` is
    /// invisible to `poisonedMailboxMessages` and refused by `RequeueMailboxMessage`
    /// (`rules.yaml#/OnlyCapPoisonedMailboxRowsAreRequeueable`), and the redelivery that supposedly
    /// "costs only a re-send" is absorbed by the enqueue-side pk dedupe.
    ///
    /// Aborting instead leaves the row RECEIVED: it retries under backoff and, at the cap, the
    /// RUNTIME flips it with `DeliveryInfrastructureError` — poison-visible, requeueable, through
    /// the operator recovery that already exists, with that code still meaning exactly what
    /// `specs/common/rules.yaml` says it means. It is the SAME posture the door already takes for an
    /// `UndeclaredMessage`: *retry, then park loudly, rather than bury the row.*
    ///
    /// Nothing free-text reaches `inbound_messages.error` from here, because nothing is written to
    /// it at all: the diagnosis goes to the LOG (where an operator with a correlation id finds it
    /// and nothing retains it for ninety days) and to the counter.
    fn park_unrecorded_fact(
        &self,
        message: &InboundMessage,
        unrecorded: crate::inbox::UnrecordedFact,
    ) -> Result<Delivery, sqlx::Error> {
        telemetry::meters::mailbox::fact_unrecorded(
            unrecorded.actor_type,
            unrecorded.message_type,
            telemetry::meters::mailbox::FACT_UNRECORDED_DEFERRED,
        );
        tracing::error!(
            actor_type = %unrecorded.actor_type,
            message_type = %unrecorded.message_type,
            message_id = %message.message_id,
            correlation_id = ?message.correlation_id,
            "mailbox: a DECLARED fact has no record route (actors.yaml `deferred:`) -- PARKING on \
             the poison path rather than losing the fact"
        );
        Err(sqlx::Error::Protocol(format!(
            "mailbox: '{}' is DECLARED on the '{}' inbox with no record route (actors.yaml \
             `deferred:`) -- PARKED, not failed: a fact cannot be refused",
            unrecorded.message_type, unrecorded.actor_type
        )))
    }

    /// The recorded/PM legs of one fact delivery — everything that needs the fenced transaction.
    async fn deliver_fact_leg(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        message: &InboundMessage,
        leg: crate::inbox::FactLeg,
    ) -> Result<Delivery, sqlx::Error> {
        // THE ROUTING DECISION was taken by `crate::inbox::fact_route` in the caller: pure, total
        // and transaction-free. It is human-owned and matches on `ActorFactInbox`, where a COMMAND
        // variant is unspellable — so a declared fact nobody consumes is an E0004 at build time,
        // never a terminal `FAILED "no delivery route"` row a customer paid for.
        //
        // THE LANE IS READ FROM THE ROW, NEVER DERIVED FROM THE PAYLOAD (vernon): `ActorInbox::parse`
        // keyed off `message.actor_type` and refuses a lane/message pair the spec does not declare,
        // so a Payment fact on a `PlaceOrderProcess` row is a value that could not be constructed.
        let record = match leg {
            // A chained PM-addressed copy (B2): the lane IS the saga — run its event leg, not the
            // record route (the fact is already on the Payment stream; this hop reacts to it).
            crate::inbox::FactLeg::ProcessManager(leg) => {
                return self.handle_pm_fact(tx, message, leg).await
            }
            // Handled by the caller before the transaction does anything.
            crate::inbox::FactLeg::Unrecorded(u) => return self.park_unrecorded_fact(message, u),
            crate::inbox::FactLeg::Record(record) => record,
        };
        // The fact widened to a `DomainEvent`, for the effects that are EVENT-shaped rather than
        // lane-shaped: the chained PM-addressed copy routes on the event, not on the recording
        // lane. NOT the recording path — the recorders below take the leg's own payload, which for
        // the money lane is its typed `PaymentFactInbox` (PR #783 review B1: widening on the way IN
        // is what left five declared Payment facts unrecordable).
        let event = record.to_domain_event();
        let actor = Actor {
            // The external system principal (deterministic per source — mirrors the enqueue side).
            user_id: message.user_id.unwrap_or_else(uuid::Uuid::nil),
            user_type: message.user_type.clone(),
            domain_id: None,
            correlation_id: message.correlation_id,
            // The causality link: the appended fact's cause is the mailbox row that carried it.
            cause_id: Some(message.message_id),
        };
        let activation = DeliveryActivation::for_message(&self.activations, message);
        let base_store: Arc<dyn EventStore> = match &activation {
            Some(a) => a.store(self.deps.store.clone()),
            None => self.deps.store.clone(),
        };
        let staging = Arc::new(StagingEventStore::new(base_store));
        let store: Arc<dyn EventStore> = staging.clone();

        // THE EFFECT. `fact_route` already decided WHICH recorder owns the append; this match is
        // over that closed, payload-free set, so it can carry no routing decision of its own and
        // there is nothing here for a future declared fact to fall through.
        //
        // ONE ARM, ONE STREAM, ONE TRANSACTION (vernon): every recorder below appends to exactly
        // one aggregate's own stream, and none of them may reach a second.
        use crate::inbox::RecordLeg;
        let outcome = match record {
            // THE MONEY PATH. The leg carries the lane's TYPED fact, so the stream lookup is
            // `payments::intent_of_fact` — total over `PaymentFactInbox`. This call is what makes
            // the E0004 guarantee load-bearing: before PR #783's review it went to the untyped
            // `record_inbound_payment_event`, whose lookup covered five of the ten declared facts,
            // and `RefundOpened` (the sole feeder of `View_PendingRefunds`) aborted instead of
            // recording.
            RecordLeg::Payment(fact) => {
                application::payments::record_inbound_payment_fact(store.as_ref(), fact, &actor)
                    .await
            }
            RecordLeg::Delivery(e) => {
                application::deliveries::record_inbound_delivery_event(store.as_ref(), e, &actor)
                    .await
            }
            RecordLeg::RestaurantRegistration(e) => {
                application::commands::record_inbound_restaurant_registration(
                    store.as_ref(),
                    e,
                    &actor,
                )
                .await
            }
            // The reminders pilot (ADR-20260731-153000): the promoted OrderExpired MESSAGE
            // records the expiry on its order's stream — Recorded / AlreadyRecorded / NoChange,
            // never a rejection (a retention deadline's passage cannot be refused).
            RecordLeg::Order(e) => {
                application::commands::record_inbound_order_event(store.as_ref(), e, &actor).await
            }
            // #167: the Order BIRTH as a mailbox delivery — the spec's "Birth: PlaceOrderProcess
            // delivers OrderPlaced" receive. Recorded idempotently; its `schedules:` start the
            // acceptance clock, and a redelivered (AlreadyRecorded) birth RE-APPLIES them —
            // safe by design: `reschedule: keep` means the first deadline wins.
            RecordLeg::OrderPlaced(e) => {
                application::commands::record_inbound_order_placed(store.as_ref(), e, &actor).await
            }
            // #167: the promoted acceptance deadline — its own route because its outcome is
            // richer than RecordOutcome (the shadow WouldCancel arm is the flip ADR's evidence)
            // and because of the young+vernon fence below: schedules apply on the
            // Recorded/Cancelled arm ONLY.
            RecordLeg::OrderAcceptanceTimeout(e) => {
                return self
                    .handle_acceptance_timeout(
                        tx,
                        message,
                        e,
                        staging.clone(),
                        activation,
                        &actor,
                    )
                    .await;
            }
            // #639 part C step 6-iv round 2 (#902): the TTL reminder records the expiry on its
            // invitation's own stream — Recorded / NoChange (already terminal, or the stream is
            // gone), never a rejection (a deadline's passage cannot be refused), the SAME shape as
            // `RecordLeg::Order` above.
            RecordLeg::RestaurantInvitation(e) => {
                application::commands::record_inbound_restaurant_invitation_expiry(
                    store.as_ref(),
                    e,
                    &actor,
                )
                .await
            }
        };
        let delivery = match outcome {
            Ok(RecordOutcome::Recorded) | Ok(RecordOutcome::Updated) => {
                let staged = staging.take_staged();
                // Same pre-flush freshness guard as the COMMAND route.
                if let Some(a) = &activation {
                    a.guard_freshness_in_tx(tx).await?;
                }
                match flush_staged_in_tx(tx, &staged).await {
                    // Same post-commit subscription fan-out as the COMMAND route: an inbound
                    // PaymentCaptured must reach `paymentStatusChanged` exactly like a
                    // command-emitted fact — the retired drain published through
                    // PgEventStore::with_bus, and the checkout screen's push depends on it.
                    Ok(()) => {
                        super::record_order_birth_lag(
                            message,
                            &staged,
                            self.deps.route_gates.enabled(
                                application::generated::process_managers::Route::OrderPlacedToOrder,
                            ),
                        );
                        // Recorded facts may declare `schedules:` too (same third-effect rule).
                        super::apply_schedules_in_tx(tx, message, &self.reminder_windows)
                            .await?;
                        // B2: the recorded Stripe fact's PM-addressed copy rides THIS
                        // transaction — atomic with the record, nudged post-commit. UNCONDITIONAL
                        // since #242 Runtime D: the gate that could turn it off is gone, and the
                        // saga runner carries no Stripe-fact triggers to race it.
                        let chained =
                            pm_delivery::chain_pm_copy_in_tx(&self.deps, tx, message, &event)
                                .await?;
                        let promote = DeliveryActivation::promote_after_commit(
                            &self.activations,
                            activation.as_ref(),
                            &staged,
                        );
                        self.fanout_delivery(&staged, chained, promote)
                    }
                    // Version clash at flush: someone appended between load and commit. That
                    // someone is NOT necessarily a redelivery of this fact — a concurrent PM-lane
                    // delivery writes the same Payment streams — so a terminal DUPLICATE here
                    // could drop a fact that never reached the log. ABORT for retry: the
                    // redelivery re-runs the fold-based dedupe against the moved stream and lands
                    // Duplicate only if the fact is genuinely in it.
                    Err(e) if is_version_conflict(&e) => {
                        if let Some(a) = &activation {
                            a.invalidate_scoped();
                        }
                        return Err(sqlx::Error::Protocol(e.to_string()));
                    }
                    Err(DomainError::Repository(detail)) => {
                        return Err(sqlx::Error::Protocol(detail));
                    }
                    Err(e) => Delivery::of(HandlerVerdict::Failed(
                        serde_json::json!({ "code": "Internal", "context": { "detail": e.to_string() } }),
                    )),
                }
            }
            // The no-append verdicts are exactly where a stale hold has no UNIQUE race to lose:
            // an Ignored/Duplicate decided from a held fold must re-assert freshness or a fact
            // could be durably absorbed against state that no longer exists.
            Ok(RecordOutcome::NoChange) => {
                if let Some(a) = &activation {
                    a.guard_freshness_in_tx(tx).await?;
                }
                Delivery::of(HandlerVerdict::Ignored)
            }
            Ok(RecordOutcome::AlreadyRecorded) => {
                if let Some(a) = &activation {
                    a.guard_freshness_in_tx(tx).await?;
                }
                // A redelivered, duplicate-absorbed fact RE-DECLARES its `schedules:` (#167,
                // vernon's design note): under `reschedule: keep` the re-declaration is the
                // in-tx ON CONFLICT no-op — the FIRST deadline wins — and a birth redelivered
                // across a deploy that introduced its reminder still gets a clock. A no-op for
                // every route whose receive declares no schedules. (The NoChange arm below
                // deliberately does NOT: an erased/birthless stream must not get a clock.)
                super::apply_schedules_in_tx(tx, message, &self.reminder_windows).await?;
                Delivery::of(HandlerVerdict::Duplicate)
            }
            // A conflict surfaced by the recorder itself: the stream moved under it — retry, same
            // reasoning as the flush-time clash above.
            Err(e) if is_version_conflict(&e) => {
                if let Some(a) = &activation {
                    a.invalidate_scoped();
                }
                return Err(sqlx::Error::Protocol(e.to_string()));
            }
            // Transient infrastructure failure while loading/folding the stream: ABORT for retry.
            // A terminal FAILED would be absorbed by the enqueue-side pk dedupe when the provider
            // redelivers, permanently losing the payment/delivery fact (PR #270 review C3).
            Err(DomainError::Repository(detail)) => {
                return Err(sqlx::Error::Protocol(detail));
            }
            Err(e) => {
                if let Some(a) = &activation {
                    a.guard_freshness_in_tx(tx).await?;
                }
                Delivery::of(HandlerVerdict::Failed(
                    serde_json::json!({ "code": "Internal", "context": { "detail": e.to_string() } }),
                ))
            }
        };
        Ok(delivery)
    }

    /// The #167 acceptance-timeout delivery (kind MESSAGE, the promoted deadline). Its own route
    /// because [`application::commands::AcceptanceTimeoutOutcome`] is deliberately richer than
    /// `RecordOutcome` — the shadow `WouldCancel` decision is the ENFORCE_ACCEPTANCE_TIMEOUT flip
    /// ADR's whole evidence set — and because of THE FENCE (PR #586 mob checkpoint, young+vernon
    /// binding): `apply_schedules_in_tx` is verdict-blind and this receive declares
    /// `schedules: OrderExpired`, so schedules apply on the **Recorded/Cancelled arm ONLY**
    /// (mirroring the record route's shape). A shadow `WouldCancel`→Ignored delivery must NEVER
    /// arm the GDPR deletion clock on a still-PLACED order.
    ///
    /// The OTLP shadow evidence (`reminder.promote`, specs/observability.yaml
    /// `acceptance-timeout`) is emitted HERE at the mailbox layer — the pure handler only
    /// decides (SDK-free rule; the `service_window_verdict` precedent).
    async fn handle_acceptance_timeout(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        message: &InboundMessage,
        event: domain::generated::events::DomainEvent,
        staging: Arc<StagingEventStore>,
        activation: Option<DeliveryActivation>,
        actor: &Actor,
    ) -> Result<Delivery, sqlx::Error> {
        use application::commands::AcceptanceTimeoutOutcome as O;
        use domain::generated::events::DomainEvent as E;
        use tracing::Instrument as _;

        let enforce = self.deps.enforce_acceptance_timeout;
        let span = telemetry::spans::reminder_promote(&message.message_type, !enforce);
        // The honest promotion slop: delivery time against the row's declared due time.
        // Absent on a redelivery under a fresh (unscheduled) identity.
        if let Some(due) = message.scheduled_at {
            let delay = (chrono::Utc::now() - due).num_milliseconds();
            telemetry::spans::record_reminder_due(&span, &due.to_rfc3339(), delay);
        }
        if let E::OrderAcceptanceTimedOut(t) = &event {
            telemetry::spans::record_order_id(&span, &t.order_id.0.to_string());
        }

        let store: Arc<dyn EventStore> = staging.clone();
        let outcome = application::commands::record_order_acceptance_timeout(
            store.as_ref(),
            event,
            enforce,
            actor,
        )
        .instrument(span.clone())
        .await;

        let delivery = match outcome {
            // Gate ON, still PLACED: the append is real — the ONLY arm that applies schedules.
            Ok(O::Cancelled) => {
                telemetry::spans::record_would_cancel(&span, true);
                let staged = staging.take_staged();
                if let Some(a) = &activation {
                    a.guard_freshness_in_tx(tx).await?;
                }
                match flush_staged_in_tx(tx, &staged).await {
                    Ok(()) => {
                        // THE FENCE: `schedules:` (the OrderExpired GDPR clock — a timed-out
                        // order is terminal and must still be erased on schedule) rides the
                        // SAME transaction as the recorded cancellation, and ONLY here.
                        super::apply_schedules_in_tx(tx, message, &self.reminder_windows)
                            .await?;
                        let promote = DeliveryActivation::promote_after_commit(
                            &self.activations,
                            activation.as_ref(),
                            &staged,
                        );
                        self.fanout_delivery(&staged, None, promote)
                    }
                    Err(e) if is_version_conflict(&e) => {
                        if let Some(a) = &activation {
                            a.invalidate_scoped();
                        }
                        return Err(sqlx::Error::Protocol(e.to_string()));
                    }
                    Err(DomainError::Repository(detail)) => {
                        return Err(sqlx::Error::Protocol(detail));
                    }
                    Err(e) => Delivery::of(HandlerVerdict::Failed(
                        serde_json::json!({ "code": "Internal", "context": { "detail": e.to_string() } }),
                    )),
                }
            }
            // Gate OFF (shadow), still PLACED: the IDENTICAL guard decided it would cancel and
            // only the append was inert. Ignored — record semantics, never Rejected — and the
            // occurrence is spent forever (flipping ON is prospective only). NO schedules: a
            // shadow delivery must never arm the GDPR clock on a still-PLACED order.
            Ok(O::WouldCancel) => {
                telemetry::spans::record_would_cancel(&span, true);
                if let Some(a) = &activation {
                    a.guard_freshness_in_tx(tx).await?;
                }
                Delivery::of(HandlerVerdict::Ignored)
            }
            // Acceptance/rejection/cancellation won the race, or the stream is gone: benign
            // no-op under either gate position. No append, no schedules.
            Ok(O::NotPlaced) | Ok(O::NoOrder) => {
                telemetry::spans::record_would_cancel(&span, false);
                if let Some(a) = &activation {
                    a.guard_freshness_in_tx(tx).await?;
                }
                Delivery::of(HandlerVerdict::Ignored)
            }
            // A redelivered deadline: the order already timed out. The fold-based dedupe stays
            // authoritative.
            Ok(O::AlreadyTimedOut) => {
                telemetry::spans::record_would_cancel(&span, false);
                if let Some(a) = &activation {
                    a.guard_freshness_in_tx(tx).await?;
                }
                Delivery::of(HandlerVerdict::Duplicate)
            }
            Err(e) if is_version_conflict(&e) => {
                if let Some(a) = &activation {
                    a.invalidate_scoped();
                }
                return Err(sqlx::Error::Protocol(e.to_string()));
            }
            // Transient infrastructure failure: ABORT for retry — a terminal verdict here would
            // spend the one occurrence this order ever gets.
            Err(DomainError::Repository(detail)) => {
                return Err(sqlx::Error::Protocol(detail));
            }
            Err(e) => {
                if let Some(a) = &activation {
                    a.guard_freshness_in_tx(tx).await?;
                }
                Delivery::of(HandlerVerdict::Failed(
                    serde_json::json!({ "code": "Internal", "context": { "detail": e.to_string() } }),
                ))
            }
        };
        Ok(delivery)
    }

    /// The committed-success Delivery: verdict + the post-commit event-bus fan-out of everything
    /// the flush just made durable (both delivery routes share this — subscriptions must hear
    /// mailbox-written facts exactly as they heard PgEventStore-written ones). `chained` names
    /// the PM lane a B2 hop was enqueued on — its worker is nudged post-commit, never before.
    /// `promote` is the activation promotion/invalidation closure (apply-after-commit) — it runs
    /// FIRST, so a subscription-triggered read-through can never observe a pre-commit held state.
    fn fanout_delivery(
        &self,
        staged: &[application::staging::StagedAppend],
        chained: Option<&'static str>,
        promote: Option<Box<dyn FnOnce() + Send>>,
    ) -> Delivery {
        let envelopes: Vec<AppendedEvent> = match &self.event_bus {
            Some(_) => staged
                .iter()
                .flat_map(|a| {
                    a.events.iter().enumerate().filter_map(|(i, e)| {
                        let tagged = serde_json::to_value(e).ok()?;
                        Some(AppendedEvent {
                            stream_name: a.stream_name.clone(),
                            event_type: tagged.get("eventType")?.as_str()?.to_owned(),
                            correlation_id: a.actor.correlation_id,
                            position: a.expected_version + i as i64 + 1,
                        })
                    })
                })
                .collect(),
            None => Vec::new(),
        };
        let bus = self.event_bus.clone();
        let nudges = self.nudges.clone();
        if bus.is_none() && (chained.is_none() || nudges.is_none()) && promote.is_none() {
            return Delivery::of(HandlerVerdict::Succeeded);
        }
        Delivery::then(HandlerVerdict::Succeeded, move || {
            if let Some(promote) = promote {
                promote();
            }
            if let Some(bus) = bus {
                for envelope in envelopes {
                    bus.publish(envelope);
                }
            }
            if let (Some(nudges), Some(actor_type)) = (nudges, chained) {
                nudges.nudge(actor_type);
            }
        })
    }

    /// One chained PM hop's delivery (B2): dispatch the saga's EVENT leg against staging stores,
    /// then flush events + run rows into the fenced transaction — the mailbox-era home of the
    /// saga runner's PlaceOrderProcess/RefundProcess Stripe-fact dispatch, now durable, fenced
    /// and ordered per order. A typed leg anomaly (`PaymentEventOrphaned`) lands REJECTED on the
    /// row — supervisable, never silently skipped; a benign skip (redelivered outcome, resolved
    /// run) lands IGNORED.
    async fn handle_pm_fact(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        message: &InboundMessage,
        leg: crate::inbox::PmFactLeg,
    ) -> Result<Delivery, sqlx::Error> {
        use application::process_managers::{place_order, refund, Outcome, TriggerEnvelope};
        use crate::inbox::PmFactLeg;

        let staging = Arc::new(StagingEventStore::new(self.deps.store.clone()));
        let payment_staging =
            application::staging::StagingPaymentProcessState::new(self.deps.pm_state.clone());
        let refund_staging =
            application::staging::StagingRefundProcessState::new(self.deps.refund_state.clone());
        // The lane sink for ROUTED `deliver:` steps (#588, ADR-20260816-040239). It is safe to
        // hand over here and nowhere else: THIS is the phase that owns the fenced transaction —
        // the prepare phase (`pm_delivery::prepare`) owns none and re-runs on redelivery, so an
        // enqueue staged there would survive a verdict that never committed.
        //
        // WHICH routes may use it is a separate question, answered per route by `route_gates`
        // below (#797) — not by whether the sink is present at all.
        let lane_sink = Arc::new(application::lanes::StagingLaneSink::new());
        // The trigger envelope: the chained row IS the trigger (its deterministic id doubles as
        // the dedup key the run row records).
        //
        // `laned` vs `unlaned` says whether this phase CAN stage, and since #597 it is the only
        // way to express it: `TriggerEnvelope::lanes` is private, so no phase can attach a sink by
        // a field write. Naming `laned` here is a claim this function can honour — `tx` is in
        // scope, and the flush below rides it. The claim is unconditional, so the constructor
        // choice is too; the per-route decision travels beside it as `route_gates`, and each
        // routed step reads its own (#797). Handing the sink only when ONE route's key was on is
        // what fused every route this delivery hosts onto that key.
        let env = TriggerEnvelope::laned(
            message.message_id,
            message.correlation_id,
            message.received_at,
            lane_sink.clone() as Arc<dyn application::lanes::LaneSink>,
            self.deps.route_gates,
        );
        // TYPED, AND THEREFORE TOTAL (#780). This was a match over `(actor_type, &DomainEvent)`
        // ending in `(actor, _) => Failed("no PM event leg")` — a catch-all on the money path, in a
        // file the router's no-catch-all scan does not read. `PmFactLeg` has exactly the three legs
        // the saga declares, so there is nothing left to fall through and a fourth is an E0004 in
        // the human-owned `fact_route`, where the decision belongs.
        let outcome = match leg {
            PmFactLeg::PlaceOrderOnPaymentAuthorized(e) => {
                place_order::on_payment_authorized(
                    staging.as_ref() as &dyn EventStore,
                    &payment_staging,
                    &e,
                    &env,
                )
                .await
            }
            PmFactLeg::PlaceOrderOnPaymentFailed(e) => {
                place_order::on_payment_failed(&payment_staging, &e, &env).await
            }
            PmFactLeg::RefundOnPaymentRefunded(e) => {
                refund::on_payment_refunded(&refund_staging, &e).await
            }
        };
        match outcome {
            Ok(Outcome::Completed) => {
                let staged = staging.take_staged();
                match flush_staged_in_tx(tx, &staged).await {
                    Ok(()) => {
                        // The ROUTED `deliver:` steps' door rows (#588): same transaction as the
                        // staged appends above and the run row below — both or neither. A
                        // duplicate collides on the primary key and is a SUCCESS, never an error.
                        super::flush_lane_enqueues_in_tx(
                            tx,
                            &super::LaneCause::of_message(message),
                            &lane_sink.take_staged(),
                        )
                            .await
                            .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
                        pm_delivery::flush_pm_rows_in_tx(
                            tx,
                            &payment_staging.take_staged(),
                            &refund_staging.take_staged(),
                        )
                        .await
                        .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
                        super::apply_schedules_in_tx(tx, message, &self.reminder_windows).await?;
                        // Same cross-writer rule as the prepared PM commit: the saga leg's
                        // appends (OrderPlaced on the Order stream) stale-out held copies.
                        let promote = DeliveryActivation::promote_after_commit(
                            &self.activations,
                            None,
                            &staged,
                        );
                        Ok(self.fanout_delivery(&staged, None, promote))
                    }
                    // Lost optimistic-concurrency race: retry the WHOLE leg (the run row's
                    // expect and the aggregates' record-idempotency absorb the replay) — the
                    // runner's exact semantics, now per-lane instead of per-tick.
                    Err(e) if is_version_conflict(&e) => Err(sqlx::Error::Protocol(e.to_string())),
                    Err(DomainError::Repository(detail)) => Err(sqlx::Error::Protocol(detail)),
                    Err(e) => Ok(Delivery::of(HandlerVerdict::Failed(serde_json::json!({
                        "code": "Internal",
                        "context": { "detail": e.to_string() }
                    })))),
                }
            }
            // Benign expected alternative (idempotent re-delivery, failed state.expect): the
            // runner LOGGED and advanced; the mailbox records it as the row's IGNORED verdict.
            Ok(Outcome::Skipped(reason)) => {
                tracing::warn!(
                    actor_type = %message.actor_type,
                    message_type = %message.message_type,
                    %reason,
                    "pm fact delivery skipped"
                );
                Ok(Delivery::of(HandlerVerdict::Ignored))
            }
            Err(e) if is_version_conflict(&e) => Err(sqlx::Error::Protocol(e.to_string())),
            Err(DomainError::Repository(detail)) => Err(sqlx::Error::Protocol(detail)),
            // A typed leg anomaly (PaymentEventOrphaned): REJECTED on the row — the runner
            // surfaced it on /saga and advanced; the mailbox keeps it queryable per message.
            Err(e) => Ok(Delivery::of(verdict_of_error(e))),
        }
    }
}

/// Handler error → terminal verdict — the same discrimination the GraphQL completion applies
/// (a catalogued errors.yaml rejection is REJECTED; everything else is the generic Internal).
///
/// **The verdict itself is unchanged by #623** and that is a fence, not an accident: catalogue
/// membership is what splits REJECTED from FAILED, so a change that moved a code into or out of the
/// catalogue would flip a verdict, an outcome and an alert's meaning at once. What changed is what
/// the row SAYS, and where the prose goes.
///
/// Two destinations, deliberately two different calls:
///
/// - the **journal row** (`inbound_messages.error`, kept for the retention window and served as
///   `Operation.errorCode`/`Operation.message`) gets a BOUNDED attribution and nothing else —
///   [`attribution::context_of`] over a type with no free-text field;
/// - the **log** gets the full diagnostic string, at the severity the class deserves.
///
/// Before this, the non-catalogued arm recorded `{}` (unattributable — #623) while the catalogued
/// arm recorded the provider's message verbatim (a key leak — #625). Those are the same bug seen
/// from two sides, which is why they are fixed in one function.
pub fn verdict_of_error(e: DomainError) -> HandlerVerdict {
    // A structured rejection already carries its declared errors.yaml context: pass it through
    // untouched. It never held provider prose — the leak was only ever in the legacy string form.
    if let DomainError::Rejected { code, context } = &e {
        return HandlerVerdict::Rejected(serde_json::json!({ "code": code, "context": context }));
    }
    if let Some(code) = attribution::catalogued_code(&e) {
        // A CATALOGUED refusal. The `detail` this arm used to carry was the provider's message —
        // the LIVE leak, since no en/fr template interpolates `{detail}` (the complete placeholder
        // set across every errors.yaml fragment is 21 tokens and `detail` is not one), so it was
        // reaching the column and the backups and nothing else. Dropping it changes no
        // customer-facing message.
        //
        // It is replaced by the SAME bounded attribution the FAILED arm records, not by `{}`: a
        // declined card leaving only `PaymentDeclined` behind answers "was it declined?" and not
        // "declined how?", which is a downgrade on the money path (#623 review, `young`).
        // Logged at INFO because a declined card is a business outcome, not a fault — the walk's
        // "nothing at ERROR or WARN" finding is about the arm below, not this one.
        let code = code.to_string();
        let context = attribution::catalogued_context(&e);
        tracing::info!(
            error_code = %code,
            gateway_status = ?context.get("gatewayStatus").and_then(|v| v.as_i64()),
            detail = %attribution::log_detail(&e),
            "command rejected with a catalogued code"
        );
        return HandlerVerdict::Rejected(serde_json::json!({ "code": code, "context": context }));
    }
    // THE #623 DISCARD SITE. A failure nobody declared, on the most consequential command in the
    // product: ERROR is the right severity, and its absence was half the defect.
    let attributed = attribution::attribute(&e);
    let context = attribution::context_of(&attributed);
    tracing::error!(
        seam = %context.get("seam").and_then(|v| v.as_str()).unwrap_or("?"),
        reason = %context.get("reason").and_then(|v| v.as_str()).unwrap_or("?"),
        gateway_status = ?context.get("gatewayStatus").and_then(|v| v.as_i64()),
        detail = %attribution::log_detail(&e),
        "command FAILED with no catalogued code — recorded as Internal"
    );
    HandlerVerdict::Failed(serde_json::json!({ "code": "Internal", "context": context }))
}

/// Post-commit fan-out of COMMAND verdicts onto the operation status bus — the mailbox-era home
/// of what `complete_operation` publishes on the legacy spawn path.
pub struct StatusBusObserver {
    bus: OperationStatusBus,
}

impl StatusBusObserver {
    pub fn new(bus: OperationStatusBus) -> Self {
        Self { bus }
    }
}

impl DeliveryObserver for StatusBusObserver {
    fn committed(&self, message: &InboundMessage, verdict: &HandlerVerdict) {
        if message.kind != "COMMAND" {
            return;
        }
        // `command_completion_ms{status}` — acceptance insert → committed terminal status, the
        // mailbox-era emission of the command-acceptance contract's histogram (#272 D1: the
        // metric must not go dark when a command leaves the journal+spawn path; this also lights
        // it back up for every Runtime-C-flipped command). Measured from the row's `received_at`
        // — the durable acceptance instant, same contract meaning as the journal insert.
        let status_label = match verdict {
            HandlerVerdict::Succeeded | HandlerVerdict::Ignored | HandlerVerdict::Duplicate => {
                "SUCCEEDED"
            }
            HandlerVerdict::Rejected(_) => "REJECTED",
            HandlerVerdict::Failed(_) => "FAILED",
        };
        let elapsed_ms = (chrono::Utc::now() - message.received_at)
            .num_milliseconds()
            .max(0) as f64;
        telemetry::meters::acceptance::completed(status_label, elapsed_ms);
        // The bus speaks the mailbox-native enum (#303) and carries the HONEST verdict —
        // IGNORED/DUPLICATE stay themselves on the wire (the API mapping folds them into
        // SUCCEEDED at the edge, where that flattening is a presentation choice, not a fact).
        use domain::generated::scalars::InboundMessageStatus as M;
        let status = match verdict {
            HandlerVerdict::Succeeded => M::SUCCEEDED,
            HandlerVerdict::Ignored => M::IGNORED,
            HandlerVerdict::Duplicate => M::DUPLICATE,
            HandlerVerdict::Rejected(_) => M::REJECTED,
            HandlerVerdict::Failed(_) => M::FAILED,
        };
        let error_code = verdict
            .error()
            .and_then(|e| e.get("code"))
            .and_then(|c| c.as_str())
            .map(str::to_owned);
        let message_text = match (&error_code, verdict.error().and_then(|e| e.get("context"))) {
            (Some(code), Some(context)) => domain::generated::errors::message_en(code, context),
            _ => None,
        };
        self.bus.publish(OperationUpdate {
            message_id: message.message_id,
            correlation_id: message.correlation_id,
            status,
            error_code,
            message: message_text,
        });
    }

    /// The poison flip (PROP-20260802-223522 D4): no handler verdict exists — the completion
    /// transaction kept failing — so this seam carries the operator event: the contract counter
    /// (`mailbox_poison_failed_total{actor_type}`) and, for COMMAND rows, the terminal FAILED on
    /// the status bus so a waiting client's `operationStatus` resolves instead of pending until
    /// its poll gives up.
    fn poisoned(&self, message: &InboundMessage, _error: &str) {
        telemetry::meters::mailbox::poison_failed(&message.actor_type);
        if message.kind != "COMMAND" {
            return;
        }
        use domain::generated::scalars::InboundMessageStatus as M;
        self.bus.publish(OperationUpdate {
            message_id: message.message_id,
            correlation_id: message.correlation_id,
            status: M::FAILED,
            error_code: Some("DeliveryInfrastructureError".to_owned()),
            message: None,
        });
    }
}

/// **T4 (#780) — THE DOOR'S VERDICT TABLE.** Five rows, on the RETURN SHAPE, never on message text.
///
/// It is a fast pure table and not five database tests because the property under test is a
/// decision, and the decision was made transaction-free precisely so it could be tested this way
/// ("make the change easy first", beck). What still needs a database is the EFFECT, and that lives
/// in `crates/infrastructure/tests/main/fact_delivery.rs`.
///
/// **Row (a) is the risk pin.** An undeclared `message_type` on a KNOWN lane must PARK, never go
/// terminal: during a rolling deploy an old consumer legitimately meets a type a newer producer
/// already emits, and terminal-failing it buries a paid order. It is one `is_transient` arm away
/// from silently becoming row (c), and nothing else in the suite would notice.
#[cfg(test)]
mod verdict_table {
    use super::*;
    use application::generated::inboxes::ActorInbox;

    /// A well-formed staged `DomainEvent` envelope for a Payment fact.
    fn staged_payment_captured() -> serde_json::Value {
        serde_json::json!({
            "eventType": "PaymentCaptured",
            "payload": {
                "paymentIntentId": "pi_verdict_table",
                "orderId": null,
                "restaurantId": "00000000-0000-0000-0000-0000000000a1",
                "amount": { "amountCents": 1960, "currency": "EUR" }
            }
        })
    }

    fn posture_of(actor_type: &str, message_type: &str, payload: &serde_json::Value) -> DoorPosture {
        match ActorInbox::parse(actor_type, message_type, payload) {
            Ok(_) => panic!("this row must NOT parse: {actor_type}/{message_type}"),
            Err(e) => parse_posture(&e),
        }
    }

    /// (a) A DECLARED lane, an UNDECLARED message type. The rolling-deploy case.
    #[test]
    fn an_undeclared_message_type_on_a_known_lane_parks() {
        assert_eq!(
            posture_of("Payment", "PaymentTeleported", &staged_payment_captured()),
            DoorPosture::Park,
            "an undeclared type must be RETRIED and then parked on the poison queue -- a build on \
             the other side of a rolling deploy can route it, and terminal-failing it buries a \
             paid order"
        );
    }

    /// (b) An unknown `actor_type`. Same posture, same reason.
    #[test]
    fn an_unknown_actor_type_parks() {
        assert_eq!(
            posture_of("Teleporter", "PaymentCaptured", &staged_payment_captured()),
            DoorPosture::Park
        );
    }

    /// (c) A DECLARED message whose payload does not deserialize. Deterministic, so terminal.
    #[test]
    fn a_declared_message_with_a_malformed_payload_is_terminal() {
        let malformed = serde_json::json!({ "eventType": "PaymentCaptured", "payload": 7 });
        assert_eq!(posture_of("Payment", "PaymentCaptured", &malformed), DoorPosture::Terminal);
    }

    /// (d) The row's `message_type` disagrees with the staged `eventType` tag. Nothing checked this
    /// before #771: a row could carry `message_type: "OrderPlaced"` with an `OrderRejected` body
    /// and the generic record route would have appended the body under the wrong name.
    #[test]
    fn a_message_type_disagreeing_with_the_payload_tag_is_terminal() {
        let mismatched = serde_json::json!({
            "eventType": "PaymentReleased",
            "payload": { "paymentIntentId": "pi_verdict_table" }
        });
        assert_eq!(posture_of("Payment", "PaymentCaptured", &mismatched), DoorPosture::Terminal);
    }

    /// (e) A DECLARED fact with no record route: PARK, and the class says which fact.
    ///
    /// Asserted through `fact_route` rather than through the parse edge, because this row is not a
    /// parse failure at all -- it is a fact that parses perfectly and has nowhere to go, which is
    /// the whole subject of #780.
    #[test]
    fn a_declared_fact_with_no_record_route_parks() {
        use crate::inbox::{fact_route, FactLegClass, UnrecordedFact};
        let staged = serde_json::json!({
            "eventType": "DeliveryRequested",
            "payload": {
                "deliveryJobId": "00000000-0000-0000-0000-0000000000d1",
                "orderId": "00000000-0000-0000-0000-0000000000d2",
                "restaurantId": "00000000-0000-0000-0000-0000000000d3",
                "pickup": { "line1": "1 rue de la Paix", "city": "Tours", "postalCode": "37000", "country": "FR" },
                "dropoff": { "line1": "2 rue Nationale", "city": "Tours", "postalCode": "37000", "country": "FR" }
            }
        });
        let inbox = ActorInbox::parse("DeliveryJob", "DeliveryRequested", &staged)
            .expect("a DECLARED fact must parse -- it is declared, it just has no route");
        let fact = inbox.into_fact().expect("a fact row projects onto the fact half");
        assert_eq!(
            fact_route(fact).class(),
            FactLegClass::Unrecorded(UnrecordedFact {
                actor_type: "DeliveryJob",
                message_type: "DeliveryRequested",
            }),
            "a declared fact with no fold rule must PARK, naming itself -- never a terminal verdict, \
             because a fact already happened and cannot be refused"
        );
    }

    /// The control that stops the table passing for the wrong reason: a fact that DOES have a route
    /// must not be classified as parked, or every row above would hold vacuously.
    #[test]
    fn a_routed_fact_is_not_parked() {
        use crate::inbox::{fact_route, FactLegClass, FactRecorder};
        let inbox = ActorInbox::parse("Payment", "PaymentCaptured", &staged_payment_captured())
            .expect("PaymentCaptured is declared on the Payment lane");
        let fact = inbox.into_fact().expect("a fact row projects onto the fact half");
        assert_eq!(fact_route(fact).class(), FactLegClass::Record(FactRecorder::Payment));
    }
}
