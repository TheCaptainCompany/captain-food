#!/usr/bin/env bash
# Guard tests for .claude/hooks/register-check.sh, plus the wiring- and drift-checks that keep the
# register-check discipline alive (run from stop-gate.sh on every turn -- pure shell; ~4s and GROWS
# WITH THE CASE COUNT, not the ~200ms once true here -- see .github/workflows/ci.yml's
# `gate-scripts` job for the measured antecedent, #914).
#
# WHY THIS EXISTS. "A gate never seen to fire is an unverified claim" (#292, beck): each case below
# shows the hook red or green against the REAL script before any session trusts it. The hook is
# exactly the silent-when-broken shape ADR-20260810-231300 warns about -- a matcher typo or a
# removed settings entry disarms it with no signal -- so cases W/W1-W3 assert the wiring
# SEMANTICALLY (event + exact matcher + command, with planted disarming mutants), case D
# asserts every standing agent still carries its citation block, and the R cases assert the
# REG-2/REG-4 row gate (ADR-20260821-095957) actually loads and reads decision rows: the BLOCK
# case is the load-proof, because with the legacy lane a hook that parsed nothing would pass
# every ALLOW case indistinguishably (beck, 2026-08-21 briefing).
#
# Hermetic: payload cases run against FIXTURE rows in a throwaway dir via REGISTER_CHECK_DECISIONS
# (never the live corpus, whose statuses change) with the log at /dev/null; the L cases then prove
# the LIVE corpus wiring on two anchors that cannot legitimately change (REG-2 is decided forever
# -- a reversal opens a NEW row, never reopens the file).
set -uo pipefail

SELFTEST_START_TS="$(date +%s)"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HOOK="$ROOT/.claude/hooks/register-check.sh"
[ -f "$HOOK" ] || { echo "register-check selftest: cannot find $HOOK" >&2; exit 2; }

# ── This selftest testifies about the WHOLE GATE SET ─────────────────────────────────────────────────
# GATE-SELF-VERIFICATION-V3 -- pinned by `assert_gate_script_self_verifies` in
# tools/codegen-rs/src/tests.rs, which runs under `cargo test --workspace` in the **build-test**
# job: a DIFFERENT job with its own checkout, outside the blast radius of anything the `changes`
# job does at runtime. THAT IS THE ONLY THING IT SAYS -- review #12 read it as claiming build-test
# is itself governed, which it was not: its `env:`/`defaults.run` are now guarded at job and step
# scope by the same helper, and a `build-test` step that rewrites the pin's own source before
# `cargo test` remains a code-review residual, named in `tests.rs` beside the others. (Four sites on this branch said "the `codegen` job". That job is a pure
# AGGREGATOR -- one step, no checkout, no cargo -- so the sentence named the wrong job, a
# non-existent checkout and the wrong `if:`. Review #10. build-test carries
# `if: needs.changes.outputs.docs_only != 'true'`, and a `.claude/**` change can never be
# docs-only, so the pin always runs on any change able to disarm it.) Neither half is sufficient
# alone.
#
# WHY ALL FOUR FILES AND NOT JUST THIS SCRIPT'S OWN PAIR. V2 had each gate script verify itself
# and the script it guards. That cannot work, and the ninth review of PR #679 proved it in one
# line: a block inside a script goes away when the script is REPLACED. `find . -name
# 'register-check-selftest.sh' -exec cp exit0.sh {} +` left both gates green, which is the exact
# mutant V2's own header claimed to answer. So each script now verifies the ENTIRE set, and
# replacing either guard is caught by the other. `assert_gate_script_self_verifies` asserts both
# scripts carry all four paths, so the two lists cannot drift apart.
#
# WHICH COMMIT, precisely: on a `pull_request` event `actions/checkout` checks out
# `refs/pull/N/merge`, so `HEAD` is the MERGE commit, not the PR head. That is the right thing to
# compare against -- the merge commit's tree IS the tree on disk, so this proves "these scripts
# are the ones the workflow was triggered for". NO RUN IS CITED HERE any more: the citation used
# to name run 32810599803, whose head is four commits behind and PREDATES the V3 block the sentence
# describes -- a figure that was true when written and silently stopped being about this code
# (review #10, ADR-20260817-105845). The behaviour is pinned by
# `the_gate_self_verification_reds_on_a_tampered_script` instead, which is re-run on every commit.
#
# DEFAULT-ON, with an explicit opt-out. V1 ran only when GITHUB_ACTIONS=true, which fails OPEN:
# the discriminator was an ordinary environment variable, settable from the same untrusted surface
# the check defends against. Now it always runs and an editor opts out BY NAME. Both opt-out names
# are forbidden as CI `env:` keys at every scope.
#
# WHEN IT IS ARMED, said plainly because "default-on" reads wider than it is -- AND REWRITTEN,
# because the change that made it wrong shipped in this same branch and the sweep for it reached
# `tests.rs` and not these three prose sites (review #77). What this paragraph used to say: "every
# INTERACTIVE caller opts out ... CI is the only caller that runs this armed ... the protection is
# PRE-MERGE, not in-session ... a session-local overwrite is caught on push and not before." All
# four clauses were falsified by review #72.
#
# WHAT IS TRUE NOW. `make hooks-test` and `make stub-tests` opt out unconditionally -- they exist
# for the edit-and-re-run loop, and an armed-only entrypoint gets DELETED rather than opted out of.
# `stop-gate.sh` opts out ONLY when a gate script is dirty in the working tree, so on an ordinary
# turn it runs this comparison ARMED. WHAT THAT BUYS IS NARROWER THAN REVIEW #72 CLAIMED AND THE
# CLAIM IS RETRACTED HERE: an ordinary `cp exit0.sh <gate script>` leaves the tree DIRTY, so it opts
# out and is still caught at push, not on the next turn. The two predicates are complements -- dirty
# is exactly when the hash comparison would mismatch -- and no working-tree test can separate
# editing from overwriting. What the armed path DOES catch is the tamper that hides from
# `git status` (`--assume-unchanged`, `--skip-worktree`): clean status, different bytes. That is the
# stealthier class, so the coverage is inverted from the naive reading -- the clumsy overwrite is
# caught at push, the careful one on the next turn. (Reviews #72 and #79.)
#
# CI is still the caller that CANNOT be talked out of it -- `env_ok` forbids both opt-out names as
# `env:` keys at every scope, which no local shell can. Default-on remains the right shape: the safe
# state needs no cooperation, and an opt-out is a visible argument in a Makefile. The residual is
# now narrow rather than total: a session that edits a gate script legitimately, and overwrites
# another one in the same turn, is still only caught on push. (Reviews #51, #72 and #77 of PR #679.)
#
# WHAT THIS DOES NOT DO: it DETECTS the named overwrite routes. It is not a defence against
# arbitrary code running before it. Closed so far, each because a review demonstrated it: a `git`
# shell function via `env: BASH_ENV` (hence `unset -f`), a PATH shim (hence the fixed `_vpath`),
# and `GIT_DIR` pointing the oracle at a decoy repo whose HEAD holds the tampered bytes (hence
# `unset "${!GIT_@}"`, plus a `GIT_*` prefix ban in `env_ok`). "Arbitrary preamble" is not a
# bounded set, and enumerating in one place only would repeat the mistake.
if [ "${REGISTER_CHECK_ALLOW_DIRTY:-}" = "1" ]; then
  echo "self-verification: OPTED OUT (REGISTER_CHECK_ALLOW_DIRTY=1) -- gate scripts NOT compared to HEAD."
