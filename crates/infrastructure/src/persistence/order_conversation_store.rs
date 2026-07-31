//! The 13-column `orderconversation` table ↔ [`OrderConversationRow`] mapping, both directions —
//! shared by the read repository (decode) and the projection worker (load current state + upsert the
//! folded row). (#131, epic #129; `claim_events` woven in per §2.5, #155.)
//!
//! Column conventions (ADR-20260728/0040): `status` is a TEXT value (see [`crate::persistence::enum_sql`]);
//! `messages`/`internal_notes`/`muted`/`claim_events` are NOT-NULL jsonb columns carrying
//! `serde_json::Value` arrays; `escalation_reason` is a nullable TEXT column widened into the
//! `EscalationReason` newtype; `customer_chat_enabled`/`admin_invited` are BOOLEAN columns.

use application::queries::OrderConversationRow;
use domain::generated::scalars::{EscalationReason, OrderId, RestaurantId};
use domain::shared::errors::DomainError;
use sqlx::postgres::PgRow;
use sqlx::Row;

use super::db_err;
use super::enum_sql::EnumText;

/// The full column list, in `OrderConversationRow` field order — keep SELECTs and the upsert in sync with it.
pub(crate) const COLUMNS: &str = "order_id, restaurant_id, customer_chat_enabled, status, messages, \
     internal_notes, opened_at, admin_invited, escalation_reason, muted, created_at, updated_at, \
     claim_events";

/// Decode one `orderconversation` row into the generated read-model DTO.
pub(crate) fn decode(row: &PgRow) -> Result<OrderConversationRow, DomainError> {
    Ok(OrderConversationRow {
        order_id: OrderId(row.try_get("order_id").map_err(db_err)?),
        restaurant_id: RestaurantId(row.try_get("restaurant_id").map_err(db_err)?),
        customer_chat_enabled: row.try_get("customer_chat_enabled").map_err(db_err)?,
        status: EnumText::from_text(&row.try_get::<String, _>("status").map_err(db_err)?)?,
        messages: row.try_get("messages").map_err(db_err)?,
        internal_notes: row.try_get("internal_notes").map_err(db_err)?,
        opened_at: row.try_get("opened_at").map_err(db_err)?,
        admin_invited: row.try_get("admin_invited").map_err(db_err)?,
        escalation_reason: row
            .try_get::<Option<String>, _>("escalation_reason")
            .map_err(db_err)?
            .map(EscalationReason),
        muted: row.try_get("muted").map_err(db_err)?,
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
        claim_events: row.try_get("claim_events").map_err(db_err)?,
    })
}

/// Load the current projected conversation for one order, or `None` before its ConversationOpened event.
pub async fn load(exec: impl sqlx::PgExecutor<'_>, id: OrderId) -> Result<Option<OrderConversationRow>, DomainError> {
    let sql = format!("SELECT {COLUMNS} FROM orderconversation WHERE order_id = $1");
    let row = sqlx::query(&sql).bind(id.0).fetch_optional(exec).await.map_err(db_err)?;
    row.as_ref().map(decode).transpose()
}

/// Write the folded row: `INSERT … ON CONFLICT (order_id) DO UPDATE` over all 12 columns.
pub async fn upsert(exec: impl sqlx::PgExecutor<'_>, row: &OrderConversationRow) -> Result<(), DomainError> {
    let sql = format!(
        "INSERT INTO orderconversation ({COLUMNS}) VALUES \
         ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) \
         ON CONFLICT (order_id) DO UPDATE SET \
         restaurant_id = EXCLUDED.restaurant_id, \
         customer_chat_enabled = EXCLUDED.customer_chat_enabled, \
         status = EXCLUDED.status, \
         messages = EXCLUDED.messages, \
         internal_notes = EXCLUDED.internal_notes, \
         opened_at = EXCLUDED.opened_at, \
         admin_invited = EXCLUDED.admin_invited, \
         escalation_reason = EXCLUDED.escalation_reason, \
         muted = EXCLUDED.muted, \
         created_at = EXCLUDED.created_at, \
         updated_at = EXCLUDED.updated_at, \
         claim_events = EXCLUDED.claim_events"
    );
    sqlx::query(&sql)
        .bind(row.order_id.0)
        .bind(row.restaurant_id.0)
        .bind(row.customer_chat_enabled)
        .bind(row.status.to_text())
        .bind(row.messages.clone())
        .bind(row.internal_notes.clone())
        .bind(row.opened_at)
        .bind(row.admin_invited)
        .bind(row.escalation_reason.as_ref().map(|v| v.0.clone()))
        .bind(row.muted.clone())
        .bind(row.created_at)
        .bind(row.updated_at)
        .bind(row.claim_events.clone())
        .execute(exec)
        .await
        .map_err(db_err)?;
    Ok(())
}
