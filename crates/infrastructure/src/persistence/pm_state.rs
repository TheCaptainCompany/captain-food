//! Postgres adapters for the process-manager STATE stores (ADR-20260719-172821): the four saga state
//! tables of `specs/database/tables/process_managers.yaml` (migration
//! `20260719200000_process_manager_state_tables.sql`), one `Pg…State` per `application::pm_state` port.
//!
//! Conventions match the projection stores: enum columns are INTEGER declaration-order ordinals
//! ([`super::enum_sql`]); scalar newtypes bind via `.0`; upserts are `INSERT … ON CONFLICT (pk) DO
//! UPDATE` over all columns. `last_update_utc` is the runtime envelope's stamp: every upsert writes
//! `now()` server-side (the row's carried value is IGNORED), reads return the stored value.

use application::pm_state::{
    CartBindingRow, CartBindingStateStore, DeliveryDispatchRow, DeliveryDispatchStateStore,
    PaymentProcessRow, PaymentProcessStateStore, RefundProcessRow, RefundProcessStateStore,
};
use async_trait::async_trait;
use domain::generated::scalars::{
    CartId, CustomerId, DeliveryJobId, ExternalReference, MoneyCents, OrderId, PaymentIntentId,
    RefundId, RestaurantId, SessionId,
};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

use super::db_err;
use super::enum_sql::EnumOrd;

// ---------------------------------------------------------------------------------------------------
// payment_process_manager
// ---------------------------------------------------------------------------------------------------

/// Column list of `payment_process_manager`, in [`PaymentProcessRow`] field order.
const PAYMENT_COLUMNS: &str = "cart_id, order_id, payment_intent_id, process_status, \
     payment_status, customer_id, session_id, client_secret, \
     last_processed_stripe_event_id, last_update_utc";

fn decode_payment(row: &PgRow) -> Result<PaymentProcessRow, DomainError> {
    Ok(PaymentProcessRow {
        cart_id: CartId(row.try_get("cart_id").map_err(db_err)?),
        order_id: OrderId(row.try_get("order_id").map_err(db_err)?),
        payment_intent_id: PaymentIntentId(row.try_get("payment_intent_id").map_err(db_err)?),
        process_status: EnumOrd::from_ord(row.try_get::<i32, _>("process_status").map_err(db_err)?)?,
        payment_status: EnumOrd::from_ord(row.try_get::<i32, _>("payment_status").map_err(db_err)?)?,
        customer_id: row
            .try_get::<Option<uuid::Uuid>, _>("customer_id")
            .map_err(db_err)?
            .map(CustomerId),
        session_id: row
            .try_get::<Option<uuid::Uuid>, _>("session_id")
            .map_err(db_err)?
            .map(SessionId),
        client_secret: row.try_get::<Option<String>, _>("client_secret").map_err(db_err)?,
        last_processed_stripe_event_id: row
            .try_get::<Option<String>, _>("last_processed_stripe_event_id")
            .map_err(db_err)?
            .map(ExternalReference),
        last_update_utc: row.try_get("last_update_utc").map_err(db_err)?,
    })
}

/// Postgres [`PaymentProcessStateStore`] over `payment_process_manager`.
pub struct PgPaymentProcessState {
    pool: PgPool,
}

impl PgPaymentProcessState {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PaymentProcessStateStore for PgPaymentProcessState {
    async fn by_cart(&self, cart_id: CartId) -> Result<Option<PaymentProcessRow>, DomainError> {
        let sql = format!("SELECT {PAYMENT_COLUMNS} FROM payment_process_manager WHERE cart_id = $1");
        let row =
            sqlx::query(&sql).bind(cart_id.0).fetch_optional(&self.pool).await.map_err(db_err)?;
        row.as_ref().map(decode_payment).transpose()
    }

    async fn by_payment_intent(
        &self,
        payment_intent_id: &PaymentIntentId,
    ) -> Result<Option<PaymentProcessRow>, DomainError> {
        let sql = format!(
            "SELECT {PAYMENT_COLUMNS} FROM payment_process_manager WHERE payment_intent_id = $1"
        );
        let row = sqlx::query(&sql)
            .bind(payment_intent_id.0.clone())
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        row.as_ref().map(decode_payment).transpose()
    }

    async fn by_order(&self, order_id: OrderId) -> Result<Option<PaymentProcessRow>, DomainError> {
        let sql = format!("SELECT {PAYMENT_COLUMNS} FROM payment_process_manager WHERE order_id = $1");
        let row =
            sqlx::query(&sql).bind(order_id.0).fetch_optional(&self.pool).await.map_err(db_err)?;
        row.as_ref().map(decode_payment).transpose()
    }

