#!/usr/bin/env bash
# Captain.Food acceptance gate (Claude Code Stop hook).
# Blocks loop/turn completion unless the DSL model is valid and generated artifacts are in step.
# Covers schema + behaviour + observability + C4 (all via the codegen validator). App-level gates
# (unit tests, lint, build) run only when they exist, so this is safe in a specs-only repo.
# Exit 0 = gates pass (allow stop); exit 2 = gates fail (block, stderr is fed back).
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# The codegen is the Rust tool (ADR-0034); make sure cargo is reachable in the hook's shell.
export PATH="$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null 2>&1 || { echo "stop-gate: cargo not found on PATH — install the Rust toolchain (rustup)." >&2; exit 2; }
MANIFEST="$ROOT/tools/codegen-rs/Cargo.toml"
[ -f "$MANIFEST" ] || { echo "stop-gate: tools/codegen-rs not found" >&2; exit 2; }

# Under Cygwin the rustup `cargo` proxy mis-detects its own argv[0] and runs as `rustup`, so any
# `cargo run` fails with "invalid value 'run' for '[+toolchain]'"; route through `rustup run` there.
CARGO=(cargo)
case "$(uname -s 2>/dev/null)" in
  CYGWIN*) CARGO=(rustup run "${RUST_CHANNEL:-stable}" cargo) ;;
esac
# ...and a native Windows cargo cannot read Cygwin/MSYS `/cygdrive/...` paths: hand it `C:/...`.
winpath() { if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi; }
MANIFEST="$(winpath "$MANIFEST")"
SPECS="$(winpath "$ROOT/specs")"

fail=0
step() { echo "→ $*"; "$@" || fail=1; }

# `cargo run --check` builds first (the compiler is the type gate) then runs the full validator
# (§1–§11: schema + actor wiring + behaviour/rules coverage + observability + C4); exits 1 on errors.
step "${CARGO[@]}" run --quiet --manifest-path "$MANIFEST" -- --check --specs "$SPECS"

# --- The workspace test gate (#474), DIFF-SCOPED. ---
#
# `make rust` above is the SPEC gate: it builds and tests tools/codegen-rs only, so it proves
# nothing about crates/**. A migration defect that permanently bricks the Cart projection passed
# THREE green `make rust` rounds on the #451 branch and went red only in CI. Documentation is not a
# gate; in this repo the contributor is an agent and this hook is its muscle memory.
#
# Scoped to the paths whose behaviour lives in the workspace suites — a docs- or specs-only turn
# must stay fast, and CLAUDE.md explicitly allows skipping the heavy gate for those. Scoping decides
# whether the suite is MANDATORY, never whether it silently vanishes: when it is in scope it runs,
# and `db-test-gate` makes a missing database a failure rather than a skip.
CODE_GLOBS='^(migrations/|crates/|tools/codegen-rs/src/emit/|Cargo\.toml|Cargo\.lock)'
changed=""
if git -C "$ROOT" rev-parse --git-dir >/dev/null 2>&1; then
  # Working tree + index + everything this branch adds on top of main: a turn that already
  # committed its work must not slip past the gate because `git status` is clean.
  base="$(git -C "$ROOT" merge-base HEAD origin/main 2>/dev/null || echo '')"
  if [ -z "$base" ]; then
    # No `origin/main` to compare against (a fresh single-branch clone, a shallow CI checkout, a
    # detached worktree whose remote ref was pruned): the branch-diff half above would evaluate to
    # NOTHING and a turn that already committed its work would slip through on a clean
    # `git status` -- the exact case the comment above says this covers. Fail SAFE, like the
    # no-git path below: scope cannot be computed, so do not guess -- run the suite.
    changed="crates/UNKNOWN"
  else
    changed="$(
      {
        git -C "$ROOT" status --porcelain --untracked-files=all | sed 's/^...//'
        git -C "$ROOT" diff --name-only "$base"...HEAD
      } 2>/dev/null | sort -u
    )"
  fi
else
  # No git (a tarball checkout, a sandbox): scope cannot be computed, so do not guess -- run it.
  changed="crates/UNKNOWN"
fi

if printf '%s\n' "$changed" | grep -Eq "$CODE_GLOBS"; then
  echo "-> make test-crates (diff touches migrations/ | crates/ | emitters -- #474)"
  ( cd "$ROOT" && make test-crates ) || fail=1
