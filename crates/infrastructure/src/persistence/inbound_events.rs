//! Postgres adapter for the inbound-event inbox (`inbound_events`, ADR-20260720-015400): adapted
//! inbound BUSINESS events staged by adapter ACLs, drained by the InboundEventsDrainWorker through
//! the normal write path. `(source, external_id)` is the delivery-level dedupe (`ON CONFLICT DO
//! NOTHING`); the aggregate's own fold stays the authoritative dedupe on delivery.

use application::journal::{InboundEventRow, InboundEvents, StageOutcome};
use async_trait::async_trait;
use domain::generated::scalars::InboundEventStatus;
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use super::db_err;
use super::enum_sql::EnumOrd;

/// Column list of `inbound_events`, in [`InboundEventRow`] field order.
const COLUMNS: &str = "inbound_event_id, source, external_id, correlation_id, event_type, payload, \
     status, error, received_at, delivered_at";

fn decode(row: &PgRow) -> Result<InboundEventRow, DomainError> {
    Ok(InboundEventRow {
        inbound_event_id: row.try_get("inbound_event_id").map_err(db_err)?,
        source: row.try_get("source").map_err(db_err)?,
        external_id: row.try_get("external_id").map_err(db_err)?,
        correlation_id: row.try_get("correlation_id").map_err(db_err)?,
        event_type: row.try_get("event_type").map_err(db_err)?,
        payload: row.try_get("payload").map_err(db_err)?,
        status: EnumOrd::from_ord(row.try_get::<i32, _>("status").map_err(db_err)?)?,
        error: row.try_get("error").map_err(db_err)?,
        received_at: row.try_get("received_at").map_err(db_err)?,
        delivered_at: row.try_get("delivered_at").map_err(db_err)?,
    })
}

/// Postgres [`InboundEvents`] over `inbound_events`.
pub struct PgInboundEvents {
    pool: PgPool,
}

impl PgInboundEvents {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl InboundEvents for PgInboundEvents {
    async fn stage(&self, row: &InboundEventRow) -> Result<StageOutcome, DomainError> {
        let sql = format!(
            "INSERT INTO inbound_events ({COLUMNS}) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,NULL,now(),NULL) \
             ON CONFLICT (source, external_id) DO NOTHING"
        );
        let inserted = sqlx::query(&sql)
            .bind(row.inbound_event_id)
            .bind(row.source.clone())
            .bind(row.external_id.clone())
            .bind(row.correlation_id)
            .bind(row.event_type.clone())
            .bind(row.payload.clone())
            .bind(InboundEventStatus::RECEIVED.to_ord())
            .execute(&self.pool)
            .await
            .map_err(db_err)?
            .rows_affected();
        Ok(if inserted == 1 { StageOutcome::Staged } else { StageOutcome::Duplicate })
    }

    async fn pending(&self, limit: i64) -> Result<Vec<InboundEventRow>, DomainError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM inbound_events WHERE status = $1 \
             ORDER BY received_at, inbound_event_id LIMIT $2"
        );
        let rows = sqlx::query(&sql)
            .bind(InboundEventStatus::RECEIVED.to_ord())
            .bind(limit.max(0))
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        rows.iter().map(decode).collect()
    }

    async fn mark_delivered(&self, inbound_event_id: uuid::Uuid) -> Result<(), DomainError> {
        sqlx::query(
            "UPDATE inbound_events SET status = $2, delivered_at = now() WHERE inbound_event_id = $1",
        )
        .bind(inbound_event_id)
        .bind(InboundEventStatus::DELIVERED.to_ord())
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn mark_failed(
        &self,
        inbound_event_id: uuid::Uuid,
        error: serde_json::Value,
    ) -> Result<(), DomainError> {
        sqlx::query("UPDATE inbound_events SET status = $2, error = $3 WHERE inbound_event_id = $1")
            .bind(inbound_event_id)
            .bind(InboundEventStatus::FAILED.to_ord())
            .bind(error)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}
