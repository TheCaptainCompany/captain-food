//! #639 part C step 5 — the restriction fact terminates the rider's socket
//! (ADR-20260905-065415-the-restriction-fact-terminates-the-rider-s-socket-a-connection-local-standing-read-inside-the-guard-and-one-writer-to-the-transport).
//!
//! The test that could not be written before this record, now written FIRST (§9): a real
//! `graphql-transport-ws` client (`tokio-tungstenite`) against the REAL production router
//! (`server::graphql_routes_with_socket_close_gate`), served on a REAL `127.0.0.1:0` listener — the
//! `rider_sign_in_door.rs` harness's "production router + production deps" idiom, extended to a
//! genuine bound TCP socket because a WebSocket upgrade cannot be exercised through
//! `tower::ServiceExt::oneshot` alone.
//!
//! **State is manufactured honestly**: `RiderRestricted` is appended through the REAL `restrictRider`
//! ADMIN mutation, run to `SUCCEEDED` through the real mailbox worker, over a `PgEventStore`
//! carrying the SAME `EventBus` the WS server's schema is built with — so the fact reaches the
//! socket exactly the way production would, never a raw `domain_events` INSERT for the fact under
//! test (birth facts — `RiderRegistered` — ARE seeded raw, the `rider_standing_walk.rs` idiom, since
//! there is no public `registerRider` mutation).
//!
//! Needs a real Postgres (`DATABASE_URL`); SKIPS (prints and returns) without one, same as every
//! other DB-gated suite here (`db_test_gate`).
//!
//! **Mutants planted, seen red, reverted** (quoted verbatim in the hand-back, per the dispatch):
//! M1 the watcher not spawned (comment out the `tokio::spawn(rider_socket::watch(...))` call) —
//! (1) times out on the positive Close assertion; M2 the stream-match narrowed to a prefix
//! (`evt.stream_name.starts_with(&wanted_stream)`) — (2) closes on another rider's fact; M3
//! `RecvError::Lagged` treated as a bare `continue` (no re-derivation call) — (6)'s RESTRICTED case
//! stays open forever; M4 the standing cell read once at connection_init instead of live
//! (`ctx.data_opt` swapped for a value captured before the watcher could run) — (3) keeps admitting
//! `acceptDelivery` after the fact lands.

#[path = "rider_restriction_closes_the_socket/harness.rs"]
mod harness;

use harness::*;

// ─── (1) an idle rider socket is closed when the restriction fact is appended ───────────────────

#[tokio::test]
async fn an_idle_rider_socket_is_closed_when_the_restriction_fact_is_appended() {
    let Some(url) = db_test_gate::database_url("rider_restriction_closes_the_socket") else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = sqlx::PgPool::connect(&url).await.expect("connect Postgres");
    apply_all_migrations(&pool).await;

    let bus = infrastructure::EventBus::default();
    let status_bus = actor_client::OperationStatusBus::default();
    spawn_mailbox_workers(&pool, status_bus.clone(), bus.clone());
    let schema = schema_over(&pool, status_bus.clone(), bus.clone(), Roster::Real(pool.clone()));
    let addr = bind_ws_server(schema.clone(), &pool, true).await;

    let rider_id = uuid::Uuid::new_v4();
    let sub = "auth-socket-close-1";
    seed_rider(&pool, rider_id, sub).await;

    let mut ws = connect_and_init(addr, sub).await;

    // SUBSCRIBE TO NOTHING (§9) — no `subscribe` frame is ever sent on this socket.
    restrict_rider_and_wait(&schema, &pool, rider_id).await;

    // POSITIVELY assert the Close frame — never "nothing arrived in N seconds" (beck).
    let frame = expect_close(&mut ws, std::time::Duration::from_secs(10)).await;
    assert_eq!(u16::from(frame.code), shared_types::RIDER_RESTRICTED_SOCKET_CLOSE_CODE);
    assert_eq!(frame.reason.as_str(), shared_types::RIDER_RESTRICTED_SOCKET_CLOSE_REASON);
}

// ─── (2) another rider's fact does NOT close this socket ────────────────────────────────────────

