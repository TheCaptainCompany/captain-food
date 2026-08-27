//! §17 — the warning RATCHET: the validator owns its own baseline.
//!
//! `make validate` must be **0 errors and no NEW warning**. The "no new warning" half used to be a
//! NUMBER WRITTEN IN PROSE in CLAUDE.md ("main carries 43 -- command-no-mutation x13, ..."). A number
//! in prose goes stale the moment a spec lands, and the prose beside it (correctly) told every session
//! to re-measure the baseline against a pristine `main` worktree before comparing. That cost a full
//! extra validator run PER SESSION -- paid four times in one day by four different agents, three of
//! whom said some version of "the pinned number looked wrong, so I re-derived it".
//!
//! So the number stops being prose and becomes an artifact the gate asserts:
//! `tools/codegen-rs/warning-baseline.json` holds the per-rule warning histogram, and every
//! `--check` run compares the live profile against it. The comparison is EXACT in both directions:
//!
//! * live > committed (or a warning KIND that is not in the baseline) -- a regression: the change
//!   widened the warning surface. If that is deliberate, refresh the artifact in the SAME commit
//!   (`make warning-baseline`) so the diff carries the +1 and the PR body can say why.
//! * live < committed -- an improvement: refresh the artifact so the ratchet tightens and the next
//!   session cannot re-spend the freed budget silently.
//!
//! Exact-match both ways is what makes the number un-stale-able: a stale baseline fails the gate
//! instead of misleading a reader. Nothing anywhere needs to restate the count in prose.
//!
//! The check covers the WHOLE file, prose included — `doc` must be verbatim what this module writes.
//! An artifact that forbids hand-editing in a field nothing asserts is just a comment, and it was
//! already wrong when it first landed (it named section 16 after the renumbering to 17).

use crate::*;

/// The committed ratchet, relative to the repo root (derived from `--specs`).
pub(crate) const WARNING_BASELINE_PATH: &str = "tools/codegen-rs/warning-baseline.json";

/// The `doc` field baked into the artifact: whoever opens the file learns how to change it without
/// leaving the file. It is ASSERTED, not just written (see `parse_warning_baseline`) — an unchecked
/// self-description is a comment, and a comment is the thing this section exists to abolish.
pub(crate) const BASELINE_DOC: &str = "GENERATED warning ratchet (validator section 17) -- do not hand-edit. \
`make validate` fails when the live per-rule warning histogram differs from this one, in EITHER \
direction. To change it, run `make warning-baseline` and commit the result in the SAME commit as \
the spec/code change that moved the number; a deliberate INCREASE is recorded by that diff plus one \
line in the PR body saying why the warning is accepted.";

/// The per-rule warning histogram — the whole ratchet state. Locations are deliberately NOT part of
/// it: they churn on every rename, and a per-location ledger would be a merge-conflict generator
/// without catching anything a per-kind count misses at this granularity.
pub(crate) type WarningProfile = BTreeMap<String, usize>;

/// Warning kinds DELIBERATELY OUTSIDE the ratchet, because their presence depends on the HOST
/// rather than on the repository.
///
/// The ratchet's whole contract is byte-stability: the artifact is a claim about what THIS TREE
/// produces, and exact-match-both-ways is what makes it un-stale-able. A signal that fires on one
/// machine and not another has no stable value to commit, and routing one in makes the artifact
/// lie in whichever direction the author's machine happened to point.
///
/// `decision-citation-corpus-unreadable` is the case that earned this list, and it earned it by
/// being wrong in both directions at once (review #35 of PR #679). It fires when `git ls-files`
/// exits non-zero — `fatal: detected dubious ownership in repository` on a bind-mounted or
/// differently-owned tree, a `git archive` extraction, a container stage that drops `.git`. Before
/// this list: the kind was absent from the baseline, so the first such run scored it
/// `0 -> 1 (NEW warning kind)` and `make validate` FAILED — a comment two files over saying "Not
/// an error" over a code path that exits 1, with a message naming neither git nor the corpus. And
/// the remedy that message prints (`make warning-baseline`, which CLAUDE.md also prescribes)
/// commits a baseline asserting *the citation gate checked nothing*, which then reds in the
/// opposite direction (`kind eliminated`) on every host where git works. Both a false red and a
/// trap for the reader who obeys it.
///
/// The posture is fail-OPEN and LOUD: the warning still prints in the `checks:` listing, so "did
/// not look" and "found nothing" still read differently. It just does not enter an artifact whose
/// value it cannot stably have. Adding a kind here is a decision — it removes that kind from the
/// only gate that counts warnings — so the list is asserted by
/// `only_host_dependent_warnings_are_exempt_from_the_ratchet` rather than merely written.
pub(crate) const RATCHET_EXEMPT: [&str; 1] = ["decision-citation-corpus-unreadable"];

