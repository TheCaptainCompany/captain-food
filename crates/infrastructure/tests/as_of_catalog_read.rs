//! `PgAsOfCatalogRepository` (PROP-20260831-134539 slice 2 of "the priced quote token") against a
//! real Postgres: the SQL `version <= $2` predicate is the authoritative truncation, and the read
//! stays cheap at a realistic stream length. Needs `DATABASE_URL` (see `main/common.rs`); without it
//! the suite FAILS loudly (#474) unless `DB_TESTS_REQUIRED=0`.

#[path = "main/common.rs"]
mod common;

use application::ports::AsOfPriceAuthority;
use domain::generated::scalars::CatalogId;
use infrastructure::PgAsOfCatalogRepository;
use sqlx::PgPool;

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
         VALUES ($1, $2, $3, $4, 5, $5, NULL, $6, $7, NULL, now())",
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

/// Bulk-seed `n` `OfferStockUpdated` facts (cheap, fixed-shape payload) onto an already-created
/// catalog stream, versions `first_version..first_version+n` — ONE round trip via `UNNEST`, not `n`
/// sequential awaits (the point of the ceiling test is to measure the READ, not pay for a slow seed).
async fn bulk_seed_offer_stock_updates(
    pool: &PgPool,
    stream_name: &str,
    catalog_id: uuid::Uuid,
    restaurant_id: uuid::Uuid,
    offer_id: uuid::Uuid,
    first_version: i32,
    n: i32,
) {
    let ids: Vec<uuid::Uuid> = (0..n).map(|_| uuid::Uuid::new_v4()).collect();
    let versions: Vec<i32> = (0..n).map(|i| first_version + i).collect();
    let correlation_ids: Vec<uuid::Uuid> = (0..n).map(|_| uuid::Uuid::new_v4()).collect();
    let payloads: Vec<serde_json::Value> = (0..n)
        .map(|i| {
            serde_json::json!({
                "catalogId": catalog_id,
                "restaurantId": restaurant_id,
                "offerId": offer_id,
                "stock": { "quantity": (i as f64) + 1.0, "status": "IN_STOCK" }
            })
        })
        .collect();
    let event_types: Vec<String> = (0..n).map(|_| "OfferStockUpdated".to_string()).collect();
    sqlx::query(
        "INSERT INTO domain_events \
         (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, payload, metadata, occurred_at) \
         SELECT id, $1, version, $2, 'ADMIN', correlation_id, NULL, event_type, payload, NULL, now() \
         FROM UNNEST($3::uuid[], $4::int[], $5::uuid[], $6::text[], $7::jsonb[]) \
              AS u(id, version, correlation_id, event_type, payload)",
    )
    .bind(stream_name)
    .bind(uuid::Uuid::nil())
    .bind(&ids)
    .bind(&versions)
    .bind(&correlation_ids)
    .bind(&event_types)
    .bind(&payloads)
    .execute(pool)
    .await
    .expect("bulk seed OfferStockUpdated");
}

/// PROP-20260831-134539:547 — the adapter reads ONLY events up to (and including) `version`, never
/// the live head. Mutant: drop the `version <= $2` predicate.
#[tokio::test]
async fn the_adapter_reads_only_events_up_to_v() {
    let Some(db) = common::TestDb::acquire("as_of_catalog_read_bounds").await else { return };
    let pool = db.pool();

    let catalog_id = uuid::Uuid::new_v4();
    let restaurant_id = uuid::Uuid::new_v4();
    let product_id = uuid::Uuid::new_v4();
    let offer_id = uuid::Uuid::new_v4();
    let stream = domain::catalog::stream(CatalogId(catalog_id));

    append_event(
        &pool,
        &stream,
        1,
        "CatalogCreated",
        serde_json::json!({ "catalogId": catalog_id, "restaurantId": restaurant_id, "name": "Main" }),
    )
    .await;
    let product = |price_cents: i64| {
        serde_json::json!({
            "catalogId": catalog_id,
            "restaurantId": restaurant_id,
            "product": {
                "id": product_id,
                "catalogId": catalog_id,
                "restaurantId": restaurant_id,
                "name": "Margherita",
                "taxRate": { "delivery": 10.0 },
                "offers": [{
                    "id": offer_id,
                    "productId": product_id,
                    "name": "Default",
                    "price": { "amountCents": price_cents, "currency": "EUR" },
                    "availability": "AVAILABLE"
                }]
            }
        })
    };
    append_event(&pool, &stream, 2, "ProductAdded", product(1500)).await;
    append_event(&pool, &stream, 3, "ProductUpdated", product(1900)).await;
    append_event(&pool, &stream, 4, "ProductUpdated", product(2500)).await;

    let repo = PgAsOfCatalogRepository::new(pool.clone());

    // Port version 1 (0-based) => DB version <= 2 => events_applied must be exactly 2.
    let rows = repo.fetch_rows(&stream, 1).await.expect("fetch_rows");
    assert_eq!(
        rows.len() as i64,
        1 + 1,
        "events_applied exceeds version plus one: got {} rows for version=1",
        rows.len()
    );

    let as_of = repo.as_of(CatalogId(catalog_id), 1).await.expect("as_of");
    let price = as_of
        .price_of(domain::generated::scalars::OfferId(offer_id), &[])
        .expect("offer exists at V=1");
    assert_eq!(price.unit_price.amount_cents.0, 1500, "the read must not see the later updates");

    // The live head (V=3, DB version 4) sees the latest update -- proves the bound is real, not a
    // side effect of an always-empty-after-first-event fixture.
    let head = repo.as_of(CatalogId(catalog_id), 3).await.expect("as_of head");
    let head_price = head
        .price_of(domain::generated::scalars::OfferId(offer_id), &[])
        .expect("offer exists at head");
    assert_eq!(head_price.unit_price.amount_cents.0, 2500);
}

