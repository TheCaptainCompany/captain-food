use crate::*;

// ─── §24 — the STATUS.md journal-split gate (docs/STATUS.md + docs/status/**, #659) ─────────────
//
// Binds the three founder-specified assertions from #659 that bind to a REAL, present artifact on
// the corpus that actually landed via #665 (merged 2026-08-21): A2 (no journal-entry opener left
// in `docs/STATUS.md`), A3 (every entry sits in the ISO-week file its own parsed date derives) and
// A5 (the validator derives every count itself; a written number is an input to check, never a
// source of truth). #659's A1 index-row term and A4 gate a rendered "recent-changes index" that
// was drafted on an abandoned branch (`3a207eb7`, never merged) and never shipped on `main` — #665
// shipped a simpler design (a bare link list, no per-entry rendering) instead of gating the index,
// so there is no index artifact left to check. Building A1's index term or A4 here would mean
// inventing a new `STATUS.md` index format inside a validator PR with no design review of its own
// (coordinator decision on #659/#711, 2026-08-28). Scope fence (the issue's own): `docs/STATUS.md`
// + `docs/status/**` ONLY — no `DECISIONS.md`, no decision-register surface.
//
// Errors, not warnings (farley): the corpus is green on all three today, so a red here means the
// STATE broke, never the gate.
//
// The opener regex (measured 220/220 on the real corpus, zero unmatched):
// `^>\s*[^*]{0,12}\*\*(\d{4}-\d{2}-\d{2})`. A journal entry is a blank-line-delimited paragraph
// whose FIRST line is the opener; continuation lines keep the `> ` blockquote prefix but are never
// themselves checked against the opener shape — they routinely carry unrelated bold dates in prose
// (e.g. `docs/status/journal-2026-W34.md:1040`, citing an earlier decision date mid-paragraph).
// Parsing is STRUCTURAL (paragraph boundaries), never positional: journal files are newest-first,
// and the eight deliberate local date inversions in the real corpus must never trip a check that
// assumes order.

/// docs/STATUS.md — the one boot file this gate makes a LOUD failure on absence/unreadability
/// (assertion 2's "positive absence proof": a missing file is a hard error, never a silent "zero
/// entry-openers found").
pub(crate) fn load_status_file(root: &std::path::Path) -> Result<(String, String), Issue> {
    const REL: &str = "docs/STATUS.md";
    fs::read_to_string(root.join(REL)).map(|c| (REL.to_string(), c)).map_err(|e| {
        err(
            "status-md-unreadable",
            REL.to_string(),
            format!(
                "docs/STATUS.md could not be read ({e}) -- every journal-split check (A2/A3/A5) \
                 depends on it; a missing or unreadable boot file is a loud failure here, never a \
                 silent \"nothing found\"."
            ),
        )
    })
}

/// Every `docs/status/journal-*.md`, sorted for determinism. Tolerant `read_dir` (mirrors
/// `load_proposal_dir`/`load_dispatch_files`) — a missing `docs/status/` yields an empty corpus,
/// same posture as every other repo-text loader in this validator; only `STATUS.md` itself is a
/// hard failure (see `load_status_file`).
pub(crate) fn load_journal_files(root: &std::path::Path) -> Vec<(String, String)> {
    let dir = root.join("docs/status");
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        let mut paths: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("journal-") && n.ends_with(".md"))
                    .unwrap_or(false)
            })
            .collect();
        paths.sort();
        for p in paths {
            if let (Some(name), Ok(content)) = (p.file_name().and_then(|n| n.to_str()), fs::read_to_string(&p)) {
                out.push((format!("docs/status/{}", name), content));
            }
        }
    }
    out
}

/// The declared ISO `(year, week)` a `docs/status/journal-YYYY-Www.md` path names, from its OWN
/// filename — the thing assertion 3 checks every entry against.
fn parse_journal_filename(path: &str) -> Option<(i64, u32)> {
    let re = regex::Regex::new(r"journal-(\d{4})-W(\d{2})\.md$").unwrap();
    let caps = re.captures(path)?;
    Some((caps[1].parse().ok()?, caps[2].parse().ok()?))
}

