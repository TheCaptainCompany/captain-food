# Claude rules — working a session (environment limits, gate choice, context economy)

Hard-won operational knowledge. Everything here cost real time in a real session; none of it is
derivable from the code. Read it before a long or exploratory session — especially one that talks to
a third-party dashboard, reads binary files, or runs the gate more than once.

Related: [codegen.md](codegen.md) (what each gate does) · [loops.md](loops.md) (budgeted autonomous
runs) · [../PLAYBOOK.md](../PLAYBOOK.md).

## 1. Pick the cheapest gate that proves the change

| Change touches | Gate | Cost |
|---|---|---|
| `docs/**` only (no regeneration) | nothing, or `make validate` | seconds |
| `specs/**` | `make validate`, then `make rust` before pushing | seconds, then minutes |
| `crates/**`, `tools/**`, CI, deploy | `make rust` | minutes — **much** worse from a cold cache |

`make rust` is `rust-build` + validate + generate + drift check — and `rust-build` is
`cargo build --manifest-path tools/codegen-rs/Cargo.toml`, i.e. **the codegen tool only, not
`crates/**`** (codegen-rs is a workspace member but is "tooling, not part of the app graph",
`Cargo.toml:10`). It is still slow on a cold cache. Do not reach for it to prove a Markdown edit.
CLAUDE.md already permits skipping it for docs-only changes — the point here is that the saving is
minutes per invocation, so it is worth being deliberate.

**On a PR BRANCH, a full local `make rust` before every push duplicates CI.** It is load-bearing in
exactly one case: a **direct-to-`main` spec/doc push, where no CI follows**. On a branch, the
pre-flight that pays for itself takes seconds — **(1)** `git fetch origin main` + a rebase/merge
check (does this branch still merge?), **(2)** `make validate`, **(3)** a markdown-table lint on any
touched register/proposal row. That triple **would have caught BOTH red CI rounds of 2026-08-15** — a
merge conflict and a malformed `DECISIONS.md` row — **without a workspace build**
([ADR-20260816-020752](../adr/ADR-20260816-020752-the-loops-context-budget-a-dispatch-card-snapshot-semantics-and-phase-commits.md)
decision 6). Corollary for CI itself: path filters are keyed on one question — *can this change
generated output?* (`docs/**`/ADRs/`STATUS.md` skip the matrix, **`specs/**` is never filtered**), and
a filtered job must still report its **required-check name** from a skip job or branch protection
deadlocks.

**CLAUDE.md's architecture summary can be STALE — check it against `docs/STATUS.md` whenever
hosting, storage or deployment topology matters.** Nothing regenerates that paragraph and no gate
covers it, so it drifts silently in the one file every session reads first. Measured cost: on
2026-08-10 it still said *"Managed Postgres"* and cited the superseded `ADR-20260731-061609` for
hosting, when the decision has been **CNPG in-cluster on OVH MKS** since `ADR-20260807-002705` — the
founder had to correct a session by hand on a fact the repo should have supplied. The cheap
tell: **an ADR id cited in prose whose own `Status:` line says Superseded.** STATUS.md is the live
state; CLAUDE.md is a summary of it, and summaries rot.

