//! The reminder-promotion DEAD-MAN'S SWITCH + SCHEDULED-depth gauge (#167, the
//! `acceptance-timeout` contract in specs/observability.yaml).
//!
//! A MONITOR, deliberately OUTSIDE the worker it watches (ADR-20260810-231300's monitoring
//! carve-out — a monitor keeps a poll in every case, because for a monitor silence is
//! ambiguous): if the promotion pass emitted its own liveness, a dead worker would silence its
//! own alarm — the exact "monitoring path that can only fire when a signal arrives" defect class
//! the ADR names. This sampler polls the durable record (`inbound_messages`) on its own clock
//! and emits on EVERY tick:
//!
//! - `reminder_promotion_due_lag_ms{actor_type}` — now minus the oldest DUE SCHEDULED row's
//!   `scheduled_at`, **0 when nothing is due**. A growing value = promotion is dead while
//!   deadlines pile up; the series STOPPING = this watcher (or the process) is dead. Both are
//!   alertable; neither requires a reminder to arrive.
//! - `mailbox_scheduled_depth{actor_type,purpose}` — SCHEDULED row depth per reminder purpose
//!   (the dba cardinality watch; V0 expectation is single digits).
//!
//! Both series are emitted for every DECLARED reminder lane (`REMINDER_SCHEDULES`), zero
//! included, so "no data" can never masquerade as "nothing scheduled".

use std::collections::BTreeSet;

use sqlx::{PgPool, Row};

/// One watch tick: query the durable SCHEDULED backlog and emit both series, zeros included.
pub async fn promotion_watch_tick(pool: &PgPool) -> Result<(), sqlx::Error> {
    let rows = sqlx::query(
        "SELECT actor_type, message_type, count(*) AS depth, \
                min(scheduled_at) FILTER (WHERE scheduled_at <= now()) AS oldest_due \
         FROM inbound_messages WHERE status = 'SCHEDULED' \
         GROUP BY actor_type, message_type",
    )
    .fetch_all(pool)
    .await?;

    // Every DECLARED reminder lane reports, zero included — a series that only exists when rows
    // exist is the silence-is-ambiguous defect this watcher exists to remove.
    let declared: BTreeSet<(&'static str, &'static str)> =
        application::generated::reminders::REMINDER_SCHEDULES
            .iter()
            .map(|s| (s.actor_type, s.payload_event))
            .collect();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut lag_by_actor: std::collections::BTreeMap<String, i64> = declared
        .iter()
        .map(|(actor, _)| (actor.to_string(), 0i64))
        .collect();

    let now = chrono::Utc::now();
    for row in &rows {
        let actor_type: String = row.try_get("actor_type")?;
        let purpose: String = row.try_get("message_type")?;
        let depth: i64 = row.try_get("depth")?;
        telemetry::meters::mailbox::scheduled_depth(&actor_type, &purpose, depth);
        if let Some(oldest_due) = row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("oldest_due")? {
            let lag = (now - oldest_due).num_milliseconds().max(0);
            let entry = lag_by_actor.entry(actor_type.clone()).or_insert(0);
            *entry = (*entry).max(lag);
        }
        seen.insert((actor_type, purpose));
    }
    for (actor_type, purpose) in &declared {
        if !seen.contains(&(actor_type.to_string(), purpose.to_string())) {
            telemetry::meters::mailbox::scheduled_depth(actor_type, purpose, 0);
        }
    }
    for (actor_type, lag) in &lag_by_actor {
        telemetry::meters::mailbox::promotion_due_lag(actor_type, *lag as f64);
    }
    Ok(())
}

/// Spawn the watch on its own clock. A failed tick logs and keeps ticking — the watcher dying
/// with its patient would defeat its purpose; its OWN death is observable as the series
/// stopping.
pub fn spawn_promotion_watch(pool: PgPool, every: std::time::Duration) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(e) = promotion_watch_tick(&pool).await {
                tracing::warn!(error = %e, "promotion watch: tick failed -- retrying next tick");
            }
        }
    });
}