#[tokio::test]
async fn another_riders_restriction_does_not_close_this_socket_and_the_cell_stays_active() {
    let Some(url) = db_test_gate::database_url("rider_restriction_closes_the_socket") else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = sqlx::PgPool::connect(&url).await.expect("connect Postgres");
    apply_all_migrations(&pool).await;

    let bus = infrastructure::EventBus::default();
    let status_bus = actor_client::OperationStatusBus::default();
    spawn_mailbox_workers(&pool, status_bus.clone(), bus.clone());
    let schema = schema_over(&pool, status_bus.clone(), bus.clone(), Roster::Real(pool.clone()));
    let addr = bind_ws_server(schema.clone(), &pool, true).await;

    let watched_id = uuid::Uuid::new_v4();
    let watched_sub = "auth-other-watched-1";
    seed_rider(&pool, watched_id, watched_sub).await;
    let other_id = uuid::Uuid::new_v4();
    let other_sub = "auth-other-restricted-1";
    seed_rider(&pool, other_id, other_sub).await;

    let mut ws = connect_and_init(addr, watched_sub).await;

    // Restrict the OTHER rider, never the watched one.
    restrict_rider_and_wait(&schema, &pool, other_id).await;

    // POSITIVE proof the watched rider's OWN cell stays ACTIVE: `myStanding.standing` echoes the
    // connection's frozen `ReadScope` (unaffected either way), so the discriminating probe is the
    // NON-carved `acceptDelivery` — it must NOT be refused with `RIDER_RESTRICTED` (any OTHER
    // error, e.g. an unknown job, is fine and unrelated to standing).
    let accept = format!(
        r#"mutation {{ acceptDelivery(input: {{ deliveryJobId: "{}" }}) {{ messageId }} }}"#,
        uuid::Uuid::new_v4()
    );
    let data = send_operation_and_await_next(&mut ws, "s1", &accept).await;
    let refused_by_standing = data["errors"]
        .as_array()
        .and_then(|e| e.first())
        .map(|e| e["extensions"]["reason"] == "RIDER_RESTRICTED")
        .unwrap_or(false);
    assert!(
        !refused_by_standing,
        "the watched rider's OWN cell must still read ACTIVE -- got: {data:?}"
    );

    // And no close ever arrives for THIS socket within a bounded wait.
    assert!(
        expect_no_close_within(&mut ws, std::time::Duration::from_secs(2)).await,
        "another rider's fact must never close this socket"
    );
}

// ─── (3) the cell, not the close, is what refuses — a guarded op is refused BEFORE the close ─────
//
// The DETERMINISTIC, race-free proof that mutant M4 ("the standing cell read once at connect
// instead of live") reds lives in `crates/server/src/graphql/acl.rs`'s own
// `standing_guard_cell_tests::the_cell_refuses_before_read_scope_ever_changes` — a real
// `StandingGuard` execution over a frozen `ReadScope` and the SAME `RiderStandingCell` type,
// asserting ADMIT-then-REFUSE with zero network involved. It is race-free BECAUSE it needs no
// race: nothing else can move faster than the test itself. The WS scenario below is its
// end-to-end sibling, over the REAL production transport: empirically, the in-process watcher
// (an in-process broadcast wakeup) reliably wins the race against an actual WS round trip, so
// this scenario accepts EITHER outcome — an explicit RIDER_RESTRICTED refusal, or the socket
// closing before an answer could be sent — as correct, because BOTH are safe (a write that never
// lands is not a write that leaked), and asserts the close itself either way.
#[tokio::test]
async fn a_guarded_op_over_the_real_socket_is_never_admitted_once_the_fact_lands() {
    let Some(url) = db_test_gate::database_url("rider_restriction_closes_the_socket") else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = sqlx::PgPool::connect(&url).await.expect("connect Postgres");
    apply_all_migrations(&pool).await;

    let bus = infrastructure::EventBus::default();
    let status_bus = actor_client::OperationStatusBus::default();
    spawn_mailbox_workers(&pool, status_bus.clone(), bus.clone());
    let schema = schema_over(&pool, status_bus.clone(), bus.clone(), Roster::Real(pool.clone()));
    let addr = bind_ws_server(schema.clone(), &pool, true).await;

    let rider_id = uuid::Uuid::new_v4();
    let sub = "auth-cell-refuses-first-1";
    seed_rider(&pool, rider_id, sub).await;

    let mut ws = connect_and_init(addr, sub).await;

    restrict_rider_and_wait(&schema, &pool, rider_id).await;

    let accept = format!(
        r#"mutation {{ acceptDelivery(input: {{ deliveryJobId: "{}" }}) {{ messageId }} }}"#,
        uuid::Uuid::new_v4()
    );
    match send_operation_or_close(&mut ws, "accept-race", &accept, std::time::Duration::from_secs(10)).await {
        OperationOrClose::Response(data) => {
            let err = data["errors"][0].clone();
            assert_eq!(
                err["extensions"]["reason"], "RIDER_RESTRICTED",
                "an explicit response, once the fact landed, must be the standing refusal: {data:?}"
            );
            // The close still follows.
            let _ = expect_close(&mut ws, std::time::Duration::from_secs(10)).await;
        }
        OperationOrClose::Closed(frame) => {
            // The watcher's close outran this operation's resolution entirely — never admitted,
            // which is the safe outcome the property actually protects.
            assert_eq!(u16::from(frame.code), shared_types::RIDER_RESTRICTED_SOCKET_CLOSE_CODE);
        }
    }
}