**`make rust` does NOT run the workspace test suite.** `rust-test` is `cargo test --manifest-path
tools/codegen-rs/Cargo.toml` — the codegen/validator tests ONLY. CI runs `cargo test --workspace`
as a separate step, so a change whose tests live in `crates/**` can pass `make rust` locally and
still go red in CI. Run `cargo test --workspace` yourself before marking a PR ready; it is cheap
once the build is warm. (This entry previously described `make rust` as including `cargo test` —
#306 relied on that, and only noticed when a brand-new integration test failed to appear in the
gate's output. If the wording ever drifts again, read the Makefile, not this table.)

**`make rust` does not run `cargo machete` either**, and CI does. It is not preinstalled in this
container, but `cargo install cargo-machete` works from crates.io in about a minute — do that and
run it before pushing any change that MOVES CODE BETWEEN CRATES. Moving code moves the *use* of a
dependency while leaving the `Cargo.toml` line behind: #306 lifted the typed clients out of
`actor_client`, which took every `serde::Serialize` bound with them, and the now-unused `serde`
entry was caught only by CI. The fix for an unused dependency is to DELETE it, not to add a
`[package.metadata.cargo-machete] ignored` entry — the whole point of the D6 step is that an
unheld capability someone can silently start using is a hole.

So the honest local pre-push gate for `crates/**` work is three commands, not one:

```bash
make rust && cargo test --workspace && cargo machete
```

**…and the middle command is itself only half a gate without `DATABASE_URL` (2026-08-04):** the
DB-gated suites take their early-return branch and report `ok`. On 2026-08-04 a local run reported
**86 suites / 847 passed** and still missed the failure CI then hit — with a real database the
`infrastructure` package alone contributes **29 suites / 87 tests** that had all silently skipped.
`ci.yml` already warns about this for CI; the part nobody had written down is that you can run them
**here**: Postgres 16 is installed in this container, it is simply not started. (2026-08-09
correction: SOME sessions' containers DO carry Docker — `which docker` before assuming; the daemon
just isn't running. `dockerd >/tmp/dockerd.log 2>&1 &`, wait ~8s, then
`docker run -d -e POSTGRES_PASSWORD=pw -e POSTGRES_DB=captain -p 55432:5432 postgres:16-alpine`
and `DB_TESTS_REQUIRED=1 DATABASE_URL=postgres://postgres:pw@localhost:55432/captain` is the
fastest full-suite path — the #430 run's 59/59 came from exactly this.)

```bash
PGDATA=/var/lib/postgresql/ci-repro                       # initdb REFUSES to run as root --
mkdir -p "$PGDATA" && chown postgres:postgres "$PGDATA"   # do it as the postgres user
chmod 700 "$PGDATA"
su postgres -c "/usr/lib/postgresql/16/bin/initdb -D $PGDATA -A trust -U postgres"
su postgres -c "/usr/lib/postgresql/16/bin/pg_ctl -D $PGDATA -l $PGDATA/server.log start"
# Keep -l INSIDE $PGDATA. Point it at a root-owned scratchpad and pg_ctl reports "stopped waiting
# / could not start server / Examine the log output" with NO log to examine -- it could not create
# the file (2026-08-16, #609: one wasted start cycle diagnosing a permission error as a startup
# failure). The infrastructure suite replays migrations itself from `include_str!`, so for
# `crates/**` tests you need only initdb + `createdb cf<NN>` -- the psql migration dance below is
# for a database you are inspecting by hand.

# sqlx-cli is NOT installed (CI gets it prebuilt from taiki-e/install-action, and building it
# here costs minutes). psql applies the migrations, but needs a per-file fallback: some
# migrations require a transaction (LOCK TABLE) and others forbid one (VACUUM), which is why a
# plain loop dies partway either way. Try -1 first — but ONLY fall back when the -1 failure is
# the "cannot run inside a transaction block" class. A BLIND retry corrupts the schema
# (2026-08-07): several migration files carry their own BEGIN/COMMIT, so a later statement
# failing under -1 leaves the inner-committed half APPLIED; the blind rerun then hits "already
# exists" on file after file and the enum int→text conversions run twice ("operator does not
# exist: text = integer"). Recovery is DROP SCHEMA public CASCADE and a clean pass.
for f in $(ls migrations/*.sql | sort); do
  out=$(PGPASSWORD=postgres psql -q -1 -h localhost -U postgres -v ON_ERROR_STOP=1 -f "$f" 2>&1) \
    || { echo "$out" | grep -q "cannot run inside a transaction block" \
           && PGPASSWORD=postgres psql -q -h localhost -U postgres -v ON_ERROR_STOP=1 -f "$f" >/dev/null \
           || { echo "FAIL $f: $out"; break; }; }
done
# Migrations alone are NOT the whole schema: some projection tables (e.g. ProspectionPipeline)
# exist only in specs/generated/schema.generated.sql — a projector that folds into one will
# log-skip its events ("relation ... does not exist") while its checkpoint advances. Apply the
# missing CREATE TABLE from schema.generated.sql if a smoke needs that read model.
# Smoke-testing a worker with RAW SQL inserts: the app's INSERTs raise
# pg_notify('inbound_messages', actor_type) / pg_notify('domain_events', ...) in-transaction;
# a psql INSERT does not, and with push LIVE the poll safety net is 60 s — so notify manually
# or your row sits RECEIVED long past any reasonable wait.

export DATABASE_URL="postgres://postgres:postgres@localhost:5432/postgres"
export DB_TESTS_REQUIRED=1                              # ALWAYS set this -- see below
cargo test -p infrastructure -- --test-threads=1        # --test-threads=1: they share ONE database
```

**Since #474 you no longer have to remember this** — `DB_TESTS_REQUIRED` defaults to REQUIRED, so a
missing `DATABASE_URL` fails the suite instead of skipping it, and `DB_TESTS_REQUIRED=1` is now
redundant (harmless, still honoured). The line above keeps it only because muscle memory is cheap.

Why the polarity had to flip: a full-suite total looks **identical** whether the DB tests ran or
not. On 2026-08-05 a session read this section's warning, ran `cargo test --workspace` with neither
variable, saw **857 passed / 0 failed**, and pushed; CI then failed on a hand-written test schema
still declaring `slug TEXT NOT NULL` for a column the change had made nullable. The same command
with both variables set reproduced it locally in seconds. **The number is never the evidence.**

Cost that earned it: a CI-only failure on a build-profile PR that could not possibly change
behaviour, an hour of diagnosis, and — on #451 — a bricked Cart projection that survived three green
local gate rounds.

**Proposal-hygiene wants every tracking-issue link in the FIRST 40 LINES (2026-08-08):** the
validator scans only the header window (`tools/codegen-rs/src/validate/proposals.rs:117`), so a
multi-issue proposal with a long `Related`/`Concerns` header can push its second tracking link past
line 40 and fail `proposal-tracking-issue-missing` for a link that IS in the file. Keep all
tracking links high; read the validator source before debugging the message. Related fumble-saver:
`make validate`'s summary line does not name warning kinds — diff kinds against baseline with
`make validate 2>&1 | grep '\[warn ]' | awk '{print $3}' | sort | uniq -c`.

**A dispatch that forbids GitHub access must CARRY the exact titles of any issues it links
(2026-08-08, second occurrence):** CLAUDE.md requires issue titles in repo markdown, but titles
live on GitHub, not in the corpus — two executors in one day had to fall back to descriptive
parentheticals after fruitless searches (~3 wasted searches each). The coordinator pastes exact
titles into the brief for every issue the dispatch may newly link. Related validator fact that
saves a check: proposal-hygiene special-cases only the literal strings `Approved`/`APPROVED`
(case-sensitive, `tools/codegen-rs/src/validate/proposals.rs`) — any other Status value,
including `Superseded`, needs only the Status line + a header tracking link, so a Superseded
flip is always gate-safe.

**Writing or running tests inside `tools/codegen-rs` (2026-08-15):** the package is named
`captain-food-codegen`, not the directory name — `cargo test -p codegen-rs` fails to resolve
(use `-p captain-food-codegen`, or the `--manifest-path tools/codegen-rs/Cargo.toml` form the
Makefile uses). And `model::Level` derives no `Debug`, so `assert_eq!` on an issue's level does
not compile; the in-tree pattern is `assert!(x.level == Level::Error, "msg")`. Costs: one failed
invocation and one wasted compile cycle, in one session.

**`check-drift` fails on ANY dirty file, and says the wrong thing about it (2026-08-04):** it diffs
the whole working tree, not just generated paths, so an uncommitted hand edit — a `Cargo.toml`
tweak, a doc fix — trips it with `generated artifacts drifted -- run 'make generate' and commit the
regenerated files`. Running `make generate` then changes nothing and the failure repeats. Read the
`--stat` line it prints directly above: if the listed files are yours rather than generated ones,
the fix is to **commit your own change**, not to regenerate. Real drift names files under
`specs/generated/**` or `crates/**/generated/**`. The check is a plain whole-tree `git diff --quiet`
(`Makefile:75`), which is why a docs-only edit trips a gate whose message only talks about generated
files — it hit again on 2026-08-13 (#516) and cost a full `make rust` cycle spent diagnosing a
phantom drift, and **again on 2026-08-15, costing an ~8-minute gate run** that failed on legitimately
uncommitted WIP in an unrelated path, and **a fourth time on 2026-08-17 (#623)** — on a branch whose
whole point was regenerating from a spec change, which is the case where "the emitter has gone mad"
is the MOST believable reading and the diagnosis therefore costs the most. Four recurrences: the
pre-emption is the rule in §"Run `make rust` only on a COMMITTED tree" — **commit first, then gate**,
and check `git status --short` before invoking. Treat `make rust` as the gate, never as a mid-edit
progress check. The durable fix is not another paragraph here: it is the recipe printing *"N of these
files are not generated — commit your own changes first"* when the `--stat` set is not confined to
`specs/generated/**` and `crates/**/generated/**`, which is the one place a reader is actually
looking when it fires.

**A PR "waiting on checks" may not be waiting on checks at all — read `mergeable_state` FIRST
(2026-08-09).** `pull_request_read` with `method: get_status` returns `{state: pending,
total_count: 0}` for BOTH "the required check is queued" and "this PR has a merge conflict", so a
conflicted PR looks exactly like a slow runner. The tell is on the PR object, not the status:
`mergeable_state: "dirty"` = conflict (also `"behind"`, `"blocked"`); `get_status` never says so.
Cost: ~40 minutes of heartbeats attributing a real conflict to runner backlog while auto-merge sat
armed and could never fire. Habit: when an armed auto-merge does not land within one CI cycle, call
`pull_request_read method:get minimal_output:true` and read `mergeable_state` before blaming the
platform. **And arming auto-merge is not delegating the merge**: on 2026-08-12 it failed to fire on
**both** PRs of the day (#500 and #507) with every required check green and no conflict, and each
had to be merged by hand. Whatever the cause, the operational rule is the one the protocol already
states and sessions keep shortcutting — supervise until the PR reads **MERGED**. A session that
arms auto-merge and walks away leaves a green PR sitting, and the next session finds the work
apparently done and unmergeable-looking. Related: the conflict was SEMANTIC, not textual — one branch moved every
`crates/infrastructure/tests/*.rs` into the `tests/main/` witness harness while the other added a
test in the old layout, so git's auto-merge produced a file mixing both idioms. Resolve onto the
NEW harness and re-run the moved test against a live database; a compile-only check would have
passed while the test silently lost its serialization guard.

**Before diagnosing weird CI behaviour, check `githubstatus.com` — three symptoms together mean the
PLATFORM, not your change**: jobs completing as `cancelled` (rather than `failure`), a run sitting
`queued` for tens of minutes, and — the tell — **pushes creating NO workflow run at all**. On
2026-08-06 all three appeared at once and `ci` went red on `main` for three consecutive docs-only
commits; the cause was a GitHub Actions major outage and nothing in the repo was wrong. One curl
settles it:

```bash
curl -sS https://www.githubstatus.com/api/v2/summary.json \
  | python3 -c "import sys,json;d=json.load(sys.stdin);print(d['status']['description']);\
print([c['status'] for c in d['components'] if c['name']=='Actions'])"
```

Two consequences worth knowing before planning around it. **A red `ci` on `main` is not automatically
yours** — read the failing job's *conclusion* first: `cancelled` with no failed step is the platform,
`failure` with a named step is you. And **a workflow-file change cannot be verified any other way**:
`make rust` does not execute `.github/workflows/**`, so the authoritative test for a CI edit is the
workflow running. During an Actions outage such a PR simply waits in draft — do not mark it ready and
never enable auto-merge on it, because a required check that cannot run leaves the PR hanging on
"Expected - waiting for status".

**Do not write a Monitor/poll loop with `|| true` on every stage.** The same session lost 14 minutes
to a watcher whose `curl` and `python3` stages were all `|| true`-guarded: every call failed, the loop
emitted nothing, and *silence was indistinguishable from "still running"*. Let the failing stage
surface, or echo an explicit heartbeat — a watcher that cannot report its own breakage is worse than
none, because it manufactures false confidence.

**`git reset --hard` to sync a branch DISCARDS uncommitted edits in the working tree** — including
ones made minutes earlier in the same turn. It cost a re-do of this very section. Commit first, or
use `git checkout <branch>` and let git refuse the switch when it would clobber something.

**After a context compaction (or any "Continue from where you left off"), your own recent actions may
be missing from what you remember — CHECK THE REPO before re-creating anything.** On 2026-08-07 a
session claimed #358, opened its draft PR, pushed two commits — then a compaction resumed it from an
earlier checkpoint, and it re-created the SAME realization backlog as seven duplicate issues
(#366–#372, closed as duplicates) and committed a docs change onto the #358 work branch without
noticing HEAD had moved. Two rules. Before creating issues/branches/PRs after any resume, spend 30
seconds on: `git reflog -5` + `git branch --show-current` + read the tracking issue's last comments —
**the claim comment carries the session link, so grep it for YOUR OWN session id**: if it is yours,
the "other session" is you, pre-compaction, and the work is yours to continue, not to redo. And
verify `git branch --show-current` immediately before EVERY commit on `main` — an unnoticed branch
switch put a `main` commit on a feature branch, and `git push origin main` then reported
"Everything up-to-date" because local `main` genuinely matched origin while HEAD sat elsewhere.

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
one lost compile plus a recovery round, on top of the build that caused it.

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

## 8. Generated code can enforce something the spec does not say

`make validate` compiles patterns from the DECODED spec and is happy; the emitted Rust is a separate
artifact that nothing re-read. A double-escaped pattern therefore shipped a regex that rejected the
app's own valid default (`OTEL_TRACES_SAMPLE_RATIO=1.0`), and only the `development` profile's
"starting anyway" fallback kept it off production's floor — `production` and `staging` refuse the boot.

**When a generator writes a literal into generated source, the test must read the GENERATED file**, not
the spec both sides agree on. `generated_config_patterns_match_the_spec_byte_for_byte` does that, and it
was confirmed to FAIL on the reverted emitter before being trusted — a regression test never seen red is
not a guard.

## 8b. A guard over Rust STRUCTURE must parse the AST, not the text

> **First ask whether it should be a guard at all** (ADR-20260803-234035). If the type system can
> make the mistake unspellable — a capability witness with a `pub(crate)` constructor, a sealed
> trait, private fields, a newtype — do that instead and write no guard. Everything below is how to
> build one WHEN THE COMPILER CANNOT, and it is the more expensive branch: the section exists
> because a scanner over a boundary the compiler already enforced cost seven review rounds and
> ~191 lines. Read it as a fallback, not as the method.

`str::find` over source cannot enforce a structural rule about Rust, and three independent review
passes proved it on one guard (#304's `every_mailbox_port_method_demands_the_access_witness`). Each
pass defeated the previous version, never with anything clever: `pub  fn` with two spaces, a
signature split across lines, an attribute before `async fn`, a comment standing in for a parameter,
`impl Default for X`, `pub const KEY: X`, a type alias, the banned trait moved one file over, a
`From<()> for crate::path::X` matching no literal pattern. Every fix bought exactly one shape.

The trap is that a textual guard **looks** like it works — it goes red on the mutation you thought
of, which is the one you wrote it for. Its real coverage is the shapes you enumerated, and the
author of the next bypass is not enumerating.

**If the rule is about Rust structure — visibility, trait membership, what a signature takes, what a
public item returns — parse it.** `syn` (+`quote`) as a dev-dependency of `tools/codegen-rs`. Reserve
text scanning for rules that genuinely are textual (`makefile_recipe_lines_are_ascii` is about bytes,
so text is right).

**But parsing alone does not converge.** Three further passes beat the AST versions too, because
each one asked *where* the guarded type appears, and every answer left a slot uninspected: item
kinds first (free fn, const, static, alias… then associated fn, associated const, struct field, enum
variant, a trait's provided method), then output-and-field positions — which still missed a generic
BOUND (`pub fn mint<T: From<Witness>>() -> T`) and a PARAMETER on a non-port item
(`pub fn with_access(f: impl FnOnce(Witness) -> R) -> R`, a scoped-capability helper written `pub`
by accident — the most plausible real mistake of the set).

**Stop asking where. Assert the guarded type appears in NO release-reachable public signature, with
a CLOSED exemption list.** For each public item take the whole signature — generics, where-clause,
inputs, output, field and variant types — as one token stream and fail on any mention; then name the
handful of legitimate sites explicitly. An open rule ("these positions are bad") loses to the next
grammar; a closed one ("nothing but these") does not.

Riders, all learned the same way:

- **Parameter and output positions are OPPOSITE problems.** In output/field position a substring is
  correct and conservative (`-> Option<T>` still hands one over). In parameter position it is
  exactly wrong: `access: Option<Witness>` mentions the type while letting the caller pass `None`.
  Compare the exact parsed type there. That single defect defeated the primary rule the guard
  existed for.
- **Watch the ESCAPE HATCH as hard as the rule.** An inverted `#[cfg(not(feature = "..."))]` was
  honoured as a test-gate while compiling in exactly the release builds it claimed to exclude — the
  guard's own exemption granting the thing it guarded. A `#[path]` ban that misses
  `#[cfg_attr(<cond>, path = "…")]`, and an `include!` ban keyed on `is_ident` that misses
  `std::include!`, are the same mistake: **ban a CLASS, matched on the last path segment, not a
  spelling.**
- **A ceiling may be the technique's, not the problem's — check before you disclaim.** The class
  above (a public wrapper that mints internally) is un-checkable *by signatures*, and that is not
  the same as un-checkable. Capabilities have a small set of SOURCES: for a token type, a value
  arrives either as a parameter or from a construction. Cover the parameter case with the signature
  rule you already have, then seed a call-graph taint scan on the CONSTRUCTIONS and propagate to a
  fixpoint. Seed and resolve calls **from the AST** — a text seed misses a respelling
  (`T { 0: () }` is the same construction as `T(())`), and an `ident(`-shaped call scan misses every
  function passed as a VALUE (`let f = T::mint;`, `.map(helper)`), which is a false negative in
  honest code and not only an attack. Stop taint at declared entry points, or every caller of the
  public API looks like a new door — but NOT at a *feature-gated* one, since a wrapper does not
  inherit the gate that contains it. Key the allowlist by `(file, name)`; a bare name pre-authorises
  any future function that takes it. The allowlist is the real artifact: it enumerates the doors,
  and opening another is an edit to it.
- **A syntactic tool does not discharge a semantic claim — and this is the trap that caught me
  LAST, after six passes of catching it elsewhere.** "A value arrives as a parameter or a
  construction" is sound provenance. "…therefore my two scans cover every route" does not follow,
  because the scans approximate the call graph by ident with no type resolution. I wrote "complete
  rule" into four documents, including an amendment that RETRACTED a correct limit an earlier review
  had earned; review then produced four ordinary counterexamples. Retracting a real limit is worse
  than the limit. State the scope in the terms the tool actually works in ("sound for constructions
  the AST recognises, and call edges resolvable by ident"), and if you want the semantic claim, say
  what it would cost — type resolution, a rustc lint or HIR/MIR reachability — and leave it to a
  proposal.
- **Know where the guard's ceiling is, and write it down.** Some classes are not checkable at all
  by the technique you chose, and finding that out is a result, not a failure. Here: a public
  in-crate wrapper that mints internally and exposes the capability through a signature that never
  names the guarded type (`pub fn cancel_any(&self, id: Uuid) -> Result<bool>`) is invisible to ANY
  signature analysis — and the codebase's own sanctioned bulk door is a member of that class, so it
  cannot even be banned. When a rule blocks one spelling of a class while its twin passes, do not
  call it closed; say which class it covers and what contains the rest.
- **Macro expansion stays invisible** to an AST walk of unexpanded source, and so do `include!` and
  a `#[path]` module if you walk a directory. Refuse those constructs outright in the guarded crate
  and SAY that is what you are doing — "banned here" is honest; "analysed" is not. (The definitive form is a check
  over the post-expansion public API, e.g. rustdoc JSON; it needs nightly, so it is not available on
  this stable CI.)
- **Mutation-test every arm, and re-run the whole battery after each rewrite.** A fix that closes one
  hole reopened another twice here — once because a mint was excused by NAME rather than by span, so
  the legitimate gated copy excused an un-gated duplicate. Keep the battery in the commit message or
  the guard's doc comment; "verified red" is only worth stating with the count and the shapes.

## 9. This file is your obligation, not just your reference

**Every session records what it learned** (ADR-20260730-034635), in the same change as the work. That
is why this file exists, and it is how it stays worth reading.

- **Where it goes**: operational findings (environment limits, tool behaviour, gate costs, workflow
  traps) → here, or the relevant `docs/claude/` topic file. Decisions → an ADR. Option spaces →
  a proposal + tracking issue. State → `STATUS.md`.
- **Prefer executable over prose.** If the lesson can be a validator rule, a behaviour test or a hook,
  write *that* — prose can be ignored, a gate cannot. `makefile_recipe_lines_are_ascii` turned a
  one-off Makefile breakage into a codegen test so it could not silently return; that is the bar.
- **Bar for an entry**: not derivable from the code, and it would cost the next session time. State
  the concrete cost that earned it — "one `search_issues` call returned six complete epics" is a rule;
  "be careful with MCP output" is noise.
- **Sharpen, don't duplicate.** Extend the existing rule rather than adding a near-identical one; two
  overlapping rules mean neither is trusted.
- **Writing nothing is a valid outcome.** A session that learned nothing transferable adds nothing.
  If this file ever reads like a diary it has failed, and the fix is deletion, not more headings.

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
open gate, twice now.

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
[ADR-20260816-020752](../adr/ADR-20260816-020752-the-loops-context-budget-a-dispatch-card-snapshot-semantics-and-phase-commits.md)
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
[ADR-20260816-134352](../adr/ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md)).
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

## 11. Installing a dev tool: crates.io works, GitHub release downloads do not

The session proxy scopes GitHub — the REST API **and** release-asset downloads — to the
repositories attached to the session: `curl https://api.github.com/repos/<other-owner>/...`
returns 403 with an `add_repo` hint, and a `releases/latest/download/<asset>` URL "succeeds"
with a tiny error body that only surfaces when `tar` rejects it (cost: one debugging round while
fetching a prebuilt cargo-machete, 2026-08-03). `cargo install <tool> --locked` from crates.io
works fine (~2–3 min compile) — go straight there for Rust tooling; do not burn turns on
prebuilt-binary URLs.

**GitHub 403 from executor sessions is API-only** (2026-08-13): plain `git fetch`/`git push` over
the git transport works even when every `api.github.com` call returns 403 — push the branch
yourself; do not hand branch pushes back to the coordinator.

**A DB-gated suite that is green in CI can still be order-dependent** — CI runs the whole
workspace against ONE shared database, so a suite can silently depend on a table a SIBLING suite
leaves behind (`graphql_write_path` needed `catalog`, created only by other suites' resets; alone
on a fresh database it hung forever). The failure shape is nasty: a mailbox delivery whose repo
query hits a missing relation aborts the completion TRANSACTION, so the status flip fails too and
the lane retries forever — row stuck RECEIVED, no error column, no panic, poll timeout with zero
evidence (cost: a 2-hour width-change bisect that ended in "fails at width 100 too", 2026-08-02).
Two rules: every suite's `reset_schema` must create EVERY table its delivery path touches, and
when a suite fails locally but CI is green, suspect suite-order leakage BEFORE code — prove it by
running the one suite against a fresh database.

**A worktree you push from must be EXCLUSIVELY yours, and commit with explicit pathspecs there
anyway (2026-08-08):** a reviewer subagent, needing a pristine-`main` scratch for a
negative-verification, found and used the coordinator's docs worktree — it copied the branch's
`tools/codegen-rs/src/tests.rs` in, and the coordinator's next docs commit there carried the
foreign +70-line hunk onto `main` (a test asserting a check `main` does not have → red codegen
gate; caught after push, reverted within a minute, one intermediate `ci` run red). Two rules.
Every agent dispatch names an EXCLUSIVE workspace and forbids using found checkouts — a clean
worktree is 200 ms to create, and "scratch" is where collisions breed. And enumerated `git add`
does NOT protect a shared tree: `git commit` commits the whole INDEX, so foreign staged content
rides along silently — in any tree another agent may have touched, commit with explicit
pathspecs (`git commit -F - -- <paths>`) or read `git diff --cached --stat` first.

**While a background agent owns the branch checkout, edit `main` through a git WORKTREE**
(`git worktree add <scratchpad>/main-wt origin/main -b <tmp>` → edit → push `<tmp>:main` →
`git worktree remove`): switching branches under a running agent yanks its files; and when the
stop-hook flags the agent's uncommitted WIP, leave it — the agent commits gated work itself;
committing under it snapshots untested state. (`git worktree remove` leaves the shell's cwd
dangling — `cd` out first or ignore the getcwd error.)

**That `push <ref>:main` form is not a style preference — the obvious alternative does not work
here.** The container's clone is SHALLOW (`.git/shallow` exists), so the local `main` branch is a
~50-commit GRAFT with no common ancestor to the freshly-fetched `origin/main`: `git checkout main
&& git merge --ff-only origin/main` dies with **`fatal: refusing to merge unrelated histories`**,
and `git log` on it shows real-looking commits that are simply a different history object. Do not
read that as divergent work someone will lose — confirm with `ls .git/shallow` and
`git merge-base --is-ancestor <local-main> origin/main`, then either push the ref directly or
`git checkout -B main origin/main`. Branch FROM `origin/main`, never from local `main`.

**The session's git credential can push a ref but cannot DELETE one.** `git push origin --delete
<branch>` and `git push origin :refs/heads/<branch>` both fail with `send-pack: unexpected
disconnect while reading sideband packet` — consistently, not transiently, so the retry-with-backoff
rule does not apply — and no GitHub MCP tool deletes a branch (`create_branch` exists, there is no
delete). **A branch created in-session cannot be cleaned up from in-session**: say so and leave it
to the founder rather than burning turns on syntax variants (cost: four retries plus a tool
search, 2026-08-05). The practical consequence: for a docs-only change that belongs on `main`, push
straight to `main` and never create the branch in the first place.

**Run `make rust` only on a COMMITTED tree, and read it ONLY by its exit code plus a post-gate
`git status --short` — never by its output.** `check-drift` regenerates and then diffs the WHOLE
tree — uncommitted source edits read as "drift" and fail the gate by design (its own comment says
so). And `make rust ... | tail` reports the PIPE's exit (tail's 0), so a background run can notify
"exit 0" over a red gate (cost: one commit pushed on a believed-green gate before the output was
re-read, 2026-08-01). Redirect to a file and echo `$?` separately, then read both. The same
`| tail` on `make test-crates` has a second failure mode even when you DO read the right exit:
it destroys the failure ENUMERATION — the per-suite detail naming which tests failed scrolls
away above the final summary line, so a red run tells you it failed but not where, and the only
recovery is running the whole target again (cost: one full re-run of a failing target,
2026-08-15). Redirect to a file and grep it; never window a test run through `tail`.
**The output is unreadable in BOTH directions, which is why the exit code is the only verdict.**
A GREEN `check-drift` is SILENT — it prints no success line at all (`Makefile:74-75`: the only
output is the failure `echo`), and the `✓ wrote <artifact>` list you see scroll past comes from its
`generate` dependency, so the tail of a PASSING gate is byte-for-byte the tail of a plain
`make generate` and tells you nothing. A RED one prints a `--stat` of whatever is dirty under a
message that only talks about generated files — i.e. your own uncommitted edits, listed as if they
were drift. So there is nothing to grep FOR: `make check-drift | grep -i drift` matches nothing on
success **and** returns grep's exit 1, i.e. a green gate reported as a failure — and on a red gate
the same pipeline reports grep's 0. Never infer this gate's result from its text.
**The procedure that always works: commit first → `make rust` → read `$?` → `git status --short`.**
Exit 0 with an empty status is the only green; a non-empty status names in one cheap command what
actually drifted, and whether the paths are yours or under `specs/generated/**` /
`crates/**/generated/**` decides commit-vs-regenerate without spending a second gate cycle (costs:
two wasted runs re-deriving a gate that had been green both times, 2026-08-12; a full ~4-minute
cycle spent investigating a non-problem plus a near-misreport in the round after it, 2026-08-14).
And `rust`'s closing `NOTE -- this gate does NOT run crates/** tests` is printed
**unconditionally** — it is a standing caveat, not a verdict on your diff: when the change touches
no `crates/**`, `make rust` IS the complete gate and that line is not asking you for anything.

**`ld terminated with signal 7 [Bus error]` at link time is the DISK ALLOWANCE, not a toolchain
fault.** The linker mmaps its output; at 98% used it dies with a bus error that looks like
corruption. `target/debug/incremental` alone held 9.2G — deleting just it recovers the build
without the full `target/debug` rebuild cost (cost: one mysterious "could not compile server"
mid-suite, 2026-08-01).

**Pass multi-line commit messages through `git commit -F -` with a quoted heredoc**, never `-m`
with a body containing backticks: bash command-substitutes `` ` `` inside double quotes, so a
`` `type: process-manager` `` in the message executed `type:` as a command and pushed a commit
with the phrase silently deleted (cost: one garbled money-path commit message, an amend and a
force-push, 2026-08-01). `git commit -F - << 'MSG' … MSG` (quoted delimiter) is immune.

**After a founder merge, diff CONTENT before declaring a remainder.** A squash merge takes
the WHOLE branch at its head — "the PO merged early, only slice N is in" is a belief about the
commit graph, not the content. Verify with `git diff main <branch-tip>` (and read the merged PR
body's checklist — its state at merge time is the record): if `main` is ahead and nothing is
missing, the work IS merged, whatever the in-flight session believed. Cost: issue #275 and a
proposal `Realized by` header both written as "only D2 merged" minutes after a squash that
contained D1+D2+D3 complete; the next session spent its opening verifying instead of building
(2026-08-01).

**The remote git proxy cannot DELETE branches.** `git push origin --delete <branch>` (and the
`:refs/heads/<branch>` form) dies with "the remote end hung up unexpectedly" — the per-session git
proxy only supports fetch/push of refs, not deletions — and the GitHub MCP toolset has no
ref-deletion tool either. Branch cleanup must happen from the GitHub UI or a normal clone; don't
burn retries on it (cost: three failed attempts before diagnosing, 2026-07-31).

**`cargo check -p actor_client` alone is RED on a green main — it is not your change.** The D6
lint floor's `unreachable_pub = deny` fires on the `bulk-door` items (`InboundFact`,
`enqueue_inbound_facts`): their re-export is feature-gated, `infrastructure` is the only crate
that lights the feature, and cargo resolves features per SELECTION — so a solo `-p actor_client`
build sees pub items nobody re-exports and denies them, while every workspace-level build (what
CI and `make rust` run) unifies the feature in and passes. Check that crate at workspace level,
or with `--features bulk-door`. The clean fix — `#[cfg(any(test, feature = "bulk-door"))]` on the
item definitions themselves — belongs to whoever next touches the bulk door, not to an unrelated
slice (cost: a false "my move broke the boundary crate" scare and a stash/verify round, 2026-08-03).

When wrapping up, state the handoff explicitly: what was pushed and to which branch, what remains on
the user's side, which decisions are blocking, and what the next code slice is.

## 12. A workspace GLOB member cannot bootstrap itself

`members = ["crates/clients/*"]` is the right shape for codegen-emitted crates — a new one joins the
workspace by being generated, so the list cannot drift from the spec. But **Cargo refuses to load a
workspace whose glob matches nothing**, and the generator that would create the directories is
itself a workspace member. The first `cargo run` after adding the glob therefore fails with
`failed to read crates/clients/*/Cargo.toml` before any code runs.

Bootstrap (cost: one wasted cycle in #306, and phase 3 will hit it again with ~17 more crates):

```bash
sed -i 's|^    "crates/clients/\*",|    # BOOTSTRAP|' Cargo.toml   # drop the glob
cargo run --manifest-path tools/codegen-rs/Cargo.toml -- --specs specs
git checkout Cargo.toml                                            # restore it
```

Related, same change: **have the emitter DELETE stale generated directories.** `check-drift` diffs
content, so it can never notice a directory that simply stopped being regenerated — and under a glob
that stale directory is still a workspace crate.

## 13. Build the narrow graph, not just the workspace

`cargo build --workspace` unifies cargo features across every member, so a crate that is only sound
*because a sibling lights a feature* looks fine. `cargo check -p <one-crate>` builds the real,
narrow graph and can fail where the workspace build passes.

In #306 the new per-actor client crates depend on `actor_client` with **no features at all** — a
configuration that had never been compiled, because until then only the whole workspace (where
`infrastructure` lights `bulk-door`) was ever built. Two `unreachable_pub`/`dead_code` findings on
the feature-gated bulk-door items surfaced immediately. They were pre-existing, not caused by the
change; the fix is `#[cfg_attr(not(feature = "…"), allow(…))]` scoped to the feature-off
configuration, never a blanket `allow`.

Corollary for any new crate: `cargo check -p <it>` **before** wiring consumers, or you debug the
crate's own feature assumptions through the noise of the whole workspace.

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

**Corollary**: after any unexplained gap, verify agent liveness before assuming a dispatch is still
running. A silent agent and a dead agent are identical from inside the session, and the difference
is one `ListAgents` call.

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
  is done. The isolation is per-DATABASE; the schema-level reset gives you none.
- **`pgrep -f "<anything from your own command>"` always matches YOUR shell**, because every Bash tool
  call runs inside a wrapper whose full script text is its command line. An
  `until ! pgrep -f "cargo test --workspace"; do sleep 30; done` written to wait for a neighbour's
  build therefore never terminates — it is waiting on itself. Cost here: two 10-minute background
  waits that outlived the build they watched. Match on the process name instead (`pgrep -x cargo`,
  `ps -eo comm`), which cannot see the wrapper's arguments.

### `make rust` does not compile the application

`rust: rust-build rust-test validate check-drift`, and both `rust-*` targets carry
`--manifest-path tools/codegen-rs/Cargo.toml` (`Makefile:68-73`). So the gate CLAUDE.md names as the
pre-push bar — "local gates are green (`make rust`)" — builds and tests **the codegen tool**, runs the
validator, and checks generated-artifact drift. It never compiles `crates/**`, and it runs none of
the workspace's tests.

A `crates/web` change that does not compile therefore passes `make rust`. So does a `crates/server`
integration test that fails: the `graphql_subscriptions` suite covering the #427 emitter change was
never run by any local gate, and a reviewer had to flag the hole after the fact. Two of the three
`make rust` invocations in that session reported OK while telling me nothing about the code I had
actually written.

**Run the crate-scoped tests yourself** — `cargo test -p web --features ssr`, `cargo test -p server
--test <suite>`, `make wasm` for anything in the browser half — and name them individually when you
report gates. "Gates green" without saying which is not a claim about the diff. CI does build the
workspace, so this bites as a late red on a PR you called ready, not as a bug in `main`.

**Cover every crate the diff touches — ENUMERATE them, do not choose them.** Avoiding `cargo test
--workspace` is a disk measure (§2), not a licence to test the crates that seem interesting. On
#451 the instruction was "run `-p server` and `-p application`"; the branch actually touched six
crates, and the failure was in the untested `crates/web` — a generated response key changed from
`cart` to `current`, a hand-written SSR fixture still scripted the old key, and CI went red on work
two rounds of local gates had called green. Derive the list instead of recalling it:

```sh
git diff origin/main...HEAD --name-only -- crates/ | cut -d/ -f2 | sort -u
```

And on a MULTI-PHASE branch, run the full `make test-crates` at every phase boundary, not only
the suites the phase touched: on 2026-08-15 two `crates/web` tests sat red for two whole phases
because every later run was targeted at the crates then being edited, and the red only surfaced
at the end — where it read as a late regression instead of pointing at the phase that caused it.
Targeted suites are for iterating inside a phase; the boundary gate is the full target.

**A green `make rust` proves nothing about `crates/**` — it never ran a line of it.** `rust-test`
(Makefile:70) is `cargo test --manifest-path tools/codegen-rs/Cargo.toml`: the codegen crate ALONE.
Since #474 the workspace suite is `make test-crates`, and `.claude/hooks/stop-gate.sh` invokes it
whenever the turn's diff touches `migrations/ | crates/ | the emitters | Cargo.{toml,lock}` — so it
is no longer something to remember, and the two gates still cover disjoint failures.

**A database is now REQUIRED, not optional.** The #230 polarity is inverted: with no `DATABASE_URL`
a DB-gated suite PANICS with the command to run it, and the only way out is an explicit
`DB_TESTS_REQUIRED=0`, which prints a summary naming every skipped suite. So there is nothing to
set in the happy path — just export `DATABASE_URL` (the ~40s `initdb` recipe is §above).

**Derive that count, never quote it** — it moves with every DB-gated test added, and it was already
wrong twice in one branch (prose said 42 while the run printed 45; the run printed 45 while 50 suites
had really skipped, because `actor_runtime`'s local copy of the gate was not writing the receipt):

```sh
cut -f1 target/db-test-skips.log | sort -u | wc -l   # how many skipped, after a DB_TESTS_REQUIRED=0 run
cut -f1 target/db-test-skips.log | sort -u           # ...and which
awk '/^test result:/{p+=$4; f+=$6} END{print p, f}' <log>   # the pass/fail totals, same reason
```

**Why that needed a receipt file rather than a louder `eprintln!`: libtest captures a passing test's
stderr as well as its stdout.** The per-suite `SKIP` lines this repo relied on since #230 were not
merely quiet, they were **unobservable** — `grep -c SKIP` over the full 990-passed baseline log
returns **0**. Anything a passing test prints is invisible without `--nocapture`, so no improvement
to the message could ever have worked; `make test-crates` reads `target/db-test-skips.log`, which
the gate appends to, and prints the summary itself. Worth knowing before designing any other
"the test warned you" mechanism in this repo. On #451 that silence hid a migration that bricked the
Cart projection through three local gate rounds; CI's Postgres job found it.

**The reverse trap: `cargo check` is equally partial.** A STALE generated file compiles perfectly —
`57b7330` built clean with `crates/server/src/graphql/generated/query.rs` still holding an
`Err("not implemented")` stub for a resolver whose body the emitter already carried, because
nothing had re-run `make generate`. Only `check-drift` catches that. So the two gates cover
disjoint failures and **neither subsumes the other**: `make rust` proves the specs and the
generated tree agree, `cargo check`/`cargo test -p <crate>` prove the code you wrote works. A
change to `crates/**` needs both, in that order (generate first, or check-drift fails on your own
uncommitted regeneration).

The executable fix (folding a workspace build+test into `rust`) is a Makefile change, so it goes
through the claim -> PR flow rather than riding a docs commit.

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
([ADR-20260816-020752](../adr/ADR-20260816-020752-the-loops-context-budget-a-dispatch-card-snapshot-semantics-and-phase-commits.md)
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
  integration`**, so a session cannot re-run a job — the only lever is a new push, and `ci.yml`
  triggers on EVERY branch push, so any commit does it (a `--allow-empty` one whose message records
  the flake, if there is nothing real to land).

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
concurrent executors with worktree isolation** (`git worktree add`, with the disk caveats above),
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
unverified work as finished, which is the same defect the "seen red" rule above exists to prevent.

### Do not push a feature branch while its executor is still working

A stop-hook prompt reported an unpushed commit; the coordinator pushed it, the executor then
amended that same commit (adding `.claude/loop-budget.json`), and local and remote diverged —
identical content, different SHAs — needing a `--force-with-lease` to realign. **An
unpushed-commit prompt is not a signal that the work is finished.** The coordinator pushes only
after the executor reports the phase complete and the tree is clean; the executor says explicitly
whether it intends to amend before handing back.

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

## 18. A CI-workflow change: does it fit the job's timeout, and does it regress the rollback path?

A mob briefing for a change to a `.github/workflows/*.yml` step must ask two questions no code lens
raises on its own: **does the step fit the job's EXISTING `timeout-minutes`?** and **does it regress
the ROLLBACK path** (the deploy job is what an incident runs to roll back — a slow step there is a
slow rollback). [#444](https://github.com/TheCaptainCompany/captain-food/issues/444) added a
`cargo build ... --bin secret-gate` as the first step of `deploy.yml`, and because the gate lived in
`tools/codegen-rs`, that build dragged the guppy/determinator tree — a COLD compile of minutes on a
cache miss, inside a `timeout-minutes: 10` job. Nobody at the briefing (farley included) asked either
question; the review caught it post-merge, and the fix ([#453](https://github.com/TheCaptainCompany/captain-food/issues/453))
was to extract the gate to a serde-only crate whose cold build is seconds. **The durable proof of
"cheap enough" is the dependency tree (`cargo tree -p <it>` = the lean set), not a warm-cache
wall-clock** — a green deploy on a warm runner hides the cold-cache tail that a rollback hits first.

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

**The rule that cost the most to learn, on that same first use**: *check the register before you ask.*
One of the six questions asked which funding model applied, and
[ADR-20260808-203443](../adr/ADR-20260808-203443-tips-voluntary-contributions-funding-model.md) had
decided it ten days earlier. His answer began *"We already discussed about that."* A question about a
settled decision spends his attention and reads as the team not knowing its own records — grep
`docs/adr/`, `docs/proposals/DECISIONS.md` and `docs/STATUS.md` for the subject of every question
before the form is published.

Ask only what is genuinely his: a real option space, an external or legal action, or a fact only he
knows. Order the questions by dependency and say so with the `gates` field. Never make a field
required, and always end with a free-text question.
