//! #639 part C step 6-ii round 2, R2-E (ADR-20260905-101349 §9): the per-role GraphQL
//! depth/complexity ceiling — the `graphql-limits` extension, exercised against the REAL master
//! schema (`server::graphql_schema::build_schema`), the `graphql_acl.rs` precedent
//! (`schema.execute(Request::new(query).data(acting_role))`, no HTTP layer needed: the extension
//! is registered on the schema itself, so it fires identically either way).
//!
//! The boundary document for each role is DERIVED from `QueryLimits::effective_max_depth` — the
//! POST-HEADROOM value `parse_query` actually enforces (`GRAPHQL_LIMIT_HEADROOM_PERCENT` on top of
//! the raw generated constant; deriving from the raw constant alone would build a document the
//! runtime happily admits under headroom, silently passing for the wrong reason) — never a
//! hand-spelled number (ADR-20260817-105845): a chain of the REAL, schema-declared
//! `Restaurant.orders -> Order.restaurant -> …` FK-navigation cycle, nested one level deeper than
//! the role's ceiling. Because the field names are REAL (not fabricated), a document the limiter
//! does NOT refuse reaches the `restaurants` resolver, which — this schema carries no read deps
//! (`build_schema(None, None, None)`) — fails with "Data `...RestaurantReadRepository` does not
//! exist", a DISTINCT and unmistakable witness that resolution actually started. A refused
//! document never carries that message: the M3 shape (a call-count witness), built from the
//! real error surface rather than a custom counting mock.

use async_graphql::Request;
use server::graphql_acl::RequestRole;
use server::graphql_generated_limits::GRAPHQL_ROLES;
use server::graphql_schema::build_schema;
use server::QueryLimits;

/// `{ restaurants(input: {}) { orders { restaurant { orders { restaurant { … } } } } } }` nested
/// so the DOCUMENT's total depth (the `restaurants` root field itself counts as depth 1) is
/// exactly `target_depth`.
fn cycle_document(target_depth: usize) -> String {
    // `target_depth` counts the root `restaurants` field as depth 1 and the leaf `id` as its own
    // depth level too — so with `levels` alternating orders/restaurant wrappers in between, the
    // chain is restaurants(1) -> W1(2) -> … -> W(levels)(levels+1) -> id(levels+2), i.e.
    // `target_depth == levels + 2`.
    let levels = target_depth.saturating_sub(2);
    // Field NAMES in root-to-leaf order: position 0 (the immediate child of `restaurants`) must
    // be `orders` (`Restaurant.orders` — the parent type here is `Restaurant`), position 1
    // `restaurant` (`Order.restaurant`), alternating. Wrapping inside-out from this list (rather
    // than assigning names by wrap-order) keeps the alternation's PARITY tied to depth-from-root,
    // not to `levels`' own parity — the earlier version put `restaurant` directly under
    // `restaurants` whenever `levels` was even, which is not a field `Restaurant` has.
    let names: Vec<&str> =
        (0..levels).map(|i| if i % 2 == 0 { "orders" } else { "restaurant" }).collect();
    let mut inner = "id".to_string();
    for field in names.into_iter().rev() {
        inner = format!("{field} {{ {inner} }}");
    }
    format!("query {{ restaurants(input: {{}}) {{ {inner} }} }}")
}

fn bound(role: RequestRole) -> server::Principal {
    server::Principal::role_binding(
        role,
        "limits-test-subject".to_string(),
        Some(uuid::Uuid::from_u128(0x639_2E)),
    )
}

async fn execute_as(role: RequestRole, query: &str) -> async_graphql::Response {
    let schema = build_schema(None, None, None);
    schema.execute(Request::new(query).data(bound(role).acting_role(role))).await
}

fn reason_of(resp: &async_graphql::Response) -> Option<String> {
    resp.errors.iter().find_map(|e| {
        e.extensions
            .as_ref()
            .and_then(|ext| ext.get("reason"))
            .map(|v| v.to_string().trim_matches('"').to_string())
    })
}

fn reached_a_resolver(resp: &async_graphql::Response) -> bool {
    resp.errors.iter().any(|e| e.message.contains("does not exist"))
}

/// (1)/(3): an over-deep document is refused on EVERY role's schema — including `/restaurant`
/// (RESTAURANT), the exact mutant round 1's M4 caught escaping ("limits applied to /public only").
#[tokio::test]
async fn every_role_refuses_a_document_one_level_past_its_own_depth_ceiling_before_any_resolver_runs() {
    for role in [
        RequestRole::Public,
        RequestRole::Customer,
        RequestRole::RestaurantAccount,
        RequestRole::Restaurant,
        RequestRole::Rider,
        RequestRole::Admin,
        RequestRole::External,
    ] {
        let ceiling = QueryLimits::from_env().effective_max_depth(role.api_name());
        let query = cycle_document(ceiling + 1);
        let resp = execute_as(role, &query).await;
        assert_eq!(
            reason_of(&resp).as_deref(),
            Some("QUERY_TOO_DEEP"),
            "{role:?} (effective ceiling {ceiling}) should have refused: {:?}",
            resp.errors
        );
        assert!(
            !reached_a_resolver(&resp),
            "{role:?}: a refused document must never reach a resolver — got {:?}",
            resp.errors
        );
    }
}