else
  # Strip anything an inherited environment could have put between us and the verifier: shell
  # functions shadowing the tools, a PATH pointing at a shim, and every GIT_* variable -- git
  # obeys GIT_DIR/GIT_WORK_TREE/GIT_OBJECT_DIRECTORY and friends, so the ORACLE itself is
  # redirectable even when the binary is not. The prefix form cannot be out-enumerated.
  unset -f git tr command 2>/dev/null || true
  unset "${!GIT_@}" 2>/dev/null || true
  _vpath="/usr/bin:/bin:/usr/local/bin"
  _git="$(PATH="$_vpath" command -v git || true)"
  # `tr` IS PART OF THE ORACLE, so it is resolved on the same pinned PATH and `unset -f` above
  # covers it: it performs the one translation this gate accepts (CR strip), and a `tr` function or
  # shim could make a tampered file hash to the committed id. `cmp` used to be here and is gone --
  # the comparison is object-id against object-id now, and requiring a binary nothing calls can
  # only produce a false refusal.
  _tr="$(PATH="$_vpath" command -v tr || true)"
  if [ -z "$_git" ] || [ -z "$_tr" ]; then
    echo "FATAL: git or tr not found on $_vpath -- refusing to report on scripts that cannot be verified." >&2
    echo "  The PATH is pinned on purpose so this cannot be sent to a shim. This is a HOST" >&2
    echo "  CAPABILITY miss, not a tamper: on NixOS (/run/current-system/sw/bin) or a slim" >&2
    echo "  container the tools are simply elsewhere." >&2
    echo "  For a DIRECT run (make hooks-test, make stub-tests): re-run with" >&2
    echo "  REGISTER_CHECK_ALLOW_DIRTY=1 rather than deleting this block." >&2
    echo "  The Stop hook does NOT need that: stop-gate.sh resolves git and tr on this same" >&2
    echo "  pinned PATH and opts out for the turn on its own, so a host like this is not" >&2
    echo "  blocked and nobody has to export the variable into the session -- which would" >&2
    echo "  disarm the comparison permanently and silently, for every later turn." >&2
    exit 1
  fi
  # WHICH COMMIT THE ORACLE READS is itself part of the defence. Comparing against `HEAD` lets a
  # CI step move the oracle instead of hiding the file: `printf 'exit 0' > register-check.sh;
  # git add -A && git commit` makes HEAD agree with the tampered disk, and both guards print OK
  # over a gate that no longer does anything (review #10 reproduced it). In CI the runner sets
  # GITHUB_SHA to the commit the workflow was triggered for, a later step cannot change it for an
  # earlier one, and `env_ok` forbids it as a workflow/job/step `env:` key -- so pin to it when it
  # is present and fall back to HEAD only for a local run, where the threat model is different.
  _ref="${GITHUB_SHA:-HEAD}"
  echo "self-verification: comparing all 4 gate scripts against their committed blobs at ${_ref}."
  # THE GATE SET. Both gate scripts carry this identical list; the codegen pin asserts that.
  # RESOLVE THE COMMIT BEFORE BLAMING A FILE. `cat-file -e "$ref:$path"` is false for two very
  # different reasons -- the path is untracked there, or the COMMIT is absent from this checkout --
  # and reporting both as "not tracked" sends the operator hunting a missing file that is present.
  #
  # THE `pull_request` MERGE-REF RACE CANNOT REACH THIS BRANCH, AND THE COMMENT THAT USED TO LIVE
  # HERE SAID IT COULD. It read: "It is not hypothetical. On a `pull_request` run GITHUB_SHA is the
  # merge commit GitHub computed when the run was QUEUED, while actions/checkout resolves
  # refs/pull/N/merge when it FETCHES. If the base branch moves in between, the merge ref is
  # recomputed and the workspace no longer holds the object GITHUB_SHA names." The first half is
  # true; the conclusion is not, because `actions/checkout` verifies exactly that and REFUSES:
  # after fetching it calls `testRef(git, settings.ref, settings.commit)`, retries once with a
  # SHA-targeted refspec on a full fetch, and then throws `The ref '<ref>' does not point to the
  # expected commit ...`. So either checkout fails -- and this step never runs -- or GITHUB_SHA is
  # present, verified by checkout itself, and `rev-parse` below cannot fail for that reason. Read
  # off `actions/checkout`'s `src/git-source-provider.ts`, not inferred (review #41 of PR #679
  # raised the availability consequence of the race; the mechanism does not hold).
  #
  # The refusal stays, as defence in depth for the cases checkout does not cover: a LOCAL run with
  # a stale GITHUB_SHA exported in the shell, and any future job whose checkout is reconfigured.
  # The message still distinguishes itself from tampering, because a reader who meets it has no
  # reason to know which of the two they are looking at.
  #
  # THE AVAILABILITY CONCERN THE REVIEW RAISED IS REAL AND UNCHANGED BY THIS: when checkout does
  # fail, `changes` fails, every sibling job is skipped and `codegen` -- the required check -- reds,
  # so nothing merges until a human re-runs. That is `actions/checkout`'s behaviour in EVERY job in
  # this workflow, not something the GITHUB_SHA pin introduced, and it is `GATE-STEP-LOCUS` option
  # (a) that bounds it. The fallback the review proposed -- read the oracle from
  # `refs/remotes/pull/N/merge` when GITHUB_SHA is unresolvable -- is NOT taken: a local ref name is
  # forgeable by any earlier step with `git update-ref`, which is precisely the property GITHUB_SHA
  # was chosen for (review #10 moved HEAD by committing), so it would trade the oracle for an
  # availability problem this path does not actually have.
  # A `git fetch --no-tags --depth=1 origin "$_ref"` RECOVERY WAS ADDED HERE AND REMOVED AGAIN,
  # and the reason is worth more than the code was. It was justified by the sentence "upload-pack
  # serves fetch-by-SHA, so one fetch usually turns the refusal back into a verification" -- an
  # antecedent-free claim of exactly the shape ADR-20260817-105845 governs, in a branch whose
  # thesis is that no completeness claim ships before it is checked. The case it targeted is an
  # ORPHANED merge commit: once the base moves, GitHub recomputes refs/pull/N/merge and the old
  # commit is reachable from no ref, which is precisely the unadvertised-object case a server
  # REFUSES. So the recovery most likely no-ops in the one situation it existed for. Nothing
  # planted it either -- the only test on this path uses a fixture repo with no `origin` at all,
  # so the fetch failed instantly on "no such remote" and proved only the refusal that already
  # existed; deleting the whole block redded nothing. And `--depth=1` against a checkout
  # deliberately fetched with `fetch-depth: 0` writes .git/shallow and shallows the workspace for
  # every later step in the job. Removed rather than kept on a hope. (Reviews #11 and #15 of
  # PR #679: #11 asked for it, #15 showed the argument for it did not hold.)
  if ! "$_git" -C "$ROOT" rev-parse -q --verify "${_ref}^{commit}" >/dev/null 2>&1; then
    echo "FATAL: commit ${_ref} is not present in this checkout -- cannot verify the gate set." >&2
    echo "  This is NOT a tamper signal." >&2
    # THE DIAGNOSIS DEPENDS ON WHERE `_ref` CAME FROM, and one message covered both for a round.
    # The merge-ref story is only true when GITHUB_SHA supplied it; on a LOCAL run `_ref` is
    # literally `HEAD`, there is no job to re-run, and the reader was sent to look for a race that
    # cannot happen there. The reachable local cause is a git that resolves but cannot read this
    # repository -- macOS WITHOUT the Command Line Tools installed is the common one: /usr/bin/git
    # is a stub that exists, so `command -v` finds it on the pinned PATH and the capability check
    # above passes, and it then fails here. An empty repository with no commit yet does the same.
    # "A gate reporting the wrong thing is worse than a gate reporting nothing, because it spends
    # the reader's time" -- this branch's own words, applied to itself. (Review #91 of PR #679.)
    if [ -n "${GITHUB_SHA:-}" ]; then
      echo "  On a pull_request run GITHUB_SHA is the merge commit as of queue time, and the merge" >&2
      echo "  ref can be recomputed before checkout fetches it. RE-RUN the job: that recomputes" >&2
      echo "  both and self-heals." >&2
    else
      echo "  This is a LOCAL run (no GITHUB_SHA), so there is no job to re-run and no merge-ref" >&2
      echo "  race to wait out: git resolved but cannot read this repository at HEAD. On macOS" >&2
      echo "  that is usually the Command Line Tools not being installed -- /usr/bin/git is a stub" >&2
      echo "  that exists, so the PATH check above passes and this is where it surfaces. Run" >&2
      echo "  'git status' yourself: if it prompts to install the CLT, that is the cause. A" >&2
      echo "  repository with no commits yet fails here identically." >&2
    fi
    exit 1
  fi
  for rel in \
    .claude/hooks/register-check.sh \
    .claude/hooks/register-check-selftest.sh \
    .claude/skills/decision-lookup/scripts/decision-lookup.sh \
    .claude/skills/decision-lookup/scripts/stub-tests.sh
  do
    _want="$("$_git" -C "$ROOT" rev-parse -q --verify "${_ref}:$rel" 2>/dev/null || true)"
    if [ -z "$_want" ]; then
      echo "FATAL: $rel is not tracked at ${_ref} -- refusing to report on a gate set CI cannot verify." >&2
      exit 1
    fi
    # COMPARE OBJECT IDS, NOT BYTES. `cat-file blob | cmp` puts the RAW blob against a SMUDGED
    # worktree file, so git's own EOL translation reads as tampering: the committed blobs are LF,
    # this repo is authored on Windows (ci.yml's drift step says so, and stop-gate.sh carries a
    # Cygwin branch), Git for Windows defaults to core.autocrlf=true and there is no
    # .gitattributes -- so a COMPLETELY CLEAN checkout failed all four comparisons and printed the
    # tamper message, with nothing anywhere mentioning line endings. The remedy a reader reaches
    # for is deleting this block, which is exactly what the header asks them not to do. CI is
    # Linux-only, so the plant-red fixture builds and reads on one platform and is structurally
    # blind to the class. (Review #13 of PR #679.)
    #
    # `--no-filters`, AND THE CR STRIP DONE HERE RATHER THAN BY CONFIG. The first fix for the
    # above was a bare `hash-object`, which runs git's clean filters -- and that is the knob that
    # makes the comparison LIE. Git locates its global config through $XDG_CONFIG_HOME/git/config
    # and $HOME/.gitconfig, neither of which is a GIT_* name, so `unset "${!GIT_@}"` does not
    # touch them; global config can set core.attributesFile, an attributes file can bind a
    # filter.<x>.clean driver, and a driver that re-emits `cat-file blob <ref>:<path>` reproduces
    # the committed id for EVERY path. All four comparisons then match over tampered scripts and
    # both guards print OK -- the GIT_DIR decoy of review #9, one config-lookup mechanism over
    # (review #15). `--no-filters` disables clean filters AND eol conversion together, so no git
    # configuration reachable from the environment, .git/config or an attributes file can affect
    # this hash. The single translation this gate accepts is then applied EXPLICITLY below: strip
    # CR and re-hash. `GIT_CONFIG_GLOBAL=/dev/null` was the tempting alternative and is wrong --
    # it also drops core.autocrlf, reinstating the Windows false red this block just removed.
    _have="$("$_git" -C "$ROOT" hash-object --no-filters -- "$ROOT/$rel" 2>/dev/null || true)"
    if [ "$_have" != "$_want" ]; then
      # THE ONE ACCEPTED TRANSLATION: a CRLF worktree over an LF blob. Narrower than a filter --
      # it can only ever turn a CRLF checkout into its committed form, and the result still has to
      # equal the committed id exactly.
      _have="$("$_tr" -d '\r' < "$ROOT/$rel" | "$_git" -C "$ROOT" hash-object --no-filters --stdin 2>/dev/null || true)"
    fi
    if [ "$_have" != "$_want" ]; then
      echo "FATAL: $rel differs from the committed blob at ${_ref}." >&2
      echo "  Something modified a gate script between checkout and this run -- the disarm shape" >&2
      echo "  this check exists to DETECT. A green here would be a lie." >&2
      echo "  Editing it locally? Re-run with REGISTER_CHECK_ALLOW_DIRTY=1." >&2
      exit 1
    fi
  done
  echo "self-verification: OK -- all 4 gate scripts are byte-identical to ${_ref}."
fi

