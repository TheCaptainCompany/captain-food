//! #582 — behaviour of the typed `ask` on the sealed per-actor clients (PROP-20260815-142349 §7),
//! exercised OUT-OF-CRATE exactly as a consumer does (the drift_guard.rs precedent).
//!
//! The seam is the `EventStore` PORT, faked here the same way
//! `crates/application/src/process_managers/mod.rs` fakes it (a `Mutex<HashMap>` of streams) —
//! the port is the fake's boundary, never a mocked adapter. Three modeled outcomes, each a test:
//!   - `Answered`: a scripted fold fixture from a REALISTIC multi-event stream (never
//!     `Default::default()`), with `served_version` asserted EXACTLY equal to the fixture's
//!     event count — `>0` would let an off-by-one envelope rot silently.
//!   - `Absent`: the fake returns `(vec![], 0)` → the fold births nothing → `AskOutcome::Absent`
//!     exactly (not `Err`, not a fabricated `Answered`).
//!   - `Deadline`: `start_paused` + a fake whose `load` NEVER completes + `advance()` past the
//!     caller's deadline → `AskOutcome::Deadline` — deterministic, no real sleeps. Seen RED
//!     before the timeout wrapper existed ("the future never completes" was the red — PR #583).
//!
//! Every match below names all three arms with NO `_` wildcard — the V5 exhaustiveness witness:
//! adding a fourth arm to `AskOutcome` refuses to compile until every caller decides.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use actor_client::ask::AskOutcome;
use application::ports::{Actor, EventStore};
use client_order::{OrderAnswer, OrderClient};
use client_payment::{PaymentAnswer, PaymentClient};
use domain::generated::answers::{OrderPaymentReferenceRequest, PaymentSettlementViewRequest};
use domain::generated::entities::{
    CheckoutSnapshot, CustomerContact, Money, PaymentBreakdown,
};
use domain::generated::events::{
    DomainEvent, OrderAcceptedByRestaurant, OrderDelivered, OrderMarkedReady, OrderPlaced,
    OrderPreparationStarted, PaymentAuthorized, PaymentCaptureFailed, PaymentIntentCreated,
};
use domain::generated::scalars::*;
use domain::shared::errors::DomainError;

// ── the port fake (the application MemStore pattern — reused, not a mocked adapter) ─────────────

#[derive(Default)]
struct MemStore {
    streams: Mutex<HashMap<String, Vec<DomainEvent>>>,
}

impl MemStore {
    fn seed(&self, stream: &str, events: Vec<DomainEvent>) {
        self.streams.lock().unwrap().insert(stream.to_string(), events);
    }
}

#[async_trait::async_trait]
impl EventStore for MemStore {
    async fn append(
        &self,
        _stream_name: &str,
        _expected_version: i64,
        _events: &[DomainEvent],
        _actor: &Actor,
    ) -> Result<i64, DomainError> {
        unreachable!("an ask is READ-ONLY — any append through this seam is a defect")
    }

    async fn load(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError> {
        let events =
            self.streams.lock().unwrap().get(stream_name).cloned().unwrap_or_default();
        let version = events.len() as i64;
        Ok((events, version))
    }
}

/// The Deadline seam: a load that NEVER completes (a stalled pool, a wedged replica). The paused
/// clock advances past the caller's deadline and the ask must come back `Deadline` — modeled,
/// never a hang.
struct StalledStore;

#[async_trait::async_trait]
impl EventStore for StalledStore {
    async fn append(
        &self,
        _stream_name: &str,
        _expected_version: i64,
        _events: &[DomainEvent],
        _actor: &Actor,
    ) -> Result<i64, DomainError> {
        unreachable!("an ask is READ-ONLY")
    }

