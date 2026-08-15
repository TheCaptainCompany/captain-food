use crate::*;

// ─── §13 — proposal hygiene (docs/proposals/PROP-*.md, #272) ────────────────────────────────────
//
// Realizes the docs/proposals/README.md header convention + the CLAUDE.md "Named concerns" rule
// (an unchecked concern mechanically blocks `Approved`). Scope: `PROP-*.md` files ONLY — the two
// legacy non-PROP files predate the convention and are grandfathered out by the filename filter.
// Severities were calibrated against the committed corpus (2026-07-31: all 31 PROP-* files carry a
// Status line and a header tracking-issue link, and every Approved one names an ADR), so all four
// rules are ERRORS — the gate stays 0-error without grandfathering any rule down to a warning.

/// Read every `docs/proposals/PROP-*.md` under the repo root, sorted for determinism. A missing
/// directory yields an empty corpus (mirrors the tolerant `read_dir` pattern of `load_model`).
pub(crate) fn load_proposal_files(root: &std::path::Path) -> Vec<(String, String)> {
    load_proposal_dir(root, |n| n.starts_with("PROP-") && n.ends_with(".md"))
}

/// The corpus §13b governs: the decision REGISTER plus every proposal. `DECISIONS.md` is deliberately
/// in scope here and out of scope for the four hygiene rules above — it carries no `**Status**` header
/// of its own, but it is the table the whole decision process reads (#572).
pub(crate) fn load_decision_table_files(root: &std::path::Path) -> Vec<(String, String)> {
    load_proposal_dir(root, |n| n == "DECISIONS.md" || (n.starts_with("PROP-") && n.ends_with(".md")))
}

/// Shared `docs/proposals` reader: every file whose name passes `keep`, sorted for determinism.
fn load_proposal_dir(root: &std::path::Path, keep: fn(&str) -> bool) -> Vec<(String, String)> {
    let dir = root.join("docs/proposals");
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        let mut paths: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.file_name().and_then(|n| n.to_str()).map(keep).unwrap_or(false))
            .collect();
        paths.sort();
        for p in paths {
            if let (Some(name), Ok(content)) = (p.file_name().and_then(|n| n.to_str()), fs::read_to_string(&p)) {
                out.push((format!("docs/proposals/{}", name), content));
            }
        }
    }
    out
}

/// The Status value: the text after `**Status**` on the first line carrying it. Tolerates both the
/// `- **Status**:` list form and the bare `**Status**:` form used by existing files.
pub(crate) fn proposal_status(content: &str) -> Option<&str> {
    for line in content.lines() {
        if let Some(idx) = line.find("**Status**") {
            let rest = &line[idx + "**Status**".len()..];
            return Some(rest.trim_start().trim_start_matches(':').trim());
        }
    }
    None
}

/// True when `text` contains a FULL clickable tracking-issue link (a bare `#NN` is a dead reference
/// in repo markdown — GitHub only auto-links it inside issues/PRs/commits).
pub(crate) fn has_tracking_issue_link(text: &str) -> bool {
    const NEEDLE: &str = "https://github.com/TheCaptainCompany/captain-food/issues/";
    let mut rest = text;
    while let Some(idx) = rest.find(NEEDLE) {
        let after = &rest[idx + NEEDLE.len()..];
        if after.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            return true;
        }
        rest = after;
    }
    false
}

/// True when an UNCHECKED `- [ ]` item exists inside a Concerns block — either the header-entry
/// form (`- **Concerns**:` + indented checklist) or a `## Concerns` section. The scan is SCOPED to
/// the block so unchecked checklists elsewhere in the body (e.g. scope checklists) never trip it:
/// a heading always ends the block; the header-entry form also ends at a blank line or the next
/// sibling `- **Field**:` entry (its own checklist items are indented `- [ ]`/`- [x]` lines).
pub(crate) fn proposal_has_unresolved_concern(content: &str) -> bool {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        let is_heading_marker =
            trimmed.starts_with('#') && trimmed.trim_start_matches('#').trim().starts_with("Concerns");
        let is_entry_marker = line.contains("**Concerns**");
        if !(is_heading_marker || is_entry_marker) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < lines.len() {
            let l = lines[j];
            let t = l.trim_start();
            if t.starts_with('#') {
                break; // next heading ends BOTH block forms
            }
            if is_entry_marker && (l.trim().is_empty() || t.starts_with("- **")) {
                break; // header-entry form: blank line or sibling header field ends the block
            }
            if t.starts_with("- [ ]") {
                return true;
            }
            j += 1;
        }
        i = j.max(i + 1);
    }
    false
}

