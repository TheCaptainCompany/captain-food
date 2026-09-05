# ADR-20260905-065415 — The restriction fact terminates the rider's socket: a connection-local standing read inside the guard, and one writer to the transport

<!-- Filename: docs/adr/ADR-20260905-065415-the-restriction-fact-terminates-the-rider-s-socket-a-connection-local-standing-read-inside-the-guard-and-one-writer-to-the-transport.md -->

## Status

Accepted — a **team decision by consent** under
[TEAM-DECIDES-OPTION-SPACES](../decisions/TEAM-DECIDES-OPTION-SPACES.yaml): the whole roster was briefed
before any code (full mob — a runtime change on the WebSocket leg, `HOLD: human`), thirteen lenses
answered, twelve favoured the same option and none favoured another; the one split (a release gate or
none) takes the reversible option behind a gate. Realizes **step 5** of
[PROP-20260831-180622](../proposals/PROP-20260831-180622-staff-authentication-the-roster-the-invitation-and-the-door.md)
§7.1 / §11 row 5, which
[ADR-20260830-234532](ADR-20260830-234532-the-second-sitting-publish-france-wide-revocation-is-immediate-and-the-objection-chain-was-decided-22-days-ago.md)
Answer 1 owed as gaps (1) and (3) of *"immediately, on the next request"*. The founder reads this record.

