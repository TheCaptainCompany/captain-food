//! Postgres adapter for `application::ports::AsOfPriceAuthority` (PROP-20260831-134539 §2.1 step 3,
//! slice 2 of "the priced quote token"). DARK in this slice: no production caller wires it yet.
//!
//! Reads a RANGE of the `Catalog-<id>` stream — `WHERE stream_name = $1 AND version <= $2` — rather
//! than the whole-stream `SELECT` [`crate::persistence::event_store::PgEventStore::load`] uses
//! (dba); the SQL predicate bounds the read cheaply, but it is NOT the sole truncation authority any
//! more (round 2 correction, see below) — [`domain::catalog_as_of::AsOfCatalog::from_stream`]'s own
//! per-event-version filtering is what actually decides which events fold, and it is real defence in
//! depth at THIS call site, not merely documented intent.
//!
//! The stream name is built through [`domain::catalog::stream`]/[`domain::catalog::CATEGORY`] —
//! never a fourth `format!("Catalog-{}")` literal (vernon CATCH).
//!
//! **`version` is [`CatalogVersion`] — the 1-based `domain_events.version` verbatim, never a
//! port-invented 0-based convention.** Round 1 shipped a bare `i64` here with a `db_version_ceiling =
//! version + 1` conversion "because the port is 0-based" — a coordinate minted from the write-side
//! fold's own returned version (`EventStore::append`'s return) would then have read ONE EVENT PAST
//! what the caller named, silently, whenever the extra event was not price-bearing (vernon
//! B1/young B1, PROP-20260831-134539 slice 2 round 2). There is now exactly one spelling of the
//! coordinate on this whole path: [`CatalogVersion::get`] passed DIRECTLY as the SQL ceiling, no `+1`
//! anywhere.
//!
//! **A coordinate beyond head is now an ERROR, never a HEAD price.** [`fetch_rows`] also returns each
//! row's own `version`; [`PgAsOfCatalogRepository::load_range`] compares the HIGHEST version returned
//! (technical rows included, before [`decode_rows`] drops them) against the requested coordinate and
//! fails closed on a mismatch — a caller with a stale, forged or garbled coordinate gets a refusal,
//! never today's silent clamp to whatever the range happened to contain.
//!
//! Decode is split from the SQL leg on purpose ([`PgAsOfCatalogRepository::fetch_rows`] /
//! [`PgAsOfCatalogRepository::decode_rows`] are both `pub`, kept that way ONLY because the DB-gated
//! benchmark test lives in `crates/infrastructure/tests/` — a separate crate from this adapter's own
//! — and needs to time the SQL leg and the decode leg separately; neither is a general `load_upto`
//! primitive) so a caller measuring cost can time each separately (dba: decode cost is the cost on a
//! per-checkout-shaped read, and a single combined timer would hide which leg a regression lands in).
//! The decode itself is the same adjacently-tagged envelope `load_inner` builds (`json!({"eventType":
//! .., "payload": ..})`) — `DomainEvent`'s wire shape (`#[serde(tag = "eventType", content =
//! "payload")]`) needs both keys, so there is no cheaper in-place shape available here.

use application::ports::AsOfPriceAuthority;
use async_trait::async_trait;
use domain::catalog_as_of::{AsOfCatalog, CatalogVersion};
use domain::generated::events::DomainEvent;
use domain::generated::scalars::CatalogId;
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};
use tracing::Instrument as _;

use crate::persistence::db_err;

/// One fetched row: its own stream VERSION, the event type tag, and its business payload, before
/// decoding. The version travels WITH the row (round 2 addition) so [`decode_rows`] can hand
/// [`domain::catalog_as_of::AsOfCatalog::from_stream`] `(CatalogVersion, DomainEvent)` pairs — each
/// event's own version, never a slice position — and so [`PgAsOfCatalogRepository::load_range`] can
/// check the highest version returned against the requested coordinate BEFORE decoding drops the
/// `$`-prefixed technical rows that would otherwise hide a short read.
type RawRow = (i64, String, serde_json::Value);

pub struct PgAsOfCatalogRepository {
    pool: PgPool,
}

