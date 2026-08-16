//! Drift guards for the GENERATED typed actor clients, ACROSS THE PHASE-2 CRATE LINE (#306,
//! PROP-20260802-130500; originally #284 slice 1, PROP-20260728-152752 §2.1).
//!
//! WHY THIS FILE MOVED. These were unit tests inside `actor_client::enqueue` for as long as the
//! clients lived in this crate. Phase 2 put each `{Actor}Client` in its own crate, so the guard
//! has to run from OUTSIDE — which is strictly better: it now exercises the clients exactly as a
//! consumer does, through the same public surface, with no in-crate visibility to lean on. It
//! reaches the reference implementation through the D5 `test-fixtures` feature (dev-dependencies
//! only, never a release graph) and compares rows through [`EntryFixture`], the all-public mirror,
//! since `MailboxEntry`'s fields stay private to the boundary crate.
//!
//! WHAT IT PROVES. A typed `send` must produce the very mailbox row `enqueue_worker_command`
//! builds for the same inputs, and a typed `record` the very row `enqueue_inbound_fact` builds —
//! field for field. If either assertion fails, the clients stopped delegating through
//! `ActorDoor` to the shared constructors and the doors have drifted. In-memory mailbox double;
//! no Postgres.

use std::sync::Arc;

use actor_client::mailbox::fixtures::EntryFixture;
use actor_client::mailbox::mem::MemMailbox;
use actor_client::mailbox::{Envelope, Mailbox};
use actor_client::{
    enqueue_inbound_fact, enqueue_worker_command, inbound_message_id, surrogate_actor_id,
    ActorClient, EnqueueOutcome, InboundFact, OperationStatusBus,
};
use application::ports::Actor;
use domain::generated::commands::MarkRestaurantClosed;
use domain::generated::entities::Money;
use domain::generated::events::PaymentCaptured;
use domain::generated::scalars::{
    CurrencyCode, InboundMessageStatus, MoneyCents, PaymentIntentId, RestaurantId,
};

use client_order::OrderClient;
use client_payment::PaymentClient;
use client_restaurant::RestaurantClient;

