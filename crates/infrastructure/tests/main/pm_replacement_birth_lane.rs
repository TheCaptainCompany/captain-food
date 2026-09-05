//! **The REPLACEMENT-order birth on the Order lane** (#595, ADR-20260829-230418 chunk C2) —
//! against a real Postgres.
//!
//! The defect these tests exist to hold closed: `ReclamationProcess`'s REPLACEMENT arm called
//! `place_replacement_order` in-process from the polling `ProcessManagerRunner` and wrote
//! `Order-{id}` straight to the event store. Two consequences, and the second is the one that
//! costs a customer something:
//!
//! 1. the birth was not serialised against the Order's own writer, and its append was atomic with
//!    nothing at all — the runner owned no transaction;
//! 2. the `OrderAcceptanceTimedOut` reminder the spec declares for `(Order,
//!    PlaceReplacementOrder)` is (re)declared by a DELIVERY. With no delivery there was no clock,
//!    so a restaurant could go silent on a remake for a claim it had already agreed to and nothing
//!    would ever say so.
//!
//! What is asserted here, in the order it happens:
//!
//! * **flag ON** — the runner stages a COMMAND door row on the Order lane and appends NOTHING; the
//!   door row and the checkpoint advance share one transaction (`xmin`); the Order's own lane
//!   worker then runs the command, appends the birth, **and arms the acceptance clock**;
//! * **flag OFF** — the legacy in-process append, unchanged, with no lane row: the rollback path
//!   is proven, not assumed;
//! * **golden payload equality** — the `OrderPlaced` payload the laned path produces is
//!   byte-identical to the one the legacy path produces. This is the precondition for deleting the
//!   legacy arm in a later change (beck): "the routes agree" must be a measurement, not a reading
//!   of two code paths that call the same function today and might not tomorrow;
//! * **redelivery** — a re-reacted resolution collides on the frozen door identity: one door row,
//!   one birth, and the deadline does not move.
//!
//! Needs `DATABASE_URL`: since #474 a missing database FAILS this suite.

use std::sync::Arc;

use actor_runtime::{MailboxWorker, WorkerConfig};
use application::generated::services::IdentityService;
use infrastructure::generated::command_router::CommandDeps;
use infrastructure::mailbox::MailboxCommandHandler;
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, FailClosedPaymentGateway,
    PgCustomerRepository, PgEventStore, PgProspectionRepository, PgRestaurantRepository,
    PgAuthSubjectReservationRepository, PgSlugReservationRepository, ProcessManagerRunner, UnverifiedGbpOrderLinkProbe,
};
use sqlx::{PgPool, Row};

fn uid(n: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128(n)
}

const ORIGINAL_ORDER: u128 = 0x0_1D_0AD1;
const RESTAURANT: u128 = 0x0E57;
const CUSTOMER: u128 = 0xC057;
const CLAIM: u128 = 0xC1A1_4;

fn money(cents: i64) -> serde_json::Value {
    serde_json::json!({ "amountCents": cents, "currency": "EUR" })
}

/// The replacement order's id, derived the way the saga derives it. Not recomputed by hand here:
/// the derivation IS the idempotency key, so a test that hard-coded a uuid would keep passing
/// through a change to it — the one change that would mint a second replacement per claim.
fn replacement_order_id() -> uuid::Uuid {
    application::process_managers::reclamation::replacement_order_id_for(
        &domain::generated::scalars::ReclamationId(uid(CLAIM)),
    )
    .0
}

