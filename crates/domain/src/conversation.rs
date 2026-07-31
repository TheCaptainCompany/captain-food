//! Conversation aggregate — the PURE write-side state fold (#129, ADR-20260725-015921), mirroring
//! `delivery_partner_registration.rs`. A per-order in-app message thread
//! (`specs/actors.yaml#/Conversation`); its identity IS its order, so id = orderId. The conversation is
//! BORN by its `ConversationOpened` fact (which snapshots whether customer↔restaurant chat is enabled)
//! and grows one `MessagePosted` at a time. The fold tracks only what the invariants read: existence
//! (`ConversationNotFound` / `ConversationAlreadyOpen`), whether customer chat is enabled
//! (`CustomerChatDisabled`) and the set of already-posted message ids (`MessageAlreadyPosted`). No I/O.

use crate::generated::events::DomainEvent;
use crate::generated::scalars::{
    ConversationAuthorRole, ConversationMessageId, CustomerId, Locale, RestaurantId,
};

/// What the Conversation command handlers need to accept or reject a command. `None` (from [`fold`])
/// means no `ConversationOpened` yet — the conversation does not exist.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationState {
    /// The customer participant the `requires.acting.CUSTOMER` check authorizes against
    /// (#235 consequence A; specs/actors.yaml Conversation state.customerId). `None` = guest
    /// order — a CUSTOMER principal is then always denied.
    pub customer_id: Option<CustomerId>,
    /// The restaurant participant (`requires.acting.RESTAURANT`; enforcement waits on the #144
    /// staff-identity bridge).
    pub restaurant_id: RestaurantId,
    /// Snapshotted at open: whether a CUSTOMER may post to this thread (else it is staff-only).
    pub customer_chat_enabled: bool,
    /// Message ids already appended — the idempotency guard for `PostMessage`.
    pub message_ids: Vec<ConversationMessageId>,
    /// Whether an admin was pulled into the thread by a reasoned escalation (#129).
    pub admin_invited: bool,
    /// The participant roles currently muted — the set `MuteParticipant`/`UnmuteParticipant` read
    /// (`ParticipantNotMuted`; #129).
    pub muted_roles: Vec<ConversationAuthorRole>,
    /// The (message, locale) pairs already translated — the idempotency guard for
    /// `RecordMessageTranslation` (`TranslationAlreadyRecorded`; #129).
    pub translations: Vec<(ConversationMessageId, Locale)>,
}

/// A violated `requires` precondition (specs/actors.yaml Conversation/PostMessage — #235,
/// PROP-20260728-135632 §2.2). Mapped to `errors.yaml#/NotAParticipant` / `#/RoleMismatch` by the
/// application handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequiresViolation {
    NotAParticipant,
    RoleMismatch,
}