FIX="$(mktemp -d "${TMPDIR:-/tmp}/register-check-selftest.XXXXXX")"
trap 'rm -rf "$FIX"' EXIT
cat > "$FIX/OPEN-ROW.yaml" <<'EOF'
key: "OPEN-ROW"
status: "open"
owner: "founder"
EOF
cat > "$FIX/OPEN-TWO.yaml" <<'EOF'
key: "OPEN-TWO"
status: "open"
owner: "founder"
EOF
cat > "$FIX/GONE-ROW.yaml" <<'EOF'
key: "GONE-ROW"
status: "decided"
owner: "founder"
decided: "2026-08-19"
decided_by: "ADR-20260819-103112"
EOF
cat > "$FIX/DEFER-ROW.yaml" <<'EOF'
key: "DEFER-ROW"
status: "deferred"
owner: "team"
until: "after one order flows end to end (#556)"
EOF
cat > "$FIX/LAW-ROW.yaml" <<'EOF'
key: "LAW-ROW"
status: "open"
owner: "counsel"
EOF
cat > "$FIX/_legacy.yaml" <<'EOF'
legacy:
  - OLD-ROW
EOF

# ── Lane D fixtures (the dispatch surface, ADR-20260831-141500) ─────────────────────────────────
# A HERMETIC agent roster and docs tree, for the same reason the row fixtures exist: the live
# roster and the live docs/ tree both change, and a case that depends on them rots. The LIVE
# wiring is proven separately by the LD cases below, on properties that cannot legitimately move.
mkdir -p "$FIX/agents" "$FIX/docs/adr"
cat > "$FIX/agents/writer.md" <<'EOF'
---
name: writer
tools: Read, Grep, Glob, Bash, Write, Edit
---
EOF
cat > "$FIX/agents/lens.md" <<'EOF'
---
name: lens
tools: Read, Grep, Glob, Bash
---
EOF
cat > "$FIX/agents/notools.md" <<'EOF'
---
name: notools
description: declares no `tools:` line, so it inherits the full set -- including Write
---
EOF
: > "$FIX/docs/adr/ADR-20260821-095957-fixture-record.md"
# A SEPARATE, non-empty fixture record for Rule 1 (the red-first card step, ADR-20260906-024838 /
# #910): the fixture above stays EMPTY on purpose (0 test-naming hits), because it is shared by
# every pre-existing Lane D case and none of them carry a `Red-first:` section -- widening it would
# flip D2/D12/D13/D14 from ALLOW to BLOCK, exactly the "altered existing verdict" the card's STOP
# condition forbids. This one is cited ONLY by the new RF cases below. TWO lines hit the token set
# -- line 1 ("...a record that names a test...") and line 4 ("...belt for Rule 1") -- so
# `rf_hit_count` over this file is 2, not 1 (#914 item 2: the earlier comment here claimed line 4
# was the SOLE hit line, which is false and was never checked by any case). Every RF case that
# needs a resolvable hit line points at line 4 specifically, by number.
cat > "$FIX/docs/adr/ADR-20260906-050000-fixture-redfirst.md" <<'EOF'
# Fixture: a record that names a test (Rule 1 / red-first cases)

Nothing here names anything on this line.
This line pins `test_the_gate_stays_red_first` -- treat it as the belt for Rule 1.
Nothing here either.
EOF
# THE OTHER TWO FILENAME ERAS. docs/adr/ holds 164 `ADR-<stamp>-*`, 47 legacy `NNNN-*` and 54
# PREFIXLESS `<stamp>-*` files; the first resolver globbed only the first shape and so refused 101
# of 265 real ADRs, offering a correct trail no exit but a fabricated id or a false negative
# (review round 1, F1). One fixture per era, so a regression to the single-glob form reds here.
: > "$FIX/docs/adr/0032-fixture-legacy-era.md"
: > "$FIX/docs/adr/20260720-233000-fixture-prefixless-era.md"
# A NON-EMPTY 0-hit fixture (#926 item 4): the fully EMPTY fixture above only ever reaches
# malformedness via a line PAST EOF (RF4b shares RF7's own branch), so no case exercised "the line
# EXISTS and carries no token" at 0 hits specifically -- a real gap, since that is a DIFFERENT
# failure mode from "no such line". Three prose lines, none of them carrying a Rule 1 token; RF12
# pins its own entry to line 2 by number.
cat > "$FIX/docs/adr/ADR-20260906-060000-fixture-zerohit.md" <<'EOF'
# Fixture: a zero-hit record for the sharper 0-hit cases (RF12/RF13/RF14, #926 item 4)
Nothing on this second line carries a marker either -- RF12 pins its own entry here.
Nor does this third and final line of the fixture.
EOF
# Agent fixtures for the shapes that used to fail OPEN (review round 1, F2). Each one is a
# `tools:` declaration the old `awk /^tools:/{print}` reduced to the literal `tools:` -- non-empty,
# so the fail-closed branch never ran and a write-capable agent was declared advisory.
cat > "$FIX/agents/listform.md" <<'EOF'
---
name: listform
tools:
  - Read
  - Write
---
EOF
cat > "$FIX/agents/continuation.md" <<'EOF'
---
name: continuation
tools:
  Read, Bash, Write
---
EOF
cat > "$FIX/agents/trailcomma.md" <<'EOF'
---
name: trailcomma
tools: Read, Grep,
  Bash, Write
---
EOF
cat > "$FIX/agents/wildcard.md" <<'EOF'
---
name: wildcard
tools: "*"
---
EOF
cat > "$FIX/agents/emptykey.md" <<'EOF'
---
name: emptykey
tools:
---
EOF
cat > "$FIX/agents/editonly.md" <<'EOF'
---
name: editonly
tools: Read, Grep, Glob, Bash, Edit
---
EOF
cat > "$FIX/agents/todowrite.md" <<'EOF'
---
name: todowrite
tools: Read, Grep, Glob, Bash, TodoWrite
---
EOF
# Round 2, F-A: FOUR MORE shapes that failed open, because value continuation is not decidable
# from the first physical line. Each of these grants Write; each read `agent-advisory` until the
# parse started reading the WHOLE value. `wrapped-readonly` is the false-positive floor beside
# them -- it must stay advisory, and the round-2 trailing-comma heuristic had gated it.
cat > "$FIX/agents/flowbreak-after.md" <<'EOF'
---
name: flowbreak-after
tools: [Read, Grep, Glob, Bash
  , Write]
---
EOF
cat > "$FIX/agents/flowbreak-before.md" <<'EOF'
---
name: flowbreak-before
tools: [Read, Grep, Glob
  , Bash, Write]
---
EOF
cat > "$FIX/agents/plainbreak.md" <<'EOF'
---
name: plainbreak
tools: Read, Grep, Glob, Bash
  , Write, Edit
---
EOF
cat > "$FIX/agents/folded.md" <<'EOF'
---
name: folded
tools: >
  Read, Write
---
EOF
cat > "$FIX/agents/wrapped-readonly.md" <<'EOF'
---
name: wrapped-readonly
tools: Read, Grep,
  Glob, Bash
---
EOF

fail=0
expect() { # expect <case> <want-exit> <decisions-dir> <payload> [want-reason]
  # want-reason (optional) is compared EXACTLY against the hook log's reason field: a case that
  # goes red for the WRONG rule is a claim without evidence — E5 sat green for months carried by
  # key-unknown while the envelope-multiple lane it names was never exercised (PR #669 review, F2).
  local case="$1" want="$2" dir="$3" payload="$4" want_reason="${5:-}" got reason log="$FIX/case.log"
  : > "$log"
  printf '%s' "$payload" | REGISTER_CHECK_LOG="$log" REGISTER_CHECK_DECISIONS="$dir" bash "$HOOK" >/dev/null 2>&1
  got=$?
  if [ "$got" -ne "$want" ]; then
    echo "register-check selftest: case $case FAILED (want exit $want, got $got)" >&2
    fail=1
  fi
  if [ -n "$want_reason" ]; then
    reason="$(tail -1 "$log" 2>/dev/null | cut -f3)"
    if [ "$reason" != "$want_reason" ]; then
      echo "register-check selftest: case $case FAILED (want reason '$want_reason', got '${reason:-none}')" >&2
      fail=1
    fi
  fi
}

TRAIL='Register check: no controlling record -- terms: fixture; nearest: none'

# ── The trail check (ADR-20260821-010543) ───────────────────────────────────────────────────────
# 1 BLOCK: no trail at all -- the incident shape (ADR-20260818-210000 defect 2).
expect 1-no-trail 2 "$FIX" '{"questions":[{"question":"Which funding model applies to tips?"}]}' trail-missing
# 2 BLOCK: bare marker token without a record id or the explicit negative -- the cargo-cult trail.
expect 2-hollow-trail 2 "$FIX" '{"questions":[{"question":"Which funding model? Register check: done"}]}' trail-hollow
# 3 BLOCK (FLIPPED 2026-08-28, ADR-20260828-120500 / #709): a trail that self-cites a controlling
#    record id AND self-declares it DECIDED, in the canonical `(<date>, <status>)` shape, is by its
#    own words an answered question -- asking anyway is the round-5 call-sheet gap the ADR names.
#    Before this change the hook only checked trail SHAPE and this case sat green as an ALLOW; #709
#    is the tracking issue and this is its red-before / green-after case.
expect 3-record-id 2 "$FIX" '{"questions":[{"question":"Confirm scope. Register check: ADR-20260819-103112 (2026-08-19, decided) -- covers refunds, silent on thresholds"}]}' trail-answered
# 3b ALLOW: the escape hatch -- a `premise-changed:` line names what changed, logged distinctly
#    rather than folded into a plain ALLOW (a hollow marker is then a decomposable defect too).
expect 3b-premise-changed 0 "$FIX" '{"questions":[{"question":"Confirm scope. Register check: ADR-20260819-103112 (2026-08-19, decided) -- covers refunds, silent on thresholds. premise-changed: HubRise dropped split payouts, invalidating the funding assumption."}]}' trail-premise-changed
# 3c ALLOW: the same two-part `(<date>, <status>)` shape citing OPEN is exactly what the trail is
#    for -- only the register's CLOSED statuses (decided/superseded/deferred/withdrawn) refuse.
expect 3c-trail-open 0 "$FIX" '{"questions":[{"question":"Any nuance left? Register check: ADR-20260820-999999 (2026-08-20, open) -- covers X, silent on Y"}]}'
# 3d BLOCK: the check spans the WHOLE closed set, not just `decided` -- a `superseded` self-citation
#    is equally an answer (the successor record is the one to ask about).
expect 3d-trail-superseded 2 "$FIX" '{"questions":[{"question":"Still valid? Register check: ADR-20260810-999999 (2026-08-10, superseded) -- covers X"}]}' trail-answered
# 4 ALLOW: legacy ADR id form -- a single-token parenthetical (no comma) carries no status clause
#    to parse, so it is untouched by the new check.
expect 4-legacy-id 0 "$FIX" '{"questions":[{"question":"... Register check: ADR-0032 (completeness) covers this"}]}'
# 5 ALLOW: explicit negative with terms -- a genuinely new question is the system working.
expect 5-no-record 0 "$FIX" '{"questions":[{"question":"New option space. Register check: no controlling record -- terms: payout, settlement, virement; nearest: none"}]}'
# 6 ALLOW: DECISIONS register section citation -- single-token parenthetical, same as case 4.
expect 6-register-row 0 "$FIX" '{"questions":[{"question":"... Register check: DECISIONS.md section 48 (open)"}]}'
# 7 BLOCK: empty stdin -- fail closed, never fail open (ADR-20260810-231300).
expect 7-empty-input 2 "$FIX" '' empty-input

