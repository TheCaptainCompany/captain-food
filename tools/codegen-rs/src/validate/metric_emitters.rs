//! §20 — DECLARED-BUT-SILENT metrics (#608, beck's gate condition).
//!
//! The defect this rule exists for is not hypothetical and it is not old: `specs/observability.yaml`
//! declared `payment_authorized_unsettled_age_seconds` as *the* dead-man's switch for a payment
//! that is never settled, and it had **zero emit sites in `crates/**`**. A contract that declares a
//! signal nothing emits reads exactly like a contract that is working: the dashboard is empty
//! either way, and "no data" on a money-path alarm is the most reassuring failure there is. #608
//! was one commit away from shipping a SECOND one.
//!
//! **What "has an emitter" means here, precisely.** A metric name is emittable only if it can be
//! spelled at a call site, and this repo spells metric names exactly once — as a constant in
//! `crates/telemetry/src/contract.rs` — and builds every instrument in
//! `crates/telemetry/src/meters.rs`. So the rule is two links:
//!
//! 1. the declared name is the VALUE of some `pub const` in `contract.rs`, and
//! 2. that constant's IDENTIFIER is referenced (as `metric::IDENT`) in `meters.rs`.
//!
//! Both links are necessary and neither is sufficient: a name with no constant cannot be emitted at
//! all, and a constant no instrument is built from is a string in a header file. What the rule
//! deliberately does NOT claim is that the emitting function is CALLED on the workflow's path —
//! that is a behaviour question, and the answer is a test that looks at the emitted series
//! (`crates/infrastructure/tests/authorized_no_birth_metric.rs`), never a text scan.
//!
//! **WARNING, not error, and the ratchet is the point.** A large existing population fails this
//! today (webhook ingestion, prospection, refunds, SIRENE, delivery dispatch — whole contracts
//! written before their runtimes). Turning those into errors would make `make validate` red on
//! `main` and the only fast way out would be to weaken the rule. As a WARNING it joins the §17
//! per-rule ratchet, which is exact in both directions: the current population is frozen, ONE MORE
//! is a hard gate failure, and every one that gains an emitter tightens the baseline. That is the
//! whole behaviour beck asked for — "shipping a second declared-but-silent contract" becomes
//! impossible — without a flag day.
//!
//! **The count is deliberately not written here.** It lives in
//! `tools/codegen-rs/warning-baseline.json`, which the gate maintains; a number in prose is a
//! second, unmaintained copy that goes stale the first time the surface moves — and this is the
//! module doc of the rule whose entire subject is "a declared number must be true". Run the gate.

use std::path::Path;

use crate::model::{warn, Issue};

/// Where the metric-name constants live: the ONE place a metric name is spelled in Rust.
const CONTRACT_RS: &str = "crates/telemetry/src/contract.rs";
/// Where instruments are built from those constants.
const METERS_RS: &str = "crates/telemetry/src/meters.rs";

/// One declared metric: `(contract feature, `metrics`|`business_metrics`, name)`.
pub(crate) type DeclaredMetric = (String, String, String);

