#!/usr/bin/env bash
# Captain.Food register-check gate (Claude Code PreToolUse hook on AskUserQuestion and Agent).
#
# TWO SURFACES, ONE SCRIPT. The gate is dispatched on `tool_name` in the hook payload:
#
#   * ASK (AskUserQuestion, or no tool_name at all -- the selftest's bare-payload form): the
#     original founder-facing gate. Lanes 1-3 below, unchanged.
#   * DISPATCH (Agent): the COORDINATOR's committing surface, added 2026-08-31 by
#     ADR-20260831-141500. A dispatch card is the coordinator's diff and it was ungated, while
#     every agent was gated on the ask -- nine failures in one session, four of them caught only
#     by the founder or a lens. Lane D below.
#
# WHY THE COORDINATOR NEEDED ONE AT ALL. `.claude/skills/decision-lookup/` already existed and was
# invoked ZERO times in the session that produced those nine. An un-invoked skill is prose with
# extra steps, so the enforcement must not depend on remembering -- the same argument that put the
# ask gate on a hook rather than in an agent block.
#
# WHY THIS EXISTS. Three founder directives of 2026-08-21, in order: "agents will no longer ask
# questions already answered" (ADR-20260821-010543 — the trail), the REG-2/REG-4 row gate
# (ADR-20260821-095957), and decision-ask-unregistered (ADR-20260821-103403 — the envelope): a
# founder-directed decision question must reference EXACTLY ONE declared register row, and that
# row must be OPEN. The failure all three gate was banked twice (ADR-20260818-210000 coordinator
# defect 2; DECISIONS.md §48 / PROP-20260819-110442). A fourth, 2026-08-28
# (ADR-20260828-120500 / #709): the TRAIL lane validated shape only, so a trail that self-cited a
# CLOSED status in the canonical `(<date>, <status>)` form still passed — closed below.
#
# THE LANES, in order:
#   1. ENVELOPE — a decision question carries one `Decision row: <KEY>` line. Exactly one token,
#      one key, whatever the line layout; the key must be DECLARED (docs/decisions/<KEY>.yaml)
#      and OPEN. A non-open row is
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
#      work? then it is a decision question and needs a row). Since 2026-08-28 a trail whose OWN
#      `(<date>, <status>)` clause self-declares a CLOSED status (decided/superseded/deferred/
#      withdrawn — the register's closed set) is refused too, citing it back: the trail is, in its
#      own words, an answered question. The escape is a `premise-changed: <what changed>` line in
#      the same trail — never a silent re-ask.
#   3. PASSIVE — any OTHER declared key referenced in the text is checked by status (defense in
#      depth); legacy keys mentioned as CONTEXT pass, logged (`key-legacy`) — ask-vs-cite is
#      distinguished by the envelope, not by the key.
#
# LANE D — the DISPATCH card (tool_name == Agent). Two questions decide whether this survives
# contact, and both are answered STRUCTURALLY rather than by a list:
#
#   THE DISCRIMINATOR: WHICH `Agent` CALLS ARE GATED. Not every Agent call is a dispatch card --
#   lens consults (`young`, `vernon`, `evans`, `beck`, ...), the `reviewer` pass and read-only
#   research travel through the SAME tool. Gating all of them makes the gate something to work
#   around; a hand-maintained exemption list of agent names is the shape this repo has retired
#   twice. So the discriminator is DERIVED FROM THE TARGET AGENT'S OWN DECLARATION: the gate fires
#   iff `.claude/agents/<subagent_type>.md` grants a WRITE tool (`Write`/`Edit`, substring, so
#   `MultiEdit`/`NotebookEdit` count) in its frontmatter `tools:` line. Today that is exactly
#   `architect`, `executor` and `generator`; the other thirteen agents declare `Read, Grep, Glob,
#   Bash` and pass untouched, logged `agent-advisory`. Nothing here enumerates those names --
#   granting an agent a write tool pulls it into the gate in the same commit, and revoking one
#   drops it, with no list to update. The rule reads: A CALL THAT CAN PRODUCE A DIFF CARRIES THE
#   TRAIL THAT LICENSES IT.
#
#   It FAILS CLOSED in all three unknowns: no `subagent_type`, no agent file (`general-purpose` is
#   the live case -- environment.md documents pasting a charter into it as the standard workaround
#   for an unregistered agent, and it holds the full tool set), or an agent file with no `tools:`
#   line (no declaration means the inherited set, which can write).
#
#   THE ESCAPE HATCH IS THE FAILURE MODE. A gate satisfiable by pasting a literal
#   `Register check: none` is theatre. So Lane D checks the trail's SHAPE in a way a bare marker
#   cannot satisfy: a POSITIVE trail must name a record id that RESOLVES TO A FILE ON DISK
#   (docs/adr/, docs/proposals/, docs/legal/, docs/status/), and a NEGATIVE trail must be the
#   explicit no-controlling-record form AND name the `terms:` searched. `Register check: none`
#   is neither and is refused; an invented `ADR-20260101-000000` is refused because it resolves
#   to nothing. That is strictly stronger than the ask surface's Lane 2, which accepts any
#   id-SHAPED token -- deliberately not back-ported here, because tightening the ask gate is a
#   separate change with its own blast radius.
#
#   WHAT LANE D DELIBERATELY DOES NOT DO: it does not run the envelope lane and does not run the
#   passive key check. On a founder QUESTION, naming a decided row means asking something already
#   answered; on a dispatch CARD, citing a decided record is exactly the behaviour being enforced.
#   Refusing a card for citing its own controlling record would invert the gate.
#
# WHAT IT PROVES — AND WHAT IT DOES NOT. It verifies envelope/trail presence and shape and row
# STATUS on the AskUserQuestion transport; it cannot prove a search happened, cannot classify a
# prose question that omits the envelope (the honest hole: misclassifying a decision question as
# a clarification is a prose dodge, caught by review, not by this gate), and cannot see questions
# travelling as free text. Row files are read at the point of need — never the generated index.
# ON LANE D the same honesty limit applies one level up, and it is the reason the skill exists
# beside the hook: A HOOK GATES A TOOL CALL. The coordinator's PROSE ANSWERS to the founder are
# not tool calls and cannot be blocked the way AskUserQuestion is — of the nine failures, the
# dispatch-shaped ones are now gated and the answer-shaped ones are not. Lane D proves a card
# CARRIES a resolvable trail; it cannot prove the trail is the RIGHT record, that it was read, or
# that the card's claims follow from it. `.claude/skills/coordinator-register-check/` carries the
# procedure for the ungateable half, and it is weaker on purpose-stated grounds — which is itself
# an argument for routing more coordinator→founder questions through AskUserQuestion, where the
# gate already bites.
# The Lane-2 status check trusts the trail's OWN prose (an ADR/PROP/journal citation has no
# machine-readable status file to read, unlike a docs/decisions/<KEY>.yaml row) — it cannot prove
# the cited status is current or that `premise-changed:` names a real change; that stays with
# review, the same honesty limit as the rest of this gate.
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
# ADR-20260828-120500 / #709: the register's own closed set (docs/decisions/README.md). `open` is
# the only status a TRAIL may cite and still ask; any other self-declared status means the cited
# record IS the answer, so citing it in the canonical `(<date>, <status>)` shape and asking anyway
# is the exact incident the ADR names (the round-5 call-sheet gap).
CLOSED_STATUS='decided|superseded|deferred|withdrawn'
PREMISE_MARKER='premise-changed:'

