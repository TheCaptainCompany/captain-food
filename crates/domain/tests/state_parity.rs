//! PARITY: the GENERATED declared-state fold (`generated::states::conversation`, emitted from
//! specs/actors.yaml `Conversation/state` — #242 Runtime A, PROP-20260728-135632 §2.1) must agree
//! FIELD BY FIELD with the hand-written write-side fold (`conversation::fold`) on every legal
//! stream. This is the executable proof that the spec lineage and the aggregate can never drift:
//! if either side changes shape or semantics, this test names the field that disagrees.
//!
//! "Legal stream" matters: the streams below are ones the command handlers can actually produce
//! (birth first, no duplicate message ids — `MessageAlreadyPosted` rejects those before append).
//! The generated fold is set-semantic (guarded insert) where the hand fold trusts the command
//! guard, so only illegal, never-appendable streams could distinguish them — by construction.

use domain::conversation as hand;
use domain::generated::events::{
    AdminInvitedToConversation, ConversationOpened, DomainEvent, MessagePosted,
    MessageTranslationAdded, ParticipantMuted, ParticipantUnmuted,
};
use domain::generated::scalars::{
    ConversationAuthorRole, ConversationMessageId, CustomerId, EscalationReason, Locale,
    MessageBody, MessageVisibility, MuteReason, OrderId, RestaurantId, TranslatedText,
};
use domain::generated::states::conversation as generated;

fn oid() -> OrderId {
    OrderId(uuid::Uuid::from_u128(1))
}

fn opened(customer: Option<u128>, chat_enabled: bool) -> DomainEvent {
    DomainEvent::ConversationOpened(ConversationOpened {
        order_id: oid(),
        restaurant_id: RestaurantId(uuid::Uuid::from_u128(7)),
        customer_id: customer.map(|n| CustomerId(uuid::Uuid::from_u128(n))),
        customer_chat_enabled: chat_enabled,
    })
}

fn posted(message: u128, role: ConversationAuthorRole) -> DomainEvent {
    DomainEvent::MessagePosted(MessagePosted {
        order_id: oid(),
        message_id: ConversationMessageId(uuid::Uuid::from_u128(message)),
        author_role: role,
        visibility: MessageVisibility::PUBLIC,
        body: MessageBody("hello".into()),
        original_locale: Locale("fr-FR".into()),
        attachment_refs: vec![],
    })
}

fn translated(message: u128, locale: &str) -> DomainEvent {
    DomainEvent::MessageTranslationAdded(MessageTranslationAdded {
        order_id: oid(),
        message_id: ConversationMessageId(uuid::Uuid::from_u128(message)),
        locale: Locale(locale.into()),
        text: TranslatedText("bonjour".into()),
    })
}

fn muted(role: ConversationAuthorRole) -> DomainEvent {
    DomainEvent::ParticipantMuted(ParticipantMuted {
        order_id: oid(),
        muted_role: role,
        reason: MuteReason("abuse".into()),
        until: None,
    })
}

fn unmuted(role: ConversationAuthorRole) -> DomainEvent {
    DomainEvent::ParticipantUnmuted(ParticipantUnmuted { order_id: oid(), muted_role: role })
}

fn escalated() -> DomainEvent {
    DomainEvent::AdminInvitedToConversation(AdminInvitedToConversation {
        order_id: oid(),
        reason: EscalationReason("rider unreachable".into()),
    })
}

/// Field-by-field equality between the two folds over one stream — a disagreement names the field.
fn assert_parity(events: &[DomainEvent]) {
    let h = hand::fold(events);
    let g = generated::fold(events);
    match (h, g) {
        (None, None) => {}
        (Some(h), Some(g)) => {
            assert_eq!(h.customer_id, g.customer_id, "customer_id diverges");
            assert_eq!(h.restaurant_id, g.restaurant_id, "restaurant_id diverges");
            assert_eq!(
                h.customer_chat_enabled, g.customer_chat_enabled,
                "customer_chat_enabled diverges"
            );
            assert_eq!(h.message_ids, g.message_ids, "message_ids diverges");
            assert_eq!(h.admin_invited, g.admin_invited, "admin_invited diverges");
            assert_eq!(h.muted_roles, g.muted_roles, "muted_roles diverges");
            assert_eq!(h.translations, g.translations, "translations diverges");
        }
        (h, g) => panic!(
            "existence diverges: hand-written = {}, generated = {}",
            h.is_some(),
            g.is_some()
        ),
    }
}

#[test]
fn empty_stream_folds_to_none_on_both_sides() {
    assert_parity(&[]);
}

#[test]
fn events_before_birth_fold_to_none_on_both_sides() {
    // Impossible on a real stream (the birth is version 1), asserted anyway: both folds must
    // refuse to conjure a state out of a non-birth event.
    assert_parity(&[posted(10, ConversationAuthorRole::CUSTOMER)]);
    assert_parity(&[escalated(), muted(ConversationAuthorRole::CUSTOMER)]);
}

#[test]
fn birth_snapshots_participants_and_chat_flag() {
    assert_parity(&[opened(Some(42), true)]);
    assert_parity(&[opened(None, false)]); // guest order, staff-only thread
}

#[test]
fn full_legal_lifecycle_stays_in_lockstep() {
    let stream = vec![
        opened(Some(42), true),
        posted(10, ConversationAuthorRole::CUSTOMER),
        posted(11, ConversationAuthorRole::RESTAURANT),
        translated(10, "en-US"),
        translated(10, "de-DE"),
        escalated(),
        muted(ConversationAuthorRole::CUSTOMER),
        posted(12, ConversationAuthorRole::ADMIN),
        unmuted(ConversationAuthorRole::CUSTOMER),
        posted(13, ConversationAuthorRole::CUSTOMER),
    ];
    // Parity must hold at EVERY prefix, not just at the end — the fold is consulted mid-stream
    // by every command handler.
    for len in 0..=stream.len() {
        assert_parity(&stream[..len]);
    }
    // And the folded values themselves are what the spec lineage says.
    let s = generated::fold(&stream).expect("state exists");
    assert_eq!(s.message_ids.len(), 4);
    assert!(s.admin_invited);
    assert!(s.muted_roles.is_empty(), "mute/unmute round-trips to empty");
    assert_eq!(s.translations.len(), 2);
}

#[test]
fn idempotent_re_delivery_of_set_lineages_stays_in_lockstep() {
    // The two idempotency-guarded lineages the spec declares as sets: a re-recorded translation
    // and a re-muted role are ignorable no-ops on BOTH sides (the hand fold guards these two
    // explicitly). Duplicate MessagePosted is deliberately absent: the stream can never carry it
    // (MessageAlreadyPosted rejects before append), and set-vs-append there is the one place the
    // folds are allowed to differ on illegal input.
    let stream = vec![
        opened(Some(42), true),
        posted(10, ConversationAuthorRole::CUSTOMER),
        translated(10, "en-US"),
        translated(10, "en-US"), // duplicate (message, locale)
        muted(ConversationAuthorRole::RIDER),
        muted(ConversationAuthorRole::RIDER), // idempotent re-mute
    ];
    for len in 0..=stream.len() {
        assert_parity(&stream[..len]);
    }
    let s = generated::fold(&stream).expect("state exists");
    assert_eq!(s.translations.len(), 1, "duplicate translation recorded once");
    assert_eq!(s.muted_roles, vec![ConversationAuthorRole::RIDER], "re-mute recorded once");
}