/// The kinds whose CONDITION is tree-caused but whose MEASUREMENT depends on the corpus being fully
/// read — so on a run where it was not, their counts are UNDER-COUNTS rather than facts.
///
/// TWO DIFFERENT SHORTFALLS, and this doc used to describe only the first. Where `git ls-files` does
/// not answer at all, nothing is computed and the counts are zero. Where the corpus is only PARTLY
/// read, the counts ARE computed and are lower bounds. Both are handled by FLOORING at the committed
/// value in `check_warning_baseline` — see the note there on why a floor and not a replacement.
///
/// This is the trap `RATCHET_EXEMPT` closes, re-entering one kind over. Both of these are
/// deliberately NOT exempt, on the correct reasoning that non-UTF-8 bytes and an out-of-allowlist
/// extension fail identically on every host and therefore have a stable value to commit. True of
/// the condition; false of the emission. Whether they are computed at all depends on
/// `git ls-files` succeeding — the exact host list `RATCHET_EXEMPT`'s own doc comment names: a
/// dubious-ownership bind mount, a `git archive` extraction, a container stage with no `.git`.
///
/// So once either is legitimately baselined at N>0 — a tracked `.claude/**` file outside the
/// allowlist, or a committed latin-1 byte accepted with `make warning-baseline` — the next run on
/// such a host reports 0, the ratchet files it under `better`, and `make validate` exits 1 with
/// `N -> 0 (kind eliminated)`. Obeying the printed remedy commits a baseline of 0, which then reds
/// `0 -> N` on every host where git works: a false red AND a trap for the reader who obeys it,
/// verbatim the sentence above, arriving through the kinds it excluded.
///
/// Latent when written (the committed artifact carries none of these kinds), which is why it is closed
/// now: it arms itself on a later, unrelated commit, and the run that trips it looks like a
/// validator regression on a tree nobody touched. The fix is this file's own vocabulary applied one
/// level down — "did not look" is not "found nothing", so on such a run these kinds are neither
/// compared nor rewritten. (Review #80 of PR #679.)
pub(crate) const CORPUS_DERIVED_KINDS: [&str; 2] = [
    "decision-citation-file-not-utf8",
    "decision-citation-file-out-of-corpus",
    // `decision-superseded-authority` WAS the third member, and its arrival and departure are both
    // the level<->list coupling working as built. It joined when reviews #81/#82 moved the rule to
    // `warn` (an error never enters `warning_profile`, so the list was complete when written and
    // incomplete two rounds later -- the sequencing defect reviews #84/#86 pinned with the
    // coupling assertion). It LEFT on 2026-08-27 when the founder decided `CITATION-RULE-LEVEL`
    // to `err` (ADR-20260827-081500): as an error it cannot appear in the profile, so keeping it
    // here would have been the reverse lie -- an unmeasured-kind floor over a kind the ratchet can
    // never see. The coupling test asserts membership tracks the level in BOTH directions.
];

