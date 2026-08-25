#!/usr/bin/env python3
"""Decide whether a PR carries a bot review verdict naming a given commit.

WHY THIS IS A FILE AND NOT A jq PROGRAM IN A YAML BLOCK SCALAR (issue #677):
as a regex inside jq inside a YAML block scalar inside a shell heredoc it was
quadruple-escaped and impossible to run in place, so six consecutive review
rounds were each the first thing ever to execute it, and each found new defects:
a backtick closer with trailing content, tilde closers, blockquoted code blocks,
list-item + indented code, an exemplar the matcher rejected. Every one of those
is a rule about *rendering* that a comment cannot assert. This module is
executable and has a table-driven battery next to it
(`assert_review_marker_test.py`), which runs in CI before the assertion does.

THE RULE, and it is deliberately ONE rule: a comment counts if it is authored by
a GitHub App bot AND carries a `Reviewed-Commit:` marker naming the head or merge
sha on a line that is not inside a fenced code block opened by a delimiter at
COLUMN 0. See `live_lines` for why every other block rule was deleted in round 8
rather than fixed.

The boundary, stated because claiming more is how this went wrong five times:
this raises the bar on a marker quoted from another comment. It does not make it
impossible. INDENTED code blocks, fences opened inside a blockquote or a list,
`<pre>`, `<code>` and multi-line HTML comments all render as code while this
counts them -- that is the accepted direction of error, not an oversight.

INDENTATION IS NOT UNIFORMLY THAT DIRECTION, though, and saying so was wrong:
an indented FENCE OPENER (legal CommonMark at 1-3 columns) errs the OTHER way,
into a live false red, because this only tracks column-0 delimiters and reads the
opener's column-0 closer as a fresh opener. See the residual below -- review #11
found that shape sitting in the committed baseline while this paragraph implied
indentation could only cost a false green.

Nor can it prove WHICH bot posted the comment, because this repo's own agent
sessions post under the same identity.
"""
from __future__ import annotations

import json
import re
import sys

# Leading blockquote/list/heading markers are accepted inline: they all render LIVE, and with the
# container-stripping pass gone (see live_lines) the marker regex is where they are tolerated.
MARKER = re.compile(
    r"""^(?:(?:[-*+]|[0-9]{1,9}[.)]|\#{1,6}|>)[ \t]*)*"""
    r"""[*_`]*Reviewed-Commit[*_`]*:?[*_`]*[ \t]*`?(?P<h>[0-9a-fA-F]{7,40})`?\b""",
)

FENCE_AT_COL0 = re.compile(r"^(?P<d>`{3,}|~{3,})(?P<rest>.*)$")
# (There was a HEADING regex here, stripped from every yielded line. It was DEAD: MARKER's own
# prefix alternation already accepts `#{1,6}`, so review #10's "drop the HEADING.sub" mutant
# survived the whole battery. Deleting a line no test can distinguish beats commenting that it
# matters -- CLAUDE.md's compiler-first rule, applied downward.)


