#!/usr/bin/env bash
# Captain.Food register-check gate (Claude Code PreToolUse hook on AskUserQuestion).
#
# WHY THIS EXISTS. Three founder directives of 2026-08-21, in order: "agents will no longer ask
# questions already answered" (ADR-20260821-010543 — the trail), the REG-2/REG-4 row gate
# (ADR-20260821-095957), and decision-ask-unregistered (ADR-20260821-103403 — the envelope): a
# founder-directed decision question must reference EXACTLY ONE declared register row, and that
# row must be OPEN. The failure all three gate was banked twice (ADR-20260818-210000 coordinator
# defect 2; DECISIONS.md §48 / PROP-20260819-110442).
#
# THE LANES, in order:
#   1. ENVELOPE — a decision question carries one `Decision row: <KEY>` line. Exactly one line,
#      one key; the key must be DECLARED (docs/decisions/<KEY>.yaml) and OPEN. A non-open row is
#      refused with the controlling record and the correct next action (`reconsiders:` for a
#      reversal); a LEGACY key (on _legacy.yaml) is refused with migrate-first — a founder-facing
#      question IS a migration trigger, and migrating in the same change unblocks the same
#      question live; an UNKNOWN key is refused with today's open rows and the create-row path.
#      A valid envelope IS the register check — no separate trail line is required (the declared
#      row carries the evidence). An open counsel-owned row takes only the external-action
#      question.
#   2. TRAIL — a question with NO envelope is a NON-decision interaction (a clarification of an
#      in-flight directive, an external-clock relay, a mechanical choice) and carries the trail of
#      docs/claude/sessions/workflow.md; since 2026-08-21 the negative trail ASSERTS "this is not
#      a decision question" (tiebreaker at the declaration site: would the answer bind future
#      work? then it is a decision question and needs a row).
#   3. PASSIVE — any OTHER declared key referenced in the text is checked by status (defense in
#      depth); legacy keys mentioned as CONTEXT pass, logged (`key-legacy`) — ask-vs-cite is
#      distinguished by the envelope, not by the key.
#
# WHAT IT PROVES — AND WHAT IT DOES NOT. It verifies envelope/trail presence and shape and row
# STATUS on the AskUserQuestion transport; it cannot prove a search happened, cannot classify a
# prose question that omits the envelope (the honest hole: misclassifying a decision question as
# a clarification is a prose dodge, caught by review, not by this gate), and cannot see questions
# travelling as free text. Row files are read at the point of need — never the generated index.
#
# VERDICTS. Exit 0 = allow. Exit 2 = block, stderr fed back. NEVER exit 1 (any other nonzero
# allows with a warning — ADR-20260810-231300's silent-fallback class). Empty input and a broken
# REGISTER_CHECK_DECISIONS override both fail closed. Dependency-free: bash + grep + sed.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOG="${REGISTER_CHECK_LOG:-$ROOT/.claude/register-check.log}"
DECISIONS_DIR="${REGISTER_CHECK_DECISIONS:-$ROOT/docs/decisions}"
FIXTURE_TAG="-"
[ -n "${REGISTER_CHECK_DECISIONS:-}" ] && FIXTURE_TAG="fixture"

MARKER='Register check:'
RECORD_ID='ADR-[0-9]{8}-[0-9]{6}|ADR-00[0-9]{2}|PROP-[0-9]{8}-[0-9]{6}|DECISIONS[^"]{0,24}[0-9]+|journal-[0-9]{4}-W[0-9]{2}'
NO_RECORD='[Nn]o (controlling|candidate) record'
ENVELOPE='Decision row:'
KEY_GRAMMAR='[A-Z][A-Z0-9-]{2,63}'

