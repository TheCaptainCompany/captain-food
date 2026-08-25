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
from assert_review_marker import FENCE_AT_COL0  # noqa: E402
from assert_review_marker import MARKER as MARKER_RE  # noqa: E402  (local MARKER is the literal)
from assert_review_marker import comment_names_commit  # noqa: E402

try:
    from markdown_it import MarkdownIt
except ImportError:
    print("markdown-it-py is not installed:  pip install markdown-it-py", file=sys.stderr)
    sys.exit(2)

H = "3f9a2c7e1b8d40526af1c93e07b45d8e2a6f1c04"
MARKER = f"Reviewed-Commit: {H}"
PREFIXES = ["", "> ", ">", "- ", "1. ", "  ", "    ", "\t", "> > ", "- > "]
# THE ALPHABET DECIDES WHAT THE BUDGET CAN SEE. Every entry here used to be a genuine fence
# opener, so no generated body could ever make the matcher and CommonMark disagree about whether a
# column-0 delimiter OPENS one -- review #10's finding, and it hid a live false red for two rounds.
# The last three are lines CommonMark does NOT treat as fences, and the middle group is delimiters
# indented out of column 0.
FENCES = [
    "```", "~~~", "````", "```yaml", "~~~~",
    " ```", "  ~~~", "   ```",
    "```x```", "```a`b", "```make validate``` is green.",
]
# A RATCHET, NOT A CEILING -- and it fails in BOTH directions.
#
# Review #10: the previous shape was `max(per_seed) > 5`. That reproduces, one level up, the exact
# defect it was introduced to fix. Seeds sitting at 0 carried five units of slack, so a change
# taking EVERY seed to 5 passed silently, and a change that fixed a seed was invisible. A single
# scalar compared against a constant cannot ratchet a vector.
#
# So the baseline is the per-seed vector itself, committed here the way
# tools/codegen-rs/warning-baseline.json is: exceeding a seed's count is a REGRESSION, and beating
# it is a ratchet that must be tightened in the same change. The run prints the replacement dict
# either way, so refreshing it is a copy-paste, never a judgement call.
CORPUS_SEEDS = (11, 1, 2, 3, 7, 99, 2024)  # a SWEEP, not one seed -- see the budget note below
CORPUS_SIZE = 4000
FALSE_RED_BASELINE = {11: 0, 1: 0, 2: 1, 3: 0, 7: 0, 99: 1, 2024: 0}


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


def live_lines_with_indent_rule(body: str):
    """OPTION (c) of docs/decisions/REVIEW-MARKER-BIAS.yaml, MADE RUNNABLE.

    That row dismissed "restore block-level rules" on a figure measured in a reviewer's scratchpad
    against code that existed nowhere -- an unverifiable antecedent propping up the only argument
    against an option in an OPEN row (ADR-20260817-105845, and review #10 caught it). So the
    variant lives here instead: `--variant indent` measures it, and anyone reopening the row can
    reproduce the cost in one command rather than trusting this file.

    The rule: column-0 fences as shipped, PLUS a line indented 4+ COLUMNS (tab = 4) is code.
    """
    fence = None
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
            if not (char == "`" and "`" in m.group("rest")):
                fence = (char, run)
                continue
        col = 0
        for ch in line:
            if ch == " ":
                col += 1
            elif ch == "\t":
                col += 4 - (col % 4)
            else:
                break
        if col >= 4:
            continue
        yield line.lstrip()


def names_commit_with_indent_rule(body: str, shas) -> bool:
    for line in live_lines_with_indent_rule(body):
        m = MARKER_RE.match(line.lstrip())
        if m and any(s.lower().startswith(m.group("h").lower()) for s in shas if s):
            return True
    return False


def main() -> int:
    variant = "--variant" in sys.argv and sys.argv[sys.argv.index("--variant") + 1] == "indent"
    classify = names_commit_with_indent_rule if variant else comment_names_commit
    if variant:
        print("VARIANT: col-0 fences PLUS a 4-column indented-code rule "
              "(REVIEW-MARKER-BIAS option (c)). Baseline comparison is skipped.")
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

    measured = {}
    first_red = None
    for seed in CORPUS_SEEDS:
        false_red = false_green = 0
        for body in bodies(CORPUS_SIZE, seed):
            got = classify(body, [H])
            want = renders_live(md, body)
            if want and not got:
                false_red += 1
                first_red = first_red or body
            elif got and not want:
                false_green += 1
        measured[seed] = false_red
        base = FALSE_RED_BASELINE.get(seed)
        mark = "  <-- REGRESSION" if base is not None and false_red > base else (
            "  <-- improved, tighten the baseline" if base is not None and false_red < base else "")
        print(f"  seed {seed:>4}:  FALSE RED {false_red:>3} (baseline {base})   "
              f"false green {false_green:>4} (accepted by design){mark}")

    if first_red:
        print("  first false red:")
        for line in first_red.split("\n"):
            print(f"    |{line}")

    if variant:
        print(f"\n  variant false reds by seed: {measured}  "
              f"(shipped design: {FALSE_RED_BASELINE})")
        return 0
    if measured != FALSE_RED_BASELINE:
        worse = [s for s in CORPUS_SEEDS if measured[s] > FALSE_RED_BASELINE.get(s, 0)]
        print()
        if worse:
            print(f"REGRESSION: seeds {worse} produce MORE false reds than the committed baseline. "
                  f"A false red reports a complete, correct review as no review at all.")
        else:
            print("RATCHET: live_lines improved. Tighten the baseline in the same change, or the "
                  "next regression hides inside the slack -- which is the defect this replaced.")
        print(f"  FALSE_RED_BASELINE = {measured}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