else
  echo "stop-gate: no migrations/ | crates/ | emitter changes -- workspace tests not required for this turn (#474)."
fi

# --- The loop-budget invariants (ADR-0014, ADR-20260812-011057). ---
#
# ALWAYS: the ~10ms audit. It refuses a committed open timer (`startedAt`) or a resurrected mutable
# counter (`secondsUsed`) in tracked budget state. Both are silent corruptions of the weekly cap
# rather than crashes -- a committed timer billed 261 phantom minutes to an unrelated 16-minute run,
# and a counter merge discarded 2.5h of another session's recorded time. Neither shows up in any
# other gate, and a committed timer travels to every branch that checks it out, so this runs on
# EVERY turn regardless of what the diff touched.
step bash "$ROOT/.claude/hooks/loop-budget.sh" audit

# ALWAYS: the register-check discipline (~100ms, pure shell, no fixtures touched). Founder
# directive 2026-08-21 (ADR-20260821-010543): agents must not re-ask answered questions. The gate
# is a PreToolUse hook that is silent-when-broken by shape (a matcher typo or a dropped settings
# entry disarms it with no signal -- ADR-20260810-231300's defect class), so its selftest runs
# every turn: hook verdicts, settings wiring, and the agent files' citation blocks.
# REGISTER_CHECK_ALLOW_DIRTY: the selftest compares ALL FOUR gate scripts (both hooks and both
# decision-lookup scripts) against their committed
# blobs and refuses to report otherwise (the overwrite class the eighth review of PR #679 planted
# green). Editing a hook and re-running is the normal interactive loop, so `make hooks-test` opts
# out unconditionally and THIS caller opts out only when a gate script is actually dirty -- see the
# block below, which is where that condition is computed and argued. CI invokes the
# script directly and verifies. Those two are the whole list. The CI path cannot be talked out of
# verifying because `env_ok`, inside `assert_pinned_in_changes_job`, forbids both opt-out names as
# `env:` keys at every scope -- planted red by `both_scopes_reject_execution_altering_env`. This
# line used to credit `assert_gate_script_self_verifies`, which never opens `ci.yml` at all: it
# asserts things about the two SHELL SCRIPTS. A maintainer following the wrong name would have
# found no `env:` handling there and concluded the ban was refactored away -- round 9's own finding
# ("a comment named a test as the thing preventing the regression; that test did not exist") one
# file over, in a sentence written after it. (Review #17.)
#
# THE OPT-OUT IS CONDITIONAL, AND WHAT THAT BUYS IS NARROWER THAN REVIEW #72 CLAIMED FOR IT.
#
# THE CLAIM, RETRACTED: "an in-session `cp exit0.sh <gate script>` is now caught on the next turn
# rather than on push." FALSE, and it cannot be made true by any working-tree predicate. The two
# tests are COMPLEMENTS: `git status --porcelain` over these paths is non-empty exactly when the
# worktree bytes differ from HEAD, which is exactly when the selftest's `hash-object` comparison
# would mismatch. So dirty implies opted out -- the comparison is skipped in precisely the state
# where it could fail -- and clean implies it runs and passes by construction. Measured, not
# reasoned: an ordinary `cp` over `register-check.sh` yields OPTED OUT. Nothing in the working tree
# separates "the maintainer is editing a gate script" from "a gate script was overwritten", because
# at the byte level they are the same state. (Review #79 of PR #679, correcting review #72.)
#
# WHAT IT DOES DELIVER, and it is not nothing: the tamper that HIDES FROM `git status`. A
# `git update-index --assume-unchanged` or `--skip-worktree` on a gate script leaves the status
# clean while the bytes differ -- this block then ARMS, and the selftest reds with
# `differs from the committed blob at HEAD`. Measured too. That is the stealthier class and the one
# an attacker who has read this file would reach for, so the coverage is inverted from the naive
# reading: the CLUMSY overwrite is caught at push, the CAREFUL one on the next turn.
#
# Pinned by `the_stop_gate_predicate_discriminates_a_hidden_tamper` -- both directions, against a
# throwaway repo -- because this rationale has now been wrong once and a sentence is what carried it.
#
# THE RESIDUAL, AND THE REPAIR DELIBERATELY NOT TAKEN. A session that legitimately edits ONE gate
# script and overwrites ANOTHER in the same turn is opted out for both. Reviews #81/#82 propose the
# obvious close: make the predicate per-FILE and keep the comparison armed over the clean ones.
# NOT DONE, and the reason is this branch's own subject: the scripts refuse wholesale, so per-file
# needs a new variable carrying a skip-list into them -- a NEW OPT-OUT LEVER on the gate surface.
# Every lever is a disarm route, `env_ok` would have to learn to ban it in CI, and eighty rounds
# here have been about levers that did not do what their sentence said. Closing a narrow residual by
# widening the disarm surface is the wrong trade on the gate set that guards the required check.
# Recorded so the next author meets it as a decision rather than an oversight.
#
# THE PREDICATE IS THE WORKING TREE, NOT THE BRANCH DIFF, and the difference decides whether this
# is a no-op. `$changed` above folds in `diff "$base"...HEAD`, so on THIS branch -- which edits all
# four gate scripts -- a branch-scoped predicate would opt out on every turn forever and the guard
# would arm only where it was never needed. What actually needs the opt-out is an UNCOMMITTED edit:
# a committed script matches its blob at HEAD and verifies fine. `git status --porcelain` alone is
# therefore the right question, and it makes the guard live on the branch that wrote it.
#
# FAIL SAFE TOWARD THE GUARD, AND THE INITIALISER IS WHERE THAT IS DECIDED. The first version of
# this block claimed exactly this and then reversed it for one of the two disjuncts: it initialised
# to 1, so `git rev-parse --git-dir` failing (no `.git` at all -- a container stage that drops it, a
# `git archive` extraction, `git` off PATH) skipped the `if` body entirely and left the value at
# "dirty", DISARMING the comparison silently on every turn, while printing the ordinary dirty-tree
# line. That is the silent-disarm shape V3 exists to remove, reintroduced by a default rather than
# by an edit anyone would notice -- and the comment above it asserted the opposite. (Review #77.)
#
# So the default is ARMED, and both failure disjuncts now reach it: no git, and a `git status` that
# errors (the pipeline takes `grep`'s status, which is 1 on empty output). A git-less host therefore
# gets the block's loud `FATAL`, whose message names the opt-out and is recoverable in one command
# -- which is what "fail safe toward the guard" has to mean if it means anything. Editing a gate
# script and re-running is still the normal loop: that turn is dirty and opts out, this one is not.
#
# AND ARMING IS THE RIGHT SEMANTICS HERE, not merely the cautious one: the comparison's ORACLE IS
# THE REPOSITORY. A checkout without one has nothing to compare against, so the selftest's own
# refusal is the honest answer rather than a nuisance -- it says the gate set cannot be verified,
# which is true, and names the opt-out, so a host that genuinely has no git is one command from a
# green. Disarming would have answered "cannot verify" with "verified".
#
# THIS IS THE FILE'S OWN CONVENTION, stated at the `crates/UNKNOWN` sentinel ~75 lines up: "Fail
# SAFE, like the no-git path below: scope cannot be computed, so do not guess -- run the suite."
# That sentence names THIS block, and it was FALSE for four rounds -- between the block landing
# (review #72) and this fix, the path below it did the opposite of what the path above claimed of
# it. An existing comment silently falsified by a new block, with nothing pointing at it: the
# half-applied-sweep class from the other end, where the stale site is the one you did not write.
#
# ONE CASE IS NOT A TAMPER AND MUST NOT BLOCK THE TURN: the selftest builds its oracle on a PINNED
# PATH (`/usr/bin:/bin:/usr/local/bin`) so it cannot be sent to a shim, and FATALs when `git` or
# `tr` is not there. On NixOS (`/run/current-system/sw/bin`) or a slim container that keeps busybox
# elsewhere, that fires with a CLEAN tree and no gate script touched -- and from THIS caller the
# FATAL is `step`'s `fail=1`, i.e. `exit 2`, i.e. EVERY TURN BLOCKED. The message names
# `REGISTER_CHECK_ALLOW_DIRTY=1`, which works for the two Makefile targets that pass it and NOT for
# this one, because the hook picks the branch itself: the only escape is exporting the variable into
# the session, which disarms the comparison PERMANENTLY AND SILENTLY for every later turn -- the end
# state the V3 header argues against ("an armed-only entrypoint gets DELETED rather than opted out
# of"). So the caller detects the CAPABILITY miss itself and opts out loudly, once, for that turn.
#
# THIS IS NOT THE `no git at all` CASE ABOVE, and conflating them is what hid it: there
# `rev-parse` fails, the tree is unknowable and arming is the honest answer. Here `rev-parse`
# SUCCEEDS on the ambient PATH, the dirty check works, the selftest is armed -- and then cannot
# build its oracle. Nor is it a weakening: the comparison is IMPOSSIBLE on such a host, so the
# choice is between saying so per turn and a permanent silent disarm. A tamperer cannot reach this
# branch either, because it is decided by what is present under a fixed absolute PATH, which no
# in-repo edit and no inherited environment can change -- that is the whole point of pinning it.
# (Review #91 of PR #679.)
_vpath="/usr/bin:/bin:/usr/local/bin"
_oracle_missing=""
PATH="$_vpath" command -v git >/dev/null 2>&1 || _oracle_missing="git"
PATH="$_vpath" command -v tr  >/dev/null 2>&1 || _oracle_missing="${_oracle_missing:+$_oracle_missing and }tr"