def live_lines(body: str):
    """Yield lines that are NOT obviously inside a code block, biased toward LIVE.

    DESIGN, after nine review rounds each found a different block-level rule wrong:

    A hand-rolled CommonMark block parser was the wrong instrument. Rounds 3-9 found, in turn:
    backtick closers with trailing content, tilde closers, blockquoted fences, list-item plus
    indented code, a fence quoting a fence, tab columns, container lifetimes, and prefix equality
    vs block structure. Each fix was correct and each left another dimension wrong, because the
    rules interact and this file re-derives what a parser already knows.

    So the rules are now MINIMAL and the ambiguity resolves DELIBERATELY toward "live":

      * ONLY a fence delimiter at COLUMN 0 opens or closes a block. That is the entire rule.
      * Everything else -- indented lines, blockquotes, lists, HTML -- is treated as LIVE.

    The bias is MEASURED, not asserted: `assert_review_marker_differential.py` classifies a
    deterministic corpus against markdown-it-py and prints its own antecedents (corpus seed,
    body count, parser version) so the figure is reproducible rather than quoted, and ratchets a
    committed per-seed baseline. Run it; do not trust a number pasted here.

    THIS PARAGRAPH USED TO END WITH ONE ANYWAY -- "false reds collapsed from dozens to at most one,
    and the indented-code rule was the last false-red source" -- and review #10 refuted both halves
    from output the same commit printed: false reds ran 0-4 across seeds, and the harness's own
    "first false red" was a column-0 opener with an indented CLOSER, not indented code. Twenty
    lines under "do not trust a number pasted here". The DIRECTION of the error is the point; the
    magnitude lives in the harness, which is the only thing that can keep it true.

    WHY BIAS THIS WAY, stated as the property rather than the blast radius. The gate exists to
    catch the SILENT NO-VERDICT paths -- the action's self-skip, a 429, a model outage, permission
    denials. EVERY ONE of those ends with no marker anywhere in the thread, so biasing toward
    counting cannot weaken what the gate was built to catch. That is the whole argument, and it is
    checkable.

    THE DIRECTION HAS A DECLARED OPEN ROW: docs/decisions/REVIEW-MARKER-BIAS.yaml. It was settled
    here by a code comment first and registered afterwards, which is the defect
    docs/decisions/REVIEW-GATE-BYPASS.yaml exists to retire -- named so the next reader finds the
    option space instead of re-deriving it from this docstring.

    IT IS NOT the argument three earlier versions of this file made. They said a false red is "a
    repo-wide merge stop whose revert needs the same check green". That is false, and review #9
    said so: a MATCHER false red blocks the one PR whose comment tripped it and clears by
    re-posting the comment; the repo-wide stop is the credit/outage case, which is a TRUE red; and
    an admin bypass exists (docs/decisions/REVIEW-GATE-BYPASS.yaml). The direction survives the
    correction -- a missed quoted marker costs a property this gate could never deliver, since it
    cannot prove WHICH bot posted -- but the reason had to be repaired, not the conclusion.

    THE RESIDUAL, stated plainly and in BOTH directions.

    Counts but renders as code (accepted): a marker quoted inside an INDENTED code block, inside a
    fence opened at any column other than 0, inside a blockquote, a list, or an HTML block. So will
    `<pre>`, `<code>`, and HTML comments. Do not paste a marker line into a comment.

    Renders live but does NOT count (the false-red side). State it as a CLASS, because the
    previous version enumerated "two shapes" and review #10 found five -- two of them already
    pinned as cases in this repo's own battery, which is an enumeration refuted by the file next
    to it:

      * THE MARKER MUST START ITS LINE, after at most the prefixes MARKER accepts (blockquote,
        bullet, ordered marker, heading, emphasis). Prose before it, a task-list box `- [x] `, or
        a GFM table cell's leading pipe all render live and are all missed.
      * ANY disagreement about which delimiter opens and which closes a fence, once indentation
        is in play. This matcher only ever sees column-0 delimiters, so both directions fail:
        a column-0 opener closed by a delimiter indented 1-3 spaces (the closer is invisible, the
        fence runs to end of comment), AND an opener indented 1-3 spaces -- legal in CommonMark --
        whose column-0 closer this reads as a fresh OPENER, with the same result. Review #11 found
        the second shape is the one actually sitting in the committed baseline, while this
        paragraph documented only the first and claimed the baseline tracked it. Both are now
        battery cases, so the accepted residual is a fixture rather than a sentence.

    The operator hint in claude-code-review.yml prints the class, because a red the operator
    cannot diagnose is a red they will work around.

    If this ever needs to be airtight, the answer is a real CommonMark parser, not another rule
    here.
    """
    fence: tuple[str, int] | None = None  # (delimiter char, run length) -- column 0 only
    for raw in body.split("\n"):
        line = raw.rstrip("\r")
        m = FENCE_AT_COL0.match(line)
        if fence is not None:
            if m:
                char, run = m.group("d")[0], len(m.group("d"))
                if char == fence[0] and run >= fence[1] and m.group("rest").strip() == "":
                    fence = None
            continue
        if m:
            char, run = m.group("d")[0], len(m.group("d"))
            # A BACKTICK fence's info string MAY NOT CONTAIN A BACKTICK (CommonMark 4.5). So
            # ```make validate``` is green. at column 0 is a paragraph carrying a code span, not
            # a fence opener -- and reviewers in this repo start lines that way constantly.
            # Review #10 measured it as a LIVE FALSE RED: the phantom fence ran to end of comment
            # and swallowed the marker. It is the same mechanism as the `{1,} mutant two cases in
            # the battery already kill, surviving at `{3,}. Tildes carry no such restriction.
            if not (char == "`" and "`" in m.group("rest")):
                fence = (char, run)
                continue
        # NOT `line.lstrip()`: both consumers lstrip again, so stripping here is a no-op whose
        # removal no test can distinguish (review #11). Same rule that deleted `HEADING`.
        yield line



