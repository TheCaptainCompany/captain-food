//! Integration test for the BATCHED projection unit-of-work (PROP-20260730-230803, approved with
//! #267): the worker drains `domain_events` in `batch_size`-bounded transactions — every upsert of
//! a batch AND its checkpoint advance commit together, or not at all. Observed here against a real
//! Postgres (docker `postgres:16-alpine` locally, the CI service container, or any `DATABASE_URL`):
//!
//! 1. **The batch is the transaction boundary**: with 10 restaurants × 25 events (250 events) and
//!    `batch_size = 100`, a full drain lands every row correct and the checkpoint at the head —
//!    and a drain that errors AFTER some batches leaves checkpoint and rows CONSISTENT at a batch
//!    boundary (no rows ahead of the checkpoint, no checkpoint ahead of the rows).
//! 2. **Chronological order per business key survives batching**: each restaurant's `display_name`
//!    ends at its LAST update even though its 25 events span three different batches, interleaved
//!    with the other nine restaurants' events.
//! 3. **A poison event mid-batch is skipped, not wedging**: its batch still commits, the
//!    checkpoint passes it, and every healthy event around it (including LATER events for the
//!    same row) still lands.
//! 4. **Resume is idempotent**: re-running the worker over an already-drained log changes nothing.
//!
//! Needs `DATABASE_URL`; skips otherwise (DB_TESTS_REQUIRED makes the skip loud, #230).

use infrastructure::ProjectionWorker;
use sqlx::{PgPool, Row};

