#!/usr/bin/env python3
"""Relative-link checker for the repository's markdown.

WHY THIS IS A CHECK AND NOT A TYPE (compiler-first, ADR-20260803-234035).
CLAUDE.md's standing position is that a check is the FALLBACK: ask first whether the type
system can make the mistake unspellable. It cannot reach here, and the reason is the one
PROP-20260802-130500 §1 already records for `specs/**` YAML: the enforcement hierarchy ranks
ways to stop *Rust code* naming something it should not, and it has no rung for a target
written in prose. No newtype, sealed trait or capability witness can make
`[x](docs/adr/ADR-20260720-233000-....md)` unspellable, because the compiler never sees the
markdown. So "start at level 4" resolves to *level 3 is the ceiling here*, exactly as it did
for the `reads:` wall (ADR-20260812-214500) -- the gate is correct rather than lazy. It is
also the case ADR-20260803-234035 names in its own carve-out: write a gate where types cannot
reach, "non-Rust artifacts".

WHAT IS CHECKED, AND WHAT IS DELIBERATELY NOT.
  * IN: relative link TARGETS in tracked markdown -- inline links `[t](path)`, images
    `![a](path)`, and reference definitions `[id]: path`. The path must resolve to a file or
    directory that exists in the tree.
  * IN: the FRAGMENT half (`#section`), against the headings of the file it points into, using
    GitHub's slug algorithm (github-slugger), including its `-1`/`-2` disambiguation and
    explicit HTML anchors. A citation that lands in the right file at the wrong heading is the
    same silent-nothing this gate exists to stop, and the slug algorithm is deterministic and
    published -- there is no network and no flake in it.
  * OUT, AND DECLARED HERE RATHER THAN LEFT IMPLIED: RAW-HTML link forms -- `<a href=...>` and
    `<img src=...>`. Markdown files may embed HTML and this repo does (0 `<a href>`, 4 `<img src>`
    at the time of writing), so those targets are NOT checked. The carve-out is stated because an
    undeclared blind spot in a gate is the same defect class as a dead link: something a reader
    reasonably assumes is covered and is not.
  * OUT, ON PURPOSE: external URL LIVENESS (`http:`, `https:`, `mailto:`, any scheme). A
    blocking gate whose verdict depends on a third party's uptime and rate limiter reds on
    honest work, and this repository has retracted that instrument five times over in
    `tools/codegen-rs/src/tests.rs` under one rule: a red that fires on innocent work trains
    readers to discount reds. An unreachable URL is also not usually a defect in THIS commit.
    Link ROT is real, but it is a periodic report's job, not a merge blocker's.

THE TRAP THIS WAS WRITTEN FOR. ADRs filed before ~2026-07-22 have NO `ADR-` prefix
(`docs/adr/20260720-233000-...`), while CLAUDE.md and dispatch cards cite them WITH it. A link
built from the citation style resolves to nothing and GitHub renders it dead with no error.
Plain existence checking catches that class, and the report names the near-miss file (always -- it
is not behind a flag) so the reader is not left guessing which of the two spellings is real.

VACUITY IS A FAILURE MODE, NOT A PASS. A scanner that matches nothing passes, confidently and
meaninglessly -- it has happened twice in one session in this repo, once inside a coordinator's
own verification. So the corpus is derived from `git ls-files` (never a literal list) and THREE
things are asserted before any verdict is reported:
  1. the corpus is non-empty;
  2. `CLAUDE.md` -- the repo's root index, which by construction exists and carries links -- is
     IN the corpus. Non-emptiness alone is satisfied by a shimmed `git` printing one junk path;
  3. the number of links EXTRACTED is non-zero. A non-empty corpus with a broken extractor
     yields zero links and a clean green, which is the vacuity mode that survives (1) and (2).
Each has its own exit and its own message; all three are exercised red by the selftest.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import unicodedata
from pathlib import Path

# github-slugger's removal rule, as a CATEGORY rule rather than a transcribed character class.
#
# The first cut WAS a transcribed class -- the general-punctuation and supplemental-punctuation
# blocks plus ASCII punctuation -- and round-1 review installed the real npm `github-slugger` and
# compared the two over every heading in the tree: 1200 of 5882 diverged (20.4%). The class kept
# everything GitHub strips that it had not thought to list: the arrow and the rest of \p{S},
# emoji (So, plus their U+FE0F variation selector), the section sign, the Latin-1 symbol and
# punctuation range, and the C0 controls. Those are pervasive in this repo's headings.
#
# ENUMERATION IS THE DEFECT, not the particular characters missed -- the lesson
# `tools/codegen-rs/src/tests.rs` records for its own guards, where a hand-listed set was walked
# past three times. So the rule is stated POSITIVELY over Unicode general categories, which is
# what github-slugger's generated class encodes: KEEP letters, numbers, marks, `-`, `_` and
# space; drop everything else. Spaces then become hyphens.
#
# It was LATENT when review found it -- 0 false-green and 0 false-red across the whole corpus --
# and blocking anyway, because the instrument's fidelity is the deliverable. The failure it
# produces is a merge-blocking red on a CORRECT citation, whose only invited "fix" is editing a
# live link into a dead one; the mirror case is a dead link that passes, silently.
#
# MARKS ARE KEPT, AND THAT INCLUDES THE VARIATION SELECTORS -- which is not what it looks like.
# A decomposed accented letter is L + Mn, so dropping Mn would mangle every French heading. The
# subtle half is U+FE0F, the emoji variation selector: it is also Mn, and github-slugger KEEPS
# it. So `## WARNING-SIGN Edge 1` anchors as `#<U+FE0F>-edge-1` -- a slug with an invisible
# leading character.
#
# This cut BOTH ways and the first attempt got it backwards: reasoning that "GitHub obviously
# strips emoji presentation", U+FE00..U+FE0F was excluded, which looked right and was measured
# WRONG -- 131 headings (2.2%) still diverged, every one of them this class, in the direction of
# producing an anchor GitHub does not generate. The oracle settled it, not the reasoning: the
# real npm `github-slugger` run over all 5882 headings in the tree. 0 divergent with the plain
# category rule. Do not "fix" this by special-casing U+FE0F again without re-running that
# comparison -- the note above the corpus derivation applies here too.
_SLUG_KEEP_EXTRA = frozenset({"-", "_", " "})


def _slug_keep(ch: str) -> bool:
    return ch in _SLUG_KEEP_EXTRA or unicodedata.category(ch)[0] in ("L", "N", "M")

# A scheme (`https:`, `mailto:`, `tel:`) or a protocol-relative `//host`. Anything matching is
# external and is skipped -- see the module docstring for why liveness is out of scope.
_EXTERNAL = re.compile(r"^(?:[a-zA-Z][a-zA-Z0-9+.\-]*:|//)")

_FENCE = re.compile(r"^\s{0,3}(`{3,}|~{3,})")
_LIST_ITEM = re.compile(r"^\s*(?:[-*+]|\d+[.)])\s+")
_INLINE_CODE = re.compile(r"(`+)(?:.|\n)*?\1")
_HTML_COMMENT = re.compile(r"<!--(?:.|\n)*?-->")

# `[text](target)` and `![alt](target)`. The target stops at the first whitespace (a link title
# follows) or at the closing paren. `<...>` is markdown's spelling for a target with spaces.
_INLINE_LINK = re.compile(r"!?\[(?:[^\[\]]|\[[^\[\]]*\])*\]\(\s*(<[^>]*>|[^()\s]*)")
# `[id]: target "title"` at the start of a line -- a LINK reference definition.
#
# `[^id]:` IS EXCLUDED, and it is not a nicety. That is a FOOTNOTE definition, whose body is
# prose, not a target. Without the exclusion this pattern read 20 footnote bodies in one
# research dump as link targets and reported every one as broken -- 20 of a first measured 49,
# i.e. the majority of the finding was the instrument. A gate whose reds are mostly its own
# artefacts is the "trains readers to discount reds" failure, before it has shipped once.
_REF_DEF = re.compile(r"^\s{0,3}\[(?!\^)[^\]]+\]:\s*(<[^>]*>|\S+)")

_ATX_HEADING = re.compile(r"^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$")
_SETEXT_UNDERLINE = re.compile(r"^\s{0,3}(=+|-+)\s*$")
_HTML_ANCHOR = re.compile(r"<a\s[^>]*\b(?:id|name)\s*=\s*[\"']([^\"']+)[\"']", re.I)
_CUSTOM_ID = re.compile(r"\{#([^}\s]+)\}\s*$")


def slugify(text: str) -> str:
    """GitHub's heading -> anchor transform (github-slugger), minus the dedup counter."""
    # Strip inline markdown so `## **Bold** and \`code\`` slugs like the rendered text does.
    text = re.sub(r"!\[([^\]]*)\]\([^)]*\)", r"\1", text)          # images -> alt
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)           # links -> text
    text = re.sub(r"\[([^\]]*)\]\[[^\]]*\]", r"\1", text)          # ref links -> text
    text = re.sub(r"`+", "", text)                                  # code spans
    # EMPHASIS, BUT NOT AN INTRAWORD UNDERSCORE. Stripping `_` unconditionally slugged the
    # heading `... \`place_order\` is a COMMAND HANDLER ...` to `...placeorder...`, so a
    # CORRECT link in ADR-20260815-030206 was reported broken and the only "fix" the message
    # invited was damaging the document. CommonMark does not treat an intraword `_` as
    # emphasis and github-slugger keeps it, so neither may this.
    text = re.sub(r"\*\*|\*|~~", "", text)                          # asterisk / tilde emphasis
    text = re.sub(r"__([^_]+)__", r"\1", text)                      # __bold__
    text = re.sub(r"(?<!\w)_([^_]+)_(?!\w)", r"\1", text)           # _italic_, word-bounded
    text = _CUSTOM_ID.sub("", text)
    text = text.strip().lower()
    text = "".join(ch for ch in text if _slug_keep(ch))
    return text.replace(" ", "-")