    async fn upsert(&self, row: &PaymentProcessRow) -> Result<(), DomainError> {
        let sql = format!(
            "INSERT INTO payment_process_manager ({PAYMENT_COLUMNS}) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,now()) \
             ON CONFLICT (cart_id) DO UPDATE SET \
             order_id = EXCLUDED.order_id, \
             payment_intent_id = EXCLUDED.payment_intent_id, \
             process_status = EXCLUDED.process_status, \
             payment_status = EXCLUDED.payment_status, \
             customer_id = EXCLUDED.customer_id, \
             session_id = EXCLUDED.session_id, \
             client_secret = EXCLUDED.client_secret, \
             last_processed_stripe_event_id = EXCLUDED.last_processed_stripe_event_id, \
             last_update_utc = now()"
        );
        sqlx::query(&sql)
            .bind(row.cart_id.0)
            .bind(row.order_id.0)
            .bind(row.payment_intent_id.0.clone())
            .bind(row.process_status.to_ord())
            .bind(row.payment_status.to_ord())
            .bind(row.customer_id.as_ref().map(|v| v.0))
            .bind(row.session_id.as_ref().map(|v| v.0))
            .bind(row.client_secret.clone())
            .bind(row.last_processed_stripe_event_id.as_ref().map(|v| v.0.clone()))
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------
// refund_process_manager
// ---------------------------------------------------------------------------------------------------

/// Column list of `refund_process_manager`, in [`RefundProcessRow`] field order.
const REFUND_COLUMNS: &str = "order_id, payment_intent_id, refund_id, process_status, \
     approved_amount_cents, reason, last_update_utc";

fn decode_refund(row: &PgRow) -> Result<RefundProcessRow, DomainError> {
    Ok(RefundProcessRow {
        order_id: OrderId(row.try_get("order_id").map_err(db_err)?),
        payment_intent_id: row
            .try_get::<Option<String>, _>("payment_intent_id")
            .map_err(db_err)?
            .map(PaymentIntentId),
        refund_id: row.try_get::<Option<String>, _>("refund_id").map_err(db_err)?.map(RefundId),
        process_status: EnumOrd::from_ord(row.try_get::<i32, _>("process_status").map_err(db_err)?)?,
        approved_amount_cents: row
            .try_get::<Option<i64>, _>("approved_amount_cents")
            .map_err(db_err)?
            .map(MoneyCents),
        reason: row.try_get("reason").map_err(db_err)?,
        last_update_utc: row.try_get("last_update_utc").map_err(db_err)?,
    })
}

/// Postgres [`RefundProcessStateStore`] over `refund_process_manager`.
pub struct PgRefundProcessState {
    pool: PgPool,
}

impl PgRefundProcessState {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RefundProcessStateStore for PgRefundProcessState {
    async fn by_order(&self, order_id: OrderId) -> Result<Option<RefundProcessRow>, DomainError> {
        let sql = format!("SELECT {REFUND_COLUMNS} FROM refund_process_manager WHERE order_id = $1");
        let row =
            sqlx::query(&sql).bind(order_id.0).fetch_optional(&self.pool).await.map_err(db_err)?;
        row.as_ref().map(decode_refund).transpose()
    }

    async fn upsert(&self, row: &RefundProcessRow) -> Result<(), DomainError> {
        let sql = format!(
            "INSERT INTO refund_process_manager ({REFUND_COLUMNS}) \
             VALUES ($1,$2,$3,$4,$5,$6,now()) \
             ON CONFLICT (order_id) DO UPDATE SET \
             payment_intent_id = EXCLUDED.payment_intent_id, \
             refund_id = EXCLUDED.refund_id, \
             process_status = EXCLUDED.process_status, \
             approved_amount_cents = EXCLUDED.approved_amount_cents, \
             reason = EXCLUDED.reason, \
             last_update_utc = now()"
        );
        sqlx::query(&sql)
            .bind(row.order_id.0)
            .bind(row.payment_intent_id.as_ref().map(|v| v.0.clone()))
            .bind(row.refund_id.as_ref().map(|v| v.0.clone()))
            .bind(row.process_status.to_ord())
            .bind(row.approved_amount_cents.map(|v| v.0))
            .bind(row.reason.clone())
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------
// cart_binding_process_manager
// ---------------------------------------------------------------------------------------------------

/// Column list of `cart_binding_process_manager`, in [`CartBindingRow`] field order.
const CART_BINDING_COLUMNS: &str = "session_id, customer_id, last_update_utc";

fn decode_cart_binding(row: &PgRow) -> Result<CartBindingRow, DomainError> {
    Ok(CartBindingRow {
        session_id: SessionId(row.try_get("session_id").map_err(db_err)?),
        customer_id: CustomerId(row.try_get("customer_id").map_err(db_err)?),
        last_update_utc: row.try_get("last_update_utc").map_err(db_err)?,
    })
}

/// Postgres [`CartBindingStateStore`] over `cart_binding_process_manager`.
pub struct PgCartBindingState {
    pool: PgPool,
}

impl PgCartBindingState {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CartBindingStateStore for PgCartBindingState {
    async fn by_session(
        &self,
        session_id: SessionId,
    ) -> Result<Option<CartBindingRow>, DomainError> {
        let sql = format!(
            "SELECT {CART_BINDING_COLUMNS} FROM cart_binding_process_manager WHERE session_id = $1"
        );
        let row =
            sqlx::query(&sql).bind(session_id.0).fetch_optional(&self.pool).await.map_err(db_err)?;
        row.as_ref().map(decode_cart_binding).transpose()
    }

    async fn upsert(&self, row: &CartBindingRow) -> Result<(), DomainError> {
        let sql = format!(
            "INSERT INTO cart_binding_process_manager ({CART_BINDING_COLUMNS}) \
             VALUES ($1,$2,now()) \
             ON CONFLICT (session_id) DO UPDATE SET \
             customer_id = EXCLUDED.customer_id, \
             last_update_utc = now()"
        );
        sqlx::query(&sql)
            .bind(row.session_id.0)
            .bind(row.customer_id.0)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------
// delivery_dispatch_process_manager
// ---------------------------------------------------------------------------------------------------

/// Column list of `delivery_dispatch_process_manager`, in [`DeliveryDispatchRow`] field order.
const DISPATCH_COLUMNS: &str =
    "order_id, restaurant_id, delivery_job_id, process_status, offer_attempts, last_update_utc";

fn decode_dispatch(row: &PgRow) -> Result<DeliveryDispatchRow, DomainError> {
    Ok(DeliveryDispatchRow {
        order_id: OrderId(row.try_get("order_id").map_err(db_err)?),
        restaurant_id: RestaurantId(row.try_get("restaurant_id").map_err(db_err)?),
        delivery_job_id: DeliveryJobId(row.try_get("delivery_job_id").map_err(db_err)?),
        process_status: EnumOrd::from_ord(row.try_get::<i32, _>("process_status").map_err(db_err)?)?,
        offer_attempts: row.try_get("offer_attempts").map_err(db_err)?,
        last_update_utc: row.try_get("last_update_utc").map_err(db_err)?,
    })
}

/// Postgres [`DeliveryDispatchStateStore`] over `delivery_dispatch_process_manager`.
pub struct PgDeliveryDispatchState {
    pool: PgPool,
}

impl PgDeliveryDispatchState {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DeliveryDispatchStateStore for PgDeliveryDispatchState {
    async fn by_order(
        &self,
        order_id: OrderId,
    ) -> Result<Option<DeliveryDispatchRow>, DomainError> {
        let sql = format!(
            "SELECT {DISPATCH_COLUMNS} FROM delivery_dispatch_process_manager WHERE order_id = $1"
        );
        let row =
            sqlx::query(&sql).bind(order_id.0).fetch_optional(&self.pool).await.map_err(db_err)?;
        row.as_ref().map(decode_dispatch).transpose()
    }

    async fn by_job(
        &self,
        delivery_job_id: DeliveryJobId,
    ) -> Result<Option<DeliveryDispatchRow>, DomainError> {
        let sql = format!(
            "SELECT {DISPATCH_COLUMNS} FROM delivery_dispatch_process_manager \
             WHERE delivery_job_id = $1"
        );
        let row = sqlx::query(&sql)
            .bind(delivery_job_id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        row.as_ref().map(decode_dispatch).transpose()
    }

    async fn upsert(&self, row: &DeliveryDispatchRow) -> Result<(), DomainError> {
        let sql = format!(
            "INSERT INTO delivery_dispatch_process_manager ({DISPATCH_COLUMNS}) \
             VALUES ($1,$2,$3,$4,$5,now()) \
             ON CONFLICT (order_id) DO UPDATE SET \
             restaurant_id = EXCLUDED.restaurant_id, \
             delivery_job_id = EXCLUDED.delivery_job_id, \
             process_status = EXCLUDED.process_status, \
             offer_attempts = EXCLUDED.offer_attempts, \
             last_update_utc = now()"
        );
        sqlx::query(&sql)
            .bind(row.order_id.0)
            .bind(row.restaurant_id.0)
            .bind(row.delivery_job_id.0)
            .bind(row.process_status.to_ord())
            .bind(row.offer_attempts)
            .execute(&self.pool)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}
