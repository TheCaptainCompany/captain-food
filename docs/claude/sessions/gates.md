# Session rules — gates and builds

Part of [`../sessions.md`](../sessions.md).

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
([ADR-20260816-020752](../../adr/ADR-20260816-020752-the-loops-context-budget-a-dispatch-card-snapshot-semantics-and-phase-commits.md)
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

**On that route, `remote: Bypassed rule violations for refs/heads/main:` followed by a list of
required checks is the EXPECTED output, not a failure.** The docs-straight-to-main directive means
the session's credential holds a bypass on `main`'s ruleset, so every such push prints the rule it
bypassed and the checks that did not run — on a push that fully succeeded (`git push` exits 0 and
the ref moves). Read the exit code and the ref update, not the `remote:` block. Cost: two sessions
spent attention deciding whether it was an error, and the next step after a "failed" push is a
retry loop that pushes nothing.

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
--workspace` is a disk measure
([§2](environment.md#2-disk-is-a-fixed-per-session-allowance-and-df-lies-about-it)), not a licence
to test the crates that seem interesting. On
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

## 19. Shapes a gate test keeps reproducing

From [#679 "RETRIEVAL-QMD-CI decided: the decision-lookup stub suite runs in
CI"](https://github.com/TheCaptainCompany/captain-food/pull/679) — many independent review passes,
all FAIL before the state that merged. **The recurring defect was never the mutants: it was that
every round's completeness claim was written before it was checked.** The early rounds found holes
in the gate; most of the later ones were about the tests FOR the gate, and a large share of those
were regressions introduced while fixing the previous round's finding.

*No total is stated here on purpose.* The first version of this paragraph said **thirty-three**
while the PR body said **thirty-one**, and both then split the same set as "the first thirteen"
plus "the last eighteen" — 31, not 33 — in the section whose own closing bullet says a derived
number stated in prose is consumed as established fact and must be derived instead. The round entries in
`docs/status/journal-2026-W35.md`, and the `(Review #NN)` citations in `tools/codegen-rs/src/**`,
are where a count can be re-derived from something **committed**. (Review #36.)

*The first version of this sentence pointed at the PR's round table instead — which is the same
defect one level up: CLAUDE.md says **GitHub is never the record**, this section exists because the
shapes were living only in a PR body, and a body is editable, unversioned and invisible to
`make validate`. It was also already stale by seven rounds when it was caught. A repo record may not
delegate its antecedent to a surface that disappears with the branch. (Review #58.)*

These are not derivable from the code, each cost a round, and **several of them reappeared inside
the very guard written to close them** — which is why they are here rather than in a comment next to
one instance. *The heading counted them for one round, which is this section's own closing bullet
committed in its title: the list grows, nothing derives the number, and the first addition made it
wrong. Dropped rather than corrected — the third application of that remedy on this branch, after
the `~30×` multiplier and the enumerated CI caps. (Review #63.)*

1. **A corpus-size floor counts plants, not coverage.** `must_red.len() >= N` says nothing about
   *which* assertions have a plant. The only way to know an assertion is pinned is to delete it and
   watch something go red. Measured: eight assertion families in one helper were each held up by a
   sentence — delete any one and every mutant and control stayed exactly as it was.
2. **`assert_ne!(mutated, original)` proves a mutation applied, never that it applied where the
   label says** — and a plant that does not apply *at all* is the sharper version of the same
   defect, because it yields a false conclusion **about the code** rather than a weak one about the
   test. A mutation driven from a shell one-liner whose escaping mangled an `&` made `str.replace`
   match nothing and change no bytes; the test stayed green, which reads identically to "the guard
   does not discriminate here" — and that inference got acted on, rewriting a control that was
   fine. **Assert the plant applied, in the same command that runs it.** `replacen(anchor, .., 1)` rewrites the FIRST match in the whole file, and CI job
   bodies are near-identical: one anchor occurred in five jobs, so a plant labelled `build-test …`
   silently mutated `lint`, satisfied `assert_ne!`, and pinned nothing.
3. **A plant that fails for the wrong reason is worse than none** — it reports the guard working
   while proving nothing. Two spellings: a mutant that reds on a YAML *parse* rather than on the
   property, and a fixture whose cases leak into each other (stacked commits made a diff
   cumulative, so four later cases were passing on an earlier case's file).
4. **A guard that inspects declared configuration is blind to the same thing done imperatively.**
   `env:` is a mapping a test can read; `echo "PATH=…" >> "$GITHUB_ENV"` needs no `env:` key at all.
   A `case`-arm allowlist is text a test can read; one appended `docs_only=true` overrides every arm
   without touching one. **The answer is to execute the thing and assert its output**, not to read
   it harder.
5. **A fixture set drawn from the shape you were thinking about proves only that shape.** The same
   hard-wrap defect returned three times wearing `-`, `1.` and `#`.
6. **A token that carries the exempting word is not evidence of an explanation.** Blank the citing
   token before testing the clause around it.
7. **A helper that constructs test inputs needs the same scrutiny as the assertions it feeds.**
   Three rounds of "is this plant pinning what it claims" all pointed at the plant *list*; none at
   the function building them — which was itself mislabelled, and swallowed a `permissions:` block.
8. **A fix verified by READING is verified against the tree you have.** Three rounds closed three
   silent ways for a file to leave the citation corpus, each checked by reading the code, and each
   missed the fourth — because the property under test is *what happens to a shape this tree does
   not contain*, and the tree contains zero such files. Reading proved the code did what it says;
   it could not show what the code does to an input nobody had written down. **Build the fixture
   that has the shape.** "I closed the other three by reading" is the reason the fourth survived,
   not evidence against it. Corollary, from the same round: **a test that checks one half of a
   description cannot detect that the other half overstates** — the records described the corpus by
   its *pathspecs*, the guard compared pathspecs, and the unstated extension filter made both
   records silently wider than the code.

9. **A justification inherited by copy is not a justification at the site it now governs.** Twice
   on this branch a `timeout-minutes` cap's reasoning was reused for a job it was not true of —
   `lint` bucketed with jobs that compile nothing, then `docs-validate`'s paragraph pasted onto
   `specs`, whose `if: docs_only != 'true'` means it never runs on the lane that argument turns on.
   Both were written by an author arguing, in the same comment, against inheriting a number from a
   different job — which is why prose cannot hold it: the next paste looks exactly like the last.
   Gated against the shape that happened — `no_two_jobs_share_a_substantial_timeout_justification`
   is a **byte-identity** check, so it stops a verbatim paste and not a paste with one word changed;
   no textual rule can decide whether a justification is *true* of the job it sits on. A short
   pointer to another site is fine and is the correct way not to repeat one.

10. **A `///` run binds to the following ITEM, and nothing between two paragraphs says "new
   docstring".** Four times on this branch a doc comment ended up on the wrong item: twice in
   `validate/decisions.rs` (a paragraph left two functions up; `struct Unit`), once on
   `validate_decisions_index_sync`, once when a new test was inserted directly under an existing
   test's docstring with no item between — so both bound to the new test and the old one shipped
   undocumented. The damage is always the same shape: **the governing rationale for X is displayed
   as the rationale for Y**, and the reader looking for "which test enforces this" finds one that
   never opens the file in question. A blank line does not break the run.

   **Deliberately NOT gated, which is the point of the entry.** Every instrument available is
   heuristic — "a paragraph that looks like an opening" false-reds on this file's own mid-docstring
   ALL-CAPS headings — and `missing_docs` does not reach private items, so the compiler-first lever
   is absent. On a gate guarding the required check, this file's standing rule is that a false red
   costs more than a latent miss. So it stays a **reading** rule: when you insert an item above an
   existing one, look at what is now directly above the item below you.

- **An edit to a surface outside the repo has no diff, so "I updated it" is a claim with no
  antecedent.** A PR body, an issue, a project field: `git diff` proves a file edit landed; nothing
  proves one of those landed except re-reading it. On this branch a "what decides this merge" box was
  drafted to a scratchpad, announced as live in two replies, and never posted — hiding the diff's one
  **founder-owned** open row from the person the body is written for, where no gate could see it.
  **Re-read the surface after writing it, and prefer stating what a reader can verify** ("the box
  names three rows") **over what only the author can** ("I updated the box"). This is the concrete
  reason CLAUDE.md says GitHub is never the record.

And from the same branch, about the records rather than the tests — **uncounted on purpose**. This
line said *"Two more"* while introducing three, in the paragraph whose own first bullet is *derive it
or drop it*; the numbered heading above was dropped for the identical reason one round earlier, and
the list grew again the round after that. Fourth application of the same remedy on one branch, which
is itself the argument: **a list that grows has no business stating its own length.**

- **A count retracted twice will be retracted a third time — derive it.** A derived number stated in
  prose is consumed as established fact and nothing re-derives it
  ([ADR-20260817-105845](../../adr/ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)).
  A citation defect reads as correct whenever someone checks the *value* instead of the antecedent.
- **A verification recipe is itself a derived claim.** "Read its last three lines" was written from
  the intended shape and not from a log; the step prints one line. Read it off a real run, or state
  a property that cannot drift.
- **An edit to a surface outside the repo has no diff, so "I updated it" is a claim with no
  antecedent.** A PR body, an issue, a project field: `git diff` proves a file edit landed; nothing
  proves one of those landed except re-reading it. On this branch a "what decides this merge" box was
  drafted to a scratchpad, announced as live in two replies, and never posted — hiding the diff's one
  **founder-owned** open row from the person the body is written for, on a surface no gate can see.
  **Re-read the surface after writing it, and prefer stating what a reader can verify** ("the box
  names three rows") **over what only the author can** ("I updated the box"). This is the concrete
  reason CLAUDE.md says GitHub is never the record.
- **A measurement is defined by what it excludes, and the omission runs permissive.** Taking a
  number is not the end of the antecedent problem — it relocates it. A "cold build, 36s" measured an
  empty target dir against a *warm* registry, which is half the path a cold CI runner walks; the
  figure was true and the multiplier built on it was not. **Say which caches were warm, in the same
  sentence as the number.** And when the honest re-measurement cannot be taken, "unmeasured" is a
  publishable answer — decide on the asymmetry instead, and say that is what you did. This one
  landed on the author, one round after writing the rule it breaks.