// ─── (4) reconnect after the close: carved admitted, acceptDelivery refused ─────────────────────

#[tokio::test]
async fn reconnecting_with_restricted_standing_admits_the_carve_set_and_refuses_accept_delivery() {
    let Some(url) = db_test_gate::database_url("rider_restriction_closes_the_socket") else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = sqlx::PgPool::connect(&url).await.expect("connect Postgres");
    apply_all_migrations(&pool).await;

    let bus = infrastructure::EventBus::default();
    let status_bus = actor_client::OperationStatusBus::default();
    spawn_mailbox_workers(&pool, status_bus.clone(), bus.clone());
    let schema = schema_over(&pool, status_bus.clone(), bus.clone(), Roster::Real(pool.clone()));
    let addr = bind_ws_server(schema.clone(), &pool, true).await;

    let rider_id = uuid::Uuid::new_v4();
    let sub = "auth-reconnect-1";
    seed_rider(&pool, rider_id, sub).await;

    let mut ws = connect_and_init(addr, sub).await;
    restrict_rider_and_wait(&schema, &pool, rider_id).await;
    let _ = expect_close(&mut ws, std::time::Duration::from_secs(10)).await;

    // A FRESH connection, same rider — `connection_init` re-resolves RESTRICTED from Postgres.
    let mut ws2 = connect_and_init(addr, sub).await;

    let data = send_operation_and_await_next(&mut ws2, "carved", r#"{ myStanding { standing } }"#).await;
    assert!(data["errors"].is_null(), "the carved op must be admitted on the new socket: {data:?}");
    assert_eq!(data["data"]["myStanding"]["standing"], "RESTRICTED");

    let accept = format!(
        r#"mutation {{ acceptDelivery(input: {{ deliveryJobId: "{}" }}) {{ messageId }} }}"#,
        uuid::Uuid::new_v4()
    );
    let data = send_operation_and_await_next(&mut ws2, "accept2", &accept).await;
    let err = data["errors"][0].clone();
    assert_eq!(err["extensions"]["reason"], "RIDER_RESTRICTED", "acceptDelivery: {data:?}");
}

// ─── (5) gate OFF: same fact, socket stays open, guard reads ReadScope ──────────────────────────

#[tokio::test]
async fn gate_off_the_socket_stays_open_and_the_guard_reads_read_scope() {
    let Some(url) = db_test_gate::database_url("rider_restriction_closes_the_socket") else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = sqlx::PgPool::connect(&url).await.expect("connect Postgres");
    apply_all_migrations(&pool).await;

    let bus = infrastructure::EventBus::default();
    let status_bus = actor_client::OperationStatusBus::default();
    spawn_mailbox_workers(&pool, status_bus.clone(), bus.clone());
    let schema = schema_over(&pool, status_bus.clone(), bus.clone(), Roster::Real(pool.clone()));
    // The GATE, OFF — the one difference from scenario (1).
    let addr = bind_ws_server(schema.clone(), &pool, false).await;

    let rider_id = uuid::Uuid::new_v4();
    let sub = "auth-gate-off-1";
    seed_rider(&pool, rider_id, sub).await;

    let mut ws = connect_and_init(addr, sub).await;

    restrict_rider_and_wait(&schema, &pool, rider_id).await;

    assert!(
        expect_no_close_within(&mut ws, std::time::Duration::from_secs(2)).await,
        "gate OFF must never close the socket"
    );
    // POSITIVE evidence the socket is still alive and the guard reads `ReadScope` (frozen ACTIVE,
    // never re-derived while the gate is off): the write-side door still admits, because the
    // resolved-at-connect standing never updates on this transport with the gate off.
    let accept = format!(
        r#"mutation {{ acceptDelivery(input: {{ deliveryJobId: "{}" }}) {{ messageId }} }}"#,
        uuid::Uuid::new_v4()
    );
    let data = send_operation_and_await_next(&mut ws, "accept-off", &accept).await;
    let refused_by_standing = data["errors"]
        .as_array()
        .and_then(|e| e.first())
        .map(|e| e["extensions"]["reason"] == "RIDER_RESTRICTED")
        .unwrap_or(false);
    assert!(
        !refused_by_standing,
        "gate OFF: the frozen ReadScope must stay ACTIVE for the socket's life -- got: {data:?}"
    );
}

// ─── (6) Lagged: ACTIVE keeps watching, RESTRICTED closes, a lookup error stays open ────────────

#[tokio::test]
async fn lagged_re_derives_once_active_continues_restricted_closes_lookup_error_stays_open() {
    let Some(url) = db_test_gate::database_url("rider_restriction_closes_the_socket") else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = sqlx::PgPool::connect(&url).await.expect("connect Postgres");
    apply_all_migrations(&pool).await;

    // A TINY bus capacity (1) forces `RecvError::Lagged` under a flood of unrelated envelopes —
    // never a `Lagged` manufactured any other way (ADR-20260810-231300: the once-per-Lagged
    // re-derivation is the declared fallback, never a timer of our own).
    let bus = infrastructure::EventBus::new(1);
    let status_bus = actor_client::OperationStatusBus::default();
    spawn_mailbox_workers(&pool, status_bus.clone(), bus.clone());

    // (6a) ACTIVE: the rider is registered but never restricted -- a flood of OTHER riders' facts
    // must lag this receiver past the capacity without ever closing it.
    let schema_active = schema_over(&pool, status_bus.clone(), bus.clone(), Roster::Real(pool.clone()));
    let addr_active = bind_ws_server(schema_active.clone(), &pool, true).await;
    let active_rider = uuid::Uuid::new_v4();
    let active_sub = "auth-lagged-active-1";
    seed_rider(&pool, active_rider, active_sub).await;
    let mut ws_active = connect_and_init(addr_active, active_sub).await;
    flood_other_riders_restricted(&bus, 8).await;
    assert!(
        expect_no_close_within(&mut ws_active, std::time::Duration::from_secs(2)).await,
        "Lagged + ACTIVE in the read model must keep the socket open (M3's red case is the \
         RESTRICTED sibling below, but a naive fix that ALWAYS closes on Lagged must fail here)"
    );

    // (6b) RESTRICTED: the rider IS restricted in the read model by the time Lagged fires -- the
    // socket must still close even though the FACT itself was skipped. This is the mutant M3
    // catches: "Lagged treated as continue" would leave this socket open forever.
    let schema_restricted = schema_over(&pool, status_bus.clone(), bus.clone(), Roster::Real(pool.clone()));
    let addr_restricted = bind_ws_server(schema_restricted.clone(), &pool, true).await;
    let restricted_rider = uuid::Uuid::new_v4();
    let restricted_sub = "auth-lagged-restricted-1";
    seed_rider(&pool, restricted_rider, restricted_sub).await;
    // Restrict this rider BEFORE the socket even connects — the fact is published (and the read
    // model updated) while NOTHING is subscribed yet, so a broadcast subscriber opened afterward
    // structurally CANNOT see it via the direct match (a broadcast never replays to a late
    // subscriber). The watcher's ONLY route to this restriction is the Lagged re-derivation the
    // flood below forces, which is exactly the mutant M3 needs to be caught: without a genuinely
    // late fact, the watcher's own idle-since-connect head start would let it observe the direct
    // match first (proven empirically — an earlier, connect-then-restrict ordering here made M3
    // pass green by accident, because the direct match closed the socket before Lagged ever fired).
    restrict_rider_and_wait(&schema_restricted, &pool, restricted_rider).await;
    let mut ws_restricted = connect_and_init(addr_restricted, restricted_sub).await;
    // Flood past capacity so the freshly-subscribed watcher's FIRST `recv()` is a Lagged, not an
    // empty wait — capacity 1 guarantees it once more than one envelope is in flight unread.
    flood_other_riders_restricted(&bus, 8).await;
    let frame = expect_close(&mut ws_restricted, std::time::Duration::from_secs(10)).await;
    assert_eq!(u16::from(frame.code), shared_types::RIDER_RESTRICTED_SOCKET_CLOSE_CODE);

    // (6c) a lookup error on re-derivation never terminates (ADR-20260904-124600 §3) -- the
    // harness's stand-in for the lookup returns `Err` unconditionally.
    let schema_err = schema_over(&pool, status_bus.clone(), bus.clone(), Roster::AlwaysErr);
    let addr_err = bind_ws_server(schema_err.clone(), &pool, true).await;
    let err_rider = uuid::Uuid::new_v4();
    let err_sub = "auth-lagged-lookup-failed-1";
    seed_rider(&pool, err_rider, err_sub).await;
    let mut ws_err = connect_and_init(addr_err, err_sub).await;
    flood_other_riders_restricted(&bus, 8).await;
    assert!(
        expect_no_close_within(&mut ws_err, std::time::Duration::from_secs(2)).await,
        "a lookup error on the Lagged re-derivation must never assert a restriction -- the socket \
         must stay open (missed counter asserted separately, not by this DB-gated suite -- see the \
         hand-back note on the OTel global-meter hazard across the other five tests in this file)"
    );
}
