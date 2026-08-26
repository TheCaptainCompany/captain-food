#!/usr/bin/env bash
# Guard tests for .claude/hooks/register-check.sh, plus the wiring- and drift-checks that keep the
# register-check discipline alive (run from stop-gate.sh on every turn -- pure shell, ~200ms).
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
# 3 ALLOW: trail citing a controlling record id.
expect 3-record-id 0 "$FIX" '{"questions":[{"question":"Confirm scope. Register check: ADR-20260819-103112 (2026-08-19, decided) -- covers refunds, silent on thresholds"}]}'
# 4 ALLOW: legacy ADR id form.
expect 4-legacy-id 0 "$FIX" '{"questions":[{"question":"... Register check: ADR-0032 (completeness) covers this"}]}'
# 5 ALLOW: explicit negative with terms -- a genuinely new question is the system working.
expect 5-no-record 0 "$FIX" '{"questions":[{"question":"New option space. Register check: no controlling record -- terms: payout, settlement, virement; nearest: none"}]}'
# 6 ALLOW: DECISIONS register section citation.
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
check_wiring() { # check_wiring <settings.json> -> 0 armed / nonzero not
  command -v python3 >/dev/null 2>&1 || return 3
  python3 - "$1" <<'PYEOF'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
except Exception:
    sys.exit(1)
for entry in d.get("hooks", {}).get("PreToolUse", []):
    if entry.get("matcher") != "AskUserQuestion":
        continue
    for h in entry.get("hooks", []):
        if h.get("type") == "command" and h.get("command", "").endswith("/.claude/hooks/register-check.sh"):
            sys.exit(0)
sys.exit(1)
PYEOF
}
# The three disarming shapes, planted as mutant fixtures derived from the REAL committed file, so
# each red case proves the checker sees through exactly one disarming move.
if ! python3 - "$ROOT/.claude/settings.json" "$FIX" <<'PYEOF'
import copy, json, sys
src, out = sys.argv[1], sys.argv[2]
d = json.load(open(src))
entry = d["hooks"]["PreToolUse"][0]
assert "register-check.sh" in entry["hooks"][0]["command"], "settings.json PreToolUse[0] is no longer the register-check entry -- update the mutant builder"
m1 = copy.deepcopy(d); m1["hooks"]["PreToolUse"][0]["matcher"] = "AskUserQuestionX"
m2 = copy.deepcopy(d); m2["hooks"]["PostToolUse"] = m2["hooks"].pop("PreToolUse")
m3 = copy.deepcopy(d); m3["hooks"]["PreToolUse"][0]["hooks"][0]["command"] = "bash \"$CLAUDE_PROJECT_DIR\"/.claude/hooks/some-other-hook.sh"
for name, m in [("settings-mutant-matcher.json", m1), ("settings-mutant-event.json", m2), ("settings-mutant-command.json", m3)]:
    json.dump(m, open(f"{out}/{name}", "w"), indent=1)
PYEOF
then
  echo "register-check selftest: case W FAILED -- python3 missing or the settings mutant builder broke (fail closed)" >&2
  fail=1
elif ! check_wiring "$ROOT/.claude/settings.json"; then
  echo "register-check selftest: case W FAILED -- .claude/settings.json no longer wires register-check.sh to a PreToolUse/AskUserQuestion declaration (the gate is disarmed)" >&2
  fail=1
else
  # W1 fuzzed matcher / W2 wrong event / W3 wrong command: the checker must refuse each.
  check_wiring "$FIX/settings-mutant-matcher.json" && { echo "register-check selftest: case W1 FAILED -- checker accepted matcher AskUserQuestionX" >&2; fail=1; }
  check_wiring "$FIX/settings-mutant-event.json"   && { echo "register-check selftest: case W2 FAILED -- checker accepted the entry under PostToolUse" >&2; fail=1; }
  check_wiring "$FIX/settings-mutant-command.json" && { echo "register-check selftest: case W3 FAILED -- checker accepted a command pointing at another script" >&2; fail=1; }
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

if [ "$fail" -ne 0 ]; then
  echo "register-check selftest: FAILED (see cases above)" >&2
  exit 2
fi
echo "register-check selftest: all cases pass."
exit 0
