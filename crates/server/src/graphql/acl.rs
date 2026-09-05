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

/// The role this request may ACT as — the single role value in the GraphQL context, and the input
/// to every guard, every `visible_*` and the `command.receive` span.
///
/// It reads an [`ActingRole`], **not** a `RequestRole` (#639 part B). That is the whole fix: a
/// `RequestRole` is a plain public enum that any code can mint from the URL path, and `routes.rs`
/// did exactly that — so a token asserting RESTAURANT with no `restaurant_id`
/// ([`crate::auth::Identity::Unbound`]) satisfied `approveRefund`'s guard and could approve any
/// pending refund. An `ActingRole` can only come from
/// [`Principal::acting_role`](crate::auth::Principal::acting_role), whose unbound arm yields
/// PUBLIC, so the privileged value cannot reach this function for such a caller.
///
/// A context with no `ActingRole` (direct schema execution outside the HTTP surface) fails CLOSED
/// to the unauthenticated PUBLIC surface. The transports cannot forget to inject one: it is
/// returned by `routes::authorize_and_resolve_scope`, which both destructure, so dropping it is a
/// compile error rather than a silent 403.
pub fn request_role(ctx: &Context<'_>) -> RequestRole {
    ctx.data_opt::<crate::auth::ActingRole>()
        .copied()
        .map(crate::auth::ActingRole::get)
        .unwrap_or(RequestRole::Public)
}

/// True when `allowed` (an operation's api.yaml `roles`) admits the role the request may ACT as.
/// The list is LITERAL (ADR-20260720-191500): `RequestRole::Public` in it admits only the anonymous
/// PUBLIC path — an operation open to every role carries no guard at all (roles omitted in the
/// spec).
///
/// Shared by the execution guard and the generated `visible_*` introspection predicates ON PURPOSE:
/// a field hidden but callable, or visible but guarded, are both wrong, so one seam answers both
/// doors and one change moves them together.
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

/// The standing carve-out guard (#639 part C step 4-i, ADR-20260904-081527 §4/§9): a SECOND,
/// orthogonal question from `RoleGuard` — chained `.and(..)` on every role-guarded operation, an
/// empty carve set when `whileRestricted:` is absent (fail-closed by absence lives in the
/// generated emitter, never here). Reads `ctx.data_opt::<ReadScope>()` ONLY — never a claim (a
/// claim has no standing) — so a guard that ignored `standing` would not compile
/// (compiler-first, ADR-20260803-234035: `ReadScope::Rider` is a struct variant carrying it).
/// Admits: any non-`ReadScope::Rider` scope (nothing to restrict — Public/Customer/Restaurant/
/// RestaurantAccount/Admin/System all pass through untouched), a `Rider` scope with `standing ==
/// ACTIVE`, or a `Rider` scope whose carve set contains `RIDER`. Denied on the RESTRICTED,
/// not-carved path: increments `rider_restricted_denied_total{operation}` (the FIRST guard-level
/// emitter — a plain FORBIDDEN emits nothing today) and logs an INFO trace event carrying
/// `rider_id`/`correlation_id` beside it (the #748 skip-trace pattern — no `rider_id` label on the
/// counter itself).
pub struct StandingGuard {
    carve: &'static [RequestRole],
    operation: &'static str,
}

impl StandingGuard {
    pub fn new(carve: &'static [RequestRole], operation: &'static str) -> Self {
        Self { carve, operation }
    }
}

impl Guard for StandingGuard {
    async fn check(&self, ctx: &Context<'_>) -> Result<()> {
        use application::queries::ReadScope;
        let scope = ctx.data_opt::<ReadScope>();
        let ReadScope::Rider { id, standing: scope_standing } = scope.unwrap_or(&ReadScope::Public) else {
            return Ok(());
        };
        // #639 part C step 5 (ADR-20260905-065415 §2): the connection-local standing cell, when
        // one is present (RIDER, gate ON), reads FIRST — per-yield freshness with zero I/O,
        // push-fed by the socket watcher; `ReadScope` is the fallback (gate OFF, or a transport
        // with no cell at all, e.g. a plain HTTP request). ONE emitted place closes queries,
        // mutations and subscriptions alike. Round 2 R2-5: keyed by the [`RiderStandingWatch`]
        // newtype, never the bare `watch::Receiver<RiderStanding>` — the SAME TypeId-collision
        // reason the sender half (`RiderStandingCell`) already had one.
        let standing = ctx
            .data_opt::<super::rider_socket::RiderStandingWatch>()
            .map(|cell| *cell.0.borrow())
            .unwrap_or(*scope_standing);
        if standing == domain::generated::scalars::RiderStanding::ACTIVE {
            return Ok(());
        }
        if self.carve.contains(&RequestRole::Rider) {
            return Ok(());
        }
        telemetry::meters::rider_restriction::denied(self.operation);
        // Round 3 item 2 (obs, reviewer): the NIL uuid, not an empty string, for an absent
        // correlation id — the `generated/query.rs` shape (#451) — because `business.correlation_id`
        // is `required: true`: an empty string is a value that satisfies the presence check while
        // meaning nothing, the nil uuid says exactly "no request context" the same way it does on
        // every other span in this file's family.
        let correlation_id = ctx
            .data_opt::<crate::graphql::session::RequestCorrelationId>()
            .map(|c| c.0)
            .unwrap_or(uuid::Uuid::nil())
            .to_string();
        // Round 2 item 6(a): a REAL span now (never just a bare event) — the `rider-restriction`
        // contract's declared `business.operation`/`business.correlation_id` attributes are
        // genuinely populated here, and `rider_id` rides the #748 skip-trace pattern as a nested
        // INFO event (deliberately off the span's own structured attributes, matching the
        // counter's own no-rider_id-label posture).
        let span = telemetry::spans::rider_standing_denied(self.operation, &correlation_id);
        span.in_scope(|| {
            tracing::info!(rider_id = %id.0, "rider.standing.denied");
        });
        // #639 part C step 4-ii (ADR-20260904-124600 §1): the ADDITIVE discriminator beside the
        // UNCHANGED `code: FORBIDDEN` — the shared `shared_types::RIDER_RESTRICTED` constant, never
        // a hand-copied literal, so the client's bounce decision (`crates/web/src/bounce.rs`) and
        // this guard can never drift. `RoleGuard::check` sets no `reason` — that asymmetry is what
        // lets the client key its bounce on the SERVER's own signal instead of on a bare FORBIDDEN.
        Err(async_graphql::Error::new(
            "forbidden: your access is restricted".to_string(),
        )
        .extend_with(|_, e| {
            e.set("code", "FORBIDDEN");
            e.set("reason", shared_types::RIDER_RESTRICTED);
        }))
    }
}

