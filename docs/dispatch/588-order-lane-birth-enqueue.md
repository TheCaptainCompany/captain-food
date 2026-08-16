# Dispatch card — [#588 "The normal checkout path never enqueues OrderPlaced onto the Order lane — the acceptance clock cannot start for saga-appended births"](https://github.com/TheCaptainCompany/captain-food/issues/588)

**Read at**: worktree `b7fd7582059b17c54ce5b3306fb3ff8eb01acaf6` (branch `167-acceptance-timeout-auto-cancel`,
with `origin/main` = `2e1024d18cf902cddd91ad74a83e10b7c6fb5a21` merged in). Every `file:line` below is
that snapshot; re-stamp if it moves.
**Artifact class**: dispatch card (ADR-20260816-020752) — first use.

## 1. Chunk

Make the normal checkout birth arrive **through the Order lane**, so `record_inbound_order_placed`
runs on the `Recorded` arm and `apply_schedules_in_tx` can key the acceptance-deadline row —
i.e. flip precondition #1 for `ENFORCE_ACCEPTANCE_TIMEOUT`.

**Not in scope**: flipping `ENFORCE_ACCEPTANCE_TIMEOUT`; #167's timeout semantics (settled, merged);
#590's verdict-blind `AlreadyRecorded` re-application (the recommended design must not depend on it);
the reclamation replacement birth beyond phase 0's liveness finding.

## 2. Paths

| Path | Why |
|---|---|
| `tools/codegen-rs/src/emit/pm_orchestrators.rs` | emits the `deliver:` step at `crates/application/src/generated/process_managers.rs:663-667` — **change the emitter, never the generated file** |
| `crates/application/src/generated/process_managers.rs` | GENERATED output; regenerates |
| `crates/application/src/process_managers/place_order.rs:37,102` | `PlaceOrderAuthorizedHooks` / the hand-written entry the seam lands beside |
| `crates/application/src/staging.rs` | where the staged-enqueue intent belongs (`application` **cannot** reach `actor_client` — `actor_client/Cargo.toml` depends on `application`) |
| `crates/infrastructure/src/mailbox/handler.rs:715-760` | the live PM invocation; owns the delivery `Transaction` and may use the typed door |
| `crates/actor_client/src/door.rs:120` | the only legal enqueue construction site |
| `crates/application/src/process_managers/reclamation.rs:104` | second birth site — phase 0 only |
| `crates/infrastructure/src/process_manager/runner.rs:415,487` | second PM invocation route; **liveness unverified** (no wiring hit in `crates/server/src`, `crates/infrastructure/src/lib.rs`, `crates/*/src/bin`) |
| `specs/ordering/processmanager.yaml:113` · `specs/ordering/actors.yaml` | the `deliver:` step and the Order's declared `receives` |
| `docs/adr/` · `docs/SPEC-LOG.md` · `docs/STATUS.md` | records |

## 3. The ruling needed

Which side of the write does the clock arm on? The merged fence — **schedules apply on the
`Recorded`/`Cancelled` arm only** (young + vernon) — is the discriminator.

**Option A — the `deliver:` step becomes a lane ENQUEUE (RECOMMENDED, final-vision-first).**
The PM stops calling `Repository::save` on the foreign `Order-{id}` stream; it stages an enqueue that
`handler.rs` turns into a typed door insert **inside the same delivery transaction**. The Order lane
worker performs the append → `Recorded` → schedules keyed on the canonical arm.
*Pros*: one aggregate per transaction (vernon); the mailbox is the serialization point for the Order's
own writer; the clock arms on `Recorded`, so **no dependency on #590**; spec-derivable — the routing
predicate is "target actor declares the event in `receives`", already tabulated as
`ACTOR_INBOUND_FACTS` (`tools/codegen-rs/src/emit/actor_clients.rs:596`), so **no DSL keyword change**.
*Cons*: the Order birth becomes asynchronous w.r.t. the checkout commit (tolerable — acceptance-first
PENDING is the recorded model, but confirm no read path assumes the stream exists at mutation return);
changes a money-path saga's commit path → **HOLD: human**; the predicate may catch other `deliver:`
steps (phase 0 must enumerate).

**Option B — enqueue rides alongside the direct append (dual write).**
*Pros*: smallest diff; commit path untouched. *Cons*: **structurally fragile** — the birth is always
already on the stream when the lane runs, so the clock would arm **only** via the verdict-blind
`AlreadyRecorded` arm that #590 flags as "safe today only because the sole route uses `keep`". It makes
a hazard load-bearing on the money path. Also a genuine dual write: an enqueue failure after a
successful append leaves the clock silently unarmed. **Reject.**

**Option C — B, but atomic (append + enqueue in one tx).**
*Pros*: removes B's dual-write hole (the `StagingEventStore` at `handler.rs:723` already flushes into
the delivery tx). *Cons*: still arms the clock on the `AlreadyRecorded` arm, so it inherits B's #590
dependency, and it keeps a PM writing a foreign aggregate's stream. **Reject.**