**Relates**: [ADR-20260904-081527](ADR-20260904-081527-rider-standing-is-a-grant-on-the-identity-row-the-doors-are-human-only-and-step-4-lands-in-three-slices.md)
§1/§4 (the grant-shaped standing, `StandingGuard`, the carve set),
[ADR-20260904-124600](ADR-20260904-124600-the-restricted-rider-is-told-on-the-client-leg-first-keyed-on-the-server-s-own-reason-and-the-page-get-leg-rides-with-the-socket.md)
§1/§3 (the rider's notice keyed on the server's own reason; `LookupFailed` never asserts a restriction),
[ADR-20260818-004646](ADR-20260818-004646-no-business-identifier-lives-in-the-identity-provider.md) / #641 (the socket must never
widen what a query would refuse), [ADR-20260810-231300](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md)
(push primary; a poll only as a declared, observable degraded mode),
[ADR-20260817-105844](ADR-20260817-105844-the-walk-goes-first-on-one-database-and-production-stays-suspended.md)
(production suspended; the rider population is zero), DECISIONS §PROP-170500 D5 (multi-instance
fan-out is LISTEN/NOTIFY + reconcile-on-reconnect — not this record's),
[RIDER-RESTRICTION-PRECONDITIONS](../decisions/RIDER-RESTRICTION-PRECONDITIONS.yaml) (open — nothing here
makes a production restriction fire).

## Context

A rider's `ReadScope::Rider { id, standing }` is resolved ONCE in the WebSocket `connection_init`
closure (`crates/server/src/graphql/routes.rs`) and lives in the connection data for the socket's life.
Every guarded operation on that socket — queries and mutations execute over graphql-ws too, not only
subscriptions — passes `StandingGuard` against that frozen copy. A rider restricted at 19:40 with a tab
open therefore keeps ACTIVE standing on every operation, `acceptDelivery` included, until the process
restarts or the tab reconnects (graphql-architect: the exact widening #641 forbids). And a rider who is
idle makes no request at all, so "next request" never comes (Answer 1 gap 3). The in-process
`EventBus` (`tokio::sync::broadcast` of `AppendedEvent { stream_name, event_type, correlation_id,
position }`, published by `PgEventStore::append` after commit) already feeds every subscription; it is
single-process by construction. `tokio-tungstenite` is already in `Cargo.lock` transitively, so the WS
client dev-dependency PROP §7.1 named adds no crate family. The Leptos client
(`crates/web/src/subscriptions.rs`) discards the `CloseEvent` and reconnects on backoff unconditionally.

## Decision

1. **Option C — the fact reaches the socket once, and both legs read it there.** When a RIDER
   connection initialises, the server subscribes a per-connection watcher to the `EventBus` **before**
   resolving the scope (young: a fact appended between resolve and subscribe must not be lost), matching
   `stream_name == RiderState::stream(id)` **and** the `RiderRestricted` event type, both **derived**
   from the generated domain types, never typed as string literals (evans: a literal here is a
   Conformist edge on a naming convention that no gate sees). The match is the connection's OWN rider
   only — a kill fired on another rider's fact is a false statement to a worker (legal, vernon, beck,
   business: equality, never prefix or contains).
2. **The connection-local standing is a `watch::Sender<RiderStanding>` — a cache of the grant, not a
   second grant** (evans; young: monotone-tightening within a connection, write-once toward RESTRICTED;
   a reinstatement never re-opens a live socket — the rider reconnects). No `bool`, no "killed", no
   "revoked" (the word is taken, ADR-081527 §7). `StandingGuard` reads the connection's current
   standing from that cell when one is present and from `ReadScope` otherwise — **one emitted place**
   that closes queries, mutations and subscriptions alike (graphql-architect), and the carve set
   `{ myStanding, delivery, reportDeliveryIssue, handBackDelivery }` keeps admitting a RESTRICTED rider
   exactly as it does over HTTP. That is 7b: per-yield freshness with zero I/O, push-fed.
3. **The socket is then terminated with a readable close** — 7a. `GraphQLWebSocket::new_with_pair`
   takes an `mpsc::Sender<Message>` as its sink; ONE forwarder task owns the real split sink, so there
   is exactly one writer to the transport, ordered (vernon: the mailbox discipline applied to the socket).
   On the fact the watcher pushes `Message::Close(CloseFrame { code: 4403, reason })` into that channel
   and closes it; the forwarder flushes and drops. 4403 is graphql-transport-ws's own Forbidden, which
   async-graphql never emits itself (graphql-architect). The code and reason are named constants in
   `shared_types` beside `RIDER_RESTRICTED` (ux), and the reason carries **no French legal wording**
   (legal). The record's word is *terminate*; the protocol's is *close*; "kill" enters no identifier,
   metric, span, comment or record (evans).
4. **`Lagged` is not benign here.** Existing subscription streams treat a lagged receiver as harmless
   because the next envelope re-resolves current state; a dropped `RiderRestricted` has no next envelope
   and is a permanent grant (young, observability). On `Lagged`/`Closed` the watcher re-derives the
   standing ONCE from the read model (a bounded Ask, vernon/dba): RESTRICTED → terminate; ACTIVE →
   continue watching; a lookup **error never terminates** (ADR-124600 §3: an infrastructure failure
   never asserts a restriction — farley) — it retries with bounded backoff and counts
   `rider_restriction_socket_close_missed_total{reason}` as the declared degraded mode.
5. **The carve set survives through reconnect, and the handback path is out of the termination's
   scope.** Closing the connection ends every stream on it, carved or not; the rider reconnects,
   `connection_init` re-resolves a RESTRICTED scope, and the carved operations (and the unguarded
   `operationStatusChanged`, which carries the handback verdict) work over the new socket; in-flight
   writes fall back to the client's existing poll interval (graphql-architect, ux). A restricted rider
   holding food keeps `delivery` and `handBackDelivery` (business, holub, legal). **Residual, named**:
   a reconnect inside the `Rider` projection's one-tick lag re-resolves ACTIVE for that tick (vernon) —
   bounded, and the new connection's watcher then covers any later fact; recorded, not fixed here.
6. **Behind a gate, default OFF** — `RUN_RIDER_RESTRICTION_SOCKET_CLOSE` (a generated config key, the
   `RUN_EVENT_PUSH` shape), flipped by a separate one-line ADR after a smoke. The split: farley (the gate
   buys deploy ≠ release — choose the flip hour, never Friday 19:40; a key flip and an image revert both
   drop every rider socket, so it buys no cheaper rollback) against holub and business (a gate on a gate:
   the production door is pinned `"false"` and the rider population is zero). ADR-013834: a split takes
   the reversible option behind a gate. The sink refactor (one writer) is structural and ungated; the
   watcher and the close are gated.
7. **Multi-instance is BUS-1's, named here as inherited debt — not this record's.** The `EventBus` is
   in-process (`crates/infrastructure/src/persistence/event_bus.rs` header: "single process … fine for the
   V0 single-instance deployment"); DECISIONS row **BUS-1** (open) already records that `operationStatusChanged`
   on that bus fails all three conditions of ADR-20260810-231300 post-#358, the gateway runtime 501s every
   WS upgrade today (`crates/gateway_runtime/src/lib.rs`), and the subgraph bins build fresh empty buses
   (`crates/server/src/bin_support.rs`). farley and architect asked for the final-vision fact source now —
   the proven cross-process push path, `pg_notify('domain_events')` raised inside the append transaction and
   consumed by `event_wake`'s one LISTEN connection per process. The team's answer: that source IS the final
   form, and it belongs to BUS-1/#385, an open row this slice must not partially realize for one event type
   behind a gateway that serves no socket. So the watcher subscribes to the `EventBus` (option C as
   written), the record names BUS-1 as the precondition of the #358 cutover for this feature, and the
   positive liveness proof is an inverted dead-man `rider_restriction_socket_watch_live` gauge
   (observability: a broadcast cannot be silently deaf; a second instance can). When BUS-1 lands, the
   watcher does not change — the bus does.
8. **The observability contract** — a section of `rider-restriction`: `rider_restriction_socket_close_total{outcome: closed|no_open_socket|missed}`,
   `rider_restriction_socket_close_latency_ms` with t0 = bus publish (not `occurred_at`, cross-host skew
   post-#358), the missed counter above, the watch-live gauge, and a nested INFO event
   `rider.restricted.socket_terminated` carrying both correlation ids (the fact's and the connection's) —
   no `rider_id` label. Every name lands with its constructor in `crates/telemetry` in the same PR
   (round-2 item 6(a), never again). Kill latency is not the business metric — handback completion
   after restriction is (business); nothing here substitutes for 3-ii's gauge.
9. **The test that could not be written, now written first**: `tokio-tungstenite` as a dev-dependency;
   the real routes served on `127.0.0.1:0` (the `rider_sign_in_door.rs` harness); a rider connects,
   `connection_init`s, **subscribes to nothing**, the fact is published, and a **Close frame with code
   4403** is asserted positively within a bounded wait — never "nothing arrived in N seconds" (beck).
   The `EventBus` is per-test and injected, so no order dependence unless a watcher registry is made a
   `static` — it lives on app state (beck). Mutants the checkpoint requires red: watcher not spawned;
   prefix-only stream match (another rider's fact must NOT close); `Lagged` treated as continue; the
   standing read at subscribe only (an in-flight subscription keeps yielding).
10. **The client leg is a NAMED GAP with its own issue, not this slice.** Today the Leptos client
    discards the close code and reconnects; a 4403 is a reconnect storm the rider never sees (ux,
    graphql-architect). Until that lands the rider learns on the next tap (the HTTP refusal → the 4-ii
    `/restricted` bounce), never "pushed"; this record says so rather than implying it. Rider screens
    declare no `subscription:` today, so 7a is rider-invisible and survivable (ux).
11. **What this does not claim.** The close does not satisfy Art. 11(3): a dead socket is a
    notification without reasons; the statement lives on `/restricted` (legal — VERIFY-FIRST, PWD
    2024/2831 transposition pending). #874 stays a blocker. No production restriction can fire
    (RIDER-RESTRICTION-PRECONDITIONS open, door pinned). One counsel question is added to the packet:
    *does abrupt loss of the working channel before the statement of reasons is displayed constitute the
    decision taking effect without its accompanying statement under Art. 11(3)?* No lens output here is
    legal advice or clearance.
12. **Order.** holub advised step 6 first — step 5 is the fifth dark PR of this epic and step 6 (the
    staff door) is the first outcome a human in Tours can experience. The founder chose the order
    3, 4, 5, 6, 7 on 2026-09-04 (answer 5→A); re-ordering it is his, so the advice goes to the decision
    queue with a recommendation and step 5 proceeds as one small PR (business, holub: one PR, 7a+7b,
    never three rounds).

## Alternatives considered

- **A alone (bus-fed close, no connection-local standing)**: the idle socket dies but the frozen
  `ReadScope` keeps admitting every guarded operation until the close lands — and `StandingGuard` on
  queries/mutations over the socket would still read the stale copy. Rejected as incomplete.
- **B (per-yield DB probe of standing)**: falsifiable by the existing suite, but the idle socket stays
  open (gap 3 unmet — the record is not satisfied), one probe per yield per subscriber turns a HubRise
  import burst into a fan-out of probes on the order path's pool at 19:40 (dba), and it is an Ask on a
  push hot path (vernon). Rejected.
- **D (periodic per-connection re-resolution)**: a poll where a push exists. Rejected on
  ADR-20260810-231300; the once-per-`Lagged` re-derivation in §4 is the declared fallback, not a timer.
- **No gate**: rejected by the split rule, not on the merits — see §6.

## Consequences

Positive: gaps (1) and (3) of Answer 1 are discharged for the single-instance runtime; a restricted
rider's socket stops carrying customer data (legal: minimisation applies to NEW pushes only — nothing
retracts what is on screen); one emitted guard reads one fact source for HTTP and WS alike.
Negative: a structural change to every WS connection's sink path (the one-writer forwarder) ships
ungated; multi-instance correctness is deferred to BUS-1/#385 (D5), named as a cutover precondition; the client leg is deferred with an
issue, so until it lands the rider is told on the next tap.

## Follow-up actions

- Issue: the client reads the 4403 close and routes to `/restricted` (the 4-ii bounce), replacing
  unconditional reconnect; the reconnect-inside-lag residual of §5 is reconsidered there.
- BUS-1 / #385 gains one line: the rider-socket termination is a consumer that needs the cross-process
  bus before the #358 cutover.
- The counsel question of §11 appended to the packet on ADR-20260904-152807.
- The flip of `RUN_RIDER_RESTRICTION_SOCKET_CLOSE`: a separate one-line ADR after a smoke.
- Decision queue: holub's ordering advice (step 6 before step 5) for the founder.

## Consulted (ADR-20260812-143619 — one line per lens)

- **vernon** — C; `new_with_pair` + one forwarder owning the sink (one writer, ordered); Tell not Ask; the once-per-Lagged re-derive is a legitimate bounded Ask; name the reconnect-inside-lag residual.
- **graphql-architect** — C amended: the freshness belongs in `StandingGuard` (queries and mutations run over graphql-ws too — the frozen scope is the #641 widening); 4403 is the protocol's own Forbidden; `operationStatusChanged` is unguarded and carries the handback verdict — state the poll fallback; the client discards the close code.
- **young** — C; fire on the appended fact, never the projector drain (a rebuild must not become a business event); subscribe before resolving; `Lagged` fails closed; the flag is monotone-tightening and a single envelope licenses a refusal, never a grant; multi-instance is D5's.
- **beck** — C, the only option whose distinguishing behaviour (the idle socket) is testable; the failing test named first (real transport on `127.0.0.1:0`, Close 4403 asserted positively); the bus is per-test, not global — the registry must never be a `static`; four mutants.
- **farley** — C with the fact source questioned: the in-process bus is silent post-#358 — carry D5 or a named gap; gate YES (deploy ≠ release, the flip hour is chosen); no kill on `Lagged`/probe error (ADR-124600 §3).
- **observability-agent** — C, `Lagged` fail-closed; `_ms` with t0 = bus publish; the missed counter as the declared degraded mode; the inverted dead-man `watch_live` gauge because the silent failure is instance #2; a nested INFO event with both correlation ids; every name with its constructor.
- **dba** — C; nothing new on Postgres (a broadcast receiver per socket, not a `PgListener`, not a pool checkout); B's per-yield probe would put import bursts and money traffic on one pool; card defect banked: the brief conflated `pg_notify` (one LISTEN connection) with the in-process bus.
- **ux-designer** — C with a condition: the client throws the close code away today, so the client leg is a named gap and the record must say the rider learns on next tap; the close code is a `shared_types` constant; 7a is rider-invisible today (no rider `subscription:`); reconnect-inside-lag is a widening to name.
- **legal-specialist** — C; discharges gaps 1 and 3 without changing the promise ("pushed" is narrower than "next request"); minimisation upside on new pushes only; a dead socket is a notification without reasons — the statement lives on `/restricted`; no French legal wording in a close reason; #874 stays a blocker; one new counsel question; not clearance.
- **business-specialist** — C, 7a+7b in ONE PR; zero production impact today (door pinned, population zero), so the split-and-gate case has no value ground; the handback path stays out of the termination's scope; handback completion, not close latency, is the business metric.
- **holub** — C, one small PR, no new gate (a gate on a gate); step 5 is inventory — step 6 should go first (to the founder's decision queue, since he set the order); the close must not be wider than the refusal — the carve set survives.
- **evans** — C; "kill" never enters the tree (terminate / close); the flag is a cache of the grant typed `RiderStanding`, never a bool; "revocation" is taken; stream name and event type derived from the domain types, never literals.
- **architect** — C, but fed by `event_wake` rather than the in-process bus (final-vision-first) — answered in §7: BUS-1 is an open row and the gateway serves no WS today, so C on the `EventBus` with BUS-1 named as inherited debt; no decided record contradicted; step 6 outranks step 5 on production value but §11 sequences 5→6→7 — go, with the card saying the loop closes in production at step 6, not here; card defect banked: the brief conflated the in-transaction `pg_notify` (drains, empty payload) with the post-commit `EventBus.publish` — two mechanisms.
