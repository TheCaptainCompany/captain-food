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

use crate::*;

/// The committed ratchet, relative to the repo root (derived from `--specs`).
pub(crate) const WARNING_BASELINE_PATH: &str = "tools/codegen-rs/warning-baseline.json";

/// The `doc` field baked into the artifact: whoever opens the file learns how to change it without
/// leaving the file.
const BASELINE_DOC: &str = "GENERATED warning ratchet (validator section 17) -- do not hand-edit. \
`make validate` fails when the live per-rule warning histogram differs from this one, in EITHER \
direction. To change it, run `make warning-baseline` and commit the result in the SAME commit as \
the spec/code change that moved the number; a deliberate INCREASE is recorded by that diff plus one \
line in the PR body saying why the warning is accepted.";

/// The per-rule warning histogram — the whole ratchet state. Locations are deliberately NOT part of
/// it: they churn on every rename, and a per-location ledger would be a merge-conflict generator
/// without catching anything a per-kind count misses at this granularity.
pub(crate) type WarningProfile = BTreeMap<String, usize>;

/// The live profile of a validation run. Takes ALL issues (the spec validator's plus §13 proposal
/// hygiene, exactly what `checks: N error(s), M warning(s)` counts) and keeps the warnings.
pub(crate) fn warning_profile(issues: &[Issue]) -> WarningProfile {
    let mut out: WarningProfile = BTreeMap::new();
    for i in issues.iter().filter(|i| i.level == Level::Warning) {
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

/// Read the committed ratchet. `total` is cross-checked against the histogram so a hand-edit that
/// fudges one of the two cannot pass — the artifact is tool-written, and this says so out loud.
pub(crate) fn parse_warning_baseline(text: &str) -> Result<WarningProfile, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
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
pub(crate) fn check_warning_baseline(root: &std::path::Path, live: &WarningProfile) -> Result<(), String> {
    let path = root.join(WARNING_BASELINE_PATH);
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("\n✗ cannot read {}: {e}\n  run `make warning-baseline` to (re)create it.\n", path.display()))?;
    let committed = parse_warning_baseline(&text)
        .map_err(|e| format!("\n✗ {}: {e}\n  run `make warning-baseline` to rewrite it.\n", path.display()))?;
    let diff = diff_warning_baseline(&committed, live);
    if diff.is_clean() {
        Ok(())
    } else {
        Err(render_baseline_failure(&diff, live))
    }
}
