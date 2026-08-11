//! GraphQL BFF (ADR-0006 "role = path"). The SDL is generated from `api.yaml`; here we host it with
//! async-graphql. Stage 1a: the generated type layer (`generated/` — wrapper scalars, output/input
//! types, QueryRoot) backs the schema; the real read resolvers land next.

pub mod acl;
/// The cart READ seam (#451): two-leg `current` lookup, by-id ownership narrowing, and the
/// one `price_cart` path every cart resolver maps through — hand-written and unit-tested;
/// the generated resolver literals only call it.
pub mod cart_read;
pub mod generated;
pub mod routes;
pub mod schema;
/// The subgraph scope slice (#385 API-tier wiring, D8): a `graphql-{scope}` bin serves the
/// master schema restricted to its own scope's operations via the generated composition table.
pub mod scope_slice;
pub mod session;
/// The request's TENANT (#469): `Host` -> `{slug}` -> `RestaurantId`, resolved ONCE at the
/// transport boundary and injected beside the `ReadScope`. Multi-tenancy by host reached the SSR
/// page renderer and nothing else until this module existed.
pub mod tenant;

/// The Runtime D1 flip gate (`PM_MAILBOX_DELIVERY`, #272 / ADR-20260801-023000), injected as
/// schema data: the generated process-manager resolvers (placeOrder / approveRefund / denyRefund)
/// carry BOTH arms and read this at request time — mailbox delivery through the prepare phase
/// when true, the legacy journal+spawn path when false (gate-then-stabilize; the default flip is
/// its own recorded decision).
#[derive(Clone, Copy, Debug)]
pub struct PmMailboxDelivery(pub bool);