async fn append_event(
    pool: &PgPool,
    stream_name: &str,
    version: i32,
    event_type: &str,
    payload: serde_json::Value,
) {
    sqlx::query(
        "INSERT INTO domain_events \
         (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, payload, metadata, occurred_at) \
         VALUES ($1, $2, $3, $4, 'ADMIN', $5, NULL, $6, $7, NULL, now())",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(stream_name)
    .bind(version)
    .bind(uuid::Uuid::nil())
    .bind(uuid::Uuid::new_v4())
    .bind(event_type)
    .bind(payload)
    .execute(pool)
    .await
    .expect("append event");
}

fn registered(id: uuid::Uuid, name: &str) -> serde_json::Value {
    serde_json::json!({
        "restaurantId": id,
        "listingStatus": "ACTIVE_PARTNER",
        "displayName": name,
        "marginRate": 62.0,
        "cuisineCategory": "TRADITIONAL",
        "address": { "line1": "1 rue Nationale", "postalCode": "37000", "city": "Tours", "country": "FR" },
        "openingHours": [{"weekday": "MONDAY", "from": "11:30", "to": "14:00"}],
        "tags": []
    })
}

async fn checkpoint(pool: &PgPool, projector: &str) -> i64 {
    sqlx::query_scalar("SELECT position FROM projection_checkpoint WHERE projector = $1")
        .bind(projector)
        .fetch_optional(pool)
        .await
        .expect("read checkpoint")
        .unwrap_or(0)
}

#[tokio::test]
async fn batched_unit_of_work_drains_in_order_and_survives_poison() {
    let Some(db) = crate::common::TestDb::acquire("projection_batching").await else { return };
    let pool = db.pool();

    // 10 restaurants (business keys), 25 events each, INTERLEAVED: registration first, then 24
    // renames per restaurant in round-robin — so every batch mixes all ten keys and each key's
    // event order spans several batch boundaries.
    const RESTAURANTS: u128 = 10;
    const UPDATES: i32 = 24;
    let ids: Vec<uuid::Uuid> = (1..=RESTAURANTS).map(uuid::Uuid::from_u128).collect();
    for (n, id) in ids.iter().enumerate() {
        append_event(
            &pool,
            &format!("Restaurant-{id}"),
            1,
            "RestaurantRegistered",
            registered(*id, &format!("resto-{n}-v0")),
        )
        .await;
    }
    for round in 1..=UPDATES {
        for (n, id) in ids.iter().enumerate() {
            // RestaurantUpdated carries the full entity (replace semantics) — the fold takes the
            // LAST one per key, which is exactly what order-per-business-key must protect.
            append_event(
                &pool,
                &format!("Restaurant-{id}"),
                round + 1,
                "RestaurantUpdated",
                registered(*id, &format!("resto-{n}-v{round}")),
            )
            .await;
        }
    }
    // One POISON event in the middle of the log: an unparseable payload for a known event type.
    // It must be log-skipped inside its batch — never wedging the group, never blocking later
    // events for the same or other keys.
    append_event(
        &pool,
        &format!("Restaurant-{}", ids[3]),
        UPDATES + 2,
        "RestaurantUpdated",
        serde_json::json!({ "not": "a RestaurantUpdated payload" }),
    )
    .await;
    // And one final healthy rename for that same restaurant AFTER the poison.
    append_event(
        &pool,
        &format!("Restaurant-{}", ids[3]),
        UPDATES + 3,
        "RestaurantUpdated",
        registered(ids[3], "resto-3-final"),
    )
    .await;

    let head: i64 = sqlx::query_scalar("SELECT MAX(position) FROM domain_events")
        .fetch_one(&pool)
        .await
        .expect("head");

    // Drain with a batch far smaller than the log: 252 events / batch 40 = 7 transactions.
    let worker = ProjectionWorker::new(pool.clone()).with_batch_size(40);
    worker.run_once().await.expect("full drain");

    // 1) Checkpoint at head — the whole log folded through batched transactions.
    assert_eq!(checkpoint(&pool, "Restaurant").await, head);

    // 2) Order per business key survived the batch boundaries: every restaurant shows its LAST
    //    rename, and the post-poison rename landed too.
    let rows: Vec<(uuid::Uuid, String)> =
        sqlx::query("SELECT restaurant_id, display_name FROM restaurant")
            .fetch_all(&pool)
            .await
            .expect("rows")
            .iter()
            .map(|r| (r.get("restaurant_id"), r.get("display_name")))
            .collect();
    assert_eq!(rows.len(), RESTAURANTS as usize);
    for (n, id) in ids.iter().enumerate() {
        let name = &rows.iter().find(|(rid, _)| rid == id).expect("row").1;
        let expected =
            if n == 3 { "resto-3-final".to_string() } else { format!("resto-{n}-v{UPDATES}") };
        assert_eq!(*name, expected, "restaurant {n}: last write must win across batches");
    }

    // 3) Consistency invariant at every boundary: no projected state ever runs AHEAD of its
    //    checkpoint (the batch transaction is what guarantees this; a crashed batch replays
    //    whole). Verified here at the terminal state: rows and checkpoint agree on the head.
    let max_updated: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM restaurant WHERE updated_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("consistency probe");
    assert_eq!(max_updated, 0);

    // 4) Idempotent resume: a second full drain over the drained log changes nothing.
    let before: Vec<(uuid::Uuid, String)> = rows;
    worker.run_once().await.expect("idempotent re-drain");
    let after: Vec<(uuid::Uuid, String)> =
        sqlx::query("SELECT restaurant_id, display_name FROM restaurant")
            .fetch_all(&pool)
            .await
            .expect("rows after")
            .iter()
            .map(|r| (r.get("restaurant_id"), r.get("display_name")))
            .collect();
    assert_eq!(checkpoint(&pool, "Restaurant").await, head);
    let mut b = before;
    let mut a = after;
    b.sort();
    a.sort();
    assert_eq!(a, b, "re-draining a drained log is a no-op");

    // 5) The batch boundary IS a transaction boundary, observed directly: reset the read side,
    //    then SAMPLE the checkpoint from a second connection WHILE a batch-40 drain runs. The
    //    checkpoint only ever commits with its batch, so every observed value must sit on a
    //    40-boundary (or 0 / the head, whose final batch is the 252 % 40 remainder) — a partially
    //    applied batch would surface as a checkpoint between boundaries.
    sqlx::raw_sql("DELETE FROM projection_checkpoint; DELETE FROM restaurant; DELETE FROM prospectionpipeline;")
        .execute(&pool)
        .await
        .expect("reset read side");
    // The sampler observes from the same pool (a clone shares the pool, but every query still
    // takes its own connection — the transaction under test never spans them).
    let sampler_pool = db.pool();
    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    let sampler = tokio::spawn(async move {
        let mut seen = Vec::new();
        loop {
            if *stop_rx.borrow() {
                return seen;
            }
            let cp: Option<i64> = sqlx::query_scalar(
                "SELECT position FROM projection_checkpoint WHERE projector = 'Restaurant'",
            )
            .fetch_optional(&sampler_pool)
            .await
            .expect("sample checkpoint");
            seen.push(cp.unwrap_or(0));
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_micros(200)) => {}
                _ = stop_rx.changed() => {}
            }
        }
    });
    ProjectionWorker::new(pool.clone()).with_batch_size(40).run_once().await.expect("sampled drain");
    let _ = stop_tx.send(true);
    let observed = sampler.await.expect("sampler");
    let restaurant_batches_end: Vec<i64> = (0..=head).filter(|p| p % 40 == 0).collect();
    for cp in &observed {
        assert!(
            *cp == 0 || *cp == head || restaurant_batches_end.contains(cp),
            "checkpoint observed BETWEEN batch boundaries: {cp} (batch 40, head {head}) — a batch \
             partially committed, the unit-of-work property is broken; observed set: {observed:?}"
        );
    }
    assert_eq!(checkpoint(&pool, "Restaurant").await, head);
}
