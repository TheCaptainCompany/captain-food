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

/// Field-for-field equality (MailboxEntry deliberately does not derive PartialEq), by FULL
/// destructuring with no `..` rest pattern — so adding an 18th column to `MailboxEntry` is a
/// COMPILE error here, not a silently-unasserted field. That structural exhaustiveness is the
/// guard's guarantee; a named-field comparison list only covers the columns someone remembered.
fn assert_same_entry(typed: &MailboxEntry, free: &MailboxEntry) {
    let MailboxEntry {
        message_id,
        kind,
        actor_type,
        actor_id,
        partition,
        message_type,
        payload,
        payload_hash,
        channel,
        user_id,
        user_type,
        correlation_id,
        cause_id,
        session_id,
        trace_id,
        source,
        external_id,
    } = typed;
    let MailboxEntry {
        message_id: f_message_id,
        kind: f_kind,
        actor_type: f_actor_type,
        actor_id: f_actor_id,
        partition: f_partition,
        message_type: f_message_type,
        payload: f_payload,
        payload_hash: f_payload_hash,
        channel: f_channel,
        user_id: f_user_id,
        user_type: f_user_type,
        correlation_id: f_correlation_id,
        cause_id: f_cause_id,
        session_id: f_session_id,
        trace_id: f_trace_id,
        source: f_source,
        external_id: f_external_id,
    } = free;
    assert_eq!(message_id, f_message_id, "message_id");
    assert_eq!(kind, f_kind, "kind");
    assert_eq!(actor_type, f_actor_type, "actor_type");
    assert_eq!(actor_id, f_actor_id, "actor_id");
    assert_eq!(partition, f_partition, "partition");
    assert_eq!(message_type, f_message_type, "message_type");
    assert_eq!(payload, f_payload, "payload");
    assert_eq!(payload_hash, f_payload_hash, "payload_hash");
    assert_eq!(channel, f_channel, "channel");
    assert_eq!(user_id, f_user_id, "user_id");
    assert_eq!(user_type, f_user_type, "user_type");
    assert_eq!(correlation_id, f_correlation_id, "correlation_id");
    assert_eq!(cause_id, f_cause_id, "cause_id");
    assert_eq!(session_id, f_session_id, "session_id");
    assert_eq!(trace_id, f_trace_id, "trace_id");
    assert_eq!(source, f_source, "source");
    assert_eq!(external_id, f_external_id, "external_id");
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
    let typed_row = typed.entry(message_id).expect("typed row keyed by the deterministic inbound id");
    assert_same_entry(&typed_row, &free.entry(message_id).expect("free row"));

    // The wire form must be the DOMAIN ENUM's own representation, not a hand-built literal that
    // happens to match it today: round-trip through `DomainEvent` so a tag/content rename in the
    // domain emitter fails HERE instead of at delivery time in production.
    let round_tripped: domain::generated::events::DomainEvent =
        serde_json::from_value(typed_row.payload.clone())
            .expect("recorded payload deserializes as DomainEvent — the delivery route's own type");
    assert!(
        matches!(round_tripped, domain::generated::events::DomainEvent::PaymentCaptured(_)),
        "the adjacent tag routes back to the variant that was recorded"
    );
}

/// Fix-1 invariant (#288 review): a payload whose DECLARED identity names a DIFFERENT aggregate
/// than the client's lane must be REFUSED at the door. Accepting it would park the command on one
/// lane while the handler acts on another aggregate — per-aggregate serialization silently broken,
/// which is the exact failure the mailbox exists to prevent.
#[tokio::test]
async fn a_mis_addressed_send_is_refused() {
    let lane = uuid::Uuid::from_u128(0xAAAA);
    let other = uuid::Uuid::from_u128(0xBBBB);
    let cmd = MarkRestaurantClosed { restaurant_id: RestaurantId(other), reason: None };
    let mailbox = Arc::new(MemMailbox::default());
    let client = RestaurantClient::new(mailbox.clone(), lane);

    let err = client
        .send(cmd, test_envelope(uuid::Uuid::from_u128(0x10)))
        .await
        .expect_err("identity mismatch must refuse, not mis-file");
    assert!(
        err.to_string().contains("does not match"),
        "the error names the mismatch: {err}"
    );
    assert!(mailbox.entries().is_empty(), "nothing may reach the mailbox on a refused send");
}

/// Typed `schedule` has no free-function counterpart (it mints the first kind-COMMAND SCHEDULED
/// rows), so its guard is ABSOLUTE assertions instead of a drift comparison: the row must carry
/// the same `command_entry` columns as an immediate send plus the `scheduled_at` the caller gave —
/// and `cancel` must withdraw it exactly once.
#[tokio::test]
async fn typed_schedule_parks_a_command_row_and_cancel_withdraws_it_once() {
    let restaurant_id = uuid::Uuid::from_u128(0xF00D);
    let cmd = MarkRestaurantClosed {
        restaurant_id: RestaurantId(restaurant_id),
        reason: Some("scheduled closure".into()),
    };
    let message_id = uuid::Uuid::from_u128(0x5C);
    let at = chrono::DateTime::parse_from_rfc3339("2026-08-03T06:00:00Z")
        .expect("fixed timestamp")
        .with_timezone(&chrono::Utc);

    let mailbox = Arc::new(MemMailbox::default());
    let client = RestaurantClient::new(mailbox.clone(), restaurant_id);
    client
        .schedule(cmd, test_envelope(message_id), at)
        .await
        .expect("typed schedule");

    let row = mailbox.entry(message_id).expect("scheduled row");
    assert_eq!(row.kind, "COMMAND");
    assert_eq!(row.actor_type, "Restaurant");
    assert_eq!(row.actor_id, restaurant_id);
    assert_eq!(row.message_type, "MarkRestaurantClosed");
    assert_eq!(row.partition, actor_runtime::stable_partition(&restaurant_id, 100));
    assert_eq!(mailbox.scheduled_at(message_id), Some(at), "parked until due, not delivered now");

    assert!(client.cancel(message_id).await.expect("cancel"), "a SCHEDULED row cancels");
    assert!(
        !client.cancel(message_id).await.expect("second cancel"),
        "a spent cancellation reports false, not an error"
    );
}

/// The envelope every test hands the client — deterministic ids, WORKER channel.
fn test_envelope(message_id: uuid::Uuid) -> Envelope {
    Envelope {
        message_id,
        correlation_id: uuid::Uuid::from_u128(0x3),
        cause_id: None,
        session_id: None,
        trace_id: None,
        user_id: Some(uuid::Uuid::from_u128(0x2)),
        user_type: "EXTERNAL".into(),
        channel: "WORKER".into(),
    }
}