/// (2): the document AT the ceiling (never past it) passes the limiter for every role — proven by
/// reaching the resolver (the missing-dependency error), never our own refusal.
#[tokio::test]
async fn every_role_admits_a_document_exactly_at_its_own_depth_ceiling() {
    for role in [
        RequestRole::Public,
        RequestRole::Customer,
        RequestRole::RestaurantAccount,
        RequestRole::Restaurant,
        RequestRole::Rider,
        RequestRole::Admin,
        RequestRole::External,
    ] {
        let ceiling = QueryLimits::from_env().effective_max_depth(role.api_name());
        let query = cycle_document(ceiling);
        let resp = execute_as(role, &query).await;
        assert_eq!(
            reason_of(&resp),
            None,
            "{role:?} (effective ceiling {ceiling}) should NOT have been refused by the limiter: {:?}",
            resp.errors
        );
        assert!(
            reached_a_resolver(&resp),
            "{role:?}: an at-ceiling document must reach the resolver (proving the limiter passed \
             it through) — got {:?}",
            resp.errors
        );
    }
}

/// A document with N aliased copies of a scalar leaf — complexity-heavy, depth-shallow (depth 2
/// regardless of N): `{ restaurants(input: {}) { id a0: id a1: id … } }`, total complexity
/// `n_aliases + 2` (the root field, the bare `id`, and each alias — an ALIAS is still its own
/// field node to this limiter, exactly as async-graphql's own default rule counts it).
fn wide_document(n_aliases: usize) -> String {
    let aliases: String = (0..n_aliases).map(|i| format!(" a{i}: id")).collect();
    format!("query {{ restaurants(input: {{}}) {{ id{aliases} }} }}")
}

/// Proves the ceiling table is genuinely PER-ROLE, not a single shared number (the mutant this
/// pins: "limits applied to /public only", or any table collapse that would make every role agree)
/// — PUBLIC and RESTAURANT have DIFFERENT emitted complexity ceilings (verified: this test would
/// be vacuous, and is asserted, if they ever coincided), so a document sized strictly between them
/// refuses for PUBLIC and passes for RESTAURANT under the IDENTICAL document text.
#[tokio::test]
async fn the_complexity_ceiling_genuinely_differs_by_role() {
    let limits = QueryLimits::from_env();
    let public_ceiling = limits.effective_max_complexity(RequestRole::Public.api_name());
    let restaurant_ceiling = limits.effective_max_complexity(RequestRole::Restaurant.api_name());
    assert_ne!(
        public_ceiling, restaurant_ceiling,
        "this test is vacuous unless PUBLIC and RESTAURANT carry different ceilings \
         (re-derive the midpoint below if the generated table ever converges them)"
    );
    let (lower, higher, lower_role, higher_role) = if public_ceiling < restaurant_ceiling {
        (public_ceiling, restaurant_ceiling, RequestRole::Public, RequestRole::Restaurant)
    } else {
        (restaurant_ceiling, public_ceiling, RequestRole::Restaurant, RequestRole::Public)
    };
    let midpoint_complexity = lower + (higher - lower) / 2 + 1; // strictly above `lower`, at or below `higher`
    let query = wide_document(midpoint_complexity.saturating_sub(2));

    let refused = execute_as(lower_role, &query).await;
    assert_eq!(
        reason_of(&refused).as_deref(),
        Some("QUERY_TOO_COMPLEX"),
        "{lower_role:?} (ceiling {lower}) should refuse a complexity-{midpoint_complexity} document: {:?}",
        refused.errors
    );

    let admitted = execute_as(higher_role, &query).await;
    assert_eq!(
        reason_of(&admitted),
        None,
        "{higher_role:?} (ceiling {higher}) should admit the SAME document {lower_role:?} refused: {:?}",
        admitted.errors
    );
    assert!(reached_a_resolver(&admitted), "the admitted document must reach the resolver: {:?}", admitted.errors);
}

/// Every role this table names actually gets exercised above — if a role were added to
/// `GRAPHQL_ROLES` without a matching arm here, this fails loudly rather than silently under-
/// covering the new role.
#[test]
fn every_generated_role_is_covered_by_this_suite() {
    assert_eq!(
        GRAPHQL_ROLES,
        ["PUBLIC", "CUSTOMER", "RESTAURANT_ACCOUNT", "RESTAURANT", "RIDER", "ADMIN", "EXTERNAL"],
        "a new role in the generated table needs a matching arm in this test file's role lists"
    );
}
