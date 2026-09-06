//! `PgAsOfCatalogRepository` (PROP-20260831-134539 slice 2 of "the priced quote token") against a
//! real Postgres: the SQL `version <= $2` predicate bounds the read, and each event's own
//! `CatalogVersion` (never a slice index) is what actually decides which events fold; a coordinate
//! beyond head fails closed. Needs `DATABASE_URL` (see `main/common.rs`); without it the suite FAILS
//! loudly (#474) unless `DB_TESTS_REQUIRED=0`.

#[path = "main/common.rs"]
mod common;

use std::time::{Duration, Instant};

use application::ports::{Actor, AsOfPriceAuthority, EventStore};
use domain::catalog_as_of::CatalogVersion;
use domain::generated::events::{CatalogCreated, DomainEvent, ProductAdded, ProductUpdated};
use domain::generated::scalars::{CatalogId, CatalogName, OfferId, ProductId, RestaurantId};
use infrastructure::{PgAsOfCatalogRepository, PgEventStore};
use sqlx::PgPool;

fn test_actor() -> Actor {
    Actor {
        user_id: uuid::Uuid::nil(),
        user_type: "ADMIN".to_string(),
        domain_id: None,
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
    }
}

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

fn product_payload(
    catalog_id: uuid::Uuid,
    restaurant_id: uuid::Uuid,
    product_id: uuid::Uuid,
    offer_id: uuid::Uuid,
    name: &str,
    price_cents: i64,
) -> serde_json::Value {
    serde_json::json!({
        "catalogId": catalog_id,
        "restaurantId": restaurant_id,
        "product": {
            "id": product_id,
            "catalogId": catalog_id,
            "restaurantId": restaurant_id,
            "name": name,
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
    if n <= 0 {
        return;
    }
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

/// Bulk-seed `n` DISTINCT `ProductAdded` facts — the EXPENSIVE replace arm (D2, beck NB4/dba B1): a
/// production-shaped read is dominated by whole-menu content, not by identical stock rows.
async fn bulk_seed_products(
    pool: &PgPool,
    stream_name: &str,
    catalog_id: uuid::Uuid,
    restaurant_id: uuid::Uuid,
    first_version: i32,
    n: i32,
) -> Vec<(uuid::Uuid, uuid::Uuid)> {
    let ids: Vec<uuid::Uuid> = (0..n).map(|_| uuid::Uuid::new_v4()).collect();
    let versions: Vec<i32> = (0..n).map(|i| first_version + i).collect();
    let correlation_ids: Vec<uuid::Uuid> = (0..n).map(|_| uuid::Uuid::new_v4()).collect();
    let product_ids: Vec<uuid::Uuid> = (0..n).map(|_| uuid::Uuid::new_v4()).collect();
    let offer_ids: Vec<uuid::Uuid> = (0..n).map(|_| uuid::Uuid::new_v4()).collect();
    let payloads: Vec<serde_json::Value> = (0..n)
        .map(|i| {
            product_payload(
                catalog_id,
                restaurant_id,
                product_ids[i as usize],
                offer_ids[i as usize],
                &format!("Product {i}"),
                1_000 + i as i64,
            )
        })
        .collect();
    let event_types: Vec<String> = (0..n).map(|_| "ProductAdded".to_string()).collect();
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
    .expect("bulk seed ProductAdded");
    product_ids.into_iter().zip(offer_ids).collect()
}

/// One `CatalogImported` carrying `products` distinct products — a full HubRise resync, the OTHER
/// expensive replace arm.
fn catalog_imported_payload(
    catalog_id: uuid::Uuid,
    restaurant_id: uuid::Uuid,
    products: &[(uuid::Uuid, uuid::Uuid)],
) -> serde_json::Value {
    let products: Vec<serde_json::Value> = products
        .iter()
        .enumerate()
        .map(|(i, (product_id, offer_id))| {
            serde_json::json!({
                "id": product_id,
                "catalogId": catalog_id,
                "restaurantId": restaurant_id,
                "name": format!("Imported {i}"),
                "taxRate": { "delivery": 10.0 },
                "offers": [{
                    "id": offer_id,
                    "productId": product_id,
                    "name": "Default",
                    "price": { "amountCents": 2_000 + i as i64, "currency": "EUR" },
                    "availability": "AVAILABLE"
                }]
            })
        })
        .collect();
    serde_json::json!({
        "catalogId": catalog_id,
        "restaurantId": restaurant_id,
        "source": "HUBRISE",
        "categories": [],
        "products": products,
        "optionLists": [],
    })
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
    let product = |price_cents: i64| product_payload(catalog_id, restaurant_id, product_id, offer_id, "Margherita", price_cents);
    append_event(&pool, &stream, 2, "ProductAdded", product(1500)).await;
    append_event(&pool, &stream, 3, "ProductUpdated", product(1900)).await;
    append_event(&pool, &stream, 4, "ProductUpdated", product(2500)).await;

    let repo = PgAsOfCatalogRepository::new(pool.clone());

    // Coordinate V=2 (the 1-based domain_events.version, verbatim) => events_applied must be
    // exactly 2 (CatalogCreated + the first ProductAdded).
    let rows = repo.fetch_rows(&stream, CatalogVersion::try_new(2).unwrap()).await.expect("fetch_rows");
    assert_eq!(rows.len(), 2, "rows exceed the coordinate: got {} rows for version=2", rows.len());

    let as_of = repo.as_of(CatalogId(catalog_id), CatalogVersion::try_new(2).unwrap()).await.expect("as_of");
    let price = as_of.price_of(OfferId(offer_id), &[]).expect("offer exists at V=2");
    assert_eq!(price.unit_price.amount_cents.0, 1500, "the read must not see the later updates");

    // The live head (V=4) sees the latest update -- proves the bound is real, not a side effect of
    // an always-empty-after-first-event fixture.
    let head = repo.as_of(CatalogId(catalog_id), CatalogVersion::try_new(4).unwrap()).await.expect("as_of head");
    let head_price = head.price_of(OfferId(offer_id), &[]).expect("offer exists at head");
    assert_eq!(head_price.unit_price.amount_cents.0, 2500);
}

/// PROP-20260831-134539:547 (red-first, round 2) — a coordinate beyond head is REFUSED, never
/// silently priced at HEAD. Mutant: drop the last-version-equals-V check in the adapter.
#[tokio::test]
async fn a_coordinate_beyond_head_is_refused_never_head_priced() {
    let Some(db) = common::TestDb::acquire("as_of_catalog_read_beyond_head").await else { return };
    let pool = db.pool();

    let catalog_id = uuid::Uuid::new_v4();
    let restaurant_id = uuid::Uuid::new_v4();
    let stream = domain::catalog::stream(CatalogId(catalog_id));
    append_event(
        &pool,
        &stream,
        1,
        "CatalogCreated",
        serde_json::json!({ "catalogId": catalog_id, "restaurantId": restaurant_id, "name": "Main" }),
    )
    .await;

    let repo = PgAsOfCatalogRepository::new(pool.clone());
    let err = repo
        .as_of(CatalogId(catalog_id), CatalogVersion::try_new(5).unwrap())
        .await
        .expect_err("a coordinate beyond head must be refused, never priced at HEAD");
    let message = format!("{err:?}");
    assert!(
        message.contains("absent or beyond head"),
        "unexpected error shape (as_of returned Ok? or a different message): {message}"
    );
}

/// PROP-20260831-134539:547 (slice 3a, D2) — `at_head` prices the LIVE head and returns the exact
/// coordinate it verified — the SAME number a subsequent `as_of` call at that coordinate would
/// demand, never a HEAD price with no coordinate to name it.
#[tokio::test]
async fn at_head_prices_the_live_head_and_returns_its_coordinate() {
    let Some(db) = common::TestDb::acquire("as_of_catalog_read_at_head").await else { return };
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
        product_payload(catalog_id, restaurant_id, product_id, offer_id, "Margherita", price_cents)
    };
    append_event(&pool, &stream, 2, "ProductAdded", product(1500)).await;
    append_event(&pool, &stream, 3, "ProductUpdated", product(1900)).await;

    let repo = PgAsOfCatalogRepository::new(pool.clone());
    let (as_of, coordinate) = repo.at_head(CatalogId(catalog_id)).await.expect("at_head reads the live head");
    assert_eq!(coordinate, CatalogVersion::try_new(3).unwrap(), "at_head must return version 3, the live head");
    assert_eq!(as_of.coordinate(), coordinate, "AsOfCatalog::coordinate must equal the returned coordinate");
    let price = as_of.price_of(OfferId(offer_id), &[]).expect("offer exists at head");
    assert_eq!(price.unit_price.amount_cents.0, 1900, "at_head must price the LATEST update, not a stale one");
}

/// PROP-20260831-134539:547 (slice 3a, D2) — a catalog that was never created (empty stream) is
/// REFUSED, never a HEAD price for a coordinate that does not exist.
#[tokio::test]
async fn at_head_refuses_a_catalog_that_was_never_created() {
    let Some(db) = common::TestDb::acquire("as_of_catalog_read_at_head_absent").await else { return };
    let pool = db.pool();

    let repo = PgAsOfCatalogRepository::new(pool.clone());
    let err = repo
        .at_head(CatalogId(uuid::Uuid::new_v4()))
        .await
        .expect_err("at_head on a never-created catalog must refuse");
    let message = format!("{err:?}");
    assert!(
        message.contains("no rows") || message.contains("catalog not created"),
        "unexpected error shape (at_head returned Ok? or a different message): {message}"
    );
}

/// PROP-20260831-134539:547 (red-first, round 2; THE FARLEY GATE) — the version RETURNED by a real
/// `EventStore::append` is the exact coordinate `as_of` reads: appending a further event must never
/// be visible through a read at the version returned BEFORE that append. Mutant: reintroduce the `+1`
/// on the SQL ceiling.
#[tokio::test]
async fn the_version_returned_by_append_is_the_coordinate_as_of_reads() {
    let Some(db) = common::TestDb::acquire("as_of_catalog_read_append_version").await else { return };
    let pool = db.pool();

    let store = PgEventStore::new(pool.clone());
    let actor = test_actor();
    let catalog_id = CatalogId(uuid::Uuid::new_v4());
    let restaurant_id = RestaurantId(uuid::Uuid::new_v4());
    let product_id = ProductId(uuid::Uuid::new_v4());
    let offer_id = OfferId(uuid::Uuid::new_v4());
    let stream = domain::catalog::stream(catalog_id);

    let created = DomainEvent::CatalogCreated(CatalogCreated {
        catalog_id,
        r#ref: None,
        restaurant_id,
        name: CatalogName("Main".into()),
    });
    let v1 = store.append(&stream, 0, std::slice::from_ref(&created), &actor).await.expect("append CatalogCreated");
    assert_eq!(v1, 1, "the first event on a stream is version 1 (ADR-20260808-171056)");

    let product = |price_cents: i64| domain::generated::entities::Product {
        id: product_id,
        r#ref: None,
        catalog_id,
        restaurant_id,
        category_ref: None,
        name: domain::generated::scalars::ProductName("Margherita".into()),
        description: None,
        tags: vec![],
        image_ids: vec![],
        tax_rate: domain::generated::entities::TaxRate {
            delivery: domain::generated::scalars::TaxRatePercent(10.0),
            collection: None,
            eat_in: None,
        },
        offers: vec![domain::generated::entities::Offer {
            id: offer_id,
            r#ref: None,
            product_id,
            name: domain::generated::scalars::OfferName("Default".into()),
            price: domain::generated::entities::Money {
                amount_cents: domain::generated::scalars::MoneyCents(price_cents),
                currency: domain::generated::scalars::CurrencyCode("EUR".into()),
            },
            availability: domain::generated::scalars::CatalogItemAvailability::AVAILABLE,
            stock: None,
            option_list_ids: vec![],
        }],
    };
    let added = DomainEvent::ProductAdded(ProductAdded {
        catalog_id,
        restaurant_id,
        product: product(1500),
    });
    let v2 = store.append(&stream, v1, std::slice::from_ref(&added), &actor).await.expect("append ProductAdded");
    assert_eq!(v2, 2);

    let updated = DomainEvent::ProductUpdated(ProductUpdated {
        catalog_id,
        restaurant_id,
        product: product(1900),
    });
    let v3 = store.append(&stream, v2, std::slice::from_ref(&updated), &actor).await.expect("append ProductUpdated");
    assert_eq!(v3, 3);

    let repo = PgAsOfCatalogRepository::new(pool.clone());
    let as_of = repo
        .as_of(catalog_id, CatalogVersion::try_new(v2).unwrap())
        .await
        .expect("as_of at the version append RETURNED for the ProductAdded");
    let price = as_of.price_of(offer_id, &[]).expect("offer exists at v2");
    assert_eq!(
        price.unit_price.amount_cents.0, 1500,
        "as_of(the returned version) must not see the event appended AFTER it (v3, price 1900)"
    );
}

/// THE BENCHMARK — dba's lane, TWO arms:
///
/// **Arm (a), V = 200 (the MUTANT DETECTOR, dba B1/beck B2):** far short of head on purpose. Its
/// assertion is `events_applied`, never a timer -- "the fold loads the whole stream ignoring V" is
/// what this arm exists to catch, and events_applied catches it regardless of how fast or slow the
/// container happens to be that day.
///
/// **Arm (b), V = L (head, round 2 addition, dba B1/business NB1):** the read PRODUCTION ACTUALLY
/// PERFORMS. A checkout reads ~L rows dominated by `CatalogImported`/`ProductAdded`, not 200 -- V=200
/// alone was a mutant detector wearing a cost-number costume. L=2,000 is `UNVERIFIED input` (no
/// measured Tours catalog stream length exists; derived from "the largest realistic HubRise import
/// (~500 products) plus a full resync plus a Friday's worth of stock syncs", a judgement call, not a
/// measurement) -- the mix is import-shaped: 500 distinct `ProductAdded`, one `CatalogImported`
/// carrying another 500 products, the remainder cheap `OfferStockUpdated`.
///
/// Both arms print SQL/decode/fold time SEPARATELY plus end-to-end `as_of()` (obs B4: the number must
/// include the fold) -- `median` and `max of 10`, never p95/p99 at N=10 (holub NB4/obs NB7).
#[tokio::test]
async fn fold_to_v_stays_under_ceiling_at_l_events() {
    let Some(db) = common::TestDb::acquire("as_of_catalog_read_ceiling").await else { return };
    let pool = db.pool();

    const L: i32 = 2_000;
    const V_PARTIAL: i64 = 200; // port coordinate (1-based) -- the mutant-detector arm
    const PRODUCTS: i32 = 500; // "~500 products", UNVERIFIED input

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
    // Versions 2..=501: 500 distinct ProductAdded (import-shaped, not identical stock rows).
    bulk_seed_products(&pool, &stream, catalog_id, restaurant_id, 2, PRODUCTS).await;
    // Version 502: one CatalogImported carrying another 500 distinct products (a full HubRise
    // resync) -- the other expensive replace arm.
    let imported_products: Vec<(uuid::Uuid, uuid::Uuid)> =
        (0..PRODUCTS).map(|_| (uuid::Uuid::new_v4(), uuid::Uuid::new_v4())).collect();
    let import_version = PRODUCTS + 2;
    append_event(
        &pool,
        &stream,
        import_version,
        "CatalogImported",
        catalog_imported_payload(catalog_id, restaurant_id, &imported_products),
    )
    .await;
    // First imported offer gets an OfferStockUpdated later on so the mutant-detector arm at V=200
    // still resolves an offer (it queries offer_id, seeded separately below).
    let remaining = L - import_version;
    bulk_seed_offer_stock_updates(
        &pool,
        &stream,
        catalog_id,
        restaurant_id,
        offer_id,
        import_version + 1,
        remaining,
    )
    .await;

    let repo = PgAsOfCatalogRepository::new(pool.clone());

    struct Stats {
        sql_median: Duration,
        sql_max: Duration,
        decode_median: Duration,
        decode_max: Duration,
        fold_median: Duration,
        fold_max: Duration,
        e2e_median: Duration,
        e2e_max: Duration,
        payload_bytes: usize,
        stream_length: usize,
        events_applied: usize,
    }

    async fn measure(
        repo: &PgAsOfCatalogRepository,
        stream: &str,
        catalog_id: uuid::Uuid,
        v: i64,
        iterations: u32,
    ) -> Stats {
        let version = CatalogVersion::try_new(v).unwrap();
        let mut sql_samples = Vec::with_capacity(iterations as usize);
        let mut decode_samples = Vec::with_capacity(iterations as usize);
        let mut fold_samples = Vec::with_capacity(iterations as usize);
        let mut e2e_samples = Vec::with_capacity(iterations as usize);
        let mut payload_bytes = 0usize;
        let mut stream_length = 0usize;
        let mut events_applied = 0usize;

        for i in 0..iterations {
            let sql_start = Instant::now();
            let rows = repo.fetch_rows(stream, version).await.expect("fetch_rows");
            sql_samples.push(sql_start.elapsed());
            stream_length = rows.len();

            // Payload-bytes measurement happens ONCE, OUTSIDE every timed window (dba NB2/round-2
            // card): re-serializing on every iteration inflated the headline by ~28% in round 1.
            if i == 0 {
                payload_bytes = rows.iter().map(|(_, _, payload)| serde_json::to_vec(payload).unwrap().len()).sum();
            }

            let decode_start = Instant::now();
            let events = infrastructure::PgAsOfCatalogRepository::decode_rows(rows).expect("decode_rows");
            decode_samples.push(decode_start.elapsed());
            events_applied = events.len();

            let fold_start = Instant::now();
            let folded = domain::catalog_as_of::AsOfCatalog::from_stream(&events, version);
            fold_samples.push(fold_start.elapsed());
            std::hint::black_box(&folded);

            // End-to-end: the READ `as_of()` actually performs (obs B4) -- a SEPARATE round trip
            // from the split legs above, so the number is honest about what a real caller pays.
            let e2e_start = Instant::now();
            let as_of = repo.as_of(CatalogId(catalog_id), version).await.expect("as_of");
            e2e_samples.push(e2e_start.elapsed());
            std::hint::black_box(&as_of);
        }

        let median_of = |mut s: Vec<Duration>| -> (Duration, Duration) {
            s.sort();
            let median = s[s.len() / 2];
            let max = *s.iter().max().unwrap();
            (median, max)
        };
        let (sql_median, sql_max) = median_of(sql_samples);
        let (decode_median, decode_max) = median_of(decode_samples);
        let (fold_median, fold_max) = median_of(fold_samples);
        let (e2e_median, e2e_max) = median_of(e2e_samples);

        Stats {
            sql_median,
            sql_max,
            decode_median,
            decode_max,
            fold_median,
            fold_max,
            e2e_median,
            e2e_max,
            payload_bytes,
            stream_length,
            events_applied,
        }
    }

    const ITERATIONS: u32 = 10;

    // Arm (a): the mutant detector. Its assertion is events_applied, not a timer.
    let partial = measure(&repo, &stream, catalog_id, V_PARTIAL, ITERATIONS).await;
    assert_eq!(
        partial.events_applied as i64, V_PARTIAL,
        "events_applied exceeds the coordinate: the fold ignored V"
    );
    println!(
        "as_of read, arm (a) V={V_PARTIAL} (mutant detector, lab-measured, peak-unverified): \
         L={L} iterations={ITERATIONS} stream_length={} events_applied={} payload_bytes={} \
         sql median={:?} max_of_{ITERATIONS}={:?} decode median={:?} max_of_{ITERATIONS}={:?} \
         fold median={:?} max_of_{ITERATIONS}={:?} end_to_end median={:?} max_of_{ITERATIONS}={:?}",
        partial.stream_length,
        partial.events_applied,
        partial.payload_bytes,
        partial.sql_median,
        partial.sql_max,
        partial.decode_median,
        partial.decode_max,
        partial.fold_median,
        partial.fold_max,
        partial.e2e_median,
        partial.e2e_max,
    );

    // Assert on the MEDIAN at magnitude scale (round-2 card D2/farley NB3: a wall-clock MAX assert
    // in shared-CI Postgres has no controlling record and is the flaky shape farley's round-1 NAF
    // already warned against) -- ~10x the ~9.5ms end-to-end median measured at authoring on this
    // container. Not an SLO -- business's stop numbers (>150ms end-to-end max escalate, >50ms
    // ship-dark-with-mitigation) are judged on ARM (b) below, against the REAL production-shaped
    // read, by the executor reading the printed numbers -- never by an assert here.
    const PARTIAL_CEILING: Duration = Duration::from_millis(100);
    assert!(
        partial.e2e_median < PARTIAL_CEILING,
        "median exceeds the magnitude-regression ceiling: median={:?} ceiling={PARTIAL_CEILING:?} \
         at L={L} V={V_PARTIAL}",
        partial.e2e_median
    );

    // Arm (b): V = L (head) -- the READ PRODUCTION ACTUALLY PERFORMS (business NB1/dba B1).
    let head = measure(&repo, &stream, catalog_id, L as i64, ITERATIONS).await;
    assert_eq!(
        head.events_applied, head.stream_length,
        "at V=L every returned row must be a business event applied (no technical rows seeded here)"
    );
    println!(
        "as_of read, arm (b) V=L=head (the read production performs, lab-measured, \
         peak-unverified, import-shaped mix): L={L} iterations={ITERATIONS} \
         stream_length={} events_applied={} payload_bytes={} \
         sql median={:?} max_of_{ITERATIONS}={:?} decode median={:?} max_of_{ITERATIONS}={:?} \
         fold median={:?} max_of_{ITERATIONS}={:?} end_to_end median={:?} max_of_{ITERATIONS}={:?} \
         -- business's stop numbers: >150ms end_to_end max escalate before slice 3, >50ms ship dark \
         with a named mitigation",
        head.stream_length,
        head.events_applied,
        head.payload_bytes,
        head.sql_median,
        head.sql_max,
        head.decode_median,
        head.decode_max,
        head.fold_median,
        head.fold_max,
        head.e2e_median,
        head.e2e_max,
    );

    // Assert on the MEDIAN at magnitude scale (~10x the ~85-90ms end-to-end median measured at
    // authoring on this container, round-2 card D2) -- a regression detector, never an SLO, and
    // never a max-based assert in shared-CI Postgres (farley round-1 NAF: no controlling record on
    // a wall-clock MAX assert; the events-count assert already catches the mutant this arm exists
    // for). Business's stop numbers (>150ms end-to-end MAX escalate before slice 3, >50ms ship dark
    // with a named mitigation -- read from the printed line above, not from this assert) are a
    // SEPARATE, protocol-level judgement the executor makes once at authoring time: round 2's
    // measurement (median ~85-90ms, max up to ~140ms across several runs) is in the ship-dark band,
    // recorded with its mitigation in the PR body and PROP-20260831-134539 §12.
    const HEAD_CEILING: Duration = Duration::from_millis(900);
    assert!(
        head.e2e_median < HEAD_CEILING,
        "median exceeds the magnitude-regression ceiling: median={:?} ceiling={HEAD_CEILING:?} \
         at L={L} V=head",
        head.e2e_median
    );
}
