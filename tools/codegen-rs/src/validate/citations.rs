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
