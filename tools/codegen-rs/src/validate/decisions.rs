use crate::*;

// ─── §22 — the decision register rows (docs/decisions/*.yaml, REG-2/REG-4, #658) ────────────────
//
// PROP-20260819-110442 D2(a)/D4(a), decided by founder directive 2026-08-21: the register row is
// the unit of decision and gets a declaration site — one YAML file per globally unique key, a
// CLOSED status vocabulary, and resolvable `decided_by`/`superseded_by`. The register's prose
// sections stay authored (D3(a)); only the index is generated (emit side of this module). The
// rules here are pure over parsed rows plus injected resolvers, mirroring proposals.rs, so the
// planted-defect tests feed fixture strings without disk or cwd assumptions.
//
// Migration is deliberately partial (the founder's own bound: "do not backfill the entire
// historical corpus in this slice"): keys without a file are LEGACY, and legacy is a DECLARATION,
// not a default — `docs/decisions/_legacy.yaml` is the committed closed allowlist of grandfathered
// prose-only keys, so "legacy" is a checkable claim and a key in neither set is simply not a
// register reference. A key may never be in both sets.

/// The closed status vocabulary (REG-4(a)). Five values covered all 148 observed rows.
pub(crate) const DECISION_STATUSES: [&str; 5] = ["open", "decided", "deferred", "superseded", "withdrawn"];

/// Who owes the next move on the ANSWER (not on adjacent design work): `counsel`/`external` mark a
/// row the founder cannot act on directly — the index must not invite him to push on those.
pub(crate) const DECISION_OWNERS: [&str; 4] = ["founder", "team", "counsel", "external"];

/// The capacity in which a closing decision was taken (legal lens, 2026-08-21 briefing): `decided`
/// on a legal-exposed row is a business decision under legal exposure, never clearance, and the
/// capacity keeps that visible.
pub(crate) const DECISION_CAPACITIES: [&str; 4] = ["founder", "team", "counsel", "architect"];

const KNOWN_FIELDS: [&str; 13] = [
    "key", "status", "question", "owner", "opened", "register", "evidence", "decided", "decided_by",
    "superseded_by", "until", "note", "reconsiders",
];
const KNOWN_OPTIONAL_EXTRA: [&str; 1] = ["capacity"];

/// One parsed decision row: scalar string fields only (dates stay strings; YAML core schema has no
/// date type and none is wanted — the emitter never computes with them, ADR: drift determinism).
pub(crate) struct DecisionRow {
    pub(crate) path: String,
    pub(crate) stem: String,
    pub(crate) fields: BTreeMap<String, String>,
}

impl DecisionRow {
    pub(crate) fn get(&self, k: &str) -> Option<&str> {
        self.fields.get(k).map(|s| s.as_str())
    }
}

/// Read every `docs/decisions/*.yaml` except underscore-prefixed control files (`_legacy.yaml`),
/// sorted for determinism. A missing directory yields an empty corpus (tolerant, like load_model —
/// the §22 rules then simply have nothing to say).
pub(crate) fn load_decision_files(root: &std::path::Path) -> Vec<(String, String)> {
    let dir = root.join("docs/decisions");
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        let mut paths: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.ends_with(".yaml") && !n.starts_with('_'))
                    .unwrap_or(false)
            })
            .collect();
        paths.sort();
        for p in paths {
            if let (Some(name), Ok(content)) = (p.file_name().and_then(|n| n.to_str()), fs::read_to_string(&p)) {
                out.push((format!("docs/decisions/{}", name), content));
            }
        }
    }
    out
}

/// The committed legacy allowlist: `_legacy.yaml`'s `legacy:` sequence. Missing file = empty list.
pub(crate) fn load_legacy_keys(root: &std::path::Path) -> Vec<String> {
    let p = root.join("docs/decisions/_legacy.yaml");
    let Ok(content) = fs::read_to_string(&p) else { return Vec::new() };
    parse_legacy_keys(&content)
}

pub(crate) fn parse_legacy_keys(content: &str) -> Vec<String> {
    let Ok(v) = serde_yaml::from_str::<Value>(content) else { return Vec::new() };
    v.get("legacy")
        .and_then(|l| l.as_sequence())
        .map(|seq| seq.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

/// The record corpus, kind-separated: what "resolvable" resolves against. Kept apart because the
/// stamp-uniqueness guarantee of the id scheme (ADR-20260718-135417) is per RECORD, not per kind —
/// a mistyped kind prefix must never silently resolve against the other kind's stamp.
pub(crate) struct RecordCorpus {
    pub(crate) adr_files: Vec<String>,
    pub(crate) proposal_files: Vec<String>,
}

/// Filenames that can close a decision or be cited as a record.
pub(crate) fn load_record_corpus(root: &std::path::Path) -> RecordCorpus {
    let list = |dir: &str| -> Vec<String> {
        let mut out = Vec::new();
        if let Ok(rd) = fs::read_dir(root.join(dir)) {
            for e in rd.flatten() {
                if let Some(n) = e.file_name().to_str() {
                    if n.ends_with(".md") {
                        out.push(n.to_string());
                    }
                }
            }
        }
        out.sort();
        out
    };
    RecordCorpus { adr_files: list("docs/adr"), proposal_files: list("docs/proposals") }
}

fn is_stamp(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 15 && b[8] == b'-' && s.chars().enumerate().all(|(i, c)| i == 8 || c.is_ascii_digit())
}

/// True when `id` names a real record file. Three shapes, resolved KIND-AWARE:
///   * `ADR-YYYYMMDD-HHMMSS` — a docs/adr file starting with the full id OR with the bare stamp
///     (104 of the middle-era ADRs are named `20260720-233000-….md`, WITHOUT the prefix — matching
///     on `contains(id)` missed every one of them, the bug this replaces);
///   * legacy `ADR-00NN` — a docs/adr file starting with the four digits (`0032-….md`);
///   * `PROP-YYYYMMDD-HHMMSS` — a docs/proposals file starting with the full id.
/// Resolution proves EXISTENCE, never authority: a resolving PROP is still an option space, a
/// resolving legal brief is still not clearance, and a held record is citable as existing, not as
/// controlling (founder requirement 9, 2026-08-21).
pub(crate) fn record_resolves(id: &str, corpus: &RecordCorpus) -> bool {
    if let Some(rest) = id.strip_prefix("ADR-") {
        if rest.len() == 4 && rest.chars().all(|c| c.is_ascii_digit()) {
            // The hyphen is part of the match: every legacy file is `NNNN-…`, and a bare digit
            // prefix would let a truncated stamp (`ADR-2026`) resolve against the 54 prefixless
            // middle-era filenames that all start `2026…` (PR #669 review, F3).
            let want = format!("{}-", rest);
            return corpus.adr_files.iter().any(|f| f.starts_with(&want));
        }
        if is_stamp(rest) {
            return corpus.adr_files.iter().any(|f| f.starts_with(id) || f.starts_with(rest));
        }
        return false;
    }
    if let Some(rest) = id.strip_prefix("PROP-") {
        if is_stamp(rest) {
            return corpus.proposal_files.iter().any(|f| f.starts_with(id));
        }
    }
    false
}

/// Parse one file into a row; scalar-typed fields only. Errors land in `issues`.
pub(crate) fn parse_decision_rows(files: &[(String, String)], issues: &mut Vec<Issue>) -> Vec<DecisionRow> {
    let mut rows = Vec::new();
    for (path, content) in files {
        let stem = path
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .trim_end_matches(".yaml")
            .to_string();
        let v: Value = match serde_yaml::from_str(content) {
            Ok(v) => v,
            Err(e) => {
                issues.push(err("decision-file-unparseable", path.clone(), format!("not parseable as YAML: {}", e)));
                continue;
            }
        };
        let Some(map) = v.as_mapping() else {
            issues.push(err(
                "decision-file-unparseable",
                path.clone(),
                "top level is not a mapping — a decision file is a flat mapping of scalar fields.".into(),
            ));
            continue;
        };
        let mut fields = BTreeMap::new();
        for (k, val) in map {
            let Some(k) = k.as_str() else {
                issues.push(err("decision-field-unknown", path.clone(), "non-string field name.".into()));
                continue;
            };
            if !KNOWN_FIELDS.contains(&k) && !KNOWN_OPTIONAL_EXTRA.contains(&k) {
                issues.push(err(
                    "decision-field-unknown",
                    path.clone(),
                    format!(
                        "unknown field `{}` — the schema is closed so a typo'd field cannot silently pass; known: {} (+ capacity).",
                        k,
                        KNOWN_FIELDS.join(", ")
                    ),
                ));
                continue;
            }
            let s = match val {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => {
                    issues.push(err(
                        "decision-field-unknown",
                        path.clone(),
                        format!("field `{}` is not a scalar — every field is a scalar string.", k),
                    ));
                    continue;
                }
            };
            fields.insert(k.to_string(), s);
        }
        rows.push(DecisionRow { path: path.clone(), stem, fields });
    }
    rows
}

fn valid_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    key.len() >= 3
        && key.len() <= 64
        && bytes[0].is_ascii_uppercase()
        && key.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')
        && !key.contains("--") // reserved: the future namespace separator for the D1–D7 family (slice 5)
        && !key.ends_with('-')
}

fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && s.chars().enumerate().all(|(i, c)| matches!(i, 4 | 7) || c.is_ascii_digit())
}