# ── The row gate (REG-2/REG-4, ADR-20260821-095957) ─────────────────────────────────────────────
# R1 BLOCK: a well-trailed question referencing a DECIDED row -- the founder's own rule, and the
#    load-proof that the fixture rows were actually parsed.
expect R1-decided-key 2 "$FIX" "{\"questions\":[{\"question\":\"Should we revisit GONE-ROW? $TRAIL\"}]}" key-decided
# R2 ALLOW: a question referencing an OPEN row is exactly what the queue is for.
expect R2-open-key 0 "$FIX" "{\"questions\":[{\"question\":\"OPEN-ROW options A/B? $TRAIL\"}]}"
# R3 ALLOW: a legacy-allowlisted key passes (no backfill; migrate at next touch), logged.
expect R3-legacy-key 0 "$FIX" "{\"questions\":[{\"question\":\"About OLD-ROW: which change carries it? $TRAIL\"}]}" key-legacy
# R4 BLOCK: a DEFERRED row is un-askable until its wake condition; the refusal cites `until`.
expect R4-deferred-key 2 "$FIX" "{\"questions\":[{\"question\":\"Can we do DEFER-ROW now? $TRAIL\"}]}" key-deferred
# R5 ALLOW (FLIPPED 2026-08-21, ADR-20260821-103403): a PASSIVE mention of an open counsel-owned
#    row is context, not the ask -- the counsel routing now binds the ENVELOPE lane (E7/E8).
expect R5-counsel-passive 0 "$FIX" "{\"questions\":[{\"question\":\"Context: LAW-ROW is still open. $TRAIL\"}]}"
# R7 ALLOW: key-shaped tokens declared nowhere are not register references in PROSE (the envelope
#    lane rejects them as E4; free-text enforcement stays un-mechanical, recorded in the ADR).
expect R7-unknown-key 0 "$FIX" "{\"questions\":[{\"question\":\"NOT-A-ROW and GONE-ROWBOAT are not references. $TRAIL\"}]}"
# R8 BLOCK: a broken REGISTER_CHECK_DECISIONS override fails closed, never silently skips.
expect R8-override-broken 2 "$FIX/absent" "{\"questions\":[{\"question\":\"Anything. $TRAIL\"}]}" override-broken

# ── The envelope lane (decision-ask-unregistered, ADR-20260821-103403) ──────────────────────────
# E1 ALLOW: a decision question = one `Decision row:` naming a declared OPEN row; the envelope IS
#    the register check, so no trail line is required. (Old hook: BLOCK trail-missing -- flipped.)
expect E1-envelope-open 0 "$FIX" '{"questions":[{"question":"Decision row: OPEN-ROW -- option A or B?"}]}' ok
# E2 BLOCK: the envelope on a DECIDED row -- the reversal path is a NEW row with reconsiders.
expect E2-envelope-decided 2 "$FIX" '{"questions":[{"question":"Decision row: GONE-ROW -- revisit?"}]}' key-decided
# E3 BLOCK: the envelope on a LEGACY key -- legacy is not a bypass; migrate in the same change,
#    then the SAME question passes live. (Old hook with a trail: ALLOW key-legacy -- flipped.)
expect E3-envelope-legacy 2 "$FIX" "{\"questions\":[{\"question\":\"Decision row: OLD-ROW -- decide it? $TRAIL\"}]}" key-legacy-ask
# E4 BLOCK: the envelope on an UNKNOWN key -- typo or undeclared; the refusal lists open rows and
#    the create-row path. (Old hook with a trail: ALLOW as a non-reference -- flipped.)
expect E4-envelope-unknown 2 "$FIX" "{\"questions\":[{\"question\":\"Decision row: NO-SUCH-ROW -- decide it? $TRAIL\"}]}" key-unknown
# E5 BLOCK: two envelope lines -- a decision question references EXACTLY ONE declared row. Both
#    keys are OPEN so no other lane can carry the red: the reason MUST be envelope-multiple (the
#    old fixture used OPEN+DECIDED on one line and sat green for the wrong rule -- PR #669, F2).
expect E5-envelope-multiple 2 "$FIX" '{"questions":[{"question":"Decision row: OPEN-ROW\nDecision row: OPEN-TWO -- pick both?"}]}' envelope-multiple
# E5b BLOCK: two tokens on ONE line -- same rule, deliberately, whatever the line layout; the
#    token count (not the extracted-line count) is what the hook gates on.
expect E5b-envelope-multiple-same-line 2 "$FIX" '{"questions":[{"question":"Decision row: OPEN-ROW and also Decision row: OPEN-TWO"}]}' envelope-multiple
# E6 BLOCK: a garbled envelope (no valid key token) fails loudly, echoing the rejected line.
expect E6-envelope-garbled 2 "$FIX" '{"questions":[{"question":"Decision row: bad-key please?"}]}' envelope-garbled
# E7 BLOCK: the envelope on an open counsel-owned row without the external-action framing.
expect E7-envelope-counsel 2 "$FIX" '{"questions":[{"question":"Decision row: LAW-ROW -- what is the answer?"}]}' key-counsel-owned
# E8 ALLOW: the documented escape -- the question asks for the external action itself.
expect E8-counsel-action 0 "$FIX" '{"questions":[{"question":"Decision row: LAW-ROW -- external action: engage counsel this week?"}]}'

# ── Lane D: the dispatch card (Agent surface, ADR-20260831-141500) ──────────────────────────────
expect_d() { # expect_d <case> <want-exit> <payload> [want-reason] [cwd] [want-stderr-fragment]
  # want-stderr-fragment (#926 item 4) asserts the hook's ACTUAL STDERR text (`block()`'s
  # `rf_msg`/message body, fed back to the caller) contains a literal fragment -- a DIFFERENT
  # thing from `want-reason` above, which only checks the log's short reason CODE. No existing
  # case's own redirection changes: stdout still goes nowhere and every prior case still passes
  # with only 5 args: stderr is now captured to a per-case file instead of being merged and
  # discarded, and only read back when a 6th arg is actually given.
  local case="$1" want="$2" payload="$3" want_reason="${4:-}" cwd="${5:-}" want_stderr="${6:-}" \
        got reason log="$FIX/case.log" errlog="$FIX/case.err"
  : > "$log"; : > "$errlog"
  if [ -n "$cwd" ]; then
    ( cd "$cwd" && printf '%s' "$payload" | REGISTER_CHECK_LOG="$log" REGISTER_CHECK_DECISIONS="$FIX" \
      REGISTER_CHECK_AGENTS="$FIX/agents" REGISTER_CHECK_DOCS="$FIX/docs" bash "$HOOK" >/dev/null 2>"$errlog" )
  else
    printf '%s' "$payload" | REGISTER_CHECK_LOG="$log" REGISTER_CHECK_DECISIONS="$FIX" \
      REGISTER_CHECK_AGENTS="$FIX/agents" REGISTER_CHECK_DOCS="$FIX/docs" bash "$HOOK" >/dev/null 2>"$errlog"
  fi
  got=$?
  if [ "$got" -ne "$want" ]; then
    echo "register-check selftest: case $case FAILED (want exit $want, got $got)" >&2
    fail=1
  fi
  if [ -n "$want_reason" ]; then
    reason="$(tail -1 "$log" 2>/dev/null | cut -f3)"
    if [ "$reason" != "$want_reason" ]; then
      echo "register-check selftest: case $case FAILED (want reason '$want_reason', got '${reason:-none}')" >&2
      fail=1
    fi
  fi
  if [ -n "$want_stderr" ]; then
    if ! grep -qF "$want_stderr" "$errlog" 2>/dev/null; then
      echo "register-check selftest: case $case FAILED (want stderr containing '$want_stderr', got: $(cat "$errlog" 2>/dev/null))" >&2
      fail=1
    fi
  fi
}
DTRAIL='Register check: ADR-20260821-095957 (2026-08-21, open) -- covers the ask gate, silent on the coordinator'

