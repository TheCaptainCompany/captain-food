//! §22 — every tenant-scoped QUERY is either bound by the emitter's read-scope table or on an
//! explicit exempt list with a MECHANICAL reason (#618, DECISIONS §39 IDOR-1).
//!
//! **The defect this rule exists for is a copy that was not made.** The `ReadScope` prelude was
//! hand-copied into each bound resolver arm: `myReclamations` and `customerCredit` carried verbatim
//! copies of the same lines, comment and all, while `restaurantReclamations` — three arms away in
//! the same `match` — had none, with a comment conceding the narrowing was a follow-up gap. Nothing
//! in the build could tell the difference between "deliberately unscoped" and "the copy was
//! forgotten", because both look like an arm with no prelude. `pendingRefunds` was the same
//! omission on the money path, and it is what §39 IDOR-1 is about.
//!
//! **What the rule checks.** For every `api.yaml` query whose `roles` include a tenant-scoped role
//! (`CUSTOMER` · `RESTAURANT` · `RESTAURANT_ACCOUNT` · `RIDER`), exactly one of:
//!
//! 1. [`read_scope_binding`] returns a binding — the emitter emits its prelude, or
//! 2. the query is in [`READ_SCOPE_BINDING_EXEMPT`] with an admissible reason.
//!
//! No drift is possible between the gate and the generator: this module calls the emitter's own
//! table, in the same binary. There is no second copy to fall out of step.
//!
//! **The exempt reason is not prose, and that is the whole point.** It is a `file:line` pointing at
//! where the scoping actually happens, or the literal token `UNSCOPED — #618`. A sentence explaining
//! why a surface is fine is the easiest thing in a diff to write and the hardest to check; a
//! `file:line` a reader can open is neither. So the rule additionally verifies that the citation
//! RESOLVES: the file exists, the line exists, and the line actually mentions the operation or an
//! identity/scope token. A pointer into unrelated code is a stale pointer, and a stale pointer is
//! prose again.
//!
//! **ERROR, not warning.** The exempt list is complete as of this rule's introduction, so nothing
//! is grandfathered and there is no ratchet to keep: a NEW tenant-scoped query simply cannot be
//! added without someone deciding, in the diff, which of the two cases it is. That decision is
//! cheap to make and expensive to discover later.
//!
//! **The `UNSCOPED — #618` rows are #618's authoritative enumeration.** The count of remaining
//! unscoped read surfaces is DERIVED from this list by anyone who wants it, rather than measured
//! once into a record and left to rot — which is exactly what #618 itself asks for ("the gate's
//! output is the authoritative list, which is why this issue carries none").

use std::path::Path;

use crate::emit::server_graphql::{read_scope_binding, READ_SCOPE_BINDING_EXEMPT, UNSCOPED_TOKEN};
use crate::model::{err, Issue};

/// The roles whose callers own a subset of the data rather than all of it. A query any of them can
/// reach must have an answer to "which caller's rows?".
///
/// `ADMIN`, `PUBLIC`, `EXTERNAL` and `SYSTEM` are deliberately absent: an admin arbitrates across
/// tenants by design, and the other three hold no tenant membership to be bound to.
pub(crate) const TENANT_SCOPED_ROLES: [&str; 4] =
    ["CUSTOMER", "RESTAURANT", "RESTAURANT_ACCOUNT", "RIDER"];

/// One declared query: `(name, roles)`.
pub(crate) type DeclaredQuery = (String, Vec<String>);

