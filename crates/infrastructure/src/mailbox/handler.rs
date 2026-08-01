//! The mailbox delivery glue: [`MailboxCommandHandler`] implements the runtime's
//! [`actor_runtime::MessageHandler`] over the GENERATED command router — deserialize, resolve the
//! acting principal (the SAME #235 by_auth_ref bridge the GraphQL edge applies), run the handler
//! against a per-delivery [`StagingEventStore`], flush the staged events into the completion
//! transaction, and map the outcome onto the row's terminal verdict. [`StatusBusObserver`]
//! publishes each COMMITTED command verdict on the [`crate::OperationStatusBus`] — after the
//! commit, never before, so `operationStatusChanged` subscribers only ever hear durable facts.

use std::sync::Arc;

use actor_runtime::{Delivery, DeliveryObserver, HandlerVerdict, InboundMessage, MessageHandler};
use application::ports::{is_version_conflict, Actor, EventStore};
use application::staging::StagingEventStore;
use domain::shared::errors::DomainError;
use sqlx::{Postgres, Transaction};

use application::payments::RecordOutcome;

use crate::generated::command_router::{dispatch_command, CommandDeps};
use crate::persistence::event_bus::{AppendedEvent, EventBus};
use crate::persistence::status_bus::{OperationStatusBus, OperationUpdate};

use super::flush_staged_in_tx;

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
    /// Reminder window keys → DAYS (`Config::reminder_windows()`): what `apply_schedules_in_tx`
    /// resolves a `schedules:` declaration's `after` against. Empty = no windows wired; a
    /// delivery that then needs one aborts for retry (wiring bug, never a terminal verdict).
    reminder_windows: std::collections::HashMap<&'static str, i64>,
}

impl MailboxCommandHandler {
    pub fn new(deps: CommandDeps) -> Self {
        Self { deps, event_bus: None, reminder_windows: Default::default() }
    }

    pub fn with_event_bus(mut self, bus: EventBus) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// Wire the configured reminder windows (composition root: `Config::reminder_windows()`).
    pub fn with_reminder_windows(
        mut self,
        windows: std::collections::HashMap<&'static str, i64>,
    ) -> Self {
        self.reminder_windows = windows;
        self
    }
}