/// HOW SHORT THIS RUN'S CORPUS SCAN FELL, which is not a boolean and was one for four rounds.
///
/// `main.rs` collapsed the two causes into `corpus_incomplete = !readable || !unread.is_empty()`
/// and fed the union to both the ratchet floor and the `--write-warning-baseline` refusal. They are
/// different shortfalls and the difference is per-KIND:
///
/// * `Nothing` — `readable == false`. `claude_citation_corpus` returns with every vector cleared,
///   so NOTHING was measured and all three corpus-derived kinds are absent rather than low.
/// * `Partial` — git answered, but at least one tracked file could not be read (a sparse checkout,
///   a dangling tracked symlink, a permission drop). The counts ARE computed. Two of them are lower
///   bounds — `decision-citation-file-not-utf8` and the citation findings lose exactly the files
///   that could not be read — but `decision-citation-file-out-of-corpus` is **EXACT**, because
///   `skipped_ext` is pushed BEFORE the `read_to_string` attempt. An unreadable file never leaves
///   it.
///
/// FLOORING THE EXACT KIND SUPPRESSES A REAL ELIMINATION, and this file's own docstring below
/// states the invariant that makes it wrong while the code did it anyway. Once that kind is
/// legitimately baselined at N>0 — the state `claude_citation_corpus` calls "a deliberate,
/// baseline-moving act" — an author who FIXES one of those files sees `N -> N-1` floored back to
/// `N` and `make validate` GREEN on any host with a single unreadable corpus file, then CI reds on
/// the change they were told was clean. And they cannot clear it locally, because the write path
/// refused on the same collapsed bool. (Review #91 of PR #679.)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CorpusShortfall {
    None,
    Partial,
    Nothing,
}

impl CorpusShortfall {
    /// Classify a corpus scan. The two arguments are the only inputs that may decide this, and
    /// that is enforced by the SIGNATURE rather than by a gate: `unread_tree` and `skipped_ext` are
    /// TREE-caused -- they drop the same files on every host, so they narrow the corpus without
    /// making its counts host-dependent, and including one would suppress the ratchet permanently
    /// the moment a single file qualified. That was previously held by a test reading the
    /// predicate's SOURCE TEXT in `main.rs`, whose own docstring admitted it "catches the disjunct
    /// being DELETED ... it cannot catch it being rewritten to something wrong". A function that
    /// cannot see the tree vectors cannot be rewritten to consult them, which is CLAUDE.md's
    /// compiler-first rule applied to the gate that was standing in for it. (Review #91.)
    pub(crate) fn from_scan(readable: bool, unread_is_empty: bool) -> Self {
        match (readable, unread_is_empty) {
            // `git ls-files` refused, or review #61's empty-corpus early return -- every vector
            // comes back cleared, so nothing was measured.
            (false, _) => Self::Nothing,
            // Git answered and listed everything, but the filesystem would not hand back at least
            // one tracked file: a sparse checkout, a dangling tracked symlink, a permission drop.
            // HOST-caused, so the counts that depend on reading are lower bounds on this machine
            // and complete on another.
            (true, false) => Self::Partial,
            (true, true) => Self::None,
        }
    }

    /// The kinds this run could not count FULLY. The floor may only ever raise a spuriously low
    /// count, so a kind belongs here exactly when this shortfall can depress it.
    pub(crate) fn unmeasured(self) -> &'static [&'static str] {
        match self {
            Self::None => &[],
            // Everything the unread files could have contributed to -- but NOT the extension
            // filter, which ran before the read attempt and is therefore exact.
            // Only `not-utf8`: the extension filter is exact (runs before the read attempt), and
            // `decision-superseded-authority` is an ERROR since 2026-08-27 -- errors never enter
            // the profile, so there is no count to floor.
            Self::Partial => &["decision-citation-file-not-utf8"],
            Self::Nothing => &CORPUS_DERIVED_KINDS,
        }
    }

    /// Whether a baseline may be MINTED from this run. Only `Nothing` refuses: with the counts
    /// absent, `render_warning_baseline` would commit a 0 for kinds this host never took, and the
    /// artifact is permanent. On a PARTIAL read the counts exist and the floor gives the right
    /// answer, so refusing there blocks `make warning-baseline` for work that has nothing to do
    /// with the corpus -- on a host condition the author may not control, with no opt-out, while
    /// CLAUDE.md requires the refreshed artifact in the SAME commit as the change that moved the
    /// surface. (Review #91.)
    pub(crate) fn may_mint_a_baseline(self) -> bool {
        self != Self::Nothing
    }
}

