//! Conversation aggregate — the PURE write-side state fold (#129, ADR-20260725-015921), mirroring
//! `delivery_partner_registration.rs`. A per-order in-app message thread
//! (`specs/actors.yaml#/Conversation`); its identity IS its order, so id = orderId. The conversation is
//! BORN by its `ConversationOpened` fact (which snapshots whether customer↔restaurant chat is enabled)
//! and grows one `MessagePosted` at a time. The fold tracks only what the invariants read: existence
//! (`ConversationNotFound` / `ConversationAlreadyOpen`), whether customer chat is enabled
//! (`CustomerChatDisabled`) and the set of already-posted message ids (`MessageAlreadyPosted`). No I/O.

use crate::generated::events::DomainEvent;
use crate::generated::scalars::ConversationMessageId;

/// What the Conversation command handlers need to accept or reject a command. `None` (from [`fold`])
/// means no `ConversationOpened` yet — the conversation does not exist.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationState {
    /// Snapshotted at open: whether a CUSTOMER may post to this thread (else it is staff-only).
    pub customer_chat_enabled: bool,
    /// Message ids already appended — the idempotency guard for `PostMessage`.
    pub message_ids: Vec<ConversationMessageId>,
}

/// Fold a Conversation stream (events in version order) into its current state. `None` ⇔ the stream
/// has no `ConversationOpened` yet, i.e. the conversation does not exist.
pub fn fold(events: &[DomainEvent]) -> Option<ConversationState> {
    events.iter().fold(None, apply)
}

/// Apply one event — a pure transition, total over the whole event union.
fn apply(state: Option<ConversationState>, event: &DomainEvent) -> Option<ConversationState> {
    match event {
        // The birth fact: establishes the conversation, snapshotting the customer-chat opt-in.
        DomainEvent::ConversationOpened(e) => Some(ConversationState {
            customer_chat_enabled: e.customer_chat_enabled,
            message_ids: Vec::new(),
        }),
        // Append the message id (idempotency guard); a MessagePosted without a birth is impossible.
        DomainEvent::MessagePosted(e) => {
            let mut s = state?;
            s.message_ids.push(e.message_id);
            Some(s)
        }
        _ => state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::Aggregate;
    use crate::generated::events::{ConversationOpened, MessagePosted};
    use crate::generated::scalars::{
        ConversationAuthorRole, Locale, MessageBody, MessageVisibility, OrderId, RestaurantId,
    };

    fn opened(customer_chat_enabled: bool) -> DomainEvent {
        DomainEvent::ConversationOpened(ConversationOpened {
            order_id: OrderId(uuid::Uuid::nil()),
            restaurant_id: RestaurantId(uuid::Uuid::nil()),
            customer_chat_enabled,
        })
    }
    fn posted(id: uuid::Uuid) -> DomainEvent {
        DomainEvent::MessagePosted(MessagePosted {
            order_id: OrderId(uuid::Uuid::nil()),
            message_id: ConversationMessageId(id),
            author_role: ConversationAuthorRole::CUSTOMER,
            visibility: MessageVisibility::PUBLIC,
            body: MessageBody("hi".into()),
            original_locale: Locale("fr-FR".into()),
            attachment_refs: Vec::new(),
        })
    }

    #[test]
    fn no_open_means_no_conversation() {
        assert_eq!(fold(&[]), None);
        // A message without a birth never materializes a conversation.
        assert_eq!(fold(&[posted(uuid::Uuid::nil())]), None);
    }

    #[test]
    fn open_births_the_conversation_with_the_chat_snapshot() {
        assert!(fold(&[opened(true)]).unwrap().customer_chat_enabled);
        assert!(!fold(&[opened(false)]).unwrap().customer_chat_enabled);
    }

    #[test]
    fn posted_messages_accumulate_in_order() {
        let m1 = uuid::Uuid::from_u128(1);
        let m2 = uuid::Uuid::from_u128(2);
        let s = fold(&[opened(true), posted(m1), posted(m2)]).unwrap();
        assert_eq!(s.message_ids, vec![ConversationMessageId(m1), ConversationMessageId(m2)]);
    }

    #[test]
    fn stream_name_matches_the_aggregate_format() {
        let id = uuid::Uuid::nil();
        assert_eq!(ConversationState::stream(OrderId(id)), format!("Conversation-{id}"));
    }
}
