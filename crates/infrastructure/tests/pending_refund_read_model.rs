//! Integration test for the refund-queue read-side slice: RefundProcess facts in `domain_events` →
//! `View_PendingRefunds` (the generated fold VIEW, projection-on-read — no worker involved, ADR-0039)
//! → read repository. Needs a real Postgres: set `DATABASE_URL` (see restaurant_projection.rs for a
//! throwaway docker one-liner). Without it the test SKIPS (prints and returns) so `cargo test` stays
//! green offline.
//!
//! rules.yaml#/PendingRefundVisibleUntilDecided: a refund opened for decision stays visible as
//! REQUESTED until decided; the decision/settlement update its status instead of dropping the row.
//!
//! One test function on purpose: the tables are shared state, so the scenario must run sequentially.

use application::queries::{RefundFilter, RefundReadRepository as _};
use chrono::{Duration, Utc};
use domain::generated::scalars::{RefundId, RefundStatus, RestaurantId};
use infrastructure::PgRefundQueueRepository;
use sqlx::PgPool;

/// Fresh `domain_events` + the `View_PendingRefunds` fold view over it (mirrors
/// migrations/20260720002210, whose view section is the generated `specs/generated/views.generated.sql`).
async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS domain_events CASCADE;
        CREATE TABLE domain_events (
          position BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
          id UUID NOT NULL UNIQUE,
          stream_name TEXT NOT NULL,
          version INTEGER NOT NULL,
          user_id UUID NOT NULL,
          user_type INTEGER NOT NULL,
          correlation_id UUID NOT NULL,
          cause_id UUID NULL,
          event_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          metadata JSONB NULL,
          occurred_at TIMESTAMPTZ NOT NULL,
          expired_at TIMESTAMPTZ NULL,
          UNIQUE (stream_name, version)
        );
        CREATE OR REPLACE VIEW View_PendingRefunds AS
        SELECT
          (c.payload->>'orderId')::uuid AS order_id,
          (c.payload->>'restaurantId')::uuid AS restaurant_id,
          (SELECT CASE e.event_type WHEN 'RefundOpened' THEN 0 WHEN 'RefundApproved' THEN 1 WHEN 'RefundDenied' THEN 2 WHEN 'PaymentRefunded' THEN 3 END FROM domain_events e
             WHERE e.stream_name = c.stream_name AND e.event_type IN ('RefundOpened', 'RefundApproved', 'RefundDenied', 'PaymentRefunded')
             ORDER BY e.position DESC LIMIT 1) AS status,
          (c.payload->'amount'->>'amountCents')::bigint AS amount_cents,
          c.payload->'amount'->>'currency' AS currency,
          (SELECT (e.payload->'amount'->>'amountCents')::bigint FROM domain_events e
             WHERE e.stream_name = c.stream_name AND e.event_type IN ('RefundApproved') AND e.payload ? 'amount'
             ORDER BY e.position DESC LIMIT 1) AS approved_amount_cents,
          (SELECT e.payload->>'reason' FROM domain_events e
             WHERE e.stream_name = c.stream_name AND e.event_type IN ('RefundOpened', 'RefundApproved', 'RefundDenied') AND e.payload ? 'reason'
             ORDER BY e.position DESC LIMIT 1) AS reason,
          (SELECT e.payload->>'refundId' FROM domain_events e
             WHERE e.stream_name = c.stream_name AND e.event_type IN ('PaymentRefunded') AND e.payload ? 'refundId'
             ORDER BY e.position DESC LIMIT 1) AS refund_id,
          c.occurred_at AS requested_at,
          (SELECT max(e.occurred_at) FROM domain_events e
             WHERE e.stream_name = c.stream_name AND e.event_type IN ('RefundApproved', 'RefundDenied')) AS decided_at,
          c.occurred_at AS created_at,
          (SELECT max(e.occurred_at) FROM domain_events e
             WHERE e.stream_name = c.stream_name AND e.event_type IN ('RefundOpened', 'RefundApproved', 'RefundDenied', 'PaymentRefunded')) AS updated_at
        FROM domain_events c
        WHERE c.event_type = 'RefundOpened';
        "#,
    )
    .execute(pool)
    .await
    .expect("reset schema");
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

fn eur(cents: i64) -> serde_json::Value {
    serde_json::json!({ "amountCents": cents, "currency": "EUR" })
}

