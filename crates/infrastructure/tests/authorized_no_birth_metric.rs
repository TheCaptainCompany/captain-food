//! THE BIRTH-GAP DEAD-MAN'S SWITCH, proved by looking at the series (#608).
//!
//! *A customer's card is authorized and no order exists.* Before this chunk nothing in the system
//! could say that had happened: `payment_authorized_unsettled_age_seconds` is computed over the
//! OrderTracking read model, and an order that was never born has no OrderTracking row — so the one
//! declared switch was structurally blind to exactly the case it was cited for. Both it and the new
//! gauge had ZERO emit sites in `crates/**`.
//!
//! **The state is manufactured honestly, never inserted.** The two stranded authorizations below
//! are produced by driving the real seams end to end:
//!
//! 1. a real `PlaceOrder` delivered on the real `PlaceOrderProcess` COMMAND lane — which is what
//!    creates the `payment_process_manager` run row at CHECKOUT time (`commands.rs`, the fact the
//!    detector's whole source resolution rests on: an authorization the saga never processed still
//!    has a run row);
//! 2. a real `PaymentAuthorized` delivered on the real `Payment` lane — which records the fact on
//!    the Payment stream AND chains the PM-addressed hop onto the PlaceOrderProcess lane, in one
//!    transaction;
//! 3. **and then nothing.** The PlaceOrderProcess lane is simply not drained. That is the crash
//!    window between two durable hops — a shape that survives #590/#595/#601 and widens after the
//!    `ROUTE_ORDER_BIRTH_THROUGH_LANE` flip, because the flip adds a third hop.
//!
//! No test here inserts a `payment_process_manager` row, an `inbound_messages` hop, or a
//! `domain_events` row by hand. The one thing moved by hand is the CLOCK: `received_at` /
//! `occurred_at` are backdated after the real write, because `extract(epoch ...)::bigint` truncates
//! and two rows created milliseconds apart both read 0 — a value-derived assertion is inexpressible
//! without ageing. Ageing an existing real row is not manufacturing one.
//!
//! **What is asserted, and why in this shape (beck's zero-healthy suite).** A monitor whose HEALTHY
//! value is zero cannot be verified by "the number is 0":
//!
//! - **presence** — with nothing stranded, a data point EXISTS for every declared reason with
//!   value 0, asserted BY EQUALITY over the full point set. `contains` cannot see a member that
//!   stopped reporting, which is the failure the zero contract exists to prevent.
//! - **a VALUE-DERIVED positive control** — two stranded authorizations at DISTINCT ages must make
//!   the gauge read the OLDER, and a second scenario at a different age must give a DIFFERENT
//!   value. Without the second value, a watcher emitting a latched constant passes everything
//!   above, and a monitor emitting garbage is worse than one emitting nothing because it looks
//!   alive.
//! - **a SAME-SWEEP negative control** — an authorization WHOSE ORDER WAS BORN, present in the same
//!   database on the same tick, must not be counted. Without it, "count every authorization" passes.
//! - **repetition** — a second tick over the unchanged state must re-emit. Delta temporality plus a
//!   draining read makes a once-at-startup emitter indistinguishable from a correct one on tick 1.
//! - **recovery** — birth the orders and the next sweep must return to 0. A gauge that only ever
//!   rises is a gauge nobody can close an incident on.
//!
//! **Its own binary, ONE `#[tokio::test]`**: `telemetry::meters` binds `opentelemetry::global::meter`
//! once per process (`OnceLock`), so the spy provider must be installed before the process's FIRST
//! metric call — unachievable in the ~30-suite `main` binary, and unstable with a second test fn
//! racing the same binding. Same constraint and same shape as `tests/mailbox_liveness_metrics.rs`
//! and `tests/orders_placed_metric.rs`.
//!
//! **SEEN RED — seven mutants, each a SEMANTIC edit of `mailbox/birth_gap_watch.rs` applied to the
//! committed phase, with an applied-check AND a revert-check** (never a line range: a range rots at
//! the next commit). All seven red on apply, all seven green on `git checkout --`:
//!
//! | Mutant | The edit | Where it dies |
//! |---|---|---|
//! | delete the emit | drop the `no_order_birth_age` loop — literally the pre-#608 world | presence, `left: []` |
//! | invert the predicate | `process_status <> 'AWAITING_PAYMENT_RESULT'` | positive control, `retry_pending` 0 vs 3600 |
//! | return 0 | emit `0` whatever the query found | positive control, every reason 0 |
//! | latch the value | memoize the first non-zero reading and re-emit it forever | the SECOND value control, 3600 vs 7200 |
//! | emit once | a `OnceLock` around the emit — the once-at-startup seeder | the re-emission assertion, `left: []` |
//! | re-point at OrderTracking | source the population from the read model — TODAY'S BLINDNESS | positive control, `retry_pending` 0 (a never-born order has no row) |
//! | drop the no-birth exclusion | count every hop, born or not | the negative control, `delivery_exhausted` 7200 vs 0 |
//!
//! **Known coverage gap, stated rather than faked**: `delivery_exhausted` is asserted PRESENT AT
//! ZERO on every tick and is covered by the predicate mutants, but is not driven to a positive
//! value here. Producing it honestly needs a PM hop that reaches a terminal status while its run
//! row stays `AWAITING_PAYMENT_RESULT`, and every route to that state in the current runtime is
//! either an induced infrastructure fault (not a seam) or resolves the run. Manufacturing it by
//! hand would be inserting the row the detector queries, which is the one thing this binary does
//! not do.
//!
//! Needs `DATABASE_URL`: since #474 a missing database FAILS this suite; only an explicit
//! `DB_TESTS_REQUIRED=0` skips it, and that leaves a receipt (`crates/db_test_gate`).