impl PgAsOfCatalogRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The SQL leg alone: rows at or before `version` (inclusive, the 1-based `domain_events.version`
    /// verbatim — no conversion) on `stream_name`, in version order — `$`-prefixed technical rows
    /// included (the predicate is on the real `version` column, so it truncates correctly regardless
    /// of how many technical rows occupy slots in the range; they are dropped by [`decode_rows`],
    /// never by this query). Kept `pub` ONLY because the DB-gated benchmark test (a separate crate)
    /// needs to time this leg directly — not a general-purpose `load_upto`.
    pub async fn fetch_rows(
        &self,
        stream_name: &str,
        version: CatalogVersion,
    ) -> Result<Vec<RawRow>, DomainError> {
        let ceiling = i32::try_from(version.get()).map_err(db_err)?;
        let rows = sqlx::query(
            "SELECT version, event_type, payload FROM domain_events \
             WHERE stream_name = $1 AND version <= $2 ORDER BY version",
        )
        .bind(stream_name)
        .bind(ceiling)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let row_version: i32 = row.try_get("version").map_err(db_err)?;
            let event_type: String = row.try_get("event_type").map_err(db_err)?;
            let payload: serde_json::Value = row.try_get("payload").map_err(db_err)?;
            out.push((row_version as i64, event_type, payload));
        }
        Ok(out)
    }

    /// The decode leg alone: business events only (`$`-prefixed technical rows are skipped, same
    /// EventStore convention `load_inner` follows — ADR-20260731-160000 §5), paired with their OWN
    /// stream version — never a slice position (round 2: this is what makes
    /// `AsOfCatalog::from_stream`'s per-event filtering possible at all). `DomainEvent`'s
    /// adjacently-tagged wire shape (`#[serde(tag = "eventType", content = "payload")]`) needs the
    /// two-key envelope `{"eventType": .., "payload": ..}` — the payload is MOVED into it, never
    /// cloned, matching `load_inner`'s own `json!({"eventType": event_type, "payload": payload})`.
    pub fn decode_rows(rows: Vec<RawRow>) -> Result<Vec<(CatalogVersion, DomainEvent)>, DomainError> {
        let mut events = Vec::with_capacity(rows.len());
        for (row_version, event_type, payload) in rows {
            if event_type.starts_with('$') {
                continue;
            }
            let event: DomainEvent = serde_json::from_value(serde_json::json!({
                "eventType": event_type,
                "payload": payload,
            }))
            .map_err(|e| db_err(format!("{event_type}: {e}")))?;
            let version = CatalogVersion::try_new(row_version).ok_or_else(|| {
                db_err(format!("non-positive stream version {row_version} for {event_type}"))
            })?;
            events.push((version, event));
        }
        Ok(events)
    }

    /// The SQL + decode legs, PLUS the fail-closed coordinate check — never the span itself (round 3:
    /// the span now brackets this AND the fold, built by [`AsOfPriceAuthority::as_of`], because a
    /// span this function owned and closed before returning could never cover work its caller does
    /// after it returns). Fails CLOSED (round 2, vernon B1/young B1/obs B6): if the highest version
    /// [`fetch_rows`] returned (technical rows included) is not exactly the requested coordinate, the
    /// coordinate is absent or beyond head, and this returns `Err` rather than silently answering
    /// with whatever the range happened to contain.
    async fn load_range(
        &self,
        stream: &str,
        version: CatalogVersion,
    ) -> Result<(Vec<(CatalogVersion, DomainEvent)>, usize), DomainError> {
        let rows = self.fetch_rows(stream, version).await?;
        let stream_length = rows.len();
        let highest = rows.iter().map(|(v, _, _)| *v).max();
        if highest != Some(version.get()) {
            return Err(db_err(format!(
                "coordinate {} is absent or beyond head on stream {stream} (highest available \
                 version: {highest:?})",
                version.get()
            )));
        }
        let events = Self::decode_rows(rows)?;
        Ok((events, stream_length))
    }
}

#[async_trait]
impl AsOfPriceAuthority for PgAsOfCatalogRepository {
    /// Both legs AND the fold, instrumented at this adapter seam (never in `pricing.rs`/
    /// `crates/domain` — obs consent): the span covers the read END TO END — SQL + decode + the
    /// fail-closed check + [`AsOfCatalog::from_stream`]'s own fold — so `catalog.as_of.fold`'s name
    /// is true (round 3 correction: round 1's span was dropped before the fold ever ran; round 2's
    /// span covered SQL+decode but still closed before the fold, which runs in this method, one
    /// level up from where round 2's `.instrument` sat).
    async fn as_of(
        &self,
        catalog_id: CatalogId,
        version: CatalogVersion,
    ) -> Result<AsOfCatalog, DomainError> {
        let stream = domain::catalog::stream(catalog_id);
        let span = telemetry::spans::catalog_as_of_fold(&catalog_id.0.to_string(), version.get());
        let span_for_record = span.clone();
        async move {
            let (events, stream_length) = self.load_range(&stream, version).await?;
            // `load_range` already fails closed when the range is short of `version`, so every
            // event here has its own version <= `version` by construction; `from_stream`'s own
            // per-event-version filter is still applied (defence in depth, never dead code at this
            // call site).
            let catalog = AsOfCatalog::from_stream(&events, version);
            telemetry::spans::record_catalog_as_of_fold(&span_for_record, stream_length, events.len());
            Ok(catalog)
        }
        .instrument(span)
        .await
    }
}
