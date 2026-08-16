# Dispatch card — [#598 "Before the birth-lane flip: the place-order latency budget still measures the old workflow, and a flat order_birth_lag_ms cannot be told from a dead lane"](https://github.com/TheCaptainCompany/captain-food/issues/598)

**Read at**: `origin/main` = `c67994706df58e8ee14f0518c91a85f811a71d5d`. Every `file:line` below is
that snapshot; re-stamp if it moves (young's snapshot rule).
**Artifact class**: dispatch card (ADR-20260816-020752) — second use.
**Position**: the LAST flip-blocker for `ROUTE_ORDER_BIRTH_THROUGH_LANE`. #597 merged as `0119085`;
#588 (PR #594) merged as `693dab3`. The flip is what makes #167's acceptance clock actually run.

## 1. Chunk / not in scope

**Chunk**: make the routed-birth handover *observable before it is switched on* — re-decide the
`place-order` latency budget and success rule now that the workflow they measure has changed, give
the Order lane a **dead-man's switch** that says "alive, nothing routed" instead of emitting nothing,
and make **both** liveness signals (this new one and #586's promotion watch, [#589](https://github.com/TheCaptainCompany/captain-food/issues/589)) provable by a mutation that silences them.

**Not in scope**: flipping `ROUTE_ORDER_BIRTH_THROUGH_LANE` (a separate one-line ADR follows, §7);
flipping `ENFORCE_ACCEPTANCE_TIMEOUT`; any change to birth routing semantics or to `domain_events`;
alert-route wiring in Honeycomb (contract only, no route shipped by this chunk).

## 2. Ruling — #589 folds IN. One chunk.

**One chunk, with #589 sequenced FIRST.** The reasoning, in the order that decides it:

1. **The deliverable is the harness, not the signal.** #589's defect is that
   `promotion_watch_tick` has exactly one test —
   `crates/infrastructure/tests/main/mailbox_acceptance_timeout.rs:343-367` — and it ends at
   `.expect("one watch tick over the real schema")`. It never inspects an emitted series, so
   deleting the zero-seeding at `promotion_watch.rs:44-47` reds nothing. Verified at this SHA.
   #598's new signal needs the *same* missing thing: a spy meter provider that can assert a
   histogram/gauge was emitted with a given value. Build it once.
2. **Building a new liveness signal against an unproven mechanism repeats the mistake one layer
   over** — which is the card's own premise. Doing #589 first means the harness is red-proved on an
   **existing** signal with an **already-identified** mutant (delete lines 44-47), before it is
   trusted to guard a signal nobody has written yet.
3. **Same files, same module, same review lenses.** Both watchers live in
   `crates/infrastructure/src/mailbox/`; splitting produces two PRs touching one module and one test
   binary — the "never dispatch two items touching the same files" rule bites.
4. **Cost of folding is near zero; cost of splitting is a second full mob.**

Fence on the ruling: if phase 1 discovers the spy provider needs work beyond the
`crates/infrastructure/tests/orders_placed_metric.rs` precedent (a single-binary,
install-provider-before-first-meter-call pattern, documented in that file's head comment), that work
is **still** #598's, so no split rescues the critical path. Do not re-open the question mid-chunk.

## 3. Paths

| Path | Why |
|---|---|
| `specs/observability.yaml:247` | `latency_budget: { max_p95_ms: 800, max_p99_ms: 1500 }` on `place-order` — the re-baseline decision |
| `specs/observability.yaml:~244` | `status_rules.success.required_spans` — `event.store.append` was removed unconditionally (§4b) |
| `crates/infrastructure/src/mailbox/promotion_watch.rs:26-71` | #589: the untested zero-seeding (`44-47`) and the emit loop (`62-69`) |
| `crates/infrastructure/tests/main/mailbox_acceptance_timeout.rs:343-367` | the assertion-free test #589 names |
| `crates/infrastructure/tests/orders_placed_metric.rs` | **the precedent**: standalone binary + spy provider bound before the first meter call; read its head comment before writing a second one |
| `crates/infrastructure/src/mailbox/flush.rs:68-90` | `record_order_birth_lag` — emits only when the delivery appended the birth, i.e. never while the flag is OFF |
| `crates/infrastructure/src/mailbox/handler.rs:440` | the sole call site |
| `crates/telemetry/src/meters.rs:184` · `crates/telemetry/src/contract.rs:126` | `ORDER_BIRTH_LAG_MS` + the meter fn; the new liveness series goes beside them |
| `crates/infrastructure/src/mailbox/mod.rs:24-25` | module exports (both watchers) |
| `tools/codegen-rs/src/emit/pm_orchestrators.rs:1112` | LOW-1: `source: "pm:{}:{}"` — the FROZEN door identity, no golden |
| `crates/infrastructure/src/mailbox/standalone.rs:130-145` | LOW-2: `std::env::var` fleet-parity gap (`ENFORCE_SERVICE_HOURS_GUARD`, `ENFORCE_ACCEPTANCE_TIMEOUT`, `ROUTE_ORDER_BIRTH_THROUGH_LANE`) |
| `docs/SPEC-LOG.md` · `docs/STATUS.md` · `docs/adr/` | records; a spec change writes one SPEC-LOG sentence in the SAME commit |

## 4. The substance (third look on PR #594)

### a. The latency budget measures a shorter workflow — decide, do not omit

`specs/observability.yaml:247` still reads `max_p95_ms: 800`. After the flip, the birth append
happens in a **different delivery**, so the `place-order` workflow the budget scores is strictly
shorter. Keeping 800 is defensible; keeping it *silently* is not. The chunk must land one of:

- **(A) Re-baseline to the routed workflow** (e.g. tighten p95, and open a companion budget on the
  Order lane delivery). Pro: the number keeps meaning "checkout felt fast". Con: no production
  distribution exists yet to baseline from — this is a guess wearing a number.
- **(B) State explicitly UNCHANGED, with the reason, in a comment beside the key** — the budget is a
  *customer-perceived* checkout ceiling, and the customer's perception did not change; the handover
  is measured separately by `order_birth_lag_ms`, and the two together cover what the single number
  used to. Pro: honest, needs no invented distribution, and names where the removed time went.
  Con: the p95 will drop after the flip and the budget will read loose until re-baselined on real
  data. **RECOMMENDED**, with a note that re-baselining is an evidence question (ADR-20260808-144738)
  answerable only after the flip has run at peak.

Either way the outcome is a recorded decision, in the spec, at the line.

### b. The success rule was loosened TODAY, in the OFF state

`event.store.append` left `required_spans` **unconditionally** — there is no `routed` predicate on
the rule. While the flag is OFF the birth still appends inline, so a `place-order` execution whose
append never happened is now scored **success**. That is a live loosening, not a post-flip one.
Decide it in the same breath as (a): either accept it (with the reason written at the line) or
restore the requirement behind the same gate the routing uses.

### c. A histogram with zero points cannot be told from a dead lane

`order_birth_lag_ms{routed}` records nothing while the flag is OFF **by design**
(`flush.rs:74-88` gates on `staged_contains_order_placed`). Post-flip, a silent series means either
"flag off" or "the Order lane worker is dead" — precisely the ADR-20260810-231300 defect class: *a
monitoring path that can only fire when a signal ARRIVES goes quiet exactly when it should scream.*

A **liveness signal is owed before the flip**. The local precedent is `promotion_watch.rs`: a
monitor on its own clock, **outside** the worker it watches (the monitoring carve-out: a poll,
permanently, with no exit), emitting on EVERY tick for every DECLARED lane, **zero included**. The
Order-lane analogue emits the routed-birth backlog/lag per lane on every tick, so the series exists
whether or not a birth was routed, and its *stopping* is the alarm. The new series and its
`alertable:` posture go in `specs/observability.yaml` in the same change (a contract, not a
call site).

### d. LOW-1 — the FROZEN door identity has no pinned golden

`tools/codegen-rs/src/emit/pm_orchestrators.rs:1112` emits
`source: "pm:{pm_name}:{event}"`, and the comment above it calls the identity FROZEN because
changing either half "re-mints the identity of every in-flight message". Nothing pins the produced
string: renaming a PM in `specs/ordering/processmanager.yaml` would silently re-mint every in-flight
birth id, gates all green. **Fold in** — a codegen test asserting the literal
`pm:<name>:OrderPlaced` for the shipped route is a few lines and converts a prose FROZEN into an
executable one. Compiler-first note: types cannot reach a formatted string built from spec input, so
a gate is the correct level here.

### e. LOW-2 — `standalone_deps` bypasses the profile-`baked` table

`crates/infrastructure/src/mailbox/standalone.rs:130-145` reads all three flags through
`std::env::var` and never consults the profile-`baked` table the monolith's `Config::resolve` uses.
Pre-existing (`ENFORCE_SERVICE_HOURS_GUARD` and `ENFORCE_ACCEPTANCE_TIMEOUT` have it too), and
harmless only while no `profiles:` block bakes one of these. One `profiles:` entry away from **half a
fleet routing and half appending** — the exact double-birth the code comment at `standalone.rs:139-141`
warns about. **Fold in if phase 3 fits the checkpoint budget; otherwise file it as its own issue with
this paragraph as the body.** It must not be dropped silently: this is the flip's fleet-parity risk.

## 5. Phases (checkpoint boundary marked)

- **Phase 1 — verify the existing switch (#589).** Build the metric-assertion harness following
  `orders_placed_metric.rs`'s single-binary/spy-provider pattern; assert `promotion_watch_tick`
  emits `reminder_promotion_due_lag_ms` **and** `mailbox_scheduled_depth` for every declared lane
  **with zeros** on an EMPTY backlog. Red-prove it: deleting `promotion_watch.rs:44-47` must fail.
- **Phase 2 — decide (a) and (b) in `specs/observability.yaml`**, with the reason at the line, plus
  the SPEC-LOG sentence. Spec + docs only.
- **── CHECKPOINT ──** The mob reads the actual diff of phases 1-2 before any new signal exists. What
  the checkpoint is for: whether the harness's failure mode is the *right* one, and whether (b)'s
  loosening is being accepted or restored. Any lens may stop the work.
- **Phase 3 — the Order-lane liveness signal.** **Test FIRST, against the phase-1 harness** — the
  ordering is the whole point of the #589 fold: write the zero-emission assertion, watch it fail
  because no watcher exists, then write the watcher. Contract in `specs/observability.yaml` in the
  same commit. Fold in LOW-1 (d) here; LOW-2 (e) per its escape clause.
- **Phase 4 — records.** ADR for the budget decision if (a) lands as a change; `STATUS.md`; SPEC-LOG.

## 6. Gates and fences

- `make rust` green · `make validate` **0 errors** · `check-drift` clean · warning baseline refreshed
  in the SAME commit if the surface moved.
- **"The monitor is verified" means a mutation that silences it goes RED** — not that
  `promotion_watch_tick(&pool).await` returns `Ok`. The named mutants, both of which MUST fail the
  suite by the end of phase 3: (i) delete `promotion_watch.rs:44-47`; (ii) make the new Order-lane
  watcher skip its zero emission when nothing is routed. If either mutant stays green, the phase is
  not done.
- **Fence**: no change to birth routing, to `record_order_birth_lag`'s call condition, or to
  `domain_events`. This chunk observes; it does not move the flip.
- **`HOLD: human`** — the chunk edits a money-path workflow's observability contract and the FROZEN
  door identity. Ready-for-review, TEAM reviewer pass, then coordinator merge
  (ADR-20260815-115220 as amended by ADR-20260815-134655).

## 7. Before the flip itself (the one-line ADR that follows this chunk)

1. **Does the flip get its own smoke?** Recommended **yes**, and it is cheap: with the flag ON in a
   non-production profile, place one order and assert (i) `OrderPlaced` lands exactly once, (ii)
   `order_birth_lag_ms{routed="true"}` has ≥1 point, (iii) the acceptance-deadline row exists. Without
   (iii) the flip's whole purpose is unverified. Gate-then-stabilize makes the smoke the precondition
   for flipping the DEFAULT, not for the gated form.
2. **What evidence must the flip ADR cite?** The three smoke assertions above; the phase-3 liveness
   series present and non-silent; the §4a/§4b budget decision recorded; and the fleet-parity posture
   for `ROUTE_ORDER_BIRTH_THROUGH_LANE` (§4e) — one value across monolith and standalone workers,
   stated, because a split fleet double-births.
3. **Rollback path**: flag back OFF is the recorded rollback; state in the ADR whether an order born
   through the lane before the rollback is affected (it is not — the birth already landed — but that
   must be written, not assumed).

## 8. Lenses to brief

Reversibility class (business's axis in [DECISIONS §44 MOB-COST-1](../proposals/DECISIONS.md), still
**OPEN — founder has not ruled**, so the standing rule is whole-roster-by-default): **mixed** — the
observability *contract* and the FROZEN door golden are cheap to change; the money-path success rule
(§4b) and anything the flip ADR then cites are not. Recommended roster, with what each catches:

- **observability-agent** — owns the contract; whether a dead-man's switch belongs on OTLP vs a fold.
- **young** — the operational/BAM split (this must NOT become a fold), and whether a re-baselined
  budget is being derived from a projection that a rebuild would change.
- **vernon** — monitor placement outside the lane it watches; head-of-line and lane-depth semantics.
- **dba** — the watcher's query shape and grouping cardinality (the `purpose` label).
- **farley / beck** — the mutation-provable test discipline; this chunk *is* a testing chunk.
- **reviewer** — the independent third look.
- **evans, ux-designer, legal-specialist, business-specialist, graphql-architect, holub** — invited by
  default per ADR-20260809-013142; "nothing in my lens" is a complete answer.

Any narrowing is the coordinator's to state openly, not the card's to assume.

## Findings

<!-- Executor and lenses append here. Empty at dispatch by design (ADR-20260816-020752; the first
     card lacked this heading and the executor had to invent it — sessions.md records the lesson). -->

### Lens verdicts (pre-code mob, ADR-20260809-013142) — all PASS, several CORRECT this card

**beck — the harness and the reds.** The card named ONE mutant; there are **three**, and one of them
is the anti-vacuity control without which a watcher that emits garbage passes:

1. delete `promotion_watch.rs:44-47` (the lag zero-seeding) — silences `reminder_promotion_due_lag_ms` only;
2. delete `promotion_watch.rs:62-66` (the zero-depth loop for declared-but-unseen lanes) — a SEPARATE
   seeding, which the card's §6 mutant (i) does not touch, so `mailbox_scheduled_depth` was unguarded;
3. hard-code the emitted lag to `0.0` at line 68 — the watcher still emits, and lies. Only a POSITIVE
   control (one due row ⇒ lag > 0, depth 1) reds it.

Shape rulings, all applied: ONE new binary, **ONE `#[tokio::test]` fn** (the precedent's constraint is
one test fn per PROVIDER, not per signal — two race the process-global meter binding); the drain lives
in `tests/main/spy_meter.rs` and is `#[path]`-INCLUDED, never copied; assert the **full sorted point
set of one tick by EQUALITY**, never `contains`; the test calls only `promotion_watch_tick`, never
`telemetry::meters::*` (that would be #588's deleted `enqueue_birth` crutch in a new costume); assert
the SCHEDULED backlog is empty BEFORE the empty tick, or "zero" means coincidence rather than seeding;
call ticks directly, never `spawn_*` + sleep. Do NOT retrofit `orders_placed_metric.rs` onto the new
harness in this chunk (Tidy First — structural change, separate commit). **Phase-order fix for §5
phase 3**: "watch it fail because no watcher exists" is a COMPILE error, not a red — land the new
watcher as an empty-bodied fn first, red the assertion, then fill it.

**observability — the contract.** §4a lands as **(B) UNCHANGED at 800**, but not as a bare comment:
re-baselining now would invent a percentile from a distribution that does not exist. Two conditions:
add a `latency_budget` on the handover (`order_birth_lag_ms`) so "paid order → restaurant told" stays
covered end to end, AND write the **re-baseline trigger** into the record — the first Fri/Sat
19:00–21:30 after the flip. **Liveness: NEVER zero-seed `order_birth_lag_ms`** — injected zeros poison
the p95 the flip is judged on. Two separate series instead: monotonic
`order_lane_watch_heartbeat_total{lane}` and gauge `order_lane_oldest_pending_age_ms{lane}`, emitted
every tick for every declared lane **including while the flag is OFF** (`routed="false"`); alert on
absence of increment, never a threshold. The bar for "verified": exact name, exact attribute set,
exact point count and value per declared lane on an empty backlog, plus a **missing-lane-fails**
assertion — return-value assertions do not count.

**farley — and this supersedes observability's §4b shape.** The `required_spans` hole is **not a flag
predicate — make it an ALTERNATION**: require `event.store.append` OR the lane-append span, same
verdict in both flag states, no gate inside the rule. *A success rule that passes when the money-path
append vanished is a gate that lies.* Verified at HEAD: `required_spans` is a flat list validated only
as ⊆ declared spans and `conditions` is free-form and structurally unvalidated — so if the loader
cannot express the alternation, land it and file the enforcement, but do NOT silence. Add **deploy-time
parity evidence** if cheap: each process emits its resolved flag state at startup
(`runtime_flag_state{flag,value,bin,version}`) — review-time parity is an assertion, this is evidence,
and it is what lets the flip be blocked while distinct values > 1. **For the flip ADR (§7), not this
chunk**: the ADR must cite smoke assertions executed **against the deployed monolith with the flag ON
in a non-production profile** (green CI proves the code path, not the deployed one), and the rollback
trigger currently **has no observer** — the ADR must name the human or the alert route, or the trigger
is decorative.

**vernon — double-birth is NOT reachable, and the real hazard is a different one.** Four absorbers:
both routes converge on `Order-{id}` with an expected-version precondition at 0 (OFF gates on
`should_deliver_order_placed` then saves at the loaded version; ON stages a door row and
`record_inbound_order_placed` returns `AlreadyRecorded` if any `OrderPlaced` is on the stream), the
door PK dedups, and the trigger skips on redelivery. **So §4e's last sentence and §7.2's line 169 are
WRONG, as is the code comment at `crates/infrastructure/src/mailbox/standalone.rs:139-141`: the word
is SPLIT-CLOCK, not "double-births".** The real split-fleet hazard, which nobody had named: under OFF,
`apply_schedules_in_tx` (`handler.rs:805`) carries the *PlaceOrderProcess* message, so the Order's
`OrderPlaced` receive schedules never apply; ON arms them (`handler.rs:446`). A split fleet therefore
gives **one birth and a coin-flip on the acceptance deadline, per order, invisibly** — invisible
because the lag histogram only emits on the routed path. Bounded by the rolling-deploy window, so the
§4e deferral stands, but the flip ADR must say THIS.

### Executor — phase 1 (#589), delivered

`crates/infrastructure/tests/main/spy_meter.rs` (the drain) + `crates/infrastructure/tests/
mailbox_liveness_metrics.rs` (one binary, one `#[tokio::test]`). All three mutants RED, each on a
DIFFERENT assertion — the discrimination beck asked for:

| Mutant | Assertion that reds | Message |
|---|---|---|
| (i) lag zero-seeding removed | `points(REMINDER_PROMOTION_DUE_LAG_MS)` on the empty tick | `left: []` vs `right: [({"actor_type": "Order"}, 0.0)]` |
| (ii) zero-depth loop deleted | `points(MAILBOX_SCHEDULED_DEPTH)` on the empty tick | `left: []` vs `right: [(…OrderAcceptanceTimedOut, 0.0), (…OrderExpired, 0.0)]` |
| (iii) lag hard-coded `0.0` | the positive control | `a reminder 90s overdue must show as ~90000ms of lag, not 0ms` |

Three refinements the rulings did not anticipate, each recorded because it costs the next executor
time:

- **Mutant (i) as literally written does not compile.** Lines 44-47 ARE the `let mut lag_by_actor`
  binding, so deleting them removes the declaration the loop below uses — a build error is not a red.
  The semantic mutant is `= BTreeMap::new()` (no seeding, same code shape). A named mutant must be
  stated as a SEMANTIC edit, never a line range, for exactly this reason.
- **Delta temporality is what makes "by EQUALITY" expressible.** Under the SDK default (cumulative)
  every flush re-reports every point ever recorded, so "the point set of ONE tick" cannot be written
  and the assertion silently degrades into `contains` — the very weakening beck forbade. The drain
  therefore installs `Temporality::Delta` and `drain()` TAKES (flush + read + reset), so no caller can
  read a vacuous empty set as a pass by forgetting a `force_flush`.
- **The expected lane set is a LITERAL, not derived from `REMINDER_SCHEDULES`.** An expectation
  computed from the same generated table the watcher reads agrees with the watcher by construction.
  A separate offline test pins the literal against the declaration, so adding a reminder lane in
  `specs/**` reds with an instruction instead of as an unexplained point-count mismatch.

Still owed to phases 2-4, none started this run: the §4a/§4b decisions (with farley's alternation),
the `routed`/heartbeat series, LOW-1's frozen-door golden, LOW-2 — and the **split-clock** rewording
of `standalone.rs:139-141` plus the acceptance-deadline coin-flip paragraph, which belong in the flip
ADR's fleet-parity section.

### Executor — phases 2-3, delivered

**§4b — the alternation LANDED, and the loader ENFORCES it.** `required_spans` is now a conjunction
whose terms are a span name **or** `{ any_of: [...] }`; `place-order` requires
`{ any_of: ["event.store.append", "order.lane.enqueue"] }`. Same verdict in both flag states, no
gate inside the rule. farley's escape clause ("land the shape and file the enforcement") was **not
needed** — the loader was extended instead, with four spec-data mutants red and no recompile:

| Mutant (semantic edit) | Rule that fires |
|---|---|
| an alternative names an undeclared span | `obs-required-span-undeclared` |
| a ONE-branch `any_of` | `obs-required-span-alternation` |
| `one_of:` instead of `any_of:` | `obs-required-span-shape` |
| a metric `latency_budget` with p95 > p99 | `obs-metric-budget` |

**A second hole was found and closed while landing it.** `the_required_observability_contracts_are_
actually_emitted` only ever checked `required: true` spans, so an alternation branch could name a
span nothing constructs and every `place-order` run would be scored by an unsatisfiable branch —
the same class of lie, one layer down. It now checks the UNION of required-true and
success-rule-named spans (mutant: drop the `.instrument(...)` at the enqueue seam ⇒ *"span
'order.lane.enqueue' has a constructor in spans.rs but no production call site invokes it"*).

**The ON branch had to be BUILT, not just named.** No span existed for the handover: the alternation
needs a span that lives in the SAGA's trace, and the lane delivery's append is a different delivery
with a different trace, so `order.lane.enqueue` (PRODUCER) was added and instrumented at
`flush_lane_enqueues_in_tx` — the infrastructure glue, the framework boundary that owns the write.

**§4a — UNCHANGED at 800/1500, with both conditions and the trigger AT THE LINE.**
`order_birth_lag_ms` carries its own `latency_budget`, and the re-baseline trigger is written into
the spec as a date: **the first Fri/Sat 19:00-21:30 after the flip**.

**The two numbers are NOT both derivations, and this section first said they were.** The review was
right to split them, because "derived" is a claim about provenance that a reader will not re-check:

- **p99 = 12000 ms is substantially derived**: one `configuration.yaml#/MAILBOX_HEARTBEAT_SECONDS`
  fallback pass (10 s) plus 2000 ms of drain slack. The 10 s is declared; the **slack is not** — it
  is a round allowance for claiming the lease, reading the row and committing, and nothing in the
  specs states it.
- **p95 = 1000 ms is a CHOSEN discrimination threshold, not a derivation.** It has no declared
  antecedent at all: the push wake is an in-transaction `pg_notify` with no declared latency, and
  the only declared cadences are 10 s and 60 s. The reason is good and stands — above a second
  means the push path is not carrying the handover and the fallback is — but that is a threshold
  picked to discriminate between two modes, not a number computed from configuration.

Neither is measured, which was the actual point (there is no distribution yet); "chosen to
discriminate" is the honest word for p95 and it is now the word at the line.

**§4c — two series, never a zero-seed.** `order_lane_watch_heartbeat_total{lane}` (monotonic) and
`order_lane_oldest_pending_age_ms{lane}` (gauge), emitted every tick for every declared routed lane
including while the flag is OFF.

**Seven mutants red — but NOT, as this section first claimed, "each on a different assertion".**
That claim was false and the review caught it: at `|ROUTED| = 1` the lane-level mutants collapse
onto two assertions, because with one routed lane there is no lane to drop *and* keep reporting.
The reds are real; the DISCRIMINATION is not, and it is worth naming precisely, because it is the
same degeneracy [#604 "The missing-lane anti-vacuity control is degenerate at one routed lane: it
asserts non-emptiness, not membership"](https://github.com/TheCaptainCompany/captain-food/issues/604)
files. Each mutant below is a SEMANTIC EDIT with the message it produces
(never a line range — the sharpened `sessions.md` rule this chunk itself wrote):

| Mutant (semantic edit) | Reds on | Observed |
|---|---|---|
| **A** `order_lane_watch_tick` no longer `add`s to the heartbeat counter | heartbeat point set, drained lane | *"every DECLARED routed lane heartbeats on every tick, drained or not"* — `left: []` vs `right: [({"lane":"Order"}, 1.0)]` |
| **C** it emits only for lanes that HAVE a backlog | **the same assertion, the same left/right** | ditto — indistinguishable from A at `|ROUTED| = 1` |
| **G** it drops a declared lane from its own set | **the same assertion**, plus `routed_lanes_are_still_the_ones_asserted` | *"the watcher's own lane set must be the declared one, not a second hand-kept list"* |
| **B** it no longer records the oldest-pending gauge | age point set, drained lane | *"a drained lane reports age 0, it does not go absent"* — `left: []` vs `right: [({"lane":"Order"}, 0.0)]` |
| **D** the age is hard-coded to `0` (it emits, and lies) | the gauge's positive control | *"a birth parked 90s on the lane must show as ~90000ms, not 0ms -- the VALUE is the alarm"* |
| **E** the pair is emitted ONCE per process instead of per tick | the SECOND drain, heartbeat | *"the heartbeat must INCREMENT on the second tick -- a monotonic counter that never increments is a dead watcher wearing a live series"* |
| **F** the PROMOTION watch seeds once at startup | its second drain | *"the dead-man's switch must RE-ASSERT on every tick: a signal emitted once proves only that the process started"* |
| **H** `declare_flag` records the declaration but never registers the observable gauge | the parity gauge's point set | *"a composition root must publish EVERY flag whose split across a fleet has a consequence"* — `left: []` vs the three resolved flags |

So the honest count is **eight mutants over five distinct assertions**. A/C/G share one and B shares
its shape with them; the assertions that genuinely discriminate are D (value, not presence), E and F
(the second drain) and H (registration).

E and F are the coordinator's addendum and it was a REAL hole: delta temporality plus a draining
read makes a seed-at-startup watcher drain identically to a correct one on the first tick, so
"every tick" — the whole dead-man's-switch claim — was unasserted until both watchers got a second
tick over an unchanged backlog.

**The declared lane population is now GENERATED.** `PM_LANE_ROUTED_DELIVERS` lived only in the
codegen, so nothing at runtime could name "the lanes a routed birth lands on" and the watcher's set
would have been a hand-kept literal — one forgotten edit from an unwatched lane. It emits
`ROUTED_LANES` into `application::generated::process_managers`, pinned by equality in BOTH
directions (a `contains` pin leaves an ADDED lane green, which is the direction that matters).

**farley's parity evidence FIT.** `runtime_flag_state{flag,value,bin}`, an OBSERVABLE gauge declared
at both composition roots. `version` is deliberately not a label — `service.version` is already a
resource attribute, and a label would multiply the series during exactly the rolling deploy it
watches.

**It IS spy-tested, and the earlier claim here that it "cannot honestly have one" was wrong.** The
review disproved it by execution, and the disproof is the important part: the tautology objection
defeats a test that calls `declare_flag` and then finds it, and says nothing about driving
`standalone_deps`, which is a real composition root that resolves the values from the environment
exactly as a deployed worker fleet does. Meanwhile **mutant H shipped GREEN** on the first cut —
delete `let _ = flag_state_gauge();` from `declare_flag` and the declaration is still recorded while
the observable gauge is never registered, so the series simply does not exist. That left the only
monitor able to see a split fleet, and the only evidence for obligation 1(iv) below, as the one
monitor in this chunk with zero reds. `mailbox_liveness_metrics.rs` now drives the root and asserts
the full point set — resolved values, not literals — re-asserted on a second drain, because an
observable gauge's whole reason for being is that it re-fires every export cycle. The deployed smoke
remains an obligation; it is no longer the ONLY proof.

**vernon's correction applied.** `standalone.rs` no longer says a split fleet "would birth some
orders twice"; it names the four absorbers, and states the SPLIT-CLOCK hazard that is real.

**Two things this chunk did NOT do**, neither dropped silently: **LOW-1** (§4d, the FROZEN door
identity's pinned golden) is not in the P2/P3 dispatch's scope list. It is now *cheaper* than the
card assumed — `ROUTED_LANES.source` puts the literal `pm:PlaceOrderProcess:OrderPlaced` in a
committed generated file, so a PM rename lands in the diff — but a codegen test asserting the
literal is still owed, and check-drift is not that test. **LOW-2** (§4e) stands deferred per its
escape clause; `runtime_flag_state` now makes its hazard observable, which was the actual worry.

### Follow-ups filed by the third look (not fixed in this PR)

Four, all found by the independent review of the full branch diff. None is in the P2/P3 scope list,
and each is filed rather than fixed because a review that fixes what it finds stops being a review.

- [#602 "Instrument the OFF birth route: `flush_staged_in_tx` emits neither alternation branch nor
  `event.publish`"](https://github.com/TheCaptainCompany/captain-food/issues/602) — **the sharpest,
  and it is about the thing this chunk shipped.** The alternation's OFF branch is not emitted on
  every live birth route: the mailbox PM lane births through `flush_staged_in_tx`, which constructs
  no span, so with the flag OFF a birth taken through that route satisfies NEITHER branch. Stated at
  the line in `specs/observability.yaml`. `event.publish` has the identical gap and is still
  `required: true` (pre-existing). Nothing caught it because `constructor_called` is a
  workspace-wide substring scan proving a constructor runs *somewhere*, never on *this workflow's*
  path — the guard #598 widened, widened correctly, and could not have made path-aware.
- [#603 "`order_lane_watch_heartbeat_total` is keyed `{lane}` only, so it cannot see a dead process
  in a multi-process fleet"](https://github.com/TheCaptainCompany/captain-food/issues/603) — with N
  watchers and no `service.instance.id`, the counter increments until the LAST one dies while the
  contract claims per-PROCESS death. Either take a `bin` dimension like `runtime_flag_state` has, or
  stop claiming it.
- [#604 "The missing-lane anti-vacuity control is degenerate at one routed lane: it asserts
  non-emptiness, not membership"](https://github.com/TheCaptainCompany/captain-food/issues/604) —
  `&ROUTED[..len-1]` is the empty slice at `|ROUTED| = 1`. Needs a synthetic two-lane input; it
  self-heals the day a second routed `deliver:` is declared, which is the day nobody will look.
- [#605 "Render per-metric `latency_budget` in the generated docs
  surfaces"](https://github.com/TheCaptainCompany/captain-food/issues/605) — the budget added in
  this very change is invisible at `emit/docs.rs:577` and `:1226`, which render name + type only.
  Verbatim the argument this branch used to justify extending the loader for `required_span_term`,
  applied to the alternation and not to the budget beside it.

## 9. Flip-ADR obligations (written here so they cannot evaporate)

The flip is a separate one-line ADR (§7). These are the things it must carry, recorded now while
the evidence for them is fresh:

1. **A DEPLOYED smoke, not a green CI run.** Green CI proves the code path; it does not prove the
   deployed one. With the flag ON in a NON-PRODUCTION profile, against the deployed monolith:
   (i) `OrderPlaced` lands exactly once, (ii) `order_birth_lag_ms{routed="true"}` has ≥1 point,
   (iii) the acceptance-deadline row exists, and (iv) `runtime_flag_state` reports **exactly one
   distinct `value` per `flag`** across every reporting `bin` — (iv) is the only assertion that can
   see a split fleet, and it is also the only proof the parity gauge itself works.
2. **The rollback trigger has NO OBSERVER, and the ADR must give it one.** "Flag back OFF" is the
   recorded rollback path, but nothing today watches for the condition that would call for it. The
   ADR names the human or the alert route — otherwise the trigger is decorative, which is worse
   than absent because it reads as covered.
3. **The SPLIT-CLOCK evidence, in vernon's words, not §4e's.** Double-birth is unreachable (four
   absorbers: both routes converge on `Order-{id}` with an expected-version precondition at 0, the
   door row's PK dedups, the trigger skips on redelivery). What a split fleet DOES produce is **one
   birth and a coin-flip on the acceptance deadline, per order, invisibly** — invisible because
   `order_birth_lag_ms` records only on the routed path. Bounded by the rolling-deploy window, and
   now observable through `runtime_flag_state`.
4. **The re-baseline trigger**: the first Fri/Sat 19:00-21:30 service after the flip. Until then
   `place-order`'s 800 ms reads LOOSE by decision, and that is recorded at the line rather than
   left to be rediscovered.
5. **The liveness series must be non-silent BEFORE the flip is armed** — they run today with the
   flag OFF precisely so this is checkable rather than assumed.
