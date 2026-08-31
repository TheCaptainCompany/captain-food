//! **T5 (#780) — THE FACT DOOR THROUGH THE REAL SEAM**, against a real Postgres.
//!
//! The verdict table (`mailbox::handler::verdict_table`) proves the DECISION; this proves the
//! EFFECT, and it is here because two of the effects are invisible to a pure test and expensive to
//! lose:
//!
//! 1. **The post-commit fan-out.** An inbound `PaymentCaptured` must append to the Payment stream
//!    AND publish onto the event bus, because `paymentStatusChanged` is what moves the customer's
//!    checkout screen. Dropping the publish breaks nothing the type system or the verdict table can
//!    see — it shows up as a screen that never updates while the money moved.
//! 2. **The PM branch does not ALSO record.** A PM-addressed copy of a payment fact runs the saga's
//!    event leg; if it fell through to the record route as well, the same money fact would land on
//!    the Payment stream twice and every downstream fold would double-count it.
//!
//! Both were true before #780 and both are now carried by a typed route rather than by a string
//! comparison on `actor_type`, so this suite is what stops that refactor being a silent regression.

use std::sync::Arc;

use actor_runtime::{MailboxWorker, WorkerConfig};
use application::generated::services::{IdentityService, PaymentService};
use infrastructure::generated::command_router::CommandDeps;
use infrastructure::mailbox::MailboxCommandHandler;
use infrastructure::persistence::event_bus::EventBus;
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, FailClosedPaymentGateway,
    PgCatalogRepository, PgCustomerRepository, PgEventStore, PgProspectionRepository,
    PgRestaurantRepository, PgSlugReservationRepository, UnverifiedGbpOrderLinkProbe,
};
use sqlx::{PgPool, Row};

const INTENT: &str = "pi_780_fact_door";

fn deps_over(pool: &PgPool) -> CommandDeps {
    CommandDeps {
        store: Arc::new(PgEventStore::new(pool.clone())),
        restaurants: Arc::new(PgRestaurantRepository::new(pool.clone())),
        slugs: Arc::new(PgSlugReservationRepository::new(pool.clone())),
        ownership: Arc::new(FailClosedGoogleOwnershipVerifier),
        probe: Arc::new(UnverifiedGbpOrderLinkProbe),
        prospection: Arc::new(PgProspectionRepository::new(pool.clone())),
        catalogs: Arc::new(PgCatalogRepository::new(pool.clone())),
        auth: Arc::new(FailClosedIdentityService) as Arc<dyn IdentityService>,
        customers: Arc::new(PgCustomerRepository::new(pool.clone())),
        sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
        payments: Arc::new(FailClosedPaymentGateway) as Arc<dyn PaymentService>,
        pm_state: Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone())),
        refund_state: Arc::new(infrastructure::persistence::PgRefundProcessState::new(pool.clone())),
        mailbox_requeue: Arc::new(
            infrastructure::persistence::mailbox_lanes::PgMailboxRequeue::new(pool.clone()),
        ),
        enforce_service_hours_guard: false,
        enforce_acceptance_timeout: false,
        route_gates: application::generated::process_managers::RouteGates {
            order_placed_to_order: false,
            place_replacement_order_to_order: false,
        },
    }
}

/// Enqueue one kind-EVENT row: the wire shape the Stripe ACL produces — an adjacently-tagged
/// `DomainEvent` in `payload`, the lane in `actor_type`, the fact name in `message_type`.
#[allow(clippy::too_many_arguments)]
async fn enqueue_fact(
    pool: &PgPool,
    n: u128,
    actor_type: &str,
    actor_id: uuid::Uuid,
    partition: i16,
    message_type: &str,
    event: serde_json::Value,
) -> uuid::Uuid {
    let id = uuid::Uuid::from_u128(n);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, user_id, correlation_id) \
         VALUES ($1, 'EVENT', $2, $3, $4, $5, $6, $7, 'EXTERNAL', 'EXTERNAL', NULL, $1)",
    )
    .bind(id)
    .bind(actor_type)
    .bind(actor_id)
    .bind(partition)
    .bind(message_type)
    .bind(&event)
    .bind(format!("h{n}"))
    .execute(pool)
    .await
    .expect("enqueue fact");
    id
}

fn payment_captured(restaurant: uuid::Uuid) -> serde_json::Value {
    serde_json::json!({
        "eventType": "PaymentCaptured",
        "payload": {
            "paymentIntentId": INTENT,
            "orderId": null,
            "restaurantId": restaurant,
            "amount": { "amountCents": 1960, "currency": "EUR" },
        }
    })
}