/// §22 rules, pure over parsed rows + the legacy list + an injected record resolver. Every rule is
/// an ERROR: the corpus starts at ~18 hand-migrated rows, so there is nothing to grandfather.
pub(crate) fn validate_decision_rows(
    rows: &[DecisionRow],
    legacy_keys: &[String],
    record_exists: &dyn Fn(&str) -> bool,
) -> Vec<Issue> {
    let mut issues = Vec::new();
    let declared: BTreeMap<String, &DecisionRow> = rows.iter().map(|r| (r.stem.clone(), r)).collect();

    // Global key uniqueness, case-insensitively, across BOTH worlds: two files (the fs allows
    // `A.yaml` + `a.yaml` on case-sensitive systems), and declared ∩ legacy (a migrated key still
    // on the allowlist has two authorities for one fact).
    let mut seen_ci: BTreeMap<String, String> = BTreeMap::new();
    for r in rows {
        if let Some(prev) = seen_ci.insert(r.stem.to_ascii_uppercase(), r.stem.clone()) {
            if prev != r.stem {
                issues.push(err(
                    "decision-key-duplicate",
                    r.path.clone(),
                    format!("key `{}` collides case-insensitively with declared key `{}`.", r.stem, prev),
                ));
            }
        }
    }
    for legacy in legacy_keys {
        if declared.contains_key(legacy) {
            issues.push(err(
                "decision-key-duplicate",
                format!("docs/decisions/{}.yaml", legacy),
                format!(
                    "key `{}` is declared AND on the `_legacy.yaml` allowlist — a migrated key leaves the allowlist in the same change, or two authorities exist for one fact.",
                    legacy
                ),
            ));
        }
    }

    for r in rows {
        let path = &r.path;
        // key: present, grammar-valid, equal to the filename stem (one file per key is only a
        // merge-conflict property; identity needs the declared name and the fs name to agree).
        match r.get("key") {
            None => issues.push(err("decision-key-file-mismatch", path.clone(), "no `key` field.".into())),
            Some(k) if k != r.stem => issues.push(err(
                "decision-key-file-mismatch",
                path.clone(),
                format!("declares key `{}` but the file is named `{}.yaml` — they must agree.", k, r.stem),
            )),
            Some(_) => {}
        }
        if !valid_key(&r.stem) {
            issues.push(err(
                "decision-key-grammar",
                path.clone(),
                format!(
                    "key `{}` violates the v1 key grammar `^[A-Z][A-Z0-9-]{{2,63}}$` (no `--`, no trailing `-`; `--` is reserved for the future namespaced-key encoding of the D1–D7 family, PROP-20260819-110442 D5 / slice 5).",
                    r.stem
                ),
            ));
        }

        // Required-always fields.
        for (field, rule) in [
            ("question", "decision-question-missing"),
            ("register", "decision-register-missing"),
            ("evidence", "decision-evidence-missing"),
        ] {
            if r.get(field).map(|s| s.trim().is_empty()).unwrap_or(true) {
                issues.push(err(
                    rule,
                    path.clone(),
                    format!("no `{}` — a row carries its one-line question, the pointer to its authoritative prose (`register`), and a verbatim `evidence` quote so the extraction stays reviewable against the source.", field),
                ));
            }
        }
        if let Some(q) = r.get("question") {
            if q.contains('\n') {
                issues.push(err(
                    "decision-question-missing",
                    path.clone(),
                    "`question` spans multiple lines — it is the index's one-line, answerable question; the reasoning lives in the prose the `register` field points at.".into(),
                ));
            }
        }
        match r.get("owner") {
            Some(o) if DECISION_OWNERS.contains(&o) => {}
            Some(o) => issues.push(err(
                "decision-owner-invalid",
                path.clone(),
                format!("owner `{}` is not in the closed set {:?} — owner = who owes the next move on the ANSWER.", o, DECISION_OWNERS),
            )),
            None => issues.push(err("decision-owner-invalid", path.clone(), format!("no `owner` — closed set {:?}.", DECISION_OWNERS))),
        }
        if let Some(c) = r.get("capacity") {
            if !DECISION_CAPACITIES.contains(&c) {
                issues.push(err(
                    "decision-capacity-invalid",
                    path.clone(),
                    format!("capacity `{}` is not in the closed set {:?}.", c, DECISION_CAPACITIES),
                ));
            }
        }
        match r.get("opened") {
            Some(d) if valid_date(d) => {}
            Some(d) => issues.push(err(
                "decision-opened-invalid",
                path.clone(),
                format!("opened `{}` is not a YYYY-MM-DD date — the per-row date is what lets any later question decompose the index's aggregate counts.", d),
            )),
            None => issues.push(err("decision-opened-invalid", path.clone(), "no `opened` date (YYYY-MM-DD).".into())),
        }

        // The status coupling table — biconditional on purpose (dba/graphql, 2026-08-21 briefing):
        // a field's PRESENCE also constrains the status, so `superseded_by` on an `open` row is as
        // unspellable as `superseded` without a successor.
        let status = r.get("status").unwrap_or("");
        if !DECISION_STATUSES.contains(&status) {
            issues.push(err(
                "decision-status-invalid",
                r.path.clone(),
                format!(
                    "status `{}` is not in the closed vocabulary {:?} (REG-4(a)). NOTE: `decided` is a recorded decision, never legal clearance.",
                    status, DECISION_STATUSES
                ),
            ));
            continue; // the coupling rules below would only echo the same defect
        }
        let has = |f: &str| r.get(f).map(|s| !s.trim().is_empty()).unwrap_or(false);
        let forbid = |f: &str, why: &str, issues: &mut Vec<Issue>| {
            if has(f) {
                issues.push(err(
                    "decision-status-field-conflict",
                    r.path.clone(),
                    format!("status `{}` forbids `{}` — {}.", status, f, why),
                ));
            }
        };
        match status {
            "open" => {
                forbid("decided", "an open row has no closing date", &mut issues);
                forbid("decided_by", "a row carrying its closing record is not open (PROP-20260819-110442 §5.2)", &mut issues);
                forbid("superseded_by", "only a `superseded` row names a successor", &mut issues);
                forbid("until", "`until` is the deferred wake condition", &mut issues);
            }
            "deferred" => {
                forbid("decided", "a deferred row is not closed", &mut issues);
                forbid("decided_by", "a deferred row is not closed", &mut issues);
                forbid("superseded_by", "only a `superseded` row names a successor", &mut issues);
                if !has("until") {
                    issues.push(err(
                        "decision-deferred-without-wake",
                        r.path.clone(),
                        "status `deferred` requires `until` (a date or a named event) — without a wake condition, deferred is `open` wearing a euphemism and the ask gate blocks it forever.".into(),
                    ));
                }
            }
            "decided" | "superseded" => {
                forbid("until", "`until` is the deferred wake condition", &mut issues);
                if !has("decided") || !r.get("decided").map(valid_date).unwrap_or(false) {
                    issues.push(err(
                        "decision-decided-without-record",
                        r.path.clone(),
                        format!("status `{}` requires a `decided` date (YYYY-MM-DD).", status),
                    ));
                }
                match r.get("decided_by") {
                    Some(id) if record_exists(id) => {}
                    Some(id) => issues.push(err(
                        "decision-decided-without-record",
                        r.path.clone(),
                        format!("`decided_by: {}` names no resolvable record under docs/adr/ or docs/proposals/. A decision with no record is a memory. Name the ADR that carries it.", id),
                    )),
                    None => issues.push(err(
                        "decision-decided-without-record",
                        r.path.clone(),
                        format!("status `{}` requires `decided_by` naming the record that closed it. A decision with no record is a memory.", status),
                    )),
                }
                if status == "superseded" {
                    match r.get("superseded_by") {
                        None => issues.push(err(
                            "decision-superseded-shape",
                            r.path.clone(),
                            "status `superseded` requires `superseded_by` naming the successor row's key.".into(),
                        )),
                        Some(succ) if succ == r.stem => issues.push(err(
                            "decision-superseded-shape",
                            r.path.clone(),
                            "a row cannot supersede itself.".into(),
                        )),
                        Some(succ) if !declared.contains_key(succ) => issues.push(err(
                            "decision-superseded-shape",
                            r.path.clone(),
                            format!("`superseded_by: {}` names no declared row (docs/decisions/{}.yaml) — a successor is migrated before it is pointed at, never dangled into the legacy prose.", succ, succ),
                        )),
                        Some(_) => {}
                    }
                } else {
                    forbid("superseded_by", "only a `superseded` row names a successor", &mut issues);
                }
            }
            "withdrawn" => {
                forbid("superseded_by", "only a `superseded` row names a successor", &mut issues);
                forbid("until", "`until` is the deferred wake condition", &mut issues);
                if !has("note") {
                    issues.push(err(
                        "decision-withdrawn-without-note",
                        r.path.clone(),
                        "status `withdrawn` requires a `note` saying why the question stopped being a question (and where any surviving finding went).".into(),
                    ));
                }
                if let Some(id) = r.get("decided_by") {
                    if !record_exists(id) {
                        issues.push(err(
                            "decision-decided-without-record",
                            r.path.clone(),
                            format!("`decided_by: {}` names no resolvable record.", id),
                        ));
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    // Supersession is a DAG walked by identity: cycles make the chain head unresolvable, so the
    // hook would cite a successor that leads back to the question being asked.
    for r in rows.iter().filter(|r| r.get("status") == Some("superseded")) {
        let mut seen = BTreeSet::new();
        let mut cur = r.stem.clone();
        while let Some(row) = declared.get(&cur) {
            if !seen.insert(cur.clone()) {
                issues.push(err(
                    "decision-superseded-shape",
                    r.path.clone(),
                    format!("supersession cycle through `{}` — the chain never reaches a live row.", cur),
                ));
                break;
            }
            match row.get("superseded_by") {
                Some(next) => cur = next.to_string(),
                None => break,
            }
        }
    }

    // ── `reconsiders` — the challenge edge (founder requirement 5, 2026-08-21) ──────────────────
    // A reopening of a decided matter is never a re-ask of the old row: a NEW row carries
    // `reconsiders: <OLD-KEY>`. The edge points backwards (challenge → challenged), distinct from
    // `superseded_by` (closed row → successor, set at close time). The field is retained after the
    // challenge closes (history is additive, never stripped).
    let chain_head = |start: &str| -> String {
        let mut cur = start.to_string();
        let mut seen = BTreeSet::new();
        while seen.insert(cur.clone()) {
            match declared.get(&cur).and_then(|row| row.get("superseded_by")) {
                Some(next) => cur = next.to_string(),
                None => break,
            }
        }
        cur
    };
    for r in rows {
        let Some(target_key) = r.get("reconsiders") else { continue };
        let rule = "decision-reconsiders-shape";
        if target_key == r.stem {
            issues.push(err(rule, r.path.clone(), "a row cannot reconsider itself.".into()));
            continue;
        }
        let Some(target) = declared.get(target_key) else {
            issues.push(err(
                rule,
                r.path.clone(),
                format!(
                    "`reconsiders: {}` names no declared row — a challenge targets a declared closed decision; a legacy prose row is migrated first (docs/decisions/README.md), in the same change.",
                    target_key
                ),
            ));
            continue;
        };
        let tstatus = target.get("status").unwrap_or("");
        let coupled = tstatus == "superseded" && target.get("superseded_by") == Some(&r.stem);
        match tstatus {
            "decided" | "withdrawn" => {}
            // The OTHER half of the coupling. The `decided` challenge below is checked for a
            // target that is not superseded by it; this is the mirror — a target superseded by a
            // challenge that has not itself closed. Without it the register rests in a split
            // state the README forbids and nothing sees: a superseded row whose authority points
            // at a question still open, so the chain head is not an answer. Found by the
            // independent review of PR #679, which asserted the coupling was total and proved
            // empirically that only one direction was enforced.
            "superseded" if coupled && !matches!(r.get("status"), Some("decided") | Some("superseded")) => issues.push(err(
                rule,
                r.path.clone(),
                format!(
                    "`{}` is superseded BY this row, but this row's status is `{}` — a supersession may only be executed by a challenge that ANSWERED its question. If this row is still `open`, the answer is coming: close it (`decided` + `decided_by`) rather than editing `{}` back, because `superseded_by` is the one legal edit to a decided row. If this row is `deferred` or `withdrawn`, no answer exists — a deferred row is parked behind its `until:` wake condition and a withdrawn one stopped being a question — so the flip was NEVER a legal supersession and reverting `{}` IS the repair; closing this row instead would fabricate a decision nobody took (docs/decisions/README.md).",
                    target_key,
                    r.get("status").unwrap_or(""),
                    target_key,
                    target_key
                ),
            )),
            "superseded" if coupled => {}
            "superseded" => issues.push(err(
                rule,
                r.path.clone(),
                format!(
                    "`reconsiders: {}` targets a superseded row — challenge the HEAD of its supersession chain (`{}`), not a record that is no longer the authority.",
                    target_key,
                    chain_head(target_key)
                ),
            )),
            _ => issues.push(err(
                rule,
                r.path.clone(),
                format!(
                    "`reconsiders: {}` targets a row whose status is `{}` — a challenge targets a CLOSED decision (decided/withdrawn); an open or deferred row is simply asked.",
                    target_key, tstatus
                ),
            )),
        }
        // Closure coupling (one controlling record per key): a DECIDED challenge and its target
        // must have executed the two-file supersession move in the same commit, or two rows each
        // believe they are controlling.
        if r.get("status") == Some("decided") && !coupled {
            issues.push(err(
                rule,
                r.path.clone(),
                format!(
                    "this reconsidering row is `decided` but `{}` is not superseded by it — closing a challenge IS the supersession move: flip `{}` to `superseded` with `superseded_by: {}` in the same commit (docs/decisions/README.md).",
                    target_key, target_key, r.stem
                ),
            ));
        }
    }
    issues
}

// ─── The index↔source sync gate (founder requirement 12, 2026-08-21) ────────────────────────────

/// The committed `GENERATED:decisions` region's inner body, if the marker pair exists.
pub(crate) fn extract_decisions_region(register_content: &str) -> Option<String> {
    let start_pat = "<!-- GENERATED:decisions START";
    let end_pat = "<!-- GENERATED:decisions END -->";
    let start_idx = register_content.find(start_pat)?;
    let after_marker = start_idx + register_content[start_idx..].find("-->")? + 3;
    let end_idx = register_content.find(end_pat)?;
    if end_idx < after_marker {
        return None;
    }
    Some(register_content[after_marker..end_idx].trim().to_string())
}

/// THE ONE corpus behind `decision-superseded-authority`, derived from GIT rather than the disk.
///
/// It existed twice -- once in `main.rs`, once re-implemented inside the test that pins the rule --
/// and the test copy lacked every guard. There is one now, and both callers use it.
///
/// IT READS `git ls-files`, NOT `read_dir`, and that is the actual fix. A filesystem walk sees
/// every untracked and gitignored file under `.claude/`, so it reddened `make validate` on a
/// leftover `.claude/worktrees/<wt>/` -- an untracked checkout of another branch that the operator
/// cannot resolve from the diff, while CI stayed green because a runner has none. The first
/// remedy was a four-name denylist (`worktrees | target | node_modules | .git`) plus a `.local.`
/// match, and review #11 pointed out that this treats a symptom: the same failure recurs under any
/// name not on the list. `.claude/wt-679/` is the ready example -- the root `.gitignore` already
/// carries `wt*/`, so that shape is expected here -- as is any scratch `.md` holding a citation.
///
/// The rule's subject is COMMITTED content: the agent surface, where a stale citation is read as
/// an instruction. Deriving the list from the index makes the local and CI corpora identical BY
/// CONSTRUCTION rather than by maintaining a denylist, and it retires the prune list and the
/// symlink guard together. CLAUDE.md's compiler-first order, applied to a gate: prefer making the
/// divergence unrepresentable over enumerating the ways it shows up.
///
/// SCOPE, decided rather than defaulted:
///   * `.claude/**` -- the agent surface, FILTERED BY EXTENSION to `md|sh|json|yaml|yml`. The
///     filter is stated here because it is part of the scope, and this bullet used to say
///     `.claude/**` flat while the code applied an allowlist the inline comment explained only for
///     the ROOT files: a `.claude/**` file with no extension, or a `.txt`/`.toml`, sits outside a
///     rule this section promised covered the whole tree. Nothing tracked under `.claude/` is
///     excluded today, so it was latent -- which is exactly how the OTHER two-statements-of-one-
///     scope divergences in this file started (review #19).
///   * The root files that carry row references in prose: `.claudeignore` (one of the eight sites
///     PR #679 fixed by hand, and NOT under `.claude/`), `.gitignore`, `CLAUDE.md` -- the resident
///     index every session loads before anything else -- and the `Makefile`.
///   * `docs/**` is deliberately OUT: a record ABOUT a supersession must name the superseded row,
///     and redding those would make the rule unusable.
///   * `.github/workflows/**` is IN, and the argument for excluding it was falsified by the diff
///     that shipped it. The bullet used to say workflow row references are "provenance comments on
///     decided work, not instructions to follow" -- while THIS change added, to `ci.yml`, directly
///     above the step it governs: *"Authorized by decision row RETRIEVAL-QMD-CI ... that row
///     authorizes THIS STEP AND ITS PIN AND NOTHING ELSE in CI."* That is a normative instruction
///     to the next author, in the `row <KEY>` form this rule recognises everywhere else. Supersession
///     on this chain is routine, not hypothetical -- `RETRIEVAL-QMD` was superseded two days after
///     being decided -- so a session adding a second CI step would follow a dead row into
///     `reconsiders: <superseded row>` and hit `decision-reconsiders-shape`, with `make validate`
///     green the whole way. `SKILL.md` and `decision-lookup.sh` were fixed by hand for exactly that
///     shape and put in corpus; `ci.yml` carried it and was not. (Review #21 of PR #679.)
///
/// A repo with no git available yields an empty corpus, i.e. the rule says nothing -- the same
/// tolerant posture `load_model` takes, and the honest one: a corpus this cannot read is not a
/// corpus it may judge.
pub(crate) fn claude_citation_corpus(root: &std::path::Path) -> (Vec<(String, String)>, bool, Vec<String>) {
    let out = match std::process::Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--",
            ".claude",
            ".claudeignore",
            ".gitignore",
            "CLAUDE.md",
            "Makefile",
            ".github/workflows",
        ])
        .current_dir(root)
        .output()
    {
        // FAIL OPEN, BUT NOT SILENTLY. Returning an empty corpus here is deliberate -- a corpus
        // this cannot read is not a corpus it may judge -- but it used to be INVISIBLE, and that
        // is what made a shimmed `git` a route rather than merely a limitation: exiting 0 with
        // empty stdout yields no files, so the rule reports nothing at all and `make validate` is
        // green with no error, no warning, and no "corpus unreadable" line. "No stale citations"
        // and "did not look" printed identically. The `readable` flag lets the caller say which
        // one happened; the posture is unchanged. (Review #27 of PR #679.)
        Ok(o) if o.status.success() => o.stdout,
        _ => return (Vec::new(), false, Vec::new()),
    };
    let mut cited = Vec::new();
    let mut unread: Vec<String> = Vec::new();
    for rel in String::from_utf8_lossy(&out).split('\0').filter(|s| !s.is_empty()) {
        let tracked_ext = std::path::Path::new(rel)
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|e| matches!(e, "md" | "sh" | "json" | "yaml" | "yml"));
        // The root files carry no extension filter -- `Makefile` has none, and `.gitignore` /
        // `.claudeignore` are extensions rather than stems.
        let is_root_file = matches!(rel, ".claudeignore" | ".gitignore" | "CLAUDE.md" | "Makefile");
        if !(tracked_ext || is_root_file) {
            continue;
        }
        // TRACKED BUT UNREADABLE IS NOT "CLEAN". `readable` distinguishes "git refused to look"
        // from "looked"; it says nothing about "read ALL of it" versus "read SOME of it". A path
        // the index lists but the filesystem cannot hand back was dropped here with no counter and
        // no warning, so `make validate` printed an identical green whether the corpus was six
        // files or sixty -- the same shape review #27 closed one level up, one level down.
        // Reachable with no tampering at all: a SPARSE CHECKOUT keeps index rows for `.claude/**`
        // while the worktree files are absent; a tracked symlink whose target is gone; a file the
        // current user cannot read (a root-created file in a container stage); and non-UTF-8
        // content, which `read_to_string` also rejects and which this repo's own T15g fixture
        // builds deliberately. Fail open -- the posture is unchanged -- but SAY SO, because "did
        // not look" and "found nothing" must not print identically. (Review #52 of PR #679.)
        match std::fs::read_to_string(root.join(rel)) {
            Ok(text) => cited.push((rel.to_string(), text)),
            Err(_) => unread.push(rel.to_string()),
        }
    }
    (cited, true, unread)
}

/// One scanning unit per BLOCK — consecutive wrapped lines joined, a new list item starting a new
/// unit — with each line's leading comment or quote marker stripped so the join reads as the
/// sentence the author actually wrote. `spans` maps a byte offset in the joined text back to the
/// physical line it came from, so a finding still names the citing line and not the block's first.
///
/// THE RULE BELOW IS CLAUSE-SCOPED AND THE CORPUS IS HARD-WRAPPED AT ~100 COLUMNS. Scanning
/// `content.lines()` decided everything inside one physical line, which broke the rule in both
/// directions at a wrap (review #13 of PR #679), and both failures land as a HARD `make validate`
/// error that blocks every push:
///
///   * FALSE RED. `cites` accepts a backticked key that OPENS a line. Wrap an ordinary sentence so
///     that ``` `KEY` ``` lands at the start of the continuation line and it reds, though nothing
///     on that line cites anything.
///   * THE ESCAPE HATCH BECOMES UNREACHABLE. The exemption needs the word `superseded` in the
///     clause around the occurrence. Write the sentence the docstring calls legal, wrap it so
///     `superseded` falls to the next line, and the exemption never sees it. The author's only
///     remaining moves are rewording or re-wrapping — a red whose escape is silence, on exactly
///     the prose the rule wants people to write.
///
/// Today's corpus was green BY LUCK: two sites repeat "superseded" on both wrapped lines and one
/// is a single long line. Every green control in the test was a single line, which is why the
/// class was invisible to it.
///
/// A LIST ITEM STARTS A NEW UNIT, and the first version of this function got that wrong in the
/// PERMISSIVE direction — the one that matters. It stripped `-`/`*` like a comment marker and
/// joined bullets together, so the exemption window grew from a line to a whole paragraph and an
/// ADJACENT BULLET could silence a live citation:
///
/// ```text
/// - Per row OLD-ROW, open a reversal decision before changing the pin
/// - (that row is superseded)
/// ```
///
/// Neither `(` nor `)` is a clause boundary, so the joined unit put `superseded` inside the citing
/// clause and the stale instruction went green — where scanning line 1 alone had redded it. That
/// is `decision-lookup.sh`'s `activation_fail` shape, i.e. the motivating incident, and review #14
/// caught it in the commit that introduced it. A markdown continuation is INDENTED, not re-marked,
/// so treating `- `/`* `/`1. ` as a block start is both correct and what closes this. `#`, `//`
/// and `>` stay continuation markers, because a shell comment block repeats them on every line.
///
/// So the honest statement of what joining buys: it fixes the two wrap failures above and catches
/// a citation split across a wrap (`Decided by row` / `` `KEY` ``) that was missed before. It does
/// NOT "strictly improve detection" — that claim was in the docstring one commit ago and this
/// bullet rule is what makes it true.
struct Unit {
    text: String,
    /// `(byte offset where this line's text starts in `text`, 1-based source line)`, ascending.
    spans: Vec<(usize, usize)>,
}

impl Unit {
    /// The physical line an offset in `text` came from.
    fn line_at(&self, at: usize) -> usize {
        self.spans
            .iter()
            .rev()
            .find(|(start, _)| *start <= at)
            .map_or(self.spans.first().map_or(1, |(_, l)| *l), |(_, l)| *l)
    }
}

fn logical_units(content: &str) -> Vec<Unit> {
    // `- foo`, `* foo`, `1. foo`, `2) foo` — and a bare marker on its own line.
    fn starts_a_block(body: &str) -> bool {
        if matches!(body, "-" | "*") || body.starts_with("- ") || body.starts_with("* ") {
            return true;
        }
        let digits: String = body.chars().take_while(char::is_ascii_digit).collect();
        !digits.is_empty()
            && matches!(body[digits.len()..].chars().next(), Some('.') | Some(')'))
            && body[digits.len() + 1..].starts_with(' ')
    }

    let mut out: Vec<Unit> = Vec::new();
    let mut cur: Option<Unit> = None;
    // Whether the previous non-blank line carried a comment/quote marker. A CHANGE in that class
    // ends the unit -- see below.
    let mut prev_marked = false;
    for (i, raw) in content.lines().enumerate() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            if let Some(u) = cur.take() {
                out.push(u);
            }
            continue;
        }
        // Comment and quote markers repeat on every wrapped line, so they are stripped and joined.
        let marked = trimmed.starts_with('#') || trimmed.starts_with("//") || trimmed.starts_with('>');
        let after_marker = trimmed
            .trim_start_matches(|c: char| matches!(c, '#' | '/' | '>'))
            .trim_start();
        // A MARKER CLASS CHANGE ALSO ENDS A UNIT, and leaving it out was review #14's bullet defect
        // reproduced one marker over. `#`/`//`/`>` are continuation markers on purpose (a shell
        // comment block repeats them on every wrapped line), but nothing ended the unit when the
        // marker STOPPED -- so a comment block and the executable line beneath it became one unit
        // and the clause exemption read across the prose/code boundary:
        //
        // ```sh
        // # kept for history: the old row is superseded
        // echo "Per row OLD-ROW: open a reversal decision"
        // ```
        //
        // No `;`, `—` or sentence dot anywhere in that join, so the whole thing was one clause, it
        // contained `superseded`, and the LIVE citation in the `echo` went green -- where
        // line-scoped it had redded. `decision-lookup.sh`'s `activation_fail` has exactly that
        // layout, and stayed caught only because its comment happens to end in a sentence dot.
        // A guard that depends on someone not reflowing a comment is not a guard. (Review #18.)
        let opens = starts_a_block(after_marker) || marked != prev_marked;
        prev_marked = marked;
        // The list marker itself is dropped from the text: `- \`KEY\`` must still read as a key
        // opening the unit, which is one of the citation forms.
        let body = if opens {
            after_marker
                .trim_start_matches(|c: char| matches!(c, '-' | '*' | ')' | '.'))
                .trim_start_matches(|c: char| c.is_ascii_digit())
                .trim_start_matches(|c: char| matches!(c, ')' | '.'))
                .trim_start()
        } else {
            after_marker
        };
        // A MARKER-ONLY LINE IS THE PARAGRAPH SEPARATOR INSIDE A COMMENT BLOCK. A bare `#` trims to
        // an empty body, `starts_a_block("")` is false, and it used to be joined as empty text --
        // so `# ...end of a paragraph.` / `#` / `# \`KEY\` still governs.` was ONE unit, and the
        // "a backticked key OPENS the unit" citation form could never fire for any paragraph after
        // the first. That form is `SKILL.md:193`'s spelling and both gate scripts are long
        // `#`-separated blocks, so the miss was in the corpus the rule exists for. One-directional
        // (it misses, it never false-reds), which is why nothing surfaced it. (Review #20.)
        if marked && after_marker.is_empty() {
            if let Some(u) = cur.take() {
                out.push(u);
            }
            continue;
        }
        if opens {
            if let Some(u) = cur.take() {
                out.push(u);
            }
        }
        match cur.as_mut() {
            Some(u) => {
                u.text.push(' ');
                u.spans.push((u.text.len(), i + 1));
                u.text.push_str(body);
            }
            None => {
                cur = Some(Unit {
                    text: body.to_string(),
                    spans: vec![(0, i + 1)],
                })
            }
        }
    }
    if let Some(u) = cur {
        out.push(u);
    }
    out
}

