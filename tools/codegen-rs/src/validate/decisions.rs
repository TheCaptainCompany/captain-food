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
            return corpus.adr_files.iter().any(|f| f.starts_with(rest));
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
        let question = if closing.is_empty() {
            r.get("question").unwrap_or("").to_string()
        } else {
            format!("{} -> {}", r.get("question").unwrap_or(""), closing)
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