def strip_code(lines: list[str]) -> list[str]:
    """Blank out code blocks, keeping line numbering intact so reports stay addressable.

    FENCED blocks, and INDENTED ones. The indented half is not optional and not cosmetic:
    `docs/STATUS.md` shows the template header for a new weekly journal file as an indented
    block, and that template contains `[../STATUS.md](../STATUS.md)` -- a path which is correct
    FROM `docs/status/`, where the template's content is destined, and dangling from
    `docs/STATUS.md`, where it is being displayed. GitHub renders it as literal text; it is not
    a link at all. Reporting it would be a false positive whose only "fix" is damaging a
    correct document to satisfy the checker.

    LIST-AWARE, because the naive rule is worse than none here. This repo indents continuation
    paragraphs under list items constantly, and those DO contain live links; treating every
    4-space indent as code would silently drop them from the corpus -- vacuity by a quieter
    door than an empty corpus, and one no assertion would catch. So the threshold is measured
    from the enclosing list item's content column, and an indented block additionally requires
    a preceding blank line (CommonMark: indented code cannot interrupt a paragraph).
    """
    out: list[str] = []
    fence: str | None = None
    prev_blank = True
    in_indented = False
    list_indent = 0  # content column of the innermost open list item
    for line in lines:
        stripped = line.strip()
        m = _FENCE.match(line)
        if fence is not None:
            if m and m.group(1)[0] * 3 == fence and not line.strip()[len(m.group(1)):].strip():
                fence = None
            out.append("")
            prev_blank = False
            continue
        if m:
            fence = m.group(1)[0] * 3
            out.append("")
            prev_blank = False
            continue

        if not stripped:
            out.append(line)
            prev_blank = True
            continue

        indent = len(line) - len(line.lstrip(" "))
        lm = _LIST_ITEM.match(line)
        if lm and not in_indented:
            list_indent = len(lm.group(0))
        elif indent == 0:
            # Back at column 0 on a non-list line: no list item is open any more.
            list_indent = 0

        is_code = indent >= list_indent + 4 and (prev_blank or in_indented)
        in_indented = is_code
        out.append("" if is_code else line)
        prev_blank = False
    return out