// The TestDb witness and the spy-meter harness, shared with the `main` binary by path rather than
// duplicated (#335, ADR-20260808-224500 item 5; #589 for the spy).
#[path = "main/common.rs"]
mod common;
#[path = "main/spy_meter.rs"]
mod spy_meter;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use actor_runtime::{MailboxWorker, WorkerConfig};
use application::generated::services::{
    IdentityService, PaymentCaptureInput, PaymentRefundInput, PaymentReleaseInput,
    PaymentRequestInput, PaymentRequestOutput, PaymentService, ServiceCallMeta,
};
use application::ports::{Actor, EventStore};
use async_trait::async_trait;
use domain::generated::entities::{Address, CartLineItem, Money};
use domain::generated::events::{
    CartLineAdded, CartStarted, DomainEvent, PaymentAuthorized, RestaurantActivated,
    RestaurantRegistered,
};
use domain::generated::scalars::*;
use domain::shared::errors::DomainError;
use infrastructure::generated::command_router::CommandDeps;
use infrastructure::mailbox::{birth_gap_watch_tick, MailboxCommandHandler};
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, PgCustomerRepository,
    PgEventStore, PgProspectionRepository, PgRestaurantRepository, PgSlugReservationRepository,
    UnverifiedGbpOrderLinkProbe,
};
use sqlx::PgPool;
use telemetry::contract::metric;

const RESTAURANT: u128 = 0x0E57;
const OFFER: u128 = 0x0FFE;

/// The three declared reasons, spelled out rather than derived from
/// [`telemetry::meters::birth_gap::REASONS`]: an expectation computed from the same constant the
/// emitter iterates would agree with it by construction. [`declared_reasons_are_the_ones_asserted`]
/// keeps the literal honest and tells a future reason-adder exactly what to update.
const REASONS: [&str; 3] = ["delivery_exhausted", "no_run", "retry_pending"];

fn uid(n: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128(n)
}
fn eur(cents: i64) -> Money {
    Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
}

/// One expected `{reason: r}` point at `age`.
fn point(reason: &str, age: f64) -> (BTreeMap<String, String>, f64) {
    (BTreeMap::from([("reason".to_string(), reason.to_string())]), age)
}

