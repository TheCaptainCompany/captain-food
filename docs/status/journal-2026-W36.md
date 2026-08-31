# Status journal — 2026-W36

Journal entries for ISO week 2026-W36, newest first, in the order they were written.

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
> declaration.** (1) The pay button never resolved `checkout.place_order` at all: it rendered the
> hard-coded literal `"Place order - "{total}` (`crates/web/src/checkout.rs`), so **every French
> customer has been shown an English button**, and fixing only the DSL copy would have changed
> nothing a customer sees — the #780 class exactly. The fix is compiler-first rather than a parity
> test (ADR-20260803-234035): the button resolves the key from the generated catalog, so the spec's
> message **is** the label by construction, one string instead of two a test must keep equal.
> (2) `collapsible: true` was read by **nothing** — not the codegen, not the renderer — and the
> hand-written runtime always rendered the recap expanded; so the flag was never the guarantee, and
> the guarantee now lives in a runtime assertion. Both new tests were mutation-checked (revert the
> catalog copy / restore the literal / wrap the recap in `<details>`): all three go red.
>
> **Adjacent, NOT fixed here and needing an issue**: the recap's own text is hard-coded English in
> the same runtime (`format!("{} items - {}", ...)` plus a literal `" from "` and `<h1>"Checkout"</h1>`),
> so a French customer reads *"2 items - 23,50 EUR from Chez Marcel"*. Same class as the button
> defect, different fix, and `PROP-20260831-134539` slice 5 rebuilds that component.

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