/// An inbound `PaymentCaptured` records on the Payment stream **and** publishes the post-commit
/// fan-out. The publish is the half nothing else covers: it is what `paymentStatusChanged` rides.
#[tokio::test]
async fn an_inbound_payment_capture_records_and_publishes() {
    let Some(db) = crate::common::TestDb::acquire("fact_delivery_capture").await else { return };
    let pool = db.pool();
    let restaurant = uuid::Uuid::from_u128(0x7E51);
    // The Payment lane is keyed by the intent, so the row's actor_id is a derived uuid; the stream
    // is resolved from the PAYLOAD's intent id, which is the point of `intent_of_fact`.
    let lane_actor = uuid::Uuid::from_u128(0x7E52);

    let message_id =
        enqueue_fact(&pool, 0x7801, "Payment", lane_actor, 0, "PaymentCaptured", payment_captured(restaurant))
            .await;

    let bus = EventBus::default();
    let mut rx = bus.subscribe();
    let worker = MailboxWorker::new(
        pool.clone(),
        "w-fact",
        "Payment",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(deps_over(&pool)).with_event_bus(bus.clone())),
    );
    worker.seed(5).await.expect("seed");
    worker.claim().await.expect("claim");
    assert_eq!(worker.drain().await.expect("drain"), 1, "the fact delivered in one pass");

    // THE APPEND: on the Payment aggregate's OWN stream, keyed by the intent in the payload, with
    // the mailbox row as its cause.
    let events = sqlx::query(
        "SELECT event_type, cause_id, user_type FROM domain_events WHERE stream_name = $1 \
         ORDER BY version",
    )
    .bind(format!("Payment-{INTENT}"))
    .fetch_all(&pool)
    .await
    .expect("events");
    assert_eq!(events.len(), 1, "exactly one append, on the Payment stream");
    assert_eq!(events[0].get::<String, _>("event_type"), "PaymentCaptured");
    assert_eq!(events[0].get::<Option<uuid::Uuid>, _>("cause_id"), Some(message_id));
    assert_eq!(
        events[0].get::<String, _>("user_type"),
        "EXTERNAL",
        "the acting principal is the ACL's system identity (ADR-0041)"
    );

    let status: String =
        sqlx::query_scalar("SELECT status FROM inbound_messages WHERE message_id = $1")
            .bind(message_id)
            .fetch_one(&pool)
            .await
            .expect("verdict");
    assert_eq!(status, "SUCCEEDED");

    // THE FAN-OUT. Dropping it is invisible to every other assertion in this file and shows up in
    // production as a checkout screen that never updates while the money moved.
    let published = rx.try_recv().expect(
        "the post-commit fan-out must publish the recorded fact -- `paymentStatusChanged` rides it",
    );
    assert_eq!(published.event_type, "PaymentCaptured");
    assert_eq!(published.stream_name, format!("Payment-{INTENT}"));
}

/// A REDELIVERED capture lands DUPLICATE and appends nothing: the aggregate's own fold
/// (`domain::payment::already_records`) stays the single idempotency rule on the money path.
#[tokio::test]
async fn a_redelivered_capture_is_absorbed_by_the_aggregates_own_fold() {
    let Some(db) = crate::common::TestDb::acquire("fact_delivery_dedupe").await else { return };
    let pool = db.pool();
    let restaurant = uuid::Uuid::from_u128(0x7E51);
    let lane_actor = uuid::Uuid::from_u128(0x7E52);

    let first =
        enqueue_fact(&pool, 0x7811, "Payment", lane_actor, 0, "PaymentCaptured", payment_captured(restaurant))
            .await;
    let again =
        enqueue_fact(&pool, 0x7812, "Payment", lane_actor, 0, "PaymentCaptured", payment_captured(restaurant))
            .await;

    let worker = MailboxWorker::new(
        pool.clone(),
        "w-fact",
        "Payment",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(deps_over(&pool))),
    );
    worker.seed(5).await.expect("seed");
    worker.claim().await.expect("claim");
    assert_eq!(worker.drain().await.expect("drain"), 2);

    let appended: i64 =
        sqlx::query_scalar("SELECT count(*) FROM domain_events WHERE stream_name = $1")
            .bind(format!("Payment-{INTENT}"))
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(appended, 1, "the second delivery must append NOTHING -- a money event recorded \
                             twice is double-counted by every downstream fold");

    let statuses: Vec<(uuid::Uuid, String)> =
        sqlx::query("SELECT message_id, status FROM inbound_messages ORDER BY position")
            .fetch_all(&pool)
            .await
            .expect("verdicts")
            .iter()
            .map(|r| (r.get("message_id"), r.get("status")))
            .collect();
    assert_eq!(statuses[0], (first, "SUCCEEDED".into()));
    assert_eq!(
        statuses[1],
        (again, "DUPLICATE".into()),
        "the aggregate's no-change decision is PERSISTED on the row (ADR-20260728-011344 D6)"
    );
}

