# Dispatch card — [#608 "Nothing detects an authorized payment with no order birth"](https://github.com/TheCaptainCompany/captain-food/issues/608)

**Read at**: `89df1d8bd75bad35c7acd9eb7d78d6575be7821e` (tip of `596-chain-lane-width-declared`,
merged to `main` as `2a035ff`, which is not yet fetched locally). Every `file:line` below is that
snapshot; re-stamp after `git pull origin main` (young's snapshot rule).
**Artifact class**: dispatch card (ADR-20260816-020752).
**Position**: top of the post-#596 queue. Ranked above the compiler-first children
([#609](https://github.com/TheCaptainCompany/captain-food/issues/609),
[#590](https://github.com/TheCaptainCompany/captain-food/issues/590),
[#601](https://github.com/TheCaptainCompany/captain-food/issues/601)) under
[docs/BACKLOG.md](../BACKLOG.md) / ADR-20260720-213024 (non-functional and cross-functional before
features): those close one known route each to the worst failure mode, this one detects the outcome
whatever the cause — including the causes not yet found, and including the ones the
`ROUTE_ORDER_BIRTH_THROUGH_LANE` flip will add.

## 1. Chunk / not in scope

**Chunk**: make "money is held and no order exists" a signal the system emits, not a fact someone
notices. Two parts, one worker:

1. **The population fix.** `specs/observability.yaml:508-514` declares
   `payment_authorized_unsettled_age_seconds` as *the* dead-man's switch for a never-attempted
   capture, computed "over the OrderTracking read model". An authorization whose order was **never
   born** has no OrderTracking row, so that gauge is structurally blind to exactly the case #608
   names. Declare the birth-gap population as its own contract with its own age gauge, and amend the
   existing header to say what it does **not** cover.
2. **The emitter.** `payment_authorized_unsettled_age_seconds` has **zero hits in `crates/**`** —
   grep-verified at this SHA; it is a declared signal with no runtime, and #608 would make two of
   them. Final-vision first (ADR-20260808-235113): one timer-driven reconciling sweep emits **both**
   gauges. Time-triggered work, so the "sleep until the next due row" pattern, explicitly outside the
   no-polling rule's scope (ADR-20260810-231300).

**Not in scope**: flipping `ROUTE_ORDER_BIRTH_THROUGH_LANE` (founder-gated: deployed non-prod smoke
plus a named observer); Honeycomb alert-route wiring (contract only, same posture as #598);
remediation of a detected orphan (detect first, decide the repair separately); any `domain_events`
shape change; #609/#590/#595/#601.

## 2. Why this and not the cheap children

- **It is the only candidate that protects against unknown causes.** #609 closes a spellable
  two-step, #590 a verdict-blind re-application, #595 an unlaned birth, #601 an unlaned deliver.
  Each is a known route to "authorized, never born". #608 is the detector for the class.
- **The flip widens exposure.** Routing birth through the lane adds a durable hop between the
  authorization and the birth. Adding a hop before the detector exists is the wrong order; and
  declaring the signal is half of the observer the founder gate asks for.
- **Observability before the bug it observes.** The other four are all verifiable *by* this signal
  once it exists; none of them make this one verifiable.
- **The discharge was scoped, not general.** `docs/STATUS.md:49-51` discharges #608 **for #596 only**,
  on the ground that production has one restaurant, no real customer orders, and Stripe in TEST
  mode. That is a statement about today's traffic, not about the system. It expires at the first real
  order, and nothing in the repo detects that moment either.

## 3. Paths

| Path | Why |
|---|---|
| `specs/observability.yaml:495-514` | the existing dead-man's-switch header — the population claim to amend |
| `specs/observability.yaml:563-567` | `payment_authorized_unsettled_age_seconds`, gauge, no emitter |
| `specs/observability.yaml:340-350` | where `authorized` is recorded (webhook path) — the source population |
| `crates/infrastructure/src/mailbox/promotion_watch.rs` | the timer-driven watcher precedent; the sweep is its sibling, not a new shape |
| `crates/infrastructure/tests/orders_placed_metric.rs` | the spy meter provider precedent — read its head comment before asserting an emitted series |
| `crates/telemetry/src/meters.rs` · `crates/telemetry/src/contract.rs` | where the gauge registration lands |
| `specs/stories.yaml` · `specs/tests.yaml` | ADR-0032 completeness for anything new on the write side |
| `docs/SPEC-LOG.md` · `docs/STATUS.md` · `docs/adr/` | a landed spec change writes one SPEC-LOG sentence in the SAME commit |

## 4. Reversibility class

**Class: HIGH-CONSEQUENCE — money-path detection + a new long-running worker.** No event shape
changes and no funds move, but the artifact is the thing that will or will not tell a human that a
customer's money is held with nothing behind it; a wrong threshold or a blind population is a silent
non-detection, which is the failure mode being fixed. The `HOLD: human` axis wins over "the diff is
small".

**Briefing roster: WHOLE ROSTER** (ADR-20260816-134352 — money movement and Tours-facing).
Named lenses whose concern is anticipated: `young` (is this a fold over `domain_events` per
ADR-20260811-014129, or operational telemetry that must work when Postgres is down? the gauge's
source *is* Postgres — say which side of the wall it sits on and why), `vernon` (a reconciling sweep
is a process manager with its own process state, not a cron), `holub` + `beck` (the mutant that
proves the signal: delete the emit and something must go red), `farley` (no flag — the OFF state is
"no detection", which is today), `dba` (the scan's index and its cost at peak).

**Merge posture: `HOLD: human`** (ADR-20260815-115220) — money path, new worker runtime. Ready-for-
review, then the team's independent reviewer pass, then the coordinator merges on green.

## 5. Checkpoint verification

**Checkpoint goes to the concern-declared subset only; any lens may opt back in.** At the checkpoint
the executor BANKS, explicitly, whether the narrow set missed anything the full roster would have
caught. **A MISS reverts this reversibility class to the whole roster for subsequent chunks; an
unanswered banking question is a RUN DEFECT, not a silent pass** (ADR-20260816-134352).

## 6. Done when

- A birth-gap detection contract exists in `specs/observability.yaml` with a declared question, a
  bounded grouping population, and a threshold justified against the ~7-day Stripe hold expiry.
- The existing `payment_authorized_unsettled_age_seconds` header no longer claims coverage it does
  not have.
- One sweep runtime emits **both** gauges, with a test that asserts an emitted series and a named
  mutant that reds it.
- ADR-0032 completeness satisfied for anything added on the write side.
- `make rust` green, `make validate` 0 errors, warning baseline unchanged or refreshed in the SAME
  commit with a stated reason, `check-drift` clean.
- One `docs/SPEC-LOG.md` sentence and a `docs/STATUS.md` entry, in the same commit.

## 7. Risk

**Scope creep into remediation.** The moment the sweep can see an orphaned authorization, someone
will want it to void the hold. That is a money-moving decision with its own option space and it is
not in this chunk. Second risk: the sweep is written as a fixed-interval poll instead of
sleep-until-next-due, which would be a fresh violation of ADR-20260810-231300 in the very chunk that
cites it.

## Findings

**Briefing verdicts (five lenses, all PASS), and the one real divergence.** `vernon` refused §1's
anti-join of Payment authorizations against Order births — two aggregates, two write paths, two
clocks, and another consumer's projection becoming an input to the money path. He and
`observability` then independently verified the fact that resolved it, which this card did not have:
**the `payment_process_manager` row is created at CHECKOUT** (`crates/application/src/commands.rs`
— `PlaceOrder` appends `PaymentIntentCreated` then opens the run at `AWAITING_PAYMENT_RESULT`; the
PM legs only `expect` an existing row). An authorization the saga never processed therefore still
has a run row, so the saga's own state carries the main population single-owner and single-write-
path. **§1's source is superseded**; recorded in
[ADR-20260816-213000](../adr/ADR-20260816-213000-the-birth-gap-detector-reads-the-saga-run-not-an-anti-join.md).

**What landed against this card, delta only:**

- Source per above. **ONE gauge**, `payment_authorized_no_order_birth_age_seconds{reason}`, bounded
  set `{retry_pending, delivery_exhausted, no_run}` — `no_run` is the two-unfenced-writes residue as
  a third REASON, never a second gauge. Zeros for every member every tick, plus
  `payment_birth_gap_sweep_heartbeat_total` after a COMPLETE sweep.
- **Contract placement moved**: the gauges live on the EXISTING `place-order` contract, not a new
  one. The birth gap is a `place-order` failure, `place-order` already hosts the sibling lane
  dead-man's switches, and a sweep has no correlation/trace identity to satisfy a fresh contract's
  mandatory `run_identity` + `spans` honestly. The `payment-settlement` header amendment is
  unchanged from §6.
- **§4's threshold antecedent was arithmetically wrong in the dispatch** (see Checkpoint
  verification below).
- **Response routing is IN**, overriding §1's "not in scope":
  `docs/runbooks/authorized-payment-no-order-birth.md`, linked from the contract, with the
  no-alert-route gap recorded and cross-linked to the flip's founder-gated observer obligation.
- **New gate**: `obs-metric-no-emitter` (validator §20). WARNING on the §17 ratchet, not an error —
  **41** declared metrics fail it today (whole contracts written before their runtimes), and 41
  errors on `main` would only be resolvable by weakening the rule. The ratchet gives beck's actual
  requirement: the 41 are frozen and a 42nd is a hard failure.
- **§7's second risk did not materialise**: the sweep is a timer-driven monitor on the
  ADR-20260810-231300 carve-out (the `promotion_watch` / `order_lane_watch` shape), not a poll of a
  push path. §7's first risk was live and is held: remediation stays manual, in prose.

**Checkpoint verification:** *did the narrow set miss anything the full roster would have caught?*
**One MISS, from `dba` — a lens the whole roster carries and the concern-declared subset did not
return to.** §4's briefing named `dba` for "the scan's index and its cost at peak", and the cost
question is answered (three grouped statements per tick, indexed on
`inbound_messages.status`/`actor_type` and `domain_events.event_type`, V0 population single digits).
But the arithmetic in the resolved dispatch is **wrong**: it states the saga's bound as
`max_delivery_attempts × retry_spacing_seconds` ≈ **50 s**, while the mailbox backoff is
**EXPONENTIAL** — `base · 2^(N−1)`, i.e. `10+20+40+80+160` = **310 s**, which
`MAILBOX_MAX_DELIVERY_ATTEMPTS`'s own `gates` prose already spells out as *"~5 min to terminal at cap
5"* (`crates/actor_runtime/src/worker.rs` `poison_raw`). A 50 s threshold pages on **every healthy
retry** on a lane that is working exactly as designed — the precise "threshold that lies" class the
chunk exists to remove. Landed as **600 s** (310 s + one schedule of slack), with both antecedent
keys named by `$ref`. **Per ADR-20260816-134352 this MISS reverts the HIGH-CONSEQUENCE class to the
whole roster for subsequent chunks** (sub-obligation MOB-COST-1a).

Second, smaller, banked rather than silently absorbed: `delivery_exhausted` is asserted
present-at-zero and mutant-covered but is **not driven to a positive value** by the test. Every
honest route to "terminal hop while the run stays `AWAITING_PAYMENT_RESULT`" in today's runtime is
an induced infrastructure fault rather than a seam, and manufacturing it would mean inserting the
row the detector queries — beck's one prohibition. Stated in the test's module docs, not papered
over.