# D1 BLOCK: THE INCIDENT SHAPE -- a dispatch card to a write-capable agent with no trail at all.
expect_d D1-card-no-trail 2 '{"tool_name":"Agent","tool_input":{"subagent_type":"writer","prompt":"DISPATCH -- build the thing. Base main = bddba6bc."}}' dispatch-trail-missing
# D2 ALLOW: the same card carrying a trail whose record RESOLVES -- the green half of the pair.
expect_d D2-card-valid-trail 0 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL\"}}" dispatch-trail-ok
# D3 ALLOW: THE DISCRIMINATOR. A read-only agent is a lens consult / reviewer pass, not a card --
#    it commits nothing, so it is never gated. This is the false-positive floor: if this reds, the
#    gate fires on every mob briefing and gets worked around.
expect_d D3-lens-advisory 0 '{"tool_name":"Agent","tool_input":{"subagent_type":"lens","prompt":"What do you see in this event shape?"}}' agent-advisory
# D4 BLOCK: THE ESCAPE HATCH. A literal `Register check: none` must not satisfy the gate, or the
#    whole thing is theatre -- it names no record and is not the explicit negative.
expect_d D4-literal-none 2 '{"tool_name":"Agent","tool_input":{"subagent_type":"writer","prompt":"DISPATCH. Register check: none"}}' dispatch-trail-hollow
# D5 BLOCK: the negative without `terms:` -- "no controlling record" with nothing searched is the
#    same free pass under a longer name.
expect_d D5-termless-negative 2 '{"tool_name":"Agent","tool_input":{"subagent_type":"writer","prompt":"DISPATCH. Register check: no controlling record"}}' dispatch-trail-termless
# D6 ALLOW: the negative WITH its terms is a PASSING trail -- a genuinely new question is the
#    system working, and the card must not be dropped for lack of a record.
expect_d D6-negative-with-terms 0 '{"tool_name":"Agent","tool_input":{"subagent_type":"writer","prompt":"DISPATCH. Register check: no controlling record -- terms: coordinator register check, PreToolUse Agent; nearest: none"}}' dispatch-trail-ok
# D7 BLOCK: an INVENTED id. The shape is perfect and the record does not exist -- the check that
#    makes a fabricated citation cost something. (The fixture docs tree holds exactly one ADR.)
expect_d D7-unresolvable-id 2 '{"tool_name":"Agent","tool_input":{"subagent_type":"writer","prompt":"DISPATCH. Register check: ADR-20260101-000000 (2026-01-01, open) -- covers everything"}}' dispatch-trail-unresolved
# D8 BLOCK: an agent with NO file -- `general-purpose` is the live case (environment.md documents
#    pasting a charter into it), and it holds the full tool set. Fail closed.
expect_d D8-undeclared-agent 2 '{"tool_name":"Agent","tool_input":{"subagent_type":"general-purpose","prompt":"You are beck. Charter pasted."}}' dispatch-trail-missing
# D9 BLOCK: no `subagent_type` at all -- fail closed, never fail open (ADR-20260810-231300).
expect_d D9-no-subagent-type 2 '{"tool_name":"Agent","tool_input":{"prompt":"do a thing"}}' dispatch-trail-missing
# D10 BLOCK: an agent file with no `tools:` line inherits the full set, so it is write-capable.
expect_d D10-no-tools-line 2 '{"tool_name":"Agent","tool_input":{"subagent_type":"notools","prompt":"go"}}' dispatch-trail-missing
# D11 BLOCK: the ASK surface is untouched by Lane D's arrival -- an AskUserQuestion payload with no
#    trail still reds on the ORIGINAL reason, not a dispatch one. (The lanes must not swap.)
expect_d D11-ask-surface-intact 2 '{"tool_name":"AskUserQuestion","questions":[{"question":"Which funding model applies to tips?"}]}' trail-missing
# D12 BLOCK: a card that cites a DECIDED-status record still passes the STATUS half but is refused
#    here only because the id does not resolve -- proving Lane D never runs the ask surface's
#    `trail-answered` rule. On a CARD, citing a decided record is the behaviour being enforced;
#    D2's trail says `open` and D12's says `decided`, and both are judged solely on resolution.
expect_d D12-decided-cite-ok 0 '{"tool_name":"Agent","tool_input":{"subagent_type":"writer","prompt":"DISPATCH. Register check: ADR-20260821-095957 (2026-08-21, decided) -- controlling, and cited as such"}}' dispatch-trail-ok

# ── F1 regression: the resolver must cover all THREE docs/adr filename eras ─────────────────────
# D13/D14 ALLOW: a legacy `ADR-00NN` and a PREFIXLESS middle-era stamp both resolve. Before the fix
#    these were exit 2 -- and the refusal told a coordinator who had done the check CORRECTLY to
#    "fix the id, or state the explicit negative", i.e. to fabricate or to lie.
expect_d D13-legacy-era-resolves 0 '{"tool_name":"Agent","tool_input":{"subagent_type":"writer","prompt":"DISPATCH. Register check: ADR-0032 (2026-07-20, open) -- covers completeness, silent on X"}}' dispatch-trail-ok
expect_d D14-prefixless-era-resolves 0 '{"tool_name":"Agent","tool_input":{"subagent_type":"writer","prompt":"DISPATCH. Register check: ADR-20260720-233000 (2026-07-20, open) -- covers the claim protocol, silent on X"}}' dispatch-trail-ok
# D15 BLOCK: a well-formed LEGACY id that names no file still refuses -- widening the resolver to
#    three eras must not widen it to "anything ADR-shaped". (This case first asserted the reason on
#    `ADR-2026`, testing the trailing-hyphen guard that stops a truncated stamp borrowing a
#    prefixless file's name. That was wrong and the red-first run caught it: `ADR-2026` matches
#    NEITHER alternative of DISPATCH_RECORD_ID, so no id is ever extracted and the verdict is
#    `dispatch-trail-hollow` -- the guard is real but unreachable through the grammar, kept only to
#    mirror decisions.rs. A case that reds for a reason it does not name is the shape E5 already
#    cost this suite once.)
expect_d D15-legacy-id-unknown 2 '{"tool_name":"Agent","tool_input":{"subagent_type":"writer","prompt":"DISPATCH. Register check: ADR-0099 (2026, open) -- covers X"}}' dispatch-trail-unresolved

# ── RULE 1: the red-first card step (ADR-20260906-024838 / #910) ───────────────────────────────
# Committed RED FIRST (D2 before D1, beck/farley): at this commit register-check.sh does not yet
# read a `Red-first:` section at all, so RF1, RF2, RF3 and RF5 below FAIL (the hook allows what it
# should refuse) and the suite reds as a whole -- the run is quoted in the PR before D1 lands.
DTRAIL_RF='Register check: ADR-20260906-050000 (2026-09-06, open) -- covers the redfirst fixture, silent on Y'
# RF1 BLOCK: the cited record names a test (fixture line 4) and the dispatch carries no
#    `Red-first:` section at all -- the incident shape Rule 1 exists to close.
expect_d RF1-redfirst-missing 2 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL_RF\"}}" dispatch-redfirst-missing
# RF2 BLOCK: a `Red-first:` entry present but missing its `mutant:`/`expected red:` fields.
expect_d RF2-redfirst-entry-missing-fields 2 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL_RF\\nRed-first: NEW::test_x — ADR-20260906-050000:4\"}}" dispatch-redfirst-shape
# RF3 BLOCK: a well-shaped entry whose `<record>:<line>` resolves to a real line that holds no
#    token (fixture line 3) -- the anti-theatre half of Rule 1, mirroring Lane D's own D7/D15.
expect_d RF3-redfirst-line-no-token 2 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL_RF\\nRed-first: NEW::test_x — ADR-20260906-050000:3 — mutant: change X — expected red: message\"}}" dispatch-redfirst-shape
# RF4 ALLOW: the explicit negative on a record that names NO test (the shared empty fixture,
#    0 hits). #914 item 2: at 0 hits Rule 1 used to never enter the parse block at all, so `none`
#    was accepted by the rule not firing, never by the rule actually reading and validating it --
#    deleting the `[Nn]one*` ALLOW arm left this case green regardless (beck). The entry parse now
#    runs whatever the hit count is; the count decides only two things (a MISSING section refused,
#    and `none` refused as a false negative), both gated on hits > 0. This case pins the
#    `[Nn]one*` ALLOW arm itself: a genuinely test-free citation, actually read and found clean.
expect_d RF4-redfirst-negative-clean 0 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL\\nRed-first: none — ADR-20260821-095957 names no test\"}}" dispatch-trail-ok
# RF4b BLOCK: at 0 hits, a POSITIVE entry pinned to the 0-hit record itself, at a line the file
#    does not even have (the empty fixture has no line 1) -- proves the parse actually runs and
#    actually checks the line at 0 hits too, not just that `none` is accepted there.
expect_d RF4b-redfirst-positive-malformed-on-zero-hit-record 2 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL\\nRed-first: NEW::test_x — ADR-20260821-095957:1 — mutant: change X — expected red: message\"}}" dispatch-redfirst-shape
# RF4c ALLOW: at 0 hits, a POSITIVE entry pinned to a DIFFERENT record that DOES carry the token
#    (an honest over-declaration, farley) -- the entry stands on its own citation, never on the
#    trail's own hit count, so this must still ALLOW.
expect_d RF4c-redfirst-positive-founded-elsewhere-on-zero-hit-record 0 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL\\nRed-first: NEW::test_x — ADR-20260906-050000:4 — mutant: change X — expected red: message\"}}" dispatch-trail-ok
# RF4d BLOCK: pins the KNOWN VERDICT CHANGE A-prime introduces (beck, checkpoint D5b -- "a change
#    never seen red is an unverified claim"). At 0 hits, prose that merely MENTIONS `Red-first:`
#    with no valid entry and no `none` (verified live: `Red-first: see the section below`) now
#    blocks with `dispatch-redfirst-shape` -- the SAME verdict the >0-hit path already gave that
#    shape. The remedy is the same explicit negative.
expect_d RF4d-redfirst-mentioned-not-entered-on-zero-hits 2 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL\\nRed-first: see the section below\"}}" dispatch-redfirst-shape
# RF5 BLOCK: the SAME explicit-negative text, but on a record that DOES name a test -- the
#    negative claims the opposite of what the citation shows and is refused.
expect_d RF5-redfirst-false-negative 2 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL_RF\\nRed-first: none — ADR-20260906-050000 names no test\"}}" dispatch-redfirst-false-negative
# RF6 ALLOW: a compliant card -- one well-shaped entry pinned to the real hit line.
expect_d RF6-redfirst-compliant 0 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL_RF\\nRed-first: NEW::test_gate_is_red_first — ADR-20260906-050000:4 — mutant: delete the token check — expected red: register-check selftest RF6 fails\"}}" dispatch-trail-ok
# RF7 BLOCK: `<record>:<line>` resolves to a REAL file but a line PAST EOF (the fixture has 5
#    lines; line 99 does not exist) -- the branch the removed `wc -l` guard held (`sed -n Np` on a
#    line past EOF prints nothing, and nothing cannot match the token regex) has never been seen
#    red (#914 item 3, reviewer/beck).
expect_d RF7-redfirst-line-past-eof 2 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL_RF\\nRed-first: NEW::test_x — ADR-20260906-050000:99 — mutant: change X — expected red: message\"}}" dispatch-redfirst-shape
# RF8/RF9 (#914 item 4, farley): no case pinned the ON-DISK `<test path>` arm, so it shipped
# unproven by the suite. `<test path>` is derived from `$HOOK` relative to `$ROOT` -- pinning the
# hook itself, never a hand literal or the selftest's own path -- so a rename of the hook file
# updates this test path with it. RF8 runs with `cwd = $FIX` (a mktemp dir, never `$ROOT`) so the
# cwd-relative `$tpath` arm of `[ ! -e "$ROOT/$tpath" ] && [ ! -e "$tpath" ]` cannot pass and only
# the `$ROOT/$tpath` arm can -- extended `expect_d` with an optional 5th `cwd` arg rather than
# writing the case inline, to keep the reason-check plumbing shared with every other RF case.
TPATH_HOOK="${HOOK#"$ROOT"/}"
# RF8 ALLOW: an EXISTING on-disk test path (the hook itself) resolves via `$ROOT/$tpath`.
expect_d RF8-redfirst-existing-test-path 0 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL_RF\\nRed-first: $TPATH_HOOK::test_x — ADR-20260906-050000:4 — mutant: change X — expected red: message\"}}" dispatch-trail-ok "$FIX"
# RF9 BLOCK: a test path that is neither `NEW` nor on disk anywhere -- the existence check itself.
expect_d RF9-redfirst-missing-test-path 2 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL_RF\\nRed-first: tests/does_not_exist.rs::x — ADR-20260906-050000:4 — mutant: change X — expected red: message\"}}" dispatch-redfirst-shape "$FIX"

