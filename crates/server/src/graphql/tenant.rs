//! The request's TENANT, resolved from the `Host` at the transport boundary (#469).
//!
//! Captain.Food is multi-tenant by host (`{slug}.captain.food`, ADR-0036), and until #469 that fact
//! reached the SSR page renderer and **nothing else**: the GraphQL edge injected role, principal,
//! session, trace and `ReadScope`, but never the tenant. `cart.current`'s claim leg therefore
//! resolved the claim-holder's newest OPEN cart *anywhere* — so a customer with an open cart at
//! `b.captain.food` would have seen it, been priced for it, and paid for it on `a.captain.food`.
//!
//! Three properties this module exists to hold:
//!
//! 1. **The tenant is an AUTHORIZATION input, so it comes from the `Host` and never from a client
//!    argument.** `current` stays zero-argument on purpose: an argument — even an optional one —
//!    would let a client assert which restaurant's cart it wants, and "unbounded when omitted" is
//!    today's bug with a nicer name. The legitimate "my carts elsewhere" surface already exists as
//!    a different query (`carts`), which is what additive growth looks like here.
//! 2. **It is its OWN datum, injected beside [`application::queries::ReadScope`], never folded into
//!    it.** Different provenance and lifetime: `ReadScope` is a pure function of verified claims,
//!    the tenant is a function of the `Host` and is legitimately absent on the marketplace host.
//!    Folding them would force every existing `match scope` arm to re-handle a product, and make
//!    ADMIN/SYSTEM carry a tenant they must ignore.
//! 3. **Absent ⇒ no tenant rows, fail closed.** No host tenant (the marketplace, `api.`, localhost,
//!    an unknown slug) and any lookup failure both yield [`TenantScope::None`], and a tenant-scoped
//!    read serves `null` rather than falling back to "newest anywhere".

use axum::http::{header, HeaderMap};
use domain::generated::scalars::{RestaurantId, Slug};

use crate::hosts::{classify_host, HostRoute, TenantLookup};

/// The restaurant this request is addressed to, resolved from its `Host`.
///
/// `None` is not "unknown, carry on" — it is "this request names no tenant", and every read scoped
/// by it must serve nothing rather than serving everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantScope {
    /// The request's host names no restaurant: the marketplace (`live.`/apex), `api.`, an internal
    /// or dev host, an unclaimed/unknown slug, or a tenant lookup that could not be completed.
    None,
    /// The request's host resolves to this registered restaurant.
    Restaurant(RestaurantId),
}