/// The ORIGINAL order's birth — the stream `place_replacement_order` folds for the items and
/// delivery details it copies. A `note` and a `ref` are present on purpose: the replacement must
/// carry the note over and must NOT carry the `ref` (HubRise's idempotent import key).
async fn seed_original_order(pool: &PgPool) {
    sqlx::query(
        "INSERT INTO domain_events \
         (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, \
          payload, metadata, occurred_at) \
         VALUES ($1, $2, 0, $3, 5, $4, NULL, 'OrderPlaced', $5, NULL, now())",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(format!("Order-{}", uid(ORIGINAL_ORDER)))
    .bind(uuid::Uuid::nil())
    .bind(uuid::Uuid::new_v4())
    .bind(serde_json::json!({
        "orderId": uid(ORIGINAL_ORDER),
        "ref": "CF-0042",
        "restaurantId": uid(RESTAURANT),
        "customerId": uid(CUSTOMER),
        "customerContact": { "displayName": "Léa", "phone": "+33612345678" },
        "serviceType": "DELIVERY",
        "deliveryAddress": {
            "line1": "12 rue Nationale",
            "postalCode": "37000",
            "city": "Tours",
            "country": "FR"
        },
        "items": [{
            "offerId": uid(0x0FFE),
            "name": "Margherita",
            "quantity": 2,
            "unitPrice": money(980),
            "lineTotal": money(1960)
        }],
        "totalAmount": money(2560),
        "breakdown": {
            "articles": money(1960),
            "delivery": money(400),
            "serviceFee": money(200),
            "total": money(2560),
            "restaurantContribution": money(160),
            "restaurantPayout": money(1800),
            "riderPayout": money(400),
            "captainNet": money(360)
        },
        "note": "sans oignons",
        "paymentIntentId": "pi_replacement_595"
    }))
    .execute(pool)
    .await
    .expect("seed the original order");
}

/// The trigger: a claim resolved as REPLACEMENT, on the Reclamation's own stream — exactly the row
/// the runner drains.
async fn seed_resolution(pool: &PgPool) {
    sqlx::query(
        "INSERT INTO domain_events \
         (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, \
          payload, metadata, occurred_at) \
         VALUES ($1, $2, 0, $3, 5, $4, NULL, 'ReclamationResolved', $5, NULL, now())",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(format!("Reclamation-{}", uid(CLAIM)))
    .bind(uuid::Uuid::nil())
    .bind(uuid::Uuid::new_v4())
    .bind(serde_json::json!({
        "reclamationId": uid(CLAIM),
        "orderId": uid(ORIGINAL_ORDER),
        "customerId": uid(CUSTOMER),
        "resolution": "REPLACEMENT"
    }))
    .execute(pool)
    .await
    .expect("seed the resolution");
}

/// The ports the Order lane's COMMAND route needs. The OrderPlaced route's gate stays at the SPEC
/// default — this suite turns exactly one knob, and it is not that one.
fn order_deps(pool: &PgPool) -> CommandDeps {
    CommandDeps {
        // #639 part C step 2c-i: the rider sign-in door's bridge + support route (not exercised here).
        riders: Arc::new(infrastructure::PgRiderRepository::new(pool.clone())),
        support_contact: None,
        run_rider_restriction_door: false,
        run_member_access_grant: false,
        store: Arc::new(PgEventStore::new(pool.clone())),
        restaurants: Arc::new(PgRestaurantRepository::new(pool.clone())),
        slugs: Arc::new(PgSlugReservationRepository::new(pool.clone())),
        auth_subjects: Arc::new(PgAuthSubjectReservationRepository::new(pool.clone())),
        ownership: Arc::new(FailClosedGoogleOwnershipVerifier),
        probe: Arc::new(UnverifiedGbpOrderLinkProbe),
        prospection: Arc::new(PgProspectionRepository::new(pool.clone())),
        catalogs: Arc::new(infrastructure::PgCatalogRepository::new(pool.clone())),
        auth: Arc::new(FailClosedIdentityService) as Arc<dyn IdentityService>,
        customers: Arc::new(PgCustomerRepository::new(pool.clone())),
        sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
        payments: Arc::new(FailClosedPaymentGateway),
        pm_state: Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone())),
        refund_state: Arc::new(infrastructure::persistence::PgRefundProcessState::new(
            pool.clone(),
        )),
        mailbox_requeue: Arc::new(
            infrastructure::persistence::mailbox_lanes::PgMailboxRequeue::new(pool.clone()),
        ),
        enforce_service_hours_guard: false,
        enforce_acceptance_timeout: false,
        route_gates: application::generated::process_managers::RouteGates {
            order_placed_to_order: true,
            place_replacement_order_to_order: false,
            // #807: routed `send:` steps -- OFF, this fixture exercises the birth routes.
            bind_cart_to_customer_to_cart: false,
            grant_customer_credit_to_customer_credit: false,
            mark_order_delivered_to_order: false,
        },
    }
}