#[tokio::test]
async fn refund_lifecycle_events_serve_the_refund_queue() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP refund_lifecycle_events_serve_the_refund_queue: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

    let (r1, r2) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let (o1, o2, o3) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let t0 = Utc::now() - Duration::minutes(30);

    // o1 (restaurant r1): opened only — the REQUESTED (pending) queue. The Payment stream also
    // carries non-refund facts the view must ignore.
    let s1 = "Payment-pi_1";
    append_event(&pool, s1, 1, "PaymentCaptured",
        serde_json::json!({ "paymentIntentId": "pi_1", "orderId": o1, "restaurantId": r1, "amount": eur(1960) }), t0).await;
    append_event(&pool, s1, 2, "RefundOpened",
        serde_json::json!({ "orderId": o1, "restaurantId": r1, "amount": eur(1960), "reason": "Out of ingredients" }),
        t0 + Duration::minutes(1)).await;

    // o2 (restaurant r1): opened → approved (partial) → settled by Stripe.
    let s2 = "Payment-pi_2";
    append_event(&pool, s2, 1, "RefundOpened",
        serde_json::json!({ "orderId": o2, "restaurantId": r1, "amount": eur(2019), "reason": "Late delivery" }),
        t0 + Duration::minutes(2)).await;
    append_event(&pool, s2, 2, "RefundApproved",
        serde_json::json!({ "orderId": o2, "amount": eur(1000), "reason": "Half refund agreed" }),
        t0 + Duration::minutes(5)).await;
    append_event(&pool, s2, 3, "PaymentRefunded",
        serde_json::json!({ "refundId": "re_2", "paymentIntentId": "pi_2", "orderId": o2, "restaurantId": r1, "amount": eur(1000) }),
        t0 + Duration::minutes(9)).await;

    // o3 (restaurant r2): opened → denied. Stays visible with its decision, not dropped.
    let s3 = "Payment-pi_3";
    append_event(&pool, s3, 1, "RefundOpened",
        serde_json::json!({ "orderId": o3, "restaurantId": r2, "amount": eur(500), "reason": "Changed my mind" }),
        t0 + Duration::minutes(3)).await;
    append_event(&pool, s3, 2, "RefundDenied",
        serde_json::json!({ "orderId": o3, "reason": "Outside the refund window" }),
        t0 + Duration::minutes(6)).await;

    let repo = PgRefundQueueRepository::new(pool.clone());

    // The whole queue, newest-request-first, full lifecycle visible.
    let all = repo.list(RefundFilter::default()).await.expect("list all");
    assert_eq!(all.len(), 3);
    assert_eq!(
        all.iter().map(|r| r.order_id.0).collect::<Vec<_>>(),
        vec![o3, o2, o1] // requested_at DESC
    );

    // The pending (awaiting-decision) queue: status = REQUESTED only.
    let pending = repo
        .list(RefundFilter { restaurant_id: None, status: Some(RefundStatus::REQUESTED) })
        .await
        .expect("list pending");
    assert_eq!(pending.len(), 1);
    let p = &pending[0];
    assert_eq!(p.order_id.0, o1);
    assert_eq!(p.restaurant_id, RestaurantId(r1));
    assert_eq!(p.amount_cents.0, 1960);
    assert_eq!(p.currency.0, "EUR");
    assert_eq!(p.approved_amount_cents, None);
    assert_eq!(p.reason.as_deref(), Some("Out of ingredients"));
    assert_eq!(p.refund_id, None);
    assert_eq!(p.decided_at, None);

    // Restaurant scoping: r1 sees only its own orders' refunds.
    let r1_rows = repo
        .list(RefundFilter { restaurant_id: Some(RestaurantId(r1)), status: None })
        .await
        .expect("list r1");
    assert_eq!(r1_rows.iter().map(|r| r.order_id.0).collect::<Vec<_>>(), vec![o2, o1]);

    // The settled approval keeps the (partial) approved amount and the Stripe refund id.
    let settled = &r1_rows[0];
    assert_eq!(settled.status, RefundStatus::REFUNDED);
    assert_eq!(settled.amount_cents.0, 2019);
    assert_eq!(settled.approved_amount_cents.map(|c| c.0), Some(1000));
    assert_eq!(settled.refund_id, Some(RefundId("re_2".into())));
    assert!(settled.decided_at.is_some());

    // The denied refund stays visible with its decision (never dropped).
    let denied = repo
        .list(RefundFilter { restaurant_id: Some(RestaurantId(r2)), status: None })
        .await
        .expect("list r2");
    assert_eq!(denied.len(), 1);
    assert_eq!(denied[0].status, RefundStatus::DENIED);
    assert_eq!(denied[0].reason.as_deref(), Some("Outside the refund window"));
}