/// The live profile of a validation run. Takes ALL issues (the spec validator's plus §13 proposal
/// hygiene, exactly what `checks: N error(s), M warning(s)` counts) and keeps the warnings that
/// the ratchet governs — see `RATCHET_EXEMPT` for the ones it deliberately does not.
pub(crate) fn warning_profile(issues: &[Issue]) -> WarningProfile {
    let mut out: WarningProfile = BTreeMap::new();
    for i in issues
        .iter()
        .filter(|i| i.level == Level::Warning)
        .filter(|i| !RATCHET_EXEMPT.contains(&i.rule))
    {
        *out.entry(i.rule.to_string()).or_insert(0) += 1;
    }
    out
}

/// Canonical JSON for the artifact: `doc`, `total`, then the histogram sorted by rule (BTreeMap), so
/// the file is byte-stable across runs and its diff reads as "+1 event-not-projected".
pub(crate) fn render_warning_baseline(profile: &WarningProfile) -> String {
    let mut s = String::from("{\n");
    s.push_str(&format!("  \"doc\": {},\n", json_string(BASELINE_DOC)));
    s.push_str(&format!("  \"total\": {},\n", profile.values().sum::<usize>()));
    s.push_str("  \"by_rule\": {\n");
    let last = profile.len().saturating_sub(1);
    for (n, (rule, count)) in profile.iter().enumerate() {
        let comma = if n == last { "" } else { "," };
        s.push_str(&format!("    {}: {}{}\n", json_string(rule), count, comma));
    }
    s.push_str("  }\n}\n");
    s
}

fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// Read the committed ratchet. EVERY field is checked, so that no byte of the artifact is outside the
/// ratchet: `total` is cross-checked against the histogram, and `doc` must be verbatim the text this
/// tool writes. The `doc` check is not decoration — `doc` is the only field a human reads and the only
/// one that tells them how to change the file, so a hand-patched or stale `doc` misdirects every future
/// reader while `by_rule`/`total` stay perfectly green. It shipped wrong on day one (it pointed at
/// validator section 16 after this section was renumbered to 17), which is precisely the "a number in
/// prose goes stale" defect this section exists to abolish, one level up.
pub(crate) fn parse_warning_baseline(text: &str) -> Result<WarningProfile, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
    let doc = v
        .get("doc")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "missing string field `doc`".to_string())?;
    if doc != BASELINE_DOC {
        return Err("`doc` is not the text this tool writes — the file is hand-edited or stale".to_string());
    }
    let by_rule = v
        .get("by_rule")
        .and_then(|x| x.as_object())
        .ok_or_else(|| "missing object field `by_rule`".to_string())?;
    let mut profile: WarningProfile = BTreeMap::new();
    for (rule, count) in by_rule {
        let n = count
            .as_u64()
            .ok_or_else(|| format!("by_rule.{rule} is not a non-negative integer"))?;
        if n == 0 {
            return Err(format!("by_rule.{rule} is 0 — drop the entry instead"));
        }
        profile.insert(rule.clone(), n as usize);
    }
    let total = v
        .get("total")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| "missing integer field `total`".to_string())?;
    let sum: usize = profile.values().sum();
    if total as usize != sum {
        return Err(format!("`total` is {total} but `by_rule` sums to {sum}"));
    }
    Ok(profile)
}

/// One rule whose count moved, with the direction implied by `committed` vs `live`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct BaselineDelta {
    pub(crate) rule: String,
    pub(crate) committed: usize,
    pub(crate) live: usize,
}

