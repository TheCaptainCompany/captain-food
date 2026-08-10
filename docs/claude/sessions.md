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

**CLAUDE.md's architecture summary can be STALE — check it against `docs/STATUS.md` whenever
hosting, storage or deployment topology matters.** Nothing regenerates that paragraph and no gate
covers it, so it drifts silently in the one file every session reads first. Measured cost: on
2026-08-10 it still said *"Managed Postgres"* and cited the superseded `ADR-20260731-061609` for
hosting, when the decision has been **CNPG in-cluster on OVH MKS** since `ADR-20260807-002705` — the
product owner had to correct a session by hand on a fact the repo should have supplied. The cheap
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

**Set `DB_TESTS_REQUIRED=1` every time, and make it the habit rather than `DATABASE_URL` alone.**
The gated tests assert on it (#230): with it set and `DATABASE_URL` missing, a suite that would have
skipped FAILS instead of printing `ok`. Without it the skip is silent, and a full-suite total looks
identical whether the DB tests ran or not — which is exactly how this rule gets re-learned. On
2026-08-05 a session read this section's warning, ran `cargo test --workspace` with neither variable,
saw **857 passed / 0 failed**, and pushed; CI then failed on a hand-written test schema still
declaring `slug TEXT NOT NULL` for a column the change had made nullable. The same command with both
variables set reproduced it locally in seconds. The number is not the evidence — the variables are.

Cost that earned it: a CI-only failure on a build-profile PR that could not possibly change
behaviour, and an hour of diagnosis that a local DB run would have front-loaded.

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

**`check-drift` fails on ANY dirty file, and says the wrong thing about it (2026-08-04):** it diffs
the whole working tree, not just generated paths, so an uncommitted hand edit — a `Cargo.toml`
tweak, a doc fix — trips it with `generated artifacts drifted -- run 'make generate' and commit the
regenerated files`. Running `make generate` then changes nothing and the failure repeats. Read the
`--stat` line it prints directly above: if the listed files are yours rather than generated ones,
the fix is to **commit your own change**, not to regenerate. Real drift names files under
`specs/generated/**` or `crates/**/generated/**`.

**A PR "waiting on checks" may not be waiting on checks at all — read `mergeable_state` FIRST
(2026-08-09).** `pull_request_read` with `method: get_status` returns `{state: pending,
total_count: 0}` for BOTH "the required check is queued" and "this PR has a merge conflict", so a
conflicted PR looks exactly like a slow runner. The tell is on the PR object, not the status:
`mergeable_state: "dirty"` = conflict (also `"behind"`, `"blocked"`); `get_status` never says so.
Cost: ~40 minutes of heartbeats attributing a real conflict to runner backlog while auto-merge sat
armed and could never fire. Habit: when an armed auto-merge does not land within one CI cycle, call
`pull_request_read method:get minimal_output:true` and read `mergeable_state` before blaming the
platform. Related: the conflict was SEMANTIC, not textual — one branch moved every
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

**After this cleanup, distrust the FIRST post-cleanup `cargo run` result (2026-08-08):** one
`make rust` check-drift ran a STALE `generate` binary right after the deps sweep and mass-pruned
the five freshly generated `crates/bins/adapter-*` crates (a diff of 4 775 deletions that looked
exactly like an emitter bug); the immediate rerun rebuilt and passed with zero drift. Cost: ~30
min of debugging a phantom. If check-drift fails with an implausible mass-deletion right after a
`target/debug` cleanup, rerun it before touching the emitter.

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

Cost: a live incident, and ~30 minutes to diagnose from a startup log the product owner happened to
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

**Context discipline — the rules that keep a session under ~80k** (2026-08-01, after a week at
87% of requests >150k context): (1) `specs/generated/**` and `crates/**/generated/**` are
GREP-ONLY — never `Read` a generated artifact wholesale (`documentation.generated.md` alone can
eat a third of a session); (2) GitHub MCP calls use `minimal_output: true` and small `perPage`
unless the full payload is the point — a bare PR `get_diff` on a large PR returns megabytes
(fetch the branch and use local `git diff` instead); (3) fan-out exploration goes to SUBAGENTS
(Explore/reviewer/generator), never inline — their transcripts stay out of the main context;
(4) ONE SESSION PER WORK CHUNK (CLAUDE.md rigor rules) — the repo carries the state, so ending a
session is free and long context measurably raises the staleness error rate.

**Coordinator/executor split** (product-owner directive, 2026-08-07): a session that has planned a
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

## 11. Installing a dev tool: crates.io works, GitHub release downloads do not

The session proxy scopes GitHub — the REST API **and** release-asset downloads — to the
repositories attached to the session: `curl https://api.github.com/repos/<other-owner>/...`
returns 403 with an `add_repo` hint, and a `releases/latest/download/<asset>` URL "succeeds"
with a tiny error body that only surfaces when `tar` rejects it (cost: one debugging round while
fetching a prebuilt cargo-machete, 2026-08-03). `cargo install <tool> --locked` from crates.io
works fine (~2–3 min compile) — go straight there for Rust tooling; do not burn turns on
prebuilt-binary URLs.

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
to the product owner rather than burning turns on syntax variants (cost: four retries plus a tool
search, 2026-08-05). The practical consequence: for a docs-only change that belongs on `main`, push
straight to `main` and never create the branch in the first place.

**Run `make rust` only on a COMMITTED tree, and never judge it through a pipe.** `check-drift`
regenerates and then diffs the WHOLE tree — uncommitted source edits read as "drift" and fail the
gate by design (its own comment says so). And `make rust ... | tail` reports the PIPE's exit (tail's
0), so a background run can notify "exit 0" over a red gate (cost: one commit pushed on a
believed-green gate before the output was re-read, 2026-08-01). Redirect to a file and echo `$?`
separately, then read both.

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