**Constraints binding all three.** `actor_runtime/src/completion.rs:69` runs `prepare` with **no
transaction open** and re-runs it on redelivery — the enqueue must happen in `handle`, inside the tx,
never in `prepare`. One aggregate per transaction is what A buys and B/C spend.

**Proportionality**: the option space collapses under an already-merged fence, and no screen or event
payload changes → **an ADR, not a proposal**. Name it in #588. If the mob prefers B or C, that reopens
the space and a proposal + #590 as a hard blocker are then required.

## 4. Phases

- **P0 — enumerate, no code.** List every `deliver:` step whose target actor declares that event in
  `receives`; state whether `runner.rs` is live; land the ADR (docs-only, straight to `main`).
  **▶ CHECKPOINT 1 — the mob reads the list and the ADR before any emitter diff.** If P0 finds more
  than the Order/`OrderPlaced` pair, the routing change ships behind a config flag (gate-then-stabilize)
  and the flip is a separate record.
- **P1 — the seam.** Staged-enqueue intent in `application`; `handler.rs` converts it via the typed
  door inside the delivery tx; emitter routes the qualifying `deliver:`. **▶ CHECKPOINT 2 — the mob
  reads the actual diff before the pg suite.**
- **P2 — red-provable tests** (written failing on today's `main` first).
- **P3 — records**: SPEC-LOG line if `specs/**` moves, `STATUS.md`, warning baseline in the same commit.

## 5. Gates and fences

**Red-provable before green** (each must fail at `b7fd758`):
1. a normal checkout writes an `inbound_messages` row for `("Order","OrderPlaced")`;
2. after lane delivery, the acceptance-deadline row is keyed to that birth;
3. the verdict on that delivery is `Recorded`, **not** `AlreadyRecorded`.

**Standing fences this must not break**:
- **No double-append of `OrderPlaced`** — `commands.rs:1102` stays the absorber; never a second
  `Repository::save`.
- **Idempotent birth under at-least-once redelivery** — the message id must be derived
  deterministically from the order id, so redelivery dedups at the door, not at the aggregate.
- **Enqueue inside the tx** — never after commit, never in `prepare` (`completion.rs:69`).
- **GDPR clock** — the retention/expiry schedule is untouched by this change; no new route may key it.
- **No new dependency on #590's verdict-blind arm.**

`make rust` green · `make validate` 0 errors · check-drift clean.

## 6. Lenses

Whole roster invited, cheap excuse allowed — [DECISIONS §44 MOB-COST-1](../proposals/DECISIONS.md) is
**OPEN**, so no coordinator-chosen subset (ADR-20260809-013142 stands).
Named as load-bearing: **vernon** (one aggregate per transaction; PM tells, does not write a foreign
stream; mailbox as serialization point) · **young** (which arm arms the clock; durability of the birth
moves; confirm no payload change, hence no upcasting) · **evans** (`deliver:` keeping its spelling while
changing mechanics is a ubiquitous-language hazard — name it or rename it) · **reviewer** (money path).

**Reversibility class: hard to reverse — gate required.** Reversible in code, not in state: once a
birth has ridden the lane in production the rows exist. Money-path saga commit + mailbox runtime →
**HOLD: human** at ready-for-review, per ADR-20260815-115220 as amended.

## 7. Founder queue

Nothing blocking. Two items to surface only if the mob diverges: (a) choosing B or C makes #590 a hard
blocker and reopens a proposal; (b) if P0 finds the routing predicate catches other `deliver:` steps,
the default flip becomes a separate recorded decision.

## 8. Findings — the mob's verdicts (PR mob evidence)

Ruling: **Option A**, five lenses PASS. One line each; the binding conditions are recorded in full
in [ADR-20260816-040239](../adr/ADR-20260816-040239-deliver-is-a-lane-enqueue-not-a-foreign-stream-append.md).

- **vernon** (aggregate boundaries) — **PASS**: the violation is worse than §1 said — `on_payment_authorized` saves `Order-{id}` AND `Cart-{id}` into ONE delivery transaction with neither write passing the Order's mailbox; being the birth AUTHORITY licenses the DECISION, never the APPEND; staged-intent-converted-in-the-delivery-tx is correct only if it never runs in `prepare` and uses the passed `&mut Transaction`; the door id is `UUIDv5(source:ORDER ID)` and FROZEN, and a duplicate enqueue is a SUCCESS to the PM; **corrects §2** — `ProcessManagerRunner` IS live and `ReclamationResolved` is unfiltered, so `reclamation.rs:104` is a second, unlaned birth site today.
- **young** (write side) — **PASS**: payload/type/stream unchanged, so NOT a migration and no upcaster; the ENVELOPE legitimately changes (lane route stamps `cause_id`/`user_id`/`user_type` from the mailbox row) and is never backfilled; conditional on a fold-set audit he did not perform — delivered in P0 §2, no fold, view or projector reads `user_type`/`cause_id`.
- **farley** (release path) — **PASS, overturning card §4**: the routing predicate goes behind a config flag UNCONDITIONALLY (gate-then-stabilize does not price a money-path commit change by blast-count; `ENFORCE_ACCEPTANCE_TIMEOUT` does not cover it), and `specs/observability.yaml:226` must be amended in the same PR — `event.store.append` stops being a REQUIRED `place-order` span and a birth-lag histogram (enqueue → `Recorded`) is added, or the 800 ms p95 budget goes green for a saga that no longer does the measured thing.
- **evans** (ubiquitous language) — **PASS**: no keyword split and no rename — the DSL, the spec note and the emitter's own comment all already say Tell; the CODE is the falsehood and Option A makes it true; compiler-first condition — a `deliver:` target that does not declare the event in its `receives` becomes a VALIDATOR ERROR, never a new word.
- **beck** (proof) — **PASS, conditional on red-first**: `pm_prepare_delivery.rs::authorized_payment_enqueues_the_order_birth_and_arms_the_clock` entered via `PlaceOrder` → `PaymentIntentCreated` → `PaymentAuthorized`, asserting the birth message and the PM row commit together, `Recorded` on FIRST delivery only, and exactly one deadline row keyed to that birth; plus three mutations in one batch and the DELETION of the `enqueue_birth` hand-INSERT crutch; no sleeps, explicit drains, `DB_TESTS_REQUIRED=1` in the transcript.

### P2/P3 — the realization (appended by the executor)

Landed on `588-order-lane-birth-enqueue`; both red gates GREEN against real Postgres with
`DB_TESTS_REQUIRED=1`. What differs from the plan above, and why:

- **The lane sink rides the `TriggerEnvelope`**, not a new leg parameter (card §2 expected
  `staging.rs`). Same seam, better placement: whether an enqueue can be staged is a property of the
  INVOCATION ROUTE — a mailbox delivery owns a fenced transaction, the polling
  `ProcessManagerRunner` owns none — and `TriggerEnvelope::unlaned` makes that unmissable at the
  four construction sites. No generated signature moved, so `behaviour_tests.rs` is untouched.
- **The typed `ActorDoor` is NOT the construction site** (card §2 named `door.rs:120`). It cannot
  be: the door inserts through `Arc<dyn Mailbox>`, i.e. a pool handle, and constraint (ii) requires
  the PASSED `&mut Transaction`. `flush_lane_enqueues_in_tx` builds the row in `infrastructure`,
  the same shape and the same place `chain_pm_copy_in_tx` already does, reusing the door's public
  FROZEN derivations (`inbound_message_id`, `stable_partition`). The `receives` proof the door does
  at runtime is replaced by the stronger `pm-deliver-lane` validator rule, at build time.
- **Lane width comes from the DECLARED contract, not the seeded registry.** `chain_pm_copy_in_tx`
  reads `count(*) FROM mailbox_partitions` and errors at zero. Copying that made the checkout saga
  FAIL when the Order worker had not seeded yet — a paid order with no birth, the domain lens's
  worst failure mode, and it showed up immediately in the pg run. `ACTOR_MAILBOXES` is the frozen
  routing contract, so the row is always addressed correctly and simply waits for a worker.
- **evans' rule already half-existed.** `pm-deliver`'s inbox check matches a bare NAME against the
  merged `receives` set, so it proves neither "the target has a mailbox" nor "the event is an
  events.yaml fact". `pm-deliver-lane` adds both arms. Mutation-red, spec data only: dropping
  Order's `mailbox:` fires arm 1 alone; re-pointing the step at `Restaurant` fires arm 2 (alongside
  the older name check, with the sharper diagnosis).
- **The `record_order_placements` hole is closed STRUCTURALLY, not by a second call site.** Adding
  the call to the inbound-fact route would have left the same shape that produced the bug — each
  route deciding for itself. The decision now lives inside `flush_staged_in_tx`, the one way a
  staged event reaches `domain_events`: no route can forget, none can double-count (exactly one
  route ever flushes a given birth), and `no_delivery_route_decides_when_to_count_a_placement`
  fails the build if a route starts deciding again.
- **beck's addendum, both items folded in**: the 2 s tolerance is replaced by deterministic `t0`/`t1`
  bracketing around the drain, and "both or neither" is now a same-`xmin` assertion — an assertion
  about the COMMIT, which goes red if the door insert ever leaves the delivery transaction.
- **Flag posture**: `ROUTE_ORDER_BIRTH_THROUGH_LANE`, default OFF. The two #588 gates turn it ON via
  `routed_deps()`; the flag-OFF posture stays proven by the pre-existing, untouched
  `payment_authorized_chains_to_the_pm_lane_and_materializes_the_order`. Two postures, two tests.