# ── Lane D constants (the dispatch surface) ─────────────────────────────────────────────────────
# Both dirs are overridable so the selftest can plant a HERMETIC corpus: the live agent roster and
# the live docs/ tree both change, and a case that depends on them is a case that rots.
AGENTS_DIR="${REGISTER_CHECK_AGENTS:-$ROOT/.claude/agents}"
DOCS_DIR="${REGISTER_CHECK_DOCS:-$ROOT/docs}"
# Lane D's id grammar adds BRIEF- (docs/legal/, docs/proposals/) to the ask surface's set: failure 4
# of the nine was composed without reading BRIEF-20260819 §4.2, so a brief must be citable as the
# record that governs. Every alternative here RESOLVES to a real path below -- an id shape that
# cannot resolve would be a hole in the anti-theatre check, not a convenience.
DISPATCH_RECORD_ID='ADR-[0-9]{8}-[0-9]{6}|ADR-00[0-9]{2}|PROP-[0-9]{8}-[0-9]{6}|BRIEF-[0-9]{8}|journal-[0-9]{4}-W[0-9]{2}|DECISIONS'

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

# Does a cited record id correspond to a file that actually exists? This is the anti-theatre half
# of Lane D: it makes the trail's SHAPE checkable without pretending to check its TRUTH. An
# invented id resolves to nothing and is refused; a real id that is the WRONG record resolves and
# passes, and catching that stays with review, like every other honesty claim in this gate.
resolve_record() { # resolve_record <id> -> 0 iff a record file on disk carries that id
  case "$1" in
    ADR-*)     set -- "$DOCS_DIR/adr/$1"*.md ;;
    PROP-*)    set -- "$DOCS_DIR/proposals/$1"*.md ;;
    BRIEF-*)   set -- "$DOCS_DIR/legal/$1"*.md "$DOCS_DIR/proposals/$1"*.md ;;
    journal-*) set -- "$DOCS_DIR/status/$1"*.md ;;
    DECISIONS) set -- "$DOCS_DIR/proposals/DECISIONS.md" ;;
    *) return 1 ;;
  esac
  for _p in "$@"; do [ -e "$_p" ] && return 0; done
  return 1
}

