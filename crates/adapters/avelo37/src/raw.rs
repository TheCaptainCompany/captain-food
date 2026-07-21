//! Postgres adapter for the Avelo37 raw webhook mirror (`external_avelo37_events`,
//! `specs/database/tables/integration_staging.yaml`, ADR-20260720-015400): adapter-OWNED staging —
//! one verbatim row per verified partner event id, UPSERTed before interpretation. `processed_at`
//! is the translation high-water mark (NULL ⇒ not yet staged into `inbound_events`).

use async_trait::async_trait;
use domain::shared::errors::DomainError;
use sqlx::PgPool;

use crate::acl::RawAvelo37Events;

fn db_err(e: impl std::fmt::Display) -> DomainError {
    DomainError::Repository(e.to_string())
}

/// Postgres [`RawAvelo37Events`] over `external_avelo37_events`.
pub struct PgRawAvelo37Events {
    pool: PgPool,
}

impl PgRawAvelo37Events {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RawAvelo37Events for PgRawAvelo37Events {
    async fn upsert(
        &self,
        avelo37_event_id: &str,
        event_type: &str,
        payload: &serde_json::Value,
    ) -> Result<bool, DomainError> {
        // A redelivery keeps the FIRST mirrored payload (facts don't change); only the receipt is new.
        let inserted = sqlx::query(
            "INSERT INTO external_avelo37_events (avelo37_event_id, event_type, payload, received_at, processed_at) \
             VALUES ($1, $2, $3, now(), NULL) \
             ON CONFLICT (avelo37_event_id) DO NOTHING",
        )
        .bind(avelo37_event_id)
        .bind(event_type)
        .bind(payload)
        .execute(&self.pool)
        .await
        .map_err(db_err)?
        .rows_affected();
        Ok(inserted == 1)
    }

    async fn mark_processed(&self, avelo37_event_id: &str) -> Result<(), DomainError> {
        sqlx::query("UPDATE external_avelo37_events SET processed_at = now() WHERE avelo37_event_id = $1")
            .bind(avelo37_event_id)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}
