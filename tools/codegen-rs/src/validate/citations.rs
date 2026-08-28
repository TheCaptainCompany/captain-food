use crate::*;

// ─── §23 — the record-citation ratchet (founder requirements 7–9, 2026-08-21; PROP-20260819-110442
// slice 1) ──────────────────────────────────────────────────────────────────────────────────────
//
// Every full-form `ADR-YYYYMMDD-HHMMSS` / `PROP-YYYYMMDD-HHMMSS` and legacy `ADR-00NN` citation in
// the governed documentation surfaces must resolve to a real record file. Measured at adoption:
// 5,130 citations across docs/** + CLAUDE.md, 4 distinct dangling ids — all four carried by
// docs/decisions/_exempt.yaml with a reason and a retirement event, so the ratchet adopts the
// whole surface with ZERO historical rewriting (the founder's own bound: no repo-wide cleanup).
//
// RESOLUTION PROVES EXISTENCE, NEVER AUTHORITY (founder requirement 9): a resolving ADR is a
// controlling decision only by its own status; a resolving PROP is an option space; a resolving
// legal brief is preparation, never clearance; a held record is citable as existing, not as
// controlling; and a citation resolves to the governing SOURCE record — never to a generated
// projection (the resolver only knows docs/adr/ and docs/proposals/ filenames, by construction).
//
// Fenced code blocks are quoted output (validator mockups, transcripts) and are NOT scanned;
// inline code spans ARE — docs legitimately cite real ids inside single backticks.

/// One `_exempt.yaml` entry: an id that is knowingly unresolvable, why, and the event that retires
/// the exemption. An exemption that exempts nothing is itself an error — the file is a
/// self-pruning queue, never a permanent bypass list.
pub(crate) struct CitationExemption {
    pub(crate) id: String,
    pub(crate) reason: String,
}

pub(crate) fn load_citation_exemptions(root: &std::path::Path) -> (Vec<CitationExemption>, Vec<Issue>) {
    let path = root.join("docs/decisions/_exempt.yaml");
    match fs::read_to_string(&path) {
        Ok(content) => parse_citation_exemptions(&content),
        Err(_) => (Vec::new(), Vec::new()),
    }
}

pub(crate) fn parse_citation_exemptions(content: &str) -> (Vec<CitationExemption>, Vec<Issue>) {
    let loc = "docs/decisions/_exempt.yaml".to_string();
    let mut out = Vec::new();
    let mut issues = Vec::new();
    let v: Value = match serde_yaml::from_str(content) {
        Ok(v) => v,
        Err(e) => return (out, vec![err("citation-exemption-shape", loc, format!("not parseable as YAML: {}", e))]),
    };
    let Some(seq) = v.get("exempt").and_then(|s| s.as_sequence()) else {
        return (out, vec![err("citation-exemption-shape", loc, "no `exempt:` sequence.".into())]);
    };
    let mut seen = BTreeSet::new();
    for entry in seq {
        let Some(map) = entry.as_mapping() else {
            issues.push(err("citation-exemption-shape", loc.clone(), "an `exempt:` entry is not a mapping.".into()));
            continue;
        };
        let get = |k: &str| map.get(Value::String(k.into())).and_then(|x| x.as_str()).map(|s| s.to_string());
        for (k, _) in map {
            let name = k.as_str().unwrap_or("?");
            if !["id", "reason", "retires_when"].contains(&name) {
                issues.push(err("citation-exemption-shape", loc.clone(), format!("unknown field `{}` on an exempt entry.", name)));
            }
        }
        match (get("id"), get("reason"), get("retires_when")) {
            (Some(id), Some(reason), Some(_)) => {
                if !seen.insert(id.clone()) {
                    issues.push(err("citation-exemption-shape", loc.clone(), format!("duplicate exemption for `{}`.", id)));
                }
                out.push(CitationExemption { id, reason });
            }
            (id, _, _) => issues.push(err(
                "citation-exemption-shape",
                loc.clone(),
                format!(
                    "an exempt entry{} must carry all of `id`, `reason` and `retires_when` — an exemption without its retirement event is a permanent bypass.",
                    id.map(|i| format!(" (`{}`)", i)).unwrap_or_default()
                ),
            )),
        }
    }
    (out, issues)
}

