#!/usr/bin/env bash
# Hermetic selftest for tools/link-check.py.
#
# WHY IT EXISTS. A link checker that matches nothing passes: it reports zero broken links over
# zero links and exits 0, confidently and meaninglessly. That is not hypothetical here -- it
# happened twice in one session, once inside a coordinator's own verification. So the checker
# carries three vacuity guards, and a guard nobody has watched fail is an unverified claim. Every
# case below is a FIXTURE the checker is run against, with the exit status asserted; the
# guard-cases assert the RED, not the green.
#
# It is also the regression suite for the four false-positive classes that were found while the
# checker was being written, each of which had exactly one "fix" available to a reader who did not
# know better: damage a correct document. Those are T8-T12.
#
# Hermetic: temporary trees under $TMPDIR, no network, no git writes, nothing outside the tempdir.
# The fixtures are NOT git repositories on purpose -- that exercises the checker's non-git
# derivation fallback as a side effect of every case.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECKER="$HERE/link-check.py"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

pass=0; fail=0
# COMPLETENESS, the same property the repo's other gate suites assert: every declared case must
# reach a verdict. A case that stops running at all is the failure this number exists to catch --
# a suite that silently drops half its cases still prints "all passed".
EXPECTED_CASES=13

ok()   { pass=$((pass+1)); printf '  PASS  %s\n' "$1"; }
bad()  { fail=$((fail+1)); printf '  FAIL  %s\n     -> %s\n' "$1" "$2"; }

# run <fixture-dir> [extra args...] -> sets RC and OUT
run() {
  local dir="$1"; shift
  OUT="$(python3 "$CHECKER" --root "$dir" "$@" 2>&1)"
  RC=$?
}

# expect_rc <name> <want-rc> <fixture> [args...]
expect_rc() {
  local name="$1" want="$2" dir="$3"; shift 3
  run "$dir" "$@"
  if [ "$RC" -eq "$want" ]; then ok "$name"; else bad "$name" "expected exit $want, got $RC. Output: $OUT"; fi
}

# expect_says <name> <want-rc> <needle> <fixture> [args...]
expect_says() {
  local name="$1" want="$2" needle="$3" dir="$4"; shift 4
  run "$dir" "$@"
  if [ "$RC" -eq "$want" ] && printf '%s' "$OUT" | grep -qF -- "$needle"; then
    ok "$name"
  else
    bad "$name" "expected exit $want and text '$needle'; got exit $RC. Output: $OUT"
  fi
}

mk() { mkdir -p "$(dirname "$1")"; cat > "$1"; }

echo "link-check selftest"

# ── T1: a resolvable relative link is GREEN ────────────────────────────────────────────────────
D="$TMP/t1"; mk "$D/a.md" <<'EOF'
See [b](b.md).
EOF
mk "$D/b.md" <<'EOF'
# B
EOF
expect_rc "T1  a resolvable relative link passes" 0 "$D"

# ── T2: THE PLANTED RED. A dangling path must FAIL, and must name file and line ────────────────
D="$TMP/t2"; mk "$D/a.md" <<'EOF'
See [gone](nope.md).
EOF
expect_says "T2  a dangling relative path reds" 1 "a.md:1" "$D"

# ── T3: the `ADR-` PREFIX TRAP, the class this checker was written for. A pre-2026-07-22 ADR has
# no `ADR-` prefix while every citation uses one, so the link resolves to nothing and GitHub
# renders it dead with no error. It must red AND name the near-miss, or the reader cannot tell
# which of the two spellings is the real file.
D="$TMP/t3"; mk "$D/adr/0004-commands-derived-from-use-cases.md" <<'EOF'
# 0004
EOF
mk "$D/cite.md" <<'EOF'
As decided in [ADR-0004](adr/ADR-0004-commands-derived-from-use-cases.md).
EOF
expect_says "T3  the ADR- prefix trap reds and suggests the real file" 1 "did you mean" "$D"

