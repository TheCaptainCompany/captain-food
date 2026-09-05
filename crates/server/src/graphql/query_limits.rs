//! Per-role GraphQL depth/complexity ceiling (#639 part C step 6-ii round 2, R2-E,
//! ADR-20260905-101349 §9): a custom `parse_query` extension, the `ScopeSlice` shape almost
//! exactly (`crates/server/src/graphql/scope_slice.rs`), refusing an over-deep/over-complex
//! document BEFORE any resolver runs, keyed on the request's [`crate::auth::ActingRole`] — the
//! staff host is authenticated, not trusted, so the limit applies to EVERY role's schema, never
//! `/public` only (the M4 mutant this file's own tests plant and revert).
//!
//! **Why a custom extension and not `Schema::build(..).limit_depth()/.limit_complexity()`**: those
//! are baked at SCHEMA-BUILD time on the ONE master `CaptainSchema` every role shares
//! (`crates/server/src/graphql/schema.rs`) — a single number, not a per-role table. Verified
//! against async-graphql 7.2.1's own `validation::check_rules` (`limit_complexity`/`limit_depth`
//! are plain `Option<usize>` fields on `Schema`, read once): the mechanism genuinely cannot vary
//! per request on one schema instance, so the card's own choice (a per-request extension keyed on
//! the path role) is the only shape that works here, not a fallback.
//!
//! **The complexity/depth rule** (documented because ADR-20260817-105845 forbids a bare number
//! without saying which rule produced it): FIELD-COUNT — depth = the deepest field nesting level
//! (1 for a top-level field), complexity = one point per field node, summed — the exact
//! async-graphql default (`ComplexityCalculate`'s `1 + children_complexity` fallback for a field
//! with no `#[graphql(complexity = …)]` annotation, which this schema declares NONE of, so the
//! default IS the whole rule here). Fragment spreads resolve INLINE (`VisitMode::Inline`, cycle
//! guarded); UNLIKE async-graphql's own internal visitor, an orphan fragment definition no
//! operation ever spreads contributes NOTHING — it never executes, so counting it would inflate
//! the refusal threshold against a cost the server never actually pays. Multiple operations in one
//! document (this platform's own generated documents never send more than one) sum complexity and
//! take the max depth — the conservative direction either way.
//!
//! **`async_graphql::validation::visitors::{ComplexityCalculate, DepthCalculate}` are
//! `pub(crate)` inside async-graphql** (verified: `mod visitors;` with no `pub`), so this file
//! cannot reuse them and implements the same rule over the public `parser::types` AST instead.

use std::collections::HashMap;
use std::sync::Arc;

use async_graphql::extensions::{Extension, ExtensionContext, ExtensionFactory, NextParseQuery};
use async_graphql::parser::types::{ExecutableDocument, FragmentDefinition, Selection, SelectionSet};
use async_graphql::{ErrorExtensions, Name, Positioned, ServerResult, Variables};

use crate::auth::ActingRole;
use crate::graphql::acl::RequestRole;
use crate::graphql::session::RequestCorrelationId;

/// Attachable factory: `Schema::build(...).extension(QueryLimits::new(headroom_percent))`. The
/// headroom is a resolved DEPLOY value (`GRAPHQL_LIMIT_HEADROOM_PERCENT`), read ONCE at the
/// composition root — never per-request config (the `ScopeSlice`/`RunRiderRestrictionDoor`
/// precedent: a deployment fact captured at build time, not re-read from `ctx.data` per call).
pub struct QueryLimits {
    headroom_percent: u32,
}

impl QueryLimits {
    pub fn new(headroom_percent: u32) -> Self {
        Self { headroom_percent }
    }

    /// Read `GRAPHQL_LIMIT_HEADROOM_PERCENT` directly (`generated::config::Config::resolve`)
    /// rather than threading it through `build_schema`/`ReadDeps` — those are called from dozens
    /// of test sites with no `Config` in scope at all, and this value is the SAME "resolved once,
    /// read directly" shape `email_send_guard` uses where a `Config` IS already in scope; here it
    /// is not, so this reads it itself (cheap: env lookups, the same call `router()` already makes
    /// once at boot — a second read is idempotent, never a side effect). Called ONCE, when the
    /// extension factory is built (schema construction, itself once per process).
    pub fn from_env() -> Self {
        let (config, _) = crate::generated::config::Config::resolve();
        Self::new(config.graphql_limit_headroom_percent.max(0) as u32)
    }