#[async_trait::async_trait]
impl MessageHandler for MailboxCommandHandler {
    async fn handle(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        message: &InboundMessage,
    ) -> Result<Delivery, sqlx::Error> {
        if message.kind == "EVENT" || message.kind == "MESSAGE" {
            // Both kinds RECORD facts: EVENT carries an adapted inbound business fact, MESSAGE a
            // promoted reminder's payload fact (ADR-20260731-153000 §1a) — same record semantics,
            // same route.
            return self.handle_recorded_fact(tx, message).await;
        }
        if message.kind != "COMMAND" {
            return Ok(Delivery::of(HandlerVerdict::Failed(serde_json::json!({
                "code": "Internal",
                "context": { "detail": format!("unroutable mailbox kind '{}'", message.kind) }
            }))));
        }

        // The acting principal, envelope → Actor (ADR-0041) with the #235 domain-identity bridge:
        // a CUSTOMER's auth subject resolves to its CustomerId — the value `requires.acting`
        // compares against folded participants. Same logic as the GraphQL edge; other roles stay
        // None until their bridges land (#144).
        let domain_id = if message.user_type == "CUSTOMER" {
            match message.user_id {
                Some(uid) => match self
                    .deps
                    .customers
                    .by_auth_ref(domain::generated::scalars::ExternalReference(uid.to_string()))
                    .await
                {
                    Ok(customer) => customer.map(|c| c.customer_id.0),
                    // An infrastructure failure resolving the principal must ABORT the delivery
                    // (row stays RECEIVED, redelivery retries) — swallowing it into `None` would
                    // durably record a legitimate customer's command as REJECTED NotAParticipant,
                    // a wrong-class terminal verdict for a transient DB blip.
                    Err(e) => return Err(sqlx::Error::Protocol(e.to_string())),
                },
                None => None,
            }
        } else {
            None
        };
        let actor = Actor {
            user_id: message.user_id.unwrap_or_else(uuid::Uuid::nil),
            user_type: message.user_type.clone(),
            domain_id,
            correlation_id: message.correlation_id,
            cause_id: Some(message.message_id),
        };

        // Stage-don't-write: the handler runs unchanged over a buffering store; its events only
        // become true when the runtime commits the fenced transaction this flush joins.
        let staging = Arc::new(StagingEventStore::new(self.deps.store.clone()));
        let mut deps = self.deps.clone();
        deps.store = staging.clone() as Arc<dyn EventStore>;

        let outcome = dispatch_command(
            &deps,
            &message.message_type,
            &message.payload,
            &actor,
            message.session_id,
        )
        .await;

        let delivery = match outcome {
            None => Delivery::of(HandlerVerdict::Failed(serde_json::json!({
                "code": "Internal",
                "context": { "detail": format!("unroutable command type '{}'", message.message_type) }
            }))),
            Some(Ok(())) => {
                let staged = staging.take_staged();
                match flush_staged_in_tx(tx, &staged).await {
                    Ok(()) => {
                        // The handler's third observable effect (ADR-20260731-214500 §2): a
                        // successful delivery (re)declares its `schedules:` reminders in the
                        // SAME transaction — commit and clock start together or not at all.
                        super::apply_schedules_in_tx(tx, message, &self.reminder_windows)
                            .await?;
                        self.fanout_delivery(&staged)
                    }
                    // A version clash at commit time: a concurrent writer (a legacy-path PM leg,
                    // another lane) moved the stream between the handler's load and this flush.
                    // ABORT the delivery — the row stays RECEIVED and the retry re-runs the
                    // handler against the moved stream. Contention is transient by construction
                    // (each retry reloads), so retry-in-place converges; a terminal FAILED here
                    // would make a peak-time clash cost the client a manual resubmit.
                    Err(e) if is_version_conflict(&e) => {
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
            Some(Err(DomainError::Repository(detail))) => {
                return Err(sqlx::Error::Protocol(detail));
            }
            Some(Err(e)) => Delivery::of(verdict_of_error(e)),
        };
        Ok(delivery)
    }
}

impl MailboxCommandHandler {
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
    ) -> Result<Delivery, sqlx::Error> {
        let event: domain::generated::events::DomainEvent =
            match serde_json::from_value(message.payload.clone()) {
                Ok(e) => e,
                Err(e) => {
                    return Ok(Delivery::of(HandlerVerdict::Failed(serde_json::json!({
                        "code": "Internal",
                        "context": { "detail": format!("unparsable staged DomainEvent: {e}") }
                    }))))
                }
            };
        let actor = Actor {
            // The external system principal (deterministic per source — mirrors the enqueue side).
            user_id: message.user_id.unwrap_or_else(uuid::Uuid::nil),
            user_type: message.user_type.clone(),
            domain_id: None,
            correlation_id: message.correlation_id,
            // The causality link: the appended fact's cause is the mailbox row that carried it.
            cause_id: Some(message.message_id),
        };
        let staging = Arc::new(StagingEventStore::new(self.deps.store.clone()));
        let store: Arc<dyn EventStore> = staging.clone();

        use domain::generated::events::DomainEvent as E;
        let outcome = match &event {
            E::PaymentCaptured(_) | E::PaymentFailed(_) | E::PaymentRefunded(_) => {
                application::payments::record_inbound_payment_event(store.as_ref(), event, &actor)
                    .await
            }
            E::DeliveryAcceptedByPartner(_)
            | E::DeliveryRejectedByPartner(_)
            | E::DeliveryStatusUpdated(_) => {
                application::deliveries::record_inbound_delivery_event(store.as_ref(), event, &actor)
                    .await
            }
            E::RestaurantRegistered(_) => {
                application::commands::record_inbound_restaurant_registration(
                    store.as_ref(),
                    event,
                    &actor,
                )
                .await
            }
            // The reminders pilot (ADR-20260731-153000): the promoted OrderExpired MESSAGE
            // records the expiry on its order's stream — Recorded / AlreadyRecorded / NoChange,
            // never a rejection (a retention deadline's passage cannot be refused).
            E::OrderExpired(_) => {
                application::commands::record_inbound_order_event(store.as_ref(), event, &actor)
                    .await
            }
            _ => {
                return Ok(Delivery::of(HandlerVerdict::Failed(serde_json::json!({
                    "code": "Internal",
                    "context": { "detail": format!("no delivery route for inbound fact type '{}'", message.message_type) }
                }))))
            }
        };
        let delivery = match outcome {
            Ok(RecordOutcome::Recorded) | Ok(RecordOutcome::Updated) => {
                let staged = staging.take_staged();
                match flush_staged_in_tx(tx, &staged).await {
                    // Same post-commit subscription fan-out as the COMMAND route: an inbound
                    // PaymentCaptured must reach `paymentStatusChanged` exactly like a
                    // command-emitted fact — the retired drain published through
                    // PgEventStore::with_bus, and the checkout screen's push depends on it.
                    Ok(()) => {
                        // Recorded facts may declare `schedules:` too (same third-effect rule).
                        super::apply_schedules_in_tx(tx, message, &self.reminder_windows)
                            .await?;
                        self.fanout_delivery(&staged)
                    }
                    // Version clash at flush: someone appended between load and commit. That
                    // someone is NOT necessarily a redelivery of this fact — the legacy-path PM
                    // legs write the same Payment streams until Runtime D — so a terminal
                    // DUPLICATE here could drop a fact that never reached the log. ABORT for
                    // retry: the redelivery re-runs the fold-based dedupe against the moved
                    // stream and lands Duplicate only if the fact is genuinely in it.
                    Err(e) if is_version_conflict(&e) => {
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
            Ok(RecordOutcome::NoChange) => Delivery::of(HandlerVerdict::Ignored),
            Ok(RecordOutcome::AlreadyRecorded) => Delivery::of(HandlerVerdict::Duplicate),
            // A conflict surfaced by the recorder itself: the stream moved under it — retry, same
            // reasoning as the flush-time clash above.
            Err(e) if is_version_conflict(&e) => {
                return Err(sqlx::Error::Protocol(e.to_string()));
            }
            // Transient infrastructure failure while loading/folding the stream: ABORT for retry.
            // A terminal FAILED would be absorbed by the enqueue-side pk dedupe when the provider
            // redelivers, permanently losing the payment/delivery fact (PR #270 review C3).
            Err(DomainError::Repository(detail)) => {
                return Err(sqlx::Error::Protocol(detail));
            }
            Err(e) => Delivery::of(HandlerVerdict::Failed(
                serde_json::json!({ "code": "Internal", "context": { "detail": e.to_string() } }),
            )),
        };
        Ok(delivery)
    }

    /// The committed-success Delivery: verdict + the post-commit event-bus fan-out of everything
    /// the flush just made durable (both delivery routes share this — subscriptions must hear
    /// mailbox-written facts exactly as they heard PgEventStore-written ones).
    fn fanout_delivery(&self, staged: &[application::staging::StagedAppend]) -> Delivery {
        match &self.event_bus {
            Some(bus) => {
                let bus = bus.clone();
                let envelopes: Vec<AppendedEvent> = staged
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
                    .collect();
                Delivery::then(HandlerVerdict::Succeeded, move || {
                    for envelope in envelopes {
                        bus.publish(envelope);
                    }
                })
            }
            None => Delivery::of(HandlerVerdict::Succeeded),
        }
    }
}

/// Handler error → terminal verdict — the same discrimination the GraphQL completion applies
/// (a catalogued errors.yaml rejection is REJECTED; everything else is the generic Internal).
fn verdict_of_error(e: DomainError) -> HandlerVerdict {
    match e {
        DomainError::Rejected { code, context } => {
            HandlerVerdict::Rejected(serde_json::json!({ "code": code, "context": context }))
        }
        DomainError::Invariant(msg) => {
            let code = msg.split(':').next().map(str::trim).unwrap_or("");
            if domain::generated::errors::find(code).is_some() {
                HandlerVerdict::Rejected(
                    serde_json::json!({ "code": code, "context": { "detail": msg } }),
                )
            } else {
                HandlerVerdict::Failed(
                    serde_json::json!({ "code": "Internal", "context": {} }),
                )
            }
        }
        DomainError::Repository(_) => {
            HandlerVerdict::Failed(serde_json::json!({ "code": "Internal", "context": {} }))
        }
    }
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
        use domain::generated::scalars::CommandJournalStatus as J;
        let status = match verdict {
            HandlerVerdict::Succeeded | HandlerVerdict::Ignored | HandlerVerdict::Duplicate => {
                J::SUCCEEDED
            }
            HandlerVerdict::Rejected(_) => J::REJECTED,
            HandlerVerdict::Failed(_) => J::FAILED,
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
}