/// Field-for-field equality, by FULL destructuring of the public [`EntryFixture`] mirror with no
/// `..` rest pattern — so adding an 18th column to `MailboxEntry` is a COMPILE error here, not a
/// silently-unasserted field (the fixture's own conversions are exhaustive by the same trick, so
/// the new column is forced all the way through to this comparison). That structural
/// exhaustiveness is the guard's guarantee; a named-field comparison list only covers the columns
/// someone remembered.
fn assert_same_entry(typed: EntryFixture, free: EntryFixture) {
    let EntryFixture {
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
    let EntryFixture {
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
        typed.entry(message_id).expect("typed row").into_fixture(),
        free.entry(message_id).expect("free row").into_fixture(),
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

    // The identity MUST be the deterministic (source, external_id) key — never caller-minted — or
    // webhook redelivery double-applies instead of colliding on the pk.
    let message_id = inbound_message_id("stripe", "evt_84");
    let typed_row = typed.entry(message_id).expect("typed row keyed by the deterministic inbound id");
    let typed_payload = typed_row.payload().clone();
    assert_same_entry(
        typed_row.into_fixture(),
        free.entry(message_id).expect("free row").into_fixture(),
    );

    // The wire form must be the DOMAIN ENUM's own representation, not a hand-built literal that
    // happens to match it today: round-trip through `DomainEvent` so a tag/content rename in the
    // domain emitter fails HERE instead of at delivery time in production.
    let round_tripped: domain::generated::events::DomainEvent =
        serde_json::from_value(typed_payload)
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
    assert!(err.to_string().contains("does not match"), "the error names the mismatch: {err}");
    assert!(mailbox.entries().is_empty(), "nothing may reach the mailbox on a refused send");
}

/// Typed `schedule` has no free-function counterpart (it mints the first kind-COMMAND SCHEDULED
/// rows), so its guard is ABSOLUTE assertions instead of a drift comparison: the row must carry
/// the same `command_entry` columns as an immediate send plus the `scheduled_at` the caller gave —
/// and `cancel_scheduling` must withdraw it exactly once.
///
/// Exercises the ORDER client on purpose: the scheduling surface is SPEC-GATED (product-owner
/// directive, 2026-08-02 — no `schedule`/`cancel_scheduling` without a `reminders:` declaration),
/// and Order is the one declaring actor today. A `RestaurantClient` (no declaration) has no
/// `schedule` method at all — and since phase 2 it does not even live in the same crate, so that
/// absence is now a missing method on a type from a different dependency.
#[tokio::test]
async fn typed_schedule_parks_a_command_row_and_cancel_withdraws_it_once() {
    let order_id = uuid::Uuid::from_u128(0x0DE7);
    let cmd = domain::generated::commands::MarkOrderDelivered {
        order_id: domain::generated::scalars::OrderId(order_id),
        restaurant_id: RestaurantId(uuid::Uuid::from_u128(0xF00D)),
    };
    let message_id = uuid::Uuid::from_u128(0x5C);
    let at = chrono::DateTime::parse_from_rfc3339("2026-08-03T06:00:00Z")
        .expect("fixed timestamp")
        .with_timezone(&chrono::Utc);

    let mailbox = Arc::new(MemMailbox::default());
    let client = OrderClient::new(mailbox.clone(), order_id);
    client.schedule(cmd, test_envelope(message_id), at).await.expect("typed schedule");

    let row = mailbox.entry(message_id).expect("scheduled row");
    assert_eq!(row.kind(), "COMMAND");
    assert_eq!(row.actor_type(), "Order");
    assert_eq!(row.actor_id(), order_id);
    assert_eq!(row.message_type(), "MarkOrderDelivered");
    assert_eq!(
        row.partition(),
        actor_client::declared_lane("Order", &order_id).expect("Order declares a mailbox"),
        "the lane is derived from the actor type and the identity property asserted above, \
         over the DECLARED Order width — never a width this test carries"
    );
    assert_eq!(mailbox.scheduled_at(message_id), Some(at), "parked until due, not delivered now");

    assert!(client.cancel_scheduling(message_id).await.expect("cancel"), "a SCHEDULED row cancels");
    assert!(
        !client.cancel_scheduling(message_id).await.expect("second cancel"),
        "a spent cancellation reports false, not an error"
    );
}

/// The READ door answers with the very row the WRITE door accepted — same identity, same status
/// the mem worker left it in — and `None` for an unknown handle. Moved out of `client.rs` with
/// phase 2 for the same reason as the guards above: it needs a typed client, and typed clients are
/// no longer in this crate.
#[tokio::test]
async fn get_operation_status_reads_the_accepted_row_and_none_for_unknown() {
    let mem = Arc::new(MemMailbox::default());
    let restaurant_id = uuid::Uuid::from_u128(0xF00D);
    let message_id = uuid::Uuid::from_u128(0x0B5);
    let cmd = MarkRestaurantClosed { restaurant_id: RestaurantId(restaurant_id), reason: None };

    let writer = RestaurantClient::new(mem.clone() as Arc<dyn Mailbox>, restaurant_id);
    writer
        .send(
            cmd,
            Envelope {
                message_id,
                correlation_id: message_id,
                cause_id: None,
                session_id: None,
                trace_id: None,
                user_id: None,
                user_type: "EXTERNAL".into(),
                channel: "WORKER".into(),
            },
        )
        .await
        .expect("typed send");

    let reader = ActorClient::new(mem.clone(), OperationStatusBus::default());
    let row = reader
        .get_operation_status(message_id)
        .await
        .expect("read")
        .expect("the accepted row is visible through the read door");
    assert_eq!(row.message_id, message_id);
    assert_eq!(row.correlation_id, message_id);
    assert_eq!(row.status, InboundMessageStatus::RECEIVED);

    assert!(
        reader
            .get_operation_status(uuid::Uuid::from_u128(0xDEAD))
            .await
            .expect("read")
            .is_none(),
        "an unknown handle reads as absent, not as an error"
    );
}
