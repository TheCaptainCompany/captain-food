//! sqlx read-model repository over the `View_PendingRefunds` SQL view (ADR-0039) — the refund queue,
//! projected ON READ as a state-fold over `domain_events` (RefundOpened / RefundApproved /
//! RefundDenied / PaymentRefunded on the Payment stream; created by the migrations from the generated
//! views SQL). Backs the `pendingRefunds` GraphQL query via
//! `application::queries::RefundReadRepository`.
//!
//! Column conventions match the materialized stores (ADR-0037): `status` comes back as its INTEGER
//! ordinal (the generated view folds it with a declaration-order CASE ladder); the Money value object
//! splits into `amount_cents` (BIGINT) + `currency` (TEXT).

use application::queries::{RefundFilter, RefundReadRepository, RefundRow};
use async_trait::async_trait;
use domain::generated::scalars::{CurrencyCode, MoneyCents, OrderId, RefundId, RestaurantId};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Postgres, QueryBuilder, Row};

use super::db_err;
use super::enum_sql::EnumOrd;

/// The view columns the read side consumes, in `RefundRow` field order (the view also carries
/// `created_at`/`updated_at`, which the API does not expose).
const COLUMNS: &str = "order_id, restaurant_id, status, amount_cents, currency, \
     approved_amount_cents, reason, refund_id, requested_at, decided_at";

/// Unquoted `CREATE VIEW View_PendingRefunds` folds to this identifier in Postgres.
const VIEW: &str = "view_pendingrefunds";

/// Decode one `View_PendingRefunds` row into the hand-written read-model DTO.
fn decode(row: &PgRow) -> Result<RefundRow, DomainError> {
    Ok(RefundRow {
        order_id: OrderId(row.try_get("order_id").map_err(db_err)?),
        restaurant_id: RestaurantId(row.try_get("restaurant_id").map_err(db_err)?),
        status: EnumOrd::from_ord(row.try_get::<i32, _>("status").map_err(db_err)?)?,
        amount_cents: MoneyCents(row.try_get("amount_cents").map_err(db_err)?),
        currency: CurrencyCode(row.try_get("currency").map_err(db_err)?),
        approved_amount_cents: row
            .try_get::<Option<i64>, _>("approved_amount_cents")
            .map_err(db_err)?
            .map(MoneyCents),
        reason: row.try_get("reason").map_err(db_err)?,
        refund_id: row.try_get::<Option<String>, _>("refund_id").map_err(db_err)?.map(RefundId),
        requested_at: row.try_get("requested_at").map_err(db_err)?,
        decided_at: row.try_get("decided_at").map_err(db_err)?,
    })
}

/// Postgres adapter for the refund-queue read model (the `View_PendingRefunds` fold view).
pub struct PgRefundQueueRepository {
    pool: PgPool,
}

impl PgRefundQueueRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RefundReadRepository for PgRefundQueueRepository {
    /// The refund queue, newest-request-first: the restaurant's own orders when `restaurant_id` is
    /// given, and `status = REQUESTED` for the pending, awaiting-decision subset.
    async fn list(&self, filter: RefundFilter) -> Result<Vec<RefundRow>, DomainError> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new(format!("SELECT {COLUMNS} FROM {VIEW} WHERE TRUE"));
        if let Some(restaurant_id) = filter.restaurant_id {
            qb.push(" AND restaurant_id = ").push_bind(restaurant_id.0);
        }
        if let Some(status) = filter.status {
            qb.push(" AND status = ").push_bind(status.to_ord());
        }
        qb.push(" ORDER BY requested_at DESC");
        let rows = qb.build().fetch_all(&self.pool).await.map_err(db_err)?;
        rows.iter().map(decode).collect()
    }
}
