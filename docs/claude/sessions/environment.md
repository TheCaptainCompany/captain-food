# Session rules — the container

Part of [`../sessions.md`](../sessions.md).

## 2. Disk is a fixed per-session allowance, and `df` lies about it

`df` reporting `Avail 0` with a low `Used` figure means the **allowance** is spent, not that the
machine is broken. Writes fail with `No space left on device` while **deletes still succeed** — and
freed space is immediately writable.

Observed: a full `cargo build --workspace` grew `target/` to **30G** and exhausted the allowance
mid-run. Recovery that worked:

```bash
rm -rf target/debug/incremental target/debug/build target/debug/deps   # freed 26G
```

**Budget for this EVERY session now (2026-08-10, #474):** `make test-crates` runs from the Stop hook
on any `crates/`/`migrations/` diff, so the workspace test binaries get built on ordinary turns, not
just when someone chooses to. The allowance is a routine constraint rather than an occasional one.
In the #474 session `target/` reached 28G with 478M free and the harness itself died mid-run
(`the temp filesystem … is full`), losing a completed 4-minute workspace run's output; `rm -rf
target/debug/incremental` alone freed **16G** (28G → 13G) and left the build warm. Clear it BEFORE a
planned workspace run, not after one fails. Note `target/release` does not exist in every container
— check before reaching for the lever below.

**But count the cost before doing it:** deleting the debug cache means every later `make rust` is a
cold build. In the session where this happened it bought two full rebuilds. Delete only when writes
are actually failing, and prefer dropping `incremental`/`deps` over the whole `target/`.

**Check `target/release` FIRST (2026-08-04):** it was **1.2G** in a session that only ever ran debug
gates — `make rust`, `cargo test --workspace` and the codegen all build debug, so `rm -rf
target/release` is free: no rebuild penalty at all, unlike every lever below it. It is the first thing
to try when `df` gets tight, not the last. Freeing it took a build from 892M headroom back to 2.1G,
which was enough to finish `cargo test --workspace` without touching the debug cache.

**The cheap lever inside `deps/` (2026-07-31):** the files **over ~50M are final-LINK products**
(test binaries — stale hashes accumulate, ~15G across ~20 files after a day of DB-test iteration),
not compiled dependencies. `find target/debug/deps -maxdepth 1 -type f -size +50M -delete` freed
15G and cost only a **relink** of whatever ran next (~seconds each), where deleting all of `deps/`
costs a full recompile of ~200 crates. Two ENOSPC build failures in one session were both cured by
this plus dropping `incremental/`. **Post-#335 (2026-08-09) the biggest producer is gone**:
`infrastructure`'s 27 integration binaries (1.4G per build state, ≈52M each — most of any `+50M`
sweep's haul) are ONE `--test main` binary at ~70M, so a stale-hash day accumulates ~20× less
there; the remaining large link products are other crates' suites and the `server` binaries. The
consolidated suite pays instead a ~0.4 s/test schema reset (the witness replays the full
42-migration chain per test): the 54-test infrastructure pass went 15 s → 37 s of pure execution,
bought back several times over by 26 fewer link steps per iteration.

**ENOSPC also kills a local Postgres, and it does not come back on its own (2026-08-13, #516):** a
`cargo build` that exhausts the allowance takes the local cluster down with it, and the cluster then
fails to RESTART, because crash recovery itself needs to write — `FATAL: could not extend file
"base/.../..._fsm": No space left on device` raised **during WAL redo**, then `shutting down due to
startup process failure`. The DB shares the allowance with `target/`, so a workspace build and a live
cluster compete for it. Two consequences, each earned: (a) when the database "disappears" mid-session,
read `<datadir>/server.log` before suspecting your code, the harness or your connection string — the
cause is two screens up and unmistakable, and it reads like the DB was never there; (b) it recovers
with a single `pg_ctl start` **once space is free** — no `initdb`, no data loss — so free space FIRST
and do not re-init in a panic. Free disk before a workspace build whenever a cluster is up. Cost here:
one lost compile plus a recovery round, on top of the build that caused it. **"A single `pg_ctl
start`" is the recovery STORY, not a literal root-shell command (2026-08-29, #729/#730):** in a root
session the bare form fails three ways in a row — `pg_ctl` is not on PATH (`/usr/lib/postgresql/16/bin/pg_ctl`),
it refuses to run as root, and `sudo -u postgres` fails here too. The working form for an existing
cluster (e.g. `/var/lib/postgresql/ci-repro`) is
`su postgres -c "/usr/lib/postgresql/16/bin/pg_ctl -D /var/lib/postgresql/ci-repro -l /var/lib/postgresql/pg-ci.log start"`
— the same `su postgres -c` shape gates.md uses for initdb. Cost: three failed start attempts on a
card that paraphrased this note as "pg_ctl start suffices".

**ANY disturbance of `target/` can leave cargo serving a STALE artifact it believes is fresh — and
the quiet failure is worse than the loud one** (2026-08-08 + 2026-08-16 #609, one rule; the two
occurrences differ only in which artifact went stale). Cargo trusts its fingerprints, and a
hand-deletion or an interrupted compile desynchronises them from what is actually on disk. Both
directions have now cost a session:

- **LOUD, and therefore cheap.** One `make rust` check-drift ran a STALE `generate` binary right
  after the deps sweep and mass-pruned the five freshly generated `crates/bins/adapter-*` crates —
  4 775 deletions that looked exactly like an emitter bug. The immediate rerun rebuilt and passed
  with zero drift. Cost ~30 min of debugging a phantom. **If check-drift fails with an implausible
  mass-deletion right after a `target/debug` cleanup, rerun it before touching the emitter.**
- **QUIET, and nearly fatal to a run's evidence.** After hand-deleting top-level link products,
  `cargo test --workspace` re-linked `actor_client`'s unit binary from cached objects instead of
  recompiling the changed source: **a test added ten minutes earlier was NOT IN THE BINARY**
  (`running 11 tests` where the same source under `cargo test -p actor_client` gave 12). The suite
  reported **1251 passed, 0 failed, exit 0** and the brand-new gate had never executed. Nothing goes
  red, so **no gate can catch this** — it is the one failure mode that survives a green board.

Three defences, all cheap:

1. **`cargo clean -p <crate>` every package you edited**, before believing a green run, after *any*
   manual deletion inside `target/` — and after any build that died mid-compile, ENOSPC included,
   where the packages that were compiling at the moment it died are the suspects.
2. **Verify a newly added test BY NAME in the run log** (`grep -a '^test .*<name> \.\.\. ok'`), never
   by the total. `1252 → 1251, still green` reads as noise, and that is exactly what it looks like.
3. **Distrust the FIRST post-cleanup result** of anything, and rerun before drawing a conclusion
   from it.

**A NEW workspace crate fails the determinator gate until it is COMMITTED (2026-08-10):**
`closure::tests::hashes_are_total_and_deterministic` panics with `closure dir 'crates/<new>' has no
tracked files in HEAD` — it hashes from `HEAD`, not the working tree. It reads exactly like a broken
determinator in code you never touched; `git add` is the whole fix. Cost: one confused re-read of an
unrelated gate. Same shape as the `configuration.yaml` env gate, which fires on a new
`std::env::var` read anywhere under `crates/` — including a **dev-only test-harness crate**, where
the right answer is the `exempt` list in `tools/codegen-rs/src/tests.rs` (with the reason), NOT a
spec key: a test knob does not belong in the boot report and every derived deployment manifest.

**Don't flip `CARGO_INCREMENTAL` mid-session (2026-08-01):** toggling it changes the crate
metadata hashes, so the next build writes a SECOND full set of workspace artifacts next to the
old one — flipping to `0` right after deleting `incremental/` re-exhausted the allowance during
the very build meant to save space. Pick one mode for the whole session; if you must switch,
delete the workspace-crate artifacts (`lib{server,web,application,infrastructure,domain,…}-*`
and the test binaries in `deps/`) in the same breath. **The same applies to changing
`[profile.dev]`** — it is the identical metadata-hash mechanism. Delete `target/debug` *before* the
first build on the new profile, never after.

**The numbers above are the pre-2026-08-04 profile.** `[profile.dev] debug = "line-tables-only"` is
now set in the root `Cargo.toml`, which removed the *cause* rather than the symptom: debug info was
85% of the largest artifact, and the same `server` binary went 506M → 197M (debug info 381M → 77M).
A clean build + full `cargo test --workspace` now lands at ~9.4G instead of ~17G. Cleanup is still
the emergency lever, it is just needed far less often. If you are debugging and want full DWARF,
override for that command — `CARGO_PROFILE_DEV_DEBUG=true cargo build` — and do not commit it back;
note that this too rewrites every artifact (see the metadata-hash rule above).

Never tell the user the container is unrecoverable — clean up first; a fresh session is the fallback,
not the first move.

## 3. Keep MCP output small — it is the biggest context cost available

GitHub MCP `search_issues` / `list_issues` / `pull_request_read` return **full issue and PR bodies** by
default. One `search_issues` call in a real session returned six complete epics — more context than
every source file read in that session combined.

- Always pass **`minimal_output: true`** unless you specifically need body text.
- Always cap **`perPage`** (5–10).
- When you need one issue's body, fetch that issue — do not search and read six.
- **`minimal_output` does nothing for `actions_list` / `actions_get`.** A `list_workflow_runs` with
  `perPage: 3` returned ~90k characters both with and without it — each run carries two full
  `repository` objects plus the head commit message. The harness spills an oversized result to a file
  under `tool-results/`; parse that with `python3 -c`, printing only `name/head_sha/status/conclusion`.
  Do NOT `Read` it (single line, too long to chunk) and do not retry the call hoping for less.

**An agent created mid-session is not immediately dispatchable (2026-08-09).** Writing
`.claude/agents/<name>.md` and pushing it does NOT register the agent in the running session:
`Agent(subagent_type: "beck")` failed with *"Agent type 'beck' not found"* minutes after the file
was committed, then registered on its own a few minutes later. So a new lens is usable in the
session that created it only after a delay, and cannot be relied on at all. Workaround that works
immediately and costs nothing: dispatch `general-purpose` with the new agent's charter PASTED into
the prompt ("You are **beck** … here is your charter, adopt it fully") — the lens behaves
correctly, and the dispatch is honest about why. Plan roster changes so the first real use is a
later dispatch, not the one that motivated writing the file.

The MCP servers also disconnect and reconnect mid-session (observed four times in one session). Tool
schemas must be re-fetched via `ToolSearch` after a reconnect; that is normal, not a fault, and it is
not worth narrating to the user.

## 4. This container cannot read PDFs

Confirmed dead ends — do not spend turns rediscovering them:

- `pdftotext` is absent and `apt-get install poppler-utils` does not succeed.
- The system `cryptography` module is broken (`ModuleNotFoundError: No module named '_cffi_backend'`,
  then a `pyo3_runtime.PanicException`), so **`pypdf` and `pdfminer.six` both crash on import** even
  after a successful `pip install`.
- `Read` on a PDF needs `pdftoppm` for page rendering, so it fails too.
- Hand-rolling zlib stream extraction returns font tables, not readable text.

**Ask the user to paste the relevant passage.** Say plainly that the extraction is unavailable and
that you are working from what they pasted rather than the full document — never imply you read a
file you could not open.

## 17. The container can restart mid-dispatch — put the handoff in the PR, not in the session

The remote container restarted while an executor was working. **In-process subagents do not survive
it** — `ListAgents` returns nothing, there is no reconnect — and neither does anything the
coordinator was holding in context about what the dispatch had done so far.

What survived was everything *pushed*, and recovery took three GitHub calls (list branches, list
commits, read the PR) because the executor had followed the existing rules: open the draft PR
**before** any code, commit at phase boundaries. Its PR body already carried the full state — what
was done, what was deliberately NOT done and why, the re-measured validator histogram, and two
adjacent findings for someone else. Nothing had to be reconstructed or redone.

**The rule this earns, one step past "commit hourly": the PR body is the supervision state, not the
session transcript.** Write the "what is true right now / what is deliberately not done" section
into the PR *as you go*. A coordinator that keeps that state only in context loses it; an executor
that saves its report for the final message loses the whole run when the container recycles.

**Sharpened 2026-08-25: the death is SILENT, and a waiting coordinator can wait forever.** Two
`reviewer` agents were launched, the session went idle, the container suspended, and on resume the
agents were gone — but **no completion notification is ever emitted for an agent that dies this
way**. A coordinator whose next step is "act on the verdict" simply waits, and a scheduled check-in
that says *"if nothing changed, re-arm silently"* will keep re-arming against a corpse. Worse, the
transcript files still exist and their **mtime updates on session resume**, so "the file was touched
a minute ago" reads as liveness when nothing is running.

**Detect it in two cheap steps, and never trust mtime alone:**

1. `ls -la` the agent's `.jsonl` twice, minutes apart, and compare **SIZE**, not mtime. Byte-identical
   size across a real interval means no output is being produced.
2. Confirm with `TaskStop` on the agent id. **`No task found with ID: ...` is the positive proof** —
   the registry has no such task, so it is not running and cannot be waited on.

Then relaunch rather than wait. Budget the relaunched brief explicitly (*"if you run long, return a
PARTIAL verdict rather than nothing"*) and give it a priority order: a long agent that dies at 95%
returns exactly as much as one that dies at 5%. This is the dead-man's-switch defect CLAUDE.md names
under the no-polling rule, in the team's own loop: **a wait that can only end when a signal ARRIVES
goes quiet exactly when the thing it waits on has died.**

**Corollary**: after any unexplained gap, verify agent liveness before assuming a dispatch is still
running. A silent agent and a dead agent are identical from inside the session, and the difference
is one `ListAgents` call.

### A mob aggregation exceeds the Bash output cap — read it in slices, never `cat`

Roster returns and their aggregations are consistently **300+ lines**; `cat`-ing one truncates, and a
truncated read of a lens's return is exactly the failure the
[verbatim rules](evidence.md#verbatim-is-a-mechanical-check-not-a-careful-read) exist to prevent —
you cannot tell a missing G8 from a clipped one. Read them with `sed -n '1,120p'` / `'120,240p'`, in
~120-line slices, and `wc -l` first so you know how many slices there are. Same for any mob artifact
in the scratchpad. Cost that earned it: a return read as "complete" that had been cut mid-section,
and the re-read that followed.

### The disk cost of a parallel mob review, and what to reclaim first

A three-lens review of one branch is three worktrees each running `make rust` — three full `target`
trees. That filled the session allowance (99%, `0MB free`), and the failure mode is the one §2
describes: writes fail while the numbers still look fine. Three things worth knowing:

- **`CLAUDE_CODE_TMPDIR` is the escape hatch when even tool stdout cannot be written** — export it
  inline in the same command (shell state does not persist between calls), or every command fails
  before it runs. **It does NOT help when the DEVICE is full, only when the tmp path is** (2026-08-17,
  #623): here the scratchpad, the session tmpfs and `/home/user` are all `/dev/vda`, so pointing
  `CLAUDE_CODE_TMPDIR` somewhere else fails identically, and the failure is total — every Bash call
  dies with *"the temp filesystem … is full. The child process's stdout/stderr writes failed with
  ENOSPC"* **before the command runs**, including the `rm` that would fix it. The way out is a command
  that WRITES NOTHING: `rm -rf <big-dir> 2>/dev/null; true` succeeds where the same `rm` followed by
  `df -h` does not, because only the second one needs to print. Recover first, measure second. Cost:
  three dead tool calls spent trying to diagnose a full disk with commands that could not report.
- **A dead agent's worktree is the first thing to reclaim, and it is free**: check
  `git status --porcelain` and `git log -1` against the pushed head; clean at the remote sha means
  nothing to lose. Here that was 8.8G — more than the whole review needed, and cheaper than
  deleting `target/debug`, which costs a rebuild.
- **Sweep the scratchpad for stale build dirs before touching `target/` at all (2026-08-13, #516):**
  earlier runs in the same session directory left **4.7G** of abandoned build trees there; clearing
  them took the allowance from 2.3G to 6.9G. Same class as the dead worktree above and same price —
  free — but easier to miss, because nothing in `git status` mentions the scratchpad. Check it before
  every `target/debug` lever in §2, all of which cost a rebuild.
- **Deleting the top-level `target/debug/<bin>` link products reclaims NOTHING** (2026-08-16, #609):
  cargo HARDLINKS them to their `deps/` artifact, so 73 files and 6.45 GB by `stat` moved `df` by
  zero bytes. It looks like the biggest lever on this list and is not a lever at all. **It is also
  not free** — that hand-deletion is what desynchronised the fingerprints in §2's stale-artifact
  rule, which cost a run its evidence; read that rule before reaching in here by hand. The honest
  levers are above (dead worktrees, the scratchpad sweep, then `incremental`) plus `cargo clean -p`
  on a few big packages, which was 2.7 GB here and leaves cargo's bookkeeping intact — **and on a
  workspace-test tree `cargo clean -p server -p web` alone was 5.8 GB in one command (2026-08-17,
  #623), which makes it the LARGEST honest lever on this list, not a last resort**. Also: deleting
  `target/debug/incremental` is wasted work if the next `cargo test` just recreates it — pass
  `CARGO_INCREMENTAL=0` on every run for the rest of the session, or you will free 1.6 GB twice and
  still run out. Symptom that
  sent this session looking: `fatal: … index.lock write error. Out of diskspace` from `git commit`
  while `df` still reported 2.3G free — §2's "writes fail while the numbers still look fine".
- **Timing a "cold" build with a second `CARGO_TARGET_DIR` costs a full second copy — and it does
  not measure what it looks like (2026-08-26, #679).** An empty target dir plus the session's warm
  `CARGO_HOME` measures COMPILE FROM A WARM REGISTRY, not the cold path a CI runner takes, which
  also fetches every crate in the tree. Setting `CARGO_HOME` too is the fix and doubles the cost
  again: a `tools/codegen-rs`-sized tree wanted more than the ~800M left after the first attempt,
  so the re-measurement died with `No space left on device` mid-build and the finding had to be
  recorded as UNMEASURED. Two lessons, and the second is the expensive one: **delete the scratch
  target dir in the same command that reports the timing** — an abandoned one is exactly the stale
  build dir the sweep above exists for, only self-inflicted — and **before quoting a build duration,
  say which caches were warm**, because a measurement is defined by what it excludes and the
  omission runs in the permissive direction.
- **A fresh worktree starts with an EMPTY `target/`, and a cold workspace build does not fit the
  allowance (2026-08-13, #516):** `<wt>/target` was 4.0K against ~1G free. Point the build at the main
  checkout's cache instead — `CARGO_TARGET_DIR=/home/user/captain-food/target make rust` — which reuses
  the ~200 compiled registry deps and made both `make rust` and `make test-crates` feasible where a
  cold build in-tree could not start. Workspace crates still recompile (the path is part of the package
  id), so a second artifact set coexists; that is the price, and it is far below a cold build. Do NOT
  reach for the `deps/ +50M` lever without **excluding `.rlib`** when sharing a cache this way — the
  sweep in §2 is worded for link products, and a bare `-size +50M -delete` there took a `server` lib
  recompile with it.
- **`git worktree add <abs-path>` from a reset cwd can land the tree INSIDE the repo.** It did here,
  creating `captain-food/cad2-wt` — a worktree nested in its own repo, showing up as an untracked
  directory one `git add -A` away from being committed. Verify with `git worktree list` after adding,
  and remove with `git worktree remove --force` rather than `rm -rf`.
- **Two sessions running `make test-crates` on ONE Postgres wipe each other**: the harness resets with
  `DROP SCHEMA public CASCADE; CREATE SCHEMA public;` (`crates/infrastructure/tests/main/common.rs`),
  which is database-wide, not test-scoped. So when a neighbour's server is already up, do not reuse
  its database — `createdb cf<NN>` and point your `DATABASE_URL` at that, then `dropdb` when the run
  is done. The isolation is per-DATABASE; the schema-level reset gives you none. **And the CLUSTER
  itself is the neighbour's to kill** (2026-08-29, #755/#756): mid-run the shared `ci-repro`
  postmaster was restarted by the concurrent session on a DIFFERENT port (5433→5432) and later died
  outright without a shutdown line, leaving a stale `postmaster.pid`. So probe port AND liveness
  (`psql -tAc 'select 1'`) immediately before any DB-gated suite — a `DATABASE_URL` that worked an
  hour ago proves nothing now. Know the disguise: a `PoolTimedOut` from `graphql_write_path.rs`
  means "no Postgres", NOT pool exhaustion — connection-refused surfaces as a 60 s pool timeout per
  test. Recovery is mechanical once `kill -0 <pid>` (pid from `postmaster.pid`) confirms the
  postmaster is gone: `rm postmaster.pid`, `pg_ctl start` — both cluster recoveries were clean and
  the neighbour's databases untouched. Cost of not probing first: two red-herring diagnoses and one
  wasted full-suite re-run in a single run.
- **`pgrep -f "<anything from your own command>"` always matches YOUR shell**, because every Bash tool
  call runs inside a wrapper whose full script text is its command line. An
  `until ! pgrep -f "cargo test --workspace"; do sleep 30; done` written to wait for a neighbour's
  build therefore never terminates — it is waiting on itself. Cost here: two 10-minute background
  waits that outlived the build they watched. Match on the process name instead (`pgrep -x cargo`,
  `ps -eo comm`), which cannot see the wrapper's arguments.

- **`git stash pop` after a no-op `git stash` pops SOMEONE ELSE'S ENTRY.** Wrapping a command in
  `git stash` / `git stash pop` is a reflex from a dirty tree; on a CLEAN tree the `stash` is a
  silent no-op and the `pop` then takes the top of a stack that may belong to another branch or
  another session. This repo's stash held `HELD: PR670 amendment (founder decision form + C2/C3
  fixes) — awaiting founder approval`, from a different branch; the pop conflicted, which is the
  only reason the entry survived — git keeps an entry whose pop failed, and drops one whose pop
  succeeded. A clean pop would have silently applied held work into an unrelated branch AND deleted
  it from the stack, with no signal in either direction. **Check `git stash list` before any pop, or
  simply do not stash**: a clean tree needs no stash, and a dirty one is better committed.

## The session disk allowance dies mid-run, silently, and subagent scratch is the hog (2026-08-28)

An executor subagent lost ALL Bash capability mid-dispatch (`ENOSPC` before any command ran — even
`echo`) because the session's writable allowance was spent. What filled it: this session's own
accumulated scratch (two full cargo target trees copied into the scratchpad by earlier review
rounds, ~1.1GB) and `target/debug/incremental` (12GB). Deletes still succeed when writes fail, so
the coordinator freed 13GB in one command and resumed the agent — nothing was lost but ~10 minutes
and a partial run. Rules earned: (1) never copy a target tree into the scratchpad (symlink or run
in place); (2) before dispatching a build-running subagent, `df` the root and prune
`target/debug/incremental` if tight — it is pure cache, and the shared non-incremental artifacts
survive; (3) a subagent reporting "environment broken, no cleanup tool" is a COORDINATOR page, not
a failure — the coordinator holds the delete capability the subagent lacks. Also observed: file
tools with absolute paths do not enforce worktree isolation (the git hook does, `Edit` does,
`curl -o` does not) — a worktree-scoped dispatch card must say "never write to the shared checkout
path" explicitly, and this one now does.

## The 38GB effective quota, ENOSPC-safe cargo flags, and the capture-share trap (2026-08-28)

The #641 executor measured what the earlier disk note implied: the session's writable allowance is
~38GB EFFECTIVE (never the 252GB `df` reports), shared across every concurrent agent worktree's
`target/` (~11-15GB each when full). Rules earned, at the cost of ~1h across 6 build attempts:
(1) build-heavy executor runs set `CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0
RUSTFLAGS="-C debuginfo=0"` from the FIRST build, not after the first ENOSPC; (2) the Bash tool's
own output capture lives on the same disk — a build that fills it breaks the NEXT command's
capture ("Command output was lost") even for `rm`; redirect big-output commands to a worktree file
(`> build.log 2>&1`) and Read it back, and remember `> /dev/null 2>&1` still gets a cleanup
command through a full-capture condition; (3) a finished worktree's `target/` is reclaimable by
the COORDINATOR only — executors must report, not delete, other agents' trees. **Corrected 2026-08-31 (#764 records dispatch)** — this paragraph used to end *"executors cannot
perform GitHub API mutations"*, and that is WRONG. `gh` is absent and the GitHub MCP tools are not
in a subagent's toolset, but **the REST API is reachable**: `curl -H "Authorization: Bearer
$GH_TOKEN" https://api.github.com/...` returned `200` from an executor subagent, and `-X POST` /
`-X PATCH` work the same way. The rule is **prefer push; verify the token before assuming** — one
`curl -o /dev/null -w '%{http_code}'` against the repo endpoint settles it in a second. Cost that
earned the correction: believing the old sentence makes an executor hand back incomplete work
(an un-updated issue, an uncommented PR) for a capability it actually has, and a dispatch card
written from it says "PR mechanics are the coordinator's" when they need not be.