/// The PostMessage `requires` check — PURE, on the actor, over its own folded state (D4 as
/// amended: the actor owns authorization, so testing the actor tests it). Hand-written at parity
/// with the spec declaration until the generated precondition lands (#242 runtime branch).
///
/// STAGED enforcement, deliberately: `claims` (authorRole pinned to the principal, with
/// RESTAURANT_ACCOUNT legitimately posting staff notes as RESTAURANT) and `acting.CUSTOMER`
/// (the live #235 forgery hole) are enforced; `acting` for RESTAURANT/RESTAURANT_ACCOUNT/RIDER
/// passes until the #144 staff/rider identity bridges land — the known, documented residual.
pub fn requires_post_message(
    state: &ConversationState,
    actor_user_type: &str,
    actor_domain_id: Option<uuid::Uuid>,
    author_role: &ConversationAuthorRole,
) -> Result<(), RequiresViolation> {
    let claim_ok = match author_role {
        ConversationAuthorRole::CUSTOMER => actor_user_type == "CUSTOMER",
        ConversationAuthorRole::RESTAURANT => {
            matches!(actor_user_type, "RESTAURANT" | "RESTAURANT_ACCOUNT")
        }
        ConversationAuthorRole::RIDER => actor_user_type == "RIDER",
        ConversationAuthorRole::ADMIN => actor_user_type == "ADMIN",
    };
    if !claim_ok {
        return Err(RequiresViolation::RoleMismatch);
    }
    match actor_user_type {
        // acting: any — stated in the spec, never implied.
        "ADMIN" => Ok(()),
        // acting: state.customerId — the resolved domain identity must BE the folded participant;
        // a guest thread (None) or an unresolved principal denies.
        "CUSTOMER" => match (&state.customer_id, actor_domain_id) {
            (Some(cid), Some(did)) if cid.0 == did => Ok(()),
            _ => Err(RequiresViolation::NotAParticipant),
        },
        // acting: state.restaurantId — enforcement waits on the #144 identity bridge (no resolved
        // restaurant identity exists yet to compare against the folded participant).
        "RESTAURANT" | "RESTAURANT_ACCOUNT" => Ok(()),
        // RIDER is DELIBERATELY ABSENT from `requires.acting` in the spec: absent = denied,
        // unconditionally — no identity bridge is needed to know a rider is never a conversation
        // participant. (The claim gate above already pinned author_role RIDER to user_type RIDER;
        // this denies the acting side too.)
        _ => Err(RequiresViolation::NotAParticipant),
    }
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
            customer_id: e.customer_id,
            restaurant_id: e.restaurant_id,
            customer_chat_enabled: e.customer_chat_enabled,
            message_ids: Vec::new(),
            admin_invited: false,
            muted_roles: Vec::new(),
            translations: Vec::new(),
        }),
        // Append the message id (idempotency guard); a MessagePosted without a birth is impossible.
        DomainEvent::MessagePosted(e) => {
            let mut s = state?;
            s.message_ids.push(e.message_id);
            Some(s)
        }
        // Cache a message translation: record the (message, locale) pair if not already present
        // (idempotency guard for RecordMessageTranslation; #129).
        DomainEvent::MessageTranslationAdded(e) => {
            let mut s = state?;
            let pair = (e.message_id, e.locale.clone());
            if !s.translations.contains(&pair) {
                s.translations.push(pair);
            }
            Some(s)
        }
        // An admin was pulled in by a reasoned escalation (#129).
        DomainEvent::AdminInvitedToConversation(_) => {
            let mut s = state?;
            s.admin_invited = true;
            Some(s)
        }
        // Mute a role: add it to the current set (idempotent — never duplicated).
        DomainEvent::ParticipantMuted(e) => {
            let mut s = state?;
            if !s.muted_roles.contains(&e.muted_role) {
                s.muted_roles.push(e.muted_role);
            }
            Some(s)
        }
        // Unmute a role: drop it from the current set.
        DomainEvent::ParticipantUnmuted(e) => {
            let mut s = state?;
            s.muted_roles.retain(|role| *role != e.muted_role);
            Some(s)
        }
        _ => state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::Aggregate;
    use crate::generated::events::{
        AdminInvitedToConversation, ConversationOpened, MessagePosted, MessageTranslationAdded,
        ParticipantMuted, ParticipantUnmuted,
    };
    use crate::generated::scalars::{
        ConversationAuthorRole, EscalationReason, Locale, MessageBody, MessageVisibility, MuteReason,
        OrderId, RestaurantId, TranslatedText,
    };

    fn opened(customer_chat_enabled: bool) -> DomainEvent {
        DomainEvent::ConversationOpened(ConversationOpened {
            order_id: OrderId(uuid::Uuid::nil()),
            restaurant_id: RestaurantId(uuid::Uuid::nil()),
            customer_id: None,
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

    fn escalated() -> DomainEvent {
        DomainEvent::AdminInvitedToConversation(AdminInvitedToConversation {
            order_id: OrderId(uuid::Uuid::nil()),
            reason: EscalationReason("need arbitration".into()),
        })
    }
    fn muted(role: ConversationAuthorRole) -> DomainEvent {
        DomainEvent::ParticipantMuted(ParticipantMuted {
            order_id: OrderId(uuid::Uuid::nil()),
            muted_role: role,
            reason: MuteReason("off-topic".into()),
            until: None,
        })
    }
    fn unmuted(role: ConversationAuthorRole) -> DomainEvent {
        DomainEvent::ParticipantUnmuted(ParticipantUnmuted {
            order_id: OrderId(uuid::Uuid::nil()),
            muted_role: role,
        })
    }

    #[test]
    fn escalation_marks_the_admin_invited() {
        assert!(!fold(&[opened(true)]).unwrap().admin_invited);
        assert!(fold(&[opened(true), escalated()]).unwrap().admin_invited);
    }

    #[test]
    fn mute_then_unmute_tracks_the_current_muted_set() {
        let s = fold(&[opened(true), muted(ConversationAuthorRole::RIDER)]).unwrap();
        assert_eq!(s.muted_roles, vec![ConversationAuthorRole::RIDER]);
        // Muting an already-muted role never duplicates it.
        let s = fold(&[opened(true), muted(ConversationAuthorRole::RIDER), muted(ConversationAuthorRole::RIDER)])
            .unwrap();
        assert_eq!(s.muted_roles, vec![ConversationAuthorRole::RIDER]);
        // Unmuting removes it again.
        let s = fold(&[opened(true), muted(ConversationAuthorRole::RIDER), unmuted(ConversationAuthorRole::RIDER)])
            .unwrap();
        assert!(s.muted_roles.is_empty());
    }

    fn translated(message: uuid::Uuid, locale: &str) -> DomainEvent {
        DomainEvent::MessageTranslationAdded(MessageTranslationAdded {
            order_id: OrderId(uuid::Uuid::nil()),
            message_id: ConversationMessageId(message),
            locale: Locale(locale.into()),
            text: TranslatedText("hello".into()),
        })
    }

    #[test]
    fn translations_accumulate_once_per_message_locale() {
        let m1 = uuid::Uuid::from_u128(1);
        // A translation is tracked as a (message, locale) pair.
        let s = fold(&[opened(true), posted(m1), translated(m1, "en-US")]).unwrap();
        assert_eq!(s.translations, vec![(ConversationMessageId(m1), Locale("en-US".into()))]);
        // Re-recording the same (message, locale) never duplicates it.
        let s = fold(&[opened(true), posted(m1), translated(m1, "en-US"), translated(m1, "en-US")])
            .unwrap();
        assert_eq!(s.translations, vec![(ConversationMessageId(m1), Locale("en-US".into()))]);
        // A different locale for the same message is a distinct pair.
        let s = fold(&[opened(true), posted(m1), translated(m1, "en-US"), translated(m1, "es-ES")])
            .unwrap();
        assert_eq!(
            s.translations,
            vec![
                (ConversationMessageId(m1), Locale("en-US".into())),
                (ConversationMessageId(m1), Locale("es-ES".into())),
            ]
        );
    }

    #[test]
    fn stream_name_matches_the_aggregate_format() {
        let id = uuid::Uuid::nil();
        assert_eq!(ConversationState::stream(OrderId(id)), format!("Conversation-{id}"));
    }
}
