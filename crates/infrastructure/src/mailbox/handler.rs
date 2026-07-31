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

use crate::generated::command_router::{dispatch_command, CommandDeps};
use crate::persistence::event_bus::{AppendedEvent, EventBus};
use crate::persistence::status_bus::{OperationStatusBus, OperationUpdate};

use super::flush_staged_in_tx;

/// The COMMAND-kind delivery glue. EVENT/MESSAGE kinds are later C/D slices (the adapter inbox
/// flip and the reminders promotion) — until then they complete FAILED with a routing error so a
/// misrouted row is loud, never silently swallowed.
pub struct MailboxCommandHandler {
    deps: CommandDeps,
    /// When present, each committed delivery publishes its appended events on the in-process bus
    /// (the GraphQL domain-fact subscriptions) — POST-COMMIT, via the runtime's Delivery hook,
    /// exactly where the pool-backed PgEventStore publishes.
    event_bus: Option<EventBus>,
}

impl MailboxCommandHandler {
    pub fn new(deps: CommandDeps) -> Self {
        Self { deps, event_bus: None }
    }

    pub fn with_event_bus(mut self, bus: EventBus) -> Self {
        self.event_bus = Some(bus);
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
        if message.kind != "COMMAND" {
            return Ok(Delivery::of(HandlerVerdict::Failed(serde_json::json!({
                "code": "Internal",
                "context": { "detail": format!("kind {} not routed yet (#242 later slices)", message.kind) }
            }))));
        }

        // The acting principal, envelope → Actor (ADR-0041) with the #235 domain-identity bridge:
        // a CUSTOMER's auth subject resolves to its CustomerId — the value `requires.acting`
        // compares against folded participants. Same logic as the GraphQL edge; other roles stay
        // None until their bridges land (#144).
        let domain_id = if message.user_type == "CUSTOMER" {
            match message.user_id {
                Some(uid) => self
                    .deps
                    .customers
                    .by_auth_ref(domain::generated::scalars::ExternalReference(uid.to_string()))
                    .await
                    .ok()
                    .flatten()
                    .map(|c| c.customer_id.0),
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
                    Ok(()) => match &self.event_bus {
                        // The domain-fact subscription fan-out — post-commit via the Delivery
                        // hook, mirroring PgEventStore's publish-after-commit.
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
                    },
                    // A version clash at commit time: a concurrent writer moved the stream between
                    // the handler's load and this flush. FAILED (not REJECTED): it is contention,
                    // not a business verdict — a resubmission under a fresh id retries cleanly.
                    Err(e) if is_version_conflict(&e) => {
                        Delivery::of(HandlerVerdict::Failed(serde_json::json!({
                            "code": "Internal",
                            "context": { "detail": e.to_string() }
                        })))
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
            Some(Err(e)) => Delivery::of(verdict_of_error(e)),
        };
        Ok(delivery)
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
