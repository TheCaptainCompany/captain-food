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
    let dir = root.join("docs/proposals");
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        let mut paths: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("PROP-") && n.ends_with(".md"))
                    .unwrap_or(false)
            })
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