/// The governed documentation surfaces: every .md/.yaml/.html under docs/** (recursive), plus
/// CLAUDE.md. `_exempt.yaml` itself is excluded — it names dangling ids on purpose. Sorted for
/// deterministic issue output.
pub(crate) fn load_governed_doc_files(root: &std::path::Path) -> Vec<(String, String)> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut stack = vec![root.join("docs")];
    while let Some(dir) = stack.pop() {
        if let Ok(rd) = fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    if (name.ends_with(".md") || name.ends_with(".yaml") || name.ends_with(".html"))
                        && name != "_exempt.yaml"
                    {
                        paths.push(p);
                    }
                }
            }
        }
    }
    paths.push(root.join("CLAUDE.md"));
    paths.sort();
    let mut out = Vec::new();
    for p in paths {
        if let Ok(content) = fs::read_to_string(&p) {
            let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            out.push((rel, content));
        }
    }
    out
}

/// True for the lines inside fenced code blocks (``` or ~~~, up to 3 spaces of indent, closing
/// fence at least as long and of the same character), fence lines included.
fn fenced_lines(content: &str) -> Vec<bool> {
    let mut fenced = Vec::new();
    let mut open: Option<(char, usize)> = None;
    for line in content.lines() {
        let t = line.trim_start();
        let ind = line.len() - t.len();
        match open {
            Some((c, n)) => {
                fenced.push(true);
                let run = t.chars().take_while(|&x| x == c).count();
                if ind < 4 && run >= n && t.chars().all(|x| x == c || x.is_whitespace()) {
                    open = None;
                }
            }
            None => {
                let c = t.chars().next().unwrap_or(' ');
                let run = t.chars().take_while(|&x| x == c).count();
                if ind < 4 && (c == '`' || c == '~') && run >= 3 {
                    open = Some((c, run));
                    fenced.push(true);
                } else {
                    fenced.push(false);
                }
            }
        }
    }
    fenced
}

/// Extract `(id, line_number)` citations from one document, skipping fenced blocks. Longest form
/// first; boundary = the neighbouring characters are outside the id alphabet (so the id-prefix of
/// a full filename mention still counts, and resolves, while `ADR-2026` inside a longer number
/// never matches).
pub(crate) fn extract_citations(content: &str) -> Vec<(String, usize)> {
    let fenced = fenced_lines(content);
    let mut out = Vec::new();
    for (li, line) in content.lines().enumerate() {
        if fenced.get(li).copied().unwrap_or(false) {
            continue;
        }
        let bytes = line.as_bytes();
        let find_next = |s: &str| -> Option<usize> {
            match (s.find("ADR-"), s.find("PROP-")) {
                (Some(a), Some(p)) => Some(a.min(p)),
                (a, p) => a.or(p),
            }
        };
        let mut i = 0;
        while let Some(rel) = find_next(&line[i..]) {
            let start = i + rel;
            // preceding boundary: not part of a longer token
            if start > 0 {
                let prev = bytes[start - 1] as char;
                if prev.is_ascii_alphanumeric() || prev == '-' || prev == '_' {
                    i = start + 1;
                    continue;
                }
            }
            let rest = &line[start..];
            let take_digits = |s: &str, n: usize| s.len() >= n && s.as_bytes()[..n].iter().all(|b| b.is_ascii_digit());
            let mut matched: Option<usize> = None; // total id length
            for (prefix, full) in [("ADR-", true), ("PROP-", true), ("ADR-", false)] {
                if let Some(tail) = rest.strip_prefix(prefix) {
                    if full && take_digits(tail, 8) && tail.as_bytes().get(8) == Some(&b'-') && take_digits(&tail[9..], 6) {
                        matched = Some(prefix.len() + 15);
                        break;
                    }
                    if !full && take_digits(tail, 4) {
                        matched = Some(prefix.len() + 4);
                        break;
                    }
                }
            }
            match matched {
                Some(len) => {
                    let end = start + len;
                    let next = line[end..].chars().next();
                    let ok = match next {
                        Some(c) if c.is_ascii_digit() => false, // a longer number, not an id
                        _ => true,
                    };
                    if ok {
                        out.push((line[start..end].to_string(), li + 1));
                    }
                    i = end;
                }
                None => i = start + 1,
            }
        }
    }
    out
}