payload="$(cat 2>/dev/null || true)"
session="$(printf '%s' "$payload" | grep -o '"session_id":"[^"]*"' | head -1 | sed 's/.*:"//;s/"$//')"

reasons=""
keys_hit=""
block_msgs=""
extra="-"

note() { # ts VERDICT reasons keys fixture session extra payload-snippet — one line per firing
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%.100s\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$1" "${2:-?}" "${3:--}" "$FIXTURE_TAG" "${session:--}" \
    "${extra//[$'\t\n']/ }" "${payload//[$'\t\n']/ }" >> "$LOG" 2>/dev/null || true
}

block() { # block <reason> <message>
  reasons="${reasons:+$reasons,}$1"
  block_msgs="${block_msgs}${2}
"
}

# Read one scalar field from a decision row file (flat schema, values optionally double-quoted).
field() { sed -n "s/^$2:[[:space:]]*\"\{0,1\}\([^\"]*\)\"\{0,1\}[[:space:]]*\$/\1/p" "$1" | head -1; }

refs_key() { printf '%s' "$payload" | grep -qE "(^|[^A-Za-z0-9-])$1([^A-Za-z0-9-]|\$)"; }

open_rows_list() { # bounded: up to 12 keys + total count
  local total=0 shown=""
  for f in "$DECISIONS_DIR"/[A-Z]*.yaml; do
    [ -e "$f" ] || continue
    if [ "$(field "$f" status)" = "open" ]; then
      total=$((total + 1))
      [ $total -le 12 ] && shown="${shown:+$shown }$(basename "$f" .yaml)"
    fi
  done
  printf '%s (of %s open rows)' "${shown:-none}" "$total"
}

if [ -z "$payload" ]; then
  block "empty-input" "register-check: empty/unreadable tool payload — failing closed."
fi
if [ -n "${REGISTER_CHECK_DECISIONS:-}" ] && { [ ! -d "$DECISIONS_DIR" ] || ! ls "$DECISIONS_DIR"/[A-Z]*.yaml >/dev/null 2>&1; }; then
  block "override-broken" "register-check: REGISTER_CHECK_DECISIONS points at a missing/empty directory — failing closed rather than silently skipping the row check."
fi

# ── Lane 1: the envelope ────────────────────────────────────────────────────────────────────────
env_key=""
env_lines="$(printf '%s' "$payload" | grep -oE "Decision row:[^\"\\\\]*" | sed 's/[[:space:]]*$//')"
env_count=0
[ -n "$env_lines" ] && env_count=$(printf '%s\n' "$env_lines" | wc -l)
if [ "$env_count" -gt 1 ]; then
  extra="$(printf '%s' "$env_lines" | tr '\n' '|')"
  block "envelope-multiple" "register-check: $env_count \`Decision row:\` lines found ($extra) — a decision question references EXACTLY ONE declared row. Split into one question per row, or drop the extra line."
elif [ "$env_count" -eq 1 ]; then
  env_key="$(printf '%s' "$env_lines" | grep -oE "Decision row:[[:space:]]*$KEY_GRAMMAR" | sed 's/Decision row:[[:space:]]*//')"
  rest_ok=true
  # the token must end at a non-key character: 'Decision row: DEC-Xx' must not half-match
  if [ -n "$env_key" ]; then
    after="$(printf '%s' "$env_lines" | sed "s/^Decision row:[[:space:]]*$env_key//")"
    case "$after" in
      ""|[!A-Za-z0-9-]*) : ;;
      *) rest_ok=false ;;
    esac
  fi
  if [ -z "$env_key" ] || [ "$rest_ok" = false ]; then
    extra="$env_lines"
    block "envelope-garbled" "register-check: the line \`$env_lines\` carries no valid row key (grammar: [A-Z][A-Z0-9-]{2,63}, e.g. \`Decision row: CAPTAINNET-ZERO\`). Fix the key and re-issue."
    env_key=""
  elif [ -e "$DECISIONS_DIR/$env_key.yaml" ]; then
    f="$DECISIONS_DIR/$env_key.yaml"
    keys_hit="${keys_hit:+$keys_hit }$env_key"
    status="$(field "$f" status)"
    case "$status" in
      open)
        owner="$(field "$f" owner)"
        if [ "$owner" = "counsel" ] && ! printf '%s' "$payload" | grep -qiF "external action"; then
          block "key-counsel-owned" "register-check: row \`$env_key\` is OPEN but counsel-owned — no lens output or founder answer is legal advice or clearance (ADR-20260812-143619). A founder question about it asks for the EXTERNAL ACTION (engaging counsel), never the answer itself; if that is what you are asking, say 'external action' in the question and re-issue."
        fi
        ;;
      decided)
        block "key-decided" "register-check: \`Decision row: $env_key\` — that row is DECIDED ($(field "$f" decided), decided_by: $(field "$f" decided_by)). A decided row is not a question: report the citation instead. If the answer's premise has changed, that is a DECISION REVERSAL — declare a NEW open row with \`reconsiders: $env_key\` and the changed premise in its evidence (docs/decisions/README.md), then ask on the new row."
        ;;
      superseded)
        block "key-superseded" "register-check: row \`$env_key\` is SUPERSEDED by \`$(field "$f" superseded_by)\` (decided_by: $(field "$f" decided_by)) — address the successor row, not this one."
        ;;
      deferred)
        block "key-deferred" "register-check: row \`$env_key\` is DEFERRED until: $(field "$f" until). When that condition is satisfied, edit the row back to \`status: open\` + \`make generate\`, then ask; if the condition itself is wrong, declare a NEW row with \`reconsiders: $env_key\`."
        ;;
      withdrawn)
        block "key-withdrawn" "register-check: row \`$env_key\` is WITHDRAWN — $(field "$f" note) If the question is live again, declare a NEW row with \`reconsiders: $env_key\`."
        ;;
      *)
        block "key-unreadable" "register-check: row \`$env_key\` has unreadable status '$status' — fix docs/decisions/$env_key.yaml (make validate names the defect) before asking about it."
        ;;
    esac
  elif [ -f "$DECISIONS_DIR/_legacy.yaml" ] && grep -qE "^  - $env_key\$" "$DECISIONS_DIR/_legacy.yaml"; then
    block "key-legacy-ask" "register-check: \`$env_key\` is a LEGACY prose-only row — legacy is not a bypass, and a founder-facing question IS a migration trigger (docs/decisions/README.md). Migrate it IN THIS SAME CHANGE: (1) create docs/decisions/$env_key.yaml from the prose row (schema: README, with register anchor + verbatim evidence), (2) remove \`$env_key\` from _legacy.yaml, (3) run \`make generate\` — then re-issue this exact question; the gate re-reads the files live."
  else
    block "key-unknown" "register-check: \`Decision row: $env_key\` names no declared row and no legacy key — fix the spelling, or if this is a GENUINELY NEW question, declare it first: create docs/decisions/$env_key.yaml with status: open, question, owner, opened, register anchor and evidence (docs/decisions/README.md), run \`make generate\`, then re-issue. Open rows today: $(open_rows_list)."
  fi
