//! Integration test for `PgHubRiseConnections` (issue #20): the account-scoped token store the
//! connect flow writes and the enricher resolves callbacks through.
//!
//! Needs a real Postgres: set `DATABASE_URL` (see infrastructure/tests/restaurant_projection.rs for
//! a throwaway docker one-liner). Without it the test SKIPS (prints and returns) so `cargo test`
//! stays green offline.

use hubrise_adapter::connections::{ConnectedLocation, HubRiseConnection};
use hubrise_adapter::{HubRiseConnections, PgHubRiseConnections};
use sqlx::PgPool;

/// Fresh copies of the two `integration_connections` tables (mirroring
/// migrations/20260721120000_hubrise_connections.sql / the generated schema).
async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS hubrise_connections, hubrise_connection_locations CASCADE;
        CREATE TABLE hubrise_connections (
          restaurant_account_id UUID PRIMARY KEY,
          hubrise_account_id TEXT NOT NULL UNIQUE,
          access_token TEXT NOT NULL,
          account_name TEXT NULL,
          connected_at TIMESTAMPTZ NOT NULL,
          last_connected_at TIMESTAMPTZ NOT NULL
        );
        CREATE TABLE hubrise_connection_locations (
          hubrise_location_id TEXT PRIMARY KEY,
          restaurant_account_id UUID NOT NULL,
          restaurant_id UUID NOT NULL,
          last_connected_at TIMESTAMPTZ NOT NULL
        );
        CREATE INDEX ON hubrise_connection_locations (restaurant_account_id);
        "#,
    )
    .execute(pool)
    .await
    .expect("reset schema");
}

fn connection(token: &str) -> HubRiseConnection {
    HubRiseConnection {
        restaurant_account_id: uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, b"acct"),
        hubrise_account_id: "acc_1".into(),
        access_token: token.into(),
        account_name: Some("Bella Pizza".into()),
    }
}

fn location(id: &str, account: uuid::Uuid) -> ConnectedLocation {
    ConnectedLocation {
        hubrise_location_id: id.into(),
        restaurant_account_id: account,
        restaurant_id: uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, id.as_bytes()),
    }
}

/// One test function on purpose: the tables are shared state, so the scenario runs sequentially.
#[tokio::test]
async fn upserts_connections_and_resolves_tokens_by_location() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set — skipping Pg-gated hubrise connections test");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect");
    reset_schema(&pool).await;
    let store = PgHubRiseConnections::new(pool.clone());

    // First connect: token + two locations stored.
    let conn = connection("tok_1");
    let account = conn.restaurant_account_id;
    store
        .upsert(&conn, &[location("loc_1", account), location("loc_2", account)])
        .await
        .expect("first upsert");
    assert_eq!(
        store.token_for_location("loc_1").await.expect("lookup"),
        Some("tok_1".to_string())
    );
    assert_eq!(
        store.token_for_location("loc_2").await.expect("lookup"),
        Some("tok_1".to_string())
    );
    // An unconnected location resolves to nothing (the enricher's definitive skip).
    assert_eq!(store.token_for_location("loc_other").await.expect("lookup"), None);

    // Re-connect: the token refreshes in place (same pk), the location snapshot upserts, and
    // `connected_at` (first connect) survives while `last_connected_at` moves.
    let (connected_before, last_before) =
        sqlx::query_as::<_, (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
            "SELECT connected_at, last_connected_at FROM hubrise_connections WHERE restaurant_account_id = $1",
        )
        .bind(account)
        .fetch_one(&pool)
        .await
        .expect("read timestamps");
    sqlx::query("UPDATE hubrise_connections SET connected_at = connected_at - interval '1 hour', last_connected_at = last_connected_at - interval '1 hour'")
        .execute(&pool)
        .await
        .expect("age rows");
    store.upsert(&connection("tok_2"), &[location("loc_1", account)]).await.expect("re-upsert");
    assert_eq!(
        store.token_for_location("loc_1").await.expect("lookup"),
        Some("tok_2".to_string())
    );
    let (connected_after, last_after) =
        sqlx::query_as::<_, (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
            "SELECT connected_at, last_connected_at FROM hubrise_connections WHERE restaurant_account_id = $1",
        )
        .bind(account)
        .fetch_one(&pool)
        .await
        .expect("read timestamps");
    assert!(connected_after < connected_before, "connected_at is NOT reset by a re-connect");
    assert!(last_after >= last_before, "last_connected_at moves forward on re-connect");

    // Exactly one connection row exists (the upsert keyed on the RestaurantAccount).
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM hubrise_connections")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(rows, 1);
}