# ── Lane D: the DISPATCH card (PreToolUse on the Agent tool) ────────────────────────────────────
# Dispatched on tool_name. An ABSENT tool_name stays on the ask surface on purpose: that is the
# selftest's bare-payload form and every pre-existing case uses it, so the ask gate's contract is
# untouched by this lane's arrival.
TOOL_NAME="$(printf '%s' "$payload" | grep -o '"tool_name":"[^"]*"' | head -1 | sed 's/.*:"//;s/"$//')"

if [ -n "$payload" ] && [ "$TOOL_NAME" = "Agent" ]; then
  sub="$(printf '%s' "$payload" | grep -o '"subagent_type":"[^"]*"' | head -1 | sed 's/.*:"//;s/"$//')"
  agent_file="$AGENTS_DIR/$sub.md"
  extra="agent=${sub:-<none>}"
  gated=yes
  why=""
  if [ -z "$sub" ]; then
    why="the call names no \`subagent_type\`"
  elif [ ! -f "$agent_file" ]; then
    why="no \`.claude/agents/$sub.md\` declares this agent, so its tool set cannot be read (an undeclared agent — \`general-purpose\` is the live case — holds the full set, including Write/Edit)"
  else
    tools_line="$(awk '/^---$/{c++; next} c==1 && /^tools:/{print; exit}' "$agent_file")"
    if [ -z "$tools_line" ]; then
      why="\`$sub.md\` declares no \`tools:\` line, so it inherits the full tool set"
    elif printf '%s' "$tools_line" | grep -qE 'Write|Edit'; then
      why="\`$sub\` is write-capable (\`$tools_line\`) — this call can produce a diff"
    else
      gated=no
    fi
  fi

  # An advisory (read-only) target is NOT a dispatch card: a lens consult, a reviewer pass or
  # read-only research commits nothing, so requiring a trail there would train the gate away.
  if [ "$gated" = no ]; then
    note ALLOW "agent-advisory" "-"
    exit 0
  fi

  trail_ok=no
  saw_id=no
  saw_neg=no
  saw_marker=no
  while IFS= read -r dline; do
    [ -n "$dline" ] || continue
    saw_marker=yes
    if printf '%s' "$dline" | grep -qE "$NO_RECORD"; then
      saw_neg=yes
      # The negative is a PASSING trail — but only when it names what was searched, otherwise
      # "no controlling record" is the same free pass as `Register check: none`.
      if printf '%s' "$dline" | grep -qF 'terms:'; then trail_ok=yes; break; fi
      continue
    fi
    dids="$(printf '%s' "$dline" | grep -oE "$DISPATCH_RECORD_ID" || true)"
    [ -n "$dids" ] && saw_id=yes
    for did in $dids; do
      if resolve_record "$did"; then trail_ok=yes; break 2; fi
    done
  done <<EOF
$(printf '%s' "$payload" | grep -oE "${MARKER}[^\"\\\\]*")
EOF

  if [ "$trail_ok" = yes ]; then
    note ALLOW "dispatch-trail-ok" "-"
    exit 0
  fi

  if [ "$saw_marker" = no ]; then
    block "dispatch-trail-missing" "register-check: this dispatch carries no \`Register check:\` trail, and it is GATED because $why."
  elif [ "$saw_neg" = yes ]; then
    block "dispatch-trail-termless" "register-check: the trail claims no controlling record but names no \`terms:\` — an unsearchable negative is the \`Register check: none\` free pass under a longer name. Say what you searched."
  elif [ "$saw_id" = yes ]; then
    block "dispatch-trail-unresolved" "register-check: the trail names a record id, but none of the ids in it resolve to a file under docs/ (adr, proposals, legal, status). A citation that resolves to nothing is not a citation — fix the id, or state the explicit negative with its terms."
  else
    block "dispatch-trail-hollow" "register-check: the trail names no record id and no explicit negative — a bare \`Register check:\` marker is not a trail."
  fi

  note BLOCK "$reasons" "-"
  printf '%s' "$block_msgs" >&2
  cat >&2 <<'EOF'

WHY THIS FIRED. A dispatch card is the coordinator's DIFF, and a call that can produce a diff
carries the trail that licenses it (ADR-20260831-141500). Lens consults, the `reviewer` pass and
read-only research are NOT gated -- the discriminator is the target agent's own `tools:` line, so
only write-capable agents reach this message.

Do the check, THEN DISPATCH -- never drop the card. Two legitimate shapes, one per claim:

  Register check: <record id> (<date>, <status>) -- covers <X>, silent on <Y>
  Register check: no controlling record -- terms: <terms searched>; nearest: <record id or none>

The record id must RESOLVE to a file under docs/adr, docs/proposals, docs/legal or docs/status,
and the negative must name its `terms:`. Procedure and worked examples:
.claude/skills/coordinator-register-check/SKILL.md -- run `decision-lookup` for candidates, then
READ the candidate itself (it is advisory, never evidence).
EOF
  exit 2
fi
if [ -n "${REGISTER_CHECK_DECISIONS:-}" ] && { [ ! -d "$DECISIONS_DIR" ] || ! ls "$DECISIONS_DIR"/[A-Z]*.yaml >/dev/null 2>&1; }; then
  block "override-broken" "register-check: REGISTER_CHECK_DECISIONS points at a missing/empty directory — failing closed rather than silently skipping the row check."
fi

# ── Lane 1: the envelope ────────────────────────────────────────────────────────────────────────
# env_count counts TOKEN occurrences, not extracted lines: two `Decision row:` tokens on ONE line
# collapse into a single greedy match, which under a line count reached the single-envelope path
# with a two-key string and garbled the block message (PR #669 review, F2). Any payload carrying
# more than one token is envelope-multiple, whatever the line layout.
env_key=""
env_lines="$(printf '%s' "$payload" | grep -oE "Decision row:[^\"\\\\]*" | sed 's/[[:space:]]*$//')"
env_count="$(printf '%s' "$payload" | grep -oF "$ENVELOPE" | wc -l)"
if [ "$env_count" -gt 1 ]; then
  extra="$(printf '%s' "$env_lines" | tr '\n' '|')"
  block "envelope-multiple" "register-check: $env_count \`Decision row:\` tokens found ($extra) — a decision question references EXACTLY ONE declared row. Split into one question per row, or drop the extra token."
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
  else
    # Shape is fine; the STATUS the trail itself reports is not yet checked. A trail written in
    # the canonical `(<date>, <status>)` shape that self-declares a CLOSED status is, by its own
    # words, citing an answer — asking anyway is the redundant re-ask ADR-20260828-120500 names.
    # This never opens a file: the trail's own prose is the only oracle a free-text citation (an
    # ADR/PROP/journal id, not a docs/decisions/<KEY>.yaml row -- those go through the envelope
    # and Lane 3 already) has, so trusting it is the honest floor, same limit the top-of-file
    # comment states for the rest of this gate.
    trail_line="$(printf '%s' "$payload" | grep -oE "${MARKER}[^\"\\\\]*" | head -1)"
    trail_status="$(printf '%s' "$trail_line" | grep -oE "\([^()]*,[[:space:]]*($CLOSED_STATUS)\)" | tail -1 | sed 's/.*,[[:space:]]*//; s/)$//')"
    if [ -n "$trail_status" ]; then
      trail_record="$(printf '%s' "$trail_line" | grep -oE "$RECORD_ID" | head -1)"
      keys_hit="${keys_hit:+$keys_hit }${trail_record:-?}"
      if printf '%s' "$payload" | grep -qF "$PREMISE_MARKER"; then
        # The escape hatch (workflow.md trail-format section): the answer exists but the premise
        # that produced it has changed. Never proved here -- honesty stays with review, like every
        # other trail claim -- but LOGGED distinctly so a hollow "premise-changed: (blank)" is a
        # decomposable defect, not invisible inside a plain ALLOW.
        reasons="${reasons:+$reasons,}trail-premise-changed"
      else
        block "trail-answered" "register-check: the trail cites \`${trail_record:-that record}\` as $trail_status (\"$trail_line\") — a $trail_status record is not a question: report the citation instead. If the answer's premise has changed, add a \`premise-changed: <what changed and why the old answer no longer holds>\` line to the trail and re-issue."
      fi
    fi
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
