//! Per-role GraphQL ACL enforcement (ADR-0006 "role = path"), spec-derived from api.yaml `roles`.
//! Executes against the schema directly with a `RequestRole` in the request context (what
//! `/{role}/graphql` injects from the URL path) — no DB needed (`build_schema(None, None, None)`):
//! - EXECUTION: a role calling an operation outside its api.yaml `roles` gets a FORBIDDEN error
//!   (extension `code`) and the resolver never runs; an authorized role reaches the resolver.
//! - INTROSPECTION: a role only sees its authorized fields, and (via async-graphql's
//!   `find_visible_types`) only the types reachable through them — this is what per-role Voyager renders.
//! - PUBLIC operations (api.yaml `roles` include PUBLIC) are open to every role, including the
//!   unauthenticated PUBLIC path; a request context without a role fails closed to PUBLIC.

use async_graphql::Request;
use serde_json::Value;
use server::graphql_acl::RequestRole;
use server::graphql_schema::{build_schema, CaptainSchema};

fn schema() -> CaptainSchema {
    // No read/write deps (nor event bus): ACL runs before resolvers, and introspection needs none.
    build_schema(None, None, None)
}

/// Execute `query` under `role` (mirrors routes.rs' `request.data(role)`).
async fn execute_as(schema: &CaptainSchema, role: RequestRole, query: &str) -> async_graphql::Response {
    schema.execute(Request::new(query).data(role)).await
}

/// True when the error is the RoleGuard rejection (extension `code: FORBIDDEN`).
fn is_forbidden(err: &async_graphql::ServerError) -> bool {
    serde_json::to_value(err)
        .ok()
        .and_then(|v| v.get("extensions").and_then(|e| e.get("code")).cloned())
        == Some(serde_json::json!("FORBIDDEN"))
}

/// The Query/Mutation field names this role's introspection exposes.
async fn introspected_fields(schema: &CaptainSchema, role: RequestRole) -> (Vec<String>, Vec<String>) {
    let resp = execute_as(
        schema,
        role,
        "{ __schema { queryType { fields { name } } mutationType { fields { name } } } }",
    )
    .await;
    assert!(resp.errors.is_empty(), "introspection errored: {:?}", resp.errors);
    let data = resp.data.into_json().expect("introspection json");
    let names = |v: &Value| -> Vec<String> {
        v["fields"]
            .as_array()
            .expect("fields array")
            .iter()
            .map(|f| f["name"].as_str().expect("field name").to_string())
            .collect()
    };
    (names(&data["__schema"]["queryType"]), names(&data["__schema"]["mutationType"]))
}

/// Whether this role's introspection resolves `__type(name:)` (types reachable only through hidden
/// fields are hidden too — async-graphql's `find_visible_types`).
async fn type_visible(schema: &CaptainSchema, role: RequestRole, ty: &str) -> bool {
    let resp =
        execute_as(schema, role, &format!("{{ __type(name: \"{ty}\") {{ name }} }}")).await;
    assert!(resp.errors.is_empty(), "__type errored: {:?}", resp.errors);
    !resp.data.into_json().expect("__type json")["__type"].is_null()
}

/// Introspection is role-filtered: PUBLIC does not see @auth-only operations (`prospectionPipeline` is
/// [ADMIN], `registerRestaurant` is [ADMIN, RESTAURANT_ACCOUNT]) nor the types reachable only through
/// them; ADMIN sees them; RESTAURANT sees neither (not in either roles list). Public operations show
/// for everyone.
#[tokio::test]
async fn introspection_is_filtered_per_role() {
    let schema = schema();

    let (public_q, public_m) = introspected_fields(&schema, RequestRole::Public).await;
    assert!(public_q.contains(&"restaurants".into()), "public query missing: {public_q:?}");
    assert!(!public_q.contains(&"prospectionPipeline".into()), "admin-only query leaked to PUBLIC");
    assert!(!public_q.contains(&"pricingPolicy".into()), "admin-only query leaked to PUBLIC");
    assert!(public_m.contains(&"verifyPhone".into()), "public mutation missing: {public_m:?}");
    assert!(!public_m.contains(&"registerRestaurant".into()), "@auth mutation leaked to PUBLIC");

    let (admin_q, admin_m) = introspected_fields(&schema, RequestRole::Admin).await;
    assert!(admin_q.contains(&"prospectionPipeline".into()), "ADMIN query missing: {admin_q:?}");
    assert!(admin_q.contains(&"restaurants".into()), "public query missing under ADMIN");
    assert!(admin_m.contains(&"registerRestaurant".into()), "ADMIN mutation missing: {admin_m:?}");

    let (rest_q, rest_m) = introspected_fields(&schema, RequestRole::Restaurant).await;
    assert!(!rest_q.contains(&"prospectionPipeline".into()), "admin-only query leaked to RESTAURANT");
    assert!(!rest_m.contains(&"registerRestaurant".into()), "mutation leaked to RESTAURANT");
    assert!(rest_q.contains(&"orders".into()), "RESTAURANT query missing: {rest_q:?}");

    // Type visibility follows field visibility: PricingPolicy is reachable only via admin-only
    // queries, RegisterRestaurantInput only via registerRestaurant. (The mutation RETURN type is the
    // shared MutationAcceptance, reachable from every mutation — visible to all roles.)
    for ty in ["PricingPolicy", "RegisterRestaurantInput"] {
        assert!(!type_visible(&schema, RequestRole::Public, ty).await, "{ty} leaked to PUBLIC");
        assert!(!type_visible(&schema, RequestRole::Restaurant, ty).await, "{ty} leaked to RESTAURANT");
        assert!(type_visible(&schema, RequestRole::Admin, ty).await, "{ty} missing under ADMIN");
    }
    assert!(type_visible(&schema, RequestRole::Public, "Restaurant").await, "public type hidden");
    assert!(
        type_visible(&schema, RequestRole::Public, "MutationAcceptance").await,
        "the shared acceptance payload must be visible to every role (acceptance-first)"
    );
}

