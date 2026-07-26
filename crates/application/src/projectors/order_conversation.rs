//! Hand-written `OrderConversationCompute` (ADR-0040; #131, epic #129). The mechanical columns
//! (`order_id`/`restaurant_id`/`customer_chat_enabled`/`opened_at`/`escalation_reason`) are set by the
//! generated `project_order_conversation` dispatch; this fold owns the computed / accumulate columns:
//! the `status` mirrored from the Order lifecycle (cross-aggregate, correlated by order_id), the
//! PUBLIC `messages` / INTERNAL `internal_notes` timelines appended from `MessagePosted` (split by
//! visibility) with `MessageTranslationAdded` folded into the matching message, the `admin_invited`
//! flag, and the current `muted` MutedParticipant set.
#![allow(unused_variables)]

use crate::projections::{Envelope, OrderConversationCompute, OrderConversationRow};
use domain::generated::events::{DomainEvent, MessagePosted, MessageTranslationAdded};
use domain::generated::scalars::{MessageVisibility, OrderStatus};
use serde_json::{json, Value};

pub struct OrderConversationProjector;

/// The prior array of a jsonb thread column (`messages` / `internal_notes` / `muted`), or empty before
/// the first fold. A projection column is always a JSON array by construction, so a non-array is treated
/// as empty rather than panicking.
fn prev_array(v: Option<&Value>) -> Vec<Value> {
    v.and_then(|j| j.as_array().cloned()).unwrap_or_default()
}

/// One `ConversationMessage` read-model element (camelCase keys match the SimpleObject's serde). The
/// author, visibility, body, locale and attachments come from the event; `postedAt` is the envelope's
/// occurred_at (envelope metadata, ADR-0041); `translations` starts empty and grows via
/// `MessageTranslationAdded`.
fn message_json(e: &MessagePosted, env: &Envelope) -> Value {
    json!({
        "messageId": e.message_id,
        "authorRole": e.author_role,
        "visibility": e.visibility,
        "body": e.body,
        "originalLocale": e.original_locale,
        "postedAt": env.occurred_at,
        "attachmentRefs": e.attachment_refs,
        "translations": [],
    })
}

/// Fold a translation into the matching message of `arr` (by messageId), idempotent per (message,
/// locale). A no-op when the message is not in this array — the dispatch recomputes BOTH threads on
/// `MessageTranslationAdded`, so the translation lands in whichever thread (PUBLIC/INTERNAL) holds it.
fn add_translation(arr: &mut [Value], e: &MessageTranslationAdded) {
    let target_id = e.message_id.0.to_string();
    for msg in arr.iter_mut() {
        if msg.get("messageId").and_then(|v| v.as_str()) != Some(target_id.as_str()) {
            continue;
        }
        if let Some(ts) = msg.get_mut("translations").and_then(|v| v.as_array_mut()) {
            let locale = e.locale.0.clone();
            let already = ts.iter().any(|t| t.get("locale").and_then(|v| v.as_str()) == Some(locale.as_str()));
            if !already {
                ts.push(json!({ "locale": e.locale, "text": e.text }));
            }
        }
    }
}

/// The PUBLIC or INTERNAL thread after this event: appends a `MessagePosted` of the matching
/// visibility, folds a `MessageTranslationAdded`, otherwise preserves the prior array.
fn fold_thread(mut arr: Vec<Value>, env: &Envelope, visibility: MessageVisibility) -> Value {
    match &env.event {
        DomainEvent::MessagePosted(e) if e.visibility == visibility => arr.push(message_json(e, env)),
        DomainEvent::MessageTranslationAdded(e) => add_translation(&mut arr, e),
        _ => {}
    }
    Value::Array(arr)
}

impl OrderConversationCompute for OrderConversationProjector {
    /// Order lifecycle status, mirrored from the Order stream (cross-aggregate, keyed by order_id).
    /// Defaults to PLACED for the opening event / before any status fold.
    fn status(&self, prev: Option<&OrderConversationRow>, env: &Envelope) -> OrderStatus {
        match &env.event {
            DomainEvent::OrderPlaced(_) => OrderStatus::PLACED,
            DomainEvent::OrderAcceptedByRestaurant(_) => OrderStatus::ACCEPTED,
            DomainEvent::OrderPreparationStarted(_) => OrderStatus::PREPARING,
            DomainEvent::OrderMarkedReady(_) => OrderStatus::READY,
            DomainEvent::OrderDelivered(_) => OrderStatus::DELIVERED,
            DomainEvent::OrderRejectedByRestaurant(_) => OrderStatus::REJECTED,
            DomainEvent::OrderCancelledByCustomer(_) => OrderStatus::CANCELLED_BY_CUSTOMER,
            DomainEvent::OrderCancelledByRestaurant(_) => OrderStatus::CANCELLED_BY_RESTAURANT,
            _ => prev.map(|r| r.status.clone()).unwrap_or(OrderStatus::PLACED),
        }
    }