/// A PM-ADDRESSED copy of a payment fact runs the saga's event leg and does **not** also take the
/// record route.
///
/// Before #780 that branch was `if message.actor_type == "PlaceOrderProcess" || ... == \
/// "RefundProcess"`; it is now the typed `FactLeg::ProcessManager`. Losing it would append the same
/// money fact to the Payment stream a second time from the saga's own hop.
#[tokio::test]
async fn a_pm_addressed_payment_fact_runs_its_leg_and_does_not_also_record() {
    let Some(db) = crate::common::TestDb::acquire("fact_delivery_pm_branch").await else { return };
    let pool = db.pool();
    let order = uuid::Uuid::from_u128(0x7821);
    let restaurant = uuid::Uuid::from_u128(0x7E51);

    // A PlaceOrderProcess-addressed `PaymentAuthorized`: the saga's own trigger, on the saga's lane.
    let message_id = enqueue_fact(
        &pool,
        0x7822,
        "PlaceOrderProcess",
        order,
        0,
        "PaymentAuthorized",
        serde_json::json!({
            "eventType": "PaymentAuthorized",
            "payload": {
                "paymentIntentId": INTENT,
                "orderId": null,
                "restaurantId": restaurant,
                "amount": { "amountCents": 1960, "currency": "EUR" },
            }
        }),
    )
    .await;

    let worker = MailboxWorker::new(
        pool.clone(),
        "w-pm",
        "PlaceOrderProcess",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(deps_over(&pool))),
    );
    worker.seed(5).await.expect("seed");
    worker.claim().await.expect("claim");
    assert_eq!(worker.drain().await.expect("drain"), 1);

    // THE INVARIANT: the saga leg ran, and the Payment stream did NOT gain a copy of the trigger.
    // (The fact is already on that stream when the saga reacts to it; this hop reacts, it does not
    // re-record.)
    let on_payment: i64 =
        sqlx::query_scalar("SELECT count(*) FROM domain_events WHERE stream_name = $1")
            .bind(format!("Payment-{INTENT}"))
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(
        on_payment, 0,
        "a PM-addressed copy must NOT take the record route -- the same money fact on the Payment \
         stream twice is double-counted by every downstream fold"
    );

    // And the row completed with a verdict rather than aborting: the leg was dispatched, not
    // dropped. A saga skip is IGNORED, a completed leg SUCCEEDED, an anomaly REJECTED -- all three
    // are dispatches; the failure this pins is a row left RECEIVED or FAILED "no PM event leg".
    let status: String =
        sqlx::query_scalar("SELECT status FROM inbound_messages WHERE message_id = $1")
            .bind(message_id)
            .fetch_one(&pool)
            .await
            .expect("verdict");
    assert!(
        ["SUCCEEDED", "IGNORED", "REJECTED"].contains(&status.as_str()),
        "the PM leg must be DISPATCHED, whatever it decides -- got {status}"
    );
}

