# Session rules — evidence

Part of [`../sessions.md`](../sessions.md).

## 5. Establish a third-party integration's shape BEFORE naming anything

The expensive lesson from the Uber session (ADR-20260730-032306): credential key names were proposed
from an assumed product and auth mechanism, and the dashboard then showed a different API suite. Two
wrong key sets and four mis-named repository secrets later, they all had to be recreated.

Establish these **first**, from the provider's own screens, before proposing a single key name:

1. **Which product/API suite** the app is registered against (Uber Direct and Uber Eats Marketplace
   are different products with different agreements — the app header states the suite).
2. **The auth mechanism** — shared secret vs asymmetric assertion. It changes the whole key set.
3. **Inbound vs outbound credentials** — they are different directions and different mechanisms.
   Conflating them yields a verifier that rejects everything, fail-closed.
4. **Which values are per-tenant.** Anything scaling with restaurants is a table row
   (`hubrise_connections`, `uber_eats_connections`), never a config key. Config is per-deployment.

Then name operator-facing keys in **the provider's vocabulary** (`APPLICATION_ID` when the dashboard
says "application id", not `CLIENT_ID`): `configuration.yaml` exists so an operator can map a
dashboard field to a secret without translating.

A secret whose **name** disagrees with its **contents** is worse than a missing one — the boot report
reads `set` and the failure surfaces later, asynchronously, as an authentication error.

## 6. Verify a config key's real consumer before declaring it

Do not infer which deployable owns a key from its name. In one session fourteen adapter keys were
classified as belonging to separate deployables when `crates/server` **links every adapter** and is
the process that runs them. Grep the composition root for the reader first:

```bash
grep -n "from_env\|std::env::var" crates/server/src/lib.rs crates/adapters/*/src/*.rs
```

The reader decides the owner. This matters because the boot report is supposed to answer "is this
integration configured in production?" from one `curl` — a key attributed to the wrong process is
absent from the report that should have shown it.

## 7. A green deploy job does not mean the new code is running

`deploy.yml` POSTs Render's deploy hook and exits. The job goes green when Render **accepts the
trigger** — not when the image is live. On 2026-08-01 that gap put an **11-day-old binary (222 commits
behind) against a schema 9 migrations ahead**: `deploy` green, `db-migrate` green on its success, and
production quietly serving `426730b6` while every worker looped on
`relation "inbound_events" does not exist`.

Cost: a live incident, and ~30 minutes to diagnose from a startup log the founder happened to
paste. Nothing in CI would ever have said a word.

