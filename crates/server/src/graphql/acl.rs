//! Role-as-path ACL (ADR-0006). The role is parsed from the URL path and injected into the GraphQL
//! request context by `routes.rs`; the generated `generated/acl.rs` derives every operation's
//! allowed-role set from api.yaml `roles` and wires it onto the generated QueryRoot/MutationRoot fields
//! as `guard = "RoleGuard::new(ALLOW_…)"` (execution — unauthorized roles get FORBIDDEN) and
//! `visible = "visible_…"` (introspection — the field is hidden, and async-graphql's
//! `find_visible_types` then hides every type reachable only through hidden fields, so per-role
//! introspection/Voyager expose only that role's surface). This module is the hand-written seam those
//! generated bindings call into: the role type, its lookup, and the guard.

use async_graphql::{Context, ErrorExtensions, Guard, Result};

/// One of the seven request roles, each served under `/{segment}/graphql`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestRole {
    Public,
    Customer,
    RestaurantAccount,
    Restaurant,
    Rider,
    Admin,
    External,
}

impl RequestRole {
    /// Map a URL path segment (`"public"`, `"restaurant-account"`, …) to a role.
    pub fn from_segment(seg: &str) -> Option<Self> {
        Some(match seg {
            "public" => RequestRole::Public,
            "customer" => RequestRole::Customer,
            "restaurant-account" => RequestRole::RestaurantAccount,
            "restaurant" => RequestRole::Restaurant,
            "rider" => RequestRole::Rider,
            "admin" => RequestRole::Admin,
            "external" => RequestRole::External,
            _ => return None,
        })
    }

    /// The URL path segment for this role.
    pub fn segment(self) -> &'static str {
        match self {
            RequestRole::Public => "public",
            RequestRole::Customer => "customer",
            RequestRole::RestaurantAccount => "restaurant-account",
            RequestRole::Restaurant => "restaurant",
            RequestRole::Rider => "rider",
            RequestRole::Admin => "admin",
            RequestRole::External => "external",
        }
    }

    /// The api.yaml role name (a `scalars.yaml#/UserType` value), as used in operations' `roles:` lists.
    pub fn api_name(self) -> &'static str {
        match self {
            RequestRole::Public => "PUBLIC",
            RequestRole::Customer => "CUSTOMER",
            RequestRole::RestaurantAccount => "RESTAURANT_ACCOUNT",
            RequestRole::Restaurant => "RESTAURANT",
            RequestRole::Rider => "RIDER",
            RequestRole::Admin => "ADMIN",
            RequestRole::External => "EXTERNAL",
        }
    }
}

/// The request's role, as injected by `routes.rs` from the URL path. A context without a role (direct
/// schema execution outside the HTTP surface, e.g. tests) fails CLOSED to the unauthenticated PUBLIC
/// surface.
pub fn request_role(ctx: &Context<'_>) -> RequestRole {
    ctx.data_opt::<RequestRole>().copied().unwrap_or(RequestRole::Public)
}

/// True when `allowed` (an operation's api.yaml `roles`) admits the request's role. The list is
/// LITERAL (ADR-20260720-191500): `RequestRole::Public` in it admits only the anonymous PUBLIC path
/// — an operation open to every role carries no guard at all (roles omitted in the spec).
pub fn role_allows(ctx: &Context<'_>, allowed: &[RequestRole]) -> bool {
    allowed.contains(&request_role(ctx))
}

/// Execution guard on the generated QueryRoot/MutationRoot fields: rejects the request with a
/// `FORBIDDEN` error (extension `code`) when the path role is not in the operation's allowed set.
/// This is PATH-role authorization (ADR-0006) — identity/authentication is a separate workstream.
pub struct RoleGuard {
    allowed: &'static [RequestRole],
}

impl RoleGuard {
    pub fn new(allowed: &'static [RequestRole]) -> Self {
        Self { allowed }
    }
}

impl Guard for RoleGuard {
    async fn check(&self, ctx: &Context<'_>) -> Result<()> {
        if role_allows(ctx, self.allowed) {
            return Ok(());
        }
        let allowed: Vec<&str> = self.allowed.iter().map(|r| r.api_name()).collect();
        Err(async_graphql::Error::new(format!(
            "forbidden: role {} is not authorized for this operation (allowed: {})",
            request_role(ctx).api_name(),
            allowed.join(", ")
        ))
        .extend_with(|_, e| e.set("code", "FORBIDDEN")))
    }
}