/// No file under `.claude/**` may cite a SUPERSEDED row as its live authority.
///
/// PR #679 flipped `RETRIEVAL-QMD` to `superseded` and rewrote the proposal to say, verbatim,
/// *"name the head, never `RETRIEVAL-QMD` ... the old wording sent the next session straight into a
/// gate error on the rollback path"*. That correction reached the proposal and stopped: seven
/// sites under `.claude/**` still named the superseded row, including
/// `decision-lookup.sh`'s RUNTIME failure message -- the string an operator reads on the exact
/// rollback path the row's FAILURE PROTOCOL governs. Doing what it said produced
/// `reconsiders: <superseded row>`, which `decision-reconsiders-shape` rejects.
///
/// CLAUDE.md already says to grep the old term after any reshape; the term lived in `.claude/**`,
/// It is derived from row STATUS, not a hard-coded key list, so it keeps working as the chain
/// grows links.
///
/// WHAT IT DOES NOT REACH, because the first version of this docstring claimed to be "the grep"
/// and review #11 measured otherwise:
///
///   * Only the CITATION FORMS below are recognised — `row <KEY>`, `Per <KEY>`, `decided_by: <KEY>`,
///     `the <KEY> decision` and the `docs/decisions/<KEY>.yaml` path. Prose that names a row some
///     other way is missed. This is a high-signal spot check, not an exhaustive grep.
///   * It sees only the files the caller hands it, and THE CALLER'S `SCOPE` SECTION IS THE ONLY
///     STATEMENT OF WHAT THAT IS — see `claude_citation_corpus`. This bullet used to enumerate the
///     set a second time and was already wrong in the commit that shipped it: it named
///     `.claude/**`, `.claudeignore` and `.gitignore` while the corpus also read `CLAUDE.md` and
///     the `Makefile`. Two lists of one scope diverge — the sentence this very change closes twice
///     elsewhere — and the cost is specific: a superseded row named in `CLAUDE.md`, the resident
///     index, reds `make validate` with a rule the reader has just been told does not reach that
///     file. So there is now one list, and it is over there.
///   * `docs/**` is deliberately NOT in scope: records *about* a supersession necessarily name the
///     superseded row, and redding those would make the rule unusable. That asymmetry is the reason
///     the scope is a caller decision rather than a walk from the repo root. **The argument is
///     about records that NARRATE HISTORY, and not every `docs/**` subtree is one** — `docs/claude/**`
///     are the topic authorities CLAUDE.md routes every session to *before it works*
///     (`sessions.md` is marked operational), `docs/PLAYBOOK.md` sits in the same position, and
///     `docs/dispatch/**` is a card a session executes. Those are instruction surfaces, which is
///     the property that puts `.claude/**` in scope at all. None carries a row citation today, so
///     the exclusion is not wrong on this tree — but it is not the enumerated exception space it
///     reads as, and `DISPATCH-CARD-CITATION` is the open row that holds the question. It was
///     opened for `docs/dispatch/**` alone and widened to the class, because naming exceptions one
///     subtree at a time is how the next one goes unnamed. (Review #59 of PR #679.)
///   * FENCED CODE IS **NOT** EXEMPT, and that is a decision, not an oversight. The sibling
///     `decision-card-row` rule tracks fences and skips them, so the two rules disagree on purpose
///     and the next author should not have to derive which is which (review #23). A card's fenced
///     block is an ILLUSTRATION of a form; a `.claude/**` doc's fenced block is the thing a session
///     COPIES — the motivating incident was a session doing exactly what a doc showed it. A fenced
///     `reconsiders: <dead row>` is therefore the most dangerous spelling in the corpus, not the
///     safest, so the exemption that is right for cards would be backwards here. Prose *about* a
///     supersession still has the clause-scoped escape; a copyable example does not need one.
/// (This block documented `struct Unit` for one round: it sits between the corpus reader and
/// `logical_units`, so `cargo doc` and rust-analyzer attached the RULE's scope, its residual and
/// the `docs/**` decision to a private helper struct, while the rule function itself carried no
/// doc comment at all. That is the mis-binding the comment further down says was fixed by moving
/// text, reproduced in mirror image by the move that fixed it -- and it matters because
/// `DISPATCH-CARD-CITATION` is an open row whose whole subject is the scope sentence above.
/// Review #53 of PR #679.)
pub(crate) fn validate_no_superseded_row_is_cited_as_authority(
    rows: &[DecisionRow],
    files: &[(String, String)],
) -> Vec<Issue> {
    // A NON-EMPTY KEY, OR THIS FUNCTION NEVER RETURNS. `line[from..].find("")` is `Some(0)`
    // unconditionally and the advance is `from = at + key.len()`, so a zero-length key spins on the
    // first unit of the first file forever. It is reachable: `parse_decision_rows` accepts an
    // explicit `key: ""` (only a YAML null is rejected as non-scalar) and `valid_key` is applied to
    // the FILE STEM, not to this field -- so a template copy-paste with a blanked `key:` line and
    // `status: "superseded"` would hang `make validate` locally and hang the `codegen`/`specs` jobs
    // until GitHub's six-hour timeout, INSTEAD of reporting the `decision-key-file-mismatch` that
    // was waiting in the same issue list. A gate that cannot report is the shape this whole change
    // argues against. (Review #20 of PR #679.)
    let superseded: Vec<&str> = rows
        .iter()
        .filter(|r| r.get("status") == Some("superseded"))
        .filter_map(|r| r.get("key"))
        .filter(|k| !k.is_empty())
        .collect();
    let mut issues = Vec::new();
    for (path, content) in files {
        for unit in logical_units(content) {
            let line = unit.text.as_str();
            for key in &superseded {
                // BIND TO THE CITING POSITION, not to the line. A first attempt flagged any line
                // mentioning the key alongside the word "row", which false-redded the very sentence
                // that names the head and EXPLAINS the supersession -- the key appeared in a
                // possessive clause. That is the key-presence instrument this repo has retracted
                // three times, reproduced here on the first try. A citation is the key immediately
                // after `row` (optionally backticked); anything else is prose about the row.
                let mut from = 0;
                while let Some(i) = line[from..].find(*key) {
                    let at = from + i;
                    from = at + key.len();
                    // Not a prefix of a longer key (`RETRIEVAL-QMD` inside `RETRIEVAL-QMD-CI`).
                    if matches!(
                        line[at + key.len()..].chars().next(),
                        Some(c) if c.is_ascii_alphanumeric() || c == '-' || c == '_'
                    ) {
                        continue;
                    }
                    // AN EXPLANATION IS NOT A CITATION -- but scope the exemption to the CLAUSE
                    // around this occurrence, not the whole line. A whole-line test is a one-word
                    // opt-out of a gate whose entire argument is that opt-outs must be explicit,
                    // and it blinded the rule on the single most load-bearing line in the corpus:
                    // SKILL.md's headline authority sentence cites the controlling row AND says
                    // "never the superseded row" further along, so reverting that citation to the
                    // dead row would have stayed green -- in the file whose stale citations
                    // motivated this rule.
                    // A `.` ENDS A CLAUSE ONLY WHEN IT ENDS A SENTENCE. The `docs/decisions/
                    // <KEY>.yaml` arm added below puts a dot immediately AFTER the key, so an
                    // unconditional `.` boundary truncated the clause to the path itself and the
                    // `superseded` exemption could never see the rest of the line: the new arm and
                    // the existing exemption did not compose, and `docs/decisions/OLD-ROW.yaml is
                    // superseded -- read the head` redded. A sentence dot is followed by whitespace
                    // or nothing; a filename dot is followed by a letter. `;` and `—` need no such
                    // test. Found by the green control, not by reading the code.
                    // ` -- ` IS A BOUNDARY TOO, and leaving it out made the exempt window enormous
                    // in exactly the files this rule targets. `—` was a boundary; the ASCII `--`
                    // that the shell and YAML side of the corpus writes throughout (the Makefile is
                    // ASCII-only by rule) was not -- so a clause there was bounded only by `;` and
                    // sentence dots and could run for five joined lines. `activation_fail` is that
                    // span today: one `superseded` written two lines earlier exempts everything to
                    // the end of the block, so any LIVE stale instruction landing anywhere in it is
                    // silenced by a word with nothing to do with it. Same defect as the adjacent
                    // bullet (review #14) and the comment-then-code join (review #18), one
                    // punctuation mark over and INSIDE the unit. (Review #20.)
                    //
                    // Whitespace on BOTH sides, so `--no-filters` and `--depth=1` are not
                    // boundaries: the dash-dash has to be a dash, not the head of a flag.
                    // AN ABBREVIATION DOT IS INSIDE A SENTENCE AND IS FOLLOWED BY A SPACE, so the
                    // whitespace test alone calls it a boundary and truncates the clause there.
                    // `Per row \`OLD-ROW\`, i.e. the row superseded by \`NEW-ROW\`.` redded as a hard
                    // error: `cites` fires on `row`, the forward scan stops at the dot in `e.`, and
                    // the `superseded` that EXPLAINS the citation is outside the clause -- the "a
                    // red whose escape is silence" shape this file argues against in five places,
                    // with rewording or re-wrapping as the author's only move. Invisible to the
                    // control set for the same reason review #25 gives for the em-dash: every green
                    // control separates its explanation with `;`, ` -- ` or a plain sentence dot.
                    //
                    // Two rules, both structural rather than a taste call. INTERIOR DOTS: the token
                    // ending at this dot itself contains one (`i.e`, `e.g`, `a.k.a`, `U.S`) -- a
                    // sentence-final word never does. NAMED SINGLE-SEGMENT ABBREVIATIONS: a short
                    // closed list. `no.`, `etc.` and `al.` are deliberately NOT on it -- each is
                    // also an ordinary sentence-final word in this corpus, and admitting them would
                    // extend clauses past real sentence ends, which is the permissive direction and
                    // the one that makes a live stale citation exempt. A miss here costs a
                    // reword; a false accept costs the gate. (Review #35.)
                    // AN INTERIOR DOT IS NOT ENOUGH, AND THE COMMENT ABOVE STATED THE PREMISE IT
                    // FAILS ON: "a sentence-final word never [contains a dot]". False of exactly
                    // this corpus. `rsplit` splits on anything outside `[A-Za-z0-9.]`, so the token
                    // before the dot in `... live in decision-lookup.sh.` is `lookup.sh` -- and so
                    // are `ci.yml`, `SKILL.md`, `index.sqlite`, `qmd 2.8.3`, `README.md`, i.e. the
                    // vocabulary of every file `claude_citation_corpus` scans. A sentence ending in
                    // one stopped being a clause boundary, so the `superseded` exemption LEAKED
                    // BACKWARDS ACROSS IT: a comment block whose first line says "the predecessor
                    // is superseded, and the notes live in decision-lookup.sh." followed by a live
                    // `Per row <DEAD-ROW>, ...` went GREEN -- and the identical text with `the
                    // wrapper` in place of the filename reds correctly. The verdict decided by a
                    // word the author did not think they were choosing, in the permissive
                    // direction, through the arm added to fix `i.e.`. Same class as the
                    // adjacent-bullet join (review #14) and the comment-then-code join (#18).
                    //
                    // So an interior-dot token must LOOK like an abbreviation: every dot-separated
                    // segment is one or two ASCII letters. `i.e`, `e.g`, `a.k.a`, `U.S` qualify;
                    // `lookup.sh`, `ci.yml`, `index.sqlite` and `2.8.3` do not (a digit is not a
                    // letter, and `sh`/`yml` fail on the segment before them). Structural, not a
                    // list. (Review #51 of PR #679.)
                    const ABBREVIATIONS: [&str; 5] = ["cf", "vs", "viz", "resp", "approx"];
                    let ends_an_abbreviation = |i: usize| {
                        let word = line[..i]
                            .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '.'))
                            .next()
                            .unwrap_or("");
                        // EVERY SEGMENT EXACTLY ONE LETTER. The first version allowed one OR two,
                        // which admits a two-letter-stem filename -- `db.md`, `de.md` -- and so
                        // extends a clause past a real sentence end: the `lookup.sh` class of
                        // review #51, one stem length over, in the fix for it. Every initialism
                        // this rule exists for is single-letter segments (`i.e`, `e.g`, `a.k.a`,
                        // `U.S`, `a.m`), so nothing is lost by the tighter form. Recorded rather
                        // than filed by the review that spotted it, on the grounds that no token in
                        // today's corpus hits it -- but the fix is one character and the direction
                        // is the permissive one. (Review #55 of PR #679.)
                        let dotted_initialism = word.contains('.')
                            && word.split('.').all(|seg| {
                                seg.len() == 1 && seg.chars().all(|c| c.is_ascii_alphabetic())
                            });
                        dotted_initialism || ABBREVIATIONS.contains(&word.to_lowercase().as_str())
                    };
                    let is_boundary = |i: usize, c: char| match c {
                        ';' | '—' => true,
                        '.' => {
                            !ends_an_abbreviation(i)
                                && line[i + c.len_utf8()..].chars().next().is_none_or(char::is_whitespace)
                        }
                        '-' => {
                            line[i..].starts_with("--")
                                && line[..i].chars().next_back().is_none_or(char::is_whitespace)
                                && line[i + 2..].chars().next().is_none_or(char::is_whitespace)
                        }
                        _ => false,
                    };
                    // `--` is two bytes wide; every other boundary is one char.
                    let boundary_width = |c: char| if c == '-' { 2 } else { c.len_utf8() };
                    // A DASH IS A BOUNDARY LOOKING BACK, NOT LOOKING FORWARD, and the asymmetry is
                    // the point rather than a shortcut. `;` and a sentence dot separate independent
                    // statements; a dash introduces an APPOSITIVE about the thing just named. So:
                    //
                    //   `KEY` — superseded by the chain head        <- the dash EXPLAINS `KEY`
                    //   the old row is superseded -- and per row `KEY`, do X   <- it does NOT
                    //
                    // Treating a dash as a boundary in both directions made the first spelling --
                    // the most idiomatic explanation in this repo -- a hard `make validate` error
                    // with rewording as the only escape, because the clause ended at the dash before
                    // `superseded` was ever seen. Treating it as a boundary in NEITHER direction
                    // reopens review #21, where one `superseded` written earlier silenced a live
                    // citation five joined lines later. Backward-only keeps both: an explanation
                    // that FOLLOWS the key still exempts it, and one that PRECEDES a dash no longer
                    // reaches past it. Every green control separated its explanation with `;`,
                    // ` -- ` or a dot, and those three behave identically -- which is why the
                    // control set was silent about the em-dash spelling specifically (review #25).
                    let clause_end = line[at..]
                        .char_indices()
                        .find(|&(i, c)| !matches!(c, '-' | '—') && is_boundary(at + i, c))
                        .map_or(line.len(), |(i, _)| at + i);
                    // `i + 1` would land INSIDE the em-dash: `—` is three bytes, and slicing a
                    // str on a non-boundary panics. The corpus contains em-dashes, so the
                    // round-trip test caught this immediately -- which is the argument for having
                    // run it against real content rather than fixtures alone.
                    let clause_start = line[..at]
                        .char_indices()
                        .filter(|&(i, c)| is_boundary(i, c))
                        .next_back()
                        .map_or(0, |(i, c)| i + boundary_width(c));

                    // TRIM TRAILING PUNCTUATION BEFORE LOOKING AT THE LAST TOKEN. A trailing colon
                    // survived, which made `Decision row: <KEY>` invisible -- THE ENVELOPE FORMAT
                    // this repo mandates and `.claude/hooks/register-check.sh` enforces as
                    // `ENVELOPE='Decision row:'`. The single most load-bearing citation form under
                    // `.claude/**` was the one the detector could not see (review #11).
                    // Strip quoting marks first, THEN look for an opening bracket, because the
                    // bracket is itself the citing signal in `(\`KEY\`, decided ...)`. Stripping
                    // it away before testing is what made that form invisible.
                    let raw_before = line[..at].trim_end();
                    // THE BACKTICK IS THE CITING SIGNAL, and `[` is not one at all. Accepting any
                    // `(` or `[` made a markdown LINK to the row's file -- `[KEY](path)`, the
                    // ordinary way a doc points at a record -- and any parenthetical that merely
                    // MENTIONS the key (`(KEY was the first attempt)`) into hard `make validate`
                    // errors, on `CLAUDE.md` among others, with rewording as the only escape. That
                    // is the "a red whose escape is silence" shape this file argues against
                    // everywhere. The form the arm was added for is SKILL.md's `` (`KEY`, decided
                    // ... `` -- a BACKTICKED key immediately inside a paren -- so require exactly
                    // that. `[` goes entirely: a link's TARGET is a path, and the
                    // `docs/decisions/<KEY>.yaml` arm below reaches it with the right semantics
                    // (pointing a session at a dead row's file IS the defect); the link's TEXT is
                    // not a citation on its own. Review #16 of PR #679; no green control covered
                    // either spelling, which is why the class was invisible.
                    let backticked = raw_before.ends_with('`');
                    let quoted = raw_before.trim_end_matches(['`', '"', '\'']).trim_end();
                    // ADJACENCY WAS AN ACCIDENT OF THE ONE SITE THE ARM WAS WRITTEN FOR.
                    // `quoted.ends_with('(')` catches `` (`KEY`, decided ... `` -- SKILL.md:326 --
                    // and misses `` (decided 2026-08-24, `KEY`) ``, which is SKILL.md:193, the
                    // line the comment three screens up NAMES as the form the marker-only-line fix
                    // restores. It is a wrapped markdown continuation, so `logical_units` joins it,
                    // the key lands mid-unit, `,` is not in the trim set (so `before` is non-empty)
                    // and `last` is a separator (so no citing token fires either): the site the
                    // records cite to prove the corpus is covered was the one still missed. It was
                    // caught in practice only because three OTHER SKILL.md lines red, so an author
                    // fixing those passes over it -- coverage by accident, which is the property
                    // this file spends thirty rounds removing from its neighbours. (Review #35.)
                    //
                    // BUT BARE CONTAINMENT WAS TOO WIDE, AND THE COMMENT THAT SHIPPED WITH IT WAS
                    // FALSE. It said containment "does not widen the class the adjacent form
                    // already accepted"; it does, and it re-admitted the exact sentence the
                    // `the`-plus-citing-noun narrowing exists to keep green. `the pin was rewritten
                    // (the `KEY` experiment was contaminated) in round 4` has `depth == 1` and a
                    // backtick, so it became a HARD error -- while the identical clause WITHOUT the
                    // parentheses stays green, because `last == "the"` and `experiment` is not a
                    // citing noun. The docstring names `the <KEY> experiment was contaminated` as a
                    // case that MUST stay green; punctuation the author did not think they were
                    // choosing decided it. `decision-superseded-authority` is an ERROR, so that
                    // reds `specs` AND `docs-validate`, and the required `codegen` check with them:
                    // nothing merges until the sentence is reworded or the word `superseded` is
                    // injected into it -- "a red whose escape is silence", from the arm added to
                    // close a MISS. And the backtick is not a distinguisher here: the records state
                    // that every row key in `CLAUDE.md`, `SKILL.md` and the register IS backticked,
                    // so bare containment fires on the house style. (Review #40.)
                    //
                    // So a parenthetical must CITE, not merely CONTAIN: some citing word has to
                    // appear inside it before the key. `(decided 2026-08-24, `KEY`)` and
                    // `(see `KEY` and the ADR)` do; `(the `KEY` experiment was contaminated)` does
                    // not. Adjacency keeps its own arm, so `` (`KEY`, decided ... `` -- where there
                    // is nothing between the paren and the key to carry a signal -- is unaffected.
                    // Green controls for the plain mention live beside the reds; there was none on
                    // either side, which is why the class was invisible for a round.
                    const PARENTHETICAL_CITES: [&str; 8] = [
                        "row", "rows", "per", "see", "decided", "decided_by", "reconsiders", "superseded_by",
                    ];
                    // A STACK, SO `open_paren` IS THE INNERMOST ONE STILL OPEN. Overwriting it on
                    // every `(` and clearing it only at depth 0 left it pointing at an INNER paren
                    // that had already closed: for `` (see (the wrapper) `KEY`) `` the window
                    // searched for a citing word was `the wrapper) `, which does not contain `see`,
                    // so the citation went unreported. Permissive, and the intent -- "some citing
                    // word has to appear inside the parenthetical before the key" -- describes the
                    // enclosing one. (Review #51.)
                    let mut open_stack: Vec<usize> = Vec::new();
                    for (i, c) in line[clause_start..at].char_indices() {
                        match c {
                            '(' => open_stack.push(clause_start + i),
                            ')' => {
                                open_stack.pop();
                            }
                            _ => {}
                        }
                    }
                    let depth = open_stack.len() as i32;
                    // INNERMOST, NOT OUTERMOST -- AND ROUND 51 CHOSE THE OTHER ONE AND PLANTED IT.
                    // The two differ only when an inner group is STILL OPEN at the key; for review
                    // #51's motivating case the inner paren has already been popped, so the stack
                    // holds one entry and the choice is invisible. Where they differ, `first()`
                    // FALSE-REDS the spelling this file's docstring names as one that must stay
                    // green: `(decided 2026-08-24 by ADR-... (the `KEY` experiment was
                    // contaminated))` picks up `decided` from the OUTER group and reds a sentence
                    // that is prose about the row. `decision-superseded-authority` is an ERROR, so
                    // that reds `specs`, `docs-validate` and the required check, with rewording as
                    // the only escape -- "a red whose escape is silence", through the arm added to
                    // close a miss. `last()` gives the identical verdict on #51's case and the
                    // right one here, because the intent -- "some citing word has to appear inside
                    // IT before the key" -- is about the parenthetical the key is actually in.
                    //
                    // WHAT THIS GIVES UP, stated rather than left implied: a citing word in an
                    // OUTER group with the key inside a still-open inner one -- `(see (the wrapper
                    // `KEY`))` -- is now a MISS. That is the deliberate direction: this file has
                    // ruled repeatedly that a false red on honest prose is the worse instrument,
                    // and both spellings now carry a control so the trade is visible rather than
                    // rediscovered. (Review #54 of PR #679, correcting review #51's choice.)
                    let open_paren = open_stack.last().copied();
                    let in_a_citing_parenthetical = backticked
                        && depth > 0
                        && open_paren.is_some_and(|o| {
                            line[o + 1..at]
                                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                                .any(|w| PARENTHETICAL_CITES.contains(&w.to_lowercase().as_str()))
                        });
                    let opens_a_parenthetical =
                        (backticked && quoted.ends_with('(')) || in_a_citing_parenthetical;
                    let mut before = quoted;
                    loop {
                        let next = before.trim_end().trim_end_matches([':', '(', '[', '*', '_', '-']);
                        if next.len() == before.len() {
                            break;
                        }
                        before = next;
                    }
                    let before = before.trim_end();
                    let last = before
                        .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                        .next()
                        .unwrap_or("")
                        .to_lowercase();
                    // TOKEN equality, not a suffix test: `ends_with("row")` also matched `narrow`,
                    // `borrow` and `arrow` -- the false-red class this file has retracted three
                    // times, carried into the rule written to stop the previous one. Green controls
                    // for all three live in `a_superseded_row_may_not_be_cited_as_live_authority`.
                    // `the` IS NOT A CITING TOKEN ON ITS OWN -- it is the definite article, and
                    // accepting it made this the false-red instrument the rest of this PR spends
                    // nine rounds retracting: `mirrors the <KEY> rollout`, `the <KEY> experiment
                    // was contaminated`, `narrower than the <KEY> surface` would each red
                    // `make validate` as a hard error, with no escape but rewording or injecting
                    // the word `superseded` into the clause. None of them tells a session to cite a
                    // dead row. The green controls missed it because every one of them avoided
                    // putting `the` immediately before the key. So `the <KEY>` counts only when the
                    // word AFTER the key is a citing noun.
                    // BOUNDED BY `clause_end`, the same boundary the exemption is scoped to. It
                    // used to trim forward over ALL non-alphanumerics, which walks straight over
                    // the sentence dot that ENDED the clause -- and, now that units are joined
                    // blocks, over the end of the physical line -- to read a word from the next
                    // sentence. So `narrower than the <KEY>. Decision rows are cheap...` matched
                    // `the` + `decision` and redded as a hard error, while a `superseded` in that
                    // following sentence could not exempt it because the clause was already over.
                    // `narrower than the <KEY>` is named three lines below as a case that MUST stay
                    // green; it did, only because of what the next word happened to be. No control
                    // put `the` before the key with a sentence break after it. (Review #20.)
                    let lookahead_end = clause_end.max(at + key.len());
                    let next_word = line[at + key.len()..lookahead_end]
                        .trim_start_matches(|c: char| !c.is_ascii_alphanumeric())
                        .split(|c: char| !c.is_ascii_alphanumeric())
                        .next()
                        .unwrap_or("")
                        .to_lowercase();
                    let the_plus_noun = last == "the"
                        && matches!(next_word.as_str(), "decision" | "row" | "record" | "ruling");
                    let cites = matches!(last.as_str(), "row" | "rows" | "per")
                        || the_plus_noun
                        // `reconsiders:` is the form the MOTIVATING INCIDENT produced -- the
                        // docstring says so in as many words -- and it was the one field name not
                        // recognised while its sibling `decided_by` was. It was dropped earlier
                        // because it false-redded this PR's own retraction comment; the
                        // clause-scoped `superseded` exemption now covers that, so it can come
                        // back doing the job it was named for.
                        || matches!(last.as_str(), "decided_by" | "reconsiders" | "superseded_by")
                        || before.to_lowercase().ends_with("decided_by")
                        // The key opening a line or a parenthetical, both live in SKILL.md today
                        // (`(\`KEY\`, decided ...` and a line beginning with the key) and both
                        // invisible to the first version of this rule.
                        || opens_a_parenthetical
                        // A BACKTICKED key opening a line is a citation (`SKILL.md:193`); a bare
                        // one is ordinary prose ("OLD-ROW was the predecessor."). The distinction
                        // is the backtick, and without it the green control for prose reds.
                        // ...but NOT when a `[` was what the trim loop ate to empty `before`.
                        // Review #16 narrowed `opens_a_parenthetical` to drop `[` on the grounds
                        // that a link's TEXT is not a citation -- and this arm re-admitted it,
                        // because `[` is in the trim set, so `` [`KEY`](target) `` left `before`
                        // empty with a backtick behind it and redded. Both green controls used the
                        // UNBACKTICKED `[KEY]`, i.e. the spelling nobody writes; every row key in
                        // `CLAUDE.md`, `SKILL.md` and the register is backticked. A link to an ADR
                        // or an issue would have been a hard error with rewording as the only
                        // escape. (`` [`KEY`](docs/decisions/KEY.yaml) `` stays red on the PATH arm
                        // below, which is the intended behaviour.) Review #20.
                        || (before.is_empty()
                            && line[..at].ends_with('`')
                            && !raw_before.trim_end_matches('`').ends_with('['))
                        // `docs/decisions/<KEY>.yaml` -- THE FORM THE REGISTER ITSELF MANDATES, and
                        // the one arm this rule shipped without. `SKILL.md` and `CLAUDE.md` both
                        // route resolution through "exact `docs/decisions/<KEY>.yaml` resolution",
                        // so the HIGHEST-authority way for a `.claude/**` file to point a session
                        // at a dead row -- a bare path, no verb, no backtick -- walked past a gate
                        // whose whole subject is that pointer, while the weaker prose forms were
                        // caught. `before` keeps its trailing `/` (the trim set is `:([*_-`), so
                        // one arm closes it. Review #12 of PR #679.
                        //
                        // `decisions/` must be the DIRECTORY, not a suffix: a bare `ends_with`
                        // also matched `docs/old-decisions/`, which is the suffix-not-token defect
                        // this file already retracted once over `narrow`/`borrow`/`arrow`. The
                        // preceding character decides, and the green control names the case.
                        || before.strip_suffix("decisions/").is_some_and(|head| {
                            head.chars()
                                .next_back()
                                .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
                        });
                    if !cites {
                        continue;
                    }
                    // THE CITING TOKEN MAY NOT SUPPLY ITS OWN EXEMPTION. `superseded_by` contains
                    // "superseded", and this test runs BEFORE `cites` -- so the `superseded_by` arm
                    // added below could never fire: `last == "superseded_by"` guarantees the
                    // substring sits in the clause, which guarantees `continue`. A `.claude/**`
                    // file mirroring a row's fields as `superseded_by: <KEY>` therefore stayed
                    // green after `<KEY>` was itself superseded further down the chain -- and this
                    // change creates the register's FIRST two-link chain, so "the successor is
                    // superseded later" is the next state, not a corner case. An arm dead by
                    // construction reads as coverage it never provided (review #21).
                    //
                    // So the exempting word is looked for in the clause with the citing token
                    // blanked out: an explanation still exempts, a field name no longer exempts
                    // itself.
                    // BLANK ON THE ORIGINAL SLICE, LOWERCASE AFTER. `lo`/`hi` are offsets into
                    // `line`, and the first version applied them to `line[..].to_lowercase()` --
                    // but Unicode lowercasing is NOT length-preserving (`ẞ` U+1E9E is 3 bytes and
                    // lowercases to `ß` at 2; `İ` U+0130 is 2 and lowercases to `i̇` at 3), so one
                    // such character anywhere before the citing token shifts every later offset.
                    // `is_char_boundary` stops the panic and nothing else, so the blanking lands in
                    // the wrong place or is skipped, SILENTLY -- and the permissive direction is
                    // the one that bites: the guard is skipped, `superseded_by` is left intact, and
                    // the arm this whole block exists to resurrect goes dead by construction again
                    // (review #21's defect, reopened). Latent on today's ASCII-plus-em-dash corpus,
                    // but the corpus includes `CLAUDE.md` and `.claude/**` prose by design, and no
                    // control put a length-changing character before the token. Doing it in this
                    // order makes the class unrepresentable rather than guarded. (Review #49.)
                    let token_start = before.len().saturating_sub(last.len());
                    let mut exempt_text = line[clause_start..clause_end].to_string();
                    if token_start >= clause_start && before.len() <= clause_end {
                        let lo = token_start - clause_start;
                        let hi = before.len() - clause_start;
                        if lo <= hi && hi <= exempt_text.len() && exempt_text.is_char_boundary(lo) && exempt_text.is_char_boundary(hi) {
                            exempt_text.replace_range(lo..hi, &" ".repeat(hi - lo));
                        }
                    }
                    let exempt_text = exempt_text.to_lowercase();
                    if exempt_text.contains("superseded") {
                        continue;
                    }
                    issues.push(err(
                        "decision-superseded-authority",
                        path.clone(),
                        format!(
                            "cites `{}` as a live row, but that row is SUPERSEDED. Name the CHAIN HEAD instead: a `reconsiders:` pointing at a superseded row is rejected, so this sends the next session into a gate error on the path it is trying to help them down. From line {}, clause: {}",
                            key,
                            unit.line_at(at),
                            line[clause_start..clause_end].trim()
                        ),
                    ));
                    break;
                }
            }
        }
    }
    issues
}

