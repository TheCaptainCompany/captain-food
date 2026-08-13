//! The **shared** SMS-OTP send counter (`sms_send_quota`, #516) — the Postgres implementation of
//! [`SmsQuotaStore`].
//!
//! **Why Postgres and not memory.** Every guard above this is only as strong as the counter under it.
//! A per-pod in-memory limiter multiplies the allowance by the replica count and resets on every
//! deploy; for the GLOBAL DAILY CEILING, which is the only guard that bounds the bill, that is the
//! difference between a ceiling and a suggestion. Every pod must count into the same row.
//!
//! **Why one statement.** `RequestPhoneVerification` has no per-phone actor lane — the GraphQL door
//! mints a fresh `actor_id` per request — so nothing serialises two concurrent requests for the same
//! number except this statement. A load-modify-save would race, and the race is worth money.
//!
//! The claim is therefore a single `INSERT … ON CONFLICT DO UPDATE … WHERE`, where **the `WHERE`
//! clause IS the limit**: a losing claim updates no row and `RETURNING` yields nothing. There is no
//! window in which a decision is held in Rust.

use application::sms_guard::{QuotaClaim, QuotaDenial, QuotaGrant, SmsQuotaStore};
use async_trait::async_trait;
use sqlx::{PgPool, Row};

/// `sms_send_quota`-backed [`SmsQuotaStore`].
pub struct PgSmsQuotaStore {
    pool: PgPool,
}

impl PgSmsQuotaStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Any database failure is `Unavailable`, and every caller treats that as a REFUSAL (fail-closed).
/// An unreachable limiter must not become an open spend path — that inversion is the whole failure
/// mode the guards exist to prevent.
fn unavailable(e: sqlx::Error) -> QuotaDenial {
    QuotaDenial::Unavailable { detail: e.to_string() }
}

/// The single atomic claim.
///
/// Read it as three decisions in one statement:
/// * **expiry** — `window_start + window <= now` resets both the window and the count. `now <
///   window_start` (a clock that went BACKWARDS) deliberately does NOT reset: treating skew as expiry
///   would hand a skewed replica a fresh allowance.
/// * **the ceiling** — `sent_count < limit` in the `DO UPDATE … WHERE`. A losing claim touches
///   nothing, so a refusal never burns budget.
/// * **the cooldown ladder** — indexed out of the `$4::bigint[]` schedule by the count so far, with
///   the last entry repeating. Postgres arrays are **1-BASED**, so the cooldown owed after
///   `sent_count` grants is element `[sent_count]` — `[sent_count + 1]` skips a whole rung, which is
///   the defect `the_cooldown_ladder_holds_in_sql` caught (30s became 120s on the first resend).
///
/// `$5` is `now` as a unix epoch, injected rather than taken from the database clock, so the window
/// edges are testable and every replica uses one clock source per call.
const CLAIM_SQL: &str = "\
    INSERT INTO sms_send_quota AS q (quota_key, window_start, sent_count, last_granted_at) \
    VALUES ($1, to_timestamp($5), 1, to_timestamp($5)) \
    ON CONFLICT (quota_key) DO UPDATE SET \
        window_start = CASE WHEN q.window_start + make_interval(secs => $2::double precision) <= to_timestamp($5) \
                            THEN to_timestamp($5) ELSE q.window_start END, \
        sent_count   = CASE WHEN q.window_start + make_interval(secs => $2::double precision) <= to_timestamp($5) \
                            THEN 1 ELSE q.sent_count + 1 END, \
        last_granted_at = to_timestamp($5) \
    WHERE $3::int >= 1 AND ( \
            q.window_start + make_interval(secs => $2::double precision) <= to_timestamp($5) \
            OR ( q.sent_count < $3::int \
                 AND to_timestamp($5) >= q.last_granted_at + make_interval(secs => \
                       COALESCE(($4::bigint[])[least(q.sent_count, array_length($4::bigint[], 1))], 0)::double precision) ) \
          ) \
    RETURNING q.sent_count";

/// The advisory look: the same three decisions, evaluated in SQL, recording nothing. It is a SELECT of
/// the identical predicate, so a `peek` can never disagree with a `try_claim` about a given row state
/// — a peek that says yes where a claim says no would make the edge check a liar.
const PEEK_SQL: &str = "\
    SELECT q.sent_count, \
           q.window_start + make_interval(secs => $2::double precision) <= to_timestamp($5) AS expired, \
           GREATEST(0, EXTRACT(EPOCH FROM (q.window_start + make_interval(secs => $2::double precision) - to_timestamp($5))))::bigint AS window_left, \
           GREATEST(0, EXTRACT(EPOCH FROM (q.last_granted_at + make_interval(secs => \
               COALESCE(($4::bigint[])[least(q.sent_count, array_length($4::bigint[], 1))], 0)::double precision) - to_timestamp($5))))::bigint AS cooldown_left \
      FROM sms_send_quota q WHERE q.quota_key = $1";

