//! The SHARED OTP send counter against a real Postgres (#516).
//!
//! `crates/application/src/sms_guard.rs` proves the POLICY against an in-memory reference store. This
//! suite proves the thing that actually runs: the single atomic
//! `INSERT … ON CONFLICT DO UPDATE … WHERE` statement. The two are not interchangeable, and the
//! difference is where the money is — the in-memory store cannot exhibit a lost update, so testing
//! only there would prove the guard against the one implementation that is never deployed.
//!
//! What is asserted here that cannot be asserted there:
//! * the SQL predicate agrees with the reference semantics at BOTH window edges;
//! * concurrent claims for one number cannot both win (the lost-update case — there is no per-phone
//!   actor lane, so this statement is the only serialisation);
//! * a refused claim records nothing, so refusals never burn the money budget;
//! * a backwards clock denies rather than resetting the window.

use application::sms_guard::{
    QuotaClaim, QuotaDenial, SmsQuotaStore, SmsSendPolicy, DAY_SECONDS, HOUR_SECONDS,
};
use infrastructure::persistence::PgSmsQuotaStore;
use sqlx::Row;

use super::common::TestDb;

const T0: i64 = 1_760_000_000;

fn phone_hour(limit: i32, backoff: Vec<i64>) -> QuotaClaim {
    QuotaClaim {
        key: "phone:+33612345678:hour".into(),
        window_seconds: HOUR_SECONDS,
        limit,
        backoff_seconds: backoff,
    }
}

fn global_day(limit: i32) -> QuotaClaim {
    QuotaClaim {
        key: SmsSendPolicy::GLOBAL_DAY_KEY.into(),
        window_seconds: DAY_SECONDS,
        limit,
        backoff_seconds: vec![],
    }
}

#[tokio::test]
async fn the_claim_counts_up_to_the_limit_then_refuses_and_resets_with_the_window() {
    let Some(db) = TestDb::acquire("sms_send_quota").await else { return };
    let store = PgSmsQuotaStore::new(db.pool());
    let claim = phone_hour(3, vec![]);

    for expected in 1..=3 {
        let grant = store
            .try_claim(&claim, T0)
            .await
            .unwrap_or_else(|e| panic!("send {expected} must be granted, got {e:?}"));
        assert_eq!(grant.sent_count, expected);
    }
    match store.try_claim(&claim, T0).await {
        Err(QuotaDenial::LimitReached { retry_after_seconds }) => {
            assert_eq!(retry_after_seconds, HOUR_SECONDS)
        }
        other => panic!("the 4th claim must be refused at the ceiling, got {other:?}"),
    }
    // BOTH edges. One second short of the window is still refused; the window itself resets the
    // count — testing only one edge lets an off-by-one and a never-expiring window both pass.
    assert!(store.try_claim(&claim, T0 + HOUR_SECONDS - 1).await.is_err());
    let reset = store.try_claim(&claim, T0 + HOUR_SECONDS).await.expect("the window turned over");
    assert_eq!(reset.sent_count, 1, "the window reset must restart the count, not continue it");
}

#[tokio::test]
async fn a_refused_claim_records_nothing() {
    let Some(db) = TestDb::acquire("sms_send_quota").await else { return };
    let store = PgSmsQuotaStore::new(db.pool());
    let claim = phone_hour(1, vec![]);
    store.try_claim(&claim, T0).await.expect("first claim");
    for _ in 0..5 {
        assert!(store.try_claim(&claim, T0).await.is_err());
    }
    // The row must still say ONE. If refusals incremented, a refused send — which costs no money —
    // would burn the ceiling that exists to bound money.
    let count: i32 = sqlx::query("SELECT sent_count FROM sms_send_quota WHERE quota_key = $1")
        .bind(&claim.key)
        .fetch_one(&db.pool())
        .await
        .expect("the row exists")
        .try_get("sent_count")
        .expect("sent_count");
    assert_eq!(count, 1, "five refusals must have recorded nothing");
}

#[tokio::test]
async fn concurrent_claims_for_one_number_cannot_both_win() {
    let Some(db) = TestDb::acquire("sms_send_quota").await else { return };
    let store = std::sync::Arc::new(PgSmsQuotaStore::new(db.pool()));
    // A ceiling of exactly 1 with 8 simultaneous claimants. `RequestPhoneVerification` has no
    // per-phone actor lane, so this statement is the ONLY thing standing between "one SMS" and
    // "eight SMS" when a client fires eight requests at once.
    let claim = std::sync::Arc::new(phone_hour(1, vec![]));
    let mut handles = Vec::new();
    for _ in 0..8 {
        let (store, claim) = (store.clone(), claim.clone());
        handles.push(tokio::spawn(async move { store.try_claim(&claim, T0).await.is_ok() }));
    }
    let mut granted = 0;
    for h in handles {
        if h.await.expect("task") {
            granted += 1;
        }
    }
    assert_eq!(granted, 1, "exactly ONE of eight concurrent claims may win, got {granted}");
}