/// A DOC COMMENT BINDS TO THE FOLLOWING ITEM, and a blank line does NOT break the block: every
/// preceding `///` run attaches to the next item. This paragraph was left two functions up, so it
/// documented `validate_no_superseded_row_is_cited_as_authority` while this function had none --
/// and the first attempt to fix it added a parenthetical DESCRIBING the mis-binding rather than
/// moving the text, which is a comment asserting the opposite of what the compiler does. Moved.
/// §22b: the committed index region must equal the fold over the source rows — at VALIDATE time,
/// before check-drift, with the clearer message. Same emit function as generation, same
/// `legacy_count` source, trimmed identically to the injector's `\n\n` framing, so the two gates
/// can never disagree. The validator reads the ROW FILES as truth and treats the region purely as
/// the projection under test (founder requirement 11).
pub(crate) fn validate_decisions_index_sync(
    rows: &[DecisionRow],
    legacy_count: usize,
    register_content: &str,
) -> Vec<Issue> {
    let loc = "docs/proposals/DECISIONS.md".to_string();
    match extract_decisions_region(register_content) {
        None => vec![err(
            "decision-index-stale",
            loc,
            "the GENERATED:decisions marker pair is missing — the register index cannot be silently absent; restore the markers and run `make generate`.".into(),
        )],
        Some(region) if region != emit_decisions_index(rows, legacy_count).trim() => vec![err(
            "decision-index-stale",
            loc,
            "the committed GENERATED:decisions region disagrees with docs/decisions/*.yaml — the projection may never disagree with its source rows: run `make generate` and commit the regenerated region in the same change.".into(),
        )],
        Some(_) => Vec::new(),
    }
}

