#!/usr/bin/env bash
# Captain.Food register-check gate (Claude Code PreToolUse hook on AskUserQuestion).
#
# WHY THIS EXISTS. Founder directives 2026-08-21: "I want to ensure that the agents will no longer
# ask questions already answered" (ADR-20260821-010543 — the trail requirement), then "a validator
# that rejects any founder-directed decision question referencing a non-open row" (the REG-2/REG-4
# slice, ADR-20260821-095957 — the row check). The failure both gate was banked twice
# (ADR-20260818-210000 coordinator defect 2; DECISIONS.md §48 / PROP-20260819-110442): a settled
# question was re-asked because nothing REQUIRED the register lookup before asking.
#
# TWO CHECKS, in order:
#   1. TRAIL — the question carries the register-check trail declared in
#      docs/claude/sessions/workflow.md: a record id, or the explicit negative with search terms.
#   2. ROWS — for every key declared in docs/decisions/*.yaml that the question references, the
#      row must be OPEN (and a counsel-owned open row only takes questions about the external
#      action). Keys on the docs/decisions/_legacy.yaml allowlist are the LEGACY lane: they pass,
#      logged, until each is migrated at its next touch. The row FILES are read at the point of
#      need — never the generated DECISIONS.md index; a stale projection must not gate a live
#      decision.
#
# WHAT IT PROVES — AND WHAT IT DOES NOT. It verifies trail PRESENCE AND SHAPE and row STATUS on
# the AskUserQuestion transport only; it cannot prove a search happened, cannot see questions
# travelling as prose (those are bound by the agent-file blocks citing workflow.md), and cannot
# tell a typo'd key from an acronym (an unknown token is simply not a register reference — the
# decision-ask-unregistered rule of PROP-20260819-110442 slice 3 closes that, after full
# migration). Honesty stays with the mob and review, via the firing log.
#
# VERDICTS. Exit 0 = allow. Exit 2 = block, stderr fed back to the model. NEVER exit 1: any other
# nonzero exit allows the call with a warning, silently converting this gate into no gate
# (ADR-20260810-231300's silent-fallback defect class). Unreadable/empty input exits 2 — fail
# closed — and so does a REGISTER_CHECK_DECISIONS override pointing at a missing/empty directory
# (a misspelled override must not disarm the row check).
#
# Dependency-free on purpose: bash + grep + sed on raw stdin text. No jq, no yq — a missing
# dependency must not be able to disarm the gate.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# Overridable so the selftest's fixture payloads never pollute the real firing log.
LOG="${REGISTER_CHECK_LOG:-$ROOT/.claude/register-check.log}"
# Overridable so the selftest runs against hermetic fixture rows, never the live corpus (whose
# statuses change). Firing-log lines carry a "fixture" tag whenever the override is set, so a
# leaked override is visible in the log rather than silently answering from fixtures.
DECISIONS_DIR="${REGISTER_CHECK_DECISIONS:-$ROOT/docs/decisions}"
FIXTURE_TAG="-"
[ -n "${REGISTER_CHECK_DECISIONS:-}" ] && FIXTURE_TAG="fixture"

# The canonical trail format is DECLARED in docs/claude/sessions/workflow.md; these regexes cite
# it, never redefine it.
MARKER='Register check:'
RECORD_ID='ADR-[0-9]{8}-[0-9]{6}|ADR-00[0-9]{2}|PROP-[0-9]{8}-[0-9]{6}|DECISIONS[^"]{0,24}[0-9]+|journal-[0-9]{4}-W[0-9]{2}'
NO_RECORD='[Nn]o (controlling|candidate) record'

payload="$(cat 2>/dev/null || true)"

reasons=""
keys_hit=""
block_msgs=""

note() { # one greppable line per firing; a logging failure must never change the verdict
  printf '%s\t%s\t%s\t%s\t%s\t%.100s\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$1" "${2:-?}" "${3:--}" "$FIXTURE_TAG" "${payload//[$'\t\n']/ }" \
    >> "$LOG" 2>/dev/null || true
}

block() { # block <reason> <message>
  reasons="${reasons:+$reasons,}$1"
  block_msgs="${block_msgs}${2}
"
}

# Read one scalar field from a decision row file (flat schema, values optionally double-quoted).
field() { sed -n "s/^$2:[[:space:]]*\"\{0,1\}\([^\"]*\)\"\{0,1\}[[:space:]]*\$/\1/p" "$1" | head -1; }

# ── Check 1: the trail ──────────────────────────────────────────────────────────────────────────
if [ -z "$payload" ]; then
  block "empty-input" "register-check: empty/unreadable tool payload — failing closed."
elif ! printf '%s' "$payload" | grep -qF "$MARKER"; then
  block "trail-missing" "register-check: this question carries no register-check trail."
elif ! printf '%s' "$payload" | grep -qE "$RECORD_ID" && ! printf '%s' "$payload" | grep -qE "$NO_RECORD"; then
  block "trail-hollow" "register-check: the trail names no record id and no explicit negative — a bare marker is not a trail."
fi