/// The four proposal-hygiene rules, pure over `(path, content)` pairs so unit tests can feed
/// fixture strings and `main` feeds `load_proposal_files(repo_root)`.
pub(crate) fn validate_proposal_hygiene(files: &[(String, String)]) -> Vec<Issue> {
    let mut issues = Vec::new();
    for (path, content) in files {
        let status = proposal_status(content);
        if status.is_none() {
            issues.push(err(
                "proposal-status-missing",
                path.clone(),
                "no `**Status**` line — the header block (docs/proposals/README.md) requires one.".into(),
            ));
        }
        // The tracking-issue link must sit in the HEADER (first 40 lines), as a full clickable URL
        // (ADR-20260724-143000). Corpus-calibrated to ERROR: every committed PROP-* file passes.
        let header: String = content.lines().take(40).collect::<Vec<_>>().join("\n");
        if !has_tracking_issue_link(&header) {
            issues.push(err(
                "proposal-tracking-issue-missing",
                path.clone(),
                "no tracking-issue link (https://github.com/TheCaptainCompany/captain-food/issues/<N>) in the first 40 lines — every proposal has a tracking issue, named in the header as a FULL clickable link (ADR-20260724-143000).".into(),
            ));
        }
        // `Approved`/`APPROVED` case-sensitively: a Proposed status mentioning "partially approved"
        // in prose is NOT an approval (e.g. PROP-20260730-032306).
        let approved = status.map(|s| s.contains("Approved") || s.contains("APPROVED")).unwrap_or(false);
        if approved {
            if proposal_has_unresolved_concern(content) {
                issues.push(err(
                    "proposal-approved-unresolved-concern",
                    path.clone(),
                    "Status is Approved but an unchecked `- [ ]` item remains in the Concerns block — an unchecked concern mechanically blocks Approved (CLAUDE.md \"Named concerns\"): resolve it by CHECKING it with a one-line resolution, never by deleting it.".into(),
                ));
            }
            // Corpus-calibrated to ERROR: every Approved PROP-* file already names an ADR.
            if !content.contains("ADR-") {
                issues.push(err(
                    "proposal-approved-without-decision",
                    path.clone(),
                    "Status is Approved but the file references no ADR (`ADR-…`) — an approval is recorded by a decision record; name the ADR that recorded it.".into(),
                ));
            }
        }
    }
    issues
}

// ─── §13b — markdown table integrity (DECISIONS.md + PROP-*.md, #572) ───────────────────────────
//
// The decision register is a TABLE, and GFM never complains about a malformed row: "if a row has
// fewer cells than the header row, empty cells are inserted; if it has more, the excess is IGNORED"
// (GFM spec §4.10). So one stray `|` silently drops the tail of a row, and the register — the
// artifact the whole decision process reads — loses content with no diff-visible signal.
//
// CELL COUNTING FOLLOWS GFM EXACTLY, and the load-bearing part is what "effective" means:
//
//   * A `|` preceded by an ODD number of backslashes is escaped and does NOT open a cell.
//   * A pipe inside a CODE SPAN still opens a cell. This is counter-intuitive and it is the one
//     thing worth checking rather than assuming — the GFM spec settles it in normative prose plus a
//     worked example: "Include a pipe in a cell's content by escaping it, INCLUDING INSIDE OTHER
//     INLINE SPANS", example 200, where the cell `` `\|` `` (escaped, inside backticks) is what
//     renders as `<code>|</code>`. Cell splitting happens on the raw row before any inline parsing,
//     so backticks are simply not visible to it. Consequence for authors: inside a table, write
//     `` `NORMAL \| BUSY` ``, never `` `NORMAL | BUSY` `` — the corpus already does this in 19 rows.
//     Modelling code spans as pipe-neutral would have been the intuitive choice and it would have
//     made this rule blind to the exact defect #572 was filed for.
//   * Leading and trailing pipes are optional delimiters, not cells; the `|---|---|` delimiter row
//     is checked against the header rather than as a body row.
//   * Fenced code blocks are not markdown at all — a `|` there is program text.
//
// Deliberately NOT modelled: GFM treats a pipe-LESS line straight after a table row as a further
// row (spec example 202 renders a trailing `bar` paragraph line as a one-cell row). Following that
// would make every paragraph glued under a table an error, so a pipe-less line ENDS the table here.
// This is the conservative direction and it costs nothing today: on the committed corpus (446
// tables, 1959 body rows) both readings report the identical set, because no file glues prose to a
// table without a blank line.

