#!/usr/bin/env python3
"""Differential harness: this matcher vs a real CommonMark implementation.

The table-driven battery is a REGRESSION NET, not a specification -- nine review rounds proved
that, each finding block-level rules the previous round's cases could not see. This harness is the
predictive check: it generates bodies mixing containers, fence characters and indents, renders each
with markdown-it-py, and reports where the matcher disagrees with what GitHub would show.

It is NOT run in CI today: it needs a pip dependency, and adding one to a security-adjacent gate
is itself a decision binding future work -- so it has a declared OPEN register row rather than only
this sentence, `docs/decisions/REVIEW-MARKER-BIAS.yaml`, which also carries the fail-open direction
below. Until that row closes, a ratchet that only fires when someone remembers is what this is.
Run it by hand whenever `live_lines` changes:

    pip install markdown-it-py && python3 .github/scripts/assert_review_marker_differential.py

WHAT THE NUMBERS MEAN. A FALSE RED (renders live, gate rejects) reports a COMPLETE, CORRECT review
as no review at all, on a required check -- it blocks that PR until the reviewer re-posts, and it
trains readers to work around the gate. A false green (renders as code, gate counts) costs the
anti-quoting property, which this gate has never been able to deliver anyway, since it cannot prove
which bot posted a comment. Every no-verdict path the gate exists to catch -- the action's
self-skip, 429, model outage, permission denials -- produces NO marker at all, so counting an
ambiguous one cannot weaken it. That is why the matcher is biased toward counting and why this
harness ratchets the false-RED number rather than driving the false-green number down.

(An earlier version of this paragraph said a false red here is "a repo-wide merge stop whose revert
needs the same check green". Review #9 refuted it: the repo-wide stop is the credit/outage case,
which is a TRUE red, and an admin bypass exists. The direction is unchanged; the reason is.)
"""
import random
import sys
from html.parser import HTMLParser
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
# THE BUDGET IS A RATCHET OVER A SWEEP, not over one corpus. Review #9: the shipped matcher
# measures a spread of false reds across ordinary seeds, so a budget checked against a SINGLE seed
# is unsensitive in both directions -- a change that multiplies false reds can pass, and a change
# that fixes them is invisible. The sweep reports every seed and ratchets on the WORST.
FALSE_RED_BUDGET = 5  # a jump above this means live_lines regressed; the run prints every seed
# A derived number may not be stated without its antecedents (ADR-20260817-105845). This harness
# is the only place allowed to state one, so it prints the corpus seed, the corpus size and the
# parser version it was measured against -- and no comment elsewhere quotes the figure.
CORPUS_SEEDS = (11, 1, 2, 3, 7, 99, 2024)  # a SWEEP, not one seed -- see the budget note below
CORPUS_SIZE = 4000


def bodies(n: int, seed: int):
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


class _LiveText(HTMLParser):
    """Collect only the text markdown-it renders OUTSIDE a <pre> block.

    THE PREVIOUS ORACLE STRIPPED EVERY `<code>`, and review #9 showed why that is wrong: an inline
    code span in a paragraph renders LIVE, and a backticked sha is the commonest real shape a
    reviewer writes (the marker regex accepts `` `<sha>` `` for exactly that reason). Treating it
    as "renders as code" made the oracle classify the most common real body as dead -- and the
    error direction HID FALSE REDS, i.e. it could only ever under-count the number the budget
    guards. A code span inside <pre> is dead; one in a paragraph is not, and only <pre> depth tells
    them apart.
    """

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.depth = 0
        self.out: list[str] = []

    def handle_starttag(self, tag, attrs):
        if tag == "pre":
            self.depth += 1

    def handle_endtag(self, tag):
        if tag == "pre" and self.depth:
            self.depth -= 1

    def handle_data(self, data):
        if self.depth == 0:
            self.out.append(data)


def renders_live(md, body: str) -> bool:
    parser = _LiveText()
    parser.feed(md.render(body))
    text = "".join(parser.out)
    return H in text or H[:12] in text


def main() -> int:
    # commonmark, not gfm-like, and that is CHECKED rather than assumed: review #9 classified
    # 4000 bodies x 7 seeds and all 55 body cases under both presets and got identical results.
    # GFM adds no rule that touches fenced or indented code, which is the only thing this oracle
    # asks about. (GitHub renders GFM; the one GFM-only shape that matters -- a marker inside a
    # TABLE CELL -- is a known miss, named in the operator hint, not a preset problem.)
    md = MarkdownIt("commonmark")
    try:
        from importlib.metadata import version as _v
        parser_version = _v("markdown-it-py")
    except Exception:  # noqa: BLE001 -- provenance is nice to have, never a reason to fail
        parser_version = "unknown"
    print(f"{CORPUS_SIZE} bodies/seed vs markdown-it-py {parser_version} (commonmark preset), "
          f"seeds {', '.join(str(s) for s in CORPUS_SEEDS)}")

    worst = 0
    first_red = None
    for seed in CORPUS_SEEDS:
        false_red = false_green = 0
        for body in bodies(CORPUS_SIZE, seed):
            got = comment_names_commit(body, [H])
            want = renders_live(md, body)
            if want and not got:
                false_red += 1
                first_red = first_red or body
            elif got and not want:
                false_green += 1
        worst = max(worst, false_red)
        print(f"  seed {seed:>4}:  FALSE RED {false_red:>3}   false green {false_green:>4} (accepted by design)")

    print(f"  WORST false-red count across the sweep: {worst}   budget {FALSE_RED_BUDGET}")
    if first_red:
        print("  first false red:")
        for line in first_red.split("\n"):
            print(f"    |{line}")
    if worst > FALSE_RED_BUDGET:
        print("REGRESSION: false reds exceed the budget on at least one seed. A false red makes a "
              "complete review report as no review; fix live_lines or justify raising the budget.")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
