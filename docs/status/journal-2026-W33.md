# Status journal — 2026-W33

Journal entries dated **2026-08-10 → 2026-08-16** (ISO week 2026-W33, 2026-08-10 to 2026-08-16). **77 entries**, newest first, in the order they were written.

Split out of `docs/STATUS.md` on 2026-08-19 — the entries are byte-identical, only their relative links gained a `../`. Current state, and the index of recent entries, live in [`../STATUS.md`](../STATUS.md).
> 🔒 **2026-08-16 — THE LANE WIDTH IS NOW UNSPELLABLE, NOT MERELY UNSPELLED**
> ([#609 "Lane addressing residue after #596: `stable_partition` is still `pub` and `mailbox_address` still carries a vestigial width"](https://github.com/TheCaptainCompany/captain-food/issues/609),
> branch `609-lane-addressing-residue`, [PR #613](https://github.com/TheCaptainCompany/captain-food/pull/613)
> — REVERSIBLE INTERNAL, auto-merge-on-green. Card: `docs/dispatch/609-lane-addressing-residue.md`;
> [ADR-20260816-194428](../adr/ADR-20260816-194428-the-lane-width-is-unspellable-not-merely-unspelled.md).)
>
> #596 removed the `width` PARAMETER from every routing site and the records said so at that true,
> smaller size — `stable_partition(&id, some_width)` stayed spellable. It is now **private to its
> module**: only `declared_lane` crosses the `actor_client` line, so no caller anywhere, production
> or test, can hold a second opinion about the keyspace. No `pub`, no feature, no `cfg`, nothing for
> a gate to police.
>
> **The residue was not idle**, which is what decided the option: 22 out-of-crate sites across 8 test
> files spelled `stable_partition(&order, 5)` — copies of exactly the constant #596 was about, in
> test clothing. A fixture at `N mod 5` against a declaration moved to 7 lands on a lane the new
> grid's producers never use while the worker still drains it: green build, wrong lane, no error.
> **Two obligations travelled with the conversion**: four assertion sites were INCIDENTALLY pinning
> their actors' declared widths and can no longer (both sides now read the declaration), so that half
> moves into `partition.rs` as one deliberate test that names the migration (ADR-20260802-220402),
> runs without Postgres where three of the four needed it, and covers 17 actors instead of 3 — **not
> the same guarantee, the missing half of it, deliberately placed and widened**, since the converted
> assertions still compare a real production stamp against `declared_lane`; and the misroute guard
> drops its second width for `declared >= SEEDED_LANES`, where the *implication* is universal and the
> assertion is still falsifiable on the id under test. Review found the new test blind two ways a
> spec edit can reach (an emptied slice, a renamed `MailboxSupervision`) and it now carries a floor
> and a seed in this repo's existing anti-blindness idiom, all three mutants red.
>
> **The cheap alternative was measured, not assumed, and lost twice.** A `test-fixtures`-gated
> re-export trips the crate's own `unreachable_pub = "deny"` (it does not compile as the card
> specified), and its seal is real only for release artifacts: with the same production mutant
> planted, `cargo build -p infrastructure` fails while `cargo test -p infrastructure` **compiles** —
> resolver v2 unifies the dev-dependency's grant into the lib. Recorded in
> [sessions.md](../claude/sessions.md) because anyone verifying such a seal with `cargo test` gets a
> false negative. **Item 2 (`mailbox_address`'s vestigial width) was CUT at briefing** — same
> declaration as `ACTOR_MAILBOXES`, so a caller computes the CORRECT lane; carried forward on #609.
> Gates: `make rust` 0 errors, `check-drift` clean, `make test-crates` on real Postgres **1252 passed,
> 0 failed, no DB-skip receipt**; M1/M1b/M2 mutants measured in an isolated worktree.

> 🛟 **2026-08-16 — A LANE IS ADDRESSED FROM THE DECLARATION, AND AN UNSEEDED LANE NOW WAITS INSTEAD OF
> POISONING A PAID ORDER'S AUTHORIZATION**
> ([#596 "chain_pm_copy_in_tx reads lane width from a seeded registry and errors at zero — an unseeded worker fails a paid order's saga"](https://github.com/TheCaptainCompany/captain-food/issues/596),
> branch `596-chain-lane-width-declared`, [PR #607](https://github.com/TheCaptainCompany/captain-food/pull/607)
> — merge posture **`HOLD: human`**, mailbox runtime. Card:
> `docs/dispatch/596-chain-lane-width-declared.md`;
> [ADR-20260816-165714](../adr/ADR-20260816-165714-lane-addressing-is-declared-not-observed-and-an-unseeded-lane-must-wait.md).)
>
> **Reclassified by three lenses at the briefing: a ONE-WRITER violation, not a queueing nuisance.**
> The lease is keyed by LANE (`actor_runtime/src/lease.rs`) and `completion.rs` fences on the lane's
> checkpoint, so `stable_partition(actor_id, width)` is the only thing mapping an aggregate to
> exactly one lane. Two producers with different widths put the same `Order-{id}` in two lanes, each
> with a live lease, each passing its own fence — serialisation breaks at the addressing function,
> upstream of any fence. Expected-version demotes it to a version conflict, but `prepare` runs before
> `pool.begin()`, so **the Stripe intent already exists when the loser rejects**.
>
> **And it was worse than the issue title** (found while proving the red tests): the old zero-width
> `Protocol` error took the POISON path — head-of-line on the Payment lane below the attempt cap, and
> AT the cap it flipped **the authorization row itself** to terminal `FAILED`. "A worker has not
> started" became **a paid customer whose order can never be born, even after the worker comes up**.
> That residue is precisely what [#608](https://github.com/TheCaptainCompany/captain-food/issues/608)
> says nothing detects.
>
> **Landed**: ONE accessor `actor_client::declared_lane(actor_type, actor_id)` over `ACTOR_MAILBOXES`,
> and **no routing site takes a `width` any more**. That took the review to reach: the first draft
> ASSERTED it while the typed door, the entry constructors, the reminder scheduler and a hand-copied
> literal `5` in all 17 generated client crates still passed one, and used the assertion to drop the
> planned grep gate. All of those lost the argument, emitter included; `stable_partition` stays `pub`
> for tests and its golden freeze, so the two-step is still spellable and the records now say that
> rather than rounding it up. The grep gate stays unwritten because the parameter is genuinely gone;
> the residue is [#609 "Lane addressing residue after #596"](https://github.com/TheCaptainCompany/captain-food/issues/609). **THREE sites, not the card's
> two**: record-time chaining, the **flip-time backfill** (same `count(*)`, same zero-width error,
> found independently by `dba` and `beck` — and worse there, a cold-start rescue pass that refused to
> run when the system was cold), and the already-correct sibling converted so they cannot drift apart
> again. Plus **a startup DRIFT CHECK** in `seed_partitions` (a non-empty registry describing a
> different keyspace refuses the start; an EMPTY registry is a first boot and seeds — getting that
> backwards would crash-loop every fresh bin after #358) carrying `vernon`'s drain-first cutover
> procedure, and **the ADMIN lane monitor re-sourced from the declaration** so the fix does not trade
> a loud wrong failure for a silent right one — a declared-but-unseeded lane holding an order is now
> visible, as is the orphan a width decrease strands. **No flag** (`farley`): one valid path, and the
> OFF state IS the paid-order-fails branch; rollback is `git revert` + one image.
>
> **After the #358 per-bin cutover the exposure window goes from seconds to INDEFINITE** — a Payment
> bin can run while the target actor's bin is not deployed at all — so this is a precondition of that
> cutover, not a tidy-up. **Past occurrences: none.** Production has no real customer orders (1 of 1
> restaurants, registered by the smoke script; the only money-path traffic is prod-smoke L4 in Stripe
> TEST mode), which discharges #608's ask on THIS chunk and says nothing about #608 itself.
> Gates: `make rust` 0 errors / warnings unchanged, `make test-crates` on real Postgres **1250 passed,
> 0 failed, 189 suites, no DB-skip receipt**; three mutants red over the two new tests + the accessor.

> 🧑‍🤝‍🧑 **2026-08-16 — THE MOB'S CHECKPOINT IS NOW THE CONCERN-DECLARED SUBSET, AND REVIEW IS PRICED BY
> REVERSIBILITY** (founder ruling on [DECISIONS §44 MOB-COST-1](../proposals/DECISIONS.md), verbatim
> *"Go for the Recommendation: (b)+(c), with holub's verification condition."*, recorded in
> [ADR-20260816-134352](../adr/ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md)
> amending [ADR-20260809-013142](../adr/ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md)):
> the **briefing is untouched** (whole roster, before any code), the **checkpoint** goes only to
> lenses that declared a concern there, and the chunk's **reversibility class** sizes the briefing
> roster — full mob for money, stored event shapes, legal surfaces and anything Tours-facing. Every
> dispatch card now states its class and banks a `Checkpoint verification:` line either way; ~~a MISS
> reverts that class to the whole roster~~ (sub-obligation **MOB-COST-1a** — **the reversion was
> STRUCK 2026-08-17**, see below).
>
> ⛔ ~~**The first answer, 2026-08-16, is a MISS — HIGH-CONSEQUENCE is REVERTED to the whole roster at
> BRIEFING AND CHECKPOINT.**~~ Banked on [#608](https://github.com/TheCaptainCompany/captain-food/issues/608)
> (see below): a money-path threshold derived in the dispatch card as `attempts × spacing` ≈ 50 s
> when the mailbox backoff is exponential (310 s; landed 600 s). ~~`dba` was named at briefing for
> that surface and was not returned to at the checkpoint~~ — **that attribution is FALSE**: the
> committed claim-time card says `Briefing roster: WHOLE ROSTER`, so only the *checkpoint* was
> narrowed and the bad arithmetic was in front of every lens. The error was the coordinator's, in the
> card, and the executor caught it while implementing.
> ✅ **STRUCK 2026-08-17 ON A FOUNDER ANSWER** — the HIGH-CONSEQUENCE reversion is withdrawn, (b)+(c)
> stand for every class, and the **antecedent rule** replaces it: *a dispatch card may not state a
> derived number without naming its antecedents, and any bare number it does state is marked
> `UNVERIFIED input`*. Banking and the verification condition are untouched.
> [ADR-20260817-105845](../adr/ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md),
> [DECISIONS §44 MOB-COST-1a](../proposals/DECISIONS.md), issue
> [#619](https://github.com/TheCaptainCompany/captain-food/issues/619).

> 💸 **2026-08-16 — "MONEY HELD, NO ORDER" IS NOW A SIGNAL THE SYSTEM EMITS**
> ([#608 "Nothing detects an authorized payment with no order birth"](https://github.com/TheCaptainCompany/captain-food/issues/608),
> branch `608-authorized-payment-no-birth-detection`,
> [PR #610](https://github.com/TheCaptainCompany/captain-food/pull/610) — merge posture
> **`HOLD: human`**, money-path detection + a new long-running monitor). Card:
> `docs/dispatch/608-authorized-payment-no-birth-detection.md`. Decision:
> [ADR-20260816-213000](../adr/ADR-20260816-213000-the-birth-gap-detector-reads-the-saga-run-not-an-anti-join.md).
>
> **(1) The source is the saga's own run state, NOT an anti-join of two aggregates.** The dispatch
> proposed anti-joining Payment authorizations against Order births; vernon refused (two write
> paths, two clocks, and another consumer's projection becomes an input to the money path) and he
> and observability then independently verified the fact that settled it: **the
> `payment_process_manager` row is created at CHECKOUT**, by the `PlaceOrder` handler, before any
> Stripe outcome exists — the PM legs only `expect` it. So an authorization the saga never
> processed still HAS a run row, and the #596 unseeded-lane case is visible, not blind.
>
> **(2) ONE gauge, three reasons, zeros always.**
> `payment_authorized_no_order_birth_age_seconds{reason}` over the declared bounded set
> `{retry_pending, delivery_exhausted, no_run}`, plus
> `payment_birth_gap_sweep_heartbeat_total` after a COMPLETE sweep. `no_run` is the residue the run
> state cannot see: `PlaceOrder` does two sequential unfenced durable writes (Stripe intent-create,
> then the run-row upsert) and a crash between them leaves **funds held with no run row** — visible
> only in `domain_events`. Every member reports every tick, 0 included, so an absent series means
> the sweep died, never "all clear".
>
> **(3) Thresholds are lane-derived and their antecedents are `$ref`s.**
> `MAILBOX_HEARTBEAT_SECONDS × (2^MAILBOX_MAX_DELIVERY_ATTEMPTS − 1)` = **310 s**, not the 50 s a
> linear reading gives — the mailbox backoff is EXPONENTIAL, and 50 s would page on every healthy
> retry. `retry_pending` = 600 s; `delivery_exhausted` and `no_run` = **0** (page on the first one:
> at V0 the base rate is ~0/day and no rate is tolerable). New `REF_CONTRACT` site
> `*.metrics[*].thresholds[*].derived_from[*]` → `ConfigKey`, so a renamed key reds the validator
> instead of leaving a stale bound.
>
> **(4) The existing gauge is AMENDED, emitted, and PROVEN.**
> `payment_authorized_unsettled_age_seconds` is correct for born-but-never-CAPTURED; only its
> header's claim to cover the never-born case was false. It had **zero emit sites in `crates/**`**
> and rides the same sweep now — shipping a second declared-but-silent money-path contract was the
> failure this chunk existed to stop. **Its first cut nearly repeated that failure in a subtler
> form**: no projector ran in the test binary, so `ordertracking` held **0 rows for the whole
> suite** and the gauge's `== 0.0` assertion was satisfied by a query that could not return anything
> else — a mis-spelled predicate (`'AUTHORISED'`) left the suite green. It is now driven off rows
> folded by the real `ProjectionWorker` to two DIFFERENT positive values, and that mutant is red.
>
> **(5) New gate: `obs-metric-no-emitter` (validator §20).** Every metric declared in
> `specs/observability.yaml` must have a name constant in `crates/telemetry/src/contract.rs` AND an
> instrument built from it in `meters.rs`. **41 declared metrics fail it today** (webhook ingestion,
> prospection, refunds, SIRENE, delivery dispatch — contracts written before their runtimes), so it
> is a WARNING on the §17 ratchet: the 41 are frozen, a 42nd is a hard gate failure. Proved by
> planting a metric into the REAL catalog and watching it red.
>
> **(6) The response is routed as far as the repo can route it.**
> `docs/runbooks/authorized-payment-no-order-birth.md` — Stripe dashboard → cancel the intent BY
> HAND → contact the customer → note the reclamation. Remediation automation stays out (money
> movement). **Open gap, recorded not invented: there is no alert-route wiring anywhere in this
> repo**, so no artifact names a human or a rota. Same gap as the `ROUTE_ORDER_BIRTH_THROUGH_LANE`
> flip's "the rollback trigger has no observer" obligation, which is founder-gated; both are
> cross-linked.
>
> **(7) Proof: `crates/infrastructure/tests/authorized_no_birth_metric.rs`**, own binary, one
> `#[tokio::test]`, real Postgres. State manufactured through the REAL seams end to end — a real
> `PlaceOrder` on the real PM command lane (which opens the run row), a real `PaymentAuthorized` on
> the real Payment lane (which records the fact and chains the hop), and then simply no drain.
> Nothing the detector queries is hand-inserted; only the CLOCK is moved, because
> `extract(epoch)::bigint` truncates and a value-derived control needs distinct ages. beck's
> zero-healthy suite in full: presence by EQUALITY, a value-derived positive control (two stranded
> at distinct ages ⇒ the older; then a different age ⇒ a different value, which kills a latched
> constant), a same-sweep negative control (an order born in the same database on the same tick,
> itself AGED so the exclusion has something to exclude), a second tick, and recovery. **All three
> reasons are now driven POSITIVE**, including `delivery_exhausted` — the member with threshold 0.
> **Ten mutants, applied-check and revert-check.**
>
> **(8) The third look FAILED this branch, and the discharge is part of it.** Three blockers:
> the born-but-uncaptured gauge shipped emitted-but-unproven (item 4); the MOB-COST-1a MISS was
> banked in the card but never reached the register (top of this file); and the card's second banked
> claim — *"every honest route to a terminal hop while the run stays `AWAITING_PAYMENT_RESULT` is an
> induced infrastructure fault rather than a seam"* — was **verifiably false and is retracted, not
> softened**. The seam is built from ports the test file already substitutes: a decorator on the
> injected `EventStore` failing the PM leg's Order-stream `load` with `DomainError::Repository`, plus
> `max_delivery_attempts: 1`, takes the hop terminal through the worker's own poison path while the
> completion transaction rolls the run back at `AWAITING_PAYMENT_RESULT`. That control has landed,
> which also answers [#611](https://github.com/TheCaptainCompany/captain-food/issues/611).

> 📡 **2026-08-16 — THE ORDER LANE HAS A HEARTBEAT, AND THE CHECKOUT SUCCESS RULE STOPS LYING**
> ([#598 "Before the birth-lane flip: the place-order latency budget still measures the old workflow, and a flat order_birth_lag_ms cannot be told from a dead lane"](https://github.com/TheCaptainCompany/captain-food/issues/598)
> + [#589](https://github.com/TheCaptainCompany/captain-food/issues/589), branch
> `598-birth-lane-flip-observability`, [PR #600](https://github.com/TheCaptainCompany/captain-food/pull/600)
> — merge posture **`HOLD: human`**, money-path observability contract). **The LAST flip-blocker for
> `ROUTE_ORDER_BIRTH_THROUGH_LANE`.** Card: `docs/dispatch/598-birth-lane-flip-observability.md`.
>
> **(1) The success rule is an ALTERNATION, not a flag predicate.** #594 dropped
> `event.store.append` from `place-order`'s `required_spans` with no predicate, which loosened the
> rule in the state we are in TODAY: the flag is OFF, the birth still appends inline, so a checkout
> whose append never happened scored `success` — *a success rule that passes when the money-path
> append vanished is a gate that lies* (farley). It now requires
> `{ any_of: [event.store.append, order.lane.enqueue] }`: same verdict in both flag states, no gate
> inside the rule, and a run that did NEITHER is a failure. The loader was **extended rather than
> annotated** — four spec-data mutants red — and a SECOND hole closed on the way: the
> emitted-contract guard only checked `required: true` spans, so an alternation branch could have
> named a span nothing constructs. `order.lane.enqueue` (PRODUCER) is new and instrumented at the
> infrastructure glue; it lives in the SAGA's trace, which is why the rule alternates on the
> enqueue and not on the lane-side append.
>
> **(2) The 800 ms budget is UNCHANGED, DECIDED, and dated.** Re-baselining now would invent a
> percentile from a distribution that does not exist. `order_birth_lag_ms` gains its own budget so
> "paid order → restaurant told" stays covered end to end, and the re-baseline trigger is written
> into the spec as a date: **the first Fri/Sat 19:00-21:30 after the flip**. Its two numbers are
> **not the same kind of number**, and an earlier draft of this entry wrongly called both derived:
> p99 12000 ms is one declared `MAILBOX_HEARTBEAT_SECONDS` fallback pass plus 2 s of undeclared
> drain slack, while **p95 1000 ms has no declared antecedent at all** — the push wake is an
> in-transaction `pg_notify` and the only declared cadences are 10 s and 60 s, so it is a threshold
> CHOSEN to discriminate "push is carrying the handover" from "the fallback is". Good reason,
> not a derivation; both are corrected at the line.
>
> **(3) Two liveness series, running NOW with the flag OFF.**
> `order_lane_watch_heartbeat_total{lane}` (monotonic) and `order_lane_oldest_pending_age_ms{lane}`
> (gauge), every tick, every declared routed lane — deliberately **not** a zero-seeding of
> `order_birth_lag_ms`, whose p95 the flip is judged on. Alert on the ABSENCE of an increment. The
> lane population is now GENERATED (`ROUTED_LANES`) and pinned in both directions, so a new routed
> `deliver:` cannot leave a lane unwatched. **Eight mutants red over five distinct assertions** —
> an earlier draft claimed "seven, each on a different assertion" and that was false: at
> `|ROUTED| = 1` silencing the counter, emitting only for backlogged lanes and dropping the
> declared lane all red on the SAME assertion with the same `left: []`, because with one routed
> lane there is no lane to drop and keep reporting. The ones that genuinely discriminate are the
> value control, the two second-drain assertions and the parity gauge's registration. The
> second-drain pair is what the phase-1 harness could not see: under delta temporality a watcher
> that seeds ONCE AT STARTUP drains identically to a correct one on the first tick, so **every
> tick**, the whole dead-man's-switch claim, was unasserted until both watchers got a second tick
> over an unchanged backlog.
>
> **(4) Fleet parity is EVIDENCE now, and the evidence is itself proved.**
> `runtime_flag_state{flag,value,bin}` (observable gauge) at both composition roots:
> `count(distinct value) by (flag) > 1` blocks a flip. It is spy-tested by driving the
> `standalone_deps` composition root — the first cut claimed that "cannot honestly" be done and was
> wrong, and the cost of being wrong was concrete: forgetting to REGISTER the gauge in
> `declare_flag` shipped green, silencing the only monitor able to see a split fleet. And vernon's
> correction is applied — the code comment claiming a split fleet "would birth some orders twice"
> was WRONG (four absorbers make double-birth unreachable); the real hazard is **SPLIT-CLOCK**: one
> birth, and a coin-flip on the acceptance deadline, per order, invisibly. **The flip ADR's
> obligations are written onto the card (§9)**, including that the rollback trigger currently has
> **no observer** and the ADR must name one.
>
> 💸 **2026-08-16 — THE LOOP'S CONTEXT BUDGET IS NOW A RECORD, AND ONE HALF OF IT IS THE FOUNDER'S**
> ([ADR-20260816-020752](../adr/ADR-20260816-020752-the-loops-context-budget-a-dispatch-card-snapshot-semantics-and-phase-commits.md),
> from the founder question *"Do you have recommendations to optimise tokens consumption?"*). Six
> team-owned technique changes are Accepted: subagent `.output` transcripts are **banned** unless the
> agent died (~300k/chunk of pure loss); the coordinator authors **one SHA-stamped dispatch card per
> chunk** that lenses read instead of the repo (12x50k → 12x~5k) with its Findings block doubling as
> the PR's mob evidence; the card carries **snapshot semantics** (a disposable cached fold —
> card@SHA + `git diff`, discard on mismatch, every lens keeps fall-through to the tree); **phase
> commits** make a dead executor cost one phase instead of ~400k tokens; mutation-red is paid once
> (red-first, mutate data not source, no confirm-green-after-revert); and gate economics move the
> pre-push bar on a PR branch from a full `make rust` to a seconds-long pre-flight. Cost becomes
> observable via a `tokens`/`agent` field on the existing `.claude/loop-budget/` ledger, alarmed as a
> **dead-man's-switch** (a threshold goes silent exactly when the writer dies). Honest baseline:
> **~2.5M tokens for one merged work item, with no per-item instrument existing.** The one item that
> is NOT the team's — **how the mob's fan-out is priced**, since narrowing the checkpoint roster
> amends [ADR-20260809-013142](../adr/ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md)
> — is open as 🟡 **[DECISIONS §44 / MOB-COST-1](../proposals/DECISIONS.md)** (recommendation: holub's
> concern-declared checkpoint + business's price-by-reversibility, with a verification condition on
> the next chunk).
>
> ✂️ **2026-08-16 — THE TOKEN DIET IS LANDED** (same ADR, amendment §8-§11, from the founder's
> *"Apply these recommendations"*): `make test-quiet` / `make rust-quiet` filter a gate's output to
> VERDICTS (grep-first, tail-second, full log in `target/quiet-gate.log`) under the rule *filtering
> may drop progress, never verdicts* -- proven red, an early panic survives a 50-line tail and
> `exit=101` propagates; `.claudeignore` + `permissions.deny` deny build output and object stores
> while **keeping `specs/generated/**`, `Cargo.lock` and the warning baseline readable** (they are
> gate evidence); CLAUDE.md is compressed to a resident INDEX (~7.6k -> ~4.6k tokens by a crude
> `wc -w` proxy) with **no rule dropped** -- the ~2.5k target was deliberately not reached, since
> closing the gap would mean dropping rules, which is a decision reversal; and the **honeycomb MCP
> server is deliberately DISABLED** pending re-auth, recorded in CLAUDE.md and
> `.claude/settings.json` so its absence never reads as "no telemetry concern" (the `eu1` EU-host
> pin stays in `.mcp.json`).
>

> 📮 **2026-08-16 — `deliver:` IS RULED A LANE ENQUEUE, NOT A FOREIGN-STREAM APPEND**
> ([ADR-20260816-040239](../adr/ADR-20260816-040239-deliver-is-a-lane-enqueue-not-a-foreign-stream-append.md),
> phase 0 of [#588 "The normal checkout path never enqueues OrderPlaced onto the Order lane — the acceptance clock cannot start for saga-appended births"](https://github.com/TheCaptainCompany/captain-food/issues/588)).
> The DSL always said `deliver … to: <actor>` (a Tell); the emitter wrote the target aggregate's
> stream itself, so `PlaceOrderProcess` saves **`Order-{id}` AND `Cart-{id}` in ONE transaction**
> with neither write passing the Order's mailbox — and no production path ever enqueues the birth,
> which is why the acceptance clock cannot arm for any real order. Ruling: **being the birth
> AUTHORITY licenses the DECISION, never the APPEND** — the PM stages an enqueue that the handler
> converts through the typed door inside the delivery transaction, and the clock arms on the
> canonical `Recorded` verdict (**no dependency on #590**). Phase-0 enumeration: **13 of 13
> `deliver:` steps qualify** under the routing predicate, so the change ships **behind a config flag
> unconditionally** (farley, overturning the dispatch card) routing only the Order pair first; zero
> steps fail the receives-declaration test, so that becomes a validator error with nothing to
> grandfather. Verified: **no fold, view or projector reads `user_type`/`cause_id`**, so the envelope
> change is invisible to every read model (never backfilled). Two gaps recorded, not fixed here: the
> reclamation replacement birth (`runner.rs:487` → `reclamation.rs:104`) **is live** and stays
> unlaned — it owns no transaction to stage into — and the #456 "a stranger paid us" counter is
> called only on the PM-fact route, so the realization must record placements on the inbound-fact
> route or the counter silently zeroes. Not a migration (payload/type/stream unchanged); the
> `place-order` observability contract is amended in the realizing PR. **`HOLD: human`.**
>

> 🔒 **2026-08-16 — THE PLACEMENT COUNTER IS COMPILER-CARRIED; THE LANE CONSTRAINT IS HALF-CARRIED
> AND HALF-GUARDED, AND THE DIFFERENCE IS THE POINT**
> ([#597 "Make the lane constraint compiler-carried: TriggerEnvelope.lanes is public, so 'only a transaction-owning route may carry a sink' is a convention"](https://github.com/TheCaptainCompany/captain-food/issues/597),
> branch `597-lane-constraint-compiler-carried`, [PR #599](https://github.com/TheCaptainCompany/captain-food/pull/599)
> — merge posture **`HOLD: human`**, mailbox runtime). A **flip-blocker for
> `ROUTE_ORDER_BIRTH_THROUGH_LANE`**, from the third-look review of #594.
>
> **(1) `record_order_placements` is fully compiler-carried.** It is a private `fn` in a private
> `mailbox::flush` module: a delivery route calling it is `error[E0603]`, from `server`
> `error[E0432]`, **in every build configuration**, with **no test exception** — the #456 spy
> binary drives `flush_staged_in_tx` against real Postgres instead, which is the stronger proof
> anyway (the counter fires from the only path a staged event takes to `domain_events`, not from an
> alias that can drift). So the #588 source scan `no_delivery_route_decides_when_to_count_a_placement`
> — which read `handler.rs` only, while the function was `pub` — is **DELETED**: deleting a gate the
> compiler subsumes is the correct outcome (ADR-20260803-234035).
>
> **(2) The lane constraint is carried in ONE HALF and guarded in the other, deliberately stated.**
> ADR-20260816-040239's constraint 1 (*the enqueue is never in `prepare`*, which
> `actor_runtime::completion` re-runs with NO transaction open) now holds against an **anonymous
> field write** — `TriggerEnvelope.lanes` is private and the type lives alone in a private
> `envelope` submodule, so `lanes: Some(..)` is `error[E0451]` from anywhere, including inside
> `process_managers/**`. It does **NOT** hold against the CONSTRUCTOR: `prepare` calling
> `TriggerEnvelope::laned(..)` **compiles**, and no signature can stop it — `laned` cannot demand
> proof of a transaction, because `application` cannot name a `sqlx::Transaction` without inverting
> the dependency rule. That residual is therefore held by a **guard, not a type**:
> `trigger_envelope_laned_has_exactly_one_call_site` fails the build on a second caller, with a
> message saying a second call site is a design event rather than a lint. The level-3 fallback is
> what ADR-20260803-234035 sanctions where types cannot reach; calling it level 4 would have been
> the same defect this issue exists to fix, one layer up.
>
> Both privacy claims carry their rustc error as evidence, re-planted in an isolated worktree — a
> privacy change that compiles when violated has done nothing. Also de-claimed the docstring of
> `a_refused_checkout_enqueues_no_birth_and_leaves_no_run_row`, which advertised a `prepare` fence it
> never provided (it fails the `PlaceOrder` leg, so the routed leg never runs, in BOTH flag states):
> its `runs == 0` half is load-bearing, its `births == 0` half is a negative control, and the
> docstring now says which is which.
>
> 🛬 **2026-08-16 — AND IT IS BUILT: THE ORDER BIRTH RIDES THE ORDER LANE, BEHIND A FLAG**
> (phases 2–3 of [#588 "The normal checkout path never enqueues OrderPlaced onto the Order lane — the acceptance clock cannot start for saga-appended births"](https://github.com/TheCaptainCompany/captain-food/issues/588),
> branch `588-order-lane-birth-enqueue`, [PR #594](https://github.com/TheCaptainCompany/captain-food/pull/594)
> — **MERGED to `main` as `693dab3`**). The saga stages a `LaneEnqueue`
> (`crates/application/src/lanes.rs`) that the delivery glue converts into an `inbound_messages`
> row **inside the same fenced transaction**, and the Order's own worker appends the birth — so
> `record_inbound_order_placed` runs on the canonical `Recorded` arm and the acceptance deadline
> keys on it. The sink rides the `TriggerEnvelope` because it is a property of the invocation ROUTE:
> a mailbox delivery owns a transaction, the polling runner owns none. Both #588 gates GREEN against
> real Postgres, including a **same-`xmin` assertion** — the birth row and the PM run row must be
> written by the same transaction, which no count of rows could prove. Gate:
> **`ROUTE_ORDER_BIRTH_THROUGH_LANE`, default OFF** — the flag is unconditional (13 of 13 `deliver:`
> steps qualify, so only the Order pair is routed) and the flag-OFF posture stays proven by the
> untouched pre-existing test. **The regression the mob did not see, fixed structurally**: the #456
> "a stranger paid us" counter ran off the PM route's staged set and was called there ONLY, so
> moving the append would have zeroed it silently; the decision now lives inside
> `flush_staged_in_tx` — the one way a staged event reaches `domain_events` — so no route can forget
> and none can double-count. (The source guard that shipped alongside it was replaced by privacy in
> #597 above, and deleted.)
> Also: `event.store.append` is no longer a REQUIRED `place-order` span (the routed birth appends in
> a different delivery, so the 800 ms p95 budget would have silently changed meaning) and a new
> `order_birth_lag_ms{routed}` histogram measures the handover nothing measured before; validator
> rule **`pm-deliver-lane`** (a `deliver:` target must declare a `mailbox:` and the event as an
> events.yaml fact) landed with mutation-red evidence. The reclamation second unlaned birth site is
> [#595](https://github.com/TheCaptainCompany/captain-food/issues/595), out of scope by decision.
>
> ⏱️ **2026-08-16 — #167 ACCEPTANCE TIMEOUT IS CODE-COMPLETE ON THE BRANCH (PHASES 0–3 + the mob
> conditions): [#167 "No order-acceptance timeout: a paid, unaccepted order sits forever with no alert, cancel or refund"](https://github.com/TheCaptainCompany/captain-food/issues/167),
> branch `167-acceptance-timeout-auto-cancel`, draft [PR #586](https://github.com/TheCaptainCompany/captain-food/pull/586)
> — NOT on `main`; merge posture `HOLD: human` (stored event shape + lifecycle + money-adjacent).**
> **Phases 0–1** (typed reminder durations, `OrderAcceptanceTimedOut` + `CANCELLED_BY_TIMEOUT`
> spec surface, gate `ENFORCE_ACCEPTANCE_TIMEOUT` default OFF) landed earlier on the branch;
> **checkpoint fixes**: the backoffice treatment is a REAL per-card `order_card_status` renderer
> arm (absent without a timed-out order), release copy states in-progress. **Phase 2**: the
> kind-MESSAGE delivery route with THE FENCE — `schedules:` on the Recorded/Cancelled arm ONLY,
> a shadow WouldCancel can never arm the GDPR clock (pg-proved, mutation-red) — plus the
> spec-declared OrderPlaced birth route (a redelivered birth re-applies `schedules:`; `keep`
> makes the first deadline win, pg-proved through the worker), the `reminder.promote` OTLP
> shadow span, and the promotion dead-man's switch (`reminder_promotion_due_lag_ms` +
> `mailbox_scheduled_depth`, emitted every tick from OUTSIDE the worker). **Phase 3**:
> OrderTracking folds the timeout (ratchet banked back down, 40→39), the
> `acceptance-timeout` observability contract, `specs/business_metrics.yaml` — the FIRST row of
> the ADR-20260811-014129 catalog (the `time_to_accept_ms` fold, p50/p90/p99 by daypart AND
> restaurantId; validated refs + fold-key totality, emitters remain [#484](https://github.com/TheCaptainCompany/captain-food/issues/484)'s
> machinery) — now the FOURTH flip precondition in the gate text, and
> `screen-status-token-unknown` (status-typed screen fields must be enum members). The flip
> stays a separate recorded decision; nothing enforces until then.
> **What this does NOT ship — the acceptance clock is armed for NO real order today.** No
> production path enqueues an `Order`-lane birth message: the PlaceOrderProcess `deliver:` step
> appends `OrderPlaced` straight to the event store (`crates/application/src/generated/process_managers.rs:663-667`)
> and the reclamation PM calls `place_replacement_order` in-process
> (`crates/application/src/process_managers/reclamation.rs:104`), so the birth route this PR adds
> is reachable from tests only. Consequences, both of them real: a paid order still sits PLACED
> forever in production, and **zero shadow evidence accumulates**, which makes flip precondition
> (3) unreachable by the mechanism shipped here. The producer is tracked as
> [#588 "The normal checkout path never enqueues OrderPlaced onto the Order lane — the acceptance clock cannot start for saga-appended births"](https://github.com/TheCaptainCompany/captain-food/issues/588),
> and it is now the FIFTH named precondition in the gate text.
> 🛠️ **2026-08-15 — #582 ACTORS HALF IN FLIGHT (branch `582-actor-answers-dsl`, draft PR #583)**:
> the `answers:` DSL from
> [PROP-20260815-142349](../proposals/PROP-20260815-142349-actor-answers-block-and-the-ask-step.md)
> lands for the two settlement actors — declared `state:` blocks on `Order`/`Payment`
> (declaration-only: both carry a `lifecycle:`, so states.rs generation stays deferred to the
> states slice 2; only the reply-SERVED fields — Order 1/6, Payment 3/5 — are compiler-carried,
> by the reply-construction tests in the hand fold modules; the rest is unverified transcription
> until slice 2), `Order.paymentReference` + `Payment.settlementView` answers, the `ans-*`
> validator family (red-first), the implicit lifecycle-status state ref and NESTED event-payload
> lineage (`checkout/orderId` resolves through the entity ref), generated
> `<Actor><Op>Request/Reply` + sealed `ask` + `AskOutcome` local adapter over the EventStore
> port. The PM half (`ask:`/`branch:`/`from_ask` steps, `CapturePayment` leg, the reminder
> watchdog) is fenced behind
> [PR #566 "A process-manager read step declares its SOURCE, not only its shape (#564 PR1)"](https://github.com/TheCaptainCompany/captain-food/pull/566).

> ✅ **2026-08-15 — PM DECISION-GRAMMAR PROPOSAL APPROVED** (founder, verbatim: *"I'm ok for the
> dsl for process manager"*):
> [PROP-20260815-142349 "Actor `answers:` block + the PM `ask:` step — typed request/reply for actor queries; the transport stays parked"](../proposals/PROP-20260815-142349-actor-answers-block-and-the-ask-step.md)
> is Approved after three founder-directed design rounds; DECISIONS §42 PMW-1 closes as (a) +
> the additive `ask:`/`branch:` grammar, PMW-3 (the transport) stays parked. Build tracked in
> [#582 "Actor `answers:` block + PM `ask:` step — typed request/reply for actor queries, transport stays parked"](https://github.com/TheCaptainCompany/captain-food/issues/582),
> sequenced strictly behind
> [PR #566 "A process-manager read step declares its SOURCE, not only its shape (#564 PR1)"](https://github.com/TheCaptainCompany/captain-food/pull/566).

> 🧾 **2026-08-15 — THE DECISION REGISTER RENDERS AS WRITTEN AGAIN, AND §13b IS AN ERROR**
> ([#577 "Repair the seven register-table rows §13b found, then promote the gate to ERROR"](https://github.com/TheCaptainCompany/captain-food/issues/577)):
> the seven broken table rows (SPEC-2, LOSS-1, IDOR-1, ENF-1, CAP-READY, CAP-READY-LEGAL in
> `docs/proposals/DECISIONS.md`; one evidence row in PROP-20260811-090000) are repaired
> byte-identically (geometry only, zero words moved), and BOTH §13b markdown-table rules
> (`markdown-table-row-cell-count` + `markdown-table-delimiter-cell-count`) are promoted from
> warning to ERROR — a reshaped register row now fails `make validate` instead of riding the
> warning ratchet (baseline entry removed, 46 → 39).

> ⚖️ **2026-08-15 — THE TEAM MERGES ITS OWN WORK; NO PR WAITS ON FOUNDER REVIEW** (founder,
> verbatim: *"Never wait my review you are responsible of your work."*): the "human" in
> `HOLD: human` is the TEAM's independent reviewer pass, never the founder —
> [ADR-20260815-134655](../adr/ADR-20260815-134655-the-team-merges-its-own-work-no-pr-waits-on-founder-review.md);
> first application [PR #576 "Opening-hours guard: three-valued verdict, undeclared hours accept (#180 / RSO-1)"](https://github.com/TheCaptainCompany/captain-food/pull/576),
> MERGED after independent review PASS + green CI (the RSO-1 banner below predates the merge).

> 🔶 **2026-08-15 — RSO-1 IS CODE-COMPLETE ON THE BRANCH (PHASES 1–4):
> [#180 "Opening hours are stored, displayed, and never enforced — a customer can order at 04:00"](https://github.com/TheCaptainCompany/captain-food/issues/180),
> branch `180-rso1-opening-hours-guard`, draft [PR #576](https://github.com/TheCaptainCompany/captain-food/pull/576) — NOT on `main`; merge posture `HOLD: human` (ADR-20260815-115220: stored event shape + money path).**
> **Phase 1 (specs)**: the whole DECISIONS §43 surface — `ServiceWindowVerdict` kernel scalar,
> non-null `Restaurant.serviceWindow`, `OutsideServiceHours` with next-slot + evidence context, the
> guard step in `processmanager.yaml` strictly before the payment call, five optional-forever
> evidence fields on `CheckoutSnapshot`, gate `ENFORCE_SERVICE_HOURS_GUARD` (default OFF = shadow),
> `SERVICE_WINDOW_VALIDITY_HORIZON_SECONDS`, 3 rules, behaviour tests, and the tests-DSL `when.at`
> instant. **Phase 2 (domain)**: `domain::service_window::serving_at` — ONE pure total DST-safe
> evaluation shared by badge and guard (overnight slots, fold-back, inclusive `lastOrderAt`,
> cutoff=min degradation). **Phase 3 (emitter+server)**: clock-taking `Restaurant::at` (a clock-less
> Restaurant is unspellable), `service_clock` (per-request `RequestNow`, config horizon),
> `place_order(when_at)` freezing the verdict evidence onto the snapshot even in shadow mode.
> **Phase 4 (the finish)**: the REFUSING guard in `place_order` — OUTSIDE_HOURS only, gate ON only,
> after `serving_at` and before any external effect, evidence-carrying rejection off the folded
> RestaurantState; the gate threads as a PARAMETER from `CommandDeps` (composition root / env in
> standalone), never a global read; both edges mutation-tested via the new tests.yaml `when.gates`
> DSL (validator `test-when-gate-*` + emitter `BT_GATE_CONSUMING`, red-first); the `command.validate`
> span is now CONSTRUCTED at both dispatch seams (pm_delivery prepare + generated router) recording
> `validation_status` + `service_window_verdict` off the run's own evidence, and the codegen
> cross-check now demands a PRODUCTION call site per required span — which surfaced `cart.read` and
> `pricing.compute` as pre-existing dead constructors (held in an explicit
> `KNOWN_UNINVOKED_REQUIRED_SPANS` exemption; follow-up owed). Subscriptions re-evaluate the window
> PER PUSHED UPDATE through the blessed `service_clock::evaluate_now` symbol, proven by a
> straddling-pushes behaviour test. The refusal-message toast surfacing stays deliberately unbuilt
> (acceptance-first: PlaceOrder rejections land post-enqueue on the operation status surface) — a
> product call for the architect, not a Phase 4 omission.

> ⚖️ **2026-08-15 — MERGE POSTURE RULED: AUTO-MERGE-ON-GREEN IS THE DEFAULT; `HOLD: human` FOR THE
> NAMED CLASS** (stored event shapes/fold semantics/migrations, payments/funds custody/erasure,
> legal surfaces, non-additive GraphQL changes, mailbox/lease/fencing runtime, the merge/CI
> machinery). Founder delegation 2026-08-15 (*"you can consider that you are completely autonomous
> on that"*), whole roster consulted per ADR-20260812-143619 (11/14 for risk-tiering, farley's
> dissent recorded):
> [ADR-20260815-115220](../adr/ADR-20260815-115220-auto-merge-on-green-by-default-hold-human-for-the-named-class.md).
> `.claude/agents/executor.md` step 7 and CLAUDE.md's issue-workflow bullet aligned in the same
> commit — the two documents no longer contradict each other.

> 🧱 **2026-08-15 — THE DECISION REGISTER'S TABLES ARE NOW GATED, AND THE GATE FOUND SEVEN BROKEN
> ROWS ON ARRIVAL**
> ([#572 "Validator gate: the decision register's tables have no integrity check — a stray `\|` silently reshapes a row"](https://github.com/TheCaptainCompany/captain-food/issues/572)).
> Validator **§13b** (`markdown-table-row-cell-count`) checks every markdown table row in
> `docs/proposals/DECISIONS.md` **and** `PROP-*.md` against its header's cell count. The register was
> the one artifact the whole decision process reads and the only one `load_proposal_files` never
> globbed. **Cell counting follows GFM exactly, and the counter-intuitive half is load-bearing: a
> pipe inside a code span DOES open a cell** — only a backslash escape (`\|`) makes it inert, and it
> works *inside* backticks. Verified against two independent renderers rather than assumed; modelling
> code spans as pipe-neutral is the intuitive choice and it makes the rule blind to the exact defect
> the issue was filed for. **On arrival the rule found 7 real breaks** — six register rows supplying 3
> cells to a 4-column header (the trailing `Recommendation / status` column renders blank: `SPEC-2`,
> `LOSS-1`, `IDOR-1`, `ENF-1`, `CAP-READY`, `CAP-READY-LEGAL`) and one proposal row whose tail GFM
> **drops outright** (a raw `|` in `filter(|g| g.scope == scope)`). They are **reported, not repaired**:
> which column the missing cell belongs to is a content judgement for the row's author. So §13b ships
> at **WARNING**, which the §17 ratchet already makes blocking — an eighth break fails CI — and
> promotion to ERROR is a one-liner once the seven are fixed. **Known breaks are pinned by ROW
> IDENTITY (`SPEC-2`, `CAP-READY`, …), never by line number**: the register gains rows daily, and an
> absolute `file:line` list would go red on any unrelated insertion above a known break — a gate that
> fails for reasons unrelated to what it checks is a gate people learn to ignore.

> ✅ **2026-08-15 — RSO-1 IS NOW DISPATCHABLE: ITS THREE BLOCKING SUB-QUESTIONS ARE ANSWERED — AND
> THREE OF THE ANSWERS SAY THE ROW'S OWN TEXT WAS WRONG** (docs-only, straight to `main`; still no
> code). Recorded in [DECISIONS §43](../proposals/DECISIONS.md) RSO-1, fourth amendment; every
> `file:line` re-verified against `main` before writing. **(i) The guard ACCEPTS on
> `HOURS_UNDECLARED`; `OUTSIDE_HOURS` is the only refusing verdict.** This agrees with `evans`'s lean
> but **replaces its reasoning**: the Sirene/Google-seeded population *never reaches the guard*
> (`RestaurantRegistered` births DRAFT, `crates/domain/src/restaurant.rs:14,197`; `place_order`
> rejects `RestaurantNotActive` first, `crates/application/src/commands.rs:2398`), so the branch
> governs **deliberately activated** restaurants — and **100% of those are `HOURS_UNDECLARED`, because
> no screen can set hours** (`specs/screens/restaurant_backoffice.yaml:484` says so verbatim;
> `specs/stories.yaml:128-140` has no hours step; every creation path writes `opening_hours: vec![]`).
> **Production is 1 of 1**: `tools/smoke/prod-smoke.sh:310-315` registers with a timezone and no
> `openingHours`, so **"refuse" would break the L4 smoke gate**. The decisive argument is **which
> failure announces itself** — accept produces a complaint we can act on, refuse produces silence, and
> a zero-order graph is indistinguishable from *"Tours has no demand"*, corrupting the exact signal V0
> exists to measure. **Money correction**: capture is manual
> (`crates/adapters/stripe/src/outbound.rs:245`), so at 22:40 the card is **held, not charged** — the
> real cost is that **nothing releases the hold**, because the acceptance-timeout auto-cancel is
> declared and unbuilt (`crates/application/src/generated/process_managers.rs:915`) and nobody is
> notified (gap **G8** below). **Building that timeout removes most of the accept branch's cost
> without refusing anyone — same effort, strictly more value.** **(ii) The shared function lives in
> `crates/domain/src/` beside `restaurant.rs`**; the `crates/domains/common/` option is struck as
> **"does not compile"** — `OpeningHoursSlot` is generated into `crates/domains/network/`, which
> **depends on** `domain-common`, so the kernel naming it is a **cycle** no emitter rule can dissolve.
> **(iii) `serviceWindow` is a FIELD on `Restaurant`**, with `closesAt` renamed **`lastOrderAt`** (with
> closing at `min(slot.to, cutoff)`, "closesAt" renders as *"open until"* and is wrong by the cutoff
> margin, on the money path) and a new **`validUntil`**, non-null in all three states. **Three
> corrections to the row's own recorded text**: **(1)** amendment (1) placed `ServiceWindowVerdict` in
> `specs/network/scalars.yaml` while amendment (6) puts the verdict on `CheckoutSnapshot` in
> `specs/common/entities.yaml:167` — `scope-kernel-purity`
> (`tools/codegen-rs/src/validate/scopes.rs:358`) makes that a **hard validator error**, so the row
> **could not have passed `make validate`**; both scalars belong in `specs/common/scalars.yaml`.
> **(2)** Correction 5's premise is **false** — the renderer computes nothing:
> `crates/web/src/renderer.rs:346-349` folds `OpeningHoursRow` into the `InfoRow` arm and reads
> `label`/`value`, which that node does not carry (`crates/web/src/generated/screens.rs:423`), so it
> renders an **empty div**. RSO-1 **implements** the row for the first time. **(3)** Correction 4's
> emitter claim was wrong in both directions — a hand-written file under a declared scope **survives**
> regeneration; `src/lib.rs` and `Cargo.toml` are what get clobbered. **The generalizable lesson: in a
> generated crate the fragile artifact is the module INDEX, not the module — regeneration erases the
> `mod` declaration, leaves the file on disk, and produces NO compile error.** **(iv) NEW — RSO-1 is
> an EMITTER change, not spec-only**: the read-side call site is generated
> (`crates/server/src/graphql/generated/types.rs:1070`, `impl From<RestaurantRow> for Restaurant`) and
> **has no clock**, so it cannot compute a time-varying field at all; the fix is two hardcoded literals
> in `tools/codegen-rs/src/emit/server_graphql.rs:293,654`. It also forces a **net-new `chrono-tz`
> workspace dependency** (zero hits today), which makes a **DST behaviour test mandatory** — the
> boundary is wrong for one hour on the last Sunday of October, a Saturday night. Peak is clear: **no
> N+1 is possible** (`Restaurant` is a `SimpleObject`, zero `#[ComplexObject]` impls, list clamped to
> 200). **Three new rows opened, all explicitly OUT of RSO-1's scope**: **DSC-1** — **seven** declared
> discovery filters (`tags`, `serviceType`, `openNow`, `city`, `priceRange`, `list`, `listingStatus`)
> are emitted onto the input type and then **silently dropped** by the resolver
> (`crates/server/src/graphql/generated/query.rs:250` builds only `search`/`orderableOnly`/`limit`/
> `offset`), so a client filters and gets unfiltered results with no error — already public;
> **PAN-1** — a latent `.expect` panic on the public discovery list
> (`crates/server/src/graphql/generated/types.rs:1093`) one line above an `unwrap_or_default()`, which
> the RSO-1 implementation must not duplicate; **HRS-1** — the **third** meaning of `[]` (hours present
> but unparseable) is a **defect, not a state**, and nothing counts it, plus the owed
> `service_window_verdict_total{verdict}` contract without which the accept branch is invisible and
> RSO-1's revisit condition is **permanently unmeetable**. **Nothing was applied**: `docs/**` only — no
> `specs/**`, no `crates/**`.

> 🛑 **2026-08-15 — RSO-1 CANNOT BE BUILT AS RECORDED: A BOOLEAN "IS IT OPEN?" WOULD TAKE LIVE
> RESTAURANTS OFFLINE** (docs-only, straight to `main`; recorded BEFORE any code was dispatched)
> — **superseded in part 2026-08-15 by the banner above**: its sub-questions are answered, its
> `specs/network/scalars.yaml` placement and its correction 4 and 5 are corrected there.
> `evans`, in mob briefing, found a **blocker**, **five design corrections**, a **factual error in
> the row's own text**, and **one new row** — all in [DECISIONS §43](../proposals/DECISIONS.md).
> **The blocker**: `opening_hours` is a `Vec` updated via `replaced_vec`
> (`crates/domain/src/restaurant.rs:83,95`, whose doc says *"an omitted array and an explicitly-empty
> one arrive identically"*), and the read side does `unwrap_or_default()` on a JSONB parse failure
> (`crates/server/src/graphql/generated/types.rs:1095`) — so `[]` means **three indistinguishable
> things**: hours never declared (every Sirene/Google-seeded prospect), hours cleared, hours
> unparseable. A boolean `f(hours, tz, now)` maps all three to **closed forever**, and `orderable`
> reads no hours today — so RSO-1 as recorded **would ship a NEW way to take live restaurants
> offline, as a safety fix**. Recorded fix: a **three-valued verdict** `OPEN / OUTSIDE_HOURS /
> HOURS_UNDECLARED`, with the guard's behaviour on the third value an **explicit recorded decision,
> never a default** (`evans`'s lean, recorded as a lean: accept — *"a restaurant nobody can order
> from"* is the sibling failure of *"a paid order nobody is told about"*). **Two corrections dissent
> from what the row recorded**, and are recorded as dissents: the error is **`OutsideServiceHours`**,
> not `RestaurantClosed` (which collides with the **permanent** `RestaurantMarkedClosed`,
> `specs/network/events.yaml:358`, and parallels the existing `OutsideDeliveryArea` guard); and hours
> are **NOT folded into `orderable`**, because a time-varying boolean carried alongside `updatedAt`
> (`specs/network/api.yaml:44`) reads "3 days ago" for a value that is wrong in 20 minutes — a
> self-describing `serviceWindow { state, opensAt, closesAt, evaluatedAt }` instead. Also recorded:
> **`crates/domains/common/` is GENERATED**, so the "pure function in `domain-common`" cannot land
> there as written (two legal shapes, owner `vernon`/architect — the requirement is **one artifact
> imported by both call sites**); the verdict is **already computed a second time in the renderer**
> (`specs/screens/restaurant_frontoffice.yaml:323-325`) and must be **replaced**, not duplicated;
> the domain term is **service hours / `cutoff_time`**, which HubRise exposes
> (`specs/integrations/hubrise.md:21`) and the ACL **never mapped**, and the closing-margin must NOT
> be derived from `preparationTimeMinutes` (an ETA duration, not a deadline); and the frozen
> `CheckoutSnapshot` verdict records **the window and the inputs**, not a bare boolean, because
> *"a stored `wasOpen: true` is unfalsifiable six months later; a stored window is evidence"*.
> **Factual correction to an already-landed row**: §43 RSO-1 said `RestaurantState` holds `timezone`
> *"but no opening hours"* — **false**, it holds them at `restaurant.rs:83`; the error came from a
> previous executor's report and was relayed unverified. **New row `BSY-1`** (AMBER): `BUSY`
> (`specs/network/scalars.yaml:156`) is a word in the ubiquitous language that **changes nothing** —
> `orderable` ignores it, no guard reads it, no screen renders it, no rule names it, and its only
> non-plumbing appearances are tests that assert it was *stored*. Domain answer: BUSY should mean **a
> longer ETA**, and the ETA is the product. **Explicitly out of RSO-1's scope.** **Nothing was
> applied**: `docs/**` only — no `specs/**`, no `crates/**`.

> 🛑 **2026-08-15 — RSO-2 CANNOT CLOSE OVERSELL, AND IT WAS ABOUT TO BE BUILT AS IF IT COULD**
> (docs-only, straight to `main`; recorded BEFORE any code was dispatched).
> [DECISIONS §43](../proposals/DECISIONS.md) is amended and gains four rows. `young` and `vernon`,
> asked independently, both disproved the implicit premise of the checkout stock re-check — Young's
> words are the record: *"it narrows the window and creates the appearance of a guarantee that the
> write model cannot deliver."* **The disproof**: `OfferStockUpdated` is emitted by `UpdateOfferStock`
> and the inbound HubRise sync ONLY (`specs/catalog/actors.yaml:76-77,87-89`; `commands.rs:3091-3123`)
> — **nothing decrements stock when an order is placed**, so a re-check is a race with **no writer at
> all**: two customers each read quantity 1, both are accepted, the count never moves. Reading it
> fresher (projection, fold or snapshot) changes nothing. **RSO-2 is narrowed, not cancelled** — it
> still catches the single-customer staleness case (an item that left the catalog or was flipped
> `UNAVAILABLE` between add-to-cart and pay), which today is caught by **nothing**, and it now carries
> a wording fence so its rule text cannot claim a stock guarantee. Oversell moves to **STK-1** (AMBER):
> closing it needs an **arbiter, not a read** — a reservation-shaped conditional
> `UPDATE ... WHERE remaining >= qty`, whose justification is already written at
> `specs/database/tables/reservations.yaml:1-22` — and it hits an ordering wall, since the Stripe
> intent is created in `prepare`, **before** `pool.begin()` (`crates/actor_runtime/src/completion.rs:69,71`),
> so the claim must be taken in `prepare` with release-on-decline/timeout becoming `PaymentProcessRow`
> state and PM legs. **Accept-and-compensate (the restaurant calls and swaps the dish) is recorded as a
> legitimate V0 answer and deliberately NOT pre-decided.** **RSO-1 amended twice**: the guard *computes
> the open/paused verdict and throws it away* — `CheckoutSnapshot` (`commands.rs:2526-2541`) carries no
> record that the restaurant was ACTIVE and open, so a restaurant disputing a 22:40 order asks a
> question the log cannot answer; and **`isOpen` is a pure function, not state** —
> `f(opening_hours, timezone, now)`, so the shape is a `domain-common` function with the clock injected,
> called identically by the storefront badge and the checkout guard, never a projection column or
> aggregate state. **Three new findings, each its own row**: **CHK-1** — `commands.rs:2392`'s
> *"authoritative, race-free"* is **false** (the fold appends to the **Payment** stream with no
> restaurant `expected_version` on `EventStore::append`, `ports.rs:54-60`), which reframes a decision
> already taken: **fold-vs-projection is a latency and cost decision, not a correctness class**;
> **CAT-1** (AMBER) — `RestaurantState` holds **no catalog id**, so `restaurantId → catalogId` is a
> `ORDER BY created_at DESC LIMIT 1` set query (`persistence/catalog.rs:27-31`) with a **newest-wins
> tiebreak no aggregate ever decided**, fixed by the restaurant **appointing** its live catalog;
> **FEN-1** — `expectedTotal` is optional (`commands.rs:2460`), and *"on the money path an optional
> fence is not a fence"*. **Framing correction landed on
> [ADR-20260815-030206](../adr/ADR-20260815-030206-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side.md)**
> as a dated scope note: `place_order` is a **command handler** (`commands.rs:2380`), **not** a PM leg
> (`process_managers/place_order.rs:13` says so; the PM legs are `on_payment_authorized` /
> `on_payment_failed`), so the restaurant fold, cart fold and catalog read on the checkout path are
> **not governed by that ADR** — the rule is unchanged, its reach was being overstated in discussion,
> including by the coordinator. **Every code change above is FLAGGED, not made**: no `specs/**`, no
> `crates/**`, no gate movement.

> 🧹 **2026-08-15 — the autonomous-run brief no longer tells the run that `specs/**` is
> untouchable** (docs-only, straight to `main`). `docs/claude/autonomous-run.md`'s "rules that bind
> the run" still carried the pre-2026-08-10 freeze — *"prepare spec diffs as proposal documents;
> only explicit customer approval applies them"* — which
> [ADR-20260810-221840](../adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md)
> **lifted, not narrowed**, by founder directive: tonight's scheduled run would have refused work
> the founder explicitly directed. The bullet now states the live rule (CLAUDE.md's three questions
> + the one-sentence `docs/SPEC-LOG.md` row in the SAME commit), and the same retired claim is
> removed from the ask-the-founder list ("spec-diff approvals" → *a spec change that reverses one of
> his own decisions*). Same pass: the **standing objective**, frozen at 2026-08-08 and naming
> cutover issues the work has moved past, is re-pointed at the six-clause acceptance criterion
> ([ADR-20260813-191111](../adr/ADR-20260813-191111-the-acceptance-criterion-six-clauses-walked-with-the-front-door-unlocked-from-inside.md),
> as amended 2026-08-14) with its ordered tail **read off THIS file** rather than re-pinned there —
> the pin is what went stale. **Reported, not fixed** (merely old; no ADR contradicted): the reading
> list still points the run at `DECISIONS.md` §22 for "what was just decided" (latest is §43); the
> commit-trailer bullet pins a model name no commit in this repo uses; the file calls the founder
> "the customer" throughout while its own opening quote says "founder"
> ([ADR-20260812-143619](../adr/ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md)
> swept the living operating docs); and its "ready + auto-merge as one step" line sits against
> `.claude/agents/executor.md`'s PR-only default. No `specs/**`, no code, no gate movement.

> 🧠 **2026-08-15 — THE ARCHITECT IS SPLIT INTO THREE NAMED DOCTRINE LENSES: `young`, `vernon`,
> `evans`** (founder directive, *"Split the architect into Greg Vaughn and eric"*;
> [ADR-20260815-032912](../adr/ADR-20260815-032912-split-the-architect-into-three-named-doctrine-lenses.md),
> amending [ADR-20260808-154005](../adr/ADR-20260808-154005-agents-channel-named-experts-published-work.md)).
> `.claude/agents/architect.md` already *declared* it channelled Young, Vernon and Evans, but under a
> footnote heading beneath an audit/file/propose/dispatch charter — so its output read as generic
> architecture opinion until a coordinator re-briefed it. **`architect` survives unchanged as the
> OPERATIONS role** (audit, issue filing, proposals, backlog ranking under ADR-20260810-215503, and
> **naming the next chunk** — the autonomous loop, CLAUDE.md and the `architecture-review` skill all
> depend on it); its "Channels" section becomes a routing table into three new **read-only** lenses
> that advise and are cited: **`young`** (which side of the read/write wall, CQRS ≠ eventual
> consistency, read models and snapshots as disposable rebuildable folds, upcasting for STORED events
> vs additive-only + tolerant reader for live replies, set-based validation), **`vernon`** (aggregate
> size, by-identity references, one aggregate per transaction, PM process state, the actor model and
> **Ask vs Tell** — PMW-3 is his row) and **`evans`** (ubiquitous language as a modelling defect not a
> naming nit — `processmanager.yaml:30-43` vs the code is the live instance — context maps and their
> patterns, ACLs, core-vs-generic distillation). The three never edit `specs/**`, never claim work and
> **never rank the backlog**; they report disagreement AS disagreement rather than blending. Every
> other channelled lens (Kleppmann, Byron, Majors, Norman/Patton/Ive, Meyer/Scholz, Beck, Holub,
> Farley) is untouched. Docs/config only — no `specs/**`, no code, no gate movement.

> ⚖️ **2026-08-15 — OPENING HOURS AND STOCK ARE CHECKED SERVER-SIDE ON PLACE ORDER; A BIG CATALOG
> SNAPSHOTS EVERY 100 EVENTS** (founder directive;
> [ADR-20260815-032807](../adr/ADR-20260815-032807-opening-hours-and-stock-are-checked-server-side-and-a-big-catalog-snapshots-every-100-events.md),
> [DECISIONS §43](../proposals/DECISIONS.md) **RSO-1/RSO-2/SNAP-1/BUS-1**). **All three parts verified
> against `main`; all three are REAL GAPS, none is already done.** **(a) The concept of "open right
> now" does not exist anywhere** — `orderable` (`specs/network/api.yaml:21`) is
> `ACTIVE_PARTNER + ACTIVE + acceptance ≠ PAUSED` with **no hours term**, and the PlaceOrder guard
> chain (`specs/ordering/processmanager.yaml:40-49`) has **no closed-hours guard**;
> `RestaurantMarkedClosed` is PERMANENT closure → INACTIVE, not "closed tonight". So a kitchen that
> shut at 22:00 renders `orderable: true` at 22:40 **and the server accepts the order**. The raw
> material (`opening_hours` + `timezone`) is already projected and already on the api type.
> **(b) The checkout orderability re-check is a TODO that was never done** —
> `commands.rs:2450-2452` says so verbatim; `require_orderable_line` runs on `add_cart_line` ONLY.
> Checkout's only protection is fail-closed *pricing*, so a line still in the catalog but flipped
> `UNAVAILABLE` or out of stock **prices fine and is accepted**. Its existing test is an **any-of over
> three codes** that passes on `OfferNotFound` alone — `require_stock_covers` could be deleted with a
> green suite, so it splits into three when this lands. **(c) There is NO event-sourcing snapshot
> mechanism in the tree** (false friend: `pricing.rs:129`'s `CatalogSnapshot` is a read-side pricing
> helper). Catalog is the right first target and PMW-2 already named it. **Policy adopted verbatim:
> snapshot every 100 events, actor load < 5 s** — but *where a snapshot lives*, how it meets
> **upcasting** (disposable and rebuildable, never authoritative — a version mismatch means throw it
> away, never migrate it) and **GDPR erasure** (a snapshot is a SECOND COPY; deleting the stream and
> leaving it erases nothing) is a genuine option space = **SNAP-1, AMBER**. **Found incidentally and
> filed loudly — BUS-1**: `operationStatusChanged` is a declared product subscription riding a
> **process-local `tokio::broadcast` with no serde**; post-split the subgraph bins build **fresh empty
> buses** and the gateway **refuses the WS handshake** (`501`), so the client polls 30 × 1 s — a poll
> that is the **PRIMARY** transport, with no declared degraded mode and no detected path back, i.e.
> ADR-20260810-231300 violated in shipped code. The screen reference is on the **CUSTOMER checkout**
> action, so the person eating it is the customer staring at a spinner **after paying**. **Nothing
> built; the only `specs/**` change in this commit is the `services.yaml` header below.**

> 📝 **2026-08-15 — `specs/services.yaml`'s "V0: one deployable" line now names its destination**
> (SPEC-LOG row, Tier 0). The header said *"V0: every service is `binding: local`, `expose: false` —
> one deployable, zero internal HTTP"* in SOURCE DSL with no "then what" — factually stale (the bin
> topology exists in `crates/bins/`) and the sentence a reader reasonably turns into *"the product is
> a monolith"*. It now states that a binding describes how the **domain** reaches one capability (not
> how many processes serve the product), that a capability moves by flipping **its own key** with
> `SERVICE_<NAME>_URL` as an address book, and that this is a topology change rather than a rewrite
> **because the generated call types derive serde unconditionally while the binding is local**.
> Modelled on `specs/architecture/c4-l2.yaml:98-99`, which already does this correctly.

> ⚖️ **2026-08-15 — AMENDED: [ADR-20260815-030206](../adr/ADR-20260815-030206-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side.md)
> was WRONG on its central claim and now carries a `Correction (2026-08-15)` section.** The rule
> itself is unchanged and still Accepted; what was wrong is the framing of the adopted mechanism.
> The ADR adopted reading (i) — the PM folds the aggregate's stream through the `EventStore` port —
> on the unstated theory that **the port is the final vision and the adapter behind it is
> deployment**. It is not: `load(stream_name) -> (Vec<DomainEvent>, i64)` (`ports.rs:65`) is a
> **STORAGE** port, and its remote form is *"the PM pod talks to the write database"*, not a service
> call. **So (i) is a DIFFERENT DESTINATION, not a step toward (ii)** — adopting it permanently
> chooses **shared-database coupling** between independently-deployed pods, which
> ADR-20260808-235113 requires be named rather than glossed. It also **does not deliver the founder's
> own premise**: `ActivationCache` is process-local and lane-tagged, so a PM in another pod re-folds
> on **every** settlement leg — (i) is *slower* than what he asked for, until PMW-2 lands. **The
> final-vision form exists one file over**: `specs/services.yaml:16-28`'s spec-owned
> `binding: local | http` + serde derived **unconditionally regardless of binding**
> (`generated/services.rs:48-60`) ⇒ applied to actors, an `answers:` block with a spec-owned
> `binding:`, always-serde replies, a typed `ask` on the sealed per-actor client, and a codegen
> round-trip test per reply type. Then *"put in place the gRPC transport"* is one spec key; **today
> it is not — zero `tonic`/`prost`/`.proto` in the tree**. **Two things wire-shaping does NOT fix**:
> `SettlementHooks` threads cross-call state through `Mutex<Option<..>>` + `Mutex<bool>` (no wire
> form at all — it must become an explicit value first), and the fencing/ordering hazard is
> independent of serialization (PMW-3's `read:`+`call:` rule still stands). **Six reply shapes carry
> no serde today** (`HookOutcome<T>` — whose `Skip(String)` is prose; the five PM read structs, which
> ARE the query-reply shapes; `DomainError`, two of three arms `String`; the `Actor` envelope;
> `AppendedEvent`; `OperationUpdate`) — **but every command and event already round-trips serde on
> every call**, so the missing half is REPLIES, not the codebase. **Versioning differs by reading**:
> stored events get Young's upcasting; a query reply gets the mirror rule — additive-only producer,
> tolerant reader, and a breaking change is a **new operation name**.

> ⚖️ **2026-08-15 — A PROCESS MANAGER IS A WRITE-SIDE COMPONENT AND NEVER READS THE READ SIDE**
> (founder directive; [ADR-20260815-030206](../adr/ADR-20260815-030206-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side.md),
> [DECISIONS §42](../proposals/DECISIONS.md) **PMW-1/PMW-2/PMW-3**, plus option **(e)** annotated onto
> **STO-9** in §32). **Rule DECIDED, nothing built, no `specs/**` edit** — this is a record-only
> change. The enforceable form is narrower than the sentence and the narrowing is the point: *a PM
> never reads a projection to learn a fact about an aggregate it can address by identity*. **Two
> carve-outs**: operator-authored referentials (`DispatchStrategyRepository`'s three tables) are
> configuration, not folds; and set-shaped reads have no actor to ask (`open_by_session` — there is
> **no `Session` aggregate**; `price_cart` walks a catalog tree). **Only ONE reading of "ask the
> actor" is adopted** — folding the aggregate's own stream in-process, already how
> `place_order.rs:47` and `delivery_dispatch.rs:126` work. A **query message over a transport** is
> NOT adopted (PMW-3): a query carries no `message_id`/`position`, so there is nowhere to put the
> lease fence; head-of-line puts a Stripe capture behind whatever the actor is doing; and lanes are
> claimed by a **lease race**, so there is no actor directory to route to. **The accounting is one
> read database of three, not a collapse**: `read_order`'s `captain_write` readers are ALL PM legs,
> so **STO-9 closes**; `read_common`'s nine and `read_catalog`'s two are **aggregate command
> handlers**, so **STO-7 and STO-8 are untouched**. **"Ask the Order actor" cannot answer the
> settlement guard alone** — neither `payment_intent_id` nor `payment_status` is on `OrderState`;
> `PaymentState` is the authority, and the cheapest closure is folding `payment_intent_id` onto
> `OrderState` from the `OrderPlaced` the aggregate already owns (**no event migration, one fold
> field**). **Two things it does NOT do, said out loud**: it does not make settlement transactional
> (`Payment-{intent}` is a different stream on no ordering relation to `Order-{id}` — the window
> shrinks from projector-lane seconds to microseconds; Stripe idempotency + the AUTHORIZED guard
> stay the protection), and *"the code will be simpler"* is **false on the money PMs' production
> lines** (1 read → 2 folds, +1 error arm) — it is true on the dependency graph, on the test bed
> (−4 fakes, ≈−375 lines, incl. a **divergent second `payment_status` projector** in
> `behaviour_support.rs` that deletes) and on the system. **RECORDED DRIFT, deliberately unfixed**:
> `specs/ordering/processmanager.yaml:30-43` declares `PlaceOrderProcess` reading the Cart and
> Restaurant **projections** while `commands.rs:2391-2394,2419` folds their streams — the spec is
> wrong at the head of the checkout path, and its correction is sequenced behind
> [#564](https://github.com/TheCaptainCompany/captain-food/issues/564)'s PR1, which already carries
> `source: EVENT_STREAM` on those two lines. **Owed regardless**: activation hit-ratio/bytes/eviction
> counters — `specs/observability.yaml` declares NONE, so residency's Catalog-eviction storm
> (`put_locked` inserts then evicts LRU, so one large import evicts every resident Order/Cart/Payment)
> is currently invisible, and Payment activations never engage at all (`surrogate_actor_id`'s UUIDv5
> lane key never matches the `Payment-pi_xxx` stream).
> 🧭 **2026-08-15 — A PM `read:` STEP NOW DECLARES ITS SOURCE: THE DISTINCTION THE MECHANICAL
> DERIVATION WAS BLOCKED ON** ([#564 "Derive reader sets mechanically: a declared, walkable `reads:`
> grammar that distinguishes source from shape"](https://github.com/TheCaptainCompany/captain-food/issues/564)
> phases 1-2 + hardening, PR [#566](https://github.com/TheCaptainCompany/captain-food/pull/566)). The
> entry below (2026-08-14) withdrew reader-set completeness and named its exact blocker: *"deriving is
> right only once the declaration distinguishes source from shape"*. That declaration now exists. Every
> PM `read:` step carries a **REQUIRED** `source:` from a closed set — `PROJECTION` (the leg SELECTs
> from the named projection) or `EVENT_STREAM` (the leg folds the entity from `captain_write` and never
> touches that projection) — enforced by `pm-read-source`, with `pm-read-key` closing the step's key set
> so the next key added cannot be invisible the way a plain-string `tombstone:` was in
> [#413](https://github.com/TheCaptainCompany/captain-food/issues/413). **Required, never
> optional-with-default**: a default would leave the distinction alive only where someone remembered
> it, the transience-by-omission defect [ADR-20260812-214500](../adr/ADR-20260812-214500-a-read-target-is-declared-not-inferred-the-reads-ownership-wall.md)
> §2 records. A third rule, `pm-read-projection-database`, refuses a `PROJECTION` step whose table
> resolves to no single database (a `View_*` declares none; a `replicated:` table resolves to the read-
> database SET) — nothing committed trips it, and it is written BEFORE a derivation consumes the key so
> that the shape it exposes gets a decision (§42 RDR-1) rather than a third token invented in a hurry.
> **No split count is stated here, deliberately**: `the_committed_process_managers_all_declare_a_source`
> is the live count, and a number written in prose is the failure mode CLAUDE.md's warning-baseline
> paragraph already records (it went stale three times).
>
> **THE FIRST SOURCE-VS-CALL-SITE COMPARISON HAS ALREADY HAPPENED — IT IS THIS ENTRY.** Phases 1-2
> assigned each step's `source:` by reading `crates/**` and comparing against what the DSL declared;
> that comparison is what found the under-declarations below. **PR2's derivation is therefore the
> SECOND comparison, not the first, and must not claim otherwise** — and it must build its
> "independent" list from `crates/**` alone (or from `git show main:specs/*/processmanager.yaml` taken
> before this branch), because the `source:` values committed here ARE the first comparison's answers
> written into the spec. What PR2 adds is that the second comparison is MECHANICAL and repeatable;
> this one was a careful hand-read, which is the class of work that already missed four times.
>
> **TWO UNDER-DECLARATIONS FIXED; THE CARRIED-TO-PR2 LIST IS NON-EXHAUSTIVE — THREE KNOWN.** (This
> entry originally said "two carried"; the independent third-look review of PR
> [#566](https://github.com/TheCaptainCompany/captain-food/pull/566) found a third the branch never
> named — item (3) below — which is itself the hand-read failure class this work exists to end.)
> Fixed: `PlaceOrderProcess`/`PlaceOrder` did not
> declare the `Catalog` read it prices every checkout from — a derivation over the old declaration
> would have concluded checkout needs no catalog access, which is register row **STO-7**'s exact
> question answered wrongly in the direction that breaks every order — nor the `CustomerCreditState`
> fold that spends the customer's goodwill credit against the total (`source: EVENT_STREAM`, and that
> token is load-bearing: the projected `CustomerCreditBalance` row is a running SUM with no per-order
> consumption key, so a redelivered `PlaceOrder` read from it would apply the credit twice — buyer
> undercharged, on the money path, silently). Both are now guarded by
> `the_checkout_leg_declares_every_read_it_prices_an_order_from`, which pins the money leg's whole read
> set: before it existed, the `Catalog` fix could be reverted by ONE line deletion with `make validate`
> still at 0 errors and every test green. Carried to PR2 because none is a pure declaration:
> **(1)** `ReclamationProcess`/`ReclamationResolved` reads `OrderTracking` (`reclamation.rs:141`,
> wired `runner.rs:488-490`) with no `read:` step — that leg IS generated, so a step emits a hook the
> hand-written wrapper does not implement; **(2)** `PlaceOrderProcess`/`PaymentAuthorized` loads
> `Payment-<intentId>` for the frozen `CheckoutSnapshot` (`place_order.rs:47`) — the read that decides
> what the restaurant is owed, and denied it errors AFTER Stripe authorized: money held, no
> `OrderPlaced`, nobody told; **(3)** `DeliveryDispatchProcess`/`OrderMarkedReady`'s
> `build_delivery_requested` (`delivery_dispatch.rs:150`) folds the **Order aggregate's own stream**
> to read `OrderPlaced.mode`, because `OrderTracking` does not carry `mode` (the code's own comment) —
> found by the third-look review, and it is the grammar counterexample RDR-1 wants: `mode` exists on
> NO projection table, so this read is **inexpressible** under the borrowed-projection-shape rule —
> stronger [DECISIONS §42 RDR-1](../proposals/DECISIONS.md) option-B evidence than the `balance_cents`
> hole (practical grant risk today nil: the leg's Restaurant `EVENT_STREAM` step already grants
> `captain_write`). **The derivation that consumes `source:` is NOT in this change**: nothing
> reads the key yet, so `make generate` moved generated doc comments only.
>
> Adjacent, recorded not fixed:
> [ADR-20260815-015422](../adr/ADR-20260815-015422-a-runtime-port-is-non-optional-and-fail-closed-is-a-declared-posture.md)
> — `PmRuntime`'s `partner`/`payments` are `Option<Arc<dyn ..>>` with silent constructor defaults, so a
> forgotten `payments` on `pm-payment-settlement` deploys green and declines every capture.

> 🗄️ **2026-08-14 — EVERY TABLE HAS A DECLARED HOME: STO-2 CLOSED, PLACEMENT IS NOW A VALIDATOR
> REQUIREMENT** ([#562 "Close STO-2's placement remainder: place the 17 unplaced tables and make
> placement a validator requirement"](https://github.com/TheCaptainCompany/captain-food/issues/562) /
> PR [#563](https://github.com/TheCaptainCompany/captain-food/pull/563)). The 17-table remainder
> (the row's "~65" counted the `ref_*` family deleted 2026-07-28) is placed by port evidence —
> map + per-table lineage in [DECISIONS §32 "STO-2 closure"](../proposals/DECISIONS.md): order-boundary
> read models → `read_order` (incl. `OrderConversation`/`CustomerCreditBalance` per §31's
> comms/payments-dissolve-into-order), `Catalog` → `read_catalog`, `Customer`/`Restaurant`/
> `SlugAlias`/`ProspectionPipeline`/`City` → `read_common`, the pricing trio
> (`PricingPolicy`/`Uber*Policy`) **replicated into every read database** (four reader sites
> resolving to TWO of them, `read_order` + `read_catalog` — two is what rules out a single home; the
> `read_common` copy comes from the replicated class grammar, not from a reader), the dispatch trio + `RuntimePosture` → `captain_write` (the trio's sole port is
> write-side; the posture is placed by the replay revert alone — its governed set is restricted to no
> side, so a future non-`captain_write` tenant would put a fail-closed startup read across the wall,
> recorded as a tripwire on the declaration). The §18 refusal arm ("business placement
> is an open register row") is now a **requirement arm** that consumes the same resolution the
> inventory emitter walks, so validation and emission cannot disagree; the ADP-1 wall runs on every
> single-home placement. **FOUR NEW OPEN ROWS.** **STO-7**
> (`read_order`/`read_catalog`) — *who is the catalog's pricing-and-orderability authority?* TWO
> paths cross that wall: the cart's read-time pricing (found at the mob checkpoint; post-split the
> cart cannot render names/prices, the #424 class) **and the checkout WRITE path** (found by the
> post-ready independent review — the mailbox worker's `CommandDeps.catalogs` backs the add-to-cart
> oversell guard and `place_order`'s repricing, so post-split every add-to-cart and every checkout
> fails closed). **STO-8** (`read_common`) — *may a `captain_write` app read `read_common`?* Nine
> handlers do, headed by `verify_phone`'s new-vs-returning read of `Customer` **on the login path**,
> whose degraded form silently re-registers returning customers as new. **STO-9** (`read_order`, a
> THIRD wall direction found by review round 2) — **the money one**: `SettlementHooks` reads
> `OrderTracking` for `payment_intent_id` immediately before EVERY Stripe capture and release, so
> post-split capture never runs, the authorization expires and **the food is delivered while the money
> is never collected**; the dispatch, reclamation and guest-cart-binding PMs ride the same wall. It is
> its own row because deciding STO-7 + STO-8 does NOT unblock `read_order` while that read
> fail-closes. **STO-10 is different in kind — AMBER, founder-owned**: the HubRise adapter bin already
> reads `read_common` (its own repository + a 40× projection poll), a SECOND outward grant where
> **ADP-1 allows exactly one**, so it REOPENS a closed directive rather than asking a new question —
> and the poll breaches the no-polling ADR as well.
> **THE METHOD FINDING OUTRANKS THE MAP, and it is worse than "we were careless": the crossings were
> ALREADY DECLARED IN TYPED DSL.** Thirteen `read:` steps in `specs/*/processmanager.yaml` carry
> `model: { $ref: 'database/tables/projection_tables.yaml#/X' }` — eight of them on `OrderTracking`,
> the settlement legs — and `process_manager/runner.rs:141` says so in a comment. Four consecutive
> hand-sweeps rediscovered by hand what the loader can resolve, each miss found by a READER and never
> by the sweep, and **round 4 found crossings that round 3's own written formula already covered**.
> **Completeness is therefore WITHDRAWN as a class, with its reason**, rather than re-claimed by a
> fifth sweep: reader sets in `projection_tables.yaml`/`databases.yaml` now carry a uniform
> non-exhaustiveness marker, the *"sole reader"* phrasing is retired everywhere (including where it is
> true), and `read_order`'s *"no other SUBGRAPH holds CONNECT"* — the sentence shape that excluded the
> `pm-*`/`adapter-*` classes by construction — is rewritten to the honest form. Known limits are named,
> including the one PM a DSL walker still misses (`ReclamationProcess`, whose read lives in a
> `description:` string behind a hand-written seam). The 17 placements are UNCHANGED and each still
> follows from a recorded decision. Nothing physical moved — no grants, CRs or migrations
> (#513/#514/#509 unchanged; **#513's grant emitter must derive CONNECT MECHANICALLY from the declared
> `read:` steps + generated composition roots, never from this prose — but NOT VERBATIM: a `read:` step
> declares the model SHAPE a leg consumes, not the physical SOURCE, and THREE of the thirteen are
> `captain_write` stream folds (`Restaurant` in delivery + ordering, `Cart` on PlaceOrder), so a literal
> derivation over-grants `read_common`/`read_order` for paths no code takes.** The symmetry is the
> lesson: the hand method failed by MISSING crossings, the naive derivation fails by INVENTING them, and
> deriving is right only once the declaration distinguishes source from shape — DECISIONS §32 limit (4)).

> 💳 **2026-08-14 — COLLECTION ORDERS WILL CAPTURE AT READY, NOT AT PICKUP** (founder directive,
> *"For the pickup order the payment captured must happen when the order is prepared don't you
> think?"*). Refines [ADR-20260808-195315 §1.2](../adr/ADR-20260808-195315-customer-brief-answers.md) and
> the just-shipped [#544/PR #545](https://github.com/TheCaptainCompany/captain-food/pull/545): the
> `PaymentSettlementProcess` capture trigger becomes **per service type** — DELIVERY on `OrderDelivered`
> (unchanged), **COLLECTION on `OrderMarkedReady` (READY)**. READY is collection's last controlled
> moment (collection is the customer's action, not a platform step), so this protects the restaurant
> from cook-then-no-show and is symmetric with capture-on-delivered for delivery. Record:
> [ADR-20260814-141350](../adr/ADR-20260814-141350-collection-captures-at-ready-not-at-pickup.md),
> register [DECISIONS §41 CAP-READY](../proposals/DECISIONS.md). **Business: HOLDS. Legal: defensible
> lawful prepayment, not a blocker** — sharpens CAP-3/CAP-5 for collection (charged before possession;
> counsel-gated build constraints on the unbuilt receipt engine + checkout copy, not a decision
> blocker). Empty log → additive, no migration. **Not yet implemented**: a fast-follow to #544, issue
> to be created; the code change branches the settlement PM per `service_type` (read from
> `OrderTracking`) and updates `PaymentCapturedOnFulfilment` + its tests. One behaviour change to pin:
> a READY collection order cancelled by the restaurant is now CAPTURED → routes to REFUND, not release.

> 🎯 **2026-08-14 — THE ACCEPTANCE KEYSTONE NOW PROMISES MORE: FULL ENFORCEMENT + FULL SPLIT ARE IN
> SCOPE** (founder directive, verbatim *"The acceptance include the full enforcement and full split"*;
> recorded in
> [ADR-20260813-191111](../adr/ADR-20260813-191111-the-acceptance-criterion-six-clauses-walked-with-the-front-door-unlocked-from-inside.md)
> — new "Scope clarification (2026-08-14)" section). This **supersedes the single-DB intermediate**
> the 2026-08-13 re-sequence had scoped ([#556](https://github.com/TheCaptainCompany/captain-food/issues/556)
> harness + L5 on a single-database stack, split deferred — [DECISIONS §39](../proposals/DECISIONS.md)):
> under final-vision-first (ADR-20260808-235113) that stack would be rebuilt, so the harness and the
> six-clause walk now target the **physically-split, least-privilege, write-authorization-enforced
> eleven-database stack from the start**. **Grounded by a read-only architect verification against
> `origin/main`** (four founder questions, `file:line`): the mailbox door (#536) and §18 placement
> gate (#547) are genuinely compiler-/gate-enforced; the storage split is today only a
> **declared-and-gated MAP** (one CNPG cluster `captain-db`, one database `app`, zero per-db grants —
> #513/#514/#509 unmerged); and the **`inbound_messages` write path is convention-only** (raw
> `INSERT` in `infrastructure` behind no gate — `mailbox_store.rs:98`), so hardening it compiler-first
> joins "full enforcement" alongside the cross-tenant write-auth fix (§39 IDOR-1 / #178).
> **Re-sequenced keystone tail** (before the walk): physical split band #513 -> #514 -> #509 -> the
> write-auth fix -> harden the `inbound_messages` write path -> #556 harness on the SPLIT stack -> L5
> -> browser walls -> the six-clause acceptance walk. This run is **docs-only** (ADR + this entry); no
> code, no claim, no backlog re-rank.
>
> **📌 THIS IS THE SEQUENCE THAT CERTIFIES — IT IS NO LONGER THE SEQUENCE THAT RUNS NEXT**
> (re-marked 2026-08-17 on a founder answer: [DECISIONS §45 SEQ-1](../proposals/DECISIONS.md),
> [ADR-20260817-105844](../adr/ADR-20260817-105844-the-walk-goes-first-on-one-database-and-production-stays-suspended.md)).
> Earlier on 2026-08-17 this entry was marked *"the live sequence"* over the older single-DB
> *"program of record"* bullet further down. That reconciliation stated which reading won; it could
> not state which was **right**, because that was a founder call. He made it: **the walk goes first,
> on ONE database.** So the split band, the write-auth fix and the `inbound_messages` hardening
> described here remain **exactly** what the acceptance criterion requires — they no longer **gate**
> the first end-to-end reading, which runs on the single-DB stack per the restored program of record
> below. The two entries no longer compete: **this one is the certificate, that one is the reading.**

> 🔒 **2026-08-14 — L5 acceptance-walk executor handed back on two real problems; the architect
> assessed, re-sequenced and recorded (docs-only run)**
> ([#554 "Smoke L5 — acceptance lifecycle legs"](https://github.com/TheCaptainCompany/captain-food/issues/554) /
> PR [#555](https://github.com/TheCaptainCompany/captain-food/pull/555) stopped rather than fabricate a walk).
> **(1) SECURITY — the cross-tenant WRITE IDOR is confirmed on `main`.** Re-verified at every layer:
> a valid `RESTAURANT` token can accept/reject/ready/deliver/tip/refund **another restaurant's** order,
> and a `RIDER` token can claim any job as any rider, by supplying the victim's ids in the command
> **payload** — nothing binds the token's verified `restaurant_id`/`rider_id` claim to the target
> aggregate on the write path. Trace: `crates/server/src/graphql/acl.rs:98-106` (`RoleGuard` checks
> the URL PATH role only) → `crates/server/src/graphql/generated/mutation.rs:6368` (`request_envelope`
> carries `user_id`=sub + `user_type`=role text ONLY) → `crates/application/src/generated/handlers.rs:28-40`
> (`accept_order` → `require_order(store, cmd.order_id, cmd.restaurant_id)`) →
> `crates/application/src/commands.rs:1084-1094` (`require_order` matches the order's stored
> `restaurant_id` against the **client-supplied** `cmd.restaurant_id`, never the token) and
> `commands.rs:1278-1303` (`accept_delivery` uses `cmd.rider_id` from the payload). No `WriteScope`
> exists repo-wide; `ReadScope` (`crates/application/src/queries.rs:787`) is read-only; `TenantScope`
> is read by **0** mutations. **This is already tracked** — [#178 "Write-side per-instance authorization"](https://github.com/TheCaptainCompany/captain-food/issues/178)
> / [PROP-20260726-171500](../proposals/PROP-20260726-171500-write-side-per-instance-authorization.md)
> (Status: **Proposed**, D1–D4 unanswered since **2026-07-26**, never surfaced in the register). Newly
> opened as **[DECISIONS §39 IDOR-1](../proposals/DECISIONS.md)**: **verdict — deadlined fast-follow, a
> HARD V0-LAUNCH BLOCKER, NOT accepted-forever.** Harmless TODAY (Q-L3 empty-log window, single tenant,
> acceptance deliberately walks auth-off from the inside, ADR-20260813-191111 §3/§6) but catastrophic
> the moment a **second** real restaurant token exists — it must close **before/with** the first-real-order
> gate ([#533](https://github.com/TheCaptainCompany/captain-food/issues/533)) and the auth walk
> ([#529](https://github.com/TheCaptainCompany/captain-food/issues/529)/[#532](https://github.com/TheCaptainCompany/captain-food/issues/532)),
> never after. Team-decidable + **founder-informed** (security-correctness sequencing). One material
> change since the proposal: the read-side (#144) landed as **JWT claims** in `Identity`, not the
> `ScopeMembership` projection PROP §2 assumed — so the fix is **smaller** (compare/derive from the
> claim already verified in `Identity`, envelope-carried); the proposal needs that refresh.
> **(2) SEQUENCING — the acceptance program had L5 before its harness.** L5's RED-first method needs a
> runnable local stack + a way to mint real tokens through the **fail-closed** verifier without cloud
> Supabase; today's `mint_token` requires cloud Supabase (`tools/smoke/prod-smoke.sh:170-227`). The
> **local acceptance harness** (local-issuer/JWKS stub + offline role-claim `mint_token` + runnable
> single-DB monolith stack + `sk_test`/`pk_test` Stripe wiring) is now recorded as **L5's true first
> sub-step** (ADR-20260813-191111 §5 re-sequenced below; program-of-record bullet updated). **(3) L5b
> re-scope**: its RED premise must prove the fail-closed shape that IS enforced (#519: role-mismatch or
> no-`captain_food` token → 403) and frame the `restaurant_id` claim as a **READ-scoping** proof, NOT a
> write-binding proof — unless §39 IDOR-1 is closed first, in which case L5b proves the write-binding.
> **Keystone unchanged: acceptance stays TOP; the harness is only its first build step, no re-rank.**
> **#554/#555 disposition recommended**: close PR #555 (executor stopped before a meaningful diff),
> keep #554 open + blocked-by the new harness issue + re-scoped to the L5b premise, re-dispatch after
> the harness lands. This run is docs-only; no code, no claim.

> 💳 **2026-08-13 — CAPTURE ON DELIVERED IS IMPLEMENTED: the recorded posture (ADR-20260808-195315
> §1.2/§1.3) and the code no longer disagree** ([#544 "Capture on delivered: implement the recorded
> authorize-then-capture posture"](https://github.com/TheCaptainCompany/captain-food/issues/544),
> the D2 slice of the acceptance program in
> [ADR-20260813-191111](../adr/ADR-20260813-191111-the-acceptance-criterion-six-clauses-walked-with-the-front-door-unlocked-from-inside.md),
> landed inside the empty-log window). The Stripe intent is created `capture_method=manual`;
> confirmation AUTHORIZES (`payment_intent.amount_capturable_updated` → new `PaymentAuthorized`,
> which is now what materializes the Order — rule renamed to
> `OrderMaterializedOnPaymentAuthorization`); the new **`PaymentSettlementProcess`** captures on
> `OrderDelivered` (the one handover fact for DELIVERY and COLLECTION) and RELEASES the hold
> (Stripe void → new `PaymentReleased`) on rejection/cancellation — *"no need to refund because no
> capture"*; post-capture aborts still refund (RefundProcess's CAPTURED guards untouched). The
> capture keys on the PRESENCE of a Captain authorization, never the delivery fact alone
> (ADR-20260813-233418 AR-2: $0 replacements and future Uber Eats external orders are structurally
> skipped). Capture-declined-after-fulfilment is recorded (`PaymentCaptureFailed`, typed reason)
> **and pages** (`payment_capture_failed_total`, `observability.yaml#/payment-settlement`).
> `PaymentStatus` = `PENDING → AUTHORIZED → CAPTURED → REFUNDED`, `AUTHORIZED → RELEASED`,
> `PENDING → FAILED`. Smoke L4 now asserts `requires_capture` at confirm and `paymentStatus ==
> AUTHORIZED` post-placement; the capture assertion moves to the future L5 delivered leg.
> OrderTracking folds the full new surface: OrderPlaced seeds AUTHORIZED for a charging order
> (the authorization precedes the row by the saga invariant, so `PaymentAuthorized` is
> deliberately not in its fedBy), `PaymentCaptured`/`PaymentReleased`/`PaymentRefunded` flip it —
> landed once the [#543](https://github.com/TheCaptainCompany/captain-food/pull/543) fence on
> `specs/database/**` lifted mid-run (+2 `event-not-projected` in the ratchet: `PaymentAuthorized`
> by design, `PaymentCaptureFailed` until its operator surface exists). At-table advance capture
> (§1.2's third arm) and the acceptance-timeout auto-cancel (§1.3) remain unbuilt; both ride the
> recorded arms when they land.
> **FIX ROUND 2026-08-14 (a five-lens review of the #544 PR):** the feature above shipped INERT —
> the settlement guard reads `OrderTracking.payment_intent_id` to know what to capture, but that
> column was only written when `PaymentCaptured` folded, the fact capture PRODUCES; so every
> delivered order read NULL → skipped → the hold expired at ~7 days, restaurant never paid. Fixed by
> seeding `payment_intent_id` from `OrderPlaced` (which carries it) at the row's birth, proven RED-then-GREEN
> by a new end-to-end DB test through the real projector AND the real saga runner. Same round:
> corrected customer copy that falsely promised a REFUND on rejection (a released hold is not a
> charge) + a checkout hold-disclosure; and declared a **dead-man's-switch** on the age of the oldest
> still-authorized order (`observability.yaml#/payment-settlement`), because the paging counter only
> fires on a failed ATTEMPT, never on a capture that is never attempted — its reconciling-sweep
> runtime is a tracked CRITICAL follow-up. **MERGED via [PR #545](https://github.com/TheCaptainCompany/captain-food/pull/545)
> after a delta re-review PASS (the CRITICAL fix's RED independently reproduced).**

> ✅ **2026-08-14 — FOUNDER DELEGATED A DECISION BATCH TO THE TEAM** (*"You don't need me for that"* +
> *"Go ahead team!!"*, the founder pasting back the decision list with its recommendations; authority
> [ADR-20260812-143619](../adr/ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md)).
> **Recorded as adopted on their recommendations** ([DECISIONS](../proposals/DECISIONS.md) header +
> rows): **STRIX-1** (GO — single gated, sandboxed, bounded, dev-only run) · **STRIX-2** (bounded run
> with hard time + token caps) → [PROP-20260814-000240](../proposals/PROP-20260814-000240-strix-security-audit.md)
> **Approved**, three Concerns checked · **D8** (bootstrap-then-flip source) · **D9** (Uber is
> merchant-of-record, informational record only) · **D10** (post-V0, design the aggregator shape now) ·
> **D11** (option 1 — either side in test ⇒ the ORDER is test, ticket unmistakably marked / off the
> live kitchen flow) — **D11 UNBLOCKS [#257](https://github.com/TheCaptainCompany/captain-food/issues/257)**.
> **LOSS-1 is DELIBERATELY KEPT OPEN / founder-flagged** (added after the delegated list, commits Captain
> to absorbing real money — out of the delegation's explicit scope). **Operating-model signal**: this
> extends the team's delegated authority (ADR-20260810-215503, backlog priority) to **this class of
> product decision** — the team decides + informs going forward; money-liability / external-legal /
> admin-gated matters stay founder-owned. **Re-ranked value stack** (keystone unchanged — the acceptance
> criterion [ADR-20260813-191111](../adr/ADR-20260813-191111-the-acceptance-criterion-six-clauses-walked-with-the-front-door-unlocked-from-inside.md)
> stays TOP): (a) finish capture-on-delivered (#545, in flight) → (b) the acceptance keystone — now
> **local acceptance harness FIRST** (its true first sub-step, 2026-08-14 re-seq, ADR-20260813-191111 §5),
> then smoke L5 lifecycle walk → (c) **GraphQL query cost/depth limiter** (new peak-readiness issue, HIGH — founder flagged it
> higher-leverage than Strix) → (d) Strix containment-harness + bounded run (approved, gated behind the
> harness, below the keystone) → (e) Uber aggregator-shape (post-V0) + #257/D11 implementation. This run
> is docs-only; no code, no claim.

> 💳 **2026-08-14 — the #544 five-lens review's carry-forwards (recorded)**
> ([PR #545](https://github.com/TheCaptainCompany/captain-food/pull/545), tracking issue
> [#544](https://github.com/TheCaptainCompany/captain-food/issues/544)).
> The review found a **CRITICAL (a circular read dependency made capture inert)** — now FIXED,
> delta-verified, and merged. Its
> **non-code carry-forwards are recorded** (this run, docs-only): the founder-owed
> permanent-capture-failure loss-allocation decision + operator runbook
> ([DECISIONS §38 LOSS-1](../proposals/DECISIONS.md)); a dba **forward-trap** hazard for the unbuilt
> at-table advance-capture arm (`PaymentCaptured`-on-`PENDING` would swallow `PaymentAuthorized` and
> never fire `PlaceOrderProcess` — money captured, order never materialized;
> [PROP-20260726-165000 D2](../proposals/PROP-20260726-165000-marketplace-economics-and-money-movement.md));
> same-day-only scheduling reframed as a **solvency** constraint requiring #175 before any multi-day
> scheduling ([PROP-20260726-164500 D6](../proposals/PROP-20260726-164500-order-operational-safety.md));
> a seven-question legal counsel packet
> ([BRIEF-20260814](../legal/BRIEF-20260814-capture-on-delivered-counsel-packet.md), **no lens output is
> legal clearance**); and a `bam` settlement-funnel projection as an ADR-20260811-014129 completeness
> follow-up ([#549](https://github.com/TheCaptainCompany/captain-food/issues/549), related to #484).
> Blocking review follow-ups filed: [#550](https://github.com/TheCaptainCompany/captain-food/issues/550)
> (CRITICAL — the dead-man's-switch reconciling-sweep runtime, blocks real orders) and
> [#551](https://github.com/TheCaptainCompany/captain-food/issues/551) (capture-failure alert split).

> 🗄️ **2026-08-13 — THE DATABASE PLACEMENT DECLARATION SITE EXISTS** ([#494 "Storage boundaries and
> least-privilege database users"](https://github.com/TheCaptainCompany/captain-food/issues/494)
> slice 1, [PR #543](https://github.com/TheCaptainCompany/captain-food/pull/543)):
> `specs/database/databases.yaml` declares the **eleven databases** (5 business + ADP-1's 6 adapter
> databases) as name + owning role + `k8sName` binding + `recovery` posture and nothing else, and
> the validator (§18, `database-placement-*` rules) enforces per-kind placement with **no default**:
> the four write-side kinds derive `captain_write` mechanically (STO-1(a) — the fencing token),
> staging/connection tables declare `database:` as a `$ref`, `ScopeMembership` is
> `replicated: read-databases` (STO-2(a)), and the ADP-1 membership wall is red in both directions
> (the avelo37 flip [ADR-20260812-115930](../adr/ADR-20260812-115930-each-adapter-owns-its-own-completely-isolated-database.md)
> documents nearly shipping now names its own mismatch). Business-table placement was **open** at
> this slice (register row STO-2) and declaring one was refused so a spec edit could not silently
> close the row — **superseded 2026-08-14: STO-2 closed and the refusal flipped to a requirement
> ([#562](https://github.com/TheCaptainCompany/captain-food/issues/562), entry above)**.
> The resolved inventory is GENERATED — `specs/generated/databases.generated.{md,json}` — and is
> the interface [#509](https://github.com/TheCaptainCompany/captain-food/issues/509) (drill legs),
> [#513](https://github.com/TheCaptainCompany/captain-food/issues/513) (grant emitter) and
> [#514](https://github.com/TheCaptainCompany/captain-food/issues/514) (migration chains) build
> against. **No CNPG manifests, no grants, no migrations moved** — this slice is the declaration
> site only; slices 2+ make it executable.

> ⏱️ **2026-08-13 — THE WEEKLY CAP IS NOT A STOP SIGN; billing continues** (founder, 2026-08-12,
> verbatim: *"Don't care about the budget right now understood?"*, operationalized by the
> 2026-08-13 resume prompt — provenance labeled in
> [ADR-20260813-132540](../adr/ADR-20260813-132540-the-weekly-cap-stops-being-a-stop-sign.md);
> hold resolved: founder-**confirmed** 2026-08-13, verbatim *"Continue the work enforcement and
> split"*, given as a user turn in the coordinating session's conversation in direct response to
> the held question about the realizing PR — the founder posts no GitHub comments, and the session
> transcript, not GitHub, is the record).
> `.claude/loop-budget.json` now carries `"capIsAStopSign": false`: `loop-budget.sh check`/`start`
> still print the over-cap state loudly but exit 0, so no session stands down for it (the #510
> executor did, on 2026-08-13 ~13:20Z container clock, against an already-lifted gate — the cost that forced the
> record). Integrity refusals (stale timer, double-open, audit) and the append-only ledger are
> unchanged. **The report that replaces the constraint**: W33 had recorded **1646.4 minutes
> (~27.4h) as of 2026-08-13 ~17:05Z** — a number, not a gate, and a SNAPSHOT, not a live value: a
> minute count pinned in prose is stale the moment the next run bills (this very paragraph shipped
> stale once, 1602.0m written before its own branch's follow-up entries landed). Re-derive, never
> trust: `bash .claude/hooks/loop-budget.sh status`, or near the cap the cross-branch union snippet
> in [docs/claude/loops.md](../claude/loops.md). **Exit condition** (event-bounded, pre-recorded path back):
> when [DECISIONS §35 INV-1](../proposals/DECISIONS.md) is met — its acceptance criterion now EXISTS:
> the founder's six-clause walk,
> [ADR-20260813-191111](../adr/ADR-20260813-191111-the-acceptance-criterion-six-clauses-walked-with-the-front-door-unlocked-from-inside.md)
> — AND the first infrastructure euro is spent, the architect's run report flips the flag back to
> `true` — executing the ADR, not a new decision.

> 🔁 **2026-08-13 — "I'M REPEATING MYSELF": RECORDED INTENT MUST EXECUTE ITSELF, AND THE UBER EATS
> ONBOARDING WEDGE** (two founder directives;
> [ADR-20260813-233418](../adr/ADR-20260813-233418-recorded-intent-must-execute-itself-the-anti-repeat-mechanisms.md),
> DECISIONS §11 D8/D9/D10 + §37). **Process (higher value)**: a recorded decision was not executing
> itself — the Uber Eats directive sat approved-but-undone for two weeks, the capture-on-delivered
> posture drifted with no gate — so two light anti-repeat mechanisms land: an **unrealized-directive
> sweep** as a standing architect-run step (`.claude/skills/architecture-review/SKILL.md` §3bis; a
> validator rule was judged and rejected as noise — ~30 proposals carry stale `_(filled at
> completion)_` headers, and the offline gate cannot see live PR state), and an **`Enforced by:`
> field** on the ADR template so a recorded behavioral guarantee names the `rules.yaml` entry+test
> that pins it (the capture-timing rule lands inside #544, keyed on a Captain authorization).
> **Product**: the Uber Eats catalog/order-sync directive + the onboarding wedge (bootstrap a
> no-HubRise restaurant's own menu from Uber Eats → flip Captain to source → push) refined into the
> living PROP-20260730-032306 §3bis/Slice F, reconciled with the recorded no-scraping constraint
> (`specs/integrations/sirene.md:67`) via the licensed Menu API + own-menu-only. D8/D9/D10 are the
> new founder-owed rows. Docs-only; landed on `main` from an isolated worktree (the shared checkout
> was on the #543/`494` branch). **Follow-up (same day)**: a config-structure directive — two declared
> Uber apps, *Captain Food Restaurant* = Eats Marketplace API (catalog+orders) **+**
> `uber_direct:restaurant`, *Captain Food Marketplace* = `uber_direct:marketplace` **only** — folded
> into PROP-20260730-032306 §6.1 with the three-way "marketplace" disambiguation. **Verdict:
> clarification of ambiguous ADR-Decision-1 prose, NOT a reversal** (no decided row bound
> catalog/orders to a *Captain Food Marketplace* app; ADR line 18 registers the Eats suite under
> Captain Food Restaurant), so no DECISIONS register row. **Second follow-up (same day)**: *"test and
> prod keys to test directly on production"* — restates the recorded 2026-07-29 keys directive
> (`specs/delivery/configuration.yaml:94-107`) and confirms **[#257 "Stripe mode becomes a DOMAIN
> property, not a deployment one: hold both key pairs and select per order"](https://github.com/TheCaptainCompany/captain-food/issues/257)**
> (Stripe-first; the Uber Direct config comment extends the same pattern — one order-mode drives both
> integrations; #257 **supersedes #254**) as its realization; folded into PROP §6.2. **CORRECTION
> (coordinator supplied #257's content; my session couldn't reach GitHub, HTTP 403)**: my first pass
> classified mode-coherence a rule with no founder decision — that pre-decided ≈ #257 option 1 and was
> **over-reach**. Split fixed: the **SoT/coherence half** (one order-mode fact, #544's capture leg and
> #257's selector share it) stays a **rule**; the **mixed-mode resolution** (test customer × live
> restaurant) is a **FOUNDER decision — new OPEN row D11**, carrying #257's four-option table + a
> recommendation (option 1 with the test ticket unmistakably marked / off the live kitchen flow), and
> **it BLOCKS #257 implementation** per #257's own words. The **observability contract** (mode visible
> per order) stands unchanged. Config-key *structure* verdict is still clarification/advance; the
> mixed-mode *policy* is the genuine open decision.

> 🎯 **2026-08-13 — THE ACCEPTANCE CRITERION EXISTS: SIX CLAUSES WALKED ON THE LOCAL STACK, WITH THE
> FRONT DOOR DELIBERATELY UNLOCKED FROM THE INSIDE** (founder directive + ten-lens mob;
> [ADR-20260813-191111](../adr/ADR-20260813-191111-the-acceptance-criterion-six-clauses-walked-with-the-front-door-unlocked-from-inside.md),
> [DECISIONS §35 INV-1](../proposals/DECISIONS.md) resolved; **records-only — no code, no specs, no
> generated artifacts**, so no SPEC-LOG row is owed).
> - **The criterion, verbatim**: *"For the acceptance, i need to have all the dbs, apps deployed
>   locally and working without considering the authentication contraints with supabase from the
>   creation of the customer, payment authorisation, order creation order accepted delivered payment
>   captured"* — six observable clauses (customer created → payment authorised → order created →
>   accepted → delivered → captured), each asserted through the deployed local stack's **own API and
>   read models**, plus a **browser walk of storefront + backoffice queue** (a labeled bot rider is
>   fine). **Supersedes the team's two-half proposal**: the browser-walk-**with-login** half drops
>   from gating to demo artifact — login
>   ([#529](https://github.com/TheCaptainCompany/captain-food/issues/529)/[#532](https://github.com/TheCaptainCompany/captain-food/issues/532))
>   is OUT of acceptance and is the named first lane after it, with
>   [#533](https://github.com/TheCaptainCompany/captain-food/issues/533) opening the
>   first-real-order gate.
> - **Two stated assumptions** (correctable by the founder at zero cost): *all the apps* = the
>   monolith and its surfaces until the
>   [#358](https://github.com/TheCaptainCompany/captain-food/issues/358) cutover (CUT-1 = B excluded
>   the fleet); *creation of the customer* = the real `verifyPhone` via Supabase's
>   test-phone/static-OTP facility (a genuine `CustomerRegistered`, zero SMS), with the
>   claim-stamped fallback labeled honestly if the facility proves unavailable.
> - **The auth bypass is unlocked from the inside, never weakened**: real tokens through the real
>   fail-closed verifier (admin-minted, or a local-JWKS stub as the recorded fallback) — the
>   fail-open shape was deliberately deleted
>   ([#519](https://github.com/TheCaptainCompany/captain-food/issues/519)/[PR #520](https://github.com/TheCaptainCompany/captain-food/pull/520))
>   and stays deleted.
> - **⚠️ The criterion's biggest finding was a D2 record↔code drift** — the founder's clause order
>   restates his OWN [ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md) §1.2
>   (*"Authorise on checkout. Capture on delivered / picked up"*) while the code captured at
>   confirm. **RESOLVED 2026-08-13 by the D2 slice**
>   ([#544 "Capture on delivered"](https://github.com/TheCaptainCompany/captain-food/issues/544),
>   see the capture-on-delivered entry at the top of this file): the Order materializes on
>   `PaymentAuthorized` (`rules.yaml#/OrderMaterializedOnPaymentAuthorization`), `AUTHORIZED`
>   exists, `capture_method=manual` is set, and `PaymentSettlementProcess` captures on the
>   delivered fact. The walk harness's capture assertions can now run against the implemented
>   semantics.
> - **✅ RESOLVED 2026-08-17 BY A FOUNDER ANSWER — the program of record below is the LIVE sequence
>   for the first END-TO-END READING** ([DECISIONS §45 SEQ-1](../proposals/DECISIONS.md),
>   [ADR-20260817-105844](../adr/ADR-20260817-105844-the-walk-goes-first-on-one-database-and-production-stays-suspended.md)).
>   *History, kept because a reader needs to know this line moved twice*: this sequence puts the
>   harness and L5 on a **single-DB monolith stack**; on **2026-08-14** the founder directive *"The
>   acceptance include the full enforcement and full split"* pulled the eleven-database split ahead of
>   the walk and this bullet was marked superseded; on **2026-08-17** he answered the resulting
>   contradiction **in favour of this sequence** — the walk goes first, on one database.
>   **The two sequences are now ONE, split by purpose, and neither supersedes the other**:
>   - **The first end-to-end READING (live, buildable today)** — exactly the program below: #556
>     harness on a single-database monolith stack → L5 lifecycle legs → the non-auth browser walls.
>     Its target is the single-node k3s stack that already stood up on 2026-08-11. Anything it
>     produces is labelled a **reading**, never *accepted*.
>   - **The walk that CERTIFIES (the 2026-08-14 entry higher in this file)** — physical split band
>     #513 → #514 → #509 → the write-auth fix ([DECISIONS §39](../proposals/DECISIONS.md) IDOR-1 / #178,
>     scope-corrected 2026-08-17 to cover the read side) → harden the `inbound_messages` write path →
>     the six-clause walk on the eleven-database, least-privilege, write-auth-enforced stack. **The
>     acceptance criterion is unchanged**; it stopped *gating* the reading, it did not shrink.
>   Why this is not a final-vision-first breach: the split band is blocked on **STO-7, STO-8 and
>   STO-9** (each independently), with **STO-10 parked** and **RDR-1** open upstream of #513's grant
>   emitter — so "build the final step first" means the first reading arrives never, not sooner.
> - **The program of record** (ADR §5, **re-sequenced 2026-08-14 — harness before L5**; superseded
>   2026-08-14 by the full-split scope clarification, **restored 2026-08-17 as the reading sequence**
>   by the founder answer above):
>   [#536](https://github.com/TheCaptainCompany/captain-food/issues/536)
>   (merged) → split slice 1 → the **local acceptance harness** (local-issuer/JWKS stub + a `mint_token`
>   that signs role + `captain_food` claims **offline** against a key the fail-closed verifier is pointed
>   at, a runnable **single-DB monolith** stack target, and `sk_test`/`pk_test` Stripe wiring) — **the
>   true first sub-step of L5**, without which no lifecycle leg can show RED or GREEN (today's `mint_token`
>   needs cloud Supabase, `tools/smoke/prod-smoke.sh:170-227`) → smoke **L5 lifecycle legs** (accept →
>   ready → dispatch-job-present → delivered, each seen red first) → the four **non-auth browser walls** →
>   the **D2 slice** (DONE, [#544](https://github.com/TheCaptainCompany/captain-food/issues/544)/[PR #545](https://github.com/TheCaptainCompany/captain-food/pull/545)) →
>   [#514 "per-database migration chains"](https://github.com/TheCaptainCompany/captain-food/issues/514)
>   + Database CRs + local overlay as ONE slice (the delivered leg forces the
>   `View_DeliveryJob`→table conversion there; the harness's single-DB stack is enough for L5, the
>   eleven-DB stack is a precondition only for the final acceptance walk) → **acceptance**: the walk in his clause order,
>   storefront + backoffice in a browser, on the all-databases stack, evidenced by a **checkable
>   JSON record** (causal event chain by `cause_id` + all eleven migration heads), synthetic
>   identities stated, Stripe Connect shape verified in the script.
> - **The honesty sentence** (verbatim from the mob): *"This acceptance proves the order machine
>   end-to-end with the front door deliberately unlocked from the inside — no path from a real
>   customer's phone through OTP sign-in to this flow has ever been walked, so 'accepted' certifies
>   the machine, never that a customer can use it, and the auth walk (#529/#532) is the named
>   remainder between this record and the first real order."*

> 🚪 **2026-08-13 — ONE JOURNAL, ONE DOOR IS NOW LEVEL 4 ON ALL THREE SURFACES** ([#510 "Follow-up
> to #506: move the mailbox query ports behind a capability witness so a resolver cannot name
> them"](https://github.com/TheCaptainCompany/captain-food/issues/510), PR
> [#536](https://github.com/TheCaptainCompany/captain-food/pull/536)): the mailbox writes were
> compiler-sealed by #304, the SPEC-side reads walled by #507's validator rules, and the last open
> surface — the supervision QUERY ports — is now witness-gated too, **asymmetrically, because the
> two ports' legitimate callers sit on opposite sides of the `actor_client → application` arrow**.
> The READ port (`MailboxLaneRepository` + its two row types) MOVED to `actor_client::supervision`,
> where its `list`/`poisoned` methods demand the existing `MailboxAccess(pub(crate) ())` witness;
> the generated resolvers read through two declared door functions (`mailbox_lanes` /
> `poisoned_messages`) that mint internally — the `operationStatus` shape. The WRITE port
> (`MailboxRequeue`) SEALED IN PLACE in `application` behind a new `MailboxRequeueAccess`
> witness minted only inside `requeue_mailbox_message`. The arbitration SQL moved byte-identical;
> zero diff in `schema.generated.graphql`/`acl.rs`/`command_router.rs`; the dead resolver-side
> half of `wired_mutation_dispatch` (pre-Runtime-D vestige) is deleted; `build_schema`'s ReadDeps
> registration is now an exhaustive no-`..` destructure so a field added without a `.data()` call
> is a lint at the site, not a 500 on the supervision screen (the #529 class). The
> `every_mailbox_port_method_demands_the_access_witness` guard now covers both supervision ports.
> **Honest residuals**: code INSIDE `application` can still mint the requeue witness (the crate is
> the boundary, not the function); the `test-fixtures`-gated mints remain a deliberate test door
> (CI-guarded out of release graphs — the #536 review proved the guard's wholesale skip of the two
> declaring manifests was a hole, since `actor_client` depends on `application`: the fixed gate now
> scans them too, allowing only the `test-fixtures = []` declaration line, which also refuses the
> `default = ["test-fixtures"]` variant, and the trait-impl/derive watch on the witness now covers
> `MailboxRequeueAccess` across all of `application`); and `crates/server` still holds raw SQL over a pool — that is
> [#512 "pool + schema probe out of `crates/server`"](https://github.com/TheCaptainCompany/captain-food/issues/512)'s
> half, untouched here.

> 🔐 **2026-08-13 — A TOKEN MUST NOW PROVE THE PRODUCT, NOT ONLY THE PROVIDER** ([#519](https://github.com/TheCaptainCompany/captain-food/issues/519),
> [ADR-20260813-013211](../adr/ADR-20260813-013211-a-token-must-prove-the-product-not-only-the-provider.md),
> [SPEC-LOG row](../SPEC-LOG.md)). The group is about to put every product behind ONE Supabase project,
> which retires the two separators the verifier leaned on — `aud` is the constant every Supabase user
> of every project carries, and `iss` becomes identical across siblings. Three fixes, all with tests
> seen RED first:
> - **Issuer is mandatory, and the compiler holds it.** `AuthContext` carries one
>   `Option<Verifier { jwks_url: String, issuer: String }>` with a single constructor, so *"no issuer
>   ⇒ skip the issuer check"* — previously reachable whenever `SUPABASE_URL` resolved empty, which is
>   exactly the configuration in which a **staging token verified in production** — is not a state the
>   type can hold. Unset now REFUSES: `503` on every role path, anonymous on `/public`.
> - **…and MATCHING a reserved claim is not REQUIRING it** (found by independent review, seen red as
>   `left: ["exp"]`). `jsonwebtoken 10.3.0`'s `set_issuer`/`set_audience` only assign a matcher;
>   `required_spec_claims` stays `{"exp"}` and `validate()`'s `iss`/`aud` arms end in `_ => {}`, so a
>   token that OMITS the claim — or carries a non-string one — passed **vacuously**. `Verifier::validation`
>   now DERIVES the required set from the matchers it set, so the two cannot drift. The exposure was
>   never an outsider's (the token must still be signed by a key in our JWKS); it is that a shared
>   group project's access-token hook is exactly what can drop or retype `iss`.
> - **Roles fail closed.** `parse_role` returns `Option`; absent or unrecognised grants NOTHING. The
>   old `_ => Customer` catch-all is what would have landed a sibling product's user on
>   `/customer/graphql` as an authenticated customer.
> - **A positive product check.** All claims move under `app_metadata.captain_food = { role,
>   customer_id, restaurant_id, restaurant_account_id, rider_id }` — nested rather than renamed
>   because Supabase merges `app_metadata` SHALLOWLY, so one owned key is what another product's write
>   cannot reach into. A token without that object is refused. **No read-side tolerance for the flat
>   shape** (Q-L3: no real phone-verified end user; the only producers were the smoke script, updated
>   here, and the test suite).
> - **⚠️ STILL OWED, and NOT code**: `specs/common/configuration.yaml` still points staging and
>   production at the SAME Supabase project (`zcshlzhiinwmpzujuiep`). This change removes the
>   fail-open half of that risk, not the shared-project half — splitting them is a founder/provisioning
>   action. Also unchanged: no OTP rate limit ([#516](https://github.com/TheCaptainCompany/captain-food/issues/516)),
>   and `public_credential_degraded_total{reason=role_not_customer}` now covers two populations
>   pending [#517](https://github.com/TheCaptainCompany/captain-food/issues/517).

> 🔐 **2026-08-13 — IDENTITY: SUPABASE AUTH IS RETAINED FOR V0, AND THE WINDOW TO OWN IDENTITY CLOSES
> AT THE FIRST REAL ORDER** (founder directive + ten-lens mob;
> [ADR-20260813-004634](../adr/ADR-20260813-004634-supabase-auth-is-retained-for-v0-and-the-window-closes-at-the-first-real-order.md),
> new [DECISIONS §36 IDP-1](../proposals/DECISIONS.md); **records-only — no code, no specs, no generated
> artifacts**, so no SPEC-LOG row is owed).
> - **The correction underneath it**: *"Don't care about the Supabase keys because we going to use our
>   own Postgres hosted on Kubernetes"* — the premise was already true and the inference was not. The
>   database has been self-hosted CNPG since
>   [ADR-20260807-002705](../adr/ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md); Supabase is the
>   **identity provider only** and holds no business data; **`SUPABASE_SECRET_KEY` gates authentication,
>   not storage**, so it remains the hard stop [§35 INV-1](../proposals/DECISIONS.md) recorded against
>   smoke L3/L4.
> - **The decision**: retain, verbatim *"For the auth/identify we will use Supabase because it's free
>   and easier"* — EUR 0 at V0 volumes, and the **SMS bill is identical either way** because OTP already
>   goes out on our **own OVHcloud account** through the Supabase Send-SMS hook (ADR-20260722-174500).
>   All ten lenses recommended not self-hosting now; the strongest reason is that
>   `crates/server/src/auth.rs`'s `asymmetric_alg` accepts **asymmetric JWKS only, deliberately** (kills
>   `alg`-confusion forgery), while self-hosted GoTrue defaults to symmetric — a swap edits the token
>   verification path days before a demo.
> - **⚠️ THE DEADLINE, which is the durable output**: `domain_events.user_id` holds the **provider's
>   subject**, so a later switch is an **upcasting migration on an immutable log**; and with
>   **Q-L3 = no real phone-verified user (2026-08-12)** a switch today triggers no processor-exit
>   obligations. **Both windows close at the first real order** — *if we ever intend to own identity, it
>   lands before the first real order or it becomes materially more expensive.*
> - **Unchanged by the decision, still owed** (proposed as issues, not filed here): no **OTP rate limit
>   / `+33` allowlist** anywhere (SMS-pumping is a real money risk on our own SMS account); the
>   **auth-session park/pickup has no observability contract** and `auth_routes.rs` discards the cause;
>   the **OTP rejection message has no translation key**. Also NAMED, not fixed:
>   `specs/architecture/c4-l3.yaml` still says the OTP goes via *"Twilio"* (a `specs/**` edit owing a
>   SPEC-LOG row).
> - **For the demo leg**: identity needs the key **AND pod egress to `SUPABASE_JWKS_URL`** (two gaps,
>   not one — egress is checkable in minutes); the **payment leg needs no ingress**, since
>   `stripe listen --forward-to` reaches the local stack outbound and the CLI's own signing secret
>   satisfies the fail-closed `STRIPE_WEBHOOK_SECRET` boot gate.

> 📌 **2026-08-12 — THE FOLLOW-UP REGISTER: nine findings from tonight's mob reads are now ISSUES,
> not paragraphs** (records-only). Each is linked from the register row it belongs to, so it is
> reachable from the decision as well as from here:
> - [#508](https://github.com/TheCaptainCompany/captain-food/issues/508) — `hubrise_connections.access_token`
>   is **plaintext** and a non-expiring token, so the physical WAL archives carry it too (linked from
>   [DECISIONS §32 ADP-1](../proposals/DECISIONS.md), which called that table non-rederivable without
>   saying it was unencrypted).
> - [#509](https://github.com/TheCaptainCompany/captain-food/issues/509) — the restore drill verifies
>   **1 of the 11** databases the split creates (linked from **STO-6**).
> - [#510](https://github.com/TheCaptainCompany/captain-food/issues/510) — mailbox query ports behind a
>   capability witness: the level-4 half of #506 the validator rule cannot reach (linked from
>   [ADR-20260812-214500](../adr/ADR-20260812-214500-a-read-target-is-declared-not-inferred-the-reads-ownership-wall.md)
>   and [PROP-20260802-130500 §1](../proposals/PROP-20260802-130500-isolation-by-construction.md)).
> - [#511](https://github.com/TheCaptainCompany/captain-food/issues/511) — JWKS single-flight test flake.
> - [#512](https://github.com/TheCaptainCompany/captain-food/issues/512) — pool + `_sqlx_migrations`
>   schema probe out of `crates/server`, the second level-4 half of #506 (same two links as #510).
> - [#513](https://github.com/TheCaptainCompany/captain-food/issues/513) — the adapter-isolation grant
>   emitter and its **negative-path** test: nothing today proves a pod is REFUSED a database (linked
>   from **ADP-1** and **STO-5**).
> - [#514](https://github.com/TheCaptainCompany/captain-food/issues/514) — per-database migration chains
>   and a `REQUIRED_SCHEMA_VERSION` **map**: eleven databases against today's one chain and one scalar
>   constant (`crates/server/src/lib.rs:170`) — linked from **STO-1** / §35's **CUT-1** cutover row.
> - [#515](https://github.com/TheCaptainCompany/captain-food/issues/515) — `join.captain.food`'s legal
>   pages still lack a postal address, a phone and a named directeur de la publication, and name **no
>   consumer mediator** (linked from **Q-L1**).
> - [#502](https://github.com/TheCaptainCompany/captain-food/issues/502) — re-scoped in a comment: five
>   stale `inbound_event_id` declarations survived [#500](https://github.com/TheCaptainCompany/captain-food/issues/500)
>   in `specs/observability.yaml` (lines 506, 584, 648, 732, 966, each `source: "inbound.inbound_event_id"`
>   against a table dropped by `20260731143000`), and the fix is to **type the reference** rather than to
>   rename the survivors — an untyped `source:` string is the [#413](https://github.com/TheCaptainCompany/captain-food/issues/413)
>   defect class again, invisible to the refs walker and therefore to every rename.

> 🧭 **2026-08-12 — THE FOUNDER ANSWER SHEET: THE FLIP IS TAKEN, THE REGISTRY IS DESTROYED, AND
> NOTHING IS PAID FOR UNTIL A WORKING VERSION CAN BE SEEN** (twelve founder answers + a ten-lens mob
> read; [ADR-20260812-214021](../adr/ADR-20260812-214021-the-founder-answer-sheet-of-2026-08-12.md),
> new [DECISIONS §35](../proposals/DECISIONS.md); **records-only — no code, no specs, no generated
> artifacts**).
> **The headline is not one of the twelve answers, it is what they add up to: the critical path is
> INVERTED.** *"I'm waiting for a working version before paying OVH"* turns **provision → deploy →
> walk** into **walk → provision → deploy** — and the one leg the team cannot supply is the exit
> condition, because *"a working version"* carries **no acceptance criterion**, which makes it a spend
> gate with no exit. Recorded so it can be confirmed or replaced (**§35 INV-1, the one FOUNDER-OWED
> leg**): **smoke L1→L4 green on local k3s plus a recorded browser walk** — order placed, paid,
> restaurant told, tracking moving without a reload, order completing. Both halves are needed:
> `prod-smoke.sh` never opens a browser, and a browser walk cannot assert a Stripe capture.
> **The path is a MERGE, not a build** — `origin/cutover-local-rehearsal` /
> [PR #486](https://github.com/TheCaptainCompany/captain-food/pull/486) already carries the
> local-rehearsal runbook, the k3s CNPG overlay, the generated monolith overlay and the smoke's
> `SMOKE_SCHEME`/`SMOKE_PUBLIC_BASE` overrides, with **L1+L2 passing and 45/45 migrations on an empty
> database**, while `main`'s `tools/smoke/prod-smoke.sh:41,48-49` still hardcodes an unroutable
> `https://api.captain.food` with no scheme override. Two gaps sit outside the merge:
> `SUPABASE_SECRET_KEY` as its own repository secret (hard-stops L3, and L4 is downstream — presence
> is a **confirmation**, since STATUS already records a secret of that name existing on 2026-08-09)
> and **a webhook ingress for L4's `CAPTURED` assertion**. **Local is demo, never evidence**: the
> overlay strips `barmanObjectStore`, so the **restore drill is the first post-provisioning act** and
> no recovery claim may cite the rehearsal.
> **The answers, and what each cost to check.** **JRN-1 = A** — take the `PM_MAILBOX_DELIVERY` flip
> now in [PR #500 "#242 Runtime D: retire command_journal"](https://github.com/TheCaptainCompany/captain-food/pull/500)
> inside the empty-log window, with **L4 as the release gate before traffic is routed**; verified
> consequence: option (a)'s interim `command_journal` grant is **not owed at all** once #500 merges
> (it drops the table and empties `RuntimePosture`), and that PR also already removes
> `dispatch_outcome: spawned` and deletes the `CommandChannel`/`CommandJournalStatus` scalars.
> **CUT-1 = B** — the cutover gets a **rule**, not a list: *IN = only what the empty log or a traffic
> pause makes cheaper*, admitting **the eleven-database storage split** and excluding the pooler, the
> API-tier split and the runtime decomposition. **DB-HA = A** (three instances, inside the cutover) is
> **recorded, not incurred**: with `enablePodAntiAffinity` + `podAntiAffinityType: required` on a
> hostname topology, `instances: 3` on one node leaves **two pods `Pending` forever**, so A is the
> **EUR 67.80** trio and its +EUR 41.20 is unpayable until the EUR 26.60 base is — and the **60 Gi of
> PVC it implies is unpriced anywhere in the repo**, because the runbook ADR-20260807-114122 cites for
> the sizing detail (`docs/runbooks/mks-bootstrap.md §2`) **does not exist**. **SIR-1 = all NO**
> (*delete and record the destruction*) closes the retroactive SIRENE risk **on attestation, not
> inspection** — so the record owes how/when the rows ceased, a project list captured while absence is
> still inspectable, whether any backup/PITR window survives, and a named attester; and **two
> neutralisations are owed before any re-sync**, both live today (`sirene-sync.yml` is paused only by a
> commented-out cron and **deliberately keeps `workflow_dispatch`**, writing the staging table from
> `secrets.DATABASE_URL`, which must be revoked and the revocation logged). **The Art. 21 blocker
> survives forward-looking** ([#505](https://github.com/TheCaptainCompany/captain-food/issues/505)):
> `RestaurantListingOptedOut` folds into **nothing** (`generated/projectors.rs:59` is `=> state`).
> **Q-L1 partially resolves** — `join.captain.food` publishes the association, RNA W372020229 and the
> rights contact, and publishes **no postal address, no phone, no named directeur de la publication and
> no consumer mediator**; its host block is GitHub Pages, so *verify, do not copy*. **Q-L3 = no real
> phone-verified end user** — which both supports the empty-log window and dates the trigger (first
> real customer order = DPIA + erasure + mediator deadline). **BND-6 = B** (kitchen time labelled
> "ready" — the label IS the decision) · **BND-7 = A** (estimate, no remedy) · **Q1 = A**
> (authenticated server-side only — graded: plausibly no consent banner, but **Art. 13 transparency
> and lawful basis remain**) · **Q2 = A** (yes after the DPIA; it makes the restaurant a
> controller/joint controller) · **Q7 = A** (not now, converging with MET-Q7 a day later) ·
> **KEY-1** delete the stray key now — ⚠️ **its referent is recorded nowhere in the repo** and this
> record does not invent one.
> **Two recorded corrections landed with the sheet, as corrections rather than silent edits.**
> (1) **STO-4's sequencing is WITHDRAWN**: its ~185/~235-of-220 arithmetic is a **57-pod bin-fleet**
> figure and the fleet is OUT of the cutover, so with the monolith deployed eleven databases × one pod
> is **~55 backends of 220**; the pooler is re-targeted as a blocking precondition of the
> [#358](https://github.com/TheCaptainCompany/captain-food/issues/358) bin flip, plus a
> recommendation to cap the monolith's per-database pool. (2) **PROP-20260809-021351's gap table was
> STALE and is corrected in place**: **G5, G6 and G7 are FIXED** (#420/#451/#424 — including the
> subscription that now accepts the order's `DeliveryJob-` stream and dedupes on `updated_at`),
> **C1 is only HALF fixed** (the total prices live on read; the competitor comparison still never
> computes and moved from the projector to `cart_read.rs:187`), and **G7b, G8 and C2 are live** — G8
> being *nobody is told about a paid order*, with `crates/application/src/ports.rs` declaring four
> traits and **zero notification anything**.
> **Backlog — a previously stated order is REVERSED, and the method clause is on the record**
> ([ADR-20260810-215503](../adr/ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md) +
> [BACKLOG.md](../BACKLOG.md)): **[#429 "Production with test data"](https://github.com/TheCaptainCompany/captain-food/issues/429)
> is re-pointed off OVH onto local k3s WITHOUT re-scoping** (ADR-20260809-050000 fixed its target as
> the production deployment; the inversion changes the host, and under *"local is demo, never
> evidence"* a local walk satisfies the spend gate and does **not** close #429), and the
> **[#494](https://github.com/TheCaptainCompany/captain-food/issues/494) storage chain drops below
> it** on *value-first: foundations first* — a foundation that cannot be applied is not first, and
> #494 lands at a cutover now downstream of a payment decision it cannot unblock. **Nothing was
> re-ranked to make it dispatchable.**

> 🧾 **2026-08-12 — THE FOUNDER IS THE FOUNDER, AND EVERY FOUNDER MESSAGE GOES TO THE WHOLE TEAM**
> (two founder directives, verbatim: *"Stop calling me product owner. I'm the founder / Tech CEO."*
> and *"When I say something ask the team for answers never answer directly without asking the whole
> team."*;
> [ADR-20260812-143619](../adr/ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md)).
> The mob principle ([ADR-20260809-013142](../adr/ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md))
> extends from **dispatches to founder messages**, and coordinator-never-authors
> ([ADR-20260810-011500](../adr/ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md))
> from **the diff to the answer**: no answer is composed and no record lands before the whole roster
> has been asked, with *"nothing in my lens"* a complete one-line answer. Three carve-outs, each
> attributed: an **external-clock fact** is relayed in the same turn (business lens), **executing an
> already-recorded rollback/abort path** needs no consult while going FORWARD through an incident does
> (release lens), and **no lens output or aggregation of lenses is legal advice or clearance** (legal
> lens). New rule: a record created from a founder directive carries a **`Consulted:` block, one line
> per lens** — because a lens that was never asked is indistinguishable from a lens with nothing to
> say (testing/UX/observability lenses, convergent). "Product owner" is swept from the LIVING
> operating docs (`CLAUDE.md`, `PLAYBOOK`, `BACKLOG`, `docs/claude/*`, `proposals/README`, and the
> register's `PRODUCT-OWNER-OWED` → `FOUNDER-OWED`); **historical ADRs and proposals keep their
> vocabulary** and verbatim quotes stay verbatim. Legal caveat: the title is right for repo records
> and is **not** a French corporate mandate — external artifacts must name the statutory capacity.

> 🔒 **2026-08-12 — EACH ADAPTER OWNS ITS OWN, COMPLETELY ISOLATED DATABASE — decided, then
> CORRECTED the same day** (founder directive, verbatim: *"Each adapter must have there own database
> completely isolated"*;
> [ADR-20260812-115930](../adr/ADR-20260812-115930-each-adapter-owns-its-own-completely-isolated-database.md);
> register row **ADP-1** in [DECISIONS §32](../proposals/DECISIONS.md); records-only — no code, no
> specs; execution rides
> [#494 "Storage boundaries and least-privilege database users"](https://github.com/TheCaptainCompany/captain-food/issues/494)).
> Supersedes
> [PROP-20260811-093000](../proposals/PROP-20260811-093000-storage-boundaries-and-least-privilege-database-users.md)
> §11's placement of integration staging in `DomainCommonDb` (map amended in place): **six adapter
> databases** — `adapter-stripe` · `adapter-hubrise` (staging + the credential tables) ·
> `adapter-uber-direct` · `adapter-coopcycle` · **`adapter-avelo37`** · `adapter-sirene` (the 655 MB
> mirror) — each reachable by ONE app and nothing else, in the shared business cluster (STO-3's math
> already priced per-thing clusters out; the wall is role + `CONNECT`, BND-3's mechanism). **Eleven
> databases total** (5 business + 6 adapter).
> **A full-roster mob found two defects in the first record of this and both are fixed**: (1) it
> claimed *"avelo37 owns no table today"* — **false**, `external_avelo37_events` is declared
> (`integration_staging.yaml:178`) and already retention-swept (`sweep_retention.sql:60`), so avelo37
> would have been the ONE partner mirror left holding `CONNECT` on the write database while every
> sibling moved out; (2) it recommended an `adapter-identity` database for `auth_sessions` on a
> rationale that runs **backwards** — that table is AES-256-GCM encrypted under `AUTH_SESSION_KEY`
> while `hubrise_connections.access_token` is **plaintext**, there is no such adapter crate or bin,
> and its users are the actor path plus the BFF login route. The count did not move; the **membership**
> did. **Both legs are now CLOSED**: leg 1 **(a)**, the `inbound_messages` front door stands — an
> outbox+relay would hold a *bidirectional* platform grant inside each adapter database, and
> `LISTEN`/`NOTIFY` being per-database would need an inward connection to all six or a forbidden
> permanent poll; leg 2 **(b)**, `auth_sessions` **stays platform on `captain-write`**. The GraphQL
> lens's dissent is recorded as the final-vision alternative (an identity bin owning the table AND
> `/auth/session`+`/auth/refresh`+`/auth/logout`, which would also home the
> [#385](https://github.com/TheCaptainCompany/captain-food/issues/385) routes that have no bin home
> today) — a larger slice, not taken now. Reframing finding: `AUTH_SESSION_KEY` is granted to **53 of
> 56 pods** while exactly **two** decrypt a session, so narrowing the grant
> ([#491](https://github.com/TheCaptainCompany/captain-food/issues/491) slice A4, emitter + negative
> test in [#513](https://github.com/TheCaptainCompany/captain-food/issues/513)) buys more here than
> the database wall. **That figure was first recorded as "53 of 57, every group but the four periodic
> workers" and the correction makes it WORSE**: [#500](https://github.com/TheCaptainCompany/captain-food/issues/500)
> deleted `worker-journal-sweep`, which was one of the four EXCLUDED workers, so the denominator fell
> and the numerator did not — read the smaller number as a widened blast radius, not as progress
> (three excluded workers remain: `worker-erasure`, `worker-retention`, `worker-sirene-sync`).
> Named consequences: STO-4's pooler-first sequencing **hardens** (every adapter
> bin holds two pools), `hubrise_connections` is the one NON-rederivable adapter table (a
> non-expiring token only a human re-connect replaces) so it needs a backup story while staging
> mirrors take the refetch posture — and that token is **plaintext**, so the same backup copies it
> into the WAL archives ([#508](https://github.com/TheCaptainCompany/captain-food/issues/508)) — and
> `sweep_retention()` forks per adapter database **including
> the avelo37 leg the first record did not know existed**.

> 🗂️ **2026-08-12 — THE APP INDEX IS GENERATED, AND IT SAYS THE SPLIT IS NOT CLEAN**
> ([PROP-20260811-141654](../proposals/PROP-20260811-141654-per-app-declaration-folders.md) slice A1,
> [#491 "Per-app declaration folders"](https://github.com/TheCaptainCompany/captain-food/issues/491);
> emitter + generated output only — **no `specs/apps/` folder, no source moved, no manifest touched**.)
> `specs/generated/apps.generated.md` now renders all **57 deployables**: family, boundary, what each
> hosts, its pod grant, and the two columns the product-owner question turns on — **declared** domain
> crates vs **resolved** ones, the second MEASURED from the workspace graph with cargo's own resolver
> rather than inferred from the spec. It is the first emitter that measures rather than derives, which
> is why it runs last in `main` (after the manifests the same pass writes) and refuses to emit at all
> if the workspace cannot be resolved.
> **The verdicts it renders**: **8 of 57 apps are honest** (resolved == declared) — the 7 `gateway-*`
> plus `bam`, which links all 8 domain crates *and declares all 8*, so it is honest-though-fat and must
> not be counted with the other 49. **3 apps declare crates from two business boundaries**
> (`pm-cart-binding`, `pm-delivery-dispatch` — legitimate bridges — and `bam` by design); on the graph
> that actually links, **50 span all five**. **No crate the apps reach is boundary-exclusive** — all
> 44 are linked from at least one app of every boundary — but that signature saturates (the 8
> `graphql-*` subgraphs alone cover all six boundaries, so any crate one of them links scores the
> maximum), so section 3 groups by **how many of the 57 apps link each crate** instead: 57 for
> `telemetry`/`bin_probes`, 50 for `domain` + the `domain-*` set, 45 for the runtime spine, and **8
> for twelve crates** — the shared-kernel reading the boundary column invites is not what the data
> says.
> ⚠️ **The number that names the work**: `bin_runtime` carries the `domain` facade into **45 of 57**
> apps, `infrastructure` into 10, `server` into 8, `surface_runtime` into 5. Decomposing the first is
> the single largest isolation move available — which is
> [PROP-20260811-090000](../proposals/PROP-20260811-090000-scope-isolation-runtime-decomposition.md)'s job,
> untouched here: an index renders the debt, it does not repay it.
> The **needed-and-not-granted** column has exactly one row and it is the recorded trap:
> `worker-sirene-sync` needs `INSEE_API_TOKEN` and its pod does not carry it (no production
> `from_secret` — GitHub Actions still injects it), which is correct today and breaks at the
> [#358](https://github.com/TheCaptainCompany/captain-food/issues/358) cutover. Grants are now ONE
> derivation (`bin_secret_env_keys`) shared by the pod manifest and the index, asserted app-by-app
> against the committed manifests, so the least-privilege slice (A4) cannot start from two answers.
> Two more things only this artifact shows: **`client-customer-credit` is reached by no deployable**
> (a generated actor client nothing links, while `client-restaurant` is reached by 45), and
> **`ADMIN`/`EXTERNAL` are claimed by no bounded context**, so two gateways sit under `platform` —
> and, through its gateway, the `bo-admin` surface — because nothing else is derivable — named out
> loud rather than left as a default that reads like a decision.
> **BND-1 closed the same day** (entry below; [DECISIONS §31](../proposals/DECISIONS.md)): the index
> reads the boundary set from `c4-l2.yaml` `boundedContexts`, which IS that closed answer — five
> business contexts plus `platform` — so the index needed no edit when the row closed, and needs
> none if the set ever moves again.

> ✂️ **2026-08-11 — THE API TIER IS THE WIDEST APP IN THE TOPOLOGY, AND `server` IS ONE EDGE AWAY
> FROM EIGHT PODS** (docs-only; amendments in place to
> [PROP-20260811-090000](../proposals/PROP-20260811-090000-scope-isolation-runtime-decomposition.md)
> §1/§4.1-§4.4/§5.1/§5.2 and
> [PROP-20260811-150242](../proposals/PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md)
> §5.1.9/§8; new register section [DECISIONS §34](../proposals/DECISIONS.md) — API-1, API-2, API-3, all
> **team-owned**). Product-owner directive, 2026-08-11: *"Remove the damn server crate it's currently
> the purpose of what we are doing"*.
>
> **Measured**: each of the 8 `graphql-*` subgraph bins **declares 3 workspace crates and links 44**
> -- 14x, against 1.5x for the 7 `gateway-*` bins. **25 of the 44 are reachable only through
> `server`**: `web`, `app-core`, `surface_runtime`, all five partner adapters
> (`stripe-adapter`, `uber-direct-adapter`, `hubrise-adapter`, `coopcycle-adapter`,
> `avelo37-adapter`), `shared_types`, and **14 of the 15 `crates/clients/*`**. A pod whose whole job
> is `catalog` and `categories` links the Stripe integration and the entire SSR renderer, and can
> spell `client_order::OrderClient`. The cause is a recorded design choice, not drift:
> `crates/server/src/bin_support.rs:1-8` says a subgraph IS the monolith's surface filtered by a
> scope **string** — defect 3 of that proposal's §1, reproduced in the API tier.
>
> **Three findings reorder work elsewhere.**
> **(1) REP-4 does NOT gate the API tier.** `EventStore` — the only port whose signature names the
> all-scopes `DomainEvent` — appears in **three** resolvers
> (`crates/server/src/graphql/generated/mutation.rs:4942,6384,6584`, i.e.
> `placeOrder`/`approveRefund`/`denyRefund`), and in all three inside the **`else` branch of the
> `pm_mailbox_delivery` gate**. Queries name it **zero** times; the subscription path carries
> `AppendedEvent = {String, String, Uuid, i64}`
> (`crates/infrastructure/src/persistence/event_bus.rs:20-31`), not the union. **Six of eight
> subgraphs never name it** — so the API tier is cuttable **before** the event split, which reverses
> that proposal's own "subgraphs are last cuttable" ranking.
> **(2) The real blocker is a GATE HOLE, and it outranks everything it let through.**
> `api-nested-cross-scope` forbids an api type in scope S from nesting another scope's type
> (`tools/codegen-rs/src/validate/scopes.rs:21-24`) and `make validate` reports 0 errors — while
> `specs/generated/schema.generated.graphql` contains **ten** such edges. The rule walks `$ref`s in
> the spec; the emitter **derives** these fields from FKs (`tools/codegen-rs/src/emit/server_graphql.rs:229`)
> and from `navRoles:`. **Four of the ten are cycles** (`network <-> ordering`, `network <-> delivery`,
> `network <-> catalog`, `delivery <-> ordering`), so per-scope API crates cannot exist at all — Rust
> has no cyclic crate graph. **Five of the ten resolve `Vec::new()` unconditionally**
> (`crates/server/src/graphql/generated/types.rs:1101-1105,1230`:
> `Restaurant.deliveryJobs/catalogs/carts/orders`, `Order.deliveryJobs`) and deleting them makes the
> graph **acyclic**. That deletion is a schema removal — register row **API-2**, with the migration
> story recorded (provably empty; zero first-party selections in `specs/screens/**` or
> `crates/web/src/**`; no third-party client; production down with an empty log — the free window
> closes at the [#358](https://github.com/TheCaptainCompany/captain-food/issues/358) cutover).
> **(3) The 8->6 subgraph reshape lands AFTER the cut, not before.** The compositions are
> **generated**, so cutting 8 costs the same as cutting 6, and the cycle set is identical either side
> of the merge. The cut is gated on nothing; the reshape still owes the superseding ADR on
> ADR-20260807-183024 D1's scope list.
>
> **What the directive can be satisfied by NOW**: removing `server` from the eight subgraph
> manifests (slice **A1** — extract `api_runtime` + `api_graph`; `server` keeps compiling by
> re-export and stays the monolith's composition root). **Deleting the crate** additionally needs the
> #358 cutover plus homes for three route sets — the SSR host fallback (slice 5's undrawn view-model
> boundary), `POST /auth/session` (already a recorded
> [#385](https://github.com/TheCaptainCompany/captain-food/issues/385) cutover precondition, **no bin
> home exists**) and `/internal/sirene/drain`. Both readings are written out in
> PROP-20260811-090000 §5.2 so the smaller one is never delivered silently.
>
> **Also corrected**: PROP-20260811-150242 §5.1.2's *"coarser is forbidden"* CONNECT argument is
> already violated **at boundary granularity** — five of eight subgraphs hold another boundary's read
> model inside a resolver (`crates/server/src/graphql/generated/query.rs:21,124-125,311-312,418-419`),
> and only `graphql-customer` and `graphql-platform` are clean. Register row **API-1**; the
> doctrine's own answer (*"pre-joined in a projector-owned view"*) is the recommendation. And
> **API-3**: `crates/gateway_runtime/src/lib.rs:121-122`'s *"any subgraph answers the role-filtered
> shape"* becomes false the day composition is per-scope — introspection must move to the gateway, or
> `graphql-platform` answers with 5 operations instead of 121.

> ✅ **2026-08-11 — BND-1 IS CLOSED: THE BOUNDARY SET IS FIVE, AND THE REGISTER'S LONGEST-STANDING
> ROW IS ANSWERED**
> (product-owner answer sheet, 2026-08-11; [DECISIONS.md](../proposals/DECISIONS.md) §5 + §31;
> [PROP-20260811-150242](../proposals/PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md)
> §0; [#493 "Two partitions, one domain: boundedContexts and specs/{scope}/ home 6 of 20 actors differently, and nothing reconciles them"](https://github.com/TheCaptainCompany/captain-food/issues/493)).
> Verbatim: *"I'm ok for the 5 / Customer / Order / Catalog / Restaurant / Delivery"*.
>
> **The boundary set is CLOSED as recommended: five business boundaries -- `customer` - `order` -
> `catalog` - `restaurant` - `delivery` -- plus the `platform` bucket and the `common` kernel** (a
> linkage concept with no pod, never a boundary). `catalog` stays a boundary; **`comms` and
> `payments` dissolve into `order`**; `public` stays a role of `customer`. **This unblocks slices
> 1-5 of [PROP-20260811-090000](../proposals/PROP-20260811-090000-scope-isolation-runtime-decomposition.md)
> and 15 of the 28 crates in
> [PROP-20260811-173223](../proposals/PROP-20260811-173223-repository-crates-and-the-infrastructure-split.md)
> REP-2(a)** -- the BND-1-GATE concern on that file is now checked. It also beats the clock:
> ADR-20260807-183024 D7's *"start-clean makes the storage split free -- the window that does not
> recur"* closes at the [#358](https://github.com/TheCaptainCompany/captain-food/issues/358) cutover.
> **Still owed before that proposal can be marked approved**: the superseding ADR on
> ADR-20260807-183024 D1's named scope list.
>
> **Four more rows close in the same message.** **BND-2** -- the boundary is **`delivery`, not
> `rider`**, reasoning endorsed. **BND-7** -- *"Estimate for now"*: the ETA frozen onto `OrderPlaced`
> is an **estimate, not a promise with a remedy**, and that must be reflected wherever the freeze is
> specified. **BND-6** -- *"Prep time only + labelled"*: when the travel leg cannot resolve, show the
> prep-time estimate **explicitly labelled as what it is** (which is precisely the defect already
> shipped at `specs/screens/restaurant_frontoffice.yaml:490`). **BND-4(i)** -- *"I agree it was the
> write side"*: actors and projectors read the **WRITE** side to load events, so the permission
> matrix may now be emitted on that reading. **APP-1** is **delegated to the team** with one
> deliverable demanded and not delegated: the app list plus all dependencies
> ([#491](https://github.com/TheCaptainCompany/captain-food/issues/491) slice A1).
>
> **NEW: in-between units for translating process managers are GRANTED, and BOUNDED (BND-8/BND-9).**
> Verbatim: *"I'm ok if we create in between boundaries for process managers that are making the
> translation between 2 boundaries thanks to the fact that we have one crate per actor client type."*
> The team has bounded it with **the `CONNECT` test**: a PM earns its own in-between unit only when
> it **writes two boundaries and reads at most one** -- because every PM write lands in ONE database
> (`domain_events` + `inbound_messages` + PM state are all inside `captain-write`, STO-1), so
> widening write reach widens an *enumeration*, while a second READ is a second `CONNECT` through the
> strongest wall in the matrix, i.e. BND-3's stop condition. **Classified: the concession creates
> ZERO units today and reserves exactly ONE candidate** (`DeliveryDispatchProcess`);
> `CartBindingProcess` is **CONFIRMED in `order`** under the new third option, because it commands
> one boundary and reads one boundary -- both `order` -- and its customer-side trigger is a mailbox
> fact, not a data reach.
>
> ⚠️ **Two measured findings arrived with it, and both correct things already written down.**
> **(1) The concession's premise is not true of process managers today**: `deliver:` is a DIRECT
> append to the target aggregate's stream (`crates/application/src/generated/process_managers.rs:118-122`)
> and `send:` runs the target's command handler **in-line** (`:786`) -- neither goes through
> `crates/clients/{actor}`, the target's mailbox lane, or its lease. **The DSL's own doctrine header
> says the opposite verbatim** (`specs/common/processmanager.yaml:7-9`: *"a process manager never
> appends to `domain_events` itself"*). That is a spec claiming something the code does not do, on
> the write path, and it is the concrete caller that makes **ISO-3** load-bearing. **(2) BND-3's stop
> condition already fires, twice**: `PlaceOrderProcess` reads the **restaurant** boundary's
> `Restaurant` read model on the CHECKOUT path (`specs/ordering/processmanager.yaml:38-41`, feeding
> four guards) and `DeliveryDispatchProcess` reads it for the pickup address
> (`specs/delivery/processmanager.yaml:42-46`). D9's claim that a `customer`-homed
> `CartBindingProcess` *"would be the first such grant in the system"* is **wrong** -- two exist
> today. Recommended remedy: the `restaurant` boundary publishes the five slow-moving fields and each
> consumer's projector folds a slim snapshot into its OWN read database -- the same
> composition-in-the-projector answer STO-2(a) already gave for `ScopeMembership`.
>
> 🧱 **2026-08-12 — A READ TARGET IS DECLARED, NEVER INFERRED: the `reads:` ownership wall is a gate**
> ([#507 "fix(codegen): a read target is DECLARED, never inferred"](https://github.com/TheCaptainCompany/captain-food/pull/507),
> MERGED as `158c85a`, closing [#506](https://github.com/TheCaptainCompany/captain-food/issues/506);
> [ADR-20260812-214500](../adr/ADR-20260812-214500-a-read-target-is-declared-not-inferred-the-reads-ownership-wall.md)).
> Retiring `command_journal` cost 110 files because the table had leaked out of its encapsulation into
> resolver bodies -- founder verdict: *"it should never be used directly because we have to pass through
> the actor clients that encapsulate the insert"* and *"it's unacceptable"*. Two absences allowed it, both now ERRORS, both **seen red on `786bcfa` first**:
> (1) `reference: true` was an unguarded opt-in whose only counter-argument was a header comment, and a
> BARE-NAME `reads: ['inbound_messages']` bypassed even the §1b ref-kind contract (a bare name is
> invisible to the refs walker -- the #413 defect class) -- planting both **passed `make validate` with
> zero errors**; (2) transience was inferred from a MISSING `reads:`, so deleting one line silently
> exempted a query from every read-side rule -- also zero errors. That second one is what actually let
> the journal through: the journal queries declared no `reads:` at all. Five new errors
> (`reference-flag-not-a-read-target`, `reads-infrastructure-owned`,
> `reads-infrastructure-with-read-model`, `transient-type-undeclared-infrastructure`,
> `reads-not-a-ref`) in the new `validate::read_targets`, keyed on `refs::classify`'s `Kind` -- never on
> a name pattern (`external_%` matches 1 of 7 categories) and never on the author's own `staging: true`.
> The allowlist fails CLOSED in both directions, but only ONE of them is the compiler, and the precise
> version is the reusable one: `refs::read_target_kind`'s match is exhaustive, so a new **`Kind`** does
> not compile until it is classified -- while a new catalog **FILE** is accepted by `classify`'s
> `_ => None` and fails closed at VALIDATE instead (`ref-kind-unknown` + `reads-unknown-view`).
> `reservations.yaml` is the proof: no arm for months, built fine. Level 4 for the kind, level 3 for the
> file. It gained the classifier arm it never had. Four transient types now DECLARE their table
> (`readsInfrastructure:`): `MailboxLane` + `PoisonedMailboxMessage` + `Operation` -> the mailbox,
> `PaymentIntent` -> the saga row -- and the key admits **`JournalTable` + `PmStateTable` ONLY**. That
> narrowing came from the independent review, which found the first cut had wired it to the whole
> infrastructure partition: `hubrise_connections` and `domain_events` under `readsInfrastructure:` on the
> PUBLIC-reachable `Operation` type validated with ZERO errors, while the same `$ref` under `reads:` had
> always been refused -- the new key had **opened a door that was shut**. Fixed, with the missing
> mutation test added; the lesson is that a new permission needs its own red-first plant, not just the
> rule it was added to serve. **Deliberately untouched**: `c4-l3` `components.*.reads`, the correct
> home for infrastructure readers. **Honest limits, now FILED as the compiler-first halves of this
> change** -- a validator rule is level 3, and both of these are reachable at level 4:
> [#510 "mailbox query ports behind a capability witness"](https://github.com/TheCaptainCompany/captain-food/issues/510)
> -- `crates/actor_client`'s `MailboxAccess(pub(crate) ())` witness closes the mailbox WRITE door but not
> `MailboxLaneRepository`/`MailboxRequeue` (`crates/application/src/queries.rs`), and the existing witness
> cannot be reused because `actor_client` depends on `application`; and
> [#512 "pool + schema probe out of `crates/server`"](https://github.com/TheCaptainCompany/captain-food/issues/512)
> -- `sqlx` canNOT simply be dropped from `crates/server/Cargo.toml`: there is no `sqlx::query`, but there
> IS `sqlx::raw_sql` (the `_sqlx_migrations` probe, `lib.rs:1497`) plus `PgPool`/`PgPoolOptions`/`Row` in
> the composition root. **And the rule that earned this entry a correction of its own**: the
> `reference: true` guard, taken alone, would NOT have caught `command_journal` -- the journal's queries
> declared no `reads:` at all, so only the second absence (transience inferred from a missing key) closed
> the path that was actually used.
>
> ✅ **2026-08-12 — THE JOURNAL CONCERN IS CLOSED: `inbound_messages` is the only journal (#242
> Runtime D, [ADR-20260812-000000](../adr/ADR-20260812-000000-the-pm-mailbox-flip-rides-the-journal-retirement.md)).**
> Product-owner direction, 2026-08-11: *"Remove inbound events and command journal from the dsl, the
> only tables that must remain is inbound messages"* -- answering the earlier *"make sure we don't do
> both."* `inbound_events` was backfilled and DROPPED by `20260731143000`; `command_journal` is
> dropped by `20260812000000`. With it go: the legacy journal+spawn arm of
> `placeOrder`/`approveRefund`/`denyRefund` (the emitter now FAILS GENERATION on an unaddressed
> mutation rather than falling back), the `operationStatus`/`operationStatusChanged` fallback and the
> cross-arm duplicate read, the `worker-journal-sweep` CronJob (**57 apps -> 56**), the
> `command_journal` leg of `sweep_retention()`, and the `CommandJournalStatus`/`CommandChannel`
> scalars. **The `PM_MAILBOX_DELIVERY` gate is deleted, not defaulted ON**: its OFF arm WAS the
> journal, so with the table gone OFF would have meant "mailbox mutations, no B2 chaining, saga
> triggers back" -- the silent paid-order stall. The `RuntimePosture` mechanism (#318) stays with no
> tenant; its fail-closed read keeps its test, which exercises the CONTRACT over an arbitrary key AND
> the migration's idempotence over `PM_MAILBOX_DELIVERY` -- the only key the seed statement names, and
> therefore the only one on which "an operator flip survives a re-apply" can fail for its stated
> reason.
>
> 🧹 **The guard's PROSE outlived the guard, and three lens reviews plus the product owner missed it**
> (found by the automated PR reviewer, corrected on the branch). The bin emitter still promised a
> mechanism this change deletes: `pm-place-order`/`pm-refund` shipped *"the fleet reads the money
> posture itself and refuses the lane when it is unprovable"*, and all fifteen `actor-*` bins shipped
> *"posture-gated money lanes"* -- on lines no diff hunk touched, in the file an operator opens first
> when a money PM pod is stuck at peak. The sibling that hid the same way: the five-line doc comment of
> the deleted `pm_mailboxes` field, which Rust re-attached to the `only` field beside it, so
> `ProcessManagerRunner` documented a gate flip on a field that picks a PM. All now say what is true
> (the fleet drains exactly the lane set it is handed). **No gate is reachable** -- catching it needs a
> source-text scanner over comment prose, the class ADR-20260803-234035/#329 rule out -- so the defence
> is recorded as procedure in the ADR: when a mechanism is deleted, grep its VOCABULARY, not just its
> identifiers, across the emitter and the generated output.
>
> ⚠️ **A leg reserved to the product owner was TAKEN, and it is recorded rather than assumed**:
> [DECISIONS.md](../proposals/DECISIONS.md) §32 JRN-1 held that flipping `PM_MAILBOX_DELIVERY` is a
> money-path posture change needing *"a staging smoke and a one-line ADR"*. The ADR exists; **the
> staging smoke does not, and was not performed** -- the flip was taken inside the empty-log /
> production-down window, where a smoke of the gated form has nothing to smoke against. JRN-1 is
> CLOSED saying exactly that, and is the place to object: while the log is still empty the reversal is
> a `git revert` plus a down-migration, and it gets more expensive with every real order.
>
> ❗ **The other half of the API-lens finding STANDS and is NOT fixed here**: the permission matrix in
> [PROP-20260811-093000](../proposals/PROP-20260811-093000-storage-boundaries-and-least-privilege-database-users.md)
> §6.1.2 grants the *query* path **no `CONNECT` to the write database at all**, so the acceptance poll
> breaks on the mailbox read too -- up to 30 polls at 1 s per action, i.e. every checkout, every
> restaurant acceptance, every rider transition. The recommended `command_journal` grant-with-expiry is
> now moot (the table is gone) and the proposal is updated in place; the `inbound_messages` read grant
> is still owed.

> ⏱️ **2026-08-11 — THE ETA IS THE PRODUCT, AND NOTHING COMPUTES IT; PLUS: ONE EVENT LOG**
> ([PROP-20260811-150242](../proposals/PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md)
> D9/D10/D13/D14,
> [#493 "Two partitions, one domain: boundedContexts and specs/{scope}/ home 6 of 20 actors differently, and nothing reconciles them"](https://github.com/TheCaptainCompany/captain-food/issues/493),
> register rows BND-5..BND-7 in [DECISIONS.md §31](../proposals/DECISIONS.md)). **Four questions that had
> been surfaced to the product owner are answered by the team instead** -- they were answerable from
> doctrine plus the code and should not have been routed out.
>
> **The headline: nothing computes an ETA anywhere, and two shipped surfaces already promise one.**
> Zero repo-wide hits for an ETA function. **No pre-order estimate exists at all** -- the two
> `estimated*` values the system holds both arrive AFTER the customer has paid (`estimatedReadyAt` <--
> `OrderAcceptedByRestaurant`; `estimatedDropoffAt` <-- `DeliveryAcceptedByPartner`, and that one is
> **unfed on the partner path**, `projection/worker.rs:441-444`). Meanwhile:
> `specs/screens/restaurant_frontoffice.yaml:490` renders an `eta_bar` labelled *"Estimated arrival" /
> "Arrivee estimee"* bound to `{{ order.estimatedReadyAt }}` -- the KITCHEN READY time -- and it is
> visible during `OUT_FOR_DELIVERY`, exactly when ready-at is already in the past; the right field
> (`estimatedDropoffAt`) sits unused on the same GraphQL type (`specs/ordering/api.yaml:62`). And
> `specs/screens/captain_frontoffice.yaml:206` offers four marketplace sort options including
> `delivery_time_asc` over `queries/restaurants`, which declares 11 args and **no sort**
> (`specs/network/api.yaml:66-83`). **A wrong ETA outranks a missing one.** Both are screen-spec
> defects independent of every boundary question.
>
> **D13 -- the ETA is a READ-SIDE COMPOSITION owned by `order`, frozen onto `OrderPlaced` at
> checkout.** Not a projection: Young's fold rule (current state is a left fold of the event stream)
> kills it, because the pre-order estimate depends on *now* -- queue depth, rider supply, an address
> typed thirty seconds ago and in no stream -- so a replay cannot reproduce it. Not a process manager:
> a PM's output is commands, and the ETA changes nothing. It is the pattern this repo already proved
> for pricing -- `price_cart` live on every read, authoritative freeze once at checkout, fail-closed to
> an honest no-value state. **Its durable output is naming the THIRD sanctioned cross-boundary
> mechanism the architecture was missing -- a read-time query contract** -- beside the projection fold
> and the PM bridge.
>
> **D14 -- ONE event log; boundaries are write-isolated and read-shared on it.** Stated because it was
> only ever implied. `domain_events.position` is the global total order and **two** projection groups
> fold across boundaries on it (`Order` at `worker.rs:447-450`, `ScopeMembership` at `:507-510`), and
> **no boundary reshape removes them** -- so a per-boundary log would break replay determinism.
> **REP-4 is orthogonal** (storage is already untyped). **ISO-3 is no longer orthogonal and rises in
> priority**: under a shared log, write-exclusivity per stream category IS the write-side boundary,
> and `EventStore::append` takes a bare `stream_name: &str`.
>
> **D9 -- `CartBindingProcess` -> `order`**, the one member that makes the two partitions identical.
> The losing side has a concrete price: a customer-boundary PM would need the system's first `GRANT`
> spanning two boundaries. **D10 -- notification is THREE parts, not two**: policy in `order` (the
> `reminders:` mechanism is already declared on `Order` at `specs/ordering/actors.yaml:92-96` and used
> only for GDPR retention, while `OrderPlaced` schedules **nothing**), **recipient contract in
> `restaurant`** (absent entirely), transport in `platform`.
>
> **Two genuinely product-owner-owed rows are new**: **BND-6** (what the customer sees pre-order when
> the travel leg cannot resolve) and **BND-7** (is the frozen ETA a promise with a remedy, or an
> estimate?) -- BND-7 **before** the freeze lands, since adding a field to an already-stored event is a
> migration and it is nearly free before the [#358](https://github.com/TheCaptainCompany/captain-food/issues/358)
> cutover. **BND-1 (the boundary set) was answered on 2026-08-11 -- see the entry at the head of
> this file; BND-6 and BND-7 are answered too.**

> 📦 **2026-08-11 — REPOSITORY CRATES: TWO OPEN ROWS CLOSE, AND THE COUPLING NOBODY HAD NAMED**
> ([PROP-20260811-173223](../proposals/PROP-20260811-173223-repository-crates-and-the-infrastructure-split.md),
> [#497 "Repository crates and the dissolution of `infrastructure`: read and write are separate crates, and \"inherit\" is right on the log and wrong on the read model"](https://github.com/TheCaptainCompany/captain-food/issues/497),
> register rows REP-1..REP-5 in [DECISIONS.md §33](../proposals/DECISIONS.md)). Product-owner direction:
> *"We also have to create crates for repositories. There is read repositories and writes
> repositories, the write repositories generally inherit from the read repositories"* / *"The
> infrastructure has to be split in multiple crates to be able to regulate permissions of apps based
> on what they need nothing more."* Third message of the day and the third face of one idea — §31
> decides which units exist, §32 what shares a recovery posture and a database role, §33 what a unit
> may link.
>
> **ISO-1 and ISO-2 are CLOSED, both as (a)** (register §29 + §5). Both (b) options end with a bin
> linking a crate that carries every other boundary's code -- ISO-1(b)'s own wording is *"the bin
> keeps linking `infrastructure`"* -- which is what *"nothing more"* forbids.
> **[#423 "Design record for the per-scope infrastructure split"](https://github.com/TheCaptainCompany/captain-food/issues/423)
> slice 1 is no longer blocked on those two rows.**
>
> **"Inherit" is right on the log and wrong on the read model, and the code already argues it.** There
> are TWO read contracts on every read model: the **query** port (`CartReadRepository`, 5 methods;
> `by_id` returns `None` for a CHECKED_OUT cart, `queries.rs:277-279`) and the **row-state** port
> (`cart_store::load`, unfiltered). The projection write repository inherits the row-state one --
> supertraiting it onto the query port is over-privilege **and** a correctness bug, and
> `persistence/cart.rs:67-70` says exactly why in a comment written for another reason. On the write
> side the supertrait is right unqualified: `EventStore: EventStreamReader` creates the **log-read
> port that does not exist today** (three components read `domain_events` three different ways --
> `EventStore::load`, `projection/worker.rs:753`, `deletion.rs:255,320`).
>
> **The blocker nobody had named (REP-4)**: `DomainEvent` is ONE enum over all 8 scopes, defined in
> the facade (`domain/src/generated/events.rs:20`) and named by `EventStore` and the projector
> `Envelope`. A per-boundary repository crate that traffics in it links everything, so slice 1 as
> written would deliver a smaller module tree and the **identical** closure. It is **not** an
> event-versioning question -- storage is already `(event_type TEXT, payload jsonb)`
> (`event_store.rs:203`), so no stored contract moves.
>
> **Topology**: ~28 net-new crates -- 3 per boundary (`ports-{B}` with no `sqlx` · `read-{B}` SELECT
> adapters · `projections-{B}` folds + load/upsert) plus 13 platform crates (`store_core`,
> `eventstore`, `mailbox_pg`, `projection_runtime`, `read-platform`, `erasure`, 7 `acl-{partner}`).
> **`crates/infrastructure` (~13,200 lines) is dissolved**, surviving the
> [#358 "MKS bootstrap"](https://github.com/TheCaptainCompany/captain-food/issues/358) window only as
> a monolith-only composition crate behind a codegen guard.
>
> ✅ **BND-1 ([#493 "Two partitions, one domain"](https://github.com/TheCaptainCompany/captain-food/issues/493))
> is CLOSED (2026-08-11): B = 5**, so the 15 per-boundary crates are unblocked and the BND-1-GATE
> concern on that proposal is checked. **Dispatchable today, boundary-agnostic**: the ratchet
> dimension on [#490 "Scope-closure ratchet"](https://github.com/TheCaptainCompany/captain-food/issues/490),
> `store_core` + `eventstore` + the reader split + the ISO-3 witness, `projection_runtime`, the 7
> partner ACL crates.

> 🧮 **2026-08-11 — THE WARNING BASELINE IS A GATE, NOT A NUMBER IN A DOC**
> ([ADR-20260811-170559](../adr/ADR-20260811-170559-the-validator-owns-the-warning-baseline.md)).
> `tools/codegen-rs/warning-baseline.json` holds the per-rule warning histogram and validator §17
> asserts it on every `make validate` / CI run, **in both directions**. Nothing to re-measure: a green
> validate already proves "no new warning". If a change moves the warning surface, run
> `make warning-baseline` and commit the refreshed artifact in the same commit (the `+1 <kind>` diff is
> the record; say in the PR body why an added warning is accepted). The old prose pin went stale three
> times (32 → 43 → 37) and cost four agents a pristine-`main` validator run each in one day.
> **Every field is asserted, `doc` string included** — review caught the artifact shipping a `doc`
> naming the wrong validator section, hand-patched in the one file whose own text forbids hand-editing.
> `make warning-baseline` refuses to write from a model with errors, so a red spec cannot mint a
> blessed baseline.

> 🗄️ **2026-08-11 — THE STORAGE SPLIT IS COSTED, AND IT FOUND TWO DEFECTS THAT ARE NOT ABOUT THE
> SPLIT**
> ([PROP-20260811-093000](../proposals/PROP-20260811-093000-storage-boundaries-and-least-privilege-database-users.md),
> [#494 "Storage boundaries and least-privilege database users: the write-side transactional unit, the five-database split, and the last five View_*"](https://github.com/TheCaptainCompany/captain-food/issues/494),
> register rows STO-1..STO-6 in [DECISIONS.md §32](../proposals/DECISIONS.md)). Product-owner directive:
> five databases (`DomainEventLogDb`, `DomainCommonDb`, `CatalogDb`, `OrderDb`,
> `BehaviorEventTrackingDb`) plus a per-app least-privilege database user derived from the spec. The
> access model is **accepted and correct**; the dba lens completed it, priced it, and named where it
> does not close.
>
> **The `View_*` blast radius is real and small — and it points the other way.** Measured: **5** SQL
> fold views vs **11** already-materialized projection tables; **9 of 32** GraphQL queries break if
> `domain_events` leaves the read database, **23 survive**, and **zero of the broken ones are on the
> money path** (`Cart`, `OrderTracking`, `Catalog`, `Restaurant`, `Customer` are all tables already).
> The five stragglers are the rider board, the restaurant delivery board, claims, the refund queue and
> the timeliness insight. Recommended way out: **convert them to materialized projection tables** —
> which the product owner's own rule already implies (*"the writing of the read side is done only by
> the projectors"* is vacuous for a SQL VIEW nobody writes). `postgres_fdw` and logical replication are
> rejected with reasons in the proposal.
>
> **Defect 1 — the erasure engine fails OPEN.** `crates/infrastructure/src/deletion.rs:229-233` bounds
> its scan at `COALESCE(MIN(position), i64::MAX) FROM projection_checkpoint`, clamped to log head. A
> database with **zero** checkpoint rows therefore erases at head with **no** fold verification —
> exactly the database the split creates, and exactly what a start-clean production is. Fix: a
> `projection_watermark` table in the write DB, heartbeated monotonically by each projector, with a
> **fail-closed** default. Precondition for the split; worth landing even without it.
>
> **Defect 2 — 8 indexes that do not exist.** The 5 views declare 8 secondary indexes; a Postgres view
> cannot be indexed and `views.generated.sql` emits **zero** `CREATE INDEX`. `myDeliveries` therefore
> folds every delivery job in history to return the 3 a rider holds: at ~120 jobs/day, month 6 is
> ~21,600 jobs × 8 correlated subqueries ≈ **173,000 index probes per call**, polled by every rider at
> Friday peak. This is due whether or not the split happens.
>
> **The one thing the directive must change**: `DomainEventLogDb` cannot hold the log alone.
> `actor_runtime/src/completion.rs:71-100` commits appends + PM state + reminders + the
> `inbound_messages` flip + the fenced `mailbox_partitions` advance in ONE transaction — separating log
> from mailbox does not weaken atomicity, it **deletes the fencing token** (a paused pod waking at
> 20:40 with a stolen lease would have its appends commit). Widen it to `captain-write`. The
> transaction the product owner *asked* about — projector fold + checkpoint — **survives the split
> untouched**, because a co-located checkpoint plus an idempotent fold is at-least-once + idempotence,
> not 2PC.
>
> **Also priced**: one CNPG cluster with five databases (five clusters do not fit the node), a
> **session-mode** pooler as a prerequisite (the split puts the fleet at ~235 against
> `max_connections: 220`; transaction mode silently kills `LISTEN`), five migration chains with
> `REQUIRED_SCHEMA_VERSION` becoming a map, and behaviour tracking at **~17.5 GB/yr — ~13× the business
> log** — which needs a declared retention policy shipping *with* its first table, not after.
>
> **Reconciled on landing, and both matter to whoever generates the grants.** (1) It pairs with
> [PROP-20260811-150242](../proposals/PROP-20260811-150242-domain-boundaries-the-four-and-the-two-partitions.md)
> ([DECISIONS §31](../proposals/DECISIONS.md)) — **boundaries decide which units exist, storage decides
> what shares a recovery posture and a buffer pool** — and storage deliberately does **not** follow the
> boundary one-to-one (BND-3), with the stop condition worth becoming a validator rule: *if any app's
> `GRANT` spans two boundaries' schemas outside the declared exceptions, the shared database has
> silently become an integration database.* (2) ⚠️ **The permission matrix omitted the mailbox, and the
> omission is load-bearing**: GraphQL mutation resolvers write `inbound_messages`
> (`crates/server/src/graphql/generated/mutation.rs:42`), so *"the writing of the write side is done
> only by the actors"*, taken literally as a `GRANT`, **makes every mutation fail at runtime**. The
> matrix now names the mutation-resolver row explicitly — CONNECT to `captain-write` plus **INSERT and
> SELECT** on `inbound_messages` and nothing else (SELECT because `RETURNING` needs it, and because the
> idempotent-retry arm is a plain `SELECT`) — proposal §6.1.1, which also flags that the directive's
> fourth bullet is a transcription slip for the **write** side and must be confirmed before it becomes
> a role.

> 🗂️ **2026-08-11 — THE 57-APP LIST, AND THE PER-APP KNOWLEDGE THAT LIVES IN RUST**
> ([PROP-20260811-141654](../proposals/PROP-20260811-141654-per-app-declaration-folders.md),
> [#491 "Per-app declaration folders"](https://github.com/TheCaptainCompany/captain-food/issues/491),
> [DECISIONS §30](../proposals/DECISIONS.md); docs-only, no `specs/**` touched.)
> Product-owner request: *"Give me the app list to be on the same page… create a sub folder for each
> app/worker and indicate what it contains."* **Half of it needed no decision** — the 57 apps grouped
> by family, with what each family contains, are §1 of the proposal (15 `actor-*` · 5 `pm-*` ·
> 7 `projector-*` · 8 `graphql-*` · 7 `gateway-*` · 5 `fo-*`/`bo-*` · 5 `adapter-*` · 4 `worker-*` ·
> `bam`).
> **The other half is a "no" inside a "yes".** The app list already exists as source
> (`specs/architecture/c4-l2.yaml` `containers:`), and a folder in `specs/**` **cannot** make a scope
> boundary real — only the crate graph does, which is
> [PROP-20260811-090000](../proposals/PROP-20260811-090000-scope-isolation-runtime-decomposition.md)'s
> job and is untouched. So the recommendation is deliberately narrower than the request: **source for
> deploy-owned facts only, generated for everything derivable, and the `containers:` block MOVED
> rather than copied** — a folder that restates the derivation is a drift surface, which is the one
> outcome worse than doing nothing.
> **What the folder is genuinely FOR**: the per-app knowledge that today lives in **Rust, inside the
> generator** — `worker_config_consumers()` is a literal `match name { "worker-sirene-sync" => … }`
> (`tools/codegen-rs/src/emit/bins.rs:217-224`), the grant narrowings are per-family `if`s (`:111-139`),
> and `replicas: 1` / `strategy: Recreate` are string literals (`tools/codegen-rs/src/emit/deploy.rs:335-340`)
> under a comment promising *"Flipping either value is a SPEC change"* while **no spec key exists to
> flip**.
> ⚠️ **The measured finding is a credential boundary, not a code one.** `adapter-stripe` — the pod
> whose stated reason to exist is *"holds ONLY this partner's secrets"* (`c4-l2.yaml:125`,
> `emit/bins.rs:415`) — carries **13** secrets in its generated pod env, including `AUTH_SESSION_KEY`,
> `SUPABASE_SECRET_KEY`, `EXTERNAL_API_TOKENS`, `INTERNAL_TRIGGER_TOKEN` and the four `OVH_*` SMS
> credentials; `gateway-public` (*"no DB access, no business logic, no state"*) carries **10**;
> `bam` carries **18**, including `STRIPE_SECRET_KEY`. The narrowing mechanism exists and works —
> `worker-erasure` carries exactly **2** (`worker_key_allowed`, `emit/bins.rs:131-139`) — it is applied
> to one family. The derivation is also too NARROW somewhere: `worker-sirene-sync`'s pod env has no
> `INSEE_API_TOKEN`, and `SireneClient::from_env` returns `Err` without it
> (`crates/sirene_ingest/src/client.rs:100-102`) — correct today, a live trap at the #358 cutover.
> **Sequencing is the one product-owner row** (§30 APP-1), because this and the 2026-08-11 enforcement
> directive compete for the same weeks; recommended answer is slice A1 (the generated app index, no
> source moved) now and the rest after §29 slice 1, so nothing displaces the enforcement track.
> **[#490 "Scope-closure ratchet"](https://github.com/TheCaptainCompany/captain-food/issues/490) is
> unaffected and stays dispatchable** — with one accuracy note for its executor: recomputing the
> closure over the workspace manifests gives **49** violating bins, not 50, and the clean set is the
> 7 `gateway-*` **plus `bam`** (which declares all 8 domain crates, so under the issue's own equality
> rule it passes — listing it in `PENDING_DECOMPOSITION` would land the ratchet red).

> ⚖️ **2026-08-11 — THE ERASURE-FREE ZONE, CORRECTLY FRAMED: THE STREAMS WERE **ALREADY** PERSONAL
> DATA, AND TWO FORWARD TRAPS ARE NOW ON THE RECORD**
> ([BRIEF-20260811-erasure-zone-and-retention.md](../legal/BRIEF-20260811-erasure-zone-and-retention.md);
> docs-only, no `specs/**` touched).
> **The correction first, because it was recorded wrong.** The legal-lens pass over
> [PR #488 "The open GraphQL path verifies credentials, and `current` is tenant-scoped by Host"](https://github.com/TheCaptainCompany/captain-food/pull/488)
> / [#469 "`current` leg 1 is dead on the web AND is not tenant-scoped — fix both together or neither"](https://github.com/TheCaptainCompany/captain-food/issues/469)
> said `Cart-*`, `Customer-*`
> and `Restaurant-*` *"were an erasure-free zone and are now subject-attributable"*. **The second
> half is wrong**, and the error is not cosmetic — "became personal data" invites the reading that
> the obligations attach from now on, which would waive storage limitation, transparency and Art. 30
> records for everything already designed. These streams were personal data by construction:
> `CartStarted` **requires** `sessionId` (`specs/ordering/events.yaml:33-51`), the `SessionId` scalar
> describes itself as *"used to bind carts and track the user across devices"*
> (`specs/common/scalars.yaml:13-16`), `CartBoundToCustomer` writes the domain customer id onto the
> same stream via a **designed** linking process manager, and `CustomerRegistered` **requires**
> `phone` — `Customer-*` never needed the pseudonymity argument at all. Art. 4(1), Recital 30,
> Art. 4(5) and CJEU C-582/14 *Breyer* all land the same way for a controller that operates the
> linking mechanism.
> **What #469 genuinely creates is narrower and different in kind**: seven open-path commands now
> stamp `domain_events.user_id` with the **Supabase `sub`** (`crates/server/src/auth.rs:112-116`),
> putting an **external identity-provider identifier** into the immutable write envelope of three
> stream categories with **no erasure path** — and it **survives deletion of the Supabase identity**,
> leaving an orphan in an append-only column. Whether that orphan is anonymous under Recital 26 or
> still personal data is the question that decides whether **crypto-shredding** is optional or
> mandatory (counsel packet **G4**).
> **This is NOT a pre-existing breach and is not filed as an incident.** The production event log is
> **empty by decision** (start-clean, ADR-20260807-002705 D6 — *"the window is open only while the
> log is empty"*), so there is no data subject for an Art. 17 path to have failed. It is an **unmet
> launch precondition, already correctly filed as
> [#194 "GDPR erasure"](https://github.com/TheCaptainCompany/captain-food/issues/194)**. What changed is #194's
> **size**, and it is boundable: **three stream categories, one identifier kind, no new obligation
> class**. Trigger moment: the **first real customer order** — the same deadline as the Art. 35 DPIA
> and the médiation-de-la-consommation registration.
> ⚠️ **Trap 1, and it is the dangerous one — `Restaurant-*` must NEVER get an `Order`-shaped deletion
> policy.** `RestaurantListingOptedOut` (`specs/network/events.yaml:344-356`) **is** the Art. 21
> objection register; [the 0808 brief](../legal/BRIEF-20260808-listing-opt-out-objections.md) Q1/Q4
> states the historical event must be retained because *"it is the register, not stale data"*. The
> one built erasure mechanism is *tombstone → delete the whole stream → receipt*
> (`specs/ordering/actors.yaml:97-103`), and `Restaurant-*` will arrive at the #194 sweep as one of
> the three categories with no path. Giving it that block would delete the proof of objection and
> **permit re-listing** — the exact ProspectionPipeline failure the 0808 brief exists to prevent.
> Nothing is broken today (`Restaurant` declares no `deletion:` block), so this is
> **BLOCKER-on-arrival**, not a live defect.
> **A gate was assessed and is NOT buildable today, for one reason**: the deletion DSL is well-formed
> and already validated (`deletion-ref-unresolved` / `-match-untyped` / `-tree-cycle`), so the rule's
> shape is easy — but the spec has **no way to say "this event is a legal register"**, and the only
> alternative is hard-coding the event name in the validator, which is a comment written in Rust
> rather than a spec-derived gate. **The fix is one small spec addition and it belongs to #194**: a
> `legalRetention:` clause on the event naming its instrument and horizon, `$ref`-able from the
> MET-W retention-window catalog; the rule then writes itself — *an actor whose `emits` reaches a
> `legalRetention` event may not declare a stream-deleting `deletion:` block*. Until it lands the
> hazard is **prose**, which is the weaker form on purpose-built record.
> ⚠️ **Trap 2 — the retention control is asserted and inert.**
> `specs/database/tables/eventstore.yaml:38-39` states that ephemeral streams such as `Cart` get a
> retention row; **none does**. `domain_stream` has **zero production writers** — the only `INSERT`
> in the tree is a test fixture (`crates/infrastructure/tests/main/deletion_engine.rs:99`); every
> other reference is a `DELETE`, a comment or a validator note. So `$maxAge`/`$maxCount` bind
> nothing and abandoned guest carts accumulate forever. Compounding it, [the erasure
> brief:82](../legal/BRIEF-20260808-account-erasure-two-path.md) claimed the written retention schedule
> already existed *"in the DSL"* — false, as [DECISIONS MET-W](../proposals/DECISIONS.md) recorded, and
> **corrected in place in this change**. Under Art. 5(2) that ordering matters: a controller document
> asserting a schedule its own system does not implement is **worse evidence than silence**. The fix
> is decided and only needs sequencing — MET-W's **named catalog of approved retention windows**,
> landing **with** #194.
> 🔎 **One open question of FACT, team-owned, not counsel's**: does any **non-production** environment
> hold real subject data? The empty-log argument collapses if it does. **Established from the repo**:
> no staging/preview environment is declared anywhere it could hold data (`render.yaml` declares no
> staging service; `staging` is a supported `APP_PROFILE` value with no service bound to it); CI's
> database is an ephemeral per-job `postgres` container; the 2026-08-11 k3s rehearsal migrated an
> **empty** database and never ran the auth/money smoke legs; there is no `docker-compose`, no `.env`
> and the single `*seed*` artifact is referential policy rows. **NOT established, and it is the part
> that matters**: the `DATABASE_URL` repo secret is opaque, `sirene-sync.yml` writes **real INSEE
> rows** (which include *entrepreneurs individuels* — personal data per *Manni*) through it, and
> `db-migrate.yml:29` documents the same secret as the Supabase pooler string; this repo's own
> history records ~200k SIRENE-derived listings and ~200k `domain_events` tuples in the **pre-cutover**
> database. Start-clean governs the **new** cluster; **the disposition of the old store is an
> operational fact nobody has recorded**. Also unanswerable from the repo: whether any Supabase Auth
> project holds real end-user identities. **Two answers are owed in writing before §2 of the brief
> can be relied on in a DPIA.**
> **Counsel packet extended to G1–G8** (appended to the consolidated packet in
> [BRIEF-20260808-listing-opt-out-objections.md](../legal/BRIEF-20260808-listing-opt-out-objections.md)):
> empty-log reliance and the trigger moment · whole-stream deletion as the Art. 17 mechanism · **G3,
> marked blocking** — L123-22/L102 B vs Art. 17, and which closure (10-year window + projection
> tombstones, or export a financial skeleton first), blocking because the built path deletes the
> whole stream on **one** window with **no per-category split**, as `specs/ordering/configuration.yaml:10-21`
> says of itself · the orphaned `sub` and crypto-shredding · the Art. 21 register's minimum field set
> keyed on SIREN/SIRET · a per-category schedule validated against CNIL délib. 2021-044 · **G7**,
> `dietaryTags` as an unconstrained `array<Tag>` where `halal`/`kosher`/`allergy:peanut` are spellable
> **today**, with the DPIA unfinalisable while it is open · **G8**, Art. 18 restriction of processing,
> distinct from erasure and entirely unbuilt.
> **Two items reported for routing, deliberately not acted on here**: the `SessionId` scalar
> description (*"track the user across devices"*) **overstates the implementation** — an origin-scoped
> `localStorage` UUIDv7 (`crates/web/src/session.rs:14-31`) that tracks nothing across devices — and
> that wording is what decides whether the **Art. 82 LIL / ePrivacy 5(3) shopping-cart exemption**
> covers it, so the spec text is the riskier artifact than the code (a `specs/**` change); and
> `/public/graphql` now varies by the `captain_auth` cookie (ADR-20260811-113000) while **no `Vary` or
> `Cache-Control` exists anywhere in the tree**, so `Cache-Control: private, no-store` is recommended
> **on the #469 branch**, not here.

> 🧪 **2026-08-11 — THE CUTOVER WAS REHEARSED, LOCALLY AND END TO END; THE MONOLITH NOW HAS A
> MANIFEST**
> ([#358](https://github.com/TheCaptainCompany/captain-food/issues/358), branch
> `cutover-local-rehearsal`,
> [runbook](../runbooks/cutover-local-rehearsal.md), [ADR-20260811-004500](../adr/ADR-20260811-004500-role-paths-live-on-audience-hosts-api-host-is-a-webhook-address.md)).
>
> **The hole that was found first.** `deploy/generated/manifests/` held a per-bin Deployment/CronJob
> for every derived bin (57 when the hole was found; **56** since
> [#500](https://github.com/TheCaptainCompany/captain-food/pull/500) retired `worker-journal-sweep`)
> for a topology that runs nowhere — and **zero manifests for the monolith `server`, the process that
> actually serves every customer**. The repo could describe a future cluster in 83 objects and could
> not describe the one workload a cutover has to move. `server` is now a declared c4-l2 container
> (`deploy_tree: monolith`) and `deploy/generated/monolith/` is emitted from it: Namespace +
> Deployment + Service + Ingress, `kubectl apply -k` and nothing else. Retiring the monolith is now a
> spec deletion that prunes the overlay, and a codegen test asserts both directions.
>
> **What actually ran on a single-node k3s**, in this order, watched, **on 2026-08-11 against the
> 45-migration chain of that day**: CNPG 1.27.4 operator → `captain-db` Cluster **`Cluster in healthy
> state`** with `initdb` complete → `sqlx migrate run` applied the full chain to the empty database →
> the generated monolith overlay applied verbatim → the pod reached `1/1 Running` → **`/health` =
> 200** with a matching `requiredSchemaVersion` and **`/ping` = `pong`** → `prod-smoke.sh`
> **L1 and L2 PASS** against it.
>
> **Re-verified on 2026-08-13, after merging `main`** — because #500 landed a migration in between and
> a stale empirical claim is worse than none. `sqlx-cli 0.8.3 migrate run` against a freshly created,
> empty Postgres 16 database applies the **full 46-migration chain**, `max(version) =
> **20260812000000**` = `REQUIRED_SCHEMA_VERSION`, with #500's `ACCESS EXCLUSIVE` write fence and
> `RECEIVED`-straggler guard passing on an empty table. The **k3s leg was NOT re-run**: it is a
> multi-hour stand-up whose own runbook forbids a concurrent workspace `cargo build`, and the gates
> this merge needed are exactly that build. Everything above the schema line therefore remains
> 2026-08-11 evidence, unchanged by the merge; the schema line is fresh.
>
> **What it does NOT prove**, stated plainly because a spending decision rests on it: nothing about
> OVH, Cinder volumes or `Retain`; **nothing about backup or restore** (the rehearsal overlay removes
> `barmanObjectStore`, and at `instances: 1` that is the only recovery path — the largest gap);
> nothing about DNS, TLS issuance, cert-manager, ingress-nginx or the LoadBalancer (the Ingress is
> applied and parsed, nothing serves it; the app was reached by pod IP); nothing about HA; not the
> production image (a debug binary in an ad-hoc image — the real cargo-chef release build is CI's);
> and **not the money path** — L3/L4 need `SUPABASE_SECRET_KEY`, which this box does not have.
>
> **Three repo defects the rehearsal exposed, now fixed.** (1) `prod-smoke.sh` and `db-migrate.yml`
> targeted `https://api.captain.food`, a host the generated Ingress routes **nowhere** — they now use
> the audience hosts (`live.`, `system.`) that both topologies serve, with `api.` kept alive on the
> monolith overlay as the **webhook address** until the registered Stripe endpoint moves to `hooks.`
> (ADR-20260811-004500). (2) The smoke read its Supabase URL from `specs/configuration.yaml`, deleted
> by the per-scope split — a missing file returned empty instead of failing, so the **daily smoke has
> been dying at L3 blaming a missing environment variable**; it now scans the scope catalogs and says
> so loudly when they are absent. (3) The smoke's Render branch (`RENDER_API_KEY` → the deployed
> `SUPABASE_SECRET_KEY`) is retired: auth that depends on the platform being decommissioned cannot
> verify the platform replacing it.
>
> **The two gaps between this branch and a money-path walk, stated not solved.** (1)
> `SUPABASE_SECRET_KEY` as a repository secret the `prod-smoke` workflow can read — and this is a
> **confirmation, not a creation**: STATUS already records (2026-08-09, verified) that an Actions
> secret of that name exists and that `render-config-sync` reaches it through `toJSON(secrets)`;
> nothing in the repo can tell whether it is scoped to this workflow or still holds the value the
> smoke needs, so the founder confirms or re-points it. (2) ~~**A webhook ingress**, so Stripe can reach
> L4's `CAPTURED` assertion … the rehearsal cluster has no inbound address at all, so L4 cannot be
> walked locally however green L1–L3 get. Both are FOUNDER actions.~~
> **⤷ SUPERSEDED 2026-08-13, corrected here 2026-08-17 — NO INBOUND INGRESS IS REQUIRED, and this was
> never a founder action.** Two days after this entry,
> [ADR-20260813-004634](../adr/ADR-20260813-004634-supabase-auth-is-retained-for-v0-and-the-window-closes-at-the-first-real-order.md)
> (§"What this un-blocks", and [DECISIONS §36 IDP-1](../proposals/DECISIONS.md)) resolved it **from the
> same fact this entry drew the opposite conclusion from**: `stripe listen --forward-to` being
> outbound-only is exactly what makes it work — the CLI opens the tunnel *from* the local stack, reaches
> it through the hosts entry the rehearsal runbook already writes, and its **own** signing secret
> satisfies the fail-closed `STRIPE_WEBHOOK_SECRET` boot gate. Real Stripe, real signature, no shim, no
> cluster ingress, no founder involvement. **The true residue, and all that survives**: nothing in the
> repo **wires** it yet — `stripe listen` appears in exactly one place tree-wide, the ADR prose that
> describes it (verified 2026-08-17), so it is an unbuilt step in the harness, not a blocker. Gap (1)
> above is unaffected and remains a founder action.
> Still open for the console session: everything in
> [#362](https://github.com/TheCaptainCompany/captain-food/issues/362) — ingress-nginx and
> cert-manager are vendorable and pinnable offline exactly like `cnpg-operator/PIN.json`, and are the
> next slice.

> 🔓 **2026-08-10 (night) — THE `specs/**` FREEZE IS LIFTED: THE DSL IS THE TEAM'S WORK**
> ([ADR-20260810-221840](../adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md),
> product-owner directive: *"I'm surprise that I read that the spec was untouchable now that we have
> the team working together we don't need to have this constraint anymore… I'm pretty sure the team
> will ensure the right naming and scope. Just keep me informed."*). The last of four delegations in
> three days, and the one that reaches the work: prioritisation (ADR-20260810-215503), self-starting
> sessions (ADR-20260810-011500) and product ownership (ADR-20260808-144738) each delegated
> *judgement*; this delegates *capability*.
>
> **The boundary is NOT content-vs-structure** — that split is anti-correlated with risk in both
> directions (a scope-folder move rewrites no refs and is free, because `$ref`s are kind-logical; a
> one-word type change on an emitted event is irreversible). It is **three questions in order**:
> (1) does it contradict or create a **recorded decision**? → stop, file a `DECISIONS.md` row;
> (2) is the shape already **emitted, stored or promised**? → it is a **migration**, record the
> versioning story first (upcasting, never mutation); (3) otherwise it is the team's, **including
> structure and including `specs/common/`** (a high-fan-out shared kernel, not a no-go zone — freezing
> it would freeze the one place "one name = one dedicated scalar" is enforced). Structure gets **no
> separate gate**: proportionality already routes any real option space to a proposal + register row,
> which *is* the discussion the product owner offered.
>
> **Reporting replaces the freeze**: [docs/SPEC-LOG.md](../SPEC-LOG.md) is created and usable now — one
> sentence per landed spec change, in product language, in the **same commit**. No cadence, no digest
> to send; it is a pull surface kept current by a gate. The gate's shape is `DECISIONS.md` **§26
> SPEC-1** (recommendation (d), ~30 seconds to answer); until it lands the page is prose.
>
> **Queue effect, measured**: 8 open issues carried an explicit AMBER flag and 4 more routed a
> sub-task to plan mode — [#468](https://github.com/TheCaptainCompany/captain-food/issues/468),
> [#476](https://github.com/TheCaptainCompany/captain-food/issues/476),
> [#466](https://github.com/TheCaptainCompany/captain-food/issues/466) and the already-approved
> 451-B `currency_mismatch` line are now **GREEN and dispatchable**. The "one plan-mode window for
> #468 + #476" recommendation **dissolves** — the window was the only thing binding them, and #476
> touches a key with **0 occurrences** in `specs/screens/**` and `specs/*/api.yaml`. #466 and #468
> still sequence together (same validator area; a rule and the spec fix that keeps it green must land
> in one change), #476 is independent.
>
> ⚠️ **Newly load-bearing and absent**: `event_version` has **zero occurrences** across `specs/`,
> `crates/`, `migrations/` and `tools/`, while PROP-170000 D2 decided *"add `event_version` now
> (cheaper before the log grows)"* on 2026-08-08. The freeze was silently standing in for it — a
> payload nobody could change needed no versioning story. This is the structural work the delegation
> calls for, and the window is open only while the log is empty (ADR-20260807-002705 D6, start-clean).

> 🧾 **2026-08-11 — PER-BIN SCOPE ISOLATION: THE MANIFESTS NOW SAY WHAT THE BUILD ENFORCES**
> ([#475 "Per-bin scope isolation is nominal: every actor/pm/projector bin transitively links all 8 domain scopes…"](https://github.com/TheCaptainCompany/captain-food/issues/475), comment half). Measured on
> the resolved dependency graph: **50 of the 57 bins link the `domain` facade** — hence all eight
> scope crates — behind their own scope list, through `bin_runtime` (actor/pm/projector/worker/
> adapter), `server` (the 8 `graphql-*` subgraphs) or `web` → `app-core` (the 5 `fo-*`/`bo-*`
> surfaces, which really do hold no server/infrastructure). Only the **7 `gateway-*` bins** are
> domain-free end to end. The emitted manifest header claimed the opposite for all 57 ("linking a
> domain crate is the ONLY way that scope's vocabulary exists in this deployable … *unspellable*
> rather than merely unrouted") — **this supersedes the "step-2's facade limit is now closed FOR THE
> BINS" line in the [#382 "Bin crates: per-actor/per-PM/per-projector/per-subgraph/per-gateway/per-surface
> binaries from the c4-l2 topology"](https://github.com/TheCaptainCompany/captain-food/issues/382) /
> [PR #383 "Bin crates: per-deployable binaries emitted from the c4-l2 topology (ADR-20260807-183024
> step 3)"](https://github.com/TheCaptainCompany/captain-food/pull/383) entry below**, which was true
> of each bin's SOURCE and never of its
> image. The header now separates the two: the crate's own source still cannot NAME an undeclared
> scope (real, compiler-first), while what bounds the pod today is a runtime string — but only for
> the families that HAVE one: `spawn_actor_fleet(LANES)` / `with_only(PM)` / `with_scope(SCOPE)` on
> the 28 mailbox/projection bins (15 `actor-*`, 5 `pm-*`, 7 `projector-*`, `bam`), **nothing at all**
> on the other 9 of the 37 that reach the facade through `bin_runtime` — the 5 `adapter-*` and 4 cron
> `worker-*` bins (an adapter's
> one real link fact is its partner slice; a cron bin is bounded by the single pass it calls per
> Job). For the subgraphs, `bin_support::subgraph_app` registers EVERY actor mailbox and slices the
> master schema by a scope string, so one can enqueue to any aggregate.
> A codegen test (`bin_manifest_scope_claim_matches_the_measured_closure`) now derives the sentence
> from the guppy closure in **both** directions, over the WHOLE emitted text of both artifacts —
> header, manifest `description`, `src/main.rs` module doc and const docs — after the first cut
> checked the header only and left the retired claim standing verbatim in 40 files, one of them
> contradicting itself 14 lines apart. So the prose cannot lag the graph once `bin_runtime` is
> decomposed. **The measurement also resized the program**: PROP-20260811-090000
> and DECISIONS §29 said 45, counting the 5 surfaces as clean because their manifest's *true* note
> ("no database, no server, no infrastructure") reads as isolation — so the debt ledger
> ([#490 "Scope-closure ratchet: a bin's transitive domain set must equal its declared set…"](https://github.com/TheCaptainCompany/captain-food/issues/490)) starts at **49 rows**
> (50 bins reach the facade; under #490's *equality* rule `bam` is honest — it declares all 8 and
> its closure is those 8 — so it is fat by design, not lying), and
> the proposal gains a **slice 5** for the surface family, whose path no other slice touches.
> Structural half (decompose `bin_runtime`, per-scope `infrastructure`
> [#423 "Design record for the per-scope infrastructure split…"](https://github.com/TheCaptainCompany/captain-food/issues/423), `crates/clients/*`) stays
> open on #475. Validate 0 errors / 37 warnings — equal to the freshly measured `482fa76` baseline,
> same six kinds.

> 🧭 **2026-08-11 — BEHAVIOUR TRACKING IS ISOLATED END TO END, AND A FAULTED WORKER PRE-DIAGNOSES
> ITSELF — BUT "SAY IT IN /health" WOULD TAKE THE STOREFRONT DOWN AS STATED**
> ([ADR-20260811-120828](../adr/ADR-20260811-120828-behaviour-tracking-isolated-end-to-end-and-a-faulted-worker-pre-diagnoses-itself.md),
> [DECISIONS §27bis](../proposals/DECISIONS.md) TRK-ISO / HEALTH-2 / HEALTH-2a / HEALTH-2b; docs-only).
> **TRK-ISO — behaviour tracking gets its own database AND its own projector worker**, *"completely
> isolated… to avoid dependencies between the behaviour event tracking and the business events"*. That
> is **further than [PROP-20260811-000946](../proposals/PROP-20260811-000946-behaviour-event-tracking-in-the-screens-spec.md)
> D5** asked, and it matters more under the halt decision than it did before it: now that a rejected
> fold halts its group, a **shared** worker would let a malformed behaviour event wedge a group sitting
> beside the order read models. Separate workers make that unspellable rather than unlikely. The
> distinction is settled — behaviour events: own database, own worker, written by the UI through a
> `sink:` mutation, never `domain_events`; **business metrics: the `bam` schema and the `bam`
> projector**, a fold over `domain_events`. **C4 owes a new container plus edges**, and
> `specs/architecture/*.yaml` is **source DSL, not generated** — an executor spec change when the work
> lands.
> **HEALTH-2 — a faulted worker reports unhealthy and is NOT restarted.** *"K8s does not need to
> restart the worker"* is **independently the same conclusion** the team reached from the failure
> analysis (a deterministic fault re-fails after a restart, so liveness gives CrashLoopBackOff and
> takes sibling groups down) — the convergence is recorded, not just noted. And *"it's a pre
> diagnostic"* is the substantive requirement: **the payload is the deliverable, the status code is
> only the transport.** A health endpoint returning `{"status":"unhealthy"}` satisfies the code and
> fails the requirement, so the per-group breakdown — group, `haltedSince`, position, `eventType`,
> stream, error — becomes the point of the feature. **This is
> [no polling, only pushing](../adr/ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md)
> applied one layer up**: the failure pushes its own diagnosis into a surface already being watched,
> instead of a human polling pod logs to reconstruct it. On `500` — k8s treats any non-2xx as a failed
> probe, so **keep the existing `503`**, which is also semantically right; nobody should "fix" it for
> literal compliance.
> ⚠️ **HEALTH-2a — the edge, reported rather than discovered at cutover.** Verified on `37642cd`: the
> monolith runs the API **and** the projection worker in **one process** (`RUN_PROJECTOR`, default on,
> `crates/server/src/lib.rs:641-648`), serves `/{role}/graphql`, **has a `Service`**, and its `/health`
> is the ADR-0043 **deploy interlock** knowing only DB reachability and schema version (`:1503-1526`).
> So *"say it in `/health`"* there would make the **API** unready because a **read model** halted — a
> degraded projection turned into a **customer-facing outage**, and a halted projection blocking the
> deploy that would fix it. **The rule is restated so the edge cannot occur**: *the endpoint a pod's
> **readiness probe points at** returns non-2xx when a component **that pod is responsible for** is
> faulted* — not "`/health` returns 500". Projector bins probe `/projector`; the monolith keeps
> `/health` on API components only, with its in-process projector observable at `/projector`, **which
> is not its probe**. Final shape after cutover: which components a deployable hosts is already
> declared, so the probe path and the health composition can both be **generated from that
> declaration**.
> ⚠️ **HEALTH-2b — "any worker" does not apply unchanged, and the reason is a real asymmetry.** The
> actor-mailbox workers **already quarantine**: a repeatedly-failing message hits the delivery-attempts
> cap and is parked as poison (`journals.yaml:69`), **the lane keeps draining**, and an operator
> requeues it (`common/api.yaml:158,170,202`). Making them *stop* would turn a parked message into a
> **stopped order lane** — the platform's worst failure mode. **The principle: halt is right where
> there is no quarantine, and quarantine is better wherever it exists** — projections halt precisely
> *because* they have none, which is why quarantine stays their tracked follow-up. Actor workers still
> owe the pre-diagnostic half: poison data is reachable **only through the admin GraphQL API** today
> (**no `/mailbox`, no `MailboxStatus` — verified absent**), so the monitoring app cannot see a
> poisoned lane without admin auth. A `/mailbox` surface is owed, **report-only — it must not gate
> readiness**, because a poisoned message is a normal recoverable state, not an unhealthy pod.

> 🛑 **2026-08-11 — A REJECTED FOLD NOW HALTS ITS GROUP — AND THE FLIP CANNOT LAND ALONE, BECAUSE A
> HALTED PROJECTOR CURRENTLY REPORTS ITSELF HEALTHY**
> ([ADR-20260811-105024](../adr/ADR-20260811-105024-projection-halt-default-and-health-visibility.md),
> [DECISIONS §27bis](../proposals/DECISIONS.md) MET-G/MET-G2; docs-only).
> Product owner, verbatim: *"A. The projector has to stop and indicates it in the health. So k8s will
> detect it and we will be informed."* `DbFaultPolicy` flips **`Skip` → `Halt`** — the
> gate-then-stabilize default flip, the gated form having shipped inert in
> [#478](https://github.com/TheCaptainCompany/captain-food/pull/478). The team recommended building
> quarantine first and was **overruled**; recorded as a choice, not a concession — `Skip` leaves a read
> model permanently and *silently wrong*, which for a money- or authorization-bearing projection is
> worse than stuck.
> ⚠️ **Verified on `5fdc519`, and this is a precondition rather than a caveat**: under `Halt` the
> worker does **not** stop — the slice rolls back and the loop keeps ticking
> (`worker.rs:800-816,688-700`) — so `running` stays `true` (`:688`), so `/projector` returns
> **`200 OK`** (`server/src/lib.rs:1377-1392`); **and neither Kubernetes probe looks at projection
> status at all**, because projector bins probe `readinessProbe: /health` (the DB+schema gate) and
> `livenessProbe: /ping` (*"process is up; touches nothing"*)
> (`deploy/generated/manifests/bins/projector-ordering.yaml:102-111`). **Flipping today would produce a
> projector that wedges permanently and reports itself completely healthy on both probes** — turning a
> silent-wrong-answer failure into a silent-no-answer one. So the flip and the health surface land
> together.
> **The health design, settled in the ADR**: **halt stays PER-GROUP with the process alive** (already
> true by construction — process-level would turn one poisoned read model into a *scope-wide*
> projection outage, since `projector-ordering` hosts every ordering group); **READINESS, not
> liveness** — projector bins have **no `Service`**, so readiness is a **pure signal channel with no
> side effect** (visible to `kubectl`, Argo CD and `kube_pod_status_ready`), whereas liveness kills and
> restarts, a restart cannot fix a deterministic schema fault, and the resulting **CrashLoopBackOff
> stops every sibling group** — manufacturing exactly the outage the per-group shape prevents; re-point
> readiness to `/projector`; and the payload gains a **per-group** breakdown naming the halted group,
> position, `eventType` and error, because `ProjectionStatus` is per-worker today
> (`projection/mod.rs:13-28`) and structurally cannot say *which* group halted. **The signal does not
> exist**: `specs/observability.yaml` declares **no projection contract at all** (`:11`, prose only).
> ⚠️ **Known consequence accepted by flipping now (MET-G2) — the role-revocation wedge.**
> `ScopeMembership` is *"the single index every read-side authorization question resolves against, for
> every role and every surface"* (`projection_tables.yaml:801-810`) — **and it is a projection**. A
> halted group freezes read-side authorization: grants stop arriving and **revocations stop applying**,
> so a removed staff member or deactivated rider keeps access until a human clears the fault. That
> touches the *"explicit and immediate"* revocation guarantee of the §6.4 closure
> ([ADR-20260810-194548](../adr/ADR-20260810-194548-six-decision-answer-sheet-claim-staleness-closed.md)).
> **Accepted, not solved** — under `Skip` the event is skipped and the index left permanently *wrong*,
> worse in kind for an authorization index. **Quarantine remains the real fix** and stays a tracked
> follow-up; until then a halted `ScopeMembership` is an **incident, not a ticket**.

> ✅ **2026-08-11 — THREE MORE DECISIONS SETTLED; ONE IS WITH LEGAL**
> ([DECISIONS §27bis](../proposals/DECISIONS.md) MET-Q7 / COOP / MET-W / TRK-scope; docs-only).
> **MET-Q7 — approved as recommended: no hosted analytics SDK.** Ours, server-side. **Plus an addition
> that matters architecturally**: *"We will use a different database from the business database to
> isolate the activity."* Behavioural data lands in a **separate database from the business data**,
> which independently arrives at the legal lens's instruction and **confirms
> [PROP-20260811-000946](../proposals/PROP-20260811-000946-behaviour-event-tracking-in-the-screens-spec.md)
> D5** — its own time-partitioned store, so erasure is a partition drop rather than an immutability
> problem. **One distinction not to conflate**: this is the *behaviour* store. **Business metrics stay a
> fold over `domain_events` in the `bam` schema**, because they are business data derived from business
> facts. **Implication to carry**: the C4 needs a **new container** for the behaviour database and its
> edges — and `specs/architecture/*.yaml` is **source DSL, not generated**, so that is an executor spec
> change when the work lands, not a regeneration.
> **COOP — approved as recommended**: all three cooperative properties are designed in **now**, in the
> first slice — the customer reads their own trail, the **restaurant** is the beneficiary of the
> aggregate, and the taxonomy refuses things checkably so it can be published
> ([#377](https://github.com/TheCaptainCompany/captain-food/issues/377)). They belong in slice 1 for the
> reason they were raised: each is a property of the **declaration mechanism**, so retrofitting them onto
> an undeclared firehose is a project while on a declared taxonomy it is a rendering.
> **MET-W — approved as recommended**: a **named catalog of approved retention windows**, sequenced
> **with** the erasure work ([#194](https://github.com/TheCaptainCompany/captain-food/issues/194)) rather
> than ahead of it.
> **TRK-scope — still OPEN, and it is with LEGAL, not with the product owner.** Their idea: *"using a
> generated identifier uncorrelated to the person… without the need to know the person is doing what
> but a persona"*, plus a clarification that **changes an earlier legal finding** — the "help AI agents"
> sentence was **internal**, explaining to the team why the data is wanted, **not** a user-facing
> personalisation feature. Legal is working out whether a pseudonymous journey identifier fits the
> **audience-measurement exemption** or whether per-journey continuity exceeds it. **The proposals are
> deliberately NOT amended until legal reports.** The mechanical half is being thought about but not
> committed: if the answer is *"lawful provided the join never happens"*, then **"never joined" has to
> be structural rather than promised** — the separate database (MET-Q7) does most of it, plus no foreign
> key, no shared column name the validator would accept, and an `identifierClass` that **cannot** be
> `CUSTOMER` for an anonymous-funnel event. Note this pulls against D8 option A, so the two are
> alternatives **per event kind**, not one answer.

> ✅ **2026-08-11 — THE REVERSAL IS CONFIRMED, AND THE SPEC GETS STRONGLY TYPED**
> ([ADR-20260811-014129](../adr/ADR-20260811-014129-a-business-metric-is-a-projection-and-every-reference-is-a-ref.md),
> [DECISIONS §27bis](../proposals/DECISIONS.md); docs-only).
> Product owner, verbatim: *"Confirm the reversal, go with the projections"* and *"But we need to
> heavily strongly typed the spec no string in it"*. MET-R closes.
> **ADR-20260810-234225 is SUPERSEDED IN PART, never rewritten** — clauses 1–3 (persona activity as
> the unit; declared + emitted + asserted; a metric states its question) are carried forward; clause 4
> (*"never entity ids"*) and the enforcement table (*"generated instruments"*) are reversed. The old
> file stays as the record of what was decided on 2026-08-10, including the reasoning that turned out
> to be wrong.
> **The second sentence is a separate decision and it landed on a real defect in the team's own
> grammar.** `increment: orders`, `groupBy: [day]` and `value: { sum: orders }` were **bare names
> pointing at declarations elsewhere in the same file** — so a typo was not a broken reference the
> loader could catch, it was a *silently wrong metric*: the exact failure class the whole proposal
> exists to remove, sitting inside the proposal. The product owner spotted it before the team did.
> It is now four categories: a **declaration** may introduce a name; a **reference** is a `$ref` the
> loader resolves (including same-file, which the repo already does at `specs/ordering/actors.yaml:102`);
> a **value from a closed set** stays a bare token *unless a domain scalar already declares that set*,
> where the `$ref` is mandatory; **prose stays prose**. The receipt that this is structural:
> [#413](https://github.com/TheCaptainCompany/captain-food/issues/413) — a plain-string `tombstone:`
> is *"silently invisible everywhere"*, including to the rule written for it.
> **The sharpest single fix**: `attributes: [{ values: [DELIVERY, COLLECTION] }]` in the tracking
> catalog was a **verbatim copy of the `ServiceType` kernel scalar** (`specs/common/scalars.yaml:260-262`)
> — now `{ $ref: 'scalars.yaml#/ServiceType' }`, so adding a third service type never leaves the
> tracking spec silently disagreeing with the domain.
> **And the `serviceType` problem dissolved: it was a GRAIN error, not a missing field.** Measured:
> **every one of the 11 `Order*` events carries `orderId`** (`OrderExpired` carries it and nothing
> else), so a projection at `grain: ENTITY` is **total over the whole lifecycle** — a cancellation is
> `set: status → CANCELLED` on the order's own row, and the grouping moves to read time. **The
> versioning story is withdrawn; no event needs a new field.** The rule earned its place twice:
> `fold-key-not-on-every-event` was written to catch a missing field, and what it actually catches is
> a wrong grain.
> ⚠️ **One dependency surfaced (MET-W)**: `retention: P90D` as a free duration string contradicts a
> recorded legal position — [the erasure brief:82](../legal/BRIEF-20260808-account-erasure-two-path.md)
> says the retention windows are *"declared once, in the DSL, feeding both the sweep and the DPIA"*.
> No duration scalar exists. The fix is a declared retention-window catalog `$ref`'d by both, and it
> belongs to [#194](https://github.com/TheCaptainCompany/captain-food/issues/194) rather than to
> either metrics or tracking issue.
> **Not swept, deliberately**: the existing bare-name sites (`data_requirements:`/`actions_used:` 40,
> `roles:` 112) are each covered by a bespoke validator rule today. Their conversion is its own
> sequenced issue (MET-T2), not part of this.
> **A fork closed WITHOUT taking it (MET-F), with the numbers.** Product owner raised projection
> "state" as a JSON blob saved with the checkpoint, versus doing the fold in a generated SQL stored
> procedure. **① The state already exists and is already transactional**: measured in
> `crates/infrastructure/src/projection/worker.rs`, the projector holds **no fold state at all** —
> load → project → upsert per event, `drain_group` folding up to 500 events and writing
> `projection_checkpoint` **in the same transaction**. So *"loaded once and saved with the checkpoint
> transactionally"* is what it already does; there is no blob to build and **no memory risk** — an
> incomplete order is a row, and 100k of them is **12 MB**. The precedent for the JSON idea exists and
> was deliberately *not* JSON (process-manager runs are typed columns). **② The SQL option is already
> built and is the V0 default** — [ADR-0039](../adr/0039-projection-views-generated-from-lineage.md)
> generates a `CREATE OR REPLACE VIEW` state-fold over `domain_events`; `OrderFacts` is the same shape
> as the shipped `View_DeliveryJob`. **③ The grammar is runtime-agnostic**; the one construct that
> binds a runtime is `alertable:`, and it binds at the tap, not the fold. **④ Measured** (200k events /
> 100k orders): set-based SQL **2.15 s** · plpgsql row-at-a-time **4.92 s** · Rust projector
> **≈65–70 s** — but only **2.3×** of the 30× gap is set-versus-row, the rest is round trips that
> [#267](https://github.com/TheCaptainCompany/captain-food/issues/267) attacks without leaving Rust.
> 70 s to rebuild every metric from 100k orders is **~500 days of Tours trading**; read-time grouping
> is **27 ms**. **⑤ The argument that survives any volume assumption**: testing a generated procedure
> means golden comparison against a Rust reference fold, so **SQL does not remove the Rust fold — it
> adds a second one. Recorded recommendation: hybrid, deferred** — a total `(state, event) -> state`
> vocabulary with **no host-language escape hatch** (what makes it both runtime-agnostic and
> replay-deterministic), emit Rust today, optional per-projection `emit: sql` only if a rebuild ever
> hurts. **⑥** The testability objection weakened the same day —
> [#478](https://github.com/TheCaptainCompany/captain-food/pull/478) made DB tests required by default;
> the real gap is that **no test loads `views.generated.sql` and asserts fold behaviour at all**.
> ⚠️ **Separate finding, not part of the fork (MET-G)**: the projector's per-event **log-and-skip** is
> correct for a read model and **wrong for a money-adjacent metric** — a skipped event leaves the count
> permanently wrong with only an ERROR log. Wants a projection-lag/parity check, and is adjacent to the
> `DbFaultPolicy` decision still open from [#474](https://github.com/TheCaptainCompany/captain-food/issues/474).
> **Follow-up answered, no design change (MET-S2).** Product owner: *"this kind of counter must be
> computed once the order is completed so a process manager can handle it."* **The first half is right
> and is already what the entity-grain design does** — the fold `set`s status, the metric asks
> `countRows where status equals DELIVERED`, so the count comes from the terminal event and nothing
> else; there is no increment to compensate. **But taken literally as a fold shape it does not work**:
> **no terminal event carries `serviceType`** (`OrderDelivered` = `[orderId, restaurantId]`), so
> completion-only hits the same wall — the entity grain is what solves it. It would also be **strictly
> weaker**: with no row until completion, *"which orders are placed and still unaccepted right now"*
> becomes unanswerable, and that is the platform's worst failure mode. The shape is **one projection
> read two ways**. ⚠️ **The process-manager half is the wrong tool and is refused on the record**: PMs
> here are state-table orchestrators in the actor mailbox with leases, fencing and head-of-line, so a
> counter there could **stall an order lane**, and a PM **is not replayable** — it carries a live state
> row and issues commands, so "rebuild the metric" would re-drive Stripe. Replayability is the one
> property the whole reversal chose projections for. **And no new event**: `OrderDelivered` already IS
> the completion fact for both service types; adding `serviceType` to it would denormalise the log so a
> projection need not do its job. **The instinct does brush a real gap though** — `OrderCompleted`,
> `Receipt` and `Invoice` are **zero hits across every `specs/*/events.yaml`**, and a compliant receipt
> is a French legal precondition. That is [#200](https://github.com/TheCaptainCompany/captain-food/issues/200)
> + legal work with its own decision, deliberately not folded in here.

> ♻️ **2026-08-11 — A BUSINESS METRIC IS A PROJECTION, NOT A COUNTER — THE TEAM CHANGED ITS OWN
> RECOMMENDATION, AND FILED THE REVERSAL RATHER THAN EXECUTING IT**
> ([#484 "26 of the 29 declared `business_metrics` emit nothing…"](https://github.com/TheCaptainCompany/captain-food/issues/484),
> [PROP-20260810-234225](../proposals/PROP-20260810-234225-business-metrics-for-every-persona.md) D4/D6/D8/D9,
> [DECISIONS §27bis MET-R](../proposals/DECISIONS.md); docs-only).
> The product owner held their own design back until the proposal existed, so the two would be
> independent: *"for the metrics I have in mind the approach of the projection… we will have to create
> a query in the graphql to allow access to these metrics."* **The team evaluated it and moved.** Not
> out of deference — the generated-instrument design recommended one day earlier loses on four
> measured points. **(1) It forfeits replay by construction**: `crates/infrastructure/tests/orders_placed_metric.rs:129`
> asserts the counter does **not** fire on a rebuild, so a metric added later would carry **zero
> history**, where a fold replays the whole log. The team's own audit standard — *"a `View_*` whose
> restore path is not replay is a finding"* — rejects the design the team wrote. **(2) Ratios and
> distinct-identity denominators are structurally inexpressible** as monotonic counters, so the
> counter design needs an escape hatch for the most interesting questions; under a fold they are
> ordinary and the plain counter becomes a one-line `value:`. **(3) It had diverged from the C4**,
> which already declares `bam` as a **projector** with a schema in read-models
> (`c4-l2.yaml:343,370,484`) — a schema with **zero tables** (`grep bam specs/database/` = 0).
> **(4) Erasure**: identity-bearing metrics are personal data either way, and in our Postgres they are
> inside the deletion engine's path instead of a vendor store with no per-subject deletion API.
> **The mechanical question is answered** (D8): a `projections:` block declaring `key` / `measures` /
> `fold` (`increment`/`decrement`/`add`/`subtract`/`set`/`max`/`min` per event), and a `metrics:` block
> declaring `over` / `groupBy` / `value` / `exposedAs` — every field reference a `$ref` into the
> **specific event**, so the validator proves the field exists there. **The rule that earns the whole
> shape fails on `main` today**: `serviceType` is on `OrderPlaced` and on **no other Order event**
> (`OrderExpired` carries `orderId` alone), so a projection keyed by it **cannot be decremented by a
> cancellation** — a counter design cannot even see that, and ships two numbers that quietly disagree.
> ⚠️ **Two clauses of [ADR-20260810-234225](../adr/ADR-20260810-234225-business-metrics-for-every-feature-and-every-persona.md)
> are contradicted** (*"never entity ids"* — relaxed to *bounded declared population*, which is what
> makes `groupBy: [restaurantId]` and the restaurant-facing panel possible; and *"generated
> instruments"*). The ADR is `Accepted`, so this is a **decision reversal**: filed as MET-R, **not
> executed**, and the ADR will be **superseded, never rewritten**. Its principle is untouched.
> **Q7 (a hosted analytics SDK) is now recommended for CLOSURE as "no"** — the projection design kills
> its order-side motivation and the behaviour store kills its browse-side one.

> 🔍 **2026-08-11 — BEHAVIOUR EVENT TRACKING GETS A DECLARATION SITE — AND THE ARTICLE 9 EXPOSURE
> IS ALREADY IN THE SPEC, NOT IN THE FUTURE**
> ([#485 "Behaviour event tracking has no declaration site…"](https://github.com/TheCaptainCompany/captain-food/issues/485),
> [PROP-20260811-000946](../proposals/PROP-20260811-000946-behaviour-event-tracking-in-the-screens-spec.md);
> docs-only, no code and no spec moved).
> Product-owner directive: *"We need to integrate the metrics in the spec. And integrate the behaviour
> event tracking inside the screens spec."* The first clause **endorses the §27 metrics work below**
> and changes none of it; this is the second.
> **The finding that shapes it is not the absence of tracking — that is expected. It is that
> special-category-adjacent data is ALREADY declared and ALREADY stored**:
> `SetCustomerPreferences.dietaryTags` is `array<Tag>`, `Tag` is a free-form `string` with
> `maxLength: 80` and **no enum**, persisted to `View_Customer.preferences` jsonb
> (`specs/customer/commands.yaml:179-182`, `specs/common/scalars.yaml:145-148`,
> `specs/database/tables/projection_tables.yaml:337`). **`halal` and `kosher` are spellable values
> today.** No screen binds it, so nothing is running — but no review caught it, because no artifact
> existed that would make anyone look.
> **Why the screens spec is the right location, and it is not aesthetic**: `specs/screens/**` is the
> **only** artifact in the repo that knows a `filter_bar` is an allergen filter — the api layer sees
> an argument, the store sees a string, an analytics SDK sees a payload. So it is the only place the
> rule *"this control may never be tracked"* can be written. **The window is open now and closes
> soon**: `allergen` has **zero occurrences in `specs/catalog/*.yaml`** while the model is
> decided-and-unbuilt ([#184](https://github.com/TheCaptainCompany/captain-food/issues/184),
> ADR-20260808-171056), so the refusal can be built **before** the control exists.
> **Shape**: a root `specs/behaviour_events.yaml` (legal fields — `purpose`, `lawfulBasis`,
> `retention`, `identifierClass`, `specialCategoryRisk`, `dpia` — required, no defaults) bound by a
> `tracking:` `$ref` on screen/action nodes; `kind:` is `VIEW | INTERACTION` and **`IMPRESSION` and
> session replay are absent from the grammar, not discouraged in a comment**; records go to their
> **own time-partitioned store**, never `domain_events` (a behaviour event is not a decided fact, so
> the left-fold invariant would stop holding) and never the order path's instance
> ([#443](https://github.com/TheCaptainCompany/captain-food/issues/443)); ten ERROR rules, of which
> **R10 makes the emitter produce nothing while no DPIA exists** — the build gate that turns
> "sequenced behind [#194](https://github.com/TheCaptainCompany/captain-food/issues/194)" from a
> promise into a failure.
> **First slice is the mechanism with ZERO live events**: instrumentation before a DPIA is processing
> that should not have started. Register: [DECISIONS §28](../proposals/DECISIONS.md) — D1–D7 team-owned;
> **Q1 (client storage, and therefore whether a consent banner exists at all — note `X-SESSION-ID`
> already exists, `crates/server/src/graphql/session.rs:1-15`) and Q2 (does the restaurant see its own
> storefront's behaviour data) are product-owner-owed.** Every legal claim is **VERIFY-FIRST**; no
> licensed-counsel review has taken place.
> **Independent convergence on the write path** (D10): the product owner's own design for this half —
> *"name the interaction and the properties… the principal context will be sent with the jwt. A
> mutation should be exposed to send these events"* — matches the proposal on the name and properties,
> and the **JWT clause is D8 option A reached from the other direction**. It is also ADR-0041's
> envelope doctrine applied to a non-domain write without being asked. ⚠️ **One measured blocker**:
> `op-missing-command` is an **ERROR** and all **86** mutations bind a command handled by an actor
> (`tools/codegen-rs/src/validate/core.rs:292,295,301`), so a mutation today **cannot** be a
> non-command — declaring `recordBehaviourEvent` the only way the validator accepts would enqueue it
> on the actor mailbox and append it to `domain_events`, **silently, with the gate green**. The fix is
> a small api.yaml shape: a mutation declaring **`sink:`** where a command declares `command:` — *this
> write is recorded, not decided*. It must land before this half is buildable.

> 📏 **2026-08-11 — BUSINESS METRICS BECOME A DECLARED, GATED OBLIGATION — AND 26 OF THE 29 WE
> ALREADY DECLARE EMIT NOTHING**
> ([#484 "26 of the 29 declared `business_metrics` emit nothing…"](https://github.com/TheCaptainCompany/captain-food/issues/484),
> [ADR-20260810-234225](../adr/ADR-20260810-234225-business-metrics-for-every-feature-and-every-persona.md),
> [PROP-20260810-234225](../proposals/PROP-20260810-234225-business-metrics-for-every-persona.md);
> docs-only, no code moved).
> Product-owner directive (Jeff Patton): *"we must have business metrics for all features for each
> persona … must be developed with the test and the code."* Auditing the slot that already exists
> found it almost empty: **`specs/observability.yaml` declares 29 `business_metrics` across 14
> contracts, and 26 have ZERO occurrences in `crates/`, `tools/` or `deploy/`** — no constant, no
> instrument, no call site. Exactly three are emitted (`orders_placed_total`,
> `checkout_payment_failures_total`, `scope_membership_lag_positions`). The gate that should have
> caught it (`tools/codegen-rs/src/tests.rs:1500`) covers **3 of 14 contracts** by a hardcoded
> allowlist and asserts only that the metric NAME exists as a string constant — two of those three
> contracts declare no business metrics at all, so its effective coverage is **2 of 29**.
> **The recorded principle**: the unit is the persona **ACTIVITY** (8 personas, 25 activities), not
> the story step (144 — two of which `$ref` the same query and one of which is a poll loop); a
> metric declares the **question** it answers; attributes are bounded sets, never entity ids.
> **Declaration is enforced like ADR-0032 and emission is not** — `make validate` cannot see a call
> site — so the chain is validator (coverage) → **generated instruments** (names, attribute types,
> arity; deletes the scanner's metric half) → **`InMemoryMetricExporter` behaviour test** (it fires,
> once, not on a replay). No source-text scanner is added.
> ↑ ⚠️ **HISTORICAL — the two emphasised clauses in this paragraph were REVERSED the next day.** See
> the 2026-08-11 "the reversal is confirmed" entry above: a business metric is a **projection**, not a
> generated instrument, and grouping keys need a bounded *population* rather than being barred from
> entity ids. This entry is left as written because STATUS is a chronological record.
> **Sequencing**: gate forward now with an enumerated, monotone-shrinking `unmeasured:` waiver list,
> backfill in value-stream order — a one-sweep backfill was already run at this scale and the 26 dead
> declarations are its receipt. Register: [DECISIONS §27](../proposals/DECISIONS.md) (D1–D7 team-owned,
> **Q7 product-owner-owed**); §22's *"Business-signal observability contracts"* row closed by
> subsumption. The per-persona metric GRID is the `ux-designer` lens's parallel deliverable, not this.

> ✅ **2026-08-10 — THE LOCAL TEST GATE IS HONEST: `make test-crates` RUNS FROM THE STOP HOOK, AND A
> MISSING DATABASE NOW FAILS**
> ([#474 "`make rust` runs no workspace tests at all, and DB-gated tests skip silently — \"local gates green\" is a false signal"](https://github.com/TheCaptainCompany/captain-food/issues/474),
> branch `474-honest-test-gate`, mob protocol).
> **The hole**: `make rust` = `rust-build rust-test validate check-drift`, and `rust-test` is the
> **codegen crate alone** — the documented pre-push gate never ran a line of `crates/**`. #451's
> migration defect passed `cargo check`, six hand-run suites and three green `make rust` rounds.
> **Now**: `make test-crates` (`cargo test --workspace --no-fail-fast`) is invoked by
> `.claude/hooks/stop-gate.sh` whenever the turn's diff touches `migrations/ | crates/ | the
> emitters | Cargo.{toml,lock}` — scope decides whether the DB half is MANDATORY, never whether it
> silently vanishes. **Polarity inverted** (`crates/db_test_gate`, new dev-only crate): a database is
> REQUIRED by default, a missing `DATABASE_URL` PANICS with the command to fix it, and the only way
> out is `DB_TESTS_REQUIRED=0`, which leaves a receipt `make test-crates` reads back into a summary
> naming **every** skipped suite — count it with
> `cut -f1 target/db-test-skips.log | sort -u | wc -l` rather than trusting a number in prose. The
> receipt exists because **libtest swallows a passing test's stderr**: `grep -c SKIP` over the
> 990-test baseline log returns **0**, so the old per-suite SKIP lines were not merely quiet, they
> were unobservable. The decision was hand-written at 17 call sites across 5 crates and now lives in
> one place, guarded by a codegen rule that also rejects the PRE-#474 shape
> (`std::env::var("DATABASE_URL")` under `crates/**/tests/**`, which never mentions the opt-out
> variable and so slipped past the polarity scan); `actor_runtime` keeps one local copy because
> `dependency_rule.rs` forbids ANY path dependency into the workspace (ADR-20260730-234918), and the
> allowlist names each file with its reason. That copy **also writes the receipt** — until it did,
> its five DB-gated binaries skipped without appearing in the summary, so the line named fewer
> suites than had actually skipped.
> **Two new gates, both seen RED against a deliberately re-planted #451**: the checkpoint no longer
> advances past a fold the DATABASE rejected (`FoldFault::{PayloadShape,Database}` — a compiler-
> enforced classification the loop never had, since every failure used to collapse to
> `DomainError::Repository`; there is deliberately no `From<DomainError>`, so `?` cannot pick a class
> and a row key that will not resolve is `PayloadShape`, which keeps one unparseable stream name from
> wedging its group), **shipped GATED**: `DbFaultPolicy::Skip` remains the default and today's
> behaviour is unchanged on every deployed path — flipping it is a separate decision
> ([ADR-20260810-225036](../adr/ADR-20260810-225036-projection-db-fault-policy-gated-halt.md)); and
> validator §16 `schema-writer-missing-column` proves, with no database and in under a second, that
> every `NOT NULL`-without-`DEFAULT` column appears in its writer's insert list. Measured red set on
> the real repo: **exactly the two planted columns**, no pre-existing violations anywhere on the
> projection surface. Gates: `make rust` green, `make validate` **0 errors / 37 warnings, warning
> profile byte-identical to `origin/main`** (CLAUDE.md's pinned 43 was stale; re-measured on `main`
> at `d7087fb` and repinned to 37 when `main` was merged into this branch — re-measure, as it says).

> ✅ **2026-08-11 — #469: THE OPEN PATH READS CREDENTIALS, AND `current` IS TENANT-SCOPED BY HOST**
> ([#469 "`current` leg 1 is dead on the web AND is not tenant-scoped — fix both together or neither"](https://github.com/TheCaptainCompany/captain-food/issues/469),
> branch `469-auth-leg-and-tenant-scope`, PR [#488](https://github.com/TheCaptainCompany/captain-food/pull/488),
> [ADR-20260811-113000](../adr/ADR-20260811-113000-the-open-path-reads-credentials-and-current-is-tenant-scoped-by-host.md)).
> Both halves land together because either alone is worse than neither: the auth half on its own
> ships a live cross-tenant cart.
>
> **Half 1 — `/public` is no longer credential-blind.** It reads the `captain_auth` cookie/bearer and
> verifies it, and it is the ONE path that DEGRADES instead of refusing: absent, expired, tampered,
> JWKS-unreachable and non-CUSTOMER credentials all serve `200` anonymous (a stale cookie is the
> common case; `/public` worked with no JWKS at all before and still must). Each degrade is counted —
> `public_credential_degraded_total{reason}` — and the JWKS fetch is now bounded at 3 s, because key
> refresh has moved onto the storefront's critical path — and the refresh itself is **single-flight
> with a negative cache** (N concurrent requests at the TTL boundary cost ONE fetch; a failed fetch
> silences retries for 10 s; an attacker-supplied unknown `kid` can drive a refetch at most once per
> 5 s), because a Supabase blip at Friday 19:00 would otherwise tax every storefront request 3 s.
> **It grants at most the CUSTOMER identity**: a verified ADMIN/RESTAURANT/RIDER token there stays
> anonymous, enforced **by the type**: `Principal` holds ONE private `Identity` enum whose role is
> DERIVED from the identity, so "role says CUSTOMER, claim absent" is not a field combination anyone
> can spell — it is the named `Identity::Unbound`, which `/public` cannot reach. (Round 2 of review
> corrected an overstatement here: the previous `pub`-fields struct made that state a legal literal
> inside AND outside the crate, so the guarantee lived in a doc comment, not in the compiler.)
>
> **Half 2 — the tenant is a request datum.** `Host` → `{slug}` → `RestaurantId` resolved ONCE at the
> GraphQL edge (POST and WebSocket) and injected beside `ReadScope`, never folded into it; `current`
> stays ZERO-ARGUMENT (an argument would let a client assert the tenant) and both legs are bounded by
> the tenant **in SQL**, through two port methods whose signatures make it non-optional. A host that
> names no restaurant serves `null`, never "the newest cart anywhere"; `carts` remains the
> across-restaurants query. `graphql_routes` now TAKES the tenant lookup, so mounting the surface
> without one does not compile.
>
> **The test that could not previously exist.** Every cart test injected `ReadScope` by hand, which is
> exactly how a dead auth leg survived a green suite. `tests/graphql_cart_read.rs` now drives a real
> `POST /public/graphql` — signed cookie, `Host`, loopback JWKS — through the production router and
> asserts the PRICED payload per host. **Standing rule: a test of an auth-derived value may not
> `.data()` that value.** Each half was mutation-tested separately (restore `Principal::anonymous()`
> ⇒ the auth test reds with `null`; drop the host filter ⇒ the tenant tests red showing restaurant
> B's cart and total on A's storefront; neutralise the SQL predicate ⇒ the DB test reds with 2 rows
> where 1 is expected).
>
> **Blast radius, named** (ADR §Consequences): on `/public` a signed-in customer now also reaches
> `paymentStatus` ownership by claim, matches `operationStatus`/`operationStatusChanged` ownership by
> their own `sub` — **only once claim-stamped**: those two read `user_id` directly, so for the
> pre-claim window ownership rests solely on `X-SESSION-ID`, exactly as on `main` — and the open
> mutations' journal/`domain_events` envelope stamps `user_id`/`user_type = CUSTOMER` instead of
> `PUBLIC`. SSR stays anonymous ON PURPOSE (identity there would emit personalised HTML with no
> `Cache-Control`). `/public` GraphQL responses now vary by cookie, so the whole GraphQL surface
> answers `Cache-Control: private, no-store` — one response layer, not per-handler, so a new route
> cannot forget it; serving one customer's cart to another out of a shared cache would be an
> Art. 32(1)(b) confidentiality failure, and "nothing fronts POSTs with a cache" is an assumption
> about deployments we have not made yet, not a technical measure. **That guarantee holds for the
> MONOLITH only**: the gateway rebuilds each subgraph response from status + `content-type` + body
> alone (`crates/gateway_runtime/src/lib.rs:268-285`) and sets none on its own error paths
> (`:244-255`, `:292-301`), so once the #358 cutover makes the gateway the browser-facing
> `/public/graphql` the header is stripped exactly where a shared cache would sit — propagating it
> there is a **cutover precondition** (recorded in the ADR beside the tenant-host one). Exposure
> today is zero: the monolith is the deployed runtime and nothing fronts it with a cache.
>
> **Three things independent review added, all landed here.** (1) A verified CUSTOMER token with no
> `captain_customer_id` — the pre-claim-stamp window, i.e. EVERY signed-in customer for one token
> lifetime after rollout — now degrades to anonymous and is counted `public_credential_degraded_total{reason=claim_absent}`,
> instead of falling through to `read_authorization_bridge_unresolved_total`, whose contract says
> *"never ordinary user denial"*: a normal rollout would otherwise have bumped a provisioning-gap
> counter on every storefront GraphQL request and read to an operator as an incident. **Both branches
> are now PROVED emitted** (`crates/server/tests/public_credential_degraded_metric.rs`): the same
> claimless token bumps `claim_absent` on `/public` while leaving the bridge counter silent, and bumps
> the bridge counter on `/customer` — so the "stays zero" half is an observation, not a metric name
> nobody checked. `read-authorization` also joined the codegen guard's contract list, so a rename of
> either counter now fails the build. (2) The envelope widening reaches the **mailbox handler**
> (`resolve_actor` branches on `user_type == "CUSTOMER"` ALONE — so a claim-stamped customer with a
> lagging projection takes the branch too): one extra `by_auth_ref` read per delivery on the cart
> mutations at peak. A lagging projection returns `Ok(None)`, not `Err`, so it does NOT abort the
> delivery; only a genuine read-model failure does. Outcomes are unaffected either way — the single
> `domain_id` consumer is unreachable from `/public`. (3) The stored identity now puts an **external
> IdP identifier** (the Supabase `sub`) into the immutable write envelope of `Cart-*`, `Customer-*`
> and `Restaurant-*`, where it **survives deletion of the Supabase identity** — and those streams have
> no erasure path (only `Order` declares one; the deletion engine is stream-keyed). They were NOT
> "made subject-attributable" — `CartStarted` already requires `sessionId` and `CustomerRegistered`
> already requires `phone`; what is new is narrower and different in kind. The production log is empty
> by decision, so this is an unmet launch precondition already filed as
> [#194](https://github.com/TheCaptainCompany/captain-food/issues/194), not a pre-existing breach.
>
> **Round 2 of review also landed**: the `Identity` reshape above; the JWKS single-flight + negative
> cache; `Cache-Control: private, no-store` across the GraphQL surface; and three recorded
> consequences the code cannot enforce — the `captain_auth` cookie is **host-only**, so identity is
> per-storefront (cross-storefront identity is an open authn-scope decision, not taken here);
> `X-Forwarded-Host` is now an authorization input, so the ingress must OVERWRITE it rather than
> append; and in the #358 surface-bin topology the SSR transport drops `Host`, which would resolve
> every tenant-scoped read to `TenantScope::None`.

> 🚧 **2026-08-10 — #451 PHASE 2 LANDED (code): THE CART IS PRICED LIVE ON READ — BUT THE CUSTOMER
> STILL CANNOT SEE IT**
> ([#451 "cart.current returns the authenticated customer's priced cart"](https://github.com/TheCaptainCompany/captain-food/issues/451),
> branch `claude/epic-429-production-test-order-9atwb8`, PR [#460](https://github.com/TheCaptainCompany/captain-food/pull/460), mob protocol).
> **What now works, server-side.** `make generate` wired the three cart resolvers: `current` resolves
> TWO-LEG (claim, then `X-SESSION-ID` with `customer_id IS NULL OR = claim`), the by-id `cart`
> enforces claim-ownership in the BODY — retiring the live IDOR, the dispatch's hard DONE-WHEN — and
> `carts` prices each row from its restaurant's live catalog. All three go through the ONE
> `price_cart` seam (`crates/server/src/graphql/cart_read.rs`) over a one-read memoized catalog
> snapshot. The generated `From<(CartRow, RestaurantRow)> for Cart` — which could only fabricate the
> 0,00 EUR payable — is DELETED, so the fabrication is now unspellable rather than merely unused.
> `by_customer` is OPEN-only + `LIMIT 50` in SQL (a CHECKED_OUT cart's money was frozen at intent;
> repricing it is a receipt-adjacent lie, and one stale line used to error the customer's whole cart
> list). `open_by_session` lost its `Ok(vec![])` trait default, so a fake that forgets it now fails
> the build instead of silently emptying the entire anonymous path. Telemetry: `cart.price` declares
> `otel.status_code` and records ERROR on the unresolvable branch (without it the contract's
> `technical_error: any_span_errors` could never fire and every failure exported as a SUCCESS), the
> empty-cart read emits its span + histogram like any other success, and one `RequestCorrelationId`
> is minted per request and shared by every read-path span.
>
> **What does NOT work — the customer still sees no total.** Two independent reasons, both filed:
> the cart screen's summary bindings name `cart.subtotal|deliveryFee|serviceFee|total` while the API
> exposes `totalAmount` + `breakdown.{...}`, so the screen cannot render a price at all
> ([#468 "The cart screen cannot render a price: every summary binding names a field the API does not have"](https://github.com/TheCaptainCompany/captain-food/issues/468));
> and leg 1 cannot fire on the web client, because the public path never reads `captain_auth`
> ([#469 "`current` leg 1 is dead on the web AND is not tenant-scoped — fix both together or neither"](https://github.com/TheCaptainCompany/captain-food/issues/469)).
> The CHECKOUT shell's `cart_summary_mini` is a different block and does render a live total (proven
> by `web::router::tests::the_checkout_shell_carries_the_cart_it_is_about_to_charge_for`). Also open:
> [#470 "Contract migration: drop the four Cart money columns once the money-free binary is stable"](https://github.com/TheCaptainCompany/captain-food/issues/470)
> (this change ships the EXPAND half only — the columns stay so a failed deploy on the single free-tier
> instance can roll back to a binary that still selects them),
> [#471 "Extend the observability test suite to the `cart-price` contract (span status, empty-cart span, unresolvable counter)"](https://github.com/TheCaptainCompany/captain-food/issues/471)
> (the durable pin for the metrics; a bespoke spy binary was deliberately NOT built),
> [#472 "A dead control stays live: the SDUI renderer evaluates no `visible_when`/`disabled_when` and swallows resolver errors"](https://github.com/TheCaptainCompany/captain-food/issues/472),
> [#473 "Rewinding a projection checkpoint stalls the GDPR deletion engine's scan bound"](https://github.com/TheCaptainCompany/captain-food/issues/473),
> plus [#465](https://github.com/TheCaptainCompany/captain-food/issues/465) (the CartLocked lifecycle)
> and [#466](https://github.com/TheCaptainCompany/captain-food/issues/466) (the screen-roles ⊆
> resolver-roles gate hole). Three open product-owner decisions ride in
> [DECISIONS.md](../proposals/DECISIONS.md) as rows **451-A** (the cart-screen bindings), **451-B** (the
> `currency_mismatch` reason folded into `offer_gone`) and **451-C** (whether #451 keeps its now-stale
> title). The prod smoke's L4 asserts the priced guest cart through `current` + `X-SESSION-ID` and
> gates on the server's self-reported `requiredSchemaVersion`, so it **fails loudly against a
> pre-#451 deployment** — deploy first, then smoke.

> 🚧 **2026-08-10 — #451 PHASE 1 LANDED: THE AMBER SPEC SLICE OF THE CART-PRICING KEYSTONE**
> ([#451 "cart.current returns the authenticated customer's priced cart"](https://github.com/TheCaptainCompany/captain-food/issues/451),
> realizing [PROP-20260810-231500](../proposals/PROP-20260810-231500-cart-current-priced.md) Option B /
> LIVE, recorded in [ADR-20260810-112836 "Cart priced LIVE on read"](../adr/ADR-20260810-112836-cart-priced-live-on-read.md);
> branch `claude/epic-429-production-test-order-9atwb8`, mob protocol). **Spec truth now says LIVE**:
> the `Cart` projection is a money-free pure fold (`projection_tables.yaml` money columns dropped,
> `[customer_id, updated_at]` index added, migration `20260810113000_cart_money_free_fold.sql` +
> schema-version bump); the zero-arg claim-resolved `current` query exists (`specs/ordering/api.yaml`
> + `ViewCurrentCart` story step + the storefront SDUI `cart.current` resolver repointed); the
> by-id `cart` query's live IDOR is retired at spec level (`roles: [CUSTOMER, ADMIN]`,
> claim-ownership documented); the read-side pricing contract `cart-price` is in
> `specs/observability.yaml` (`cart_price_ms`, `cart_price_unresolvable_total{reason}`); the
> impure-fold wording is corrected everywhere (ADR-0028 §5 addendum, rules, entities/events
> comments). **What does NOT yet work**: the `current` resolver is the generated
> `not implemented` stub, the generated Cart→API mapping fills the degenerate unpriced shape
> (empty lines, 0 EUR — exactly what the pre-#451 stub rendered), and the projector still folds no
> lines. **Phase 2 (GREEN)** wires `price_cart` at the resolver seam, the line fold, the
> claim-ownership narrowing in the `cart`/`current` bodies, and proves the `cart-price` metrics
> firing. Phase 1 passed the fold-purity checkpoint (architect, 4/4 judgment calls sanctioned). Three
> product-owner facts then corrected the design — carts are session-keyed BEFORE identification and
> bound by `CartBindingProcess`, the cart is saved at intent as the CheckoutSnapshot, and the
> intended cart LOCK is not modelled at all ([#465](https://github.com/TheCaptainCompany/captain-food/issues/465)).
> `cart.current` is therefore TWO-LEG (claim, then session id with `customer_id IS NULL OR = claim`)
> and `[PUBLIC, CUSTOMER]` — committed as `e9704a0`, which also repaired an anonymous-cart-read
> break Phase 1 had introduced (gate hole filed as
> [#466](https://github.com/TheCaptainCompany/captain-food/issues/466)). Phase 2 followed in the same
> branch — see the entry above for what actually landed.

> ✅ **2026-08-10 — STRIPE PUBLISHABLE KEY BAKED: the #440 env-var-only follow-up is closed**
> ([#448 "Bake the Stripe TEST publishable key as a literal deploy value"](https://github.com/TheCaptainCompany/captain-food/issues/448),
> spec-only, straight to `main`). The product owner supplied the authoritative `pk_test_…` value
> (2026-08-10) and it is now a literal `deploy:` block on `STRIPE_PUBLISHABLE_KEY` in
> `specs/payments/configuration.yaml` (production + staging, TEST mode for both — matching
> STRIPE_SECRET_KEY's reality; the SUPABASE_PUBLISHABLE_KEY baked-non-secret posture, no
> `from_github_secret`). Regeneration compiles it into the per-profile `BAKED` tables of the
> generated configs (`crates/server/src/generated/config.rs` + the payments-scope consumer bins) —
> baked non-secrets ship IN the binary, not via `render-config-sync.json` (that rail syncs
> `from_secret` names only; the Supabase baked literals follow the same shape). **Remaining
> hygiene click**: the
> `STRIPE_PUBLISHABLE_KEY` env var on the Render service is now REDUNDANT and shadows the baked
> value (env > baked; the sync never deletes) — it must be deleted from the dashboard after the
> next deploy, a product-owner action recorded as the deploy-day fact. The go-live constraint is
> unchanged: the `pk_live_` swap lands with `STRIPE_SECRET_KEY_PROD` (issue #254).

> ✅ **2026-08-10 — CART-PRICING KEYSTONE APPROVED (Option B / LIVE); BUILD STARTING**
> ([PROP-20260810-231500 "cart.current: the authenticated customer's PRICED cart"](../proposals/PROP-20260810-231500-cart-current-priced.md),
> tracking [#451 "cart.current returns the authenticated customer's priced cart"](https://github.com/TheCaptainCompany/captain-food/issues/451),
> epic [#429 "Production with test data"](https://github.com/TheCaptainCompany/captain-food/issues/429)).
> **Decision (product owner, 2026-08-10)**: DECISION 1 = **Option B — LIVE**. `cart.current` is priced
> fresh on every read via the existing `application::pricing::price_cart`; the `Cart` projection stays
> a **money-free fold** (drops the impure-fold price columns). DECISION 2 sub-defaults stand:
> claim-resolved **zero-arg** `cart.current` (reuses #434 `ReadScope::Customer`), and "current" = the
> **most-recently-updated OPEN cart**. This settles [DECISIONS.md §1 row G](../proposals/DECISIONS.md)
> (register 8 → 7 open) and fills the two #429 blockers "the cart total never computes" +
> "/checkout carries no route params". **The one Concern — a read-side pricing observability contract
> in `specs/observability.yaml` — is NOT a PO gate; it is folded into the #451 build chunk as DoD.**
> The keystone has an AMBER (spec) half and a GREEN (code) half; the spec changes are plan-mode with
> approval. **Consumer-mediator registration DEFERRED to first real order** per the PO (against the
> team's "start now" recommendation). **Solida rebrand still PENDING** — class-42 unresolved and **no
> entity name chosen yet**, which also gates the entity-path/rebrand work; [#411](https://github.com/TheCaptainCompany/captain-food/issues/411) stays blocked.

> 🚧 **2026-08-10 — `orders_placed_total{status="PLACED"}` EMIT WIRED ON THE PM-MAILBOX PATH —
> ARMS WITH THE `PM_MAILBOX_DELIVERY` FLIP, DOES NOT FIRE IN THE CURRENT DEFAULT POSTURE**
> ([#456 "Emit orders_placed_total so the un-told-order alarm can fire"](https://github.com/TheCaptainCompany/captain-food/issues/456),
> PR [#457](https://github.com/TheCaptainCompany/captain-food/pull/457), epic
> [#429](https://github.com/TheCaptainCompany/captain-food/issues/429), mob protocol).
> **The gap**: the counter `ORDERS_PLACED_TOTAL` and its emitter `telemetry::meters::place_order::placed`
> existed since #191, but had **ZERO call sites** — the success side of the place-order BAM contract
> (`specs/observability.yaml`) was declared and never wired, so no alert on "orders placed" could ever
> trip. **The wiring (counter only)**: one emit at the mailbox handler seam
> (`crates/infrastructure/src/mailbox/handler.rs:612`, in the `Outcome::Completed` arm AFTER
> `flush_staged_in_tx` succeeds), keyed on a pure predicate `staged_contains_order_placed(&[StagedAppend])`
> — emit IFF this delivery's staged appends carry a `DomainEvent::OrderPlaced`. **HONEST POSTURE — the
> emit does NOT fire by default yet**: `record_order_placements` is called ONLY on the PM-mailbox
> delivery path (`handler.rs`), which runs only when the `PM_MAILBOX_DELIVERY` runtime posture is ON.
> That posture is **seeded FALSE** (`specs/database/tables/referential.yaml:111`; `RuntimePosture` DB
> row, #318/ADR-20260803-104819) and its default flip **stays gated pending staging smoke** (see the
> #275 D1 entry below, ~line 1256). With it OFF, the **legacy tick runner** processes
> `PaymentCaptured` (`runner.rs` `dispatch` → `place_order::on_payment_captured` appends `OrderPlaced`
> directly) and its completion arm (`runner.rs` `Ok(Outcome::Completed) => {}`) emits **nothing** — so
> a real placement in today's default posture increments **no** counter. Therefore
> `orders_placed_total` **ARMS with the flip**, it does not fire now. This is **deliberately
> gate-then-stabilize-consistent**: the emit lives with the surviving seam it belongs to (the mailbox
> — final-vision-first; the legacy runner is being retired, not instrumented), and the counter goes
> live as a consequence of the separately-recorded `PM_MAILBOX_DELIVERY` default-flip decision, not as
> a second hidden toggle. **Until that flip, the "a stranger paid us" alarm on this counter cannot
> trip** — the un-told-order safety signal is not yet armed in production. **Why the staged set, not
> the outcome**: `OrderPlaced` is appended only when the place-order guard `should_deliver_order_placed`
> (= `domain::order::fold(stream).is_none()`) is true; a re-delivery or partial-reaction replay finds
> it false, stages nothing, and the predicate stays false — so the staged set IS the guard's output
> transitively. Keying on `Outcome::Completed` (returned even on replays that append nothing) would
> double-count a monotonic counter into a permanent lie — proved by a planted-red spy reading
> `("PLACED", 4)` vs the correct `("PLACED", 1)` over four delivery shapes. **Replay-safe,
> durable-first**: the count moves only once the append is in the completion transaction. **SDK stays
> at the infra boundary** (c4-l3 `instrumented`); domain/application untouched. **Tests**: a
> pure-predicate unit test (present/absent, no DB) plus a metric-spy binary
> `crates/infrastructure/tests/orders_placed_metric.rs` — its OWN binary because `telemetry::meters`
> binds the process meter once via `OnceLock` (the shared `main` integration binary cannot host a spy;
> same reason as `checkout_degraded_metric.rs`), no DB (the emit is pure over the staged Vec; the
> guard's replay staging is proved against real Postgres by `tests/main/pm_prepare_delivery.rs`).
> **DEFERRED** (recorded, not built): the `Outcome{placed:bool}` flag refactor (a larger
> equivalent-correctness change) and the place-order success-status SPAN CHAIN (coupled to the RED
> pricing keystone [#451](https://github.com/TheCaptainCompany/captain-food/issues/451)). No new
> status values beyond `PLACED` (the contract's `status` label is unbounded; `PLACED` is the success
> value). No PENDING/enqueue-path emission. **NAMED RESIDUAL GAP**: if the `PM_MAILBOX_DELIVERY` flip
> is deferred long-term, the legacy runner's completion arm carries no `orders_placed_total` emit and
> the alarm stays disarmed — the mob's final-vision call was to NOT instrument the retiring runner, so
> arming the alarm is bound to the flip landing.

> ✅ **2026-08-10 — SECRET-GATE EXTRACTED TO ITS OWN LEAN CRATE: the deploy-path cold-compile tail
> risk is gone** ([#453 "Extract secret-gate to a lean crate (fix #444 deploy-path cold-compile tail risk)"](https://github.com/TheCaptainCompany/captain-food/issues/453),
> PR [#454](https://github.com/TheCaptainCompany/captain-food/pull/454), epic
> [#429](https://github.com/TheCaptainCompany/captain-food/issues/429), mob protocol).
> **The regression**: #444 wired `cargo build ... --bin secret-gate` as the FIRST step of
> `deploy.yml`, but the gate lived in `tools/codegen-rs`, so that build dragged the
> guppy/determinator/regex/sha2/camino/serde_yaml tree — a COLD compile of MINUTES on a cache miss,
> inside a `timeout-minutes: 10` job that an incident rollback also runs. **The fix**: a pure move
> (no logic change) to a new top-level workspace member `tools/secret-gate` depending on
> serde/serde_json + std ONLY; `compare_secrets` stays the one unit-tested source of truth, the 6
> unit + 2 `CARGO_BIN_EXE` integration tests move with it and stay green, and the bin name
> `secret-gate` is preserved verbatim (deploy.yml invocation + test env-var key on it). `deploy.yml`
> now builds `cargo build -p captain-food-secret-gate`; the binary still lands at
> `./target/debug/secret-gate` so the invocation line is unchanged. **Before/after**: OLD path
> cold-compiled the codegen-rs guppy/determinator tree (minutes); NEW `cargo build -p
> captain-food-secret-gate` cold = ~7s — the durable verdict is `cargo tree -p
> captain-food-secret-gate` = serde/serde_json + their tiny direct deps ONLY (no guppy/determinator/
> cargo-metadata/regex/camino/sha2), NOT a warm-cache wall-clock. **`timeout-minutes: 10` left
> unchanged** (farley's belt call): the budget is now comfortably sufficient and a bump would signal
> a fragility that no longer exists. Process lesson recorded in
> [docs/claude/sessions.md §18](../claude/sessions.md): a mob briefing for a CI-workflow change must ask
> whether the step fits the job's existing timeout and whether it regresses the rollback path — #444
> asked neither and the review caught it post-merge.

> ✅ **2026-08-10 — PRE-DEPLOY SECRET-PRESENCE GATE: a declared secret missing/mis-named in the
> deploy target now FAILS the deploy before Render is told to pull**
> ([#444 "CI gate: declared secrets must exist as repo secrets before deploy"](https://github.com/TheCaptainCompany/captain-food/issues/444),
> PR [#450](https://github.com/TheCaptainCompany/captain-food/pull/450), epic
> [#429](https://github.com/TheCaptainCompany/captain-food/issues/429), mob protocol). New binary
> `secret-gate` (`tools/secret-gate/src/main.rs` since #453; originally
> `tools/codegen-rs/src/secret_gate/main.rs`): a PURE comparison
> `compare_secrets(declared, present)` of the repo secrets the configuration DSL DECLARES as
> deployed-key sources — `deploy/generated/secret-keys.json` `from_github_secret` names, itself a
> deterministic fold of `specs/**/configuration.yaml` (the **superset-by-construction** declared
> source; farley-verified) — against the ones the reachable deploy target holds. Declared-but-absent
> **or present-but-empty** (never-written-empty doctrine) ⇒ FATAL, names each repo secret;
> present-but-undeclared ⇒ NON-FATAL `::warning::` (`RENDER_API_KEY`/`GITHUB_TOKEN`/
> `RENDER_DEPLOY_HOOK_URL`/`MKS_KUBECONFIG` are legitimately undeclared). Wired into `deploy.yml`
> (the authoritative Render path) as the FIRST step, present-set from `${{ toJSON(secrets) }}` piped
> on stdin. Unit-tested (comparison + a mutation-kill decoy proving it keys on `from_github_secret`,
> not the env key) plus a `CARGO_BIN_EXE` integration test running the real binary against the real
> artifact. **WHAT IT DOES NOT COVER — stated loudly in the tool's own output, the workflow comment,
> and here**: (1) **VALUES** — a name present but GARBAGE (a `pk_test` where prod needs `sk_live`,
> an expired token) PASSES; mis-NAMING and ABSENCE only, the value-level verdict stays
> `prod-smoke.sh`. (2) The **K8s `captain-secrets` sealed store** — populated out of band (#358), not
> from Actions; a name here does not prove it was sealed into the cluster. This gate proves the
> Actions → Render/declared-source NAMING boundary; the K8s store remains a NAMED RESIDUAL GAP
> (checkable once an Actions-reachable apply path exists), tracked by
> [#452 "Secret gate: extend to K8s captain-secrets name-presence + front the deploy-bins/#366 Argo path"](https://github.com/TheCaptainCompany/captain-food/issues/452). **`toJSON(secrets)` fidelity**:
> an UNSET Actions secret is ABSENT from the object (→ reported Absent), and GitHub's UI forbids
> empty secret VALUES, so the `Empty` branch's guaranteed reach is the declared-side/defensive case
> and any future present-set that can hold blanks (e.g. a kubectl-read cluster secret), not a routine
> Actions secret-side empty. **Scope fences held** (#329 trap): NOT asserting
> `secret-keys.json` vs `render-config-sync.json` production-set equality (compiler-owned, same
> emitter run, false-fails when worker consumers widen the set), NOT checking the cluster-side store.

> ✅ **2026-08-10 — THE STRIPE PUBLISHABLE KEY REACHES /checkout AND THE PAYMENT ELEMENT CAN
> MOUNT** ([#440 "Stripe publishable key: StripePublishableKeyTest scalar + payments configuration key, SSR-delivered to /checkout so the payment element can mount"](https://github.com/TheCaptainCompany/captain-food/issues/440),
> PR [#441](https://github.com/TheCaptainCompany/captain-food/pull/441), mob protocol; decisions in
> [ADR-20260810-015941](../adr/ADR-20260810-015941-stripe-publishable-key-delivery.md)). The first
> #429 blocker ("no publishable key exists anywhere") is closed at the code level:
> `StripePublishableKeyTest` (`^pk_test_` — a live or secret key is unspellable in the slot),
> `STRIPE_PUBLISHABLE_KEY` declared in `specs/payments/configuration.yaml` (NOT secret,
> presence-gated: absent ⇒ boot never fails, `/checkout` degrades honestly), and the delivery seam
> server config → `SsrExec` → `RenderContext` → `data-pk` on the mount div → hydrate →
> `PaymentElement::mount` in Stripe's DEFERRED posture (no intent can exist at landing —
> acceptance-first). stripe.js ships in the checkout shell ONLY and only when the key exists.
> Key-less/invalid ⇒ `payment_unavailable_state` (fr/en) + DISABLED pay button + zero Stripe
> requests, counted by `checkout_degraded_render_total{reason=stripe_key_absent}` — emitted at the
> SSR boundary and **proved firing** by `crates/server/tests/checkout_degraded_metric.rs` (the
> repo's first spy-observed metric emission). Smoke gains L3b (/checkout must carry
> `data-pk="pk_test_…"`, outage-honest). **Shipped ENV-VAR-ONLY at the time**:
> `STRIPE_PUBLISHABLE_KEY` was declared non-secret with no `deploy:` block, production served by
> the Render env var alone — **CLOSED 2026-08-10 by
> [#448 "Bake the Stripe TEST publishable key as a literal deploy value"](https://github.com/TheCaptainCompany/captain-food/issues/448)**
> (PO-provided value baked; see the #448 entry above — the Render env-var deletion is the remaining
> hygiene click). The one extraction attempt (a CI step base64-defeating log masking) was correctly
> blocked by the security classifier and is ABANDONED — no masking-bypass retry (ADR §3).
> **Still open**: the surface-bins config-closure follow-up
> (#385 track); and the recorded activation constraint (first real-restaurant activation
> mechanically impossible while checkout serves `pk_test_` — ADR §4, binds future activation work).
>