/// The verdict of comparing a live run against the committed ratchet.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct BaselineDiff {
    /// Kinds that grew, including kinds absent from the baseline (`committed: 0`) — regressions.
    pub(crate) worse: Vec<BaselineDelta>,
    /// Kinds that shrank or disappeared (`live: 0`) — improvements.
    pub(crate) better: Vec<BaselineDelta>,
}

impl BaselineDiff {
    pub(crate) fn is_clean(&self) -> bool {
        self.worse.is_empty() && self.better.is_empty()
    }
}

pub(crate) fn diff_warning_baseline(committed: &WarningProfile, live: &WarningProfile) -> BaselineDiff {
    let mut diff = BaselineDiff::default();
    let rules: BTreeSet<&String> = committed.keys().chain(live.keys()).collect();
    for rule in rules {
        let c = committed.get(rule).copied().unwrap_or(0);
        let l = live.get(rule).copied().unwrap_or(0);
        let delta = BaselineDelta { rule: rule.clone(), committed: c, live: l };
        match l.cmp(&c) {
            std::cmp::Ordering::Greater => diff.worse.push(delta),
            std::cmp::Ordering::Less => diff.better.push(delta),
            std::cmp::Ordering::Equal => {}
        }
    }
    diff
}

/// The failure message — it must be actionable on its own, because the reader is mid-gate and the
/// whole point of this section is that they do not have to go re-derive anything.
pub(crate) fn render_baseline_failure(diff: &BaselineDiff, live: &WarningProfile) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "\n✗ warning baseline drift — {} does not match this run ({} warning(s) live).\n",
        WARNING_BASELINE_PATH,
        live.values().sum::<usize>()
    ));
    for d in &diff.worse {
        let what = if d.committed == 0 { " (NEW warning kind)" } else { "" };
        s.push_str(&format!("  + {}: {} -> {}{}\n", d.rule, d.committed, d.live, what));
    }
    for d in &diff.better {
        let what = if d.live == 0 { " (kind eliminated)" } else { "" };
        s.push_str(&format!("  - {}: {} -> {}{}\n", d.rule, d.committed, d.live, what));
    }
    if !diff.worse.is_empty() {
        s.push_str("  This change WIDENS the warning surface. Fix it — or, if the warning is\n");
        s.push_str("  deliberately accepted, run `make warning-baseline`, commit the refreshed\n");
        s.push_str("  artifact in the SAME commit, and say in the PR body why it is accepted.\n");
    } else {
        s.push_str("  This change NARROWS the warning surface — tighten the ratchet: run\n");
        s.push_str("  `make warning-baseline` and commit the refreshed artifact in the SAME commit.\n");
    }
    s
}