/// An Order-lane worker with the reminder windows wired — a missing window aborts every delivery on
/// the lane by design, so the acceptance clock cannot be observed without them.
async fn order_worker(pool: &PgPool, id: &str) -> MailboxWorker {
    let windows = std::collections::HashMap::from([
        ("ORDER_ACCEPTANCE_TIMEOUT_SECONDS", std::time::Duration::from_secs(300)),
        ("ORDER_RETENTION_WINDOW_DAYS", std::time::Duration::from_secs(30 * 86_400)),
    ]);
    let w = MailboxWorker::new(
        pool.clone(),
        id,
        "Order",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(order_deps(pool)).with_reminder_windows(windows)),
    );
    w.seed(5).await.expect("seed order lanes");
    w.claim().await.expect("claim order lanes");
    w
}

async fn drain_all(worker: &MailboxWorker) -> u64 {
    let mut delivered = 0;
    for lane in worker.owned().await {
        delivered += worker.drain_lane(&lane).await.expect("lane drains clean");
    }
    delivered
}

/// The runner restricted to ReclamationProcess, with the #595 route ON or OFF.
///
/// #797: the OrderPlaced route's gate is held at `false` INDEPENDENTLY of `laned`. That the two
/// can now be set apart at all is the point of the chunk — before it, one boolean decided both.
fn runner(pool: &PgPool, laned: bool) -> ProcessManagerRunner {
    ProcessManagerRunner::new(pool.clone()).with_only("ReclamationProcess").with_route_gates(
        application::generated::process_managers::RouteGates {
            order_placed_to_order: false,
            place_replacement_order_to_order: laned,
            // #807: routed `send:` steps -- OFF, this fixture isolates the replacement birth route.
            bind_cart_to_customer_to_cart: false,
            grant_customer_credit_to_customer_credit: false,
            mark_order_delivered_to_order: false,
        },
    )
}

async fn birth_payloads(pool: &PgPool, order_id: uuid::Uuid) -> Vec<serde_json::Value> {
    sqlx::query(
        "SELECT payload FROM domain_events WHERE stream_name = $1 AND event_type = 'OrderPlaced' \
         ORDER BY version",
    )
    .bind(format!("Order-{order_id}"))
    .fetch_all(pool)
    .await
    .expect("the replacement stream")
    .into_iter()
    .map(|r| r.get::<serde_json::Value, _>("payload"))
    .collect()
}

async fn door_rows(pool: &PgPool) -> Vec<sqlx::postgres::PgRow> {
    sqlx::query(
        "SELECT message_id, kind, actor_type, actor_id, message_type, payload, source, \
                external_id, cause_id, correlation_id, user_type, status \
         FROM inbound_messages WHERE message_type = 'PlaceReplacementOrder'",
    )
    .fetch_all(pool)
    .await
    .expect("query the replacement door")
}

