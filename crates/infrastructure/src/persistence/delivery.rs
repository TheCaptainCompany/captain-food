//! sqlx read-model repository over the `View_DeliveryJob` SQL view (ADR-0031/0039) — the delivery
//! operational board, projected ON READ as a state-fold over `domain_events` (created by
//! migrations/20260717120000 from the generated views SQL). Backs the `delivery` / `myDeliveries` /
//! `restaurantDeliveries` GraphQL queries via `application::queries::DeliveryReadRepository`.
//!
//! Column conventions match the materialized stores (ADR-0037): `status`/`provider` come back as
//! INTEGER ordinals (the generated view folds them with declaration-order CASE ladders), addresses and
//! the courier as jsonb, `partner_ref` as text.

use application::queries::{DeliveryJobRow, DeliveryReadRepository};
use async_trait::async_trait;
use domain::generated::scalars::{
    DeliveryJobId, DeliveryStatus, ExternalReference, OrderId, RestaurantId, RiderId,
};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Postgres, QueryBuilder, Row};

use super::db_err;
use super::enum_sql::{opt_from_text, EnumText};

/// The view columns the read side consumes, in `DeliveryJobRow` field order (the view also carries
/// `last_partner_rejection`/`created_at`/`updated_at`, which the API does not expose).
const COLUMNS: &str = "delivery_job_id, order_id, restaurant_id, status, provider, rider_id, \
     courier, partner_ref, pickup_address, dropoff_address, estimated_pickup_at, \
     estimated_dropoff_at, requested_at, picked_up_at, delivered_at, open_issue_kind, \
     food_location, handed_back_at";

/// Unquoted `CREATE VIEW View_DeliveryJob` folds to this identifier in Postgres.
const VIEW: &str = "view_deliveryjob";

/// Normalize a nullable jsonb: a JSON `null` in the column means "no value".
fn opt_json(v: Option<serde_json::Value>) -> Option<serde_json::Value> {
    v.filter(|j| !j.is_null())
}

/// Decode one `View_DeliveryJob` row into the hand-written read-model DTO.
fn decode(row: &PgRow) -> Result<DeliveryJobRow, DomainError> {
    Ok(DeliveryJobRow {
        delivery_job_id: DeliveryJobId(row.try_get("delivery_job_id").map_err(db_err)?),
        order_id: OrderId(row.try_get("order_id").map_err(db_err)?),
        restaurant_id: RestaurantId(row.try_get("restaurant_id").map_err(db_err)?),
        status: EnumText::from_text(&row.try_get::<String, _>("status").map_err(db_err)?)?,
        provider: opt_from_text(row.try_get("provider").map_err(db_err)?)?,
        rider_id: row.try_get::<Option<uuid::Uuid>, _>("rider_id").map_err(db_err)?.map(RiderId),
        courier: opt_json(row.try_get("courier").map_err(db_err)?),
        partner_ref: row
            .try_get::<Option<String>, _>("partner_ref")
            .map_err(db_err)?
            .map(ExternalReference),
        pickup_address: row.try_get("pickup_address").map_err(db_err)?,
        dropoff_address: row.try_get("dropoff_address").map_err(db_err)?,
        estimated_pickup_at: row.try_get("estimated_pickup_at").map_err(db_err)?,
        estimated_dropoff_at: row.try_get("estimated_dropoff_at").map_err(db_err)?,
        requested_at: row.try_get("requested_at").map_err(db_err)?,
        picked_up_at: row.try_get("picked_up_at").map_err(db_err)?,
        delivered_at: row.try_get("delivered_at").map_err(db_err)?,
        open_issue_kind: opt_from_text(row.try_get("open_issue_kind").map_err(db_err)?)?,
        food_location: opt_from_text(row.try_get("food_location").map_err(db_err)?)?,
        handed_back_at: row.try_get("handed_back_at").map_err(db_err)?,
    })
}

/// Postgres adapter for the DeliveryJob read model (the `View_DeliveryJob` fold view).
pub struct PgDeliveryRepository {
    pool: PgPool,
}

impl PgDeliveryRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DeliveryReadRepository for PgDeliveryRepository {
    async fn by_order(&self, order_id: OrderId) -> Result<Option<DeliveryJobRow>, DomainError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM {VIEW} WHERE order_id = $1 ORDER BY requested_at DESC LIMIT 1"
        );
        let row = sqlx::query(&sql)
            .bind(order_id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        row.as_ref().map(decode).transpose()
    }

    /// The rider's assigned jobs OR the available pool (PENDING, unassigned) — `myDeliveries`'
    /// "assigned/available" semantics — with `status` narrowing the union when given (e.g. PENDING →
    /// only the available pool, ASSIGNED → only theirs).
    async fn for_rider(
        &self,
        rider_id: RiderId,
        status: Option<DeliveryStatus>,
    ) -> Result<Vec<DeliveryJobRow>, DomainError> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new(format!("SELECT {COLUMNS} FROM {VIEW} WHERE (rider_id = "));
        qb.push_bind(rider_id.0)
            .push(" OR (status = ")
            .push_bind(DeliveryStatus::PENDING.to_text())
            .push(" AND rider_id IS NULL))");
        if let Some(status) = status {
            qb.push(" AND status = ").push_bind(status.to_text());
        }
        qb.push(" ORDER BY requested_at DESC");
        let rows = qb.build().fetch_all(&self.pool).await.map_err(db_err)?;
        rows.iter().map(decode).collect()
    }

    /// #639 part C step 4-ii (ADR-20260904-124600 §5, #879): the SQL `held_by_rider` narrows —
    /// one row, `rider_id = $1 AND status IN (ASSIGNED, PICKED_UP, OUT_FOR_DELIVERY)`, never the
    /// `for_rider(..).find(..)` scan over the rider's whole history plus the unassigned PENDING
    /// pool `for_rider` exists to serve `myDeliveries`.
    async fn held_by_rider(&self, rider_id: RiderId) -> Result<Option<DeliveryJobRow>, DomainError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM {VIEW} WHERE rider_id = $1 AND status IN ($2, $3, $4) \
             ORDER BY requested_at DESC LIMIT 1"
        );
        let row = sqlx::query(&sql)
            .bind(rider_id.0)
            .bind(DeliveryStatus::ASSIGNED.to_text())
            .bind(DeliveryStatus::PICKED_UP.to_text())
            .bind(DeliveryStatus::OUT_FOR_DELIVERY.to_text())
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        row.as_ref().map(decode).transpose()
    }

    async fn by_restaurant(
        &self,
        restaurant_id: RestaurantId,
        status: Option<DeliveryStatus>,
    ) -> Result<Vec<DeliveryJobRow>, DomainError> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new(format!("SELECT {COLUMNS} FROM {VIEW} WHERE restaurant_id = "));
        qb.push_bind(restaurant_id.0);
        if let Some(status) = status {
            qb.push(" AND status = ").push_bind(status.to_text());
        }
        qb.push(" ORDER BY requested_at DESC");
        let rows = qb.build().fetch_all(&self.pool).await.map_err(db_err)?;
        rows.iter().map(decode).collect()
    }
}
