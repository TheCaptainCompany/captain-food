//! sqlx read-model repository over the `View_DeliveryPartnerAvailability` SQL view (#61, ADR-0039) —
//! delivery-partner self-registration, projected ON READ as a state-fold over `domain_events`
//! (DeliveryPartnerAvailabilityRequested / Approved / Revoked on the DeliveryPartnerRegistration
//! stream; created by the migrations from the generated views SQL). Backs the
//! `deliveryPartnerAvailabilities` GraphQL query via
//! `application::queries::DeliveryPartnerAvailabilityReadRepository`.
//!
//! Column conventions match the other view repos (ADR-0037): `status` comes back as its INTEGER ordinal
//! (the generated view folds it with a declaration-order CASE ladder).

use application::queries::{
    DeliveryPartnerAvailabilityFilter, DeliveryPartnerAvailabilityReadRepository,
    DeliveryPartnerAvailabilityRow,
};
use async_trait::async_trait;
use domain::generated::scalars::{
    CityId, DeliveryChannelKey, DeliveryPartnerName, DeliveryPartnerRegistrationId, EmailAddress,
};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Postgres, QueryBuilder, Row};

use super::db_err;
use super::enum_sql::EnumOrd;

/// The view columns the read side consumes, in `DeliveryPartnerAvailabilityRow` field order.
const COLUMNS: &str = "registration_id, channel, city_id, partner_name, contact_email, status, \
     requested_at, decided_at";

/// Unquoted `CREATE VIEW View_DeliveryPartnerAvailability` folds to this identifier in Postgres.
const VIEW: &str = "view_deliverypartneravailability";

/// Decode one `View_DeliveryPartnerAvailability` row into the hand-written read-model DTO.
fn decode(row: &PgRow) -> Result<DeliveryPartnerAvailabilityRow, DomainError> {
    Ok(DeliveryPartnerAvailabilityRow {
        registration_id: DeliveryPartnerRegistrationId(row.try_get("registration_id").map_err(db_err)?),
        channel: DeliveryChannelKey(row.try_get("channel").map_err(db_err)?),
        city_id: CityId(row.try_get("city_id").map_err(db_err)?),
        partner_name: DeliveryPartnerName(row.try_get("partner_name").map_err(db_err)?),
        contact_email: EmailAddress(row.try_get("contact_email").map_err(db_err)?),
        status: EnumOrd::from_ord(row.try_get::<i32, _>("status").map_err(db_err)?)?,
        requested_at: row.try_get("requested_at").map_err(db_err)?,
        decided_at: row.try_get("decided_at").map_err(db_err)?,
    })
}

/// Postgres adapter for the delivery-partner availability read model (the
/// `View_DeliveryPartnerAvailability` fold view).
pub struct PgDeliveryPartnerAvailabilityRepository {
    pool: PgPool,
}

impl PgDeliveryPartnerAvailabilityRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DeliveryPartnerAvailabilityReadRepository for PgDeliveryPartnerAvailabilityRepository {
    /// The availability registrations, newest-request-first, honouring the optional city/channel/status
    /// filters (status = PENDING is the admin review queue).
    async fn list(
        &self,
        filter: DeliveryPartnerAvailabilityFilter,
    ) -> Result<Vec<DeliveryPartnerAvailabilityRow>, DomainError> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new(format!("SELECT {COLUMNS} FROM {VIEW} WHERE TRUE"));
        if let Some(city_id) = filter.city_id {
            qb.push(" AND city_id = ").push_bind(city_id.0);
        }
        if let Some(channel) = filter.channel {
            qb.push(" AND channel = ").push_bind(channel.0);
        }
        if let Some(status) = filter.status {
            qb.push(" AND status = ").push_bind(status.to_ord());
        }
        qb.push(" ORDER BY requested_at DESC");
        let rows = qb.build().fetch_all(&self.pool).await.map_err(db_err)?;
        rows.iter().map(decode).collect()
    }
}
