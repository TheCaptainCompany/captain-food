//! §19 — the business-metric catalog (`specs/business_metrics.yaml`, ADR-20260811-014129 /
//! PROP-20260810-234225 D8): projections are declared folds over `domain_events`, metrics reads
//! over them. Reference RESOLUTION and target KINDS are §1/§1b's job (every `$ref` here is under
//! REF_CONTRACT); this section owns the semantic shape:
//!
//! - closed sets (`grain`, measure `type`, `bucket`, value ops) — a typo is red, never a
//!   silently wrong metric (the #413 class);
//! - `bam-fold-key-not-on-every-event` — every fold source event carries every key property, so
//!   the fold is provably total (the rule PROP-20260810-234225 names; what it actually catches
//!   is a wrong grain, D8a);
//! - `bam-measure-foreign` — a metric's `groupBy`/`value` measures belong to its own `over`
//!   projection (a resolvable ref into the WRONG projection would be a silently wrong grouping);
//! - fold rows are checkable shapes: `set`+`from`/`to`, `add`+`from`, target measures declared.

use crate::*;

const GRAINS: [&str; 2] = ["ENTITY", "AGGREGATE"];
const MEASURE_TYPES: [&str; 4] = ["counter", "sum", "gauge", "set"];
const BUCKETS: [&str; 5] = ["HOUR", "DAY", "WEEK", "MONTH", "DAYPART"];
const VALUE_OPS: [&str; 4] = ["sum", "countRows", "ratio", "percentiles"];

/// The `#/projections/<P>/measures/<m>` tail of a same-file measure ref, if it is one.
fn measure_ref(v: &Value) -> Option<(String, String)> {
    let r = v.get("$ref")?.as_str()?;
    let pr = parse_ref(r)?;
    if !pr.file.is_empty() && pr.file != "business_metrics.yaml" {
        return None;
    }
    match pr.path.as_slice() {
        // path = [projections, <projection>, measures, <measure>]
        [root, projection, measures, measure] if root == "projections" && measures == "measures" => {
            Some((projection.clone(), measure.clone()))
        }
        _ => None,
    }
}