/// The full expected point set of ONE tick: every declared reason, in sorted order.
fn all_reasons(ages: &[(&str, f64)]) -> Vec<(BTreeMap<String, String>, f64)> {
    let mut out: Vec<(BTreeMap<String, String>, f64)> = REASONS
        .iter()
        .map(|r| {
            let age = ages.iter().find(|(n, _)| n == r).map(|(_, a)| *a).unwrap_or(0.0);
            point(r, age)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
    out
}

// ─── the stubs the checkout path needs ──────────────────────────────────────────────────────────

/// Fixed-price catalog: the only offer is `OFFER` at 9.80 EUR. Pricing is exercised; the catalog
/// projection is not what this suite is about.
struct StubCatalog;

#[async_trait]
impl application::queries::CatalogReadRepository for StubCatalog {
    async fn by_restaurant(
        &self,
        _restaurant_id: RestaurantId,
    ) -> Result<Option<application::queries::CatalogRow>, DomainError> {
        Ok(None)
    }

    async fn offer_by_id(
        &self,
        _restaurant_id: RestaurantId,
        offer_id: OfferId,
    ) -> Result<Option<application::queries::OfferView>, DomainError> {
        if offer_id != OfferId(uid(OFFER)) {
            return Ok(None);
        }
        Ok(Some(application::queries::OfferView {
            offer_id,
            product_id: ProductId(uid(0xF00D)),
            product_name: ProductName("Kebab".into()),
            offer_name: OfferName("Solo".into()),
            price: eur(980),
            availability: CatalogItemAvailability::AVAILABLE,
            stock_status: StockStatus::IN_STOCK,
            stock_quantity: None,
            option_lists: Vec::new(),
        }))
    }
}

/// A gateway that mints a DISTINCT intent id per call, so the three checkouts below have three
/// intents the chain can correlate independently.
#[derive(Default)]
struct SerialGateway {
    issued: Mutex<usize>,
}

#[async_trait]
impl PaymentService for SerialGateway {
    async fn request(
        &self,
        _input: PaymentRequestInput,
        _meta: &ServiceCallMeta,
    ) -> Result<PaymentRequestOutput, DomainError> {
        let mut n = self.issued.lock().unwrap();
        *n += 1;
        Ok(PaymentRequestOutput {
            payment_intent_id: PaymentIntentId(format!("pi_608_{n}")),
            client_secret: format!("pi_608_{n}_secret"),
        })
    }
    async fn capture(&self, _i: PaymentCaptureInput, _m: &ServiceCallMeta) -> Result<(), DomainError> {
        unreachable!("the checkout path never captures")
    }
    async fn release(&self, _i: PaymentReleaseInput, _m: &ServiceCallMeta) -> Result<(), DomainError> {
        unreachable!("the checkout path never voids")
    }
    async fn refund(&self, _i: PaymentRefundInput, _m: &ServiceCallMeta) -> Result<(), DomainError> {
        unreachable!("the checkout path never refunds")
    }
}

fn deps_over(pool: &PgPool, payments: Arc<dyn PaymentService>) -> CommandDeps {
    CommandDeps {
        store: Arc::new(PgEventStore::new(pool.clone())),
        restaurants: Arc::new(PgRestaurantRepository::new(pool.clone())),
        slugs: Arc::new(PgSlugReservationRepository::new(pool.clone())),
        ownership: Arc::new(FailClosedGoogleOwnershipVerifier),
        probe: Arc::new(UnverifiedGbpOrderLinkProbe),
        prospection: Arc::new(PgProspectionRepository::new(pool.clone())),
        catalogs: Arc::new(StubCatalog),
        auth: Arc::new(FailClosedIdentityService) as Arc<dyn IdentityService>,
        customers: Arc::new(PgCustomerRepository::new(pool.clone())),
        sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
        payments,
        pm_state: Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone())),
        refund_state: Arc::new(infrastructure::persistence::PgRefundProcessState::new(pool.clone())),
        mailbox_requeue: Arc::new(
            infrastructure::persistence::mailbox_lanes::PgMailboxRequeue::new(pool.clone()),
        ),
        enforce_service_hours_guard: false,
        enforce_acceptance_timeout: false,
        route_order_birth_through_lane: false,
    }
}

