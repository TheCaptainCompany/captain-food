# ADR-20260906-152024 — Two rules and a second container: pre-push checks on confirmation rounds, claim pinning, and concurrent chunks

<!-- Filename: docs/adr/ADR-20260906-152024-two-rules-and-a-second-container-pre-push-checks-on-confirmation-rounds-claim-pinning-and-concurrent-chunks.md -->

## Status

Accepted — a **founder decision**, 2026-09-06, recorded under `/decision` (the founder decided; the team is the
scribe). His words, verbatim, answering the coordinator's three levers in the same `/direct-question` exchange:

> **"Do 2 and 3 as rules, and add a second container"**

where lever 2 was *targeted gates on confirmation rounds*, lever 3 *antecedent pinning*, and the second container
*a parallel lane*. The lenses renamed all three (evans; below) and the founder's meaning is unchanged: the record
uses the repo's words. Reversal check, run on the terms *one session per chunk*, *never weaken a gate*,
*antecedent*, *lane*, *targeted gate*, *second container*, *parallel lane*, *confirmation round*, *pre-push* across
`docs/decisions/`, `docs/proposals/DECISIONS.md`, `docs/adr/` and `docs/claude/`:

- **Nothing is reversed.** CLAUDE.md's *never weaken a gate* binds the BLOCKING set (CI required checks, the
  validator's 0 errors, the warning ratchet); rule 1 removes nothing from it (farley, beck, holub). CLAUDE.md's
  *prefer one session per work chunk* is kept exactly: two containers are two chunks, each in its own session
  (architect, evans).
- **Two records are AMENDED in place**, banners applied in this change: [ADR-20260816-020752](ADR-20260816-020752-the-loops-context-budget-a-dispatch-card-snapshot-semantics-and-phase-commits.md)
  decision 6 (a local pre-flight on a branch is licensed because CI is the verdict) gains the round shape below;
  [ADR-20260817-105845](ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)
  keeps *antecedent* for a derived number's inputs and gains a pointer to *pin* for a prose claim (evans: one
  name, one meaning). The journal line of 2026-09-06 recording holub's WIP-one-lane condition for slices 3b/4 is
  amended by the founder's third decision: the condition was a dependent-queue cap, and a second concurrent chunk
  keeps it iff it touches nothing on the #816 critical path (holub).

## Enforced by

