//! HubRise connection/token store (`hubrise_connections` + `hubrise_connection_locations`,
//! `specs/database/tables/integration_connections.yaml`, issue #20): adapter-OWNED credential state.
//!
//! One row per connected HubRise Account, keyed by the `RestaurantAccount` the connect flow
//! provisioned (`restaurant_account_id` = UUIDv5 of the HubRise account id). The token is a revocable
//! CREDENTIAL, never a business fact: it is never event-sourced and never referenced by api.yaml, so
//! no GraphQL edge can reach it. Callbacks carry a `location_id`, so each connect snapshots the
//! account's locations into `hubrise_connection_locations` — the enricher resolves
//! `callback.location_id → access_token` through [`HubRiseConnections::token_for_location`].

use async_trait::async_trait;
use domain::shared::errors::DomainError;
use sqlx::PgPool;

fn db_err(e: impl std::fmt::Display) -> DomainError {
    DomainError::Repository(e.to_string())
}

/// One connected HubRise account (the `hubrise_connections` row, minus timestamps).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubRiseConnection {
    /// UUIDv5(account:<hubrise account id>) — the RestaurantAccount aggregate the connect provisioned.
    pub restaurant_account_id: uuid::Uuid,
    pub hubrise_account_id: String,
    /// Account-scoped, non-expiring OAuth access token (HubRise has no refresh tokens).
    pub access_token: String,
    pub account_name: Option<String>,
}

/// One location covered by a connection (a `hubrise_connection_locations` row, minus timestamps).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectedLocation {
    pub hubrise_location_id: String,
    pub restaurant_account_id: uuid::Uuid,
    /// UUIDv5(location:<id>) — the Restaurant aggregate registered for this location.
    pub restaurant_id: uuid::Uuid,
}

/// Adapter-owned connection/token port; trait so the connect flow and the enricher are unit-testable
/// in memory (like [`crate::raw::RawHubRiseCallbacks`]).
#[async_trait]
pub trait HubRiseConnections: Send + Sync {
    /// UPSERT a connection and its location snapshot (re-connect = token refresh + location catch-up).
    async fn upsert(
        &self,
        connection: &HubRiseConnection,
        locations: &[ConnectedLocation],
    ) -> Result<(), DomainError>;

    /// Resolve a callback's `location_id` to the owning account's access token, or `None` when the
    /// location belongs to no connected account (callback skipped until a re-connect).
    async fn token_for_location(
        &self,
        hubrise_location_id: &str,
    ) -> Result<Option<String>, DomainError>;
}

/// Postgres [`HubRiseConnections`] over the two `integration_connections` tables.
pub struct PgHubRiseConnections {
    pool: PgPool,
}

impl PgHubRiseConnections {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl HubRiseConnections for PgHubRiseConnections {
    async fn upsert(
        &self,
        connection: &HubRiseConnection,
        locations: &[ConnectedLocation],
    ) -> Result<(), DomainError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        sqlx::query(
            "INSERT INTO hubrise_connections \
               (restaurant_account_id, hubrise_account_id, access_token, account_name, connected_at, last_connected_at) \
             VALUES ($1, $2, $3, $4, now(), now()) \
             ON CONFLICT (restaurant_account_id) DO UPDATE SET \
               hubrise_account_id = EXCLUDED.hubrise_account_id, \
               access_token = EXCLUDED.access_token, \
               account_name = EXCLUDED.account_name, \
               last_connected_at = now()",
        )
        .bind(connection.restaurant_account_id)
        .bind(&connection.hubrise_account_id)
        .bind(&connection.access_token)
        .bind(&connection.account_name)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        for loc in locations {
            sqlx::query(
                "INSERT INTO hubrise_connection_locations \
                   (hubrise_location_id, restaurant_account_id, restaurant_id, last_connected_at) \
                 VALUES ($1, $2, $3, now()) \
                 ON CONFLICT (hubrise_location_id) DO UPDATE SET \
                   restaurant_account_id = EXCLUDED.restaurant_account_id, \
                   restaurant_id = EXCLUDED.restaurant_id, \
                   last_connected_at = now()",
            )
            .bind(&loc.hubrise_location_id)
            .bind(loc.restaurant_account_id)
            .bind(loc.restaurant_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)
    }

    async fn token_for_location(
        &self,
        hubrise_location_id: &str,
    ) -> Result<Option<String>, DomainError> {
        let token = sqlx::query_scalar::<_, String>(
            "SELECT c.access_token FROM hubrise_connection_locations l \
             JOIN hubrise_connections c USING (restaurant_account_id) \
             WHERE l.hubrise_location_id = $1",
        )
        .bind(hubrise_location_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(token)
    }
}

/// In-memory [`HubRiseConnections`] double for unit tests (connect flow + enricher).
pub mod mem {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct MemHubRiseConnections {
        state: Mutex<MemState>,
    }

    #[derive(Default)]
    struct MemState {
        connections: HashMap<uuid::Uuid, HubRiseConnection>,
        locations: HashMap<String, ConnectedLocation>,
    }

    impl MemHubRiseConnections {
        pub fn connection(&self, restaurant_account_id: uuid::Uuid) -> Option<HubRiseConnection> {
            self.state.lock().unwrap().connections.get(&restaurant_account_id).cloned()
        }
        pub fn location(&self, hubrise_location_id: &str) -> Option<ConnectedLocation> {
            self.state.lock().unwrap().locations.get(hubrise_location_id).cloned()
        }
    }

    #[async_trait]
    impl HubRiseConnections for MemHubRiseConnections {
        async fn upsert(
            &self,
            connection: &HubRiseConnection,
            locations: &[ConnectedLocation],
        ) -> Result<(), DomainError> {
            let mut s = self.state.lock().unwrap();
            s.connections.insert(connection.restaurant_account_id, connection.clone());
            for loc in locations {
                s.locations.insert(loc.hubrise_location_id.clone(), loc.clone());
            }
            Ok(())
        }

        async fn token_for_location(
            &self,
            hubrise_location_id: &str,
        ) -> Result<Option<String>, DomainError> {
            let s = self.state.lock().unwrap();
            Ok(s.locations
                .get(hubrise_location_id)
                .and_then(|l| s.connections.get(&l.restaurant_account_id))
                .map(|c| c.access_token.clone()))
        }
    }
}