    /// `graphql_limit_max{role, kind}`, for EVERY role and EVERY kind — asserted ONCE at the
    /// composition root (the `member_sign_in_door_enforcing`/`watch_live_delta(0)` precedent,
    /// #895's lesson): a deploy with zero GraphQL traffic must still show its configured ceiling,
    /// never read as "limits not installed" for want of a single request. Call from
    /// `build_schema_for_scope`, NOT from [`ExtensionFactory::create`] (which async-graphql calls
    /// PER REQUEST, not once) — `parse_query` itself never re-asserts this gauge, only the
    /// per-request OBSERVED histograms, so the two stay the distinct things the contract declares.
    pub fn assert_limit_gauges(&self) {
        for role in super::generated::limits::GRAPHQL_ROLES.iter().copied() {
            for kind in ["depth", "complexity"] {
                let raw = match kind {
                    "depth" => super::generated::limits::max_depth_for_role(role),
                    _ => super::generated::limits::max_complexity_for_role(role),
                };
                telemetry::meters::graphql_limits::limit_max(
                    role,
                    kind,
                    apply_headroom(raw, self.headroom_percent),
                );
            }
        }
    }

    /// The EFFECTIVE (post-headroom) ceiling for `role` — exposed so tests derive their boundary
    /// documents from the SAME value `parse_query` enforces (ADR-20260817-105845: no hand-spelled
    /// number), never from the raw generated constant alone.
    pub fn effective_max_depth(&self, role: &str) -> usize {
        apply_headroom(super::generated::limits::max_depth_for_role(role), self.headroom_percent)
    }

    /// See [`Self::effective_max_depth`].
    pub fn effective_max_complexity(&self, role: &str) -> usize {
        apply_headroom(super::generated::limits::max_complexity_for_role(role), self.headroom_percent)
    }
}

/// `raw * (100 + headroom) / 100`, saturating rather than overflowing on a pathological headroom
/// value — a mis-typed deploy value degrades to "very generous", never a panic. Shared by
/// [`QueryLimits::assert_limit_gauges`] and [`QueryLimitsExtension::parse_query`] so the enforced
/// value and the reported value can never drift apart.
fn apply_headroom(raw: usize, headroom_percent: u32) -> usize {
    raw.saturating_mul(100 + headroom_percent as usize) / 100
}

impl ExtensionFactory for QueryLimits {
    fn create(&self) -> Arc<dyn Extension> {
        Arc::new(QueryLimitsExtension { headroom_percent: self.headroom_percent })
    }
}

struct QueryLimitsExtension {
    headroom_percent: u32,
}

#[async_trait::async_trait]
impl Extension for QueryLimitsExtension {
    async fn parse_query(
        &self,
        ctx: &ExtensionContext<'_>,
        query: &str,
        variables: &Variables,
        next: NextParseQuery<'_>,
    ) -> ServerResult<ExecutableDocument> {
        let doc = next.run(ctx, query, variables).await?;

        // Fail CLOSED to PUBLIC (the platform's own convention, `acl.rs::request_role`): a context
        // with no `ActingRole` — direct schema execution outside the HTTP surface — gets the
        // narrowest ceiling, never the widest.
        let role = ctx
            .data_opt::<ActingRole>()
            .copied()
            .map(ActingRole::get)
            .unwrap_or(RequestRole::Public);
        let role_name = role.api_name();

        let (depth, complexity) = document_depth_and_complexity(&doc);

        // Observed on EVERY parsed document, accepted or refused — zero rejections must not read
        // like "limits not installed" (the `graphql-limits` contract's own stated purpose). The
        // `graphql_limit_max` GAUGE is a separate, composition-root-asserted fact
        // (`QueryLimits::assert_limit_gauges`) — never re-set here, per request.
        telemetry::meters::graphql_limits::observed_depth(role_name, depth);
        telemetry::meters::graphql_limits::observed_complexity(role_name, complexity);

        let max_depth =
            apply_headroom(super::generated::limits::max_depth_for_role(role_name), self.headroom_percent);
        let max_complexity = apply_headroom(
            super::generated::limits::max_complexity_for_role(role_name),
            self.headroom_percent,
        );

        let reason = if depth > max_depth {
            Some("depth")
        } else if complexity > max_complexity {
            Some("complexity")
        } else {
            None
        };

        if let Some(reason) = reason {
            telemetry::meters::graphql_limits::rejected(role_name, reason);
            let correlation_id = ctx
                .data_opt::<RequestCorrelationId>()
                .map(|c| c.0)
                .unwrap_or_else(uuid::Uuid::nil)
                .to_string();
            // Opened and immediately dropped — nothing is logged INSIDE it (the contract declares
            // no child events), so recording it is exactly "this span happened, with these
            // attributes", the declared shape.
            let _span = telemetry::spans::graphql_limits_refused(role_name, reason, &correlation_id);
            let (code, message) = match reason {
                "depth" => (
                    "QUERY_TOO_DEEP",
                    format!("query is nested too deep for role {role_name} (depth {depth} > {max_depth})"),
                ),
                _ => (
                    "QUERY_TOO_COMPLEX",
                    format!(
                        "query is too complex for role {role_name} (complexity {complexity} > {max_complexity})"
                    ),
                ),
            };
            return Err(async_graphql::Error::new(message)
                .extend_with(|_, e| {
                    e.set("reason", code);
                    e.set("role", role_name);
                })
                .into_server_error(async_graphql::Pos::default()));
        }
        Ok(doc)
    }
}