/// Executing an operation outside the role's api.yaml `roles` is rejected by the guard (FORBIDDEN)
/// before the resolver runs; an authorized role passes the guard and reaches the resolver.
#[tokio::test]
async fn unauthorized_execution_is_forbidden() {
    let schema = schema();
    let admin_query = "{ prospectionPipeline { score } }"; // [ADMIN]

    // PUBLIC → the guard rejects; the (wired) resolver never runs, so the only error is FORBIDDEN.
    let resp = execute_as(&schema, RequestRole::Public, admin_query).await;
    assert_eq!(resp.errors.len(), 1, "expected one error: {:?}", resp.errors);
    assert!(is_forbidden(&resp.errors[0]), "expected FORBIDDEN: {:?}", resp.errors[0]);
    // No role in the context at all (direct execution) fails closed to PUBLIC too.
    let resp = schema.execute(admin_query).await;
    assert!(is_forbidden(&resp.errors[0]), "missing role must fail closed: {:?}", resp.errors);

    // ADMIN → the guard passes; with no deps injected the resolver itself errors (missing repo),
    // which proves execution reached it — and it is NOT the FORBIDDEN rejection.
    let resp = execute_as(&schema, RequestRole::Admin, admin_query).await;
    assert_eq!(resp.errors.len(), 1, "expected the resolver error: {:?}", resp.errors);
    assert!(!is_forbidden(&resp.errors[0]), "guard must pass for ADMIN: {:?}", resp.errors[0]);

    // Same for a mutation: registerRestaurant is [ADMIN, RESTAURANT_ACCOUNT].
    let mutation = r#"mutation {
        registerRestaurant(input: {
            restaurantId: "00000000-0000-0000-0000-000000000001",
            slug: "chez-marco",
            displayName: "Chez Marco",
            address: { line1: "1 Rue Nationale", postalCode: "37000", city: "Tours", country: "FR" }
        }) { correlationId }
    }"#;
    for role in [RequestRole::Public, RequestRole::Restaurant, RequestRole::Rider] {
        let resp = execute_as(&schema, role, mutation).await;
        assert_eq!(resp.errors.len(), 1, "expected one error for {role:?}: {:?}", resp.errors);
        assert!(is_forbidden(&resp.errors[0]), "expected FORBIDDEN for {role:?}: {:?}", resp.errors[0]);
    }
    let resp = execute_as(&schema, RequestRole::RestaurantAccount, mutation).await;
    assert!(!is_forbidden(&resp.errors[0]), "guard must pass for RESTAURANT_ACCOUNT: {:?}", resp.errors);
}

/// Operations with `roles:` OMITTED (literal roles, ADR-20260720-191500) run under the
/// unauthenticated PUBLIC role — and under every other role.
#[tokio::test]
async fn public_operations_are_open_to_all_roles() {
    let schema = schema();
    // phoneCountries has roles omitted (open) and is unwired: reaching its `not implemented` stub proves the ACL let
    // the resolver run (no DB in this test, so wired resolvers can't fully succeed).
    for role in [RequestRole::Public, RequestRole::Customer, RequestRole::Admin] {
        let resp = execute_as(&schema, role, "{ phoneCountries { dialingCode } }").await;
        assert_eq!(resp.errors.len(), 1, "expected the stub error for {role:?}: {:?}", resp.errors);
        assert!(!is_forbidden(&resp.errors[0]), "public op forbidden for {role:?}");
        assert_eq!(resp.errors[0].message, "not implemented", "resolver did not run for {role:?}");
    }
    // restaurants (roles omitted, wired) passes the ACL under PUBLIC: the only failure without deps is the
    // missing repository, never FORBIDDEN.
    let resp = execute_as(&schema, RequestRole::Public, "{ restaurants { slug } }").await;
    assert!(!resp.errors.is_empty() && !is_forbidden(&resp.errors[0]), "restaurants blocked: {:?}", resp.errors);
}

