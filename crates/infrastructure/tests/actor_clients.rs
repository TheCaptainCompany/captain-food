//! Drift guards for the GENERATED typed actor clients (#284 slice 1, PROP-20260728-152752 §2.1):
//! a typed `send` must produce the very mailbox row `enqueue_worker_command` builds for the same
//! inputs, and a typed `record` the very row `enqueue_inbound_fact` builds — field for field.
//! If either assertion ever fails, the clients stopped delegating to the shared constructors in
//! `mailbox::enqueue` and the two doors have drifted. In-memory mailbox double; no Postgres.

use std::sync::Arc;

use application::mailbox::mem::MemMailbox;
use application::mailbox::{Envelope, MailboxEntry};
use application::ports::Actor;
use domain::generated::commands::MarkRestaurantClosed;
use domain::generated::entities::Money;
use domain::generated::events::PaymentCaptured;
use domain::generated::scalars::{CurrencyCode, MoneyCents, PaymentIntentId, RestaurantId};
use infrastructure::generated::actor_clients::{PaymentClient, RestaurantClient};
use infrastructure::mailbox::{
    enqueue_inbound_fact, enqueue_worker_command, inbound_message_id, surrogate_actor_id,
    EnqueueOutcome, InboundFact,
};

/// Field-for-field equality (MailboxEntry deliberately does not derive PartialEq): every column
/// named so a new envelope column cannot slip past the drift guard unasserted.
fn assert_same_entry(typed: &MailboxEntry, free: &MailboxEntry) {
    assert_eq!(typed.message_id, free.message_id, "message_id");
    assert_eq!(typed.kind, free.kind, "kind");
    assert_eq!(typed.actor_type, free.actor_type, "actor_type");
    assert_eq!(typed.actor_id, free.actor_id, "actor_id");
    assert_eq!(typed.partition, free.partition, "partition");
    assert_eq!(typed.message_type, free.message_type, "message_type");
    assert_eq!(typed.payload, free.payload, "payload");
    assert_eq!(typed.payload_hash, free.payload_hash, "payload_hash");
    assert_eq!(typed.channel, free.channel, "channel");
    assert_eq!(typed.user_id, free.user_id, "user_id");
    assert_eq!(typed.user_type, free.user_type, "user_type");
    assert_eq!(typed.correlation_id, free.correlation_id, "correlation_id");
    assert_eq!(typed.cause_id, free.cause_id, "cause_id");
    assert_eq!(typed.session_id, free.session_id, "session_id");
    assert_eq!(typed.trace_id, free.trace_id, "trace_id");
    assert_eq!(typed.source, free.source, "source");
    assert_eq!(typed.external_id, free.external_id, "external_id");
}

#[tokio::test]
async fn typed_send_builds_the_same_row_as_enqueue_worker_command() {
    let restaurant_id = uuid::Uuid::from_u128(0xF00D);
    let cmd = MarkRestaurantClosed {
        restaurant_id: RestaurantId(restaurant_id),
        reason: Some("SIRENE closure".into()),
    };
    let message_id = uuid::Uuid::from_u128(0x1);
    let actor = Actor {
        user_id: uuid::Uuid::from_u128(0x2),
        user_type: "EXTERNAL".into(),
        domain_id: None,
        correlation_id: uuid::Uuid::from_u128(0x3),
        cause_id: Some(uuid::Uuid::from_u128(0x4)),
    };

    let free = MemMailbox::default();
    let outcome = enqueue_worker_command(
        &free,
        message_id,
        "MarkRestaurantClosed",
        serde_json::to_value(&cmd).expect("serialize command"),
        &actor,
    )
    .await
    .expect("free-function enqueue");
    assert_eq!(outcome, EnqueueOutcome::Enqueued);

    let typed = Arc::new(MemMailbox::default());
    let client = RestaurantClient::new(typed.clone(), restaurant_id);
    let env = Envelope {
        message_id,
        correlation_id: actor.correlation_id,
        cause_id: actor.cause_id,
        session_id: None,
        trace_id: None,
        user_id: Some(actor.user_id),
        user_type: actor.user_type.clone(),
        channel: "WORKER".into(),
    };
    assert_eq!(client.send(cmd, env).await.expect("typed send"), EnqueueOutcome::Enqueued);

    assert_same_entry(
        &typed.entry(message_id).expect("typed row"),
        &free.entry(message_id).expect("free row"),
    );
}

#[tokio::test]
async fn typed_record_builds_the_same_row_as_enqueue_inbound_fact() {
    let fact = PaymentCaptured {
        payment_intent_id: PaymentIntentId("pi_84".into()),
        order_id: None,
        restaurant_id: RestaurantId(uuid::Uuid::from_u128(0xF00D)),
        amount: Money { amount_cents: MoneyCents(1990), currency: CurrencyCode("EUR".into()) },
    };
    // The Payment lane id is the UUIDv5 surrogate over the gateway's intent id — the same key the
    // Stripe ACL uses, so the typed client and the adapter land on the same lane.
    let actor_id = surrogate_actor_id("Payment", "pi_84");
    let correlation_id = uuid::Uuid::from_u128(0xC0);
    let tagged = serde_json::json!({
        "eventType": "PaymentCaptured",
        "payload": serde_json::to_value(&fact).expect("serialize fact"),
    });

    let free = MemMailbox::default();
    let outcome = enqueue_inbound_fact(
        &free,
        InboundFact {
            source: "stripe".into(),
            external_id: "evt_84".into(),
            event_type: "PaymentCaptured".into(),
            payload: tagged,
            correlation_id,
            actor_type: "Payment".into(),
            actor_id,
        },
    )
    .await
    .expect("free-function enqueue");
    assert_eq!(outcome, EnqueueOutcome::Enqueued);

    let typed = Arc::new(MemMailbox::default());
    let client = PaymentClient::new(typed.clone(), actor_id);
    assert_eq!(
        client.record(fact, "stripe", "evt_84", correlation_id).await.expect("typed record"),
        EnqueueOutcome::Enqueued
    );

    // The identity MUST be the deterministic (source, external_id) key — never caller-minted —
    // or webhook redelivery double-applies instead of colliding on the pk.
    let message_id = inbound_message_id("stripe", "evt_84");
    assert_same_entry(
        &typed.entry(message_id).expect("typed row keyed by the deterministic inbound id"),
        &free.entry(message_id).expect("free row"),
    );
}