/// Split a markdown table row into its cells, GFM-style; `None` when the line carries no unescaped
/// pipe and so is not a row at all. Leading/trailing pipes are delimiters and are dropped.
pub(crate) fn split_table_row(line: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = line.trim().chars().collect();
    // Indices of the pipes that actually delimit — a run of backslashes of ODD length escapes.
    let mut pipes: Vec<usize> = Vec::new();
    let mut backslashes = 0usize;
    for (i, c) in chars.iter().enumerate() {
        match c {
            '\\' => backslashes += 1,
            '|' => {
                if backslashes % 2 == 0 {
                    pipes.push(i);
                }
                backslashes = 0;
            }
            _ => backslashes = 0,
        }
    }
    if pipes.is_empty() {
        return None;
    }
    let mut start = 0usize;
    let mut end = chars.len();
    if pipes.first() == Some(&0) {
        pipes.remove(0);
        start = 1;
    }
    if !chars.is_empty() && pipes.last() == Some(&(chars.len() - 1)) {
        pipes.pop();
        end = chars.len() - 1;
    }
    let mut cells = Vec::new();
    let mut cursor = start;
    for p in pipes {
        cells.push(chars[cursor..p].iter().collect::<String>().trim().to_string());
        cursor = p + 1;
    }
    cells.push(chars[cursor.min(end)..end].iter().collect::<String>().trim().to_string());
    Some(cells)
}

/// A delimiter cell: hyphens, optionally colon-anchored on either side (`---`, `:--`, `--:`, `:-:`).
fn is_delimiter_cell(cell: &str) -> bool {
    let t = cell.trim();
    let t = t.strip_prefix(':').unwrap_or(t);
    let t = t.strip_suffix(':').unwrap_or(t);
    !t.is_empty() && t.chars().all(|c| c == '-')
}

/// True when every cell of `line` is a delimiter cell — the `|---|---|` shape, whatever its width.
fn delimiter_row_cells(line: &str) -> Option<usize> {
    let cells = split_table_row(line)?;
    if cells.iter().all(|c| is_delimiter_cell(c)) { Some(cells.len()) } else { None }
}

/// True when `line` opens/closes nothing and simply cannot start a table body row: a heading, a
/// blockquote or a fence all end the table per GFM ("the beginning of another block-level structure").
fn ends_table_block(line: &str) -> bool {
    let t = line.trim_start();
    t.is_empty()
        || t.starts_with('#')
        || t.starts_with('>')
        || t.starts_with("```")
        || t.starts_with("~~~")
}

/// The opening fence of a code block, if this line is one: `(fence char, run length)`. Indented by
/// four or more spaces it is an indented code block, not a fence.
fn code_fence(line: &str) -> Option<(char, usize)> {
    let t = line.trim_start();
    if line.len() - t.len() >= 4 {
        return None;
    }
    let c = t.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let n = t.chars().take_while(|&x| x == c).count();
    (n >= 3).then_some((c, n))
}

/// §13b: every table row must carry exactly as many cells as its header. Pure over `(path, content)`
/// so tests can feed fixture strings; `main` feeds `load_decision_table_files(repo_root)`.
pub(crate) fn validate_markdown_tables(files: &[(String, String)]) -> Vec<Issue> {
    let mut issues = Vec::new();
    for (path, content) in files {
        let lines: Vec<&str> = content.lines().collect();
        let mut open_fence: Option<(char, usize)> = None;
        let mut i = 0usize;
        while i < lines.len() {
            let line = lines[i];
            // Fenced code is program text: a `|` in there delimits nothing.
            if let Some((fence_char, fence_len)) = open_fence {
                let t = line.trim();
                if t.chars().count() >= fence_len && t.chars().all(|x| x == fence_char) {
                    open_fence = None;
                }
                i += 1;
                continue;
            }
            if let Some(f) = code_fence(line) {
                open_fence = Some(f);
                i += 1;
                continue;
            }
            // A table = a header row whose NEXT line is an all-delimiter row. GFM additionally
            // requires the two to agree in width; when they do not, nothing renders as a table at
            // all — a silent whole-block loss, so it is reported rather than skipped.
            let Some(header) = split_table_row(line) else {
                i += 1;
                continue;
            };
            let Some(delim) = lines.get(i + 1).and_then(|l| delimiter_row_cells(l)) else {
                i += 1;
                continue;
            };
            if delim != header.len() {
                issues.push(warn(
                    "markdown-table-delimiter-cell-count",
                    format!("{}:{}", path, i + 2),
                    format!(
                        "delimiter row has {} cell(s) but the header above has {} — GFM requires them to match, so this block does NOT render as a table at all (the pipes show up as literal text).",
                        delim,
                        header.len()
                    ),
                ));
                i += 2;
                continue;
            }
            let mut j = i + 2;
            while j < lines.len() {
                let row = lines[j];
                if ends_table_block(row) {
                    break;
                }
                let Some(cells) = split_table_row(row) else {
                    break; // conservative: a pipe-less line ends the table (see the section note)
                };
                if cells.len() != header.len() {
                    issues.push(warn(
                        "markdown-table-row-cell-count",
                        format!("{}:{}", path, j + 1),
                        format!(
                            "table row has {} cell(s) but its header declares {} — GFM pads a short row and SILENTLY DROPS the excess of a long one, so this row does not render as written. A `|` inside a code span still opens a cell: escape it as `\\|`.",
                            cells.len(),
                            header.len()
                        ),
                    ));
                }
                j += 1;
            }
            i = j;
        }
    }
    issues
}