# ── #926 item 1: the `none` form is the DECLARED form, not a prefix glob ────────────────────────
# The OLD `[Nn]one*)` case matched ANY text starting with "none", so `Red-first: none` followed by
# ARBITRARY PROSE at 0 hits read as the explicit negative -- found LIVE on this card's own first
# draft: a coordinator sentence containing the marker followed by the word "nonesuch" tripped Lane
# D's dispatch-redfirst-false-negative reason at 17:26Z (the antecedent for RF10 and RF15 below --
# the card that fixes the false positive was itself refused by it, on a genuine hit-count case).
# The negative is now `none — <record-id> names no test`, where `<record-id>` must RESOLVE and must
# be CITED IN THIS TRAIL (tested by resolved path against `rf_files`, never by id string). Anything
# else beginning with `none` is not the declared form and falls through to the ordinary shape
# parse, blocking with the SAME `dispatch-redfirst-shape` a malformed positive entry already gets.
# RF10 BLOCK: "nonesuch garbage" at 0 hits -- the exact incident shape, on the 0-hit fixture. Also
#    asserts the hook's actual STDERR text carries the shape message (#926 item 4's new argument).
expect_d RF10-none-prefix-glob-refused 2 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL\\nRed-first: nonesuch garbage\"}}" dispatch-redfirst-shape "" "no entry matches the required shape"
# RF11a BLOCK: the declared none form's OWN record id must resolve -- an invented id inside
#    `none — <record> names no test` is not a free pass either.
expect_d RF11a-none-unresolvable-record 2 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL\\nRed-first: none — ADR-20260101-000000 names no test\"}}" dispatch-redfirst-shape
# RF11b BLOCK: the declared none form's record must also be CITED IN THIS TRAIL -- tested by
#    RESOLVED PATH against `rf_files`, never by id string. ADR-20260906-050000 resolves (it is the
#    redfirst fixture) but this trail (`$DTRAIL`) cites only ADR-20260821-095957 -- the negative may
#    not borrow a citation the trail never made.
expect_d RF11b-none-uncited-record 2 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL\\nRed-first: none — ADR-20260906-050000 names no test\"}}" dispatch-redfirst-shape

# RF12 BLOCK: a POSITIVE entry pinned to LINE 2 of the NEW non-empty 0-hit fixture -- the line
#    EXISTS and carries no token, a DIFFERENT failure mode from "no such line" (RF4b/RF7, both
#    past-EOF). Also asserts stderr. Mutant: gate the token check behind `rf_hit_count -gt 0` --
#    at 0 hits that would skip the check entirely and wrongly ALLOW.
DTRAIL_ZH='Register check: ADR-20260906-060000 (2026-09-06, open) -- covers the zero-hit fixture, silent on Y'
expect_d RF12-zero-hit-line-without-token 2 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL_ZH\\nRed-first: NEW::test_x — ADR-20260906-060000:2 — mutant: change X — expected red: message\"}}" dispatch-redfirst-shape "" "no entry matches the required shape"
# RF13 BLOCK: an EXISTING on-disk test path with NO `::` separator at >0 hits (`$DTRAIL_RF`) --
#    `$TPATH_HOOK` alone, so `testref` never contains `::` and the entry is skipped as malformed.
#    Mutant: delete the `case "$testref" in *'::'*) ...` guard -- without it `tpath`/`tname` both
#    collapse to the whole (existing, non-empty) path and the entry flips to ALLOW.
expect_d RF13-missing-double-colon-on-existing-path 2 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL_RF\\nRed-first: $TPATH_HOOK — ADR-20260906-050000:4 — mutant: change X — expected red: message\"}}" dispatch-redfirst-shape "$FIX"
# RF14 BLOCK: a trail citing the ZERO-hit fixture FIRST and the redfirst fixture SECOND (citation
#    ORDER pinned here on purpose), with the full declared none form naming the FIRST (0-hit)
#    record. `rf_hit_count` is summed over the WHOLE trail's cited records (0 + 2 = 2), so the
#    negative is still refused as a false negative even though the record IT NAMES has 0 hits --
#    "the negative speaks for the whole trail, not per-record" (workflow.md). Mutant: sum hits over
#    the FIRST cited record only -- that would read 0 and wrongly ALLOW.
DTRAIL_ZH_THEN_RF='Register check: ADR-20260906-060000 (2026-09-06, open) -- covers the zero-hit fixture, and ADR-20260906-050000 (2026-09-06, open) -- covers the redfirst fixture too'
expect_d RF14-none-over-two-records-one-hitting 2 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL_ZH_THEN_RF\\nRed-first: none — ADR-20260906-060000 names no test\"}}" dispatch-redfirst-false-negative
# RF14b BLOCK (beck): the SAME two-record trail as RF14, with the citation ORDER REVERSED (the
#    redfirst fixture FIRST, the zero-hit fixture SECOND) -- same expected refusal, but a
#    DIFFERENT isolating mutant. RF14 alone does not catch "count hits over the LAST cited record
#    only": in RF14's own order that mutant would take the redfirst fixture (the LAST record
#    there) and still sum to the correct answer BY ACCIDENT, surviving the whole suite. Reversing
#    the order here closes that gap: under the "last only" mutant this trail's last-cited record
#    is the ZERO-hit fixture, so the mutant reads 0 and wrongly ALLOWs.
DTRAIL_RF_THEN_ZH='Register check: ADR-20260906-050000 (2026-09-06, open) -- covers the redfirst fixture, and ADR-20260906-060000 (2026-09-06, open) -- covers the zero-hit fixture too'
expect_d RF14b-none-over-two-records-reversed-order 2 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL_RF_THEN_ZH\\nRed-first: none — ADR-20260906-060000 names no test\"}}" dispatch-redfirst-false-negative
# RF15 BLOCK: the SAME malformed text as RF10 ("nonesuch garbage"), but on a record that DOES have
#    hits (`$DTRAIL_RF`) -- PRECEDENCE. This must resolve to `dispatch-redfirst-shape`, never to the
#    false negative RF5 pins: the OLD prefix glob read "nonesuch" as the negative (it starts with
#    "none"), so at >0 hits it blocked for the WRONG reason (`dispatch-redfirst-false-negative`) --
#    still BLOCKED either way, but a case that reds for the wrong reason is a claim without
#    evidence (the shape E5 already cost this suite once).
expect_d RF15-malformed-none-at-hits-is-shape 2 "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"writer\",\"prompt\":\"DISPATCH. $DTRAIL_RF\\nRed-first: nonesuch garbage\"}}" dispatch-redfirst-shape

# ── F2 regression: every `tools:` shape that cannot be READ must fail CLOSED ────────────────────
# A parse failure was being reported as a read declaration of read-only, so each of these was
# exit 0 -- ungated -- on a card with no trail at all.
expect_d D16-tools-list-form 2 '{"tool_name":"Agent","tool_input":{"subagent_type":"listform","prompt":"DISPATCH, no trail."}}' dispatch-trail-missing
expect_d D17-tools-continuation 2 '{"tool_name":"Agent","tool_input":{"subagent_type":"continuation","prompt":"DISPATCH, no trail."}}' dispatch-trail-missing
# D18: the fourth shape, found while fixing the three the review named -- an inline value that is
#    only the FIRST FRAGMENT of a list continued on the next line.
expect_d D18-tools-trailing-comma 2 '{"tool_name":"Agent","tool_input":{"subagent_type":"trailcomma","prompt":"DISPATCH, no trail."}}' dispatch-trail-missing
# D19: `tools: "*"` fails open by a DIFFERENT mechanism -- a non-empty value with no write token --
#    so an emptiness check alone never reaches it.
expect_d D19-tools-wildcard 2 '{"tool_name":"Agent","tool_input":{"subagent_type":"wildcard","prompt":"DISPATCH, no trail."}}' dispatch-trail-missing
expect_d D20-tools-empty-key 2 '{"tool_name":"Agent","tool_input":{"subagent_type":"emptykey","prompt":"DISPATCH, no trail."}}' dispatch-trail-missing
# D21 BLOCK: `Edit` alone is write capability -- the discriminator is not Write-only.
expect_d D21-edit-only-agent 2 '{"tool_name":"Agent","tool_input":{"subagent_type":"editonly","prompt":"DISPATCH, no trail."}}' dispatch-trail-missing
# D22 ALLOW: the token set is CLOSED, so `TodoWrite` -- which grants no filesystem write -- does not
#    drag a lens into the gate the way a `Write|Edit` substring did.
expect_d D22-todowrite-not-write 0 '{"tool_name":"Agent","tool_input":{"subagent_type":"todowrite","prompt":"lens consult"}}' agent-advisory

