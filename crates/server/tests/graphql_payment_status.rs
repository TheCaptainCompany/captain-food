//! `queries/paymentStatus` ownership scoping under the PUBLIC ACL (#13, ADR-20260720-015500):
//! the operation is `@public` (open on every role path) and the generated resolver enforces
//! ownership — the checkout's customer, its anonymous session (X-SESSION-ID), or ADMIN. Strangers
//! resolve null so the PUBLIC surface never becomes an existence oracle. Executed against the
//! schema directly with the `PaymentProcessStateStore` injected as request data (no DB).

use std::sync::Arc;

use application::pm_state::{mem::MemPaymentProcessState, PaymentProcessRow, PaymentProcessStateStore};
use async_graphql::Request;
use domain::generated::scalars as ds;
use server::graphql_acl::RequestRole;
use server::graphql_schema::build_schema;

fn checkout_row(order_id: uuid::Uuid, session: uuid::Uuid) -> PaymentProcessRow {
    PaymentProcessRow {
        cart_id: ds::CartId(uuid::Uuid::new_v4()),
        order_id: ds::OrderId(order_id),
        payment_intent_id: ds::PaymentIntentId("pi_1".into()),
        process_status: ds::PaymentProcessStatus::AWAITING_PAYMENT_RESULT,
        payment_status: ds::PaymentStatus::PENDING,
        customer_id: None,
        session_id: Some(ds::SessionId(session)),
        client_secret: Some("pi_1_secret".into()),
        last_processed_stripe_event_id: None,
        last_update_utc: chrono::Utc::now(),
    }
}

async fn query_payment_status(
    pm_port: &Arc<dyn PaymentProcessStateStore>,
    order_id: uuid::Uuid,
    role: RequestRole,
    session: Option<uuid::Uuid>,
) -> serde_json::Value {
    let schema = build_schema(None, None, None);
    let query = format!(
        r#"{{ paymentStatus(input: {{ orderId: "{order_id}" }}) {{ paymentIntentId clientSecret status }} }}"#
    );
    let resp = schema
        .execute(
            Request::new(query)
                .data(role)
                .data(server::graphql_session::SessionHeader(session))
                .data(pm_port.clone()),
        )
        .await;
    assert!(resp.errors.is_empty(), "paymentStatus errored: {:?}", resp.errors);
    resp.data.into_json().expect("json")["paymentStatus"].clone()
}

/// The anonymous checkout owner (matching X-SESSION-ID) reads its run on the PUBLIC path; a
/// stranger session — and a caller with no session at all — resolves null, not an error.
#[tokio::test(flavor = "multi_thread")]
async fn payment_status_is_public_and_session_scoped() {
    let pm = Arc::new(MemPaymentProcessState::default());
    let pm_port: Arc<dyn PaymentProcessStateStore> = pm.clone();
    let (order_id, session) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    pm.upsert(&checkout_row(order_id, session)).await.unwrap();

    let owned = query_payment_status(&pm_port, order_id, RequestRole::Public, Some(session)).await;
    assert_eq!(owned["clientSecret"], serde_json::json!("pi_1_secret"));
    assert_eq!(owned["status"], serde_json::json!("PENDING"));

    let stranger =
        query_payment_status(&pm_port, order_id, RequestRole::Public, Some(uuid::Uuid::new_v4()))
            .await;
    assert!(stranger.is_null(), "a stranger session must resolve null, got {stranger}");

    let sessionless = query_payment_status(&pm_port, order_id, RequestRole::Public, None).await;
    assert!(sessionless.is_null(), "no session must resolve null, got {sessionless}");
}

/// ADMIN reads any checkout run — the ownership branch the [CUSTOMER]-only guard used to make
/// unreachable (#13).
#[tokio::test(flavor = "multi_thread")]
async fn payment_status_admin_reads_any_checkout() {
    let pm = Arc::new(MemPaymentProcessState::default());
    let pm_port: Arc<dyn PaymentProcessStateStore> = pm.clone();
    let (order_id, session) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    pm.upsert(&checkout_row(order_id, session)).await.unwrap();

    let admin = query_payment_status(&pm_port, order_id, RequestRole::Admin, None).await;
    assert_eq!(admin["paymentIntentId"], serde_json::json!("pi_1"));
    assert_eq!(admin["status"], serde_json::json!("PENDING"));
}