/// **The chunk, end to end, flag ON.** The runner decides and ENQUEUES; the Order's own lane
/// worker appends and, in the same delivery, arms the acceptance clock.
#[tokio::test]
async fn routed_replacement_is_born_by_the_order_lane_and_arms_the_acceptance_clock() {
    let Some(db) = crate::common::TestDb::acquire("pm_replacement_birth_lane").await else {
        return;
    };
    let pool = db.pool();
    seed_original_order(&pool).await;
    seed_resolution(&pool).await;
    let replacement = replacement_order_id();

    runner(&pool, true).run_once().await.expect("the reclamation group drains clean");

    // --- (a) the saga DECIDED and appended nothing ------------------------------------------------
    assert!(
        birth_payloads(&pool, replacement).await.is_empty(),
        "ReclamationProcess must NOT append OrderPlaced to the replacement Order's stream: being \
         the birth AUTHORITY licenses the DECISION, never the APPEND (ADR-20260816-040239). This \
         is the assertion that goes red if the route reverts to the in-process call."
    );

    // --- (b) one COMMAND door row, on the replacement Order's own lane ---------------------------
    let doors = door_rows(&pool).await;
    assert_eq!(doors.len(), 1, "exactly ONE replacement door row per resolved claim");
    let door = &doors[0];
    assert_eq!(
        door.get::<String, _>("kind"),
        "COMMAND",
        "PlaceReplacementOrder is a REQUEST the Order may refuse, not a fact already decided — the \
         COMMAND door is what gives a rejection a REJECTED verdict on a supervisable row, and what \
         makes the delivery declare the (Order, PlaceReplacementOrder) reminder"
    );
    assert_eq!(door.get::<String, _>("actor_type"), "Order");
    assert_eq!(
        door.get::<uuid::Uuid, _>("actor_id"),
        replacement,
        "the lane is the REPLACEMENT order's — the aggregate being born is the writer to serialise"
    );
    let source: String = door.get::<Option<String>, _>("source").expect("a door row has a source");
    let external_id: String =
        door.get::<Option<String>, _>("external_id").expect("a door row has an external_id");
    assert_eq!(source, "pm:ReclamationProcess:PlaceReplacementOrder");
    assert_eq!(
        external_id,
        replacement.to_string(),
        "external_id MUST be the TARGET aggregate's id — deriving it from the trigger would mint a \
         fresh identity on every re-reaction and place a second replacement per claim"
    );
    assert_eq!(
        door.get::<uuid::Uuid, _>("message_id"),
        actor_client::inbound_message_id(&source, &external_id),
        "the door identity is UUIDv5(source:external_id) — FROZEN"
    );

    // --- (c) the enqueue and the checkpoint advance are ONE transaction --------------------------
    // Asserted about the COMMIT, not about row counts: Postgres exposes the inserting transaction
    // as `xmin`, so two rows written by the same transaction share it. If the flush ever moves off
    // the leg's `&mut Transaction` onto a pool handle, these diverge — and no count of rows could
    // see that.
    let door_xmin: i64 =
        sqlx::query_scalar("SELECT xmin::text::bigint FROM inbound_messages WHERE message_id = $1")
            .bind(door.get::<uuid::Uuid, _>("message_id"))
            .fetch_one(&pool)
            .await
            .expect("the door row's inserting transaction");
    let checkpoint_xmin: i64 = sqlx::query_scalar(
        "SELECT xmin::text::bigint FROM projection_checkpoint WHERE projector = \
         'pm:ReclamationProcess'",
    )
    .fetch_one(&pool)
    .await
    .expect("the checkpoint advanced");
    assert_eq!(
        door_xmin, checkpoint_xmin,
        "the door row and the checkpoint advance were written by the SAME transaction (#595 \
         `commit_leg`). Split them and the two orders both fail: checkpoint-then-enqueue can lose \
         the replacement entirely on a crash — the position spent, nobody enqueued"
    );

    // --- (d) the Order's own lane worker appends the birth ---------------------------------------
    let worker = order_worker(&pool, "w-REPL595").await;
    let t0 = chrono::Utc::now();
    assert_eq!(drain_all(&worker).await, 1, "the Order lane delivers the replacement command");
    let t1 = chrono::Utc::now();

    let door_id: uuid::Uuid = door.get("message_id");
    let (status, error): (String, Option<serde_json::Value>) =
        sqlx::query_as("SELECT status, error FROM inbound_messages WHERE message_id = $1")
            .bind(door_id)
            .fetch_one(&pool)
            .await
            .expect("the door row's verdict");
    assert_eq!(
        (status.as_str(), error),
        ("SUCCEEDED", None),
        "the command was accepted by the Order aggregate"
    );

    let births = birth_payloads(&pool, replacement).await;
    assert_eq!(births.len(), 1, "one birth on the replacement stream, appended by the Order's lane");
    assert_eq!(
        births[0].get("replacementOf").and_then(|v| v.as_str()),
        Some(uid(ORIGINAL_ORDER).to_string()).as_deref(),
        "the replacement is linked to the order it replaces"
    );
    assert_eq!(
        births[0].get("totalAmount").and_then(|m| m.get("amountCents")).and_then(|c| c.as_i64()),
        Some(0),
        "a replacement is NO-CHARGE — no money moves and there is no Stripe intent"
    );
    assert!(
        births[0].get("paymentIntentId").is_none_or(|v| v.is_null()),
        "a replacement carries no paymentIntentId"
    );

    // --- (e) THE POINT OF THE ISSUE: the acceptance clock ARMS -----------------------------------
    // Bracket the drain: `apply_schedules_in_tx` computes `now() + window` INSIDE the delivery
    // transaction, so the deadline must land in `[t0 + window, t1 + window]` — deterministic under
    // any CI pause, where a fixed tolerance is a flake waiting for a slow runner.
    let deadlines = sqlx::query(
        "SELECT message_id, status, scheduled_at, cause_id FROM inbound_messages \
         WHERE actor_id = $1 AND message_type = 'OrderAcceptanceTimedOut'",
    )
    .bind(replacement)
    .fetch_all(&pool)
    .await
    .expect("query the deadline");
    assert_eq!(
        deadlines.len(),
        1,
        "a REPLACEMENT order gets an acceptance clock. Before #595 there were ZERO rows here: the \
         reminder is declared on the DELIVERY of (Order, PlaceReplacementOrder), and the \
         in-process call performed no delivery — so a restaurant could go silent on a remake it \
         had already agreed to and nothing would ever say so"
    );
    assert_eq!(deadlines[0].get::<String, _>("status"), "SCHEDULED");
    assert_eq!(
        deadlines[0].get::<Option<uuid::Uuid>, _>("cause_id"),
        Some(door_id),
        "the clock is armed BY the replacement delivery, not by some other route"
    );
    let scheduled_at: chrono::DateTime<chrono::Utc> = deadlines[0].get("scheduled_at");
    let window = chrono::Duration::seconds(300);
    assert!(
        (t0 + window..=t1 + window).contains(&scheduled_at),
        "due one ORDER_ACCEPTANCE_TIMEOUT_SECONDS window after the birth delivery: expected within \
         [{}, {}], got {scheduled_at}",
        t0 + window,
        t1 + window
    );
}