Rule 1: `make gate-round` (to be built — [#923](https://github.com/TheCaptainCompany/captain-food/issues/923)) and gates.md §*Pre-push checks*; rule 2: the
`Pinned by:` card block and Lane D of `.claude/hooks/register-check.sh` (presence + resolution of the pin symbol,
#619's home); rule 3: the claim comment (the lease) and the write-set test on every dispatch card. Until the
executable halves land, the prose rules in [gates.md](../claude/sessions/gates.md) and
[workflow.md](../claude/sessions/workflow.md) bind.

## Decision

1. **Pre-push checks on confirmation rounds** (lever 2; the term is *pre-push check*, never *gate* — a gate is
   executable and blocking, and CI stays the gate). Round 1 of a PR runs the full local set (`make validate`,
   `make rust`, the DB-gated workspace `make test-crates`, `cargo clippy --workspace`, `make check-drift`). A
   confirmation round (2 or 3) runs `make validate` plus the tests and clippy of the **transitive
   reverse-dependency closure** of the crates it touched (`cargo tree --invert`; beck), plus every suite whose
   INPUT is a touched file with no crate edge — `specs/**`, generated artifacts, migrations and manifests all
   feed `tools/codegen-rs` (beck); a diff touching `migrations/**` or `specs/database/**` expands to the full
   DB-gated set (`infrastructure`, `actor_runtime`, `server`, `-j 1`; dba); a diff touching `specs/screens/**`
   runs the whole screen-binding validator, never a single-file check (ux). **The terminating clause**: the
   hand-back's *green* and the coordinator's ready flip mean **CI green on the head**, never a subset-green
   local run (beck, farley, holub). Before the rule is trusted, the CI flake rate (red-then-green on re-run over
   the last 50 runs) is measured and recorded (farley).
2. **Claim pinning** (lever 3; the term is *pin*, the card line is `Pinned by: <test symbol>`; *antecedent*
   stays reserved for a derived number's inputs — evans). A dispatch card may not cite a spec note, a doc
   comment, a proposal sentence or a `gaps:` bullet **about the code** unless a test pins the claim to the code,
   and the card names that test. The pin **executes the code path and asserts the observable** — a lexical check
   that the note contains a word pins the note to itself and is the tautology this rule forbids; the shipped
   `eventstore_version_note_matches_the_writer` is exactly that and is replaced by an append-into-an-empty-stream
   assertion (beck; #921). A pin never seen red against a code mutation is not a pin. Where no test can pin a
   claim, the card states it as `UNVERIFIED input` (holub; ADR-20260817-105845's shape). Named surfaces:
   stream-position semantics, write-side reads of the read side, rebuild/snapshot disposability (young); *one
   aggregate per transaction*, PM state ownership, activation/residency (vernon); index existence and the
   version base as `pg_index`-reading tests (dba); response-only, nullability and role-visibility claims as
   assertions over the emitted SDL (graphql-architect); a screen's `data_requirements` as a test over the emitted
   screen tree, and the renderer-arm coverage test (ux-designer); `latency_budget` values labelled INITIAL are
   citable only as *chosen, unmeasured* (observability-agent); **a legal claim in `specs/**` pins to an
   ENFORCEMENT test — the `rules:` → `tests.yaml` → generated behaviour-test chain — never to prose; the card
   inherits only what the test asserts and states the residue; narrowing a legal claim to fit a test is a
   decision reversal, never execution** (legal-specialist — completeness, never advice or clearance).
3. **A second container = a second concurrent chunk** (the term is *concurrent chunk*: a chunk, in a session, in
   its own worktree, named by its `NN-slug`; never *lane*, which the mailbox and the CI docs-only path already
   own — evans). The relationship between the two containers is **Separate Ways** (evans). The discipline is the
   mailbox discipline (vernon): the **claim is the lease** — one container owns one claimed issue and its branch
   and never commits to the other's; `main` is the shared aggregate, written fast-forward, contended rows
   (STATUS, the journal, DECISIONS.md, the warning baseline) resolved by rebase-and-re-append at the top, never
   a hand-merge of two tops; **independence is a write-set test** — two chunks are independent iff their cards
   DECLARE disjoint files and neither regenerates `specs/generated/**` nor moves the warning baseline (vernon,
   farley, young); the second chunk **yields** to the #816 critical path on merge order (holub). Never in flight
   in both containers at once: a stored event shape or upcaster, a migration or a projector `fold:`, the same
   aggregate's stream (young, dba); an `api.yaml` fragment reaching the same role, or ANY non-additive schema
   change (graphql-architect); a `specs/screens/<audience>.yaml` + translations sidecar pair, `stories.yaml`,
   `translations.yaml` (ux-designer); `specs/database/**` and a projection rebuild recipe (dba). A generated
   file is never merged by hand: rebase, `make generate`, `make rust`, commit (graphql-architect). The one
   **Ask** between containers (everything else is Tell — push to `main`, the other rebases): a shared port
   signature or the fence seam, and any warning-baseline refresh — addressed on the other chunk's claimed
   issue, with a timeout (no reply by chunk end ⇒ the change does not land; vernon). Each container runs its own
   local Postgres — the two-sessions-one-Postgres rule of environment.md does not apply across containers —
   but `tools/db-preflight.sh` checks reachability, never identity: it gains a refusal of any non-loopback host
   (dba). **The weekly loop-budget cap is shared**, not doubled: the cap is the sum of the ACCOUNTS on the ledger
   ([ADR-20260812-142454](ADR-20260812-142454-the-loop-budget-cap-covers-two-claude-accounts.md)); each container
   runs its own timer and commits its own ledger file; the weekly journal states the cross-branch reconciliation
   (observability-agent). The second container's chunks are **user-visible (GREEN) by preference** — the #816
   lane is dark behind a door and a second dark queue doubles unobserved inventory (holub).
4. **The meter** (business-specialist, observability-agent): the per-PR wide row of ADR-20260906-024838 §4 gains
   `lane` (container id), `wall_clock_dispatch_to_merge` (card timestamp → `mergedAt`), `gate_minutes_per_round`
   (CI check-run times, never the loop ledger — a different population), `local_gate_rounds`, and the
   platform-reported session usage beside the ledger's elapsed seconds; the ledger run file gains `pr`, `lane`,
   `role`, `tier`. No standing ratio is ever recorded; every fraction stays re-derivable. Antecedent for any
   throughput claim, marked `UNVERIFIED input`: #918 ≈ 1 h 50, #920 ≈ 2 h 37 over three rounds, one container,
   n = 2 — *"two containers double throughput"* is unpriced until ten merged PRs carry a lane label
   (business-specialist).

## Consequences

- The founder performs nothing here except the disk allowance already recorded; the second container is created
  by the coordinator (a session in the same environment) and starts by itself under CLAUDE.md, on the chunk the
  architect names in the Consulted block: [#914 "#910 follow-ups"](https://github.com/TheCaptainCompany/captain-food/issues/914) items 2–6.
- Cost is a rate change, not a reduction: a second container doubles the spend of an unmeasured unit until the
  meter above holds data (business-specialist).
- Executable halves tracked on [#923 "Make the founder's 6 Sep process decision executable"](https://github.com/TheCaptainCompany/captain-food/issues/923): `make gate-round`; the `Pinned by:` block + Lane D check (#619's
  scope); the ledger and per-PR row fields; `db-preflight` identity; the replacement of the lexical eventstore
  pin. Prose binds until each lands.

## Consulted (ADR-20260812-143619 — one line per lens)

Consulted for the completeness of the record, never to relitigate; **no lens output is legal advice or
clearance**.

- holub — The WIP-one condition was a dependent-queue cap, not a headcount cap: a second lane keeps its meaning iff it touches nothing on the #816 critical path (the door, AsOfCatalog/AsOfPriceAuthority/price_cart/the cart.current read path, the quote-token specs surface, any crate the 3b/4 executor has open) and carries the same one-chunk-at-a-time ceiling; (2) hides no break because CI stays the gate — the one way it could is a ready-flip conditioned on the local subset. Completeness: the second lane's chunks are user-visible (GREEN) — a second dark lane doubles unobserved inventory; a merge-order yield rule (the second lane rebases and yields to #816); flow numbers per lane (WIP 1, age of oldest in-progress, time since last user-visible change); (3)'s no-test-possible arm = UNVERIFIED input.
- beck — Concur with (2) and (3), two corrections: the shipped `eventstore_version_note_matches_the_writer` is itself the tautology (3) forbids (it pins the NOTE to itself — stays green if the writer flips to `+ index`); the pin's assertion must execute the code path and assert the observable (append into an empty stream yields version 1), the NOTE naming the test id; mutation bar: a pin never seen red against a code mutation is not a pin. "Touched crates" = the transitive REVERSE-dependency closure (`cargo tree --invert`) plus suites whose INPUT is a touched file with no crate edge (specs/**, generated, migrations, manifests → tools/codegen-rs). Terminating clause: hand-back/ready-flip on CI full green, never a subset-green local run.
- farley — (2) is a re-sequencing, not a weakening (gates.md §1 + ADR-20260816-020752 decision 6 already license a local pre-flight on a branch because CI is the verdict); conditions: A the ready flip's evidence is CI green; B measure the ci flake rate (red-then-green-on-rerun over the last 50 runs) before adopting. Executable form of (2): a Makefile target `make gate-round` deriving touched crates from `git diff --name-only origin/main...HEAD`, running validate + `cargo test -p <derived>` (DB-gated, -j 1) + clippy -p; gates.md gains one row; the card names only the ROUND. Executable form of (3): an `Antecedents:` block on the card, each entry `— pinned by: <test symbol>` or `UNVERIFIED input`; Lane D can check presence + resolution of the symbol over that block only (residual: not truth). (4): environment.md:312 does NOT apply across containers (per-database DROP SCHEMA; separate clusters); the hazards are the shared repo: claim/branch collisions, check-drift regeneration races on specs/generated/**, concurrent auto-merge supervision, the append-only loop-budget ledger; independence = no shared generated artifact. Bring forward the #410 demo-deploy drill once per lane-week.
- young — (3) is the doctrine that stored-event and fold claims are pinned by a fold that replays, not a note; name explicitly: stream-position semantics (the pin template exists: as_of_catalog_read.rs "the first event on a stream is version 1"); write-side reads of the read side (the carve-out inventory is hand-maintained prose, projection_tables.yaml:358-360/:899 — a card asserting none must mark UNVERIFIED until a gate enumerates them); rebuild/snapshot disposability (no drop-and-replay parity test exists; SNAP-1 open). (4): two lanes never share a stored event shape in flight (events.yaml payload or upcaster = single-lane exclusive), a migration or a projector fold definition, or the same aggregate's stream/lane key; each card DECLARES the streams and event types it touches; declared overlap makes the second chunk non-dispatchable.
- vernon — Two containers are two actors on one repo: partition by lane key (= the claimed issue + its branch; the claim comment is the lease; a lane never commits to another lane's branch); the shared aggregate is main, written fast-forward only, contended rows (STATUS, journal, DECISIONS.md, warning-baseline.json) resolved by rebase-and-re-append at the top, never a hand-merge of two tops; independence is a WRITE-SET test (disjoint files, neither regenerates specs/generated/** nor moves the baseline); the one Ask (else Tell): a shared port signature / the fence seam (envelope.rs LaneSink/Route) and any baseline refresh — addressed, with a timeout (no reply by chunk end ⇒ the change does not land). Antecedents worth pinning: "one aggregate per transaction" is prose only (bam config.rs:363 clause (c)); PM state ownership (a test that the PM step reads no View_*); activation/residency = UNVERIFIED while PMW-2 is open.
- dba — No objection; a second container shares no database PROVIDED the isolation is asserted: tests reset with a database-wide DROP SCHEMA, so separate clusters isolate by construction, but tools/db-preflight.sh checks reachability, never identity — it would print OK for a URL pointing at a neighbour, staging or production; instrument: preflight refuses any non-loopback host and any URL without a per-lane database name. The two lanes must never both: add a migrations/*.sql file (timestamp collision + the embedded-manifest test fails in whichever merges second), change specs/database/**, run a projection rebuild recipe. (2) scope clause: touching migrations/** or specs/database/** expands the local set to the full DB-gated crates (infrastructure, actor_runtime, server), -j 1. DB antecedents to pin as pg_index-reading tests: the version base (first append = 1), index existence on peak read paths (OrderTracking (restaurant_id, status, placed_at)), the duplicate domain_events index (#921 item 15).
- graphql-architect — Nothing blocks; the composed per-role SDL is ONE artifact from every api.yaml fragment: two lanes may never both hold in flight an api.yaml fragment reaching the same role, nor ANY non-additive change (single-lane repo-wide). Drift shape: each PR's codegen gate is green on its own base; the collision appears as a merge conflict on specs/generated/** (or a clean auto-merge silently dropping one lane's fields). Resolution: never hand-resolve a generated file, never --ours/--theirs — rebase, make generate, make rust, commit the regenerated artifacts in one commit. API antecedents to pin: response-only/server-derived → absence from the emitted <Command>Input SDL; nullability → the exact `!` on the emitted field per role; role-visibility → absence in that role's composed schema (pattern at tests.rs:6849).
- observability-agent — The per-PR row is the right shape; add FOUR columns with antecedents: wall_clock_dispatch_to_merge (card timestamp → mergedAt), gate_minutes_per_round (CI check-run startedAt/completedAt — never the loop ledger, a different population), lane (container/run id), local_gate_rounds; per-PR, no weekly mean. Prose antecedents to pin: observability.yaml:527 INITIAL/TUNABLE budgets (citable only as CHOSEN, unmeasured), :251-273 thresholds, the :760 loader hazard (unknown key silently ignored → a closed key inventory in tools/codegen-rs, compiler-first). Two containers SHARE ONE weekly cap (ADR-20260812-142454: the cap is the sum of the ACCOUNTS sharing the ledger; a second container on the same accounts adds capacity to spend, not to have); each lane runs its own timer (loop-budget-timer--<run id>.json) and commits its own ledger file; usage never doubled; two parallel lanes make `check` under-report by construction (unmerged-branch usage is a lower bound) — the weekly journal line states the cross-branch reconciliation.
- ux-designer — Endorse; today's false antecedent was screen-shaped (the priced read on every mini-cart render, refuted by restaurant_frontoffice.yaml's data_requirements). Pin shape for a screen claim = a codegen test over the EMITTED screen tree (exact data_requirements/skipped_reads for the named screen; emitter web.rs flattens both to ResolverKey); `gaps:` bullets are prose and pin nothing — cite the structural absence or mark UNVERIFIED; the renderer-arm coverage test (every emitted ComponentKind has a match arm in crates/web) remains the only pin for "the user actually sees it". Lane exclusivity: the unit is the PAIR specs/screens/<audience>.yaml + its translations sidecar, plus single-writer across lanes for stories.yaml, translations.yaml and warning-baseline.json — a lane claims an AUDIENCE PAIR, never a screen. A confirmation-round local gate runs the whole screen-binding validator, never a single-file check.
- evans — Consent on all three; three naming defects: "antecedent" must NOT widen (ADR-20260817-105845 uses it for an input to a derivation; a test pinning a prose claim is a WITNESS) → the rule is CLAIM PINNING, the artifact is the PIN, the card line `Pinned by: <test>`; "targeted gate" collides (gate = executable and blocking; gates.md:94 already says "the honest local pre-push gate" — sharpen it in the same change) → PRE-PUSH CHECK; never "lane" for (4) (mailbox lane + the CI docs-only lane already own the word) → CONCURRENT CHUNK: a chunk, in a session, in its own worktree, named by its NN-slug — rank-free; the edge between the two containers is Separate Ways, named so it cannot grow into an accidental shared kernel (gates.md:415-431 precedent).
- legal-specialist — No objection (completeness, never advice or clearance): a legal claim in specs pins to an ENFORCEMENT test, never prose — the pin is the rules ↔ tests.yaml ↔ generated behaviour-test chain (ServerPriceAuthority is pinned by three tests, but narrower than its prose: expectedTotal is optional, so the unconditional display sentence is enforced only when a client sends it) → a card cites the TEST and inherits only what the test asserts, stating the residue; an unpinned legal posture assertion is a misrepresentation risk, not a weak antecedent; narrowing a legal claim to fit an existing test is a decision reversal, never execution. (4): sessions carry no customer personal data while a container holds no production credential; the artifact-capacity and admin-gated rules bind each container independently — no lane inherits authority from the other. Executable home: #619 (a rules.yaml id cited on a card must resolve onward to a tests.yaml entry).
- business-specialist — No objection; (2)+(3) attack the round count (where cost sits); a second container is a RATE change, not a cost reduction, and it doubles the spend of an unmeasured unit (§5 cost clause UNVERIFIED). The ledger run file gains `pr`, `lane`, `role`, `tier` and the platform-reported session usage at stop; the per-PR wide row gains lane, elapsed, usage; antecedent for any throughput claim: #918 ≈ 1h50, #920 ≈ 2h37 (three rounds), one lane, n = 2 — "two containers double throughput" is unpriced until ≥ 10 merged PRs carry a lane label.
- architect — No objection; no reversal: CLAUDE.md's one session PER WORK CHUNK caps chunks per session, never sessions per repo — a second container on its OWN chunk satisfies it exactly. The second container's first chunk: #914 items 2–6 (the red-first gate's selftest holes — arms never seen red; item 1 deferred until the NEW:: fraction is metered), method clause docs/BACKLOG.md:47 foundations first / :61 Urgent = tier-1 gate correctness; class reversible gate tooling (reviewer, beck, farley at briefing; not HOLD: human); zero code overlap with 816-s3-fold-priced-read verified; claim protocol: re-read the issue's labels immediately before commenting and abort if status/in-progress is present, the claim names branch 914-red-first-selftest-holes, its OWN session link, the line `Lane: B (parallel container) — not the #816 quote-token lane`, and the expected touched paths; a claim carrying the other container's session link is an invalid claim, not a race; #908 (full-mob class), #919 (admin-gated halves), #921 (same lane by construction) not recommended for it.

