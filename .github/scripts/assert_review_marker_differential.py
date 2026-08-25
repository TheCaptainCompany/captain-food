#!/usr/bin/env python3
"""Differential harness: this matcher vs a real CommonMark implementation.

The table-driven battery is a REGRESSION NET, not a specification -- eight review rounds proved
that, each finding block-level rules the previous round's cases could not see. This harness is the
predictive check: it generates bodies mixing containers, fence characters and indents, renders each
with markdown-it-py, and reports where the matcher disagrees with what GitHub would show.

It is NOT run in CI: it needs a pip dependency, and adding one to a security-adjacent gate is a
decision nobody has taken. Run it by hand whenever `live_lines` changes:

    pip install markdown-it-py && python3 .github/scripts/assert_review_marker_differential.py

WHAT THE NUMBERS MEAN. A FALSE RED (renders live, gate rejects) is the expensive direction:
`claude-review` is required, so it is a repo-wide merge stop whose revert needs the same check
green. A false green (renders as code, gate counts) costs the anti-quoting property -- which this
gate has never been able to deliver anyway, since it cannot prove which bot posted a comment. So
the matcher is deliberately biased toward counting, and this harness exists to keep the false-RED
number at or near zero rather than to drive the false-green number down.
"""
import random
import re
import sys
import pathlib

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from assert_review_marker import comment_names_commit  # noqa: E402

try:
    from markdown_it import MarkdownIt
except ImportError:
    print("markdown-it-py is not installed:  pip install markdown-it-py", file=sys.stderr)
    sys.exit(2)

H = "3f9a2c7e1b8d40526af1c93e07b45d8e2a6f1c04"
MARKER = f"Reviewed-Commit: {H}"
PREFIXES = ["", "> ", ">", "- ", "1. ", "  ", "    ", "\t", "> > ", "- > "]
FENCES = ["```", "~~~", "````", "```yaml", "~~~~"]
FALSE_RED_BUDGET = 5  # measured 2 at the time of writing; a jump means live_lines regressed


def bodies(n: int, seed: int = 11):
    rnd = random.Random(seed)
    for _ in range(n):
        out = []
        for _ in range(rnd.randint(1, 6)):
            k = rnd.random()
            if k < 0.30:
                out.append(rnd.choice(PREFIXES) + rnd.choice(FENCES))
            elif k < 0.55:
                out.append(rnd.choice(PREFIXES) + MARKER)
            elif k < 0.75:
                out.append("")
            else:
                out.append(rnd.choice(PREFIXES) + "some finding text")
        yield "\n".join(out)


def renders_live(md, body: str) -> bool:
    html = md.render(body)
    html = re.sub(r"<pre>.*?</pre>", "", html, flags=re.S)
    html = re.sub(r"<code>.*?</code>", "", html, flags=re.S)
    return H in html or H[:12] in html


def main() -> int:
    md = MarkdownIt("commonmark")
    total = false_red = false_green = 0
    first_red = None
    for body in bodies(4000):
        total += 1
        got = comment_names_commit(body, [H])
        want = renders_live(md, body)
        if want and not got:
            false_red += 1
            first_red = first_red or body
        elif got and not want:
            false_green += 1
    print(f"{total} bodies vs markdown-it-py (commonmark)")
    print(f"  FALSE RED   (renders live, gate rejects): {false_red}   budget {FALSE_RED_BUDGET}")
    print(f"  false green (renders code, gate counts) : {false_green}   accepted by design")
    if first_red:
        print("  first false red:")
        for line in first_red.split("\n"):
            print(f"    |{line}")
    if false_red > FALSE_RED_BUDGET:
        print(f"REGRESSION: false reds exceed the budget. A false red on a required check is a "
              f"repo-wide merge stop; fix live_lines or justify raising the budget.")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
