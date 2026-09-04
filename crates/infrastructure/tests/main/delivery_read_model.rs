//! Integration test for the DeliveryJob read-side slice: lifecycle events in `domain_events` →
//! `View_DeliveryJob` (the generated fold VIEW, projection-on-read — no worker involved, ADR-0039) →
//! read repository. Needs a real Postgres: set `DATABASE_URL` (see restaurant_projection.rs for a
//! throwaway docker one-liner). Without it the test SKIPS (prints and returns) so `cargo test` stays
//! green offline.
//!
//! One test function on purpose: the tables are shared state, so the scenario must run sequentially.

use application::queries::DeliveryReadRepository as _;
use chrono::{Duration, Utc};
use domain::generated::scalars::{DeliveryProvider, DeliveryStatus, OrderId, RestaurantId, RiderId};
use infrastructure::{PgDeliveryRepository, ProjectionWorker};
use sqlx::PgPool;

fn money(cents: i64) -> serde_json::Value {
    serde_json::json!({ "amountCents": cents, "currency": "EUR" })
}

async fn append_event(
    pool: &PgPool,
    stream_name: &str,
    version: i32,
    event_type: &str,
    payload: serde_json::Value,
    occurred_at: chrono::DateTime<Utc>,
) {
    sqlx::query(
        "INSERT INTO domain_events \
         (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, payload, metadata, occurred_at) \
         VALUES ($1, $2, $3, $4, 5, $5, NULL, $6, $7, NULL, $8)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(stream_name)
    .bind(version)
    .bind(uuid::Uuid::nil()) // acting user (ADMIN=5 above) — envelope metadata, ADR-0041
    .bind(uuid::Uuid::new_v4())
    .bind(event_type)
    .bind(payload)
    .bind(occurred_at)
    .execute(pool)
    .await
    .expect("append event");
}

fn address(line1: &str) -> serde_json::Value {
    serde_json::json!({ "line1": line1, "city": "Tours", "postalCode": "37000", "country": "FR" })
}

#[tokio::test]
async fn delivery_lifecycle_events_serve_the_three_read_queries() {
    let Some(db) = crate::common::TestDb::acquire("delivery_read_model").await else { return };
    let pool = db.pool();

    let (r1, r2) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let (o1, o2, o3) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let (j1, j2, j3) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let rider = uuid::Uuid::new_v4();
    let t0 = Utc::now() - Duration::minutes(30);

    // j1 (restaurant r1): requested → accepted by an independent rider → picked up.
    let s1 = format!("DeliveryJob-{j1}");
    append_event(
        &pool,
        &s1,
        1,
        "DeliveryRequested",
        serde_json::json!({
            "deliveryJobId": j1, "orderId": o1, "restaurantId": r1,
            "pickup": address("1 rue de la Paix"), "dropoff": address("2 avenue Grammont"),
        }),
        t0,
    )
    .await;
    append_event(
        &pool,
        &s1,
        2,
        "DeliveryAcceptedByRider",
        serde_json::json!({ "deliveryJobId": j1, "orderId": o1, "riderId": rider }),
        t0 + Duration::minutes(2),
    )
    .await;
    append_event(
        &pool,
        &s1,
        3,
        "DeliveryPickedUp",
        serde_json::json!({ "deliveryJobId": j1, "orderId": o1, "riderId": rider }),
        t0 + Duration::minutes(10),
    )
    .await;

    // j2 (restaurant r1): requested only — the PENDING available pool.
    append_event(
        &pool,
        &format!("DeliveryJob-{j2}"),
        1,
        "DeliveryRequested",
        serde_json::json!({
            "deliveryJobId": j2, "orderId": o2, "restaurantId": r1,
            "pickup": address("1 rue de la Paix"), "dropoff": address("3 rue Colbert"),
        }),
        t0 + Duration::minutes(5),
    )
    .await;

    // j3 (restaurant r2): requested → accepted by the PARTNER (courier + ETAs) → reported DELIVERED.
    let s3 = format!("DeliveryJob-{j3}");
    append_event(
        &pool,
        &s3,
        1,
        "DeliveryRequested",
        serde_json::json!({
            "deliveryJobId": j3, "orderId": o3, "restaurantId": r2,
            "pickup": address("9 place Plumereau"), "dropoff": address("4 rue Nationale"),
        }),
        t0 + Duration::minutes(1),
    )
    .await;
    append_event(
        &pool,
        &s3,
        2,
        "DeliveryAcceptedByPartner",
        serde_json::json!({
            "deliveryJobId": j3, "partnerRef": "AV-42",
            "courier": { "displayName": "Marc", "phone": "+33600000000" },
            "estimatedPickupAt": (t0 + Duration::minutes(8)).to_rfc3339(),
            "estimatedDropoffAt": (t0 + Duration::minutes(20)).to_rfc3339(),
        }),
        t0 + Duration::minutes(3),
    )
    .await;
    append_event(
        &pool,
        &s3,
        3,
        "DeliveryStatusUpdated",
        serde_json::json!({ "deliveryJobId": j3, "orderId": o3, "status": "DELIVERED" }),
        t0 + Duration::minutes(22),
    )
    .await;

    let repo = PgDeliveryRepository::new(pool.clone());

    // `delivery` (by order): the independent-rider job folded to PICKED_UP.
    let job = repo
        .by_order(OrderId(o1))
        .await
        .expect("by_order")
        .expect("j1 visible through the view");
    assert_eq!(job.delivery_job_id.0, j1);
    assert_eq!(job.restaurant_id.0, r1);
    assert_eq!(job.status, DeliveryStatus::PICKED_UP);
    assert_eq!(job.provider, Some(DeliveryProvider::INDEPENDENT));
    assert_eq!(job.rider_id, Some(RiderId(rider)));
    assert!(job.picked_up_at.is_some(), "picked_up_at set by DeliveryPickedUp");
    assert!(job.delivered_at.is_none());
    assert_eq!(job.pickup_address["city"], "Tours");

    // Unknown order → None.
    assert!(repo.by_order(OrderId(uuid::Uuid::new_v4())).await.expect("by_order").is_none());

    // `myDeliveries`: the rider's assigned job + the available PENDING pool (not the partner's job).
    let mine = repo.for_rider(RiderId(rider), None).await.expect("for_rider");
    let mut ids: Vec<uuid::Uuid> = mine.iter().map(|j| j.delivery_job_id.0).collect();
    ids.sort();
    let mut expected = vec![j1, j2];
    expected.sort();
    assert_eq!(ids, expected, "assigned + available, partner job excluded");
    // Status filter narrows the union: PENDING → only the available pool.
    let available = repo.for_rider(RiderId(rider), Some(DeliveryStatus::PENDING)).await.expect("for_rider");
    assert_eq!(available.len(), 1);
    assert_eq!(available[0].delivery_job_id.0, j2);
    assert_eq!(available[0].provider, None);

    // `restaurantDeliveries`: r1's board (newest requested first), r2's filtered to DELIVERED.
    let board = repo.by_restaurant(RestaurantId(r1), None).await.expect("by_restaurant");
    assert_eq!(
        board.iter().map(|j| j.delivery_job_id.0).collect::<Vec<_>>(),
        vec![j2, j1],
        "newest requested first"
    );
    let delivered = repo
        .by_restaurant(RestaurantId(r2), Some(DeliveryStatus::DELIVERED))
        .await
        .expect("by_restaurant");
    assert_eq!(delivered.len(), 1);
    let j = &delivered[0];
    assert_eq!(j.delivery_job_id.0, j3);
    assert_eq!(j.order_id.0, o3);
    assert_eq!(j.status, DeliveryStatus::DELIVERED);
    assert_eq!(j.provider, Some(DeliveryProvider::PARTNER));
    assert_eq!(j.partner_ref.as_ref().map(|r| r.0.as_str()), Some("AV-42"));
    assert_eq!(j.courier.as_ref().and_then(|c| c["displayName"].as_str()), Some("Marc"));
    assert!(j.estimated_pickup_at.is_some() && j.estimated_dropoff_at.is_some());
    assert!(j.delivered_at.is_some(), "delivered_at set by DeliveryStatusUpdated=DELIVERED");
    assert!(j.rider_id.is_none(), "partner delivery carries no independent rider id");
}

/// #639 part C step 3-i (ADR-20260904-015903 §Decision 3/4): the issue door tells the restaurant
/// THROUGH THE READ MODEL — `View_DeliveryJob.open_issue_kind` folds `DeliveryIssueReported.kind`
/// and `DeliveryIssueResolved` clears it (the `derive:` grammar's explicit `null`). The view is a
/// projection-on-read (no projector to run): appending the facts is the whole write side.
///
/// RED with no runtime change (#861's stand-in): the column does not exist in the APPLIED DDL,
/// because `views.generated.sql` is applied by nothing — the test bed replays the real migration
/// chain, so a regenerated view that never became a migration leaves this red. It stays red when
/// the column is declared but the hand-written `CREATE OR REPLACE VIEW` migration (and its
/// `include_str!` chain entry) is missing.
#[tokio::test]
async fn a_reported_issue_shows_on_the_board_and_a_resolution_clears_it() {
    let Some(db) = crate::common::TestDb::acquire("delivery_read_model_issue").await else { return };
    let pool = db.pool();

    let (restaurant, order, job, rider) =
        (uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let t0 = Utc::now() - Duration::minutes(20);
    let stream = format!("DeliveryJob-{job}");

    append_event(
        &pool,
        &stream,
        1,
        "DeliveryRequested",
        serde_json::json!({
            "deliveryJobId": job, "orderId": order, "restaurantId": restaurant,
            "pickup": address("1 rue de la Paix"), "dropoff": address("2 avenue Grammont"),
        }),
        t0,
    )
    .await;
    append_event(
        &pool,
        &stream,
        2,
        "DeliveryAcceptedByRider",
        serde_json::json!({ "deliveryJobId": job, "orderId": order, "riderId": rider }),
        t0 + Duration::minutes(2),
    )
    .await;
    append_event(
        &pool,
        &stream,
        3,
        "DeliveryIssueReported",
        serde_json::json!({ "deliveryJobId": job, "riderId": rider, "kind": "CUSTOMER_UNREACHABLE" }),
        t0 + Duration::minutes(12),
    )
    .await;

    let open_issue = |pool: PgPool| async move {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT open_issue_kind FROM view_deliveryjob WHERE delivery_job_id = $1",
        )
        .bind(job)
        .fetch_one(&pool)
        .await
        .expect("View_DeliveryJob carries open_issue_kind (the 3-i view migration)")
    };

    assert_eq!(
        open_issue(pool.clone()).await.as_deref(),
        Some("CUSTOMER_UNREACHABLE"),
        "the board must read the reported kind — a report nobody is told about is §7.2 again"
    );

    // The restaurant acknowledges it: the open issue clears (the fold's explicit `null` arm).
    append_event(
        &pool,
        &stream,
        4,
        "DeliveryIssueResolved",
        serde_json::json!({ "deliveryJobId": job, "resolution": "REASSIGNED" }),
        t0 + Duration::minutes(15),
    )
    .await;
    assert_eq!(
        open_issue(pool.clone()).await,
        None,
        "DeliveryIssueResolved must clear open_issue_kind (derive: null → THEN NULL)"
    );

    // And the rest of the row is untouched by the issue facts: status stays ASSIGNED, the rider
    // stays — 3-i never moves status (rules.yaml#/DeliveryIssueLifecycle).
    let repo = PgDeliveryRepository::new(pool.clone());
    let row = repo.by_order(OrderId(order)).await.expect("by_order").expect("row");
    assert_eq!(row.status, DeliveryStatus::ASSIGNED);
    assert_eq!(row.rider_id, Some(RiderId(rider)));
}

/// #639 part C step 3-ii (ADR-20260904-015903 §1-3): a handback moves BOTH read models in the same
/// slice — `View_DeliveryJob` (projection-on-read, no worker) AND `OrderTracking` (materialized,
/// `ProjectionWorker`, hand-written `Compute` hook). RED with no runtime change until the migration
/// lands: `food_location`/`handed_back_at` do not exist in the APPLIED DDL.
#[tokio::test]
async fn a_handed_back_job_reappears_pending_on_the_board_and_the_customers_mirror() {
    let Some(db) = crate::common::TestDb::acquire("delivery_read_model_handback").await else { return };
    let pool = db.pool();

    let (restaurant, order, job, rider1, rider2) = (
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
    );
    let t0 = Utc::now() - Duration::minutes(20);
    let stream = format!("DeliveryJob-{job}");
    let order_stream = format!("Order-{order}");

    // The customer's order, so an OrderTracking mirror row exists to assert against.
    append_event(
        &pool,
        &order_stream,
        1,
        "OrderPlaced",
        serde_json::json!({
            "orderId": order, "ref": "CF-0639", "restaurantId": restaurant,
            "customerId": uuid::Uuid::new_v4(),
            "customerContact": { "displayName": "Léa", "phone": "+33612345678" },
            "serviceType": "DELIVERY",
            "items": [{ "offerId": uuid::Uuid::new_v4(), "name": "Margherita", "quantity": 1, "unitPrice": money(980), "lineTotal": money(980) }],
            "totalAmount": money(1580),
            "breakdown": {
                "articles": money(980), "delivery": money(400), "serviceFee": money(200),
                "total": money(1580), "restaurantContribution": money(160),
                "restaurantPayout": money(820), "riderPayout": money(400), "captainNet": money(360)
            },
            "paymentIntentId": "pi_639",
        }),
        t0,
    )
    .await;

    append_event(
        &pool,
        &stream,
        1,
        "DeliveryRequested",
        serde_json::json!({
            "deliveryJobId": job, "orderId": order, "restaurantId": restaurant,
            "pickup": address("1 rue de la Paix"), "dropoff": address("2 avenue Grammont"),
        }),
        t0,
    )
    .await;
    append_event(
        &pool,
        &stream,
        2,
        "DeliveryAcceptedByRider",
        serde_json::json!({ "deliveryJobId": job, "orderId": order, "riderId": rider1 }),
        t0 + Duration::minutes(2),
    )
    .await;
    append_event(
        &pool,
        &stream,
        3,
        "DeliveryPickedUp",
        serde_json::json!({ "deliveryJobId": job, "orderId": order, "riderId": rider1 }),
        t0 + Duration::minutes(5),
    )
    .await;
    append_event(
        &pool,
        &stream,
        4,
        "DeliveryHandedBackByRider",
        serde_json::json!({ "deliveryJobId": job, "orderId": order, "riderId": rider1, "foodLocation": "RETURNED_TO_RESTAURANT" }),
        t0 + Duration::minutes(9),
    )
    .await;

    let repo = PgDeliveryRepository::new(pool.clone());
    let row = repo.by_order(OrderId(order)).await.expect("by_order").expect("row exists");
    assert_eq!(row.status, DeliveryStatus::PENDING, "PENDING unless WITH_RIDER — this is RETURNED_TO_RESTAURANT");
    assert_eq!(row.rider_id, None, "the fold-reset proof: rider_id clears on a handback");
    assert_eq!(row.provider, None, "provider clears too — the job is unassigned again");
    assert_eq!(
        row.food_location.map(|f| format!("{f:?}")).as_deref(),
        Some("RETURNED_TO_RESTAURANT"),
        "the custody fact itself"
    );
    assert!(row.handed_back_at.is_some());

    // The old rider no longer holds it (ASSIGNED filter excludes it); a DIFFERENT rider sees it in
    // the available pool (PENDING filter includes it) — a second courier CAN pick it up.
    let old_riders_assigned = repo.for_rider(RiderId(rider1), Some(DeliveryStatus::ASSIGNED)).await.expect("for_rider");
    assert!(
        !old_riders_assigned.iter().any(|j| j.delivery_job_id.0 == job),
        "the old rider's ASSIGNED view must not list a job it handed back"
    );
    let new_riders_pool = repo.for_rider(RiderId(rider2), Some(DeliveryStatus::PENDING)).await.expect("for_rider");
    assert!(
        new_riders_pool.iter().any(|j| j.delivery_job_id.0 == job),
        "a different rider's available pool must offer the re-offered job"
    );

    // The customer's OrderTracking mirror (application-layer projector, hand-written Compute hook —
    // NOT the derive: grammar, since this column is Complex-classified, `OrderTrackingCompute::
    // delivery_status`/`courier`): delivery_status PENDING, courier reset to null.
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (handback slice)");
    let (delivery_status, courier): (Option<String>, Option<serde_json::Value>) =
        sqlx::query_as("SELECT delivery_status, courier FROM ordertracking WHERE order_id = $1")
            .bind(order)
            .fetch_one(&pool)
            .await
            .expect("order tracking row");
    assert_eq!(delivery_status.as_deref(), Some("PENDING"), "the customer's mirror moves too");
    assert!(courier.is_none(), "courier resets — the job is unassigned again");

    // The WITH_RIDER twin: from PICKED_UP, WITH_RIDER fails the job closed rather than re-offer it.
    let job2 = uuid::Uuid::new_v4();
    let stream2 = format!("DeliveryJob-{job2}");
    let order2 = uuid::Uuid::new_v4();
    append_event(
        &pool,
        &stream2,
        1,
        "DeliveryRequested",
        serde_json::json!({
            "deliveryJobId": job2, "orderId": order2, "restaurantId": restaurant,
            "pickup": address("1 rue de la Paix"), "dropoff": address("5 rue Nationale"),
        }),
        t0,
    )
    .await;
    append_event(
        &pool,
        &stream2,
        2,
        "DeliveryAcceptedByRider",
        serde_json::json!({ "deliveryJobId": job2, "orderId": order2, "riderId": rider1 }),
        t0 + Duration::minutes(2),
    )
    .await;
    append_event(
        &pool,
        &stream2,
        3,
        "DeliveryPickedUp",
        serde_json::json!({ "deliveryJobId": job2, "orderId": order2, "riderId": rider1 }),
        t0 + Duration::minutes(5),
    )
    .await;
    append_event(
        &pool,
        &stream2,
        4,
        "DeliveryHandedBackByRider",
        serde_json::json!({ "deliveryJobId": job2, "orderId": order2, "riderId": rider1, "foodLocation": "WITH_RIDER" }),
        t0 + Duration::minutes(9),
    )
    .await;
    let row2 = repo.by_order(OrderId(order2)).await.expect("by_order").expect("row exists");
    assert_eq!(row2.status, DeliveryStatus::FAILED, "WITH_RIDER fails closed — never re-offered while the food is still in a restricted rider's bag");
    assert_eq!(row2.rider_id, None);
    assert_eq!(
        row2.food_location.map(|f| format!("{f:?}")).as_deref(),
        Some("WITH_RIDER")
    );
}