/// A DECLARED fact with no record route **PARKS**: the row stays deliverable, the attempt counter
/// advances, and nothing is written to `inbound_messages.error`.
///
/// This is the property that decides whether a routed `deliver:` landing before its fold rule loses
/// the fact or merely delays it — asserted as the transient POSTURE (row still RECEIVED, attempts
/// advanced), never on the message text.
#[tokio::test]
async fn a_declared_fact_with_no_record_route_parks_and_stays_deliverable() {
    let Some(db) = crate::common::TestDb::acquire("fact_delivery_park").await else { return };
    let pool = db.pool();
    let job = uuid::Uuid::from_u128(0x78d1);

    let message_id = enqueue_fact(
        &pool,
        0x78d2,
        "DeliveryJob",
        job,
        0,
        "DeliveryRequested",
        serde_json::json!({
            "eventType": "DeliveryRequested",
            "payload": {
                "deliveryJobId": job,
                "orderId": uuid::Uuid::from_u128(0x78d3),
                "restaurantId": uuid::Uuid::from_u128(0x78d4),
                "pickup": { "line1": "1 rue de la Paix", "city": "Tours", "postalCode": "37000", "country": "FR" },
                "dropoff": { "line1": "2 rue Nationale", "city": "Tours", "postalCode": "37000", "country": "FR" },
            }
        }),
    )
    .await;

    let worker = MailboxWorker::new(
        pool.clone(),
        "w-park",
        "DeliveryJob",
        // Pacing OFF so the single attempt is observable without waiting out a backoff window;
        // the cap is left high so the row does NOT flip to poison inside this test.
        WorkerConfig {
            lease_seconds: 300,
            retry_spacing_seconds: 0,
            max_delivery_attempts: 50,
            ..WorkerConfig::default()
        },
        Arc::new(MailboxCommandHandler::new(deps_over(&pool))),
    );
    worker.seed(5).await.expect("seed");
    worker.claim().await.expect("claim");
    let _ = worker.drain().await;

    let row = sqlx::query(
        "SELECT status, attempts, error FROM inbound_messages WHERE message_id = $1",
    )
    .bind(message_id)
    .fetch_one(&pool)
    .await
    .expect("row");
    assert_eq!(
        row.get::<String, _>("status"),
        "RECEIVED",
        "a declared fact with no record route must stay DELIVERABLE -- a terminal verdict would be \
         invisible to `poisonedMailboxMessages` and refused by `RequeueMailboxMessage`, and the \
         redelivery that would supposedly rescue it is absorbed by the enqueue-side pk dedupe"
    );
    assert!(
        row.get::<i16, _>("attempts") >= 1,
        "the attempt must be COUNTED -- that is what carries the row to the poison queue at the cap"
    );
    assert!(
        row.get::<Option<serde_json::Value>, _>("error").is_none(),
        "the park path writes NOTHING to the 90-day error column: the diagnosis goes to the log"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM domain_events WHERE stream_name = $1")
            .bind(format!("DeliveryJob-{job}"))
            .fetch_one(&pool)
            .await
            .expect("count"),
        0,
        "and nothing was appended"
    );
}

// ─── THE FIVE NEWLY-WIRED PAYMENT FACTS (PR #783 review B1) ──────────────────────────────────────
//
// `PaymentCaptureFailed`, `PaymentIntentCreated`, `RefundApproved`, `RefundDenied` and
// `RefundOpened` are the five the `Payment` actor declares it receives that the pre-review route
// could NOT record: it widened the lane's typed fact to a `DomainEvent` and handed it to
// `record_inbound_payment_event`, whose stream lookup ended in `_ => None` over exactly these five.
// The result was not a rejection — a fact cannot be refused — but a `Repository` error that becomes
// `sqlx::Error::Protocol`, aborts the transaction, and re-enters the lane HEAD-OF-LINE until the
// attempts cap. The reviewer's probe measured it: `status=RECEIVED attempts=1
// appended_on_payment_stream=0`.
//
// The suite was green throughout, because it exercised only `PaymentCaptured`/`PaymentAuthorized`
// — both already routed before that branch (beck: a suite that does not predict production for the
// facts the change delivers). These two tests are that probe made permanent.

fn eur(cents: i64) -> domain::generated::entities::Money {
    use domain::generated::scalars::{CurrencyCode, MoneyCents};
    domain::generated::entities::Money {
        amount_cents: MoneyCents(cents),
        currency: CurrencyCode("EUR".into()),
    }
}

/// The wire shape of a fact, built from the TYPED event rather than hand-written JSON.
///
/// Hand-written JSON is how a fixture drifts from the payload it claims to be: it keeps parsing
/// after a field is added and the test goes on asserting about a shape production no longer has.
/// Serializing the real struct means the row carries exactly what the Stripe ACL stages.
fn staged(event: domain::generated::events::DomainEvent) -> serde_json::Value {
    serde_json::to_value(&event).expect("a DomainEvent serializes to its adjacently-tagged wire shape")
}