def anchors_of(path: Path) -> set[str]:
    """Every fragment GitHub would resolve inside `path`."""
    try:
        raw = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return set()
    body = _HTML_COMMENT.sub("", raw)
    lines = strip_code(body.splitlines())

    found: set[str] = set()
    seen: dict[str, int] = {}

    def add(text: str) -> None:
        base = slugify(text)
        if not base:
            return
        n = seen.get(base, 0)
        seen[base] = n + 1
        found.add(base if n == 0 else f"{base}-{n}")

    for i, line in enumerate(lines):
        m = _ATX_HEADING.match(line)
        if m:
            add(m.group(2))
            continue
        # Setext: an underline of = or - under a non-blank, non-list line.
        if _SETEXT_UNDERLINE.match(line) and i > 0:
            prev = lines[i - 1].strip()
            if prev and not prev.startswith(("-", "*", "+", "|", ">", "#")):
                add(prev)

    for m in _HTML_ANCHOR.finditer(body):
        found.add(m.group(1))
    for line in lines:
        m = _CUSTOM_ID.search(line)
        if m and _ATX_HEADING.match(line):
            found.add(m.group(1))
    return found


def targets_in(path: Path) -> list[tuple[int, str]]:
    """(line number, raw target) for every relative-capable link in `path`."""
    try:
        raw = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return []
    body = _HTML_COMMENT.sub("", raw)
    lines = strip_code(body.splitlines())

    out: list[tuple[int, str]] = []
    for n, line in enumerate(lines, start=1):
        clean = _INLINE_CODE.sub(lambda m: " " * len(m.group(0)), line)
        for m in _INLINE_LINK.finditer(clean):
            out.append((n, m.group(1)))
        m = _REF_DEF.match(clean)
        if m:
            out.append((n, m.group(1)))
    return out


