//! Per-role GraphQL ACL enforcement (ADR-0006 "role = path"), spec-derived from api.yaml `roles`.
//! Executes against the schema directly with an `ActingRole` in the request context (what
//! `/{role}/graphql` injects) — no DB needed (`build_schema(None, None, None)`):
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

/// A verified principal BOUND to `role`, the way `AuthContext::authorize` builds one from a token
/// carrying both a role and its domain claim. Roles that carry no domain binding by design (ADMIN,
/// EXTERNAL, PUBLIC) ignore the uuid, exactly as `Principal::role_path` does.
fn bound(role: RequestRole) -> server::Principal {
    server::Principal::role_binding(
        role,
        "acl-test-subject".to_string(),
        Some(uuid::Uuid::from_u128(0x639)),
    )
}

/// Execute `query` as a caller BOUND to `role` (mirrors what `routes.rs` injects).
///
/// The context carries an `ActingRole`, and there is no way to fabricate one (#639 part B) — this
/// helper has to go through a `Principal`, which is the point: every case in this file now asserts
/// something about a caller who actually holds the binding for the role they are exercising, rather
/// than about a bare enum somebody typed. `unbound_denied_on_the_money_path` below is the same
/// helper with the binding taken away.
async fn execute_as(schema: &CaptainSchema, role: RequestRole, query: &str) -> async_graphql::Response {
    schema.execute(Request::new(query).data(bound(role).acting_role(role))).await
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
    // `restaurants` has roles omitted (open) and is wired. With no DB in this test it cannot succeed,
    // so the assertion is the ACL one: whatever error comes back, it is never FORBIDDEN — the resolver
    // ran. (This used to also exercise `phoneCountries`, the one query with no wired body, whose
    // `not implemented` stub proved the same thing more directly. It was deleted with #305; every
    // remaining query is wired, so the stub branch has no subject left.)
    for role in [RequestRole::Public, RequestRole::Customer, RequestRole::Admin] {
        let resp = execute_as(&schema, role, "{ restaurants { slug } }").await;
        assert_eq!(resp.errors.len(), 1, "expected the missing-dependency error for {role:?}: {:?}", resp.errors);
        assert!(!is_forbidden(&resp.errors[0]), "public op forbidden for {role:?}: {:?}", resp.errors);
        // NAME the expected failure. `!is_forbidden` alone would also pass on `Unknown field
        // "restaurants"`, so a deleted or renamed query would leave this test green while proving
        // nothing — asserting the resolver reached its missing repo is what proves the body RAN.
        assert!(
            resp.errors[0].message.contains("RestaurantReadRepository"),
            "resolver body did not run for {role:?}: {:?}",
            resp.errors[0].message
        );
    }
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

/// GDPR erasure ops are CUSTOMER-ONLY, on every axis (#708, PROP-20260829-150752 §3.1).
///
/// This is the highest-consequence roles list in the schema, and it is the one the rest of the
/// suite does not reach: the other tests pin operations whose leak would expose an order or a
/// payment, while these four expose WHETHER A NAMED PERSON IS DELETING THEMSELVES. That fact is
/// sensitive even when the answer is "no request exists", and `erasureStatus` takes NO ARGUMENTS
/// precisely so that the subject can only ever be the caller — a design that is only as good as the
/// guard in front of it, because the moment another role can call it, "no args" stops meaning "only
/// me" and starts meaning "whoever the server thinks you are".
///
/// Three containments, because a role-as-path leak has three independent doors and shutting one
/// proves nothing about the others:
///   1. EXECUTION — every non-CUSTOMER path is FORBIDDEN by the guard, and the resolver never runs.
///   2. INTROSPECTION — no other role's schema even NAMES these operations. A hidden-but-callable
///      field and a visible-but-guarded field are both wrong, and only introspection catches the
///      second: the mere presence of `requestErasure` in an ADMIN-facing schema advertises a
///      capability we do not intend to offer through that path.
///   3. TYPE REACHABILITY — `CustomerErasure` must not be reachable for anyone else. A type that
///      survives while its fields are hidden leaks the SHAPE of the thing (status enum members,
///      timestamps), which is how the existence of the journey gets inferred without calling it.
///
/// ADMIN is asserted alongside the others ON PURPOSE and is not an oversight: an admin-side erasure
/// console is a plausible future ask, and if it is ever built it must arrive as a DECIDED change to
/// the roles list with its own audit trail, not by quietly widening this one. This test is the
/// thing that makes that a deliberate act.
#[tokio::test]
async fn erasure_operations_are_customer_only_on_every_axis() {
    let schema = schema();
    const MUTATIONS: [&str; 3] = ["requestErasure", "confirmErasure", "cancelErasure"];
    const OTHER_ROLES: [RequestRole; 5] = [
        RequestRole::Public,
        RequestRole::Restaurant,
        RequestRole::RestaurantAccount,
        RequestRole::Rider,
        RequestRole::Admin,
    ];

    // 1. EXECUTION. The mutations carry required inputs, so a bare selection would fail on ARGUMENT
    // VALIDATION before the guard and prove nothing; each query below is fully formed, which is why
    // a non-FORBIDDEN error is a genuine "the guard let it through" signal.
    let calls = [
        (r#"mutation { requestErasure(input: { customerId: "3f6d3c9a-8f04-4f7e-9f0e-3a1b2c4d5e6f", erasureRequestId: "5c2e1a7b-9d43-4c81-bf20-6e8a0d7c1f39" }) { operationStatus } }"#, "requestErasure"),
        (r#"mutation { confirmErasure(input: { customerId: "3f6d3c9a-8f04-4f7e-9f0e-3a1b2c4d5e6f", erasureRequestId: "5c2e1a7b-9d43-4c81-bf20-6e8a0d7c1f39", token: "tok" }) { operationStatus } }"#, "confirmErasure"),
        (r#"mutation { cancelErasure(input: { customerId: "3f6d3c9a-8f04-4f7e-9f0e-3a1b2c4d5e6f", erasureRequestId: "5c2e1a7b-9d43-4c81-bf20-6e8a0d7c1f39" }) { operationStatus } }"#, "cancelErasure"),
        (r#"{ erasureStatus { status } }"#, "erasureStatus"),
    ];
    for (query, name) in calls {
        for role in OTHER_ROLES {
            let resp = execute_as(&schema, role, query).await;
            assert!(!resp.errors.is_empty(), "{name} must not succeed for {role:?}");
            assert!(
                is_forbidden(&resp.errors[0]),
                "{name} must be FORBIDDEN for {role:?}, got: {:?}",
                resp.errors[0]
            );
        }
        // CUSTOMER passes the guard. It still errors — the handlers refuse with the typed
        // ErasureEngineUnavailable while the journey is unbuilt — and that the error is NOT
        // FORBIDDEN is exactly the proof that the guard admitted the call and the resolver ran.
        let resp = execute_as(&schema, RequestRole::Customer, query).await;
        assert!(!resp.errors.is_empty(), "{name}: expected the typed refusal for CUSTOMER");
        assert!(
            !is_forbidden(&resp.errors[0]),
            "{name} must pass the guard for CUSTOMER: {:?}",
            resp.errors[0]
        );
    }

    // 2. INTROSPECTION.
    let (cust_q, cust_m) = introspected_fields(&schema, RequestRole::Customer).await;
    assert!(cust_q.contains(&"erasureStatus".into()), "erasureStatus missing for CUSTOMER: {cust_q:?}");
    for m in MUTATIONS {
        assert!(cust_m.contains(&m.to_string()), "{m} missing for CUSTOMER: {cust_m:?}");
    }
    for role in OTHER_ROLES {
        let (q, m) = introspected_fields(&schema, role).await;
        assert!(!q.contains(&"erasureStatus".into()), "erasureStatus leaked to {role:?}");
        for name in MUTATIONS {
            assert!(!m.contains(&name.to_string()), "{name} leaked to {role:?}");
        }
    }

    // 3. TYPE REACHABILITY.
    assert!(
        type_visible(&schema, RequestRole::Customer, "CustomerErasure").await,
        "CustomerErasure must be visible to CUSTOMER"
    );
    for role in OTHER_ROLES {
        assert!(
            !type_visible(&schema, role, "CustomerErasure").await,
            "the CustomerErasure type leaked to {role:?}"
        );
    }
}

/// **`unbound ⇒ denied`, on the money path** — the companion test
/// [ADR-20260818-101500](../../../docs/adr/ADR-20260818-101500-the-restaurant-signs-in-by-email-link-and-638-freezes-at-chunk-1.md)
/// banked at the briefing, and the one it says *"matters more than the obvious one"*: without it,
/// `domain_id: None` gets coded as "unknown ⇒ allow", the cross-tenant test still passes, and the
/// hole is untouched.
///
/// The defect (#639 §4, ADR-20260818-004646 Correction 3): a token asserting
/// `captain_food.role = "RESTAURANT"` with NO `restaurant_id` produced an `Identity::Unbound` whose
/// `role()` returned RESTAURANT, so it satisfied `approveRefund`'s `ALLOW_RESTAURANT_ADMIN` guard —
/// and `approveRefund` resolves its actor from the payload's `orderId`, never from the caller, so
/// that caller could approve ANY pending refund.
///
/// Asserted as a PAIR, never as a lone denial: an `acting_role` that returned PUBLIC for
/// *everything* would pass a one-sided "unbound is refused" test while breaking every real
/// restaurateur. The bound leg proves the guard still admits, by asserting the request gets past
/// FORBIDDEN — it then fails inside the resolver on the absent mailbox, which is exactly the
/// evidence wanted: the guard let it through.
///
/// Both doors are asserted because both read `role_allows`: EXECUTION (the guard) and
/// INTROSPECTION (`visible_restaurant_admin`). A field hidden but callable, or visible but guarded,
/// are both wrong.
#[tokio::test]
async fn an_unbound_restaurant_principal_is_denied_on_the_money_path() {
    let schema = schema();
    // A COMPLETE, valid input: async-graphql validates arguments before it runs the field guard,
    // so a malformed one would fail validation and never reach the ACL — a green test proving
    // nothing about authorization.
    const APPROVE: &str = r#"mutation { approveRefund(input: {
        orderId: "00000000-0000-0000-0000-000000000002",
        amount: { amountCents: 1250, currency: EUR }
    }) { messageId } }"#;

    // BOUND: admitted by the guard. Not "no errors" — the resolver has no mailbox in this schema —
    // but specifically NOT the FORBIDDEN refusal.
    let bound_resp = schema
        .execute(
            Request::new(APPROVE)
                .data(bound(RequestRole::Restaurant).acting_role(RequestRole::Restaurant)),
        )
        .await;
    assert!(
        !bound_resp.errors.iter().any(is_forbidden),
        "a RESTAURANT bound to a restaurant must pass the guard — refund approval stays with the \
         restaurant (ADR-20260818-094500 ruling B): {:?}",
        bound_resp.errors
    );

    // UNBOUND: the same role, the same path, no domain binding. `role_binding(.., None)` is the one
    // honest way to spell it, and it is the identity a hand-stamped `{"role":"RESTAURANT"}` token
    // produces today.
    let unbound = server::Principal::role_binding(
        RequestRole::Restaurant,
        "acl-test-subject".to_string(),
        None,
    );
    assert_eq!(
        unbound.acting_role(RequestRole::Restaurant).get(),
        RequestRole::Public,
        "an unbound caller cannot ACT as RESTAURANT — the witness is the mechanism, not this test"
    );
    let unbound_resp = schema
        .execute(Request::new(APPROVE).data(unbound.acting_role(RequestRole::Restaurant)))
        .await;
    assert!(
        unbound_resp.errors.iter().any(is_forbidden),
        "an authenticated caller with NO restaurant binding must be FORBIDDEN from approveRefund: \
         {:?}",
        unbound_resp.errors
    );

    // INTROSPECTION, the second door onto the same `role_allows`.
    let (_, bound_mutations) = introspected_fields(&schema, RequestRole::Restaurant).await;
    assert!(
        bound_mutations.contains(&"approveRefund".to_string()),
        "approveRefund must stay visible to a bound RESTAURANT: {bound_mutations:?}"
    );
    let hidden = schema
        .execute(
            Request::new("{ __schema { mutationType { fields { name } } } }")
                .data(unbound.acting_role(RequestRole::Restaurant)),
        )
        .await;
    let names = hidden.data.into_json().expect("introspection json")["__schema"]["mutationType"]
        ["fields"]
        .as_array()
        .expect("fields array")
        .iter()
        .map(|f| f["name"].as_str().expect("field name").to_string())
        .collect::<Vec<_>>();
    assert!(
        !names.contains(&"approveRefund".to_string()),
        "approveRefund must not be introspectable by an unbound caller either: {names:?}"
    );
}

/// Every role, one table, because the unbound arm must not have been bought by breaking the others.
///
/// ADMIN and EXTERNAL are the ones that would fail silently and expensively: neither carries a
/// domain claim BY DESIGN (`Identity::Admin` — "its scope IS the role"), so an `acting_role` that
/// keyed on *claim presence* rather than on the identity VARIANT would black them out. `/external`
/// is the Stripe and HubRise webhook path: dark at peak, that is a paid order nobody is told about.
#[test]
fn every_role_acts_as_itself_when_bound_and_as_public_when_not() {
    for role in [
        RequestRole::Public,
        RequestRole::Customer,
        RequestRole::RestaurantAccount,
        RequestRole::Restaurant,
        RequestRole::Rider,
        RequestRole::Admin,
        RequestRole::External,
    ] {
        assert_eq!(
            bound(role).acting_role(role).get(),
            role,
            "{role:?}: a bound caller acts as its own role"
        );
    }

    // Only the four roles that carry a domain binding can be unbound — `role_binding` with `None`
    // for ADMIN / EXTERNAL / PUBLIC yields their own claim-free identities, not `Unbound`, which is
    // why they keep acting as themselves above. RIDER is back in this list (the #849
    // re-presentation): its binding is a Postgres row rather than a claim, but "no binding" means
    // the same thing — nobody — and 2b as first pushed had taken it OUT and asserted the inverse.
    for role in [
        RequestRole::Customer,
        RequestRole::RestaurantAccount,
        RequestRole::Restaurant,
        RequestRole::Rider,
    ] {
        let unbound = server::Principal::role_binding(role, "s".to_string(), None);
        assert_eq!(
            unbound.acting_role(role).get(),
            RequestRole::Public,
            "{role:?}: no binding, no action"
        );
        assert_eq!(
            unbound.recorded_role(),
            RequestRole::Public,
            "{role:?}: and no false author in domain_events.user_type either"
        );
    }

}

/// A scripted `Rider` table for the seam-driven pair below: one fixed outcome for every subject.
struct ScriptedRiderTable(server::RiderIdentityResolution);

#[async_trait::async_trait]
impl server::ResolveRiderIdentity for ScriptedRiderTable {
    async fn resolve(&self, _auth_subject: &str) -> server::RiderIdentityResolution {
        self.0.clone()
    }
}

fn rider_seam(outcome: server::RiderIdentityResolution) -> server::IdentitySources {
    server::IdentitySources {
        customer: server::CustomerIdentitySource::Claim,
        rider: server::RiderIdentitySource::new(std::sync::Arc::new(ScriptedRiderTable(outcome))),
    }
}

/// The RIDER binding is not a claim but a Postgres row, resolved at the request seam (#639 part C
/// step 2b) — so the unbound rider is asserted the way the runtime produces one: a RIDER-role
/// principal DRIVEN THROUGH `resolve_read_scope` over a seam with no row, never a scope injected by
/// hand. The principal the seam hands back acts as PUBLIC and records PUBLIC; the same principal
/// through a seam that answers a row acts as RIDER and records RIDER. Both halves read the ONE
/// outcome (ADR-20260830-191457; ADR-20260818-101500 "unbound => denied").
///
/// The #849 re-presentation: as first pushed, this file asserted the inverse — a no-row rider
/// "ACTS as RIDER and RECORDS RIDER" — because the runtime minted the witness before the seam ran
/// and the test had been rewritten to match it.
#[tokio::test]
async fn an_unbound_rider_acts_and_records_as_public_through_the_seam() {
    let sub = "rider-subject-with-no-row";
    let correlation = server::graphql_session::RequestCorrelationId(uuid::Uuid::from_u128(0x2B));

    // No row: nobody, on both halves.
    let token_only = server::Principal::role_binding(RequestRole::Rider, sub.to_string(), None);
    let (unbound, scope) = server::resolve_read_scope(
        token_only,
        correlation,
        &rider_seam(server::RiderIdentityResolution::NoMapping),
    )
    .await;
    assert_eq!(scope, application::queries::ReadScope::Public, "no row: reads as nobody");
    assert_eq!(
        unbound.acting_role(RequestRole::Rider).get(),
        RequestRole::Public,
        "no row: acts as nobody — the RIDER guards refuse"
    );
    assert_eq!(
        unbound.recorded_role(),
        RequestRole::Public,
        "no row: recorded as nobody — no false RIDER author in domain_events.user_type"
    );
    assert_eq!(unbound.user_id(), Some(sub), "the auth subject stays on the envelope, honestly");

    // The seam could not answer: identical at this boundary (PAGE vs OBSERVE is telemetry's).
    let token_only = server::Principal::role_binding(RequestRole::Rider, sub.to_string(), None);
    let (failed, scope) = server::resolve_read_scope(
        token_only,
        correlation,
        &rider_seam(server::RiderIdentityResolution::LookupFailed(
            server::LookupFailureReason::Repository,
        )),
    )
    .await;
    assert_eq!(scope, application::queries::ReadScope::Public);
    assert_eq!(failed.acting_role(RequestRole::Rider).get(), RequestRole::Public);
    assert_eq!(failed.recorded_role(), RequestRole::Public);

    // A row: the SAME token, and now a rider on both halves — the pair that keeps the refusals
    // above honest.
    let rider_id = domain::generated::scalars::RiderId(uuid::Uuid::from_u128(0x600D));
    let token_only = server::Principal::role_binding(RequestRole::Rider, sub.to_string(), None);
    let (bound, scope) = server::resolve_read_scope(
        token_only,
        correlation,
        &rider_seam(server::RiderIdentityResolution::Resolved(rider_id)),
    )
    .await;
    assert_eq!(scope, application::queries::ReadScope::Rider(rider_id), "a row: reads that rider");
    assert_eq!(bound.acting_role(RequestRole::Rider).get(), RequestRole::Rider, "a row: acts as RIDER");
    assert_eq!(bound.recorded_role(), RequestRole::Rider, "a row: recorded as RIDER");
    // And the /public rule still holds for a resolved rider: the ACL runs against the PATH.
    assert_eq!(bound.acting_role(RequestRole::Public).get(), RequestRole::Public);
}

/// The `/public` rule the ACL depends on: an identified customer on the OPEN path (#469 — the
/// storefront IS the open path) is evaluated as PUBLIC, so no `roles: [CUSTOMER]` operation becomes
/// reachable, or introspectable, from the one path anyone can reach. This is why `acting_role`
/// takes the PATH role instead of deriving one from the identity.
#[test]
fn an_identified_customer_on_the_public_path_still_acts_as_public() {
    let customer = server::Principal::role_binding(
        RequestRole::Customer,
        "s".to_string(),
        Some(uuid::Uuid::from_u128(0x469)),
    );
    assert_eq!(
        customer.acting_role(RequestRole::Public).get(),
        RequestRole::Public,
        "the ACL runs against the PATH, so the storefront's own path widens nothing"
    );
    assert_eq!(
        customer.recorded_role(),
        RequestRole::Customer,
        "but the ENVELOPE records who they are — a storefront order is authored by the customer, \
         not by an anonymous visitor"
    );
}

/// The issue doors of #639 part C step 3-i (ADR-20260904-015903 §Decision 5), on the literal-roles
/// axis: `reportDeliveryIssue` is [RIDER, ADMIN] (the reporter, or ops on their behalf),
/// `resolveDeliveryIssue` is [RESTAURANT, RESTAURANT_ACCOUNT, ADMIN] (whoever is TOLD acts — the
/// reporter never closes their own issue), `declineDelivery` is [RIDER]. The sets start narrow on
/// purpose: widening later is additive, narrowing is a break.
///
/// Every input is COMPLETE so the guard — not argument validation — is what answers (a malformed
/// input fails before the ACL and proves nothing).
#[tokio::test]
async fn the_issue_doors_admit_exactly_their_listed_paths() {
    let schema = schema();
    // #865: `riderId` carries no field on either Input type any more (`derived: { riderId: rider
    // }` on both) — a literal that still supplied it would fail GraphQL's OWN static validation
    // for every role uniformly (no `extensions.code` at all), which `is_forbidden` cannot tell
    // apart from the role guard's own refusal (the exact "expected-red" trap #865 records).
    const REPORT: &str = r#"mutation { reportDeliveryIssue(input: {
        deliveryJobId: "00000000-0000-0000-0000-00000000000d",
        kind: CUSTOMER_UNREACHABLE
    }) { messageId } }"#;
    const RESOLVE: &str = r#"mutation { resolveDeliveryIssue(input: {
        deliveryJobId: "00000000-0000-0000-0000-00000000000d",
        resolution: REASSIGNED
    }) { messageId } }"#;
    const DECLINE: &str = r#"mutation { declineDelivery(input: {
        deliveryJobId: "00000000-0000-0000-0000-00000000000d"
    }) { messageId } }"#;

    let forbidden = |name: &'static str, query: &'static str, roles: Vec<RequestRole>| {
        let schema = schema.clone();
        async move {
            for role in roles {
                let resp = execute_as(&schema, role, query).await;
                assert_eq!(resp.errors.len(), 1, "{name}: expected one error for {role:?}: {:?}", resp.errors);
                assert!(is_forbidden(&resp.errors[0]), "{name} must be FORBIDDEN for {role:?}: {:?}", resp.errors[0]);
            }
        }
    };
    let admitted = |name: &'static str, query: &'static str, roles: Vec<RequestRole>| {
        let schema = schema.clone();
        async move {
            for role in roles {
                let resp = execute_as(&schema, role, query).await;
                // Past the guard, SOMETHING fails: the mailbox this schema does not carry, OR —
                // for a `derived:` REQUIRED property (`declineDelivery`'s `riderId`, #865) with no
                // `ReadScope` in this schema-only context — the seam's OWN `errors.yaml#/Forbidden`
                // (`Forbidden`, distinctly-coded from the role guard's `FORBIDDEN`). Either way the
                // ONE thing this proves is that the guard admitted the call: `is_forbidden` checks
                // the role guard's own literal code, never the derived seam's.
                assert!(!resp.errors.is_empty(), "{name}: expected an error past the guard for {role:?}");
                assert!(!is_forbidden(&resp.errors[0]), "{name} must pass the guard for {role:?}: {:?}", resp.errors[0]);
            }
        }
    };

    forbidden("reportDeliveryIssue", REPORT, vec![RequestRole::Public, RequestRole::Customer, RequestRole::Restaurant]).await;
    admitted("reportDeliveryIssue", REPORT, vec![RequestRole::Rider, RequestRole::Admin]).await;

    forbidden("resolveDeliveryIssue", RESOLVE, vec![RequestRole::Rider, RequestRole::Public, RequestRole::Customer]).await;
    admitted("resolveDeliveryIssue", RESOLVE, vec![RequestRole::Restaurant, RequestRole::RestaurantAccount, RequestRole::Admin]).await;

    forbidden(
        "declineDelivery",
        DECLINE,
        vec![RequestRole::Public, RequestRole::Customer, RequestRole::Restaurant, RequestRole::RestaurantAccount, RequestRole::Admin, RequestRole::External],
    )
    .await;
    admitted("declineDelivery", DECLINE, vec![RequestRole::Rider]).await;

    // Introspection follows: the resolve door is not even NAMED on the rider's schema, and the
    // rider-only doors are absent from the restaurant's.
    let (_q, rider_m) = introspected_fields(&schema, RequestRole::Rider).await;
    assert!(rider_m.contains(&"reportDeliveryIssue".into()), "reportDeliveryIssue missing for RIDER: {rider_m:?}");
    assert!(rider_m.contains(&"declineDelivery".into()), "declineDelivery missing for RIDER: {rider_m:?}");
    assert!(!rider_m.contains(&"resolveDeliveryIssue".into()), "resolveDeliveryIssue leaked to RIDER");
    let (_q, rest_m) = introspected_fields(&schema, RequestRole::Restaurant).await;
    assert!(rest_m.contains(&"resolveDeliveryIssue".into()), "resolveDeliveryIssue missing for RESTAURANT: {rest_m:?}");
    assert!(!rest_m.contains(&"reportDeliveryIssue".into()), "reportDeliveryIssue leaked to RESTAURANT");
    assert!(!rest_m.contains(&"declineDelivery".into()), "declineDelivery leaked to RESTAURANT");
}