    async fn load(&self, _stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError> {
        std::future::pending().await
    }
}

// ── realistic fixtures (the order.rs / payment.rs builder shapes) ───────────────────────────────

fn oid() -> OrderId {
    OrderId(uuid::Uuid::nil())
}
fn rid() -> RestaurantId {
    RestaurantId(uuid::Uuid::nil())
}
fn pi() -> PaymentIntentId {
    PaymentIntentId("pi_test_1".into())
}
fn money(cents: i64) -> Money {
    Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
}
fn breakdown() -> PaymentBreakdown {
    let z = money(0);
    PaymentBreakdown {
        articles: z.clone(),
        delivery: z.clone(),
        service_fee: z.clone(),
        total: money(1000),
        restaurant_contribution: z.clone(),
        restaurant_payout: z.clone(),
        rider_payout: z.clone(),
        captain_net: z,
    }
}
fn contact() -> CustomerContact {
    CustomerContact {
        display_name: CustomerDisplayName("Jo".into()),
        email: None,
        phone: PhoneNumber("+33600000000".into()),
    }
}

fn placed(payment_intent_id: Option<PaymentIntentId>) -> DomainEvent {
    DomainEvent::OrderPlaced(OrderPlaced {
        mode: None,
        order_id: oid(),
        r#ref: None,
        restaurant_id: rid(),
        customer_id: CustomerId(uuid::Uuid::nil()),
        customer_contact: contact(),
        service_type: ServiceType::DELIVERY,
        delivery_address: None,
        items: vec![],
        total_amount: money(1000),
        breakdown: breakdown(),
        note: None,
        replacement_of: None,
        payment_intent_id,
    })
}

/// placed → accepted → preparing → ready → delivered: FIVE events, so `served_version` must be
/// exactly 5 — the envelope carries the fold's version, not a flag.
fn delivered_order_stream(payment_intent_id: Option<PaymentIntentId>) -> Vec<DomainEvent> {
    vec![
        placed(payment_intent_id),
        DomainEvent::OrderAcceptedByRestaurant(OrderAcceptedByRestaurant {
            order_id: oid(),
            restaurant_id: rid(),
            estimated_ready_at: None,
        }),
        DomainEvent::OrderPreparationStarted(OrderPreparationStarted {
            order_id: oid(),
            restaurant_id: rid(),
        }),
        DomainEvent::OrderMarkedReady(OrderMarkedReady { order_id: oid(), restaurant_id: rid() }),
        DomainEvent::OrderDelivered(OrderDelivered { order_id: oid(), restaurant_id: rid() }),
    ]
}

fn intent_created() -> DomainEvent {
    DomainEvent::PaymentIntentCreated(PaymentIntentCreated {
        payment_intent_id: pi(),
        restaurant_id: rid(),
        customer_id: CustomerId(uuid::Uuid::nil()),
        amount: money(1000),
        checkout: CheckoutSnapshot {
            order_id: oid(),
            cart_id: CartId(uuid::Uuid::nil()),
            restaurant_id: rid(),
            customer_id: CustomerId(uuid::Uuid::nil()),
            mode: None,
            r#ref: None,
            customer_contact: contact(),
            service_type: ServiceType::DELIVERY,
            delivery_address: None,
            items: vec![],
            total_amount: money(1000),
            breakdown: breakdown(),
            note: None,
            verdict: None,
            window_from: None,
            window_to: None,
            timezone: None,
            evaluated_at: None,
        },
    })
}

/// intent → authorized → capture-failed: THREE events, `served_version` exactly 3.
fn authorized_payment_stream() -> Vec<DomainEvent> {
    vec![
        intent_created(),
        DomainEvent::PaymentAuthorized(PaymentAuthorized {
            payment_intent_id: pi(),
            order_id: Some(oid()),
            restaurant_id: rid(),
            amount: money(1000),
        }),
        DomainEvent::PaymentCaptureFailed(PaymentCaptureFailed {
            payment_intent_id: pi(),
            order_id: oid(),
            restaurant_id: rid(),
            reason: CaptureFailureReason::CARD_DECLINED,
            detail: Some("Your card was declined.".into()),
        }),
    ]
}

fn order_client() -> OrderClient {
    OrderClient::new(std::sync::Arc::new(actor_client::mailbox::mem::MemMailbox::default()), uuid::Uuid::nil())
}
fn payment_client() -> PaymentClient {
    PaymentClient::new(std::sync::Arc::new(actor_client::mailbox::mem::MemMailbox::default()), uuid::Uuid::nil())
}

const DEADLINE: Duration = Duration::from_millis(250);

// ── Answered ────────────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn order_ask_answers_with_the_projected_fold_and_the_exact_served_version() {
    let store = MemStore::default();
    store.seed(&format!("Order-{}", uuid::Uuid::nil()), delivered_order_stream(Some(pi())));
    let out = order_client()
        .ask(OrderPaymentReferenceRequest { order_id: oid() }, &store, DEADLINE)
        .await
        .expect("infrastructure is healthy");
    // All three arms named, no wildcard — the V5 exhaustiveness witness.
    match out {
        AskOutcome::Answered { reply, served_version } => {
            assert_eq!(reply.payment_intent_id, Some(pi()));
            // EXACTLY the fixture's event count (5), never `>0`.
            assert_eq!(served_version, 5);
        }
        AskOutcome::Absent => panic!("a delivered order stream is not absent"),
        AskOutcome::Deadline => panic!("the store answered instantly"),
    }
    // QUERY_TYPE is the whole address — the ref path encodes the actor (PROP §2).
    assert_eq!(
        <OrderPaymentReferenceRequest as OrderAnswer>::QUERY_TYPE,
        "actors.yaml#/Order/answers/paymentReference"
    );
}

#[tokio::test]
async fn order_ask_serves_an_absent_payment_reference_as_answered_none() {
    // A $0 replacement order EXISTS (Answered) but has no payment intent — reply None, so the
    // caller's absence-branch is forced by the type, not by a missing stream.
    let store = MemStore::default();
    store.seed(&format!("Order-{}", uuid::Uuid::nil()), delivered_order_stream(None));
    let out = order_client()
        .ask(OrderPaymentReferenceRequest { order_id: oid() }, &store, DEADLINE)
        .await
        .unwrap();
    match out {
        AskOutcome::Answered { reply, served_version } => {
            assert_eq!(reply.payment_intent_id, None);
            assert_eq!(served_version, 5);
        }
        AskOutcome::Absent => panic!("the replacement order exists — Absent would be a lie"),
        AskOutcome::Deadline => panic!("the store answered instantly"),
    }
}

#[tokio::test]
async fn payment_ask_answers_the_settlement_view_with_the_exact_served_version() {
    let store = MemStore::default();
    store.seed("Payment-pi_test_1", authorized_payment_stream());
    let out = payment_client()
        .ask(PaymentSettlementViewRequest { payment_intent_id: pi() }, &store, DEADLINE)
        .await
        .unwrap();
    match out {
        AskOutcome::Answered { reply, served_version } => {
            assert_eq!(reply.order_id, oid());
            assert_eq!(reply.status, PaymentStatus::AUTHORIZED);
            assert!(reply.capture_failed);
            // EXACTLY the fixture's event count (3).
            assert_eq!(served_version, 3);
        }
        AskOutcome::Absent => panic!("the payment stream exists"),
        AskOutcome::Deadline => panic!("the store answered instantly"),
    }
    // The generated stream address equals the hand helper's — the format edge is pinned.
    assert_eq!(
        PaymentSettlementViewRequest { payment_intent_id: pi() }.stream_name(),
        domain::payment::stream(&pi())
    );
}

// ── Absent ──────────────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn ask_on_an_unborn_stream_is_absent_not_an_error() {
    // The fake returns (vec![], 0): the fold births nothing → Absent EXACTLY — never Err, never
    // a fabricated Answered (the modeled arm the old Skip(String) prose could not carry).
    let store = MemStore::default();
    let out = payment_client()
        .ask(PaymentSettlementViewRequest { payment_intent_id: pi() }, &store, DEADLINE)
        .await
        .unwrap();
    match out {
        AskOutcome::Answered { .. } => panic!("an unborn stream cannot answer"),
        AskOutcome::Absent => {}
        AskOutcome::Deadline => panic!("the store answered instantly"),
    }
    // A stream with facts but NO birth event folds to nothing — still Absent (payment.rs's
    // "a fact without a birth folds to nothing").
    let store = MemStore::default();
    store.seed("Payment-pi_test_1", authorized_payment_stream()[1..].to_vec());
    let out = payment_client()
        .ask(PaymentSettlementViewRequest { payment_intent_id: pi() }, &store, DEADLINE)
        .await
        .unwrap();
    assert_eq!(out, AskOutcome::Absent);
}

