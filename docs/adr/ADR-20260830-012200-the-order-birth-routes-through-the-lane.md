# ADR-20260830-012200 — The Order birth routes through the lane: `ROUTE_ORDER_BIRTH_THROUGH_LANE` defaults ON

- **Status**: Accepted
- **Date**: 2026-08-30 (founder answer 2026-08-29)
- **Decider**: the **FOUNDER / Tech CEO**, answer sheet round 4 (2026-08-29), verbatim:

  > **"LANE-FLIP (ROUTE_ORDER_BIRTH_THROUGH_LANE): A — Flip it ON"**

- **Satisfies**: dispatch card
  [`docs/dispatch/598-birth-lane-flip-observability.md`](../dispatch/598-birth-lane-flip-observability.md)
  §7's standing founder-gated flip obligation (the "separate one-line ADR" it names), carrying the
  §9 evidence items 1–5 below. Chunk **C1** of
  [ADR-20260829-230418](ADR-20260829-230418-aggregates-own-the-facts-isolation-first.md) is hereby
  complete: C1a produced the evidence and never flipped; this record is the flip.
- **Relates**: [ADR-20260816-040239](ADR-20260816-040239-deliver-is-a-lane-enqueue-not-a-foreign-stream-append.md)
  (the semantic ruling) ·
  [#758 "C1a: pre-flip evidence for ROUTE_ORDER_BIRTH_THROUGH_LANE — birth-lag seen recorded, the paid-then-null tracking guard, split flip preconditions"](https://github.com/TheCaptainCompany/captain-food/issues/758)
  / PR [#761](https://github.com/TheCaptainCompany/captain-food/pull/761), squash-merged as
  `2408fc73` after the team reviewer PASS ·
  [ADR-20260817-105844](ADR-20260817-105844-the-walk-goes-first-on-one-database-and-production-stays-suspended.md)
  (walk evidence is a READING, never a certificate).
- **Consulted** (ADR-20260812-143619): banked, no new fan-out — this is the founder's approval of
  the team-recommended option. The 13-lens block of
  [ADR-20260829-230418 §Consulted](ADR-20260829-230418-aggregates-own-the-facts-isolation-first.md)
  covers this plan chunk; the pre-code mob verdicts and the independent reviewer pass on PR #761
  (lens verdicts in dispatch card 598 §Findings; reviewer PASS comment on #761, 2026-08-29) are the
  per-diff consults.

## The decision

`specs/ordering/configuration.yaml` `ROUTE_ORDER_BIRTH_THROUGH_LANE` **`default: false` → `true`**,
nothing else in the key. `PlaceOrderProcess`'s `deliver: OrderPlaced to: Order` step now stages a
lane ENQUEUE by default; the Order's own lane worker appends the birth, and the acceptance deadline
is keyed on THAT delivery's `Recorded` verdict. Scoped, per the key's own text, to the
Order/`OrderPlaced` pair alone — the other twelve `deliver:` steps keep the legacy append until
each is moved by its own record (ADR-20260829-230418 C3).

**Explicitly NOT taken: the `ENFORCE_ACCEPTANCE_TIMEOUT` flip.** Precondition (5b) still waits on
production time-to-accept distributions (p50/p90/p99 by daypart and restaurant), which no walk can
produce — the TTL is chosen from how long real restaurants take, and that number only exists once
real orders flow. The acceptance clock now ARMS (this flip is precondition (5a)); it does not yet
cancel.

## Evidence (dispatch card 598 §9, items 1–5)

1. **The smoke — walk stack, not production** (a reading of the deliberately suspended production,
   ADR-20260817-105844; production has no traffic, so "deployed monolith" cannot be exercised —
   the walk suites are the executable walk available today, stated in #761's body). PR #761,
   merged `2408fc73`: (i) `OrderPlaced` lands **exactly once** on the stream, the birth row
   observed on `inbound_messages ('Order','OrderPlaced')`; (ii)
   `order_birth_lag_ms{routed="true"}` recorded — **18 ms** on the executor's run, **21 ms** on
   the reviewer's independent re-proof against a fresh Postgres; (iii) the acceptance-deadline row
   exists and is caused by THAT birth delivery (`pm_prepare_delivery` flag-ON legs); (iv)
   `runtime_flag_state` is asserted from the real standalone composition root with resolved
   values re-asserted on every export cycle (`mailbox_liveness_metrics`), the sole input to the
   `count(distinct value) by (flag) > 1` parity query. **Re-run at the flipped default in this
   change with NO env override** — the default itself now carries the route; the observed numbers
   are in the flip commit and the journal entry.
2. **Rollback, and its observer.** Rollback is a **config flip back OFF, not a redeploy**: the next
   delivery appends the legacy way. An order born through the lane before a rollback is **not
   affected** — its birth already landed (reversible in code, not in state; those rows stay,
   written here rather than assumed, per card 598 §7.3). The trigger's observer: while production
   stays suspended there is no customer traffic to miss, and the observer is the **operating
   session** reading the liveness pair (`order_lane_watch_heartbeat_total` stopping /
   `order_lane_oldest_pending_age_ms` climbing) and the birth-gap gauge
   `payment_authorized_no_order_birth_age_seconds`. **Before first real traffic, the
   founder-gated Honeycomb alert-route wiring (`specs/observability.yaml` alerting gap) must give
   that pair a route** — recorded here as the standing condition so the trigger never reads as
   covered while unobserved.
3. **The split-clock evidence, in vernon's words** (card 598 §9.3). Double-birth is unreachable —
   four absorbers: both routes converge on `Order-{id}` with an expected-version precondition at 0,
   the door row's primary key dedups, the trigger skips on redelivery. What a split fleet DOES
   produce is **one birth and a coin-flip on the acceptance deadline, per order, invisibly** —
   invisible because `order_birth_lag_ms` records only on the routed path; bounded by the
   rolling-deploy window and observable through `runtime_flag_state`. **Fleet-parity posture: one
   value across the monolith and any standalone worker fleet.** This change flips the generated
   `Config` fallback AND the standalone composition root's env fallback
   (`crates/infrastructure/src/mailbox/standalone.rs`) in the same commit, so both roots default ON
   and "same default as the spec" stays true in both places.
4. **The re-baseline trigger**: the first Friday/Saturday 19:00–21:30 service after this flip runs
   in production. Until then `place-order`'s 800 ms p95 reads LOOSE **by decision**, recorded at
   the line in `specs/observability.yaml`.
5. **The liveness series were non-silent BEFORE the flip was armed**: the heartbeat/age pair emits
   on every tick for every declared routed lane **including while the flag was OFF**, pinned by the
   mutation-provable suite (`mailbox_liveness_metrics` — a once-at-startup seeder and a
   skip-when-empty watcher both go red).

**The never-born terminal state (the reviewer's packet requirement, #761 PASS).** If a routed birth
NEVER lands, the paid customer's terminal state is the reassurance, not an error: *"exhaustion
keeps the reassurance on screen rather than ever degrading to the not-found hero"*
(`crates/web/src/tracking.rs` — the render keys on `birth_pending`, and the `orderStatusChanged`
subscription remains the push path). A paid customer never sees "Commande introuvable" in the
handoff window or after it; the never-born case is an **ops alarm**
(`payment_authorized_no_order_birth_age_seconds`, the #608 detector built for exactly this class),
never a customer-facing verdict. On the write side the timeout-cancel handler's `NoOrder` arm
already treats an empty stream ("erased, or never born") as a terminal no-op — nothing left to
cancel.

## Consequences

- The default flip is a Tier 0 spec change riding this record (young, ADR-20260829-230418
  §Consulted: replay-safe, NOT a migration — payload, type and stream unchanged; the birth's
  envelope change is recorded and stored rows are never backfilled).
- `ENFORCE_ACCEPTANCE_TIMEOUT` precondition (5a) is satisfied; (5b) remains open on production
  distributions. The erasure BUILD is unblocked per ADR-20260829-230418's sequencing (C1 proved +
  the founder's PROP-20260829-150752 approval).
- The `graphql_write_path` suite's dep pin moves to the new default in the same change, so the
  suite keeps testing reality (its pre-flip OFF pin was recorded in #761's body).