# ── Round 2, F-A: continuation shapes the first-line parse could not see ────────────────────────
# All four grant Write and all four read `agent-advisory` before the whole-value parse. The first
# two carry an UNBALANCED `[`, which `tr -d "[]"` used to discard before the token scan -- a louder
# "this value is incomplete" signal than the trailing comma round 2 did handle.
expect_d D23-flow-break-after-comma 2 '{"tool_name":"Agent","tool_input":{"subagent_type":"flowbreak-after","prompt":"DISPATCH, no trail."}}' dispatch-trail-missing
# D24 is the mirror of D18: the line breaks BEFORE the comma, so no trailing-comma test can see it.
expect_d D24-flow-break-before-comma 2 '{"tool_name":"Agent","tool_input":{"subagent_type":"flowbreak-before","prompt":"DISPATCH, no trail."}}' dispatch-trail-missing
expect_d D25-plain-break-before-comma 2 '{"tool_name":"Agent","tool_input":{"subagent_type":"plainbreak","prompt":"DISPATCH, no trail."}}' dispatch-trail-missing
# D26: a folded scalar carries NO punctuation at all -- nothing on the first line hints at more.
expect_d D26-folded-scalar 2 '{"tool_name":"Agent","tool_input":{"subagent_type":"folded","prompt":"DISPATCH, no trail."}}' dispatch-trail-missing
# D27 ALLOW: THE FALSE-POSITIVE FLOOR. A genuinely read-only list wrapped across two lines must
# stay advisory -- the round-2 trailing-comma heuristic gated it, and a gate that fires on ordinary
# formatting is one that gets worked around.
expect_d D27-wrapped-readonly-advisory 0 '{"tool_name":"Agent","tool_input":{"subagent_type":"wrapped-readonly","prompt":"lens consult"}}' agent-advisory

# LD LIVE wiring: the fixture cases above would all pass with the LIVE paths mis-wired, so two
# anchors on the real roster -- `executor` is write-capable and `reviewer` is read-only, and
# either changing is a roster decision that should red this case and be looked at.
# LD1 covers ALL THREE live write-capable agents, not just `executor`: the F-A failure scenario is
# a formatter reflowing a `tools:` list across two lines, and that reaches `architect` and
# `generator` exactly as easily (review round 2, F-A).
for _wa in architect executor generator; do
  printf '%s' "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"$_wa\",\"prompt\":\"DISPATCH with no trail whatsoever.\"}}" | REGISTER_CHECK_LOG=/dev/null bash "$HOOK" >/dev/null 2>&1
  if [ $? -ne 2 ]; then
    echo "register-check selftest: case LD1 FAILED -- the LIVE .claude/agents roster did not gate a trail-less dispatch to write-capable '$_wa' (a reflowed or reformatted \`tools:\` list reads advisory)" >&2
    fail=1
  fi
done
printf '%s' '{"tool_name":"Agent","tool_input":{"subagent_type":"reviewer","prompt":"Review the full branch diff."}}' | REGISTER_CHECK_LOG=/dev/null bash "$HOOK" >/dev/null 2>&1
if [ $? -ne 0 ]; then
  echo "register-check selftest: case LD2 FAILED -- the LIVE roster gated a read-only 'reviewer' consult; Lane D must never fire on an advisory call" >&2
  fail=1
fi
# LD3: the live docs tree resolves a real record id -- the anti-theatre check is only as good as
# its resolver, and a resolver pointed at the wrong root refuses EVERY citation (fail-shut drift
# that would read as "the coordinator keeps writing bad trails"). Carries a `Red-first:` entry
# since #910: the REAL ADR-20260821-095957 names its own test discipline, so once Rule 1 exists a
# trail-only citation of it is refused -- keeping this card trail-only would have flipped LD3's own
# verdict, which the card's STOP condition forbids.
# #914 item 5 (reviewer, beck): this used to pin a HARD-CODED line 17, structure-sensitive to an
# edit of that ADR unrelated to Lane D itself. The hit line is now DERIVED at selftest time, the
# same way the corpus test (`every_record_in_the_corpus_is_citable_through_lane_d`,
# tools/codegen-rs/src/tests.rs) does: read `REDFIRST_TOKENS` FROM THE HOOK FILE itself (one `sed`
# over its own `REDFIRST_TOKENS='...'` declaration -- no second copy of the token set to drift),
# then `grep -niE` the live file for its own first hit line.
ld3_tokens="$(sed -n "s/^REDFIRST_TOKENS='\(.*\)'\$/\1/p" "$HOOK")"
ld3_file="$(ls "$ROOT"/docs/adr/ADR-20260821-095957-*.md 2>/dev/null | head -1)"
ld3_line=""
if [ -n "$ld3_tokens" ] && [ -n "$ld3_file" ]; then
  ld3_line="$(grep -niE "$ld3_tokens" "$ld3_file" 2>/dev/null | head -1 | cut -d: -f1)"
fi
if [ -z "$ld3_tokens" ]; then
  echo "register-check selftest: case LD3 FAILED -- could not extract REDFIRST_TOKENS from $HOOK; the sed pattern here is out of sync with the hook's own declaration" >&2
  fail=1
elif [ -z "$ld3_line" ]; then
  echo "register-check selftest: case LD3 FAILED -- no line of $ld3_file matches the extracted token set; re-pin or investigate before trusting this case" >&2
  fail=1
else
  printf '%s' "{\"tool_name\":\"Agent\",\"tool_input\":{\"subagent_type\":\"executor\",\"prompt\":\"DISPATCH. $DTRAIL\\nRed-first: NEW::proves_live_docs_resolve — ADR-20260821-095957:$ld3_line — mutant: delete the docs override guard — expected red: register-check selftest LD3 fails to resolve\"}}" | REGISTER_CHECK_LOG=/dev/null bash "$HOOK" >/dev/null 2>&1
  if [ $? -ne 0 ]; then
    echo "register-check selftest: case LD3 FAILED -- a trail citing the LIVE ADR-20260821-095957 (derived hit line $ld3_line) did not resolve; check REGISTER_CHECK_DOCS/docs layout before rewriting trails" >&2
    fail=1
  fi
fi

# ── The LIVE corpus wiring (no env override) ────────────────────────────────────────────────────
# L1: the live dir parses and gates -- REG-2 is decided forever (a reversal opens a NEW row).
printf '%s' "{\"questions\":[{\"question\":\"Reopen REG-2? $TRAIL\"}]}" | REGISTER_CHECK_LOG=/dev/null bash "$HOOK" >/dev/null 2>&1
if [ $? -ne 2 ]; then
  echo "register-check selftest: case L1 FAILED -- the LIVE docs/decisions corpus did not gate a question referencing decided row REG-2" >&2
  fail=1
fi
# L3: the live envelope lane rejects an unknown key (proves the live wiring of the new lane).
printf '%s' '{"questions":[{"question":"Decision row: ZZZZ-NOT-DECLARED -- decide?"}]}' | REGISTER_CHECK_LOG=/dev/null bash "$HOOK" >/dev/null 2>&1
if [ $? -ne 2 ]; then
  echo "register-check selftest: case L3 FAILED -- the LIVE corpus did not reject an unknown envelope key" >&2
  fail=1
fi
# L2: the live legacy allowlist exists and is non-empty (legacy is a declaration, not a default).
if ! sed -n 's/^  - \([A-Z][A-Z0-9-]*\)$/\1/p' "$ROOT/docs/decisions/_legacy.yaml" 2>/dev/null | grep -q .; then
  echo "register-check selftest: case L2 FAILED -- docs/decisions/_legacy.yaml missing or lists no legacy keys" >&2
  fail=1
fi

# W WIRING (SEMANTIC since the 2026-08-21 hardening slice): the arming declaration is checked
# structurally, not by substring -- the old greps stayed green with the matcher fuzzed
# (AskUserQuestionX) or the whole entry moved to PostToolUse, both of which disarm the gate.
# check_wiring proves .claude/settings.json carries a hooks.PreToolUse entry whose matcher is
# EXACTLY AskUserQuestion and whose command runs the real script. python3 (stdlib json only, no
# added toolchain) is required; its absence FAILS the case -- fail closed, never a silent skip.
# Parameterised by MATCHER since 2026-08-31: the same script is now armed on two surfaces
# (AskUserQuestion = the ask gate, Agent = Lane D, the coordinator's dispatch card), and each entry
# can be disarmed independently, so each is proven independently.
check_wiring() { # check_wiring <settings.json> <matcher> -> 0 armed / nonzero not
  command -v python3 >/dev/null 2>&1 || return 3
  python3 - "$1" "$2" <<'PYEOF'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
except Exception:
    sys.exit(1)
for entry in d.get("hooks", {}).get("PreToolUse", []):
    if entry.get("matcher") != sys.argv[2]:
        continue
    for h in entry.get("hooks", []):
        if h.get("type") == "command" and h.get("command", "").endswith("/.claude/hooks/register-check.sh"):
            sys.exit(0)
sys.exit(1)
PYEOF
}
# The three disarming shapes, planted as mutant fixtures derived from the REAL committed file, so
# each red case proves the checker sees through exactly one disarming move.
# The mutants are located BY MATCHER, not by index: a second entry (Agent) was appended in
# 2026-08-31 and an index-addressed builder silently mutates whichever entry happens to sit there.
if ! python3 - "$ROOT/.claude/settings.json" "$FIX" <<'PYEOF'
import copy, json, sys
src, out = sys.argv[1], sys.argv[2]
d = json.load(open(src))