# ── T4: a fragment that exists is GREEN; T5: one that does not is RED ──────────────────────────
D="$TMP/t4"; mk "$D/a.md" <<'EOF'
Jump to [there](b.md#the-section-title) and [here](#local-heading).

## Local heading
EOF
mk "$D/b.md" <<'EOF'
## The Section Title
EOF
expect_rc "T4  a fragment that resolves passes" 0 "$D"

D="$TMP/t5"; mk "$D/a.md" <<'EOF'
Jump to [nowhere](b.md#no-such-heading).
EOF
mk "$D/b.md" <<'EOF'
## Something else
EOF
expect_says "T5  a fragment with no heading reds" 1 "no heading anchor" "$D"

# ── T6: VACUITY GUARD 1 -- an EMPTY CORPUS IS A FAILURE, not a pass ────────────────────────────
D="$TMP/t6"; mkdir -p "$D"
expect_says "T6  an empty corpus reds (vacuity guard)" 2 "corpus is EMPTY" "$D"

# ── T7: VACUITY GUARD 2 -- a real corpus from which ZERO links are extracted is a broken
# extractor, not a clean bill of health. This is the guard that survives T6: the files are there
# and are read, and the scan still means nothing.
D="$TMP/t7"; mk "$D/a.md" <<'EOF'
# A document with prose and https://example.com/external but no relative links at all.
EOF
expect_says "T7  a corpus yielding zero links reds (vacuity guard)" 2 "ZERO relative links" "$D"

# ── T8: VACUITY GUARD 3 -- the repo-root run must actually be looking at THIS repository.
# Non-emptiness alone is satisfied by a `git` that prints one junk path, so the guard requires
# CLAUDE.md. Exercised by copying the checker into a tree that has markdown but no CLAUDE.md and
# running it with NO --root, which is the only mode the guard applies to.
D="$TMP/t8"; mkdir -p "$D/tools"
cp "$CHECKER" "$D/tools/link-check.py"
mk "$D/doc.md" <<'EOF'
A [link](doc.md) to itself.
EOF
OUT="$(cd "$D" && python3 tools/link-check.py 2>&1)"; RC=$?
if [ "$RC" -eq 2 ] && printf '%s' "$OUT" | grep -qF "does not contain \`CLAUDE.md\`"; then
  ok "T8  a corpus that is not this repository reds (vacuity guard)"
else
  bad "T8  a corpus that is not this repository reds (vacuity guard)" "expected exit 2 and the CLAUDE.md message; got exit $RC. Output: $OUT"
fi

# ── T9: EXTERNAL URLS ARE OUT OF SCOPE, deliberately and testably. A blocking gate must not
# depend on a third party's uptime or rate limiter. If someone later wires network checking in,
# this case reds and the decision gets made again on purpose.
D="$TMP/t9"; mk "$D/a.md" <<'EOF'
[live](https://example.com/definitely/not/real) · [mail](mailto:x@y.z) · [self](a.md)
EOF
expect_rc "T9  external URLs are not checked" 0 "$D"

# ── T10: a link inside a FENCED code block is not a link ───────────────────────────────────────
D="$TMP/t10"; mk "$D/a.md" <<'EOF'
Real: [self](a.md)

```markdown
[example](does-not-exist.md)
```
EOF
expect_rc "T10 a link in a fenced code block is ignored" 0 "$D"

# ── T11: a link inside an INDENTED code block is not a link. `docs/STATUS.md` shows the template
# header for a new weekly journal file this way, and it contains `../STATUS.md` -- correct from
# `docs/status/`, dangling from `docs/STATUS.md`. Reporting it would be a false positive whose
# only "fix" is damaging a correct document.
D="$TMP/t11"; mk "$D/a.md" <<'EOF'
Real: [self](a.md)

    # A template for another file
    Current state: [`../STATUS.md`](../STATUS.md).

Back to prose.
EOF
expect_rc "T11 a link in an indented code block is ignored" 0 "$D"

# ── T12: ...AND THE CONVERSE, which is the half that matters. This repo indents continuation
# paragraphs under list items constantly and those DO contain live links. If the indented-code
# rule of T11 is written naively it swallows them, and the corpus loses real links with no
# assertion able to notice -- vacuity by a quieter door than T6. This case must RED.
D="$TMP/t12"; mk "$D/a.md" <<'EOF'
- A list item.

  A continuation paragraph under it, linking to [nothing](does-not-exist.md).
EOF
expect_says "T12 a link in a list continuation is still checked" 1 "does-not-exist.md" "$D"

# ── T13: a FOOTNOTE definition is not a link reference definition. `[^27]: some prose` has a
# prose body, not a target; reading it as one produced 20 false reds in a single research dump,
# i.e. the majority of the first measured finding was the instrument itself.
D="$TMP/t13"; mk "$D/a.md" <<'EOF'
Prose with a footnote.[^1] And a real link: [self](a.md)

[^1]: 20260627-ADR-019 Some Document Title.md
EOF
expect_rc "T13 a footnote definition is not read as a link" 0 "$D"

echo
total=$((pass+fail))
if [ "$total" -ne "$EXPECTED_CASES" ]; then
  echo "link-check selftest: FAIL — $total case(s) reached a verdict, expected $EXPECTED_CASES."
  echo "  A case stopped running rather than failing. That is the defect this count exists to"
  echo "  catch: a suite that silently drops cases still prints a green summary for the rest."
  exit 1
fi
if [ "$fail" -ne 0 ]; then
  echo "link-check selftest: FAIL — $fail of $total case(s) failed."
  exit 1
fi
echo "link-check selftest: OK — $pass/$EXPECTED_CASES cases passed."