_gate_scripts_dirty=0
if git -C "$ROOT" rev-parse --git-dir >/dev/null 2>&1; then
  if git -C "$ROOT" status --porcelain --untracked-files=all -- \
       .claude/hooks/register-check.sh \
       .claude/hooks/register-check-selftest.sh \
       .claude/skills/decision-lookup/scripts/decision-lookup.sh \
       .claude/skills/decision-lookup/scripts/stub-tests.sh 2>/dev/null | grep -q .
  then _gate_scripts_dirty=1; else _gate_scripts_dirty=0; fi
fi
# --- END OF THE DIRTY PREDICATE --- (anchor: `the_stop_gate_predicate_discriminates_a_hidden_tamper`
# lifts everything above this line out of the shipped script and runs it, rather than
# re-implementing it here. An explicit marker rather than "the next `if`", because the dispatch
# below grew a branch and the test's end anchor silently swallowed half of it -- the lifted snippet
# became an unterminated `if`, bash printed nothing, and the assertion read as "a clean tree does
# not arm" when the predicate was fine. Move this marker with the predicate, never past a branch.)
if [ -n "$_oracle_missing" ]; then
  echo "-> register-check selftest ($_oracle_missing not under $_vpath -- the comparison's oracle CANNOT BE BUILT on this host, so it is opted out for this turn rather than blocking it; the gate set is NOT verified here and an overwrite is caught at push)"
  step env REGISTER_CHECK_ALLOW_DIRTY=1 bash "$ROOT/.claude/hooks/register-check-selftest.sh"