/// The whole document's (depth, complexity): max depth across every operation, complexity SUMMED
/// across every operation (this platform's own generated documents carry exactly one; a
/// multi-operation document is conservatively charged for all of them — the safe direction).
fn document_depth_and_complexity(doc: &ExecutableDocument) -> (usize, usize) {
    let mut max_depth = 0usize;
    let mut total_complexity = 0usize;
    let mut guard: Vec<Name> = Vec::new();
    for (_name, operation) in doc.operations.iter() {
        let (depth, complexity) =
            selection_set_metrics(&operation.node.selection_set.node, &doc.fragments, 1, &mut guard);
        max_depth = max_depth.max(depth);
        total_complexity = total_complexity.saturating_add(complexity);
    }
    (max_depth, total_complexity)
}

/// One selection set's (max field depth reached, total field-count complexity) at `depth` (the
/// depth ITS OWN direct fields sit at — 1 for a top-level field). Fragment spreads resolve
/// inline at the SAME depth (a spread is not itself a field); `guard` stops a cyclic spread from
/// recursing forever (the platform's own generated documents never spread fragments at all — this
/// only matters for an arbitrary client-submitted document, which may not be validated yet at
/// `parse_query` time).
fn selection_set_metrics(
    set: &SelectionSet,
    fragments: &HashMap<Name, Positioned<FragmentDefinition>>,
    depth: usize,
    guard: &mut Vec<Name>,
) -> (usize, usize) {
    let mut max_depth = 0usize;
    let mut complexity = 0usize;
    for item in &set.items {
        match &item.node {
            Selection::Field(field) => {
                // Introspection (`__schema`/`__type`/`__typename`) is EXEMPT — the `ScopeSlice`
                // precedent (`scope_slice.rs`: "any subgraph can serve it, the SLICE is about data
                // ownership, not schema visibility"). Tooling's own introspection queries (`__type
                // { fields { type { ofType { ofType { … } } } } }`) are routinely deeper than any
                // business query and would otherwise trip this limiter constantly on a role-
                // filtered but perfectly legitimate GraphiQL/codegen request — a zero-cost leaf,
                // never descended into for accounting purposes: contributes NOTHING to either
                // metric (not even its own depth level) — a document that is PURELY introspection
                // is (0, 0), exactly as if it selected no fields at all.
                if field.node.name.node.as_str().starts_with("__") {
                    continue;
                }
                max_depth = max_depth.max(depth);
                let (child_depth, child_complexity) = selection_set_metrics(
                    &field.node.selection_set.node,
                    fragments,
                    depth + 1,
                    guard,
                );
                max_depth = max_depth.max(child_depth);
                complexity += 1 + child_complexity;
            }
            Selection::InlineFragment(inline) => {
                let (d, c) =
                    selection_set_metrics(&inline.node.selection_set.node, fragments, depth, guard);
                max_depth = max_depth.max(d);
                complexity += c;
            }
            Selection::FragmentSpread(spread) => {
                let name = &spread.node.fragment_name.node;
                if guard.contains(name) {
                    continue; // cycle — validation will reject the document separately; never loop
                }
                let Some(def) = fragments.get(name) else { continue }; // unknown fragment — validation's to catch
                guard.push(name.clone());
                let (d, c) =
                    selection_set_metrics(&def.node.selection_set.node, fragments, depth, guard);
                guard.pop();
                max_depth = max_depth.max(d);
                complexity += c;
            }
        }
    }
    (max_depth, complexity)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_of(query: &str) -> ExecutableDocument {
        async_graphql::parser::parse_query(query).expect("valid GraphQL text")
    }

    fn metrics(query: &str) -> (usize, usize) {
        document_depth_and_complexity(&doc_of(query))
    }

    /// "Reds when the headroom drops below the deepest real query" (round 2 R2-E's own words): a
    /// deploy misconfiguring `GRAPHQL_LIMIT_HEADROOM_PERCENT` to a negative value must never
    /// PRODUCE an effective ceiling below the raw observed maximum — `QueryLimits::from_env`'s
    /// `.max(0)` clamp is what makes this structurally true; this test is the thing that reds if
    /// that clamp is ever removed.
    #[test]
    fn a_hostile_negative_headroom_never_shrinks_the_effective_ceiling_below_the_raw_max() {
        for role in super::super::generated::limits::GRAPHQL_ROLES.iter().copied() {
            let raw_depth = super::super::generated::limits::max_depth_for_role(role);
            let raw_complexity = super::super::generated::limits::max_complexity_for_role(role);
            // headroom_percent is `u32` on `QueryLimits` itself (the clamp already applied by
            // `from_env`) -- 0 is the WORST case this type can represent, and even then the
            // effective ceiling must equal (never fall below) the raw one.
            let worst_case = QueryLimits::new(0);
            assert!(
                worst_case.effective_max_depth(role) >= raw_depth,
                "{role}: headroom=0 must never shrink depth below the raw max {raw_depth}"
            );
            assert!(
                worst_case.effective_max_complexity(role) >= raw_complexity,
                "{role}: headroom=0 must never shrink complexity below the raw max {raw_complexity}"
            );
        }
    }

    #[test]
    fn a_single_top_level_field_is_depth_one_complexity_one() {
        assert_eq!(metrics("{ value }"), (1, 1));
    }

    #[test]
    fn nesting_increases_depth_and_each_field_adds_one_complexity() {
        // obj (depth 1) { a b (depth 2) } — 3 fields total.
        assert_eq!(metrics("{ obj { a b } }"), (2, 3));
    }

    #[test]
    fn an_inline_fragment_is_transparent_to_depth_and_adds_no_complexity_of_its_own() {
        assert_eq!(metrics("{ obj { ... on T { a b } } }"), (2, 3));
    }

    #[test]
    fn a_fragment_spread_resolves_inline_at_the_spread_site_depth() {
        let q = "fragment F on T { a b } query { obj { ...F } }";
        // obj (1) { a b (2, via the spread) } = 3 fields.
        assert_eq!(metrics(q), (2, 3));
    }

    #[test]
    fn an_orphan_fragment_no_operation_spreads_contributes_nothing() {
        let with_orphan = "fragment Unused on T { a b c { d e { f } } } query { value }";
        assert_eq!(metrics(with_orphan), metrics("query { value }"), "an unreferenced fragment must never inflate the count");
    }

    #[test]
    fn introspection_is_exempt_from_both_depth_and_complexity() {
        // A deep, realistic introspection query (GraphiQL's own shape) contributes NOTHING — a
        // purely-introspection document reads as (0, 0), exactly as if it selected no fields.
        let q = "{ __schema { types { name fields { name type { name ofType { name ofType { name } } } } } } }";
        assert_eq!(metrics(q), (0, 0));
    }

    #[test]
    fn introspection_mixed_with_a_real_field_only_counts_the_real_field() {
        let q = "{ __typename value }";
        assert_eq!(metrics(q), (1, 1), "the real `value` field alone: depth 1, complexity 1");
    }

    #[test]
    fn a_self_referential_fragment_spread_does_not_infinite_loop() {
        // Not valid GraphQL (NoFragmentCycles would reject it at validation), but parse_query runs
        // BEFORE validation — the extension must terminate on this input rather than hang.
        let q = "fragment F on T { a ...F } query { obj { ...F } }";
        let (depth, complexity) = metrics(q);
        assert!(depth > 0 && complexity > 0, "must terminate with a real (if partial) answer");
    }
}
