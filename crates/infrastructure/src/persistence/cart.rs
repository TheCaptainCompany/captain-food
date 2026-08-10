//! sqlx read-model repository over the materialized `cart` projection table (ADR-0040). Backs the
//! `carts` / `cart` GraphQL queries via `application::queries::CartReadRepository`.

use application::queries::{CartReadRepository, CartRow};
use async_trait::async_trait;
use domain::generated::scalars::{CartId, CartStatus, CustomerId, SessionId};
use domain::shared::errors::DomainError;
use sqlx::PgPool;

use super::cart_store;
use super::db_err;
use super::enum_sql::EnumText;

/// Postgres adapter for the Cart read model.
pub struct PgCartRepository {
    pool: PgPool,
}

impl PgCartRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CartReadRepository for PgCartRepository {
    /// The customer's OPEN carts, most recently updated first, bounded.
    ///
    /// The `status = OPEN` predicate is LOAD-BEARING, not a convenience (#451): every consumer of
    /// this port prices what it returns through `price_cart` against TODAY's catalog, and a
    /// CHECKED_OUT cart's money was frozen at payment intent (`PaymentIntentCreated.checkout`) —
    /// repricing it would render a receipt-adjacent number that never matched what was charged.
    /// Historical price belongs to the CheckoutSnapshot, a read this port does not perform; so the
    /// honest fix is that a non-OPEN row never enters the pricing path at all. It also stops ONE
    /// historical cart holding a since-deleted offer from erroring the customer's WHOLE cart list
    /// (the `carts` literal pushes `priced(..).await?` per row — one unresolvable line fails all).
    ///
    /// `LIMIT 50` bounds the pricing fan-out: the response costs one catalog read per row, so an
    /// unbounded list is an unbounded read amplification on a request path. Fifty open carts is
    /// already far past any real customer (one open cart per restaurant).
    async fn by_customer(&self, customer_id: CustomerId) -> Result<Vec<CartRow>, DomainError> {
        let sql = format!(
            "SELECT {} FROM cart WHERE customer_id = $1 AND status = $2 ORDER BY updated_at DESC LIMIT 50",
            cart_store::COLUMNS
        );
        let rows = sqlx::query(&sql)
            .bind(customer_id.0)
            .bind(CartStatus::OPEN.to_text())
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        rows.iter().map(cart_store::decode).collect()
    }

    /// One OPEN cart by id, or `None`.
    ///
    /// Same OPEN-only rule as `by_customer`, in the same place, for the same reason (#451): the
    /// caller prices what this returns against TODAY's catalog, and a CHECKED_OUT cart's money was
    /// frozen at payment intent. One rule enforced on one path and not the other is how a diff
    /// ends up asserting two different answers to the same question.
    ///
    /// Post-checkout money is NOT this port's to answer: the aggregate that owns "what was
    /// charged" is the Order, and the frozen amount lives in the CheckoutSnapshot on
    /// `PaymentIntentCreated`. A cart read model answering a receipt question is a wrong-aggregate
    /// read — so `None` here is the final shape, not a gap awaiting a snapshot lookup.
    ///
    /// Deliberately NOT delegating to `cart_store::load` any more: that function is also the
    /// PROJECTOR's read-modify-write (`projection/worker.rs` loads prior state before folding the
    /// next event), so an OPEN predicate there would stop a CHECKED_OUT cart from ever folding
    /// another event onto itself. The narrowing belongs to the READ port only.
    async fn by_id(&self, id: CartId) -> Result<Option<CartRow>, DomainError> {
        let sql = format!(
            "SELECT {} FROM cart WHERE cart_id = $1 AND status = $2",
            cart_store::COLUMNS
        );
        let row = sqlx::query(&sql)
            .bind(id.0)
            .bind(CartStatus::OPEN.to_text())
            .fetch_optional(&self.pool)
            .await
            .map_err(db_err)?;
        row.as_ref().map(cart_store::decode).transpose()
    }

    /// The session's OPEN carts (CartBindingProcess's `read` step): a real SQL predicate over the
    /// projected `status` value, overriding the provided empty default.
    async fn open_by_session(&self, session_id: SessionId) -> Result<Vec<CartRow>, DomainError> {
        let sql = format!(
            "SELECT {} FROM cart WHERE session_id = $1 AND status = $2 ORDER BY updated_at DESC",
            cart_store::COLUMNS
        );
        let rows = sqlx::query(&sql)
            .bind(session_id.0)
            .bind(CartStatus::OPEN.to_text())
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;
        rows.iter().map(cart_store::decode).collect()
    }
}
