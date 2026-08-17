//! §21 — a `technical_error` rule that CANNOT FIRE (#623/#624 part 1).
//!
//! The defect this rule exists for, measured on `place-order`: the contract classifies a run as
//! `technical_error` via `status_rules.technical_error.any_span_errors: true`, and **not one span
//! on that workflow can carry an error status**. `tracing` cannot `record` a field that was not
//! declared at construction, so `payment.intent.create` — the riskiest leg of checkout, the
//! outbound Stripe call — declared `business.result` and nothing else. The rule was structurally
//! unreachable. Every gateway failure exported as a plain, successful CLIENT span.
//!
//! That is worse than a missing classification, because it reads as working: the contract declares
//! the class, dashboards offer it, and it is permanently empty. The neighbouring failure mode is the
//! one ADR-20260810-231300 names — a monitor that can only fire when a signal arrives goes quiet
//! exactly when it should scream.
//!
//! **The two links, and why neither alone is enough.** This repo spells a span exactly once, as a
//! constructor in `crates/telemetry/src/spans.rs` (verified: no other `crates/**` file calls
//! `info_span!`). So:
//!
//! 1. at least one span the contract declares must be CONSTRUCTED with `otel.status_code`, and
//! 2. some function in the same file must SET it to `"ERROR"`, and be named after that
//!    constructor — `record_cart_price_error` for `cart_price`, `record_claims_stamp_result` for
//!    `claims_stamp`, the two precedents that already pass.
//!
//! A declared-but-never-set field is a span that reports success on every failure; a recorder with
//! no declared field records SILENTLY into nothing, which is the same outcome with a different
//! cause. Link 2 is per-constructor rather than file-global on purpose: a global "somebody, somewhere
//! sets ERROR" is satisfied by `cart-price` alone and would pass every contract in the repo.
//!
//! **Scope: EMITTED contracts only.** A contract none of whose spans is constructed at all is a
//! different and larger defect (the contract has no runtime), and reporting it here would bury this
//! one. Those are out of scope by design, not by oversight.
//!
//! **WARNING, not error, for the §17 reason.** A large existing population fails this today —
//! contracts written before their runtimes, plus every contract whose only constructed span is the
//! shared `event.store.append`. Errors would make `main` red and the fast way out would be to weaken
//! the rule. As a warning it joins the per-rule ratchet: the population is frozen, ONE MORE is a hard
//! gate failure, and each contract that gains an error status tightens the baseline. The count is
//! deliberately not written here — it lives in `tools/codegen-rs/warning-baseline.json`.

use std::path::Path;

use crate::model::{warn, Issue};

/// Where spans are constructed: the ONE place a span name is spelled in Rust.
const SPANS_RS: &str = "crates/telemetry/src/spans.rs";

/// One contract that classifies technical errors by span status: `(feature, declared span names)`.
pub(crate) type SpanErrorContract = (String, Vec<String>);

/// Every contract whose `status_rules.technical_error` is `any_span_errors: true`, with the span
/// names it declares, in a stable order.
pub(crate) fn contracts_classifying_by_span_error(model: &crate::Model) -> Vec<SpanErrorContract> {
    let mut out: Vec<SpanErrorContract> = Vec::new();
    let Some(obs) = model.defs.get("observability.yaml").and_then(|x| x.as_mapping()) else {
        return out;
    };
    for (fk, contract) in obs {
        let Some(feature) = fk.as_str() else { continue };
        let classifies = contract
            .get("status_rules")
            .and_then(|r| r.get("technical_error"))
            .and_then(|t| t.get("any_span_errors"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !classifies {
            continue;
        }
        let spans: Vec<String> = contract
            .get("spans")
            .and_then(|x| x.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|s| s.get("name").and_then(|n| n.as_str()).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        out.push((feature.to_string(), spans));
    }
    out.sort();
    out.dedup();
    out
}

/// Read the one Rust file the rule inspects. A missing file yields an empty string, which makes
/// every contract read as UNEMITTED and report nothing — deliberately the quiet direction here,
/// because the loud one (`§20`'s posture) would fire on every contract for a reason that has
/// nothing to do with any of them.
pub(crate) fn load_span_source(repo_root: &Path) -> String {
    std::fs::read_to_string(repo_root.join(SPANS_RS)).unwrap_or_default()
}

/// One `fn` block of `spans.rs`: its identifier and its body text.
struct FnBlock<'a> {
    ident: &'a str,
    text: &'a str,
}

/// Split `spans.rs` into its top-level functions. A hand-rolled scan for the same reason §20 uses
/// one: the shape is fixed by rustfmt and by this file's own convention, and a parser dependency
/// would be a much larger commitment than the check is worth.
fn fn_blocks(spans_rs: &str) -> Vec<FnBlock<'_>> {
    let mut out = Vec::new();
    let starts: Vec<usize> = spans_rs
        .match_indices("\npub fn ")
        .chain(spans_rs.match_indices("\nfn "))
        .map(|(i, _)| i + 1)
        .collect();
    let mut starts = starts;
    starts.sort_unstable();
    for (n, &start) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(spans_rs.len());
        // TRUNCATE AT THE FUNCTION'S OWN CLOSING BRACE. Without this the block runs on into the
        // NEXT function's doc comment, and this file's doc comments talk at length about
        // `otel.status_code` — `cart_read` was reported as declaring the field because
        // `cart_price`'s doc comment explains why IT declares it. A rule that reads prose as
        // instrumentation is worse than no rule: it certifies exactly the contracts whose
        // documentation is most conscientious.
        let raw = &spans_rs[start..end];
        let text = match raw.rfind("\n}") {
            Some(brace) => &raw[..brace + 2],
            None => raw,
        };
        let Some(after_fn) = text.find("fn ") else { continue };
        let rest = &text[after_fn + 3..];
        let ident_end = rest.find(['(', '<', ' ']).unwrap_or(0);
        if ident_end == 0 {
            continue;
        }
        out.push(FnBlock { ident: &rest[..ident_end], text });
    }
    out
}

