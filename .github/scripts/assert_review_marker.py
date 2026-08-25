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

MARKER = re.compile(
    r"""^[*_`]*Reviewed-Commit[*_`]*:?[*_`]*[ \t]*`?(?P<h>[0-9a-fA-F]{7,40})`?\b""",
)

# A container prefix GitHub strips before block parsing: blockquote markers and
# list-item markers. Stripped ONE level at a time so `> > ` and `- > ` work, and
# so the indented-code rule is applied to what remains -- the bug that let
# `>     marker` and `-     marker` through was applying the two rules to
# different strings.
#
# EXACTLY ONE space after the marker, not the whole run: `-     marker` is how
# CommonMark spells "this list item's content is an indented code block", so
# eating all five spaces here would render the indent test blind. The battery
# caught this on its first execution -- which is the argument for the battery.
CONTAINER = re.compile(r"^ {0,3}(?:>[ \t]?|(?:[-*+]|[0-9]{1,9}[.)])[ \t])")
FENCE = re.compile(r"^ {0,3}(?P<d>`{3,}|~{3,})(?P<rest>.*)$")
HEADING = re.compile(r"^ {0,3}#{1,6}[ \t]+")


def _strip_containers(line: str) -> str:
    """Remove blockquote/list prefixes, innermost-last, as a block parser would."""
    prev = None
    while prev != line:
        prev = line
        line = CONTAINER.sub("", line, count=1)
    return line


def live_lines(body: str):
    """Yield each line of `body` that GitHub renders as live text.

    Skips fenced code blocks (``` and ~~~, respecting delimiter char, run length
    and the rule that a CLOSER may carry only whitespace after it -- for BOTH
    fence characters, which is what CommonMark says) and indented code blocks.
    """
    fence: tuple[str, int] | None = None
    for raw in body.split("\n"):
        line = raw.rstrip("\r")
        stripped = _strip_containers(line)
        m = FENCE.match(stripped)
        if m:
            char, run = m.group("d")[0], len(m.group("d"))
            if fence is None:
                # An opener may carry an info string; a closer may not.
                fence = (char, run)
                continue
            open_char, open_run = fence
            if char == open_char and run >= open_run and m.group("rest").strip() == "":
                fence = None
            continue
        if fence is not None:
            continue
        # Indented code: 4+ spaces AFTER container stripping. A heading or a
        # paragraph line can never be indented code, but we do not need that
        # nuance -- erring toward "code" only ever costs a false red, and the
        # prompt asks for the marker at the left margin.
        if stripped.startswith("    ") or stripped.startswith("\t"):
            continue
        yield HEADING.sub("", stripped)


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