fn address() -> Address {
    Address {
        line1: AddressLine("1 rue Nationale".into()),
        line2: None,
        postal_code: PostalCode("37000".into()),
        city: CityName("Tours".into()),
        country: CountryCode("FR".into()),
    }
}

fn actor() -> Actor {
    Actor {
        user_id: uid(0xAD),
        user_type: "ADMIN".into(),
        domain_id: None,
        correlation_id: uid(0xC0),
        cause_id: None,
    }
}

/// One ACTIVE restaurant, and one OPEN cart with a line per checkout — through the ordinary store.
async fn seed_world(pool: &PgPool, carts: &[u128]) {
    let store = PgEventStore::new(pool.clone());
    store
        .append(
            &format!("Restaurant-{}", uid(RESTAURANT)),
            0,
            &[
                DomainEvent::RestaurantRegistered(RestaurantRegistered {
                    mode: None,
                    restaurant_id: RestaurantId(uid(RESTAURANT)),
                    account_id: None,
                    listing_status: RestaurantListingStatus::NON_PARTNER,
                    r#ref: None,
                    external_identifiers: Vec::new(),
                    display_name: RestaurantDisplayName("Chez Test".into()),
                    contact: None,
                    website: None,
                    tags: Vec::new(),
                    margin_rate: None,
                    cuisine_category: None,
                    uber_prices_opt_in: None,
                    address: address(),
                    location: None,
                    timezone: None,
                    preparation_time_minutes: None,
                    opening_hours: Vec::new(),
                }),
                DomainEvent::RestaurantActivated(RestaurantActivated {
                    restaurant_id: RestaurantId(uid(RESTAURANT)),
                    reason: None,
                }),
            ],
            &actor(),
        )
        .await
        .expect("restaurant stream");
    for (i, cart) in carts.iter().enumerate() {
        store
            .append(
                &format!("Cart-{}", uid(*cart)),
                0,
                &[
                    DomainEvent::CartStarted(CartStarted {
                        cart_id: CartId(uid(*cart)),
                        restaurant_id: RestaurantId(uid(RESTAURANT)),
                        session_id: SessionId(uid(0x5E55)),
                        customer_id: None,
                    }),
                    DomainEvent::CartLineAdded(CartLineAdded {
                        cart_id: CartId(uid(*cart)),
                        line: CartLineItem {
                            cart_line_id: CartLineId(uid(0x11E0 + i as u128)),
                            offer_id: OfferId(uid(OFFER)),
                            quantity: 1,
                            selected_option_ids: Vec::new(),
                        },
                    }),
                ],
                &actor(),
            )
            .await
            .expect("cart stream");
    }
}

/// Enqueue one row onto a lane, exactly as the production enqueue does (deterministic partition
/// over the width-5 keyspace). This is the DOOR, not the state the detector reads: a `PlaceOrder`
/// command and an inbound Stripe fact are both things the outside world genuinely posts.
async fn enqueue(
    pool: &PgPool,
    kind: &str,
    actor_type: &str,
    actor_id: uuid::Uuid,
    n: u128,
    message_type: &str,
    payload: serde_json::Value,
) {
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, correlation_id, session_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'GRAPHQL', 'PUBLIC', $1, $9)",
    )
    .bind(uid(n))
    .bind(kind)
    .bind(actor_type)
    .bind(actor_id)
    .bind(actor_client::stable_partition(&actor_id, 5))
    .bind(message_type)
    .bind(&payload)
    .bind(format!("h{n}"))
    .bind(uid(0x5E55))
    .execute(pool)
    .await
    .expect("enqueue");
}

async fn worker(pool: &PgPool, actor_type: &str, deps: CommandDeps) -> MailboxWorker {
    let w = MailboxWorker::new(
        pool.clone(),
        &format!("w-{actor_type}"),
        actor_type,
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(deps)),
    );
    w.seed(5).await.expect("seed");
    w.claim().await.expect("claim");
    w
}