    /// The PUBLIC (customer-visible) message timeline.
    fn messages(&self, prev: Option<&OrderConversationRow>, env: &Envelope) -> Value {
        fold_thread(prev_array(prev.map(|r| &r.messages)), env, MessageVisibility::PUBLIC)
    }

    /// The INTERNAL (staff-only) notes timeline.
    fn internal_notes(&self, prev: Option<&OrderConversationRow>, env: &Envelope) -> Value {
        fold_thread(prev_array(prev.map(|r| &r.internal_notes)), env, MessageVisibility::INTERNAL)
    }

    /// Latches true once an admin is invited (reasoned escalation); otherwise preserved.
    fn admin_invited(&self, prev: Option<&OrderConversationRow>, env: &Envelope) -> bool {
        match &env.event {
            DomainEvent::AdminInvitedToConversation(_) => true,
            _ => prev.map(|r| r.admin_invited).unwrap_or(false),
        }
    }

    /// The current muted-participant set: add (replacing any prior entry for the role) on
    /// `ParticipantMuted`, remove on `ParticipantUnmuted`.
    fn muted(&self, prev: Option<&OrderConversationRow>, env: &Envelope) -> Value {
        let mut arr = prev_array(prev.map(|r| &r.muted));
        match &env.event {
            DomainEvent::ParticipantMuted(e) => {
                let role = serde_json::to_value(&e.muted_role).unwrap_or(Value::Null);
                arr.retain(|m| m.get("role") != Some(&role));
                arr.push(json!({ "role": e.muted_role, "reason": e.reason, "until": e.until }));
            }
            DomainEvent::ParticipantUnmuted(e) => {
                let role = serde_json::to_value(&e.muted_role).unwrap_or(Value::Null);
                arr.retain(|m| m.get("role") != Some(&role));
            }
            _ => {}
        }
        Value::Array(arr)
    }