def idx(doc, matcher):
    for i, e in enumerate(doc["hooks"]["PreToolUse"]):
        if e.get("matcher") == matcher:
            assert "register-check.sh" in e["hooks"][0]["command"], \
                f"settings.json PreToolUse[{matcher}] is no longer a register-check entry -- update the mutant builder"
            return i
    raise AssertionError(f"settings.json declares no PreToolUse entry with matcher {matcher} -- the gate is disarmed on that surface")

ask, agent = idx(d, "AskUserQuestion"), idx(d, "Agent")
OTHER = "bash \"$CLAUDE_PROJECT_DIR\"/.claude/hooks/some-other-hook.sh"
m1 = copy.deepcopy(d); m1["hooks"]["PreToolUse"][ask]["matcher"] = "AskUserQuestionX"
m2 = copy.deepcopy(d); m2["hooks"]["PostToolUse"] = m2["hooks"].pop("PreToolUse")
m3 = copy.deepcopy(d); m3["hooks"]["PreToolUse"][ask]["hooks"][0]["command"] = OTHER
m4 = copy.deepcopy(d); m4["hooks"]["PreToolUse"][agent]["matcher"] = "AgentX"
m5 = copy.deepcopy(d); m5["hooks"]["PreToolUse"][agent]["hooks"][0]["command"] = OTHER
m6 = copy.deepcopy(d); del m6["hooks"]["PreToolUse"][agent]
for name, m in [("settings-mutant-matcher.json", m1), ("settings-mutant-event.json", m2),
                ("settings-mutant-command.json", m3), ("settings-mutant-agent-matcher.json", m4),
                ("settings-mutant-agent-command.json", m5), ("settings-mutant-agent-absent.json", m6)]:
    json.dump(m, open(f"{out}/{name}", "w"), indent=1)
PYEOF
then
  echo "register-check selftest: case W FAILED -- python3 missing or the settings mutant builder broke (fail closed)" >&2
  fail=1
elif ! check_wiring "$ROOT/.claude/settings.json" AskUserQuestion; then
  echo "register-check selftest: case W FAILED -- .claude/settings.json no longer wires register-check.sh to a PreToolUse/AskUserQuestion declaration (the ask gate is disarmed)" >&2
  fail=1
elif ! check_wiring "$ROOT/.claude/settings.json" Agent; then
  echo "register-check selftest: case WD FAILED -- .claude/settings.json no longer wires register-check.sh to a PreToolUse/Agent declaration (Lane D, the coordinator's dispatch gate, is disarmed -- ADR-20260831-141500)" >&2
  fail=1
else
  # W1 fuzzed matcher / W2 wrong event / W3 wrong command: the checker must refuse each.
  check_wiring "$FIX/settings-mutant-matcher.json" AskUserQuestion && { echo "register-check selftest: case W1 FAILED -- checker accepted matcher AskUserQuestionX" >&2; fail=1; }
  check_wiring "$FIX/settings-mutant-event.json"   AskUserQuestion && { echo "register-check selftest: case W2 FAILED -- checker accepted the entry under PostToolUse" >&2; fail=1; }
  check_wiring "$FIX/settings-mutant-command.json" AskUserQuestion && { echo "register-check selftest: case W3 FAILED -- checker accepted a command pointing at another script" >&2; fail=1; }
  # W4-W6: the SAME three disarming shapes against the Agent entry, plus its deletion -- the one
  # that costs nothing to do accidentally, since the ask gate stays green while Lane D vanishes.
  check_wiring "$FIX/settings-mutant-agent-matcher.json" Agent && { echo "register-check selftest: case W4 FAILED -- checker accepted matcher AgentX" >&2; fail=1; }
  check_wiring "$FIX/settings-mutant-agent-command.json" Agent && { echo "register-check selftest: case W5 FAILED -- checker accepted an Agent command pointing at another script" >&2; fail=1; }
  check_wiring "$FIX/settings-mutant-agent-absent.json"  Agent && { echo "register-check selftest: case W6 FAILED -- checker accepted settings with the Agent entry deleted" >&2; fail=1; }
  check_wiring "$FIX/settings-mutant-event.json"         Agent && { echo "register-check selftest: case W7 FAILED -- checker accepted the Agent entry under PostToolUse" >&2; fail=1; }
fi

# D DRIFT: every standing agent carries the citation block (marker + pointer to the canonical rule).
for f in "$ROOT"/.claude/agents/*.md; do
  if ! grep -qF 'Register check:' "$f" || ! grep -q 'check the register before you ask' "$f"; then
    echo "register-check selftest: case D FAILED -- $(basename "$f") lacks the register-check citation block (docs/claude/sessions/workflow.md is the canonical rule)" >&2
    fail=1
  fi
done

# C CANON: the canonical rule the blocks and the hook cite still exists where they point.
if ! grep -q 'check the register before you ask' "$ROOT/docs/claude/sessions/workflow.md"; then
  echo "register-check selftest: case C FAILED -- docs/claude/sessions/workflow.md no longer carries the canonical register-check rule the blocks cite" >&2
  fail=1
fi

# CC CORPUS RULE: the generalisation Lane D cost two rounds to learn is prose, and prose can be
# deleted silently. The hook's own completeness comment cites it by name, so this case makes the
# pairing non-deletable: the rule must exist, and the test that makes it executable must still be
# named in the suite that enforces it (ADR-20260831-141500; workflow.md, "a gate that classifies
# members of a corpus is tested against the CORPUS, not against fixtures").
if ! grep -q 'only the corpus proves the classification' "$ROOT/docs/claude/sessions/workflow.md"; then
  echo "register-check selftest: case CC FAILED -- docs/claude/sessions/workflow.md no longer carries the corpus-vs-fixtures rule that .claude/hooks/register-check.sh cites for its completeness claim" >&2
  fail=1
fi
if ! grep -q 'every_record_in_the_corpus_is_citable_through_lane_d' "$ROOT/tools/codegen-rs/src/tests.rs"; then
  echo "register-check selftest: case CC FAILED -- the corpus-completeness test every_record_in_the_corpus_is_citable_through_lane_d is gone from tools/codegen-rs/src/tests.rs; Lane D's completeness claim would be prose again" >&2
  fail=1
fi

# ── #926 item 5, RESHAPED by farley: a WARN-only regression TRIPWIRE, never a hang bound ────────
# A post-hoc self-timer cannot BE a hang bound: a hung script never reaches the line that would
# read it -- CI's `gate-scripts` job already carries `timeout-minutes: 10` for that, at the JOB
# level. What a self-timer CAN do is make growth VISIBLE: the elapsed wall time is printed on
# every run (a reading visible in every CI log), and crossing
# REGISTER_CHECK_SELFTEST_CEILING_SECONDS (default 120s) prints exactly ONE WARN line to stderr,
# naming itself a TRIPWIRE -- an UNVERIFIED ceiling, never a hang bound (the job timeout above
# already owns that) and never a METER (#923 §4's `gate_minutes_per_round`, read from CI check-run
# times, owns that). It NEVER changes the exit code: the every-turn local path from stop-gate.sh
# must not become a trainer that teaches a session to re-run past a warning.
# Antecedent: the gate-scripts job's own selftest step measured 4s on five recent green main runs
# (Actions API, 2026-09-06, 1s granularity) and 4.7s locally on this container.
# A small in-process FUNCTION, so ST1 below can call it directly with SYNTHETIC elapsed/ceiling
# values rather than actually sleeping the suite past the ceiling.
register_check_selftest_ceiling_warn() { # register_check_selftest_ceiling_warn <elapsed> <ceiling>
  local elapsed="$1" ceiling="$2"
  if [ "$elapsed" -gt "$ceiling" ]; then
    echo "register-check selftest: WARN -- elapsed ${elapsed}s past REGISTER_CHECK_SELFTEST_CEILING_SECONDS=${ceiling}s (TRIPWIRE, UNVERIFIED ceiling: a run past it is a regression signal, not a hang bound -- CI bounds hangs at the job level, timeout-minutes: 10 -- and not a meter, #923 §4 owns that)" >&2
  fi
}
# ST1: the WARN fires when elapsed > ceiling and stays SILENT when it does not -- pinned with
# synthetic values (5s against a ceiling of 0, then 5s against the real default 120) so the case
# never depends on the suite's own actual runtime.
st1_out="$(register_check_selftest_ceiling_warn 5 0 2>&1 >/dev/null)"
if ! printf '%s' "$st1_out" | grep -qF 'TRIPWIRE'; then
  echo "register-check selftest: case ST1-selftest-ceiling-tripwire-warns FAILED -- no WARN/TRIPWIRE line on stderr with elapsed=5 ceiling=0 (got: $st1_out)" >&2
  fail=1
fi
st1_quiet="$(register_check_selftest_ceiling_warn 5 120 2>&1 >/dev/null)"
if [ -n "$st1_quiet" ]; then
  echo "register-check selftest: case ST1-selftest-ceiling-tripwire-warns FAILED -- unexpected WARN with elapsed=5 ceiling=120 (got: $st1_quiet)" >&2
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  echo "register-check selftest: FAILED (see cases above)" >&2
  exit 2
fi
SELFTEST_ELAPSED=$(( $(date +%s) - SELFTEST_START_TS ))
SELFTEST_CEILING="${REGISTER_CHECK_SELFTEST_CEILING_SECONDS:-120}"
register_check_selftest_ceiling_warn "$SELFTEST_ELAPSED" "$SELFTEST_CEILING"
echo "register-check selftest: all cases pass (${SELFTEST_ELAPSED}s)."
exit 0