/// §23a: every citation resolves or is exempt; §23b: every exemption exempts something.
pub(crate) fn validate_citations(
    files: &[(String, String)],
    corpus: &RecordCorpus,
    exemptions: &[CitationExemption],
) -> Vec<Issue> {
    let mut issues = Vec::new();
    let mut used: BTreeSet<String> = BTreeSet::new();
    for (path, content) in files {
        for (id, line) in extract_citations(content) {
            if record_resolves(&id, corpus) {
                continue;
            }
            if let Some(ex) = exemptions.iter().find(|e| e.id == id) {
                used.insert(ex.id.clone());
                continue;
            }
            issues.push(err(
                "record-citation-unresolved",
                format!("{}:{}", path, line),
                format!(
                    "cites `{}`; no file matches under docs/adr/ or docs/proposals/. Fix the id, or — ONLY for a held/not-yet-deposited record — declare it in docs/decisions/_exempt.yaml with a reason and a retirement event. NOTE: resolution proves existence, never authority — an ADR controls by its own status, a PROP is an option space, a legal brief is never clearance.",
                    id
                ),
            ));
        }
    }
    for ex in exemptions {
        if !used.contains(&ex.id) {
            issues.push(err(
                "citation-exemption-unused",
                "docs/decisions/_exempt.yaml".to_string(),
                format!(
                    "exemption `{}` exempts nothing (reason on file: {}) — every citation of it now resolves, or none exists; remove the entry in this change. The exemption file is a self-pruning queue, never a permanent bypass.",
                    ex.id, ex.reason
                ),
            ));
        }
    }
    issues
}

/// §23c: no two record files may share a `YYYYMMDD-HHMMSS` stamp within a kind — stamp-based
/// resolution (the prefixless middle-era filenames) is sound only while the id scheme's
/// concurrency guarantee (ADR-20260718-135417) actually holds on disk.
pub(crate) fn validate_record_stamps(corpus: &RecordCorpus) -> Vec<Issue> {
    let mut issues = Vec::new();
    for (kind, files, dir) in [
        ("ADR", &corpus.adr_files, "docs/adr"),
        ("PROP", &corpus.proposal_files, "docs/proposals"),
    ] {
        let mut by_stamp: BTreeMap<String, Vec<&String>> = BTreeMap::new();
        for f in files {
            let bare = f.strip_prefix("ADR-").or(f.strip_prefix("PROP-")).unwrap_or(f);
            if bare.len() >= 15 {
                let stamp = &bare[..15];
                if stamp.as_bytes()[8] == b'-'
                    && stamp.chars().enumerate().all(|(i, c)| i == 8 || c.is_ascii_digit())
                {
                    by_stamp.entry(stamp.to_string()).or_default().push(f);
                }
            }
        }
        for (stamp, fs) in by_stamp {
            if fs.len() > 1 {
                issues.push(err(
                    "record-stamp-collision",
                    format!("{}/{}", dir, fs[0]),
                    format!(
                        "{} record stamp `{}` is carried by {} files ({:?}) — stamp-based citation resolution becomes ambiguous; re-mint one id (the scheme is concurrency-safe by the second, ADR-20260718-135417).",
                        kind, stamp, fs.len(), fs
                    ),
                ));
            }
        }
    }
    issues
}

