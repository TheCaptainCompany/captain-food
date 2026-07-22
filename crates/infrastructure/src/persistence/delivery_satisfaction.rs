//! sqlx read-model repository over the `View_DeliverySatisfaction` SQL view (ADR-0039, #62) — the
//! delivery-delay satisfaction answers, projected ON READ as a state-fold over `domain_events`
//! (`DeliverySatisfactionRecorded` on the Order stream; created by the migrations from the generated
//! views SQL). Backs the `restaurantDeliverySatisfaction` GraphQL query via
//! `application::queries::DeliverySatisfactionReadRepository`.
//!
//! Column conventions match the materialized stores (ADR-0037): `timeliness` comes back as its INTEGER
//! ordinal (the generated view folds it with a declaration-order CASE ladder); `reason` is a nullable
//! TEXT column.

use application::queries::{DeliverySatisfactionReadRepository, DeliverySatisfactionRow};
use async_trait::async_trait;
use domain::generated::scalars::{DeliveryDissatisfactionReason, OrderId, RestaurantId};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Postgres, QueryBuilder, Row};

use super::db_err;
use super::enum_sql::EnumOrd;

/// The view columns the read side consumes, in `DeliverySatisfactionRow` field order (the view also
/// carries `created_at`/`updated_at`, which the API does not expose).
const COLUMNS: &str = "order_id, restaurant_id, timeliness, reason, recorded_at";

/// Unquoted `CREATE VIEW View_DeliverySatisfaction` folds to this identifier in Postgres.
const VIEW: &str = "view_deliverysatisfaction";

/// Decode one `View_DeliverySatisfaction` row into the hand-written read-model DTO.
fn decode(row: &PgRow) -> Result<DeliverySatisfactionRow, DomainError> {
    Ok(DeliverySatisfactionRow {
        order_id: OrderId(row.try_get("order_id").map_err(db_err)?),
        restaurant_id: RestaurantId(row.try_get("restaurant_id").map_err(db_err)?),
        timeliness: EnumOrd::from_ord(row.try_get::<i32, _>("timeliness").map_err(db_err)?)?,
        reason: row
            .try_get::<Option<String>, _>("reason")
            .map_err(db_err)?
            .map(DeliveryDissatisfactionReason),
        recorded_at: row.try_get("recorded_at").map_err(db_err)?,
    })
}

/// Postgres adapter for the delivery-satisfaction read model (the `View_DeliverySatisfaction` fold view).
pub struct PgDeliverySatisfactionRepository {
    pool: PgPool,
}

impl PgDeliverySatisfactionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DeliverySatisfactionReadRepository for PgDeliverySatisfactionRepository {
    /// The restaurant's delivery-satisfaction answers, newest-first; optionally filtered to one
    /// timeliness verdict.
    async fn by_restaurant(
        &self,
        restaurant_id: RestaurantId,
        timeliness: Option<domain::generated::scalars::DeliveryTimeliness>,
    ) -> Result<Vec<DeliverySatisfactionRow>, DomainError> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new(format!("SELECT {COLUMNS} FROM {VIEW} WHERE restaurant_id = "));
        qb.push_bind(restaurant_id.0);
        if let Some(timeliness) = timeliness {
            qb.push(" AND timeliness = ").push_bind(timeliness.to_ord());
        }
        qb.push(" ORDER BY recorded_at DESC");
        let rows = qb.build().fetch_all(&self.pool).await.map_err(db_err)?;
        rows.iter().map(decode).collect()
    }
}