/// The full opener shape, capturing the date. Compiled once per call site, not per line — these
/// files are a few hundred KB at most, so recompiling per call costs nothing measurable.
fn opener_regex() -> regex::Regex {
    regex::Regex::new(r"^>\s*[^*]{0,12}\*\*(\d{4})-(\d{2})-(\d{2})").unwrap()
}

/// The opener's PREFIX shape without the date — a blockquote line that opens a bold run within the
/// same 12-character budget. A first-paragraph-line matching this but NOT the full `opener_regex`
/// is a line that tried to be an entry opener and failed; per the dispatch, that is a LOUD error
/// (`status-journal-opener-unmatched`), never a silent skip.
fn opener_lookalike_regex() -> regex::Regex {
    regex::Regex::new(r"^>\s*[^*]{0,12}\*\*").unwrap()
}

/// Blank-line-delimited paragraphs as `(first_line_no, first_line)` — ONLY the first line of each
/// paragraph is ever a candidate journal-entry opener. 1-based line numbers, matching every other
/// `path:line` location in this validator.
fn paragraph_openers(content: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut in_block = false;
    for (i, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            in_block = false;
            continue;
        }
        if !in_block {
            out.push((i + 1, line));
            in_block = true;
        }
    }
    out
}

// ─── Proleptic-Gregorian date arithmetic (no date crate in this workspace) ───────────────────────
// Howard Hinnant's constant-time civil-calendar algorithms
// (http://howardhinnant.github.io/date_algorithms.html), the standard reference implementation.
// `civil_from_days` round-trips `days_from_civil`, which is how `is_valid_date` catches a
// non-existent date (`2026-02-30`, `2026-13-01`) from a regex capture that only constrains digit
// COUNT, never calendar validity.