pub(crate) fn validate_business_metrics(model: &Model, issues: &mut Vec<Issue>) {
    let Some(doc) = model.defs.get("business_metrics.yaml") else { return };

    // Event name -> its declared property names (for the fold-key totality proof).
    let event_props = |event: &str| -> Option<BTreeSet<String>> {
        model
            .defs
            .get("events.yaml")
            .and_then(|e| e.get(event))
            .and_then(|e| e.get("properties"))
            .and_then(|p| p.as_mapping())
            .map(|m| m.keys().filter_map(|k| k.as_str().map(str::to_string)).collect())
    };

    let projections = doc.get("projections").and_then(|p| p.as_mapping());
    let mut declared_measures: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    if let Some(projections) = projections {
        for (pk, proj) in projections {
            let pname = pk.as_str().unwrap_or("?");
            let at = format!("business_metrics.yaml/projections/{pname}");

            match proj.get("grain").and_then(|g| g.as_str()) {
                Some(g) if GRAINS.contains(&g) => {}
                other => issues.push(err(
                    "bam-closed-set",
                    format!("{at}.grain"),
                    format!("grain must be one of {} (got {:?}).", GRAINS.join("|"), other),
                )),
            }

            // Key properties (by referenced property NAME — the derived column name, D8).
            let key_props: Vec<String> = proj
                .get("key")
                .and_then(|k| k.as_sequence())
                .map(|s| {
                    s.iter()
                        .filter_map(|k| k.get("from"))
                        .filter_map(|f| f.get("$ref"))
                        .filter_map(|r| r.as_str())
                        .filter_map(|r| parse_ref(r))
                        .filter_map(|pr| pr.path.last().cloned())
                        .collect()
                })
                .unwrap_or_default();
            if key_props.is_empty() {
                issues.push(err(
                    "bam-projection-no-key",
                    at.clone(),
                    "a projection must declare >=1 `key` entry (`from:` a $ref into an event property).".into(),
                ));
            }

            // Measures: a MAP keyed by name (the declaration IS the key — resolvable same-file).
            let mut measures: BTreeSet<String> = BTreeSet::new();
            if let Some(ms) = proj.get("measures").and_then(|m| m.as_mapping()) {
                for (mk, mv) in ms {
                    let mname = mk.as_str().unwrap_or("?");
                    measures.insert(mname.to_string());
                    match mv.get("type").and_then(|t| t.as_str()) {
                        Some(t) if MEASURE_TYPES.contains(&t) => {}
                        other => issues.push(err(
                            "bam-closed-set",
                            format!("{at}.measures.{mname}.type"),
                            format!(
                                "measure type must be one of {} (got {:?}).",
                                MEASURE_TYPES.join("|"),
                                other
                            ),
                        )),
                    }
                    // An envelope source is `{ envelope: occurredAt, bucket? }` — the one place
                    // infrastructure metadata legitimately feeds a business fold (D8).
                    if let Some(of) = mv.get("of") {
                        if let Some(env) = of.get("envelope").and_then(|e| e.as_str()) {
                            if env != "occurredAt" {
                                issues.push(err(
                                    "bam-envelope-unknown",
                                    format!("{at}.measures.{mname}.of"),
                                    format!("envelope source '{env}' is not a known envelope field (occurredAt)."),
                                ));
                            }
                        }
                        if let Some(b) = of.get("bucket").and_then(|b| b.as_str()) {
                            if !BUCKETS.contains(&b) {
                                issues.push(err(
                                    "bam-closed-set",
                                    format!("{at}.measures.{mname}.of.bucket"),
                                    format!("bucket must be one of {} (got '{b}').", BUCKETS.join("|")),
                                ));
                            }
                        }
                    }
                }
            }

            // Fold rows: each `on` event must carry EVERY key property (totality — the fold can
            // always find its row), and each `set`/`add` must target a declared OWN measure.
            if let Some(fold) = proj.get("fold").and_then(|f| f.as_sequence()) {
                for (i, row) in fold.iter().enumerate() {
                    let fat = format!("{at}.fold[{i}]");
                    let on = row
                        .get("on")
                        .and_then(|o| o.get("$ref"))
                        .and_then(|r| r.as_str())
                        .and_then(ref_name);
                    match on.as_deref() {
                        None => issues.push(err(
                            "bam-fold-no-on",
                            fat.clone(),
                            "fold row has no `on:` event $ref.".into(),
                        )),
                        Some(event) => {
                            if let Some(props) = event_props(event) {
                                let missing: Vec<&String> =
                                    key_props.iter().filter(|k| !props.contains(*k)).collect();
                                if !missing.is_empty() {
                                    issues.push(err(
                                        "bam-fold-key-not-on-every-event",
                                        fat.clone(),
                                        format!(
                                            "folded event '{event}' does not carry key propert{} {:?} — the fold cannot address its row (usually a GRAIN error, PROP-20260810-234225 D8a).",
                                            if missing.len() == 1 { "y" } else { "ies" },
                                            missing
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                    let target = row.get("set").or_else(|| row.get("add"));
                    match target.and_then(measure_ref) {
                        None => issues.push(err(
                            "bam-fold-no-target",
                            fat.clone(),
                            "fold row must `set:` or `add:` a same-file measure $ref (`#/projections/<P>/measures/<m>`).".into(),
                        )),
                        Some((proj_ref, measure)) => {
                            if proj_ref != pname || !measures.contains(&measure) {
                                issues.push(err(
                                    "bam-measure-foreign",
                                    fat.clone(),
                                    format!(
                                        "fold targets '{proj_ref}/{measure}', which is not a declared measure of projection '{pname}'."
                                    ),
                                ));
                            }
                        }
                    }
                    let has_from = row.get("from").is_some();
                    let has_to = row.get("to").is_some();
                    if row.get("set").is_some() && !has_from && !has_to {
                        issues.push(err(
                            "bam-fold-no-source",
                            fat.clone(),
                            "a `set:` row needs a `from:` (payload $ref or envelope) or a literal `to:`.".into(),
                        ));
                    }
                    if row.get("add").is_some() && !has_from {
                        issues.push(err(
                            "bam-fold-no-source",
                            fat,
                            "an `add:` row needs a `from:` numeric payload $ref.".into(),
                        ));
                    }
                }
            }
            declared_measures.insert(pname.to_string(), measures);
        }
    }

    // Metrics: the read layer.
    if let Some(metrics) = doc.get("metrics").and_then(|m| m.as_mapping()) {
        for (mk, metric) in metrics {
            let mname = mk.as_str().unwrap_or("?");
            let at = format!("business_metrics.yaml/metrics/{mname}");

            if metric
                .get("question")
                .and_then(|q| q.as_str())
                .map(|q| q.trim().is_empty())
                .unwrap_or(true)
            {
                issues.push(err(
                    "bam-question-empty",
                    at.clone(),
                    "a metric DECLARES the question it answers (ADR-20260810-234225 clause 3).".into(),
                ));
            }
            if metric.get("activity").and_then(|a| a.get("$ref")).is_none() {
                issues.push(err(
                    "bam-no-activity",
                    at.clone(),
                    "a metric binds a persona ACTIVITY (`activity:` $ref into stories.yaml — the unit of the obligation).".into(),
                ));
            }

            let over = metric
                .get("over")
                .and_then(|o| o.get("$ref"))
                .and_then(|r| r.as_str())
                .and_then(|r| parse_ref(r))
                .and_then(|pr| pr.path.get(1).cloned());
            let Some(over) = over else {
                issues.push(err(
                    "bam-no-over",
                    at,
                    "a metric reads `over:` exactly one declared projection ($ref).".into(),
                ));
                continue;
            };
            let own = declared_measures.get(&over).cloned().unwrap_or_default();

            // Every measure ref used by groupBy/value must belong to the `over` projection.
            let mut used: Vec<(String, String, String)> = Vec::new(); // (site, proj, measure)
            if let Some(gb) = metric.get("groupBy").and_then(|g| g.as_sequence()) {
                for (i, g) in gb.iter().enumerate() {
                    if let Some((p, m)) = measure_ref(g) {
                        used.push((format!("{at}.groupBy[{i}]"), p, m));
                    }
                }
            }
            let mut value_refs: Vec<(String, String)> = Vec::new();
            if let Some(v) = metric.get("value") {
                collect_measure_refs(v, &mut value_refs);
                for (p, m) in value_refs {
                    used.push((format!("{at}.value"), p, m));
                }
                // The value op itself is a closed set.
                if let Some(map) = v.as_mapping() {
                    for (op, _) in map {
                        let op = op.as_str().unwrap_or("?");
                        if !VALUE_OPS.contains(&op) {
                            issues.push(err(
                                "bam-closed-set",
                                format!("{at}.value"),
                                format!("value op must be one of {} (got '{op}').", VALUE_OPS.join("|")),
                            ));
                        }
                    }
                }
            }
            for (site, p, m) in used {
                if p != over || !own.contains(&m) {
                    issues.push(err(
                        "bam-measure-foreign",
                        site,
                        format!(
                            "references measure '{p}/{m}', which is not a declared measure of the metric's `over` projection '{over}'."
                        ),
                    ));
                }
            }
        }
    }
}

/// Every same-file measure `$ref` under a `value:` subtree.
fn collect_measure_refs(v: &Value, out: &mut Vec<(String, String)>) {
    match v {
        Value::Mapping(m) => {
            if let Some(pm) = measure_ref(v) {
                out.push(pm);
                return;
            }
            for (_, val) in m {
                collect_measure_refs(val, out);
            }
        }
        Value::Sequence(s) => {
            for val in s {
                collect_measure_refs(val, out);
            }
        }
        _ => {}
    }
}