elif [ "$_gate_scripts_dirty" = "1" ]; then
  echo "-> register-check selftest (a gate script is dirty in the working tree -- comparison opted out; an overwrite in this state is caught at push, not here)"
  step env REGISTER_CHECK_ALLOW_DIRTY=1 bash "$ROOT/.claude/hooks/register-check-selftest.sh"
else
  step bash "$ROOT/.claude/hooks/register-check-selftest.sh"
fi

# WHEN THE HOOK ITSELF CHANGES: the full guard suite (~2s, hermetic git fixture). Diff-scoped for
# the same reason as the workspace suite above -- it proves the budget hook, so it runs when the
# budget hook moves, and costs a docs turn nothing.
# (`crates/UNKNOWN` is the sentinel the scope computation above uses when it cannot diff at all --
# fail SAFE there too and run the suite rather than assume the hook is untouched.)
if printf '%s\n' "$changed" | grep -Eq '^\.claude/hooks/loop-budget|^crates/UNKNOWN$'; then
  echo "-> loop-budget selftest (diff touches .claude/hooks/loop-budget*)"
  step bash "$ROOT/.claude/hooks/loop-budget.sh" selftest
fi

# Optional app-level gates — only if a root package.json defines them (no-op until apps/ exists).
if [ -f "$ROOT/package.json" ]; then
  if grep -q '"test"' "$ROOT/package.json"; then ( cd "$ROOT" && npm test --silent ) || fail=1; fi
  if grep -q '"lint"' "$ROOT/package.json"; then ( cd "$ROOT" && npm run --silent lint ) || fail=1; fi
fi

if [ "$fail" -ne 0 ]; then
  echo "stop-gate: acceptance gates FAILED — fix before completing (see output above)." >&2
  exit 2
fi
echo "stop-gate: all acceptance gates passed."
exit 0
