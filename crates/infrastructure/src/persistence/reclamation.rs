//! sqlx read-model repository over the `View_Reclamation` SQL view (#154, ADR-0039) — customer
//! claims/disputes, projected ON READ as a state-fold over `domain_events` (ReclamationOpened /
//! Resolved / Rejected / Reopened on the Reclamation stream; created by the migrations from the
//! generated views SQL). Backs the `myReclamations` / `restaurantReclamations` / `reclamation`
//! GraphQL queries via `application::queries::ReclamationReadRepository`.
//!
//! Column conventions match the other view repos (ADR-0037): the enum columns come back as their
//! INTEGER ordinal (the generated view folds `status` with a declaration-order CASE ladder); the
//! optional refund Money value object splits into `refund_amount_cents` (BIGINT) + `currency` (TEXT).

use application::queries::{ReclamationFilter, ReclamationReadRepository, ReclamationRow};
use async_trait::async_trait;
use domain::generated::scalars::{
    CurrencyCode, CustomerId, MoneyCents, OrderId, ReclamationDescription, ReclamationId,
    ReclamationReason, RestaurantId,
};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Postgres, QueryBuilder, Row};

use super::db_err;
use super::enum_sql::{opt_from_ord, EnumOrd};

/// The view columns the read side consumes, in `ReclamationRow` field order.
const COLUMNS: &str = "reclamation_id, order_id, customer_id, restaurant_id, category, description, \
     requested_resolution, status, resolution, refund_amount_cents, currency, reject_reason, \
     opened_at, decided_at";

/// Unquoted `CREATE VIEW View_Reclamation` folds to this identifier in Postgres.
const VIEW: &str = "view_reclamation";

/// Decode one `View_Reclamation` row into the hand-written read-model DTO.
fn decode(row: &PgRow) -> Result<ReclamationRow, DomainError> {
    Ok(ReclamationRow {
        reclamation_id: ReclamationId(row.try_get("reclamation_id").map_err(db_err)?),
        order_id: OrderId(row.try_get("order_id").map_err(db_err)?),
        customer_id: CustomerId(row.try_get("customer_id").map_err(db_err)?),
        restaurant_id: RestaurantId(row.try_get("restaurant_id").map_err(db_err)?),
        category: EnumOrd::from_ord(row.try_get::<i32, _>("category").map_err(db_err)?)?,
        description: ReclamationDescription(row.try_get("description").map_err(db_err)?),
        requested_resolution: opt_from_ord(
            row.try_get::<Option<i32>, _>("requested_resolution").map_err(db_err)?,
        )?,
        status: EnumOrd::from_ord(row.try_get::<i32, _>("status").map_err(db_err)?)?,
        resolution: opt_from_ord(row.try_get::<Option<i32>, _>("resolution").map_err(db_err)?)?,
        refund_amount_cents: row
            .try_get::<Option<i64>, _>("refund_amount_cents")
            .map_err(db_err)?
            .map(MoneyCents),
        currency: row.try_get::<Option<String>, _>("currency").map_err(db_err)?.map(CurrencyCode),
        reject_reason: row
            .try_get::<Option<String>, _>("reject_reason")
            .map_err(db_err)?
            .map(ReclamationReason),
        opened_at: row.try_get("opened_at").map_err(db_err)?,
        decided_at: row.try_get("decided_at").map_err(db_err)?,
    })
}

/// Postgres adapter for the reclamation read model (the `View_Reclamation` fold view).
pub struct PgReclamationRepository {
    pool: PgPool,
}

impl PgReclamationRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ReclamationReadRepository for PgReclamationRepository {
    /// A customer's own reclamations, newest-first (the `myReclamations` list).
    async fn by_customer(
        &self,
        customer_id: CustomerId,
    ) -> Result<Vec<ReclamationRow>, DomainError> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new(format!("SELECT {COLUMNS} FROM {VIEW} WHERE customer_id = "));
        qb.push_bind(customer_id.0);
        qb.push(" ORDER BY opened_at DESC");
        let rows = qb.build().fetch_all(&self.pool).await.map_err(db_err)?;
        rows.iter().map(decode).collect()
    }

    /// The restaurant/admin claims queue, newest-first, honouring the status/category filter
    /// (status OPEN = the outstanding queue). Restaurant/ownership narrowing is a recorded follow-up
    /// gap (no restaurant principal in the GraphQL context yet — same as the EXTERNAL partner queue).
    async fn list(&self, filter: ReclamationFilter) -> Result<Vec<ReclamationRow>, DomainError> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new(format!("SELECT {COLUMNS} FROM {VIEW} WHERE TRUE"));
        if let Some(status) = filter.status {
            qb.push(" AND status = ").push_bind(status.to_ord());
        }
        if let Some(category) = filter.category {
            qb.push(" AND category = ").push_bind(category.to_ord());
        }
        qb.push(" ORDER BY opened_at DESC");
        let rows = qb.build().fetch_all(&self.pool).await.map_err(db_err)?;
        rows.iter().map(decode).collect()
    }

    /// A single reclamation by id (claim detail); `None` when unknown.
    async fn by_id(&self, id: ReclamationId) -> Result<Option<ReclamationRow>, DomainError> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new(format!("SELECT {COLUMNS} FROM {VIEW} WHERE reclamation_id = "));
        qb.push_bind(id.0);
        let row = qb.build().fetch_optional(&self.pool).await.map_err(db_err)?;
        row.as_ref().map(decode).transpose()
    }
}