def comment_names_commit(body: str, shas: list[str]) -> bool:
    for line in live_lines(body):
        m = MARKER.match(line.lstrip())
        if not m:
            continue
        h = m.group("h").lower()
        # BOTH sides are lowered. The comment side has a battery case ("uppercase hex, real sha");
        # the ARGUMENT side did not, so `s.lower()` -> `s` survived review #10's mutation run.
        # GitHub writes shas lowercase, but nothing here guarantees the caller does.
        if any(s.lower().startswith(h) for s in shas if s):
            return True
    return False


def parse_comment_stream(raw: str) -> list:
    """Parse what `gh api --paginate` writes, WITHOUT relying on how it joins pages.

    Some gh versions merge top-level arrays across pages into one array; others emit one array
    per page, back to back. A plain `json.load` handles the first and raises "Extra data" on the
    second -- which would be a FALSE RED on every PR with more than one page of comments, the
    exact defect class this gate has already shipped twice (the per-page `| length`, and the
    SIGPIPE). Accepting both shapes costs four lines and removes the question.
    """
    raw = raw.strip()
    if not raw:
        return []
    decoder = json.JSONDecoder()
    out: list = []
    idx = 0
    while idx < len(raw):
        value, end = decoder.raw_decode(raw, idx)
        if not isinstance(value, list):
            raise ValueError(f"expected a JSON array of comments, got {type(value).__name__}")
        out.extend(value)
        idx = end
        while idx < len(raw) and raw[idx] in " \t\r\n":
            idx += 1
    return out


def count_matches(comments, shas: list[str]) -> int:
    n = 0
    for c in comments:
        if (c.get("user") or {}).get("type") != "Bot":
            continue
        if comment_names_commit(c.get("body") or "", shas):
            n += 1
    return n


def main() -> int:
    # Validate POSITIONALLY. An earlier version filtered empties out of argv first, so an empty
    # HEAD_SHA was silently dropped and the gate degraded to matching the merge sha alone -- a
    # vacuous pass shape introduced by this very extraction, and caught by the battery below.
    # GitHub sets these variables to empty rather than unsetting them, so `set -u` never sees it.
    args = sys.argv[1:]
    if not args:
        print("usage: assert_review_marker.py <head-sha> [merge-sha]", file=sys.stderr)
        return 2
    if len(args) > 2:
        print(f"::error::expected at most 2 shas (head, merge), got {len(args)} - "
              f"an unvalidated extra sha would be matched by prefix like the others.",
              file=sys.stderr)
        return 2
    # EVERY argument, not just the first two. `zip` stopped at the shorter sequence while
    # `shas = list(args)` took them all, so a third argument was matched unvalidated (review #10).
    # The arity check above bounds `args` to two, which makes a third name here DEAD -- review #11
    # measured that mutant as surviving the battery and it is equivalent by construction, not a
    # coverage hole. Deleted rather than pinned, same rule this file applied to `HEADING`.
    for name, value in zip(("head", "merge"), args):
        if not re.fullmatch(r"[0-9a-fA-F]{40}", value or ""):
            print(f"::error::the {name} sha is empty or not 40 hex chars (got {value!r}) - "
                  f"refusing to assert against a marker that anything could satisfy.",
                  file=sys.stderr)
            return 1
    shas = list(args)
    raw = sys.stdin.read()
    try:
        comments = parse_comment_stream(raw)
    except Exception as exc:  # a malformed/empty body must never read as "no review"
        print(f"::error::could not parse the comment list: {exc}", file=sys.stderr)
        return 1
    print(count_matches(comments, shas))
    return 0


if __name__ == "__main__":
    sys.exit(main())