/// Every query declared across the per-scope `api.yaml` fragments, with its `roles`.
pub(crate) fn declared_queries(model: &crate::Model) -> Vec<DeclaredQuery> {
    let mut out: Vec<DeclaredQuery> = Vec::new();
    let Some(queries) = model.defs.get("api.yaml").and_then(|a| a.get("queries")).and_then(|q| q.as_mapping())
    else {
        return out;
    };
    for (k, v) in queries {
        let Some(name) = k.as_str() else { continue };
        let roles: Vec<String> = v
            .get("roles")
            .and_then(|r| r.as_sequence())
            .map(|s| s.iter().filter_map(|r| r.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        out.push((name.to_string(), roles));
    }
    out.sort();
    out
}

/// Whether a `roles` list reaches any tenant-scoped caller.
fn is_tenant_scoped(roles: &[String]) -> bool {
    roles.iter().any(|r| TENANT_SCOPED_ROLES.contains(&r.as_str()))
}

/// Tokens that make a cited line plausibly the scoping site. A citation landing on a line with none
/// of these — and not naming the operation either — has drifted onto unrelated code.
const SCOPE_TOKENS: [&str; 6] =
    ["scope", "Scope", "principal", "Principal", "auth_ref", "rider_id"];

/// Does the exempt reason RESOLVE — file present, line present, and the line actually about this
/// operation's scoping? Returns the concrete complaint, or `None` when the citation is good.
fn citation_problem(repo_root: &Path, op: &str, reason: &str) -> Option<String> {
    let Some((file, line_no)) = reason.rsplit_once(':') else {
        return Some(format!(
            "reason {reason:?} is neither a `file:line` nor the literal `{UNSCOPED_TOKEN}`; prose \
             is not admissible here"
        ));
    };
    let Ok(line_no) = line_no.parse::<usize>() else {
        return Some(format!(
            "reason {reason:?} is neither a `file:line` nor the literal `{UNSCOPED_TOKEN}`; prose \
             is not admissible here"
        ));
    };
    if line_no == 0 {
        return Some(format!("reason {reason:?} cites line 0"));
    }
    let Ok(text) = std::fs::read_to_string(repo_root.join(file)) else {
        return Some(format!("reason {reason:?} cites a file that does not exist"));
    };
    let Some(line) = text.lines().nth(line_no - 1) else {
        return Some(format!(
            "reason {reason:?} cites line {line_no}, past the end of a {}-line file",
            text.lines().count()
        ));
    };
    if line.contains(op) || SCOPE_TOKENS.iter().any(|t| line.contains(t)) {
        return None;
    }
    Some(format!(
        "reason {reason:?} points at a line that mentions neither `{op}` nor any scope token \
         ({}) -- the citation has drifted onto unrelated code: {}",
        SCOPE_TOKENS.join(" | "),
        line.trim()
    ))
}

/// The rule. Pure over its inputs (bar the citation file reads, which is what a `file:line` means)
/// so it can be exercised with planted query data — a rule tested only against the real, passing
/// corpus tests nothing.
pub(crate) fn validate_read_scope_binding(
    queries: &[DeclaredQuery],
    repo_root: &Path,
) -> Vec<Issue> {
    let mut issues = Vec::new();
    let exempt_reason = |op: &str| {
        READ_SCOPE_BINDING_EXEMPT.iter().find(|(n, _)| *n == op).map(|(_, reason)| *reason)
    };

    for (name, roles) in queries {
        let at = format!("api.yaml/queries/{name}");
        let bound = read_scope_binding(name).is_some();
        let exempt = exempt_reason(name);
        if !is_tenant_scoped(roles) {
            // A query no tenant-scoped role can reach has nothing to bind to. Being in either
            // structure anyway is a stale entry, not a harmless extra.
            if bound || exempt.is_some() {
                issues.push(err(
                    "read-scope-binding-not-tenant-scoped",
                    at,
                    format!(
                        "'{name}' declares roles {roles:?}, none tenant-scoped, yet it is listed \
                         in the read-scope binding table or its exempt list -- a stale entry \
                         outlives the reason it was written for"
                    ),
                ));
            }
            continue;
        }
        match (bound, exempt) {
            (true, Some(_)) => issues.push(err(
                "read-scope-binding-both",
                at,
                format!(
                    "'{name}' is BOTH bound by read_scope_binding() and on the exempt list -- one \
                     of the two is a leftover, and whichever it is, the list no longer says what \
                     is true"
                ),
            )),
            (true, None) => {}
            (false, Some(reason)) => {
                if reason != UNSCOPED_TOKEN {
                    if let Some(problem) = citation_problem(repo_root, name, reason) {
                        issues.push(err(
                            "read-scope-binding-exempt-reason",
                            at,
                            format!("'{name}': {problem}"),
                        ));
                    }
                }
            }
            (false, None) => issues.push(err(
                "read-scope-missing",
                at,
                format!(
                    "'{name}' is reachable by a tenant-scoped role ({}) but is neither bound by \
                     the emitter's read_scope_binding() table nor on its exempt list. Decide here, \
                     in this diff: bind it, or exempt it with a `file:line` pointing at where the \
                     scoping actually happens -- or with the literal `{UNSCOPED_TOKEN}` if it is \
                     genuinely unscoped. An arm with no prelude cannot tell 'deliberate' from \
                     'forgotten', which is how #618 happened.",
                    roles
                        .iter()
                        .filter(|r| TENANT_SCOPED_ROLES.contains(&r.as_str()))
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )),
        }
    }

    // A row naming an operation that no longer exists is a list that has stopped describing the
    // system — the same defect as a missing row, one direction over.
    let declared: Vec<&str> = queries.iter().map(|(n, _)| n.as_str()).collect();
    for (name, _) in READ_SCOPE_BINDING_EXEMPT {
        if !declared.contains(name) {
            issues.push(err(
                "read-scope-binding-stale-exempt",
                "api.yaml/queries".to_string(),
                format!("the read-scope exempt list names '{name}', which no query declares"),
            ));
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Issue` carries no `Debug`, so assertions read the rule names.
    fn rules(issues: &[Issue]) -> Vec<&str> {
        issues.iter().map(|i| i.rule).collect()
    }

    /// The rules raised for `name` ALONE. The staleness sweep necessarily fires for every exempt row
    /// a partial corpus omits, which is correct against the real model and pure noise in a unit
    /// test — so each test below reads only the findings about the query it planted.
    fn about<'a>(issues: &'a [Issue], name: &str) -> Vec<&'a str> {
        issues
            .iter()
            .filter(|i| i.location == format!("api.yaml/queries/{name}"))
            .map(|i| i.rule)
            .collect()
    }

    fn q(name: &str, roles: &[&str]) -> DeclaredQuery {
        (name.to_string(), roles.iter().map(|r| r.to_string()).collect())
    }

    fn repo_root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap()
    }

    /// The rule's reason for existing: a NEW tenant-scoped query that nobody decided about is an
    /// ERROR. Without it, an arm with no prelude is indistinguishable from an arm that forgot one —
    /// which is exactly how `pendingRefunds` shipped unscoped on the money path.
    #[test]
    fn an_undecided_tenant_scoped_query_is_an_error() {
        let issues =
            validate_read_scope_binding(&[q("brandNewRestaurantQueue", &["RESTAURANT"])], repo_root());
        assert_eq!(about(&issues, "brandNewRestaurantQueue"), vec!["read-scope-missing"]);
    }

    /// An ADMIN-only query is nobody's tenant slice, so it needs no decision — and the rule must not
    /// force one, or it becomes noise that gets suppressed.
    #[test]
    fn an_admin_only_query_needs_no_decision() {
        let issues = validate_read_scope_binding(&[q("pricingPolicy", &["ADMIN"])], repo_root());
        assert_eq!(about(&issues, "pricingPolicy"), Vec::<&str>::new());
    }

    /// The three bound operations raise nothing: this is the rule running against the tree it ships
    /// in, on the arms this chunk actually decided.
    #[test]
    fn the_bound_operations_are_decided() {
        let queries = vec![
            q("pendingRefunds", &["RESTAURANT", "ADMIN"]),
            q("myReclamations", &["CUSTOMER"]),
            q("customerCredit", &["CUSTOMER"]),
        ];
        let issues = validate_read_scope_binding(&queries, repo_root());
        for name in ["pendingRefunds", "myReclamations", "customerCredit"] {
            assert_eq!(about(&issues, name), Vec::<&str>::new(), "{name} must need no exemption");
        }
    }

    /// EVERY `file:line` in the shipped exempt list resolves against the real tree — file present,
    /// line present, and the line actually about that operation's scoping. This is the assertion
    /// that keeps the list mechanical: a citation nobody can follow is prose again.
    #[test]
    fn every_shipped_citation_resolves_against_the_tree() {
        let bad: Vec<String> = READ_SCOPE_BINDING_EXEMPT
            .iter()
            .filter(|(_, reason)| *reason != UNSCOPED_TOKEN)
            .filter_map(|(op, reason)| {
                citation_problem(repo_root(), op, reason).map(|p| format!("{op}: {p}"))
            })
            .collect();
        assert_eq!(bad, Vec::<String>::new());
    }

    /// Prose is refused where a `file:line` is required, and so is a citation past the end of the
    /// file. Without this the exempt list degrades into a comment column and the gate becomes a
    /// formality.
    #[test]
    fn a_prose_reason_is_refused() {
        assert!(citation_problem(repo_root(), "somewhere", "this one is fine, trust me").is_some());
        assert!(citation_problem(repo_root(), "somewhere", "Cargo.toml:99999").is_some());
        assert!(citation_problem(repo_root(), "somewhere", "no/such/file.rs:1").is_some());
    }

    /// A stale row — one naming an operation no query declares — is reported, because a list that
    /// has stopped describing the system is not evidence of anything.
    #[test]
    fn a_stale_exempt_row_is_reported() {
        let issues = validate_read_scope_binding(&[q("pendingRefunds", &["RESTAURANT"])], repo_root());
        assert!(
            !issues.is_empty() && issues.iter().all(|i| i.rule == "read-scope-binding-stale-exempt"),
            "every exempt row is stale against a one-query corpus; got {:?}",
            rules(&issues)
        );
    }
}
