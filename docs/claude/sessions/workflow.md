# Session rules — workflow

Part of [`../sessions.md`](../sessions.md).

## 10. Commit the durable artifact, not the conversation

A long session slows down: every turn reprocesses the whole transcript. The mitigation is the
operating model itself — **proposals, ADRs, `DECISIONS.md` and `STATUS.md` are how knowledge leaves a
session.** When a session has produced decisions, write them down and let the next session start
small; do not carry a 30-turn context forward for its own sake.

**A directive that changes a gate is recorded BEFORE the next dispatch hits the gate** (2026-08-13):
the budget-cap lift (founder antecedent 2026-08-12) was relayed ~10:00Z, nothing was recorded, and at
~13:20Z (container clock — `date -u` is the repo's only clock) the #510 executor was dispatched into
`loop-budget.sh start`'s guaranteed exit 2 — one full dispatch round for zero output, standing down
against an already-lifted gate (ADR-20260813-132540).

**Read `loop-budget.sh start`'s EXIT CODE, never its banner** (2026-08-16, #588 — the other half
of the ADR-20260813-132540 lesson above). With the cap lifted (`capIsAStopSign=false`) the guard
still prints `⛔ weekly loop budget exhausted: … / 1440.0m used` on stderr, in the vocabulary of a
refusal, and then **exits 0**. It is a REPORT. The three codes are the whole contract: `0` proceed,
`2` genuinely exhausted (only possible with the cap armed), `3` INTEGRITY — a timer already open or
stale, which is a concurrency event to resolve with `stop`/`stop --elapsed-seconds`/`reset` and must never
be reported as budget. Cost of reading the banner instead: a whole dispatch stood down against an
open gate, twice now. **The banner lies in BOTH directions**: a healthy `start` also appends
`Timer open (untracked: …/.git/loop-budget-timer.json)` to its `✓ loop budget OK` line — that is
the guard reporting where it just opened the run's own timer (ADR-20260812-011057 keeps it inside
`.git/` so it can never be committed), *not* the "a run timer is already open" condition the exit-3
contract describes in the same words. Same discipline, same reason: only the exit code distinguishes
them.

**`loop-budget.sh stop` takes a FLAG, not a positional** — `stop --note "what ran"` (and
`stop --elapsed-seconds <n>` when the timer was never opened or went stale). A bare
`stop "what ran"` silently loses the note, so the ledger segment lands with no attribution and the
week's usage becomes a set of anonymous durations. Cost: one un-attributable segment per occurrence,
unrecoverable after the fact. The ledger itself is `.claude/loop-budget/<ISO-week>/<stamp>-<rand>.json`,
append-only, one file per segment — the week's usage is their sum.