/// **The rollback path, proven rather than assumed.** With the gate OFF the legacy in-process
/// append runs unchanged and NO lane row exists — which is what makes "rollback is a config flip,
/// not a redeploy" a fact about this build (farley, gate-then-stabilize).
#[tokio::test]
async fn unrouted_replacement_keeps_the_legacy_in_process_append_and_enqueues_nothing() {
    let Some(db) = crate::common::TestDb::acquire("pm_replacement_birth_lane").await else {
        return;
    };
    let pool = db.pool();
    seed_original_order(&pool).await;
    seed_resolution(&pool).await;
    let replacement = replacement_order_id();

    runner(&pool, false).run_once().await.expect("the reclamation group drains clean");

    assert_eq!(
        birth_payloads(&pool, replacement).await.len(),
        1,
        "gated OFF, the saga still appends the replacement birth itself — byte for byte the \
         behaviour that shipped"
    );
    assert!(
        door_rows(&pool).await.is_empty(),
        "and no lane row is written: the OFF path must not half-route"
    );
    let deadlines: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM inbound_messages WHERE actor_id = $1 AND message_type = \
         'OrderAcceptanceTimedOut'",
    )
    .bind(replacement)
    .fetch_one(&pool)
    .await
    .expect("count deadlines");
    assert_eq!(
        deadlines, 0,
        "and the OFF path still arms no clock — that gap is what the flip closes, and stating it \
         here keeps the flip's value measured rather than argued"
    );
}