/// The constructor identifier for each span NAME, plus whether it declares `otel.status_code`.
fn constructors(spans_rs: &str) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    for block in fn_blocks(spans_rs) {
        let Some(macro_at) = block.text.find("_span!(") else { continue };
        let after = &block.text[macro_at + "_span!(".len()..];
        let Some(open) = after.find('"') else { continue };
        let rest = &after[open + 1..];
        let Some(close) = rest.find('"') else { continue };
        let span_name = &rest[..close];
        // The name must look like a span name, not some other leading string literal.
        if !span_name.contains('.') {
            continue;
        }
        out.push((
            span_name.to_string(),
            block.ident.to_string(),
            block.text.contains("otel.status_code"),
        ));
    }
    out
}

/// Every function that SETS `otel.status_code` to `"ERROR"`, by identifier.
fn error_recorders(spans_rs: &str) -> Vec<String> {
    fn_blocks(spans_rs)
        .into_iter()
        .filter(|b| {
            // Tolerate rustfmt's line break between the two arguments.
            let compact: String = b.text.split_whitespace().collect::<Vec<_>>().join(" ");
            compact.contains(r#""otel.status_code", "ERROR""#)
        })
        .map(|b| b.ident.to_string())
        .collect()
}

/// The rule. Pure over its inputs, so it can be exercised with planted data — a rule tested only
/// against the real, passing corpus tests nothing.
pub(crate) fn validate_span_error_status(
    contracts: &[SpanErrorContract],
    spans_rs: &str,
) -> Vec<Issue> {
    let ctors = constructors(spans_rs);
    let recorders = error_recorders(spans_rs);
    let mut issues = Vec::new();
    for (feature, spans) in contracts {
        let declared: Vec<&(String, String, bool)> =
            ctors.iter().filter(|(name, _, _)| spans.contains(name)).collect();
        // An UNEMITTED contract (no span of it is constructed anywhere) is a different defect.
        if declared.is_empty() {
            continue;
        }
        let at = format!("observability.yaml/{feature}.status_rules.technical_error");
        let with_field: Vec<&&(String, String, bool)> =
            declared.iter().filter(|(_, _, has)| *has).collect();
        if with_field.is_empty() {
            issues.push(warn(
                "obs-technical-error-unreachable",
                at,
                format!(
                    "'{feature}' classifies runs as technical_error via `any_span_errors`, but not \
                     one of its constructed spans declares `otel.status_code` in {SPANS_RS} \
                     ({}). `tracing` cannot record a field that was not declared at construction, \
                     so the rule can NEVER fire and every failed run exports as a plain success. \
                     Declare `otel.status_code = Empty` on the span that owns the failure and add a \
                     recorder that sets it to \"ERROR\".",
                    declared
                        .iter()
                        .map(|(n, _, _)| n.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
            continue;
        }
        let recorded = with_field
            .iter()
            .any(|(_, ident, _)| recorders.iter().any(|r| r.contains(ident.as_str())));
        if !recorded {
            issues.push(warn(
                "obs-technical-error-unreachable",
                at,
                format!(
                    "'{feature}' declares `otel.status_code` on {} but nothing in {SPANS_RS} sets \
                     it to \"ERROR\" for that span — a declared-but-never-set field reports success \
                     on every failure, which is the same empty dashboard with a different cause. \
                     Add a recorder named after the constructor (the `record_cart_price_error` / \
                     `cart_price` precedent).",
                    with_field
                        .iter()
                        .map(|(n, _, _)| n.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract(spans: &[&str]) -> Vec<SpanErrorContract> {
        vec![("place-order".to_string(), spans.iter().map(|s| s.to_string()).collect())]
    }

    fn render(issues: &[Issue]) -> String {
        issues
            .iter()
            .map(|i| format!("  {} {}: {}", i.rule, i.location, i.message))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The #623 defect itself, reduced: the span exists, and it can never carry an error status.
    #[test]
    fn a_span_without_the_status_field_makes_the_rule_unreachable() {
        let spans_rs = r#"
pub fn payment_intent_create() -> Span {
    tracing::info_span!("payment.intent.create", otel.kind = "client", business.result = Empty,)
}
"#;
        let issues = validate_span_error_status(&contract(&["payment.intent.create"]), spans_rs);
        assert_eq!(issues.len(), 1, "{}", render(&issues));
        assert_eq!(issues[0].rule, "obs-technical-error-unreachable");
        assert!(issues[0].message.contains("can NEVER fire"), "{}", issues[0].message);
    }

    /// The half-done shape: the field is declared and nothing ever sets it. A rule that stopped at
    /// the declaration would call this instrumented.
    #[test]
    fn a_declared_field_with_no_recorder_is_reported() {
        let spans_rs = r#"
pub fn payment_intent_create() -> Span {
    tracing::info_span!("payment.intent.create", otel.kind = "client", otel.status_code = Empty,)
}
pub fn record_payment_result(span: &Span, result: &str) {
    span.record(attr::RESULT, result);
}
"#;
        let issues = validate_span_error_status(&contract(&["payment.intent.create"]), spans_rs);
        assert_eq!(issues.len(), 1, "{}", render(&issues));
        assert!(issues[0].message.contains("never-set"), "{}", issues[0].message);
    }

    /// The passing shape, including rustfmt's line break between `record`'s two arguments — the
    /// form the real file takes, so a scan that only understood one line would report a span that
    /// is correctly instrumented.
    #[test]
    fn a_declared_field_with_a_matching_recorder_passes() {
        let spans_rs = r#"
pub fn payment_intent_create() -> Span {
    tracing::info_span!("payment.intent.create", otel.kind = "client", otel.status_code = Empty,)
}
pub fn record_payment_intent_create_error(span: &Span) {
    span.record(
        "otel.status_code",
        "ERROR",
    );
}
"#;
        let issues = validate_span_error_status(&contract(&["payment.intent.create"]), spans_rs);
        assert!(issues.is_empty(), "{}", render(&issues));
    }

    /// Link 2 is PER-CONSTRUCTOR. A recorder for someone else's span must not satisfy this
    /// contract — the file-global reading would pass every contract in the repo on `cart-price`'s
    /// instrumentation alone.
    #[test]
    fn a_recorder_for_another_span_does_not_count() {
        let spans_rs = r#"
pub fn payment_intent_create() -> Span {
    tracing::info_span!("payment.intent.create", otel.kind = "client", otel.status_code = Empty,)
}
pub fn cart_price(aggregate_id: &str) -> Span {
    tracing::info_span!("cart.price", otel.kind = "internal", otel.status_code = Empty,)
}
pub fn record_cart_price_error(span: &Span) {
    span.record("otel.status_code", "ERROR");
}
"#;
        let issues = validate_span_error_status(&contract(&["payment.intent.create"]), spans_rs);
        assert_eq!(issues.len(), 1, "{}", render(&issues));
        assert!(issues[0].message.contains("never-set"), "{}", issues[0].message);
    }

    /// THE BUG THIS RULE SHIPPED WITH FOR ONE RUN, kept as a test because it is the failure mode a
    /// text scan has and a parser does not: a fn block that runs on into the NEXT function's doc
    /// comment reads that prose as instrumentation. Here `payment_intent_create` declares nothing
    /// and the following doc comment explains `otel.status_code` at length — exactly the real
    /// file's shape, which is how the first run of this rule certified `cart.read`.
    #[test]
    fn a_neighbours_doc_comment_is_not_instrumentation() {
        let spans_rs = r#"
pub fn payment_intent_create() -> Span {
    tracing::info_span!("payment.intent.create", otel.kind = "client", business.result = Empty,)
}

/// `cart.price` (INTERNAL). `otel.status_code` is late-bound and DECLARED here because the
/// contract classifies an unresolvable price as technical_error via any_span_errors.
pub fn cart_price(aggregate_id: &str) -> Span {
    tracing::info_span!("cart.price", otel.kind = "internal", otel.status_code = Empty,)
}
"#;
        let issues = validate_span_error_status(&contract(&["payment.intent.create"]), spans_rs);
        assert_eq!(issues.len(), 1, "{}", render(&issues));
        assert!(issues[0].message.contains("can NEVER fire"), "{}", issues[0].message);
    }

    /// A contract none of whose spans is constructed is a DIFFERENT defect and is out of scope:
    /// reporting it here would bury the one this rule is for.
    #[test]
    fn an_unemitted_contract_is_out_of_scope() {
        let spans_rs = r#"
pub fn cart_price(aggregate_id: &str) -> Span {
    tracing::info_span!("cart.price", otel.kind = "internal",)
}
"#;
        let issues = validate_span_error_status(&contract(&["webhook.verify"]), spans_rs);
        assert!(issues.is_empty(), "{}", render(&issues));
    }
}
