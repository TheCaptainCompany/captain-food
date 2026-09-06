# ADR-20260906-191836 — The client leg of the restriction close: the pair selects Restricted, an unknown 4403 is Terminal, and the route is the screen's own

<!-- Filename: docs/adr/ADR-20260906-191836-the-client-leg-of-the-restriction-close-the-pair-selects-restricted-an-unknown-4403-is-terminal-and-the-route-is-the-screens-own.md -->

## Status

Accepted — a **team decision by consent** under
[TEAM-DECIDES-OPTION-SPACES](../decisions/TEAM-DECIDES-OPTION-SPACES.yaml) /
[ADR-20260904-013834](ADR-20260904-013834-the-team-decides-option-spaces-and-spec-diffs-external-legal-and-admin-gated-actions-stay-with-the-founder.md).
[#894 "Rider client: read the 4403 restricted WebSocket close and route to /restricted instead of
reconnecting on backoff (step 5's client leg)"](https://github.com/TheCaptainCompany/captain-food/issues/894)
is a Tours-facing rider surface (`HOLD: human`), so the whole roster was briefed before any code. The
checkpoint went to the lenses who declared a concern at the briefing (reviewer, ux-designer,
graphql-architect, vernon, beck), who re-ran the mutants and reproduced the 4404 constant-substitution
proof on the diff; because the chunk's reversibility class calls for the full mob on Tours-facing work,
the coordinator then invited the remaining eight lenses on the diff as well — a **roster-width
defect, banked by the coordinator**. All **thirteen** lenses consent to D1/D2/D3 as coded, with the
corrections landed in [PR #929](https://github.com/TheCaptainCompany/captain-food/pull/929)'s second
commit. Realizes the client leg
[ADR-20260905-065415](ADR-20260905-065415-the-restriction-fact-terminates-the-rider-s-socket-a-connection-local-standing-read-inside-the-guard-and-one-writer-to-the-transport.md)
§10 named as a gap.

## Enforced by

n/a — no behavioral guarantee (this ADR records frontend Rust behaviour with no `specs/rules.yaml`
entry — the client speaks no DSL rule here). The guarantee is pinned by native tests in
`crates/web/src/subscriptions.rs`: `close_disposition_restricted_on_code_and_reason`,
`close_disposition_is_terminal_on_4403_with_another_reason`,
`close_disposition_is_terminal_on_4403_with_empty_reason`,
`close_disposition_reconnects_on_a_non_terminal_code_with_the_restricted_reason`,
`handle_close_routes_and_never_reconnects_on_the_restriction_close`,
`handle_close_holds_at_backoff_when_no_route_is_declared`,
`handle_close_reconnects_on_an_ordinary_drop`, `handle_close_is_terminal_on_an_unknown_4403`, and
`crates/web/src/bounce.rs`'s `restricted_target_equals_the_screens_declared_route` /
`restricted_target_is_none_on_the_sign_in_door`.

## Context

ADR-20260905-065415 §10 named the client leg a gap: "Today the Leptos client discards the close code
and reconnects; a 4403 is a reconnect storm the rider never sees." §5 named a residual: "a reconnect
inside the `Rider` projection's one-tick lag re-resolves ACTIVE for that tick — bounded, and the new
connection's watcher then covers any later fact; recorded, not fixed here."
[ADR-20260904-124600](ADR-20260904-124600-the-restricted-rider-is-told-on-the-client-leg-first-keyed-on-the-server-s-own-reason-and-the-page-get-leg-rides-with-the-socket.md)
§1/§3 already keys the client's HTTP-leg bounce on the server's own `RIDER_RESTRICTED` reason and the
per-screen `restricted_route`, never on a bare code — this record extends the SAME rule to a
socket-originated close.

Emitter uniqueness, grepped and quoted at hand-back: the only `CloseFrame` construction in the whole
server crate is `crates/server/src/graphql/rider_socket.rs:139`, always pairing
`shared_types::RIDER_RESTRICTED_SOCKET_CLOSE_CODE` with `_REASON`; no `4429` literal (async-graphql's
own close code) exists anywhere in `crates/**`. The gate `RUN_RIDER_RESTRICTION_SOCKET_CLOSE`
(`specs/common/configuration.yaml:660`) ships DEFAULT `false`; the restrict door itself
(`RUN_RIDER_RESTRICTION_DOOR`) is also pinned off and the rider population is zero
([ADR-20260817-105844](ADR-20260817-105844-the-walk-goes-first-on-one-database-and-production-stays-suspended.md)),
so no production socket can receive this close today.

## Decision

1. **The disposition reads the PAIR from `shared_types`, never `code` alone.**
   `close_disposition(code, reason) -> CloseDisposition { Reconnect, Restricted, Terminal }`:
   `Restricted` iff `code == RIDER_RESTRICTED_SOCKET_CLOSE_CODE AND reason ==
   RIDER_RESTRICTED_SOCKET_CLOSE_REASON` — the signal is the pair; a bounce on an unrecognised
   signal would invent a refusal the server never sent.
2. **An unknown 4403 (any other reason, the empty string included — a browser delivers an absent
   close reason as `""`) is `Terminal`: no reconnect, no bounce.** `Terminal` is **declared and
   UNOBSERVABLE** — no client-side telemetry exists in `crates/web` at all — and is admissible only
   while UNREACHABLE: the gate is OFF and the one emitter always sends the pair, so no 4403 with a
   different reason can occur in this codebase today. The full 4400–4499 terminal band is the
   designed final step, recorded here as a **NOTE, not filed as an issue** (holub: inventory is not
   the same as a filed defect) — it may not land before client-side close observability exists
   (observability, graphql), and its remaining shape lives in
   [#931 "#929 follow-ups (step 5 client leg): the position-fenced standing resolve in
   connection_init, /restricted re-read on reconnect, client-side close observability before the
   flip, bundle skew, LookupFailed retry amplification"](https://github.com/TheCaptainCompany/captain-food/issues/931).
   Whole-string reason equality makes the close frame a **CLOSED ENUM**: extending the reason is a
   new constant plus a client release, never in place (young). The reason constant is now
   load-bearing on TWO legs at once — the HTTP `extensions.reason` and the close frame — additive-only
   discipline applies to both (graphql).
3. **Composition option A: the native seam stays screen-agnostic.**
   `handle_close(code, reason, attempt, reconnect, restricted, navigate)` composes the disposition
   with a caller's per-screen rule; the wasm `onclose` handler is a three-line adapter over it
   (`e.code()`/`e.reason()`, call the seam); `bounce::restricted_target(screen) ->
   Option<&'static str>` is the ONE seam reading `screen.restricted_route`, called by both the HTTP
   leg (`bounce_after`) and the socket leg, and grows nowhere else; the two `Connection::open`
   callers pass it (`interact.rs`, the screen in hand) or a fixed `Rc::new(|| None)`
   (`handwritten.rs`, customer order tracking, no `Screen` — an explicit no-route posture, never a
   synthesised one).
4. **No declared route is UNCHANGED behaviour: today's reconnect**, the safe posture if a close
   arrives at all — never a new "degraded mode" (that phrase is ADR-20260810-231300's and requires
   an observability contract this arm does not carry). On `/restricted` (declares no
   `restricted_route`, `while_restricted: true`) the reconnect cost is **one indexed projection
   probe per reconnect, capped at 30s** — an UPPER BOUND, since the server does not re-close an
   already-restricted reconnect as a matter of course (it seeds an already-RESTRICTED cell at
   `connection_init` and closes again only on a NEW fact or a `Lagged` re-derivation); reinstatement
   is reload-driven (ADR-20260904-124600 §4), so the reconnect is necessary for a still-restricted
   rider to keep learning of new facts, never sufficient on its own. The bounded re-derivation's 3×
   `LookupFailed` retry amplifies on a saturated pool, watched by
   `rider_restriction_socket_close_missed_total{reason="lookup_failed"}` (dba). The server-side
   remainder of this arm — the position-fenced standing resolve in `connection_init`, and
   `/restricted`'s own re-read on reconnect — is filed on #931, not this PR.
5. **§5's residual is NARROWED, not closed, and only where a route is declared.** Where
   `jobs`/`job_detail` declare `/restricted`, the 4403-then-reconnect instance disappears entirely —
   the rider navigates and the socket that would have re-entered the lag never opens. On the
   `/restricted` screen itself (no declared route), the residual is now reached
   **DETERMINISTICALLY** at `backoff::delay(1)` = 1s on every close, not merely possibly: a
   reconnect inside the Rider projection's one-tick lag is not one stale tick but a **permanent
   ACTIVE grant for that connection** (§4's no-next-envelope argument, unchanged — a dropped
   `RiderRestricted` on that connection's watcher has no next envelope to correct it). "The
   capability does not go stale" is true of THIS client — every mutation on `/restricted`'s carve
   set goes over HTTP, re-guarded by `StandingGuard` on the request's own fresh read — and is
   **NOT** true of the guard surface generally; the remainder is the server's position-fenced
   resolve, filed on #931 as a follow-up, not fixed here.

## Alternatives considered

- **Code-only `Restricted`** (reviewer, at the briefing): bounces on `code == 4403` alone, any
  reason. Rejected: a bounce on an unrecognised signal invents a refusal the server never sent, and
  4403 is the protocol's own terminal code — an unrelated middlebox close on 4403 must not read as
  this platform signal.
- **`Reconnect` on an unknown 4403** (vernon, beck, ux, at the briefing): the conservative choice.
  Rejected: 4403 is `graphql-transport-ws`'s own terminal `Forbidden`, which the reference client
  never retries; reconnecting would be a permanent capped hot loop at Friday peak on any close this
  platform did not intend as a restriction bounce.
- **A hard-coded `/restricted` navigation** (rejected at the coordinator's proposal stage): would
  navigate a screen that declares no restricted route, an invented route no `Screen` ever promised.
- **A dead socket on no route** (rejected): `/restricted` itself holds a socket the server closes
  again only on a later fact; a dead socket there would strand a rider's only push signal on the one
  screen whose entire purpose is to learn when it changes.

## Consequences

### Positive

- The bounce SHORTENS the food-custody clock: `/restricted` carries the hand-back control
  (`delivery`/`handBackDelivery`, the carve set), so a restricted rider with food in hand is told and
  can still act, rather than being stranded on a frozen board (business).
- Terminal's frozen board and the 30s reinstatement lag are bounded, rider-idle costs — zero-order
  exposure while the gate is OFF, both unmeasurable client-side until a reserved web metric exists
  (business, observability).
- The reconnect cycle costs one indexed projection probe per 30s per restricted rider — roughly 300
  probes across a Friday peak for the entire restricted population, no write, no WAL, no growth
  (dba). The only load-coupled term is the bounded re-derivation's 3× retry on a saturated pool,
  watched by `socket_close_missed{lookup_failed}`.

### Negative

- What still stands between this code and a rider actually being told (holub): step 5's client AND
  server halves are now both complete, and both invisible — preconditions (2)–(4) of
  [RIDER-RESTRICTION-PRECONDITIONS](../decisions/RIDER-RESTRICTION-PRECONDITIONS.yaml) are still
  open, two config-key flips are required, production stays suspended, and the rider population is
  zero.
- The release path (farley): `/assets/web.js` is served unhashed with no `Cache-Control`
  (`crates/server/src/lib.rs` ~:1747, `crates/server/src/router.rs` ~:302), so "merged" does not mean
  "riders run it" — a stale bundle against a flipped gate is exactly the hot loop this chunk deletes.
  The pre-flip smoke: prove the served bundle changed (`/health` build version + a fetched `web.js`
  diff), confirm 1006 still reconnects on the new bundle, flip in staging in a real browser (`jobs`
  navigates and never reopens a socket; `/restricted` keeps reconnecting), run the stale-bundle drill
  off-peak, confirm flipping the gate OFF restores reconnect without a redeploy, and never flip first
  at Friday/Saturday 19:00. Client-side close observability precedes the flip.
- Legal (completeness, never clearance): routing is not reasons — ADR-20260905-065415 §11 stands
  **UNAMENDED**; §10 inherits only what the unit tests assert ("the close handler routes to the
  screen's declared restricted route instead of reconnecting; holds at backoff otherwise") — never
  "the rider is now told on the close." New residue: the close fires on the fact itself,
  milliseconds after it lands, so `/restricted`'s one-tick `details_pending` transient
  (`specs/screens/rider.yaml` ~:512) is now structurally reachable in a way it was not while the
  client just reconnected blindly — a notice rendered without its ground or dates for that one tick;
  the contest path is intact regardless. [#874](https://github.com/TheCaptainCompany/captain-food/issues/874)
  stays a blocker. One counsel question is appended to the packet on ADR-20260904-152807, quoted
  verbatim: *"where the platform's own client navigates the worker to the statement screen at the
  instant of the decision, and that screen can transiently display the restriction without its
  ground or dates, has the statement been provided under Art. 11(3) at that moment, or only when the
  ground and effectiveAt render? And is a client-side navigation a durable medium at all, or does the
  durable-medium duty fall entirely on #874's SMS?"*

### Follow-up actions

- [#931 "#929 follow-ups (step 5 client leg): the position-fenced standing resolve in
  connection_init, /restricted re-read on reconnect, client-side close observability before the
  flip, bundle skew, LookupFailed retry amplification"](https://github.com/TheCaptainCompany/captain-food/issues/931)
  carries: the server's position-fenced standing resolve in `connection_init` (§5's remainder);
  `/restricted`'s own re-read on reconnect; client-side close observability, required before any
  flip; the bundle-skew smoke procedure above; the `LookupFailed` retry-amplification watch.
- The full 4400–4499 terminal band stays a NOTE in this record (§Decision 2), not filed as
  inventory — client-side close observability is its precondition.
- The counsel question above appended to the packet on ADR-20260904-152807.

## Consulted (ADR-20260812-143619 — one line per lens)

- **reviewer** — code-only `Restricted` recorded as the alternative not taken; the no-route posture
  must be pure and tested (T5); the permanent `/restricted` hold loop is invisible to §8's counters —
  named here.
- **ux-designer** — a socket close may not bounce a screen that declares no restricted route; the
  `/restricted` hold is necessary, not sufficient — reinstatement is reload-driven
  (ADR-20260904-124600 §4); the rider sees: reconnect stops, hard nav, title + no-more-jobs first,
  then hand-back, then contact.
- **graphql-architect** — 4403 is `graphql-transport-ws`'s terminal `Forbidden`; an unknown 4403 is
  `Terminal`, scoped to 4403; never a synthesised `TransportError` into `bounce_target`; the reason
  constant is load-bearing on two legs, additive-only.
- **vernon** — one reconnect driver per socket; every terminal state declared; the §5 residual
  narrows and its remainder is the server's position-fenced resolve.
- **beck** — the disposition is the fold and the seam is the product: T4–T6 plus T3c pin
  `handle_close`; the literal 4403 is pinned by no test by design (both legs read one constant).
- **farley** — consented; the flip is gated on bundle rollout, not on merge — `Terminal` on an
  unrecognised 4403 is silent by design, so client-side close observability precedes the flip.
- **holub-focus** — the slice is minimal and the `Terminal` arm earns its three lines; the
  4400–4499 band is deferred as a note, not filed as inventory, and the ADR states what still stands
  between this code and a rider being told.
- **business** — the bounce shortens the food-custody clock rather than stranding held food
  (`/restricted` carries the hand-back control); `Terminal`'s frozen board and the 30s reinstatement
  lag are bounded rider-idle costs, zero-order exposure while the gate is OFF, and both are
  unmeasurable client-side until the reserved web metric exists.
- **dba-architect** — the reconnect cycle costs one indexed projection probe per 30s per restricted
  rider — 300 across the Friday peak, no write, no WAL, no growth; the only load-coupled term is the
  bounded re-derivation's 3x retry on a saturated pool, watched by
  `socket_close_missed{lookup_failed}`.
- **evans** — the pair (code AND reason) is the published signal and the enum keeps it out of the
  browser arm; "degraded mode" is struck from the no-route arm (ADR-20260810-231300 owns it and the
  arm is unchanged reconnect, not a new mode); "Stop" is the tree's "terminate" (`actor_client`) and
  collided with "Stop one subscription" in its own file — renamed.
- **observability-agent** — a degraded mode nobody can query is an outage with better manners —
  `Terminal` and the no-route hold are declared, unobservable, and admissible only while
  unreachable.
- **legal-specialist** — consent; routing is not reasons, so ADR-20260905-065415 §11 stands
  unamended and §10 inherits only the unit-asserted "routes to the screen's declared restricted
  route, never reconnects"; the close fires on the fact, so `/restricted`'s one-tick
  `details_pending` transient is now structurally reachable — a notice without its ground or dates,
  contest path intact (Art. 11(2)/11(3) Dir. 2024/2831, VERIFY-FIRST); #874 stays a blocker; not
  clearance.
- **young** — consent; the narrowing holds only where a route is declared, and on the no-route
  screens the reconnect-inside-lag residual is now reached deterministically at 1s; a reconnect
  inside the lag is not one stale tick but a permanent ACTIVE grant on that connection (§4's
  no-next-envelope argument, unchanged); the close reason is matched whole-string, so the frame is a
  closed enum and any new reason reads as `Terminal`.