def suggest(root: Path, missing: Path) -> str:
    """Name a near-miss file, so an `ADR-`-prefix break says which spelling is real."""
    parent = missing.parent
    if not parent.is_dir():
        return ""
    stem = missing.name
    candidates = sorted(p.name for p in parent.iterdir())
    for cand in candidates:
        if cand == stem:
            continue
        # The prefix trap in BOTH directions, plus case-only slips. `removeprefix`, not
        # `lstrip`: `lstrip("ADR-")` strips a character SET, so it eats the leading digits of
        # `ADR-20260720-...` down to `720-...` and matches things that are not near-misses.
        if cand.removeprefix("ADR-") == stem.removeprefix("ADR-") or cand.lower() == stem.lower():
            return f"  did you mean `{(parent / cand).relative_to(root)}`?"
    return ""


def corpus(root: Path) -> list[Path]:
    """Tracked markdown, DERIVED FROM THE TREE. Never a literal list."""
    try:
        # TRACKED **AND** NEW-BUT-NOT-YET-ADDED (`--others --exclude-standard`, so `.gitignore`
        # is respected). Tracked-only was the first cut and it has a hole a contributor meets on
        # their FIRST run: a brand-new ADR is untracked until `git add`, so the file whose links
        # are most likely to be wrong -- the one being written right now -- was the one file the
        # local gate skipped. It was caught here by this checker reporting 451 files while the
        # tree had 452. CI never sees that hole (everything is committed by then), which is
        # exactly why it would have survived: green in CI, useless where it was needed.
        res = subprocess.run(
            ["git", "-C", str(root), "ls-files", "-z", "--others", "--cached",
             "--exclude-standard", "--", "*.md", "*.markdown"],
            capture_output=True, check=True,
        )
        names = sorted(set(n for n in res.stdout.decode("utf-8", "replace").split("\0") if n))
    except (OSError, subprocess.CalledProcessError):
        # Not a git tree (the selftest's fixtures are not): walk instead. Same derivation
        # principle, different oracle.
        names = [
            str(p.relative_to(root))
            for p in root.rglob("*")
            if p.is_file() and p.suffix in (".md", ".markdown")
        ]
    return [root / n for n in sorted(names)]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--root", default=None, help="tree to scan (default: this repository)")
    ap.add_argument("--no-anchors", action="store_true", help="check paths only, not fragments")
    args = ap.parse_args()

    root = Path(args.root).resolve() if args.root else Path(__file__).resolve().parent.parent
    files = corpus(root)

    # ── VACUITY GUARDS. A scanner that matches nothing passes; these three make that a RED. ──
    if not files:
        print(
            f"link-check: FAIL — the corpus is EMPTY under {root}.\n"
            "  A scan of no files reports no broken links, which is a clean, confident and\n"
            "  entirely meaningless green. Something is wrong with the file derivation\n"
            "  (`git ls-files`), not with the links.",
            file=sys.stderr,
        )
        return 2
    if not args.root and not any(f.name == "CLAUDE.md" and f.parent == root for f in files):
        print(
            "link-check: FAIL — the corpus does not contain `CLAUDE.md`.\n"
            "  Non-emptiness alone is satisfied by a `git` that prints one junk path. The repo\n"
            "  root index exists by construction and carries links, so its absence means the\n"
            "  corpus is not this repository.",
            file=sys.stderr,
        )
        return 2

    broken: list[str] = []
    scanned = 0
    anchor_cache: dict[Path, set[str]] = {}

    for f in files:
        rel = f.relative_to(root)
        for lineno, raw in targets_in(f):
            target = raw.strip()
            if target.startswith("<") and target.endswith(">"):
                target = target[1:-1]
            if not target or _EXTERNAL.match(target):
                continue
            scanned += 1

            path_part, _, frag = target.partition("#")
            path_part = path_part.strip()

            if path_part:
                base = root / path_part.lstrip("/") if path_part.startswith("/") else f.parent / path_part
                try:
                    dest = base.resolve()
                except OSError:
                    dest = base
                if not dest.exists():
                    broken.append(
                        f"{rel}:{lineno}: `{target}` -> no such file or directory"
                        + suggest(root, base)
                    )
                    continue
            else:
                dest = f  # a bare `#fragment` is same-document

            if frag and not args.no_anchors and dest.is_file() and dest.suffix in (".md", ".markdown"):
                if dest not in anchor_cache:
                    anchor_cache[dest] = anchors_of(dest)
                if frag not in anchor_cache[dest]:
                    where = "this file" if dest == f else f"`{path_part}`"
                    broken.append(
                        f"{rel}:{lineno}: `{target}` -> the file exists, but {where} has no "
                        f"heading anchor `#{frag}`"
                    )

    if scanned == 0:
        print(
            f"link-check: FAIL — {len(files)} markdown files were read and ZERO relative links\n"
            "  were extracted from them. The corpus is real, so this is the extractor, not the\n"
            "  content: a green here would mean nothing.",
            file=sys.stderr,
        )
        return 2

    if broken:
        print(f"link-check: FAIL — {len(broken)} broken link(s):\n", file=sys.stderr)
        for b in broken:
            print(f"  {b}", file=sys.stderr)
        print(
            f"\n  Scanned {scanned} relative link(s) across {len(files)} markdown file(s).\n"
            "  A broken link is a citation that silently resolves to nothing: GitHub renders it\n"
            "  dead with no error, so nothing else in this repository will ever tell you.\n"
            "  External URLs are NOT checked here (see tools/link-check.py's docstring).",
            file=sys.stderr,
        )
        return 1

    print(
        f"link-check: OK — {scanned} relative link(s) across {len(files)} markdown file(s); "
        f"0 broken.{'' if not args.no_anchors else ' (paths only, --no-anchors)'}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
