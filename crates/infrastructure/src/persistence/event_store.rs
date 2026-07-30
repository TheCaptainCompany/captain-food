//! Postgres adapter for the `application::ports::EventStore` write port (ADR-0035): appends business
//! events to the append-only `domain_events` log. The technical envelope (event id, stream/version,
//! acting user, correlation, occurred_at) is stamped HERE — payloads stay business-only (ADR-0041).
//! Optimistic concurrency rides the UNIQUE(stream_name, version) constraint: a clash maps to the
//! canonical `version_conflict` DomainError so handlers can absorb idempotent replays.

use application::ports::{version_conflict, Actor, EventStore};
use async_trait::async_trait;
use domain::generated::events::DomainEvent;
use domain::shared::errors::DomainError;
use sqlx::{PgPool, Row};
use tracing::Instrument as _;

use crate::persistence::db_err;
use crate::persistence::event_bus::{AppendedEvent, EventBus};

pub struct PgEventStore {
    pool: PgPool,
    /// Optional in-process fan-out: when present, every successfully committed append is published as
    /// a lightweight [`AppendedEvent`] envelope (AFTER the commit — a notification, never a write
    /// dependency). `None` keeps the store silent (workers/tests that need no push).
    bus: Option<EventBus>,
}

impl PgEventStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool, bus: None }
    }

    /// A store that also broadcasts each committed append on `bus` (the GraphQL subscription feed).
    pub fn with_bus(pool: PgPool, bus: EventBus) -> Self {
        Self { pool, bus: Some(bus) }
    }
}

#[async_trait]
impl EventStore for PgEventStore {
    async fn append(
        &self,
        stream_name: &str,
        expected_version: i64,
        events: &[DomainEvent],
        actor: &Actor,
    ) -> Result<i64, DomainError> {
        // Split every event ONCE, up front. This used to happen inside the insert loop; hoisting it
        // means the `event.store.append` span can name `business.event_type` (the contract requires it,
        // and it is only knowable after serialization) without serializing each event a second time.
        let split: Vec<(String, serde_json::Value)> =
            events.iter().map(split_event).collect::<Result<_, _>>()?;

        // event.store.append (INTERNAL). `business.event_type` names the FIRST event of the batch —
        // the contract's attribute is singular while an append is one transaction over possibly several
        // events, so `event_count` carries the rest. Naming the first is what makes the span findable
        // for the event that drove the command; the count stops that reading as "there was only one".
        let span = telemetry::spans::event_store_append(
            split.first().map(|(t, _)| t.as_str()).unwrap_or("<empty>"),
            stream_name,
        );
        telemetry::spans::record_event_count(&span, split.len());

        // `.instrument` rather than `span.enter()`: this body awaits (begin, insert, commit), and a
        // guard held across an await re-parents whatever else that worker thread picks up.
        self.append_instrumented(stream_name, expected_version, split, actor).instrument(span).await
    }

    async fn load(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError> {
        self.load_inner(stream_name).await
    }
}

impl PgEventStore {
    /// The append body, separated so the public `append` stays the instrumentation boundary.
    async fn append_instrumented(
        &self,
        stream_name: &str,
        expected_version: i64,
        split: Vec<(String, serde_json::Value)>,
        actor: &Actor,
    ) -> Result<i64, DomainError> {
        let event_count = split.len();
        // One transaction per append: a multi-event emission lands atomically or not at all, and a
        // version clash on ANY row rolls the whole batch back.
        let mut tx = self.pool.begin().await.map_err(db_err)?;

        // Envelopes to broadcast once (and only if) the transaction commits.
        let mut published: Vec<AppendedEvent> = Vec::new();

        for (index, (event_type, payload)) in split.into_iter().enumerate() {
            let version = expected_version + index as i64 + 1;
            if self.bus.is_some() {
                published.push(AppendedEvent {
                    stream_name: stream_name.to_owned(),
                    event_type: event_type.clone(),
                    correlation_id: actor.correlation_id,
                    position: version,
                });
            }

            let insert = sqlx::query(
                "INSERT INTO domain_events \
                 (id, stream_name, version, user_id, user_type, correlation_id, cause_id, \
                  event_type, payload, metadata, occurred_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULL, now())",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(stream_name)
            .bind(i32::try_from(version).map_err(db_err)?)
            .bind(actor.user_id)
            .bind(actor.user_type.clone())
            .bind(actor.correlation_id)
            .bind(actor.cause_id)
            .bind(&event_type)
            .bind(payload)
            .execute(&mut *tx)
            .await;

            if let Err(e) = insert {
                // The event id is a fresh v4 UUID, so a unique violation here is (in practice) the
                // (stream_name, version) guard — i.e. we lost the optimistic-concurrency race.
                if is_unique_violation(&e) {
                    return Err(version_conflict(stream_name, expected_version));
                }
                return Err(db_err(e));
            }
        }

        tx.commit().await.map_err(db_err)?;
        // Publish AFTER the commit: subscribers only ever hear about durable facts. Best effort —
        // no subscribers / a closed channel is a no-op (the bus is a notification, not a ledger).
        if let Some(bus) = &self.bus {
            for envelope in published {
                // event.publish (PRODUCER), one span PER envelope rather than one for the batch: the
                // contract binds the span to `business.event_type`, and a single span covering
                // OrderPlaced + CartCheckedOut could only name one of the two.
                let publish_span = telemetry::spans::event_publish(&envelope.event_type);
                publish_span.in_scope(|| bus.publish(envelope));
            }
        }
        Ok(expected_version + event_count as i64)
    }

    /// The load body. Not instrumented as a contract span: no contract declares one for a stream read,
    /// and `c4-l3.yaml` scopes `event-store-adapter` instrumentation to the append path.
    async fn load_inner(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError> {
        let rows = sqlx::query(
            "SELECT event_type, payload, version FROM domain_events \
             WHERE stream_name = $1 ORDER BY version",
        )
        .bind(stream_name)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let mut events = Vec::with_capacity(rows.len());
        let mut version: i64 = 0;
        for row in rows {
            let event_type: String = row.try_get("event_type").map_err(db_err)?;
            let payload: serde_json::Value = row.try_get("payload").map_err(db_err)?;
            let row_version: i32 = row.try_get("version").map_err(db_err)?;
            version = version.max(i64::from(row_version));
            // Rebuild the typed event from the (event_type, payload) columns via the adjacent tag —
            // the same envelope-join the projection worker uses.
            let event: DomainEvent = serde_json::from_value(serde_json::json!({
                "eventType": event_type,
                "payload": payload,
            }))
            .map_err(|e| db_err(format!("{stream_name} v{row_version} ({event_type}): {e}")))?;
            events.push(event);
        }
        Ok((events, version))
    }
}

/// Split the adjacently-tagged [`DomainEvent`] (`{"eventType": …, "payload": …}`) into the
/// `event_type` + `payload` columns of `domain_events`.
fn split_event(event: &DomainEvent) -> Result<(String, serde_json::Value), DomainError> {
    let tagged = serde_json::to_value(event).map_err(db_err)?;
    let event_type = tagged
        .get("eventType")
        .and_then(|t| t.as_str())
        .ok_or_else(|| db_err("DomainEvent serialized without an eventType tag"))?
        .to_owned();
    // Unit-payload variants would serialize without `payload`; store `{}` rather than SQL NULL
    // (the column is NOT NULL).
    let payload = tagged.get("payload").cloned().unwrap_or_else(|| serde_json::json!({}));
    Ok((event_type, payload))
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.is_unique_violation())
}
