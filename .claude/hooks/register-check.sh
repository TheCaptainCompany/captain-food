#!/usr/bin/env bash
# Captain.Food register-check gate (Claude Code PreToolUse hook on AskUserQuestion).
#
# WHY THIS EXISTS. Founder directive 2026-08-21: "I want to ensure that the agents will no longer
# ask questions already answered." The failure it gates was banked twice (ADR-20260818-210000
# coordinator defect 2; DECISIONS.md paragraph 48 / PROP-20260819-110442): a settled question was
# re-asked because nothing REQUIRED the register lookup before asking. This hook makes the lookup's
# trail a precondition of the ask -- the enforcement-on-the-ask direction of REG-1(a), in the form
# buildable before the register rows carry machine-readable status.
#
# WHAT IT PROVES -- AND WHAT IT DOES NOT. It verifies the PRESENCE AND SHAPE of a register-check
# trail in the question payload, not that a search actually happened. The trail must name a
# verifiable artifact (a record id) or explicitly record the negative with its search terms, so a
# hollow trail is auditably hollow on spot-check (the log below). Honesty is enforced by the mob
# and review, per docs/claude/sessions/workflow.md ("check the register before you ask -- and
# before you assert"), the single canonical statement of the protocol. This hook gates the
# AskUserQuestion transport only; questions travelling as prose (reports, PR bodies, register
# rows, decision forms) are bound by the agent-file blocks citing the same rule.
#
# VERDICTS. Exit 0 = trail present and well-shaped (allow). Exit 2 = block, stderr fed back to the
# model. NEVER exit 1: in Claude Code any non-2 nonzero exit allows the call with a warning, which
# would silently convert this gate into no gate (the ADR-20260810-231300 silent-fallback defect
# class). Unreadable or empty input therefore also exits 2 -- fail closed.
#
# Dependency-free on purpose: bash + grep on raw stdin text. No jq -- a missing dependency must not
# be able to disarm the gate.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# Overridable so the selftest's fixture payloads never pollute the real firing log.
LOG="${REGISTER_CHECK_LOG:-$ROOT/.claude/register-check.log}"

# The canonical trail format is DECLARED in docs/claude/sessions/workflow.md; this regex cites it,
# never redefines it. A passing trail contains the marker line and either
#   (a) a record id: ADR-YYYYMMDD-HHMMSS | legacy ADR-00NN | PROP-YYYYMMDD-HHMMSS |
#       a DECISIONS register section | a status journal week file, or
#   (b) the explicit negative: "no controlling record" / "no candidate record" (with its terms).
MARKER='Register check:'
RECORD_ID='ADR-[0-9]{8}-[0-9]{6}|ADR-00[0-9]{2}|PROP-[0-9]{8}-[0-9]{6}|DECISIONS[^"]{0,24}[0-9]+|journal-[0-9]{4}-W[0-9]{2}'
NO_RECORD='[Nn]o (controlling|candidate) record'

payload="$(cat 2>/dev/null || true)"

note() { # one greppable line per firing; a logging failure must never change the verdict
  printf '%s\t%s\t%.100s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$1" "${payload//[$'\t\n']/ }" >> "$LOG" 2>/dev/null || true
}

if [ -n "$payload" ] && printf '%s' "$payload" | grep -qF "$MARKER"; then
  if printf '%s' "$payload" | grep -qE "$RECORD_ID" || printf '%s' "$payload" | grep -qE "$NO_RECORD"; then
    note ALLOW
    exit 0
  fi
fi

note BLOCK
cat >&2 <<'EOF'
register-check: this question carries no register-check trail, so it cannot go to the founder yet.
A settled question re-asked spends his attention and reads as the team not knowing its own records
(the record last re-litigated was one grep away). Do the check, THEN ASK -- never drop the question.

1. Search the decision sources with the question's own vocabulary AND the alias table's terms:
   docs/adr/, docs/proposals/DECISIONS.md, recent docs/status/journal-YYYY-Www.md
   (docs/legal/ for legal subjects). Read the surrounding record, not the matching line, and
   follow supersession/amendment references to the current head.
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
exit 2