/// Drain every owned lane, SURFACING a delivery error — the worker's own `drain` logs and retries,
/// which would hide a broken hop as `delivered == 0` and turn a metric assertion into a mystery.
async fn drain_all(w: &MailboxWorker) -> u64 {
    let mut n = 0;
    for lane in w.owned().await {
        n += w.drain_lane(&lane).await.expect("lane drains clean");
    }
    n
}

/// Move a stranded hop's clock back. The row itself came from the real chain; only its age is
/// synthetic, because a truncating `extract(epoch ...)` cannot tell two rows a millisecond apart
/// apart, and a value-derived control is inexpressible without distinct ages.
async fn age_hop(pool: &PgPool, intent: &str, seconds: i64) {
    let n = sqlx::query(
        "UPDATE inbound_messages SET received_at = now() - make_interval(secs => $2) \
         WHERE actor_type = 'PlaceOrderProcess' AND message_type = 'PaymentAuthorized' \
           AND payload->'payload'->>'paymentIntentId' = $1",
    )
    .bind(intent)
    .bind(seconds as f64)
    .execute(pool)
    .await
    .expect("age the hop")
    .rows_affected();
    assert_eq!(n, 1, "exactly one real chained hop for {intent} — ageing nothing tests nothing");
}

/// Same, for the log residue the `no_run` reason reads.
async fn age_fact(pool: &PgPool, intent: &str, seconds: i64) {
    let n = sqlx::query(
        "UPDATE domain_events SET occurred_at = now() - make_interval(secs => $2) \
         WHERE event_type = 'PaymentAuthorized' AND payload->>'paymentIntentId' = $1",
    )
    .bind(intent)
    .bind(seconds as f64)
    .execute(pool)
    .await
    .expect("age the fact")
    .rows_affected();
    assert_eq!(n, 1, "exactly one real PaymentAuthorized fact for {intent}");
}

fn authorized(intent: &str) -> serde_json::Value {
    serde_json::to_value(DomainEvent::PaymentAuthorized(PaymentAuthorized {
        payment_intent_id: PaymentIntentId(intent.into()),
        order_id: None,
        restaurant_id: RestaurantId(uid(RESTAURANT)),
        amount: eur(980),
    }))
    .expect("serialize the inbound fact")
}

/// The literal in [`REASONS`] must stay the DECLARED set the emitter iterates — otherwise the
/// equality assertions below silently stop covering a member somebody added.
#[test]
fn declared_reasons_are_the_ones_asserted() {
    let mut declared = telemetry::meters::birth_gap::REASONS.to_vec();
    declared.sort_unstable();
    let mut asserted = REASONS.to_vec();
    asserted.sort_unstable();
    assert_eq!(
        declared, asserted,
        "REASONS here must equal telemetry::meters::birth_gap::REASONS -- a reason added to the \
         emitter and not here would never be asserted on"
    );
}