# ── Check 2: the rows (REG-2/REG-4 ask gate) ────────────────────────────────────────────────────
if [ -n "${REGISTER_CHECK_DECISIONS:-}" ] && { [ ! -d "$DECISIONS_DIR" ] || ! ls "$DECISIONS_DIR"/[A-Z]*.yaml >/dev/null 2>&1; }; then
  block "override-broken" "register-check: REGISTER_CHECK_DECISIONS points at a missing/empty directory — failing closed rather than silently skipping the row check."
fi
refs_key() { printf '%s' "$payload" | grep -qE "(^|[^A-Za-z0-9-])$1([^A-Za-z0-9-]|\$)"; }
if [ -d "$DECISIONS_DIR" ]; then
  for f in "$DECISIONS_DIR"/[A-Z]*.yaml; do
    [ -e "$f" ] || continue
    key="$(basename "$f" .yaml)"
    refs_key "$key" || continue
    keys_hit="${keys_hit:+$keys_hit }$key"
    status="$(field "$f" status)"
    case "$status" in
      open)
        owner="$(field "$f" owner)"
        if [ "$owner" = "counsel" ] && ! printf '%s' "$payload" | grep -qiF "external action"; then
          block "key-counsel-owned" "register-check: row \`$key\` is OPEN but counsel-owned — no lens output or founder answer is legal advice or clearance (ADR-20260812-143619). A founder question about it asks for the EXTERNAL ACTION (engaging counsel), never the answer itself; if that is what you are asking, say 'external action' in the question and re-issue."
        fi
        ;;
      decided)
        block "key-decided" "register-check: the question references register row \`$key\`, whose status is DECIDED ($(field "$f" decided) — decided_by: $(field "$f" decided_by)). A decided row is not a question: report the citation instead. If the answer is wrong or its premise has changed, that is a DECISION REVERSAL — open a NEW row citing this one (docs/decisions/README.md), do not re-ask it. If you only cited \`$key\` as the nearest record, cite its decided_by record id instead of the row key."
        ;;
      superseded)
        block "key-superseded" "register-check: row \`$key\` is SUPERSEDED by row \`$(field "$f" superseded_by)\` (decided_by: $(field "$f" decided_by)) — address the successor row, not this one."
        ;;
      deferred)
        block "key-deferred" "register-check: row \`$key\` is DEFERRED until: $(field "$f" until). Re-asking before the wake condition is a decision reversal — open a NEW row citing this one if the condition itself is wrong."
        ;;
      withdrawn)
        block "key-withdrawn" "register-check: row \`$key\` is WITHDRAWN — $(field "$f" note) If the question is live again, open a NEW row citing this one."
        ;;
      *)
        block "key-unreadable" "register-check: row \`$key\` has unreadable status '$status' — fix docs/decisions/$key.yaml (make validate names the defect) before asking about it."
        ;;
    esac
  done
  # The legacy lane: a closed, committed allowlist — logged, never blocking (founder bound:
  # no backfill; a legacy row migrates at its next touch, and citing one is not a touch).
  if [ -f "$DECISIONS_DIR/_legacy.yaml" ]; then
    while IFS= read -r lk; do
      if refs_key "$lk"; then
        keys_hit="${keys_hit:+$keys_hit }$lk(legacy)"
        reasons="${reasons:+$reasons,}key-legacy"
      fi
    done < <(sed -n 's/^  - \([A-Z][A-Z0-9-]*\)$/\1/p' "$DECISIONS_DIR/_legacy.yaml")
  fi
fi

if [ -z "$block_msgs" ]; then
  note ALLOW "${reasons:-ok}" "${keys_hit:--}"
  exit 0
fi

note BLOCK "$reasons" "${keys_hit:--}"
printf '%s' "$block_msgs" >&2
case "$reasons" in *trail-missing*|*trail-hollow*|*empty-input*) cat >&2 <<'EOF'

A settled question re-asked spends the founder's attention and reads as the team not knowing its
own records. Do the check, THEN ASK -- never drop the question.

1. Search the decision sources with the question's own vocabulary AND the alias table's terms:
   docs/decisions/ (the declared rows), docs/adr/, docs/proposals/DECISIONS.md, recent
   docs/status/journal-YYYY-Www.md (docs/legal/ for legal subjects). Read the surrounding record,
   follow supersession to the current head.
2. If a controlling record answers it: do not ask. Report the citation instead
   (id + date + status), per docs/claude/sessions/workflow.md.
3. Otherwise re-issue the question with one trail line in its text, in the canonical form
   declared in docs/claude/sessions/workflow.md:
     Register check: <record id> (<date>, <status>) -- covers <X>, silent on <Y>
   or, when the search comes back empty (a genuinely new question, a clarification of a directive
   the founder just gave, or an external-clock relay -- all legitimate and never to be delayed):
     Register check: no controlling record -- terms: <terms searched>; nearest: <record or none>
A counsel-gated or still-open record is NOT an answer -- cite it and ask. "The answer exists but
the underlying facts changed" is a legitimate re-ask that names the record.
EOF
esac
exit 2