fn refund_opened(intent: &str, order: uuid::Uuid, restaurant: uuid::Uuid) -> domain::generated::events::DomainEvent {
    use domain::generated::scalars::{OrderId, PaymentIntentId, RestaurantId};
    domain::generated::events::DomainEvent::RefundOpened(domain::generated::events::RefundOpened {
        payment_intent_id: PaymentIntentId(intent.into()),
        order_id: OrderId(order),
        restaurant_id: RestaurantId(restaurant),
        amount: eur(1960),
        reason: Some("customer cancelled after capture".into()),
    })
}

/// **THE MONEY FACT THE REFUND QUEUE IS FOLDED FROM RECORDS.**
///
/// `RefundOpened` is the sole feeder of `View_PendingRefunds`
/// (`specs/database/projection_views.yaml`), so losing it is not a missing row in a report: the
/// restaurant is never asked to decide, and money captured from a customer stays captured. It is
/// therefore the one of the five worth asserting end to end — through the real worker, against a
/// real Postgres, on the real lane.
#[tokio::test]
async fn an_inbound_refund_opened_records_on_the_payment_stream() {
    let Some(db) = crate::common::TestDb::acquire("fact_delivery_refund_opened").await else {
        return;
    };
    let pool = db.pool();
    let intent = "pi_780_refund_opened";
    let order = uuid::Uuid::from_u128(0x7E61);
    let restaurant = uuid::Uuid::from_u128(0x7E51);
    let lane_actor = uuid::Uuid::from_u128(0x7E62);

    let message_id = enqueue_fact(
        &pool,
        0x7831,
        "Payment",
        lane_actor,
        0,
        "RefundOpened",
        staged(refund_opened(intent, order, restaurant)),
    )
    .await;

    let bus = EventBus::default();
    let mut rx = bus.subscribe();
    let worker = MailboxWorker::new(
        pool.clone(),
        "w-refund",
        "Payment",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(deps_over(&pool)).with_event_bus(bus.clone())),
    );
    worker.seed(5).await.expect("seed");
    worker.claim().await.expect("claim");
    assert_eq!(worker.drain().await.expect("drain"), 1, "the fact delivered in one pass");

    // THE APPEND. This is the assertion the pre-review branch failed: zero rows here, with the
    // message left RECEIVED and the lane wedged behind it.
    let events = sqlx::query(
        "SELECT event_type, cause_id, user_type FROM domain_events WHERE stream_name = $1 \
         ORDER BY version",
    )
    .bind(format!("Payment-{intent}"))
    .fetch_all(&pool)
    .await
    .expect("events");
    assert_eq!(
        events.len(),
        1,
        "RefundOpened must APPEND on the Payment stream -- View_PendingRefunds folds from this \
         fact and from nothing else, so a fact that does not record is a refund nobody is asked \
         to decide"
    );
    assert_eq!(events[0].get::<String, _>("event_type"), "RefundOpened");
    assert_eq!(events[0].get::<Option<uuid::Uuid>, _>("cause_id"), Some(message_id));
    assert_eq!(events[0].get::<String, _>("user_type"), "EXTERNAL");

    let row = sqlx::query("SELECT status, attempts, error FROM inbound_messages WHERE message_id = $1")
        .bind(message_id)
        .fetch_one(&pool)
        .await
        .expect("row");
    assert_eq!(
        row.get::<String, _>("status"),
        "SUCCEEDED",
        "not RECEIVED-and-retrying: an aborted delivery re-enters the lane head-of-line and wedges \
         the money path until the attempts cap"
    );
    assert!(
        row.get::<Option<serde_json::Value>, _>("error").is_none(),
        "and nothing is written to the 90-day durable error column"
    );

    let published = rx.try_recv().expect("the post-commit fan-out must publish the recorded fact");
    assert_eq!(published.event_type, "RefundOpened");
    assert_eq!(published.stream_name, format!("Payment-{intent}"));
}