fn floor_div(a: i64, b: i64) -> i64 {
    let q = a / b;
    let r = a % b;
    if (r != 0) && ((r < 0) != (b < 0)) { q - 1 } else { q }
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = floor_div(y, 400);
    let yoe = y - era * 400; // [0, 399]
    let mp = (m + 9) % 12; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = floor_div(z, 146097);
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `true` only for a date that actually exists on the proleptic Gregorian calendar — a round trip
/// through `days_from_civil`/`civil_from_days` is a stronger check than a month-length table (and
/// needs no leap-year special case of its own).
pub(crate) fn is_valid_date(y: i64, m: i64, d: i64) -> bool {
    (1..=12).contains(&m) && (1..=31).contains(&d) && civil_from_days(days_from_civil(y, m, d)) == (y, m, d)
}

/// 1 (Monday) .. 7 (Sunday). 1970-01-01 (`days_from_civil` epoch 0) was a Thursday (ISO weekday 4).
fn weekday_from_days(days: i64) -> i64 {
    let rem = days.rem_euclid(7);
    (rem + 3).rem_euclid(7) + 1
}

/// The ISO week-year's own last week number: December 28 always falls in it, by the ISO 8601
/// definition of "the week containing the year's first Thursday" — so applying the same
/// ordinal/weekday formula to December 28 needs no recursion or clamping to answer it directly.
fn last_iso_week_of_year(y: i64) -> u32 {
    let days = days_from_civil(y, 12, 28);
    let ordinal = days - days_from_civil(y, 1, 1) + 1;
    let weekday = weekday_from_days(days);
    ((ordinal - weekday + 10) / 7) as u32
}

/// The ISO 8601 `(week-year, week)` a calendar date belongs to. `d` must already have passed
/// `is_valid_date`.
pub(crate) fn iso_year_week(y: i64, m: i64, d: i64) -> (i64, u32) {
    let days = days_from_civil(y, m, d);
    let ordinal = days - days_from_civil(y, 1, 1) + 1;
    let weekday = weekday_from_days(days);
    let week = floor_div(ordinal - weekday + 10, 7);
    if week < 1 {
        (y - 1, last_iso_week_of_year(y - 1))
    } else if week as u32 > last_iso_week_of_year(y) {
        (y + 1, 1)
    } else {
        (y, week as u32)
    }
}

/// Every VALID entry-opener date in `content`, as `(line_no, y, m, d)` — the derived truth every
/// other check in this file compares against. An opener-looking line whose date does not validate
/// is deliberately NOT counted here (it is reported once, by `validate_journal_entries_own_week`,
/// as `status-journal-entry-date-invalid` — counting it here too would double-report the same
/// defect under two rules).
fn derived_entry_dates(content: &str) -> Vec<(usize, i64, i64, i64)> {
    let opener = opener_regex();
    paragraph_openers(content)
        .into_iter()
        .filter_map(|(line_no, first)| {
            let caps = opener.captures(first)?;
            let y: i64 = caps[1].parse().ok()?;
            let m: i64 = caps[2].parse().ok()?;
            let d: i64 = caps[3].parse().ok()?;
            is_valid_date(y, m, d).then_some((line_no, y, m, d))
        })
        .collect()
}

// ─── A2 — no journal-entry opener remains in docs/STATUS.md ─────────────────────────────────────

/// Fires on the FIRST offending entry (every match is reported, not just the first found — "fires
/// on the first offender" means one is already enough, not that this stops looking after one).
pub(crate) fn validate_no_journal_opener_in_status(path: &str, content: &str) -> Vec<Issue> {
    let opener = opener_regex();
    content
        .lines()
        .enumerate()
        .filter(|(_, l)| opener.is_match(l))
        .map(|(i, _)| {
            err(
                "status-journal-opener-in-status-md",
                format!("{}:{}", path, i + 1),
                "a dated journal-entry opener remains in docs/STATUS.md -- STATUS.md is durable \
                 state plus the journal index; write dated entries to the applicable \
                 docs/status/journal-YYYY-Www.md instead (see its own `## Journal` section)."
                    .into(),
            )
        })
        .collect()
}

// ─── A3 — every journal entry sits in the ISO-week file its own parsed date derives ─────────────

pub(crate) fn validate_journal_entries_own_week(files: &[(String, String)]) -> Vec<Issue> {
    let mut issues = Vec::new();
    let opener = opener_regex();
    let lookalike = opener_lookalike_regex();
    for (path, content) in files {
        let Some((decl_year, decl_week)) = parse_journal_filename(path) else {
            issues.push(err(
                "status-journal-filename-unparseable",
                path.clone(),
                "file name does not match `journal-YYYY-Www.md` -- its declared ISO week cannot \
                 be derived, so entries inside it cannot be checked against it."
                    .into(),
            ));
            continue;
        };
        for (line_no, first) in paragraph_openers(content) {
            let Some(caps) = opener.captures(first) else {
                // An opener-LOOKING line whose date fragment doesn't fully match: a loud error,
                // never a silent skip -- a malformed opener would otherwise vanish from every
                // count with no signal. A first line that isn't opener-shaped at all is simply not
                // a journal entry (header prose, etc.) and is correctly ignored.
                if lookalike.is_match(first) {
                    issues.push(err(
                        "status-journal-opener-unmatched",
                        format!("{}:{}", path, line_no),
                        "line opens a blockquote paragraph with an early bold run (looks like a \
                         journal-entry opener) but its date does not fully match the measured \
                         opener shape `^>\\s*[^*]{0,12}\\*\\*YYYY-MM-DD` -- fix the entry rather \
                         than let it silently fall out of every count."
                            .into(),
                    ));
                }
                continue;
            };
            let y: i64 = caps[1].parse().unwrap();
            let m: i64 = caps[2].parse().unwrap();
            let d: i64 = caps[3].parse().unwrap();
            if !is_valid_date(y, m, d) {
                issues.push(err(
                    "status-journal-entry-date-invalid",
                    format!("{}:{}", path, line_no),
                    format!(
                        "journal entry opener names {:04}-{:02}-{:02}, which is not a real \
                         calendar date.",
                        y, m, d
                    ),
                ));
                continue;
            }
            let (iso_y, iso_w) = iso_year_week(y, m, d);
            if (iso_y, iso_w) != (decl_year, decl_week) {
                issues.push(err(
                    "status-journal-entry-wrong-week",
                    format!("{}:{}", path, line_no),
                    format!(
                        "entry dated {:04}-{:02}-{:02} is ISO week {}-W{:02}, but sits in {} \
                         (declared {}-W{:02}).",
                        y, m, d, iso_y, iso_w, path, decl_year, decl_week
                    ),
                ));
            }
        }
    }
    issues
}

// ─── A5 — the validator derives every count itself; a written number is an input to check ───────
//
// Two declaration shapes, deliberately narrow so an unrelated "<N> entries" elsewhere in a journal
// file's prose (e.g. `docs/status/journal-2026-W34.md:457`, "`docs/adr/README.md` was still 13
// entries stale" -- about the ADR index, nothing to do with the journal) can never false-positive:
//   * "<N> journal entries"  -- a claim about the file it appears in, checked against that file's
//     OWN derived count.
//   * "<N> entries total"    -- a claim about the WHOLE corpus (wherever it appears -- STATUS.md or
//     any journal file), checked against the sum of every journal file's own derived count.
// If neither shape appears anywhere, there is nothing to check, and that is fine (assertion 5 is
// about not TRUSTING a written number, not about requiring one to exist).

fn declared_counts(content: &str, phrase: &str) -> Vec<(usize, usize)> {
    let re = regex::Regex::new(&format!(r"\b(\d{{1,4}})\s+{}\b", phrase)).unwrap();
    content
        .lines()
        .enumerate()
        .flat_map(|(i, line)| {
            re.captures_iter(line).filter_map(move |c| c[1].parse::<usize>().ok().map(|n| (i + 1, n)))
        })
        .collect()
}

/// Per-file "`<N> journal entries`" claims, checked against THAT file's own derived count.
pub(crate) fn validate_journal_declared_counts(files: &[(String, String)]) -> Vec<Issue> {
    let mut issues = Vec::new();
    for (path, content) in files {
        let derived = derived_entry_dates(content).len();
        for (line_no, declared) in declared_counts(content, "journal entries") {
            if declared != derived {
                issues.push(err(
                    "status-journal-declared-count-mismatch",
                    format!("{}:{}", path, line_no),
                    format!(
                        "declares {} journal entries but this file's own openers derive to {} -- \
                         a written number is an input to check, never a source of truth.",
                        declared, derived
                    ),
                ));
            }
        }
    }
    issues
}

/// "`<N> entries total`" claims across `docs/STATUS.md` and every journal file, checked against the
/// sum of every journal file's own derived count.
pub(crate) fn validate_declared_entries_total(
    status_file: &(String, String),
    journal_files: &[(String, String)],
) -> Vec<Issue> {
    let total: usize = journal_files.iter().map(|(_, c)| derived_entry_dates(c).len()).sum();
    let mut declarations: Vec<(String, usize, usize)> = declared_counts(&status_file.1, "entries total")
        .into_iter()
        .map(|(line_no, n)| (status_file.0.clone(), line_no, n))
        .collect();
    for (path, content) in journal_files {
        declarations.extend(
            declared_counts(content, "entries total").into_iter().map(|(line_no, n)| (path.clone(), line_no, n)),
        );
    }
    declarations
        .into_iter()
        .filter(|(_, _, declared)| *declared != total)
        .map(|(path, line_no, declared)| {
            err(
                "status-journal-declared-total-mismatch",
                format!("{}:{}", path, line_no),
                format!(
                    "declares {} entries total but the journal corpus (docs/status/journal-*.md) \
                     derives to {} across {} file(s).",
                    declared,
                    total,
                    journal_files.len()
                ),
            )
        })
        .collect()
}
