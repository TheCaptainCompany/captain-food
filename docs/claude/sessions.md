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

`make rust` is `cargo build` + `cargo test` + validate + generate + drift check. On a warm cache it is
slow; on a cold one it rebuilds the whole workspace. Do not reach for it to prove a Markdown edit.
CLAUDE.md already permits skipping it for docs-only changes — the point here is that the saving is
minutes per invocation, so it is worth being deliberate.

**`check-drift` fails on ANY dirty file, and says the wrong thing about it (2026-08-04):** it diffs
the whole working tree, not just generated paths, so an uncommitted hand edit — a `Cargo.toml`
tweak, a doc fix — trips it with `generated artifacts drifted -- run 'make generate' and commit the
regenerated files`. Running `make generate` then changes nothing and the failure repeats. Read the
`--stat` line it prints directly above: if the listed files are yours rather than generated ones,
the fix is to **commit your own change**, not to regenerate. Real drift names files under
`specs/generated/**` or `crates/**/generated/**`.

**`cargo test --workspace` without `DATABASE_URL` is a green that covers nothing (2026-08-04):** the
DB-gated suites take their early-return branch and report `ok`. On 2026-08-04 a local run reported
**86 suites / 847 passed** and still missed the failure CI then hit — with a real database the
`infrastructure` package alone contributes **29 suites / 87 tests** that had all silently skipped.
`ci.yml` already warns about this for CI; the part nobody had written down is that you can run them
**here**: Postgres 16 is installed in this container (there is no Docker), it is simply not started.

```bash
PGDATA=/var/lib/postgresql/ci-repro                       # initdb REFUSES to run as root --
mkdir -p "$PGDATA" && chown postgres:postgres "$PGDATA"   # do it as the postgres user
chmod 700 "$PGDATA"
su postgres -c "/usr/lib/postgresql/16/bin/initdb -D $PGDATA -A trust -U postgres"
su postgres -c "/usr/lib/postgresql/16/bin/pg_ctl -D $PGDATA -l $PGDATA/server.log start"

# sqlx-cli is NOT installed (CI gets it prebuilt from taiki-e/install-action, and building it
# here costs minutes). psql applies the migrations, but needs a per-file fallback: some
# migrations require a transaction (LOCK TABLE) and others forbid one (VACUUM), which is why a
# plain loop dies partway either way. Try -1 first, retry without it.
for f in $(ls migrations/*.sql | sort); do
  PGPASSWORD=postgres psql -q -1 -h localhost -U postgres -v ON_ERROR_STOP=1 -f "$f" >/dev/null 2>&1 \
    || PGPASSWORD=postgres psql -q -h localhost -U postgres -v ON_ERROR_STOP=1 -f "$f" >/dev/null
done

export DATABASE_URL="postgres://postgres:postgres@localhost:5432/postgres"
cargo test -p infrastructure -- --test-threads=1        # --test-threads=1: they share ONE database
```

Cost that earned it: a CI-only failure on a build-profile PR that could not possibly change
behaviour, and an hour of diagnosis that a local DB run would have front-loaded.

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
this plus dropping `incremental/`.

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

**While a background agent owns the branch checkout, edit `main` through a git WORKTREE**
(`git worktree add <scratchpad>/main-wt origin/main -b <tmp>` → edit → push `<tmp>:main` →
`git worktree remove`): switching branches under a running agent yanks its files; and when the
stop-hook flags the agent's uncommitted WIP, leave it — the agent commits gated work itself;
committing under it snapshots untested state. (`git worktree remove` leaves the shell's cwd
dangling — `cd` out first or ignore the getcwd error.)

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
