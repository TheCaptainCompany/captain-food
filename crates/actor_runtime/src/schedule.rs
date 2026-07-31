//! The PROMOTION pass (PROP-20260728-152752 §3.4, ADR-20260731-150500): a SCHEDULED row —
//! `position` NULL, invisible to the drain — becomes deliverable by flipping to RECEIVED with a
//! FRESH position once `scheduled_at` is due. Position-at-promotion is what makes reminders
//! ordinary mailbox rows afterwards: the drain, the fence and the checkpoint never learn that a
//! row was ever scheduled.

use sqlx::PgPool;

/// Promote every due SCHEDULED row of one actor type. One atomic statement; idempotent and safe
/// under concurrent workers — the `status` predicate re-evaluates under the row lock, so each row
/// is promoted exactly once (the loser of the race matches zero rows). Positions are per-row
/// `nextval`, so simultaneously-due reminders order by allocation — they are causally unrelated,
/// only the per-lane head-of-line order after promotion matters. The scan rides the
/// `idx_inbound_messages_due` partial index (`scheduled_at WHERE status = 'SCHEDULED'`).
///
/// Cross-partition by design: promotion needs no lease — it is the flip that MAKES rows subject
/// to the leased drain, and double-delivery is impossible regardless of who flips.
pub async fn promote_due(pool: &PgPool, actor_type: &str) -> sqlx::Result<u64> {
    Ok(sqlx::query(
        "UPDATE inbound_messages \
         SET status = 'RECEIVED', position = nextval('inbound_messages_position_seq') \
         WHERE actor_type = $1 AND status = 'SCHEDULED' AND scheduled_at <= now()",
    )
    .bind(actor_type)
    .execute(pool)
    .await?
    .rows_affected())
}
