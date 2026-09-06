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

    /// The unbounded range read: EVERY row of `stream_name`, in version order — the leg
    /// [`AsOfPriceAuthority::at_head`] needs (PROP-20260831-134539 slice 3a, D2), where the
    /// coordinate is NOT known before reading, unlike [`Self::fetch_rows`], which bounds at a
    /// caller-KNOWN coordinate and fails closed against it. Never a `latest_version()`-style
    /// separate lookup: this is the ONE read at-head performs, and the coordinate it returns is
    /// derived from these SAME rows (the highest raw version among them), never a second query.
    /// Kept `pub` for the same benchmark reason as [`Self::fetch_rows`]/[`Self::decode_rows`].
    pub async fn fetch_all_rows(&self, stream_name: &str) -> Result<Vec<RawRow>, DomainError> {
        let rows = sqlx::query(
            "SELECT version, event_type, payload FROM domain_events \
             WHERE stream_name = $1 ORDER BY version",
        )
        .bind(stream_name)
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
    /// with whatever the range happened to contain — recording the refusal onto the caller's span
    /// (round 3, obs NB2) before returning, since a bare `Err` propagated up leaves the span with no
    /// `otel.status_code` and a refusal indistinguishable from a success.
    async fn load_range(
        &self,
        stream: &str,
        version: CatalogVersion,
        span: &tracing::Span,
    ) -> Result<(Vec<(CatalogVersion, DomainEvent)>, usize), DomainError> {
        let rows = self.fetch_rows(stream, version).await?;
        let stream_length = rows.len();
        let highest = rows.iter().map(|(v, _, _)| *v).max();
        if highest != Some(version.get()) {
            telemetry::spans::record_catalog_as_of_fold_error(span, "coordinate_beyond_head");
            return Err(db_err(format!(
                "coordinate {} is absent or beyond head on stream {stream} (highest available \
                 version: {highest:?})",
                version.get()
            )));
        }
        let events = Self::decode_rows(rows)?;
        Ok((events, stream_length))
    }

    /// The unbounded leg for [`AsOfPriceAuthority::at_head`] (slice 3a, D2): read EVERY row of
    /// `stream`, then take the CEILING as the max over the RAW rows returned (technical rows
    /// included, before [`Self::decode_rows`] drops them) — the SAME technique
    /// [`Self::load_range`]'s fail-closed check uses, repurposed here as the SOURCE of the
    /// coordinate rather than as a verification against a caller-supplied number, because at-head
    /// there is no separately-requested version to check against: whatever the highest row is IS
    /// the head, by construction of reading everything. `Err` when the stream has no rows at all —
    /// the catalog does not exist yet — never a HEAD price for a catalog that was never created.
    ///
    /// Instrumented at [`AsOfPriceAuthority::at_head`], the ONE caller (deliverable 4, D5): this
    /// leg only computes what `at_head` records — `payload_bytes` is the SUM of the raw JSON
    /// payload sizes over EVERY row returned (technical rows included, before
    /// [`Self::decode_rows`] drops them), because L's cost is bytes, not length (dba: one
    /// 500-product `CatalogImported` resync is ~200 KB in 0.05% of L).
    ///
    /// **`payload_bytes` is read straight off Postgres, never re-serialized per row** (round 2, the
    /// PROP-20260831-134539 §12 payload_bytes lever, HEAD of the cheapest-lever order): the
    /// previous form called `serde_json::to_vec` on every decoded `serde_json::Value` to re-measure
    /// what Postgres had already told it, and that re-serialization — instrumentation living INSIDE
    /// the timed leg, in PRODUCTION code — decomposed to roughly a third of the native `at_head`
    /// cost at L=2,000 (~28 ms of ~114 ms). [`Self::fetch_all_rows_with_byte_total`] asks Postgres
    /// for the sum instead (`sum(octet_length(payload::text)) OVER ()`, in the SAME query), so no
    /// row is ever turned back into bytes in Rust.
    async fn load_to_head(
        &self,
        stream: &str,
    ) -> Result<(Vec<(CatalogVersion, DomainEvent)>, usize, CatalogVersion, usize), DomainError> {
        let (rows, payload_bytes) = self.fetch_all_rows_with_byte_total(stream).await?;
        let stream_length = rows.len();
        let highest = rows.iter().map(|(v, _, _)| *v).max();
        let Some(coordinate) = highest.and_then(CatalogVersion::try_new) else {
            return Err(db_err(format!(
                "stream {stream} has no rows: catalog not created (highest available version: \
                 {highest:?})"
            )));
        };
        let events = Self::decode_rows(rows)?;
        Ok((events, stream_length, coordinate, payload_bytes))
    }

    /// [`Self::fetch_all_rows`]'s SAME rows, plus the total payload byte count Postgres computes in
    /// the SAME query (`sum(octet_length(payload::text)) OVER ()`, one extra column expression on
    /// every row, never a second round trip and never a second query). Kept SEPARATE from
    /// `fetch_all_rows` on purpose: the DB-gated benchmark times `fetch_all_rows` directly as "the
    /// SQL leg alone" (`crates/infrastructure/tests/as_of_catalog_read.rs`), and that shape must
    /// stay exactly what it always was — widening its row shape would move the goalposts under an
    /// existing, unrelated measurement. `pub` for the SAME reason `fetch_rows`/`fetch_all_rows`/
    /// `decode_rows` are: the DB-gated test needs to verify the reported byte total directly,
    /// against `sum(octet_length(payload::text))` computed independently.
    pub async fn fetch_all_rows_with_byte_total(
        &self,
        stream_name: &str,
    ) -> Result<(Vec<RawRow>, usize), DomainError> {
        let rows = sqlx::query(
            "SELECT version, event_type, payload, \
             sum(octet_length(payload::text)) OVER () AS total_bytes \
             FROM domain_events WHERE stream_name = $1 ORDER BY version",
        )
        .bind(stream_name)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let mut out = Vec::with_capacity(rows.len());
        let mut total_bytes: usize = 0;
        for row in &rows {
            let row_version: i32 = row.try_get("version").map_err(db_err)?;
            let event_type: String = row.try_get("event_type").map_err(db_err)?;
            let payload: serde_json::Value = row.try_get("payload").map_err(db_err)?;
            out.push((row_version as i64, event_type, payload));
        }
        if let Some(first) = rows.first() {
            let bytes: i64 = first.try_get("total_bytes").map_err(db_err)?;
            total_bytes = bytes.max(0) as usize;
        }
        Ok((out, total_bytes))
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
            let (events, stream_length) = self.load_range(&stream, version, &span_for_record).await?;
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

    /// ONE unbounded range read to head (slice 3a, D2): the coordinate carried is the CEILING the
    /// fold was bounded at — the highest RAW row version returned by [`Self::load_to_head`],
    /// verified over the same rows that produced the prices, never a second, separately-checked
    /// value. `Err` when the stream has no rows at all (the catalog does not exist yet) — never a
    /// HEAD price for a catalog that was never created.
    ///
    /// Instrumented end to end (deliverable 4, D5): `catalog.as_of.fold` joins the `cart-price`
    /// contract's spans, `catalog_as_of_fold_ms`/`catalog_as_of_stream_length`/
    /// `catalog_as_of_payload_bytes` join its metrics, and `catalog_as_of_reads_total{outcome}` is
    /// the dead-man — `correlation_id` is this read's own first real value (recorded IMMEDIATELY,
    /// the observability HARD STOP).
    async fn at_head(
        &self,
        catalog_id: CatalogId,
        correlation_id: uuid::Uuid,
    ) -> Result<(AsOfCatalog, CatalogVersion), DomainError> {
        let stream = domain::catalog::stream(catalog_id);
        let span = telemetry::spans::catalog_as_of_fold_at_head(
            &catalog_id.0.to_string(),
            &correlation_id.to_string(),
        );
        let span_for_record = span.clone();
        let started = std::time::Instant::now();
        let outcome = async move {
            let (events, stream_length, coordinate, payload_bytes) =
                self.load_to_head(&stream).await.map_err(|e| {
                    telemetry::spans::record_catalog_as_of_fold_error(
                        &span_for_record,
                        "catalog_not_created",
                    );
                    e
                })?;
            let catalog = AsOfCatalog::from_stream(&events, coordinate);
            telemetry::spans::record_catalog_as_of_fold(&span_for_record, stream_length, events.len());
            telemetry::spans::record_catalog_as_of_fold_version(&span_for_record, coordinate.get());
            telemetry::meters::catalog_as_of::stream_length(stream_length as f64);
            telemetry::meters::catalog_as_of::payload_bytes(payload_bytes as f64);
            Ok((catalog, coordinate))
        }
        .instrument(span)
        .await;
        telemetry::meters::catalog_as_of::fold_duration(started.elapsed().as_secs_f64() * 1000.0);
        telemetry::meters::catalog_as_of::reads_total(if outcome.is_ok() { "applied" } else { "refused" });
        outcome
    }
}