// ─── The decision-form template contract (founder requirement 6, 2026-08-21) ────────────────────

/// The committed form template's example FORM must anchor each option-question to a register row
/// via a `row:` field, so a form authored from the template starts from the contract. Published
/// form COPIES are uncommitted artifacts and are NOT mechanically validated — that boundary is
/// recorded in the ADR, not papered over here.
pub(crate) fn validate_decision_form_template(path: &str, content: &str) -> Vec<Issue> {
    let form_region = content
        .find("const FORM")
        .and_then(|s| content[s..].find("DO NOT EDIT BELOW").map(|e| &content[s..s + e]))
        .unwrap_or("");
    if form_region.contains("row:") {
        Vec::new()
    } else {
        vec![err(
            "decision-form-template-row",
            path.to_string(),
            "the template's example FORM declares no `row:` field — every option-question on a decision form anchors to its register row key (`row: \"<KEY>\"`), so a form authored from this template starts from the contract.".into(),
        )]
    }
}

// ─── The generated index (REG-3(a): only the index; the prose stays authored) ───────────────────

fn cell(s: &str) -> String {
    // GFM cell-splitting sees raw pipes even inside code spans (§13b commentary in proposals.rs);
    // escape them, and double a trailing backslash so it cannot escape the closing delimiter.
    let mut out = s.replace('|', "\\|").replace('\n', " ");
    if out.ends_with('\\') {
        out.push('\\');
    }
    out
}

