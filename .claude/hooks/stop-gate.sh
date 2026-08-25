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
# REGISTER_CHECK_ALLOW_DIRTY: the selftest compares itself and the hook against their committed
# blobs and refuses to report otherwise (the overwrite class the eighth review of PR #679 planted
# green). Editing a hook and re-running is the normal interactive loop, so THIS caller -- and only
# this one -- opts out, visibly and in the repo. CI invokes the script directly and verifies.
step env REGISTER_CHECK_ALLOW_DIRTY=1 bash "$ROOT/.claude/hooks/register-check-selftest.sh"

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