/// **ALL FIVE, THROUGH THE LANE.** One test per fact would assert the same one thing five times;
/// what matters is that no member of the set is left behind, so the set itself is the assertion —
/// and it is spelled out fact by fact, because a loop over a list nobody maintains is how the
/// missing five went unnoticed in the first place.
///
/// The birth goes first so the fold EXISTS and `already_records` answers from state rather than
/// from the birthless structural-equality fallback: that is the harder path, and it is the one
/// production takes.
#[tokio::test]
async fn every_newly_wired_payment_fact_records_through_the_lane() {
    use domain::generated::entities::{CheckoutSnapshot, CustomerContact, PaymentBreakdown};
    use domain::generated::events::{
        DomainEvent, PaymentCaptureFailed, PaymentIntentCreated, RefundApproved, RefundDenied,
    };
    use domain::generated::scalars::*;

    let Some(db) = crate::common::TestDb::acquire("fact_delivery_five_facts").await else { return };
    let pool = db.pool();
    let intent = "pi_780_five_facts";
    let order = uuid::Uuid::from_u128(0x7E71);
    let cart = uuid::Uuid::from_u128(0x7E72);
    let restaurant = uuid::Uuid::from_u128(0x7E51);
    let lane_actor = uuid::Uuid::from_u128(0x7E73);
    let z = eur(0);

    let birth = DomainEvent::PaymentIntentCreated(PaymentIntentCreated {
        payment_intent_id: PaymentIntentId(intent.into()),
        restaurant_id: RestaurantId(restaurant),
        customer_id: CustomerId(uuid::Uuid::nil()),
        amount: eur(1960),
        checkout: CheckoutSnapshot {
            order_id: OrderId(order),
            cart_id: CartId(cart),
            restaurant_id: RestaurantId(restaurant),
            customer_id: CustomerId(uuid::Uuid::nil()),
            mode: None,
            r#ref: None,
            customer_contact: CustomerContact {
                display_name: CustomerDisplayName("Johnny".into()),
                email: None,
                phone: PhoneNumber("+33612345678".into()),
            },
            service_type: ServiceType::DELIVERY,
            delivery_address: None,
            items: Vec::new(),
            total_amount: eur(1960),
            breakdown: PaymentBreakdown {
                articles: eur(1960),
                delivery: z.clone(),
                service_fee: z.clone(),
                total: eur(1960),
                restaurant_contribution: z.clone(),
                restaurant_payout: eur(1960),
                rider_payout: z.clone(),
                captain_net: z,
            },
            note: None,
            verdict: None,
            window_from: None,
            window_to: None,
            timezone: None,
            evaluated_at: None,
        },
    });
    let capture_failed = DomainEvent::PaymentCaptureFailed(PaymentCaptureFailed {
        payment_intent_id: PaymentIntentId(intent.into()),
        order_id: OrderId(order),
        restaurant_id: RestaurantId(restaurant),
        reason: CaptureFailureReason::CARD_DECLINED,
        detail: None,
    });
    let approved = DomainEvent::RefundApproved(RefundApproved {
        payment_intent_id: PaymentIntentId(intent.into()),
        order_id: OrderId(order),
        amount: eur(1960),
        reason: None,
    });
    let denied = DomainEvent::RefundDenied(RefundDenied {
        payment_intent_id: PaymentIntentId(intent.into()),
        order_id: OrderId(order),
        reason: "outside the claim window".into(),
    });

    let facts = [
        ("PaymentIntentCreated", birth),
        ("PaymentCaptureFailed", capture_failed),
        ("RefundOpened", refund_opened(intent, order, restaurant)),
        ("RefundApproved", approved),
        ("RefundDenied", denied),
    ];
    for (n, (message_type, event)) in facts.iter().enumerate() {
        enqueue_fact(
            &pool,
            0x7841 + n as u128,
            "Payment",
            lane_actor,
            0,
            message_type,
            staged(event.clone()),
        )
        .await;
    }

    let worker = MailboxWorker::new(
        pool.clone(),
        "w-five",
        "Payment",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(deps_over(&pool))),
    );
    worker.seed(10).await.expect("seed");
    worker.claim().await.expect("claim");
    assert_eq!(worker.drain().await.expect("drain"), 5, "all five delivered in one pass");

    let appended: Vec<String> = sqlx::query_scalar(
        "SELECT event_type FROM domain_events WHERE stream_name = $1 ORDER BY version",
    )
    .bind(format!("Payment-{intent}"))
    .fetch_all(&pool)
    .await
    .expect("events");
    assert_eq!(
        appended,
        vec![
            "PaymentIntentCreated",
            "PaymentCaptureFailed",
            "RefundOpened",
            "RefundApproved",
            "RefundDenied"
        ],
        "every one of the five must record, in delivery order -- each was refused by the untyped \
         stream lookup before PR #783's review"
    );

    let statuses: Vec<String> =
        sqlx::query_scalar("SELECT status FROM inbound_messages ORDER BY position")
            .fetch_all(&pool)
            .await
            .expect("verdicts");
    assert_eq!(statuses, vec!["SUCCEEDED"; 5], "and none of them aborted for retry");
}