/// DISPLAY rank, not the vocabulary order: pending rows (open, then deferred) sit on top, closed
/// rows (decided, superseded, withdrawn) below — a deferred row is still pending, not closed.
fn status_rank(s: &str) -> usize {
    match s {
        "open" => 0,
        "deferred" => 1,
        "decided" => 2,
        "superseded" => 3,
        "withdrawn" => 4,
        _ => 9,
    }
}

/// The DECISIONS.md index body, deterministic: open first (oldest first, then key), then deferred,
/// decided, superseded, withdrawn (by key). No clocks, no ages — the "Since" column is the stored
/// `opened` date, because a generated artifact that computes an age is red on the drift gate every
/// day (the reason the PROP's mocked Age column is NOT emitted).
pub(crate) fn emit_decisions_index(rows: &[DecisionRow], legacy_count: usize) -> String {
    let mut sorted: Vec<&DecisionRow> = rows.iter().collect();
    sorted.sort_by(|a, b| {
        let (ra, rb) = (status_rank(a.get("status").unwrap_or("")), status_rank(b.get("status").unwrap_or("")));
        ra.cmp(&rb)
            .then_with(|| {
                if ra <= 1 {
                    a.get("opened").unwrap_or("").cmp(b.get("opened").unwrap_or(""))
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then_with(|| a.stem.cmp(&b.stem))
    });
    let mut lines = vec![
        "### The machine-readable index — one row per `docs/decisions/<KEY>.yaml` (REG-2/REG-4)".to_string(),
        String::new(),
        "| Key | Status | Since | Question | Owner |".to_string(),
        "| --- | --- | --- | --- | --- |".to_string(),
    ];
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut oldest_open: Option<&DecisionRow> = None;
    for r in &sorted {
        let status = r.get("status").unwrap_or("?");
        *counts.entry(DECISION_STATUSES.iter().find(|s| **s == status).copied().unwrap_or("?")).or_insert(0) += 1;
        if status == "open" && oldest_open.is_none() {
            oldest_open = Some(r);
        }
        let closing = match status {
            "decided" | "superseded" => r.get("decided_by").unwrap_or(""),
            _ => "",
        };
        // A SUPERSEDED ROW MUST NAME ITS SUCCESSOR HERE. The arrow was built from `decided_by`
        // for `decided` and `superseded` alike, so a superseded row pointed at its OWN deciding
        // record and gave the reader no route to the head. That is the one GENERATED surface --
        // the thing `make generate` puts in front of the next session -- and it was the only
        // surface this change did not rewrite to name the head, while simultaneously making both
        // of the reader's next moves illegal: `reconsiders:` at a superseded row is rejected, and
        // so is citing it under `.claude/**`. Pointed out on PR #679; the data is already on the
        // row and `validate_decision_rows` guarantees it resolves.
        //
        // AND IT MUST NOT DROP THE DECIDING RECORD WHILE DOING SO. The first version REPLACED the
        // `decided_by` arrow with the successor, which routed the reader correctly and deleted the
        // only index reference to the predecessor's deciding record. For this chain that record is
        // `PROP-20260822-171212` -- the option-space authority for the whole thing, rewritten in
        // this same change to name the head -- and after the replacement it appeared NOWHERE in
        // the register index. The routing argument justifies naming the successor FIRST, not
        // dropping the record: a superseded ROW is a dead end (its `reconsiders:` is rejected and
        // citing it under `.claude/**` is gate-rejected), but its deciding RECORD stays readable
        // and is often the live design document. Both, successor first. (Review #42 of PR #679.)
        let question = match (closing.is_empty(), r.get("superseded_by")) {
            (true, Some(head)) if status == "superseded" => format!(
                "{} -> superseded by `{}`",
                r.get("question").unwrap_or(""),
                head
            ),
            (false, Some(head)) if status == "superseded" => format!(
                // "(this row decided by ...)", NOT "(decided by ...)". `closing` is THIS row's
                // `decided_by`, and rendered bare inside parentheses immediately after the
                // successor's key it reads as the SUCCESSOR's deciding record. The committed line
                // said `superseded by `RETRIEVAL-QMD-CI` (decided by PROP-20260822-171212)` --
                // false: the head was decided by `ADR-20260824-205911`, and the PROP decided the
                // row that is now dead. On the one GENERATED surface a reader consults to find the
                // live authority, on a chain where both of their other moves (`reconsiders:` at the
                // dead row, citing it under `.claude/**`) are gate-rejected. Round 42 got the
                // content right -- both, successor first -- and the binding wrong. (Review #59.)
                "{} -> superseded by `{}` (this row decided by {})",
                r.get("question").unwrap_or(""),
                head,
                closing
            ),
            (true, _) => r.get("question").unwrap_or("").to_string(),
            (false, _) => format!("{} -> {}", r.get("question").unwrap_or(""), closing),
        };
        lines.push(format!(
            "| `{}` | {} | {} | {} | {} |",
            cell(&r.stem),
            status,
            r.get(if status_rank(status) <= 1 { "opened" } else { "decided" }).or(r.get("opened")).unwrap_or("-"),
            cell(&question),
            r.get("owner").unwrap_or("-"),
        ));
    }
    let count_str = DECISION_STATUSES
        .iter()
        .filter_map(|s| counts.get(s).map(|n| format!("{} {}", n, s)))
        .collect::<Vec<_>>()
        .join(" · ");
    lines.push(String::new());
    let oldest = oldest_open
        .map(|r| {
            format!(
                " Oldest open row: `{}` since {} (owner: {}).",
                r.stem,
                r.get("opened").unwrap_or("?"),
                r.get("owner").unwrap_or("?")
            )
        })
        .unwrap_or_default();
    lines.push(format!("**Migrated rows: {} — {}.**{}", sorted.len(), count_str, oldest));
    lines.push(String::new());
    // ONE canonical boundary text (2026-08-21 verification slice): the emitter test, §22b
    // index-sync and check-drift all compare this single string — never an assembled paraphrase.
    // Deliberately fold-over-HEAD only: "migrated in the current change" is NOT derivable from
    // committed state, and the diff of these lines IS the per-change migration record.
    const LEGACY_TAIL: &str = "(`docs/decisions/_legacy.yaml`, the closed allowlist — a declared \
migration boundary, never an authority and never a founder-question bypass). **This index is NOT \
exhaustive of open decisions.** Migration is mandatory, in the same change, on any of: \
decision-question reference · amendment · reopening/challenge (`reconsiders`) · explicit dispatch. \
The diff of these lines is the per-change migration record.";
    lines.push(format!("**Legacy rows remaining: {}** {}", legacy_count, LEGACY_TAIL));
    lines.push(String::new());
    lines.push(
        "For every key above, `docs/decisions/<KEY>.yaml` is **authoritative for CURRENT status**; the \
prose sections below are its history and their glyphs are not current status."
            .to_string(),
    );
    lines.join("\n")
}

// ─── §22d — dispatch-card decision references (founder requirement 6, ADR-20260821-103403) ──────

/// Every `docs/dispatch/*.md`, sorted; the structured card surface the coordinator authors.
pub(crate) fn load_dispatch_files(root: &std::path::Path) -> Vec<(String, String)> {
    let dir = root.join("docs/dispatch");
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        let mut paths: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
            .collect();
        paths.sort();
        for p in paths {
            if let (Some(name), Ok(content)) = (p.file_name().and_then(|n| n.to_str()), fs::read_to_string(&p)) {
                out.push((format!("docs/dispatch/{}", name), content));
            }
        }
    }
    out
}

/// A `Decision row:` line on a committed dispatch card must name a DECLARED, non-legacy key —
/// declare-before-ask on the card surface. Deliberately RESOLUTION-ONLY: the row's status is
/// enforced at ask time by the hook, never here, because a committed card is a fact at its
/// timestamp and a rule that reddens history when the row later closes would be the
/// projection-rebuild sin in gate form (2026-08-21 briefing, unanimous). Fenced blocks are quoted
/// output and are not scanned.
pub(crate) fn validate_dispatch_card_rows(
    files: &[(String, String)],
    declared: &BTreeSet<String>,
    legacy: &[String],
) -> Vec<Issue> {
    let mut issues = Vec::new();
    for (path, content) in files {
        let mut fence: Option<(char, usize)> = None;
        for (li, line) in content.lines().enumerate() {
            let t = line.trim_start();
            let ind = line.len() - t.len();
            if let Some((c, n)) = fence {
                let run = t.chars().take_while(|&x| x == c).count();
                if ind < 4 && run >= n && t.chars().all(|x| x == c || x.is_whitespace()) {
                    fence = None;
                }
                continue;
            }
            let c0 = t.chars().next().unwrap_or(' ');
            let run0 = t.chars().take_while(|&x| x == c0).count();
            if ind < 4 && (c0 == '`' || c0 == '~') && run0 >= 3 {
                fence = Some((c0, run0));
                continue;
            }
            let Some(pos) = line.find("Decision row:") else { continue };
            let after = line[pos + "Decision row:".len()..].trim_start();
            let token: String = after
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '-')
                .collect();
            let loc = format!("{}:{}", path, li + 1);
            if token.len() < 3 || !token.chars().next().unwrap().is_ascii_uppercase() {
                issues.push(err(
                    "decision-card-row",
                    loc,
                    format!("`Decision row:` carries no valid row key (line: `{}`).", line.trim()),
                ));
            } else if declared.contains(&token) {
                // status deliberately NOT checked — see the doc comment.
            } else if legacy.iter().any(|l| l == &token) {
                issues.push(err(
                    "decision-card-row",
                    loc,
                    format!("`Decision row: {}` names a LEGACY prose-only row — a card that escalates it migrates it in the same change (docs/decisions/README.md burn-down triggers); legacy is never a bypass.", token),
                ));
            } else {
                issues.push(err(
                    "decision-card-row",
                    loc,
                    format!("`Decision row: {}` names no declared row — declare docs/decisions/{}.yaml first (declare-before-ask), or fix the spelling.", token, token),
                ));
            }
        }
    }
    issues
}