// ── Deadline ────────────────────────────────────────────────────────────────────────────────────

#[tokio::test(start_paused = true)]
async fn ask_past_the_callers_deadline_is_deadline_not_a_hang() {
    // Paused clock + a load that never completes: advance past the caller's deadline and the
    // ask must resolve to Deadline — deterministic, no real sleeps. RED before the timeout
    // wrapper existed: this future never completed (PR #583 records the hang).
    let client = order_client();
    let ask = client.ask(OrderPaymentReferenceRequest { order_id: oid() }, &StalledStore, DEADLINE);
    tokio::pin!(ask);
    // Not resolved before the deadline…
    assert!(
        futures_poll_once(ask.as_mut()).await.is_none(),
        "the ask must still be pending before the deadline"
    );
    tokio::time::advance(DEADLINE + Duration::from_millis(1)).await;
    let out = ask.await.unwrap();
    match out {
        AskOutcome::Answered { .. } => panic!("a stalled store cannot answer"),
        AskOutcome::Absent => panic!("a stalled store proves nothing about absence"),
        AskOutcome::Deadline => {}
    }
}

/// Poll a future exactly once (no external futures crate needed).
async fn futures_poll_once<F: std::future::Future>(mut f: std::pin::Pin<&mut F>) -> Option<F::Output> {
    std::future::poll_fn(|cx| {
        std::task::Poll::Ready(match f.as_mut().poll(cx) {
            std::task::Poll::Ready(v) => Some(v),
            std::task::Poll::Pending => None,
        })
    })
    .await
}