// ─── §24 — an ADR cited in the instruction-surface corpus must still be LIVE (the unbuilt half of
// #477 "No gate checks a citation of a superseded ADR by its own Status line", #712) ─────────────
//
// §23 above proves a citation RESOLVES to a real file; it says nothing about whether that file is
// still the authority. `record_resolves`'s own docstring states the split: "resolution proves
// existence, never authority". This section reads the RESOLVED ADR's own `Status:` line and
// enforces the other half.
//
// DIVISION OF LABOR, mirroring the sentence `validate_no_superseded_row_is_cited_as_authority`
// draws between itself and the register's status-coupling rules: an id that does not resolve at
// all is §23's error and is never duplicated here; an id that resolves but names a SUPERSEDED
// record is this section's.
//
// SCOPE: the INSTRUCTION-SURFACE corpus (`claude_citation_corpus`, decisions.rs, widened by #710)
// — not the wider docs/** + CLAUDE.md corpus §23 scans. A stale citation inside a record that
// NARRATES history (an ADR discussing its own predecessor, or `docs/adr/README.md`'s index) is not
// the failure mode #477 named; a stale citation inside a file a SESSION READS AS AN INSTRUCTION
// before working is — the same reasoning `claude_citation_corpus`'s own SCOPE section gives.

/// One ADR's own authority, read from its `Status:` field.
enum AdrAuthority {
    Live,
    SupersededInPart,
    SupersededFully,
    /// The Status field could not be located or parsed. Reported loudly — but ONLY for a file that
    /// is actually CITED from the instruction-surface corpus, never a background sweep of the
    /// whole `docs/adr/` tree: a rule that cannot tell whether an uncited record is live has
    /// nothing to say about it.
    Unparseable,
}

/// The prose of an ADR's own `Status:` field, tolerant of the shapes the real corpus writes it in
/// — read from the tree before writing this parser, per #712's own instruction:
///   * a `## Status` / `# Status` HEADING, value = the first paragraph beneath it
///     (`20260720-004556-partner-reoffer-policy.md`: `## Status`, blank line, `Superseded by
///     ADR-… (…) — …`);
///   * an INLINE bold field, `**Status**: …` or `- **Status**: …`, possibly one of several
///     `·`-separated bold fields on the same physical line
///     (`ADR-20260808-195315-…`: `**Status**: Accepted · **Date**: … · **Deciders**: …`), and
///     possibly continued on INDENTED lines below — a wrapped list item
///     (`ADR-20260731-061609-…`: `- **Status**: **Superseded IN PART by […](…)**` then an indented
///     continuation line starting `(product owner, 2026-08-06: …) — only point 1 …`).
/// Returns `None` when NEITHER shape is found in the file.
fn adr_status_text(content: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    for (i, raw) in lines.iter().enumerate() {
        let t = raw.trim();
        if t.eq_ignore_ascii_case("## Status") || t.eq_ignore_ascii_case("# Status") {
            let mut buf = String::new();
            for l in &lines[i + 1..] {
                let lt = l.trim();
                if lt.is_empty() {
                    if buf.is_empty() {
                        continue;
                    }
                    break;
                }
                if lt.starts_with('#') {
                    break;
                }
                if !buf.is_empty() {
                    buf.push(' ');
                }
                buf.push_str(lt);
            }
            if !buf.is_empty() {
                return Some(buf);
            }
            continue;
        }
        if let Some(idx) = t.find("**Status**") {
            let rest = t[idx + "**Status**".len()..].trim_start();
            let rest = rest.strip_prefix(':').unwrap_or(rest).trim_start();
            let mut buf = match rest.find(" · **") {
                Some(sep) => rest[..sep].to_string(),
                None => rest.to_string(),
            };
            let mut j = i + 1;
            while j < lines.len() {
                let cont = lines[j];
                let lt = cont.trim();
                if lt.is_empty() || !(cont.starts_with(' ') || cont.starts_with('\t')) {
                    break;
                }
                buf.push(' ');
                buf.push_str(lt);
                j += 1;
            }
            let buf = buf.trim().to_string();
            if !buf.is_empty() {
                return Some(buf);
            }
        }
    }
    None
}