#[tokio::test]
async fn the_cooldown_ladder_holds_in_sql() {
    let Some(db) = TestDb::acquire("sms_send_quota").await else { return };
    let store = PgSmsQuotaStore::new(db.pool());
    // 30s -> 2min -> 10min, generous ceiling so the ladder is what refuses.
    let claim = phone_hour(99, vec![30, 120, 600]);
    store.try_claim(&claim, T0).await.expect("first send");
    assert!(store.try_claim(&claim, T0 + 29).await.is_err(), "1s short of 30s must refuse");
    store.try_claim(&claim, T0 + 30).await.expect("the 30s boundary allows");
    assert!(store.try_claim(&claim, T0 + 149).await.is_err(), "1s short of 2min must refuse");
    store.try_claim(&claim, T0 + 150).await.expect("the 2min boundary allows");
    assert!(store.try_claim(&claim, T0 + 749).await.is_err(), "1s short of 10min must refuse");
    store.try_claim(&claim, T0 + 750).await.expect("the 10min boundary allows");
    // The last step repeats rather than falling off the end of the array.
    assert!(store.try_claim(&claim, T0 + 1_349).await.is_err(), "the 10min step must repeat");
}

#[tokio::test]
async fn a_backwards_clock_denies_rather_than_granting_a_fresh_allowance() {
    let Some(db) = TestDb::acquire("sms_send_quota").await else { return };
    let store = PgSmsQuotaStore::new(db.pool());
    let claim = phone_hour(1, vec![]);
    store.try_claim(&claim, T0).await.expect("first claim");
    // A replica an hour behind must not be handed a new window: reading `now < window_start` as
    // expiry would make clock skew a bypass of every per-number cap at once.
    assert!(
        store.try_claim(&claim, T0 - HOUR_SECONDS).await.is_err(),
        "a backwards clock must deny, never reset"
    );
}

#[tokio::test]
async fn the_kill_switch_refuses_the_very_first_send() {
    let Some(db) = TestDb::acquire("sms_send_quota").await else { return };
    let store = PgSmsQuotaStore::new(db.pool());
    // A ceiling of zero on a key with no row at all — the operator's no-deploy stop must work on a
    // cold table, not only once a row happens to exist.
    assert!(matches!(
        store.try_claim(&global_day(0), T0).await,
        Err(QuotaDenial::LimitReached { .. })
    ));
    let none: Option<i32> = sqlx::query_scalar("SELECT sent_count FROM sms_send_quota WHERE quota_key = $1")
        .bind(SmsSendPolicy::GLOBAL_DAY_KEY)
        .fetch_optional(&store_pool(&db))
        .await
        .expect("query");
    assert!(none.is_none(), "a refused claim must not create the row either");
}

#[tokio::test]
async fn the_operator_can_stop_and_restore_sends_with_one_row_and_no_deploy() {
    let Some(db) = TestDb::acquire("sms_send_quota").await else { return };
    let store = PgSmsQuotaStore::new(db.pool());
    let ceiling = global_day(200);
    store.try_claim(&ceiling, T0).await.expect("a normal send");

    // THE 02:00 KILL SWITCH: push the global counter over its ceiling. No rollout, no restart.
    sqlx::query("UPDATE sms_send_quota SET sent_count = 999999 WHERE quota_key = $1")
        .bind(SmsSendPolicy::GLOBAL_DAY_KEY)
        .execute(&store_pool(&db))
        .await
        .expect("raise the counter");
    assert!(store.try_claim(&ceiling, T0 + 1).await.is_err(), "sends must stop immediately");

    // And restoring is the same one row.
    sqlx::query("DELETE FROM sms_send_quota WHERE quota_key = $1")
        .bind(SmsSendPolicy::GLOBAL_DAY_KEY)
        .execute(&store_pool(&db))
        .await
        .expect("clear the counter");
    store.try_claim(&ceiling, T0 + 2).await.expect("sends resume");
}

#[tokio::test]
async fn peek_agrees_with_the_claim_and_charges_nothing() {
    let Some(db) = TestDb::acquire("sms_send_quota").await else { return };
    let store = PgSmsQuotaStore::new(db.pool());
    let claim = phone_hour(1, vec![]);
    // A peek on a cold key is free and says yes.
    store.peek(&claim, T0).await.expect("cold peek");
    store.peek(&claim, T0).await.expect("still cold — a peek records nothing");
    store.try_claim(&claim, T0).await.expect("the first real send");
    // Now the edge check must see the claim, or it would be shedding nothing while claiming to.
    assert!(store.peek(&claim, T0).await.is_err(), "a peek must see the claim");
    // And it still charged nothing of its own.
    let count: i32 = sqlx::query_scalar("SELECT sent_count FROM sms_send_quota WHERE quota_key = $1")
        .bind(&claim.key)
        .fetch_one(&store_pool(&db))
        .await
        .expect("the row exists");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn the_retention_sweep_reclaims_dead_quota_rows_and_spares_live_ones() {
    let Some(db) = TestDb::acquire("sms_send_quota").await else { return };
    let pool = db.pool();
    // The key embeds a phone number, so a bucket nobody is still asking about is personal data with
    // no reason to exist. A LIVE bucket must survive the same sweep.
    sqlx::query(
        "INSERT INTO sms_send_quota (quota_key, window_start, sent_count, last_granted_at) VALUES \
         ('phone:+33600000001:hour', now() - INTERVAL '3 days', 1, now() - INTERVAL '3 days'), \
         ('phone:+33600000002:hour', now(), 1, now())",
    )
    .execute(&pool)
    .await
    .expect("seed");
    sqlx::query("SELECT * FROM sweep_retention()").execute(&pool).await.expect("sweep");
    let keys: Vec<String> = sqlx::query_scalar("SELECT quota_key FROM sms_send_quota ORDER BY quota_key")
        .fetch_all(&pool)
        .await
        .expect("read back");
    assert_eq!(keys, vec!["phone:+33600000002:hour".to_string()]);
}

/// The pool, for the assertions that read the row back directly.
fn store_pool(db: &TestDb) -> sqlx::PgPool {
    db.pool()
}