#[tokio::test]
async fn stranded_authorization_ages_and_clears() {
    // The spy FIRST — before the database, before anything can bind the process-wide meter.
    let spy = spy_meter::SpyMeter::install();
    let Some(db) = common::TestDb::acquire("authorized_no_birth_metric").await else { return };
    let pool = db.pool();

    const CART_A: u128 = 0xCA71;
    const CART_B: u128 = 0xCA72;
    const CART_C: u128 = 0xCA73;
    const ORDER_A: u128 = 0x0A71;
    const ORDER_B: u128 = 0x0A72;
    const ORDER_C: u128 = 0x0A73;
    seed_world(&pool, &[CART_A, CART_B, CART_C]).await;

    let gateway = Arc::new(SerialGateway::default());
    let pm = worker(&pool, "PlaceOrderProcess", deps_over(&pool, gateway.clone())).await;
    let payment = worker(&pool, "Payment", deps_over(&pool, gateway.clone())).await;

    // ── (a) PRESENCE ────────────────────────────────────────────────────────────────────────────
    // Nothing stranded, nothing authorized, nothing even ordered: every declared reason must still
    // produce a data point, at 0. By EQUALITY over the whole set — a `contains` assertion cannot
    // see the member that stopped reporting, which is the entire failure mode.
    birth_gap_watch_tick(&pool).await.expect("a tick over an empty database");
    let s = spy.drain();
    assert_eq!(
        s.points(metric::PAYMENT_AUTHORIZED_NO_ORDER_BIRTH_AGE_SECONDS),
        all_reasons(&[]),
        "every declared reason reports 0 on an empty database -- an ABSENT series must never be \
         readable as 'nothing is stranded'"
    );
    assert_eq!(
        s.points(metric::PAYMENT_AUTHORIZED_UNSETTLED_AGE_SECONDS),
        vec![(BTreeMap::new(), 0.0)],
        "the born-but-never-captured gauge rides the same sweep and is emitted on every tick too"
    );
    assert_eq!(
        s.points(metric::PAYMENT_BIRTH_GAP_SWEEP_HEARTBEAT_TOTAL),
        vec![(BTreeMap::new(), 1.0)],
        "one completed sweep, one heartbeat -- the only thing that separates 'healthy zero' from \
         'the sweep is dead'"
    );
    assert!(
        !s.points(metric::PAYMENT_BIRTH_GAP_SWEEP_HEARTBEAT_TOTAL).is_empty(),
        "spy provider not installed before first meter call"
    );

    // ── three real checkouts ────────────────────────────────────────────────────────────────────
    // The `PlaceOrder` handler is what opens the run row, at CHECKOUT, at AWAITING_PAYMENT_RESULT —
    // the fact the detector's source resolution rests on.
    for (i, (cart, order)) in
        [(CART_A, ORDER_A), (CART_B, ORDER_B), (CART_C, ORDER_C)].iter().enumerate()
    {
        enqueue(
            &pool,
            "COMMAND",
            "PlaceOrderProcess",
            uid(*order),
            0x100 + i as u128,
            "PlaceOrder",
            serde_json::json!({
                "orderId": uid(*order),
                "restaurantId": uid(RESTAURANT),
                "cartId": uid(*cart),
                "customerId": uid(0xC057),
                "customerContact": { "displayName": "Johnny", "phone": "+33612345678" },
                "serviceType": "COLLECTION",
                "paymentMethodId": "pm_card_visa",
            }),
        )
        .await;
    }
    assert_eq!(drain_all(&pm).await, 3, "three checkouts accepted");
    let runs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM payment_process_manager WHERE process_status = 'AWAITING_PAYMENT_RESULT'",
    )
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(runs, 3, "three run rows, opened by the real PlaceOrder handler");

    // ── (c) the SAME-SWEEP NEGATIVE CONTROL, born for real ──────────────────────────────────────
    // C's authorization is delivered AND its PM hop is drained, so C's order is born. It sits in
    // the same database on the same tick as A and B and must not be counted.
    enqueue(&pool, "EVENT", "Payment", uid(0xE003), 0x203, "PaymentAuthorized", authorized("pi_608_3"))
        .await;
    assert_eq!(drain_all(&payment).await, 1, "C's authorization recorded and chained");
    assert_eq!(drain_all(&pm).await, 1, "C's chained hop delivered -- the order is BORN");
    let born: i64 =
        sqlx::query_scalar("SELECT count(*) FROM domain_events WHERE event_type = 'OrderPlaced'")
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(born, 1, "exactly one order born -- C's");

    // ── A and B: authorized, and then NOTHING ───────────────────────────────────────────────────
    // The Payment lane records both facts and chains both hops; the PlaceOrderProcess lane is not
    // drained again. That IS the crash window between two durable hops.
    enqueue(&pool, "EVENT", "Payment", uid(0xE001), 0x201, "PaymentAuthorized", authorized("pi_608_1"))
        .await;
    enqueue(&pool, "EVENT", "Payment", uid(0xE002), 0x202, "PaymentAuthorized", authorized("pi_608_2"))
        .await;
    assert_eq!(drain_all(&payment).await, 2, "A and B authorized -- funds held");

    // The `no_run` residue: the real recording seam, for an intent no checkout ever opened a run
    // for. This is the crash between PlaceOrder's two sequential unfenced writes — the Stripe
    // intent exists, the run row does not — and it is visible ONLY in the log.
    application::payments::record_inbound_payment_event(
        &PgEventStore::new(pool.clone()),
        DomainEvent::PaymentAuthorized(PaymentAuthorized {
            payment_intent_id: PaymentIntentId("pi_608_orphan".into()),
            order_id: None,
            restaurant_id: RestaurantId(uid(RESTAURANT)),
            amount: eur(980),
        }),
        &actor(),
    )
    .await
    .expect("the real seam records an orphaned authorization -- facts are never dropped");

    age_hop(&pool, "pi_608_1", 3600).await;
    age_hop(&pool, "pi_608_2", 60).await;
    age_fact(&pool, "pi_608_orphan", 120).await;

    // ── (b) the VALUE-DERIVED POSITIVE CONTROL ──────────────────────────────────────────────────
    birth_gap_watch_tick(&pool).await.expect("a tick with two stranded authorizations");
    let s = spy.drain();
    assert_eq!(
        s.points(metric::PAYMENT_AUTHORIZED_NO_ORDER_BIRTH_AGE_SECONDS),
        all_reasons(&[("retry_pending", 3600.0), ("no_run", 120.0)]),
        "the gauge reads the OLDEST of each class -- 3600s (A, not B's 60s) for the stranded saga \
         hops, 120s for the log residue, and delivery_exhausted still reports its zero"
    );

    // ── (d) SECOND TICK over UNCHANGED state ────────────────────────────────────────────────────
    // Delta temporality makes a once-at-startup emitter drain identically to a correct one on tick
    // 1; this is the only assertion that can tell them apart.
    birth_gap_watch_tick(&pool).await.expect("a second tick");
    let s = spy.drain();
    assert_eq!(
        s.points(metric::PAYMENT_AUTHORIZED_NO_ORDER_BIRTH_AGE_SECONDS),
        all_reasons(&[("retry_pending", 3600.0), ("no_run", 120.0)]),
        "EVERY tick re-emits -- a series that reports once is a dead-man's switch that fires once"
    );
    assert_eq!(
        s.points(metric::PAYMENT_BIRTH_GAP_SWEEP_HEARTBEAT_TOTAL),
        vec![(BTreeMap::new(), 1.0)],
        "the heartbeat increments on every completed sweep, not only the first"
    );

    // ── (b') a DIFFERENT age gives a DIFFERENT value ────────────────────────────────────────────
    // Without this, a watcher emitting a latched constant passes everything above.
    age_hop(&pool, "pi_608_1", 7200).await;
    birth_gap_watch_tick(&pool).await.expect("a tick after the oldest aged further");
    assert_eq!(
        spy.drain().points(metric::PAYMENT_AUTHORIZED_NO_ORDER_BIRTH_AGE_SECONDS),
        all_reasons(&[("retry_pending", 7200.0), ("no_run", 120.0)]),
        "the value is DERIVED from the data, not latched -- a constant emitter dies here"
    );

    // ── (e) RECOVERY ────────────────────────────────────────────────────────────────────────────
    // Deliver the two stranded hops for real. The runs resolve, the orders are born, and the next
    // sweep must return to zero — a gauge nobody can close an incident on is not a gauge.
    assert_eq!(drain_all(&pm).await, 2, "A and B's hops delivered -- both orders born");
    let born: i64 =
        sqlx::query_scalar("SELECT count(*) FROM domain_events WHERE event_type = 'OrderPlaced'")
            .fetch_one(&pool)
            .await
            .expect("count");
    assert_eq!(born, 3, "all three orders exist now");
    birth_gap_watch_tick(&pool).await.expect("a tick after recovery");
    assert_eq!(
        spy.drain().points(metric::PAYMENT_AUTHORIZED_NO_ORDER_BIRTH_AGE_SECONDS),
        all_reasons(&[("no_run", 120.0)]),
        "the saga hops cleared; the log residue is a DIFFERENT anomaly and correctly still reads \
         120s -- birthing an order does not retroactively give the orphan a run"
    );
}
