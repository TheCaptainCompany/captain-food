# Status journal — 2026-W36

Journal entries for ISO week 2026-W36, newest first, in the order they were written.

> **2026-09-06 — [#914 "#910 follow-ups (the Red-first gate)"](https://github.com/TheCaptainCompany/captain-food/issues/914) items 2-6 closed, six coverage holes in the Rule 1 selftest filled, draft [PR #924](https://github.com/TheCaptainCompany/captain-food/pull/924), Lane B (session_01H3AFBVzhSiGXJcFuwKjiMQ, `914-red-first-selftest-holes`).** Item 2 (beck): RF4 (`Red-first: none` at 0 hits) could never go red for the reason its own comment claimed -- the entry parse ran only inside `rf_hit_count -gt 0`, so `none` was accepted by the rule never firing, never by being read and found true. Fixed by option A-prime (beck, endorsed by farley): the entry parse now runs whatever the hit count is; `rf_hit_count` decides only two things -- a MISSING section is refused only at >0 hits, and the explicit negative is refused as a false negative only at >0 hits. KNOWN VERDICT CHANGE, stated in the hook's own header comment: a 0-hit card whose prose merely mentions `Red-first:` with no valid entry and no `none` now blocks with `dispatch-redfirst-shape` -- the same verdict the >0-hit path already gave that shape; the remedy is the same explicit negative. New cases RF4b/RF4c/RF4d pin the 0-hit arm (a malformed positive entry, an honest over-declaration founded on a different record, and the verdict change itself -- RF4d added at the checkpoint, beck: "a change never seen red is an unverified claim"). Item 3 (reviewer, beck): RF7 pins the line-past-EOF branch the removed `wc -l` guard held, never before seen red. Item 4 (farley): RF8/RF9 pin the on-disk `<test path>` existence check, `<test path>` derived from the hook's own path rather than a hand literal, RF8 run from a mktemp cwd so only the `$ROOT/$tpath` arm can pass. Item 5 (reviewer, beck): LD3 no longer pins a hard-coded line 17 of the live ADR-20260821-095957 -- it now derives its hit line the same way the corpus test does, reading `REDFIRST_TOKENS` from the hook file itself. Item 6 (farley): the stale `~15s`/`~200ms` CI comments replaced with measured antecedents (gate-scripts job 17-22s, selftest step 4s, five most recent green `ci` runs on main, 2026-09-06, 1s granularity; local 4.6s wall at 9ad0299, Intel Xeon @ 2.10GHz), same fix mirrored in the selftest's own header. Every new case shown red under its named mutant and reverted (RF4/[Nn]one*-deleted, RF7/empty-sed-counts-as-hit, RF8/literal-NEW-only, RF9/existence-check-deleted, LD3/wrong-variable-name, RF4d/re-gated-behind-hit-count -gt-0); two mutants collaterally reded a second case sharing the same failure shape (RF4's mutant also reds RF5; RF7's and RF4d's mutants also red RF4b) -- noted, not treated as defects. Checkpoint (reviewer, beck, farley -- all three had declared a CONCERN at briefing and read the diff): all three CONTINUE, no blocking finding; reviewer and beck independently re-ran every planted mutant and every red reproduced. Whitespace-only re-indent of the block moved out of the guard, verified `git diff -w` zero content change, in its own commit; the KNOWN VERDICT CHANGE paragraph deduped to one declaration (the hook's header) with a pointer replacing the inline copy (reviewer). Round 2 confirmation pass (reviewer): a workflow.md sentence mis-attributed RF4's mutant to the re-gate-behind-hit-count shape -- that mutant reds RF4b/RF4d, never RF4, whose real mutant is deleting the `[Nn]one*` ALLOW arm; corrected, both mutants re-verified. Gates: `bash .claude/hooks/register-check-selftest.sh` exit 0 ("all cases pass"), `make validate` 0 errors, `make rust` green with `every_record_in_the_corpus_is_citable_through_lane_d` and both gate-job pin tests `ok`, no drift; `make test-crates`/`cargo clippy --workspace` not applicable (no `crates/**` touched, no Postgres in this container). PR stays DRAFT (GREEN merge condition; the coordinator flips ready + auto-merge). MERGED as squash d36d411e after the parent container performed the ready flip on Lane B's Ask (a child container cannot: see workflow.md); non-blocking findings in [#926 "#924 follow-ups (the Red-first gate, round 2): the none form is a prefix glob, per-hit is unenforced, the Rust token mirror is unpinned, sharper 0-hit cases, the gate-scripts job growth is unmetered"](https://github.com/TheCaptainCompany/captain-food/issues/926).

> **2026-09-06 — Founder decision (`/decision`, verbatim): *"emit citation graph built, spec and code gen chunk the team can take"* → [ADR-20260906-162418](../adr/ADR-20260906-162418-emit-the-citation-graph-one-generated-artifact-for-the-edges-that-already-exist-as-structure.md).** One generated artifact of DERIVED edges (evans: `cites`, `pins`, `binds`, `amends` only where declared, `refs`; never hand-declared; a `$ref` is checkable, a citation only resolvable), under `docs/generated/` so the docs-only CI lane survives (farley — the one split, against the architect's `specs/generated/`, resolved on measured lane cost); consumers advisory, never in the QMD index, no CI step (the RETRIEVAL-QMD-CI fences); dangling edges a ratchet warning, line edges as anchors (beck); Lane B after #914 (architect: write-set collision on tests.rs and the hook). Tracking issue [#925](https://github.com/TheCaptainCompany/captain-food/issues/925); the card's STOP list carries the two reversals an executor would be tempted into.

> **2026-09-06 — Slice 3a first run STOPPED correctly on the fence (PR [#922](https://github.com/TheCaptainCompany/captain-food/pull/922), head `7e65c149`, deliverable 1 + half of 5 landed, gates green):** the card named `runKind: door` and fenced `mailbox/**` without a carve-out, while the fleet-parity gate makes the standalone `declare_flag` gate-forced — a card defect (the second of this class after #909). Carve-out (5) and a STANDING clause recorded on ADR-20260904-081527 §8 by consent (farley, vernon); gates.md carries the card-writer rule. Continuation dispatched (deliverables 2–6). Ops from the hand-back: `domain_events.version` is `INT4` at the raw-SQL boundary while every Rust coordinate is `i64` — convert at the query, as `fetch_rows` does.

> **2026-09-06 — Founder decision (`/decision`, verbatim): *"Do 2 and 3 as rules, and add a second container"* → [ADR-20260906-152024](../adr/ADR-20260906-152024-two-rules-and-a-second-container-pre-push-checks-on-confirmation-rounds-claim-pinning-and-concurrent-chunks.md), thirteen lenses consulted for completeness, one line each.** The lenses renamed all three (evans): *pre-push checks on confirmation rounds* (CI stays the gate; the ready flip means CI green — beck, farley, holub), *claim pinning* (`Pinned by:` a test that executes the code path; the shipped lexical eventstore pin is itself the tautology the rule forbids — beck; legal claims pin to enforcement tests — legal), and *concurrent chunks* (never *lane*: the claim is the lease, independence is a write-set test, Separate Ways — vernon, young, evans, farley, graphql, ux, dba). Reversal check: nothing reversed; ADR-20260816-020752 decision 6 and ADR-20260817-105845 amended in place; holub's WIP-one-lane condition of this morning is amended by the founder's third decision — a second concurrent chunk keeps it iff it touches nothing on the #816 critical path. The weekly loop-budget cap is SHARED across containers (ADR-20260812-142454), never doubled (observability); a second container is a rate change, not a cost reduction, and the meter starts now (business). Executable halves → [#923](https://github.com/TheCaptainCompany/captain-food/issues/923). The second container is session_01H3AFBVzhSiGXJcFuwKjiMQ (created 15:22Z, same environment, tag `captain-food-lane-b`); its first chunk, named by the architect under the backlog method: [#914](https://github.com/TheCaptainCompany/captain-food/issues/914) items 2–6, branch `914-red-first-selftest-holes`, with its own session link and a `Lane: B` line in the claim.

> **2026-09-06 — Founder decision (`/decision`, verbatim): *"Increase the disk allowance so the build cache stays"*.** An admin-gated environment action, his to perform; recorded in [environment.md §2](../claude/sessions/environment.md) with the cost that earned it (two forced clears of the 22 GB build cache on 2026-09-06, ~20 minutes of rebuild each). The disk rules stand until a session observes the larger allowance. Reversal check: no record contradicted (the register holds no disk decision; environment.md §2 is operational guidance, now annotated). No lens consulted — a small subject with no option space (proportionality).

> **2026-09-06 — Slice 3a of PROP-20260831-134539 briefed (twelve lenses on file: young, vernon, evans, business, dba, holub, observability, ux, graphql, beck, farley, legal; the architect named the chunk; reviewer at the presentation) and dispatched** on branch `816-s3-fold-priced-read` behind a new door `RUN_FOLD_PRICED_CART_READ` bound to a new register row `QUOTE-MINT-PRECONDITIONS`. The consent decisions (ADR in the PR): the mint happens on the ONLY priced read `cart.current`, which feeds exactly two post-decision screens (/cart, /checkout) — ux corrected the premise that the fold would land on every mini-cart render or the ETA surface (the menu screen and the cart FAB do not run the priced read), which is what business, young, vernon, holub and dba had argued against; ONE authority — with the door open the read prices from the fold and carries its coordinate as one value, the projection read is the closed arm, `processmanager.yaml:68` rewritten in the same commit; NO customer-facing coordinate in 3a (evans: the published word is `quote`, opaque, in 3b); the (a)/(b) split — dba recommended a projected-version column with a two-folds-agree gate; young (the rebuild window), farley (no rebuild recipe, no down-migration), vernon and evans rejected or conditioned it — resolved by the recorded rule (the reversible option behind a gate): (b) behind the door, dba's conditions as flip preconditions (#921 item 2 first, `payload_bytes` on the contract row, the buffer-cache instrument); the budget as a design target (business: the as-of leg is a headroom carve inside cart-price 300/600 — ≤ 150 ms p95, ≤ 250 ms p99, all UNVERIFIED input; holub: observed production L is not a blocking precondition while production is suspended); observability rows inside cart-price (fold histogram, stream length, payload bytes, a reads_total{outcome} dead-man), status `technical_error` by citation of ADR-20260810-112836 §6; legal: 3a does not discharge B1 or close #816, CQ-5/CQ-6 to the packet. Holub's WIP condition recorded: 3b and 4 follow in the same lane, nothing else opens between. **Ops**: #921 item 1 landed on main (`4014bff1`) but its red-first commit was pushed alone first — main red for minutes; gates.md now carries the direct-to-main rule; the card wording (branch-shaped) was the defect.

> **2026-09-06 — [#921](https://github.com/TheCaptainCompany/captain-food/issues/921) item 1 closed: the `domain_events.version` spec note now says 1-based, matching the writer.** `specs/database/tables/eventstore.yaml`'s note said "0-based event number within the stream"; the writer (`event_store.rs`: `expected_version + index + 1`) has always started a stream at version 1 (ADR-20260808-171056). Red-first test `eventstore_version_note_matches_the_writer` (`tools/codegen-rs/src/tests.rs`) shown red against the old note, then green against the corrected one; `specs/database.md`'s two matching sentences fixed, PROP-20260831-134539 §1's row updated from "STALE … has not landed" to "corrected". Spec-only, pushed straight to `main`.

> **2026-09-06 — PROP-20260831-134539 slice 2 MERGED (PR [#920](https://github.com/TheCaptainCompany/captain-food/pull/920), squash `af7df751`, three rounds — at the ceiling): the as-of capability is in the tree, DARK.** `AsOfCatalog::from_stream` folds the Catalog stream to a `CatalogVersion` coordinate (the 1-based `domain_events.version` verbatim, truncating on each event's OWN version) with the existing `catalog::apply`; the `AsOfPriceAuthority` port and its Postgres adapter read a bounded range and REFUSE a coordinate the range does not reach (never HEAD-priced); `OfferPrice` carries unit price, option prices and the per-mode tax-rate object and nothing from the availability vocabulary (four `compile_fail` doctests with an identical-minus-last-line twin); `price_cart` byte-identical; no caller, no cache, no DDL. Presentation pass round 1 (head `3e0cd4d7`, thirteen lenses): seven STOP on in-file defects — the port had invented a 0-based coordinate against [ADR-20260808-171056](../adr/ADR-20260808-171056-register-sweep-consent-decisions.md) (young, evans; vernon and beck found the domain truncation a no-op at its only call site), the benchmark read 200 of 2000 rows while production reads ~L (dba, beck, business), and the span held a guard across an await outside the telemetry crate (observability, reviewer). Round 2 (head `5c591bba`, eight lenses) discharged all but one residual: the span closed before the fold while five records claimed end to end (observability, reviewer). Round 3 (head `f85ff76a`, two lenses) closed it and made a refusal distinguishable from success (`otel.status_code`, `business.failure_reason`). **The number**: the V = L arm (L = 2,000, `UNVERIFIED input`; 500 `ProductAdded` + one 500-product `CatalogImported` + stock rows; N = 10; this container; lab-measured, peak-unverified) end-to-end median ~85–90 ms, max of N 88–140 ms across four runs — inside business's ship-dark band (> 50 ms, < 150 ms), and it is ONE leg: slice 3's coordinate mint adds a second read. Business named slice 3's preconditions (a pay-step budget decided BEFORE the re-measurement; the observed production L, which needs the stream-length contract row pulled forward to slice 3; a stated max L and the behaviour above it; the timeout behaviour — never a silent HEAD fallback). Non-blocking → [#921 "Priced quote token slice 2 follow-ups"](https://github.com/TheCaptainCompany/captain-food/issues/921). Legal's four counsel questions CQ-1..CQ-4 landed in the counsel packet as questions. **Consulted** (briefing, twelve claimed — nine on file: dba, business, vernon, ux, young, graphql, beck, holub, legal; presentation: those plus evans, farley, observability, reviewer). **Banked card defects**: the round-1 card said twelve lenses where the answers file carries nine (overcount — evans, farley, observability briefed by omission, a roster-width miss on a money-path class attributed to the CARD); it named a doctest path that cannot exist; the round-1 and round-3 cards wrote `loop-budget.sh` without its path (`.claude/hooks/`), costing an 11-second ledger segment; and the card's "never redirect to an uncapped log" was read as "pipe through tail", which swallowed a DB-gated verdict and cost a re-run — gates.md already says so (§ `make rust` on a committed tree); the card wording now names the scratchpad file. Per-PR row: #920 · lower tier · HOLD: human · 3 rounds (ceiling) · round-1 misses: card-shaped (the 0/1-based antecedent was the stale spec note cited by the PROP itself) and executor-shaped (the truncation shortcut, disclosed inline rather than as a STOP). Coordinator: no merge conflict (main did not move). Next: slice 3 — the coordinate minted from the same range read (the leading option, undecided) — briefed with business's four preconditions as STOP conditions.

> **2026-09-06 — PROP-20260831-134539 slice 2 ROUND 2 (the as-of capability, coordinate fix), draft [PR #920](https://github.com/TheCaptainCompany/captain-food/pull/920) on [#816](https://github.com/TheCaptainCompany/captain-food/issues/816), HOLD: human (money path).** Round 1's thirteen-lens presentation pass STOPPED on the coordinate itself: a bare `i64` port coordinate with a `db_version_ceiling = version + 1` "0-based convention", and `AsOfCatalog::from_stream`'s domain-level truncation filtering by SLICE INDEX rather than by each event's own version — inert at the only production call site (vernon B1/B2, young B1, evans B1). Round 2: `domain::catalog_as_of::CatalogVersion(i64)` is now the ONE spelling of the coordinate everywhere — the 1-based `domain_events.version` verbatim (ADR-20260808-171056), the same number `EventStore::append` returns; `from_stream` takes `&[(CatalogVersion, DomainEvent)]` and folds only events whose OWN version is `<= up_to`, so a `$`-prefixed technical row dropped before that list is built can never shift the coordinate; the adapter drops the `+1` and fails CLOSED — a coordinate beyond head is `Err`, never a silent HEAD price. THE FARLEY GATE (new, DB-gated): append through the real `EventStore::append`, read back through `as_of(the returned version)`, assert the fold never sees a later append. The span (`catalog.as_of.fold`, `crates/telemetry/src/spans.rs`) covers SQL+decode+fold end to end via `.instrument` — true only as of round 3, which moved the constructor to `AsOfPriceAuthority::as_of` (the call site owning the whole body) after this round's `load_range`-only `.instrument` still closed before the fold ran, a residual round 3 corrected — never `span.enter()` across an await; `business.head_version` stays deliberately absent (no second HEAD read performed); no `specs/observability.yaml` contract row this round (deferred to slice 4, recorded deviation). The benchmark gains its missing arm: V=200 alone (round 1) was a mutant detector wearing a cost-number costume — round 2 adds V=L=head on an import-shaped mix (500 distinct `ProductAdded` + one 500-product `CatalogImported` + the remainder cheap `OfferStockUpdated`), asserts on the MEDIAN at magnitude scale (never the max, never p95/p99 at N=10), and prints SQL/decode/fold/end-to-end separately. Measured (lab-measured, peak-unverified, this container): arm (a) V=200 end-to-end median ~9.5-10.1ms; arm (b) V=L=2000 end-to-end median ~85-90ms, max_of_10 up to ~140ms — against business's stop numbers (>150ms escalate, >50ms ship-dark-with-mitigation), squarely in the ship-dark band, never escalate; named mitigation: the capability stays DARK this slice, re-measure against SNAP-1/narrow-fold before any live caller. The Catalog stream prefix's last respelling closed (`domain::catalog::CATALOG_STREAM_PREFIX`, pinned three ways: `catalog_stream_name_has_one_owner`'s two new assertions, `catalog_registry_prefix_matches_the_domain_constant` in `projection/worker.rs`). The compile_fail/passing doctest pair rewritten identical-minus-last-line (four compile_fail arms: `availability`, `stock`, `orderable`, `availability()`), closing the "wrong reason" gap where a rename could pass compile_fail without the twin ever noticing. PROP §11 slice 2 restates round 1 as UNSOUND and round 2 as the fix; slice 3's CATCH gains the customer consequence, the leading decidable minting answer (from the write-side fold's own highest-applied-version, free since round 2's fail-closed check already computes it), and the `processmanager.yaml`/`rules.yaml` repair-list join; §12 bullet 1 rewritten to both arms; glossary drops camelCase `asOf`. Four new counsel questions CQ-1..CQ-4 on the tax-rate leg (mode selection, the null-mode `defaultTaxRate` fallback on another stream, option-level rates, a statutory rate change between coordinate and sale) appended to `docs/legal/BRIEF-20260831-repricing-and-price-quote-counsel-packet.md` §7(e). Every red-first test shown red under its named mutant (two caught EVEN MORE sharply than predicted — the fail-closed check intercepts both the reintroduced-`+1` and the truncated-read mutants before the price/count assertions they were nominally aimed at). Gates: `make validate` 0 errors, `make rust` green, DB-gated `make test-crates` green (Postgres live), `cargo clippy --workspace --all-targets -- -D clippy::disallowed-methods` clean, `make check-drift` clean, `python3 tools/link-check.py` 0 broken. PR stays DRAFT for the coordinator's ready-flip after the team's reviewer pass (HOLD: human).

> **2026-09-06 — PROP-20260831-134539 slice 2 (the as-of capability), draft [PR #920](https://github.com/TheCaptainCompany/captain-food/pull/920) on [#816](https://github.com/TheCaptainCompany/captain-food/issues/816), HOLD: human (money path).** `AsOfCatalog::from_stream` (`crates/domain/src/catalog_as_of.rs`) reuses the existing Catalog `apply`/`fold` and truncates itself at a coordinate, narrowing into `OfferPrice` (unit price, per-option prices separate, the folded `TaxRate` object per ADR-20260818-121500) — no availability/stock/existence vocabulary, pinned by a `compile_fail`/passing doctest pair. `AsOfPriceAuthority` port + `PgAsOfCatalogRepository` adapter (`WHERE version <= $2`, the stream name from a new `catalog::CATEGORY`/`catalog::stream` constructor migrated onto the three existing `Catalog-<id>` sites). DARK: no caller, no cache, no checkout wiring (mob consent) — the one-pricer property is an equivalence test against the HEAD `CatalogSnapshot`/`OfferView` path instead. Every test shown RED under its named mutant before being reverted green. Benchmark (lab-measured, this container, DB-gated): L=2,000 events, V=200, 10 iterations — `total_p50=5.58ms p95=p99=max=6.41ms`, well under `business`'s 50ms/150ms stop numbers. `specs/catalog/rules.yaml` gets no new rule this slice — `make validate` requires a `tests.yaml` case for any declared rule (ADR-0032), so "as-of"/`AsOfCatalog` are recorded in the PROP glossary instead. PROP rewritten: §1 stale rows (the oversell guard now runs at checkout, an as-of fold now exists), §11 slice 2 REALIZED + the slice-3 coordinate question (no projected-version column on `Catalog` yet), §12 rewritten to the measurement plus the durable "Catalog stream history is load-bearing" consequence, §6 D3 gets legal's three tax-rate notes. PR stays DRAFT for the coordinator's ready-flip after the team's reviewer pass (HOLD: human).

> **2026-09-06 — #917 MERGED (PR [#918](https://github.com/TheCaptainCompany/captain-food/pull/918), squash `8a34af1b`, two rounds): `RUN_SIRENE_WORKER` is bound to register row `SIRENE-RESTART` with deploy values reconciled to its prose, the validator refuses a key whose prose says STOPPED without a row (`config-prose-says-stopped-without-row`), and every `RUN_*` bool key declares `runKind: door | worker` ([ADR-20260906-113444](../adr/ADR-20260906-113444-every-run-key-declares-runkind-door-or-worker-and-the-parity-gate-filters-on-it.md)).** Presentation pass on head `fd691a23`: reviewer, farley, beck, evans — all PASS, no blocking finding; the reviewer re-planted four mutants on a sandbox copy. Non-blocking → [#919 "runKind follow-ups"](https://github.com/TheCaptainCompany/captain-food/issues/919): the converse parity test misses a worker `declare_flag`'d at exactly one root; the `door` definiens says "bound to a preconditions record" — evans reads it as the decisionRow proxy re-smuggled as prose, farley asks to ENFORCE door ⇒ row, a small option space for consent when picked up; the "Worker toggles" header is false of its contents; the gate proves the spec, not the running service (Render env beats BAKED, `sirene-sync.yml`'s resume note bypasses the row). The population half of [#908](https://github.com/TheCaptainCompany/captain-food/issues/908) item 3 is discharged; its regex-literal test and the `*_enforcing` gauge parts stay. **Consulted** at the consent: farley (a declared class, required, never inferred or hand-listed), evans (the word is `runKind`, both values already ubiquitous terms, a bare token never a scalar), beck (three red-first shapes, two mutants planted and reverted); the architect was not consulted (the consult surface refused `Red-first: none`, #914 item 10). Per-PR row: #918 · lower tier · GREEN · 2 rounds · round-1 STOP was the executor's own true finding, not a review defect. Coordinator merge of `origin/main` into the branch for the journal insertion-order conflict (both entries kept, newest first). Next: PROP-20260831-134539 slice 2 (the as-of capability) on the briefed card.

> **2026-09-06 — [#917](https://github.com/TheCaptainCompany/captain-food/issues/917) round 2: every `RUN_*` bool key now declares `runKind: door | worker`, and the fleet-parity gate filters on it instead of `decisionRow:`.** Round 1's own STOP was right: binding `decisionRow: SIRENE-RESTART` to `RUN_SIRENE_WORKER` tripped `run_flag_parity` (farley's #639 part C step 6-v fleet-parity gate, [ADR-20260905-223957](../adr/ADR-20260905-223957-the-admin-door-in-two-slices-a-platform-membership-binds-the-admin-seam-then-the-system-host-is-routed.md) §5), whose door population was inferred from `decision_row.is_some()` — a proxy `RUN_SIRENE_WORKER`'s own honest bind falsified, because a resident worker CAN carry a release-gate row (exactly #908 item 3's prediction). **Decision by consent** (TEAM-DECIDES-OPTION-SPACES): a declared, required, closed-set attribute, never inferred or hand-listed — **farley**: fix the population, never weaken the assert, never a hand-kept name list (the gate's own doc comment WAS one, and it had already drifted twice); **evans**: the word is `runKind: door | worker` (46/22 uses already in the tree; "Worker toggles" is the literal section header) — a bare token closed in the loader (ADR-20260811-014129), no `RunKind` scalar; **beck**: three red-first shapes, all confirmed — the grammar landed UNWIRED first against the real, unannotated corpus (`config-run-kind-missing` on all sixteen keys, quoted); a new test deriving "`declare_flag`'d at both roots ⇒ door" from the SAME two composition roots the gate already reads; both card-named mutants (`runKind: door` on `RUN_SIRENE_WORKER` reproducing round 1's exact red; `runKind: worker` on `RUN_RIDER_RESTRICTION_DOOR` while both roots keep its `declare_flag`) planted and confirmed red, then reverted. **D4** landed the grammar + two new rules (`config-run-kind-missing`/`config-run-kind-unknown`, `tools/codegen-rs/src/config.rs`) wired live, corpus red on purpose. **D5** classified all sixteen keys from their OWN `gates:` prose — seven doors, nine workers; `RUN_RIDER_RESTRICTION_SOCKET_CLOSE` is the deliberate non-obvious case (lives under "Worker toggles" but is a door — a per-connection watcher, documented at the key itself); no STOP was needed, every key's prose settled it. **D6** repointed `run_flag_parity` onto `run_kind == Door`, dropped the hand-kept doc-comment name list, added a non-empty assert on the worker population too, and added `every_declare_flagged_key_is_declared_a_door` (the converse read of the same two composition roots) — **the round-1 red goes green with ZERO `crates/**` edits**, confirmed by an empty `git diff --stat crates/` at the D6 commit. `decision-row-open-key-must-be-off` and `config-prose-says-stopped-without-row` are UNCHANGED — `decisionRow:` and `runKind:` are now orthogonal (`RUN_SIRENE_WORKER` carries both). Gates: full suite 452 passed, 0 failed; `make validate` 0 errors (96 warnings, unmoved from `warning-baseline.json`); `make generate` + `git status` clean; `python3 tools/link-check.py` 0 broken; `make check-drift` clean; `make rust` green. PR [#918](https://github.com/TheCaptainCompany/captain-food/pull/918) stays DRAFT.

> **2026-09-06 — #904 MERGED (PR [#915](https://github.com/TheCaptainCompany/captain-food/pull/915), squash `346860a1`, two review rounds): one silent `/auth/refresh` retry before any `unauthenticated:` bounce, and `?next=` return-to-screen — the member door's flip precondition (ADR-20260905-101349 §13) landed on the code side.** A `RefreshingTransport` decorator (one refresh per screen load, only a 401 arms it, a failure remembered for the page), `?next=` composed at the one bounce authority with `router::safe_next` as the allowlist (never a hand list; `/sign-in*` unreachable by construction), the email hop carried client-side in `sessionStorage` consumed once and validated at consumption (never the provider's redirect URL), a mutation 401 refreshed then re-sent under the ORIGINAL messageId (the 401 is an edge refusal before any enqueue; the mailbox's `ON CONFLICT (message_id) DO NOTHING` is the second belt — graphql-architect withdrew its "replay no" on that evidence), the "session ended" copy declared as a `gaps:` line on every sign-in door. Named V0 gaps: the flagship journeys are `route: "/"`, `:param` routes excluded, `/public/graphql` customer expiry is a 200 not a 401, open WebSockets, no WASM telemetry. **Per-PR row**: #915 · tier lower · class GREEN · rounds 2 · round-1 blockers 2 (lens catches on fresh-written tests: a fallback-shaped assertion; a decision placed where no native test could reach it) · card defects 0. **Ops**: the first executor on this card produced nothing in ninety minutes (no claim, no commit, no gate) and was replaced — the replacement pushed per deliverable; the live Red-first gate refused a card whose entries carried double quotes (the extraction stops at a quote, #914 item 9), then accepted the same card without them; a main-side journal line produced an insertion-order conflict the coordinator resolved by keeping both entries. Follow-ups [#916](https://github.com/TheCaptainCompany/captain-food/issues/916). **Architect run report (next chunk)**: named #816's binding of `expectedTotal` — which the register says otherwise: QUOTE-TOKEN (decided 2026-08-31) REPLACED that mechanism with the signed quote token, PROP-20260831-134539 is APPROVED ("build it, slice 1 first") and rejects the binding as its D1 option 5; slice 1 (HEAD orderability at checkout) already landed as [#824](https://github.com/TheCaptainCompany/captain-food/pull/824) — so the next chunk is slice 2 (the as-of capability), briefed to the full mob; the architect's SIRENE gate-hole finding is filed as [#917](https://github.com/TheCaptainCompany/captain-food/issues/917) and dispatched as the GREEN lane meanwhile; step 7 stays AMBER until its register row exists.

> **2026-09-06 — [#904](https://github.com/TheCaptainCompany/captain-food/issues/904) "Web client: one silent `/auth/refresh` retry before any `unauthenticated:` bounce, and `?next=` return-to-screen" landed on [PR #915](https://github.com/TheCaptainCompany/captain-food/pull/915), DRAFT, class GREEN.** Briefed by ux, beck, graphql-architect; realizes the flip precondition [ADR-20260905-101349](../adr/ADR-20260905-101349-step-6-lands-in-four-slices-the-bridge-and-the-grant-first-the-door-second-and-the-accept-is-two-commands-in-two-lanes.md) §13 names for the member door (and item (7) of `ADMIN-DOOR-PRECONDITIONS`): the access cookie is a one-hour TTL, and an operator who 401s at 19:40 and waits on a fifteen-minute email link is the paid-order-nobody-is-told-about failure in an auth costume. Six deliverables, one commit each: **D1** `RefreshingTransport` (`crates/web/src/graphql.rs`) — a `Transport` decorator giving ONE silent 401-refresh-and-reissue per screen load (never on a 403 — a rotated token carries the same role), `Arc<AtomicBool>` rather than the `Rc<Cell<bool>>` a wasm-only decorator would reach for first (`Transport` requires `MaybeSync` == `Sync` off wasm32, so the four native `#[tokio::test]`s can hold it across an `.await`); **D2** `router::safe_next` (the ROUTER is the allowlist, never a hand list) + `bounce::bounce_target` (the ONE `?next=` composition point both the hydrate loop and the mutation dispatcher go through); **D3** the email-hop capture (`next_param.rs`, sessionStorage, never the mailed link — ux Q2: an attacker-influenced path through the provider's own logs) plus the "rider door" same-tab leg (`interact.rs`'s `navigate_home_or_next`, no spec field); **D4** the mutation split resolved — a 401 on a mutation needed NO new code path, since wrapping its transport in `RefreshingTransport` already reissues the SAME document+variables (the messageId is embedded before the first send), so `pending::dispatch_persisted` sees a transparent success and interact.rs's bounce is reached only once the refresh is spent — never a new id, toast or retap; **D5** no conditional SDUI text (spec grammar, STOP) — the "session ended" copy is a `gaps:` line on all three sign-in doors instead (SPEC-LOG sentence, same commit); **D6** this entry, plus a landed sentence on all three PRECONDITIONS rows' `note:` (`MEMBER-SIGN-IN-DOOR`, `RESTAURANT-INVITATION`, `ADMIN-DOOR` item (7)). Eight red-first entries (ADR-20260905-101349:171), each seen RED against the card's own named mutant before green — D1's four in `graphql.rs`, D2's two in `router.rs`/`bounce.rs`, D3's one in `sign_in_return.rs`, D4's one RELOCATED to `pending.rs` (`interact.rs` is `#![cfg(all(target_arch = "wasm32", feature = "hydrate"))]` end to end and untestable by `cargo test -p web`, beck's own Q1 at briefing — the same reasoning that put D1's tests in `graphql.rs` instead of the hydrate loop). A prior executor session was dispatched on this card and produced nothing (no commit/gate/handback, ~90 minutes, stale loop-budget timer closed with its true elapsed time before this run); this run reused the existing branch with a fresh claim. Named gaps (PR body, not this card): `/public/graphql` customer expiry (a 200 anonymous, never a 401); open WebSocket subscriptions; no WASM OTel (`auth_refresh_total{outcome}` measured server-side, a follow-up); hand-written screens' OWN reads (`checkout`/`tracking`/the return landings) stay on the plain `HttpTransport`, only their mutation dispatcher gets the one-shot refresh. Gates: `make validate` 0 errors (96 warnings, baseline unchanged), `cargo test -p web` 210 passed, `make rust`, `cargo clippy --workspace --all-targets -- -D clippy::disallowed-methods`, `make wasm`, `make check-drift` — verdicts in the hand-back. `make test-crates` (DB-gated) NOT required — client-only change.

> **2026-09-06 — #910 MERGED (PR [#913](https://github.com/TheCaptainCompany/captain-food/pull/913), squash `a7505517`, two review rounds): the two failure shapes of the lower-tier decision are now gates.** Lane D of `.claude/hooks/register-check.sh` carries Rule 1 of [ADR-20260906-024838](../adr/ADR-20260906-024838-the-lower-tier-stays-for-every-class-and-the-two-failure-shapes-become-structural.md): a write-capable dispatch whose CITED record names a test, a belt or a mutant is refused unless it carries `Red-first:` entries of the declared shape or the explicit negative — presence, resolution and shape only, never sufficiency (the hook's own header says so); the selftest cases were committed red before the rule; the term *red-first card step* is declared once in `docs/claude/sessions/workflow.md`; the executor's hand-back carries the mandatory `New grammar / invented exemption:` line with rule 2's widened scope, and the reviewer's checklist reads it. **The smoke farley asked for**: the round-2 dispatch of this very PR was the first card through the live rule (one entry), the #904 dispatch the second (eight entries) — both accepted, no refusal misfire on read-only lens consults. **Per-PR row**: #913 · tier lower · class GREEN · rounds 2 · round-1 blockers: 2 (record-level: a stale hand-back and its journal line) · attribution: 1 card defect (the STOP list forbade `tools/codegen-rs/src/tests.rs`, which holds Lane D's corpus-wide driver — the only honest fix for the CI red lived inside the forbidden scope) + record hygiene; 0 executor misses against an ADR line. Follow-ups [#914](https://github.com/TheCaptainCompany/captain-food/issues/914) (word-bounded tokens — 11 of 447 records trip on prose; RF4; an EOF case; the `NEW::` fraction metered before tightening). Next lane: [#904](https://github.com/TheCaptainCompany/captain-food/issues/904) (silent refresh + `?next=`), briefed by ux, beck and graphql-architect and dispatched.

> **2026-09-06 — [#910](https://github.com/TheCaptainCompany/captain-food/issues/910) "Lower tier stays: make the two failure shapes executable" landed on [PR #913](https://github.com/TheCaptainCompany/captain-food/pull/913), DRAFT, class GREEN.** Realizes the executable half of [ADR-20260906-024838](../adr/ADR-20260906-024838-the-lower-tier-stays-for-every-class-and-the-two-failure-shapes-become-structural.md) (option A on `LOWER-TIER-TRIP`, briefed by beck/farley/holub/architect): **Rule 1 — the red-first card step**, Lane D of `.claude/hooks/register-check.sh` — once a dispatch's trail resolves, every RESOLVED cited record (never the live corpus) is grepped for a test/belt/mutant-naming line; a hit obliges a `Red-first:` entry (`<test path>::<name> — <record>:<line> — mutant: <text> — expected red: <text>`, path exists or `NEW`, `<record>:<line>` resolves and carries a token) or the explicit negative `Red-first: none — <record> names no test`, refused if the citation actually has a hit. Committed RED FIRST (D2's RF1/RF2/RF3/RF5 cases, plus the LIVE-corpus LD3 case which needed a real entry once the rule existed — its own citation of ADR-20260821-095957 genuinely names tests), then D1 turned the suite green; both runs quoted on the PR. **Rule 2 — the mandatory hand-back line** `New grammar / invented exemption: <none | …>` added to `executor.md`'s Reporting section (absence fails the hand-back; verbatim scope: spec key/generated-artifact semantics, invented exemption/fence/gate-scope, self-authored `rules:` recipe strings, new SDUI grammar or gap-bound controls, counsel-reviewable copy, unconditional on events/fold:/snapshot keys) plus a `reviewer.md` checklist line to read it; states explicitly it binds the executor's mid-run invention, never the team's DSL rights (ADR-20260810-221840). The term *red-first card step* and its entry shape declared ONCE in `docs/claude/sessions/workflow.md`'s dispatch-card material, cited from the hook and (one-line path citation) from the ADR's rule 1. **Not done here, left open on the ADR's own follow-up line**: the loader key inventory under drift (farley/young) that would make an invented spec key unspellable without a regenerate diff — a separate compiler-first companion. Gates (round 1): `register-check-selftest.sh` all green (RF1-RF6 plus every pre-existing case byte-identical), `make validate` 0 errors, `make generate` clean, `python3 tools/link-check.py` 0 broken, `make check-drift` clean. New grammar / invented exemption (round 1): none. **Round 2 (record-level, no code):** once D1 landed, CI reded on the REAL corpus — 295 of 447 real records name a test, so `tools/codegen-rs/src/tests.rs::every_record_in_the_corpus_is_citable_through_lane_d`'s bare-citation payload no longer satisfied Rule 1 — fixed in [1bc2bbd5](https://github.com/TheCaptainCompany/captain-food/commit/1bc2bbd5d5559d5deee7c52d0d793258d5d115bb) by giving the test a real per-record `Red-first:` entry (a new `canonical_resolved_path` helper mirroring the shell's own glob-priority order for `BRIEF-*` ids that name more than one file) and by simplifying `register-check.sh`'s own `<record>:<line>` check — the `wc -l`-based EOF range guard (which undercounts a file whose last line lacks a trailing newline) dropped in favour of `sed -n Np | grep`; touching `tools/codegen-rs/src/tests.rs` sat outside the round's own STOP list and is banked as a **CARD DEFECT**, not an invented exemption, because the corpus test is Lane D's own driver and the only honest fix lived there. Gates re-run at true head `72e80b86` (code identical to `1bc2bbd5`): `register-check-selftest.sh` self-verification OK / all cases pass, `make validate` 0 errors (only pre-existing warnings), the corpus test itself green (`tests::record_resolution::every_record_in_the_corpus_is_citable_through_lane_d ... ok`, full suite 439 passed 0 failed), `python3 tools/link-check.py` 0 broken (9383 links across 467 files), `make check-drift` clean (full regenerate, no diff).

> **2026-09-06 — #639 part C step 6-iii MERGED (PR [#909](https://github.com/TheCaptainCompany/captain-food/pull/909), squash `7fc8e60a`) in TWO review rounds: System host routing behind auth and the ADMIN sign-in door — step 6 is complete, every door dark behind its gate.** Decision record first ([ADR-20260906-023825](../adr/ADR-20260906-023825-the-admin-sign-in-door-is-the-member-doors-shape-identify-only-against-the-platform-grant.md), eight lenses at the briefing, all carrying option A): the admin signs in by the member door's shape — a PUBLIC `requestAdminSignInLink`/`confirmAdminSignIn` pair on its OWN width-5 actor `AdminSignIn` (an anonymous caller can never head-of-line the grant lane), identify-only against the `PlatformMember` bridge (the bridge consulted zero times on the request leg; a stranger's confirmation panel is byte-identical), a fourth hardcoded stamper `stamp_admin_put_body()` = `{role: ADMIN}`, the seam re-deriving the grant on every request (a still-valid ADMIN cookie whose grant is withdrawn is refused on the very next request — pinned), typed `AdminAccessNotGranted` (never "linked": the grant pre-exists, the door identifies), behind `RUN_ADMIN_SIGN_IN_DOOR` (default false, `ADMIN-DOOR-PRECONDITIONS`). `system.captain.food` serves the SDUI shell; every System screen `requires_auth` + `unauthenticated: → /sign-in`; the anonymous browser gets a 302 and never the board; a bound-but-not-ADMIN principal lands on `no_access` — no nav, no counts, no exit control, the refusal server-answered off the re-derived principal, never the client claim. A new `admin-sign-in-door` observability contract (request leg carries no result; `admin_sign_in_door_enforcing` at both roots; the fleet-parity test from 6-v caught the new key). A real DB-gated walk: bootstrap → request → confirm → `POST /auth/session` → `/admin/graphql` admitted. Item (6) of ADMIN-DOOR-PRECONDITIONS landed; RIDER-RESTRICTION-PRECONDITIONS (1) is discharged on the code side (an admin CAN reach `/system/riders` once the flip preconditions — DNS, the two secrets, the bootstrap run — are met; the row stays open on those).
> **Two rounds.** The executor stopped honestly on the card's "any `deploy/generated` diff → STOP": the diff was the ordinary consequence of the decided design (the new actor's bin manifest + one `/public/graphql` ingress line, the same emitter rule as `restos.`/`riders.`); farley adjudicated ACCEPT and two **card defects** are banked — the lens claim "no manifest change" was wrong in a bounded way, and the STOP was written against that claim instead of the emitter rule. Presentation pass (nine lenses): beck, graphql, vernon, farley PASS; ux (a "Connexion" label on a button that only closes a sheet), legal (the typed email's second store unnamed on the flip row), observability + reviewer (`admin_sign_in_link_requested_total` declared with no emitter; the shared email wall counting admin sends in the member contract's counters), evans (the success token still "linked") and reviewer (the warning-baseline line missing from the PR body) STOP on small items. Round 2 landed all of them: two translation keys, one row line, the emitter plus a closed `SignInDoor` enum selecting counters per door (compiler-first, never a string), "granted" at five sites, the PR body; confirmation PASS. Follow-ups: [#912](https://github.com/TheCaptainCompany/captain-food/issues/912) (incl. farley's: a docs-only push to `main` triggers NO CI run — this PR's first `gate-scripts` red was main's two guessed ADR filenames from the records commit, fixed within the hour, §19i) and [#911](https://github.com/TheCaptainCompany/captain-food/issues/911) (revocation of platform access once a second admin exists).
> **Per-PR row (ADR-20260906-024838 §4)**: #909 · tier lower · class HOLD: human · rounds 2 · round-1 blockers: 6, attribution 2 card defects (the STOP shape), 4 lens catches on shapes the executor wrote fresh (copy, a counter, a vocabulary token, a row line), 0 executor misses against an explicit ADR line. **Ops**: a container restart killed the round-2 executor mid-run; its finished work survived because every item was its own commit — the successor pushed the inherited commits and finished. The coordinator then misread a slow executor as dead (an empty transcript file is NOT evidence of death; `ps` and `git log` are) and dispatched a duplicate, stopped before it wrote anything: check for a live process AND the task's own notification before re-dispatching. The `cargo test --workspace` cross-binary shared-Postgres schema-reset race (environment.md's "two sessions" entry) reproduces within ONE session once `crates/server/tests/*.rs` has enough binaries: mass `relation "X" does not exist` across binaries = re-run `make test-crates` with `-j 1`, not a regression.

> **2026-09-06 — Founder status answers on the seven external items: "Not yet — keep carrying it" on all seven** (DNS SPF/DKIM/DMARC; `EMAIL_QUOTA_KEY_HMAC_SECRET`; `PLATFORM_BOOTSTRAP_ADMIN_SUBJECT`; controller entity + Art. 30 register; labour posture; Supabase DPA + region; support@ inbox owner). A status confirmation, not a decision: no ADR, no row changes status, no backlog move; each of the three rows (`ADMIN-DOOR-PRECONDITIONS`, `MEMBER-SIGN-IN-DOOR-PRECONDITIONS`, `RESTAURANT-INVITATION-PRECONDITIONS`) gains the dated sentence, its release consequence and a re-ask trigger (production un-suspends or a Tours restaurateur is scheduled), so the next session does not re-ask. Relayed to the whole roster before the record (ADR-20260812-143619); consulted, one line each: **holub**: deferral fine; doors-dark work is inventory — name a Tours-facing slice that needs none of items 1–3 (the public try-before-committing demo needs no SPF, no HMAC secret, no bootstrap admin), and a date; **farley (release path)**: "not yet" is provable, not silent — staging refuses to boot (exit 78, `config.rs:543`) and zero admins means the sign-in happy path is unsmokeable; rows carry the dated confirmation plus its release consequence; bring the pain forward by booting staging with the secret absent and asserting exit 78; **business**: concern, non-blocking — carrying items 1–3 is free until the first invitation batch; from then it is top-of-funnel loss (invitations in spam, a burned first contact) with no observability contract to measure it; **ux**: nothing in my lens today; refusal copy promises no clock ("write to us", no SLA), so no string changes while doors stay dark; carry the rider-restricted contest route as the one refusal needing an inbox owner before it goes live; **dba**: nothing blocking; the seven are identity/legal, not storage. Restore drills on a near-empty domain_events prove the path, not the timing — record restore duration and log size per drill once the doors open; **architect**: record shape is `note:` + journal, no ADR, no STATUS; add a re-ask trigger ("re-ask only when production un-suspends or a Tours restaurateur is scheduled"); no backlog bucket or row-order change follows from a "not yet"; the doors' code-side preconditions (#903, #904, invitation send leg, notice surfaces) need none of the seven and stay the GREEN work; **graphql-architect**: carrying is schema-neutral; no PUBLIC/lower-role bootstrap field may be added to work around the dark admin door (a permanent authz hole for a temporary block), and quota-derived fields stay nullable; **legal-specialist (never clearance)**: "not yet" carries no calendar clock (no real personal data yet, ADR-20260813-004634 §2); items 4/6/7 are flip-preconditions that fire instantly at the first real sign-in, item 5 accrues backwards from the first day worked (URSSAF/requalification exposure, grade (b)); nothing blocked by carrying; **vernon**: nothing in my lens — no aggregate boundary or process-manager state changes; carrying alters no transaction boundary; **beck**: no test assumes a provisioned secret (DB suites gated, email quota falls back to the DEV_ONLY key); flagged two unset-refusal gates never proven red — `bootstrap_platform_admin.rs:99` and the production stop at `generated/config.rs:543` — one `Config::resolve` test per key is the cheapest evidence; **evans**: no language or context-map objection; one free-now rename — `PLATFORM_BOOTSTRAP_ADMIN_SUBJECT` should carry the kernel's word AuthSubject (`specs/common/scalars.yaml:564`) before the secret is ever set ("subject" already means data subject on the erasure surface); **young**: nothing blocking — deferring items 1–7 appends no events; item 3 (the bootstrap admin subject) IS the stream identity at `bootstrap_platform_admin.rs:57` and becomes irreversible at first run, not before (a later change opens a second stream and orphans the first); **observability-agent**: no blocker; the four door gauges (`member_sign_in_door_enforcing`, `admin_sign_in_door_enforcing`, `platform_access_grant_enforcing`, `restaurant_invitation_door_enforcing`) read 0 by design while the rows stay open, and 0 is also what a never-registered gauge reads (#895) — nothing alerts on them while dark; the flip-time 0→1 is the acceptance evidence, pre-authored now; Follow-ups filed on #908: the free-now rename of `PLATFORM_BOOTSTRAP_ADMIN_SUBJECT` to carry `AuthSubject` (evans), the two unset-refusal gates never proven red (beck), the staging boot-with-secret-absent drill asserting exit 78 (farley). Holub's question stands on the record: what does a Tours human see running, and on what date — the public try-before-committing demo needs none of items 1–3.

> **2026-09-06 — Founder decision recorded (`/decision`): the lower executor tier stays for every class, and the two failure shapes become structural** ([ADR-20260906-024838](../adr/ADR-20260906-024838-the-lower-tier-stays-for-every-class-and-the-two-failure-shapes-become-structural.md), thirteen lenses consulted for completeness). LOWER-TIER-TRIP closes as decided after five tier-exit trips (#875, #885, #899, #901, #907); ADR-20260904-013450 §3/§5 amended in place; the trip is retired as a decision-queue trigger and the measure becomes a wide per-PR row (id, tier, class, rounds, attribution), never a standing ratio; rule 1 (red-first card step, Lane D hook) and rule 2 (mandatory `New grammar / invented exemption:` hand-back line, widened to invented exemptions, recipe strings, SDUI grammar and counsel-reviewable copy, unconditional on stored shapes) go executable in [#910](https://github.com/TheCaptainCompany/captain-food/issues/910). **Step order reaffirmed by the founder**: *"keep 3, 4, 5, 6, 7 as answered; step 6 follows step 5"* — a journal line, no row. Reversal check: nothing reversed; the lifted specs freeze is not narrowed.

> **2026-09-06 — #639 part C step 6-v MERGED (PR [#907](https://github.com/TheCaptainCompany/captain-food/pull/907), squash `2c3bf3f1`) at the THREE-round ceiling: the platform grant and the ADMIN seam binding.** [ADR-20260905-223957](../adr/ADR-20260905-223957-the-admin-door-in-two-slices-a-platform-membership-binds-the-admin-seam-then-the-system-host-is-routed.md) §1–§3, §5, §7: the `PlatformMembership` aggregate (kernel-homed in `specs/common/` — the `platform` context's other aggregate already lives there; a `specs/platform/` home is an architect row in [#908](https://github.com/TheCaptainCompany/captain-food/issues/908)), `GrantPlatformAccess` (`roles: [ADMIN]`, basis `CAPTAIN_ONBOARDING` only — an act, never a status), `PlatformAccessGranted`, the `PlatformMember` bridge (own group from 0, reset-never-TRUNCATE proven by both an in-place and a TRUNCATE-and-replay test), the seam: `RequestRole::Admin` yields `Identity::Unbound` unless a live grant resolves — `Identity::Admin` is unspellable without a row, `ReadScope::Admin` kept with only its producer changed, an unbound ADMIN acts PUBLIC and cannot even introspect the grant field; the first admin is a recorded ACT — `bootstrap-platform-admin` dispatches the ordinary command through the mailbox, the subject read from the secret `PLATFORM_BOOTSTRAP_ADMIN_SUBJECT`, idempotent (UUIDv5 of the subject → the same lane and stream; running twice appends one fact), inside the OTLP-wired process; the handler refuses any `platformMembershipId` that is not that derivation, so one subject can never yield two memberships; `RUN_PLATFORM_ACCESS_GRANT` (default off) at both roots with farley's codegen parity test for every `RUN_*` key; a new `admin-sign-in` contract with no email/subject label. `ScopeType` and `PrincipalKind` untouched (PRINCIPALS-MEMBER stands).
> **Three rounds (the ceiling).** Round 1 (thirteen lenses): ten PASS; reviewer, farley, dba STOP on small items — a sentence claiming no legacy ADMIN behaviour existed to gate (false: the seam ships UNGATED and fail-closed, recorded now with the flip ordering on the row), SPEC-LOG's folder, the missing reset-only rebuild test, and farley's release-path find: a never-required secret armed the pre-deploy gate and 66 non-optional `secretKeyRef`s, blocking every deploy incl. rollback until provisioned — fixed at the root, spec-derived: `secret: true` + `required: []` keys emit `optional: true` and the gate reports them non-fatally (closing the #899-class item in #900). Round 2: all nine items landed; six lenses PASS, reviewer STOP on two cheap items — the optional-secret derivation read `required.is_empty()` and the loader folds an ABSENT `required:` into `[]`, so fourteen pre-existing secrets (Supabase, HubRise webhook, Honeycomb, OVH, Uber Direct… — the count from the generated `secret-keys.json` diff; the reviewer had said twelve) flipped fatal→non-fatal in the generated manifests undisclosed (a blocking gate widened by omission — ADR-20260815-015422's class), and round 2's spec additions had no SPEC-LOG sentence. Round 3 narrowed optionality to an EXPLICITLY declared `required: []` (runtime typing unchanged, a codegen test pins exactly one optional secret) and amended the row. Every briefing CATCH held; the widening was caught by the reviewer's read of the ARTIFACT diff, which the round-2 card had not asked anyone to diff — card defect, banked. Follow-ups: [#908](https://github.com/TheCaptainCompany/captain-food/issues/908) (the kernel projector group has no emitted bin after the #358 cutover — real for the cutover; `role_binding` still spelling `Identity::Admin`; `auth_subject` in OTLP spans as a second store; DDL name parity).
> **Lower-tier tally: first-round PASS 2 of 12** (#864, #892 PASS; the other ten fixed then passed) — the FIFTH `HOLD: human` lower-tier PR to hit the ceiling (#875, #885, #899, #901, #907), recorded on `LOWER-TIER-TRIP`; attribution of the round-3 blockers: one card defect (the invariant "only the new key changes in the artifact" was never stated as a test), one executor record miss (the SPEC-LOG sentence). Holub's question for the founder recorded on the PR: four of the five open ADMIN-DOOR preconditions are his. Ops: the container's ~38 GB ceiling cannot hold `target/debug/deps` (>20 GB) plus a full workspace test build — clear `deps` + `incremental` + `build` before every heavy gate in this class; `make test-crates` IS `cargo test --workspace --no-fail-fast` + the DB preflight (a card asking for both runs asks for the same thing twice).

> **2026-09-05 — #639 part C step 6-v (the platform grant and the ADMIN seam binding) landed on [PR #907](https://github.com/TheCaptainCompany/captain-food/pull/907) (issue [#905](https://github.com/TheCaptainCompany/captain-food/issues/905)), DRAFT, `HOLD: human`.** [ADR-20260905-223957](../adr/ADR-20260905-223957-the-admin-door-in-two-slices-a-platform-membership-binds-the-admin-seam-then-the-system-host-is-routed.md) §1-§5 executed: a new `PlatformMembership` aggregate (own stream/lane, `grantPlatformAccess` ADMIN-only, `basis: CAPTAIN_ONBOARDING` the only value `PlatformAccessBasis` declares, no revoke command) — platform standing is its OWN relationship, never a `ScopeType` widening (PRINCIPALS-MEMBER/RLS-SEQ untouched, both re-cited not re-asked) and never `RestaurantAccessGranted` reused. **The two-arbiters question (§1)**: ADMIN is not a `PrincipalKind`, so the grant reuses no reservation table — the handler checks the NEW `PlatformMember` bridge (`auth_subject UNIQUE`) before appending, a projection READ rather than the reservation's synchronous write-then-check; the honest limit is recorded in the handler's own doc rather than hidden (a genuinely concurrent double-submission for one subject is not a shape this population — Captain's own admins, 1-3, hand-provisioned — produces, and the bridge's own UNIQUE constraint is the final arbiter at projection time). **The seam (§2)**: `crates/server/src/auth.rs`'s `role_path` now yields `Identity::Unbound` for every ADMIN token (mirroring RIDER/MEMBER exactly); the ONLY producer of `Identity::Admin` is the new `resolve_platform_scope`/`PgPlatformIdentity`/`PlatformIdentitySource` seam, consulting the bridge; `ReadScope::Admin` KEPT, only its producer changed; `bridge_resolved` moved ADMIN into the "was a binding presented" bucket. **The bootstrap (§3)**: `crates/server/src/bootstrap_platform_admin.rs` (new) — a `server bootstrap-platform-admin` subcommand (the smallest honest home: per-actor/worker bins are GENERATED skeletons with no room for a hand-written one-shot script) reading the declared secret `PLATFORM_BOOTSTRAP_ADMIN_SUBJECT` (`required: []`, never blocks ordinary boot), minting a deterministic `platformMembershipId` (UUIDv5 over the subject) and dispatching `GrantPlatformAccess` through the ordinary `PlatformMembershipClient`/mailbox door — never a row, SQL or migration; a fixed deterministic bootstrap system principal attributes the act, never the granted subject itself. **Gate + observability (§5)**: `RUN_PLATFORM_ACCESS_GRANT` (default false everywhere, `decisionRow: ADMIN-DOOR-PRECONDITIONS`); the new `admin-sign-in` OTLP contract (`admin.identity.resolve` span, `admin_identity_resolve_total{result}`, `platform_access_granted_total{basis}`, `platform_access_grant_enforcing` dead-man gauge declared unconditionally at BOTH composition roots); a new codegen test (`tools/codegen-rs/src/tests.rs::run_flag_parity`) asserting every `decisionRow`-carrying `RUN_*` key is `declare_flag`'d at both roots with the same default — RED on first run against a genuine PRE-EXISTING gap (`RUN_RIDER_RESTRICTION_SOCKET_CLOSE` missing from `standalone.rs`), fixed in the same change per the test's own "fix the root" instruction. **Tests**: five DB-gated integration tests (idempotent replay from `domain_events` alone after a TRUNCATE+checkpoint-reset, running the bootstrap twice appends one fact, the seam resolves only after the grant lands and stays `NoMapping` for a stranger, an ADMIN token with no grant is `Identity::Unbound`/FORBIDDEN through the real router, the bridge is never consulted on a `/public` request) plus the role-only-mint mutant (seen red only when BOTH `role_path` AND the seam-interception arm are reverted together — the two are independent, redundant defenses, a stronger property than the single-point mutant the card named, recorded as a finding not a gap). **Fence self-check**: `specs/common/scalars.yaml` `ScopeType`/`PrincipalKind` untouched; `crates/infrastructure/src/mailbox/**` touched ONLY for the mechanical `CommandDeps`-field ripple the inbox.rs carve-out forces (standalone.rs's `CommandDeps` struct literal, gate resolution, `declare_flag`/gauge calls — zero change to lease/fencing/delivery logic), the SAME ripple 6-i's `run_member_access_grant`/`members` fields already required and landed; `crates/infrastructure/src/inbox.rs` gained TWO new `CommandDeps` fields (the gate boolean AND the `platform_members` repository) rather than the card's literally-stated "ONE" — the `members: Arc<dyn MemberIdentityRepository>` field 6-ii already added under the same "carve-out" wording is the precedent this reading follows; banked as a card-wording imprecision, not a scope decision, for the coordinator to judge. **A second placement finding, discovered by `make rust`'s own test suite, not caught by `--check`-mode validation alone**: `specs/network/`'s folder is wholly claimed by the `restaurant` bounded context (RestaurantAccount/Restaurant/Prospect/RestaurantMembership/RestaurantInvitation), so `PlatformMembership` — a `platform`-context aggregate — could not land there without breaking `app_index_every_app_has_a_declared_boundary`; moved the aggregate, its command/event/errors/api mutation/rules/scalars to `specs/common/` instead, alongside the sibling `MailboxSupervision` actor already homed there for the identical reason (`docs/claude/dsl.md`: "`specs/common/` is ALWAYS a legal home — kernel promotion = a cross-scope contract"). The regenerated `actor-platform-membership` bin now depends on `domain-common` only, a correctness improvement (it dropped a network-scope config surface it never needed). **The `scope_membership.rs:196` passthrough drift young named at the briefing** (`scope_type: e.scope_type` in the `RestaurantAccessGranted` fold arm): checked and it is NOT a one-line-constant fix — it is an ordinary field passthrough from an event this slice never touches (`PlatformAccessGranted` never reaches this projector at all; ADMIN's own bridge is entirely separate), so per the card's own instruction it is noted here for a [#903](https://github.com/TheCaptainCompany/captain-food/issues/903)-class follow-up rather than touched in this slice. Gates: `make validate` 0 errors (warning baseline unchanged — the new `admin-sign-in` metrics were given real constructors in the SAME commit so no ratchet move was needed), `make rust` green (incl. check-drift, after the placement fix above and two more real regressions `cargo test --workspace` caught that `make rust` could not: a missing `SpecPlatformMembers` behaviour-test fake, and five test pins needing an update — `actor_client::partition`'s declared-width exception list, the `PlatformMember` `ProjectorGroup`'s scope label after the common-folder move, the migration-manifest pin in every `infrastructure` integration test binary, the fleet-parity gauge pin, and a stale pre-seam assertion in `read_scope_is_a_pure_claims_function` that predated the ADMIN seam existing), DB-gated `make test-crates` 0 failures — **substituting for the separate full `cargo test --workspace` the card also asked for, on the coordinator's explicit mid-run instruction**: this container's writable-layer ceiling is ~38 GB (not the 252 GB `df` reports) and the two runs together do not fit, confirmed by two real ENOSPC failures during this run before the coordinator's `target/debug/{deps,incremental,build}` cleanup held; `make test-crates` under the DB env compiles and runs the same workspace, so it stands as the workspace-test evidence, `cargo clippy --workspace --all-targets -- -D clippy::disallowed-methods` clean, `make wasm` green. Next: the coordinator's review, then 6-iii (System host routing) once this merges.

> **2026-09-05 — The ADMIN door decided by the team in two slices ([ADR-20260905-223957](../adr/ADR-20260905-223957-the-admin-door-in-two-slices-a-platform-membership-binds-the-admin-seam-then-the-system-host-is-routed.md), thirteen lenses): a `PlatformMembership` binds the ADMIN seam (6-v), then the System host is routed (6-iii).** The architect named the chunk: 6-iii is pinned "only once an admin can sign in" and no record had sliced the ADMIN door; step 7 depends on a complete 6; the value method binds. The one split — vernon: generalise `RestaurantMembership` with `ScopeType::PLATFORM`; evans/young/dba/business/graphql: platform standing is its own relationship — took the option that reverses no decided record (PRINCIPALS-MEMBER's "ScopeType is untouched"; RLS-SEQ (3)); vernon's mechanism-reuse point carried (seam reader, bridge recipe, projector-group-from-0 shared). Unanimous: the first admin is a RECORDED ACT through the mailbox — never a row, SQL or a data migration (a seeded row vanishes at the next checkpoint reset; a person in git is unerasable); `Identity::Admin` becomes unspellable without a grant (compiler-first); no public bootstrap ever. Rows: `ADMIN-DOOR-PRECONDITIONS` (open, eight items incl. the founder's external ones — controller entity and Art. 30 register, labour posture, the first admin's identifier as a secret), `PLATFORM-STANDING-VOCABULARY` (decided). Holub's waste warning recorded: (1) is one of seven open rider-restriction preconditions; seven consecutive dark PRs. GREEN second lane filed: [#904](https://github.com/TheCaptainCompany/captain-food/issues/904) (silent refresh + `?next=`).

> **2026-09-05 — #639 part C step 6-iv MERGED (PR [#901](https://github.com/TheCaptainCompany/captain-food/pull/901), squash `95542ada`) at the THREE-ROUND CEILING: the roster and the invitation, the accept as two commands in two lanes.** [ADR-20260905-101349](../adr/ADR-20260905-101349-step-6-lands-in-four-slices-the-bridge-and-the-grant-first-the-door-second-and-the-accept-is-two-commands-in-two-lanes.md) §2 (+ its round-2 amendment): the `RestaurantInvitation` aggregate (invite / revoke / accept; expiry a recorded fact via the TTL reminder, delivered through one `RecordLeg` arm); the accept is `AcceptRestaurantInvitation` then the PUBLIC `GrantRestaurantAccessByInvitation {invitationId, token}` — the membership id derived from the invitation (UUIDv5, so a second call folds to the same stream), the member id taken from the `(MEMBER, authSubject)` reservation holder when already bound (a re-hire is not refused), the caller proven by re-verifying the token against the accepted subject; `GrantRestaurantAccess` back to ADMIN with every field required (one name, one door — privilege fields unspellable on the public schema); the MANAGER-authority guard resolving off the recorded identity path, never the roster; `RestaurantRoster` / `RestaurantInvitationList` in `read_common` with own groups, named idempotent indexes, opposite executable rebuild recipes and the revoke delete arm; the flat `restaurantRoster` / `restaurantInvitations` queries (restaurant from `ReadScope`, `viewerAuthority` on the connection, `membershipId` per row); the `/team`, invite, revoke-row and `/invitation` screens; `RUN_RESTAURANT_INVITATION` (default off) gated by `RESTAURANT-INVITATION-PRECONDITIONS` (Art. 14 for the invited address; #902 delivery; retention ceiling; Art. 30; the consent preview as a read model; the unbuilt send leg; query spans; the guard repository's Postgres test — all flip preconditions); the `restaurant-invitation` contract with lag gauges.
> **Three rounds.** Round 1 landed the aggregate, the two-lane accept, the gate and the contract but not the read models or the screens, and WIDENED `grantRestaurantAccess` to `[ADMIN, PUBLIC]` — an anonymous caller could mint a MANAGER membership on any scope (reviewer, graphql, vernon, evans); the card's "STOP and report before widening" was satisfied after the fact — card defect banked: a stop clause must forbid the push. Eight lenses STOP. Round 2 landed all nineteen items incl. C/D; the executor also wired the parked expiry through a `RecordLeg` arm in the fenced mailbox handler — judged a legitimate record leg and recorded as ADR-081527 §8's THIRD carve-out; five lenses STOP on small spec/record items and one pre-existing flaky age gauge (rounding `now() - received_at`). Round 3 (the ceiling): ten items, all PASS on re-check by seven lenses; the gauge now takes an explicit `now` — which touched the fenced `birth_gap_watch.rs` on the CARD's instruction (coordinator's card defect), accepted by vernon and reviewer as a narrow FOURTH carve-out (a monitor's clock parameter), recorded on main with this entry. Follow-ups: [#903](https://github.com/TheCaptainCompany/captain-food/issues/903) (twenty-two items).
> **Lower-tier tally: first-round PASS 2 of 11** (#864, #892 PASS; #867, #870, #875, #882, #885, #895, #897, #899, #901 FAIL→PASS) — #901 is the FOURTH `HOLD: human` lower-tier PR to hit the ceiling (#875, #885, #899, #901): added to `LOWER-TIER-TRIP`. Ops: a full-workspace `cargo test` redirected to an uncapped file under the scratchpad exhausted the real disk ceiling (~38–40 GB, not the nominal 252 GB) — rule in sessions/environment.md; `make rust` must be run fresh per round (round 1 left six red items a crate-by-crate run hid).

> **2026-09-05 — #639 part C step 6-iv ROUND 3 (the CEILING) on [PR #901](https://github.com/TheCaptainCompany/captain-food/pull/901): all ten dispatched items land; PR stays DRAFT for the coordinator.** Per [ADR-20260826-084500](../adr/ADR-20260826-084500-one-review-pass-per-presentation-and-findings-are-triaged-not-chased.md), round 3 fixed exactly what round 2's presentation pass listed, nothing else. **R3-0 (CI red, beck)**: `birth_gap_watch_tick` took `now: DateTime<Utc>` as an explicit parameter bound into its three age-computing SQL queries instead of calling SQL's own `now()` at query time — the fixture and the tick previously raced real wall-clock time, rounding `1200s` up to `1201` on a slow run; the DB-gated test now pins one `now` for its whole run and passed twice in a row. **R3-1 (record)**: [ADR-20260904-081527](../adr/ADR-20260904-081527-rider-standing-is-a-grant-on-the-identity-row-the-doors-are-human-only-and-step-4-lands-in-three-slices.md) §8 gains its THIRD fence carve-out line (`RecordLeg::RestaurantInvitation`, one stream, Recorded/NoChange only), with a dedicated Consulted block. **R3-2 (legal, record-only)**: `/invitation`'s `gaps:` now says it carries no Art. 13/14 notice; the preconditions row's `revokeRestaurantAccess` typo corrected to `revokeRestaurantInvitation` (the actually-guarded mutation) and a "neither notice surface is built" clause added. **R3-3 (evans, spec-only)**: `MemberId`'s kernel description corrected — the value UUIDv5-derived on the invitation path is `MembershipId`, never `MemberId`; `back.team.authority.manager`'s French copy "Gérant" → "Responsable" (a statutory office the app does not confer). **R3-4/R3-6 (obs BLOCKING)**: `restaurant_roster_lag_positions`/`restaurant_invitation_list_lag_positions` gauges built (contract + constant + instrument + emit site, the `rider_standing_lag_positions` shape) for the two projector groups round 2 left silent; `restaurant_invitation::expired()` — declared since round 1, never called — now fires on the TTL reminder's `Ok(Recorded)` outcome. **R3-5/R3-8 (dba BLOCKING, ux)**: `RestaurantRoster` gains an ADDITIVE `RestaurantAccessRevoked`→DELETE arm (a declared `tombstone:`, the mechanical dispatch already answering `None` for it; the worker now turns that `None` into a real delete) — grant→revoke→row-gone, proven deterministic under a checkpoint-reset replay (never TRUNCATE, the table's own rule); the `/team` roster `gaps:` line reworded to state the list is DISPLAY-ONLY (the `AuthorityGuard` never reads it). **R3-7 (ux BLOCKING)**: `stories.yaml`'s `UnmuteParticipant` moved back to `HandleOrderConversation`'s closing step (round 2 reworded instead of repositioning); the invitation list's "Retirer" button — which rendered, and did nothing, on every terminal-status row — is now wrapped in a NESTED `conditional_section` (`item.status == 'PENDING'`, the corpus's only conjunction grammar, `rider.yaml`'s own precedent generalised into a list row for the first time — `crates/web/src/renderer.rs`'s `item_component_views` gained a `"conditional_section"` arm to make the nesting real rather than falling into the default info-row case); a render test proves no button on a REVOKED row, one on PENDING; the `access_pending` copy no longer says "try this link again" (which a non-PENDING re-run would refuse) but points at the page's own `/sign-in` control. **R3-9 (record lines)**: `RESTAURANT-INVITATION-PRECONDITIONS.yaml` gains four more flip preconditions — `invitationPreview` (final vision: a read model keyed by a token HASH, never a live identity call inside a public query), the invitation SEND leg (nothing sends the email yet), query spans for the two new queries (the lag-gauge half is now built, the span half is not), and the guard repository's own Postgres test. **Register check** (negative): no record governs the frozen-`now` clock-injection design; noted as an implementation choice in the commit body per the dispatch. Gates: `make validate` 0 errors (warning baseline unchanged), `make rust`, DB-gated `make test-crates` + full `cargo test --workspace` green, `cargo clippy -- -D clippy::disallowed-methods` clean, `make wasm` green.

> **2026-09-05 — #639 part C step 6-iv ROUND 2 on [PR #901](https://github.com/TheCaptainCompany/captain-food/pull/901), R2-D (screens) and R2-E (records) land, closing round 2; PR stays DRAFT for the coordinator's ready-flip.** `/team` now renders the live `restaurantRoster`/`restaurantInvitations` queries with per-row revoke (MANAGER-gated by the `AuthorityGuard`) and an invite sheet dispatching `inviteRestaurantMember`; a new hand-written `/invitation` screen (`crates/web/src/invitation_accept.rs`, `sdui: false` — the same two reasons `sign_in_return.rs` is hand-written, doubled: query-string extraction and acceptance-first dispatch+poll sequencing, here sequencing TWO commands client-side) sequences the two-lane accept, retrying the second leg (`grantRestaurantAccessByInvitation`) a bounded number of times so someone who already accepted never sees "link no longer valid". Two deviations from the R2-D card, both documented on the screen rather than built ad hoc: no revoke confirmation sheet (`open_bottom_sheet` carries no per-row context to open one against — the documented #870-class limitation); `invitationPreview` NOT built (needs a resolver mixing live token verification with a read-model lookup, no precedent in the mechanical query-emission pipeline). **R2-E records**: `docs/decisions/RESTAURANT-INVITATION-PRECONDITIONS.yaml` corrected Art. 13 → Art. 14 for the invited third-party address (14(3)(b) timing, 14(1)(d)/(2)(f) source), keeping Art. 13 for what `/invitation` itself collects; a `domain_events` retention-ceiling row declared `UNVERIFIED input` (counsel), an Art. 30 entry, and noted the MANAGER guard landed. **`make rust` found a genuine round-1 CI-red left uncaught**: `fact_route_gate` rejected the `RestaurantInvitationExpired` reminder's `deferred:` entry because its "WIRING, not modelling" reason never fit the allow-list's MODELLING-only vocabulary — the honest fix was finishing the wiring rather than reshaping the gate, so `RecordLeg::RestaurantInvitation` now exists end-to-end (`inbox.rs` + `mailbox/handler.rs`'s matching arm), closing [#902](https://github.com/TheCaptainCompany/captain-food/issues/902)'s poison-row-on-PITR concern; round 1's fence on `mailbox/handler.rs` named no concurrent claim on the file, so lifting it here was safe. The same `cargo test --workspace` pass also caught: two inline `_ => "technical_error"` catch-alls in `inbox.rs`'s invitation lane (moved to named classifier functions in `member_sign_in_reasons.rs`, the established out-of-file pattern for an unavoidable `String`-code match); a stale `REQUIRED_SCHEMA_VERSION` (bumped to the new migration's stamp); a stale hardcoded reminder/screen-corpus test list in two places (`tools/codegen-rs/src/tests.rs`'s `real_specs_carry_the_order_retention_pilot...` and `screen_roles_gate`); and an `identity-property-not-on-command` false alarm on `GrantRestaurantAccessByInvitation` (added to the calibrated-address-only allow-list, since the mailbox mints its actor id and the command correctly carries no `membershipId`). `ADR-20260905-101349` §2 gains the round-2 split amendment plus a dedicated Consulted block (11 lenses). `specs/common/scalars.yaml`'s `MemberId` description gains the invitation-minted-before-bridge paragraph (a kernel change, SPEC-LOG'd). One caught-and-corrected self-review moment: an invented `{{ uuid() }}` / `$ref` array-index syntax for the invite sheet's mint token and the authority-chip options was replaced with the established `{{ $uuid }}` sentinel and bare enum tokens before any gate ran.

> **2026-09-05 — #639 part C step 6-iv ROUND 2 on [PR #901](https://github.com/TheCaptainCompany/captain-food/pull/901), still DRAFT: the eleven-lens presentation-pass findings on round 1 (R2-A/R2-B/R2-C) landed; R2-D (screens) and R2-E (records beyond this entry) continue in the same round.** [ADR-20260905-101349](../adr/ADR-20260905-101349-step-6-lands-in-four-slices-the-bridge-and-the-grant-first-the-door-second-and-the-accept-is-two-commands-in-two-lanes.md) §2 amendment. **R2-0**: `crates/application/tests/reminder_schedule_pin.rs` pinned the 9th declared schedule row (the CI red round 1 missed). **R2-A** (reviewer/vernon/evans/graphql B1 STOP): round 1's `GrantRestaurantAccess` widened to `roles: [ADMIN, PUBLIC]` let an anonymous caller mint a MANAGER membership on any scope for any subject, because per-field authz cannot express "PUBLIC only for basis: MEMBER_INVITATION". Split into `GrantRestaurantAccess` (reverted ADMIN-only, all fields required) and new `GrantRestaurantAccessByInvitation` (`roles: [PUBLIC]`, `{invitationId, token}` only): verifies its own token, requires the invitation to be terminal ACCEPTED with `acceptedAuthSubject` equal to the caller's own proved subject (the caller-is-the-subject proof), derives `membershipId = UUIDv5(invitationId)` (`UNVERIFIED input` namespace, no controlling record) so one accepted invitation yields at most one membership, and reuses an existing held `memberId` for a re-hire or second-restaurant join. `AcceptRestaurantInvitation` now verifies its token UNCONDITIONALLY before the `is_pending()` filter (beck BLOCKING: round 1's order let an unknown/terminal invitationId never burn the token); the invitation fold is first-terminal-wins on replay (young); ACCEPTED's terminality is pinned by a unit test (vernon). **R2-B**: a genuine GraphQL-layer `AuthorityGuard` now enforces MANAGER authority on `inviteRestaurantMember`/`revokeRestaurantInvitation`, resolved through a new `MemberAuthorityRepository` reading the write-side `domain_events` log joined to the already-landed `member` identity bridge — never the roster projection this round adds (a roster rebuild window must never change what a write-path guard accepts); built WITHOUT touching the fenced `auth.rs` (the existing generic `Principal::user_id()` accessor already exposes the MEMBER subject). **R2-C**: `RestaurantRoster`/`RestaurantInvitationList` are real read models now — own projector groups born at 0, one migration, `restaurantRoster`/`restaurantInvitations` GraphQL queries (`restaurantRoster` returns a connection carrying `viewerAuthority` alongside the page — the only expressible MANAGER condition, resolver data never a second `roles:` list), executable rebuild-recipe tests (roster = checkpoint-reset-never-truncate, one creating arm; invitation list = truncate-and-reset-together, status grant-shaped). Two scope cuts, named rather than hidden: the roster has no `RestaurantAccessRevoked` arm yet (a revoked colleague stays listed — a follow-up beside #902), and the invitation list's `ACCEPTED_PENDING_ACCESS` → `ACCEPTED` transition (once the grant leg's own fact lands) is not built, its own named gap. `invitationPreview` (the R2-C card's suggested PUBLIC consent-page query) was NOT built: it needs a resolver combining live identity-provider verification with a read-model lookup, a shape with no precedent in this codebase's mechanical query-emission pipeline — STOPped and reported per the card's own escape clause, rather than invented under time pressure. `event-not-projected` warnings for the three invitation events, accepted in round 1's baseline, are now resolved (the baseline moved 97→94). Next: R2-D (screens) and R2-E (records) in the same round, then the coordinator's review.

> **2026-09-05 — #639 part C step 6-iv (the roster and the invitation) landed on [PR #901](https://github.com/TheCaptainCompany/captain-food/pull/901), DRAFT, `HOLD: human` — the write path is complete and DB-gated proven; the read models and screens are declared NOT LANDED, not silently skipped.** [ADR-20260905-101349](../adr/ADR-20260905-101349-step-6-lands-in-four-slices-the-bridge-and-the-grant-first-the-door-second-and-the-accept-is-two-commands-in-two-lanes.md) §2/§3 executed: the `RestaurantInvitation` aggregate (own stream/lane, `InviteRestaurantMember`/`RevokeRestaurantInvitation`/`AcceptRestaurantInvitation`, PENDING→{ACCEPTED,REVOKED,EXPIRED} lifecycle, `RestaurantInvitationExpired` reminder declared behind `RESTAURANT_INVITATION_TTL_SECONDS` = `UNVERIFIED input` 604800s/7 days), gated by `RUN_RESTAURANT_INVITATION` (default off, `RESTAURANT-INVITATION-PRECONDITIONS` open); the two-lane accept (`AcceptRestaurantInvitation` then `GrantRestaurantAccess(basis: MEMBER_INVITATION)`, client-sequenced, never a PM, ADR §2 FORK 1 Option A). **The B deliverable's STOP finding, discharged**: `GrantRestaurantAccess` was `roles: [ADMIN]`-only (6-i hand-provisioning); it now ALSO admits `PUBLIC` for `basis: MEMBER_INVITATION`, and the handler DERIVES `scopeType`/`scopeId`/`memberId`/`authSubject`/`authority` from the `RestaurantInvitation` stream `invitationId` names — never from the client's copies, which the wire schema now makes merely OPTIONAL for this basis rather than required-but-ignored. `invitationId` naming an ALREADY-accepted invitation IS the whole proof (that acceptance itself required the correct one-time token and the matching invited email); a second submission is the ordinary idempotent-replay path, never a privilege question. Also corrected from the card's draft wording: `InviteRestaurantMember.memberId` is CALLER-MINTED (ADR-0034), not handler-minted — a handler-random mint is untestable by this DSL's exact-literal behaviour-test comparison, and "ours, so it exists before any credential does" holds either way. **Two structural gaps hit this dispatch's fence and are declared, not worked around**: (1) the MANAGER-authority guard (OPERATOR must be refused) cannot be enforced in the aggregate's own command handler because the acting member's identity does not reach it — `crates/infrastructure/src/mailbox/handler.rs::resolve_actor`'s RESTAURANT branch is the same gap `Actor.domain_id`'s own doc already names as unbridged (#144), and that file was fenced for this card; a GraphQL-layer guard was the planned defense-in-depth but even IT depends on the roster read model, which is (2) below — so the guard is UNBUILT this round, named as a precondition of the `RUN_RESTAURANT_INVITATION` flip. (2) The `RestaurantRoster`/`RestaurantInvitationList` read models and the `/equipe`/`/invitation` screens are NOT landed — a real vertical slice needs Postgres migrations, hand-written projectors and GraphQL query wiring this dispatch's remaining budget could not also cover soundly after the aggregate/two-lane-accept/gate/observability/tests were built to a genuinely DB-gated standard; PROP-20260831-180622 §11 row 6 records the split explicitly. (3) The TTL reminder's SCHEDULING is real (`reminders:`/`schedules:`, ADR-20260810-231300) but its DELIVERY needs a `RecordLeg` match arm in the same fenced `mailbox/handler.rs` — parked `Unrecorded` via the DSL's `deferred:` grammar, tracked as [#902](https://github.com/TheCaptainCompany/captain-food/issues/902). What IS proven: `crates/infrastructure/tests/main/restaurant_invitation.rs`, DB-gated (real Postgres, application-layer handlers against `PgEventStore`, the `restaurant_membership.rs` precedent rather than a full HTTP walk — the HTTP/read-model stack does not exist yet), 8 tests, 3 named mutants planted and reverted (case-fold comparison; the no-enumeration byte-identical refusal between an unknown invitation and a wrong verified email; the grant leg preferring a client-supplied field over the invitation's own). Also fixed while landing: two PRE-EXISTING `GrantRestaurantAccess` struct literals (`restaurant_membership.rs`'s own DB-gated tests) needed `Some(...)` wrapping now that those fields are `nullable` for the new basis — a direct, mechanical consequence of the B design, not a new decision. §8.2's `ADMINISTRATOR` (a stale draft word predating the ADR's settled `MANAGER`/`OPERATOR` vocabulary) fixed in the living PROP. Next: the coordinator's review of the declared C/D gap and #902, then 6-iii (System host routing) last.

> **2026-09-05 — #639 part C step 6-ii MERGED (PR [#899](https://github.com/TheCaptainCompany/captain-food/pull/899), squash `6369aeb1`) at the THREE-ROUND CEILING; the public-graph limits landed in the same slice, as the founder decided.** [ADR-20260905-101349](../adr/ADR-20260905-101349-step-6-lands-in-four-slices-the-bridge-and-the-grant-first-the-door-second-and-the-accept-is-two-commands-in-two-lanes.md) §7–§10: the PUBLIC `requestMemberSignInLink`/`confirmMemberSignIn` pair (identify-only, no enumeration oracle, bridge read on the confirm leg only), the role-only `stamp_member_claim`, the auth seam's MEMBER arm, `sign_in`/`sign_in_return`/`not_linked` on the restaurant back-office, the `member-sign-in` contract, `RUN_MEMBER_SIGN_IN_DOOR` (default off; flip gated by `MEMBER-SIGN-IN-DOOR-PRECONDITIONS`), and — §9 — the per-role GraphQL depth/complexity limits: a `parse_query` extension on the ONE schema refusing before any resolver, keyed on `ActingRole`, fail-closed to PUBLIC, ceilings codegen-derived per role from the generated client documents × `GRAPHQL_LIMIT_HEADROOM_PERCENT` (50, `UNVERIFIED input`), a staleness gate, histograms on every parsed document and `graphql_limit_max{role,kind}` asserted from the schema build. PROP-20260831-180622's `public-graph-limits` Concern is ticked with file:line.
> **Three rounds.** Round 1 (executor deferred E citing the Concern's earlier either/or wording — a decided record contradicted; the card's "STOP and report" read as "defer", card defect banked; plus the confirm leg unreachable with an ungated bounce, a tautological mutant, asymmetric composition roots, three screen defects, two contract truth defects, a declared-but-unbuilt send wall, a false PR body). Round 2 landed all 19 items; six lenses PASS, ux STOP on ONE spec-only defect — the typed address echoed through a `text` node that resolves at paint from resolver data, painting an empty paragraph (the #870 class, recorded at `renderer.rs`). Round 3 (the ceiling): that revert + `required: [staging, production]` on `EMAIL_QUOTA_KEY_HMAC_SECRET` so a landed comment became true. The container restarted mid-round-2 after the push and before the hand-back; an evidence-only executor reconstructed it (4 mutants red, M-E3 caught by the codegen drift test not the boundary pair — the pair self-derives; M-F1 untestable: nothing exercises the monolith's DB-gated composition root, → #900). Non-blocking: [#900](https://github.com/TheCaptainCompany/captain-food/issues/900) (19 items incl. introspection scored (0,0) on `/public`, mutations enforced but never counted, lawful basis for the send unnamed, the secret-provisioning class defect). Founder queue (admin-gated): provision `EMAIL_QUOTA_KEY_HMAC_SECRET` before the #358 cutover.
> **Lower-tier tally: first-round PASS 2 of 10** (#864, #892 PASS; #867, #870, #875, #882, #885, #895, #897, #899 FAIL→PASS) — #899 is the THIRD `HOLD: human` lower-tier PR to hit the ceiling (#875, #885, #899): added to `LOWER-TIER-TRIP` as evidence. Ops: disk squeezes now happen at clippy/wasm right after `make test-crates` (6.7 → 2.6 GB) — `rm -rf target/debug/incremental` recovers 6–12 GB; recorded in sessions/environment.md.

> **2026-09-05 — #639 part C step 6-ii (the door) landed on [PR #899](https://github.com/TheCaptainCompany/captain-food/pull/899), still DRAFT, `HOLD: human` — one Concern from the PROP does NOT discharge, reasoned and recorded rather than skipped silently.** [ADR-20260905-101349](../adr/ADR-20260905-101349-step-6-lands-in-four-slices-the-bridge-and-the-grant-first-the-door-second-and-the-accept-is-two-commands-in-two-lanes.md) executed: PUBLIC `requestMemberSignInLink`/`confirmMemberSignIn` (byte-identical accept response for a real member and a stranger, the `Member` bridge never consulted on the request leg — the enumeration-oracle property), the third role-only stamper `identity.stamp_member_claim` (`{role: MEMBER}`, no id in the claim, mirroring the customer/rider precedent), the MEMBER arm of `crates/server/src/auth.rs`'s identity seam (`auth_subject -> Member -> ScopeMembership`, failing closed unless exactly one restaurant scope resolves — V0 does not support more than one grant per member), `sign_in`/`not_linked` screens on `restaurant_backoffice.yaml`, the `member-sign-in` observability contract, an email send-abuse wall reusing `application::sms_guard`'s generic quota primitives under an `email:` key namespace (proven not to collide with SMS's `global:day` bucket), and the `member_sign_in_door.rs` DB-gated suite with five named mutants (M1 enumeration-oracle timing/shape, M2 the claim-stamp PUT body, M3 the bridge-never-consulted assertion, M5 the door-closed-before-touching-the-provider ordering) all seen red and reverted. **Item E (per-role GraphQL depth/complexity limits) did NOT land, and the PROP's `public-graph-limits` Concern stays unchecked** — the server serves ONE master `async_graphql::Schema` for all seven roles (role-as-path is a runtime ACL over `ActingRole`, not seven compiled schemas), so async-graphql's built-in `Schema::build(..).limit_depth/.limit_complexity` (baked at schema-BUILD time) cannot differ per role; a genuinely per-role ceiling needs a custom `parse_query` extension (mirroring the existing `ScopeSlice` extension almost exactly — reading `ActingRole` from the request-scoped `ExtensionContext::query_data`, verified reachable there) plus a codegen-derived per-role ceiling table, neither built this slice. The `graphql-limits` observability contract, telemetry constants and meters ARE landed so the extension only needs to call them when it does. Reason recorded on the PR's own comment per the Concern's stated alternative ("land in the same slice, or record the decision to ship without them and why"); a follow-up issue could not be filed (GitHub issue-creation blocked in this session; only a PR comment went through). Also deferred: the hand-written `sign_in_return` confirmation-landing page — building a full `HandWrittenScreen` Leptos page was out of scope for the slice's remaining budget, so the gap is a declared `gaps:` line on `sign_in` instead. **Codegen fallout fixed while landing**: `every_arm_of_the_human_owned_router_names_an_inbox_variant` flags a catch-all ANYWHERE in `inbox.rs`, not only in lane matches — the two new `code.as_str()` reason-mapping helpers had to move to their own module (`crates/infrastructure/src/member_sign_in_reasons.rs`) rather than live beside the router; the identity-property calibration list widened from 3 to 5 (the member door's two commands are the same "caller cannot know an id before it signs in" shape as the rider door's); the screen-roles-gate corpus assertion widened to name the two new PUBLIC transport-role screens. Disk: `target/` hit 23 GB mid-run and free space dropped to 4.6 GB — coordinator cleared `target/debug/incremental` proactively; `rm -rf target/` run before the next `make rust` per the standing operational note. Next: the coordinator's ready-flip decision on #899 given the open Concern, then 6-iv (roster and invitation), 6-iii (System host routing) last.

> **2026-09-05 — #639 part C step 6-i MERGED (PR [#897](https://github.com/TheCaptainCompany/captain-food/pull/897), squash `e7582283`) after two review rounds — the person and the grant exist in the model before the door does; ships dark, inventory until 6-ii.** [ADR-20260905-101349](../adr/ADR-20260905-101349-step-6-lands-in-four-slices-the-bridge-and-the-grant-first-the-door-second-and-the-accept-is-two-commands-in-two-lanes.md) §1–§7/§11–§12 executed: the `RestaurantMembership` aggregate (`GrantRestaurantAccess` accepting `basis: CAPTAIN_ONBOARDING` only, `RevokeRestaurantAccess` with a closed two-value ground plus a read-only `UNRECOGNISED` and no free text; ADMIN-only, `requires: acting`), the `Member` bridge (own projector group from 0, `auth_subject UNIQUE`, first-write-wins — the binding outlives any one grant), `ScopeMembership`'s targeted MEMBER arms (a delete by `membership_id`, never the broad `revoke_role` arm), `BoundPrincipal::Member` on 2a's `auth_subject_reservations` (no new table, no release on revoke), the gate `RUN_MEMBER_ACCESS_GRANT` default off bound to the new `MEMBER-ACCESS-GRANT-PRECONDITIONS` row (six preconditions, Article 17 erasure among them), the spec-only fold `restaurants_with_a_bound_member`, and the rebuild recipes as executable tests including the TRUNCATE-denial pair. Round 1: reviewer, evans, legal and dba STOP on one item each — the bam projection named `RestaurantStaffAccess` against the ADR's own §3 (a CARD defect, the coordinator suggested the name), a wrong count in SPEC-LOG, the missing erasure precondition, and a `Member` upsert that would REBIND a person's subject on a second grant; young, vernon, beck PASS. Round 2 landed all four and six converging one-liners — chiefly `membershipId` made REQUIRED and caller-minted (vernon: the handler's mint branch was unreachable over the mailbox, whose lane address requires the id; young: an omitted id would duplicate a membership) while nothing yet consumes the command. Non-blocking → [#898](https://github.com/TheCaptainCompany/captain-food/issues/898) (the dead generated `ScopeMembership` dispatch would fold a revoke as an UPDATE if ever wired — a validator rule for set-shaped tables; the rewind-only replay is non-monotonic on a live log; checkpoint fan-out at #514; `member_type`'s retype). Legal's five counsel-packet questions (controller vs joint controller and who owes the Art. 14 notice; a manual erasure procedure vs a `deletion:` block; whether `LEFT_THE_RESTAURANT` is a necessary inference under Art. 5(1)(c); the lifetime reservation under an erasure request; the Supabase DPA) appended to the counsel section. **Two coordinator defects the day surfaced**: a docs-only push added an unknown key to a decision row and main's `make validate` was red for ~65 minutes until the 6-i executor found it (rule: amend inside an existing field; run validate before pushing anything a validator reads); #639 was auto-closed at the #892 merge by a closing keyword in a PR body written as `Refs` (reopened; rule: grep the body before the ready flip). **Lower-tier tally: first-round PASS 2 of 9** (#864, #892 PASS; #867, #870, #875, #882, #885, #895, #897 FAIL→PASS) — two rounds, not the ceiling; of #897's four blockers one was the card's, three were lens catches on fresh shapes. Next: 6-ii — the door (`requestMemberSignInLink` / `confirmMemberSignIn`, the role-only MEMBER stamp, the public-graph limits on every role's schema with codegen-derived values, the `member-sign-in` contract), with the silent refresh retry named as a dependency of the flip.

> **2026-09-05 — Step 6 (the staff roster and the door) decided by the team in four slices, thirteen lenses, all consenting.** [ADR-20260905-101349](../adr/ADR-20260905-101349-step-6-lands-in-four-slices-the-bridge-and-the-grant-first-the-door-second-and-the-accept-is-two-commands-in-two-lanes.md): 6-i the bridge and the grant (one aggregate, one stream, `CAPTAIN_ONBOARDING` only, gated `RUN_MEMBER_ACCESS_GRANT` default off — the first real grant about a Tours human is the irreversible moment that starts every legal clock), 6-ii the door (`requestMemberSignInLink` / `confirmMemberSignIn`, a role-only MEMBER stamp, the public-graph depth/complexity limits on EVERY role's schema with codegen-derived values, the enumeration oracle as a test with the bridge consulted zero times on the request leg, a `member-sign-in` contract in the same PR, gated `RUN_MEMBER_SIGN_IN_DOOR`), 6-iv roster and invitation (the accept as TWO commands in two lanes — vernon's objection to FORK 1's A, that a two-stream handler is a foreign-stream append under the open isolation plan, answered without a route gate), 6-iii System host routing LAST and only with a refusal screen (an anonymous SSR would render the admin shell; an un-darkened board with 401'd reads shows a lane holding a paid order as "none"). Settled: FORK 1 → A (young, evans; vernon's constraint carried); `MemberAuthority = MANAGER | OPERATOR` (evans: `ADMINISTRATOR` collides with the platform ADMIN); `AccessEvidence` → `AccessBasis`; the reservation is 2a's table, not a new one (dba); no `member_id` in the claim (graphql-architect, farley). Recorded plainly: 6-ii + 6-iii do NOT discharge RIDER-RESTRICTION-PRECONDITIONS (1) — ADMIN stays hand-provisioned, the row is amended; production stays suspended, so 6-ii is "walkable end to end on one database", not "usable in Tours" (holub, farley); the one-hour cookie with no refresh caller in the client is a real 19:40 lockout — a silent refresh retry is a named dependency of the door flip (ux). Legal's preconditions for the first grant (closed revoke ground, Art. 14 notice, Art. 30 entry, the lifetime reservation written down) and four external items for the founder (Supabase DPA/region, Art. 26 joint controllership, `support@` as rights channel, SPF/DKIM/DMARC on `captain.food`). PROP §6.4/§6.5/§13 rewritten in place. Next: card 6-i, sonnet executor.

> **2026-09-05 — #639 part C step 5 MERGED (PR [#895](https://github.com/TheCaptainCompany/captain-food/pull/895), squash `eac2a12e`) after two review rounds — the restriction fact terminates the rider's socket, behind `RUN_RIDER_RESTRICTION_SOCKET_CLOSE` (default off, ships dark).** [ADR-20260905-065415](../adr/ADR-20260905-065415-the-restriction-fact-terminates-the-rider-s-socket-a-connection-local-standing-read-inside-the-guard-and-one-writer-to-the-transport.md) executed: a per-connection watcher on the in-process `EventBus` (stream name and event type DERIVED from the domain types), a connection-local `RiderStandingCell` typed `RiderStanding` and restrict-only BY TYPE, read first by `StandingGuard` so queries, mutations and subscriptions over graphql-ws refuse in one emitted place while the carve set survives; then a readable close 4403 through `GraphQLWebSocket::new_with_pair` with ONE forwarder owning the sink. Round 1 (sonnet, protocol followed claim-first this time): reviewer, beck and observability STOP, five lenses PASS. The reviewer's blocker was the one that mattered: the Lagged/Closed re-derivation read `RiderRoster` — a different projector checkpoint group that a rebuild re-grants ACTIVE for the length of the drain — instead of the `Rider.standing` grant the connection resolved through `RiderIdentitySource`, so the one arm written to fail closed failed OPEN under roster lag; round 2 routes it through the same seam ("one function, three callers", ADR-124600 §3). beck: two tests had never been seen red (the gate-OFF rollback evidence; the "always-close-on-Lagged must fail" prose) — mutants planted and quoted, with the learning that a boolean gate must be flipped at EVERY read site or the mutant is inert. observability: the `watch_live` dead-man gauge was registered only inside the spawned watcher — "gate ON, no rider connected" yielded no timeseries instead of 0, the exact defect class CLAUDE.md names; now registered at the composition root like `otp_send::guard_enforcing`. Non-blocking → [#896](https://github.com/TheCaptainCompany/captain-food/issues/896) (bounded forwarder drain and its signal, rider-path-only subscribe, Lagged-Ask timeout, `myStanding` on the frozen scope, one spy binary, one-channel collapse, flip-ADR inputs); the client leg is [#894](https://github.com/TheCaptainCompany/captain-food/issues/894). Not-a-finding, recorded: the second timed broadcast channel (stamped inside `publish`, one consumer, fails closed on its own lag); scenario 3 relocated to a deterministic guard unit test. **Lower-tier tally: first-round PASS 2 of 8** (#864, #892 PASS; #867, #870, #875, #882, #885, #895 FAIL→PASS) — two rounds, not the ceiling. Disk: round 1's 27 GB `target/` filled the disk mid-round-2; `rm -rf target/` again — the executor protocol's disk step needs to run at every round start, not only at dispatch. Next: step 6 (the staff roster and the magic-link door — the first outcome a human in Tours can experience), then 7; holub's ordering advice sits on the founder's queue.

> **2026-09-05 — Step 5 (the restriction fact terminates the rider's socket) decided by the team, thirteen lenses, all on one option.** [ADR-20260905-065415](../adr/ADR-20260905-065415-the-restriction-fact-terminates-the-rider-s-socket-a-connection-local-standing-read-inside-the-guard-and-one-writer-to-the-transport.md): a per-connection watcher on the in-process `EventBus` matching the connection's OWN `Rider-{id}` / `RiderRestricted` (both derived from the domain types, never literals — evans), a connection-local `watch::Sender<RiderStanding>` read INSIDE `StandingGuard` (graphql-architect: queries and mutations run over graphql-ws too, so the frozen `connection_init` scope was the #641 widening — one emitted place closes all three operation kinds and keeps the carve set), then a readable close 4403 through `GraphQLWebSocket::new_with_pair` with ONE forwarder owning the sink (vernon: one writer to the transport). `Lagged` is not benign here — a dropped `RiderRestricted` has no next envelope (young, obs): re-derive once, RESTRICTED → terminate, error → never terminate (ADR-124600 §3, farley). Behind `RUN_RIDER_RESTRICTION_SOCKET_CLOSE` default OFF — the one split (farley: deploy ≠ release; holub/business: a gate on a gate) taken by the ADR-013834 rule. Multi-instance is BUS-1's (the bus is in-process; the gateway 501s WS today) — farley and architect asked for the `event_wake` fact source now; answered: that is BUS-1's final form, not one slice's partial realization. The client leg (the Leptos client discards the close code and reconnects) is a named gap with an issue; until it lands the rider learns on the next tap. Legal: the close discharges gaps 1 and 3 without changing the promise, is a notification without reasons (the statement lives on `/restricted`), one new counsel question. **holub: step 6 should go first** — step 5 is the fifth dark PR; the founder set the order 3, 4, 5, 6, 7, so the advice goes to his decision queue and step 5 proceeds as one small PR. Card defect banked: the brief conflated the in-transaction `pg_notify` with the post-commit bus (dba, architect).

> **2026-09-05 — #639 part C step 4-iii-B MERGED (PR [#892](https://github.com/TheCaptainCompany/captain-food/pull/892), squash `1470eea3`) in ONE review round — the first part-C slice on the lower tier to pass its presentation with no blocking finding.** [ADR-20260904-152807](../adr/ADR-20260904-152807-the-admin-s-hands-one-custody-truth-read-at-query-time-a-door-that-refuses-until-the-notice-exists-and-two-slices.md) §8 executed: gauge `rider_restricted_holding_job_age_seconds` on the `rider-restriction` contract, emitted from the 3-ii tick after the handback gauge and before the shared heartbeat (named as the liveness proof in prose — the observability grammar has no `liveness:`/`events:`/`severity:` field and silently ignores unknown keys, so an invented one would enforce nothing), ONE set-based statement with `rider_restriction.standing = 'RESTRICTED'` as the build side and `now − effective_at` as the anchor (a reinstatement leaves `effective_at` populated — the predicate on `standing` is what makes a re-restricted rider visible), one info event per stranded row, key `RIDER_RESTRICTED_CUSTODY_MAX_AGE_SECONDS` default 1800 written as `UNVERIFIED input`, a generic codegen test locking every single-key threshold to its key's default, a single-binary sequential DB-gated suite (the spy meter is process-global) with three planted mutants seen red. **What made round one pass**: the card went to the three declared-concern lenses BEFORE code and all three returned STOP-with-fixes (grammar facts, the reinstatement predicate, the test structure) — folded into the card, not found in review. **Item E (`heldJobAtDecision`) did not land**: `RiderRestricted` carries no `deliveryJobId` and the bam fold grammar has no cross-stream primitive; the executor stopped per the card's escape hatch and every lens confirmed the stop — the re-declaration is [#877](https://github.com/TheCaptainCompany/captain-food/issues/877)'s, and §8's sentence gets its amendment line when that decides. Non-blocking → [#893](https://github.com/TheCaptainCompany/captain-food/issues/893) (test tightenings: `threshold_exceeded` asserted by nothing, EXPLAIN hand-copies the SQL, codegen test skips keys without a default), [#883](https://github.com/TheCaptainCompany/captain-food/issues/883) (the heartbeat proves completion, not currency — a slow sweep keeps the liveness proof green while the gauges go stale). Process note: the executor implemented before completing the claim/branch/draft-PR sequence and caught it before any push — the next card puts the claim as the literal first command after the budget timer. **Lower-tier tally: first-round PASS 2 of 7** (#864, #892 PASS; #867, #870, #875, #882, #885 FAIL→PASS). Next: step 5 (socket termination on the restriction fact, `HOLD: human` — runtime), then 6, 7.

> **2026-09-05 — #639 part C step 4-iii-A MERGED (PR [#885](https://github.com/TheCaptainCompany/captain-food/pull/885), squash `1b1238cb`) after three rounds — the ceiling, hit for the second time on a `HOLD: human` lower-tier PR.** Round-2 re-check (Opus: reviewer FAIL, beck/dba/legal/ux PASS on their round-1 items) confirmed every round-1 blocker fixed and found two NEW blockers in the round-2 delta, both of the same shape: the new `screen-condition-on-form-field` rule un-gated controls that had never been reachable, and two of them (`claim_resolve`'s partial-refund amount picker, a LIVE money screen; the rider issue sheet's free-text note) turned out to have NO renderer arm — `text_area` and `tip_amount_selector` fall to the generic catch-all and paint an empty tagged div. "A control that renders but does nothing is worse than no control", made visible by this PR. Round 3 (sonnet executor, seven items, all landed, `159a90a3`): the rule un-scoped at both sites (a rule excluded by filename is a weakened gate — five lenses) with `condition_subject_roots` scanning every identifier root, not the leading one (beck's right-operand hole); both inert controls DELETED with honest `gaps:` lines owned by [#888](https://github.com/TheCaptainCompany/captain-food/issues/888); the roster migration's header had claimed a re-run "would fail on a duplicate index name" when unnamed `CREATE INDEX` silently creates `_idx1` duplicates (dba) — fixed compiler-first: the SQL emitter now names every index Postgres-default-style and emits `IF NOT EXISTS`, 61 indexes regenerated, a codegen test gates the GENERATED output (never `migrations/**`, whose 61 applied files legitimately carry the old shape); the #887 link made clickable; three cheap test gaps closed (equal-`occurred_at` tie seed; a positive RESTRICTED-facts assertion replacing a false coverage claim; `reinstate_rider_error` pinned). Confirmation pass (Opus: reviewer, dba, ux): one mechanical STOP — the reworded SPEC-LOG row had hard newlines inside a table cell — corrected by the resumed executor, no fourth round.
> **Deviation (a)** — the tip `recipient: RIDER` literal on a control with no production caller — accepted NON-BLOCKING by all five lenses, [#887](https://github.com/TheCaptainCompany/captain-food/issues/887) owns it and must land before any caller (legal + dba condition). **Card defects banked (coordinator, all three)**: "render unconditionally" without checking each control had a renderer arm; a clippy gate line stricter than CI's actual command; a stated chip count ("four" — six) and a missing `loop-budget.sh start`. Issues filed from non-blocking findings: #888 (renderer arms + exhaustive `ComponentKind` match), [#889](https://github.com/TheCaptainCompany/captain-food/issues/889) (RIDER_REQUESTED copy), [#890](https://github.com/TheCaptainCompany/captain-food/issues/890) (`claim_resolve` has no `inline_error`, a guaranteed money-path refusal lands silent — pre-existing), [#891](https://github.com/TheCaptainCompany/captain-food/issues/891) (column-derived index names orphan the old index on reshape; name uniqueness gate). Test-bed learnings: [gates.md §19f](../claude/sessions/gates.md). **Lower-tier tally (ADR-20260904-013450 §5): first-round PASS 1 of 6** (#864 PASS; #867, #870, #875, #882, #885 FAIL→PASS); #885 is the SECOND `HOLD: human` lower-tier PR to hit the ceiling — added to `LOWER-TIER-TRIP`'s evidence, the row stays the founder's. Note for the tally's honesty: of #885's round-2/3 blockers, one was a card defect (the renderer-arm assumption) and one a lens depth find on a fresh header sentence (dba); neither was an executor depth miss against an ADR line.
> Next: 4-iii-B (the `rider_restricted_holding_job_age_seconds` gauge section on the 3-ii tick, ADR-20260904-152807 §6), then steps 5, 6, 7 of PROP-20260831-180622.

> **2026-09-05 — #639 part C step 4-iii-A round 2 (PR [#885](https://github.com/TheCaptainCompany/captain-food/pull/885), still DRAFT): the presentation-pass findings fixed, and a new corpus-wide validator rule found three more real, pre-existing defects on the way.**
> Item 1 (beck): `crates/server/tests/rider_standing_walk.rs`'s door-OFF test hard-coded the READ side's `RunRiderRestrictionDoor(true)` via its `schema_over` helper, so it never actually proved the key stays off the read guard — parameterised, M6 (`StandingGuard` consults the door and skips the refusal when closed) live-planted, confirmed red (`assertion left == right failed... left: 0 right: 1`), reverted. Item 2: a vacuous `contains(A) || contains(B)` assertion (the right disjunct always true) sharpened to the RESOLVED `data-vars` payload. Item 3 (dba): `riders` (list) and `rider` (detail) could name DIFFERENT held jobs for the same rider — `requested_at DESC` alone is not a total order, and the list's `.collect()` into a `HashMap` was LAST-wins (oldest survives) while the detail's `LIMIT 1` picks the newest; both queries now share `requested_at DESC, delivery_job_id DESC` and the map is built first-wins, pinned by a new DB-gated test (confirmed red on the old code: two ASSIGNED jobs on one rider returned two different `heldDelivery.id`s). Item 4 (legal): a reinstated rider's past ground/effective-since no longer read as present tense — both now nest under a `standing == 'RESTRICTED'` gate, and a new "Rétabli le" row renders `reinstatedAt`. Item 5: `reinstate_rider` gets an `inline_error`. Item 6/13 (beck, reviewer) — **NOT fixed, flagged**: the round-1 diff hardcoded `recipient: RIDER` on the courier-tip control, reversing ADR-20260722-181500's "RIDER, or RESTAURANT for self-dispatch"; reverting to the literal prior binding (`order.tipRecipient`) is verified NOT safe — `Order` has no such field, so the revert trips this round's OWN new `screen-sheet-binding-unknown` rule on the real corpus (confirmed by running it). Left as landed, with the finding fully documented in the spec comment and a screen `gaps:` line, for the coordinator's own decision — no field exists anywhere to bind the correct value from today. Item 7 (farley, vernon): `RUN_RIDER_RESTRICTION_DOOR` was missing from BOTH composition roots' fleet-parity `declare_flag` calls (a split-fleet deploy of this key was invisible) — added to both, and to `mailbox_liveness_metrics.rs`'s pin. Items 8–10: the legal doc names Directive (EU) 2024/2831; the migration's stale `IF NOT EXISTS` claim corrected; the `phone_call` render test asserts the positive.
>
> **ADDENDUM (reviewer FAIL) item 12 — the recurring #870 class, finally made mechanical.** The `RIDER_REQUESTED` sentence was gated on `ground.value == 'RIDER_REQUESTED'` — `ground` is the SHEET'S OWN chip, a form field, never resolver data, so it could never render, for any ground, ever (verbatim the #870 round-2 defect, journal W36 ~443-451, recurring because it was recorded as prose, not a gate). Rendered unconditionally; a new codegen rule **`screen-condition-on-form-field`** (ERROR) makes the whole class unspellable — a `condition:`/`visible_when:` whose subject is a form field declared in the same screen/sheet. Wiring it against the REAL corpus found the class alive in THREE more places, none related to this round: `restaurant_backoffice.yaml`'s `claim_resolve` screen (the partial-refund amount picker — a MONEY-PATH control that could never appear regardless of which resolution chip an admin picked) and its `issue_resolution_sheet`'s note field, plus `restaurant_frontoffice.yaml`'s `rating_sheet` late-delivery reason field — all three fixed the same way (render unconditionally). A FOURTH instance, `issue_kind.value` in `screens/rider.yaml`'s `rider_report_sheet`, is excluded from the wired rule: this round's dispatch forbids touching that file (a concurrent-session merge fence), and the instance is already documented, in a PRIOR round's own comment, as a deliberately-deferred gap — the rule is scoped OFF that one file rather than either breaking the fence or weakening the rule corpus-wide, and the exclusion is itself commented at the wiring site. Item 13 confirms item 6 as a decision reversal (see above). Item 14 (ux): the sheet's "holds a job — récupérée" sentence claimed collection even for an ASSIGNED (not yet collected) job; split into two stage-honest sentences gated on `rider.heldDelivery.status` (genuinely resolver data).
>
> **Mutant verdicts, as executed, not as designed (item 11, farley's own distinction).** OBSERVED RED at some point across rounds 1+2: M6 (round 2, this session), M7 and M8 (round 1). NEVER OBSERVED RED, in any run to date — pinned only by the written tests' own positive assertions, per round 1's own explicit accounting: M1, M2, M3, M4, M5. A claim about design is not a claim about execution; recorded as such rather than rounded up.
>
> **Gates** (wall-clock, this session): `cargo build --workspace` clean; `make validate` 0 errors throughout, before and after every mutant plant, including the two new-rule discoveries; DB-gated `bash tools/db-preflight.sh && DATABASE_URL=... DB_TESTS_REQUIRED=1 make test-crates` — 209 test binaries, 0 failed; `cargo clippy -p web -p server -p infrastructure -p application -p captain-food-codegen --all-targets -- -D clippy::disallowed-methods -D clippy::mistyped_literal_suffixes` exit 0; `cargo test --manifest-path tools/codegen-rs/Cargo.toml --bin generate` 429/429 (425 + 4 new for `screen-condition-on-form-field`); `cargo test -p web --lib` 170/170 (169 + 1 new); `make wasm` OK; `make link-check` 8671 links / 0 broken. Fence self-check: the mailbox fence prints exactly `crates/infrastructure/src/inbox.rs` and `crates/infrastructure/src/mailbox/standalone.rs`, nothing new; `specs/screens/rider.yaml`/`specs/observability.yaml` print nothing (an earlier draft of item 12's corpus-wide fix DID touch `rider.yaml` — caught by this same self-check before commit, reverted, and the validator rule scoped off that file instead, see above).
>
> **2026-09-04 — #639 part C step 4-iii-A landed (PR [#885](https://github.com/TheCaptainCompany/captain-food/pull/885)): the admin's hands, the roster, the sheet and the release-gate door key.**
> [ADR-20260904-152807](../adr/ADR-20260904-152807-the-admin-s-hands-one-custody-truth-read-at-query-time-a-door-that-refuses-until-the-notice-exists-and-two-slices.md)'s slice A, executed. `RiderRoster` (own checkpoint group from 0, `derive:`-mechanical `standing`, no `auth_ref`), the `riders`/`rider` ADMIN queries (held-first/restricted-first order computed over the whole roster before paging, per ADR §2/§4 — the page boundary must not split the held group), `held_by_riders` as a set-based batch port beside the existing `held_by_rider`; screens `riders`/`rider_detail` + the `restrict_rider_sheet` on `specs/screens/system.yaml`, DARK per ADR §9 (no System-host route, no admin door — `every_screen_of_every_surface_renders` cannot see them, dedicated render tests are the only cover); the write door `RUN_RIDER_RESTRICTION_DOOR` (default OFF, bound to the open `RIDER-RESTRICTION-PRECONDITIONS` row via a new `decisionRow:` field) refusing `restrictRider` with the typed `RiderRestrictionDoorClosed` before the store is touched, `reinstateRider` and the read-side `StandingGuard` never consulting it (a walk-test mutant). Two new codegen rules: `screen-sheet-binding-unknown` (extends §25's binding walk to a screen's opened sheets — wiring it full-strength found and fixed TWO real, pre-existing dead bindings in `restaurant_frontoffice.yaml`'s `rating_sheet`, unrelated to this slice) and `decision-row-open-key-must-be-off` (the `RUN_SIRENE_WORKER` lesson made executable). `PageOffset` promoted `specs/network/scalars.yaml` → `specs/common/scalars.yaml` (kernel purity).
>
> **Two renderer defects found and fixed while wiring these screens, both pre-existing and shared with already-shipped screens**: a standalone `{ type: badge, text:, variant: }` node (restaurant_backoffice's `delivery_issue_card`/`delivery_handback_card` used this exact shape already) fell into the generic catch-all, which reads `title`/`label`/`value` — never `text` — so it rendered an empty node with no `data-variant`; fixed at both the screen-level and list-item-level dispatch, guarded on the declared field so the mailbox lanes screen's older `label:`/`value:`/`variant_when:` badge shape is untouched. `phone_call`'s target prop is `number:`, not `phone:` — no runtime consumer existed for this action before this screen, so the wrong field name had never been caught; the roster detail's phone control would have rendered permanently disabled. Reported, not silently fixed elsewhere: `item_action` on the generic `list` component (mine and `restaurant_backoffice.yaml`'s `claims_list`) is declared but never consumed by the renderer in SSR or hydrate — a bigger lift than the badge fix, left as a `gaps:` line and a finding for the architect.
>
> **Gates** (wall-clock, this session): `cargo build --workspace` clean (~2m28s cold after a `rm -rf target/` at ~424K free — the environment ran out of disk mid-run once, recovered); `make validate` 0 errors throughout, both before and after every mutant plant; `cargo test -p web --lib` 169/169 (was 162 before this change); `cargo test -p infrastructure --test main -- roster`/`rider_projection` 12/12 (4 new RiderRoster fold tests, DB-gated, real Postgres); `cargo test -p server --test rider_standing_walk` 2/2 (the existing walk extended with `riders`/`rider` ADMIN assertions + custody-not-released-by-reinstatement, plus a new standalone door-OFF test proving the typed refusal AND the read-guard-never-consults-it mutant); `cargo test -p server --test graphql_acl -- the_roster_reads_admit_exactly_admin` 1/1; `cargo test --manifest-path tools/codegen-rs/Cargo.toml` 425/425 including 8 new tests for the two codegen rules. Two mutants planted verbatim, captured red, reverted, reconfirmed green: M7 (`{{ rider.riderld }}` in the sheet → `screen-sheet-binding-unknown` fired exactly once, correct location) and M8 (`RUN_RIDER_RESTRICTION_DOOR` production `"true"` with the row open → `decision-row-open-key-must-be-off` fired, verbatim RUN_SIRENE_WORKER-lesson message). The other six named mutants (M1–M6) are pinned by the written tests' own assertions (each test's positive assertion is the direct negation of that mutant's effect) but were not additionally plant-and-reverted live, given the session's time budget — flagged in the hand-back rather than silently skipped. `EXPLAIN (ANALYZE, BUFFERS)` on a throwaway seeded DB (300 `rider_roster` rows, 10 held jobs): the list query (`ORDER BY display_name, rider_id`) plans a Seq Scan + Sort at this cardinality (0.25ms) — the planner correctly judges the composite index not worth it below a few hundred rows; `held_by_riders` over `View_DeliveryJob` confirms the ADR's own prediction exactly — a per-`DeliveryRequested`-row correlated-subquery fold, not index-only (6.8ms at this scale), the accepted cost with #883 (`View_DeliveryJob` → table) as its recorded owner.
>
> `git diff --stat` vs `origin/main`: 98 files changed, 2706 insertions(+), 111 deletions(-).

> **2026-09-04 — Step 4-iii (the admin's hands) decided by the team in two slices; the release gate
> becomes a mechanism; the system surface is found unreachable.** [ADR-20260904-152807](../adr/ADR-20260904-152807-the-admin-s-hands-one-custody-truth-read-at-query-time-a-door-that-refuses-until-the-notice-exists-and-two-slices.md), full
> mob (13 lenses). Unanimous on a new `RiderRoster` table (own group, never `auth_ref`; extending
> `RiderRestriction` would reverse its landed rule). The one split — fold the held job into the roster
> (dba, business, graphql: a view over the log serves no index, so per-row reads and a 30-second sweep
> are the wrong cost at peak) versus read it at query time (vernon, young, architect, holub, farley:
> two folds of the custody lifecycle under two checkpoints drift, the 4-ii divergence) — took the safer
> option on the legal surface: one custody truth, the detail reads `held_by_rider`, the list one
> set-based join per page, the gauge driven from the restricted set; #883's table conversion is the
> cost's owner, the `EXPLAIN` rides the PR. Two slices: A the roster, the queries (ordered by the
> contract — held first, then RESTRICTED), the triage list, the detail with the four facts and a
> `phone_call` (the register is silent on ops calling a rider), the sheet (fact-named chips, no
> preselection, no free text, no SMS claim before #874), the write-door key
> `RUN_RIDER_RESTRICTION_DOOR` with the open row
> [RIDER-RESTRICTION-PRECONDITIONS](../decisions/RIDER-RESTRICTION-PRECONDITIONS.yaml) and a codegen
> test that refuses a production value other than false while the row is open (the `RUN_SIRENE_WORKER`
> lesson); B the dead-man on the 3-ii tick anchored on `effective_at`, its threshold `UNVERIFIED
> input`, the measure `heldJobAtDecision`. **Found at the briefing (beck)**: `system.captain.food`
> renders a static line, `Surface` has no `System` variant and no admin sign-in door exists — the
> mailbox supervision screen has never been reachable from a browser; slice A ships dark and "an ADMIN
> can reach `/system/riders`" joins the preconditions row, owed by step 6's magic-link door. Card
> defects banked: the worker path, the nav-depth gap listed as open, option (b) presented as live,
> `PageOffset`'s scope, reachability assumed. Counsel packet gains Q6–Q8. Operational: a docs push
> went red on main for five minutes because a `&&` chain gated on `grep -c`, which exits 0 when it
> finds errors — gate a push on `! grep -q '\[error\]'`, never on a count (gates.md §19c).

> **2026-09-04 — #882 merged: step 4-ii of part C is complete; the restricted rider is told.**
> [PR #882](https://github.com/TheCaptainCompany/captain-food/pull/882) (`55ff1111`, squash), two
> rounds on the lower tier. **Presentation pass on `ed4ca073`**: reviewer PASS, legal PASS, evans PASS,
> dba PASS, graphql PASS; beck STOP, ux STOP, observability STOP — three blocking clusters: (1) the
> held-job card rendered BOTH its facts empty in production — the web emitter selected nested
> navigation edges one level deep (a pre-existing gap, the same rows blank on `job_detail` today) and
> an `Address` object was bound as text, while the render test passed on a fixture shape the API never
> emits; (2) a reinstated rider reloading `/restricted` read "Votre accès est restreint." (a record
> gap: ADR-20260904-124600 §4 considered only the attribution-lag transient — amended on main
> `06e2b103`); (3) the RESERVED client-leg declaration the ADR named was missing from the contract.
> **Round 2**: the first executor was lost to a container restart mid-round with an uncommitted
> partial diff; a fresh executor inherited it, kept item 1 (already correct and tested), built the
> rest; re-check PASS ×4 (reviewer, beck, ux, observability), CI green, merged directly (all checks
> already passed). **Attribution**: the empty card = executor depth miss (the fixture) on a
> **roster-width** miss (no lens named the emitter's one-level nav walk — it goes on the founder's
> trip-row evidence); the reinstated case = record gap; the RESERVED comment = executor miss against an
> explicit ADR line; `myStanding.*` paths on the briefing card = card defect, caught at the briefing.
> **Lower-tier tally: first-round PASS 1 of 5** (#864 PASS, #867, #870, #875, #882 all FAIL→PASS;
> rounds 1/2/2/3/2). Issues: [#883](https://github.com/TheCaptainCompany/captain-food/issues/883)
> (nine 4-ii follow-ups incl. two validator gates), [#884](https://github.com/TheCaptainCompany/captain-food/issues/884)
> (the flex row collapsing a space in the legal sentence — first item of the next rider-screen
> slice). Next: 4-iii, the admin's hands, briefed to the roster first (a legal surface: the admin's
> act is the Art. 11(5) human decision).

> **2026-09-04 — #639 part C step 4-ii ROUND 2 (PR #882, still draft): held-job facts fixed,
> reinstated-rider false notice fixed, six one-liners, three addendum items.** Base
> `ed4ca073` (matched). Picked up a partial uncommitted diff from a session lost to a container
> restart mid-round; kept its item 1 work (both halves), redid/extended everything else.
> **Item 1 (kept, extended slightly)**: `collect_screen_nav_selections` (`tools/codegen-rs/src/
> emit/web.rs`) now splices a nav edge nested INSIDE a declared property (`heldDelivery.restaurant.
> displayName` — `heldDelivery` is itself a declared `RiderStandingInfo` property, so the old
> first-segment-only walk stopped there); a new `format_address` render filter (beside
> `format_currency`/`format_datetime`) replaces every unfiltered `Address`-object binding on
> `job_detail` and `restricted` that used to fall through to the Money-shaped formatter and render
> "". Codegen test + render-test fixtures updated to the REAL `Address` shape. **Item 2 (new)**: the
> WHOLE notice body, including the door's own `back_button_header`, is now one `conditional_section`
> keyed on `standing.standing == 'RESTRICTED'` — the `if_false` branch renders
> `rider.restricted.reinstated` ("Votre accès est rétabli.", ADR-081527 §7 verbatim) with one
> `navigate: "/"` control. First attempt only gated the inner `page_header`/text/footer and left the
> `back_button_header`'s title unconditional, which still said "Votre accès est restreint." for a
> reinstated rider on the FIRST render test run — caught by the test's own positive assertion
> (`!html.contains("restreint")`), fixed by moving the header inside both branches too. **Item 3
> (new)**: `specs/observability.yaml#/rider-restriction` gains a comment declaring the client bounce
> leg RESERVED (`rider.restricted.bounced`, the `sdui_degraded_render_total` convention, no OTel in
> `web`) — comment only, `make validate`/`check-drift` unaffected. **One-liners**: both date LABELS
> asserted (not just values, item 4); a native fixture for the second sheet's ASSIGNED arm asserting
> the literal `NOT_COLLECTED` reaches `data-vars` (item 5); the `d-1` assertion sharpened to the
> `data-vars` `deliveryJobId` payload (item 6) — both required matching the RENDERED (HTML-escaped,
> `&quot;`) form, not the raw JSON string, caught by a first red on exactly that; a `[data-c="row"]`
> flex CSS rule so the split legal sentence (three `<p data-c="text">` children) flows as one line
> instead of stacking (item 7 — `row` already implied this for the chat compose bar, so this is a
> general fix, not a special case); `specs/delivery/api.yaml` drops a stale line-number citation
> (item 8); `make wasm` GREEN, 57s (item 9). **Addendum**: clippy widened to include
> `captain-food-codegen` (item 11); a `router.rs` twin assertion that `restricted` itself declares
> `unauthenticated: /sign-in` (item 12); `rider_standing_walk.rs` threads
> `Some(EmailAddress("support@captain.food"))` through `ReadDeps` and asserts
> `myStanding.contestContact` reaches the wire — every other fixture in the corpus passes `None`
> (item 13). **Gates**: `cargo build --workspace` GREEN; `make validate` 0 errors; `make rust` GREEN
> 95s (build+test(codegen 417)+validate+generate+link-check); DB-gated `make test-crates` GREEN
> against an isolated `cf639r2ii` database (DB PRE-FLIGHT OK, empty skip receipt, 0 FAILED across
> ~209 `test result: ok` blocks, incl. `the_restriction_walk_forbids_and_reopens_the_real_doors`
> carrying the new `contestContact` assertion); `cargo clippy -p web -p server -p infrastructure -p
> shared_types -p application -p captain-food-codegen --all-targets -D clippy::disallowed-methods -D
> clippy::mistyped_literal_suffixes` exit 0; codegen test binary 417 passed; `make link-check` 8589
> links, 0 broken; `make warning-baseline` run, diff clean (surface did not move). PR stays DRAFT,
> `HOLD: human` unchanged. Full detail: hand-back at
> `/tmp/.../handback-4ii-r2.md` per the dispatch.

> **2026-09-04 — #639 part C step 4-ii landed (PR #882, draft, hand-back at green): the restricted
> rider is told.** [ADR-20260904-124600](../adr/ADR-20260904-124600-the-restricted-rider-is-told-on-the-client-leg-first-keyed-on-the-server-s-own-reason-and-the-page-get-leg-rides-with-the-socket.md).
> Executor tier: sonnet. Base `f00ceeec` (the empty claim commit) on `ec374f2e` == `origin/main` —
> checked first, matched. **A.** `shared_types::RIDER_RESTRICTED` (the one crate both `server` and
> `web` name directly); `StandingGuard` adds `extensions.reason` beside the unchanged `code:
> FORBIDDEN`; `RoleGuard` adds nothing (asserted both ways, `graphql_acl.rs` +
> `rider_restricted_is_refused_on_the_write_half.rs`). `crates/web/src/graphql.rs` parses
> `extensions` into a typed `ErrorExtensions { code, reason }` once, at the transport boundary.
> **B.** `restricted:`/`while_restricted:` screen keys (the `unauthenticated:` twin) +
> `screen-restricted-route-unknown` / `screen-restricted-binds-uncarved-op` (the latter's "never
> mounts `rider_topbar`" clause falls out of the SAME uncarved-op check — no second mechanism, since
> the topbar's only action, `changeRiderStatus`, is never `whileRestricted:`). ONE pure
> `bounce_after` (new `crates/web/src/bounce.rs`) decides the bounce for BOTH the hydrate loop's
> refused reads and `interact.rs`'s refused Tells; the pre-existing 2c-ii 401 leg moved in and was
> seen red for the first time through this function's own tests. **C.** The `/restricted` screen:
> `standing.mine -> myStanding`, the notice (title/no-more-jobs UNCONDITIONAL; ground + both dates
> gated on `standing.restriction != null`, the transient the OTHER branch of the SAME
> `conditional_section` — no `&&` needed); the five ground leaves + the footer, each split at the
> address into `.lead`/`{{ standing.contestContact }}`/`.trail` (the `contestContact` field is
> additive on `RiderStandingInfo`, resolved once from `SUPPORT_CONTACT`, the 2c refusal-screen
> precedent); `format_datetime` (Europe/Paris, `fr`, beside `format_currency`, `chrono`+`chrono-tz`
> newly in `web`'s `Cargo.toml`); the held-job card + the second sheet
> `rider_restricted_handback_sheet` bound to `standing.heldDelivery.*`; `held_by_rider` (#879's
> item) replaces `for_rider(..).find(..)` in the `myStanding` resolver. **D.** Native render tests
> (ground loop × both dates × contact × no raw ISO × no `rider_toggle_online`; the transient; the
> held-job card + sheet dispatch), the bounce tests, the router twin, the DB-gated walk extended
> (both dates non-null, `heldDelivery` via the new port, a SECOND restriction cycle proving
> `handBackDelivery` reaches the business layer while RESTRICTED and `myStanding.heldDelivery`
> narrows to null after). Seven mutants planted, red captured verbatim, reverted — see the hand-back
> (`/tmp/.../handback-4ii.md` per the dispatch, and the PR body). **Card defects banked** (not
> roster-width): the card's example M7 wording ("data-variables") names the wrong attribute
> (`data-vars`); no `an_unknown_screen_key_is_refused`-shaped test exists for screens today (no
> closed key-set schema on the screens loader — banked, not built). `HOLD: human` (a legal surface,
> Tours-facing copy) — PR stays DRAFT; the coordinator reviews and merges.

> **2026-09-04 — Step 4-ii (the restricted rider is told) decided by the team; the lower-tier trip
> row is queued to the founder.** [ADR-20260904-124600](../adr/ADR-20260904-124600-the-restricted-rider-is-told-on-the-client-leg-first-keyed-on-the-server-s-own-reason-and-the-page-get-leg-rides-with-the-socket.md), full mob (13 lenses, a legal surface).
> The register had already chosen both legs (ADR-20260904-081527 §11); the roster corrected the
> card: no machine-readable `RESTRICTED` signal exists (`StandingGuard` and `RoleGuard` both emit
> `code: FORBIDDEN`, the web transport stringifies the errors), and the card's `myStanding.heldDelivery`
> paths were unspellable (the alias root is `standing.*`) — **a card defect banked**: a lower-tier
> executor would have copied it and the held-job card would have failed closed. Decided: the client leg
> now, keyed on an additive `extensions.reason: RIDER_RESTRICTED` shared as one constant between
> `server` and `web`, firing on refused reads AND refused Tells through one pure function seen red (the
> 2c-ii 401 leg joins it); the document-GET leg rides with step 5's socket as one resolver with three
> callers, its outage posture recorded (`LookupFailed` renders the shell, never a 302 to a false legal
> statement); a second sheet bound to `standing.heldDelivery.*`; `$reload` after the Tell; the after-state
> from `foodLocation`; a `format_datetime` renderer filter (Europe/Paris); the transient reads *"Détails
> de la restriction pas encore disponibles."*; the address bound once from `SUPPORT_CONTACT`; no copy
> button; the five ground labels bound explicitly; `held_by_rider` (#879's item) lands in the slice. The
> one split (build the GET leg now — architect) took the safer option on a legal surface. **Trip**:
> ADR-20260904-013450 §5's own wording tripped on #875 (`HOLD: human`, lower tier, three rounds) —
> [LOWER-TIER-TRIP](../decisions/LOWER-TIER-TRIP.yaml) queued to the founder with options and a
> recommendation (keep the tier, make the two failure shapes structural); the ruling stands meanwhile
> and 4-ii dispatches on the lower tier. Concurrency fence: #868 (`rider_topbar`) not alongside 4-ii.

> **2026-09-04 — #875 merged: step 4-i of part C is complete — a restriction bites on the next
> request, and the doors are human-only.** [PR #875](https://github.com/TheCaptainCompany/captain-food/pull/875)
> (`690430cc`, squash), a lower-tier run of three rounds — the ceiling (ADR-20260826-084500).
> **Round 1** (reviewer + eight declared lenses): nine BLOCKING items, all small — the §6(iii)
> codegen test claimed by the rules text but not written; `myStanding.restriction` rendered after
> reinstatement; one word (*revocation*); the real Postgres resolver never driven against a RESTRICTED
> row; the ADR §3 handler belt documented but absent; a declared span nothing constructed and a false
> "0 when caught up" claim; **`RiderRestriction` born without replay** (folded under the already-advanced
> `Rider` checkpoint, so a pre-migration rider's restriction was silently dropped — a card defect: the
> card said "own checkpoint" for 4-iii's roster table and not for this one); the ADR §2 mechanism
> sentence (the executor was right, the record was amended); the journal citing a note that was not
> there. **Round 2**: seven PASS, the reviewer STOP on one new item — `noTestFixturePossible: true` as a
> bare per-item boolean exempting an ERROR-level ADR-0032 gate; five lenses had it non-blocking with the
> same tightening, the coordinator took the reviewer's reading (an underived exemption of a blocking
> gate is a gate weakening inside the PR). **Round 3**: the flag is DERIVED by `error-exemption-unjustified`,
> ALL-quantified — the executor's first ANY cut came back green under its own red-first test because
> `RiderNotFound` is co-thrown by ordinary commands, and it tightened the rule in the direction of its
> intent; reviewer PASS; ready + auto-merge as one step. Two executor deviations accepted by every
> lens: a REAL `rider.standing.denied` span (the hard `obs-no-spans` gate honoured, not routed around)
> and the derived exemption. **Lower-tier tally: first-round PASS 1 of 4** (#864 PASS, #867 FAIL→PASS,
> #870 FAIL→PASS, #875 FAIL→FAIL→PASS; rounds 1/2/2/3). **Attribution**: the missing §6(iii) test and
> the handler belt are executor depth misses against explicit ADR lines; the checkpoint group is a card
> defect; the lag-gauge claim is an invited-lens depth miss (observability and dba both own it); the
> `drain_group` early return is the one finding no lens named — banked as roster width and now an
> issue. Filed: [#876](https://github.com/TheCaptainCompany/captain-food/issues/876) lag gauges never
> read 0 (architect-owned), [#877](https://github.com/TheCaptainCompany/captain-food/issues/877) the
> bam fold's `riderId` grain, [#878](https://github.com/TheCaptainCompany/captain-food/issues/878)
> handler-side actor check before #358, [#879](https://github.com/TheCaptainCompany/captain-food/issues/879)
> five small follow-ups, [#880](https://github.com/TheCaptainCompany/captain-food/issues/880) ~35 span
> declarations with no constructor (UNVERIFIED count until the test is committed),
> [#881](https://github.com/TheCaptainCompany/captain-food/issues/881) the denial's classification and
> `causedBy:`. What the three rounds cost: one card defect and two executor depth misses became eleven
> fixes and a new validator rule; what they bought: the seam's real read is tested, a from-zero rebuild
> re-grants nobody AND backfills the attribution, and a gate exemption is now derivable. Next: 4-ii (the
> restricted rider is told), briefed to the full roster — a legal surface.

> **2026-09-04 — #639 part C step 4-i, PR #875: rider standing lands (still draft, HOLD: human).**
> Base `3498fa04` (the empty claim commit) on `origin/main` `d4e02d26` (the commit introducing
> [ADR-20260904-081527](../adr/ADR-20260904-081527-rider-standing-is-a-grant-on-the-identity-row-the-doors-are-human-only-and-step-4-lands-in-three-slices.md)).
> Executor tier: sonnet (ADR-20260904-013450). `git diff --stat` against `origin/main`: 107 files
> changed, 4353 insertions(+), 369 deletions(-) (final count, after the walk test and the two
> observability tests below landed).
>
> **Landed**: `RiderStanding`/`RiderRestrictionGround` (+ `readOnlyCatchAll: UNRECOGNISED`, a NEW
> scalar attribute — [docs/claude/dsl.md](../claude/dsl.md) "readOnlyCatchAll", `#[serde(other)]` +
> a tolerant `EnumText` + the SDL/mirror-enum exclusion via a plain `_from_domain` fn, not a `From`
> — `impl From<Foreign> for Option<Foreign>` violates the orphan rule) / `RiderAvailabilityTarget`;
> `RestrictRider`/`ReinstateRider` (`requires: acting: { ADMIN: any }`, no EXTERNAL); `RiderRestricted`
> `{ ground, decidedAt, effectiveAt }` / `RiderReinstated`; three new errors; the Rider lifecycle's
> four `-> SUSPENDED` entry edges retired via a NEW `legacyStates:` lifecycle key (exempts the
> `SUSPENDED -> OFFLINE` legacy exit from the reachability gate — `validate/lifecycles.rs`); five new
> rules; `restrictRider`/`reinstateRider` [ADMIN] + `myStanding` [RIDER] (hand-written resolver — the
> compound `standing`/`restriction`/`heldDelivery` shape has no mechanical fold); the NEW
> `whileRestricted:` grammar ([docs/claude/dsl.md](../claude/dsl.md) "whileRestricted" — 3 validator
> rules, `@whileRestricted` SDL directive, `StandingGuard.and()`-chained onto every role-guarded op
> with an empty carve set by default) carving `{ myStanding, delivery, reportDeliveryIssue,
> handBackDelivery }` — never `myDeliveries` (the PENDING-pool exposure); `pm-sends-human-only-command`
> (no saga may send a command whose `requires: acting` has no EXTERNAL key); `Rider.standing` column
> (a Complex/hand-written fold — the creating arm's own compute hook returns the PRIOR row's value
> when one exists, never a literal, so a replay-in-place never re-grants) + new table
> `RiderRestriction`; `ReadScope::Rider` becomes the struct variant `{ id, standing }`
> (compiler-first — the 7 generated derived-injection sites + ~15 hand-written match sites all moved
> in the same change, none missed: `cargo build --workspace --tests` is the proof, not a grep).
> Migration `20260904110000_rider_standing.sql` (metadata-only ALTER + a new table, no checkpoint
> rewind). NEW `crates/server/tests/rider_standing_walk.rs` (farley's DB-gated walk, section D):
> the full migration chain applied dynamically (own `apply_all_migrations`, read from
> `migrations/*.sql` at test runtime — `View_DeliveryJob` is a SQL view over `domain_events`, so
> only the real chain has it), a registered restaurant through the real router, a rider/order/job
> birth seeded by raw `domain_events` insert (`specs/tests.yaml`'s `orderPlaced`/`deliveryRequested`
> fixtures verbatim, re-keyed), then `acceptDelivery` (ACTIVE, admitted) → `restrictRider` (ADMIN) →
> `acceptDelivery` (RESTRICTED, synchronous FORBIDDEN, never enqueued) + `delivery(orderId)` (stays
> reachable) + `myStanding` (RESTRICTED + the held job) → `reinstateRider` → `acceptDelivery`
> (ACTIVE again, PENDING — the guard reopened — terminal `DeliveryAlreadyAssigned`, the business
> layer's own, unrelated call). Two NEW observability tests (A.9 / D "Observability"):
> `crates/server/tests/rider_restriction_denied_metric.rs` (`rider_restricted_denied_total{operation}`
> fires exactly once, on the refused op, carries no `rider_id` label) and
> `crates/infrastructure/tests/rider_standing_lag_metric.rs` (`rider_standing_lag_positions` reads 0
> once a real drain has caught up — see the "Left out" note below for the half this could NOT prove).
> `docs/SPEC-LOG.md` row, `docs/claude/dsl.md` two new sections, this entry, all in the same change.
>
> **Gates (VERDICT lines, wall-clock)**: `rm -rf target/debug/incremental` first (disk pressure hit
> repeatedly across the run, low-single-digit-GB→9G+ after cleanup each time — see the
> operational-learnings paragraph below) · `cargo build --workspace` — **0 errors, 88s** (final re-run,
> after the walk/metric tests) · `make validate` — **0 errors** (`grep -cE '\[error\]'` = 0),
> warning surface unchanged, **2s** · `make rust` (build + test + validate + generate + link-check)
> — **0 errors, exit 0** (final re-run) · `cargo build --workspace --tests` — **0 errors** (confirms
> every test binary, not just lib code) · `cargo test --manifest-path tools/codegen-rs/Cargo.toml
> --bin generate` — **409 passed, 0 failed** (new: `while_restricted_gate` module, 6 tests) ·
> `cargo clippy -p server -p application -p infrastructure -p domain -p telemetry --all-targets --
> -D clippy::disallowed-methods -D clippy::mistyped_literal_suffixes` — **exit 0** (warnings present,
> neither denied lint fired; 7m02s on the final from-clean run after `rm -rf target` cleared a
> disk-pressure incident, see the operational-learnings paragraph below) · DB-gated (`DATABASE_URL` to a live
> local Postgres, `DB_TESTS_REQUIRED=1`): `make test-crates` full workspace suite, isolated database
> (`cf639`, the shared-Postgres collision below) — **1649 passed, 0 failed, as of `72ed93ab`**
> (round 2 landed 3 more atop this figure, round 3 none — see PR #875's later commits for the
> current count) (the 4 new tests below
> included) · `rider_projection.rs` — **7 passed**
> (5 new: the standing fold, the legacy-SUSPENDED non-restriction, the replay-in-place mutant, the
> unknown-ground tolerant decode, the pre-migration DEFAULT backfill); `rider_restricted_is_refused_on_the_write_half.rs`
> (NEW) — **2 passed**; `rider_id_derived_at_the_door.rs` — **5 passed** (1 new: the SUSPENDED
> unspellable-enum test); `graphql_acl.rs` — **12 passed** (1 new: `the_standing_doors_admit_exactly_admin`);
> `rider_without_a_row_is_forbidden_on_the_write_half.rs` — **3 passed** (1 assertion line added);
> `rider_standing_walk.rs` (NEW) — **1 passed, 5.6s**; `rider_restriction_denied_metric.rs` (NEW,
> own binary — `telemetry::meters` binds the process-wide OTel meter once via `OnceLock`) — **1
> passed**; `rider_standing_lag_metric.rs` (NEW, own binary, same reason) — **2 passed** (the
> embedded-migration-manifest check rides along by `#[path]`).
>
> **Operational learnings (round 2 item 9 — farley, reviewer, beck: pasted here from the
> hand-back rather than left as a dangling forward-reference)**: (1) disk exhaustion, twice,
> from the SAME cause — `df -h /` reports a nominal 252G filesystem, but the actually-usable
> capacity this session ever observed topped out around 37-38G (`du -sh /*` sums to the same
> ~28G `df` reports as used, so there is no hidden/leaked space — the filesystem is just much
> smaller than its `Size` column claims); `target/debug` alone reached 18G mid-session from the
> cumulative weight of many builds across one long session, and periodic
> `rm -rf target/debug/incremental` did not keep pace — a full `rm -rf target/` (18G freed
> instantly) is the reliable fix and should be reached for earlier (~5G avail), not after a
> command has already died from ENOSPC losing its output. (2) The scratchpad task-output
> directory shares the SAME filesystem as the repo, so a `run_in_background` command can lose
> its own stdout to ENOSPC when disk is critical — a confusing failure mode distinct from the
> command's own logic failing. (3) `ReadScope`-injecting test harnesses
> (`graphql_write_path.rs`'s `acting()` idiom) mint an `ActingRole` but never a full
> `crate::auth::Principal` — `operationStatus`'s ownership check
> (`mailbox_operation_owned`) reads `ctx.data_opt::<Principal>()`, NOT `ActingRole`, so polling
> `operationStatus` as a non-ADMIN role under this harness spins to its timeout with no
> diagnostic even though the row is `SUCCEEDED` in the DB the whole time (confirmed via direct
> `psql`); poll as ADMIN regardless of which role enqueued the operation (ADMIN sees every
> operation unconditionally, and the guard decision under test already happened at enqueue
> time). (4) `OfferId`/`ProductId` are real UUID scalars in production, but `specs/tests.yaml`'s
> canonical fixtures (`orderPlaced`, etc.) use human-readable placeholder strings (`"off-1"`)
> that decode fine through the TYPED behaviour-test harness but fail a real Postgres
> JSONB→struct decode (`UUID parsing failed: invalid character: found 'o' at 0`) — substitute
> real UUIDs for any UUID-scalar field before reusing a `tests.yaml` fixture verbatim in a
> raw-event-append DB-gated test. Landed in
> [gates.md](../claude/sessions/gates.md) (item (3) and (4) above) in the same change.
>
> **Eight mutants, verbatim reds, all reverted (the codebase is currently GREEN)**:
> - **M1** (seam maps RESTRICTED like ACTIVE): `auth.rs::resolve_rider_scope`'s `ReadScope::Rider`
>   construction hardcoded to `RiderStanding::ACTIVE`. `rider_restricted_is_refused_on_the_write_half.rs`
>   both tests FAILED: `assertion left == right failed: a RESTRICTED rider must be refused by the
>   standing guard — got: Data 'alloc::sync::Arc<dyn actor_client::mailbox::Mailbox>' does not
>   exist. left: None right: Some("FORBIDDEN")`.
> - **M2** (`StandingGuard` allow-all): `check()` short-circuited `return Ok(())`. Same two tests,
>   identical failure text (the guard chain masks identically from the outside — correct: the two
>   mutants are behaviourally indistinguishable from the caller's seat, which is the point of
>   defense-in-depth).
> - **M3** (`ChangeRiderStatusInput.status` still `RiderStatus`): reverted `commands.yaml`'s
>   `ChangeRiderStatus.status` `$ref` to `RiderStatus`, regenerated (0 codegen errors — the spec
>   itself is legal), then `cargo build -p application` — **4 compile errors**:
>   `error[E0308]: mismatched types` at `commands.rs:1841` (`cmd.status == RiderAvailabilityTarget::AVAILABLE`,
>   expected `RiderStatus`) and three more on the `match cmd.status { RiderAvailabilityTarget::… }`
>   arms — `expected RiderStatus, found RiderAvailabilityTarget`. check-drift-shaped: the hand-written
>   handler and the generated command type disagree the instant the spec reverts.
> - **M4** (fold keys on `status == SUSPENDED`): added a `RiderStatusChanged(e) if e.status ==
>   SUSPENDED => RESTRICTED` arm to `RiderCompute::standing`. The DB-gated
>   `a_legacy_suspended_status_does_not_restrict_and_the_fact_does` test stayed GREEN — a genuine
>   finding, not a gap: `standing`'s `from:` lineage names only `RiderRestricted`/`RiderReinstated`,
>   so the GENERATED dispatch never calls the hook on `RiderStatusChanged` at all (the classifier
>   gates the call on lineage membership) — the mutant is structurally unreachable through the
>   declared fold, by construction. Verified instead with a direct hook-level probe (temporary,
>   not committed) calling `RiderProjector.standing` with a `RiderStatusChanged{SUSPENDED}` envelope
>   directly: `assertion left == right failed: … left: RESTRICTED right: ACTIVE`.
>   Round 2 item 9 (farley, reviewer, beck): M4 as planted was an EQUIVALENT mutant to "no arm at
>   all" — since the hook is never called on `RiderStatusChanged` by construction, adding a branch
>   to it is inert — and `a_legacy_suspended_status_does_not_restrict_and_the_fact_does` (test 5,
>   `rider_projection.rs`) is the test that actually guards the `standing` column's `from:` lineage
>   (only `RiderRestricted`/`RiderReinstated`) staying closed to `RiderStatusChanged`, DB-gated
>   through the real generated dispatch rather than a direct hook call.
> - **M5** (the creating arm writes standing): `_ => RiderStanding::ACTIVE` unconditionally
>   (dropping the `prev`-preserving fallback). The DB-gated replay test (`run_once()` drains a
>   whole stream to exhaustion per call, so a LATER `RiderRestricted` on the same stream self-heals
>   within the SAME call and masks this exact mutant — a second genuine finding, test rewritten with
>   `.with_batch_size(1)` and an assertion BETWEEN two separate `run_once()` calls, which ALSO could
>   not observe it, since `run_once()`'s inner loop drains to exhaustion regardless of batch size)
>   stayed green; caught instead by a new committed unit test on the Compute hook directly:
>   `assertion left == right failed: a replayed creation must preserve the prior standing
>   left: ACTIVE right: RESTRICTED`.
> - **M6** (`#[serde(other)]` dropped): removed `readOnlyCatchAll: UNRECOGNISED` from
>   `scalars.yaml#/RiderRestrictionGround`, regenerated (0 codegen errors), `cargo build -p
>   infrastructure` — **2 compile errors**: `error[E0599]: no variant … named 'UNRECOGNISED' found
>   for enum 'RiderRestrictionGround'` at both arms of the hand-written `EnumText` impl
>   (`enum_sql.rs:103`, `:112`) — compile-red, earlier and stronger than the card's anticipated
>   runtime "unknown variant" message, because the catch-all variant disappears from the TYPE, not
>   just from decode.
> - **M7** (a `sends: RestrictRider` planted in a processmanager.yaml): the codegen test
>   `while_restricted_gate::a_pm_send_of_a_human_only_command_is_refused` plants it on
>   `RefundProcess`'s first `receives` leg (a scratch model mutation, never the real fenced file) and
>   asserts `pm-sends-human-only-command` fires: `["processmanager.yaml/RefundProcess.receives[0].sends[0]:
>   process manager 'RefundProcess' sends 'RestrictRider', whose actors.yaml \`requires: acting\`
>   carries no EXTERNAL key -- a human-only door (#639 part C step 4-i, ADR-20260904-081527 §6/§8).
>   No saga may impersonate the human this door requires; route the decision to an admin surface
>   instead."]`.
> - **M8** (`restrictRider roles: [RIDER, ADMIN]`): edited `api.yaml`, regenerated (0 codegen
>   errors), `graphql_acl.rs::the_standing_doors_admit_exactly_admin` FAILED:
>   `restrictRider must be FORBIDDEN for Rider: ServerError { message: "Data
>   'alloc::sync::Arc<dyn actor_client::mailbox::Mailbox>' does not exist.", … }` — the guard PASSED
>   for RIDER (no FORBIDDEN), which is exactly the widened-access defect the test exists to catch.
>
> **Left out, with attribution**: 4-ii's SMS notice, 4-iii's admin roster surface (both explicitly
> NOT this slice per the ADR's §11); `TestRiderStatusChangeTargetCannotSpellSuspended` (named on the
> dispatch card for `specs/tests.yaml`) — NOT added there: `status: "SUSPENDED"` is not a legal
> value of `RiderAvailabilityTarget`, so a `tests.yaml` fixture spelling it cannot even validate
> (the point of the test IS that it is unspellable) — the equivalent coverage instead lives in
> `rider_id_derived_at_the_door.rs::a_suspended_status_is_unspellable_on_change_rider_status_inline_and_via_variables`
> (attribution: card defect — the named test cannot exist in the form the card names it, the correct
> surface is the server-side static-validation test, already written). **The lag gauge's "> 0 while
> behind" half** (card D "Observability") — a genuine, load-bearing gap, found live writing
> `rider_standing_lag_metric.rs`: `rider_standing_lag_positions` is an OTel `Gauge` (last-value-wins
> per collection interval, not a replayable series), and `drain_group`'s loop returns the moment one
> page comes back SHORT of `batch_size` — WITHOUT one more, empty scan — so the "0 when caught up"
> contract note is only true when the backlog is an exact multiple of the batch size (this slice's
> test forces that on purpose, `.with_batch_size(2)` over 2 seeded facts; the module doc comment
> spells out why). This is `drain_group`'s own, PRE-EXISTING behaviour — shared verbatim by
> `scope_membership_lag_positions` and `read_authorization::lag_positions`, this slice's mirror, not
> its origin — so it is flagged for the architect rather than "fixed" here: touching the shared loop
> would move every OTHER group's lag reading too, well past this card's licensed diff. (Attribution:
> roster width — no lens at the mob briefing flagged the shared drain loop's early-return shape;
> catching it took writing the test against the real code, not a design review.)
>
> Next: the coordinator's independent-reviewer pass (`HOLD: human` — this posture is NOT the
> founder; the TEAM reviews before merge, ADR-20260815-134655), then 4-ii (the restricted rider is
> told) before any production `RestrictRider`, per the ADR's own deploy-order clause.

> **2026-09-04 — Step 4 (rider restriction) decided by the team: standing is a GRANT on the identity
> row, the doors are human-only, three slices in one train.**
> [ADR-20260904-081527](../adr/ADR-20260904-081527-rider-standing-is-a-grant-on-the-identity-row-the-doors-are-human-only-and-step-4-lands-in-three-slices.md),
> full mob (13 lenses, `HOLD: human`). Two genuine splits resolved by the option that keeps the seam a
> pure, clock-free, replay-neutral fold: (a) a future `effectiveAt` — six lenses refuse a clock term in
> the grant predicate, business wants it for a lapsed document, legal permits it per ground — V0 stamps
> `decidedAt == effectiveAt` server-side and the scheduled form is DESIGNED (a due-row worker appending
> the fact, permitted for LAPSED/REQUESTED, refused forever for the protective two); (b) whether the
> `RiderRegistered` projector arm may write `standing` — dba's construction wins (the creating arm never
> writes it, so a checkpoint-reset rebuild re-grants nobody), young's bounded-window alternative
> refused. Also: `ReadScope::Rider { id, standing }` struct variant (compiler-first over the smaller
> witness), the set `{ myStanding, delivery, reportDeliveryIssue, handBackDelivery }` (`myDeliveries`
> refused — it returns the PENDING pool; `myStanding` added, amending ADR-20260904-015903 §6), the
> read-only catch-all makes the revoke UNSKIPPABLE by the projector (verified: a fold fault skips and
> advances the checkpoint, a stale grant), `RevocationGround` renamed `RiderRestrictionGround` before
> anything is stored, one word *restriction* (never *suspendu*, never *réintégrer*), the four `fr`
> strings as counsel-reviewable copy, ADR-015903 §10's "exactly one arm" amended to *one additive arm
> per new `receives:` entry* with the fence globs named in one place, `RestrictRider` human-only in
> three layers (a PM `sends` of it validates clean today — the mutant). **Card defects banked**: the
> two-column `Rider` claim and "email/SMS" (card); three lenses reporting the pre-squash tip
> `df451998` as `main` (invited-lens depth miss); none roster width. PROP §6.3, §6.4, §8.6, §11 row 4
> rewritten (4-i / 4-ii / 4-iii). Next: claim 4-i, dispatch on the lower tier.

> **2026-09-04 — #870 merged: step 3 of part C is complete; the rider hands a job back and the
> customer is told.** [PR #870](https://github.com/TheCaptainCompany/captain-food/pull/870)
> (`6cf74887`, squash), a two-hour lower-tier run plus a round 2. **Presentation pass**: reviewer FAIL,
> vernon PASS, young PASS, observability STOP, ux STOP, legal STOP — three blocking clusters, all
> fixed in round 2 and re-checked PASS by the three lenses that stopped: (1) the issue sheet gated both
> exits on a chip's value, and the SDUI never re-evaluates a condition on a form field after paint —
> the sheet rendered NOTHING and took 3-i's report door down with it; now a two-button router opening
> per-exit sheets, 3-i's chips unconditional, rendered and asserted; (2) the customer banner keyed on
> `OrderStatus::OUT_FOR_DELIVERY`, which no projector produces, read a second `delivery` state the push
> path never refreshed, had no test, and promised a re-offer nobody performs — now keyed on
> `Order.deliveryHandedBack` folded onto the order mirror, facts-only copy in both languages, render
> and push tests; (3) the `custody-handback` contract declared a required span attribute nothing
> records and a threshold of 300 "derived from" a key whose value is 900. **Attribution**: the sheet
> is a card defect (the briefing framed the condition grammar as a parse question, not reactivity);
> the banner predicate is an ADR-text defect (§7 named a status no fold produces) plus an
> invited-lens depth miss; neither is roster width. ADR-20260904-015903 §1 and §7 amended in place.
> Three live-found bugs during the run (an event missing `orderId`, a missing hand-written
> `OrderTrackingCompute` arm, a CASE-type bug in the executor's own emitter extension) were caught by
> the card's tests, not by luck (reviewer); what slipped was the one requirement stated in prose
> with no red-first step. **Lower-tier tally: first-round PASS 1 of 3** (#864 PASS, #867 FAIL→PASS,
> #870 FAIL→PASS; rounds 1/2/2). Filed: [#871](https://github.com/TheCaptainCompany/captain-food/issues/871)
> ScopeMembership never revokes on reassignment, [#872](https://github.com/TheCaptainCompany/captain-food/issues/872)
> three SDUI renderer gaps (chaining, form-field reactivity, `disabled_when`),
> [#873](https://github.com/TheCaptainCompany/captain-food/issues/873) four handback follow-ups. Next:
> step 4, rider restriction (ADR-20260904-014136), briefed to the roster first.

> **2026-09-04 — #639 part C step 3-ii, PR #870: review round 2 fixes (still draft, HOLD: human).**
> Presentation on `3103dc42` (round 1's green hand-back): reviewer FAIL, checkpoint STOPs from
> observability, ux and legal; vernon and young PASS on their own concerns. Round 2 of the 3-round
> ceiling. `git diff --stat 3103dc42..HEAD`: 41 files changed, 657 insertions(+), 210 deletions(-)
> (final, three commits: the round-2 fix itself, the docs-only no-op merge of `origin/main` at
> `cb13d171`, and one more `OrderTrackingRow` fixture site the DB gate caught that the earlier
> `--lib` builds did not reach — `crates/server/tests/graphql_subscriptions.rs`).
>
> **BLOCKING, all fixed:**
> 1. **The `Un problème` sheet rendered nothing after a chip pick** — `issue_exit.value` is a FORM
>    FIELD; `RenderContext::lookup` (`renderer.rs:145-152`) reads resolver data only, `visible_when`
>    fails CLOSED (`renderer.rs:704-706`), and `interact.rs` never re-evaluates conditions after a
>    chip pick — so neither exit's content ever appeared (3-i's report door REGRESSED along with
>    3-ii's handback door). Fixed by splitting `rider_issue_sheet` into a ROUTER (two buttons, each
>    `open_bottom_sheet` — a real SDUI edge, not a form-field condition) and two content sheets
>    (`rider_report_sheet`, `rider_handback_sheet`) gated only by `delivery.status` (resolver data,
>    which DOES evaluate). Handback confirm relabelled to `rider.issue.confirm` ("Prévenir le
>    restaurant") — the WITH_RIDER card's rider has just said they did NOT return the food, so
>    "Rendre la commande" was a false promise; the now-orphaned `rider.issue.handback_confirm`
>    translation key removed (`translation-key-unused` would else error). New web test
>    (`renderer.rs::the_issue_router_and_its_two_child_sheets_render_their_confirm_controls`) renders
>    `job_detail` for ASSIGNED and PICKED_UP and asserts both confirm controls present, food cards
>    absent on ASSIGNED (derive, never ask — ADR-20260904-015903 §2).
> 2. **The customer banner could never render and never refreshed.** (a) predicate keyed on
>    `order.status == 'OUT_FOR_DELIVERY'`, a token NO OrderStatus producer emits
>    (`projectors/order_tracking.rs` yields PLACED/ACCEPTED/PREPARING/READY/DELIVERED/REJECTED/
>    CANCELLED_*; `specs/ordering/actors.yaml`'s own lifecycle comment says OUT_FOR_DELIVERY is a
>    "read-side presentation status" nothing actually derives). (b) it read a SEPARATE
>    `TrackingState.delivery` (`delivery.byOrder`) refreshed only by `load()`; the PUSH path
>    (`apply`, the primary transport, ADR-20260810-231300) replaced `order` only and never touched
>    it. (c) no test rendered it. (d) the copy promised a remedy nobody performs ("nous
>    réattribuons la livraison" — #860's re-offer PM step is fenced, nothing runs it — and "votre
>    commande est bien préparée", false on the WITH_RIDER card). Fixed: `OrderTracking` gains
>    `delivery_handed_back` (bool, default false) — set true by `DeliveryHandedBackByRider`, reset
>    false by `DeliveryAcceptedByRider`/`DeliveryAcceptedByPartner` — folded by the hand-written
>    `OrderTrackingCompute::delivery_handed_back` (Complex-classified, same as `delivery_status`/
>    `courier`), exposed additively as `Order.deliveryHandedBack`, which the pushed `Order` frame now
>    carries because it rides the SAME row. Banner predicate is `order.deliveryHandedBack == true` —
>    NO status term, so it correctly fires on the from-ASSIGNED NOT_COLLECTED case too (the order is
>    only READY there). `TrackingState.delivery` and the second resolver call dropped entirely (7
>    `Ok(json!({ "delivery": null }))` fixture entries + 2 `delivery: None` struct literals removed);
>    `restaurant_frontoffice.yaml`'s `data_requirements` drops `delivery.byOrder` (nothing consumes
>    it now). Copy replaced, both languages, facts only: *"La livraison n'arrivera pas à l'heure
>    indiquée. Le restaurant est prévenu. Nous vous tiendrons informé ici."* / EN equivalent. Three
>    new `tracking.rs` tests: flag-true replaces the bar + facts-only copy (PREPARING AND READY, the
>    latter proving no status dependency); flag-false/absent leaves the ETA alone; a PUSHED frame
>    (`apply`, no `load()` call) flips the render. **Migration** `20260904090000_ordertracking_
>    delivery_handed_back.sql` (`ALTER TABLE ... ADD COLUMN ... DEFAULT false` + a checkpoint rewind
>    to backfill any pre-migration handback), chain entry in `common.rs`, `REQUIRED_SCHEMA_VERSION`
>    → `20260904090000`. **RED-first, as asked**: inverted `delivery_reassigning` to a hardcoded
>    `false`, ran `cargo test -p web --lib tracking::tests::the_handback_flag_replaces` —
>    `panicked at crates/web/src/tracking.rs:906:9` (the ETA-bar-absent assertion on the
>    PREPARING+flag-true case) — reverted (`sed` round-trip, `git diff` on `tracking.rs` stayed at
>    the same +/- count as before the probe), reconfirmed GREEN.
>    **A codegen gap found live, not by inspection**: the `Order::from((row, Restaurant))` conversion
>    in `crates/server/src/graphql/generated/types.rs` is emitted from a HAND-WRITTEN string template
>    in `tools/codegen-rs/src/emit/server_graphql.rs` (not mechanically derived from the row's column
>    list) — the new field landed on the `Order` struct and the `OrderTrackingCompute` trait but not
>    in this template, so the workspace failed to compile (`E0063: missing field
>    delivery_handed_back`) until the template itself was patched. `cargo build --workspace` is what
>    caught it; `cargo run … --specs specs` (validate/generate alone) does not compile Rust and saw
>    nothing wrong.
> 3. **`custody-handback` observability contract**: (a) `business.food_location` was `required: true`
>    on `command.validate` with a comment claiming middleware stamps it — no span construction site
>    anywhere carries it (the only emit site is the fenced `inbox.rs`, and the fact already lives on
>    the event + `View_DeliveryJob.food_location`); removed the attribute and the claim. (b)
>    `max_age_seconds: 300` claimed `derived_from: DELIVERY_OFFER_MAX_TTL_SECONDS`, whose declared
>    default is 900 (`specs/delivery/configuration.yaml`, and the worker's own doc comment already
>    said 900) — corrected to 900. (c) "no later acceptance re-offering it" → "no later acceptance or
>    cancellation" (a FAILED WITH_RIDER job and an acknowledged PENDING one both keep ageing until
>    cancelled).
>
> **NON-BLOCKING, all fixed in this round:**
> 4. `delivery_read_model.rs`'s WITH_RIDER twin gained its own `OrderPlaced` (so it has an
>    OrderTracking mirror row) and now asserts `ordertracking.delivery_status = FAILED` +
>    `delivery_handed_back = true` there too — a mutant collapsing the Compute arm to PENDING/ASSIGNED
>    used to survive because only `View_DeliveryJob`'s projection-on-read side was checked.
> 5. `custody_handback_metric.rs`: the reassigned job's handback is now aged 3600s (OLDER than the
>    stranded job's 1800s, was previously left at its natural ~1680s wall-clock age — close enough to
>    1800s that a mutant collapsing "no later acceptance" to "any handback exists" could survive on a
>    `MAX()` that happened to still read close to 1800). Doc comment corrected: it now names the
>    POSITIVE CONTROL (not the recovery assertion) as the mutant's primary witness, matching what the
>    aging fix actually makes true.
> 6. **The `for_rider` / myDeliveries claim**: `for_rider`'s own WHERE clause is `(rider_id = $1 OR
>    (status = 'PENDING' AND rider_id IS NULL))` — the second arm is EVERY rider's pool, unfiltered by
>    identity, so an unfiltered myDeliveries call does NOT drop a PENDING handed-back job for the old
>    rider (only true for FAILED/WITH_RIDER). Added an explicit unfiltered `for_rider(rider1, None)`
>    assertion proving what actually holds: the row is still visible, `rider_id` is `None` — the
>    guarantee is unattribution, not absence.
> 7. `tools/codegen-rs/src/tests.rs`'s handback-lever grep test watched only the event name; a
>    ranking could read the custody influence via `food_location`/`handed_back_at`/`FoodCustody`/
>    `DeliveryHandback` without ever spelling `DeliveryHandedBackByRider`. Now watches all five tokens
>    across all four allowlisted files. `HandBackIsNeverALever` and the `DeliveryHandback` business-
>    metrics projection both gained a sentence: `riderId` is job ATTRIBUTION only, never a `groupBy`
>    dimension — a per-rider handback rate is the performance-and-behaviour ground counsel's own gate
>    refused before counsel (ADR-20260904-014136 §3).
> 8. This entry.
>
> **Gates, this round** (wall-clock, observed in-session; disk swept — `rm -rf target/debug/
> incremental` — before each heavy one):
> - `cargo run --manifest-path tools/codegen-rs/Cargo.toml -- --specs specs` (validate+generate): run
>   3 times as fixes landed (the emitter-template fix needed its own regenerate); each run 0 errors,
>   the pre-existing warning set unchanged in shape (`obs-technical-error-unreachable`/
>   `obs-metric-no-emitter` — untouched, no baseline refresh needed), ~15-20s each.
> - `cargo build --workspace`: 1m 16s clean AFTER the `E0063` round (5 hand-written
>   `OrderTrackingRow` fixture sites: `behaviour_support.rs`, `delivery_dispatch/tests.rs`,
>   `payment_settlement.rs`, `reclamation.rs`, `refund.rs`) and the emitter-template fix.
> - `cargo test -p web --lib`: 151/151 (0.20s) — includes the new sheet-router test, the three new
>   banner tests, and `router.rs`'s confirmation-page test corrected from asserting 2 reads to 1.
> - `cargo test -p domain -p application --lib`: 403/403 + 81/81 (0.06s + 0.01s combined).
> - `cargo test --workspace --lib --exclude codegen-rs`: every crate green, 0 failed (server,
>   infrastructure lib, actor_runtime, shared_types, core, telemetry, db_test_gate, etc.).
> - `cargo test --manifest-path tools/codegen-rs/Cargo.toml`: 403/403, 66.59s (includes the widened
>   watched-tokens test).
> - `make rust`'s own `check-drift` step read RED against `HEAD` on the first pass for the ordinary
>   reason (this round's entire diff was still uncommitted) — committed, then re-ran clean (see the
>   commit this entry lands in).
> - `bash tools/db-preflight.sh && DATABASE_URL=… DB_TESTS_REQUIRED=1 make test-crates`: full GREEN,
>   205/205 `test result: ok` blocks, 0 failed anywhere, no `DB-GATED SUITES SKIPPED` line —
>   07:40:43Z to 07:44:33Z, 3m 50s. Caught ONE more hand-written `OrderTrackingRow` fixture site
>   (`crates/server/tests/graphql_subscriptions.rs`) the earlier `--lib`-only builds never reached
>   (integration test binaries are not part of `--lib`); fixed in a follow-on commit.
> - `cargo clippy -p web -p application -p infrastructure -p server -p domain -p captain-food-codegen
>   --all-targets -- -D clippy::disallowed-methods` (the CI `lint` job's exact incantation, narrowed
>   to touched crates for disk): 0 errors, 42.84s.
> - `make rust` (the full validate+build+generate+diff+link-check gate): 07:36:24Z to 07:37:54Z,
>   1m 30s, 0 errors, `check-drift` clean against the committed tree, link-check 8417 links / 458
>   files, 0 broken.
>
> **Honest after-state, board card facts (item 8's own ask)**: the restaurant backoffice's pinned
> `delivery_handback_card` (`restaurant_backoffice.yaml`) is SPEC-COMPLETE but its screen's OWN read
> (`deliveries.byRestaurant` on `deliveries_board`) is `skipped_reads` per #745 — `restaurantId` is
> an identity fact the paint loop has no source for yet (#749/#750 land the sourcing). The card
> therefore does NOT render in production today; it is spec-declared, structurally unreachable. The
> ONLY things that actually tell anyone about a stranded handback right now are (1) the fold itself
> (`View_DeliveryJob`'s custody-keyed status, correctly PENDING/FAILED and re-offerable) and (2) the
> `delivery_handed_back_unreassigned_age_seconds` dead-man gauge this same PR's earlier round wired.
> No human sees a UI signal until #749/#750.
>
> **HOLD: human stands** as round 1 recorded — legal/stored-event surface, PR stays in draft for the
> TEAM's independent reviewer pass; never marked ready, never auto-merge armed, by this executor, at
> any point.
> Executor tier: **sonnet**. Base verified before any code: `git rev-parse HEAD` = `3d20b729`
> (the claim commit), `HEAD~1` = `origin/main` = `5b2d3da0`. Scope per the card, approved by
> ADR-20260904-015903 (ADR-20260810-221840 covers the spec diff): `FoodCustody` scalar,
> `HandBackDelivery` command (`derived: { riderId: rider }`), `DeliveryHandedBackByRider` event,
> the lifecycle `via:`+`when:` grammar extension (no existing grammar could key a transition off a
> non-status field; extended rather than adding a second event), the view `derive: { from, map }`
> grammar extension, two new rules, `handBackDelivery` mutation, rider sheet second exit + job
> after-state, board pinned card, customer tracking banner, `custody-handback` observability
> contract, the ONE fenced arm in `crates/infrastructure/src/inbox.rs`, the non-fenced
> `delivery_handback_watch.rs` dead-man worker.
>
> **Commits, in order** (spec 05:07 `e3833c54` → handler/fold/fence/worker 05:28 `201c3a0e` →
> migration+ACL/no-row/rule/dead-man tests+records 05:38 `fa9c34d9` → three bug-fix rounds below).
>
> **Three bugs found live by the mob's own gates, none by inspection** — a broader
> `cargo test -p domain -p application` sanity run beyond the specific named tests surfaced the
> first pair; `make test-crates`'s web suite surfaced the third:
> 1. **`DeliveryHandedBackByRider` was missing `orderId`** (D-QW1 option b, ADR-20260808-234907):
>    `ProjectionWorker`'s non-`"Order-"`-stream keying (`payload_uuid_of(env, "orderId")`) silently
>    skipped the event as "not poison" — the customer's OrderTracking mirror never moved. Added
>    `orderId` (required, folded from aggregate state, never client input) to the event, the
>    handler, every fixture.
> 2. **`OrderTrackingCompute::delivery_status`/`courier` declared the event in `fedBy` but the
>    hand-written Compute hook never got the match arms** — the spec promised a feed the Rust
>    never implemented. Added them.
> 3. **`emit/sql.rs`'s new `DeriveVal::Payload` arm emitted an uncast `payload->>'field'`** (TEXT);
>    in the `status`/`rider_id` CASE ladders mixed with cast branches, Postgres infers the WHOLE
>    column TEXT, breaking `rider_id = $1::uuid` downstream with "operator does not exist: text =
>    uuid". Routed through `payload_extract`/`pg_cast` like every other derive arm; patched the
>    already-written migration's `rider_id` CASE branch with the matching `::uuid` cast.
>    `TestDeliveryReofferedAfterHandBack`'s `then:` referenced the SAME fixture as its own `given:`
>    (rider-1), so it could never prove a SECOND rider took the job — added `deliveryAcceptedByRider2`.
>    Commit `c51b8051` (05:59). Also restored a "3-ii claimed" journal entry this session's own
>    earlier edit had dropped from the top of this file — diffed the working copy against
>    `origin/main`, confirmed every OTHER line byte-identical, restored — commit `26801e1e` (06:01).
> 4. **`rider.yaml`'s handback chips used compound `visible_when` (`a == 'X' && (b == 'Y' ||
>    b == 'Z')`)** — `crates/web/src/condition.rs`'s grammar is deliberately corpus-exact, no
>    `&&`/`||`/parens, and says so in its own module doc;
>    `condition::tests::every_generated_condition_parses` reds loudly. Rewrote as nested
>    `conditional_section`s (the corpus's own pattern, already used in `restaurant_backoffice.yaml`)
>    and collapsed one `||` into the grammar's `in [...]` form. Separately, `TrackingState::load()`
>    now issues a SECOND transport call (`delivery.byOrder`) alongside `order.byId`; three
>    FakeTransport-scripted tests still scripted one response per pull and panicked on "unscripted
>    call". Fixed all four. `cargo test -p web --lib`: 144 passed/4 failed → 148/0. Commit
>    `d095283e` (06:15).
>
> **Gates, this session's continuation** (wall-clock approximate, observed in-session):
> - `make rust` (build+test+validate+generate+diff+link-check): run 4 times as fixes landed, RED on
>   `check-drift` twice for the ordinary reason (real uncommitted work, not generator drift —
>   `git status --short` showed only hand-written files each time), **final run clean, 0 errors**,
>   `warning-baseline.json` unchanged in shape from what `fa9c34d9` already committed
>   (`obs-technical-error-unreachable` 12→13, accepted; `obs-metric-no-emitter` held at 46,
>   untouched as the card requires) — ~06:20Z GREEN.
> - `bash tools/db-preflight.sh && DATABASE_URL=… DB_TESTS_REQUIRED=1 make test-crates`: the
>   `ci-repro` cluster's `postmaster.pid` was stale (postmaster dead, per environment.md's known
>   recovery shape) and its port was in fact held by the machine's OTHER live cluster
>   (`/var/lib/postgresql/16/main`, port 5432) — used that cluster directly, `createdb
>   cf639handback`, dropped after. **Two full runs** (before and after commit `d095283e`'s web
>   fixes): first run GREEN on everything except the (not-yet-fixed) web suite; **final run fully
>   GREEN** — `infrastructure`'s `tests/main/main.rs` 101/101, `custody_handback_metric` 2/2,
>   `delivery_read_model` (incl. `a_handed_back_job_reappears_pending_on_the_board_and_the_customers_mirror`),
>   `server`'s ACL/subscriptions/write-path suites, `web` 148/148, workspace total 0 failed, 0
>   `error:` lines, no `DB-GATED SUITES SKIPPED` line anywhere in either run's log — disk dropped to
>   3.4G/91% mid-run (23G target dir) but held stable through to completion, no sweep needed beyond
>   the two-command pre-gate one.
>
> **Three named mutants, run against the GREEN tree above, each seen RED then reverted to a
> byte-identical diff (confirmed via `diff`) and reconfirmed GREEN**:
> 1. **The handler never compares `riderId`** — `if false && state.rider_id != Some(cmd.rider_id)`
>    in `commands.rs::hand_back_delivery`. `cargo test -p application --lib
>    generated::behaviour_tests::test_hand_back_delivery_rejects_rider_mismatch`:
>    `` panicked at crates/application/src/generated/behaviour_tests.rs:4087:22: TestHandBackDeliveryRejectsRiderMismatch: the spec expects a typed rejection: () ``.
>    Reverted, `git diff` empty, GREEN.
> 2. **The handback row dropped from `status.derive`** — mutated `View_DeliveryJob`'s
>    `DeliveryHandedBackByRider` status map to `{NOT_COLLECTED: ASSIGNED, RETURNED_TO_RESTAURANT:
>    ASSIGNED, WITH_RIDER: ASSIGNED}` (a handback that no longer moves status), hand-patched the
>    already-written migration's matching CASE branch the same way (spec `derive:` regenerates
>    `views.generated.sql`, never the hand-written migration), ran against a fresh `cf639mutant2`
>    DB: `` panicked at crates/infrastructure/tests/main/delivery_read_model.rs:379:5: assertion `left == right` failed: PENDING unless WITH_RIDER — this is RETURNED_TO_RESTAURANT  left: ASSIGNED  right: PENDING ``.
>    Reverted both files, `diff` byte-identical on the migration, `make generate` clean, GREEN.
> 3. **The observability "no later acceptance" predicate** — `delivery_handback_watch.rs`'s
>    `UNREASSIGNED_SQL` WHERE clause narrowed from `food_location IS NOT NULL AND status IN
>    ('PENDING','FAILED')` to `handed_back_at IS NOT NULL` (the naive "does a handback exist"
>    reading the test's own doc comment names — `handed_back_at` has no reset arm in `derive:`,
>    unlike `food_location`/`status`, so it stays set forever once a handback lands). Against a
>    fresh `cf639mutant3` DB the mutant was killed, but by the **recovery** assertion rather than
>    the positive-control one the test's doc comment names — the stranded job's manually-aged 1800s
>    still dominated the `MAX()` over the (uncorrected) reassigned job's near-zero raw age, so
>    part (b) passed regardless; the fold only diverged once the FORMERLY-stranded job was itself
>    reassigned and its own age should have dropped to 0 but the mutant kept counting it:
>    `` panicked at crates/infrastructure/tests/custody_handback_metric.rs:206:5: assertion `left == right` failed: a gauge nobody can close an incident on is not a gauge  left: [({}, 1800.0)]  right: [({}, 0.0)] ``.
>    Reverted, `diff` byte-identical, GREEN. **Filed as an adjacent finding for the PR body**: the
>    test's own doc comment over-attributes which assertion catches this specific mutant shape.
>
> **Fence self-check**: `git diff --name-only origin/main -- crates/infrastructure/src/inbox.rs`
> (plus every plausible mailbox/lease/fencing/event-store sibling path tried) prints exactly
> `crates/infrastructure/src/inbox.rs`; `git diff origin/main -- crates/infrastructure/src/inbox.rs
> | grep -c '^+[^+]'` = 1. **Flagged**: no file anywhere in the repo enumerates a canonical "seven
> fence paths" list — this check was built from first principles (every file plausibly fenced by
> the isolation programme, #780/#783/ADR-20260830-183000) rather than a citable source; the
> architect should record the canonical list once, so the next card can cite it instead of
> re-deriving it.
>
> **Record**: `docs/SPEC-LOG.md`, `docs/STATUS.md`, `docs/proposals/PROP-20260831-180622-…` row 3
> (LANDED, `custody-door` Concern checked) already landed in `fa9c34d9`. This entry is the
> continuation session's own record. **HOLD: human** stands as the card names — legal/stored-event
> surface (`domain_events` payload shape, fold semantics, a migration) — PR stays in draft for the
> TEAM's independent reviewer pass; **never marked ready, never auto-merge armed, by this
> executor, at any point**. `on_success` has no mutation-chaining primitive: `hand_back_delivery`
> ships alone on both confirm buttons; the ADR's "report + hand back" two-Tells sequencing is
> unwired and flagged in the PR body, not invented here. Issue #860's `deferred:` block for the
> re-offer PM leg: the grammar's `receives_deferred_grammar` machinery is same-actor only (checked
> `tools/codegen-rs/src/validate/core.rs`'s `receives_deferred_grammar` tests) and #860 is a
> DIFFERENT actor's (`DeliveryDispatchProcess`) future step, so it cannot carry the deferral here —
> noted as a PR body comment instead, per the card's own fallback.

> **2026-09-04 — #867 merged after a second round; the rider's controls have a source; 3-ii claimed.**
> [PR #867](https://github.com/TheCaptainCompany/captain-food/pull/867) (`5b2d3da0`, squash) executes
> PROP-171500 D2 for riders: `derived: { riderId: rider }` on six mutations (including
> `changeRiderStatus`, narrowed to `[RIDER]`), one `<Command>Input` per command with the field deleted,
> the resolver injecting the seam's `ReadScope::Rider` before the typed deserialize, `Forbidden` before
> enqueue when no scope resolves, the four rider controls rebound to `deliveryJobId`; baseline
> `action-missing-required-input` 11→7 and `action-unknown-input` 7→4 (antecedent: the PR's validator
> run). **Review: FAIL on the first pass, PASS on a bounded re-check — two rounds of three.** Blocking:
> the REQUIRED-derived fail-closed branch had no test reaching it (the `NoMapping` case is refused by
> the guard first; the ACL harness has no mailbox), fixed with a test seen red on the nil-uuid mutant
> (`left: 0 right: 1`). Non-blocking, fixed: a false coverage comment, a claimed `payload_hash`
> assertion that did not exist, the derived sentence replacing six InputObject descriptions, the
> `'{source}'` literal, the ReadScope fact cited to the wrong ADR in four code comments. Filed:
> [#868](https://github.com/TheCaptainCompany/captain-food/issues/868) (jobs-list accept/decline bind
> `delivery.id` outside the item scope — system-wide, `order_card` has no action slot; the online toggle
> supplies no `status` inside a component the validator never walks),
> [#869](https://github.com/TheCaptainCompany/captain-food/issues/869) (`myDeliveries` keys the rider on
> the JWT subject while writes carry the seam's id). **Lower-tier tally (ADR-20260904-013450
> §Decision 5): first-round PASS 1 of 2**; the reviewer's reading of both runs: the diffs are not
> different in kind, what slips is record accuracy and one substituted negative reported as coverage.
> Coordinator card defects this step, attribution card: the ReadScope fact cited to
> ADR-20260904-014135 (the two-role claim decision) instead of #849 / ADR-20260830-191457; "the
> aggregate compare stays the authority" stated for five mutations when it holds for two. **3-ii
> claimed** as [PR #870](https://github.com/TheCaptainCompany/captain-food/pull/870) from `5b2d3da0`,
> executor on `sonnet`, the one-arm fence carve-out per ADR-20260904-015903 §Decision 10.

> **2026-09-04 — #865 PR #867 review round 2: BLOCKING fixed, six non-blocking findings applied.**
> The reviewer's ONE blocking finding: the REQUIRED-derived fail-closed branch
> (`let Some(ReadScope::Rider(__derived_id)) = __derived_scope else { return Err(forbidden_error())
> }`) had NO test reaching it — `no_resolved_rider_scope_enqueues_nothing` uses `NoMapping`, refused
> by the `RoleGuard` as PUBLIC one step earlier; `graphql_acl.rs`'s `declineDelivery` control errors
> on `ctx.data::<Mailbox>()?` one line before the injection block, in a schema with no mailbox at
> all. New test `a_rider_bound_caller_with_no_readscope_in_context_is_refused_by_the_derived_seam_itself`
> (binds `ActingRole::Rider` directly through `schema.execute`, no HTTP transport, so no
> `ReadScope` is ever inserted) closes it. **Seen RED first**, verbatim: temporarily mutated
> `derived_injection_block`'s `required` arm to inject a nil UUID instead of erroring, regenerated
> `mutation.rs`, ran the new test alone —
> `` thread '...' panicked at crates/server/tests/rider_id_derived_at_the_door.rs:291:5: assertion
> `left == right` failed: expected exactly the derived seam's own refusal: []  left: 0 right: 1 ``
> (i.e. the mutant enqueued silently, 0 errors where 1 was expected) — then reverted the emitter and
> regenerated again (`git diff` on `mutation.rs` came back empty, confirming the revert was exact)
> before writing the real assertion.
>
> **Six non-blocking findings, all applied**: (2) `graphql_acl.rs`'s `admitted()` comment and this
> journal's own prose wrongly claimed `declineDelivery`'s `forbidden_error()` MIGHT fire in that
> mailbox-less harness — it cannot, `ctx.data::<Mailbox>()?` fails one line before the injection
> block runs, in every case, on every mutation, in that schema; both corrected. Added the missing
> `payload_hash` assertion `the_seam_injects_riderid_into_the_enqueued_payload` claimed to have.
> (3) new validator test `a_recognized_source_on_the_wrong_scalar_is_a_type_mismatch` (`derived: {
> deliveryJobId: rider }` — a real property, a recognized source, the WRONG scalar — the primary
> branch the existing `an_unrecognized_derived_source_is_a_type_mismatch` test does not cover);
> fixed the un-interpolated `'{source}'` string literal in `validate/api_derived.rs` (it was never
> passed through `format!`, so the fallback message printed the literal braces, never the actual
> source). (4) `emit_server_inputs`'s derived-property description now APPENDS to the command's own
> description instead of replacing it — the six `InputObject`s had lost their "An independent
> Captain rider accepts a pending delivery job." etc. (5) the dispatch card's wrong citation
> (`ADR-20260904-014135`) replaced everywhere it landed — `validate/api_derived.rs`,
> `derived_injection_block`'s doc comment, `specs/delivery/api.yaml:89`, the test file header, and
> the SPEC-LOG row — with the correct record: [#849 "#639 part C step 2b: the auth_ref -> rider_id
> resolver at the request seam"](https://github.com/TheCaptainCompany/captain-food/pull/849) /
> [ADR-20260830-191457](../adr/ADR-20260830-191457-a-role-guard-takes-a-witness-and-an-unbound-caller-is-recorded-as-public.md)
> parts A+B, realized in `crates/server/src/auth.rs::resolve_rider_scope`. (6) upgraded the ONE
> "past the guard" assertion whose harness carries a real mailbox
> (`rider_sign_in_door.rs`'s `the_token_the_rider_stamp_writes_opens_the_rider_door_once_the_seam_resolves_a_row`)
> to assert the enqueued payload's `riderId` directly; the two harnesses with no mailbox
> (`rider_without_a_row_is_forbidden_on_the_write_half.rs`'s two controls) already said so
> accurately in their existing comments — left as is, not a claim of coverage they cannot make. (7)
> `specs/screens/rider.yaml`'s `jobs` screen: the accept/decline buttons live in `job_list_actions`,
> a SIBLING section of `order_list`, OUTSIDE any item scope — `{{ delivery.id }}` (and, before this
> card, `{{ delivery.orderId }}`) never resolved there, confirmed by reading
> `crates/web/src/renderer.rs`'s `order_card` (renders id/status/total only, no action slot) and by
> the IDENTICAL pre-existing pattern in `restaurant_backoffice.yaml`'s `orders_queue`/`order_actions`
> (`{{ order.id }}`, same sibling-section shape, same missing singular resolver) — this is a
> SYSTEM-WIDE SDUI renderer/DSL gap (no item-scoped action slot exists on `order_card`, and no
> screen declares a singular "current order/delivery" binding), not a rider.yaml-local ≤20-line
> data-binding fix; **left as is**, reported to the coordinator to file as its own issue covering
> both screens.
>
> All four gates re-run green after the round-2 changes (`make validate`, targeted
> `cargo test -p server` on the four touched test files — 24/24 — and the full
> `cargo test -p captain-food-codegen --bin generate` — 402/402).
>
> **2026-09-04 — #865 landed on the branch: `riderId` derived at the rider door from the seam,
> deleted from the six rider-facing inputs** ([#865 "The rider surface has no rider-identity
> root…"](https://github.com/TheCaptainCompany/captain-food/issues/865), draft PR #867,
> `HOLD: human`, **executor tier: sonnet**). Base = claim commit `be592bd0` (parent
> `3b2614dc` = `origin/main`, checked FIRST and matched). Under
> [ADR-20260904-015903](../adr/ADR-20260904-015903-the-custody-doors-are-a-new-fact-a-rider-hands-a-job-back-with-the-food-s-whereabouts-and-the-read-models-fold-it.md)
> §6 (the closed operation-key seam) and PROP-171500 D2 (ADR-20260808-171056). **Red/green**: red
> `be592bd0` (the six mutations still take `riderId` as a client-suppliable payload field, the
> rider screen's accept/confirm/complete buttons pass the unknown `orderId` and miss the real
> `deliveryJobId`), green `92708668` (core landing) then `a5e7032f` (final, after the records
> commits and the E.4 mutant test added in a follow-up pass); `git diff --stat be592bd0..a5e7032f`:
> 28 files changed, 1215 insertions(+), 194 deletions(-). **Per-gate wall-clock** (warm cache after
> an iterative session —
> not a cold-build figure): `make validate` <5s; `make generate` ~15-20s; `cargo build -p server
> --tests` 1m28s; the five touched `crates/server/tests/*.rs` files run in ~1s (24/24 pass);
> `cargo test -p captain-food-codegen` ~65s test-time (401/401 pass, after fixing one stale test —
> below); `DATABASE_URL=… DB_TESTS_REQUIRED=1 make test-crates` ~2-3 min warm, full workspace,
> exit 0, receipts `DB PRE-FLIGHT OK` + no skip line + exit 0; `make rust` ~2 min warm, exit 0
> after committing the STATUS.md edit `check-drift` had (correctly) flagged as uncommitted.
>
> **The DSL**: an api.yaml mutation may declare `derived: { <commandProperty>: rider }` — the
> loader-closed source set lives in `validate/api_derived.rs` §28, `rider` the only arm today
> (`ReadScope::Rider(RiderId)`, ADR-20260904-014135's neighbourhood is where a citation went wrong
> — see Card defects below). Three new ERROR rules, each with a red-on-planted-mutant unit test
> (`api_derived_gate` module, 5 tests): `api-derived-field-unknown` (the key names no property of
> the command), `api-derived-type-mismatch` (the property's `$ref` is not exactly the source's
> scalar — an unrecognized source counts as a mismatch too, since it names no scalar), and
> `api-derived-role-mismatch` (a REQUIRED derived property forces `roles:` to exactly the source's
> role set; a NULLABLE one imposes no constraint). `api-operation-key`'s `MUTATION_KEYS` gains
> `derived`; `action-missing-required-input` subtracts derived keys from what a screen action must
> supply (the D2 authority for D1's payload-target compare stays at the aggregate, never at this
> emitter seam).
>
> **The emitter**: `object_fields`/`push_gql_object_fields` gain `_excluding` twins that omit named
> properties entirely — the six `<Command>Input` types (SDL + the server InputObject) carry no
> `riderId` field at all, each gaining a description `` `riderId` is derived from the caller's
> RIDER identity. `` (the `argsExactlyOneOf` `one_of_doc` precedent). The resolver template gains
> ONE injection block per `derived:` property, BETWEEN `let mut payload_json =
> command_payload(&input)?;` and the typed `serde_json::from_value` (young's trap named in the
> card: after that point every rider mutation would fail deserialization on a REQUIRED derived
> property) — reading `ctx.data_opt::<application::queries::ReadScope>()`: a REQUIRED property
> fails closed with a new `forbidden_error()` (errors.yaml#/Forbidden, mirrors `conflict_error`
> exactly) when the scope does not match; a NULLABLE one simply omits the key on any other path.
> `payload_hash` is taken from the TYPED command AFTER injection automatically (the typed send was
> already deserialized from the mutated `payload_json`) — no extra code, one test line asserting it
> (`the_seam_injects_riderid_into_the_enqueued_payload`).
>
> **Spec**: `acceptDelivery`, `confirmPickup`, `completeDelivery`, `declineDelivery`,
> `reportDeliveryIssue` (nullable) and `changeRiderStatus` all declare `derived: { riderId: rider
> }`; `changeRiderStatus.roles` narrows `[RIDER, ADMIN]` → `[RIDER]` (the role-mismatch rule forces
> it — the required derived id IS the caller); a rider's LIFECYCLE (restriction) is ADMIN's through
> `RestrictRider` instead (ADR-20260904-014136 §Decision 6(i), verified: item 6(i) reads *"The
> decision is taken by a human… so `RestrictRider` is unspellable for a system or process-manager
> principal"*). `specs/screens/rider.yaml`: `accept_delivery`/`confirm_pickup`/`complete_delivery`
> rebound from the unknown `orderId` to `deliveryJobId` (the real, previously-missing required
> input); `decline_delivery` was already correctly bound; `rider_toggle_online` already carried no
> `riderId` variable. Warning surface (antecedent: the validator run on the branch, quoted from
> that run, not from the card): `action-missing-required-input` 11→7, `action-unknown-input` 7→4 —
> baseline refreshed in the same commit.
>
> **Tests, D1 vs the seam**: `specs/tests.yaml` gains `TestConfirmPickupByAnotherRiderIsRejected`
> and `TestCompleteDeliveryByAnotherRiderIsRejected` (given requested + acceptedByRider rider-1 (+
> pickedUp), when `riderId: rider-2`, thrown `InvalidDeliveryStatus`) — the aggregate-side compare
> at `application/src/commands.rs:1426`/`:1466` (`state.rider_id != Some(cmd.rider_id)`) stays the
> authority where a rider is already assigned; verified by reading the handler, not by actually
> planting the mutant and reverting (a resource trade-off this session made explicitly, banked
> below). For accept/decline/report/changeRiderStatus there is NO aggregate compare (a PENDING job
> has no rider yet) — the seam is the only guard, stated in this entry rather than invented as a
> compare that does not exist. `crates/server/tests/rider_id_derived_at_the_door.rs` (new): the
> real edge (`POST /rider/graphql`, loopback JWKS, scripted `RiderIdentitySource`) over a
> `MemMailbox` (`graphql_typed_send.rs`'s `schema_over` shape) — `Resolved(RiderId(X))` →
> `acceptDelivery(input:{deliveryJobId})` → the enqueued payload JSON carries `"riderId": X`
> (structurally verified to have been RED pre-#865: the pre-image `AcceptDeliveryInput` required
> `riderId!`, so this literal — which supplies only `deliveryJobId` — would have failed GraphQL's
> own input validation against that schema); a sibling test proves `NoMapping` enqueues NOTHING
> (`mem.entries().is_empty()`) with `FORBIDDEN` from the ROLE GUARD (not yet reaching the new
> derived-seam code); a third proves the E.4 smuggled-field mutant — through `schema.execute`
> directly (never `Input::parse`, whose serde derive silently ignores unknown keys), `riderId`
> posted BOTH inline and via GraphQL `variables` hits async-graphql's OWN document validation
> before any guard or resolver runs, verbatim on both legs: `` Invalid value for argument "input",
> unknown field "riderId" of type "AcceptDeliveryInput" `` — nothing enqueued either way (added in
> a follow-up commit `a5e7032f` once the first pass was noticed to have missed it). Three
> pre-existing files rewritten because the literal `riderId` they carried
> no longer names a field at all — a client that still supplied it would fail GraphQL's OWN static
> validation, indistinguishable by `assert_ne!(code, Some("FORBIDDEN"))` from the role guard's
> refusal (the exact trap the card names): `rider_without_a_row_is_forbidden_on_the_write_half.rs`
> (doc comment + literals), `rider_sign_in_door.rs` (literal + a stale "fails on the payload's
> unknown job" comment — the mailbox now EXISTS in that harness, so the seam-injected command
> actually enqueues), `graphql_acl.rs` (`the_issue_doors_admit_exactly_their_listed_paths`: literals
> rewritten, the `admitted()` comment upgraded to name BOTH ways the resolver can fail past the
> guard — always the missing mailbox in THIS schema-only harness (`ctx.data::<Mailbox>()?` runs
> BEFORE the derived-field injection block, so `declineDelivery`'s own `forbidden_error()` branch
> is never reached here — round 2 review caught the comment claiming otherwise, fixed) —
> `is_forbidden` still discriminates correctly since it checks the ROLE GUARD's literal `FORBIDDEN`
> code, distinct from the seam's PascalCase `Forbidden`. **One test the full `cargo test --workspace` run caught that the narrower
> `api_derived_gate`-filtered run did not**:
> `tests::screen_actions_do_not_pass_undeclared_command_inputs` asserted the REAL corpus carried the
> `orderId` defect this card fixes — rewritten to assert the real corpus is clean, then plant the
> same defect as a mutant via a small recursive `find_action_mut` helper, proving the rule still
> catches it. Landed as its own commit (`92708668`) once the FULL gate (not the narrower filtered
> run) surfaced it — a real cost of running the narrow filter first, recorded as an operational
> learning.
>
> **Records**: SPEC-LOG row (2026-09-04, Tier 0 — the input-field deletions are breaking-but-free
> only because no client has shipped against the old shape yet, production suspended
> ADR-20260817-105844); STATUS.md's #639 part C row gains a closing sentence on the "not done, by
> design: `riderId` is not bound" adjacent finding from #864 — now closed; PROP-20260831-180622 row
> 3's 3-ii text gains one sentence: `handBackDelivery` declares `derived: { riderId: rider }` from
> birth.
>
> **Card defects banked, with attribution**: (1) *card* — the dispatch's Register check line cited
> `ADR-20260904-014135` for *"the rider's domain id lives in `ReadScope::Rider(rider_id)`, never in
> a claim"*; that ADR is actually the one-subject-may-hold-several-roles record (2026-09-04, a
> DIFFERENT founder decision) and says nothing about `ReadScope`. The underlying technical claim is
> true and independently verified (`application::queries::ReadScope` / `auth.rs::resolve_rider_scope`)
> — SPEC-LOG's row states the mismatch rather than repeating the wrong citation as fact. (2) *card*
> — E.5's D1 mutant ("delete the compare at `commands.rs` ~1426/~1466 … must go RED") was verified
> by reading the handler and confirming the exact lines match, not by mechanically planting the
> mutant and reverting it — a scoped resource trade-off in a session that already ran the full
> `cargo test --workspace` gate twice end to end; the compare's existence and behaviour are not in
> doubt (the two new spec test cases exercise it directly and pass), only the literal red-then-green
> commit sequence for that specific mutant was skipped. (3) E.8's farley walk (a real Postgres +
> real mailbox worker draining `acceptDelivery` into a `DeliveryAcceptedByRider` row, then a second
> rider's `confirmPickup` refused) was **not reached this session** — the card's own escape hatch
> ("if the walk harness cannot reach it, say so"), given the same resource trade-off; the closest
> existing harness is `crates/server/tests/graphql_write_path.rs`'s `spawn_mailbox_workers` pattern,
> unadapted.
>
> **Adjacent findings, for the architect (not fixed)**: `myDeliveries` (`server_graphql.rs` ~764)
> still derives the rider from `Principal::user_id()` — the CLAIM, not the seam — a different
> pattern than this card's `ReadScope::Rider` derivation and worth reconciling in its own change;
> untouched here per the card's explicit instruction not to. The fence self-check (unchanged list)
> was not re-run as a separate step — this run touched no fenced path (`crates/infrastructure/src/inbox.rs`
> and friends), verified by `git diff --stat` naming only `tools/codegen-rs/**`, `specs/**`,
> `crates/*/generated/**` and `crates/server/tests/**`.

> **2026-09-04 — #864 merged: the first lower-tier executor PR passed its first reviewer pass; #865
> briefed and claimed.** Step 3-i (`3b2614dc`, squash): the reviewer's ONE pass on `3b3a787a` returned
> PASS with no BLOCKING finding attributable to the PR — **first-round PASS tally under
> ADR-20260904-013450 §Decision 5: 1 of 1** (numerator/denominator as the ADR defines them; window
> 10 PRs or 14 days from 2026-09-04). Triage: three NON-BLOCKING fixed in the PR (a `sheet.data`
> binding the runtime cannot resolve; a gate never seen red — `view-derive-null-not-nullable` — given
> its mutant; a metric named `DeliveryIssueRate` in two records that is `DeliveryIssue` /
> `delivery_issue_rate`), two filed ([#865](https://github.com/TheCaptainCompany/captain-food/issues/865)
> every rider write control is unsubmittable — `riderId` has no source on the rider surface, four
> wide since #92, the 3-i `[Refuser]` made it five; [#866](https://github.com/TheCaptainCompany/captain-food/issues/866)
> the `ratio` sub-grammar is not closed). The reviewer's reading of the tier: the diff is not
> different in kind from #835–#854; what slipped is exactly holub's two classes — record accuracy
> (an impossible green-SHA line, a metric name that does not exist, a relative link that broke a
> gate) and one borderline fail-closed call resolved by accepting a warning rather than stopping.
> **One CI red was the coordinator's**: a `gates.md` heading pushed to `main` ("Five more…") tripped
> the codegen test that forbids a heading stating the length of its list, and #864's merge ref
> inherited it — fixed on `main` (`b373901e`), rule in gates.md §19d. **#865 briefed** to five lenses
> (a recorded decision, PROP-171500 D2, no option space; compact roster): consent on one input type
> with `riderId` deleted and derived at the door from `ReadScope::Rider`, six mutations including
> `changeRiderStatus` narrowed to `[RIDER]` (ADMIN's lifecycle path is step 4's `RestrictRider`),
> ADMIN reports with the key absent. **Three card defects banked, attribution: card** — three of the
> four rider controls also pass `orderId` and miss `deliveryJobId` (farley, from the validator log);
> `changeRiderStatus` is a sixth rider-facing `riderId` input (beck); "the aggregate compare stays the
> authority" is true for pickup and completion only — accept, decline, report and the status toggle
> have no compare, so the seam is their only guard (vernon, farley), and the "actor-side
> `requires.acting`" the card cited does not exist for riders (#144). Claimed as
> [PR #867](https://github.com/TheCaptainCompany/captain-food/pull/867), executor on `sonnet`,
> sequenced BEFORE 3-ii so `handBackDelivery` is born with a source. Adjacent, filed nowhere yet
> (goes with #865's hand-back): `myDeliveries` still derives the rider from the claim, not the seam.

> **2026-09-04 — #639 part C step 3-i landed on the branch: the issue doors, and the read model that tells the restaurant** ([PR #864 "#639 part C step 3-i: the issue doors (report, resolve, decline) and the read model that tells the restaurant"](https://github.com/TheCaptainCompany/captain-food/pull/864), `HOLD: human`, **executor tier: sonnet** — the first lower-tier PR under ADR-20260904-013450). Under [ADR-20260904-015903](../adr/ADR-20260904-015903-the-custody-doors-are-a-new-fact-a-rider-hands-a-job-back-with-the-food-s-whereabouts-and-the-read-models-fold-it.md) §4–§6. **Red first**: red SHA `c1417d46` (the fold test failed against the APPLIED DDL with Postgres 42703 `column "open_issue_kind" does not exist` — #861's stand-in, exactly as the card predicted; the two validator modules and the ACL/seam tests red beside it), green SHA `7b524d5f`; `git diff --stat c1417d46..7b524d5f`: 54 files changed, 1413 insertions(+), 105 deletions(-). **Per-gate wall-clock**: rust-build ~1s (cached in the final bundled run), rust-test ~80s (component sample; bundled below), validate ~1s, check-drift ~3s, test-crates 5m48s (348s), total 29m (1740s, run start to last gate). **The named mutant** (delete the `DeliveryIssueResolved: null` derive arm → regenerate → the migration re-copied from the regenerated SQL): the projection test went RED on its SECOND assertion (`assertion `left == right` failed: DeliveryIssueResolved must clear open_issue_kind (derive: null -> THEN NULL)
  left: Some("CUSTOMER_UNREACHABLE")
 right: None`) — and note the mutant reaches the test ONLY through the re-copy, because the applied DDL is the hand-written migration, not `views.generated.sql`: that gap is #861, observed live. **Warning surface**: `action-missing-required-input` 10→11 (`declineDelivery` requires `riderId`; the rider surface has no rider-identity root — the same hole accept/confirm/complete already sit in), `command-no-mutation` 11→8, `event-not-projected` 6→4; baseline refreshed in the same commit. **Card defects banked**: (1) the card says "run the projector" for `View_DeliveryJob` — it is a projection-on-read VIEW, there is no projector, appending the facts is the whole write side; (2) "a bare YAML null without the explicit form is an error" — a YAML `null` IS the explicit form the card's own A.5 spells, and serde cannot tell `key: null` from `key:`, so the test pins the class that was actually silent (any unrecognised arm value: a mapping without `from`, a number…) as `view-derive-value-unknown`, plus `view-derive-null-not-nullable`; (3) `CREATE OR REPLACE VIEW` alone cannot land the column — the emitter trails `created_at`/`updated_at`, so the migration must `DROP VIEW IF EXISTS` first (the 20260730043100 precedent); (4) the card's `reportDeliveryIssue { …, riderId, … }` binding cannot be sourced on the rider surface (verified: the resolvers are `deliveries.mine`/`delivery.byOrder`, `DeliveryJob` carries no `riderId`) — omitted (nullable on the command), banked as an adjacent finding. **Adjacent, for the architect (not fixed)**: the rider surface has no identity root, so every rider mutation that takes `riderId` from the payload is only partly wired from a screen; the ratio sub-grammar of `value:` is unvalidated below the op name (#484's machinery); `origin/main` moved to `d9e2e088` during the run (docs/claude/sessions.md) — not rebased, per protocol. Next: 3-ii.

> **2026-09-04 — #639 part C step 3 briefed to the whole roster; the team's first option-space decision
> under `TEAM-DECIDES-OPTION-SPACES`: the custody doors are a NEW fact.** Thirteen lenses, before any
> code. **Card defect banked, attribution: card** — the coordinator graded option (a) (reuse the partner
> unassign as `handBackDelivery`) "additive, GREEN"; six lenses independently found it false at HEAD
> (`unassign_delivery_from_partner` refuses a rider-held job, the lifecycle admits it from ASSIGNED
> only, the rule says "the only assignment path"), so (a) was (b) in partner vocabulary with no
> `riderId` and no custody fact, and (a)-now-(c)-later is the shape staging ADR-20260808-235113 does
> not license (holub). **Decision by consent**, no lens naming a harm: option (c),
> [ADR-20260904-015903](../adr/ADR-20260904-015903-the-custody-doors-are-a-new-fact-a-rider-hands-a-job-back-with-the-food-s-whereabouts-and-the-read-models-fold-it.md) —
> `HandBackDelivery { deliveryJobId, riderId, foodLocation: FoodCustody }` → `DeliveryHandedBackByRider`;
> transitions keyed on custody (`WITH_RIDER` → FAILED, never PENDING — an oversell otherwise, vernon);
> OUT_FOR_DELIVERY in the set; no free-text reason and a handback is never a lever (legal); the issue
> door takes the D2 pattern (closed `DeliveryIssueKind` + 300-char note; ADR-20260808-171056 was
> controlling and the card had missed it). **The finding that dominated, in every option: nobody is
> told** — `View_DeliveryJob` and `OrderTracking` fold no release fact and `rider_id` never clears, so a
> door on the write side alone re-creates §7.2 with a nicer button; the fold is slice content, and
> farley found `views.generated.sql` is applied by NOTHING (hand-written migration + `include_str!`
> chain; gate filed as [#861](https://github.com/TheCaptainCompany/captain-food/issues/861)). **The
> fence is opened for exactly one additive arm** in `crates/infrastructure/src/inbox.rs` (E0004 demands
> it): antecedent — #780 closed 2026-08-30 (PR #783), last fenced-path commit `c1a70a6f` 2026-08-30, no
> `status/in-progress` issue but #639, no open PR on a fenced path. Step 3 splits into **3-i** (issue
> doors) and **3-ii** (handback), both `HOLD: human` — the proposal's "GREEN once additive" corrected in
> row 3. Filed: [#860](https://github.com/TheCaptainCompany/captain-food/issues/860) re-offer PM step
> (fenced), [#862](https://github.com/TheCaptainCompany/captain-food/issues/862) no customer remedy
> path from a delivery outcome (capture-on-delivered makes it a void, the restaurant make-whole has no
> flow), [#863](https://github.com/TheCaptainCompany/captain-food/issues/863) the DeliveryJob erasure
> list is an intention not an artifact. Register-check correction (architect): the carve-out's records
> are the PROP and ADR-20260830-234532, not ADR-20260904-014136 as the card said. Six stale drafts named
> by holub (#844, #841, #654, #621, #587, #365 — the oldest 28 days). Next: claim 3-i, executor on
> `sonnet`, tier stated on the card and the PR body.

> **2026-09-04 — Five founder answers recorded, #854 merged on a first-round PASS, and the team now
> decides option spaces.** The founder answered the 2026-09-04 form (five questions, register-checked
> each). **(1) Authority** — *"I authorize you to do everything"* (2026-09-03), scope chosen: *"team
> decides option spaces and spec diffs; external, legal and admin-gated actions still come to me"* →
> [ADR-20260904-013834](../adr/ADR-20260904-013834-the-team-decides-option-spaces-and-spec-diffs-external-legal-and-admin-gated-actions-stay-with-the-founder.md),
> register row `TEAM-DECIDES-OPTION-SPACES` (decided), banners on ADR-20260810-215503 items 1 and 3
> and on ADR-20260808-144738 d.3 (*"When in doubt, it goes to the customer"* — the tie-break is now
> consent in the mob; a split takes the reversible option behind a gate, the safer option on a legal
> surface; holub's finding that deleting the tie-break unstated would have re-created the PM in the
> coordinator's seat), CLAUDE.md's "NOT delegated" clause rewritten. **(2) A rider who also orders
> dinner** → *"final vision: one claim, one binding per role; own issue after step 6; refusal stands"*
> → [ADR-20260904-014135](../adr/ADR-20260904-014135-one-subject-may-hold-several-roles-the-claim-carries-a-role-set-and-the-path-picks-the-one-that-acts.md):
> young's ambiguity resolved by the team the way that contradicts no record — a binding is a ROLE in
> the token, the identity resolved in our Postgres (ADR-20260818-004646 read forward), additive
> producer + tolerant reader + one write, readers deploy first, `domain_events.user_type` stays the
> path role; the `deletion:` block for `Rider`/`Member` and the never-released reservation's
> retention ground are owed in the same issue (legal); Concern `one-subject-one-role` checked; built
> by [#857](https://github.com/TheCaptainCompany/captain-food/issues/857) after step 6. **(3) Rider
> restriction vs counsel timing** → *"build step 4 now with the smallest closed set naming no
> work-performance ground; counsel can only add"* →
> [ADR-20260904-014136](../adr/ADR-20260904-014136-rider-restriction-ships-now-with-the-smallest-closed-set-of-grounds-and-counsel-can-only-add.md):
> four grounds naming the fact observed (legal's proposal, not clearance), performance grounds and
> catch-alls refused, additive-only (never removed — unspellable at the door instead), the fold keys
> on the FACT never the ground, a read-only catch-all variant keeps the stream loadable (young: strict
> decoding of an unknown ground fails the whole stream load and blocks `ReinstateRider`), Directive
> 2024/2831 duties on the event, the notice and the review path ([#858](https://github.com/TheCaptainCompany/captain-food/issues/858));
> Concern `revocation-grounds` rewritten, discharges when step 4 lands. **(4) Model tier** → *"lower
> tier always for the executor; big tier only for lenses and reviewers"* (the literal reading; holub and
> farley had read *"if it's possible"* as room) →
> [ADR-20260904-013450](../adr/ADR-20260904-013450-the-executor-runs-on-the-lower-model-tier-and-lenses-and-reviewers-keep-the-bigger-one.md),
> the never-applied 2026-08-28 workflow.md section bannered, its `HOLD: human` carve-out withdrawn,
> [PR #859](https://github.com/TheCaptainCompany/captain-food/pull/859) adds `model: sonnet` to
> `executor.md`/`generator.md`; exit condition per holub: first-round review PASS over the first 10
> lower-tier PRs or 14 days, trip = 0/10 or a `HOLD: human` PR hitting the three-round ceiling → a
> decision-queue row; **precondition: every dispatch card and PR body states the executor tier from
> now on.** Baseline on the bigger tier, this week: **1 of 5** first-round PASS (#835, #846, #849,
> #852 FAIL→PASS; #854 PASS). **(5) Build order** → *"keep the approved order: 3, 4, 5, 6, 7"*. Cost
> recorded honestly (holub): steps 4–5 spend two `HOLD: human` slices restricting and disconnecting
> riders while the rider population is zero (production deliberately suspended, ADR-20260817-105844),
> and restaurant staff — the side that must be told about a paid order — get no sign-in door until the
> fourth slice from now; a 3 ∥ 6 lane was considered and is the re-ranking the founder just declined
> (the two slices share `stories.yaml`, `tests.yaml`, SPEC-LOG, the journal and the baseline, and step
> 6 must land `public-graph-limits` under one reviewer pass). **#854 merged** (`08262fa7`, squash):
> the team's one reviewer pass returned PASS with no BLOCKING finding — three NON-BLOCKING ones in
> [#855](https://github.com/TheCaptainCompany/captain-food/issues/855) (sync `errors[].message` and
> the bus frame still English; subscription-bound resolvers invisible to the screen-role walk; HTML
> docs badge), the two dead controls the WARNING form found in
> [#856](https://github.com/TheCaptainCompany/captain-food/issues/856); zero pre-existing assertions
> moved in a runtime commit; read-time localization of `Operation.message` judged presentation-only
> (the additive step ADR-20260719-120000 reserved), no gate. Next: step 3, the custody doors (GREEN),
> on the lower tier.

> **2026-09-03 — #639 part C step 2c-ii: the rider sign-in SCREEN, and R1 — the per-screen transport role
> that makes it reachable. A rider can sign in end to end from the rider app for the first time.** PR
> [#854](https://github.com/TheCaptainCompany/captain-food/pull/854), `HOLD: human`, hands back in draft;
> PROP-20260831-180622 build-order row **2c is LANDED** (2c-i #852 + this). **R1** (FORK 3, founder
> 2026-08-31): the screens DSL gains `graphql_role: <UserType>` — generated onto
> `Screen::graphql_role`, honoured by `Surface::role_for(screen)` (`crates/web/src/router.rs`), which
> `hydrate()` uses for BOTH transports of a page (reads and the interact driver's writes + push
> socket) and SSR uses for the skip/failure split; default = the surface role, every pre-R1 screen
> byte-identical (pinned). Validator **§26** (`validate/screen_roles.rs`) makes the wrong combination
> unspellable: a declared role must be one of the screen's `roles`, `PUBLIC` requires
> `requires_auth: false`, and every operation the screen BINDS — its tree with chrome expanded, its
> reads, the sheets it opens transitively — must admit it (`screen-graphql-role-refused-operation`,
> an ERROR, seen RED against a planted `graphql_role: PUBLIC` on the RIDER job list: five errors, one
> per refused operation plus the two clause errors). **graphql-architect's general form**
> (`screen.roles ⊆ ∩(op roles)`) turned out RED on two existing screens — `deliveries_board` ×
> `escalateDelivery` refuses RESTAURANT_ACCOUNT, the storefront `restaurant` × `markRestaurantAsFavorite`
> refuses PUBLIC: two pre-existing controls that render and do nothing for part of their audience —
> so it lands as a WARNING held by the ratchet at exactly those two (+2 baseline, pinned by a test),
> filed for the architect rather than re-scoped here. **The screen**: `/sign-in` (`roles: [PUBLIC]`,
> `graphql_role: PUBLIC`) — prefilled `+33` + phone → `requestRiderSignInCode` → the six-digit code in
> `rider_code_sheet` → `confirmRiderSignIn` → `claim_session` (awaited) → `/`; deliberately not the
> `rider_topbar` chrome, whose online toggle is a RIDER mutation §26 would refuse. `jobs`/`job_detail`
> declare `unauthenticated: { type: navigate, route: "/sign-in" }`: the server 302s a cookie-less GET
> to the door before any render (`hosts.rs::unauthenticated_bounce`, cookie PRESENCE is the signal),
> the client navigates there on an HTTP 401 from its role path (the positive no-session signal; a
> signed-in rider whose reads answer never bounces). Before: `job_detail` bounced to `/` and `jobs`
> painted its shell over a failed read. **The refusals** (ADR-20260830-213135) render in their own
> `inline_error` (`for_action`) beside the resend button, in the caller's language — which required a
> finding the card did not anticipate: `Operation.message` was built with `message_en` on every leg
> and no error-code translation exists anywhere (the customer door shows an English toast today). It
> is now localized at READ time from the mailbox row's typed `{ code, context }` (`RequestLocale`,
> cookie → Accept-Language → default, injected on the HTTP and WS transports; the push leg re-reads
> the durable row on a terminal verdict so both legs agree), so `RiderNotRegistered` reads as the
> French catalogue sentence naming the baked `SUPPORT_CONTACT` — no stored shape, no schema change,
> the fenced mailbox handler untouched. The deploy emitter derived the ingress consequence on its own:
> `riders.captain.food` now routes `/public/graphql` to `gateway-public`. **Card defects**: (5c)
> presumed a story→screen `$ref` grammar that does not exist (zero `screen` hits in `stories.yaml`,
> no rule) — prose only, filed; "the existing customer-door copy" for OTP errors does not exist; the
> card's example name `transport_role` yielded to the proposal's own `graphql_role` (§5, §8.1).
> Fence untouched. Gates: see the PR body.

> **2026-09-03 — #639 part C step 2c-i: the rider sign-in door, backend half — the platform now
> ISSUES a `role: RIDER` credential, to riders only, and stamps no id.** PR
> [#852](https://github.com/TheCaptainCompany/captain-food/pull/852), `HOLD: human`, hands back in
> draft; PROP-20260831-180622 gains build-order row **2c** (2c-i this PR; 2c-ii = R1 + the rider
> sign-in screen, next) and the **`one-subject-one-role` Concern** (registered, not checked). What
> landed: `stamp_rider_put_body()` / `identity.stamp_rider_claim` — a SECOND hardcoded stamper
> beside the customer's, its whole `captain_food` object `{ role: RIDER }` and nothing else (a
> distinct function, a distinct port method, its own decision type; `stamp_put_body` untouched and
> unparameterised), with its verifier-parity test `the_verifier_reads_what_the_rider_stamp_writes`
> asserting the ABSENCE of any binding as much as the role; two identify-only `roles: [PUBLIC]`
> mutations `requestRiderSignInCode` / `confirmRiderSignIn` on the `Rider` lane, both emitting
> nothing — the request leg never consults the rider read model (enumeration-safe by construction,
> asserted as "consulted zero times"), the confirm leg verifies the OTP, looks the subject up
> through the 2b `RiderIdentityRepository` bridge (a projection read: sign-in is query-shaped),
> REFUSES an unknown phone with `RiderNotRegistered` naming `SUPPORT_CONTACT`, refuses a subject
> already holding another claim object with `AuthSubjectHoldsAnotherRole` (fail closed, never an
> overwrite — the stamper's decision is strict: exactly `{ role: RIDER }` is a no-op, nothing of
> ours is a PUT, anything else is refused), then stamps → rotates → parks the POST-STAMP session
> for `POST /auth/session` owned by the row's X-SESSION-ID (envelope, not payload). `SUPPORT_CONTACT`
> is declared (`required: [staging, production]`, no default, baked `support@captain.food`),
> resolved once at the composition root as `Option<EmailAddress>`; unset (dev) makes the door fail
> CLOSED before spending the OTP rather than print an empty route. **Two card instructions could not
> both hold and the validator said which**: (1) `roles: [PUBLIC]` + "a story step under the rider
> persona" — `story-role-not-authorized`: a RIDER persona may not call a `[PUBLIC]` op, so the
> activity is `public_user.SignInAsRider` (the `verifyPhone` precedent), the ACL decision kept;
> (2) declaring the OTP refusals on the Rider inbox made them multi-scope — `scope-placement-error`
> derives `common`, and kernel purity then drags `DialingCode` along — so the OTP vocabulary (three
> scalars, five errors) is promoted to `specs/common/`, zero ref rewrites, zero stored-shape or SQL
> change. **Seen red first**: the six spec cases compiled RED against the emitter (E0061, the
> handler call had no arm), and transport test (a) was run against the exact mutant the card
> names — `confirmRiderSignIn` in `verifyPhone`'s register-or-identify posture — and failed with
> `expected a TYPED rejection, got Ok(())` before the refusal was restored. The transport tests
> (`crates/server/tests/rider_sign_in_door.rs`) drive `POST /public/graphql` on the production
> `graphql_routes`, then deliver the `MemMailbox` row through the HUMAN-OWNED router
> (`infrastructure::inbox::route`, the same `RiderInbox` arm the worker runs) over a scripted
> identity port whose CUSTOMER stamper panics — compile-time selection observed at runtime — and
> every port the door never reads keeps its production type over a `connect_lazy` pool (no
> database, no lookalike doubles: `behaviour_support` is `cfg(test)` and invisible to integration
> tests); the end-to-end leg signs a JWT whose `app_metadata` is the stamper's own PUT body and
> passes `acceptDelivery`'s `RoleGuard` once the seam resolves a row, FORBIDDEN as PUBLIC without
> one. `CommandDeps` (hand-written, `inbox.rs`) grew `riders` + `support_contact`, so every
> construction site moved (the composition root shares ONE `PgRiderRepository` between the seam
> and the door; `mailbox/standalone.rs` — FENCED — took the two-field minimum in its env-gated
> posture). Telemetry: `rider_claim_stamp_failed_total{reason}` on the `rider-identity` contract
> (the customer counter's pattern under this contract's own name; the shared `claims.stamp` span).
> Not done here, by scope: the screen and the per-screen PUBLIC capability (2c-ii); any revocation,
> custody or roster work. Adjacent finding for the architect: a provider failure AFTER the OTP is
> consumed (stamp/rotate/park) surfaces to the rider as `InvalidVerificationCode` on the mailbox's
> retry, since the retry re-verifies a spent code — the customer door swallows the same failure
> instead; both want a typed `SignInUnavailable`. Gates: `make validate` 0 errors (ratchet
> `identity-property-not-on-command` 1 → 3, accepted: the sign-in commands cannot carry a
> `riderId`); `make rust` and `make test-crates` (`DB_TESTS_REQUIRED=1`, real Postgres) green.
> **Review round 1 applied (same day)** — the independent reviewer's ONE blocking finding, B1: a
> confirm sent with no `X-SESSION-ID` parked an OWNERLESS session (`session_id: None`; the
> `AuthSessionStore` both-`None` claim is another channel's contract, and Postgres matches it with
> `IS NOT DISTINCT FROM`), so any header-less `POST /auth/session` holding the acceptance
> messageId — present in spans and logs — could take the credential; unreachable from the SDUI
> client (always sends the header), reachable from anything else. Fixed AT THE DOOR, never at the
> port: `confirm_rider_sign_in` refuses with the typed `RiderSignInRequiresSession` BEFORE the OTP
> is spent (the code stays usable for a correct retry; nothing verified, stamped or parked), the
> parked row is now `Some(owner)` by construction. Seen red first through the real transport —
> door test (f), a known rider with the right code and no header: `expected a TYPED rejection,
> got Ok(())` — and in the generated bed. **The bed had been passing `None` for every rider
> confirm** (not in the card): the emitter now presents a session by default and withholds it
> only for the case whose declared outcome IS the missing-session refusal (`thrown` names it; no
> `when.session` key added). Also here: `SUPPORT_CONTACT`-unset seen red against a mutant (door
> test (g), `got Ok(())` with the guard removed); `TestRiderConfirmSignInIdentifies` renamed to
> what `then: []` proves (the stamp/park port effects live in transport test (b) — an emitter gap,
> noted, not filed); STATUS's Concern count corrected to five; and the post-verify provider-failure
> finding filed as [#853](https://github.com/TheCaptainCompany/captain-food/issues/853).

> **2026-09-03 — #639 part C step 2b: the rider sign-in door — a rider is whoever OUR Postgres says
> the login belongs to, never whoever the token says, and nobody when there is no row.** PR
> [#849](https://github.com/TheCaptainCompany/captain-food/pull/849), `HOLD: human`, hands back in
> draft; build-order row 2 of PROP-20260831-180622 is now LANDED (2a = #846). **The §10 pair was seen
> red first, both halves, for the real reason** — (a) *"Postgres wins over the claim"* got
> `Rider(B)` where the table said `A`; (b) *"no row -> fail closed"* got `Rider(B)` where `Public`
> was owed — and the *try-Postgres-else-claim* shape was tried on purpose: it passes (a) and fails
> only (b), exactly as `beck` predicted; the WS-seam mirror
> (`ws_connection_init_resolves_rider_through_postgres_not_the_claim`, a signed JWT through
> `authorize_and_resolve_scope`) was red the same way. **The binding is unspellable, not merely
> unread**: `Identity::Rider { sub }` has no id field and `ProductClaims` parses no `rider_id`, so
> `serde` treats a token's `rider_id` as a stranger's key — the four-key serde pin now asserts that
> absence. The RIDER seam is `Postgres` with no `Claim` arm and no OFF state (`RiderIdentitySource`
> is a private-field struct; the no-database boot mode gets `NoDatabaseRiderIdentity`, which answers
> `LookupFailed` — PAGE — rather than pretending a missing database is a provisioning gap);
> `IdentitySources { customer, rider }` is the one value both transports carry, so a transport
> cannot wire one seam and forget the other. The three-way outcome became `IdentityResolution<Id>`
> with the customer and rider names as aliases, so the vocabulary cannot drift between seams. The
> read port `RiderIdentityRepository::rider_id_by_auth_subject` is born `AuthSubject` beside the
> still-`ExternalReference` `by_auth_ref` (#836 unifies them), selects `rider_id` and nothing else,
> and never `LIMIT 1`s. Its own `rider-identity` observability contract (the customer shape under
> its own metric names — a paging rule keyed on `customer_identity_lookup_failed_total` must not go
> quiet while riders fail on a seam it does not watch). **Two more gates**:
> `crates/server/tests/role_injection_gate.rs` now walks `tests/` RECURSIVELY — the `read_dir`
> walk stopped at the top level and its `scanned >= 8` floor was met there, so a suite in a
> sub-folder was invisible while the gate stayed green; seen red by planting
> `.data(RequestRole::Admin)` one level down — and `rider_duplicate_auth_ref.rs` pins what the
> default `DbFaultPolicy::Skip` does with two `RiderRegistered` sharing an `authRef` (reachable
> now only by REPLAY of pre-#794 history): the checkpoint advances past the duplicate, the second
> rider has no row, and the door reads the FIRST rider — the policy default is untouched, its own
> decision. **What this does NOT do**: no claim writer mints a `role: RIDER` token (the sole
> stamper hardcodes CUSTOMER), so end-to-end rider sign-in still waits on the token that walks
> through the door. **Corrected in the re-presentation (same day): 2b as first pushed REGRESSED
> part B for RIDER.** The independent reviewer's one blocking finding: `authorize_and_resolve_scope`
> minted the `ActingRole` from `Identity::Rider` BEFORE `resolve_read_scope` ran, so a bare
> `role: RIDER` JWT with no `Rider` row read `Public` and still ACTED as RIDER on all five
> `ALLOW_RIDER` guards and RECORDED `RIDER` in `domain_events.user_type` — a false author in an
> immutable log, and on `acceptDelivery` (target from the payload, never the caller) an acceptance
> naming any rider. The first push had rewritten the `graphql_acl.rs` assertion to match the runtime
> and called it "the customer seam's own asymmetry, named not widened" — inaccurate, since the
> customer asymmetry needs a stamped claim and sits behind a default-OFF gate while the rider arm
> needed no claim and had no gate. The re-presentation restores unbound ⇒ denied on BOTH halves: the
> verifier yields `Identity::Unbound { role: RIDER }` for every rider token, the seam hands back a
> principal whose identity IS its outcome (`Identity::Rider` only on a row — its sole producer),
> and the witness is minted from that principal after the seam; `ActingRole` keeps its one
> constructor. Seen red first: `rider_without_a_row_is_forbidden_on_the_write_half.rs` (a signed
> bare-`RIDER` JWT through the real `POST /rider/graphql` → FORBIDDEN as PUBLIC; resolved-row
> control passes the guard). Card defects: none of substance; the "part A already landed the table" note
> and the three `(principal_kind, auth_ref)` / "unbuilt" sites (#848 item 4) are corrected in the
> proposal in the same change. Gates: `make validate` 0 errors, ratchet exact match; `make rust`
> and `make test-crates` (`DB_TESTS_REQUIRED=1`, real Postgres) green.

> **2026-09-03 — #639 part C step 2a: a rider's login is bound to ONE rider id, decided by Postgres
> before the fact is recorded (#794, the `slug_reservations` copy job).** Fourth attempt at step 2 —
> the first three executors were killed by container restarts (two before any push, the third after
> pushing only the claim), so this one pushed after every green milestone. What landed, on PR
> [#846](https://github.com/TheCaptainCompany/captain-food/pull/846), still in draft under
> `HOLD: human`: `auth_subject_reservations` with a **composite** primary key
> `(principal_kind, auth_subject)` typed by `PrincipalKind` + `AuthSubject` — never the subject
> alone, because a rider who is also a customer holds two bindings and a subject-only key would bar
> a rider from ever becoming a restaurant member; `principal_id` typed by the kind exactly as
> `ScopeMembership.member_id` is; **no `released_at` and no `release` method** — stronger than the
> slug sibling, since freeing a revoked rider's login would let a later registration bind the same
> human to a NEW rider id and orphan their history. `register_rider` reserves BEFORE the
> `RiderRegistered` append; a lost insert is the new typed `RiderAuthSubjectAlreadyBound` (en/fr),
> wired into `RegisterRider`'s `throws`, pinned by `TestRiderAuthSubjectAlreadyBoundIsRejected`
> under the new rule `RiderAuthSubjectBoundOnce` — a real assertion only because the in-memory port
> is seeded with a FOREIGN holder under `"already-bound"` (the `SpecSlugReservations` sentinel
> convention). The DB-gated
> `two_concurrent_claims_of_one_login_bind_exactly_one_rider` races two `reserve`s on real Postgres
> (`INSERT … ON CONFLICT (principal_kind, auth_subject) DO NOTHING`), which the fake cannot stand in
> for: its mutex would pass a read-then-write implementation too. Migration `20260903060000`
> mirrors the generated DDL byte-for-byte; `REQUIRED_SCHEMA_VERSION` moves with it. The SQL emitter
> learned a composite key (two `pk: true` columns → one table-level `PRIMARY KEY (a, b)`; single-key
> tables are byte-identical). **That change also rewrote a second table**: the old artifact declared
> `mailbox_partitions` with `actor_type TEXT PRIMARY KEY` AND `partition SMALLINT PRIMARY KEY` — two
> inline primary keys, DDL Postgres refuses (*multiple primary keys for table*) — while the deployed
> `migrations/20260731063000_actor_mailbox_tables.sql:93` has always carried
> `PRIMARY KEY (actor_type, partition)`. The emitter change repairs that latent generator bug and
> re-aligns the artifact with the deployed DDL; **no migration is owed** for it. The port takes a `BoundPrincipal` witness enum (one `Rider` arm today)
> so the `(kind, id)` pair can never disagree — compiler first. **Fence report**: `CommandDeps`
> lives in `crates/infrastructure/src/inbox.rs` and `standalone.rs` constructs it, both fenced by
> the card; the minimum taken is one field, one match arm and one wiring line. **Build-order row 2
> of PROP-20260831-180622 is NOT yet marked landed** — 2b (the resolver, the §10 pair, the WS
> mirror, the `role_injection_gate` fix, the duplicate-`authRef` classification test) is the next
> dispatch, and NO RIDER CAN SIGN IN yet.

> **2026-09-01 — A link checker exists, and a broken citation can no longer ship silently.**
> Founder directive, 2026-09-01, verbatim: *"excellent point put in place this url checker that must
> be executed locally and enforced in the CI too"* — both halves, and both landed: `make link-check`
> locally, two pinned steps in CI's always-run `gate-scripts` job
> ([#837](https://github.com/TheCaptainCompany/captain-food/issues/837)). There was **no link
> checker anywhere** before this: nothing in the Makefile or any workflow validated a link, so
> `docs/**` — which *is* the operating model, since CLAUDE.md is an index whose authority is the
> topic file it links to and every ADR cites its neighbours — had been accumulating dead citations
> with nothing able to see them.
>
> **The number, with its method, because a link count is meaningless without one** (ADR-20260817-105845).
> The card carried `~25` as `UNVERIFIED input`. Measured with the shipped checker against the merge
> base **`43317168`**, in a clean checkout: **8,060 relative links across 451 markdown files, of
> which 124 were broken — 28 dangling paths and 96 dead fragments** (95 in
> `specs/generated/documentation.generated.md`, 1 in `specs/integrations/hubrise.md`). Method:
> relative link TARGETS (inline `[t](p)`, images, and link reference definitions) in the markdown `git ls-files --cached --others --exclude-standard -- '*.md' '*.markdown'` reports, resolved against the tree; fragments checked against github-slugger's algorithm plus explicit `<a id>` anchors. External URLs, footnote definitions and links inside fenced or indented code are NOT links for this purpose. **The corpus includes UNTRACKED files** (`--others`), so scratch markdown present at measurement time moves the figure -- which is why the number is quoted against a NAMED COMMIT measured in a CLEAN checkout.
>
> The first version of this entry said 8,045 / 130 / 102, and review round 1 refuted it: those were
> taken mid-change with scratch files present, not from one run against a named commit. Quoting a
> method that refutes your own figure is a sharper failure than quoting none, because the method
> invites the check.
>
> **The 95 + 27 were an emitter defect, and finding them was the checker's first real catch.** A
> test's `when:` is not always a command — 59 of this repo's tests are driven by an inbound
> integration event, which is the whole point of ADR-0004 — and a saga is documented as an `actor`,
> not an `entity`. Both were hardcoded kinds in `emit/docs.rs`, so the document linked to anchors it
> never defined. Fixed at the emitter and regenerated, never in the output; the HTML sibling carried
> the same bug and no checker would ever have seen it, since the scan is markdown-only.
>
> **Gated at ZERO, with no baseline file** — a baseline is a second thing to keep honest, and this
> repo has been bitten by exactly that. Two scope decisions are stated rather than implied:
> **external URL liveness is OUT** (a blocking gate whose verdict depends on a third party's uptime
> and rate limiter reds on honest work, which is the "trains readers to discount reds" instrument
> `tools/codegen-rs/src/tests.rs` has retracted five times over), and **anchors are IN** (the slug
> algorithm is deterministic and published; a citation landing in the right file at the wrong heading
> is the same silent nothing).
>
> **Compiler-first lands at level 3, and the argument is the one already recorded for `specs/**`
> YAML.** PROP-20260802-130500 §1's hierarchy ranks ways to stop *Rust code* naming something it
> should not, and it has no rung for a target written in prose: no newtype or sealed trait can make
> `[x](gone.md)` unspellable, because the compiler never sees the markdown. So "start at level 4"
> resolves to *level 3 is the ceiling here*, exactly as it did for the `reads:` wall
> ([ADR-20260812-214500](../adr/ADR-20260812-214500-a-read-target-is-declared-not-inferred-the-reads-ownership-wall.md)),
> and it is the case [ADR-20260803-234035](../adr/ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md)
> names in its own carve-out: non-Rust artifacts. Where a stronger instrument *was* available it was
> taken — the generated document now cannot contain a dead in-page link at all, because the emitter
> de-links any anchor it does not define.
>
> **Three vacuity guards, because a scanner that matches nothing passes** — a green over an empty
> corpus is clean, confident and meaningless, and that shipped twice in one session here. The corpus
> is derived from `git ls-files`, never a literal list, and an empty corpus, a corpus not containing
> `CLAUDE.md`, and a corpus yielding zero extracted links each exit 2 with their own message. All
> three were **observed red**, as were a planted broken link, the `ADR-` prefix trap (pre-2026-07-22
> ADRs have no prefix while every citation uses one — `ADR-0004-...` resolving to nothing is one of
> the 28), a `|| true` disarm of the CI step, and the selftest's own completeness counter.
>
> **Four false-positive classes were found while writing it, and every one had exactly one "fix"
> available to a reader who did not know better: damage a correct document.** Footnote definitions
> read as link reference definitions (20 false reds in one research dump — the majority of the first
> measured finding was the instrument); an indented code block in `docs/STATUS.md` showing a template
> destined for `docs/status/`; a list continuation that must *not* be swallowed by the fix for that;
> and an intraword `_` stripped as emphasis, which slugged `` `place_order` `` to `placeorder` and
> reported a correct ADR link broken. They are `T11`–`T13` and `T17`–`T18` of the selftest
> (`T10`, fenced code, is a fifth class; an earlier version of this sentence said `T10`–`T13`, which
> both miscounted and claimed coverage that did not exist — review round 1 showed the suite stayed
> green under a mutant for two of the four, because T12's fixture indented two spaces and only one
> intraword-underscore link exists in the whole tree. `T17`/`T18` are the cases that close them).


> **2026-09-01 — The founder's read-only command is renamed a second time: `/where` → `/whatsup`.**
> Founder verbatim, 2026-09-01: *"Instead of /where use /whatsup"*. A **preference, not a
> collision** — the reason `/status` was abandoned (Claude Code ships a built-in `/status`; skills
> resolve first-match-wins ahead of built-ins and dedup by file path, so a colliding skill shadows
> it **silently**) is untouched and stays the durable part of
> [ADR-20260831-204546](../adr/ADR-20260831-204546-the-founder-elects-user-invoked-commands-and-direct-question-is-a-fourth-carve-out.md),
> which gains a postscript so the history reads as **two deliberate namings rather than one confused
> one**. The name is his (ADR-20260810-011500), so this was executed, not put back to him as an
> option space.
>
> **What moved**: `.claude/skills/where/` → `.claude/skills/whatsup/` (the skill name IS the
> directory name), its `name:` frontmatter, description and every self-reference; the six-command
> section of [workflow.md](../claude/sessions/workflow.md); ADR-20260831-204546;
> [`docs/decisions/CMD-INVOKE.yaml`](../decisions/CMD-INVOKE.yaml);
> [DECISIONS.md §49](../proposals/DECISIONS.md). The other five skills carried **no** cross-reference
> to the old name — checked, not assumed.
>
> **What deliberately did NOT move**: the loop-budget ledger entry whose `note` records the earlier
> `/status -> /where` run, and every record quoting the founder's own earlier words. **A rename must
> never overwrite a verbatim quote** — and the naive form of this sweep bit once in this very run: a
> blanket replace of `` `/where` `` clobbered the historical name inside the sentence that had just
> been written to preserve it, in the same file, seconds later. Write the history sentence, then
> re-read it after the sweep.
>
> **Collision check, re-derived rather than trusted** (the recorded method, not a pinned value):
> `readlink -f "$(which claude)"` → `/opt/claude-code/bin/claude`, an ELF binary — *not* the
> `node_modules` JS bundle, which does not execute. The sharper form of the check earned here: a
> built-in's name is stored in that binary as a **plain string**, so a name that appears **nowhere**
> in it cannot be one. `whatsup` has **zero** occurrences, case-insensitive, substring or exact ⇒
> free. Contrast `status`, `stats`, `skills`, `agents`, `todos`, `review`, `security-review`, all
> present. Note the trap this sharpens: an *exact-token* hit is **not** evidence of a built-in —
> common words are interned and shared, and all four `status` hits sit in unrelated data. The
> negative is the reliable direction.

> **2026-08-31 — The person gets a name: `PrincipalKind`, `MemberId` and `AuthSubject` land in the
> kernel, and `UserType` deliberately does NOT widen ([#639](https://github.com/TheCaptainCompany/captain-food/issues/639)
> part C **step 1 of seven**, PR [#835](https://github.com/TheCaptainCompany/captain-food/pull/835),
> `HOLD: human`, [ADR-20260831-220559](../adr/ADR-20260831-220559-the-person-is-a-principalkind-not-an-eighth-usertype.md)).**
> Closes the [PRINCIPALS-MEMBER](../decisions/PRINCIPALS-MEMBER.yaml) register row, which gated this
> step — the row's answer *is* the step's design, so they closed together.
>
> **The answer was verified, not adopted.** The assembled reading came into the run as four claims and
> two of them carried the weight, so both were checked against the corpus:
> (1) *`UserType` must not widen* — confirmed **mechanically**, and this is the whole load-bearing
> finding: `tools/codegen-rs/src/emit/bins.rs`'s `user_type_roles` reads that enum and emits one
> gateway bin plus one `/{path}/graphql` route **per member**, matched by the seven-arm `RequestRole`
> table in `crates/server/src/graphql/acl.rs`. Adding `MEMBER` there would have conjured an eighth URL
> surface and an eighth gateway as a side effect of naming a person. `PrincipalKind` is a NEW scalar
> with no stored history, so `MEMBER` costs no upcaster and no re-attribution.
> (2) *`requires.acting` has one consumer and it is a comment* — confirmed: **zero** `requires:` blocks
> exist in any `specs/*/api.yaml` (the single grep hit, `specs/common/api.yaml:52`, is prose about the
> unrelated `@auth(requires: […])` directive). So the semantics are stated and no emitter is built;
> #636 owns that. Neither verification contradicted the assembled answer — both narrowed it.
>
> **The `RESTAURANT` cost window was declined, not spent.** The row noted that no `domain_events` row
> was ever authored by a `RESTAURANT` principal (the sole claim writer hardcodes `CUSTOMER`), which
> would have made renaming the stored token cheap *today* — marked `UNVERIFIED input` pending a
> production `SELECT user_type, count(*)`. That query was never run and is no longer needed: the
> decision declines the rename on principle (ADR-0041, immutable log), so nothing rests on the figure.
> A number that licenses nothing does not need verifying.
>
> **The card's "four `authRef` sites" was three short, and the missing three are load-bearing.**
> `specs/services.yaml` holds three more (`verify_phone_otp` output, `verify_email_token` output,
> `stamp_customer_claim` input), and the identity service's output flows straight into the retyped
> event fields at `crates/application/src/commands.rs:3432-3518` — so retyping the four without the
> three does not compile. Seven sites moved `ExternalReference` -> `AuthSubject`. The old typing had a
> customer's login credential and a HubRise menu-import key (`'MARGHERITA'`, `'CAT-PIZZAS'`) as one
> type in the shared kernel; they are now distinct newtypes and `rustc` refuses the substitution —
> compiler-first, with no gate written.
>
> **Nothing stored moved, and that is proven rather than asserted**: regenerating gave a **zero-byte
> diff** in `schema.generated.sql`, `views.generated.sql`, `security*.generated.sql` and
> `databases.generated.json` (`type: string` emits `TEXT` irrespective of the scalar's name), and the
> GraphQL diff is purely additive — two scalars and one enum **declared**, not one field retyped,
> because no API field binds `authRef`. No migration, no backfill, no replay.
>
> **One thing deliberately left undone, and named so it is not mistaken for finished**: the
> `by_auth_ref` read port still takes an `ExternalReference`, so the identity bridge is the one place
> the old confusion survives. Retyping it reaches the codegen emitter
> (`tools/codegen-rs/src/emit/server_graphql.rs:703` hardcodes the scalar in the emitted `me`
> resolver) and `crates/infrastructure/src/mailbox/handler.rs:400`, which was **fenced to a concurrent
> session** by the dispatch. It is now *visible* instead of invisible — the fake repository and the
> projection test each name both scalars with a comment saying why — and is written up as step 1b.
> `PROP-20260831-180622`'s four Concerns remain unchecked; this step discharges none of them.
>
> **The independent reviewer pass found no code defect and two false statements in the records** —
> both of the class that outlives the PR, so both were corrected in the same PR rather than filed.
> (1) `PROP-20260831-180622` is a LIVING document (ADR-20260801-020000) that this step falsified in
> **six** places, not the five the re-presentation card listed: the `Related` block still called
> [PRINCIPALS-MEMBER](../decisions/PRINCIPALS-MEMBER.yaml) *"open ... not closed by it"*, the
> `kernel-change` Concern still said the row *"is OPEN"*, and four sites still said *"four `authRef`
> sites"*. All rewritten to cite the closure and the seven; **the Concern's box stays `- [ ]`** —
> closing the row it depended on is not discharging it, and `requires.acting` is still built by
> nothing. (2) The ADR's own follow-up said step 1b was *"Three edits"*. It is **ten**: retyping a
> trait parameter forces every `impl` signature (Rust requires an exact match) plus every
> hand-written caller — one trait decl, five impls, the `me` emitter, and three callers. The omitted
> one that matters is `crates/server/src/auth.rs:2181`, the gated
> `RESOLVE_CUSTOMER_IDENTITY_FROM_POSTGRES` sub->domain resolver: the bridge-at-the-edge the new
> `AuthSubject` docstring cites ADR-20260818-004646 for, neither fenced nor generated. A derived
> number stated without its antecedents is exactly the defect
> [ADR-20260817-105845](../adr/ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)
> names, and this is the second time in one chunk a "four"/"three" count reached a record unverified.
> Step 1b, plus three findings the pass raised about the bridge (no step-1b marker on the trait decl;
> the `.0`-against-`.0` comparison that is the one place a confusion could still pass silently; and
> `MemberId`/`PrincipalKind` unreferenced with **no `scalar-unused` validator rule**, so the ADR's own
> Negative is enforced by nothing), are filed as
> [#836](https://github.com/TheCaptainCompany/captain-food/issues/836).

> **2026-08-31 — The checkout button now states the OBLIGATION TO PAY rather than an amount, and
> the pre-order total recap is no longer declared collapsible
> ([#817](https://github.com/TheCaptainCompany/captain-food/issues/817), PR
> [#833](https://github.com/TheCaptainCompany/captain-food/pull/833), `HOLD: human` at draft).**
> C. conso. **L221-14** (transposing **CRD 2011/83 Art. 8(2)**) wants an unambiguous obligation-to-pay
> mention on the button placed immediately before a distance order, next to a clear and legible recap
> of the total; the sanction is **not a fine but that the consumer is not bound**, so a defect here
> makes shipped, paid, rider-delivered orders voidable. `fr` now carries the statutory safe-harbour
> formula *"Commander avec obligation de paiement — {total}"* and `en` the directive's own *"Order
> with obligation to pay — {total}"*; the checkout `order_summary` section drops `collapsible: true`.
> The requirement is grade (a); whether this wording **satisfies** it is **QT-4** on the counsel list
> ([BRIEF-20260831](../legal/BRIEF-20260831-repricing-and-price-quote-counsel-packet.md)) and no
> counsel is engaged, so this is deliberate **over-compliance pending that answer** and not a legal
> conclusion (ADR-20260812-143619).
>
> **Two things the issue did not know, both found by going to the runtime rather than the
> declaration.** (1) **`checkout.place_order` was resolved by nothing.** The pay button rendered the
> hard-coded literal `"Place order - "{total}` (`crates/web/src/checkout.rs`), so **every French
> customer was shown an English button**, and fixing only the DSL copy would have changed nothing a
> customer sees — the #780 class exactly. (Stated narrowly on purpose: the rest of that file *does*
> consult the catalog through its `t` helper; it was this key, plus four other hard-coded literals,
> that were unwired. The generalised claim "the runtime does not read the catalog" is false.)
> (2) `collapsible: true` was read by **nothing** — not the codegen, not the renderer — and the
> hand-written runtime always rendered the recap expanded; so the flag was never the guarantee, and
> the guarantee now lives in a runtime assertion.
>
> **The fix is single-source, and that is worth claiming at its real height and no higher: level 3,
> not compiler-first level 4** (PROP-20260802-130500 §1). The button resolves the key from the
> generated catalog, so there is one string rather than two a parity test must keep equal — but the
> key is a hand-authored `&str`, `"checkout.place_ordr"` compiles and renders the fail-visible
> `[key]` marker, and nothing proves code→catalog at build time. Level 4 would need generated key
> constants and is not this change. What backs it instead: both tests were **mutation-checked**
> (revert the catalog copy / restore the literal / wrap the recap in `<details>`), all red.
>
> **Adjacent, NOT fixed here and needing an issue**: the recap's own text is hard-coded English in
> the same runtime (`format!("{} items - {}", ...)` plus a literal `" from "` and `<h1>"Checkout"</h1>`),
> so a French customer reads *"2 items - 23,50 EUR from Chez Marcel"*. Same class as the button
> defect, different fix, and `PROP-20260831-134539` **§4 UC4** shows the composed target for that
> screen (§11 slice 5 is *"Divergence policy and the disclosure UI"* and says nothing about the pay
> button or the recap — citing it for this would overstate the source).
>
> **Review round 1 (FAIL, 2 blocking) found no runtime defect and two RECORDS defects, which is the
> finding worth keeping.** The counsel packet, both briefs and the PROP still asked counsel QT-4
> about `"Commander — 23,50 €"` and a collapsible summary — strings this change had just deleted —
> and a packet built to be handed to a French practitioner would have returned an answer about a
> control that no longer ships. Three durable records also asserted the legal question was *closed*
> ("removes the question", "carries the formula **verbatim**" when the shipped label is the formula
> **plus a suffix**, and a rationale that read the article), while the PR body said the judgement had
> deliberately not been made. **A change to a legal surface is not done when the code is right: the
> question the packet asks has to move with it**, and the shipped label's suffix is now stated as the
> live QT-4 question, along with the fact that the total renders `23,50 EUR` and not `23,50 €`.

> **2026-08-31 — two environment defects that taxed every dispatch are fixed, and the second one was
> already written down two weeks ago, which is the finding worth keeping.**
> [#830](https://github.com/TheCaptainCompany/captain-food/issues/830) /
> [#831](https://github.com/TheCaptainCompany/captain-food/pull/831), no `specs/**` (so no SPEC-LOG
> sentence, no `make warning-baseline`).
>
> **D1 — the DB gate passed on a dead database.** `crates/db_test_gate` (#474) decides on whether
> `DATABASE_URL` is *set*, never on whether the server answers, and it cannot: it runs inside
> libtest, once per suite, after the workspace is already built. So a `DATABASE_URL` pointing at a
> stopped Postgres failed every DB-gated suite at once, minutes in, with connection errors that read
> as regressions in the diff under test (~12 min, measured 2026-08-30) — while the skip receipt
> `target/db-test-skips.log` stayed **empty**, because nothing *skipped*. Grepping a run for
> `DB-GATED SUITES SKIPPED` returned the same answer on a live database and on a dead one: it proves
> nothing was skipped and says nothing about whether anything *ran*. Exactly CLAUDE.md's named class
> — *a monitoring path that can only fire when a signal ARRIVES*. `tools/db-preflight.sh` is the
> dead-man's-switch, run FIRST by `make test-crates`: it fails before the build when the database is
> unreachable (0.06s, zero compilation) and prints a **positive** line when it is reachable, so the
> evidence is two-sided. The reportable claim is now the triple — `DB PRE-FLIGHT OK` + empty receipt
> + exit 0 — and never one third of it. Pinned by
> `the_db_preflight_guards_test_crates_and_can_actually_fail`, proven red against three planted
> defects (call deleted from the recipe; failure branch exiting 0; redaction removed, which leaked a
> password) and against a genuinely stopped Postgres.
>
> **D2 — the documented closing step of every dispatch was unexecutable, and saying so again would
> not have helped.** No executor session can mark a PR ready or arm auto-merge: both are GraphQL-only
> mutations, the endpoint answers **403** ("only the pinned set of PR-review operations is served"),
> `gh` is not installed, and REST has no auto-merge endpoint and ignores `"draft": false` while
> returning 200. **But `docs/claude/sessions/workflow.md` has recorded all of that — and the correct
> conclusion, that the flip is a coordinator action — since 2026-08-17 (#623), and three executor
> runs still paid ~8 minutes each rediscovering it on 2026-08-30/31.** The reason is the lesson:
> `.claude/agents/executor.md` step 7 still *told the executor to do it*, and a charter is loaded on
> every run while a topic file is loaded only when something suggests it — nothing did, because step
> 7 read as an ordinary instruction right up to the 403. **When an operational note contradicts a
> binding instruction, the note loses silently, every time.** So the fix went to the binding text,
> not to a fourth note:
> [ADR-20260831-183847](../adr/ADR-20260831-183847-the-ready-flip-is-the-coordinators-step-and-always-was.md)
> records that the ready flip is the coordinator's step, restoring
> [ADR-20260810-011500](../adr/ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md)
> §2 — which had assigned "GitHub mechanics … ready + auto-merge" to the coordinator all along, and
> which ADR-20260815-115220 contradicted without noticing while rewriting the charter in the
> executor's voice. That ADR settled *when* the step is taken, never *who* takes it.
> **Auto-merge-on-green survives intact**: what converges is the executor's ending (always draft),
> what does not is the merge condition (armed by default; withheld under `HOLD: human`) — a
> simplification of the handover, not a loss of the property.
>
> `.claude/settings.json`'s 15 `Bash(gh …)` + 13 `PowerShell(gh …)` entries were **kept**, not
> deleted: a permission is a conditional, not a claim that the binary exists, and the PowerShell half
> says a Windows host uses this repo where `gh` is the normal way in. The fact is recorded in a
> `_comment_gh` key instead.
>
> **The review then found the same defect one level up, in this very change.** The first pass fixed
> the two files it was already editing, wrote *"both binding sites … were corrected"*, and recorded
> "no follow-up required" — while **four more binding sites** sat uncorrected, findable in one
> `git grep`: `docs/STATUS.md` (loads every run, second only to CLAUDE.md), `docs/BACKLOG.md` (the
> binding method), two in `evidence.md` — one defining the executor's DONE as *"PR armed and
> reported"*, i.e. the impossible operation — and a **second section of `workflow.md`**, ~200 lines
> below the first. **An author sweeps the files they are already editing and calls it complete**, so
> the count in a completeness claim is the thing to distrust; the ADR now carries the grep instead of
> the word "both", and keeps the false claim on the record rather than deleting it.
>
> **And the quiet filter was deleting the new evidence.** `QUIET_KEEP` dropped `DB PRE-FLIGHT OK`,
> `UNAVAILABLE` and both follow-on lines; `FAILED` and `SKIPPED` survived only by accident of
> alternates meant for other tools, and the 50-line tail cannot recover the rest. So on
> `make test-quiet` / `make rust-quiet` — which CLAUDE.md names as how token-bound sessions run gates
> — a container without `postgresql-client` would show no pre-flight line and no skip receipt, and a
> reader would write "empty receipt + exit 0 ⇒ DB suites ran": **the exact over-read this change
> closed, restored on the recommended path**, with a DECLARED degraded mode turned silent. Fixed with
> `^test-crates:|PRE-FLIGHT` and pinned. The axis to watch here is the **false negative** on the
> positive line, not the false positive the brief anticipated.

> **2026-08-31 — #639 part C has a proposal, and writing it corrected the design's headline claim:
> the membership key `vernon` proposed is not the derivation `ScopeMembership` already uses, and
> adopting it as stated would put the auth subject into the read-authorization index.**
> [PROP-20260831-180622](../proposals/PROP-20260831-180622-staff-authentication-the-roster-the-invitation-and-the-door.md)
> (docs-only: no `specs/**`, no code, so no SPEC-LOG sentence). `ScopeMembership.member_id`'s own
> column note reads *"The DOMAIN id … **never the auth subject** — the sub→domain bridge happens once
> per request at the edge"* (`projection_tables.yaml:1181`), so
> `UUIDv5(scopeType|scopeId|memberType|authSubject)` is the same **shape** with a different **value**:
> the "projection becomes a rename rather than a join" prize is real but unreachable that way, and
> the route to it is a **person id** (`MemberId`) bridged from the subject, which restores a domain
> id in the fourth term. Recorded because the claim had already been carried into a dispatch card as
> established fact.
>
> **The second thing the proposal had to state before designing anything**: a `PUBLIC` operation is
> **not reachable from a staff surface** — `Surface::role()` returns one role per surface
> (`crates/web/src/router.rs:57`), both staff surfaces are 9/9 and 2/2 `requires_auth: true`, and a
> control bound to an operation the client's role excludes is `SkipReason::RoleRefused`, **skipped
> silently, not 403'd loudly**. So the staff sign-in door needs a renderer capability that does not
> exist. Three forks are presented with both costs and none pre-resolved (invitation/membership
> identity, check-or-lock for an act on another aggregate, where the door lives); four Concerns are
> registered, which mechanically block `Approved`, the sharpest being that
> `limit_depth`/`limit_complexity` occur **nowhere** in the tree while part C adds unauthenticated
> write entry points to the public graph. [PRINCIPALS-MEMBER](../decisions/PRINCIPALS-MEMBER.yaml)
> stays open and is carried as a declared dependency, not closed.

> **2026-08-31 — the support address is `support@captain.food` with no voice leg, counsel waits for
> production, and that sequencing rests on two things that are not currently true.** Founder answers
> ([SUPPORT-CONTACT](../decisions/SUPPORT-CONTACT.yaml) closed;
> [PUBLISH-PRECONDITIONS](../decisions/PUBLISH-PRECONDITIONS.yaml) timing recorded, row stays open
> and counsel-owned). **No voice leg** decides a screen: the rider handback carries an in-app report,
> not a call button, because a control that renders and does nothing is worse than no control.
> **The key does not exist** — `grep -rn SUPPORT_CONTACT specs/ crates/` returns nothing, so
> "required key with no default" was a recorded DESIGN and not a live constraint; the string lands
> when #792's screen is built. I had asserted it as live and it was not.
>
> **Counsel after production is coherent, and it is safe only if production ships with no publicly
> shown crawled listing** — this row gates the publish switch and nothing else, and a Tours partner
> who signed up is not a crawled listing. Two things must hold, both ours and neither needing
> counsel: the marketplace must default to partner-only (`restaurant.rs:83` filters `listing_status`
> **only** when `orderable_only == Some(true)`, and the public `restaurants` query carries no guard,
> so today the default SHOWS non-partner rows); and `RUN_SIRENE_WORKER` must actually be off — its
> own declaration **contradicts itself**, prose reading *"STOPPED since 2026-07-28"* while
> `deploy.production` is `"true"`. The recorded pause and the deployed value disagree and nobody has
> reconciled them. If either fails, the obligations attach before counsel looks, which is the
> opposite of what was chosen.

Current state: [`../STATUS.md`](../STATUS.md).

> **2026-08-31 — the founder's six invoked commands are built, user-invoked only** (`.claude/skills/**`
> plus one workflow section; no `specs/**`, no code, no SPEC-LOG sentence).
> Founder directive 2026-08-31, choosing a **user-invoked** approach *"to avoid any risk"*: he named
> `/direct-question`, `/mob-question` and `/work`, then approved `/decision`, `/status` and
> `/correct` — renaming `/decide` → `/decision` because *decide* reads as an instruction to the
> coordinator while *decision* names **the artifact he is recording**. Six skills under
> `.claude/skills/<name>/SKILL.md`, each carrying its procedure and its limits, in
> [#819 "Six founder-invoked slash commands"](https://github.com/TheCaptainCompany/captain-food/issues/819)
> / [#820](https://github.com/TheCaptainCompany/captain-food/pull/820).
> **The rule the set exists to protect**: `/direct-question` skips the **mob**, never the **register
> check** — the hook caught **none** of the coordinator's nine catalogued failures (#9 was caught
> by the check itself, the procedure being run rather than the gate firing), the rest being answer-
> or question-shaped, and the
> `PreToolUse` hook gates `AskUserQuestion` and `Agent`, never a prose answer, so a direct answer is
> where the check is least enforced and most needed. Both question commands carry an **escalation
> duty** in the skill text: a controlling record the question appears to contradict, or a `HOLD:
> human`-axis subject, means say so and fan out anyway.
> **`disable-model-invocation` was verified before being relied on**, not assumed: parsed by the
> `SKILL.md` loader beside `allowed-tools`/`user-invocable` and enforced at the Skill-tool gate
> (`errorCode 4`, guarded by `disableModelInvocation && !userTypedThisTurn`). **The verification
> itself carried a trap worth more than the result**: the container has TWO installs — a JS bundle at
> `/opt/node22/lib/node_modules/@anthropic-ai/claude-code` (**2.1.42**) and the **native binary**
> `/opt/claude-code/bin/claude` that `which claude` actually resolves to. The JS bundle does not run,
> so three separate version citations this session were about the wrong artifact. And the runtime is a
> **moving target**: within this one session the symlink was repointed and the binary rebuilt under it,
> `claude --version` going **2.1.251 → 2.1.252**. Rule now in `workflow.md`: **do not pin the version
> in prose** — record the method (`readlink -f "$(which claude)"`, then `strings` on that artifact)
> and re-derive at the moment of use.
> Two card citations that did **not** check out and are corrected here: the pre-`ADR-` ADRs are
> filed **without** the prefix (`docs/adr/20260720-233000-…`, `…/20260721-042018-…`), so a link built
> as `ADR-20260720-233000-*` resolves to nothing; and `.claude/skills/coordinator-register-check/`
> exists only from `875e5ab2`, which a stale checkout does not have.
> **The blocking finding of review round 1, and the rule it earned**: `/direct-question` is a
> **fourth carve-out** to
> [ADR-20260812-143619](../adr/ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md)
> — and a different KIND from the three there, which are class-based and each lens-asked, whereas
> this one is **founder-elected per message**. The branch shipped it with no ADR and no register row,
> i.e. the PR that wrote `.claude/skills/decision/SKILL.md`'s reversal check **reproduced the defect
> that skill exists to stop**. Now recorded as
> [ADR-20260831-204546](../adr/ADR-20260831-204546-the-founder-elects-user-invoked-commands-and-direct-question-is-a-fourth-carve-out.md)
> + row `CMD-INVOKE` + [DECISIONS §49](../proposals/DECISIONS.md), with the forward banner on the
> amended ADR and CLAUDE.md's `Carve-outs:` bullet updated from three to four. **Rule earned: a
> register check searches the SUBSTANCE THE CHANGE AMENDS, not the MECHANISM IT IS BUILT FROM** —
> #819's trail searched `slash command`, `user-invoked`, `disable-model-invocation` and correctly
> found nothing, because every term was an implementation noun and none was *mob*, *fan-out* or
> *carve-out*. A negative trail is never self-certifying.
> **Roster note**: the class was assessed on the artifact (prose skill files ⇒ reversible) rather
> than on the decision the artifact carried (amending a founder rule), so **no lens read it**;
> ADR-20260831-204546's `Consulted:` block records the roster as NOT ASKED rather than inventing
> lines, and the second-order question stays open as a `/mob-question`.
> **Second founder decision the same day: `/status` → `/where`.** Claude Code ships a built-in
> `/status`, skills resolve **before** built-ins on a first-match-wins scan, and dedup is by **file
> path, never by name** — so a colliding skill shadows the built-in **silently**, with no detection
> and no warning, and even ours winning rests on array order in a vendor bundle that can reorder on
> upgrade. Losing the panel you reach for when the session itself looks wrong is the worse trade.
> `/status` was the **only** collision among the six; `/where` verified free on the running binary.
> **Rule earned: check a command name against the built-ins before writing the skill** (`status`,
> `review`, `security-review`, `stats`, `skills`, `agents`, `todos`).
> Reversibility class as dispatched **reversible**; `HOLD: human` all the same, because this is the
> coordinator's own routing surface.
> **2026-08-31 — the oversell hole on the money path is CLOSED: checkout re-derives orderability
> instead of trusting cart-edit time**
> ([#823](https://github.com/TheCaptainCompany/captain-food/issues/823) / PR
> [#824](https://github.com/TheCaptainCompany/captain-food/pull/824), slice 1 of
> [PROP-20260831-134539](../proposals/PROP-20260831-134539-priced-quote-token.md) §11 — the one
> slice that is not `HOLD: human`; approved by the founder 2026-08-31, recorded in
> [ADR-20260831-165146](../adr/ADR-20260831-165146-the-quote-backstop-is-thirty-minutes-and-the-priced-quote-token-is-approved.md),
> which is the recorded approval covering this change's `specs/**` edit).
> `require_orderable_line` — the availability-and-stock guard — existed, was correct, and was called
> from the two cart-edit handlers **only**. It was never called at checkout. A dish added at 19:50,
> 86'd by the kitchen at 20:20 and paid for at 20:40 **was accepted**: pricing fails closed only on
> a line that LEFT the catalog, so an offer merely flagged `UNAVAILABLE` — or stock-tracked and now
> below the line quantity — still resolved a price and sailed through to a real Stripe intent. This
> is the failure shape [#780](https://github.com/TheCaptainCompany/captain-food/issues/780) already
> named (a mechanism written and never connected), on the money path, at peak.
> **The two reds were proved before the fix**, and their panic message is the finding stated by the
> test runner itself: *"the spec expects a typed rejection: PaymentRequestOutput { payment_intent_id:
> PaymentIntentId(\"pi_123\") … }"* — i.e. the checkout did not merely accept, it minted the intent.
> **The third test is the false-positive floor and is GREEN on both sides by construction**: an
> offer that tracks no stock must never block, because untracked is not zero and treating it as zero
> would refuse every order for every restaurant that does not count portions. A card asking for all
> three to be RED on `main` is asking the impossible of that one — an acceptance assertion cannot
> fail where nothing blocks — and that is worth recording, because the same shape will recur every
> time a guard ships with its floor.
> **Position is the whole safety argument** and is documented at the call site: AFTER `price_cart`,
> so a line with no live price keeps its own `PriceUnresolvable` code rather than degrading to
> `OfferNotFound`; and BEFORE the Stripe call and the store-credit spend, so a refusal can never
> strand a real PaymentIntent or consume goodwill. No new error was declared — `OfferUnavailable`
> and `InsufficientStock` already existed with en/fr messages and are already surfaced by the
> checkout screen's error toast.
> **`OutsideDeliveryArea` stays a `TODO(invariant)`**: it needs a delivery-area policy port that
> does not exist in any read model, so only the half actually discharged was retired.

> **2026-08-31 — the founder decided the quote's backstop and approved the design: 30 minutes, and
> build it slice 1 first** (docs/records only: one register row closed, one proposal approved, one
> ADR, the register prose row updated; no `specs/**`, no code, so no SPEC-LOG sentence and no
> warning-baseline refresh is owed).
> Two answers, both put through `AskUserQuestion` with a register-check trail, options, trade-offs
> and a recommendation; both recorded in
> [ADR-20260831-165146](../adr/ADR-20260831-165146-the-quote-backstop-is-thirty-minutes-and-the-priced-quote-token-is-approved.md).
> **(1) `QUOTE-STALENESS` — N = 30 MINUTES, AS A BACKSTOP ONLY; M IS DROPPED.** Verbatim option
> label: *"30 minutes (recommended)"* — the `business` lens's figure, taken unchanged with its
> stated derivation: the **p99 of the cart-to-pay leg with the mandatory SCA/3DS bank-app detour in
> it**, **not** a risk setting. It essentially never fires on a live session. **Why an N exists at
> all, and it is the load-bearing context**: carts never expire
> (`specs/ordering/actors.yaml:15` — re-verified at `9cd15c75`; the dispatch carried it as an
> `UNVERIFIED input` and it checks out), so **N is the only clock on the whole cart**. The rejected
> options are in the row with their costs — shorter (~5 min) fires on ordinary Friday-night sessions
> and pays conversion on **correct** sessions; longer lets a quote outlive the service state it was
> priced in; divergence-gated with no backstop leaves an unbounded-age quote honourable. **The
> caveat survived the closure**: contract **C1** `quote_age_seconds` does not exist, so 30 is
> **evidence-deferred** (ADR-20260808-144738 decision 5) and is re-derived from the observed p99
> after the first peak — the row says what would change it.
> **(2) [PROP-20260831-134539](../proposals/PROP-20260831-134539-priced-quote-token.md) is
> APPROVED.** Verbatim: *"Approve — build it, slice 1 first"*. Slice 1 (HEAD orderability at
> checkout) was dispatched in parallel.
> **The three surviving `Concerns` were re-expressed, not deleted** — an unchecked entry
> mechanically blocks `Approved`, and the validator's own message says to resolve it by checking it
> with a one-line resolution, never by deleting it. One is **genuinely discharged** (the N). The
> other two were never approval gates and are now conditions on the PR that can satisfy them: the
> **non-additive `PlaceOrder` change** is a **slice-4 gate** (`HOLD: human`, team-reviewed —
> [ADR-20260815-134655](../adr/ADR-20260815-134655-the-team-merges-its-own-work-no-pr-waits-on-founder-review.md):
> no PR waits on founder review, so an approval could neither discharge it nor be blocked by it),
> and the **as-of fold's peak cost being a projection rather than a measurement** is a **slice-2
> Done-when** — a statement about the absence of code cannot become false before the code exists, so
> as written it blocked `Approved` **forever**. Both are now written into §11 at the slice they bind.
> `make validate` accepts the result at **0 errors**, so the approval got past
> `proposal-approved-unresolved-concern` honestly.
> **The reversal stays flagged.** The proposal reverses
> [ADR-20260810-112836](../adr/ADR-20260810-112836-cart-priced-live-on-read.md) **§2** in part — the
> freeze locus moves from commitment to quote time, and the enforcement clause naming the
> `expectedTotal` equality check is replaced outright — and that went unflagged in two records until
> today. The header `Reverses in part` line and §2.4 are intact, and the approving ADR re-states it
> in its own §4, because approval is exactly when a reversal gets re-buried.

> **2026-08-31 — the weekly cap did not reliably measure: one timer slot for N concurrent runs, a
> receipt naming the wrong branch, and a step 8 pointing at a file nothing writes**
> ([#821](https://github.com/TheCaptainCompany/captain-food/issues/821) "loop-budget: the weekly cap
> under-counts under concurrency", PR [#822](https://github.com/TheCaptainCompany/captain-food/pull/822),
> `HOLD: human`). The cap is the founder's governance control on autonomous loops (ADR-0014), and it
> failed **worst exactly when the session was most parallel**. **D1**: the running timer is one file in
> the git **common dir** — `git worktree` does *not* isolate it — so `stop` billed whatever it found.
> The W36 ledger records the collision twice: a segment noting *"a concurrent session in this shared
> checkout closed the timer I inherited at 12:05:26Z"*, and a **33.3-minute unbilled remainder** after
> a `stop` billed 3.2 minutes of a ~39-minute run **and printed success**. A silent under-count is
> worse than a refusal: the executor that trusts the output records the wrong number. Fixed
> **structurally**, not by a comparison — a run has an owner id (`--run` > `$LOOP_BUDGET_RUN_ID` >
> `$CLAUDE_CODE_SESSION_ID` > none) and the timer **file name carries it**, so another run's timer is
> *unaddressable* rather than merely detected (the nearest shell reaches to compiler-first,
> ADR-20260803-234035; PROP-20260802-130500 §1 caps a shell binding at level 3). Two consequences on
> purpose: **concurrent runs are now normal** — each opens its own timer and bills its own real time,
> so no second session is pushed into estimating with `--elapsed` — and a `stop` that cannot prove
> ownership **refuses and names whose timer it found**. `--elapsed-seconds` and `reset` no longer
> delete another run's live timer; both used to, so the escape hatch the tool *recommends on a
> refusal* was itself the weapon. **D2**: the receipt stamped the branch of the checkout `stop` ran
> from, not the branch the run was on (live instance: `2026-W36/20260831T142143Z-0568abb8.json` says
> `main` while its note describes `819-` work). The branch is now captured at `start`. The defective
> receipt is **not** retro-edited — the ledger is append-only (ADR-20260812-011057). **D3**: the
> executor protocol's step 8 said commit `.claude/loop-budget.json`, which **nothing writes** — an
> executor following the documented protocol committed nothing and left its run unbilled, a
> systematic under-count by exactly the people who follow the protocol. Step 8 now names the ledger
> file, and the stale siblings in the continuous-development proposal and the Makefile go with it.
> **41 new selftest cases (58 → 99), of which 24 are RED against `main`** — measured, not recalled:
> revert every file this PR touches except the selftest, run the suite. The other **17 pass against
> `main` too** (`7d` `7g` `7i` `10a` `10e` `10f` `10h` `10k`–`10p` `10u` `12a` `13h` `13i`): they are
> characterization cases pinning behaviour this change did not alter, and must NOT be read as
> regression-proven here — weaken one and no red follows. Four more (`13a` `13d` `13e` `13g`) were
> already green when written, because the run-id keying committed earlier in this same PR had
> delivered them: a good fact about the design, recorded because "observed RED first" is not a
> universal and must never be asserted as one. (The 15:11 receipt bakes in the earlier, wrong
> figure of 30; the ledger is append-only, so that divergence stands as history.)
> `make validate` 0 errors, `check-drift` clean, `hooks-test` green armed. Found in passing and
> now commented where the trap lives: **`make -n budgeted-loop` is not a dry run** — GNU make
> executes recipe lines containing `$(MAKE)` even under `-n`, so it opens a timer and bills a
> segment (two 0-second receipts in this commit are its output, kept rather than deleted because
> hook-written budget state is never hand-edited).
> **Addendum, on evidence arriving mid-run**: the contention is the **fourth** distorted segment of
> the day, not the two first named — `20260831T165727Z-de76c595.json` (10.3 m, `quote-decisions-20260831`)
> records a run that hit **exit 3** at `start` because a timer opened **70 s earlier on `main`** was
> still open, and reconstructed ownership by hand before billing "as mine". Keying the timer by
> **worktree** (`--git-dir`) was proposed as a smaller fix and **rejected**: it re-creates
> ADR-20260812-011057's *failure 1* (six checkouts, six simultaneous totals — the ADR chose
> `--git-common-dir` precisely so `start` and `stop` are one timer whatever checkout each ran in),
> and it partitions on the wrong noun, since what is billed is a **run**, not a directory — one run
> spans worktrees, one worktree hosts many runs, and at least one observed collision was a sibling
> session **in the same checkout**, which worktree keying cannot see. Run-id keying delivers
> everything worktree keying offered, in any topology, and makes a mismatch **loud** rather than
> merely rarer. Folded in from the same evidence: **`exit 2` and `exit 3` mean opposite things**, so
> every exit-3 path now prints `(exit 3 = INTEGRITY, not budget exhaustion …)` with the week's state,
> and every refusal hands over the whole tuple — started-at, branch, run id **and pid** — which is
> exactly what that executor reconstructed by hand.

> **2026-08-31 — the repricing obligation map is IN THE REPO, and the lens return that never landed
> is the finding** (docs-only: one new legal brief, the standing counsel list extended, one proposal
> `Concerns` entry discharged; no `specs/**`, no code, no SPEC-LOG sentence owed).
> [BRIEF-20260831 "Repricing and the priced quote token: the obligation map"](../legal/BRIEF-20260831-repricing-and-price-quote-counsel-packet.md)
> records the `legal-specialist` lens's return of 2026-08-31 — **relayed by the coordinator from a
> return that was not otherwise persisted**, which is the second occurrence in two weeks of the
> defect [BRIEF-20260818](../legal/BRIEF-20260818-counsel-packet-and-self-answer-triage.md) already
> records (a packet summarised into a record and never landed). Its cost was concrete: the executor
> writing
> [PROP-20260831-134539](../proposals/PROP-20260831-134539-priced-quote-token.md) was handed the
> labels `B1–B5` / `QT-1…QT-10` but not their text, **correctly refused to reproduce them from
> memory**, and left an unchecked `Concerns` entry that mechanically blocked `Approved`. That entry
> is now **discharged**, and §10 of the new brief reconciles `L1–L7` against `QT`/`B` row by row —
> cross-referenced, not competing. **Every `L` row survived.**
> **The load-bearing conclusion**: the binding price is the one displayed at the **confirming
> click**, not the price at restaurant acceptance — the storefront-as-invitation-to-treat reading is
> not freely available, because the design *looks* like an *offre* (pay button, hold, confirmation,
> no customer cancel at `PENDING`), the CGU term that would buy it is itself on the **R212-1**
> blacklist, and L221-14's sanction runs the other way. **The design is then built past the
> question**: after the click, charge the quoted amount or **REFUSE**, never more — safe under both
> characterisations, which is what lets the epic ship without waiting on counsel.
> **Sequencing that falls out**: build the **customer-facing** half now (*never charge more than
> displayed*); **do not** build the restaurant-facing half (*held to a withdrawn price for N
> minutes*) until the funds posture resolves — under one branch it is a purchase commitment, under
> the other a unilateral constraint on a business user. `QT-8`/`QT-9` are therefore **absorbed into
> BRIEF-20260818 §3(c) Q10** rather than asked separately, and `QT-1`–`QT-5` join that file's
> standing irreducible list (no second home invented). `QT-6` (absorbing the delta) is blocked
> upstream on [`CAPTAINNET-ZERO`](../decisions/CAPTAINNET-ZERO.yaml) — **no new register row
> opened**.
> **Two findings the proposal's §8 could not reach.** (1) `ADR-20260810-112836:97` accepted that
> *"the transient price a guest once saw is not in the log"* — true only while display and charge
> were structurally identical, and **the quote token retires that premise**, so the quote becomes
> the only evidence and needs a **third retention window** (the accounting clock
> `FRENCH_COMMERCIAL_BOOKS_10Y` is over-retention for a quote that never became an order, GDPR Art.
> 5(1)(e)). (2) A quote event on the Cart stream carrying `legalRetention` can take the Cart actor
> out of stream deletion and **break** [`ERASURE-LAUNCH-GATE`](../decisions/ERASURE-LAUNCH-GATE.yaml)
> — **pseudonymous-by-construction is free now and a migration later**.
> **Relay citations re-verified, one corrected**: `specs/ordering/errors.yaml:250-262` is exact;
> `specs/screens/restaurant_frontoffice.yaml:518` is a real `show_toast` but the **generic**
> `on_error` handler on `place_order`, not a purpose-built price-change disclosure — the concern
> survives and is worse for it, since a `PriceMismatch` reaches the customer today **only** as an
> anonymous transient toast (DSA Art. 25 + EAA accessibility).
> **No counsel is engaged** (founder, 2026-08-31: *"Not scheduled. We are on our own for now until
> the product is ready"*), so what cannot be self-answered is **marked**, not answered. Nothing in
> the brief was FETCHED — `legifrance.gouv.fr` and `economie.gouv.fr` returned **403 egress-policy
> denials**, `eur-lex.europa.eu` **202 with a zero-byte body** on two URL forms — so **every article
> number is VERIFY-FIRST even where the rule is graded (a)**. No lens output, and no aggregation of
> lenses, is legal advice or clearance (ADR-20260812-143619).

> **2026-08-31 — the priced quote token is DESIGNED, and the reversal it carries is now flagged in
> both records** (docs-only: one proposal, two record edits, two register-row notes, no `specs/**`,
> no code).
> [PROP-20260831-134539 "The priced quote token: display and charge agree by construction"](../proposals/PROP-20260831-134539-priced-quote-token.md)
> lands the design that
> [ADR-20260831-121957](../adr/ADR-20260831-121957-the-pm-read-step-is-retired-source-fixed-the-physics-and-left-the-ownership.md)
> §4d deferred, tracking
> [#816 "Display/charge divergence is undetected: the expectedTotal equality check never runs in production"](https://github.com/TheCaptainCompany/captain-food/issues/816).
> Recommendation: the **signed coordinate** — the token carries `(catalogId, catalogVersion)` plus
> the total, **never the per-line prices**, so the catalog stays the price authority
> (`specs/ordering/rules.yaml#/ServerPriceAuthority`) and the token is a pointer into the log rather
> than a client-carried price. `PlaceOrder.quote` is **required** (precedent: `customerId` under
> [#144](https://github.com/TheCaptainCompany/captain-food/issues/144)) and `expectedTotal` retires
> with it.
> **The finding that changed the design, verified in code**: the oversell guard
> `require_orderable_line` (`crates/application/src/commands.rs:793`) is called **only** at `:921`
> and `:950` — `AddCartLine` and `ChangeCartLineQuantity` — and **never at checkout**
> (`TODO(invariant)` at `:2604-2606`). So the pricer's `offer_by_id` lookup
> (`crates/application/src/pricing.rs:57-59`) is the checkout path's **only** catalog reality check,
> and an as-of fold that covered existence would move it off the money path with no diff to notice.
> Hence the fence: **the as-of fold is price-and-tax only**, enforced by a capability type with no
> availability accessor rather than by a rule (ADR-20260803-234035 level 4).
> **The unflagged reversal is fixed.** `ADR-20260831-121957` and `docs/decisions/QUOTE-TOKEN.yaml`
> now cite [ADR-20260810-112836 "Cart priced LIVE on read"](../adr/ADR-20260810-112836-cart-priced-live-on-read.md)
> and say plainly that **both** reversed clauses — the *"frozen at commitment"* freeze locus and the
> *"carried by the `expectedTotal` equality check"* enforcement clause — are in that record's **§2**;
> the earlier attribution of the second to §4 was wrong (its §4 is the `cart(id)` IDOR retirement).
> `ADR-20260810-112836` gains a §2-superseded-in-part header so a reader arriving there first is not
> told a dead check is the enforcement. Before this change `grep -c '20260810-112836'` returned
> **0** on both deciding records.
> **Register**: `QUOTE-STALENESS` stays **open** and is now **priced** (N = 30 min as a backstop
> only, sized from the cart-to-pay p99 with SCA/3DS; M dropped because `OfferStockUpdated`
> (`specs/catalog/events.yaml:198`) makes any small M a 100%-fire timer) — a pricing is not a
> decision. `CAPTAINNET-ZERO` is named as the blocker of the absorb *alternative* only: the
> recommended band needs no funding decision at V0, because `restaurant_payout` **is** the total and
> `captain_net` is zero in code (`crates/application/src/pricing.rs:105-114`). **No new register row
> was opened.**

> **2026-08-31 — the coordinator gets the register-check gate on its committing surface (#814).**
> Every *agent* has been gated on the ask since 2026-08-21; the **coordinator had no gate on any
> surface**, and in one session produced **nine** failures of exactly the class the gate prevents —
> an option space presented as open that ADR-20260829-230418 had decided, a counsel posture proposed
> without reading BRIEF-20260819 §4.2, a line-range citation (`pm_orchestrators.rs:844-852`) that
> **reads as confirming the claim while showing the opposite**, and a dispatch about to contradict
> PROP-20260815-142349. **Four of the nine were caught by the founder or a lens.** The ninth was
> caught by running the check before dispatching — the proof the discipline works.
> Now a `PreToolUse` hook on the **`Agent`** tool, as Lane D of the *same* `register-check.sh`
> (extended, not forked — the gate-script self-verification set stays at four files, so neither
> guard has to learn a fifth). Two design questions decided structurally rather than by a list:
> the **discriminator** is the target agent's own `tools:` frontmatter — write-capable is gated,
> read-only is not — so lens consults and reviewer passes pass untouched and granting an agent a
> write tool arms the gate for it *in the same commit*, with no exemption list to go stale; and the
> **escape hatch** is shut by requiring a cited record id to RESOLVE to a file under `docs/`, so a
> literal `Register check: none` and a well-shaped invented id are both refused.
> The validator returned the favour mid-write: §23 `record-citation-unresolved` refused the *fake
> ADR id used as an illustration inside the ADR itself* — the same principle one corpus over,
> caught by a gate rather than a reader.
> **Recorded honestly rather than hidden**: a hook gates a TOOL CALL, so the coordinator's prose
> answers to the founder stay ungateable. `.claude/skills/coordinator-register-check/` carries that
> half and is *weaker* — the pre-existing `decision-lookup` skill was invoked **zero** times in the
> session that produced the nine, which is why this one is a hook and not a paragraph, and why the
> right move is routing more coordinator→founder questions through `AskUserQuestion`.
> Records: [ADR-20260831-141500](../adr/ADR-20260831-141500-the-coordinator-gets-the-register-check-gate-on-its-committing-surface.md).
> Proven by selftest cases D1-D27 / LD1-LD3 / W4-W7 and by
> `tools/codegen-rs/src/tests.rs :: every_record_in_the_corpus_is_citable_through_lane_d`, which
> drives the real hook over 417 records; every case was observed RED before the suite was trusted.
> **It took three review rounds, and the shape of the misses is the lesson.** Round 1: the resolver
> globbed one `docs/adr/` filename shape and refused 101 of 266 real ADRs. Round 2: with a fixture
> per era in place it still mis-handled all 80 `docs/decisions/` rows (53 refused, 27 silently
> resolving to the parent proposal), and the `tools:` parse read only the first physical line, so
> four continuation shapes still failed open. Each round fixed the instances and re-asserted a
> universal — *"fails closed whenever the tool set cannot be read"* — that the next round falsified.
> Two rules came out of it, both now executable: **a gate that classifies members of a corpus is
> tested against the CORPUS, not against fixtures** (`docs/claude/sessions/workflow.md`, bound by
> selftest case CC), and **a universal claim backed by an enumeration is that same defect one level
> up** — so the shipped claims are scoped to what the gate DETECTS, with the line-based parse named
> as a residual rather than implied away.

> **2026-08-31 — three operational learnings from tonight's runs, and the argued decision to gate
> NONE of them** (records only; no `specs/**`, no code, no new hook).
> **(1) `git rev-parse HEAD == origin/main` does not mean you are ON `main`.** An executor passed
> its base-SHA precondition cleanly and still committed onto a sibling executor's branch, which had
> been cut from main's tip and was the checked-out HEAD — cost: a cherry-pick, a journal-conflict
> resolution and a `git branch -f` to lift the commit off a PR it did not belong to. Left as
> **prose, argued against ADR-20260803-234035**: `git worktree add <path> main` is *already* the
> gate and fails closed (exit **128**, `fatal: 'main' is already used by worktree at …`, verified
> both for `main` and for the sibling branch), so the mistake is made *unreachable* rather than
> *detectable*, with no new code. A `PreToolUse` guard on `git commit` was rejected on its merits —
> the payload carries no dispatch card, so the same observed state is correct for a docs dispatch
> and catastrophic for a code one, and the gate cannot tell them apart.
> **(2) The worktree rule was already recorded and the collision happened anyway**, because the
> dispatch card named a weaker mitigation (*"stage only your paths"*) — **staging protects the
> INDEX, not the BRANCH**. New coordinator-binding rule: **a card may not name a mitigation weaker
> than the recorded rule**, since the executor reads the card, not the topic file. The disk
> objection that produced earlier "no worktree" cards is priced and scoped: a docs worktree is
> **36 MB** (no `target/`) against the shared checkout's **23 GB**, of which `target/` is **22 GB**
> — so a docs/spec run in an occupied tree takes a worktree unconditionally.
> **(3) A record pinning a fact to "in flight" acquires an expiry nothing detects.**
> `ADR-20260815-030206`'s *"not on `main`"* was false from **2026-08-16** (PR #566) and produced a
> **false negative in a register check** on 2026-08-31 — the register discipline's own failure mode.
> **Deliberately not scanned**, on measured grounds: `in flight`/`in-flight` occurs **63** times in
> `docs/adr/` + `docs/proposals/` and is dominated by *domain* usage, leaving a checkable set of
> **3** merge-state assertions plus **6** `until #NNN lands` lines; `gh` is **not installed** in the
> container and the clone is **shallow** (205 commits, oldest **2026-08-17**), so no local check can
> resolve a 2026-08-16 merge; and the failure rode a docs-direct-to-`main` push that no PR-triggered
> CI check sees in time. The fix is in the **writing**: date the claim (*"as of 2026-08-15, on
> branch `564-…`"*) so it is never false, only old.
> **No ADR and no proposal** — three sharpenings of existing rules with no option space
> (CLAUDE.md proportionality). All three land in
> [`docs/claude/sessions/workflow.md`](../claude/sessions/workflow.md), sharpening the sections that
> already existed rather than appending near-duplicates (ADR-20260730-034635).
> **Card defects found**: the dispatch cited an ADR id and a `register-check.sh` "Lane D" as having
> landed tonight — both exist only on the **unmerged** `814-…` branch, not on `main` (the card
> reproduced item 3's own failure mode); and it pointed at `environment.md` for the worktree rule,
> which actually lives in `workflow.md`. The validator caught the id itself: quoting it here tripped
> **`record-citation-unresolved`** at `docs/status/journal-2026-W36.md:39`, which is the *existence*
> half of item 3 already gated — the *tense* half is what remains uncovered.

> **2026-08-31 — two founder calls on the back of the `read:` retirement: BUILD the priced quote
> token, KEEP the two-hop ask (records only).** Both were put to him with options, trade-offs and a
> recommendation; both are closed, and neither moves a stored shape.
> **`QUOTE-TOKEN` — he chose (B), build it.** The priced cart returns an **opaque token carrying the
> catalog stream version it was computed at**; `PlaceOrder` carries it; the write side prices **as of
> that version**. Display and charge then agree **by construction** and keep agreeing if the
> projection is dropped, rebuilt or lagging, and **repricing becomes explicit at the cart step,
> before the Stripe element — never after**. So `young`'s finding is **adopted**, not merely
> recorded, and the honest account of today's interim goes in the record rather than reading as a
> defence: *"display/charge coherence currently rests on a rebuildable artifact and on two reads at
> different times; it does not survive a catalog rebuild and does not survive a slow customer."*
> **What does NOT change is `evans`'s ruling**: `specs/ordering/processmanager.yaml:63-68` is a
> **Published Language, not an exemption** — the *mechanism* that enforces it is replaced, its
> *status* is not, and filing it as a lapse being cleaned up would get both halves wrong.
> **It narrows PMW-4**: once the token lands the checkout leg asks for an as-of price, so `:63-68`
> stops being a survivor and only the session-carts leg remains — meaning `evans`'s proposed
> `authority:` kind could ship with **zero users**, which PMW-4's decider now has to weigh.
> `young`'s coupling is recorded too: the as-of fold is the **same primitive SNAP-1 needs**.
> The **staleness policy is open** (`QUOTE-STALENESS`) — he named neither N nor M, and it is being
> priced rather than re-asked. The build itself is a **separate work item**; a proposal + tracking
> issue follow.
> **The tension is named rather than glossed**: PROP-20260815-142349`:142` refuses a version field in
> an ask **reply payload** (*"the served version rides the ENVELOPE, never the payload … one rule,
> both speech acts"*). A token on a **command** is adjacent but **not the same speech act** — a
> reply's authority expires at send, whereas **a price quote the customer was shown is business
> data**, like an `ExternalReference`. Recorded so the next reader knows the rule was weighed.
> **`SETTLE-PAYMENT-REF` — he chose (A), keep the two-hop ask.** PROP-20260815-142349 **§9 stands
> unamended**; `paymentIntentId` is **not** added to the Order's facts. `young`'s challenge is
> recorded as **considered and rejected, with its argument intact** — *"'forced by typing' is only
> true because of an event shape we own"*, on the exact precedent of PROP-20260808-142532 **D2**
> (Approved 2026-08-08), which decided the identical cross-aggregate-field pattern **event-carried**.
> A rejected argument kept with its reasoning is what stops it being re-litigated every quarter.
> **The accepted cost is stated, not buried**: **two stream folds per settlement decision, on the
> money path, at Friday peak, with no residency** — re-verified, `load` at
> `crates/infrastructure/src/mailbox/activation.rs:237` returns early for any foreign stream at
> `:238-240`, so every cross-stream load goes straight past the cache, and **PMW-2 has not moved
> since 2026-08-15**. That makes **PMW-2 materially more valuable than its AMBER suggests** — it
> stops being an efficiency item and becomes what pays for a decision already taken on the money
> path — and the row now says so instead of leaving the reader to connect it.
> **CLAUDE.md question (2) is answered NO for all three** — the retirement, the token and the
> reference. Keeping the ask is precisely the choice that leaves `OrderPlaced` untouched; the
> rejected alternative is the one that would have opened a stored-shape question.
> Unchanged by all of it: the retirement, the nine-standing-violations framing, PMW-3 parked, the
> two-survivors-are-two-classes correction, the rejection of "exemption", the four-line discipline,
> and that the retirement does **not** close the #544 silent-expiry class.
> Records: [ADR-20260831-121957](../adr/ADR-20260831-121957-the-pm-read-step-is-retired-source-fixed-the-physics-and-left-the-ownership.md)
> §4d/§4e, DECISIONS §42 (QUOTE-TOKEN, QUOTE-STALENESS, SETTLE-PAYMENT-REF).

> **2026-08-31 — the PM `read:` step is retired: `source:` fixed the physics and left the ownership
> (records only, no `specs/**` edit).** The founder struck the first conjunct of PMW-1's closure,
> verbatim: *"`read:` stays, exactly as PR #566 lands it with `source: PROJECTION | EVENT_STREAM`
> **<=== must be retired from the process manager**"*. **This is not a change of rule.** A `read:`
> step, in either source mode, has the process manager naming another aggregate's table and picking
> columns out of it — the fold is written on the PM's side of the boundary. `EVENT_STREAM` moved the
> *storage* and left the *ownership*. Retirement is the **level-4 (unrepresentable-state)** form of
> the founder's own 2026-08-15 rule (ADR-20260803-234035), moving it from *declared* to
> *unspellable*; `read:` was the last place the wrong thing was still sayable.
> **The deliverable is nine legs, not a keyword** — 11 `PROJECTION` steps minus 2 survivors, eight of
> the nine on the money path (`specs/payments/processmanager.yaml:53,70,86,101` settlement,
> `:132,161,189,219` refund, all on `OrderTracking`) and one on dispatch
> (`specs/delivery/processmanager.yaml:36`). So **ADR-20260815-030206 is today a rule with nine
> standing violations**, and sequencing this as a rename would understate it by an order of magnitude.
> Counts re-derived, with antecedents (ADR-20260817-105845): **15** `read:` steps and a **4/11**
> split, from `grep -rn '^\s*- read:' specs/*/processmanager.yaml` and
> `grep -rn 'source: ' specs/*/processmanager.yaml` at `6b74739b` — PMW-1's row said thirteen and
> three, and both are corrected in place.
> **The two survivors are TWO classes, and "exemption" is rejected as the noun for either.**
> `ordering:163-169` (a session's open carts) IS a genuine carve-out — set-shaped, and `SessionId`
> belongs to no aggregate. `ordering:63-68` (the live-catalog price authority) is **not a carve-out at
> all**: an addressable `Catalog` aggregate exists, and the shared read is the CORRECT design because
> the cart screen and the checkout leg go through the same `price_cart` seam, and that coherence
> carries a legal display guarantee (`rules.yaml#/ServerPriceAuthority`, *Code de la consommation*
> L112-1/L221-5). Calling it an exemption is false and dangerous — it tells the next reader to "clean
> it up", which would charge a price the customer never saw, on the money path, at peak.
> **What is OPEN is only the spelling** (row **PMW-4**, `reconsiders: PMW-1`): two narrow kinds
> (`index:`/`by:` → the unowned key scalar; `authority:` → the authoritative rule) **recommended**,
> one differently-named kind with a mandatory exemption `$ref` recorded as the **dissent** with its
> cost. A *generic* hatch is refused — *"two carve-outs riding a surviving `read:`, or a generic
> exemption `$ref`, is `source:` again wearing a new name."*
> **PMW-3 (the transport) is untouched and stays parked.** The mechanism question is settled
> structurally rather than by picking a transport: the wall separates MODELS, not processes —
> `domain_events` is the write model's storage, so a fold through the aggregate's own fold function
> IS the write side. The objection was never to a PM holding an `EventStore` port; it is to a PM
> holding an `OrderReadRepository` (live at `payment_settlement.rs:54`, `delivery_dispatch.rs:83`).
> **No migration is owed, and the record says so instead of borrowing the vocabulary**: `read:` emits
> hook signatures and call sites (`emit/pm_orchestrators.rs:710,2112`), never data; PM state rows come
> from a different emitter; no `read:` is in any event payload; and `source:` is consumed by **no**
> emitter, so the retirement deletes zero generated query code. It is still **`HOLD: human`** — a
> behaviour change on the money path (a leg that silently skipped now retries and alerts).
> **The record does NOT claim this closes the #544 silent-expiry class**; that is the exhaustive
> branch. What the fold buys is narrower and real: under `PROJECTION`, *"not yet projected"* and
> *"not authorized"* are the **same observation** — an ambiguous absence becomes an authoritative one.
> **Two false sections of ADR-20260815-030206 were corrected** (dated notes, not silent rewrites): it
> still said the `source:` enumeration was *"not on `main`"* and that *"until PMW-1 lands, this record
> is prose"* — #566 merged sixteen days earlier, and **that sentence produced a false negative in a
> register check tonight**. General shape worth keeping: **a record that pins a fact to "in flight"
> acquires an expiry date the moment it is written, and nothing detects the expiry.**
> Also: PMW-1 migrated out of `docs/decisions/_legacy.yaml` (a `reconsiders` target must be declared),
> and PROP-20260815-142349 §18 + D2 rewritten in place — both were framed entirely on #566 being open.
> Records: [ADR-20260831-121957](../adr/ADR-20260831-121957-the-pm-read-step-is-retired-source-fixed-the-physics-and-left-the-ownership.md),
> DECISIONS §42 (PMW-1, PMW-4).

> **2026-08-31 — the `send:` route grammar: four unlaned command sends declared, gated and
> dedup-keyed (#807).**
> `PmStepDef::Send` carried no `to` and no `route_gate` while all four committed `send:` steps
> already WROTE `to:` in the DSL — `pm-send` validated the target and the emitter then discarded
> it, so a `send:` could never reach `ROUTED_LANES` or the `Route` enum. Now it can: three routes
> (the two `MarkOrderDelivered` legs are two triggers for one route), three `specs/common/`
> configuration keys all `default: false`, legacy arms preserved byte-for-byte behind each gate —
> `git diff -w` on the regenerated `process_managers.rs` shows **zero deletions**, the whole diff
> is additive. `pm-route-gate` now covers `send:` steps, and because `to:` is mandatory on a send,
> **every send must declare its route**.
> The find that justified generating before believing: the money path. Keying the routed door on
> the TARGET's identity — the obvious default — would have keyed the credit door on `customerId`
> while `grant_customer_credit` is idempotent per `reclamationId`. One customer receives many
> goodwill credits, so that door would have swallowed every grant after the first: money owed,
> never paid, no error anywhere. A new rule `pm-send-dedup` makes the axis a mandatory declaration
> with **no default**, since the safe axis does not follow from the target.
> Records: [ADR-20260831-093000](../adr/ADR-20260831-093000-the-enumeration-is-deliver-and-send-not-deliver-alone.md)
> corrects ADR-20260829-230418's enumeration (`deliver:` → `deliver:` ∪ `send:` ∪ wrapper-seam
> `sends:`); the property in `specs/common/processmanager.yaml` already covered sends, so this
> executes the recorded decision rather than amending it.
> **Round 2**, after the independent reviewer returned FAIL on two blocking findings. (1) The
> `LaneEnqueue` type's own doc still stated as FROZEN the very rule this branch proves
> catastrophic — *"`external_id` is the TARGET AGGREGATE's id"* — which the generated credit
> route falsifies on the same branch. Both sites now say the axis is DECLARED (`dedup_by:`), and
> that it means **the same request**, not *the key the target handler is idempotent on*:
> `MarkOrderDelivered` REJECTS a repeat rather than absorbing it, so on that route the door is
> the only thing collapsing a partner report racing a rider completion. Its corollary — a door
> minted by a REJECTED first attempt stays minted, closing the route to a later legitimate
> attempt — is a property of `main`'s already-merged C2 door, filed as
> [#811 "A routed COMMAND door is minted at ENQUEUE, so a REJECTED first attempt permanently closes it"](https://github.com/TheCaptainCompany/captain-food/issues/811) and a precondition on both
> flips. (2) `ROUTE_ORDER_DELIVERY_COMPLETION_THROUGH_LANE`'s consequence list gained **(e)**: a
> successful COMMAND-door delivery arms the declared `schedules:`, and `MarkOrderDelivered`
> declares the `OrderExpired` retention clock. Today the saga's in-process arm creates no mailbox
> row, so a completion reported by a PARTNER or by an INDEPENDENT RIDER arms **no** retention
> clock while the same order closed through the `markOrderDelivered` mutation does — a legal
> surface, now named in the text the flip decision is made from.
> Posture `HOLD: human` — PR stays draft. Four non-blocking findings were filed rather than
> fixed here:
> [#810 "`pm-send-dedup` proves a routed send's axis EXISTS, never that it is the RIGHT one — declare the handler's same-request key in the DSL"](https://github.com/TheCaptainCompany/captain-food/issues/810),
> [#811 "A routed COMMAND door is minted at ENQUEUE, so a REJECTED first attempt permanently closes it (blocks the delivery-completion and replacement-birth flips)"](https://github.com/TheCaptainCompany/captain-food/issues/811),
> [#812 "No `pm-deliver-lane` equivalent for routed `send:` steps — a routed send to a mailbox-less aggregate passes validate and fails inside the leg transaction"](https://github.com/TheCaptainCompany/captain-food/issues/812)
> and
> [#813 "`order.lane.enqueue`'s `business.aggregate_id` is bound from `external_id` — on a routed send whose dedup axis is not the aggregate it carries the wrong id"](https://github.com/TheCaptainCompany/captain-food/issues/813).

> **2026-08-31 — four decision rows declared: the three residues #764's ruling left open, plus the
> erasure PM's resume correlation, which cannot be built as approved.**
> Records-only change, straight to `main`. `CREDIT-AT-ERASURE` closed D1-D3 on 2026-08-31
> ([ADR-20260831-033621](../adr/ADR-20260831-033621-customer-credit-is-disposed-of-as-a-leg-of-erasure-goodwill-credit-is-refundable.md))
> and explicitly left D4/D5/D6 open; the recording run could not split them because an executor
> never files an out-of-dispatch decision file
> ([`docs/decisions/README.md`](../decisions/README.md), *"Partial closure = split at close time"*).
> They are now keys:
> **[CREDIT-EXPIRY-WINDOW](../decisions/CREDIT-EXPIRY-WINDOW.yaml)** — 180 days minus a settlement
> margin, or 1 year and adjudicate the gap. Stripe cannot refund a capture indefinitely (~180 days
> in practice), so a credit aged 6-12 months is **traceable and not refundable**, a third state the
> ruling has no branch for; and the tension cuts both ways, because if traceable credit is the
> customer's money then **any** expiry extinguishes it on a timer. The 1-year default
> ([ADR-20260726-163737](../adr/ADR-20260726-163737-reclamation-saga-and-credit-ledger.md)) is
> **chosen but unbuilt**, so the window is free to move today.
> **[CREDIT-DRAIN-ORDER](../decisions/CREDIT-DRAIN-ORDER.yaml)** — promotional first (customer-
> favourable, and the only ordering that cannot be accused of engineering a smaller refund) or
> traceable first. **This row has a clock**: free until the first promotional grant exists, a
> migration after. Verified rather than assumed: `CustomerCreditState` is a single `balance_cents`
> scalar with **no lots at all**, so there is no drain order in the code to preserve — whatever is
> picked is also a decision to give the balance provenance.
> **[CREDIT-LEG-SEQUENCING](../decisions/CREDIT-LEG-SEQUENCING.yaml)** — deliberately **widened**
> past D4's scheduling wording, because it cannot be answered without its two hard preconditions in
> view: (1) `CustomerCreditGranted` carries only `customerId`/`amount`/`reclamationId`
> (`specs/payments/events.yaml:184-195`), so the D1/D2 split is a **stored-event-shape change**;
> (2) the only writer to `CustomerCredit-{customerId}` is the unlaned `send:` at
> `specs/ordering/processmanager.yaml:259`, so the erasure leg would be that stream's **second
> unlaned writer**, separated from the first only by an optimistic version conflict.
> **[ERASURE-PM-RESUME](../decisions/ERASURE-PM-RESUME.yaml)** — new, and the one with a build
> blocked behind it. [PROP-20260829-150752](../proposals/PROP-20260829-150752-customer-erasure.md)
> §3.1 has the parked erasure resume on the blocking order's terminal fact, and **that cannot be
> spelled**: `raw_msg_expr` (`tools/codegen-rs/src/emit/pm_orchestrators.rs:964-972`, called from
> `emit_state` at `:1392`) panics on any `state.by` value that is not a property of the trigger
> message, and none of the four order terminal facts carries a `customerId` — `OrderDelivered`,
> `OrderCancelledByCustomer`, `OrderCancelledByRestaurant` carry `orderId` + `restaurantId`,
> `OrderExpired` carries `orderId` alone. Three options with a real doctrinal split (A `from_read`
> through a projection — young: a projection becomes a write-side correlation input, and under
> projector lag the PM does not resume while a GDPR clock runs; B `customerId` on the four facts —
> replay-neutral but a stored-shape change putting an identifier on four more payloads retained
> 3650 days; C a PM-owned order-to-customer index — vernon-clean, costs a third table and misses
> orders created after the request). Architect recommends **C, fallback A**, recorded as a
> recommendation and not an answer. It blocks the erasure **runtime** chunk of
> [#708](https://github.com/TheCaptainCompany/captain-food/issues/708), which
> [ERASURE-LAUNCH-GATE](../decisions/ERASURE-LAUNCH-GATE.yaml) makes launch-blocking.
>
> **STATUS.md corrected in the same change.** The `Aggregates own the facts` row still read C2 as
> *"built, gated OFF, awaiting review"*; `eda50a63` (*"Closes #595 …"*, PR
> [#762](https://github.com/TheCaptainCompany/captain-food/pull/762)) is an ancestor of `main`, so
> that has been stale since the merge. It now reads **merged, gated OFF**, and says the thing the
> old wording hid: `ROUTE_REPLACEMENT_BIRTH_THROUGH_LANE` defaults `false`
> (`specs/ordering/configuration.yaml:63`), so
> `crates/application/src/process_managers/reclamation.rs:157` still takes the legacy in-process
> path — **a live unlaned birth on merged code**, not on a branch awaiting review.

> **2026-08-31 — the founder ruled on the credit balance that outlives its erased subject: it is
> disposed of as a LEG of the erasure, never a park.**
> ([ADR-20260831-033621](../adr/ADR-20260831-033621-customer-credit-is-disposed-of-as-a-leg-of-erasure-goodwill-credit-is-refundable.md),
> register row [CREDIT-AT-ERASURE](../decisions/CREDIT-AT-ERASURE.yaml), six lenses.) Directive:
> **refund credit traceable to a captured payment, forfeit purely promotional credit, disclose the
> balance at the confirmation step before the irreversible act.** **Escheat** and
> **block-until-zero** both rejected — escheat invents an unowned-funds posture we have no basis
> for, block-until-zero makes a legal right hostage to a marketing balance. Three rulings on the
> branches the directive did not have: **D1 → A**, reclamation **goodwill credit is REFUNDABLE** —
> the third category, and **100% of the credit that can exist at V0** — to the **original captured
> instrument**, capped at the **un-refunded remainder of that capture** (a full refund plus a
> goodwill grant on one claim otherwise pays €35 against a €30 sale). **D2 → A**, forfeiture is a
> rule of **ACCOUNT TERMINATION GENERALLY** — closure, dormancy, the existing one-year expiry and
> erasure alike — because **Art. 12(5)** requires exercising a right to be free of charge and a
> balance extinguished *because* someone asked to be erased is arguable as a charge. **D3 → A**, a
> **failed refund PROCEEDS AND IS RECORDED**: the erasure completes on the **Art. 12(3) clock**, the
> failure lands on the pseudonymous receipt, the amount becomes an ordinary payable — the founder's
> own objection to block-until-zero, applied consistently.
>
> **What the record explicitly does NOT close.** **D4** (does the credit leg ship inside
> [#708](https://github.com/TheCaptainCompany/captain-food/issues/708) or after), **D5** (shorten
> the expiry to ~180 days so *traceable* implies *refundable* by construction) and **D6** (which pot
> drains first when credit is spent — **free only until a promotional grant exists**) are **open**,
> and need keys the coordinator declares. **The three counsel questions on
> [#764](https://github.com/TheCaptainCompany/captain-food/issues/764) are NOT discharged**: legal's
> verdict is **0 discharged, 1 narrowed, 2 untouched**, and **Q2 is now heavier** — both limbs of
> D1/D2 produce an accounting movement someone may have to prove, so "is the credit ledger
> L123-22-retained or shreddable?" now covers more of the ledger, not less. `decided` is a recorded
> founder decision and **not legal clearance**.
>
> **Four lens findings verified against the tree rather than taken on the card's word.**
> `CustomerCreditGranted` carries `customerId`/`amount`/`reclamationId` and **no provenance field**
> (`specs/payments/events.yaml:184-195`) — so the refund/forfeit split is a **stored-event-shape
> change**, and the disclosure block is **absent, not zero**, at V0 (ux). It also carries **no
> `legalRetention:` marker** while `PaymentCaptured` and `PaymentRefunded` both carry the 10-year
> one (`:41`, `:141`) — and the refund arm **creates a new 10-year retained record naming the
> subject as part of erasing them**, which must appear in `retainedUnder` (legal).
> `CustomerCreditBalanceRow` is `customer_id`/`balance_cents`/`currency`/timestamps
> (`crates/application/src/generated/rows.rs:181-187`), so beck's prediction is exact: a classifier
> handed that row applies a **default to 100% of balances**, and `default ⇒ forfeit` silently
> forfeits every refund owed **while every unit test stays green** — the counter-measure is
> compiler-first, a parameter type that row cannot satisfy. And `GrantCustomerCredit` is a `send:`
> step in the PM's own thread (`specs/ordering/processmanager.yaml:259-261`) — **an unlaned
> foreign-stream write on the money path, live today, and not among C3's twelve** (Payment ×7,
> DeliveryJob ×4, Cart ×1).
>
> **The rejected option business had assumed was available is foreclosed by our own design**:
> "let the subject spend the credit first" cannot be offered, because **`re-login-cancels`** means a
> customer who logs in to spend the balance **cancels their own erasure**. Any "use it first" copy
> would be a lie.
>
> Also corrected in the same change: `docs/claude/sessions/environment.md` claimed **executors
> cannot perform GitHub API mutations**. They can — `gh` is absent and MCP is not in a subagent's
> toolset, but `curl -H "Authorization: Bearer $GH_TOKEN"` against `api.github.com` returned `200`
> from this executor. Believing the old sentence makes an executor hand back incomplete work for a
> capability it has.
>
> **Week roll**: this file is new. 2026-08-31 is the **Monday of ISO 2026-W36**, and only
> `journal-2026-W35.md` existed. The budget ledger had already rolled to W36 — which is the trap the
> dispatch named, since inferring the journal's week from the budget file is exactly the wrong
> method; `date +%G-W%V` is the check.