fi

# ── Lane 2: the trail (non-decision interactions only — the envelope supersedes it) ─────────────
if [ -n "$payload" ] && [ "$env_count" -eq 0 ]; then
  if ! printf '%s' "$payload" | grep -qF "$MARKER"; then
    block "trail-missing" "register-check: this question carries no \`Decision row:\` envelope and no register-check trail."
  elif ! printf '%s' "$payload" | grep -qE "$RECORD_ID" && ! printf '%s' "$payload" | grep -qE "$NO_RECORD"; then
    block "trail-hollow" "register-check: the trail names no record id and no explicit negative — a bare marker is not a trail."
  fi
fi

# ── Lane 3: passive references (defense in depth; the envelope key is handled above) ────────────
if [ -d "$DECISIONS_DIR" ]; then
  for f in "$DECISIONS_DIR"/[A-Z]*.yaml; do
    [ -e "$f" ] || continue
    key="$(basename "$f" .yaml)"
    [ "$key" = "$env_key" ] && continue
    refs_key "$key" || continue
    keys_hit="${keys_hit:+$keys_hit }$key"
    status="$(field "$f" status)"
    case "$status" in
      open) : ;;
      decided)
        block "key-decided" "register-check: the question references register row \`$key\`, whose status is DECIDED ($(field "$f" decided) — decided_by: $(field "$f" decided_by)). A decided row is not a question: cite its record id instead of the row key, or declare a reversal row (\`reconsiders: $key\`)."
        ;;
      superseded)
        block "key-superseded" "register-check: row \`$key\` is SUPERSEDED by \`$(field "$f" superseded_by)\` — reference the successor, or cite the record id."
        ;;
      deferred)
        block "key-deferred" "register-check: row \`$key\` is DEFERRED until: $(field "$f" until) — cite its record id if you only mean it as context."
        ;;
      withdrawn)
        block "key-withdrawn" "register-check: row \`$key\` is WITHDRAWN — $(field "$f" note)"
        ;;
      *)
        block "key-unreadable" "register-check: row \`$key\` has unreadable status '$status' — fix docs/decisions/$key.yaml first."
        ;;
    esac
  done
  if [ -f "$DECISIONS_DIR/_legacy.yaml" ]; then
    while IFS= read -r lk; do
      [ "$lk" = "$env_key" ] && continue
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

Do the check, THEN ASK -- never drop the question. Two legitimate shapes:

A. A DECISION QUESTION (the answer would bind future work) carries the envelope:
     Decision row: <KEY>
   where docs/decisions/<KEY>.yaml is declared and OPEN. Genuinely new question -> declare the
   row first (docs/decisions/README.md); challenging a decided row -> a NEW row with
   `reconsiders: <OLD-KEY>`. The envelope IS the register check; no trail line needed.

B. A NON-decision interaction (clarifying an in-flight directive, an external-clock relay, a
   mechanical choice) carries the trail of docs/claude/sessions/workflow.md:
     Register check: <record id> (<date>, <status>) -- covers <X>, silent on <Y>
     Register check: no controlling record -- terms: <terms>; nearest: <record or none>
   Since 2026-08-21 the negative trail ASSERTS "this is not a decision question".
EOF
esac
exit 2