/// Classify a `Status:` field's prose. Case-insensitive; every spelling the real corpus writes
/// today (`Superseded by`, `**Superseded by […]**`, `Superseded IN PART by`, `Superseded in part
/// by`) is caught by finding the bare word and checking a short window after it for "in part" — the
/// corpus never hedges a FULL supersession with other qualifiers between the word and its target,
/// so anything containing "superseded" without "in part" nearby is treated as full.
fn classify_adr_status(status_text: &str) -> AdrAuthority {
    let lower = status_text.to_lowercase();
    let Some(idx) = lower.find("superseded") else {
        return AdrAuthority::Live;
    };
    let window_end = (idx + "superseded".len() + 24).min(lower.len());
    let window = &lower[idx..window_end];
    if window.contains("in part") || window.contains("in-part") {
        AdrAuthority::SupersededInPart
    } else {
        AdrAuthority::SupersededFully
    }
}

/// Resolve an ADR id to its filename among `adr_files`, mirroring `record_resolves`'s ADR branch
/// (full stamp, or legacy `ADR-00NN`) but returning the MATCH rather than a bool — needed here to
/// go read the target's own Status line. `record_resolves` stays the one EXISTENCE check this repo
/// asks anywhere else; this is its filename-returning twin, used only where the caller needs the
/// file.
fn resolve_adr_filename<'a>(id: &str, adr_files: &'a [String]) -> Option<&'a String> {
    let rest = id.strip_prefix("ADR-")?;
    if rest.len() == 4 && rest.chars().all(|c| c.is_ascii_digit()) {
        let want = format!("{}-", rest);
        return adr_files.iter().find(|f| f.starts_with(&want));
    }
    let is_stamp = rest.len() == 15
        && rest.as_bytes()[8] == b'-'
        && rest.chars().enumerate().all(|(i, c)| i == 8 || c.is_ascii_digit());
    if is_stamp {
        return adr_files.iter().find(|f| f.starts_with(id) || f.starts_with(rest));
    }
    None
}

/// Read every `docs/adr/*.md` file's content — the TARGET side of §24, distinct from
/// `load_record_corpus`'s filenames-only list (that list proves existence; this one is read for
/// authority). A missing directory yields an empty corpus (tolerant, like `load_model`).
pub(crate) fn load_adr_status_corpus(root: &std::path::Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(root.join("docs/adr")) {
        for e in rd.flatten() {
            let p = e.path();
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if name.ends_with(".md") {
                    if let Ok(content) = fs::read_to_string(&p) {
                        out.push((name.to_string(), content));
                    }
                }
            }
        }
    }
    out.sort();
    out
}