/// LITERAL roles lists (ADR-20260720-191500, #31): PUBLIC in a `roles:` list is just the anonymous
/// path — the list admits exactly the listed paths. `paymentStatus` is [PUBLIC, CUSTOMER, ADMIN]:
/// open on those three paths, FORBIDDEN + hidden on any other; `verifyPhone` is [PUBLIC, CUSTOMER].
#[tokio::test]
async fn literal_roles_lists_admit_only_listed_paths() {
    let schema = schema();

    // Execution: the three listed paths pass the guard (the resolver then errors on the missing
    // PM store — proof it ran); RESTAURANT and RIDER are rejected by the guard itself.
    let query = r#"{ paymentStatus(input: { orderId: "3f6d3c9a-8f04-4f7e-9f0e-3a1b2c4d5e6f" }) { status } }"#;
    for role in [RequestRole::Public, RequestRole::Customer, RequestRole::Admin] {
        let resp = execute_as(&schema, role, query).await;
        assert!(!resp.errors.is_empty(), "expected the missing-dep error for {role:?}");
        assert!(!is_forbidden(&resp.errors[0]), "listed path {role:?} must pass the guard: {:?}", resp.errors[0]);
    }
    for role in [RequestRole::Restaurant, RequestRole::Rider, RequestRole::External] {
        let resp = execute_as(&schema, role, query).await;
        assert_eq!(resp.errors.len(), 1, "expected one error for {role:?}: {:?}", resp.errors);
        assert!(is_forbidden(&resp.errors[0]), "unlisted path {role:?} must be FORBIDDEN: {:?}", resp.errors[0]);
    }

    // Introspection follows: listed paths see the field, unlisted paths don't.
    for role in [RequestRole::Public, RequestRole::Customer, RequestRole::Admin] {
        let (q, _m) = introspected_fields(&schema, role).await;
        assert!(q.contains(&"paymentStatus".into()), "paymentStatus missing under {role:?}: {q:?}");
    }
    let (rest_q, rest_m) = introspected_fields(&schema, RequestRole::Restaurant).await;
    assert!(!rest_q.contains(&"paymentStatus".into()), "paymentStatus leaked to RESTAURANT");
    assert!(!rest_m.contains(&"verifyPhone".into()), "verifyPhone ([PUBLIC, CUSTOMER]) leaked to RESTAURANT");
    let (_rider_q, rider_m) = introspected_fields(&schema, RequestRole::Rider).await;
    assert!(!rider_m.contains(&"verifyPhone".into()), "verifyPhone ([PUBLIC, CUSTOMER]) leaked to RIDER");
}

/// FK-derived navigation edges with `navRoles` (#22, ADR-20260720-230000): the guarded edges off
/// the PUBLIC-reachable Restaurant are hidden from unlisted roles' introspection and visible to
/// listed ones; unguarded edges (catalogs) stay open to everyone.
#[tokio::test]
async fn guarded_nav_edges_are_hidden_from_unlisted_roles() {
    let schema = schema();
    let type_fields = |role: RequestRole| {
        let schema = schema.clone();
        async move {
        let resp = execute_as(
            &schema,
            role,
            r#"{ __type(name: "Restaurant") { fields { name } } }"#,
        )
        .await;
        assert!(resp.errors.is_empty(), "introspection errored: {:?}", resp.errors);
        let data = resp.data.into_json().expect("json");
        data["__type"]["fields"]
            .as_array()
            .expect("fields")
            .iter()
            .map(|f| f["name"].as_str().expect("name").to_string())
            .collect::<Vec<_>>()
        }
    };

    let public = type_fields(RequestRole::Public).await;
    assert!(public.contains(&"catalogs".into()), "open edge missing for PUBLIC: {public:?}");
    assert!(!public.contains(&"carts".into()), "carts ([ADMIN]) leaked to PUBLIC");
    assert!(!public.contains(&"orders".into()), "orders leaked to PUBLIC");
    assert!(!public.contains(&"deliveryJobs".into()), "deliveryJobs leaked to PUBLIC");

    let restaurant = type_fields(RequestRole::Restaurant).await;
    assert!(restaurant.contains(&"orders".into()), "orders missing for RESTAURANT: {restaurant:?}");
    assert!(restaurant.contains(&"deliveryJobs".into()), "deliveryJobs missing for RESTAURANT");
    assert!(!restaurant.contains(&"carts".into()), "carts ([ADMIN]) leaked to RESTAURANT");

    let admin = type_fields(RequestRole::Admin).await;
    for f in ["catalogs", "carts", "orders", "deliveryJobs"] {
        assert!(admin.contains(&f.to_string()), "{f} missing for ADMIN: {admin:?}");
    }
}