**After a product-owner merge, diff CONTENT before declaring a remainder.** A squash merge takes
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

**The interactive decision form (2026-08-08, product-owner directive: keep this approach)** — when
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
  before it runs.
- **A dead agent's worktree is the first thing to reclaim, and it is free**: check
  `git status --porcelain` and `git log -1` against the pushed head; clean at the remote sha means
  nothing to lose. Here that was 8.8G — more than the whole review needed, and cheaper than
  deleting `target/debug`, which costs a rebuild.
- **`git worktree add <abs-path>` from a reset cwd can land the tree INSIDE the repo.** It did here,
  creating `captain-food/cad2-wt` — a worktree nested in its own repo, showing up as an untracked
  directory one `git add -A` away from being committed. Verify with `git worktree list` after adding,
  and remove with `git worktree remove --force` rather than `rm -rf`.

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

**A green `make rust` proves nothing about anything touching `migrations/**` or a `View_*`.** Every
DB-gated suite SKIPS without `DATABASE_URL`, counting as passed, and `rust-test` (Makefile:70) does
not set one. **Run those suites with `DATABASE_URL` set and `DB_TESTS_REQUIRED=1`** — the loud-skip
flag already exists (#230, documented in the headers of
`crates/infrastructure/tests/main/mailbox_activations.rs`, `mailbox_retention.rs` and
`standalone_workers.rs`); it turns a silent skip into a panic. Without it, `cargo test -p
infrastructure` reports a clean pass having executed none of the migration chain. On #451 that
silence hid a migration that bricked the Cart projection through three local gate rounds; CI's
Postgres job found it. A local Postgres is cheap — `initdb -A trust` + `pg_ctl start` in a writable
dir, then point `DATABASE_URL` at it.

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

### A "seen red" claim must name HOW the test was made to fail

Not that it failed — **how**: the clause deleted, the fallback re-planted, the stub it ran against.
A claim a reader cannot re-run is not evidence, and the repo already contains both kinds. The good
ones say what was mutated — `crates/server/src/auth.rs` ("Seen RED by re-planting #430's
fallbacks"), `crates/infrastructure/tests/main/scope_membership.rs` ("Seen RED by deleting the
EXISTS clause from `PgOrderRepository::list`") — and neither names a commit, correctly, because the
mutation was made by hand and never committed.

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

### Do not push a feature branch while its executor is still working

A stop-hook prompt reported an unpushed commit; the coordinator pushed it, the executor then
amended that same commit (adding `.claude/loop-budget.json`), and local and remote diverged —
identical content, different SHAs — needing a `--force-with-lease` to realign. **An
unpushed-commit prompt is not a signal that the work is finished.** The coordinator pushes only
after the executor reports the phase complete and the tree is clean; the executor says explicitly
whether it intends to amend before handing back.

### One more shell trap in commit messages

`git commit -m "…"` with **backticks** inside the double quotes runs command substitution: a message
containing `` `system` `` silently lost the word and committed the gap. The existing ASCII rule
covers Makefile recipes; this is the same class one layer over. Write any commit message with
backticks, `$`, or `!` to a file and use `git commit -F <file>`.

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
