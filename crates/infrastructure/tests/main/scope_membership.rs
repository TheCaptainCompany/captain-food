//! Integration tests for the read-side per-instance authorization slice (#144,
//! PROP-20260725-185140): events → ScopeMembership projector group → `scopemembership` rows →
//! the SCOPED `PgOrderRepository`. Needs a real Postgres (`DATABASE_URL`); SKIPS offline like the
//! rest of this binary — the authorization done-bar therefore requires a run WITH the database
//! (`DB_TESTS_REQUIRED=1`), a green without it is a skip wearing a pass.

use application::queries::{OrderFilter, OrderReadRepository as _, ReadScope};
use domain::generated::scalars::{CustomerId, OrderId, RestaurantId, ScopeType, UserType};
use infrastructure::persistence::scope_membership_store;
use infrastructure::{PgOrderRepository, ProjectionWorker};
use sqlx::PgPool;

async fn append_event(
    pool: &PgPool,
    stream_name: &str,
    version: i32,
    event_type: &str,
    payload: serde_json::Value,
) {
    sqlx::query(
        "INSERT INTO domain_events \
         (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, payload, metadata, occurred_at) \
         VALUES ($1, $2, $3, $4, 5, $5, NULL, $6, $7, NULL, now())",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(stream_name)
    .bind(version)
    .bind(uuid::Uuid::nil())
    .bind(uuid::Uuid::new_v4())
    .bind(event_type)
    .bind(payload)
    .execute(pool)
    .await
    .expect("append event");
}

fn money(cents: i64) -> serde_json::Value {
    serde_json::json!({ "amountCents": cents, "currency": "EUR" })
}

fn address() -> serde_json::Value {
    serde_json::json!({ "line1": "1 rue Nationale", "postalCode": "37000", "city": "Tours", "country": "FR" })
}

fn order_placed_payload(order: uuid::Uuid, restaurant: uuid::Uuid, customer: uuid::Uuid) -> serde_json::Value {
    serde_json::json!({
        "orderId": order,
        "restaurantId": restaurant,
        "customerId": customer,
        "customerContact": { "displayName": "Léa", "phone": "+33612345678" },
        "serviceType": "DELIVERY",
        "items": [{
            "offerId": uuid::Uuid::new_v4(),
            "name": "Margherita",
            "quantity": 1,
            "unitPrice": money(980),
            "lineTotal": money(980)
        }],
        "totalAmount": money(980),
        "breakdown": {
            "articles": money(980), "delivery": money(0), "serviceFee": money(0),
            "total": money(980), "restaurantContribution": money(0),
            "restaurantPayout": money(900), "riderPayout": money(0), "captainNet": money(80)
        },
        "paymentIntentId": "pi_test_scope"
    })
}

/// THE vulnerability test, at the seam where enforcement lives: the same `ordertracking` row is
/// visible to its OWN customer, invisible (empty list / `None`, never an error) to a STRANGER
/// customer and to PUBLIC, and visible to ADMIN. Before #144 the stranger's no-filter `list`
/// returned the entire table.
///
/// Seen RED by deleting the EXISTS clause from `PgOrderRepository::list` (stranger list came back
/// with 1 row: `strangers see no orders` failed) and by deleting the membership check from `by_id`
/// (`stranger by_id resolves None` failed) — both recorded in the PR.
#[tokio::test]
async fn order_reads_are_scoped_to_the_membership_index() {
    let Some(db) = crate::common::TestDb::acquire("scope_membership_reads").await else { return };
    let pool = db.pool();

    let order_id = uuid::Uuid::new_v4();
    let restaurant_id = uuid::Uuid::new_v4();
    let customer_id = uuid::Uuid::new_v4();
    append_event(
        &pool,
        &format!("Order-{order_id}"),
        1,
        "OrderPlaced",
        order_placed_payload(order_id, restaurant_id, customer_id),
    )
    .await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once");

    let repo = PgOrderRepository::new(pool.clone());
    let owner = ReadScope::Customer(CustomerId(customer_id));
    let stranger = ReadScope::Customer(CustomerId(uuid::Uuid::new_v4()));

    let mine = repo.list(OrderFilter::default(), &owner).await.expect("owner list");
    assert_eq!(mine.len(), 1, "the owning customer sees their order");

    let theirs = repo.list(OrderFilter::default(), &stranger).await.expect("stranger list");
    assert!(theirs.is_empty(), "strangers see no orders — the pre-#144 full-table dump is closed");

    let nobody = repo.list(OrderFilter::default(), &ReadScope::Public).await.expect("public list");
    assert!(nobody.is_empty(), "public sees nothing rather than everything");

    assert!(
        repo.by_id(OrderId(order_id), &stranger).await.expect("stranger by_id").is_none(),
        "stranger by_id resolves None — a denial is indistinguishable from absence (no oracle)"
    );
    assert!(
        repo.by_id(OrderId(order_id), &ReadScope::Public).await.expect("public by_id").is_none(),
        "public by_id resolves None"
    );
    assert!(
        repo.by_id(OrderId(order_id), &owner).await.expect("owner by_id").is_some(),
        "the owner reads their order by id"
    );
    assert!(
        repo.by_id(OrderId(order_id), &ReadScope::Admin).await.expect("admin by_id").is_some(),
        "admin short-circuits without membership rows"
    );
    // The RESTAURANT participant was granted at placement too.
    let resto = ReadScope::Restaurant(RestaurantId(restaurant_id));
    assert!(
        repo.by_id(OrderId(order_id), &resto).await.expect("restaurant by_id").is_some(),
        "the order's restaurant reads it (its own back-office queue)"
    );
}

/// Store semantics: a replayed grant is a no-op onto the same row (`granted_at`/`created_at`
/// preserved — ON CONFLICT DO NOTHING), and `revoke_role` drops EVERY rider on the scope, not a
/// named one (the stale-grant breach is the targeted delete leaving the other rider standing).
#[tokio::test]
async fn grant_is_idempotent_and_revoke_drops_the_whole_role() {
    let Some(db) = crate::common::TestDb::acquire("scope_membership_store").await else { return };
    let pool = db.pool();

    let order = uuid::Uuid::new_v4();
    let (rider_a, rider_b) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let t0 = chrono::Utc::now() - chrono::Duration::minutes(10);
    let t1 = chrono::Utc::now();

    let mut conn = pool.acquire().await.expect("conn");
    for (rider, at) in [(rider_a, t0), (rider_b, t0), (rider_a, t1)] {
        scope_membership_store::grant(&mut conn, ScopeType::ORDER, order, UserType::RIDER, rider, at)
            .await
            .expect("grant");
    }
    let (count, oldest): (i64, chrono::DateTime<chrono::Utc>) = sqlx::query_as(
        "SELECT COUNT(*), MIN(granted_at) FROM scopemembership WHERE scope_id = $1",
    )
    .bind(order)
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(count, 2, "a replayed grant upserts onto itself, never duplicates");
    assert!((oldest - t0).num_seconds().abs() < 2, "granted_at preserved on replay");

    assert!(
        scope_membership_store::is_member(&pool, ScopeType::ORDER, order, UserType::RIDER, rider_a)
            .await
            .expect("is_member")
    );
    let scopes =
        scope_membership_store::scopes_for(&pool, UserType::RIDER, rider_a, ScopeType::ORDER)
            .await
            .expect("scopes_for");
    assert_eq!(scopes, vec![order]);

    let deleted =
        scope_membership_store::revoke_role(&mut conn, ScopeType::ORDER, order, UserType::RIDER)
            .await
            .expect("revoke_role");
    assert_eq!(deleted, 2, "the revoke drops EVERY rider on the scope, not a named one");
    assert!(
        !scope_membership_store::is_member(&pool, ScopeType::ORDER, order, UserType::RIDER, rider_b)
            .await
            .expect("is_member after revoke")
    );
}

/// The worker end to end across the group's three categories under ONE checkpoint: OrderPlaced
/// grants (customer + restaurant + the account resolved from the group's OWN RestaurantRegistered
/// grant), the rider is granted on acceptance, and DeliveryCancelled — whose payload carries no
/// orderId — resolves its order through `view_deliveryjob` and revokes the rider role. An orphan
/// cancel (birthless job stream) yields no change and does NOT wedge the group.
#[tokio::test]
async fn delivery_lifecycle_grants_and_revokes_through_the_worker() {
    let Some(db) = crate::common::TestDb::acquire("scope_membership_worker").await else { return };
    let pool = db.pool();

    let order_id = uuid::Uuid::new_v4();
    let restaurant_id = uuid::Uuid::new_v4();
    let account_id = uuid::Uuid::new_v4();
    let customer_id = uuid::Uuid::new_v4();
    let job_id = uuid::Uuid::new_v4();
    let rider_id = uuid::Uuid::new_v4();

    // Restaurant birth (with its owning account) BEFORE the order, as in any real log.
    append_event(
        &pool,
        &format!("Restaurant-{restaurant_id}"),
        1,
        "RestaurantRegistered",
        serde_json::json!({
            "restaurantId": restaurant_id,
            "accountId": account_id,
            "listingStatus": "ACTIVE_PARTNER",
            "displayName": "Chez Marco",
            "address": address(),
            "tags": []
        }),
    )
    .await;
    append_event(
        &pool,
        &format!("Order-{order_id}"),
        1,
        "OrderPlaced",
        order_placed_payload(order_id, restaurant_id, customer_id),
    )
    .await;
    let job_stream = format!("DeliveryJob-{job_id}");
    append_event(
        &pool,
        &job_stream,
        1,
        "DeliveryRequested",
        serde_json::json!({
            "deliveryJobId": job_id,
            "orderId": order_id,
            "restaurantId": restaurant_id,
            "pickup": address(),
            "dropoff": address()
        }),
    )
    .await;
    append_event(
        &pool,
        &job_stream,
        2,
        "DeliveryAcceptedByRider",
        serde_json::json!({ "deliveryJobId": job_id, "orderId": order_id, "riderId": rider_id }),
    )
    .await;

    let worker = ProjectionWorker::new(pool.clone());
    worker.run_once().await.expect("run_once (grants)");

    let member = |st: ScopeType, sid: uuid::Uuid, pt: UserType, pid: uuid::Uuid| {
        let pool = pool.clone();
        async move {
            scope_membership_store::is_member(&pool, st, sid, pt, pid).await.expect("is_member")
        }
    };
    assert!(member(ScopeType::RESTAURANT, restaurant_id, UserType::RESTAURANT, restaurant_id).await);
    assert!(
        member(ScopeType::RESTAURANT, restaurant_id, UserType::RESTAURANT_ACCOUNT, account_id).await
    );
    assert!(member(ScopeType::ORDER, order_id, UserType::CUSTOMER, customer_id).await);
    assert!(member(ScopeType::ORDER, order_id, UserType::RESTAURANT, restaurant_id).await);
    assert!(
        member(ScopeType::ORDER, order_id, UserType::RESTAURANT_ACCOUNT, account_id).await,
        "the account grant resolves from the group's own RestaurantRegistered fold — deterministic under full replay"
    );
    assert!(member(ScopeType::ORDER, order_id, UserType::RIDER, rider_id).await);

    // The cancel carries NO orderId — the worker resolves it via view_deliveryjob, then revokes.
    append_event(
        &pool,
        &job_stream,
        3,
        "DeliveryCancelled",
        serde_json::json!({ "deliveryJobId": job_id, "reason": "bike broke" }),
    )
    .await;
    // An ORPHAN cancel on a birthless stream: resolves to no order, changes nothing, advances past.
    append_event(
        &pool,
        &format!("DeliveryJob-{}", uuid::Uuid::new_v4()),
        1,
        "DeliveryCancelled",
        serde_json::json!({ "deliveryJobId": uuid::Uuid::new_v4(), "reason": "orphan" }),
    )
    .await;
    worker.run_once().await.expect("run_once (revoke + orphan)");

    assert!(
        !member(ScopeType::ORDER, order_id, UserType::RIDER, rider_id).await,
        "the cancel revoked the rider role on the order"
    );
    assert!(
        member(ScopeType::ORDER, order_id, UserType::CUSTOMER, customer_id).await,
        "customer membership is permanent — only the rider role ends"
    );
    let checkpoint: i64 = sqlx::query_scalar(
        "SELECT position FROM projection_checkpoint WHERE projector = 'ScopeMembership'",
    )
    .fetch_one(&pool)
    .await
    .expect("ScopeMembership checkpoint");
    assert_eq!(checkpoint, 6, "the orphan cancel advanced the checkpoint rather than wedging it");
}