/// The request's effective host string: `X-Forwarded-Host` (what the ingress saw) with `Host` as
/// the fallback — the SAME chain the SSR fallback uses (`hosts::host_root`), deliberately, because
/// a page rendered for one tenant whose GraphQL reads resolved another tenant is precisely the
/// class of bug this module exists to close.
pub fn raw_host(headers: &HeaderMap) -> &str {
    headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

/// Resolve the request's tenant: `Host` → `{slug}` → the registered restaurant's id.
///
/// **Fails CLOSED, unlike the SSR fallback.** `hosts::tenant_page` fails OPEN on a lookup error (a
/// DB hiccup must never show "this address is available" for a real restaurant) — that is a
/// RENDERING decision about a shell with no data in it. Scope is the opposite: a request whose
/// tenant cannot be established must read NOTHING tenant-scoped, because the alternative is reading
/// everything. A storefront whose cart is empty during a DB blip is a degrade; a storefront showing
/// another restaurant's cart is the incident.
///
/// Costs one indexed `by_slug` read, and only on a tenant host — the marketplace, `api.` and dev
/// hosts short-circuit before touching the database.
pub async fn resolve_tenant(headers: &HeaderMap, lookup: &TenantLookup) -> TenantScope {
    let HostRoute::Tenant(slug) = classify_host(raw_host(headers)) else {
        return TenantScope::None;
    };
    let Some(repo) = lookup.0.as_ref() else {
        // No database configured (dev): there is no registered restaurant to resolve, so there is
        // no tenant — not "every tenant".
        return TenantScope::None;
    };
    match repo.by_slug(Slug(slug)).await {
        Ok(Some(row)) => TenantScope::Restaurant(row.restaurant_id),
        // Unclaimed slug, superseded label (the 301 puts the browser on the current host before it
        // ever POSTs), or a read-model failure: no tenant.
        Ok(Option::None) | Err(_) => TenantScope::None,
    }
}

#[cfg(test)]
mod tests {
    use application::queries::{RestaurantFilter, RestaurantReadRepository, RestaurantRow};
    use async_trait::async_trait;
    use domain::shared::errors::DomainError;
    use std::sync::Arc;

    use super::*;

    fn rid(n: u8) -> RestaurantId {
        RestaurantId(uuid::Uuid::from_u128(n as u128))
    }

    /// One registered slug → one restaurant id; `erroring` stands in for a read-model outage.
    struct Stub {
        registered: &'static str,
        id: RestaurantId,
        erroring: bool,
    }

    fn row(slug: &str, id: RestaurantId) -> RestaurantRow {
        serde_json::from_value(serde_json::json!({
            "restaurant_id": id.0, "restaurant_account_id": null,
            "listing_status": "ACTIVE_PARTNER", "external_identifiers": null, "google_place_id": null,
            "slug": slug, "display_name": "Chez Test", "description": null,
            "tags": null, "margin_rate": null, "cuisine_category": null,
            "uber_prices_opt_in": null, "website": null, "rating": null, "reviews_count": null,
            "gbp_order_url": null, "gbp_link_status": null,
            "address": {}, "location": null, "opening_hours": {},
            "status": "ACTIVE", "order_acceptance": "NORMAL", "default_currency": "EUR",
            "timezone": null, "preparation_time_minutes": null,
            "created_at": "2026-08-11T00:00:00Z", "updated_at": "2026-08-11T00:00:00Z",
        }))
        .expect("stub row deserializes")
    }

    #[async_trait]
    impl RestaurantReadRepository for Stub {
        async fn list(&self, _f: RestaurantFilter) -> Result<Vec<RestaurantRow>, DomainError> {
            Ok(vec![])
        }
        async fn by_slug(&self, slug: Slug) -> Result<Option<RestaurantRow>, DomainError> {
            if self.erroring {
                return Err(DomainError::Repository("read model down".into()));
            }
            Ok((slug.0 == self.registered).then(|| row(&slug.0, self.id)))
        }
        async fn by_id(&self, _id: RestaurantId) -> Result<Option<RestaurantRow>, DomainError> {
            Ok(None)
        }
        async fn by_previous_slug(&self, _s: Slug) -> Result<Option<RestaurantRow>, DomainError> {
            Ok(None)
        }
    }

    fn lookup(erroring: bool) -> TenantLookup {
        TenantLookup(Some(Arc::new(Stub { registered: "chez-test", id: rid(7), erroring })))
    }

    fn headers(host: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, host.parse().unwrap());
        h
    }

    #[tokio::test]
    async fn a_registered_tenant_host_resolves_its_restaurant() {
        assert_eq!(
            resolve_tenant(&headers("chez-test.captain.food"), &lookup(false)).await,
            TenantScope::Restaurant(rid(7))
        );
    }

    /// The ingress header wins over `Host`, matching the SSR fallback's chain — otherwise the page
    /// and its data could resolve two different tenants behind a proxy.
    #[tokio::test]
    async fn the_forwarded_host_wins_like_the_ssr_fallback() {
        let mut h = headers("internal-svc.cluster.local");
        h.insert("x-forwarded-host", "chez-test.captain.food".parse().unwrap());
        assert_eq!(resolve_tenant(&h, &lookup(false)).await, TenantScope::Restaurant(rid(7)));
    }

    /// Every non-tenant host is `None` — including the MARKETPLACE, decided explicitly (#469, API
    /// lens): on `live.captain.food` a tenant-scoped read serves null, never "newest anywhere".
    #[tokio::test]
    async fn non_tenant_hosts_carry_no_tenant() {
        for host in [
            "live.captain.food",
            "captain.food",
            "api.captain.food",
            "restos.captain.food",
            "riders.captain.food",
            "localhost:8080",
            "captain-food.onrender.com",
            "",
        ] {
            assert_eq!(
                resolve_tenant(&headers(host), &lookup(false)).await,
                TenantScope::None,
                "{host}"
            );
        }
    }

    /// Fail CLOSED where the renderer fails open: an unknown slug and a read-model outage both
    /// resolve NO tenant, so a scoped read serves nothing rather than everything.
    #[tokio::test]
    async fn an_unknown_slug_or_a_failed_lookup_resolves_no_tenant() {
        assert_eq!(
            resolve_tenant(&headers("nobody.captain.food"), &lookup(false)).await,
            TenantScope::None
        );
        assert_eq!(
            resolve_tenant(&headers("chez-test.captain.food"), &lookup(true)).await,
            TenantScope::None,
            "a read-model failure must not widen the scope to every tenant"
        );
        assert_eq!(
            resolve_tenant(&headers("chez-test.captain.food"), &TenantLookup(None)).await,
            TenantScope::None,
            "no database configured is no tenant, not every tenant"
        );
    }
}