**So after any deploy, verify what is actually RUNNING before you believe it landed.** The startup line
`captain-food server starting — version <sha>` (and `/health`'s `version`) is the only ground truth; the
workflow's own success is not evidence. If the SHA is not the one you deployed, the deploy did not
happen — whatever GitHub says.

Two traps behind it, both worth knowing before you reach for the same explanation:

- A **service env-var change in Render redeploys the CURRENTLY configured image**, which can silently
  override a deploy that was triggered but never completed. That is how this one was masked.
- `/health`'s schema gate does **not** protect the in-process workers — they start and hammer the
  database whatever the schema says, and the gate is looking at the new instance while the old one is
  the one actually serving.

Two more from the API side (2026-08-03 — cost: production briefly ROLLED BACK to the 07-29 binary
while restoring auth):

- The dashboard's "save and deploy" behaviour does NOT exist in the API: **`PUT
  /v1/services/{id}/env-vars/{key}` changes the stored env and restarts NOTHING**. The running
  process keeps its old environment until a deploy is explicitly POSTed — probe the actual
  behaviour (the 503→401 dummy-JWT flip, not the env listing) before declaring a config change live.
- The service is **image-backed with a PINNED `imagePath`** that CI's deploy-hook calls override
  per-deploy but never update. So a bare **`POST /v1/services/{id}/deploys` redeploys the stale
  pinned image** — the July binary, not what's live. Always pass the intended digest explicitly:
  read `image.ref` from the latest good deploy (`GET .../deploys?limit=N`) and POST
  `{"imageUrl": "<that ref>"}`. Verify `/health`'s `version` after, per the rule above.

Tracked as [#281](https://github.com/TheCaptainCompany/captain-food/issues/281); until it lands, the
manual check above is the whole safety net.

### "Verbatim" is a mechanical check, not a careful read

A record that claims to quote — a founder directive, a lens's return, a fetched legal instrument —
states a property a script can decide, so decide it with a script. A careful re-read does not catch
a dropped clause in a long quote; two sessions proved that.

- **Assert substring containment per FRAGMENT, on whitespace-normalised text** (collapse runs of
  whitespace in both haystack and needle before comparing). Markdown re-wrapping changes line breaks
  inside a quote that is otherwise perfect, so a line-based diff reports noise and a naive equality
  check fails on a correct quote.
- **Anchor the extractor on STRUCTURAL markers — list items, table rows, headings — never on quote
  characters.** Nested quotation (a quote inside a quoted passage, or an apostrophe in French prose)
  breaks any extractor that pairs `"` or `«`, and it breaks it silently: it returns a shorter
  fragment that still passes containment.
- **When the record quotes a FETCHED instrument, re-extract from the fetch artifact if it is still on
  disk** — the scratchpad copy of the fetched page, not an aggregation of it that some earlier turn
  wrote. That is what recovered the first sentence of CRD 2011/83 Art. 22 after a summary had
  quietly dropped it; the summary read fine and was wrong.

The general form, which is the expensive half: **an aggregation of a source is not the source** —
which is also the card-authoring rule below.

### A card that says "carry lens X's return" must point AT the lens's own return

Otherwise it must say plainly that the executor is composing. The two are different deliverables and
only one of them may be attributed to the lens. When a card names an **aggregation** — the
coordinator's summary of a lens's return — as the source for material it describes as
**transcription**, the executor authors content and ships it under the lens's name: on 2026-08-18
that produced a legal artifact whose counsel questions and obligation map were the executor's, not
`legal-specialist`'s, with a numbering that diverged from the lens's own (caught before merge,
corrected in revision 2; cost: one full re-authoring of a legal brief). A card carrying a
`verbatim` claim therefore names a retrievable artifact — a file path, a message, a fetch artifact —
never a summary of one, and the executor treats "carry X's return" over an unretrievable source as a
**card defect** to flag rather than a writing task to complete.

**The executor's half is a PRE-FLIGHT, not a discovery at the section that needs it.** Before writing
the first line of a document that transcribes, resolve every item the card names as verbatim against
its stated source and confirm each is actually present — one grep per named item, on structural
anchors (`G8`, a heading, a table row), not a skim. On 2026-08-19 a card asserted counsel questions
**G8–G11** were carried verbatim in a round-4 aggregation; they existed only in the lens's own return
and never reached the aggregation, and the executor found the absence on **reaching §5**, with the
other five sections already composed. Refusing to compose them was correct — but the same refusal an
hour earlier costs one grep instead of a composed document, a follow-up dispatch and a second review.
An item the card names and the source does not contain is a **card defect reported before work
starts**; the document is then written once, around a known gap or not at all.

## 14. A green review job does not mean a review happened

Sibling of §7, and it hid for longer because the job is *supposed* to be quiet. `Claude Code Review`
ran **271 times, every run green, and never posted a single comment** — no review, no thread, no
check-run output (`output.summary` is empty). One run on
[#344 "Close four 'declared but does nothing' holes"](https://github.com/TheCaptainCompany/captain-food/pull/344)
cost 15.7 minutes and $14.93 of model usage to end at `No buffered inline comments`.

Three independent causes, all invisible from the run list:

- **`/code-review:code-review` reports to stdout unless given `--comment`** — and the action hides
  stdout by default (`show_full_output: false`, for secret safety). The review existed; nothing
  could read it.
- **`permissions: pull-requests: read`** in the workflow. Posting needs `write`; without it the
  job still concludes `success`.
- **The plugin skips DRAFT PRs by design.** Under the claim protocol (docs/BACKLOG.md) a PR is a
  draft for nearly its whole life, so most of those 271 runs were pre-paid no-ops.

Three things worth carrying beyond this workflow:

- **This reviewer cannot be changed on a branch. Only `main` counts.** Under the OAuth-token path
  the action validates that the workflow file is *byte-identical to the copy on the default branch*
  and **skips itself** otherwise — green in 10 seconds, one `##[warning]` deep in the log, no review:

  > Skipping action due to workflow validation: The workflow file must exist and have identical
  > content to the version on the repository's default branch.

  So a PR that edits `claude-code-review.yml` disables its own reviewer, and any change to the
  reviewer is unprovable until it is merged. Smoke-test it **after** the merge, from a branch that
  does not touch the workflow. (Same reason the action restores `.claude/**`, `.mcp.json` and
  `CLAUDE.md` from `origin/main` — "PR head is untrusted". Its config is `main`'s config, always.)
- **Tool permissions belong in `claude_args: --allowedTools`, not `.claude/settings.json`** — the
  interactive allowlist is restored from `origin/main` and does not cover what the review plugin
  needs (`gh pr comment`, the inline-comment MCP tool).
- **`permission_denials_count` in the run's result JSON is the health metric.** 41 denials in a
  33-turn review meant the agent spent its turns bouncing off an allowlist written for interactive
  sessions. A review job that is green with a high denial count is a review that did not happen.

Smoke-test the reviewer the same way you would a deploy: land the change, then open a PR carrying a
deliberate, realistic bug and confirm the finding arrives **on the PR**. "The workflow ran" is not
evidence — here it was not even true.

- **The self-skip EXITS 0, and `claude-review` is a REQUIRED check — so such a PR clears its own
  review gate** ([#677](https://github.com/TheCaptainCompany/captain-food/issues/677)). Since
  [#680](https://github.com/TheCaptainCompany/captain-food/pull/680) the job asserts the outcome
  instead: the reviewer's summary comment must CONTAIN, on a line of its own,

  > `Reviewed-Commit: <sha>`

  on a line of its own, outside any ``` / ~~~ fence **opened at column 0** (that is the entire
  block rule — indentation, blockquotes and lists are all treated as live), where
  `<sha>` is the PR's **head or merge** commit — decorated (backticks, bold) and abbreviated to
  ≥7 hex are accepted, because across 23 real reviewer comments (every `claude[bot]` comment on
  PRs #670/#674/#675 — 5 + 10 + 8 — via `GET /repos/{owner}/{repo}/issues/{n}/comments`) it wrote
  a bare 40-char sha **zero** times. **Copy the sha from the prompt, not from the working tree**: on a `pull_request`
  event the checkout is the MERGE ref, so `git rev-parse HEAD` reports something else.

  Two things that gate CANNOT do, both learned the expensive way. **It cannot prove WHICH bot
  reviewed** — this repo's own mob-lens sessions post under the identical `claude[bot]` identity, so
  anyone quoting a marker into a comment satisfies it. Never paste a marker line into a comment: it
  flips the check green and destroys the only signal it carries. **And it cannot make the check
  required or un-required**, nor stop the ruleset accepting `skipped`. Requiredness lives in ruleset
  `19179892` — [DECISIONS §45 REV-1](../../proposals/DECISIONS.md) and
  [#593](https://github.com/TheCaptainCompany/captain-food/issues/593), still open. A red from that
  step means **no verdict was produced**; it does not mean the reviewer found a problem.

- **`gh api --paginate --jq` applies the filter PER PAGE and concatenates**, and `--slurp` is
  *rejected* together with `--jq`. So a filter ending in `| length` emits one integer **per page**
  (`"1\n0"` on two pages) — which reds any numeric guard on a PR with more than 30 comments. The
  only shape that works across pages **while still using `--jq`** is emit one line per hit and
  count the lines. Better: drop `--jq` and parse the whole array once in a real language — and
  accept BOTH shapes, because a plain `json.load` raises "Extra data" on back-to-back arrays. Cost that
  earned this: a false red on a required check, on the second attempt at the same assertion.
- **`x="$(cmd | grep -c ... || true)"` binds `|| true` to the WHOLE pipeline**, not to `grep`. An
  API failure then reads as "zero matches" instead of aborting under `set -e` — an outage
  diagnosed as a missing review — and a partial failure after a match reads as a **pass**. Split
  the fetch from the count: one assignment each, so the fetch's failure is its own exit status.
- **Do not require a marker to be the LAST line of a bot comment.** ANY trailing text at all — a
  footer, a sign-off, a horizontal rule, a postscript — then reds a complete, correct review. That
  is the whole argument and it needs no premise about how often it happens; two earlier versions of
  this bullet asserted one (a corpus count, then a repo rule requiring footers on bot comments) and
  neither survived measurement.

  **Do not hand-roll a CommonMark block parser to decide where the marker renders.** Nine review
  rounds each found a different block rule wrong — backtick closers with trailing content, tilde
  closers, blockquoted fences, list-item plus indented code, a fence quoting a fence, tab columns,
  container lifetimes, prefix equality vs block structure — because the rules interact and a
  hand-rolled parser re-derives what a parser already knows. Two earlier versions of this bullet
  prescribed rules (`^ {0,3}` openers, excluding 4-space-indented code) that the design later
  deleted on purpose; they are gone rather than annotated.

  **Decide the DIRECTION of error first, and justify it with the property, not with the blast
  radius.** For a marker gate the argument that survives measurement is: *no no-verdict path
  produces a body that counts.* The action's self-skip, a 429, a model outage and permission
  denials all end with no marker anywhere, so biasing toward counting cannot weaken what the gate
  was built to catch. **The tempting argument — "a false red is a repo-wide merge stop whose revert
  needs the same check green" — is false and was written into three files before a review checked
  it**: a *matcher* false red blocks the one PR whose comment tripped it and clears by re-posting;
  the repo-wide stop is the credit/outage case, which is a TRUE red; and an admin bypass exists
  (`docs/decisions/REVIEW-GATE-BYPASS.yaml`). Get the mechanism right before reaching for the
  consequence.

  So: keep ONE rule (a fence delimiter at column 0), state the residual, and keep a DIFFERENTIAL
  harness against a real parser with a false-red budget —
  `.github/scripts/assert_review_marker_differential.py`. **Its oracle must track `<pre>` depth,
  not strip every `<code>`**: an inline code span in a paragraph renders LIVE, it is the commonest
  real shape for a sha, and blanket-stripping it makes the harness under-count exactly the number
  the budget guards. **Sweep several seeds and ratchet the VECTOR, not a scalar against a constant.** A
  single-seed budget lets a change that multiplies false reds pass and makes a fix invisible; so
  does `max(per_seed) > K`, one level up, because seeds sitting at zero carry the slack. Commit the
  per-seed counts the way `warning-baseline.json` is committed, and fail in both directions. No
  magnitude is quoted here on purpose — an earlier version of this line stated a range that the
  branch's own committed baseline refuted within two commits, which is the note's own lesson
  happening inside the note for the second time.
  **The harness prints its own antecedents** (corpus seed, corpus size, parser version) and no
  comment quotes its figure: the first version of this line stated a bare count that had already
  drifted by the time the next commit landed, which is ADR-20260817-105845 happening inside the
  note recording the lesson.

  **A generated corpus can only find disagreements its ALPHABET can express.** Every entry in this
  one's fence list was a genuine fence opener, so no body it emitted could ever disagree with
  CommonMark about whether a column-0 delimiter OPENS a fence — and a live false red sat in that
  blind spot for two rounds (```` ```make validate``` ```` starts a paragraph, not a fence: a
  backtick info string may not contain a backtick). Widening the alphabet with NON-openers and
  indented delimiters found it, and the measured numbers then improved. **Before trusting a
  differential harness, ask what its generator cannot produce.**

  **The residual is real and now cheap to trip**: a quoted marker inside a blockquote, a list, an
  indented block or an HTML block counts, as do `<pre>`, `<code>` and HTML comments. That RAISES
  the bar on quoting; it does not make it impossible, and it never could — the gate cannot prove
  WHICH bot posted. Never paste a marker line into a comment.

  **And make the exemplar you give the model conform to the rule you enforce** — a prompt that
  demonstrates the marker indented, under an assertion that requires the left margin, reds every
  real review while the pass path stays unexercised.
  Even then, anyone willing can satisfy such a gate; say so rather than claiming otherwise.

**And the `code-review` plugin was still not enough.** With `--comment`, `pull-requests: write` and
`permission_denials_count: 0`, it posted nothing on three consecutive probes of a 5-line diff
carrying a deliberate oversell hole — 5 turns / $0.29, then 11 turns / $1.01, PR untouched, and no
"no issues found" summary despite its docs promising one. It front-loads an eligibility check
(closed / draft / **trivial** / already reviewed) plus a confidence filter, and both decisions are
invisible from the run. Two consequences worth keeping:

- **Wording in the PR itself decides whether you get a review.** The first probe was titled
  `DO NOT MERGE` with a body saying "do not review by hand" — the plugin read that as *not a real
  PR* and bailed in 5 turns. A probe that announces itself is not a probe.
- **A direct prompt (`gh pr comment` + `create_inline_comment`, "post one every time, including
  when you find nothing") has no such gate**, which is why the workflow now uses one instead of the
  plugin. Prefer the form whose contract you can read in the workflow file.

The direct prompt then **passed the smoke test on the first try** — 17 turns, 82s, $0.51, one
denial: it named the `Some(0.0)`/`None` collapse, tied it to the oversell lens, noticed the PR
description claimed behaviour was unchanged, and proposed the `let-else` fix. Four configurations
were needed to get there, and only the last one produced any evidence at all.

Separately, that probe proved a **test gap**: `cargo test --workspace` and the DB suites both go
green with the oversell hole in place, so nothing asserts that a stock-TRACKED offer at quantity 0
rejects the line.

**The overnight stall that cost 5 hours (2026-08-08, #385 API-tier wiring)** — three compounding
failures, each with a rule:
(1) **In-session cron jobs are IN-MEMORY and die silently when the remote container recycles.**
Webhook and agent-completion wakeups survive restarts; scheduled probes do not. A watch that must
survive the night needs a durable trigger (`send_later`/Routines — approve the MCP permission), and
every wake should re-check `CronList` and re-arm missing jobs. Never trust a 5-minute cron to still
exist an hour later.
(2) **A stalled executor generates no events, so event-driven supervision cannot see it.** The
probe must escalate, not just report: N consecutive no-commit/no-tree-change probes on a "running"
executor (~45 min) ⇒ SendMessage a convergence order (status + 15-minute budget: arm the PR on
green gates, or report the concrete failure). The 07:44 manual intervention recovered 5 lost hours
in two minutes — automate it.
(3) **Executors stall at the finish line, not mid-work.** The dispatch template must bind the
final actions (push, PR body, ready + auto-merge) into the SAME work unit as the last gate — "gates
green" is not done; "PR armed and reported" is done. Also: commit at phase boundaries at least
hourly (a 3-hour implementation with no commit is indistinguishable from a hang from outside), run
`cargo machete` locally (CI's lint gate does), and keep baseline checkouts of main in the
SCRATCHPAD — a stray clone in the repo root became a committed gitlink via a coordinator
`git add -A` (itself a mistake: enumerate paths in shared trees).

**Pinning third-party artifacts when the GitHub API is proxy-blocked (2026-08-08, #360)**: in this
container `api.github.com` returns 403 through the agent proxy, but `raw.githubusercontent.com`
serves release manifests fine (probe versioned paths directly, e.g.
`.../release-1.27/releases/cnpg-1.27.4.yaml` — 200 vs 404 walks the patch versions), and registry
digests need no `gh` at all: `curl "https://ghcr.io/token?scope=repository:{org}/{repo}:pull"`
yields an anonymous token whose `docker-content-digest` response header on
`/v2/{org}/{repo}/manifests/{tag}` is the digest to pin (same flow works unauthenticated on Docker
Hub via `hub.docker.com/v2/repositories/{org}/{repo}/tags`). Vendor the manifest BYTE-IDENTICAL
and record url+sha256 in a PIN.json a test recomputes — a header comment inside the vendored file
would silently break the checksum.

**The interactive decision form (2026-08-08, founder directive: keep this approach)** — when
a batch of decisions goes to the customer, do NOT deliver a wall of markdown: publish the brief as
an **interactive artifact** and let them answer at their own tempo. **This binds even when the
customer is LIVE in-session, and `AskUserQuestion` is NOT a substitute for a batch of 3+** — the
inline tool has no room for the per-lens arguments, so the customer decides blind; on 2026-08-08
(night) the #348 batch went through it, the customer had to re-raise the contract themselves
("I was supposed to have an html page… I thought it was in the rules"), and the brief was rebuilt
after the fact with the answers pre-filled for review. `AskUserQuestion` stays right for a single
quick mechanical follow-up only. The ten-decision brief closed
same-day this way where the register had been accumulating for weeks. Recipe (rebuildable in any
session): one `<article>` per decision (question, per-lens arguments, recommendation, links into
docs/proposals); per-card widgets = three radio chips ("Approve as recommended" / "Different
choice" / "Let's discuss") + a free textarea for questions/counter-views; `localStorage`
persistence so answering survives visits; a sticky bar with a live "N / M answered" count. The
RETURN PATH must be honest about artifact capabilities — there is NO shared state, so the page
cannot send answers back: build a "Copy my answers" button that serializes choices+notes to a
markdown answer sheet in the clipboard (toast: "paste it to Claude in the session") plus a
"Download .md" fallback via `window.claude.downloads.save` (declare `capabilities:
{downloads:true}`). The pasted sheet is then processed like any customer answer: record in
DECISIONS.md + ADR with VERBATIM quotes, run "Let's discuss" items through the relevant specialist
lenses, and close the loop in the same session. Reference run: BRIEF-20260808-customer-decisions.md
→ ADR-20260808-195315 + ADR-20260808-203443. Pair it with per-chapter GitHub decision-thread
issues only if the customer wants an async back-and-forth channel too (issue comments do NOT wake
a session — that channel needs a Routine or an explicit "check the threads").

## 15. Read what a gate EXCLUDES before treating it as evidence

Third in the family with §7 and §14, and the most expensive so far. For weeks `main` was green and
read as "the product works". The four-lens briefing of
[#410 "Epic: public try-before-committing demo"](https://github.com/TheCaptainCompany/captain-food/issues/410)
found the entire customer-visible half inert — checkout mounts no Stripe element, its place-order
button dispatches nothing, and the tracking route renders the not-found hero for every order — while
**22 web tests passed in 10 ms**.

Neither gate was broken. Both were *narrower than the claim they were read as supporting*, and in
each case the narrowing is one line you have to go and look at:

- `every_sdui_screen_of_every_surface_renders()` opens with a skip for `!screen.sdui` — i.e. it
  excludes exactly the two hand-written screens. The suite's name says "every screen".
- `tools/smoke/prod-smoke.sh` never opens a browser, so no page-level defect is reachable by it at
  all — and it orders `COLLECTION`, so the only thing that runs against production **never
  dispatches a delivery**: every rider hop is unexercised, daily, on a green badge.

The operational rule: **before citing a gate as evidence for a claim, read its skip conditions, its
fixture shape and its entry point** — a test that builds its own populated state instead of calling
production's call site proves the renderer, not the page. Cheap tell: unit tests that assert a state
production never constructs (here `payment_failed: true`, hardcoded `false` at the only real call
site). And when a gate's scope is narrower than its name, **rename it or widen it in the same
change** — the name is what the next reader trusts.

### Grepping for a type name does not find where that type is INJECTED

Looking for where `SessionHeader` reaches the GraphQL context, a grep for `SessionHeader` across
`crates/` returned the definition, the generated readers and a dozen tests — and **not
`routes.rs`**, the file that actually injects it. The value arrives as `.data(session)`, where
`session` came from `session_header(&headers)`; the type is never spelled. The near-conclusion was
that the anonymous cart path was unwired in production, which would have produced a "fix" for a
non-problem in a dispatch that had explicitly authorized wiring it.

**Search the injection SHAPE at the transport boundary, not the type**: `grep -n '\.data(' ` in the
route handlers and any `Data::default()` assembly (here: HTTP POST, the WS `connection_init`, and
the in-process SSR transport). Then read those handlers top to bottom — the binding that carries
the value is often three lines above the `.data(` call under a different name. Same trap applies to
axum `Extension`, `tower` layers, and anything else registered by value rather than by name.

### A handoff's "remaining work" list is a claim, not an inventory

`docs/HANDOFF-451.md` listed four outstanding Phase-2 items. Two of them — the anonymous-leg
ownership tests and the unresolvable-line test — were already written, in the very commit the
handoff described as unfinished. Trusting the list would have meant writing duplicates of tests
that already existed and passed.

**Before working any item a handoff says is owed, check the artifact**: `grep -n 'fn ' <test file>`
for tests, `git show <commit> --stat` for what actually landed. Cost here was small; the cost of
believing a claim you cannot reproduce is a test suite nobody can trust.

Same class, opposite direction (2026-08-15): **a dispatch's call-site inventory ("N call sites")
is a FLOOR, not a census.** RSO-1's "8 call sites" was 9 on first contact and 15 counted another
way. The census is the compiler: delete the old symbol and let `cargo check` enumerate every
site (ADR-20260803-234035 working as intended). Dispatches should phrase the number as "at least
N; the compiler decides", and executors should not treat exhausting the list as done.

### A "seen red" claim must name HOW the test was made to fail

Not that it failed — **how**: the clause deleted, the fallback re-planted, the stub it ran against.
**Name a mutant as the SEMANTIC EDIT and its expected failure message, never as a line range**
(2026-08-16, #598): a range rots at the next commit, and #598's dispatch named "delete
`promotion_watch.rs:44-47`", which deletes the `let mut lag_by_actor` binding the loop below uses —
a build error, and a build error is not a red. Cost: one wasted mutation run, plus the executor
having to re-derive what the mutant was *for*.
A claim a reader cannot re-run is not evidence, and the repo already contains both kinds. The good
ones say what was mutated — `crates/server/src/auth.rs` ("Seen RED by re-planting #430's
fallbacks"), `crates/infrastructure/tests/main/scope_membership.rs` ("Seen RED by deleting the
EXISTS clause from `PgOrderRepository::list`") — and neither names a commit, correctly, because the
mutation was made by hand and never committed.

**The same burden falls on "this cannot be tested", and it is the direction that actually ships
holes** (2026-08-16, #598). A written-out reason why a test would be a tautology reads like rigour
and gets waved through, where a bare "it is tested" would not. #598 recorded that its fleet-parity
gauge "has no spy test and cannot honestly have one" — its driver is a composition root, and a test
calling the emitter then finding it is a tautology. Both halves were true and the conclusion was
false: **driving the composition root is not calling the emitter.** The review disproved it by
writing the ~15-line test, and the cost was already banked — deleting the gauge REGISTRATION (the
declaration still recorded, the observable gauge never built) was GREEN, so the only monitor able
to see a split fleet was the one monitor with zero reds. Two consequences, both cheap:

- **Attempt the test before recording that it is impossible.** "I could not find a way" is a
  different, honest sentence, and it invites the next reader to try.
- **A monitor with no red is not covered, whatever the prose beside it says.** If the driver is a
  composition root, drive the composition root — it is `pub`, it resolves real values, and asserting
  against the values it RESOLVED (never a literal) is what separates the test from the tautology.

**A monitor whose HEALTHY value is ZERO needs five assertions, not one** (2026-08-16, #608 —
the general form of the two rules above, and of #598's second-drain lesson). "The gauge reads 0" is
satisfied by a dead emitter, an absent series, a hard-coded constant, an emitter that fired once at
startup, and a correct monitor — five different worlds, one observation. Three signals nearly
shipped unverified in one session on exactly that. The suite:

1. **presence** — with nothing wrong, a data point EXISTS for every declared label value, at 0,
   asserted **by equality over the full point set**. `contains` cannot see the member that stopped
   reporting, which is the failure the zero contract exists to prevent.
2. **a VALUE-DERIVED positive control** — not "it went above zero": two subjects at DISTINCT
   magnitudes must yield the right one, and a **second scenario at a different magnitude must yield
   a different number**. Without the second, a latched constant passes everything.
   **A SIXTH world hides here, and it shipped**: the query's population may be EMPTY IN THE TEST
   BINARY. #608's second gauge read `ordertracking` while nothing in that binary projected — 0 rows
   for the whole suite against 3 `OrderPlaced` in `domain_events` — so mis-spelling its predicate
   (`'AUTHORIZED'` → `'AUTHORISED'`) left the suite GREEN, and the metric was claimed "no longer
   silent" in an ADR, SPEC-LOG and STATUS on the strength of a runtime nobody had seen work. **Every
   table a monitor reads needs a row that arrived the way production makes it** (here: run the real
   `ProjectionWorker`) — a gauge over a permanently-empty population is not distinguishable on a
   dashboard from the declared-but-silent state it replaced. Corollary for `obs-metric-no-emitter`
   (validator §20) and any rule like it: it proves a name can be SPELLED at a call site, never that
   the call site is reached with a value.
3. **a SAME-SWEEP negative control** — a subject that must NOT be counted, present in the same
   state on the same tick. Without it, "count everything" passes. **Age the excluded subject too**:
   #608's negative control was vacuous at its own assertion point because the born order's hop was
   fresh, so `max(age)` over the wrongly-included row was 0 and the drop-the-exclusion mutant passed
   there, dying three assertions later. A control whose subject reads the healthy value anyway
   discriminates nothing where it claims to.
4. **repetition** — a second tick over unchanged state must re-emit. Under delta temporality a
   once-at-startup emitter drains identically to a correct one on tick 1, and *every tick* is the
   whole dead-man's-switch claim.
5. **recovery** — fix the condition and the next tick must return to 0. A gauge nobody can close an
   incident on is not a gauge.

Plus one guard the harness itself needs: assert the exporter is non-empty overall and fail with
*"spy provider not installed before first meter call"* — the `OnceLock` meter binding makes a silent
no-op provider the default failure, and it looks exactly like "nothing was emitted".

**A visibility seal must be measured with `cargo build`, and `cargo test` is not the same
question** (2026-08-16, #609, measured both ways). A `#[cfg(any(test, feature = "test-fixtures"))]`
re-export looks like a seal and is one *for release artifacts only*. With a caller planted in a
PRODUCTION source file of `infrastructure`, `cargo build -p infrastructure` failed with
`error[E0425]: cannot find function ...` while `cargo test -p infrastructure` on the identical tree
**compiled and linked**: resolver v2 (`Cargo.toml:8`) unifies a dev-dependency's feature grant into
the single unit the lib links against during a test build, so the lib itself is compiled with the
test-only export lit. Consequences, both of which cost real time here:

- **Anyone verifying such a seal with `cargo test` gets a false negative** and will report
  "unspellable" for something that is spellable in half the builds. The honest claim is
  *"unspellable in any release artifact; still spellable from the lib of a crate whose
  dev-dependencies light the feature, under `cargo test`"* — level 4 for the shipped binary,
  level 3 elsewhere. Do not round it up.
- **Prefer making the item private over gating its export**, when the call sites allow it: the
  qualifier disappears, and so does the assertion you would otherwise need to stop one unreviewed
  line from deleting the `cfg`.

**A candidate seam that needs `allow(<lint>)` to compile is the COMPILER VOTING FOR THE OTHER
OPTION** (`beck`, 2026-08-16, #609 — the generalisation, and it is the cheap one). `crates/actor_client`
sets `unreachable_pub = "deny"` in its `[lints]`, so gating only the *re-export* leaves `pub fn` in a
private module unreachable in a release build: `error: unreachable pub item`. The gated-export design
therefore has to open with `#[cfg_attr(not(...), allow(unreachable_pub))]` — suppressing the exact
lint that exists to catch "a `pub` item nobody outside uses". **Read the suppression as a verdict,
not an obstacle**: the alternative it was arguing for (make the item private) is the one to take.
Read `[lints]` in the target crate's `Cargo.toml` *at briefing*, before pricing a `cfg`-gated export
at "five lines" — here the option died after a counterfactual build instead of in one line.

**A chunk that removes a spelling has a SEMANTIC conflict with every branch open beside it, and
`git merge` cannot see it** (2026-08-16, #609 — measured, not predicted). #609 made
`actor_client::stable_partition` private; #610 merged to `main` first and brought a **new** test file
that called it. Different files, so the textual merge was CLEAN — one conflict, in an unrelated
records section — and the merged tree **did not compile**:
`error[E0425]: cannot find function 'stable_partition' in crate 'actor_client'`. Two things follow:

- **The compiler is the merge gate here, so run the BUILD after any merge into a
  removal chunk**, before believing a clean `git merge`. A conflict-free merge of a removal is
  evidence of nothing; this one had zero conflicts in the affected language.
- **It is also the proof the seal works.** A parallel branch reintroduced exactly the hand-copied
  `stable_partition(&id, 5)` the chunk exists to prevent, within hours, written by someone who had
  no reason to know — and it could not land. Before the chunk it would have compiled and stamped a
  fixture onto a lane derived from a literal. That is the whole argument for level 4 over a review
  habit, and it arrived unprompted.

**When a chunk's method is "make X unspellable", every existing spelling of X is a candidate
INCIDENTAL PIN — enumerate what each one was holding before deleting it** (`vernon`, 2026-08-16,
#609; this is the rule that would have caught that chunk's checkpoint MISS). Four test assertions
spelled `stable_partition(&cart_id, 5)`. Converting them to read the declaration was the whole point
and also silently removed the only thing in the repository pinning Cart's and Order's declared
widths — a contract over STORED rows, where a change is a migration (ADR-20260802-220402), so the
"cleanup" was a gate weakening that every gate would have reported green. A spelling being redundant
with the declaration is exactly what makes it a pin; the redundancy is the point, not the defect. Ask
of each site: *what would notice if this expectation and the thing it duplicates stopped agreeing?*

Two fabricated claims shipped on one branch (`crates/server/tests/graphql_cart_read.rs` and
`crates/application/src/pricing.rs`), both asserting a red against a stub that the same commit had
introduced alongside its own tests. Reviewers caught both; no gate could have. A scanner was
proposed and abandoned after checking the corpus: the fictions and the honest records use the same
trigger words, so a phrase rule would have failed the two checkable claims and passed anything
containing seven hex characters.

**If no red was observed, say so plainly.** `crates/application/src/pricing.rs` (the HONESTY NOTE on
`a_line_with_an_option_at_quantity_two_prices_to_3400`) is the model: it states the test was born
green, quotes the claim it previously made, explains why that claim was false, and then says what
can honestly be said instead — that the evidence is ordinary, the assertion of specific values a
wrong implementation would not produce.

**Restore a plant with `git checkout -- <path>`, NEVER from a copy you took yourself.** The rule
above is what creates this hazard: proving red means editing a committed file, and the obvious
mechanics — `cp <file> /tmp/x` before, `cp /tmp/x <file>` after — are **not re-entrant**. Plant
once, prove red, restore from `/tmp`; plant a *second* mutation in the same file and the `cp`
snapshots the **already-planted** text, so the "restore" writes the first mutation back and the
tree is silently wrong. It survives `make validate` whenever the mutation is one the validator was
never taught to refuse, which is exactly the case a red-first proof is about, and the diff that
reaches review then contains a deliberate defect nobody wrote on purpose. Git already holds the
pristine copy: `git checkout -- <path>` is idempotent, needs no bookkeeping, and cannot restore the
wrong generation. Verify with `git status --short` before every gate run, not only at the end —
a clean tree is the only evidence the plants are gone.

**Pay for the red ONCE.** Plant-after-green pays for the mutation, the run, the restore and a
re-verification; four cheaper habits get the same evidence
([ADR-20260816-020752](../../adr/ADR-20260816-020752-the-loops-context-budget-a-dispatch-card-snapshot-semantics-and-phase-commits.md)
decision 5): **(1) red-FIRST** — write the assertion before the rule it checks, and the red is a TDD
byproduct that costs nothing extra; **(2) mutate DATA, not Rust source** — a deliberately bad spec
fragment pushed through `make validate` proves a validator rule with **no recompile**, where editing
a `.rs` file buys a full rebuild; **(3) BATCH** independent mutations whose tests fail
*distinguishably* into one run; **(4) never re-run the full suite "to confirm green after revert"** —
an empty `git diff` plus the prior green already is that evidence, and the extra run is a whole gate
cycle bought for zero information.

### Running a mutation by hand: `git checkout <file>` reverts to HEAD, not to your work

The mutation loop is edit → run → **revert**, and `git checkout <file>` is the reflex for the third
step. It is only correct when the file is COMMITTED. On a multi-phase branch the fix under test is
usually still in the working tree, and the checkout throws it away silently — the mutation is
reverted and so is the thing being proved (2026-08-17, #623: a 50-line `verdict_of_error` rewrite,
gone, and the give-away was a still-red test after a "revert"). Two habits, both cheap:

- **Commit the fix BEFORE mutating it.** A red mutation run wants a clean base anyway, and the
  commit is what makes `git checkout` mean what the reflex assumes.
- **`git checkout` cannot touch an UNTRACKED file at all** — it errors with *"pathspec … did not
  match any file(s) known to git"*, which reads like a typo and is actually the safe direction. A new
  module mutated before its first commit has to be reverted by editing it back.
