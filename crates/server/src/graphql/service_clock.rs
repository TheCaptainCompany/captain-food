//! The request clock for service-window evaluation (RSO-1, DECISIONS §43).
//!
//! `Restaurant.serviceWindow` promises `evaluatedAt` is "the request clock, read ONCE per
//! request, never per row" (specs/network/api.yaml). This module is that seam: [`RequestNow`] is
//! MINTED at the bounded transport boundaries — the HTTP POST handler and the SSR transport —
//! exactly where `RequestCorrelationId` is, and injected into the GraphQL context; the generated
//! resolvers read it back through [`evaluation`] and thread it as a plain parameter (the
//! `sms_guard.rs` "now is a parameter" precedent — deliberately NO `Clock` trait: a port would
//! let a resolver re-read the clock per row, which is the defect this type exists to make
//! unspellable). The WS transport deliberately injects NOTHING: a socket lives for hours, so a
//! connection-scoped clock would go stale — there "the request" is each operation, and
//! [`evaluation`]'s per-execution fallback read is the correct clock.
//!
//! [`ServiceWindowHorizon`] is the validity horizon (`SERVICE_WINDOW_VALIDITY_HORIZON_SECONDS`,
//! specs/network/configuration.yaml) — configuration, read once at the composition root and
//! registered as schema data beside the read repositories.

/// The ONE instant this request evaluates every service window at. Minted at the transport
/// boundary; a request that builds fifty restaurant cards gets fifty verdicts that agree on
/// "now".
#[derive(Clone, Copy, Debug)]
pub struct RequestNow(pub chrono::DateTime<chrono::Utc>);

impl RequestNow {
    /// Read the system clock — the ONLY place the read path does, once per request.
    pub fn mint() -> Self {
        Self(chrono::Utc::now())
    }
}

/// The service-window validity horizon (`SERVICE_WINDOW_VALIDITY_HORIZON_SECONDS`): how long a
/// client may trust a verdict when no window transition caps it sooner.
#[derive(Clone, Copy, Debug)]
pub struct ServiceWindowHorizon(pub chrono::Duration);

impl ServiceWindowHorizon {
    /// From the configured seconds (the generated config surface's
    /// `service_window_validity_horizon_seconds`).
    pub fn from_seconds(seconds: i64) -> Self {
        Self(chrono::Duration::seconds(seconds))
    }
}

impl Default for ServiceWindowHorizon {
    /// The spec default (specs/network/configuration.yaml: 900). Used by schemas built without
    /// deps (`build_schema(None, ..)` — tests, GraphiQL-only surfaces); the composition root
    /// always passes the configured value.
    fn default() -> Self {
        Self::from_seconds(900)
    }
}

/// The (now, horizon) pair a resolver threads into `Restaurant::at`.
///
/// An ABSENT `RequestNow` means either the WS transport (where per-operation IS the request —
/// see the module doc) or a schema executed outside any transport (a direct unit-test
/// execution); both resolve to a single clock read HERE — one per execution, never per row. An
/// absent horizon (schema built without deps) is the spec default.
pub fn evaluation(ctx: &async_graphql::Context<'_>) -> (chrono::DateTime<chrono::Utc>, chrono::Duration) {
    let now = ctx.data_opt::<RequestNow>().map(|n| n.0).unwrap_or_else(chrono::Utc::now);
    let horizon = ctx.data_opt::<ServiceWindowHorizon>().copied().unwrap_or_default().0;
    (now, horizon)
}