/// §24: a citation of a SUPERSEDED ADR, read as live authority anywhere in the instruction-surface
/// corpus, sends the next session to follow a record that no longer speaks — #477's motivating
/// class, verbatim: CLAUDE.md's architecture summary named `ADR-20260731-061609` for hosting after
/// that ADR had been superseded, and the founder corrected a session by hand on a fact the repo
/// should have supplied.
///
/// FULLY superseded is an ERROR: the citing text is flatly wrong about who is in charge. Superseded
/// IN PART is a WARNING: some of the cited ADR may still hold, and there is no mechanical way to
/// tell whether the citing clause lands on the surviving part or the reversed one, so this is a
/// surfaced finding for a human read rather than an automatic error (#712's own instruction).
///
/// EXEMPTION — reusing `decision-superseded-authority`'s DESIGN rather than inventing a second
/// mechanism (never a `_exempt.yaml`-style allowlist for this rule, never a blanket path
/// exemption): a citation sitting inside a logical UNIT (`logical_units`, the exact
/// paragraph/list-item/table-row join that rule uses) that ALSO narrates the supersession —
/// contains "supersed" anywhere in the joined text — is a citation ABOUT the history, not a live
/// pointer, and is not reported. `docs/claude/sessions/gates.md`'s own "measured cost" paragraph is
/// exactly this shape: it cites `ADR-20260731-061609` explaining, in the same sentence, that the
/// citation used to be stale — narration, not authority, and green without a manual exemption entry.
///
/// Scoped to the UNIT rather than the narrower CLAUSE `decision-superseded-authority` computes: an
/// ADR id is an unambiguous citation on its own (no citing-form disambiguation the way a bare
/// register key needs — `row OLD-ROW` versus a plain mention of `OLD-ROW` — because nothing else in
/// this corpus looks like `ADR-YYYYMMDD-HHMMSS`), so the residual risk this trades for the simpler
/// join is only an over-broad exemption from an unrelated "superseded" elsewhere in the same
/// paragraph; narrowing to a clause is future work if the real corpus ever shows that miss.
pub(crate) fn validate_no_superseded_adr_is_cited_as_authority(
    files: &[(String, String)],
    adr_files: &[(String, String)],
) -> Vec<Issue> {
    let adr_filenames: Vec<String> = adr_files.iter().map(|(f, _)| f.clone()).collect();
    let mut issues = Vec::new();
    for (path, content) in files {
        let units = logical_units(content);
        let mut reported: BTreeSet<(String, usize)> = BTreeSet::new();
        for (id, line) in extract_citations(content) {
            if !id.starts_with("ADR-") {
                continue; // scope: ADR ids only — a PROP is an option space, never a supersedable record
            }
            let Some(fname) = resolve_adr_filename(&id, &adr_filenames) else {
                continue; // unresolved: §23 already reports this, never duplicated here
            };
            let Some((_, target_content)) = adr_files.iter().find(|(f, _)| f == fname) else {
                continue;
            };
            let status_text = adr_status_text(target_content);
            let authority = status_text.as_deref().map(classify_adr_status).unwrap_or(AdrAuthority::Unparseable);
            if matches!(authority, AdrAuthority::Live) {
                continue;
            }
            let exempt = units
                .iter()
                .find(|u| u.spans.iter().any(|&(_, l)| l == line))
                .map(|u| u.text.to_lowercase().contains("supersed"))
                .unwrap_or(false);
            if exempt || !reported.insert((id.clone(), line)) {
                continue;
            }
            let superseding = status_text
                .as_deref()
                .map(extract_citations)
                .unwrap_or_default()
                .into_iter()
                .map(|(i, _)| i)
                .find(|i| i != &id);
            match authority {
                AdrAuthority::SupersededFully => issues.push(err(
                    "adr-superseded-citation",
                    format!("{}:{}", path, line),
                    format!(
                        "cites `{}` as live authority, but its own Status line says it is SUPERSEDED{}. Name the current record instead.",
                        id,
                        superseding.map(|s| format!(" (by `{}`)", s)).unwrap_or_default(),
                    ),
                )),
                AdrAuthority::SupersededInPart => issues.push(warn(
                    "adr-superseded-citation-in-part",
                    format!("{}:{}", path, line),
                    format!(
                        "cites `{}` as live authority, but its own Status line says it is SUPERSEDED IN PART{} — check whether the cited point is one of the parts still standing, or narrow/replace the citation.",
                        id,
                        superseding.map(|s| format!(" (by `{}`)", s)).unwrap_or_default(),
                    ),
                )),
                AdrAuthority::Unparseable => issues.push(warn(
                    "adr-status-unparseable",
                    format!("{}:{}", path, line),
                    format!(
                        "cites `{}`, whose own Status line (docs/adr/{}) this rule could not locate or parse — so it cannot tell whether the citation is still live. Fix the Status line's shape (a `## Status` heading, or a `**Status**:` inline field), or check by hand.",
                        id, fname,
                    ),
                )),
                AdrAuthority::Live => unreachable!(),
            }
        }
    }
    issues
}
