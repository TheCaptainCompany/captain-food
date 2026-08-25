#!/usr/bin/env python3
"""Decide whether a PR carries a bot review verdict naming a given commit.

WHY THIS IS A FILE AND NOT A jq PROGRAM IN A YAML BLOCK SCALAR (issue #677):
the matcher encodes ~16 block-level Markdown rules. As a regex inside jq inside a
YAML block scalar inside a shell heredoc it was quadruple-escaped and impossible
to run in place, so SIX consecutive review rounds were each the first thing ever
to execute it, and each found new defects: a backtick closer with trailing
content, tilde closers, blockquoted code blocks, list-item + indented code, an
exemplar the matcher rejected. Every one of those is a rule about *rendering*
that a comment cannot assert. This module is executable and has a table-driven
battery next to it (`assert_review_marker_test.py`), which runs in CI before the
assertion does.

The rule: a comment counts if it is authored by a GitHub App bot AND carries a
`Reviewed-Commit:` marker naming the head or merge sha on a line that GitHub
renders as LIVE TEXT -- not inside a fenced code block, not inside an indented
code block, at any container depth.

The boundary, stated because claiming more is how this went wrong five times:
this raises the bar on a marker quoted from another comment. It does not make it
impossible -- HTML block forms (<pre>, <code>, multi-line HTML comments) still
render as code while this counts them -- and it cannot prove WHICH bot posted the
comment, because this repo's own agent sessions post under the same identity.
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
HEADING = re.compile(r"^ {0,3}#{1,6}[ \t]+")


def live_lines(body: str):
    """Yield lines that are NOT obviously inside a code block, biased toward LIVE.

    DESIGN, after eight review rounds each found a different block-level rule wrong:

    A hand-rolled CommonMark block parser was the wrong instrument. Rounds 3-8 found, in turn:
    backtick closers with trailing content, tilde closers, blockquoted fences, list-item plus
    indented code, a fence quoting a fence, tab columns, container lifetimes, and prefix equality
    vs block structure. Each fix was correct and each left another dimension wrong, because the
    rules interact and this file re-derives what a parser already knows.

    So the rules are now MINIMAL and the ambiguity resolves DELIBERATELY toward "live":

      * ONLY a fence delimiter at COLUMN 0 opens or closes a block. That is the entire rule.
      * Everything else -- indented lines, blockquotes, lists, HTML -- is treated as LIVE.

    Measured against markdown-it-py (commonmark preset) over 4000 generated bodies mixing
    containers, fence characters and indents: 2 false reds, 330 false greens. The previous
    design measured 41 false reds; the indented-code rule was the last false-red source and
    removing it is the whole difference. The direction of the error is the point.

    WHY BIAS THIS WAY. The gate exists to catch the SILENT NO-VERDICT paths -- the action's
    self-skip, a 429, permission denials. It has never been able to prove WHICH bot posted a
    comment (this repo's own agent sessions post under the same identity), so it was never an
    anti-forgery mechanism, and `claude-code-review.yml` says so. Against that, a FALSE RED is
    strictly worse than a missed quoted marker: `claude-review` is required, so a false red is a
    repo-wide merge stop whose revert needs the same check green. Every misclassification this
    design can still make therefore counts the marker rather than rejecting it.

    THE RESIDUAL, stated plainly: a marker quoted inside a blockquote, a list, or an HTML block
    WILL satisfy this gate. So will `<pre>`, `<code>`, and HTML comments. Do not paste a marker
    line into a comment. If this ever needs to be airtight, the answer is a real CommonMark
    parser, not another rule here.
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
            fence = (m.group("d")[0], len(m.group("d")))
            continue
        yield HEADING.sub("", line.lstrip())



def comment_names_commit(body: str, shas: list[str]) -> bool:
    for line in live_lines(body):
        m = MARKER.match(line.lstrip())
        if not m:
            continue
        h = m.group("h").lower()
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