/// Every metric and business_metric declared by `observability.yaml`, in a stable order.
pub(crate) fn declared_metrics(model: &crate::Model) -> Vec<DeclaredMetric> {
    let mut out: Vec<DeclaredMetric> = Vec::new();
    let Some(obs) = model.defs.get("observability.yaml").and_then(|x| x.as_mapping()) else {
        return out;
    };
    for (fk, contract) in obs {
        let Some(feature) = fk.as_str() else { continue };
        for kind in ["metrics", "business_metrics"] {
            let Some(seq) = contract.get(kind).and_then(|x| x.as_sequence()) else { continue };
            for m in seq {
                if let Some(name) = m.get("name").and_then(|n| n.as_str()) {
                    out.push((feature.to_string(), kind.to_string(), name.to_string()));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Read the two Rust files the rule inspects. A missing file yields an empty string, which makes
/// EVERY metric report — loud, never a silent pass, exactly like §16's unparsed-DDL posture.
pub(crate) fn load_metric_emitter_sources(repo_root: &Path) -> (String, String) {
    let read = |rel: &str| std::fs::read_to_string(repo_root.join(rel)).unwrap_or_default();
    (read(CONTRACT_RS), read(METERS_RS))
}

/// The identifier of the `pub const` in `contract.rs` whose value is `name`, if any.
///
/// A hand-rolled scan rather than a regex dependency: the shape is fixed by rustfmt and by the
/// module's own convention, and it must tolerate the two-line form rustfmt produces for long names
/// (`pub const X: &str =\n    "very_long_name";`).
fn const_named(contract_rs: &str, name: &str) -> Option<String> {
    let needle = format!("\"{name}\";");
    for (idx, _) in contract_rs.match_indices(&needle) {
        // Walk back to the `pub const` that owns this literal, across at most one line break.
        let head = &contract_rs[..idx];
        let Some(eq) = head.rfind('=') else { continue };
        let Some(decl) = head[..eq].rfind("pub const ") else { continue };
        // Nothing but whitespace may separate `=` from the literal: otherwise this is some other
        // expression that happens to end in the same string.
        if !head[eq + 1..].trim().is_empty() {
            continue;
        }
        let after = &head[decl + "pub const ".len()..eq];
        let ident = after.split(':').next().unwrap_or("").trim();
        if !ident.is_empty() {
            return Some(ident.to_string());
        }
    }
    None
}

/// Is `ident` referenced as a metric constant in `meters.rs`?
fn instrument_built_from(meters_rs: &str, ident: &str) -> bool {
    let needle = format!("metric::{ident}");
    meters_rs.match_indices(&needle).any(|(i, _)| {
        // Reject a PREFIX match: `metric::ORDER_LANE_WATCH` must not satisfy a lookup for
        // `ORDER_LANE_WATCH_HEARTBEAT_TOTAL`'s shorter sibling and vice versa.
        meters_rs[i + needle.len()..]
            .chars()
            .next()
            .map(|c| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(true)
    })
}

/// The rule. Pure over its inputs so it can be exercised with planted spec data, which is what
/// proves it can go red (a rule tested only against the real, passing corpus tests nothing).
pub(crate) fn validate_metric_emitters(
    declared: &[DeclaredMetric],
    contract_rs: &str,
    meters_rs: &str,
) -> Vec<Issue> {
    let mut issues = Vec::new();
    for (feature, kind, name) in declared {
        let at = format!("observability.yaml/{feature}.{kind}");
        match const_named(contract_rs, name) {
            None => issues.push(warn(
                "obs-metric-no-emitter",
                at,
                format!(
                    "metric '{name}' is declared but no `pub const` in {CONTRACT_RS} carries that \
                     name — nothing in crates/** can spell it, so the contract is a promise with \
                     no runtime. Add the constant and build its instrument in {METERS_RS}, or drop \
                     the declaration."
                ),
            )),
            Some(ident) if !instrument_built_from(meters_rs, &ident) => issues.push(warn(
                "obs-metric-no-emitter",
                at,
                format!(
                    "metric '{name}' has the constant `{ident}` in {CONTRACT_RS} but no instrument \
                     is built from it in {METERS_RS} — a metric name in a header file is not a \
                     metric. Build the instrument and emit it from the seam that owns the fact."
                ),
            )),
            Some(_) => {}
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(name: &str) -> Vec<DeclaredMetric> {
        vec![("place-order".to_string(), "metrics".to_string(), name.to_string())]
    }

    fn render(issues: &[Issue]) -> String {
        issues
            .iter()
            .map(|i| format!("  {} {}: {}", i.rule, i.location, i.message))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The #608 defect itself, reduced: declared, and no constant anywhere.
    #[test]
    fn a_declared_metric_with_no_constant_is_reported() {
        let issues = validate_metric_emitters(&declared("payment_authorized_unsettled_age_seconds"), "", "");
        assert_eq!(issues.len(), 1, "{}", render(&issues));
        assert_eq!(issues[0].rule, "obs-metric-no-emitter");
        assert!(issues[0].message.contains("no `pub const`"), "{}", issues[0].message);
    }

    /// The half-done shape: the name exists as a constant, but nothing builds an instrument from
    /// it. A rule that stopped at the constant would call this emitted.
    #[test]
    fn a_constant_with_no_instrument_is_reported() {
        let issues = validate_metric_emitters(
            &declared("orders_placed_total"),
            "pub const ORDERS_PLACED_TOTAL: &str = \"orders_placed_total\";",
            "// no instrument here",
        );
        assert_eq!(issues.len(), 1, "{}", render(&issues));
        assert!(issues[0].message.contains("no instrument is built"), "{}", issues[0].message);
    }

    /// The passing shape, including rustfmt's two-line form for a long name — which is exactly the
    /// form #608's own constants take, so a scan that only understood one line would report the
    /// metric this rule was written for as missing.
    #[test]
    fn a_constant_wrapped_onto_two_lines_with_an_instrument_passes() {
        let issues = validate_metric_emitters(
            &declared("payment_authorized_no_order_birth_age_seconds"),
            "pub const PAYMENT_AUTHORIZED_NO_ORDER_BIRTH_AGE_SECONDS: &str =\n    \
             \"payment_authorized_no_order_birth_age_seconds\";",
            "meter().i64_gauge(metric::PAYMENT_AUTHORIZED_NO_ORDER_BIRTH_AGE_SECONDS).build()",
        );
        assert!(issues.is_empty(), "{}", render(&issues));
    }

    /// A PREFIX reference must not satisfy the lookup: `metric::A_B` in meters.rs is not an
    /// instrument for the constant `A_B_TOTAL`.
    #[test]
    fn a_prefix_reference_does_not_count_as_an_instrument() {
        let issues = validate_metric_emitters(
            &declared("a_b_total"),
            "pub const A_B_TOTAL: &str = \"a_b_total\";",
            "meter().u64_counter(metric::A_B).build()",
        );
        assert_eq!(issues.len(), 1, "{}", render(&issues));
        assert!(issues[0].message.contains("no instrument is built"), "{}", issues[0].message);
    }
}