/// **Golden payload equality** (beck) — the DELETION precondition for the legacy arm.
///
/// Run the same claim through both routes on two clean databases and compare the recorded
/// `OrderPlaced` payloads byte for byte. The two paths call the same handler TODAY; the point of
/// measuring is that nothing enforces they still will when the legacy arm is deleted, and a
/// silently diverged payload is a different order for the same claim.
#[tokio::test]
async fn the_two_routes_produce_a_byte_identical_birth_payload() {
    let replacement = replacement_order_id();

    let laned = {
        let Some(db) = crate::common::TestDb::acquire("pm_replacement_birth_lane").await else {
            return;
        };
        let pool = db.pool();
        seed_original_order(&pool).await;
        seed_resolution(&pool).await;
        runner(&pool, true).run_once().await.expect("routed run");
        let worker = order_worker(&pool, "w-REPL595g").await;
        assert_eq!(drain_all(&worker).await, 1, "the Order lane delivers");
        birth_payloads(&pool, replacement).await
    };

    let legacy = {
        let Some(db) = crate::common::TestDb::acquire("pm_replacement_birth_lane").await else {
            return;
        };
        let pool = db.pool();
        seed_original_order(&pool).await;
        seed_resolution(&pool).await;
        runner(&pool, false).run_once().await.expect("legacy run");
        birth_payloads(&pool, replacement).await
    };

    assert_eq!(laned.len(), 1);
    assert_eq!(legacy.len(), 1);
    assert_eq!(
        serde_json::to_string(&laned[0]).expect("laned payload serializes"),
        serde_json::to_string(&legacy[0]).expect("legacy payload serializes"),
        "the routed birth's PAYLOAD must be byte-identical to the foreign-stream append it \
         replaces. Only the ENVELOPE legitimately differs (user_id/user_type and cause_id now come \
         from the mailbox row) — and stored rows are never backfilled, so a payload difference \
         would be permanent"
    );
}

/// **Redelivery.** Rewind the checkpoint and drain again: the frozen door identity collides on the
/// primary key, so a re-reacted resolution mints no second replacement — and the acceptance
/// deadline does not move (`reschedule: keep`), which a `DO UPDATE` would silently break.
#[tokio::test]
async fn a_re_reacted_resolution_dedups_the_replacement_at_the_door() {
    let Some(db) = crate::common::TestDb::acquire("pm_replacement_birth_lane").await else {
        return;
    };
    let pool = db.pool();
    seed_original_order(&pool).await;
    seed_resolution(&pool).await;
    let replacement = replacement_order_id();

    runner(&pool, true).run_once().await.expect("first run");
    let worker = order_worker(&pool, "w-REPL595r").await;
    assert_eq!(drain_all(&worker).await, 1, "the birth delivers");
    let first_deadline: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT scheduled_at FROM inbound_messages WHERE actor_id = $1 AND message_type = \
         'OrderAcceptanceTimedOut'",
    )
    .bind(replacement)
    .fetch_one(&pool)
    .await
    .expect("the armed deadline");

    // Rewind the group's checkpoint: the same resolution is reacted to a second time, which is
    // exactly what a crash between the leg and its commit produces.
    sqlx::query("UPDATE projection_checkpoint SET position = 0 WHERE projector = $1")
        .bind("pm:ReclamationProcess")
        .execute(&pool)
        .await
        .expect("rewind");
    runner(&pool, true).run_once().await.expect("second run — the dedup must be a SUCCESS");

    assert_eq!(door_rows(&pool).await.len(), 1, "ONE door row: the re-reaction collided on the pk");
    worker.claim().await.expect("re-claim");
    drain_all(&worker).await;
    assert_eq!(
        birth_payloads(&pool, replacement).await.len(),
        1,
        "ONE replacement per resolved claim, never two"
    );
    let deadlines = sqlx::query(
        "SELECT scheduled_at FROM inbound_messages WHERE actor_id = $1 AND message_type = \
         'OrderAcceptanceTimedOut'",
    )
    .bind(replacement)
    .fetch_all(&pool)
    .await
    .expect("the deadline");
    assert_eq!(deadlines.len(), 1, "exactly ONE pending occurrence per (actor, purpose)");
    assert_eq!(
        deadlines[0].get::<chrono::DateTime<chrono::Utc>, _>("scheduled_at"),
        first_deadline,
        "and the deadline did NOT move — `reschedule: keep`, a deadline a redelivered birth must \
         never push out"
    );
}
