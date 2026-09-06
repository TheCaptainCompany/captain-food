//! Postgres adapter for `application::ports::AsOfPriceAuthority` (PROP-20260831-134539 §2.1 step 3,
//! slice 2 of "the priced quote token"). DARK in this slice: no production caller wires it yet.
//!
//! Reads a RANGE of the `Catalog-<id>` stream — `WHERE stream_name = $1 AND version <= $2` — rather
//! than the whole-stream `SELECT` [`crate::persistence::event_store::PgEventStore::load`] uses
//! (dba): the SQL predicate is the authoritative truncation (it alone correctly accounts for the
//! `$`-prefixed technical rows that occupy a version slot but never decode into a [`DomainEvent`]);
//! [`domain::catalog_as_of::AsOfCatalog::from_stream`]'s own index-based truncation on top of that is
//! defence in depth, not the primary boundary.
//!
//! The stream name is built through [`domain::catalog::stream`]/[`domain::catalog::CATEGORY`] —
//! never a fourth `format!("Catalog-{}")` literal (vernon CATCH).
//!
//! `version` is the PORT's own 0-based coordinate (`version` V ⇔ V+1 events applied) — deliberately
//! NOT the same number as the `domain_events.version` column, which starts at 1 for a stream's first
//! row; the `+1` conversion happens once, here, at the SQL boundary.
//!
//! Decode is split from the SQL leg on purpose ([`PgAsOfCatalogRepository::fetch_rows`] /
//! [`PgAsOfCatalogRepository::decode_rows`] are both `pub`) so a caller measuring cost — the DB-gated
//! ceiling test — can time each separately (dba: decode cost is the cost on a per-checkout-shaped
//! read, and a single combined timer would hide which leg a regression lands in). The decode itself
//! is the same adjacently-tagged envelope `load_inner` builds (`json!({"eventType": .., "payload":
//! ..})`) — `DomainEvent`'s wire shape (`#[serde(tag = "eventType", content = "payload")]`) needs
//! both keys, so there is no cheaper in-place shape available here.

use application::ports::AsOfPriceAuthority;
use async_trait::async_trait;
use domain::catalog_as_of::AsOfCatalog;
use domain::generated::events::DomainEvent;
use domain::generated::scalars::CatalogId;
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};

use crate::persistence::db_err;

/// One fetched row: the event type tag and its business payload, before decoding.
type RawRow = (String, serde_json::Value);

pub struct PgAsOfCatalogRepository {
    pool: PgPool,
}

impl PgAsOfCatalogRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The SQL leg alone: rows at or before `version` (inclusive, port's 0-based convention) on
    /// `stream_name`, in version order — `$`-prefixed technical rows included (the predicate is on
    /// the real `version` column, so it truncates correctly regardless of how many technical rows
    /// occupy slots in the range; they are dropped by [`decode_rows`], never by this query).
    /// Exposed `pub` (not only via the [`AsOfPriceAuthority`] trait) so a caller measuring cost can
    /// time the SQL leg and the decode leg separately.
    pub async fn fetch_rows(&self, stream_name: &str, version: i64) -> Result<Vec<RawRow>, DomainError> {
        let db_version_ceiling = i32::try_from(version + 1).map_err(db_err)?;
        let rows = sqlx::query(
            "SELECT event_type, payload FROM domain_events \
             WHERE stream_name = $1 AND version <= $2 ORDER BY version",
        )
        .bind(stream_name)
        .bind(db_version_ceiling)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let event_type: String = row.try_get("event_type").map_err(db_err)?;
            let payload: serde_json::Value = row.try_get("payload").map_err(db_err)?;
            out.push((event_type, payload));
        }
        Ok(out)
    }

    /// The decode leg alone: business events only (`$`-prefixed technical rows are skipped, same
    /// EventStore convention `load_inner` follows — ADR-20260731-160000 §5). `DomainEvent`'s
    /// adjacently-tagged wire shape (`#[serde(tag = "eventType", content = "payload")]`) needs the
    /// two-key envelope `{"eventType": .., "payload": ..}` — the payload is MOVED into it, never
    /// cloned, matching `load_inner`'s own `json!({"eventType": event_type, "payload": payload})`.
    pub fn decode_rows(rows: Vec<RawRow>) -> Result<Vec<DomainEvent>, DomainError> {
        let mut events = Vec::with_capacity(rows.len());
        for (event_type, payload) in rows {
            if event_type.starts_with('$') {
                continue;
            }
            let event: DomainEvent = serde_json::from_value(serde_json::json!({
                "eventType": event_type,
                "payload": payload,
            }))
            .map_err(|e| db_err(format!("{event_type}: {e}")))?;
            events.push(event);
        }
        Ok(events)
    }

    /// Both legs, instrumented at this adapter seam (never in `pricing.rs`/`crates/domain` — obs
    /// consent): span `catalog.as_of.fold` (INTERNAL). The `specs/observability.yaml` contract row
    /// for it lands with slice 4, once a caller supplies a real correlation id; the span exists now
    /// so slice 4 only has to bind it, not invent it.
    async fn load_range(&self, catalog_id: CatalogId, version: i64) -> Result<Vec<DomainEvent>, DomainError> {
        let stream = domain::catalog::stream(catalog_id);
        let span = tracing::info_span!(
            "catalog.as_of.fold",
            otel.kind = "internal",
            business.aggregate_id = %catalog_id.0,
            business.version = version,
            business.events_applied = tracing::field::Empty,
        );
        let _entered = span.enter();
        let rows = self.fetch_rows(&stream, version).await?;
        let events = Self::decode_rows(rows)?;
        span.record("business.events_applied", events.len());
        Ok(events)
    }
}

#[async_trait]
impl AsOfPriceAuthority for PgAsOfCatalogRepository {
    async fn as_of(&self, catalog_id: CatalogId, version: i64) -> Result<AsOfCatalog, DomainError> {
        let events = self.load_range(catalog_id, version).await?;
        // The SQL predicate already bounded `events` correctly; `up_to` here only needs to keep
        // everything fetched (defence in depth against a caller that supplied more than it should
        // have is `AsOfCatalog::from_stream`'s own job, not this call site's).
        Ok(AsOfCatalog::from_stream(&events, events.len() as i64 - 1))
    }
}