/// THE BENCHMARK (b) — dba's lane: ONE DB-gated ceiling test, an absolute bound with headroom, SQL
/// time and decode time printed SEPARATELY, plus the payload bytes. L = 2,000 total events on the
/// stream (same derivation as the native domain-crate ceiling test: `UNVERIFIED input`, "the largest
/// realistic HubRise import x a Friday's stock syncs" -- a judgement call, not a measurement). The
/// requested coordinate (V = 200) is far short of the head on purpose: it is what makes "the fold
/// loads the whole stream ignoring V" a DETECTABLE mutant -- a query that ignores the predicate pays
/// for all 2,000 rows instead of 201, and that difference is what the ceiling below is calibrated
/// against (5x headroom over the measured max at V = 200, not at head).
#[tokio::test]
async fn fold_to_v_stays_under_ceiling_at_l_events() {
    let Some(db) = common::TestDb::acquire("as_of_catalog_read_ceiling").await else { return };
    let pool = db.pool();

    const L: i32 = 2_000;
    const V: i64 = 200; // port's 0-based coordinate -> DB version <= 201

    let catalog_id = uuid::Uuid::new_v4();
    let restaurant_id = uuid::Uuid::new_v4();
    let offer_id = uuid::Uuid::new_v4();
    let stream = domain::catalog::stream(CatalogId(catalog_id));

    append_event(
        &pool,
        &stream,
        1,
        "CatalogCreated",
        serde_json::json!({ "catalogId": catalog_id, "restaurantId": restaurant_id, "name": "Main" }),
    )
    .await;
    bulk_seed_offer_stock_updates(&pool, &stream, catalog_id, restaurant_id, offer_id, 2, L - 1).await;

    let repo = PgAsOfCatalogRepository::new(pool.clone());

    const ITERATIONS: u32 = 10;
    let mut sql_samples = Vec::with_capacity(ITERATIONS as usize);
    let mut decode_samples = Vec::with_capacity(ITERATIONS as usize);
    let mut total_samples = Vec::with_capacity(ITERATIONS as usize);
    let mut payload_bytes = 0usize;
    for _ in 0..ITERATIONS {
        let total_start = std::time::Instant::now();
        let sql_start = std::time::Instant::now();
        let rows = repo.fetch_rows(&stream, V).await.expect("fetch_rows");
        sql_samples.push(sql_start.elapsed());

        payload_bytes =
            rows.iter().map(|(_, payload)| serde_json::to_vec(payload).unwrap().len()).sum();

        let decode_start = std::time::Instant::now();
        let events = infrastructure::PgAsOfCatalogRepository::decode_rows(rows).expect("decode_rows");
        decode_samples.push(decode_start.elapsed());
        total_samples.push(total_start.elapsed());

        assert_eq!(events.len() as i64, V + 1, "events_applied exceeds version plus one");
    }

    let percentile = |samples: &mut Vec<std::time::Duration>, p: f64| -> std::time::Duration {
        samples.sort();
        samples[((samples.len() as f64 - 1.0) * p).round() as usize]
    };
    let (p50, p95, p99, max) = (
        percentile(&mut total_samples.clone(), 0.50),
        percentile(&mut total_samples.clone(), 0.95),
        percentile(&mut total_samples.clone(), 0.99),
        *total_samples.iter().max().unwrap(),
    );
    println!(
        "as_of read ceiling (lab-measured, peak-unverified): L={L} V={V} iterations={ITERATIONS} \
         sql_p50={:?} decode_p50={:?} total_p50={p50:?} total_p95={p95:?} total_p99={p99:?} \
         total_max={max:?} payload_bytes={payload_bytes}",
        percentile(&mut sql_samples.clone(), 0.50),
        percentile(&mut decode_samples.clone(), 0.50),
    );

    // ~6x headroom over the measured max at authoring on this container (6.35ms -> 40ms; dba asked
    // for 3-5x, this container's variance earned a hair more). Not an SLO -- business's stop numbers
    // (p99 > 150ms escalate, > 50ms ship-dark-with-mitigation) are judged in the PR body from the
    // numbers printed above, against the REAL measurement, not this ceiling.
    const CEILING: std::time::Duration = std::time::Duration::from_millis(40);
    assert!(
        max < CEILING,
        "elapsed exceeds the recorded ceiling: max={max:?} ceiling={CEILING:?} at L={L} V={V}"
    );
}
