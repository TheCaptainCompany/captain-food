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
use infrastructure::PgDeliveryRepository;
use sqlx::PgPool;

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
        serde_json::json!({ "deliveryJobId": j1, "riderId": rider }),
        t0 + Duration::minutes(2),
    )
    .await;
    append_event(
        &pool,
        &s1,
        3,
        "DeliveryPickedUp",
        serde_json::json!({ "deliveryJobId": j1, "riderId": rider }),
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
        serde_json::json!({ "deliveryJobId": j3, "status": "DELIVERED" }),
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
