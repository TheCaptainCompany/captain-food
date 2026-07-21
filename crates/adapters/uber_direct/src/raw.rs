//! Postgres adapter for the Uber Direct raw webhook mirror (`external_uber_direct_events`,
//! `specs/database/tables/integration_staging.yaml`, ADR-20260721-172500): adapter-OWNED staging —
//! one verbatim row per verified event, keyed by the provider `event_id`. `processed_at` is the
//! translation high-water mark (NULL ⇒ not yet staged into `inbound_events`).

use async_trait::async_trait;
use domain::shared::errors::DomainError;
use sqlx::PgPool;

use crate::acl::RawUberDirectEvents;

fn db_err(e: impl std::fmt::Display) -> DomainError {
    DomainError::Repository(e.to_string())
}

/// Postgres [`RawUberDirectEvents`] over `external_uber_direct_events`.
pub struct PgRawUberDirectEvents {
    pool: PgPool,
}

impl PgRawUberDirectEvents {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RawUberDirectEvents for PgRawUberDirectEvents {
    async fn upsert(
        &self,
        uber_event_id: &str,
        event_type: &str,
        payload: &serde_json::Value,
    ) -> Result<bool, DomainError> {
        // A redelivery keeps the FIRST mirrored payload (facts don't change); only the receipt is new.
        let inserted = sqlx::query(
            "INSERT INTO external_uber_direct_events (uber_event_id, event_type, payload, received_at, processed_at) \
             VALUES ($1, $2, $3, now(), NULL) \
             ON CONFLICT (uber_event_id) DO NOTHING",
        )
        .bind(uber_event_id)
        .bind(event_type)
        .bind(payload)
        .execute(&self.pool)
        .await
        .map_err(db_err)?
        .rows_affected();
        Ok(inserted == 1)
    }

    async fn mark_processed(&self, uber_event_id: &str) -> Result<(), DomainError> {
        sqlx::query("UPDATE external_uber_direct_events SET processed_at = now() WHERE uber_event_id = $1")
            .bind(uber_event_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}