    /// The claim-lifecycle timeline woven into the order thread (§2.5, #155): appends one
    /// `ClaimTimelineEntry` per Reclamation* event, with `kind` derived from the event type and the
    /// per-kind fields carried through (category/requestedResolution on OPENED, resolution/refundAmount/
    /// note on RESOLVED, reason on REJECTED/REOPENED). `at` is the envelope's occurred_at (ADR-0041). All
    /// keys are always emitted (nulls where a kind does not carry them) so the read-model deserialization
    /// is total, mirroring `message_json`/`muted`. The row is keyed by the event's `orderId` upstream, so
    /// entries land on the right order thread; entries are appended in global `position` order.
    fn claim_events(&self, prev: Option<&OrderConversationRow>, env: &Envelope) -> Value {
        let mut arr = prev_array(prev.map(|r| &r.claim_events));
        let entry = match &env.event {
            DomainEvent::ReclamationOpened(e) => Some(json!({
                "kind": "OPENED",
                "reclamationId": e.reclamation_id,
                "category": e.category,
                "requestedResolution": e.requested_resolution,
                "resolution": Value::Null,
                "refundAmount": Value::Null,
                "reason": Value::Null,
                "at": env.occurred_at,
            })),
            DomainEvent::ReclamationResolved(e) => Some(json!({
                "kind": "RESOLVED",
                "reclamationId": e.reclamation_id,
                "category": Value::Null,
                "requestedResolution": Value::Null,
                "resolution": e.resolution,
                "refundAmount": e.refund_amount,
                "reason": e.note,
                "at": env.occurred_at,
            })),
            DomainEvent::ReclamationRejected(e) => Some(json!({
                "kind": "REJECTED",
                "reclamationId": e.reclamation_id,
                "category": Value::Null,
                "requestedResolution": Value::Null,
                "resolution": Value::Null,
                "refundAmount": Value::Null,
                "reason": e.reason,
                "at": env.occurred_at,
            })),
            DomainEvent::ReclamationReopened(e) => Some(json!({
                "kind": "REOPENED",
                "reclamationId": e.reclamation_id,
                "category": Value::Null,
                "requestedResolution": Value::Null,
                "resolution": Value::Null,
                "refundAmount": Value::Null,
                "reason": e.reason,
                "at": env.occurred_at,
            })),
            _ => None,
        };
        if let Some(entry) = entry {
            arr.push(entry);
        }
        Value::Array(arr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projections::project_order_conversation;
    use domain::generated::events::{
        ConversationOpened, MessagePosted, OrderAcceptedByRestaurant, ReclamationOpened,
        ReclamationResolved,
    };
    use domain::generated::scalars::{
        ConversationAuthorRole, ConversationMessageId, CustomerId, Locale, MessageBody, OrderId,
        ReclamationCategory, ReclamationId, ReclamationResolution, RestaurantId,
    };

    const NIL: &str = "00000000-0000-0000-0000-000000000000";

    fn env(event: DomainEvent) -> Envelope {
        Envelope {
            stream_name: "Conversation-1".into(),
            position: 1,
            occurred_at: chrono::DateTime::from_timestamp(1, 0).unwrap(),
            event,
        }
    }

    fn opened() -> DomainEvent {
        DomainEvent::ConversationOpened(ConversationOpened {
            order_id: OrderId(NIL.parse().unwrap()),
            restaurant_id: RestaurantId(NIL.parse().unwrap()),
            customer_chat_enabled: true,
        })
    }

    fn posted(visibility: MessageVisibility, body: &str) -> DomainEvent {
        DomainEvent::MessagePosted(MessagePosted {
            order_id: OrderId(NIL.parse().unwrap()),
            message_id: ConversationMessageId(uuid::Uuid::new_v4()),
            author_role: ConversationAuthorRole::CUSTOMER,
            visibility,
            body: MessageBody(body.into()),
            original_locale: Locale("fr-FR".into()),
            attachment_refs: vec![],
        })
    }

    /// A PUBLIC message lands in `messages`, an INTERNAL one in `internal_notes` — never crossed.
    #[test]
    fn message_visibility_splits_public_from_internal() {
        let c = OrderConversationProjector;
        let row = project_order_conversation(&c, None, &env(opened())).unwrap();
        let row = project_order_conversation(&c, Some(row), &env(posted(MessageVisibility::PUBLIC, "hi"))).unwrap();
        let row = project_order_conversation(&c, Some(row), &env(posted(MessageVisibility::INTERNAL, "note"))).unwrap();
        assert_eq!(row.messages.as_array().unwrap().len(), 1);
        assert_eq!(row.internal_notes.as_array().unwrap().len(), 1);
        assert_eq!(row.messages[0].get("body").unwrap(), "hi");
        assert_eq!(row.internal_notes[0].get("body").unwrap(), "note");
    }

    /// The order's status folds into the conversation from the Order lifecycle events.
    #[test]
    fn order_status_folds_into_conversation() {
        let c = OrderConversationProjector;
        let row = project_order_conversation(&c, None, &env(opened())).unwrap();
        assert_eq!(row.status, OrderStatus::PLACED);
        let accepted = DomainEvent::OrderAcceptedByRestaurant(OrderAcceptedByRestaurant {
            order_id: OrderId(NIL.parse().unwrap()),
            restaurant_id: RestaurantId(NIL.parse().unwrap()),
            estimated_ready_at: None,
        });
        let row = project_order_conversation(&c, Some(row), &env(accepted)).unwrap();
        assert_eq!(row.status, OrderStatus::ACCEPTED);
    }

    /// A claim opened then resolved appends two `claim_events` entries, in order, on the order thread
    /// (§2.5, #155). The Reclamation* events are keyed onto the order row by the worker; here we drive the
    /// projector directly to prove the fold appends and preserves order.
    #[test]
    fn claim_lifecycle_weaves_into_the_thread() {
        let c = OrderConversationProjector;
        let row = project_order_conversation(&c, None, &env(opened())).unwrap();
        assert_eq!(row.claim_events.as_array().unwrap().len(), 0);

        let opened_claim = DomainEvent::ReclamationOpened(ReclamationOpened {
            reclamation_id: ReclamationId(uuid::Uuid::new_v4()),
            order_id: OrderId(NIL.parse().unwrap()),
            customer_id: CustomerId(NIL.parse().unwrap()),
            restaurant_id: RestaurantId(NIL.parse().unwrap()),
            category: ReclamationCategory::MISSING_ITEM,
            description: domain::generated::scalars::ReclamationDescription("Drinks missing.".into()),
            requested_resolution: Some(ReclamationResolution::FULL_REFUND),
        });
        let row = project_order_conversation(&c, Some(row), &env(opened_claim)).unwrap();

        let resolved_claim = DomainEvent::ReclamationResolved(ReclamationResolved {
            reclamation_id: ReclamationId(uuid::Uuid::new_v4()),
            order_id: OrderId(NIL.parse().unwrap()),
            resolution: ReclamationResolution::FULL_REFUND,
            note: None,
            refund_amount: None,
        });
        let row = project_order_conversation(&c, Some(row), &env(resolved_claim)).unwrap();

        let claims = row.claim_events.as_array().unwrap();
        assert_eq!(claims.len(), 2);
        assert_eq!(claims[0].get("kind").unwrap(), "OPENED");
        assert_eq!(claims[0].get("requestedResolution").unwrap(), "FULL_REFUND");
        assert_eq!(claims[1].get("kind").unwrap(), "RESOLVED");
        assert_eq!(claims[1].get("resolution").unwrap(), "FULL_REFUND");
    }
}