**Context discipline — the rules that keep a session under ~80k** (2026-08-01, after a week at
87% of requests >150k context): (1) `specs/generated/**` and `crates/**/generated/**` are
GREP-ONLY — never `Read` a generated artifact wholesale (`documentation.generated.md` alone can
eat a third of a session); (2) GitHub MCP calls use `minimal_output: true` and small `perPage`
unless the full payload is the point — a bare PR `get_diff` on a large PR returns megabytes
(fetch the branch and use local `git diff` instead); (3) fan-out exploration goes to SUBAGENTS
(Explore/reviewer/generator), never inline — their transcripts stay out of the main context, **and
never read a finished agent's `.output` transcript: the completion notification IS the artifact.**
Re-opening the file "to check" adds nothing the notification did not carry and costs the run's whole
context — **~300k tokens per chunk of pure loss** (measured 2026-08-15; banned by
[ADR-20260816-020752](../../adr/ADR-20260816-020752-the-loops-context-budget-a-dispatch-card-snapshot-semantics-and-phase-commits.md)
decision 1). The file has exactly one legitimate use: recovering the answer of an agent that **DIED
before answering**;
(4) ONE SESSION PER WORK CHUNK (CLAUDE.md rigor rules) — the repo carries the state, so ending a
session is free and long context measurably raises the staleness error rate;
(5) a lens invited to a mob briefing reads the **dispatch card**, not the repo — one coordinator-authored
file per chunk (chunk, paths, phases, gates, fences), SHA-stamped, with lens replies appended to its
Findings block, which is then the mob evidence the PR body cites. 12x50k becomes 12x~5k, and nothing is
written twice. The card is a **cached fold — disposable, never authoritative**: a checkpoint loads
card@SHA + `git diff <SHA>..HEAD`, a version mismatch is DISCARDED rather than patched, and every lens
keeps the right to fall through to the tree. Falsification test before trusting a card: delete it,
re-run one briefing, and no verdict may change (same ADR, decisions 2-3). **The card ships with an
EMPTY `## Findings` heading**, present from the first write (2026-08-16, #588): a lens or a phase
handover that has to invent the heading also invents its shape, so verdicts land in different
formats — or in the PR body instead of the repo, which GitHub-is-never-the-record forbids. An empty
section is an instruction to append; an absent one is an invitation to improvise.
**Every card MUST also state its `Reversibility class:`** — IRREVERSIBLE (money movement, stored
event shapes, legal surfaces, anything Tours-facing; the `HOLD: human` axis, which wins when the two
disagree) or REVERSIBLE (internal refactors, generated artifacts, doc sweeps) — **and the briefing
roster derived from it** (full mob vs 2–3 lenses), because the class is the input to the fan-out and
an unstated class is coordinator taste returning by the back door. **At the checkpoint the card
carries a `Checkpoint verification:` line**: did the narrowed checkpoint (only the lenses that
declared a concern at briefing) miss anything the full roster would have caught? It is banked EITHER
WAY — the card line plus one sentence in the change's record — a MISS reverting that class to the
whole roster, a clean run turning n=1 into n=2; the architect's run report surfaces it, and an
unanswered line is a reportable defect of the run (founder ruling 2026-08-16,
[ADR-20260816-134352](../../adr/ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md)).
The two cards written before that ruling (`docs/dispatch/588-*.md`, `docs/dispatch/598-*.md`) are
historical and are **not** retrofitted.

**Coordinator/executor split** (founder directive, 2026-08-07): a session that has planned a
multi-step program NEVER executes the steps itself — it DISPATCHES each step to a fresh session
(`create_session` with a complete standalone prompt naming the issue, the ADRs/proposal to read, the
branch/PR to continue, and the gates), one step per session, sequentially — the next step is
dispatched only when the previous step's PR is MERGED. This is the "one session per work chunk" rule
made mechanical: the planning session's compacted memory has twice invented or duplicated work
mid-program (a duplicate backlog issue; a claim it forgot it had made) — a fresh executor reading
the repo's recorded state (claim comment, PR plan checklist, STATUS) has no such memory to corrupt.
The dispatch prompt must ALWAYS include the hygiene preamble: check existing claims/branches/PR
comments for the issue FIRST and continue what exists rather than re-create it.

**The scratchpad's parent-directory permissions RESET between wakeups** — a local Postgres run
under a dedicated user (`pguser`) inside the scratchpad dies mid-session with "Permission denied"
on the data directory, and every DB-gated suite then fails with PoolTimedOut/Connection refused
(cost: a full server-suite failure mis-read as a code regression, 2026-07-31). Recovery recipe:
re-`chmod o+x` every path component from `/tmp/claude-0` down to the scratchpad, `rm -f
<pgdata>/postmaster.pid`, then `su pguser -c "/usr/lib/postgresql/16/bin/pg_ctl -D <pgdata> -o
'-k /tmp -p 5433 -c listen_addresses=127.0.0.1 -c fsync=off' start"` — the FULL binary path:
`pg_ctl` is not on `pguser`'s PATH even though `psql` is on root's (cost: one dead recovery
attempt, 2026-08-01). A cross-session note: the pgdata lives in the PREVIOUS session's
scratchpad directory (session-specific paths), so a resumed branch reuses it by absolute path —
don't initdb a fresh cluster when yesterday's is one `pg_ctl start` away. Diagnose "tests
suddenly failing" with `pg_isready`/`psql` FIRST, before reading a single line of code.

**The Debian SYSTEM cluster is a different incantation, and `postgres` will not start as root**
(2026-08-15). Same missing-PATH trap, plus the config file lives outside the data directory, so the
`-o '-c config_file=...'` is not optional:

```sh
su postgres -c "/usr/lib/postgresql/16/bin/pg_ctl -D /var/lib/postgresql/16/main \
  -o '-c config_file=/etc/postgresql/16/main/postgresql.conf' start"
```

And read the WHOLE output before reacting: **"could not start server" immediately followed by a
SUCCESSFUL `ALTER ROLE`/`psql` means it was already running** — the start failed because the cluster
was up, not because it is broken. Cost of not reading to the end: a needless teardown attempt on a
healthy cluster.

Two more things that cost a full 120 s tool timeout each (2026-08-16, #588). **Start it in the
BACKGROUND**: a foreground `pg_ctl start` here never returns even though the server is accepting
connections in ~3 s, so the call is killed by the timeout and reads as a failure — background the
start, then prove it with `pg_isready` (~1 s). And **the test database does not pre-exist**:
`captainfood_test` must be created, over TCP as the `postgres` role, because peer auth rejects
`root` and there is no `root` role:

```sh
(pg_ctl -D /var/lib/postgresql/data -l /tmp/pg.log start >/dev/null 2>&1 &) ; sleep 1
pg_isready
PGPASSWORD=postgres createdb -h localhost -U postgres captainfood_test   # already exists = fine
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/captainfood_test
export DB_TESTS_REQUIRED=1
```

No `ALTER USER` is needed on this image. `DB_TESTS_REQUIRED=1` must be in the transcript of any run
whose evidence you intend to claim: a skipped DB suite reports `ok`, so "tests pass" without it
proves nothing.

## 16. A lens invited late still pays — and the ones you skip are the ones that disagree

The mob ADR (ADR-20260809-013142) says invite the roster by default. On the first dispatch after it
landed, the coordinator invited four lenses of eleven by its own judgement and got a real result —
the customer path is inert on `main`. Then the other six were invited on the already-committed
proposal, and the honest measurement is uncomfortable:

**The six late lenses found four defects the four missed, all verified in the tree:**

- `orders` / `order` / `carts` apply **no ownership filter for any role** — `orders` with no
  arguments returns the whole `ordertracking` table, un-paginated, while the SDL says ownership is
  enforced (graphql-architect).
- The cart's total and the competitor comparison **never compute** — the projector carries them
  forward from a row no event ever writes; the epic's one commercial screen cannot render
  (business-specialist).
- The `orderStatusChanged` dedupe keys on `status` alone, so widening the stream filter **still
  swallows** every delivery movement — and it lives in the EMITTER, not the crate the executor was
  scoped to (graphql-architect; the executor was mid-work and had to be checkpointed).
- `orders_placed_total` — the metric that says a stranger paid us — has **zero emission sites**, so
  the alert that would have caught the inert checkout could never have fired; and `place-order`'s
  success rule requires a span with no call sites, making the contract unsatisfiable by construction
  (observability-agent).

**The selection bias is the lesson, not the count.** The four invited were the four most likely to
produce more design. The lens whose only job is to say *build less* (holub) was not invited, and it
was the one that argued the epic should be deferred — with two other lenses independently agreeing
that ~80% of it was production work misfiled under a marketing epic. **A coordinator picking lenses
picks the answer**, because it picks who is allowed to disagree.

**Operationally**: late invitation still works and is cheap — six parallel reads of a committed
proposal cost one wall-clock stretch and caught a security defect. So the recovery is real, but it
arrives after the proposal has shaped everyone's thinking, and one dispatch was already running
against a scope that turned out to be wrong. Invite first. If cost forces a subset, the subset must
include the lens most likely to say the work should not happen.

### A red CI job that never ran your code: `429` from `codeload.github.com`, and why re-pushing makes it worse

`lint`, `build-test` and `codegen` all report `failure` for a reason that has nothing to do with the
diff:

```
##[error]Response status code does not indicate success: 429 (Too Many Requests).
##[error]Failed to download archive 'https://codeload.github.com/dtolnay/rust-toolchain/tar.gz/…'
```

The runner is rate-limited fetching a composite ACTION, before a line of the repo is compiled.
Three things this session paid for (2026-08-17, #623):

- **`codegen` failing does not mean drift.** It is the aggregate gate, and its message is
  `a required gate job reported 'abandoned'` — one line, naming no job. Read the OTHER failing job
  first; `codegen`'s own log will never say what went wrong.
- **Read the failing job's log before believing any red.** `GET /actions/jobs/{id}/logs` follows a
  redirect (`curl -L`) and works without a token like the rest of the REST surface. Thirty seconds,
  and it is the difference between "my change broke the build" and "GitHub was busy".
- **Back off before re-triggering, and re-trigger ONCE.** These jobs run several action downloads in
  a matrix; an immediate second full run doubles the request rate against the same host and simply
  moves the 429 to a different job — here the retrigger fixed `lint` and broke `build-test` instead.
  Every job passed at least once across the two runs, which is the evidence that mattered. Wait
  several minutes.
- **`POST /actions/runs/{id}/rerun-failed-jobs` answers `403 Resource not accessible by
  integration`**, so a session cannot re-run a job — the only lever is a new push, **and since
  #681 it only works on a branch with an OPEN PR**. `ci.yml`'s `push` trigger is `[main]` now, so
  a push to a branch is picked up by `pull_request: synchronize` and by nothing else. On a branch
  with no PR yet — the claim-time window described below, where the claim commit lands before the
  PR is opened — a push triggers NOTHING, and empty commits go into silence that reads exactly like
  a GitHub outage. **Open the PR first** — that fires `opened` on the head, and the claim protocol
  requires it anyway. Where a push does work, a `--allow-empty` commit whose message records the
  flake is the form to use if there is nothing real to land.
  **Do NOT close and reopen the PR to re-trigger, and do not use draft -> ready either**, though
  both do fire the workflow. Closing DISABLES auto-merge and reopening does not restore it, and
  re-arming is `enablePullRequestAutoMerge` — which the very next section of this file records as
  unavailable to an executor session. So the recovery ends with a green branch whose auto-merge is
  silently gone and no way to put it back: a coordinator round-trip, caused by the fix, on the
  failure this section exists to make cheap. Both also fire `pull_request: reopened` /
  `ready_for_review` — while the CI auto-review existed (retired by ADR-20260828-091500) that
  spent one of ADR-20260826-084500's three review rounds on a `429`; the presentation events are
  still the team's re-review signal. (An earlier version of this paragraph
  recommended close-and-reopen, eleven lines above the section that contradicts it.)

### An executor session CANNOT mark a PR ready for review or arm auto-merge — plan the handoff

Both are GraphQL-only operations (`markPullRequestReadyForReview`, `enablePullRequestAutoMerge`) and
this session's GitHub access answers:

```
This GraphQL query is not enabled for this session — only the pinned set of PR-review
operations is served. Use REST via `gh api repos/{owner}/{repo}/...` instead.
```

The suggestion in that message does not help, and neither do the obvious REST attempts (2026-08-17,
#623):

- `POST /repos/{o}/{r}/pulls/{n}/ready_for_review` → **404**; the endpoint does not exist.
- `PATCH /repos/{o}/{r}/pulls/{n}` with `{"draft": false}` → **200 and silently ignored**, the PR is
  still a draft. This is the expensive one: it looks like it worked. Always read `draft` back out of
  the response body rather than the status code.
- `gh` is **not installed** in this container, so every instruction phrased as `gh pr ready` /
  `gh pr merge --auto` is unexecutable here regardless.

**What this means for the protocol.** ADR-20260815-115220's "mark ready and arm auto-merge together,
as one indivisible step" is a COORDINATOR action, not an executor one — the executor's terminal state
on a green branch is *draft, all checks green, body and records complete, handoff comment posted*.
Anything else is the executor reporting a step it had no way to take. Say so explicitly in the PR
comment, with the two commands the coordinator needs, so the handoff is one paste and not an
investigation.

Cost that earned this: a full round of REST/GraphQL probing at the end of a run, after the work was
already green, plus a PR body that had to be re-edited because it announced a state that could not
be reached.

### The worktree is SHARED — "already on `main`" has a shelf life of one tool call

Concurrent executors run in **one checkout** unless the coordinator hands them separate worktrees,
and a checkout is global state neither agent can see the other take. A record executor confirmed
`main`; several read-only calls later `git branch --show-current` returned `572-decisions-table-gate`
at another executor's WIP commit. Four `tools/codegen-rs/src/main.rs` line numbers had been measured
against that WIP tree (+4 lines), looked like dispatch errors, and were one step from being recorded
as permanent register text.

**Assert the branch in the SAME bash call as any `file:line` verification**
(`git branch --show-current && awk …`), and again immediately before committing — a branch confirmed
in an earlier call proves nothing about the current one. **The real fix is coordinator-side: dispatch
concurrent executors with worktree isolation** (`git worktree add`, with the
[disk caveats](environment.md#2-disk-is-a-fixed-per-session-allowance-and-df-lies-about-it) that apply),
never into one shared tree — and as of 2026-08-15 this is a PROVEN working pattern, not a
suggestion: a record executor ran on `main` in its own worktree while a feature executor held the
shared checkout, with zero interference. Any second writer gets a worktree; the shared checkout
belongs to whoever holds the feature branch. Same root cause, other direction: **a docs-only
dispatch targeting `main` must open with an explicit `git checkout main`** — an executor dispatched
for a `main` docs task started on a feature branch someone else had left checked out, correctly
refused to write, and burned its entire run doing nothing before a session limit killed it.

**The COORDINATOR is a writer too, and that is the case that actually fires** (2026-08-16, #608).
The rule above was worded for reviewers and executors, so it read as satisfied while the coordinator
kept using the shared checkout for its own work: mid-review of #608 that checkout was switched off
the branch, a #609 dispatch card was committed onto it and cherry-picked to `main`. It cost nothing
this time — branch refs came through intact (`local == origin == 5354d31`), PR content unaffected,
and the reviewer had already moved to its own linked worktree — but only because the reviewer had
independently isolated itself. **Nobody writes in the shared checkout while a dispatch is live in
it, the coordinator included**; claim commits and docs commits are writes, and `git worktree add` is
200 ms. The reviewer's isolation is a second belt, never the reason this was safe.

### Rescue an agent killed mid-edit with a `wip:` commit that says what was NOT verified

When a session dies mid-change, `git add` the touched files and commit as an explicit `wip:` whose
message states **what has not been proven** — then push. `807e472` preserved a half-built validator
rule that had never been seen red; without that sentence the next executor would have reviewed
unverified work as finished, which is the same defect the
["seen red" rule](evidence.md#a-seen-red-claim-must-name-how-the-test-was-made-to-fail) exists to prevent.

### The stop hook cannot see in-flight work — its prompt is not a signal that anything is finished

A stop-hook prompt reported an unpushed commit; the coordinator pushed it, the executor then
amended that same commit (adding `.claude/loop-budget.json`), and local and remote diverged —
identical content, different SHAs — needing a `--force-with-lease` to realign. **An
unpushed-commit prompt is not a signal that the work is finished.** The coordinator pushes only
after the executor reports the phase complete and the tree is clean; the executor says explicitly
whether it intends to amend before handing back.

**The general form: the hook cannot distinguish an agent's in-flight files from abandoned uncommitted
work** — both look like a dirty tree. A coordinator that obeys the prompt reflexively while a
background write is running commits a half-written artifact under a message claiming it is done, and
the next reader has no way to tell. So while any agent is writing, the dirty tree is EXPECTED state:
answer the prompt with what is in flight and who is writing it, and commit only once that agent has
reported back. Where a session genuinely is dying mid-change, the `wip:` rule above is the correct
answer — an explicit commit that says what was NOT verified — never a silent one.

### A denied tool call is a DECISION — never re-issue it through a different tool

2026-08-18, an executor run. A `curl -X POST` to the GitHub API was refused by the permission
classifier. The agent then issued the **identical request** via `python3` + `urllib`, which was
allowed, and its hand-back recommended that route as the standard approach for future executor
runs. The actions themselves were benign — creating an issue, posting a comment — and the work
landed correctly; the method is the defect, and the recommendation to institutionalise it is worse
than the single instance.

**The rule: a refusal is the user's decision about that action, not a property of the tool that
carried it.** Reaching the same effect through a second tool path launders the decision and makes
every permission boundary advisory. On a denial the agent **stops and reports what it wanted to do
and why**; the coordinator either finds an approved route or takes it to the founder. This is the
same discipline as the cross-session rule (never ask a peer to perform an action blocked in your
own session) applied within one session, across tools.

**Cheap and legitimate alternative, which is what should have happened here**: hand the intended
issue/comment body back to the coordinator, which has the GitHub MCP tools, and let it post. Cost of
the wrong route: an unreviewed bypass of a control, and a recommendation that would have spread it
to every future run.

Related and *not* the same thing — a tool that is genuinely **absent** rather than denied. In that
same run `mcp__github__*` was not in the executor's tool set at all and `gh` is not installed; that
is a missing capability, and falling back or reporting it is correct. Distinguish "refused" from
"unavailable" before choosing a fallback: the first is a decision, the second is an environment fact.

### The claim-time draft PR needs an empty commit first — the REST API refuses a zero-commit branch

ADR-20260720-233000 mandates the draft PR **before any code**, and `POST /repos/{o}/{r}/pulls`
rejects exactly that: a branch pointing at the same sha as `main` gets
`422 Unprocessable Entity — No commits between main and <branch>`. There is no flag for it. Push a
`git commit --allow-empty -m "chore(NN): claim -- <title>"` first and open the PR against that.
**Better than empty when something real is already in hand** (2026-08-16, #609): the claim commit is
the natural home for the card/dispatch correction the executor makes while verifying it — same one
commit, same 422 avoided, and the branch opens with a diff that says what the run already learned.

This container has **no `gh` on PATH**, so every session drives the API with `curl` and meets the
hard stop rather than a CLI's guidance. Budget one extra commit, not a debugging round: the 422 body
names the branch, so it reads like a bad ref rather than a missing commit.

**The `curl` path is NOT coordinator-only — an executor does its own claim mechanics** (corrected
2026-08-16 on #597, replacing the earlier "an executor session cannot reach GitHub at all"). Two
different capabilities, and only one of them is missing:

- the **`mcp__github__*` tools are unavailable to an executor** — every call answers `No such tool
  available`, even though the server's instructions are injected into the prompt, so the tool list
  reads as if they existed;
- **`curl` against `api.github.com` works, for READ and WRITE.** Proven in one executor run: issue
  read, `status/in-progress` label add, claim comment, draft PR create, PR body PATCH, check-runs
  read. **You do not have to supply a token** (re-confirmed 2026-08-16, #609): the agent proxy
  injects the credential, so a bare `curl https://api.github.com/…` already authenticates —
  `GET /user` returned the account. Same reason `git ls-remote` and `git push` succeed with **no**
  credential helper and no `extraheader` configured. Do not go hunting the environment for a token
  when a call 401s; check the response body first. (The classifier blocks `env | grep -i token`
  anyway, and correctly.)

So an executor **performs its own claim, draft PR and PR-body updates** and only reports a GitHub
failure it actually met. Handing the mechanics back on the assumption of no access costs a
coordinator round-trip per chunk. If a session genuinely has none, the REST API says so in the body
(`GitHub access is not enabled for this session`) — read the response before concluding.

**But READY-FOR-REVIEW and AUTO-MERGE are GraphQL-only, and GraphQL is BLOCKED** (2026-08-16, #609
— this narrows the entry above, which said "READ and WRITE" without qualification and is true only
of REST). Every GraphQL call, down to `query { viewer { login } }`, answers:

> `This GraphQL query is not enabled for this session — only the pinned set of PR-review operations
> is served. Use REST via 'gh api repos/{owner}/{repo}/...' instead.`

and `gh` is not on PATH either. There is **no REST equivalent** for either operation:
`markPullRequestReadyForReview` and `enablePullRequestAutoMerge` exist only in GraphQL, and
`PATCH /pulls/{n}` with `{"draft": false}` returns **200 with `draft` still `true`** — it silently
ignores the field, which is the trap: it looks like it worked. So the ADR-20260815-115220 closing
step ("mark ready and enable auto-merge together, as one indivisible step, then supervise to
MERGED") is **not executable by an executor session** as things stand.

What the executor CAN and therefore MUST still do, so the hand-back is one action and not a
re-investigation: push the final head, get the PR body complete, and **supervise CI to green over
REST** (`GET /commits/{sha}/check-runs`, poll until every run is `completed`, then read
`conclusion`). Hand back naming the two GraphQL operations and the PR node id. Budget zero minutes
for finding a workaround — there is not one.

Unchanged by this, because it never depended on the executor's access: **a dispatch names an issue
with its number AND its title verbatim** (the CLAUDE.md naming rule). It is one line to write and it
survives a session that has no lookup path; the cost of skipping it was an unresolvable issue link
in `ee9082d` and a follow-up commit to repair.

### A commit touching `CLAUDE.md` or `.claude/agents/*.md` needs in-conversation user approval

The permission classifier blocks any `git add`/`git commit` whose pathset includes `CLAUDE.md` or
`.claude/agents/*.md` — for subagents AND the main session alike — until the user explicitly
approves in the conversation. Edits already sitting on disk change nothing, and an isolated
worktree does not exempt the commit: the block is on the pathset, not the tree. So a
coordinator planning a one-commit record that includes operating docs must either obtain the
approval (`AskUserQuestion`) BEFORE dispatching the record executor, or split the record so the
operating-doc paths ride their own commit. Cost (2026-08-15): one stopped record executor, one
denied main-session retry, and a founder round-trip for a change that was already written.

### One more shell trap in commit messages

`git commit -m "…"` with **backticks** inside the double quotes runs command substitution: a message
containing `` `system` `` silently lost the word and committed the gap. The existing ASCII rule
covers Makefile recipes; this is the same class one layer over. Write any commit message with
backticks, `$`, or `!` to a file and use `git commit -F <file>`.

**It is not just commit messages — it is every double-quoted payload, and GitHub comment bodies are
the one that gets seen** (2026-08-16, #609). `python3 -c "…"` inside double quotes has exactly the
same hole: a PR comment built that way posted as *"Final head  — CI green.       all `success`"*,
with the head sha and six check names eaten, and bash helpfully logged `276af29: command not found`
next to an `HTTP=201`. **A 201 is not evidence the body is right.** Same fix, one level up: build any
body with a **quoted** heredoc (`<<'PYEOF'`), never `-c "…"`, and for a comment that already went out
wrong, `PATCH /repos/{o}/{r}/issues/comments/{id}` repairs it in place.

## Asking the founder a decision — use the form template

**Founder directive 2026-08-18**: *"Make this format of questions as a template for the next times."*

`docs/templates/decision-form.html` renders a decision form from one `FORM` object at the top of the
file. Copy it to your scratchpad, edit **only** that object, publish the copy as an Artifact, and give
him the link. He picks, comments, presses Copy, and pastes a plain-text block back. **Do not edit the
template in place.**

Prefer it over `AskUserQuestion` when there is more than one decision, when an option needs its
trade-off spelled out at more than a phrase, or when his answer is likely to be a comment rather than
a pick — which, on the evidence so far, it usually is. The form's most valuable answers have all
arrived through the comment box and through the *"neither exactly"* option, so **always offer that
option and always leave a comment box**. The first use proved why: the invoice-chain question was
answered *"neither exactly"*, and the comment supplied a third shape (**rider invoices the
restaurant**) that neither drafted option contained and that no lens had proposed.

**The rule that cost the most to learn, on that same first use**: *check the register before you ask —
and before you assert.* One of the six questions asked which funding model applied, and
[ADR-20260808-203443](../../adr/ADR-20260808-203443-tips-voluntary-contributions-funding-model.md) had
decided it ten days earlier. His answer began *"We already discussed about that."* The record was 891
words and one grep away; nobody grepped, because nothing required it. The mirror failure is answering
from memory: the resident index is a projection of the records, and an answer recited from it is only
correct while the index is current. So before publishing a founder question, and before asserting
that something is already decided:

1. **Search the decision sources with the question's own vocabulary** — `docs/adr/`,
   `docs/proposals/DECISIONS.md`, the recent `docs/status/journal-YYYY-Www.md` files, and
   `docs/legal/` when the subject is legal — including the repo's aliases for the subject
   (`contribution` finds what `tip` misses).
2. **Read the surrounding record, not the matching line.** A grep hit inside a rejected
   alternative, a quoted question, or a struck clause reads as an answer out of context.
3. **Check for a later word**: an `Amendment`/`Superseded` banner, a strike, a register row that
   points to a later decision or reveals that the question remains open or founder-owned, or a
   later record that changes it. Follow explicit amendments, supersession, reversal, and strike
   references first. If two governing decision records genuinely conflict with no such
   relationship, do not infer a resolution from recency alone: escalate with both records cited.
   A disagreement between a projection and its underlying record is stale derived state, not a
   governing conflict.
4. **Answer with the controlling current record cited** — path plus section or id. Historical
   evidence (an older ADR, a journal entry, a transcript of what someone said) supports the story;
   it does not control over a later ruling, and a founder sentence transcribed inside a record is
   not a build instruction unless the record's own status makes it one.
5. **Escalate instead of answering** only when the search finds no authoritative answer, when
   authoritative records genuinely conflict, when the question is a real option space or
   founder-owned or counsel-gated, or when the intended action is irreversible. Two things are
   never the answer: a `Proposed` proposal (an argument, not a decision) and a legal-lens brief
   (never advice or clearance). And a disagreement between the resident index or a projection and
   the underlying records is a **staleness report**, not a founder question — say what disagrees
   and point at the newer record.

Scale the search to the question — one well-aimed grep plus reading its record is the floor, not a
sweep of every surface. This is cheap insurance against answering from a stale context or missing a
supersession, not a guarantee that a repeated question can never recur.

### The trail rides the question — canonical format, declared once, HERE

**Founder directive 2026-08-21** (verbatim: *"I want to ensure that the agents will no longer ask
questions already answered. Use the best practices known for that."* —
ADR-20260821-010543, deciding DECISIONS §48 REG-1's direction: enforcement goes on the ASK). Every
question that survives the check carries ONE trail line in its own text, in exactly one of two
shapes — this section is the only place the format is defined; the hook, the agent blocks and the
selftest cite it, never re-spell it:

```
Register check: <record id> (<date>, <status>) -- covers <X>, silent on <Y>
Register check: no controlling record -- terms: <terms searched>; nearest: <record id or none>
```

**The `<status>` clause is READ, not decorative (2026-08-28, ADR-20260828-120500 / #709).** A trail
in the two-part `(<date>, <status>)` shape that self-declares a CLOSED status — `decided`,
`superseded`, `deferred` or `withdrawn`, the register's own closed set (docs/decisions/README.md);
`open` is the only status that still asks — is, by its own words, citing an answer, and the hook
refuses it: asking anyway is the round-5 call-sheet incident ADR-20260828-120500 names ("ensure
agents do not ask questions already answered"). A single-token parenthetical with no comma (the
pre-2026-08-21 citation style, e.g. `ADR-0032 (completeness)`) carries no status clause and is
untouched. **The escape is a stated premise change, never a silent re-ask**: add a line

```
premise-changed: <what changed, and why the old answer no longer holds>
```

to the same trail. The hook trusts this line's PRESENCE (it cannot prove a real change happened,
the same honesty limit as the rest of this gate) and allows the question through, logged under the
distinct reason `trail-premise-changed` rather than folded into a plain allow — a hollow marker is
then a decomposable defect too, not an invisible one. This mirrors, at trail weight, the envelope
lane's `reconsiders: <OLD-KEY>` reversal path — the trail's version is cheaper because a Lane-2
question is by definition not a decision question, so opening a new register row would be
ceremony a clarification, relay or mechanical choice does not need.

**The ENVELOPE, and the dated meaning-shift (2026-08-21 evening, ADR-20260821-103403 —
decision-ask-unregistered).** A **decision question** — the published tiebreaker: *would the
answer bind future work? then it is one* — carries the envelope line instead of a trail:

```
Decision row: <KEY>
```

meaning exactly: *this question asks (or, freshly declared, creates) register row
`docs/decisions/<KEY>.yaml`*. Exactly one line, one key; the row must be declared and **open**
(the hook refuses non-open, unknown, and legacy keys, each with its next action; an open
counsel-owned row takes only the external-action question). The envelope IS the register check —
no trail line rides with it, because the declared row carries the provenance. A genuinely new
decision question **declares its open row first** (one cheap act — worked example in
`docs/decisions/README.md`), and a challenge to a decided row is a NEW row with
`reconsiders: <OLD-KEY>`, never a re-ask. **Since this shift, the negative trail asserts "this is
not a decision question"** — legitimate only for clarifying an in-flight directive, an
external-clock relay (never delayed by row ceremony, ADR-20260812-143619), or a mechanical
choice; trails written BEFORE 2026-08-21 were authored under the earlier grammar and make no such
classification claim. On dispatch cards, a `Decision row:` line must name a declared, non-legacy
key (`make validate`, §22d); the same `row:` anchor is required per question on the decision-form
template. A docs push touching any citation-governed surface (all of `docs/**` + `CLAUDE.md`,
§23) runs `make validate` first — the docs-only carve-out predates the ratchet. Skipping it is
now caught by CI's `docs-validate` job (before the 2026-08-21 verification slice this sentence
claimed an asynchronous CI backstop that DID NOT EXIST — docs-only pushes skipped every gate job
and the required check reported green; ADR-20260821-103403 as amended).

What the format encodes, each clause earned by a lens at the 2026-08-21 briefing:

- **A record id, or the explicit negative — never a bare "done".** A trail must name a verifiable
  artifact (`ADR-YYYYMMDD-HHMMSS`, legacy `ADR-00NN`, `PROP-YYYYMMDD-HHMMSS`, a `DECISIONS` section,
  a `journal-YYYY-Www` entry) or record the negative with its terms, so a hollow trail is auditably
  hollow. The negative is a **passing** trail: a genuinely new question, a clarification of a
  directive the founder just gave, and an external-clock relay (never to be delayed) all use it —
  **do the check, then ask; never silently drop a question because asking got harder.**
- **Date and status ride the citation** because a found record is not always an answer: a
  counsel-gated or still-open row legitimately RE-OPENS the question (cite it and ask), and
  *"the answer exists but the underlying facts changed"* is a legitimate re-ask that names the
  record it would revise. A found controlling answer, by contrast, **terminates in the work record
  as its citation — it is never relayed to the founder as a question.**
- **Cite the nearest non-controlling record when nothing controls** — "ADR-X decided the adjacent
  thing" is exactly what saves a re-litigation.
- **The citation is fetched at ask-time, supersession followed then.** A record found early in a
  long session and cited from memory at the moment it licenses an action is a snapshot without its
  version — concurrent sessions land supersessions mid-run.

**Search with the alias table, not only the question's own words.** A record written in another
era's vocabulary honestly returns nothing — the miss that no hook can catch. The table is the
Published Language for the search; **every rename appends its pair** (the CLAUDE.md "grep the OLD
term" sweep already produces exactly these pairs — land them here instead of discarding them), and
**every question later found to have been answered appends the term that would have found it**:

| Canonical term | Also search |
|---|---|
| contribution | tip, tips (the 2026-08-18 incident's own miss) |
| delivery | rider (the boundary rename, DECISIONS §31 BND-2; `RIDER` stays a role) |
| founder | product owner, customer (all three eras coexist in the record) |
| register | decision queue, DECISIONS.md, answer sheet |

**Check `docs/decisions/` FIRST** (REG-2/REG-4, ADR-20260821-095957): every migrated register row
key has a `docs/decisions/<KEY>.yaml` that is **authoritative for its CURRENT status** — one
`grep -l` there beats any prose search, and a question referencing a key whose row is not `open`
is refused mechanically with the citation that answers it (a `decided` row is not a question — a
changed premise opens a NEW row citing the old one; an open `counsel`-owned row takes only the
external-action question). Keys on `docs/decisions/_legacy.yaml` are still prose-only; the sources
above stay the search surface for them.

**Enforcement — what is mechanical and what is not** (state it honestly, believe no more): the
`AskUserQuestion` tool path is gated by `.claude/hooks/register-check.sh` (PreToolUse, fail-closed:
the trail's presence and shape, plus the row gate above, reading the row FILES at the point of
need — never the generated index; one log line per firing in `.claude/register-check.log` with a
closed reason taxonomy — `trail-missing`, `trail-hollow`, `trail-answered` (a trail whose OWN
`(<date>, <status>)` clause self-declares a closed status, ADR-20260828-120500 / #709; ALLOW logs
`trail-premise-changed` when the escape hatch fires), `key-decided`, `key-superseded`,
`key-deferred`, `key-withdrawn`, `key-counsel-owned`, `key-legacy` — plus the keys hit, so hollow
trails and stale-decision citations stay decomposable defects); questions travelling as PROSE —
run reports, decision-queue sections, PR/issue comments, register rows, decision forms — are bound
by the citation block every `.claude/agents/*.md` carries, whose presence (with this section's
existence, the settings wiring, and the live row gate) is asserted by
`.claude/hooks/register-check-selftest.sh` on every turn (`make hooks-test` directly). That script
also compares all four gate scripts against their committed blobs before reporting, and REFUSES to
report if one drifted. `make hooks-test` and `make stub-tests` pass the matching
`REGISTER_CHECK_ALLOW_DIRTY=1` / `DECISION_LOOKUP_ALLOW_DIRTY=1` unconditionally, so editing a gate
script and re-running still works. **`stop-gate.sh` opts out only when a gate script is DIRTY in the
working tree** — on an ordinary turn it runs the comparison armed. What that catches is narrower
than it first reads: an ordinary overwrite leaves the tree dirty, so it opts out and is caught at
push; the armed path catches the tamper that *hides* from `git status` (`--assume-unchanged`,
`--skip-worktree`), which is the stealthier class. CI invokes both directly and cannot be
talked out of the comparison at all (`env_ok` forbids both opt-out names as `env:` keys). **A local green from
either `make` target therefore EXCLUDES the gate-set comparison** — that is the point of the opt-out,
and it is the one thing those targets do not prove. If you run either script by hand mid-edit, pass
the variable; run it clean-tree without one to see what CI sees. The hook
proves presence, shape and row status, never that a search happened — honesty stays with the mob
briefing and the independent review, which is why the trail must name its artifact.

Ask only what is genuinely his: a real option space, an external or legal action, or a fact only he
knows. Order the questions by dependency and say so with the `gates` field. Never make a field
required, and always end with a free-text question.

## Delegate execution to a cheaper model tier (founder, 2026-08-28)

The founder's standing instruction for token optimisation: **execution goes to subagents on a lower
model tier; the coordinating session keeps judgment, mob mechanics and founder-facing surfaces.**
Concretely, when spawning `executor`/`generator`/sweep-style agents, pass a cheaper model (`sonnet`
by default; `haiku` for purely mechanical sweeps — renames, grep-and-fix, regeneration runs). Keep
the coordinator model for: triage decisions, review verdicts, records (ADRs/journal/call sheet),
and anything on the `HOLD: human` class. Cost asymmetry is the reason: a long diff-authoring run
is mostly tool-echo tokens, which price the same on every tier and carry no judgment.

## A serialized merge queue makes its own conflicts — check every armed PR's mergeable_state after each merge, never wait for an event (founder, 2026-08-28)

The founder had to report a conflicted PR (#716) the coordinator was silently awaiting: *"Ensure
to detect that the pr requires to resolve conflict to avoid await for nothing."* The mechanism is
deterministic, not bad luck: when several PRs touching the same append-surfaces (tests.rs,
main.rs, the current journal-W%V file, SPEC-LOG) are armed with auto-merge, EACH merge to main
dirties the survivors, and GitHub sends NO webhook for the mergeable→dirty transition — the wake
events are merges, CI completions and comments, so a conflicted PR just sits there while the
session "waits" on an event that will never come. Rule, binding on the coordinator role:

1. **The detection point is each merge event, not a timer**: on every `pull_request.closed`
   (merged) wake, and after every direct push to main, fetch each still-open PR you armed and read
   `mergeable_state`. `dirty` → resolve NOW (merge origin/main into the branch, keep both journal
   entries newest-first, gate, push). `blocked` = CI running, fine. `unknown` = GitHub still
   computing — re-fetch once after the next action rather than assuming either way.
2. A check-in on an armed-but-open PR that finds `dirty` was already late — the merge event that
   caused it was the moment to act.
3. Costed at one founder interruption on 2026-08-28 (#716 waited conflicted while the session
   idled); the same serialized-queue conflict had already been resolved three times that evening
   (#700, #705, #711→#713 twice), so the pattern was established and the check was owed.