/// The remaining window on a DENIED claim, for the `retryAfterSeconds` the screen renders. A second
/// statement, on the refusal path only: it is advisory (the caller has already been refused), so it
/// cannot reopen the race the claim closed. `None` when the row vanished meanwhile — the caller then
/// reports the window length, never zero, because zero would invite an immediate retry.
const RETRY_AFTER_SQL: &str = "\
    SELECT GREATEST(0, EXTRACT(EPOCH FROM (q.window_start + make_interval(secs => $2::double precision) - to_timestamp($3))))::bigint AS window_left, \
           GREATEST(0, EXTRACT(EPOCH FROM (q.last_granted_at + make_interval(secs => \
               COALESCE(($4::bigint[])[least(q.sent_count, array_length($4::bigint[], 1))], 0)::double precision) - to_timestamp($3))))::bigint AS cooldown_left, \
           q.sent_count \
      FROM sms_send_quota q WHERE q.quota_key = $1";

impl PgSmsQuotaStore {
    /// Classify a denial: inside the cooldown ladder, or at the ceiling. The distinction is what the
    /// customer sees — a countdown they can wait out, or a cap that needs help.
    async fn denial_for(&self, claim: &QuotaClaim, now_unix: i64) -> QuotaDenial {
        let backoff: Vec<i64> = claim.backoff_seconds.clone();
        let row = sqlx::query(RETRY_AFTER_SQL)
            .bind(&claim.key)
            .bind(claim.window_seconds as f64)
            .bind(now_unix)
            .bind(&backoff)
            .fetch_optional(&self.pool)
            .await;
        match row {
            Ok(Some(row)) => {
                let window_left: i64 = row.try_get("window_left").unwrap_or(claim.window_seconds);
                let cooldown_left: i64 = row.try_get("cooldown_left").unwrap_or(0);
                let sent_count: i32 = row.try_get("sent_count").unwrap_or(claim.limit);
                if sent_count >= claim.limit {
                    QuotaDenial::LimitReached { retry_after_seconds: window_left }
                } else {
                    QuotaDenial::Cooldown { retry_after_seconds: cooldown_left.max(1) }
                }
            }
            // No row and yet the claim lost: only possible with `limit < 1` (the kill switch) or a
            // concurrent sweep. Report the ceiling — the loud, strict reading.
            Ok(None) => QuotaDenial::LimitReached { retry_after_seconds: claim.window_seconds },
            Err(e) => unavailable(e),
        }
    }
}

#[async_trait]
impl SmsQuotaStore for PgSmsQuotaStore {
    async fn try_claim(&self, claim: &QuotaClaim, now_unix: i64) -> Result<QuotaGrant, QuotaDenial> {
        // THE KILL SWITCH, and it must fire on a COLD key too. The `$3 >= 1` guard inside CLAIM_SQL
        // only reaches the `DO UPDATE` branch, so with no row yet the plain `INSERT` would win and a
        // ceiling of zero would grant the first send of the day. Found by
        // `the_kill_switch_refuses_the_very_first_send` against a real Postgres; the in-memory
        // reference store had the same hole and the same fix.
        if claim.limit < 1 {
            return Err(QuotaDenial::LimitReached { retry_after_seconds: claim.window_seconds });
        }
        let backoff: Vec<i64> = claim.backoff_seconds.clone();
        let row = sqlx::query(CLAIM_SQL)
            .bind(&claim.key)
            .bind(claim.window_seconds as f64)
            .bind(claim.limit)
            .bind(&backoff)
            .bind(now_unix)
            .fetch_optional(&self.pool)
            .await
            .map_err(unavailable)?;
        match row {
            Some(row) => {
                let sent_count: i32 = row.try_get("sent_count").map_err(unavailable)?;
                Ok(QuotaGrant { sent_count })
            }
            None => Err(self.denial_for(claim, now_unix).await),
        }
    }

    async fn peek(&self, claim: &QuotaClaim, now_unix: i64) -> Result<(), QuotaDenial> {
        // The kill switch must be visible to the edge check too, not only to the claim.
        if claim.limit < 1 {
            return Err(QuotaDenial::LimitReached { retry_after_seconds: claim.window_seconds });
        }
        let backoff: Vec<i64> = claim.backoff_seconds.clone();
        let row = sqlx::query(PEEK_SQL)
            .bind(&claim.key)
            .bind(claim.window_seconds as f64)
            .bind(claim.limit)
            .bind(&backoff)
            .bind(now_unix)
            .fetch_optional(&self.pool)
            .await
            .map_err(unavailable)?;
        let Some(row) = row else { return Ok(()) };
        let expired: bool = row.try_get("expired").map_err(unavailable)?;
        if expired {
            return Ok(());
        }
        let sent_count: i32 = row.try_get("sent_count").map_err(unavailable)?;
        if sent_count >= claim.limit {
            let window_left: i64 = row.try_get("window_left").map_err(unavailable)?;
            return Err(QuotaDenial::LimitReached { retry_after_seconds: window_left });
        }
        let cooldown_left: i64 = row.try_get("cooldown_left").map_err(unavailable)?;
        if cooldown_left > 0 {
            return Err(QuotaDenial::Cooldown { retry_after_seconds: cooldown_left });
        }
        Ok(())
    }
}