#[cfg(test)]
mod standing_guard_cell_tests {
    use super::*;
    use application::queries::ReadScope;
    use domain::generated::scalars::{RiderId, RiderStanding};

    fn acting(role: RequestRole) -> crate::auth::ActingRole {
        crate::auth::Principal::role_binding(role, "test-subject".to_string(), Some(uuid::Uuid::from_u128(0x0505)))
            .acting_role(role)
    }

    /// #639 part C step 5 (ADR-20260905-065415 §2), the mutant M4 the checkpoint requires red
    /// ("the standing cell read once at connect instead of live"): `ReadScope::Rider.standing`
    /// stays ACTIVE for the connection's WHOLE life (exactly as the real WS transport freezes it
    /// at `connection_init`) while the connection-local cell — the SAME `RiderStandingCell` type
    /// `rider_socket::watch` uses — flips to RESTRICTED mid-connection. `StandingGuard` must read
    /// the CELL first: the guard admits `acceptDelivery` before `restrict()`, and refuses it after,
    /// on the IDENTICAL frozen `ReadScope`. Deterministic and race-free (unlike a live WS/watcher
    /// integration test, which the watcher — an in-process broadcast wakeup — reliably wins against
    /// any real network round trip, so it cannot observe this property reliably; that end-to-end
    /// race is documented in `rider_restriction_closes_the_socket.rs`).
    #[tokio::test]
    async fn the_cell_refuses_before_read_scope_ever_changes() {
        let schema = crate::graphql::schema::build_schema(None, None, None);
        let rider_id = RiderId(uuid::Uuid::from_u128(0x6394_5A));
        // Frozen for the WHOLE test — never mutated, exactly like the real WS connection's
        // `ReadScope::Rider` copy baked into `Data` at `connection_init`.
        let frozen_active = ReadScope::Rider { id: rider_id, standing: RiderStanding::ACTIVE };
        let (cell, standing_rx) = super::super::rider_socket::RiderStandingCell::seeded(RiderStanding::ACTIVE);

        let accept = || {
            format!(
                r#"mutation {{ acceptDelivery(input: {{ deliveryJobId: "{}" }}) {{ messageId }} }}"#,
                uuid::Uuid::new_v4()
            )
        };

        // BEFORE `restrict()`: admitted (whatever fails past the guard is unrelated to standing).
        let resp = schema
            .execute(
                async_graphql::Request::new(accept())
                    .data(acting(RequestRole::Rider))
                    .data(frozen_active.clone())
                    .data(standing_rx.clone()),
            )
            .await;
        let denied_before = resp.errors.iter().any(|e| e.extensions.as_ref().is_some_and(|ext| {
            ext.get("reason").is_some_and(|r| r.to_string().contains("RIDER_RESTRICTED"))
        }));
        assert!(!denied_before, "ACTIVE cell must admit: {:?}", resp.errors);

        // The fact lands on the CELL — `ReadScope` (`frozen_active`) is NEVER touched again.
        cell.restrict();

        // AFTER `restrict()`, same frozen ReadScope: refused. Under mutant M4 (the guard reading
        // `ctx.data_opt::<ReadScope>()`'s stale copy, or ignoring the cell entirely) this assertion
        // reds, because `frozen_active.standing` is still, and forever, `ACTIVE`.
        let resp = schema
            .execute(
                async_graphql::Request::new(accept())
                    .data(acting(RequestRole::Rider))
                    .data(frozen_active)
                    .data(standing_rx),
            )
            .await;
        let denied_after = resp.errors.iter().any(|e| {
            e.extensions.as_ref().is_some_and(|ext| ext.get("reason").is_some_and(|r| r.to_string().contains("RIDER_RESTRICTED")))
        });
        assert!(denied_after, "the CELL must refuse even though ReadScope stayed ACTIVE: {:?}", resp.errors);
    }
}