/// Load + compare in one step; `Ok(())` means the ratchet holds. A MISSING or malformed artifact is
/// a gate failure too, not a silent pass — an absent ratchet is exactly the state this section exists
/// to make impossible.
/// `unmeasured` names kinds this run could not count fully (see `CORPUS_DERIVED_KINDS`). Their live
/// value is FLOORED at the committed one rather than replaced by it, so an under-count cannot read
/// as an elimination while a genuine INCREASE still blocks. An empty slice is the ordinary case.
///
/// A FLOOR, NOT A CARRY-FORWARD, and the difference is the whole gate. The first version replaced
/// the live value outright, which is sound only where NOTHING was measured — true of
/// `readable == false`, where `claude_citation_corpus` returns with every vector cleared. It is
/// FALSE on the partial-read path (`readable == true`, `unread` non-empty), where the counts are
/// still computed and are merely LOWER BOUNDS: `skipped_ext` is pushed BEFORE the `read_to_string`
/// attempt, so it is exact; `unread_tree` and the citation findings lose only the files that could
/// not be read. An unread file can therefore only REDUCE a count, never inflate one.
///
/// So a symmetric replacement made the ratchet SILENTLY NON-BLOCKING for three kinds on any host
/// with one dangling tracked symlink, one sparse-checkout gap or one root-owned file anywhere in the
/// corpus: adding an out-of-allowlist `.claude/**` file would have scored clean — against the
/// promise in `claude_citation_corpus` that doing so "becomes a deliberate, baseline-moving act" —
/// and a genuinely new stale citation would have landed with the ratchet quiet. The hazard is
/// one-directional, so the remedy is too. (Review #87 of PR #679, on the fix from review #84.)
/// Raise `live`'s entries for `unmeasured` to the committed baseline's values, so a count this run
/// could only under-take is not written down as the truth.
///
/// SHARED BY THE COMPARE AND WRITE PATHS ON PURPOSE. The write path used to REFUSE outright on any
/// shortfall, which is right where nothing was measured and wrong on a partial read: there the
/// counts exist as lower bounds, the floor is exactly the correction they need, and refusing
/// instead blocked `make warning-baseline` on that host for ANY change -- including one made for an
/// unrelated warning kind, with CLAUDE.md requiring the refreshed artifact in the same commit and
/// the printed remedy ("fix the checkout") outside the author's control. Two call sites deriving
/// the same correction separately is how they drift; one function is the fix. (Review #91.)
pub(crate) fn floor_unmeasured(live: &WarningProfile, committed: &WarningProfile, unmeasured: &[&str]) -> WarningProfile {
    let mut effective = live.clone();
    for kind in unmeasured {
        // FLOOR, not replace: `max(live, committed)`. Where nothing was measured the live value is
        // 0 and this is identical to carrying the committed value forward; where the count is a
        // lower bound it suppresses only the spurious DECREASE. An increase survives and still reds.
        let c = committed.get(*kind).copied().unwrap_or(0);
        let l = live.get(*kind).copied().unwrap_or(0);
        match l.max(c) {
            0 => {
                effective.remove(*kind);
            }
            n => {
                effective.insert((*kind).to_string(), n);
            }
        }
    }
    effective
}

/// The kinds `floor_unmeasured` would RAISE, as `(kind, live, committed)` -- i.e. where the
/// committed value wins over a lower live one. Split out so both paths that floor can SAY they
/// floored: issue #685's defect was `--write-warning-baseline` flooring on a partial read, writing
/// a byte-identical artifact, and printing `✓ wrote ...` -- an author who had genuinely FIXED a
/// baselined finding saw success locally and an unclearable `N -> N-1` red in CI, with the printed
/// remedy re-writing the same bytes. The floor is correct (a lower bound must not be written down
/// as the truth); what was missing is the sentence naming what it did.
pub(crate) fn floor_raises(live: &WarningProfile, committed: &WarningProfile, unmeasured: &[&str]) -> Vec<(String, usize, usize)> {
    let mut raised = Vec::new();
    for kind in unmeasured {
        let c = committed.get(*kind).copied().unwrap_or(0);
        let l = live.get(*kind).copied().unwrap_or(0);
        if c > l {
            raised.push(((*kind).to_string(), l, c));
        }
    }
    raised
}

/// Read and parse the committed baseline, with the two failure messages the callers print.
pub(crate) fn read_committed_baseline(root: &std::path::Path) -> Result<WarningProfile, String> {
    let path = root.join(WARNING_BASELINE_PATH);
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("\n✗ cannot read {}: {e}\n  run `make warning-baseline` to (re)create it.\n", path.display()))?;
    parse_warning_baseline(&text)
        .map_err(|e| format!("\n✗ {}: {e}\n  run `make warning-baseline` to rewrite it.\n", path.display()))
}

pub(crate) fn check_warning_baseline(
    root: &std::path::Path,
    live: &WarningProfile,
    unmeasured: &[&str],
) -> Result<(), String> {
    let committed = read_committed_baseline(root)?;
    let effective = floor_unmeasured(live, &committed, unmeasured);
    let diff = diff_warning_baseline(&committed, &effective);
    if diff.is_clean() {
        Ok(())
    } else {
        Err(render_baseline_failure(&diff, &effective))
    }
}
